use alloc::vec::Vec;

use crate::intel::types::Rgba8;

const UI3_SHELL_WINDOW_ID: u32 = 0x5533_0001;
const UI3_SHELL_COLS: usize = 140;
const UI3_SHELL_PADDING_PX: u32 = 18;
const UI3_SHELL_MAX_ROWS: usize = 96;
const UI3_SHELL_BLACK_RGBA: u32 = 0xFF00_0000;
const UI3_SHELL_DEFAULT_BG: (u8, u8, u8) = (0x0C, 0x10, 0x16);

#[derive(Debug)]
pub(crate) struct Ui3ShellOverlayState {
    pub(crate) active: bool,
    keyboard_read_seq: u64,
    last_rendered_seq: u32,
    attached_width: u32,
    attached_height: u32,
    attached_rows: usize,
    rendered_rows: Vec<Ui3ShellRow>,
    rendered_panel: crate::intel::LiveOverlayRect,
}

impl Default for Ui3ShellOverlayState {
    fn default() -> Self {
        Self {
            active: false,
            keyboard_read_seq: 0,
            last_rendered_seq: 0,
            attached_width: 0,
            attached_height: 0,
            attached_rows: 0,
            rendered_rows: Vec::new(),
            rendered_panel: crate::intel::LiveOverlayRect::new(0, 0, 0, 0, Rgba8::new(0, 0, 0, 0)),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Ui3ShellRow {
    cells: Vec<crate::shell2::Ui2ShellCell>,
}

impl Ui3ShellRow {
    fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct Ui3ShellOverlayInput {
    pub(crate) toggled_on: bool,
    pub(crate) toggled_off: bool,
}

impl Ui3ShellOverlayState {
    pub(crate) fn deactivate(&mut self) {
        self.active = false;
        self.last_rendered_seq = 0;
        self.rendered_rows.clear();
    }
}

pub(crate) fn handle_keyboard(
    state: &mut Ui3ShellOverlayState,
    viewport_width: u32,
    viewport_height: u32,
) -> Ui3ShellOverlayInput {
    let mut out = [crate::r::keyboard::TrueosKeyboardOutputEvent::default(); 32];
    let (next_seq, dropped, wrote) =
        crate::r::keyboard::read_output_events_since(state.keyboard_read_seq, &mut out);
    state.keyboard_read_seq = next_seq;
    if dropped != 0 {
        crate::log!("ui3-shell-overlay: keyboard dropped={}\n", dropped);
    }

    let mut input = Ui3ShellOverlayInput::default();
    for event in out.iter().take(wrote).copied() {
        if is_start_key(event) {
            state.active = !state.active;
            if state.active {
                input.toggled_on = true;
                attach_shell_window(state, viewport_width, viewport_height);
            } else {
                input.toggled_off = true;
                state.last_rendered_seq = 0;
                state.rendered_rows.clear();
            }
            crate::log!(
                "ui3-shell-overlay: toggle active={} source=start-key\n",
                state.active as u8
            );
            continue;
        }

        if state.active {
            attach_shell_window(state, viewport_width, viewport_height);
            let _ = crate::shell2::queue_ui2_shell_keyboard_event(UI3_SHELL_WINDOW_ID, event);
        }
    }
    input
}

pub(crate) fn draw_scene_if_dirty(
    state: &mut Ui3ShellOverlayState,
    surface: &crate::ui3::ui3_surface::Ui3RgbaSurface,
    viewport_width: u32,
    viewport_height: u32,
    scroll_y: u32,
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

    let mut rows = snapshot_rows(&snapshot);
    trim_empty_head_to_fit(&mut rows, visible_row_cap(rect.height));
    let mut doc_panel = rect;
    doc_panel.y = doc_panel.y.saturating_add(scroll_y);
    let full_repaint = force
        || state.last_rendered_seq == 0
        || state.rendered_panel.x != doc_panel.x
        || state.rendered_panel.y != doc_panel.y
        || state.rendered_panel.width != doc_panel.width
        || state.rendered_panel.height != doc_panel.height
        || state.rendered_rows.len() != rows.len();
    let dirty_rows = if full_repaint {
        all_row_indices(rows.len())
    } else {
        dirty_row_indices(state.rendered_rows.as_slice(), rows.as_slice())
    };
    if dirty_rows.is_empty() {
        state.last_rendered_seq = seq;
        crate::shell2::ui2_shell_mark_rendered(seq);
        return false;
    }

    let mut fill_descs = if full_repaint {
        let doc_rect = crate::intel::gpgpu::GpgpuRect::new(
            doc_panel.x as i32,
            doc_panel.y as i32,
            doc_panel.width,
            doc_panel.height,
        );
        fill_scene_rect(surface, doc_rect, UI3_SHELL_BLACK_RGBA)
    } else {
        fill_dirty_row_rects(surface, doc_panel, dirty_rows.as_slice())
    };
    fill_descs = fill_descs.saturating_add(fill_dirty_cell_backgrounds(
        surface,
        rows.as_slice(),
        doc_panel,
        dirty_rows.as_slice(),
    ));

    let placements =
        collect_dirty_row_text_placements(rows.as_slice(), doc_panel, dirty_rows.as_slice());
    let sprite_submits = if placements.is_empty() {
        0
    } else {
        draw_scene_text(surface, placements.as_slice(), reason)
    };
    state.last_rendered_seq = seq;
    state.rendered_panel = doc_panel;
    state.rendered_rows = rows.clone();
    crate::shell2::ui2_shell_mark_rendered(seq);
    crate::log!(
        "ui3-shell-overlay: scene-draw seq={} rows={} dirty_rows={} full={} placements={} rect={}x{}@{},{} scroll_y={} fill_descs={} sprite_submits={}\n",
        seq,
        rows.len(),
        dirty_rows.len(),
        full_repaint as u8,
        placements.len(),
        rect.width,
        rect.height,
        rect.x,
        rect.y,
        scroll_y,
        fill_descs,
        sprite_submits
    );
    true
}

fn is_start_key(event: crate::r::keyboard::TrueosKeyboardOutputEvent) -> bool {
    event.kind == crate::r::keyboard::KEYBOARD_OUTPUT_KIND_KEY
        && event.key_code == crate::r::keyboard::KEYBOARD_KEY_START
}

fn fill_scene_rect(
    surface: &crate::ui3::ui3_surface::Ui3RgbaSurface,
    doc_rect: crate::intel::gpgpu::GpgpuRect,
    color_rgba: u32,
) -> usize {
    let mut descs = 0usize;
    for page in surface.pages() {
        let page_y0 = page.y0 as i32;
        let page_y1 = page_y0.saturating_add(page.height as i32);
        let rect_y1 = doc_rect.y.saturating_add(doc_rect.height as i32);
        if rect_y1 <= page_y0 || doc_rect.y >= page_y1 {
            continue;
        }
        let local_y0 = doc_rect.y.max(page_y0).saturating_sub(page_y0);
        let local_y1 = rect_y1.min(page_y1).saturating_sub(page_y0);
        if local_y1 <= local_y0 {
            continue;
        }
        descs = descs.saturating_add(crate::intel::gpgpu::fill_rect_worklist_rgba8(
            page.as_gpgpu(surface.width, surface.pitch_bytes),
            crate::intel::gpgpu::GpgpuRect::new(
                doc_rect.x,
                local_y0,
                doc_rect.width,
                (local_y1 - local_y0) as u32,
            ),
            color_rgba,
        ));
    }
    descs
}

fn fill_dirty_row_rects(
    surface: &crate::ui3::ui3_surface::Ui3RgbaSurface,
    panel: crate::intel::LiveOverlayRect,
    dirty_rows: &[usize],
) -> usize {
    let mut descs = 0usize;
    for row in dirty_rows.iter().copied() {
        if let Some(rect) = shell_row_rect(panel, row) {
            descs = descs.saturating_add(fill_scene_rect(surface, rect, UI3_SHELL_BLACK_RGBA));
        }
    }
    descs
}

fn fill_dirty_cell_backgrounds(
    surface: &crate::ui3::ui3_surface::Ui3RgbaSurface,
    rows: &[Ui3ShellRow],
    panel: crate::intel::LiveOverlayRect,
    dirty_rows: &[usize],
) -> usize {
    let face = crate::ui3::althlasfont::bitmapfont::ATHLAS_FONT_FACE_LUCIDA_HALF;
    let line_height = shell_line_height() as i32;
    let content_x0 = panel.x.saturating_add(UI3_SHELL_PADDING_PX) as i32;
    let content_x1 = panel
        .x
        .saturating_add(panel.width)
        .saturating_sub(UI3_SHELL_PADDING_PX) as i32;
    let content_y0 = panel.y.saturating_add(UI3_SHELL_PADDING_PX) as i32;
    let content_y1 = panel
        .y
        .saturating_add(panel.height)
        .saturating_sub(UI3_SHELL_PADDING_PX) as i32;
    let space_advance = shell_space_advance(line_height);
    let mut descs = 0usize;

    for row_idx in dirty_rows.iter().copied() {
        let Some(row) = rows.get(row_idx) else {
            continue;
        };
        let row_y0 = content_y0.saturating_add((row_idx as i32).saturating_mul(line_height));
        let row_y1 = row_y0.saturating_add(line_height).min(content_y1);
        if row_y0 >= row_y1 {
            break;
        }

        let mut pen_x = content_x0;
        let mut run_x0 = 0i32;
        let mut run_x1 = 0i32;
        let mut run_bg: Option<(u8, u8, u8)> = None;
        for cell in row.cells.iter().copied() {
            let advance = shell_cell_advance(face, cell.ch, line_height, space_advance);
            let next_x = pen_x.saturating_add(advance).min(content_x1);
            let bg = if cell.bg == UI3_SHELL_DEFAULT_BG {
                None
            } else {
                Some(cell.bg)
            };
            if bg != run_bg {
                descs = descs
                    .saturating_add(flush_bg_run(surface, run_bg, run_x0, run_x1, row_y0, row_y1));
                run_bg = bg;
                run_x0 = pen_x;
            }
            run_x1 = next_x;
            pen_x = pen_x.saturating_add(advance);
            if pen_x >= content_x1 {
                break;
            }
        }
        descs = descs.saturating_add(flush_bg_run(surface, run_bg, run_x0, run_x1, row_y0, row_y1));
    }

    descs
}

fn flush_bg_run(
    surface: &crate::ui3::ui3_surface::Ui3RgbaSurface,
    bg: Option<(u8, u8, u8)>,
    x0: i32,
    x1: i32,
    y0: i32,
    y1: i32,
) -> usize {
    let Some(bg) = bg else {
        return 0;
    };
    if x0 >= x1 || y0 >= y1 {
        return 0;
    }
    fill_scene_rect(
        surface,
        crate::intel::gpgpu::GpgpuRect::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32),
        rgb_to_gpgpu_rgba(bg),
    )
}

fn draw_scene_text(
    surface: &crate::ui3::ui3_surface::Ui3RgbaSurface,
    placements: &[crate::intel::gpgpu::GpgpuSprite64Placement],
    reason: &str,
) -> usize {
    let mut submitted = 0usize;
    let mut shifted = Vec::new();
    let cell = crate::intel::gpgpu::SPRITE64_WORKLIST_CELL_PIXELS as i32;
    for page in surface.pages() {
        shifted.clear();
        let page_y0 = page.y0 as i32;
        let page_y1 = page_y0.saturating_add(page.height as i32);
        for placement in placements.iter().copied() {
            let glyph_y0 = placement.dst_y();
            let glyph_y1 = glyph_y0.saturating_add(cell);
            if glyph_y1 <= page_y0 || glyph_y0 >= page_y1 {
                continue;
            }
            shifted.push(placement.translated(0, -page_y0));
        }
        if shifted.is_empty() {
            continue;
        }
        if let Some(result) = crate::intel::gpgpu::sprite64_worklist_surface(
            shifted.as_slice(),
            page.as_gpgpu(surface.width, surface.pitch_bytes),
            reason,
        ) {
            if result.submitted {
                submitted = submitted.saturating_add(1);
            }
        }
    }
    submitted
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
    state.rendered_rows.clear();
    state.last_rendered_seq = 0;
}

fn shell_rect(viewport_width: u32, viewport_height: u32) -> crate::intel::LiveOverlayRect {
    if viewport_width == 0 || viewport_height == 0 {
        return crate::intel::LiveOverlayRect::new(0, 0, 0, 0, Rgba8::new(0, 0, 0, 0));
    }
    let height = viewport_height.saturating_mul(2).saturating_div(3);
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

fn snapshot_rows(snapshot: &crate::shell2::Ui2ShellScreenSnapshot) -> Vec<Ui3ShellRow> {
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
        let mut cells = snapshot.cells[start..end].to_vec();
        trim_end_default_blank_cells(&mut cells);
        out.push(Ui3ShellRow { cells });
    }
    out
}

fn trim_empty_head_to_fit(rows: &mut Vec<Ui3ShellRow>, cap: usize) {
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

fn trim_end_default_blank_cells(cells: &mut Vec<crate::shell2::Ui2ShellCell>) {
    while cells.last().is_some_and(is_default_blank_cell) {
        cells.pop();
    }
}

fn is_default_blank_cell(cell: &crate::shell2::Ui2ShellCell) -> bool {
    cell.ch == ' ' && cell.bg == UI3_SHELL_DEFAULT_BG
}

fn all_row_indices(len: usize) -> Vec<usize> {
    let mut rows = Vec::new();
    for index in 0..len {
        rows.push(index);
    }
    rows
}

fn dirty_row_indices(previous: &[Ui3ShellRow], rows: &[Ui3ShellRow]) -> Vec<usize> {
    let mut dirty = Vec::new();
    let len = previous.len().max(rows.len());
    for index in 0..len {
        if previous.get(index) != rows.get(index) {
            dirty.push(index);
        }
    }
    dirty
}

fn shell_row_rect(
    panel: crate::intel::LiveOverlayRect,
    row_idx: usize,
) -> Option<crate::intel::gpgpu::GpgpuRect> {
    let line_height = shell_line_height();
    let x = panel.x.saturating_add(UI3_SHELL_PADDING_PX);
    let y = panel
        .y
        .saturating_add(UI3_SHELL_PADDING_PX)
        .saturating_add((row_idx as u32).saturating_mul(line_height));
    let x1 = panel
        .x
        .saturating_add(panel.width)
        .saturating_sub(UI3_SHELL_PADDING_PX);
    let y1 = panel
        .y
        .saturating_add(panel.height)
        .saturating_sub(UI3_SHELL_PADDING_PX)
        .min(y.saturating_add(line_height));
    if x >= x1 || y >= y1 {
        return None;
    }
    Some(crate::intel::gpgpu::GpgpuRect::new(
        x as i32,
        y as i32,
        x1.saturating_sub(x),
        y1.saturating_sub(y),
    ))
}

fn collect_dirty_row_text_placements(
    rows: &[Ui3ShellRow],
    panel: crate::intel::LiveOverlayRect,
    dirty_rows: &[usize],
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
    let space_advance = shell_space_advance(line_height);
    let mut placements = Vec::new();
    for row_idx in dirty_rows.iter().copied() {
        let Some(row) = rows.get(row_idx) else {
            continue;
        };
        let baseline_y = content_y0.saturating_add((row_idx as i32).saturating_mul(line_height));
        if baseline_y.saturating_add(line_height) > content_y1 {
            break;
        }
        let mut pen_x = content_x0;
        for cell in row.cells.iter().copied() {
            let ch = cell.ch;
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
            let advance = shell_glyph_advance(region, line_height);
            if pen_x.saturating_add(advance) > content_x1 {
                break;
            }
            if let Some(slot) = crate::intel::gpgpu::sprite64_font_slot_for_region(face, region) {
                placements.push(crate::intel::gpgpu::GpgpuSprite64Placement::tinted_src_over(
                    slot,
                    pen_x,
                    baseline_y,
                    rgb_to_gpgpu_rgba(cell.fg),
                ));
            }
            pen_x = pen_x.saturating_add(advance);
        }
    }
    placements
}

fn shell_space_advance(line_height: i32) -> i32 {
    (line_height.saturating_mul(35) / 100).max(1)
}

fn shell_glyph_advance(
    region: crate::ui3::althlasfont::athlasmetrics::AthlasGlyphRegion,
    _line_height: i32,
) -> i32 {
    i32::from(crate::ui3::althlasfont::bitmapfont::athlas_glyph_advance_px(region))
}

fn shell_cell_advance(
    face: crate::ui3::althlasfont::bitmapfont::AthlasFontFace,
    ch: char,
    line_height: i32,
    space_advance: i32,
) -> i32 {
    if ch == '\t' {
        return space_advance.saturating_mul(4);
    }
    if ch.is_whitespace() || ch.is_control() {
        return space_advance;
    }
    crate::ui3::althlasfont::bitmapfont::athlas_lookup_glyph_region(face, ch)
        .map(|region| shell_glyph_advance(region, line_height))
        .unwrap_or(space_advance)
}

fn rgb_to_gpgpu_rgba(rgb: (u8, u8, u8)) -> u32 {
    0xFF00_0000 | ((rgb.2 as u32) << 16) | ((rgb.1 as u32) << 8) | rgb.0 as u32
}
