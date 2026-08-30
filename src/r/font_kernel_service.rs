//! Shared multi-consumer GPU-font service.
//!
//! `RetainScene` yields GPU-VM-resident Skrifa coverage that a caller may
//! restamp repeatedly. `Stamp` is the one-shot path: the worker creates the
//! same retained representation temporarily. Prepared glyph plans are
//! materialized as request-local coverage; there is no kernel-global glyph
//! lookup or tile cache. Overlapping placements retain the proven max-union
//! representation.
//! The service composites ordered font/color layers into either a new
//! GPU-visible premultiplied RGBA8 buffer or a leased UI4 frame, and returns
//! the owned buffer or exact producer-release proof asynchronously. Stamp
//! callers may preserve a canvas or request an exact coverage-union crop; both
//! obey the UHD/4K pixel and 4096-glyph soft caps.
//! The lane is deliberately local to real font retain/stamp work. Unrelated
//! GPU clients own admission through the GPU executor and GuC contexts.

use alloc::{boxed::Box, collections::VecDeque, string::String, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use embassy_sync::semaphore::{FairSemaphore, Semaphore, SemaphoreReleaser};
use embassy_sync::signal::Signal;
use spin::Mutex;
use trueos_time::{Duration as EmbassyDuration, Instant, Timer};

use crate::intel::gpu_font::{
    GpuFontFace, GpuFontGlyphRecipe, GpuFontGlyphRecipeKey, GpuFontJobEntry, GpuFontRetainedScene,
    GpuFontRetainedSceneError, GpuFontRgba, GpuFontTextRequest, MAX_DYNAMIC_TEXT_CHARS,
    build_gpu_font_glyph_recipe, ensure_font_face_available, font_face_is_available,
    font_face_supports_text, gpu_font_centered_glyph_recipe_key,
    place_gpu_font_origin_glyph_recipe, retain_gpu_font_centered_scene_at_raster,
    retain_gpu_font_prepared_centered_scene, retain_gpu_font_prepared_origin_scene,
    retain_gpu_font_scene_at_raster, wait_for_font_face_available,
};
use crate::r::font_plan_service::PreparedGlyphPlan;

const FONT_KERNEL_QUEUE_CAPACITY: usize = 32;
const FONT_KERNEL_MAX_RUNS: usize = 64;
const FONT_KERNEL_MAX_STAMP_LAYERS: usize = 64;
pub(crate) const FONT_STAMP_MAX_EXTENT: u32 = 4096;
pub(crate) const FONT_STAMP_MAX_PIXELS: u64 = 3840 * 2160;
pub(crate) const FONT_STAMP_MAX_GLYPHS: usize = 4096;
const FONT_KERNEL_LANE_RETRY_MS: u64 = 2;
const FONT_KERNEL_GPU_RETRY_MS: u64 = 2;
const FONT_KERNEL_GPU_WAITERS: usize = 32;
/// One registration owns exactly this much recipe-state budget.  The recipe
/// is colorless outline/coverage state, not an RGBA sprite, so the final stamp
/// can retain its independently changing foreground color.
pub(crate) const FONT_PRODUCER_GLYPH_CACHE_BYTES: usize = 80 * 1024;
const FONT_PRODUCER_GLYPH_CACHE_THEORETICAL_32_BYTES: usize = FONT_PRODUCER_GLYPH_CACHE_BYTES * 32;
static NEXT_TICKET: AtomicU64 = AtomicU64::new(1);
static ONLINE: AtomicBool = AtomicBool::new(false);
static IN_FLIGHT: AtomicBool = AtomicBool::new(false);
static GPU_RETRY_DELAY_PENDING: AtomicBool = AtomicBool::new(false);
static LAST_RETAIN_PARTITION_LOG_TICKET: AtomicU64 = AtomicU64::new(0);
static LAST_STAMP_PARTITION_LOG_TICKET: AtomicU64 = AtomicU64::new(0);
static WORK_AVAILABLE: Signal<crate::wait::EmbassySpinRawMutex, ()> = Signal::new();
static REQUESTS: Mutex<VecDeque<QueuedFontRequest>> = Mutex::new(VecDeque::new());
static STATUS: Mutex<FontKernelServiceStatus> = Mutex::new(FontKernelServiceStatus::new());
static GPU_LANE: FairSemaphore<crate::wait::EmbassySpinRawMutex, FONT_KERNEL_GPU_WAITERS> =
    FairSemaphore::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FontKernelTicket(u64);

impl FontKernelTicket {
    pub(crate) const fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FontKernelConsumerPath {
    RetainScene,
    Stamp,
}

impl FontKernelConsumerPath {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::RetainScene => "retain-scene",
            Self::Stamp => "stamp",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FontKernelConsumer {
    pub(crate) path: FontKernelConsumerPath,
    pub(crate) id: u64,
}

impl FontKernelConsumer {
    pub(crate) const fn new(path: FontKernelConsumerPath, id: u64) -> Self {
        Self { path, id }
    }
}

type FontKernelGpuSemaphore =
    FairSemaphore<crate::wait::EmbassySpinRawMutex, FONT_KERNEL_GPU_WAITERS>;

/// Exclusive, FIFO admission to Font Engine direct-RCS work.
///
/// The hardware path remains deliberately single-submit. Dropping the lease
/// hands the lane to the oldest asynchronous waiter.
pub(crate) struct FontKernelGpuLease {
    permit: Option<SemaphoreReleaser<'static, FontKernelGpuSemaphore>>,
    consumer: FontKernelConsumer,
}

impl Drop for FontKernelGpuLease {
    fn drop(&mut self) {
        {
            let mut status = STATUS.lock();
            if status.active_consumer == Some(self.consumer) {
                status.active_consumer = None;
            }
        }
        drop(self.permit.take());
    }
}

pub(crate) async fn acquire_gpu_lane(consumer: FontKernelConsumer) -> FontKernelGpuLease {
    let wait_started_ms = Instant::now().as_millis();
    let permit = if let Some(permit) = GPU_LANE.try_acquire(1) {
        permit
    } else {
        {
            let mut status = STATUS.lock();
            status.lane_contentions = status.lane_contentions.saturating_add(1);
            status.lane_waiters = status.lane_waiters.saturating_add(1);
            status.lane_peak_waiters = status.lane_peak_waiters.max(status.lane_waiters);
        }
        let permit = loop {
            match GPU_LANE.acquire(1).await {
                Ok(permit) => break permit,
                Err(_) => {
                    // The semaphore has bounded waiter storage. Recover if a
                    // burst temporarily exhausts that bookkeeping capacity.
                    Timer::after(EmbassyDuration::from_millis(FONT_KERNEL_LANE_RETRY_MS)).await;
                }
            }
        };
        {
            let mut status = STATUS.lock();
            status.lane_waiters = status.lane_waiters.saturating_sub(1);
        }
        permit
    };
    record_gpu_lane_admission(consumer, Instant::now().as_millis().saturating_sub(wait_started_ms));
    FontKernelGpuLease {
        permit: Some(permit),
        consumer,
    }
}

fn record_gpu_lane_admission(consumer: FontKernelConsumer, waited_ms: u64) {
    let mut status = STATUS.lock();
    status.active_consumer = Some(consumer);
    status.lane_admissions = status.lane_admissions.saturating_add(1);
    status.lane_wait_ms = status.lane_wait_ms.saturating_add(waited_ms);
    status.lane_wait_max_ms = status.lane_wait_max_ms.max(waited_ms);
    match consumer.path {
        FontKernelConsumerPath::RetainScene => {
            status.retain_lane_admissions = status.retain_lane_admissions.saturating_add(1);
        }
        FontKernelConsumerPath::Stamp => {
            status.stamp_lane_admissions = status.stamp_lane_admissions.saturating_add(1);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FontKernelError {
    QueueFull,
    InvalidRequest(&'static str),
    Unavailable(&'static str),
    SubmittedIncomplete(&'static str),
}

impl From<GpuFontRetainedSceneError> for FontKernelError {
    fn from(error: GpuFontRetainedSceneError) -> Self {
        match error {
            GpuFontRetainedSceneError::Unavailable(reason) => Self::Unavailable(reason),
            GpuFontRetainedSceneError::SubmittedIncomplete(reason) => {
                Self::SubmittedIncomplete(reason)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RetainedFontPositioning {
    SceneOrigin,
    VisualBoundsCenter,
}

#[derive(Clone, Debug)]
pub(crate) struct RetainedFontRun {
    pub(crate) text: String,
    pub(crate) position: [f32; 2],
    pub(crate) font_pixels: f32,
    pub(crate) slant: f32,
}

#[derive(Clone, Debug)]
pub(crate) struct RetainSceneRequest {
    pub(crate) runs: Vec<RetainedFontRun>,
    pub(crate) font: GpuFontFace,
    pub(crate) viewport_width: u32,
    pub(crate) viewport_height: u32,
    pub(crate) raster_width: u32,
    pub(crate) raster_height: u32,
    pub(crate) positioning: RetainedFontPositioning,
}

#[derive(Clone, Debug)]
pub(crate) struct FontStampLayer {
    pub(crate) scene: RetainSceneRequest,
    pub(crate) foreground: GpuFontRgba,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FontStampFit {
    /// Preserve the caller's complete raster, including transparent space.
    Canvas,
    /// Crop the returned allocation to the union of all generated coverage.
    Tight,
}

#[derive(Clone, Debug)]
pub(crate) struct FontStampRequest {
    pub(crate) layers: Vec<FontStampLayer>,
    pub(crate) fit: FontStampFit,
}

/// Convert CPU-owned Picasso Unicode rows into the existing one-release
/// FontKernel canvas request.
///
/// This is the compatibility visual bridge while native Picasso glyph
/// instances are still waiting for compositor release/dependency integration.
/// Rows are grouped by face and color, retain per-run slant, and copy Unicode
/// once at service admission. SceneDB never carries TTF glyph IDs, outline
/// commands, recipe keys, or R8 atlas bytes.
pub(crate) fn picasso_font_lookup_canvas_request(
    rows: &[trueos_helio_runtime::picasso_scene::FontLookupRun],
    viewport_width: u32,
    viewport_height: u32,
    raster_width: u32,
    raster_height: u32,
) -> Result<FontStampRequest, FontKernelError> {
    use trueos_helio_runtime::picasso_scene::FontFace;

    if rows.is_empty()
        || viewport_width == 0
        || viewport_height == 0
        || raster_width == 0
        || raster_height == 0
    {
        return Err(FontKernelError::InvalidRequest("font-picasso-lookup-contract"));
    }
    let mut layers: Vec<(GpuFontFace, GpuFontRgba, Vec<RetainedFontRun>)> = Vec::new();
    for row in rows {
        let font = match row.face {
            FontFace::Default => GpuFontFace::Default,
            FontFace::NotoSansSc => GpuFontFace::NotoSansSc,
            FontFace::Inconsolata => GpuFontFace::Inconsolata,
        };
        let color =
            GpuFontRgba::new(row.color.red, row.color.green, row.color.blue, row.color.alpha);
        let group = layers
            .iter()
            .position(|(candidate_font, candidate_color, runs)| {
                *candidate_font == font
                    && *candidate_color == color
                    && runs.len() < FONT_KERNEL_MAX_RUNS
            });
        let group = match group {
            Some(group) => group,
            None => {
                if layers.len() >= FONT_KERNEL_MAX_STAMP_LAYERS {
                    return Err(FontKernelError::InvalidRequest(
                        "font-picasso-lookup-layer-softcap",
                    ));
                }
                layers.push((font, color, Vec::new()));
                layers.len() - 1
            }
        };
        layers[group].2.push(RetainedFontRun {
            text: row.text.clone(),
            position: row.origin,
            font_pixels: row.font_pixels,
            slant: row.slant.kernel_shear(),
        });
    }
    let request = FontStampRequest {
        layers: layers
            .into_iter()
            .map(|(font, foreground, runs)| FontStampLayer {
                scene: RetainSceneRequest {
                    runs,
                    font,
                    viewport_width,
                    viewport_height,
                    raster_width,
                    raster_height,
                    positioning: RetainedFontPositioning::SceneOrigin,
                },
                foreground,
            })
            .collect(),
        fit: FontStampFit::Canvas,
    };
    validate_stamp_request(&request)?;
    Ok(request)
}

/// Ownership carried by a direct-frame request after admission.
///
/// A normal request still performs its generic validation before this value is
/// constructed. A prepared plan is already sealed by the plan service, so the
/// font kernel only inspects its O(1) frame contract before moving it into the
/// FIFO.
enum FrameStampInput {
    Request(FontStampRequest),
    /// Producer-owned request which may reuse its generation-local, bounded
    /// colorless glyph recipes. The resources Arc keeps this exact generation
    /// alive until the queued row completes; it does not alter row ACKs.
    ProducerRequest {
        request: FontStampRequest,
        glyph_cache: Arc<FontGpuProducerResources>,
    },
    Prepared(PreparedGlyphPlan),
    FontRushClear {
        color_rgba: u32,
        raster_width: u32,
        raster_height: u32,
    },
    FontRushRgba8Sprites {
        source: Arc<FontStampedBuffer>,
        descriptors: Vec<crate::intel::gpgpu::GpgpuSpriteQuadWorklistDesc>,
        logical_glyphs: usize,
        raster_width: u32,
        raster_height: u32,
    },
}

impl FrameStampInput {
    fn fit(&self) -> FontStampFit {
        match self {
            Self::Request(request) => request.fit,
            Self::ProducerRequest { request, .. } => request.fit,
            Self::Prepared(plan) => plan.fit(),
            Self::FontRushClear { .. } | Self::FontRushRgba8Sprites { .. } => FontStampFit::Canvas,
        }
    }

    fn raster_extent(&self) -> Option<(u32, u32)> {
        match self {
            Self::Request(request) => request
                .layers
                .first()
                .map(|layer| (layer.scene.raster_width, layer.scene.raster_height)),
            Self::ProducerRequest { request, .. } => request
                .layers
                .first()
                .map(|layer| (layer.scene.raster_width, layer.scene.raster_height)),
            Self::Prepared(plan) => Some((plan.raster_width(), plan.raster_height())),
            Self::FontRushClear {
                raster_width,
                raster_height,
                ..
            }
            | Self::FontRushRgba8Sprites {
                raster_width,
                raster_height,
                ..
            } => Some((*raster_width, *raster_height)),
        }
    }
}

/// A prepared frame was not admitted, with its exact sealed plan returned.
///
/// Admission has not transferred ownership on this path. The producer may
/// retain the plan for backpressure retry or drop it to release its bounded
/// plan-service storage.
pub(crate) struct PreparedFrameStampRejection {
    error: FontKernelError,
    plan: PreparedGlyphPlan,
}

impl PreparedFrameStampRejection {
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) const fn error(&self) -> FontKernelError {
        self.error
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) const fn plan(&self) -> &PreparedGlyphPlan {
        &self.plan
    }

    pub(crate) fn into_parts(self) -> (FontKernelError, PreparedGlyphPlan) {
        (self.error, self.plan)
    }
}

/// One logical retained scene backed by bounded analytical coverage masks.
///
/// Gridpaper uses the same low-level model: keep independently admitted R8
/// masks resident, then composite them together as one draw-time layer batch.
pub(crate) struct FontKernelRetainedScene {
    masks: Vec<GpuFontRetainedScene>,
}

impl FontKernelRetainedScene {
    pub(crate) fn masks(
        &self,
    ) -> impl Iterator<Item = Option<(crate::intel::gpgpu::GpgpuMask8Surface, [i32; 2])>> + '_ {
        self.masks
            .iter()
            .map(|mask| Some((mask.mask_surface()?, mask.origin_px()?)))
    }

    pub(crate) const fn mask_count(&self) -> usize {
        self.masks.len()
    }
}

/// GPU-visible RGBA output from one asynchronous stamp request.
pub(crate) struct FontStampedBuffer {
    ticket: FontKernelTicket,
    storage: crate::intel::gpgpu::GpgpuOwnedRgba8Surface,
    origin_px: [i32; 2],
    glyphs: usize,
    submits: usize,
    active_walkers: usize,
}

/// Completion metadata for a stamp written directly into a caller-owned UI4
/// frame. The release is bound to that exact allocation and is the only token
/// accepted by the frame pool for GPU-authored publication.
pub(crate) struct FontFrameStamp {
    ticket: FontKernelTicket,
    glyphs: usize,
    submits: usize,
    clear_submits: usize,
    active_walkers: usize,
    pre_service_ms: u64,
    clear_ms: u64,
    prepare_coverage_ms: u64,
    coverage_build_ms: u64,
    coverage_audit_ms: u64,
    coverage_submits: usize,
    instance_release_ms: u64,
    total_service_ms: u64,
    release: crate::intel::gpgpu::GpgpuRgba8ReleaseFence,
}

struct FontGpuProducerResources {
    lease: crate::r::font_producer_service::FontProducerLease,
    storage: FontGpuProducerStorage,
    /// The cache is owned by this lease/generation, never by a face or a
    /// registry slot. Re-registration therefore installs a fresh cache even
    /// if the numerical producer id is reused.
    glyph_cache: Mutex<ProducerGlyphRecipeCache>,
}

struct ProducerGlyphRecipeCacheEntry {
    recipe: Arc<GpuFontGlyphRecipe>,
    accounted_bytes: usize,
}

/// First-fill, no-eviction cache for one producer lease. Cache accounting
/// includes the immutable recipe header and the entry pointer as well as its
/// outline-op bytes. Allocator bookkeeping is deliberately not treated as
/// cache payload; `used_bytes` can never exceed the fixed 80 KiB contract.
struct ProducerGlyphRecipeCache {
    entries: Vec<ProducerGlyphRecipeCacheEntry>,
    used_bytes: usize,
    hits: u64,
    misses: u64,
    uncached: u64,
    accepting_fills: bool,
}

impl ProducerGlyphRecipeCache {
    const fn new() -> Self {
        Self {
            entries: Vec::new(),
            used_bytes: 0,
            hits: 0,
            misses: 0,
            uncached: 0,
            accepting_fills: true,
        }
    }

    fn recipe_bytes(recipe: &GpuFontGlyphRecipe) -> usize {
        core::mem::size_of::<GpuFontGlyphRecipe>()
            .saturating_add(recipe.ops_bytes())
            .saturating_add(core::mem::size_of::<ProducerGlyphRecipeCacheEntry>())
    }

    /// Build while holding the per-producer lock. A producer can have two
    /// frame-ring requests in flight, so this also single-flights first fill
    /// without a global glyph-cache lock.
    fn get_or_build(
        &mut self,
        key: GpuFontGlyphRecipeKey,
    ) -> Result<(Arc<GpuFontGlyphRecipe>, bool), &'static str> {
        if let Some(entry) = self.entries.iter().find(|entry| entry.recipe.key() == key) {
            self.hits = self.hits.saturating_add(1);
            return Ok((Arc::clone(&entry.recipe), true));
        }
        self.misses = self.misses.saturating_add(1);
        let recipe = build_gpu_font_glyph_recipe(key)?;
        if !self.accepting_fills {
            // A retired generation may still have an already-admitted row in
            // the FIFO. It completes normally but cannot resurrect cache
            // storage after unregister has freed it.
            self.uncached = self.uncached.saturating_add(1);
            return Ok((recipe, false));
        }
        let bytes = Self::recipe_bytes(recipe.as_ref());
        if bytes <= FONT_PRODUCER_GLYPH_CACHE_BYTES.saturating_sub(self.used_bytes) {
            self.used_bytes = self.used_bytes.saturating_add(bytes);
            self.entries.push(ProducerGlyphRecipeCacheEntry {
                recipe: Arc::clone(&recipe),
                accounted_bytes: bytes,
            });
            Ok((recipe, false))
        } else {
            // No eviction: early frequently requested glyphs remain useful;
            // a later unique glyph still renders through the normal path.
            self.uncached = self.uncached.saturating_add(1);
            Ok((recipe, false))
        }
    }

    fn retire(&mut self) {
        // Dropping the Arcs releases every cache-owned recipe immediately.
        self.entries.clear();
        self.used_bytes = 0;
        self.hits = 0;
        self.misses = 0;
        self.uncached = 0;
        self.accepting_fills = false;
    }

    fn diagnostics(&self) -> (usize, usize, u64, u64, u64) {
        debug_assert_eq!(
            self.used_bytes,
            self.entries
                .iter()
                .fold(0usize, |total, entry| total.saturating_add(entry.accounted_bytes))
        );
        (self.entries.len(), self.used_bytes, self.hits, self.misses, self.uncached)
    }
}

enum FontGpuProducerStorage {
    /// Producer-owned output used by non-UI4 clients.
    RetainedRows(Vec<crate::intel::gpgpu::GpgpuOwnedRgba8Surface>),
    /// The retained rows are the ordinary UI4 frame ring itself.  The frame
    /// pool owns those allocations and lends one exact buffer per submission.
    Ui4FrameRing,
}

impl Drop for FontGpuProducerResources {
    fn drop(&mut self) {
        self.glyph_cache.lock().retire();
        let _ = crate::r::font_producer_service::release_producer(self.lease);
    }
}

/// Registered semi-persistent Font producer with a fixed tier and retained
/// row-output ring.  Registration allocates every output once; ordinary row
/// submission only selects an ACKed slot and dispatches into its stable GPU
/// virtual range through the existing serialized Font RCS lane.
pub(crate) struct FontGpuProducer {
    resources: Arc<FontGpuProducerResources>,
}

impl Clone for FontGpuProducer {
    fn clone(&self) -> Self {
        Self {
            resources: Arc::clone(&self.resources),
        }
    }
}

struct QueuedFontProducerRow {
    token: crate::r::font_producer_service::FontRowToken,
    resources: Arc<FontGpuProducerResources>,
}

pub(crate) struct PendingFontProducerRow {
    token: crate::r::font_producer_service::FontRowToken,
    surface: crate::intel::gpgpu::GpgpuRgba8Surface,
    resources: Arc<FontGpuProducerResources>,
    completion: PendingFontFrameStamp,
}

#[expect(
    dead_code,
    reason = "semi-persistent producer API awaits its first UI4 row consumer"
)]
pub(crate) struct FontProducedRow {
    token: crate::r::font_producer_service::FontRowToken,
    surface: crate::intel::gpgpu::GpgpuRgba8Surface,
    resources: Option<Arc<FontGpuProducerResources>>,
    stamp: FontFrameStamp,
    surflive: bool,
    acknowledged: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FontGpuProducerError {
    Control(crate::r::font_producer_service::FontProducerError),
    Kernel(FontKernelError),
}

impl From<crate::r::font_producer_service::FontProducerError> for FontGpuProducerError {
    fn from(error: crate::r::font_producer_service::FontProducerError) -> Self {
        Self::Control(error)
    }
}

impl From<FontKernelError> for FontGpuProducerError {
    fn from(error: FontKernelError) -> Self {
        Self::Kernel(error)
    }
}

impl FontFrameStamp {
    pub(crate) const fn ticket(&self) -> FontKernelTicket {
        self.ticket
    }

    pub(crate) const fn glyphs(&self) -> usize {
        self.glyphs
    }

    pub(crate) const fn submits(&self) -> usize {
        self.submits
    }

    pub(crate) const fn clear_submits(&self) -> usize {
        self.clear_submits
    }

    pub(crate) const fn active_walkers(&self) -> usize {
        self.active_walkers
    }

    /// Time from FIFO insertion until the blocking service worker began. This
    /// deliberately includes FIFO backlog, GPU-lane admission, and blocking
    /// worker dispatch; it is not presented as a pure queue measurement.
    pub(crate) const fn pre_service_ms(&self) -> u64 {
        self.pre_service_ms
    }

    pub(crate) const fn clear_ms(&self) -> u64 {
        self.clear_ms
    }

    pub(crate) const fn prepare_coverage_ms(&self) -> u64 {
        self.prepare_coverage_ms
    }

    /// Outline preparation, allocation, and analytical R8 GPU generation.
    pub(crate) const fn coverage_build_ms(&self) -> u64 {
        self.coverage_build_ms
    }

    /// CPU cache flush and full-mask nonzero integrity scan.
    pub(crate) const fn coverage_audit_ms(&self) -> u64 {
        self.coverage_audit_ms
    }

    pub(crate) const fn coverage_submits(&self) -> usize {
        self.coverage_submits
    }

    pub(crate) const fn instance_release_ms(&self) -> u64 {
        self.instance_release_ms
    }

    /// Worker time from optional clear admission through exact release proof.
    /// Pre-service delay is reported separately and is not included here.
    pub(crate) const fn total_service_ms(&self) -> u64 {
        self.total_service_ms
    }

    pub(crate) const fn release(&self) -> crate::intel::gpgpu::GpgpuRgba8ReleaseFence {
        self.release
    }
}

fn producer_font_pixels_milli(font_pixels: f32) -> Option<u32> {
    if !font_pixels.is_finite() || font_pixels <= 0.0 {
        return None;
    }
    let scaled = libm::roundf(font_pixels * 1_000.0);
    if scaled < 1.0 || scaled > u32::MAX as f32 {
        return None;
    }
    Some(scaled as u32)
}

fn validate_font_producer_row_request(
    registration: crate::r::font_producer_service::FontProducerRegistration,
    request: &FontStampRequest,
) -> Result<usize, FontKernelError> {
    validate_stamp_request(request)?;
    if request.fit != FontStampFit::Canvas
        || registration.format
            != crate::r::font_producer_service::FontProducerFormat::Rgba8Premultiplied
    {
        return Err(FontKernelError::InvalidRequest("font-producer-row-format"));
    }
    let expected_face = GpuFontFace::from_id(u32::from(registration.face))
        .ok_or(FontKernelError::InvalidRequest("font-producer-face"))?;
    let mut chars = 0usize;
    for layer in &request.layers {
        let scene = &layer.scene;
        if scene.font != expected_face
            || scene.viewport_width != registration.row_width_px
            || scene.viewport_height != registration.row_height_px
            || scene.raster_width != registration.row_width_px
            || scene.raster_height != registration.row_height_px
            || scene.positioning != RetainedFontPositioning::SceneOrigin
        {
            return Err(FontKernelError::InvalidRequest("font-producer-row-contract"));
        }
        for run in &scene.runs {
            if producer_font_pixels_milli(run.font_pixels) != Some(registration.font_pixels_milli)
                || run
                    .text
                    .chars()
                    .any(|character| matches!(character, '\r' | '\n'))
            {
                return Err(FontKernelError::InvalidRequest("font-producer-static-tier"));
            }
            chars = chars.saturating_add(run.text.chars().count());
        }
    }
    if chars == 0 || chars > registration.max_chars {
        return Err(FontKernelError::InvalidRequest("font-producer-row-length"));
    }
    Ok(chars)
}

#[expect(
    dead_code,
    reason = "semi-persistent producer API awaits its first UI4 row consumer"
)]
pub(crate) fn register_gpu_font_producer(
    registration: crate::r::font_producer_service::FontProducerRegistration,
) -> Result<FontGpuProducer, FontGpuProducerError> {
    if registration.format
        != crate::r::font_producer_service::FontProducerFormat::Rgba8Premultiplied
        || u64::from(registration.row_width_px) * u64::from(registration.row_height_px)
            > FONT_STAMP_MAX_PIXELS
    {
        return Err(FontKernelError::InvalidRequest("font-producer-gpu-geometry").into());
    }
    let face = GpuFontFace::from_id(u32::from(registration.face))
        .ok_or(FontKernelError::InvalidRequest("font-producer-face"))?;
    ensure_font_face_available(face).map_err(FontKernelError::Unavailable)?;
    let lease = crate::r::font_producer_service::register_producer(registration)?;
    let mut rows = Vec::with_capacity(registration.row_ring_depth);
    for _ in 0..registration.row_ring_depth {
        let Some(row) = crate::intel::gpgpu::allocate_font_instance_rgba8_surface(
            registration.row_width_px,
            registration.row_height_px,
        ) else {
            drop(rows);
            let _ = crate::r::font_producer_service::release_producer(lease);
            return Err(FontKernelError::Unavailable("font-producer-row-allocation").into());
        };
        rows.push(row);
    }
    crate::log_info!(target: "render";
        "font-kernel-service: producer registered id={} generation={} face={} tier={} font_pixels_milli={} extent={}x{} rows={} max_chars={} storage=persistent-font-rgba8 glyph_recipe_cache=colorless-first-fill-no-evict cache_budget_bytes={} theoretical_32_cache_bytes={} submit_lane=serialized-font-rcs\n",
        lease.producer_id(),
        lease.generation(),
        face.registry_name(),
        registration.tier,
        registration.font_pixels_milli,
        registration.row_width_px,
        registration.row_height_px,
        registration.row_ring_depth,
        registration.max_chars,
        FONT_PRODUCER_GLYPH_CACHE_BYTES,
        FONT_PRODUCER_GLYPH_CACHE_THEORETICAL_32_BYTES,
    );
    Ok(FontGpuProducer {
        resources: Arc::new(FontGpuProducerResources {
            lease,
            storage: FontGpuProducerStorage::RetainedRows(rows),
            glyph_cache: Mutex::new(ProducerGlyphRecipeCache::new()),
        }),
    })
}

/// Register a semi-persistent producer whose retained output ring is an
/// ordinary UI4 dirty/double Frame.  Registration fixes the font tier and row
/// geometry, while UI4 remains the sole owner of the backing allocations.
pub(crate) fn register_ui4_gpu_font_producer(
    registration: crate::r::font_producer_service::FontProducerRegistration,
) -> Result<FontGpuProducer, FontGpuProducerError> {
    if registration.format
        != crate::r::font_producer_service::FontProducerFormat::Rgba8Premultiplied
        || registration.row_ring_depth != 2
        || u64::from(registration.row_width_px) * u64::from(registration.row_height_px)
            > FONT_STAMP_MAX_PIXELS
    {
        return Err(FontKernelError::InvalidRequest("font-producer-ui4-geometry").into());
    }
    let face = GpuFontFace::from_id(u32::from(registration.face))
        .ok_or(FontKernelError::InvalidRequest("font-producer-face"))?;
    ensure_font_face_available(face).map_err(FontKernelError::Unavailable)?;
    let lease = crate::r::font_producer_service::register_producer(registration)?;
    crate::log_info!(target: "render";
        "font-kernel-service: UI4 producer registered id={} generation={} face={} tier={} font_pixels_milli={} extent={}x{} rows={} max_chars={} storage=ui4-frame-ring glyph_recipe_cache=colorless-first-fill-no-evict cache_budget_bytes={} theoretical_32_cache_bytes={} submit_lane=serialized-font-rcs\n",
        lease.producer_id(),
        lease.generation(),
        face.registry_name(),
        registration.tier,
        registration.font_pixels_milli,
        registration.row_width_px,
        registration.row_height_px,
        registration.row_ring_depth,
        registration.max_chars,
        FONT_PRODUCER_GLYPH_CACHE_BYTES,
        FONT_PRODUCER_GLYPH_CACHE_THEORETICAL_32_BYTES,
    );
    Ok(FontGpuProducer {
        resources: Arc::new(FontGpuProducerResources {
            lease,
            storage: FontGpuProducerStorage::Ui4FrameRing,
            glyph_cache: Mutex::new(ProducerGlyphRecipeCache::new()),
        }),
    })
}

#[expect(
    dead_code,
    reason = "semi-persistent producer API awaits its first UI4 row consumer"
)]
impl FontGpuProducer {
    pub(crate) fn lease(&self) -> crate::r::font_producer_service::FontProducerLease {
        self.resources.lease
    }

    pub(crate) fn status(
        &self,
    ) -> Result<
        crate::r::font_producer_service::FontProducerStatus,
        crate::r::font_producer_service::FontProducerError,
    > {
        crate::r::font_producer_service::producer_status(self.resources.lease)
    }

    pub(crate) fn request_release(
        &self,
    ) -> Result<bool, crate::r::font_producer_service::FontProducerError> {
        let retired = crate::r::font_producer_service::release_producer(self.resources.lease)?;
        // Retirement prohibits further row reservations. Existing queued work
        // keeps its normal exact-buffer lifecycle, but must fall back to
        // uncached preparation rather than retain producer-owned state.
        self.resources.glyph_cache.lock().retire();
        Ok(retired)
    }

    pub(crate) fn submit_row(
        &self,
        request: FontStampRequest,
        clear_rgba: u32,
    ) -> Result<PendingFontProducerRow, FontGpuProducerError> {
        let registration = self.resources.lease.registration();
        let char_count = validate_font_producer_row_request(registration, &request)?;
        let token = crate::r::font_producer_service::reserve_producer_row(
            self.resources.lease,
            char_count,
        )?;
        let row_index = usize::from(token.row_index());
        let FontGpuProducerStorage::RetainedRows(rows) = &self.resources.storage else {
            let _ = crate::r::font_producer_service::cancel_reserved_producer_row(token);
            return Err(FontKernelError::InvalidRequest("font-producer-storage-mode").into());
        };
        let Some(surface) = rows
            .get(row_index)
            .map(crate::intel::gpgpu::GpgpuOwnedRgba8Surface::surface)
        else {
            let _ = crate::r::font_producer_service::cancel_reserved_producer_row(token);
            return Err(FontKernelError::InvalidRequest("font-producer-row-index").into());
        };
        let queued_row = QueuedFontProducerRow {
            token,
            resources: Arc::clone(&self.resources),
        };
        let completion = match queue_frame_stamp(
            FrameStampInput::ProducerRequest {
                request,
                glyph_cache: Arc::clone(&self.resources),
            },
            surface,
            Some(clear_rgba),
            Some(queued_row),
        ) {
            Ok(completion) => completion,
            Err(rejection) => {
                let _ = crate::r::font_producer_service::cancel_reserved_producer_row(token);
                return Err(rejection.error.into());
            }
        };
        Ok(PendingFontProducerRow {
            token,
            surface,
            resources: Arc::clone(&self.resources),
            completion,
        })
    }

    /// Submit one row directly into the exact UI4 frame buffer currently held
    /// by `buffer_index`.  The registry token is deliberately reserved for
    /// that same index so a later reacquisition can ACK precisely the backing
    /// which UI4 has stopped reading.
    pub(crate) fn submit_ui4_row(
        &self,
        request: FontStampRequest,
        destination: crate::intel::gpgpu::GpgpuRgba8Surface,
        buffer_index: u8,
        clear_rgba: u32,
    ) -> Result<PendingFontProducerRow, FontGpuProducerError> {
        self.submit_ui4_row_with_clear(request, destination, buffer_index, Some(clear_rgba))
    }

    /// Continue an already-acquired UI4 plane canvas without clearing it.
    /// This lets several registered producers contribute disjoint regions to
    /// one exact back buffer before the final producer release is published.
    pub(crate) fn submit_ui4_row_over(
        &self,
        request: FontStampRequest,
        destination: crate::intel::gpgpu::GpgpuRgba8Surface,
        buffer_index: u8,
    ) -> Result<PendingFontProducerRow, FontGpuProducerError> {
        self.submit_ui4_row_with_clear(request, destination, buffer_index, None)
    }

    fn submit_ui4_row_with_clear(
        &self,
        request: FontStampRequest,
        destination: crate::intel::gpgpu::GpgpuRgba8Surface,
        buffer_index: u8,
        clear_rgba: Option<u32>,
    ) -> Result<PendingFontProducerRow, FontGpuProducerError> {
        let registration = self.resources.lease.registration();
        let char_count = validate_font_producer_row_request(registration, &request)?;
        if !matches!(&self.resources.storage, FontGpuProducerStorage::Ui4FrameRing)
            || usize::from(buffer_index) >= registration.row_ring_depth
            || !destination.is_valid()
            || destination.width != registration.row_width_px
            || destination.height != registration.row_height_px
            || destination.storage_order != crate::intel::gpgpu::GpgpuRgba8StorageOrder::Rgba
        {
            return Err(FontKernelError::InvalidRequest("font-producer-ui4-surface").into());
        }
        let token = crate::r::font_producer_service::reserve_specific_producer_row(
            self.resources.lease,
            usize::from(buffer_index),
            char_count,
        )?;
        let queued_row = QueuedFontProducerRow {
            token,
            resources: Arc::clone(&self.resources),
        };
        let completion = match queue_frame_stamp(
            FrameStampInput::ProducerRequest {
                request,
                glyph_cache: Arc::clone(&self.resources),
            },
            destination,
            clear_rgba,
            Some(queued_row),
        ) {
            Ok(completion) => completion,
            Err(rejection) => {
                let _ = crate::r::font_producer_service::cancel_reserved_producer_row(token);
                return Err(rejection.error.into());
            }
        };
        Ok(PendingFontProducerRow {
            token,
            surface: destination,
            resources: Arc::clone(&self.resources),
            completion,
        })
    }
}

#[expect(
    dead_code,
    reason = "semi-persistent producer API awaits its first UI4 row consumer"
)]
impl PendingFontProducerRow {
    fn produced(
        token: crate::r::font_producer_service::FontRowToken,
        surface: crate::intel::gpgpu::GpgpuRgba8Surface,
        resources: &Arc<FontGpuProducerResources>,
        stamp: FontFrameStamp,
    ) -> Result<FontProducedRow, FontKernelError> {
        if crate::r::font_producer_service::producer_row_state(token)
            != Ok(crate::r::font_producer_service::FontRowState::Produced)
        {
            return Err(FontKernelError::SubmittedIncomplete("font-producer-completion-state"));
        }
        Ok(FontProducedRow {
            token,
            surface,
            resources: Some(Arc::clone(resources)),
            stamp,
            surflive: false,
            acknowledged: false,
        })
    }

    pub(crate) const fn token(&self) -> crate::r::font_producer_service::FontRowToken {
        self.token
    }

    pub(crate) const fn ticket(&self) -> FontKernelTicket {
        self.completion.ticket()
    }

    pub(crate) fn try_take(&mut self) -> Option<Result<FontProducedRow, FontKernelError>> {
        let result = self.completion.try_take()?;
        Some(
            result
                .and_then(|stamp| Self::produced(self.token, self.surface, &self.resources, stamp)),
        )
    }

    pub(crate) async fn wait(self) -> Result<FontProducedRow, FontKernelError> {
        let Self {
            token,
            surface,
            resources,
            completion,
        } = self;
        let stamp = completion.wait().await?;
        Self::produced(token, surface, &resources, stamp)
    }
}

#[expect(
    dead_code,
    reason = "semi-persistent producer API awaits its first UI4 row consumer"
)]
impl FontProducedRow {
    pub(crate) const fn token(&self) -> crate::r::font_producer_service::FontRowToken {
        self.token
    }

    pub(crate) const fn surface(&self) -> crate::intel::gpgpu::GpgpuRgba8Surface {
        self.surface
    }

    pub(crate) const fn stamp(&self) -> &FontFrameStamp {
        &self.stamp
    }

    /// Record that this exact row became display-live. This does not restore
    /// its credit: scanout may now be actively reading it.
    pub(crate) fn mark_surflive(
        &mut self,
    ) -> Result<(), crate::r::font_producer_service::FontProducerError> {
        let expected = crate::r::font_producer_service::FontRowCompletion {
            release_fence: self.stamp.release().sequence(),
            metadata: self.stamp.ticket().raw(),
        };
        crate::r::font_producer_service::mark_producer_row_surflive(self.token, expected)?;
        self.surflive = true;
        Ok(())
    }

    /// Restore the row credit only after a later compositor transaction made
    /// a replacement SURFLIVE and released this row's exact display lease.
    pub(crate) fn acknowledge_display_release(
        mut self,
    ) -> Result<(), crate::r::font_producer_service::FontProducerError> {
        if !self.surflive {
            return Err(crate::r::font_producer_service::FontProducerError::RowNotSurfLive);
        }
        crate::r::font_producer_service::acknowledge_producer_row(self.token)?;
        self.acknowledged = true;
        Ok(())
    }

    /// Restore a row credit when UI4 reacquired the exact backing without the
    /// row's publish serial ever crossing SURFLIVE (normal coalescing under a
    /// faster producer). This is not a display-live acknowledgement.
    pub(crate) fn acknowledge_unpresented_reacquire(
        mut self,
    ) -> Result<(), crate::r::font_producer_service::FontProducerError> {
        if self.surflive {
            return Err(crate::r::font_producer_service::FontProducerError::RowNotProduced);
        }
        let expected = crate::r::font_producer_service::FontRowCompletion {
            release_fence: self.stamp.release().sequence(),
            metadata: self.stamp.ticket().raw(),
        };
        crate::r::font_producer_service::acknowledge_unpresented_producer_row(
            self.token, expected,
        )?;
        self.acknowledged = true;
        Ok(())
    }

    /// Teardown ACK used only after UI4 has destroyed the complete Frame
    /// which owned this row's backing.  It also covers a GPU-complete row that
    /// was never published because shutdown raced its completion.
    pub(crate) fn acknowledge_ui4_frame_retirement(
        mut self,
    ) -> Result<(), crate::r::font_producer_service::FontProducerError> {
        let expected = crate::r::font_producer_service::FontRowCompletion {
            release_fence: self.stamp.release().sequence(),
            metadata: self.stamp.ticket().raw(),
        };
        crate::r::font_producer_service::acknowledge_retired_producer_row(self.token, expected)?;
        self.acknowledged = true;
        Ok(())
    }
}

impl Drop for FontProducedRow {
    fn drop(&mut self) {
        if self.acknowledged {
            return;
        }
        let _ = crate::r::font_producer_service::abandon_producer_row(self.token);
        if let Some(resources) = self.resources.take() {
            // The exact display acknowledgement was lost. Preserve every
            // backing in this generation instead of freeing a row which may
            // still be display-owned.
            core::mem::forget(resources);
        }
    }
}

impl FontStampedBuffer {
    pub(crate) const fn ticket(&self) -> FontKernelTicket {
        self.ticket
    }

    pub(crate) const fn surface(&self) -> crate::intel::gpgpu::GpgpuRgba8Surface {
        self.storage.surface()
    }

    /// Logical scene coordinate represented by output pixel (0, 0).
    pub(crate) const fn origin_px(&self) -> [i32; 2] {
        self.origin_px
    }

    pub(crate) const fn glyphs(&self) -> usize {
        self.glyphs
    }

    pub(crate) const fn submits(&self) -> usize {
        self.submits
    }

    pub(crate) const fn active_walkers(&self) -> usize {
        self.active_walkers
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn readback_tight_rgba(&self) -> Option<Vec<u8>> {
        self.storage.readback_tight_rgba()
    }
}

pub(crate) struct PendingRetainScene {
    ticket: FontKernelTicket,
    reply: Arc<
        Signal<crate::wait::EmbassySpinRawMutex, Result<FontKernelRetainedScene, FontKernelError>>,
    >,
}

impl PendingRetainScene {
    pub(crate) const fn ticket(&self) -> FontKernelTicket {
        self.ticket
    }

    /// Take a completed retained scene without blocking the caller.
    ///
    /// VM-facing UI4 producers use this to turn the Embassy completion into a
    /// cooperative submit/poll boundary: the guest yields while the worker
    /// owns outline preparation and GPU coverage creation.
    pub(crate) fn try_take(&mut self) -> Option<Result<FontKernelRetainedScene, FontKernelError>> {
        self.reply.try_take()
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) async fn wait(self) -> Result<FontKernelRetainedScene, FontKernelError> {
        self.reply.wait().await
    }
}

pub(crate) struct PendingFontStamp {
    ticket: FontKernelTicket,
    reply:
        Arc<Signal<crate::wait::EmbassySpinRawMutex, Result<FontStampedBuffer, FontKernelError>>>,
}

pub(crate) struct PendingFontFrameStamp {
    ticket: FontKernelTicket,
    queued_ahead: usize,
    reply: Arc<Signal<crate::wait::EmbassySpinRawMutex, Result<FontFrameStamp, FontKernelError>>>,
}

impl PendingFontFrameStamp {
    pub(crate) const fn ticket(&self) -> FontKernelTicket {
        self.ticket
    }

    /// Exact number of requests already resident in the service FIFO while
    /// this request was inserted. A separately dequeued active request is not
    /// included.
    pub(crate) const fn queued_ahead(&self) -> usize {
        self.queued_ahead
    }

    /// Take a completed direct-frame stamp without blocking the caller.
    ///
    /// Blueprint publishers use this as a cooperative submit/poll boundary
    /// while retaining the exact UI4 write lease targeted by the worker.
    pub(crate) fn try_take(&mut self) -> Option<Result<FontFrameStamp, FontKernelError>> {
        self.reply.try_take()
    }

    pub(crate) async fn wait(self) -> Result<FontFrameStamp, FontKernelError> {
        self.reply.wait().await
    }
}

impl PendingFontStamp {
    pub(crate) const fn ticket(&self) -> FontKernelTicket {
        self.ticket
    }

    /// Take a completed one-shot stamp without blocking the caller.
    ///
    /// UI4 owns the returned RGBA allocation until its compositor submission
    /// has retired, so Blueprint VM calls can cooperatively submit and poll
    /// without copying the raster through guest or CPU memory.
    pub(crate) fn try_take(&mut self) -> Option<Result<FontStampedBuffer, FontKernelError>> {
        self.reply.try_take()
    }

    pub(crate) async fn wait(self) -> Result<FontStampedBuffer, FontKernelError> {
        self.reply.wait().await
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct FontKernelServiceStatus {
    pub(crate) online: bool,
    pub(crate) active_ticket: Option<FontKernelTicket>,
    pub(crate) active_stage: &'static str,
    pub(crate) active_consumer: Option<FontKernelConsumer>,
    pub(crate) submitted_retain: u64,
    pub(crate) submitted_stamp: u64,
    pub(crate) completed_retain: u64,
    pub(crate) completed_stamp: u64,
    pub(crate) failed: u64,
    pub(crate) lane_retries: u64,
    pub(crate) gpu_retries: u64,
    pub(crate) lane_waiters: usize,
    pub(crate) lane_peak_waiters: usize,
    pub(crate) lane_admissions: u64,
    pub(crate) lane_contentions: u64,
    pub(crate) lane_wait_ms: u64,
    pub(crate) lane_wait_max_ms: u64,
    pub(crate) retain_lane_admissions: u64,
    pub(crate) stamp_lane_admissions: u64,
    pub(crate) queued: usize,
}

impl FontKernelServiceStatus {
    const fn new() -> Self {
        Self {
            online: false,
            active_ticket: None,
            active_stage: "idle",
            active_consumer: None,
            submitted_retain: 0,
            submitted_stamp: 0,
            completed_retain: 0,
            completed_stamp: 0,
            failed: 0,
            lane_retries: 0,
            gpu_retries: 0,
            lane_waiters: 0,
            lane_peak_waiters: 0,
            lane_admissions: 0,
            lane_contentions: 0,
            lane_wait_ms: 0,
            lane_wait_max_ms: 0,
            retain_lane_admissions: 0,
            stamp_lane_admissions: 0,
            queued: 0,
        }
    }
}

enum QueuedFontRequest {
    Retain {
        ticket: FontKernelTicket,
        request: RetainSceneRequest,
        reply: Arc<
            Signal<
                crate::wait::EmbassySpinRawMutex,
                Result<FontKernelRetainedScene, FontKernelError>,
            >,
        >,
    },
    Stamp {
        ticket: FontKernelTicket,
        request: FontStampRequest,
        reply: Arc<
            Signal<crate::wait::EmbassySpinRawMutex, Result<FontStampedBuffer, FontKernelError>>,
        >,
    },
    FrameStamp {
        ticket: FontKernelTicket,
        input: FrameStampInput,
        destination: crate::intel::gpgpu::GpgpuRgba8Surface,
        clear_rgba: Option<u32>,
        producer_row: Option<QueuedFontProducerRow>,
        enqueued_ms: u64,
        reply:
            Arc<Signal<crate::wait::EmbassySpinRawMutex, Result<FontFrameStamp, FontKernelError>>>,
    },
}

impl QueuedFontRequest {
    const fn ticket(&self) -> FontKernelTicket {
        match self {
            Self::Retain { ticket, .. }
            | Self::Stamp { ticket, .. }
            | Self::FrameStamp { ticket, .. } => *ticket,
        }
    }

    const fn consumer(&self) -> FontKernelConsumer {
        let path = match self {
            Self::Retain { .. } => FontKernelConsumerPath::RetainScene,
            Self::Stamp { .. } | Self::FrameStamp { .. } => FontKernelConsumerPath::Stamp,
        };
        FontKernelConsumer::new(path, self.ticket().raw())
    }

    /// Requests built from raw text must not enter the service lane until all
    /// their faces have complete registered raw bytes. Prepared plans have
    /// already crossed that boundary and contain no raw face lookup at all.
    async fn wait_for_fonts(&self) {
        match self {
            Self::Retain { request, .. } => {
                wait_for_font_registration(self.ticket(), request.font).await;
            }
            Self::Stamp { request, .. }
            | Self::FrameStamp {
                input: FrameStampInput::Request(request),
                ..
            }
            | Self::FrameStamp {
                input: FrameStampInput::ProducerRequest { request, .. },
                ..
            } => {
                for layer in &request.layers {
                    wait_for_font_registration(self.ticket(), layer.scene.font).await;
                }
            }
            Self::FrameStamp { .. } => {}
        }
    }
}

async fn wait_for_font_registration(ticket: FontKernelTicket, font: GpuFontFace) {
    if font_face_is_available(font) {
        return;
    }
    let started_ms = Instant::now().as_millis();
    crate::log_info!(target: "global";
        "font-kernel-service: registration-wait ticket={} font={} action=hold-request-before-gpu-lane\n",
        ticket.raw(), font.registry_name(),
    );
    wait_for_font_face_available(font).await;
    crate::log_info!(target: "global";
        "font-kernel-service: registration-ready ticket={} font={} waited_ms={} action=resume-request\n",
        ticket.raw(), font.registry_name(), Instant::now().as_millis().saturating_sub(started_ms),
    );
}

struct FrameStampQueueRejection {
    error: FontKernelError,
    input: FrameStampInput,
}

pub(crate) fn status() -> FontKernelServiceStatus {
    let mut status = *STATUS.lock();
    status.online = ONLINE.load(Ordering::Acquire);
    status.queued = REQUESTS.lock().len();
    status
}

pub(crate) fn submit_retain_scene(
    request: RetainSceneRequest,
) -> Result<PendingRetainScene, FontKernelError> {
    validate_retain_request(&request)?;
    let ticket = next_ticket();
    let reply = Arc::new(Signal::new());
    {
        let mut queue = REQUESTS.lock();
        if queue.len() >= FONT_KERNEL_QUEUE_CAPACITY {
            return Err(FontKernelError::QueueFull);
        }
        queue.push_back(QueuedFontRequest::Retain {
            ticket,
            request,
            reply: Arc::clone(&reply),
        });
    }
    {
        let mut status = STATUS.lock();
        status.submitted_retain = status.submitted_retain.saturating_add(1);
    }
    WORK_AVAILABLE.signal(());
    Ok(PendingRetainScene { ticket, reply })
}

pub(crate) fn submit_stamp(request: FontStampRequest) -> Result<PendingFontStamp, FontKernelError> {
    validate_stamp_request(&request)?;
    let ticket = next_ticket();
    let reply = Arc::new(Signal::new());
    {
        let mut queue = REQUESTS.lock();
        if queue.len() >= FONT_KERNEL_QUEUE_CAPACITY {
            return Err(FontKernelError::QueueFull);
        }
        queue.push_back(QueuedFontRequest::Stamp {
            ticket,
            request,
            reply: Arc::clone(&reply),
        });
    }
    {
        let mut status = STATUS.lock();
        status.submitted_stamp = status.submitted_stamp.saturating_add(1);
    }
    WORK_AVAILABLE.signal(());
    Ok(PendingFontStamp { ticket, reply })
}

/// Queue a stamp directly into one caller-owned RGBA8 surface.
///
/// Only canvas-fit requests are admitted because the destination extent and
/// ownership are fixed before submission. The caller must retain its write
/// lease until the returned exact-surface release is published or discarded.
pub(crate) fn submit_frame_stamp(
    request: FontStampRequest,
    destination: crate::intel::gpgpu::GpgpuRgba8Surface,
) -> Result<PendingFontFrameStamp, FontKernelError> {
    validate_stamp_request(&request)?;
    queue_frame_stamp(FrameStampInput::Request(request), destination, None, None)
        .map_err(|rejection| rejection.error)
}

/// Queue a frame whose exact centered glyph plan was sealed during admission.
///
/// The plan service has already validated the request/entry relationship and
/// bounded its work and storage. This boundary performs only constant-time
/// fit, extent, destination, and FIFO-capacity checks. Rejection returns the
/// same plan so ownership never becomes ambiguous under backpressure.
pub(crate) fn submit_prepared_frame_stamp_with_clear(
    plan: PreparedGlyphPlan,
    destination: crate::intel::gpgpu::GpgpuRgba8Surface,
    clear_rgba: u32,
) -> Result<PendingFontFrameStamp, PreparedFrameStampRejection> {
    queue_frame_stamp(FrameStampInput::Prepared(plan), destination, Some(clear_rgba), None).map_err(
        |rejection| {
            let FrameStampInput::Prepared(plan) = rejection.input else {
                unreachable!("prepared frame admission returned a request input")
            };
            PreparedFrameStampRejection {
                error: rejection.error,
                plan,
            }
        },
    )
}

/// Queue a sealed prepared plan over the destination's existing pixels.
///
/// This is intentionally separate from the ordinary clear-and-stamp entry:
/// callers must own a dirty/double-buffer history and mirror accumulated
/// waves across both buffers themselves. Rejection returns the exact plan.
pub(crate) fn submit_prepared_frame_stamp(
    plan: PreparedGlyphPlan,
    destination: crate::intel::gpgpu::GpgpuRgba8Surface,
) -> Result<PendingFontFrameStamp, PreparedFrameStampRejection> {
    queue_frame_stamp(FrameStampInput::Prepared(plan), destination, None, None).map_err(
        |rejection| {
            let FrameStampInput::Prepared(plan) = rejection.input else {
                unreachable!("prepared frame admission returned a request input")
            };
            PreparedFrameStampRejection {
                error: rejection.error,
                plan,
            }
        },
    )
}

/// Queue one Font-owned full-frame clear and return the exact scanout fence.
///
/// This is the blank transition between the title and the cache charge.  It
/// deliberately carries no glyph plan: a visually empty frame must not pay
/// Skrifa, tessellation, coverage, or shading work for a dummy character.
pub(crate) fn submit_font_rush_frame_clear(
    destination: crate::intel::gpgpu::GpgpuRgba8Surface,
    color_rgba: u32,
) -> Result<PendingFontFrameStamp, FontKernelError> {
    queue_frame_stamp(
        FrameStampInput::FontRushClear {
            color_rgba,
            raster_width: destination.width,
            raster_height: destination.height,
        },
        destination,
        None,
        None,
    )
    .map_err(|rejection| rejection.error)
}

/// Clear one caller-owned frame, then scale/tint an immutable Font-owned RGBA8
/// stamp into it through the proven Font sprite worklist.
///
/// The source allocation remains owned by the queued request until the exact
/// destination release is proven. `clear_rgba == 0` gives the Font Rush
/// showcase its transparent base without rebuilding Skrifa coverage for every
/// presentation-size or color change.
pub(crate) fn submit_font_rush_showcase_sprite_frame(
    source: Arc<FontStampedBuffer>,
    descriptors: Vec<crate::intel::gpgpu::GpgpuSpriteQuadWorklistDesc>,
    glyphs: usize,
    destination: crate::intel::gpgpu::GpgpuRgba8Surface,
    clear_rgba: u32,
) -> Result<PendingFontFrameStamp, FontKernelError> {
    queue_frame_stamp(
        FrameStampInput::FontRushRgba8Sprites {
            source,
            descriptors,
            logical_glyphs: glyphs,
            raster_width: destination.width,
            raster_height: destination.height,
        },
        destination,
        Some(clear_rgba),
        None,
    )
    .map_err(|rejection| rejection.error)
}

fn font_frame_gpu_ranges_overlap(
    left: u64,
    left_bytes: usize,
    right: u64,
    right_bytes: usize,
) -> bool {
    let Some(left_end) = left.checked_add(left_bytes as u64) else {
        return true;
    };
    let Some(right_end) = right.checked_add(right_bytes as u64) else {
        return true;
    };
    left < right_end && right < left_end
}

fn font_rush_rgba8_sprite_descriptor_is_valid(
    descriptor: crate::intel::gpgpu::GpgpuSpriteQuadWorklistDesc,
) -> bool {
    const VALID_FLAGS: u32 = crate::intel::gpgpu::SPRITE_QUAD_WORKLIST_FLAG_SRC_OVER
        | crate::intel::gpgpu::SPRITE_QUAD_WORKLIST_FLAG_PREMUL_SRC;
    let coordinates = [
        descriptor.c0_x,
        descriptor.c0_y,
        descriptor.c0_u,
        descriptor.c0_v,
        descriptor.c1_x,
        descriptor.c1_y,
        descriptor.c1_u,
        descriptor.c1_v,
        descriptor.c2_x,
        descriptor.c2_y,
        descriptor.c2_u,
        descriptor.c2_v,
        descriptor.c3_x,
        descriptor.c3_y,
        descriptor.c3_u,
        descriptor.c3_v,
    ];
    if coordinates.iter().any(|coordinate| !coordinate.is_finite())
        || descriptor.flags & !VALID_FLAGS != 0
        || (descriptor.flags & crate::intel::gpgpu::SPRITE_QUAD_WORKLIST_FLAG_PREMUL_SRC != 0
            && descriptor.flags & crate::intel::gpgpu::SPRITE_QUAD_WORKLIST_FLAG_SRC_OVER == 0)
    {
        return false;
    }

    let edge_x = [
        descriptor.c1_x - descriptor.c0_x,
        descriptor.c1_y - descriptor.c0_y,
    ];
    let edge_y = [
        descriptor.c3_x - descriptor.c0_x,
        descriptor.c3_y - descriptor.c0_y,
    ];
    let determinant = edge_x[0] * edge_y[1] - edge_x[1] * edge_y[0];
    determinant.is_finite() && determinant.abs() >= 0.00001
}

fn font_rush_rgba8_sprite_frame_is_valid(
    source: &FontStampedBuffer,
    descriptors: &[crate::intel::gpgpu::GpgpuSpriteQuadWorklistDesc],
    logical_glyphs: usize,
    destination: crate::intel::gpgpu::GpgpuRgba8Surface,
) -> bool {
    let source = source.surface();
    if logical_glyphs == 0
        || logical_glyphs > FONT_STAMP_MAX_GLYPHS
        || descriptors.is_empty()
        || descriptors.len() > crate::intel::gpgpu::sprite_quad_worklist_max_descs()
        || !source.is_valid()
        || source.storage_order != crate::intel::gpgpu::GpgpuRgba8StorageOrder::Rgba
        || !destination.is_valid()
        || destination.storage_order != crate::intel::gpgpu::GpgpuRgba8StorageOrder::Rgba
        || font_frame_gpu_ranges_overlap(
            source.gpu,
            source.bytes,
            destination.gpu,
            destination.bytes,
        )
        || font_frame_gpu_ranges_overlap(
            source.phys,
            source.bytes,
            destination.phys,
            destination.bytes,
        )
    {
        return false;
    }
    descriptors
        .iter()
        .copied()
        .all(font_rush_rgba8_sprite_descriptor_is_valid)
}

fn queue_frame_stamp(
    input: FrameStampInput,
    destination: crate::intel::gpgpu::GpgpuRgba8Surface,
    clear_rgba: Option<u32>,
    producer_row: Option<QueuedFontProducerRow>,
) -> Result<PendingFontFrameStamp, FrameStampQueueRejection> {
    let extent = input.raster_extent();
    let input_is_valid = match &input {
        FrameStampInput::FontRushRgba8Sprites {
            source,
            descriptors,
            logical_glyphs,
            ..
        } => {
            clear_rgba.is_some()
                && font_rush_rgba8_sprite_frame_is_valid(
                    source,
                    descriptors.as_slice(),
                    *logical_glyphs,
                    destination,
                )
        }
        _ => true,
    };
    if input.fit() != FontStampFit::Canvas
        || !destination.is_valid()
        || extent != Some((destination.width, destination.height))
        || !input_is_valid
    {
        return Err(FrameStampQueueRejection {
            error: FontKernelError::InvalidRequest("font-frame-stamp-destination"),
            input,
        });
    }
    let ticket = next_ticket();
    let reply = Arc::new(Signal::new());
    let queued_ahead = {
        let mut queue = REQUESTS.lock();
        if queue.len() >= FONT_KERNEL_QUEUE_CAPACITY {
            return Err(FrameStampQueueRejection {
                error: FontKernelError::QueueFull,
                input,
            });
        }
        let queued_ahead = queue.len();
        queue.push_back(QueuedFontRequest::FrameStamp {
            ticket,
            input,
            destination,
            clear_rgba,
            producer_row,
            enqueued_ms: Instant::now().as_millis(),
            reply: Arc::clone(&reply),
        });
        queued_ahead
    };
    {
        let mut status = STATUS.lock();
        status.submitted_stamp = status.submitted_stamp.saturating_add(1);
    }
    WORK_AVAILABLE.signal(());
    Ok(PendingFontFrameStamp {
        ticket,
        queued_ahead,
        reply,
    })
}

fn next_ticket() -> FontKernelTicket {
    loop {
        let current = NEXT_TICKET.fetch_add(1, Ordering::AcqRel);
        if current != 0 {
            return FontKernelTicket(current);
        }
    }
}

fn validate_retain_request(request: &RetainSceneRequest) -> Result<(), FontKernelError> {
    validate_scene_request(request, FONT_KERNEL_MAX_RUNS)
}

fn validate_scene_request(
    request: &RetainSceneRequest,
    max_runs: usize,
) -> Result<(), FontKernelError> {
    if request.runs.is_empty() || request.runs.len() > max_runs {
        return Err(FontKernelError::InvalidRequest("font-service-run-count"));
    }
    if request.viewport_width == 0
        || request.viewport_height == 0
        || request.raster_width == 0
        || request.raster_height == 0
    {
        return Err(FontKernelError::InvalidRequest("font-service-empty-extent"));
    }
    for run in &request.runs {
        let chars = run.text.chars().count();
        let max_chars = if request.positioning == RetainedFontPositioning::SceneOrigin {
            FONT_STAMP_MAX_GLYPHS
        } else {
            MAX_DYNAMIC_TEXT_CHARS
        };
        if chars == 0 || chars > max_chars {
            return Err(FontKernelError::InvalidRequest("font-service-text-length"));
        }
        if run.text.chars().any(char::is_control)
            || !run.position[0].is_finite()
            || !run.position[1].is_finite()
            || !run.font_pixels.is_finite()
            || run.font_pixels <= 0.0
            || !run.slant.is_finite()
            || run.slant.abs() > 1.0
        {
            return Err(FontKernelError::InvalidRequest("font-service-run"));
        }
    }
    Ok(())
}

fn validate_stamp_request(request: &FontStampRequest) -> Result<(), FontKernelError> {
    if request.layers.is_empty() || request.layers.len() > FONT_KERNEL_MAX_STAMP_LAYERS {
        return Err(FontKernelError::InvalidRequest("font-stamp-layer-count"));
    }
    let first = &request.layers[0].scene;
    if first.raster_width > FONT_STAMP_MAX_EXTENT
        || first.raster_height > FONT_STAMP_MAX_EXTENT
        || u64::from(first.raster_width) * u64::from(first.raster_height) > FONT_STAMP_MAX_PIXELS
    {
        return Err(FontKernelError::InvalidRequest("font-stamp-extent-softcap"));
    }
    let mut glyphs = 0usize;
    let mut runs = 0usize;
    for layer in &request.layers {
        validate_scene_request(&layer.scene, FONT_STAMP_MAX_GLYPHS)?;
        if layer.scene.viewport_width != first.viewport_width
            || layer.scene.viewport_height != first.viewport_height
            || layer.scene.raster_width != first.raster_width
            || layer.scene.raster_height != first.raster_height
            || layer.scene.raster_width > FONT_STAMP_MAX_EXTENT
            || layer.scene.raster_height > FONT_STAMP_MAX_EXTENT
        {
            return Err(FontKernelError::InvalidRequest("font-stamp-layer-extent"));
        }
        runs = runs
            .checked_add(layer.scene.runs.len())
            .ok_or(FontKernelError::InvalidRequest("font-stamp-run-softcap"))?;
        if runs > FONT_STAMP_MAX_GLYPHS {
            return Err(FontKernelError::InvalidRequest("font-stamp-run-softcap"));
        }
        for run in &layer.scene.runs {
            glyphs = glyphs
                .checked_add(run.text.chars().count())
                .ok_or(FontKernelError::InvalidRequest("font-stamp-glyph-softcap"))?;
            if glyphs > FONT_STAMP_MAX_GLYPHS {
                return Err(FontKernelError::InvalidRequest("font-stamp-glyph-softcap"));
            }
        }
    }
    Ok(())
}

fn set_active_stage(ticket: FontKernelTicket, stage: &'static str) {
    let mut status = STATUS.lock();
    status.active_ticket = Some(ticket);
    status.active_stage = stage;
}

fn ensure_font_rcs_lane_available() -> Result<(), FontKernelError> {
    if crate::intel::gpgpu::font_rcs_context_is_quarantined() {
        Err(FontKernelError::Unavailable("font-rcs-context-quarantined"))
    } else {
        Ok(())
    }
}

fn process_retain_scene(
    ticket: FontKernelTicket,
    request: &RetainSceneRequest,
) -> Result<FontKernelRetainedScene, FontKernelError> {
    ensure_font_rcs_lane_available()?;
    let glyph_runs = expand_origin_runs(ticket, request)?;
    let mut masks = Vec::new();
    process_retain_scene_partition(ticket, request, glyph_runs.as_slice(), &mut masks)?;
    Ok(FontKernelRetainedScene { masks })
}

fn expand_origin_runs(
    ticket: FontKernelTicket,
    request: &RetainSceneRequest,
) -> Result<Vec<RetainedFontRun>, FontKernelError> {
    if request.positioning != RetainedFontPositioning::SceneOrigin {
        return Ok(request.runs.clone());
    }

    set_active_stage(ticket, "font-layout");
    ensure_font_face_available(request.font).map_err(FontKernelError::Unavailable)?;
    let mut glyph_runs = Vec::new();
    let mut fallback_glyphs = 0usize;
    let fallback_glyph = if font_face_supports_text(request.font, "\u{2022}") {
        '\u{2022}'
    } else if font_face_supports_text(request.font, "\u{FFFD}") {
        '\u{FFFD}'
    } else if font_face_supports_text(request.font, "?") {
        '?'
    } else {
        return Err(FontKernelError::Unavailable("font-fallback-glyph-missing"));
    };
    for run in &request.runs {
        let mut pen_x = 0.0f32;
        for ch in run.text.chars() {
            let mut glyph = String::new();
            glyph.push(ch);
            if !ch.is_whitespace() && !font_face_supports_text(request.font, glyph.as_str()) {
                glyph.clear();
                glyph.push(fallback_glyph);
                fallback_glyphs = fallback_glyphs.saturating_add(1);
            }
            let advance = crate::graphics::font::text_advance_width(
                request.font.registry_name(),
                glyph.as_str(),
                run.font_pixels,
            )
            .map_err(FontKernelError::Unavailable)?;
            if !ch.is_whitespace() {
                glyph_runs.push(RetainedFontRun {
                    text: glyph,
                    position: [run.position[0] + pen_x, run.position[1]],
                    font_pixels: run.font_pixels,
                    slant: run.slant,
                });
            }
            pen_x += advance;
        }
    }
    if glyph_runs.is_empty() {
        return Err(FontKernelError::Unavailable("font-coverage-empty"));
    }
    crate::log_info!(
        target: "global";
        "font-kernel-service: bounded glyph layout ticket={} source_runs={} glyph_entries={} fallback_glyphs={} fallback=U+{:04X} positioning=scene-origin policy=per-glyph-analytical-coverage\n",
        ticket.raw(),
        request.runs.len(),
        glyph_runs.len(),
        fallback_glyphs,
        u32::from(fallback_glyph),
    );
    Ok(glyph_runs)
}

fn process_retain_scene_partition(
    ticket: FontKernelTicket,
    request: &RetainSceneRequest,
    runs: &[RetainedFontRun],
    masks: &mut Vec<GpuFontRetainedScene>,
) -> Result<(), FontKernelError> {
    match process_retain_scene_runs(ticket, request, runs) {
        Ok(mask) => {
            masks.push(mask);
            Ok(())
        }
        Err(FontKernelError::Unavailable("font-coverage-workload"))
            if runs.len() > 1 && request.positioning == RetainedFontPositioning::SceneOrigin =>
        {
            let midpoint = runs.len() / 2;
            if ticket.raw()
                > LAST_RETAIN_PARTITION_LOG_TICKET.fetch_max(ticket.raw(), Ordering::Relaxed)
            {
                crate::log_info!(
                    target: "global";
                    "font-kernel-service: retain partition ticket={} runs={} split={}+{} reason=font-coverage-workload storage=gpu-vm-r8-layers\n",
                    ticket.raw(),
                    runs.len(),
                    midpoint,
                    runs.len().saturating_sub(midpoint),
                );
            }
            process_retain_scene_partition(ticket, request, &runs[..midpoint], masks)?;
            process_retain_scene_partition(ticket, request, &runs[midpoint..], masks)
        }
        Err(error) => Err(error),
    }
}

fn process_retain_scene_runs(
    ticket: FontKernelTicket,
    request: &RetainSceneRequest,
    runs: &[RetainedFontRun],
) -> Result<GpuFontRetainedScene, FontKernelError> {
    set_active_stage(ticket, "font-warm");
    ensure_font_face_available(request.font).map_err(FontKernelError::Unavailable)?;
    set_active_stage(ticket, "coverage");
    let entries = runs
        .iter()
        .map(|run| GpuFontJobEntry {
            text: GpuFontTextRequest::SingleLine(run.text.as_str()),
            position: run.position,
            font_pixels: run.font_pixels,
            slant: run.slant,
        })
        .collect::<Vec<_>>();
    let result = match request.positioning {
        RetainedFontPositioning::SceneOrigin => retain_gpu_font_scene_at_raster(
            entries.as_slice(),
            request.font,
            request.viewport_width,
            request.viewport_height,
            request.raster_width,
            request.raster_height,
        ),
        RetainedFontPositioning::VisualBoundsCenter => retain_gpu_font_centered_scene_at_raster(
            entries.as_slice(),
            request.font,
            request.viewport_width,
            request.viewport_height,
            request.raster_width,
            request.raster_height,
        ),
    };
    result.map_err(FontKernelError::Unavailable)
}

fn collect_stamp_scenes(
    ticket: FontKernelTicket,
    layer: &FontStampLayer,
    runs: &[RetainedFontRun],
    scenes: &mut Vec<(GpuFontRetainedScene, GpuFontRgba)>,
) -> Result<(), FontKernelError> {
    let scene = match process_retain_scene_runs(ticket, &layer.scene, runs) {
        Ok(scene) => scene,
        Err(FontKernelError::Unavailable("font-coverage-empty")) => return Ok(()),
        Err(FontKernelError::Unavailable("font-coverage-workload"))
            if runs.len() > 1
                && layer.scene.positioning == RetainedFontPositioning::SceneOrigin =>
        {
            let midpoint = runs.len() / 2;
            if ticket.raw()
                > LAST_STAMP_PARTITION_LOG_TICKET.fetch_max(ticket.raw(), Ordering::Relaxed)
            {
                crate::log_info!(
                    target: "global";
                    "font-kernel-service: stamp partition ticket={} runs={} split={}+{} reason=font-coverage-workload destination=gpu-vm-rgba8\n",
                    ticket.raw(),
                    runs.len(),
                    midpoint,
                    runs.len().saturating_sub(midpoint),
                );
            }
            collect_stamp_scenes(ticket, layer, &runs[..midpoint], scenes)?;
            return collect_stamp_scenes(ticket, layer, &runs[midpoint..], scenes);
        }
        Err(error) => return Err(error),
    };
    scenes.push((scene, layer.foreground));
    Ok(())
}

fn tight_stamp_bounds(
    scenes: &[(GpuFontRetainedScene, GpuFontRgba)],
) -> Result<([i32; 2], u32, u32), FontKernelError> {
    let mut union: Option<(i64, i64, i64, i64)> = None;
    for (scene, _) in scenes {
        let origin = scene
            .origin_px()
            .ok_or(FontKernelError::Unavailable("font-stamp-mask-origin"))?;
        let mask = scene
            .mask_surface()
            .ok_or(FontKernelError::Unavailable("font-stamp-mask-surface"))?;
        let bounds = (
            i64::from(origin[0]),
            i64::from(origin[1]),
            i64::from(origin[0]) + i64::from(mask.width),
            i64::from(origin[1]) + i64::from(mask.height),
        );
        union = Some(match union {
            Some(current) => (
                current.0.min(bounds.0),
                current.1.min(bounds.1),
                current.2.max(bounds.2),
                current.3.max(bounds.3),
            ),
            None => bounds,
        });
    }
    let (left, top, right, bottom) =
        union.ok_or(FontKernelError::Unavailable("font-stamp-empty"))?;
    let width = u32::try_from(right - left)
        .map_err(|_| FontKernelError::InvalidRequest("font-stamp-extent-softcap"))?;
    let height = u32::try_from(bottom - top)
        .map_err(|_| FontKernelError::InvalidRequest("font-stamp-extent-softcap"))?;
    if width == 0
        || height == 0
        || width > FONT_STAMP_MAX_EXTENT
        || height > FONT_STAMP_MAX_EXTENT
        || u64::from(width) * u64::from(height) > FONT_STAMP_MAX_PIXELS
    {
        return Err(FontKernelError::InvalidRequest("font-stamp-extent-softcap"));
    }
    let origin = [
        i32::try_from(left)
            .map_err(|_| FontKernelError::InvalidRequest("font-stamp-origin-range"))?,
        i32::try_from(top)
            .map_err(|_| FontKernelError::InvalidRequest("font-stamp-origin-range"))?,
    ];
    Ok((origin, width, height))
}

fn prepare_stamp_scenes(
    ticket: FontKernelTicket,
    request: &FontStampRequest,
) -> Result<(Vec<(GpuFontRetainedScene, GpuFontRgba)>, usize), FontKernelError> {
    let mut scenes = Vec::new();
    let mut glyphs = 0usize;
    let mut transparent_layers = 0usize;
    for layer in &request.layers {
        glyphs = layer
            .scene
            .runs
            .iter()
            .fold(glyphs, |total, run| total.saturating_add(run.text.chars().count()));
        let glyph_runs = match expand_origin_runs(ticket, &layer.scene) {
            Ok(glyph_runs) => glyph_runs,
            Err(FontKernelError::Unavailable("font-coverage-empty")) => {
                transparent_layers = transparent_layers.saturating_add(1);
                continue;
            }
            Err(error) => return Err(error),
        };
        let scenes_before = scenes.len();
        collect_stamp_scenes(ticket, layer, glyph_runs.as_slice(), &mut scenes)?;
        if scenes.len() == scenes_before {
            transparent_layers = transparent_layers.saturating_add(1);
        }
    }
    if scenes.is_empty() {
        return Err(FontKernelError::Unavailable("font-coverage-empty"));
    }
    if transparent_layers != 0 {
        crate::log_info!(
            target: "render";
            "font-kernel-service: stamp transparent layers ticket={} layers={} transparent_layers={} retained_layers={} reason=font-coverage-empty action=skip-empty-layer\n",
            ticket.raw(),
            request.layers.len(),
            transparent_layers,
            scenes.len(),
        );
    }
    Ok((scenes, glyphs))
}

/// Build producer-row coverage from generation-owned glyph-local recipes.
/// Recipes intentionally contain neither foreground color nor absolute
/// placement. The current request supplies both, then the normal R8 coverage
/// and final premultiplied stamp paths remain unchanged.
fn prepare_producer_stamp_scenes(
    ticket: FontKernelTicket,
    request: &FontStampRequest,
    resources: &Arc<FontGpuProducerResources>,
) -> Result<(Vec<(GpuFontRetainedScene, GpuFontRgba)>, usize, (u64, u64, u64), bool), FontKernelError>
{
    // Producer rows use a one-to-one viewport/raster contract, so ppem is the
    // registered font pixel size.  The origin recipe path rounds
    // `position_y + ppem`, while the legacy path rounds position and applies
    // ppem afterwards. Those are exactly equivalent for integral ppem only.
    // Keep fractional static tiers on the established uncached path until the
    // recipe carries the legacy split-rounding representation.
    if request.layers.iter().any(|layer| {
        layer
            .scene
            .runs
            .iter()
            .any(|run| run.font_pixels != libm::truncf(run.font_pixels))
    }) {
        let (scenes, glyphs) = prepare_stamp_scenes(ticket, request)?;
        return Ok((scenes, glyphs, (0, 0, 0), false));
    }
    let mut scenes = Vec::with_capacity(request.layers.len());
    let mut glyphs = 0usize;
    let before = resources.glyph_cache.lock().diagnostics();
    for layer in &request.layers {
        // Registered row requests deliberately require SceneOrigin. Keep the
        // generic path below as a defensive compatibility fallback if a
        // future producer contract broadens that rule.
        if layer.scene.positioning != RetainedFontPositioning::SceneOrigin {
            let (scenes, glyphs) = prepare_stamp_scenes(ticket, request)?;
            return Ok((scenes, glyphs, (0, 0, 0), false));
        }
        let glyph_runs = expand_origin_runs(ticket, &layer.scene)?;
        let prepared = {
            let mut cache = resources.glyph_cache.lock();
            let mut prepared = Vec::with_capacity(glyph_runs.len());
            for run in glyph_runs {
                let scalar = run
                    .text
                    .chars()
                    .next()
                    .ok_or(FontKernelError::Unavailable("font-coverage-empty"))?;
                let key = gpu_font_centered_glyph_recipe_key(
                    scalar,
                    layer.scene.font,
                    run.font_pixels,
                    run.slant,
                    layer.scene.viewport_width,
                    layer.scene.viewport_height,
                    layer.scene.raster_width,
                    layer.scene.raster_height,
                )
                .map_err(FontKernelError::Unavailable)?;
                let (recipe, _) = cache
                    .get_or_build(key)
                    .map_err(FontKernelError::Unavailable)?;
                prepared.push(
                    place_gpu_font_origin_glyph_recipe(
                        scalar,
                        recipe,
                        run.position,
                        run.font_pixels,
                        run.slant,
                        layer.scene.viewport_width,
                        layer.scene.viewport_height,
                        layer.scene.raster_width,
                        layer.scene.raster_height,
                    )
                    .map_err(FontKernelError::Unavailable)?,
                );
            }
            prepared
        };
        glyphs = glyphs.saturating_add(prepared.len());
        let scene = retain_gpu_font_prepared_origin_scene(prepared)
            .map_err(FontKernelError::Unavailable)?;
        scenes.push((scene, layer.foreground));
    }
    let after = resources.glyph_cache.lock().diagnostics();
    Ok((
        scenes,
        glyphs,
        (
            after.2.saturating_sub(before.2),
            after.3.saturating_sub(before.3),
            after.4.saturating_sub(before.4),
        ),
        true,
    ))
}

fn allocate_font_stamp_output(
    ticket: FontKernelTicket,
    width: u32,
    height: u32,
) -> Result<crate::intel::gpgpu::GpgpuOwnedRgba8Surface, FontKernelError> {
    let Some(storage) = crate::intel::gpgpu::allocate_font_instance_rgba8_surface(width, height)
    else {
        let stats = crate::phys::pmm_stats();
        crate::log_warn!(
            target: "global";
            "font-kernel-service: output allocation rejected ticket={} extent={}x{} rgba_bytes={} pmm_free_bytes={} pmm_largest_free_bytes={} pmm_free_regions={} admission=before-temporary-masks\n",
            ticket.raw(),
            width,
            height,
            u64::from(width).saturating_mul(u64::from(height)).saturating_mul(4),
            stats.map_or(0, |value| value.free_bytes),
            stats.map_or(0, |value| value.largest_free_region),
            stats.map_or(0, |value| value.free_regions),
        );
        return Err(FontKernelError::Unavailable("font-stamp-output-allocation"));
    };
    Ok(storage)
}

fn process_stamp(
    ticket: FontKernelTicket,
    request: &FontStampRequest,
) -> Result<FontStampedBuffer, FontKernelError> {
    ensure_font_rcs_lane_available()?;
    // A canvas-fit request already names its exact final allocation. Reserve
    // that largest contiguous resource before temporary retained masks can
    // fragment the shared DMA/Font-RCS VA arenas. This also keeps the
    // admission order aligned with the RenderTicket contract: secure the
    // destination first, then admit dependent GPU work. Tight-fit requests
    // still have to derive their bounds from the prepared masks.
    let canvas_output = if request.fit == FontStampFit::Canvas {
        let scene = &request.layers[0].scene;
        set_active_stage(ticket, "output-allocate");
        Some(allocate_font_stamp_output(ticket, scene.raster_width, scene.raster_height)?)
    } else {
        None
    };
    let (scenes, glyphs) = prepare_stamp_scenes(ticket, request)?;
    let (origin_px, storage) = match (request.fit, canvas_output) {
        (FontStampFit::Canvas, Some(storage)) => ([0, 0], storage),
        (FontStampFit::Tight, None) => {
            let (origin_px, width, height) = tight_stamp_bounds(scenes.as_slice())?;
            set_active_stage(ticket, "output-allocate");
            let storage = allocate_font_stamp_output(ticket, width, height)?;
            (origin_px, storage)
        }
        _ => return Err(FontKernelError::InvalidRequest("font-stamp-fit-state")),
    };
    let surface = storage.surface();
    // The owned RGBA allocation is zeroed and DMA-flushed before return.
    // Dispatching another GPU clear here only adds direct-RCS contention.
    let translation = [
        origin_px[0]
            .checked_neg()
            .ok_or(FontKernelError::InvalidRequest("font-stamp-origin-range"))?,
        origin_px[1]
            .checked_neg()
            .ok_or(FontKernelError::InvalidRequest("font-stamp-origin-range"))?,
    ];
    let mut submits = 0usize;
    let mut active_walkers = 0usize;
    for (scene, foreground) in scenes {
        set_active_stage(ticket, "instance");
        let rendered = match scene.restamp_identity(surface, translation, foreground, false) {
            Ok(rendered) => rendered,
            Err(error) => {
                let error = FontKernelError::from(error);
                if matches!(error, FontKernelError::SubmittedIncomplete(_)) {
                    core::mem::forget(storage);
                }
                return Err(error);
            }
        };
        submits = submits.saturating_add(rendered.submits);
        active_walkers = active_walkers.saturating_add(rendered.active_walkers);
    }
    Ok(FontStampedBuffer {
        ticket,
        storage,
        origin_px,
        glyphs,
        submits,
        active_walkers,
    })
}

fn validate_frame_clear_outcome(
    outcome: crate::intel::gpgpu::GpgpuSubmissionOutcome,
) -> Result<(), FontKernelError> {
    match outcome {
        crate::intel::gpgpu::GpgpuSubmissionOutcome::Complete => Ok(()),
        crate::intel::gpgpu::GpgpuSubmissionOutcome::SubmittedIncomplete => {
            Err(FontKernelError::SubmittedIncomplete("font-frame-clear-submit-incomplete"))
        }
        crate::intel::gpgpu::GpgpuSubmissionOutcome::Unavailable => {
            Err(FontKernelError::Unavailable("font-frame-clear-unavailable"))
        }
    }
}

enum FontFrameCoverage {
    Retained(Vec<(GpuFontRetainedScene, GpuFontRgba)>),
}

fn process_font_rush_frame_clear(
    ticket: FontKernelTicket,
    color_rgba: u32,
    destination: crate::intel::gpgpu::GpgpuRgba8Surface,
    enqueued_ms: u64,
) -> Result<FontFrameStamp, FontKernelError> {
    let service_started_ms = Instant::now().as_millis();
    ensure_font_rcs_lane_available()?;
    if !destination.is_valid() {
        return Err(FontKernelError::InvalidRequest("font-rush-clear-destination"));
    }

    set_active_stage(ticket, "font-rush-blank-clear");
    let clear_started_ms = Instant::now().as_millis();
    let cleared = crate::intel::gpgpu::font_fill_solid_rect_rgba8_scanout_result(
        destination,
        crate::intel::gpgpu::GpgpuSolidRect {
            rect: destination.bounds(),
            color_rgba,
        },
    );
    validate_frame_clear_outcome(cleared.outcome)?;
    let clear_ms = Instant::now().as_millis().saturating_sub(clear_started_ms);

    // The clear has already changed the leased frame.  If the release packet
    // cannot be proven, report an irreversible completion so UI4 retains the
    // exact write lease rather than recycling a possibly live destination.
    set_active_stage(ticket, "font-rush-blank-release");
    let finalized = crate::intel::gpgpu::font_release_rgba8_surface_for_scanout(destination);
    if !finalized.ok {
        return Err(FontKernelError::SubmittedIncomplete("font-rush-clear-release-incomplete"));
    }
    let release = finalized
        .release
        .ok_or(FontKernelError::SubmittedIncomplete("font-rush-clear-release-missing"))?;
    let completed_ms = Instant::now().as_millis();
    crate::log_info!(
        target: "render";
        "font-kernel-service: font-rush blank complete ticket={} clear=0x{:08X} extent={}x{} clear_submits={} release_submits={} release={} pre_service_ms={} clear_ms={} service_ms={} planning=0 skrifa=0 tessellation=0 coverage=0 shading=0\n",
        ticket.raw(),
        color_rgba,
        destination.width,
        destination.height,
        cleared.stats.submits,
        usize::from(finalized.submitted),
        release.sequence(),
        service_started_ms.saturating_sub(enqueued_ms),
        clear_ms,
        completed_ms.saturating_sub(service_started_ms),
    );
    Ok(FontFrameStamp {
        ticket,
        glyphs: 0,
        submits: usize::from(finalized.submitted),
        clear_submits: cleared.stats.submits,
        active_walkers: cleared.stats.walkers,
        pre_service_ms: service_started_ms.saturating_sub(enqueued_ms),
        clear_ms,
        prepare_coverage_ms: 0,
        coverage_build_ms: 0,
        coverage_audit_ms: 0,
        coverage_submits: 0,
        instance_release_ms: completed_ms.saturating_sub(clear_started_ms),
        total_service_ms: completed_ms.saturating_sub(service_started_ms),
        release,
    })
}

fn process_font_rush_rgba8_sprite_frame(
    ticket: FontKernelTicket,
    source: Arc<FontStampedBuffer>,
    descriptors: Vec<crate::intel::gpgpu::GpgpuSpriteQuadWorklistDesc>,
    logical_glyphs: usize,
    destination: crate::intel::gpgpu::GpgpuRgba8Surface,
    clear_rgba: u32,
    enqueued_ms: u64,
) -> Result<FontFrameStamp, FontKernelError> {
    let service_started_ms = Instant::now().as_millis();
    let pre_service_ms = service_started_ms.saturating_sub(enqueued_ms);
    ensure_font_rcs_lane_available()?;
    if !font_rush_rgba8_sprite_frame_is_valid(
        &source,
        descriptors.as_slice(),
        logical_glyphs,
        destination,
    ) {
        return Err(FontKernelError::InvalidRequest("font-rush-sprite-frame-contract"));
    }

    // Allocate, upload, and map the immutable sprite worklist resources while
    // this request is still reversible.  A cold-path failure after the clear
    // would leave the caller's exact frame allocation partially mutated.
    set_active_stage(ticket, "font-rush-showcase-sprite-warm");
    if !crate::intel::gpgpu::prepare_font_sprite_quad_worklist_rgba8() {
        return Err(FontKernelError::Unavailable("font-rush-showcase-sprite-warm"));
    }

    set_active_stage(ticket, "font-rush-showcase-clear");
    let clear_started_ms = Instant::now().as_millis();
    let cleared = crate::intel::gpgpu::font_fill_solid_rect_rgba8_scanout_result(
        destination,
        crate::intel::gpgpu::GpgpuSolidRect {
            rect: destination.bounds(),
            color_rgba: clear_rgba,
        },
    );
    validate_frame_clear_outcome(cleared.outcome)?;
    let clear_ms = Instant::now().as_millis().saturating_sub(clear_started_ms);

    set_active_stage(ticket, "font-rush-showcase-sprite");
    let sprite_started_ms = Instant::now().as_millis();
    let source_surface = source.surface();
    let runs = [crate::intel::gpgpu::GpgpuSpriteQuadWorklistRun {
        src: source_surface,
        descs: descriptors.as_slice(),
    }];
    let rendered = crate::intel::gpgpu::font_sprite_quad_worklist_rgba8_runs_over_result(
        destination,
        runs.as_slice(),
    );
    match rendered.outcome {
        crate::intel::gpgpu::GpgpuSubmissionOutcome::SubmittedIncomplete => {
            // A late Font-context read may still address this exact source.
            // Preserve its allocation just as UI4 preserves the mutated write
            // lease after receiving SubmittedIncomplete.
            core::mem::forget(source);
            return Err(FontKernelError::SubmittedIncomplete(
                "font-rush-sprite-frame-submit-incomplete",
            ));
        }
        crate::intel::gpgpu::GpgpuSubmissionOutcome::Unavailable => {
            // The successful clear already changed the caller's frame, so an
            // otherwise reversible sprite admission failure is irreversible
            // at the frame boundary.
            return Err(FontKernelError::SubmittedIncomplete(
                "font-rush-sprite-frame-after-clear-unavailable",
            ));
        }
        crate::intel::gpgpu::GpgpuSubmissionOutcome::Complete => {}
    }
    if rendered.stats.descs != descriptors.len() {
        return Err(FontKernelError::SubmittedIncomplete("font-rush-sprite-frame-count-mismatch"));
    }
    let release = rendered
        .release
        .ok_or(FontKernelError::SubmittedIncomplete("font-rush-sprite-frame-release-missing"))?;
    let completed_ms = Instant::now().as_millis();
    crate::log_info!(
        target: "render";
        "font-kernel-service: font-rush showcase sprite complete ticket={} source_ticket={} logical_glyphs={} sprites={} source={}x{} destination={}x{} clear=0x{:08X} clear_submits={} sprite_submits={} walkers={} release={} pre_service_ms={} clear_ms={} sprite_ms={} service_ms={} prewarm=1 path=resident-rgba8->scaled-tinted-font-sprite planning=0 skrifa=0 tessellation=0 coverage=0 shading=1\n",
        ticket.raw(),
        source.ticket().raw(),
        logical_glyphs,
        rendered.stats.descs,
        source_surface.width,
        source_surface.height,
        destination.width,
        destination.height,
        clear_rgba,
        cleared.stats.submits,
        rendered.stats.submits,
        rendered.stats.walkers,
        release.sequence(),
        pre_service_ms,
        clear_ms,
        completed_ms.saturating_sub(sprite_started_ms),
        completed_ms.saturating_sub(service_started_ms),
    );
    Ok(FontFrameStamp {
        ticket,
        glyphs: logical_glyphs,
        submits: rendered.stats.submits,
        clear_submits: cleared.stats.submits,
        active_walkers: rendered.stats.walkers,
        pre_service_ms,
        clear_ms,
        prepare_coverage_ms: 0,
        coverage_build_ms: 0,
        coverage_audit_ms: 0,
        coverage_submits: 0,
        instance_release_ms: completed_ms.saturating_sub(sprite_started_ms),
        total_service_ms: completed_ms.saturating_sub(service_started_ms),
        release,
    })
}

fn process_frame_stamp(
    ticket: FontKernelTicket,
    input: FrameStampInput,
    destination: crate::intel::gpgpu::GpgpuRgba8Surface,
    clear_rgba: Option<u32>,
    enqueued_ms: u64,
) -> Result<FontFrameStamp, FontKernelError> {
    let input = match input {
        FrameStampInput::FontRushClear { color_rgba, .. } => {
            return process_font_rush_frame_clear(ticket, color_rgba, destination, enqueued_ms);
        }
        FrameStampInput::FontRushRgba8Sprites {
            source,
            descriptors,
            logical_glyphs,
            ..
        } => {
            let clear_rgba = clear_rgba
                .ok_or(FontKernelError::InvalidRequest("font-rush-sprite-frame-clear"))?;
            return process_font_rush_rgba8_sprite_frame(
                ticket,
                source,
                descriptors,
                logical_glyphs,
                destination,
                clear_rgba,
                enqueued_ms,
            );
        }
        input => input,
    };

    use crate::intel::gpgpu::GpgpuSolidRect;

    let service_started_ms = Instant::now().as_millis();
    let pre_service_ms = service_started_ms.saturating_sub(enqueued_ms);
    ensure_font_rcs_lane_available()?;
    set_active_stage(ticket, "frame-prepare-coverage");
    let prepare_started_ms = Instant::now().as_millis();
    let (coverage, glyphs, coverage_input, producer_cache) = match input {
        FrameStampInput::Prepared(plan) => {
            let glyphs = plan.glyph_count();
            let foreground = plan.foreground();
            let prepared = plan.into_prepared();
            let scene = retain_gpu_font_prepared_centered_scene(prepared)
                .map_err(FontKernelError::Unavailable)?;
            (
                FontFrameCoverage::Retained(alloc::vec![(scene, foreground)]),
                glyphs,
                "prepared-plan-union-coverage",
                None,
            )
        }
        FrameStampInput::Request(request) => {
            let (scenes, glyphs) = prepare_stamp_scenes(ticket, &request)?;
            (FontFrameCoverage::Retained(scenes), glyphs, "request-outline", None)
        }
        FrameStampInput::ProducerRequest {
            request,
            glyph_cache,
        } => {
            let (scenes, glyphs, delta, cache_used) =
                prepare_producer_stamp_scenes(ticket, &request, &glyph_cache)?;
            let diagnostics = cache_used.then(|| glyph_cache.glyph_cache.lock().diagnostics());
            (
                FontFrameCoverage::Retained(scenes),
                glyphs,
                if cache_used {
                    "producer-recipe-cache"
                } else {
                    "producer-outline-fallback"
                },
                diagnostics.map(|diagnostics| (diagnostics, delta)),
            )
        }
        FrameStampInput::FontRushClear { .. } | FrameStampInput::FontRushRgba8Sprites { .. } => {
            unreachable!("font-rush special frame was dispatched before coverage preparation")
        }
    };
    let prepare_coverage_ms = Instant::now()
        .as_millis()
        .saturating_sub(prepare_started_ms);
    let (coverage_build_ms, coverage_audit_ms, coverage_submits, scene_count) = match &coverage {
        FontFrameCoverage::Retained(scenes) => (
            scenes
                .iter()
                .fold(0u64, |total, (scene, _)| total.saturating_add(scene.coverage_build_ms())),
            scenes
                .iter()
                .fold(0u64, |total, (scene, _)| total.saturating_add(scene.coverage_audit_ms())),
            scenes
                .iter()
                .fold(0usize, |total, (scene, _)| total.saturating_add(scene.coverage_submits())),
            scenes.len(),
        ),
    };
    let cache_log = if let Some((diagnostics, delta)) = producer_cache {
        alloc::format!(
            "cache=producer-recipe-colorless budget_bytes={} used_bytes={} entries={} hits={} misses={} uncached={} request_hits={} request_misses={} request_uncached={}",
            FONT_PRODUCER_GLYPH_CACHE_BYTES,
            diagnostics.1,
            diagnostics.0,
            diagnostics.2,
            diagnostics.3,
            diagnostics.4,
            delta.0,
            delta.1,
            delta.2,
        )
    } else {
        String::from("cache=none")
    };
    crate::log_info!(
        target: "render";
        "font-kernel-service: frame coverage ticket={} input={} glyphs={} scenes={} prepare_coverage_ms={} coverage_build_ms={} coverage_audit_ms={} coverage_submits={} {}\n",
        ticket.raw(),
        coverage_input,
        glyphs,
        scene_count,
        prepare_coverage_ms,
        coverage_build_ms,
        coverage_audit_ms,
        coverage_submits,
        cache_log,
    );

    // Coverage admission is still reversible: until every mask exists, the
    // caller's leased destination must remain byte-for-byte untouched.  Clear
    // only after preparation succeeds so an unsupported/over-budget glyph
    // cannot consume the frame and strand an unreplayable partial request.
    let mut clear_submits = 0usize;
    let clear_ms = if let Some(color_rgba) = clear_rgba {
        set_active_stage(ticket, "frame-clear-irreversible");
        let clear_started_ms = Instant::now().as_millis();
        let clear = GpgpuSolidRect {
            rect: destination.bounds(),
            color_rgba,
        };
        let cleared =
            crate::intel::gpgpu::font_fill_solid_rect_rgba8_scanout_result(destination, clear);
        clear_submits = cleared.stats.submits;
        let elapsed_ms = Instant::now().as_millis().saturating_sub(clear_started_ms);
        validate_frame_clear_outcome(cleared.outcome)?;
        elapsed_ms
    } else {
        0
    };

    let mut submits = 0usize;
    let mut active_walkers = 0usize;
    let mut release = None;
    let instance_started_ms = Instant::now().as_millis();
    match coverage {
        FontFrameCoverage::Retained(scenes) => {
            for (scene, foreground) in scenes {
                set_active_stage(ticket, "frame-instance");
                let rendered = match scene.restamp_identity(destination, [0, 0], foreground, true) {
                    Ok(rendered) => rendered,
                    Err(error) => {
                        let error = FontKernelError::from(error);
                        if (clear_rgba.is_some() || submits != 0)
                            && matches!(error, FontKernelError::Unavailable(_))
                        {
                            return Err(FontKernelError::SubmittedIncomplete(
                                "font-frame-after-clear-partial",
                            ));
                        }
                        return Err(error);
                    }
                };
                submits = submits.saturating_add(rendered.submits);
                active_walkers = active_walkers.saturating_add(rendered.active_walkers);
                if rendered.release.is_some() {
                    release = rendered.release;
                }
            }
        }
    }
    let release = release.ok_or(if clear_rgba.is_some() || submits != 0 {
        FontKernelError::SubmittedIncomplete("font-frame-stamp-release-missing")
    } else {
        FontKernelError::Unavailable("font-frame-stamp-release-missing")
    })?;
    let completed_ms = Instant::now().as_millis();
    Ok(FontFrameStamp {
        ticket,
        glyphs,
        submits,
        clear_submits,
        active_walkers,
        pre_service_ms,
        clear_ms,
        prepare_coverage_ms,
        coverage_build_ms,
        coverage_audit_ms,
        coverage_submits,
        instance_release_ms: completed_ms.saturating_sub(instance_started_ms),
        total_service_ms: completed_ms.saturating_sub(service_started_ms),
        release,
    })
}

fn complete_status(ticket: FontKernelTicket, retain: bool, succeeded: bool) {
    let mut status = STATUS.lock();
    if succeeded {
        if retain {
            status.completed_retain = status.completed_retain.saturating_add(1);
        } else {
            status.completed_stamp = status.completed_stamp.saturating_add(1);
        }
    } else {
        status.failed = status.failed.saturating_add(1);
    }
    if status.active_ticket == Some(ticket) {
        status.active_ticket = None;
        status.active_stage = "idle";
    }
}

fn log_failure(ticket: FontKernelTicket, operation: &'static str, error: &FontKernelError) {
    let stage = STATUS.lock().active_stage;
    let queued = REQUESTS.lock().len();
    crate::log_warn!(
        target: "global";
        "font-kernel-service: {} failed ticket={} stage={} reason={:?} queued={} action=signal-caller+keep-service-online\n",
        operation,
        ticket.raw(),
        stage,
        error,
        queued,
    );
}

fn retryable_gpu_error(error: &FontKernelError) -> bool {
    !crate::intel::gpgpu::font_rcs_context_is_quarantined()
        && matches!(
            error,
            FontKernelError::Unavailable(
                "font-coverage-dispatch"
                    | "font-retained-identity-restamp-unavailable"
                    | "font-retained-instance-restamp-unavailable"
            )
        )
}

fn record_gpu_retry(ticket: FontKernelTicket, operation: &'static str, error: &FontKernelError) {
    let retry = {
        let mut status = STATUS.lock();
        status.gpu_retries = status.gpu_retries.saturating_add(1);
        if status.active_ticket == Some(ticket) {
            status.active_ticket = None;
            status.active_stage = "idle";
        }
        status.gpu_retries
    };
    if retry <= 8 || retry.is_multiple_of(120) {
        crate::log_info!(
            target: "render";
            "font-kernel-service: {} deferred ticket={} reason={:?} gpu_retry={} queued={} retry_ms={} action=requeue-ticket+pace-font-lane\n",
            operation,
            ticket.raw(),
            error,
            retry,
            REQUESTS.lock().len().saturating_add(1),
            FONT_KERNEL_GPU_RETRY_MS,
        );
    }
}

fn process_queued_request(request: QueuedFontRequest) {
    match request {
        QueuedFontRequest::Retain {
            ticket,
            request,
            reply,
        } => {
            set_active_stage(ticket, "dispatch");
            let result = process_retain_scene(ticket, &request);
            if let Err(error) = &result
                && retryable_gpu_error(error)
            {
                record_gpu_retry(ticket, "retain", error);
                GPU_RETRY_DELAY_PENDING.store(true, Ordering::Release);
                REQUESTS.lock().push_back(QueuedFontRequest::Retain {
                    ticket,
                    request,
                    reply,
                });
                IN_FLIGHT.store(false, Ordering::Release);
                WORK_AVAILABLE.signal(());
                return;
            }
            if let Err(error) = &result {
                log_failure(ticket, "retain", error);
            }
            complete_status(ticket, true, result.is_ok());
            crate::log_info!(
                target: "render";
                "font-kernel-service: retain complete ticket={} ok={} queued={}\n",
                ticket.raw(),
                result.is_ok() as u8,
                REQUESTS.lock().len(),
            );
            reply.signal(result);
        }
        QueuedFontRequest::Stamp {
            ticket,
            request,
            reply,
        } => {
            set_active_stage(ticket, "dispatch");
            let result = process_stamp(ticket, &request);
            if let Err(error) = &result
                && retryable_gpu_error(error)
            {
                record_gpu_retry(ticket, "stamp", error);
                GPU_RETRY_DELAY_PENDING.store(true, Ordering::Release);
                REQUESTS.lock().push_back(QueuedFontRequest::Stamp {
                    ticket,
                    request,
                    reply,
                });
                IN_FLIGHT.store(false, Ordering::Release);
                WORK_AVAILABLE.signal(());
                return;
            }
            if let Err(error) = &result {
                log_failure(ticket, "stamp", error);
            }
            complete_status(ticket, false, result.is_ok());
            crate::log_info!(
                target: "render";
                "font-kernel-service: stamp complete ticket={} ok={} queued={}\n",
                ticket.raw(),
                result.is_ok() as u8,
                REQUESTS.lock().len(),
            );
            reply.signal(result);
        }
        QueuedFontRequest::FrameStamp {
            ticket,
            input,
            destination,
            clear_rgba,
            producer_row,
            enqueued_ms,
            reply,
        } => {
            set_active_stage(ticket, "dispatch");
            let result = process_frame_stamp(ticket, input, destination, clear_rgba, enqueued_ms);
            if let Some(producer_row) = producer_row {
                match &result {
                    Ok(stamp) if stamp.release().matches(destination.phys, destination.bytes) => {
                        let completion = crate::r::font_producer_service::FontRowCompletion {
                            release_fence: stamp.release().sequence(),
                            metadata: stamp.ticket().raw(),
                        };
                        if crate::r::font_producer_service::mark_producer_row_gpu_complete(
                            producer_row.token,
                            completion,
                        )
                        .is_err()
                        {
                            let _ = crate::r::font_producer_service::abandon_producer_row(
                                producer_row.token,
                            );
                            core::mem::forget(producer_row.resources);
                        }
                    }
                    Ok(_) | Err(FontKernelError::SubmittedIncomplete(_)) => {
                        let _ = crate::r::font_producer_service::quarantine_producer_row(
                            producer_row.token,
                        );
                        core::mem::forget(producer_row.resources);
                    }
                    Err(_) => {
                        let _ = crate::r::font_producer_service::cancel_reserved_producer_row(
                            producer_row.token,
                        );
                    }
                }
            }
            // A destination stamp is not replayed: an earlier ordered layer
            // may already have retired into the leased frame, so retrying the
            // whole source-over sequence would composite it twice.
            if let Err(error) = &result {
                log_failure(ticket, "frame-stamp", error);
            }
            complete_status(ticket, false, result.is_ok());
            crate::log_info!(
                target: "render";
                "font-kernel-service: frame-stamp complete ticket={} ok={} queued={}\n",
                ticket.raw(),
                result.is_ok() as u8,
                REQUESTS.lock().len(),
            );
            reply.signal(result);
        }
    }
    IN_FLIGHT.store(false, Ordering::Release);
    WORK_AVAILABLE.signal(());
}

fn dispatch_to_service_lane(
    request: QueuedFontRequest,
    gpu_lane: FontKernelGpuLease,
) -> Result<(), QueuedFontRequest> {
    let shared_request = Arc::new(Mutex::new(Some(request)));
    let worker_request = Arc::clone(&shared_request);
    let job = Box::new(move || {
        let _gpu_lane = gpu_lane;
        if let Some(request) = worker_request.lock().take() {
            process_queued_request(request);
        }
    });
    match crate::r::blocking::try_spawn_blocking_job_with_purpose(job, "font-kernel-service") {
        Ok(()) => Ok(()),
        Err(job) => {
            drop(job);
            Err(shared_request
                .lock()
                .take()
                .expect("rejected font service-lane job retained its request"))
        }
    }
}

#[trueos_executor::task]
pub(crate) async fn font_kernel_service_task() {
    ONLINE.store(true, Ordering::Release);
    crate::log_info!(
        target: "render";
        "font-kernel-service: online paths=retain-scene+async-stamp+async-frame-stamp+prepared-frame-stamp+registered-persistent-row+font-rush-clear-only+font-rush-showcase-rgba8-sprite controller=bsp worker=leased-blocking-service-lane font_lane=fair-fifo-font-only gpu_context=kernel-gpgpu-font queue_capacity={} producer_slots=32 producer_rows=persistent-generation-tagged-ack-credit retained_storage=gpu-vm-r8 prepared_storage=bounded-transient-move-once glyph_cache=per-producer-colorless-recipe-first-fill-no-evict cache_budget_bytes={} theoretical_32_cache_bytes={} r8_atlas_cache=not-yet stamp_output=owned-or-ui4-leased-gpu-vm-rgba8 completion=signal\n",
        FONT_KERNEL_QUEUE_CAPACITY,
        FONT_PRODUCER_GLYPH_CACHE_BYTES,
        FONT_PRODUCER_GLYPH_CACHE_THEORETICAL_32_BYTES,
    );
    loop {
        if GPU_RETRY_DELAY_PENDING.swap(false, Ordering::AcqRel) {
            Timer::after(EmbassyDuration::from_millis(FONT_KERNEL_GPU_RETRY_MS)).await;
        }
        if IN_FLIGHT.load(Ordering::Acquire) {
            WORK_AVAILABLE.wait().await;
            continue;
        }
        let Some(request) = REQUESTS.lock().pop_front() else {
            WORK_AVAILABLE.wait().await;
            continue;
        };
        let ticket = request.ticket();
        set_active_stage(ticket, "font-registration-wait");
        request.wait_for_fonts().await;
        set_active_stage(ticket, "lane-admission");
        let consumer = request.consumer();
        let gpu_lane = acquire_gpu_lane(consumer).await;
        IN_FLIGHT.store(true, Ordering::Release);
        if let Err(request) = dispatch_to_service_lane(request, gpu_lane) {
            IN_FLIGHT.store(false, Ordering::Release);
            REQUESTS.lock().push_front(request);
            {
                let mut status = STATUS.lock();
                status.active_stage = "lane-wait";
                status.lane_retries = status.lane_retries.saturating_add(1);
            }
            Timer::after(EmbassyDuration::from_millis(FONT_KERNEL_LANE_RETRY_MS)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> RetainSceneRequest {
        RetainSceneRequest {
            runs: alloc::vec![RetainedFontRun {
                text: String::from("retained"),
                position: [20.0, 30.0],
                font_pixels: 24.0,
                slant: 0.0,
            }],
            font: GpuFontFace::Default,
            viewport_width: 256,
            viewport_height: 128,
            raster_width: 256,
            raster_height: 128,
            positioning: RetainedFontPositioning::SceneOrigin,
        }
    }

    #[test]
    fn retained_request_accepts_owned_runs() {
        assert_eq!(validate_retain_request(&request()), Ok(()));
    }

    #[test]
    fn producer_recipe_cache_retirement_frees_budget_and_closes_refill() {
        let mut cache = ProducerGlyphRecipeCache::new();
        assert_eq!(FONT_PRODUCER_GLYPH_CACHE_BYTES * 32, 2_560 * 1024);
        assert_eq!(cache.diagnostics(), (0, 0, 0, 0, 0));
        assert!(cache.accepting_fills);
        cache.retire();
        assert_eq!(cache.diagnostics(), (0, 0, 0, 0, 0));
        assert!(!cache.accepting_fills);
    }

    #[test]
    fn retained_request_rejects_control_text_and_empty_extent() {
        let mut invalid_text = request();
        invalid_text.runs[0].text = String::from("bad\nrun");
        assert_eq!(
            validate_retain_request(&invalid_text),
            Err(FontKernelError::InvalidRequest("font-service-run"))
        );

        let mut invalid_extent = request();
        invalid_extent.raster_width = 0;
        assert_eq!(
            validate_retain_request(&invalid_extent),
            Err(FontKernelError::InvalidRequest("font-service-empty-extent"))
        );
    }

    #[test]
    fn stamp_contract_accepts_layers_and_enforces_glyph_and_4k_caps() {
        let mut stamp = FontStampRequest {
            layers: alloc::vec![FontStampLayer {
                scene: request(),
                foreground: GpuFontRgba::new(255, 255, 255, 255),
            }],
            fit: FontStampFit::Tight,
        };
        assert_eq!(validate_stamp_request(&stamp), Ok(()));

        stamp.layers[0].scene.runs[0].text = "x".repeat(FONT_STAMP_MAX_GLYPHS + 1);
        assert_eq!(
            validate_stamp_request(&stamp),
            Err(FontKernelError::InvalidRequest("font-service-text-length"))
        );

        stamp.layers[0].scene.runs[0].text = String::from("x");
        stamp.layers[0].scene.raster_width = FONT_STAMP_MAX_EXTENT;
        stamp.layers[0].scene.viewport_width = FONT_STAMP_MAX_EXTENT;
        stamp.layers[0].scene.raster_height = FONT_STAMP_MAX_EXTENT;
        stamp.layers[0].scene.viewport_height = FONT_STAMP_MAX_EXTENT;
        assert_eq!(
            validate_stamp_request(&stamp),
            Err(FontKernelError::InvalidRequest("font-stamp-extent-softcap"))
        );
    }

    #[test]
    fn picasso_lookup_canvas_groups_face_and_color_but_preserves_slant() {
        use trueos_helio_runtime::picasso_scene::{
            Color, FontFace, FontLookupRun, FontSlant, Rect,
        };

        let row = |text: &str, face, slant, color| FontLookupRun {
            rect: Rect::new(0.0, 0.0, 120.0, 24.0),
            origin: [4.0, 20.0],
            text: String::from(text),
            face,
            slant,
            font_pixels: 18.0,
            color,
        };
        let rows = alloc::vec![
            row("normal", FontFace::Inconsolata, FontSlant::Normal, Color::rgba(10, 20, 30, 255),),
            row("italic", FontFace::Inconsolata, FontSlant::Italic, Color::rgba(10, 20, 30, 255),),
            row("default", FontFace::Default, FontSlant::Normal, Color::rgba(10, 20, 30, 255),),
        ];
        let request = picasso_font_lookup_canvas_request(&rows, 320, 200, 640, 400).unwrap();
        assert_eq!(request.fit, FontStampFit::Canvas);
        assert_eq!(request.layers.len(), 2);
        let inconsolata = request
            .layers
            .iter()
            .find(|layer| layer.scene.font == GpuFontFace::Inconsolata)
            .unwrap();
        assert_eq!(inconsolata.scene.runs.len(), 2);
        assert_eq!(inconsolata.scene.runs[0].slant, 0.0);
        assert_eq!(inconsolata.scene.runs[1].slant, 0.15);
        assert_eq!(inconsolata.scene.viewport_width, 320);
        assert_eq!(inconsolata.scene.raster_width, 640);
    }

    #[test]
    fn transient_gpu_dispatch_failures_are_retried() {
        assert!(retryable_gpu_error(&FontKernelError::Unavailable("font-coverage-dispatch")));
        assert!(retryable_gpu_error(&FontKernelError::Unavailable(
            "font-retained-instance-restamp-unavailable"
        )));
        assert!(!retryable_gpu_error(&FontKernelError::Unavailable(
            "font-stamp-output-allocation"
        )));
        assert!(!retryable_gpu_error(&FontKernelError::SubmittedIncomplete(
            "font-retained-instance-submit-incomplete"
        )));
    }

    #[test]
    fn frame_clear_preserves_submission_boundary_failures() {
        use crate::intel::gpgpu::GpgpuSubmissionOutcome;

        assert_eq!(validate_frame_clear_outcome(GpgpuSubmissionOutcome::Complete), Ok(()));
        assert_eq!(
            validate_frame_clear_outcome(GpgpuSubmissionOutcome::Unavailable),
            Err(FontKernelError::Unavailable("font-frame-clear-unavailable"))
        );
        assert_eq!(
            validate_frame_clear_outcome(GpgpuSubmissionOutcome::SubmittedIncomplete),
            Err(FontKernelError::SubmittedIncomplete("font-frame-clear-submit-incomplete"))
        );
    }

    #[test]
    fn consumer_paths_keep_independent_identity() {
        let retain = FontKernelConsumer::new(FontKernelConsumerPath::RetainScene, 1);
        let other_retain = FontKernelConsumer::new(FontKernelConsumerPath::RetainScene, 2);
        let stamp = FontKernelConsumer::new(FontKernelConsumerPath::Stamp, 1);
        assert_ne!(retain, other_retain);
        assert_ne!(retain, stamp);
        assert_eq!(retain.path.name(), "retain-scene");
        assert_eq!(stamp.path.name(), "stamp");
    }
}
