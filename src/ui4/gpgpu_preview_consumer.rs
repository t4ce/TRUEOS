//! Shell-controlled GPGPU live previews backed exclusively by UI4 frames.
//!
//! This is a trusted kernel app beside the permanent UI4 compositor. It
//! owns frame/window lifetime and compute cadence, but deliberately knows
//! nothing about display pipes or universal-plane slots.  Published windows
//! are ordinary inputs to the existing UI4 compositor.

use alloc::vec::Vec;

use embassy_time::{Duration, Instant, Timer};
use spin::Mutex;

use super::{
    DamageRect, FrameCadence, FrameContent, FrameHandle, FramePlanError, FramePoolError, FrameSpec,
    FrameWriteLease, OutputId, PremultipliedRgba8, ScanoutFormat, Ui4InputEvent, WindowCreate,
    WindowId, WindowOwner, WindowPlacement, WindowPlane, WindowSessionCloseRequest,
    WindowSessionId, acquire_frame_buffer, begin_window_session, cancel_frame_buffer, create_frame,
    create_window, destroy_frame, finish_window_session, finish_window_session_with_request,
    gpgpu_rgba_surface, publish_frame_buffer, publish_window_frame, replace_window_frame,
    take_owner_input_events, writable_rgba_view,
};

const PREVIEW_OWNER: WindowOwner = WindowOwner::GPGPU_PREVIEW;
const PREVIEW_WIDTH: u32 = super::DEFAULT_FRAME_WIDTH;
const PREVIEW_HEIGHT: u32 = super::DEFAULT_FRAME_HEIGHT;
const PREVIEW_MARGIN: u32 = 64;
const PREVIEW_Z: i32 = 30;
const IDLE_POLL_MS: u64 = 20;
const COMMAND_POLL_MAX_MS: u64 = 10;
const STATIC30_FRAME_COUNT: usize = 30;
const STATIC30_PLANE_COUNT: usize = 3;
const STATIC30_COLUMNS: u32 = 6;
const STATIC30_ROWS: u32 = 5;
const STATIC30_MAX_WIDTH: u32 = 320;
const STATIC30_MAX_HEIGHT: u32 = 180;

pub(crate) const GPGPU_PREVIEW_DEFAULT_DURATION_MS: u64 = 5_000;
pub(crate) const GPGPU_PREVIEW_DEFAULT_CADENCE_MS: u64 = 33;
pub(crate) const GPGPU_PREVIEW_DEFAULT_PUBLISH_EVERY: u32 = 1;
pub(crate) const GPGPU_PREVIEW_MAX_CADENCE_MS: u64 = 60_000;
pub(crate) const GPGPU_PREVIEW_MAX_PUBLISH_EVERY: u32 = 1_024;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum GpgpuPreviewPreset {
    Static,
    Static30,
    Mandelbrot,
    Chart,
    Plasma,
}

impl GpgpuPreviewPreset {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Static => "static",
            Self::Static30 => "static30",
            Self::Mandelbrot => "mandelbrot",
            Self::Chart => "chart",
            Self::Plasma => "plasma",
        }
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
        preset: GpgpuPreviewPreset::Mandelbrot,
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
    pub(crate) metrics: GpgpuPreviewMetrics,
    pub(crate) last_error: &'static str,
}

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
            last_error: "none",
        }
    }
}

#[derive(Copy, Clone)]
struct DesiredPreview {
    serial: u64,
    running: bool,
    config: GpgpuPreviewConfig,
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
            },
            status: GpgpuPreviewStatus::initial(),
        }
    }
}

static PREVIEW_CONTROL: Mutex<PreviewControl> = Mutex::new(PreviewControl::new());

struct ActivePreview {
    request_serial: u64,
    config: GpgpuPreviewConfig,
    session: WindowSessionId,
    frame: FrameHandle,
    window: WindowId,
    width: u32,
    height: u32,
    started: Instant,
    next_render: Instant,
    static_needs_publish: bool,
    extra_sessions: Vec<WindowSessionId>,
    extra_surfaces: Vec<StaticPreviewSurface>,
    metrics: GpgpuPreviewMetrics,
}

#[derive(Copy, Clone)]
struct StaticPreviewSurface {
    frame: FrameHandle,
    window: WindowId,
    scheme: u8,
}

pub(crate) fn request_gpgpu_preview_start(config: GpgpuPreviewConfig) -> Result<u64, &'static str> {
    let config = config.validate()?;
    let mut control = PREVIEW_CONTROL.lock();
    let serial = next_serial(control.desired.serial);
    control.desired = DesiredPreview {
        serial,
        running: true,
        config,
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
    serial
}

pub(crate) fn gpgpu_preview_status() -> GpgpuPreviewStatus {
    PREVIEW_CONTROL.lock().status
}

#[embassy_executor::task(pool_size = 1)]
pub(crate) async fn gpgpu_preview_consumer_service_task(worker_slot: u32) {
    crate::log_info!(
        target: "ui4";
        "ui4 gpgpu-preview-consumer carrier online owner={:?} placement=worker-ap2+ assigned_slot={} current_slot={} display_api=none activation=Shell2/on-demand buffering=double compute_release=completion-marker-before-publish interaction=movable-fixed-size\n",
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
    let mut active: Option<ActivePreview> = None;
    let mut retired_frames = Vec::new();

    loop {
        retire_frames(&mut retired_frames);
        drain_preview_input(&mut active, &mut retired_frames);

        let desired = PREVIEW_CONTROL.lock().desired;
        if desired.serial != applied_serial {
            if let Some(previous) = active.take() {
                stop_active_preview(previous, "command-replaced");
            }
            applied_serial = desired.serial;
            if desired.running {
                mark_starting(desired);
                match initialize_preview(desired) {
                    Ok(preview) => {
                        crate::log_info!(
                            target: "ui4";
                            "ui4 gpgpu-preview start request={} preset={} producer={} owner={:?} frame={} window={} windows={} extent={}x{} cadence_ms={} publish_every={} duration_ms={} buffering={} release={} plane_layout={} plane_mutation=none\n",
                            desired.serial,
                            preview.config.preset.label(),
                            preview_producer_label(preview.config.preset),
                            PREVIEW_OWNER,
                            preview.frame.raw(),
                            preview.window.raw(),
                            preview_surface_count(&preview),
                            preview.width,
                            preview.height,
                            preview.config.cadence_ms,
                            preview.config.publish_every,
                            preview.config.duration_ms,
                            preview_buffering_label(preview.config.preset),
                            preview_release_label(preview.config.preset),
                            preview_plane_layout(preview.config.preset),
                        );
                        publish_active_status(&preview, GpgpuPreviewPhase::Running, "none");
                        active = Some(preview);
                    }
                    Err(reason) => {
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
                mark_idle(applied_serial, "stopped");
            }
        }

        let now = Instant::now();
        let mut duration_expired = false;
        let mut render_fault = None;
        if let Some(preview) = active.as_mut() {
            preview.metrics.elapsed_ms = now.saturating_duration_since(preview.started).as_millis();
            duration_expired = preview.config.duration_ms != 0
                && preview.metrics.elapsed_ms >= preview.config.duration_ms;
            if !duration_expired && preview_needs_render(preview) && now >= preview.next_render {
                match render_preview_frame(preview) {
                    Ok(()) => {
                        schedule_next_render(preview);
                        publish_active_status(preview, GpgpuPreviewPhase::Running, "none");
                    }
                    Err(reason) => render_fault = Some(reason),
                }
            }
        }

        if let Some(reason) = render_fault {
            if let Some(failed) = active.take() {
                let serial = failed.request_serial;
                let metrics = failed.metrics;
                stop_active_preview(failed, "render-fault");
                mark_runtime_fault(serial, metrics, reason);
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
            if let Some(finished) = active.take() {
                let serial = finished.request_serial;
                let metrics = finished.metrics;
                stop_active_preview(finished, "duration-complete");
                mark_duration_complete(serial, metrics);
            }
        }

        let wait_ms = active.as_ref().map(next_poll_ms).unwrap_or(IDLE_POLL_MS);
        Timer::after(Duration::from_millis(wait_ms)).await;
    }
}

fn initialize_preview(desired: DesiredPreview) -> Result<ActivePreview, &'static str> {
    if desired.config.preset == GpgpuPreviewPreset::Static30 {
        return initialize_static30_preview(desired);
    }
    let output = OutputId::from_slot(0).ok_or("output-d01-unavailable")?;
    let frame = create_preview_frame(output, PREVIEW_WIDTH, PREVIEW_HEIGHT)
        .map_err(preview_frame_create_error_label)?;
    let session = match begin_window_session(PREVIEW_OWNER) {
        Ok(session) => session,
        Err(_) => {
            let _ = destroy_frame(frame);
            return Err("window-session-create-failed");
        }
    };
    let (scanout_width, _) =
        crate::intel::active_scanout_dimensions().unwrap_or((PREVIEW_WIDTH, PREVIEW_HEIGHT));
    let x = scanout_width.saturating_sub(PREVIEW_WIDTH.saturating_add(PREVIEW_MARGIN)) as i32;
    let window = match create_window(WindowCreate {
        owner: PREVIEW_OWNER,
        session,
        frame,
        output,
        // The static checkpoint must not accidentally invoke the compute
        // compositor. A sole opaque CPU-authored window on slot 1 is eligible
        // for UI4's direct GGTT-import + hardware-plane-flip path. Animated
        // presets retain their existing primary composition path until that
        // shader path is repaired independently.
        plane: preview_plane(desired.config.preset),
        placement: WindowPlacement {
            x,
            y: PREVIEW_MARGIN as i32,
            width: PREVIEW_WIDTH,
            height: PREVIEW_HEIGHT,
            z: PREVIEW_Z,
            opacity: u8::MAX,
            visible: true,
        },
        // This checkpoint restores compute-frame publication and broker-level
        // motion only. Dynamic resize/maximize remains parked until the
        // fixed-size double-buffer path is proven under composition.
        interaction: super::WindowInteraction::MOVABLE_FRAME,
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
        session,
        frame,
        window,
        width: PREVIEW_WIDTH,
        height: PREVIEW_HEIGHT,
        started: now,
        next_render: now,
        static_needs_publish: true,
        extra_sessions: Vec::new(),
        extra_surfaces: Vec::new(),
        metrics: GpgpuPreviewMetrics::default(),
    })
}

const fn preview_frame_create_error_label(error: FramePoolError) -> &'static str {
    match error {
        FramePoolError::InvalidPlan(FramePlanError::EmptyExtent) => {
            "frame-create-invalid-plan-empty-extent"
        }
        FramePoolError::InvalidPlan(FramePlanError::BaseColorRequiresPremultipliedRgba) => {
            "frame-create-invalid-plan-base-color-format"
        }
        FramePoolError::InvalidPlan(FramePlanError::VideoRequiresRgbaOrNv12) => {
            "frame-create-invalid-plan-video-format"
        }
        FramePoolError::InvalidPlan(FramePlanError::Nv12RequiresVideo) => {
            "frame-create-invalid-plan-nv12-content"
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
        format: ScanoutFormat::Rgba8888Premultiplied,
        width,
        height,
        base_color: Some(PremultipliedRgba8::from_straight_rgba(0, 0, 0, u8::MAX)),
    })
}

fn render_preview_frame(preview: &mut ActivePreview) -> Result<(), &'static str> {
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
    if preview.config.preset == GpgpuPreviewPreset::Static {
        if let Err(reason) = fill_static_preview_frame(lease) {
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
        preview.metrics.last_iterations = result.iterations;
        preview.metrics.last_marker = result.marker;
        if result.submitted {
            preview.metrics.submitted = preview.metrics.submitted.saturating_add(1);
        }
        preview.metrics.last_submit_ms = result.submit_ms;
        if !result.ok {
            let _ = cancel_frame_buffer(lease);
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            return Err(result.error);
        }
    }
    preview.metrics.completed = preview.metrics.completed.saturating_add(1);

    if !publish_this_frame {
        let _ = cancel_frame_buffer(lease);
        return Ok(());
    }
    if publish_frame_buffer(lease).is_err() {
        preview.metrics.failed = preview.metrics.failed.saturating_add(1);
        return Err("frame-publish-failed");
    }
    if publish_window_frame(PREVIEW_OWNER, preview.window, DamageRect::FULL).is_err() {
        preview.metrics.failed = preview.metrics.failed.saturating_add(1);
        return Err("window-publish-failed");
    }
    preview.metrics.published = preview.metrics.published.saturating_add(1);
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

/// Paint one unmistakable CPU-authored frame. This path deliberately does not
/// request a GPGPU surface, upload a kernel, submit through GuC, or poll a GPU
/// completion marker. The cache release is the only producer-side operation
/// between the CPU writes and ordinary UI4 publication.
fn fill_static_preview_frame(lease: FrameWriteLease) -> Result<(), &'static str> {
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

    let navy = PremultipliedRgba8::from_straight_rgba(8, 24, 64, u8::MAX).to_native_bytes();
    let blue = PremultipliedRgba8::from_straight_rgba(24, 104, 224, u8::MAX).to_native_bytes();
    let cyan = PremultipliedRgba8::from_straight_rgba(16, 224, 208, u8::MAX).to_native_bytes();
    let magenta =
        PremultipliedRgba8::from_straight_rgba(224, 48, 160, u8::MAX).to_native_bytes();
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
    error: &'static str,
}

fn dispatch_preview_kernel(
    preview: &ActivePreview,
    surface: crate::intel::gpgpu::GpgpuRgba8Surface,
) -> PreviewDispatchResult {
    match preview.config.preset {
        GpgpuPreviewPreset::Static => PreviewDispatchResult {
            ok: false,
            submitted: false,
            iterations: 0,
            marker: 0,
            submit_ms: 0,
            error: "static-preset-entered-gpu-dispatch",
        },
        GpgpuPreviewPreset::Mandelbrot => {
            let iterations = 32 + ((preview.metrics.attempted - 1) % 97) as u32;
            match crate::intel::gpgpu::mandel64_worklist_surface_full(surface, iterations) {
                Some(result) => PreviewDispatchResult {
                    ok: result.ok,
                    submitted: result.submitted,
                    iterations,
                    marker: 0,
                    submit_ms: result.submit_ms,
                    error: "mandelbrot-dispatch-failed",
                },
                None => PreviewDispatchResult {
                    ok: false,
                    submitted: false,
                    iterations,
                    marker: 0,
                    submit_ms: 0,
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
                error: "plasma-dispatch-failed",
            }
        }
    }
}

const fn preview_producer_label(preset: GpgpuPreviewPreset) -> &'static str {
    match preset {
        GpgpuPreviewPreset::Static => "cpu-static",
        _ => "guc-compute",
    }
}

const fn preview_release_label(preset: GpgpuPreviewPreset) -> &'static str {
    match preset {
        GpgpuPreviewPreset::Static => "clflush-mfence-before-publish",
        _ => "completion-marker-before-publish",
    }
}

const fn preview_plane(preset: GpgpuPreviewPreset) -> WindowPlane {
    match preset {
        GpgpuPreviewPreset::Static => {
            WindowPlane::Universal(super::ALPHA_OVERLAY_PLANE_SLOT as u8)
        }
        _ => WindowPlane::Primary,
    }
}

fn preview_needs_render(preview: &ActivePreview) -> bool {
    preview.config.preset != GpgpuPreviewPreset::Static || preview.static_needs_publish
}

fn schedule_next_render(preview: &mut ActivePreview) {
    let period = Duration::from_millis(preview.config.cadence_ms);
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

fn drain_preview_input(active: &mut Option<ActivePreview>, retired_frames: &mut Vec<FrameHandle>) {
    for event in take_owner_input_events(PREVIEW_OWNER) {
        let Ui4InputEvent::Resize(event) = event else {
            continue;
        };
        let Some(preview) = active.as_mut() else {
            continue;
        };
        if event.window != preview.window || event.width == 0 || event.height == 0 {
            continue;
        }
        if let Err(reason) = resize_preview(preview, event.width, event.height, retired_frames) {
            set_active_error(preview.request_serial, reason);
            crate::log_warn!(
                target: "ui4";
                "ui4 gpgpu-preview resize rejected request={} window={} extent={}x{} reason={}\n",
                preview.request_serial,
                preview.window.raw(),
                event.width,
                event.height,
                reason,
            );
        }
    }
}

fn resize_preview(
    preview: &mut ActivePreview,
    width: u32,
    height: u32,
    retired_frames: &mut Vec<FrameHandle>,
) -> Result<(), &'static str> {
    let output = OutputId::from_slot(0).ok_or("output-d01-unavailable")?;
    let replacement =
        create_preview_frame(output, width, height).map_err(|_| "resize-frame-create-failed")?;
    if replace_window_frame(PREVIEW_OWNER, preview.window, replacement).is_err() {
        let _ = destroy_frame(replacement);
        return Err("resize-window-replace-failed");
    }
    let previous = preview.frame;
    preview.frame = replacement;
    preview.width = width;
    preview.height = height;
    preview.next_render = Instant::now();
    preview.static_needs_publish = true;
    retired_frames.push(previous);
    crate::log_info!(
        target: "ui4";
        "ui4 gpgpu-preview resize applied request={} window={} frame={} extent={}x{} plane_mutation=none\n",
        preview.request_serial,
        preview.window.raw(),
        replacement.raw(),
        width,
        height,
    );
    Ok(())
}

fn stop_active_preview(preview: ActivePreview, reason: &'static str) {
    let close = WindowSessionCloseRequest::default().animate_and_retire_frames();
    let _ = finish_window_session_with_request(PREVIEW_OWNER, preview.session, close);
    crate::log_info!(
        target: "ui4";
        "ui4 gpgpu-preview stopped request={} preset={} frame={} window={} attempted={} completed={} published={} dropped_busy={} failed={} late={} elapsed_ms={} reason={} plane_mutation=none\n",
        preview.request_serial,
        preview.config.preset.label(),
        preview.frame.raw(),
        preview.window.raw(),
        preview.metrics.attempted,
        preview.metrics.completed,
        preview.metrics.published,
        preview.metrics.dropped_busy,
        preview.metrics.failed,
        preview.metrics.late,
        preview.metrics.elapsed_ms,
        reason,
    );
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
    control.status.metrics = GpgpuPreviewMetrics::default();
    control.status.last_error = "none";
}

fn mark_faulted(desired: DesiredPreview, reason: &'static str) {
    let mut control = PREVIEW_CONTROL.lock();
    control.status.phase = GpgpuPreviewPhase::Faulted;
    control.status.applied_serial = desired.serial;
    control.status.frame = None;
    control.status.window = None;
    control.status.last_error = reason;
}

fn mark_idle(serial: u64, reason: &'static str) {
    let mut control = PREVIEW_CONTROL.lock();
    control.status.phase = GpgpuPreviewPhase::Idle;
    control.status.applied_serial = serial;
    control.status.frame = None;
    control.status.window = None;
    control.status.last_error = reason;
}

fn mark_duration_complete(serial: u64, metrics: GpgpuPreviewMetrics) {
    let mut control = PREVIEW_CONTROL.lock();
    if control.desired.serial == serial {
        control.desired.running = false;
        control.status.desired_running = false;
    }
    control.status.phase = GpgpuPreviewPhase::Idle;
    control.status.applied_serial = serial;
    control.status.frame = None;
    control.status.window = None;
    control.status.metrics = metrics;
    control.status.last_error = "duration-complete";
}

fn mark_runtime_fault(serial: u64, metrics: GpgpuPreviewMetrics, reason: &'static str) {
    let mut control = PREVIEW_CONTROL.lock();
    if control.desired.serial == serial {
        control.desired.running = false;
        control.status.desired_running = false;
    }
    control.status.phase = GpgpuPreviewPhase::Faulted;
    control.status.applied_serial = serial;
    control.status.frame = None;
    control.status.window = None;
    control.status.metrics = metrics;
    control.status.last_error = reason;
}

fn publish_active_status(
    preview: &ActivePreview,
    phase: GpgpuPreviewPhase,
    last_error: &'static str,
) {
    let mut control = PREVIEW_CONTROL.lock();
    if control.status.applied_serial != preview.request_serial {
        return;
    }
    control.status.phase = phase;
    control.status.frame = Some(preview.frame);
    control.status.window = Some(preview.window);
    control.status.metrics = preview.metrics;
    if control.status.last_error == "none" || last_error != "none" {
        control.status.last_error = last_error;
    }
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
        GpgpuPreviewPreset, preview_frame_create_error_label,
    };

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
