//! Native line/quad geometry and one reusable, color-free R8 atlas. Intel
//! retains its boot-lifetime geometry; virgl consumes CPU-decoded raster seeds
//! as native line/point/quad primitives, with no glyph tessellation.
//! Consumers execute while the atlas is locked; no asynchronous mask reader may
//! survive the callback. Width/size invalidate pixels, never resident geometry.
use crate::{
    intel::{
        gpgpu::GpgpuMask8Surface,
        render::{self, ResidentScenePrimitiveTopology as Topology},
    },
    ui4,
};
use alloc::vec::Vec;

pub(crate) const PREVIEW_HEADER_HEIGHT: u32 = 30;
pub(crate) const ATLAS_COLUMNS: u32 = 16;
pub(crate) const LATIN_SLOTS: usize = 95;
pub(crate) const BOX_START: usize = LATIN_SLOTS;
pub(crate) const BLOCK_START: usize = BOX_START + 128;
const EXTRA_START: usize = BLOCK_START + 32;
const fn is_extra_line(ch: char) -> bool {
    !(' ' <= ch && ch <= '~') && !('\u{2500}' <= ch && ch <= '\u{259f}')
}
const fn extra_count() -> usize {
    let mut count = 0;
    let mut i = 0;
    while i < microfont::raster_line::GLYPH_CHARACTERS.len() {
        if is_extra_line(microfont::raster_line::GLYPH_CHARACTERS[i]) { count += 1; }
        i += 1;
    }
    count
}
pub(crate) const BRAILLE_START: usize = EXTRA_START + extra_count();
pub(crate) const SLOT_COUNT: usize = BRAILLE_START + 256;
pub(crate) const ATLAS_ROWS: u32 = SLOT_COUNT.div_ceil(ATLAS_COLUMNS as usize) as u32;
// Only line slots use packed joins; Braille quads follow them in the atlas.
// Join recipes use seven bits of X and eight bits of Y. Reject overflow
// explicitly rather than silently changing the shape of a future glyph.
const _: () = assert!(ATLAS_COLUMNS * 6 <= 128 && (BRAILLE_START as u32).div_ceil(ATLAS_COLUMNS) * 11 <= 256);

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct AtlasConfig {
    /// Uniform source-cell scale; the default fits the atlas within 4096 pixels.
    pub(crate) glyph_scale: u32,
    /// Optional native-pixel height of the eleven-row em cell.
    pub(crate) glyph_height_px: Option<u32>,
    /// Native line width in pixels, rounded once to the engine's U11.7 step.
    pub(crate) line_width_px: f32,
}
impl Default for AtlasConfig {
    fn default() -> Self {
        Self {
            // Keep a multiple of four for the 4x quad specimen.
            glyph_scale: (16.min(4096 / (ATLAS_ROWS * 11)) / 4 * 4).max(4),
            glyph_height_px: None,
            line_width_px: 8.0,
        }
    }
}
impl AtlasConfig {
    fn normalized(self) -> Result<Self, &'static str> {
        if !(1..=23).contains(&self.glyph_scale)
            || !self.line_width_px.is_finite()
            || self.line_width_px < 0.25
            || self.line_width_px > 32.0
            || (self.glyph_height_px.is_none() && self.line_width_px > self.glyph_scale as f32)
            || self.glyph_height_px.is_some_and(|h| !(11..=253).contains(&h))
            || self.extent().0 > 4096 || self.extent().1 > 4096
        {
            return Err("microfont-atlas-config");
        }
        Ok(Self {
            line_width_px: libm::roundf(self.line_width_px * 128.0) / 128.0,
            ..self
        })
    }
    fn scale(self) -> f32 {
        self.glyph_height_px.map_or(self.glyph_scale as f32, |h| h as f32 / 11.0)
    }
    fn gutter(self) -> u32 {
        if self.glyph_height_px.is_some() { (libm::ceilf(self.line_width_px * 0.5) as u32 + 1).max(6) }
        else { 0 }
    }
    pub(crate) fn cell_extent(self) -> (u32, u32) {
        let height = self.glyph_height_px.unwrap_or(11 * self.glyph_scale);
        let gutter = 2 * self.gutter();
        (((height * 6 + 10) / 11).max(1) + gutter, height + gutter)
    }
    pub(crate) fn extent(self) -> (u32, u32) {
        let (width, height) = self.cell_extent();
        (ATLAS_COLUMNS * width, ATLAS_ROWS * height)
    }
}

pub(crate) fn glyph_slot(ch: char) -> Option<usize> {
    match ch {
        ' '..='~' => Some(ch as usize - 0x20),
        '\u{2500}'..='\u{257f}' => Some(BOX_START + ch as usize - 0x2500),
        '\u{2580}'..='\u{259f}' => Some(BLOCK_START + ch as usize - 0x2580),
        '\u{2800}'..='\u{28ff}' => Some(BRAILLE_START + ch as usize - 0x2800),
        _ => microfont::raster_line::GLYPH_CHARACTERS.iter()
            .copied().filter(|&candidate| is_extra_line(candidate))
            .position(|candidate| candidate == ch).map(|index| EXTRA_START + index),
    }
}

struct ResidentPrimitive {
    mesh: render::ResidentTriangleMesh,
    topology: Topology,
    params: [u32; 16],
}
struct ResidentGlyphs {
    batches: Vec<ResidentPrimitive>,
    bytes: usize,
    constants: Option<render::ResidentRenderBuffer>,
}
impl Drop for ResidentGlyphs {
    fn drop(&mut self) {
        if let Some(constants) = self.constants.take() {
            let _ = render::release_resident_render_buffer(&constants);
        }
        for batch in &self.batches {
            let _ = render::release_resident_triangle_mesh(&batch.mesh);
        }
    }
}
fn cell_origin(slot: usize) -> [f32; 2] {
    [
        (slot % ATLAS_COLUMNS as usize) as f32 * 6.0,
        (slot / ATLAS_COLUMNS as usize) as f32 * 11.0,
    ]
}
fn upload(
    batches: &mut ResidentGlyphs,
    vertices: &[[f32; 3]],
    indices: &[u32],
    topology: Topology,
    params: [u32; 16],
) -> Result<(), &'static str> {
    if indices.is_empty() {
        return Ok(());
    }
    let mesh = render::create_resident_indexed_mesh(vertices, indices, topology)?;
    batches.bytes += vertices.len() * 12 + indices.len() * 4;
    batches.batches.push(ResidentPrimitive {
        mesh,
        topology,
        params,
    });
    Ok(())
}
fn append_quad(
    vertices: &mut Vec<[f32; 3]>,
    indices: &mut Vec<u32>,
    slot: usize,
    corners: [[f32; 2]; 4],
) {
    let at = cell_origin(slot);
    let base = vertices.len() as u32;
    // Quad edges may lie exactly at x=6/y=11. Carry the owning cell explicitly
    // so physical gutters do not turn that boundary into the next cell.
    vertices.extend(corners.map(|p| [at[0] + p[0], at[1] + p[1], -(slot as f32 + 1.0)]));
    indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
}
fn endpoint_params(strip: &microfont::raster_line::Strip, slot: usize) -> [u32; 16] {
    use microfont::raster_line::Boundary::End;
    let mut p = [0.0f32; 16];
    let origin = cell_origin(slot);
    for (offset, index, start) in [(0, 0, true), (4, strip.points.len() - 1, false)] {
        let at = strip.point(index).unwrap();
        let neighbor = if start {
            (1..strip.points.len()).find_map(|i| strip.point(i).filter(|v| *v != at))
        } else {
            (0..index)
                .rev()
                .find_map(|i| strip.point(i).filter(|v| *v != at))
        };
        if let Some(next) = neighbor {
            let d = [
                next.x() as f32 - at.x() as f32,
                next.y() as f32 - at.y() as f32,
            ];
            let len = libm::sqrtf(d[0] * d[0] + d[1] * d[1]);
            p[offset..offset + 4].copy_from_slice(&[
                origin[0] + at.x() as f32 + 0.5,
                origin[1] + at.y() as f32 + 0.5,
                d[0] / len,
                d[1] / len,
            ]);
            p[if start { 10 } else { 11 }] = if (if start { strip.start } else { strip.end }) == End
            {
                1.0
            } else {
                0.0
            };
        }
    }
    p[8] = 1.0; // line mode; replaced by half-width on each dispatch
    p.map(f32::to_bits)
}

/// Join recipe: one exact packed centre and three offsets per corner. The VS
/// expands offsets by the current pixel width and exports the edge distance.
/// A bevel patch uses the shared centre and the exposed endpoint of each
/// perpendicular half-width edge. Its winding is positive after the Y flip.
/// This diagnostic computes join patches; the stroke bodies remain native.
pub(super) fn corner_triangle(
    previous: [f32; 2],
    at: [f32; 2],
    next: [f32; 2],
    width: f32,
) -> Option<[[f32; 2]; 3]> {
    let mut incoming = [at[0] - previous[0], at[1] - previous[1]];
    let mut outgoing = [next[0] - at[0], next[1] - at[1]];
    let a = libm::sqrtf(incoming[0] * incoming[0] + incoming[1] * incoming[1]);
    let b = libm::sqrtf(outgoing[0] * outgoing[0] + outgoing[1] * outgoing[1]);
    if !width.is_finite() || width <= 0.0 || a <= 1e-6 || b <= 1e-6 {
        return None;
    }
    for k in 0..2 {
        incoming[k] /= a;
        outgoing[k] /= b;
    }
    let turn = incoming[0] * outgoing[1] - incoming[1] * outgoing[0];
    if turn.abs() <= 1e-6 {
        return None;
    }
    let side = if turn > 0.0 { 1.0 } else { -1.0 };
    let half = width * 0.5 * side;
    let mut triangle = [
        at,
        [at[0] + incoming[1] * half, at[1] - incoming[0] * half],
        [at[0] + outgoing[1] * half, at[1] - outgoing[0] * half],
    ];
    let cross = (triangle[1][0] - at[0]) * (triangle[2][1] - at[1])
        - (triangle[1][1] - at[1]) * (triangle[2][0] - at[0]);
    if cross > 0.0 {
        triangle.swap(1, 2);
    }
    Some(triangle)
}

fn append_join(out: &mut Vec<[f32; 3]>, slot: usize, triple: &[microfont::raster_line::Point]) {
    let points = core::array::from_fn::<_, 3, _>(|i| {
        [triple[i].x() as f32 + 0.5, triple[i].y() as f32 + 0.5]
    });
    let Some(triangle) = corner_triangle(points[0], points[1], points[2], 1.0) else {
        return;
    };
    let at = cell_origin(slot);
    let x = at[0] as u32 + triple[1].x() as u32;
    let y = at[1] as u32 + triple[1].y() as u32;
    let code = (x | (y << 7)) as f32;
    let a = [
        triangle[1][0] - triangle[0][0],
        triangle[1][1] - triangle[0][1],
    ];
    let b = [
        triangle[2][0] - triangle[0][0],
        triangle[2][1] - triangle[0][1],
    ];
    // Reflex exterior >270 degrees means the two outward normals are
    // more than 90 degrees apart. Only these sharp tips get a circular join.
    if a[0] * b[0] + a[1] * b[1] < -1e-6 {
        let length = libm::sqrtf((a[0]+b[0])*(a[0]+b[0]) + (a[1]+b[1])*(a[1]+b[1]));
        let middle = [(a[0]+b[0])*0.5/length, (a[1]+b[1])*0.5/length];
        let round_code = (code as u32 | 0x8000) as f32;
        // Two bounded tangent envelopes cover the circular sector. Four
        // triangles share one centre; the PS supplies the exact circular edge.
        for (left, right) in [(a, middle), (middle, b)] {
            let divisor = 1.0 + 4.0*(left[0]*right[0]+left[1]*right[1]);
            let tangent = [(left[0]+right[0])/divisor, (left[1]+right[1])/divisor];
            for (one, two) in [(left, tangent), (tangent, right)] {
                out.extend_from_slice(&[
                    [round_code, 0.0, 0.0],
                    [round_code, one[0], one[1]],
                    [round_code, two[0], two[1]],
                ]);
            }
        }
        return;
    }
    let edge = [b[0] - a[0], b[1] - a[1]];
    let height =
        (a[0] * b[1] - a[1] * b[0]).abs() / libm::sqrtf(edge[0] * edge[0] + edge[1] * edge[1]);
    out.extend_from_slice(&[
        [code, -1.0 - height, 0.0],
        [code, a[0], a[1]],
        [code, b[0], b[1]],
    ]);
}
pub(super) fn append_strip_joins(out: &mut Vec<[f32; 3]>, slot: usize, strip: &microfont::raster_line::Strip) {
    use microfont::raster_line::Boundary::Junction;
    let points: Vec<_> = strip.iter_points().collect();
    for triple in points.windows(3) {
        append_join(out, slot, triple);
    }
    // The repeated point already supplies the closing line segment. Only the
    // join spanning the end/start boundary is absent from windows(3).
    if points.len() >= 4 && points.first() == points.last()
        && strip.start == Junction && strip.end == Junction
    {
        append_join(out, slot, &[points[points.len()-2], points[0], points[1]]);
    }
}

fn upload_glyphs() -> Result<ResidentGlyphs, &'static str> {
    let mut resident = ResidentGlyphs {
        batches: Vec::new(),
        bytes: 0,
        constants: None,
    };
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for (i, &(ch, _)) in microfont::BOX_DRAW_SYMBOLS.iter().enumerate() {
        if let Some(quads) = microfont::raster_quad::boxdraw_quad_slice(ch) {
            for quad in quads {
                append_quad(&mut vertices, &mut indices, BOX_START + i, quad.vertices);
            }
        }
    }
    for (i, &(ch, _)) in microfont::BLOCK_SYMBOLS.iter().enumerate() {
        if let Some(quads) = microfont::raster_quad::block_quad_slice(ch) {
            for quad in quads {
                let [l, t] = quad.top_left.map(f32::from);
                let [r, b] = quad.bottom_right.map(f32::from);
                append_quad(
                    &mut vertices,
                    &mut indices,
                    BLOCK_START + i,
                    [[l, t], [r, t], [r, b], [l, b]],
                );
            }
        }
    }
    for code in 0x2800..=0x28ff {
        let ch = char::from_u32(code).unwrap();
        for quad in microfont::raster_quad::braille_quad_slice(ch).unwrap() {
            let [l, t] = quad.top_left.map(f32::from);
            let [r, b] = quad.bottom_right.map(f32::from);
            append_quad(&mut vertices, &mut indices, BRAILLE_START + (code - 0x2800) as usize,
                [[l, t], [r, t], [r, b], [l, b]]);
        }
    }
    upload(&mut resident, &vertices, &indices, Topology::TriangleList, [0; 16])?;
    for ch in (' '..='~').chain(microfont::raster_line::GLYPH_CHARACTERS.iter()
        .copied().filter(|&ch| is_extra_line(ch))) {
        upload_line_glyph(&mut resident, ch,
            glyph_slot(ch).ok_or("microfont-glyph-slot")?)?;
    }
    finish_upload(&mut resident)?;
    crate::log_important!(target:"microfont-mask";
        "microfont-mask: geometry-ready uploads=1 draws={} geometry_bytes={} metadata_bytes={} slots={} cpu_geometry_rebuilds=0 storage=none\n",
        resident.batches.len(),resident.bytes,resident.batches.len()*64,SLOT_COUNT);
    Ok(resident)
}

fn finish_upload(resident: &mut ResidentGlyphs) -> Result<(), &'static str> {
    resident.constants =
        Some(render::allocate_resident_render_buffer(resident.batches.len() * 64)?);
    let constants = resident.constants.as_ref().unwrap();
    for (i, batch) in resident.batches.iter().enumerate() {
        for (k, word) in batch.params.iter().enumerate() {
            if !constants.write(i * 64 + k * 4, &word.to_le_bytes()) {
                return Err("microfont-constants-upload");
            }
        }
    }
    constants.flush();
    Ok(())
}

fn upload_line_glyph(resident: &mut ResidentGlyphs, ch: char, slot: usize) -> Result<(), &'static str> {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let Some(strips) = microfont::raster_line::glyph_strips(ch) else {
        return Ok(());
    };
    let at = cell_origin(slot);
    for strip in strips {
        vertices.clear();
        indices.clear();
        for i in 0..strip.points.len() + 2 {
            let [x, y, z] = strip.adjacency_vertex(i).ok_or("microfont-adjacency")?;
            // Z is a resident VS recipe marker, not a seed-data change.
            // Identify real endpoints by index, including coincident points.
            let z = if i == 1 { 0.25 } else if i == strip.points.len() { 0.75 } else { z };
            vertices.push([at[0] + x + 0.5, at[1] + y + 0.5, z]);
        }
        indices.extend(0..vertices.len() as u32);
        upload(
            resident,
            &vertices,
            &indices,
            Topology::LineStripAdj,
            endpoint_params(strip, slot),
        )?;
    }
    vertices.clear();
    indices.clear();
    for strip in strips {
        append_strip_joins(&mut vertices, slot, strip);
    }
    indices.extend(0..vertices.len() as u32);
    let mut params = [0; 16];
    params[8] = (-1.0f32).to_bits();
    upload(resident, &vertices, &indices, Topology::TriangleList, params)?;
    Ok(())
}

/// Decode the same raster-line and raster-quad seeds for the emulator. CPU
/// work is limited to coordinate transforms and semantic endpoint/join data;
/// lines remain native GPU lines and joins remain native points.
fn native_mask_geometry(
    config: AtlasConfig, width: u32, height: u32, single: Option<char>,
) -> Result<crate::virtio_gpu_logo::NativeMaskGeometry, &'static str> {
    use crate::virtio_gpu_logo::{NativeMaskGeometry, NativeMaskVertex};
    use microfont::raster_line::{Boundary, Point};
    let mut geometry = NativeMaskGeometry { lines: Vec::new(), points: Vec::new(),
        quads: Vec::new(), line_width_px: config.line_width_px };
    let radius = config.line_width_px * 0.5;
    let (cell_width, cell_height) = config.cell_extent();
    let pixel = |slot: usize, at: [f32; 2]| -> [f32; 2] {
        [(slot % ATLAS_COLUMNS as usize) as f32 * cell_width as f32
            + at[0] * config.scale() + config.gutter() as f32,
         (slot / ATLAS_COLUMNS as usize) as f32 * cell_height as f32
            + at[1] * config.scale() + config.gutter() as f32]
    };
    let vertex = |at: [f32; 2], data: [f32; 4], style: [f32; 4]| NativeMaskVertex {
        position: [at[0] * 2.0 / width as f32 - 1.0,
            at[1] * 2.0 / height as f32 - 1.0, 0.0, 1.0],
        geometry: data, style,
    };
    let point = |slot: usize, at: Point| pixel(slot, [at.x() as f32 + 0.5, at.y() as f32 + 0.5]);
    let direction = |a: [f32; 2], b: [f32; 2]| -> Option<[f32; 2]> {
        let delta = [b[0] - a[0], b[1] - a[1]];
        let length = libm::sqrtf(delta[0] * delta[0] + delta[1] * delta[1]);
        (length > 1e-6).then(|| [delta[0] / length, delta[1] / length])
    };
    let glyphs: Vec<_> = if let Some(ch) = single { alloc::vec![(ch, 0)] } else {
        (' '..='~').chain(microfont::raster_line::GLYPH_CHARACTERS.iter().copied()
            .filter(|&ch| is_extra_line(ch)))
            .filter_map(|ch| glyph_slot(ch).map(|slot| (ch, slot))).collect()
    };
    for (ch, slot) in glyphs {
        let Some(strips) = microfont::raster_line::glyph_strips(ch) else { continue; };
        for strip in strips {
            let points: Vec<_> = strip.iter_points().map(|p| point(slot, p)).collect();
            for segment in points.windows(2) {
                let Some(inward) = direction(segment[0], segment[1]) else { continue; };
                let data = [segment[0][0], segment[0][1], inward[0], inward[1]];
                let style = [radius, 1.0, 0.0, 0.0];
                geometry.lines.extend(segment.iter().map(|&at| vertex(at, data, style)));
            }
            // A point's shader clips the disc to the added outer half only;
            // this leaves the original native line body coverage untouched.
            for (end, boundary) in [(0, strip.start), (points.len() - 1, strip.end)] {
                if boundary != Boundary::End { continue; }
                let neighbor = if end == 0 { points.iter().copied().find(|&p| p != points[end]) }
                    else { points.iter().rev().copied().find(|&p| p != points[end]) };
                if let Some(inward) = neighbor.and_then(|p| direction(points[end], p)) {
                    geometry.points.push(vertex(points[end],
                        [points[end][0], points[end][1], inward[0], inward[1]],
                        [radius, 2.0, 0.0, 0.0]));
                }
            }
            let mut join = |previous: [f32; 2], at: [f32; 2], next: [f32; 2]| {
                let (Some(incoming), Some(outgoing)) = (direction(previous, at), direction(at, next)) else { return; };
                let turn = incoming[0] * outgoing[1] - incoming[1] * outgoing[0];
                if turn.abs() <= 1e-6 { return; }
                let side = if turn > 0.0 { 1.0 } else { -1.0 };
                let mut a = [incoming[1] * radius * side, -incoming[0] * radius * side];
                let mut b = [outgoing[1] * radius * side, -outgoing[0] * radius * side];
                if a[0] * b[1] - a[1] * b[0] > 0.0 { core::mem::swap(&mut a, &mut b); }
                // Match the native mask recipe: bevel ordinary corners and
                // use an exact circular sector only at sharp exterior tips.
                let mode = if a[0] * b[0] + a[1] * b[1] < -1e-6 { 4.0 } else { 3.0 };
                geometry.points.push(vertex(at, [at[0], at[1], a[0], a[1]],
                    [radius, mode, b[0], b[1]]));
            };
            for triple in points.windows(3) { join(triple[0], triple[1], triple[2]); }
            if points.len() >= 4 && points.first() == points.last()
                && strip.start == Boundary::Junction && strip.end == Boundary::Junction {
                join(points[points.len() - 2], points[0], points[1]);
            }
        }
    }
    if single.is_none() {
        let mut quad = |slot: usize, corners: [[f32; 2]; 4]| {
            geometry.quads.extend(corners.map(|at| vertex(pixel(slot, at), [0.0; 4], [0.0; 4])));
        };
        for (i, &(ch, _)) in microfont::BOX_DRAW_SYMBOLS.iter().enumerate() {
            if let Some(quads) = microfont::raster_quad::boxdraw_quad_slice(ch) {
                for q in quads { quad(BOX_START + i, q.vertices); }
            }
        }
        for (i, &(ch, _)) in microfont::BLOCK_SYMBOLS.iter().enumerate() {
            if let Some(quads) = microfont::raster_quad::block_quad_slice(ch) {
                for q in quads {
                    let [l, t] = q.top_left.map(f32::from);
                    let [r, b] = q.bottom_right.map(f32::from);
                    quad(BLOCK_START + i, [[l, t], [r, t], [r, b], [l, b]]);
                }
            }
        }
        for code in 0x2800..=0x28ff {
            for q in microfont::raster_quad::braille_quad_slice(char::from_u32(code).unwrap()).unwrap() {
                let [l, t] = q.top_left.map(f32::from);
                let [r, b] = q.bottom_right.map(f32::from);
                quad(BRAILLE_START + (code - 0x2800) as usize, [[l, t], [r, t], [r, b], [l, b]]);
            }
        }
    }
    Ok(geometry)
}


pub(crate) struct CoverageAtlas {
    geometry: ResidentGlyphs,
    mask: Option<render::ResidentCoverageMask>,
    /// Virgl produces the same color-free service result using native lines,
    /// points and quads. Retain its fenced readback under this atlas lock.
    native_mask: Option<(Vec<u8>, u32, u32, u64)>,
    native_glyph: Option<char>,
    config: Option<AtlasConfig>,
    generation: u64,
    poisoned: bool,
}
impl Drop for CoverageAtlas {
    fn drop(&mut self) {
        if self.poisoned {
            // A future service teardown must not free vertices while an
            // accepted GPU submission has no proof of retirement.
            core::mem::forget(core::mem::take(&mut self.geometry.batches));
            if let Some(constants) = self.geometry.constants.take() {
                core::mem::forget(constants);
            }
            if let Some(mask) = self.mask.take() {
                core::mem::forget(mask);
            }
        } else if let Some(mask) = self.mask.take() {
            let _ = render::release_resident_coverage_mask(mask);
        }
    }
}
static ATLAS: spin::Mutex<Option<CoverageAtlas>> = spin::Mutex::new(None);

impl CoverageAtlas {
    pub(crate) fn surface(&self) -> GpgpuMask8Surface {
        if let Some((pixels, width, height, phys)) = &self.native_mask {
            return GpgpuMask8Surface { phys: *phys, bytes: pixels.len(), width: *width,
                height: *height, pitch_bytes: *width, ..Default::default() };
        }
        self.mask.as_ref().unwrap().surface()
    }
    /// Atlas rectangle in pixels; absent source glyphs retain an empty slot.
    pub(crate) fn glyph_rect(&self, ch: char) -> Option<[u32; 4]> {
        let slot = glyph_slot(ch)?;
        let (width, height) = self.config().cell_extent();
        Some([
            (slot % ATLAS_COLUMNS as usize) as u32 * width,
            (slot / ATLAS_COLUMNS as usize) as u32 * height,
            width, height,
        ])
    }
    pub(crate) fn config(&self) -> AtlasConfig {
        self.config.unwrap()
    }
    fn warm(&mut self, config: AtlasConfig) -> Result<(), &'static str> {
        self.warm_extent(config, config.extent())
    }
    fn warm_extent(&mut self, config: AtlasConfig, (width, height): (u32, u32)) -> Result<(), &'static str> {
        if self.poisoned {
            return Err("microfont-atlas-quarantined");
        }
        if self.config == Some(config) {
            return Ok(());
        }
        if crate::virtio_gpu_logo::output_dimensions().is_some() {
            self.config = None;
            let geometry = native_mask_geometry(config, width, height, self.native_glyph)?;
            let pixels = crate::virtio_gpu_logo::render_coverage_mask(width, height, geometry)?;
            let phys = crate::phys::virt_to_phys_checked(pixels.as_ptr())
                .ok_or("microfont-virgl-mask-address")?;
            self.native_mask = Some((pixels, width, height, phys));
            self.config = Some(config);
            self.generation += 1;
            crate::log_important!(target: "microfont-mask";
                "microfont-mask: warm generation={} format=r8 size={}x{} width_px={} backend=virgl-native-lines cpu_bitmap=0 glyph_triangles=0 fenced=1\n",
                self.generation, width, height, config.line_width_px);
            return Ok(());
        }
        if self
            .mask
            .as_ref()
            .is_some_and(|m| m.surface().width != width || m.surface().height != height)
        {
            if !render::release_resident_coverage_mask(self.mask.take().unwrap()) {
                self.poisoned = true;
                return Err("microfont-atlas-release");
            }
        }
        // Invalidate before writing: a failed submission cannot leave an older
        // configuration falsely marked warm after a partially changed target.
        self.config = None;
        if self.mask.is_none() {
            self.mask = Some(render::create_resident_coverage_mask(width, height)?);
        }
        let constants = self.geometry.constants.as_ref().unwrap();
        // Only dynamic values are written. Endpoint frames, boundary tags and
        // join kind remain in their original GPU allocation for service life.
        let transform = [
            config.scale(),
            config.line_width_px,
            1.0 / width as f32,
            1.0 / height as f32,
        ]
        .map(f32::to_bits);
        for (i, batch) in self.geometry.batches.iter().enumerate() {
            // Physical-size mode packs independent cells with a stroke-sized
            // gutter. It prevents wide strokes leaking into adjacent masks.
            if !constants.write(i * 64 + 36,
                &(config.glyph_height_px.unwrap_or(0) as f32).to_bits().to_le_bytes()) {
                return Err("microfont-constants-size");
            }
            if batch.topology == Topology::LineStripAdj
                && !constants
                    .write(i * 64 + 32, &(config.line_width_px * 0.5).to_bits().to_le_bytes())
            {
                return Err("microfont-constants-width");
            }
            for (k, word) in transform.iter().enumerate() {
                if !constants.write(i * 64 + 48 + k * 4, &word.to_le_bytes()) {
                    return Err("microfont-constants-transform");
                }
            }
        }
        constants.flush();
        let draws: Vec<_> = self
            .geometry
            .batches
            .iter()
            .enumerate()
            .map(|(i, batch)| render::ResidentSceneDraw {
                mesh: &batch.mesh,
                depth_flags: None,
                rgba: [255; 4],
                sampled_texture: None,
                fragment_contract: render::ResidentSceneFragmentContract::MicrofontMask(
                    constants.gpu_base() + i as u64 * 64,
                ),
                viewport_translation_px: [0.0; 2],
                topology: batch.topology,
                point_width_px: 0,
                line_width_px: if batch.topology == Topology::LineStripAdj {
                    config.line_width_px
                } else {
                    0.0
                },
                clip_adjacency: batch.topology == Topology::LineStripAdj,
            })
            .collect();
        // Err means no unretired GPU work: the encoder rejected the request,
        // or earlier chunks retired before a later chunk was rejected. Keep
        // the mask cold and reusable. Only uncertain completion below pins it.
        let result = render::render_resident_coverage_mask(&draws, self.mask.as_mut().unwrap())?;
        if let Some(reason) = result.completion_error() {
            self.poisoned = true;
            return Err(reason);
        }
        if result.release_fence.is_none() {
            self.poisoned = true;
            return Err("microfont-mask-release");
        }
        self.config = Some(config);
        self.generation += 1;
        crate::log_important!(target:"microfont-mask";
            "microfont-mask: warm generation={} format=r8 size={}x{} bytes={} scale={} width_px={} geometry_uploads=1 geometry_reuploads=0 gpu_body={:?} gpu_target_ready={:?} samples=1\n",
            self.generation,width,height,self.surface().bytes,config.scale(),config.line_width_px,
            result.gpu_timestamp,result.gpu_target_ready_timestamp);
        Ok(())
    }
}

/// Owns geometry for the lifetime of the service, independent of preview windows.
/// Repeated identical requests reuse the ready mask without submitting GPU work.
pub(crate) fn with_warmed_atlas<T>(
    config: AtlasConfig,
    consumer: impl FnOnce(&CoverageAtlas) -> Result<T, &'static str>,
) -> Result<T, &'static str> {
    let config = config.normalized()?;
    let mut guard = ATLAS.try_lock().ok_or("microfont-atlas-busy")?;
    warm_start_glyph()?;
    if guard.is_none() {
        *guard = Some(CoverageAtlas {
            geometry: if crate::virtio_gpu_logo::output_dimensions().is_some() {
                ResidentGlyphs { batches: Vec::new(), bytes: 0, constants: None }
            } else { upload_glyphs()? },
            mask: None,
            native_mask: None,
            native_glyph: None,
            config: None,
            generation: 0,
            poisoned: false,
        });
    }
    let atlas = guard.as_mut().unwrap();
    atlas.warm(config)?;
    consumer(atlas)
}

pub(crate) const START_GLYPH_EM_MM: u32 = 50;
static START_GLYPH: spin::Once<OwnedGlyphMask> = spin::Once::new();
static START_GLYPH_READY: crate::wait::WaitQueue = crate::wait::WaitQueue::new();
static START_GLYPH_WARM: spin::Mutex<()> = spin::Mutex::new(());

/// Return the service-owned mask as soon as its one-time production completes.
/// Observe before checking publication so completion cannot race waiter setup.
pub(crate) async fn wait_for_start_glyph() -> &'static OwnedGlyphMask {
    loop {
        let observed = START_GLYPH_READY.observe();
        if let Some(glyph) = START_GLYPH.get() {
            return glyph;
        }
        START_GLYPH_READY.wait_after(observed).await;
    }
}

// Called under the atlas service lock, before any regular atlas production.
pub(crate) fn warm_start_glyph() -> Result<(), &'static str> {
    if START_GLYPH.get().is_some() { return Ok(()); }
    let _warm = START_GLYPH_WARM.try_lock().ok_or("microfont-start-glyph-busy")?;
    if START_GLYPH.get().is_some() { return Ok(()); }
    let (_, height) = crate::ui4::physical_extent_pixels(START_GLYPH_EM_MM, START_GLYPH_EM_MM)
        .ok_or("microfont-start-glyph-physical-size-unavailable")?;
    let glyph = build_single_glyph_mask('§', height)?;
    let glyph = START_GLYPH.call_once(|| glyph);
    START_GLYPH_READY.notify_all();
    crate::log_important!(target: "microfont-mask";
        "microfont-start-glyph: ready glyph=U+00A7 em_mm={} em_height_px={} extent={}x{} format=r8 bytes={} productions=1 retained=service-lifetime\n",
        START_GLYPH_EM_MM, glyph.em_height_px, glyph.width, glyph.height, glyph.pixels.len());
    Ok(())
}

/// One independently warmed glyph. It never enlarges or invalidates the atlas.
pub(crate) struct OwnedGlyphMask {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) em_height_px: u32,
    pub(crate) pixels: Vec<u8>,
}

pub(crate) fn build_single_glyph_mask(ch: char, em_height_px: u32) -> Result<OwnedGlyphMask, &'static str> {
    if microfont::raster_line::glyph_strips(ch).is_none() {
        return Err("microfont-single-glyph-missing");
    }
    // A single glyph can be much larger than a full-atlas cell. Its 6x11
    // geometry and the existing native-line/cap/join shaders remain identical.
    let config = AtlasConfig { glyph_height_px: Some(em_height_px), line_width_px: 16.0, ..AtlasConfig::default() };
    if !(11..=4000).contains(&em_height_px) { return Err("microfont-single-glyph-size"); }
    let (width, height) = config.cell_extent();
    let mut geometry = ResidentGlyphs { batches: Vec::new(), bytes: 0, constants: None };
    if crate::virtio_gpu_logo::output_dimensions().is_none() {
        upload_line_glyph(&mut geometry, ch, 0)?;
        finish_upload(&mut geometry)?;
    }
    let mut glyph = CoverageAtlas { geometry, mask: None, native_mask: None,
        native_glyph: Some(ch), config: None, generation: 0, poisoned: false };
    glyph.warm_extent(config, (width, height))?;
    let surface = glyph.surface();
    let source = crate::phys::phys_to_virt(surface.phys as usize) as *mut u8;
    if !crate::intel::dma_flush_strided_row_spans(&[
        crate::intel::DmaFlushRows::new(source, width as usize, surface.pitch_bytes as usize, height as usize),
    ]) { return Err("microfont-single-glyph-cache"); }
    let mut pixels = alloc::vec![0; width as usize * height as usize];
    for y in 0..height as usize {
        for x in 0..width as usize {
            pixels[y * width as usize + x] = unsafe {
                core::ptr::read_volatile(source.add(y * surface.pitch_bytes as usize + x))
            };
        }
    }
    if !pixels.iter().any(|&coverage| coverage != 0) { return Err("microfont-single-glyph-empty"); }
    Ok(OwnedGlyphMask { width, height, em_height_px, pixels })
}

/// Immutable service-owned R8 snapshot. Keeping this data alive does not hold
/// the preview atlas lock; later Tab/width changes cannot replace its pixels.
/// Glyph rows are tightly packed, including the safe cap overhang gutter.
pub(crate) struct OwnedAlphaAtlas {
    pub(crate) em_height_px: u32,
    pub(crate) line_width_px: f32,
    pub(crate) glyph_width: u32,
    pub(crate) glyph_height: u32,
    pub(crate) gutter_px: u32,
    pixels: Vec<u8>,
}
impl OwnedAlphaAtlas {
    pub(crate) fn glyph(&self, ch: char) -> Option<&[u8]> {
        // Unminted line slots stay blank for the demo, but are absent to font
        // consumers so they can apply their missing-glyph policy.
        if ch != ' ' && microfont::raster_line::glyph_strips(ch).is_none()
            && !('\u{2500}'..='\u{259f}').contains(&ch)
            && !('\u{2800}'..='\u{28ff}').contains(&ch) {
            return None;
        }
        let slot = glyph_slot(ch)?;
        let bytes = self.glyph_width as usize * self.glyph_height as usize;
        self.pixels.get(slot * bytes..(slot + 1) * bytes)
    }
    pub(crate) fn glyphs(&self) -> impl Iterator<Item = (char, &[u8])> + '_ {
        (' '..='~').chain('\u{2500}'..='\u{259f}').chain('\u{2800}'..='\u{28ff}')
            .chain(microfont::raster_line::GLYPH_CHARACTERS.iter().copied()
                .filter(|&ch| is_extra_line(ch)))
            .filter_map(|ch| self.glyph(ch).map(|pixels| (ch, pixels)))
    }
}

/// One native-line/quad render submission and one R8 readback for Shell3's
/// service lifetime. Resident source geometry is shared with the demo; each
/// consumer retains its own immutable result after this function returns.
static REPORTED_FULL_ATLAS_SIZES: spin::Mutex<alloc::collections::BTreeSet<u32>> =
    spin::Mutex::new(alloc::collections::BTreeSet::new());

pub(crate) fn build_shell3_alpha_atlas(em_height_px: u32) -> Result<OwnedAlphaAtlas, &'static str> {
    let config = AtlasConfig {
        glyph_height_px: Some(em_height_px),
        line_width_px: 3.5,
        ..AtlasConfig::default()
    };
    with_warmed_atlas(config, |atlas| {
        let surface = atlas.surface();
        let source = crate::phys::phys_to_virt(surface.phys as usize) as *mut u8;
        if !crate::intel::dma_flush_strided_row_spans(&[
            crate::intel::DmaFlushRows::new(source, surface.width as usize,
                surface.pitch_bytes as usize, surface.height as usize),
        ]) {
            return Err("microfont-shell3-alpha-cache");
        }
        let (glyph_width, glyph_height) = config.cell_extent();
        let glyph_bytes = glyph_width as usize * glyph_height as usize;
        let mut pixels = alloc::vec![0; SLOT_COUNT * glyph_bytes];
        for slot in 0..SLOT_COUNT {
            let left = (slot % ATLAS_COLUMNS as usize) * glyph_width as usize;
            let top = (slot / ATLAS_COLUMNS as usize) * glyph_height as usize;
            for y in 0..glyph_height as usize {
                for x in 0..glyph_width as usize {
                    pixels[slot * glyph_bytes + y * glyph_width as usize + x] = unsafe {
                        core::ptr::read_volatile(source.add(
                            (top + y) * surface.pitch_bytes as usize + left + x))
                    };
                }
            }
        }
        if REPORTED_FULL_ATLAS_SIZES.lock().insert(em_height_px) {
            // ONCE severity with per-size deduplication: log_once! would instead
            // suppress all sizes after the first occurrence at this call site.
            crate::log_os::log_with_target_level("microfont-mask", crate::log_os::LogLevel::Once,
                format_args!("microfont-atlas: full em_height_px={} glyph_slots={} gpu_backing_bytes={} retained_cpu_mask_bytes={} total_backing_bytes={} shared_geometry_excluded=1\n",
                    em_height_px, SLOT_COUNT, surface.bytes, pixels.capacity(),
                    surface.bytes.saturating_add(pixels.capacity())));
        }
        Ok(OwnedAlphaAtlas {
            em_height_px,
            line_width_px: config.line_width_px,
            glyph_width,
            glyph_height,
            gutter_px: config.gutter(),
            pixels,
        })
    })
}

/// Preview sink only. The cached baseline remains color-free; apply tint and
/// opacity here exactly once. Output is premultiplied, with optional background.
pub(crate) fn stamp_preview(
    frame: ui4::FrameHandle,
    config: AtlasConfig,
    glyphs: &[char],
    columns: usize,
    display_scale: u32,
    ink: [u8; 4],
    background: [u8; 4],
) -> Result<usize, &'static str> {
    stamp_preview_colored(frame, config, glyphs, columns, display_scale, &[ink], background)
}

/// One ink broadcasts to every cell; otherwise provide one straight RGBA ink
/// per glyph cell. Coverage and opacity are premultiplied exactly once below.
pub(crate) fn stamp_preview_colored(
    frame: ui4::FrameHandle,
    config: AtlasConfig,
    glyphs: &[char],
    columns: usize,
    display_scale: u32,
    inks: &[[u8; 4]],
    background: [u8; 4],
) -> Result<usize, &'static str> {
    stamp_preview_colored_with_header(frame, config, glyphs, columns, display_scale, inks, background, None)
}

/// Optional fixed-size status row, separate from the native-line glyphs.
pub(crate) fn stamp_preview_colored_with_header(
    frame: ui4::FrameHandle,
    config: AtlasConfig,
    glyphs: &[char],
    columns: usize,
    display_scale: u32,
    inks: &[[u8; 4]],
    background: [u8; 4],
    header: Option<&str>,
) -> Result<usize, &'static str> {
    let header_height = if header.is_some() { PREVIEW_HEADER_HEIGHT } else { 0 };
    if inks.len() != 1 && inks.len() != glyphs.len() {
        return Err("microfont-stamp-colors");
    }
    with_warmed_atlas(config, |atlas| {
        let lease = ui4::acquire_frame_buffer(frame).map_err(|_| "frame-busy")?;
        let result = (|| {
            let view = ui4::writable_rgba_view(lease).map_err(|_| "frame-view")?;
            let mask = atlas.surface();
            let scale = atlas.config().glyph_scale;
            let (cell_width, cell_height) = if atlas.config().glyph_height_px.is_some() {
                atlas.config().cell_extent()
            } else { (6 * display_scale, 11 * display_scale) };
            if columns == 0
                || display_scale == 0
                || (atlas.config().glyph_height_px.is_none() && scale % display_scale != 0)
                || view.width != columns as u32 * cell_width
                || view.height != (glyphs.len().div_ceil(columns)) as u32 * cell_height + header_height
            {
                return Err("microfont-stamp-shape");
            }
            let header_width = view.width.saturating_sub(8) as usize / 2;
            let header_mask = header.map(|text| {
                let mut pixels = alloc::vec![0u8; header_width * microfont::FHEIGHT];
                microfont::stamp_bytes(&mut pixels, header_width, microfont::FHEIGHT,
                    0, 0, text.as_bytes(), 255u8).map_err(|_| "microfont-header-text")?;
                Ok::<_, &'static str>(pixels)
            }).transpose()?;
            let source = crate::phys::phys_to_virt(mask.phys as usize) as *mut u8;
            let spans = [
                crate::intel::DmaFlushRows::new(
                    source,
                    mask.width as usize,
                    mask.pitch_bytes as usize,
                    mask.height as usize,
                ),
                crate::intel::DmaFlushRows::new(
                    view.virt,
                    view.width as usize * 4,
                    view.pitch as usize,
                    view.height as usize,
                ),
            ];
            if !crate::intel::dma_flush_strided_row_spans(&spans) {
                return Err("microfont-stamp-cache");
            }
            let mut partial = 0usize;
            let mut nonzero = 0usize;
            for y in 0..view.height {
                for x in 0..view.width {
                    let in_header = y < header_height;
                    let body_y = y.saturating_sub(header_height);
                    let cell = (body_y / cell_height) as usize * columns
                        + (x / cell_width) as usize;
                    let coverage = glyphs
                        .get(cell).filter(|_| !in_header)
                        .and_then(|&ch| atlas.glyph_rect(ch))
                        .map_or(0, |rect| {
                            let sx = rect[0]
                                + ((2 * (x % cell_width) + 1) * rect[2] / (2 * cell_width)).min(rect[2] - 1);
                            let sy = rect[1]
                                + ((2 * (body_y % cell_height) + 1) * rect[3] / (2 * cell_height)).min(rect[3] - 1);
                            unsafe {
                                core::ptr::read_volatile(
                                    source.add((sy * mask.pitch_bytes + sx) as usize),
                                )
                            }
                        });
                    partial += usize::from(coverage != 0 && coverage != 255);
                    nonzero += usize::from(coverage != 0);
                    let ink = if inks.len() == 1 { inks[0] }
                        else { inks.get(cell).copied().unwrap_or([0; 4]) };
                    let pixel = if in_header {
                        let hx = x.saturating_sub(4) as usize / 2;
                        let hy = y.saturating_sub(4) as usize / 2;
                        let a = if x >= 4 && y >= 4 && hx < header_width && hy < microfont::FHEIGHT {
                            header_mask.as_ref().unwrap()[hy * header_width + hx]
                        } else { 0 };
                        tint_coverage(a, [48, 64, 96, 255], background)
                    } else { tint_coverage(coverage, ink, background) };
                    unsafe {
                        core::ptr::write_unaligned(
                            view.virt
                                .add(y as usize * view.pitch as usize + x as usize * 4)
                                .cast::<u32>(),
                            u32::from_le_bytes(pixel),
                        );
                    }
                }
            }
            if !crate::intel::dma_flush_strided_row_spans(&spans[1..]) {
                return Err("microfont-stamp-publish-cache");
            }
            ui4::mark_frame_buffer_cpu_authored(lease).map_err(|_| "frame-cpu-authored")?;
            if background[3] == 255 {
                ui4::mark_frame_buffer_fully_opaque(lease).map_err(|_| "frame-opaque")?;
            }
            ui4::publish_frame_buffer(lease).map_err(|_| "frame-publish")?;
            crate::log_important!(target:"microfont-mask";
                "microfont-mask: stamp generation={} glyphs={} copies=1 nonzero={} partial={} inks={} background={:?} geometry_uploads=1\n",
                atlas.generation,glyphs.len(),nonzero,partial,inks.len(),background);
            Ok(nonzero)
        })();
        if result.is_err() {
            let _ = ui4::cancel_frame_buffer(lease);
        }
        result
    })
}

fn tint_coverage(coverage: u8, ink: [u8; 4], background: [u8; 4]) -> [u8; 4] {
    let a = (u32::from(coverage) * u32::from(ink[3]) + 127) / 255;
    let mut out = [0; 4];
    for k in 0..3 {
        out[k] = ((u32::from(ink[k]) * a + u32::from(background[k]) * (255 - a) + 127) / 255) as u8;
    }
    out[3] = (a + (u32::from(background[3]) * (255 - a) + 127) / 255) as u8;
    out
}
