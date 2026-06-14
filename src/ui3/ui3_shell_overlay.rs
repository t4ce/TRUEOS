use alloc::{string::String, vec::Vec};

use crate::intel::types::Rgba8;

const UI3_SHELL_WINDOW_ID: u32 = 0x5533_0001;
const UI3_SHELL_COLS: usize = 180;
const UI3_SHELL_PADDING_PX: u32 = 18;
const UI3_SHELL_MAX_ROWS: usize = 96;
const UI3_SHELL_TEXT_RGBA: u32 = 0xFFFF_FFFF;
const UI3_SHELL_BLACK_RGBA: u32 = 0xFF00_0000;

#[derive(Debug, Default)]
pub(crate) struct Ui3ShellOverlayState {
    pub(crate) active: bool,
    keyboard_read_seq: u64,
    last_rendered_seq: u32,
    attached_width: u32,
    attached_height: u32,
    attached_rows: usize,
}

impl Ui3ShellOverlayState {
    pub(crate) fn deactivate(&mut self) {
        self.active = false;
        self.last_rendered_seq = 0;
    }
}

pub(crate) fn handle_keyboard(
    state: &mut Ui3ShellOverlayState,
    viewport_width: u32,
    viewport_height: u32,
) -> bool {
    let mut out = [crate::r::keyboard::TrueosKeyboardOutputEvent::default(); 32];
    let (next_seq, dropped, wrote) =
        crate::r::keyboard::read_output_events_since(state.keyboard_read_seq, &mut out);
    state.keyboard_read_seq = next_seq;
    if dropped != 0 {
        crate::log!("ui3-shell-overlay: keyboard dropped={}\n", dropped);
    }

    let mut dirty = false;
    for event in out.iter().take(wrote).copied() {
        if is_start_key(event) {
            state.active = !state.active;
            dirty = true;
            if state.active {
                attach_shell_window(state, viewport_width, viewport_height);
            } else {
                state.last_rendered_seq = 0;
                let _ = crate::intel::present_live_overlay_rects(&[], "ui3-shell-overlay-hide");
            }
            crate::log!("ui3-shell-overlay: toggle active={}\n", state.active as u8);
            continue;
        }

        if state.active {
            attach_shell_window(state, viewport_width, viewport_height);
            if crate::shell2::queue_ui2_shell_keyboard_event(UI3_SHELL_WINDOW_ID, event) {
                dirty = true;
            }
        }
    }
    dirty
}

pub(crate) fn redraw_if_dirty(
    state: &mut Ui3ShellOverlayState,
    viewport_width: u32,
    viewport_height: u32,
    force: bool,
    reason: &str,
) -> bool {
    if !state.active {
        return false;
    }
    attach_shell_window(state, viewport_width, viewport_height);
    let Some((seq, snapshot)) = crate::shell2::ui2_shell_snapshot(UI3_SHELL_WINDOW_ID) else {
        return false;
    };
    if !force && seq == state.last_rendered_seq {
        return false;
    }
    let rect = shell_rect(viewport_width, viewport_height);
    if rect.width == 0 || rect.height == 0 {
        return false;
    }

    let _ = crate::intel::present_live_overlay_rects(&[], "ui3-shell-overlay-clear");
    let Some(dst) = crate::intel::gpgpu::ui3_overlay_rgba8_surface(rect) else {
        return false;
    };
    let fill_descs = crate::intel::gpgpu::fill_rect_worklist_rgba8(
        dst,
        crate::intel::gpgpu::GpgpuRect::new(rect.x as i32, rect.y as i32, rect.width, rect.height),
        UI3_SHELL_BLACK_RGBA,
    );

    let mut rows = snapshot_rows(&snapshot);
    trim_empty_head_to_fit(&mut rows, visible_row_cap(rect.height));
    let placements = collect_row_text_placements(rows.as_slice(), rect);
    let sprite = if placements.is_empty() {
        None
    } else {
        crate::intel::gpgpu::sprite64_worklist_surface(placements.as_slice(), dst, reason)
    };
    state.last_rendered_seq = seq;
    crate::log!(
        "ui3-shell-overlay: redraw seq={} rows={} placements={} rect={}x{}@{},{} fill_descs={} sprite_submitted={}\n",
        seq,
        rows.len(),
        placements.len(),
        rect.width,
        rect.height,
        rect.x,
        rect.y,
        fill_descs,
        sprite.as_ref().is_some_and(|result| result.submitted) as u8
    );
    true
}

fn is_start_key(event: crate::r::keyboard::TrueosKeyboardOutputEvent) -> bool {
    event.kind == crate::r::keyboard::KEYBOARD_OUTPUT_KIND_KEY
        && event.key_code == crate::r::keyboard::KEYBOARD_KEY_START
}

fn attach_shell_window(
    state: &mut Ui3ShellOverlayState,
    viewport_width: u32,
    viewport_height: u32,
) {
    let rect = shell_rect(viewport_width, viewport_height);
    let rows = visible_row_cap(rect.height).max(1);
    if state.attached_width == rect.width
        && state.attached_height == rect.height
        && state.attached_rows == rows
    {
        return;
    }
    crate::shell2::ui2_shell_attach_window(UI3_SHELL_WINDOW_ID, UI3_SHELL_COLS, rows);
    state.attached_width = rect.width;
    state.attached_height = rect.height;
    state.attached_rows = rows;
}

fn shell_rect(viewport_width: u32, viewport_height: u32) -> crate::intel::LiveOverlayRect {
    if viewport_width == 0 || viewport_height == 0 {
        return crate::intel::LiveOverlayRect::new(0, 0, 0, 0, Rgba8::new(0, 0, 0, 0));
    }
    let height = viewport_height / 2;
    let width = height
        .saturating_mul(16)
        .saturating_div(9)
        .min(viewport_width);
    let x = viewport_width.saturating_sub(width) / 2;
    let y = viewport_height.saturating_sub(height) / 2;
    crate::intel::LiveOverlayRect::new(x, y, width, height, Rgba8::new(0, 0, 0, 255))
}

fn visible_row_cap(panel_height: u32) -> usize {
    let line_height = shell_line_height();
    panel_height
        .saturating_sub(UI3_SHELL_PADDING_PX.saturating_mul(2))
        .saturating_div(line_height.max(1))
        .min(UI3_SHELL_MAX_ROWS as u32)
        .max(1) as usize
}

fn shell_line_height() -> u32 {
    let face = crate::ui3::althlasfont::bitmapfont::ATHLAS_FONT_FACE_LUCIDA_HALF;
    u32::from(crate::ui3::althlasfont::bitmapfont::athlas_font_line_height_px(face).unwrap_or(22))
}

fn snapshot_rows(snapshot: &crate::shell2::Ui2ShellScreenSnapshot) -> Vec<String> {
    let cols = snapshot.cols as usize;
    let rows = snapshot.rows as usize;
    let mut out = Vec::new();
    if cols == 0 || rows == 0 {
        return out;
    }
    for row in 0..rows {
        let start = row.saturating_mul(cols);
        let end = start.saturating_add(cols).min(snapshot.cells.len());
        if start >= end {
            break;
        }
        let mut text = String::new();
        for cell in &snapshot.cells[start..end] {
            text.push(cell.ch);
        }
        trim_string_end_spaces(&mut text);
        out.push(text);
    }
    out
}

fn trim_empty_head_to_fit(rows: &mut Vec<String>, cap: usize) {
    while rows.len() > cap {
        if rows.first().is_some_and(|row| row.is_empty()) {
            rows.remove(0);
        } else {
            break;
        }
    }
    if rows.len() > cap {
        let drop = rows.len().saturating_sub(cap);
        rows.drain(0..drop);
    }
}

fn trim_string_end_spaces(text: &mut String) {
    let trimmed_len = text.trim_end_matches(' ').len();
    text.truncate(trimmed_len);
}

fn collect_row_text_placements(
    rows: &[String],
    panel: crate::intel::LiveOverlayRect,
) -> Vec<crate::intel::gpgpu::GpgpuSprite64Placement> {
    let face = crate::ui3::althlasfont::bitmapfont::ATHLAS_FONT_FACE_LUCIDA_HALF;
    let line_height = shell_line_height() as i32;
    let content_x0 = panel.x.saturating_add(UI3_SHELL_PADDING_PX) as i32;
    let content_y0 = panel.y.saturating_add(UI3_SHELL_PADDING_PX) as i32;
    let content_x1 = panel
        .x
        .saturating_add(panel.width)
        .saturating_sub(UI3_SHELL_PADDING_PX) as i32;
    let content_y1 = panel
        .y
        .saturating_add(panel.height)
        .saturating_sub(UI3_SHELL_PADDING_PX) as i32;
    let space_advance = preserved_space_advance(face, line_height);
    let mut placements = Vec::new();
    for (row_idx, row) in rows.iter().enumerate() {
        let baseline_y = content_y0.saturating_add((row_idx as i32).saturating_mul(line_height));
        if baseline_y.saturating_add(line_height) > content_y1 {
            break;
        }
        let mut pen_x = content_x0;
        for ch in row.chars() {
            if ch.is_control() {
                continue;
            }
            if ch.is_whitespace() {
                pen_x = pen_x.saturating_add(if ch == '\t' {
                    space_advance.saturating_mul(4)
                } else {
                    space_advance
                });
                if pen_x >= content_x1 {
                    break;
                }
                continue;
            }
            let Some(region) =
                crate::ui3::althlasfont::bitmapfont::athlas_lookup_glyph_region(face, ch)
            else {
                pen_x = pen_x.saturating_add(space_advance);
                continue;
            };
            let advance = i32::from(region.src_w.max(1)).max(space_advance);
            if pen_x.saturating_add(advance) > content_x1 {
                break;
            }
            if let Some(slot) = crate::intel::gpgpu::sprite64_font_slot_for_region(face, region) {
                placements.push(crate::intel::gpgpu::GpgpuSprite64Placement::tinted_src_over(
                    slot,
                    pen_x,
                    baseline_y,
                    UI3_SHELL_TEXT_RGBA,
                ));
            }
            pen_x = pen_x.saturating_add(advance);
        }
    }
    placements
}

fn preserved_space_advance(
    face: crate::ui3::althlasfont::bitmapfont::AthlasFontFace,
    line_height: i32,
) -> i32 {
    crate::ui3::althlasfont::bitmapfont::athlas_lookup_glyph_region(face, 'M')
        .map(|region| i32::from(region.src_w.max(1)))
        .unwrap_or((line_height / 2).max(1))
}
