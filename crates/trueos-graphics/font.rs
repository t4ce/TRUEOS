//! Font assets and warmed vector outline state.
//!
//! This module owns the real-font doorway for graphics. It keeps font bytes
//! plus size-independent outline commands resident, while leaving
//! tessellation, raster masks, and GPU coverage as later consumers.

use alloc::{string::String, sync::Arc, vec::Vec};
use core::{
    mem::size_of,
    sync::atomic::{AtomicBool, AtomicU8, Ordering},
};

use embassy_executor::SpawnError;
use embassy_sync::watch::Watch;
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
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const FONT_TESSEL_SAMPLE_TEXT: &str = "True OS §";
pub(crate) const FONT_TESSEL_BASE_PX: f32 = 48.0;
pub(crate) const FONT_GPU_OUTLINE_OP_WORDS: usize = 8;
// The restored render probes retain their one/two/all clip-field dispatch
// labels. Keep the original full-field vertex count at the graphics boundary.
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) const FONT_CLIP_FIELD_VERTICES: usize = 6 * 3 * 3;

const EMBEDDED_FONTS: [EmbeddedFontSpec; 2] = [
    EmbeddedFontSpec {
        name: "font",
        file_name: "L_10646.TTF",
        bytes: include_bytes!("../../tools/fnt/L_10646.TTF"),
    },
    EmbeddedFontSpec {
        name: "inconsolata",
        file_name: "Inconsolata-Regular.ttf",
        bytes: include_bytes!("../../tools/fnt/Inconsolata-Regular.ttf"),
    },
];
const TRUEOSFS_FONTS: [TrueosFsFontSpec; 1] = [TrueosFsFontSpec {
    name: "noto-sans-sc",
    file_name: "NotoSansSC[wght].ttf",
    path: "fonts/NotoSansSC[wght].ttf",
}];
const TRUEOSFS_FONT_HEARTBEAT_SECS: u64 = 30;
const FONT_WARM_POOL_SIZE: usize = 2;
const FONT_WARM_JOB_COUNT: usize = 3;
const FONT_WARM_ALL_READY: u8 = (1 << FONT_WARM_JOB_COUNT) - 1;
static FONT_REGISTRY: Mutex<FontRegistry> = Mutex::new(FontRegistry::new());
static FONT_WARM_WORKERS_ADMITTED: AtomicU8 = AtomicU8::new(0);
static FONT_WARM_READY: AtomicU8 = AtomicU8::new(0);
static FONT_WARM_READY_LOGGED: AtomicBool = AtomicBool::new(false);
// Font publication is append-only. Consumers that require a TrueOSFS face
// therefore wait for this one monotonic notification rather than treating a
// boot-order race as a failed text/frame request.
const FONT_REGISTRY_WATCH_RECEIVERS: usize = 16;
static FONT_REGISTRY_WATCH: Watch<
    crate::wait::EmbassySpinRawMutex,
    u64,
    FONT_REGISTRY_WATCH_RECEIVERS,
> = Watch::new_with(0);
static FONT_REGISTRY_EPOCH: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

#[derive(Clone, Copy)]
enum FontWarmJob {
    Embedded(usize),
    TrueosFs(usize),
}

// These are the complete, deliberately hardcoded TTF warm jobs known at boot.
const FONT_WARM_JOBS: [FontWarmJob; FONT_WARM_JOB_COUNT] = [
    FontWarmJob::Embedded(0),
    FontWarmJob::Embedded(1),
    FontWarmJob::TrueosFs(0),
];

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

#[derive(Clone, Copy, Debug)]
pub(crate) struct FontWarmSummary {
    pub(crate) status: &'static str,
    pub(crate) name: &'static str,
    pub(crate) file_name: &'static str,
    pub(crate) endstate: &'static str,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) bytes: usize,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) tables: usize,
    pub(crate) glyphs: u16,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) units_per_em: u16,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) range_bytes: usize,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) op_bytes: usize,
    pub(crate) cache_bytes: usize,
    pub(crate) resident_bytes: usize,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) range_first_glyph: u16,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) range_last_glyph: u16,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) range_max_ops: u32,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) outline_glyphs: usize,
    pub(crate) outline_success: usize,
    pub(crate) outline_failures: usize,
    pub(crate) empty_outlines: usize,
    pub(crate) commands: usize,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) move_to: usize,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) line_to: usize,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) quad_to: usize,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) curve_to: usize,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) close: usize,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) min_x: f32,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) min_y: f32,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) max_x: f32,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) max_y: f32,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) parse_ms: u64,
    pub(crate) outline_ms: u64,
    pub(crate) total_ms: u64,
}

#[derive(Clone, Copy, Debug)]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) struct FontRegistrySummary {
    pub(crate) fonts: usize,
    pub(crate) endstates: usize,
    pub(crate) resident_bytes: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct FontTesselSummary {
    pub(crate) status: &'static str,
    pub(crate) reason: &'static str,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) text: String,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) font_name: &'static str,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) font_file: &'static str,
    pub(crate) outline_source: &'static str,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) px_size: f32,
    pub(crate) glyphs: usize,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) glyph_hits: usize,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) glyph_misses: usize,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) outline_glyphs: usize,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) empty_glyphs: usize,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) path_commands: usize,
    pub(crate) tessellate_failures: usize,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) vertices: usize,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) indices: usize,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) triangles: usize,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) vertex_bytes: usize,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) index_bytes: usize,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) geometry_bytes: usize,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) min_x: f32,
    pub(crate) min_y: f32,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) max_x: f32,
    pub(crate) max_y: f32,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) charmap_ms: u64,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) path_ms: u64,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) tessellate_ms: u64,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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
    pub(crate) units_per_em: u16,
    pub(crate) ops: Vec<[u32; FONT_GPU_OUTLINE_OP_WORDS]>,
}

/// One canonical warmed glyph outline identified by the font's glyph ID.
///
/// Post-warm caches key derived raster work by `glyph_id`, rather than by the
/// Unicode scalar which happened to select it. Several scalars may resolve to
/// the same glyph and must not manufacture duplicate GPU-resident recipes.
pub(crate) struct FontGpuGlyphOutline {
    pub(crate) units_per_em: u16,
    pub(crate) glyph_id: u32,
    pub(crate) ops: Vec<[u32; FONT_GPU_OUTLINE_OP_WORDS]>,
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn default_gpu_outline() -> Result<FontGpuOutline, &'static str> {
    gpu_outline_for_text("font", FONT_TESSEL_SAMPLE_TEXT)
}

pub(crate) fn gpu_outline_for_text(
    name: &'static str,
    text: &str,
) -> Result<FontGpuOutline, &'static str> {
    // The overwhelmingly common path is already warm. Take one immutable
    // record lease so availability does not first manufacture a full
    // diagnostic summary, and release the registry lock before doing work.
    if let Some(outline) = gpu_outline_for_registered_font(name, text) {
        return outline;
    }

    match warm_embedded_font_by_name(name).map_err(|_| "font-warm-failed")? {
        Some(_) => {}
        None => return Err("font-not-registered"),
    }
    gpu_outline_for_registered_font(name, text).ok_or("font-not-registered")?
}

/// Read a GPU outline only from an already-published warm face.
///
/// Post-warm producer pools use this boundary so a missed registration can
/// never silently turn an E-core planning task into a raw TTF warm task.
pub(crate) fn gpu_outline_for_registered_text(
    name: &'static str,
    text: &str,
) -> Result<FontGpuOutline, &'static str> {
    gpu_outline_for_registered_font(name, text).ok_or("font-not-registered")?
}

/// Resolve one scalar through an already-warm face and retain the result in a
/// tiny face-local scalar map. This never warms or reparses outline tables.
pub(crate) fn gpu_glyph_id_for_registered_scalar(
    name: &'static str,
    scalar: char,
) -> Result<u32, &'static str> {
    let font_record = registered_font(name).ok_or("font-not-registered")?;
    font_record
        .glyph_id_for_scalar(scalar)?
        .map(GlyphId::to_u32)
        .ok_or("glyph-unavailable")
}

/// Copy one canonical glyph from the size-independent warm outline store.
/// Character mapping is deliberately separate so derived caches can probe by
/// glyph ID before allocating or transforming another command stream.
pub(crate) fn gpu_outline_for_registered_glyph(
    name: &'static str,
    glyph_id: u32,
) -> Result<FontGpuGlyphOutline, &'static str> {
    let font_record = registered_font(name).ok_or("font-not-registered")?;
    let FontWarmEndState::Outline(outline) = font_record
        .outline_endstate()
        .ok_or("outline-cache-missing")?;
    let glyph_id = GlyphId::new(glyph_id);
    let mut ops = Vec::new();
    outline.append_glyph_gpu_ops(glyph_id, 0.0, &mut ops);
    if ops.is_empty() {
        return Err("outline-empty");
    }
    Ok(FontGpuGlyphOutline {
        units_per_em: font_record.units_per_em,
        glyph_id: glyph_id.to_u32(),
        ops,
    })
}

fn gpu_outline_for_registered_font(
    name: &'static str,
    text: &str,
) -> Option<Result<FontGpuOutline, &'static str>> {
    let font_record = registered_font(name)?;
    Some(gpu_outline_from_registered_font(&font_record, text))
}

fn gpu_outline_from_registered_font(
    font_record: &RegisteredFont,
    text: &str,
) -> Result<FontGpuOutline, &'static str> {
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
    for ch in text.chars() {
        if ch.is_whitespace() {
            pen_x += space_advance;
            continue;
        }
        let Some(glyph_id) = charmap.map(ch) else {
            pen_x += fallback_advance;
            continue;
        };
        outline.append_glyph_gpu_ops(glyph_id, pen_x, &mut ops);
        pen_x += metrics.advance_width(glyph_id).unwrap_or(fallback_advance);
    }
    if ops.is_empty() {
        return Err("outline-empty");
    }
    Ok(FontGpuOutline {
        units_per_em: font_record.units_per_em,
        ops,
    })
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn default_font_summary() -> Option<FontWarmSummary> {
    font_summary("font")
}

pub(crate) fn font_summary(name: &str) -> Option<FontWarmSummary> {
    registered_font(name).and_then(|font| {
        font.endstates
            .iter()
            .find(|endstate| endstate.name() == FONT_ENDSTATE_OUTLINE_COMMANDS)
            .map(|endstate| endstate.summary(&font, "registered", 0, 0, 0))
    })
}

/// Report whether every non-whitespace character has a glyph in a warmed
/// face. This lets retained scenes choose a compact primary face per text run
/// while keeping a broad Unicode face as a transparent fallback.
pub(crate) fn font_supports_text(name: &str, text: &str) -> bool {
    let Some(font_record) = registered_font(name) else {
        return false;
    };
    let Ok(font) = FontRef::new(font_record.bytes.as_slice()) else {
        return false;
    };
    let charmap = font.charmap();
    text.chars()
        .all(|ch| ch.is_whitespace() || charmap.map(ch).is_some())
}

/// Measure the horizontal advance used by the unhinted tessellator without
/// constructing any glyph paths.  Document layout uses the same metrics and
/// fallback advance as `tessellate_text_mesh_grouped`, so wrapping decisions
/// remain stable when the resulting rows are uploaded as resident geometry.
pub(crate) fn text_advance_width(
    name: &'static str,
    text: &str,
    px_size: f32,
) -> Result<f32, &'static str> {
    if !px_size.is_finite() || px_size <= 0.0 {
        return Err("font-size-invalid");
    }
    match warm_embedded_font_by_name(name).map_err(|_| "font-warm-failed")? {
        Some(_) => {}
        None => return Err("font-not-registered"),
    }
    let font_record = registered_font(name).ok_or("font-not-registered")?;
    let font = FontRef::new(font_record.bytes.as_slice()).map_err(|_| "font-parse-failed")?;
    let charmap = font.charmap();
    let metrics = font.glyph_metrics(Size::unscaled(), LocationRef::default());
    let units_per_em = (font_record.units_per_em as f32).max(1.0);
    let scale = px_size / units_per_em;
    let fallback_advance = units_per_em * 0.35;
    let space_advance = charmap
        .map(' ')
        .and_then(|glyph_id| metrics.advance_width(glyph_id))
        .unwrap_or(fallback_advance);
    let mut advance = 0.0f32;
    for ch in text.chars() {
        let glyph_advance = if ch.is_whitespace() {
            space_advance
        } else {
            charmap
                .map(ch)
                .and_then(|glyph_id| metrics.advance_width(glyph_id))
                .unwrap_or(fallback_advance)
        };
        advance += glyph_advance * scale;
    }
    Ok(advance)
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn registry_summary() -> FontRegistrySummary {
    let fonts = FONT_REGISTRY.lock().fonts.clone();
    let mut endstates = 0usize;
    let mut resident_bytes = 0usize;
    for font in &fonts {
        for endstate in &font.endstates {
            endstates = endstates.saturating_add(1);
            resident_bytes = resident_bytes.saturating_add(font.bytes.len());
            resident_bytes = resident_bytes.saturating_add(endstate.cache_bytes());
        }
    }
    FontRegistrySummary {
        fonts: fonts.len(),
        endstates,
        resident_bytes,
    }
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn tessellate_default_text() -> FontTesselSummary {
    tessellate_default_text_mesh().summary
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

    let Some(font_record) = registered_font(name) else {
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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
    if registered_font(name)
        .as_deref()
        .and_then(RegisteredFont::outline_endstate)
        .is_some()
    {
        return Ok(true);
    }
    let Some(index) = EMBEDDED_FONTS.iter().position(|spec| spec.name == name) else {
        return Ok(false);
    };
    warm_embedded_font_once(index).map(|_| true)
}

/// Query only the append-only warmed registry.
///
/// UI cadence and post-warm producer paths use this so observing readiness can
/// never turn into an inline TTF load.  Raw font publication remains owned by
/// the existing warm pool (or an explicit legacy ensure call).
pub(crate) fn font_is_available(name: &str) -> bool {
    registered_font(name)
        .as_deref()
        .and_then(RegisteredFont::outline_endstate)
        .is_some()
}

/// Wait for a font's completed outline end-state to be published.
///
/// This is specifically the external-font startup contract: a request may
/// arrive after TrueOSFS is live but before its warm worker has parsed and
/// published the face. Waiting here keeps the request pending instead of
/// making the caller retry or lose its window session.
pub(crate) async fn wait_for_font_available(name: &'static str) {
    if font_is_available(name) {
        return;
    }
    let mut receiver = FONT_REGISTRY_WATCH
        .receiver()
        .expect("font registry watch receiver capacity exhausted");
    loop {
        if font_is_available(name) {
            return;
        }
        let _ = receiver.changed().await;
    }
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
    registry.fonts.push(Arc::new(prepared));
    let epoch = FONT_REGISTRY_EPOCH
        .fetch_add(1, Ordering::AcqRel)
        .saturating_add(1);
    FONT_REGISTRY_WATCH.sender().send(epoch);
    summary
}

async fn warm_trueosfs_font(job_index: usize, spec_index: usize, slot: u32) {
    let spec = TRUEOSFS_FONTS[spec_index];
    let mut heartbeat = 0u64;
    loop {
        heartbeat = heartbeat.saturating_add(1);
        if font_summary(spec.name).is_some() {
            record_font_warm_ready(job_index, slot);
            return;
        }

        if !crate::r::readiness::is_set(crate::r::readiness::TRUEOSFS_ROOT_MOUNTED) {
            crate::log_info!(
                target: "boot";
                "graphics-font: status=waiting name={} path=trueosfs:/{} reason=root-not-ready slot={} heartbeat={} retry_secs={} warm_index={} warm_total={}\n",
                spec.name,
                spec.path,
                slot,
                heartbeat,
                TRUEOSFS_FONT_HEARTBEAT_SECS,
                job_index + 1,
                FONT_WARM_JOB_COUNT,
            );
            embassy_time::Timer::after(embassy_time::Duration::from_secs(
                TRUEOSFS_FONT_HEARTBEAT_SECS,
            ))
            .await;
            continue;
        }

        let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
            crate::log_info!(
                target: "boot";
                "graphics-font: status=waiting name={} path=trueosfs:/{} reason=root-not-mounted slot={} heartbeat={} retry_secs={} warm_index={} warm_total={}\n",
                spec.name,
                spec.path,
                slot,
                heartbeat,
                TRUEOSFS_FONT_HEARTBEAT_SECS,
                job_index + 1,
                FONT_WARM_JOB_COUNT,
            );
            embassy_time::Timer::after(embassy_time::Duration::from_secs(
                TRUEOSFS_FONT_HEARTBEAT_SECS,
            ))
            .await;
            continue;
        };

        match crate::r::fs::trueosfs::file_out_async(disk, spec.path).await {
            Ok(Some(bytes)) => {
                crate::log_info!(
                    target: "boot";
                    "graphics-font: status=warming name={} file={} path=trueosfs:/{} source=trueosfs resident_input_bytes={} slot={} heartbeat={} warm_index={} warm_total={} warm_policy=eager-all\n",
                    spec.name,
                    spec.file_name,
                    spec.path,
                    bytes.len(),
                    slot,
                    heartbeat,
                    job_index + 1,
                    FONT_WARM_JOB_COUNT,
                );
                match warm_font_bytes_once(spec.name, spec.file_name, FontBytes::TrueosFs(bytes)) {
                    Ok(summary) => {
                        crate::log_info!(
                            target: "boot";
                            "graphics-font: status={} name={} file={} path=trueosfs:/{} source=trueosfs endstate={} resident_bytes={} outline_cache_bytes={} glyphs={} success={} empty={} failures={} commands={} outline_ms={} total_ms={} slot={} heartbeat={} warm_index={} warm_total={} warm_policy=eager-all\n",
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
                            slot,
                            heartbeat,
                            job_index + 1,
                            FONT_WARM_JOB_COUNT,
                        );
                        record_font_warm_ready(job_index, slot);
                        return;
                    }
                    Err(err) => crate::log_warn!(
                        target: "boot";
                        "graphics-font: status=invalid name={} file={} path=trueosfs:/{} source=trueosfs slot={} heartbeat={} retry_secs={} warm_index={} warm_total={} err={:?}\n",
                        spec.name,
                        spec.file_name,
                        spec.path,
                        slot,
                        heartbeat,
                        TRUEOSFS_FONT_HEARTBEAT_SECS,
                        job_index + 1,
                        FONT_WARM_JOB_COUNT,
                        err,
                    ),
                }
            }
            Ok(None) => crate::log_info!(
                target: "boot";
                "graphics-font: status=waiting name={} path=trueosfs:/{} reason=file-not-present slot={} heartbeat={} retry_secs={} warm_index={} warm_total={}\n",
                spec.name,
                spec.path,
                slot,
                heartbeat,
                TRUEOSFS_FONT_HEARTBEAT_SECS,
                job_index + 1,
                FONT_WARM_JOB_COUNT,
            ),
            Err(err) => crate::log_warn!(
                target: "boot";
                "graphics-font: status=waiting name={} path=trueosfs:/{} reason=file-read-failed slot={} heartbeat={} retry_secs={} warm_index={} warm_total={} err={:?}\n",
                spec.name,
                spec.path,
                slot,
                heartbeat,
                TRUEOSFS_FONT_HEARTBEAT_SECS,
                job_index + 1,
                FONT_WARM_JOB_COUNT,
                err,
            ),
        }

        embassy_time::Timer::after(embassy_time::Duration::from_secs(TRUEOSFS_FONT_HEARTBEAT_SECS))
            .await;
    }
}

fn warm_embedded_font_job(job_index: usize, spec_index: usize, slot: u32) {
    let spec = EMBEDDED_FONTS[spec_index];
    crate::log_info!(
        target: "boot";
        "graphics-font: status=warming name={} file={} source=embedded slot={} warm_index={} warm_total={} warm_policy=eager-all\n",
        spec.name,
        spec.file_name,
        slot,
        job_index + 1,
        FONT_WARM_JOB_COUNT,
    );
    match warm_embedded_font_once(spec_index) {
        Ok(summary) => {
            crate::log_info!(
                target: "boot";
                "graphics-font: status={} name={} file={} source=embedded endstate={} resident_bytes={} outline_cache_bytes={} glyphs={} success={} empty={} failures={} commands={} outline_ms={} total_ms={} slot={} warm_index={} warm_total={} warm_policy=eager-all\n",
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
                slot,
                job_index + 1,
                FONT_WARM_JOB_COUNT,
            );
            record_font_warm_ready(job_index, slot);
        }
        Err(err) => crate::log_warn!(
            target: "boot";
            "graphics-font: status=failed name={} file={} source=embedded slot={} warm_index={} warm_total={} warm_policy=eager-all err={:?}\n",
            spec.name,
            spec.file_name,
            slot,
            job_index + 1,
            FONT_WARM_JOB_COUNT,
            err,
        ),
    }
}

fn record_font_warm_ready(job_index: usize, slot: u32) {
    let ready_bit = 1u8 << job_index;
    let ready = FONT_WARM_READY.fetch_or(ready_bit, Ordering::AcqRel) | ready_bit;
    crate::log_info!(
        target: "boot";
        "graphics-font: job-ready warm_index={} warm_total={} slot={} ready_mask=0x{:02X}\n",
        job_index + 1,
        FONT_WARM_JOB_COUNT,
        slot,
        ready,
    );
    if ready == FONT_WARM_ALL_READY
        && FONT_WARM_READY_LOGGED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    {
        crate::log_info!(
            target: "boot";
            "graphics-font: resident-ready fonts={} workers={} placement=AP2+-profile-selected bsp=excluded ui_core=excluded\n",
            FONT_WARM_JOB_COUNT,
            FONT_WARM_POOL_SIZE,
        );
    }
}

#[embassy_executor::task(pool_size = FONT_WARM_POOL_SIZE)]
async fn font_warm_worker_task(worker_index: usize, expected_slot: u32, expected_kind: u8) {
    let actual_slot = crate::percpu::current_slot() as u32;
    let actual_kind = crate::workers::core_kind_for_slot(actual_slot);
    if actual_slot != expected_slot
        || actual_kind != expected_kind
        || !crate::workers::is_background_worker_slot(actual_slot)
    {
        crate::log_warn!(
            target: "boot";
            "graphics-font: worker-refused worker={} expected_slot={} actual_slot={} expected_kind={} actual_kind={} placement=AP2+-profile-selected\n",
            worker_index,
            expected_slot,
            actual_slot,
            expected_kind,
            actual_kind,
        );
        FONT_WARM_WORKERS_ADMITTED.fetch_and(!(1u8 << worker_index), Ordering::AcqRel);
        crate::r::spawn_service::retry_font_warm_pool_autostart();
        return;
    }

    crate::log_info!(
        target: "boot";
        "graphics-font: worker-online worker={} slot={} core_kind={} jobs={} placement=AP2+-profile-selected\n",
        worker_index,
        actual_slot,
        actual_kind,
        FONT_WARM_JOBS
            .iter()
            .skip(worker_index)
            .step_by(FONT_WARM_POOL_SIZE)
            .count(),
    );
    for job_index in (worker_index..FONT_WARM_JOB_COUNT).step_by(FONT_WARM_POOL_SIZE) {
        match FONT_WARM_JOBS[job_index] {
            FontWarmJob::Embedded(spec_index) => {
                warm_embedded_font_job(job_index, spec_index, actual_slot);
            }
            FontWarmJob::TrueosFs(spec_index) => {
                warm_trueosfs_font(job_index, spec_index, actual_slot).await;
            }
        }
    }
    crate::log_info!(
        target: "boot";
        "graphics-font: worker-complete worker={} slot={} core_kind={}\n",
        worker_index,
        actual_slot,
        actual_kind,
    );
}

/// Admit the two font warm workers onto the profile-selected AP2+ fleet.
///
/// Two distinct executors are used when available. On a one-worker fleet both
/// pool tasks safely share that AP; the BSP and AP1 UI core are never fallbacks.
pub(crate) fn spawn_font_warm_pool() -> Result<bool, SpawnError> {
    if !crate::workers::all_topology_spawners_registered() {
        return Ok(false);
    }
    let workers = crate::workers::pick_background_spawners_with_slots(FONT_WARM_POOL_SIZE);
    if workers.is_empty() {
        return Ok(false);
    }

    let mut admitted = 0usize;
    let mut spawned = 0usize;
    for worker_index in 0..FONT_WARM_POOL_SIZE {
        let worker_bit = 1u8 << worker_index;
        if FONT_WARM_WORKERS_ADMITTED.fetch_or(worker_bit, Ordering::AcqRel) & worker_bit != 0 {
            admitted += 1;
            continue;
        }

        let (slot, core_kind, spawner) = workers[worker_index % workers.len()];
        let token = match font_warm_worker_task(worker_index, slot, core_kind) {
            Ok(token) => token,
            Err(error) => {
                FONT_WARM_WORKERS_ADMITTED.fetch_and(!worker_bit, Ordering::AcqRel);
                if admitted == 0 && spawned == 0 {
                    return Err(error);
                }
                crate::log_warn!(
                    target: "boot";
                    "graphics-font: worker-spawn-failed worker={} slot={} core_kind={} err={:?}\n",
                    worker_index,
                    slot,
                    core_kind,
                    error,
                );
                continue;
            }
        };
        let wake_sent = spawner.spawn_and_wake_remote(token);
        crate::log_info!(
            target: "boot";
            "graphics-font: worker-spawned worker={} slot={} core_kind={} wake_sent={}\n",
            worker_index,
            slot,
            core_kind,
            wake_sent,
        );
        admitted += 1;
        spawned += 1;
    }
    crate::log_info!(
        target: "boot";
        "graphics-font: pool-admitted spawned_now={} active_or_admitted={} workers={} fonts={} selected_slots={} placement=AP2+-profile-selected\n",
        spawned,
        admitted,
        FONT_WARM_POOL_SIZE,
        FONT_WARM_JOB_COUNT,
        workers.len(),
    );
    Ok(admitted == FONT_WARM_POOL_SIZE)
}

struct FontRegistry {
    fonts: Vec<Arc<RegisteredFont>>,
}

impl FontRegistry {
    const fn new() -> Self {
        Self { fonts: Vec::new() }
    }

    fn font_by_name(&self, name: &str) -> Option<&Arc<RegisteredFont>> {
        self.fonts.iter().find(|font| font.name == name)
    }
}

/// Borrow one immutable warmed face without retaining the registry spin lock.
///
/// Registration is append-only after a face has been completely prepared, so
/// an `Arc` snapshot gives readers stable bytes and outline ranges while a
/// different warm worker publishes another face.
fn registered_font(name: &str) -> Option<Arc<RegisteredFont>> {
    let registry = FONT_REGISTRY.lock();
    registry.font_by_name(name).map(Arc::clone)
}

struct RegisteredFont {
    name: &'static str,
    file_name: &'static str,
    bytes: FontBytes,
    tables: usize,
    glyphs: u16,
    units_per_em: u16,
    endstates: Vec<FontWarmEndState>,
    /// Demand-built scalar aliases for post-warm clients. The raw warm stage
    /// remains unchanged; a face only learns mappings that consumers use.
    glyph_ids: Mutex<Vec<(char, Option<u16>)>>,
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
            glyph_ids: Mutex::new(Vec::new()),
        }
    }

    fn glyph_id_for_scalar(&self, scalar: char) -> Result<Option<GlyphId>, &'static str> {
        if let Some((_, glyph_id)) = self
            .glyph_ids
            .lock()
            .iter()
            .find(|(candidate, _)| *candidate == scalar)
        {
            return Ok(glyph_id.map(|glyph_id| GlyphId::new(u32::from(glyph_id))));
        }

        // Parsing a FontRef is a cold alias miss. Do it without holding the
        // face-local lock so independent planner workers can resolve different
        // scalars concurrently.
        let font = FontRef::new(self.bytes.as_slice()).map_err(|_| "font-parse-failed")?;
        let resolved = font
            .charmap()
            .map(scalar)
            .and_then(|glyph_id| u16::try_from(glyph_id.to_u32()).ok());
        let mut glyph_ids = self.glyph_ids.lock();
        if let Some((_, glyph_id)) = glyph_ids.iter().find(|(candidate, _)| *candidate == scalar) {
            return Ok(glyph_id.map(|glyph_id| GlyphId::new(u32::from(glyph_id))));
        }
        glyph_ids.push((scalar, resolved));
        Ok(resolved.map(|glyph_id| GlyphId::new(u32::from(glyph_id))))
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

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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
    range_max_ops: u32,
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
            range_max_ops: 0,
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
        debug_assert!(
            self.ranges
                .last()
                .is_none_or(|range| range.glyph_index < glyph_index),
            "warmed glyph ranges must remain sorted"
        );
        self.range_max_ops = self.range_max_ops.max(len);
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

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    fn cache_bytes(&self) -> usize {
        self.ranges
            .len()
            .saturating_mul(size_of::<WarmGlyphRange>())
            .saturating_add(self.ops.len().saturating_mul(size_of::<WarmOutlineOp>()))
    }

    fn glyph_range(&self, glyph_id: GlyphId) -> Option<&WarmGlyphRange> {
        let glyph_index = u16::try_from(glyph_id.to_u32()).ok()?;
        self.ranges
            .binary_search_by_key(&glyph_index, |range| range.glyph_index)
            .ok()
            .and_then(|index| self.ranges.get(index))
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
        let Some(range) = self.glyph_range(glyph_id) else {
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
    ) {
        let Some(range) = self.glyph_range(glyph_id) else {
            return;
        };
        let start = range.op_start as usize;
        let end = start
            .saturating_add(range.op_len as usize)
            .min(self.ops.len());
        if start >= end {
            return;
        }
        output.reserve(end - start);
        for op in &self.ops[start..end] {
            output.push(op.gpu_words(pen_x));
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
        let range_bytes = self
            .ranges
            .len()
            .saturating_mul(size_of::<WarmGlyphRange>());
        let op_bytes = self.ops.len().saturating_mul(size_of::<WarmOutlineOp>());
        let cache_bytes = range_bytes.saturating_add(op_bytes);
        let range_first_glyph = self.ranges.first().map_or(0, |range| range.glyph_index);
        let range_last_glyph = self.ranges.last().map_or(0, |range| range.glyph_index);
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
            range_max_ops: self.range_max_ops,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn append_test_glyph(cache: &mut FontOutlineCache, glyph_index: u16, lines: usize) {
        let start = cache.ops.len() as u32;
        let mut pen = WarmOutlinePen::default();
        pen.move_to(0.0, 0.0);
        for index in 0..lines {
            pen.line_to(index as f32 + 1.0, index as f32 + 2.0);
        }
        pen.close();
        cache.merge_pen(glyph_index, start, pen);
    }

    #[test]
    fn sparse_warm_glyph_ranges_use_exact_binary_lookup() {
        let mut cache = FontOutlineCache::new(FONT_ENDSTATE_OUTLINE_COMMANDS, 16);
        append_test_glyph(&mut cache, 2, 1);
        append_test_glyph(&mut cache, 7, 3);
        append_test_glyph(&mut cache, 13, 2);

        assert_eq!(cache.glyph_range(GlyphId::new(2)).unwrap().glyph_index, 2);
        assert_eq!(cache.glyph_range(GlyphId::new(7)).unwrap().glyph_index, 7);
        assert_eq!(cache.glyph_range(GlyphId::new(13)).unwrap().glyph_index, 13);
        assert!(cache.glyph_range(GlyphId::new(0)).is_none());
        assert!(cache.glyph_range(GlyphId::new(6)).is_none());
        assert!(
            cache
                .glyph_range(GlyphId::new(u32::from(u16::MAX) + 1))
                .is_none()
        );
    }

    #[test]
    fn warm_summary_reuses_incremental_range_aggregates() {
        let mut cache = FontOutlineCache::new(FONT_ENDSTATE_OUTLINE_COMMANDS, 16);
        append_test_glyph(&mut cache, 2, 1);
        append_test_glyph(&mut cache, 7, 4);
        append_test_glyph(&mut cache, 13, 2);
        let expected_max_ops = cache.glyph_range(GlyphId::new(7)).unwrap().op_len;

        let mut font =
            RegisteredFont::new("test", "test.ttf", FontBytes::Embedded(&[]), 0, 16, 1_000);
        font.endstates.push(FontWarmEndState::Outline(cache));
        let summary = font
            .outline_endstate()
            .unwrap()
            .summary(&font, "registered", 0, 0, 0);

        assert_eq!(summary.range_first_glyph, 2);
        assert_eq!(summary.range_last_glyph, 13);
        assert_eq!(summary.range_max_ops, expected_max_ops);
        assert_eq!(summary.commands, 13);
    }

    #[test]
    fn registered_font_arc_lease_survives_registry_mutation() {
        let mut registry = FontRegistry::new();
        registry.fonts.push(Arc::new(RegisteredFont::new(
            "leased",
            "leased.ttf",
            FontBytes::Embedded(&[]),
            0,
            1,
            1_000,
        )));

        let leased = Arc::clone(registry.font_by_name("leased").unwrap());
        registry.fonts.clear();

        assert_eq!(leased.name, "leased");
        assert_eq!(leased.units_per_em, 1_000);
    }
}
