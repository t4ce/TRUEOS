extern crate alloc;

use alloc::format;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use spin::Mutex;

use super::xelp_media_mp4::{
    ParsedPps, ParsedSps, build_annex_b_access_unit, first_sample_nal_types,
    parse_h264_mp4_summary, parse_pps, parse_sps,
};

const MAX_MEDIA_ENGINES: usize = 4;
const MAX_MEDIA_API_ROUTES: usize = 4;
const MAX_MEDIA_RESULT_SLOTS: usize = 4;

const FORCEWAKE_MEDIA_GEN11: usize = 0x0A184;
const FORCEWAKE_MEDIA_VDBOX0: usize = 0x0A540;
const FORCEWAKE_MEDIA_VDBOX1: usize = 0x0A544;
const FORCEWAKE_MEDIA_VDBOX2: usize = 0x0A548;
const FORCEWAKE_MEDIA_VDBOX3: usize = 0x0A54C;
const FORCEWAKE_MEDIA_VEBOX0: usize = 0x0A560;
const FORCEWAKE_MEDIA_VEBOX1: usize = 0x0A564;
const FORCEWAKE_MEDIA_VEBOX2: usize = 0x0A568;
const FORCEWAKE_MEDIA_VEBOX3: usize = 0x0A56C;
const FORCEWAKE_ACK_MEDIA: usize = 0x0D88;
const FORCEWAKE_ACK_VDBOX0: usize = 0x0D50;
const FORCEWAKE_ACK_VDBOX1: usize = 0x0D54;
const FORCEWAKE_ACK_VDBOX2: usize = 0x0D58;
const FORCEWAKE_ACK_VDBOX3: usize = 0x0D5C;
const FORCEWAKE_ACK_VEBOX0: usize = 0x0D70;
const FORCEWAKE_ACK_VEBOX1: usize = 0x0D74;
const FORCEWAKE_ACK_VEBOX2: usize = 0x0D78;
const FORCEWAKE_ACK_VEBOX3: usize = 0x0D7C;
const FORCEWAKE_KERNEL: u32 = 1 << 0;

const GEN11_VCS0_RING_BASE: usize = 0x1C0000;
const GEN11_VCS1_RING_BASE: usize = 0x1C4000;
const GEN11_VECS0_RING_BASE: usize = 0x1C8000;
const GEN11_VECS1_RING_BASE: usize = 0x1D8000;

const RING_TAIL: usize = 0x30;
const RING_HEAD: usize = 0x34;
const RING_START: usize = 0x38;
const RING_CTL: usize = 0x3C;
const RING_ACTHD: usize = 0x74;
const RING_MI_MODE: usize = 0x9C;
const RING_IPEIR: usize = 0x64;
const RING_IPEHR: usize = 0x68;
const RING_INSTDONE: usize = 0x6C;
const RING_INSTPS: usize = 0x70;
const RING_CONTEXT_CONTROL: usize = 0x244;
const RING_CONTEXT_CONTROL_REF: usize = 0x5A0;
const RING_MODE_GEN7: usize = 0x29C;
const RING_EXECLIST_STATUS_LO: usize = 0x234;
const RING_EXECLIST_STATUS_HI: usize = 0x238;
const RING_EXECLIST_CONTROL: usize = 0x550;

const MEDIA_ENGINE_GPU_ADDR_BASE: u64 = 0x0120_0000;
const MEDIA_ENGINE_GPU_ADDR_STRIDE: u64 = 0x0080_0000;
const MEDIA_DEFAULT_RING_BYTES: usize = 16 * 1024;
const MEDIA_DEFAULT_CONTEXT_BYTES: usize = 22 * 4096;
const MEDIA_DEFAULT_BATCH_BYTES: usize = 32 * 1024;
const MEDIA_DEFAULT_RESULT_BYTES: usize = 4 * 4096;
const MEDIA_DEFAULT_BITSTREAM_BYTES: usize = 256 * 1024;
const MEDIA_DEFAULT_OUTPUT_SURFACE_BYTES: usize = 4 * 1024 * 1024;
const MEDIA_DEFAULT_SCRATCH_BYTES: usize = 64 * 1024;
const MEDIA_SCRATCH_OFFSET_BYTES: usize = MEDIA_DEFAULT_SCRATCH_BYTES;
const MEDIA_SUBMIT_POLL_ITERS: usize = 100_000;

const MEDIA_HTTP_LOCAL_DEMO_URLS: [&str; 2] = [
    "http://192.168.178.112:8080/tools/vid/demo_yelly.mp4",
    "http://pcjb:8080/tools/vid/demo_yelly.mp4",
];
const MEDIA_HTTP_LOCAL_DEMO_TIMEOUT_MS: u32 = 90_000;
const MEDIA_HTTP_LOCAL_DEMO_MAX_BYTES: usize = 1024 * 1024 * 1024;

const RING_HWS_PGA: usize = 0x80;
const RING_HWSTAM: usize = 0x98;
const RING_EXECLIST_SUBMIT_PORT: usize = 0x230;
const RING_EXECLIST_SQ_LO: usize = 0x510;
const RING_EXECLIST_SQ_HI: usize = 0x514;
const RING_BBADDR: usize = 0x140;
const RING_BBADDR_UDW: usize = 0x168;
const GEN12_RING_FAULT_REG: usize = 0x0000_CEC4;

const MI_STORE_DWORD_IMM_GEN4: u32 = (0x20 << 23) | 2;
const MI_USE_GGTT: u32 = 1 << 22;
const MI_STORE_DWORD_IMM_GEN4_LEN_DW4: u32 = MI_STORE_DWORD_IMM_GEN4 | MI_USE_GGTT | (4 - 2);
const MI_FLUSH_DW: u32 = (0x26 << 23) | 3;
const MI_FLUSH_DW_VIDEO_PIPELINE_CACHE_INVALIDATE: u32 = 1 << 7;
const MI_FLUSH_DW_POST_SYNC_WRITE_IMMEDIATE: u32 = 1 << 14;
const MI_FLUSH_DW_ADDR_GTT: u32 = 1 << 2;
const MI_ARB_CHECK: u32 = 0x0280_0000;
const MI_BATCH_BUFFER_END: u32 = 0x0500_0000;
const MI_BATCH_BUFFER_START_GEN8: u32 = (0x31 << 23) | 1;
const MI_BATCH_GTT: u32 = 2 << 6;
const MI_NOOP: u32 = 0;
const MI_FORCE_WAKEUP: u32 = 29 << 23;
const MI_FORCE_WAKEUP_MFX_WELL: u32 = (1 << 9) | (0x300 << 16);
const MI_LOAD_REGISTER_IMM: u32 = 0x1100_0000;
const MI_LRI_CS_MMIO: u32 = 1 << 19;
const MI_LRI_FORCE_POSTED: u32 = 1 << 12;

const EL_CTRL_LOAD: u32 = 1 << 0;
const CTX_CTRL_RS_CTX_ENABLE: u32 = 1 << 1;
const CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT: u32 = 1 << 0;
const CTX_CTRL_ENGINE_CTX_SAVE_INHIBIT: u32 = 1 << 2;
const CTX_CTRL_INHIBIT_SYN_CTX_SWITCH: u32 = 1 << 3;
const CTX_DESC_VALID: u32 = 1 << 0;
const CTX_DESC_FORCE_RESTORE: u32 = 1 << 2;
const CTX_DESC_PRIVILEGE: u32 = 1 << 8;
const CTX_DESC_PRIORITY_NORMAL: u32 = 1 << 9;
const CTX_DESC_ADDRESSING_MODE_SHIFT: u32 = 3;
const INTEL_LEGACY_64B_CONTEXT: u32 = 3;
const GEN11_GFX_DISABLE_LEGACY_MODE: u32 = 1 << 3;
const STOP_RING: u32 = 1 << 8;

const MEDIA_PIPELINE_MFX: u32 = 2;
const MEDIA_CMD_OPCODE_MFX_COMMON: u32 = 0;
const MEDIA_CMD_OPCODE_MFX_AVC: u32 = 1;
const MFX_PIPE_MODE_SELECT: u32 = 0;
const MFX_SURFACE_STATE: u32 = 1;
const MFX_PIPE_BUF_ADDR_STATE: u32 = 2;
const MFX_IND_OBJ_BASE_ADDR_STATE: u32 = 3;
const MFX_BSP_BUF_BASE_ADDR_STATE: u32 = 4;
const MFX_AVC_IMG_STATE: u32 = 0;
const MFD_AVC_BSD_OBJECT: u32 = 8;
const MFX_CMD_LEN_PIPE_MODE_SELECT: u32 = 3;
const MFX_CMD_LEN_SURFACE_STATE: u32 = 4;
const MFX_CMD_LEN_PIPE_BUF_ADDR_STATE: u32 = 63;
const MFX_CMD_LEN_IND_OBJ_BASE_ADDR_STATE: u32 = 24;
const MFX_CMD_LEN_BSP_BUF_BASE_ADDR_STATE: u32 = 8;
const MFX_CMD_LEN_AVC_IMG_STATE: u32 = 19;
const MFX_CMD_LEN_AVC_BSD_OBJECT: u32 = 5;

const MFX_QM_STATE: u32 = 7;
const MFX_CMD_LEN_QM_STATE: u32 = 16;
const MFX_AVC_DIRECTMODE_STATE: u32 = 2;
const MFX_CMD_LEN_AVC_DIRECTMODE_STATE: u32 = 69;
const QM_AVC_4X4_INTRA: u32 = 0;
const QM_AVC_4X4_INTER: u32 = 1;
const QM_AVC_8X8_INTRA: u32 = 2;
const QM_AVC_8X8_INTER: u32 = 3;
const QM_FLAT_VALUE: u32 = 0x10101010;

// MFX_WAIT: 1-DWord command, CommandType=3(GFX), CommandSubtype=1, MFXSyncControlFlag=1
const MFX_WAIT_SYNC: u32 = (3 << 29) | (1 << 27) | (1 << 8);

// MFD_AVC_DPB_STATE: SubOpcodeA=1, SubOpcodeB=6, MediaCmdOpcode=1, length=27 (bias 2)
const MFD_AVC_DPB_STATE: u32 = 6;
const MFD_AVC_DPB_STATE_SUBOPCODE_A: u32 = 1;
const MFX_CMD_LEN_AVC_DPB_STATE: u32 = 25;

// MFD_AVC_PICID_STATE: SubOpcodeA=1, SubOpcodeB=5, MediaCmdOpcode=1, length=10 (bias 2)
const MFD_AVC_PICID_STATE: u32 = 5;
const MFD_AVC_PICID_STATE_SUBOPCODE_A: u32 = 1;
const MFX_CMD_LEN_AVC_PICID_STATE: u32 = 8;

// TGL MOCS index 1 = pagetable-controlled (UC). Index 0 = error/invalid.
const MFX_MOCS_UC: u32 = 1;

const MEDIA_RESULT_SLOT_BYTES: u64 = 8;
const MEDIA_RESULT_KICKOFF_SLOT: u64 = 0;
const MEDIA_RESULT_PRESUBMIT_SLOT: u64 = MEDIA_RESULT_KICKOFF_SLOT + MEDIA_RESULT_SLOT_BYTES;
const MEDIA_RESULT_POSTSUBMIT_SLOT: u64 = MEDIA_RESULT_PRESUBMIT_SLOT + MEDIA_RESULT_SLOT_BYTES;
const MEDIA_RESULT_COMPLETE_SLOT: u64 = MEDIA_RESULT_POSTSUBMIT_SLOT + MEDIA_RESULT_SLOT_BYTES;
const MEDIA_RESULT_BITSTREAM_ADDR_LO_SLOT: u64 =
    MEDIA_RESULT_COMPLETE_SLOT + MEDIA_RESULT_SLOT_BYTES;
const MEDIA_RESULT_BITSTREAM_ADDR_HI_SLOT: u64 =
    MEDIA_RESULT_BITSTREAM_ADDR_LO_SLOT + MEDIA_RESULT_SLOT_BYTES;
const MEDIA_RESULT_BITSTREAM_BYTES_SLOT: u64 =
    MEDIA_RESULT_BITSTREAM_ADDR_HI_SLOT + MEDIA_RESULT_SLOT_BYTES;
const MEDIA_RESULT_SAMPLE_NALS_SLOT: u64 =
    MEDIA_RESULT_BITSTREAM_BYTES_SLOT + MEDIA_RESULT_SLOT_BYTES;
const MEDIA_RESULT_STAGE_FLAGS_SLOT: u64 = MEDIA_RESULT_SAMPLE_NALS_SLOT + MEDIA_RESULT_SLOT_BYTES;
const MEDIA_RESULT_OUTPUT_SURFACE_ADDR_LO_SLOT: u64 =
    MEDIA_RESULT_STAGE_FLAGS_SLOT + MEDIA_RESULT_SLOT_BYTES;
const MEDIA_RESULT_OUTPUT_SURFACE_ADDR_HI_SLOT: u64 =
    MEDIA_RESULT_OUTPUT_SURFACE_ADDR_LO_SLOT + MEDIA_RESULT_SLOT_BYTES;
const MEDIA_RESULT_OUTPUT_SURFACE_BYTES_SLOT: u64 =
    MEDIA_RESULT_OUTPUT_SURFACE_ADDR_HI_SLOT + MEDIA_RESULT_SLOT_BYTES;
const MEDIA_RESULT_FRAME_DIMS_SLOT: u64 =
    MEDIA_RESULT_OUTPUT_SURFACE_BYTES_SLOT + MEDIA_RESULT_SLOT_BYTES;

static MEDIA_KICKOFF_RAN: AtomicBool = AtomicBool::new(false);
static MEDIA_DEMO_RAN: AtomicBool = AtomicBool::new(false);
static MEDIA_KICKOFF_STATE: Mutex<Option<MediaKickoffState>> = Mutex::new(None);
static MEDIA_BACKING: Mutex<Option<MediaBitstreamBacking>> = Mutex::new(None);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum MediaEngineClass {
    VideoDecode,
    VideoEnhancement,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum MediaProvisioning {
    Kickoff,
    ScaleOutReserve,
    Disabled,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum MediaWorkloadKind {
    DecodeBitstream,
    DecodeFrame,
    EnhanceFrame,
    SessionSnapshot,
    EngineReset,
    Smoke,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum MediaSubmissionTransport {
    GuC,
    Execlists,
    Disabled,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum MediaCompletionMode {
    ResultMemoryPoll,
    ExeclistStatusPoll,
    None,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum MediaKickoffStage {
    Discovery,
    ResourcePlanning,
    SubmissionWiring,
    CommandEncoding,
    Smoke,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MediaEngineId {
    pub class: MediaEngineClass,
    pub instance: u8,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MediaCapabilities {
    pub decode: bool,
    pub enhance: bool,
    pub huc_assist: bool,
    pub sfc: bool,
    pub relative_mmio_lrc: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MediaEngineDescriptor {
    pub id: MediaEngineId,
    pub name: &'static str,
    pub ring_base: usize,
    pub provisioning: MediaProvisioning,
    pub capabilities: MediaCapabilities,
    pub default_workload: MediaWorkloadKind,
}

impl MediaEngineDescriptor {
    const fn unused() -> Self {
        Self {
            id: MediaEngineId {
                class: MediaEngineClass::VideoDecode,
                instance: 0,
            },
            name: "unused",
            ring_base: 0,
            provisioning: MediaProvisioning::Disabled,
            capabilities: MediaCapabilities {
                decode: false,
                enhance: false,
                huc_assist: false,
                sfc: false,
                relative_mmio_lrc: false,
            },
            default_workload: MediaWorkloadKind::SessionSnapshot,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MediaGpuWindowLayout {
    pub ring_gpu_addr: u64,
    pub context_gpu_addr: u64,
    pub batch_gpu_addr: u64,
    pub bitstream_gpu_addr: u64,
    pub output_surface_gpu_addr: u64,
    pub result_gpu_addr: u64,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct MediaEngineRuntimeSnapshot {
    pub name: &'static str,
    pub ring_base: usize,
    pub observed: bool,
    pub tail: u32,
    pub head: u32,
    pub start: u32,
    pub ctl: u32,
    pub acthd: u32,
    pub mi_mode: u32,
    pub mode: u32,
    pub ctx_ctl: u32,
    pub execlist_ctl: u32,
    pub execlist_status_lo: u32,
    pub execlist_status_hi: u32,
    pub ipeir: u32,
    pub ipehr: u32,
    pub instdone: u32,
    pub instps: u32,
}

impl MediaEngineRuntimeSnapshot {
    const fn unused() -> Self {
        Self {
            name: "unused",
            ring_base: 0,
            observed: false,
            tail: 0,
            head: 0,
            start: 0,
            ctl: 0,
            acthd: 0,
            mi_mode: 0,
            mode: 0,
            ctx_ctl: 0,
            execlist_ctl: 0,
            execlist_status_lo: 0,
            execlist_status_hi: 0,
            ipeir: 0,
            ipehr: 0,
            instdone: 0,
            instps: 0,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct MediaSliceWakeAck {
    pub name: &'static str,
    pub value: u32,
    pub awake: bool,
}

impl MediaSliceWakeAck {
    const fn empty() -> Self {
        Self {
            name: "unused",
            value: 0,
            awake: false,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct MediaForcewakeSnapshot {
    pub global_req: u32,
    pub global_ack: u32,
    pub awake_count: usize,
    pub slice_count: usize,
    pub slices: [MediaSliceWakeAck; 8],
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct MediaApiRoute {
    pub name: &'static str,
    pub workload: MediaWorkloadKind,
    pub preferred_engine_class: Option<MediaEngineClass>,
    pub transport: MediaSubmissionTransport,
    pub summary: &'static str,
}

impl MediaApiRoute {
    const fn empty() -> Self {
        Self {
            name: "unused",
            workload: MediaWorkloadKind::SessionSnapshot,
            preferred_engine_class: None,
            transport: MediaSubmissionTransport::Disabled,
            summary: "",
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct MediaApiShape {
    pub route_count: usize,
    pub routes: [MediaApiRoute; MAX_MEDIA_API_ROUTES],
}

impl MediaApiShape {
    const fn empty() -> Self {
        Self {
            route_count: 0,
            routes: [MediaApiRoute::empty(); MAX_MEDIA_API_ROUTES],
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct MediaTopology {
    pub sku_name: &'static str,
    pub active_engine_count: usize,
    pub planned_engine_count: usize,
    pub engines: [MediaEngineDescriptor; MAX_MEDIA_ENGINES],
    pub default_decode: Option<MediaEngineId>,
    pub default_enhance: Option<MediaEngineId>,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct MediaBitstreamDemoState {
    pub ready: bool,
    pub engine_name: &'static str,
    pub ring_gpu_addr: u64,
    pub context_gpu_addr: u64,
    pub batch_gpu_addr: u64,
    pub result_gpu_addr: u64,
    pub bitstream_gpu_addr: u64,
    pub output_surface_gpu_addr: u64,
    pub bitstream_phys: u64,
    pub output_surface_phys: u64,
    pub bitstream_bytes: usize,
    pub output_surface_bytes: usize,
    pub frame_width: u16,
    pub frame_height: u16,
    pub output_surface_pitch: usize,
    pub sample_nal_count: usize,
    pub has_idr: bool,
    pub kickoff_marker: u32,
    pub complete_marker: u32,
    pub output_surface_signature: u32,
    pub output_surface_nonzero_samples: usize,
    pub submit_completed: bool,
    pub present_attempted: bool,
    pub present_ready: bool,
    pub synthetic_preview: bool,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct MediaKickoffState {
    pub topology: MediaTopology,
    pub runtime_count: usize,
    pub runtimes: [MediaEngineRuntimeSnapshot; MAX_MEDIA_ENGINES],
    pub wake: MediaForcewakeSnapshot,
    pub api: MediaApiShape,
    pub preferred_transport: MediaSubmissionTransport,
    pub guc_ready: bool,
    pub guc_status: u32,
    pub stage: MediaKickoffStage,
    pub decode_bitstream_demo: Option<MediaBitstreamDemoState>,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct MediaSurfaceWindow {
    pub name: &'static str,
    pub gpu_addr: u64,
    pub phys: u64,
    pub virt: *mut u8,
    pub bytes: usize,
}

unsafe impl Send for MediaSurfaceWindow {}
unsafe impl Sync for MediaSurfaceWindow {}

#[derive(Copy, Clone)]
struct MediaBitstreamBacking {
    ring_phys: u64,
    ring_virt: *mut u8,
    ring_bytes: usize,
    context_phys: u64,
    context_virt: *mut u8,
    context_bytes: usize,
    batch_phys: u64,
    batch_virt: *mut u8,
    batch_bytes: usize,
    result_phys: u64,
    result_virt: *mut u8,
    result_bytes: usize,
    bitstream_phys: u64,
    bitstream_virt: *mut u8,
    bitstream_bytes: usize,
    output_surface_phys: u64,
    output_surface_virt: *mut u8,
    output_surface_bytes: usize,
    ppgtt_pml4_phys: u64,
}

unsafe impl Send for MediaBitstreamBacking {}
unsafe impl Sync for MediaBitstreamBacking {}

#[inline]
fn media_msg_slice_regs() -> [(&'static str, usize); 8] {
    [
        ("vdbox0", FORCEWAKE_ACK_VDBOX0),
        ("vdbox1", FORCEWAKE_ACK_VDBOX1),
        ("vdbox2", FORCEWAKE_ACK_VDBOX2),
        ("vdbox3", FORCEWAKE_ACK_VDBOX3),
        ("vebox0", FORCEWAKE_ACK_VEBOX0),
        ("vebox1", FORCEWAKE_ACK_VEBOX1),
        ("vebox2", FORCEWAKE_ACK_VEBOX2),
        ("vebox3", FORCEWAKE_ACK_VEBOX3),
    ]
}

fn current_topology() -> MediaTopology {
    let decode0 = MediaEngineDescriptor {
        id: MediaEngineId {
            class: MediaEngineClass::VideoDecode,
            instance: 0,
        },
        name: "vcs0",
        ring_base: GEN11_VCS0_RING_BASE,
        provisioning: MediaProvisioning::Kickoff,
        capabilities: MediaCapabilities {
            decode: true,
            enhance: false,
            huc_assist: true,
            sfc: true,
            relative_mmio_lrc: true,
        },
        default_workload: MediaWorkloadKind::DecodeFrame,
    };
    let enhance0 = MediaEngineDescriptor {
        id: MediaEngineId {
            class: MediaEngineClass::VideoEnhancement,
            instance: 0,
        },
        name: "vecs0",
        ring_base: GEN11_VECS0_RING_BASE,
        provisioning: MediaProvisioning::Kickoff,
        capabilities: MediaCapabilities {
            decode: false,
            enhance: true,
            huc_assist: false,
            sfc: false,
            relative_mmio_lrc: true,
        },
        default_workload: MediaWorkloadKind::EnhanceFrame,
    };
    let decode1 = MediaEngineDescriptor {
        id: MediaEngineId {
            class: MediaEngineClass::VideoDecode,
            instance: 1,
        },
        name: "vcs1",
        ring_base: GEN11_VCS1_RING_BASE,
        provisioning: MediaProvisioning::ScaleOutReserve,
        capabilities: MediaCapabilities {
            decode: true,
            enhance: false,
            huc_assist: true,
            sfc: true,
            relative_mmio_lrc: true,
        },
        default_workload: MediaWorkloadKind::DecodeFrame,
    };
    let enhance1 = MediaEngineDescriptor {
        id: MediaEngineId {
            class: MediaEngineClass::VideoEnhancement,
            instance: 1,
        },
        name: "vecs1",
        ring_base: GEN11_VECS1_RING_BASE,
        provisioning: MediaProvisioning::ScaleOutReserve,
        capabilities: MediaCapabilities {
            decode: false,
            enhance: true,
            huc_assist: false,
            sfc: false,
            relative_mmio_lrc: true,
        },
        default_workload: MediaWorkloadKind::EnhanceFrame,
    };

    MediaTopology {
        sku_name: "xelp-media-preview",
        active_engine_count: 2,
        planned_engine_count: 4,
        engines: [decode0, enhance0, decode1, enhance1],
        default_decode: Some(decode0.id),
        default_enhance: Some(enhance0.id),
    }
}

fn current_api_shape(transport: MediaSubmissionTransport) -> MediaApiShape {
    let mut api = MediaApiShape::empty();
    api.route_count = 4;
    api.routes[0] = MediaApiRoute {
        name: "media.decode.preview",
        workload: MediaWorkloadKind::DecodeBitstream,
        preferred_engine_class: Some(MediaEngineClass::VideoDecode),
        transport,
        summary: "fetch a local HTTP MP4, parse H.264 AU0, and preview it on the primary surface",
    };
    api.routes[1] = MediaApiRoute {
        name: "media.decode.submit",
        workload: MediaWorkloadKind::DecodeFrame,
        preferred_engine_class: Some(MediaEngineClass::VideoDecode),
        transport,
        summary: "reserve the VCS-shaped resource layout for a future decode submit path",
    };
    api.routes[2] = MediaApiRoute {
        name: "media.observe.snapshot",
        workload: MediaWorkloadKind::SessionSnapshot,
        preferred_engine_class: None,
        transport,
        summary: "snapshot forcewake and live engine registers for the media slices",
    };
    api.routes[3] = MediaApiRoute {
        name: "media.engine.smoke",
        workload: MediaWorkloadKind::Smoke,
        preferred_engine_class: None,
        transport,
        summary: "keep the preview scaffolding ready for later VCS command encoding",
    };
    api
}

fn engine_window(slot: usize) -> MediaGpuWindowLayout {
    let base = MEDIA_ENGINE_GPU_ADDR_BASE + (slot as u64) * MEDIA_ENGINE_GPU_ADDR_STRIDE;
    MediaGpuWindowLayout {
        ring_gpu_addr: base,
        context_gpu_addr: base + 0x0001_0000,
        batch_gpu_addr: base + 0x0008_0000,
        bitstream_gpu_addr: base + 0x0014_0000,
        output_surface_gpu_addr: base + 0x0020_0000,
        result_gpu_addr: base + 0x0060_0000,
    }
}

fn preferred_transport() -> MediaSubmissionTransport {
    MediaSubmissionTransport::Execlists
}

fn snapshot_forcewake(dev: crate::intel::Dev) -> MediaForcewakeSnapshot {
    let mut slices = [MediaSliceWakeAck::empty(); 8];
    let mut awake_count = 0usize;
    for (idx, (name, reg)) in media_msg_slice_regs().into_iter().enumerate() {
        let value = super::mmio_read(dev, reg);
        let awake = (value & FORCEWAKE_KERNEL) != 0;
        if awake {
            awake_count += 1;
        }
        slices[idx] = MediaSliceWakeAck { name, value, awake };
    }
    MediaForcewakeSnapshot {
        global_req: super::mmio_read(dev, FORCEWAKE_MEDIA_GEN11),
        global_ack: super::mmio_read(dev, FORCEWAKE_ACK_MEDIA),
        awake_count,
        slice_count: slices.len(),
        slices,
    }
}

fn snapshot_runtime(
    dev: crate::intel::Dev,
    desc: MediaEngineDescriptor,
) -> MediaEngineRuntimeSnapshot {
    let base = desc.ring_base;
    let tail = super::mmio_read(dev, base + RING_TAIL);
    let head = super::mmio_read(dev, base + RING_HEAD);
    let start = super::mmio_read(dev, base + RING_START);
    let ctl = super::mmio_read(dev, base + RING_CTL);
    let acthd = super::mmio_read(dev, base + RING_ACTHD);
    let mi_mode = super::mmio_read(dev, base + RING_MI_MODE);
    let mode = super::mmio_read(dev, base + RING_MODE_GEN7);
    let ctx_ctl = super::mmio_read(dev, base + RING_CONTEXT_CONTROL);
    let execlist_ctl = super::mmio_read(dev, base + RING_EXECLIST_CONTROL);
    let execlist_status_lo = super::mmio_read(dev, base + RING_EXECLIST_STATUS_LO);
    let execlist_status_hi = super::mmio_read(dev, base + RING_EXECLIST_STATUS_HI);
    let ipeir = super::mmio_read(dev, base + RING_IPEIR);
    let ipehr = super::mmio_read(dev, base + RING_IPEHR);
    let instdone = super::mmio_read(dev, base + RING_INSTDONE);
    let instps = super::mmio_read(dev, base + RING_INSTPS);
    let observed = tail != 0
        || head != 0
        || start != 0
        || ctl != 0
        || acthd != 0
        || ctx_ctl != 0
        || execlist_status_lo != 0
        || execlist_status_hi != 0;

    MediaEngineRuntimeSnapshot {
        name: desc.name,
        ring_base: desc.ring_base,
        observed,
        tail,
        head,
        start,
        ctl,
        acthd,
        mi_mode,
        mode,
        ctx_ctl,
        execlist_ctl,
        execlist_status_lo,
        execlist_status_hi,
        ipeir,
        ipehr,
        instdone,
        instps,
    }
}

fn rebuild_kickoff_state(
    stage: MediaKickoffStage,
    demo: Option<MediaBitstreamDemoState>,
) -> Option<MediaKickoffState> {
    let dev = super::claimed_device()?;
    let topology = current_topology();
    let transport = preferred_transport();
    let mut runtimes = [MediaEngineRuntimeSnapshot::unused(); MAX_MEDIA_ENGINES];
    for (idx, desc) in topology
        .engines
        .iter()
        .take(topology.planned_engine_count)
        .copied()
        .enumerate()
    {
        runtimes[idx] = snapshot_runtime(dev, desc);
    }
    Some(MediaKickoffState {
        topology,
        runtime_count: topology.planned_engine_count,
        runtimes,
        wake: snapshot_forcewake(dev),
        api: current_api_shape(transport),
        preferred_transport: transport,
        guc_ready: super::guc_ready(),
        guc_status: super::guc::status(dev),
        stage,
        decode_bitstream_demo: demo,
    })
}

fn store_kickoff_state(stage: MediaKickoffStage, demo: Option<MediaBitstreamDemoState>) {
    *MEDIA_KICKOFF_STATE.lock() = rebuild_kickoff_state(stage, demo);
}

fn log_kickoff_overview(state: &MediaKickoffState) {
    let default_decode = state
        .topology
        .default_decode
        .and_then(|id| {
            state
                .topology
                .engines
                .iter()
                .take(state.topology.planned_engine_count)
                .find(|engine| engine.id == id)
                .copied()
        })
        .unwrap_or(state.topology.engines[0]);
    let active_route = state
        .api
        .routes
        .iter()
        .take(state.api.route_count)
        .find(|route| route.workload == MediaWorkloadKind::DecodeBitstream)
        .copied()
        .unwrap_or(MediaApiRoute::empty());
    let runtime = state
        .runtimes
        .iter()
        .take(state.runtime_count)
        .find(|runtime| runtime.name == default_decode.name)
        .copied()
        .unwrap_or(MediaEngineRuntimeSnapshot::unused());

    crate::log!(
        "intel/media: gather sku={} route={} engine={} class={:?} caps=decode:{} huc:{} sfc:{} rel_lrc:{} transport={:?}\n",
        state.topology.sku_name,
        active_route.name,
        default_decode.name,
        default_decode.id.class,
        default_decode.capabilities.decode as u8,
        default_decode.capabilities.huc_assist as u8,
        default_decode.capabilities.sfc as u8,
        default_decode.capabilities.relative_mmio_lrc as u8,
        state.preferred_transport,
    );
    crate::log!(
        "intel/media: probe wake={}/{} engine={} observed={} head=0x{:08X} tail=0x{:08X} acthd=0x{:08X} ipehr=0x{:08X} instdone=0x{:08X}\n",
        state.wake.awake_count,
        state.wake.slice_count,
        runtime.name,
        runtime.observed as u8,
        runtime.head,
        runtime.tail,
        runtime.acthd,
        runtime.ipehr,
        runtime.instdone,
    );
}

fn find_fourcc(bytes: &[u8], fourcc: &[u8; 4]) -> Option<usize> {
    bytes.windows(4).position(|window| window == fourcc)
}

fn marker_base(engine: MediaEngineDescriptor) -> u32 {
    let class_base = match engine.id.class {
        MediaEngineClass::VideoDecode => 0x4D44_1000,
        MediaEngineClass::VideoEnhancement => 0x4D45_1000,
    };
    class_base + (engine.id.instance as u32) * 0x100
}

fn fit_output_dims(
    width: usize,
    height: usize,
    budget_bytes: usize,
) -> Option<(usize, usize, usize)> {
    let mut out_w = width.max(1);
    let mut out_h = height.max(1);
    loop {
        let pitch = super::align_up(out_w.checked_mul(4)?, 64)?;
        if pitch.checked_mul(out_h)? <= budget_bytes {
            return Some((out_w, out_h, pitch));
        }
        if out_w == 1 && out_h == 1 {
            return None;
        }
        out_w = (out_w / 2).max(1);
        out_h = (out_h / 2).max(1);
    }
}

fn sample_surface_signature(bytes: &[u8]) -> (u32, usize) {
    let sample_count = bytes.len().min(4096);
    if sample_count == 0 {
        return (0, 0);
    }
    let step = (bytes.len() / sample_count.max(1)).max(1);
    let mut signature = 0u32;
    let mut nonzero = 0usize;
    let mut idx = 0usize;
    let mut seen = 0usize;
    while idx < bytes.len() && seen < sample_count {
        let value = bytes[idx];
        signature = signature.rotate_left(5) ^ u32::from(value);
        nonzero += usize::from(value != 0);
        idx = idx.saturating_add(step);
        seen += 1;
    }
    (signature, nonzero)
}

fn progressive_present_output_surface(
    output_surface: &[u8],
    frame_width: u16,
    frame_height: u16,
    output_pitch: usize,
    submit_completed: bool,
) -> (bool, u32, usize) {
    let (signature, nonzero_samples) = sample_surface_signature(output_surface);

    if frame_width != 0
        && frame_height != 0
        && output_pitch >= frame_width as usize
        && output_surface.len()
            >= output_pitch
                .saturating_mul(frame_height as usize)
                .saturating_add((output_pitch.saturating_mul(frame_height as usize)) / 2)
        && submit_completed
    {
        let ready = super::display::present_nv12_surface_center(
            output_surface,
            frame_width as u32,
            frame_height as u32,
            output_pitch,
        );
        return (ready, signature, nonzero_samples);
    }

    if nonzero_samples != 0 {
        return (false, signature, nonzero_samples);
    }

    (false, signature, nonzero_samples)
}

#[inline]
fn align_up_u32(value: u32, align: u32) -> u32 {
    if align == 0 {
        value
    } else {
        value.saturating_add(align.saturating_sub(1)) & !align.saturating_sub(1)
    }
}

#[inline]
fn masked_bits_update(set_bits: u32, clear_bits: u32) -> u32 {
    let update = set_bits | clear_bits;
    set_bits | (update << 16)
}

#[inline]
fn mi_lri_num_regs(num_regs: u32) -> u32 {
    num_regs.saturating_mul(2).saturating_sub(1)
}

#[inline]
fn mi_lri_cmd(num_regs: u32, flags: u32) -> u32 {
    MI_LOAD_REGISTER_IMM | MI_LRI_CS_MMIO | flags | mi_lri_num_regs(num_regs)
}

fn push_mi_nops(state: &mut [u32], idx: &mut usize, count: usize) {
    for _ in 0..count {
        state[*idx] = MI_NOOP;
        *idx += 1;
    }
}

/// Build a minimal 4-level PPGTT (PML4→PDP→PD→PT) that identity-maps
/// the given (gpu_addr, phys, size) ranges so the MFX pipe can reach them.
/// Returns (pml4_phys, total_allocated_bytes) or None on failure.
fn build_ppgtt_for_ranges(ranges: &[(u64, u64, usize)]) -> Option<u64> {
    const PAGE: usize = 4096;
    const ENTRIES: usize = 512;
    const PTE_PRESENT_RW: u64 = 0x3; // Present + Read/Write (leaf PTEs)
    const PDE_PRESENT_RW_UC: u64 = 0x3 | (1 << 3) | (1 << 4); // Present + RW + PWT + PCD (directory entries)

    // Determine which 2MB-aligned PD indices we need PT pages for
    let mut pd_min = usize::MAX;
    let mut pd_max = 0usize;
    for &(gpu, _phys, size) in ranges {
        if size == 0 {
            continue;
        }
        let first_pd = (gpu as usize) >> 21;
        let last_pd = (gpu as usize + size - 1) >> 21;
        if first_pd < pd_min {
            pd_min = first_pd;
        }
        if last_pd > pd_max {
            pd_max = last_pd;
        }
    }
    if pd_min > pd_max {
        return None;
    }
    let pt_count = pd_max - pd_min + 1;
    // Allocate: 1 PML4 + 1 PDP + 1 PD + pt_count PT pages
    let total_pages = 3 + pt_count;
    let alloc_bytes = total_pages * PAGE;
    let (base_phys, base_virt) = crate::dma::alloc(alloc_bytes, PAGE)?;

    let tables = unsafe { core::slice::from_raw_parts_mut(base_virt as *mut u64, alloc_bytes / 8) };
    // Zero all pages
    tables.fill(0);

    let pml4_off = 0; // page 0
    let pdp_off = ENTRIES; // page 1
    let pd_off = 2 * ENTRIES; // page 2
    let pt_base_off = 3 * ENTRIES; // pages 3..3+pt_count

    let pml4_phys = base_phys;
    let pdp_phys = base_phys + PAGE as u64;
    let pd_phys = base_phys + 2 * PAGE as u64;

    // PML4[0] → PDP (directory entries need PPAT_UNCACHED = PWT+PCD)
    tables[pml4_off] = pdp_phys | PDE_PRESENT_RW_UC;
    // PDP[0] → PD
    tables[pdp_off] = pd_phys | PDE_PRESENT_RW_UC;

    // PD[pd_min..=pd_max] → PT pages
    for i in 0..pt_count {
        let pt_phys = base_phys + (3 + i) as u64 * PAGE as u64;
        tables[pd_off + pd_min + i] = pt_phys | PDE_PRESENT_RW_UC;
    }

    // Fill PT entries for each range
    for &(gpu, phys, size) in ranges {
        if size == 0 {
            continue;
        }
        let mut offset = 0usize;
        while offset < size {
            let va = gpu as usize + offset;
            let pa = phys + offset as u64;
            let pd_idx = va >> 21;
            let pt_idx = (va >> 12) & 0x1FF;
            let pt_page = pd_idx - pd_min;
            let slot = pt_base_off + pt_page * ENTRIES + pt_idx;
            tables[slot] = (pa & !0xFFF) | PTE_PRESENT_RW;
            offset += PAGE;
        }
    }

    crate::intel::dma_flush(base_virt, alloc_bytes);
    Some(pml4_phys)
}

fn build_ring_batch_start_words(
    ring_virt: *mut u8,
    ring_bytes: usize,
    result_gpu_addr: u64,
    prelaunch_marker: u32,
    batch_gpu_addr: u64,
) -> Option<usize> {
    if ring_virt.is_null() || ring_bytes < 32 {
        return None;
    }
    let dwords = unsafe { core::slice::from_raw_parts_mut(ring_virt as *mut u32, 8) };
    dwords[0] = MI_STORE_DWORD_IMM_GEN4_LEN_DW4;
    dwords[1] = (result_gpu_addr + MEDIA_RESULT_KICKOFF_SLOT) as u32;
    dwords[2] = ((result_gpu_addr + MEDIA_RESULT_KICKOFF_SLOT) >> 32) as u32;
    dwords[3] = prelaunch_marker;
    dwords[4] = MI_BATCH_BUFFER_START_GEN8;
    dwords[5] = batch_gpu_addr as u32;
    dwords[6] = (batch_gpu_addr >> 32) as u32;
    dwords[7] = MI_NOOP;
    Some(32)
}

fn ring_ctl_value_for_size(size: usize) -> Option<u32> {
    let size = u32::try_from(size).ok()?;
    Some(size.checked_sub(4096)? | 1)
}

fn build_execlist_context_descriptor_for_gpu_addr(context_gpu_addr: u64) -> (u32, u32) {
    let base = (context_gpu_addr as u32) & 0xFFFF_F000;
    (
        base | CTX_DESC_VALID
            | CTX_DESC_FORCE_RESTORE
            | CTX_DESC_PRIVILEGE
            | CTX_DESC_PRIORITY_NORMAL
            | (INTEL_LEGACY_64B_CONTEXT << CTX_DESC_ADDRESSING_MODE_SHIFT),
        (context_gpu_addr >> 32) as u32,
    )
}

fn write_video_lrc_ring_tail(context_virt: *mut u8, context_len: usize, ring_tail: u32) {
    const LRC_STATE_OFFSET_DWORDS: usize = 4096 / core::mem::size_of::<u32>();
    const CTX_RING_TAIL_DW: usize = 7;

    if context_virt.is_null() {
        return;
    }
    let total_dwords = context_len / core::mem::size_of::<u32>();
    if total_dwords <= LRC_STATE_OFFSET_DWORDS + CTX_RING_TAIL_DW {
        return;
    }

    let dwords = unsafe { core::slice::from_raw_parts_mut(context_virt as *mut u32, total_dwords) };
    dwords[LRC_STATE_OFFSET_DWORDS + CTX_RING_TAIL_DW] = ring_tail;
    super::dma_flush(context_virt, context_len);
}

fn init_gen12_video_context_image(
    context_virt: *mut u8,
    context_len: usize,
    ring_base: usize,
    ring_start: u32,
    ring_tail: u32,
    ring_ctl: u32,
    _hws_pga: u32,
    pml4_phys: u64,
) -> bool {
    const LRC_STATE_OFFSET_DWORDS: usize = 4096 / core::mem::size_of::<u32>();
    const CTX_RING_HEAD_DW: usize = 5;
    const CTX_RING_TAIL_DW: usize = 7;
    const CTX_RING_START_DW: usize = 9;
    const CTX_RING_CTL_DW: usize = 11;

    if context_virt.is_null() {
        return false;
    }
    let total_dwords = context_len / core::mem::size_of::<u32>();
    if total_dwords <= LRC_STATE_OFFSET_DWORDS {
        return false;
    }
    let dwords = unsafe { core::slice::from_raw_parts_mut(context_virt as *mut u32, total_dwords) };
    dwords.fill(0);
    let state = &mut dwords[LRC_STATE_OFFSET_DWORDS..];
    if state.len() < 96 {
        return false;
    }
    let ring_base = ring_base as u32;
    let mut idx = 0usize;
    state[idx] = MI_NOOP;
    idx += 1;
    state[idx] = mi_lri_cmd(13, MI_LRI_FORCE_POSTED);
    idx += 1;
    state[idx] = ring_base + 0x244;
    state[idx + 1] = 0x0009_0009;
    state[idx + 2] = ring_base + 0x34;
    state[idx + 3] = 0;
    state[idx + 4] = ring_base + 0x30;
    state[idx + 5] = ring_tail;
    state[idx + 6] = ring_base + 0x38;
    state[idx + 7] = ring_start;
    state[idx + 8] = ring_base + 0x3C;
    state[idx + 9] = ring_ctl;
    state[idx + 10] = ring_base + 0x168;
    state[idx + 11] = 0;
    state[idx + 12] = ring_base + 0x140;
    state[idx + 13] = 0;
    state[idx + 14] = ring_base + 0x110;
    state[idx + 15] = 0;
    state[idx + 16] = ring_base + 0x1C0;
    state[idx + 17] = 0;
    state[idx + 18] = ring_base + 0x1C4;
    state[idx + 19] = 0;
    state[idx + 20] = ring_base + 0x1C8;
    state[idx + 21] = 0;
    state[idx + 22] = ring_base + 0x180;
    state[idx + 23] = 0;
    state[idx + 24] = ring_base + 0x2B4;
    state[idx + 25] = 0;
    idx += 26;
    push_mi_nops(state, &mut idx, 5);
    state[idx] = mi_lri_cmd(9, MI_LRI_FORCE_POSTED);
    idx += 1;
    // CTX_TIMESTAMP, PDP3..PDP1 (unused=0), PDP0 = PML4 phys
    let pdp_values: [(u32, u32); 9] = [
        (0x3A8, 0),                        // CTX_TIMESTAMP
        (0x28C, 0),                        // PDP3_UDW
        (0x288, 0),                        // PDP3_LDW
        (0x284, 0),                        // PDP2_UDW
        (0x280, 0),                        // PDP2_LDW
        (0x27C, 0),                        // PDP1_UDW
        (0x278, 0),                        // PDP1_LDW
        (0x274, (pml4_phys >> 32) as u32), // PDP0_UDW
        (0x270, pml4_phys as u32),         // PDP0_LDW
    ];
    for (offset, value) in pdp_values {
        state[idx] = ring_base + offset;
        state[idx + 1] = value;
        idx += 2;
    }
    push_mi_nops(state, &mut idx, 12);
    state[CTX_RING_HEAD_DW] = 0;
    state[CTX_RING_TAIL_DW] = ring_tail;
    state[CTX_RING_START_DW] = ring_start;
    state[CTX_RING_CTL_DW] = ring_ctl;
    state[idx] = MI_BATCH_BUFFER_END | 1;
    true
}

fn emit_store_dword(batch: &mut [u32], idx: &mut usize, gpu_addr: u64, value: u32) -> bool {
    if idx.saturating_add(4) > batch.len() {
        return false;
    }
    batch[*idx] = MI_STORE_DWORD_IMM_GEN4_LEN_DW4;
    batch[*idx + 1] = gpu_addr as u32;
    batch[*idx + 2] = (gpu_addr >> 32) as u32;
    batch[*idx + 3] = value;
    *idx += 4;
    true
}

#[inline]
fn media_cmd_header(
    media_opcode: u32,
    subopcode_a: u32,
    subopcode_b: u32,
    dword_length: u32,
) -> u32 {
    (3 << 29)
        | (MEDIA_PIPELINE_MFX << 27)
        | (media_opcode << 24)
        | (subopcode_a << 21)
        | (subopcode_b << 16)
        | dword_length
}

fn begin_batch_packet(
    batch: &mut [u32],
    idx: &mut usize,
    dword_count: usize,
    header: u32,
) -> Option<usize> {
    if idx.saturating_add(dword_count) > batch.len() {
        return None;
    }
    let start = *idx;
    let end = start + dword_count;
    batch[start..end].fill(0);
    batch[start] = header;
    *idx = end;
    Some(start)
}

#[inline]
fn packet_write_addr64(batch: &mut [u32], packet_start: usize, dword_index: usize, gpu_addr: u64) {
    batch[packet_start + dword_index] = gpu_addr as u32;
    batch[packet_start + dword_index + 1] = (gpu_addr >> 32) as u32;
}

fn emit_mfx_wait(batch: &mut [u32], idx: &mut usize) -> bool {
    if *idx >= batch.len() {
        return false;
    }
    batch[*idx] = MFX_WAIT_SYNC;
    *idx += 1;
    true
}

#[inline]
fn read_result_dword(base_virt: *mut u8, slot_off: u64) -> u32 {
    let ptr = (base_virt as usize).saturating_add(slot_off as usize) as *const u32;
    unsafe { core::ptr::read_volatile(ptr) }
}

fn build_h264_decode_batch_skeleton(
    batch_virt: *mut u8,
    batch_bytes: usize,
    result_gpu_addr: u64,
    bitstream_gpu_addr: u64,
    output_surface_gpu_addr: u64,
    scratch_gpu_addr: u64,
    output_surface_bytes: usize,
    frame_width: u16,
    frame_height: u16,
    annexb_bytes: usize,
    sample_nal_count: usize,
    has_idr: bool,
    idr_nal_offset: usize,
    idr_nal_length: usize,
    sps: &ParsedSps,
    pps: &ParsedPps,
    kickoff_marker: u32,
    presubmit_marker: u32,
    postsubmit_marker: u32,
    complete_marker: u32,
) -> Option<usize> {
    let batch = unsafe {
        core::slice::from_raw_parts_mut(
            batch_virt as *mut u32,
            batch_bytes / core::mem::size_of::<u32>(),
        )
    };
    let mut idx = 0usize;
    let width = frame_width as u32;
    let height = frame_height as u32;
    let width_mbs = width.saturating_add(15) / 16;
    let height_mbs = height.saturating_add(15) / 16;
    let frame_dims = width | (height << 16);
    let output_pitch = align_up_u32(width.max(64), 64);
    let chroma_y_offset = height;
    let stage_flags = (has_idr as u32) | (1 << 1);

    if !emit_store_dword(
        batch,
        &mut idx,
        result_gpu_addr + MEDIA_RESULT_KICKOFF_SLOT,
        kickoff_marker,
    ) || !emit_store_dword(
        batch,
        &mut idx,
        result_gpu_addr + MEDIA_RESULT_BITSTREAM_ADDR_LO_SLOT,
        bitstream_gpu_addr as u32,
    ) || !emit_store_dword(
        batch,
        &mut idx,
        result_gpu_addr + MEDIA_RESULT_BITSTREAM_ADDR_HI_SLOT,
        (bitstream_gpu_addr >> 32) as u32,
    ) || !emit_store_dword(
        batch,
        &mut idx,
        result_gpu_addr + MEDIA_RESULT_BITSTREAM_BYTES_SLOT,
        annexb_bytes as u32,
    ) || !emit_store_dword(
        batch,
        &mut idx,
        result_gpu_addr + MEDIA_RESULT_SAMPLE_NALS_SLOT,
        sample_nal_count as u32,
    ) || !emit_store_dword(
        batch,
        &mut idx,
        result_gpu_addr + MEDIA_RESULT_STAGE_FLAGS_SLOT,
        stage_flags,
    ) || !emit_store_dword(
        batch,
        &mut idx,
        result_gpu_addr + MEDIA_RESULT_OUTPUT_SURFACE_ADDR_LO_SLOT,
        output_surface_gpu_addr as u32,
    ) || !emit_store_dword(
        batch,
        &mut idx,
        result_gpu_addr + MEDIA_RESULT_OUTPUT_SURFACE_ADDR_HI_SLOT,
        (output_surface_gpu_addr >> 32) as u32,
    ) || !emit_store_dword(
        batch,
        &mut idx,
        result_gpu_addr + MEDIA_RESULT_OUTPUT_SURFACE_BYTES_SLOT,
        output_surface_bytes as u32,
    ) || !emit_store_dword(
        batch,
        &mut idx,
        result_gpu_addr + MEDIA_RESULT_FRAME_DIMS_SLOT,
        frame_dims,
    ) || !emit_store_dword(
        batch,
        &mut idx,
        result_gpu_addr + MEDIA_RESULT_PRESUBMIT_SLOT,
        presubmit_marker,
    ) || !emit_store_dword(
        batch,
        &mut idx,
        result_gpu_addr + MEDIA_RESULT_POSTSUBMIT_SLOT,
        postsubmit_marker,
    ) {
        return None;
    }

    let flush = begin_batch_packet(
        batch,
        &mut idx,
        5,
        MI_FLUSH_DW
            | MI_FLUSH_DW_VIDEO_PIPELINE_CACHE_INVALIDATE
            | MI_FLUSH_DW_POST_SYNC_WRITE_IMMEDIATE,
    )?;
    batch[flush + 1] = (result_gpu_addr as u32) | MI_FLUSH_DW_ADDR_GTT;
    batch[flush + 2] = (result_gpu_addr >> 32) as u32;
    batch[flush + 3] = postsubmit_marker;
    batch[flush + 4] = 0;

    // --- MI_FORCE_WAKEUP (Gen12: power on MFX decode well) ---
    if idx.saturating_add(2) > batch.len() {
        return None;
    }
    batch[idx] = MI_FORCE_WAKEUP;
    batch[idx + 1] = MI_FORCE_WAKEUP_MFX_WELL;
    idx += 2;

    // --- MFX_WAIT (Gen12+: required before PIPE_MODE_SELECT) ---
    if !emit_mfx_wait(batch, &mut idx) {
        return None;
    }

    // --- MFX_PIPE_MODE_SELECT ---
    // StandardSelect=2(AVC), PostDeblockingOutputEnable(bit9), Short Format (bit17=0 default)
    let pipe_mode = begin_batch_packet(
        batch,
        &mut idx,
        (MFX_CMD_LEN_PIPE_MODE_SELECT + 2) as usize,
        media_cmd_header(
            MEDIA_CMD_OPCODE_MFX_COMMON,
            0,
            MFX_PIPE_MODE_SELECT,
            MFX_CMD_LEN_PIPE_MODE_SELECT,
        ),
    )?;
    batch[pipe_mode + 1] = 2 | (1 << 9);

    // --- MFX_WAIT (Gen12+: required before SURFACE_STATE) ---
    if !emit_mfx_wait(batch, &mut idx) {
        return None;
    }

    // --- MFX_SURFACE_STATE ---
    let surface = begin_batch_packet(
        batch,
        &mut idx,
        (MFX_CMD_LEN_SURFACE_STATE + 2) as usize,
        media_cmd_header(
            MEDIA_CMD_OPCODE_MFX_COMMON,
            0,
            MFX_SURFACE_STATE,
            MFX_CMD_LEN_SURFACE_STATE,
        ),
    )?;
    batch[surface + 2] = ((width.saturating_sub(1)) << 4) | ((height.saturating_sub(1)) << 18);
    batch[surface + 3] = 1 | ((output_pitch.saturating_sub(1)) << 3) | (1 << 27) | (4 << 28);
    batch[surface + 4] = chroma_y_offset;
    batch[surface + 5] = chroma_y_offset; // Y Offset for V(Cr) = same as U(Cb)

    // --- MFX_PIPE_BUF_ADDR_STATE ---
    let pipe_buf = begin_batch_packet(
        batch,
        &mut idx,
        (MFX_CMD_LEN_PIPE_BUF_ADDR_STATE + 2) as usize,
        media_cmd_header(
            MEDIA_CMD_OPCODE_MFX_COMMON,
            0,
            MFX_PIPE_BUF_ADDR_STATE,
            MFX_CMD_LEN_PIPE_BUF_ADDR_STATE,
        ),
    )?;
    // DW3: Pre Deblocking Attributes (unused but needs valid MOCS)
    batch[pipe_buf + 3] = MFX_MOCS_UC;
    // DW4-5: Post Deblocking Destination = output surface
    packet_write_addr64(batch, pipe_buf, 4, output_surface_gpu_addr);
    batch[pipe_buf + 6] = MFX_MOCS_UC; // Post Deblocking Attributes
    // DW9: Original Uncompressed Picture Source Attributes
    batch[pipe_buf + 9] = MFX_MOCS_UC;
    // DW12: Stream-Out Data Destination Attributes
    batch[pipe_buf + 12] = MFX_MOCS_UC;
    // DW13-14: Intra Row Store Scratch Buffer
    packet_write_addr64(batch, pipe_buf, 13, scratch_gpu_addr);
    batch[pipe_buf + 15] = MFX_MOCS_UC; // Intra Row Store Attributes
    // DW16-17: Deblocking Filter Row Store Scratch Buffer
    packet_write_addr64(batch, pipe_buf, 16, scratch_gpu_addr + 0x4000);
    batch[pipe_buf + 18] = MFX_MOCS_UC; // Deblocking Filter Row Store Attributes
    // DW51: Reference Picture Attributes
    batch[pipe_buf + 51] = MFX_MOCS_UC;
    // DW54: MB Status Buffer Attributes
    batch[pipe_buf + 54] = MFX_MOCS_UC;
    // DW57: MB ILDB Stream-Out Buffer Attributes
    batch[pipe_buf + 57] = MFX_MOCS_UC;
    // DW60: Second MB ILDB Stream-Out Buffer Attributes
    batch[pipe_buf + 60] = MFX_MOCS_UC;
    // DW64: Scaled Reference Surface Attributes
    batch[pipe_buf + 64] = MFX_MOCS_UC;

    // --- MFX_IND_OBJ_BASE_ADDR_STATE ---
    let ind_obj = begin_batch_packet(
        batch,
        &mut idx,
        (MFX_CMD_LEN_IND_OBJ_BASE_ADDR_STATE + 2) as usize,
        media_cmd_header(
            MEDIA_CMD_OPCODE_MFX_COMMON,
            0,
            MFX_IND_OBJ_BASE_ADDR_STATE,
            MFX_CMD_LEN_IND_OBJ_BASE_ADDR_STATE,
        ),
    )?;
    packet_write_addr64(batch, ind_obj, 1, bitstream_gpu_addr);
    batch[ind_obj + 3] = MFX_MOCS_UC; // Bitstream Attributes
    packet_write_addr64(batch, ind_obj, 4, bitstream_gpu_addr + annexb_bytes as u64);
    batch[ind_obj + 8] = MFX_MOCS_UC; // MV Object Attributes
    batch[ind_obj + 13] = MFX_MOCS_UC; // IT-COEFF Attributes
    batch[ind_obj + 18] = MFX_MOCS_UC; // IT-DBLK Attributes
    batch[ind_obj + 23] = MFX_MOCS_UC; // PAK-BSE Attributes

    // --- MFX_BSP_BUF_BASE_ADDR_STATE ---
    let bsp = begin_batch_packet(
        batch,
        &mut idx,
        (MFX_CMD_LEN_BSP_BUF_BASE_ADDR_STATE + 2) as usize,
        media_cmd_header(
            MEDIA_CMD_OPCODE_MFX_COMMON,
            0,
            MFX_BSP_BUF_BASE_ADDR_STATE,
            MFX_CMD_LEN_BSP_BUF_BASE_ADDR_STATE,
        ),
    )?;
    packet_write_addr64(batch, bsp, 1, scratch_gpu_addr + 0x8000);
    batch[bsp + 3] = MFX_MOCS_UC; // BSD Row Store Attributes
    packet_write_addr64(batch, bsp, 4, scratch_gpu_addr + 0xC000);
    batch[bsp + 6] = MFX_MOCS_UC; // MPR Row Store Attributes
    batch[bsp + 9] = MFX_MOCS_UC; // Bitplane Read Buffer Attributes

    // --- MFD_AVC_DPB_STATE (27 DWords, all zeros for IDR) ---
    let _dpb = begin_batch_packet(
        batch,
        &mut idx,
        (MFX_CMD_LEN_AVC_DPB_STATE + 2) as usize,
        media_cmd_header(
            MEDIA_CMD_OPCODE_MFX_AVC,
            MFD_AVC_DPB_STATE_SUBOPCODE_A,
            MFD_AVC_DPB_STATE,
            MFX_CMD_LEN_AVC_DPB_STATE,
        ),
    )?;

    // --- MFD_AVC_PICID_STATE (10 DWords) ---
    let picid = begin_batch_packet(
        batch,
        &mut idx,
        (MFX_CMD_LEN_AVC_PICID_STATE + 2) as usize,
        media_cmd_header(
            MEDIA_CMD_OPCODE_MFX_AVC,
            MFD_AVC_PICID_STATE_SUBOPCODE_A,
            MFD_AVC_PICID_STATE,
            MFX_CMD_LEN_AVC_PICID_STATE,
        ),
    )?;
    // DW1: PictureIDRemappingDisable = 0 (enable remapping)
    batch[picid + 1] = 0;
    // DW2-9: 16 PictureIDs packed as 16-bit values, all 0xFFFF (no references)
    for dw in 2..10 {
        batch[picid + dw] = 0xFFFF_FFFF;
    }

    // --- MFX_AVC_IMG_STATE (21 DWords) ---
    let pic_height = if sps.frame_mbs_only_flag {
        sps.pic_height_in_map_units_minus1 + 1
    } else {
        (sps.pic_height_in_map_units_minus1 + 1) * 2
    };
    let avc_img = begin_batch_packet(
        batch,
        &mut idx,
        (MFX_CMD_LEN_AVC_IMG_STATE + 2) as usize,
        media_cmd_header(MEDIA_CMD_OPCODE_MFX_AVC, 0, MFX_AVC_IMG_STATE, MFX_CMD_LEN_AVC_IMG_STATE),
    )?;
    // DW1: FrameSize
    batch[avc_img + 1] = (width_mbs * pic_height) & 0xFFFF;
    // DW2: FrameWidth(7:0), FrameHeight(23:16)
    batch[avc_img + 2] =
        (width_mbs.saturating_sub(1) & 0xFF) | ((pic_height.saturating_sub(1) & 0xFF) << 16);
    // DW3: ImageStructure(9:8)=0(frame), WeightedBiPredIDC(11:10), WeightedPredEnable(12),
    //       FirstChromaQPOffset(20:16), SecondChromaQPOffset(28:24)
    batch[avc_img + 3] = ((pps.weighted_bipred_idc & 3) << 10)
        | ((pps.weighted_pred_flag as u32) << 12)
        | (((pps.chroma_qp_index_offset as u32) & 0x1F) << 16)
        | (((pps.second_chroma_qp_index_offset as u32) & 0x1F) << 24);
    // DW4: FieldPic(0), MBAFF(1), FrameMBOnly(2), 8x8IDCT(3), Direct8x8Inf(4),
    //       ConstrainedIntra(5), NonRefPic(6), EntropyCodingSync(7),
    //       ChromaFormatIDC(11:10)
    batch[avc_img + 4] = ((sps.frame_mbs_only_flag as u32) << 2)
        | ((pps.transform_8x8_mode_flag as u32) << 3)
        | ((sps.direct_8x8_inference_flag as u32) << 4)
        | ((pps.constrained_intra_pred_flag as u32) << 5)
        | ((pps.entropy_coding_mode_flag as u32) << 7)
        | ((sps.chroma_format_idc & 3) << 10);
    // DW5: TrellisQuantizationChromaDisable(27)
    batch[avc_img + 5] = 1 << 27;
    // DW13: InitialQP(7:0), NumActiveRefL0(13:8), NumActiveRefL1(21:16),
    //        NumRefFrames(28:24)
    batch[avc_img + 13] = ((pps.pic_init_qp_minus26 as u32) & 0xFF)
        | (((pps.num_ref_idx_l0_default_active_minus1 + 1) & 0x3F) << 8)
        | (((pps.num_ref_idx_l1_default_active_minus1 + 1) & 0x3F) << 16)
        | ((sps.max_num_ref_frames & 0x1F) << 24);
    // DW14: PicOrderPresent(0), DeltaPicOrderAlwaysZero(1), PicOrderCntType(3:2),
    //        RedundantPicCntPresent(11), DeblockingFilterCtrlPresent(15),
    //        Log2MaxFrameNum(23:16), Log2MaxPicOrderCountLSB(31:24)
    batch[avc_img + 14] = (pps.bottom_field_pic_order_in_frame_present_flag as u32)
        | ((sps.delta_pic_order_always_zero_flag as u32) << 1)
        | ((sps.pic_order_cnt_type & 3) << 2)
        | ((pps.redundant_pic_cnt_present_flag as u32) << 11)
        | ((pps.deblocking_filter_control_present_flag as u32) << 15)
        | ((sps.log2_max_frame_num_minus4 & 0xFF) << 16)
        | ((sps.log2_max_pic_order_cnt_lsb_minus4 & 0xFF) << 24);

    // --- MFX_QM_STATE (flat quantization matrices) ---
    for &qm_type in &[
        QM_AVC_4X4_INTRA,
        QM_AVC_4X4_INTER,
        QM_AVC_8X8_INTRA,
        QM_AVC_8X8_INTER,
    ] {
        let qm = begin_batch_packet(
            batch,
            &mut idx,
            (MFX_CMD_LEN_QM_STATE + 2) as usize,
            media_cmd_header(MEDIA_CMD_OPCODE_MFX_COMMON, 0, MFX_QM_STATE, MFX_CMD_LEN_QM_STATE),
        )?;
        batch[qm + 1] = qm_type;
        for dw in 2..18 {
            batch[qm + dw] = QM_FLAT_VALUE;
        }
    }

    // --- MFX_AVC_DIRECTMODE_STATE (71 DWords, zeroed for IDR) ---
    let _directmode = begin_batch_packet(
        batch,
        &mut idx,
        (MFX_CMD_LEN_AVC_DIRECTMODE_STATE + 2) as usize,
        media_cmd_header(
            MEDIA_CMD_OPCODE_MFX_AVC,
            0,
            MFX_AVC_DIRECTMODE_STATE,
            MFX_CMD_LEN_AVC_DIRECTMODE_STATE,
        ),
    )?;

    // --- MFD_AVC_BSD_OBJECT (7 DWords) ---
    let avc_bsd = begin_batch_packet(
        batch,
        &mut idx,
        (MFX_CMD_LEN_AVC_BSD_OBJECT + 2) as usize,
        media_cmd_header(
            MEDIA_CMD_OPCODE_MFX_AVC,
            1,
            MFD_AVC_BSD_OBJECT,
            MFX_CMD_LEN_AVC_BSD_OBJECT,
        ),
    )?;
    // DW1: IndirectBSDDataLength = IDR NAL bytes
    batch[avc_bsd + 1] = idr_nal_length as u32;
    // DW2: IndirectBSDDataStartAddress = offset of IDR NAL within bitstream buffer
    batch[avc_bsd + 2] = (idr_nal_offset as u32) & 0x1FFF_FFFF;
    // DW3: InlineData DW0 — ConcealmentMethod=1(bit31), ISliceConcealmentMode
    batch[avc_bsd + 3] = 1 << 31;
    // DW4: InlineData DW1 — LastSlice(3), EmulationPreventionBytePresent(4), FixPrevMBSkipped(7)
    batch[avc_bsd + 4] = (1 << 3) | (1 << 4) | (1 << 7);
    // DW5: InlineData DW2 — IntraPredictionErrorControl(0), Intra8x84x4(1)
    batch[avc_bsd + 5] = 1 | (1 << 1);

    // --- Post-decode: drain MFX pipeline, then write completion marker ---
    let done_flush = begin_batch_packet(
        batch,
        &mut idx,
        5,
        MI_FLUSH_DW
            | MI_FLUSH_DW_VIDEO_PIPELINE_CACHE_INVALIDATE
            | MI_FLUSH_DW_POST_SYNC_WRITE_IMMEDIATE,
    )?;
    batch[done_flush + 1] =
        ((result_gpu_addr + MEDIA_RESULT_COMPLETE_SLOT) as u32) | MI_FLUSH_DW_ADDR_GTT;
    batch[done_flush + 2] = ((result_gpu_addr + MEDIA_RESULT_COMPLETE_SLOT) >> 32) as u32;
    batch[done_flush + 3] = complete_marker;
    batch[done_flush + 4] = 0;

    if idx.saturating_add(3) > batch.len() {
        return None;
    }
    batch[idx] = MI_ARB_CHECK;
    batch[idx + 1] = MI_BATCH_BUFFER_END;
    batch[idx + 2] = MI_NOOP;
    Some((idx + 3).saturating_mul(core::mem::size_of::<u32>()))
}

fn execlist_submit_port_push(
    dev: crate::intel::Dev,
    ring_base: usize,
    context0_lo: u32,
    context0_hi: u32,
    _context1_lo: u32,
    _context1_hi: u32,
) {
    super::mmio_write(dev, ring_base + RING_EXECLIST_SQ_LO, context0_lo);
    super::mmio_write(dev, ring_base + RING_EXECLIST_SQ_HI, context0_hi);
}

fn wake_media_engine_forcewake(dev: crate::intel::Dev, engine: MediaEngineDescriptor) {
    let (req, ack) = match engine.id.class {
        MediaEngineClass::VideoDecode => match engine.id.instance {
            0 => (FORCEWAKE_MEDIA_VDBOX0, FORCEWAKE_ACK_VDBOX0),
            1 => (FORCEWAKE_MEDIA_VDBOX1, FORCEWAKE_ACK_VDBOX1),
            2 => (FORCEWAKE_MEDIA_VDBOX2, FORCEWAKE_ACK_VDBOX2),
            _ => (FORCEWAKE_MEDIA_VDBOX3, FORCEWAKE_ACK_VDBOX3),
        },
        MediaEngineClass::VideoEnhancement => match engine.id.instance {
            0 => (FORCEWAKE_MEDIA_VEBOX0, FORCEWAKE_ACK_VEBOX0),
            1 => (FORCEWAKE_MEDIA_VEBOX1, FORCEWAKE_ACK_VEBOX1),
            2 => (FORCEWAKE_MEDIA_VEBOX2, FORCEWAKE_ACK_VEBOX2),
            _ => (FORCEWAKE_MEDIA_VEBOX3, FORCEWAKE_ACK_VEBOX3),
        },
    };
    super::mmio_write(dev, req, super::mask_en(FORCEWAKE_KERNEL));
    for _ in 0..20_000 {
        if (super::mmio_read(dev, ack) & FORCEWAKE_KERNEL) != 0 {
            break;
        }
        core::hint::spin_loop();
    }
}

fn seed_media_ring_live_state(
    dev: crate::intel::Dev,
    ring_base: usize,
    pphwsp_gpu: u32,
    ring_start: u32,
    ring_ctl: u32,
    ring_tail: u32,
) {
    let ctx_ctl_req = masked_bits_update(2, 1 | 4 | 8);
    super::mmio_write(dev, ring_base + RING_CONTEXT_CONTROL, ctx_ctl_req);
    super::mmio_write(dev, ring_base + RING_MI_MODE, STOP_RING << 16);
    super::mmio_write(dev, ring_base + RING_HWS_PGA, pphwsp_gpu);
    super::mmio_write(dev, ring_base + RING_HWSTAM, !0u32);
    super::mmio_write(dev, ring_base + RING_HEAD, 0);
    super::mmio_write(dev, ring_base + RING_START, ring_start);
    super::mmio_write(dev, ring_base + RING_CTL, ring_ctl);
    super::mmio_write(dev, ring_base + RING_TAIL, ring_tail);
}

fn submit_decode_bitstream_demo(
    dev: crate::intel::Dev,
    engine: MediaEngineDescriptor,
    windows: MediaGpuWindowLayout,
    backing: MediaBitstreamBacking,
    frame_width: u16,
    frame_height: u16,
    annex_b: &[u8],
    sample_nal_count: usize,
    has_idr: bool,
    idr_nal_offset: usize,
    idr_nal_length: usize,
    sps: &ParsedSps,
    pps: &ParsedPps,
) -> Option<(bool, usize, usize, u64, u64, *mut u8)> {
    let output_pitch = align_up_u32((frame_width as u32).max(64), 64) as usize;
    let output_bytes = output_pitch
        .checked_mul(frame_height as usize)?
        .checked_add((output_pitch.checked_mul(frame_height as usize)?) / 2)?;
    let output_budget = backing
        .output_surface_bytes
        .checked_sub(MEDIA_SCRATCH_OFFSET_BYTES)?;
    if output_bytes > output_budget || annex_b.len() > backing.bitstream_bytes {
        return None;
    }

    let scratch_gpu_addr = windows.output_surface_gpu_addr;
    let output_surface_gpu_addr =
        windows.output_surface_gpu_addr + MEDIA_SCRATCH_OFFSET_BYTES as u64;
    let output_surface_phys = backing.output_surface_phys + MEDIA_SCRATCH_OFFSET_BYTES as u64;
    let output_surface_virt =
        unsafe { backing.output_surface_virt.add(MEDIA_SCRATCH_OFFSET_BYTES) };

    unsafe {
        core::ptr::write_bytes(backing.ring_virt, 0, backing.ring_bytes);
        core::ptr::write_bytes(backing.context_virt, 0, backing.context_bytes);
        core::ptr::write_bytes(backing.batch_virt, 0, backing.batch_bytes);
        core::ptr::write_bytes(backing.result_virt, 0, backing.result_bytes);
        core::ptr::write_bytes(backing.bitstream_virt, 0, backing.bitstream_bytes);
        core::ptr::write_bytes(backing.output_surface_virt, 0, backing.output_surface_bytes);
        core::ptr::copy_nonoverlapping(annex_b.as_ptr(), backing.bitstream_virt, annex_b.len());
    }

    let kickoff_marker = marker_base(engine);
    let ring_prelaunch_marker = kickoff_marker.wrapping_sub(1);
    let presubmit_marker = kickoff_marker + 1;
    let postsubmit_marker = kickoff_marker + 2;
    let complete_marker = kickoff_marker + 3;
    let batch_tail_bytes = build_h264_decode_batch_skeleton(
        backing.batch_virt,
        backing.batch_bytes,
        windows.result_gpu_addr,
        windows.bitstream_gpu_addr,
        output_surface_gpu_addr,
        scratch_gpu_addr,
        output_bytes,
        frame_width,
        frame_height,
        annex_b.len(),
        sample_nal_count,
        has_idr,
        idr_nal_offset,
        idr_nal_length,
        sps,
        pps,
        kickoff_marker,
        presubmit_marker,
        postsubmit_marker,
        complete_marker,
    )?;

    // Decisive diagnostics: addresses, bitstream NAL header, batch size
    let bs_hdr = if idr_nal_offset + 4 <= annex_b.len() {
        u32::from_be_bytes([
            annex_b[idr_nal_offset],
            annex_b[idr_nal_offset + 1],
            annex_b[idr_nal_offset + 2],
            annex_b[idr_nal_offset + 3],
        ])
    } else {
        0xDEAD
    };
    crate::log!(
        "intel/media: addrs output_gpu=0x{:08X} scratch_gpu=0x{:08X} bitstream_gpu=0x{:08X} result_gpu=0x{:08X} batch_dwords={} idr_nal_hdr=0x{:08X}\n",
        output_surface_gpu_addr,
        scratch_gpu_addr,
        windows.bitstream_gpu_addr,
        windows.result_gpu_addr,
        batch_tail_bytes / 4,
        bs_hdr,
    );

    let ring_tail_bytes = build_ring_batch_start_words(
        backing.ring_virt,
        backing.ring_bytes,
        windows.result_gpu_addr,
        ring_prelaunch_marker,
        windows.batch_gpu_addr,
    )?;
    let ring_ctl = ring_ctl_value_for_size(backing.ring_bytes)?;
    let ring_start = windows.ring_gpu_addr as u32;
    let pphwsp_gpu = (windows.context_gpu_addr & !0xFFF) as u32;
    if !init_gen12_video_context_image(
        backing.context_virt,
        backing.context_bytes,
        engine.ring_base,
        ring_start,
        ring_tail_bytes as u32,
        ring_ctl,
        pphwsp_gpu,
        backing.ppgtt_pml4_phys,
    ) {
        return None;
    }
    write_video_lrc_ring_tail(backing.context_virt, backing.context_bytes, ring_tail_bytes as u32);

    super::dma_flush(backing.bitstream_virt, annex_b.len());
    super::dma_flush(backing.batch_virt, batch_tail_bytes);
    super::dma_flush(backing.ring_virt, ring_tail_bytes);
    super::dma_flush(backing.context_virt, backing.context_bytes);
    super::dma_flush(backing.result_virt, backing.result_bytes);
    super::dma_flush(backing.output_surface_virt, backing.output_surface_bytes);

    wake_media_engine_forcewake(dev, engine);
    seed_media_ring_live_state(
        dev,
        engine.ring_base,
        pphwsp_gpu,
        ring_start,
        ring_ctl,
        ring_tail_bytes as u32,
    );
    let (ctx_desc_lo, ctx_desc_hi) =
        build_execlist_context_descriptor_for_gpu_addr(windows.context_gpu_addr);
    super::mmio_write(
        dev,
        engine.ring_base + RING_MODE_GEN7,
        GEN11_GFX_DISABLE_LEGACY_MODE | (GEN11_GFX_DISABLE_LEGACY_MODE << 16),
    );
    let ctx_ctl_after = masked_bits_update(
        CTX_CTRL_RS_CTX_ENABLE,
        CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT
            | CTX_CTRL_ENGINE_CTX_SAVE_INHIBIT
            | CTX_CTRL_INHIBIT_SYN_CTX_SWITCH,
    );
    super::mmio_write(dev, engine.ring_base + RING_CONTEXT_CONTROL, ctx_ctl_after);
    super::mmio_write(dev, engine.ring_base + RING_CONTEXT_CONTROL_REF, ctx_ctl_after);
    super::mmio_write(dev, engine.ring_base + RING_HWS_PGA, pphwsp_gpu);
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    execlist_submit_port_push(dev, engine.ring_base, ctx_desc_lo, ctx_desc_hi, 0, 0);
    super::mmio_write(dev, engine.ring_base + RING_EXECLIST_CONTROL, EL_CTRL_LOAD);

    let mut completed = false;
    let mut iter = 0usize;
    while iter < MEDIA_SUBMIT_POLL_ITERS {
        super::dma_flush(
            unsafe { backing.result_virt.add(MEDIA_RESULT_COMPLETE_SLOT as usize) },
            8,
        );
        let complete = read_result_dword(backing.result_virt, MEDIA_RESULT_COMPLETE_SLOT);
        if complete == complete_marker {
            completed = true;
            break;
        }
        core::hint::spin_loop();
        iter += 1;
    }
    super::dma_flush(output_surface_virt, output_bytes);
    super::dma_flush(backing.result_virt, backing.result_bytes);

    // Decisive: scan output surface for first nonzero dword, report engine post-state
    {
        let out_dwords = unsafe {
            core::slice::from_raw_parts(output_surface_virt as *const u32, output_bytes / 4)
        };
        let mut first_nz_off: u32 = 0xFFFF_FFFF;
        let mut first_nz_val: u32 = 0;
        let mut nz_count: u32 = 0;
        for (i, &dw) in out_dwords.iter().enumerate() {
            if dw != 0 {
                if first_nz_off == 0xFFFF_FFFF {
                    first_nz_off = (i * 4) as u32;
                    first_nz_val = dw;
                }
                nz_count += 1;
            }
        }
        let head_post = super::mmio_read(dev, engine.ring_base + RING_HEAD);
        let tail_post = super::mmio_read(dev, engine.ring_base + RING_TAIL);
        let acthd_post = super::mmio_read(dev, engine.ring_base + RING_ACTHD);
        let fault_post = super::mmio_read(dev, GEN12_RING_FAULT_REG);
        crate::log!(
            "intel/media: scanout completed={} iters={} nz_dwords={} first_nz_off=0x{:08X} first_nz_val=0x{:08X} head=0x{:08X} tail=0x{:08X} acthd=0x{:08X} fault=0x{:08X}\n",
            completed as u8,
            iter,
            nz_count,
            first_nz_off,
            first_nz_val,
            head_post,
            tail_post,
            acthd_post,
            fault_post,
        );
    }

    if !completed {
        let kickoff = read_result_dword(backing.result_virt, MEDIA_RESULT_KICKOFF_SLOT);
        let presubmit = read_result_dword(backing.result_virt, MEDIA_RESULT_PRESUBMIT_SLOT);
        let postsubmit = read_result_dword(backing.result_virt, MEDIA_RESULT_POSTSUBMIT_SLOT);
        let complete = read_result_dword(backing.result_virt, MEDIA_RESULT_COMPLETE_SLOT);
        let head = super::mmio_read(dev, engine.ring_base + RING_HEAD);
        let tail = super::mmio_read(dev, engine.ring_base + RING_TAIL);
        let acthd = super::mmio_read(dev, engine.ring_base + RING_ACTHD);
        let ipehr = super::mmio_read(dev, engine.ring_base + RING_IPEHR);
        let ipeir = super::mmio_read(dev, engine.ring_base + RING_IPEIR);
        let instdone = super::mmio_read(dev, engine.ring_base + RING_INSTDONE);
        let el_status_lo = super::mmio_read(dev, engine.ring_base + RING_EXECLIST_STATUS_LO);
        let el_status_hi = super::mmio_read(dev, engine.ring_base + RING_EXECLIST_STATUS_HI);
        let bbaddr_lo = super::mmio_read(dev, engine.ring_base + RING_BBADDR);
        let bbaddr_hi = super::mmio_read(dev, engine.ring_base + RING_BBADDR_UDW);
        let fault = super::mmio_read(dev, GEN12_RING_FAULT_REG);

        crate::log!(
            "intel/media: submit diag engine={} kickoff=0x{:08X}/0x{:08X} presubmit=0x{:08X}/0x{:08X} postsubmit=0x{:08X}/0x{:08X} complete=0x{:08X}/0x{:08X}\n",
            engine.name,
            kickoff,
            kickoff_marker,
            presubmit,
            presubmit_marker,
            postsubmit,
            postsubmit_marker,
            complete,
            complete_marker,
        );
        crate::log!(
            "intel/media: submit probe engine={} head=0x{:08X} tail=0x{:08X} acthd=0x{:08X} ipehr=0x{:08X} ipeir=0x{:08X} instdone=0x{:08X} bbaddr=0x{:08X}{:08X} fault=0x{:08X} el_lo=0x{:08X} el_hi=0x{:08X}\n",
            engine.name,
            head,
            tail,
            acthd,
            ipehr,
            ipeir,
            instdone,
            bbaddr_hi,
            bbaddr_lo,
            fault,
            el_status_lo,
            el_status_hi,
        );
    }

    Some((
        completed,
        output_pitch,
        output_bytes,
        output_surface_gpu_addr,
        output_surface_phys,
        output_surface_virt,
    ))
}

fn render_whitenoise_preview_into_output(
    ptr: *mut u8,
    pitch_bytes: usize,
    width: usize,
    height: usize,
    source: &[u8],
    highlight_idr: bool,
) {
    if ptr.is_null() || width == 0 || height == 0 || pitch_bytes < width.saturating_mul(4) {
        return;
    }
    let len = source.len().max(1);
    unsafe {
        for y in 0..height {
            let row = ptr.add(y.saturating_mul(pitch_bytes));
            for x in 0..width {
                let pixel = row.add(x.saturating_mul(4));
                let idx = (y.saturating_mul(width) + x) % len;
                let a = source[idx];
                let b = source[(idx.saturating_mul(5) + 17) % len];
                let c = source[(idx.saturating_mul(11) + 31) % len];
                let pos = ((x as u32).wrapping_mul(0x45D9F3B)
                    ^ (y as u32).wrapping_mul(0x119D_E1F3))
                .rotate_left(7);
                let mut noise = a
                    ^ b.rotate_left((x & 7) as u32)
                    ^ c.rotate_right((y & 7) as u32)
                    ^ pos as u8
                    ^ (pos >> 8) as u8;
                if highlight_idr {
                    noise ^= 0x3C;
                }
                core::ptr::write_volatile(pixel, noise);
                core::ptr::write_volatile(pixel.add(1), noise);
                core::ptr::write_volatile(pixel.add(2), noise);
                core::ptr::write_volatile(pixel.add(3), 0);
            }
        }
    }
}

fn ensure_demo_backing(
    dev: crate::intel::Dev,
    windows: MediaGpuWindowLayout,
) -> Option<MediaBitstreamBacking> {
    if let Some(backing) = *MEDIA_BACKING.lock() {
        return Some(backing);
    }

    let (ring_phys, ring_virt) =
        crate::dma::alloc(MEDIA_DEFAULT_RING_BYTES, crate::intel::WARM_ALIGN)?;
    let (context_phys, context_virt) =
        crate::dma::alloc(MEDIA_DEFAULT_CONTEXT_BYTES, crate::intel::WARM_ALIGN)?;
    let (batch_phys, batch_virt) =
        crate::dma::alloc(MEDIA_DEFAULT_BATCH_BYTES, crate::intel::WARM_ALIGN)?;
    let (result_phys, result_virt) =
        crate::dma::alloc(MEDIA_DEFAULT_RESULT_BYTES, crate::intel::WARM_ALIGN)?;
    let (bitstream_phys, bitstream_virt) =
        crate::dma::alloc(MEDIA_DEFAULT_BITSTREAM_BYTES, crate::intel::WARM_ALIGN)?;
    let (output_surface_phys, output_surface_virt) =
        crate::dma::alloc(MEDIA_DEFAULT_OUTPUT_SURFACE_BYTES, crate::intel::WARM_ALIGN)?;

    let mapped = super::map_ggtt(dev, ring_phys, MEDIA_DEFAULT_RING_BYTES, windows.ring_gpu_addr)
        && super::map_ggtt(
            dev,
            context_phys,
            MEDIA_DEFAULT_CONTEXT_BYTES,
            windows.context_gpu_addr,
        )
        && super::map_ggtt(dev, batch_phys, MEDIA_DEFAULT_BATCH_BYTES, windows.batch_gpu_addr)
        && super::map_ggtt(dev, result_phys, MEDIA_DEFAULT_RESULT_BYTES, windows.result_gpu_addr)
        && super::map_ggtt(
            dev,
            bitstream_phys,
            MEDIA_DEFAULT_BITSTREAM_BYTES,
            windows.bitstream_gpu_addr,
        )
        && super::map_ggtt(
            dev,
            output_surface_phys,
            MEDIA_DEFAULT_OUTPUT_SURFACE_BYTES,
            windows.output_surface_gpu_addr,
        );
    if !mapped {
        return None;
    }
    super::ggtt_invalidate(dev);

    // Build PPGTT page tables so MFX pipe data addresses resolve
    let ppgtt_pml4_phys = build_ppgtt_for_ranges(&[
        (windows.bitstream_gpu_addr, bitstream_phys, MEDIA_DEFAULT_BITSTREAM_BYTES),
        (windows.output_surface_gpu_addr, output_surface_phys, MEDIA_DEFAULT_OUTPUT_SURFACE_BYTES),
        (windows.result_gpu_addr, result_phys, MEDIA_DEFAULT_RESULT_BYTES),
    ])?;

    crate::log!("intel/media: ppgtt pml4_phys=0x{:08X}\n", ppgtt_pml4_phys,);

    let backing = MediaBitstreamBacking {
        ring_phys,
        ring_virt,
        ring_bytes: MEDIA_DEFAULT_RING_BYTES,
        context_phys,
        context_virt,
        context_bytes: MEDIA_DEFAULT_CONTEXT_BYTES,
        batch_phys,
        batch_virt,
        batch_bytes: MEDIA_DEFAULT_BATCH_BYTES,
        result_phys,
        result_virt,
        result_bytes: MEDIA_DEFAULT_RESULT_BYTES,
        bitstream_phys,
        bitstream_virt,
        bitstream_bytes: MEDIA_DEFAULT_BITSTREAM_BYTES,
        output_surface_phys,
        output_surface_virt,
        output_surface_bytes: MEDIA_DEFAULT_OUTPUT_SURFACE_BYTES,
        ppgtt_pml4_phys,
    };
    *MEDIA_BACKING.lock() = Some(backing);
    Some(backing)
}

pub(crate) fn kickoff_once() {
    MEDIA_KICKOFF_RAN.store(true, Ordering::Release);
    let prior_demo = MEDIA_KICKOFF_STATE
        .lock()
        .as_ref()
        .and_then(|state| state.decode_bitstream_demo);
    store_kickoff_state(MediaKickoffStage::CommandEncoding, prior_demo);
    if let Some(state) = *MEDIA_KICKOFF_STATE.lock() {
        crate::log!(
            "intel/media: kickoff stage={:?} guc_ready={} guc_status=0x{:08X} active={} planned={} transport={:?}\n",
            state.stage,
            state.guc_ready as u8,
            state.guc_status,
            state.topology.active_engine_count,
            state.topology.planned_engine_count,
            state.preferred_transport
        );
        log_kickoff_overview(&state);
    }
}

pub(crate) fn kickoff_state() -> Option<MediaKickoffState> {
    *MEDIA_KICKOFF_STATE.lock()
}

pub(crate) fn demo_surface_window(name: &str) -> Option<MediaSurfaceWindow> {
    let state = *MEDIA_KICKOFF_STATE.lock();
    let demo = state?.decode_bitstream_demo?;
    let backing = MEDIA_BACKING.lock().as_ref().copied()?;
    match name {
        "media.ring" => Some(MediaSurfaceWindow {
            name: "media.ring",
            gpu_addr: demo.ring_gpu_addr,
            phys: backing.ring_phys,
            virt: backing.ring_virt,
            bytes: backing.ring_bytes,
        }),
        "media.context" => Some(MediaSurfaceWindow {
            name: "media.context",
            gpu_addr: demo.context_gpu_addr,
            phys: backing.context_phys,
            virt: backing.context_virt,
            bytes: backing.context_bytes,
        }),
        "media.batch" => Some(MediaSurfaceWindow {
            name: "media.batch",
            gpu_addr: demo.batch_gpu_addr,
            phys: backing.batch_phys,
            virt: backing.batch_virt,
            bytes: backing.batch_bytes,
        }),
        "media.result" => Some(MediaSurfaceWindow {
            name: "media.result",
            gpu_addr: demo.result_gpu_addr,
            phys: backing.result_phys,
            virt: backing.result_virt,
            bytes: backing.result_bytes,
        }),
        "media.bitstream" => Some(MediaSurfaceWindow {
            name: "media.bitstream",
            gpu_addr: demo.bitstream_gpu_addr,
            phys: backing.bitstream_phys,
            virt: backing.bitstream_virt,
            bytes: backing.bitstream_bytes,
        }),
        "media.output" => Some(MediaSurfaceWindow {
            name: "media.output",
            gpu_addr: demo.output_surface_gpu_addr,
            phys: demo.output_surface_phys,
            virt: {
                let offset = usize::try_from(
                    demo.output_surface_phys
                        .checked_sub(backing.output_surface_phys)?,
                )
                .ok()?;
                if offset > backing.output_surface_bytes {
                    return None;
                }
                unsafe { backing.output_surface_virt.add(offset) }
            },
            bytes: demo.output_surface_bytes.min(
                backing.output_surface_bytes.saturating_sub(
                    usize::try_from(
                        demo.output_surface_phys
                            .checked_sub(backing.output_surface_phys)?,
                    )
                    .ok()?,
                ),
            ),
        }),
        _ => None,
    }
}

async fn fetch_media_demo_body_async() -> Option<(&'static str, Vec<u8>)> {
    for url in MEDIA_HTTP_LOCAL_DEMO_URLS {
        crate::log!(
            "intel/media: try local url={} timeout_ms={} max_bytes={}\n",
            url,
            MEDIA_HTTP_LOCAL_DEMO_TIMEOUT_MS,
            MEDIA_HTTP_LOCAL_DEMO_MAX_BYTES,
        );
        match crate::r::net::http::fetch_http_body(
            url,
            MEDIA_HTTP_LOCAL_DEMO_TIMEOUT_MS,
            MEDIA_HTTP_LOCAL_DEMO_MAX_BYTES,
        )
        .await
        {
            Ok(body) => return Some((url, body)),
            Err(err) => {
                crate::log!("intel/media: local fetch failed url={} err={:?}\n", url, err);
            }
        }
    }

    None
}

pub(crate) async fn run_https_media_demo_once_async() {
    kickoff_once();

    if MEDIA_DEMO_RAN.swap(true, Ordering::AcqRel) {
        crate::log!("intel/media: demo skipped reason=already-ran\n");
        return;
    }

    let Some(dev) = super::claimed_device() else {
        crate::log!("intel/media: demo skipped reason=no-device\n");
        return;
    };
    let topology = current_topology();
    let engine = topology
        .default_decode
        .map(|_| topology.engines[0])
        .unwrap_or(topology.engines[0]);
    let windows = engine_window(0);

    crate::log!(
        "intel/media: begin local_urls={:?} engine={}\n",
        MEDIA_HTTP_LOCAL_DEMO_URLS,
        engine.name
    );

    let (source_url, body) = match fetch_media_demo_body_async().await {
        Some(payload) => payload,
        None => {
            crate::log!("intel/media: fetch failed local_http_candidates_exhausted=1\n");
            store_kickoff_state(MediaKickoffStage::SubmissionWiring, None);
            return;
        }
    };

    crate::log!(
        "intel/media: fetched url={} bytes={} mp4_ftyp={} mp4_moov={} mp4_mdat={}\n",
        source_url,
        body.len(),
        find_fourcc(body.as_slice(), b"ftyp").is_some() as u8,
        find_fourcc(body.as_slice(), b"moov").is_some() as u8,
        find_fourcc(body.as_slice(), b"mdat").is_some() as u8
    );

    let summary = match parse_h264_mp4_summary(body.as_slice()) {
        Ok(summary) => summary,
        Err(err) => {
            crate::log!("intel/media: parse failed err={:?}\n", err);
            store_kickoff_state(MediaKickoffStage::ResourcePlanning, None);
            return;
        }
    };

    let annex_b = match build_annex_b_access_unit(&summary) {
        Ok(annex_b) => annex_b,
        Err(err) => {
            crate::log!("intel/media: annexb build failed err={:?}\n", err);
            store_kickoff_state(MediaKickoffStage::ResourcePlanning, None);
            return;
        }
    };
    let nal_types = first_sample_nal_types(summary.first_sample, summary.avcc.nal_length_size, 6);

    crate::log!(
        "intel/media: h264 dims={}x{} samples={} first_sample={} nal_len={} sps={} pps={} profile=0x{:02X} level=0x{:02X} annexb_bytes={} sample_nals={} idr={} first_nals={:?}\n",
        summary.width,
        summary.height,
        summary.sample_count,
        summary.first_sample_size,
        summary.avcc.nal_length_size,
        summary.avcc.sps.len(),
        summary.avcc.pps.len(),
        summary.avcc.profile_idc,
        summary.avcc.level_idc,
        annex_b.bytes.len(),
        annex_b.sample_nal_count,
        annex_b.has_idr as u8,
        nal_types
    );
    crate::log!(
        "intel/media: codec codec=h264 profile=0x{:02X} level=0x{:02X} frame={}x{} avcc_nal={} sample_nals={} idr={} annexb=0x{:X}\n",
        summary.avcc.profile_idc,
        summary.avcc.level_idc,
        summary.width,
        summary.height,
        summary.avcc.nal_length_size,
        annex_b.sample_nal_count,
        annex_b.has_idr as u8,
        annex_b.bytes.len(),
    );

    let Some(sps) = parse_sps(summary.avcc.sps) else {
        crate::log!("intel/media: sps parse failed\n");
        store_kickoff_state(MediaKickoffStage::ResourcePlanning, None);
        return;
    };
    let Some(pps) = parse_pps(summary.avcc.pps, &sps) else {
        crate::log!("intel/media: pps parse failed\n");
        store_kickoff_state(MediaKickoffStage::ResourcePlanning, None);
        return;
    };
    crate::log!(
        "intel/media: sps chroma={} depth_y={} depth_c={} log2_frame={} poc_type={} log2_poc={} refs={} mbs={}x{} frame_only={} direct8x8={}\n",
        sps.chroma_format_idc,
        sps.bit_depth_luma_minus8,
        sps.bit_depth_chroma_minus8,
        sps.log2_max_frame_num_minus4,
        sps.pic_order_cnt_type,
        sps.log2_max_pic_order_cnt_lsb_minus4,
        sps.max_num_ref_frames,
        sps.pic_width_in_mbs_minus1 + 1,
        sps.pic_height_in_map_units_minus1 + 1,
        sps.frame_mbs_only_flag as u8,
        sps.direct_8x8_inference_flag as u8,
    );
    crate::log!(
        "intel/media: pps cabac={} pic_order_present={} l0={} l1={} wpred={} wbipred={} qp={} cqp={} cqp2={} deblock={} constrained={} t8x8={}\n",
        pps.entropy_coding_mode_flag as u8,
        pps.bottom_field_pic_order_in_frame_present_flag as u8,
        pps.num_ref_idx_l0_default_active_minus1,
        pps.num_ref_idx_l1_default_active_minus1,
        pps.weighted_pred_flag as u8,
        pps.weighted_bipred_idc,
        pps.pic_init_qp_minus26,
        pps.chroma_qp_index_offset,
        pps.second_chroma_qp_index_offset,
        pps.deblocking_filter_control_present_flag as u8,
        pps.constrained_intra_pred_flag as u8,
        pps.transform_8x8_mode_flag as u8,
    );
    crate::log!(
        "intel/media: idr_nal offset={} length={}\n",
        annex_b.idr_nal_offset,
        annex_b.idr_nal_length,
    );

    let Some(backing) = ensure_demo_backing(dev, windows) else {
        crate::log!("intel/media: backing alloc/map failed\n");
        store_kickoff_state(MediaKickoffStage::ResourcePlanning, None);
        return;
    };

    let Some((preview_w, preview_h, preview_pitch)) = fit_output_dims(
        summary.width as usize,
        summary.height as usize,
        backing.output_surface_bytes,
    ) else {
        crate::log!(
            "intel/media: preview sizing failed dims={}x{}\n",
            summary.width,
            summary.height
        );
        store_kickoff_state(MediaKickoffStage::ResourcePlanning, None);
        return;
    };

    let kickoff_marker = marker_base(engine);
    let complete_marker = kickoff_marker + 3;
    let preview_output_bytes = preview_pitch.saturating_mul(preview_h);
    let mut used_output_bytes = preview_output_bytes;
    let mut output_surface_gpu_addr = windows.output_surface_gpu_addr;
    let mut output_surface_phys = backing.output_surface_phys;
    let mut output_surface_virt = backing.output_surface_virt;
    let mut submit_completed = false;
    let mut synthetic_preview = false;

    match submit_decode_bitstream_demo(
        dev,
        engine,
        windows,
        backing,
        summary.width,
        summary.height,
        annex_b.bytes.as_slice(),
        annex_b.sample_nal_count,
        annex_b.has_idr,
        annex_b.idr_nal_offset,
        annex_b.idr_nal_length,
        &sps,
        &pps,
    ) {
        Some((
            completed,
            decoded_pitch,
            decoded_bytes,
            decoded_gpu_addr,
            decoded_phys,
            decoded_virt,
        )) => {
            used_output_bytes = decoded_bytes;
            output_surface_gpu_addr = decoded_gpu_addr;
            output_surface_phys = decoded_phys;
            output_surface_virt = decoded_virt;
            submit_completed = completed;
            let output_surface = unsafe {
                core::slice::from_raw_parts(output_surface_virt as *const u8, used_output_bytes)
            };
            let (ready, signature, nonzero) = progressive_present_output_surface(
                output_surface,
                summary.width,
                summary.height,
                decoded_pitch,
                submit_completed,
            );
            if ready {
                let demo = MediaBitstreamDemoState {
                    ready,
                    engine_name: engine.name,
                    ring_gpu_addr: windows.ring_gpu_addr,
                    context_gpu_addr: windows.context_gpu_addr,
                    batch_gpu_addr: windows.batch_gpu_addr,
                    result_gpu_addr: windows.result_gpu_addr,
                    bitstream_gpu_addr: windows.bitstream_gpu_addr,
                    output_surface_gpu_addr,
                    bitstream_phys: backing.bitstream_phys,
                    output_surface_phys,
                    bitstream_bytes: annex_b.bytes.len(),
                    output_surface_bytes: used_output_bytes,
                    frame_width: summary.width,
                    frame_height: summary.height,
                    output_surface_pitch: decoded_pitch,
                    sample_nal_count: annex_b.sample_nal_count,
                    has_idr: annex_b.has_idr,
                    kickoff_marker,
                    complete_marker,
                    output_surface_signature: signature,
                    output_surface_nonzero_samples: nonzero,
                    submit_completed,
                    present_attempted: true,
                    present_ready: ready,
                    synthetic_preview: false,
                };
                crate::log!(
                    "intel/media: decoded output ready engine={} frame={}x{} pitch=0x{:X} bitstream_bytes={} output_sig=0x{:08X} output_nonzero_samples={} present_ready={} submit_completed={} synthetic_preview=0 transport={:?}\n",
                    demo.engine_name,
                    demo.frame_width,
                    demo.frame_height,
                    demo.output_surface_pitch,
                    demo.bitstream_bytes,
                    demo.output_surface_signature,
                    demo.output_surface_nonzero_samples,
                    demo.present_ready as u8,
                    demo.submit_completed as u8,
                    preferred_transport()
                );
                store_kickoff_state(MediaKickoffStage::Smoke, Some(demo));
                return;
            }

            crate::log!(
                "intel/media: decoded output rejected engine={} frame={}x{} pitch=0x{:X} bitstream_bytes={} output_sig=0x{:08X} output_nonzero_samples={} present_ready={} submit_completed={} fallback=white-noise-preview\n",
                engine.name,
                summary.width,
                summary.height,
                decoded_pitch,
                annex_b.bytes.len(),
                signature,
                nonzero,
                ready as u8,
                submit_completed as u8,
            );
        }
        None => {
            crate::log!(
                "intel/media: decode submit unavailable engine={} fallback=white-noise-preview\n",
                engine.name,
            );
        }
    }

    synthetic_preview = true;
    used_output_bytes = preview_output_bytes;
    output_surface_gpu_addr = windows.output_surface_gpu_addr;
    output_surface_phys = backing.output_surface_phys;
    output_surface_virt = backing.output_surface_virt;

    render_whitenoise_preview_into_output(
        output_surface_virt,
        preview_pitch,
        preview_w,
        preview_h,
        annex_b.bytes.as_slice(),
        annex_b.has_idr,
    );
    super::dma_flush(output_surface_virt, used_output_bytes);

    let output_surface =
        unsafe { core::slice::from_raw_parts(output_surface_virt as *const u8, used_output_bytes) };
    let present_ready = super::display::present_rgba_surface_center(
        output_surface,
        preview_w as u32,
        preview_h as u32,
        preview_pitch,
    );
    let (output_surface_signature, output_surface_nonzero_samples) =
        sample_surface_signature(output_surface);

    let demo = MediaBitstreamDemoState {
        ready: present_ready,
        engine_name: engine.name,
        ring_gpu_addr: windows.ring_gpu_addr,
        context_gpu_addr: windows.context_gpu_addr,
        batch_gpu_addr: windows.batch_gpu_addr,
        result_gpu_addr: windows.result_gpu_addr,
        bitstream_gpu_addr: windows.bitstream_gpu_addr,
        output_surface_gpu_addr,
        bitstream_phys: backing.bitstream_phys,
        output_surface_phys,
        bitstream_bytes: annex_b.bytes.len(),
        output_surface_bytes: used_output_bytes,
        frame_width: preview_w as u16,
        frame_height: preview_h as u16,
        output_surface_pitch: preview_pitch,
        sample_nal_count: annex_b.sample_nal_count,
        has_idr: annex_b.has_idr,
        kickoff_marker,
        complete_marker,
        output_surface_signature,
        output_surface_nonzero_samples,
        submit_completed,
        present_attempted: true,
        present_ready,
        synthetic_preview,
    };

    crate::log!(
        "intel/media: white-noise preview fallback engine={} preview={}x{} pitch=0x{:X} bitstream_bytes={} output_sig=0x{:08X} output_nonzero_samples={} present_ready={} submit_completed={} synthetic_preview=1 transport={:?}\n",
        demo.engine_name,
        demo.frame_width,
        demo.frame_height,
        demo.output_surface_pitch,
        demo.bitstream_bytes,
        demo.output_surface_signature,
        demo.output_surface_nonzero_samples,
        demo.present_ready as u8,
        demo.submit_completed as u8,
        preferred_transport()
    );

    store_kickoff_state(MediaKickoffStage::Smoke, Some(demo));
}
