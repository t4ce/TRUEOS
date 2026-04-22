extern crate alloc;

use core::sync::atomic::{AtomicBool, Ordering};

const MAX_MEDIA_ENGINES: usize = 4;
const MAX_MEDIA_API_ROUTES: usize = 4;
const MAX_MEDIA_RESULT_SLOTS: usize = 4;

static MEDIA2_KICKED_OFF: AtomicBool = AtomicBool::new(false);
static MEDIA2_LOGGED_FIRST_FRAME: AtomicBool = AtomicBool::new(false);

#[derive(Copy, Clone, Debug)]
pub(crate) enum MediaEngineClass {
    VideoDecode,
    VideoEnhancement,
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum MediaProvisioning {
    Kickoff,
    ScaleOutReserve,
    Disabled,
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum MediaWorkloadKind {
    DecodeBitstream,
    DecodeFrame,
    EnhanceFrame,
    SessionSnapshot,
    EngineReset,
    Smoke,
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum MediaSubmissionTransport {
    GuC,
    Execlists,
    Disabled,
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum MediaKickoffStage {
    Discovery,
    ResourcePlanning,
    SubmissionWiring,
    CommandEncoding,
    Smoke,
}

#[derive(Copy, Clone, Debug)]
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

#[derive(Copy, Clone, Debug)]
pub(crate) struct MediaEngineDescriptor {
    pub name: &'static str,
    pub id: MediaEngineId,
    pub ring_base: usize,
    pub provisioning: MediaProvisioning,
    pub default_workload: MediaWorkloadKind,
    pub capabilities: MediaCapabilities,
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

#[derive(Copy, Clone, Debug)]
pub(crate) struct MediaSliceWakeAck {
    pub name: &'static str,
    pub value: u32,
    pub awake: bool,
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

#[derive(Copy, Clone, Debug)]
pub(crate) struct MediaApiShape {
    pub route_count: usize,
    pub routes: [MediaApiRoute; MAX_MEDIA_API_ROUTES],
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
    pub bitstream_bytes: usize,
    pub sample_nal_count: usize,
    pub has_idr: bool,
}

const fn empty_capabilities() -> MediaCapabilities {
    MediaCapabilities {
        decode: false,
        enhance: false,
        huc_assist: false,
        sfc: false,
        relative_mmio_lrc: false,
    }
}

const fn engine_descriptor(
    name: &'static str,
    class: MediaEngineClass,
    instance: u8,
) -> MediaEngineDescriptor {
    MediaEngineDescriptor {
        name,
        id: MediaEngineId { class, instance },
        ring_base: 0,
        provisioning: MediaProvisioning::Disabled,
        default_workload: MediaWorkloadKind::Smoke,
        capabilities: empty_capabilities(),
    }
}

const fn runtime_snapshot(name: &'static str) -> MediaEngineRuntimeSnapshot {
    MediaEngineRuntimeSnapshot {
        name,
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

const fn empty_slice() -> MediaSliceWakeAck {
    MediaSliceWakeAck {
        name: "unused",
        value: 0,
        awake: false,
    }
}

const fn empty_route() -> MediaApiRoute {
    MediaApiRoute {
        name: "placeholder",
        workload: MediaWorkloadKind::Smoke,
        preferred_engine_class: None,
        transport: MediaSubmissionTransport::Disabled,
        summary: "media2 placeholder",
    }
}

const fn zero_decode_frame() -> MediaDecodeFrameState {
    MediaDecodeFrameState {
        ready: false,
        engine_name: "vcs0",
        ring_gpu_addr: 0,
        context_gpu_addr: 0,
        batch_gpu_addr: 0,
        result_gpu_addr: 0,
        bitstream_gpu_addr: 0,
        output_surface_gpu_addr: 0,
        bitstream_phys: 0,
        output_surface_phys: 0,
        bitstream_bytes: 0,
        output_surface_bytes: 0,
        frame_width: 0,
        frame_height: 0,
        output_surface_pitch: 0,
        sample_nal_count: 0,
        has_idr: false,
        kickoff_marker: 0,
        complete_marker: 0,
        output_surface_signature: 0,
        output_surface_nonzero_samples: 0,
        submit_completed: false,
        present_attempted: false,
        present_ready: false,
        synthetic_preview: false,
    }
}

fn placeholder_kickoff_state() -> MediaKickoffState {
    MediaKickoffState {
        topology: MediaTopology {
            sku_name: "media2-placeholder",
            active_engine_count: MAX_MEDIA_ENGINES,
            planned_engine_count: MAX_MEDIA_ENGINES,
            engines: [
                engine_descriptor("vcs0", MediaEngineClass::VideoDecode, 0),
                engine_descriptor("vcs1", MediaEngineClass::VideoDecode, 1),
                engine_descriptor("vecs0", MediaEngineClass::VideoEnhancement, 0),
                engine_descriptor("vecs1", MediaEngineClass::VideoEnhancement, 1),
            ],
            default_decode: Some(MediaEngineId {
                class: MediaEngineClass::VideoDecode,
                instance: 0,
            }),
            default_enhance: Some(MediaEngineId {
                class: MediaEngineClass::VideoEnhancement,
                instance: 0,
            }),
        },
        runtime_count: MAX_MEDIA_ENGINES,
        runtimes: [
            runtime_snapshot("vcs0"),
            runtime_snapshot("vcs1"),
            runtime_snapshot("vecs0"),
            runtime_snapshot("vecs1"),
        ],
        wake: MediaForcewakeSnapshot {
            global_req: 0,
            global_ack: 0,
            awake_count: 0,
            slice_count: 0,
            slices: [empty_slice(); 8],
        },
        api: MediaApiShape {
            route_count: 1,
            routes: [
                empty_route(),
                empty_route(),
                empty_route(),
                empty_route(),
            ],
        },
        preferred_transport: MediaSubmissionTransport::Disabled,
        guc_ready: false,
        guc_status: 0,
        stage: MediaKickoffStage::Smoke,
        last_decode_frame: Some(zero_decode_frame()),
    }
}

pub(crate) fn kickoff_once() {
    if !MEDIA2_KICKED_OFF.swap(true, Ordering::AcqRel) {
        crate::log!("intel/media2: kickoff placeholder ready=0\n");
    }
}

pub(crate) fn kickoff_state() -> Option<MediaKickoffState> {
    Some(placeholder_kickoff_state())
}

pub(crate) fn decode_surface_window(_name: &str) -> Option<MediaSurfaceWindow> {
    None
}

pub(crate) async fn run_media_decode_async() {
    kickoff_once();
    let _ = run_media2_first_frame_async().await;
}

pub(crate) async fn run_media2_first_frame_async() -> Option<Media2FirstFrameState> {
    kickoff_once();
    let state = Media2FirstFrameState {
        ready: false,
        submit_completed: false,
        present_ready: false,
        frame_width: 0,
        frame_height: 0,
        output_surface_pitch: 0,
        output_surface_bytes: 0,
        output_surface_signature: 0,
        output_surface_nonzero_samples: 0,
        bitstream_bytes: 0,
        sample_nal_count: 0,
        has_idr: false,
    };
    if !MEDIA2_LOGGED_FIRST_FRAME.swap(true, Ordering::AcqRel) {
        crate::log!(
            "intel/media2: first frame idx=0 simplistic_submit=1 ready={} submit_completed={} present_ready={} bitstream_bytes={} output_surface_bytes={}\n",
            state.ready as u8,
            state.submit_completed as u8,
            state.present_ready as u8,
            state.bitstream_bytes,
            state.output_surface_bytes,
        );
    }
    Some(state)
}
