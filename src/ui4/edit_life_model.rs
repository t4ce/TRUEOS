//! Bounded reference implementation of Edit's glyph-driven Conway field.
//! Input coverage and straight glyph colors remain immutable. Only this
//! task-owned world and its previous-input comparison image are changed.

use alloc::vec::Vec;

const MAX_SIDE: u32 = 512;
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
    initialized: bool,
    generation: u64,
}

impl Life {
    pub(crate) fn new(width: u32, height: u32) -> Option<Self> {
        if width == 0 || height == 0 { return None; }
        let grid_width = width.div_ceil(3).min(MAX_SIDE);
        let grid_height = height.div_ceil(3).min(MAX_SIDE);
        let count = (grid_width as usize).checked_mul(grid_height as usize)?;
        let pixels = (width as usize).checked_mul(height as usize)?;
        let mut cells = Vec::new();
        let mut next = Vec::new();
        let mut previous = Vec::new();
        cells.try_reserve_exact(count).ok()?;
        next.try_reserve_exact(count).ok()?;
        previous.try_reserve_exact(pixels).ok()?;
        cells.resize(count, Cell::default());
        next.resize(count, Cell::default());
        previous.resize(pixels, 0);
        Some(Self { width, height, grid_width, grid_height, cells, next,
            previous, initialized: false, generation: 0 })
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
        let reseed = reset || !self.initialized;
        if reseed || emit {
            for y in 0..self.grid_height {
                for x in 0..self.grid_width {
                    let (coverage, rgb) = self.input_cell(mask, colors, page_width, left, top, x, y, !reseed);
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
        self.initialized = true;
        true
    }

    fn input_cell(&self, mask: &[u8], colors: &[u8], page_width: u32,
        left: u32, top: u32, x: u32, y: u32, delta: bool) -> (u32, [u8; 3]) {
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
                let sx = (bx + i).clamp(0, i64::from(self.width) - 1) as usize;
                let sy = (by + j).clamp(0, i64::from(self.height) - 1) as usize;
                let at = (top as usize + sy) * page_width as usize + left as usize + sx;
                let input = mask[at];
                let value = if delta {
                    input.saturating_sub(self.previous[sy * self.width as usize + sx])
                } else { input };
                let weight = (if i == 0 { 256 - fx } else { fx })
                    * (if j == 0 { 256 - fy } else { fy });
                let score = u32::from(value) * weight;
                alpha += score;
                if score > strongest {
                    strongest = score;
                    chosen.copy_from_slice(&colors[at * 4..at * 4 + 3]);
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
    }

    fn cell(&self, x: i32, y: i32) -> Option<Cell> {
        if x < 0 || y < 0 || x as u32 >= self.grid_width || y as u32 >= self.grid_height {
            None
        } else { Some(self.cells[(y as u32 * self.grid_width + x as u32) as usize]) }
    }

    /// Opaque background RGB relative to the viewport. The fixed 50% dark
    /// scrim affects the effect only; the caller composites foreground ink
    /// afterward with its original alpha coverage.
    pub(crate) fn pixel(&self, x: u32, y: u32) -> [u8; 3] {
        if x >= self.width || y >= self.height { return [2, 3, 5]; }
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
        let base = [4.59, 6.12, 9.945];
        let mut rgb = [0; 3];
        for channel in 0..3 {
            let pigment = f32::from(cell.rgb[channel]);
            let light = base[channel] * (1.0 - strength) + pigment * strength
                + halo[channel] * 0.018
                + if cell.born { pigment * tile * 0.10 } else { 0.0 };
            rgb[channel] = (light * 0.5 + 0.5).clamp(0.0, 255.0) as u8;
        }
        rgb
    }
}

fn add_rgb(a: [u8; 3], b: [u8; 3]) -> [u8; 3] {
    core::array::from_fn(|i| a[i].wrapping_add(b[i]))
}

/// The glyph producer has already applied coverage exactly once. Composite
/// its premultiplied ink over the effect, then publish a fully opaque pixel.
pub(crate) fn composite_opaque(foreground: [u8; 4], effect: [u8; 3]) -> [u8; 4] {
    let inverse = u32::from(255 - foreground[3]);
    let mut pixel = [0, 0, 0, 255];
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
        let effect = [60, 120, 240];
        assert_eq!(composite_opaque([210, 100, 33, 255], effect), [210, 100, 33, 255]);
        assert_eq!(composite_opaque([0, 0, 0, 255], effect), [0, 0, 0, 255]);
        assert_eq!(composite_opaque([0; 4], effect), [60, 120, 240, 255]);
        // A half-covered white glyph is already [128,128,128,128]. Applying
        // coverage again would dim it to 64 before adding the background.
        assert_eq!(composite_opaque([128; 4], effect), [158, 188, 248, 255]);
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
}
