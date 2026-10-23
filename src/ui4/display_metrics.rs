//! Physical sizing policy shared by UI producers on native and virtual outputs.
//!
//! Native displays retain the Intel driver's validated EDID conversion. QEMU's
//! virtual output has no host physical monitor size; its desktop density is an
//! explicit 96 DPI, independent of window dimensions and host scaling. This
//! changes glyph production size only, never the subsequent mask placement.

const VIRTUAL_DESKTOP_DPI: u64 = 96;

pub(crate) fn physical_extent_pixels(width_mm: u32, height_mm: u32) -> Option<(u32, u32)> {
    if width_mm == 0 || height_mm == 0 {
        return None;
    }
    if let Some(extent) = crate::intel::physical_extent_pixels(width_mm, height_mm) {
        return Some(extent);
    }
    // Publish geometry only after the virtual compositor is ready, just as the
    // native API waits for an active scanout. An absent display stays absent.
    crate::virtio_gpu_logo::output_dimensions()?;
    Some((virtual_axis_pixels(width_mm)?, virtual_axis_pixels(height_mm)?))
}

fn virtual_axis_pixels(millimeters: u32) -> Option<u32> {
    // 25.4 mm per inch, with nearest-pixel rounding and checked conversion.
    // Keep the fractional millimeter in integer arithmetic so axes agree.
    let numerator = u64::from(millimeters).checked_mul(VIRTUAL_DESKTOP_DPI * 10)?;
    let pixels = u32::try_from(numerator.checked_add(127)? / 254).ok()?;
    (pixels != 0).then_some(pixels)
}

#[cfg(test)]
mod tests {
    use super::virtual_axis_pixels;

    #[test]
    fn virtual_density_is_96_dpi_with_nearest_pixel_rounding() {
        assert_eq!(virtual_axis_pixels(5), Some(19));
        assert_eq!(virtual_axis_pixels(50), Some(189));
        assert_eq!(virtual_axis_pixels(127), Some(480));
        assert_eq!(virtual_axis_pixels(254), Some(960));
    }

    #[test]
    fn empty_or_unrepresentable_extents_fail_closed() {
        assert_eq!(virtual_axis_pixels(0), None);
        assert_eq!(virtual_axis_pixels(u32::MAX), None);
    }
}
