//! Kernel-owned UI4 entry point for configuring coherent HID input groups.
//!
//! LINK deliberately starts as a lifecycle and presentation surface only. The
//! kernel already owns [`crate::usb2::hid::hut::InputCombo`], including its
//! mouse, keyboard, tablet, and gamepad binding API. Device enumeration and
//! mutations are withheld until their reviewable UI flow is ready; opening this
//! panel must never create or alter a combo implicitly.

extern crate alloc;

use alloc::vec;
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;
use trueos_time::{Duration, Timer};

use super::{
    DamageRect, FrameBuffering, FrameCadence, FrameContent, FrameHandle, FrameSpec, OutputId,
    PremultipliedRgba8, ScanoutFormat, Ui4CursorSource, WindowCreate, WindowInteraction,
    WindowOwner, WindowPlacement, WindowPlane, WindowSessionCloseRequest, WindowSessionId,
    acquire_frame_buffer, begin_window_session, create_frame, create_window, destroy_frame,
    finish_window_session, finish_window_session_with_request, publish_frame_buffer,
    publish_window_frame, register_global_keyboard_hook, unregister_global_keyboard_hook,
    writable_rgba_view,
};

const OWNER: WindowOwner = WindowOwner::LINK_SERVICE;
const SERVICE_POLL_MS: u64 = 16;
const PANEL_WIDTH: u32 = 512;
const PANEL_HEIGHT: u32 = 320;
const PANEL_MARGIN: u32 = 24;
const TEXT_SCALE: u32 = 2;
const TEXT_LEFT: u32 = 24;
const TEXT_TOP: u32 = 24;
const TEXT_LINE_HEIGHT: u32 = 28;

static OPEN_REQUEST: Mutex<Option<LinkOpenRequest>> = Mutex::new(None);
static ESCAPE_REQUESTED: AtomicBool = AtomicBool::new(false);

#[derive(Copy, Clone)]
struct LinkOpenRequest {
    source: Ui4CursorSource,
    anchor: (u32, u32),
}

struct ActiveLink {
    session: WindowSessionId,
    frame: FrameHandle,
    window: super::WindowId,
    escape_hook: super::GlobalKeyboardHookId,
}

/// Queue a LINK panel open from the desktop context menu. Multiple requests
/// coalesce, which prevents duplicate kernel-owned configuration windows.
pub(crate) fn request_open(source: Ui4CursorSource, anchor: (u32, u32)) {
    *OPEN_REQUEST.lock() = Some(LinkOpenRequest { source, anchor });
}

fn capture_escape(
    event: &crate::r::keyboard::TrueosKeyboardOutputEvent,
) -> super::GlobalKeyboardDisposition {
    if event.kind == crate::r::keyboard::KEYBOARD_OUTPUT_KIND_KEY
        && event.key_code == crate::r::keyboard::KEYBOARD_KEY_ESCAPE
    {
        ESCAPE_REQUESTED.store(true, Ordering::Release);
        super::GlobalKeyboardDisposition::Consume
    } else {
        super::GlobalKeyboardDisposition::PassThrough
    }
}

#[trueos_executor::task(pool_size = 1)]
pub(crate) async fn ui4_link_service_task() {
    crate::log_info!(target: "ui4/link";
        "ui4/link: service online carrier=bsp owner=kernel-internal lifecycle=context-menu-open+escape-close input-model=kernel-hid-input-combo device-enumeration=deferred binding-mutation=deferred\n"
    );
    let mut active = None;
    loop {
        if let Some(request) = OPEN_REQUEST.lock().take() {
            if active.is_none() {
                match open_link(request) {
                    Ok(link) => active = Some(link),
                    Err(reason) => crate::log_warn!(target: "ui4/link";
                        "ui4/link: open rejected reason={}\n", reason,
                    ),
                }
            }
        }

        if ESCAPE_REQUESTED.swap(false, Ordering::AcqRel)
            && active
                .as_ref()
                .is_some_and(|link| close_link(link, "escape"))
        {
            active = None;
        }
        Timer::after(Duration::from_millis(SERVICE_POLL_MS)).await;
    }
}

fn open_link(request: LinkOpenRequest) -> Result<ActiveLink, &'static str> {
    let output = OutputId::from_slot(0).ok_or("output-unavailable")?;
    let (screen_width, screen_height) =
        crate::intel::active_scanout_dimensions().ok_or("scanout-unavailable")?;
    if screen_width < PANEL_WIDTH || screen_height < PANEL_HEIGHT {
        return Err("panel-exceeds-scanout");
    }

    let session = begin_window_session(OWNER).map_err(|_| "session-create")?;
    let frame = match create_link_frame(output) {
        Ok(frame) => frame,
        Err(_) => {
            let _ = finish_window_session(OWNER, session);
            return Err("frame-create");
        }
    };
    let x = request
        .anchor
        .0
        .saturating_add(PANEL_MARGIN)
        .min(screen_width.saturating_sub(PANEL_WIDTH));
    let y = request
        .anchor
        .1
        .saturating_add(PANEL_MARGIN)
        .min(screen_height.saturating_sub(PANEL_HEIGHT));
    let window = match create_window(WindowCreate {
        owner: OWNER,
        session,
        frame,
        output,
        plane: WindowPlane::Interaction,
        placement: WindowPlacement {
            x: x as i32,
            y: y as i32,
            width: PANEL_WIDTH,
            height: PANEL_HEIGHT,
            z: 100,
            opacity: u8::MAX,
            visible: true,
        },
        interaction: WindowInteraction::APPLICATION_FIXED_FRAME,
    }) {
        Ok(window) => window,
        Err(_) => {
            cleanup_failed_open(session, frame);
            return Err("window-create");
        }
    };
    let escape_hook = match register_global_keyboard_hook(u8::MAX, capture_escape) {
        Ok(hook) => hook,
        Err(_) => {
            cleanup_failed_open(session, frame);
            return Err("escape-hook-register");
        }
    };
    let link = ActiveLink {
        session,
        frame,
        window,
        escape_hook,
    };
    if render_link(&link).is_err() {
        let _ = close_link(&link, "initial-render-failed");
        return Err("initial-render");
    }
    let _ = super::input_broker::select_window_for_cursor_at(
        request.source,
        OWNER,
        window,
        request.anchor.0,
        request.anchor.1,
    );
    crate::log_info!(target: "ui4/link";
        "ui4/link: opened session={} frame={} window={} panel={}x{}@{},{} controls=device-list+binding=deferred\n",
        session.raw(), frame.raw(), window.raw(), PANEL_WIDTH, PANEL_HEIGHT, x, y,
    );
    Ok(link)
}

fn create_link_frame(output: OutputId) -> Result<FrameHandle, super::FramePoolError> {
    create_frame(FrameSpec {
        output,
        content: FrameContent::Image,
        cadence: FrameCadence::Dirty,
        buffering: FrameBuffering::Double,
        format: ScanoutFormat::Rgba8888Premultiplied,
        width: PANEL_WIDTH,
        height: PANEL_HEIGHT,
        base_color: Some(PremultipliedRgba8::TRANSPARENT),
    })
}

fn cleanup_failed_open(session: WindowSessionId, frame: FrameHandle) {
    let _ = finish_window_session(OWNER, session);
    let _ = destroy_frame(frame);
}

fn close_link(link: &ActiveLink, reason: &'static str) -> bool {
    match finish_window_session_with_request(
        OWNER,
        link.session,
        WindowSessionCloseRequest::default().animate_and_retire_frames(),
    ) {
        Ok(closed) => {
            let _ = unregister_global_keyboard_hook(link.escape_hook);
            super::input_broker::notify_slot4_visual_change();
            crate::log_info!(target: "ui4/link";
                "ui4/link: closed reason={} session={} windows={} frame_retirement=ui4-owned\n",
                reason, link.session.raw(), closed,
            );
            true
        }
        Err(error) => {
            crate::log_warn!(target: "ui4/link";
                "ui4/link: close failed reason={} session={} error={:?}\n",
                reason, link.session.raw(), error,
            );
            false
        }
    }
}

fn render_link(link: &ActiveLink) -> Result<(), ()> {
    let lease = acquire_frame_buffer(link.frame).map_err(|_| ())?;
    let view = match writable_rgba_view(lease) {
        Ok(view) => view,
        Err(_) => {
            let _ = super::cancel_frame_buffer(lease);
            return Err(());
        }
    };
    let pixels = unsafe { core::slice::from_raw_parts_mut(view.virt, view.byte_len) };
    pixels.fill(0);
    fill_rect(pixels, view.pitch as usize, 0, 0, view.width, view.height, [20, 24, 33, 255]);
    draw_border(pixels, view.pitch as usize, view.width, view.height, [78, 186, 255, 255]);
    fill_rect(pixels, view.pitch as usize, 0, 62, view.width, 2, [78, 186, 255, 255]);
    draw_text(pixels, view.pitch as usize, TEXT_LEFT, TEXT_TOP, "LINK", [168, 232, 255, 255]);
    draw_text(
        pixels,
        view.pitch as usize,
        TEXT_LEFT,
        TEXT_TOP + TEXT_LINE_HEIGHT,
        "INPUT GROUPING",
        [255, 255, 255, 255],
    );
    draw_text(
        pixels,
        view.pitch as usize,
        TEXT_LEFT,
        TEXT_TOP + TEXT_LINE_HEIGHT * 3,
        "KEYBOARD + MOUSE + HID COMBOS",
        [216, 224, 236, 255],
    );
    draw_text(
        pixels,
        view.pitch as usize,
        TEXT_LEFT,
        TEXT_TOP + TEXT_LINE_HEIGHT * 4,
        "ONE HUMAN OR AI INPUT IDENTITY",
        [216, 224, 236, 255],
    );
    draw_text(
        pixels,
        view.pitch as usize,
        TEXT_LEFT,
        TEXT_TOP + TEXT_LINE_HEIGHT * 6,
        "DEVICE LIST AND BINDING CONTROLS",
        [255, 190, 64, 255],
    );
    draw_text(
        pixels,
        view.pitch as usize,
        TEXT_LEFT,
        TEXT_TOP + TEXT_LINE_HEIGHT * 7,
        "ARRIVE IN THE NEXT UI4 PASS",
        [255, 190, 64, 255],
    );
    draw_text(
        pixels,
        view.pitch as usize,
        TEXT_LEFT,
        TEXT_TOP + TEXT_LINE_HEIGHT * 9,
        "ESC CLOSES",
        [168, 232, 255, 255],
    );
    crate::intel::dma_flush(view.virt, view.byte_len);
    publish_frame_buffer(lease).map_err(|_| ())?;
    publish_window_frame(OWNER, link.window, DamageRect::FULL).map_err(|_| ())?;
    super::input_broker::notify_slot4_visual_change();
    Ok(())
}

fn fill_rect(
    pixels: &mut [u8],
    pitch: usize,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    color: [u8; 4],
) {
    for row in y..y.saturating_add(height).min(PANEL_HEIGHT) {
        for col in x..x.saturating_add(width).min(PANEL_WIDTH) {
            put_pixel(pixels, pitch, col, row, color);
        }
    }
}

fn draw_border(pixels: &mut [u8], pitch: usize, width: u32, height: u32, color: [u8; 4]) {
    if width == 0 || height == 0 {
        return;
    }
    fill_rect(pixels, pitch, 0, 0, width, 2, color);
    fill_rect(pixels, pitch, 0, height.saturating_sub(2), width, 2, color);
    fill_rect(pixels, pitch, 0, 0, 2, height, color);
    fill_rect(pixels, pitch, width.saturating_sub(2), 0, 2, height, color);
}

fn draw_text(pixels: &mut [u8], pitch: usize, x: u32, y: u32, text: &str, color: [u8; 4]) {
    let width = PANEL_WIDTH.saturating_sub(x) as usize / TEXT_SCALE as usize;
    let height = microfont::FHEIGHT;
    let Some(size) = width.checked_mul(height) else {
        return;
    };
    let mut glyphs = vec![0u8; size];
    if microfont::stamp_text(&mut glyphs, width, height, 0, 0, text, 1u8).is_err() {
        return;
    }
    for (row, scanline) in glyphs.chunks_exact(width).enumerate() {
        for (col, alpha) in scanline.iter().enumerate() {
            if *alpha != 0 {
                fill_rect(
                    pixels,
                    pitch,
                    x + col as u32 * TEXT_SCALE,
                    y + row as u32 * TEXT_SCALE,
                    TEXT_SCALE,
                    TEXT_SCALE,
                    color,
                );
            }
        }
    }
}

fn put_pixel(pixels: &mut [u8], pitch: usize, x: u32, y: u32, color: [u8; 4]) {
    let offset = y as usize * pitch + x as usize * 4;
    if let Some(pixel) = pixels.get_mut(offset..offset + 4) {
        pixel.copy_from_slice(&color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_text_rows_fit_inside_the_frame() {
        assert!(
            TEXT_TOP + TEXT_LINE_HEIGHT * 9 + microfont::FHEIGHT as u32 * TEXT_SCALE
                <= PANEL_HEIGHT
        );
    }
}
