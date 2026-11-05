//! Bounded reference implementation of Edit's glyph-driven Conway field.
//! Input coverage and straight glyph colors remain immutable. Only this
//! task-owned world and its previous-input comparison image are changed.

use alloc::vec::Vec;

const MAX_SIDE: u32 = 512;
const MAX_CANONICAL_PIXELS: usize = 2 * 1024 * 1024;
pub(crate) const BACKGROUND_ALPHA: u8 = 153; // Static 60% opacity.
const COVERAGE_THRESHOLD: u32 = 46; // GLSL step(.18, coverage), in R8 bytes.

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct Cell {
    alive: bool,
    trail: u8,
    born: bool,
    rgb: [u8; 3],
}

pub(crate) struct Life {
    width: u32,
    height: u32,
    grid_width: u32,
    grid_height: u32,
    cells: Vec<Cell>,
    next: Vec<Cell>,
    previous: Vec<u8>,
    previous_hex: Vec<[u32; 4]>,
    previous_hex_layout: Option<[u32; 3]>,
    canonical_rgba: Vec<[u8; 4]>,
    visible_width: u32,
    visible_height: u32,
    initialized: bool,
    generation: u64,
}

impl Life {
    pub(crate) fn new(width: u32, height: u32) -> Option<Self> {
        if width == 0 || height == 0 { return None; }
        let grid_width = width.div_ceil(3).min(MAX_SIDE);
        let grid_height = height.div_ceil(3).min(MAX_SIDE);
        let count = (grid_width as usize).checked_mul(grid_height as usize)?;
        let mut cells = Vec::new();
        let mut next = Vec::new();
        cells.try_reserve_exact(count).ok()?;
        next.try_reserve_exact(count).ok()?;
        cells.resize(count, Cell::default());
        next.resize(count, Cell::default());
        Some(Self { width, height, grid_width, grid_height, cells, next,
            previous: Vec::new(), previous_hex: Vec::new(), previous_hex_layout: None,
            canonical_rgba: Vec::new(), visible_width: grid_width, visible_height: grid_height,
            initialized: false, generation: 0 })
    }

    pub(crate) fn extent(&self) -> (u32, u32) { (self.width, self.height) }

    pub(crate) fn generation(&self) -> u64 { self.generation }

    /// Capture arrays are tightly packed full-page R8 and straight RGBA8.
    /// `rect` is the editor viewport in page pixels. Whitespace-only input
    /// still refreshes the comparison baseline, without emitting new cells.
    pub(crate) fn snapshot(
        &mut self, mask: &[u8], colors: &[u8], page_width: u32,
        rect: [u32; 4], reset: bool, emit: bool,
    ) -> bool {
        let [left, top, width, height] = rect;
        if width != self.width || height != self.height || page_width == 0
            || left.checked_add(width).is_none_or(|right| right > page_width)
        { return false; }
        let Some(end) = top.checked_add(height).and_then(|bottom|
            (bottom as usize).checked_mul(page_width as usize)) else { return false; };
        if end > mask.len() || end.checked_mul(4).is_none_or(|end| end > colors.len()) {
            return false;
        }
        let switching_from_hex = self.previous_hex_layout.is_some();
        if !self.resize_grid(width.div_ceil(3).min(MAX_SIDE), height.div_ceil(3).min(MAX_SIDE)) {
            return false;
        }
        // The bitmap path uses a cell-sized baseline. Allocate the legacy
        // per-pixel comparison image only when this reference API is used.
        let Some(pixels) = (width as usize).checked_mul(height as usize) else { return false; };
        if self.previous.len() != pixels {
            if self.previous.try_reserve_exact(pixels.saturating_sub(self.previous.len())).is_err() {
                return false;
            }
            self.previous.resize(pixels, 0);
        }
        let reseed = reset || !self.initialized || switching_from_hex;
        if reseed || emit {
            for y in 0..self.grid_height {
                for x in 0..self.grid_width {
                    let (coverage, rgb) = self.input_cell(x, y, |sx, sy| {
                        let at = (top as usize + sy as usize) * page_width as usize + left as usize + sx as usize;
                        let input = mask[at];
                        let value = if !reseed {
                            input.saturating_sub(self.previous[sy as usize * width as usize + sx as usize])
                        } else { input };
                        (value, [colors[at * 4], colors[at * 4 + 1], colors[at * 4 + 2]])
                    });
                    let index = (y * self.grid_width + x) as usize;
                    let old = self.cells[index];
                    if reseed {
                        let alive = coverage >= COVERAGE_THRESHOLD;
                        self.cells[index] = Cell { alive, trail: if alive { 255 } else { 0 },
                            born: alive, rgb: if alive { rgb } else { [0; 3] } };
                    } else if coverage >= COVERAGE_THRESHOLD {
                        self.cells[index] = Cell { alive: true, trail: 255,
                            born: old.born || !old.alive,
                            rgb: if old.alive { add_rgb(old.rgb, rgb) } else { rgb } };
                    }
                }
            }
        }
        for y in 0..height as usize {
            let source = (top as usize + y) * page_width as usize + left as usize;
            let target = y * width as usize;
            self.previous[target..target + width as usize]
                .copy_from_slice(&mask[source..source + width as usize]);
        }
        if reseed { self.generation = 0; }
        self.previous_hex_layout = None;
        self.previous_hex.clear();
        self.canonical_rgba = Vec::new();
        self.visible_width = self.grid_width;
        self.visible_height = self.grid_height;
        self.initialized = true;
        true
    }

    /// Snapshot viewport-local cells [glyph, foreground, background, flags].
    /// The world is exactly six by eleven pixels per terminal cell. Glyph
    /// bits seed those same pixels directly; physical size affects only the
    /// final nearest-neighbor copy, never Conway or its retained page input.
    pub(crate) fn snapshot_hex(&mut self, cells: &[[u32; 4]], columns: u32,
        cell_width: u32, cell_height: u32, reset: bool, emit: bool) -> bool {
        if cell_width < 6 || cell_height < 11 || cell_width > 256 || cell_height > 256
            || columns != self.width.div_ceil(cell_width) {
            return false;
        }
        let Some(count) = columns.checked_mul(self.height.div_ceil(cell_height))
            .and_then(|count| usize::try_from(count).ok()) else { return false; };
        if cells.len() != count { return false; }
        let Some(grid_width) = columns.checked_mul(6) else { return false; };
        let Some(grid_height) = self.height.div_ceil(cell_height).checked_mul(11) else { return false; };
        let Some(pixels) = (grid_width as usize).checked_mul(grid_height as usize)
            .filter(|&pixels| pixels <= MAX_CANONICAL_PIXELS) else { return false; };
        if self.previous_hex.try_reserve_exact(count.saturating_sub(self.previous_hex.len())).is_err()
            || self.canonical_rgba.try_reserve_exact(pixels.saturating_sub(self.canonical_rgba.len())).is_err()
            || !self.resize_grid(grid_width, grid_height) { return false; }
        self.canonical_rgba.resize(pixels, [0; 4]);
        self.visible_width = (self.width - 1) / cell_width * 6
            + ((self.width - 1) % cell_width) * 6 / cell_width + 1;
        self.visible_height = (self.height - 1) / cell_height * 11
            + ((self.height - 1) % cell_height) * 11 / cell_height + 1;
        let layout = [columns, cell_width, cell_height];
        let reseed = reset || !self.initialized || self.previous_hex_layout != Some(layout);
        if reseed || emit {
            for y in 0..self.grid_height {
                for x in 0..self.grid_width {
                    let index = (y * self.grid_width + x) as usize;
                    if x >= self.visible_width || y >= self.visible_height {
                        self.cells[index] = Cell::default();
                        continue;
                    }
                    let (coverage, rgb) = hex_input(cells, columns, 6, 11, x, y);
                    let coverage = if !reseed {
                        coverage.saturating_sub(hex_input(&self.previous_hex, columns, 6, 11, x, y).0)
                    } else { coverage };
                    let old = self.cells[index];
                    if reseed {
                        let alive = coverage != 0;
                        self.cells[index] = Cell { alive, trail: if alive { 255 } else { 0 },
                            born: alive, rgb: if alive { rgb } else { [0; 3] } };
                    } else if coverage != 0 {
                        self.cells[index] = Cell { alive: true, trail: 255,
                            born: old.born || !old.alive,
                            rgb: if old.alive { add_rgb(old.rgb, rgb) } else { rgb } };
                    }
                }
            }
        }
        self.previous_hex.clear();
        self.previous_hex.extend_from_slice(cells);
        self.previous_hex_layout = Some(layout);
        self.previous = Vec::new();
        if reseed { self.generation = 0; }
        self.initialized = true;
        self.resolve_hex();
        true
    }

    fn resize_grid(&mut self, width: u32, height: u32) -> bool {
        if (width, height) == (self.grid_width, self.grid_height) { return true; }
        let Some(count) = (width as usize).checked_mul(height as usize) else { return false; };
        let mut cells = Vec::new();
        let mut next = Vec::new();
        if cells.try_reserve_exact(count).is_err() || next.try_reserve_exact(count).is_err() { return false; }
        cells.resize(count, Cell::default());
        next.resize(count, Cell::default());
        self.cells = cells;
        self.next = next;
        self.grid_width = width;
        self.grid_height = height;
        self.initialized = false;
        true
    }

    fn input_cell(&self, x: u32, y: u32,
        mut sample: impl FnMut(u32, u32) -> (u8, [u8; 3])) -> (u32, [u8; 3]) {
        // Nine bilinear taps reproduce the demo's .34-cell reach. Fixed-point
        // arithmetic keeps native capture independent of a float math runtime.
        let cx = ((u64::from(x) * 2 + 1) * u64::from(self.width) * 128
            / u64::from(self.grid_width)) as i64 - 128;
        let cy = ((u64::from(y) * 2 + 1) * u64::from(self.height) * 128
            / u64::from(self.grid_height)) as i64 - 128;
        let reach_x = (u64::from(self.width) * 87 / u64::from(self.grid_width)) as i64;
        let reach_y = (u64::from(self.height) * 87 / u64::from(self.grid_height)) as i64;
        let mut coverage = 0;
        let mut strongest = 0;
        let mut chosen = [220, 226, 232];
        for dy in -1..=1i64 { for dx in -1..=1i64 {
            let qx = cx + dx * reach_x;
            let qy = cy + dy * reach_y;
            let bx = qx.div_euclid(256);
            let by = qy.div_euclid(256);
            let fx = qx.rem_euclid(256) as u32;
            let fy = qy.rem_euclid(256) as u32;
            let mut alpha = 0;
            for j in 0..2 { for i in 0..2 {
                let sx = (bx + i).clamp(0, i64::from(self.width) - 1) as u32;
                let sy = (by + j).clamp(0, i64::from(self.height) - 1) as u32;
                let (value, rgb) = sample(sx, sy);
                let weight = (if i == 0 { 256 - fx } else { fx })
                    * (if j == 0 { 256 - fy } else { fy });
                let score = u32::from(value) * weight;
                alpha += score;
                if score > strongest {
                    strongest = score;
                    chosen = rgb;
                }
            }}
            coverage = coverage.max((alpha + 32768) / 65536);
        }}
        (coverage, chosen)
    }

    /// One B3/S23 generation, with finite dead boundaries. Color, fading and
    /// birth accents never participate in the occupancy rule.
    pub(crate) fn step(&mut self) {
        if !self.initialized { return; }
        for y in 0..self.grid_height as i32 { for x in 0..self.grid_width as i32 {
            let index = (y as u32 * self.grid_width + x as u32) as usize;
            if self.previous_hex_layout.is_some()
                && (x as u32 >= self.visible_width || y as u32 >= self.visible_height) {
                self.next[index] = Cell::default();
                continue;
            }
            let old = self.cells[index];
            let mut count = 0;
            let mut first = [0; 3];
            let mut sum = [0; 3];
            let mut same = true;
            for dy in -1..=1 { for dx in -1..=1 {
                if dx == 0 && dy == 0 { continue; }
                if let Some(neighbor) = self.cell(x + dx, y + dy) {
                    if neighbor.alive {
                        if count == 0 { first = neighbor.rgb; }
                        else if first != neighbor.rgb { same = false; }
                        sum = add_rgb(sum, neighbor.rgb);
                        count += 1;
                    }
                }
            }}
            let alive = count == 3 || (old.alive && count == 2);
            let born = alive && !old.alive;
            self.next[index] = Cell { alive, born,
                trail: if alive { 255 } else {
                    // The demo maps persistence .75 to decay .905.
                    ((u32::from(old.trail) * 905 + 500) / 1000).saturating_sub(1) as u8
                },
                rgb: if born { if same { first } else { sum } } else { old.rgb } };
        }}
        core::mem::swap(&mut self.cells, &mut self.next);
        self.generation = self.generation.saturating_add(1);
        if self.previous_hex_layout.is_some() { self.resolve_hex(); }
    }

    fn cell(&self, x: i32, y: i32) -> Option<Cell> {
        if x < 0 || y < 0 || x as u32 >= self.grid_width || y as u32 >= self.grid_height {
            None
        } else { Some(self.cells[(y as u32 * self.grid_width + x as u32) as usize]) }
    }

    fn canonical_background(&self, x: u32, y: u32) -> [u8; 3] {
        let cell = self.cell(x as i32, y as i32).unwrap_or_default();
        let strength = (if cell.alive { 0.85 } else { 0.0 })
            + f32::from(cell.trail) / 255.0 * 0.15;
        let mut halo = [0.0; 3];
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let neighbor = self.cell(x as i32 + dx, y as i32 + dy).unwrap_or_default();
            for channel in 0..3 {
                halo[channel] += f32::from(neighbor.rgb[channel]) * f32::from(neighbor.trail) / 255.0;
            }
        }
        core::array::from_fn(|channel| {
            let pigment = f32::from(cell.rgb[channel]);
            let light = pigment * strength + halo[channel] * 0.018
                + if cell.born { pigment * 0.10 } else { 0.0 };
            (light * 0.6 + 0.5).clamp(0.0, f32::from(BACKGROUND_ALPHA)) as u8
        })
    }

    fn resolve_hex(&mut self) {
        let Some([columns, _, _]) = self.previous_hex_layout else { return; };
        for y in 0..self.grid_height { for x in 0..self.grid_width {
            let index = (y * self.grid_width + x) as usize;
            self.canonical_rgba[index] = if x < self.visible_width && y < self.visible_height {
                let ink = hex_ink(&self.previous_hex, columns, 6, 11, x, y);
                composite_background(ink, self.canonical_background(x, y))
            } else { [0, 0, 0, BACKGROUND_ALPHA] };
        }}
    }

    /// Presentation is one integer nearest-neighbor lookup. No glyph mask,
    /// Conway neighborhood, floating-point shading or blending runs here.
    pub(crate) fn hex_pixel(&self, x: u32, y: u32) -> [u8; 4] {
        if x >= self.width || y >= self.height { return [0; 4]; }
        let Some([_, cell_width, cell_height]) = self.previous_hex_layout else { return [0; 4]; };
        let gx = x / cell_width * 6 + (x % cell_width) * 6 / cell_width;
        let gy = y / cell_height * 11 + (y % cell_height) * 11 / cell_height;
        self.canonical_rgba.get((gy * self.grid_width + gx) as usize).copied().unwrap_or([0; 4])
    }

    /// Premultiplied background RGB at static 60% alpha, relative to the
    /// viewport. Foreground ink is composited afterward at its original
    /// coverage; shader light never raises the background's alpha.
    pub(crate) fn pixel(&self, x: u32, y: u32) -> [u8; 3] {
        if x >= self.width || y >= self.height { return [0; 3]; }
        if let Some([_, cell_width, cell_height]) = self.previous_hex_layout {
            let gx = x / cell_width * 6 + (x % cell_width) * 6 / cell_width;
            let gy = y / cell_height * 11 + (y % cell_height) * 11 / cell_height;
            return self.canonical_background(gx, gy);
        }
        let px = (x as f32 + 0.5) * self.grid_width as f32 / self.width as f32;
        let py = (y as f32 + 0.5) * self.grid_height as f32 / self.height as f32;
        let gx = px as i32;
        let gy = py as i32;
        let local = (px - gx as f32 - 0.5).abs().max((py - gy as f32 - 0.5).abs());
        let t = ((local - 0.32) / 0.17).clamp(0.0, 1.0);
        let tile = 1.0 - t * t * (3.0 - 2.0 * t);
        let cell = self.cell(gx, gy).unwrap_or_default();
        let strength = (if cell.alive { 0.85 } else { 0.0 }
            + f32::from(cell.trail) / 255.0 * 0.15) * tile;
        let mut halo = [0.0; 3];
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            // Demo's display halo clamps at the outer edge; Conway does not.
            let neighbor = self.cell((gx + dx).clamp(0, self.grid_width as i32 - 1),
                (gy + dy).clamp(0, self.grid_height as i32 - 1)).unwrap_or_default();
            for channel in 0..3 {
                halo[channel] += f32::from(neighbor.rgb[channel]) * f32::from(neighbor.trail) / 255.0;
            }
        }
        let mut rgb = [0; 3];
        for channel in 0..3 {
            let pigment = f32::from(cell.rgb[channel]);
            let light = pigment * strength
                + halo[channel] * 0.018
                + if cell.born { pigment * tile * 0.10 } else { 0.0 };
            rgb[channel] = (light * 0.6 + 0.5).clamp(0.0, f32::from(BACKGROUND_ALPHA)) as u8;
        }
        rgb
    }
}

fn add_rgb(a: [u8; 3], b: [u8; 3]) -> [u8; 3] {
    core::array::from_fn(|i| a[i].wrapping_add(b[i]))
}

fn hex_record(cells: &[[u32; 4]], columns: u32, cell_width: u32, cell_height: u32,
    x: u32, y: u32) -> Option<([u32; 4], u8)> {
    if columns == 0 || cell_width == 0 || cell_height == 0 || x / cell_width >= columns {
        return None;
    }
    let index = (y / cell_height).checked_mul(columns)?.checked_add(x / cell_width)?;
    let cell = *cells.get(index as usize)?;
    let coverage = crate::r::services::shell3_font_service::hex::sample(
        cell[0], x % cell_width, y % cell_height, cell_width, cell_height, cell[3] & 2 != 0);
    Some((cell, coverage))
}

fn hex_input(cells: &[[u32; 4]], columns: u32, cell_width: u32, cell_height: u32,
    x: u32, y: u32) -> (u8, [u8; 3]) {
    let Some((cell, coverage)) = hex_record(cells, columns, cell_width, cell_height, x, y)
        else { return (0, [0; 3]); };
    let foreground = cell[1].to_le_bytes();
    (coverage, [foreground[0], foreground[1], foreground[2]])
}

/// The exact page ink produced by the shared bitmap face. Unstyled page
/// backgrounds are transparent; selected/inverted backgrounds stay opaque.
pub(crate) fn hex_ink(cells: &[[u32; 4]], columns: u32, cell_width: u32, cell_height: u32,
    x: u32, y: u32) -> [u8; 4] {
    let Some((cell, coverage)) = hex_record(cells, columns, cell_width, cell_height, x, y)
        else { return [0; 4]; };
    let foreground = cell[1].to_le_bytes();
    let background = if cell[3] & 1 != 0 { cell[2].to_le_bytes() } else { [0; 4] };
    let alpha = (u32::from(coverage) * u32::from(foreground[3]) + 127) / 255;
    let mut pixel = [0; 4];
    for channel in 0..3 {
        let bg = (u32::from(background[channel]) * u32::from(background[3]) + 127) / 255;
        pixel[channel] = ((u32::from(foreground[channel]) * alpha + bg * (255 - alpha) + 127) / 255) as u8;
    }
    pixel[3] = (alpha + (u32::from(background[3]) * (255 - alpha) + 127) / 255) as u8;
    pixel
}

/// The glyph producer has already applied coverage exactly once. Composite
/// that ink over the premultiplied 60%-opacity background. Opaque text and
/// overlay pixels remain opaque; uncovered background stays at alpha 153.
pub(crate) fn composite_background(foreground: [u8; 4], effect: [u8; 3]) -> [u8; 4] {
    let inverse = u32::from(255 - foreground[3]);
    let alpha = u32::from(foreground[3])
        + (u32::from(BACKGROUND_ALPHA) * inverse + 127) / 255;
    let mut pixel = [0, 0, 0, alpha as u8];
    for channel in 0..3 {
        pixel[channel] = (u32::from(foreground[channel])
            + (u32::from(effect[channel]) * inverse + 127) / 255).min(255) as u8;
    }
    pixel
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composition_retains_opaque_text_chrome_and_single_pass_antialiasing() {
        let effect = [30, 60, 120];
        assert_eq!(composite_background([255; 4], effect), [255; 4]);
        assert_eq!(composite_background([210, 100, 33, 255], effect), [210, 100, 33, 255]);
        assert_eq!(composite_background([0, 0, 0, 255], effect), [0, 0, 0, 255]);
        assert_eq!(composite_background([0; 4], effect), [30, 60, 120, 153]);
        // A half-covered white glyph is already [128,128,128,128]. Applying
        // coverage again would dim it to 64 before adding the background.
        assert_eq!(composite_background([128; 4], effect), [143, 158, 188, 204]);
    }

    #[test]
    fn background_alpha_is_fixed_and_reveals_content_below() {
        let empty = world(1, 1, &[]);
        assert_eq!(empty.pixel(1, 1), [0; 3], "unlit field must be pure black");
        assert_eq!(empty.pixel(3, 3), [0; 3]);
        let black = composite_background([0; 4], [0; 3]);
        assert_eq!(black, [0, 0, 0, 153]);
        let over_desktop = |pixel: [u8; 4], desktop: [u8; 3]| -> [u8; 3] {
            core::array::from_fn(|channel| {
                (u32::from(pixel[channel])
                    + (u32::from(desktop[channel]) * u32::from(255 - pixel[3]) + 127) / 255) as u8
            })
        };
        assert_eq!(over_desktop(black, [20, 80, 160]), [8, 32, 64]);
        let lit = composite_background([0; 4], [30, 60, 120]);
        assert_eq!(lit[3], black[3], "shader color must not add opacity");
        assert_eq!(over_desktop(lit, [20, 80, 160]), [38, 92, 184]);
        for coverage in 0..=255 {
            let pixel = composite_background([coverage; 4], [BACKGROUND_ALPHA; 3]);
            assert!(pixel[..3].iter().all(|&channel| channel <= pixel[3]));
            assert!(pixel[3] >= BACKGROUND_ALPHA);
        }
    }

    #[test]
    fn bright_shader_pixels_stay_premultiplied_at_fixed_alpha() {
        let life = world(1, 1, &[(0, 0, [255; 3])]);
        let effect = life.pixel(1, 1);
        assert!(effect.iter().all(|&channel| channel <= BACKGROUND_ALPHA));
        assert_eq!(composite_background([0; 4], effect)[3], BACKGROUND_ALPHA);
    }
    use alloc::vec;

    fn world(width: u32, height: u32, live: &[(u32, u32, [u8; 3])]) -> Life {
        let mut life = Life::new(width * 3, height * 3).unwrap();
        life.initialized = true;
        for &(x, y, rgb) in live {
            life.cells[(y * width + x) as usize] = Cell { alive: true, trail: 255, born: false, rgb };
        }
        life
    }

    fn occupied(life: &Life) -> Vec<(u32, u32)> {
        life.cells.iter().enumerate().filter(|(_, cell)| cell.alive)
            .map(|(i, _)| (i as u32 % life.grid_width, i as u32 / life.grid_width)).collect()
    }

    #[test]
    fn blinker_and_still_block_follow_b3_s23() {
        let color = [20, 40, 60];
        let mut life = world(5, 5, &[(1, 2, color), (2, 2, color), (3, 2, color)]);
        life.step();
        assert_eq!(occupied(&life), vec![(2, 1), (2, 2), (2, 3)]);
        assert_eq!(life.cells[7].rgb, color);
        life.step();
        assert_eq!(occupied(&life), vec![(1, 2), (2, 2), (3, 2)]);
        let mut block = world(4, 4, &[(1, 1, color), (2, 1, color), (1, 2, color), (2, 2, color)]);
        let original = occupied(&block);
        for _ in 0..10 { block.step(); }
        assert_eq!(occupied(&block), original);
    }

    #[test]
    fn glider_translates_one_cell_in_four_generations() {
        let color = [1, 2, 3];
        let mut life = world(7, 7, &[(2, 1, color), (3, 2, color), (1, 3, color), (2, 3, color), (3, 3, color)]);
        for _ in 0..4 { life.step(); }
        assert_eq!(occupied(&life), vec![(3, 2), (4, 3), (2, 4), (3, 4), (4, 4)]);
    }

    #[test]
    fn mixed_birth_wraps_rgb_and_black_remains_alive() {
        let mut life = world(3, 3, &[(0, 0, [255, 200, 10]), (1, 0, [1, 100, 20]), (2, 0, [0, 212, 226])]);
        life.step();
        assert!(life.cells[4].alive);
        assert_eq!(life.cells[4].rgb, [0, 0, 0]);
        assert_eq!(life.cells[1].rgb, [1, 100, 20]);
    }

    #[test]
    fn dead_boundaries_do_not_wrap_and_trails_do_not_create_life() {
        let mut life = world(4, 4, &[(0, 0, [9; 3]), (3, 0, [9; 3]), (0, 3, [9; 3])]);
        for cell in &mut life.cells { cell.trail = 255; cell.born = true; }
        life.step();
        assert!(occupied(&life).is_empty());
        assert_eq!(life.cells[4].trail, 230);
        assert_eq!(life.cells[0].rgb, [9; 3]);
    }

    #[test]
    fn snapshots_emit_positive_delta_and_scroll_reseeds() {
        let mut life = Life::new(9, 9).unwrap();
        let mut mask = vec![0; 81];
        let colors: Vec<u8> = (0..81).flat_map(|_| [250, 10, 20, 255]).collect();
        mask[4 * 9 + 4] = 255;
        let original = mask.clone();
        assert!(life.snapshot(&mask, &colors, 9, [0, 0, 9, 9], false, true));
        assert_eq!(occupied(&life), vec![(1, 1)]);
        let seeded = life.cells.clone();
        assert!(life.snapshot(&mask, &colors, 9, [0, 0, 9, 9], false, true));
        assert_eq!(life.cells, seeded);
        assert_eq!(mask, original);
        life.step();
        assert!(occupied(&life).is_empty());
        mask[4 * 9 + 7] = 255;
        assert!(life.snapshot(&mask, &colors, 9, [0, 0, 9, 9], false, true));
        assert_eq!(occupied(&life), vec![(2, 1)]);
        assert_eq!(life.generation(), 1);
        mask.fill(0);
        assert!(life.snapshot(&mask, &colors, 9, [0, 0, 9, 9], false, true));
        assert_eq!(occupied(&life), vec![(2, 1)]);
        assert!(life.snapshot(&mask, &colors, 9, [0, 0, 9, 9], true, true));
        assert!(occupied(&life).is_empty());
        assert_eq!(life.generation(), 0);
    }

    #[test]
    fn no_emit_updates_baseline_and_invalid_input_leaves_it_untouched() {
        let mut life = Life::new(3, 3).unwrap();
        let colors = vec![255; 36];
        assert!(life.snapshot(&[0; 9], &colors, 3, [0, 0, 3, 3], false, false));
        assert!(life.snapshot(&[255; 9], &colors, 3, [0, 0, 3, 3], false, false));
        assert!(life.snapshot(&[255; 9], &colors, 3, [0, 0, 3, 3], false, true));
        assert!(occupied(&life).is_empty());
        let previous = life.previous.clone();
        assert!(!life.snapshot(&[0; 8], &colors, 3, [0, 0, 3, 3], true, true));
        assert_eq!(life.previous, previous);
    }

    fn hex_cell(character: char, foreground: [u8; 4], background: [u8; 4], flags: u32) -> [u32; 4] {
        let (glyph, foreground) = crate::r::services::shell3_font_service::hex::resolve(character, foreground);
        [glyph, u32::from_le_bytes(foreground), u32::from_le_bytes(background), flags]
    }

    #[test]
    fn canonical_seed_and_generations_are_identical_at_every_physical_font_size() {
        let cells = vec![
            hex_cell('A', [250, 10, 20, 255], [0, 0, 0, 255], 0),
            hex_cell('g', [20, 240, 40, 255], [0, 0, 0, 255], 0),
            hex_cell(' ', [0, 0, 250, 255], [44, 44, 44, 255], 1),
            hex_cell('\u{10ffff}', [77, 88, 99, 255], [0, 0, 0, 255], 2),
        ];
        let mut reference = Life::new(12, 22).unwrap();
        assert!(reference.snapshot_hex(&cells, 2, 6, 11, false, true));
        assert_eq!((reference.grid_width, reference.grid_height), (12, 22));
        for y in 0..22 { for x in 0..12 {
            let (coverage, rgb) = hex_input(&cells, 2, 6, 11, x, y);
            let actual = reference.cells[(y * 12 + x) as usize];
            assert_eq!(actual.alive, coverage != 0, "direct bitmap bit at {x},{y}");
            assert_eq!(actual.rgb, if coverage != 0 { rgb } else { [0; 3] });
        }}
        for (cell_width, cell_height) in [(6, 11), (7, 13), (17, 31), (23, 42)] {
            let mut life = Life::new(cell_width * 2, cell_height * 2).unwrap();
            assert!(life.snapshot_hex(&cells, 2, cell_width, cell_height, false, true));
            assert_eq!(life.cells, reference.cells);
            assert_eq!(life.canonical_rgba, reference.canonical_rgba);
            let mut expected = Life::new(12, 22).unwrap();
            assert!(expected.snapshot_hex(&cells, 2, 6, 11, false, true));
            for _ in 0..3 {
                for y in 0..life.height { for x in 0..life.width {
                    let gx = x / cell_width * 6 + (x % cell_width) * 6 / cell_width;
                    let gy = y / cell_height * 11 + (y % cell_height) * 11 / cell_height;
                    assert_eq!(life.hex_pixel(x, y), expected.canonical_rgba[(gy * 12 + gx) as usize]);
                }}
                life.step();
                expected.step();
                assert_eq!(life.cells, expected.cells);
                assert_eq!(life.canonical_rgba, expected.canonical_rgba);
            }
        }
    }

    #[test]
    fn canonical_emission_is_positive_bitmap_delta_and_no_emit_refreshes_baseline() {
        let blank = [hex_cell(' ', [255; 4], [0, 0, 0, 255], 0)];
        let ink = [hex_cell('A', [20, 40, 60, 255], [0, 0, 0, 255], 0)];
        let full = [hex_cell('█', [255; 4], [0, 0, 0, 255], 0)];
        let mut life = Life::new(17, 31).unwrap();
        assert!(life.snapshot_hex(&blank, 1, 17, 31, false, true));
        assert!(occupied(&life).is_empty());
        assert!(life.snapshot_hex(&ink, 1, 17, 31, false, true));
        let seeded = life.cells.clone();
        assert!(seeded.iter().any(|cell| cell.alive));
        assert!(life.snapshot_hex(&ink, 1, 17, 31, false, true));
        assert_eq!(life.cells, seeded, "unchanged ink must not add color or extra life");
        life.step();
        let advanced = life.cells.clone();
        assert!(life.snapshot_hex(&full, 1, 17, 31, false, false));
        assert_eq!(life.cells, advanced);
        assert!(life.snapshot_hex(&full, 1, 17, 31, false, true));
        assert_eq!(life.cells, advanced, "no-emit still advanced the positive-delta baseline");
        assert!(life.snapshot_hex(&blank, 1, 17, 31, false, true));
        assert_eq!(life.cells, advanced, "erasing ink never kills a running world");
        assert!(life.snapshot_hex(&blank, 1, 17, 31, true, true));
        assert!(occupied(&life).is_empty());
        assert_eq!(life.generation(), 0);
    }

    #[test]
    fn clipped_glyph_tail_is_dead_and_cannot_evolve_into_visible_page() {
        let cells = [hex_cell('█', [255; 4], [0, 0, 0, 255], 0)];
        let mut life = Life::new(2, 2).unwrap();
        assert!(life.snapshot_hex(&cells, 1, 7, 13, false, true));
        assert_eq!((life.grid_width, life.grid_height), (6, 11));
        assert_eq!((life.visible_width, life.visible_height), (1, 1));
        assert_eq!(occupied(&life), vec![(0, 0)]);
        for _ in 0..4 {
            life.step();
            assert!(occupied(&life).is_empty());
            assert!(life.cells.iter().all(|cell| !cell.alive && !cell.born));
        }
        for (index, pixel) in life.canonical_rgba.iter().enumerate().skip(1) {
            assert_eq!(*pixel, [0, 0, 0, BACKGROUND_ALPHA], "hidden canonical pixel {index}");
        }
    }

    #[test]
    fn canonical_pixel_budget_rejects_before_changing_retained_world() {
        let mut life = Life::new(1800, 1500).unwrap();
        let normal = vec![hex_cell('A', [255; 4], [0, 0, 0, 255], 0); 8 * 6];
        assert!(life.snapshot_hex(&normal, 8, 256, 256, false, true));
        let retained = life.cells.clone();
        let too_many = vec![normal[0]; 300 * 137];
        assert!(!life.snapshot_hex(&too_many, 300, 6, 11, true, true));
        assert_eq!(life.cells, retained);
    }

    #[test]
    fn physical_2560_width_uses_exact_17_pixel_cells_without_cumulative_drift() {
        let cells = vec![hex_cell('A', [255; 4], [0, 0, 0, 255], 0); 151];
        let mut life = Life::new(2560, 31).unwrap();
        assert!(life.snapshot_hex(&cells, 151, 17, 31, false, true));
        assert_eq!(life.grid_width, 906);
        for y in 0..31 { for x in 0..2560 {
            let gx = x / 17 * 6 + (x % 17) * 6 / 17;
            let gy = y * 11 / 31;
            assert_eq!(life.hex_pixel(x, y), life.canonical_rgba[(gy * 906 + gx) as usize]);
        }}
        assert_eq!(life.visible_width, 904);
        assert_eq!((2559 / 17) * 6 + (2559 % 17) * 6 / 17, 903);
        assert_ne!(2559 * 906 / 2560, 903, "whole-viewport scaling would drift away from the text cells");
    }

    #[test]
    fn compact_hex_validates_before_changing_world_or_comparison_baseline() {
        let mut life = Life::new(12, 22).unwrap();
        let cells = [hex_cell('A', [255; 4], [0, 0, 0, 255], 0); 4];
        assert!(life.snapshot_hex(&cells, 2, 6, 11, false, true));
        life.step();
        let before = life.cells.clone();
        let previous = life.previous_hex.clone();
        for (input, columns, width, height) in [
            (&cells[..3], 2, 6, 11), (&cells[..], 0, 6, 11),
            (&cells[..], 2, 0, 11), (&cells[..], 2, 6, 0),
            (&cells[..], 3, 6, 11), (&cells[..], 3, 5, 11),
            (&cells[..], 2, 6, 10),
        ] {
            assert!(!life.snapshot_hex(input, columns, width, height, true, true));
            assert_eq!(life.cells, before);
            assert_eq!(life.previous_hex, previous);
            assert_eq!(life.generation(), 1);
        }
        let colors = vec![255; 12 * 22 * 4];
        assert!(life.snapshot(&vec![0; 12 * 22], &colors, 12, [0, 0, 12, 22], false, true));
        assert_eq!(life.previous.len(), 12 * 22);
        assert!(life.previous_hex.is_empty());
        assert!(life.snapshot_hex(&cells, 2, 6, 11, false, true));
        assert!(life.previous.is_empty(), "return to compact input releases legacy pixel baseline");
    }

    #[test]
    fn hex_ink_preserves_exact_mask_alpha_and_opaque_styled_backgrounds() {
        let background = [40, 90, 150, 255];
        for alpha in 0..=255u8 {
            let foreground = [17, 129, 251, alpha];
            for flags in 0..4 {
                let cells = [hex_cell('A', foreground, background, flags)];
                for y in 0..11 { for x in 0..6 {
                    let coverage = crate::r::services::shell3_font_service::hex::sample(
                        cells[0][0], x, y, 6, 11, flags & 2 != 0);
                    let a = (u32::from(coverage) * u32::from(alpha) + 127) / 255;
                    let bg = if flags & 1 != 0 { background } else { [0; 4] };
                    let mut expected = [0; 4];
                    for channel in 0..3 {
                        expected[channel] = ((u32::from(foreground[channel]) * a
                            + u32::from(bg[channel]) * (255 - a) + 127) / 255) as u8;
                    }
                    expected[3] = (a + (u32::from(bg[3]) * (255 - a) + 127) / 255) as u8;
                    assert_eq!(hex_ink(&cells, 1, 6, 11, x, y), expected);
                }}
            }
        }
        assert_eq!(hex_ink(&[], 0, 0, 0, 0, 0), [0; 4]);
    }
}
