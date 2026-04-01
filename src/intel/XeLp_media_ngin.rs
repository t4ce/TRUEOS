extern crate alloc;

use core::sync::atomic::{AtomicBool, Ordering};

use alloc::format;
use spin::Mutex;

use super::intel_guc;
use super::intel_igpu770::{
    Igpu770WarmState, cpu_framebuffer_visualize_bytes_center, forcewake_all_acquire,
    forcewake_media_refresh, mmio_read32, warm_state,
};
use super::xelp_media_mp4::{
    build_annex_b_access_unit, first_sample_nal_types, parse_h264_mp4_summary,
};

const MAX_MEDIA_ENGINES: usize = 4;
const MAX_MEDIA_API_ROUTES: usize = 5;
const MAX_MEDIA_RESULT_SLOTS: usize = 4;
const MAX_MEDIA_OBSERVE_REGS: usize = 10;

const FORCEWAKE_MEDIA_GEN11: usize = 0x0A184;
const FORCEWAKE_ACK_MEDIA: usize = 0x0D88;
const FORCEWAKE_KERNEL: u32 = 1 << 0;

const FORCEWAKE_ACK_VDBOX4: usize = 0x0D60;
const FORCEWAKE_ACK_VDBOX5: usize = 0x0D64;
const FORCEWAKE_ACK_VDBOX6: usize = 0x0D68;
const FORCEWAKE_ACK_VDBOX7: usize = 0x0D6C;
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
const RING_ACTHD: usize = 0x74;
const RING_MI_MODE: usize = 0x9C;
const RING_IPEIR: usize = 0x64;
const RING_IPEHR: usize = 0x68;
const RING_INSTDONE: usize = 0x6C;
const RING_INSTPS: usize = 0x70;
const RING_CONTEXT_CONTROL: usize = 0x244;
const RING_MODE_GEN7: usize = 0x29C;
const RING_EXECLIST_STATUS_LO: usize = 0x234;
const RING_EXECLIST_STATUS_HI: usize = 0x238;
const RING_EXECLIST_CONTROL: usize = 0x550;

const MEDIA_SHARED_STATUS_GPU_ADDR: u64 = 0x0110_0000;
const MEDIA_SHARED_STATUS_BYTES: usize = 0x1000;
const MEDIA_ENGINE_GPU_ADDR_BASE: u64 = 0x0120_0000;
const MEDIA_ENGINE_GPU_ADDR_STRIDE: u64 = 0x0020_0000;

const MEDIA_DEFAULT_RING_BYTES: usize = 16 * 1024;
const MEDIA_DEFAULT_CONTEXT_BYTES: usize = 22 * 4096;
const MEDIA_DEFAULT_BATCH_BYTES: usize = 32 * 1024;
const MEDIA_DEFAULT_SCRATCH_BYTES: usize = 64 * 1024;
const MEDIA_DEFAULT_RESULT_BYTES: usize = 4 * 4096;
const MEDIA_DEFAULT_WATCHDOG_ITERS: usize = 100_000;
const MEDIA_LRC_STATE_OFFSET_DWORDS: usize = 4096 / core::mem::size_of::<u32>();
const MEDIA_HTTPS_DEMO_URL: &str =
    "https://test-videos.co.uk/vids/bigbuckbunny/mp4/h264/720/Big_Buck_Bunny_720_10s_1MB.mp4";
const MEDIA_HTTPS_DEMO_TIMEOUT_MS: u32 = 45_000;
const MEDIA_HTTPS_DEMO_MAX_BYTES: usize = 2 * 1024 * 1024;
const MEDIA_HTTPS_DEMO_VIS_W: usize = 128;
const MEDIA_HTTPS_DEMO_VIS_H: usize = 72;

const MEDIA_RESULT_SLOT_BYTES: u64 = 8;
const MEDIA_RESULT_KICKOFF_SLOT: u64 = 0;
const MEDIA_RESULT_PRESUBMIT_SLOT: u64 = MEDIA_RESULT_KICKOFF_SLOT + MEDIA_RESULT_SLOT_BYTES;
const MEDIA_RESULT_POSTSUBMIT_SLOT: u64 = MEDIA_RESULT_PRESUBMIT_SLOT + MEDIA_RESULT_SLOT_BYTES;
const MEDIA_RESULT_COMPLETE_SLOT: u64 = MEDIA_RESULT_POSTSUBMIT_SLOT + MEDIA_RESULT_SLOT_BYTES;

const MEDIA_SLICE_ACK_REGS: [(&str, usize); 8] = [
    ("vdbox4", FORCEWAKE_ACK_VDBOX4),
    ("vdbox5", FORCEWAKE_ACK_VDBOX5),
    ("vdbox6", FORCEWAKE_ACK_VDBOX6),
    ("vdbox7", FORCEWAKE_ACK_VDBOX7),
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
                result_bytes: 0,
                windows: MediaGpuWindowLayout {
                    ring_gpu_addr: 0,
                    context_gpu_addr: 0,
                    batch_gpu_addr: 0,
                    scratch_gpu_addr: 0,
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
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct MediaJobDraft {
    pub engine: MediaEngineDescriptor,
    pub resources: MediaResourcePlan,
    pub context: MediaContextPlan,
    pub batch: MediaBatchTemplate,
    pub submission: MediaSubmissionPlan,
    pub next_stage: MediaKickoffStage,
}

static MEDIA_KICKOFF_RAN: AtomicBool = AtomicBool::new(false);
static MEDIA_KICKOFF_STATE: Mutex<Option<MediaKickoffState>> = Mutex::new(None);

#[inline]
const fn media_completion_slot_addr(base: u64, slot_off: u64) -> u64 {
    base + slot_off
}

#[inline]
const fn preferred_transport(guc_ready: bool) -> MediaSubmissionTransport {
    if guc_ready {
        MediaSubmissionTransport::GuC
    } else {
        MediaSubmissionTransport::Execlists
    }
}

#[inline]
fn media_command_encoding_ready(guc_ready: bool, wake: MediaForcewakeSnapshot) -> bool {
    guc_ready && (((wake.global_ack & FORCEWAKE_KERNEL) != 0) || wake.awake_count != 0)
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
        result_gpu_addr: base + 0x0018_0000,
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
        next_stage: plan.next_stage,
    })
}

fn log_job_draft(label: &str, draft: MediaJobDraft) {
    crate::log!(
        "intel/media: {} engine={} class={} provisioning={} transport={} workload={} ring=0x{:X} ctx=0x{:X} batch=0x{:X} scratch=0x{:X} result=0x{:X} next_stage={} completion_addr=0x{:X}\n",
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
            "intel/media: plan engine={} class={} provisioning={} ring_base=0x{:X} ring=0x{:X} ctx=0x{:X} batch=0x{:X} scratch=0x{:X} result=0x{:X} queue_depth={} completion={} next_stage={} observed={} ctl=0x{:08X} head=0x{:08X} tail=0x{:08X} execlist_lo=0x{:08X} execlist_hi=0x{:08X}\n",
            plan.descriptor.name,
            plan.descriptor.id.class.as_str(),
            plan.descriptor.provisioning.as_str(),
            plan.descriptor.ring_base,
            plan.resources.windows.ring_gpu_addr,
            plan.resources.windows.context_gpu_addr,
            plan.resources.windows.batch_gpu_addr,
            plan.resources.windows.scratch_gpu_addr,
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
            cpu_framebuffer_visualize_bytes_center(
                "media-demo-fetch-error",
                msg.as_bytes(),
                MEDIA_HTTPS_DEMO_VIS_W,
                MEDIA_HTTPS_DEMO_VIS_H,
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
            let annex_b = match build_annex_b_access_unit(&summary) {
                Ok(annex_b) => annex_b,
                Err(err) => {
                    crate::log!(
                        "intel/media-demo: annexb staging failed err={:?} fallback=raw-sample-vis\n",
                        err
                    );
                    cpu_framebuffer_visualize_bytes_center(
                        "media-demo-h264-sample0",
                        summary.first_sample,
                        MEDIA_HTTPS_DEMO_VIS_W,
                        MEDIA_HTTPS_DEMO_VIS_H,
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
            crate::log!(
                "intel/media-demo: forged-ingress stage=fetch->parse->annexb-vis decode_ready={} decode_impl={}\n",
                1,
                0
            );
            cpu_framebuffer_visualize_bytes_center(
                "media-demo-h264-annexb-au0",
                annex_b.bytes.as_slice(),
                MEDIA_HTTPS_DEMO_VIS_W,
                MEDIA_HTTPS_DEMO_VIS_H,
            );
        }
        Err(err) => {
            crate::log!(
                "intel/media-demo: h264 parse failed err={:?} fallback=whole-file-vis\n",
                err
            );
            cpu_framebuffer_visualize_bytes_center(
                "media-demo-mp4-bytes",
                body.as_slice(),
                MEDIA_HTTPS_DEMO_VIS_W,
                MEDIA_HTTPS_DEMO_VIS_H,
            );
        }
    }
}
