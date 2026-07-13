//! Kernel-owned cache for reusable GPU font geometry.
//!
//! The graphics font registry owns embedded bytes and size-independent Skrifa
//! outlines. This service owns the reusable default mesh, one-shot arbitrary
//! text jobs, and explicitly tagged persistent jobs. A persistent-job lease
//! transfers its prepared geometry from CPU-build authority to a dedicated
//! render-PPGTT allocation; later draws borrow that allocation without another
//! geometry upload. Native size, row grouping, and eventual color remain draw
//! properties.

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

/// Stable audit identity for one kernel-owned resident font job.
///
/// Tags are static deliberately: resident allocations must have a named kernel
/// owner and purpose rather than inheriting arbitrary input text as identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GpuFontResidencyTag {
    owner: &'static str,
    name: &'static str,
}

impl GpuFontResidencyTag {
    pub(crate) const fn new(owner: &'static str, name: &'static str) -> Self {
        Self { owner, name }
    }

    pub(crate) const fn owner(self) -> &'static str {
        self.owner
    }

    pub(crate) const fn name(self) -> &'static str {
        self.name
    }
}

/// Non-copyable authority lease for a persistent GPU font job.
///
/// The service registry owns the actual DMA pages. This lease can only borrow
/// them for synchronous submission, and dropping it requests an unmap-then-free
/// release. An uncertain GPU retirement quarantines the registry entry instead
/// of freeing memory that hardware could still reference.
pub(crate) struct PersistentGpuFontJob {
    id: u64,
    generation: u64,
    tag: GpuFontResidencyTag,
    released: bool,
}

impl PersistentGpuFontJob {
    pub(crate) const fn id(&self) -> u64 {
        self.id
    }

    pub(crate) const fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) const fn tag(&self) -> GpuFontResidencyTag {
        self.tag
    }

    pub(crate) fn submit(
        &self,
    ) -> Result<crate::intel::render::RenderJokerResult, &'static str> {
        submit_persistent_font_job(self)
    }

    /// Reuse the same resident geometry at another supported native size.
    pub(crate) fn submit_at_scale(
        &self,
        native_scale: u32,
    ) -> Result<crate::intel::render::RenderJokerResult, &'static str> {
        submit_persistent_font_job_at_scale(self, native_scale)
    }

    pub(crate) fn release(mut self) -> Result<(), &'static str> {
        if self.released {
            return Err("resident-lease-released");
        }
        self.released = true;
        release_persistent_font_job(self.id, self.generation, self.tag)
    }
}

impl Drop for PersistentGpuFontJob {
    fn drop(&mut self) {
        if !self.released {
            self.released = true;
            let _ = release_persistent_font_job(self.id, self.generation, self.tag);
        }
    }
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

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct GpuFontResidentStatus {
    pub(crate) active_jobs: usize,
    pub(crate) resident_bytes: usize,
    pub(crate) quarantined_jobs: usize,
    pub(crate) uploads: u64,
    pub(crate) submit_attempts: u64,
    pub(crate) retired_submits: u64,
    pub(crate) releases: u64,
    pub(crate) release_failures: u64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct GpuFontResidentAuditEntry {
    pub(crate) id: u64,
    pub(crate) generation: u64,
    pub(crate) tag: GpuFontResidencyTag,
    pub(crate) gpu_base: u64,
    pub(crate) resident_bytes: usize,
    pub(crate) entries: usize,
    pub(crate) text_chars: usize,
    pub(crate) rows: usize,
    pub(crate) glyphs: usize,
    pub(crate) submits: u64,
    pub(crate) in_flight: bool,
    pub(crate) quarantined: bool,
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

struct BuiltGpuFontJob {
    summaries: Vec<FontTesselSummary>,
    vertices: Vec<[f32; 2]>,
    indices: Vec<u32>,
    bounds: (f32, f32, f32, f32),
    entries: usize,
    text_chars: usize,
    rows: usize,
    glyphs: usize,
}

struct CachedGpuFont {
    generation: u64,
    mesh: FontTesselMesh,
}

struct ResidentGpuFontJobRecord {
    id: u64,
    generation: u64,
    tag: GpuFontResidencyTag,
    mesh: crate::intel::render::ResidentFontMesh,
    native_scale: u32,
    entries: usize,
    text_chars: usize,
    rows: usize,
    glyphs: usize,
    submits: u64,
    in_flight: bool,
    quarantined: bool,
}

struct KernelGpuFontService {
    default_font: Option<Arc<CachedGpuFont>>,
    generation: u64,
    warm_requests: u64,
    cache_hits: u64,
    cache_misses: u64,
    build_failures: u64,
    invalidations: u64,
    resident_generation: u64,
    next_resident_id: u64,
    resident_jobs: Vec<ResidentGpuFontJobRecord>,
    resident_uploads: u64,
    resident_submit_attempts: u64,
    resident_retired_submits: u64,
    resident_releases: u64,
    resident_release_failures: u64,
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
            resident_generation: 0,
            next_resident_id: 1,
            resident_jobs: Vec::new(),
            resident_uploads: 0,
            resident_submit_attempts: 0,
            resident_retired_submits: 0,
            resident_releases: 0,
            resident_release_failures: 0,
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
    let native_scale = job.native_scale;
    let built = build_font_job_mesh(job.entries)?;
    let render = crate::intel::render::submit_font_mesh_once_scaled(
        built.vertices.as_slice(),
        built.indices.as_slice(),
        built.bounds,
        native_scale,
    )?;
    crate::log_info!(
        target: "render";
        "intel/gpu-font: job-render ok=1 entries={} text_chars={} rows={} native_scale={} vertices={} indices={} submits=1 mesh_cache=none\n",
        built.entries,
        built.text_chars,
        built.rows,
        native_scale,
        built.vertices.len(),
        built.indices.len(),
    );
    Ok(GpuFontJobRender {
        summaries: built.summaries,
        render,
        entries: built.entries,
        text_chars: built.text_chars,
        rows: built.rows,
        glyphs: built.glyphs,
        vertices: built.vertices.len(),
        indices: built.indices.len(),
    })
}

fn build_font_job_mesh(
    entries: &[GpuFontJobEntry<'_>],
) -> Result<BuiltGpuFontJob, &'static str> {
    if entries.is_empty() {
        return Err("font-job-empty");
    }
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let mut summaries = Vec::with_capacity(entries.len());
    let mut bounds: Option<(f32, f32, f32, f32)> = None;
    let mut text_chars = 0usize;
    let mut rows = 0usize;
    let mut glyphs = 0usize;

    for entry in entries {
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

    Ok(BuiltGpuFontJob {
        summaries,
        vertices,
        indices,
        bounds: bounds.ok_or("font-job-bounds")?,
        entries: entries.len(),
        text_chars,
        rows,
        glyphs,
    })
}

/// Prepare a font job once and retain its final indexed geometry in dedicated
/// render-PPGTT pages until the returned authority lease is released.
///
/// `(owner, name)` must be unique among live jobs. This keeps every resident
/// allocation attributable and prevents accidental replacement from orphaning
/// the old GPU mapping.
pub(crate) fn persist_font_job(
    tag: GpuFontResidencyTag,
    job: GpuFontJob<'_>,
) -> Result<PersistentGpuFontJob, &'static str> {
    if tag.owner.trim().is_empty() || tag.name.trim().is_empty() {
        return Err("resident-tag-empty");
    }
    if !crate::intel::render::font_native_scale_supported(job.native_scale) {
        return Err("font-native-scale-range");
    }
    // Serialize the full create transaction. Once pages are mapped there is no
    // fallible ownership handoff: this registry immediately receives them.
    let mut service = GPU_FONT_SERVICE.lock();
    if service.resident_jobs.iter().any(|record| record.tag == tag) {
        return Err("resident-tag-in-use");
    }
    let id = service.next_resident_id;
    let Some(next_id) = id.checked_add(1) else {
        return Err("resident-id-exhausted");
    };
    service.next_resident_id = next_id;
    service.resident_generation = service.resident_generation.saturating_add(1).max(1);
    let generation = service.resident_generation;

    let native_scale = job.native_scale;
    let built = build_font_job_mesh(job.entries)?;
    let mesh = crate::intel::render::create_resident_font_mesh(
        built.vertices.as_slice(),
        built.indices.as_slice(),
        built.bounds,
    )?;
    let resident_bytes = mesh.storage_bytes;
    let gpu_base = mesh.gpu_base;
    service.resident_jobs.push(ResidentGpuFontJobRecord {
        id,
        generation,
        tag,
        mesh,
        native_scale,
        entries: built.entries,
        text_chars: built.text_chars,
        rows: built.rows,
        glyphs: built.glyphs,
        submits: 0,
        in_flight: false,
        quarantined: false,
    });
    service.resident_uploads = service.resident_uploads.saturating_add(1);
    crate::log_info!(
        target: "render";
        "intel/gpu-font: resident-create ok=1 id={} generation={} owner={} name={} authority=cpu-build->gpu-resident entries={} text_chars={} rows={} glyphs={} vertices={} indices={} native_scale={} gpu=0x{:X} bytes=0x{:X} geometry_uploads=1\n",
        id,
        generation,
        tag.owner,
        tag.name,
        built.entries,
        built.text_chars,
        built.rows,
        built.glyphs,
        built.vertices.len(),
        built.indices.len(),
        native_scale,
        gpu_base,
        resident_bytes,
    );
    Ok(PersistentGpuFontJob {
        id,
        generation,
        tag,
        released: false,
    })
}

/// Reuse a persistent job's resident VB/IB directly for one synchronous draw.
pub(crate) fn submit_persistent_font_job(
    lease: &PersistentGpuFontJob,
) -> Result<crate::intel::render::RenderJokerResult, &'static str> {
    submit_persistent_font_job_inner(lease, None)
}

pub(crate) fn submit_persistent_font_job_at_scale(
    lease: &PersistentGpuFontJob,
    native_scale: u32,
) -> Result<crate::intel::render::RenderJokerResult, &'static str> {
    if !crate::intel::render::font_native_scale_supported(native_scale) {
        return Err("font-native-scale-range");
    }
    submit_persistent_font_job_inner(lease, Some(native_scale))
}

fn submit_persistent_font_job_inner(
    lease: &PersistentGpuFontJob,
    native_scale_override: Option<u32>,
) -> Result<crate::intel::render::RenderJokerResult, &'static str> {
    if lease.released {
        return Err("resident-lease-released");
    }

    // The lock deliberately remains held through synchronous submission. It
    // makes release and submission mutually exclusive without making the
    // physical allocation reference-counted or exposing a CPU pointer.
    let mut service = GPU_FONT_SERVICE.lock();
    let Some(position) = service.resident_jobs.iter().position(|record| {
        record.id == lease.id
            && record.generation == lease.generation
            && record.tag == lease.tag
    }) else {
        return Err("resident-lease-stale");
    };
    {
        let record = &mut service.resident_jobs[position];
        if record.quarantined {
            return Err("resident-job-quarantined");
        }
        if record.in_flight {
            return Err("resident-job-in-flight");
        }
        record.in_flight = true;
        record.submits = record.submits.saturating_add(1);
    }
    service.resident_submit_attempts = service.resident_submit_attempts.saturating_add(1);

    let native_scale = native_scale_override
        .unwrap_or(service.resident_jobs[position].native_scale);
    let result = {
        let record = &service.resident_jobs[position];
        crate::intel::render::submit_resident_font_mesh_once(
            &record.mesh,
            native_scale,
        )
    };
    let completed = result.as_ref().is_ok_and(|render| render.completed);
    let (submit_count, gpu_base, resident_bytes) = {
        let record = &mut service.resident_jobs[position];
        record.in_flight = false;
        if result.as_ref().is_ok_and(|render| !render.completed) {
            // A timeout is not permission to free pages potentially still
            // referenced by the engine. Keep them tracked and non-reusable.
            record.quarantined = true;
        }
        (record.submits, record.mesh.gpu_base, record.mesh.storage_bytes)
    };
    if completed {
        service.resident_retired_submits = service.resident_retired_submits.saturating_add(1);
    }
    crate::log_info!(
        target: "render";
        "intel/gpu-font: resident-submit id={} generation={} owner={} name={} authority=borrowed-gpu-resident cpu_geometry_copy=0 geometry_uploads=0 attempt={} native_scale={} result={} retired={} quarantined={} gpu=0x{:X} bytes=0x{:X}\n",
        lease.id,
        lease.generation,
        lease.tag.owner,
        lease.tag.name,
        submit_count,
        native_scale,
        if result.is_ok() { "draw-returned" } else { "pre-submit-error" },
        completed as u8,
        (result.is_ok() && !completed) as u8,
        gpu_base,
        resident_bytes,
    );
    result
}

fn release_persistent_font_job(
    id: u64,
    generation: u64,
    tag: GpuFontResidencyTag,
) -> Result<(), &'static str> {
    let mut service = GPU_FONT_SERVICE.lock();
    let Some(position) = service.resident_jobs.iter().position(|record| {
        record.id == id && record.generation == generation && record.tag == tag
    }) else {
        return Err("resident-lease-stale");
    };
    if service.resident_jobs[position].in_flight {
        service.resident_release_failures = service.resident_release_failures.saturating_add(1);
        return Err("resident-job-in-flight");
    }
    if service.resident_jobs[position].quarantined {
        service.resident_release_failures = service.resident_release_failures.saturating_add(1);
        crate::log_error!(
            target: "render";
            "intel/gpu-font: resident-release refused id={} generation={} owner={} name={} reason=retirement-uncertain authority=gpu-quarantine tracked=1\n",
            id,
            generation,
            tag.owner,
            tag.name,
        );
        return Err("resident-job-quarantined");
    }

    let gpu_base = service.resident_jobs[position].mesh.gpu_base;
    let resident_bytes = service.resident_jobs[position].mesh.storage_bytes;
    if !crate::intel::render::release_resident_font_mesh(
        &service.resident_jobs[position].mesh,
    ) {
        service.resident_jobs[position].quarantined = true;
        service.resident_release_failures = service.resident_release_failures.saturating_add(1);
        crate::log_error!(
            target: "render";
            "intel/gpu-font: resident-release failed id={} generation={} owner={} name={} reason=ppgtt-unmap authority=gpu-quarantine tracked=1 gpu=0x{:X} bytes=0x{:X}\n",
            id,
            generation,
            tag.owner,
            tag.name,
            gpu_base,
            resident_bytes,
        );
        return Err("resident-unmap-failed");
    }

    let record = service.resident_jobs.swap_remove(position);
    service.resident_releases = service.resident_releases.saturating_add(1);
    crate::log_info!(
        target: "render";
        "intel/gpu-font: resident-release ok=1 id={} generation={} owner={} name={} authority=gpu-resident->unmapped->freed submits={} gpu=0x{:X} bytes=0x{:X} tracked=0\n",
        id,
        generation,
        tag.owner,
        tag.name,
        record.submits,
        gpu_base,
        resident_bytes,
    );
    Ok(())
}

pub(crate) fn resident_status() -> GpuFontResidentStatus {
    let service = GPU_FONT_SERVICE.lock();
    GpuFontResidentStatus {
        active_jobs: service.resident_jobs.len(),
        resident_bytes: service
            .resident_jobs
            .iter()
            .fold(0usize, |total, record| {
                total.saturating_add(record.mesh.storage_bytes)
            }),
        quarantined_jobs: service
            .resident_jobs
            .iter()
            .filter(|record| record.quarantined)
            .count(),
        uploads: service.resident_uploads,
        submit_attempts: service.resident_submit_attempts,
        retired_submits: service.resident_retired_submits,
        releases: service.resident_releases,
        release_failures: service.resident_release_failures,
    }
}

/// Snapshot every live allocation together with its accountable owner tag.
pub(crate) fn resident_audit() -> Vec<GpuFontResidentAuditEntry> {
    GPU_FONT_SERVICE
        .lock()
        .resident_jobs
        .iter()
        .map(|record| GpuFontResidentAuditEntry {
            id: record.id,
            generation: record.generation,
            tag: record.tag,
            gpu_base: record.mesh.gpu_base,
            resident_bytes: record.mesh.storage_bytes,
            entries: record.entries,
            text_chars: record.text_chars,
            rows: record.rows,
            glyphs: record.glyphs,
            submits: record.submits,
            in_flight: record.in_flight,
            quarantined: record.quarantined,
        })
        .collect()
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
