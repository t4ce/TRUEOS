//! Software MicroFont renderer for Shell3's UI4 image frame.

use alloc::{string::String, vec};

use crate::ui4::{
    FrameRgbaView, acquire_frame_buffer, cancel_frame_buffer,
    publish_frame_buffer, writable_rgba_view,
};

use super::{Ui4Surface, rendered_lines};

pub(super) fn present(
    surface: &mut Ui4Surface,
    lines: [&str; 3],
    fallback_segments: &[super::super::SegmentUpdate],
) -> Result<Option<crate::ui4::DamageRect>, &'static str> {
    let lease = acquire_frame_buffer(surface.frame).map_err(|_| "shell3-show-cpu-frame-busy")?;
    let index = lease.buffer_index as usize;
    let previous = surface.frame_contents[index].clone();
    let current = rendered_lines(lines);
    let mut segments = super::super::update::diff_rendered_lines(previous.as_ref(), &current);
    if segments.is_empty() {
        segments.extend_from_slice(fallback_segments);
    }
    let view = match writable_rgba_view(lease) {
        Ok(view) => view,
        Err(_) => {
            let _ = cancel_frame_buffer(lease);
            return Err("shell3-show-cpu-frame-view");
        }
    };
    if paint_segments(view, &segments).is_err() {
        let _ = cancel_frame_buffer(lease);
        return Err("shell3-show-cpu-glyph-paint");
    }
    if publish_frame_buffer(lease).is_err() {
        let _ = cancel_frame_buffer(lease);
        return Err("shell3-show-cpu-frame-publish");
    }
    surface.frame_contents[index] = Some(current);
    let damage = if previous.is_none() && segments.is_empty() {
        Some(crate::ui4::DamageRect::FULL)
    } else {
        super::damage_for_segments(&segments, surface.width, surface.height)
    };
    Ok(damage)
}

pub(super) fn paint_segments(
    view: FrameRgbaView,
    segments: &[super::super::SegmentUpdate],
) -> Result<(), ()> {
    for segment in segments {
        paint_segment(view, segment)?;
    }
    Ok(())
}

fn paint_segment(view: FrameRgbaView, segment: &super::super::SegmentUpdate) -> Result<(), ()> {
    let row = match segment.row {
        super::super::SpecialRows::TitleRow => 0usize,
        super::super::SpecialRows::StatusRow => 1,
        super::super::SpecialRows::PromtRow => 2,
    };
    let glyph_width = microfont::FWIDTH;
    let glyph_height = microfont::FHEIGHT;
    let x = segment.offset.checked_mul(glyph_width).ok_or(())?;
    let y = row.checked_mul(glyph_height).ok_or(())?;
    let columns = segment.remove.max(segment.text.chars().count());
    let requested_width = columns.checked_mul(glyph_width).ok_or(())?;
    if requested_width == 0 || x >= view.width as usize || y >= view.height as usize {
        return Ok(());
    }
    let width = requested_width.min(view.width as usize - x);
    let height = glyph_height.min(view.height as usize - y);
    let background = super::BACKGROUND.rgba();
    let foreground = super::FOREGROUND.rgba();
    let pixels = unsafe { core::slice::from_raw_parts_mut(view.virt as *mut u8, view.byte_len) };
    for py in y..y + height {
        let row_offset = py.checked_mul(view.pitch as usize).ok_or(())?;
        for px in x..x + width {
            let offset = row_offset.checked_add(px.checked_mul(4).ok_or(())?).ok_or(())?;
            pixels.get_mut(offset..offset + 4).ok_or(())?.copy_from_slice(&background);
        }
    }

    let text = ascii_text(&segment.text);
    let mask_width = width;
    let mask_height = height;
    let mut glyphs = vec![0u8; mask_width.checked_mul(mask_height).ok_or(())?];
    microfont::stamp_text(&mut glyphs, mask_width, mask_height, 0, 0, &text, 1u8).map_err(|_| ())?;
    for (index, alpha) in glyphs.iter().copied().enumerate() {
        if alpha == 0 {
            continue;
        }
        let px = x + index % mask_width;
        let py = y + index / mask_width;
        let offset = py
            .checked_mul(view.pitch as usize)
            .and_then(|offset| offset.checked_add(px.checked_mul(4)?))
            .ok_or(())?;
        pixels.get_mut(offset..offset + 4).ok_or(())?.copy_from_slice(&foreground);
    }
    crate::intel::dma_cache_flush_range(
        view.virt.wrapping_add(y * view.pitch as usize),
        height.saturating_mul(view.pitch as usize),
    );
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
