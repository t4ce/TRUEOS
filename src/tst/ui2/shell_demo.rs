use alloc::{vec, vec::Vec};

use embassy_time::Instant;

use crate::r::ui2::{self, Ui2FontTier, Ui2Rect, Ui2WindowResizeMode};

const UI2_SHELL_TEX_ID: u32 = crate::tst::ui2::ids::Ui2DemoTexId::Shell.get();
const UI2_SHELL_CONTENT_ID: u32 = crate::tst::ui2::ids::Ui2DemoContentId::Shell.get();
const UI2_SHELL_VIEW_W: u32 = 600;
const UI2_SHELL_VIEW_H: u32 = 400;
const UI2_SHELL_WINDOW_X: f32 = 300.0;
const UI2_SHELL_WINDOW_Y: f32 = 140.0;
const UI2_SHELL_WINDOW_Z: i16 = 31;
const UI2_SHELL_WINDOW_ALPHA: u8 = 0xFF;
const UI2_SHELL_BG_RGBA: [u8; 4] = [0x0C, 0x10, 0x16, 0xFF];
const UI2_SHELL_CURSOR_RGBA: [u8; 4] = [0xF1, 0xF4, 0xF8, 0xFF];
const UI2_SHELL_CURSOR_CH: char = '▏';
const UI2_SHELL_CURSOR_BLINK_ENABLED: bool = true;
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

fn ui2_shell_cell_width_px() -> usize {
    ui2_shell_cell_advance_px('W').max(1)
}

fn shell_cell_visual_advance_px(cell: &crate::shell2::Ui2ShellCell) -> usize {
    ui2_shell_cell_advance_px(cell.ch).max(1)
}

fn ui2_shell_layout_for_viewport(viewport_w: u32, viewport_h: u32) -> (usize, usize) {
    let cols = (viewport_w as usize)
        .checked_div(ui2_shell_cell_width_px())
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

    let mut col = 0usize;
    let mut pen_x = 0usize;
    let target_x = x.max(0.0) as usize;
    while col + 1 < cols {
        let cell = snapshot_cell(snapshot, row, col);
        let next_x = pen_x.saturating_add(shell_cell_visual_advance_px(&cell));
        if target_x < next_x {
            break;
        }
        pen_x = next_x;
        col += 1;
    }
    Ui2ShellSelectionCell { row, col }
}

fn update_selection_from_mouse(
    selection: &mut Ui2ShellSelectionState,
    window_id: u32,
    snapshot: &crate::shell2::Ui2ShellScreenSnapshot,
) -> bool {
    let mut changed = false;
    for event in crate::r::ui2::take_window_cursor_events(window_id) {
        if selection.drag_active {
            if event.slot_id != selection.drag_slot_id {
                continue;
            }
            let next_focus = ui2_shell_position_to_cell(snapshot, event.x, event.y);
            if selection.focus != Some(next_focus) {
                selection.focus = Some(next_focus);
                changed = true;
            }
            if (event.buttons_down & UI2_SHELL_PRIMARY_BUTTON_MASK) == 0 {
                selection.drag_active = false;
                selection.drag_slot_id = 0;
                if selection.anchor == selection.focus && selection.anchor.is_some() {
                    selection.anchor = None;
                    selection.focus = None;
                }
                changed = true;
            }
            continue;
        }

        if (event.buttons_down & UI2_SHELL_PRIMARY_BUTTON_MASK) == 0 {
            continue;
        }
        let anchor = ui2_shell_position_to_cell(snapshot, event.x, event.y);
        *selection = Ui2ShellSelectionState {
            anchor: Some(anchor),
            focus: Some(anchor),
            drag_slot_id: event.slot_id,
            drag_active: true,
        };
        changed = true;
    }
    changed
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
    let cell_w = ui2_shell_cell_width_px();
    let x = cursor_col.saturating_mul(cell_w);
    let cursor_w = cell_w;
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

fn row_cell_x(snapshot: &crate::shell2::Ui2ShellScreenSnapshot, row: usize, col: usize) -> usize {
    let cols = (snapshot.cols as usize).max(1);
    let mut x = 0usize;
    for c in 0..col.min(cols) {
        let cell = snapshot_cell(snapshot, row, c);
        x = x.saturating_add(shell_cell_visual_advance_px(&cell));
    }
    x
}

fn row_range_span_px(
    snapshot: &crate::shell2::Ui2ShellScreenSnapshot,
    row: usize,
    start_col: usize,
    end_col: usize,
    content_w: u32,
) -> Option<(usize, usize)> {
    let cols = (snapshot.cols as usize).max(1);
    if row >= snapshot.rows as usize || start_col >= cols {
        return None;
    }
    let start = row_cell_x(snapshot, row, start_col).min(content_w as usize);
    let mut end = start;
    let end_col = end_col.min(cols.saturating_sub(1));
    for col in start_col..=end_col {
        let cell = snapshot_cell(snapshot, row, col);
        end = end.saturating_add(shell_cell_visual_advance_px(&cell));
    }
    let end = end.min(content_w as usize);
    if end <= start {
        return None;
    }
    Some((start, end))
}

fn row_range_px(
    prev_snapshot: Option<&crate::shell2::Ui2ShellScreenSnapshot>,
    snapshot: &crate::shell2::Ui2ShellScreenSnapshot,
    row: usize,
    start_col: usize,
    end_col: usize,
    content_w: u32,
) -> Option<(u32, u32)> {
    let (mut start, mut end) = row_range_span_px(snapshot, row, start_col, end_col, content_w)?;
    if let Some(prev) = prev_snapshot {
        if let Some((prev_start, prev_end)) =
            row_range_span_px(prev, row, start_col, end_col, content_w)
        {
            start = start.min(prev_start);
            end = end.max(prev_end);
        }
    }
    if end <= start {
        return None;
    }
    Some((start as u32, (end - start) as u32))
}

fn push_dirty_range(ranges: &mut Vec<(usize, usize, usize)>, row: usize, start: usize, end: usize) {
    if start > end {
        return;
    }
    if let Some(existing) = ranges.iter_mut().find(|range| range.0 == row) {
        existing.1 = existing.1.min(start);
        existing.2 = existing.2.max(end);
        return;
    }
    ranges.push((row, start, end));
}

fn collect_dirty_row_ranges(
    prev_snapshot: Option<&crate::shell2::Ui2ShellScreenSnapshot>,
    snapshot: &crate::shell2::Ui2ShellScreenSnapshot,
    prev_selection: &Ui2ShellSelectionState,
    selection: &Ui2ShellSelectionState,
    prev_blink_on: bool,
    blink_on: bool,
) -> Option<Vec<(usize, usize, usize)>> {
    let prev = prev_snapshot?;
    if prev.cols != snapshot.cols || prev.rows != snapshot.rows {
        return None;
    }

    let cols = (snapshot.cols as usize).max(1);
    let rows = (snapshot.rows as usize).max(1);
    let mut ranges = Vec::new();

    for row in 0..rows {
        let mut first_cell_change = None;
        let mut first_deco_change = None;
        let mut last_deco_change = 0usize;
        for col in 0..cols {
            let idx = row.saturating_mul(cols).saturating_add(col);
            if prev.cells.get(idx) != snapshot.cells.get(idx) && first_cell_change.is_none() {
                first_cell_change = Some(col);
            }
            let was_selected = selection_contains(prev_selection, row, col, cols);
            let is_selected = selection_contains(selection, row, col, cols);
            if was_selected != is_selected {
                first_deco_change.get_or_insert(col);
                last_deco_change = col;
            }
        }
        if let Some(start) = first_cell_change {
            push_dirty_range(&mut ranges, row, start, cols.saturating_sub(1));
        }
        if let Some(start) = first_deco_change {
            push_dirty_range(&mut ranges, row, start, last_deco_change);
        }
    }

    if prev.cursor_visible && prev_blink_on {
        push_dirty_range(
            &mut ranges,
            prev.cursor_row as usize,
            prev.cursor_col as usize,
            prev.cursor_col as usize,
        );
    }
    if snapshot.cursor_visible && blink_on {
        push_dirty_range(
            &mut ranges,
            snapshot.cursor_row as usize,
            snapshot.cursor_col as usize,
            snapshot.cursor_col as usize,
        );
    }

    Some(ranges)
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
            let advance_px = shell_cell_visual_advance_px(&cell);
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

fn render_shell_row_range_rgba(
    atlases: &ui2::Ui2FontCpuAtlases,
    snapshot: &crate::shell2::Ui2ShellScreenSnapshot,
    selection: &Ui2ShellSelectionState,
    blink_on: bool,
    row: usize,
    start_col: usize,
    end_col: usize,
    range_x: u32,
    width: u32,
) -> Vec<u8> {
    let width_usize = width as usize;
    let height_usize = ui2_shell_line_height().max(1) as usize;
    let cols = (snapshot.cols as usize).max(1);
    let range_x = range_x as usize;
    let mut rgba = vec![0u8; width_usize.saturating_mul(height_usize).saturating_mul(4)];
    fill_rect_rgba(
        rgba.as_mut_slice(),
        width_usize,
        height_usize,
        0,
        0,
        width_usize,
        height_usize,
        UI2_SHELL_BG_RGBA,
    );

    for col in start_col..=end_col.min(cols.saturating_sub(1)) {
        let cell =
            effective_cell_for_render(snapshot_cell(snapshot, row, col), selection, row, col, cols);
        let pen_x = row_cell_x(snapshot, row, col).saturating_sub(range_x);
        let advance_px = shell_cell_visual_advance_px(&cell);
        fill_rect_rgba(
            rgba.as_mut_slice(),
            width_usize,
            height_usize,
            pen_x,
            0,
            advance_px,
            height_usize,
            cell_bg_rgba(&cell),
        );
        if cell.ch != ' ' {
            render_cell_glyph(
                rgba.as_mut_slice(),
                width_usize,
                height_usize,
                atlases,
                pen_x,
                0,
                advance_px,
                &cell,
            );
        }
    }

    let cursor_row = snapshot.cursor_row as usize;
    let cursor_col = snapshot.cursor_col as usize;
    if snapshot.cursor_visible
        && blink_on
        && cursor_row == row
        && cursor_col >= start_col
        && cursor_col <= end_col
    {
        let cell = effective_cell_for_render(
            snapshot_cell(snapshot, cursor_row, cursor_col),
            selection,
            cursor_row,
            cursor_col,
            cols,
        );
        let pen_x = row_cell_x(snapshot, row, cursor_col).saturating_sub(range_x);
        let advance_px = shell_cell_visual_advance_px(&cell);
        fill_rect_rgba(
            rgba.as_mut_slice(),
            width_usize,
            height_usize,
            pen_x,
            0,
            advance_px,
            height_usize,
            cell_bg_rgba(&cell),
        );
        let cursor_cell = crate::shell2::Ui2ShellCell {
            ch: UI2_SHELL_CURSOR_CH,
            fg: (UI2_SHELL_CURSOR_RGBA[0], UI2_SHELL_CURSOR_RGBA[1], UI2_SHELL_CURSOR_RGBA[2]),
            bg: cell.bg,
        };
        render_cell_glyph(
            rgba.as_mut_slice(),
            width_usize,
            height_usize,
            atlases,
            pen_x,
            0,
            advance_px,
            &cursor_cell,
        );
    }

    rgba
}

#[embassy_executor::task]
pub async fn ui2_shell_demo_task() {
    let _task_guard = crate::r::spawn_service::task_run_guard("ui2-shell-demo");
    let Some(atlases) = ui2::ui2_font_decode_cpu_atlases(UI2_SHELL_FONT_SIZE_CASE) else {
        return;
    };

    let initial_layout = ui2_shell_layout_for_viewport(UI2_SHELL_VIEW_W, UI2_SHELL_VIEW_H);
    let content_w = UI2_SHELL_VIEW_W;
    let content_h = UI2_SHELL_VIEW_H;
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
    let _ = crate::r::ui2::set_window_resize_mode(surface.window_id(), Ui2WindowResizeMode::Live);
    let _ = crate::r::ui2::set_window_content_preserve_scale(surface.window_id(), false);

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
        initial_layout.0,
        initial_layout.1
    );

    let mut last_rendered_seq = 0u32;
    let mut last_blink_on = false;
    let mut selection = Ui2ShellSelectionState::default();
    let mut last_selection = Ui2ShellSelectionState::default();
    let mut last_snapshot: Option<crate::shell2::Ui2ShellScreenSnapshot> = None;
    let mut last_viewport = (0u32, 0u32);
    let mut last_content_size = (0u32, 0u32);
    let mut attached_layout = None;
    let mut full_present_pending = true;
    loop {
        if crate::r::spawn_service::task_stop_requested("ui2-shell-demo") {
            break;
        }
        let blink_on = !UI2_SHELL_CURSOR_BLINK_ENABLED
            || ((Instant::now().as_millis() as u64) / (UI2_SHELL_CURSOR_BLINK_MS / 2)) % 2 == 0;
        let viewport = crate::r::ui2::window_content_rect_by_id(surface.window_id())
            .map(|rect| (rect.w.max(1.0) as u32, rect.h.max(1.0) as u32))
            .unwrap_or((content_w, content_h));
        let mut viewport_needs_present = false;
        if viewport != last_viewport {
            last_viewport = viewport;
            viewport_needs_present = true;
            full_present_pending = true;
        }
        let layout = ui2_shell_layout_for_viewport(viewport.0, viewport.1);
        let content_size = (viewport.0.max(1), viewport.1.max(1));
        if content_size != last_content_size || viewport_needs_present {
            last_content_size = content_size;
            viewport_needs_present = true;
            full_present_pending = true;
            let _ = surface.bind_hosted_scroll_state(
                UI2_SHELL_CONTENT_ID,
                content_size.0,
                content_size.1,
            );
        }
        if attached_layout != Some(layout) {
            let (cols, rows) = layout;
            crate::shell2::ui2_shell_attach_window(surface.window_id(), cols, rows);
            attached_layout = Some(layout);
            last_rendered_seq = 0;
            last_blink_on = false;
            selection = Ui2ShellSelectionState::default();
            last_selection = selection;
            last_snapshot = None;
            viewport_needs_present = true;
            full_present_pending = true;
        }
        if let Some((dirty_seq, snapshot)) = crate::shell2::ui2_shell_snapshot(surface.window_id())
        {
            let selection_changed =
                update_selection_from_mouse(&mut selection, surface.window_id(), &snapshot);
            let shell_dirty = dirty_seq != last_rendered_seq
                && dirty_seq != crate::shell2::ui2_shell_last_rendered_seq();
            let needs_full_present =
                full_present_pending || viewport_needs_present || last_snapshot.is_none();
            if needs_full_present {
                let rgba = render_shell_snapshot_rgba(
                    &atlases,
                    &snapshot,
                    &selection,
                    blink_on,
                    last_content_size.0.max(1),
                    last_content_size.1.max(1),
                );
                if crate::r::io::cabi::queue_texture_rgba_image_upload_copy(
                    surface.tex_id(),
                    last_content_size.0.max(1),
                    last_content_size.1.max(1),
                    rgba.as_slice(),
                    surface.window_id(),
                    "ui2-shell-demo-present",
                ) {
                    if dirty_seq != last_rendered_seq {
                        crate::shell2::ui2_shell_mark_rendered(dirty_seq);
                    }
                    last_rendered_seq = dirty_seq;
                    last_blink_on = blink_on;
                    last_selection = selection;
                    last_snapshot = Some(snapshot.clone());
                    full_present_pending = false;
                }
            } else if dirty_seq != 0
                && (shell_dirty || selection_changed || blink_on != last_blink_on)
            {
                let ranges = collect_dirty_row_ranges(
                    last_snapshot.as_ref(),
                    &snapshot,
                    &last_selection,
                    &selection,
                    last_blink_on,
                    blink_on,
                );
                let mut uploaded_any = false;
                let mut all_uploads_ok = true;
                if let Some(ranges) = ranges {
                    for (row, start_col, end_col) in ranges {
                        let Some((x, w)) = row_range_px(
                            last_snapshot.as_ref(),
                            &snapshot,
                            row,
                            start_col,
                            end_col,
                            last_content_size.0.max(1),
                        ) else {
                            continue;
                        };
                        let y = (row as u32)
                            .saturating_mul(ui2_shell_line_height())
                            .min(last_content_size.1.saturating_sub(1));
                        let h = ui2_shell_line_height()
                            .min(last_content_size.1.saturating_sub(y))
                            .max(1);
                        let rgba = render_shell_row_range_rgba(
                            &atlases, &snapshot, &selection, blink_on, row, start_col, end_col, x,
                            w,
                        );
                        let ok = crate::r::io::cabi::queue_texture_rgba_image_region_upload_copy(
                            surface.tex_id(),
                            last_content_size.0.max(1),
                            last_content_size.1.max(1),
                            x,
                            y,
                            w,
                            h,
                            rgba.as_slice(),
                            u32::MAX,
                            "ui2-shell-demo-dirty",
                        );
                        uploaded_any |= ok;
                        all_uploads_ok &= ok;
                    }
                } else {
                    let rgba = render_shell_snapshot_rgba(
                        &atlases,
                        &snapshot,
                        &selection,
                        blink_on,
                        last_content_size.0.max(1),
                        last_content_size.1.max(1),
                    );
                    all_uploads_ok = crate::r::io::cabi::queue_texture_rgba_image_upload_copy(
                        surface.tex_id(),
                        last_content_size.0.max(1),
                        last_content_size.1.max(1),
                        rgba.as_slice(),
                        u32::MAX,
                        "ui2-shell-demo-dirty-full",
                    );
                    uploaded_any = all_uploads_ok;
                }
                if all_uploads_ok {
                    if shell_dirty && dirty_seq != last_rendered_seq {
                        crate::shell2::ui2_shell_mark_rendered(dirty_seq);
                        last_rendered_seq = dirty_seq;
                    }
                    if uploaded_any || blink_on != last_blink_on {
                        last_snapshot = Some(snapshot.clone());
                    }
                    last_blink_on = blink_on;
                    last_selection = selection;
                }
            }
        }
        if crate::r::spawn_service::wait_task_or_timeout_ms("ui2-shell-demo", 33).await {
            break;
        }
    }
}
