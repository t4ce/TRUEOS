//! UI4 ownership wrapper for decoded video.
//!
//! The decoder remains a native Y-tiled NV12 producer. This boundary converts
//! each decoded picture into one of three ordinary premultiplied-RGBA UI4
//! buffers, so video participates in the same broker ordering and primary
//! composition as every other window. The proven linked-NV12 presenter remains
//! below this function as the caller's fallback when conversion cannot run.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use embassy_time::{Duration, Timer};
use spin::Mutex;

use super::{
    DamageRect, FrameCadence, FrameContent, FrameHandle, FrameSpec, OutputId, ScanoutFormat,
    Ui4InputEvent, WindowCreate, WindowId, WindowOwner, WindowPlacement, WindowSessionId,
    acquire_frame_buffer, begin_window_session, cancel_frame_buffer, create_frame, create_window,
    destroy_frame, finish_window_session, publish_frame_buffer, publish_window_frame,
    replace_window_frame, take_owner_input_events, writable_rgba_view,
};

// The decoded-video producer owns one ordinary broker window independently of
// the compositor service.
const VIDEO_OWNER: WindowOwner = WindowOwner::KernelApp(2);
const VIDEO_OUTPUT: OutputId = OutputId::from_slot(0).unwrap();
const VIDEO_INPUT_POLL_MS: u64 = 10;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct DecodedNv12Source {
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

/// Dimensions available before a decode submission.  The VD-to-SFC path will
/// reserve a UI4 buffer from this description, bind it into the media PPGTT,
/// and retain its write lease until the complete VDBOX+SFC job retires.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct DecodedVideoFrameSpec {
    pub(crate) coded_width: u32,
    pub(crate) coded_height: u32,
    pub(crate) visible_width: u32,
    pub(crate) visible_height: u32,
    pub(crate) progressive: bool,
}

impl DecodedVideoFrameSpec {
    const fn from_nv12_source(source: DecodedNv12Source) -> Self {
        Self {
            coded_width: source.width,
            coded_height: source.height,
            visible_width: source.visible_width,
            visible_height: source.visible_height,
            // The current live AVC milestone rejects field and MBAFF input.
            progressive: true,
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

/// One unpublished UI4 video buffer with its GPU mapping and ownership token.
/// This value is intentionally not `Copy`: exactly one commit or cancel must
/// consume the target.
pub(crate) struct DecodedRgbaWriteTarget {
    stream: VideoStream,
    write: super::FrameWriteLease,
    surface: super::FrameRgbaView,
}

impl DecodedRgbaWriteTarget {
    pub(crate) const fn frame(&self) -> FrameHandle {
        self.stream.frame
    }

    pub(crate) const fn buffer_index(&self) -> u8 {
        self.write.buffer_index
    }

    pub(crate) const fn rgba(&self) -> super::FrameRgbaView {
        self.surface
    }

    pub(crate) const fn sfc_output_surface(
        &self,
    ) -> crate::intel::xelp_media_sfc::SfcRgbaOutputSurface {
        crate::intel::xelp_media_sfc::SfcRgbaOutputSurface {
            gpu_addr: self.surface.gpu,
            phys_addr: self.surface.phys,
            byte_len: self.surface.byte_len,
            width: self.surface.width,
            height: self.surface.height,
            pitch_bytes: self.surface.pitch,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum DecodedRgbaProducer {
    CpuNv12Converter,
    VdboxSfc,
}

impl DecodedRgbaProducer {
    const fn label(self) -> &'static str {
        match self {
            Self::CpuNv12Converter => "cpu-nv12-converter",
            Self::VdboxSfc => "vdbox-sfc",
        }
    }
}

static VIDEO_STREAM: Mutex<Option<VideoStream>> = Mutex::new(None);
static VIDEO_RETIRED_FRAMES: Mutex<Vec<FrameHandle>> = Mutex::new(Vec::new());
static VIDEO_PUBLISH_SEQ: AtomicU64 = AtomicU64::new(0);
static SFC_TARGET_READY_LOGGED: AtomicBool = AtomicBool::new(false);
static SFC_TARGET_UNAVAILABLE_LOGS: AtomicU64 = AtomicU64::new(0);
static VIDEO_PLAYBACK_PAUSED: AtomicBool = AtomicBool::new(true);

/// Install the boot player's ordinary UI4 window without decoding a picture.
/// Its initialized black frame gives the user a focusable target while the
/// playback gate remains paused.
pub(crate) fn prepare_decoded_video_player() -> bool {
    VIDEO_PLAYBACK_PAUSED.store(true, Ordering::Release);
    if VIDEO_STREAM.lock().is_some() {
        return true;
    }
    let placeholder_spec = DecodedVideoFrameSpec {
        coded_width: 16,
        coded_height: 9,
        visible_width: 16,
        visible_height: 9,
        progressive: true,
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
        "ui4 video-player ready owner={:?} frame={} window={} playback=paused-default control=focused-space source=await-first-frame\n",
        VIDEO_OWNER,
        stream.frame.raw(),
        stream.window.raw(),
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

pub(crate) fn present_decoded_nv12_stream_frame(source: DecodedNv12Source, reason: &str) -> bool {
    if !valid_source(source) {
        return false;
    }
    let target = match acquire_decoded_rgba_stream_target(
        DecodedVideoFrameSpec::from_nv12_source(source),
        reason,
    ) {
        Some(target) => target,
        None => return false,
    };
    let destination = target.rgba();
    if !convert_decoded_ytile_nv12_to_rgba(
        source,
        destination,
        target.stream.pan_x,
        target.stream.pan_y,
    ) {
        cancel_decoded_rgba_stream_target(target, "cpu-conversion-failed");
        return false;
    }
    publish_decoded_rgba_stream_target(
        target,
        source,
        DecodedRgbaProducer::CpuNv12Converter,
        reason,
    )
}

pub(crate) fn acquire_decoded_rgba_stream_target(
    spec: DecodedVideoFrameSpec,
    reason: &str,
) -> Option<DecodedRgbaWriteTarget> {
    if !spec.valid() {
        return None;
    }
    // The frame extent follows the broker window exactly. The decoded picture
    // is copied into that viewport at 1:1 native resolution, either cropped or
    // centered with untouched opaque-black letterbox pixels.
    let current = *VIDEO_STREAM.lock();
    if current.is_some() {
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
                crate::log_info!(
                    target: "ui4";
                    "ui4 video-player source-bound frame={} window={} source={}x{} visible={}x{} viewport={}x{} source_crop={}x{}@{},{} destination={},{} scaling=none-1to1 playback={}\n",
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
                );
            }
        }
    }

    // Do not match directly on the lock expression: the scrutinee temporary
    // otherwise lives through the selected arm, and the `None` arm deadlocks
    // when it installs the newly-created stream by taking this mutex again.
    let existing = { *VIDEO_STREAM.lock() };
    let stream = match existing {
        Some(stream) => stream,
        None => match create_stream(
            spec,
            super::DEFAULT_FRAME_WIDTH,
            super::DEFAULT_FRAME_HEIGHT,
            false,
        ) {
            Some(stream) => {
                *VIDEO_STREAM.lock() = Some(stream);
                stream
            }
            None => return None,
        },
    };

    let write = match acquire_frame_buffer(stream.frame) {
        Ok(write) => write,
        Err(error) => {
            crate::log_warn!(
                target: "ui4";
                "ui4 video-frame acquire failed reason={} error={:?}\n",
                reason,
                error
            );
            return None;
        }
    };
    let destination = match writable_rgba_view(write) {
        Ok(surface) => surface,
        Err(error) => {
            let _ = cancel_frame_buffer(write);
            crate::log_warn!(
                target: "ui4";
                "ui4 video-frame view failed reason={} error={:?}\n",
                reason,
                error
            );
            return None;
        }
    };

    let target = DecodedRgbaWriteTarget {
        stream,
        write,
        surface: destination,
    };
    let sfc_input = crate::intel::xelp_media_sfc::SfcAvcInputFrame {
        coded_width: spec.coded_width,
        coded_height: spec.coded_height,
        visible_width: spec.visible_width,
        visible_height: spec.visible_height,
        progressive: spec.progressive,
    };
    match crate::intel::xelp_media_sfc::plan_avc_ui4_visible_output(
        sfc_input,
        target.sfc_output_surface(),
    ) {
        Ok(plan) => {
            if !SFC_TARGET_READY_LOGGED.swap(true, Ordering::AcqRel) {
                crate::log_info!(
                        target: "ui4";
                    "ui4 video-frame sfc-target planned frame={} buffer={} gpu=0x{:X} phys=0x{:X} bytes=0x{:X} pitch=0x{:X} commands={} scratch=0x{:X} scaling={} avs_coefficients={} mode=shadow-disabled reason={}\n",
                        target.frame().raw(),
                        target.buffer_index(),
                        plan.output.gpu_addr,
                        plan.output.phys_addr,
                        plan.output.byte_len,
                        plan.output.pitch_bytes,
                    plan.command_dwords,
                    plan.scratch.page_aligned_total_bytes,
                    plan.scaling_enabled as u8,
                    plan.avs_coefficients_required as u8,
                    reason,
                );
            }
        }
        Err(error) => {
            let count = SFC_TARGET_UNAVAILABLE_LOGS.fetch_add(1, Ordering::Relaxed) + 1;
            if count <= 4 || count.is_power_of_two() {
                crate::log_warn!(
                    target: "ui4";
                    "ui4 video-frame sfc-target unavailable count={} frame={} buffer={} error={:?} reason={} cpu_fallback=1 log_policy=first4-powers-of-two\n",
                    count,
                    target.frame().raw(),
                    target.buffer_index(),
                    error,
                    reason,
                );
            }
        }
    }
    Some(target)
}

pub(crate) fn publish_decoded_rgba_stream_target(
    target: DecodedRgbaWriteTarget,
    source: DecodedNv12Source,
    producer: DecodedRgbaProducer,
    reason: &str,
) -> bool {
    let destination = target.surface;
    let stream = target.stream;
    let write = target.write;
    let published = match publish_frame_buffer(write) {
        Ok(published) => published,
        Err(error) => {
            let _ = cancel_frame_buffer(write);
            crate::log_warn!(
                target: "ui4";
                "ui4 video-frame publish failed frame={} buffer={} producer={} error={:?} reason={}\n",
                stream.frame.raw(),
                write.buffer_index,
                producer.label(),
                error,
                reason,
            );
            return false;
        }
    };
    let window_serial = match publish_window_frame(VIDEO_OWNER, stream.window, DamageRect::FULL) {
        Ok(serial) => serial,
        Err(_) => return false,
    };

    let seq = VIDEO_PUBLISH_SEQ.fetch_add(1, Ordering::AcqRel) + 1;
    if seq <= 8 || seq.is_multiple_of(120) {
        crate::log_info!(
            target: "ui4";
            "ui4 video-frame published seq={} frame={} window={} buffer={} frame_serial={} window_serial={} producer={} source=ytile-nv12 {}x{} visible={}x{} output=rgba8-premultiplied {}x{} output_gpu=0x{:X} source_gpu=0x{:X}\n",
            seq,
            stream.frame.raw(),
            stream.window.raw(),
            published.buffer_index,
            published.publish_serial,
            window_serial,
            producer.label(),
            source.width,
            source.height,
            source.visible_width,
            source.visible_height,
            destination.width,
            destination.height,
            destination.gpu,
            source.gpu,
        );
    }
    true
}

pub(crate) fn cancel_decoded_rgba_stream_target(target: DecodedRgbaWriteTarget, reason: &str) {
    if let Err(error) = cancel_frame_buffer(target.write) {
        crate::log_warn!(
            target: "ui4";
            "ui4 video-frame cancel failed frame={} buffer={} error={:?} reason={}\n",
            target.frame().raw(),
            target.buffer_index(),
            error,
            reason,
        );
    }
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
    }) {
        Ok(window) => window,
        Err(_) => {
            let _ = finish_window_session(VIDEO_OWNER, session);
            let _ = destroy_frame(frame);
            return None;
        }
    };
    // This is an ordinary broker-owned RGBA window. It must never reprogram
    // the retired linked-NV12 slots: slot 2 belongs to Solara and slot 3 to
    // Draw3D. Any legacy direct-NV12 fallback owns its own explicit lifetime.
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
        "ui4 video-frame created owner={:?} frame={} window={} buffers=3 cadence=streaming format=rgba8-premultiplied source={} source_size={}x{} frame_size={}x{} source_crop={}x{}@{},{} destination={},{} scaling=none-1to1 placement={},{} z={} plane_mutation=none\n",
        VIDEO_OWNER,
        frame.raw(),
        window.raw(),
        if placeholder { "await-first-frame" } else { "ytile-nv12" },
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
        // Initialize all three backing buffers once so native content smaller
        // than the viewport receives stable opaque-black letterbox pixels.
        base_color: Some(super::PremultipliedRgba8::from_straight_rgba(0, 0, 0, u8::MAX)),
    })
}

fn convert_decoded_ytile_nv12_to_rgba(
    source: DecodedNv12Source,
    destination: super::FrameRgbaView,
    pan_x: u32,
    pan_y: u32,
) -> bool {
    let destination_width = destination.width as usize;
    let destination_height = destination.height as usize;
    let pitch = destination.pitch as usize;
    if pitch < destination_width.saturating_mul(4)
        || destination.byte_len < pitch.saturating_mul(destination_height)
    {
        return false;
    }
    let layout = match native_viewport_layout(
        source.visible_width,
        source.visible_height,
        destination.width,
        destination.height,
        pan_x,
        pan_y,
    ) {
        Some(layout) => layout,
        None => return false,
    };
    let copy_width = layout.width as usize;
    let copy_height = layout.height as usize;
    let source_origin_x = layout.source_x as usize;
    let source_origin_y = layout.source_y as usize;
    let destination_origin_x = layout.destination_x as usize;
    let destination_origin_y = layout.destination_y as usize;
    let tiles_per_row = source.pitch_bytes / 128;
    let chroma_row = source.uv_offset / source.pitch_bytes;
    let total_rows = match chroma_row.checked_add(source.height.div_ceil(2) as usize) {
        Some(rows) => rows,
        None => return false,
    };
    let required_source = match total_rows
        .div_ceil(32)
        .checked_mul(tiles_per_row)
        .and_then(|tiles| tiles.checked_mul(4096))
    {
        Some(bytes) => bytes,
        None => return false,
    };
    if tiles_per_row == 0 || source.byte_len < required_source {
        return false;
    }

    for output_y in 0..copy_height {
        let source_y = source_origin_y + output_y;
        let destination_y = destination_origin_y + output_y;
        let destination_row = unsafe { destination.virt.add(destination_y * pitch) };
        for output_x in 0..copy_width {
            let source_x = source_origin_x + output_x;
            let y_offset = ytile_8bpp_offset(source_x, source_y, tiles_per_row);
            let uv_x = (source_x / 2) * 2;
            let uv_offset = ytile_8bpp_offset(uv_x, chroma_row + source_y / 2, tiles_per_row);
            if y_offset >= source.byte_len || uv_offset.saturating_add(1) >= source.byte_len {
                return false;
            }
            let luma =
                unsafe { core::ptr::read_volatile((source.virt as *const u8).add(y_offset)) };
            let u = unsafe { core::ptr::read_volatile((source.virt as *const u8).add(uv_offset)) };
            let v =
                unsafe { core::ptr::read_volatile((source.virt as *const u8).add(uv_offset + 1)) };
            let [r, g, b] = nv12_to_rgb(luma, u, v);
            let pixel = u32::from_le_bytes([r, g, b, u8::MAX]);
            unsafe {
                core::ptr::write_volatile(
                    destination_row
                        .add((destination_origin_x + output_x) * 4)
                        .cast::<u32>(),
                    pixel,
                )
            };
        }
    }
    crate::intel::dma_flush(destination.virt, destination.byte_len);
    true
}

#[derive(Copy, Clone)]
struct NativeViewportLayout {
    source_x: u32,
    source_y: u32,
    destination_x: u32,
    destination_y: u32,
    width: u32,
    height: u32,
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

#[inline(always)]
fn ytile_8bpp_offset(byte_x: usize, row_y: usize, tiles_per_row: usize) -> usize {
    let tile_col = byte_x / 128;
    let tile_row = row_y / 32;
    let in_x = byte_x % 128;
    let in_y = row_y % 32;
    let within_tile = (in_x / 16) * 512 + in_y * 16 + in_x % 16;
    (tile_row * tiles_per_row + tile_col) * 4096 + within_tile
}

#[inline(always)]
fn nv12_to_rgb(y: u8, u: u8, v: u8) -> [u8; 3] {
    let c = (i32::from(y) - 16).max(0);
    let d = i32::from(u) - 128;
    let e = i32::from(v) - 128;
    let clamp = |value: i32| ((value + 128) >> 8).clamp(0, 255) as u8;
    [
        clamp(298 * c + 409 * e),
        clamp(298 * c - 100 * d - 208 * e),
        clamp(298 * c + 516 * d),
    ]
}

fn valid_source(source: DecodedNv12Source) -> bool {
    source.gpu != 0
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
