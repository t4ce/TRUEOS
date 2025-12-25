use font8x8::{UnicodeFonts, BASIC_FONTS};
use spin::Once;

const FONT_W: usize = 8;
const FONT_H: usize = 8;
const CHAR_SPACING: usize = 1;
const DEFAULT_FG_COLOR: u32 = 0x00_FF_FF_FF;
const DEFAULT_BG_COLOR: u32 = 0x00_08_18_30;
const DEFAULT_SHADOW_COLOR: u32 = 0x00_00_00_00;
pub(super) const PINK_FG_COLOR: u32 = 0x00_FF_55_FF;
const BANNER_X: usize = 16;
const BANNER_Y: usize = 8;
const TOP_MARGIN: usize = 50;

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

pub fn init(framebuffers: Option<&'static ::limine::response::FramebufferResponse>) {
    let _ = FRAMEBUFFER.call_once(|| {
        framebuffers
            .and_then(|resp| resp.framebuffers().next())
            .and_then(FramebufferSurface::from_limine)
    });
    let _ = with_framebuffer(|fb| fb.clear(DEFAULT_BG_COLOR));
}

pub fn current_colors() -> Option<(u32, u32, u32)> {
    with_framebuffer(|_| (DEFAULT_FG_COLOR, DEFAULT_BG_COLOR, DEFAULT_SHADOW_COLOR))
}

pub fn render_framebuffer_banner(text: &str) -> bool {
    with_framebuffer(|fb| {
        fb.blit_text(
            text,
            BANNER_X,
            BANNER_Y,
            DEFAULT_FG_COLOR,
            DEFAULT_BG_COLOR,
            DEFAULT_SHADOW_COLOR,
        );
        true
    })
    .unwrap_or(false)
}

pub fn framebuffer_dimensions() -> Option<(u32, u32)> {
    with_framebuffer(|fb| (fb.width as u32, fb.height as u32))
}

pub fn header_height() -> usize {
    TOP_MARGIN
}

fn with_framebuffer<R>(f: impl FnOnce(&FramebufferSurface) -> R) -> Option<R> {
    FRAMEBUFFER.get().and_then(|fb| fb.as_ref()).map(f)
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
        for y in 0..self.height {
            let row_ptr = unsafe { self.addr.add(y.saturating_mul(self.pitch)) as *mut u32 };
            for x in 0..self.width {
                unsafe { row_ptr.add(x).write_volatile(color) };
            }
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
            cursor_x = cursor_x.saturating_add(FONT_W + CHAR_SPACING);
            if cursor_x >= self.width.saturating_sub(FONT_W + CHAR_SPACING) {
                break;
            }
        }
    }

    fn blit_glyph(&self, ch: char, origin_x: usize, origin_y: usize, fg: u32, bg: u32, shadow: u32) {
        let glyph = BASIC_FONTS
            .get(ch)
            .or_else(|| BASIC_FONTS.get('?'))
            .unwrap_or([0; 8]);
        for row in 0..FONT_H {
            let pixel_y = origin_y + row;
            if pixel_y >= self.height {
                continue;
            }
            let bits = glyph[row];
            for col in 0..FONT_W {
                let pixel_x = origin_x + col;
                if pixel_x >= self.width {
                    continue;
                }
                let bit_set = (bits >> col) & 1 == 1;
                let color = if bit_set { fg } else { bg };
                self.write_pixel(pixel_x, pixel_y, color);
                if bit_set {
                    let shadow_x = pixel_x + 1;
                    let shadow_y = pixel_y + 1;
                    if shadow_x < self.width && shadow_y < self.height {
                        self.write_pixel(shadow_x, shadow_y, shadow);
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

    pub(super) fn draw_line(&self, mut x0: i32, mut y0: i32, x1: i32, y1: i32, color: u32) {
        let dx = (x1 - x0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let dy = -(y1 - y0).abs();
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;

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

    pub(super) fn clear_rect(
        &self,
        origin_x: usize,
        origin_y: usize,
        width: usize,
        height: usize,
        color: u32,
    ) {
        let max_x = origin_x.saturating_add(width).min(self.width);
        let max_y = origin_y.saturating_add(height).min(self.height);
        for y in origin_y..max_y {
            for x in origin_x..max_x {
                self.write_pixel(x, y, color);
            }
        }
    }
}

pub fn draw_line(x0: i32, y0: i32, x1: i32, y1: i32, color: u32) {
    let _ = with_framebuffer(|fb| fb.draw_line(x0, y0, x1, y1, color));
}

pub fn clear_rect(origin_x: usize, origin_y: usize, width: usize, height: usize, color: u32) {
    let _ = with_framebuffer(|fb| fb.clear_rect(origin_x, origin_y, width, height, color));
}
