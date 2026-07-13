//! Kernel-owned cache for reusable GPU font geometry.
//!
//! The graphics font registry owns embedded bytes and size-independent Skrifa
//! outlines. This service owns the reusable default mesh plus the one-shot
//! arbitrary-text doorway. Native size, row grouping, and eventual color are
//! draw properties; dynamic meshes are deliberately not retained.

use alloc::{string::String, sync::Arc, vec::Vec};

use spin::Mutex;

use crate::graphics::font::{FontTesselMesh, FontTesselSummary};

pub(crate) const MAX_DYNAMIC_TEXT_CHARS: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GpuFontTextLayout {
    SingleLine,
    Rows,
}

impl GpuFontTextLayout {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::SingleLine => "single-line",
            Self::Rows => "rows",
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum GpuFontTextRequest<'a> {
    SingleLine(&'a str),
    Rows(&'a [&'a str]),
}

/// One positioned text group in a font job.
///
/// Positions are expressed in the shared base-font coordinate space: +X is
/// right and +Y is down. The complete job bounds are fitted to the native
/// target once, so relative placement is preserved across every entry.
#[derive(Clone, Copy)]
pub(crate) struct GpuFontJobEntry<'a> {
    pub(crate) text: GpuFontTextRequest<'a>,
    pub(crate) position: [f32; 2],
}

pub(crate) struct GpuFontJob<'a> {
    pub(crate) entries: &'a [GpuFontJobEntry<'a>],
    pub(crate) native_scale: u32,
}

#[derive(Clone, Debug)]
pub(crate) struct GpuFontWarmResult {
    pub(crate) cache_hit: bool,
    pub(crate) generation: u64,
    pub(crate) font_name: &'static str,
    pub(crate) font_file: &'static str,
    pub(crate) text: String,
    pub(crate) base_px: f32,
    pub(crate) vertices: usize,
    pub(crate) indices: usize,
    pub(crate) geometry_bytes: usize,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct GpuFontCacheStatus {
    pub(crate) ready: bool,
    pub(crate) generation: u64,
    pub(crate) warm_requests: u64,
    pub(crate) cache_hits: u64,
    pub(crate) cache_misses: u64,
    pub(crate) build_failures: u64,
    pub(crate) invalidations: u64,
    pub(crate) geometry_bytes: usize,
}

/// Borrowed, uncolored fill geometry suitable for an indexed GPU draw.
///
/// The coordinates use the cached base size only as a tessellation-quality
/// reference. Consumers should transform them at draw time rather than create
/// a cache entry for every requested font size.
pub(crate) struct GpuFontGeometry<'a> {
    pub(crate) summary: &'a FontTesselSummary,
    pub(crate) vertices: &'a [[f32; 2]],
    pub(crate) indices: &'a [u32],
    pub(crate) bounds: (f32, f32, f32, f32),
}

pub(crate) struct GpuFontTextRender {
    pub(crate) summary: FontTesselSummary,
    pub(crate) render: crate::intel::render::RenderJokerResult,
    pub(crate) layout: GpuFontTextLayout,
    pub(crate) text_chars: usize,
    pub(crate) rows: usize,
}

pub(crate) struct GpuFontJobRender {
    pub(crate) summaries: Vec<FontTesselSummary>,
    pub(crate) render: crate::intel::render::RenderJokerResult,
    pub(crate) entries: usize,
    pub(crate) text_chars: usize,
    pub(crate) rows: usize,
    pub(crate) glyphs: usize,
    pub(crate) vertices: usize,
    pub(crate) indices: usize,
}

struct CachedGpuFont {
    generation: u64,
    mesh: FontTesselMesh,
}

struct KernelGpuFontService {
    default_font: Option<Arc<CachedGpuFont>>,
    generation: u64,
    warm_requests: u64,
    cache_hits: u64,
    cache_misses: u64,
    build_failures: u64,
    invalidations: u64,
}

impl KernelGpuFontService {
    const fn new() -> Self {
        Self {
            default_font: None,
            generation: 0,
            warm_requests: 0,
            cache_hits: 0,
            cache_misses: 0,
            build_failures: 0,
            invalidations: 0,
        }
    }
}

static GPU_FONT_SERVICE: Mutex<KernelGpuFontService> =
    Mutex::new(KernelGpuFontService::new());

fn acquire_default_font() -> Result<(Arc<CachedGpuFont>, bool), &'static str> {
    // Keep the lock during the first build. It is a one-time boot operation,
    // and doing so guarantees that concurrent first users cannot tessellate the
    // same font twice. The returned Arc lets all later users drop the lock.
    let mut service = GPU_FONT_SERVICE.lock();
    service.warm_requests = service.warm_requests.saturating_add(1);
    if let Some(cached) = service.default_font.as_ref().map(Arc::clone) {
        service.cache_hits = service.cache_hits.saturating_add(1);
        return Ok((cached, true));
    }

    service.cache_misses = service.cache_misses.saturating_add(1);
    let mesh = crate::graphics::font::tessellate_default_text_mesh();
    if mesh.summary.status != "ok"
        || mesh.summary.tessellate_failures != 0
        || mesh.vertices.is_empty()
        || mesh.indices.is_empty()
        || !mesh.indices.len().is_multiple_of(3)
    {
        service.build_failures = service.build_failures.saturating_add(1);
        crate::log_error!(
            target: "render";
            "intel/gpu-font: warm failed reason={} font={} file={} text=\"{}\"\n",
            mesh.summary.reason,
            mesh.summary.font_name,
            mesh.summary.font_file,
            mesh.summary.text,
        );
        return Err(mesh.summary.reason);
    }

    service.generation = service.generation.saturating_add(1).max(1);
    let cached = Arc::new(CachedGpuFont {
        generation: service.generation,
        mesh,
    });
    crate::log_info!(
        target: "render";
        "intel/gpu-font: warm ok=1 cache_hit=0 generation={} font={} file={} text=\"{}\" base_px={} vertices={} indices={} geometry_bytes={} coverage=uncolored-vector-fill size_policy=draw-time\n",
        cached.generation,
        cached.mesh.summary.font_name,
        cached.mesh.summary.font_file,
        cached.mesh.summary.text,
        cached.mesh.summary.px_size as u32,
        cached.mesh.summary.vertices,
        cached.mesh.summary.indices,
        cached.mesh.summary.geometry_bytes,
    );
    service.default_font = Some(Arc::clone(&cached));
    Ok((cached, false))
}

/// Warm the embedded font and its default GPU-ready mesh exactly once.
///
/// This is safe to call both during boot and lazily from a first consumer.
pub(crate) fn warm_default_font_once() -> Result<GpuFontWarmResult, &'static str> {
    let (cached, cache_hit) = acquire_default_font()?;
    let summary = cached.mesh.summary.clone();
    Ok(GpuFontWarmResult {
        cache_hit,
        generation: cached.generation,
        font_name: summary.font_name,
        font_file: summary.font_file,
        text: summary.text,
        base_px: summary.px_size,
        vertices: summary.vertices,
        indices: summary.indices,
        geometry_bytes: summary.geometry_bytes,
    })
}

/// Use the cached base mesh without copying its vertex or index buffers.
pub(crate) fn with_default_font_geometry<R>(
    use_geometry: impl FnOnce(GpuFontGeometry<'_>) -> R,
) -> Result<R, &'static str> {
    let (cached, _) = acquire_default_font()?;
    let summary = &cached.mesh.summary;
    let bounds = (summary.min_x, summary.min_y, summary.max_x, summary.max_y);
    Ok(use_geometry(GpuFontGeometry {
        summary,
        vertices: cached.mesh.vertices.as_slice(),
        indices: cached.mesh.indices.as_slice(),
        bounds,
    }))
}

/// Convenient current consumer: draw the cached geometry at a native size.
///
/// The scale changes the render target and viewport, not the cached geometry.
/// Color remains a render-state concern and is deliberately absent here.
pub(crate) fn render_default_font(
    native_scale: u32,
) -> Result<crate::intel::render::RenderJokerResult, &'static str> {
    with_default_font_geometry(|geometry| {
        crate::intel::render::submit_font_mesh_once_scaled(
            geometry.vertices,
            geometry.indices,
            geometry.bounds,
            native_scale,
        )
    })?
}

/// Tessellate one caller-provided string from the warmed outline registry,
/// submit it immediately, and drop the invocation-specific mesh afterwards.
pub(crate) fn render_text_once(
    request: GpuFontTextRequest<'_>,
    native_scale: u32,
) -> Result<GpuFontTextRender, &'static str> {
    let layout = match request {
        GpuFontTextRequest::SingleLine(_) => GpuFontTextLayout::SingleLine,
        GpuFontTextRequest::Rows(_) => GpuFontTextLayout::Rows,
    };
    let entry = GpuFontJobEntry {
        text: request,
        position: [0.0, 0.0],
    };
    let job = render_font_job_once(GpuFontJob {
        entries: core::slice::from_ref(&entry),
        native_scale,
    })?;
    let mut summaries = job.summaries;
    let summary = summaries.pop().ok_or("font-job-summary")?;
    Ok(GpuFontTextRender {
        summary,
        render: job.render,
        layout,
        text_chars: job.text_chars,
        rows: job.rows,
    })
}

/// Build all positioned text groups into one mesh and issue one indexed draw.
///
/// Each entry retains the 256-character text-request limit. A job has no
/// aggregate character cap, allowing callers to compose many independently
/// positioned lines/row groups without multiplying GPU submissions.
pub(crate) fn render_font_job_once(
    job: GpuFontJob<'_>,
) -> Result<GpuFontJobRender, &'static str> {
    if job.entries.is_empty() {
        return Err("font-job-empty");
    }

    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let mut summaries = Vec::with_capacity(job.entries.len());
    let mut bounds: Option<(f32, f32, f32, f32)> = None;
    let mut text_chars = 0usize;
    let mut rows = 0usize;
    let mut glyphs = 0usize;

    for entry in job.entries {
        if !entry.position[0].is_finite() || !entry.position[1].is_finite() {
            return Err("font-job-position");
        }
        let (mesh, entry_chars, entry_rows) = tessellate_text_request(entry.text)?;
        if vertices.len() > u32::MAX as usize {
            return Err("font-job-vertex-range");
        }
        let base_index = vertices.len() as u32;
        let next_vertex_len = vertices
            .len()
            .checked_add(mesh.vertices.len())
            .ok_or("font-job-vertex-overflow")?;
        if next_vertex_len > u32::MAX as usize {
            return Err("font-job-vertex-range");
        }
        vertices.reserve(mesh.vertices.len());
        for vertex in &mesh.vertices {
            vertices.push([
                vertex[0] + entry.position[0],
                vertex[1] + entry.position[1],
            ]);
        }
        indices.reserve(mesh.indices.len());
        for index in &mesh.indices {
            indices.push(
                base_index
                    .checked_add(*index)
                    .ok_or("font-job-index-overflow")?,
            );
        }

        let entry_bounds = (
            mesh.summary.min_x + entry.position[0],
            mesh.summary.min_y + entry.position[1],
            mesh.summary.max_x + entry.position[0],
            mesh.summary.max_y + entry.position[1],
        );
        bounds = Some(match bounds {
            Some((min_x, min_y, max_x, max_y)) => (
                min_x.min(entry_bounds.0),
                min_y.min(entry_bounds.1),
                max_x.max(entry_bounds.2),
                max_y.max(entry_bounds.3),
            ),
            None => entry_bounds,
        });
        text_chars = text_chars.saturating_add(entry_chars);
        rows = rows.saturating_add(entry_rows);
        glyphs = glyphs.saturating_add(mesh.summary.glyphs);
        summaries.push(mesh.summary);
    }

    let bounds = bounds.ok_or("font-job-bounds")?;
    let render = crate::intel::render::submit_font_mesh_once_scaled(
        vertices.as_slice(),
        indices.as_slice(),
        bounds,
        job.native_scale,
    )?;
    crate::log_info!(
        target: "render";
        "intel/gpu-font: job-render ok=1 entries={} text_chars={} rows={} native_scale={} vertices={} indices={} submits=1 mesh_cache=none\n",
        job.entries.len(),
        text_chars,
        rows,
        job.native_scale,
        vertices.len(),
        indices.len(),
    );
    Ok(GpuFontJobRender {
        summaries,
        render,
        entries: job.entries.len(),
        text_chars,
        rows,
        glyphs,
        vertices: vertices.len(),
        indices: indices.len(),
    })
}

fn tessellate_text_request(
    request: GpuFontTextRequest<'_>,
) -> Result<(FontTesselMesh, usize, usize), &'static str> {
    let (layout, normalized, row_lengths) = normalize_text_request(request)?;
    let char_count = normalized.chars().count();
    if normalized.trim().is_empty() {
        return Err("text-empty");
    }
    if char_count > MAX_DYNAMIC_TEXT_CHARS {
        return Err("text-too-long");
    }
    let rows = row_lengths.len();
    let mesh = match layout {
        GpuFontTextLayout::SingleLine => crate::graphics::font::tessellate_text_mesh(
            "font",
            normalized.as_str(),
            crate::graphics::font::FONT_TESSEL_BASE_PX,
        ),
        GpuFontTextLayout::Rows => crate::graphics::font::tessellate_text_rows_mesh(
            "font",
            normalized.as_str(),
            crate::graphics::font::FONT_TESSEL_BASE_PX,
            row_lengths.as_slice(),
        ),
    };
    if mesh.summary.status != "ok"
        || mesh.summary.tessellate_failures != 0
        || mesh.vertices.is_empty()
        || mesh.indices.is_empty()
        || !mesh.indices.len().is_multiple_of(3)
    {
        return Err(mesh.summary.reason);
    }
    Ok((mesh, char_count, rows))
}

fn normalize_text_request(
    request: GpuFontTextRequest<'_>,
) -> Result<(GpuFontTextLayout, String, Vec<usize>), &'static str> {
    let single_row;
    let (layout, rows): (GpuFontTextLayout, &[&str]) = match request {
        GpuFontTextRequest::SingleLine(text) => {
            single_row = [text];
            (GpuFontTextLayout::SingleLine, &single_row)
        }
        GpuFontTextRequest::Rows(rows) => {
            if rows.is_empty() {
                return Err("rows-empty");
            }
            (GpuFontTextLayout::Rows, rows)
        }
    };

    let capacity = rows
        .iter()
        .fold(0usize, |total, row| total.saturating_add(row.len()));
    let mut normalized = String::with_capacity(capacity);
    let mut row_lengths = Vec::with_capacity(rows.len());
    for row in rows {
        let row_start = normalized.chars().count();
        for ch in row.chars() {
            if is_line_separator(ch) {
                continue;
            }
            if ch.is_control() {
                return Err("text-control-character");
            }
            normalized.push(ch);
        }
        let row_len = normalized.chars().count().saturating_sub(row_start);
        if row_len == 0 {
            return Err("row-empty");
        }
        row_lengths.push(row_len);
    }
    Ok((layout, normalized, row_lengths))
}

const fn is_line_separator(ch: char) -> bool {
    matches!(
        ch,
        '\n' | '\r' | '\u{000B}' | '\u{000C}' | '\u{0085}' | '\u{2028}' | '\u{2029}'
    )
}

pub(crate) fn cached_default_font_summary() -> Option<FontTesselSummary> {
    GPU_FONT_SERVICE
        .lock()
        .default_font
        .as_ref()
        .map(|cached| cached.mesh.summary.clone())
}

pub(crate) fn cache_status() -> GpuFontCacheStatus {
    let service = GPU_FONT_SERVICE.lock();
    GpuFontCacheStatus {
        ready: service.default_font.is_some(),
        generation: service.generation,
        warm_requests: service.warm_requests,
        cache_hits: service.cache_hits,
        cache_misses: service.cache_misses,
        build_failures: service.build_failures,
        invalidations: service.invalidations,
        geometry_bytes: service
            .default_font
            .as_ref()
            .map(|cached| cached.mesh.summary.geometry_bytes)
            .unwrap_or(0),
    }
}

/// Invalidate only geometry derived from `font_name`.
///
/// A future external-font loader should call this after replacing a registered
/// font. Existing draws remain safe because active users retain an Arc.
pub(crate) fn invalidate_font(font_name: &str, reason: &str) -> bool {
    let mut service = GPU_FONT_SERVICE.lock();
    let matches = service
        .default_font
        .as_ref()
        .is_some_and(|cached| cached.mesh.summary.font_name == font_name);
    if !matches {
        return false;
    }
    service.default_font = None;
    service.invalidations = service.invalidations.saturating_add(1);
    crate::log_info!(
        target: "render";
        "intel/gpu-font: invalidate font={} reason={} invalidations={}\n",
        font_name,
        reason,
        service.invalidations,
    );
    true
}

/// Drop every geometry entry after the underlying font registry changes.
///
/// Use this for font replacement because the new font may not have the same
/// name as the entry currently cached here.
pub(crate) fn invalidate_all(reason: &str) -> bool {
    let mut service = GPU_FONT_SERVICE.lock();
    let Some(cached) = service.default_font.take() else {
        return false;
    };
    service.invalidations = service.invalidations.saturating_add(1);
    crate::log_info!(
        target: "render";
        "intel/gpu-font: invalidate-all previous_font={} reason={} invalidations={}\n",
        cached.mesh.summary.font_name,
        reason,
        service.invalidations,
    );
    true
}

/// Rebuild after changed font data or a future tessellation-policy change.
pub(crate) fn rebuild_default_font(reason: &str) -> Result<GpuFontWarmResult, &'static str> {
    let _ = invalidate_all(reason);
    warm_default_font_once()
}
