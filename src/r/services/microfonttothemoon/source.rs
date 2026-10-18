//! Runtime glyph discovery and image layout; all masks come from Microfont.
use super::mesh;
use alloc::{vec, vec::Vec};

pub const WIDTH: u32 = 2560;
pub const HEIGHT: u32 = 1152;
pub const SCALE: u32 = 8;
pub const TILE_WIDTH: u32 = 160;
pub const TILE_HEIGHT: u32 = 128;

pub struct Settings {
    pub mesh: mesh::Recipe,
    pub reach: f64,
    pub long_reach_value: f64,
    pub long_reach: Vec<char>,
}
impl Settings {
    pub fn reach_for(&self, character: char) -> f64 {
        if self.long_reach.contains(&character) {
            self.long_reach_value
        } else {
            self.reach
        }
    }
}
pub struct Glyph {
    pub character: char,
    pub cells: Vec<u128>,
}
impl Glyph {
    pub fn points(&self) -> Vec<[f64; 2]> {
        let mut points = Vec::new();
        for (cell, &bits) in self.cells.iter().enumerate() {
            for bit in 0..66 {
                if bits & (1u128 << (65 - bit)) != 0 {
                    points.push([(cell * 6 + bit % 6) as f64 + 0.5, (bit / 6) as f64 + 0.5]);
                }
            }
        }
        points
    }
    pub fn build(&self, settings: &Settings) -> Result<mesh::Mesh, &'static str> {
        mesh::build(
            &self.points(),
            self.cells.len() as f64 * 6.0,
            settings.mesh,
            settings.reach_for(self.character),
        )
    }
}
pub fn glyphs(han: bool) -> Result<Vec<Glyph>, &'static str> {
    let mut output = Vec::new();
    if han {
        for character in microfont::gui_han_characters() {
            let bits = microfont::gui_han_cell_pixels(character).ok_or("han-mask-missing")?;
            output.push(Glyph {
                character,
                cells: vec![bits],
            });
        }
        if output.len() != microfont::GUI_HAN_COUNT || output.len() != 512 {
            return Err("han-count");
        }
    } else {
        for byte in 0x20u8..=0xff {
            output.push(Glyph {
                character: microfont::glyph_character(byte).ok_or("atlas-slot")?,
                cells: vec![(microfont::font_pixels(byte) as u128) << 2],
            });
        }
        for cp in (0x2800..=0x28ff).chain(0x2580..=0x259f) {
            let character = char::from_u32(cp).ok_or("unicode-cell")?;
            output.push(Glyph {
                character,
                cells: vec![microfont::glyph_cell_pixels(character)],
            });
        }
        for character in ['\u{1f5fa}', '\u{2328}'] {
            let cells = microfont::SPECIAL
                .iter()
                .find(|(ch, _)| *ch == character)
                .ok_or("icon-missing")?
                .1;
            output.push(Glyph {
                character,
                cells: cells.to_vec(),
            });
        }
        if output.len() != 514 {
            return Err("studio-count");
        }
    }
    Ok(output)
}
pub struct Part {
    pub vertices: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    pub glyph_count: usize,
    pub long_reach_count: usize,
}
pub fn build_part(han: bool, part: usize, settings: &Settings) -> Result<Part, &'static str> {
    if part >= 4 {
        return Err("part-range");
    }
    let mut output = Part {
        vertices: Vec::new(),
        indices: Vec::new(),
        glyph_count: 0,
        long_reach_count: 0,
    };
    for (slot, glyph) in glyphs(han)?.into_iter().enumerate() {
        let column = slot % 32;
        let row = slot / 32;
        if (row / 9) * 2 + column / 16 != part {
            continue;
        }
        let built = glyph.build(settings)?;
        let base = u32::try_from(output.vertices.len()).map_err(|_| "vertex-count")?;
        let x0 = (column % 16) as f64 * TILE_WIDTH as f64;
        let y0 = (row % 9) as f64 * TILE_HEIGHT as f64;
        for p in built.positions {
            let x = x0 + SCALE as f64 + p[0] * SCALE as f64;
            let y = y0 + 2.5 * SCALE as f64 + p[1] * SCALE as f64;
            if !x.is_finite()
                || !y.is_finite()
                || x < x0
                || x > x0 + TILE_WIDTH as f64
                || y < y0
                || y > y0 + TILE_HEIGHT as f64
            {
                return Err("glyph-exceeds-tile");
            }
            output.vertices.push([
                (x / WIDTH as f64 * 2.0 - 1.0) as f32,
                (1.0 - y / HEIGHT as f64 * 2.0) as f32,
                0.5,
            ]);
        }
        for triangle in built.indices.chunks_exact(3) {
            let mut ids = [base + triangle[0], base + triangle[2], base + triangle[1]];
            let a = output.vertices[ids[0] as usize];
            let b = output.vertices[ids[1] as usize];
            let c = output.vertices[ids[2] as usize];
            let area = (b[0] as f64 - a[0] as f64) * (c[1] as f64 - a[1] as f64)
                - (b[1] as f64 - a[1] as f64) * (c[0] as f64 - a[0] as f64);
            if area == 0.0 {
                continue;
            }
            if area < 0.0 {
                ids.swap(1, 2);
            }
            output.indices.extend_from_slice(&ids);
        }
        output.glyph_count += 1;
        output.long_reach_count += usize::from(settings.long_reach.contains(&glyph.character));
    }
    Ok(output)
}
