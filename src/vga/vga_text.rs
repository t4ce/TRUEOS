use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use fontdue::{Font, FontSettings};
use spin::Once;

pub const FONT_CELL_W: usize = 6;
pub const FONT_CELL_H: usize = 6;
pub const BANNER_CELL_W: usize = 24;
pub const BANNER_CELL_H: usize = 12;
pub(crate) const BANNER_TEXT: &str = "TRUE OS §";

static FONT_CACHE_SMALL: Once<FontCacheSmall> = Once::new();
static FONT_CACHE_LARGE: Once<FontCacheLarge> = Once::new();
static FONT_READY_SMALL: AtomicBool = AtomicBool::new(false);
static FONT_READY_LARGE: AtomicBool = AtomicBool::new(false);

struct GlyphCell {
    alpha: [u8; FONT_CELL_W * FONT_CELL_H],
}

#[derive(Clone, Copy)]
struct GlyphInkBounds {
    top: i32,
    bottom: i32,
}

#[inline]
fn lowercase_target_bottom_for_cell(ch: char, cell_h: usize) -> Option<i32> {
    if !ch.is_ascii_lowercase() {
        return None;
    }

    let baseline = match ch {
        'g' | 'j' | 'p' | 'q' | 'y' => cell_h as i32 - 1,
        _ => cell_h as i32 - 2,
    };
    Some(baseline)
}

fn bitmap_ink_bounds(width: usize, height: usize, bitmap: &[u8]) -> Option<GlyphInkBounds> {
    let mut top = height as i32;
    let mut bottom = -1i32;

    for y in 0..height {
        let row_off = y * width;
        for x in 0..width {
            if bitmap[row_off + x] == 0 {
                continue;
            }
            let y = y as i32;
            top = top.min(y);
            bottom = bottom.max(y);
        }
    }

    if bottom < top {
        return None;
    }

    Some(GlyphInkBounds { top, bottom })
}

#[inline]
fn glyph_metric_y0(ch: char, cell_h: usize, ymin: i32, ink_bounds: Option<GlyphInkBounds>) -> i32 {
    let ymin = if (ch.is_ascii_uppercase() || ch.is_ascii_digit()) && ymin < 0 {
        0
    } else {
        ymin
    };

    let mut y0 = -ymin;
    if let (Some(bounds), Some(target_bottom)) =
        (ink_bounds, lowercase_target_bottom_for_cell(ch, cell_h))
    {
        let min_y0 = -bounds.top;
        let max_y0 = (cell_h as i32 - 1) - bounds.bottom;
        if min_y0 <= max_y0 {
            y0 = (target_bottom - bounds.bottom).clamp(min_y0, max_y0);
        }
    }

    y0
}

struct FontCacheSmall {
    glyphs: Vec<GlyphCell>,
    index: [u16; 256],
}

impl FontCacheSmall {
    fn lookup(&self, ch: char) -> Option<&GlyphCell> {
        let code = ch as u32;
        if code > 0xFF {
            return None;
        }
        let idx = self.index[code as usize];
        if idx == u16::MAX {
            return None;
        }
        self.glyphs.get(idx as usize)
    }
}

fn font_cache_small() -> &'static FontCacheSmall {
    FONT_CACHE_SMALL.call_once(build_font_cache_small)
}

fn build_font_cache_small() -> FontCacheSmall {
    static FONT_BYTES: &[u8] = include_bytes!("../../lucidasansunicode.ttf");

    let settings = FontSettings {
        scale: FONT_CELL_H as f32,
        ..FontSettings::default()
    };
    let font = Font::from_bytes(FONT_BYTES, settings).expect("lucida font load");

    let mut glyphs = Vec::new();
    let mut index = [u16::MAX; 256];

    fn add_glyph(font: &Font, ch: char, glyphs: &mut Vec<GlyphCell>, index: &mut [u16; 256]) {
        let (metrics, bitmap) = font.rasterize(ch, FONT_CELL_H as f32);
        let mut cell = [0u8; FONT_CELL_W * FONT_CELL_H];
        let ink_bounds = bitmap_ink_bounds(metrics.width, metrics.height, &bitmap);

        let cell_w = FONT_CELL_W as i32;
        let glyph_w = metrics.width as i32;
        let glyph_h = metrics.height as i32;

        if glyph_w > 0 && glyph_h > 0 {
            let x0 = (cell_w - glyph_w) / 2 - metrics.xmin;
            let y0 = glyph_metric_y0(ch, FONT_CELL_H, metrics.ymin, ink_bounds);

            for y in 0..metrics.height {
                for x in 0..metrics.width {
                    let src_idx = y * metrics.width + x;
                    let alpha = bitmap[src_idx];
                    if alpha == 0 {
                        continue;
                    }
                    let cx = x0 + x as i32;
                    let cy = y0 + y as i32;
                    if cx < 0 || cy < 0 {
                        continue;
                    }
                    let cx = cx as usize;
                    let cy = cy as usize;
                    if cx >= FONT_CELL_W || cy >= FONT_CELL_H {
                        continue;
                    }
                    let dst_idx = cy * FONT_CELL_W + cx;
                    let existing = cell[dst_idx];
                    if alpha > existing {
                        cell[dst_idx] = alpha;
                    }
                }
            }
        }

        let slot = glyphs.len() as u16;
        glyphs.push(GlyphCell { alpha: cell });

        if (ch as u32) <= 0xFF {
            index[ch as usize] = slot;
        }
    }

    for code in 0x20u32..=0x7Eu32 {
        if let Some(ch) = core::char::from_u32(code) {
            add_glyph(&font, ch, &mut glyphs, &mut index);
        }
    }
    for code in 0xA0u32..=0xFFu32 {
        if let Some(ch) = core::char::from_u32(code) {
            add_glyph(&font, ch, &mut glyphs, &mut index);
        }
    }

    if index[b'?' as usize] == u16::MAX {
        add_glyph(&font, '?', &mut glyphs, &mut index);
    }
    if index[b' ' as usize] == u16::MAX {
        add_glyph(&font, ' ', &mut glyphs, &mut index);
    }

    FontCacheSmall { glyphs, index }
}

struct GlyphCellLarge {
    alpha: [u8; BANNER_CELL_W * BANNER_CELL_H],
    width: u8,
}

struct FontCacheLarge {
    glyphs: Vec<GlyphCellLarge>,
    index: [u16; 256],
}

impl FontCacheLarge {
    fn lookup(&self, ch: char) -> Option<&GlyphCellLarge> {
        let code = ch as u32;
        if code > 0xFF {
            return None;
        }
        let idx = self.index[code as usize];
        if idx == u16::MAX {
            return None;
        }
        self.glyphs.get(idx as usize)
    }
}

fn font_cache_large() -> &'static FontCacheLarge {
    FONT_CACHE_LARGE.call_once(build_font_cache_large)
}

fn build_font_cache_large() -> FontCacheLarge {
    static FONT_BYTES: &[u8] = include_bytes!("../../lucidasansunicode.ttf");

    let settings = FontSettings {
        scale: BANNER_CELL_H as f32,
        ..FontSettings::default()
    };
    let font = Font::from_bytes(FONT_BYTES, settings).expect("lucida font load");

    let mut glyphs = Vec::new();
    let mut index = [u16::MAX; 256];

    fn add_glyph(font: &Font, ch: char, glyphs: &mut Vec<GlyphCellLarge>, index: &mut [u16; 256]) {
        let (metrics, bitmap) = font.rasterize(ch, BANNER_CELL_H as f32);
        let mut cell = [0u8; BANNER_CELL_W * BANNER_CELL_H];
        let ink_bounds = bitmap_ink_bounds(metrics.width, metrics.height, &bitmap);
        let mut width = (metrics.advance_width + 0.5) as i32;
        width = width.clamp(1, BANNER_CELL_W as i32);
        let mut max_ink_x: i32 = -1;

        let glyph_w = metrics.width as i32;
        let glyph_h = metrics.height as i32;
        if glyph_w > 0 && glyph_h > 0 {
            let x0 = (-metrics.xmin).max(0);
            let y0 = glyph_metric_y0(ch, BANNER_CELL_H, metrics.ymin, ink_bounds);
            for y in 0..metrics.height {
                for x in 0..metrics.width {
                    let src_idx = y * metrics.width + x;
                    let alpha = bitmap[src_idx];
                    if alpha == 0 {
                        continue;
                    }
                    let cx = x0 + x as i32;
                    let cy = y0 + y as i32;
                    if cx < 0 || cy < 0 {
                        continue;
                    }
                    let cx = cx as usize;
                    let cy = cy as usize;
                    if cx >= BANNER_CELL_W || cy >= BANNER_CELL_H {
                        continue;
                    }
                    let dst_idx = cy * BANNER_CELL_W + cx;
                    let existing = cell[dst_idx];
                    if alpha > existing {
                        cell[dst_idx] = alpha;
                    }
                    max_ink_x = max_ink_x.max(cx as i32);
                }
            }
        }

        if max_ink_x >= 0 {
            width = width.max(max_ink_x + 1).clamp(1, BANNER_CELL_W as i32);
        }

        let width = width as u8;
        let slot = glyphs.len() as u16;
        glyphs.push(GlyphCellLarge { alpha: cell, width });
        if (ch as u32) <= 0xFF {
            index[ch as usize] = slot;
        }
    }

    for code in 0x20u32..=0x7Eu32 {
        if let Some(ch) = core::char::from_u32(code) {
            add_glyph(&font, ch, &mut glyphs, &mut index);
        }
    }
    for code in 0xA0u32..=0xFFu32 {
        if let Some(ch) = core::char::from_u32(code) {
            add_glyph(&font, ch, &mut glyphs, &mut index);
        }
    }

    if index[b'?' as usize] == u16::MAX {
        add_glyph(&font, '?', &mut glyphs, &mut index);
    }
    if index[b' ' as usize] == u16::MAX {
        add_glyph(&font, ' ', &mut glyphs, &mut index);
    }

    FontCacheLarge { glyphs, index }
}

pub fn init_font_cache() {
    let _ = font_cache_small();
    let _ = font_cache_large();
    FONT_READY_SMALL.store(true, Ordering::Release);
    FONT_READY_LARGE.store(true, Ordering::Release);
}

#[embassy_executor::task]
pub(crate) async fn init_font_cache_task() {
    async move {
        init_font_cache();
    }
    .await;
}

pub(crate) fn small_font_ready() -> bool {
    FONT_READY_SMALL.load(Ordering::Acquire)
}

pub(crate) fn large_font_ready() -> bool {
    FONT_READY_LARGE.load(Ordering::Acquire)
}

pub fn get_banner_glyph(ch: char) -> Option<(&'static [u8], usize)> {
    font_cache_large()
        .lookup(ch)
        .map(|g| (&g.alpha[..], g.width as usize))
}

pub fn get_small_glyph(ch: char) -> Option<&'static [u8]> {
    font_cache_small().lookup(ch).map(|g| &g.alpha[..])
}

pub fn get_logo_buffer() -> (Vec<u32>, usize, usize) {
    let text = BANNER_TEXT;
    let height = BANNER_CELL_H;
    let mut glyph_runs: Vec<(&'static [u8], usize, u32)> = Vec::with_capacity(text.len());
    let mut total_width = 0;

    for ch in text.chars() {
        if let Some((alpha, w)) = get_banner_glyph(ch) {
            let rgb = if ch == '§' {
                0x00_FF_37_FF
            } else {
                0x00_FF_FF_FF
            };
            glyph_runs.push((alpha, w, rgb));
            total_width += w + 1;
        }
    }
    total_width = total_width.saturating_sub(1);

    let mut buffer = vec![0_u32; total_width * height];
    let mut current_x = 0;

    for (alpha, w, rgb) in glyph_runs {
        for y in 0..height {
            for x in 0..w {
                let val = alpha[y * BANNER_CELL_W + x];
                if val > 0 {
                    let dest_x = current_x + x;
                    if dest_x < total_width {
                        let color_u32 = ((val as u32) << 24) | rgb;
                        buffer[y * total_width + dest_x] = color_u32;
                    }
                }
            }
        }
        current_x += w + 1;
    }

    (buffer, total_width, height)
}
