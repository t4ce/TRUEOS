//! Shell-controlled GPGPU live previews backed exclusively by UI4 frames.
//!
//! This is a trusted kernel app beside the permanent UI4 compositor. It
//! owns frame/window lifetime and compute cadence. Standalone C++/IGC demos
//! reuse the same exact-surface publication lifecycle on slot 1.
//! Display pipe programming remains exclusively compositor-owned.

use alloc::vec::Vec;

use spin::Mutex;
use trueos_time::{Duration, Instant, Timer};

use super::{
    DamageRect, FrameCadence, FrameContent, FrameHandle, FramePlanError, FramePoolError, FrameSpec,
    FrameWriteLease, OutputId, PremultipliedRgba8, ScanoutFormat, Ui4CursorSource, Ui4InputEvent,
    WindowBrokerError, WindowCreate, WindowId, WindowOwner, WindowPlacement, WindowPlane,
    WindowSessionCloseRequest, WindowSessionId, acquire_frame_buffer, begin_window_session,
    cancel_frame_buffer, commit_window_frame_replacement, create_frame, create_window,
    destroy_frame, finish_window_session, finish_window_session_with_request, gpgpu_rgba_surface,
    publish_frame_buffer, publish_gpgpu_frame_buffer, publish_window_frame,
    reselect_window_for_cursor, retire_frame_when_released, take_owner_input_events,
    window_placement, window_resize_state, writable_rgba_view,
};

const PREVIEW_OWNER: WindowOwner = WindowOwner::GPGPU_PREVIEW;
const PREVIEW_WIDTH: u32 = super::DEFAULT_FRAME_WIDTH;
const PREVIEW_HEIGHT: u32 = super::DEFAULT_FRAME_HEIGHT;
const PREVIEW_MARGIN: u32 = 64;
const PREVIEW_Z: i32 = 30;
const IDLE_POLL_MS: u64 = 20;
const COMMAND_POLL_MAX_MS: u64 = 10;
const RESIZE_RETRY_MS: u64 = 250;
const PRIMARY_BUTTON_MASK: u32 = 1 << 0;

pub(crate) const GPGPU_PREVIEW_DEFAULT_DURATION_MS: u64 = 5_000;
pub(crate) const GPGPU_PREVIEW_DEFAULT_CADENCE_MS: u64 = 33;
pub(crate) const GPGPU_PREVIEW_DEFAULT_PUBLISH_EVERY: u32 = 1;
#[cfg(test)]
pub(crate) const GPGPU_PREVIEW_MAX_CADENCE_MS: u64 = 60_000;
#[cfg(test)]
pub(crate) const GPGPU_PREVIEW_MAX_PUBLISH_EVERY: u32 = 1_024;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum GpgpuPreviewPreset {
    Static,

    CppCloudHighWisps,
    CppAudio,
    CppParticle,
}

impl GpgpuPreviewPreset {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Static => "static",

            Self::CppCloudHighWisps => "cpp-cloud-high-wisps",
            Self::CppAudio => "cpp-audio",
            Self::CppParticle => "cpp-particle",
        }
    }

    pub(crate) const fn is_cpp(self) -> bool {
        matches!(self, |Self::CppCloudHighWisps| Self::CppAudio | Self::CppParticle)
    }

    pub(crate) const fn is_resizable_cpp(self) -> bool {
        self.is_cpp()
    }

    pub(crate) const fn buffering_label(self) -> &'static str {
        "double"
    }

    pub(crate) const fn plane_layout_label(self) -> &'static str {
        match self {
            Self::Static => "slot1-direct",

            Self::CppCloudHighWisps | Self::CppAudio | Self::CppParticle => "slot1-direct",
        }
    }
}

const fn preview_extent(preset: GpgpuPreviewPreset) -> (u32, u32) {
    match preset {

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
        preset: GpgpuPreviewPreset::Static,
        duration_ms: GPGPU_PREVIEW_DEFAULT_DURATION_MS,
        cadence_ms: GPGPU_PREVIEW_DEFAULT_CADENCE_MS,
        publish_every: GPGPU_PREVIEW_DEFAULT_PUBLISH_EVERY,
    };

    #[cfg(test)]
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
    interactive_win_demo: bool,
    refocus_source: Option<Ui4CursorSource>,
}

impl PreviewRunPolicy {
    const SHELL: Self = Self {
        frame_limit: 0,
        target_hz: 0,
        interactive_win_demo: false,
        refocus_source: None,
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

struct CloudBrushState {
    brush: crate::intel::gpgpu::CppCloudBrush,
    dragging: Option<Ui4CursorSource>,
}

impl CloudBrushState {
    const fn new() -> Self {
        Self {
            brush: crate::intel::gpgpu::CppCloudBrush::new(),
            dragging: None,
        }
    }
}

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
    /// The broker keeps this exact front live until `frame` has completed and
    /// is atomically committed as its replacement.
    pending_resize_previous_frame: Option<FrameHandle>,
    pending_resize_logical_extent: Option<(u32, u32)>,
    pending_resize_epoch: Option<u64>,
    committed_logical_extent: (u32, u32),
    started: Instant,
    next_render: Instant,
    static_needs_publish: bool,
    extra_surfaces: Vec<StaticPreviewSurface>,
    particle_craft: Option<crate::intel::gpgpu::GpgpuOwnedParticleCraftState>,
    cloud_brush: CloudBrushState,

    metrics: GpgpuPreviewMetrics,
}

#[derive(Copy, Clone)]
struct StaticPreviewSurface {
    frame: FrameHandle,
    window: WindowId,
}

#[trueos_executor::task(pool_size = 1)]
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
                // Rush2 owns ordinary UI4 frame rings, so its close animation
                // must start immediately. Any final GPU row remains attached
                // to the retired frame and is polled by the non-blocking
                // retirement path below. Awaiting it here can pin the sole
                // preview worker before UI4 is even told to close the windows.
                stop_active_previews(
                    core::mem::take(&mut active),
                    &mut retired_frames,
                    "command-replaced",
                );
            }
            // Apply the newest request after the old session has closed.
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

        {
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
        restore_interactive_win_focus(&mut active);
        if !active.is_empty() && render_fault.is_none() && !duration_expired {
            publish_active_status(&active, GpgpuPreviewPhase::Running, "none");
        }

        if let Some(reason) = render_fault {
            if !active.is_empty() {
                let serial = active[0].request_serial;
                let metrics = aggregate_preview_metrics(&active);
                stop_active_previews(
                    core::mem::take(&mut active),
                    &mut retired_frames,
                    "render-fault",
                );
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
            if !active.is_empty() {
                let serial = active[0].request_serial;
                let metrics = aggregate_preview_metrics(&active);
                stop_active_previews(
                    core::mem::take(&mut active),
                    &mut retired_frames,
                    "duration-complete",
                );
                mark_duration_complete(serial, metrics);
            }
        }

        let wait_ms = next_preview_group_poll_ms(&active);
        Timer::after(Duration::from_millis(wait_ms)).await;
    }
}

fn initialize_previews(desired: DesiredPreview) -> Result<Vec<ActivePreview>, &'static str> {
    Ok(alloc::vec![initialize_preview(desired)?])
}

fn initialize_preview(desired: DesiredPreview) -> Result<ActivePreview, &'static str> {
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
        // Every C++ demo consumes UI4's dock/restore extent notification
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
        pending_resize_previous_frame: None,
        pending_resize_logical_extent: None,
        pending_resize_epoch: None,
        committed_logical_extent: (width, height),
        started: now,
        next_render: now,
        static_needs_publish: true,
        extra_surfaces: Vec::new(),
        particle_craft: None,
        cloud_brush: CloudBrushState::new(),

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

async fn render_preview_frame(preview: &mut ActivePreview) -> Result<(), &'static str> {
    preview.metrics.attempted = preview.metrics.attempted.saturating_add(1);
    let publish_this_frame = preview.pending_resize_previous_frame.is_some()
        || (preview.metrics.attempted - 1) % u64::from(preview.config.publish_every) == 0;
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
        GpgpuPreviewPreset::CppCloudHighWisps
        | GpgpuPreviewPreset::CppAudio
        | GpgpuPreviewPreset::CppParticle => match gpu_release {
            Some(release) => publish_gpgpu_frame_buffer(lease, release),
            None => Err(FramePoolError::ProducerReleaseRequired),
        },
        GpgpuPreviewPreset::Static => publish_frame_buffer(lease),
    };
    if publish_result.is_err() {
        preview.metrics.failed = preview.metrics.failed.saturating_add(1);
        return Err("frame-publish-failed");
    }
    match publish_preview_window_frame(preview, DamageRect::FULL) {
        Ok(true) => {}
        Ok(false) => return Ok(()),
        Err(_) => {
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            return Err("window-publish-failed");
        }
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

fn publish_preview_window_frame(
    preview: &mut ActivePreview,
    damage: DamageRect,
) -> Result<bool, &'static str> {
    let Some(previous) = preview.pending_resize_previous_frame else {
        return publish_window_frame(PREVIEW_OWNER, preview.window, damage)
            .map(|_| true)
            .map_err(|_| "window-publish-failed");
    };
    let (logical_width, logical_height) = preview
        .pending_resize_logical_extent
        .ok_or("resize-logical-extent-missing")?;
    let resize_epoch = preview.pending_resize_epoch.ok_or("resize-epoch-missing")?;
    let current_placement = window_placement(PREVIEW_OWNER, preview.window)
        .map_err(|_| "resize-placement-unavailable")?;
    let staged_placement = WindowPlacement {
        width: logical_width,
        height: logical_height,
        ..current_placement
    };
    match commit_window_frame_replacement(
        PREVIEW_OWNER,
        preview.window,
        preview.frame,
        staged_placement,
        resize_epoch,
        damage,
    ) {
        Ok(_) => {}
        Err(WindowBrokerError::StaleResize) => {
            // The broker rejected this exact extent, so retain its current
            // SURFLIVE front and let authoritative extent reconciliation stage
            // a successor. Holding this published-but-unattached allocation
            // would needlessly consume a bounded frame-pool slot.
            let superseded = preview.frame;
            preview.frame = previous;
            (preview.width, preview.height) = preview_backing_extent(
                preview.config.preset,
                preview.committed_logical_extent.0,
                preview.committed_logical_extent.1,
            );
            preview.pending_resize_previous_frame = None;
            preview.pending_resize_logical_extent = None;
            preview.pending_resize_epoch = None;
            retire_frame_when_released(superseded);
            crate::log_info!(
                target: "ui4";
                "ui4 gpgpu-preview resize front superseded request={} window={} frame={} staged_extent={}x{} current_extent={}x{} action=reuse-committed-front-and-restage\n",
                preview.request_serial,
                preview.window.raw(),
                superseded.raw(),
                staged_placement.width,
                staged_placement.height,
                current_placement.width,
                current_placement.height,
            );
            return Ok(false);
        }
        Err(_) => return Err("resize-window-commit-failed"),
    }
    preview.committed_logical_extent = (staged_placement.width, staged_placement.height);
    preview.pending_resize_previous_frame = None;
    preview.pending_resize_logical_extent = None;
    preview.pending_resize_epoch = None;
    retire_frame_when_released(previous);
    crate::log_info!(
        target: "ui4";
        "ui4 gpgpu-preview resize front committed request={} window={} old_frame={} frame={} logical_extent={}x{} action=atomic-published-replacement old_front=held-1:1-until-commit\n",
        preview.request_serial,
        preview.window.raw(),
        previous.raw(),
        preview.frame.raw(),
        staged_placement.width,
        staged_placement.height,
    );
    Ok(true)
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
        GpgpuPreviewPreset::Static => PreviewDispatchResult {
            ok: false,
            submitted: false,
            iterations: 0,
            marker: 0,
            submit_ms: 0,
            release: None,
            error: "static-preset-entered-gpu-dispatch",
        },

        GpgpuPreviewPreset::CppCloudHighWisps => {
            let seconds = preview.metrics.elapsed_ms as f32 / 1_000.0;
            let mode = crate::intel::gpgpu::CPP_DEMO_MODE_CLOUD_HIGH_WISPS;
            let seed = (preview.request_serial as u32)
                .rotate_left(13)
                .wrapping_add(0xC0DE_C901);
            let result = crate::intel::gpgpu::cpp_demo_rgba8_surface_full(
                surface,
                seconds,
                mode,
                seed,
                &preview.cloud_brush.brush.points[..preview.cloud_brush.brush.count],
            );
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
        GpgpuPreviewPreset::CppCloudHighWisps
        | GpgpuPreviewPreset::CppAudio
        | GpgpuPreviewPreset::CppParticle => "pipe-control+post-marker-exact-surface",

        GpgpuPreviewPreset::Static => "clflush-mfence-before-publish",
    }
}

const fn preview_producer_label(preset: GpgpuPreviewPreset) -> &'static str {
    match preset {

        GpgpuPreviewPreset::Static => "cpu-static",

        GpgpuPreviewPreset::CppCloudHighWisps | GpgpuPreviewPreset::CppAudio => "guc-cpp-single",
        GpgpuPreviewPreset::CppParticle => "guc-cpp-stateful-three-pass",
    }
}

const fn preview_plane(preset: GpgpuPreviewPreset) -> WindowPlane {
    match preset {
        GpgpuPreviewPreset::Static => WindowPlane::Universal(super::ALPHA_OVERLAY_PLANE_SLOT as u8),
        GpgpuPreviewPreset::CppCloudHighWisps
        | GpgpuPreviewPreset::CppAudio
        | GpgpuPreviewPreset::CppParticle => {
            WindowPlane::Universal(preview_plane_slot(preset) as u8)
        }

    }
}

const fn preview_consumer_label(preset: GpgpuPreviewPreset) -> &'static str {
    match preset {

        GpgpuPreviewPreset::CppCloudHighWisps
        | GpgpuPreviewPreset::CppAudio
        | GpgpuPreviewPreset::CppParticle => "ui4-cpp-resizable-slot1",

        GpgpuPreviewPreset::Static => "ui4-overlay",
    }
}

fn preview_surface_count(preview: &ActivePreview) -> usize {
    1usize.saturating_add(preview.extra_surfaces.len())
}

fn preview_needs_render(preview: &ActivePreview) -> bool {
    if preview.pending_resize_previous_frame.is_some() {
        // A completed replacement must be allowed to reach the atomic broker
        // commit even after a demo's ordinary frame limit has been reached.
        return true;
    }
    if preview.policy.frame_limit != 0 && preview.metrics.published >= preview.policy.frame_limit {
        return false;
    }
    !matches!(preview.config.preset, GpgpuPreviewPreset::Static) || preview.static_needs_publish
}

fn schedule_next_render(preview: &mut ActivePreview) {
    let period = if preview.policy.target_hz == 0 {
        Duration::from_millis(preview.config.cadence_ms)
    } else {
        let hz = preview.policy.target_hz;
        let mut ticks = trueos_time::TICK_HZ / hz;
        preview.cadence_phase += trueos_time::TICK_HZ % hz;
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
        match event {
            Ui4InputEvent::Resize(event) => {
                let Some(preview) = active
                    .iter_mut()
                    .find(|preview| event.window == preview.window)
                else {
                    continue;
                };
                if event.width == 0 || event.height == 0 {
                    continue;
                }
                try_resize_preview(
                    preview,
                    event.width,
                    event.height,
                    retired_frames,
                    true,
                    event.resize_epoch,
                    "input-event",
                );
            }
            Ui4InputEvent::Pointer(event) => {
                let Some(preview) = active.iter_mut().find(|preview| {
                    event.window == preview.window
                        && preview.config.preset == GpgpuPreviewPreset::CppCloudHighWisps
                }) else {
                    continue;
                };
                if event.buttons_pressed & PRIMARY_BUTTON_MASK != 0 {
                    preview.cloud_brush.dragging = Some(event.source);
                    preview.cloud_brush.brush.last = None;
                }
                if preview.cloud_brush.dragging == Some(event.source)
                    && event.buttons_down & PRIMARY_BUTTON_MASK != 0
                {
                    preview.cloud_brush.brush.drag_to(
                        event.local_x,
                        event.local_y,
                        preview.width,
                        preview.height,
                    );
                }
                if preview.cloud_brush.dragging == Some(event.source)
                    && event.buttons_released & PRIMARY_BUTTON_MASK != 0
                {
                    preview.cloud_brush.dragging = None;
                    preview.cloud_brush.brush.last = None;
                }
            }
            Ui4InputEvent::Keyboard(event) => {
                let Some(preview) = active.iter().find(|preview| {
                    event.window == preview.window
                        || preview
                            .extra_surfaces
                            .iter()
                            .any(|surface| event.window == surface.window)
                }) else {
                    continue;
                };
                if !preview.policy.interactive_win_demo
                    || event.event.kind != crate::r::keyboard::KEYBOARD_OUTPUT_KIND_KEY
                {
                    continue;
                }
                match event.event.key_code {
                    crate::r::keyboard::KEYBOARD_KEY_ESCAPE => {
                        queue_interactive_win_stop();
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

fn queue_interactive_win_stop() {
    let serial = {
        let mut control = PREVIEW_CONTROL.lock();
        if !control.desired.running || !control.desired.policy.interactive_win_demo {
            return;
        }
        let serial = next_serial(control.desired.serial);
        control.desired.serial = serial;
        control.desired.running = false;
        control.desired.policy.refocus_source = None;
        control.status.desired_running = false;
        control.status.request_serial = serial;

        serial
    };
    crate::log_info!(target: "ui4";
        "ui4 win stop request={} input=keyboard-escape\n",
        serial,
    );
}

fn restore_interactive_win_focus(active: &mut [ActivePreview]) {
    let Some(source) = active
        .first()
        .filter(|preview| preview.policy.interactive_win_demo)
        .and_then(|preview| preview.policy.refocus_source)
    else {
        return;
    };
    let window = active[0].window;
    if reselect_window_for_cursor(source, PREVIEW_OWNER, window).is_ok() {
        for preview in active {
            preview.policy.refocus_source = None;
        }
    }
}

/// Resize notifications wake the fast path, while the broker's logical extent
/// remains authoritative for retries and superseded targets. The committed
/// logical extent is tracked separately because Particle Craft can use the
/// same backing allocation for two different presentation extents.
fn reconcile_preview_extents(active: &mut [ActivePreview], retired_frames: &mut Vec<FrameHandle>) {
    for preview in active {
        if !preview.config.preset.is_resizable_cpp() {
            continue;
        }
        let Ok((placement, resize_epoch)) = window_resize_state(PREVIEW_OWNER, preview.window)
        else {
            continue;
        };
        let logical_extent = (placement.width, placement.height);
        let retry_targets_extent =
            (preview.resize_retry_width, preview.resize_retry_height) == logical_extent;
        if placement.width == 0
            || placement.height == 0
            || preview.pending_resize_logical_extent == Some(logical_extent)
            || (preview.pending_resize_logical_extent.is_none()
                && preview.committed_logical_extent == logical_extent
                && !retry_targets_extent)
        {
            continue;
        }
        try_resize_preview(
            preview,
            placement.width,
            placement.height,
            retired_frames,
            false,
            resize_epoch,
            "broker-reconcile",
        );
    }
}

fn try_resize_preview(
    preview: &mut ActivePreview,
    width: u32,
    height: u32,
    retired_frames: &mut Vec<FrameHandle>,
    broker_resize_event: bool,
    resize_epoch: u64,
    source: &'static str,
) {
    let logical_extent = (width, height);
    if preview.pending_resize_previous_frame.is_some()
        && preview.committed_logical_extent == logical_extent
    {
        discard_pending_preview_resize(preview, retired_frames);
        preview.resize_retry_width = 0;
        preview.resize_retry_height = 0;
        preview.resize_retry_at = Instant::now();
        return;
    }
    let retry_targets_extent =
        (preview.resize_retry_width, preview.resize_retry_height) == logical_extent;
    if preview.pending_resize_logical_extent == Some(logical_extent)
        || (preview.pending_resize_logical_extent.is_none()
            && preview.committed_logical_extent == logical_extent)
            && !broker_resize_event
            && !retry_targets_extent
    {
        return;
    }
    let now = Instant::now();
    if width == preview.resize_retry_width
        && height == preview.resize_retry_height
        && now < preview.resize_retry_at
    {
        return;
    }
    match resize_preview(preview, width, height, resize_epoch, retired_frames) {
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
    resize_epoch: u64,
    retired_frames: &mut Vec<FrameHandle>,
) -> Result<(), &'static str> {
    let output = OutputId::from_slot(0).ok_or("output-d01-unavailable")?;
    let previous = discard_pending_preview_resize(preview, retired_frames).unwrap_or(preview.frame);
    let (backing_width, backing_height) =
        preview_backing_extent(preview.config.preset, logical_width, logical_height);
    let replacement = create_preview_frame(output, backing_width, backing_height)
        .map_err(preview_frame_create_error_label)?;
    preview.pending_resize_previous_frame = Some(previous);
    preview.pending_resize_logical_extent = Some((logical_width, logical_height));
    preview.pending_resize_epoch = Some(resize_epoch);
    preview.frame = replacement;
    preview.width = backing_width;
    preview.height = backing_height;
    preview.next_render = Instant::now();
    preview.static_needs_publish = true;
    crate::log_info!(
        target: "ui4";
        "ui4 gpgpu-preview resize staged request={} window={} old_frame={} frame={} logical_extent={}x{} backing_extent={}x{} presentation={} broker_front=held-1:1-until-atomic-commit\n",
        preview.request_serial,
        preview.window.raw(),
        previous.raw(),
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

fn discard_pending_preview_resize(
    preview: &mut ActivePreview,
    retired_frames: &mut Vec<FrameHandle>,
) -> Option<FrameHandle> {
    let previous = preview.pending_resize_previous_frame.take()?;
    // Release the obsolete staged allocation before asking the bounded frame
    // pool for its successor. The broker still pins `previous` as its live
    // front, so reverting local producer state is safe if allocation retries.
    let superseded = preview.frame;
    preview.frame = previous;
    (preview.width, preview.height) = preview_backing_extent(
        preview.config.preset,
        preview.committed_logical_extent.0,
        preview.committed_logical_extent.1,
    );
    preview.pending_resize_logical_extent = None;
    preview.pending_resize_epoch = None;
    if !retired_frames.contains(&superseded) {
        retired_frames.push(superseded);
    }
    retire_frames(retired_frames);
    Some(previous)
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
    for mut preview in previews {
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
        let pending_resize_previous = preview.pending_resize_previous_frame.take();
        if let Some(previous) = pending_resize_previous {
            // `preview.frame` was never attached to the broker; the broker
            // still owns `previous` and may already have transferred its
            // lifetime to the close animation above.
            if !retired_frames.contains(&preview.frame) {
                retired_frames.push(preview.frame);
            }
            if !frame_lifecycle_transferred && !retired_frames.contains(&previous) {
                retired_frames.push(previous);
            }
            if !frame_lifecycle_transferred {
                for surface in &preview.extra_surfaces {
                    if !retired_frames.contains(&surface.frame) {
                        retired_frames.push(surface.frame);
                    }
                }
            }
        }
        if pending_resize_previous.is_none() && !frame_lifecycle_transferred {
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
    control.status.width = 0;
    control.status.height = 0;
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
    control.status.width = 0;
    control.status.height = 0;
    control.status.metrics = metrics;
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
    control.status.config = first.config;
    control.status.frame = Some(first.frame);
    control.status.window = Some(first.window);
    control.status.width = first.width;
    control.status.height = first.height;
    control.status.metrics = aggregate_preview_metrics(previews);
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

const fn preview_plane_slot(preset: GpgpuPreviewPreset) -> usize {
    if matches!(preset, |GpgpuPreviewPreset::CppAudio| GpgpuPreviewPreset::CppParticle) {
        return 1;
    }
    super::ALPHA_OVERLAY_PLANE_SLOT
}

fn active_preview_plane_slot(preview: &ActivePreview) -> usize {
    super::window_broker::window_snapshot(PREVIEW_OWNER, preview.window)
        .map(|window| window.plane.slot())
        .unwrap_or_else(|| preview_plane_slot(preview.config.preset))
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
        GpgpuPreviewPreset, PREVIEW_HEIGHT, PREVIEW_WIDTH, preview_extent,
        preview_frame_create_error_label, preview_plane_slot,
    };

    #[test]

    #[test]
    fn cpp_modes_share_the_resizable_slot1_application_surface() {
        let modes = [
            GpgpuPreviewPreset::CppCloudHighWisps,
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
    fn cloud_brush_uses_frame_local_normalized_points_and_bounds_its_ring() {
        let mut brush = crate::intel::gpgpu::CppCloudBrush::new();
        brush.push(0, 0, 101, 51);
        brush.push(100, 50, 101, 51);
        assert_eq!(brush.points[0], 0);
        assert_eq!(brush.points[1], u32::MAX);

        brush.last = Some((0, 25));
        brush.drag_to(100, 25, 101, 51);
        assert!(brush.count > 2);
        assert!(brush.count <= crate::intel::gpgpu::CPP_CLOUD_BRUSH_POINT_CAPACITY);

        for index in 0..64 {
            brush.push(index, index, 101, 51);
        }
        assert_eq!(brush.count, crate::intel::gpgpu::CPP_CLOUD_BRUSH_POINT_CAPACITY);
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
                preset: GpgpuPreviewPreset::Static,
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
                preset: GpgpuPreviewPreset::Static,
                duration_ms: 1,
                cadence_ms: 0,
                publish_every: 1,
            }
            .validate()
            .is_err()
        );
        assert!(
            GpgpuPreviewConfig {
                preset: GpgpuPreviewPreset::Static,
                duration_ms: 1,
                cadence_ms: GPGPU_PREVIEW_MAX_CADENCE_MS + 1,
                publish_every: 1,
            }
            .validate()
            .is_err()
        );
        assert!(
            GpgpuPreviewConfig {
                preset: GpgpuPreviewPreset::Static,
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
