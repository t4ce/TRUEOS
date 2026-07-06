//! Font assets and warmed vector outline state.
//!
//! This module owns the real-font doorway for graphics. It keeps embedded font
//! bytes plus size-independent outline commands resident, while leaving
//! tessellation, raster masks, and GPU coverage as later consumers.

use alloc::vec::Vec;
use core::mem::size_of;

use lyon_tessellation::{
    BuffersBuilder, FillOptions, FillTessellator, FillVertex as LyonFillVertex,
    VertexBuffers as LyonVertexBuffers,
    math::point,
    path::{Builder as LyonPathBuilder, Path as LyonPath},
};
use skrifa::{
    FontRef, GlyphId, MetadataProvider,
    instance::{LocationRef, Size},
    outline::{DrawSettings, OutlinePen},
    raw::TableProvider,
};
use spin::Mutex;

const FONT_ENDSTATE_OUTLINE_COMMANDS: &str = "font-units-outline-commands";
const FONT_TESSEL_SAMPLE_TEXT: &str = "hello world";
const FONT_TESSEL_SAMPLE_PX: f32 = 48.0;
const FONT_CLIP_FIELD_AXES: usize = 6;
const FONT_CLIP_FIELD_RINGS: usize = 3;
pub(crate) const FONT_CLIP_FIELD_TRIANGLES: usize = FONT_CLIP_FIELD_AXES * FONT_CLIP_FIELD_RINGS;
pub(crate) const FONT_CLIP_FIELD_VERTICES: usize = FONT_CLIP_FIELD_TRIANGLES * 3;

const EMBEDDED_FONTS: [EmbeddedFontSpec; 1] = [EmbeddedFontSpec {
    name: "font",
    file_name: "L_10646.TTF",
    bytes: include_bytes!("../../tools/L_10646.TTF"),
}];

static FONT_REGISTRY: Mutex<FontRegistry> = Mutex::new(FontRegistry::new());

#[derive(Clone, Copy)]
struct EmbeddedFontSpec {
    name: &'static str,
    file_name: &'static str,
    bytes: &'static [u8],
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct FontWarmSummary {
    pub(crate) status: &'static str,
    pub(crate) name: &'static str,
    pub(crate) file_name: &'static str,
    pub(crate) endstate: &'static str,
    pub(crate) bytes: usize,
    pub(crate) tables: usize,
    pub(crate) glyphs: u16,
    pub(crate) units_per_em: u16,
    pub(crate) range_bytes: usize,
    pub(crate) op_bytes: usize,
    pub(crate) cache_bytes: usize,
    pub(crate) resident_bytes: usize,
    pub(crate) range_first_glyph: u16,
    pub(crate) range_last_glyph: u16,
    pub(crate) range_max_ops: u32,
    pub(crate) outline_glyphs: usize,
    pub(crate) outline_success: usize,
    pub(crate) outline_failures: usize,
    pub(crate) empty_outlines: usize,
    pub(crate) commands: usize,
    pub(crate) move_to: usize,
    pub(crate) line_to: usize,
    pub(crate) quad_to: usize,
    pub(crate) curve_to: usize,
    pub(crate) close: usize,
    pub(crate) min_x: f32,
    pub(crate) min_y: f32,
    pub(crate) max_x: f32,
    pub(crate) max_y: f32,
    pub(crate) parse_ms: u64,
    pub(crate) outline_ms: u64,
    pub(crate) total_ms: u64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct FontRegistrySummary {
    pub(crate) fonts: usize,
    pub(crate) endstates: usize,
    pub(crate) resident_bytes: usize,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct FontTesselSummary {
    pub(crate) status: &'static str,
    pub(crate) reason: &'static str,
    pub(crate) text: &'static str,
    pub(crate) font_name: &'static str,
    pub(crate) font_file: &'static str,
    pub(crate) outline_source: &'static str,
    pub(crate) px_size: f32,
    pub(crate) glyphs: usize,
    pub(crate) glyph_hits: usize,
    pub(crate) glyph_misses: usize,
    pub(crate) outline_glyphs: usize,
    pub(crate) empty_glyphs: usize,
    pub(crate) path_commands: usize,
    pub(crate) tessellate_failures: usize,
    pub(crate) vertices: usize,
    pub(crate) indices: usize,
    pub(crate) triangles: usize,
    pub(crate) vertex_bytes: usize,
    pub(crate) index_bytes: usize,
    pub(crate) geometry_bytes: usize,
    pub(crate) min_x: f32,
    pub(crate) min_y: f32,
    pub(crate) max_x: f32,
    pub(crate) max_y: f32,
    pub(crate) charmap_ms: u64,
    pub(crate) path_ms: u64,
    pub(crate) tessellate_ms: u64,
    pub(crate) total_ms: u64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct FontScratchTriangle {
    pub(crate) vertices: [[f32; 3]; 3],
    pub(crate) source_vertices: [[f32; 2]; 3],
    pub(crate) source_indices: [u32; 3],
    pub(crate) source_vertex_count: usize,
    pub(crate) source_index_count: usize,
    pub(crate) source_triangle_count: usize,
    pub(crate) source_area2: f32,
    pub(crate) scratch_area2: f32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct FontClipTriangleField {
    pub(crate) vertices: [[f32; 3]; FONT_CLIP_FIELD_VERTICES],
    pub(crate) vertex_count: usize,
    pub(crate) triangle_count: usize,
    pub(crate) axes: usize,
    pub(crate) rings: usize,
    pub(crate) radii: [f32; FONT_CLIP_FIELD_RINGS],
    pub(crate) sizes: [f32; FONT_CLIP_FIELD_RINGS],
    pub(crate) rotations_deg: [i32; FONT_CLIP_FIELD_RINGS],
    pub(crate) min_x: f32,
    pub(crate) min_y: f32,
    pub(crate) min_z: f32,
    pub(crate) max_x: f32,
    pub(crate) max_y: f32,
    pub(crate) max_z: f32,
}

impl FontScratchTriangle {
    pub(crate) fn snapped_vertices(self) -> [[f32; 3]; 3] {
        let mut vertices = self.vertices;
        for vertex in &mut vertices {
            vertex[0] = snap_scratch_coord(vertex[0]);
            vertex[1] = snap_scratch_coord(vertex[1]);
        }
        if triangle_area2_3d_screen(vertices) < 0.0 {
            vertices.swap(1, 2);
        }
        vertices
    }

    pub(crate) fn mirrored_clip_field(self) -> FontClipTriangleField {
        let _ = self;
        const RADII: [f32; FONT_CLIP_FIELD_RINGS] = [1.0, 10.0, 100.0];
        const SIZES: [f32; FONT_CLIP_FIELD_RINGS] = [1.0, 5.0, 25.0];
        const ROTATIONS: [i32; FONT_CLIP_FIELD_RINGS] = [0, 90, 180];

        let mut field = FontClipTriangleField {
            vertices: [[0.0; 3]; FONT_CLIP_FIELD_VERTICES],
            vertex_count: 0,
            triangle_count: 0,
            axes: FONT_CLIP_FIELD_AXES,
            rings: FONT_CLIP_FIELD_RINGS,
            radii: RADII,
            sizes: SIZES,
            rotations_deg: ROTATIONS,
            min_x: f32::INFINITY,
            min_y: f32::INFINITY,
            min_z: f32::INFINITY,
            max_x: f32::NEG_INFINITY,
            max_y: f32::NEG_INFINITY,
            max_z: f32::NEG_INFINITY,
        };

        for ring in 0..FONT_CLIP_FIELD_RINGS {
            let radius = RADII[ring];
            let size = SIZES[ring];
            for axis in 0..FONT_CLIP_FIELD_AXES {
                let (center, inward, tangent) = clip_field_axis(axis, ring, radius);
                push_facing_triangle(&mut field, center, inward, tangent, size);
            }
        }

        field
    }
}

impl FontClipTriangleField {
    pub(crate) fn isolated_scratch_triangle(self) -> [[f32; 3]; 3] {
        self.isolated_scratch_vertices::<3>()
    }

    pub(crate) fn isolated_scratch_two_triangles(self) -> [[f32; 3]; 6] {
        self.isolated_scratch_vertices::<6>()
    }

    pub(crate) fn isolated_scratch_all_triangles(self) -> [[f32; 3]; FONT_CLIP_FIELD_VERTICES] {
        self.isolated_scratch_vertices::<FONT_CLIP_FIELD_VERTICES>()
    }

    fn isolated_scratch_vertices<const N: usize>(self) -> [[f32; 3]; N] {
        let mut raw = [[0.0; 3]; N];
        let take = self.vertex_count.min(N).min(self.vertices.len());
        for (dst, src) in raw.iter_mut().zip(self.vertices.iter()).take(take) {
            *dst = *src;
        }

        let mut min_x = raw[0][0];
        let mut min_y = raw[0][1];
        let mut max_x = raw[0][0];
        let mut max_y = raw[0][1];
        for vertex in &raw[1..take] {
            min_x = min_x.min(vertex[0]);
            min_y = min_y.min(vertex[1]);
            max_x = max_x.max(vertex[0]);
            max_y = max_y.max(vertex[1]);
        }
        let width = (max_x - min_x).max(0.0001);
        let height = (max_y - min_y).max(0.0001);
        let sx = 7.0 / width;
        let sy = 7.0 / height;

        let mut vertices = [[0.0; 3]; N];
        for (dst, src) in vertices.iter_mut().zip(raw.iter()) {
            *dst = [
                0.5 + (src[0] - min_x) * sx,
                0.5 + (src[1] - min_y) * sy,
                0.0,
            ];
        }
        for triangle in vertices.chunks_exact_mut(3) {
            if triangle_area2_3d_screen([triangle[0], triangle[1], triangle[2]]) < 0.0 {
                triangle.swap(1, 2);
            }
        }
        vertices
    }
}

impl FontTesselSummary {
    fn failed(
        reason: &'static str,
        font_name: &'static str,
        font_file: &'static str,
        text: &'static str,
        px_size: f32,
        total_start: u64,
    ) -> Self {
        Self {
            status: "failed",
            reason,
            text,
            font_name,
            font_file,
            outline_source: "",
            px_size,
            glyphs: text.chars().count(),
            glyph_hits: 0,
            glyph_misses: 0,
            outline_glyphs: 0,
            empty_glyphs: 0,
            path_commands: 0,
            tessellate_failures: 0,
            vertices: 0,
            indices: 0,
            triangles: 0,
            vertex_bytes: 0,
            index_bytes: 0,
            geometry_bytes: 0,
            min_x: 0.0,
            min_y: 0.0,
            max_x: 0.0,
            max_y: 0.0,
            charmap_ms: 0,
            path_ms: 0,
            tessellate_ms: 0,
            total_ms: elapsed_ms_since(total_start),
        }
    }
}

pub(crate) fn default_font_summary() -> Option<FontWarmSummary> {
    font_summary("font")
}

pub(crate) fn font_summary(name: &str) -> Option<FontWarmSummary> {
    let registry = FONT_REGISTRY.lock();
    registry.font_by_name(name).and_then(|font| {
        font.endstates
            .iter()
            .find(|endstate| endstate.name() == FONT_ENDSTATE_OUTLINE_COMMANDS)
            .map(|endstate| endstate.summary(font, "registered", 0, 0, 0))
    })
}

pub(crate) fn registry_summary() -> FontRegistrySummary {
    let registry = FONT_REGISTRY.lock();
    let mut endstates = 0usize;
    let mut resident_bytes = 0usize;
    for font in &registry.fonts {
        for endstate in &font.endstates {
            endstates = endstates.saturating_add(1);
            resident_bytes = resident_bytes.saturating_add(font.bytes.len());
            resident_bytes = resident_bytes.saturating_add(endstate.cache_bytes());
        }
    }
    FontRegistrySummary {
        fonts: registry.fonts.len(),
        endstates,
        resident_bytes,
    }
}

pub(crate) fn tessellate_default_text() -> FontTesselSummary {
    tessellate_text("font", FONT_TESSEL_SAMPLE_TEXT, FONT_TESSEL_SAMPLE_PX)
}

pub(crate) fn font_tessellated_scratch_triangle() -> Option<FontScratchTriangle> {
    let _ = warm_embedded_fonts_once().ok()?;

    let registry = FONT_REGISTRY.lock();
    let font_record = registry.font_by_name("font")?;
    let FontWarmEndState::Outline(outline) = font_record.outline_endstate()?;
    let font = FontRef::new(font_record.bytes).ok()?;
    let charmap = font.charmap();
    let metrics = font.glyph_metrics(Size::unscaled(), LocationRef::default());
    let scale = FONT_TESSEL_SAMPLE_PX / (font_record.units_per_em as f32).max(1.0);
    let fallback_advance = font_record.units_per_em as f32 * 0.35;
    let space_advance = charmap
        .map(' ')
        .and_then(|glyph_id| metrics.advance_width(glyph_id))
        .unwrap_or(fallback_advance);

    let mut builder = LyonPath::builder();
    let mut bounds = TesselBounds::default();
    let mut pen_x = 0.0f32;
    let baseline_y = FONT_TESSEL_SAMPLE_PX;
    for ch in FONT_TESSEL_SAMPLE_TEXT.chars() {
        if ch.is_whitespace() {
            pen_x += space_advance * scale;
            continue;
        }
        let Some(glyph_id) = charmap.map(ch) else {
            pen_x += fallback_advance * scale;
            continue;
        };
        outline.append_glyph_path(glyph_id, &mut builder, pen_x, baseline_y, scale, &mut bounds);
        pen_x += metrics.advance_width(glyph_id).unwrap_or(fallback_advance) * scale;
    }
    let path = builder.build();

    let mut buffers: LyonVertexBuffers<[f32; 2], u32> = LyonVertexBuffers::new();
    FillTessellator::new()
        .tessellate_path(
            &path,
            &FillOptions::default(),
            &mut BuffersBuilder::new(&mut buffers, |vertex: LyonFillVertex| {
                let position = vertex.position();
                [position.x, position.y]
            }),
        )
        .ok()?;

    for indices in buffers.indices.chunks_exact(3) {
        let ia = indices[0] as usize;
        let ib = indices[1] as usize;
        let ic = indices[2] as usize;
        let Some((&a, &b, &c)) = buffers
            .vertices
            .get(ia)
            .zip(buffers.vertices.get(ib))
            .zip(buffers.vertices.get(ic))
            .map(|((a, b), c)| (a, b, c))
        else {
            continue;
        };

        let min_x = a[0].min(b[0]).min(c[0]);
        let min_y = a[1].min(b[1]).min(c[1]);
        let max_x = a[0].max(b[0]).max(c[0]);
        let max_y = a[1].max(b[1]).max(c[1]);
        let width = max_x - min_x;
        let height = max_y - min_y;
        let source_area2 = triangle_area2_2d(a, b, c);
        if source_area2.abs() <= 0.0001 || width <= 0.0001 || height <= 0.0001 {
            continue;
        }

        let sx = 7.0 / width;
        let sy = 7.0 / height;
        let mut vertices = [
            [0.5 + (a[0] - min_x) * sx, 0.5 + (a[1] - min_y) * sy, 0.0],
            [0.5 + (b[0] - min_x) * sx, 0.5 + (b[1] - min_y) * sy, 0.0],
            [0.5 + (c[0] - min_x) * sx, 0.5 + (c[1] - min_y) * sy, 0.0],
        ];
        if triangle_area2_3d_screen(vertices) < 0.0 {
            vertices.swap(1, 2);
        }

        return Some(FontScratchTriangle {
            vertices,
            source_vertices: [a, b, c],
            source_indices: [indices[0], indices[1], indices[2]],
            source_vertex_count: buffers.vertices.len(),
            source_index_count: buffers.indices.len(),
            source_triangle_count: buffers.indices.len() / 3,
            source_area2,
            scratch_area2: triangle_area2_3d_screen(vertices),
        });
    }

    None
}

fn tessellate_text(name: &'static str, text: &'static str, px_size: f32) -> FontTesselSummary {
    let total_start = embassy_time_driver::now();
    if warm_embedded_fonts_once().is_err() {
        return FontTesselSummary::failed("font-warm-failed", name, "", text, px_size, total_start);
    }

    let registry = FONT_REGISTRY.lock();
    let Some(font_record) = registry.font_by_name(name) else {
        return FontTesselSummary::failed(
            "font-not-registered",
            name,
            "",
            text,
            px_size,
            total_start,
        );
    };
    let Some(FontWarmEndState::Outline(outline)) = font_record.outline_endstate() else {
        return FontTesselSummary::failed(
            "outline-cache-missing",
            font_record.name,
            font_record.file_name,
            text,
            px_size,
            total_start,
        );
    };
    let Ok(font) = FontRef::new(font_record.bytes) else {
        return FontTesselSummary::failed(
            "font-parse-failed",
            font_record.name,
            font_record.file_name,
            text,
            px_size,
            total_start,
        );
    };

    let charmap_start = embassy_time_driver::now();
    let charmap = font.charmap();
    let metrics = font.glyph_metrics(Size::unscaled(), LocationRef::default());
    let scale = px_size / (font_record.units_per_em as f32).max(1.0);
    let fallback_advance = font_record.units_per_em as f32 * 0.35;
    let space_advance = charmap
        .map(' ')
        .and_then(|glyph_id| metrics.advance_width(glyph_id))
        .unwrap_or(fallback_advance);
    let charmap_ms = elapsed_ms_since(charmap_start);

    let path_start = embassy_time_driver::now();
    let mut builder = LyonPath::builder();
    let mut glyphs = 0usize;
    let mut glyph_hits = 0usize;
    let mut glyph_misses = 0usize;
    let mut outline_glyphs = 0usize;
    let mut empty_glyphs = 0usize;
    let mut path_commands = 0usize;
    let mut pen_x = 0.0f32;
    let baseline_y = px_size;
    let mut bounds = TesselBounds::default();
    for ch in text.chars() {
        glyphs = glyphs.saturating_add(1);
        if ch.is_whitespace() {
            pen_x += space_advance * scale;
            continue;
        }
        let Some(glyph_id) = charmap.map(ch) else {
            glyph_misses = glyph_misses.saturating_add(1);
            pen_x += fallback_advance * scale;
            continue;
        };
        glyph_hits = glyph_hits.saturating_add(1);
        let appended = outline.append_glyph_path(
            glyph_id,
            &mut builder,
            pen_x,
            baseline_y,
            scale,
            &mut bounds,
        );
        if appended == 0 {
            empty_glyphs = empty_glyphs.saturating_add(1);
        } else {
            outline_glyphs = outline_glyphs.saturating_add(1);
            path_commands = path_commands.saturating_add(appended);
        }
        pen_x += metrics.advance_width(glyph_id).unwrap_or(fallback_advance) * scale;
    }
    let path = builder.build();
    let path_ms = elapsed_ms_since(path_start);

    let tessellate_start = embassy_time_driver::now();
    let mut buffers: LyonVertexBuffers<[f32; 2], u32> = LyonVertexBuffers::new();
    let tessellated = FillTessellator::new().tessellate_path(
        &path,
        &FillOptions::default(),
        &mut BuffersBuilder::new(&mut buffers, |vertex: LyonFillVertex| {
            let position = vertex.position();
            [position.x, position.y]
        }),
    );
    let tessellate_ms = elapsed_ms_since(tessellate_start);
    let tessellate_failures = usize::from(tessellated.is_err());
    let vertices = buffers.vertices.len();
    let indices = buffers.indices.len();
    let vertex_bytes = vertices.saturating_mul(size_of::<[f32; 2]>());
    let index_bytes = indices.saturating_mul(size_of::<u32>());
    let geometry_bytes = vertex_bytes.saturating_add(index_bytes);

    FontTesselSummary {
        status: if tessellated.is_ok() { "ok" } else { "failed" },
        reason: if tessellated.is_ok() {
            "tessellated"
        } else {
            "lyon-fill-failed"
        },
        text,
        font_name: font_record.name,
        font_file: font_record.file_name,
        outline_source: outline.name,
        px_size,
        glyphs,
        glyph_hits,
        glyph_misses,
        outline_glyphs,
        empty_glyphs,
        path_commands,
        tessellate_failures,
        vertices,
        indices,
        triangles: indices / 3,
        vertex_bytes,
        index_bytes,
        geometry_bytes,
        min_x: bounds.min_x,
        min_y: bounds.min_y,
        max_x: bounds.max_x,
        max_y: bounds.max_y,
        charmap_ms,
        path_ms,
        tessellate_ms,
        total_ms: elapsed_ms_since(total_start),
    }
}

fn triangle_area2_2d(a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> f32 {
    (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])
}

fn triangle_area2_3d_screen(vertices: [[f32; 3]; 3]) -> f32 {
    (vertices[1][0] - vertices[0][0]) * (vertices[2][1] - vertices[0][1])
        - (vertices[2][0] - vertices[0][0]) * (vertices[1][1] - vertices[0][1])
}

fn clip_field_axis(axis: usize, ring: usize, radius: f32) -> ([f32; 3], [f32; 3], [f32; 3]) {
    const DIAG: f32 = 0.70710677;
    match axis {
        0 => (
            [radius, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            match ring % 4 {
                0 => [0.0, 1.0, 0.0],
                1 => [0.0, 0.0, 1.0],
                2 => [0.0, -1.0, 0.0],
                _ => [0.0, 0.0, -1.0],
            },
        ),
        1 => (
            [-radius, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            match ring % 4 {
                0 => [0.0, -1.0, 0.0],
                1 => [0.0, 0.0, 1.0],
                2 => [0.0, 1.0, 0.0],
                _ => [0.0, 0.0, -1.0],
            },
        ),
        2 => (
            [0.0, radius, 0.0],
            [0.0, -1.0, 0.0],
            match ring % 4 {
                0 => [-1.0, 0.0, 0.0],
                1 => [0.0, 0.0, 1.0],
                2 => [1.0, 0.0, 0.0],
                _ => [0.0, 0.0, -1.0],
            },
        ),
        3 => (
            [0.0, -radius, 0.0],
            [0.0, 1.0, 0.0],
            match ring % 4 {
                0 => [1.0, 0.0, 0.0],
                1 => [0.0, 0.0, 1.0],
                2 => [-1.0, 0.0, 0.0],
                _ => [0.0, 0.0, -1.0],
            },
        ),
        4 => {
            let (x, y, ix, iy, tx, ty) = match ring % 4 {
                0 => (radius, radius, -DIAG, -DIAG, -DIAG, DIAG),
                1 => (-radius, radius, DIAG, -DIAG, -DIAG, -DIAG),
                2 => (-radius, -radius, DIAG, DIAG, DIAG, -DIAG),
                _ => (radius, -radius, -DIAG, DIAG, DIAG, DIAG),
            };
            ([x, y, radius], [ix, iy, -1.0], [tx, ty, 0.0])
        }
        _ => {
            let (x, y, ix, iy, tx, ty) = match ring % 4 {
                0 => (-radius, -radius, DIAG, DIAG, DIAG, -DIAG),
                1 => (radius, -radius, -DIAG, DIAG, DIAG, DIAG),
                2 => (radius, radius, -DIAG, -DIAG, -DIAG, DIAG),
                _ => (-radius, radius, DIAG, -DIAG, -DIAG, -DIAG),
            };
            ([x, y, -radius], [ix, iy, 1.0], [tx, ty, 0.0])
        }
    }
}

fn push_facing_triangle(
    field: &mut FontClipTriangleField,
    center: [f32; 3],
    inward: [f32; 3],
    tangent: [f32; 3],
    size: f32,
) {
    if field.vertex_count + 3 > FONT_CLIP_FIELD_VERTICES {
        return;
    }

    let apex = add3(center, mul3(inward, size * 0.75));
    let base = add3(center, mul3(inward, -size * 0.25));
    let mut tri = [
        apex,
        add3(base, mul3(tangent, size * 0.5)),
        add3(base, mul3(tangent, -size * 0.5)),
    ];
    if triangle_area2_3d_screen(tri) < 0.0 {
        tri.swap(1, 2);
    }

    for vertex in tri {
        field.vertices[field.vertex_count] = vertex;
        field.vertex_count += 1;
        field.min_x = field.min_x.min(vertex[0]);
        field.min_y = field.min_y.min(vertex[1]);
        field.min_z = field.min_z.min(vertex[2]);
        field.max_x = field.max_x.max(vertex[0]);
        field.max_y = field.max_y.max(vertex[1]);
        field.max_z = field.max_z.max(vertex[2]);
    }
    field.triangle_count += 1;
}

fn add3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn mul3(a: [f32; 3], scale: f32) -> [f32; 3] {
    [a[0] * scale, a[1] * scale, a[2] * scale]
}

fn snap_scratch_coord(value: f32) -> f32 {
    let scaled = value * 2.0;
    let snapped = ((scaled + 0.5) as i32 as f32) * 0.5;
    if snapped < 0.5 {
        0.5
    } else if snapped > 7.5 {
        7.5
    } else {
        snapped
    }
}

pub(crate) fn warm_embedded_fonts_once() -> Result<Vec<FontWarmSummary>, skrifa::raw::ReadError> {
    let mut summaries = Vec::with_capacity(EMBEDDED_FONTS.len());
    for index in 0..EMBEDDED_FONTS.len() {
        summaries.push(warm_embedded_font_once(index)?);
    }
    Ok(summaries)
}

fn warm_embedded_font_once(index: usize) -> Result<FontWarmSummary, skrifa::raw::ReadError> {
    let spec = EMBEDDED_FONTS[index];
    if let Some(summary) = font_summary(spec.name) {
        return Ok(FontWarmSummary {
            status: "warm-cache",
            ..summary
        });
    }

    let total_start = embassy_time_driver::now();
    let parse_start = embassy_time_driver::now();
    let font = FontRef::new(spec.bytes)?;
    let head = font.head()?;
    let maxp = font.maxp()?;
    let parse_ms = elapsed_ms_since(parse_start);

    let mut prepared = RegisteredFont::new(
        spec.name,
        spec.file_name,
        spec.bytes,
        font.table_directory().table_records().len(),
        maxp.num_glyphs(),
        head.units_per_em(),
    );

    let mut outline = FontOutlineCache::new(FONT_ENDSTATE_OUTLINE_COMMANDS, maxp.num_glyphs());
    let outlines = font.outline_glyphs();
    let outline_start = embassy_time_driver::now();
    for glyph_index in 0..maxp.num_glyphs() {
        let glyph_id = GlyphId::new(u32::from(glyph_index));
        let Some(glyph) = outlines.get(glyph_id) else {
            outline.outline_failures = outline.outline_failures.saturating_add(1);
            continue;
        };
        outline.outline_glyphs = outline.outline_glyphs.saturating_add(1);
        let start = outline.ops.len() as u32;
        let mut pen = WarmOutlinePen::default();
        let settings = DrawSettings::unhinted(Size::unscaled(), LocationRef::default());
        match glyph.draw(settings, &mut pen) {
            Ok(_) => {
                if pen.ops.is_empty() {
                    outline.empty_outlines = outline.empty_outlines.saturating_add(1);
                } else {
                    outline.outline_success = outline.outline_success.saturating_add(1);
                }
                outline.merge_pen(glyph_index, start, pen);
            }
            Err(_) => {
                outline.outline_failures = outline.outline_failures.saturating_add(1);
            }
        }
    }
    let outline_ms = elapsed_ms_since(outline_start);
    let total_ms = elapsed_ms_since(total_start);
    prepared.endstates.push(FontWarmEndState::Outline(outline));

    let summary = prepared
        .outline_endstate()
        .map(|endstate| endstate.summary(&prepared, "cold-built", parse_ms, outline_ms, total_ms))
        .unwrap_or_else(|| prepared.empty_summary("cold-built", parse_ms, outline_ms, total_ms));

    let mut registry = FONT_REGISTRY.lock();
    if let Some(existing) = registry.font_by_name(spec.name)
        && let Some(endstate) = existing.outline_endstate()
    {
        return Ok(endstate.summary(existing, "warm-cache", 0, 0, 0));
    }
    registry.fonts.push(prepared);
    Ok(summary)
}

#[embassy_executor::task]
pub(crate) async fn font_warm_task() {
    match warm_embedded_fonts_once() {
        Ok(summaries) => {
            for summary in summaries {
                crate::log_info!(
                    target: "boot";
                    "graphics-font: status={} name={} file={} endstate={} resident_bytes={} outline_cache_bytes={} glyphs={} success={} empty={} failures={} commands={} outline_ms={} total_ms={}\n",
                    summary.status,
                    summary.name,
                    summary.file_name,
                    summary.endstate,
                    summary.resident_bytes,
                    summary.cache_bytes,
                    summary.glyphs,
                    summary.outline_success,
                    summary.empty_outlines,
                    summary.outline_failures,
                    summary.commands,
                    summary.outline_ms,
                    summary.total_ms
                );
            }
        }
        Err(err) => crate::log_warn!(
            target: "boot";
            "graphics-font: status=failed err={:?}\n",
            err
        ),
    }
}

struct FontRegistry {
    fonts: Vec<RegisteredFont>,
}

impl FontRegistry {
    const fn new() -> Self {
        Self { fonts: Vec::new() }
    }

    fn font_by_name(&self, name: &str) -> Option<&RegisteredFont> {
        self.fonts.iter().find(|font| font.name == name)
    }
}

struct RegisteredFont {
    name: &'static str,
    file_name: &'static str,
    bytes: &'static [u8],
    tables: usize,
    glyphs: u16,
    units_per_em: u16,
    endstates: Vec<FontWarmEndState>,
}

impl RegisteredFont {
    fn new(
        name: &'static str,
        file_name: &'static str,
        bytes: &'static [u8],
        tables: usize,
        glyphs: u16,
        units_per_em: u16,
    ) -> Self {
        Self {
            name,
            file_name,
            bytes,
            tables,
            glyphs,
            units_per_em,
            endstates: Vec::with_capacity(1),
        }
    }

    fn outline_endstate(&self) -> Option<&FontWarmEndState> {
        self.endstates
            .iter()
            .find(|endstate| endstate.name() == FONT_ENDSTATE_OUTLINE_COMMANDS)
    }

    fn empty_summary(
        &self,
        status: &'static str,
        parse_ms: u64,
        outline_ms: u64,
        total_ms: u64,
    ) -> FontWarmSummary {
        FontWarmSummary {
            status,
            name: self.name,
            file_name: self.file_name,
            endstate: FONT_ENDSTATE_OUTLINE_COMMANDS,
            bytes: self.bytes.len(),
            tables: self.tables,
            glyphs: self.glyphs,
            units_per_em: self.units_per_em,
            range_bytes: 0,
            op_bytes: 0,
            cache_bytes: 0,
            resident_bytes: self.bytes.len(),
            range_first_glyph: 0,
            range_last_glyph: 0,
            range_max_ops: 0,
            outline_glyphs: 0,
            outline_success: 0,
            outline_failures: 0,
            empty_outlines: 0,
            commands: 0,
            move_to: 0,
            line_to: 0,
            quad_to: 0,
            curve_to: 0,
            close: 0,
            min_x: 0.0,
            min_y: 0.0,
            max_x: 0.0,
            max_y: 0.0,
            parse_ms,
            outline_ms,
            total_ms,
        }
    }
}

enum FontWarmEndState {
    Outline(FontOutlineCache),
}

impl FontWarmEndState {
    fn name(&self) -> &'static str {
        match self {
            FontWarmEndState::Outline(outline) => outline.name,
        }
    }

    fn cache_bytes(&self) -> usize {
        match self {
            FontWarmEndState::Outline(outline) => outline.cache_bytes(),
        }
    }

    fn summary(
        &self,
        font: &RegisteredFont,
        status: &'static str,
        parse_ms: u64,
        outline_ms: u64,
        total_ms: u64,
    ) -> FontWarmSummary {
        match self {
            FontWarmEndState::Outline(outline) => {
                outline.summary(font, status, parse_ms, outline_ms, total_ms)
            }
        }
    }
}

struct FontOutlineCache {
    name: &'static str,
    ranges: Vec<WarmGlyphRange>,
    ops: Vec<WarmOutlineOp>,
    outline_glyphs: usize,
    outline_success: usize,
    outline_failures: usize,
    empty_outlines: usize,
    move_to: usize,
    line_to: usize,
    quad_to: usize,
    curve_to: usize,
    close: usize,
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
    has_bounds: bool,
}

impl FontOutlineCache {
    fn new(name: &'static str, glyphs: u16) -> Self {
        Self {
            name,
            ranges: Vec::with_capacity(glyphs as usize),
            ops: Vec::new(),
            outline_glyphs: 0,
            outline_success: 0,
            outline_failures: 0,
            empty_outlines: 0,
            move_to: 0,
            line_to: 0,
            quad_to: 0,
            curve_to: 0,
            close: 0,
            min_x: 0.0,
            min_y: 0.0,
            max_x: 0.0,
            max_y: 0.0,
            has_bounds: false,
        }
    }

    fn merge_pen(&mut self, glyph_index: u16, start: u32, pen: WarmOutlinePen) {
        let len = pen.ops.len() as u32;
        self.move_to = self.move_to.saturating_add(pen.move_to);
        self.line_to = self.line_to.saturating_add(pen.line_to);
        self.quad_to = self.quad_to.saturating_add(pen.quad_to);
        self.curve_to = self.curve_to.saturating_add(pen.curve_to);
        self.close = self.close.saturating_add(pen.close);
        for op in &pen.ops {
            op.visit_points(|x, y| self.include_point(x, y));
        }
        self.ops.extend_from_slice(pen.ops.as_slice());
        self.ranges.push(WarmGlyphRange {
            glyph_index,
            op_start: start,
            op_len: len,
        });
    }

    fn include_point(&mut self, x: f32, y: f32) {
        if self.has_bounds {
            self.min_x = self.min_x.min(x);
            self.min_y = self.min_y.min(y);
            self.max_x = self.max_x.max(x);
            self.max_y = self.max_y.max(y);
        } else {
            self.min_x = x;
            self.min_y = y;
            self.max_x = x;
            self.max_y = y;
            self.has_bounds = true;
        }
    }

    fn cache_bytes(&self) -> usize {
        self.ranges
            .len()
            .saturating_mul(size_of::<WarmGlyphRange>())
            .saturating_add(self.ops.len().saturating_mul(size_of::<WarmOutlineOp>()))
    }

    fn append_glyph_path(
        &self,
        glyph_id: GlyphId,
        builder: &mut LyonPathBuilder,
        pen_x: f32,
        baseline_y: f32,
        scale: f32,
        bounds: &mut TesselBounds,
    ) -> usize {
        let glyph_index = glyph_id.to_u32();
        let Some(range) = self
            .ranges
            .iter()
            .find(|range| u32::from(range.glyph_index) == glyph_index)
        else {
            return 0;
        };
        let start = range.op_start as usize;
        let end = start
            .saturating_add(range.op_len as usize)
            .min(self.ops.len());
        if start >= end {
            return 0;
        }

        let mut open = false;
        let mut appended = 0usize;
        for op in &self.ops[start..end] {
            op.append_to_builder(builder, pen_x, baseline_y, scale, bounds, &mut open);
            appended = appended.saturating_add(1);
        }
        if open {
            builder.end(false);
        }
        appended
    }

    fn summary(
        &self,
        font: &RegisteredFont,
        status: &'static str,
        parse_ms: u64,
        outline_ms: u64,
        total_ms: u64,
    ) -> FontWarmSummary {
        let range_bytes = self
            .ranges
            .len()
            .saturating_mul(size_of::<WarmGlyphRange>());
        let op_bytes = self.ops.len().saturating_mul(size_of::<WarmOutlineOp>());
        let cache_bytes = range_bytes.saturating_add(op_bytes);
        let mut range_first_glyph = 0u16;
        let mut range_last_glyph = 0u16;
        let mut range_max_ops = 0u32;
        let mut range_high_end = 0u32;
        for (index, range) in self.ranges.iter().enumerate() {
            if index == 0 {
                range_first_glyph = range.glyph_index;
            }
            range_last_glyph = range.glyph_index;
            range_max_ops = range_max_ops.max(range.op_len);
            range_high_end = range_high_end.max(range.op_start.saturating_add(range.op_len));
        }
        FontWarmSummary {
            status,
            name: font.name,
            file_name: font.file_name,
            endstate: self.name,
            bytes: font.bytes.len(),
            tables: font.tables,
            glyphs: font.glyphs,
            units_per_em: font.units_per_em,
            range_bytes,
            op_bytes,
            cache_bytes,
            resident_bytes: font.bytes.len().saturating_add(cache_bytes),
            range_first_glyph,
            range_last_glyph,
            range_max_ops: range_max_ops.min(range_high_end),
            outline_glyphs: self.outline_glyphs,
            outline_success: self.outline_success,
            outline_failures: self.outline_failures,
            empty_outlines: self.empty_outlines,
            commands: self.ops.len(),
            move_to: self.move_to,
            line_to: self.line_to,
            quad_to: self.quad_to,
            curve_to: self.curve_to,
            close: self.close,
            min_x: self.min_x,
            min_y: self.min_y,
            max_x: self.max_x,
            max_y: self.max_y,
            parse_ms,
            outline_ms,
            total_ms,
        }
    }
}

#[derive(Default)]
struct TesselBounds {
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
    has_bounds: bool,
}

impl TesselBounds {
    fn include(&mut self, x: f32, y: f32) {
        if self.has_bounds {
            self.min_x = self.min_x.min(x);
            self.min_y = self.min_y.min(y);
            self.max_x = self.max_x.max(x);
            self.max_y = self.max_y.max(y);
        } else {
            self.min_x = x;
            self.min_y = y;
            self.max_x = x;
            self.max_y = y;
            self.has_bounds = true;
        }
    }
}

#[derive(Clone, Copy)]
struct WarmGlyphRange {
    glyph_index: u16,
    op_start: u32,
    op_len: u32,
}

#[derive(Clone, Copy)]
enum WarmOutlineOp {
    MoveTo(f32, f32),
    LineTo(f32, f32),
    QuadTo(f32, f32, f32, f32),
    CurveTo(f32, f32, f32, f32, f32, f32),
    Close,
}

impl WarmOutlineOp {
    fn append_to_builder(
        self,
        builder: &mut LyonPathBuilder,
        pen_x: f32,
        baseline_y: f32,
        scale: f32,
        bounds: &mut TesselBounds,
        open: &mut bool,
    ) {
        let map = |x: f32, y: f32| (pen_x + x * scale, baseline_y - y * scale);
        match self {
            WarmOutlineOp::MoveTo(x, y) => {
                if *open {
                    builder.end(false);
                }
                let (x, y) = map(x, y);
                bounds.include(x, y);
                builder.begin(point(x, y));
                *open = true;
            }
            WarmOutlineOp::LineTo(x, y) => {
                let (x, y) = map(x, y);
                bounds.include(x, y);
                builder.line_to(point(x, y));
            }
            WarmOutlineOp::QuadTo(cx0, cy0, x, y) => {
                let (cx0, cy0) = map(cx0, cy0);
                let (x, y) = map(x, y);
                bounds.include(cx0, cy0);
                bounds.include(x, y);
                builder.quadratic_bezier_to(point(cx0, cy0), point(x, y));
            }
            WarmOutlineOp::CurveTo(cx0, cy0, cx1, cy1, x, y) => {
                let (cx0, cy0) = map(cx0, cy0);
                let (cx1, cy1) = map(cx1, cy1);
                let (x, y) = map(x, y);
                bounds.include(cx0, cy0);
                bounds.include(cx1, cy1);
                bounds.include(x, y);
                builder.cubic_bezier_to(point(cx0, cy0), point(cx1, cy1), point(x, y));
            }
            WarmOutlineOp::Close => {
                if *open {
                    builder.close();
                    *open = false;
                }
            }
        }
    }

    fn visit_points(self, mut visit: impl FnMut(f32, f32)) {
        match self {
            WarmOutlineOp::MoveTo(x, y) | WarmOutlineOp::LineTo(x, y) => visit(x, y),
            WarmOutlineOp::QuadTo(cx0, cy0, x, y) => {
                visit(cx0, cy0);
                visit(x, y);
            }
            WarmOutlineOp::CurveTo(cx0, cy0, cx1, cy1, x, y) => {
                visit(cx0, cy0);
                visit(cx1, cy1);
                visit(x, y);
            }
            WarmOutlineOp::Close => {}
        }
    }
}

#[derive(Default)]
struct WarmOutlinePen {
    ops: Vec<WarmOutlineOp>,
    move_to: usize,
    line_to: usize,
    quad_to: usize,
    curve_to: usize,
    close: usize,
}

impl OutlinePen for WarmOutlinePen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.move_to = self.move_to.saturating_add(1);
        self.ops.push(WarmOutlineOp::MoveTo(x, y));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.line_to = self.line_to.saturating_add(1);
        self.ops.push(WarmOutlineOp::LineTo(x, y));
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.quad_to = self.quad_to.saturating_add(1);
        self.ops.push(WarmOutlineOp::QuadTo(cx0, cy0, x, y));
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.curve_to = self.curve_to.saturating_add(1);
        self.ops
            .push(WarmOutlineOp::CurveTo(cx0, cy0, cx1, cy1, x, y));
    }

    fn close(&mut self) {
        self.close = self.close.saturating_add(1);
        self.ops.push(WarmOutlineOp::Close);
    }
}

fn elapsed_ms_since(start: u64) -> u64 {
    let now = embassy_time_driver::now();
    let ticks = now.saturating_sub(start);
    let hz = embassy_time_driver::TICK_HZ;
    if hz == 0 {
        0
    } else {
        ticks.saturating_mul(1000) / hz
    }
}
