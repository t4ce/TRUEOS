//! Software MicroFont renderer for Shell3's UI4 image frame.

use alloc::vec;

use crate::ui4::{
    FrameRgbaView, acquire_frame_buffer, cancel_frame_buffer,
    publish_frame_buffer, writable_rgba_view,
};

use super::{Ui4Surface, rendered_lines};

pub(super) fn present(
    surface: &mut Ui4Surface,
    lines: [&[(char, Option<super::super::RgbaColor>)]; 3],
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
    let clearing = surface.clear_buffers[index];
    if clearing {
        let pixels = unsafe { core::slice::from_raw_parts_mut(view.virt as *mut u8, view.byte_len) };
        for pixel in pixels.chunks_exact_mut(4) { pixel.copy_from_slice(&super::BACKGROUND.rgba()); }
    }
    if paint_segments(view, &segments, surface.scale).is_err() {
        let _ = cancel_frame_buffer(lease);
        return Err("shell3-show-cpu-glyph-paint");
    }
    if clearing { crate::intel::dma_cache_flush_range(view.virt, view.byte_len); }
    if publish_frame_buffer(lease).is_err() {
        let _ = cancel_frame_buffer(lease);
        return Err("shell3-show-cpu-frame-publish");
    }
    surface.frame_contents[index] = Some(current);
    surface.clear_buffers[index] = false;
    let damage = if clearing || (previous.is_none() && segments.is_empty()) {
        Some(crate::ui4::DamageRect::FULL)
    } else {
        super::damage_for_segments(&segments, surface.width, surface.height, surface.scale)
    };
    Ok(damage)
}

pub(super) fn paint_segments(
    view: FrameRgbaView,
    segments: &[super::super::SegmentUpdate],
    scale: u32,
) -> Result<(), ()> {
    for segment in segments {
        paint_segment(view, segment, scale)?;
    }
    Ok(())
}

fn paint_segment(view: FrameRgbaView, segment: &super::super::SegmentUpdate, scale: u32) -> Result<(), ()> {
    let row = match segment.row {
        super::super::SpecialRows::TitleRow => 0usize,
        super::super::SpecialRows::StatusRow => 1,
        super::super::SpecialRows::PromtRow => 2,
    };
    let scale = scale as usize;
    let glyph_width = microfont::FWIDTH * scale;
    let glyph_height = microfont::FHEIGHT * scale;
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
    let pixels = unsafe { core::slice::from_raw_parts_mut(view.virt as *mut u8, view.byte_len) };
    for py in y..y + height {
        let row_offset = py.checked_mul(view.pitch as usize).ok_or(())?;
        for px in x..x + width {
            let offset = row_offset.checked_add(px.checked_mul(4).ok_or(())?).ok_or(())?;
            pixels.get_mut(offset..offset + 4).ok_or(())?.copy_from_slice(&background);
        }
    }

    let mask_width = width.div_ceil(scale);
    let mask_height = height.div_ceil(scale);
    let mut glyphs = vec![0u8; mask_width.checked_mul(mask_height).ok_or(())?];
    microfont::stamp_text(&mut glyphs, mask_width, mask_height, 0, 0, &segment.text, 1u8).map_err(|_| ())?;
    for py in 0..height {
        for px in 0..width {
            if glyphs[(py / scale) * mask_width + px / scale] == 0 { continue; }
            let offset = (y + py) * view.pitch as usize + (x + px) * 4;
            let foreground = segment.colors.get(px / glyph_width).copied().flatten()
                .unwrap_or(super::FOREGROUND).rgba();
            pixels.get_mut(offset..offset + 4).ok_or(())?.copy_from_slice(&foreground);
        }
    }
    crate::intel::dma_cache_flush_range(
        view.virt.wrapping_add(y * view.pitch as usize),
        height.saturating_mul(view.pitch as usize),
    );
    Ok(())
}
