//! Shell-controlled SVG outline probes presented through an ordinary UI4 frame.
//!
//! SVG parsing/normalization and the GPGPU implementation live below the
//! `intel::gpgpu` boundary. This carrier owns only the broker window, frame
//! lifecycle, producer-release handoff, and command/status control plane.

use alloc::vec::Vec;

use embassy_time::{Duration, Timer};
use spin::Mutex;

use super::{
    DamageRect, FrameCadence, FrameContent, FrameHandle, FramePoolError, FrameSpec, OutputId,
    PremultipliedRgba8, ScanoutFormat, Ui4InputEvent, WindowCreate, WindowId, WindowOwner,
    WindowPlacement, WindowPlane, WindowSessionCloseRequest, WindowSessionId, acquire_frame_buffer,
    begin_window_session, cancel_frame_buffer, create_frame, create_window, destroy_frame,
    finish_window_session, finish_window_session_with_request, gpgpu_rgba_surface,
    publish_gpgpu_frame_buffer, publish_window_frame, replace_window_frame,
    take_owner_input_events,
};

const SVG_PROBE_OWNER: WindowOwner = WindowOwner::SVG_OUTLINE_PROBE;
const SVG_PROBE_WIDTH: u32 = super::DEFAULT_FRAME_WIDTH;
const SVG_PROBE_HEIGHT: u32 = super::DEFAULT_FRAME_HEIGHT;
const SVG_PROBE_MARGIN: u32 = 64;
const SVG_PROBE_Z: i32 = 31;
const CONTROL_POLL_MS: u64 = 20;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct GpgpuSvgProbeConfig {
    pub(crate) demo: crate::intel::gpgpu::SvgOutlineProbeDemo,
}

impl GpgpuSvgProbeConfig {
    pub(crate) const DEFAULT: Self = Self {
        demo: crate::intel::gpgpu::SvgOutlineProbeDemo::Basic,
    };
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum GpgpuSvgProbePhase {
    Offline,
    Idle,
    Starting,
    Presented,
    Faulted,
}

impl GpgpuSvgProbePhase {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Offline => "offline",
            Self::Idle => "idle",
            Self::Starting => "starting",
            Self::Presented => "presented",
            Self::Faulted => "faulted",
        }
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct GpgpuSvgProbeMetrics {
    pub(crate) attempted: u64,
    pub(crate) submitted: u64,
    pub(crate) published: u64,
    pub(crate) layers: usize,
    pub(crate) ops: usize,
    pub(crate) nonzero_pixels: usize,
    pub(crate) submit_ms: u64,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct GpgpuSvgProbeStatus {
    pub(crate) online: bool,
    pub(crate) desired_running: bool,
    pub(crate) request_serial: u64,
    pub(crate) applied_serial: u64,
    pub(crate) phase: GpgpuSvgProbePhase,
    pub(crate) config: GpgpuSvgProbeConfig,
    pub(crate) frame: Option<FrameHandle>,
    pub(crate) window: Option<WindowId>,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) metrics: GpgpuSvgProbeMetrics,
    pub(crate) last_error: &'static str,
}

impl GpgpuSvgProbeStatus {
    const fn initial() -> Self {
        Self {
            online: false,
            desired_running: false,
            request_serial: 0,
            applied_serial: 0,
            phase: GpgpuSvgProbePhase::Offline,
            config: GpgpuSvgProbeConfig::DEFAULT,
            frame: None,
            window: None,
            width: SVG_PROBE_WIDTH,
            height: SVG_PROBE_HEIGHT,
            metrics: GpgpuSvgProbeMetrics {
                attempted: 0,
                submitted: 0,
                published: 0,
                layers: 0,
                ops: 0,
                nonzero_pixels: 0,
                submit_ms: 0,
            },
            last_error: "none",
        }
    }
}

#[derive(Copy, Clone)]
struct DesiredSvgProbe {
    serial: u64,
    running: bool,
    config: GpgpuSvgProbeConfig,
}

struct SvgProbeControl {
    desired: DesiredSvgProbe,
    status: GpgpuSvgProbeStatus,
}

static SVG_PROBE_CONTROL: Mutex<SvgProbeControl> = Mutex::new(SvgProbeControl {
    desired: DesiredSvgProbe {
        serial: 0,
        running: false,
        config: GpgpuSvgProbeConfig::DEFAULT,
    },
    status: GpgpuSvgProbeStatus::initial(),
});

struct ActiveSvgProbe {
    request_serial: u64,
    config: GpgpuSvgProbeConfig,
    session: WindowSessionId,
    frame: FrameHandle,
    window: WindowId,
    width: u32,
    height: u32,
    metrics: GpgpuSvgProbeMetrics,
}

pub(crate) fn request_gpgpu_svg_probe_start(
    config: GpgpuSvgProbeConfig,
) -> Result<u64, &'static str> {
    let mut control = SVG_PROBE_CONTROL.lock();
    if !control.status.online {
        return Err("svg-probe-service-offline");
    }
    let serial = next_serial(control.desired.serial);
    control.desired = DesiredSvgProbe {
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

pub(crate) fn request_gpgpu_svg_probe_stop() -> u64 {
    let mut control = SVG_PROBE_CONTROL.lock();
    let serial = next_serial(control.desired.serial);
    control.desired.serial = serial;
    control.desired.running = false;
    control.status.desired_running = false;
    control.status.request_serial = serial;
    serial
}

pub(crate) fn gpgpu_svg_probe_status() -> GpgpuSvgProbeStatus {
    SVG_PROBE_CONTROL.lock().status
}

#[embassy_executor::task(pool_size = 1)]
pub(crate) async fn gpgpu_svg_probe_consumer_service_task(worker_slot: u32) {
    crate::log_info!(
        target: "ui4";
        "ui4 gpgpu-svg-probe carrier online owner={:?} placement=worker-ap2+ assigned_slot={} current_slot={} activation=Shell2/on-demand buffering=double producer_release=exact-surface interaction=movable-fixed-size\n",
        SVG_PROBE_OWNER,
        worker_slot,
        crate::percpu::current_slot(),
    );
    crate::intel::wait_hw_logo_sequence_done().await;
    {
        let mut control = SVG_PROBE_CONTROL.lock();
        control.status.online = true;
        control.status.phase = GpgpuSvgProbePhase::Idle;
    }

    let mut applied_serial = 0u64;
    let mut active = None::<ActiveSvgProbe>;
    let mut retired_frames = Vec::new();

    loop {
        retire_frames(&mut retired_frames);
        drain_svg_probe_input(&mut active, &mut retired_frames);

        let desired = SVG_PROBE_CONTROL.lock().desired;
        if desired.serial != applied_serial {
            if let Some(probe) = active.take() {
                stop_active_svg_probe(probe, &mut retired_frames, "command-replaced");
            }
            applied_serial = desired.serial;
            if desired.running {
                mark_starting(desired);
                match initialize_svg_probe(desired) {
                    Ok(mut probe) => match render_svg_probe(&mut probe) {
                        Ok(()) => {
                            publish_presented_status(&probe);
                            crate::log_info!(
                                target: "ui4";
                                "ui4 gpgpu-svg-probe presented request={} demo={} frame={} window={} extent={}x{} layers={} ops={} nonzero_pixels={} submit_ms={} producer=guc-gpgpu consumer=ui4-direct-slot1 display_release=surflive buffering=double\n",
                                probe.request_serial,
                                probe.config.demo.label(),
                                probe.frame.raw(),
                                probe.window.raw(),
                                probe.width,
                                probe.height,
                                probe.metrics.layers,
                                probe.metrics.ops,
                                probe.metrics.nonzero_pixels,
                                probe.metrics.submit_ms,
                            );
                            active = Some(probe);
                        }
                        Err(reason) => {
                            let serial = probe.request_serial;
                            let metrics = probe.metrics;
                            stop_active_svg_probe(probe, &mut retired_frames, "render-fault");
                            mark_faulted(serial, desired.config, metrics, reason);
                        }
                    },
                    Err(reason) => mark_faulted(
                        desired.serial,
                        desired.config,
                        GpgpuSvgProbeMetrics::default(),
                        reason,
                    ),
                }
            } else {
                mark_idle(applied_serial, "stopped");
            }
        }

        Timer::after(Duration::from_millis(CONTROL_POLL_MS)).await;
    }
}

fn initialize_svg_probe(desired: DesiredSvgProbe) -> Result<ActiveSvgProbe, &'static str> {
    let output = OutputId::from_slot(0).ok_or("output-d01-unavailable")?;
    let frame = create_svg_probe_frame(output, SVG_PROBE_WIDTH, SVG_PROBE_HEIGHT)
        .map_err(|_| "svg-probe-frame-create-failed")?;
    let session = match begin_window_session(SVG_PROBE_OWNER) {
        Ok(session) => session,
        Err(_) => {
            let _ = destroy_frame(frame);
            return Err("svg-probe-session-create-failed");
        }
    };
    let (scanout_width, scanout_height) =
        crate::intel::active_scanout_dimensions().unwrap_or((SVG_PROBE_WIDTH, SVG_PROBE_HEIGHT));
    let x = scanout_width
        .saturating_sub(SVG_PROBE_WIDTH)
        .checked_div(2)
        .unwrap_or(0) as i32;
    let y = scanout_height
        .saturating_sub(SVG_PROBE_HEIGHT)
        .checked_div(2)
        .unwrap_or(0)
        .max(SVG_PROBE_MARGIN.min(scanout_height.saturating_sub(SVG_PROBE_HEIGHT)))
        as i32;
    let window = match create_window(WindowCreate {
        owner: SVG_PROBE_OWNER,
        session,
        frame,
        output,
        plane: WindowPlane::Universal(super::ALPHA_OVERLAY_PLANE_SLOT as u8),
        placement: WindowPlacement {
            x,
            y,
            width: SVG_PROBE_WIDTH,
            height: SVG_PROBE_HEIGHT,
            z: SVG_PROBE_Z,
            opacity: u8::MAX,
            visible: true,
        },
        interaction: super::WindowInteraction::MOVABLE_FRAME,
    }) {
        Ok(window) => window,
        Err(_) => {
            let _ = finish_window_session(SVG_PROBE_OWNER, session);
            let _ = destroy_frame(frame);
            return Err("svg-probe-window-create-failed");
        }
    };
    Ok(ActiveSvgProbe {
        request_serial: desired.serial,
        config: desired.config,
        session,
        frame,
        window,
        width: SVG_PROBE_WIDTH,
        height: SVG_PROBE_HEIGHT,
        metrics: GpgpuSvgProbeMetrics::default(),
    })
}

fn create_svg_probe_frame(
    output: OutputId,
    width: u32,
    height: u32,
) -> Result<FrameHandle, FramePoolError> {
    create_frame(FrameSpec {
        output,
        content: FrameContent::Image,
        cadence: FrameCadence::Dirty,
        buffering: super::FrameBuffering::Double,
        format: ScanoutFormat::Rgba8888Premultiplied,
        width,
        height,
        base_color: Some(PremultipliedRgba8::from_straight_rgba(0, 0, 0, u8::MAX)),
    })
}

fn render_svg_probe(probe: &mut ActiveSvgProbe) -> Result<(), &'static str> {
    probe.metrics.attempted = probe.metrics.attempted.saturating_add(1);
    let lease = acquire_frame_buffer(probe.frame).map_err(|_| "svg-probe-frame-acquire-failed")?;
    let surface = match gpgpu_rgba_surface(lease) {
        Ok(surface) => surface,
        Err(_) => {
            let _ = cancel_frame_buffer(lease);
            return Err("svg-probe-gpgpu-surface-unavailable");
        }
    };
    let result = crate::intel::gpgpu::submit_svg_outline_probe(surface, probe.config.demo);
    probe.metrics.layers = result.layers;
    probe.metrics.ops = result.ops;
    probe.metrics.nonzero_pixels = result.nonzero_pixels;
    probe.metrics.submit_ms = result.submit_ms;
    if result.destination_submitted {
        probe.metrics.submitted = probe.metrics.submitted.saturating_add(1);
    }
    if !result.ok {
        if result.destination_submitted {
            crate::log_error!(
                target: "ui4";
                "ui4 gpgpu-svg-probe producer quarantine request={} demo={} frame={} buffer={} reason={} action=retain-write-lease-no-reuse\n",
                probe.request_serial,
                probe.config.demo.label(),
                lease.frame.raw(),
                lease.buffer_index,
                result.error,
            );
        } else {
            let _ = cancel_frame_buffer(lease);
        }
        return Err(result.error);
    }
    let Some(release) = result.release else {
        if !result.destination_submitted {
            let _ = cancel_frame_buffer(lease);
        }
        return Err("svg-probe-release-missing");
    };
    publish_gpgpu_frame_buffer(lease, release).map_err(|_| "svg-probe-frame-publish-failed")?;
    publish_window_frame(SVG_PROBE_OWNER, probe.window, DamageRect::FULL)
        .map_err(|_| "svg-probe-window-publish-failed")?;
    probe.metrics.published = probe.metrics.published.saturating_add(1);
    Ok(())
}

fn drain_svg_probe_input(
    active: &mut Option<ActiveSvgProbe>,
    retired_frames: &mut Vec<FrameHandle>,
) {
    for event in take_owner_input_events(SVG_PROBE_OWNER) {
        let Ui4InputEvent::Resize(event) = event else {
            continue;
        };
        let Some(probe) = active.as_mut() else {
            continue;
        };
        if event.window != probe.window || event.width == 0 || event.height == 0 {
            continue;
        }
        if let Err(reason) = resize_svg_probe(probe, event.width, event.height, retired_frames) {
            mark_faulted(probe.request_serial, probe.config, probe.metrics, reason);
            crate::log_warn!(
                target: "ui4";
                "ui4 gpgpu-svg-probe resize rejected request={} window={} extent={}x{} reason={}\n",
                probe.request_serial,
                probe.window.raw(),
                event.width,
                event.height,
                reason,
            );
        } else {
            publish_presented_status(probe);
        }
    }
}

fn resize_svg_probe(
    probe: &mut ActiveSvgProbe,
    width: u32,
    height: u32,
    retired_frames: &mut Vec<FrameHandle>,
) -> Result<(), &'static str> {
    let output = OutputId::from_slot(0).ok_or("output-d01-unavailable")?;
    let replacement = create_svg_probe_frame(output, width, height)
        .map_err(|_| "svg-probe-resize-frame-create-failed")?;
    if replace_window_frame(SVG_PROBE_OWNER, probe.window, replacement).is_err() {
        let _ = destroy_frame(replacement);
        return Err("svg-probe-resize-window-replace-failed");
    }
    let previous = probe.frame;
    probe.frame = replacement;
    probe.width = width;
    probe.height = height;
    retired_frames.push(previous);
    render_svg_probe(probe)?;
    crate::log_info!(
        target: "ui4";
        "ui4 gpgpu-svg-probe resize applied request={} demo={} window={} frame={} extent={}x{} plane_mutation=none\n",
        probe.request_serial,
        probe.config.demo.label(),
        probe.window.raw(),
        replacement.raw(),
        width,
        height,
    );
    Ok(())
}

fn stop_active_svg_probe(
    probe: ActiveSvgProbe,
    retired_frames: &mut Vec<FrameHandle>,
    reason: &'static str,
) {
    let lifecycle_transferred = finish_window_session_with_request(
        SVG_PROBE_OWNER,
        probe.session,
        WindowSessionCloseRequest::default().direct_plane_animate_and_retire_frames(),
    )
    .is_ok();
    if !lifecycle_transferred {
        let _ = finish_window_session(SVG_PROBE_OWNER, probe.session);
        retired_frames.push(probe.frame);
    }
    crate::log_info!(
        target: "ui4";
        "ui4 gpgpu-svg-probe stopped request={} demo={} frame={} window={} published={} reason={} teardown={} frame_retire=after-surflive-display-lease-drain\n",
        probe.request_serial,
        probe.config.demo.label(),
        probe.frame.raw(),
        probe.window.raw(),
        probe.metrics.published,
        reason,
        if lifecycle_transferred { "direct-plane-scaler+alpha" } else { "broker-detach-no-animation" },
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
                    "ui4 gpgpu-svg-probe frame retire abandoned frame={} error={:?}\n",
                    frame.raw(),
                    error,
                );
            }
        }
    }
}

fn mark_starting(desired: DesiredSvgProbe) {
    let mut control = SVG_PROBE_CONTROL.lock();
    control.status.applied_serial = desired.serial;
    control.status.phase = GpgpuSvgProbePhase::Starting;
    control.status.config = desired.config;
    control.status.frame = None;
    control.status.window = None;
    control.status.metrics = GpgpuSvgProbeMetrics::default();
    control.status.last_error = "none";
}

fn publish_presented_status(probe: &ActiveSvgProbe) {
    let mut control = SVG_PROBE_CONTROL.lock();
    control.status.applied_serial = probe.request_serial;
    control.status.phase = GpgpuSvgProbePhase::Presented;
    control.status.config = probe.config;
    control.status.frame = Some(probe.frame);
    control.status.window = Some(probe.window);
    control.status.width = probe.width;
    control.status.height = probe.height;
    control.status.metrics = probe.metrics;
    control.status.last_error = "none";
}

fn mark_faulted(
    serial: u64,
    config: GpgpuSvgProbeConfig,
    metrics: GpgpuSvgProbeMetrics,
    reason: &'static str,
) {
    let mut control = SVG_PROBE_CONTROL.lock();
    control.status.applied_serial = serial;
    control.status.phase = GpgpuSvgProbePhase::Faulted;
    control.status.config = config;
    control.status.frame = None;
    control.status.window = None;
    control.status.metrics = metrics;
    control.status.last_error = reason;
}

fn mark_idle(serial: u64, reason: &'static str) {
    let mut control = SVG_PROBE_CONTROL.lock();
    control.status.applied_serial = serial;
    control.status.phase = GpgpuSvgProbePhase::Idle;
    control.status.frame = None;
    control.status.window = None;
    control.status.last_error = reason;
}

const fn next_serial(serial: u64) -> u64 {
    let next = serial.wrapping_add(1);
    if next == 0 { 1 } else { next }
}
