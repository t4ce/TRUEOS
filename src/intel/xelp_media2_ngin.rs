use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

use super::xelp_media2_ngin_hw_pic::MediaEncodedStreamProof;

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

pub(super) const RING_TAIL: usize = 0x30;
pub(super) const RING_HEAD: usize = 0x34;
pub(super) const RING_START: usize = 0x38;
pub(super) const RING_CTL: usize = 0x3C;
pub(super) const RING_PSMI_CTL: usize = 0x50;
pub(super) const RING_ACTHD_UDW: usize = 0x5C;
pub(super) const RING_DMA_FADD_UDW: usize = 0x60;
pub(super) const RING_IPEIR: usize = 0x64;
pub(super) const RING_IPEHR: usize = 0x68;
const RING_INSTDONE: usize = 0x6C;
pub(super) const RING_INSTPS: usize = 0x70;
pub(super) const RING_ACTHD: usize = 0x74;
pub(super) const RING_DMA_FADD: usize = 0x78;
pub(super) const RING_HWS_PGA: usize = 0x80;
pub(super) const RING_NOPID: usize = 0x94;
const RING_HWSTAM: usize = 0x98;
pub(super) const RING_MI_MODE: usize = 0x9C;
pub(super) const RING_BBSTATE: usize = 0x110;
pub(super) const RING_BBADDR: usize = 0x140;
pub(super) const RING_BBADDR_UDW: usize = 0x168;
pub(super) const RING_CONTEXT_CONTROL: usize = 0x244;
pub(super) const RING_MODE_GEN7: usize = 0x29C;
pub(super) const RING_CONTEXT_CONTROL_REF: usize = 0x5A0;
pub(super) const RING_ESR: usize = 0xB8;
pub(super) const RING_EXECLIST_SUBMIT_PORT: usize = 0x230;
pub(super) const RING_EXECLIST_STATUS_LO: usize = 0x234;
pub(super) const RING_EXECLIST_STATUS_HI: usize = 0x238;
pub(super) const RING_EXECLIST_SQ_LO: usize = 0x510;
pub(super) const RING_EXECLIST_SQ_HI: usize = 0x514;
pub(super) const RING_EXECLIST_CONTROL: usize = 0x550;
pub(super) const GEN12_RING_FAULT_REG: usize = 0x0000_CEC4;

const MEDIA_ENGINE_GPU_ADDR_BASE: u64 = 0x0120_0000;
const MEDIA_ENGINE_GPU_ADDR_STRIDE: u64 = 0x0100_0000;
const MEDIA_DEFAULT_RING_BYTES: usize = 16 * 1024;
const MEDIA_DEFAULT_CONTEXT_BYTES: usize = 22 * 4096;
const MEDIA_DEFAULT_BATCH_BYTES: usize = 32 * 1024;
const MEDIA_DEFAULT_RESULT_BYTES: usize = 4 * 4096;
const MEDIA_DEFAULT_BITSTREAM_BYTES: usize = 8 * 1024 * 1024;
const MEDIA_DEFAULT_OUTPUT_SURFACE_BYTES: usize = 16 * 1024 * 1024;
pub(super) const MEDIA_SUBMIT_POLL_ITERS: usize = 100_000;

const MI_STORE_DWORD_IMM_GEN4: u32 = (0x20 << 23) | 2;
const MI_STORE_DWORD_IMM_GEN4_LEN_DW4_PPGTT: u32 = MI_STORE_DWORD_IMM_GEN4 | (4 - 2);
pub(super) const MI_FLUSH_DW: u32 = (0x26 << 23) | 3;
pub(super) const MI_FLUSH_DW_VIDEO_PIPELINE_CACHE_INVALIDATE: u32 = 1 << 7;
pub(super) const MI_FLUSH_DW_POST_SYNC_WRITE_IMMEDIATE: u32 = 1 << 14;
pub(super) const MI_ARB_CHECK: u32 = 0x0280_0000;
pub(super) const MI_BATCH_BUFFER_END: u32 = 0x0500_0000;
const MI_BATCH_BUFFER_START_GEN8: u32 = (0x31 << 23) | 1;
const MI_BATCH_PPGTT: u32 = 1 << 8;
pub(super) const MI_NOOP: u32 = 0;
pub(super) const MI_FORCE_WAKEUP: u32 = 29 << 23;
pub(super) const MI_FORCE_WAKEUP_MFX_WELL: u32 = (1 << 9) | (0x300 << 16);
const MI_LOAD_REGISTER_IMM: u32 = 0x1100_0000;
const MI_LRI_CS_MMIO: u32 = 1 << 19;
const MI_LRI_FORCE_POSTED: u32 = 1 << 12;

pub(super) const EL_CTRL_LOAD: u32 = 1 << 0;
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
pub(super) const GEN11_GFX_DISABLE_LEGACY_MODE: u32 = 1 << 3;
pub(super) const GFX_RUN_LIST_ENABLE: u32 = 1 << 15;
pub(super) const STOP_RING: u32 = 1 << 8;

const MEDIA_PIPELINE_MFX: u32 = 2;
pub(super) const MEDIA_CMD_OPCODE_MFX_COMMON: u32 = 0;
pub(super) const MFX_PIPE_MODE_SELECT: u32 = 0;
pub(super) const MFX_SURFACE_STATE: u32 = 1;
pub(super) const MFX_PIPE_BUF_ADDR_STATE: u32 = 2;
pub(super) const MFX_IND_OBJ_BASE_ADDR_STATE: u32 = 3;
pub(super) const MFX_QM_STATE: u32 = 7;
pub(super) const MFX_CMD_LEN_PIPE_MODE_SELECT: u32 = 3;
pub(super) const MFX_CMD_LEN_SURFACE_STATE: u32 = 4;
pub(super) const MFX_CMD_LEN_PIPE_BUF_ADDR_STATE: u32 = 63;
pub(super) const MFX_CMD_LEN_IND_OBJ_BASE_ADDR_STATE: u32 = 24;
pub(super) const MFX_CMD_LEN_QM_STATE: u32 = 16;
pub(super) const MFX_MOCS_UC: u32 = 1;
const MFX_WAIT_SYNC: u32 = (3 << 29) | (1 << 27) | (1 << 8);

const MEDIA_RESULT_SLOT_BYTES: u64 = 8;
pub(super) const MEDIA_RESULT_KICKOFF_SLOT: u64 = 0;
pub(super) const MEDIA_RESULT_PRESUBMIT_SLOT: u64 =
    MEDIA_RESULT_KICKOFF_SLOT + MEDIA_RESULT_SLOT_BYTES;
pub(super) const MEDIA_RESULT_POSTSUBMIT_SLOT: u64 =
    MEDIA_RESULT_PRESUBMIT_SLOT + MEDIA_RESULT_SLOT_BYTES;
pub(super) const MEDIA_RESULT_COMPLETE_SLOT: u64 =
    MEDIA_RESULT_POSTSUBMIT_SLOT + MEDIA_RESULT_SLOT_BYTES;
pub(super) const MEDIA_RESULT_BITSTREAM_ADDR_LO_SLOT: u64 =
    MEDIA_RESULT_COMPLETE_SLOT + MEDIA_RESULT_SLOT_BYTES;
pub(super) const MEDIA_RESULT_BITSTREAM_ADDR_HI_SLOT: u64 =
    MEDIA_RESULT_BITSTREAM_ADDR_LO_SLOT + MEDIA_RESULT_SLOT_BYTES;
pub(super) const MEDIA_RESULT_BITSTREAM_BYTES_SLOT: u64 =
    MEDIA_RESULT_BITSTREAM_ADDR_HI_SLOT + MEDIA_RESULT_SLOT_BYTES;
pub(super) const MEDIA_RESULT_SAMPLE_NALS_SLOT: u64 =
    MEDIA_RESULT_BITSTREAM_BYTES_SLOT + MEDIA_RESULT_SLOT_BYTES;
pub(super) const MEDIA_RESULT_STAGE_FLAGS_SLOT: u64 =
    MEDIA_RESULT_SAMPLE_NALS_SLOT + MEDIA_RESULT_SLOT_BYTES;
pub(super) const MEDIA_RESULT_OUTPUT_SURFACE_ADDR_LO_SLOT: u64 =
    MEDIA_RESULT_STAGE_FLAGS_SLOT + MEDIA_RESULT_SLOT_BYTES;
pub(super) const MEDIA_RESULT_OUTPUT_SURFACE_ADDR_HI_SLOT: u64 =
    MEDIA_RESULT_OUTPUT_SURFACE_ADDR_LO_SLOT + MEDIA_RESULT_SLOT_BYTES;
pub(super) const MEDIA_RESULT_OUTPUT_SURFACE_BYTES_SLOT: u64 =
    MEDIA_RESULT_OUTPUT_SURFACE_ADDR_HI_SLOT + MEDIA_RESULT_SLOT_BYTES;
pub(super) const MEDIA_RESULT_FRAME_DIMS_SLOT: u64 =
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
pub(super) struct MediaEngineForcewakeAck {
    ack_reg: usize,
    ack_value: u32,
    awake: bool,
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
    MediaTopology {
        sku_name: "xelp-jpeg-only",
        active_engine_count: 1,
        planned_engine_count: 1,
        engines: [
            decode0,
            MediaEngineDescriptor::unused(),
            MediaEngineDescriptor::unused(),
            MediaEngineDescriptor::unused(),
        ],
        default_decode: Some(decode0.id),
        default_enhance: None,
    }
}

fn current_api_shape(transport: MediaSubmissionTransport) -> MediaApiShape {
    let mut api = MediaApiShape::empty();
    api.route_count = 2;
    api.routes[0] = MediaApiRoute {
        name: "media.jpeg.submit",
        workload: MediaWorkloadKind::DecodeBitstream,
        preferred_engine_class: Some(MediaEngineClass::VideoDecode),
        transport,
        summary: "submit one boot-logo JPEG through the VCS media path",
    };
    api.routes[1] = MediaApiRoute {
        name: "media.observe.snapshot",
        workload: MediaWorkloadKind::SessionSnapshot,
        preferred_engine_class: None,
        transport,
        summary: "snapshot forcewake and live VCS registers",
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
    (current_topology().engines[0], engine_window(0))
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
        awake_count += usize::from(awake);
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

fn rebuild_kickoff_state(stage: MediaKickoffStage) -> Option<MediaKickoffState> {
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
        last_decode_frame: None,
    })
}

fn store_kickoff_state(stage: MediaKickoffStage) {
    *MEDIA_KICKOFF_STATE.lock() = rebuild_kickoff_state(stage);
}

pub(crate) fn kickoff_once() {
    MEDIA_KICKOFF_RAN.store(true, Ordering::Release);
    store_kickoff_state(MediaKickoffStage::CommandEncoding);
}

pub(crate) fn kickoff_state() -> Option<MediaKickoffState> {
    *MEDIA_KICKOFF_STATE.lock()
}

pub(crate) fn decode_surface_window(_name: &str) -> Option<MediaSurfaceWindow> {
    None
}

pub(crate) async fn run_media_decode_async() {
    let _ = run_media2_first_frame_async().await;
}

pub(crate) async fn run_media2_first_frame_async() -> Option<Media2FirstFrameState> {
    kickoff_once();
    crate::log!("intel/media2: disabled reason=jpeg-only-engine-cut\n");
    store_kickoff_state(MediaKickoffStage::SubmissionWiring);
    MEDIA_DECODE_RAN.store(true, Ordering::Release);
    None
}

pub(super) fn sample_buffer_dword(
    base_virt: *mut u8,
    buffer_bytes: usize,
    offset_bytes: usize,
) -> u32 {
    if offset_bytes.saturating_add(core::mem::size_of::<u32>()) > buffer_bytes {
        return 0;
    }
    unsafe { core::ptr::read_volatile(base_virt.add(offset_bytes) as *const u32) }
}

pub(super) fn classify_media_acthd(
    acthd: u32,
    windows: MediaGpuWindowLayout,
    backing: MediaBitstreamBacking,
    batch_tail_bytes: usize,
    ring_tail_bytes: usize,
) -> (&'static str, u32, u32) {
    let acthd_aligned = acthd & !0x3;
    let regions = [
        ("ring", windows.ring_gpu_addr, ring_tail_bytes, backing.ring_virt),
        ("batch", windows.batch_gpu_addr, batch_tail_bytes, backing.batch_virt),
        ("bitstream", windows.bitstream_gpu_addr, backing.bitstream_bytes, backing.bitstream_virt),
        (
            "output",
            windows.output_surface_gpu_addr,
            backing.output_surface_bytes,
            backing.output_surface_virt,
        ),
    ];

    for (name, gpu_addr, buffer_bytes, base_virt) in regions {
        let base = gpu_addr as u32;
        if acthd_aligned < base {
            continue;
        }
        let offset = acthd_aligned.wrapping_sub(base) as usize;
        if offset < buffer_bytes {
            return (name, offset as u32, sample_buffer_dword(base_virt, buffer_bytes, offset));
        }
    }
    ("unknown", 0, 0)
}

pub(super) fn marker_base(engine: MediaEngineDescriptor) -> u32 {
    let class_base = match engine.id.class {
        MediaEngineClass::VideoDecode => 0x4D44_1000,
        MediaEngineClass::VideoEnhancement => 0x4D45_1000,
    };
    class_base + (engine.id.instance as u32) * 0x100
}

pub(super) fn surface_signature(bytes: &[u8]) -> (u32, usize) {
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
            let value = *surface.get(media_ytile_offset(col, row, tiles_per_row))?;
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

pub(super) fn probe_output_surface(
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

pub(super) fn log_output_surface_probe(
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

pub(super) fn output_surface_has_decoded_detail(probe: &MediaSurfaceProbe) -> bool {
    probe.valid
        && (probe.luma_visible_last_row.has_range()
            || probe.luma_prev_mb_row.has_range()
            || probe.luma_bottom_mb_row.has_range()
            || probe.chroma_prev_mb_row.has_range()
            || probe.chroma_bottom_mb_row.has_range())
}

#[inline]
pub(super) fn align_up_u32(value: u32, align: u32) -> u32 {
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
pub(super) fn masked_bit_disable(bit: u32) -> u32 {
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

fn build_ppgtt_for_ranges(ranges: &[(u64, u64, usize)]) -> Option<u64> {
    const PAGE: usize = 4096;
    const ENTRIES: usize = 512;
    const PTE_PRESENT_RW: u64 = 0x3;
    const PDE_PRESENT_RW_UC: u64 = 0x3 | (1 << 3) | (1 << 4);
    let mut pd_min = usize::MAX;
    let mut pd_max = 0usize;
    for &(gpu, _phys, size) in ranges {
        if size == 0 {
            continue;
        }
        let first_pd = (gpu as usize) >> 21;
        let last_pd = (gpu as usize + size - 1) >> 21;
        pd_min = pd_min.min(first_pd);
        pd_max = pd_max.max(last_pd);
    }
    if pd_min > pd_max {
        return None;
    }
    let pt_count = pd_max - pd_min + 1;
    let alloc_bytes = (3 + pt_count) * PAGE;
    let (base_phys, base_virt) = crate::dma::alloc(alloc_bytes, PAGE)?;
    let tables = unsafe { core::slice::from_raw_parts_mut(base_virt as *mut u64, alloc_bytes / 8) };
    tables.fill(0);
    let pml4_off = 0;
    let pdp_off = ENTRIES;
    let pd_off = 2 * ENTRIES;
    let pt_base_off = 3 * ENTRIES;
    tables[pml4_off] = (base_phys + PAGE as u64) | PDE_PRESENT_RW_UC;
    tables[pdp_off] = (base_phys + 2 * PAGE as u64) | PDE_PRESENT_RW_UC;
    for i in 0..pt_count {
        tables[pd_off + pd_min + i] =
            (base_phys + (3 + i) as u64 * PAGE as u64) | PDE_PRESENT_RW_UC;
    }
    for &(gpu, phys, size) in ranges {
        let mut offset = 0usize;
        while offset < size {
            let va = gpu as usize + offset;
            let pd_idx = va >> 21;
            let pt_idx = (va >> 12) & 0x1FF;
            let slot = pt_base_off + (pd_idx - pd_min) * ENTRIES + pt_idx;
            tables[slot] = ((phys + offset as u64) & !0xFFF) | PTE_PRESENT_RW;
            offset += PAGE;
        }
    }
    crate::intel::dma_flush(base_virt, alloc_bytes);
    Some(base_phys)
}

pub(super) fn build_ring_batch_start_words(
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
    let dwords = unsafe { core::slice::from_raw_parts_mut(base as *mut u32, 10) };
    dwords[0] = MI_STORE_DWORD_IMM_GEN4_LEN_DW4_PPGTT;
    dwords[1] = (result_gpu_addr + MEDIA_RESULT_KICKOFF_SLOT) as u32;
    dwords[2] = ((result_gpu_addr + MEDIA_RESULT_KICKOFF_SLOT) >> 32) as u32;
    dwords[3] = prelaunch_marker;
    dwords[4] = MI_BATCH_BUFFER_START_GEN8 | MI_BATCH_PPGTT;
    dwords[5] = batch_gpu_addr as u32;
    dwords[6] = (batch_gpu_addr >> 32) as u32;
    dwords[7] = MI_ARB_CHECK;
    dwords[8] = MI_NOOP;
    dwords[9] = MI_NOOP;
    Some(ring_offset + 40)
}

pub(super) fn ring_ctl_value_for_size(size: usize) -> Option<u32> {
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

pub(super) fn build_media_execlist_context_descriptor(
    context_gpu_addr: u64,
    _engine: MediaEngineDescriptor,
    _sw_counter: u32,
    force_restore: bool,
) -> (u32, u32) {
    let (mut lo, _) = build_execlist_context_descriptor_for_gpu_addr(context_gpu_addr);
    if force_restore {
        lo |= CTX_DESC_FORCE_RESTORE;
    }
    let hi =
        ((context_gpu_addr >> 32) as u32) | (media_sw_context_id_for_submit(context_gpu_addr) << 7);
    (lo, hi)
}

pub(super) fn media_ctx_control_value(inhibit_restore: bool) -> u32 {
    let mut ctl =
        masked_bits_update(CTX_CTRL_INHIBIT_SYN_CTX_SWITCH, CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT);
    if inhibit_restore {
        ctl |= CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT;
    }
    ctl
}

pub(super) fn init_gen12_video_context_image(
    context_virt: *mut u8,
    context_len: usize,
    ring_base: usize,
    _ring_head: u32,
    ring_start: u32,
    ring_tail: u32,
    ring_ctl: u32,
    _hws_pga: u32,
    pml4_phys: u64,
    inhibit_restore: bool,
) -> bool {
    const LRC_STATE_OFFSET_DWORDS: usize = 4096 / core::mem::size_of::<u32>();
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
    if state.len() < 192 {
        return false;
    }
    let ring_base = ring_base as u32;
    let mut idx = 0usize;
    state[idx] = MI_NOOP;
    idx += 1;
    state[idx] = mi_lri_cmd(13, MI_LRI_FORCE_POSTED);
    idx += 1;
    state[idx] = ring_base + 0x244;
    state[idx + 1] = media_ctx_control_value(inhibit_restore);
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
    state[idx + 26] = ring_base + 0x5A8;
    state[idx + 27] = 0;
    state[idx + 28] = ring_base + 0x5AC;
    state[idx + 29] = 0;
    idx += 30;
    push_mi_nops(state, &mut idx, 5);
    state[idx] = mi_lri_cmd(9, MI_LRI_FORCE_POSTED);
    idx += 1;
    for (offset, value) in [
        (0x3A8, 0),
        (0x28C, 0),
        (0x288, 0),
        (0x284, 0),
        (0x280, 0),
        (0x27C, 0),
        (0x278, 0),
        (0x274, (pml4_phys >> 32) as u32),
        (0x270, pml4_phys as u32),
    ] {
        state[idx] = ring_base + offset;
        state[idx + 1] = value;
        idx += 2;
    }
    state[idx] = mi_lri_cmd(3, MI_LRI_FORCE_POSTED);
    idx += 1;
    state[idx] = ring_base + 0x1B0;
    state[idx + 1] = 0;
    state[idx + 2] = ring_base + 0x5A8;
    state[idx + 3] = 0;
    state[idx + 4] = ring_base + 0x5AC;
    state[idx + 5] = 0;
    idx += 6;
    push_mi_nops(state, &mut idx, 6);
    state[idx] = mi_lri_cmd(1, MI_LRI_FORCE_POSTED);
    idx += 1;
    state[idx] = ring_base + 0xC8;
    state[idx + 1] = 0x7FFF_FFFF;
    idx += 2;
    push_mi_nops(state, &mut idx, 13);
    state[idx] = mi_lri_cmd(4, MI_LRI_FORCE_POSTED);
    idx += 1;
    state[idx] = ring_base + 0x28;
    state[idx + 1] = 0;
    state[idx + 2] = ring_base + 0x9C;
    state[idx + 3] = masked_bit_disable(STOP_RING);
    state[idx + 4] = ring_base + 0x68;
    state[idx + 5] = 0;
    state[idx + 6] = ring_base + 0x84;
    state[idx + 7] = 0;
    idx += 8;
    push_mi_nops(state, &mut idx, 8);
    state[CTX_RING_TAIL_DW] = ring_tail;
    state[CTX_RING_START_DW] = ring_start;
    state[CTX_RING_CTL_DW] = ring_ctl;
    state[idx] = MI_BATCH_BUFFER_END | 1;
    true
}

pub(super) fn emit_store_dword_ppgtt(
    batch: &mut [u32],
    idx: &mut usize,
    gpu_addr: u64,
    value: u32,
) -> bool {
    if idx.saturating_add(4) > batch.len() {
        return false;
    }
    batch[*idx] = MI_STORE_DWORD_IMM_GEN4_LEN_DW4_PPGTT;
    batch[*idx + 1] = gpu_addr as u32;
    batch[*idx + 2] = (gpu_addr >> 32) as u32;
    batch[*idx + 3] = value;
    *idx += 4;
    true
}

#[inline]
pub(super) fn media_cmd_header(
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

pub(super) fn begin_batch_packet(
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
pub(super) fn packet_write_addr64(
    batch: &mut [u32],
    packet_start: usize,
    dword_index: usize,
    gpu_addr: u64,
) {
    batch[packet_start + dword_index] = gpu_addr as u32;
    batch[packet_start + dword_index + 1] = (gpu_addr >> 32) as u32;
}

pub(super) fn emit_mfx_wait(batch: &mut [u32], idx: &mut usize) -> bool {
    if *idx >= batch.len() {
        return false;
    }
    batch[*idx] = MFX_WAIT_SYNC;
    *idx += 1;
    true
}

#[inline]
pub(super) fn read_result_dword(base_virt: *mut u8, slot_off: u64) -> u32 {
    let ptr = (base_virt as usize).saturating_add(slot_off as usize) as *const u32;
    unsafe { core::ptr::read_volatile(ptr) }
}

pub(super) fn execlist_submit_port_push(
    dev: crate::intel::Dev,
    ring_base: usize,
    context0_lo: u32,
    context0_hi: u32,
    context1_lo: u32,
    context1_hi: u32,
) {
    super::mmio_write(dev, ring_base + RING_EXECLIST_SQ_LO, context0_lo);
    super::mmio_write(dev, ring_base + RING_EXECLIST_SQ_HI, context0_hi);
    super::mmio_write(dev, ring_base + RING_EXECLIST_SQ_LO + 8, context1_lo);
    super::mmio_write(dev, ring_base + RING_EXECLIST_SQ_HI + 8, context1_hi);
}

pub(super) fn wake_media_engine_forcewake(
    dev: crate::intel::Dev,
    engine: MediaEngineDescriptor,
) -> MediaEngineForcewakeAck {
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
    let mut ack_value = 0u32;
    for _ in 0..20_000 {
        ack_value = super::mmio_read(dev, ack);
        if (ack_value & FORCEWAKE_KERNEL) != 0 {
            break;
        }
        core::hint::spin_loop();
    }
    MediaEngineForcewakeAck {
        ack_reg: ack,
        ack_value,
        awake: (ack_value & FORCEWAKE_KERNEL) != 0,
    }
}

const GDRST: usize = 0x0000_941C;
const GRDOM_MEDIA_VCS0: u32 = 1 << 5;
const MODE_IDLE: u32 = 1 << 9;
const GEN12_HWSP_CSB_WRITE_OFFSET: usize = 0xBC;
const GEN12_CSB_RESET_VALUE: u32 = 11;
const GEN12_HWSP_CSB_BUF0_OFFSET: usize = 0x40;
const GEN12_CSB_ENTRIES: usize = 12;

pub(super) fn init_csb_pointers(dev: crate::intel::Dev, ring_base: usize, hwsp_virt: *mut u8) {
    let csb_init: u32 = 0xFFFF_0000 | (GEN12_CSB_RESET_VALUE << 8) | GEN12_CSB_RESET_VALUE;
    super::mmio_write(dev, ring_base + 0x3A0, csb_init);
    let _ = super::mmio_read(dev, ring_base + 0x3A0);
    unsafe {
        core::ptr::write_volatile(
            hwsp_virt.add(GEN12_HWSP_CSB_WRITE_OFFSET) as *mut u32,
            GEN12_CSB_RESET_VALUE,
        );
        let csb_buf = hwsp_virt.add(GEN12_HWSP_CSB_BUF0_OFFSET) as *mut u64;
        for i in 0..GEN12_CSB_ENTRIES {
            core::ptr::write_volatile(csb_buf.add(i), !0u64);
        }
    }
    core::sync::atomic::fence(Ordering::SeqCst);
    super::dma_flush(hwsp_virt, GEN12_HWSP_CSB_WRITE_OFFSET + 8);
    super::mmio_write(dev, ring_base + 0x3A0, csb_init);
    let _ = super::mmio_read(dev, ring_base + 0x3A0);
}

pub(super) fn reset_media_engine(
    dev: crate::intel::Dev,
    engine: MediaEngineDescriptor,
    _context_virt: *mut u8,
) {
    let ring_base = engine.ring_base;
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
    super::ggtt_invalidate(dev);
}

pub(super) fn seed_media_ring_live_state(
    dev: crate::intel::Dev,
    ring_base: usize,
    pphwsp_gpu: u32,
    ring_start: u32,
    ring_ctl: u32,
    ring_tail: u32,
) {
    super::mmio_write(dev, ring_base + RING_HEAD, 0);
    super::mmio_write(dev, ring_base + RING_TAIL, ring_tail);
    super::mmio_write(dev, ring_base + RING_START, ring_start);
    super::mmio_write(dev, ring_base + RING_CTL, ring_ctl);
    super::mmio_write(dev, ring_base + RING_MI_MODE, STOP_RING << 16);
    super::mmio_write(dev, ring_base + RING_MI_MODE, masked_bit_disable(STOP_RING));
    super::mmio_write(dev, ring_base + RING_HWS_PGA, pphwsp_gpu);
    super::mmio_write(dev, ring_base + RING_HWSTAM, !0u32);
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
    let ppgtt_pml4_phys = build_ppgtt_for_ranges(&[
        (windows.batch_gpu_addr, batch_phys, MEDIA_DEFAULT_BATCH_BYTES),
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
    let engine_wake = wake_media_engine_forcewake(dev, engine);
    let wake = snapshot_forcewake(dev);
    unsafe {
        core::ptr::copy_nonoverlapping(encoded.as_ptr(), backing.bitstream_virt, encoded.len());
        let clear_len = backing
            .bitstream_bytes
            .saturating_sub(encoded.len())
            .min(256);
        if clear_len != 0 {
            core::ptr::write_bytes(backing.bitstream_virt.add(encoded.len()), 0, clear_len);
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
        forcewake_engine_ack_reg: engine_wake.ack_reg,
        forcewake_engine_ack: engine_wake.ack_value,
        forcewake_engine_awake: engine_wake.awake,
        forcewake_global_ack: wake.global_ack,
        forcewake_awake_count: wake.awake_count,
    })
}
