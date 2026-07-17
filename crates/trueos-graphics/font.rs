//! Font assets and warmed vector outline state.
//!
//! This module owns the real-font doorway for graphics. It keeps font bytes
//! plus size-independent outline commands resident, while leaving
//! tessellation, raster masks, and GPU coverage as later consumers.

use alloc::{string::String, vec::Vec};
use core::mem::size_of;

use skrifa::{
    FontRef, GlyphId, MetadataProvider,
    instance::{LocationRef, Size},
    outline::{DrawSettings, HintingInstance, HintingOptions, OutlinePen},
    raw::TableProvider,
};
use spin::Mutex;

use super::path_mesh::{
    FillError, FillOptions, Path as FillPath, PathBuilder as FillPathBuilder, Point, point,
};

const FONT_ENDSTATE_OUTLINE_COMMANDS: &str = "font-units-outline-commands";
const FONT_TESSEL_SAMPLE_TEXT: &str = "True OS §";
pub(crate) const FONT_TESSEL_BASE_PX: f32 = 48.0;
pub(crate) const FONT_GPU_OUTLINE_OP_WORDS: usize = 8;
// The restored render probes retain their one/two/all clip-field dispatch
// labels. Keep the original full-field vertex count at the graphics boundary.
pub(crate) const FONT_CLIP_FIELD_VERTICES: usize = 6 * 3 * 3;

const EMBEDDED_FONTS: [EmbeddedFontSpec; 1] = [EmbeddedFontSpec {
    name: "font",
    file_name: "L_10646.TTF",
    bytes: include_bytes!("../../tools/L_10646.TTF"),
}];
const TRUEOSFS_FONTS: [TrueosFsFontSpec; 2] = [
    TrueosFsFontSpec {
        name: "inconsolata",
        file_name: "Inconsolata-Regular.ttf",
        path: "fonts/Inconsolata-Regular.ttf",
    },
    TrueosFsFontSpec {
        name: "noto-sans-sc",
        file_name: "NotoSansSC[wght].ttf",
        path: "fonts/NotoSansSC[wght].ttf",
    },
];
const TRUEOSFS_FONT_HEARTBEAT_SECS: u64 = 30;
static FONT_REGISTRY: Mutex<FontRegistry> = Mutex::new(FontRegistry::new());

#[derive(Clone, Copy)]
struct EmbeddedFontSpec {
    name: &'static str,
    file_name: &'static str,
    bytes: &'static [u8],
}

#[derive(Clone, Copy)]
struct TrueosFsFontSpec {
    name: &'static str,
    file_name: &'static str,
    path: &'static str,
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
#[derive(Clone, Debug)]
pub(crate) struct FontTesselSummary {
    pub(crate) status: &'static str,
    pub(crate) reason: &'static str,
    pub(crate) text: String,
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

pub(crate) struct FontTesselMesh {
    pub(crate) summary: FontTesselSummary,
    pub(crate) vertices: Vec<[f32; 2]>,
    pub(crate) indices: Vec<u32>,
}

/// Per-call glyph geometry. This avoids running the scanline fill over one
/// combined multi-glyph path and lets repeated glyphs reuse their local mesh.
/// It is dropped with the completed text mesh and is never made resident.
struct TransientGlyphMesh {
    glyph_index: u32,
    path_commands: usize,
    advance_width: f32,
    bounds: TesselBounds,
    vertices: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

/// GPU-facing, size-independent outline stream for the font compute probes.
///
/// Each record is eight little-endian dwords. Word zero is the operation kind
/// (`move`, `line`, `quad`, `cubic`, `close` = 0..=4); the remaining words are
/// IEEE-754 coordinates followed by a reserved zero. Coordinates stay in font
/// units. Scale, baseline/Y orientation, curve flattening, and mesh generation
/// are deliberately left to the compute artifact.
pub(crate) struct FontGpuOutline {
    pub(crate) text: &'static str,
    pub(crate) font_name: &'static str,
    pub(crate) font_file: &'static str,
    pub(crate) units_per_em: u16,
    pub(crate) glyphs: usize,
    pub(crate) contours: usize,
    pub(crate) checksum: u32,
    pub(crate) ops: Vec<[u32; FONT_GPU_OUTLINE_OP_WORDS]>,
}

pub(crate) fn default_gpu_outline() -> Result<FontGpuOutline, &'static str> {
    gpu_outline_for_text("font", FONT_TESSEL_SAMPLE_TEXT)
}

fn gpu_outline_for_text(
    name: &'static str,
    text: &'static str,
) -> Result<FontGpuOutline, &'static str> {
    match warm_embedded_font_by_name(name).map_err(|_| "font-warm-failed")? {
        Some(_) => {}
        None => return Err("font-not-registered"),
    }
    let registry = FONT_REGISTRY.lock();
    let font_record = registry.font_by_name(name).ok_or("font-not-registered")?;
    let FontWarmEndState::Outline(outline) = font_record
        .outline_endstate()
        .ok_or("outline-cache-missing")?;
    let font = FontRef::new(font_record.bytes.as_slice()).map_err(|_| "font-parse-failed")?;
    let charmap = font.charmap();
    let metrics = font.glyph_metrics(Size::unscaled(), LocationRef::default());
    let fallback_advance = font_record.units_per_em as f32 * 0.35;
    let space_advance = charmap
        .map(' ')
        .and_then(|glyph_id| metrics.advance_width(glyph_id))
        .unwrap_or(fallback_advance);
    let mut ops = Vec::new();
    let mut pen_x = 0.0f32;
    let mut glyphs = 0usize;
    let mut contours = 0usize;
    for ch in text.chars() {
        if ch.is_whitespace() {
            pen_x += space_advance;
            continue;
        }
        let Some(glyph_id) = charmap.map(ch) else {
            pen_x += fallback_advance;
            continue;
        };
        glyphs = glyphs.saturating_add(1);
        contours = contours.saturating_add(outline.append_glyph_gpu_ops(glyph_id, pen_x, &mut ops));
        pen_x += metrics.advance_width(glyph_id).unwrap_or(fallback_advance);
    }
    if ops.is_empty() {
        return Err("outline-empty");
    }
    let checksum = outline_words_checksum(ops.as_slice());
    Ok(FontGpuOutline {
        text,
        font_name: font_record.name,
        font_file: font_record.file_name,
        units_per_em: font_record.units_per_em,
        glyphs,
        contours,
        checksum,
        ops,
    })
}

fn outline_words_checksum(ops: &[[u32; FONT_GPU_OUTLINE_OP_WORDS]]) -> u32 {
    let mut hash = 0x811C_9DC5u32;
    for op in ops {
        for word in op {
            hash ^= *word;
            hash = hash.wrapping_mul(0x0100_0193);
        }
    }
    hash
}

impl FontTesselMesh {
    fn failed(
        reason: &'static str,
        font_name: &'static str,
        font_file: &'static str,
        text: &str,
        px_size: f32,
        total_start: u64,
    ) -> Self {
        Self {
            summary: FontTesselSummary::failed(
                reason,
                font_name,
                font_file,
                text,
                px_size,
                total_start,
            ),
            vertices: Vec::new(),
            indices: Vec::new(),
        }
    }
}

impl FontTesselSummary {
    fn failed(
        reason: &'static str,
        font_name: &'static str,
        font_file: &'static str,
        text: &str,
        px_size: f32,
        total_start: u64,
    ) -> Self {
        Self {
            status: "failed",
            reason,
            text: text.into(),
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

/// Report whether every non-whitespace character has a glyph in a warmed
/// face. This lets retained scenes choose a compact primary face per text run
/// while keeping a broad Unicode face as a transparent fallback.
pub(crate) fn font_supports_text(name: &str, text: &str) -> bool {
    let registry = FONT_REGISTRY.lock();
    let Some(font_record) = registry.font_by_name(name) else {
        return false;
    };
    let Ok(font) = FontRef::new(font_record.bytes.as_slice()) else {
        return false;
    };
    let charmap = font.charmap();
    text.chars()
        .all(|ch| ch.is_whitespace() || charmap.map(ch).is_some())
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
    tessellate_default_text_mesh().summary
}

pub(crate) fn tessellate_default_text_mesh() -> FontTesselMesh {
    tessellate_text_mesh("font", FONT_TESSEL_SAMPLE_TEXT, FONT_TESSEL_BASE_PX)
}

pub(crate) fn tessellate_text_mesh(name: &'static str, text: &str, px_size: f32) -> FontTesselMesh {
    tessellate_text_mesh_grouped(name, text, px_size, None, FillOptions::DEFAULT, false)
}

pub(crate) fn tessellate_text_mesh_with_tolerance(
    name: &'static str,
    text: &str,
    px_size: f32,
    tolerance: f32,
) -> FontTesselMesh {
    tessellate_text_mesh_grouped(
        name,
        text,
        px_size,
        None,
        FillOptions::DEFAULT.with_tolerance(tolerance),
        false,
    )
}

/// Tessellate at the final raster ppem after applying the face's smooth-target
/// hinting program. This is intentionally separate from the warmed unscaled
/// outline path: retained scene consumers opt in only when they know the
/// physical pixel scale of their final target.
pub(crate) fn tessellate_text_mesh_hinted(
    name: &'static str,
    text: &str,
    px_size: f32,
) -> FontTesselMesh {
    tessellate_text_mesh_grouped(name, text, px_size, None, FillOptions::DEFAULT, true)
}

pub(crate) fn tessellate_text_rows_mesh(
    name: &'static str,
    text: &str,
    px_size: f32,
    row_lengths: &[usize],
) -> FontTesselMesh {
    tessellate_text_mesh_grouped(
        name,
        text,
        px_size,
        Some(row_lengths),
        FillOptions::DEFAULT,
        false,
    )
}

pub(crate) fn tessellate_text_rows_mesh_with_tolerance(
    name: &'static str,
    text: &str,
    px_size: f32,
    row_lengths: &[usize],
    tolerance: f32,
) -> FontTesselMesh {
    tessellate_text_mesh_grouped(
        name,
        text,
        px_size,
        Some(row_lengths),
        FillOptions::DEFAULT.with_tolerance(tolerance),
        false,
    )
}

pub(crate) fn tessellate_text_rows_mesh_hinted(
    name: &'static str,
    text: &str,
    px_size: f32,
    row_lengths: &[usize],
) -> FontTesselMesh {
    tessellate_text_mesh_grouped(name, text, px_size, Some(row_lengths), FillOptions::DEFAULT, true)
}

fn tessellate_text_mesh_grouped(
    name: &'static str,
    text: &str,
    px_size: f32,
    row_lengths: Option<&[usize]>,
    fill_options: FillOptions,
    hinted: bool,
) -> FontTesselMesh {
    let total_start = embassy_time_driver::now();
    match warm_embedded_font_by_name(name) {
        Ok(Some(_)) => {}
        Ok(None) => {
            return FontTesselMesh::failed(
                "font-not-registered",
                name,
                "",
                text,
                px_size,
                total_start,
            );
        }
        Err(_) => {
            return FontTesselMesh::failed(
                "font-warm-failed",
                name,
                "",
                text,
                px_size,
                total_start,
            );
        }
    }

    let registry = FONT_REGISTRY.lock();
    let Some(font_record) = registry.font_by_name(name) else {
        return FontTesselMesh::failed("font-not-registered", name, "", text, px_size, total_start);
    };
    let Some(FontWarmEndState::Outline(outline)) = font_record.outline_endstate() else {
        return FontTesselMesh::failed(
            "outline-cache-missing",
            font_record.name,
            font_record.file_name,
            text,
            px_size,
            total_start,
        );
    };
    let Ok(font) = FontRef::new(font_record.bytes.as_slice()) else {
        return FontTesselMesh::failed(
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
    let outlines = font.outline_glyphs();
    let hinting = hinted
        .then(|| {
            HintingInstance::new(
                &outlines,
                Size::new(px_size),
                LocationRef::default(),
                HintingOptions::default(),
            )
            .ok()
        })
        .flatten();
    let hinted = hinting.is_some();
    let metrics_size = if hinted {
        Size::new(px_size)
    } else {
        Size::unscaled()
    };
    let metrics = font.glyph_metrics(metrics_size, LocationRef::default());
    let scale = if hinted {
        1.0
    } else {
        px_size / (font_record.units_per_em as f32).max(1.0)
    };
    let fallback_advance = if hinted {
        px_size * 0.35
    } else {
        font_record.units_per_em as f32 * 0.35
    };
    let space_advance = charmap
        .map(' ')
        .and_then(|glyph_id| metrics.advance_width(glyph_id))
        .unwrap_or(fallback_advance)
        * scale;
    let charmap_ms = elapsed_ms_since(charmap_start);

    let mut glyphs = 0usize;
    let mut glyph_hits = 0usize;
    let mut glyph_misses = 0usize;
    let mut outline_glyphs = 0usize;
    let mut empty_glyphs = 0usize;
    let mut path_commands = 0usize;
    let mut pen_x = 0.0f32;
    let mut baseline_y = px_size;
    let mut row_index = 0usize;
    let mut chars_placed = 0usize;
    let mut next_row_at = row_lengths
        .and_then(|lengths| lengths.first().copied())
        .unwrap_or(usize::MAX);
    let mut bounds = TesselBounds::default();
    let mut path_ms = 0u64;
    let mut tessellate_ms = 0u64;
    let mut tessellate_failures = 0usize;
    let mut tessellate_error = None;
    let mut glyph_meshes: Vec<TransientGlyphMesh> = Vec::new();
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for ch in text.chars() {
        if row_lengths
            .is_some_and(|lengths| chars_placed == next_row_at && row_index + 1 < lengths.len())
        {
            pen_x = 0.0;
            baseline_y += px_size * 1.25;
            row_index += 1;
            next_row_at = next_row_at.saturating_add(
                row_lengths
                    .and_then(|lengths| lengths.get(row_index).copied())
                    .unwrap_or(0),
            );
        }
        glyphs = glyphs.saturating_add(1);
        if ch.is_whitespace() {
            pen_x += space_advance;
            chars_placed = chars_placed.saturating_add(1);
            continue;
        }
        let Some(glyph_id) = charmap.map(ch) else {
            glyph_misses = glyph_misses.saturating_add(1);
            pen_x += fallback_advance * scale;
            chars_placed = chars_placed.saturating_add(1);
            continue;
        };
        glyph_hits = glyph_hits.saturating_add(1);
        let glyph_index = glyph_id.to_u32();
        let cache_index = if let Some(index) = glyph_meshes
            .iter()
            .position(|cached| cached.glyph_index == glyph_index)
        {
            index
        } else {
            let path_start = embassy_time_driver::now();
            let mut builder = FillPath::builder_with_options(&fill_options);
            let mut glyph_bounds = TesselBounds::default();
            let mut hinted_advance = None;
            let appended = if let Some(hinting) = hinting.as_ref() {
                if let Some(glyph) = outlines.get(glyph_id) {
                    let mut pen = RasterOutlinePen::new(&mut builder, &mut glyph_bounds);
                    match glyph.draw(DrawSettings::hinted(hinting, false), &mut pen) {
                        Ok(adjusted) => {
                            hinted_advance = adjusted.advance_width;
                            pen.finish()
                        }
                        Err(_) => 0,
                    }
                } else {
                    0
                }
            } else {
                outline.append_glyph_path(
                    glyph_id,
                    &mut builder,
                    0.0,
                    0.0,
                    scale,
                    &mut glyph_bounds,
                )
            };
            let path = builder.build();
            path_ms = path_ms.saturating_add(elapsed_ms_since(path_start));

            let (glyph_vertices, glyph_indices) = if appended == 0 {
                (Vec::new(), Vec::new())
            } else {
                let tessellate_start = embassy_time_driver::now();
                let tessellated = path.tessellate(&fill_options);
                tessellate_ms = tessellate_ms.saturating_add(elapsed_ms_since(tessellate_start));
                match tessellated {
                    Ok(buffers) => (buffers.vertices, buffers.indices),
                    Err(error) => {
                        tessellate_failures = tessellate_failures.saturating_add(1);
                        tessellate_error.get_or_insert(error);
                        (Vec::new(), Vec::new())
                    }
                }
            };
            glyph_meshes.push(TransientGlyphMesh {
                glyph_index,
                path_commands: appended,
                advance_width: hinted_advance
                    .or_else(|| metrics.advance_width(glyph_id))
                    .unwrap_or(fallback_advance)
                    * scale,
                bounds: glyph_bounds,
                vertices: glyph_vertices,
                indices: glyph_indices,
            });
            glyph_meshes.len() - 1
        };

        let cached = &glyph_meshes[cache_index];
        if cached.path_commands == 0 {
            empty_glyphs = empty_glyphs.saturating_add(1);
        } else {
            outline_glyphs = outline_glyphs.saturating_add(1);
            path_commands = path_commands.saturating_add(cached.path_commands);
            if cached.bounds.has_bounds {
                bounds.include(cached.bounds.min_x + pen_x, cached.bounds.min_y + baseline_y);
                bounds.include(cached.bounds.max_x + pen_x, cached.bounds.max_y + baseline_y);
            }

            let Ok(base_index) = u32::try_from(vertices.len()) else {
                tessellate_failures = tessellate_failures.saturating_add(1);
                tessellate_error.get_or_insert(FillError::IndexOverflow);
                break;
            };
            if cached
                .indices
                .iter()
                .any(|index| base_index.checked_add(*index).is_none())
            {
                tessellate_failures = tessellate_failures.saturating_add(1);
                tessellate_error.get_or_insert(FillError::IndexOverflow);
                break;
            }
            vertices.reserve(cached.vertices.len());
            vertices.extend(
                cached
                    .vertices
                    .iter()
                    .map(|vertex| [vertex[0] + pen_x, vertex[1] + baseline_y]),
            );
            indices.reserve(cached.indices.len());
            indices.extend(cached.indices.iter().map(|index| base_index + *index));
        }
        pen_x += cached.advance_width;
        chars_placed = chars_placed.saturating_add(1);
    }
    let tessellated_ok = tessellate_failures == 0;
    if !tessellated_ok {
        vertices.clear();
        indices.clear();
    }
    let vertex_count = vertices.len();
    let index_count = indices.len();
    let vertex_bytes = vertex_count.saturating_mul(size_of::<[f32; 2]>());
    let index_bytes = index_count.saturating_mul(size_of::<u32>());
    let geometry_bytes = vertex_bytes.saturating_add(index_bytes);

    let summary = FontTesselSummary {
        status: if tessellated_ok { "ok" } else { "failed" },
        reason: tessellate_error
            .map(FillError::reason)
            .unwrap_or("tessellated"),
        text: text.into(),
        font_name: font_record.name,
        font_file: font_record.file_name,
        outline_source: if hinted {
            "skrifa-size-hinted-outline"
        } else {
            outline.name
        },
        px_size,
        glyphs,
        glyph_hits,
        glyph_misses,
        outline_glyphs,
        empty_glyphs,
        path_commands,
        tessellate_failures,
        vertices: vertex_count,
        indices: index_count,
        triangles: index_count / 3,
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
    };

    FontTesselMesh {
        summary,
        vertices,
        indices,
    }
}

pub(crate) fn warm_embedded_fonts_once() -> Result<Vec<FontWarmSummary>, skrifa::raw::ReadError> {
    let mut summaries = Vec::with_capacity(EMBEDDED_FONTS.len());
    for index in 0..EMBEDDED_FONTS.len() {
        summaries.push(warm_embedded_font_once(index)?);
    }
    Ok(summaries)
}

/// Ensure that a named font has a resident, size-independent outline cache.
///
/// Embedded faces are warmed synchronously on first use. Filesystem-backed
/// faces report unavailable until the background loader has registered them.
pub(crate) fn ensure_font_available(name: &str) -> Result<bool, skrifa::raw::ReadError> {
    Ok(warm_embedded_font_by_name(name)?.is_some())
}

fn warm_embedded_font_by_name(
    name: &str,
) -> Result<Option<FontWarmSummary>, skrifa::raw::ReadError> {
    if let Some(index) = EMBEDDED_FONTS.iter().position(|spec| spec.name == name) {
        return warm_embedded_font_once(index).map(Some);
    }
    if TRUEOSFS_FONTS.iter().any(|spec| spec.name == name) {
        return Ok(font_summary(name).map(|summary| FontWarmSummary {
            status: "warm-cache",
            ..summary
        }));
    }
    Ok(None)
}

fn warm_embedded_font_once(index: usize) -> Result<FontWarmSummary, skrifa::raw::ReadError> {
    let spec = EMBEDDED_FONTS[index];
    if let Some(summary) = font_summary(spec.name) {
        return Ok(FontWarmSummary {
            status: "warm-cache",
            ..summary
        });
    }

    warm_font_bytes_once(spec.name, spec.file_name, FontBytes::Embedded(spec.bytes))
}

fn warm_font_bytes_once(
    name: &'static str,
    file_name: &'static str,
    bytes: FontBytes,
) -> Result<FontWarmSummary, skrifa::raw::ReadError> {
    if let Some(summary) = font_summary(name) {
        return Ok(FontWarmSummary {
            status: "warm-cache",
            ..summary
        });
    }

    let total_start = embassy_time_driver::now();
    let parse_start = embassy_time_driver::now();
    let font = FontRef::new(bytes.as_slice())?;
    let head = font.head()?;
    let maxp = font.maxp()?;
    let parse_ms = elapsed_ms_since(parse_start);

    let tables = font.table_directory().table_records().len();
    let glyphs = maxp.num_glyphs();
    let units_per_em = head.units_per_em();
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
    Ok(register_warmed_font(
        name,
        file_name,
        bytes,
        tables,
        glyphs,
        units_per_em,
        outline,
        parse_ms,
        outline_ms,
        total_ms,
    ))
}

#[allow(clippy::too_many_arguments)]
fn register_warmed_font(
    name: &'static str,
    file_name: &'static str,
    bytes: FontBytes,
    tables: usize,
    glyphs: u16,
    units_per_em: u16,
    outline: FontOutlineCache,
    parse_ms: u64,
    outline_ms: u64,
    total_ms: u64,
) -> FontWarmSummary {
    let mut prepared = RegisteredFont::new(name, file_name, bytes, tables, glyphs, units_per_em);
    prepared.endstates.push(FontWarmEndState::Outline(outline));

    let summary = prepared
        .outline_endstate()
        .map(|endstate| endstate.summary(&prepared, "cold-built", parse_ms, outline_ms, total_ms))
        .unwrap_or_else(|| prepared.empty_summary("cold-built", parse_ms, outline_ms, total_ms));

    let mut registry = FONT_REGISTRY.lock();
    if let Some(existing) = registry.font_by_name(name)
        && let Some(endstate) = existing.outline_endstate()
    {
        return endstate.summary(existing, "warm-cache", 0, 0, 0);
    }
    registry.fonts.push(prepared);
    summary
}

#[embassy_executor::task]
pub(crate) async fn font_warm_task() {
    for (index, spec) in EMBEDDED_FONTS.iter().enumerate() {
        crate::log_info!(
            target: "boot";
            "graphics-font: status=warming name={} file={} warm_index={} warm_total={} warm_policy=eager-all\n",
            spec.name,
            spec.file_name,
            index + 1,
            EMBEDDED_FONTS.len(),
        );

        match warm_embedded_font_once(index) {
            Ok(summary) => crate::log_info!(
                target: "boot";
                "graphics-font: status={} name={} file={} endstate={} resident_bytes={} outline_cache_bytes={} glyphs={} success={} empty={} failures={} commands={} outline_ms={} total_ms={} warm_index={} warm_total={} warm_policy=eager-all\n",
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
                summary.total_ms,
                index + 1,
                EMBEDDED_FONTS.len(),
            ),
            Err(err) => crate::log_warn!(
                target: "boot";
                "graphics-font: status=failed name={} file={} warm_index={} warm_total={} warm_policy=eager-all err={:?}\n",
                spec.name,
                spec.file_name,
                index + 1,
                EMBEDDED_FONTS.len(),
                err,
            ),
        }
    }

    let mut heartbeat = 0u64;
    loop {
        heartbeat = heartbeat.saturating_add(1);
        if TRUEOSFS_FONTS
            .iter()
            .all(|spec| font_summary(spec.name).is_some())
        {
            return;
        }

        let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
            for spec in TRUEOSFS_FONTS
                .iter()
                .filter(|spec| font_summary(spec.name).is_none())
            {
                crate::log_info!(
                    target: "boot";
                    "graphics-font: status=waiting name={} path=trueosfs:/{} reason=root-not-mounted heartbeat={} retry_secs={}\n",
                    spec.name,
                    spec.path,
                    heartbeat,
                    TRUEOSFS_FONT_HEARTBEAT_SECS,
                );
            }
            embassy_time::Timer::after(embassy_time::Duration::from_secs(
                TRUEOSFS_FONT_HEARTBEAT_SECS,
            ))
            .await;
            continue;
        };

        for (index, spec) in TRUEOSFS_FONTS.iter().enumerate() {
            if font_summary(spec.name).is_some() {
                continue;
            }
            match crate::r::fs::trueosfs::file_out_async(disk, spec.path).await {
                Ok(Some(bytes)) => {
                    crate::log_info!(
                        target: "boot";
                        "graphics-font: status=warming name={} file={} path=trueosfs:/{} source=trueosfs resident_input_bytes={} heartbeat={} warm_index={} warm_total={} warm_policy=eager-all\n",
                        spec.name,
                        spec.file_name,
                        spec.path,
                        bytes.len(),
                        heartbeat,
                        index + 1,
                        TRUEOSFS_FONTS.len(),
                    );
                    match warm_font_bytes_once(
                        spec.name,
                        spec.file_name,
                        FontBytes::TrueosFs(bytes),
                    ) {
                        Ok(summary) => crate::log_info!(
                            target: "boot";
                            "graphics-font: status={} name={} file={} path=trueosfs:/{} source=trueosfs endstate={} resident_bytes={} outline_cache_bytes={} glyphs={} success={} empty={} failures={} commands={} outline_ms={} total_ms={} heartbeat={} warm_index={} warm_total={} warm_policy=eager-all\n",
                            summary.status,
                            summary.name,
                            summary.file_name,
                            spec.path,
                            summary.endstate,
                            summary.resident_bytes,
                            summary.cache_bytes,
                            summary.glyphs,
                            summary.outline_success,
                            summary.empty_outlines,
                            summary.outline_failures,
                            summary.commands,
                            summary.outline_ms,
                            summary.total_ms,
                            heartbeat,
                            index + 1,
                            TRUEOSFS_FONTS.len(),
                        ),
                        Err(err) => crate::log_warn!(
                            target: "boot";
                            "graphics-font: status=invalid name={} file={} path=trueosfs:/{} source=trueosfs heartbeat={} retry_secs={} warm_index={} warm_total={} err={:?}\n",
                            spec.name,
                            spec.file_name,
                            spec.path,
                            heartbeat,
                            TRUEOSFS_FONT_HEARTBEAT_SECS,
                            index + 1,
                            TRUEOSFS_FONTS.len(),
                            err,
                        ),
                    }
                }
                Ok(None) => crate::log_info!(
                    target: "boot";
                    "graphics-font: status=waiting name={} path=trueosfs:/{} reason=file-not-present heartbeat={} retry_secs={} warm_index={} warm_total={}\n",
                    spec.name,
                    spec.path,
                    heartbeat,
                    TRUEOSFS_FONT_HEARTBEAT_SECS,
                    index + 1,
                    TRUEOSFS_FONTS.len(),
                ),
                Err(err) => crate::log_warn!(
                    target: "boot";
                    "graphics-font: status=waiting name={} path=trueosfs:/{} reason=file-read-failed heartbeat={} retry_secs={} warm_index={} warm_total={} err={:?}\n",
                    spec.name,
                    spec.path,
                    heartbeat,
                    TRUEOSFS_FONT_HEARTBEAT_SECS,
                    index + 1,
                    TRUEOSFS_FONTS.len(),
                    err,
                ),
            }
        }

        if TRUEOSFS_FONTS
            .iter()
            .all(|spec| font_summary(spec.name).is_some())
        {
            return;
        }

        embassy_time::Timer::after(embassy_time::Duration::from_secs(TRUEOSFS_FONT_HEARTBEAT_SECS))
            .await;
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
    bytes: FontBytes,
    tables: usize,
    glyphs: u16,
    units_per_em: u16,
    endstates: Vec<FontWarmEndState>,
}

impl RegisteredFont {
    fn new(
        name: &'static str,
        file_name: &'static str,
        bytes: FontBytes,
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

enum FontBytes {
    Embedded(&'static [u8]),
    TrueosFs(Vec<u8>),
}

impl FontBytes {
    fn as_slice(&self) -> &[u8] {
        match self {
            Self::Embedded(bytes) => bytes,
            Self::TrueosFs(bytes) => bytes.as_slice(),
        }
    }

    fn len(&self) -> usize {
        self.as_slice().len()
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
        builder: &mut FillPathBuilder,
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

    fn append_glyph_gpu_ops(
        &self,
        glyph_id: GlyphId,
        pen_x: f32,
        output: &mut Vec<[u32; FONT_GPU_OUTLINE_OP_WORDS]>,
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
        let mut contours = 0usize;
        for op in &self.ops[start..end] {
            if matches!(op, WarmOutlineOp::MoveTo(..)) {
                contours = contours.saturating_add(1);
            }
            output.push(op.gpu_words(pen_x));
        }
        contours
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
    fn gpu_words(self, pen_x: f32) -> [u32; FONT_GPU_OUTLINE_OP_WORDS] {
        let f = |value: f32| value.to_bits();
        match self {
            WarmOutlineOp::MoveTo(x, y) => [0, f(x + pen_x), f(y), 0, 0, 0, 0, 0],
            WarmOutlineOp::LineTo(x, y) => [1, f(x + pen_x), f(y), 0, 0, 0, 0, 0],
            WarmOutlineOp::QuadTo(cx, cy, x, y) => {
                [2, f(cx + pen_x), f(cy), f(x + pen_x), f(y), 0, 0, 0]
            }
            WarmOutlineOp::CurveTo(cx0, cy0, cx1, cy1, x, y) => [
                3,
                f(cx0 + pen_x),
                f(cy0),
                f(cx1 + pen_x),
                f(cy1),
                f(x + pen_x),
                f(y),
                0,
            ],
            WarmOutlineOp::Close => [4, 0, 0, 0, 0, 0, 0, 0],
        }
    }

    fn append_to_builder(
        self,
        builder: &mut FillPathBuilder,
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

/// Receives an already-scaled hinted outline and maps its +Y-up coordinates
/// into the +Y-down space used by the fill tessellator.
struct RasterOutlinePen<'a> {
    builder: &'a mut FillPathBuilder,
    bounds: &'a mut TesselBounds,
    open: bool,
    commands: usize,
}

impl<'a> RasterOutlinePen<'a> {
    fn new(builder: &'a mut FillPathBuilder, bounds: &'a mut TesselBounds) -> Self {
        Self {
            builder,
            bounds,
            open: false,
            commands: 0,
        }
    }

    fn map(&mut self, x: f32, y: f32) -> Point {
        let mapped = point(x, -y);
        self.bounds.include(mapped.x, mapped.y);
        mapped
    }

    fn finish(mut self) -> usize {
        if self.open {
            self.builder.end(false);
            self.open = false;
        }
        self.commands
    }
}

impl OutlinePen for RasterOutlinePen<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        if self.open {
            self.builder.end(false);
        }
        let to = self.map(x, y);
        self.builder.begin(to);
        self.open = true;
        self.commands = self.commands.saturating_add(1);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let to = self.map(x, y);
        self.builder.line_to(to);
        self.commands = self.commands.saturating_add(1);
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        let control = self.map(cx0, cy0);
        let to = self.map(x, y);
        self.builder.quadratic_bezier_to(control, to);
        self.commands = self.commands.saturating_add(1);
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let control0 = self.map(cx0, cy0);
        let control1 = self.map(cx1, cy1);
        let to = self.map(x, y);
        self.builder.cubic_bezier_to(control0, control1, to);
        self.commands = self.commands.saturating_add(1);
    }

    fn close(&mut self) {
        if self.open {
            self.builder.close();
            self.open = false;
        }
        self.commands = self.commands.saturating_add(1);
    }
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
