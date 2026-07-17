//! UI4 composition screenshots.
//!
//! Input only arms a request. The compositor consumes one request after a
//! successful frame, copies the exact leased UI4 buffers into a transparent
//! RGBA image, and queues that immutable image for the filesystem worker.
//! Encoding and TRUEOSFS I/O therefore never run in the composition loop.

use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use embassy_time::{Duration, Timer};
use spin::Mutex;

use super::{
    FramePoolError, FrameReadLease, FrameRgbaView, WindowSnapshot, acquire_published_frame,
    published_rgba_view, release_published_frame,
};

const MAX_PENDING_REQUESTS: u32 = 4;
const MAX_CAPTURE_QUEUE: usize = 1;
const SAVE_IDLE_PERIOD_MS: u64 = 25;
const ROOT_RETRY_PERIOD_MS: u64 = 500;
const SCREENSHOT_DIRECTORY: &str = "screenshots";

static PENDING_REQUESTS: AtomicU32 = AtomicU32::new(0);
static CAPTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static CAPTURE_IN_FLIGHT: AtomicBool = AtomicBool::new(false);
static CAPTURE_QUEUE: Mutex<VecDeque<CapturedComposition>> = Mutex::new(VecDeque::new());

enum CaptureError {
    NoScanout,
    DimensionTooLarge,
    Frame(FramePoolError),
}

impl fmt::Debug for CaptureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoScanout => formatter.write_str("NoScanout"),
            Self::DimensionTooLarge => formatter.write_str("DimensionTooLarge"),
            Self::Frame(error) => formatter.debug_tuple("Frame").field(error).finish(),
        }
    }
}

impl From<FramePoolError> for CaptureError {
    fn from(error: FramePoolError) -> Self {
        Self::Frame(error)
    }
}

struct CapturedComposition {
    sequence: u64,
    unix_seconds: Option<u64>,
    monotonic_ms: u64,
    width: u32,
    height: u32,
    /// Straight-alpha RGBA8, ready for PNG encoding.
    rgba: Vec<u8>,
}

/// Arm one composition capture. Calls are bounded so a burst of side-button
/// transitions cannot retain an unbounded number of full-screen images.
pub(super) fn request_capture(mouse_button: u8) {
    let mut pending = PENDING_REQUESTS.load(Ordering::Acquire);
    loop {
        if pending >= MAX_PENDING_REQUESTS {
            crate::log_warn!(target: "ui4/screenshot";
                "ui4/screenshot: request dropped trigger=mouse-button-{} reason=request-queue-full pending={}\n",
                mouse_button,
                pending,
            );
            return;
        }
        match PENDING_REQUESTS.compare_exchange_weak(
            pending,
            pending + 1,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => {
                crate::log_info!(target: "ui4/screenshot";
                    "ui4/screenshot: request armed trigger=mouse-button-{} pending={} capture=next-composed-frame\n",
                    mouse_button,
                    pending + 1,
                );
                return;
            }
            Err(observed) => pending = observed,
        }
    }
}

/// Capture at most one request from this compositor frame.
///
/// `windows` is the immutable broker snapshot used for the frame and `rects`
/// contains UI4's software-only visual plane. The hardware mouse cursor is
/// intentionally absent, matching normal screenshot behavior.
pub(super) fn capture_compositor_frame(
    windows: &[WindowSnapshot],
    rects: &[crate::intel::LiveOverlayRect],
) {
    if PENDING_REQUESTS.load(Ordering::Acquire) == 0 {
        return;
    }
    if CAPTURE_IN_FLIGHT
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    if !take_capture_request() {
        CAPTURE_IN_FLIGHT.store(false, Ordering::Release);
        return;
    }

    let started_ns = crate::chronos::monotonic_nanos();
    match capture_windows(windows, rects) {
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
            crate::log_info!(target: "ui4/screenshot";
                "ui4/screenshot: frame captured sequence={} size={}x{} rgba_bytes={} windows={} visuals={} copy_us={} alpha=straight-transparent-background\n",
                sequence,
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

fn take_capture_request() -> bool {
    let mut pending = PENDING_REQUESTS.load(Ordering::Acquire);
    loop {
        if pending == 0 {
            return false;
        }
        match PENDING_REQUESTS.compare_exchange_weak(
            pending,
            pending - 1,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return true,
            Err(observed) => pending = observed,
        }
    }
}

fn capture_windows(
    windows: &[WindowSnapshot],
    rects: &[crate::intel::LiveOverlayRect],
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
        for (window, view) in ordered_windows.iter().zip(views.iter()) {
            crate::intel::dma_flush(view.virt, view.byte_len);
            let pixels =
                unsafe { core::slice::from_raw_parts(view.virt.cast_const(), view.byte_len) };
            blend_window(&mut rgba, stride, width, height, *window, *view, pixels);
        }
        for rect in rects {
            blend_visual_rect(&mut rgba, stride, width, height, *rect);
        }
        unpremultiply_rgba(&mut rgba);
        Ok::<_, CaptureError>(rgba)
    })();
    release_leases(&leases);
    let rgba = result?;

    Ok(CapturedComposition {
        sequence: CAPTURE_SEQUENCE.fetch_add(1, Ordering::AcqRel) + 1,
        unix_seconds: crate::chronos::best_effort_unix_time_seconds(),
        monotonic_ms: crate::chronos::monotonic_nanos() / 1_000_000,
        width,
        height,
        rgba,
    })
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
        for (src, dst) in source_row
            .chunks_exact(4)
            .zip(destination_row.chunks_exact_mut(4))
        {
            let scale = |value: u8| multiply_u8(value, opacity);
            blend_premultiplied(dst, scale(src[0]), scale(src[1]), scale(src[2]), scale(src[3]));
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
    let wall = capture.unix_seconds.unwrap_or(0);
    format!(
        "{}/ui4-{}-{}-{:06}.png",
        SCREENSHOT_DIRECTORY, wall, capture.monotonic_ms, capture.sequence
    )
}

/// Prefer the normal primary root, but do not let a later-mounted read-only
/// image hide an earlier writable TRUEOSFS disk from screenshot persistence.
fn writable_screenshot_root_handle() -> Option<crate::disc::block::DeviceHandle> {
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
        "ui4/screenshot: service online trigger=mouse-buttons-4-or-5 capture=next-composed-frame format=png-rgba destination=trueosfs:/screenshots worker=background\n"
    );
    let mut root_wait_logged = false;
    loop {
        let capture = CAPTURE_QUEUE.lock().pop_front();
        let Some(capture) = capture else {
            Timer::after(Duration::from_millis(SAVE_IDLE_PERIOD_MS)).await;
            continue;
        };
        let Some(disk) = writable_screenshot_root_handle() else {
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
                CAPTURE_IN_FLIGHT.store(false, Ordering::Release);
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
                CAPTURE_IN_FLIGHT.store(false, Ordering::Release);
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
        CAPTURE_IN_FLIGHT.store(false, Ordering::Release);
    }
}
