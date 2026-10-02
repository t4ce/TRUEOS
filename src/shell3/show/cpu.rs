//! Software MicroFont renderer for Shell3's UI4 image frame.

use alloc::{string::String, vec};

use crate::ui4::{acquire_frame_buffer, cancel_frame_buffer, publish_frame_buffer, writable_rgba_view};

use super::{FOREGROUND, Ui4Surface};

pub(super) fn present(
    surface: &Ui4Surface,
    lines: [&str; 3],
) -> Result<(), &'static str> {
    let lease = acquire_frame_buffer(surface.frame).map_err(|_| "shell3-show-cpu-frame-busy")?;
    let view = match writable_rgba_view(lease) {
        Ok(view) => view,
        Err(_) => {
            let _ = cancel_frame_buffer(lease);
            return Err("shell3-show-cpu-frame-view");
        }
    };
    if paint_text(view, lines).is_err() {
        let _ = cancel_frame_buffer(lease);
        return Err("shell3-show-cpu-glyph-paint");
    }
    crate::intel::dma_cache_flush_range(view.virt, view.byte_len);
    if publish_frame_buffer(lease).is_err() {
        let _ = cancel_frame_buffer(lease);
        return Err("shell3-show-cpu-frame-publish");
    }
    Ok(())
}

pub(super) fn paint_text(
    view: crate::ui4::FrameRgbaView,
    lines: [&str; 3],
) -> Result<(), ()> {
    let pixels = unsafe { core::slice::from_raw_parts_mut(view.virt as *mut u8, view.byte_len) };
    let background = super::BACKGROUND.rgba();
    let foreground = FOREGROUND.rgba();
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.copy_from_slice(&background);
    }
    let width = view.width as usize;
    let height = view.height as usize;
    let mut glyphs = vec![0u8; width.checked_mul(height).ok_or(())?];
    for (row, line) in lines.iter().enumerate() {
        let text = ascii_text(line);
        let y = i32::try_from(row.saturating_mul(microfont::FHEIGHT)).map_err(|_| ())?;
        microfont::stamp_text(&mut glyphs, width, height, 0, y, &text, 1u8).map_err(|_| ())?;
    }
    for (index, alpha) in glyphs.iter().copied().enumerate() {
        if alpha == 0 {
            continue;
        }
        let x = index % width;
        let y = index / width;
        let offset = y * view.pitch as usize + x * 4;
        pixels
            .get_mut(offset..offset + 4)
            .ok_or(())?
            .copy_from_slice(&foreground);
    }
    Ok(())
}

fn ascii_text(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_ascii_graphic() || character == ' ' {
                character
            } else {
                '-'
            }
        })
        .collect()
}
