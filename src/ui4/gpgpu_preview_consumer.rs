//! Shell-controlled GPGPU live previews backed exclusively by UI4 frames.
//!
//! This is a trusted kernel app beside the temporary UI4 dummy consumer.  It
//! owns frame/window lifetime and compute cadence, but deliberately knows
//! nothing about display pipes or universal-plane slots.  Published windows
//! are ordinary inputs to the existing UI4 compositor.

use alloc::vec::Vec;

use embassy_time::{Duration, Instant, Timer};
use spin::Mutex;

use super::{
    DamageRect, FrameCadence, FrameContent, FrameHandle, FramePoolError, FrameSpec, OutputId,
    PremultipliedRgba8, ScanoutFormat, Ui4InputEvent, WindowCreate, WindowId, WindowOwner,
    WindowPlacement, WindowPlane, WindowSessionId, acquire_frame_buffer, begin_window_session,
    cancel_frame_buffer, create_frame, create_window, destroy_frame, finish_window_session,
    gpgpu_rgba_surface, publish_frame_buffer, publish_window_frame, replace_window_frame,
    take_owner_input_events,
};

const PREVIEW_OWNER: WindowOwner = WindowOwner::KernelApp(5);
const PREVIEW_WIDTH: u32 = super::BOOT_DEMO_FRAME_WIDTH;
const PREVIEW_HEIGHT: u32 = super::BOOT_DEMO_FRAME_HEIGHT;
const PREVIEW_MARGIN: u32 = 64;
const PREVIEW_Z: i32 = 30;
const IDLE_POLL_MS: u64 = 20;
const COMMAND_POLL_MAX_MS: u64 = 10;

pub(crate) const GPGPU_PREVIEW_DEFAULT_DURATION_MS: u64 = 5_000;
pub(crate) const GPGPU_PREVIEW_DEFAULT_CADENCE_MS: u64 = 33;
pub(crate) const GPGPU_PREVIEW_DEFAULT_PUBLISH_EVERY: u32 = 1;
pub(crate) const GPGPU_PREVIEW_MAX_CADENCE_MS: u64 = 60_000;
pub(crate) const GPGPU_PREVIEW_MAX_PUBLISH_EVERY: u32 = 1_024;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct GpgpuPreviewConfig {
    pub(crate) duration_ms: u64,
    pub(crate) cadence_ms: u64,
    pub(crate) publish_every: u32,
}

impl GpgpuPreviewConfig {
    pub(crate) const DEFAULT: Self = Self {
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
    metrics: GpgpuPreviewMetrics,
}

pub(crate) fn request_mandel_preview_start(
    config: GpgpuPreviewConfig,
) -> Result<u64, &'static str> {
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
        "ui4 gpgpu-preview-consumer carrier online owner={:?} placement=worker-ap2+ assigned_slot={} current_slot={} display_api=none\n",
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
                stop_active_preview(previous, &mut retired_frames, "command-replaced");
            }
            applied_serial = desired.serial;
            if desired.running {
                mark_starting(desired);
                match initialize_preview(desired) {
                    Ok(preview) => {
                        crate::log_info!(
                            target: "ui4";
                            "ui4 gpgpu-preview start request={} owner={:?} frame={} window={} extent={}x{} cadence_ms={} publish_every={} duration_ms={} plane_mutation=none\n",
                            desired.serial,
                            PREVIEW_OWNER,
                            preview.frame.raw(),
                            preview.window.raw(),
                            preview.width,
                            preview.height,
                            preview.config.cadence_ms,
                            preview.config.publish_every,
                            preview.config.duration_ms,
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
        if let Some(preview) = active.as_mut() {
            preview.metrics.elapsed_ms = now.saturating_duration_since(preview.started).as_millis();
            duration_expired = preview.config.duration_ms != 0
                && preview.metrics.elapsed_ms >= preview.config.duration_ms;
            if !duration_expired && now >= preview.next_render {
                render_preview_frame(preview);
                schedule_next_render(preview);
                publish_active_status(preview, GpgpuPreviewPhase::Running, "none");
            }
        }

        if duration_expired {
            if let Some(finished) = active.take() {
                let serial = finished.request_serial;
                let metrics = finished.metrics;
                stop_active_preview(finished, &mut retired_frames, "duration-complete");
                mark_duration_complete(serial, metrics);
            }
        }

        let wait_ms = active.as_ref().map(next_poll_ms).unwrap_or(IDLE_POLL_MS);
        Timer::after(Duration::from_millis(wait_ms)).await;
    }
}

fn initialize_preview(desired: DesiredPreview) -> Result<ActivePreview, &'static str> {
    let output = OutputId::from_slot(0).ok_or("output-d01-unavailable")?;
    let frame = create_preview_frame(output, PREVIEW_WIDTH, PREVIEW_HEIGHT)
        .map_err(|_| "frame-create-failed")?;
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
        plane: WindowPlane::Primary,
        placement: WindowPlacement {
            x,
            y: PREVIEW_MARGIN as i32,
            width: PREVIEW_WIDTH,
            height: PREVIEW_HEIGHT,
            z: PREVIEW_Z,
            opacity: u8::MAX,
            visible: true,
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
        session,
        frame,
        window,
        width: PREVIEW_WIDTH,
        height: PREVIEW_HEIGHT,
        started: now,
        next_render: now,
        metrics: GpgpuPreviewMetrics::default(),
    })
}

fn create_preview_frame(
    output: OutputId,
    width: u32,
    height: u32,
) -> Result<FrameHandle, FramePoolError> {
    create_frame(FrameSpec {
        output,
        content: FrameContent::Image,
        cadence: FrameCadence::Streaming,
        format: ScanoutFormat::Rgba8888Premultiplied,
        width,
        height,
        base_color: Some(PremultipliedRgba8::from_straight_rgba(0, 0, 0, u8::MAX)),
    })
}

fn render_preview_frame(preview: &mut ActivePreview) {
    preview.metrics.attempted = preview.metrics.attempted.saturating_add(1);
    let publish_this_frame =
        (preview.metrics.attempted - 1) % u64::from(preview.config.publish_every) == 0;
    let lease = match acquire_frame_buffer(preview.frame) {
        Ok(lease) => lease,
        Err(FramePoolError::Busy) => {
            preview.metrics.dropped_busy = preview.metrics.dropped_busy.saturating_add(1);
            return;
        }
        Err(_) => {
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            set_active_error(preview.request_serial, "frame-acquire-failed");
            return;
        }
    };
    let surface = match gpgpu_rgba_surface(lease) {
        Ok(surface) => surface,
        Err(_) => {
            let _ = cancel_frame_buffer(lease);
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            set_active_error(preview.request_serial, "gpgpu-surface-unavailable");
            return;
        }
    };

    let iterations = 32 + ((preview.metrics.attempted - 1) % 97) as u32;
    preview.metrics.last_iterations = iterations;
    let Some(result) = crate::intel::gpgpu::mandel64_worklist_surface_full(surface, iterations)
    else {
        let _ = cancel_frame_buffer(lease);
        preview.metrics.failed = preview.metrics.failed.saturating_add(1);
        set_active_error(preview.request_serial, "mandelbrot-dispatch-unavailable");
        return;
    };
    if result.submitted {
        preview.metrics.submitted = preview.metrics.submitted.saturating_add(1);
    }
    preview.metrics.last_submit_ms = result.submit_ms;
    if !result.ok {
        let _ = cancel_frame_buffer(lease);
        preview.metrics.failed = preview.metrics.failed.saturating_add(1);
        set_active_error(preview.request_serial, "mandelbrot-dispatch-failed");
        return;
    }
    preview.metrics.completed = preview.metrics.completed.saturating_add(1);

    if !publish_this_frame {
        let _ = cancel_frame_buffer(lease);
        return;
    }
    if publish_frame_buffer(lease).is_err() {
        preview.metrics.failed = preview.metrics.failed.saturating_add(1);
        set_active_error(preview.request_serial, "frame-publish-failed");
        return;
    }
    if publish_window_frame(PREVIEW_OWNER, preview.window, DamageRect::FULL).is_err() {
        preview.metrics.failed = preview.metrics.failed.saturating_add(1);
        set_active_error(preview.request_serial, "window-publish-failed");
        return;
    }
    preview.metrics.published = preview.metrics.published.saturating_add(1);
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

fn stop_active_preview(
    preview: ActivePreview,
    retired_frames: &mut Vec<FrameHandle>,
    reason: &'static str,
) {
    let _ = finish_window_session(PREVIEW_OWNER, preview.session);
    retired_frames.push(preview.frame);
    crate::log_info!(
        target: "ui4";
        "ui4 gpgpu-preview stopped request={} frame={} window={} attempted={} completed={} published={} dropped_busy={} failed={} late={} elapsed_ms={} reason={} plane_mutation=none\n",
        preview.request_serial,
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
    use super::{GPGPU_PREVIEW_MAX_CADENCE_MS, GpgpuPreviewConfig};

    #[test]
    fn preview_config_accepts_continuous_duration() {
        assert!(
            GpgpuPreviewConfig {
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
                duration_ms: 1,
                cadence_ms: 0,
                publish_every: 1,
            }
            .validate()
            .is_err()
        );
        assert!(
            GpgpuPreviewConfig {
                duration_ms: 1,
                cadence_ms: GPGPU_PREVIEW_MAX_CADENCE_MS + 1,
                publish_every: 1,
            }
            .validate()
            .is_err()
        );
        assert!(
            GpgpuPreviewConfig {
                duration_ms: 1,
                cadence_ms: 1,
                publish_every: 0,
            }
            .validate()
            .is_err()
        );
    }
}
