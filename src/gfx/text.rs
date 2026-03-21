use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Once;

struct FontAtlasBuffers {
    alpha: Vec<u8>,
    index: Vec<u16>,
    widths: Vec<u8>,
    width: u32,
    height: u32,
    cell_w: u32,
    cell_h: u32,
    grid_w: u32,
    grid_h: u32,
}

pub struct FontAtlasView<'a> {
    pub alpha: &'a [u8],
    pub index: &'a [u16],
    pub widths: &'a [u8],
    pub width: u32,
    pub height: u32,
    pub cell_w: u32,
    pub cell_h: u32,
    pub grid_w: u32,
    pub grid_h: u32,
}

static FONT_ATLAS_SMALL: Once<FontAtlasBuffers> = Once::new();
static FONT_ATLAS_LARGE: Once<FontAtlasBuffers> = Once::new();

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

const ATLAS_TEX_ID: u32 = 1001;
static ATLAS_UPLOADED: AtomicBool = AtomicBool::new(false);
static ATLAS_RGBA: Once<Vec<u8>> = Once::new();

#[inline]
fn fill_cell(
    alpha: &mut [u8],
    atlas_w: usize,
    cell_w: usize,
    cell_h: usize,
    slot: usize,
    src: &[u8],
) {
    const GRID: usize = 16;
    let cell_x = (slot % GRID) * cell_w;
    let cell_y = (slot / GRID) * cell_h;
    for y in 0..cell_h {
        let dst_y = cell_y + y;
        let src_off = y * cell_w;
        let dst_off = dst_y * atlas_w + cell_x;
        alpha[dst_off..dst_off + cell_w].copy_from_slice(&src[src_off..src_off + cell_w]);
    }
}

fn build_font_atlas_small() -> FontAtlasBuffers {
    const GRID: usize = 16;
    let cell_w = crate::vga::FONT_CELL_W;
    let cell_h = crate::vga::FONT_CELL_H;
    let width = GRID * cell_w;
    let height = GRID * cell_h;
    let mut alpha = vec![0u8; width * height];
    let mut index = vec![u16::MAX; 256];

    for code in 0u32..=0xFF {
        let Some(ch) = core::char::from_u32(code) else {
            continue;
        };
        let Some(glyph) = crate::vga::get_small_glyph(ch) else {
            continue;
        };
        let slot = code as usize;
        fill_cell(&mut alpha, width, cell_w, cell_h, slot, glyph);
        index[slot] = slot as u16;
    }

    FontAtlasBuffers {
        alpha,
        index,
        widths: Vec::new(),
        width: width as u32,
        height: height as u32,
        cell_w: cell_w as u32,
        cell_h: cell_h as u32,
        grid_w: GRID as u32,
        grid_h: GRID as u32,
    }
}

fn build_font_atlas_large() -> FontAtlasBuffers {
    const GRID: usize = 16;
    let cell_w = crate::vga::BANNER_CELL_W;
    let cell_h = crate::vga::BANNER_CELL_H;
    let width = GRID * cell_w;
    let height = GRID * cell_h;
    let mut alpha = vec![0u8; width * height];
    let mut index = vec![u16::MAX; 256];
    let mut widths = vec![0u8; 256];

    for code in 0u32..=0xFF {
        let Some(ch) = core::char::from_u32(code) else {
            continue;
        };
        let Some((glyph, w)) = crate::vga::get_banner_glyph(ch) else {
            continue;
        };
        let slot = code as usize;
        fill_cell(&mut alpha, width, cell_w, cell_h, slot, glyph);
        index[slot] = slot as u16;
        widths[slot] = w.min(cell_w) as u8;
    }

    FontAtlasBuffers {
        alpha,
        index,
        widths,
        width: width as u32,
        height: height as u32,
        cell_w: cell_w as u32,
        cell_h: cell_h as u32,
        grid_w: GRID as u32,
        grid_h: GRID as u32,
    }
}

fn font_atlas_small() -> &'static FontAtlasBuffers {
    FONT_ATLAS_SMALL.call_once(build_font_atlas_small)
}

fn font_atlas_large() -> &'static FontAtlasBuffers {
    FONT_ATLAS_LARGE.call_once(build_font_atlas_large)
}

#[inline]
fn font_atlas_view_from_buffers(atlas: &'static FontAtlasBuffers) -> FontAtlasView<'static> {
    FontAtlasView {
        alpha: atlas.alpha.as_slice(),
        index: atlas.index.as_slice(),
        widths: atlas.widths.as_slice(),
        width: atlas.width,
        height: atlas.height,
        cell_w: atlas.cell_w,
        cell_h: atlas.cell_h,
        grid_w: atlas.grid_w,
        grid_h: atlas.grid_h,
    }
}

pub fn font_atlas_small_view() -> FontAtlasView<'static> {
    font_atlas_view_from_buffers(font_atlas_small())
}

pub fn font_atlas_large_view() -> FontAtlasView<'static> {
    font_atlas_view_from_buffers(font_atlas_large())
}

fn atlas_rgba() -> &'static [u8] {
    ATLAS_RGBA.call_once(|| {
        let atlas = font_atlas_large_view();
        let tex_px = (atlas.width as usize).saturating_mul(atlas.height as usize);
        let mut tex_rgba = alloc::vec![0u8; tex_px.saturating_mul(4)];
        for (i, &a) in atlas.alpha.iter().enumerate() {
            let o = i.saturating_mul(4);
            tex_rgba[o] = 255;
            tex_rgba[o + 1] = 255;
            tex_rgba[o + 2] = 255;
            tex_rgba[o + 3] = a;
        }
        tex_rgba
    })
}

fn ensure_atlas_uploaded() -> bool {
    if ATLAS_UPLOADED.load(Ordering::Acquire) {
        return true;
    }

    let atlas = font_atlas_large_view();
    let rgba = atlas_rgba();
    let rc = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_upload_texture_rgba(
            ATLAS_TEX_ID,
            atlas.width,
            atlas.height,
            rgba.as_ptr(),
            rgba.len(),
        )
    };
    if rc != 0 {
        return false;
    }
    ATLAS_UPLOADED.store(true, Ordering::Release);
    true
}

fn build_vertices(
    text: &[u8],
    x: f32,
    y: f32,
    view_w: f32,
    view_h: f32,
    alpha: u8,
    out: &mut Vec<TexVertex>,
) {
    if text.is_empty() {
        return;
    }

    let atlas = font_atlas_large_view();
    let grid_w = atlas.grid_w.max(1);
    let atlas_w = atlas.width as f32;
    let atlas_h = atlas.height as f32;
    let fallback = atlas.index.get(b'?' as usize).copied().unwrap_or(0);

    let glyph_advance_px = |ch: u8| {
        let mut slot = atlas.index.get(ch as usize).copied().unwrap_or(fallback);
        if slot == u16::MAX {
            slot = fallback;
        }
        atlas
            .widths
            .get(slot as usize)
            .copied()
            .unwrap_or(atlas.cell_w as u8) as f32
    };

    let mut pen_x = x;
    let mut pen_y = y;

    for &ch in text {
        if ch == b'\n' {
            pen_x = x;
            pen_y += atlas.cell_h as f32;
            continue;
        }
        if ch == b' ' {
            pen_x += glyph_advance_px(ch);
            continue;
        }

        let mut slot = atlas.index.get(ch as usize).copied().unwrap_or(fallback);
        if slot == u16::MAX {
            slot = fallback;
        }

        let glyph_w_px = atlas
            .widths
            .get(slot as usize)
            .copied()
            .unwrap_or(atlas.cell_w as u8) as f32;
        let glyph_h_px = atlas.cell_h as f32;

        let sx = (slot as u32) % grid_w;
        let sy = (slot as u32) / grid_w;
        let px0 = (sx * atlas.cell_w) as f32;
        let py0 = (sy * atlas.cell_h) as f32;
        let u0 = px0 / atlas_w;
        let v0 = py0 / atlas_h;
        let u1 = (px0 + glyph_w_px) / atlas_w;
        let v1 = (py0 + glyph_h_px) / atlas_h;

        let x0 = pen_x;
        let y0 = pen_y;
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

        pen_x += glyph_w_px;
    }
}

pub fn draw_atlas_text_in_frame(text: &[u8], x: f32, y: f32, view_w: u32, view_h: u32) -> bool {
    draw_atlas_text_in_frame_alpha(text, x, y, view_w, view_h, 255)
}

pub fn atlas_text_width_px(text: &[u8]) -> f32 {
    if text.is_empty() {
        return 0.0;
    }

    let atlas = font_atlas_large_view();
    let fallback = atlas.index.get(b'?' as usize).copied().unwrap_or(0);
    let glyph_advance_px = |ch: u8| {
        let mut slot = atlas.index.get(ch as usize).copied().unwrap_or(fallback);
        if slot == u16::MAX {
            slot = fallback;
        }
        atlas
            .widths
            .get(slot as usize)
            .copied()
            .unwrap_or(atlas.cell_w as u8) as f32
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

pub fn draw_atlas_text_in_frame_alpha(
    text: &[u8],
    x: f32,
    y: f32,
    view_w: u32,
    view_h: u32,
    alpha: u8,
) -> bool {
    if text.is_empty() {
        return false;
    }
    if !ensure_atlas_uploaded() {
        return false;
    }

    let _ = unsafe { crate::r::io::cabi::trueos_cabi_gfx_set_sampler(0, 0, 0, 0) };
    let _ = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_set_blend(1, 0x0302, 0x0303, 0x0302, 0x0303, 0, 0)
    };

    let mut verts = Vec::with_capacity(text.len().saturating_mul(6));
    build_vertices(
        text,
        x,
        y,
        view_w.max(1) as f32,
        view_h.max(1) as f32,
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
            ATLAS_TEX_ID,
            ptr,
            len,
        )
    };
    rc == 0
}

pub fn draw_atlas_text(text: &[u8], x: f32, y: f32) -> bool {
    let (view_w, view_h) = crate::limine::framebuffer_response()
        .and_then(|resp| resp.framebuffers().next())
        .map(|fb| (fb.width() as u32, fb.height() as u32))
        .unwrap_or((1024, 768));

    let begin_rc = unsafe { crate::r::io::cabi::trueos_cabi_gfx_begin_frame(0xFFFFFF) };
    if begin_rc != 0 {
        return false;
    }

    let ok = draw_atlas_text_in_frame(text, x, y, view_w, view_h);
    let end_rc = unsafe { crate::r::io::cabi::trueos_cabi_gfx_end_frame() };
    ok && end_rc == 0
}
