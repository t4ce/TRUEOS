//! Pure, encoded-RGB grading model shared by the CHROMATIC controls and LUT.
//!
//! This follows the reference demo's selective yellow, shadow/highlight tint,
//! global saturation and hue-indexed saturation curve. It is not an HDR or
//! scene-referred transform. Nodes use Intel's actual identity codes rather
//! than the demo's uniformly normalized 0..16 sampling positions.

pub(crate) const LUT_EDGE: usize = 17;
pub(crate) const LUT_ENTRIES: usize = LUT_EDGE * LUT_EDGE * LUT_EDGE;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Settings {
    pub(crate) yellow: f32,
    pub(crate) shadows: f32,
    pub(crate) highlights: f32,
    pub(crate) saturation: f32,
    pub(crate) strength: f32,
    /// R, Y, G, C, B, M, R; the two red endpoints are linked by the UI.
    pub(crate) sat_curve: [f32; 7],
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            yellow: 0.0,
            shadows: 0.0,
            highlights: 0.0,
            saturation: 1.0,
            strength: 1.0,
            sat_curve: [0.0; 7],
        }
    }
}

fn finite_clamp(value: f32, low: f32, high: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(low, high)
    } else {
        fallback
    }
}

fn smooth(value: f32) -> f32 {
    let t = value.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn rgb_to_hsv([r, g, b]: [f32; 3]) -> [f32; 3] {
    let value = r.max(g).max(b);
    let delta = value - r.min(g).min(b);
    let hue = if delta <= 1e-8 {
        0.0
    } else if value == r {
        ((g - b) / delta + if g < b { 6.0 } else { 0.0 }) * 60.0
    } else if value == g {
        ((b - r) / delta + 2.0) * 60.0
    } else {
        ((r - g) / delta + 4.0) * 60.0
    };
    [
        if hue >= 360.0 { 0.0 } else { hue },
        if value > 0.0 { delta / value } else { 0.0 },
        value,
    ]
}

fn hsv_to_rgb(hue: f32, saturation: f32, value: f32) -> [f32; 3] {
    let h = hue / 60.0;
    let chroma = value * saturation;
    let x = chroma * (1.0 - (h % 2.0 - 1.0).abs());
    let base = value - chroma;
    let rgb = match h as u32 {
        0 => [chroma, x, 0.0],
        1 => [x, chroma, 0.0],
        2 => [0.0, chroma, x],
        3 => [0.0, x, chroma],
        4 => [x, 0.0, chroma],
        _ => [chroma, 0.0, x],
    };
    rgb.map(|channel| channel + base)
}

impl Settings {
    pub(crate) fn transform(&self, rgb: [f32; 3]) -> [f32; 3] {
        let input = rgb.map(|v| finite_clamp(v, 0.0, 1.0, 0.0));
        let strength = finite_clamp(self.strength, 0.0, 1.0, 1.0);
        // Besides avoiding work, this preserves identity exactly at every node.
        if strength == 0.0 || *self == Self::default() {
            return input;
        }
        let [hue, saturation, value] = rgb_to_hsv(input);
        let hue_distance = (hue - 60.0).abs();
        let yellow_delta = hue_distance.min(360.0 - hue_distance);
        let yellow_weight = libm::expf(-0.5 * (yellow_delta / 24.0) * (yellow_delta / 24.0))
            * (1.0 - smooth((yellow_delta - 36.0) / 24.0));
        let segment = (hue / 60.0) as usize;
        let fraction = smooth(hue / 60.0 - segment as f32);
        let curve_a = finite_clamp(self.sat_curve[segment], -1.0, 1.0, 0.0);
        let curve_b = finite_clamp(self.sat_curve[segment + 1], -1.0, 1.0, 0.0);
        let curve = curve_a + (curve_b - curve_a) * fraction;
        let new_saturation = (saturation
            * finite_clamp(self.saturation, 0.0, 1.8, 1.0)
            * (1.0 + curve)
            * (1.0 - 0.95 * finite_clamp(self.yellow, 0.0, 1.0, 0.0) * yellow_weight))
            .clamp(0.0, 1.0);
        let mut output = hsv_to_rgb(hue, new_saturation, value);
        let luma = input[0] * 0.2126 + input[1] * 0.7152 + input[2] * 0.0722;
        let dark_weight = (1.0 - luma) * (1.0 - luma) * libm::sqrtf(luma);
        let light_weight = luma * luma;
        let warmth = 0.27
            * (finite_clamp(self.shadows, -1.0, 1.0, 0.0) * dark_weight
                + finite_clamp(self.highlights, -1.0, 1.0, 0.0) * light_weight);
        output[0] += warmth;
        output[1] += warmth * 0.12;
        output[2] -= warmth;
        core::array::from_fn(|channel| {
            let mapped = output[channel].clamp(0.0, 1.0);
            (input[channel] + (mapped - input[channel]) * strength).clamp(0.0, 1.0)
        })
    }

    pub(crate) fn build_table(&self, table: &mut [u32; LUT_ENTRIES]) {
        for red in 0..LUT_EDGE {
            for green in 0..LUT_EDGE {
                for blue in 0..LUT_EDGE {
                    let input = [red, green, blue].map(|n| identity_code(n) as f32 / 1023.0);
                    let output = self.transform(input).map(|v| (v * 1023.0 + 0.5) as u32);
                    table[node_index(red, green, blue)] =
                        (output[0] << 20) | (output[1] << 10) | output[2];
                }
            }
        }
    }
}

pub(crate) const fn node_index(red: usize, green: usize, blue: usize) -> usize {
    (red * LUT_EDGE + green) * LUT_EDGE + blue
}

pub(crate) const fn identity_code(node: usize) -> u32 {
    if node >= LUT_EDGE - 1 {
        1023
    } else {
        node as u32 * 64
    }
}

pub(crate) fn unpack_node(packed: u32) -> [f32; 3] {
    [packed >> 20, packed >> 10, packed].map(|v| (v & 1023) as f32 / 1023.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_uses_prm_codes_and_blue_fastest_order() {
        let mut table = [0; LUT_ENTRIES];
        Settings::default().build_table(&mut table);
        for r in 0..LUT_EDGE {
            for g in 0..LUT_EDGE {
                for b in 0..LUT_EDGE {
                    assert_eq!(
                        table[node_index(r, g, b)],
                        identity_code(r) << 20 | identity_code(g) << 10 | identity_code(b)
                    );
                }
            }
        }
        assert_eq!(table[1], 64);
        assert_eq!(table[17], 64 << 10);
        assert_eq!(table[289], 64 << 20);
        assert_eq!(table[LUT_ENTRIES - 1], 0x3fff_ffff);
    }

    #[test]
    fn yellow_softening_preserves_red_green_anchors() {
        let settings = Settings {
            yellow: 1.0,
            ..Settings::default()
        };
        assert_eq!(settings.transform([1.0, 0.0, 0.0]), [1.0, 0.0, 0.0]);
        assert_eq!(settings.transform([0.0, 1.0, 0.0]), [0.0, 1.0, 0.0]);
        let yellow = settings.transform([1.0, 1.0, 0.0]);
        assert!((yellow[2] - 0.95).abs() < 1e-6);
    }

    #[test]
    fn zero_strength_bypasses_every_control() {
        let mut table = [0; LUT_ENTRIES];
        let mut identity = [0; LUT_ENTRIES];
        Settings {
            yellow: 1.0,
            shadows: -1.0,
            highlights: 1.0,
            saturation: 0.0,
            strength: 0.0,
            sat_curve: [1.0; 7],
        }
        .build_table(&mut table);
        Settings::default().build_table(&mut identity);
        assert_eq!(table, identity);
    }

    #[test]
    fn hue_curve_is_selective_and_table_stays_30_bit() {
        let mut settings = Settings::default();
        settings.sat_curve[2] = -1.0;
        assert_eq!(settings.transform([0.0, 1.0, 0.0]), [1.0; 3]);
        assert_eq!(settings.transform([0.0, 0.0, 1.0]), [0.0, 0.0, 1.0]);
        settings.shadows = -1.0;
        settings.highlights = 1.0;
        settings.saturation = 1.8;
        let mut table = [0; LUT_ENTRIES];
        settings.build_table(&mut table);
        assert!(table.iter().all(|v| v & 0xc000_0000 == 0));
    }
}
