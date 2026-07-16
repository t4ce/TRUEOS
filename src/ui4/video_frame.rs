//! UI4 ownership wrapper for the proven decoded-video presentation path.
//!
//! The decoder still produces Y-tiled NV12 and the established staging copy
//! still produces linear NV12. The difference is ownership: the three staging
//! buffers are now one Streaming UI4 frame with normal write/read leases and a
//! window-broker identity before the linked display planes see a surface.

use core::sync::atomic::{AtomicU64, Ordering};

use spin::Mutex;

use super::{
    DamageRect, FrameCadence, FrameContent, FrameHandle, FrameSpec, OutputId, ScanoutFormat,
    WindowCreate, WindowId, WindowOwner, WindowPlacement, WindowSessionId,
    acknowledge_window_frame, acquire_frame_buffer, acquire_published_frame, begin_window_session,
    cancel_frame_buffer, create_window, destroy_frame, finish_window_session,
    import_native_nv12_frame, publish_frame_buffer, publish_window_frame,
    published_native_nv12_view, release_published_frame, writable_native_nv12_view,
};

const VIDEO_OWNER: WindowOwner = WindowOwner::KernelApp(3);
const VIDEO_OUTPUT: OutputId = OutputId::from_slot(0).unwrap();

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

#[derive(Copy, Clone)]
struct VideoStream {
    session: WindowSessionId,
    frame: FrameHandle,
    window: WindowId,
    source_width: u32,
    source_height: u32,
    scale: u32,
    frame_width: u32,
    frame_height: u32,
}

static VIDEO_STREAM: Mutex<Option<VideoStream>> = Mutex::new(None);
static VIDEO_PUBLISH_SEQ: AtomicU64 = AtomicU64::new(0);

pub(crate) fn present_decoded_nv12_stream_frame(source: DecodedNv12Source, reason: &str) -> bool {
    if !valid_source(source) {
        return false;
    }
    let scale = crate::intel::ui4_decoded_nv12_staging_scale(source.width, source.height).max(1);
    let Some(frame_width) = source.width.checked_mul(scale) else {
        return false;
    };
    let Some(frame_height) = source.height.checked_mul(scale) else {
        return false;
    };
    let Some(visible_width) = source.visible_width.checked_mul(scale) else {
        return false;
    };
    let Some(visible_height) = source.visible_height.checked_mul(scale) else {
        return false;
    };

    let current = *VIDEO_STREAM.lock();
    if current.is_some_and(|stream| {
        stream.source_width != source.width
            || stream.source_height != source.height
            || stream.scale != scale
            || stream.frame_width != frame_width
            || stream.frame_height != frame_height
    }) {
        let _ = stop_decoded_nv12_stream("ui4-video-format-change");
    }

    let stream = match *VIDEO_STREAM.lock() {
        Some(stream) => stream,
        None => match create_stream(source, scale, frame_width, frame_height) {
            Some(stream) => {
                *VIDEO_STREAM.lock() = Some(stream);
                stream
            }
            None => return false,
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
            return false;
        }
    };
    let destination = match writable_native_nv12_view(write) {
        Ok(surface) => surface,
        Err(error) => {
            let _ = cancel_frame_buffer(write);
            crate::log_warn!(
                target: "ui4";
                "ui4 video-frame view failed reason={} error={:?}\n",
                reason,
                error
            );
            return false;
        }
    };
    if !crate::intel::ui4_copy_decoded_ytile_nv12_to_linear(source, destination, scale) {
        let _ = cancel_frame_buffer(write);
        return false;
    }
    let published = match publish_frame_buffer(write) {
        Ok(published) => published,
        Err(_) => return false,
    };
    let window_serial = match publish_window_frame(VIDEO_OWNER, stream.window, DamageRect::FULL) {
        Ok(serial) => serial,
        Err(_) => return false,
    };

    let read = match acquire_published_frame(stream.frame) {
        Ok(read) => read,
        Err(_) => return false,
    };
    let presented = published_native_nv12_view(read).is_ok_and(|surface| {
        crate::intel::ui4_present_linear_nv12_surface(
            surface,
            visible_width,
            visible_height,
            reason,
        )
    });
    let _ = release_published_frame(read);
    if !presented {
        return false;
    }
    let _ = acknowledge_window_frame(stream.window, window_serial);

    let seq = VIDEO_PUBLISH_SEQ.fetch_add(1, Ordering::AcqRel) + 1;
    if seq <= 8 || seq.is_multiple_of(120) {
        crate::log_info!(
            target: "ui4";
            "ui4 video-frame published seq={} frame={} window={} buffer={} frame_serial={} window_serial={} source=ytile-nv12 {}x{} visible={}x{} staging=linear-nv12 {}x{} scale={} gpu=0x{:X} source_gpu=0x{:X}\n",
            seq,
            stream.frame.raw(),
            stream.window.raw(),
            published.buffer_index,
            published.publish_serial,
            window_serial,
            source.width,
            source.height,
            source.visible_width,
            source.visible_height,
            destination.width,
            destination.height,
            scale,
            destination.gpu,
            source.gpu,
        );
    }
    true
}

pub(crate) fn stop_decoded_nv12_stream(reason: &str) -> bool {
    let stream = VIDEO_STREAM.lock().take();
    let hidden = crate::intel::hide_decoded_nv12_overlay_plane(reason);
    if let Some(stream) = stream {
        let _ = finish_window_session(VIDEO_OWNER, stream.session);
        let _ = destroy_frame(stream.frame);
        crate::log_info!(
            target: "ui4";
            "ui4 video-frame stopped reason={} frame={} window={} hidden={}\n",
            reason,
            stream.frame.raw(),
            stream.window.raw(),
            hidden as u8
        );
    }
    hidden
}

fn create_stream(
    source: DecodedNv12Source,
    scale: u32,
    frame_width: u32,
    frame_height: u32,
) -> Option<VideoStream> {
    let surfaces = crate::intel::ui4_decoded_nv12_linear_staging_set(
        VIDEO_OUTPUT.slot(),
        frame_width,
        frame_height,
    )?;
    let frame = import_native_nv12_frame(
        FrameSpec {
            output: VIDEO_OUTPUT,
            content: FrameContent::Video,
            cadence: FrameCadence::Streaming,
            format: ScanoutFormat::Nv12Linear,
            width: frame_width,
            height: frame_height,
        },
        surfaces,
    )
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
        placement,
    }) {
        Ok(window) => window,
        Err(_) => {
            let _ = finish_window_session(VIDEO_OWNER, session);
            let _ = destroy_frame(frame);
            return None;
        }
    };
    crate::log_info!(
        target: "ui4";
        "ui4 video-frame created owner={:?} frame={} window={} buffers=3 cadence=streaming format=nv12-linear source=ytile-nv12 source_size={}x{} frame_size={}x{} scale={} placement={},{}\n",
        VIDEO_OWNER,
        frame.raw(),
        window.raw(),
        source.width,
        source.height,
        frame_width,
        frame_height,
        scale,
        placement.x,
        placement.y,
    );
    Some(VideoStream {
        session,
        frame,
        window,
        source_width: source.width,
        source_height: source.height,
        scale,
        frame_width,
        frame_height,
    })
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
