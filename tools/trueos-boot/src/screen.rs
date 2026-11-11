//! A quiet boot screen; detailed diagnostics stay in the boot log and COM1.

use crate::handoff::Framebuffer;
use font8x8::{UnicodeFonts, BASIC_FONTS, LATIN_FONTS};

pub const TITLE: &str = "Boot-Loading TrueOS §";

fn title_scale(fb: Framebuffer) -> usize {
    (fb.height as usize / 200).clamp(2, 8)
        .min((fb.width as usize / (TITLE.chars().count() * 8)).max(1))
}

fn title_y(fb: Framebuffer, scale: usize) -> usize {
    (fb.height as usize / 2).saturating_sub(10 * scale)
}

unsafe fn clear_rows(fb: Framebuffer, y: usize, height: usize) {
    for row in y..y.saturating_add(height).min(fb.height as usize) {
        for x in 0..fb.width as usize {
            let address = fb.phys as usize + row * fb.pitch as usize + x * 4;
            unsafe { (address as *mut u32).write_volatile(0); }
        }
    }
}

unsafe fn centered_text(fb: Framebuffer, text: &str, y: usize, scale: usize, shade: u32) {
    let width = text.chars().count() * 8 * scale;
    let left = (fb.width as usize).saturating_sub(width) / 2;
    let color = (shade << fb.red_mask_shift) | (shade << fb.green_mask_shift)
        | (shade << fb.blue_mask_shift);
    for (column, ch) in text.chars().enumerate() {
        let Some(glyph) = BASIC_FONTS.get(ch).or_else(|| LATIN_FONTS.get(ch)) else { continue; };
        for (row, bits) in glyph.iter().enumerate() {
            for bit in 0..8 {
                if bits & (1 << bit) == 0 { continue; }
                for dy in 0..scale {
                    for dx in 0..scale {
                        let px = left + (column * 8 + bit) * scale + dx;
                        let py = y + row * scale + dy;
                        if px < fb.width as usize && py < fb.height as usize {
                            let address = fb.phys as usize + py * fb.pitch as usize + px * 4;
                            unsafe { (address as *mut u32).write_volatile(color); }
                        }
                    }
                }
            }
        }
    }
}

/// The framebuffer must be mapped and writable, including after firmware exit.
pub unsafe fn initialize(fb: Framebuffer) {
    let scale = title_scale(fb);
    unsafe {
        clear_rows(fb, 0, fb.height as usize);
        centered_text(fb, TITLE, title_y(fb, scale), scale, 0xe8);
    }
}

/// Replace only the current stage beneath the title.
pub unsafe fn stage(fb: Framebuffer, text: &str) {
    let scale = title_scale(fb);
    let y = title_y(fb, scale) + 14 * scale;
    let small = scale.div_ceil(2)
        .min((fb.width as usize / (text.chars().count().max(1) * 8)).max(1));
    unsafe {
        clear_rows(fb, y, 8 * scale.div_ceil(2));
        centered_text(fb, text, y, small, 0xaa);
    }
}
