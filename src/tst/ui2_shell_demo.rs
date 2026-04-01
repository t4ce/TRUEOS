use alloc::{vec, vec::Vec};

use embassy_time::{Duration as EmbassyDuration, Instant, Timer};

use crate::gfx::althlasfont::athlasmetrics;
use crate::gfx::png_codec::DecodedPng;
use crate::r::ui2::{self, Ui2FontTier, Ui2Rect};

const UI2_SHELL_TEX_ID: u32 = 4_705;
const UI2_SHELL_CONTENT_ID: u32 = 43;
const UI2_SHELL_VIEW_W: u32 = 600;
const UI2_SHELL_VIEW_H: u32 = 400;
const UI2_SHELL_WINDOW_X: f32 = 300.0;
const UI2_SHELL_WINDOW_Y: f32 = 140.0;
const UI2_SHELL_WINDOW_Z: i16 = 31;
const UI2_SHELL_BG_RGBA: [u8; 4] = [0x0C, 0x10, 0x16, 0xFF];
const UI2_SHELL_CURSOR_RGBA: [u8; 4] = [0xF1, 0xF4, 0xF8, 0xFF];
const UI2_SHELL_CURSOR_CH: char = '▏';
const UI2_SHELL_CURSOR_BLINK_MS: u64 = 1_000;
const UI2_SHELL_TEXT_COLS: usize = 100;
const UI2_SHELL_TEXT_ROWS: usize = 12;
const UI2_SHELL_FONT_TIER: Ui2FontTier = Ui2FontTier::Half;
const UI2_SHELL_HALF_SIZE_CASE: usize = UI2_SHELL_FONT_TIER.size_case();

fn ui2_shell_surface_width() -> u32 {
    UI2_SHELL_VIEW_W
}

fn ui2_shell_surface_height() -> u32 {
    UI2_SHELL_VIEW_H
}

fn ui2_shell_line_height() -> u32 {
    u32::from(ui2::ui2_font_native_line_height_px(UI2_SHELL_FONT_TIER))
}

fn ui2_shell_resolve_glyph(ch: char) -> Option<ui2::Ui2FontGlyph> {
    ui2::ui2_font_resolve_glyph(UI2_SHELL_FONT_TIER, ch)
        .or_else(|| ui2::ui2_font_resolve_glyph(UI2_SHELL_FONT_TIER, '?'))
}

fn ui2_shell_cell_advance_px(ch: char) -> usize {
    ui2_shell_resolve_glyph(ch)
        .map(|glyph| glyph.advance_px.max(1) as usize)
        .unwrap_or(1)
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
            let alpha = (coverage * u16::from(fg_rgba[3])) / 255;
            let inv_alpha = 255u16.saturating_sub(alpha);
            dst[dst_idx] =
                (((u16::from(fg_rgba[0]) * alpha) + (u16::from(dst[dst_idx]) * inv_alpha) + 127)
                    / 255) as u8;
            dst[dst_idx + 1] = (((u16::from(fg_rgba[1]) * alpha)
                + (u16::from(dst[dst_idx + 1]) * inv_alpha)
                + 127)
                / 255) as u8;
            dst[dst_idx + 2] = (((u16::from(fg_rgba[2]) * alpha)
                + (u16::from(dst[dst_idx + 2]) * inv_alpha)
                + 127)
                / 255) as u8;
            dst[dst_idx + 3] = 0xFF;
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
    pen_x: usize,
    row_y: usize,
    advance_px: usize,
    cell: &crate::shell2::Ui2ShellCell,
) {
    let Some(glyph) = ui2_shell_resolve_glyph(cell.ch) else {
        return;
    };
    let Some(atlas) = decoded.get(glyph.region.bucket as usize) else {
        return;
    };
    let draw_rect = ui2::ui2_font_place_glyph_top_center(
        &glyph,
        Ui2Rect {
            x: pen_x as f32,
            y: row_y as f32,
            w: advance_px as f32,
            h: ui2_shell_line_height() as f32,
        },
    );
    blit_half_glyph_rgba(
        rgba,
        dst_width,
        dst_height,
        atlas,
        &glyph.region,
        draw_rect.x.max(0.0) as usize,
        draw_rect.y.max(0.0) as usize,
        cell_fg_rgba(cell),
    );
}

fn render_cursor(
    rgba: &mut [u8],
    dst_width: usize,
    dst_height: usize,
    decoded: &[DecodedPng],
    snapshot: &crate::shell2::Ui2ShellScreenSnapshot,
    blink_on: bool,
) {
    if !snapshot.cursor_visible || !blink_on {
        return;
    }
    let cursor_col = (snapshot.cursor_col as usize).min(UI2_SHELL_TEXT_COLS.saturating_sub(1));
    let cursor_row = (snapshot.cursor_row as usize).min(UI2_SHELL_TEXT_ROWS.saturating_sub(1));
    let row_start = cursor_row.saturating_mul(UI2_SHELL_TEXT_COLS);
    let mut x = 0usize;
    for col in 0..cursor_col {
        let ch = snapshot
            .cells
            .get(row_start.saturating_add(col))
            .map(|cell| cell.ch)
            .unwrap_or(' ');
        x = x.saturating_add(ui2_shell_cell_advance_px(ch));
    }
    let cursor_w = snapshot
        .cells
        .get(row_start.saturating_add(cursor_col))
        .map(|cell| ui2_shell_cell_advance_px(cell.ch))
        .unwrap_or_else(|| ui2_shell_cell_advance_px(UI2_SHELL_CURSOR_CH));
    let row_y = cursor_row.saturating_mul(ui2_shell_line_height() as usize);
    let cell = snapshot
        .cells
        .get(row_start.saturating_add(cursor_col))
        .copied()
        .unwrap_or(crate::shell2::Ui2ShellCell {
            ch: ' ',
            fg: (UI2_SHELL_CURSOR_RGBA[0], UI2_SHELL_CURSOR_RGBA[1], UI2_SHELL_CURSOR_RGBA[2]),
            bg: (UI2_SHELL_BG_RGBA[0], UI2_SHELL_BG_RGBA[1], UI2_SHELL_BG_RGBA[2]),
        });
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
        decoded,
        x,
        row_y,
        cursor_w.max(1),
        &cursor_cell,
    );
}

fn render_shell_snapshot_rgba(
    decoded: &[DecodedPng],
    snapshot: &crate::shell2::Ui2ShellScreenSnapshot,
    blink_on: bool,
) -> Vec<u8> {
    let content_w = ui2_shell_surface_width() as usize;
    let content_h = ui2_shell_surface_height() as usize;
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

    for row in 0..UI2_SHELL_TEXT_ROWS {
        let row_y = row.saturating_mul(ui2_shell_line_height() as usize);
        if row_y >= content_h {
            break;
        }
        let mut pen_x = 0usize;
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
                    decoded,
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

    render_cursor(rgba.as_mut_slice(), content_w, content_h, decoded, snapshot, blink_on);
    rgba
}

#[embassy_executor::task]
pub async fn ui2_shell_demo_task() {
    let Some(decoded) = decode_half_bucket_textures() else {
        return;
    };

    let content_w = ui2_shell_surface_width();
    let content_h = ui2_shell_surface_height();
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
    let mut clear_rgba = clear_rgba;
    for idx in (0..clear_rgba.len()).step_by(4) {
        clear_rgba[idx] = UI2_SHELL_BG_RGBA[0];
        clear_rgba[idx + 1] = UI2_SHELL_BG_RGBA[1];
        clear_rgba[idx + 2] = UI2_SHELL_BG_RGBA[2];
        clear_rgba[idx + 3] = UI2_SHELL_BG_RGBA[3];
    }
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
    let mut last_blink_on = false;
    loop {
        let blink_on =
            ((Instant::now().as_millis() as u64) / (UI2_SHELL_CURSOR_BLINK_MS / 2)) % 2 == 0;
        if let Some((dirty_seq, snapshot)) = crate::shell2::ui2_shell_snapshot(surface.window_id())
            && dirty_seq != 0
            && ((dirty_seq != last_rendered_seq
                && dirty_seq != crate::shell2::ui2_shell_last_rendered_seq())
                || blink_on != last_blink_on)
        {
            let rgba = render_shell_snapshot_rgba(decoded.as_slice(), &snapshot, blink_on);
            if surface.upload_rgba(rgba.as_slice(), "ui2-shell-demo-present") {
                if dirty_seq != last_rendered_seq {
                    crate::shell2::ui2_shell_mark_rendered(dirty_seq);
                }
                last_rendered_seq = dirty_seq;
                last_blink_on = blink_on;
            }
        }
        Timer::after(EmbassyDuration::from_millis(33)).await;
    }
}
