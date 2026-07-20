//! UI4 ownership wrapper for decoded video.
//!
//! The decoder remains a native media-Y-tiled NV12 producer. One SIMD16 GuC dispatch
//! writes an exact broker-owned RGBA backbuffer. GuC completion releases the
//! decoder picture; UI4 publication transfers only the completed RGBA surface,
//! whose display ownership independently ends at SURFLIVE.
//! The older `Tile64` Rust/kernel symbol names are retained as an artifact ABI;
//! the shader uses the proven 128x32 media Y-tile byte layout.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use embassy_time::{Duration, Timer};
use spin::Mutex;

use super::{
    DamageRect, FrameBuffering, FrameCadence, FrameContent, FrameHandle, FrameSpec, OutputId,
    ScanoutFormat, Ui4InputEvent, WindowCreate, WindowId, WindowOwner, WindowPlacement,
    WindowSessionCloseRequest, WindowSessionId, acquire_frame_buffer, begin_window_session,
    cancel_frame_buffer, create_frame, create_window, destroy_frame, finish_window_session,
    finish_window_session_with_request, gpgpu_rgba_surface, publish_gpgpu_video_frame_buffer,
    publish_window_frame, take_owner_input_events,
};

// The decoded-video producer owns one ordinary broker window independently of
// the compositor service.
const VIDEO_OWNER: WindowOwner = WindowOwner::VIDEO_PLAYER;
const VIDEO_OUTPUT: OutputId = OutputId::from_slot(0).unwrap();
const VIDEO_PLANE_SLOT: usize = super::ALPHA_OVERLAY_PLANE_SLOT;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct DecodedNv12Source {
    pub(crate) decode_sequence: u64,
    pub(crate) gpu: u64,
    pub(crate) phys: u64,
    pub(crate) virt: usize,
    pub(crate) byte_len: usize,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) visible_width: u32,
    pub(crate) visible_height: u32,
    pub(crate) pitch_bytes: usize,
    pub(crate) uv_offset: usize,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeViewportLayout {
    pub(crate) source_x: u32,
    pub(crate) source_y: u32,
    pub(crate) destination_x: u32,
    pub(crate) destination_y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

/// Dimensions used to bind one decoded picture to its UI4 viewport. Pixel
/// storage stays decoder-owned until GuC completion; only the converted RGBA
/// allocation enters the UI4 publication lifetime.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct DecodedVideoFrameSpec {
    pub(crate) coded_width: u32,
    pub(crate) coded_height: u32,
    pub(crate) visible_width: u32,
    pub(crate) visible_height: u32,
}

impl DecodedVideoFrameSpec {
    const fn from_nv12_source(source: DecodedNv12Source) -> Self {
        Self {
            coded_width: source.width,
            coded_height: source.height,
            visible_width: source.visible_width,
            visible_height: source.visible_height,
        }
    }

    const fn valid(self) -> bool {
        self.coded_width != 0
            && self.coded_height != 0
            && self.visible_width != 0
            && self.visible_height != 0
            && self.visible_width <= self.coded_width
            && self.visible_height <= self.coded_height
    }
}

#[derive(Copy, Clone)]
struct VideoStream {
    session: WindowSessionId,
    frame: FrameHandle,
    window: WindowId,
    source_width: u32,
    source_height: u32,
    visible_width: u32,
    visible_height: u32,
    frame_width: u32,
    frame_height: u32,
    pan_x: u32,
    pan_y: u32,
    active_pan_source: Option<super::Ui4CursorSource>,
}

static VIDEO_STREAM: Mutex<Option<VideoStream>> = Mutex::new(None);
static VIDEO_RETIRED_FRAMES: Mutex<Vec<FrameHandle>> = Mutex::new(Vec::new());
static VIDEO_PUBLISH_SEQ: AtomicU64 = AtomicU64::new(0);
static VIDEO_LIFECYCLE_RESERVED: AtomicBool = AtomicBool::new(false);

/// Reserve the decoded-video lifetime without allocating or publishing pixels.
/// The exact double-buffered Frame is created only when the decoder supplies
/// its first real source. This keeps all DMA allocation and PPGTT work on the
/// producer handoff side of the TRUEOSFS/decode boundary.
pub(crate) fn begin_shell_decoded_video_player() -> bool {
    if VIDEO_LIFECYCLE_RESERVED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return false;
    }
    crate::log_info!(
        target: "ui4";
        "ui4 video-player lifetime reserved owner={:?} playback=playing control=broker-pan source=await-first-decoded-frame lifecycle_owner=shell2-vid-task frame=deferred window=deferred broker_state=deferred-until-first-decoded-frame rgba_ring_allocation=deferred placeholder_present=0\n",
        VIDEO_OWNER,
    );
    true
}

fn poll_decoded_video_player_input() {
    reap_retired_video_frames();
    for event in take_owner_input_events(VIDEO_OWNER) {
        match event {
            Ui4InputEvent::Pan(event) => pan_video_viewport(event),
            Ui4InputEvent::Resize(event) => crate::log_warn!(
                target: "ui4";
                "ui4 video-player resize ignored window={} extent={}x{} reason=fixed-shell-vid-frame no-placeholder-publish=1\n",
                event.window.raw(), event.width, event.height,
            ),
            _ => {}
        }
    }
}

fn pan_video_viewport(event: super::Ui4PanEvent) {
    let mut slot = VIDEO_STREAM.lock();
    let Some(stream) = slot.as_mut() else {
        return;
    };
    if event.window != stream.window {
        return;
    }
    match event.phase {
        super::Ui4PanPhase::Begin => stream.active_pan_source = Some(event.source),
        super::Ui4PanPhase::Update if stream.active_pan_source == Some(event.source) => {
            stream.pan_x = move_crop_origin(
                stream.pan_x,
                event.dx,
                stream.visible_width.saturating_sub(stream.frame_width),
            );
            stream.pan_y = move_crop_origin(
                stream.pan_y,
                event.dy,
                stream.visible_height.saturating_sub(stream.frame_height),
            );
        }
        super::Ui4PanPhase::End if stream.active_pan_source == Some(event.source) => {
            stream.active_pan_source = None;
            crate::log_info!(
                target: "ui4";
                "ui4 video-player pan ended window={} native={}x{} viewport={}x{} crop_origin={},{} scaling=none-1to1\n",
                stream.window.raw(),
                stream.visible_width,
                stream.visible_height,
                stream.frame_width,
                stream.frame_height,
                stream.pan_x,
                stream.pan_y,
            );
        }
        _ => {}
    }
}

fn move_crop_origin(origin: u32, drag_delta: i32, maximum: u32) -> u32 {
    (i64::from(origin) - i64::from(drag_delta)).clamp(0, i64::from(maximum)) as u32
}

fn cleanup_uninstalled_stream(stream: VideoStream) {
    let _ = finish_window_session(VIDEO_OWNER, stream.session);
    retire_video_frame(stream.frame);
}

fn retire_video_frame(frame: FrameHandle) {
    match destroy_frame(frame) {
        Ok(()) | Err(super::FramePoolError::InvalidHandle) => {}
        Err(super::FramePoolError::Busy) => {
            let mut retired = VIDEO_RETIRED_FRAMES.lock();
            if !retired.contains(&frame) {
                retired.push(frame);
            }
        }
        Err(error) => crate::log_warn!(
            target: "ui4";
            "ui4 video-frame retire abandoned frame={} error={:?}\n",
            frame.raw(),
            error,
        ),
    }
}

fn reap_retired_video_frames() {
    VIDEO_RETIRED_FRAMES
        .lock()
        .retain(|frame| matches!(destroy_frame(*frame), Err(super::FramePoolError::Busy)));
}

/// Convert one decoder picture into an exact UI4 RGBA backbuffer. The function
/// returns after the GuC read of NV12 has retired and the RGBA publication is
/// visible to the broker; it deliberately does not wait for display SURFLIVE.
pub(crate) async fn present_decoded_nv12_stream_frame(
    source: DecodedNv12Source,
    reason: &str,
) -> bool {
    if !valid_source(source) {
        return false;
    }
    // Shell-driven playback owns the same application window as boot playback;
    // drain its broker queue at frame cadence so move/resize/pan never depends
    // on the boot-only pause gate.
    poll_decoded_video_player_input();
    let Some(stream) =
        bind_decoded_source_stream(DecodedVideoFrameSpec::from_nv12_source(source), reason)
    else {
        return false;
    };
    let Some(layout) = native_viewport_layout(
        stream.visible_width,
        stream.visible_height,
        stream.frame_width,
        stream.frame_height,
        stream.pan_x,
        stream.pan_y,
    ) else {
        return false;
    };
    let write = loop {
        match acquire_frame_buffer(stream.frame) {
            Ok(write) => break write,
            Err(super::FramePoolError::Busy) => {
                // Double buffering intentionally applies display backpressure:
                // one surface may be live while the other is the sole producer
                // target. No decoder pixels are copied or published here.
                Timer::after(Duration::from_millis(1)).await;
            }
            Err(error) => {
                crate::log_warn!(target: "ui4";
                    "ui4 video-frame acquire failed reason={} error={:?}\n", reason, error,
                );
                return false;
            }
        }
    };
    let destination = match gpgpu_rgba_surface(write) {
        Ok(surface) => surface,
        Err(error) => {
            let _ = cancel_frame_buffer(write);
            crate::log_warn!(target: "ui4";
                "ui4 video-frame destination unavailable frame={} buffer={} error={:?} reason={}\n",
                stream.frame.raw(), write.buffer_index, error, reason,
            );
            return false;
        }
    };
    let pitch_bytes = match u32::try_from(source.pitch_bytes) {
        Ok(pitch) => pitch,
        Err(_) => {
            let _ = cancel_frame_buffer(write);
            return false;
        }
    };
    let uv_offset = match u32::try_from(source.uv_offset) {
        Ok(offset) => offset,
        Err(_) => {
            let _ = cancel_frame_buffer(write);
            return false;
        }
    };
    let Some(native_source) = crate::intel::gpgpu::GpgpuNv12Tile64Surface::new(
        source.phys,
        source.gpu,
        source.byte_len,
        source.width,
        source.height,
        pitch_bytes,
        uv_offset,
    ) else {
        let _ = cancel_frame_buffer(write);
        crate::log_warn!(target: "ui4";
            "ui4 video-frame native source rejected frame={} decode_seq={} media_gpu=0x{:X} reason={}\n",
            stream.frame.raw(), source.decode_sequence, source.gpu, reason,
        );
        return false;
    };

    let submission = loop {
        match crate::intel::gpgpu::queue_ui4_video_frame_nv12_tile64_to_rgba8(
            native_source,
            destination,
            layout.destination_x,
            layout.destination_y,
            layout.width,
            layout.height,
            layout.source_x,
            layout.source_y,
        ) {
            Ok(submission) => break submission,
            Err(crate::intel::gpgpu::Ui4CompositorSubmitError::Busy) => {
                // The dedicated Frame lease and decoder picture remain pinned
                // until this GuC runtime accepts their exact handoff.
                Timer::after(Duration::from_millis(1)).await;
            }
            Err(error) => {
                let _ = cancel_frame_buffer(write);
                crate::log_warn!(target: "ui4";
                    "ui4 video-frame GuC queue failed frame={} buffer={} decode_seq={} error={:?} reason={}\n",
                    stream.frame.raw(), write.buffer_index, source.decode_sequence, error, reason,
                );
                return false;
            }
        }
    };
    let mut completion_failure_logged = false;
    let (release, submit_ms) = loop {
        match crate::intel::gpgpu::poll_ui4_video_frame_submission(submission, destination) {
            crate::intel::gpgpu::Ui4VideoFrameCompletion::Pending => {
                Timer::after(Duration::from_millis(1)).await;
            }
            crate::intel::gpgpu::Ui4VideoFrameCompletion::Complete { stats, release } => {
                break (release, stats.submit_ms);
            }
            crate::intel::gpgpu::Ui4VideoFrameCompletion::Failed => {
                // An accepted request has no cancellation proof. Keep the NV12
                // picture and write lease quarantined instead of permitting
                // either allocation to be reused under unfinished GPU work.
                if !completion_failure_logged {
                    completion_failure_logged = true;
                    crate::log_error!(target: "ui4";
                        "ui4 video-frame GuC completion failed frame={} buffer={} decode_seq={} reason={} action=retain-native-and-write-lease-no-publish log=once\n",
                        stream.frame.raw(), write.buffer_index, source.decode_sequence, reason,
                    );
                }
                Timer::after(Duration::from_millis(1)).await;
            }
        }
    };
    // From this point onward the decoder source is no longer referenced by any
    // queued GPU command. Only the completed RGBA allocation crosses into UI4.
    let sequence = VIDEO_PUBLISH_SEQ.fetch_add(1, Ordering::AcqRel) + 1;
    let published = match publish_gpgpu_video_frame_buffer(write, release) {
        Ok(published) => published,
        Err(error) => {
            let _ = cancel_frame_buffer(write);
            crate::log_warn!(target: "ui4";
                "ui4 video-frame GPU publish failed frame={} buffer={} error={:?} reason={}\n",
                stream.frame.raw(), write.buffer_index, error, reason,
            );
            return false;
        }
    };
    let window_serial = match publish_window_frame(VIDEO_OWNER, stream.window, DamageRect::FULL) {
        Ok(serial) => serial,
        Err(error) => {
            crate::log_warn!(target: "ui4";
                "ui4 video-frame window publish failed frame={} window={} error={:?} reason={} action=close-stream source_already_released_at=guc-completion\n",
                stream.frame.raw(), stream.window.raw(), error, reason,
            );
            let _ = stop_decoded_nv12_stream("window-publish-failed");
            return false;
        }
    };
    if sequence <= 8 || sequence.is_multiple_of(120) {
        crate::log_info!(target: "ui4";
            "ui4 video-frame published seq={} decode_seq={} frame={} window={} buffer={} frame_serial={} window_serial={} producer=guc-nv12-to-ui4-rgba8-frame producer_release={} submit_ms={} source=media-ytile-nv12 {}x{} visible={}x{} crop={}x{}@{},{} destination={},{} media_gpu=0x{:X} target_gpu=0x{:X} frame_buffers=2 plane_route=slot1-rgba8 decoder_source_release=guc-completion display_release=surflive native_attachment=0 linked_nv12_slots=0 producer_plane_mmio=0 cpu_pixel_copy=0\n",
            sequence,
            source.decode_sequence,
            stream.frame.raw(),
            stream.window.raw(),
            published.buffer_index,
            published.publish_serial,
            window_serial,
            release.sequence(),
            submit_ms,
            source.width,
            source.height,
            source.visible_width,
            source.visible_height,
            layout.width,
            layout.height,
            layout.source_x,
            layout.source_y,
            layout.destination_x,
            layout.destination_y,
            source.gpu,
            destination.gpu,
        );
    }
    true
}

fn bind_decoded_source_stream(spec: DecodedVideoFrameSpec, reason: &str) -> Option<VideoStream> {
    if !spec.valid() || !VIDEO_LIFECYCLE_RESERVED.load(Ordering::Acquire) {
        return None;
    }
    {
        let mut slot = VIDEO_STREAM.lock();
        if let Some(stream) = slot.as_mut() {
            let source_changed = stream.source_width != spec.coded_width
                || stream.source_height != spec.coded_height
                || stream.visible_width != spec.visible_width
                || stream.visible_height != spec.visible_height;
            stream.source_width = spec.coded_width;
            stream.source_height = spec.coded_height;
            stream.visible_width = spec.visible_width;
            stream.visible_height = spec.visible_height;
            if source_changed {
                stream.pan_x = centered_crop_origin(spec.visible_width, stream.frame_width);
                stream.pan_y = centered_crop_origin(spec.visible_height, stream.frame_height);
                let layout = native_viewport_layout(
                    stream.visible_width,
                    stream.visible_height,
                    stream.frame_width,
                    stream.frame_height,
                    stream.pan_x,
                    stream.pan_y,
                )?;
                crate::log_info!(target: "ui4";
                    "ui4 video-player source-bound frame={} window={} source={}x{} visible={}x{} viewport={}x{} source_crop={}x{}@{},{} destination={},{} scaling=none-1to1 playback=playing producer=guc-nv12-to-ui4-rgba8-frame attachment=none frame_buffers=2 plane_slot={} reason={}\n",
                    stream.frame.raw(),
                    stream.window.raw(),
                    spec.coded_width,
                    spec.coded_height,
                    spec.visible_width,
                    spec.visible_height,
                    stream.frame_width,
                    stream.frame_height,
                    layout.width,
                    layout.height,
                    layout.source_x,
                    layout.source_y,
                    layout.destination_x,
                    layout.destination_y,
                    VIDEO_PLANE_SLOT,
                    reason,
                );
            }
            return Some(*stream);
        }
    }
    let stream = create_stream(spec)?;
    let mut slot = VIDEO_STREAM.lock();
    if let Some(existing) = *slot {
        drop(slot);
        cleanup_uninstalled_stream(stream);
        return Some(existing);
    }
    *slot = Some(stream);
    Some(stream)
}

pub(crate) fn stop_decoded_nv12_stream(reason: &str) -> bool {
    let reserved = VIDEO_LIFECYCLE_RESERVED.swap(false, Ordering::AcqRel);
    let stream = VIDEO_STREAM.lock().take();
    if let Some(stream) = stream {
        let animated = finish_window_session_with_request(
            VIDEO_OWNER,
            stream.session,
            WindowSessionCloseRequest::default().direct_plane_animate_and_retire_frames(),
        )
        .is_ok();
        if !animated {
            let _ = finish_window_session(VIDEO_OWNER, stream.session);
            retire_video_frame(stream.frame);
        }
        crate::log_info!(
            target: "ui4";
            "ui4 video-frame stopped reason={} frame={} window={} teardown={} display_release=surflive plane_mutation=none\n",
            reason,
            stream.frame.raw(),
            stream.window.raw(),
            if animated { "direct-plane-shrink+fade" } else { "immediate-fallback" },
        );
        true
    } else if reserved {
        crate::log_info!(
            target: "ui4";
            "ui4 video-frame stopped reason={} frame=none window=none teardown=none-before-first-decoded-frame display_release=none plane_mutation=none\n",
            reason,
        );
        true
    } else {
        false
    }
}

fn create_stream(spec: DecodedVideoFrameSpec) -> Option<VideoStream> {
    let frame_width = super::DEFAULT_FRAME_WIDTH;
    let frame_height = super::DEFAULT_FRAME_HEIGHT;
    let frame = create_video_frame().ok()?;
    let session = match begin_window_session(VIDEO_OWNER) {
        Ok(session) => session,
        Err(_) => {
            let _ = destroy_frame(frame);
            return None;
        }
    };
    let (scanout_width, scanout_height) =
        crate::intel::active_scanout_dimensions().unwrap_or((frame_width, frame_height));
    let placement = WindowPlacement {
        x: (scanout_width.saturating_sub(frame_width) / 2) as i32,
        y: (scanout_height.saturating_sub(frame_height) / 2) as i32,
        width: frame_width,
        height: frame_height,
        z: 100,
        opacity: 0xFF,
        visible: true,
    };
    let window = match create_window(WindowCreate {
        owner: VIDEO_OWNER,
        session,
        frame,
        output: VIDEO_OUTPUT,
        plane: super::WindowPlane::Universal(VIDEO_PLANE_SLOT as u8),
        placement,
        interaction: super::WindowInteraction::APPLICATION_FIXED_FRAME,
    }) {
        Ok(window) => window,
        Err(_) => {
            let _ = finish_window_session(VIDEO_OWNER, session);
            let _ = destroy_frame(frame);
            return None;
        }
    };
    // No placeholder is published. The first visible buffer is always a fully
    // converted, GuC-released RGBA picture; the producer never writes display
    // MMIO and decoder memory is never attached to the broker Frame.
    let visible_width = spec.visible_width;
    let visible_height = spec.visible_height;
    let pan_x = centered_crop_origin(visible_width, frame_width);
    let pan_y = centered_crop_origin(visible_height, frame_height);
    let layout = native_viewport_layout(
        spec.visible_width,
        spec.visible_height,
        frame_width,
        frame_height,
        centered_crop_origin(spec.visible_width, frame_width),
        centered_crop_origin(spec.visible_height, frame_height),
    )?;
    crate::log_info!(
        target: "ui4";
        "ui4 video-frame created owner={:?} frame={} window={} buffers=2 cadence=streaming frame_format=rgba8-premultiplied native_format=media-ytile-nv12 attachment=none source={} source_size={}x{} frame_size={}x{} source_crop={}x{}@{},{} destination={},{} scaling=none-1to1 placement={},{} z={} plane_slot={} direct_import=after-compute-release plane_mutation=none\n",
        VIDEO_OWNER,
        frame.raw(),
        window.raw(),
        "media-ytile-nv12",
        spec.coded_width,
        spec.coded_height,
        frame_width,
        frame_height,
        layout.width,
        layout.height,
        layout.source_x,
        layout.source_y,
        layout.destination_x,
        layout.destination_y,
        placement.x,
        placement.y,
        placement.z,
        VIDEO_PLANE_SLOT,
    );
    Some(VideoStream {
        session,
        frame,
        window,
        source_width: spec.coded_width,
        source_height: spec.coded_height,
        visible_width,
        visible_height,
        frame_width,
        frame_height,
        pan_x,
        pan_y,
        active_pan_source: None,
    })
}

fn create_video_frame() -> Result<FrameHandle, super::FramePoolError> {
    create_frame(FrameSpec {
        output: VIDEO_OUTPUT,
        content: FrameContent::Video,
        cadence: FrameCadence::Streaming,
        buffering: FrameBuffering::Double,
        format: ScanoutFormat::Rgba8888Premultiplied,
        width: super::DEFAULT_FRAME_WIDTH,
        height: super::DEFAULT_FRAME_HEIGHT,
        // The SIMD16 producer overwrites every pixel, including opaque-black
        // letterbox regions, before this allocation can be published.
        base_color: None,
    })
}

const fn centered_crop_origin(source_extent: u32, viewport_extent: u32) -> u32 {
    source_extent.saturating_sub(viewport_extent) / 2
}

/// Map native pixels into an equally-sized destination rectangle. A smaller
/// viewport selects a movable source crop; a larger viewport centers the whole
/// native picture and leaves the surrounding initialized pixels as letterbox.
fn native_viewport_layout(
    source_width: u32,
    source_height: u32,
    destination_width: u32,
    destination_height: u32,
    pan_x: u32,
    pan_y: u32,
) -> Option<NativeViewportLayout> {
    if source_width == 0 || source_height == 0 || destination_width == 0 || destination_height == 0
    {
        return None;
    }
    Some(NativeViewportLayout {
        source_x: pan_x.min(source_width.saturating_sub(destination_width)),
        source_y: pan_y.min(source_height.saturating_sub(destination_height)),
        destination_x: destination_width.saturating_sub(source_width) / 2,
        destination_y: destination_height.saturating_sub(source_height) / 2,
        width: source_width.min(destination_width),
        height: source_height.min(destination_height),
    })
}

fn valid_source(source: DecodedNv12Source) -> bool {
    source.decode_sequence != 0
        && source.gpu != 0
        && source.phys != 0
        && source.virt != 0
        && source.byte_len != 0
        && source.width != 0
        && source.height != 0
        && source.visible_width != 0
        && source.visible_height != 0
        && source.visible_width <= source.width
        && source.visible_height <= source.height
        && source.pitch_bytes >= source.width as usize
        && source.pitch_bytes.is_multiple_of(128)
        && source.uv_offset < source.byte_len
        && source.uv_offset.is_multiple_of(source.pitch_bytes)
}
