//! CPU copy sink for Shell3's shared antialiased MicroFont glyph cells.

use alloc::vec::Vec;

use crate::ui4::{
    FrameRgbaView, acquire_frame_buffer, cancel_frame_buffer, publish_frame_buffer,
    writable_rgba_view,
};

use super::{Ui4Surface, rendered_lines};

pub(super) fn present(
    surface: &mut Ui4Surface,
    lines: &[&[(char, Option<super::super::RgbaColor>)]],
    fallback_segments: &[super::super::SegmentUpdate],
) -> Result<Option<crate::ui4::DamageRect>, &'static str> {
    let lease = acquire_frame_buffer(surface.frame).map_err(|_| "shell3-show-cpu-frame-busy")?;
    let index = lease.buffer_index as usize;
    let previous = surface.frame_contents[index].clone();
    let current = rendered_lines(lines);
    let mut segments = super::super::update::diff_rendered_lines(previous.as_deref(), &current);
    if segments.is_empty() && previous.is_some() {
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
    // A failed sparse paint must retry against a freshly cleared background.
    surface.frame_contents[index] = None;
    surface.clear_buffers[index] = true;
    if clearing {
        let pixels =
            unsafe { core::slice::from_raw_parts_mut(view.virt as *mut u8, view.byte_len) };
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.copy_from_slice(&super::BACKGROUND);
        }
    }
    if paint_segments(view, &segments, surface.scale).is_err() {
        let _ = cancel_frame_buffer(lease);
        return Err("shell3-show-cpu-glyph-paint");
    }
    if clearing {
        crate::intel::dma_cache_flush_range(view.virt, view.byte_len);
    }
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

fn paint_segment(
    view: FrameRgbaView,
    segment: &super::super::SegmentUpdate,
    scale: u32,
) -> Result<(), ()> {
    let row = match segment.row {
        super::super::SpecialRows::TitleRow => 0usize,
        super::super::SpecialRows::StatusRow => 1,
        super::super::SpecialRows::PromtRow => 2,
        super::super::SpecialRows::MatrixRow(index) => index + 3,
    };
    let (cell_width, cell_height) = crate::r::services::shell3_font_service::cell_extent(scale).ok_or(())?;
    let y = u32::try_from(row).ok().and_then(|row| row.checked_mul(cell_height)).ok_or(())?;
    if y >= view.height {
        return Ok(());
    }
    let mut characters = segment.text.chars();
    let count = segment.remove.max(segment.text.chars().count());
    let mut copies = Vec::new();
    for column in 0..count {
        let x = segment.offset.checked_add(column)
            .and_then(|x| u32::try_from(x).ok())
            .and_then(|x| x.checked_mul(cell_width)).ok_or(())?;
        if x >= view.width {
            break;
        }
        let character = characters.next().unwrap_or(' ');
        copies.push(super::copy::glyph_for_cell(
            x, y, cell_width.min(view.width - x), cell_height.min(view.height - y),
            (character, segment.colors.get(column).copied().flatten()), scale,
        ).map_err(|_| ())?);
    }
    if copies.is_empty() {
        return Ok(());
    }
    // This path is the complete synchronous CPU fallback, independent of the
    // small-request threshold used by the normal BCS0 renderer.
    if unsafe { crate::ui4::text_blit::copy(super::copy::bcs_surface(view), &copies) } {
        Ok(())
    } else {
        Err(())
    }
}
