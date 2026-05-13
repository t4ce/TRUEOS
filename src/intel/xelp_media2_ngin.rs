extern crate alloc;

use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

use super::xelp_media_h264src::parse_h264_source_summary;
use super::xelp_media_mp4::{
    AnnexBAccessUnit, H264VclInfo, ParsedPps, ParsedSps, h264_crop_offsets_px, parse_pps,
    parse_sample_vcl_info, parse_sps, visible_h264_frame_dims, write_annex_b_for_sample,
};
use super::xelp_media_source;

const MAX_MEDIA_ENGINES: usize = 4;
const MAX_MEDIA_API_ROUTES: usize = 4;
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
const MEDIA_ENGINE_GPU_ADDR_STRIDE: u64 = 0x0100_0000;
const MEDIA_DEFAULT_RING_BYTES: usize = 16 * 1024;
const MEDIA_DEFAULT_CONTEXT_BYTES: usize = 22 * 4096;
const MEDIA_DEFAULT_BATCH_BYTES: usize = 32 * 1024;
const MEDIA_DEFAULT_RESULT_BYTES: usize = 4 * 4096;
const MEDIA_DEFAULT_BITSTREAM_BYTES: usize = 8 * 1024 * 1024;
const MEDIA_DEFAULT_OUTPUT_SURFACE_BYTES: usize = 16 * 1024 * 1024;
const MEDIA_DEFAULT_SCRATCH_BYTES: usize = 256 * 1024;
const MEDIA_SCRATCH_OFFSET_BYTES: usize = MEDIA_DEFAULT_SCRATCH_BYTES;
const MEDIA_SUBMIT_POLL_ITERS: usize = 100_000;

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
const CTX_DESC_PPGTT_ENABLE: u32 = 1 << 5;
const CTX_DESC_PRIVILEGE: u32 = 1 << 8;
const CTX_DESC_PRIORITY_NORMAL: u32 = 1 << 9;
const CTX_DESC_ADDRESSING_MODE_SHIFT: u32 = 3;
const INTEL_LEGACY_64B_CONTEXT: u32 = 3;
const GEN11_GFX_DISABLE_LEGACY_MODE: u32 = 1 << 3;
/// Enable ExecList submission mode (i915: GFX_RUN_LIST_ENABLE).  Without
/// this the scheduler stays in legacy ring-buffer mode and never promotes
/// pending SQ entries to active.  GuC firmware sets it during boot; GDRST
/// clears it, so we must re-set it after every engine reset.
const GFX_RUN_LIST_ENABLE: u32 = 1 << 15;
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
// MFX_AVC_SLICE_STATE: SubOpcodeA=0, SubOpcodeB=3, MediaCmdOpcode=1, length=11 (bias 2)
const MFX_AVC_SLICE_STATE: u32 = 3;
const MFX_CMD_LEN_AVC_SLICE_STATE: u32 = 9;
// MFX_AVC_REF_IDX_STATE: SubOpcodeA=0, SubOpcodeB=4, MediaCmdOpcode=1, length=10 (bias 2)
const MFX_AVC_REF_IDX_STATE: u32 = 4;
const MFX_CMD_LEN_AVC_REF_IDX_STATE: u32 = 8;
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
static MEDIA_DECODE_RAN: AtomicBool = AtomicBool::new(false);
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
pub(crate) struct MediaSurfaceProbeBand {
    pub signature: u32,
    pub active_samples: usize,
    pub sample_count: usize,
    pub min_value: u8,
    pub max_value: u8,
}

impl MediaSurfaceProbeBand {
    const fn empty() -> Self {
        Self {
            signature: 0,
            active_samples: 0,
            sample_count: 0,
            min_value: 0,
            max_value: 0,
        }
    }

    fn has_range(self) -> bool {
        self.sample_count != 0 && self.min_value != self.max_value
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct MediaSurfaceProbe {
    pub valid: bool,
    pub luma_visible_last_row: MediaSurfaceProbeBand,
    pub luma_prev_mb_row: MediaSurfaceProbeBand,
    pub luma_bottom_mb_row: MediaSurfaceProbeBand,
    pub chroma_prev_mb_row: MediaSurfaceProbeBand,
    pub chroma_bottom_mb_row: MediaSurfaceProbeBand,
}

impl MediaSurfaceProbe {
    const fn empty() -> Self {
        Self {
            valid: false,
            luma_visible_last_row: MediaSurfaceProbeBand::empty(),
            luma_prev_mb_row: MediaSurfaceProbeBand::empty(),
            luma_bottom_mb_row: MediaSurfaceProbeBand::empty(),
            chroma_prev_mb_row: MediaSurfaceProbeBand::empty(),
            chroma_bottom_mb_row: MediaSurfaceProbeBand::empty(),
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct MediaDecodeFrameState {
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
    pub output_surface_probe: MediaSurfaceProbe,
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
    pub last_decode_frame: Option<MediaDecodeFrameState>,
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

#[derive(Copy, Clone, Debug)]
pub(crate) struct Media2FirstFrameState {
    pub ready: bool,
    pub submit_completed: bool,
    pub present_ready: bool,
    pub frame_width: u16,
    pub frame_height: u16,
    pub output_surface_pitch: usize,
    pub output_surface_bytes: usize,
    pub output_surface_signature: u32,
    pub output_surface_nonzero_samples: usize,
    pub output_surface_probe: MediaSurfaceProbe,
    pub bitstream_bytes: usize,
    pub sample_nal_count: usize,
    pub has_idr: bool,
}

#[derive(Copy, Clone)]
pub(super) struct MediaBitstreamBacking {
    pub(super) ring_phys: u64,
    pub(super) ring_virt: *mut u8,
    pub(super) ring_bytes: usize,
    pub(super) context_phys: u64,
    pub(super) context_virt: *mut u8,
    pub(super) context_bytes: usize,
    pub(super) batch_phys: u64,
    pub(super) batch_virt: *mut u8,
    pub(super) batch_bytes: usize,
    pub(super) result_phys: u64,
    pub(super) result_virt: *mut u8,
    pub(super) result_bytes: usize,
    pub(super) bitstream_phys: u64,
    pub(super) bitstream_virt: *mut u8,
    pub(super) bitstream_bytes: usize,
    pub(super) output_surface_phys: u64,
    pub(super) output_surface_virt: *mut u8,
    pub(super) output_surface_bytes: usize,
    pub(super) ppgtt_pml4_phys: u64,
}

unsafe impl Send for MediaBitstreamBacking {}
unsafe impl Sync for MediaBitstreamBacking {}

#[derive(Copy, Clone, Debug)]
pub(super) struct MediaEncodedStreamProof {
    pub engine_name: &'static str,
    pub bitstream_gpu_addr: u64,
    pub bitstream_phys: u64,
    pub bitstream_virt: usize,
    pub bytes_written: usize,
    pub capacity: usize,
    pub signature: u32,
    pub forcewake_global_ack: u32,
    pub forcewake_awake_count: usize,
}

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
        result_gpu_addr: base + 0x00A0_0000,
    }
}

pub(super) fn default_decode_engine_and_window() -> (MediaEngineDescriptor, MediaGpuWindowLayout) {
    let topology = current_topology();
    let engine = topology
        .default_decode
        .map(|_| topology.engines[0])
        .unwrap_or(topology.engines[0]);
    (engine, engine_window(0))
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
    demo: Option<MediaDecodeFrameState>,
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
        last_decode_frame: demo,
    })
}

fn store_kickoff_state(stage: MediaKickoffStage, demo: Option<MediaDecodeFrameState>) {
    *MEDIA_KICKOFF_STATE.lock() = rebuild_kickoff_state(stage, demo);
}

fn media2_first_frame_state(demo: MediaDecodeFrameState) -> Media2FirstFrameState {
    Media2FirstFrameState {
        ready: demo.ready,
        submit_completed: demo.submit_completed,
        present_ready: demo.present_ready,
        frame_width: demo.frame_width,
        frame_height: demo.frame_height,
        output_surface_pitch: demo.output_surface_pitch,
        output_surface_bytes: demo.output_surface_bytes,
        output_surface_signature: demo.output_surface_signature,
        output_surface_nonzero_samples: demo.output_surface_nonzero_samples,
        output_surface_probe: demo.output_surface_probe,
        bitstream_bytes: demo.bitstream_bytes,
        sample_nal_count: demo.sample_nal_count,
        has_idr: demo.has_idr,
    }
}

fn marker_base(engine: MediaEngineDescriptor) -> u32 {
    let class_base = match engine.id.class {
        MediaEngineClass::VideoDecode => 0x4D44_1000,
        MediaEngineClass::VideoEnhancement => 0x4D45_1000,
    };
    class_base + (engine.id.instance as u32) * 0x100
}

fn surface_signature(bytes: &[u8]) -> (u32, usize) {
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

fn byte_signature(bytes: &[u8]) -> u32 {
    let mut signature = 0u32;
    for &value in bytes.iter().take(4096) {
        signature = signature.rotate_left(5) ^ u32::from(value);
    }
    signature
}

const MEDIA_YTILE_W: usize = 128;
const MEDIA_YTILE_H: usize = 32;

#[inline(always)]
fn media_ytile_offset(byte_x: usize, row_y: usize, tiles_per_row: usize) -> usize {
    let tile_col = byte_x / MEDIA_YTILE_W;
    let tile_row = row_y / MEDIA_YTILE_H;
    let in_x = byte_x % MEDIA_YTILE_W;
    let in_y = row_y % MEDIA_YTILE_H;
    let oword_col = in_x / 16;
    let byte_in_oword = in_x % 16;
    let within_tile = oword_col * 512 + in_y * 16 + byte_in_oword;
    (tile_row * tiles_per_row + tile_col) * 4096 + within_tile
}

fn probe_tiled_rect(
    surface: &[u8],
    output_pitch: usize,
    byte_x: usize,
    row_y: usize,
    width: usize,
    row_count: usize,
    baseline: u8,
) -> Option<MediaSurfaceProbeBand> {
    if width == 0 || row_count == 0 || output_pitch < byte_x.saturating_add(width) {
        return None;
    }
    let tiles_per_row = output_pitch / MEDIA_YTILE_W;
    if tiles_per_row == 0 {
        return None;
    }

    let mut signature = 0u32;
    let mut active_samples = 0usize;
    let mut sample_count = 0usize;
    let mut min_value = u8::MAX;
    let mut max_value = u8::MIN;
    for row in row_y..row_y.saturating_add(row_count) {
        for col in byte_x..byte_x.saturating_add(width) {
            let offset = media_ytile_offset(col, row, tiles_per_row);
            let value = *surface.get(offset)?;
            signature = signature.rotate_left(5) ^ u32::from(value);
            active_samples += usize::from(value != baseline);
            sample_count += 1;
            min_value = min_value.min(value);
            max_value = max_value.max(value);
        }
    }

    Some(MediaSurfaceProbeBand {
        signature,
        active_samples,
        sample_count,
        min_value,
        max_value,
    })
}

#[inline]
fn luma_band_to_chroma_band(luma_row: usize, luma_rows: usize) -> (usize, usize) {
    let chroma_row = luma_row / 2;
    let chroma_end = (luma_row.saturating_add(luma_rows).saturating_add(1)) / 2;
    (chroma_row, chroma_end.saturating_sub(chroma_row))
}

fn probe_output_surface(
    output_surface: &[u8],
    coded_width: u16,
    coded_height: u16,
    visible_x: u16,
    visible_y: u16,
    visible_width: u16,
    visible_height: u16,
    output_pitch: usize,
) -> MediaSurfaceProbe {
    let coded_width = coded_width as usize;
    let coded_height = coded_height as usize;
    let visible_x = visible_x as usize;
    let visible_y = visible_y as usize;
    let visible_width = visible_width as usize;
    let visible_height = visible_height as usize;

    if coded_width == 0
        || coded_height == 0
        || visible_width == 0
        || visible_height == 0
        || output_pitch < coded_width
    {
        return MediaSurfaceProbe::empty();
    }

    let visible_bottom = visible_y.saturating_add(visible_height).min(coded_height);
    if visible_x.saturating_add(visible_width) > coded_width || visible_bottom <= visible_y {
        return MediaSurfaceProbe::empty();
    }

    let bottom_luma_rows = coded_height.min(16);
    let bottom_luma_row = coded_height.saturating_sub(bottom_luma_rows);
    let prev_luma_rows = bottom_luma_row.min(16);
    let prev_luma_row = bottom_luma_row.saturating_sub(prev_luma_rows);
    let visible_last_row = visible_bottom.saturating_sub(1);

    let chroma_y_offset = (coded_height + MEDIA_YTILE_H - 1) & !(MEDIA_YTILE_H - 1);
    let (prev_chroma_row, prev_chroma_rows) =
        luma_band_to_chroma_band(prev_luma_row, prev_luma_rows);
    let (bottom_chroma_row, bottom_chroma_rows) =
        luma_band_to_chroma_band(bottom_luma_row, bottom_luma_rows);

    let luma_visible_last_row = probe_tiled_rect(
        output_surface,
        output_pitch,
        visible_x,
        visible_last_row,
        visible_width,
        1,
        0,
    );
    let luma_prev_mb_row = probe_tiled_rect(
        output_surface,
        output_pitch,
        0,
        prev_luma_row,
        coded_width,
        prev_luma_rows,
        0,
    );
    let luma_bottom_mb_row = probe_tiled_rect(
        output_surface,
        output_pitch,
        0,
        bottom_luma_row,
        coded_width,
        bottom_luma_rows,
        0,
    );
    let chroma_prev_mb_row = probe_tiled_rect(
        output_surface,
        output_pitch,
        0,
        chroma_y_offset.saturating_add(prev_chroma_row),
        coded_width,
        prev_chroma_rows,
        0x80,
    );
    let chroma_bottom_mb_row = probe_tiled_rect(
        output_surface,
        output_pitch,
        0,
        chroma_y_offset.saturating_add(bottom_chroma_row),
        coded_width,
        bottom_chroma_rows,
        0x80,
    );

    let valid = luma_visible_last_row.is_some()
        && luma_prev_mb_row.is_some()
        && luma_bottom_mb_row.is_some()
        && chroma_prev_mb_row.is_some()
        && chroma_bottom_mb_row.is_some();

    MediaSurfaceProbe {
        valid,
        luma_visible_last_row: luma_visible_last_row.unwrap_or_else(MediaSurfaceProbeBand::empty),
        luma_prev_mb_row: luma_prev_mb_row.unwrap_or_else(MediaSurfaceProbeBand::empty),
        luma_bottom_mb_row: luma_bottom_mb_row.unwrap_or_else(MediaSurfaceProbeBand::empty),
        chroma_prev_mb_row: chroma_prev_mb_row.unwrap_or_else(MediaSurfaceProbeBand::empty),
        chroma_bottom_mb_row: chroma_bottom_mb_row.unwrap_or_else(MediaSurfaceProbeBand::empty),
    }
}

fn log_output_surface_probe(
    engine_name: &'static str,
    sample_idx: u32,
    submit_completed: bool,
    probe: MediaSurfaceProbe,
) {
    if !probe.valid {
        crate::log!(
            "intel/media2: output-probe phase=pre-present engine={} sample={} submit_completed={} valid=false\n",
            engine_name,
            sample_idx,
            submit_completed
        );
        return;
    }

    crate::log!(
        "intel/media2: output-probe phase=pre-present engine={} sample={} submit_completed={} y_last(sig=0x{:08X} active={}/{} range={}..{}) y_prev_mb(sig=0x{:08X} active={}/{} range={}..{}) y_bottom_mb(sig=0x{:08X} active={}/{} range={}..{}) uv_prev_mb(sig=0x{:08X} active={}/{} range={}..{}) uv_bottom_mb(sig=0x{:08X} active={}/{} range={}..{})\n",
        engine_name,
        sample_idx,
        submit_completed,
        probe.luma_visible_last_row.signature,
        probe.luma_visible_last_row.active_samples,
        probe.luma_visible_last_row.sample_count,
        probe.luma_visible_last_row.min_value,
        probe.luma_visible_last_row.max_value,
        probe.luma_prev_mb_row.signature,
        probe.luma_prev_mb_row.active_samples,
        probe.luma_prev_mb_row.sample_count,
        probe.luma_prev_mb_row.min_value,
        probe.luma_prev_mb_row.max_value,
        probe.luma_bottom_mb_row.signature,
        probe.luma_bottom_mb_row.active_samples,
        probe.luma_bottom_mb_row.sample_count,
        probe.luma_bottom_mb_row.min_value,
        probe.luma_bottom_mb_row.max_value,
        probe.chroma_prev_mb_row.signature,
        probe.chroma_prev_mb_row.active_samples,
        probe.chroma_prev_mb_row.sample_count,
        probe.chroma_prev_mb_row.min_value,
        probe.chroma_prev_mb_row.max_value,
        probe.chroma_bottom_mb_row.signature,
        probe.chroma_bottom_mb_row.active_samples,
        probe.chroma_bottom_mb_row.sample_count,
        probe.chroma_bottom_mb_row.min_value,
        probe.chroma_bottom_mb_row.max_value
    );
}

fn output_surface_has_decoded_detail(probe: &MediaSurfaceProbe) -> bool {
    probe.valid
        && (probe.luma_visible_last_row.has_range()
            || probe.luma_prev_mb_row.has_range()
            || probe.luma_bottom_mb_row.has_range()
            || probe.chroma_prev_mb_row.has_range()
            || probe.chroma_bottom_mb_row.has_range())
}

fn present_nv12_frame(
    output_surface: &[u8],
    coded_width: u16,
    coded_height: u16,
    visible_x: u16,
    visible_y: u16,
    frame_width: u16,
    frame_height: u16,
    output_pitch: usize,
    submit_completed: bool,
) -> (bool, u32, usize) {
    let (signature, nonzero_samples) = surface_signature(output_surface);

    // Y-tile NV12: chroma starts at tile-row-aligned height (matches batch builder).
    let chroma_y_aligned = ((coded_height as usize) + 31) & !31;
    let total_height = chroma_y_aligned + ((coded_height as usize) + 1) / 2;
    let total_tile_rows = (total_height + 31) & !31;
    if frame_width != 0
        && frame_height != 0
        && output_pitch >= coded_width as usize
        && output_surface.len() >= output_pitch.saturating_mul(total_tile_rows)
        && submit_completed
    {
        let ready = super::display::present_nv12_surface_center(
            output_surface,
            coded_width as u32,
            coded_height as u32,
            visible_x as u32,
            visible_y as u32,
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

pub(super) fn decode_and_present_frame(
    dev: crate::intel::Dev,
    engine: MediaEngineDescriptor,
    windows: MediaGpuWindowLayout,
    backing: MediaBitstreamBacking,
    frame_width: u16,
    frame_height: u16,
    annex_b: &AnnexBAccessUnit,
    vcl_info: Option<H264VclInfo>,
    sps: &ParsedSps,
    pps: &ParsedPps,
    sample_idx: u32,
    ref_surface_idx: u32,
) -> Option<MediaDecodeFrameState> {
    let kickoff_marker = marker_base(engine);
    let complete_marker = kickoff_marker + 3;
    let (
        submit_completed,
        output_surface_pitch,
        output_surface_bytes,
        output_surface_gpu_addr,
        output_surface_phys,
        output_surface_virt,
    ) = submit_h264_frame(
        dev,
        engine,
        windows,
        backing,
        frame_width,
        frame_height,
        annex_b,
        vcl_info,
        sps,
        pps,
        sample_idx,
        ref_surface_idx,
    )?;

    let output_surface = unsafe {
        core::slice::from_raw_parts(output_surface_virt as *const u8, output_surface_bytes)
    };
    let (coded_width, coded_height) = coded_h264_frame_dims(sps);
    let (visible_x, visible_y) = h264_crop_offsets_px(sps);
    let output_surface_probe = probe_output_surface(
        output_surface,
        u16::try_from(coded_width).unwrap_or(u16::MAX),
        u16::try_from(coded_height).unwrap_or(u16::MAX),
        u16::try_from(visible_x).unwrap_or(0),
        u16::try_from(visible_y).unwrap_or(0),
        frame_width,
        frame_height,
        output_surface_pitch,
    );
    log_output_surface_probe(engine.name, sample_idx, submit_completed, output_surface_probe);
    let decoded_detail = output_surface_has_decoded_detail(&output_surface_probe);
    let (present_ready, output_surface_signature, output_surface_nonzero_samples) =
        if decoded_detail {
            present_nv12_frame(
                output_surface,
                u16::try_from(coded_width).unwrap_or(u16::MAX),
                u16::try_from(coded_height).unwrap_or(u16::MAX),
                u16::try_from(visible_x).unwrap_or(0),
                u16::try_from(visible_y).unwrap_or(0),
                frame_width,
                frame_height,
                output_surface_pitch,
                submit_completed,
            )
        } else {
            let (signature, nonzero_samples) = surface_signature(output_surface);
            crate::log!(
                "intel/media2: first-frame blank-surface engine={} sample={} submit_completed={} detail_range=0 present_skipped=1 sig=0x{:08X} nonzero_samples={}\n",
                engine.name,
                sample_idx,
                submit_completed as u8,
                signature,
                nonzero_samples,
            );
            (false, signature, nonzero_samples)
        };

    Some(MediaDecodeFrameState {
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
        bitstream_bytes: annex_b.bytes_written,
        output_surface_bytes,
        frame_width,
        frame_height,
        output_surface_pitch,
        sample_nal_count: annex_b.sample_nal_count,
        has_idr: annex_b.has_idr,
        kickoff_marker,
        complete_marker,
        output_surface_signature,
        output_surface_nonzero_samples,
        output_surface_probe,
        submit_completed,
        present_attempted: decoded_detail,
        present_ready,
        synthetic_preview: false,
    })
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
fn masked_bit_disable(bit: u32) -> u32 {
    bit << 16
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
    ring_offset: usize,
    result_gpu_addr: u64,
    prelaunch_marker: u32,
    batch_gpu_addr: u64,
) -> Option<usize> {
    if ring_virt.is_null() || ring_offset + 40 > ring_bytes {
        return None;
    }
    let base = unsafe { ring_virt.add(ring_offset) };
    // Gen8+ execlists requests need a post-batch preemption point, and the
    // final RING_TAIL must remain qword aligned.
    let dwords = unsafe { core::slice::from_raw_parts_mut(base as *mut u32, 10) };
    dwords[0] = MI_STORE_DWORD_IMM_GEN4_LEN_DW4;
    dwords[1] = (result_gpu_addr + MEDIA_RESULT_KICKOFF_SLOT) as u32;
    dwords[2] = ((result_gpu_addr + MEDIA_RESULT_KICKOFF_SLOT) >> 32) as u32;
    dwords[3] = prelaunch_marker;
    dwords[4] = MI_BATCH_BUFFER_START_GEN8;
    dwords[5] = batch_gpu_addr as u32;
    dwords[6] = (batch_gpu_addr >> 32) as u32;
    dwords[7] = MI_ARB_CHECK;
    dwords[8] = MI_NOOP;
    dwords[9] = MI_NOOP;
    Some(ring_offset + 40)
}

fn ring_ctl_value_for_size(size: usize) -> Option<u32> {
    let size = u32::try_from(size).ok()?;
    Some(size.checked_sub(4096)? | 1)
}

fn build_execlist_context_descriptor_for_gpu_addr(context_gpu_addr: u64) -> (u32, u32) {
    let base = (context_gpu_addr as u32) & 0xFFFF_F000;
    (
        base | CTX_DESC_VALID
            | CTX_DESC_PPGTT_ENABLE
            | CTX_DESC_PRIVILEGE
            | CTX_DESC_PRIORITY_NORMAL
            | (INTEL_LEGACY_64B_CONTEXT << CTX_DESC_ADDRESSING_MODE_SHIFT),
        (context_gpu_addr >> 32) as u32,
    )
}

fn media_sw_context_id_for_submit(context_gpu_addr: u64) -> u32 {
    let sw_context_id = ((context_gpu_addr >> 12) as u32) & 0x7FF;
    if sw_context_id == 0 { 1 } else { sw_context_id }
}

fn build_media_execlist_context_descriptor(
    context_gpu_addr: u64,
    engine: MediaEngineDescriptor,
    sw_counter: u32,
    force_restore: bool,
) -> (u32, u32) {
    let (lo, mut hi) = build_execlist_context_descriptor_for_gpu_addr(context_gpu_addr);
    let mut lo = lo;
    // Pre-DG2 Gen12 descriptors include engine class [63:61] and instance [53:48].
    let class = match engine.id.class {
        MediaEngineClass::VideoDecode => 1u32,
        MediaEngineClass::VideoEnhancement => 2u32,
    };
    if force_restore {
        lo |= CTX_DESC_FORCE_RESTORE;
    }
    hi |= (media_sw_context_id_for_submit(context_gpu_addr) & 0x7FF) << 5;
    hi |= (engine.id.instance as u32 & 0x3F) << 16;
    hi |= (sw_counter & 0x3F) << 23;
    hi |= (class & 0x7) << 29;
    (lo, hi)
}

fn media_ctx_control_value(inhibit_restore: bool) -> u32 {
    let mut ctl =
        masked_bits_update(CTX_CTRL_INHIBIT_SYN_CTX_SWITCH, CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT);
    if inhibit_restore {
        ctl |= CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT;
    }
    ctl
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

fn write_video_lrc_ring_head(context_virt: *mut u8, context_len: usize, ring_head: u32) {
    const LRC_STATE_OFFSET_DWORDS: usize = 4096 / core::mem::size_of::<u32>();
    const CTX_RING_HEAD_DW: usize = 5;

    if context_virt.is_null() {
        return;
    }
    let total_dwords = context_len / core::mem::size_of::<u32>();
    if total_dwords <= LRC_STATE_OFFSET_DWORDS + CTX_RING_HEAD_DW {
        return;
    }

    let dwords = unsafe { core::slice::from_raw_parts_mut(context_virt as *mut u32, total_dwords) };
    dwords[LRC_STATE_OFFSET_DWORDS + CTX_RING_HEAD_DW] = ring_head;
    super::dma_flush(context_virt, context_len);
}

fn write_video_lrc_context_control(context_virt: *mut u8, context_len: usize, ctx_ctl: u32) {
    const LRC_STATE_OFFSET_DWORDS: usize = 4096 / core::mem::size_of::<u32>();
    const CTX_CONTEXT_CONTROL_DW: usize = 3;

    if context_virt.is_null() {
        return;
    }
    let total_dwords = context_len / core::mem::size_of::<u32>();
    if total_dwords <= LRC_STATE_OFFSET_DWORDS + CTX_CONTEXT_CONTROL_DW {
        return;
    }

    let dwords = unsafe { core::slice::from_raw_parts_mut(context_virt as *mut u32, total_dwords) };
    dwords[LRC_STATE_OFFSET_DWORDS + CTX_CONTEXT_CONTROL_DW] = ctx_ctl;
    super::dma_flush(context_virt, context_len);
}

fn prepare_video_lrc_for_submit(
    context_virt: *mut u8,
    context_len: usize,
    ring_head: u32,
    ring_tail: u32,
    ctx_ctl: u32,
) {
    write_video_lrc_ring_head(context_virt, context_len, ring_head);
    write_video_lrc_ring_tail(context_virt, context_len, ring_tail);
    write_video_lrc_context_control(context_virt, context_len, ctx_ctl);
}

fn read_video_lrc_slot(context_virt: *mut u8, context_len: usize, slot_dw: usize) -> u32 {
    const LRC_STATE_OFFSET_DWORDS: usize = 4096 / core::mem::size_of::<u32>();

    if context_virt.is_null() {
        return 0;
    }
    let total_dwords = context_len / core::mem::size_of::<u32>();
    if total_dwords <= LRC_STATE_OFFSET_DWORDS + slot_dw {
        return 0;
    }

    let dwords = unsafe { core::slice::from_raw_parts(context_virt as *const u32, total_dwords) };
    dwords[LRC_STATE_OFFSET_DWORDS + slot_dw]
}

fn init_gen12_video_context_image(
    context_virt: *mut u8,
    context_len: usize,
    ring_base: usize,
    ring_head: u32,
    ring_start: u32,
    ring_tail: u32,
    ring_ctl: u32,
    _hws_pga: u32,
    pml4_phys: u64,
    inhibit_restore: bool,
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
    if state.len() < 112 {
        return false;
    }
    let ring_base = ring_base as u32;
    let mut idx = 0usize;
    state[idx] = MI_NOOP;
    idx += 1;
    state[idx] = mi_lri_cmd(13, MI_LRI_FORCE_POSTED);
    idx += 1;
    state[idx] = ring_base + 0x244;
    // Gen12 media follows the reference LRC template: inhibit synchronous
    // context switch, and use restore-inhibit for empty/default contexts.
    state[idx + 1] = media_ctx_control_value(inhibit_restore);
    state[idx + 2] = ring_base + 0x34;
    state[idx + 3] = ring_head;
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
    // Keep MI_MODE aligned with the cold-start path by explicitly clearing
    // STOP_RING from the restored context image on every replay.
    state[idx] = mi_lri_cmd(1, MI_LRI_FORCE_POSTED);
    idx += 1;
    state[idx] = ring_base + 0x9C;
    state[idx + 1] = masked_bit_disable(STOP_RING);
    idx += 2;
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

#[allow(clippy::too_many_arguments)]
fn log_media2_batch_geometry(
    coded_width: u32,
    coded_height: u32,
    visible_width: u32,
    visible_height: u32,
    width_mbs: u32,
    pic_height_mbs: u32,
    output_pitch: u32,
    output_surface_bytes: usize,
    surface_dw2: u32,
    surface_dw3: u32,
    chroma_y_offset: u32,
    pipe_post_attr: u32,
    pipe_ref_attr: u32,
    intra_row_addr: u64,
    intra_row_attr: u32,
    deblock_row_addr: u64,
    deblock_row_attr: u32,
    bsd_row_addr: u64,
    bsd_row_attr: u32,
    mpr_row_addr: u64,
    mpr_row_attr: u32,
) {
    if !crate::logflag::INTEL_MEDIA_NGIN_LOGS {
        return;
    }

    let tiles_per_row = output_pitch / MEDIA_YTILE_W as u32;
    let frame_mbs_only = pic_height_mbs == coded_height / 16;
    let intra_cls = width_mbs;
    let deblock_cls = if frame_mbs_only {
        width_mbs.saturating_mul(2)
    } else {
        width_mbs.saturating_mul(4)
    };
    let bsd_cls = if frame_mbs_only {
        width_mbs
    } else {
        width_mbs.saturating_mul(2)
    };
    let mpr_cls = bsd_cls;
    let row_total_cls = intra_cls
        .saturating_add(deblock_cls)
        .saturating_add(bsd_cls)
        .saturating_add(mpr_cls);

    crate::log!(
        "intel/media2: batch-geometry coded={}x{} visible={}x{} mbs={}x{} pitch={} tiles_per_row={} bytes=0x{:X} surface_dw2=0x{:08X} surface_dw3=0x{:08X} chroma_y={} post_attr=0x{:08X} ref_attr=0x{:08X} rowstore_cls(intra={} deblock={} bsd={} mpr={} total={}/640) rowstore_ext(intra=0x{:X}/0x{:08X} deblock=0x{:X}/0x{:08X} bsd=0x{:X}/0x{:08X} mpr=0x{:X}/0x{:08X}) luma_only={}\n",
        coded_width,
        coded_height,
        visible_width,
        visible_height,
        width_mbs,
        pic_height_mbs,
        output_pitch,
        tiles_per_row,
        output_surface_bytes,
        surface_dw2,
        surface_dw3,
        chroma_y_offset,
        pipe_post_attr,
        pipe_ref_attr,
        intra_cls,
        deblock_cls,
        bsd_cls,
        mpr_cls,
        row_total_cls,
        intra_row_addr,
        intra_row_attr,
        deblock_row_addr,
        deblock_row_attr,
        bsd_row_addr,
        bsd_row_attr,
        mpr_row_addr,
        mpr_row_attr,
        crate::logflag::INTEL_MEDIA_PRESENT_LUMA_ONLY
    );
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

fn coded_h264_frame_dims(sps: &ParsedSps) -> (u32, u32) {
    let width_mbs = sps.pic_width_in_mbs_minus1 + 1;
    let pic_height_map_units = sps.pic_height_in_map_units_minus1 + 1;
    let frame_height_mbs = if sps.frame_mbs_only_flag {
        pic_height_map_units
    } else {
        pic_height_map_units.saturating_mul(2)
    };
    (width_mbs.saturating_mul(16), frame_height_mbs.saturating_mul(16))
}

fn build_h264_decode_batch_skeleton(
    batch_virt: *mut u8,
    batch_bytes: usize,
    result_gpu_addr: u64,
    bitstream_gpu_addr: u64,
    output_surface_gpu_addr: u64,
    ref_surface_gpu_addr: u64,
    scratch_gpu_addr: u64,
    output_surface_bytes: usize,
    frame_width: u16,
    frame_height: u16,
    annexb_bytes: usize,
    sample_nal_count: usize,
    has_idr: bool,
    idr_nal_offset: usize,
    idr_nal_length: usize,
    vcl_info: Option<H264VclInfo>,
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
    let visible_width = frame_width as u32;
    let visible_height = frame_height as u32;
    let (coded_width, coded_height) = coded_h264_frame_dims(sps);
    let width_mbs = coded_width.saturating_add(15) / 16;
    let pic_height = if sps.frame_mbs_only_flag {
        sps.pic_height_in_map_units_minus1 + 1
    } else {
        (sps.pic_height_in_map_units_minus1 + 1) * 2
    };
    let frame_dims = visible_width | (visible_height << 16);
    let output_pitch = align_up_u32(coded_width.max(128), 128); // Y-tile: 128-byte tile width
    // Y-tile NV12: chroma plane must start on a 32-row tile boundary so the
    // HW doesn't share a physical tile row between Y bottom and UV top.
    let chroma_y_offset = align_up_u32(coded_height, 32);
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
    // Mesa and genxml both keep decode output on the post-deblock path.
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
    batch[surface + 2] =
        ((coded_width.saturating_sub(1)) << 4) | ((coded_height.saturating_sub(1)) << 18);
    // DW3: SurfaceFormat(31:28)=4(PLANAR_420_8/NV12), TiledSurface(27)=1,
    //       TileWalk(26)=1(Y-major), SurfacePitch-1(17:3),
    //       InterleaveChroma(1)=1: NV12 uses interleaved CbCr pairs.
    batch[surface + 3] =
        (1 << 1) | ((output_pitch.saturating_sub(1)) << 3) | (1 << 26) | (1 << 27) | (4 << 28);
    batch[surface + 4] = chroma_y_offset;
    batch[surface + 5] = chroma_y_offset; // Y Offset for V(Cr) = same as U(Cb)
    let surface_dw2 = batch[surface + 2];
    let surface_dw3 = batch[surface + 3];

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
    // DW1-2: Pre Deblocking Destination = output surface.
    // This matches the older first-frame probe that produced the tile-debug image.
    packet_write_addr64(batch, pipe_buf, 1, output_surface_gpu_addr);
    batch[pipe_buf + 3] = MFX_MOCS_UC;
    // DW4-5: Post Deblocking Destination = output surface
    packet_write_addr64(batch, pipe_buf, 4, output_surface_gpu_addr);
    batch[pipe_buf + 6] = MFX_MOCS_UC; // Post Deblocking Attributes
    // DW9: Original Uncompressed Picture Source Attributes
    batch[pipe_buf + 9] = MFX_MOCS_UC;
    // DW7-8: Original Uncompressed Picture Source = reference surface (previous frame)
    if !has_idr {
        packet_write_addr64(batch, pipe_buf, 7, ref_surface_gpu_addr);
    }
    // DW12: Stream-Out Data Destination Attributes
    batch[pipe_buf + 12] = MFX_MOCS_UC;
    // DW13-14: Intra Row Store Scratch Buffer (32KB at +0x00000)
    packet_write_addr64(batch, pipe_buf, 13, scratch_gpu_addr);
    batch[pipe_buf + 15] = MFX_MOCS_UC; // Intra Row Store Attributes
    // DW16-17: Deblocking Filter Row Store Scratch Buffer (32KB at +0x08000)
    // Gen12 needs width_in_mbs × 256 bytes; for 1280px = 80 MBs = 20KB.
    packet_write_addr64(batch, pipe_buf, 16, scratch_gpu_addr + 0x08000);
    batch[pipe_buf + 18] = MFX_MOCS_UC; // Deblocking Filter Row Store Attributes
    // DW19-50: Reference Picture Frame Store addresses (16 refs × 2 DWords each)
    //          For non-IDR frames, point ref 0 to the reference surface (previous decoded frame).
    if !has_idr {
        packet_write_addr64(batch, pipe_buf, 19, ref_surface_gpu_addr);
    }
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
    // BSD Row Store (32KB at +0x10000)
    packet_write_addr64(batch, bsp, 1, scratch_gpu_addr + 0x10000);
    batch[bsp + 3] = MFX_MOCS_UC; // BSD Row Store Attributes
    // MPR Row Store (32KB at +0x18000)
    packet_write_addr64(batch, bsp, 4, scratch_gpu_addr + 0x18000);
    batch[bsp + 6] = MFX_MOCS_UC; // MPR Row Store Attributes
    batch[bsp + 9] = MFX_MOCS_UC; // Bitplane Read Buffer Attributes

    log_media2_batch_geometry(
        coded_width,
        coded_height,
        visible_width,
        visible_height,
        width_mbs,
        pic_height,
        output_pitch,
        output_surface_bytes,
        surface_dw2,
        surface_dw3,
        chroma_y_offset,
        batch[pipe_buf + 6],
        batch[pipe_buf + 51],
        scratch_gpu_addr,
        batch[pipe_buf + 15],
        scratch_gpu_addr + 0x08000,
        batch[pipe_buf + 18],
        scratch_gpu_addr + 0x10000,
        batch[bsp + 3],
        scratch_gpu_addr + 0x18000,
        batch[bsp + 6],
    );

    // --- MFD_AVC_DPB_STATE (27 DWords) ---
    // For non-IDR frames, mark frame store 0 as a valid short-term reference
    // pointing to the same output surface (self-reference).  For IDR frames
    // the entire packet stays zeroed.
    let dpb = begin_batch_packet(
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
    if !has_idr {
        // DW1: FrameStore_ID[0]=0, NonExisting[0]=0 (valid), InUse(LongTerm)[0]=0 (short-term)
        // All other frame stores: NonExisting=1
        batch[dpb + 1] = 0x0000_FFFE; // bits[15:1]=1 → frame stores 1..15 non-existing
    }

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
    if has_idr {
        // All PicIDs invalid (0xFFFF)
        for dw in 2..10 {
            batch[picid + dw] = 0xFFFF_FFFF;
        }
    } else {
        // PicID[0] = 0 (valid), rest = 0xFFFF (invalid)
        batch[picid + 2] = 0xFFFF_0000;
        for dw in 3..10 {
            batch[picid + dw] = 0xFFFF_FFFF;
        }
    }

    // --- MFX_AVC_IMG_STATE (21 DWords) ---
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
        | ((vcl_info.map(|info| info.nal_ref_idc == 0).unwrap_or(false) as u32) << 6)
        | ((pps.entropy_coding_mode_flag as u32) << 7)
        | ((sps.chroma_format_idc & 3) << 10);
    // DW5: TrellisQuantizationChromaDisable(27)
    batch[avc_img + 5] = 1 << 27;
    // DW13: InitialQP(7:0), NumActiveRefL0(13:8), NumActiveRefL1(21:16),
    //        NumRefFrames(28:24)
    let programmed_ref_frames = if has_idr { 0 } else { 1 };
    batch[avc_img + 13] = ((pps.pic_init_qp_minus26 as u32) & 0xFF)
        | (((pps.num_ref_idx_l0_default_active_minus1 + 1) & 0x3F) << 8)
        | (((pps.num_ref_idx_l1_default_active_minus1 + 1) & 0x3F) << 16)
        | ((programmed_ref_frames & 0x1F) << 24);
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
    batch[avc_img + 15] = vcl_info
        .map(|info| (info.frame_num & 0xFFFF) << 16)
        .unwrap_or(0);

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

    // --- MFX_AVC_DIRECTMODE_STATE (71 DWords) ---
    // For non-IDR, set frame store 0's direct MV buffer to the scratch area
    // so the MFX pipe has a valid address even if direct mode isn't used.
    let directmode = begin_batch_packet(
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
    if !has_idr {
        // DW1-2: MV buffer for picture 0 (current) (32KB at +0x20000)
        packet_write_addr64(batch, directmode, 1, scratch_gpu_addr + 0x20000);
        // DW35-36: MV buffer for reference frame store 0
        packet_write_addr64(batch, directmode, 35, scratch_gpu_addr + 0x20000);
    }

    // Derived slice-level parameters
    let canonical_slice = vcl_info
        .map(|i| {
            if i.slice_type >= 5 {
                i.slice_type - 5
            } else {
                i.slice_type
            }
        })
        .unwrap_or(if has_idr { 2 } else { 0 });
    let is_p_or_b = canonical_slice == 0 || canonical_slice == 1 || canonical_slice == 3;
    let is_b = canonical_slice == 1;
    // --- MFX_AVC_REF_IDX_STATE (L0) ---
    if is_p_or_b && !has_idr {
        let ri0 = begin_batch_packet(
            batch,
            &mut idx,
            (MFX_CMD_LEN_AVC_REF_IDX_STATE + 2) as usize,
            media_cmd_header(
                MEDIA_CMD_OPCODE_MFX_AVC,
                0,
                MFX_AVC_REF_IDX_STATE,
                MFX_CMD_LEN_AVC_REF_IDX_STATE,
            ),
        )?;
        batch[ri0 + 1] = 0; // L0
        batch[ri0 + 2] = 0xFFFF_FF00; // entry[0]=ref store 0, rest=0xFF
        for dw in 3..=9 {
            batch[ri0 + dw] = 0xFFFF_FFFF;
        }
    }
    if is_b && !has_idr {
        let ri1 = begin_batch_packet(
            batch,
            &mut idx,
            (MFX_CMD_LEN_AVC_REF_IDX_STATE + 2) as usize,
            media_cmd_header(
                MEDIA_CMD_OPCODE_MFX_AVC,
                0,
                MFX_AVC_REF_IDX_STATE,
                MFX_CMD_LEN_AVC_REF_IDX_STATE,
            ),
        )?;
        batch[ri1 + 1] = 1; // L1
        batch[ri1 + 2] = 0xFFFF_FF00;
        for dw in 3..=9 {
            batch[ri1 + dw] = 0xFFFF_FFFF;
        }
    }

    // AVC decode is running in short-format mode (MFX_PIPE_MODE_SELECT bit17=0),
    // so the hardware derives slice locations from the bitstream/BSD objects.
    // PRM says MFX_AVC_SLICE_STATE is not issued for AVC short-format decode.

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
    // DW1: IndirectBSDDataLength = last-slice bytes from the current NAL header
    // through the end of the access unit. Mesa does the same for the final AVC
    // slice in short-format decode, which lets the BSD unit consume any trailing
    // bytes the encoder left after the slice payload.
    let bsd_data_len = annexb_bytes.saturating_sub(idr_nal_offset);
    batch[avc_bsd + 1] = bsd_data_len as u32;
    // DW2: IndirectBSDDataStartAddress = offset of IDR NAL within bitstream buffer
    batch[avc_bsd + 2] = (idr_nal_offset as u32) & 0x1FFF_FFFF;
    // DW3: InlineData DW0 — keep concealment/error-routing at reset values.
    // For an IDR slice that starts at MB0, explicitly seed Current_MB_Number from
    // this slice instead of inheriting stale concealment state from prior engine use.
    // In short-format mode the slice start comes from the parsed slice header.
    batch[avc_bsd + 3] = if has_idr
        && vcl_info
            .map(|info| info.first_mb_in_slice == 0)
            .unwrap_or(true)
    {
        1 << 30
    } else {
        0
    };
    // DW4: InlineData DW1 — LastSlice(3), FixPrevMBSkipped(7).
    // Leave FirstMbByte/BitOffset and EmulationPreventionBytePresent cleared.
    batch[avc_bsd + 4] = (1 << 3) | (1 << 7);
    // DW5: InlineData DW2 — IntraPredictionErrorControl(0),
    // Intra8x8/4x4PredictionErrorConcealmentControl(1),
    // ISliceConcealmentMode(31)=Intra concealment.
    batch[avc_bsd + 5] = 1 | (1 << 1) | (1 << 31);

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
    context1_lo: u32,
    context1_hi: u32,
) {
    // Gen12 ELSP: must write ALL 4 SQ dwords (both context entries).
    // Leaving slot 1 unwritten can cause the scheduler to read stale data.
    super::mmio_write(dev, ring_base + RING_EXECLIST_SQ_LO, context0_lo);
    super::mmio_write(dev, ring_base + RING_EXECLIST_SQ_HI, context0_hi);
    super::mmio_write(dev, ring_base + RING_EXECLIST_SQ_LO + 8, context1_lo);
    super::mmio_write(dev, ring_base + RING_EXECLIST_SQ_HI + 8, context1_hi);
}

fn media_execlists_ready_for_hot_submit(
    dev: crate::intel::Dev,
    engine: MediaEngineDescriptor,
    hwsp_virt: *mut u8,
) -> bool {
    let ring_base = engine.ring_base;
    drain_csb(dev, engine, hwsp_virt);
    let status = super::mmio_read(dev, ring_base + RING_EXECLIST_STATUS_LO);
    // Hot resubmission is legal while a context is active, as long as the
    // scheduler is not already stuck in a pending-load or preempt-to-idle flow.
    if status & (1 << 30 | 0x6) == 0 {
        return true;
    }

    false
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

const GDRST: usize = 0x0000_941C;
/// Gen11+ GDRST domain bits shifted vs Gen6-9.  BIT(2) is BLT on Gen11+;
/// VCS0 (media decode) is GEN11_GRDOM_MEDIA = BIT(5).
const GRDOM_MEDIA_VCS0: u32 = 1 << 5;
const MODE_IDLE: u32 = 1 << 9;

/// Gen12 HWSP CSB write pointer: dword 0x2F = byte offset 0xBC.
/// On Gen11+ hardware updates the write pointer ONLY in HWSP memory, NOT in MMIO.
const GEN12_HWSP_CSB_WRITE_OFFSET: usize = 0xBC;
/// Gen12 has 12 CSB entries; initial reset value = csb_size - 1 = 11.
const GEN12_CSB_RESET_VALUE: u32 = 11;
/// Gen11+ CSB pointer fields are 4 bits wide (0-15).
const GEN11_CSB_READ_PTR_MASK: u32 = 0xF << 8;
const GEN11_CSB_WRITE_PTR_MASK: u32 = 0xF;

/// Acknowledge all pending CSB events so the ELSP scheduler releases the context.
/// Reads the write pointer from HWSP (the only place Gen12 HW updates it) and
/// sets the MMIO read pointer equal so the scheduler knows all events are consumed.
/// IMPORTANT: only touch the SW-managed read pointer (bits [11:8]). The write
/// pointer (bits [3:0]) is HW-managed — overwriting it desyncs the scheduler's
/// internal tracker and prevents future context dispatches.
fn drain_csb(dev: crate::intel::Dev, engine: MediaEngineDescriptor, hwsp_virt: *mut u8) {
    let ring_base = engine.ring_base;
    super::dma_flush(hwsp_virt, GEN12_HWSP_CSB_WRITE_OFFSET + 8);
    let write_ptr = unsafe {
        core::ptr::read_volatile(hwsp_virt.add(GEN12_HWSP_CSB_WRITE_OFFSET) as *const u32)
    } & 0xF;
    // Masked MMIO write: ONLY set read pointer = write pointer.
    // Do NOT mask/write the write pointer bits — they are HW-owned.
    let masked_write = (GEN11_CSB_READ_PTR_MASK << 16) | (write_ptr << 8);
    super::mmio_write(dev, ring_base + 0x3A0, masked_write);
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    for _ in 0..2_000u32 {
        core::hint::spin_loop();
    }
}

/// Seed HWSP and MMIO CSB pointers to the Gen12 reset state (both = 11).
/// Follows the i915 reset_csb_pointers() pattern: full-mask MMIO write,
/// posting read, clear CSB entry buffer, flush, repeat.
const GEN12_HWSP_CSB_BUF0_OFFSET: usize = 0x40; // dword 0x10 = byte 0x40
const GEN12_CSB_ENTRIES: usize = 12;

fn init_csb_pointers(dev: crate::intel::Dev, ring_base: usize, hwsp_virt: *mut u8) {
    let csb_init: u32 = 0xFFFF_0000 | (GEN12_CSB_RESET_VALUE << 8) | GEN12_CSB_RESET_VALUE;

    // First MMIO write + posting read (i915 pattern).
    super::mmio_write(dev, ring_base + 0x3A0, csb_init);
    let _ = super::mmio_read(dev, ring_base + 0x3A0); // posting read

    // Set SW-side HWSP write pointer to reset value.
    unsafe {
        core::ptr::write_volatile(
            hwsp_virt.add(GEN12_HWSP_CSB_WRITE_OFFSET) as *mut u32,
            GEN12_CSB_RESET_VALUE,
        );
    }
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

    // Clear CSB entry buffer with -1 so HW writes are distinguishable.
    unsafe {
        let csb_buf = hwsp_virt.add(GEN12_HWSP_CSB_BUF0_OFFSET) as *mut u64;
        for i in 0..GEN12_CSB_ENTRIES {
            core::ptr::write_volatile(csb_buf.add(i), !0u64);
        }
    }
    super::dma_flush(hwsp_virt, GEN12_HWSP_CSB_WRITE_OFFSET + 8);

    // Second MMIO write + posting read (i915 does this twice for paranoia).
    super::mmio_write(dev, ring_base + 0x3A0, csb_init);
    let _ = super::mmio_read(dev, ring_base + 0x3A0); // posting read
}

/// Wait for prior execution to retire and acknowledge pending CSB events.
/// Fallback to GDRST only if the engine refuses to go idle.
fn reset_media_engine(
    dev: crate::intel::Dev,
    engine: MediaEngineDescriptor,
    _context_virt: *mut u8,
) {
    let ring_base = engine.ring_base;

    // Always do a hard engine reset (GDRST) so the ELSP scheduler's
    // entire state machine — including the internal CSB write counter —
    // is fully cleared.  A soft drain_csb alone leaves the scheduler in
    // a state where it queues but never promotes subsequent contexts.
    //
    // Sequence: wait for engine idle → STOP_RING → MODE_IDLE → GDRST → clear STOP.
    for _ in 0..200_000u32 {
        let el = super::mmio_read(dev, ring_base + RING_EXECLIST_STATUS_LO);
        if (el >> 30) == 0 {
            break;
        }
        core::hint::spin_loop();
    }
    super::mmio_write(dev, ring_base + RING_MI_MODE, STOP_RING | (STOP_RING << 16));
    for _ in 0..50_000u32 {
        if super::mmio_read(dev, ring_base + RING_MI_MODE) & MODE_IDLE != 0 {
            break;
        }
        core::hint::spin_loop();
    }
    super::mmio_write(dev, GDRST, GRDOM_MEDIA_VCS0);
    for _ in 0..500_000u32 {
        if super::mmio_read(dev, GDRST) & GRDOM_MEDIA_VCS0 == 0 {
            break;
        }
        core::hint::spin_loop();
    }
    super::mmio_write(dev, ring_base + RING_MI_MODE, STOP_RING << 16);
    // After domain reset, invalidate GGTT TLB so the engine's page-walker
    // cache doesn't serve stale translations for LRC/ring/batch addresses.
    super::ggtt_invalidate(dev);
}

fn seed_media_ring_live_state(
    dev: crate::intel::Dev,
    ring_base: usize,
    pphwsp_gpu: u32,
    _ring_start: u32,
    _ring_ctl: u32,
    _ring_tail: u32,
) {
    // Gen12 Execlists: ring registers are loaded from LRC, NOT from MMIO.
    // Only set engine-level config here.
    super::mmio_write(dev, ring_base + RING_MI_MODE, STOP_RING << 16);
    super::mmio_write(dev, ring_base + RING_MI_MODE, masked_bit_disable(STOP_RING));
    super::mmio_write(dev, ring_base + RING_HWS_PGA, pphwsp_gpu);
    super::mmio_write(dev, ring_base + RING_HWSTAM, !0u32);
}

fn submit_h264_frame(
    dev: crate::intel::Dev,
    engine: MediaEngineDescriptor,
    windows: MediaGpuWindowLayout,
    backing: MediaBitstreamBacking,
    frame_width: u16,
    frame_height: u16,
    annex_b: &AnnexBAccessUnit,
    vcl_info: Option<H264VclInfo>,
    sps: &ParsedSps,
    pps: &ParsedPps,
    sample_idx: u32,
    ref_surface_idx: u32,
) -> Option<(bool, usize, usize, u64, u64, *mut u8)> {
    submit_h264_frame_once(
        dev,
        engine,
        windows,
        backing,
        frame_width,
        frame_height,
        annex_b,
        vcl_info,
        sps,
        pps,
        sample_idx,
        ref_surface_idx,
        true,
    )
}

fn submit_h264_frame_once(
    dev: crate::intel::Dev,
    engine: MediaEngineDescriptor,
    windows: MediaGpuWindowLayout,
    backing: MediaBitstreamBacking,
    frame_width: u16,
    frame_height: u16,
    annex_b: &AnnexBAccessUnit,
    vcl_info: Option<H264VclInfo>,
    sps: &ParsedSps,
    pps: &ParsedPps,
    sample_idx: u32,
    ref_surface_idx: u32,
    force_reset: bool,
) -> Option<(bool, usize, usize, u64, u64, *mut u8)> {
    let (coded_width, coded_height) = coded_h264_frame_dims(sps);
    let output_pitch = align_up_u32(coded_width.max(128), 128) as usize; // Y-tile: 128-byte tile width
    // Y-tile NV12: chroma starts at tile-row-aligned height (same as batch builder).
    let chroma_y_aligned = ((coded_height as usize) + 31) & !31;
    let total_height = chroma_y_aligned + ((coded_height as usize) + 1) / 2;
    let total_tile_rows = (total_height + 31) & !31;
    let output_bytes = output_pitch.checked_mul(total_tile_rows)?;
    let output_budget = backing
        .output_surface_bytes
        .checked_sub(MEDIA_SCRATCH_OFFSET_BYTES)?;
    // Double-buffer: need 2× output_bytes for decode target + reference surface.
    let double_output = output_bytes.checked_mul(2)?;
    if double_output > output_budget || annex_b.bytes_written > backing.bitstream_bytes {
        return None;
    }

    let scratch_gpu_addr = windows.output_surface_gpu_addr;
    // Double-buffer: surface A at scratch_end, surface B at scratch_end + output_bytes.
    // Reference frames toggle A/B; non-reference frames decode into the non-ref surface.
    let surface_a_offset = MEDIA_SCRATCH_OFFSET_BYTES;
    let surface_b_offset = MEDIA_SCRATCH_OFFSET_BYTES + output_bytes;
    let (cur_offset, ref_offset) = if ref_surface_idx % 2 == 0 {
        (surface_a_offset, surface_b_offset)
    } else {
        (surface_b_offset, surface_a_offset)
    };
    let output_surface_gpu_addr = windows.output_surface_gpu_addr + cur_offset as u64;
    let ref_surface_gpu_addr = windows.output_surface_gpu_addr + ref_offset as u64;
    let output_surface_phys = backing.output_surface_phys + cur_offset as u64;
    let output_surface_virt = unsafe { backing.output_surface_virt.add(cur_offset) };

    let ring_virt = backing.ring_virt;
    let context_virt = backing.context_virt;
    let ring_gpu_addr = windows.ring_gpu_addr;
    let context_gpu_addr = windows.context_gpu_addr;

    let cold_start = sample_idx == 0;

    // Prefer keeping the engine live across successful frames.
    // Fall back to a full reset for cold start and whenever a hot submit fails.
    wake_media_engine_forcewake(dev, engine);
    if force_reset {
        reset_media_engine(dev, engine, context_virt);
    } else {
        if !media_execlists_ready_for_hot_submit(dev, engine, context_virt) {
            return None;
        }
    }

    unsafe {
        if force_reset {
            core::ptr::write_bytes(ring_virt, 0, backing.ring_bytes);
        }
        if force_reset {
            core::ptr::write_bytes(context_virt, 0, backing.context_bytes);
        }
        if cold_start {
            core::ptr::write_bytes(backing.output_surface_virt, 0, backing.output_surface_bytes);
        }
        // Fill UV chroma tile region with neutral 128 for the current output surface
        // every frame so any UV tiles not fully written by the decoder remain
        // neutral gray instead of showing stale pink/green from prior frames.
        {
            let tiles_per_row = output_pitch / 128;
            let uv_tile_start = (chroma_y_aligned / 32) * tiles_per_row * 4096;
            let uv_fill_len = output_bytes.saturating_sub(uv_tile_start);
            let surf_ptr = backing.output_surface_virt.add(cur_offset);
            if uv_tile_start < output_bytes {
                core::ptr::write_bytes(surf_ptr.add(uv_tile_start), 0x80, uv_fill_len);
            }
            if cold_start {
                // Also fill the reference surface on first frame.
                let ref_ptr = backing.output_surface_virt.add(ref_offset);
                core::ptr::write_bytes(ref_ptr.add(uv_tile_start), 0x80, uv_fill_len);
            }
        }
        core::ptr::write_bytes(backing.batch_virt, 0, backing.batch_bytes);
        core::ptr::write_bytes(backing.result_virt, 0, backing.result_bytes);
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
        ref_surface_gpu_addr,
        scratch_gpu_addr,
        output_bytes,
        frame_width,
        frame_height,
        annex_b.bytes_written,
        annex_b.sample_nal_count,
        annex_b.has_idr,
        annex_b.idr_nal_offset,
        annex_b.idr_nal_length,
        vcl_info,
        sps,
        pps,
        kickoff_marker,
        presubmit_marker,
        postsubmit_marker,
        complete_marker,
    )?;

    // Decisive diagnostics: addresses, bitstream NAL header, batch size
    // Always start ring at offset 0.
    const MEDIA_RING_REQUEST_BYTES: usize = 40;
    let ring_head_bytes = (sample_idx as usize).saturating_mul(MEDIA_RING_REQUEST_BYTES);
    if ring_head_bytes + MEDIA_RING_REQUEST_BYTES > backing.ring_bytes {
        return None;
    }
    let ring_tail_bytes = build_ring_batch_start_words(
        ring_virt,
        backing.ring_bytes,
        ring_head_bytes,
        windows.result_gpu_addr,
        ring_prelaunch_marker,
        windows.batch_gpu_addr,
    )?;
    let ring_ctl = ring_ctl_value_for_size(backing.ring_bytes)?;
    let ring_start = ring_gpu_addr as u32;
    let pphwsp_gpu = (context_gpu_addr & !0xFFF) as u32;

    // Streaming submits need restore enabled so the engine reloads the LRC
    // head/tail we rewrite for each frame instead of staying on the prior
    // restored state.
    let ctx_ctl_after = media_ctx_control_value(false);
    let force_context_restore = true;
    if force_reset {
        if !init_gen12_video_context_image(
            context_virt,
            backing.context_bytes,
            engine.ring_base,
            ring_head_bytes as u32,
            ring_start,
            ring_tail_bytes as u32,
            ring_ctl,
            pphwsp_gpu,
            backing.ppgtt_pml4_phys,
            false,
        ) {
            return None;
        }
    } else {
        prepare_video_lrc_for_submit(
            context_virt,
            backing.context_bytes,
            ring_head_bytes as u32,
            ring_tail_bytes as u32,
            ctx_ctl_after,
        );
    }
    {
        let mode_bits = GFX_RUN_LIST_ENABLE | GEN11_GFX_DISABLE_LEGACY_MODE;
        super::mmio_write(dev, engine.ring_base + RING_MODE_GEN7, mode_bits | (mode_bits << 16));
    }
    seed_media_ring_live_state(
        dev,
        engine.ring_base,
        pphwsp_gpu,
        ring_start,
        ring_ctl,
        ring_tail_bytes as u32,
    );
    if force_reset {
        init_csb_pointers(dev, engine.ring_base, context_virt);
    }

    super::dma_flush(backing.bitstream_virt, annex_b.bytes_written);
    super::dma_flush(backing.batch_virt, batch_tail_bytes);
    super::dma_flush(ring_virt, ring_tail_bytes);
    super::dma_flush(context_virt, backing.context_bytes);
    super::dma_flush(backing.result_virt, backing.result_bytes);
    // Only flush the full output surface on cold start (when we zero-filled
    // and wrote UV neutral). On subsequent frames the GPU writes directly.
    if cold_start {
        super::dma_flush(backing.output_surface_virt, backing.output_surface_bytes);
    }

    {
        super::mmio_write(dev, engine.ring_base + RING_CONTEXT_CONTROL, ctx_ctl_after);
        super::mmio_write(dev, engine.ring_base + RING_CONTEXT_CONTROL_REF, ctx_ctl_after);
        super::mmio_write(dev, engine.ring_base + RING_MI_MODE, masked_bit_disable(STOP_RING));
        super::mmio_write(dev, engine.ring_base + RING_HWS_PGA, pphwsp_gpu);
    }
    // FORCE_RESTORE: forces the scheduler to reload LRC state from memory
    // every submission, whether cold (GDRST cleared tracker) or hot (new ring pointers).
    let submit_counter = sample_idx.wrapping_add(1) & 0x3F;
    let (ctx_desc_lo, ctx_desc_hi) = build_media_execlist_context_descriptor(
        context_gpu_addr,
        engine,
        submit_counter,
        force_context_restore,
    );
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

    Some((
        completed,
        output_pitch,
        output_bytes,
        output_surface_gpu_addr,
        output_surface_phys,
        output_surface_virt,
    ))
}

pub(super) fn ensure_decode_backing(
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

pub(super) fn stream_encoded_to_bitstream(
    dev: crate::intel::Dev,
    engine: MediaEngineDescriptor,
    windows: MediaGpuWindowLayout,
    backing: MediaBitstreamBacking,
    encoded: &[u8],
) -> Option<MediaEncodedStreamProof> {
    if encoded.is_empty() || encoded.len() > backing.bitstream_bytes {
        return None;
    }

    wake_media_engine_forcewake(dev, engine);
    let wake = snapshot_forcewake(dev);

    unsafe {
        core::ptr::copy_nonoverlapping(encoded.as_ptr(), backing.bitstream_virt, encoded.len());
        let clear_start = encoded.len();
        let clear_len = backing.bitstream_bytes.saturating_sub(clear_start).min(256);
        if clear_len != 0 {
            core::ptr::write_bytes(backing.bitstream_virt.add(clear_start), 0, clear_len);
        }
    }
    super::dma_flush(backing.bitstream_virt, encoded.len());

    Some(MediaEncodedStreamProof {
        engine_name: engine.name,
        bitstream_gpu_addr: windows.bitstream_gpu_addr,
        bitstream_phys: backing.bitstream_phys,
        bitstream_virt: backing.bitstream_virt as usize,
        bytes_written: encoded.len(),
        capacity: backing.bitstream_bytes,
        signature: byte_signature(encoded),
        forcewake_global_ack: wake.global_ack,
        forcewake_awake_count: wake.awake_count,
    })
}

pub(crate) fn kickoff_once() {
    MEDIA_KICKOFF_RAN.store(true, Ordering::Release);
    let prior_demo = MEDIA_KICKOFF_STATE
        .lock()
        .as_ref()
        .and_then(|state| state.last_decode_frame);
    store_kickoff_state(MediaKickoffStage::CommandEncoding, prior_demo);
}

pub(crate) fn kickoff_state() -> Option<MediaKickoffState> {
    *MEDIA_KICKOFF_STATE.lock()
}

pub(crate) fn decode_surface_window(name: &str) -> Option<MediaSurfaceWindow> {
    let state = *MEDIA_KICKOFF_STATE.lock();
    let demo = state?.last_decode_frame?;
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

pub(crate) async fn run_media_decode_async() {
    let _ = run_media2_first_frame_async().await;
}

pub(crate) async fn run_media2_first_frame_async() -> Option<Media2FirstFrameState> {
    kickoff_once();

    if MEDIA_DECODE_RAN.load(Ordering::Acquire) {
        crate::log!("intel/media2: first-frame cached state=already-ran\n");
        return MEDIA_KICKOFF_STATE
            .lock()
            .as_ref()
            .and_then(|state| state.last_decode_frame)
            .map(media2_first_frame_state);
    }

    let dev = super::claimed_device()?;
    let topology = current_topology();
    let engine = topology
        .default_decode
        .and_then(|id| {
            topology
                .engines
                .iter()
                .take(topology.planned_engine_count)
                .find(|engine| engine.id == id)
                .copied()
        })
        .unwrap_or(topology.engines[0]);
    let engine_slot = topology
        .engines
        .iter()
        .take(topology.planned_engine_count)
        .position(|desc| desc.id == engine.id)
        .unwrap_or(0);
    let windows = engine_window(engine_slot);

    let source = match xelp_media_source::fetch_media_source_async().await {
        Some(source) => source,
        None => {
            crate::log!("intel/media2: first-frame abort stage=source reason=unavailable\n");
            store_kickoff_state(MediaKickoffStage::SubmissionWiring, None);
            return None;
        }
    };
    crate::log!(
        "intel/media2: first-frame source ready source={} bytes={}\n",
        source.source_name(),
        source.total_len(),
    );
    let summary = match parse_h264_source_summary(&source).await {
        Ok(summary) => summary,
        Err(err) => {
            crate::log!(
                "intel/media2: first-frame abort stage=parse reason={} source={} bytes={}\n",
                err,
                source.source_name(),
                source.total_len(),
            );
            store_kickoff_state(MediaKickoffStage::ResourcePlanning, None);
            return None;
        }
    };
    crate::log!(
        "intel/media2: first-frame summary container={} coded={}x{} samples={} first_sample_bytes={} avcc_profile={} avcc_level={} nal_len={} sps_bytes={} pps_bytes={}\n",
        summary.container_name(),
        summary.width(),
        summary.height(),
        summary.sample_count(),
        summary.first_sample().len(),
        summary.avcc().profile_idc,
        summary.avcc().level_idc,
        summary.avcc().nal_length_size,
        summary.avcc().sps.len(),
        summary.avcc().pps.len(),
    );

    let Some(sps) = parse_sps(&summary.avcc().sps) else {
        crate::log!("intel/media2: first-frame abort stage=sps reason=parse-failed\n");
        store_kickoff_state(MediaKickoffStage::ResourcePlanning, None);
        return None;
    };
    let Some(pps) = parse_pps(&summary.avcc().pps, &sps) else {
        crate::log!("intel/media2: first-frame abort stage=pps reason=parse-failed\n");
        store_kickoff_state(MediaKickoffStage::ResourcePlanning, None);
        return None;
    };
    let (coded_width, coded_height) = coded_h264_frame_dims(&sps);
    let (visible_x, visible_y) = h264_crop_offsets_px(&sps);
    let (sps_visible_width, sps_visible_height) = visible_h264_frame_dims(&sps);
    let summary_width = u32::from(summary.width());
    let summary_height = u32::from(summary.height());
    let display_width =
        if summary_width != 0 && visible_x.saturating_add(summary_width) <= coded_width {
            summary.width()
        } else {
            u16::try_from(sps_visible_width)
                .ok()
                .filter(|width| *width != 0)
                .unwrap_or_else(|| summary.width())
        };
    let display_height =
        if summary_height != 0 && visible_y.saturating_add(summary_height) <= coded_height {
            summary.height()
        } else {
            u16::try_from(sps_visible_height)
                .ok()
                .filter(|height| *height != 0)
                .unwrap_or_else(|| summary.height())
        };

    let Some(backing) = ensure_decode_backing(dev, windows) else {
        crate::log!("intel/media2: first-frame abort stage=backing reason=alloc-or-map-failed\n");
        store_kickoff_state(MediaKickoffStage::ResourcePlanning, None);
        return None;
    };

    let first_annex_b = {
        let bitstream = unsafe {
            core::slice::from_raw_parts_mut(backing.bitstream_virt, backing.bitstream_bytes)
        };
        match write_annex_b_for_sample(summary.first_sample(), summary.avcc(), bitstream) {
            Ok(annex_b) => annex_b,
            Err(err) => {
                crate::log!(
                    "intel/media2: first-frame abort stage=annex-b err={:?} sample_bytes={} bitstream_capacity={}\n",
                    err,
                    summary.first_sample().len(),
                    backing.bitstream_bytes,
                );
                store_kickoff_state(MediaKickoffStage::ResourcePlanning, None);
                return None;
            }
        }
    };
    let vcl_info =
        parse_sample_vcl_info(summary.first_sample(), summary.avcc().nal_length_size, &sps, &pps);
    let demo = match decode_and_present_frame(
        dev,
        engine,
        windows,
        backing,
        display_width,
        display_height,
        &first_annex_b,
        vcl_info,
        &sps,
        &pps,
        0,
        0,
    ) {
        Some(demo) => demo,
        None => {
            crate::log!(
                "intel/media2: first-frame abort stage=submit engine={} bitstream_bytes={}\n",
                engine.name,
                first_annex_b.bytes_written,
            );
            return None;
        }
    };
    MEDIA_DECODE_RAN.store(true, Ordering::Release);
    store_kickoff_state(MediaKickoffStage::Smoke, Some(demo));
    Some(media2_first_frame_state(demo))
}
