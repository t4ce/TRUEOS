//! Shell-controlled GPGPU live previews backed exclusively by UI4 frames.
//!
//! This is a trusted kernel app beside the permanent UI4 compositor. It
//! owns frame/window lifetime and compute cadence. The compute trio is admitted
//! through one broker session onto dedicated universal-plane slots; standalone
//! C++/IGC demos reuse the same exact-surface publication lifecycle on slot 1.
//! Display pipe programming remains exclusively compositor-owned.

use alloc::{string::String, vec::Vec};
use core::fmt::Write as _;

use embassy_time::{Duration, Instant, Timer};
use spin::Mutex;

use super::{
    DamageRect, FrameCadence, FrameContent, FrameHandle, FramePlanError, FramePoolError, FrameSpec,
    FrameWriteLease, OutputId, PremultipliedRgba8, ScanoutFormat, Ui4InputEvent, WindowCreate,
    WindowId, WindowOwner, WindowPlacement, WindowPlane, WindowSessionCloseRequest,
    WindowSessionId, acquire_frame_buffer, begin_window_session, cancel_frame_buffer, create_frame,
    create_window, destroy_frame, finish_window_session, finish_window_session_with_request,
    gpgpu_rgba_surface, publish_frame_buffer, publish_gpgpu_frame_buffer,
    publish_gpu_font_frame_buffer, publish_window_frame, publish_window_frames,
    replace_window_frame, take_owner_input_events, window_placement, writable_rgba_view,
};

const PREVIEW_OWNER: WindowOwner = WindowOwner::GPGPU_PREVIEW;
const PREVIEW_WIDTH: u32 = super::DEFAULT_FRAME_WIDTH;
const PREVIEW_HEIGHT: u32 = super::DEFAULT_FRAME_HEIGHT;
const LAB256_PREVIEW_SIZE: u32 = 256;
const PREVIEW_MARGIN: u32 = 64;
const PREVIEW_GRID_GAP: u32 = 16;
const PREVIEW_Z: i32 = 30;
const IDLE_POLL_MS: u64 = 20;
const COMMAND_POLL_MAX_MS: u64 = 10;
const RESIZE_RETRY_MS: u64 = 250;
const STATIC30_FRAME_COUNT: usize = 30;
const STATIC30_PLANE_COUNT: usize = 3;
const STATIC30_COLUMNS: u32 = 6;
const STATIC30_ROWS: u32 = 5;
const STATIC30_MAX_WIDTH: u32 = 320;
const STATIC30_MAX_HEIGHT: u32 = 180;
const CPP_FONT_RUSH_MAX_PLANES: usize = 4;
const CPP_FONT_RUSH_CADENCE_MS: u64 = 1_000;
const CPP_FONT_RUSH_ADD_PLANE_MS: u64 = 3_000;
const CPP_FONT_RUSH_VIEWPORT_SCALE: u32 = 4;
const CPP_FONT_RUSH_GLYPHS: [usize; CPP_FONT_RUSH_MAX_PLANES] = [1, 2, 4, 16];

pub(crate) const GPGPU_PREVIEW_DEFAULT_DURATION_MS: u64 = 5_000;
pub(crate) const GPGPU_PREVIEW_DEFAULT_CADENCE_MS: u64 = 33;
pub(crate) const GPGPU_PREVIEW_DEFAULT_PUBLISH_EVERY: u32 = 1;
pub(crate) const GPGPU_PREVIEW_MAX_CADENCE_MS: u64 = 60_000;
pub(crate) const GPGPU_PREVIEW_MAX_PUBLISH_EVERY: u32 = 1_024;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum GpgpuPreviewPreset {
    All,
    Static,
    Static30,
    Mandelbrot,
    Chart,
    Plasma,
    Lab256,
    CppGallery,
    CppAurora,
    CppJulia,
    CppSdf,
    CppVoronoi,
    CppRetroSun,
    CppAudio,
    CppParticle,
    CppFont,
    CppFontRush,
}

impl GpgpuPreviewPreset {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::All => "compute-trio",
            Self::Static => "static",
            Self::Static30 => "static30",
            Self::Mandelbrot => "mandelbrot",
            Self::Chart => "chart",
            Self::Plasma => "plasma",
            Self::Lab256 => "lab256",
            Self::CppGallery => "cpp-gallery",
            Self::CppAurora => "cpp-aurora",
            Self::CppJulia => "cpp-julia",
            Self::CppSdf => "cpp-sdf",
            Self::CppVoronoi => "cpp-voronoi",
            Self::CppRetroSun => "cpp-retro-sun",
            Self::CppAudio => "cpp-audio",
            Self::CppParticle => "cpp-particle",
            Self::CppFont => "cpp-font",
            Self::CppFontRush => "cpp-font-rush",
        }
    }

    pub(crate) const fn is_cpp(self) -> bool {
        matches!(
            self,
            Self::CppGallery
                | Self::CppAurora
                | Self::CppJulia
                | Self::CppSdf
                | Self::CppVoronoi
                | Self::CppRetroSun
                | Self::CppAudio
                | Self::CppParticle
                | Self::CppFont
                | Self::CppFontRush
        )
    }

    pub(crate) const fn is_resizable_cpp(self) -> bool {
        self.is_cpp() && !matches!(self, Self::CppFont | Self::CppFontRush)
    }

    pub(crate) const fn buffering_label(self) -> &'static str {
        match self {
            Self::All => "double-per-frame",
            Self::Static30 | Self::CppFont => "single",
            Self::CppFontRush => "double-per-plane",
            _ => "double",
        }
    }

    pub(crate) const fn plane_layout_label(self) -> &'static str {
        match self {
            Self::All => "slots1+2+3-direct",
            Self::Static => "slot1-direct",
            Self::Static30 => "slots1+2+3/10-each",
            Self::Mandelbrot => "slot1-direct",
            Self::Chart => "slot2-direct",
            Self::Plasma => "slot3-direct",
            Self::Lab256 => "slot1-alpha-256x256",
            Self::CppGallery
            | Self::CppAurora
            | Self::CppJulia
            | Self::CppSdf
            | Self::CppVoronoi
            | Self::CppRetroSun
            | Self::CppAudio
            | Self::CppParticle
            | Self::CppFont => "slot1-direct",
            Self::CppFontRush => "slots0+1+2+3-direct-capability-bounded",
        }
    }
}

const fn preview_extent(preset: GpgpuPreviewPreset) -> (u32, u32) {
    match preset {
        GpgpuPreviewPreset::Lab256 => (LAB256_PREVIEW_SIZE, LAB256_PREVIEW_SIZE),
        GpgpuPreviewPreset::CppParticle => (
            crate::intel::gpgpu::PARTICLE_CRAFT_FRAME_WIDTH,
            crate::intel::gpgpu::PARTICLE_CRAFT_FRAME_HEIGHT,
        ),
        _ => (PREVIEW_WIDTH, PREVIEW_HEIGHT),
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct GpgpuPreviewConfig {
    pub(crate) preset: GpgpuPreviewPreset,
    pub(crate) duration_ms: u64,
    pub(crate) cadence_ms: u64,
    pub(crate) publish_every: u32,
}

impl GpgpuPreviewConfig {
    pub(crate) const DEFAULT: Self = Self {
        preset: GpgpuPreviewPreset::All,
        duration_ms: GPGPU_PREVIEW_DEFAULT_DURATION_MS,
        cadence_ms: GPGPU_PREVIEW_DEFAULT_CADENCE_MS,
        publish_every: GPGPU_PREVIEW_DEFAULT_PUBLISH_EVERY,
    };

    fn validate(self) -> Result<Self, &'static str> {
        if self.cadence_ms == 0 || self.cadence_ms > GPGPU_PREVIEW_MAX_CADENCE_MS {
            return Err("cadence-ms-out-of-range-1-to-60000");
        }
        if self.publish_every == 0 || self.publish_every > GPGPU_PREVIEW_MAX_PUBLISH_EVERY {
            return Err("publish-every-out-of-range-1-to-1024");
        }
        Ok(self)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum GpgpuPreviewPhase {
    Offline,
    Idle,
    Starting,
    Running,
    Faulted,
}

impl GpgpuPreviewPhase {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Offline => "offline",
            Self::Idle => "idle",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Faulted => "faulted",
        }
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct GpgpuPreviewMetrics {
    pub(crate) attempted: u64,
    pub(crate) submitted: u64,
    pub(crate) completed: u64,
    pub(crate) published: u64,
    pub(crate) scanout_live: u64,
    pub(crate) scanout_superseded: u64,
    pub(crate) dropped_busy: u64,
    pub(crate) dropped_frame_busy: u64,
    pub(crate) dropped_queue_full: u64,
    pub(crate) dropped_in_flight: u64,
    pub(crate) dropped_cadence: u64,
    pub(crate) failed: u64,
    pub(crate) late: u64,
    pub(crate) elapsed_ms: u64,
    pub(crate) last_iterations: u32,
    pub(crate) last_marker: u32,
    pub(crate) last_submit_ms: u64,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct GpgpuPreviewStatus {
    pub(crate) online: bool,
    pub(crate) desired_running: bool,
    pub(crate) phase: GpgpuPreviewPhase,
    pub(crate) request_serial: u64,
    pub(crate) applied_serial: u64,
    pub(crate) config: GpgpuPreviewConfig,
    pub(crate) frame: Option<FrameHandle>,
    pub(crate) window: Option<WindowId>,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) metrics: GpgpuPreviewMetrics,
    pub(crate) members: [GpgpuPreviewMemberStatus; 4],
    pub(crate) last_error: &'static str,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct GpgpuPreviewMemberStatus {
    pub(crate) preset: GpgpuPreviewPreset,
    pub(crate) frame: Option<FrameHandle>,
    pub(crate) window: Option<WindowId>,
    pub(crate) plane_slot: u8,
    pub(crate) metrics: GpgpuPreviewMetrics,
}

impl GpgpuPreviewMemberStatus {
    const fn inactive(preset: GpgpuPreviewPreset, plane_slot: u8) -> Self {
        Self {
            preset,
            frame: None,
            window: None,
            plane_slot,
            metrics: GpgpuPreviewMetrics {
                attempted: 0,
                submitted: 0,
                completed: 0,
                published: 0,
                scanout_live: 0,
                scanout_superseded: 0,
                dropped_busy: 0,
                dropped_frame_busy: 0,
                dropped_queue_full: 0,
                dropped_in_flight: 0,
                dropped_cadence: 0,
                failed: 0,
                late: 0,
                elapsed_ms: 0,
                last_iterations: 0,
                last_marker: 0,
                last_submit_ms: 0,
            },
        }
    }
}

const INACTIVE_PREVIEW_MEMBERS: [GpgpuPreviewMemberStatus; 4] = [
    GpgpuPreviewMemberStatus::inactive(GpgpuPreviewPreset::Mandelbrot, 1),
    GpgpuPreviewMemberStatus::inactive(GpgpuPreviewPreset::Chart, 2),
    GpgpuPreviewMemberStatus::inactive(GpgpuPreviewPreset::Plasma, 3),
    GpgpuPreviewMemberStatus::inactive(GpgpuPreviewPreset::CppFontRush, 0),
];

const COMPUTE_PREVIEW_PRESETS: [GpgpuPreviewPreset; 3] = [
    GpgpuPreviewPreset::Mandelbrot,
    GpgpuPreviewPreset::Chart,
    GpgpuPreviewPreset::Plasma,
];

impl GpgpuPreviewStatus {
    const fn initial() -> Self {
        Self {
            online: false,
            desired_running: false,
            phase: GpgpuPreviewPhase::Offline,
            request_serial: 0,
            applied_serial: 0,
            config: GpgpuPreviewConfig::DEFAULT,
            frame: None,
            window: None,
            width: 0,
            height: 0,
            metrics: GpgpuPreviewMetrics {
                attempted: 0,
                submitted: 0,
                completed: 0,
                published: 0,
                scanout_live: 0,
                scanout_superseded: 0,
                dropped_busy: 0,
                dropped_frame_busy: 0,
                dropped_queue_full: 0,
                dropped_in_flight: 0,
                dropped_cadence: 0,
                failed: 0,
                late: 0,
                elapsed_ms: 0,
                last_iterations: 0,
                last_marker: 0,
                last_submit_ms: 0,
            },
            members: INACTIVE_PREVIEW_MEMBERS,
            last_error: "none",
        }
    }
}

#[derive(Copy, Clone)]
struct DesiredPreview {
    serial: u64,
    running: bool,
    config: GpgpuPreviewConfig,
    policy: PreviewRunPolicy,
}

#[derive(Copy, Clone)]
struct PreviewRunPolicy {
    frame_limit: u64,
    target_hz: u64,
}

impl PreviewRunPolicy {
    const SHELL: Self = Self {
        frame_limit: 0,
        target_hz: 0,
    };
}

struct PreviewControl {
    desired: DesiredPreview,
    status: GpgpuPreviewStatus,
}

impl PreviewControl {
    const fn new() -> Self {
        Self {
            desired: DesiredPreview {
                serial: 0,
                running: false,
                config: GpgpuPreviewConfig::DEFAULT,
                policy: PreviewRunPolicy::SHELL,
            },
            status: GpgpuPreviewStatus::initial(),
        }
    }
}

static PREVIEW_CONTROL: Mutex<PreviewControl> = Mutex::new(PreviewControl::new());
static CPP_FONT_REQUEST: Mutex<Option<(u64, crate::r::font_kernel_service::FontStampRequest)>> =
    Mutex::new(None);

struct ActivePreview {
    request_serial: u64,
    config: GpgpuPreviewConfig,
    policy: PreviewRunPolicy,
    cadence_phase: u64,
    session: WindowSessionId,
    frame: FrameHandle,
    window: WindowId,
    width: u32,
    height: u32,
    resize_retry_width: u32,
    resize_retry_height: u32,
    resize_retry_at: Instant,
    started: Instant,
    next_render: Instant,
    static_needs_publish: bool,
    extra_surfaces: Vec<StaticPreviewSurface>,
    particle_craft: Option<crate::intel::gpgpu::GpgpuOwnedParticleCraftState>,
    font_stamp: Option<crate::r::font_kernel_service::FontStampRequest>,
    font_rush: Option<CppFontRushPlaneState>,
    exclusive_admission: Option<super::Ui4ExclusiveResourceAdmission>,
    metrics: GpgpuPreviewMetrics,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct CppFontRushTopology {
    width: u32,
    height: u32,
    plane_mask: u8,
    plane_count: u8,
}

struct CppFontRushPlaneState {
    rank: u8,
    topology: CppFontRushTopology,
    /// The 3-second plane-growth cadence begins only once Slot0 has actually
    /// published its first generated frame. Service-lane admission may delay
    /// that first frame during boot and must not make all later planes catch up
    /// in one burst.
    growth_started: Option<Instant>,
    rng: crate::tyche::SoftRng,
    pending: Option<CppFontRushPendingFrame>,
    scanout_pending: Option<CppFontRushPendingScanout>,
}

struct CppFontRushPendingFrame {
    lease: FrameWriteLease,
    completion: crate::r::font_kernel_service::PendingFontFrameStamp,
    ticket: crate::r::font_kernel_service::FontKernelTicket,
    sequence: u64,
    scheduled_at: Instant,
    submit_started_at: Instant,
    accepted_at: Instant,
    requested_glyphs: usize,
    columns: u8,
    rows: u8,
    glyph_fingerprint: u64,
    glyph_ids: String,
    fifo_queued_ahead: usize,
    request_build_ms: u64,
}

#[derive(Copy, Clone, Debug)]
struct CppFontRushPendingScanout {
    ticket: crate::r::font_kernel_service::FontKernelTicket,
    sequence: u64,
    producer_buffer: u8,
    frame_publish_serial: u64,
    window_publish_serial: u64,
    release_sequence: u64,
    published_at: Instant,
    glyph_fingerprint: u64,
}

#[derive(Copy, Clone)]
struct StaticPreviewSurface {
    frame: FrameHandle,
    window: WindowId,
    scheme: u8,
}

pub(crate) fn request_gpgpu_preview_start(config: GpgpuPreviewConfig) -> Result<u64, &'static str> {
    request_gpgpu_preview_start_with_policy(config, PreviewRunPolicy::SHELL)
}

pub(crate) fn request_cpp_font_preview_start(
    request: crate::r::font_kernel_service::FontStampRequest,
) -> Result<u64, &'static str> {
    let first = request.layers.first().ok_or("font-stamp-layer-count")?;
    if request.fit != crate::r::font_kernel_service::FontStampFit::Canvas {
        return Err("font-preview-requires-canvas");
    }
    let width = first.scene.raster_width;
    let height = first.scene.raster_height;
    let config = GpgpuPreviewConfig {
        preset: GpgpuPreviewPreset::CppFont,
        duration_ms: 0,
        cadence_ms: GPGPU_PREVIEW_DEFAULT_CADENCE_MS,
        publish_every: 1,
    };
    let mut control = PREVIEW_CONTROL.lock();
    let serial = next_serial(control.desired.serial);
    *CPP_FONT_REQUEST.lock() = Some((serial, request));
    control.desired = DesiredPreview {
        serial,
        running: true,
        config,
        policy: PreviewRunPolicy::SHELL,
    };
    control.status.desired_running = true;
    control.status.phase = GpgpuPreviewPhase::Starting;
    control.status.request_serial = serial;
    control.status.config = config;
    control.status.width = width;
    control.status.height = height;
    control.status.last_error = "none";
    Ok(serial)
}

pub(crate) fn request_cpp_font_rush_start() -> Result<u64, &'static str> {
    if !crate::r::font_kernel_service::status().online {
        return Err("font-service-offline");
    }
    crate::intel::gpu_font::ensure_font_face_available(
        crate::intel::gpu_font::GpuFontFace::Default,
    )?;
    ensure_cpp_font_rush_ui4_idle("request")?;
    let (_, topology) = cpp_font_rush_topology()?;
    log_cpp_font_rush_capabilities("request", 0, topology);
    let config = GpgpuPreviewConfig {
        preset: GpgpuPreviewPreset::CppFontRush,
        duration_ms: 0,
        cadence_ms: CPP_FONT_RUSH_CADENCE_MS,
        publish_every: 1,
    }
    .validate()?;
    // The authoritative busy check and request commit share one control lock.
    // A concurrent preview request can win before this point, but it can no
    // longer be silently overwritten between separate check/commit sections.
    let mut control = PREVIEW_CONTROL.lock();
    if control.desired.running {
        return Err("gpgpu-preview-busy");
    }
    CPP_FONT_REQUEST.lock().take();
    let serial = next_serial(control.desired.serial);
    control.desired = DesiredPreview {
        serial,
        running: true,
        config,
        policy: PreviewRunPolicy::SHELL,
    };
    control.status.desired_running = true;
    control.status.phase = GpgpuPreviewPhase::Starting;
    control.status.request_serial = serial;
    control.status.config = config;
    control.status.last_error = "none";
    Ok(serial)
}

pub(crate) fn request_cpp_font_rush_stop() -> Result<u64, &'static str> {
    let mut control = PREVIEW_CONTROL.lock();
    if !control.desired.running || control.desired.config.preset != GpgpuPreviewPreset::CppFontRush
    {
        return Err("font-rush-not-running");
    }
    let serial = next_serial(control.desired.serial);
    control.desired.serial = serial;
    control.desired.running = false;
    control.status.desired_running = false;
    control.status.request_serial = serial;
    CPP_FONT_REQUEST.lock().take();
    Ok(serial)
}

fn ensure_cpp_font_rush_ui4_idle(stage: &'static str) -> Result<(), &'static str> {
    let usage = super::ui4_live_resource_usage();
    if usage.is_display_idle() {
        if usage.active_frames != 0 {
            crate::log_info!(
                target: "ui4";
                "ui4 cpp-font-rush admission accepted stage={} display_idle=1 detached_active_frames={} active_sessions=0 live_windows=0 detached_policy=allowed-exclusive-session-gate\n",
                stage,
                usage.active_frames,
            );
        }
        return Ok(());
    }
    crate::log_warn!(
        target: "ui4";
        "ui4 cpp-font-rush admission rejected stage={} reason=ui4-not-idle active_frames={} active_sessions={} live_windows={}\n",
        stage,
        usage.active_frames,
        usage.active_sessions,
        usage.live_windows,
    );
    Err("ui4-not-idle")
}

fn acquire_cpp_font_rush_ui4_admission(
    stage: &'static str,
) -> Result<super::Ui4ExclusiveResourceAdmission, &'static str> {
    match super::try_acquire_ui4_exclusive_resource_admission() {
        Ok(admission) => Ok(admission),
        Err(failure) => {
            crate::log_warn!(
                target: "ui4";
                "ui4 cpp-font-rush admission rejected stage={} reason={} active_frames={} active_sessions={} live_windows={} exclusive_reservation=1\n",
                stage,
                failure.reason,
                failure.usage.active_frames,
                failure.usage.active_sessions,
                failure.usage.live_windows,
            );
            Err(failure.reason)
        }
    }
}

fn cpp_font_rush_contiguous_plane_count(application_plane_mask: u8) -> u8 {
    let mut count = 0u8;
    while usize::from(count) < CPP_FONT_RUSH_MAX_PLANES
        && application_plane_mask & (1u8 << count) != 0
    {
        count += 1;
    }
    count
}

const fn cpp_font_rush_target_plane_count(elapsed_ms: u64, available_planes: u8) -> usize {
    let timed_planes = 1usize.saturating_add((elapsed_ms / CPP_FONT_RUSH_ADD_PLANE_MS) as usize);
    let available = if available_planes as usize > CPP_FONT_RUSH_MAX_PLANES {
        CPP_FONT_RUSH_MAX_PLANES
    } else {
        available_planes as usize
    };
    if timed_planes < available {
        timed_planes
    } else {
        available
    }
}

fn cpp_font_rush_topology() -> Result<(OutputId, CppFontRushTopology), &'static str> {
    let output = OutputId::from_slot(0).ok_or("output-d01-unavailable")?;
    let capabilities =
        super::ui4_output_capabilities(output).ok_or("ui4-output-capabilities-unavailable")?;
    let plane_count = cpp_font_rush_contiguous_plane_count(capabilities.application_plane_mask);
    if plane_count == 0 {
        return Err("font-rush-primary-plane-unavailable");
    }
    if capabilities.width == 0
        || capabilities.height == 0
        || capabilities.width > crate::r::font_kernel_service::FONT_STAMP_MAX_EXTENT
        || capabilities.height > crate::r::font_kernel_service::FONT_STAMP_MAX_EXTENT
        || u64::from(capabilities.width) * u64::from(capabilities.height)
            > crate::r::font_kernel_service::FONT_STAMP_MAX_PIXELS
    {
        return Err("font-rush-output-extent-unsupported");
    }
    Ok((
        output,
        CppFontRushTopology {
            width: capabilities.width,
            height: capabilities.height,
            plane_mask: capabilities.application_plane_mask,
            plane_count,
        },
    ))
}

fn log_cpp_font_rush_capabilities(
    stage: &'static str,
    active_planes: usize,
    topology: CppFontRushTopology,
) {
    crate::log_info!(
        target: "ui4";
        "ui4 cpp-font-rush capabilities stage={} extent={}x{} application_plane_mask=0x{:02X} reported_planes={} usable_contiguous_planes={} active_planes={} max_planes={} policy=stop-at-first-unsupported-slot\n",
        stage,
        topology.width,
        topology.height,
        topology.plane_mask,
        topology.plane_mask.count_ones(),
        topology.plane_count,
        active_planes,
        CPP_FONT_RUSH_MAX_PLANES,
    );
}

pub(crate) fn request_gpgpu_lab256_startup(
    frame_limit: u64,
    target_hz: u64,
) -> Result<u64, &'static str> {
    if frame_limit == 0 {
        return Err("frame-limit-must-be-nonzero");
    }
    if target_hz == 0 || target_hz > embassy_time::TICK_HZ {
        return Err("target-hz-out-of-range");
    }
    request_gpgpu_preview_start_with_policy(
        GpgpuPreviewConfig {
            preset: GpgpuPreviewPreset::Lab256,
            duration_ms: 0,
            cadence_ms: 1_000u64.div_ceil(target_hz),
            publish_every: 1,
        },
        PreviewRunPolicy {
            frame_limit,
            target_hz,
        },
    )
}

fn request_gpgpu_preview_start_with_policy(
    config: GpgpuPreviewConfig,
    policy: PreviewRunPolicy,
) -> Result<u64, &'static str> {
    let config = config.validate()?;
    let mut control = PREVIEW_CONTROL.lock();
    CPP_FONT_REQUEST.lock().take();
    let serial = next_serial(control.desired.serial);
    control.desired = DesiredPreview {
        serial,
        running: true,
        config,
        policy,
    };
    control.status.desired_running = true;
    control.status.phase = GpgpuPreviewPhase::Starting;
    control.status.request_serial = serial;
    control.status.config = config;
    control.status.last_error = "none";
    Ok(serial)
}

pub(crate) fn request_gpgpu_preview_stop() -> u64 {
    let mut control = PREVIEW_CONTROL.lock();
    let serial = next_serial(control.desired.serial);
    control.desired.serial = serial;
    control.desired.running = false;
    control.status.desired_running = false;
    control.status.request_serial = serial;
    CPP_FONT_REQUEST.lock().take();
    serial
}

pub(crate) fn gpgpu_preview_status() -> GpgpuPreviewStatus {
    PREVIEW_CONTROL.lock().status
}

#[embassy_executor::task(pool_size = 1)]
pub(crate) async fn gpgpu_preview_consumer_service_task(worker_slot: u32) {
    crate::log_info!(
        target: "ui4";
        "ui4 gpgpu-preview-consumer carrier online owner={:?} placement=worker-ap2+ assigned_slot={} current_slot={} display_api=none activation=Shell2/on-demand buffering=double compute_release=completion-marker-before-publish interaction=movable/maximize-resize-cpp\n",
        PREVIEW_OWNER,
        worker_slot,
        crate::percpu::current_slot(),
    );
    {
        let mut control = PREVIEW_CONTROL.lock();
        control.status.online = true;
        control.status.phase = GpgpuPreviewPhase::Idle;
    }

    let mut applied_serial = 0u64;
    let mut active = Vec::<ActivePreview>::new();
    let mut retired_frames = Vec::new();

    loop {
        retire_frames(&mut retired_frames);
        drain_preview_input(&mut active, &mut retired_frames);
        reconcile_preview_extents(&mut active, &mut retired_frames);

        let mut desired = PREVIEW_CONTROL.lock().desired;
        if desired.serial != applied_serial {
            if !active.is_empty() {
                drain_cpp_font_rush_pending(&mut active).await;
                stop_active_previews(
                    core::mem::take(&mut active),
                    &mut retired_frames,
                    "command-replaced",
                );
            }
            // A drain can take long enough for another shell command to
            // replace the one which initiated it. Apply the newest request
            // after all old producer leases are safe, never a stale snapshot.
            desired = PREVIEW_CONTROL.lock().desired;
            applied_serial = desired.serial;
            if desired.running {
                crate::aud::audio_visualizer::set_enabled(
                    desired.config.preset == GpgpuPreviewPreset::CppAudio,
                );
                mark_starting(desired);
                match initialize_previews(desired) {
                    Ok(previews) => {
                        crate::log_info!(
                            target: "ui4";
                            "ui4 gpgpu-preview start request={} preset={} producer={} owner={:?} frames={} windows={} extent={}x{} cadence_ms={} target_hz={} frame_limit={} publish_every={} duration_ms={} buffering={} release={} plane_layout={} slot_policy=fixed-per-window/no-round-robin broker_session={} plane_mutation=none\n",
                            desired.serial,
                            desired.config.preset.label(),
                            preview_producer_label(desired.config.preset),
                            PREVIEW_OWNER,
                            previews.len(),
                            previews.iter().map(preview_surface_count).sum::<usize>(),
                            previews.first().map_or(0, |preview| preview.width),
                            previews.first().map_or(0, |preview| preview.height),
                            desired.config.cadence_ms,
                            desired.policy.target_hz,
                            desired.policy.frame_limit,
                            desired.config.publish_every,
                            desired.config.duration_ms,
                            desired.config.preset.buffering_label(),
                            preview_release_label(desired.config.preset),
                            desired.config.preset.plane_layout_label(),
                            previews.first().map_or(0, |preview| preview.session.raw()),
                        );
                        for preview in &previews {
                            crate::log_info!(
                                target: "ui4";
                                "ui4 gpgpu-preview broker-member request={} preset={} frame={} window={} slot={} extent={}x{} buffering=double consumer={} display_release=surflive\n",
                                preview.request_serial,
                                preview.config.preset.label(),
                                preview.frame.raw(),
                                preview.window.raw(),
                                active_preview_plane_slot(preview),
                                preview.width,
                                preview.height,
                                preview_consumer_label(preview.config.preset),
                            );
                        }
                        publish_active_status(&previews, GpgpuPreviewPhase::Running, "none");
                        active = previews;
                    }
                    Err(reason) => {
                        crate::aud::audio_visualizer::set_enabled(false);
                        mark_faulted(desired, reason);
                        crate::log_warn!(
                            target: "ui4";
                            "ui4 gpgpu-preview start rejected request={} reason={}\n",
                            desired.serial,
                            reason,
                        );
                    }
                }
            } else {
                crate::aud::audio_visualizer::set_enabled(false);
                mark_idle(applied_serial, "stopped");
            }
        }

        let now = Instant::now();
        let mut duration_expired = false;
        let mut render_fault = None;
        let font_rush_active = active
            .first()
            .is_some_and(|preview| preview.config.preset == GpgpuPreviewPreset::CppFontRush);
        if font_rush_active {
            if let Err(reason) = poll_cpp_font_rush_consumers(&mut active) {
                render_fault = Some(reason);
            }
            let now = Instant::now();
            if render_fault.is_none()
                && let Err(reason) = grow_cpp_font_rush(&mut active, now)
            {
                render_fault = Some(reason);
            }
            for preview in &mut active {
                preview.metrics.elapsed_ms = Instant::now()
                    .saturating_duration_since(preview.started)
                    .as_millis();
                duration_expired |= preview.config.duration_ms != 0
                    && preview.metrics.elapsed_ms >= preview.config.duration_ms;
            }
            if render_fault.is_none()
                && !duration_expired
                && let Err(reason) = queue_due_cpp_font_rush_consumers(&mut active, Instant::now())
            {
                render_fault = Some(reason);
            }
        } else {
            for preview in &mut active {
                if render_fault.is_some() {
                    break;
                }
                preview.metrics.elapsed_ms =
                    now.saturating_duration_since(preview.started).as_millis();
                duration_expired |= preview.config.duration_ms != 0
                    && preview.metrics.elapsed_ms >= preview.config.duration_ms;
                if !duration_expired && preview_needs_render(preview) && now >= preview.next_render
                {
                    match render_preview_frame(preview).await {
                        Ok(()) => {
                            schedule_next_render(preview);
                        }
                        Err(reason) => render_fault = Some(reason),
                    }
                }
            }
        }
        if !active.is_empty() && render_fault.is_none() && !duration_expired {
            publish_active_status(&active, GpgpuPreviewPhase::Running, "none");
        }

        if let Some(reason) = render_fault {
            if !active.is_empty() {
                drain_cpp_font_rush_pending(&mut active).await;
                let serial = active[0].request_serial;
                let metrics = aggregate_preview_metrics(&active);
                let members = preview_member_statuses(&active);
                stop_active_previews(
                    core::mem::take(&mut active),
                    &mut retired_frames,
                    "render-fault",
                );
                mark_runtime_fault(serial, metrics, members, reason);
                crate::log_warn!(
                    target: "ui4";
                    "ui4 gpgpu-preview faulted request={} reason={} attempted={} submitted={} completed={} published={} failed={}\n",
                    serial,
                    reason,
                    metrics.attempted,
                    metrics.submitted,
                    metrics.completed,
                    metrics.published,
                    metrics.failed,
                );
            }
        }

        if duration_expired {
            if !active.is_empty() {
                drain_cpp_font_rush_pending(&mut active).await;
                let serial = active[0].request_serial;
                let metrics = aggregate_preview_metrics(&active);
                let members = preview_member_statuses(&active);
                stop_active_previews(
                    core::mem::take(&mut active),
                    &mut retired_frames,
                    "duration-complete",
                );
                mark_duration_complete(serial, metrics, members);
            }
        }

        let wait_ms = next_preview_group_poll_ms(&active);
        Timer::after(Duration::from_millis(wait_ms)).await;
    }
}

fn initialize_previews(desired: DesiredPreview) -> Result<Vec<ActivePreview>, &'static str> {
    if desired.config.preset == GpgpuPreviewPreset::All {
        initialize_compute_preview_set(desired)
    } else if desired.config.preset == GpgpuPreviewPreset::CppFontRush {
        initialize_cpp_font_rush_set(desired)
    } else {
        Ok(alloc::vec![initialize_preview(desired)?])
    }
}

fn initialize_compute_preview_set(
    desired: DesiredPreview,
) -> Result<Vec<ActivePreview>, &'static str> {
    let output = OutputId::from_slot(0).ok_or("output-d01-unavailable")?;
    let session =
        begin_window_session(PREVIEW_OWNER).map_err(|_| "compute-trio-session-create-failed")?;
    let (output_width, output_height) =
        crate::intel::active_scanout_dimensions().unwrap_or((2560, 1440));
    let columns = ((output_width.saturating_sub(PREVIEW_GRID_GAP))
        / PREVIEW_WIDTH.saturating_add(PREVIEW_GRID_GAP))
    .clamp(1, COMPUTE_PREVIEW_PRESETS.len() as u32);
    let mut previews = Vec::with_capacity(COMPUTE_PREVIEW_PRESETS.len());

    for (index, preset) in COMPUTE_PREVIEW_PRESETS.iter().copied().enumerate() {
        let frame = match create_preview_frame(output, PREVIEW_WIDTH, PREVIEW_HEIGHT) {
            Ok(frame) => frame,
            Err(_) => {
                abandon_compute_preview_initialization(session, &previews);
                return Err("compute-trio-frame-create-failed");
            }
        };
        let column = index as u32 % columns;
        let row = index as u32 / columns;
        let x = PREVIEW_GRID_GAP
            .saturating_add(column.saturating_mul(PREVIEW_WIDTH.saturating_add(PREVIEW_GRID_GAP)));
        let y = PREVIEW_GRID_GAP
            .saturating_add(row.saturating_mul(PREVIEW_HEIGHT.saturating_add(PREVIEW_GRID_GAP)));
        if x.saturating_add(PREVIEW_WIDTH) > output_width
            || y.saturating_add(PREVIEW_HEIGHT) > output_height
        {
            let _ = destroy_frame(frame);
            abandon_compute_preview_initialization(session, &previews);
            return Err("compute-trio-output-too-small");
        }
        let plane_slot = index + 1;
        let window = match create_window(WindowCreate {
            owner: PREVIEW_OWNER,
            session,
            frame,
            output,
            plane: WindowPlane::Universal(plane_slot as u8),
            placement: WindowPlacement {
                x: x as i32,
                y: y as i32,
                width: PREVIEW_WIDTH,
                height: PREVIEW_HEIGHT,
                z: PREVIEW_Z.saturating_add(index as i32),
                opacity: u8::MAX,
                visible: true,
            },
            interaction: super::WindowInteraction::MOVABLE_FRAME,
        }) {
            Ok(window) => window,
            Err(_) => {
                let _ = destroy_frame(frame);
                abandon_compute_preview_initialization(session, &previews);
                return Err("compute-trio-window-create-failed");
            }
        };
        let mut config = desired.config;
        config.preset = preset;
        let now = Instant::now();
        previews.push(ActivePreview {
            request_serial: desired.serial,
            config,
            policy: desired.policy,
            cadence_phase: 0,
            session,
            frame,
            window,
            width: PREVIEW_WIDTH,
            height: PREVIEW_HEIGHT,
            resize_retry_width: 0,
            resize_retry_height: 0,
            resize_retry_at: now,
            started: now,
            next_render: now,
            static_needs_publish: true,
            extra_surfaces: Vec::new(),
            particle_craft: None,
            font_stamp: None,
            font_rush: None,
            exclusive_admission: None,
            metrics: GpgpuPreviewMetrics::default(),
        });
    }
    Ok(previews)
}

fn abandon_compute_preview_initialization(session: WindowSessionId, previews: &[ActivePreview]) {
    let _ = finish_window_session(PREVIEW_OWNER, session);
    for preview in previews {
        let _ = destroy_frame(preview.frame);
    }
}

fn initialize_cpp_font_rush_set(
    desired: DesiredPreview,
) -> Result<Vec<ActivePreview>, &'static str> {
    // The shell-side observation is only an early rejection. This reservation
    // atomically proves the producer set idle and blocks all ordinary frame or
    // session creation for the complete Rush lifetime.
    let admission = acquire_cpp_font_rush_ui4_admission("initialize")?;
    let (output, topology) = cpp_font_rush_topology()?;
    log_cpp_font_rush_capabilities("start", 1, topology);
    let session = super::window_broker::begin_window_session_with_exclusive_admission(
        PREVIEW_OWNER,
        &admission,
    )
    .map_err(|_| "font-rush-session-create-failed")?;
    let started = Instant::now();
    let mut preview = match create_cpp_font_rush_preview(
        desired, output, session, topology, 0, started, started, &admission,
    ) {
        Ok(preview) => preview,
        Err(reason) => {
            let _ = finish_window_session(PREVIEW_OWNER, session);
            return Err(reason);
        }
    };
    preview.exclusive_admission = Some(admission);
    Ok(alloc::vec![preview])
}

fn create_cpp_font_rush_preview(
    desired: DesiredPreview,
    output: OutputId,
    session: WindowSessionId,
    topology: CppFontRushTopology,
    rank: u8,
    started: Instant,
    activated: Instant,
    admission: &super::Ui4ExclusiveResourceAdmission,
) -> Result<ActivePreview, &'static str> {
    if rank >= topology.plane_count {
        return Err("font-rush-plane-unsupported");
    }
    let frame =
        create_cpp_font_rush_frame(output, topology.width, topology.height, rank, admission)
            .map_err(preview_frame_create_error_label)?;
    let plane = if rank == 0 {
        WindowPlane::Primary
    } else {
        WindowPlane::Universal(rank)
    };
    let window = match create_window(WindowCreate {
        owner: PREVIEW_OWNER,
        session,
        frame,
        output,
        plane,
        placement: WindowPlacement {
            x: 0,
            y: 0,
            width: topology.width,
            height: topology.height,
            z: PREVIEW_Z.saturating_add(i32::from(rank)),
            opacity: u8::MAX,
            visible: true,
        },
        interaction: super::WindowInteraction {
            movable: false,
            maximizable: false,
            receives_input: false,
            resize_on_maximize: false,
        },
    }) {
        Ok(window) => window,
        Err(_) => {
            let _ = destroy_frame(frame);
            return Err("font-rush-window-create-failed");
        }
    };
    Ok(ActivePreview {
        request_serial: desired.serial,
        config: desired.config,
        policy: desired.policy,
        cadence_phase: 0,
        session,
        frame,
        window,
        width: topology.width,
        height: topology.height,
        resize_retry_width: 0,
        resize_retry_height: 0,
        resize_retry_at: started,
        started,
        // Each newly added plane is an independent consumer whose cadence
        // begins when that consumer joins, not at Slot0's older start time.
        next_render: activated,
        static_needs_publish: true,
        extra_surfaces: Vec::new(),
        particle_craft: None,
        font_stamp: None,
        font_rush: Some(CppFontRushPlaneState {
            rank,
            topology,
            growth_started: None,
            rng: crate::tyche::soft_rng(),
            pending: None,
            scanout_pending: None,
        }),
        exclusive_admission: None,
        metrics: GpgpuPreviewMetrics::default(),
    })
}

fn grow_cpp_font_rush(previews: &mut Vec<ActivePreview>, now: Instant) -> Result<(), &'static str> {
    let Some(first) = previews.first() else {
        return Ok(());
    };
    if first.config.preset != GpgpuPreviewPreset::CppFontRush {
        return Ok(());
    }
    let state = first
        .font_rush
        .as_ref()
        .ok_or("font-rush-plane-state-missing")?;
    let Some(growth_started) = state.growth_started else {
        return Ok(());
    };
    let elapsed_ms = now.saturating_duration_since(growth_started).as_millis();
    let threshold_planes = cpp_font_rush_target_plane_count(elapsed_ms, state.topology.plane_count);
    if previews.len() >= threshold_planes
        || previews.len() >= usize::from(state.topology.plane_count)
    {
        return Ok(());
    }

    // Read the hardware-neutral descriptor again at every growth boundary.
    // If a future display owner publishes fewer usable application planes,
    // Rush simply stops growing at that descriptor instead of reaching into a
    // driver-specific engine table.
    let (output, topology) = cpp_font_rush_topology()?;
    let target_planes = cpp_font_rush_target_plane_count(elapsed_ms, topology.plane_count);
    if previews.len() >= target_planes {
        return Ok(());
    }
    if topology.width != state.topology.width || topology.height != state.topology.height {
        return Err("font-rush-output-topology-changed");
    }

    let desired = DesiredPreview {
        serial: first.request_serial,
        running: true,
        config: first.config,
        policy: first.policy,
    };
    let session = first.session;
    let started = first.started;
    while previews.len() < target_planes {
        let rank = previews.len() as u8;
        log_cpp_font_rush_capabilities("expand", previews.len(), topology);
        let admission = previews
            .first()
            .and_then(|preview| preview.exclusive_admission.as_ref())
            .ok_or("font-rush-exclusive-admission-missing")?;
        let preview = create_cpp_font_rush_preview(
            desired, output, session, topology, rank, started, now, admission,
        )?;
        crate::log_info!(
            target: "ui4";
            "ui4 cpp-font-rush plane added request={} elapsed_ms={} rank={} slot={} frame={} window={} active_planes={} usable_planes={} application_plane_mask=0x{:02X} glyphs={} grid={}x{} cadence_ms={}\n",
            desired.serial,
            elapsed_ms,
            rank,
            rank,
            preview.frame.raw(),
            preview.window.raw(),
            previews.len() + 1,
            topology.plane_count,
            topology.plane_mask,
            cpp_font_rush_glyph_count(rank),
            cpp_font_rush_grid(rank).0,
            cpp_font_rush_grid(rank).1,
            desired.config.cadence_ms,
        );
        previews.push(preview);
    }
    Ok(())
}

fn initialize_preview(desired: DesiredPreview) -> Result<ActivePreview, &'static str> {
    if desired.config.preset == GpgpuPreviewPreset::CppFont {
        return initialize_cpp_font_preview(desired);
    }
    if desired.config.preset == GpgpuPreviewPreset::Static30 {
        return initialize_static30_preview(desired);
    }
    let output = OutputId::from_slot(0).ok_or("output-d01-unavailable")?;
    let (width, height) = preview_extent(desired.config.preset);
    let frame =
        create_preview_frame(output, width, height).map_err(preview_frame_create_error_label)?;
    let session = match begin_window_session(PREVIEW_OWNER) {
        Ok(session) => session,
        Err(_) => {
            let _ = destroy_frame(frame);
            return Err("window-session-create-failed");
        }
    };
    let (scanout_width, _) = crate::intel::active_scanout_dimensions().unwrap_or((width, height));
    let x = scanout_width.saturating_sub(width.saturating_add(PREVIEW_MARGIN)) as i32;
    let window = match create_window(WindowCreate {
        owner: PREVIEW_OWNER,
        session,
        frame,
        output,
        // A sole opaque static or released compute window on its assigned
        // slot uses UI4's direct GGTT-import + hardware-plane-flip path.
        // SURFLIVE remains the consumer-side ownership boundary.
        plane: preview_plane(desired.config.preset),
        placement: WindowPlacement {
            x,
            y: PREVIEW_MARGIN as i32,
            width,
            height,
            z: PREVIEW_Z,
            opacity: u8::MAX,
            visible: true,
        },
        // Every C++ demo consumes UI4's maximize/restore extent notification
        // and replaces its double-buffer frame. Other probes retain their
        // proven fixed-size placement behavior.
        interaction: if desired.config.preset.is_resizable_cpp() {
            super::WindowInteraction::APPLICATION
        } else {
            super::WindowInteraction::MOVABLE_FRAME
        },
    }) {
        Ok(window) => window,
        Err(_) => {
            let _ = finish_window_session(PREVIEW_OWNER, session);
            let _ = destroy_frame(frame);
            return Err("window-create-failed");
        }
    };
    let now = Instant::now();
    Ok(ActivePreview {
        request_serial: desired.serial,
        config: desired.config,
        policy: desired.policy,
        cadence_phase: 0,
        session,
        frame,
        window,
        width,
        height,
        resize_retry_width: 0,
        resize_retry_height: 0,
        resize_retry_at: now,
        started: now,
        next_render: now,
        static_needs_publish: true,
        extra_surfaces: Vec::new(),
        particle_craft: None,
        font_stamp: None,
        font_rush: None,
        exclusive_admission: None,
        metrics: GpgpuPreviewMetrics::default(),
    })
}

fn initialize_cpp_font_preview(desired: DesiredPreview) -> Result<ActivePreview, &'static str> {
    let request = {
        let mut queued = CPP_FONT_REQUEST.lock();
        match queued.take() {
            Some((serial, request)) if serial == desired.serial => request,
            Some(stale) => {
                *queued = Some(stale);
                return Err("font-preview-request-serial-mismatch");
            }
            None => return Err("font-preview-request-missing"),
        }
    };
    let scene = &request.layers[0].scene;
    let (width, height) = (scene.raster_width, scene.raster_height);
    let output = OutputId::from_slot(0).ok_or("output-d01-unavailable")?;
    let frame = create_frame(FrameSpec {
        output,
        content: FrameContent::FontScene2d,
        cadence: FrameCadence::Immutable,
        buffering: super::FrameBuffering::Single,
        format: ScanoutFormat::Rgba8888Premultiplied,
        width,
        height,
        base_color: Some(PremultipliedRgba8::from_straight_rgba(10, 14, 24, u8::MAX)),
    })
    .map_err(preview_frame_create_error_label)?;
    let session = match begin_window_session(PREVIEW_OWNER) {
        Ok(session) => session,
        Err(_) => {
            let _ = destroy_frame(frame);
            return Err("font-preview-window-session-create-failed");
        }
    };
    let (scanout_width, _) = crate::intel::active_scanout_dimensions().unwrap_or((width, height));
    let x = scanout_width.saturating_sub(width.saturating_add(PREVIEW_MARGIN)) as i32;
    let window = match create_window(WindowCreate {
        owner: PREVIEW_OWNER,
        session,
        frame,
        output,
        plane: preview_plane(GpgpuPreviewPreset::CppFont),
        placement: WindowPlacement {
            x,
            y: PREVIEW_MARGIN as i32,
            width,
            height,
            z: PREVIEW_Z,
            opacity: u8::MAX,
            visible: true,
        },
        interaction: super::WindowInteraction::MOVABLE_FRAME,
    }) {
        Ok(window) => window,
        Err(_) => {
            let _ = finish_window_session(PREVIEW_OWNER, session);
            let _ = destroy_frame(frame);
            return Err("font-preview-window-create-failed");
        }
    };
    let now = Instant::now();
    Ok(ActivePreview {
        request_serial: desired.serial,
        config: desired.config,
        policy: desired.policy,
        cadence_phase: 0,
        session,
        frame,
        window,
        width,
        height,
        resize_retry_width: 0,
        resize_retry_height: 0,
        resize_retry_at: now,
        started: now,
        next_render: now,
        static_needs_publish: true,
        extra_surfaces: Vec::new(),
        particle_craft: None,
        font_stamp: Some(request),
        font_rush: None,
        exclusive_admission: None,
        metrics: GpgpuPreviewMetrics::default(),
    })
}

fn initialize_static30_preview(desired: DesiredPreview) -> Result<ActivePreview, &'static str> {
    let output = OutputId::from_slot(0).ok_or("output-d01-unavailable")?;
    let (output_width, output_height) = crate::intel::active_scanout_dimensions()
        .unwrap_or((PREVIEW_WIDTH.saturating_mul(2), PREVIEW_HEIGHT.saturating_mul(2)));
    let cell_width = (output_width / STATIC30_COLUMNS).max(1);
    let cell_height = (output_height / STATIC30_ROWS).max(1);
    let mut frames = Vec::with_capacity(STATIC30_FRAME_COUNT);
    let mut surfaces = Vec::with_capacity(STATIC30_FRAME_COUNT);
    let session =
        begin_window_session(PREVIEW_OWNER).map_err(|_| "static30-session-create-failed")?;

    for index in 0..STATIC30_FRAME_COUNT {
        let index_u32 = index as u32;
        let plane_slot = index % STATIC30_PLANE_COUNT + 1;
        let plane_offset = plane_slot as u32 * 4;
        let inset = 8u32.saturating_add(plane_offset);
        let width = cell_width
            .saturating_sub(inset.saturating_mul(2))
            .min(STATIC30_MAX_WIDTH)
            .max(1);
        let height = cell_height
            .saturating_sub(inset.saturating_mul(2))
            .min(STATIC30_MAX_HEIGHT)
            .max(1);
        let x = (index_u32 % STATIC30_COLUMNS)
            .saturating_mul(cell_width)
            .saturating_add(inset);
        let y = (index_u32 / STATIC30_COLUMNS)
            .saturating_mul(cell_height)
            .saturating_add(inset);
        let frame = match create_static30_frame(output, width, height, index as u8) {
            Ok(frame) => frame,
            Err(_) => {
                abandon_static30_initialization(session, &frames);
                return Err("static30-frame-create-failed");
            }
        };
        frames.push(frame);
        let window = match create_window(WindowCreate {
            owner: PREVIEW_OWNER,
            session,
            frame,
            output,
            plane: WindowPlane::Universal(plane_slot as u8),
            placement: WindowPlacement {
                x: x as i32,
                y: y as i32,
                width,
                height,
                z: PREVIEW_Z.saturating_add(index as i32),
                opacity: u8::MAX,
                visible: true,
            },
            interaction: super::WindowInteraction::MOVABLE_FRAME,
        }) {
            Ok(window) => window,
            Err(error) => {
                crate::log_warn!(
                    target: "ui4";
                    "ui4 gpgpu-preview static30 window admission failed index={} plane_slot={} session={} active_frames={} error={:?}\n",
                    index,
                    plane_slot,
                    session.raw(),
                    frames.len(),
                    error,
                );
                abandon_static30_initialization(session, &frames);
                return Err("static30-window-create-failed");
            }
        };
        surfaces.push(StaticPreviewSurface {
            frame,
            window,
            scheme: index as u8,
        });
    }

    let first = surfaces[0];
    let now = Instant::now();
    Ok(ActivePreview {
        request_serial: desired.serial,
        config: desired.config,
        policy: desired.policy,
        cadence_phase: 0,
        session,
        frame: first.frame,
        window: first.window,
        width: cell_width.saturating_sub(24).min(STATIC30_MAX_WIDTH).max(1),
        height: cell_height
            .saturating_sub(24)
            .min(STATIC30_MAX_HEIGHT)
            .max(1),
        resize_retry_width: 0,
        resize_retry_height: 0,
        resize_retry_at: now,
        started: now,
        next_render: now,
        static_needs_publish: true,
        extra_surfaces: surfaces.iter().copied().skip(1).collect(),
        particle_craft: None,
        font_stamp: None,
        font_rush: None,
        exclusive_admission: None,
        metrics: GpgpuPreviewMetrics::default(),
    })
}

fn abandon_static30_initialization(session: WindowSessionId, frames: &[FrameHandle]) {
    let _ = finish_window_session(PREVIEW_OWNER, session);
    for frame in frames.iter().copied() {
        let _ = destroy_frame(frame);
    }
}

const fn preview_frame_create_error_label(error: FramePoolError) -> &'static str {
    match error {
        FramePoolError::InvalidPlan(FramePlanError::EmptyExtent) => {
            "frame-create-invalid-plan-empty-extent"
        }
        FramePoolError::InvalidPlan(FramePlanError::BaseColorRequiresPremultipliedRgba) => {
            "frame-create-invalid-plan-base-color-format"
        }
        FramePoolError::InvalidPlan(FramePlanError::VideoRequiresPremultipliedRgba) => {
            "frame-create-invalid-plan-video-format"
        }
        FramePoolError::InvalidPlan(FramePlanError::VideoRequiresStreamingCadence) => {
            "frame-create-invalid-plan-video-cadence"
        }
        FramePoolError::InvalidPlan(FramePlanError::VideoRequiresQuadBuffering) => {
            "frame-create-invalid-plan-video-buffering"
        }
        FramePoolError::InvalidPlan(FramePlanError::VideoExceedsPixelSoftCap) => {
            "frame-create-invalid-plan-video-extent"
        }
        FramePoolError::InvalidPlan(FramePlanError::RenderSceneRequiresPremultipliedRgba) => {
            "frame-create-invalid-plan-render-scene-format"
        }
        FramePoolError::InvalidPlan(FramePlanError::RenderSceneRequiresStreamingCadence) => {
            "frame-create-invalid-plan-render-scene-cadence"
        }
        FramePoolError::InvalidPlan(FramePlanError::RenderSceneRequiresTripleBuffering) => {
            "frame-create-invalid-plan-render-scene-buffering"
        }
        FramePoolError::InvalidHandle => "frame-create-invalid-handle",
        FramePoolError::UnsupportedFormat => "frame-create-unsupported-format",
        FramePoolError::OutOfMemory => "frame-create-out-of-memory",
        FramePoolError::Busy => "frame-create-busy",
        FramePoolError::ImmutablePublished => "frame-create-immutable-published",
        FramePoolError::NotPublished => "frame-create-not-published",
        FramePoolError::InvalidLease => "frame-create-invalid-lease",
        FramePoolError::ProducerReleaseRequired => "frame-create-producer-release-required",
    }
}

fn create_preview_frame(
    output: OutputId,
    width: u32,
    height: u32,
) -> Result<FrameHandle, FramePoolError> {
    create_frame(FrameSpec {
        output,
        content: FrameContent::Image,
        // Each compute dispatch waits for its completion marker before this
        // frame is published. Two buffers are therefore sufficient: the
        // published front and one producer back buffer, with Busy providing
        // backpressure while the compositor still owns the retired front.
        cadence: FrameCadence::Dirty,
        buffering: super::FrameBuffering::Double,
        format: ScanoutFormat::Rgba8888Premultiplied,
        width,
        height,
        base_color: Some(PremultipliedRgba8::from_straight_rgba(0, 0, 0, u8::MAX)),
    })
}

fn create_static30_frame(
    output: OutputId,
    width: u32,
    height: u32,
    scheme: u8,
) -> Result<FrameHandle, FramePoolError> {
    let seed = u16::from(scheme);
    create_frame(FrameSpec {
        output,
        content: FrameContent::FontScene2d,
        // Every Lorem Ipsum canvas is stamped and published exactly once. A
        // single buffer keeps the full lease/release contract honest while
        // avoiding redundant storage across the 30-window probe.
        cadence: FrameCadence::Immutable,
        buffering: super::FrameBuffering::Single,
        format: ScanoutFormat::Rgba8888Premultiplied,
        width,
        height,
        base_color: Some(PremultipliedRgba8::from_straight_rgba(
            (10 + seed * 7 % 28) as u8,
            (14 + seed * 11 % 32) as u8,
            (28 + seed * 13 % 44) as u8,
            u8::MAX,
        )),
    })
}

fn create_cpp_font_rush_frame(
    output: OutputId,
    width: u32,
    height: u32,
    rank: u8,
    admission: &super::Ui4ExclusiveResourceAdmission,
) -> Result<FrameHandle, FramePoolError> {
    super::frame_pool::create_frame_with_exclusive_admission(
        FrameSpec {
            output,
            content: FrameContent::FontScene2d,
            cadence: FrameCadence::Dirty,
            buffering: super::FrameBuffering::Double,
            format: ScanoutFormat::Rgba8888Premultiplied,
            width,
            height,
            base_color: Some(cpp_font_rush_background(rank, 0)),
        },
        admission,
    )
}

async fn render_preview_frame(preview: &mut ActivePreview) -> Result<(), &'static str> {
    if preview.config.preset == GpgpuPreviewPreset::CppFont {
        return render_cpp_font_frame(preview).await;
    }
    if preview.config.preset == GpgpuPreviewPreset::CppFontRush {
        return Err("font-rush-independent-controller-required");
    }
    if preview.config.preset == GpgpuPreviewPreset::Static30 {
        return render_static30_frames(preview).await;
    }
    preview.metrics.attempted = preview.metrics.attempted.saturating_add(1);
    let publish_this_frame =
        (preview.metrics.attempted - 1) % u64::from(preview.config.publish_every) == 0;
    let lease = match acquire_frame_buffer(preview.frame) {
        Ok(lease) => lease,
        Err(FramePoolError::Busy) => {
            preview.metrics.dropped_busy = preview.metrics.dropped_busy.saturating_add(1);
            return Ok(());
        }
        Err(_) => {
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            return Err("frame-acquire-failed");
        }
    };
    let mut gpu_release = None;
    if preview.config.preset == GpgpuPreviewPreset::Static {
        if let Err(reason) = fill_static_preview_frame(lease, 0) {
            let _ = cancel_frame_buffer(lease);
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            return Err(reason);
        }
    } else {
        let surface = match gpgpu_rgba_surface(lease) {
            Ok(surface) => surface,
            Err(_) => {
                let _ = cancel_frame_buffer(lease);
                preview.metrics.failed = preview.metrics.failed.saturating_add(1);
                return Err("gpgpu-surface-unavailable");
            }
        };

        let result = dispatch_preview_kernel(preview, surface);
        gpu_release = result.release;
        preview.metrics.last_iterations = result.iterations;
        preview.metrics.last_marker = result.marker;
        if result.submitted {
            preview.metrics.submitted = preview.metrics.submitted.saturating_add(1);
        }
        preview.metrics.last_submit_ms = result.submit_ms;
        if !result.ok {
            if result.submitted {
                // A submitted producer owns this exact back buffer until its
                // retirement marker is observed. On a genuine timeout we
                // quarantine the write lease by leaving it acquired; making
                // the surface reusable would permit late GPU writes into a
                // future frame.
                crate::log_error!(
                    target: "ui4";
                    "ui4 gpgpu-preview producer quarantine request={} preset={} frame={} buffer={} observed=0x{:08X} reason=submit-accepted-marker-timeout action=retain-write-lease-no-reuse\n",
                    preview.request_serial,
                    preview.config.preset.label(),
                    lease.frame.raw(),
                    lease.buffer_index,
                    result.marker,
                );
            } else {
                let _ = cancel_frame_buffer(lease);
            }
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            return Err(result.error);
        }
    }
    preview.metrics.completed = preview.metrics.completed.saturating_add(1);

    if !publish_this_frame {
        let _ = cancel_frame_buffer(lease);
        return Ok(());
    }
    let publish_result = match preview.config.preset {
        GpgpuPreviewPreset::All
        | GpgpuPreviewPreset::Mandelbrot
        | GpgpuPreviewPreset::Chart
        | GpgpuPreviewPreset::Plasma
        | GpgpuPreviewPreset::Lab256
        | GpgpuPreviewPreset::CppGallery
        | GpgpuPreviewPreset::CppAurora
        | GpgpuPreviewPreset::CppJulia
        | GpgpuPreviewPreset::CppSdf
        | GpgpuPreviewPreset::CppVoronoi
        | GpgpuPreviewPreset::CppRetroSun
        | GpgpuPreviewPreset::CppAudio
        | GpgpuPreviewPreset::CppParticle => match gpu_release {
            Some(release) => publish_gpgpu_frame_buffer(lease, release),
            None => Err(FramePoolError::ProducerReleaseRequired),
        },
        GpgpuPreviewPreset::Static
        | GpgpuPreviewPreset::Static30
        | GpgpuPreviewPreset::CppFont
        | GpgpuPreviewPreset::CppFontRush => publish_frame_buffer(lease),
    };
    let published = match publish_result {
        Ok(published) => published,
        Err(_) => {
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            return Err("frame-publish-failed");
        }
    };
    if publish_window_frame(PREVIEW_OWNER, preview.window, DamageRect::FULL).is_err() {
        preview.metrics.failed = preview.metrics.failed.saturating_add(1);
        return Err("window-publish-failed");
    }
    preview.metrics.published = preview.metrics.published.saturating_add(1);
    if preview.policy.frame_limit != 0 && preview.metrics.published == preview.policy.frame_limit {
        crate::log_info!(
            target: "ui4";
            "ui4 gpgpu-preview frame-limit reached request={} preset={} published={} action=hold-last-frame producer=unchanged display_release=surflive\n",
            preview.request_serial,
            preview.config.preset.label(),
            preview.metrics.published,
        );
    }
    if !matches!(preview.config.preset, GpgpuPreviewPreset::Static | GpgpuPreviewPreset::Static30)
        && should_log_preview_checkpoint(preview.metrics.published)
    {
        crate::log_info!(
            target: "ui4";
            "ui4 gpgpu-preview frame-ready request={} preset={} frame={} buffer={} publish_serial={} marker=0x{:08X} producer_release={} consumer={} compositor_backpressure=read-lease buffering=double published={} dropped_busy={}\n",
            preview.request_serial,
            preview.config.preset.label(),
            published.frame.raw(),
            published.buffer_index,
            published.publish_serial,
            preview.metrics.last_marker,
            preview_release_label(preview.config.preset),
            preview_consumer_label(preview.config.preset),
            preview.metrics.published,
            preview.metrics.dropped_busy,
        );
    }
    if preview.config.preset == GpgpuPreviewPreset::Static {
        preview.static_needs_publish = false;
        crate::log_info!(
            target: "ui4";
            "ui4 gpgpu-preview static frame published request={} frame={} window={} submitted=0 marker=0 producer=cpu release=clflush-mfence\n",
            preview.request_serial,
            preview.frame.raw(),
            preview.window.raw(),
        );
    }
    Ok(())
}

async fn render_cpp_font_frame(preview: &mut ActivePreview) -> Result<(), &'static str> {
    preview.metrics.attempted = preview.metrics.attempted.saturating_add(1);
    let lease = match acquire_frame_buffer(preview.frame) {
        Ok(lease) => lease,
        Err(FramePoolError::Busy) => {
            preview.metrics.dropped_busy = preview.metrics.dropped_busy.saturating_add(1);
            return Ok(());
        }
        Err(_) => {
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            return Err("font-preview-frame-acquire-failed");
        }
    };
    let destination = match gpgpu_rgba_surface(lease) {
        Ok(destination) => destination,
        Err(_) => {
            let _ = cancel_frame_buffer(lease);
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            return Err("font-preview-surface-unavailable");
        }
    };
    let Some(request) = preview.font_stamp.take() else {
        let _ = cancel_frame_buffer(lease);
        preview.metrics.failed = preview.metrics.failed.saturating_add(1);
        return Err("font-preview-request-consumed");
    };
    let pending =
        match crate::r::font_kernel_service::submit_frame_stamp(request.clone(), destination) {
            Ok(pending) => pending,
            Err(crate::r::font_kernel_service::FontKernelError::QueueFull) => {
                preview.font_stamp = Some(request);
                let _ = cancel_frame_buffer(lease);
                preview.metrics.dropped_busy = preview.metrics.dropped_busy.saturating_add(1);
                return Ok(());
            }
            Err(_) => {
                let _ = cancel_frame_buffer(lease);
                preview.metrics.failed = preview.metrics.failed.saturating_add(1);
                return Err("font-preview-submit-failed");
            }
        };
    let stamped = match pending.wait().await {
        Ok(stamped) => stamped,
        Err(crate::r::font_kernel_service::FontKernelError::SubmittedIncomplete(_)) => {
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            return Err("font-preview-submit-incomplete");
        }
        Err(_) => {
            let _ = cancel_frame_buffer(lease);
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            return Err("font-preview-stamp-failed");
        }
    };
    preview.metrics.submitted = preview.metrics.submitted.saturating_add(1);
    preview.metrics.completed = preview.metrics.completed.saturating_add(1);
    if publish_gpu_font_frame_buffer(lease, stamped.release()).is_err() {
        preview.metrics.failed = preview.metrics.failed.saturating_add(1);
        return Err("font-preview-frame-publish-failed");
    }
    if publish_window_frame(PREVIEW_OWNER, preview.window, DamageRect::FULL).is_err() {
        preview.metrics.failed = preview.metrics.failed.saturating_add(1);
        return Err("font-preview-window-publish-failed");
    }
    preview.metrics.published = preview.metrics.published.saturating_add(1);
    preview.metrics.last_iterations = stamped.glyphs() as u32;
    preview.metrics.last_marker = stamped.release().sequence() as u32;
    preview.static_needs_publish = false;
    crate::log_info!(
        target: "ui4";
        "ui4 cpp-font preview ready request={} frame={} window={} extent={}x{} glyphs={} submits={} walkers={} release={} path=skrifa->gpu-vm-r8->cpp-igc->guc-rcs->ui4-font-scene\n",
        preview.request_serial,
        preview.frame.raw(),
        preview.window.raw(),
        preview.width,
        preview.height,
        stamped.glyphs(),
        stamped.submits(),
        stamped.active_walkers(),
        stamped.release().sequence(),
    );
    Ok(())
}

fn poll_cpp_font_rush_consumers(previews: &mut [ActivePreview]) -> Result<(), &'static str> {
    for preview in previews {
        poll_cpp_font_rush_scanout(preview)?;
        poll_cpp_font_rush_frame(preview)?;
    }
    Ok(())
}

fn poll_cpp_font_rush_scanout(preview: &mut ActivePreview) -> Result<(), &'static str> {
    let state = preview
        .font_rush
        .as_mut()
        .ok_or("font-rush-plane-state-missing")?;
    let Some(pending) = state.scanout_pending else {
        return Ok(());
    };
    let ready_serial = crate::intel::ui4_direct_scanout_ready_for_frame(preview.frame.raw());
    if !ready_serial.is_some_and(|serial| serial >= pending.frame_publish_serial) {
        return Ok(());
    }
    state.scanout_pending = None;
    preview.metrics.scanout_live = preview.metrics.scanout_live.saturating_add(1);
    crate::log_info!(
        target: "ui4";
        "ui4 cpp-font-rush scanout-live request={} rank={} slot={} sequence={} ticket={} frame={} producer_buffer={} frame_publish_serial={} window_publish_serial={} release={} glyph_hash=0x{:016X} publish_to_surflive_us={} proof_serial={} path=ui4-font-scene->display-plane-direct compositor_jobs=0\n",
        preview.request_serial,
        state.rank,
        state.rank,
        pending.sequence,
        pending.ticket.raw(),
        preview.frame.raw(),
        pending.producer_buffer,
        pending.frame_publish_serial,
        pending.window_publish_serial,
        pending.release_sequence,
        pending.glyph_fingerprint,
        Instant::now()
            .saturating_duration_since(pending.published_at)
            .as_micros(),
        ready_serial.unwrap_or(0),
    );
    Ok(())
}

fn poll_cpp_font_rush_frame(preview: &mut ActivePreview) -> Result<(), &'static str> {
    use crate::r::font_kernel_service::FontKernelError;

    let completion = {
        let state = preview
            .font_rush
            .as_mut()
            .ok_or("font-rush-plane-state-missing")?;
        state
            .pending
            .as_mut()
            .and_then(|pending| pending.completion.try_take())
    };
    let Some(completion) = completion else {
        return Ok(());
    };
    let pending = preview
        .font_rush
        .as_mut()
        .and_then(|state| state.pending.take())
        .ok_or("font-rush-pending-state-missing")?;
    let completed_at = Instant::now();
    let stamped = match completion {
        Ok(stamped) => stamped,
        Err(FontKernelError::SubmittedIncomplete(reason)) => {
            // The worker may still reference this exact allocation. Removing
            // the state intentionally leaves its frame write lease acquired,
            // quarantining the destination until reboot.
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            crate::log_error!(
                target: "ui4";
                "ui4 cpp-font-rush consumer quarantined request={} rank={} sequence={} ticket={} frame={} buffer={} reason={} action=retain-frame-write-lease\n",
                preview.request_serial,
                cpp_font_rush_rank(preview)?,
                pending.sequence,
                pending.ticket.raw(),
                pending.lease.frame.raw(),
                pending.lease.buffer_index,
                reason,
            );
            return Err("font-rush-submit-incomplete");
        }
        Err(error) => {
            let _ = cancel_frame_buffer(pending.lease);
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            crate::log_warn!(
                target: "ui4";
                "ui4 cpp-font-rush consumer failed request={} rank={} sequence={} ticket={} stage=font-service reason={:?} action=cancel-complete-write-lease\n",
                preview.request_serial,
                cpp_font_rush_rank(preview)?,
                pending.sequence,
                pending.ticket.raw(),
                error,
            );
            return Err("font-rush-stamp-failed");
        }
    };
    if stamped.ticket() != pending.ticket {
        let _ = cancel_frame_buffer(pending.lease);
        preview.metrics.failed = preview.metrics.failed.saturating_add(1);
        return Err("font-rush-ticket-mismatch");
    }
    preview.metrics.completed = preview.metrics.completed.saturating_add(1);
    let release = stamped.release();
    let published = match publish_gpu_font_frame_buffer(pending.lease, release) {
        Ok(published) => published,
        Err(_) => {
            let _ = cancel_frame_buffer(pending.lease);
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            return Err("font-rush-frame-publish-failed");
        }
    };
    let window_publish_serial =
        match publish_window_frame(PREVIEW_OWNER, preview.window, DamageRect::FULL) {
            Ok(serial) => serial,
            Err(_) => {
                preview.metrics.failed = preview.metrics.failed.saturating_add(1);
                return Err("font-rush-window-publish-failed");
            }
        };
    let published_at = Instant::now();
    preview.metrics.published = preview.metrics.published.saturating_add(1);
    preview.metrics.last_iterations = stamped.glyphs() as u32;
    preview.metrics.last_marker = release.sequence() as u32;
    preview.metrics.last_submit_ms = completed_at
        .saturating_duration_since(pending.submit_started_at)
        .as_millis();

    let (rank, topology, replaced_scanout) = {
        let state = preview
            .font_rush
            .as_mut()
            .ok_or("font-rush-plane-state-missing")?;
        if state.rank == 0 && preview.metrics.published == 1 {
            state.growth_started = Some(published_at);
        }
        let replaced = state.scanout_pending.replace(CppFontRushPendingScanout {
            ticket: pending.ticket,
            sequence: pending.sequence,
            producer_buffer: published.buffer_index,
            frame_publish_serial: published.publish_serial,
            window_publish_serial,
            release_sequence: release.sequence(),
            published_at,
            glyph_fingerprint: pending.glyph_fingerprint,
        });
        (state.rank, state.topology, replaced)
    };
    if let Some(replaced) = replaced_scanout {
        preview.metrics.scanout_superseded = preview.metrics.scanout_superseded.saturating_add(1);
        crate::log_warn!(
            target: "ui4";
            "ui4 cpp-font-rush scanout proof superseded request={} rank={} old_sequence={} old_frame_publish_serial={} new_sequence={} new_frame_publish_serial={} scanout_superseded={} action=count-present-drop-candidate\n",
            preview.request_serial,
            rank,
            replaced.sequence,
            replaced.frame_publish_serial,
            pending.sequence,
            published.publish_serial,
            preview.metrics.scanout_superseded,
        );
    }
    let service = crate::r::font_kernel_service::status();
    crate::log_info!(
        target: "ui4";
        "ui4 cpp-font-rush frame-ready request={} consumer={} rank={} slot={} sequence={} publication={} ticket={} elapsed_ms={} scheduled_ms={} deadline_to_submit_ms={} submit_call_us={} request_build_ms={} font_wait_ms={} pre_service_ms={} clear_ms={} prepare_coverage_ms={} coverage_build_ms={} coverage_audit_ms={} instance_release_ms={} service_ms={} fifo_queued_ahead={} service_queue_at_complete={} completion_to_publish_us={} frame={} producer_buffer={} frame_publish_serial={} window_publish_serial={} glyph_hash=0x{:016X} glyph_ids={} extent={}x{} cadence_ms={} requested_glyphs={} rendered_glyphs={} grid={}x{} font=0 application_plane_mask=0x{:02X} usable_planes={} clear_submits={} coverage_submits={} instance_release_submits={} known_gpu_submits={} walkers={} release={} consumer_in_flight=0 path=font-service-fifo[gpu-clear->skrifa->gpu-vm-r8->coverage-audit->cpp-igc->guc-rcs]->ui4-font-scene->display-plane-direct compositor_jobs=0 rgba_cpu_readback=0 coverage_audit_cpu_readback=1 cpu_frame_copy=0\n",
        preview.request_serial,
        rank,
        rank,
        rank,
        pending.sequence,
        preview.metrics.published,
        pending.ticket.raw(),
        completed_at.saturating_duration_since(preview.started).as_millis(),
        pending.scheduled_at.saturating_duration_since(preview.started).as_millis(),
        pending
            .submit_started_at
            .saturating_duration_since(pending.scheduled_at)
            .as_millis(),
        pending
            .accepted_at
            .saturating_duration_since(pending.submit_started_at)
            .as_micros(),
        pending.request_build_ms,
        completed_at
            .saturating_duration_since(pending.submit_started_at)
            .as_millis(),
        stamped.pre_service_ms(),
        stamped.clear_ms(),
        stamped.prepare_coverage_ms(),
        stamped.coverage_build_ms(),
        stamped.coverage_audit_ms(),
        stamped.instance_release_ms(),
        stamped.total_service_ms(),
        pending.fifo_queued_ahead,
        service.queued,
        published_at
            .saturating_duration_since(completed_at)
            .as_micros(),
        preview.frame.raw(),
        published.buffer_index,
        published.publish_serial,
        window_publish_serial,
        pending.glyph_fingerprint,
        pending.glyph_ids,
        preview.width,
        preview.height,
        preview.config.cadence_ms,
        pending.requested_glyphs,
        stamped.glyphs(),
        pending.columns,
        pending.rows,
        topology.plane_mask,
        topology.plane_count,
        stamped.clear_submits(),
        stamped.coverage_submits(),
        stamped.submits(),
        stamped
            .clear_submits()
            .saturating_add(stamped.coverage_submits())
            .saturating_add(stamped.submits()),
        stamped.active_walkers(),
        release.sequence(),
    );
    Ok(())
}

fn queue_due_cpp_font_rush_consumers(
    previews: &mut [ActivePreview],
    now: Instant,
) -> Result<(), &'static str> {
    for preview in previews {
        if !preview_needs_render(preview) {
            continue;
        }
        let Some((due_ticks, scheduled_at)) = take_cpp_font_rush_due_ticks(preview, now) else {
            continue;
        };
        preview.metrics.attempted = preview.metrics.attempted.saturating_add(due_ticks);
        let superseded = due_ticks.saturating_sub(1);
        if superseded != 0 {
            preview.metrics.dropped_busy = preview.metrics.dropped_busy.saturating_add(superseded);
            preview.metrics.dropped_cadence =
                preview.metrics.dropped_cadence.saturating_add(superseded);
            preview.metrics.late = preview.metrics.late.saturating_add(superseded);
            crate::log_warn!(
                target: "ui4";
                "ui4 cpp-font-rush consumer backpressure request={} rank={} slot={} sequence={} due_ticks={} drop=cadence-superseded dropped_intervals={} service_queued={} cadence_ms={} policy=keep-latest-tick+preserve-phase\n",
                preview.request_serial,
                cpp_font_rush_rank(preview)?,
                cpp_font_rush_rank(preview)?,
                preview.metrics.attempted,
                due_ticks,
                superseded,
                crate::r::font_kernel_service::status().queued,
                preview.config.cadence_ms,
            );
        }
        let sequence = preview.metrics.attempted;
        let pending = preview
            .font_rush
            .as_ref()
            .ok_or("font-rush-plane-state-missing")?
            .pending
            .as_ref();
        if let Some(pending) = pending {
            preview.metrics.dropped_busy = preview.metrics.dropped_busy.saturating_add(1);
            preview.metrics.dropped_in_flight = preview.metrics.dropped_in_flight.saturating_add(1);
            preview.metrics.late = preview.metrics.late.saturating_add(1);
            crate::log_warn!(
                target: "ui4";
                "ui4 cpp-font-rush consumer backpressure request={} rank={} slot={} sequence={} due_ticks={} drop=in-flight active_ticket={} active_sequence={} in_flight_ms={} service_queued={} cadence_ms={} next_deadline_ms={}\n",
                preview.request_serial,
                cpp_font_rush_rank(preview)?,
                cpp_font_rush_rank(preview)?,
                sequence,
                due_ticks,
                pending.ticket.raw(),
                pending.sequence,
                now.saturating_duration_since(pending.submit_started_at).as_millis(),
                crate::r::font_kernel_service::status().queued,
                preview.config.cadence_ms,
                preview.next_render.saturating_duration_since(preview.started).as_millis(),
            );
            continue;
        }
        queue_cpp_font_rush_frame(preview, sequence, scheduled_at)?;
    }
    Ok(())
}

fn queue_cpp_font_rush_frame(
    preview: &mut ActivePreview,
    sequence: u64,
    scheduled_at: Instant,
) -> Result<(), &'static str> {
    use crate::r::font_kernel_service::FontKernelError;

    let rank = cpp_font_rush_rank(preview)?;
    let lease = match acquire_frame_buffer(preview.frame) {
        Ok(lease) => lease,
        Err(FramePoolError::Busy) => {
            preview.metrics.dropped_busy = preview.metrics.dropped_busy.saturating_add(1);
            preview.metrics.dropped_frame_busy =
                preview.metrics.dropped_frame_busy.saturating_add(1);
            crate::log_warn!(
                target: "ui4";
                "ui4 cpp-font-rush consumer backpressure request={} rank={} slot={} sequence={} due_ticks=1 drop=frame-busy service_queued={} cadence_ms={}\n",
                preview.request_serial,
                rank,
                rank,
                sequence,
                crate::r::font_kernel_service::status().queued,
                preview.config.cadence_ms,
            );
            return Ok(());
        }
        Err(_) => {
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            return Err("font-rush-frame-acquire-failed");
        }
    };
    let destination = match gpgpu_rgba_surface(lease) {
        Ok(destination) => destination,
        Err(_) => {
            let _ = cancel_frame_buffer(lease);
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            return Err("font-rush-surface-unavailable");
        }
    };
    let request_build_started = Instant::now();
    let (request, requested_glyphs, (columns, rows)) = {
        let state = preview
            .font_rush
            .as_mut()
            .ok_or("font-rush-plane-state-missing")?;
        match cpp_font_rush_stamp_request(preview.width, preview.height, rank, &mut state.rng) {
            Ok(request) => request,
            Err(_) => {
                let _ = cancel_frame_buffer(lease);
                preview.metrics.failed = preview.metrics.failed.saturating_add(1);
                return Err("font-rush-glyph-admission-failed");
            }
        }
    };
    let glyph_fingerprint = cpp_font_rush_glyph_fingerprint(&request);
    let glyph_ids = cpp_font_rush_glyph_ids(&request);
    let request_build_ms = Instant::now()
        .saturating_duration_since(request_build_started)
        .as_millis();
    let clear_rgba = u32::from_le_bytes(cpp_font_rush_background(rank, sequence).to_native_bytes());
    let submit_started_at = Instant::now();
    let completion = match crate::r::font_kernel_service::submit_frame_stamp_with_clear(
        request,
        destination,
        clear_rgba,
    ) {
        Ok(pending) => pending,
        Err(FontKernelError::QueueFull) => {
            let _ = cancel_frame_buffer(lease);
            preview.metrics.dropped_busy = preview.metrics.dropped_busy.saturating_add(1);
            preview.metrics.dropped_queue_full =
                preview.metrics.dropped_queue_full.saturating_add(1);
            crate::log_warn!(
                target: "ui4";
                "ui4 cpp-font-rush consumer backpressure request={} rank={} slot={} sequence={} due_ticks=1 drop=font-queue-full service_queued={} cadence_ms={}\n",
                preview.request_serial,
                rank,
                rank,
                sequence,
                crate::r::font_kernel_service::status().queued,
                preview.config.cadence_ms,
            );
            return Ok(());
        }
        Err(_) => {
            let _ = cancel_frame_buffer(lease);
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            return Err("font-rush-submit-failed");
        }
    };
    let accepted_at = Instant::now();
    let ticket = completion.ticket();
    let fifo_queued_ahead = completion.queued_ahead();
    preview.metrics.submitted = preview.metrics.submitted.saturating_add(1);
    let state = preview
        .font_rush
        .as_mut()
        .ok_or("font-rush-plane-state-missing")?;
    state.pending = Some(CppFontRushPendingFrame {
        lease,
        completion,
        ticket,
        sequence,
        scheduled_at,
        submit_started_at,
        accepted_at,
        requested_glyphs,
        columns,
        rows,
        glyph_fingerprint,
        glyph_ids: glyph_ids.clone(),
        fifo_queued_ahead,
        request_build_ms,
    });
    crate::log_info!(
        target: "ui4";
        "ui4 cpp-font-rush consumer enqueued request={} consumer={} rank={} slot={} sequence={} ticket={} frame={} producer_buffer={} glyph_hash=0x{:016X} glyph_ids={} glyphs={} grid={}x{} scheduled_ms={} deadline_to_submit_ms={} submit_call_us={} request_build_ms={} fifo_queued_ahead={} consumer_in_flight=1 consumer_pending_limit=1 service_model=fifo-32+one-global-in-flight\n",
        preview.request_serial,
        rank,
        rank,
        rank,
        sequence,
        ticket.raw(),
        lease.frame.raw(),
        lease.buffer_index,
        glyph_fingerprint,
        glyph_ids,
        requested_glyphs,
        columns,
        rows,
        scheduled_at.saturating_duration_since(preview.started).as_millis(),
        submit_started_at
            .saturating_duration_since(scheduled_at)
            .as_millis(),
        accepted_at
            .saturating_duration_since(submit_started_at)
            .as_micros(),
        request_build_ms,
        fifo_queued_ahead,
    );
    Ok(())
}

fn take_cpp_font_rush_due_ticks(
    preview: &mut ActivePreview,
    now: Instant,
) -> Option<(u64, Instant)> {
    if now < preview.next_render {
        return None;
    }
    let period_ticks = Duration::from_millis(preview.config.cadence_ms)
        .as_ticks()
        .max(1);
    let next_tick = preview.next_render.as_ticks();
    let due_ticks = cpp_font_rush_due_tick_count(now.as_ticks(), next_tick, period_ticks);
    let latest_offset = period_ticks.saturating_mul(due_ticks.saturating_sub(1));
    let advance = period_ticks.saturating_mul(due_ticks);
    let scheduled_at = Instant::from_ticks(next_tick.saturating_add(latest_offset));
    preview.next_render = Instant::from_ticks(next_tick.saturating_add(advance));
    Some((due_ticks, scheduled_at))
}

const fn cpp_font_rush_due_tick_count(now_tick: u64, next_tick: u64, period_ticks: u64) -> u64 {
    if now_tick < next_tick || period_ticks == 0 {
        return 0;
    }
    now_tick
        .saturating_sub(next_tick)
        .saturating_div(period_ticks)
        .saturating_add(1)
}

fn cpp_font_rush_rank(preview: &ActivePreview) -> Result<u8, &'static str> {
    preview
        .font_rush
        .as_ref()
        .map(|state| state.rank)
        .ok_or("font-rush-plane-state-missing")
}

fn cpp_font_rush_glyph_fingerprint(
    request: &crate::r::font_kernel_service::FontStampRequest,
) -> u64 {
    let mut hash = 0xCBF2_9CE4_8422_2325u64;
    for (layer_index, layer) in request.layers.iter().enumerate() {
        hash ^= layer_index as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
        for (run_index, run) in layer.scene.runs.iter().enumerate() {
            hash ^= run_index as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
            for ch in run.text.chars() {
                hash ^= u64::from(ch as u32);
                hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
            }
        }
    }
    hash
}

fn cpp_font_rush_glyph_ids(request: &crate::r::font_kernel_service::FontStampRequest) -> String {
    let mut ids = String::new();
    for layer in &request.layers {
        for run in &layer.scene.runs {
            for ch in run.text.chars() {
                if !ids.is_empty() {
                    ids.push(',');
                }
                let _ = write!(&mut ids, "U+{:04X}", ch as u32);
            }
        }
    }
    ids
}

async fn drain_cpp_font_rush_pending(previews: &mut [ActivePreview]) {
    use crate::r::font_kernel_service::FontKernelError;

    for preview in previews {
        let pending = preview
            .font_rush
            .as_mut()
            .and_then(|state| state.pending.take());
        let Some(pending) = pending else {
            continue;
        };
        crate::log_info!(
            target: "ui4";
            "ui4 cpp-font-rush consumer drain request={} rank={} sequence={} ticket={} frame={} buffer={} action=await-exact-destination-retirement-before-teardown\n",
            preview.request_serial,
            cpp_font_rush_rank(preview).unwrap_or(u8::MAX),
            pending.sequence,
            pending.ticket.raw(),
            pending.lease.frame.raw(),
            pending.lease.buffer_index,
        );
        match pending.completion.wait().await {
            Ok(stamped) => {
                preview.metrics.completed = preview.metrics.completed.saturating_add(1);
                let _ = cancel_frame_buffer(pending.lease);
                crate::log_info!(
                    target: "ui4";
                    "ui4 cpp-font-rush consumer drained request={} rank={} sequence={} ticket={} release={} result=complete action=cancel-unpublished-write-lease\n",
                    preview.request_serial,
                    cpp_font_rush_rank(preview).unwrap_or(u8::MAX),
                    pending.sequence,
                    pending.ticket.raw(),
                    stamped.release().sequence(),
                );
            }
            Err(FontKernelError::SubmittedIncomplete(reason)) => {
                preview.metrics.failed = preview.metrics.failed.saturating_add(1);
                crate::log_error!(
                    target: "ui4";
                    "ui4 cpp-font-rush consumer drain quarantined request={} rank={} sequence={} ticket={} reason={} action=retain-frame-write-lease\n",
                    preview.request_serial,
                    cpp_font_rush_rank(preview).unwrap_or(u8::MAX),
                    pending.sequence,
                    pending.ticket.raw(),
                    reason,
                );
            }
            Err(error) => {
                preview.metrics.failed = preview.metrics.failed.saturating_add(1);
                let _ = cancel_frame_buffer(pending.lease);
                crate::log_warn!(
                    target: "ui4";
                    "ui4 cpp-font-rush consumer drained request={} rank={} sequence={} ticket={} result={:?} action=cancel-retired-write-lease\n",
                    preview.request_serial,
                    cpp_font_rush_rank(preview).unwrap_or(u8::MAX),
                    pending.sequence,
                    pending.ticket.raw(),
                    error,
                );
            }
        }
    }
}

fn cpp_font_rush_background(rank: u8, sequence: u64) -> PremultipliedRgba8 {
    if rank == 0 {
        let pulse = (sequence as u8).wrapping_mul(3) % 18;
        PremultipliedRgba8::from_straight_rgba(5, 11 + pulse, 25 + pulse / 2, u8::MAX)
    } else {
        PremultipliedRgba8::from_straight_rgba(0, 0, 0, 0)
    }
}

const fn cpp_font_rush_glyph_count(rank: u8) -> usize {
    CPP_FONT_RUSH_GLYPHS[if rank < CPP_FONT_RUSH_MAX_PLANES as u8 {
        rank as usize
    } else {
        CPP_FONT_RUSH_MAX_PLANES - 1
    }]
}

const fn cpp_font_rush_grid(rank: u8) -> (u8, u8) {
    match rank {
        0 => (1, 1),
        1 => (2, 1),
        2 => (2, 2),
        _ => (4, 4),
    }
}

fn cpp_font_rush_stamp_request(
    width: u32,
    height: u32,
    rank: u8,
    rng: &mut crate::tyche::SoftRng,
) -> Result<(crate::r::font_kernel_service::FontStampRequest, usize, (u8, u8)), &'static str> {
    use crate::r::font_kernel_service::{
        FontStampFit, FontStampLayer, FontStampRequest, RetainSceneRequest,
        RetainedFontPositioning, RetainedFontRun,
    };

    let (columns, rows) = cpp_font_rush_grid(rank);
    let glyph_count = cpp_font_rush_glyph_count(rank);
    // Keep analytical font units within the font engine's 256 px contract,
    // then magnify the logical scene into the full scanout raster. At the
    // current 1440p target this makes every glyph occupy roughly 70% of its
    // physical cell instead of leaving the lower-rank planes visually tiny.
    let viewport_width = width.div_ceil(CPP_FONT_RUSH_VIEWPORT_SCALE).max(1);
    let viewport_height = height.div_ceil(CPP_FONT_RUSH_VIEWPORT_SCALE).max(1);
    let cell_width = viewport_width as f32 / f32::from(columns);
    let cell_height = viewport_height as f32 / f32::from(rows);
    let font_pixels = (cell_width * 0.70)
        .min(cell_height * 0.70)
        .clamp(4.0, 256.0);
    let font = crate::intel::gpu_font::GpuFontFace::Default;
    let foreground = match rank {
        0 => crate::intel::gpu_font::GpuFontRgba::new(245, 248, 255, u8::MAX),
        1 => crate::intel::gpu_font::GpuFontRgba::new(50, 225, 255, 224),
        2 => crate::intel::gpu_font::GpuFontRgba::new(255, 78, 214, 216),
        _ => crate::intel::gpu_font::GpuFontRgba::new(255, 205, 64, 208),
    };
    let glyph_work_limit =
        crate::intel::gpu_font::gpu_font_analytical_work_limit() / glyph_count.max(1) as u64;
    let mut runs = Vec::with_capacity(glyph_count);
    for index in 0..glyph_count {
        let (glyph, glyph_font_pixels) = cpp_font_rush_random_glyph(
            rng,
            font_pixels,
            viewport_width,
            viewport_height,
            width,
            height,
            glyph_work_limit,
        )?;
        let column = (index % usize::from(columns)) as f32;
        let row = (index / usize::from(columns)) as f32;
        // All cells on one plane share a color and transform contract, so one
        // retained scene can cover every independently rolled entry. Coverage
        // generation remains one analytical dispatch per glyph; only the
        // redundant per-glyph instance submissions are removed.
        runs.push(RetainedFontRun {
            text: glyph,
            position: [(column + 0.5) * cell_width, (row + 0.5) * cell_height],
            font_pixels: glyph_font_pixels,
            slant: 0.0,
        });
    }
    let request = FontStampRequest {
        layers: alloc::vec![FontStampLayer {
            scene: RetainSceneRequest {
                runs,
                font,
                viewport_width,
                viewport_height,
                raster_width: width,
                raster_height: height,
                positioning: RetainedFontPositioning::VisualBoundsCenter,
            },
            foreground,
        }],
        fit: FontStampFit::Canvas,
    };
    Ok((request, glyph_count, (columns, rows)))
}

fn cpp_font_rush_random_glyph(
    rng: &mut crate::tyche::SoftRng,
    preferred_font_pixels: f32,
    viewport_width: u32,
    viewport_height: u32,
    raster_width: u32,
    raster_height: u32,
    max_work: u64,
) -> Result<(String, f32), &'static str> {
    const RANGES: &[(u32, u32)] = &[
        (0x0021, 0x007E),
        (0x00A1, 0x024F),
        (0x0370, 0x052F),
        (0x0531, 0x058F),
        (0x0590, 0x06FF),
        (0x0900, 0x097F),
        (0x10A0, 0x10FF),
        (0x2000, 0x206F),
        (0x2190, 0x22FF),
        (0x2500, 0x27BF),
        (0x2C60, 0x2C7F),
        (0x4E00, 0x9FFF),
        (0x1F300, 0x1F64F),
    ];
    for _ in 0..32 {
        let (first, last) = RANGES[rng.usize_below(RANGES.len())];
        let scalar = first.saturating_add(rng.usize_below((last - first + 1) as usize) as u32);
        let Some(ch) = char::from_u32(scalar) else {
            continue;
        };
        if ch.is_control() || ch.is_whitespace() {
            continue;
        }
        if let Some(glyph) = cpp_font_rush_admit_glyph(
            ch,
            preferred_font_pixels,
            viewport_width,
            viewport_height,
            raster_width,
            raster_height,
            max_work,
        ) {
            return Ok((glyph, preferred_font_pixels));
        }
    }

    // Always retain a visible, bounded fallback even if the random scalar was
    // unsupported, outline-empty, or too complex at the magnified ppem. The
    // starting offset is still independently SoftRng-rolled for every cell.
    const SIMPLE_FALLBACKS: [char; 10] = ['.', '-', '|', '!', '1', 'I', 'l', '+', '/', '\\'];
    let fallback_start = rng.usize_below(SIMPLE_FALLBACKS.len());
    let mut font_pixels = preferred_font_pixels;
    loop {
        for offset in 0..SIMPLE_FALLBACKS.len() {
            let ch = SIMPLE_FALLBACKS[(fallback_start + offset) % SIMPLE_FALLBACKS.len()];
            if let Some(glyph) = cpp_font_rush_admit_glyph(
                ch,
                font_pixels,
                viewport_width,
                viewport_height,
                raster_width,
                raster_height,
                max_work,
            ) {
                return Ok((glyph, font_pixels));
            }
        }
        if font_pixels <= 4.0 {
            break;
        }
        font_pixels = (font_pixels * 0.75).max(4.0);
    }
    Err("font-rush-visible-glyph-unavailable")
}

fn cpp_font_rush_admit_glyph(
    ch: char,
    font_pixels: f32,
    viewport_width: u32,
    viewport_height: u32,
    raster_width: u32,
    raster_height: u32,
    max_work: u64,
) -> Option<String> {
    let mut glyph = String::new();
    glyph.push(ch);
    let work = crate::intel::gpu_font::gpu_font_analytical_text_work_estimate(
        glyph.as_str(),
        crate::intel::gpu_font::GpuFontFace::Default,
        font_pixels,
        viewport_width,
        viewport_height,
        raster_width,
        raster_height,
    )
    .ok()?;
    (work <= max_work).then_some(glyph)
}

async fn render_static30_frames(preview: &mut ActivePreview) -> Result<(), &'static str> {
    let mut surfaces = Vec::with_capacity(STATIC30_FRAME_COUNT);
    surfaces.push(StaticPreviewSurface {
        frame: preview.frame,
        window: preview.window,
        scheme: 0,
    });
    surfaces.extend(preview.extra_surfaces.iter().copied());

    let mut glyphs = 0usize;
    let mut kernel_submits = 0usize;
    let mut active_walkers = 0usize;
    let mut last_release = 0u64;
    for surface in &surfaces {
        preview.metrics.attempted = preview.metrics.attempted.saturating_add(1);
        let lease = match acquire_frame_buffer(surface.frame) {
            Ok(lease) => lease,
            Err(FramePoolError::Busy) => {
                preview.metrics.dropped_busy = preview.metrics.dropped_busy.saturating_add(1);
                preview.metrics.failed = preview.metrics.failed.saturating_add(1);
                return Err("static30-frame-busy");
            }
            Err(_) => {
                preview.metrics.failed = preview.metrics.failed.saturating_add(1);
                return Err("static30-frame-acquire-failed");
            }
        };
        let destination = match gpgpu_rgba_surface(lease) {
            Ok(destination) => destination,
            Err(_) => {
                let _ = cancel_frame_buffer(lease);
                preview.metrics.failed = preview.metrics.failed.saturating_add(1);
                return Err("static30-font-surface-unavailable");
            }
        };
        let request =
            static30_font_stamp_request(destination.width, destination.height, surface.scheme);
        let pending = match crate::r::font_kernel_service::submit_frame_stamp(request, destination)
        {
            Ok(pending) => pending,
            Err(_) => {
                let _ = cancel_frame_buffer(lease);
                preview.metrics.failed = preview.metrics.failed.saturating_add(1);
                return Err("static30-font-submit-failed");
            }
        };
        let stamped = match pending.wait().await {
            Ok(stamped) => stamped,
            Err(crate::r::font_kernel_service::FontKernelError::SubmittedIncomplete(_)) => {
                // The accepted producer may still target this exact surface.
                // Preserve the write lease so neither UI4 nor a future
                // producer can recycle it underneath a late GPU write.
                preview.metrics.failed = preview.metrics.failed.saturating_add(1);
                return Err("static30-font-submit-incomplete");
            }
            Err(_) => {
                let _ = cancel_frame_buffer(lease);
                preview.metrics.failed = preview.metrics.failed.saturating_add(1);
                return Err("static30-font-stamp-failed");
            }
        };
        preview.metrics.submitted = preview.metrics.submitted.saturating_add(1);
        preview.metrics.completed = preview.metrics.completed.saturating_add(1);
        glyphs = glyphs.saturating_add(stamped.glyphs());
        kernel_submits = kernel_submits.saturating_add(stamped.submits());
        active_walkers = active_walkers.saturating_add(stamped.active_walkers());
        last_release = stamped.release().sequence();
        if publish_gpu_font_frame_buffer(lease, stamped.release()).is_err() {
            let _ = cancel_frame_buffer(lease);
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            return Err("static30-font-frame-publish-failed");
        }
    }

    let publications = surfaces
        .iter()
        .map(|surface| (surface.window, DamageRect::FULL))
        .collect::<Vec<_>>();
    if publish_window_frames(PREVIEW_OWNER, &publications).is_err() {
        preview.metrics.failed = preview.metrics.failed.saturating_add(1);
        return Err("static30-window-publish-failed");
    }
    preview.metrics.published = preview
        .metrics
        .published
        .saturating_add(publications.len() as u64);

    preview.static_needs_publish = false;
    crate::log_info!(
        target: "ui4";
        "ui4 gpgpu-preview static30 published request={} frames={} windows={} slots=1+2+3 per_slot=10 glyphs={} font_jobs={} kernel_submits={} active_walkers={} release_sequence={} producer=font-kernel-service path=skrifa->gpu-vm-r8->cpp-font-instance->ui4-frame cadence=immutable/single publish_passes=1 cpu_readback=0 cpu_frame_copy=0\n",
        preview.request_serial,
        preview.metrics.published,
        preview_surface_count(preview),
        glyphs,
        preview.metrics.submitted,
        kernel_submits,
        active_walkers,
        last_release,
    );
    Ok(())
}

fn static30_font_stamp_request(
    width: u32,
    height: u32,
    scheme: u8,
) -> crate::r::font_kernel_service::FontStampRequest {
    use crate::r::font_kernel_service::{
        FontStampFit, FontStampLayer, FontStampRequest, RetainSceneRequest,
        RetainedFontPositioning, RetainedFontRun,
    };

    let horizontal_padding = 10.0f32;
    let vertical_padding = 8.0f32;
    let width_fit = (width.saturating_sub(20) as f32 / 9.0).max(8.0);
    let height_fit = (height.saturating_sub(16) as f32 / 5.4).max(8.0);
    let font_pixels = width_fit.min(height_fit).clamp(8.0, 26.0);
    let baseline = vertical_padding + font_pixels;
    let line_advance = font_pixels * 1.22;
    let scene = |runs| RetainSceneRequest {
        runs,
        font: crate::intel::gpu_font::GpuFontFace::Default,
        viewport_width: width,
        viewport_height: height,
        raster_width: width,
        raster_height: height,
        positioning: RetainedFontPositioning::SceneOrigin,
    };
    let run = |text: &'static str, row: usize| RetainedFontRun {
        text: String::from(text),
        position: [horizontal_padding, baseline + row as f32 * line_advance],
        font_pixels,
        slant: if scheme.is_multiple_of(3) { 0.08 } else { 0.0 },
    };
    let seed = u16::from(scheme);
    FontStampRequest {
        layers: alloc::vec![
            FontStampLayer {
                scene: scene(alloc::vec![run("Lorem ipsum", 0)]),
                foreground: crate::intel::gpu_font::GpuFontRgba::new(
                    (112 + seed * 37 % 143) as u8,
                    (144 + seed * 53 % 111) as u8,
                    (176 + seed * 29 % 79) as u8,
                    u8::MAX,
                ),
            },
            FontStampLayer {
                scene: scene(alloc::vec![
                    run("dolor sit amet,", 1),
                    run("consectetur", 2),
                    run("adipiscing elit.", 3),
                ]),
                foreground: crate::intel::gpu_font::GpuFontRgba::new(238, 244, 255, u8::MAX,),
            },
        ],
        fit: FontStampFit::Canvas,
    }
}

/// Paint one unmistakable CPU-authored frame. This path deliberately does not
/// request a GPGPU surface, upload a kernel, submit through GuC, or poll a GPU
/// completion marker. The cache release is the only producer-side operation
/// between the CPU writes and ordinary UI4 publication.
fn fill_static_preview_frame(lease: FrameWriteLease, scheme: u8) -> Result<(), &'static str> {
    let view = writable_rgba_view(lease).map_err(|_| "static-rgba-view-unavailable")?;
    let row_bytes = (view.width as usize)
        .checked_mul(4)
        .ok_or("static-rgba-layout-invalid")?;
    let pitch = view.pitch as usize;
    let required = pitch
        .checked_mul(view.height as usize)
        .ok_or("static-rgba-layout-invalid")?;
    if pitch < row_bytes || required > view.byte_len {
        return Err("static-rgba-layout-invalid");
    }

    let seed = u16::from(scheme);
    let navy = PremultipliedRgba8::from_straight_rgba(
        (8 + seed * 17 % 40) as u8,
        (16 + seed * 29 % 48) as u8,
        (40 + seed * 13 % 64) as u8,
        u8::MAX,
    )
    .to_native_bytes();
    let blue = PremultipliedRgba8::from_straight_rgba(
        (32 + seed * 53 % 192) as u8,
        (32 + seed * 97 % 192) as u8,
        (32 + seed * 151 % 192) as u8,
        u8::MAX,
    )
    .to_native_bytes();
    let cyan = PremultipliedRgba8::from_straight_rgba(
        (48 + seed * 71 % 176) as u8,
        (48 + seed * 113 % 176) as u8,
        (48 + seed * 37 % 176) as u8,
        u8::MAX,
    )
    .to_native_bytes();
    let magenta = PremultipliedRgba8::from_straight_rgba(
        (48 + seed * 131 % 176) as u8,
        (48 + seed * 43 % 176) as u8,
        (48 + seed * 83 % 176) as u8,
        u8::MAX,
    )
    .to_native_bytes();
    let white = PremultipliedRgba8::from_straight_rgba(240, 248, 255, u8::MAX).to_native_bytes();
    let width = view.width as usize;
    let height = view.height as usize;
    let half_width = width / 2;
    let half_height = height / 2;

    // SAFETY: the write lease exclusively owns this allocation, and the
    // checked pitch/height product is contained by `byte_len`.
    let bytes = unsafe { core::slice::from_raw_parts_mut(view.virt, view.byte_len) };
    for y in 0..height {
        let row = &mut bytes[y * pitch..y * pitch + row_bytes];
        for x in 0..width {
            let border = x < 8 || y < 8 || x + 8 >= width || y + 8 >= height;
            let grid = x % 64 < 2 || y % 64 < 2;
            let pixel = if border {
                white
            } else if grid {
                cyan
            } else if x < half_width && y < half_height {
                blue
            } else if x >= half_width && y >= half_height {
                magenta
            } else {
                navy
            };
            row[x * 4..x * 4 + 4].copy_from_slice(&pixel);
        }
    }
    crate::intel::dma_flush(view.virt, view.byte_len);
    Ok(())
}

#[derive(Copy, Clone)]
struct PreviewDispatchResult {
    ok: bool,
    submitted: bool,
    iterations: u32,
    marker: u32,
    submit_ms: u64,
    release: Option<crate::intel::gpgpu::GpgpuRgba8ReleaseFence>,
    error: &'static str,
}

fn dispatch_preview_kernel(
    preview: &mut ActivePreview,
    surface: crate::intel::gpgpu::GpgpuRgba8Surface,
) -> PreviewDispatchResult {
    match preview.config.preset {
        GpgpuPreviewPreset::All => PreviewDispatchResult {
            ok: false,
            submitted: false,
            iterations: 0,
            marker: 0,
            submit_ms: 0,
            release: None,
            error: "compute-trio-entered-single-dispatch",
        },
        GpgpuPreviewPreset::Static
        | GpgpuPreviewPreset::Static30
        | GpgpuPreviewPreset::CppFont
        | GpgpuPreviewPreset::CppFontRush => PreviewDispatchResult {
            ok: false,
            submitted: false,
            iterations: 0,
            marker: 0,
            submit_ms: 0,
            release: None,
            error: "static-preset-entered-gpu-dispatch",
        },
        GpgpuPreviewPreset::Mandelbrot => {
            let iterations = 32 + ((preview.metrics.attempted - 1) % 97) as u32;
            match crate::intel::gpgpu::mandel64_worklist_surface_full(surface, iterations) {
                Some(result) => PreviewDispatchResult {
                    ok: result.ok,
                    submitted: result.submitted,
                    iterations,
                    marker: result.marker,
                    submit_ms: result.submit_ms,
                    release: result.release,
                    error: "mandelbrot-dispatch-failed",
                },
                None => PreviewDispatchResult {
                    ok: false,
                    submitted: false,
                    iterations,
                    marker: 0,
                    submit_ms: 0,
                    release: None,
                    error: "mandelbrot-dispatch-unavailable",
                },
            }
        }
        GpgpuPreviewPreset::Chart => {
            let seconds = preview.metrics.elapsed_ms as f32 / 1_000.0;
            let flags = crate::intel::gpgpu::CHART_SINE_FLAG_GRID
                | crate::intel::gpgpu::CHART_SINE_FLAG_AXES
                | crate::intel::gpgpu::CHART_SINE_FLAG_GLOW
                | crate::intel::gpgpu::CHART_SINE_FLAG_BORDER;
            let result = crate::intel::gpgpu::chart_sine_rgba8_surface_full(
                surface,
                seconds * core::f32::consts::FRAC_PI_2,
                flags,
            );
            PreviewDispatchResult {
                ok: result.ok,
                submitted: result.submitted,
                iterations: 0,
                marker: result.marker,
                submit_ms: result.submit_ms,
                release: result.release,
                error: "chart-dispatch-failed",
            }
        }
        GpgpuPreviewPreset::Plasma => {
            let seconds = preview.metrics.elapsed_ms as f32 / 1_000.0;
            let flags = crate::intel::gpgpu::PIXEL_PLASMA_FLAG_VIGNETTE
                | crate::intel::gpgpu::PIXEL_PLASMA_FLAG_RINGS
                | crate::intel::gpgpu::PIXEL_PLASMA_FLAG_SCANLINE
                | crate::intel::gpgpu::PIXEL_PLASMA_FLAG_FIELD_PALETTE;
            let result =
                crate::intel::gpgpu::pixel_plasma_rgba8_surface_full(surface, seconds, flags);
            PreviewDispatchResult {
                ok: result.ok,
                submitted: result.submitted,
                iterations: 0,
                marker: result.marker,
                submit_ms: result.submit_ms,
                release: result.release,
                error: "plasma-dispatch-failed",
            }
        }
        GpgpuPreviewPreset::Lab256 => {
            let result = crate::intel::gpgpu::lab256_preview_frame(surface);
            PreviewDispatchResult {
                ok: result.ok,
                submitted: result.submitted,
                iterations: 3,
                marker: result.marker,
                submit_ms: result.submit_ms,
                release: result.release,
                error: "lab256-dispatch-failed",
            }
        }
        GpgpuPreviewPreset::CppGallery
        | GpgpuPreviewPreset::CppAurora
        | GpgpuPreviewPreset::CppJulia
        | GpgpuPreviewPreset::CppSdf
        | GpgpuPreviewPreset::CppVoronoi
        | GpgpuPreviewPreset::CppRetroSun => {
            let seconds = preview.metrics.elapsed_ms as f32 / 1_000.0;
            let mode = match preview.config.preset {
                GpgpuPreviewPreset::CppGallery => crate::intel::gpgpu::CPP_DEMO_MODE_GALLERY,
                GpgpuPreviewPreset::CppAurora => crate::intel::gpgpu::CPP_DEMO_MODE_AURORA,
                GpgpuPreviewPreset::CppJulia => crate::intel::gpgpu::CPP_DEMO_MODE_JULIA,
                GpgpuPreviewPreset::CppSdf => crate::intel::gpgpu::CPP_DEMO_MODE_SDF,
                GpgpuPreviewPreset::CppVoronoi => crate::intel::gpgpu::CPP_DEMO_MODE_VORONOI,
                GpgpuPreviewPreset::CppRetroSun => crate::intel::gpgpu::CPP_DEMO_MODE_RETRO_SUN,
                _ => crate::intel::gpgpu::CPP_DEMO_MODE_GALLERY,
            };
            let seed = (preview.request_serial as u32)
                .rotate_left(13)
                .wrapping_add(0xC0DE_C901);
            let result =
                crate::intel::gpgpu::cpp_demo_rgba8_surface_full(surface, seconds, mode, seed);
            PreviewDispatchResult {
                ok: result.ok,
                submitted: result.submitted,
                iterations: mode,
                marker: result.marker,
                submit_ms: result.submit_ms,
                release: result.release,
                error: "cpp-demo-dispatch-failed",
            }
        }
        GpgpuPreviewPreset::CppAudio => {
            let seconds = preview.metrics.elapsed_ms as f32 / 1_000.0;
            let snapshot = crate::aud::audio_visualizer::snapshot();
            let result = crate::intel::gpgpu::cpp_audio_visualizer_rgba8_surface_full(
                surface,
                seconds,
                preview.metrics.attempted as u32,
                &snapshot,
            );
            PreviewDispatchResult {
                ok: result.ok,
                submitted: result.submitted,
                iterations: crate::aud::audio_visualizer::AUDIO_VISUALIZER_FFT_FRAMES as u32,
                marker: result.marker,
                submit_ms: result.submit_ms,
                release: result.release,
                error: "cpp-audio-visualizer-dispatch-failed",
            }
        }
        GpgpuPreviewPreset::CppParticle => {
            let seconds = preview.metrics.elapsed_ms as f32 / 1_000.0;
            let attempted = preview.metrics.attempted;
            let dt = (preview.config.cadence_ms.min(50) as f32 / 1_000.0).max(0.001);
            let seed = (preview.request_serial as u32)
                .rotate_left(11)
                .wrapping_add(0xC0FF_EE51);
            let mut params =
                crate::intel::gpgpu::ParticleCraftParamsV1::arc_forge(seconds, dt, seed);
            if attempted == 1 {
                params.flags |= crate::intel::gpgpu::PARTICLE_CRAFT_FLAG_RESET;
            }
            let craft = match preview.particle_craft.as_mut() {
                Some(craft) => craft,
                None => {
                    preview.particle_craft =
                        crate::intel::gpgpu::GpgpuOwnedParticleCraftState::allocate();
                    let Some(craft) = preview.particle_craft.as_mut() else {
                        return PreviewDispatchResult {
                            ok: false,
                            submitted: false,
                            iterations: 0,
                            marker: 0,
                            submit_ms: 0,
                            release: None,
                            error: "particle-craft-state-allocation-failed",
                        };
                    };
                    craft
                }
            };
            let result = crate::intel::gpgpu::particle_craft_rgba8_frame(craft, surface, params);
            PreviewDispatchResult {
                ok: result.ok,
                submitted: result.submitted,
                iterations: params.active_count,
                marker: result.marker,
                submit_ms: result.submit_ms,
                release: result.release,
                error: "particle-craft-dispatch-failed",
            }
        }
    }
}

const fn preview_release_label(preset: GpgpuPreviewPreset) -> &'static str {
    match preset {
        GpgpuPreviewPreset::All
        | GpgpuPreviewPreset::Mandelbrot
        | GpgpuPreviewPreset::Chart
        | GpgpuPreviewPreset::Plasma
        | GpgpuPreviewPreset::CppGallery
        | GpgpuPreviewPreset::CppAurora
        | GpgpuPreviewPreset::CppJulia
        | GpgpuPreviewPreset::CppSdf
        | GpgpuPreviewPreset::CppVoronoi
        | GpgpuPreviewPreset::CppRetroSun
        | GpgpuPreviewPreset::CppAudio
        | GpgpuPreviewPreset::CppParticle => "pipe-control+post-marker-exact-surface",
        GpgpuPreviewPreset::Lab256 => "three-pass+pipe-control+post-marker-exact-surface",
        GpgpuPreviewPreset::Static => "clflush-mfence-before-publish",
        GpgpuPreviewPreset::Static30 => "font-instance+pipe-control+post-marker-exact-surface",
        GpgpuPreviewPreset::CppFont | GpgpuPreviewPreset::CppFontRush => {
            "font-instance+pipe-control+post-marker-exact-surface"
        }
    }
}

const fn preview_producer_label(preset: GpgpuPreviewPreset) -> &'static str {
    match preset {
        GpgpuPreviewPreset::All => "guc-compute-trio",
        GpgpuPreviewPreset::Lab256 => "guc-lab256-three-pass",
        GpgpuPreviewPreset::Static => "cpu-static",
        GpgpuPreviewPreset::Static30 => "font-kernel-service-cpp",
        GpgpuPreviewPreset::Mandelbrot | GpgpuPreviewPreset::Chart | GpgpuPreviewPreset::Plasma => {
            "guc-compute-single"
        }
        GpgpuPreviewPreset::CppGallery
        | GpgpuPreviewPreset::CppAurora
        | GpgpuPreviewPreset::CppJulia
        | GpgpuPreviewPreset::CppSdf
        | GpgpuPreviewPreset::CppVoronoi
        | GpgpuPreviewPreset::CppRetroSun
        | GpgpuPreviewPreset::CppAudio => "guc-cpp-single",
        GpgpuPreviewPreset::CppParticle => "guc-cpp-stateful-three-pass",
        GpgpuPreviewPreset::CppFont | GpgpuPreviewPreset::CppFontRush => "font-kernel-service-cpp",
    }
}

const fn preview_plane(preset: GpgpuPreviewPreset) -> WindowPlane {
    match preset {
        GpgpuPreviewPreset::All | GpgpuPreviewPreset::Static | GpgpuPreviewPreset::Static30 => {
            WindowPlane::Universal(super::ALPHA_OVERLAY_PLANE_SLOT as u8)
        }
        GpgpuPreviewPreset::Mandelbrot
        | GpgpuPreviewPreset::Chart
        | GpgpuPreviewPreset::Plasma
        | GpgpuPreviewPreset::CppGallery
        | GpgpuPreviewPreset::CppAurora
        | GpgpuPreviewPreset::CppJulia
        | GpgpuPreviewPreset::CppSdf
        | GpgpuPreviewPreset::CppVoronoi
        | GpgpuPreviewPreset::CppRetroSun
        | GpgpuPreviewPreset::CppAudio
        | GpgpuPreviewPreset::CppParticle
        | GpgpuPreviewPreset::CppFont => WindowPlane::Universal(preview_plane_slot(preset) as u8),
        GpgpuPreviewPreset::CppFontRush => WindowPlane::Primary,
        GpgpuPreviewPreset::Lab256 => WindowPlane::Universal(super::ALPHA_OVERLAY_PLANE_SLOT as u8),
    }
}

const fn preview_consumer_label(preset: GpgpuPreviewPreset) -> &'static str {
    match preset {
        GpgpuPreviewPreset::All => "ui4-direct-slots1+2+3",
        GpgpuPreviewPreset::Mandelbrot => "ui4-direct-slot1",
        GpgpuPreviewPreset::Chart => "ui4-direct-slot2",
        GpgpuPreviewPreset::Plasma => "ui4-direct-slot3",
        GpgpuPreviewPreset::Lab256 => "ui4-alpha-slot1-256x256",
        GpgpuPreviewPreset::CppGallery
        | GpgpuPreviewPreset::CppAurora
        | GpgpuPreviewPreset::CppJulia
        | GpgpuPreviewPreset::CppSdf
        | GpgpuPreviewPreset::CppVoronoi
        | GpgpuPreviewPreset::CppRetroSun
        | GpgpuPreviewPreset::CppAudio
        | GpgpuPreviewPreset::CppParticle => "ui4-cpp-resizable-slot1",
        GpgpuPreviewPreset::CppFont => "ui4-font-scene-slot1",
        GpgpuPreviewPreset::CppFontRush => "ui4-direct-font-scene-slots0+1+2+3",
        GpgpuPreviewPreset::Static => "ui4-overlay",
        GpgpuPreviewPreset::Static30 => "ui4-font-scene-slots1+2+3",
    }
}

fn preview_surface_count(preview: &ActivePreview) -> usize {
    1usize.saturating_add(preview.extra_surfaces.len())
}

const fn should_log_preview_checkpoint(sequence: u64) -> bool {
    sequence <= 8 || sequence.is_multiple_of(120)
}

fn preview_needs_render(preview: &ActivePreview) -> bool {
    if preview.policy.frame_limit != 0 && preview.metrics.published >= preview.policy.frame_limit {
        return false;
    }
    !matches!(
        preview.config.preset,
        GpgpuPreviewPreset::Static | GpgpuPreviewPreset::Static30 | GpgpuPreviewPreset::CppFont
    ) || preview.static_needs_publish
}

fn schedule_next_render(preview: &mut ActivePreview) {
    let period = if preview.policy.target_hz == 0 {
        Duration::from_millis(preview.config.cadence_ms)
    } else {
        let hz = preview.policy.target_hz;
        let mut ticks = embassy_time::TICK_HZ / hz;
        preview.cadence_phase += embassy_time::TICK_HZ % hz;
        if preview.cadence_phase >= hz {
            preview.cadence_phase -= hz;
            ticks += 1;
        }
        Duration::from_ticks(ticks.max(1))
    };
    let scheduled = preview.next_render + period;
    let now = Instant::now();
    if now > scheduled {
        preview.metrics.late = preview.metrics.late.saturating_add(1);
        preview.next_render = now + period;
    } else {
        preview.next_render = scheduled;
    }
}

fn next_poll_ms(preview: &ActivePreview) -> u64 {
    if !preview_needs_render(preview) {
        return COMMAND_POLL_MAX_MS;
    }
    if preview
        .font_rush
        .as_ref()
        .is_some_and(|state| state.pending.is_some() || state.scanout_pending.is_some())
    {
        return 1;
    }
    let now = Instant::now();
    if preview.next_render <= now {
        return 1;
    }
    preview
        .next_render
        .saturating_duration_since(now)
        .as_millis()
        .clamp(1, COMMAND_POLL_MAX_MS)
}

fn drain_preview_input(active: &mut [ActivePreview], retired_frames: &mut Vec<FrameHandle>) {
    for event in take_owner_input_events(PREVIEW_OWNER) {
        let Ui4InputEvent::Resize(event) = event else {
            continue;
        };
        let Some(preview) = active
            .iter_mut()
            .find(|preview| event.window == preview.window)
        else {
            continue;
        };
        if event.width == 0 || event.height == 0 {
            continue;
        }
        try_resize_preview(preview, event.width, event.height, retired_frames, "input-event");
    }
}

/// Resize notifications are intentionally only a wakeup hint. The broker's
/// geometry is authoritative, so a dropped event or a transient allocation
/// failure cannot leave a maximized C++ preview permanently backed by its
/// original 640x400 frame or repeatedly request an already-live scaled backing.
fn reconcile_preview_extents(active: &mut [ActivePreview], retired_frames: &mut Vec<FrameHandle>) {
    for preview in active {
        if !preview.config.preset.is_resizable_cpp() {
            continue;
        }
        let Ok(placement) = window_placement(PREVIEW_OWNER, preview.window) else {
            continue;
        };
        let (backing_width, backing_height) =
            preview_backing_extent(preview.config.preset, placement.width, placement.height);
        if placement.width == 0
            || placement.height == 0
            || (backing_width == preview.width && backing_height == preview.height)
        {
            continue;
        }
        try_resize_preview(
            preview,
            placement.width,
            placement.height,
            retired_frames,
            "broker-reconcile",
        );
    }
}

fn try_resize_preview(
    preview: &mut ActivePreview,
    width: u32,
    height: u32,
    retired_frames: &mut Vec<FrameHandle>,
    source: &'static str,
) {
    let (backing_width, backing_height) =
        preview_backing_extent(preview.config.preset, width, height);
    if backing_width == preview.width && backing_height == preview.height {
        return;
    }
    let now = Instant::now();
    if width == preview.resize_retry_width
        && height == preview.resize_retry_height
        && now < preview.resize_retry_at
    {
        return;
    }
    match resize_preview(preview, width, height, retired_frames) {
        Ok(()) => {
            preview.resize_retry_width = 0;
            preview.resize_retry_height = 0;
            preview.resize_retry_at = now;
            set_active_error(preview.request_serial, "none");
        }
        Err(reason) => {
            preview.resize_retry_width = width;
            preview.resize_retry_height = height;
            preview.resize_retry_at = now + Duration::from_millis(RESIZE_RETRY_MS);
            set_active_error(preview.request_serial, reason);
            crate::log_warn!(
                target: "ui4";
                "ui4 gpgpu-preview resize rejected request={} window={} logical_extent={}x{} source={} retry_ms={} reason={}\n",
                preview.request_serial,
                preview.window.raw(),
                width,
                height,
                source,
                RESIZE_RETRY_MS,
                reason,
            );
        }
    }
}

fn resize_preview(
    preview: &mut ActivePreview,
    logical_width: u32,
    logical_height: u32,
    retired_frames: &mut Vec<FrameHandle>,
) -> Result<(), &'static str> {
    let output = OutputId::from_slot(0).ok_or("output-d01-unavailable")?;
    let (backing_width, backing_height) =
        preview_backing_extent(preview.config.preset, logical_width, logical_height);
    let replacement = create_preview_frame(output, backing_width, backing_height)
        .map_err(preview_frame_create_error_label)?;
    if replace_window_frame(PREVIEW_OWNER, preview.window, replacement).is_err() {
        let _ = destroy_frame(replacement);
        return Err("resize-window-replace-failed");
    }
    let previous = preview.frame;
    preview.frame = replacement;
    preview.width = backing_width;
    preview.height = backing_height;
    preview.next_render = Instant::now();
    preview.static_needs_publish = true;
    retired_frames.push(previous);
    crate::log_info!(
        target: "ui4";
        "ui4 gpgpu-preview resize applied request={} window={} frame={} logical_extent={}x{} backing_extent={}x{} presentation={} plane_mutation=scaler-only\n",
        preview.request_serial,
        preview.window.raw(),
        replacement.raw(),
        logical_width,
        logical_height,
        backing_width,
        backing_height,
        if (backing_width, backing_height) == (logical_width, logical_height) {
            "1:1"
        } else {
            "direct-plane-2x"
        },
    );
    Ok(())
}

fn preview_backing_extent(
    preset: GpgpuPreviewPreset,
    logical_width: u32,
    logical_height: u32,
) -> (u32, u32) {
    if preset == GpgpuPreviewPreset::CppParticle {
        crate::intel::gpgpu::particle_craft_backing_extent(logical_width, logical_height)
    } else {
        (logical_width, logical_height)
    }
}

fn stop_active_previews(
    previews: Vec<ActivePreview>,
    retired_frames: &mut Vec<FrameHandle>,
    reason: &'static str,
) {
    let Some(first) = previews.first() else {
        return;
    };
    if previews
        .iter()
        .any(|preview| preview.config.preset == GpgpuPreviewPreset::CppAudio)
    {
        crate::aud::audio_visualizer::set_enabled(false);
    }
    // One source per hardware slot can close through pipe-scaler geometry and
    // plane constant alpha beside fresh GGTT aliases for the same allocation.
    // No composition target is created; UI4 owns retirement through the final
    // SURFLIVE-backed plane replacement. Three planes use two scaler waves.
    let direct_close = previews_support_direct_close(&previews);
    let frame_lifecycle_transferred = if direct_close {
        finish_window_session_with_request(
            PREVIEW_OWNER,
            first.session,
            WindowSessionCloseRequest::default().direct_plane_animate_and_retire_frames(),
        )
        .is_ok()
    } else {
        false
    };
    if !frame_lifecycle_transferred {
        let _ = finish_window_session(PREVIEW_OWNER, first.session);
    }
    let metrics = aggregate_preview_metrics(&previews);
    crate::log_info!(
        target: "ui4";
        "ui4 gpgpu-preview stopped request={} preset={} frames={} windows={} attempted={} submitted={} completed={} published={} scanout_live={} scanout_superseded={} dropped_busy={} dropped_frame_busy={} dropped_queue_full={} dropped_in_flight={} dropped_cadence={} failed={} late={} elapsed_ms={} reason={} teardown={} frame_retire=after-surflive-display-lease-drain source_buffer_mutation=none\n",
        first.request_serial,
        first.config.preset.label(),
        previews.len(),
        previews.iter().map(preview_surface_count).sum::<usize>(),
        metrics.attempted,
        metrics.submitted,
        metrics.completed,
        metrics.published,
        metrics.scanout_live,
        metrics.scanout_superseded,
        metrics.dropped_busy,
        metrics.dropped_frame_busy,
        metrics.dropped_queue_full,
        metrics.dropped_in_flight,
        metrics.dropped_cadence,
        metrics.failed,
        metrics.late,
        metrics.elapsed_ms,
        reason,
        if frame_lifecycle_transferred {
            "direct-plane-scaler+alpha"
        } else {
            "broker-detach-no-animation"
        },
    );
    for preview in previews {
        crate::log_info!(
            target: "ui4";
            "ui4 gpgpu-preview stopped-member request={} preset={} frame={} window={} slot={} attempted={} submitted={} completed={} published={} scanout_live={} scanout_superseded={} dropped_busy={} dropped_frame_busy={} dropped_queue_full={} dropped_in_flight={} dropped_cadence={} failed={} late={} elapsed_ms={} reason={}\n",
            preview.request_serial,
            preview.config.preset.label(),
            preview.frame.raw(),
            preview.window.raw(),
            active_preview_plane_slot(&preview),
            preview.metrics.attempted,
            preview.metrics.submitted,
            preview.metrics.completed,
            preview.metrics.published,
            preview.metrics.scanout_live,
            preview.metrics.scanout_superseded,
            preview.metrics.dropped_busy,
            preview.metrics.dropped_frame_busy,
            preview.metrics.dropped_queue_full,
            preview.metrics.dropped_in_flight,
            preview.metrics.dropped_cadence,
            preview.metrics.failed,
            preview.metrics.late,
            preview.metrics.elapsed_ms,
            reason,
        );
        if !frame_lifecycle_transferred {
            if !retired_frames.contains(&preview.frame) {
                retired_frames.push(preview.frame);
            }
            for surface in preview.extra_surfaces {
                if !retired_frames.contains(&surface.frame) {
                    retired_frames.push(surface.frame);
                }
            }
        }
    }
}

fn previews_support_direct_close(previews: &[ActivePreview]) -> bool {
    let mut slots = 0u8;
    for preview in previews {
        if !preview.extra_surfaces.is_empty() {
            return false;
        }
        let slot = active_preview_plane_slot(preview);
        if !(1..=3).contains(&slot) {
            return false;
        }
        let bit = 1u8 << slot;
        if slots & bit != 0 {
            return false;
        }
        slots |= bit;
    }
    !previews.is_empty()
}

fn retire_frames(frames: &mut Vec<FrameHandle>) {
    let mut index = 0;
    while index < frames.len() {
        match destroy_frame(frames[index]) {
            Ok(()) | Err(FramePoolError::InvalidHandle) => {
                frames.swap_remove(index);
            }
            Err(FramePoolError::Busy) => index += 1,
            Err(error) => {
                let frame = frames.swap_remove(index);
                crate::log_warn!(
                    target: "ui4";
                    "ui4 gpgpu-preview frame retire abandoned frame={} error={:?}\n",
                    frame.raw(),
                    error,
                );
            }
        }
    }
}

fn mark_starting(desired: DesiredPreview) {
    let mut control = PREVIEW_CONTROL.lock();
    control.status.phase = GpgpuPreviewPhase::Starting;
    control.status.applied_serial = desired.serial;
    control.status.config = desired.config;
    control.status.frame = None;
    control.status.window = None;
    control.status.width = 0;
    control.status.height = 0;
    control.status.metrics = GpgpuPreviewMetrics::default();
    control.status.members = INACTIVE_PREVIEW_MEMBERS;
    control.status.last_error = "none";
}

fn mark_faulted(desired: DesiredPreview, reason: &'static str) {
    let mut control = PREVIEW_CONTROL.lock();
    control.status.phase = GpgpuPreviewPhase::Faulted;
    control.status.applied_serial = desired.serial;
    control.status.frame = None;
    control.status.window = None;
    control.status.width = 0;
    control.status.height = 0;
    control.status.members = INACTIVE_PREVIEW_MEMBERS;
    control.status.last_error = reason;
}

fn mark_idle(serial: u64, reason: &'static str) {
    let mut control = PREVIEW_CONTROL.lock();
    control.status.phase = GpgpuPreviewPhase::Idle;
    control.status.applied_serial = serial;
    control.status.frame = None;
    control.status.window = None;
    control.status.width = 0;
    control.status.height = 0;
    clear_preview_member_handles(&mut control.status.members);
    control.status.last_error = reason;
}

fn mark_duration_complete(
    serial: u64,
    metrics: GpgpuPreviewMetrics,
    mut members: [GpgpuPreviewMemberStatus; 4],
) {
    clear_preview_member_handles(&mut members);
    let mut control = PREVIEW_CONTROL.lock();
    if control.desired.serial == serial {
        control.desired.running = false;
        control.status.desired_running = false;
    }
    control.status.phase = GpgpuPreviewPhase::Idle;
    control.status.applied_serial = serial;
    control.status.frame = None;
    control.status.window = None;
    control.status.width = 0;
    control.status.height = 0;
    control.status.metrics = metrics;
    control.status.members = members;
    control.status.last_error = "duration-complete";
}

fn mark_runtime_fault(
    serial: u64,
    metrics: GpgpuPreviewMetrics,
    mut members: [GpgpuPreviewMemberStatus; 4],
    reason: &'static str,
) {
    clear_preview_member_handles(&mut members);
    let mut control = PREVIEW_CONTROL.lock();
    if control.desired.serial == serial {
        control.desired.running = false;
        control.status.desired_running = false;
    }
    control.status.phase = GpgpuPreviewPhase::Faulted;
    control.status.applied_serial = serial;
    control.status.frame = None;
    control.status.window = None;
    control.status.width = 0;
    control.status.height = 0;
    control.status.metrics = metrics;
    control.status.members = members;
    control.status.last_error = reason;
}

fn publish_active_status(
    previews: &[ActivePreview],
    phase: GpgpuPreviewPhase,
    last_error: &'static str,
) {
    let Some(first) = previews.first() else {
        return;
    };
    let mut control = PREVIEW_CONTROL.lock();
    if control.status.applied_serial != first.request_serial {
        return;
    }
    control.status.phase = phase;
    control.status.frame = Some(first.frame);
    control.status.window = Some(first.window);
    control.status.width = first.width;
    control.status.height = first.height;
    control.status.metrics = aggregate_preview_metrics(previews);
    control.status.members = preview_member_statuses(previews);
    if control.status.last_error == "none" || last_error != "none" {
        control.status.last_error = last_error;
    }
}

fn aggregate_preview_metrics(previews: &[ActivePreview]) -> GpgpuPreviewMetrics {
    let mut aggregate = GpgpuPreviewMetrics::default();
    for preview in previews {
        aggregate.attempted = aggregate
            .attempted
            .saturating_add(preview.metrics.attempted);
        aggregate.submitted = aggregate
            .submitted
            .saturating_add(preview.metrics.submitted);
        aggregate.completed = aggregate
            .completed
            .saturating_add(preview.metrics.completed);
        aggregate.published = aggregate
            .published
            .saturating_add(preview.metrics.published);
        aggregate.scanout_live = aggregate
            .scanout_live
            .saturating_add(preview.metrics.scanout_live);
        aggregate.scanout_superseded = aggregate
            .scanout_superseded
            .saturating_add(preview.metrics.scanout_superseded);
        aggregate.dropped_busy = aggregate
            .dropped_busy
            .saturating_add(preview.metrics.dropped_busy);
        aggregate.dropped_frame_busy = aggregate
            .dropped_frame_busy
            .saturating_add(preview.metrics.dropped_frame_busy);
        aggregate.dropped_queue_full = aggregate
            .dropped_queue_full
            .saturating_add(preview.metrics.dropped_queue_full);
        aggregate.dropped_in_flight = aggregate
            .dropped_in_flight
            .saturating_add(preview.metrics.dropped_in_flight);
        aggregate.dropped_cadence = aggregate
            .dropped_cadence
            .saturating_add(preview.metrics.dropped_cadence);
        aggregate.failed = aggregate.failed.saturating_add(preview.metrics.failed);
        aggregate.late = aggregate.late.saturating_add(preview.metrics.late);
        aggregate.elapsed_ms = aggregate.elapsed_ms.max(preview.metrics.elapsed_ms);
        aggregate.last_iterations = aggregate
            .last_iterations
            .max(preview.metrics.last_iterations);
        aggregate.last_marker = aggregate.last_marker.max(preview.metrics.last_marker);
        aggregate.last_submit_ms = aggregate.last_submit_ms.max(preview.metrics.last_submit_ms);
    }
    aggregate
}

fn preview_member_statuses(previews: &[ActivePreview]) -> [GpgpuPreviewMemberStatus; 4] {
    let mut members = INACTIVE_PREVIEW_MEMBERS;
    for preview in previews {
        let index = if preview.config.preset == GpgpuPreviewPreset::CppFontRush {
            match preview.font_rush.as_ref() {
                Some(state) if usize::from(state.rank) < members.len() => usize::from(state.rank),
                _ => continue,
            }
        } else {
            let Some(index) = compute_preview_index(preview.config.preset) else {
                continue;
            };
            index
        };
        members[index] = GpgpuPreviewMemberStatus {
            preset: preview.config.preset,
            frame: Some(preview.frame),
            window: Some(preview.window),
            plane_slot: active_preview_plane_slot(preview) as u8,
            metrics: preview.metrics,
        };
    }
    members
}

fn clear_preview_member_handles(members: &mut [GpgpuPreviewMemberStatus; 4]) {
    for member in members {
        member.frame = None;
        member.window = None;
    }
}

const fn compute_preview_index(preset: GpgpuPreviewPreset) -> Option<usize> {
    match preset {
        GpgpuPreviewPreset::Mandelbrot => Some(0),
        GpgpuPreviewPreset::Chart => Some(1),
        GpgpuPreviewPreset::Plasma => Some(2),
        GpgpuPreviewPreset::All
        | GpgpuPreviewPreset::Static
        | GpgpuPreviewPreset::Static30
        | GpgpuPreviewPreset::Lab256
        | GpgpuPreviewPreset::CppGallery
        | GpgpuPreviewPreset::CppAurora
        | GpgpuPreviewPreset::CppJulia
        | GpgpuPreviewPreset::CppSdf
        | GpgpuPreviewPreset::CppVoronoi
        | GpgpuPreviewPreset::CppRetroSun
        | GpgpuPreviewPreset::CppAudio
        | GpgpuPreviewPreset::CppParticle
        | GpgpuPreviewPreset::CppFont
        | GpgpuPreviewPreset::CppFontRush => None,
    }
}

const fn preview_plane_slot(preset: GpgpuPreviewPreset) -> usize {
    if matches!(preset, GpgpuPreviewPreset::CppFontRush) {
        return 0;
    }
    if matches!(
        preset,
        GpgpuPreviewPreset::CppGallery
            | GpgpuPreviewPreset::CppAurora
            | GpgpuPreviewPreset::CppJulia
            | GpgpuPreviewPreset::CppSdf
            | GpgpuPreviewPreset::CppVoronoi
            | GpgpuPreviewPreset::CppRetroSun
            | GpgpuPreviewPreset::CppAudio
            | GpgpuPreviewPreset::CppParticle
    ) {
        return 1;
    }
    match compute_preview_index(preset) {
        Some(index) => index + 1,
        None => super::ALPHA_OVERLAY_PLANE_SLOT,
    }
}

fn active_preview_plane_slot(preview: &ActivePreview) -> usize {
    super::window_broker::window_snapshot(PREVIEW_OWNER, preview.window)
        .map(|window| window.plane.slot())
        .unwrap_or_else(|| {
            preview.font_rush.as_ref().map_or_else(
                || preview_plane_slot(preview.config.preset),
                |state| state.rank as usize,
            )
        })
}

fn next_preview_group_poll_ms(previews: &[ActivePreview]) -> u64 {
    previews
        .iter()
        .map(next_poll_ms)
        .min()
        .unwrap_or(IDLE_POLL_MS)
}

fn set_active_error(serial: u64, reason: &'static str) {
    let mut control = PREVIEW_CONTROL.lock();
    if control.status.applied_serial == serial {
        control.status.last_error = reason;
    }
}

const fn next_serial(serial: u64) -> u64 {
    let next = serial.wrapping_add(1);
    if next == 0 { 1 } else { next }
}

#[cfg(test)]
mod tests {
    use super::{
        FramePlanError, FramePoolError, GPGPU_PREVIEW_MAX_CADENCE_MS, GpgpuPreviewConfig,
        GpgpuPreviewPreset, LAB256_PREVIEW_SIZE, PREVIEW_HEIGHT, PREVIEW_WIDTH,
        cpp_font_rush_contiguous_plane_count, cpp_font_rush_due_tick_count,
        cpp_font_rush_glyph_count, cpp_font_rush_grid, cpp_font_rush_stamp_request,
        cpp_font_rush_target_plane_count, preview_extent, preview_frame_create_error_label,
        preview_plane_slot, static30_font_stamp_request,
    };

    #[test]
    fn lab256_preview_keeps_artifact_extent() {
        assert_eq!(
            preview_extent(GpgpuPreviewPreset::Lab256),
            (LAB256_PREVIEW_SIZE, LAB256_PREVIEW_SIZE)
        );
    }

    #[test]
    fn static30_builds_bounded_lorem_canvas_layers() {
        let request = static30_font_stamp_request(320, 180, 7);
        assert_eq!(request.fit, crate::r::font_kernel_service::FontStampFit::Canvas);
        assert_eq!(request.layers.len(), 2);
        assert!(request.layers.iter().all(|layer| {
            layer.scene.raster_width == 320
                && layer.scene.raster_height == 180
                && layer.scene.viewport_width == 320
                && layer.scene.viewport_height == 180
        }));
        assert_eq!(
            request
                .layers
                .iter()
                .flat_map(|layer| layer.scene.runs.iter())
                .map(|run| run.text.chars().count())
                .sum::<usize>(),
            53
        );
    }

    #[test]
    fn cpp_font_rush_maps_each_plane_to_its_requested_grid() {
        assert_eq!(cpp_font_rush_glyph_count(0), 1);
        assert_eq!(cpp_font_rush_grid(0), (1, 1));
        assert_eq!(cpp_font_rush_glyph_count(1), 2);
        assert_eq!(cpp_font_rush_grid(1), (2, 1));
        assert_eq!(cpp_font_rush_glyph_count(2), 4);
        assert_eq!(cpp_font_rush_grid(2), (2, 2));
        assert_eq!(cpp_font_rush_glyph_count(3), 16);
        assert_eq!(cpp_font_rush_grid(3), (4, 4));
    }

    #[test]
    fn cpp_font_rush_requires_contiguous_slots_from_primary() {
        assert_eq!(cpp_font_rush_contiguous_plane_count(0b0000), 0);
        assert_eq!(cpp_font_rush_contiguous_plane_count(0b0001), 1);
        assert_eq!(cpp_font_rush_contiguous_plane_count(0b0011), 2);
        assert_eq!(cpp_font_rush_contiguous_plane_count(0b0111), 3);
        assert_eq!(cpp_font_rush_contiguous_plane_count(0b1111), 4);
        assert_eq!(cpp_font_rush_contiguous_plane_count(0b1101), 1);
        assert_eq!(cpp_font_rush_contiguous_plane_count(0b1110), 0);
        assert_eq!(cpp_font_rush_contiguous_plane_count(u8::MAX), 4);
    }

    #[test]
    fn cpp_font_rush_adds_one_capability_bounded_plane_every_three_seconds() {
        assert_eq!(cpp_font_rush_target_plane_count(0, 4), 1);
        assert_eq!(cpp_font_rush_target_plane_count(2_999, 4), 1);
        assert_eq!(cpp_font_rush_target_plane_count(3_000, 4), 2);
        assert_eq!(cpp_font_rush_target_plane_count(5_999, 4), 2);
        assert_eq!(cpp_font_rush_target_plane_count(6_000, 4), 3);
        assert_eq!(cpp_font_rush_target_plane_count(9_000, 4), 4);
        assert_eq!(cpp_font_rush_target_plane_count(u64::MAX, 2), 2);
        assert_eq!(cpp_font_rush_target_plane_count(0, 0), 0);
    }

    #[test]
    fn cpp_font_rush_due_ticks_remain_phase_anchored() {
        assert_eq!(cpp_font_rush_due_tick_count(999, 1_000, 1_000), 0);
        assert_eq!(cpp_font_rush_due_tick_count(1_000, 1_000, 1_000), 1);
        assert_eq!(cpp_font_rush_due_tick_count(1_999, 1_000, 1_000), 1);
        assert_eq!(cpp_font_rush_due_tick_count(2_000, 1_000, 1_000), 2);
        assert_eq!(cpp_font_rush_due_tick_count(4_550, 1_000, 1_000), 4);
        assert_eq!(cpp_font_rush_due_tick_count(4_550, 3_000, 1_000), 2);
        assert_eq!(cpp_font_rush_due_tick_count(1_000, 1_000, 0), 0);
    }

    #[test]
    fn cpp_font_rush_batches_independent_grid_rolls_into_one_plane_layer() {
        for rank in 0..4u8 {
            let mut rng = crate::tyche::SoftRng::from_seed(0xC0DE_0000 + u64::from(rank));
            let (request, glyphs, grid) =
                cpp_font_rush_stamp_request(2_560, 1_440, rank, &mut rng).unwrap();
            assert_eq!(request.fit, crate::r::font_kernel_service::FontStampFit::Canvas);
            assert_eq!(request.layers.len(), 1);
            assert_eq!(glyphs, cpp_font_rush_glyph_count(rank));
            assert_eq!(grid, cpp_font_rush_grid(rank));
            let per_glyph_work_limit =
                crate::intel::gpu_font::gpu_font_analytical_work_limit() / glyphs as u64;
            let scene = &request.layers[0].scene;
            assert_eq!(scene.font, crate::intel::gpu_font::GpuFontFace::Default);
            assert_eq!(scene.viewport_width, 640);
            assert_eq!(scene.viewport_height, 360);
            assert_eq!(scene.raster_width, 2_560);
            assert_eq!(scene.raster_height, 1_440);
            assert_eq!(scene.runs.len(), glyphs);
            assert_eq!(
                scene.positioning,
                crate::r::font_kernel_service::RetainedFontPositioning::VisualBoundsCenter,
            );
            for run in &scene.runs {
                assert_eq!(run.text.chars().count(), 1,);
                assert!(run.font_pixels > 0.0);
                assert!(run.position[0].is_finite() && run.position[1].is_finite());
                let work = crate::intel::gpu_font::gpu_font_analytical_text_work_estimate(
                    run.text.as_str(),
                    scene.font,
                    run.font_pixels,
                    scene.viewport_width,
                    scene.viewport_height,
                    scene.raster_width,
                    scene.raster_height,
                )
                .unwrap();
                assert!(work <= per_glyph_work_limit);
            }
        }
    }

    #[test]
    fn cpp_font_rush_uses_primary_as_its_first_plane() {
        let mode = GpgpuPreviewPreset::CppFontRush;
        assert_eq!(preview_plane_slot(mode), 0);
        assert_eq!(mode.buffering_label(), "double-per-plane");
        assert_eq!(mode.plane_layout_label(), "slots0+1+2+3-direct-capability-bounded");
    }

    #[test]
    fn cpp_modes_share_the_resizable_slot1_application_surface() {
        let modes = [
            GpgpuPreviewPreset::CppGallery,
            GpgpuPreviewPreset::CppAurora,
            GpgpuPreviewPreset::CppJulia,
            GpgpuPreviewPreset::CppSdf,
            GpgpuPreviewPreset::CppVoronoi,
            GpgpuPreviewPreset::CppRetroSun,
            GpgpuPreviewPreset::CppAudio,
        ];
        for mode in modes {
            assert_eq!(preview_extent(mode), (PREVIEW_WIDTH, PREVIEW_HEIGHT));
            assert_eq!(preview_plane_slot(mode), 1);
            assert_eq!(mode.buffering_label(), "double");
            assert_eq!(mode.plane_layout_label(), "slot1-direct");
        }
    }

    #[test]
    fn particle_craft_starts_native_and_uses_the_resizable_cpp_plane() {
        let mode = GpgpuPreviewPreset::CppParticle;
        assert_eq!(
            preview_extent(mode),
            (
                crate::intel::gpgpu::PARTICLE_CRAFT_FRAME_WIDTH,
                crate::intel::gpgpu::PARTICLE_CRAFT_FRAME_HEIGHT,
            )
        );
        assert_eq!(preview_plane_slot(mode), 1);
        assert_eq!(mode.buffering_label(), "double");
        assert_eq!(mode.plane_layout_label(), "slot1-direct");
        assert_eq!(crate::intel::gpgpu::particle_craft_sample_extent(640, 400), (320, 200));
        let maximized_backing = super::preview_backing_extent(mode, 2560, 1440);
        assert_eq!(maximized_backing, (1280, 720));
        assert_eq!(
            crate::intel::gpgpu::particle_craft_sample_extent(
                maximized_backing.0,
                maximized_backing.1,
            ),
            maximized_backing,
        );
    }

    #[test]
    fn preview_config_accepts_continuous_duration() {
        assert!(
            GpgpuPreviewConfig {
                preset: GpgpuPreviewPreset::Plasma,
                duration_ms: 0,
                cadence_ms: 16,
                publish_every: 2,
            }
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn preview_config_rejects_invalid_scheduler_values() {
        assert!(
            GpgpuPreviewConfig {
                preset: GpgpuPreviewPreset::Mandelbrot,
                duration_ms: 1,
                cadence_ms: 0,
                publish_every: 1,
            }
            .validate()
            .is_err()
        );
        assert!(
            GpgpuPreviewConfig {
                preset: GpgpuPreviewPreset::Chart,
                duration_ms: 1,
                cadence_ms: GPGPU_PREVIEW_MAX_CADENCE_MS + 1,
                publish_every: 1,
            }
            .validate()
            .is_err()
        );
        assert!(
            GpgpuPreviewConfig {
                preset: GpgpuPreviewPreset::Plasma,
                duration_ms: 1,
                cadence_ms: 1,
                publish_every: 0,
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn preview_frame_create_errors_keep_stable_detail() {
        assert_eq!(
            preview_frame_create_error_label(FramePoolError::OutOfMemory),
            "frame-create-out-of-memory"
        );
        assert_eq!(
            preview_frame_create_error_label(FramePoolError::UnsupportedFormat),
            "frame-create-unsupported-format"
        );
        assert_eq!(
            preview_frame_create_error_label(FramePoolError::InvalidPlan(
                FramePlanError::EmptyExtent
            )),
            "frame-create-invalid-plan-empty-extent"
        );
    }
}
