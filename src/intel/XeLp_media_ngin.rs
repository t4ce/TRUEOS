extern crate alloc;

use core::{
    ptr,
    sync::atomic::{AtomicBool, Ordering},
};

use alloc::format;
use spin::Mutex;

use super::intel_guc;
use super::intel_igpu770::{
    Igpu770WarmState, build_execlist_context_descriptor_for_gpu_addr, build_ring_batch_start_words,
    cpu_framebuffer_media_status_card_center, cpu_framebuffer_visualize_bytes_center,
    cpu_framebuffer_visualize_nv12_center, dma_cache_flush_range, forcewake_all_acquire,
    forcewake_media_refresh, gen12_lrc_context_control_seed, ggtt_map_system_ram_range,
    init_gen12_video_context_image, mmio_read32, mmio_write32, ring_ctl_value_for_size, warm_state,
};
use super::xelp_media_mp4::{
    build_annex_b_access_unit, first_sample_nal_types, parse_h264_mp4_summary,
};

const MAX_MEDIA_ENGINES: usize = 4;
const MAX_MEDIA_API_ROUTES: usize = 5;
const MAX_MEDIA_RESULT_SLOTS: usize = 4;
const MAX_MEDIA_OBSERVE_REGS: usize = 10;

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
const FORCEWAKE_KERNEL: u32 = 1 << 0;

const FORCEWAKE_ACK_VDBOX0: usize = 0x0D50;
const FORCEWAKE_ACK_VDBOX1: usize = 0x0D54;
const FORCEWAKE_ACK_VDBOX2: usize = 0x0D58;
const FORCEWAKE_ACK_VDBOX3: usize = 0x0D5C;
const FORCEWAKE_ACK_VEBOX0: usize = 0x0D70;
const FORCEWAKE_ACK_VEBOX1: usize = 0x0D74;
const FORCEWAKE_ACK_VEBOX2: usize = 0x0D78;
const FORCEWAKE_ACK_VEBOX3: usize = 0x0D7C;

const GEN11_VCS0_RING_BASE: usize = 0x1C0000;
const GEN11_VCS1_RING_BASE: usize = 0x1C4000;
const GEN11_VECS0_RING_BASE: usize = 0x1C8000;
const GEN11_VECS1_RING_BASE: usize = 0x1D8000;

const RING_TAIL: usize = 0x30;
const RING_HEAD: usize = 0x34;
const RING_START: usize = 0x38;
const RING_CTL: usize = 0x3C;
const RING_PSMI_CTL: usize = 0x50;
const RING_ACTHD: usize = 0x74;
const RING_HWS_PGA: usize = 0x80;
const RING_HWSTAM: usize = 0x98;
const RING_MI_MODE: usize = 0x9C;
const RING_IMR: usize = 0xA8;
const RING_EIR: usize = 0xB0;
const RING_EMR: usize = 0xB4;
const RING_IPEIR: usize = 0x64;
const RING_IPEHR: usize = 0x68;
const RING_INSTDONE: usize = 0x6C;
const RING_INSTPS: usize = 0x70;
const RING_CONTEXT_CONTROL: usize = 0x244;
const RING_CONTEXT_CONTROL_REF: usize = 0x5A0;
const RING_MODE_GEN7: usize = 0x29C;
const RING_RNCID: usize = 0x198;
const RING_EXECLIST_SUBMIT_PORT: usize = 0x230;
const RING_EXECLIST_STATUS_LO: usize = 0x234;
const RING_EXECLIST_STATUS_HI: usize = 0x238;
const RING_EXECLIST_CONTROL: usize = 0x550;
const RING_EXECLIST_SQ_LO: usize = 0x510;
const RING_EXECLIST_SQ_HI: usize = 0x514;
const RING_BBADDR: usize = 0x140;
const RING_BBADDR_UDW: usize = 0x168;

const MSG_IDLE_VCS0: usize = 0x8004;
const MSG_IDLE_VCS1: usize = 0x8008;
const MSG_IDLE_VECS0: usize = 0x8010;
const MSG_IDLE_VECS1: usize = 0x80D8;
const GEN8_RING_FAULT_REG_VCS: usize = 0x4194;
const GEN8_RING_FAULT_REG_VECS: usize = 0x4394;
const GEN12_FAULT_TLB_DATA0: usize = 0xCEB8;
const GEN12_FAULT_TLB_DATA1: usize = 0xCEBC;
const GEN12_RING_FAULT_REG: usize = 0xCEC4;
const GDRST: usize = 0x941C;

const MEDIA_SHARED_STATUS_GPU_ADDR: u64 = 0x0110_0000;
const MEDIA_SHARED_STATUS_BYTES: usize = 0x1000;
const MEDIA_ENGINE_GPU_ADDR_BASE: u64 = 0x0120_0000;
const MEDIA_ENGINE_GPU_ADDR_STRIDE: u64 = 0x0040_0000;

const MEDIA_DEFAULT_RING_BYTES: usize = 16 * 1024;
const MEDIA_DEFAULT_CONTEXT_BYTES: usize = 22 * 4096;
const MEDIA_DEFAULT_BATCH_BYTES: usize = 32 * 1024;
const MEDIA_DEFAULT_SCRATCH_BYTES: usize = 64 * 1024;
const MEDIA_DEFAULT_BITSTREAM_BYTES: usize = 256 * 1024;
const MEDIA_DEFAULT_OUTPUT_SURFACE_BYTES: usize = 4 * 1024 * 1024;
const MEDIA_DEFAULT_RESULT_BYTES: usize = 4 * 4096;
const MEDIA_DEFAULT_WATCHDOG_ITERS: usize = 100_000;
const MEDIA_LRC_STATE_OFFSET_DWORDS: usize = 4096 / core::mem::size_of::<u32>();
const MEDIA_EXECLIST_TRANSPORT_WIRED: bool = true;
const MEDIA_GUC_TRANSPORT_WIRED: bool = false;
const MEDIA_HTTPS_DEMO_URL: &str =
    "https://test-videos.co.uk/vids/bigbuckbunny/mp4/h264/720/Big_Buck_Bunny_720_10s_1MB.mp4";
const MEDIA_HTTPS_DEMO_TIMEOUT_MS: u32 = 45_000;
const MEDIA_HTTPS_DEMO_MAX_BYTES: usize = 2 * 1024 * 1024;
const MEDIA_HTTPS_DEMO_VIS_W: usize = 128;
const MEDIA_HTTPS_DEMO_VIS_H: usize = 72;
const MEDIA_SUBMIT_POLL_LOG_STEP: usize = 10_000;
const MEDIA_VCS_BATCH_MODE_MI_ONLY_PROBE: u32 = 1 << 2;
const MEDIA_MI_ONLY_PROBE_PREFIX_NOOPS: usize = 8;

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

const MI_STORE_DWORD_IMM_GEN4: u32 = (0x20 << 23) | 2;
const MI_USE_GGTT: u32 = 1 << 22;
const MI_STORE_DWORD_IMM_GEN4_LEN_DW4: u32 = MI_STORE_DWORD_IMM_GEN4 | MI_USE_GGTT | (4 - 2);
const MI_FLUSH_DW: u32 = (0x26 << 23) | 1;
const MI_FLUSH_DW_VIDEO_PIPELINE_CACHE_INVALIDATE: u32 = 1 << 16;
const MI_FLUSH_DW_USE_GTT: u32 = 1 << 2;
const MI_ARB_CHECK: u32 = 0x0280_0000;
const MI_BATCH_BUFFER_END: u32 = 0x0500_0000;
const MI_NOOP: u32 = 0;

const EL_CTRL_LOAD: u32 = 1 << 0;
const CTX_DESC_VALID: u32 = 1 << 0;
const CTX_DESC_FORCE_RESTORE: u32 = 1 << 2;
const CTX_CTRL_RS_CTX_ENABLE: u32 = 1 << 1;
const CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT: u32 = 1 << 0;
const CTX_CTRL_ENGINE_CTX_SAVE_INHIBIT: u32 = 1 << 2;
const CTX_CTRL_INHIBIT_SYN_CTX_SWITCH: u32 = 1 << 3;
const CTX_DESC_PRIVILEGE: u32 = 1 << 8;
const CTX_DESC_PRIORITY_NORMAL: u32 = 1 << 9;
const CTX_DESC_ADDRESSING_MODE_SHIFT: u32 = 3;
const CTX_DESC_ADDRESSING_MODE_MASK: u32 = 0x7;
const GEN11_GFX_DISABLE_LEGACY_MODE: u32 = 1 << 3;
const STOP_RING: u32 = 1 << 8;
const PSMI_SLEEP_MSG_DISABLE: u32 = 1 << 0;
const BSD_SLEEP_INDICATOR: u32 = 1 << 3;
const SW_CTX_ID_SHIFT: u32 = 37;

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

const MEDIA_SLICE_ACK_REGS: [(&str, usize); 8] = [
    ("vdbox0", FORCEWAKE_ACK_VDBOX0),
    ("vdbox1", FORCEWAKE_ACK_VDBOX1),
    ("vdbox2", FORCEWAKE_ACK_VDBOX2),
    ("vdbox3", FORCEWAKE_ACK_VDBOX3),
    ("vebox0", FORCEWAKE_ACK_VEBOX0),
    ("vebox1", FORCEWAKE_ACK_VEBOX1),
    ("vebox2", FORCEWAKE_ACK_VEBOX2),
    ("vebox3", FORCEWAKE_ACK_VEBOX3),
];

const MEDIA_OBSERVE_REG_OFFSETS: [usize; MAX_MEDIA_OBSERVE_REGS] = [
    RING_TAIL,
    RING_HEAD,
    RING_START,
    RING_CTL,
    RING_ACTHD,
    RING_MI_MODE,
    RING_MODE_GEN7,
    RING_CONTEXT_CONTROL,
    RING_EXECLIST_STATUS_LO,
    RING_EXECLIST_STATUS_HI,
];

static MEDIA_HTTPS_DEMO_RAN: AtomicBool = AtomicBool::new(false);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum MediaEngineClass {
    VideoDecode,
    VideoEnhancement,
}

impl MediaEngineClass {
    const fn as_str(self) -> &'static str {
        match self {
            Self::VideoDecode => "video-decode",
            Self::VideoEnhancement => "video-enhancement",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum MediaProvisioning {
    Kickoff,
    ScaleOutReserve,
    Disabled,
}

impl MediaProvisioning {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Kickoff => "kickoff",
            Self::ScaleOutReserve => "scaleout-reserve",
            Self::Disabled => "disabled",
        }
    }
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

impl MediaWorkloadKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::DecodeBitstream => "decode-bitstream",
            Self::DecodeFrame => "decode-frame",
            Self::EnhanceFrame => "enhance-frame",
            Self::SessionSnapshot => "session-snapshot",
            Self::EngineReset => "engine-reset",
            Self::Smoke => "smoke",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum MediaSubmissionTransport {
    GuC,
    Execlists,
    Disabled,
}

impl MediaSubmissionTransport {
    const fn as_str(self) -> &'static str {
        match self {
            Self::GuC => "guc",
            Self::Execlists => "execlists",
            Self::Disabled => "disabled",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum MediaVcsBatchMode {
    MiOnlyProbe,
    H264Decode,
}

impl MediaVcsBatchMode {
    const fn as_str(self) -> &'static str {
        match self {
            Self::MiOnlyProbe => "mi-only-probe",
            Self::H264Decode => "h264-decode",
        }
    }
}

const ACTIVE_MEDIA_VCS_BATCH_MODE: MediaVcsBatchMode = MediaVcsBatchMode::MiOnlyProbe;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum MediaVcsSubmitMode {
    RingInline,
    BatchStart,
}

impl MediaVcsSubmitMode {
    const fn as_str(self) -> &'static str {
        match self {
            Self::RingInline => "ring-inline",
            Self::BatchStart => "batch-start",
        }
    }
}

const ACTIVE_MEDIA_VCS_SUBMIT_MODE: MediaVcsSubmitMode = MediaVcsSubmitMode::RingInline;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum MediaCompletionMode {
    ResultMemoryPoll,
    ExeclistStatusPoll,
    None,
}

impl MediaCompletionMode {
    const fn as_str(self) -> &'static str {
        match self {
            Self::ResultMemoryPoll => "result-memory-poll",
            Self::ExeclistStatusPoll => "execlist-status-poll",
            Self::None => "none",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum MediaKickoffStage {
    Discovery,
    ResourcePlanning,
    SubmissionWiring,
    CommandEncoding,
    Smoke,
}

impl MediaKickoffStage {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Discovery => "discovery",
            Self::ResourcePlanning => "resource-planning",
            Self::SubmissionWiring => "submission-wiring",
            Self::CommandEncoding => "command-encoding",
            Self::Smoke => "smoke",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MediaEngineId {
    pub class: MediaEngineClass,
    pub instance: u8,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct MediaCapabilities {
    pub decode: bool,
    pub enhance: bool,
    pub huc_assist: bool,
    pub sfc: bool,
    pub relative_mmio_lrc: bool,
}

impl MediaCapabilities {
    const fn none() -> Self {
        Self {
            decode: false,
            enhance: false,
            huc_assist: false,
            sfc: false,
            relative_mmio_lrc: false,
        }
    }
}

#[derive(Copy, Clone, Debug)]
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
            capabilities: MediaCapabilities::none(),
            default_workload: MediaWorkloadKind::SessionSnapshot,
        }
    }

    const fn supports_workload(self, workload: MediaWorkloadKind) -> bool {
        match workload {
            MediaWorkloadKind::DecodeBitstream | MediaWorkloadKind::DecodeFrame => {
                self.capabilities.decode
            }
            MediaWorkloadKind::EnhanceFrame => self.capabilities.enhance,
            MediaWorkloadKind::SessionSnapshot
            | MediaWorkloadKind::EngineReset
            | MediaWorkloadKind::Smoke => {
                matches!(
                    self.provisioning,
                    MediaProvisioning::Kickoff | MediaProvisioning::ScaleOutReserve
                )
            }
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct MediaGpuWindowLayout {
    pub ring_gpu_addr: u64,
    pub context_gpu_addr: u64,
    pub batch_gpu_addr: u64,
    pub scratch_gpu_addr: u64,
    pub bitstream_gpu_addr: u64,
    pub output_surface_gpu_addr: u64,
    pub result_gpu_addr: u64,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct MediaResourcePlan {
    pub shared_status_gpu_addr: u64,
    pub shared_status_bytes: usize,
    pub ring_bytes: usize,
    pub context_bytes: usize,
    pub batch_bytes: usize,
    pub scratch_bytes: usize,
    pub bitstream_bytes: usize,
    pub output_surface_bytes: usize,
    pub result_bytes: usize,
    pub windows: MediaGpuWindowLayout,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct MediaContextPlan {
    pub bytes: usize,
    pub lrc_state_offset_dwords: usize,
    pub indirect_state_dwords_budget: usize,
    pub uses_relative_mmio: bool,
    pub engine_mmio_base: usize,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct MediaBatchTemplate {
    pub preamble_dwords: usize,
    pub payload_budget_dwords: usize,
    pub epilogue_dwords: usize,
    pub completion_slot_gpu_addr: u64,
    pub completion_marker_value: u32,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct MediaSubmissionPlan {
    pub workload: MediaWorkloadKind,
    pub transport: MediaSubmissionTransport,
    pub completion: MediaCompletionMode,
    pub queue_depth: usize,
    pub watchdog_iters: usize,
    pub prefers_parallel_submission: bool,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct MediaResultSlot {
    pub name: &'static str,
    pub gpu_addr: u64,
    pub expected_marker: u32,
}

impl MediaResultSlot {
    const fn empty() -> Self {
        Self {
            name: "unused",
            gpu_addr: 0,
            expected_marker: 0,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct MediaObservabilityPlan {
    pub label: &'static str,
    pub result_slots: [MediaResultSlot; MAX_MEDIA_RESULT_SLOTS],
    pub reg_count: usize,
    pub reg_offsets: [usize; MAX_MEDIA_OBSERVE_REGS],
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct MediaEnginePlan {
    pub descriptor: MediaEngineDescriptor,
    pub resources: MediaResourcePlan,
    pub context: MediaContextPlan,
    pub batch: MediaBatchTemplate,
    pub submission: MediaSubmissionPlan,
    pub observability: MediaObservabilityPlan,
    pub next_stage: MediaKickoffStage,
}

impl MediaEnginePlan {
    const fn empty() -> Self {
        Self {
            descriptor: MediaEngineDescriptor::unused(),
            resources: MediaResourcePlan {
                shared_status_gpu_addr: 0,
                shared_status_bytes: 0,
                ring_bytes: 0,
                context_bytes: 0,
                batch_bytes: 0,
                scratch_bytes: 0,
                bitstream_bytes: 0,
                output_surface_bytes: 0,
                result_bytes: 0,
                windows: MediaGpuWindowLayout {
                    ring_gpu_addr: 0,
                    context_gpu_addr: 0,
                    batch_gpu_addr: 0,
                    scratch_gpu_addr: 0,
                    bitstream_gpu_addr: 0,
                    output_surface_gpu_addr: 0,
                    result_gpu_addr: 0,
                },
            },
            context: MediaContextPlan {
                bytes: 0,
                lrc_state_offset_dwords: 0,
                indirect_state_dwords_budget: 0,
                uses_relative_mmio: false,
                engine_mmio_base: 0,
            },
            batch: MediaBatchTemplate {
                preamble_dwords: 0,
                payload_budget_dwords: 0,
                epilogue_dwords: 0,
                completion_slot_gpu_addr: 0,
                completion_marker_value: 0,
            },
            submission: MediaSubmissionPlan {
                workload: MediaWorkloadKind::SessionSnapshot,
                transport: MediaSubmissionTransport::Disabled,
                completion: MediaCompletionMode::None,
                queue_depth: 0,
                watchdog_iters: 0,
                prefers_parallel_submission: false,
            },
            observability: MediaObservabilityPlan {
                label: "unused",
                result_slots: [MediaResultSlot::empty(); MAX_MEDIA_RESULT_SLOTS],
                reg_count: 0,
                reg_offsets: [0; MAX_MEDIA_OBSERVE_REGS],
            },
            next_stage: MediaKickoffStage::Discovery,
        }
    }
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
    const fn empty() -> Self {
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

impl MediaTopology {
    const fn empty() -> Self {
        Self {
            sku_name: "uninitialized",
            active_engine_count: 0,
            planned_engine_count: 0,
            engines: [MediaEngineDescriptor::unused(); MAX_MEDIA_ENGINES],
            default_decode: None,
            default_enhance: None,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct MediaKickoffState {
    pub topology: MediaTopology,
    pub plan_count: usize,
    pub plans: [MediaEnginePlan; MAX_MEDIA_ENGINES],
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
pub(crate) struct MediaJobDraft {
    pub engine: MediaEngineDescriptor,
    pub resources: MediaResourcePlan,
    pub context: MediaContextPlan,
    pub batch: MediaBatchTemplate,
    pub submission: MediaSubmissionPlan,
    pub observability: MediaObservabilityPlan,
    pub next_stage: MediaKickoffStage,
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
    pub submit_iters: usize,
}

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
}

unsafe impl Send for MediaBitstreamBacking {}
unsafe impl Sync for MediaBitstreamBacking {}

static MEDIA_KICKOFF_RAN: AtomicBool = AtomicBool::new(false);
static MEDIA_KICKOFF_STATE: Mutex<Option<MediaKickoffState>> = Mutex::new(None);
static MEDIA_BITSTREAM_BACKING: Mutex<Option<MediaBitstreamBacking>> = Mutex::new(None);

#[inline]
const fn media_completion_slot_addr(base: u64, slot_off: u64) -> u64 {
    base + slot_off
}

#[inline]
const fn preferred_transport(guc_ready: bool) -> MediaSubmissionTransport {
    if MEDIA_GUC_TRANSPORT_WIRED && guc_ready {
        MediaSubmissionTransport::GuC
    } else if MEDIA_EXECLIST_TRANSPORT_WIRED {
        MediaSubmissionTransport::Execlists
    } else {
        MediaSubmissionTransport::Disabled
    }
}

#[inline]
fn media_command_encoding_ready(guc_ready: bool, wake: MediaForcewakeSnapshot) -> bool {
    let wake_ready = ((wake.global_ack & FORCEWAKE_KERNEL) != 0) || wake.awake_count != 0;
    wake_ready && ((MEDIA_EXECLIST_TRANSPORT_WIRED) || (MEDIA_GUC_TRANSPORT_WIRED && guc_ready))
}

fn stored_kickoff_state() -> Option<MediaKickoffState> {
    *MEDIA_KICKOFF_STATE.lock()
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
        sku_name: "xelp-media-kickoff",
        active_engine_count: 2,
        planned_engine_count: 4,
        engines: [decode0, enhance0, decode1, enhance1],
        default_decode: Some(decode0.id),
        default_enhance: Some(enhance0.id),
    }
}

fn current_api_shape(transport: MediaSubmissionTransport) -> MediaApiShape {
    let mut api = MediaApiShape::empty();
    api.route_count = 5;
    api.routes[0] = MediaApiRoute {
        name: "media.decode.submit",
        workload: MediaWorkloadKind::DecodeFrame,
        preferred_engine_class: Some(MediaEngineClass::VideoDecode),
        transport,
        summary: "submit a decode-oriented session through the planned VCS path",
    };
    api.routes[1] = MediaApiRoute {
        name: "media.decode.bitstream",
        workload: MediaWorkloadKind::DecodeBitstream,
        preferred_engine_class: Some(MediaEngineClass::VideoDecode),
        transport,
        summary: "reserve decode staging, patch buffers, and queue a bitstream-style workload",
    };
    api.routes[2] = MediaApiRoute {
        name: "media.enhance.submit",
        workload: MediaWorkloadKind::EnhanceFrame,
        preferred_engine_class: Some(MediaEngineClass::VideoEnhancement),
        transport,
        summary: "submit a VEBox-style enhancement workload through the kickoff API",
    };
    api.routes[3] = MediaApiRoute {
        name: "media.observe.snapshot",
        workload: MediaWorkloadKind::SessionSnapshot,
        preferred_engine_class: None,
        transport,
        summary: "snapshot the planned media rings, result slots, and forcewake coverage",
    };
    api.routes[4] = MediaApiRoute {
        name: "media.engine.smoke",
        workload: MediaWorkloadKind::Smoke,
        preferred_engine_class: None,
        transport,
        summary: "run a future media-engine smoke path with the same kickoff resource layout",
    };
    api
}

fn engine_window(slot: usize) -> MediaGpuWindowLayout {
    let base = MEDIA_ENGINE_GPU_ADDR_BASE + (slot as u64) * MEDIA_ENGINE_GPU_ADDR_STRIDE;
    MediaGpuWindowLayout {
        ring_gpu_addr: base,
        context_gpu_addr: base + 0x0001_0000,
        batch_gpu_addr: base + 0x0008_0000,
        scratch_gpu_addr: base + 0x0010_0000,
        bitstream_gpu_addr: base + 0x0014_0000,
        output_surface_gpu_addr: base + 0x0020_0000,
        result_gpu_addr: base + 0x0030_0000,
    }
}

fn marker_base(desc: MediaEngineDescriptor) -> u32 {
    let class_base = match desc.id.class {
        MediaEngineClass::VideoDecode => 0x4D44_1000,
        MediaEngineClass::VideoEnhancement => 0x4D45_1000,
    };
    class_base + (desc.id.instance as u32) * 0x100
}

fn observability_plan(
    desc: MediaEngineDescriptor,
    resources: MediaResourcePlan,
) -> MediaObservabilityPlan {
    let base = marker_base(desc);
    MediaObservabilityPlan {
        label: desc.name,
        result_slots: [
            MediaResultSlot {
                name: "kickoff",
                gpu_addr: media_completion_slot_addr(
                    resources.windows.result_gpu_addr,
                    MEDIA_RESULT_KICKOFF_SLOT,
                ),
                expected_marker: base,
            },
            MediaResultSlot {
                name: "pre-submit",
                gpu_addr: media_completion_slot_addr(
                    resources.windows.result_gpu_addr,
                    MEDIA_RESULT_PRESUBMIT_SLOT,
                ),
                expected_marker: base + 1,
            },
            MediaResultSlot {
                name: "post-submit",
                gpu_addr: media_completion_slot_addr(
                    resources.windows.result_gpu_addr,
                    MEDIA_RESULT_POSTSUBMIT_SLOT,
                ),
                expected_marker: base + 2,
            },
            MediaResultSlot {
                name: "complete",
                gpu_addr: media_completion_slot_addr(
                    resources.windows.result_gpu_addr,
                    MEDIA_RESULT_COMPLETE_SLOT,
                ),
                expected_marker: base + 3,
            },
        ],
        reg_count: MEDIA_OBSERVE_REG_OFFSETS.len(),
        reg_offsets: MEDIA_OBSERVE_REG_OFFSETS,
    }
}

fn build_engine_plan(
    slot: usize,
    desc: MediaEngineDescriptor,
    guc_ready: bool,
    command_encoding_ready: bool,
) -> MediaEnginePlan {
    let resources = MediaResourcePlan {
        shared_status_gpu_addr: MEDIA_SHARED_STATUS_GPU_ADDR,
        shared_status_bytes: MEDIA_SHARED_STATUS_BYTES,
        ring_bytes: MEDIA_DEFAULT_RING_BYTES,
        context_bytes: MEDIA_DEFAULT_CONTEXT_BYTES,
        batch_bytes: MEDIA_DEFAULT_BATCH_BYTES,
        scratch_bytes: MEDIA_DEFAULT_SCRATCH_BYTES,
        bitstream_bytes: MEDIA_DEFAULT_BITSTREAM_BYTES,
        output_surface_bytes: MEDIA_DEFAULT_OUTPUT_SURFACE_BYTES,
        result_bytes: MEDIA_DEFAULT_RESULT_BYTES,
        windows: engine_window(slot),
    };
    let observability = observability_plan(desc, resources);
    let payload_budget_dwords = match desc.id.class {
        MediaEngineClass::VideoDecode => 768,
        MediaEngineClass::VideoEnhancement => 384,
    };
    let preamble_dwords = match desc.id.class {
        MediaEngineClass::VideoDecode => 48,
        MediaEngineClass::VideoEnhancement => 32,
    };
    let indirect_state_dwords_budget = match desc.id.class {
        MediaEngineClass::VideoDecode => 160,
        MediaEngineClass::VideoEnhancement => 128,
    };
    let prefers_parallel_submission = desc.provisioning == MediaProvisioning::ScaleOutReserve;
    let transport = if desc.provisioning == MediaProvisioning::Disabled {
        MediaSubmissionTransport::Disabled
    } else {
        preferred_transport(guc_ready)
    };
    let next_stage = match desc.provisioning {
        MediaProvisioning::Kickoff if command_encoding_ready => MediaKickoffStage::CommandEncoding,
        MediaProvisioning::Kickoff => MediaKickoffStage::SubmissionWiring,
        MediaProvisioning::ScaleOutReserve => MediaKickoffStage::ResourcePlanning,
        MediaProvisioning::Disabled => MediaKickoffStage::Discovery,
    };

    MediaEnginePlan {
        descriptor: desc,
        resources,
        context: MediaContextPlan {
            bytes: resources.context_bytes,
            lrc_state_offset_dwords: MEDIA_LRC_STATE_OFFSET_DWORDS,
            indirect_state_dwords_budget,
            uses_relative_mmio: desc.capabilities.relative_mmio_lrc,
            engine_mmio_base: desc.ring_base,
        },
        batch: MediaBatchTemplate {
            preamble_dwords,
            payload_budget_dwords,
            epilogue_dwords: 16,
            completion_slot_gpu_addr: observability.result_slots[3].gpu_addr,
            completion_marker_value: observability.result_slots[3].expected_marker,
        },
        submission: MediaSubmissionPlan {
            workload: desc.default_workload,
            transport,
            completion: MediaCompletionMode::ResultMemoryPoll,
            queue_depth: if desc.id.class == MediaEngineClass::VideoDecode {
                2
            } else {
                1
            },
            watchdog_iters: MEDIA_DEFAULT_WATCHDOG_ITERS,
            prefers_parallel_submission,
        },
        observability,
        next_stage,
    }
}

fn snapshot_forcewake(warm: Igpu770WarmState) -> MediaForcewakeSnapshot {
    let mut slices = [MediaSliceWakeAck::empty(); 8];
    let mut awake_count = 0usize;
    let mut idx = 0usize;
    while idx < MEDIA_SLICE_ACK_REGS.len() {
        let value = mmio_read32(warm, MEDIA_SLICE_ACK_REGS[idx].1);
        let awake = (value & FORCEWAKE_KERNEL) != 0;
        if awake {
            awake_count += 1;
        }
        slices[idx] = MediaSliceWakeAck {
            name: MEDIA_SLICE_ACK_REGS[idx].0,
            value,
            awake,
        };
        idx += 1;
    }

    MediaForcewakeSnapshot {
        global_req: mmio_read32(warm, FORCEWAKE_MEDIA_GEN11),
        global_ack: mmio_read32(warm, FORCEWAKE_ACK_MEDIA),
        awake_count,
        slice_count: MEDIA_SLICE_ACK_REGS.len(),
        slices,
    }
}

fn snapshot_engine_runtime(
    warm: Igpu770WarmState,
    desc: MediaEngineDescriptor,
) -> MediaEngineRuntimeSnapshot {
    let base = desc.ring_base;
    let tail = mmio_read32(warm, base + RING_TAIL);
    let head = mmio_read32(warm, base + RING_HEAD);
    let start = mmio_read32(warm, base + RING_START);
    let ctl = mmio_read32(warm, base + RING_CTL);
    let acthd = mmio_read32(warm, base + RING_ACTHD);
    let mi_mode = mmio_read32(warm, base + RING_MI_MODE);
    let mode = mmio_read32(warm, base + RING_MODE_GEN7);
    let ctx_ctl = mmio_read32(warm, base + RING_CONTEXT_CONTROL);
    let execlist_ctl = mmio_read32(warm, base + RING_EXECLIST_CONTROL);
    let execlist_status_lo = mmio_read32(warm, base + RING_EXECLIST_STATUS_LO);
    let execlist_status_hi = mmio_read32(warm, base + RING_EXECLIST_STATUS_HI);
    let ipeir = mmio_read32(warm, base + RING_IPEIR);
    let ipehr = mmio_read32(warm, base + RING_IPEHR);
    let instdone = mmio_read32(warm, base + RING_INSTDONE);
    let instps = mmio_read32(warm, base + RING_INSTPS);
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

#[inline]
fn media_msg_idle_reg(desc: MediaEngineDescriptor) -> Option<usize> {
    match (desc.id.class, desc.id.instance) {
        (MediaEngineClass::VideoDecode, 0) => Some(MSG_IDLE_VCS0),
        (MediaEngineClass::VideoDecode, 1) => Some(MSG_IDLE_VCS1),
        (MediaEngineClass::VideoEnhancement, 0) => Some(MSG_IDLE_VECS0),
        (MediaEngineClass::VideoEnhancement, 1) => Some(MSG_IDLE_VECS1),
        _ => None,
    }
}

#[inline]
fn media_legacy_ring_fault_reg(desc: MediaEngineDescriptor) -> usize {
    match desc.id.class {
        MediaEngineClass::VideoDecode => GEN8_RING_FAULT_REG_VCS,
        MediaEngineClass::VideoEnhancement => GEN8_RING_FAULT_REG_VECS,
    }
}

#[inline]
fn media_forcewake_regs(desc: MediaEngineDescriptor) -> Option<(&'static str, usize, usize)> {
    match (desc.id.class, desc.id.instance) {
        (MediaEngineClass::VideoDecode, 0) => {
            Some(("vdbox0", FORCEWAKE_MEDIA_VDBOX0, FORCEWAKE_ACK_VDBOX0))
        }
        (MediaEngineClass::VideoDecode, 1) => {
            Some(("vdbox1", FORCEWAKE_MEDIA_VDBOX1, FORCEWAKE_ACK_VDBOX1))
        }
        (MediaEngineClass::VideoDecode, 2) => {
            Some(("vdbox2", FORCEWAKE_MEDIA_VDBOX2, FORCEWAKE_ACK_VDBOX2))
        }
        (MediaEngineClass::VideoDecode, 3) => {
            Some(("vdbox3", FORCEWAKE_MEDIA_VDBOX3, FORCEWAKE_ACK_VDBOX3))
        }
        (MediaEngineClass::VideoEnhancement, 0) => {
            Some(("vebox0", FORCEWAKE_MEDIA_VEBOX0, FORCEWAKE_ACK_VEBOX0))
        }
        (MediaEngineClass::VideoEnhancement, 1) => {
            Some(("vebox1", FORCEWAKE_MEDIA_VEBOX1, FORCEWAKE_ACK_VEBOX1))
        }
        (MediaEngineClass::VideoEnhancement, 2) => {
            Some(("vebox2", FORCEWAKE_MEDIA_VEBOX2, FORCEWAKE_ACK_VEBOX2))
        }
        (MediaEngineClass::VideoEnhancement, 3) => {
            Some(("vebox3", FORCEWAKE_MEDIA_VEBOX3, FORCEWAKE_ACK_VEBOX3))
        }
        _ => None,
    }
}

fn wake_media_engine_forcewake(
    warm: Igpu770WarmState,
    desc: MediaEngineDescriptor,
    label: &str,
) -> bool {
    let Some((domain, req_reg, ack_reg)) = media_forcewake_regs(desc) else {
        crate::log!(
            "intel/media-demo: engine-forcewake label={} engine={} domain=unmapped\n",
            label,
            desc.name
        );
        return false;
    };

    let _ = mmio_write32(warm, req_reg, masked_bit_enable(FORCEWAKE_KERNEL));

    let mut req = mmio_read32(warm, req_reg);
    let mut req_iters = 0usize;
    while req_iters < MEDIA_DEFAULT_WATCHDOG_ITERS / 10 {
        if (req & FORCEWAKE_KERNEL) != 0 {
            break;
        }
        core::hint::spin_loop();
        req_iters += 1;
        req = mmio_read32(warm, req_reg);
    }

    let mut ack = mmio_read32(warm, ack_reg);
    let mut ack_iters = 0usize;
    while ack_iters < MEDIA_DEFAULT_WATCHDOG_ITERS / 10 {
        if (ack & FORCEWAKE_KERNEL) != 0 {
            break;
        }
        core::hint::spin_loop();
        ack_iters += 1;
        ack = mmio_read32(warm, ack_reg);
    }

    crate::log!(
        "intel/media-demo: engine-forcewake label={} engine={} domain={} req=0x{:08X} ack=0x{:08X} req_latched={} acked={} req_iters={} ack_iters={}\n",
        label,
        desc.name,
        domain,
        req,
        ack,
        ((req & FORCEWAKE_KERNEL) != 0) as u8,
        ((ack & FORCEWAKE_KERNEL) != 0) as u8,
        req_iters,
        ack_iters
    );

    (ack & FORCEWAKE_KERNEL) != 0
}

fn wake_media_ring_for_submit(
    warm: Igpu770WarmState,
    desc: MediaEngineDescriptor,
    label: &str,
) -> bool {
    let psmi_reg = desc.ring_base + RING_PSMI_CTL;
    let _ = mmio_write32(warm, psmi_reg, masked_bit_enable(PSMI_SLEEP_MSG_DISABLE));
    let _ = mmio_write32(warm, desc.ring_base + RING_RNCID, 0);

    let mut iter = 0usize;
    let mut psmi = mmio_read32(warm, psmi_reg);
    while iter < MEDIA_DEFAULT_WATCHDOG_ITERS / 10 {
        if (psmi & BSD_SLEEP_INDICATOR) == 0 {
            break;
        }
        core::hint::spin_loop();
        iter += 1;
        psmi = mmio_read32(warm, psmi_reg);
    }

    crate::log!(
        "intel/media-demo: ring-wake label={} engine={} psmi=0x{:08X} slept={} iters={}\n",
        label,
        desc.name,
        psmi,
        ((psmi & BSD_SLEEP_INDICATOR) != 0) as u8,
        iter
    );
    (psmi & BSD_SLEEP_INDICATOR) == 0
}

fn execlist_submit_port_push(
    warm: Igpu770WarmState,
    desc: MediaEngineDescriptor,
    context0_lo: u32,
    context0_hi: u32,
    context1_lo: u32,
    context1_hi: u32,
) {
    let submit_port = desc.ring_base + RING_EXECLIST_SUBMIT_PORT;
    let _ = mmio_write32(warm, submit_port, context0_lo);
    let _ = mmio_write32(warm, submit_port, context0_hi);
    let _ = mmio_write32(warm, submit_port, context1_lo);
    let _ = mmio_write32(warm, submit_port, context1_hi);
}

fn log_media_submit_diag(
    warm: Igpu770WarmState,
    desc: MediaEngineDescriptor,
    phase: &str,
    kickoff: u32,
    complete: u32,
) {
    let base = desc.ring_base;
    let msg_idle = media_msg_idle_reg(desc)
        .map(|off| mmio_read32(warm, off))
        .unwrap_or(0);
    let legacy_fault = mmio_read32(warm, media_legacy_ring_fault_reg(desc));
    let ring_fault = mmio_read32(warm, GEN12_RING_FAULT_REG);
    let fault_tlb0 = mmio_read32(warm, GEN12_FAULT_TLB_DATA0);
    let fault_tlb1 = mmio_read32(warm, GEN12_FAULT_TLB_DATA1);
    let forcewake = snapshot_forcewake(warm);
    let (engine_forcewake_req, engine_forcewake_ack) =
        if let Some((_, req_reg, ack_reg)) = media_forcewake_regs(desc) {
            (mmio_read32(warm, req_reg), mmio_read32(warm, ack_reg))
        } else {
            (0, 0)
        };
    crate::log!(
        "intel/media-demo: {} diag engine={} msg_idle=0x{:08X} head=0x{:08X} tail=0x{:08X} start=0x{:08X} ctl=0x{:08X} acthd=0x{:08X} hws=0x{:08X} hwstam=0x{:08X} bbaddr=0x{:08X} bbaddr_udw=0x{:08X} ipeir=0x{:08X} ipehr=0x{:08X} instdone=0x{:08X} instps=0x{:08X} imr=0x{:08X} eir=0x{:08X} emr=0x{:08X} mi_mode=0x{:08X} mode=0x{:08X} ctx_ctl=0x{:08X} ctx_ref=0x{:08X} el_ctl=0x{:08X} el_sq_lo=0x{:08X} el_sq_hi=0x{:08X} el_status_lo=0x{:08X} el_status_hi=0x{:08X} gdrst=0x{:08X} legacy_fault=0x{:08X} ring_fault=0x{:08X} tlb0=0x{:08X} tlb1=0x{:08X} media_req=0x{:08X} media_ack=0x{:08X} engine_req=0x{:08X} engine_ack=0x{:08X} awake={}/{} kickoff=0x{:08X} complete=0x{:08X}\n",
        phase,
        desc.name,
        msg_idle,
        mmio_read32(warm, base + RING_HEAD),
        mmio_read32(warm, base + RING_TAIL),
        mmio_read32(warm, base + RING_START),
        mmio_read32(warm, base + RING_CTL),
        mmio_read32(warm, base + RING_ACTHD),
        mmio_read32(warm, base + RING_HWS_PGA),
        mmio_read32(warm, base + RING_HWSTAM),
        mmio_read32(warm, base + RING_BBADDR),
        mmio_read32(warm, base + RING_BBADDR_UDW),
        mmio_read32(warm, base + RING_IPEIR),
        mmio_read32(warm, base + RING_IPEHR),
        mmio_read32(warm, base + RING_INSTDONE),
        mmio_read32(warm, base + RING_INSTPS),
        mmio_read32(warm, base + RING_IMR),
        mmio_read32(warm, base + RING_EIR),
        mmio_read32(warm, base + RING_EMR),
        mmio_read32(warm, base + RING_MI_MODE),
        mmio_read32(warm, base + RING_MODE_GEN7),
        mmio_read32(warm, base + RING_CONTEXT_CONTROL),
        mmio_read32(warm, base + RING_CONTEXT_CONTROL_REF),
        mmio_read32(warm, base + RING_EXECLIST_CONTROL),
        mmio_read32(warm, base + RING_EXECLIST_SQ_LO),
        mmio_read32(warm, base + RING_EXECLIST_SQ_HI),
        mmio_read32(warm, base + RING_EXECLIST_STATUS_LO),
        mmio_read32(warm, base + RING_EXECLIST_STATUS_HI),
        mmio_read32(warm, GDRST),
        legacy_fault,
        ring_fault,
        fault_tlb0,
        fault_tlb1,
        forcewake.global_req,
        forcewake.global_ack,
        engine_forcewake_req,
        engine_forcewake_ack,
        forcewake.awake_count,
        forcewake.slice_count,
        kickoff,
        complete
    );
}

fn find_context_lri_value(state: &[u32], reg: u32) -> Option<u32> {
    let mut idx = 0usize;
    while idx + 1 < state.len() {
        if state[idx] == reg {
            return Some(state[idx + 1]);
        }
        idx += 1;
    }
    None
}

#[inline]
fn media_sw_ctx_id(desc: MediaEngineDescriptor) -> u32 {
    match desc.id.class {
        MediaEngineClass::VideoDecode => 1 + desc.id.instance as u32,
        MediaEngineClass::VideoEnhancement => 3 + desc.id.instance as u32,
    }
}

#[inline]
fn with_sw_context_id(desc_lo: u32, desc_hi: u32, sw_ctx_id: u32) -> (u32, u32) {
    let hi_shift = SW_CTX_ID_SHIFT - 32;
    (desc_lo, desc_hi | (sw_ctx_id << hi_shift))
}

fn log_video_context_image(
    desc: MediaEngineDescriptor,
    context_virt: *mut u8,
    context_bytes: usize,
    context_gpu_addr: u64,
) {
    if context_virt.is_null() || context_bytes < (MEDIA_LRC_STATE_OFFSET_DWORDS + 32) * 4 {
        crate::log!(
            "intel/media-demo: lrc-image engine={} unavailable context_gpu=0x{:X} bytes=0x{:X}\n",
            desc.name,
            context_gpu_addr,
            context_bytes
        );
        return;
    }

    let dwords = unsafe {
        core::slice::from_raw_parts(
            context_virt as *const u32,
            context_bytes / core::mem::size_of::<u32>(),
        )
    };
    let state = &dwords[MEDIA_LRC_STATE_OFFSET_DWORDS..];
    let ring_base = desc.ring_base as u32;
    let lri_ctx =
        find_context_lri_value(state, ring_base + RING_CONTEXT_CONTROL as u32).unwrap_or(0);
    let lri_head = find_context_lri_value(state, ring_base + RING_HEAD as u32).unwrap_or(0);
    let lri_tail = find_context_lri_value(state, ring_base + RING_TAIL as u32).unwrap_or(0);
    let lri_start = find_context_lri_value(state, ring_base + RING_START as u32).unwrap_or(0);
    let lri_ctl = find_context_lri_value(state, ring_base + RING_CTL as u32).unwrap_or(0);
    let lri_hws = find_context_lri_value(state, ring_base + RING_HWS_PGA as u32).unwrap_or(0);
    let lri_bbaddr = find_context_lri_value(state, ring_base + RING_BBADDR as u32).unwrap_or(0);
    let lri_bbaddr_udw =
        find_context_lri_value(state, ring_base + RING_BBADDR_UDW as u32).unwrap_or(0);
    let pphwsp_gpu = context_gpu_addr & !0xFFFu64;

    crate::log!(
        "intel/media-demo: lrc-image engine={} context_gpu=0x{:X} pphwsp_gpu=0x{:X} lrc_off_dw={} ctx=0x{:08X} head=0x{:08X} tail=0x{:08X} start=0x{:08X} ctl=0x{:08X} hws=0x{:08X} bbaddr=0x{:08X} bbaddr_udw=0x{:08X}\n",
        desc.name,
        context_gpu_addr,
        pphwsp_gpu,
        MEDIA_LRC_STATE_OFFSET_DWORDS,
        lri_ctx,
        lri_head,
        lri_tail,
        lri_start,
        lri_ctl,
        lri_hws,
        lri_bbaddr,
        lri_bbaddr_udw
    );
    crate::log!(
        "intel/media-demo: lrc-dwords engine={} d0=0x{:08X} d1=0x{:08X} d2=0x{:08X} d3=0x{:08X} d4=0x{:08X} d5=0x{:08X} d6=0x{:08X} d7=0x{:08X} d8=0x{:08X} d9=0x{:08X} d10=0x{:08X} d11=0x{:08X} d12=0x{:08X} d13=0x{:08X} d14=0x{:08X} d15=0x{:08X}\n",
        desc.name,
        state.get(0).copied().unwrap_or(0),
        state.get(1).copied().unwrap_or(0),
        state.get(2).copied().unwrap_or(0),
        state.get(3).copied().unwrap_or(0),
        state.get(4).copied().unwrap_or(0),
        state.get(5).copied().unwrap_or(0),
        state.get(6).copied().unwrap_or(0),
        state.get(7).copied().unwrap_or(0),
        state.get(8).copied().unwrap_or(0),
        state.get(9).copied().unwrap_or(0),
        state.get(10).copied().unwrap_or(0),
        state.get(11).copied().unwrap_or(0),
        state.get(12).copied().unwrap_or(0),
        state.get(13).copied().unwrap_or(0),
        state.get(14).copied().unwrap_or(0),
        state.get(15).copied().unwrap_or(0)
    );
}

fn log_media_submission_words(
    desc: MediaEngineDescriptor,
    ring_virt: *mut u8,
    ring_bytes: usize,
    batch_virt: *mut u8,
    batch_bytes: usize,
) {
    if ring_virt.is_null() || batch_virt.is_null() {
        return;
    }

    let ring_dwords = unsafe {
        core::slice::from_raw_parts(
            ring_virt as *const u32,
            (ring_bytes / core::mem::size_of::<u32>()).min(4),
        )
    };
    let batch_dwords = unsafe {
        core::slice::from_raw_parts(
            batch_virt as *const u32,
            (batch_bytes / core::mem::size_of::<u32>()).min(8),
        )
    };

    crate::log!(
        "intel/media-demo: ring-dwords engine={} d0=0x{:08X} d1=0x{:08X} d2=0x{:08X} d3=0x{:08X}\n",
        desc.name,
        ring_dwords.first().copied().unwrap_or(0),
        ring_dwords.get(1).copied().unwrap_or(0),
        ring_dwords.get(2).copied().unwrap_or(0),
        ring_dwords.get(3).copied().unwrap_or(0)
    );
    crate::log!(
        "intel/media-demo: batch-dwords engine={} d0=0x{:08X} d1=0x{:08X} d2=0x{:08X} d3=0x{:08X} d4=0x{:08X} d5=0x{:08X} d6=0x{:08X} d7=0x{:08X}\n",
        desc.name,
        batch_dwords.first().copied().unwrap_or(0),
        batch_dwords.get(1).copied().unwrap_or(0),
        batch_dwords.get(2).copied().unwrap_or(0),
        batch_dwords.get(3).copied().unwrap_or(0),
        batch_dwords.get(4).copied().unwrap_or(0),
        batch_dwords.get(5).copied().unwrap_or(0),
        batch_dwords.get(6).copied().unwrap_or(0),
        batch_dwords.get(7).copied().unwrap_or(0)
    );
}

fn seed_media_ring_live_state(
    warm: Igpu770WarmState,
    desc: MediaEngineDescriptor,
    pphwsp_gpu: u32,
    ring_head: u32,
    ring_start: u32,
    ring_ctl: u32,
    ring_tail: u32,
) {
    let base = desc.ring_base;
    let mi_mode_req = masked_bits_update(0, STOP_RING);
    let ctx_ctl_req = masked_bits_update(
        CTX_CTRL_RS_CTX_ENABLE,
        CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT
            | CTX_CTRL_ENGINE_CTX_SAVE_INHIBIT
            | CTX_CTRL_INHIBIT_SYN_CTX_SWITCH,
    );

    let _ = mmio_write32(warm, base + RING_CONTEXT_CONTROL, ctx_ctl_req);
    let _ = mmio_write32(warm, base + RING_CONTEXT_CONTROL_REF, ctx_ctl_req);
    let _ = mmio_write32(warm, base + RING_MI_MODE, mi_mode_req);
    let _ = mmio_write32(warm, base + RING_HWS_PGA, pphwsp_gpu);
    let _ = mmio_write32(warm, base + RING_HWSTAM, !0u32);
    let _ = mmio_write32(warm, base + RING_HEAD, ring_head);
    let _ = mmio_write32(warm, base + RING_START, ring_start);
    let _ = mmio_write32(warm, base + RING_CTL, ring_ctl);
    let _ = mmio_write32(warm, base + RING_TAIL, ring_tail);

    crate::log!(
        "intel/media-demo: live-ring-seed engine={} ctx_ctl_req=0x{:08X} ctx_ctl_ref_req=0x{:08X} mi_mode_req=0x{:08X} hws_req=0x{:08X} hwstam_req=0x{:08X} head_req=0x{:08X} start_req=0x{:08X} ctl_req=0x{:08X} tail_req=0x{:08X} ctx_ctl_rb=0x{:08X} ctx_ctl_ref_rb=0x{:08X} mi_mode_rb=0x{:08X} hws_rb=0x{:08X} hwstam_rb=0x{:08X} head_rb=0x{:08X} start_rb=0x{:08X} ctl_rb=0x{:08X} tail_rb=0x{:08X}\n",
        desc.name,
        ctx_ctl_req,
        ctx_ctl_req,
        mi_mode_req,
        pphwsp_gpu,
        !0u32,
        ring_head,
        ring_start,
        ring_ctl,
        ring_tail,
        mmio_read32(warm, base + RING_CONTEXT_CONTROL),
        mmio_read32(warm, base + RING_CONTEXT_CONTROL_REF),
        mmio_read32(warm, base + RING_MI_MODE),
        mmio_read32(warm, base + RING_HWS_PGA),
        mmio_read32(warm, base + RING_HWSTAM),
        mmio_read32(warm, base + RING_HEAD),
        mmio_read32(warm, base + RING_START),
        mmio_read32(warm, base + RING_CTL),
        mmio_read32(warm, base + RING_TAIL)
    );
}

fn build_kickoff_state(warm: Igpu770WarmState) -> MediaKickoffState {
    let topology = current_topology();
    let guc_ready = intel_guc::ready();
    let wake = snapshot_forcewake(warm);
    let command_encoding_ready = media_command_encoding_ready(guc_ready, wake);
    let transport = preferred_transport(guc_ready);
    let mut plans = [MediaEnginePlan::empty(); MAX_MEDIA_ENGINES];
    let mut runtimes = [MediaEngineRuntimeSnapshot::empty(); MAX_MEDIA_ENGINES];

    let mut idx = 0usize;
    while idx < topology.planned_engine_count {
        let desc = topology.engines[idx];
        plans[idx] = build_engine_plan(idx, desc, guc_ready, command_encoding_ready);
        runtimes[idx] = snapshot_engine_runtime(warm, desc);
        idx += 1;
    }

    MediaKickoffState {
        topology,
        plan_count: topology.planned_engine_count,
        plans,
        runtime_count: topology.planned_engine_count,
        runtimes,
        wake,
        api: current_api_shape(transport),
        preferred_transport: transport,
        guc_ready,
        guc_status: intel_guc::status(warm),
        stage: if command_encoding_ready {
            MediaKickoffStage::CommandEncoding
        } else if guc_ready {
            MediaKickoffStage::SubmissionWiring
        } else {
            MediaKickoffStage::ResourcePlanning
        },
        decode_bitstream_demo: None,
    }
}

fn select_engine_for_workload(
    topology: MediaTopology,
    workload: MediaWorkloadKind,
) -> Option<(usize, MediaEngineDescriptor)> {
    let mut reserve = None;
    let mut idx = 0usize;
    while idx < topology.planned_engine_count {
        let desc = topology.engines[idx];
        if !desc.supports_workload(workload) {
            idx += 1;
            continue;
        }
        if desc.provisioning == MediaProvisioning::Kickoff {
            return Some((idx, desc));
        }
        if reserve.is_none() && desc.provisioning == MediaProvisioning::ScaleOutReserve {
            reserve = Some((idx, desc));
        }
        idx += 1;
    }
    reserve
}

pub(crate) fn draft_job_for_workload(workload: MediaWorkloadKind) -> Option<MediaJobDraft> {
    let state = stored_kickoff_state();
    let topology = state.map(|s| s.topology).unwrap_or_else(current_topology);
    let guc_ready = state.map(|s| s.guc_ready).unwrap_or_else(intel_guc::ready);
    let command_encoding_ready = state
        .map(|s| media_command_encoding_ready(s.guc_ready, s.wake))
        .unwrap_or(guc_ready);
    let (slot, desc) = select_engine_for_workload(topology, workload)?;
    let mut plan = build_engine_plan(slot, desc, guc_ready, command_encoding_ready);
    plan.submission.workload = workload;
    Some(MediaJobDraft {
        engine: plan.descriptor,
        resources: plan.resources,
        context: plan.context,
        batch: plan.batch,
        submission: plan.submission,
        observability: plan.observability,
        next_stage: plan.next_stage,
    })
}

fn log_job_draft(label: &str, draft: MediaJobDraft) {
    crate::log!(
        "intel/media: {} engine={} class={} provisioning={} transport={} workload={} ring=0x{:X} ctx=0x{:X} batch=0x{:X} scratch=0x{:X} bitstream=0x{:X} output=0x{:X} result=0x{:X} next_stage={} completion_addr=0x{:X}\n",
        label,
        draft.engine.name,
        draft.engine.id.class.as_str(),
        draft.engine.provisioning.as_str(),
        draft.submission.transport.as_str(),
        draft.submission.workload.as_str(),
        draft.resources.windows.ring_gpu_addr,
        draft.resources.windows.context_gpu_addr,
        draft.resources.windows.batch_gpu_addr,
        draft.resources.windows.scratch_gpu_addr,
        draft.resources.windows.bitstream_gpu_addr,
        draft.resources.windows.output_surface_gpu_addr,
        draft.resources.windows.result_gpu_addr,
        draft.next_stage.as_str(),
        draft.batch.completion_slot_gpu_addr
    );
}

fn log_kickoff_state(state: MediaKickoffState, forcewake_ack: u32) {
    crate::log!(
        "intel/media: kickoff summary sku={} active={} planned={} transport={} stage={} guc_ready={} guc_status=0x{:08X} render_forcewake_ack=0x{:08X} media_req=0x{:08X} media_ack=0x{:08X} media_domains_awake={}/{}\n",
        state.topology.sku_name,
        state.topology.active_engine_count,
        state.topology.planned_engine_count,
        state.preferred_transport.as_str(),
        state.stage.as_str(),
        state.guc_ready as u8,
        state.guc_status,
        forcewake_ack,
        state.wake.global_req,
        state.wake.global_ack,
        state.wake.awake_count,
        state.wake.slice_count
    );

    let mut route_idx = 0usize;
    while route_idx < state.api.route_count {
        let route = state.api.routes[route_idx];
        crate::log!(
            "intel/media: api route={} workload={} class={} transport={} summary={}\n",
            route.name,
            route.workload.as_str(),
            route
                .preferred_engine_class
                .map(MediaEngineClass::as_str)
                .unwrap_or("any"),
            route.transport.as_str(),
            route.summary
        );
        route_idx += 1;
    }

    let mut idx = 0usize;
    while idx < state.plan_count {
        let plan = state.plans[idx];
        let runtime = state.runtimes[idx];
        crate::log!(
            "intel/media: plan engine={} class={} provisioning={} ring_base=0x{:X} ring=0x{:X} ctx=0x{:X} batch=0x{:X} scratch=0x{:X} bitstream=0x{:X} output=0x{:X} result=0x{:X} queue_depth={} completion={} next_stage={} observed={} ctl=0x{:08X} head=0x{:08X} tail=0x{:08X} execlist_lo=0x{:08X} execlist_hi=0x{:08X}\n",
            plan.descriptor.name,
            plan.descriptor.id.class.as_str(),
            plan.descriptor.provisioning.as_str(),
            plan.descriptor.ring_base,
            plan.resources.windows.ring_gpu_addr,
            plan.resources.windows.context_gpu_addr,
            plan.resources.windows.batch_gpu_addr,
            plan.resources.windows.scratch_gpu_addr,
            plan.resources.windows.bitstream_gpu_addr,
            plan.resources.windows.output_surface_gpu_addr,
            plan.resources.windows.result_gpu_addr,
            plan.submission.queue_depth,
            plan.submission.completion.as_str(),
            plan.next_stage.as_str(),
            runtime.observed as u8,
            runtime.ctl,
            runtime.head,
            runtime.tail,
            runtime.execlist_status_lo,
            runtime.execlist_status_hi
        );
        idx += 1;
    }

    let mut slice_idx = 0usize;
    while slice_idx < state.wake.slice_count {
        let slice = state.wake.slices[slice_idx];
        crate::log!(
            "intel/media: forcewake slice={} awake={} raw=0x{:08X}\n",
            slice.name,
            slice.awake as u8,
            slice.value
        );
        slice_idx += 1;
    }
}

pub(crate) fn kickoff_once() {
    if MEDIA_KICKOFF_RAN.swap(true, Ordering::AcqRel) {
        return;
    }

    let Some(warm) = warm_state() else {
        crate::log!("intel/media: kickoff skipped reason=not-warmed\n");
        return;
    };

    let forcewake_ack = forcewake_all_acquire(warm);
    let _ = forcewake_media_refresh(warm, "media-kickoff");
    let state = build_kickoff_state(warm);
    {
        let mut slot = MEDIA_KICKOFF_STATE.lock();
        *slot = Some(state);
    }

    log_kickoff_state(state, forcewake_ack);

    if let Some(draft) = draft_job_for_workload(MediaWorkloadKind::DecodeFrame) {
        log_job_draft("draft-decode", draft);
    }
    if let Some(draft) = draft_job_for_workload(MediaWorkloadKind::EnhanceFrame) {
        log_job_draft("draft-enhance", draft);
    }
}

fn find_fourcc(bytes: &[u8], tag: &[u8; 4]) -> Option<usize> {
    bytes.windows(4).position(|window| window == tag)
}

fn log_nal_summary(nal_types: &[u8]) {
    let mut idx = 0usize;
    while idx < nal_types.len() {
        crate::log!("intel/media-demo: first-sample nal[{}]=0x{:02X}\n", idx, nal_types[idx]);
        idx += 1;
    }
}

#[inline]
fn masked_bit_enable(bit: u32) -> u32 {
    bit | (bit << 16)
}

#[inline]
fn masked_bits_update(set_bits: u32, clear_bits: u32) -> u32 {
    set_bits | ((set_bits | clear_bits) << 16)
}

#[inline]
fn read_result_dword(base_virt: *mut u8, slot_off: u64) -> u32 {
    let ptr = (base_virt as usize).saturating_add(slot_off as usize) as *const u32;
    unsafe { core::ptr::read_volatile(ptr) }
}

fn sample_surface_signature(bytes: &[u8]) -> (u32, usize) {
    if bytes.is_empty() {
        return (0, 0);
    }

    let sample_count = bytes.len().min(4096);
    let step = (bytes.len() / sample_count.max(1)).max(1);
    let mut idx = 0usize;
    let mut seen = 0usize;
    let mut nonzero = 0usize;
    let mut sig = 0x4D44_5641u32;

    while idx < bytes.len() && seen < sample_count {
        let byte = bytes[idx];
        if byte != 0 {
            nonzero += 1;
        }
        sig = sig.rotate_left(5) ^ (byte as u32) ^ (seen as u32).wrapping_mul(0x45D9_F3B);
        idx = idx.saturating_add(step);
        seen += 1;
    }

    (sig, nonzero)
}

fn progressive_present_output_surface(
    label: &str,
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
        && (submit_completed || nonzero_samples != 0)
    {
        cpu_framebuffer_visualize_nv12_center(
            label,
            output_surface,
            frame_width as usize,
            frame_height as usize,
            output_pitch,
        );
        return (true, signature, nonzero_samples);
    }

    if nonzero_samples != 0 {
        let preview_len = output_surface.len().min(32 * 1024);
        cpu_framebuffer_visualize_bytes_center(
            label,
            &output_surface[..preview_len],
            MEDIA_HTTPS_DEMO_VIS_W.saturating_mul(2),
            MEDIA_HTTPS_DEMO_VIS_H.saturating_mul(2),
        );
        return (true, signature, nonzero_samples);
    }

    (false, signature, nonzero_samples)
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
fn stage_flags(has_idr: bool, batch_mode: MediaVcsBatchMode) -> u32 {
    let mut flags = (has_idr as u32) | (1 << 1);
    if batch_mode == MediaVcsBatchMode::MiOnlyProbe {
        flags |= MEDIA_VCS_BATCH_MODE_MI_ONLY_PROBE;
    }
    flags
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

#[inline]
fn execlist_desc_gpu_addr(desc_lo: u32, desc_hi: u32) -> u64 {
    let hi_addr_mask = (1u64 << (SW_CTX_ID_SHIFT - 32)) - 1;
    (((desc_hi as u64) & hi_addr_mask) << 32) | ((desc_lo & 0xFFFF_F000) as u64)
}

fn log_context_descriptor_audit(
    desc: MediaEngineDescriptor,
    phase: &str,
    desc_lo: u32,
    desc_hi: u32,
    expected_gpu_addr: u64,
    expected_sw_ctx_id: u32,
) {
    let decoded_gpu_addr = execlist_desc_gpu_addr(desc_lo, desc_hi);
    let addr_mode = (desc_lo >> CTX_DESC_ADDRESSING_MODE_SHIFT) & CTX_DESC_ADDRESSING_MODE_MASK;
    let sw_ctx_id = desc_hi >> (SW_CTX_ID_SHIFT - 32);
    crate::log!(
        "intel/media-demo: execlist-desc phase={} engine={} lo=0x{:08X} hi=0x{:08X} gpu=0x{:X} gpu_expect=0x{:X} gpu_match={} valid={} force_restore={} privilege={} priority_normal={} addr_mode={} sw_ctx_id={} sw_ctx_expect={} sw_ctx_match={}\n",
        phase,
        desc.name,
        desc_lo,
        desc_hi,
        decoded_gpu_addr,
        expected_gpu_addr,
        (decoded_gpu_addr == expected_gpu_addr) as u8,
        ((desc_lo & CTX_DESC_VALID) != 0) as u8,
        ((desc_lo & CTX_DESC_FORCE_RESTORE) != 0) as u8,
        ((desc_lo & CTX_DESC_PRIVILEGE) != 0) as u8,
        ((desc_lo & CTX_DESC_PRIORITY_NORMAL) != 0) as u8,
        addr_mode,
        sw_ctx_id,
        expected_sw_ctx_id,
        (sw_ctx_id == expected_sw_ctx_id) as u8
    );
}

fn store_decode_bitstream_demo_state(next: MediaBitstreamDemoState) {
    let mut slot = MEDIA_KICKOFF_STATE.lock();
    if let Some(mut state) = *slot {
        state.decode_bitstream_demo = Some(next);
        *slot = Some(state);
    }
}

fn ensure_demo_backing(resources: MediaResourcePlan) -> Option<&'static mut MediaBitstreamBacking> {
    let mut slot = MEDIA_BITSTREAM_BACKING.lock();
    if slot.is_none() {
        let (ring_phys, ring_virt) = crate::dma::alloc(resources.ring_bytes, 4096)?;
        let (context_phys, context_virt) = crate::dma::alloc(resources.context_bytes, 4096)?;
        let (batch_phys, batch_virt) = crate::dma::alloc(resources.batch_bytes, 4096)?;
        let (result_phys, result_virt) = crate::dma::alloc(resources.result_bytes, 4096)?;
        let (bitstream_phys, bitstream_virt) = crate::dma::alloc(resources.bitstream_bytes, 4096)?;
        let (output_surface_phys, output_surface_virt) =
            crate::dma::alloc(resources.output_surface_bytes, 4096)?;
        *slot = Some(MediaBitstreamBacking {
            ring_phys,
            ring_virt,
            ring_bytes: resources.ring_bytes,
            context_phys,
            context_virt,
            context_bytes: resources.context_bytes,
            batch_phys,
            batch_virt,
            batch_bytes: resources.batch_bytes,
            result_phys,
            result_virt,
            result_bytes: resources.result_bytes,
            bitstream_phys,
            bitstream_virt,
            bitstream_bytes: resources.bitstream_bytes,
            output_surface_phys,
            output_surface_virt,
            output_surface_bytes: resources.output_surface_bytes,
        });
    }

    let backing = slot.as_mut()?;
    Some(unsafe { &mut *(backing as *mut MediaBitstreamBacking) })
}

fn build_h264_decode_batch_skeleton(
    batch_virt: *mut u8,
    batch_bytes: usize,
    draft: MediaJobDraft,
    frame_width: u16,
    frame_height: u16,
    annexb_bytes: usize,
    sample_nal_count: usize,
    has_idr: bool,
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
    let chroma_y_offset = output_pitch.saturating_mul(height);

    if !emit_store_dword(
        batch,
        &mut idx,
        draft.observability.result_slots[0].gpu_addr,
        draft.observability.result_slots[0].expected_marker,
    ) || !emit_store_dword(
        batch,
        &mut idx,
        draft.resources.windows.result_gpu_addr + MEDIA_RESULT_BITSTREAM_ADDR_LO_SLOT,
        draft.resources.windows.bitstream_gpu_addr as u32,
    ) || !emit_store_dword(
        batch,
        &mut idx,
        draft.resources.windows.result_gpu_addr + MEDIA_RESULT_BITSTREAM_ADDR_HI_SLOT,
        (draft.resources.windows.bitstream_gpu_addr >> 32) as u32,
    ) || !emit_store_dword(
        batch,
        &mut idx,
        draft.resources.windows.result_gpu_addr + MEDIA_RESULT_BITSTREAM_BYTES_SLOT,
        annexb_bytes as u32,
    ) || !emit_store_dword(
        batch,
        &mut idx,
        draft.resources.windows.result_gpu_addr + MEDIA_RESULT_SAMPLE_NALS_SLOT,
        sample_nal_count as u32,
    ) || !emit_store_dword(
        batch,
        &mut idx,
        draft.resources.windows.result_gpu_addr + MEDIA_RESULT_STAGE_FLAGS_SLOT,
        stage_flags(has_idr, MediaVcsBatchMode::H264Decode),
    ) || !emit_store_dword(
        batch,
        &mut idx,
        draft.resources.windows.result_gpu_addr + MEDIA_RESULT_OUTPUT_SURFACE_ADDR_LO_SLOT,
        draft.resources.windows.output_surface_gpu_addr as u32,
    ) || !emit_store_dword(
        batch,
        &mut idx,
        draft.resources.windows.result_gpu_addr + MEDIA_RESULT_OUTPUT_SURFACE_ADDR_HI_SLOT,
        (draft.resources.windows.output_surface_gpu_addr >> 32) as u32,
    ) || !emit_store_dword(
        batch,
        &mut idx,
        draft.resources.windows.result_gpu_addr + MEDIA_RESULT_OUTPUT_SURFACE_BYTES_SLOT,
        draft.resources.output_surface_bytes as u32,
    ) || !emit_store_dword(
        batch,
        &mut idx,
        draft.resources.windows.result_gpu_addr + MEDIA_RESULT_FRAME_DIMS_SLOT,
        frame_dims,
    ) || !emit_store_dword(
        batch,
        &mut idx,
        draft.observability.result_slots[1].gpu_addr,
        draft.observability.result_slots[1].expected_marker,
    ) || !emit_store_dword(
        batch,
        &mut idx,
        draft.observability.result_slots[2].gpu_addr,
        draft.observability.result_slots[2].expected_marker,
    ) || !emit_store_dword(
        batch,
        &mut idx,
        draft.observability.result_slots[3].gpu_addr,
        draft.observability.result_slots[3].expected_marker,
    ) {
        return None;
    }

    let flush = begin_batch_packet(
        batch,
        &mut idx,
        5,
        MI_FLUSH_DW | MI_FLUSH_DW_VIDEO_PIPELINE_CACHE_INVALIDATE | MI_FLUSH_DW_USE_GTT,
    )?;
    batch[flush + 1] = draft.observability.result_slots[2].gpu_addr as u32;
    batch[flush + 2] = (draft.observability.result_slots[2].gpu_addr >> 32) as u32;
    batch[flush + 3] = draft.observability.result_slots[2].expected_marker;

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
    batch[pipe_mode + 1] = 2 | (1 << 9) | (1 << 11);

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
    packet_write_addr64(batch, pipe_buf, 4, draft.resources.windows.output_surface_gpu_addr);
    packet_write_addr64(batch, pipe_buf, 13, draft.resources.windows.scratch_gpu_addr);
    packet_write_addr64(batch, pipe_buf, 16, draft.resources.windows.scratch_gpu_addr + 0x4000);
    packet_write_addr64(batch, pipe_buf, 52, draft.resources.windows.result_gpu_addr + 0x100);
    packet_write_addr64(batch, pipe_buf, 55, draft.resources.windows.result_gpu_addr + 0x200);
    packet_write_addr64(batch, pipe_buf, 58, draft.resources.windows.result_gpu_addr + 0x300);

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
    packet_write_addr64(batch, ind_obj, 1, draft.resources.windows.bitstream_gpu_addr);
    packet_write_addr64(
        batch,
        ind_obj,
        4,
        draft.resources.windows.bitstream_gpu_addr + annexb_bytes as u64,
    );

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
    packet_write_addr64(batch, bsp, 1, draft.resources.windows.scratch_gpu_addr + 0x8000);
    packet_write_addr64(batch, bsp, 4, draft.resources.windows.scratch_gpu_addr + 0xC000);

    let avc_img = begin_batch_packet(
        batch,
        &mut idx,
        (MFX_CMD_LEN_AVC_IMG_STATE + 2) as usize,
        media_cmd_header(MEDIA_CMD_OPCODE_MFX_AVC, 0, MFX_AVC_IMG_STATE, MFX_CMD_LEN_AVC_IMG_STATE),
    )?;
    batch[avc_img + 1] = ((width_mbs.saturating_mul(height_mbs)) & 0xFFFF) as u32;
    batch[avc_img + 2] =
        (width_mbs.saturating_sub(1) & 0xFF) | ((height_mbs.saturating_sub(1) & 0xFF) << 16);
    batch[avc_img + 3] = 0;

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
    batch[avc_bsd + 1] = annexb_bytes as u32;
    batch[avc_bsd + 2] = 0;
    batch[avc_bsd + 4] = 1 << 3;
    batch[avc_bsd + 5] = 1 | (1 << 1) | (1 << 15);

    if idx.saturating_add(2) > batch.len() {
        return None;
    }
    batch[idx] = MI_ARB_CHECK;
    batch[idx + 1] = MI_BATCH_BUFFER_END;
    Some((idx + 2).saturating_mul(core::mem::size_of::<u32>()))
}

fn build_mi_only_probe_batch(
    batch_virt: *mut u8,
    batch_bytes: usize,
    draft: MediaJobDraft,
    frame_width: u16,
    frame_height: u16,
    annexb_bytes: usize,
    sample_nal_count: usize,
    has_idr: bool,
) -> Option<usize> {
    let batch = unsafe {
        core::slice::from_raw_parts_mut(
            batch_virt as *mut u32,
            batch_bytes / core::mem::size_of::<u32>(),
        )
    };
    let mut idx = 0usize;
    let frame_dims = (frame_width as u32) | ((frame_height as u32) << 16);

    let mut noop_idx = 0usize;
    while noop_idx < MEDIA_MI_ONLY_PROBE_PREFIX_NOOPS {
        if idx >= batch.len() {
            return None;
        }
        batch[idx] = MI_NOOP;
        idx += 1;
        noop_idx += 1;
    }

    if !emit_store_dword(
        batch,
        &mut idx,
        draft.observability.result_slots[0].gpu_addr,
        draft.observability.result_slots[0].expected_marker,
    ) || !emit_store_dword(
        batch,
        &mut idx,
        draft.resources.windows.result_gpu_addr + MEDIA_RESULT_BITSTREAM_ADDR_LO_SLOT,
        draft.resources.windows.bitstream_gpu_addr as u32,
    ) || !emit_store_dword(
        batch,
        &mut idx,
        draft.resources.windows.result_gpu_addr + MEDIA_RESULT_BITSTREAM_ADDR_HI_SLOT,
        (draft.resources.windows.bitstream_gpu_addr >> 32) as u32,
    ) || !emit_store_dword(
        batch,
        &mut idx,
        draft.resources.windows.result_gpu_addr + MEDIA_RESULT_BITSTREAM_BYTES_SLOT,
        annexb_bytes as u32,
    ) || !emit_store_dword(
        batch,
        &mut idx,
        draft.resources.windows.result_gpu_addr + MEDIA_RESULT_SAMPLE_NALS_SLOT,
        sample_nal_count as u32,
    ) || !emit_store_dword(
        batch,
        &mut idx,
        draft.resources.windows.result_gpu_addr + MEDIA_RESULT_STAGE_FLAGS_SLOT,
        stage_flags(has_idr, MediaVcsBatchMode::MiOnlyProbe),
    ) || !emit_store_dword(
        batch,
        &mut idx,
        draft.resources.windows.result_gpu_addr + MEDIA_RESULT_OUTPUT_SURFACE_ADDR_LO_SLOT,
        draft.resources.windows.output_surface_gpu_addr as u32,
    ) || !emit_store_dword(
        batch,
        &mut idx,
        draft.resources.windows.result_gpu_addr + MEDIA_RESULT_OUTPUT_SURFACE_ADDR_HI_SLOT,
        (draft.resources.windows.output_surface_gpu_addr >> 32) as u32,
    ) || !emit_store_dword(
        batch,
        &mut idx,
        draft.resources.windows.result_gpu_addr + MEDIA_RESULT_OUTPUT_SURFACE_BYTES_SLOT,
        draft.resources.output_surface_bytes as u32,
    ) || !emit_store_dword(
        batch,
        &mut idx,
        draft.resources.windows.result_gpu_addr + MEDIA_RESULT_FRAME_DIMS_SLOT,
        frame_dims,
    ) || !emit_store_dword(
        batch,
        &mut idx,
        draft.observability.result_slots[1].gpu_addr,
        draft.observability.result_slots[1].expected_marker,
    ) {
        return None;
    }

    let flush = begin_batch_packet(
        batch,
        &mut idx,
        5,
        MI_FLUSH_DW | MI_FLUSH_DW_VIDEO_PIPELINE_CACHE_INVALIDATE | MI_FLUSH_DW_USE_GTT,
    )?;
    batch[flush + 1] = draft.observability.result_slots[2].gpu_addr as u32;
    batch[flush + 2] = (draft.observability.result_slots[2].gpu_addr >> 32) as u32;
    batch[flush + 3] = draft.observability.result_slots[2].expected_marker;

    if !emit_store_dword(
        batch,
        &mut idx,
        draft.observability.result_slots[3].gpu_addr,
        draft.observability.result_slots[3].expected_marker,
    ) {
        return None;
    }

    if idx.saturating_add(2) > batch.len() {
        return None;
    }
    batch[idx] = MI_ARB_CHECK;
    batch[idx + 1] = MI_BATCH_BUFFER_END;
    Some((idx + 2).saturating_mul(core::mem::size_of::<u32>()))
}

fn build_vcs_demo_batch(
    batch_virt: *mut u8,
    batch_bytes: usize,
    draft: MediaJobDraft,
    frame_width: u16,
    frame_height: u16,
    annexb_bytes: usize,
    sample_nal_count: usize,
    has_idr: bool,
) -> Option<usize> {
    match ACTIVE_MEDIA_VCS_BATCH_MODE {
        MediaVcsBatchMode::MiOnlyProbe => build_mi_only_probe_batch(
            batch_virt,
            batch_bytes,
            draft,
            frame_width,
            frame_height,
            annexb_bytes,
            sample_nal_count,
            has_idr,
        ),
        MediaVcsBatchMode::H264Decode => build_h264_decode_batch_skeleton(
            batch_virt,
            batch_bytes,
            draft,
            frame_width,
            frame_height,
            annexb_bytes,
            sample_nal_count,
            has_idr,
        ),
    }
}

fn build_vcs_demo_ring(
    ring_virt: *mut u8,
    ring_bytes: usize,
    draft: MediaJobDraft,
    frame_width: u16,
    frame_height: u16,
    annexb_bytes: usize,
    sample_nal_count: usize,
    has_idr: bool,
) -> Option<usize> {
    match ACTIVE_MEDIA_VCS_SUBMIT_MODE {
        MediaVcsSubmitMode::RingInline => build_mi_only_probe_batch(
            ring_virt,
            ring_bytes,
            draft,
            frame_width,
            frame_height,
            annexb_bytes,
            sample_nal_count,
            has_idr,
        ),
        MediaVcsSubmitMode::BatchStart => build_ring_batch_start_words(
            ring_virt,
            ring_bytes,
            draft.resources.windows.batch_gpu_addr,
        ),
    }
}

fn prepare_decode_bitstream_demo(
    warm: Igpu770WarmState,
    draft: MediaJobDraft,
    frame_width: u16,
    frame_height: u16,
    annexb: &[u8],
    sample_nal_count: usize,
    has_idr: bool,
) -> Option<MediaBitstreamDemoState> {
    if annexb.len() > draft.resources.bitstream_bytes {
        crate::log!(
            "intel/media-demo: bitstream staging skipped reason=window-too-small bytes={} window={}\n",
            annexb.len(),
            draft.resources.bitstream_bytes
        );
        return None;
    }

    let backing = ensure_demo_backing(draft.resources)?;
    if !ggtt_map_system_ram_range(
        "media-ring",
        warm,
        backing.ring_phys,
        backing.ring_bytes,
        draft.resources.windows.ring_gpu_addr,
    ) || !ggtt_map_system_ram_range(
        "media-context",
        warm,
        backing.context_phys,
        backing.context_bytes,
        draft.resources.windows.context_gpu_addr,
    ) || !ggtt_map_system_ram_range(
        "media-batch",
        warm,
        backing.batch_phys,
        backing.batch_bytes,
        draft.resources.windows.batch_gpu_addr,
    ) || !ggtt_map_system_ram_range(
        "media-result",
        warm,
        backing.result_phys,
        backing.result_bytes,
        draft.resources.windows.result_gpu_addr,
    ) || !ggtt_map_system_ram_range(
        "media-bitstream",
        warm,
        backing.bitstream_phys,
        backing.bitstream_bytes,
        draft.resources.windows.bitstream_gpu_addr,
    ) || !ggtt_map_system_ram_range(
        "media-output-surface",
        warm,
        backing.output_surface_phys,
        backing.output_surface_bytes,
        draft.resources.windows.output_surface_gpu_addr,
    ) {
        crate::log!("intel/media-demo: bitstream staging skipped reason=ggtt-map\n");
        return None;
    }

    unsafe {
        ptr::write_bytes(backing.ring_virt, 0, backing.ring_bytes);
        ptr::write_bytes(backing.context_virt, 0, backing.context_bytes);
        ptr::write_bytes(backing.batch_virt, 0, backing.batch_bytes);
        ptr::write_bytes(backing.result_virt, 0, backing.result_bytes);
        ptr::write_bytes(backing.bitstream_virt, 0, backing.bitstream_bytes);
        ptr::write_bytes(backing.output_surface_virt, 0, backing.output_surface_bytes);
        ptr::copy_nonoverlapping(annexb.as_ptr(), backing.bitstream_virt, annexb.len());
    }
    dma_cache_flush_range(backing.bitstream_virt as *const u8, annexb.len());
    dma_cache_flush_range(backing.result_virt as *const u8, backing.result_bytes);
    dma_cache_flush_range(backing.output_surface_virt as *const u8, backing.output_surface_bytes);

    let batch_tail_bytes = if ACTIVE_MEDIA_VCS_SUBMIT_MODE == MediaVcsSubmitMode::BatchStart {
        let tail = build_vcs_demo_batch(
            backing.batch_virt,
            backing.batch_bytes,
            draft,
            frame_width,
            frame_height,
            annexb.len(),
            sample_nal_count,
            has_idr,
        )?;
        dma_cache_flush_range(backing.batch_virt as *const u8, tail);
        tail
    } else {
        0
    };

    let ring_tail_bytes = build_vcs_demo_ring(
        backing.ring_virt,
        backing.ring_bytes,
        draft,
        frame_width,
        frame_height,
        annexb.len(),
        sample_nal_count,
        has_idr,
    )?;
    dma_cache_flush_range(backing.ring_virt as *const u8, ring_tail_bytes);
    let ring_ctl = ring_ctl_value_for_size(backing.ring_bytes)?;
    let ring_start = draft.resources.windows.ring_gpu_addr as u32;
    let pphwsp_gpu = (draft.resources.windows.context_gpu_addr & !0xFFF) as u32;
    if !init_gen12_video_context_image(
        backing.context_virt,
        backing.context_bytes,
        draft.engine.ring_base,
        ring_start,
        ring_tail_bytes as u32,
        ring_ctl,
        pphwsp_gpu,
    ) {
        crate::log!("intel/media-demo: bitstream staging skipped reason=context-init\n");
        return None;
    }
    log_video_context_image(
        draft.engine,
        backing.context_virt,
        backing.context_bytes,
        draft.resources.windows.context_gpu_addr,
    );

    let (ctx_desc_lo, ctx_desc_hi_base) =
        build_execlist_context_descriptor_for_gpu_addr(draft.resources.windows.context_gpu_addr);
    let expected_sw_ctx_id = media_sw_ctx_id(draft.engine);
    let (ctx_desc_lo, ctx_desc_hi) =
        with_sw_context_id(ctx_desc_lo, ctx_desc_hi_base, expected_sw_ctx_id);
    crate::log!(
        "intel/media-demo: bitstream-plan engine={} batch_mode={} submit_mode={} ring=0x{:X} ctx=0x{:X} batch=0x{:X} bitstream=0x{:X}/phys=0x{:X} bytes={} output=0x{:X}/phys=0x{:X}/bytes={} result=0x{:X} kickoff=0x{:08X} complete=0x{:08X}\n",
        draft.engine.name,
        ACTIVE_MEDIA_VCS_BATCH_MODE.as_str(),
        ACTIVE_MEDIA_VCS_SUBMIT_MODE.as_str(),
        draft.resources.windows.ring_gpu_addr,
        draft.resources.windows.context_gpu_addr,
        draft.resources.windows.batch_gpu_addr,
        draft.resources.windows.bitstream_gpu_addr,
        backing.bitstream_phys,
        annexb.len(),
        draft.resources.windows.output_surface_gpu_addr,
        backing.output_surface_phys,
        backing.output_surface_bytes,
        draft.resources.windows.result_gpu_addr,
        draft.observability.result_slots[0].expected_marker,
        draft.observability.result_slots[3].expected_marker
    );
    log_context_descriptor_audit(
        draft.engine,
        "request",
        ctx_desc_lo,
        ctx_desc_hi,
        draft.resources.windows.context_gpu_addr,
        expected_sw_ctx_id,
    );
    crate::log!(
        "intel/media-demo: vcs-submit prep batch_mode={} submit_mode={} ring_start=0x{:08X} ring_ctl=0x{:08X} ring_tail=0x{:X} batch_tail=0x{:X} batch_gpu=0x{:X} context_gpu=0x{:X} ctx_desc_lo=0x{:08X} ctx_desc_hi=0x{:08X} ctx_id={}\n",
        ACTIVE_MEDIA_VCS_BATCH_MODE.as_str(),
        ACTIVE_MEDIA_VCS_SUBMIT_MODE.as_str(),
        ring_start,
        ring_ctl,
        ring_tail_bytes,
        batch_tail_bytes,
        draft.resources.windows.batch_gpu_addr,
        draft.resources.windows.context_gpu_addr,
        ctx_desc_lo,
        ctx_desc_hi,
        media_sw_ctx_id(draft.engine)
    );
    log_media_submission_words(
        draft.engine,
        backing.ring_virt,
        backing.ring_bytes,
        backing.batch_virt,
        backing.batch_bytes,
    );

    let _ = forcewake_all_acquire(warm);
    let _ = wake_media_engine_forcewake(warm, draft.engine, "media-bitstream-submit");
    let _ = forcewake_media_refresh(warm, "media-bitstream-submit");
    let _ = wake_media_ring_for_submit(warm, draft.engine, "media-bitstream-submit");
    let _ = mmio_write32(
        warm,
        draft.engine.ring_base + RING_MODE_GEN7,
        masked_bit_enable(GEN11_GFX_DISABLE_LEGACY_MODE),
    );
    crate::log!(
        "intel/media-demo: vcs-submit lrc-seed engine={} pphwsp_gpu=0x{:08X} direct_ring_state_writes=1\n",
        draft.engine.name,
        pphwsp_gpu
    );
    let ctx_ctl_lrc_seed = gen12_lrc_context_control_seed();
    crate::log!(
        "intel/media-demo: vcs-submit ctx-ctl engine={} lrc_seed=0x{:08X} live_ctx_mmio=1\n",
        draft.engine.name,
        ctx_ctl_lrc_seed
    );
    seed_media_ring_live_state(
        warm,
        draft.engine,
        pphwsp_gpu,
        0,
        ring_start,
        ring_ctl,
        ring_tail_bytes as u32,
    );
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    execlist_submit_port_push(warm, draft.engine, ctx_desc_lo, ctx_desc_hi, 0, 0);
    let _ = mmio_write32(warm, draft.engine.ring_base + RING_EXECLIST_CONTROL, EL_CTRL_LOAD);
    let sq_lo_rb = mmio_read32(warm, draft.engine.ring_base + RING_EXECLIST_SQ_LO);
    let sq_hi_rb = mmio_read32(warm, draft.engine.ring_base + RING_EXECLIST_SQ_HI);
    let mode_rb = mmio_read32(warm, draft.engine.ring_base + RING_MODE_GEN7);
    let ctx_ctl_rb = mmio_read32(warm, draft.engine.ring_base + RING_CONTEXT_CONTROL);
    let ctx_ctl_ref_rb = mmio_read32(warm, draft.engine.ring_base + RING_CONTEXT_CONTROL_REF);
    let el_ctl_rb = mmio_read32(warm, draft.engine.ring_base + RING_EXECLIST_CONTROL);
    let el_status_lo_rb = mmio_read32(warm, draft.engine.ring_base + RING_EXECLIST_STATUS_LO);
    let el_status_hi_rb = mmio_read32(warm, draft.engine.ring_base + RING_EXECLIST_STATUS_HI);
    log_context_descriptor_audit(
        draft.engine,
        "sq-readback",
        sq_lo_rb,
        sq_hi_rb,
        draft.resources.windows.context_gpu_addr,
        expected_sw_ctx_id,
    );
    crate::log!(
        "intel/media-demo: execlist-submit context engine={} batch_mode={} submit_mode={} sq_lo_req=0x{:08X} sq_hi_req=0x{:08X} sq_lo_rb=0x{:08X} sq_hi_rb=0x{:08X} mode_rb=0x{:08X} ctx_ctl_rb=0x{:08X} ctx_ctl_ref_rb=0x{:08X} el_ctl_rb=0x{:08X} el_status_lo_rb=0x{:08X} el_status_hi_rb=0x{:08X}\n",
        draft.engine.name,
        ACTIVE_MEDIA_VCS_BATCH_MODE.as_str(),
        ACTIVE_MEDIA_VCS_SUBMIT_MODE.as_str(),
        ctx_desc_lo,
        ctx_desc_hi,
        sq_lo_rb,
        sq_hi_rb,
        mode_rb,
        ctx_ctl_rb,
        ctx_ctl_ref_rb,
        el_ctl_rb,
        el_status_lo_rb,
        el_status_hi_rb
    );
    log_media_submit_diag(warm, draft.engine, "vcs-submit", 0, 0);

    let mut completed = false;
    let mut iter = 0usize;
    while iter < draft.submission.watchdog_iters {
        let kickoff = read_result_dword(backing.result_virt, MEDIA_RESULT_KICKOFF_SLOT);
        let complete = read_result_dword(backing.result_virt, MEDIA_RESULT_COMPLETE_SLOT);
        if iter == 0 || (iter % MEDIA_SUBMIT_POLL_LOG_STEP) == 0 {
            crate::log!(
                "intel/media-demo: vcs-poll iter={} head=0x{:08X} tail=0x{:08X} acthd=0x{:08X} bbaddr=0x{:08X} execlist_lo=0x{:08X} kickoff=0x{:08X} complete=0x{:08X}\n",
                iter,
                mmio_read32(warm, draft.engine.ring_base + RING_HEAD),
                mmio_read32(warm, draft.engine.ring_base + RING_TAIL),
                mmio_read32(warm, draft.engine.ring_base + RING_ACTHD),
                mmio_read32(warm, draft.engine.ring_base + RING_BBADDR),
                mmio_read32(warm, draft.engine.ring_base + RING_EXECLIST_STATUS_LO),
                kickoff,
                complete
            );
            if iter == 0 {
                log_media_submit_diag(warm, draft.engine, "vcs-poll", kickoff, complete);
            }
        }
        if complete == draft.observability.result_slots[3].expected_marker {
            completed = true;
            break;
        }
        core::hint::spin_loop();
        iter += 1;
    }

    let kickoff = read_result_dword(backing.result_virt, MEDIA_RESULT_KICKOFF_SLOT);
    let pre = read_result_dword(backing.result_virt, MEDIA_RESULT_PRESUBMIT_SLOT);
    let post = read_result_dword(backing.result_virt, MEDIA_RESULT_POSTSUBMIT_SLOT);
    let complete = read_result_dword(backing.result_virt, MEDIA_RESULT_COMPLETE_SLOT);
    let bitstream_lo = read_result_dword(backing.result_virt, MEDIA_RESULT_BITSTREAM_ADDR_LO_SLOT);
    let bitstream_hi = read_result_dword(backing.result_virt, MEDIA_RESULT_BITSTREAM_ADDR_HI_SLOT);
    let bytes = read_result_dword(backing.result_virt, MEDIA_RESULT_BITSTREAM_BYTES_SLOT);
    let nals = read_result_dword(backing.result_virt, MEDIA_RESULT_SAMPLE_NALS_SLOT);
    let flags = read_result_dword(backing.result_virt, MEDIA_RESULT_STAGE_FLAGS_SLOT);
    let output_lo =
        read_result_dword(backing.result_virt, MEDIA_RESULT_OUTPUT_SURFACE_ADDR_LO_SLOT);
    let output_hi =
        read_result_dword(backing.result_virt, MEDIA_RESULT_OUTPUT_SURFACE_ADDR_HI_SLOT);
    let output_bytes =
        read_result_dword(backing.result_virt, MEDIA_RESULT_OUTPUT_SURFACE_BYTES_SLOT);
    let frame_dims = read_result_dword(backing.result_virt, MEDIA_RESULT_FRAME_DIMS_SLOT);
    let output_surface = unsafe {
        core::slice::from_raw_parts(
            backing.output_surface_virt as *const u8,
            backing.output_surface_bytes,
        )
    };
    let output_pitch = align_up_u32(frame_width as u32, 64) as usize;
    let (present_ready, output_surface_signature, output_surface_nonzero_samples) =
        progressive_present_output_surface(
            "media-demo-output-surface",
            output_surface,
            frame_width,
            frame_height,
            output_pitch,
            completed,
        );
    if !completed {
        log_media_submit_diag(warm, draft.engine, "vcs-timeout", kickoff, complete);
    }
    crate::log!(
        "intel/media-demo: vcs-submit result completed={} iters={} kickoff=0x{:08X} pre=0x{:08X} post=0x{:08X} done=0x{:08X} bitstream_lo=0x{:08X} bitstream_hi=0x{:08X} bytes={} nals={} flags=0x{:08X} output_lo=0x{:08X} output_hi=0x{:08X} output_bytes={} frame_dims=0x{:08X} surface_sig=0x{:08X} surface_nonzero_samples={} present_ready={}\n",
        completed as u8,
        iter,
        kickoff,
        pre,
        post,
        complete,
        bitstream_lo,
        bitstream_hi,
        bytes,
        nals,
        flags,
        output_lo,
        output_hi,
        output_bytes,
        frame_dims,
        output_surface_signature,
        output_surface_nonzero_samples,
        present_ready as u8
    );

    Some(MediaBitstreamDemoState {
        ready: true,
        engine_name: draft.engine.name,
        ring_gpu_addr: draft.resources.windows.ring_gpu_addr,
        context_gpu_addr: draft.resources.windows.context_gpu_addr,
        batch_gpu_addr: draft.resources.windows.batch_gpu_addr,
        result_gpu_addr: draft.resources.windows.result_gpu_addr,
        bitstream_gpu_addr: draft.resources.windows.bitstream_gpu_addr,
        output_surface_gpu_addr: draft.resources.windows.output_surface_gpu_addr,
        bitstream_phys: backing.bitstream_phys,
        output_surface_phys: backing.output_surface_phys,
        bitstream_bytes: annexb.len(),
        output_surface_bytes: backing.output_surface_bytes,
        frame_width,
        frame_height,
        output_surface_pitch: output_pitch,
        sample_nal_count,
        has_idr,
        kickoff_marker: draft.observability.result_slots[0].expected_marker,
        complete_marker: draft.observability.result_slots[3].expected_marker,
        output_surface_signature,
        output_surface_nonzero_samples,
        submit_completed: completed,
        present_attempted: true,
        present_ready,
        submit_iters: iter,
    })
}

pub(crate) async fn run_https_media_demo_once_async() {
    if MEDIA_HTTPS_DEMO_RAN.swap(true, Ordering::AcqRel) {
        return;
    }

    crate::log!(
        "intel/media-demo: begin url={} timeout_ms={} max_bytes={}\n",
        MEDIA_HTTPS_DEMO_URL,
        MEDIA_HTTPS_DEMO_TIMEOUT_MS,
        MEDIA_HTTPS_DEMO_MAX_BYTES
    );

    let body = match crate::r::net::https::fetch_https_body_async(
        MEDIA_HTTPS_DEMO_URL,
        MEDIA_HTTPS_DEMO_TIMEOUT_MS,
        MEDIA_HTTPS_DEMO_MAX_BYTES,
    )
    .await
    {
        Ok(body) => body,
        Err(err) => {
            let msg = format!("media-demo-fetch-error:{:?}", err);
            crate::log!("intel/media-demo: fetch failed err={:?}\n", err);
            cpu_framebuffer_media_status_card_center(
                "media-demo-fetch-error",
                MEDIA_HTTPS_DEMO_VIS_W,
                MEDIA_HTTPS_DEMO_VIS_H,
                0x00D8_4A3A,
                msg.len(),
                MEDIA_HTTPS_DEMO_TIMEOUT_MS as usize,
                false,
            );
            return;
        }
    };

    let has_ftyp = find_fourcc(&body, b"ftyp");
    let has_moov = find_fourcc(&body, b"moov");
    let has_mdat = find_fourcc(&body, b"mdat");
    let first_box_size = body
        .get(0..4)
        .map(|bytes| u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        .unwrap_or(0);

    crate::log!(
        "intel/media-demo: fetched bytes={} mp4_ftyp={} mp4_moov={} mp4_mdat={} first_box_size=0x{:08X}\n",
        body.len(),
        has_ftyp.is_some() as u8,
        has_moov.is_some() as u8,
        has_mdat.is_some() as u8,
        first_box_size
    );
    crate::log!(
        "intel/media-demo: forged-ingress stage=fetch->stage->visualize decode_ready={} decode_impl={}\n",
        (has_ftyp.is_some() && has_mdat.is_some()) as u8,
        0
    );

    match parse_h264_mp4_summary(body.as_slice()) {
        Ok(summary) => {
            let nal_types =
                first_sample_nal_types(summary.first_sample, summary.avcc.nal_length_size, 4);
            let mut presented_surface = false;
            let annex_b = match build_annex_b_access_unit(&summary) {
                Ok(annex_b) => annex_b,
                Err(err) => {
                    crate::log!(
                        "intel/media-demo: annexb staging failed err={:?} fallback=raw-sample-vis\n",
                        err
                    );
                    cpu_framebuffer_media_status_card_center(
                        "media-demo-h264-sample0",
                        MEDIA_HTTPS_DEMO_VIS_W,
                        MEDIA_HTTPS_DEMO_VIS_H,
                        0x00E2_B64A,
                        summary.first_sample_size as usize,
                        usize::from(summary.width).saturating_mul(usize::from(summary.height)),
                        false,
                    );
                    return;
                }
            };
            crate::log!(
                "intel/media-demo: h264 track=1 dims={}x{} timescale={} duration={} samples={} first_chunk=0x{:X} first_sample={} nal_len={} sps={} pps={} profile=0x{:02X} level=0x{:02X}\n",
                summary.width,
                summary.height,
                summary.timescale,
                summary.duration,
                summary.sample_count,
                summary.first_chunk_offset,
                summary.first_sample_size,
                summary.avcc.nal_length_size,
                summary.avcc.sps.len(),
                summary.avcc.pps.len(),
                summary.avcc.profile_idc,
                summary.avcc.level_idc
            );
            log_nal_summary(&nal_types);
            crate::log!(
                "intel/media-demo: annexb au0 bytes={} sample_nals={} idr={} stage=bitstream-ready\n",
                annex_b.bytes.len(),
                annex_b.sample_nal_count,
                annex_b.has_idr as u8
            );
            if let Some(draft) = draft_job_for_workload(MediaWorkloadKind::DecodeBitstream) {
                if let Some(warm) = warm_state() {
                    if let Some(staged) = prepare_decode_bitstream_demo(
                        warm,
                        draft,
                        summary.width,
                        summary.height,
                        annex_b.bytes.as_slice(),
                        annex_b.sample_nal_count,
                        annex_b.has_idr,
                    ) {
                        presented_surface = staged.present_ready;
                        store_decode_bitstream_demo_state(staged);
                    }
                }
            }
            crate::log!(
                "intel/media-demo: forged-ingress stage=fetch->parse->annexb-vis decode_ready={} decode_impl={}\n",
                1,
                1
            );
            if !presented_surface {
                cpu_framebuffer_media_status_card_center(
                    "media-demo-h264-annexb-au0",
                    MEDIA_HTTPS_DEMO_VIS_W,
                    MEDIA_HTTPS_DEMO_VIS_H,
                    0x003D_B7FF,
                    annex_b.bytes.len(),
                    annex_b.sample_nal_count,
                    true,
                );
            }
        }
        Err(err) => {
            crate::log!(
                "intel/media-demo: h264 parse failed err={:?} fallback=whole-file-vis\n",
                err
            );
            cpu_framebuffer_media_status_card_center(
                "media-demo-mp4-bytes",
                MEDIA_HTTPS_DEMO_VIS_W,
                MEDIA_HTTPS_DEMO_VIS_H,
                0x00C9_5A22,
                body.len(),
                first_box_size as usize,
                false,
            );
        }
    }
}
