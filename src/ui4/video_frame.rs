//! UI4 ownership wrapper for decoded video.
//!
//! The decoder remains a native Tile64 NV12 producer. One GuC dispatch writes
//! the exact leased UI4 RGBA triple-buffer allocation, and the producer release
//! makes that allocation eligible for the ordinary slot-1 direct-import path.
//! The native source remains attached to the same lease until SURFLIVE, so the
//! decoder cannot recycle it while scanout still depends on the converted
//! picture.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use embassy_time::{Duration, Timer};
use spin::Mutex;

use super::{
    DamageRect, FrameCadence, FrameContent, FrameHandle, FrameSpec, OutputId, ScanoutFormat,
    Ui4InputEvent, WindowCreate, WindowId, WindowOwner, WindowPlacement, WindowSessionId,
    acquire_frame_buffer, begin_window_session, cancel_frame_buffer, create_frame, create_window,
    destroy_frame, finish_window_session, gpgpu_rgba_surface, publish_frame_buffer,
    publish_gpgpu_native_video_frame_buffer, publish_window_frame, replace_window_frame,
    take_owner_input_events,
};

// The decoded-video producer owns one ordinary broker window independently of
// the compositor service.
const VIDEO_OWNER: WindowOwner = WindowOwner::VIDEO_PLAYER;
const VIDEO_PLAYBACK_AUTOSTART: bool = true;
const VIDEO_OUTPUT: OutputId = OutputId::from_slot(0).unwrap();
const VIDEO_PLANE_SLOT: usize = super::ALPHA_OVERLAY_PLANE_SLOT;
const VIDEO_INPUT_POLL_MS: u64 = 10;
const VIDEO_PRESENT_ACK_TIMEOUT_MS: u64 = 2_000;

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

/// Dimensions used to bind one decoded picture to its UI4 viewport.  Pixel
/// storage stays decoder-owned; the UI4 frame owns the publication lifetime
/// and binds the native source to one exact leased buffer slot.
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
static VIDEO_NATIVE_ACK: AtomicU64 = AtomicU64::new(0);
static VIDEO_PLAYBACK_PAUSED: AtomicBool = AtomicBool::new(true);
static VIDEO_GUC_PREPARED_CHECKPOINT_LOGGED: AtomicBool = AtomicBool::new(false);
static VIDEO_GUC_ADMITTED_CHECKPOINT_LOGGED: AtomicBool = AtomicBool::new(false);
static VIDEO_GUC_RETIRED_CHECKPOINT_LOGGED: AtomicBool = AtomicBool::new(false);

/// Install the boot player's ordinary UI4 window without decoding a picture.
/// Its initialized black frame gives the user a focusable target while the
/// playback gate remains paused.
pub(crate) fn prepare_decoded_video_player() -> bool {
    VIDEO_PLAYBACK_PAUSED.store(!VIDEO_PLAYBACK_AUTOSTART, Ordering::Release);
    if VIDEO_STREAM.lock().is_some() {
        return true;
    }
    let placeholder_spec = DecodedVideoFrameSpec {
        coded_width: 16,
        coded_height: 9,
        visible_width: 16,
        visible_height: 9,
    };
    let Some(stream) = create_stream(
        placeholder_spec,
        super::DEFAULT_FRAME_WIDTH,
        super::DEFAULT_FRAME_HEIGHT,
        true,
    ) else {
        return false;
    };
    let write = match acquire_frame_buffer(stream.frame) {
        Ok(write) => write,
        Err(_) => {
            cleanup_uninstalled_stream(stream);
            return false;
        }
    };
    if publish_frame_buffer(write).is_err()
        || publish_window_frame(VIDEO_OWNER, stream.window, DamageRect::FULL).is_err()
    {
        let _ = cancel_frame_buffer(write);
        cleanup_uninstalled_stream(stream);
        return false;
    }
    *VIDEO_STREAM.lock() = Some(stream);
    crate::log_info!(
        target: "ui4";
        "ui4 video-player ready owner={:?} frame={} window={} playback={} control=focused-space source=await-first-frame\n",
        VIDEO_OWNER,
        stream.frame.raw(),
        stream.window.raw(),
        if VIDEO_PLAYBACK_AUTOSTART { "playing-autostart" } else { "paused-default" },
    );
    true
}

/// Drain only this consumer's owner queue and wait until focused Space leaves
/// the player in its running state. Returns true when a pause interval occurred
/// so the decoder can reset its frame deadline instead of reporting fake lag.
pub(crate) async fn wait_decoded_video_playback_ready() -> bool {
    let mut waited = VIDEO_PLAYBACK_PAUSED.load(Ordering::Acquire);
    loop {
        poll_decoded_video_player_input();
        if !VIDEO_PLAYBACK_PAUSED.load(Ordering::Acquire) {
            return waited;
        }
        waited = true;
        Timer::after(Duration::from_millis(VIDEO_INPUT_POLL_MS)).await;
    }
}

fn poll_decoded_video_player_input() {
    reap_retired_video_frames();
    for event in take_owner_input_events(VIDEO_OWNER) {
        match event {
            Ui4InputEvent::Keyboard(event) => {
                let window = VIDEO_STREAM.lock().as_ref().map(|stream| stream.window);
                if Some(event.window) != window
                    || event.event.kind != crate::r::keyboard::KEYBOARD_OUTPUT_KIND_KEY
                    || event.event.key_code != crate::r::keyboard::KEYBOARD_KEY_SPACE
                {
                    continue;
                }
                let was_paused = VIDEO_PLAYBACK_PAUSED.fetch_xor(true, Ordering::AcqRel);
                crate::log_info!(
                    target: "ui4";
                    "ui4 video-player playback-toggle owner={:?} window={} state={} trigger=focused-space keyboard={}:{}:{} combo={} virtual={}\n",
                    VIDEO_OWNER,
                    event.window.raw(),
                    if was_paused { "playing" } else { "paused" },
                    event.event.controller_id,
                    event.event.slot_id,
                    event.event.ep_target,
                    event.combo_id,
                    event.virtual_keyboard as u8,
                );
            }
            Ui4InputEvent::Pan(event) => pan_video_viewport(event),
            Ui4InputEvent::Resize(event) => {
                if let Err(reason) = resize_video_viewport(event.window, event.width, event.height)
                {
                    crate::log_warn!(
                        target: "ui4";
                        "ui4 video-player resize rejected window={} extent={}x{} reason={}\n",
                        event.window.raw(),
                        event.width,
                        event.height,
                        reason,
                    );
                }
            }
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

fn resize_video_viewport(window: WindowId, width: u32, height: u32) -> Result<(), &'static str> {
    if width == 0 || height == 0 {
        return Err("empty-extent");
    }
    let previous = VIDEO_STREAM
        .lock()
        .as_ref()
        .copied()
        .filter(|stream| stream.window == window)
        .ok_or("window-not-active")?;
    if previous.frame_width == width && previous.frame_height == height {
        return Ok(());
    }

    let replacement = create_video_frame(width, height).map_err(|_| "frame-create-failed")?;
    let write = acquire_frame_buffer(replacement).map_err(|_| {
        let _ = destroy_frame(replacement);
        "frame-prime-acquire-failed"
    })?;
    if publish_frame_buffer(write).is_err() {
        let _ = cancel_frame_buffer(write);
        let _ = destroy_frame(replacement);
        return Err("frame-prime-publish-failed");
    }
    if replace_window_frame(VIDEO_OWNER, window, replacement).is_err() {
        let _ = destroy_frame(replacement);
        return Err("window-replace-failed");
    }

    let updated = {
        let mut slot = VIDEO_STREAM.lock();
        match slot.as_mut() {
            Some(stream) if stream.window == window && stream.frame == previous.frame => {
                stream.frame = replacement;
                stream.frame_width = width;
                stream.frame_height = height;
                stream.pan_x = centered_crop_origin(stream.visible_width, width);
                stream.pan_y = centered_crop_origin(stream.visible_height, height);
                stream.active_pan_source = None;
                true
            }
            _ => false,
        }
    };
    if !updated {
        let _ = replace_window_frame(VIDEO_OWNER, window, previous.frame);
        let _ = publish_window_frame(VIDEO_OWNER, window, DamageRect::FULL);
        retire_video_frame(replacement);
        return Err("stream-changed");
    }
    let _ = publish_window_frame(VIDEO_OWNER, window, DamageRect::FULL);
    retire_video_frame(previous.frame);

    let stream = VIDEO_STREAM
        .lock()
        .as_ref()
        .copied()
        .ok_or("stream-disappeared")?;
    let layout = native_viewport_layout(
        stream.visible_width,
        stream.visible_height,
        width,
        height,
        stream.pan_x,
        stream.pan_y,
    );
    if let Some(layout) = layout {
        crate::log_info!(
            target: "ui4";
            "ui4 video-player resize applied window={} frame={} viewport={}x{} native={}x{} source_crop={}x{}@{},{} destination={},{} letterbox={} scaling=none-1to1\n",
            window.raw(),
            replacement.raw(),
            width,
            height,
            stream.visible_width,
            stream.visible_height,
            layout.width,
            layout.height,
            layout.source_x,
            layout.source_y,
            layout.destination_x,
            layout.destination_y,
            (width > stream.visible_width || height > stream.visible_height) as u8,
        );
    } else {
        crate::log_info!(
            target: "ui4";
            "ui4 video-player resize applied window={} frame={} viewport={}x{} native=await-first-frame scaling=none-1to1\n",
            window.raw(),
            replacement.raw(),
            width,
            height,
        );
    }
    Ok(())
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

pub(crate) fn acknowledge_native_video_publication(sequence: u64) {
    VIDEO_NATIVE_ACK.fetch_max(sequence, Ordering::AcqRel);
}

/// Convert and publish one native decoder surface, retaining it until the
/// direct-import flip reaches SURFLIVE. This is the same producer-release ->
/// Frame publication -> display-release lifecycle used by the compute previews.
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
    let write = match acquire_frame_buffer(stream.frame) {
        Ok(write) => write,
        Err(error) => {
            crate::log_warn!(target: "ui4";
                "ui4 video-frame acquire failed reason={} error={:?}\n", reason, error,
            );
            return false;
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
            "ui4 video-frame native source rejected frame={} decode_seq={} source_gpu=0x{:X} reason={}\n",
            stream.frame.raw(), source.decode_sequence, source.gpu, reason,
        );
        return false;
    };
    if !VIDEO_GUC_PREPARED_CHECKPOINT_LOGGED.swap(true, Ordering::AcqRel) {
        crate::log_info!(target: "ui4";
            "ui4 video-frame checkpoint stage=guc-prepared decode_seq={} frame={} buffer={} source_gpu=0x{:X} source_phys=0x{:X} source_bytes=0x{:X} target_gpu=0x{:X} target_phys=0x{:X} target_bytes=0x{:X} bindings=2 action=submit\n",
            source.decode_sequence,
            stream.frame.raw(),
            write.buffer_index,
            source.gpu,
            source.phys,
            source.byte_len,
            destination.gpu,
            destination.phys,
            destination.bytes,
        );
    }

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
                // Keep both the decoder picture and exact write lease pinned;
                // no frame has been dropped or made visible yet.
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
    if !VIDEO_GUC_ADMITTED_CHECKPOINT_LOGGED.swap(true, Ordering::AcqRel) {
        crate::log_info!(target: "ui4";
            "ui4 video-frame checkpoint stage=guc-admitted decode_seq={} frame={} buffer={} source_gpu=0x{:X} target_gpu=0x{:X} action=wait-retirement\n",
            source.decode_sequence,
            stream.frame.raw(),
            write.buffer_index,
            source.gpu,
            destination.gpu,
        );
    }
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
                // An accepted GPU job has no cancellation proof. Keep this
                // future alive so both the decoder picture and write lease are
                // quarantined; returning would permit unsafe source reuse.
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
    if !VIDEO_GUC_RETIRED_CHECKPOINT_LOGGED.swap(true, Ordering::AcqRel) {
        crate::log_info!(target: "ui4";
            "ui4 video-frame checkpoint stage=guc-retired decode_seq={} frame={} buffer={} release={} submit_ms={} action=publish-exact-lease\n",
            source.decode_sequence,
            stream.frame.raw(),
            write.buffer_index,
            release.sequence(),
            submit_ms,
        );
    }
    let sequence = VIDEO_PUBLISH_SEQ.fetch_add(1, Ordering::AcqRel) + 1;
    let native = super::NativeVideoFrameView {
        source: super::NativeNv12Surface {
            phys: source.phys,
            gpu: source.gpu,
            virt: source.virt,
            byte_len: source.byte_len,
            width: source.width,
            height: source.height,
            pitch_bytes,
            uv_offset: source.uv_offset,
            layout: super::Nv12Layout::YTiled,
            pipeline_slot: VIDEO_OUTPUT.slot(),
        },
        source_x: layout.source_x,
        source_y: layout.source_y,
        destination_x: layout.destination_x,
        destination_y: layout.destination_y,
        width: layout.width,
        height: layout.height,
        decode_sequence: source.decode_sequence,
        presentation_sequence: sequence,
    };
    let published = match publish_gpgpu_native_video_frame_buffer(write, native, release) {
        Ok(published) => published,
        Err(error) => {
            let _ = cancel_frame_buffer(write);
            crate::log_warn!(target: "ui4";
                "ui4 video-frame GPU/native publish failed frame={} buffer={} error={:?} reason={}\n",
                stream.frame.raw(), write.buffer_index, error, reason,
            );
            return false;
        }
    };
    let window_serial = match publish_window_frame(VIDEO_OWNER, stream.window, DamageRect::FULL) {
        Ok(serial) => serial,
        Err(error) => {
            crate::log_warn!(target: "ui4";
                "ui4 video-frame window publish failed frame={} window={} error={:?} reason={} action=close-stream-before-source-reuse\n",
                stream.frame.raw(), stream.window.raw(), error, reason,
            );
            let _ = stop_decoded_nv12_stream("window-publish-failed");
            return false;
        }
    };
    if sequence <= 8 || sequence.is_multiple_of(120) {
        crate::log_info!(target: "ui4";
            "ui4 video-frame published seq={} decode_seq={} frame={} window={} buffer={} frame_serial={} window_serial={} producer=guc-nv12-to-ui4-rgba8-frame producer_release={} submit_ms={} source=tile64-nv12 {}x{} visible={}x{} crop={}x{}@{},{} destination={},{} source_gpu=0x{:X} target_gpu=0x{:X} plane_route=slot1-direct-rgba8-after-guc linked_nv12_slots=0 producer_plane_mmio=0 cpu_pixel_copy=0\n",
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

    let deadline =
        embassy_time::Instant::now() + Duration::from_millis(VIDEO_PRESENT_ACK_TIMEOUT_MS);
    let mut overdue_logged = false;
    while VIDEO_NATIVE_ACK.load(Ordering::Acquire) < sequence {
        if !overdue_logged && embassy_time::Instant::now() >= deadline {
            overdue_logged = true;
            crate::log_warn!(target: "ui4";
                "ui4 video-frame present overdue seq={} frame={} window={} reason={} action=retain-native-source-and-wait-for-surflive\n",
                sequence, stream.frame.raw(), stream.window.raw(), reason,
            );
        }
        Timer::after(Duration::from_millis(1)).await;
    }
    true
}

fn bind_decoded_source_stream(spec: DecodedVideoFrameSpec, reason: &str) -> Option<VideoStream> {
    if !spec.valid() {
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
                    "ui4 video-player source-bound frame={} window={} source={}x{} visible={}x{} viewport={}x{} source_crop={}x{}@{},{} destination={},{} scaling=none-1to1 playback={} producer=guc-nv12-to-ui4-rgba8-frame attachment=exact-frame-buffer plane_slot={} reason={}\n",
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
                    if VIDEO_PLAYBACK_PAUSED.load(Ordering::Acquire) { "paused" } else { "playing" },
                    VIDEO_PLANE_SLOT,
                    reason,
                );
            }
            return Some(*stream);
        }
    }
    let stream =
        create_stream(spec, super::DEFAULT_FRAME_WIDTH, super::DEFAULT_FRAME_HEIGHT, false)?;
    *VIDEO_STREAM.lock() = Some(stream);
    Some(stream)
}

pub(crate) fn stop_decoded_nv12_stream(reason: &str) -> bool {
    let stream = VIDEO_STREAM.lock().take();
    if let Some(stream) = stream {
        let _ = finish_window_session(VIDEO_OWNER, stream.session);
        retire_video_frame(stream.frame);
        crate::log_info!(
            target: "ui4";
            "ui4 video-frame stopped reason={} frame={} window={} plane_mutation=none\n",
            reason,
            stream.frame.raw(),
            stream.window.raw(),
        );
        true
    } else {
        false
    }
}

fn create_stream(
    spec: DecodedVideoFrameSpec,
    frame_width: u32,
    frame_height: u32,
    placeholder: bool,
) -> Option<VideoStream> {
    let frame = create_video_frame(frame_width, frame_height).ok()?;
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
        interaction: super::WindowInteraction::APPLICATION,
    }) {
        Ok(window) => window,
        Err(_) => {
            let _ = finish_window_session(VIDEO_OWNER, session);
            let _ = destroy_frame(frame);
            return None;
        }
    };
    // This is an ordinary broker-owned RGBA window. The producer never writes
    // display MMIO: slot 1 imports only its completed RGBA allocation, while
    // slots 2 and 3 remain owned by Gridpaper and Draw3D.
    let visible_width = if placeholder { 0 } else { spec.visible_width };
    let visible_height = if placeholder { 0 } else { spec.visible_height };
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
        "ui4 video-frame created owner={:?} frame={} window={} buffers=3 cadence=streaming frame_format=rgba8-premultiplied native_format=tile64-nv12 attachment=exact-frame-buffer source={} source_size={}x{} frame_size={}x{} source_crop={}x{}@{},{} destination={},{} scaling=none-1to1 placement={},{} z={} plane_slot={} direct_import=after-compute-release plane_mutation=none\n",
        VIDEO_OWNER,
        frame.raw(),
        window.raw(),
        if placeholder { "await-first-frame" } else { "tile64-nv12" },
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
        source_width: if placeholder { 0 } else { spec.coded_width },
        source_height: if placeholder { 0 } else { spec.coded_height },
        visible_width,
        visible_height,
        frame_width,
        frame_height,
        pan_x,
        pan_y,
        active_pan_source: None,
    })
}

fn create_video_frame(width: u32, height: u32) -> Result<FrameHandle, super::FramePoolError> {
    create_frame(FrameSpec {
        output: VIDEO_OUTPUT,
        content: FrameContent::Video,
        cadence: FrameCadence::Streaming,
        format: ScanoutFormat::Rgba8888Premultiplied,
        width,
        height,
        // The GuC producer overwrites the complete allocation: decoded pixels
        // fill the viewport and opaque black fills any letterbox region.
        base_color: Some(super::PremultipliedRgba8::from_straight_rgba(0, 0, 0, u8::MAX)),
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
