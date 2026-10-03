//! Metrics from the same registered TTF used by the kernel glyph producer.
use skrifa::{
    FontRef, MetadataProvider,
    instance::{LocationRef, Size},
};

#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct FontMetricsV1 {
    pub version: u32,
    pub font_id: u32,
    pub flags: u32,
    pub pixels: f32,
    pub cell_advance: f32,
    pub line_height: f32,
    pub ascent: f32,
    pub descent: f32,
    pub underline_position: f32,
    pub underline_thickness: f32,
    pub strikeout_position: f32,
    pub strikeout_thickness: f32,
}
const _: () = assert!(core::mem::size_of::<FontMetricsV1>() == 48);

pub fn from_bytes(bytes: &[u8], font_id: u32, pixels: f32) -> Result<FontMetricsV1, &'static str> {
    if !pixels.is_finite() || !(4.0..=256.0).contains(&pixels) {
        return Err("font-size-invalid");
    }
    let font = FontRef::new(bytes).map_err(|_| "font-parse-failed")?;
    let size = Size::new(pixels);
    let metrics = font.metrics(size, LocationRef::default());
    let glyph = font
        .charmap()
        .map('m')
        .ok_or("font-missing-cell-reference")?;
    let advance = font
        .glyph_metrics(size, LocationRef::default())
        .advance_width(glyph)
        .ok_or("font-missing-advance")?;
    let height = metrics.ascent - metrics.descent + metrics.leading.max(0.0);
    if !advance.is_finite() || advance <= 0.0 || !height.is_finite() || height <= 0.0 {
        return Err("font-metrics-invalid");
    }
    Ok(FontMetricsV1 {
        version: 1,
        font_id,
        flags: u32::from(metrics.is_monospace),
        pixels,
        cell_advance: advance,
        line_height: height,
        ascent: metrics.ascent,
        descent: metrics.descent,
        underline_position: metrics
            .underline
            .map_or(metrics.descent / 2.0, |v| v.offset),
        underline_thickness: metrics
            .underline
            .map_or(pixels / 16.0, |v| v.thickness)
            .max(1.0),
        strikeout_position: metrics.strikeout.map_or(metrics.ascent / 3.0, |v| v.offset),
        strikeout_thickness: metrics
            .strikeout
            .map_or(pixels / 16.0, |v| v.thickness)
            .max(1.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    const FONT: &[u8] = include_bytes!("../../tools/fnt/Inconsolata-Regular.ttf");
    #[test]
    fn shipped_monospace_metrics_scale_and_use_real_font_advances() {
        let small = from_bytes(FONT, 3, 15.0).unwrap();
        let large = from_bytes(FONT, 3, 30.0).unwrap();
        assert_eq!(small.version, 1);
        assert_eq!(small.font_id, 3);
        assert!(small.cell_advance > 0.0 && small.line_height > small.cell_advance);
        assert!(small.ascent > 0.0 && small.descent <= 0.0);
        assert!((large.cell_advance - small.cell_advance * 2.0).abs() < 0.001);
        assert!((large.line_height - small.line_height * 2.0).abs() < 0.001);
        let font = FontRef::new(FONT).unwrap();
        let metrics = font.glyph_metrics(Size::new(15.0), LocationRef::default());
        for scalar in [' ', 'i', 'W', '0'] {
            let advance = metrics
                .advance_width(font.charmap().map(scalar).unwrap())
                .unwrap();
            assert!((advance - small.cell_advance).abs() < 0.001);
        }
    }
    #[test]
    fn rejects_invalid_sizes_and_invalid_fonts() {
        for size in [0.0, 3.0, 257.0, f32::NAN, f32::INFINITY] {
            assert!(from_bytes(FONT, 3, size).is_err());
        }
        assert!(from_bytes(&[], 3, 15.0).is_err());
    }
}
