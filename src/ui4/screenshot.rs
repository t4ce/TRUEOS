//! UI4 composition, window-frame, and opt-in final-frame screenshots.
//!
//! Input only arms a request. The compositor consumes one request after a
//! successful frame, copies either the composition or one exact leased UI4
//! window buffer into an RGBA image, and queues that immutable image for the
//! filesystem worker. Session teardown can likewise copy its last published
//! buffers after broker detach and before producer frame destruction. Encoding
//! and TRUEOSFS I/O therefore never run in the composition or teardown path.

use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use embassy_time::{Duration, Timer};
use spin::Mutex;

use super::{
    FramePoolError, FrameReadLease, FrameRgbaView, WindowSnapshot, acquire_published_frame,
    published_rgba_view, release_published_frame,
};

const MAX_PENDING_REQUESTS: usize = 4;
const MAX_CAPTURE_QUEUE: usize = 4;
const SAVE_IDLE_PERIOD_MS: u64 = 25;
const ROOT_RETRY_PERIOD_MS: u64 = 500;
const SCREENSHOT_DIRECTORY: &str = "screenshots";
const FINAL_FRAME_DIRECTORY: &str = "finalframes";

/// Maximum encoded payload returned by one compact selected-window capture.
///
/// Keeping the image itself at 64 KiB leaves room for base64 expansion and
/// request metadata in transports with a substantially smaller limit than a
/// normal HTTP client.
pub(crate) const COMPACT_WINDOW_OBSERVATION_MAX_PNG_BYTES: usize = 64 * 1024;
/// Window-local coordinate extent painted into compact observations.
pub(crate) const COMPACT_WINDOW_GRID_EXTENT: u16 = 1_000;
/// Distance between the major coordinate lines painted into observations.
pub(crate) const COMPACT_WINDOW_GRID_MAJOR_STEP: u16 = 100;

const COMPACT_WINDOW_INITIAL_MAX_EDGE: u32 = 512;
const COMPACT_WINDOW_SHRINK_NUMERATOR: u32 = 3;
const COMPACT_WINDOW_SHRINK_DENOMINATOR: u32 = 4;
const COMPACT_WINDOW_BACKGROUND: [u8; 3] = [24, 24, 28];
const COMPACT_WINDOW_GRID_ALPHA: u8 = 72;
const COMPACT_WINDOW_LABEL_ALPHA: u8 = 184;

static CAPTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static CAPTURE_IN_FLIGHT: AtomicBool = AtomicBool::new(false);
static CAPTURE_REQUESTS: Mutex<VecDeque<CaptureRequest>> = Mutex::new(VecDeque::new());
static CAPTURE_QUEUE: Mutex<VecDeque<CapturedComposition>> = Mutex::new(VecDeque::new());

pub(super) enum CaptureError {
    NoScanout,
    DimensionTooLarge,
    InvalidFrameLayout,
    WindowUnavailable(super::WindowId),
    Frame(FramePoolError),
}

/// One transport-sized, vision-ready observation of an exact UI4 window.
///
/// `placement` is the presentation rectangle associated with the broker
/// snapshot. The PNG depicts the complete native buffer with its aspect ratio
/// intact, so normalized `0..=grid_extent` coordinates map directly across the
/// placement even when UI4 scales the native buffer for presentation.
pub(crate) struct CompactWindowObservation {
    pub(crate) window_id: super::WindowId,
    pub(crate) native_width: u32,
    pub(crate) native_height: u32,
    pub(crate) capture_width: u32,
    pub(crate) capture_height: u32,
    pub(crate) placement: super::WindowPlacement,
    /// Broker metadata copied from the supplied snapshot. The frame lease
    /// makes the pixels coherent; callers should still treat these identifiers
    /// as the freshness token for the observation they requested.
    pub(crate) revision: u64,
    pub(crate) publish_serial: u64,
    pub(crate) grid_extent: u16,
    pub(crate) grid_major_step: u16,
    pub(crate) png: Vec<u8>,
}

impl fmt::Debug for CompactWindowObservation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CompactWindowObservation")
            .field("window_id", &self.window_id)
            .field("native_width", &self.native_width)
            .field("native_height", &self.native_height)
            .field("capture_width", &self.capture_width)
            .field("capture_height", &self.capture_height)
            .field("placement", &self.placement)
            .field("revision", &self.revision)
            .field("publish_serial", &self.publish_serial)
            .field("grid_extent", &self.grid_extent)
            .field("grid_major_step", &self.grid_major_step)
            .field("png_bytes", &self.png.len())
            .finish()
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum CompactWindowObservationError {
    NoScanout,
    DimensionTooLarge,
    InvalidFrameLayout,
    WindowUnavailable(super::WindowId),
    Frame(FramePoolError),
    Png(crate::graphics::encoder::png::PngEncodeError),
    PngTooLarge {
        width: u32,
        height: u32,
        bytes: usize,
        limit: usize,
    },
}

impl From<CaptureError> for CompactWindowObservationError {
    fn from(error: CaptureError) -> Self {
        match error {
            CaptureError::NoScanout => Self::NoScanout,
            CaptureError::DimensionTooLarge => Self::DimensionTooLarge,
            CaptureError::InvalidFrameLayout => Self::InvalidFrameLayout,
            CaptureError::WindowUnavailable(window) => Self::WindowUnavailable(window),
            CaptureError::Frame(error) => Self::Frame(error),
        }
    }
}

impl From<crate::graphics::encoder::png::PngEncodeError> for CompactWindowObservationError {
    fn from(error: crate::graphics::encoder::png::PngEncodeError) -> Self {
        Self::Png(error)
    }
}

impl fmt::Debug for CaptureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoScanout => formatter.write_str("NoScanout"),
            Self::DimensionTooLarge => formatter.write_str("DimensionTooLarge"),
            Self::InvalidFrameLayout => formatter.write_str("InvalidFrameLayout"),
            Self::WindowUnavailable(window) => formatter
                .debug_tuple("WindowUnavailable")
                .field(&window.raw())
                .finish(),
            Self::Frame(error) => formatter.debug_tuple("Frame").field(error).finish(),
        }
    }
}

impl From<FramePoolError> for CaptureError {
    fn from(error: FramePoolError) -> Self {
        Self::Frame(error)
    }
}

#[derive(Copy, Clone)]
enum CaptureTrigger {
    MouseButton(u8),
    F1,
}

#[derive(Copy, Clone)]
enum CaptureSelection {
    Composition,
    Window {
        id: super::WindowId,
        plane_slot: usize,
    },
}

#[derive(Copy, Clone)]
struct CaptureRequest {
    trigger: CaptureTrigger,
    selection: CaptureSelection,
}

#[derive(Copy, Clone)]
enum CaptureScope {
    Composition,
    Window {
        id: super::WindowId,
        plane_slot: usize,
    },
}

struct CapturedComposition {
    sequence: u64,
    unix_seconds: Option<u64>,
    monotonic_ms: u64,
    width: u32,
    height: u32,
    /// Straight-alpha RGBA8 for file captures; premultiplied RGBA8 for streams.
    rgba: Vec<u8>,
    scope: CaptureScope,
    path_override: Option<String>,
    release_interactive_gate: bool,
    slot0_scanout_pixels: usize,
    spirit_overlay_pixels: usize,
}

/// Tight native premultiplied RGBA copied while holding one published-frame
/// read lease. Keeping this representation until the consumer chooses an
/// output avoids an unnecessary unpremultiply/re-premultiply round trip.
struct CapturedWindowRgba {
    width: u32,
    height: u32,
    rgba_premultiplied: Vec<u8>,
}

/// One immutable logical snapshot of D01, composed in the same hardware-plane
/// order as UI4 presentation. The encoder consumes this in memory; it never
/// enters the screenshot queue or filesystem worker.
pub(super) struct StreamScanoutCapture {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) slot0_scanout_pixels: usize,
    pub(super) spirit_overlay_pixels: usize,
}

pub(super) fn capture_stream_scanout_rgba_into(
    rgba_premultiplied: &mut [u8],
) -> Result<StreamScanoutCapture, CaptureError> {
    let output = super::OutputId::from_slot(0).ok_or(CaptureError::NoScanout)?;
    let (width, height) =
        crate::intel::active_scanout_dimensions().ok_or(CaptureError::NoScanout)?;
    let stride = usize::try_from(width)
        .ok()
        .and_then(|width| width.checked_mul(4))
        .ok_or(CaptureError::DimensionTooLarge)?;
    let byte_len = stride
        .checked_mul(usize::try_from(height).map_err(|_| CaptureError::DimensionTooLarge)?)
        .ok_or(CaptureError::DimensionTooLarge)?;
    if rgba_premultiplied.len() != byte_len {
        return Err(CaptureError::InvalidFrameLayout);
    }
    let mut windows = super::visible_windows_for_output(output);
    windows.sort_unstable_by_key(|window| (window.plane.slot(), window.placement.z, window.id));
    let rects = super::slot4_service::presented_rects();

    let mut leases = Vec::with_capacity(windows.len());
    for window in &windows {
        match acquire_published_frame(window.frame) {
            Ok(lease) => leases.push(lease),
            Err(error) => {
                release_leases(&leases);
                return Err(error.into());
            }
        }
    }
    let result = (|| {
        let views = leases
            .iter()
            .copied()
            .map(published_rgba_view)
            .collect::<Result<Vec<FrameRgbaView>, _>>()?;
        let slot0_scanout_pixels =
            copy_stream_pipe_a_slot0_premultiplied(rgba_premultiplied, width, height);
        if slot0_scanout_pixels == 0 {
            // The reusable stream buffer still contains the preceding frame.
            // Clear only when the full-screen physical base could not replace
            // it; the normal SURFLIVE path overwrites every destination byte.
            rgba_premultiplied.fill(0);
        }
        for (window, view) in windows.iter().zip(views.iter()) {
            crate::intel::dma_flush(view.virt, view.byte_len);
            let pixels =
                unsafe { core::slice::from_raw_parts(view.virt.cast_const(), view.byte_len) };
            blend_window(rgba_premultiplied, stride, width, height, *window, *view, pixels);
        }
        for rect in rects {
            blend_visual_rect(rgba_premultiplied, stride, width, height, rect);
        }
        let spirit_overlay_pixels =
            blend_stream_spirit_overlay_premultiplied(rgba_premultiplied, width, height);
        Ok::<_, CaptureError>((slot0_scanout_pixels, spirit_overlay_pixels))
    })();
    release_leases(&leases);
    let (slot0_scanout_pixels, spirit_overlay_pixels) = result?;

    Ok(StreamScanoutCapture {
        width,
        height,
        slot0_scanout_pixels,
        spirit_overlay_pixels,
    })
}

fn copy_stream_pipe_a_slot0_premultiplied(
    destination: &mut [u8],
    width: u32,
    height: u32,
) -> usize {
    let Some(row_bytes) = (width as usize).checked_mul(4) else {
        return 0;
    };
    crate::intel::with_ui4_stream_pipe_a_slot0_surflive(|slot0| {
        if slot0.width != width
            || slot0.height != height
            || (slot0.pitch_bytes as usize) < row_bytes
        {
            return 0;
        }
        for row in 0..height as usize {
            let source_offset = row.saturating_mul(slot0.pitch_bytes as usize);
            let destination_offset = row.saturating_mul(row_bytes);
            let Some(source_row) = slot0
                .rgba_premultiplied
                .get(source_offset..source_offset + row_bytes)
            else {
                return 0;
            };
            let Some(destination_row) =
                destination.get_mut(destination_offset..destination_offset + row_bytes)
            else {
                return 0;
            };
            destination_row.copy_from_slice(source_row);
        }
        (width as usize).saturating_mul(height as usize)
    })
    .unwrap_or(0)
}

fn blend_stream_spirit_overlay_premultiplied(
    destination: &mut [u8],
    width: u32,
    height: u32,
) -> usize {
    let destination_stride = width as usize * 4;
    crate::spirit::with_stream_overlay_pipe_a(|overlay| {
        let left = i64::from(overlay.left).max(0);
        let top = i64::from(overlay.top).max(0);
        let right = (i64::from(overlay.left) + i64::from(overlay.width)).min(i64::from(width));
        let bottom = (i64::from(overlay.top) + i64::from(overlay.height)).min(i64::from(height));
        if right <= left || bottom <= top {
            return 0;
        }

        let source_x = left.saturating_sub(i64::from(overlay.left)) as usize;
        let source_y = top.saturating_sub(i64::from(overlay.top)) as usize;
        let copy_width = (right - left) as usize;
        let copy_height = (bottom - top) as usize;
        let mut blended_pixels = 0usize;
        for row in 0..copy_height {
            let source_offset = (source_y + row)
                .saturating_mul(overlay.pitch_bytes as usize)
                .saturating_add(source_x.saturating_mul(4));
            let destination_offset = (top as usize + row)
                .saturating_mul(destination_stride)
                .saturating_add(left as usize * 4);
            let Some(source_row) = overlay
                .bgra_premultiplied
                .get(source_offset..source_offset + copy_width * 4)
            else {
                return blended_pixels;
            };
            let Some(destination_row) =
                destination.get_mut(destination_offset..destination_offset + copy_width * 4)
            else {
                return blended_pixels;
            };
            for (source, destination) in source_row
                .chunks_exact(4)
                .zip(destination_row.chunks_exact_mut(4))
            {
                if source[3] == 0 {
                    continue;
                }
                blend_premultiplied(destination, source[2], source[1], source[0], source[3]);
                blended_pixels = blended_pixels.saturating_add(1);
            }
        }
        blended_pixels
    })
    .unwrap_or(0)
}

/// Arm one composition capture. Calls are bounded so a burst of side-button
/// transitions cannot retain an unbounded number of full-screen images.
pub(super) fn request_capture(mouse_button: u8) {
    enqueue_request(CaptureRequest {
        trigger: CaptureTrigger::MouseButton(mouse_button),
        selection: CaptureSelection::Composition,
    });
}

/// Arm a capture of one exact UI4 window on the next composed frame.
pub(super) fn request_window_capture(window: WindowSnapshot, cursor_x: u32, cursor_y: u32) {
    let pending = enqueue_request(CaptureRequest {
        trigger: CaptureTrigger::F1,
        selection: CaptureSelection::Window {
            id: window.id,
            plane_slot: window.plane.slot(),
        },
    });
    if let Some(pending) = pending {
        crate::log_info!(target: "ui4/screenshot";
            "ui4/screenshot: F1 target selected window={} plane_slot={} z={} cursor={},{} pending={} capture=published-ui4-frame alpha=straight\n",
            window.id.raw(),
            window.plane.slot(),
            window.placement.z,
            cursor_x,
            cursor_y,
            pending,
        );
    }
}

fn enqueue_request(request: CaptureRequest) -> Option<usize> {
    let mut requests = CAPTURE_REQUESTS.lock();
    if requests.len() >= MAX_PENDING_REQUESTS {
        match request.trigger {
            CaptureTrigger::MouseButton(button) => crate::log_warn!(target: "ui4/screenshot";
                "ui4/screenshot: request dropped trigger=mouse-button-{} reason=request-queue-full pending={}\n",
                button,
                requests.len(),
            ),
            CaptureTrigger::F1 => crate::log_warn!(target: "ui4/screenshot";
                "ui4/screenshot: request dropped trigger=F1 reason=request-queue-full pending={}\n",
                requests.len(),
            ),
        }
        return None;
    }
    requests.push_back(request);
    let pending = requests.len();
    if let CaptureTrigger::MouseButton(button) = request.trigger {
        crate::log_info!(target: "ui4/screenshot";
            "ui4/screenshot: request armed trigger=mouse-button-{} pending={} capture=next-composed-frame\n",
            button,
            pending,
        );
    }
    Some(pending)
}

/// Capture at most one request from this compositor frame.
///
/// `windows` is the immutable broker snapshot used for the frame. Slot 4 is
/// sampled from its independent service only after a capture request is
/// actually consumed. The hardware mouse cursor remains intentionally absent.
pub(super) fn capture_compositor_frame(windows: &[WindowSnapshot]) {
    let request = {
        let mut requests = CAPTURE_REQUESTS.lock();
        if requests.is_empty()
            || CAPTURE_IN_FLIGHT
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return;
        }
        requests.pop_front()
    };
    let Some(request) = request else {
        CAPTURE_IN_FLIGHT.store(false, Ordering::Release);
        return;
    };

    let started_ns = crate::chronos::monotonic_nanos();
    let rects = match request.selection {
        CaptureSelection::Composition => super::slot4_service::presented_rects(),
        CaptureSelection::Window { .. } => heapless::Vec::new(),
    };
    let result = match request.selection {
        CaptureSelection::Composition => capture_windows(windows, &rects, false),
        CaptureSelection::Window { id, plane_slot } => windows
            .iter()
            .copied()
            .find(|window| window.id == id && window.plane.slot() == plane_slot)
            .ok_or(CaptureError::WindowUnavailable(id))
            .and_then(capture_window),
    };
    match result {
        Ok(capture) => {
            let sequence = capture.sequence;
            let width = capture.width;
            let height = capture.height;
            let bytes = capture.rgba.len();
            let mut queue = CAPTURE_QUEUE.lock();
            if queue.len() >= MAX_CAPTURE_QUEUE {
                CAPTURE_IN_FLIGHT.store(false, Ordering::Release);
                crate::log_warn!(target: "ui4/screenshot";
                    "ui4/screenshot: capture dropped sequence={} reason=encode-queue-full size={}x{} bytes={}\n",
                    sequence,
                    width,
                    height,
                    bytes,
                );
                return;
            }
            queue.push_back(capture);
            drop(queue);
            let (scope, window, plane_slot) = capture_scope_log_fields(request.selection);
            crate::log_info!(target: "ui4/screenshot";
                "ui4/screenshot: frame captured sequence={} scope={} window={} plane_slot={} size={}x{} rgba_bytes={} windows={} visuals={} copy_us={} alpha=straight\n",
                sequence,
                scope,
                window,
                plane_slot,
                width,
                height,
                bytes,
                windows.len(),
                rects.len(),
                crate::chronos::monotonic_nanos().saturating_sub(started_ns) / 1_000,
            );
        }
        Err(error) => {
            CAPTURE_IN_FLIGHT.store(false, Ordering::Release);
            crate::log_warn!(target: "ui4/screenshot";
                "ui4/screenshot: capture failed error={:?} windows={} visuals={}\n",
                error,
                windows.len(),
                rects.len(),
            );
        }
    }
}

fn capture_scope_log_fields(selection: CaptureSelection) -> (&'static str, u32, usize) {
    match selection {
        CaptureSelection::Composition => ("composition", 0, 0),
        CaptureSelection::Window { id, plane_slot } => ("window", id.raw(), plane_slot),
    }
}

fn capture_windows(
    windows: &[WindowSnapshot],
    rects: &[crate::intel::LiveOverlayRect],
    stream_capture: bool,
) -> Result<CapturedComposition, CaptureError> {
    let (width, height) =
        crate::intel::active_scanout_dimensions().ok_or(CaptureError::NoScanout)?;
    let stride = usize::try_from(width)
        .ok()
        .and_then(|width| width.checked_mul(4))
        .ok_or(CaptureError::DimensionTooLarge)?;
    let byte_len = stride
        .checked_mul(usize::try_from(height).map_err(|_| CaptureError::DimensionTooLarge)?)
        .ok_or(CaptureError::DimensionTooLarge)?;

    // Hardware planes impose an order across otherwise independent z stacks.
    // Retain that order in the exported logical composition as well.
    let mut ordered_windows = windows.to_vec();
    ordered_windows
        .sort_unstable_by_key(|window| (window.plane.slot(), window.placement.z, window.id));

    // Hold all front buffers simultaneously. Producers may continue into
    // other cadence buffers, but none of these pixels can change mid-copy.
    let mut leases = Vec::with_capacity(ordered_windows.len());
    for window in &ordered_windows {
        match acquire_published_frame(window.frame) {
            Ok(lease) => leases.push(lease),
            Err(error) => {
                release_leases(&leases);
                return Err(error.into());
            }
        }
    }
    let result = (|| {
        let views = leases
            .iter()
            .copied()
            .map(published_rgba_view)
            .collect::<Result<Vec<FrameRgbaView>, _>>()?;
        let mut rgba = alloc::vec![0u8; byte_len];
        // D01's original pipe-A primary is the immutable opaque background.
        // The fixed test rig has no broker windows on slot 0; a changed
        // SURFLIVE therefore rejects this copy in the display accessor.
        let slot0_scanout_pixels = if stream_capture {
            copy_stream_pipe_a_slot0_premultiplied(&mut rgba, width, height)
        } else {
            0
        };
        for (window, view) in ordered_windows.iter().zip(views.iter()) {
            crate::intel::dma_flush(view.virt, view.byte_len);
            let pixels =
                unsafe { core::slice::from_raw_parts(view.virt.cast_const(), view.byte_len) };
            blend_window(&mut rgba, stride, width, height, *window, *view, pixels);
        }
        for rect in rects {
            blend_visual_rect(&mut rgba, stride, width, height, *rect);
        }
        // The encoder composites premultiplied RGB directly onto black. Keep
        // that representation for stream capture and reserve the expensive
        // straight-alpha conversion for exported screenshots.
        let spirit_overlay_pixels = if stream_capture {
            blend_stream_spirit_overlay_premultiplied(&mut rgba, width, height)
        } else {
            0
        };
        if !stream_capture {
            unpremultiply_rgba(&mut rgba);
        }
        Ok::<_, CaptureError>((rgba, slot0_scanout_pixels, spirit_overlay_pixels))
    })();
    release_leases(&leases);
    let (rgba, slot0_scanout_pixels, spirit_overlay_pixels) = result?;

    Ok(CapturedComposition {
        sequence: CAPTURE_SEQUENCE.fetch_add(1, Ordering::AcqRel) + 1,
        unix_seconds: crate::chronos::best_effort_unix_time_seconds(),
        monotonic_ms: crate::chronos::monotonic_nanos() / 1_000_000,
        width,
        height,
        rgba,
        scope: CaptureScope::Composition,
        path_override: None,
        release_interactive_gate: true,
        slot0_scanout_pixels,
        spirit_overlay_pixels,
    })
}

/// Capture and encode one exact broker window as a compact RGB PNG carrying a
/// light normalized coordinate grid.
///
/// The first attempt preserves the native aspect ratio within a 512-pixel long
/// edge. If the encoded PNG is larger than the hard payload budget, each retry
/// reduces that edge to 75 percent and redraws the grid at the new resolution.
/// No oversized image is ever returned.
pub(crate) fn capture_compact_window_observation(
    window: WindowSnapshot,
) -> Result<CompactWindowObservation, CompactWindowObservationError> {
    let captured = capture_window_rgba(window)?;
    let native_width = captured.width;
    let native_height = captured.height;
    let (capture_width, capture_height, png) = encode_compact_window_png(
        native_width,
        native_height,
        captured.rgba_premultiplied.as_slice(),
    )?;

    Ok(CompactWindowObservation {
        window_id: window.id,
        native_width,
        native_height,
        capture_width,
        capture_height,
        placement: window.placement,
        revision: window.revision,
        publish_serial: window.publish_serial,
        grid_extent: COMPACT_WINDOW_GRID_EXTENT,
        grid_major_step: COMPACT_WINDOW_GRID_MAJOR_STEP,
        png,
    })
}

fn capture_window_rgba(window: WindowSnapshot) -> Result<CapturedWindowRgba, CaptureError> {
    let lease = acquire_published_frame(window.frame)?;
    let result = (|| {
        let view = published_rgba_view(lease)?;
        let row_bytes = usize::try_from(view.width)
            .ok()
            .and_then(|width| width.checked_mul(4))
            .ok_or(CaptureError::DimensionTooLarge)?;
        let height = usize::try_from(view.height).map_err(|_| CaptureError::DimensionTooLarge)?;
        let byte_len = row_bytes
            .checked_mul(height)
            .ok_or(CaptureError::DimensionTooLarge)?;
        crate::intel::dma_flush(view.virt, view.byte_len);
        let source = unsafe { core::slice::from_raw_parts(view.virt.cast_const(), view.byte_len) };
        let mut rgba = alloc::vec![0u8; byte_len];
        for row in 0..height {
            let source_offset = row
                .checked_mul(view.pitch as usize)
                .ok_or(CaptureError::DimensionTooLarge)?;
            let destination_offset = row
                .checked_mul(row_bytes)
                .ok_or(CaptureError::DimensionTooLarge)?;
            let source_end = source_offset
                .checked_add(row_bytes)
                .ok_or(CaptureError::DimensionTooLarge)?;
            let destination_end = destination_offset
                .checked_add(row_bytes)
                .ok_or(CaptureError::DimensionTooLarge)?;
            let source_row = source
                .get(source_offset..source_end)
                .ok_or(CaptureError::InvalidFrameLayout)?;
            let destination_row = rgba
                .get_mut(destination_offset..destination_end)
                .ok_or(CaptureError::InvalidFrameLayout)?;
            destination_row.copy_from_slice(source_row);
        }
        Ok::<_, CaptureError>(CapturedWindowRgba {
            width: view.width,
            height: view.height,
            rgba_premultiplied: rgba,
        })
    })();
    let _ = release_published_frame(lease);
    result
}

fn capture_window(window: WindowSnapshot) -> Result<CapturedComposition, CaptureError> {
    let captured = capture_window_rgba(window)?;
    let mut rgba = captured.rgba_premultiplied;
    unpremultiply_rgba(&mut rgba);

    Ok(CapturedComposition {
        sequence: CAPTURE_SEQUENCE.fetch_add(1, Ordering::AcqRel) + 1,
        unix_seconds: crate::chronos::best_effort_unix_time_seconds(),
        monotonic_ms: crate::chronos::monotonic_nanos() / 1_000_000,
        width: captured.width,
        height: captured.height,
        rgba,
        scope: CaptureScope::Window {
            id: window.id,
            plane_slot: window.plane.slot(),
        },
        path_override: None,
        release_interactive_gate: true,
        slot0_scanout_pixels: 0,
        spirit_overlay_pixels: 0,
    })
}

fn encode_compact_window_png(
    native_width: u32,
    native_height: u32,
    rgba_premultiplied: &[u8],
) -> Result<(u32, u32, Vec<u8>), CompactWindowObservationError> {
    let native_long_edge = native_width.max(native_height);
    if native_long_edge == 0 {
        return Err(CompactWindowObservationError::InvalidFrameLayout);
    }
    let mut max_edge = native_long_edge.min(COMPACT_WINDOW_INITIAL_MAX_EDGE);

    loop {
        let (width, height) = compact_dimensions(native_width, native_height, max_edge)?;
        let mut rgb = resample_premultiplied_rgba_to_rgb(
            native_width,
            native_height,
            rgba_premultiplied,
            width,
            height,
        )?;
        overlay_normalized_grid(rgb.as_mut_slice(), width, height)?;
        let stride = usize::try_from(width)
            .ok()
            .and_then(|width| width.checked_mul(3))
            .ok_or(CompactWindowObservationError::DimensionTooLarge)?;
        let png =
            crate::graphics::encoder::png::encode_rgb8_png(width, height, rgb.as_slice(), stride)?;
        if png.len() <= COMPACT_WINDOW_OBSERVATION_MAX_PNG_BYTES {
            return Ok((width, height, png));
        }
        if max_edge == 1 {
            return Err(CompactWindowObservationError::PngTooLarge {
                width,
                height,
                bytes: png.len(),
                limit: COMPACT_WINDOW_OBSERVATION_MAX_PNG_BYTES,
            });
        }
        let reduced = max_edge.saturating_mul(COMPACT_WINDOW_SHRINK_NUMERATOR)
            / COMPACT_WINDOW_SHRINK_DENOMINATOR;
        max_edge = reduced.clamp(1, max_edge - 1);
    }
}

fn compact_dimensions(
    native_width: u32,
    native_height: u32,
    max_edge: u32,
) -> Result<(u32, u32), CompactWindowObservationError> {
    if native_width == 0 || native_height == 0 || max_edge == 0 {
        return Err(CompactWindowObservationError::InvalidFrameLayout);
    }
    let native_long_edge = native_width.max(native_height);
    if native_long_edge <= max_edge {
        return Ok((native_width, native_height));
    }

    let scale_dimension = |dimension: u32| {
        let numerator = u64::from(dimension)
            .saturating_mul(u64::from(max_edge))
            .saturating_add(u64::from(native_long_edge / 2));
        u32::try_from(numerator / u64::from(native_long_edge))
            .unwrap_or(u32::MAX)
            .max(1)
    };
    if native_width >= native_height {
        Ok((max_edge, scale_dimension(native_height)))
    } else {
        Ok((scale_dimension(native_width), max_edge))
    }
}

fn resample_premultiplied_rgba_to_rgb(
    source_width: u32,
    source_height: u32,
    source: &[u8],
    destination_width: u32,
    destination_height: u32,
) -> Result<Vec<u8>, CompactWindowObservationError> {
    let source_width_usize = usize::try_from(source_width)
        .map_err(|_| CompactWindowObservationError::DimensionTooLarge)?;
    let source_height_usize = usize::try_from(source_height)
        .map_err(|_| CompactWindowObservationError::DimensionTooLarge)?;
    let source_stride = source_width_usize
        .checked_mul(4)
        .ok_or(CompactWindowObservationError::DimensionTooLarge)?;
    let source_len = source_stride
        .checked_mul(source_height_usize)
        .ok_or(CompactWindowObservationError::DimensionTooLarge)?;
    if source_width == 0
        || source_height == 0
        || destination_width == 0
        || destination_height == 0
        || source.len() < source_len
    {
        return Err(CompactWindowObservationError::InvalidFrameLayout);
    }

    let destination_width_usize = usize::try_from(destination_width)
        .map_err(|_| CompactWindowObservationError::DimensionTooLarge)?;
    let destination_height_usize = usize::try_from(destination_height)
        .map_err(|_| CompactWindowObservationError::DimensionTooLarge)?;
    let destination_stride = destination_width_usize
        .checked_mul(3)
        .ok_or(CompactWindowObservationError::DimensionTooLarge)?;
    let destination_len = destination_stride
        .checked_mul(destination_height_usize)
        .ok_or(CompactWindowObservationError::DimensionTooLarge)?;
    let mut destination = alloc::vec![0u8; destination_len];

    let source_width_u64 = u64::from(source_width);
    let source_height_u64 = u64::from(source_height);
    let destination_width_u64 = u64::from(destination_width);
    let destination_height_u64 = u64::from(destination_height);
    for destination_y in 0..destination_height_usize {
        let source_y = ((2 * destination_y as u64 + 1) * source_height_u64
            / (2 * destination_height_u64))
            .min(source_height_u64 - 1) as usize;
        for destination_x in 0..destination_width_usize {
            let source_x = ((2 * destination_x as u64 + 1) * source_width_u64
                / (2 * destination_width_u64))
                .min(source_width_u64 - 1) as usize;
            let source_offset = source_y * source_stride + source_x * 4;
            let destination_offset = destination_y * destination_stride + destination_x * 3;
            let alpha = source[source_offset + 3];
            let inverse_alpha = u8::MAX - alpha;
            for channel in 0..3 {
                destination[destination_offset + channel] =
                    u16::from(source[source_offset + channel])
                        .saturating_add(u16::from(multiply_u8(
                            COMPACT_WINDOW_BACKGROUND[channel],
                            inverse_alpha,
                        )))
                        .min(255) as u8;
            }
        }
    }
    Ok(destination)
}

fn overlay_normalized_grid(
    rgb: &mut [u8],
    width: u32,
    height: u32,
) -> Result<(), CompactWindowObservationError> {
    let width_usize =
        usize::try_from(width).map_err(|_| CompactWindowObservationError::DimensionTooLarge)?;
    let height_usize =
        usize::try_from(height).map_err(|_| CompactWindowObservationError::DimensionTooLarge)?;
    let expected_len = width_usize
        .checked_mul(height_usize)
        .and_then(|pixels| pixels.checked_mul(3))
        .ok_or(CompactWindowObservationError::DimensionTooLarge)?;
    if width == 0 || height == 0 || rgb.len() < expected_len {
        return Err(CompactWindowObservationError::InvalidFrameLayout);
    }

    let divisions = u32::from(COMPACT_WINDOW_GRID_EXTENT / COMPACT_WINDOW_GRID_MAJOR_STEP);
    for division in 0..=divisions {
        let x = grid_coordinate(division, divisions, width);
        for y in 0..height {
            blend_contrast_rgb_pixel(rgb, width, height, x, y, COMPACT_WINDOW_GRID_ALPHA);
        }
        let y = grid_coordinate(division, divisions, height);
        for x in 0..width {
            blend_contrast_rgb_pixel(rgb, width, height, x, y, COMPACT_WINDOW_GRID_ALPHA);
        }
    }

    // Tiny labels are useful at ordinary observation sizes but become more
    // occlusion than guidance after adaptive shrinking.
    if width >= 240 && height >= 7 {
        let mut digits = [0u8; 4];
        for division in 0..=divisions {
            let value = division * u32::from(COMPACT_WINDOW_GRID_MAJOR_STEP);
            let label = decimal_label(value, &mut digits);
            let text_width = compact_text_width(label.len());
            let center = grid_coordinate(division, divisions, width);
            let x = center
                .saturating_sub(text_width / 2)
                .min(width.saturating_sub(text_width));
            draw_compact_text(rgb, width, height, x, 2, label);
        }
    }
    if height >= 100 && width >= 17 {
        let mut digits = [0u8; 4];
        for division in 0..=divisions {
            let value = division * u32::from(COMPACT_WINDOW_GRID_MAJOR_STEP);
            let label = decimal_label(value, &mut digits);
            let center = grid_coordinate(division, divisions, height);
            let y = center.saturating_sub(2).min(height.saturating_sub(5));
            draw_compact_text(rgb, width, height, 2, y, label);
        }
    }
    Ok(())
}

fn grid_coordinate(division: u32, divisions: u32, extent: u32) -> u32 {
    if divisions == 0 || extent <= 1 {
        return 0;
    }
    division
        .saturating_mul(extent - 1)
        .saturating_add(divisions / 2)
        / divisions
}

fn decimal_label<'a>(mut value: u32, buffer: &'a mut [u8; 4]) -> &'a [u8] {
    let mut cursor = buffer.len();
    loop {
        cursor -= 1;
        buffer[cursor] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            return &buffer[cursor..];
        }
    }
}

fn compact_text_width(byte_len: usize) -> u32 {
    if byte_len == 0 {
        0
    } else {
        (byte_len as u32).saturating_mul(4).saturating_sub(1)
    }
}

fn draw_compact_text(rgb: &mut [u8], width: u32, height: u32, x: u32, y: u32, text: &[u8]) {
    for (character_index, character) in text.iter().copied().enumerate() {
        let glyph_x = x.saturating_add((character_index as u32).saturating_mul(4));
        for (row, bits) in compact_digit_glyph(character).into_iter().enumerate() {
            for column in 0..3u32 {
                if bits & (1 << (2 - column)) != 0 {
                    blend_contrast_rgb_pixel(
                        rgb,
                        width,
                        height,
                        glyph_x.saturating_add(column),
                        y.saturating_add(row as u32),
                        COMPACT_WINDOW_LABEL_ALPHA,
                    );
                }
            }
        }
    }
}

fn compact_digit_glyph(character: u8) -> [u8; 5] {
    match character {
        b'0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        b'1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        b'2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        b'3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        b'4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        b'5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        b'6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        b'7' => [0b111, 0b001, 0b010, 0b010, 0b010],
        b'8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        b'9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        _ => [0; 5],
    }
}

fn blend_contrast_rgb_pixel(rgb: &mut [u8], width: u32, height: u32, x: u32, y: u32, alpha: u8) {
    if x >= width || y >= height {
        return;
    }
    let Some(offset) = (y as usize)
        .checked_mul(width as usize)
        .and_then(|pixel| pixel.checked_add(x as usize))
        .and_then(|pixel| pixel.checked_mul(3))
    else {
        return;
    };
    let Some(pixel) = rgb.get_mut(offset..offset + 3) else {
        return;
    };
    let luminance =
        (u32::from(pixel[0]) * 77 + u32::from(pixel[1]) * 150 + u32::from(pixel[2]) * 29) >> 8;
    let target = if luminance < 128 { u8::MAX } else { 0 };
    let inverse_alpha = u8::MAX - alpha;
    for channel in pixel {
        *channel = ((u16::from(*channel) * u16::from(inverse_alpha)
            + u16::from(target) * u16::from(alpha)
            + 127)
            / 255) as u8;
    }
}

/// Copy the last published frames after the broker has atomically detached the
/// session but before its consumer can destroy the backing frame handles.
/// Final-frame requests intentionally do not wait for a future filesystem: an
/// absent writable root makes teardown capture a cheap no-op.
pub(super) fn capture_final_session_frames(
    owner: super::WindowOwner,
    session: super::WindowSessionId,
    windows: &[WindowSnapshot],
    requested_name: Option<&str>,
) {
    if windows.is_empty() {
        crate::log_info!(target: "ui4/screenshot";
            "ui4/final-frame: skipped owner={:?} session={} reason=no-ready-published-window\n",
            owner,
            session.raw(),
        );
        return;
    }
    if writable_capture_root_handle().is_none() {
        crate::log_info!(target: "ui4/screenshot";
            "ui4/final-frame: skipped owner={:?} session={} frames={} reason=no-writable-trueosfs-root mounted_roots={}\n",
            owner,
            session.raw(),
            windows.len(),
            crate::r::fs::trueosfs::roots_len(),
        );
        return;
    }

    let identity = final_frame_identity(owner, requested_name);
    for (index, window) in windows.iter().copied().enumerate() {
        if CAPTURE_QUEUE.lock().len() >= MAX_CAPTURE_QUEUE {
            crate::log_warn!(target: "ui4/screenshot";
                "ui4/final-frame: dropped owner={:?} session={} identity={} window={} index={}/{} reason=capture-queue-full capacity={}\n",
                owner,
                session.raw(),
                identity.as_str(),
                window.id.raw(),
                index + 1,
                windows.len(),
                MAX_CAPTURE_QUEUE,
            );
            continue;
        }

        let mut capture = match capture_window(window) {
            Ok(capture) => capture,
            Err(error) => {
                crate::log_warn!(target: "ui4/screenshot";
                    "ui4/final-frame: capture failed owner={:?} session={} identity={} window={} index={}/{} error={:?}\n",
                    owner,
                    session.raw(),
                    identity.as_str(),
                    window.id.raw(),
                    index + 1,
                    windows.len(),
                    error,
                );
                continue;
            }
        };
        capture.path_override = Some(final_frame_path(identity.as_str(), index, windows.len()));
        capture.release_interactive_gate = false;
        let path = capture
            .path_override
            .as_deref()
            .unwrap_or("finalframes/invalid.png");
        let width = capture.width;
        let height = capture.height;
        let bytes = capture.rgba.len();
        let sequence = capture.sequence;
        let mut queue = CAPTURE_QUEUE.lock();
        if queue.len() >= MAX_CAPTURE_QUEUE {
            crate::log_warn!(target: "ui4/screenshot";
                "ui4/final-frame: dropped owner={:?} session={} path=trueosfs:/{} window={} reason=capture-queue-raced-full capacity={}\n",
                owner,
                session.raw(),
                path,
                window.id.raw(),
                MAX_CAPTURE_QUEUE,
            );
            continue;
        }
        crate::log_info!(target: "ui4/screenshot";
            "ui4/final-frame: captured owner={:?} session={} path=trueosfs:/{} window={} index={}/{} sequence={} size={}x{} rgba_bytes={} alpha=straight overwrite=1\n",
            owner,
            session.raw(),
            path,
            window.id.raw(),
            index + 1,
            windows.len(),
            sequence,
            width,
            height,
            bytes,
        );
        queue.push_back(capture);
    }
}

fn final_frame_identity(owner: super::WindowOwner, requested_name: Option<&str>) -> String {
    let source = match requested_name {
        Some(name) if !name.trim().is_empty() => String::from(name),
        _ => match owner {
            super::WindowOwner::Vm(vm_id) => crate::hv::blueprint_process_arg(vm_id, 0)
                .unwrap_or_else(|| format!("blueprint-vm-{vm_id}")),
            super::WindowOwner::KernelApp(app_id) => format!("dummy-kernel-app-{app_id}"),
            super::WindowOwner::Kernel => String::from("dummy-kernel"),
        },
    };
    sanitize_final_frame_identity(source.as_str())
}

fn sanitize_final_frame_identity(source: &str) -> String {
    const MAX_IDENTITY_BYTES: usize = 96;

    let leaf = source
        .trim()
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(source)
        .trim_end_matches(".bp");
    let mut identity = String::new();
    let mut separator = false;
    for ch in leaf.chars() {
        if identity.len() >= MAX_IDENTITY_BYTES {
            break;
        }
        if ch.is_ascii_alphanumeric() {
            identity.push(ch.to_ascii_lowercase());
            separator = false;
        } else if !identity.is_empty() && !separator {
            identity.push('-');
            separator = true;
        }
    }
    while identity.ends_with('-') {
        identity.pop();
    }
    if identity.is_empty() {
        String::from("ui4-app")
    } else {
        identity
    }
}

fn final_frame_path(identity: &str, index: usize, count: usize) -> String {
    if count == 1 {
        format!("{FINAL_FRAME_DIRECTORY}/{identity}.png")
    } else {
        format!("{FINAL_FRAME_DIRECTORY}/{identity}-window-{:02}.png", index + 1)
    }
}

fn release_leases(leases: &[FrameReadLease]) {
    for lease in leases {
        let _ = release_published_frame(*lease);
    }
}

fn blend_window(
    destination: &mut [u8],
    destination_stride: usize,
    screen_width: u32,
    screen_height: u32,
    window: WindowSnapshot,
    view: FrameRgbaView,
    source: &[u8],
) {
    let placement = window.placement;
    if !placement.visible || placement.opacity == 0 {
        return;
    }
    let draw_width = view.width.min(placement.width);
    let draw_height = view.height.min(placement.height);
    let left = i64::from(placement.x).max(0);
    let top = i64::from(placement.y).max(0);
    let right = (i64::from(placement.x) + i64::from(draw_width)).min(i64::from(screen_width));
    let bottom = (i64::from(placement.y) + i64::from(draw_height)).min(i64::from(screen_height));
    if right <= left || bottom <= top {
        return;
    }
    let source_x = left.saturating_sub(i64::from(placement.x)) as usize;
    let source_y = top.saturating_sub(i64::from(placement.y)) as usize;
    let copy_width = (right - left) as usize;
    let copy_height = (bottom - top) as usize;
    let opacity = placement.opacity;

    for row in 0..copy_height {
        let source_offset = (source_y + row)
            .saturating_mul(view.pitch as usize)
            .saturating_add(source_x.saturating_mul(4));
        let destination_offset = (top as usize + row)
            .saturating_mul(destination_stride)
            .saturating_add(left as usize * 4);
        let Some(source_row) = source.get(source_offset..source_offset + copy_width * 4) else {
            return;
        };
        let Some(destination_row) =
            destination.get_mut(destination_offset..destination_offset + copy_width * 4)
        else {
            return;
        };
        if opacity == u8::MAX
            && source_row
                .chunks_exact(4)
                .all(|source| source[3] == u8::MAX)
        {
            destination_row.copy_from_slice(source_row);
            continue;
        }
        for (src, dst) in source_row
            .chunks_exact(4)
            .zip(destination_row.chunks_exact_mut(4))
        {
            if src[3] == 0 {
                continue;
            }
            if opacity == u8::MAX {
                if src[3] == u8::MAX {
                    dst.copy_from_slice(src);
                } else {
                    blend_premultiplied(dst, src[0], src[1], src[2], src[3]);
                }
            } else {
                let scale = |value: u8| multiply_u8(value, opacity);
                blend_premultiplied(
                    dst,
                    scale(src[0]),
                    scale(src[1]),
                    scale(src[2]),
                    scale(src[3]),
                );
            }
        }
    }
}

fn blend_visual_rect(
    destination: &mut [u8],
    destination_stride: usize,
    screen_width: u32,
    screen_height: u32,
    rect: crate::intel::LiveOverlayRect,
) {
    let right = rect.x.saturating_add(rect.width).min(screen_width);
    let bottom = rect.y.saturating_add(rect.height).min(screen_height);
    if right <= rect.x || bottom <= rect.y || rect.color.a == 0 {
        return;
    }
    let alpha = rect.color.a;
    let r = multiply_u8(rect.color.r, alpha);
    let g = multiply_u8(rect.color.g, alpha);
    let b = multiply_u8(rect.color.b, alpha);
    for y in rect.y..bottom {
        let row_offset = y as usize * destination_stride;
        for x in rect.x..right {
            let offset = row_offset + x as usize * 4;
            blend_premultiplied(&mut destination[offset..offset + 4], r, g, b, alpha);
        }
    }
}

#[inline]
fn multiply_u8(value: u8, factor: u8) -> u8 {
    ((u16::from(value) * u16::from(factor) + 127) / 255) as u8
}

#[inline]
fn blend_premultiplied(destination: &mut [u8], r: u8, g: u8, b: u8, a: u8) {
    let inverse_alpha = u8::MAX - a;
    destination[0] = u16::from(r)
        .saturating_add(u16::from(multiply_u8(destination[0], inverse_alpha)))
        .min(255) as u8;
    destination[1] = u16::from(g)
        .saturating_add(u16::from(multiply_u8(destination[1], inverse_alpha)))
        .min(255) as u8;
    destination[2] = u16::from(b)
        .saturating_add(u16::from(multiply_u8(destination[2], inverse_alpha)))
        .min(255) as u8;
    destination[3] = u16::from(a)
        .saturating_add(u16::from(multiply_u8(destination[3], inverse_alpha)))
        .min(255) as u8;
}

fn unpremultiply_rgba(rgba: &mut [u8]) {
    for pixel in rgba.chunks_exact_mut(4) {
        let alpha = u32::from(pixel[3]);
        if alpha == 0 {
            pixel[0] = 0;
            pixel[1] = 0;
            pixel[2] = 0;
            continue;
        }
        for channel in &mut pixel[..3] {
            *channel = ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
        }
    }
}

fn screenshot_path(capture: &CapturedComposition) -> String {
    if let Some(path) = capture.path_override.as_ref() {
        return path.clone();
    }
    let wall = capture.unix_seconds.unwrap_or(0);
    match capture.scope {
        CaptureScope::Composition => format!(
            "{}/ui4-{}-{}-{:06}.png",
            SCREENSHOT_DIRECTORY, wall, capture.monotonic_ms, capture.sequence
        ),
        CaptureScope::Window { id, plane_slot } => format!(
            "{}/ui4-frame-w{}-slot{}-{}-{}-{:06}.png",
            SCREENSHOT_DIRECTORY,
            id.raw(),
            plane_slot,
            wall,
            capture.monotonic_ms,
            capture.sequence,
        ),
    }
}

/// Prefer the normal primary root, but do not let a later-mounted read-only
/// image hide an earlier writable TRUEOSFS disk from screenshot persistence.
fn writable_capture_root_handle() -> Option<crate::disc::block::DeviceHandle> {
    if let Some(disk) = crate::r::fs::trueosfs::primary_root_handle()
        && !disk.info().is_read_only()
    {
        return Some(disk);
    }
    crate::r::fs::trueosfs::list_roots()
        .into_iter()
        .filter_map(|root| crate::disc::block::device_handle(root.disk_id))
        .find(|disk| !disk.info().is_read_only())
}

#[embassy_executor::task(pool_size = 1)]
pub(crate) async fn ui4_screenshot_service_task() {
    crate::log_info!(target: "ui4/screenshot";
        "ui4/screenshot: service online trigger=F1/topmost-window-below-cursor+mouse-buttons-4-or-5/composition+opt-in-session-close capture=next-composed-or-coherent-final-frame format=png-rgba destination=trueosfs:/screenshots|/finalframes worker=background final_frame_overwrite=1\n"
    );
    let mut root_wait_logged = false;
    loop {
        let capture = CAPTURE_QUEUE.lock().pop_front();
        let Some(capture) = capture else {
            Timer::after(Duration::from_millis(SAVE_IDLE_PERIOD_MS)).await;
            continue;
        };
        let Some(disk) = writable_capture_root_handle() else {
            if !root_wait_logged {
                crate::log_warn!(target: "ui4/screenshot";
                    "ui4/screenshot: save waiting sequence={} reason=no-writable-trueosfs-root mounted_roots={} retry_ms={}\n",
                    capture.sequence,
                    crate::r::fs::trueosfs::roots_len(),
                    ROOT_RETRY_PERIOD_MS,
                );
                root_wait_logged = true;
            }
            let mut queue = CAPTURE_QUEUE.lock();
            if queue.len() < MAX_CAPTURE_QUEUE {
                queue.push_front(capture);
            } else {
                release_interactive_capture_gate(&capture);
                crate::log_warn!(target: "ui4/screenshot";
                    "ui4/screenshot: capture dropped sequence={} reason=no-root-and-queue-full\n",
                    capture.sequence,
                );
            }
            drop(queue);
            Timer::after(Duration::from_millis(ROOT_RETRY_PERIOD_MS)).await;
            continue;
        };
        root_wait_logged = false;
        let disk_id = disk.id().raw();
        let path = screenshot_path(&capture);
        let encode_started_ns = crate::chronos::monotonic_nanos();
        let stride = capture.width as usize * 4;
        let png = match crate::graphics::encoder::png::encode_rgba8_png(
            capture.width,
            capture.height,
            capture.rgba.as_slice(),
            stride,
        ) {
            Ok(png) => png,
            Err(error) => {
                release_interactive_capture_gate(&capture);
                crate::log_warn!(target: "ui4/screenshot";
                    "ui4/screenshot: PNG encode failed sequence={} error={:?} size={}x{}\n",
                    capture.sequence,
                    error,
                    capture.width,
                    capture.height,
                );
                continue;
            }
        };
        let encoded_ns = crate::chronos::monotonic_nanos();
        match crate::r::fs::trueosfs::file_in_async(disk, path.as_str(), png.as_slice()).await {
            Ok(true) => crate::log_info!(target: "ui4/screenshot";
                "ui4/screenshot: saved path=trueosfs:/{} disk_id={} sequence={} format=png-rgba size={}x{} png_bytes={} encode_us={} write_us={}\n",
                path,
                disk_id,
                capture.sequence,
                capture.width,
                capture.height,
                png.len(),
                encoded_ns.saturating_sub(encode_started_ns) / 1_000,
                crate::chronos::monotonic_nanos().saturating_sub(encoded_ns) / 1_000,
            ),
            Ok(false) => crate::log_warn!(target: "ui4/screenshot";
                "ui4/screenshot: save failed path=trueosfs:/{} disk_id={} sequence={} reason=no-space-or-root-placement\n",
                path,
                disk_id,
                capture.sequence,
            ),
            Err(error) => crate::log_warn!(target: "ui4/screenshot";
                "ui4/screenshot: save failed path=trueosfs:/{} disk_id={} sequence={} error={:?}\n",
                path,
                disk_id,
                capture.sequence,
                error,
            ),
        }
        release_interactive_capture_gate(&capture);
    }
}

fn release_interactive_capture_gate(capture: &CapturedComposition) {
    if capture.release_interactive_gate {
        CAPTURE_IN_FLIGHT.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        COMPACT_WINDOW_BACKGROUND, COMPACT_WINDOW_OBSERVATION_MAX_PNG_BYTES, compact_dimensions,
        encode_compact_window_png, final_frame_path, overlay_normalized_grid,
        resample_premultiplied_rgba_to_rgb, sanitize_final_frame_identity,
    };

    #[test]
    fn final_frame_identity_is_stable_and_path_safe() {
        assert_eq!(sanitize_final_frame_identity("apps/Solara Demo.bp"), "solara-demo");
        assert_eq!(sanitize_final_frame_identity("../../.bp"), "ui4-app");
    }

    #[test]
    fn one_window_overwrites_the_app_name_and_multi_window_uses_stable_slots() {
        assert_eq!(final_frame_path("solara", 0, 1), "finalframes/solara.png");
        assert_eq!(
            final_frame_path("dummy-kernel-app-1", 1, 3),
            "finalframes/dummy-kernel-app-1-window-02.png"
        );
    }

    #[test]
    fn compact_dimensions_preserve_aspect_ratio_and_never_empty_an_axis() {
        assert_eq!(compact_dimensions(768, 512, 512).unwrap(), (512, 341));
        assert_eq!(compact_dimensions(1, 4_096, 128).unwrap(), (1, 128));
        assert!(compact_dimensions(0, 512, 512).is_err());
    }

    #[test]
    fn compact_rgb_composites_transparency_before_encoding() {
        let rgb = resample_premultiplied_rgba_to_rgb(1, 1, &[0, 0, 0, 0], 1, 1).unwrap();
        assert_eq!(rgb.as_slice(), COMPACT_WINDOW_BACKGROUND.as_slice());
    }

    #[test]
    fn normalized_grid_reaches_both_edges_and_major_coordinates() {
        let mut rgb = alloc::vec![64u8; 101 * 101 * 3];
        overlay_normalized_grid(rgb.as_mut_slice(), 101, 101).unwrap();
        let pixel = |x: usize, y: usize| &rgb[(y * 101 + x) * 3..(y * 101 + x) * 3 + 3];
        assert_ne!(pixel(0, 50), &[64, 64, 64]);
        assert_ne!(pixel(10, 50), &[64, 64, 64]);
        assert_ne!(pixel(100, 50), &[64, 64, 64]);
        assert_eq!(pixel(5, 5), &[64, 64, 64]);
    }

    #[test]
    fn incompressible_capture_is_adapted_below_the_hard_png_limit() {
        let mut rgba = alloc::vec![0u8; 512 * 512 * 4];
        let mut state = 0x1234_5678u32;
        for pixel in rgba.chunks_exact_mut(4) {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            pixel[0] = state as u8;
            pixel[1] = (state >> 8) as u8;
            pixel[2] = (state >> 16) as u8;
            pixel[3] = u8::MAX;
        }
        let (width, height, png) = encode_compact_window_png(512, 512, rgba.as_slice()).unwrap();
        assert!(width < 512 && height < 512);
        assert!(png.len() <= COMPACT_WINDOW_OBSERVATION_MAX_PNG_BYTES);
        assert_eq!(&png[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
    }
}
