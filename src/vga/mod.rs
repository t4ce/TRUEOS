pub mod cube;
use fontdue::{Font, FontSettings};
use libm::{cosf, roundf, sinf};
use spin::Once;

// NOTE: VGA is immediate-mode into the Limine framebuffer.

use alloc::vec::Vec;
use core::f32::consts::PI;
use core::fmt;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

pub struct Image<'a> {
    pub width: usize,
    pub height: usize,
    pub pixels: &'a [u32],
}

pub const FONT_CELL_W: usize = 6;
pub const FONT_CELL_H: usize = 6;
pub const BANNER_CELL_W: usize = 24;
pub const BANNER_CELL_H: usize = 12;
const FONT_W: usize = FONT_CELL_W;
const FONT_H: usize = FONT_CELL_H;
const DEFAULT_FG_COLOR: u32 = 0x00_FF_FF_FF;
const DEFAULT_BG_COLOR: u32 = 0x00_08_18_30;
pub const DEFAULT_SHADOW_COLOR: u32 = 0x00_00_00_00;
const BANNER_TEXT: &str = "TRUE OS §";
const DEFAULT_TOP_MARGIN: usize = 50;

pub(super) struct FramebufferSurface {
    addr: *mut u8,
    pitch: usize,
    bytes_per_pixel: usize,
    pub(super) width: usize,
    pub(super) height: usize,
}

unsafe impl Send for FramebufferSurface {}
unsafe impl Sync for FramebufferSurface {}

static FRAMEBUFFER: Once<Option<FramebufferSurface>> = Once::new();
static FONT_CACHE_SMALL: Once<FontCacheSmall> = Once::new();
static FONT_CACHE_LARGE: Once<FontCacheLarge> = Once::new();
static FONT_READY_SMALL: AtomicBool = AtomicBool::new(false);
static FONT_READY_LARGE: AtomicBool = AtomicBool::new(false);
static TOP_MARGIN: AtomicUsize = AtomicUsize::new(DEFAULT_TOP_MARGIN);
static LOG_NEXT_Y: AtomicUsize = AtomicUsize::new(DEFAULT_TOP_MARGIN);
static LOG_CUR_X: AtomicUsize = AtomicUsize::new(0);

#[inline]
pub fn vga_swapped() -> bool {
    // Temporary override: keep VGA on direct Limine framebuffer writes.
    false
}

pub fn restore_vga_from_gfx_backbuffer() -> bool {
    with_framebuffer(|fb| {
        crate::gfx::with_cpu_backbuffer_mut(|pixels, bw, bh| {
            fb.blit_from_cpu_backbuffer(pixels, bw, bh);
        })
        .is_some()
    })
    .unwrap_or(false)
}

fn log_advance_line(fb_height: usize, current_y: usize) -> usize {
    let start_y = TOP_MARGIN.load(Ordering::Relaxed);
    let mut next = current_y.saturating_add(FONT_H);
    if next.saturating_add(FONT_H) > fb_height {
        next = start_y;
    }
    next
}

struct VgaLogWriter {
    fg: u32,
    bg: u32,
    shadow: u32,
}

impl fmt::Write for VgaLogWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if log_colored(s, self.fg, self.bg, self.shadow) {
            Ok(())
        } else {
            Err(fmt::Error)
        }
    }
}

pub fn init(framebuffers: Option<&'static ::limine::response::FramebufferResponse>) {
    let _ = FRAMEBUFFER.call_once(|| {
        framebuffers
            .and_then(|resp| resp.framebuffers().next())
            .and_then(FramebufferSurface::from_limine)
    });
    let _ = with_framebuffer(|fb| {
        update_layout(fb);
        fb.clear(DEFAULT_BG_COLOR)
    });
    render_framebuffer_banner(BANNER_TEXT); // the one true operating system
}

pub fn init_font_cache() {
    let _ = font_cache_small();
    let _ = font_cache_large();
    FONT_READY_SMALL.store(true, Ordering::Release);
    FONT_READY_LARGE.store(true, Ordering::Release);
    let _ = with_framebuffer(update_layout);
    render_framebuffer_banner(BANNER_TEXT);
}

#[embassy_executor::task]
pub(crate) async fn init_font_cache_task() {
    async move {
        init_font_cache();
    }
    .await;
}

pub fn current_colors() -> Option<(u32, u32, u32)> {
    with_framebuffer(|_| (DEFAULT_FG_COLOR, DEFAULT_BG_COLOR, DEFAULT_SHADOW_COLOR))
}

fn render_framebuffer_banner(text: &str) -> bool {
    with_framebuffer(|fb| {
        update_layout(fb);
        fb.blit_text_large(
            text,
            1,
            1,
            DEFAULT_FG_COLOR,
            DEFAULT_BG_COLOR,
            DEFAULT_SHADOW_COLOR,
        );
        true
    })
    .unwrap_or(false)
}

fn log_colored(text: &str, fg: u32, bg: u32, shadow: u32) -> bool {
    with_framebuffer(|fb| {
        let start_y = TOP_MARGIN.load(Ordering::Relaxed);

        let mut y = LOG_NEXT_Y.load(Ordering::Relaxed);
        if y < start_y {
            y = start_y;
        }
        if y.saturating_add(FONT_H) > fb.height {
            y = start_y;
        }

        for chunk in text.split_inclusive('\n') {
            let (part, has_nl) = match chunk.strip_suffix('\n') {
                Some(p) => (p, true),
                None => (chunk, false),
            };

            if !part.is_empty() {
                let x = LOG_CUR_X.load(Ordering::Relaxed).min(fb.width);
                fb.blit_text(part, x, y, fg, bg, shadow);

                let advance = part.chars().count().saturating_mul(FONT_W);
                LOG_CUR_X.store(x.saturating_add(advance).min(fb.width), Ordering::Relaxed);
            }

            if has_nl {
                y = log_advance_line(fb.height, y);
                LOG_NEXT_Y.store(y, Ordering::Relaxed);
                LOG_CUR_X.store(0, Ordering::Relaxed);
            }
        }

        true
    })
    .unwrap_or(false)
}

pub fn log(args: fmt::Arguments<'_>) -> bool {
    let (fg, bg, shadow) =
        current_colors().unwrap_or((DEFAULT_FG_COLOR, DEFAULT_BG_COLOR, DEFAULT_SHADOW_COLOR));
    let mut w = VgaLogWriter { fg, bg, shadow };
    fmt::write(&mut w, args).is_ok()
}

pub fn framebuffer_dimensions() -> Option<(u32, u32)> {
    with_framebuffer(|fb| (fb.width as u32, fb.height as u32))
}

pub fn draw_header_square(
    total_slots: usize,
    slot: usize,
    color: u32,
    outline_color: u32,
    degree: u32,
) -> bool {
    let square_side_length: usize = 25;
    if total_slots == 0 {
        return false;
    }
    with_framebuffer(|fb| {
        let slot = slot % total_slots;
        let total_width = square_side_length.saturating_mul(total_slots);
        let left = (fb.width as isize / 2) - (total_width as isize / 2);
        let origin_x = left + (square_side_length as isize * slot as isize);
        let origin_x = origin_x as usize;

        fb.clear_rect(origin_x, 0, square_side_length, square_side_length, color);

        let side = square_side_length - 10;
        if side > 0 {
            let cx = origin_x as f32 + (square_side_length as f32) * 0.5;
            let cy = (square_side_length as f32) * 0.5;
            let half = (side as f32) * 0.5;

            let angle = ((degree % 360) as f32) * (PI / 180.0);
            let (c, s) = (cosf(angle), sinf(angle));

            let corners = [(-half, -half), (half, -half), (half, half), (-half, half)];

            let mut pts = [(0_i32, 0_i32); 4];
            for (i, (dx, dy)) in corners.into_iter().enumerate() {
                let rx = dx * c - dy * s;
                let ry = dx * s + dy * c;
                pts[i] = (roundf(cx + rx) as i32, roundf(cy + ry) as i32);
            }
            for i in 0..4 {
                let (x0, y0) = pts[i];
                let (x1, y1) = pts[(i + 1) % 4];
                fb.draw_line(x0, y0, x1, y1, outline_color);
            }
        }

        true
    })
    .unwrap_or(false)
}

fn with_framebuffer<R>(f: impl FnOnce(&FramebufferSurface) -> R) -> Option<R> {
    FRAMEBUFFER.get().and_then(|fb| fb.as_ref()).map(f)
}

fn update_layout(fb: &FramebufferSurface) {
    let top_margin = (1 + BANNER_CELL_H + 6).min(fb.height.saturating_sub(1));
    TOP_MARGIN.store(top_margin.max(1), Ordering::Relaxed);
    LOG_NEXT_Y.store(top_margin.max(1), Ordering::Relaxed);
    LOG_CUR_X.store(0, Ordering::Relaxed);
}

impl FramebufferSurface {
    fn from_limine(fb: ::limine::framebuffer::Framebuffer<'static>) -> Option<FramebufferSurface> {
        use ::limine::framebuffer::MemoryModel;

        if fb.memory_model() != MemoryModel::RGB {
            return None;
        }
        let bpp = fb.bpp();
        if bpp != 32 {
            return None;
        }
        Some(FramebufferSurface {
            addr: fb.addr(),
            pitch: fb.pitch() as usize,
            bytes_per_pixel: (bpp / 8) as usize,
            width: fb.width() as usize,
            height: fb.height() as usize,
        })
    }

    fn clear(&self, color: u32) {
        if vga_swapped() {
            let _ = crate::gfx::with_cpu_backbuffer_mut(|pixels, bw, bh| {
                let copy_w = self.width.min(bw);
                let copy_h = self.height.min(bh);
                for y in 0..copy_h {
                    let off = y.saturating_mul(bw);
                    pixels[off..off + copy_w].fill(color);
                }
            });
            return;
        }

        if self.bytes_per_pixel != 4 || self.width == 0 || self.height == 0 {
            return;
        }

        for y in 0..self.height {
            let row_ptr = unsafe { self.addr.add(y.saturating_mul(self.pitch)) as *mut u32 };
            // Safety: Limine framebuffer is 32bpp RGB and rows are at least `width * 4` bytes.
            // We only touch the visible `width` pixels, not the full `pitch`.
            let row = unsafe { core::slice::from_raw_parts_mut(row_ptr, self.width) };
            row.fill(color);
        }
    }

    fn blit_from_cpu_backbuffer(&self, src: &[u32], src_w: usize, src_h: usize) {
        if self.bytes_per_pixel != 4 || self.width == 0 || self.height == 0 {
            return;
        }
        let copy_w = self.width.min(src_w);
        let copy_h = self.height.min(src_h);
        if copy_w == 0 || copy_h == 0 {
            return;
        }
        for y in 0..copy_h {
            let src_off = y.saturating_mul(src_w);
            let src_row = &src[src_off..src_off + copy_w];
            let dst_row_ptr = unsafe { self.addr.add(y.saturating_mul(self.pitch)) as *mut u32 };
            let dst_row = unsafe { core::slice::from_raw_parts_mut(dst_row_ptr, self.width) };
            dst_row[..copy_w].copy_from_slice(src_row);
        }
    }
    fn blit_text(
        &self,
        text: &str,
        origin_x: usize,
        origin_y: usize,
        fg: u32,
        bg: u32,
        shadow: u32,
    ) {
        let mut cursor_x = origin_x;
        for ch in text.chars() {
            self.blit_glyph(ch, cursor_x, origin_y, fg, bg, shadow);
            cursor_x = cursor_x.saturating_add(FONT_W);
            if cursor_x >= self.width.saturating_sub(FONT_W) {
                break;
            }
        }
    }

    fn blit_text_large(
        &self,
        text: &str,
        origin_x: usize,
        origin_y: usize,
        fg: u32,
        bg: u32,
        shadow: u32,
    ) {
        if !FONT_READY_LARGE.load(Ordering::Acquire) {
            return;
        }
        let cache = font_cache_large();
        let mut cursor_x = origin_x;
        for ch in text.chars() {
            let glyph = cache
                .lookup(ch)
                .or_else(|| cache.lookup('?'))
                .or_else(|| cache.lookup(' '));
            let Some(glyph) = glyph else {
                continue;
            };

            self.blit_glyph_large_cell(glyph, cursor_x, origin_y, fg, bg, shadow);
            let width = glyph.width as usize;
            cursor_x = cursor_x.saturating_add(width);
            if cursor_x >= self.width.saturating_sub(BANNER_CELL_W) {
                break;
            }
        }
    }

    fn blit_glyph(
        &self,
        ch: char,
        origin_x: usize,
        origin_y: usize,
        fg: u32,
        bg: u32,
        shadow: u32,
    ) {
        if !FONT_READY_SMALL.load(Ordering::Acquire) {
            return;
        }
        let swapped = vga_swapped();
        let cache = font_cache_small();

        // Fast path: default log palette uses a precolored 6x6 cache.
        // Keep swapped-mode writes routed through `write_pixel`.
        if !swapped
            && fg == DEFAULT_FG_COLOR
            && bg == DEFAULT_BG_COLOR
            && shadow == DEFAULT_SHADOW_COLOR
        {
            let glyph = cache
                .lookup_colored(ch)
                .or_else(|| cache.lookup_colored('?'))
                .or_else(|| cache.lookup_colored(' '));
            let Some(glyph) = glyph else {
                return;
            };

            if origin_x >= self.width || origin_y >= self.height {
                return;
            }

            let copy_w = FONT_CELL_W.min(self.width - origin_x);
            let copy_h = FONT_CELL_H.min(self.height - origin_y);
            if copy_w == 0 || copy_h == 0 {
                return;
            }

            // Shadow is offset by (1,1) and only applies where alpha>0.
            // IMPORTANT: draw shadow first, then foreground. The old per-pixel implementation
            // would eventually overwrite any shadow pixels that overlap foreground pixels.
            for row in 0..copy_h {
                let sh_y = origin_y + row + 1;
                if sh_y >= self.height {
                    break;
                }
                let row_ptr = unsafe { self.addr.add(sh_y.saturating_mul(self.pitch)) as *mut u32 };
                let dst_row = unsafe { core::slice::from_raw_parts_mut(row_ptr, self.width) };

                for col in 0..copy_w {
                    let idx = row * FONT_CELL_W + col;
                    if (glyph.mask >> (idx as u64)) & 1 == 0 {
                        continue;
                    }
                    let sh_x = origin_x + col + 1;
                    if sh_x >= self.width {
                        break;
                    }
                    dst_row[sh_x] = glyph.sh[idx];
                }
            }

            // Copy the pre-blended foreground cell (includes background).
            for row in 0..copy_h {
                let dst_y = origin_y + row;
                let row_ptr =
                    unsafe { self.addr.add(dst_y.saturating_mul(self.pitch)) as *mut u32 };
                let dst_row = unsafe { core::slice::from_raw_parts_mut(row_ptr, self.width) };

                let src_off = row * FONT_CELL_W;
                dst_row[origin_x..origin_x + copy_w]
                    .copy_from_slice(&glyph.fg[src_off..src_off + copy_w]);
            }

            return;
        }

        let glyph = cache
            .lookup(ch)
            .or_else(|| cache.lookup('?'))
            .or_else(|| cache.lookup(' '));
        let Some(glyph) = glyph else {
            return;
        };

        if swapped {
            for row in 0..FONT_CELL_H {
                let pixel_y = origin_y + row;
                if pixel_y >= self.height {
                    continue;
                }
                for col in 0..FONT_CELL_W {
                    let pixel_x = origin_x + col;
                    if pixel_x >= self.width {
                        continue;
                    }
                    let alpha = glyph.alpha[row * FONT_CELL_W + col];
                    let color = Self::blend_color(bg, fg, alpha);
                    self.write_pixel_swapped(pixel_x, pixel_y, color);
                    if alpha > 0 {
                        let shadow_x = pixel_x + 1;
                        let shadow_y = pixel_y + 1;
                        if shadow_x < self.width && shadow_y < self.height {
                            let shadow_color = Self::blend_color(bg, shadow, alpha);
                            self.write_pixel_swapped(shadow_x, shadow_y, shadow_color);
                        }
                    }
                }
            }
        } else {
            for row in 0..FONT_CELL_H {
                let pixel_y = origin_y + row;
                if pixel_y >= self.height {
                    continue;
                }
                for col in 0..FONT_CELL_W {
                    let pixel_x = origin_x + col;
                    if pixel_x >= self.width {
                        continue;
                    }
                    let alpha = glyph.alpha[row * FONT_CELL_W + col];
                    let color = Self::blend_color(bg, fg, alpha);
                    self.write_pixel(pixel_x, pixel_y, color);
                    if alpha > 0 {
                        let shadow_x = pixel_x + 1;
                        let shadow_y = pixel_y + 1;
                        if shadow_x < self.width && shadow_y < self.height {
                            let shadow_color = Self::blend_color(bg, shadow, alpha);
                            self.write_pixel(shadow_x, shadow_y, shadow_color);
                        }
                    }
                }
            }
        }
    }

    fn blit_glyph_large_cell(
        &self,
        glyph: &GlyphCellLarge,
        origin_x: usize,
        origin_y: usize,
        fg: u32,
        bg: u32,
        shadow: u32,
    ) {
        if vga_swapped() {
            for row in 0..BANNER_CELL_H {
                let pixel_y = origin_y + row;
                if pixel_y >= self.height {
                    continue;
                }
                for col in 0..BANNER_CELL_W {
                    let pixel_x = origin_x + col;
                    if pixel_x >= self.width {
                        continue;
                    }
                    let alpha = glyph.alpha[row * BANNER_CELL_W + col];
                    let color = Self::blend_color(bg, fg, alpha);
                    self.write_pixel_swapped(pixel_x, pixel_y, color);
                    if alpha > 0 {
                        let shadow_x = pixel_x + 1;
                        let shadow_y = pixel_y + 1;
                        if shadow_x < self.width && shadow_y < self.height {
                            let shadow_color = Self::blend_color(bg, shadow, alpha);
                            self.write_pixel_swapped(shadow_x, shadow_y, shadow_color);
                        }
                    }
                }
            }
        } else {
            for row in 0..BANNER_CELL_H {
                let pixel_y = origin_y + row;
                if pixel_y >= self.height {
                    continue;
                }
                for col in 0..BANNER_CELL_W {
                    let pixel_x = origin_x + col;
                    if pixel_x >= self.width {
                        continue;
                    }
                    let alpha = glyph.alpha[row * BANNER_CELL_W + col];
                    let color = Self::blend_color(bg, fg, alpha);
                    self.write_pixel(pixel_x, pixel_y, color);
                    if alpha > 0 {
                        let shadow_x = pixel_x + 1;
                        let shadow_y = pixel_y + 1;
                        if shadow_x < self.width && shadow_y < self.height {
                            let shadow_color = Self::blend_color(bg, shadow, alpha);
                            self.write_pixel(shadow_x, shadow_y, shadow_color);
                        }
                    }
                }
            }
        }
    }

    fn write_pixel(&self, x: usize, y: usize, color: u32) {
        let offset = y
            .saturating_mul(self.pitch)
            .saturating_add(x.saturating_mul(self.bytes_per_pixel));
        unsafe {
            core::ptr::write_volatile(self.addr.add(offset) as *mut u32, color);
        }
    }

    fn write_pixel_swapped(&self, x: usize, y: usize, color: u32) {
        let _ = crate::gfx::with_cpu_backbuffer_mut(|pixels, w, h| {
            if x < w && y < h {
                let off = y.saturating_mul(w).saturating_add(x);
                pixels[off] = color;
            }
        });
    }

    fn plot_if_visible(&self, x: i32, y: i32, color: u32) {
        if x < 0 || y < 0 {
            return;
        }
        let (xu, yu) = (x as usize, y as usize);
        if xu >= self.width || yu >= self.height {
            return;
        }
        self.write_pixel(xu, yu, color);
    }

    fn plot_if_visible_swapped(&self, x: i32, y: i32, color: u32) {
        if x < 0 || y < 0 {
            return;
        }
        let (xu, yu) = (x as usize, y as usize);
        if xu >= self.width || yu >= self.height {
            return;
        }
        self.write_pixel_swapped(xu, yu, color);
    }

    pub(super) fn plot(&self, x: i32, y: i32, color: u32) {
        self.plot_if_visible(x, y, color);
    }

    pub(super) fn plot_swapped(&self, x: i32, y: i32, color: u32) {
        self.plot_if_visible_swapped(x, y, color);
    }

    #[inline]
    fn blend_color(bg: u32, fg: u32, alpha: u8) -> u32 {
        if alpha == 0 {
            return bg;
        }
        if alpha == 0xFF {
            return fg;
        }
        let a = alpha as u32;
        let inv = 0xFFu32.saturating_sub(a);

        let br = (bg >> 16) & 0xFF;
        let bgc = (bg >> 8) & 0xFF;
        let bb = bg & 0xFF;

        let fr = (fg >> 16) & 0xFF;
        let fg_c = (fg >> 8) & 0xFF;
        let fb = fg & 0xFF;

        let r = (fr * a + br * inv) / 0xFF;
        let g = (fg_c * a + bgc * inv) / 0xFF;
        let b = (fb * a + bb * inv) / 0xFF;

        (r << 16) | (g << 8) | b
    }
    pub(super) fn draw_line(&self, mut x0: i32, mut y0: i32, x1: i32, y1: i32, color: u32) {
        let dx = (x1 - x0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let dy = -(y1 - y0).abs();
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;

        if vga_swapped() {
            loop {
                self.plot_if_visible_swapped(x0, y0, color);
                if x0 == x1 && y0 == y1 {
                    break;
                }
                let e2 = err.saturating_mul(2);
                if e2 >= dy {
                    err += dy;
                    x0 = x0.saturating_add(sx);
                }
                if e2 <= dx {
                    err += dx;
                    y0 = y0.saturating_add(sy);
                }
            }
        } else {
            loop {
                self.plot_if_visible(x0, y0, color);
                if x0 == x1 && y0 == y1 {
                    break;
                }
                let e2 = err.saturating_mul(2);
                if e2 >= dy {
                    err += dy;
                    x0 = x0.saturating_add(sx);
                }
                if e2 <= dx {
                    err += dx;
                    y0 = y0.saturating_add(sy);
                }
            }
        }
    }

    pub(super) fn clear_rect(
        &self,
        origin_x: usize,
        origin_y: usize,
        width: usize,
        height: usize,
        color: u32,
    ) {
        if vga_swapped() {
            let _ = crate::gfx::with_cpu_backbuffer_mut(|pixels, bw, bh| {
                let max_x = origin_x.saturating_add(width).min(self.width).min(bw);
                let max_y = origin_y.saturating_add(height).min(self.height).min(bh);
                if origin_x >= max_x || origin_y >= max_y {
                    return;
                }
                for y in origin_y..max_y {
                    let off = y.saturating_mul(bw);
                    pixels[off + origin_x..off + max_x].fill(color);
                }
            });
            return;
        }

        if self.bytes_per_pixel != 4 {
            return;
        }
        let max_x = origin_x.saturating_add(width).min(self.width);
        let max_y = origin_y.saturating_add(height).min(self.height);
        if origin_x >= max_x || origin_y >= max_y {
            return;
        }

        for y in origin_y..max_y {
            let row_ptr = unsafe { self.addr.add(y.saturating_mul(self.pitch)) as *mut u32 };
            // Safety: same as `clear()`; we only touch the visible `width` pixels.
            let row = unsafe { core::slice::from_raw_parts_mut(row_ptr, self.width) };
            row[origin_x..max_x].fill(color);
        }
    }

    fn blit_image(&self, origin_x: usize, origin_y: usize, image: &Image<'_>) {
        if image.width == 0 || image.height == 0 {
            return;
        }
        let expected = image.width.saturating_mul(image.height);
        if image.pixels.len() < expected {
            return;
        }

        if origin_x >= self.width || origin_y >= self.height {
            return;
        }

        let copy_w = image.width.min(self.width - origin_x);
        let copy_h = image.height.min(self.height - origin_y);
        if copy_w == 0 || copy_h == 0 {
            return;
        }

        if vga_swapped() {
            let _ = crate::gfx::with_cpu_backbuffer_mut(|pixels, bw, bh| {
                if origin_x >= bw || origin_y >= bh {
                    return;
                }
                let routed_w = copy_w.min(bw - origin_x);
                let routed_h = copy_h.min(bh - origin_y);
                if routed_w == 0 || routed_h == 0 {
                    return;
                }
                for y in 0..routed_h {
                    let dst_y = origin_y + y;
                    let src_off = y * image.width;
                    let src_row = &image.pixels[src_off..src_off + routed_w];
                    let dst_off = dst_y.saturating_mul(bw);
                    pixels[dst_off + origin_x..dst_off + origin_x + routed_w]
                        .copy_from_slice(src_row);
                }
            });
            return;
        }

        if self.bytes_per_pixel != 4 {
            return;
        }

        for y in 0..copy_h {
            let dst_y = origin_y + y;
            let dst_row_ptr =
                unsafe { self.addr.add(dst_y.saturating_mul(self.pitch)) as *mut u32 };
            let dst_row = unsafe { core::slice::from_raw_parts_mut(dst_row_ptr, self.width) };

            let src_off = y * image.width;
            let src_row = &image.pixels[src_off..src_off + copy_w];
            dst_row[origin_x..origin_x + copy_w].copy_from_slice(src_row);
        }
    }
}

struct GlyphCell {
    alpha: [u8; FONT_CELL_W * FONT_CELL_H],
}

// Precolored glyph cells for the small 6x6 font for the default VGA log colors.
//
// This avoids per-pixel alpha blending in the hot path. We keep the original alpha
// cache because it is also used for the font atlas export.
struct GlyphCellColoredSmall {
    // Foreground (already blended over DEFAULT_BG_COLOR for each alpha).
    fg: [u32; FONT_CELL_W * FONT_CELL_H],
    // Shadow (already blended over DEFAULT_BG_COLOR for each alpha).
    sh: [u32; FONT_CELL_W * FONT_CELL_H],
    // Bitmask of pixels with alpha>0 (LSB is cell[0]). Used to draw shadow only where needed.
    mask: u64,
}

struct FontCacheSmall {
    glyphs: Vec<GlyphCell>,
    colored: Vec<GlyphCellColoredSmall>,
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

    fn lookup_colored(&self, ch: char) -> Option<&GlyphCellColoredSmall> {
        let code = ch as u32;
        if code > 0xFF {
            return None;
        }
        let idx = self.index[code as usize];
        if idx == u16::MAX {
            return None;
        }
        self.colored.get(idx as usize)
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
    let mut colored = Vec::new();
    let mut index = [u16::MAX; 256];

    fn add_glyph(
        font: &Font,
        ch: char,
        glyphs: &mut Vec<GlyphCell>,
        colored: &mut Vec<GlyphCellColoredSmall>,
        index: &mut [u16; 256],
    ) {
        let (metrics, bitmap) = font.rasterize(ch, FONT_CELL_H as f32);
        let mut cell = [0u8; FONT_CELL_W * FONT_CELL_H];

        let cell_w = FONT_CELL_W as i32;
        let cell_h = FONT_CELL_H as i32;
        let glyph_w = metrics.width as i32;
        let glyph_h = metrics.height as i32;

        if glyph_w > 0 && glyph_h > 0 {
            let x0 = (cell_w - glyph_w) / 2 - metrics.xmin;
            let y0 = (cell_h - glyph_h) / 2 - metrics.ymin - 1;

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

        // Build the precolored cell for the default VGA log palette.
        let mut fg_px = [DEFAULT_BG_COLOR; FONT_CELL_W * FONT_CELL_H];
        let mut sh_px = [DEFAULT_BG_COLOR; FONT_CELL_W * FONT_CELL_H];
        let mut mask: u64 = 0;
        for (i, &a) in cell.iter().enumerate() {
            let a = a;
            fg_px[i] = FramebufferSurface::blend_color(DEFAULT_BG_COLOR, DEFAULT_FG_COLOR, a);
            sh_px[i] = FramebufferSurface::blend_color(DEFAULT_BG_COLOR, DEFAULT_SHADOW_COLOR, a);
            if a != 0 {
                mask |= 1u64 << (i as u64);
            }
        }
        colored.push(GlyphCellColoredSmall {
            fg: fg_px,
            sh: sh_px,
            mask,
        });

        if (ch as u32) <= 0xFF {
            index[ch as usize] = slot;
        }
    }

    for code in 0x20u32..=0x7Eu32 {
        if let Some(ch) = core::char::from_u32(code) {
            add_glyph(&font, ch, &mut glyphs, &mut colored, &mut index);
        }
    }
    for code in 0xA0u32..=0xFFu32 {
        if let Some(ch) = core::char::from_u32(code) {
            add_glyph(&font, ch, &mut glyphs, &mut colored, &mut index);
        }
    }

    if index[b'?' as usize] == u16::MAX {
        add_glyph(&font, '?', &mut glyphs, &mut colored, &mut index);
    }
    if index[b' ' as usize] == u16::MAX {
        add_glyph(&font, ' ', &mut glyphs, &mut colored, &mut index);
    }

    FontCacheSmall {
        glyphs,
        colored,
        index,
    }
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

    let mut buffer = alloc::vec![0_u32; total_width * height];
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
        let mut width = (metrics.advance_width + 0.5) as i32;
        width = width.clamp(1, BANNER_CELL_W as i32);
        let width = width as u8;

        let _cell_h = BANNER_CELL_H as i32;
        let glyph_w = metrics.width as i32;
        let glyph_h = metrics.height as i32;

        if glyph_w > 0 && glyph_h > 0 {
            let x0 = (-metrics.xmin).max(0);
            // Baseline anchor only: height-centering biases shorter lowercase
            // glyphs downward compared to uppercase.
            let y0 = -metrics.ymin - 1;

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
                }
            }
        }

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

pub fn blit_image(origin_x: usize, origin_y: usize, image: &Image<'_>) -> bool {
    with_framebuffer(|fb| {
        // Clamp origin so the image fits inside the framebuffer when possible.
        let mut ox = origin_x;
        let mut oy = origin_y;
        if image.width <= fb.width && ox > fb.width - image.width {
            ox = fb.width - image.width;
        }
        if image.height <= fb.height && oy > fb.height - image.height {
            oy = fb.height - image.height;
        }
        fb.blit_image(ox, oy, image);
        true
    })
    .unwrap_or(false)
}

static RENDER_MANDELBROT_ONCE: Once<()> = Once::new();

const MANDELBROT_W: usize = 256;
const MANDELBROT_H: usize = 256;

#[unsafe(link_section = ".bss")]
static mut MANDELBROT_PIXELS: [u32; MANDELBROT_W * MANDELBROT_H] = [0; MANDELBROT_W * MANDELBROT_H];

pub(crate) fn draw_mandelbrot() {
    let Some((fb_w, fb_h)) = framebuffer_dimensions() else {
        return;
    };

    let fb_w = fb_w as usize;
    let fb_h = fb_h as usize;
    let w = MANDELBROT_W;
    let h = MANDELBROT_H;
    let expected = w * h;

    // Rendering is the expensive part; do it once.
    RENDER_MANDELBROT_ONCE.call_once(|| unsafe {
        trueos_math::render_mandelbrot_rgb32(&mut MANDELBROT_PIXELS[..expected], w, h, 64);
    });

    unsafe {
        let img = Image {
            width: w,
            height: h,
            pixels: &MANDELBROT_PIXELS[..expected],
        };

        let (origin_x, origin_y) = match crate::efi::acpi::bgrt::last_logo_rect() {
            Some((logo_x, logo_y, logo_w, _logo_h)) => {
                let x = logo_x.saturating_add(logo_w).saturating_sub(w);
                let y = logo_y.saturating_sub(h);
                (x, y)
            }
            None => (fb_w, fb_h),
        };
        let _ = blit_image(origin_x, origin_y, &img);
    }
}
