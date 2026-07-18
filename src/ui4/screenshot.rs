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

static CAPTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static CAPTURE_IN_FLIGHT: AtomicBool = AtomicBool::new(false);
static CAPTURE_REQUESTS: Mutex<VecDeque<CaptureRequest>> = Mutex::new(VecDeque::new());
static CAPTURE_QUEUE: Mutex<VecDeque<CapturedComposition>> = Mutex::new(VecDeque::new());

enum CaptureError {
    NoScanout,
    DimensionTooLarge,
    InvalidFrameLayout,
    WindowUnavailable(super::WindowId),
    Frame(FramePoolError),
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
    /// Straight-alpha RGBA8, ready for PNG encoding.
    rgba: Vec<u8>,
    scope: CaptureScope,
    path_override: Option<String>,
    release_interactive_gate: bool,
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
        CaptureSelection::Composition => capture_windows(windows, &rects),
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
        scope: CaptureScope::Composition,
        path_override: None,
        release_interactive_gate: true,
    })
}

fn capture_window(window: WindowSnapshot) -> Result<CapturedComposition, CaptureError> {
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
            let source_row = source
                .get(source_offset..source_offset + row_bytes)
                .ok_or(CaptureError::InvalidFrameLayout)?;
            rgba[destination_offset..destination_offset + row_bytes].copy_from_slice(source_row);
        }
        unpremultiply_rgba(&mut rgba);
        Ok::<_, CaptureError>((view.width, view.height, rgba))
    })();
    let _ = release_published_frame(lease);
    let (width, height, rgba) = result?;

    Ok(CapturedComposition {
        sequence: CAPTURE_SEQUENCE.fetch_add(1, Ordering::AcqRel) + 1,
        unix_seconds: crate::chronos::best_effort_unix_time_seconds(),
        monotonic_ms: crate::chronos::monotonic_nanos() / 1_000_000,
        width,
        height,
        rgba,
        scope: CaptureScope::Window {
            id: window.id,
            plane_slot: window.plane.slot(),
        },
        path_override: None,
        release_interactive_gate: true,
    })
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
        .trim_end_matches(".bp")
        .trim_end_matches(".vm");
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
    use super::{final_frame_path, sanitize_final_frame_identity};

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
}
