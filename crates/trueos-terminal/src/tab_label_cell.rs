//! Internal paint cells for Edit's selection-only, half-size tab labels.
//! These identities are synthesized by the renderer, never document text.

const BASE: u32 = 0xf0000;
const TAG: u32 = 0x8000_0000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TabLabelCell {
    pub count: u32,
    pub columns: u32,
    pub column: u32,
}

impl TabLabelCell {
    pub fn new(count: u32, columns: u32, column: u32) -> Option<Self> {
        (count >= 1 && count <= 4 && columns >= count && columns <= count * 8
            && column < columns).then_some(Self { count, columns, column })
    }

    fn payload(self) -> u32 {
        ((self.count - 1) << 10) | ((self.columns - 1) << 5) | self.column
    }

    pub fn character(self) -> char { char::from_u32(BASE + self.payload()).unwrap() }
    pub fn glyph(self) -> u32 { TAG | self.payload() }

    pub fn from_character(character: char) -> Option<Self> {
        Self::decode((character as u32).checked_sub(BASE)?)
    }

    pub fn from_glyph(glyph: u32) -> Option<Self> {
        Self::decode(glyph.checked_sub(TAG)?)
    }

    fn decode(payload: u32) -> Option<Self> {
        if payload >= 4096 { return None; }
        Self::new((payload >> 10) + 1, ((payload >> 5) & 31) + 1, payload & 31)
    }

    /// Map a full-size cell pixel to a glyph in the centered half-size label.
    /// A tab stop narrower than the label clips it within its whitespace span.
    pub fn point(self, x: u32, y: u32, width: u32, height: u32)
        -> Option<(char, u32, u32, u32, u32)> {
        if width == 0 || height == 0 || x >= width || y >= height { return None; }
        let w = (width / 2).max(1);
        let h = (height / 2).max(1);
        let length = if self.count == 1 { 3 } else { 4 };
        let left = (self.columns as i64 * width as i64 - length * w as i64).div_euclid(2);
        let px = self.column as i64 * width as i64 + x as i64 - left;
        let py = y as i64 - ((height - h) / 2) as i64;
        if px < 0 || px >= length * w as i64 || py < 0 || py >= h as i64 { return None; }
        let index = px as u32 / w;
        let character = if self.count == 1 {
            [ 't', 'a', 'b' ][index as usize]
        } else {
            [char::from_u32(b'0' as u32 + self.count).unwrap(), 't', 'a', 'b'][index as usize]
        };
        Some((character, px as u32 % w, py as u32, w, h))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_centered_and_half_size_across_cell_boundaries() {
        for count in 1..=4 {
            let columns = count * 4;
            let text = ["tab", "2tab", "3tab", "4tab"][(count - 1) as usize];
            let left = (columns * 12 - text.len() as u32 * 6) / 2;
            for x in 0..columns * 12 {
                let cell = TabLabelCell::new(count, columns, x / 12).unwrap();
                assert_eq!(TabLabelCell::from_character(cell.character()), Some(cell));
                assert_eq!(TabLabelCell::from_glyph(cell.glyph()), Some(cell));
                let sample = cell.point(x % 12, 5, 12, 22);
                if x >= left && x < left + text.len() as u32 * 6 {
                    let index = (x - left) / 6;
                    assert_eq!(sample, Some((text.as_bytes()[index as usize] as char, (x-left)%6, 0, 6, 11)));
                } else { assert_eq!(sample, None); }
                assert_eq!(cell.point(x % 12, 4, 12, 22), None);
                assert_eq!(cell.point(x % 12, 16, 12, 22), None);
            }
        }
        assert!(TabLabelCell::from_glyph(u32::MAX).is_none());
    }
}
