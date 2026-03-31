use alloc::{vec, vec::Vec};

use embassy_time::{Duration as EmbassyDuration, Timer};

use crate::gfx::althlasfont::athlasmetrics;
use crate::gfx::png_codec::DecodedPng;

const UI2_SHELL_TEX_ID: u32 = 4_705;
const UI2_SHELL_CONTENT_ID: u32 = 43;
const UI2_SHELL_VIEW_W: u32 = 600;
const UI2_SHELL_VIEW_H: u32 = 400;
const UI2_SHELL_WINDOW_X: f32 = 300.0;
const UI2_SHELL_WINDOW_Y: f32 = 140.0;
const UI2_SHELL_WINDOW_Z: i16 = 31;
const UI2_SHELL_BG_RGBA: [u8; 4] = [0x0C, 0x10, 0x16, 0xFF];
const UI2_SHELL_CURSOR_RGBA: [u8; 4] = [0xF1, 0xF4, 0xF8, 0xFF];
const UI2_SHELL_TEXT_COLS: usize = 100;
const UI2_SHELL_TEXT_ROWS: usize = 12;
const UI2_SHELL_CELL_W: u32 = 8;
const UI2_SHELL_CELL_H: u32 = 32;
const UI2_SHELL_HALF_SIZE_CASE: usize = 0;

fn ui2_shell_content_width() -> u32 {
    UI2_SHELL_CELL_W.saturating_mul(UI2_SHELL_TEXT_COLS as u32)
}

fn ui2_shell_content_height() -> u32 {
    UI2_SHELL_CELL_H.saturating_mul(UI2_SHELL_TEXT_ROWS as u32)
}

fn decode_half_bucket_textures() -> Option<Vec<DecodedPng>> {
    crate::r::ui2::ui2_font_bucketproducer_decode_variant(UI2_SHELL_HALF_SIZE_CASE)
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

fn blit_half_glyph_rgba(
    dst: &mut [u8],
    dst_width: usize,
    dst_height: usize,
    atlas: &DecodedPng,
    region: &athlasmetrics::AthlasGlyphRegion,
    dst_x: usize,
    dst_y: usize,
    fg_rgba: [u8; 4],
) {
    let glyph_w = region.src_w as usize;
    let glyph_h = region.src_h as usize;
    let src_x = region.src_x as usize;
    let src_y = region.src_y as usize;
    let atlas_width = atlas.width as usize;

    for row in 0..glyph_h {
        let target_y = dst_y + row;
        if target_y >= dst_height {
            break;
        }
        for col in 0..glyph_w {
            let target_x = dst_x + col;
            if target_x >= dst_width {
                break;
            }

            let atlas_idx = ((src_y + row) * atlas_width + (src_x + col)) * 4;
            let coverage = atlas.rgba.get(atlas_idx).copied().unwrap_or(0) as u16;
            if coverage == 0 {
                continue;
            }

            let dst_idx = (target_y * dst_width + target_x) * 4;
            dst[dst_idx] = fg_rgba[0];
            dst[dst_idx + 1] = fg_rgba[1];
            dst[dst_idx + 2] = fg_rgba[2];
            dst[dst_idx + 3] = ((coverage * u16::from(fg_rgba[3])) / 255) as u8;
        }
    }
}

fn cell_bg_rgba(cell: &crate::shell2::Ui2ShellCell) -> [u8; 4] {
    [cell.bg.0, cell.bg.1, cell.bg.2, 0xFF]
}

fn cell_fg_rgba(cell: &crate::shell2::Ui2ShellCell) -> [u8; 4] {
    [cell.fg.0, cell.fg.1, cell.fg.2, 0xFF]
}

fn render_cell_glyph(
    rgba: &mut [u8],
    dst_width: usize,
    dst_height: usize,
    decoded: &[DecodedPng],
    row: usize,
    col: usize,
    cell: &crate::shell2::Ui2ShellCell,
) {
    let Some(region) = athlasmetrics::athlas_lookup_glyph_region(UI2_SHELL_HALF_SIZE_CASE, cell.ch)
    else {
        return;
    };
    let Some(atlas) = decoded.get(region.bucket as usize) else {
        return;
    };
    let cell_x = col.saturating_mul(UI2_SHELL_CELL_W as usize);
    let cell_y = row.saturating_mul(UI2_SHELL_CELL_H as usize);
    let glyph_x = cell_x
        .saturating_add(((UI2_SHELL_CELL_W as usize).saturating_sub(region.src_w as usize)) / 2);
    blit_half_glyph_rgba(
        rgba,
        dst_width,
        dst_height,
        atlas,
        &region,
        glyph_x,
        cell_y,
        cell_fg_rgba(cell),
    );
}

fn render_cursor(
    rgba: &mut [u8],
    dst_width: usize,
    dst_height: usize,
    snapshot: &crate::shell2::Ui2ShellScreenSnapshot,
) {
    if !snapshot.cursor_visible {
        return;
    }
    let cursor_col = (snapshot.cursor_col as usize).min(UI2_SHELL_TEXT_COLS.saturating_sub(1));
    let cursor_row = (snapshot.cursor_row as usize).min(UI2_SHELL_TEXT_ROWS.saturating_sub(1));
    let x = cursor_col.saturating_mul(UI2_SHELL_CELL_W as usize);
    let y = cursor_row
        .saturating_mul(UI2_SHELL_CELL_H as usize)
        .saturating_add((UI2_SHELL_CELL_H as usize).saturating_sub(3));
    fill_rect_rgba(
        rgba,
        dst_width,
        dst_height,
        x,
        y,
        UI2_SHELL_CELL_W as usize,
        2,
        UI2_SHELL_CURSOR_RGBA,
    );
}

fn render_shell_snapshot_rgba(
    decoded: &[DecodedPng],
    snapshot: &crate::shell2::Ui2ShellScreenSnapshot,
) -> Vec<u8> {
    let content_w = ui2_shell_content_width() as usize;
    let content_h = ui2_shell_content_height() as usize;
    let mut rgba = vec![0u8; content_w.saturating_mul(content_h).saturating_mul(4)];

    for row in 0..UI2_SHELL_TEXT_ROWS {
        for col in 0..UI2_SHELL_TEXT_COLS {
            let idx = row.saturating_mul(UI2_SHELL_TEXT_COLS).saturating_add(col);
            let cell = snapshot
                .cells
                .get(idx)
                .copied()
                .unwrap_or(crate::shell2::Ui2ShellCell {
                    ch: ' ',
                    fg: (0xF1, 0xF4, 0xF8),
                    bg: (0x0C, 0x10, 0x16),
                });
            fill_rect_rgba(
                rgba.as_mut_slice(),
                content_w,
                content_h,
                col.saturating_mul(UI2_SHELL_CELL_W as usize),
                row.saturating_mul(UI2_SHELL_CELL_H as usize),
                UI2_SHELL_CELL_W as usize,
                UI2_SHELL_CELL_H as usize,
                cell_bg_rgba(&cell),
            );
            if cell.ch != ' ' {
                render_cell_glyph(
                    rgba.as_mut_slice(),
                    content_w,
                    content_h,
                    decoded,
                    row,
                    col,
                    &cell,
                );
            }
        }
    }

    render_cursor(rgba.as_mut_slice(), content_w, content_h, snapshot);
    rgba
}

#[embassy_executor::task]
pub async fn ui2_shell_demo_task() {
    let Some(decoded) = decode_half_bucket_textures() else {
        return;
    };

    let content_w = ui2_shell_content_width();
    let content_h = ui2_shell_content_height();
    let Some(surface) = crate::r::ui2::Ui2SurfaceWindow::from_existing_texture_with_size(
        "Shell",
        crate::r::ui2::Ui2Rect {
            x: UI2_SHELL_WINDOW_X,
            y: UI2_SHELL_WINDOW_Y,
            w: UI2_SHELL_VIEW_W as f32,
            h: UI2_SHELL_VIEW_H as f32,
        },
        UI2_SHELL_WINDOW_Z,
        128,
        UI2_SHELL_TEX_ID,
        true,
        content_w,
        content_h,
    ) else {
        crate::log!("ui2-shell-demo: window creation failed tex={}\n", UI2_SHELL_TEX_ID);
        return;
    };

    let clear_rgba = vec![
        0u8;
        (content_w as usize)
            .saturating_mul(content_h as usize)
            .saturating_mul(4)
    ];
    if !surface.upload_rgba(clear_rgba.as_slice(), "ui2-shell-demo-clear") {
        crate::log!("ui2-shell-demo: initial texture upload failed tex={}\n", UI2_SHELL_TEX_ID);
        return;
    }
    let _ = surface.bind_hosted_scroll_state(UI2_SHELL_CONTENT_ID, content_w, content_h);
    crate::shell2::ui2_shell_attach_window(
        surface.window_id(),
        UI2_SHELL_TEXT_COLS,
        UI2_SHELL_TEXT_ROWS,
    );

    crate::log!(
        "ui2-shell-demo: window={} tex={} viewport={}x{} content={}x{} cols={} rows={}\n",
        surface.window_id(),
        surface.tex_id(),
        UI2_SHELL_VIEW_W,
        UI2_SHELL_VIEW_H,
        content_w,
        content_h,
        UI2_SHELL_TEXT_COLS,
        UI2_SHELL_TEXT_ROWS
    );

    let mut last_rendered_seq = 0u32;
    loop {
        if let Some((dirty_seq, snapshot)) = crate::shell2::ui2_shell_snapshot(surface.window_id())
            && dirty_seq != 0
            && dirty_seq != last_rendered_seq
            && dirty_seq != crate::shell2::ui2_shell_last_rendered_seq()
        {
            let rgba = render_shell_snapshot_rgba(decoded.as_slice(), &snapshot);
            if surface.upload_rgba(rgba.as_slice(), "ui2-shell-demo-present") {
                crate::shell2::ui2_shell_mark_rendered(dirty_seq);
                last_rendered_seq = dirty_seq;
            }
        }
        Timer::after(EmbassyDuration::from_millis(33)).await;
    }
}
