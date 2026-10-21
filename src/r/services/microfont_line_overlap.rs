//! Compare three GPU-rendered cells: outer stroke, crossbar, combined A.

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
        assert_eq!(
            compare(&pixels, 40, 3, 1),
            Ok(Counts {
                background_pixel: 0xffffffff,
                ink_pixel: 0xffd86430,
                second_ink_pixel: 0,
                outer: 2,
                crossbar: 1,
                duplicated: 1,
                left: 0,
                right: 1,
                union: 2,
                combined: 2,
                mismatch: 0,
            })
        );
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
