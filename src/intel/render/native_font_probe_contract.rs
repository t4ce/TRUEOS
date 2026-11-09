//! Geometry and readback oracle for the explicit native Microfont R8 probe.
pub(crate) const EXTENT: u32 = 16;
pub(crate) const VERTICES: [[f32; 3]; 4] = [
    [4.0, 4.0, -1.0],
    [12.0, 4.0, -1.0],
    [12.0, 12.0, -1.0],
    [4.0, 12.0, -1.0],
];
pub(crate) const INDICES: [u32; 6] = [0, 1, 2, 0, 2, 3];

/// Native mask.vert's stored-quad mode, unit source scale, no cell gutter.
pub(crate) fn parameters() -> [u32; 16] {
    let mut words = [0; 16];
    words[12] = 1.0f32.to_bits();
    words[13] = 1.0f32.to_bits();
    words[14] = (1.0 / EXTENT as f32).to_bits();
    words[15] = (1.0 / EXTENT as f32).to_bits();
    words
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Pixels {
    pub(crate) white_inside: u32,
    pub(crate) black_outside: u32,
    pub(crate) mismatched: u32,
}

impl Pixels {
    pub(crate) const fn passed(self) -> bool {
        self.white_inside == 64 && self.black_outside == 192 && self.mismatched == 0
    }
}

pub(crate) fn classify(pixels: &[u8], pitch: usize) -> Option<Pixels> {
    let extent = EXTENT as usize;
    if pitch < extent || pixels.len() < pitch.checked_mul(extent)? {
        return None;
    }
    let mut result = Pixels::default();
    for y in 0..extent {
        for x in 0..extent {
            let inside = (4..12).contains(&x) && (4..12).contains(&y);
            match (inside, pixels[y * pitch + x]) {
                (true, 255) => result.white_inside += 1,
                (false, 0) => result.black_outside += 1,
                _ => result.mismatched += 1,
            }
        }
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expected(pitch: usize) -> Vec<u8> {
        let mut pixels = vec![0; pitch * EXTENT as usize];
        for y in 4..12 {
            pixels[y * pitch + 4..y * pitch + 12].fill(255);
        }
        pixels
    }

    #[test]
    fn native_quad_parameters_have_no_atlas_or_cell_size_dependency() {
        let p = parameters().map(f32::from_bits);
        assert_eq!(&p[..12], &[0.0; 12]);
        assert_eq!(&p[12..], &[1.0, 1.0, 1.0 / 16.0, 1.0 / 16.0]);
        assert_eq!(INDICES, [0, 1, 2, 0, 2, 3]);
        for vertex in VERTICES {
            assert_eq!(vertex[2], -1.0, "stored quad selects slot zero");
            let clip = [vertex[0] * 2.0 / 16.0 - 1.0, 1.0 - vertex[1] * 2.0 / 16.0];
            assert!(clip.iter().all(|v| v.abs() == 0.5));
        }
    }

    #[test]
    fn exact_inset_passes_with_padding_ignored() {
        let mut pixels = expected(64);
        for row in pixels.chunks_exact_mut(64) {
            row[16..].fill(0xA5);
        }
        assert!(classify(&pixels, 64).unwrap().passed());
    }

    #[test]
    fn blank_target_full_target_and_partial_coverage_cannot_pass() {
        assert_eq!(classify(&[0; 256], 16).unwrap().mismatched, 64);
        assert_eq!(classify(&[255; 256], 16).unwrap().mismatched, 192);
        let mut pixels = expected(16);
        pixels[4 * 16 + 4] = 128;
        assert!(!classify(&pixels, 16).unwrap().passed());
        pixels[4 * 16 + 4] = 255;
        pixels[0] = 255;
        assert_eq!(classify(&pixels, 16).unwrap().mismatched, 1);
    }

    #[test]
    fn malformed_readback_is_rejected() {
        assert!(classify(&[0; 256], 15).is_none());
        assert!(classify(&[0; 255], 16).is_none());
        assert!(classify(&[], usize::MAX).is_none());
    }
}
