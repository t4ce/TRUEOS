extern crate alloc;

use alloc::vec::Vec;

use crate::r::ui2::{self, Ui2FontTier, Ui2HostedInteractiveRect, Ui2Rect, Ui2WindowResizeMode};

const UI2_CURSORPICKER_TEX_ID: u32 = crate::tst::ui2::ids::Ui2DemoTexId::CursorPicker.get();
const UI2_CURSORPICKER_CONTENT_ID: u32 = crate::tst::ui2::ids::Ui2DemoContentId::CursorPicker.get();
const UI2_CURSORPICKER_WINDOW_TITLE: &str = "Cursor Picker";
const UI2_CURSORPICKER_TITLE_ICON: char = '🦋';
const UI2_CURSORPICKER_VIEW_W: u32 = 360;
const UI2_CURSORPICKER_VIEW_H: u32 = 240;
const UI2_CURSORPICKER_WINDOW_X: f32 = 520.0;
const UI2_CURSORPICKER_WINDOW_Y: f32 = 120.0;
const UI2_CURSORPICKER_WINDOW_Z: i16 = 42;
const UI2_CURSORPICKER_WINDOW_ALPHA: u8 = 0xFF;
const UI2_CURSORPICKER_FONT_TIER: Ui2FontTier = Ui2FontTier::OneX;
const UI2_CURSORPICKER_FONT_SIZE_CASE: usize = UI2_CURSORPICKER_FONT_TIER.size_case();
const UI2_CURSORPICKER_BG_RGBA: [u8; 4] = [0x0E, 0x13, 0x18, 0xFF];
const UI2_CURSORPICKER_ICON_RGBA: [u8; 4] = [0xFF, 0xFF, 0xFF, 0xFF];
const UI2_CURSORPICKER_PAD: u32 = 8;
const UI2_CURSORPICKER_CELL: u32 = 64;

fn render_scene(
    viewport_w: u32,
    viewport_h: u32,
    atlases: &ui2::Ui2FontCpuAtlases,
) -> (Vec<u8>, Vec<Ui2HostedInteractiveRect>) {
    let width = viewport_w.max(1) as usize;
    let height = viewport_h.max(1) as usize;
    let mut pixels = alloc::vec![0u8; width * height * 4];
    for px in pixels.chunks_exact_mut(4) {
        px.copy_from_slice(UI2_CURSORPICKER_BG_RGBA.as_slice());
    }

    let choices = ui2::cursor_spirit_choices();
    let available_w = viewport_w.saturating_sub(UI2_CURSORPICKER_PAD * 2);
    let cols = (available_w / UI2_CURSORPICKER_CELL).max(1);
    let mut interactives = Vec::with_capacity(choices.len());

    for (idx, ch) in choices.iter().enumerate() {
        let col = (idx as u32) % cols;
        let row = (idx as u32) / cols;
        let x = UI2_CURSORPICKER_PAD + col * UI2_CURSORPICKER_CELL;
        let y = UI2_CURSORPICKER_PAD + row * UI2_CURSORPICKER_CELL;
        if y.saturating_add(UI2_CURSORPICKER_CELL) > viewport_h.saturating_sub(4) {
            break;
        }
        let rect = Ui2Rect {
            x: x as f32,
            y: y as f32,
            w: UI2_CURSORPICKER_CELL as f32,
            h: UI2_CURSORPICKER_CELL as f32,
        };
        let _ = ui2::ui2_font_blit_char_rgba(
            pixels.as_mut_slice(),
            width,
            height,
            atlases,
            UI2_CURSORPICKER_FONT_TIER,
            *ch,
            rect,
            UI2_CURSORPICKER_ICON_RGBA,
        );
        interactives.push(Ui2HostedInteractiveRect {
            item_id: (idx as u32).saturating_add(1),
            x,
            y,
            width: UI2_CURSORPICKER_CELL,
            height: UI2_CURSORPICKER_CELL,
        });
    }

    (pixels, interactives)
}

fn glyph_for_item(item_id: u32) -> Option<char> {
    let idx = usize::try_from(item_id.checked_sub(1)?).ok()?;
    ui2::cursor_spirit_choices().get(idx).copied()
}

#[embassy_executor::task]
pub async fn ui2_cursorpicker_demo_task() {
    let _task_guard = crate::r::spawn_service::task_run_guard("ui2-cursorpicker-demo");
    let Some(atlases) = ui2::ui2_font_decode_cpu_atlases(UI2_CURSORPICKER_FONT_SIZE_CASE) else {
        return;
    };

    let Some(surface) = ui2::Ui2SurfaceWindow::get_or_create_for_hosted_content_with_size(
        UI2_CURSORPICKER_WINDOW_TITLE,
        Ui2Rect {
            x: UI2_CURSORPICKER_WINDOW_X,
            y: UI2_CURSORPICKER_WINDOW_Y,
            w: UI2_CURSORPICKER_VIEW_W as f32,
            h: UI2_CURSORPICKER_VIEW_H as f32,
        },
        UI2_CURSORPICKER_WINDOW_Z,
        UI2_CURSORPICKER_WINDOW_ALPHA,
        UI2_CURSORPICKER_CONTENT_ID,
        UI2_CURSORPICKER_TEX_ID,
        true,
        UI2_CURSORPICKER_VIEW_W,
        UI2_CURSORPICKER_VIEW_H,
    ) else {
        return;
    };

    let _ = surface.bind_spawn_task("ui2-cursorpicker-demo");
    let _ = ui2::set_window_title(surface.window_id(), UI2_CURSORPICKER_WINDOW_TITLE);
    let _ = ui2::set_window_title_twemoji(surface.window_id(), UI2_CURSORPICKER_TITLE_ICON);
    let _ = ui2::set_window_decorations(surface.window_id(), ui2::Ui2WindowDecorationMode::System);
    let _ = ui2::set_window_bottom_bar_visible(surface.window_id(), true);
    let _ = ui2::set_window_left_scrollbar_visible(surface.window_id(), false);
    let _ = ui2::set_window_bottom_scrollbar_visible(surface.window_id(), false);
    let _ = ui2::set_window_resize_mode(surface.window_id(), Ui2WindowResizeMode::PreviewCommit);

    let mut last_viewport = (0u32, 0u32);
    let mut last_click_seq = 0u32;
    let mut needs_render = true;

    loop {
        if crate::r::spawn_service::task_stop_requested("ui2-cursorpicker-demo") {
            break;
        }

        let viewport = crate::r::ui2::window_content_rect_by_id(surface.window_id())
            .map(|rect| (rect.w.max(1.0) as u32, rect.h.max(1.0) as u32))
            .unwrap_or((UI2_CURSORPICKER_VIEW_W, UI2_CURSORPICKER_VIEW_H));
        if viewport != last_viewport {
            last_viewport = viewport;
            needs_render = true;
        }

        if let Some((seq, item_id, cursor_slot)) =
            crate::r::ui2::take_window_last_clicked_item_with_cursor(surface.window_id())
            && seq != last_click_seq
        {
            last_click_seq = seq;
            if let Some(glyph) = glyph_for_item(item_id) {
                let _ = ui2::set_cursor_spirit_glyph(cursor_slot, glyph);
            }
        }

        if needs_render {
            let (pixels, interactives) = render_scene(last_viewport.0, last_viewport.1, &atlases);
            let _ = surface.bind_hosted_scroll_state(
                UI2_CURSORPICKER_CONTENT_ID,
                last_viewport.0.max(1),
                last_viewport.1.max(1),
            );
            let _ = surface.set_interactives(interactives.as_slice());
            if !crate::r::io::cabi::queue_texture_rgba_image_upload_owned(
                surface.tex_id(),
                last_viewport.0.max(1),
                last_viewport.1.max(1),
                pixels,
                surface.window_id(),
                "ui2-cursorpicker-demo-present",
            ) {
                break;
            }
            needs_render = false;
        }

        if crate::r::spawn_service::wait_task_or_timeout_ms("ui2-cursorpicker-demo", 50).await {
            break;
        }
    }
}
