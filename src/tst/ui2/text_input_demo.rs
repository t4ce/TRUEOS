extern crate alloc;

use alloc::{string::String, vec, vec::Vec};

use crate::r::ui2::{self, Ui2FontTier, Ui2HostedInteractiveRect, Ui2Rect};

const UI2_TEXT_INPUT_TEX_ID: u32 = crate::tst::ui2::ids::Ui2DemoTexId::TextInput.get();
const UI2_TEXT_INPUT_CONTENT_ID: u32 = crate::tst::ui2::ids::Ui2DemoContentId::TextInput.get();
const UI2_TEXT_INPUT_RT_W: u32 = 260;
const UI2_TEXT_INPUT_FONT_TIER: Ui2FontTier = Ui2FontTier::OneX;
const UI2_TEXT_INPUT_PAD_X: usize = 5;
const UI2_TEXT_INPUT_PAD_Y: usize = 5;
const UI2_TEXT_INPUT_MAX_CHARS: usize = 64;
const UI2_TEXT_INPUT_ITEM_FIELD: u32 = 1;
const UI2_TEXT_INPUT_WINDOW_X: f32 = 220.0;
const UI2_TEXT_INPUT_WINDOW_Y: f32 = 160.0;
const UI2_TEXT_INPUT_WINDOW_Z: i16 = 30;
const UI2_TEXT_INPUT_BG_RGBA: [u8; 4] = [0x10, 0x14, 0x1A, 0xFF];
const UI2_TEXT_INPUT_FIELD_RGBA: [u8; 4] = [0x18, 0x1F, 0x2A, 0xFF];
const UI2_TEXT_INPUT_BORDER_RGBA: [u8; 4] = [0x48, 0x5A, 0x70, 0xFF];
const UI2_TEXT_INPUT_FOCUS_BORDER_RGBA: [u8; 4] = [0x74, 0xC7, 0xFF, 0xFF];
const UI2_TEXT_INPUT_TEXT_RGBA: [u8; 4] = [0xF1, 0xF4, 0xF8, 0xFF];
const UI2_TEXT_INPUT_CURSOR_RGBA: [u8; 4] = [0xF8, 0xFB, 0xFF, 0xFF];
const UI2_TEXT_INPUT_TEXT: &str = "Hello";

#[derive(Clone, Copy, Debug)]
struct DirtyRect {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

fn ui2_text_input_rt_h() -> u32 {
    u32::from(ui2::ui2_font_native_line_height_px(UI2_TEXT_INPUT_FONT_TIER))
        .saturating_add((UI2_TEXT_INPUT_PAD_Y as u32).saturating_mul(2))
        .max(1)
}

fn fill_rect_rgba(
    dst: &mut [u8],
    dst_width: usize,
    dst_height: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    rgba: [u8; 4],
) {
    let end_y = y.saturating_add(h).min(dst_height);
    let end_x = x.saturating_add(w).min(dst_width);
    for row in y.min(dst_height)..end_y {
        for col in x.min(dst_width)..end_x {
            let idx = (row * dst_width + col) * 4;
            dst[idx] = rgba[0];
            dst[idx + 1] = rgba[1];
            dst[idx + 2] = rgba[2];
            dst[idx + 3] = rgba[3];
        }
    }
}

fn stroke_rect_rgba(
    dst: &mut [u8],
    dst_width: usize,
    dst_height: usize,
    rect: Ui2Rect,
    rgba: [u8; 4],
) {
    let x = rect.x.max(0.0) as usize;
    let y = rect.y.max(0.0) as usize;
    let w = rect.w.max(0.0) as usize;
    let h = rect.h.max(0.0) as usize;
    if w == 0 || h == 0 {
        return;
    }
    fill_rect_rgba(dst, dst_width, dst_height, x, y, w, 1, rgba);
    fill_rect_rgba(
        dst,
        dst_width,
        dst_height,
        x,
        y.saturating_add(h.saturating_sub(1)),
        w,
        1,
        rgba,
    );
    fill_rect_rgba(dst, dst_width, dst_height, x, y, 1, h, rgba);
    fill_rect_rgba(
        dst,
        dst_width,
        dst_height,
        x.saturating_add(w.saturating_sub(1)),
        y,
        1,
        h,
        rgba,
    );
}

fn char_count(text: &str) -> usize {
    text.chars().count()
}

fn byte_index_for_char(text: &str, char_idx: usize) -> usize {
    text.char_indices()
        .nth(char_idx)
        .map(|(idx, _)| idx)
        .unwrap_or(text.len())
}

fn prefix_width_px(text: &str, char_idx: usize) -> usize {
    text.chars()
        .take(char_idx)
        .map(|ch| usize::from(ui2::ui2_font_char_advance_px(UI2_TEXT_INPUT_FONT_TIER, ch).max(1)))
        .sum()
}

fn cursor_index_for_x(text: &str, x: f32) -> usize {
    let local_x = (x.max(0.0) as usize).saturating_sub(UI2_TEXT_INPUT_PAD_X);
    let mut pen_x = 0usize;
    for (idx, ch) in text.chars().enumerate() {
        let advance =
            usize::from(ui2::ui2_font_char_advance_px(UI2_TEXT_INPUT_FONT_TIER, ch).max(1));
        if local_x < pen_x.saturating_add(advance / 2) {
            return idx;
        }
        pen_x = pen_x.saturating_add(advance);
    }
    char_count(text)
}

fn insert_text(text: &mut String, cursor_idx: &mut usize, input: &str) -> bool {
    let mut changed = false;
    for ch in input.chars() {
        if ch.is_control() || char_count(text) >= UI2_TEXT_INPUT_MAX_CHARS {
            continue;
        }
        let byte_idx = byte_index_for_char(text, *cursor_idx);
        text.insert(byte_idx, ch);
        *cursor_idx = cursor_idx.saturating_add(1);
        changed = true;
    }
    changed
}

fn handle_keyboard_event(
    text: &mut String,
    cursor_idx: &mut usize,
    event: crate::r::keyboard::TrueosKeyboardOutputEvent,
) -> bool {
    match event.kind {
        crate::r::keyboard::KEYBOARD_OUTPUT_KIND_TEXT => {
            let utf8_len = (event.utf8_len as usize).min(event.utf8.len());
            if utf8_len != 0 {
                if let Ok(input) = core::str::from_utf8(&event.utf8[..utf8_len]) {
                    return insert_text(text, cursor_idx, input);
                }
            }
            if let Some(ch) = char::from_u32(event.codepoint) {
                let mut input = String::new();
                input.push(ch);
                return insert_text(text, cursor_idx, input.as_str());
            }
            false
        }
        crate::r::keyboard::KEYBOARD_OUTPUT_KIND_KEY => match event.key_code {
            crate::r::keyboard::KEYBOARD_KEY_BACKSPACE => {
                if *cursor_idx == 0 {
                    return false;
                }
                let start = byte_index_for_char(text, cursor_idx.saturating_sub(1));
                let end = byte_index_for_char(text, *cursor_idx);
                text.replace_range(start..end, "");
                *cursor_idx = cursor_idx.saturating_sub(1);
                true
            }
            crate::r::keyboard::KEYBOARD_KEY_DELETE => {
                if *cursor_idx >= char_count(text) {
                    return false;
                }
                let start = byte_index_for_char(text, *cursor_idx);
                let end = byte_index_for_char(text, cursor_idx.saturating_add(1));
                text.replace_range(start..end, "");
                true
            }
            crate::r::keyboard::KEYBOARD_KEY_ARROW_LEFT => {
                let next = cursor_idx.saturating_sub(1);
                let changed = next != *cursor_idx;
                *cursor_idx = next;
                changed
            }
            crate::r::keyboard::KEYBOARD_KEY_ARROW_RIGHT => {
                let next = cursor_idx.saturating_add(1).min(char_count(text));
                let changed = next != *cursor_idx;
                *cursor_idx = next;
                changed
            }
            crate::r::keyboard::KEYBOARD_KEY_HOME => {
                let changed = *cursor_idx != 0;
                *cursor_idx = 0;
                changed
            }
            crate::r::keyboard::KEYBOARD_KEY_END => {
                let next = char_count(text);
                let changed = next != *cursor_idx;
                *cursor_idx = next;
                changed
            }
            _ => false,
        },
        _ => false,
    }
}

fn diff_dirty_rect_rgba(prev: &[u8], next: &[u8], width: u32, height: u32) -> Option<DirtyRect> {
    if width == 0 || height == 0 || prev.len() != next.len() {
        return Some(DirtyRect {
            x: 0,
            y: 0,
            w: width.max(1),
            h: height.max(1),
        });
    }

    let width_usize = width as usize;
    let height_usize = height as usize;
    let mut min_x = width_usize;
    let mut min_y = height_usize;
    let mut max_x = 0usize;
    let mut max_y = 0usize;
    let mut dirty = false;

    for y in 0..height_usize {
        for x in 0..width_usize {
            let idx = (y * width_usize + x) * 4;
            if prev.get(idx..idx + 4) != next.get(idx..idx + 4) {
                dirty = true;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }

    dirty.then_some(DirtyRect {
        x: min_x as u32,
        y: min_y as u32,
        w: max_x.saturating_sub(min_x).saturating_add(1) as u32,
        h: max_y.saturating_sub(min_y).saturating_add(1) as u32,
    })
}

fn copy_rect_rgba(pixels: &[u8], width: u32, rect: DirtyRect) -> Vec<u8> {
    let width_usize = width as usize;
    let x = rect.x as usize;
    let y = rect.y as usize;
    let w = rect.w as usize;
    let h = rect.h as usize;
    let mut region = Vec::with_capacity(w.saturating_mul(h).saturating_mul(4));
    for row in y..y.saturating_add(h) {
        let start = (row.saturating_mul(width_usize).saturating_add(x)).saturating_mul(4);
        let end = start.saturating_add(w.saturating_mul(4));
        if let Some(slice) = pixels.get(start..end) {
            region.extend_from_slice(slice);
        }
    }
    region
}

fn render_text_input(
    atlases: &ui2::Ui2FontCpuAtlases,
    text: &str,
    cursor_idx: usize,
    focused: bool,
) -> Vec<u8> {
    let width = UI2_TEXT_INPUT_RT_W as usize;
    let height = ui2_text_input_rt_h() as usize;
    let mut pixels = vec![0u8; width * height * 4];
    for px in pixels.chunks_exact_mut(4) {
        px.copy_from_slice(UI2_TEXT_INPUT_BG_RGBA.as_slice());
    }

    let field_rect = Ui2Rect {
        x: 0.0,
        y: 0.0,
        w: width as f32,
        h: height as f32,
    };
    fill_rect_rgba(&mut pixels, width, height, 0, 0, width, height, UI2_TEXT_INPUT_FIELD_RGBA);
    stroke_rect_rgba(
        &mut pixels,
        width,
        height,
        field_rect,
        if focused {
            UI2_TEXT_INPUT_FOCUS_BORDER_RGBA
        } else {
            UI2_TEXT_INPUT_BORDER_RGBA
        },
    );

    let text_max_w = width.saturating_sub(UI2_TEXT_INPUT_PAD_X * 2);
    let _ = ui2::ui2_font_blit_text_rgba(
        &mut pixels,
        width,
        height,
        atlases,
        UI2_TEXT_INPUT_FONT_TIER,
        UI2_TEXT_INPUT_PAD_X,
        UI2_TEXT_INPUT_PAD_Y,
        text_max_w,
        text,
        UI2_TEXT_INPUT_TEXT_RGBA,
    );

    if focused {
        let cursor_x =
            UI2_TEXT_INPUT_PAD_X.saturating_add(prefix_width_px(text, cursor_idx).min(text_max_w));
        fill_rect_rgba(
            &mut pixels,
            width,
            height,
            cursor_x.min(width.saturating_sub(1)),
            UI2_TEXT_INPUT_PAD_Y,
            1,
            usize::from(ui2::ui2_font_native_line_height_px(UI2_TEXT_INPUT_FONT_TIER)),
            UI2_TEXT_INPUT_CURSOR_RGBA,
        );
    }

    pixels
}

#[embassy_executor::task]
pub async fn ui2_text_input_demo_task() {
    let _task_guard = crate::r::spawn_service::task_run_guard("ui2-text-input-demo");
    let height = ui2_text_input_rt_h();
    let Some(surface) = crate::r::ui2::Ui2SurfaceWindow::new(
        "Text Input",
        crate::r::ui2::Ui2Rect {
            x: UI2_TEXT_INPUT_WINDOW_X,
            y: UI2_TEXT_INPUT_WINDOW_Y,
            w: UI2_TEXT_INPUT_RT_W as f32,
            h: height as f32,
        },
        UI2_TEXT_INPUT_WINDOW_Z,
        128,
        UI2_TEXT_INPUT_TEX_ID,
        false,
        UI2_TEXT_INPUT_BG_RGBA,
    ) else {
        return;
    };
    let _ = surface.bind_spawn_task("ui2-text-input-demo");
    let window_id = surface.window_id();
    let _ = crate::r::ui2::set_window_left_scrollbar_visible(window_id, false);
    let _ = crate::r::ui2::set_window_bottom_scrollbar_visible(window_id, false);
    let Some(atlases) = ui2::ui2_font_decode_cpu_atlases(UI2_TEXT_INPUT_FONT_TIER.size_case())
    else {
        crate::log!("ui2-text-input-demo: lucida 1x atlas decode failed\n");
        return;
    };

    let _ =
        surface.bind_hosted_scroll_state(UI2_TEXT_INPUT_CONTENT_ID, UI2_TEXT_INPUT_RT_W, height);
    let _ = surface.set_interactives(&[Ui2HostedInteractiveRect {
        item_id: UI2_TEXT_INPUT_ITEM_FIELD,
        x: 0,
        y: 0,
        width: UI2_TEXT_INPUT_RT_W,
        height,
    }]);

    crate::log!(
        "ui2-text-input-demo: window={} tex={} size={}x{} text={}\n",
        window_id,
        surface.tex_id(),
        UI2_TEXT_INPUT_RT_W,
        height,
        UI2_TEXT_INPUT_TEXT
    );

    let mut text = String::from(UI2_TEXT_INPUT_TEXT);
    let mut cursor_idx = char_count(text.as_str());
    let mut focused = false;
    let mut keyboard_read_seq = 0u64;
    let mut last_click_seq = 0u32;
    let mut needs_render = true;
    let mut rendered_pixels: Option<Vec<u8>> = None;
    let mut raw_events = [crate::r::keyboard::TrueosKeyboardOutputEvent::default(); 16];

    loop {
        if crate::r::spawn_service::task_stop_requested("ui2-text-input-demo") {
            break;
        }

        if let Some((seq, item_id, cursor_slot)) =
            crate::r::ui2::take_window_last_clicked_item_with_cursor(window_id)
        {
            if seq != last_click_seq {
                last_click_seq = seq;
                focused = item_id == UI2_TEXT_INPUT_ITEM_FIELD;
                if focused {
                    if let Some(cursor) = crate::r::ui2::window_content_cursor_positions(window_id)
                        .into_iter()
                        .find(|cursor| cursor.slot_id == cursor_slot)
                    {
                        cursor_idx = cursor_index_for_x(text.as_str(), cursor.x);
                    }
                }
                needs_render = true;
            }
        }

        loop {
            let (next_seq, _dropped, wrote) =
                crate::r::keyboard::read_output_events_since(keyboard_read_seq, &mut raw_events);
            if wrote == 0 {
                break;
            }
            keyboard_read_seq = next_seq;
            if focused {
                for event in raw_events.iter().take(wrote).copied() {
                    needs_render |= handle_keyboard_event(&mut text, &mut cursor_idx, event);
                }
            }
        }

        if needs_render {
            cursor_idx = cursor_idx.min(char_count(text.as_str()));
            let next_pixels = render_text_input(&atlases, text.as_str(), cursor_idx, focused);
            let uploaded = if let Some(prev_pixels) = rendered_pixels.as_ref() {
                if let Some(dirty) = diff_dirty_rect_rgba(
                    prev_pixels.as_slice(),
                    next_pixels.as_slice(),
                    UI2_TEXT_INPUT_RT_W,
                    height,
                ) {
                    let region = copy_rect_rgba(next_pixels.as_slice(), UI2_TEXT_INPUT_RT_W, dirty);
                    surface.upload_rgba_region(
                        dirty.x,
                        dirty.y,
                        dirty.w,
                        dirty.h,
                        region.as_slice(),
                        "ui2-text-input-demo-dirty",
                    )
                } else {
                    true
                }
            } else {
                surface.upload_rgba(next_pixels.as_slice(), "ui2-text-input-demo")
            };
            if !uploaded {
                crate::log!("ui2-text-input-demo: upload failed tex={}\n", surface.tex_id());
                break;
            }
            rendered_pixels = Some(next_pixels);
            needs_render = false;
        }

        if crate::r::spawn_service::wait_task_or_timeout_ms("ui2-text-input-demo", 40).await {
            break;
        }
    }
}
