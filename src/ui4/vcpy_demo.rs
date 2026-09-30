//! A service-owned MicroFont source copied into a broker-owned UI4 frame by BCS0.

extern crate alloc;

use alloc::vec;
use spin::Mutex;

use super::{
    DamageRect, FrameBuffering, FrameCadence, FrameContent, FrameHandle, FrameRgbaView,
    FrameSpec, FrameWriteLease, OutputId, PremultipliedRgba8, ScanoutFormat, WindowCreate,
    WindowInteraction, WindowOwner, WindowPlacement, WindowPlane, WindowSessionCloseRequest,
    WindowSessionId, acquire_frame_buffer, begin_window_session, cancel_frame_buffer,
    create_frame, create_window, destroy_frame, finish_window_session,
    finish_window_session_with_request, publish_frame_buffer, publish_window_frame,
    writable_rgba_view,
};

const OWNER: WindowOwner = WindowOwner::VCPY_SERVICE;
const WIDTH: u32 = 512;
const ROW_HEIGHT: u32 = microfont::FHEIGHT as u32;
const ROWS: u32 = 20;
const HEIGHT: u32 = ROW_HEIGHT * ROWS;
const BACKGROUND: [u8; 4] = [14, 21, 34, 255];
const FOREGROUND: [u8; 4] = [110, 235, 185, 255];
const LABEL: &str = "MICROFONT / BCS0 / FAST COPY";
const COPY_PERIOD_MS: u64 = 250;

static DEMO: Mutex<Option<Demo>> = Mutex::new(None);

struct Pending {
    submission: crate::intel::GucBcs0CopySubmission,
    lease: FrameWriteLease,
    view: FrameRgbaView,
    rows: u32,
}

struct Demo {
    session: WindowSessionId,
    source: FrameHandle,
    source_lease: FrameWriteLease,
    source_view: FrameRgbaView,
    frame: FrameHandle,
    window: super::WindowId,
    next_rows: u32,
    published_rows: u32,
    next_submit_ms: u64,
    pending: Option<Pending>,
    poisoned: Option<FrameWriteLease>,
    stopping: bool,
}

pub(crate) fn tick() -> Result<(), &'static str> {
    let mut guard = DEMO.lock();
    if guard.is_none() {
        *guard = Some(open()?);
    }
    let demo = guard.as_mut().ok_or("demo-not-open")?;
    if demo.poisoned.is_some() {
        return Ok(());
    }
    if let Some(pending) = demo.pending.take() {
        match crate::r::services::vcpy_service::poll_rgba_copies(pending.submission) {
            crate::intel::GucBcs0CopyCompletion::Pending => {
                demo.pending = Some(pending);
                return Ok(());
            }
            crate::intel::GucBcs0CopyCompletion::Complete => {
                if pending.rows == 0 {
                    cancel_frame_buffer(pending.lease).map_err(|_| "marker-lease-cancel")?;
                    demo.next_rows = 1;
                    crate::log_info!(target: "gfx";
                        "vcpy: marker-only retired=1 context_saved=1 next=xy-fast-copy-blt cadence_ms=250\n");
                    return Ok(());
                }
                // The hardware release is retired. Invalidate CPU aliases before
                // UI4's ordinary frame reader can inspect the copied pixels.
                crate::intel::dma_cache_flush_range(pending.view.virt, pending.view.byte_len);
                publish_frame_buffer(pending.lease).map_err(|_| "frame-publish")?;
                publish_window_frame(OWNER, demo.window, DamageRect::FULL)
                    .map_err(|_| "window-publish")?;
                super::input_broker::notify_slot4_visual_change();
                crate::log_info!(target: "gfx";
                    "vcpy: completed rows={} bytes={} frame={} source=service-microfont engine=bcs0 command=xy-fast-copy-blt\n",
                    pending.rows, pending.rows as usize * ROW_HEIGHT as usize * WIDTH as usize * 4,
                    demo.frame.raw(),
                );
                demo.next_rows = if pending.rows == ROWS { 1 } else { pending.rows + 1 };
                demo.published_rows = pending.rows;
            }
            _ => {
                // An ambiguous failure may still have a GPU writer. Retain the
                // lease, source and frame until the lane is proven retired.
                demo.poisoned = Some(pending.lease);
                return Err("copy-completion-failed; allocation-pinned");
            }
        }
    }
    if demo.stopping {
        return Ok(());
    }
    let now_ms = trueos_time::Instant::now().as_millis();
    if now_ms < demo.next_submit_ms {
        return Ok(());
    }
    let lease = acquire_frame_buffer(demo.frame).map_err(|_| "destination-busy")?;
    let view = match writable_rgba_view(lease) {
        Ok(view) => view,
        Err(_) => {
            let _ = cancel_frame_buffer(lease);
            return Err("destination-view");
        }
    };
    // The display's inactive back buffer may contain the previous completed
    // cycle. Clear it while leased, then let BCS provide every visible glyph.
    fill(view, BACKGROUND);
    crate::intel::dma_cache_flush_range(view.virt, view.byte_len);
    let rows = demo.next_rows;
    let copy = crate::intel::GucBcs0RgbaCopy {
        source: surface(demo.source_view),
        source_x: 0,
        source_y: 0,
        destination_x: 0,
        destination_y: 0,
        width: WIDTH,
        height: rows * ROW_HEIGHT,
    };
    let queued = if rows == 0 {
        crate::r::services::vcpy_service::queue_marker(surface(view))
    } else {
        crate::r::services::vcpy_service::queue_rgba_copies(surface(view), &[copy])
    };
    match queued {
        Ok(submission) => {
            demo.pending = Some(Pending { submission, lease, view, rows });
            demo.next_submit_ms = now_ms.saturating_add(COPY_PERIOD_MS);
            Ok(())
        }
        Err(error) => {
            match error {
                crate::intel::GucBcs0CopySubmitError::SubmitFailed => {
                    demo.poisoned = Some(lease);
                    Err("ambiguous-submit; allocation-pinned")
                }
                crate::intel::GucBcs0CopySubmitError::Busy => {
                    let _ = cancel_frame_buffer(lease);
                    Ok(())
                }
                _ => {
                    let _ = cancel_frame_buffer(lease);
                    Err("fast-copy-submit")
                }
            }
        }
    }
}

#[derive(Copy, Clone)]
pub(crate) struct Status {
    pub(crate) published_rows: u32,
    pub(crate) marker_retired: bool,
    pub(crate) pending: bool,
    pub(crate) pinned: bool,
}

pub(crate) fn status() -> Status {
    let guard = DEMO.lock();
    match guard.as_ref() {
        Some(demo) => Status {
            published_rows: demo.published_rows,
            marker_retired: demo.next_rows != 0,
            pending: demo.pending.is_some(),
            pinned: demo.poisoned.is_some(),
        },
        None => Status { published_rows: 0, marker_retired: false, pending: false, pinned: false },
    }
}

pub(crate) fn stop() -> bool {
    let mut guard = DEMO.lock();
    let Some(demo) = guard.as_mut() else { return true };
    demo.stopping = true;
    if demo.poisoned.is_some() {
        // Quarantined backing is permanently retained. The worker itself can
        // stop; restarting will report the pinned state without touching it.
        return true;
    }
    if demo.pending.is_some() {
        // The service worker will need to keep polling until ownership is
        // returned; an in-flight destination cannot be released here.
        return false;
    }
    let demo = guard.take().unwrap();
    let _ = cancel_frame_buffer(demo.source_lease);
    let _ = destroy_frame(demo.source);
    let _ = finish_window_session_with_request(
        OWNER,
        demo.session,
        WindowSessionCloseRequest::default().animate_and_retire_frames(),
    );
    super::input_broker::notify_slot4_visual_change();
    true
}

fn open() -> Result<Demo, &'static str> {
    let output = OutputId::from_slot(0).ok_or("output-unavailable")?;
    let (screen_width, screen_height) =
        crate::intel::active_scanout_dimensions().ok_or("scanout-unavailable")?;
    if screen_width < WIDTH || screen_height < HEIGHT {
        return Err("scanout-too-small");
    }
    let source = create_frame(FrameSpec {
        output,
        content: FrameContent::Image,
        cadence: FrameCadence::Immutable,
        buffering: FrameBuffering::Single,
        format: ScanoutFormat::Rgba8888Premultiplied,
        width: WIDTH,
        height: HEIGHT,
        base_color: None,
    }).map_err(|_| "source-create")?;
    let source_lease = match acquire_frame_buffer(source) {
        Ok(lease) => lease,
        Err(_) => { let _ = destroy_frame(source); return Err("source-acquire") }
    };
    let source_view = match writable_rgba_view(source_lease) {
        Ok(view) => view,
        Err(_) => {
            let _ = cancel_frame_buffer(source_lease);
            let _ = destroy_frame(source);
            return Err("source-view");
        }
    };
    paint_source(source_view);
    crate::intel::dma_cache_flush_range(source_view.virt, source_view.byte_len);

    let session = match begin_window_session(OWNER) {
        Ok(session) => session,
        Err(_) => { cleanup_source(source, source_lease); return Err("session-create") }
    };
    let frame = match create_frame(FrameSpec {
        output,
        content: FrameContent::CopyEngine,
        cadence: FrameCadence::Dirty,
        buffering: FrameBuffering::Double,
        format: ScanoutFormat::Rgba8888Premultiplied,
        width: WIDTH,
        height: HEIGHT,
        base_color: Some(PremultipliedRgba8::from_straight_rgba(14, 21, 34, 255)),
    }) {
        Ok(frame) => frame,
        Err(_) => {
            let _ = finish_window_session(OWNER, session);
            cleanup_source(source, source_lease);
            return Err("frame-create");
        }
    };
    let window = match create_window(WindowCreate {
        owner: OWNER,
        session,
        frame,
        output,
        plane: WindowPlane::Universal(super::ALPHA_OVERLAY_PLANE_SLOT as u8),
        placement: WindowPlacement {
            x: ((screen_width - WIDTH) / 2) as i32,
            y: ((screen_height - HEIGHT) / 2) as i32,
            width: WIDTH,
            height: HEIGHT,
            z: 90,
            opacity: u8::MAX,
            visible: true,
        },
        interaction: WindowInteraction::APPLICATION_FIXED_FRAME,
    }) {
        Ok(window) => window,
        Err(_) => {
            let _ = destroy_frame(frame);
            let _ = finish_window_session(OWNER, session);
            cleanup_source(source, source_lease);
            return Err("window-create");
        }
    };
    crate::log_info!(target: "gfx";
        "vcpy: opened window={} frame={} source={} area={}x{} cadence_ms=250\n",
        window.raw(), frame.raw(), source.raw(), WIDTH, HEIGHT,
    );
    Ok(Demo { session, source, source_lease, source_view, frame, window,
        next_rows: 0, published_rows: 0, next_submit_ms: 0, pending: None, poisoned: None,
        stopping: false })
}

fn cleanup_source(source: FrameHandle, lease: FrameWriteLease) {
    let _ = cancel_frame_buffer(lease);
    let _ = destroy_frame(source);
}

fn surface(view: FrameRgbaView) -> crate::intel::GucBcs0RgbaSurface {
    crate::intel::GucBcs0RgbaSurface {
        phys: view.phys,
        gpu: view.gpu,
        bytes: view.byte_len,
        width: view.width,
        height: view.height,
        pitch_bytes: view.pitch,
    }
}

fn fill(view: FrameRgbaView, color: [u8; 4]) {
    let pixels = unsafe { core::slice::from_raw_parts_mut(view.virt, view.byte_len) };
    for row in 0..view.height as usize {
        for col in 0..view.width as usize {
            let offset = row * view.pitch as usize + col * 4;
            pixels[offset..offset + 4].copy_from_slice(&color);
        }
    }
}

fn paint_source(view: FrameRgbaView) {
    fill(view, BACKGROUND);
    let pixels = unsafe { core::slice::from_raw_parts_mut(view.virt, view.byte_len) };
    let mut glyphs = vec![0u8; WIDTH as usize * ROW_HEIGHT as usize];
    let _ = microfont::stamp_text(&mut glyphs, WIDTH as usize, ROW_HEIGHT as usize,
        8, 0, LABEL, 1u8);
    for row in 0..ROWS as usize {
        for y in 0..ROW_HEIGHT as usize {
            for x in 0..WIDTH as usize {
                if glyphs[y * WIDTH as usize + x] == 0 { continue; }
                let offset = (row * ROW_HEIGHT as usize + y) * view.pitch as usize + x * 4;
                pixels[offset..offset + 4].copy_from_slice(&FOREGROUND);
            }
        }
    }
}
