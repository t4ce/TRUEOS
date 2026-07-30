//! Shell-controlled GPGPU live previews backed exclusively by UI4 frames.
//!
//! This is a trusted kernel app beside the permanent UI4 compositor. It
//! owns frame/window lifetime and compute cadence. The compute trio is admitted
//! through one broker session onto dedicated universal-plane slots; standalone
//! C++/IGC demos reuse the same exact-surface publication lifecycle on slot 1.
//! Display pipe programming remains exclusively compositor-owned.

use alloc::{string::String, vec::Vec};

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
const CPP_FONT_GO_WINDOWS: usize = 4;
const CPP_FONT_GO_DURATION_MS: u64 = 60_000;
const CPP_FONT_GO_CADENCE_MS: u64 = 1_000;
const CPP_FONT_GO_STAGE_MS: u64 = 10_000;

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
    CppFontGo,
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
            Self::CppFontGo => "cpp-font-go",
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
                | Self::CppFontGo
        )
    }

    pub(crate) const fn is_resizable_cpp(self) -> bool {
        self.is_cpp() && !matches!(self, Self::CppFont | Self::CppFontGo)
    }

    pub(crate) const fn buffering_label(self) -> &'static str {
        match self {
            Self::All => "double-per-frame",
            Self::Static30 | Self::CppFont => "single",
            Self::CppFontGo => "double-per-frame",
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
            Self::CppFontGo => "slots1+2+3/2x2-composed",
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
    pub(crate) dropped_busy: u64,
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
    pub(crate) members: [GpgpuPreviewMemberStatus; 3],
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
                dropped_busy: 0,
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

const INACTIVE_PREVIEW_MEMBERS: [GpgpuPreviewMemberStatus; 3] = [
    GpgpuPreviewMemberStatus::inactive(GpgpuPreviewPreset::Mandelbrot, 1),
    GpgpuPreviewMemberStatus::inactive(GpgpuPreviewPreset::Chart, 2),
    GpgpuPreviewMemberStatus::inactive(GpgpuPreviewPreset::Plasma, 3),
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
                dropped_busy: 0,
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
    font_go_quadrant: Option<u8>,
    font_go_stage: u8,
    metrics: GpgpuPreviewMetrics,
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
    control.status.request_serial = serial;
    control.status.config = config;
    control.status.width = width;
    control.status.height = height;
    control.status.last_error = "none";
    Ok(serial)
}

pub(crate) fn request_cpp_font_go_start() -> Result<u64, &'static str> {
    if !crate::r::font_kernel_service::status().online {
        return Err("font-service-offline");
    }
    request_gpgpu_preview_start(GpgpuPreviewConfig {
        preset: GpgpuPreviewPreset::CppFontGo,
        duration_ms: CPP_FONT_GO_DURATION_MS,
        cadence_ms: CPP_FONT_GO_CADENCE_MS,
        publish_every: 1,
    })
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
    crate::intel::wait_hw_logo_sequence_done().await;
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

        let desired = PREVIEW_CONTROL.lock().desired;
        if desired.serial != applied_serial {
            if !active.is_empty() {
                stop_active_previews(
                    core::mem::take(&mut active),
                    &mut retired_frames,
                    "command-replaced",
                );
            }
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
        for preview in &mut active {
            if render_fault.is_some() {
                break;
            }
            preview.metrics.elapsed_ms = now.saturating_duration_since(preview.started).as_millis();
            duration_expired |= preview.config.duration_ms != 0
                && preview.metrics.elapsed_ms >= preview.config.duration_ms;
            if !duration_expired && preview_needs_render(preview) && now >= preview.next_render {
                match render_preview_frame(preview).await {
                    Ok(()) => {
                        schedule_next_render(preview);
                    }
                    Err(reason) => render_fault = Some(reason),
                }
            }
        }
        if !active.is_empty() && render_fault.is_none() && !duration_expired {
            publish_active_status(&active, GpgpuPreviewPhase::Running, "none");
        }

        if let Some(reason) = render_fault {
            if !active.is_empty() {
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
    } else if desired.config.preset == GpgpuPreviewPreset::CppFontGo {
        initialize_cpp_font_go_set(desired)
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
            font_go_quadrant: None,
            font_go_stage: u8::MAX,
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

fn initialize_cpp_font_go_set(desired: DesiredPreview) -> Result<Vec<ActivePreview>, &'static str> {
    let output = OutputId::from_slot(0).ok_or("output-d01-unavailable")?;
    let session =
        begin_window_session(PREVIEW_OWNER).map_err(|_| "font-go-session-create-failed")?;
    let (output_width, output_height) =
        crate::intel::active_scanout_dimensions().unwrap_or((2560, 1440));
    let width = output_width / 2;
    let height = output_height / 2;
    if width == 0 || height == 0 {
        let _ = finish_window_session(PREVIEW_OWNER, session);
        return Err("font-go-output-too-small");
    }
    let mut previews = Vec::with_capacity(CPP_FONT_GO_WINDOWS);
    for index in 0..CPP_FONT_GO_WINDOWS {
        let (x, y, tile_width, tile_height, plane_slot) =
            cpp_font_go_tile(output_width, output_height, index);
        let frame = match create_cpp_font_go_frame(output, width, height, index as u8) {
            Ok(frame) => frame,
            Err(_) => {
                abandon_compute_preview_initialization(session, &previews);
                return Err("font-go-frame-create-failed");
            }
        };
        let window = match create_window(WindowCreate {
            owner: PREVIEW_OWNER,
            session,
            frame,
            output,
            plane: WindowPlane::Universal(plane_slot as u8),
            placement: WindowPlacement {
                x: x as i32,
                y: y as i32,
                width: tile_width,
                height: tile_height,
                z: PREVIEW_Z.saturating_add(index as i32),
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
                abandon_compute_preview_initialization(session, &previews);
                return Err("font-go-window-create-failed");
            }
        };
        let now = Instant::now();
        previews.push(ActivePreview {
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
            font_go_quadrant: Some(index as u8),
            font_go_stage: u8::MAX,
            metrics: GpgpuPreviewMetrics::default(),
        });
    }
    Ok(previews)
}

fn cpp_font_go_tile(
    output_width: u32,
    output_height: u32,
    index: usize,
) -> (u32, u32, u32, u32, usize) {
    let width = output_width / 2;
    let height = output_height / 2;
    let column = index as u32 % 2;
    let row = index as u32 / 2;
    (column.saturating_mul(width), row.saturating_mul(height), width, height, index % 3 + 1)
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
        font_go_quadrant: None,
        font_go_stage: u8::MAX,
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
        font_go_quadrant: None,
        font_go_stage: u8::MAX,
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
        font_go_quadrant: None,
        font_go_stage: u8::MAX,
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

fn create_cpp_font_go_frame(
    output: OutputId,
    width: u32,
    height: u32,
    quadrant: u8,
) -> Result<FrameHandle, FramePoolError> {
    create_frame(FrameSpec {
        output,
        content: FrameContent::FontScene2d,
        cadence: FrameCadence::Dirty,
        buffering: super::FrameBuffering::Double,
        format: ScanoutFormat::Rgba8888Premultiplied,
        width,
        height,
        base_color: Some(cpp_font_go_background(quadrant, 0)),
    })
}

async fn render_preview_frame(preview: &mut ActivePreview) -> Result<(), &'static str> {
    if preview.config.preset == GpgpuPreviewPreset::CppFont {
        return render_cpp_font_frame(preview).await;
    }
    if preview.config.preset == GpgpuPreviewPreset::CppFontGo {
        return render_cpp_font_go_frame(preview).await;
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
        | GpgpuPreviewPreset::CppFontGo => publish_frame_buffer(lease),
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

async fn render_cpp_font_go_frame(preview: &mut ActivePreview) -> Result<(), &'static str> {
    use crate::intel::gpgpu::{GpgpuSolidRect, GpgpuSubmissionOutcome};
    use crate::r::font_kernel_service::{
        FontKernelConsumer, FontKernelConsumerPath, FontKernelError,
    };

    let quadrant = preview.font_go_quadrant.ok_or("font-go-quadrant-missing")?;
    let stage = (preview.metrics.elapsed_ms / CPP_FONT_GO_STAGE_MS).min(5) as u8;
    preview.metrics.attempted = preview.metrics.attempted.saturating_add(1);
    let sequence = preview.metrics.attempted;
    let lease = match acquire_frame_buffer(preview.frame) {
        Ok(lease) => lease,
        Err(FramePoolError::Busy) => {
            preview.metrics.dropped_busy = preview.metrics.dropped_busy.saturating_add(1);
            return Ok(());
        }
        Err(_) => {
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            return Err("font-go-frame-acquire-failed");
        }
    };
    let destination = match gpgpu_rgba_surface(lease) {
        Ok(destination) => destination,
        Err(_) => {
            let _ = cancel_frame_buffer(lease);
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            return Err("font-go-surface-unavailable");
        }
    };

    // A dirty font frame is reused. Clear its exact back buffer on the same
    // fair direct-RCS lane before source-over glyph stamping, so prior samples
    // never ghost into a later variation and no CPU frame write is introduced.
    let clear_rgba =
        u32::from_le_bytes(cpp_font_go_background(quadrant, sequence).to_native_bytes());
    let clear = GpgpuSolidRect {
        rect: destination.bounds(),
        color_rgba: clear_rgba,
    };
    let clear_result = {
        let _lane = crate::r::font_kernel_service::acquire_gpu_lane(FontKernelConsumer::new(
            FontKernelConsumerPath::Stamp,
            preview
                .request_serial
                .rotate_left(8)
                .wrapping_add(u64::from(quadrant)),
        ))
        .await;
        crate::intel::gpgpu::fill_solid_rects_rgba8_scanout_result(
            destination,
            core::slice::from_ref(&clear),
        )
    };
    match clear_result.outcome {
        GpgpuSubmissionOutcome::Complete => {}
        GpgpuSubmissionOutcome::SubmittedIncomplete => {
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            return Err("font-go-clear-submit-incomplete");
        }
        GpgpuSubmissionOutcome::Unavailable => {
            let _ = cancel_frame_buffer(lease);
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            return Err("font-go-clear-unavailable");
        }
    }

    let (request, requested_glyphs, layers, runs) =
        cpp_font_go_stamp_request(preview.width, preview.height, quadrant, stage, sequence);
    let pending = match crate::r::font_kernel_service::submit_frame_stamp(request, destination) {
        Ok(pending) => pending,
        Err(FontKernelError::QueueFull) => {
            let _ = cancel_frame_buffer(lease);
            preview.metrics.dropped_busy = preview.metrics.dropped_busy.saturating_add(1);
            return Ok(());
        }
        Err(_) => {
            let _ = cancel_frame_buffer(lease);
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            return Err("font-go-submit-failed");
        }
    };
    let stamped = match pending.wait().await {
        Ok(stamped) => stamped,
        Err(FontKernelError::SubmittedIncomplete(_)) => {
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            return Err("font-go-submit-incomplete");
        }
        Err(_) => {
            let _ = cancel_frame_buffer(lease);
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            return Err("font-go-stamp-failed");
        }
    };
    preview.metrics.submitted = preview.metrics.submitted.saturating_add(1);
    preview.metrics.completed = preview.metrics.completed.saturating_add(1);
    if publish_gpu_font_frame_buffer(lease, stamped.release()).is_err() {
        preview.metrics.failed = preview.metrics.failed.saturating_add(1);
        return Err("font-go-frame-publish-failed");
    }
    if publish_window_frame(PREVIEW_OWNER, preview.window, DamageRect::FULL).is_err() {
        preview.metrics.failed = preview.metrics.failed.saturating_add(1);
        return Err("font-go-window-publish-failed");
    }
    preview.metrics.published = preview.metrics.published.saturating_add(1);
    preview.metrics.last_iterations = stamped.glyphs() as u32;
    preview.metrics.last_marker = stamped.release().sequence() as u32;
    if preview.font_go_stage != stage {
        preview.font_go_stage = stage;
        crate::log_info!(
            target: "ui4";
            "ui4 cpp-font-go ramp request={} quadrant={} stage={}/5 elapsed_ms={} extent={}x{} cadence_ms={} requested_glyphs={} rendered_glyphs={} layers={} runs={} clear_submits={} font_submits={} walkers={} release={} path=gpu-clear->skrifa->gpu-vm-r8->cpp-igc->guc-rcs->ui4-font-scene cpu_readback=0 cpu_frame_copy=0\n",
            preview.request_serial,
            quadrant,
            stage,
            preview.metrics.elapsed_ms,
            preview.width,
            preview.height,
            preview.config.cadence_ms,
            requested_glyphs,
            stamped.glyphs(),
            layers,
            runs,
            clear_result.stats.submits,
            stamped.submits(),
            stamped.active_walkers(),
            stamped.release().sequence(),
        );
    }
    Ok(())
}

fn cpp_font_go_background(quadrant: u8, sequence: u64) -> PremultipliedRgba8 {
    let pulse = (sequence as u8).wrapping_mul(3) % 18;
    match quadrant % 4 {
        0 => PremultipliedRgba8::from_straight_rgba(8, 18 + pulse, 38, u8::MAX),
        1 => PremultipliedRgba8::from_straight_rgba(28, 8, 34 + pulse, u8::MAX),
        2 => PremultipliedRgba8::from_straight_rgba(7, 30 + pulse, 25, u8::MAX),
        _ => PremultipliedRgba8::from_straight_rgba(34 + pulse, 18, 7, u8::MAX),
    }
}

fn cpp_font_go_stamp_request(
    width: u32,
    height: u32,
    quadrant: u8,
    stage: u8,
    sequence: u64,
) -> (crate::r::font_kernel_service::FontStampRequest, usize, usize, usize) {
    use crate::r::font_kernel_service::{
        FontStampFit, FontStampLayer, FontStampRequest, RetainSceneRequest,
        RetainedFontPositioning, RetainedFontRun,
    };

    const GLYPH_TARGETS: [usize; 6] = [192, 320, 512, 768, 1_024, 1_408];
    const LAYER_COUNTS: [usize; 6] = [3, 4, 6, 8, 10, 12];
    const RUNS_PER_LAYER: [usize; 6] = [2, 3, 4, 4, 5, 5];
    let stage_index = usize::from(stage.min(5));
    let glyph_target = GLYPH_TARGETS[stage_index];
    let layer_count = LAYER_COUNTS[stage_index];
    let runs_per_layer = RUNS_PER_LAYER[stage_index];
    let total_runs = layer_count.saturating_mul(runs_per_layer);
    let mut remaining_glyphs = glyph_target;
    let mut remaining_runs = total_runs;
    let mut global_run = 0usize;
    let mut layers = Vec::with_capacity(layer_count);
    let usable_y = height.saturating_sub(72).max(1);
    let usable_x = width.saturating_sub(360).max(1);

    for layer_index in 0..layer_count {
        let font = match (layer_index + usize::from(quadrant) + sequence as usize) % 3 {
            0 => crate::intel::gpu_font::GpuFontFace::Default,
            1 => crate::intel::gpu_font::GpuFontFace::NotoSansSc,
            _ => crate::intel::gpu_font::GpuFontFace::Inconsolata,
        };
        let color_seed = (layer_index as u16)
            .wrapping_mul(37)
            .wrapping_add(u16::from(quadrant) * 53)
            .wrapping_add(sequence as u16 * 7);
        let foreground = crate::intel::gpu_font::GpuFontRgba::new(
            (96 + color_seed % 160) as u8,
            (112 + color_seed.wrapping_mul(3) % 144) as u8,
            (128 + color_seed.wrapping_mul(5) % 128) as u8,
            (176 + color_seed % 80) as u8,
        );
        let mut runs = Vec::with_capacity(runs_per_layer);
        for run_index in 0..runs_per_layer {
            let run_glyphs = remaining_glyphs.div_ceil(remaining_runs);
            let seed = sequence
                .wrapping_mul(131)
                .wrapping_add(global_run as u64 * 29)
                .wrapping_add(u64::from(quadrant) * 997);
            let header = if global_run == 0 {
                Some(alloc::format!(
                    "CPP FONT GO  Q{}  STAGE {}  {} GLYPHS  ",
                    quadrant + 1,
                    stage,
                    glyph_target,
                ))
            } else {
                None
            };
            let text = cpp_font_go_text(run_glyphs, seed, header);
            let position_seed = global_run as u32 * 71
                + layer_index as u32 * 43
                + run_index as u32 * 19
                + sequence as u32 * 11;
            let large = global_run.is_multiple_of(7);
            let font_pixels = if large {
                58.0 + f32::from(stage) * 8.0 + f32::from(quadrant) * 3.0
            } else {
                18.0 + (position_seed % (26 + u32::from(stage) * 5)) as f32
            };
            runs.push(RetainedFontRun {
                text,
                position: [
                    18.0 + (position_seed % usable_x) as f32,
                    42.0 + (global_run as u32)
                        .saturating_mul(usable_y)
                        .checked_div(total_runs as u32)
                        .unwrap_or(0) as f32,
                ],
                font_pixels,
                slant: ((position_seed % 9) as f32 - 4.0) / 5.0,
            });
            remaining_glyphs = remaining_glyphs.saturating_sub(run_glyphs);
            remaining_runs = remaining_runs.saturating_sub(1);
            global_run = global_run.saturating_add(1);
        }
        layers.push(FontStampLayer {
            scene: RetainSceneRequest {
                runs,
                font,
                viewport_width: width,
                viewport_height: height,
                raster_width: width,
                raster_height: height,
                positioning: RetainedFontPositioning::SceneOrigin,
            },
            foreground,
        });
    }

    (
        FontStampRequest {
            layers,
            fit: FontStampFit::Canvas,
        },
        glyph_target,
        layer_count,
        total_runs,
    )
}

fn cpp_font_go_text(length: usize, seed: u64, header: Option<String>) -> String {
    const SAMPLE: &[u8] = b"Lorem ipsum FONT kernel retained stamp Alpha beta 0123456789 ";
    let mut text = header.unwrap_or_else(|| String::with_capacity(length));
    if text.len() > length {
        text.truncate(length);
        return text;
    }
    while text.len() < length {
        let index = (seed as usize).wrapping_add(text.len()) % SAMPLE.len();
        text.push(SAMPLE[index] as char);
    }
    text
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
        | GpgpuPreviewPreset::CppFontGo => PreviewDispatchResult {
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
        GpgpuPreviewPreset::CppFont | GpgpuPreviewPreset::CppFontGo => {
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
        GpgpuPreviewPreset::CppFont | GpgpuPreviewPreset::CppFontGo => "font-kernel-service-cpp",
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
        | GpgpuPreviewPreset::CppFont
        | GpgpuPreviewPreset::CppFontGo => WindowPlane::Universal(preview_plane_slot(preset) as u8),
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
        GpgpuPreviewPreset::CppFontGo => "ui4-font-scene-2x2",
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
        "ui4 gpgpu-preview stopped request={} preset={} frames={} windows={} attempted={} completed={} published={} dropped_busy={} failed={} late={} elapsed_ms={} reason={} teardown={} frame_retire=after-surflive-display-lease-drain source_buffer_mutation=none\n",
        first.request_serial,
        first.config.preset.label(),
        previews.len(),
        previews.iter().map(preview_surface_count).sum::<usize>(),
        metrics.attempted,
        metrics.completed,
        metrics.published,
        metrics.dropped_busy,
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
            "ui4 gpgpu-preview stopped-member request={} preset={} frame={} window={} slot={} attempted={} completed={} published={} dropped_busy={} failed={} late={} elapsed_ms={} reason={}\n",
            preview.request_serial,
            preview.config.preset.label(),
            preview.frame.raw(),
            preview.window.raw(),
            active_preview_plane_slot(&preview),
            preview.metrics.attempted,
            preview.metrics.completed,
            preview.metrics.published,
            preview.metrics.dropped_busy,
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
    mut members: [GpgpuPreviewMemberStatus; 3],
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
    mut members: [GpgpuPreviewMemberStatus; 3],
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
        aggregate.dropped_busy = aggregate
            .dropped_busy
            .saturating_add(preview.metrics.dropped_busy);
        aggregate.failed = aggregate.failed.saturating_add(preview.metrics.failed);
        aggregate.late = aggregate.late.saturating_add(preview.metrics.late);
        aggregate.elapsed_ms = aggregate.elapsed_ms.max(preview.metrics.elapsed_ms);
    }
    aggregate
}

fn preview_member_statuses(previews: &[ActivePreview]) -> [GpgpuPreviewMemberStatus; 3] {
    let mut members = INACTIVE_PREVIEW_MEMBERS;
    for preview in previews {
        let Some(index) = compute_preview_index(preview.config.preset) else {
            continue;
        };
        members[index] = GpgpuPreviewMemberStatus {
            preset: preview.config.preset,
            frame: Some(preview.frame),
            window: Some(preview.window),
            plane_slot: (index + 1) as u8,
            metrics: preview.metrics,
        };
    }
    members
}

fn clear_preview_member_handles(members: &mut [GpgpuPreviewMemberStatus; 3]) {
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
        | GpgpuPreviewPreset::CppFontGo => None,
    }
}

const fn preview_plane_slot(preset: GpgpuPreviewPreset) -> usize {
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
    preview.font_go_quadrant.map_or_else(
        || preview_plane_slot(preview.config.preset),
        |quadrant| usize::from(quadrant) % 3 + 1,
    )
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
        cpp_font_go_stamp_request, cpp_font_go_tile, preview_extent,
        preview_frame_create_error_label, preview_plane_slot, static30_font_stamp_request,
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
    fn cpp_font_go_tiles_1440p_into_four_equal_frames() {
        assert_eq!(cpp_font_go_tile(2560, 1440, 0), (0, 0, 1280, 720, 1));
        assert_eq!(cpp_font_go_tile(2560, 1440, 1), (1280, 0, 1280, 720, 2));
        assert_eq!(cpp_font_go_tile(2560, 1440, 2), (0, 720, 1280, 720, 3));
        assert_eq!(cpp_font_go_tile(2560, 1440, 3), (1280, 720, 1280, 720, 1));
    }

    #[test]
    fn cpp_font_go_ramp_stays_bounded_and_increases_work() {
        let expected_glyphs = [192usize, 320, 512, 768, 1_024, 1_408];
        let expected_layers = [3usize, 4, 6, 8, 10, 12];
        let mut previous_runs = 0usize;
        for stage in 0..6u8 {
            let (request, glyphs, layers, runs) =
                cpp_font_go_stamp_request(1280, 720, 2, stage, 17);
            assert_eq!(request.fit, crate::r::font_kernel_service::FontStampFit::Canvas);
            assert_eq!(glyphs, expected_glyphs[stage as usize]);
            assert_eq!(layers, expected_layers[stage as usize]);
            assert_eq!(request.layers.len(), layers);
            assert!(runs >= previous_runs);
            assert!(glyphs <= crate::r::font_kernel_service::FONT_STAMP_MAX_GLYPHS);
            assert_eq!(
                request
                    .layers
                    .iter()
                    .flat_map(|layer| layer.scene.runs.iter())
                    .map(|run| run.text.chars().count())
                    .sum::<usize>(),
                glyphs
            );
            assert!(request.layers.iter().all(|layer| {
                layer.scene.raster_width == 1280
                    && layer.scene.raster_height == 720
                    && layer
                        .scene
                        .runs
                        .iter()
                        .all(|run| run.slant.abs() <= 1.0 && run.font_pixels > 0.0)
            }));
            previous_runs = runs;
        }
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
