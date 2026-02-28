use alloc::vec;
use alloc::vec::Vec;
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
