use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use libm::{ceilf, floorf};
use spin::Once;

use crate::gfx::imbafont::{
    ImbaFontFace, ImbaFontGlyphMetricsPx, ImbaFontMaskTexture, glyph_metrics_px,
    rasterize_glyph_mask_texture,
};

struct ImbaAthlasBuffers {
    alpha: Vec<u8>,
    index: Vec<u16>,
    widths: Vec<u8>,
    heights: Vec<u8>,
    advances: Vec<u8>,
    sample_xs: Vec<u8>,
    sample_ys: Vec<u8>,
    width: u32,
    height: u32,
    cell_w: u32,
    cell_h: u32,
    grid_w: u32,
    grid_h: u32,
    left_pad: u16,
}

pub struct ImbaAthlasView<'a> {
    pub alpha: &'a [u8],
    pub index: &'a [u16],
    pub widths: &'a [u8],
    pub heights: &'a [u8],
    pub advances: &'a [u8],
    pub sample_xs: &'a [u8],
    pub sample_ys: &'a [u8],
    pub width: u32,
    pub height: u32,
    pub cell_w: u32,
    pub cell_h: u32,
    pub grid_w: u32,
    pub grid_h: u32,
    pub left_pad: u16,
}

static IMBA_ATHLAS_SMALL: Once<ImbaAthlasBuffers> = Once::new();
static IMBA_ATHLAS_LARGE: Once<ImbaAthlasBuffers> = Once::new();

#[repr(C)]
#[derive(Clone, Copy)]
struct TexVertex {
    x: f32,
    y: f32,
    u: f32,
    v: f32,
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

const IMBA_ATHLAS_SMALL_TEX_ID: u32 = 1001;
const IMBA_ATHLAS_LARGE_TEX_ID: u32 = 1002;
static IMBA_ATHLAS_SMALL_UPLOADED: AtomicBool = AtomicBool::new(false);
static IMBA_ATHLAS_LARGE_UPLOADED: AtomicBool = AtomicBool::new(false);
static IMBA_ATHLAS_SMALL_RGBA: Once<Vec<u8>> = Once::new();
static IMBA_ATHLAS_LARGE_RGBA: Once<Vec<u8>> = Once::new();

const IMBA_ATHLAS_GRID: usize = 16;
const IMBA_ATHLAS_SMALL_TILE_H: f32 = 14.0;
const IMBA_ATHLAS_LARGE_TILE_H: f32 = 24.0;
const IMBA_ATHLAS_SIDE_PAD_PX: f32 = 1.0;
const IMBA_ATHLAS_TOP_PAD_PX: f32 = 1.0;
const IMBA_ATHLAS_BOTTOM_PAD_PX: f32 = 1.0;
const IMBA_ATHLAS_PLACEHOLDER_ADVANCE_FACTOR: f32 = 0.72;

struct ImbaAthlasGlyphSource {
    metrics: Option<ImbaFontGlyphMetricsPx>,
    mask: Option<ImbaFontMaskTexture>,
}

#[inline]
fn fill_cell(
    alpha: &mut [u8],
    athlas_w: usize,
    cell_w: usize,
    cell_h: usize,
    slot: usize,
    src: &[u8],
) {
    let cell_x = (slot % IMBA_ATHLAS_GRID) * cell_w;
    let cell_y = (slot / IMBA_ATHLAS_GRID) * cell_h;
    for y in 0..cell_h {
        let dst_y = cell_y + y;
        let src_off = y * cell_w;
        let dst_off = dst_y * athlas_w + cell_x;
        alpha[dst_off..dst_off + cell_w].copy_from_slice(&src[src_off..src_off + cell_w]);
    }
}

#[inline]
fn round_px(value: f32) -> usize {
    floorf(value + 0.5).max(0.0) as usize
}

fn blit_mask_alpha(
    cell: &mut [u8],
    cell_w: usize,
    cell_h: usize,
    mask: &ImbaFontMaskTexture,
    dst_x: usize,
    dst_y: usize,
) {
    for sy in 0..mask.height as usize {
        let cy = dst_y.saturating_add(sy);
        if cy >= cell_h {
            continue;
        }

        for sx in 0..mask.width as usize {
            let cx = dst_x.saturating_add(sx);
            if cx >= cell_w {
                continue;
            }

            let src_idx = sy
                .saturating_mul(mask.width as usize)
                .saturating_add(sx)
                .saturating_mul(4)
                .saturating_add(3);
            let Some(&alpha) = mask.rgba.get(src_idx) else {
                continue;
            };
            if alpha == 0 {
                continue;
            }

            let dst_idx = cy.saturating_mul(cell_w).saturating_add(cx);
            if let Some(dst) = cell.get_mut(dst_idx) {
                *dst = (*dst).max(alpha);
            }
        }
    }
}

fn draw_placeholder_cell(cell: &mut [u8], cell_w: usize, cell_h: usize, glyph_w: usize) {
    if cell_w == 0 || cell_h == 0 {
        return;
    }

    let draw_w = glyph_w.min(cell_w).max(3);
    let left = 1usize.min(draw_w.saturating_sub(1));
    let right = draw_w.saturating_sub(2).max(left);
    let top = 1usize.min(cell_h.saturating_sub(1));
    let bottom = cell_h.saturating_sub(2).max(top);

    for y in top..=bottom {
        for x in left..=right {
            let border = y == top || y == bottom || x == left || x == right;
            let slash = (x - left) == (y - top).min(right.saturating_sub(left));
            if !border && !slash {
                continue;
            }
            let idx = y.saturating_mul(cell_w).saturating_add(x);
            if let Some(px) = cell.get_mut(idx) {
                *px = 0xE0;
            }
        }
    }
}

fn build_imba_athlas(face: ImbaFontFace, tile_h: f32) -> ImbaAthlasBuffers {
    let mut glyphs = Vec::with_capacity(256);
    let mut ascent = tile_h;
    let mut descent = 0.0f32;
    let mut min_left = 0.0f32;
    let mut max_right = tile_h;
    let mut max_advance = tile_h * IMBA_ATHLAS_PLACEHOLDER_ADVANCE_FACTOR;

    for code in 0u16..=0xFF {
        let ch = code as u8 as char;
        let metrics = glyph_metrics_px(face, ch, tile_h);
        let mask = rasterize_glyph_mask_texture(face, ch, tile_h, 0.0);

        if let (Some(metrics), Some(_)) = (metrics, mask.as_ref()) {
            ascent = ascent.max((metrics.baseline - metrics.top).max(0.0));
            descent = descent.max((metrics.bottom - metrics.baseline).max(0.0));
            min_left = min_left.min(metrics.left);
            max_right = max_right.max(metrics.right);
            max_advance = max_advance.max(metrics.advance);
        } else if let Some(metrics) = metrics {
            max_advance = max_advance.max(metrics.advance);
        }

        glyphs.push(ImbaAthlasGlyphSource { metrics, mask });
    }

    let left_pad = ceilf((-min_left).max(0.0) + IMBA_ATHLAS_SIDE_PAD_PX).max(1.0) as usize;
    let baseline_y = ceilf(ascent + IMBA_ATHLAS_TOP_PAD_PX).max(1.0) as usize;
    let cell_h = ceilf(ascent + descent + IMBA_ATHLAS_TOP_PAD_PX + IMBA_ATHLAS_BOTTOM_PAD_PX)
        .max(1.0) as usize;
    let placeholder_w = ceilf(
        tile_h * IMBA_ATHLAS_PLACEHOLDER_ADVANCE_FACTOR + left_pad as f32 + IMBA_ATHLAS_SIDE_PAD_PX,
    )
    .max(3.0) as usize;
    let cell_w =
        ceilf(max_advance.max(max_right - min_left) + left_pad as f32 + IMBA_ATHLAS_SIDE_PAD_PX)
            .max(placeholder_w as f32)
            .max(1.0) as usize;

    let width = IMBA_ATHLAS_GRID * cell_w;
    let height = IMBA_ATHLAS_GRID * cell_h;
    let mut alpha = vec![0u8; width * height];
    let mut index = vec![u16::MAX; 256];
    let mut widths = vec![0u8; 256];
    let mut heights = vec![0u8; 256];
    let mut advances = vec![0u8; 256];
    let mut sample_xs = vec![0u8; 256];
    let mut sample_ys = vec![0u8; 256];

    for (slot, glyph) in glyphs.iter().enumerate() {
        let ch = slot as u8 as char;
        let mut cell = vec![0u8; cell_w * cell_h];
        let (sample_x, sample_y, sample_w, sample_h, advance_w) = match (&glyph.metrics, &glyph.mask) {
            (Some(metrics), Some(mask)) => {
                let dst_x = round_px(left_pad as f32 + mask.draw_x);
                let dst_y = round_px(baseline_y as f32 - metrics.baseline + mask.draw_y);
                blit_mask_alpha(&mut cell, cell_w, cell_h, mask, dst_x, dst_y);
                let advance_w = ceilf(metrics.advance).max(1.0) as usize;
                (
                    dst_x.min(u8::MAX as usize) as u8,
                    dst_y.min(u8::MAX as usize) as u8,
                    mask.width.min(u8::MAX as u32) as u8,
                    mask.height.min(u8::MAX as u32) as u8,
                    advance_w,
                )
            }
            (Some(metrics), None) if ch == ' ' => {
                let advance_w = ceilf(metrics.advance).max(1.0) as usize;
                (0, 0, 0, 0, advance_w)
            }
            _ => {
                draw_placeholder_cell(&mut cell, cell_w, cell_h, placeholder_w);
                (
                    0,
                    0,
                    placeholder_w.min(cell_w).min(u8::MAX as usize) as u8,
                    cell_h.min(u8::MAX as usize) as u8,
                    placeholder_w,
                )
            }
        };

        fill_cell(&mut alpha, width, cell_w, cell_h, slot, &cell);
        index[slot] = slot as u16;
        widths[slot] = sample_w.min(cell_w.min(u8::MAX as usize) as u8);
        heights[slot] = sample_h.min(cell_h.min(u8::MAX as usize) as u8);
        advances[slot] = advance_w.min(cell_w).min(u8::MAX as usize) as u8;
        sample_xs[slot] = sample_x.min(cell_w.min(u8::MAX as usize) as u8);
        sample_ys[slot] = sample_y.min(cell_h.min(u8::MAX as usize) as u8);
    }

    ImbaAthlasBuffers {
        alpha,
        index,
        widths,
        heights,
        advances,
        sample_xs,
        sample_ys,
        width: width as u32,
        height: height as u32,
        cell_w: cell_w as u32,
        cell_h: cell_h as u32,
        grid_w: IMBA_ATHLAS_GRID as u32,
        grid_h: IMBA_ATHLAS_GRID as u32,
        left_pad: left_pad.min(u16::MAX as usize) as u16,
    }
}

fn build_imba_athlas_small() -> ImbaAthlasBuffers {
    build_imba_athlas(ImbaFontFace::Lucidasansunicode, IMBA_ATHLAS_SMALL_TILE_H)
}

fn build_imba_athlas_large() -> ImbaAthlasBuffers {
    build_imba_athlas(ImbaFontFace::Lucidasansunicode, IMBA_ATHLAS_LARGE_TILE_H)
}

fn imba_athlas_small() -> &'static ImbaAthlasBuffers {
    IMBA_ATHLAS_SMALL.call_once(build_imba_athlas_small)
}

fn imba_athlas_large() -> &'static ImbaAthlasBuffers {
    IMBA_ATHLAS_LARGE.call_once(build_imba_athlas_large)
}

#[inline]
fn imba_athlas_view_from_buffers(athlas: &'static ImbaAthlasBuffers) -> ImbaAthlasView<'static> {
    ImbaAthlasView {
        alpha: athlas.alpha.as_slice(),
        index: athlas.index.as_slice(),
        widths: athlas.widths.as_slice(),
        heights: athlas.heights.as_slice(),
        advances: athlas.advances.as_slice(),
        sample_xs: athlas.sample_xs.as_slice(),
        sample_ys: athlas.sample_ys.as_slice(),
        width: athlas.width,
        height: athlas.height,
        cell_w: athlas.cell_w,
        cell_h: athlas.cell_h,
        grid_w: athlas.grid_w,
        grid_h: athlas.grid_h,
        left_pad: athlas.left_pad,
    }
}

pub fn imba_athlas_small_view() -> ImbaAthlasView<'static> {
    imba_athlas_view_from_buffers(imba_athlas_small())
}

pub fn imba_athlas_large_view() -> ImbaAthlasView<'static> {
    imba_athlas_view_from_buffers(imba_athlas_large())
}

#[inline]
fn imba_athlas_kind_for_px_h(px_h: f32) -> u32 {
    let small_h = imba_athlas_small_view().cell_h.max(1) as f32;
    let large_h = imba_athlas_large_view().cell_h.max(1) as f32;
    let requested = if px_h.is_finite() && px_h > 0.0 { px_h } else { large_h };
    if (requested - small_h).abs() <= (requested - large_h).abs() {
        0
    } else {
        1
    }
}

#[inline]
fn imba_athlas_view_for_kind(kind: u32) -> ImbaAthlasView<'static> {
    if kind == 0 {
        imba_athlas_small_view()
    } else {
        imba_athlas_large_view()
    }
}

#[inline]
fn imba_athlas_tex_id_for_kind(kind: u32) -> u32 {
    if kind == 0 {
        IMBA_ATHLAS_SMALL_TEX_ID
    } else {
        IMBA_ATHLAS_LARGE_TEX_ID
    }
}

fn imba_athlas_rgba_for_kind(kind: u32) -> &'static [u8] {
    let rgba_once = if kind == 0 {
        &IMBA_ATHLAS_SMALL_RGBA
    } else {
        &IMBA_ATHLAS_LARGE_RGBA
    };
    rgba_once.call_once(|| {
        let athlas = imba_athlas_view_for_kind(kind);
        let tex_px = (athlas.width as usize).saturating_mul(athlas.height as usize);
        let mut tex_rgba = alloc::vec![0u8; tex_px.saturating_mul(4)];
        for (i, &a) in athlas.alpha.iter().enumerate() {
            let o = i.saturating_mul(4);
            tex_rgba[o] = 255;
            tex_rgba[o + 1] = 255;
            tex_rgba[o + 2] = 255;
            tex_rgba[o + 3] = a;
        }
        tex_rgba
    })
}

fn ensure_imba_athlas_uploaded_kind(kind: u32) -> bool {
    let uploaded = if kind == 0 {
        &IMBA_ATHLAS_SMALL_UPLOADED
    } else {
        &IMBA_ATHLAS_LARGE_UPLOADED
    };
    if uploaded.load(Ordering::Acquire) {
        return true;
    }

    let athlas = imba_athlas_view_for_kind(kind);
    let rgba = imba_athlas_rgba_for_kind(kind);
    let tex_id = imba_athlas_tex_id_for_kind(kind);
    let rc = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_upload_texture_rgba(
            tex_id,
            athlas.width,
            athlas.height,
            rgba.as_ptr(),
            rgba.len(),
        )
    };
    if rc != 0 {
        return false;
    }
    uploaded.store(true, Ordering::Release);
    true
}

fn build_vertices(
    athlas: ImbaAthlasView<'_>,
    text: &[u8],
    x: f32,
    y: f32,
    view_w: f32,
    view_h: f32,
    scale: f32,
    alpha: u8,
    out: &mut Vec<TexVertex>,
) {
    if text.is_empty() {
        return;
    }

    let grid_w = athlas.grid_w.max(1);
    let athlas_w = athlas.width as f32;
    let athlas_h = athlas.height as f32;
    let fallback = athlas.index.get(b'?' as usize).copied().unwrap_or(0);

    let glyph_advance_px = |ch: u8| {
        let mut slot = athlas.index.get(ch as usize).copied().unwrap_or(fallback);
        if slot == u16::MAX {
            slot = fallback;
        }
        athlas
            .advances
            .get(slot as usize)
            .copied()
            .unwrap_or(athlas.cell_w as u8) as f32
    };

    let mut pen_x = x;
    let mut pen_y = y;
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };

    for &ch in text {
        if ch == b'\n' {
            pen_x = x;
            pen_y += athlas.cell_h as f32 * scale;
            continue;
        }
        if ch == b' ' {
            pen_x += glyph_advance_px(ch) * scale;
            continue;
        }

        let mut slot = athlas.index.get(ch as usize).copied().unwrap_or(fallback);
        if slot == u16::MAX {
            slot = fallback;
        }

        let glyph_w_px = athlas
            .widths
            .get(slot as usize)
            .copied()
            .unwrap_or(athlas.cell_w as u8) as f32
            * scale;
        let glyph_h_px = athlas
            .heights
            .get(slot as usize)
            .copied()
            .unwrap_or(athlas.cell_h as u8) as f32
            * scale;
        if glyph_w_px <= 0.0 || glyph_h_px <= 0.0 {
            pen_x += glyph_advance_px(ch) * scale;
            continue;
        }

        let sx = (slot as u32) % grid_w;
        let sy = (slot as u32) / grid_w;
        let sample_x = athlas
            .sample_xs
            .get(slot as usize)
            .copied()
            .unwrap_or(0) as f32;
        let sample_y = athlas
            .sample_ys
            .get(slot as usize)
            .copied()
            .unwrap_or(0) as f32;
        let px0 = (sx * athlas.cell_w) as f32 + sample_x;
        let py0 = (sy * athlas.cell_h) as f32 + sample_y;
        let u0 = px0 / athlas_w;
        let v0 = py0 / athlas_h;
        let u1 = (px0 + glyph_w_px) / athlas_w;
        let v1 = (py0 + glyph_h_px) / athlas_h;

        let x0 = pen_x + (sample_x - athlas.left_pad as f32) * scale;
        let y0 = pen_y + sample_y * scale;
        let x1 = x0 + glyph_w_px;
        let y1 = y0 + glyph_h_px;

        let nx0 = (2.0 * (x0 / view_w)) - 1.0;
        let ny0 = 1.0 - (2.0 * (y0 / view_h));
        let nx1 = (2.0 * (x1 / view_w)) - 1.0;
        let ny1 = 1.0 - (2.0 * (y1 / view_h));

        let c = (16u8, 16u8, 16u8, alpha);
        out.push(TexVertex {
            x: nx0,
            y: ny1,
            u: u0,
            v: v1,
            r: c.0,
            g: c.1,
            b: c.2,
            a: c.3,
        });
        out.push(TexVertex {
            x: nx1,
            y: ny1,
            u: u1,
            v: v1,
            r: c.0,
            g: c.1,
            b: c.2,
            a: c.3,
        });
        out.push(TexVertex {
            x: nx1,
            y: ny0,
            u: u1,
            v: v0,
            r: c.0,
            g: c.1,
            b: c.2,
            a: c.3,
        });
        out.push(TexVertex {
            x: nx0,
            y: ny1,
            u: u0,
            v: v1,
            r: c.0,
            g: c.1,
            b: c.2,
            a: c.3,
        });
        out.push(TexVertex {
            x: nx1,
            y: ny0,
            u: u1,
            v: v0,
            r: c.0,
            g: c.1,
            b: c.2,
            a: c.3,
        });
        out.push(TexVertex {
            x: nx0,
            y: ny0,
            u: u0,
            v: v0,
            r: c.0,
            g: c.1,
            b: c.2,
            a: c.3,
        });

        pen_x += glyph_advance_px(ch) * scale;
    }
}

pub fn draw_imba_athlas_text_in_frame(
    text: &[u8],
    x: f32,
    y: f32,
    view_w: u32,
    view_h: u32,
) -> bool {
    draw_imba_athlas_text_in_frame_alpha(text, x, y, view_w, view_h, 255)
}

#[inline]
fn imba_athlas_scale_for_px_h(px_h: f32) -> f32 {
    let athlas = imba_athlas_large_view();
    let base_h = athlas.cell_h.max(1) as f32;
    if px_h.is_finite() && px_h > 0.0 {
        px_h / base_h
    } else {
        1.0
    }
}

pub fn imba_athlas_text_width_px(text: &[u8]) -> f32 {
    imba_athlas_text_width_scaled_px(text, imba_athlas_large_view().cell_h as f32)
}

pub fn imba_athlas_text_width_scaled_px(text: &[u8], px_h: f32) -> f32 {
    if text.is_empty() {
        return 0.0;
    }

    let athlas = imba_athlas_large_view();
    let scale = imba_athlas_scale_for_px_h(px_h);
    let fallback = athlas.index.get(b'?' as usize).copied().unwrap_or(0);
    let glyph_advance_px = |ch: u8| {
        let mut slot = athlas.index.get(ch as usize).copied().unwrap_or(fallback);
        if slot == u16::MAX {
            slot = fallback;
        }
        athlas
            .advances
            .get(slot as usize)
            .copied()
            .unwrap_or(athlas.cell_w as u8) as f32
    };

    let mut line_w = 0.0f32;
    let mut max_w = 0.0f32;
    for &ch in text {
        if ch == b'\n' {
            max_w = max_w.max(line_w);
            line_w = 0.0;
            continue;
        }
        line_w += glyph_advance_px(ch) * scale;
    }
    max_w.max(line_w)
}

pub fn imba_athlas_text_width_nearest_px(text: &[u8], px_h: f32) -> f32 {
    if text.is_empty() {
        return 0.0;
    }

    let athlas = imba_athlas_view_for_kind(imba_athlas_kind_for_px_h(px_h));
    let fallback = athlas.index.get(b'?' as usize).copied().unwrap_or(0);
    let glyph_advance_px = |ch: u8| {
        let mut slot = athlas.index.get(ch as usize).copied().unwrap_or(fallback);
        if slot == u16::MAX {
            slot = fallback;
        }
        athlas
            .advances
            .get(slot as usize)
            .copied()
            .unwrap_or(athlas.cell_w as u8) as f32
    };

    let mut line_w = 0.0f32;
    let mut max_w = 0.0f32;
    for &ch in text {
        if ch == b'\n' {
            max_w = max_w.max(line_w);
            line_w = 0.0;
            continue;
        }
        line_w += glyph_advance_px(ch);
    }
    max_w.max(line_w)
}

#[inline]
fn blend_rgba_pixel(dst: &mut [u8], dst_idx: usize, rgba: (u8, u8, u8, u8), coverage: u8) {
    if dst_idx + 3 >= dst.len() || coverage == 0 || rgba.3 == 0 {
        return;
    }

    let src_a = ((rgba.3 as u32) * (coverage as u32) + 127) / 255;
    if src_a == 0 {
        return;
    }
    let inv = 255u32.saturating_sub(src_a);
    let dst_r = dst[dst_idx] as u32;
    let dst_g = dst[dst_idx + 1] as u32;
    let dst_b = dst[dst_idx + 2] as u32;
    let dst_a = dst[dst_idx + 3] as u32;

    dst[dst_idx] = (((rgba.0 as u32) * src_a + dst_r * inv + 127) / 255).min(255) as u8;
    dst[dst_idx + 1] = (((rgba.1 as u32) * src_a + dst_g * inv + 127) / 255).min(255) as u8;
    dst[dst_idx + 2] = (((rgba.2 as u32) * src_a + dst_b * inv + 127) / 255).min(255) as u8;
    dst[dst_idx + 3] = (src_a + ((dst_a * inv + 127) / 255)).min(255) as u8;
}

pub fn blit_imba_athlas_text_rgba(
    dst: &mut [u8],
    dst_w: u32,
    dst_h: u32,
    text: &[u8],
    x: i32,
    y: i32,
    rgba: (u8, u8, u8, u8),
) -> bool {
    if text.is_empty() || dst_w == 0 || dst_h == 0 {
        return false;
    }

    let athlas = imba_athlas_large_view();
    if athlas.alpha.is_empty() {
        return false;
    }

    let expected = (dst_w as usize)
        .saturating_mul(dst_h as usize)
        .saturating_mul(4);
    if dst.len() < expected {
        return false;
    }

    let fallback = athlas.index.get(b'?' as usize).copied().unwrap_or(0);
    let mut pen_x = x;
    let mut pen_y = y;
    let base_x = x;
    let mut touched = false;

    for &ch in text {
        if ch == b'\n' {
            pen_x = base_x;
            pen_y += athlas.cell_h as i32;
            continue;
        }

        let mut slot = athlas.index.get(ch as usize).copied().unwrap_or(fallback);
        if slot == u16::MAX {
            slot = fallback;
        }
        let advance = athlas
            .advances
            .get(slot as usize)
            .copied()
            .unwrap_or(athlas.cell_w as u8) as i32;
        if ch == b' ' {
            pen_x += advance.max(1);
            continue;
        }

        let sample_x = athlas
            .sample_xs
            .get(slot as usize)
            .copied()
            .unwrap_or(0) as i32;
        let sample_y = athlas
            .sample_ys
            .get(slot as usize)
            .copied()
            .unwrap_or(0) as i32;
        let sample_w = athlas
            .widths
            .get(slot as usize)
            .copied()
            .unwrap_or(athlas.cell_w as u8) as i32;
        let sample_h = athlas
            .heights
            .get(slot as usize)
            .copied()
            .unwrap_or(athlas.cell_h as u8) as i32;
        if sample_w <= 0 || sample_h <= 0 {
            pen_x += advance.max(1);
            continue;
        }
        let glyph_x = (slot as u32 % athlas.grid_w) as i32 * athlas.cell_w as i32 + sample_x;
        let glyph_y = (slot as u32 / athlas.grid_w) as i32 * athlas.cell_h as i32 + sample_y;
        let draw_x0 = pen_x + sample_x - athlas.left_pad as i32;
        let draw_y0 = pen_y + sample_y;
        for row in 0..sample_h {
            let dst_y = draw_y0 + row;
            if dst_y < 0 || dst_y >= dst_h as i32 {
                continue;
            }
            for col in 0..sample_w {
                let dst_x = draw_x0 + col;
                if dst_x < 0 || dst_x >= dst_w as i32 {
                    continue;
                }

                let src_x = glyph_x + col;
                let src_y = glyph_y + row;
                let src_idx = (src_y as usize)
                    .saturating_mul(athlas.width as usize)
                    .saturating_add(src_x as usize);
                let Some(&coverage) = athlas.alpha.get(src_idx) else {
                    continue;
                };
                if coverage == 0 {
                    continue;
                }

                let dst_idx = ((dst_y as usize)
                    .saturating_mul(dst_w as usize)
                    .saturating_add(dst_x as usize))
                .saturating_mul(4);
                blend_rgba_pixel(dst, dst_idx, rgba, coverage);
                touched = true;
            }
        }

        pen_x += advance.max(1);
    }

    touched
}

pub fn draw_imba_athlas_text_in_frame_alpha(
    text: &[u8],
    x: f32,
    y: f32,
    view_w: u32,
    view_h: u32,
    alpha: u8,
) -> bool {
    draw_imba_athlas_text_in_frame_alpha_scaled(
        text,
        x,
        y,
        view_w,
        view_h,
        imba_athlas_large_view().cell_h as f32,
        alpha,
    )
}

pub fn draw_imba_athlas_text_in_frame_alpha_nearest_px(
    text: &[u8],
    x: f32,
    y: f32,
    view_w: u32,
    view_h: u32,
    px_h: f32,
    alpha: u8,
) -> bool {
    if text.is_empty() {
        return false;
    }
    let kind = imba_athlas_kind_for_px_h(px_h);
    let athlas = imba_athlas_view_for_kind(kind);
    if !ensure_imba_athlas_uploaded_kind(kind) {
        return false;
    }

    let _ = unsafe { crate::r::io::cabi::trueos_cabi_gfx_set_sampler(0, 0, 0, 0) };
    let _ = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_set_blend(1, 0x0302, 0x0303, 0x0302, 0x0303, 0, 0)
    };

    let mut verts = Vec::with_capacity(text.len().saturating_mul(6));
    build_vertices(
        athlas,
        text,
        x,
        y,
        view_w.max(1) as f32,
        view_h.max(1) as f32,
        1.0,
        alpha,
        &mut verts,
    );
    if verts.is_empty() {
        return false;
    }

    let ptr = verts.as_ptr() as *const u8;
    let len = verts
        .len()
        .saturating_mul(core::mem::size_of::<TexVertex>());
    let rc = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_draw_tex_triangles_no_present(
            imba_athlas_tex_id_for_kind(kind),
            ptr,
            len,
        )
    };
    rc == 0
}

pub fn draw_imba_athlas_text_in_frame_alpha_scaled(
    text: &[u8],
    x: f32,
    y: f32,
    view_w: u32,
    view_h: u32,
    px_h: f32,
    alpha: u8,
) -> bool {
    if text.is_empty() {
        return false;
    }
    let athlas = imba_athlas_large_view();
    if !ensure_imba_athlas_uploaded_kind(1) {
        return false;
    }

    let _ = unsafe { crate::r::io::cabi::trueos_cabi_gfx_set_sampler(0, 0, 0, 0) };
    let _ = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_set_blend(1, 0x0302, 0x0303, 0x0302, 0x0303, 0, 0)
    };

    let mut verts = Vec::with_capacity(text.len().saturating_mul(6));
    build_vertices(
        athlas,
        text,
        x,
        y,
        view_w.max(1) as f32,
        view_h.max(1) as f32,
        imba_athlas_scale_for_px_h(px_h),
        alpha,
        &mut verts,
    );
    if verts.is_empty() {
        return false;
    }

    let ptr = verts.as_ptr() as *const u8;
    let len = verts
        .len()
        .saturating_mul(core::mem::size_of::<TexVertex>());
    let rc = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_draw_tex_triangles_no_present(
            IMBA_ATHLAS_LARGE_TEX_ID,
            ptr,
            len,
        )
    };
    rc == 0
}

pub fn draw_imba_athlas_text(text: &[u8], x: f32, y: f32) -> bool {
    let (view_w, view_h) = crate::limine::framebuffer_response()
        .and_then(|resp| resp.framebuffers().next())
        .map(|fb| (fb.width() as u32, fb.height() as u32))
        .unwrap_or((1024, 768));

    let begin_rc = unsafe { crate::r::io::cabi::trueos_cabi_gfx_begin_frame(0xFFFFFF) };
    if begin_rc != 0 {
        return false;
    }

    let ok = draw_imba_athlas_text_in_frame(text, x, y, view_w, view_h);
    let end_rc = unsafe { crate::r::io::cabi::trueos_cabi_gfx_end_frame() };
    ok && end_rc == 0
}
