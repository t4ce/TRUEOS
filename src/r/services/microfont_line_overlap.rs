//! Compare three GPU-rendered cells: outer stroke, crossbar, combined A.
#[derive(Default, Debug, PartialEq, Eq)]
pub(super) struct Counts {
    pub background_pixel: u32,
    pub ink_pixel: u32,
    pub second_ink_pixel: u32,
    pub outer: usize,
    pub crossbar: usize,
    pub duplicated: usize,
    pub left: usize,
    pub right: usize,
    pub union: usize,
    pub combined: usize,
    pub mismatch: usize,
}

pub(super) fn compare(
    bytes: &[u8], pitch: usize, cell_width: usize, height: usize,
) -> Result<Counts, &'static str> {
    let row = cell_width.checked_mul(12).ok_or("overlap-layout")?;
    let size = pitch.checked_mul(height).ok_or("overlap-layout")?;
    if cell_width == 0 || height == 0 || pitch < row || bytes.len() < size {
        return Err("overlap-layout");
    }
    // Coverage is independent of channel order and any target color conversion.
    // Require opaque background plus at most two ink colors; never interpret arbitrary
    // unexpected pixels as successful stroke coverage.
    let background = &bytes[..4];
    if background[3] != 255 { return Err("overlap-background-alpha"); }
    let mut ink = None;
    let mut second_ink = None;
    for y in 0..height {
        for x in 0..3 * cell_width {
            let offset = y * pitch + x * 4;
            let pixel = &bytes[offset..offset + 4];
            if pixel[3] != 255 { return Err("overlap-ink-alpha"); }
            if pixel != background {
                match ink {
                    None => ink = Some(pixel),
                    Some(color) if color == pixel => {},
                    Some(_) => match second_ink {
                        None => second_ink = Some(pixel),
                        Some(color) if color == pixel => {},
                        Some(_) => return Err("overlap-multiple-ink-colors"),
                    },
                }
            }
        }
    }
    let ink = ink.ok_or("overlap-empty-frame")?;
    let mut counts = Counts {
        background_pixel: u32::from_le_bytes(background.try_into().unwrap()),
        ink_pixel: u32::from_le_bytes(ink.try_into().unwrap()),
        second_ink_pixel: second_ink.map(|p| u32::from_le_bytes(p.try_into().unwrap())).unwrap_or(0),
        ..Counts::default()
    };
    for y in 0..height {
        for x in 0..cell_width {
            let mut covered = [false; 3];
            for cell in 0..3 {
                let offset = y * pitch + (cell * cell_width + x) * 4;
                covered[cell] = &bytes[offset..offset + 4] != background;
            }
            let [a, b, combined] = covered;
            counts.outer += usize::from(a);
            counts.crossbar += usize::from(b);
            counts.duplicated += usize::from(a && b);
            counts.left += usize::from(a && b && x < cell_width / 2);
            counts.right += usize::from(a && b && x >= cell_width / 2);
            counts.union += usize::from(a || b);
            counts.combined += usize::from(combined);
            counts.mismatch += usize::from(combined != (a || b));
        }
    }
    Ok(counts)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn intersection_is_not_hidden_by_opaque_overlay_and_padding_is_ignored() {
        // Cell masks [0,1,1], [0,1,0], union [0,1,1], plus padding.
        let mut pixels = [255; 40];
        for x in [1, 2, 4, 7, 8] {
            pixels[x * 4..x * 4 + 4].copy_from_slice(&[48, 100, 216, 255]);
        }
        assert_eq!(compare(&pixels, 40, 3, 1), Ok(Counts {
            background_pixel: 0xffffffff, ink_pixel: 0xffd86430, second_ink_pixel: 0,
            outer: 2, crossbar: 1, duplicated: 1, left: 0, right: 1,
            union: 2, combined: 2, mismatch: 0,
        }));
        pixels[7 * 4..8 * 4].fill(255);
        assert_eq!(compare(&pixels, 40, 3, 1).unwrap().mismatch, 1);
    }
    #[test]
    fn invalid_layout_and_unexpected_colors_fail() {
        assert_eq!(compare(&[], 0, 0, 0), Err("overlap-layout"));
        assert_eq!(compare(&[255; 12], 11, 1, 1), Err("overlap-layout"));
        assert_eq!(compare(&[255; 11], 12, 1, 1), Err("overlap-layout"));
        assert_eq!(compare(&[0; 12], 12, 1, 1), Err("overlap-background-alpha"));
    }
}
