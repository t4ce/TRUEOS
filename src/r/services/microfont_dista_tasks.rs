//! Seed/task upload only: positions, link acceptance and curves belong to GLSL.
use alloc::vec::Vec;
use microfont::FontShape;

pub(super) const TEXT: &str = "ABC123/Yy%";
pub(super) const GLYPH_WIDTH: u32 = 96;
pub(super) const GLYPH_HEIGHT: u32 = 176;
pub(super) const WIDTH: u32 = TEXT.len() as u32 * GLYPH_WIDTH;
pub(super) const HEIGHT: u32 = 2 * GLYPH_HEIGHT;

/// Maximum circle chord error at the demo's 16 pixels per source unit.
#[derive(Clone, Copy, Debug)]
pub(super) enum Quality {
    Draft,
    Fine,
    High,
}

impl Quality {
    pub(super) const fn segments(self, shape: FontShape) -> usize {
        match (self, shape) {
            (Self::Draft, FontShape::Meso) => 12,
            (Self::Draft, FontShape::Lean) => 8,
            (Self::Fine, FontShape::Meso) => 16,
            (Self::Fine, FontShape::Lean) => 12,
            (Self::High, FontShape::Meso) => 24,
            (Self::High, FontShape::Lean) => 16,
        }
    }

    pub(super) const fn error_micropixels(self) -> u32 {
        match self { Self::Draft => 250_000, Self::Fine => 125_000, Self::High => 62_500 }
    }

    pub(super) const fn cpu_detail(self) -> microfont::FontDetail {
        match self {
            Self::Draft | Self::Fine => microfont::FontDetail::Fine,
            Self::High => microfont::FontDetail::High,
        }
    }
}

pub(super) struct Tasks {
    /// Float3 ABI: vertex token, packed codepoint/column/row, circle segments.
    pub(super) descriptors: Vec<[f32; 3]>,
    pub(super) indices: Vec<u32>,
    pub(super) row_candidates: [usize; 2],
}

pub(super) fn build(quality: Quality) -> Tasks {
    let mut tasks = Tasks { descriptors: Vec::new(), indices: Vec::new(), row_candidates: [0; 2] };
    for (row, shape) in [FontShape::Meso, FontShape::Lean].into_iter().enumerate() {
        for (column, ch) in TEXT.chars().enumerate() {
            let stamp = ch as u32 | (column as u32) << 7 | (row as u32) << 11;
            append_glyph(&mut tasks, ch, stamp, quality.segments(shape), row);
        }
    }
    tasks
}

#[cfg(feature = "microfont_dista_bench")]
pub(super) const ATLAS_WIDTH: u32 = 16 * GLYPH_WIDTH;
#[cfg(feature = "microfont_dista_bench")]
pub(super) const ATLAS_HEIGHT: u32 = 6 * GLYPH_HEIGHT;

/// One complete printable Basic Latin sheet, including a blank space slot.
/// Shapes use separate buffers so each stays inside the render target limits.
#[cfg(feature = "microfont_dista_bench")]
pub(super) fn build_atlas(quality: Quality, shape: FontShape, direct_checks: bool) -> Tasks {
    let mut tasks = Tasks { descriptors: Vec::new(), indices: Vec::new(), row_candidates: [0; 2] };
    let row = usize::from(shape == FontShape::Lean);
    for code in 0x20u32..=0x7e {
        let slot = code - 0x20;
        let stamp = code | (slot % 16) << 7 | (row as u32) << 11
            | (slot / 16) << 12 | 1 << 15 | u32::from(direct_checks) << 16;
        append_glyph(&mut tasks, char::from_u32(code).unwrap(), stamp, quality.segments(shape), row);
    }
    tasks
}

fn append_glyph(tasks: &mut Tasks, ch: char, stamp: u32, segments: usize, row: usize) {
    let seed = microfont::raster::basic_seed(ch).expect("Basic Latin seed");
    let offsets = if seed.long_reach { 10 } else { 4 };
    for site in 0..66 {
        if seed.pixels & (1u128 << (65 - site)) == 0 { continue; }
        let base = tasks.descriptors.len() as u32;
        for corner in 0..=segments {
            tasks.descriptors.push([(site * (segments + 1) + corner) as f32, stamp as f32, segments as f32]);
        }
        for ring in 0..segments {
            tasks.indices.extend_from_slice(&[
                base, base + 1 + ((ring + 1) % segments) as u32, base + 1 + ring as u32,
            ]);
        }
        for offset in 0..offsets {
            let base = tasks.descriptors.len() as u32;
            let token = 66 * (segments + 1) + (site * 10 + offset) * 4;
            for corner in 0..4 {
                tasks.descriptors.push([(token + corner) as f32, stamp as f32, segments as f32]);
            }
            tasks.indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
        }
        tasks.row_candidates[row] += segments + offsets * 2;
    }
}
