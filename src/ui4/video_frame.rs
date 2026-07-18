//! UI4 ownership wrapper for decoded video.
//!
//! The decoder remains a native Y-tiled NV12 producer. This boundary converts
//! each decoded picture into one of three ordinary premultiplied-RGBA UI4
//! buffers, so video participates in the same broker ordering and primary
//! composition as every other window. The proven linked-NV12 presenter remains
//! below this function as the caller's fallback when conversion cannot run.

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use embassy_time::{Duration, Timer};
use spin::Mutex;

use super::{
    DamageRect, FrameCadence, FrameContent, FrameHandle, FrameSpec, OutputId, ScanoutFormat,
    Ui4InputEvent, WindowCreate, WindowId, WindowOwner, WindowPlacement, WindowSessionId,
    acquire_frame_buffer, begin_window_session, cancel_frame_buffer, create_frame, create_window,
    destroy_frame, finish_window_session, publish_frame_buffer, publish_window_frame,
    take_owner_input_events, writable_rgba_view,
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
    frame_width: u32,
    frame_height: u32,
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
static VIDEO_PUBLISH_SEQ: AtomicU64 = AtomicU64::new(0);
static SFC_TARGET_READY_LOGGED: AtomicBool = AtomicBool::new(false);
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
    let window = VIDEO_STREAM.lock().as_ref().map(|stream| stream.window);
    for event in take_owner_input_events(VIDEO_OWNER) {
        let Ui4InputEvent::Keyboard(event) = event else {
            continue;
        };
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
}

fn cleanup_uninstalled_stream(stream: VideoStream) {
    let _ = finish_window_session(VIDEO_OWNER, stream.session);
    let _ = destroy_frame(stream.frame);
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
    if !convert_decoded_ytile_nv12_to_rgba(source, destination) {
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
    // Use UI4's default broker extent. The CPU fallback performs aspect-fit
    // scaling directly while converting NV12, without first materializing a
    // native-size RGBA frame.
    let frame_width = super::DEFAULT_FRAME_WIDTH;
    let frame_height = super::DEFAULT_FRAME_HEIGHT;

    let current = *VIDEO_STREAM.lock();
    if current.is_some_and(|stream| {
        stream.source_width != 0
            && (stream.source_width != spec.coded_width
                || stream.source_height != spec.coded_height
                || stream.frame_width != frame_width
                || stream.frame_height != frame_height)
    }) {
        let _ = stop_decoded_nv12_stream("ui4-video-format-change");
    } else if current.is_some_and(|stream| stream.source_width == 0) {
        let mut slot = VIDEO_STREAM.lock();
        if let Some(stream) = slot.as_mut() {
            stream.source_width = spec.coded_width;
            stream.source_height = spec.coded_height;
            crate::log_info!(
                target: "ui4";
                "ui4 video-player source-bound frame={} window={} source={}x{} visible={}x{} playback={}\n",
                stream.frame.raw(),
                stream.window.raw(),
                spec.coded_width,
                spec.coded_height,
                spec.visible_width,
                spec.visible_height,
                if VIDEO_PLAYBACK_PAUSED.load(Ordering::Acquire) { "paused" } else { "playing" },
            );
        }
    }

    // Do not match directly on the lock expression: the scrutinee temporary
    // otherwise lives through the selected arm, and the `None` arm deadlocks
    // when it installs the newly-created stream by taking this mutex again.
    let existing = { *VIDEO_STREAM.lock() };
    let stream = match existing {
        Some(stream) => stream,
        None => match create_stream(spec, frame_width, frame_height, false) {
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
            crate::log_warn!(
                target: "ui4";
                "ui4 video-frame sfc-target unavailable frame={} buffer={} error={:?} reason={} cpu_fallback=1\n",
                target.frame().raw(),
                target.buffer_index(),
                error,
                reason,
            );
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
        let _ = destroy_frame(stream.frame);
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
    let frame = create_frame(FrameSpec {
        output: VIDEO_OUTPUT,
        content: FrameContent::Video,
        cadence: FrameCadence::Streaming,
        format: ScanoutFormat::Rgba8888Premultiplied,
        width: frame_width,
        height: frame_height,
        // The 16:9 movie is aspect-fitted inside the 3:2 demo frame. Initialize
        // all three backing buffers once so the untouched letterbox area stays
        // opaque black without a per-picture clear.
        base_color: Some(super::PremultipliedRgba8::from_straight_rgba(0, 0, 0, u8::MAX)),
    })
    .ok()?;
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
    // This is an ordinary broker-owned RGBA window.  It must never reprogram
    // the retired linked-NV12 slots: slot 2 belongs to Solara and slot 3 to
    // Draw3D.  Any legacy direct-NV12 fallback owns its own explicit lifetime.
    let fitted =
        aspect_fit_rect(spec.visible_width, spec.visible_height, frame_width, frame_height)?;
    crate::log_info!(
        target: "ui4";
        "ui4 video-frame created owner={:?} frame={} window={} buffers=3 cadence=streaming format=rgba8-premultiplied source={} source_size={}x{} frame_size={}x{} fitted_content={}x{}@{},{} scaling=fused-nearest placement={},{} z={} plane_mutation=none\n",
        VIDEO_OWNER,
        frame.raw(),
        window.raw(),
        if placeholder { "await-first-frame" } else { "ytile-nv12" },
        spec.coded_width,
        spec.coded_height,
        frame_width,
        frame_height,
        fitted.width,
        fitted.height,
        fitted.x,
        fitted.y,
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
        frame_width,
        frame_height,
    })
}

fn convert_decoded_ytile_nv12_to_rgba(
    source: DecodedNv12Source,
    destination: super::FrameRgbaView,
) -> bool {
    let source_width = source.visible_width as usize;
    let source_height = source.visible_height as usize;
    let destination_width = destination.width as usize;
    let destination_height = destination.height as usize;
    let pitch = destination.pitch as usize;
    if pitch < destination_width.saturating_mul(4)
        || destination.byte_len < pitch.saturating_mul(destination_height)
    {
        return false;
    }
    let fitted = match aspect_fit_rect(
        source.visible_width,
        source.visible_height,
        destination.width,
        destination.height,
    ) {
        Some(fitted) => fitted,
        None => return false,
    };
    let fitted_width = fitted.width as usize;
    let fitted_height = fitted.height as usize;
    let fitted_x = fitted.x as usize;
    let fitted_y = fitted.y as usize;
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

    for output_y in 0..fitted_height {
        let source_y = output_y.saturating_mul(source_height) / fitted_height;
        let destination_y = fitted_y + output_y;
        let destination_row = unsafe { destination.virt.add(destination_y * pitch) };
        for output_x in 0..fitted_width {
            let source_x = output_x.saturating_mul(source_width) / fitted_width;
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
                    destination_row.add((fitted_x + output_x) * 4).cast::<u32>(),
                    pixel,
                )
            };
        }
    }
    crate::intel::dma_flush(destination.virt, destination.byte_len);
    true
}

#[derive(Copy, Clone)]
struct AspectFitRect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

/// Fit one pixel extent into another without changing its aspect ratio. The
/// resulting content dimensions are even when possible so an NV12 producer's
/// 2x2 chroma samples remain naturally aligned.
fn aspect_fit_rect(
    source_width: u32,
    source_height: u32,
    destination_width: u32,
    destination_height: u32,
) -> Option<AspectFitRect> {
    if source_width == 0 || source_height == 0 || destination_width == 0 || destination_height == 0
    {
        return None;
    }

    let width_limited_height =
        u64::from(destination_width) * u64::from(source_height) / u64::from(source_width);
    let (mut width, mut height) = if width_limited_height <= u64::from(destination_height) {
        (destination_width, u32::try_from(width_limited_height).ok()?)
    } else {
        let height_limited_width =
            u64::from(destination_height) * u64::from(source_width) / u64::from(source_height);
        (u32::try_from(height_limited_width).ok()?, destination_height)
    };
    if width > 1 {
        width &= !1;
    }
    if height > 1 {
        height &= !1;
    }
    if width == 0 || height == 0 {
        return None;
    }

    Some(AspectFitRect {
        x: (destination_width - width) / 2,
        y: (destination_height - height) / 2,
        width,
        height,
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
