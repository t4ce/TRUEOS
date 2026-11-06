//! One CPU production of the private home-button line asset.
//!
//! This is independent of the GPU atlas and never accesses its allocations.
//! The caller publishes the immutable result for the service lifetime. Text
//! uses the canonical bitmap atlas; this one private asset has no bitmap alias.

use super::microfont_mask_service::{AtlasConfig, OwnedGlyphMask};

pub(crate) fn build(em_height_px: u32) -> Result<OwnedGlyphMask, &'static str> {
    if !(11..=4000).contains(&em_height_px) {
        return Err("microfont-single-glyph-size");
    }
    let strips = microfont::raster_line::home_button_strips()
        .ok_or("microfont-homebutton-missing")?;
    // Match build_home_button_mask, including its six-pixel stroke and gutter.
    let config = AtlasConfig {
        glyph_height_px: Some(em_height_px),
        line_width_px: 6.0,
        ..AtlasConfig::default()
    };
    let (width, height) = config.cell_extent();
    let bytes = (width as usize).checked_mul(height as usize)
        .ok_or("microfont-single-glyph-size")?;
    let mut pixels = alloc::vec::Vec::new();
    pixels.try_reserve_exact(bytes).map_err(|_| "microfont-homebutton-allocation")?;
    pixels.resize(bytes, 0);
    let mut mask = OwnedGlyphMask { width, height, em_height_px, pixels };
    let gutter = (height - em_height_px) as f32 * 0.5;
    let scale = em_height_px as f32 / 11.0;
    for strip in strips {
        for segment in 0..strip.segments() {
            let a = strip.point(segment).ok_or("microfont-homebutton-geometry")?;
            let b = strip.point(segment + 1).ok_or("microfont-homebutton-geometry")?;
            segment_coverage(&mut mask,
                [(a.x() as f32 + 0.5) * scale + gutter,
                 (a.y() as f32 + 0.5) * scale + gutter],
                [(b.x() as f32 + 0.5) * scale + gutter,
                 (b.y() as f32 + 0.5) * scale + gutter],
                config.line_width_px * 0.5);
        }
    }
    if !mask.pixels.iter().any(|&coverage| coverage != 0) {
        return Err("microfont-single-glyph-empty");
    }
    Ok(mask)
}

// As in Hull's CPU segment rasterizer, merge round-ended segments with max
// coverage. Work is restricted to each segment's clipped stroke bounds; no
// GPU submission, font bitmap substitution, or render-state check is involved.
fn segment_coverage(mask: &mut OwnedGlyphMask, a: [f32; 2], b: [f32; 2], radius: f32) {
    let left = libm::floorf(a[0].min(b[0]) - radius - 0.5).max(0.0) as u32;
    let top = libm::floorf(a[1].min(b[1]) - radius - 0.5).max(0.0) as u32;
    let right = libm::ceilf(a[0].max(b[0]) + radius + 0.5).min(mask.width as f32) as u32;
    let bottom = libm::ceilf(a[1].max(b[1]) + radius + 0.5).min(mask.height as f32) as u32;
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let length_squared = dx * dx + dy * dy;
    for y in top..bottom {
        for x in left..right {
            let px = x as f32 + 0.5 - a[0];
            let py = y as f32 + 0.5 - a[1];
            let t = if length_squared > 0.0 {
                ((px * dx + py * dy) / length_squared).clamp(0.0, 1.0)
            } else { 0.0 };
            let distance = libm::sqrtf(
                (px - t * dx) * (px - t * dx) + (py - t * dy) * (py - t * dy));
            let coverage = ((radius + 0.5 - distance).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            let at = y as usize * mask.width as usize + x as usize;
            mask.pixels[at] = mask.pixels[at].max(coverage);
        }
    }
}
