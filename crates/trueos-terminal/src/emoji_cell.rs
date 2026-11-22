//! Encode both halves of an emoji in a cell-addressed screen.

// Keep these cells separate from the tab labels in private-use plane 15.
const BASE: u32 = 0x100000;
const BMP_START: u32 = 0xa0;
const BMP_END: u32 = 0x3fff;
const EMOJI_START: u32 = 0x1f000;
const EMOJI_END: u32 = 0x1ffff;
const PRIVATE_START: u32 = 0xe000;
const PRIVATE_END: u32 = 0xf8ff;
const EMOJI_OFFSET: u32 = BMP_END - BMP_START + 1;
const PRIVATE_OFFSET: u32 = EMOJI_OFFSET + EMOJI_END - EMOJI_START + 1;
const HALF_STRIDE: u32 = PRIVATE_OFFSET + PRIVATE_END - PRIVATE_START + 1;

/// Return the left and right screen cells for a glyph in the emoji ranges.
pub fn cells(character: char) -> Option<[char; 2]> {
    let index = match character as u32 {
        scalar @ BMP_START..=BMP_END => scalar - BMP_START,
        scalar @ EMOJI_START..=EMOJI_END => EMOJI_OFFSET + scalar - EMOJI_START,
        scalar @ PRIVATE_START..=PRIVATE_END => PRIVATE_OFFSET + scalar - PRIVATE_START,
        _ => return None,
    };
    Some([
        char::from_u32(BASE + index)?,
        char::from_u32(BASE + HALF_STRIDE + index)?,
    ])
}

/// Recover the source glyph and the half to draw, 0 for left and 1 for right.
pub fn decode(cell: char) -> Option<(char, u32)> {
    let encoded = (cell as u32).checked_sub(BASE)?;
    if encoded >= 2 * HALF_STRIDE {
        return None;
    }
    let index = encoded % HALF_STRIDE;
    let scalar = match index {
        0..EMOJI_OFFSET => BMP_START + index,
        EMOJI_OFFSET..PRIVATE_OFFSET => EMOJI_START + index - EMOJI_OFFSET,
        _ => PRIVATE_START + index - PRIVATE_OFFSET,
    };
    Some((char::from_u32(scalar)?, encoded / HALF_STRIDE))
}

/// Recover the glyph and its reserved column in a plain-text cell stream.
pub fn plain_character(cell: char) -> char {
    match decode(cell) {
        Some((glyph, 0)) => glyph,
        Some(_) => ' ',
        None => cell,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_halves_keep_the_glyph_identity() {
        for glyph in [
            '\u{a0}',
            '☺',
            '\u{3fff}',
            '\u{1f000}',
            '🥁',
            '\u{1ffff}',
            '\u{e50a}',
            '\u{f8ff}',
        ] {
            let [left, right] = cells(glyph).unwrap();
            assert_eq!(decode(left), Some((glyph, 0)));
            assert_eq!(decode(right), Some((glyph, 1)));
            assert_eq!(plain_character(left), glyph);
            assert_eq!(plain_character(right), ' ');
        }
        assert_eq!(cells('\u{9f}'), None);
        assert_eq!(cells('\u{4000}'), None);
        assert_eq!(cells('\u{20000}'), None);
        assert_eq!(decode('\u{f0000}'), None);
        assert_eq!(
            decode(char::from_u32(BASE + 2 * HALF_STRIDE).unwrap()),
            None
        );
    }
}
