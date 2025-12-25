use core::sync::atomic::{AtomicUsize, Ordering};

use font8x8::{UnicodeFonts, BASIC_FONTS};
use spin::Mutex;

use crate::{debugconf, phys};

const FONT_W: usize = 8;
const FONT_H: usize = 8;
const CHAR_SPACING: usize = 1;
const LINE_SPACING: usize = 4;
const DEFAULT_FG_COLOR: u32 = 0x00_FF_FF_FF;
const DEFAULT_BG_COLOR: u32 = 0x00_08_18_30;
const DEFAULT_SHADOW_COLOR: u32 = 0x00_00_00_00;
pub(super) const PINK_FG_COLOR: u32 = 0x00_FF_55_FF;
const LEFT_MARGIN: usize = 16;
const BANNER_X: usize = 16;
const BANNER_Y: usize = 8;
// Reserved header area (banner + indicators + cube). Log output starts below this.
const TOP_MARGIN: usize = 50;
const TEXT_LINE_HEIGHT: usize = FONT_H + LINE_SPACING;
const VGA_TEXT_PHYS: usize = 0xB8000;
pub(super) const VGA_COLUMNS: usize = 80;
const VGA_ROWS: usize = 25;
const VGA_TEXT_CELLS: usize = VGA_COLUMNS * VGA_ROWS;
const LEGACY_DEFAULT_COLOR: u8 = 0x1F;
const LEGACY_STATS_COLOR: u8 = 0x1D;
const LOG_GUARD_MAX_BYTES: usize = 512;

pub(super) struct FramebufferSurface {
    addr: *mut u8,
    pitch: usize,
    bytes_per_pixel: usize,
    pub(super) width: usize,
    pub(super) height: usize,
    cursor_x: usize,
    cursor_y: usize,
    default_fg_color: u32,
    default_bg_color: u32,
    default_shadow_color: u32,
    fg_color: u32,
    bg_color: u32,
    shadow_color: u32,
}

unsafe impl Send for FramebufferSurface {}
unsafe impl Sync for FramebufferSurface {}

pub(super) static FRAMEBUFFER: Mutex<Option<FramebufferSurface>> = Mutex::new(None);
static LEGACY_ROW: AtomicUsize = AtomicUsize::new(0);

pub fn init(framebuffers: Option<&'static ::limine::response::FramebufferResponse>) {
    let mut surface = FRAMEBUFFER.lock();
    *surface = framebuffers
        .and_then(|resp| resp.framebuffers().next())
        .and_then(FramebufferSurface::from_limine);
    if let Some(fb) = surface.as_mut() {
        fb.clear();
    }
    LEGACY_ROW.store(1, Ordering::Relaxed);
}

pub fn log_framebuffers(framebuffers: Option<&'static ::limine::response::FramebufferResponse>) {
    match framebuffers {
        Some(resp) => {
            for (idx, fb) in resp.framebuffers().enumerate() {
                debugconf!(
                    "Framebuffer[{}]: {}x{} pitch={} bpp={}\n",
                    idx,
                    fb.width(),
                    fb.height(),
                    fb.pitch(),
                    fb.bpp()
                );
            }
        }
        None => debugconf!("Framebuffers: unavailable\n"),
    }
}

pub fn current_colors() -> Option<(u32, u32, u32)> {
    FRAMEBUFFER
        .lock()
        .as_ref()
        .map(|fb| (fb.fg_color, fb.bg_color, fb.shadow_color))
}

pub fn write_str(s: &str) {
    if s.is_empty() {
        return;
    }
    let mut guard = FRAMEBUFFER.lock();
    if let Some(fb) = guard.as_mut() {
        fb.write_str(s);
    }
}

/// Best-effort framebuffer write that never blocks.
///
/// Intended for logging paths that may run from interrupt/exception context.
pub fn try_write_str(s: &str) {
    if s.is_empty() {
        return;
    }
    if let Some(mut guard) = FRAMEBUFFER.try_lock() {
        if let Some(fb) = guard.as_mut() {
            fb.write_str(s);
        }
    }
}

pub fn write_log_guarded(s: &str) {
    if s.is_empty() {
        return;
    }
    if !log_payload_passes_guard(s) {
        render_log_placeholder_dot();
        return;
    }
    write_str(s);
}

/// Best-effort log mirror that never blocks.
pub fn try_write_log_guarded(s: &str) {
    if s.is_empty() {
        return;
    }
    if !log_payload_passes_guard(s) {
        // If the framebuffer is busy, skip the placeholder to avoid blocking.
        try_write_str(".");
        return;
    }
    try_write_str(s);
}

fn log_payload_passes_guard(s: &str) -> bool {
    if s.len() > LOG_GUARD_MAX_BYTES {
        return false;
    }
    for ch in s.chars() {
        if ch.is_control() {
            match ch {
                '\n' | '\r' | '\t' | '\x1b' => continue,
                _ => return false,
            }
        }
    }
    true
}

fn render_log_placeholder_dot() {
    let mut guard = FRAMEBUFFER.lock();
    if let Some(fb) = guard.as_mut() {
        fb.write_str(".");
    }
}

pub fn render_framebuffer_banner(text: &str) -> bool {
    let mut guard = FRAMEBUFFER.lock();
    if let Some(fb) = guard.as_mut() {
        fb.blit_text(text, BANNER_X, BANNER_Y);
        return true;
    }
    drop(guard);
    append_legacy_line(text, false);
    false
}

pub fn framebuffer_dimensions() -> Option<(u32, u32)> {
    FRAMEBUFFER
        .lock()
        .as_ref()
        .map(|fb| (fb.width as u32, fb.height as u32))
}

pub fn header_height() -> usize {
    TOP_MARGIN
}

fn append_legacy_line(line: &str, is_stats: bool) {
    let color = if is_stats {
        LEGACY_STATS_COLOR
    } else {
        LEGACY_DEFAULT_COLOR
    };
    let mut row = LEGACY_ROW.fetch_add(1, Ordering::Relaxed);
    let max_rows = VGA_TEXT_CELLS / VGA_COLUMNS;
    if row >= max_rows {
        scroll_legacy_text(color);
        row = max_rows.saturating_sub(1);
        LEGACY_ROW.store(max_rows, Ordering::Relaxed);
    }
    let truncated = if line.len() > VGA_COLUMNS {
        &line[..VGA_COLUMNS]
    } else {
        line
    };
}

fn scroll_legacy_text(color: u8) {
    let buffer_ptr = phys::phys_to_virt(VGA_TEXT_PHYS);
    if buffer_ptr == 0 {
        return;
    }
    unsafe {
        // Keep row 0 intact (banner). Scroll rows 1.. down by one.
        let base = buffer_ptr as *mut u16;
        let dst = base.add(VGA_COLUMNS);
        let src = dst.add(VGA_COLUMNS);
        let cells_to_move = VGA_TEXT_CELLS.saturating_sub(2 * VGA_COLUMNS);
        if cells_to_move > 0 {
            core::ptr::copy(src, dst, cells_to_move);
        }
        let blank = ((color as u16) << 8) | b' ' as u16;
        // Clear the last row.
        let start = base.add(VGA_TEXT_CELLS.saturating_sub(VGA_COLUMNS));
        for offset in 0..VGA_COLUMNS {
            core::ptr::write_volatile(start.add(offset), blank);
        }
    }
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
            cursor_x: LEFT_MARGIN,
            cursor_y: TOP_MARGIN,
            default_fg_color: DEFAULT_FG_COLOR,
            default_bg_color: DEFAULT_BG_COLOR,
            default_shadow_color: DEFAULT_SHADOW_COLOR,
            fg_color: DEFAULT_FG_COLOR,
            bg_color: DEFAULT_BG_COLOR,
            shadow_color: DEFAULT_SHADOW_COLOR,
        })
    }

    fn clear(&mut self) {
        for y in 0..self.height {
            let row_ptr = unsafe { self.addr.add(y.saturating_mul(self.pitch)) as *mut u32 };
            for x in 0..self.width {
                unsafe { row_ptr.add(x).write_volatile(self.bg_color) };
            }
        }
        self.cursor_x = LEFT_MARGIN;
        self.cursor_y = TOP_MARGIN;
    }

    fn write_str(&mut self, text: &str) {
        let mut chars = text.chars();
        while let Some(ch) = chars.next() {
            match ch {
                '\r' => continue,
                '\n' => self.new_line(),
                '\x1b' => self.consume_ansi(&mut chars),
                _ => self.put_char(ch),
            }
        }
    }

    fn consume_ansi<I>(&mut self, iter: &mut I)
    where
        I: Iterator<Item = char>,
    {
        if iter.next() != Some('[') {
            return;
        }
        let mut value: u16 = 0;
        let mut has_value = false;
        while let Some(ch) = iter.next() {
            match ch {
                '0'..='9' => {
                    value = value
                        .saturating_mul(10)
                        .saturating_add((ch as u16) - b'0' as u16);
                    has_value = true;
                }
                ';' => {
                    if has_value {
                        self.apply_ansi(value);
                    }
                    value = 0;
                    has_value = false;
                }
                'm' => {
                    if has_value {
                        self.apply_ansi(value);
                    }
                    break;
                }
                _ => break,
            }
        }
    }

    fn apply_ansi(&mut self, code: u16) {
        match code {
            0 => {
                self.fg_color = self.default_fg_color;
                self.bg_color = self.default_bg_color;
                self.shadow_color = self.default_shadow_color;
            }
            95 => {
                self.fg_color = PINK_FG_COLOR;
            }
            _ => {}
        }
    }

    fn put_char(&mut self, ch: char) {
        if ch.is_control() {
            return;
        }
        self.blit_glyph(ch, self.cursor_x, self.cursor_y);
        self.cursor_x = self.cursor_x.saturating_add(FONT_W + CHAR_SPACING);
        if self.cursor_x >= self.width.saturating_sub(FONT_W + CHAR_SPACING) {
            self.new_line();
        }
    }

    fn new_line(&mut self) {
        self.cursor_x = LEFT_MARGIN;
        self.cursor_y = self.cursor_y.saturating_add(TEXT_LINE_HEIGHT);
        if self.cursor_y >= self.height.saturating_sub(TEXT_LINE_HEIGHT) {
            self.scroll_framebuffer();
        }
    }

    fn blit_text(&mut self, text: &str, origin_x: usize, origin_y: usize) {
        let mut cursor_x = origin_x;
        for ch in text.chars() {
            self.blit_glyph(ch, cursor_x, origin_y);
            cursor_x = cursor_x.saturating_add(FONT_W + CHAR_SPACING);
            if cursor_x >= self.width.saturating_sub(FONT_W + CHAR_SPACING) {
                break;
            }
        }
    }

    fn blit_glyph(&mut self, ch: char, origin_x: usize, origin_y: usize) {
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
                let color = if bit_set {
                    self.fg_color
                } else {
                    self.bg_color
                };
                self.write_pixel(pixel_x, pixel_y, color);
                if bit_set {
                    let shadow_x = pixel_x + 1;
                    let shadow_y = pixel_y + 1;
                    if shadow_x < self.width && shadow_y < self.height {
                        self.write_pixel(shadow_x, shadow_y, self.shadow_color);
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

    fn plot_if_visible(&mut self, x: i32, y: i32, color: u32) {
        if x < 0 || y < 0 {
            return;
        }
        let (xu, yu) = (x as usize, y as usize);
        if xu >= self.width || yu >= self.height {
            return;
        }
        self.write_pixel(xu, yu, color);
    }

    pub(super) fn draw_line(&mut self, mut x0: i32, mut y0: i32, x1: i32, y1: i32, color: u32) {
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
        &mut self,
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

    fn scroll_framebuffer(&mut self) {
        let line_height = TEXT_LINE_HEIGHT.min(self.height);
        if line_height == 0 || self.height <= line_height {
            self.clear();
            return;
        }
        // Preserve the header area (banner) by only scrolling the region below TOP_MARGIN.
        let header_rows = TOP_MARGIN.min(self.height);
        let scroll_rows = self.height.saturating_sub(header_rows);
        if scroll_rows <= line_height {
            // Not enough room to scroll: clear only the scrollable area.
            self.clear_rect(0, header_rows, self.width, scroll_rows, self.bg_color);
            self.cursor_y = header_rows;
            self.cursor_x = LEFT_MARGIN;
            return;
        }

        let bytes_to_move = self
            .pitch
            .saturating_mul(scroll_rows.saturating_sub(line_height));
        unsafe {
            core::ptr::copy(
                self.addr.add(
                    self.pitch
                        .saturating_mul(header_rows.saturating_add(line_height)),
                ),
                self.addr.add(self.pitch.saturating_mul(header_rows)),
                bytes_to_move,
            );
        }

        let start_y = self.height.saturating_sub(line_height);
        for y in start_y..self.height {
            let row_ptr = unsafe { self.addr.add(y.saturating_mul(self.pitch)) as *mut u32 };
            for x in 0..self.width {
                unsafe { row_ptr.add(x).write_volatile(self.bg_color) };
            }
        }
        self.cursor_y = start_y;
        self.cursor_x = LEFT_MARGIN;
    }
}

pub fn draw_line(x0: i32, y0: i32, x1: i32, y1: i32, color: u32) {
    if let Some(fb) = FRAMEBUFFER.lock().as_mut() {
        fb.draw_line(x0, y0, x1, y1, color);
    }
}

pub fn clear_rect(origin_x: usize, origin_y: usize, width: usize, height: usize, color: u32) {
    if let Some(fb) = FRAMEBUFFER.lock().as_mut() {
        fb.clear_rect(origin_x, origin_y, width, height, color);
    }
}
