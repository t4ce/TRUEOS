//! UI4 ownership wrapper for decoded video.
//!
//! The decoder remains a native Tile64 NV12 producer. Each retired source is
//! attached to one exact UI4 triple-buffer lease. The compositor then performs
//! the proven full-primary GuC conversion and retains the native source until
//! the resulting primary flip reaches SURFLIVE.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use embassy_time::{Duration, Timer};
use spin::Mutex;

use super::{
    DamageRect, FrameCadence, FrameContent, FrameHandle, FrameSpec, OutputId, ScanoutFormat,
    Ui4InputEvent, WindowCreate, WindowId, WindowOwner, WindowPlacement, WindowSessionId,
    acquire_frame_buffer, begin_window_session, cancel_frame_buffer, create_frame, create_window,
    destroy_frame, finish_window_session, publish_native_video_frame_buffer, publish_window_frame,
    take_owner_input_events,
};

// The decoded-video producer owns one ordinary broker window independently of
// the compositor service.
const VIDEO_OWNER: WindowOwner = WindowOwner::VIDEO_PLAYER;
const VIDEO_PLAYBACK_AUTOSTART: bool = true;
const VIDEO_OUTPUT: OutputId = OutputId::from_slot(0).unwrap();
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

/// Register the decoded-video ownership records without presenting pixels.
/// The broker window remains pending until the first decoder-retired NV12
/// attachment becomes the first visible publication.
pub(crate) fn prepare_decoded_video_player() -> bool {
    install_decoded_video_player(true, !VIDEO_PLAYBACK_AUTOSTART, "kernel-boot-video-task")
}

/// Begin the fixed `vid` command lifetime. Unlike the boot preparation helper,
/// this refuses to borrow an existing stream: exactly one Embassy playback
/// task owns the session it will later close.
pub(crate) fn begin_shell_decoded_video_player() -> bool {
    install_decoded_video_player(false, false, "shell2-vid-task")
}

fn install_decoded_video_player(
    allow_existing: bool,
    initially_paused: bool,
    lifecycle_owner: &str,
) -> bool {
    if VIDEO_STREAM.lock().is_some() {
        return allow_existing;
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
    let mut slot = VIDEO_STREAM.lock();
    if slot.is_some() {
        drop(slot);
        cleanup_uninstalled_stream(stream);
        return allow_existing;
    }
    *slot = Some(stream);
    drop(slot);
    VIDEO_PLAYBACK_PAUSED.store(initially_paused, Ordering::Release);
    crate::log_info!(
        target: "ui4";
        "ui4 video-player ready owner={:?} frame={} window={} playback={} control=focused-space source=await-first-decoded-frame lifecycle_owner={} broker_state=pending placeholder_present=0\n",
        VIDEO_OWNER,
        stream.frame.raw(),
        stream.window.raw(),
        if initially_paused { "paused-default" } else { "playing" },
        lifecycle_owner,
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

pub(crate) fn acknowledge_native_video_publication(sequence: u64) {
    VIDEO_NATIVE_ACK.fetch_max(sequence, Ordering::AcqRel);
}

/// Publish one native decoder surface and retain it until the compositor has
/// completed both its GuC conversion and the primary SURFLIVE transition.
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
    let pitch_bytes = match u32::try_from(source.pitch_bytes) {
        Ok(pitch) => pitch,
        Err(_) => {
            let _ = cancel_frame_buffer(write);
            return false;
        }
    };
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
    let published = match publish_native_video_frame_buffer(write, native) {
        Ok(published) => published,
        Err(error) => {
            let _ = cancel_frame_buffer(write);
            crate::log_warn!(target: "ui4";
                "ui4 video-frame native publish failed frame={} buffer={} error={:?} reason={}\n",
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
            "ui4 video-frame published seq={} decode_seq={} frame={} window={} buffer={} frame_serial={} window_serial={} producer=guc-native-nv12-frame-primary source=tile64-nv12 {}x{} visible={}x{} crop={}x{}@{},{} destination={},{} source_gpu=0x{:X} plane_route=primary-xrgb-after-guc linked_nv12_slots=0 producer_plane_mmio=0 cpu_pixel_copy=0\n",
            sequence,
            source.decode_sequence,
            stream.frame.raw(),
            stream.window.raw(),
            published.buffer_index,
            published.publish_serial,
            window_serial,
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
                    "ui4 video-player source-bound frame={} window={} source={}x{} visible={}x{} viewport={}x{} source_crop={}x{}@{},{} destination={},{} scaling=none-1to1 playback={} producer=guc-native-nv12-frame-primary attachment=per-frame-buffer reason={}\n",
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
        plane: super::WindowPlane::Primary,
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
    // The RGBA ring carries broker ownership and publication serials only. No
    // carrier buffer is published by itself: each visible publication carries
    // the exact decoder-retired NV12 attachment. The producer never writes
    // display MMIO.
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
        "ui4 video-frame created owner={:?} frame={} window={} buffers=3 cadence=streaming carrier_format=rgba8-premultiplied native_format=tile64-nv12 attachment=per-frame-buffer source={} source_size={}x{} frame_size={}x{} source_crop={}x{}@{},{} destination={},{} scaling=none-1to1 placement={},{} z={} plane_route=primary-guc-conversion plane_mutation=none\n",
        VIDEO_OWNER,
        frame.raw(),
        window.raw(),
        if placeholder { "await-first-decoded-frame" } else { "tile64-nv12" },
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
        // The RGBA allocation remains an initialized ownership/serial carrier;
        // the compositor consumes the attached native surface directly.
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
