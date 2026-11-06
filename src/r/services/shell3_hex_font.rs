//! Edit's shared full-cell bitmap font. Both the copy renderer and Conway use
//! the same canonical masks, without enabling MicroFont's runtime hex feature.
//! Generate the Rust/OpenCL pair with `python3 tools/edit_hex_font.py`.

include!("shell3_hex_font.generated.rs");

#[path = "../../../crates/trueos-terminal/src/tab_label_cell.rs"]
pub(crate) mod tab_label_cell;

/// Unsupported Unicode uses the same visible red replacement in both paths.
pub(crate) fn resolve(character: char, foreground: [u8; 4]) -> (u32, [u8; 4]) {
    if let Some(label) = tab_label_cell::TabLabelCell::from_character(character) {
        return (label.glyph(), foreground);
    }
    match CODEPOINTS.binary_search(&(character as u32)) {
        Ok(index) => (index as u32, foreground),
        Err(_) => (FALLBACK_GLYPH, [255, 0, 0, foreground[3]]),
    }
}

pub(crate) fn native_coverage(glyph_index: u32, x: u32, y: u32) -> u8 {
    if x >= 6 || y >= 11 { return 0; }
    if let Some(label) = tab_label_cell::TabLabelCell::from_glyph(glyph_index) {
        let Some((character, px, py, width, height)) = label.point(x, y, 6, 11) else { return 0; };
        return native_coverage(resolve(character, [255; 4]).0, px * 6 / width, py * 11 / height);
    }
    let words = MASKS.get(glyph_index as usize).unwrap_or(&MASKS[FALLBACK_GLYPH as usize]);
    let bit = 65 - y * 6 - x;
    (((words[(bit >> 5) as usize] >> (bit & 31)) & 1) * 255) as u8
}

/// Integer nearest-neighbour mapping, shared with the compute input sampler.
/// Cell extents remain those of the physical raster-font layout.
pub(crate) fn sample(glyph_index: u32, x: u32, y: u32, width: u32, height: u32, underline: bool) -> u8 {
    if width == 0 || height == 0 || x >= width || y >= height { return 0; }
    if let Some(label) = tab_label_cell::TabLabelCell::from_glyph(glyph_index) {
        let Some((character, px, py, w, h)) = label.point(x, y, width, height) else { return 0; };
        return sample(resolve(character, [255; 4]).0, px, py, w, h, false);
    }
    let source_x = (u64::from(x) * 6 / u64::from(width)) as u32;
    let source_y = (u64::from(y) * 11 / u64::from(height)) as u32;
    if underline && source_y == 10 { 255 }
    else { native_coverage(glyph_index, source_x, source_y) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_labels_keep_full_bitmap_detail_at_half_physical_size() {
        for count in 1..=4 {
            let columns = count * 4;
            let text = ["tab", "2tab", "3tab", "4tab"][(count-1) as usize];
            let left = (columns * 12 - text.len() as u32 * 6) / 2;
            for x in 0..columns * 12 {
                let cell = tab_label_cell::TabLabelCell::new(count, columns, x / 12).unwrap();
                assert_eq!(resolve(cell.character(), [17;4]), (cell.glyph(), [17;4]));
                for y in 0..22 {
                    let expected = if x >= left && x < left + text.len() as u32 * 6 && (5..16).contains(&y) {
                        let ch = text.as_bytes()[((x-left)/6) as usize] as char;
                        native_coverage(resolve(ch, [255;4]).0, (x-left)%6, y-5)
                    } else { 0 };
                    assert_eq!(sample(cell.glyph(), x%12, y, 12, 22, false), expected);
                }
            }
        }
    }

    #[test]
    fn scaled_coverage_matches_every_canonical_bit_at_integer_and_physical_sizes() {
        for (index, words) in MASKS.iter().enumerate() {
            let bits = u128::from(words[0]) | (u128::from(words[1]) << 32)
                | (u128::from(words[2]) << 64) | (u128::from(words[3]) << 96);
            assert_eq!(bits >> 66, 0);
            for (width, height) in [(6, 11), (12, 22), (17, 31), (23, 42)] {
                for y in 0..height {
                    for x in 0..width {
                        let bit = 65 - (y * 11 / height) * 6 - x * 6 / width;
                        let expected = if bits & (1u128 << bit) == 0 { 0 } else { 255 };
                        assert_eq!(sample(index as u32, x, y, width, height, false), expected);
                    }
                }
            }
        }
    }

    #[test]
    fn full_blocks_keep_the_two_pixels_missing_from_the_legacy_u64_api() {
        let (block, _) = resolve('█', [255; 4]);
        assert_eq!(native_coverage(block, 4, 10), 255);
        assert_eq!(native_coverage(block, 5, 10), 255);
        assert_eq!(native_coverage(block, 0, 0), 255);
    }

    #[test]
    fn blank_underlining_and_fallback_share_the_input_mapping() {
        assert_eq!(resolve(' ', [17; 4]).0, SPACE_GLYPH);
        for y in 0..31 {
            for x in 0..17 {
                assert_eq!(sample(SPACE_GLYPH, x, y, 17, 31, false), 0);
                assert_eq!(sample(SPACE_GLYPH, x, y, 17, 31, true), if y * 11 / 31 == 10 { 255 } else { 0 });
            }
        }
        assert_eq!(resolve('漢', [1, 2, 3, 129]), (FALLBACK_GLYPH, [255, 0, 0, 129]));
        assert_eq!(sample(SPACE_GLYPH, 0, 0, 0, 31, true), 0);
        assert_eq!(sample(SPACE_GLYPH, 17, 31, 17, 31, true), 0);
        for character in ['ẽ', 'ỹ', 'ŷ', 'ẙ', 'ń', 'ĩ', 'ů', 'ỳ', 'ũ', 'ǹ'] {
            assert_ne!(resolve(character, [255; 4]).0, FALLBACK_GLYPH);
        }
    }
}
