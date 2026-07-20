//! Staged UI4 video-path probe.
//!
//! The harness starts at the broker-owned double-buffered Frame and adds one
//! boundary per cut.  Decoder work is deliberately absent until cut 9, so the
//! last emitted checkpoint identifies the first broken ownership transition.

use core::sync::atomic::{AtomicBool, Ordering};

use embassy_time::{Duration, Instant, Timer};

use super::{
    DamageRect, FrameBuffering, FrameCadence, FrameContent, FrameHandle, FramePoolError, FrameSpec,
    OutputId, PremultipliedRgba8, ScanoutFormat, WindowCreate, WindowId, WindowInteraction,
    WindowOwner, WindowPlacement, WindowPlane, WindowSessionCloseRequest, WindowSessionId,
    acquire_frame_buffer, begin_window_session, cancel_frame_buffer, create_frame, create_window,
    destroy_frame, finish_window_session, finish_window_session_with_request, frame_snapshot,
    gpgpu_rgba_surface, publish_frame_buffer, publish_gpgpu_video_frame_buffer,
    publish_window_frame, visible_windows_for_output, writable_rgba_view,
};

const PROBE_OWNER: WindowOwner = WindowOwner::VIDEO_PROBE;
const PROBE_OUTPUT: OutputId = OutputId::from_slot(0).unwrap();
const PROBE_PLANE_SLOT: usize = super::ALPHA_OVERLAY_PLANE_SLOT;
const FRAME_WIDTH: u32 = super::DEFAULT_FRAME_WIDTH;
const FRAME_HEIGHT: u32 = super::DEFAULT_FRAME_HEIGHT;
const ACQUIRE_TIMEOUT_MS: u64 = 2_000;
const PRESENT_TIMEOUT_MS: u64 = 2_000;
const GPU_TIMEOUT_MS: u64 = 5_000;
const CLOSE_TIMEOUT_MS: u64 = 2_000;
const CUT_SETTLE_MS: u64 = 100;
const SHORT_RING_FRAMES: usize = 10;
const GPU_RING_FRAMES: usize = 30;
const SYNTHETIC_NV12_PITCH: u32 = 768;
const SYNTHETIC_NV12_UV_ROW: u32 = 512;
const SYNTHETIC_NV12_UV_OFFSET: u32 = SYNTHETIC_NV12_PITCH * SYNTHETIC_NV12_UV_ROW;
const SYNTHETIC_NV12_TOTAL_ROWS: u32 = 768;
const SYNTHETIC_NV12_BYTES: usize = (SYNTHETIC_NV12_PITCH * SYNTHETIC_NV12_TOTAL_ROWS) as usize;
const SYNTHETIC_MEDIA_GPU: u64 = 0x3000_0000;

static VIDEO_PROBE_ACTIVE: AtomicBool = AtomicBool::new(false);

struct ProbeActiveGuard;

impl Drop for ProbeActiveGuard {
    fn drop(&mut self) {
        VIDEO_PROBE_ACTIVE.store(false, Ordering::Release);
    }
}

#[derive(Copy, Clone)]
struct ProbeWindow {
    session: WindowSessionId,
    frame: FrameHandle,
    window: WindowId,
}

const fn cut_name(cut: u8) -> &'static str {
    match cut {
        1 => "frame-contract",
        2 => "window-attach",
        3 => "cpu-single-present",
        4 => "cpu-double-ring",
        5 => "guc-rgba-single",
        6 => "guc-rgba-double-ring",
        7 => "synthetic-nv12-single",
        8 => "synthetic-nv12-double-ring",
        9 => "decoder-first-frame",
        10 => "decoder-stream",
        _ => "invalid",
    }
}

/// Run all ten cuts (`selection=None`) or exactly one numbered cut.
pub(crate) async fn run_video_probe(selection: Option<u8>, trigger: &str) -> bool {
    if VIDEO_PROBE_ACTIVE
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        crate::log!("ui4/video-probe: rejected trigger={} reason=already-active\n", trigger);
        return false;
    }
    let _guard = ProbeActiveGuard;
    if selection.is_some_and(|cut| !(1..=10).contains(&cut)) {
        crate::log!(
            "ui4/video-probe: rejected trigger={} reason=invalid-cut requested={:?}\n",
            trigger,
            selection
        );
        return false;
    }

    crate::log!(
        "ui4/video-probe: manifest trigger={} selection={} cuts=1:frame-contract,2:window-attach,3:cpu-single-present,4:cpu-double-ring,5:guc-rgba-single,6:guc-rgba-double-ring,7:synthetic-nv12-single,8:synthetic-nv12-double-ring,9:decoder-first-frame,10:decoder-stream policy=stop-first-failure\n",
        trigger,
        selection.map_or(0, usize::from)
    );

    let first = selection.unwrap_or(1);
    let last = selection.unwrap_or(10);
    for cut in first..=last {
        crate::log!(
            "ui4/video-probe: cut={} name={} stage=start trigger={}\n",
            cut,
            cut_name(cut),
            trigger
        );
        let result = run_cut(cut).await;
        match result {
            Ok(frames) => crate::log!(
                "ui4/video-probe: cut={} name={} stage=pass frames={} boundary=surflive-or-contract-confirmed\n",
                cut,
                cut_name(cut),
                frames
            ),
            Err(reason) => {
                crate::log!(
                    "ui4/video-probe: cut={} name={} stage=fail reason={} action=stop-first-failure\n",
                    cut,
                    cut_name(cut),
                    reason
                );
                return false;
            }
        }
        Timer::after(Duration::from_millis(CUT_SETTLE_MS)).await;
    }
    true
}

async fn run_cut(cut: u8) -> Result<usize, &'static str> {
    match cut {
        1 => cut_frame_contract(),
        2 => cut_window_attach(),
        3 => cut_cpu_single_present().await,
        4 => cut_cpu_double_ring().await,
        5 => cut_guc_rgba_single().await,
        6 => cut_guc_rgba_double_ring().await,
        7 => cut_synthetic_nv12(7, 1).await,
        8 => cut_synthetic_nv12(8, GPU_RING_FRAMES).await,
        9 => cut_decoder(9, 1).await,
        10 => cut_decoder(10, 0).await,
        _ => Err("invalid-cut"),
    }
}

fn cut_frame_contract() -> Result<usize, &'static str> {
    let frame = create_probe_frame(Some(opaque(0, 0, 0)))?;
    let snapshot = frame_snapshot(frame).map_err(|_| "frame-snapshot-failed")?;
    let valid = snapshot.plan.content == FrameContent::Video
        && snapshot.plan.cadence == FrameCadence::Streaming
        && snapshot.plan.buffering == FrameBuffering::Double
        && snapshot.plan.format == ScanoutFormat::Rgba8888Premultiplied
        && snapshot.plan.width == FRAME_WIDTH
        && snapshot.plan.height == FRAME_HEIGHT
        && snapshot.buffer_count == 2
        && snapshot.front_buffer.is_none()
        && !snapshot.writer_active;
    crate::log!(
        "ui4/video-probe: cut=1 checkpoint=frame-snapshot frame={} buffers={} front={:?} writer={} valid={}\n",
        frame.raw(),
        snapshot.buffer_count,
        snapshot.front_buffer,
        snapshot.writer_active as u8,
        valid as u8
    );
    let destroyed = destroy_frame(frame).is_ok();
    if !valid {
        return Err("frame-contract-mismatch");
    }
    destroyed.then_some(0).ok_or("frame-destroy-failed")
}

fn cut_window_attach() -> Result<usize, &'static str> {
    let probe = create_probe_window(Some(opaque(0, 0, 0)), 2)?;
    crate::log!(
        "ui4/video-probe: cut=2 checkpoint=window-created frame={} window={} session={} state=pending pixels_published=0\n",
        probe.frame.raw(),
        probe.window.raw(),
        probe.session.raw()
    );
    cleanup_unpublished(probe)?;
    Ok(0)
}

async fn cut_cpu_single_present() -> Result<usize, &'static str> {
    let probe = create_probe_window(Some(opaque(0, 0, 0)), 3)?;
    let lease = acquire_with_timeout(probe.frame).await?;
    let published = publish_frame_buffer(lease).map_err(|_| "cpu-frame-publish-failed")?;
    let serial = publish_window_frame(PROBE_OWNER, probe.window, DamageRect::FULL)
        .map_err(|_| "window-publish-failed")?;
    crate::log!(
        "ui4/video-probe: cut=3 checkpoint=published frame={} buffer={} frame_serial={} window_serial={} producer=cpu-initial-color cpu_copy=0 next=surflive-ack\n",
        probe.frame.raw(),
        published.buffer_index,
        published.publish_serial,
        serial
    );
    wait_window_ack(probe.window, serial).await?;
    close_presented(probe).await?;
    Ok(1)
}

async fn cut_cpu_double_ring() -> Result<usize, &'static str> {
    let probe = create_probe_window(Some(opaque(0, 0, 0)), 4)?;
    for index in 0..SHORT_RING_FRAMES {
        let lease = acquire_with_timeout(probe.frame).await?;
        let view = writable_rgba_view(lease).map_err(|_| "cpu-view-unavailable")?;
        fill_rgba(
            view,
            if index.is_multiple_of(2) {
                opaque(8, 24, 40)
            } else {
                opaque(16, 72, 96)
            },
        )?;
        let published = publish_frame_buffer(lease).map_err(|_| "cpu-ring-publish-failed")?;
        let serial = publish_window_frame(PROBE_OWNER, probe.window, DamageRect::FULL)
            .map_err(|_| "window-publish-failed")?;
        crate::log!(
            "ui4/video-probe: cut=4 checkpoint=published index={} frame={} buffer={} frame_serial={} window_serial={} next=surflive-ack\n",
            index + 1,
            probe.frame.raw(),
            published.buffer_index,
            published.publish_serial,
            serial
        );
        wait_window_ack(probe.window, serial).await?;
    }
    close_presented(probe).await?;
    Ok(SHORT_RING_FRAMES)
}

async fn cut_guc_rgba_single() -> Result<usize, &'static str> {
    let probe = create_probe_window(None, 5)?;
    publish_chart_frame(5, probe, 0, 0.0).await?;
    close_presented(probe).await?;
    Ok(1)
}

async fn cut_guc_rgba_double_ring() -> Result<usize, &'static str> {
    let probe = create_probe_window(None, 6)?;
    for index in 0..GPU_RING_FRAMES {
        publish_chart_frame(6, probe, index, index as f32 * 0.025).await?;
    }
    close_presented(probe).await?;
    Ok(GPU_RING_FRAMES)
}

async fn publish_chart_frame(
    cut: u8,
    probe: ProbeWindow,
    index: usize,
    phase: f32,
) -> Result<(), &'static str> {
    let lease = acquire_with_timeout(probe.frame).await?;
    let surface = gpgpu_rgba_surface(lease).map_err(|_| "guc-destination-unavailable")?;
    crate::log!(
        "ui4/video-probe: cut={} checkpoint=pre-submit index={} producer=chart-simd16 dst_gpu=0x{:X} dst_phys=0x{:X} bytes=0x{:X}\n",
        cut,
        index + 1,
        surface.gpu,
        surface.phys,
        surface.bytes
    );
    let result = crate::intel::gpgpu::chart_sine_rgba8_surface_full(
        surface,
        phase,
        crate::intel::gpgpu::CHART_SINE_FLAG_GRID
            | crate::intel::gpgpu::CHART_SINE_FLAG_AXES
            | crate::intel::gpgpu::CHART_SINE_FLAG_BORDER,
    );
    crate::log!(
        "ui4/video-probe: cut={} checkpoint=post-submit index={} submitted={} ok={} marker=0x{:X} submit_ms={}\n",
        cut,
        index + 1,
        result.submitted as u8,
        result.ok as u8,
        result.marker,
        result.submit_ms
    );
    let Some(release) = result.release.filter(|_| result.ok) else {
        if !result.submitted {
            let _ = cancel_frame_buffer(lease);
        }
        return Err(if result.submitted {
            "guc-chart-failed-lease-quarantined"
        } else {
            "guc-chart-not-submitted"
        });
    };
    let published = publish_gpgpu_video_frame_buffer(lease, release)
        .map_err(|_| "guc-video-frame-publish-failed")?;
    let serial = publish_window_frame(PROBE_OWNER, probe.window, DamageRect::FULL)
        .map_err(|_| "window-publish-failed")?;
    crate::log!(
        "ui4/video-probe: cut={} checkpoint=producer-released index={} buffer={} frame_serial={} window_serial={} release={} next=surflive-ack\n",
        cut,
        index + 1,
        published.buffer_index,
        published.publish_serial,
        serial,
        release.sequence()
    );
    wait_window_ack(probe.window, serial).await
}

async fn cut_synthetic_nv12(cut: u8, frames: usize) -> Result<usize, &'static str> {
    let probe = create_probe_window(None, cut)?;
    let (source_phys, source_ptr) =
        crate::dma::alloc(SYNTHETIC_NV12_BYTES, crate::intel::WARM_ALIGN)
            .ok_or("synthetic-nv12-allocation-failed")?;
    let source_virt = source_ptr as usize;
    fill_synthetic_nv12(source_virt as *mut u8, SYNTHETIC_NV12_BYTES)?;
    let source = crate::intel::gpgpu::GpgpuNv12Tile64Surface::new(
        source_phys,
        SYNTHETIC_MEDIA_GPU,
        SYNTHETIC_NV12_BYTES,
        FRAME_WIDTH,
        FRAME_HEIGHT,
        SYNTHETIC_NV12_PITCH,
        SYNTHETIC_NV12_UV_OFFSET,
    )
    .ok_or("synthetic-nv12-contract-invalid")?;
    crate::log!(
        "ui4/video-probe: cut={} checkpoint=source-ready source=synthetic-tile64-nv12 phys=0x{:X} media_gpu=0x{:X} bytes=0x{:X} pitch={} uv_offset=0x{:X}\n",
        cut,
        source.phys,
        source.gpu,
        source.bytes,
        source.pitch_bytes,
        source.uv_offset
    );

    for index in 0..frames {
        let lease = acquire_with_timeout(probe.frame).await?;
        let destination = gpgpu_rgba_surface(lease).map_err(|_| "nv12-destination-unavailable")?;
        crate::log!(
            "ui4/video-probe: cut={} checkpoint=pre-submit index={} producer=synthetic-nv12-simd16 source_phys=0x{:X} dst_gpu=0x{:X} dst_phys=0x{:X}\n",
            cut,
            index + 1,
            source.phys,
            destination.gpu,
            destination.phys
        );
        let submission = queue_video_conversion_with_timeout(source, destination).await?;
        crate::log!(
            "ui4/video-probe: cut={} checkpoint=submit-accepted index={} next=guc-completion\n",
            cut,
            index + 1
        );
        let release = wait_video_conversion(cut, index, submission, destination).await?;
        let published = publish_gpgpu_video_frame_buffer(lease, release)
            .map_err(|_| "nv12-video-frame-publish-failed")?;
        let serial = publish_window_frame(PROBE_OWNER, probe.window, DamageRect::FULL)
            .map_err(|_| "window-publish-failed")?;
        crate::log!(
            "ui4/video-probe: cut={} checkpoint=producer-released index={} buffer={} frame_serial={} window_serial={} release={} next=surflive-ack\n",
            cut,
            index + 1,
            published.buffer_index,
            published.publish_serial,
            serial,
            release.sequence()
        );
        wait_window_ack(probe.window, serial).await?;
    }

    crate::dma::dealloc(source_virt as *mut u8, SYNTHETIC_NV12_BYTES);
    close_presented(probe).await?;
    Ok(frames)
}

async fn queue_video_conversion_with_timeout(
    source: crate::intel::gpgpu::GpgpuNv12Tile64Surface,
    destination: crate::intel::gpgpu::GpgpuRgba8Surface,
) -> Result<crate::intel::gpgpu::Ui4CompositorSubmission, &'static str> {
    let started = Instant::now();
    loop {
        match crate::intel::gpgpu::queue_ui4_video_frame_nv12_tile64_to_rgba8(
            source,
            destination,
            0,
            0,
            FRAME_WIDTH,
            FRAME_HEIGHT,
            0,
            0,
        ) {
            Ok(submission) => return Ok(submission),
            Err(crate::intel::gpgpu::Ui4CompositorSubmitError::Busy)
                if started.elapsed().as_millis() < GPU_TIMEOUT_MS =>
            {
                Timer::after(Duration::from_millis(1)).await;
            }
            Err(crate::intel::gpgpu::Ui4CompositorSubmitError::Busy) => {
                return Err("nv12-guc-queue-busy-timeout");
            }
            Err(crate::intel::gpgpu::Ui4CompositorSubmitError::Unavailable) => {
                return Err("nv12-guc-unavailable");
            }
            Err(crate::intel::gpgpu::Ui4CompositorSubmitError::InvalidWorklist) => {
                return Err("nv12-guc-invalid-worklist");
            }
            Err(crate::intel::gpgpu::Ui4CompositorSubmitError::SubmissionRejected) => {
                return Err("nv12-guc-submission-rejected");
            }
        }
    }
}

async fn wait_video_conversion(
    cut: u8,
    index: usize,
    submission: crate::intel::gpgpu::Ui4CompositorSubmission,
    destination: crate::intel::gpgpu::GpgpuRgba8Surface,
) -> Result<crate::intel::gpgpu::GpgpuRgba8ReleaseFence, &'static str> {
    let started = Instant::now();
    loop {
        match crate::intel::gpgpu::poll_ui4_video_frame_submission(submission, destination) {
            crate::intel::gpgpu::Ui4VideoFrameCompletion::Pending
                if started.elapsed().as_millis() < GPU_TIMEOUT_MS =>
            {
                Timer::after(Duration::from_millis(1)).await;
            }
            crate::intel::gpgpu::Ui4VideoFrameCompletion::Pending => {
                crate::log!(
                    "ui4/video-probe: cut={} checkpoint=guc-timeout index={} action=quarantine-source-and-write-lease\n",
                    cut,
                    index + 1
                );
                return Err("nv12-guc-completion-timeout-quarantined");
            }
            crate::intel::gpgpu::Ui4VideoFrameCompletion::Complete { stats, release } => {
                crate::log!(
                    "ui4/video-probe: cut={} checkpoint=guc-complete index={} submit_ms={} release={}\n",
                    cut,
                    index + 1,
                    stats.submit_ms,
                    release.sequence()
                );
                return Ok(release);
            }
            crate::intel::gpgpu::Ui4VideoFrameCompletion::Failed => {
                crate::log!(
                    "ui4/video-probe: cut={} checkpoint=guc-failed index={} action=quarantine-source-and-write-lease\n",
                    cut,
                    index + 1
                );
                return Err("nv12-guc-completion-failed-quarantined");
            }
        }
    }
}

async fn cut_decoder(cut: u8, frame_limit: usize) -> Result<usize, &'static str> {
    if !super::begin_shell_decoded_video_player() {
        return Err("decoder-ui4-lifetime-unavailable");
    }
    crate::log!(
        "ui4/video-probe: cut={} checkpoint=decoder-lifetime-reserved frame_limit={} asset={} next=decode-submit\n",
        cut,
        frame_limit,
        crate::intel::media::hw_vid::H264_BOOT_PROBE_STREAM_PATH
    );
    let options = crate::intel::media::hw_vid::H264PlaybackOptions::new(
        60,
        false,
        crate::intel::media::hw_vid::H264PlaybackCacheMode::Off,
        false,
        false,
        false,
        true,
        false,
    )
    .with_frame_limit(frame_limit);
    crate::log!(
        "ui4/video-probe: cut={} checkpoint=decoder-options-ready frame_limit={} next=kernel-decoder-direct\n",
        cut,
        frame_limit
    );
    let report = match crate::intel::media::hw_vid::run_kernel_video_probe_playback(options).await {
        Ok(report) => report,
        Err(error) => {
            let _ = super::stop_decoded_nv12_stream("video-probe-decode-failed");
            crate::log!("ui4/video-probe: cut={} checkpoint=decoder-return error={}\n", cut, error);
            return Err("decoder-playback-failed");
        }
    };
    crate::log!(
        "ui4/video-probe: cut={} checkpoint=decoder-return submitted={} skipped={} elapsed_ms={} next=latest-surflive-ack\n",
        cut,
        report.submitted,
        report.skipped_unsupported,
        report.elapsed_ms
    );
    if report.submitted == 0 || (frame_limit != 0 && report.submitted != frame_limit) {
        let _ = super::stop_decoded_nv12_stream("video-probe-frame-count-mismatch");
        return Err("decoder-frame-count-mismatch");
    }
    if !super::wait_decoded_video_presented(PRESENT_TIMEOUT_MS).await {
        let _ = super::stop_decoded_nv12_stream("video-probe-present-timeout");
        return Err("decoder-surflive-ack-timeout");
    }
    if !super::stop_decoded_nv12_stream("video-probe-cut-complete") {
        return Err("decoder-window-close-rejected");
    }
    wait_owner_windows_gone(WindowOwner::VIDEO_PLAYER).await?;
    Ok(report.submitted)
}

fn create_probe_frame(base_color: Option<PremultipliedRgba8>) -> Result<FrameHandle, &'static str> {
    create_frame(FrameSpec {
        output: PROBE_OUTPUT,
        content: FrameContent::Video,
        cadence: FrameCadence::Streaming,
        buffering: FrameBuffering::Double,
        format: ScanoutFormat::Rgba8888Premultiplied,
        width: FRAME_WIDTH,
        height: FRAME_HEIGHT,
        base_color,
    })
    .map_err(|_| "frame-create-failed")
}

fn create_probe_window(
    base_color: Option<PremultipliedRgba8>,
    cut: u8,
) -> Result<ProbeWindow, &'static str> {
    let frame = create_probe_frame(base_color)?;
    let session = match begin_window_session(PROBE_OWNER) {
        Ok(session) => session,
        Err(_) => {
            let _ = destroy_frame(frame);
            return Err("window-session-create-failed");
        }
    };
    let (output_width, output_height) =
        crate::intel::active_scanout_dimensions().unwrap_or((FRAME_WIDTH, FRAME_HEIGHT));
    let placement = WindowPlacement {
        x: (output_width.saturating_sub(FRAME_WIDTH) / 2) as i32,
        y: (output_height.saturating_sub(FRAME_HEIGHT) / 2) as i32,
        width: FRAME_WIDTH,
        height: FRAME_HEIGHT,
        z: 110,
        opacity: 0xFF,
        visible: true,
    };
    let window = match create_window(WindowCreate {
        owner: PROBE_OWNER,
        session,
        frame,
        output: PROBE_OUTPUT,
        plane: WindowPlane::Universal(PROBE_PLANE_SLOT as u8),
        placement,
        interaction: WindowInteraction::APPLICATION_FIXED_FRAME,
    }) {
        Ok(window) => window,
        Err(_) => {
            let _ = finish_window_session(PROBE_OWNER, session);
            let _ = destroy_frame(frame);
            return Err("window-create-failed");
        }
    };
    crate::log!(
        "ui4/video-probe: cut={} checkpoint=window-ready-for-producer frame={} window={} session={} buffers=2 size={}x{} placement={},{} plane_slot={} state=pending\n",
        cut,
        frame.raw(),
        window.raw(),
        session.raw(),
        FRAME_WIDTH,
        FRAME_HEIGHT,
        placement.x,
        placement.y,
        PROBE_PLANE_SLOT
    );
    Ok(ProbeWindow {
        session,
        frame,
        window,
    })
}

fn cleanup_unpublished(probe: ProbeWindow) -> Result<(), &'static str> {
    finish_window_session(PROBE_OWNER, probe.session)
        .map_err(|_| "window-session-finish-failed")?;
    destroy_frame(probe.frame).map_err(|_| "frame-destroy-failed")
}

async fn close_presented(probe: ProbeWindow) -> Result<(), &'static str> {
    finish_window_session_with_request(
        PROBE_OWNER,
        probe.session,
        WindowSessionCloseRequest::default().direct_plane_animate_and_retire_frames(),
    )
    .map_err(|_| "animated-close-start-failed")?;
    crate::log!(
        "ui4/video-probe: checkpoint=close-start frame={} window={} lifecycle=shrink-fade+surflive-retire\n",
        probe.frame.raw(),
        probe.window.raw()
    );
    wait_window_gone(probe.window).await
}

async fn acquire_with_timeout(frame: FrameHandle) -> Result<super::FrameWriteLease, &'static str> {
    let started = Instant::now();
    loop {
        match acquire_frame_buffer(frame) {
            Ok(lease) => return Ok(lease),
            Err(FramePoolError::Busy) if started.elapsed().as_millis() < ACQUIRE_TIMEOUT_MS => {
                Timer::after(Duration::from_millis(1)).await;
            }
            Err(FramePoolError::Busy) => return Err("frame-acquire-timeout"),
            Err(_) => return Err("frame-acquire-failed"),
        }
    }
}

async fn wait_window_ack(window: WindowId, serial: u64) -> Result<(), &'static str> {
    let started = Instant::now();
    loop {
        let acknowledged = visible_windows_for_output(PROBE_OUTPUT)
            .iter()
            .find(|snapshot| snapshot.id == window)
            .is_some_and(|snapshot| snapshot.publish_serial == serial && snapshot.damage.is_none());
        if acknowledged {
            crate::log!(
                "ui4/video-probe: checkpoint=surflive-ack window={} window_serial={} elapsed_ms={}\n",
                window.raw(),
                serial,
                started.elapsed().as_millis()
            );
            return Ok(());
        }
        if started.elapsed().as_millis() >= PRESENT_TIMEOUT_MS {
            return Err("surflive-ack-timeout");
        }
        Timer::after(Duration::from_millis(1)).await;
    }
}

async fn wait_window_gone(window: WindowId) -> Result<(), &'static str> {
    let started = Instant::now();
    loop {
        if visible_windows_for_output(PROBE_OUTPUT)
            .iter()
            .all(|snapshot| snapshot.id != window)
        {
            crate::log!(
                "ui4/video-probe: checkpoint=close-complete window={} elapsed_ms={}\n",
                window.raw(),
                started.elapsed().as_millis()
            );
            return Ok(());
        }
        if started.elapsed().as_millis() >= CLOSE_TIMEOUT_MS {
            return Err("window-close-timeout");
        }
        Timer::after(Duration::from_millis(2)).await;
    }
}

async fn wait_owner_windows_gone(owner: WindowOwner) -> Result<(), &'static str> {
    let started = Instant::now();
    loop {
        if visible_windows_for_output(PROBE_OUTPUT)
            .iter()
            .all(|snapshot| snapshot.owner != owner)
        {
            return Ok(());
        }
        if started.elapsed().as_millis() >= CLOSE_TIMEOUT_MS {
            return Err("decoder-window-close-timeout");
        }
        Timer::after(Duration::from_millis(2)).await;
    }
}

fn fill_rgba(view: super::FrameRgbaView, color: PremultipliedRgba8) -> Result<(), &'static str> {
    if view.virt.is_null()
        || view.pitch < view.width.saturating_mul(4)
        || (view.pitch as usize).saturating_mul(view.height as usize) > view.byte_len
    {
        return Err("cpu-rgba-view-invalid");
    }
    let pixel = u32::from_le_bytes(color.to_native_bytes());
    for y in 0..view.height as usize {
        let row = unsafe { view.virt.add(y * view.pitch as usize).cast::<u32>() };
        for x in 0..view.width as usize {
            unsafe { core::ptr::write_volatile(row.add(x), pixel) };
        }
    }
    crate::intel::dma_flush(view.virt, view.byte_len);
    Ok(())
}

fn fill_synthetic_nv12(virt: *mut u8, bytes: usize) -> Result<(), &'static str> {
    if virt.is_null() || bytes < SYNTHETIC_NV12_BYTES {
        return Err("synthetic-nv12-view-invalid");
    }
    unsafe { core::ptr::write_bytes(virt, 0, bytes) };
    let tiles_per_row = (SYNTHETIC_NV12_PITCH / 256) as usize;
    for y in 0..FRAME_HEIGHT as usize {
        for x in 0..FRAME_WIDTH as usize {
            let offset = crate::intel::media::xelp_media2_ngin::media_tile64_8bpp_offset(
                x,
                y,
                tiles_per_row,
            );
            if offset >= bytes {
                return Err("synthetic-nv12-luma-overflow");
            }
            let luma = 16usize + x.saturating_mul(219) / FRAME_WIDTH as usize;
            unsafe { core::ptr::write_volatile(virt.add(offset), luma as u8) };
        }
    }
    for y in 0..(FRAME_HEIGHT as usize / 2) {
        let row = SYNTHETIC_NV12_UV_ROW as usize + y;
        for x in (0..FRAME_WIDTH as usize).step_by(2) {
            let offset = crate::intel::media::xelp_media2_ngin::media_tile64_8bpp_offset(
                x,
                row,
                tiles_per_row,
            );
            if offset + 1 >= bytes {
                return Err("synthetic-nv12-chroma-overflow");
            }
            unsafe {
                core::ptr::write_volatile(virt.add(offset), 96);
                core::ptr::write_volatile(virt.add(offset + 1), 176);
            }
        }
    }
    crate::intel::dma_flush(virt, bytes);
    Ok(())
}

const fn opaque(r: u8, g: u8, b: u8) -> PremultipliedRgba8 {
    PremultipliedRgba8::from_straight_rgba(r, g, b, 0xFF)
}
