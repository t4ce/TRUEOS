#![no_std]
#![forbid(unsafe_code)]

//! Seeded 32 × 32 flower, snowflake, and mosaic growth atlases based on Motif.
//! The renderer writes to caller-owned memory and requires no allocator.
//!
//! ```
//! use microflake::{Settings, render_atlas};
//!
//! let settings = Settings { background_alpha: 0, ..Settings::default() };
//! let mut pixels = vec![0; settings.steps.byte_len()];
//! render_atlas(&settings, &mut pixels).unwrap();
//! ```

use core::f64::consts::{FRAC_PI_2, PI, TAU};
use libm::{atan2, cos, pow, sin, sqrt};

pub const TILE_SIZE: usize = 32;
pub const TILE_BYTES: usize = TILE_SIZE * TILE_SIZE * 4;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Motif {
    #[default]
    Flower,
    Snowflake,
    Mosaic,
    /// Cycles through the three families in an atlas.
    Mix,
}

impl Motif {
    /// Resolves Mix to a family for a frame or a stable caller-provided index.
    pub const fn family(self, index: usize) -> Self {
        match (self, index % 3) {
            (Self::Mix, 0) => Self::Flower,
            (Self::Mix, 1) => Self::Snowflake,
            (Self::Mix, _) => Self::Mosaic,
            (family, _) => family,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Symmetry {
    Four = 4,
    #[default]
    Six = 6,
    Eight = 8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Steps {
    #[default]
    Sixteen = 16,
    ThirtyTwo = 32,
    FortyEight = 48,
}

impl Steps {
    pub const fn count(self) -> usize {
        self as usize
    }

    pub const fn columns(self) -> usize {
        match self {
            Self::Sixteen => 4,
            Self::ThirtyTwo | Self::FortyEight => 8,
        }
    }

    pub const fn width(self) -> usize {
        self.columns() * TILE_SIZE
    }

    pub const fn height(self) -> usize {
        self.count() / self.columns() * TILE_SIZE
    }

    pub const fn byte_len(self) -> usize {
        self.width() * self.height() * 4
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Palette {
    #[default]
    Auto,
    Ember,
    Lagoon,
    Cobalt,
    Orchid,
    Saffron,
    Porcelain,
    /// Tonal shades of the supplied Lilly cursor accent.
    Spirit {
        accent: [u8; 3],
    },
}

#[derive(Clone, Copy, Debug)]
pub struct Settings<'a> {
    pub motif: Motif,
    pub seed: &'a str,
    pub symmetry: Symmetry,
    pub steps: Steps,
    pub palette: Palette,
    /// Defaults to 60. The balanced renderer clamps this to 20–90.
    pub contrast: u8,
    /// Straight alpha applied only to the background. Defaults to 255.
    pub background_alpha: u8,
}

impl Default for Settings<'_> {
    fn default() -> Self {
        Self {
            motif: Motif::Flower,
            seed: "garden",
            symmetry: Symmetry::Six,
            steps: Steps::Sixteen,
            palette: Palette::Auto,
            contrast: 60,
            background_alpha: 255,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BufferTooSmall {
    pub required: usize,
}

/// Writes a tightly packed, straight-alpha RGBA8 atlas and returns the written
/// slice. Frames run left to right, then top to bottom. Extra bytes are untouched.
///
/// Growth starts with both controls at 0% and ends with both at 100%. Those
/// percentages span Motif's balanced ranges. Every fourth transition, starting
/// with the first, advances density. The other transitions advance detail.
pub fn render_atlas<'a>(
    settings: &Settings<'_>,
    rgba: &'a mut [u8],
) -> Result<&'a mut [u8], BufferTooSmall> {
    let steps = settings.steps;
    let required = steps.byte_len();
    let rgba = rgba
        .get_mut(..required)
        .ok_or(BufferTooSmall { required })?;
    for frame in 0..steps.count() {
        draw_frame(
            settings,
            frame,
            rgba,
            steps.width() * 4,
            frame % steps.columns() * TILE_SIZE,
            frame / steps.columns() * TILE_SIZE,
        );
    }
    Ok(rgba)
}

/// Draws one frame without allocating the atlas. Out-of-range indices use the last frame.
pub fn render_tile(settings: &Settings<'_>, frame: usize, rgba: &mut [u8; TILE_BYTES]) {
    draw_frame(
        settings,
        frame.min(settings.steps.count() - 1),
        rgba,
        TILE_SIZE * 4,
        0,
        0,
    );
}

fn draw_frame(
    settings: &Settings<'_>,
    frame: usize,
    rgba: &mut [u8],
    stride: usize,
    origin_x: usize,
    origin_y: usize,
) {
    let motif = settings.motif.family(frame);
    let random = random_values(settings.seed, motif);
    let colors = palette(settings, random[0]);
    let n = settings.symmetry as usize;
    let (density, detail) = growth(frame, settings.steps);
    let mut tile = Tile {
        rgba,
        origin_x,
        origin_y,
        stride,
        colors: &colors,
    };
    match motif {
        Motif::Flower => flower(&mut tile, n, density, detail, &random),
        Motif::Snowflake => snowflake(&mut tile, n, density, detail, &random),
        Motif::Mosaic => mosaic(&mut tile, n, density, detail, &random),
        Motif::Mix => unreachable!(),
    }
}

fn growth(frame: usize, steps: Steps) -> (f64, f64) {
    let density_steps = steps.count() / 4;
    let detail_steps = steps.count() - 1 - density_steps;
    let density = frame.div_ceil(4);
    let detail = frame - density;
    (
        0.20 + 0.62 * density as f64 / density_steps as f64,
        0.10 + 0.68 * detail as f64 / detail_steps as f64,
    )
}

struct Tile<'a> {
    rgba: &'a mut [u8],
    origin_x: usize,
    origin_y: usize,
    stride: usize,
    colors: &'a [[u8; 4]; 6],
}

impl Tile<'_> {
    fn draw(&mut self, mut ink: impl FnMut(usize, usize, f64, f64) -> usize) {
        for y in 0..TILE_SIZE {
            for x in 0..TILE_SIZE {
                let offset = (self.origin_y + y) * self.stride + (self.origin_x + x) * 4;
                let index = ink(x, y, x as f64 - 15.5, y as f64 - 15.5);
                self.rgba[offset..offset + 4].copy_from_slice(&self.colors[index]);
            }
        }
    }
}

fn random_values(seed: &str, motif: Motif) -> [f64; 8] {
    let suffix = match motif {
        Motif::Flower => ":flower",
        Motif::Snowflake => ":snowflake",
        Motif::Mosaic => ":mosaic",
        Motif::Mix => unreachable!(),
    };
    let mut state = 2_166_136_261u32;
    for c in seed.chars().chain(suffix.chars()) {
        let unit = c.encode_utf16(&mut [0; 2])[0];
        state = (state ^ u32::from(unit)).wrapping_mul(16_777_619);
    }
    core::array::from_fn(|_| {
        state = state.wrapping_add(0x6D2B79F5);
        let mut a = (state ^ (state >> 15)).wrapping_mul(1 | state);
        a ^= a.wrapping_add((a ^ (a >> 7)).wrapping_mul(61 | a));
        f64::from(a ^ (a >> 14)) / 4_294_967_296.0
    })
}

fn palette(settings: &Settings<'_>, random: f64) -> [[u8; 4]; 6] {
    const COLORS: [[u32; 6]; 6] = [
        [0x202735, 0x864352, 0xE87565, 0xFFB072, 0xE6C86D, 0xFFF0C5],
        [0x163A38, 0x32675F, 0x5EA393, 0x91C5A6, 0xD3D09B, 0xFFF1CC],
        [0x1E2D49, 0x3D5988, 0x628DCA, 0x9DC7EA, 0xDED7A7, 0xFFF4DD],
        [0x30263D, 0x6C5184, 0xAA7ABC, 0xE5A0C1, 0xF0BC7A, 0xFFF0D0],
        [0x333A2D, 0x697750, 0xA1AD70, 0xD7CA7D, 0xE58D61, 0xFFF0C5],
        [0xEDE5D2, 0x304C58, 0x568394, 0x7AA5A4, 0xC56F54, 0xFFF9EA],
    ];
    let base = match settings.palette {
        Palette::Auto => COLORS[(random * COLORS.len() as f64) as usize],
        Palette::Ember => COLORS[0],
        Palette::Lagoon => COLORS[1],
        Palette::Cobalt => COLORS[2],
        Palette::Orchid => COLORS[3],
        Palette::Saffron => COLORS[4],
        Palette::Porcelain => COLORS[5],
        Palette::Spirit { accent } => {
            let shade = |factor: u16, white: u16| {
                let [r, g, b] =
                    accent.map(|c| u32::from((u16::from(c) * factor + 255 * white) / 100));
                (r << 16) | (g << 8) | b
            };
            [
                shade(18, 0),
                shade(53, 0),
                shade(78, 0),
                shade(100, 0),
                shade(65, 35),
                shade(25, 75),
            ]
        }
    };
    let contrast = 0.48 + f64::from(settings.contrast.clamp(20, 90)) * 0.0079;
    let channel = |color: u32, shift: u32| f64::from((color >> shift) & 255);
    core::array::from_fn(|i| {
        let rgb = [16, 8, 0].map(|shift| {
            let bg = channel(base[0], shift);
            (bg + (channel(base[i], shift) - bg) * contrast + 0.5).clamp(0.0, 255.0) as u8
        });
        let alpha = if i == 0 {
            settings.background_alpha
        } else {
            255
        };
        [rgb[0], rgb[1], rgb[2], alpha]
    })
}

fn square(x: f64) -> f64 {
    x * x
}

fn flower(tile: &mut Tile<'_>, n: usize, density: f64, detail: f64, r: &[f64; 8]) {
    let d = density;
    let rotation = -FRAC_PI_2 + if r[1] > 0.58 { PI / n as f64 } else { 0.0 };
    let petal_center = (6.5 + d * 1.9) * (0.91 + r[5] * 0.17);
    let petal_length = (4.0 + d * 1.6) * (0.84 + r[6] * 0.29);
    let petal_width = (2.25 + d * 1.9) * pow(6.0 / n as f64, 0.6) * (0.78 + r[7] * 0.4);
    let inner_center = 3.9 + d * 0.7;
    let inner_length = 2.6 + d * 0.9;
    let center_radius = 1.8 + d * 1.5 + (r[2] - 0.5) * 0.5;
    let twist = if r[3] > 0.5 { 0.5 * PI / n as f64 } else { 0.0 };
    let mut petals = [[0.0; 4]; 8];
    for (i, petal) in petals[..n].iter_mut().enumerate() {
        let angle = rotation + i as f64 * TAU / n as f64;
        *petal = [
            cos(angle),
            sin(angle),
            cos(angle + twist),
            sin(angle + twist),
        ];
    }

    tile.draw(|x, y, dx, dy| {
        let radius = sqrt(square(dx) + square(dy));
        let (mut best, mut inner) = (f64::INFINITY, f64::INFINITY);
        let (mut petal_u, mut petal_v) = (0.0, 0.0);
        for &[c, s, ic, is] in &petals[..n] {
            let u = dx * c + dy * s;
            let v = -dx * s + dy * c;
            let ellipse = square((u - petal_center) / petal_length) + square(v / petal_width);
            if ellipse < best {
                best = ellipse;
                (petal_u, petal_v) = (u, v);
            }
            inner = inner.min(
                square((dx * ic + dy * is - inner_center) / inner_length)
                    + square((-dx * is + dy * ic) / (petal_width * 0.78)),
            );
        }

        let mut ink = 0;
        if best <= 1.14 && detail > 0.22 {
            ink = 1;
        }
        if best <= 1.0 {
            ink = if best > 0.76 { 2 } else { 3 };
            if r[4] > 0.5 {
                ink -= 1;
            }
            if detail > 0.30
                && petal_v.abs() < 0.7
                && petal_u > 6.2
                && petal_u < petal_center + petal_length - 1.0
            {
                ink = 4;
            }
            if detail > 0.58 && best < 0.53 && petal_u > petal_center + 1.5 {
                ink = 5;
            }
        }
        if detail > 0.18 && inner < 1.0 {
            ink = if inner > 0.68 { 2 } else { 4 };
        }
        if detail > 0.60 && inner < 0.45 {
            ink = 3;
        }
        if radius < center_radius + 0.8 {
            ink = 1;
        }
        if radius < center_radius {
            ink = 4;
        }
        if radius < center_radius - 1.15 {
            ink = 5;
        }
        if detail > 0.68 && radius < center_radius - 0.3 && (x + y) % 2 == 0 {
            ink = 3;
        }
        ink
    });
}

#[derive(Clone, Copy, Default)]
struct Segment {
    x: f64,
    vx: f64,
    vy: f64,
    projection_scale: f64,
    distance_scale: f64,
    ink: usize,
}

impl Segment {
    fn new(x: f64, vx: f64, vy: f64, weight: f64, ink: usize) -> Self {
        Self {
            x,
            vx,
            vy,
            projection_scale: 1.0 / (square(vx) + square(vy)),
            distance_scale: 1.0 / square(weight),
            ink,
        }
    }

    fn distance_squared(self, x: f64, y: f64) -> f64 {
        let x = x - self.x;
        let t = ((x * self.vx + y * self.vy) * self.projection_scale).clamp(0.0, 1.0);
        (square(x - t * self.vx) + square(y - t * self.vy)) * self.distance_scale
    }
}

fn snowflake(tile: &mut Tile<'_>, n: usize, density: f64, detail: f64, r: &[f64; 8]) {
    let d = density;
    let rotation = -FRAC_PI_2 + if r[1] > 0.78 { PI / n as f64 } else { 0.0 };
    let length = 10.6 + d * 3.9;
    let width = 0.43 + d * 0.37;
    let branch_count = 1 + (detail * 4.99) as usize;
    let branch_angle = if r[2] > 0.5 { PI / 3.0 } else { PI / 4.0 };
    let mut directions = [(0.0, 0.0); 8];
    for (i, direction) in directions[..n].iter_mut().enumerate() {
        let angle = rotation + i as f64 * TAU / n as f64;
        *direction = (cos(angle), sin(angle));
    }

    // All arms share the same segment geometry. Rotate the sample into arm space.
    let mut segments = [Segment::default(); 11];
    segments[0] = Segment::new(0.0, length, 0.0, 1.0, 4);
    let mut count = 1;
    for j in 0..branch_count {
        let t = (j + 1) as f64 / (branch_count + 1) as f64;
        let reach = length * (0.30 + t * 0.55);
        let spread = if n == 8 { 0.8 } else { 1.0 };
        let branch = (2.3 + d * 3.7) * (1.0 - t * 0.5) * spread * (0.9 + r[4] * 0.2);
        for sign in [-1.0, 1.0] {
            segments[count] = Segment::new(
                reach,
                cos(branch_angle) * branch,
                sin(branch_angle) * branch * sign,
                0.8,
                if j % 2 == 0 { 5 } else { 3 },
            );
            count += 1;
            if detail > 0.76 && j == 0 {
                segments[count] = Segment::new(
                    reach * 0.6,
                    cos(PI / 3.0) * 2.0,
                    sin(PI / 3.0) * -2.0 * sign,
                    0.7,
                    3,
                );
                count += 1;
            }
        }
    }
    let core_width = square(width);
    let outline_width = square(width + 0.48);
    tile.draw(|_, _, dx, dy| {
        let radius = sqrt(square(dx) + square(dy));
        let mut nearest = f64::INFINITY;
        let mut nearest_ink = 0;
        for &(c, s) in &directions[..n] {
            let x = dx * c + dy * s;
            let y = -dx * s + dy * c;
            for &segment in &segments[..count] {
                let distance = segment.distance_squared(x, y);
                if distance < nearest {
                    nearest = distance;
                    nearest_ink = segment.ink;
                }
            }
        }
        let mut ink = if nearest <= core_width {
            nearest_ink
        } else if d > 0.40 && nearest <= outline_width {
            1
        } else {
            0
        };
        if radius < 1.2 + d * 1.8 {
            ink = 3;
        }
        if radius < 0.8 + d * 0.8 {
            ink = 5;
        }
        if detail > 0.38 && radius > 2.3 + d && radius < 3.2 + d && nearest < square(1.5) {
            ink = 2;
        }
        ink
    });
}

fn mosaic(tile: &mut Tile<'_>, n: usize, d: f64, detail: f64, r: &[f64; 8]) {
    let family = (r[1] * 3.0) as usize;
    let extent = 8.8 + d * 5.6;
    let ring = 1.3 + (1.0 - detail) * 2.5;
    let corner_extent = 2.7 + d * 4.4;
    let sector = TAU / n as f64;
    let apothem = cos(PI / n as f64);
    tile.draw(|x, y, dx, dy| {
        let angle = atan2(dy, dx);
        let radius = sqrt(square(dx) + square(dy));
        let angular = cos(n as f64 * angle);
        let folded = ((angle + sector / 2.0) % sector + sector) % sector - sector / 2.0;
        let shape = match family {
            0 => radius * cos(folded) / apothem,
            1 => radius / (0.85 + 0.15 * angular),
            _ => radius / (0.74 + 0.26 * angular),
        };
        let edge = x.min(31 - x).min(y).min(31 - y);
        let corner = sqrt(square(16.0 - dx.abs()) + square(16.0 - dy.abs()));
        let mut ink = 0;
        if corner < corner_extent + 1.2 {
            ink = 1;
        }
        if corner < corner_extent {
            ink = 4;
        }
        if corner < corner_extent - ring {
            ink = 2;
        }
        if corner < corner_extent - ring * 1.8 {
            ink = 5;
        }
        if shape < extent + 1.0 {
            ink = 1;
        }
        if shape < extent {
            let band = ((extent - shape) / ring) as usize;
            ink = [2, 3, 1, 4, 5, 3, 2, 4][band % 8];
            if family == 1 && detail > 0.36 && (dx.abs() - dy.abs()).abs() < 0.55 {
                ink = 5;
            }
            if family == 2
                && detail > 0.36
                && sin(n as f64 * angle).abs() < 0.16
                && shape < extent - 1.0
            {
                ink = 4;
            }
        }
        if shape < 2.1 + d {
            ink = 5;
        }
        if detail > 0.72
            && shape > extent + 2.0
            && corner > corner_extent + 1.0
            && (dx.abs() == 0.5 || dy.abs() == 0.5)
        {
            ink = 3;
        }
        if detail > 0.58 && edge == 1 && shape > extent + 1.0 && corner > corner_extent + 1.2 {
            ink = 2;
        }
        ink
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn growth_advances_one_control_at_a_time() {
        for steps in [Steps::Sixteen, Steps::ThirtyTwo, Steps::FortyEight] {
            assert_eq!(growth(0, steps), (0.20, 0.10));
            let (density, detail) = growth(steps.count() - 1, steps);
            assert!((density - 0.82).abs() < 1e-12);
            assert!((detail - 0.78).abs() < 1e-12);
            for frame in 1..steps.count() {
                let before = growth(frame - 1, steps);
                let after = growth(frame, steps);
                if frame % 4 == 1 {
                    assert!(after.0 > before.0);
                    assert_eq!(after.1, before.1);
                } else {
                    assert_eq!(after.0, before.0);
                    assert!(after.1 > before.1);
                }
            }
        }
    }

    #[test]
    fn spirit_mix_tile_matches_the_atlas() {
        let settings = Settings {
            motif: Motif::Mix,
            palette: Palette::Spirit {
                accent: [32, 168, 255],
            },
            background_alpha: 0,
            ..Settings::default()
        };
        let mut atlas = [0; Steps::Sixteen.byte_len()];
        render_atlas(&settings, &mut atlas).unwrap();
        let mut tile = [0; TILE_BYTES];
        for frame in 0..3 {
            render_tile(&settings, frame, &mut tile);
            let family_settings = Settings {
                motif: settings.motif.family(frame),
                ..settings
            };
            let mut family_tile = [0; TILE_BYTES];
            render_tile(&family_settings, frame, &mut family_tile);
            assert_eq!(tile, family_tile);
            for (y, row) in tile.chunks_exact(TILE_SIZE * 4).enumerate() {
                let start = (y * settings.steps.width() + frame * TILE_SIZE) * 4;
                assert_eq!(row, &atlas[start..start + TILE_SIZE * 4]);
            }
            assert!(tile.chunks_exact(4).any(|p| p[3] == 0));
            assert!(tile.chunks_exact(4).any(|p| p[3] == 255 && p[2] > p[0]));
        }
    }

    #[test]
    fn renders_every_tile_with_background_alpha_only() {
        let mut rgba = [0; Steps::FortyEight.byte_len() + 1];
        for motif in [Motif::Flower, Motif::Snowflake] {
            for symmetry in [Symmetry::Four, Symmetry::Six, Symmetry::Eight] {
                let settings = Settings {
                    motif,
                    symmetry,
                    steps: Steps::FortyEight,
                    palette: Palette::Cobalt,
                    background_alpha: 73,
                    ..Settings::default()
                };
                let pixels = render_atlas(&settings, &mut rgba).unwrap();
                assert_eq!(pixels.len(), 256 * 192 * 4);
                assert!(pixels.chunks_exact(4).all(|p| match p[3] {
                    73 => p[..3] == [0x1e, 0x2d, 0x49],
                    255 => true,
                    _ => false,
                }));
                for frame in 0..settings.steps.count() {
                    let x = frame % settings.steps.columns() * TILE_SIZE;
                    let y = frame / settings.steps.columns() * TILE_SIZE;
                    let alpha = |x, y| pixels[(y * settings.steps.width() + x) * 4 + 3];
                    assert_eq!(alpha(x, y), 73);
                    assert_eq!(alpha(x + 15, y + 15), 255);
                }
                let foreground = |frame: usize| {
                    let x = frame % settings.steps.columns() * TILE_SIZE;
                    let y = frame / settings.steps.columns() * TILE_SIZE;
                    (y..y + TILE_SIZE)
                        .flat_map(|row| {
                            let start = (row * settings.steps.width() + x) * 4;
                            pixels[start..start + TILE_SIZE * 4].chunks_exact(4)
                        })
                        .filter(|p| p[3] == 255)
                        .count()
                };
                assert!(foreground(47) > foreground(0));
                assert_eq!(rgba[Steps::FortyEight.byte_len()], 0);
            }
        }
    }
}
