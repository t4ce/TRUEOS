use alloc::{vec, vec::Vec};

use embassy_time::Instant;

use crate::r::ui2::{self, Ui2FontTier, Ui2Rect};

const UI2_SHELL_TEX_ID: u32 = crate::tst_ui2_ids::Ui2DemoTexId::Shell.get();
const UI2_SHELL_CONTENT_ID: u32 = crate::tst_ui2_ids::Ui2DemoContentId::Shell.get();
const UI2_SHELL_VIEW_W: u32 = 600;
const UI2_SHELL_VIEW_H: u32 = 400;
const UI2_SHELL_WINDOW_X: f32 = 300.0;
const UI2_SHELL_WINDOW_Y: f32 = 140.0;
const UI2_SHELL_WINDOW_Z: i16 = 31;
const UI2_SHELL_WINDOW_ALPHA: u8 = 0xFF;
const UI2_SHELL_BG_RGBA: [u8; 4] = [0x0C, 0x10, 0x16, 0xFF];
const UI2_SHELL_CURSOR_RGBA: [u8; 4] = [0xF1, 0xF4, 0xF8, 0xFF];
const UI2_SHELL_CURSOR_CH: char = '▏';
const UI2_SHELL_CURSOR_BLINK_MS: u64 = 1_000;
const UI2_SHELL_SELECTION_BG_RGBA: [u8; 4] = [0x1E, 0x5A, 0x96, 0xFF];
const UI2_SHELL_SELECTION_FG_RGBA: [u8; 4] = [0xF8, 0xFB, 0xFF, 0xFF];
const UI2_SHELL_PRIMARY_BUTTON_MASK: u32 = 1;
const UI2_SHELL_BASE_TEXT_COLS: usize = 100;
const UI2_SHELL_BASE_TEXT_ROWS: usize = 12;
const UI2_SHELL_FONT_TIER: Ui2FontTier = Ui2FontTier::Third;
const UI2_SHELL_FONT_SIZE_CASE: usize = UI2_SHELL_FONT_TIER.size_case();

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Ui2ShellSelectionCell {
    row: usize,
    col: usize,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Ui2ShellSelectionState {
    anchor: Option<Ui2ShellSelectionCell>,
    focus: Option<Ui2ShellSelectionCell>,
    drag_slot_id: u32,
    drag_active: bool,
}

fn ui2_shell_surface_width() -> u32 {
    UI2_SHELL_VIEW_W
}

fn ui2_shell_surface_height() -> u32 {
    UI2_SHELL_VIEW_H
}

fn ui2_shell_layout_for_viewport(viewport_w: u32, viewport_h: u32) -> (usize, usize) {
    let cols = ((viewport_w as usize).saturating_mul(UI2_SHELL_BASE_TEXT_COLS))
        .checked_div(UI2_SHELL_VIEW_W as usize)
        .unwrap_or(UI2_SHELL_BASE_TEXT_COLS)
        .max(1);
    let rows = (viewport_h as usize)
        .checked_div(ui2_shell_line_height() as usize)
        .unwrap_or(UI2_SHELL_BASE_TEXT_ROWS)
        .max(1);
    (cols, rows)
}

fn ui2_shell_line_height() -> u32 {
    u32::from(ui2::ui2_font_native_line_height_px(UI2_SHELL_FONT_TIER))
}

fn ui2_shell_cell_advance_px(ch: char) -> usize {
    usize::from(ui2::ui2_font_char_advance_px(UI2_SHELL_FONT_TIER, ch).max(1))
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

fn cell_bg_rgba(cell: &crate::shell2::Ui2ShellCell) -> [u8; 4] {
    [cell.bg.0, cell.bg.1, cell.bg.2, 0xFF]
}

fn cell_fg_rgba(cell: &crate::shell2::Ui2ShellCell) -> [u8; 4] {
    [cell.fg.0, cell.fg.1, cell.fg.2, 0xFF]
}

fn selection_linear_index(cell: Ui2ShellSelectionCell, cols: usize) -> usize {
    cell.row.saturating_mul(cols).saturating_add(cell.col)
}

fn selection_bounds(
    selection: &Ui2ShellSelectionState,
    cols: usize,
) -> Option<(Ui2ShellSelectionCell, Ui2ShellSelectionCell)> {
    let anchor = selection.anchor?;
    let focus = selection.focus?;
    let anchor_idx = selection_linear_index(anchor, cols);
    let focus_idx = selection_linear_index(focus, cols);
    if anchor_idx == focus_idx {
        return None;
    }
    if anchor_idx < focus_idx {
        Some((anchor, focus))
    } else {
        Some((focus, anchor))
    }
}

fn selection_contains(
    selection: &Ui2ShellSelectionState,
    row: usize,
    col: usize,
    cols: usize,
) -> bool {
    let Some((start, end)) = selection_bounds(selection, cols) else {
        return false;
    };
    let idx = row.saturating_mul(cols).saturating_add(col);
    idx >= selection_linear_index(start, cols) && idx <= selection_linear_index(end, cols)
}

fn effective_cell_for_render(
    cell: crate::shell2::Ui2ShellCell,
    selection: &Ui2ShellSelectionState,
    row: usize,
    col: usize,
    cols: usize,
) -> crate::shell2::Ui2ShellCell {
    if !selection_contains(selection, row, col, cols) {
        return cell;
    }
    crate::shell2::Ui2ShellCell {
        ch: cell.ch,
        fg: (
            UI2_SHELL_SELECTION_FG_RGBA[0],
            UI2_SHELL_SELECTION_FG_RGBA[1],
            UI2_SHELL_SELECTION_FG_RGBA[2],
        ),
        bg: (
            UI2_SHELL_SELECTION_BG_RGBA[0],
            UI2_SHELL_SELECTION_BG_RGBA[1],
            UI2_SHELL_SELECTION_BG_RGBA[2],
        ),
    }
}

fn snapshot_cell(
    snapshot: &crate::shell2::Ui2ShellScreenSnapshot,
    row: usize,
    col: usize,
) -> crate::shell2::Ui2ShellCell {
    let cols = (snapshot.cols as usize).max(1);
    snapshot
        .cells
        .get(row.saturating_mul(cols).saturating_add(col))
        .copied()
        .unwrap_or(crate::shell2::Ui2ShellCell {
            ch: ' ',
            fg: (0xF1, 0xF4, 0xF8),
            bg: (0x0C, 0x10, 0x16),
        })
}

fn ui2_shell_position_to_cell(
    snapshot: &crate::shell2::Ui2ShellScreenSnapshot,
    x: f32,
    y: f32,
) -> Ui2ShellSelectionCell {
    let cols = (snapshot.cols as usize).max(1);
    let rows = (snapshot.rows as usize).max(1);
    let line_h = ui2_shell_line_height().max(1) as f32;
    let mut row = if y <= 0.0 {
        0
    } else {
        libm::floorf(y / line_h) as usize
    };
    row = row.min(rows.saturating_sub(1));

    let mut pen_x = 0usize;
    for col in 0..cols {
        let cell = snapshot_cell(snapshot, row, col);
        let advance_px = ui2_shell_cell_advance_px(cell.ch).max(1);
        if x < (pen_x.saturating_add(advance_px)) as f32 || col + 1 == cols {
            return Ui2ShellSelectionCell { row, col };
        }
        pen_x = pen_x.saturating_add(advance_px);
    }

    Ui2ShellSelectionCell {
        row,
        col: cols.saturating_sub(1),
    }
}

fn window_cursor_buttons(slot_id: u32) -> u32 {
    crate::r::cursor::ordered_cursor_snapshot_with_slot_buttons()
        .into_iter()
        .find(|(cursor_slot_id, _, _, _)| *cursor_slot_id == slot_id)
        .map(|(_, _, _, buttons_down)| buttons_down)
        .unwrap_or(0)
}

fn update_selection_from_mouse(
    selection: &mut Ui2ShellSelectionState,
    window_id: u32,
    snapshot: &crate::shell2::Ui2ShellScreenSnapshot,
) -> bool {
    let cursors = crate::r::ui2::window_content_cursor_positions(window_id);
    if selection.drag_active {
        let buttons_down = window_cursor_buttons(selection.drag_slot_id);
        let mut changed = false;
        if let Some(cursor) = cursors
            .iter()
            .find(|cursor| cursor.slot_id == selection.drag_slot_id)
        {
            let next_focus = ui2_shell_position_to_cell(snapshot, cursor.x, cursor.y);
            if selection.focus != Some(next_focus) {
                selection.focus = Some(next_focus);
                changed = true;
            }
        }
        if (buttons_down & UI2_SHELL_PRIMARY_BUTTON_MASK) == 0 {
            selection.drag_active = false;
            selection.drag_slot_id = 0;
            if selection.anchor == selection.focus && selection.anchor.is_some() {
                selection.anchor = None;
                selection.focus = None;
                changed = true;
            }
        }
        return changed;
    }

    for cursor in &cursors {
        let buttons_down = window_cursor_buttons(cursor.slot_id);
        if (buttons_down & UI2_SHELL_PRIMARY_BUTTON_MASK) == 0 {
            continue;
        }
        let anchor = ui2_shell_position_to_cell(snapshot, cursor.x, cursor.y);
        *selection = Ui2ShellSelectionState {
            anchor: Some(anchor),
            focus: Some(anchor),
            drag_slot_id: cursor.slot_id,
            drag_active: true,
        };
        return true;
    }

    false
}

fn render_cell_glyph(
    rgba: &mut [u8],
    dst_width: usize,
    dst_height: usize,
    atlases: &ui2::Ui2FontCpuAtlases,
    pen_x: usize,
    row_y: usize,
    advance_px: usize,
    cell: &crate::shell2::Ui2ShellCell,
) {
    let _ = ui2::ui2_font_blit_char_rgba(
        rgba,
        dst_width,
        dst_height,
        atlases,
        UI2_SHELL_FONT_TIER,
        cell.ch,
        Ui2Rect {
            x: pen_x as f32,
            y: row_y as f32,
            w: advance_px as f32,
            h: ui2_shell_line_height() as f32,
        },
        cell_fg_rgba(cell),
    );
}

fn render_cursor(
    rgba: &mut [u8],
    dst_width: usize,
    dst_height: usize,
    atlases: &ui2::Ui2FontCpuAtlases,
    snapshot: &crate::shell2::Ui2ShellScreenSnapshot,
    selection: &Ui2ShellSelectionState,
    blink_on: bool,
) {
    if !snapshot.cursor_visible || !blink_on {
        return;
    }
    let cols = (snapshot.cols as usize).max(1);
    let cursor_col = (snapshot.cursor_col as usize).min(cols.saturating_sub(1));
    let cursor_row = (snapshot.cursor_row as usize).min((snapshot.rows as usize).saturating_sub(1));
    let mut x = 0usize;
    for col in 0..cursor_col {
        let ch = snapshot_cell(snapshot, cursor_row, col).ch;
        x = x.saturating_add(ui2_shell_cell_advance_px(ch));
    }
    let cursor_w = ui2_shell_cell_advance_px(snapshot_cell(snapshot, cursor_row, cursor_col).ch);
    let row_y = cursor_row.saturating_mul(ui2_shell_line_height() as usize);
    let cell = effective_cell_for_render(
        snapshot_cell(snapshot, cursor_row, cursor_col),
        selection,
        cursor_row,
        cursor_col,
        cols,
    );
    fill_rect_rgba(
        rgba,
        dst_width,
        dst_height,
        x,
        row_y,
        cursor_w.max(1),
        ui2_shell_line_height() as usize,
        cell_bg_rgba(&cell),
    );
    let cursor_cell = crate::shell2::Ui2ShellCell {
        ch: UI2_SHELL_CURSOR_CH,
        fg: (UI2_SHELL_CURSOR_RGBA[0], UI2_SHELL_CURSOR_RGBA[1], UI2_SHELL_CURSOR_RGBA[2]),
        bg: cell.bg,
    };
    render_cell_glyph(
        rgba,
        dst_width,
        dst_height,
        atlases,
        x,
        row_y,
        cursor_w.max(1),
        &cursor_cell,
    );
}

fn render_shell_snapshot_rgba(
    atlases: &ui2::Ui2FontCpuAtlases,
    snapshot: &crate::shell2::Ui2ShellScreenSnapshot,
    selection: &Ui2ShellSelectionState,
    blink_on: bool,
    content_w: u32,
    content_h: u32,
) -> Vec<u8> {
    let content_w = content_w as usize;
    let content_h = content_h as usize;
    let cols = (snapshot.cols as usize).max(1);
    let rows = (snapshot.rows as usize).max(1);
    let mut rgba = vec![0u8; content_w.saturating_mul(content_h).saturating_mul(4)];

    fill_rect_rgba(
        rgba.as_mut_slice(),
        content_w,
        content_h,
        0,
        0,
        content_w,
        content_h,
        UI2_SHELL_BG_RGBA,
    );

    for row in 0..rows {
        let row_y = row.saturating_mul(ui2_shell_line_height() as usize);
        if row_y >= content_h {
            break;
        }
        let mut pen_x = 0usize;
        for col in 0..cols {
            let cell = effective_cell_for_render(
                snapshot_cell(snapshot, row, col),
                selection,
                row,
                col,
                cols,
            );
            let advance_px = ui2_shell_cell_advance_px(cell.ch);
            fill_rect_rgba(
                rgba.as_mut_slice(),
                content_w,
                content_h,
                pen_x,
                row_y,
                advance_px,
                ui2_shell_line_height() as usize,
                cell_bg_rgba(&cell),
            );
            if cell.ch != ' ' {
                render_cell_glyph(
                    rgba.as_mut_slice(),
                    content_w,
                    content_h,
                    atlases,
                    pen_x,
                    row_y,
                    advance_px,
                    &cell,
                );
            }
            pen_x = pen_x.saturating_add(advance_px);
            if pen_x >= content_w {
                break;
            }
        }
    }

    render_cursor(
        rgba.as_mut_slice(),
        content_w,
        content_h,
        atlases,
        snapshot,
        selection,
        blink_on,
    );
    rgba
}

#[embassy_executor::task]
pub async fn ui2_shell_demo_task() {
    let _task_guard = crate::r::spawn_service::task_run_guard("ui2-shell-demo");
    let Some(atlases) = ui2::ui2_font_decode_cpu_atlases(UI2_SHELL_FONT_SIZE_CASE) else {
        return;
    };

    let content_w = ui2_shell_surface_width();
    let content_h = ui2_shell_surface_height();
    let Some(surface) = crate::r::ui2::Ui2SurfaceWindow::get_or_create_for_hosted_content_with_size(
        "Shell",
        crate::r::ui2::Ui2Rect {
            x: UI2_SHELL_WINDOW_X,
            y: UI2_SHELL_WINDOW_Y,
            w: UI2_SHELL_VIEW_W as f32,
            h: UI2_SHELL_VIEW_H as f32,
        },
        UI2_SHELL_WINDOW_Z,
        UI2_SHELL_WINDOW_ALPHA,
        UI2_SHELL_CONTENT_ID,
        UI2_SHELL_TEX_ID,
        true,
        content_w,
        content_h,
    ) else {
        crate::log!("ui2-shell-demo: window creation failed tex={}\n", UI2_SHELL_TEX_ID);
        return;
    };
    let _ = surface.bind_spawn_task("ui2-shell-demo");
    let _ = crate::r::ui2::set_window_title_twemoji(surface.window_id(), '\u{1F40C}');

    let clear_rgba = vec![
        0u8;
        (content_w as usize)
            .saturating_mul(content_h as usize)
            .saturating_mul(4)
    ];
    let mut clear_rgba = clear_rgba;
    for idx in (0..clear_rgba.len()).step_by(4) {
        clear_rgba[idx] = UI2_SHELL_BG_RGBA[0];
        clear_rgba[idx + 1] = UI2_SHELL_BG_RGBA[1];
        clear_rgba[idx + 2] = UI2_SHELL_BG_RGBA[2];
        clear_rgba[idx + 3] = UI2_SHELL_BG_RGBA[3];
    }
    if !surface.upload_rgba_owned(clear_rgba, "ui2-shell-demo-clear") {
        crate::log!("ui2-shell-demo: initial texture upload failed tex={}\n", UI2_SHELL_TEX_ID);
        return;
    }
    if !crate::r::ui2::maximize_window(surface.window_id()) {
        crate::log!("ui2-shell-demo: maximize failed window={}\n", surface.window_id());
    }
    if !crate::r::ui2::focus_window(surface.window_id()) {
        crate::log!("ui2-shell-demo: front-push failed window={}\n", surface.window_id());
    }

    crate::log!(
        "ui2-shell-demo: window={} tex={} viewport={}x{} content={}x{} cols={} rows={}\n",
        surface.window_id(),
        surface.tex_id(),
        UI2_SHELL_VIEW_W,
        UI2_SHELL_VIEW_H,
        content_w,
        content_h,
        UI2_SHELL_BASE_TEXT_COLS,
        UI2_SHELL_BASE_TEXT_ROWS
    );

    let mut last_rendered_seq = 0u32;
    let mut last_blink_on = false;
    let mut selection = Ui2ShellSelectionState::default();
    let mut last_viewport = (0u32, 0u32);
    let mut attached_layout = None;
    loop {
        if crate::r::spawn_service::task_stop_requested("ui2-shell-demo") {
            break;
        }
        let blink_on =
            ((Instant::now().as_millis() as u64) / (UI2_SHELL_CURSOR_BLINK_MS / 2)) % 2 == 0;
        let viewport = crate::r::ui2::window_content_rect_by_id(surface.window_id())
            .map(|rect| (rect.w.max(1.0) as u32, rect.h.max(1.0) as u32))
            .unwrap_or((content_w, content_h));
        let mut viewport_needs_present = false;
        if viewport != last_viewport {
            last_viewport = viewport;
            viewport_needs_present = true;
            let _ = surface.bind_hosted_scroll_state(UI2_SHELL_CONTENT_ID, viewport.0, viewport.1);
        }
        let layout = ui2_shell_layout_for_viewport(viewport.0, viewport.1);
        if attached_layout != Some(layout) {
            let (cols, rows) = layout;
            crate::shell2::ui2_shell_attach_window(surface.window_id(), cols, rows);
            attached_layout = Some(layout);
            last_rendered_seq = 0;
            last_blink_on = false;
            selection = Ui2ShellSelectionState::default();
            viewport_needs_present = true;
        }
        if let Some((dirty_seq, snapshot)) = crate::shell2::ui2_shell_snapshot(surface.window_id())
        {
            let selection_changed =
                update_selection_from_mouse(&mut selection, surface.window_id(), &snapshot);
            if dirty_seq != 0
                && (viewport_needs_present
                    || ((dirty_seq != last_rendered_seq
                        && dirty_seq != crate::shell2::ui2_shell_last_rendered_seq())
                        || blink_on != last_blink_on)
                    || selection_changed)
            {
                let rgba = render_shell_snapshot_rgba(
                    &atlases,
                    &snapshot,
                    &selection,
                    blink_on,
                    last_viewport.0.max(1),
                    last_viewport.1.max(1),
                );
                if crate::r::io::cabi::queue_texture_rgba_image_upload_copy(
                    surface.tex_id(),
                    last_viewport.0.max(1),
                    last_viewport.1.max(1),
                    rgba.as_slice(),
                    surface.window_id(),
                    "ui2-shell-demo-present",
                ) {
                    if dirty_seq != last_rendered_seq {
                        crate::shell2::ui2_shell_mark_rendered(dirty_seq);
                    }
                    last_rendered_seq = dirty_seq;
                    last_blink_on = blink_on;
                }
            }
        }
        if crate::r::spawn_service::wait_task_or_timeout_ms("ui2-shell-demo", 33).await {
            break;
        }
    }
}
