//! Bounded post-warm CPU preparation for exact GPU-font frame plans.
//!
//! The service owns no font files, retained cache, GPU context, or UI frame.
//! It only turns already-warm, deterministic per-cell glyph requests into one
//! sealed plan that can move into `FontKernel` without replaying accepted
//! outline or raster-transform work.

extern crate alloc;

use alloc::{collections::VecDeque, string::String, sync::Arc, vec::Vec};
use core::{
    fmt::Write as _,
    sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering},
};

use spin::Mutex;
use trueos_executor::SpawnError;
use trueos_time::{Duration as EmbassyDuration, Instant, Timer};

use crate::intel::gpu_font::{
    GpuFontGlyphRecipe, GpuFontGlyphRecipeKey, GpuFontPreparedCenteredGlyph, GpuFontRgba,
};
use crate::r::font_kernel_service::FontStampFit;

pub(crate) const FONT_PLAN_WORKER_COUNT: usize = 32;
pub(crate) const FONT_PLAN_MAX_ACTIVE_BATCHES: usize = 4;
pub(crate) const FONT_PLAN_MAX_CELLS_PER_BATCH: usize = 64;
pub(crate) const FONT_PLAN_MAX_ACTIVE_CELLS: usize =
    FONT_PLAN_MAX_ACTIVE_BATCHES * FONT_PLAN_MAX_CELLS_PER_BATCH;
const FONT_PLAN_MAX_RESERVED_OP_BYTES_PER_BATCH: usize = 4 * 1024 * 1024;
const FONT_PLAN_RANDOM_CANDIDATES: u8 = 32;
const FONT_PLAN_GLYPH_ID_LOG_LIMIT: usize = 16;
const FONT_PLAN_ALL_WORKERS_MASK: u32 = u32::MAX;
const FONT_RECIPE_CACHE_SHARDS: usize = 16;
const FONT_RECIPE_CACHE_SOFT_ENTRIES: usize = 4_096;
const FONT_RECIPE_CACHE_BYTES: usize = 16 * 1024 * 1024;
const FONT_RECIPE_SHARD_ENTRY_CAP: usize =
    FONT_RECIPE_CACHE_SOFT_ENTRIES / FONT_RECIPE_CACHE_SHARDS;
const FONT_RECIPE_SHARD_BYTE_CAP: usize = FONT_RECIPE_CACHE_BYTES / FONT_RECIPE_CACHE_SHARDS;

static FONT_PLAN_BATCHES: Mutex<VecDeque<Arc<FontPlanBatch>>> = Mutex::new(VecDeque::new());
static FONT_PLAN_WAIT: crate::wait::WaitQueue = crate::wait::WaitQueue::new();
static FONT_PLAN_ADMITTED_WORKERS: AtomicU32 = AtomicU32::new(0);
static FONT_PLAN_ONLINE_WORKERS: AtomicU32 = AtomicU32::new(0);
static FONT_PLAN_POOL_ONLINE_LOGGED: AtomicBool = AtomicBool::new(false);
static FONT_PLAN_ACTIVE_BATCHES: AtomicUsize = AtomicUsize::new(0);
static FONT_PLAN_ACTIVE_CELLS: AtomicUsize = AtomicUsize::new(0);
static FONT_PLAN_NEXT_BATCH_ID: AtomicU64 = AtomicU64::new(1);
static FONT_PLAN_CLAIM_RR: AtomicUsize = AtomicUsize::new(0);
static FONT_PLAN_COMPLETED: AtomicU64 = AtomicU64::new(0);
static FONT_PLAN_FAILED: AtomicU64 = AtomicU64::new(0);
static FONT_PLAN_CANDIDATE_ATTEMPTS: AtomicU64 = AtomicU64::new(0);
static FONT_PLAN_COOPERATIVE_YIELDS: AtomicU64 = AtomicU64::new(0);
static FONT_RECIPE_SHARDS: [Mutex<Vec<FontRecipeCacheEntry>>; FONT_RECIPE_CACHE_SHARDS] =
    [const { Mutex::new(Vec::new()) }; FONT_RECIPE_CACHE_SHARDS];
static FONT_RECIPE_NEXT_BUILD: AtomicU64 = AtomicU64::new(1);
static FONT_RECIPE_TOUCH: AtomicU64 = AtomicU64::new(1);
static FONT_RECIPE_HITS: AtomicU64 = AtomicU64::new(0);
static FONT_RECIPE_MISSES: AtomicU64 = AtomicU64::new(0);
static FONT_RECIPE_COALESCED: AtomicU64 = AtomicU64::new(0);
static FONT_RECIPE_BUILDS: AtomicU64 = AtomicU64::new(0);
static FONT_RECIPE_FAILURES: AtomicU64 = AtomicU64::new(0);
static FONT_RECIPE_EVICTIONS: AtomicU64 = AtomicU64::new(0);
static FONT_RECIPE_RESIDENT_ENTRIES: AtomicUsize = AtomicUsize::new(0);
static FONT_RECIPE_RESIDENT_BYTES: AtomicUsize = AtomicUsize::new(0);

struct FontRecipeCacheEntry {
    key: GpuFontGlyphRecipeKey,
    state: FontRecipeCacheState,
}

enum FontRecipeCacheState {
    Building {
        build_id: u64,
    },
    Ready {
        recipe: Arc<GpuFontGlyphRecipe>,
        bytes: usize,
        last_touch: u64,
    },
    Failed {
        reason: &'static str,
        last_touch: u64,
    },
}

enum FontRecipeProbe {
    Hit(Arc<GpuFontGlyphRecipe>),
    Build(u64),
    Wait,
    Failed(&'static str),
    Saturated,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FontPlanError {
    PoolOffline,
    PoolFull,
    InvalidRequest(&'static str),
    BuildFailed(&'static str),
    Cancelled,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PreparedGlyphPlanDiagnostics {
    producer: &'static str,
    glyph_fingerprint: u64,
    glyph_ids_sample: String,
    candidate_attempts: u64,
}

impl PreparedGlyphPlanDiagnostics {
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) const fn producer(&self) -> &'static str {
        self.producer
    }

    pub(crate) const fn glyph_fingerprint(&self) -> u64 {
        self.glyph_fingerprint
    }

    pub(crate) fn glyph_ids_sample(&self) -> &str {
        self.glyph_ids_sample.as_str()
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) const fn candidate_attempts(&self) -> u64 {
        self.candidate_attempts
    }
}

/// Exact one-frame handoff from the post-warm producer pool to FontKernel.
///
/// Deliberately not `Clone`: the request and its prepared sidecar have one
/// owner and cross the service boundary together.
pub(crate) struct PreparedGlyphPlan {
    fit: FontStampFit,
    font: crate::intel::gpu_font::GpuFontFace,
    foreground: GpuFontRgba,
    raster_width: u32,
    raster_height: u32,
    prepared: Vec<GpuFontPreparedCenteredGlyph>,
    ops_bytes: usize,
    estimated_work: u64,
    diagnostics: PreparedGlyphPlanDiagnostics,
    _permit: FontPlanPermit,
}

impl PreparedGlyphPlan {
    pub(crate) const fn fit(&self) -> FontStampFit {
        self.fit
    }

    pub(crate) const fn font(&self) -> crate::intel::gpu_font::GpuFontFace {
        self.font
    }

    pub(crate) const fn raster_width(&self) -> u32 {
        self.raster_width
    }

    pub(crate) const fn raster_height(&self) -> u32 {
        self.raster_height
    }

    pub(crate) const fn foreground(&self) -> GpuFontRgba {
        self.foreground
    }

    pub(crate) fn glyph_count(&self) -> usize {
        self.prepared.len()
    }

    pub(crate) const fn ops_bytes(&self) -> usize {
        self.ops_bytes
    }

    pub(crate) const fn estimated_work(&self) -> u64 {
        self.estimated_work
    }

    pub(crate) const fn diagnostics(&self) -> &PreparedGlyphPlanDiagnostics {
        &self.diagnostics
    }

    pub(crate) fn into_prepared(self) -> Vec<GpuFontPreparedCenteredGlyph> {
        self.prepared
    }
}

/// Cheap post-warm description of one independently admitted centered cell.
///
/// The caller rolls only the deterministic seed. Character selection,
/// warmed-outline lookup, raster transformation, and analytical admission all
/// happen on the producer pool.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FontPlanCellRequest {
    position: [f32; 2],
    font_pixels: f32,
    slant: f32,
    max_work: u64,
    rng_seed: u64,
    fixed_scalar: Option<char>,
    worker_affinity: Option<u8>,
}

impl FontPlanCellRequest {
    pub(crate) const fn new(
        position: [f32; 2],
        font_pixels: f32,
        slant: f32,
        max_work: u64,
        rng_seed: u64,
    ) -> Self {
        Self {
            position,
            font_pixels,
            slant,
            max_work,
            rng_seed,
            fixed_scalar: None,
            worker_affinity: None,
        }
    }

    /// Describe one exact character while retaining the same shared recipe
    /// cache and worker-pool path as rolled demo glyphs.
    pub(crate) const fn fixed(
        position: [f32; 2],
        font_pixels: f32,
        slant: f32,
        max_work: u64,
        scalar: char,
    ) -> Self {
        Self {
            position,
            font_pixels,
            slant,
            max_work,
            rng_seed: 0,
            fixed_scalar: Some(scalar),
            worker_affinity: None,
        }
    }

    /// Reserve this cell for one stable member of the 32-task producer pool.
    /// Ordinary callers remain affinity-free and preserve work stealing.
    pub(crate) const fn with_worker_affinity(mut self, worker_id: u8) -> Self {
        self.worker_affinity = Some(worker_id);
        self
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) const fn position(self) -> [f32; 2] {
        self.position
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) const fn font_pixels(self) -> f32 {
        self.font_pixels
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) const fn max_work(self) -> u64 {
        self.max_work
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) const fn rng_seed(self) -> u64 {
        self.rng_seed
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) const fn fixed_scalar(self) -> Option<char> {
        self.fixed_scalar
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) const fn worker_affinity(self) -> Option<u8> {
        self.worker_affinity
    }
}

/// One bounded frame-plan batch.
///
/// `parallelism` is an explicit borrow width. Ordinary text clients request
/// one producer; a parallel UI may request several without changing result
/// ordering because cells are assembled by their original index.
pub(crate) struct FontPlanBatchRequest {
    producer: &'static str,
    font: crate::intel::gpu_font::GpuFontFace,
    foreground: GpuFontRgba,
    viewport_width: u32,
    viewport_height: u32,
    raster_width: u32,
    raster_height: u32,
    cells: Vec<FontPlanCellRequest>,
    parallelism: usize,
}

impl FontPlanBatchRequest {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        producer: &'static str,
        font: crate::intel::gpu_font::GpuFontFace,
        foreground: GpuFontRgba,
        viewport_width: u32,
        viewport_height: u32,
        raster_width: u32,
        raster_height: u32,
        cells: Vec<FontPlanCellRequest>,
        parallelism: usize,
    ) -> Self {
        Self {
            producer,
            font,
            foreground,
            viewport_width,
            viewport_height,
            raster_width,
            raster_height,
            cells,
            parallelism,
        }
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) const fn font(&self) -> crate::intel::gpu_font::GpuFontFace {
        self.font
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) const fn viewport_extent(&self) -> (u32, u32) {
        (self.viewport_width, self.viewport_height)
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) const fn raster_extent(&self) -> (u32, u32) {
        (self.raster_width, self.raster_height)
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn cells(&self) -> &[FontPlanCellRequest] {
        self.cells.as_slice()
    }

    pub(crate) const fn parallelism(&self) -> usize {
        self.parallelism
    }

    fn validate(&self) -> Result<(), FontPlanError> {
        if self.producer.is_empty()
            || self.viewport_width == 0
            || self.viewport_height == 0
            || self.raster_width == 0
            || self.raster_height == 0
            || self.cells.is_empty()
            || self.cells.len() > FONT_PLAN_MAX_CELLS_PER_BATCH
            || self.parallelism == 0
            || self.parallelism > FONT_PLAN_WORKER_COUNT
        {
            return Err(FontPlanError::InvalidRequest("font-plan-contract"));
        }
        if self.cells.iter().any(|cell| {
            !cell.position[0].is_finite()
                || !cell.position[1].is_finite()
                || !cell.font_pixels.is_finite()
                || !cell.slant.is_finite()
                || cell.font_pixels <= 0.0
                || cell.slant.abs() > 1.0
                || cell.max_work == 0
                || cell
                    .fixed_scalar
                    .is_some_and(|scalar| scalar.is_control() || scalar.is_whitespace())
                || cell
                    .worker_affinity
                    .is_some_and(|worker| usize::from(worker) >= self.parallelism)
        }) {
            return Err(FontPlanError::InvalidRequest("font-plan-cell"));
        }
        // This service begins strictly after raw TTF/Skrifa publication. Its
        // workers must never become an implicit fallback warming channel.
        if !crate::intel::gpu_font::font_face_is_available(self.font) {
            return Err(FontPlanError::InvalidRequest("font-plan-font-not-warm"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct FontPlanServiceStatus {
    pub(crate) online_workers: usize,
    pub(crate) active_batches: usize,
    pub(crate) active_cells: usize,
    pub(crate) queued_cells: usize,
    pub(crate) completed_batches: u64,
    pub(crate) failed_batches: u64,
    pub(crate) candidate_attempts: u64,
    pub(crate) cooperative_yields: u64,
    pub(crate) recipe_hits: u64,
    pub(crate) recipe_misses: u64,
    pub(crate) recipe_coalesced: u64,
    pub(crate) recipe_builds: u64,
    pub(crate) recipe_failures: u64,
    pub(crate) recipe_evictions: u64,
    pub(crate) recipe_resident_entries: usize,
    pub(crate) recipe_resident_bytes: usize,
}

pub(crate) fn status() -> FontPlanServiceStatus {
    let queued_cells = FONT_PLAN_BATCHES
        .lock()
        .iter()
        .map(|batch| batch.remaining.load(Ordering::Acquire))
        .sum();
    FontPlanServiceStatus {
        online_workers: FONT_PLAN_ONLINE_WORKERS
            .load(Ordering::Acquire)
            .count_ones() as usize,
        active_batches: FONT_PLAN_ACTIVE_BATCHES.load(Ordering::Acquire),
        active_cells: FONT_PLAN_ACTIVE_CELLS.load(Ordering::Acquire),
        queued_cells,
        completed_batches: FONT_PLAN_COMPLETED.load(Ordering::Acquire),
        failed_batches: FONT_PLAN_FAILED.load(Ordering::Acquire),
        candidate_attempts: FONT_PLAN_CANDIDATE_ATTEMPTS.load(Ordering::Acquire),
        cooperative_yields: FONT_PLAN_COOPERATIVE_YIELDS.load(Ordering::Acquire),
        recipe_hits: FONT_RECIPE_HITS.load(Ordering::Acquire),
        recipe_misses: FONT_RECIPE_MISSES.load(Ordering::Acquire),
        recipe_coalesced: FONT_RECIPE_COALESCED.load(Ordering::Acquire),
        recipe_builds: FONT_RECIPE_BUILDS.load(Ordering::Acquire),
        recipe_failures: FONT_RECIPE_FAILURES.load(Ordering::Acquire),
        recipe_evictions: FONT_RECIPE_EVICTIONS.load(Ordering::Acquire),
        recipe_resident_entries: FONT_RECIPE_RESIDENT_ENTRIES.load(Ordering::Acquire),
        recipe_resident_bytes: FONT_RECIPE_RESIDENT_BYTES.load(Ordering::Acquire),
    }
}

fn next_recipe_counter(counter: &AtomicU64) -> u64 {
    loop {
        let value = counter.fetch_add(1, Ordering::AcqRel);
        if value != 0 {
            return value;
        }
    }
}

fn recipe_shard(key: GpuFontGlyphRecipeKey) -> usize {
    key.fingerprint() as usize & (FONT_RECIPE_CACHE_SHARDS - 1)
}

fn probe_font_recipe(key: GpuFontGlyphRecipeKey) -> FontRecipeProbe {
    let shard_index = recipe_shard(key);
    let touch = next_recipe_counter(&FONT_RECIPE_TOUCH);
    let mut shard = FONT_RECIPE_SHARDS[shard_index].lock();
    if let Some(entry) = shard.iter_mut().find(|entry| entry.key == key) {
        return match &mut entry.state {
            FontRecipeCacheState::Ready {
                recipe, last_touch, ..
            } => {
                *last_touch = touch;
                FONT_RECIPE_HITS.fetch_add(1, Ordering::AcqRel);
                FontRecipeProbe::Hit(Arc::clone(recipe))
            }
            FontRecipeCacheState::Building { .. } => {
                FONT_RECIPE_COALESCED.fetch_add(1, Ordering::AcqRel);
                FontRecipeProbe::Wait
            }
            FontRecipeCacheState::Failed { reason, last_touch } => {
                *last_touch = touch;
                FontRecipeProbe::Failed(*reason)
            }
        };
    }

    while shard.len() >= FONT_RECIPE_SHARD_ENTRY_CAP {
        let Some(victim) = shard
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| match entry.state {
                FontRecipeCacheState::Building { .. } => None,
                FontRecipeCacheState::Ready { last_touch, .. }
                | FontRecipeCacheState::Failed { last_touch, .. } => Some((index, last_touch)),
            })
            .min_by_key(|(_, last_touch)| *last_touch)
            .map(|(index, _)| index)
        else {
            return FontRecipeProbe::Saturated;
        };
        if let FontRecipeCacheState::Ready { bytes, .. } = shard.swap_remove(victim).state {
            FONT_RECIPE_RESIDENT_ENTRIES.fetch_sub(1, Ordering::AcqRel);
            FONT_RECIPE_RESIDENT_BYTES.fetch_sub(bytes, Ordering::AcqRel);
        }
        FONT_RECIPE_EVICTIONS.fetch_add(1, Ordering::AcqRel);
    }

    let build_id = next_recipe_counter(&FONT_RECIPE_NEXT_BUILD);
    shard.push(FontRecipeCacheEntry {
        key,
        state: FontRecipeCacheState::Building { build_id },
    });
    FONT_RECIPE_MISSES.fetch_add(1, Ordering::AcqRel);
    FontRecipeProbe::Build(build_id)
}

fn publish_font_recipe(
    key: GpuFontGlyphRecipeKey,
    build_id: u64,
    result: Result<Arc<GpuFontGlyphRecipe>, &'static str>,
) -> Result<Arc<GpuFontGlyphRecipe>, &'static str> {
    let shard_index = recipe_shard(key);
    let touch = next_recipe_counter(&FONT_RECIPE_TOUCH);
    let mut shard = FONT_RECIPE_SHARDS[shard_index].lock();
    let Some(build_index) = shard.iter().position(|entry| {
        entry.key == key
            && matches!(
                entry.state,
                FontRecipeCacheState::Building { build_id: active } if active == build_id
            )
    }) else {
        return Err("font-recipe-build-stale");
    };
    shard.swap_remove(build_index);

    let published = match result {
        Ok(recipe) => {
            let bytes = recipe.ops_bytes();
            if bytes > FONT_RECIPE_SHARD_BYTE_CAP {
                FONT_RECIPE_FAILURES.fetch_add(1, Ordering::AcqRel);
                shard.push(FontRecipeCacheEntry {
                    key,
                    state: FontRecipeCacheState::Failed {
                        reason: "font-recipe-entry-byte-cap",
                        last_touch: touch,
                    },
                });
                Err("font-recipe-entry-byte-cap")
            } else {
                let mut resident_bytes = shard
                    .iter()
                    .filter_map(|entry| match entry.state {
                        FontRecipeCacheState::Ready { bytes, .. } => Some(bytes),
                        _ => None,
                    })
                    .sum::<usize>();
                while resident_bytes.saturating_add(bytes) > FONT_RECIPE_SHARD_BYTE_CAP {
                    let Some(victim) = shard
                        .iter()
                        .enumerate()
                        .filter_map(|(index, entry)| match entry.state {
                            FontRecipeCacheState::Ready {
                                bytes, last_touch, ..
                            } => Some((index, last_touch, bytes)),
                            _ => None,
                        })
                        .min_by_key(|(_, last_touch, _)| *last_touch)
                    else {
                        break;
                    };
                    let removed = shard.swap_remove(victim.0);
                    if let FontRecipeCacheState::Ready { bytes, .. } = removed.state {
                        resident_bytes = resident_bytes.saturating_sub(bytes);
                        FONT_RECIPE_RESIDENT_ENTRIES.fetch_sub(1, Ordering::AcqRel);
                        FONT_RECIPE_RESIDENT_BYTES.fetch_sub(bytes, Ordering::AcqRel);
                        FONT_RECIPE_EVICTIONS.fetch_add(1, Ordering::AcqRel);
                    }
                }
                if resident_bytes.saturating_add(bytes) > FONT_RECIPE_SHARD_BYTE_CAP {
                    FONT_RECIPE_FAILURES.fetch_add(1, Ordering::AcqRel);
                    Err("font-recipe-shard-byte-cap")
                } else {
                    shard.push(FontRecipeCacheEntry {
                        key,
                        state: FontRecipeCacheState::Ready {
                            recipe: Arc::clone(&recipe),
                            bytes,
                            last_touch: touch,
                        },
                    });
                    FONT_RECIPE_BUILDS.fetch_add(1, Ordering::AcqRel);
                    FONT_RECIPE_RESIDENT_ENTRIES.fetch_add(1, Ordering::AcqRel);
                    FONT_RECIPE_RESIDENT_BYTES.fetch_add(bytes, Ordering::AcqRel);
                    Ok(recipe)
                }
            }
        }
        Err(reason) => {
            FONT_RECIPE_FAILURES.fetch_add(1, Ordering::AcqRel);
            shard.push(FontRecipeCacheEntry {
                key,
                state: FontRecipeCacheState::Failed {
                    reason,
                    last_touch: touch,
                },
            });
            Err(reason)
        }
    };
    drop(shard);
    wake_plan_workers(FONT_PLAN_WORKER_COUNT);
    published
}

/// Resolve one exact recipe through the same shared, sharded single-flight
/// cache used by producer batches.
///
/// FontKernel calls this only from its leased blocking service lane while
/// preparing retained SceneDB resources. The key already names a glyph in the
/// append-only boot-warmed outline registry; no TTF bytes or outline commands
/// are supplied by the scene producer.
#[derive(Clone)]
pub(crate) struct WarmedFontRecipeLease {
    recipe: Arc<GpuFontGlyphRecipe>,
}

impl WarmedFontRecipeLease {
    pub(crate) fn key(&self) -> GpuFontGlyphRecipeKey {
        self.recipe.key()
    }

    #[cfg(test)]
    fn shares_allocation(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.recipe, &other.recipe)
    }
}

pub(crate) fn resolve_warmed_font_recipe(
    key: GpuFontGlyphRecipeKey,
) -> Result<WarmedFontRecipeLease, FontPlanError> {
    match probe_font_recipe(key) {
        FontRecipeProbe::Hit(recipe) => Ok(WarmedFontRecipeLease { recipe }),
        FontRecipeProbe::Build(build_id) => {
            let built = crate::intel::gpu_font::build_gpu_font_glyph_recipe(key);
            publish_font_recipe(key, build_id, built)
                .map(|recipe| WarmedFontRecipeLease { recipe })
                .map_err(FontPlanError::BuildFailed)
        }
        FontRecipeProbe::Wait | FontRecipeProbe::Saturated => Err(FontPlanError::PoolFull),
        FontRecipeProbe::Failed(reason) => Err(FontPlanError::BuildFailed(reason)),
    }
}

/// Timing and fairness evidence produced alongside one sealed plan.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct FontPlanBuildStats {
    batch_id: u64,
    queue_wait_ms: u64,
    build_ms: u64,
    candidate_attempts: u64,
    rejected_candidates: u64,
    worker_slices: u64,
    cooperative_yields: u64,
    parallelism: usize,
    participant_mask: u32,
    reserved_ops_bytes: usize,
}

impl FontPlanBuildStats {
    pub(crate) const fn batch_id(self) -> u64 {
        self.batch_id
    }

    pub(crate) const fn queue_wait_ms(self) -> u64 {
        self.queue_wait_ms
    }

    pub(crate) const fn build_ms(self) -> u64 {
        self.build_ms
    }

    pub(crate) const fn candidate_attempts(self) -> u64 {
        self.candidate_attempts
    }

    pub(crate) const fn rejected_candidates(self) -> u64 {
        self.rejected_candidates
    }

    pub(crate) const fn worker_slices(self) -> u64 {
        self.worker_slices
    }

    pub(crate) const fn cooperative_yields(self) -> u64 {
        self.cooperative_yields
    }

    pub(crate) const fn parallelism(self) -> usize {
        self.parallelism
    }

    pub(crate) const fn participant_mask(self) -> u32 {
        self.participant_mask
    }

    pub(crate) const fn participants(self) -> usize {
        self.participant_mask.count_ones() as usize
    }

    pub(crate) const fn reserved_ops_bytes(self) -> usize {
        self.reserved_ops_bytes
    }
}

pub(crate) struct PreparedGlyphPlanOutput {
    plan: PreparedGlyphPlan,
    stats: FontPlanBuildStats,
}

impl PreparedGlyphPlanOutput {
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) const fn plan(&self) -> &PreparedGlyphPlan {
        &self.plan
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) const fn stats(&self) -> FontPlanBuildStats {
        self.stats
    }

    pub(crate) fn into_parts(self) -> (PreparedGlyphPlan, FontPlanBuildStats) {
        (self.plan, self.stats)
    }
}

/// Reserved active-batch/cell capacity. It moves into the sealed plan and is
/// released only when FontKernel consumes that plan or the caller drops it.
struct FontPlanPermit {
    cells: usize,
}

impl Drop for FontPlanPermit {
    fn drop(&mut self) {
        if self.cells != 0 {
            FONT_PLAN_ACTIVE_CELLS.fetch_sub(self.cells, Ordering::AcqRel);
        }
        FONT_PLAN_ACTIVE_BATCHES.fetch_sub(1, Ordering::AcqRel);
    }
}

pub(crate) struct FontPlanBatchBorrow {
    permit: Option<FontPlanPermit>,
}

pub(crate) fn borrow_plan_batch() -> Result<FontPlanBatchBorrow, FontPlanError> {
    if FONT_PLAN_ONLINE_WORKERS.load(Ordering::Acquire) != FONT_PLAN_ALL_WORKERS_MASK {
        return Err(FontPlanError::PoolOffline);
    }
    let admitted =
        FONT_PLAN_ACTIVE_BATCHES.try_update(Ordering::AcqRel, Ordering::Acquire, |current| {
            (current < FONT_PLAN_MAX_ACTIVE_BATCHES).then_some(current + 1)
        });
    if admitted.is_err() {
        return Err(FontPlanError::PoolFull);
    }
    Ok(FontPlanBatchBorrow {
        permit: Some(FontPlanPermit { cells: 0 }),
    })
}

impl FontPlanBatchBorrow {
    pub(crate) fn submit(
        mut self,
        request: FontPlanBatchRequest,
    ) -> Result<PendingPreparedGlyphPlan, FontPlanError> {
        request.validate()?;
        let cell_count = request.cells.len();
        let cells_admitted =
            FONT_PLAN_ACTIVE_CELLS.try_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current
                    .checked_add(cell_count)
                    .filter(|next| *next <= FONT_PLAN_MAX_ACTIVE_CELLS)
            });
        if cells_admitted.is_err() {
            return Err(FontPlanError::PoolFull);
        }
        let mut permit = self
            .permit
            .take()
            .expect("font plan borrow submitted more than once");
        permit.cells = cell_count;
        let batch_id = next_batch_id();
        let parallelism = request.parallelism.min(cell_count).max(1);
        let enqueued_ms = Instant::now().as_millis();
        let cells = request
            .cells
            .into_iter()
            .map(PlanCell::new)
            .collect::<Vec<_>>();
        let batch = Arc::new(FontPlanBatch {
            id: batch_id,
            producer: request.producer,
            font: request.font,
            foreground: request.foreground,
            viewport_width: request.viewport_width,
            viewport_height: request.viewport_height,
            raster_width: request.raster_width,
            raster_height: request.raster_height,
            parallelism,
            enqueued_ms,
            first_started_ms: AtomicU64::new(0),
            remaining: AtomicUsize::new(cell_count),
            active_workers: AtomicUsize::new(0),
            terminal: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            candidate_attempts: AtomicU64::new(0),
            rejected_candidates: AtomicU64::new(0),
            worker_slices: AtomicU64::new(0),
            cooperative_yields: AtomicU64::new(0),
            participant_mask: AtomicU32::new(0),
            state: Mutex::new(FontPlanBatchState {
                cells,
                logical_ops_bytes: 0,
                reserved_ops_bytes: 0,
                estimated_work: 0,
                permit: Some(permit),
            }),
            completion: crate::wait::CompletionCell::new(),
        });
        FONT_PLAN_BATCHES.lock().push_back(Arc::clone(&batch));
        wake_plan_workers(parallelism);
        crate::log_info!(
            target: "render";
            "font-plan-service: batch queued id={} producer={} cells={} parallelism={} active_batches={} active_cells={} cap_batches={} cap_cells={} recipe_cache=shared-sharded-single-flight cache_entries={} cache_bytes={}\n",
            batch_id,
            batch.producer,
            cell_count,
            parallelism,
            FONT_PLAN_ACTIVE_BATCHES.load(Ordering::Acquire),
            FONT_PLAN_ACTIVE_CELLS.load(Ordering::Acquire),
            FONT_PLAN_MAX_ACTIVE_BATCHES,
            FONT_PLAN_MAX_ACTIVE_CELLS,
            FONT_RECIPE_RESIDENT_ENTRIES.load(Ordering::Acquire),
            FONT_RECIPE_RESIDENT_BYTES.load(Ordering::Acquire),
        );
        Ok(PendingPreparedGlyphPlan { batch })
    }
}

pub(crate) struct PendingPreparedGlyphPlan {
    batch: Arc<FontPlanBatch>,
}

impl PendingPreparedGlyphPlan {
    pub(crate) fn batch_id(&self) -> u64 {
        self.batch.id
    }

    pub(crate) fn try_take(&mut self) -> Option<Result<PreparedGlyphPlanOutput, FontPlanError>> {
        self.batch.completion.try_take()
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) async fn wait(self) -> Result<PreparedGlyphPlanOutput, FontPlanError> {
        self.batch.completion.join().await
    }
}

impl Drop for PendingPreparedGlyphPlan {
    fn drop(&mut self) {
        if !self.batch.terminal.load(Ordering::Acquire) {
            self.batch.cancelled.store(true, Ordering::Release);
            FONT_PLAN_WAIT.notify_one();
        }
    }
}

struct FontPlanBatch {
    id: u64,
    producer: &'static str,
    font: crate::intel::gpu_font::GpuFontFace,
    foreground: GpuFontRgba,
    viewport_width: u32,
    viewport_height: u32,
    raster_width: u32,
    raster_height: u32,
    parallelism: usize,
    enqueued_ms: u64,
    first_started_ms: AtomicU64,
    remaining: AtomicUsize,
    active_workers: AtomicUsize,
    terminal: AtomicBool,
    cancelled: AtomicBool,
    candidate_attempts: AtomicU64,
    rejected_candidates: AtomicU64,
    worker_slices: AtomicU64,
    cooperative_yields: AtomicU64,
    participant_mask: AtomicU32,
    state: Mutex<FontPlanBatchState>,
    completion: crate::wait::CompletionCell<Result<PreparedGlyphPlanOutput, FontPlanError>>,
}

struct FontPlanBatchState {
    cells: Vec<PlanCell>,
    logical_ops_bytes: usize,
    reserved_ops_bytes: usize,
    estimated_work: u64,
    permit: Option<FontPlanPermit>,
}

struct PlanCell {
    request: FontPlanCellRequest,
    selection: GlyphSelectionState,
    retry_candidate: Option<(char, f32)>,
    status: PlanCellStatus,
}

impl PlanCell {
    fn new(request: FontPlanCellRequest) -> Self {
        Self {
            selection: GlyphSelectionState::new(
                request.rng_seed,
                request.font_pixels,
                request.fixed_scalar,
            ),
            request,
            retry_candidate: None,
            status: PlanCellStatus::Pending,
        }
    }
}

enum PlanCellStatus {
    Pending,
    Working,
    Ready(Option<PreparedPlanCell>),
}

struct PreparedPlanCell {
    scalar: char,
    prepared: GpuFontPreparedCenteredGlyph,
}

struct GlyphSelectionState {
    rng: crate::tyche::SoftRng,
    fixed_scalar: Option<char>,
    fixed_claimed: bool,
    random_attempts: u8,
    fallback_start: Option<usize>,
    fallback_offset: usize,
    fallback_font_pixels: f32,
}

impl GlyphSelectionState {
    fn new(seed: u64, font_pixels: f32, fixed_scalar: Option<char>) -> Self {
        Self {
            rng: crate::tyche::SoftRng::from_seed(seed),
            fixed_scalar,
            fixed_claimed: false,
            random_attempts: 0,
            fallback_start: None,
            fallback_offset: 0,
            fallback_font_pixels: font_pixels,
        }
    }

    fn next_candidate(&mut self) -> Option<(char, f32)> {
        if let Some(scalar) = self.fixed_scalar {
            if self.fixed_claimed {
                return None;
            }
            self.fixed_claimed = true;
            return Some((scalar, self.fallback_font_pixels));
        }
        const SHARED_DENSE_RANGE: (u32, u32) = (0x0021, 0x007E);
        const RANGES: &[(u32, u32)] = &[
            SHARED_DENSE_RANGE,
            (0x00A1, 0x024F),
            (0x0370, 0x052F),
            (0x0531, 0x058F),
            (0x0590, 0x06FF),
            (0x0900, 0x097F),
            (0x10A0, 0x10FF),
            (0x2000, 0x206F),
            (0x2190, 0x22FF),
            (0x2500, 0x27BF),
            (0x2C60, 0x2C7F),
            (0x4E00, 0x9FFF),
            (0x1F300, 0x1F64F),
        ];
        while self.random_attempts < FONT_PLAN_RANDOM_CANDIDATES {
            self.random_attempts = self.random_attempts.saturating_add(1);
            // Printable ASCII is the only dense range shared by every face in
            // the current three-font cycle. Prefer it without excluding the
            // broad Unicode stress roll; this avoids spending a one-producer
            // layer mostly yielding after predictably unsupported candidates.
            let (first, last) = if self.rng.usize_below(4) != 0 {
                SHARED_DENSE_RANGE
            } else {
                RANGES[self.rng.usize_below(RANGES.len())]
            };
            let scalar =
                first.saturating_add(self.rng.usize_below((last - first + 1) as usize) as u32);
            let Some(ch) = char::from_u32(scalar) else {
                continue;
            };
            if !ch.is_control() && !ch.is_whitespace() {
                return Some((ch, self.fallback_font_pixels));
            }
        }

        const FALLBACKS: [char; 10] = ['.', '-', '|', '!', '1', 'I', 'l', '+', '/', '\\'];
        let start = *self
            .fallback_start
            .get_or_insert_with(|| self.rng.usize_below(FALLBACKS.len()));
        if self.fallback_offset >= FALLBACKS.len() {
            self.fallback_offset = 0;
            if self.fallback_font_pixels <= 4.0 {
                return None;
            }
            self.fallback_font_pixels = (self.fallback_font_pixels * 0.75).max(4.0);
        }
        let ch = FALLBACKS[(start + self.fallback_offset) % FALLBACKS.len()];
        self.fallback_offset = self.fallback_offset.saturating_add(1);
        Some((ch, self.fallback_font_pixels))
    }
}

struct FontPlanClaim {
    batch: Arc<FontPlanBatch>,
    cell_index: usize,
    scalar: char,
    font_pixels: f32,
    request: FontPlanCellRequest,
}

enum FontPlanClaimResult {
    Attempt(FontPlanClaim),
    Fail(Arc<FontPlanBatch>, FontPlanError),
}

fn wake_plan_workers(count: usize) {
    for _ in 0..count.min(FONT_PLAN_WORKER_COUNT) {
        FONT_PLAN_WAIT.notify_one();
    }
}

fn next_batch_id() -> u64 {
    loop {
        let id = FONT_PLAN_NEXT_BATCH_ID.fetch_add(1, Ordering::AcqRel);
        if id != 0 {
            return id;
        }
    }
}

fn claim_plan_work(worker_id: usize) -> Option<FontPlanClaimResult> {
    let batches = FONT_PLAN_BATCHES.lock();
    if batches.is_empty() {
        return None;
    }
    let start = FONT_PLAN_CLAIM_RR.fetch_add(1, Ordering::Relaxed) % batches.len();
    for offset in 0..batches.len() {
        let batch = Arc::clone(&batches[(start + offset) % batches.len()]);
        if batch.terminal.load(Ordering::Acquire) {
            continue;
        }
        if batch.cancelled.load(Ordering::Acquire) {
            return Some(FontPlanClaimResult::Fail(batch, FontPlanError::Cancelled));
        }
        if batch.active_workers.load(Ordering::Acquire) >= batch.parallelism {
            continue;
        }
        let mut state = batch.state.lock();
        let Some((cell_index, cell)) = state.cells.iter_mut().enumerate().find(|(_, cell)| {
            matches!(cell.status, PlanCellStatus::Pending)
                && cell
                    .request
                    .worker_affinity
                    .is_none_or(|affinity| usize::from(affinity) == worker_id)
        }) else {
            continue;
        };
        let candidate = cell
            .retry_candidate
            .take()
            .or_else(|| cell.selection.next_candidate());
        let Some((scalar, font_pixels)) = candidate else {
            drop(state);
            drop(batches);
            return Some(FontPlanClaimResult::Fail(
                batch,
                FontPlanError::BuildFailed("font-rush-visible-glyph-unavailable"),
            ));
        };
        let request = cell.request;
        cell.status = PlanCellStatus::Working;
        batch.active_workers.fetch_add(1, Ordering::AcqRel);
        let now_ms = Instant::now().as_millis();
        let _ = batch.first_started_ms.compare_exchange(
            0,
            now_ms.max(1),
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        drop(state);
        drop(batches);
        return Some(FontPlanClaimResult::Attempt(FontPlanClaim {
            batch,
            cell_index,
            scalar,
            font_pixels,
            request,
        }));
    }
    None
}

fn retire_plan_worker(batch: &Arc<FontPlanBatch>) {
    let previous = batch.active_workers.fetch_sub(1, Ordering::AcqRel);
    debug_assert!(previous != 0, "font plan active-worker underflow");
    release_terminal_permit_if_quiescent(batch);
}

enum FontPlanAttemptResult {
    Prepared(GpuFontPreparedCenteredGlyph),
    WaitForRecipe,
    Rejected,
}

fn run_plan_attempt(claim: FontPlanClaim, worker_id: usize) {
    let batch = &claim.batch;
    batch
        .participant_mask
        .fetch_or(1u32 << worker_id, Ordering::AcqRel);
    batch.worker_slices.fetch_add(1, Ordering::AcqRel);
    batch.candidate_attempts.fetch_add(1, Ordering::AcqRel);
    FONT_PLAN_CANDIDATE_ATTEMPTS.fetch_add(1, Ordering::AcqRel);
    let result = if batch.cancelled.load(Ordering::Acquire) {
        FontPlanAttemptResult::Rejected
    } else {
        let key = crate::intel::gpu_font::gpu_font_centered_glyph_recipe_key(
            claim.scalar,
            batch.font,
            claim.font_pixels,
            claim.request.slant,
            batch.viewport_width,
            batch.viewport_height,
            batch.raster_width,
            batch.raster_height,
        );
        match key {
            Err(_) => FontPlanAttemptResult::Rejected,
            Ok(key) => {
                let recipe = match probe_font_recipe(key) {
                    FontRecipeProbe::Hit(recipe) => Ok(recipe),
                    FontRecipeProbe::Build(build_id) => {
                        let built = crate::intel::gpu_font::build_gpu_font_glyph_recipe(key);
                        publish_font_recipe(key, build_id, built)
                    }
                    FontRecipeProbe::Wait | FontRecipeProbe::Saturated => Err("font-recipe-wait"),
                    FontRecipeProbe::Failed(reason) => Err(reason),
                };
                match recipe {
                    Err("font-recipe-wait") => FontPlanAttemptResult::WaitForRecipe,
                    Err(_) => FontPlanAttemptResult::Rejected,
                    Ok(recipe) => {
                        match crate::intel::gpu_font::place_gpu_font_centered_glyph_recipe(
                            claim.scalar,
                            recipe,
                            claim.request.position,
                            claim.font_pixels,
                            claim.request.slant,
                            batch.viewport_width,
                            batch.viewport_height,
                            batch.raster_width,
                            batch.raster_height,
                        ) {
                            Ok(prepared) => FontPlanAttemptResult::Prepared(prepared),
                            Err(_) => FontPlanAttemptResult::Rejected,
                        }
                    }
                }
            }
        }
    };

    if batch.terminal.load(Ordering::Acquire) {
        retire_plan_worker(&claim.batch);
        return;
    }
    if batch.cancelled.load(Ordering::Acquire) {
        finish_batch_error(&claim.batch, FontPlanError::Cancelled);
        retire_plan_worker(&claim.batch);
        return;
    }

    match result {
        FontPlanAttemptResult::Prepared(prepared)
            if prepared.estimated_segment_evaluations() <= claim.request.max_work =>
        {
            let logical_bytes = prepared.ops_bytes();
            let reserved_bytes = prepared.allocated_ops_bytes();
            let work = prepared.estimated_segment_evaluations();
            let admitted = {
                let mut state = batch.state.lock();
                if claim.cell_index >= state.cells.len() {
                    Err(FontPlanError::BuildFailed("font-plan-cell-index"))
                } else {
                    let next_logical = state
                        .logical_ops_bytes
                        .checked_add(logical_bytes)
                        .ok_or(FontPlanError::BuildFailed("font-plan-ops-bytes"));
                    let next_reserved = state
                        .reserved_ops_bytes
                        .checked_add(reserved_bytes)
                        .filter(|bytes| *bytes <= FONT_PLAN_MAX_RESERVED_OP_BYTES_PER_BATCH)
                        .ok_or(FontPlanError::BuildFailed("font-plan-reserved-bytes"));
                    let next_work = state
                        .estimated_work
                        .checked_add(work)
                        .filter(|work| {
                            *work <= crate::intel::gpu_font::gpu_font_analytical_work_limit()
                        })
                        .ok_or(FontPlanError::BuildFailed("font-plan-workload"));
                    match (next_logical, next_reserved, next_work) {
                        (Ok(next_logical), Ok(next_reserved), Ok(next_work)) => {
                            state.logical_ops_bytes = next_logical;
                            state.reserved_ops_bytes = next_reserved;
                            state.estimated_work = next_work;
                            state.cells[claim.cell_index].status =
                                PlanCellStatus::Ready(Some(PreparedPlanCell {
                                    scalar: claim.scalar,
                                    prepared,
                                }));
                            Ok(())
                        }
                        (Err(error), _, _) | (_, Err(error), _) | (_, _, Err(error)) => Err(error),
                    }
                }
            };
            if let Err(error) = admitted {
                finish_batch_error(&claim.batch, error);
                retire_plan_worker(&claim.batch);
                return;
            }
            if batch.remaining.fetch_sub(1, Ordering::AcqRel) == 1 {
                finish_batch_success(&claim.batch);
                retire_plan_worker(&claim.batch);
            } else {
                retire_plan_worker(&claim.batch);
                FONT_PLAN_WAIT.notify_one();
            }
        }
        FontPlanAttemptResult::WaitForRecipe => {
            let mut state = batch.state.lock();
            if let Some(cell) = state.cells.get_mut(claim.cell_index) {
                cell.retry_candidate = Some((claim.scalar, claim.font_pixels));
                cell.status = PlanCellStatus::Pending;
            }
            drop(state);
            retire_plan_worker(&claim.batch);
        }
        FontPlanAttemptResult::Prepared(_) | FontPlanAttemptResult::Rejected => {
            batch.rejected_candidates.fetch_add(1, Ordering::AcqRel);
            let mut state = batch.state.lock();
            if let Some(cell) = state.cells.get_mut(claim.cell_index) {
                cell.status = PlanCellStatus::Pending;
            }
            drop(state);
            retire_plan_worker(&claim.batch);
            FONT_PLAN_WAIT.notify_one();
        }
    }
}

fn remove_batch(batch_id: u64) {
    FONT_PLAN_BATCHES
        .lock()
        .retain(|batch| batch.id != batch_id);
}

fn release_terminal_permit_if_quiescent(batch: &Arc<FontPlanBatch>) {
    if batch.terminal.load(Ordering::Acquire) && batch.active_workers.load(Ordering::Acquire) == 0 {
        batch.state.lock().permit.take();
    }
}

fn finish_batch_error(batch: &Arc<FontPlanBatch>, error: FontPlanError) {
    if batch
        .terminal
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    remove_batch(batch.id);
    // An accepted outline attempt is synchronous inside its one-cell slice.
    // Keep capacity charged until every such slice has observed terminal and
    // retired; otherwise cancellation could over-admit hidden CPU/memory work.
    release_terminal_permit_if_quiescent(batch);
    FONT_PLAN_FAILED.fetch_add(1, Ordering::AcqRel);
    let _ = batch.completion.complete(Err(error));
    crate::log_warn!(
        target: "render";
        "font-plan-service: batch failed id={} producer={} reason={:?} remaining={} attempts={} active_workers={} active_batches={} active_cells={} action=signal-caller+release-permit-after-quiescence\n",
        batch.id,
        batch.producer,
        error,
        batch.remaining.load(Ordering::Acquire),
        batch.candidate_attempts.load(Ordering::Acquire),
        batch.active_workers.load(Ordering::Acquire),
        FONT_PLAN_ACTIVE_BATCHES.load(Ordering::Acquire),
        FONT_PLAN_ACTIVE_CELLS.load(Ordering::Acquire),
    );
}

fn finish_batch_success(batch: &Arc<FontPlanBatch>) {
    if batch.cancelled.load(Ordering::Acquire) {
        finish_batch_error(batch, FontPlanError::Cancelled);
        return;
    }
    if batch
        .terminal
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    remove_batch(batch.id);

    let (scalars, prepared, permit, logical_ops_bytes, reserved_ops_bytes, estimated_work) = {
        let mut state = batch.state.lock();
        let logical_ops_bytes = state.logical_ops_bytes;
        let reserved_ops_bytes = state.reserved_ops_bytes;
        let estimated_work = state.estimated_work;
        let mut scalars = Vec::with_capacity(state.cells.len());
        let mut prepared = Vec::with_capacity(state.cells.len());
        for cell in &mut state.cells {
            let PlanCellStatus::Ready(slot) = &mut cell.status else {
                drop(state);
                batch.terminal.store(false, Ordering::Release);
                finish_batch_error(
                    batch,
                    FontPlanError::BuildFailed("font-plan-assembly-incomplete"),
                );
                return;
            };
            let Some(ready) = slot.take() else {
                drop(state);
                batch.terminal.store(false, Ordering::Release);
                finish_batch_error(
                    batch,
                    FontPlanError::BuildFailed("font-plan-assembly-consumed"),
                );
                return;
            };
            scalars.push(ready.scalar);
            prepared.push(ready.prepared);
        }
        let Some(permit) = state.permit.take() else {
            drop(state);
            batch.terminal.store(false, Ordering::Release);
            finish_batch_error(batch, FontPlanError::BuildFailed("font-plan-permit-missing"));
            return;
        };
        (scalars, prepared, permit, logical_ops_bytes, reserved_ops_bytes, estimated_work)
    };

    let diagnostics = plan_diagnostics(
        batch.producer,
        batch.font,
        scalars.as_slice(),
        batch.candidate_attempts.load(Ordering::Acquire),
    );
    let plan = PreparedGlyphPlan {
        fit: FontStampFit::Canvas,
        font: batch.font,
        foreground: batch.foreground,
        raster_width: batch.raster_width,
        raster_height: batch.raster_height,
        prepared,
        ops_bytes: logical_ops_bytes,
        estimated_work,
        diagnostics,
        _permit: permit,
    };
    let completed_ms = Instant::now().as_millis();
    let first_started_ms = batch.first_started_ms.load(Ordering::Acquire);
    let stats = FontPlanBuildStats {
        batch_id: batch.id,
        queue_wait_ms: first_started_ms.saturating_sub(batch.enqueued_ms),
        build_ms: completed_ms.saturating_sub(first_started_ms.max(batch.enqueued_ms)),
        candidate_attempts: batch.candidate_attempts.load(Ordering::Acquire),
        rejected_candidates: batch.rejected_candidates.load(Ordering::Acquire),
        worker_slices: batch.worker_slices.load(Ordering::Acquire),
        cooperative_yields: batch.cooperative_yields.load(Ordering::Acquire),
        parallelism: batch.parallelism,
        participant_mask: batch.participant_mask.load(Ordering::Acquire),
        reserved_ops_bytes,
    };
    FONT_PLAN_COMPLETED.fetch_add(1, Ordering::AcqRel);
    let glyphs = plan.glyph_count();
    let fingerprint = plan.diagnostics().glyph_fingerprint();
    let _ = batch
        .completion
        .complete(Ok(PreparedGlyphPlanOutput { plan, stats }));
    crate::log_info!(
        target: "render";
        "font-plan-service: batch ready id={} producer={} glyphs={} parallelism={} participants={} participant_mask=0x{:08X} queue_wait_ms={} build_ms={} attempts={} rejected={} worker_slices={} yields={} ops_bytes={} reserved_ops_bytes={} estimated_work={} glyph_hash=0x{:016X} request_rebuild=0 prepared_replay=0 recipe_cache=shared-sharded-single-flight cache_hits={} cache_misses={} cache_coalesced={} cache_builds={} cache_failures={} cache_evictions={} cache_entries={} cache_bytes={}\n",
        batch.id,
        batch.producer,
        glyphs,
        stats.parallelism,
        stats.participants(),
        stats.participant_mask,
        stats.queue_wait_ms,
        stats.build_ms,
        stats.candidate_attempts,
        stats.rejected_candidates,
        stats.worker_slices,
        stats.cooperative_yields,
        logical_ops_bytes,
        reserved_ops_bytes,
        estimated_work,
        fingerprint,
        FONT_RECIPE_HITS.load(Ordering::Acquire),
        FONT_RECIPE_MISSES.load(Ordering::Acquire),
        FONT_RECIPE_COALESCED.load(Ordering::Acquire),
        FONT_RECIPE_BUILDS.load(Ordering::Acquire),
        FONT_RECIPE_FAILURES.load(Ordering::Acquire),
        FONT_RECIPE_EVICTIONS.load(Ordering::Acquire),
        FONT_RECIPE_RESIDENT_ENTRIES.load(Ordering::Acquire),
        FONT_RECIPE_RESIDENT_BYTES.load(Ordering::Acquire),
    );
}

fn plan_diagnostics(
    producer: &'static str,
    font: crate::intel::gpu_font::GpuFontFace,
    scalars: &[char],
    candidate_attempts: u64,
) -> PreparedGlyphPlanDiagnostics {
    let mut fingerprint = 0xCBF2_9CE4_8422_2325u64;
    let mut glyph_ids_sample = String::new();
    // Preserve the previous one-layer/per-run diagnostic identity without
    // allocating a String and RetainedFontRun for every prepared glyph.
    fingerprint ^= 0;
    fingerprint = fingerprint.wrapping_mul(0x0000_0100_0000_01B3);
    fingerprint ^= u64::from(font.id());
    fingerprint = fingerprint.wrapping_mul(0x0000_0100_0000_01B3);
    for (run_index, ch) in scalars.iter().copied().enumerate() {
        fingerprint ^= run_index as u64;
        fingerprint = fingerprint.wrapping_mul(0x0000_0100_0000_01B3);
        fingerprint ^= u64::from(ch as u32);
        fingerprint = fingerprint.wrapping_mul(0x0000_0100_0000_01B3);
        if run_index < FONT_PLAN_GLYPH_ID_LOG_LIMIT {
            if !glyph_ids_sample.is_empty() {
                glyph_ids_sample.push(',');
            }
            let _ = write!(&mut glyph_ids_sample, "U+{:04X}", ch as u32);
        }
    }
    if scalars.len() > FONT_PLAN_GLYPH_ID_LOG_LIMIT {
        let _ = write!(
            &mut glyph_ids_sample,
            ",...(+{})",
            scalars.len() - FONT_PLAN_GLYPH_ID_LOG_LIMIT,
        );
    }
    PreparedGlyphPlanDiagnostics {
        producer,
        glyph_fingerprint: fingerprint,
        glyph_ids_sample,
        candidate_attempts,
    }
}

#[trueos_executor::task(pool_size = FONT_PLAN_WORKER_COUNT)]
async fn font_plan_worker_task(worker_id: usize, expected_slot: u32, expected_kind: u8) {
    let bit = 1u32 << worker_id;
    let actual_slot = u32::try_from(crate::percpu::current_slot()).unwrap_or(u32::MAX);
    let actual_kind = crate::workers::core_kind_for_slot(actual_slot);
    if actual_slot != expected_slot
        || actual_kind != expected_kind
        || !crate::workers::is_general_background_worker_slot(actual_slot)
    {
        FONT_PLAN_ADMITTED_WORKERS.fetch_and(!bit, Ordering::AcqRel);
        crate::r::spawn_service::retry_font_plan_pool_autostart();
        crate::log_error!(
            target: "render";
            "font-plan-service: worker refused worker={} expected_slot={} actual_slot={} expected_kind={} actual_kind={} action=retry-complete-topology-placement\n",
            worker_id,
            expected_slot,
            actual_slot,
            expected_kind,
            actual_kind,
        );
        return;
    }
    let online = FONT_PLAN_ONLINE_WORKERS.fetch_or(bit, Ordering::AcqRel) | bit;
    if online == FONT_PLAN_ALL_WORKERS_MASK
        && FONT_PLAN_POOL_ONLINE_LOGGED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    {
        crate::log_info!(
            target: "render";
            "font-plan-service: online workers={} active_batch_cap={} cells_per_batch={} active_cell_cap={} idle=signal-only yield=one-candidate-slice placement=ecore-preferred-complete-topology cache=none\n",
            FONT_PLAN_WORKER_COUNT,
            FONT_PLAN_MAX_ACTIVE_BATCHES,
            FONT_PLAN_MAX_CELLS_PER_BATCH,
            FONT_PLAN_MAX_ACTIVE_CELLS,
        );
    }

    loop {
        let observed = FONT_PLAN_WAIT.observe();
        match claim_plan_work(worker_id) {
            Some(FontPlanClaimResult::Attempt(claim)) => {
                let batch = Arc::clone(&claim.batch);
                run_plan_attempt(claim, worker_id);
                batch.cooperative_yields.fetch_add(1, Ordering::AcqRel);
                FONT_PLAN_COOPERATIVE_YIELDS.fetch_add(1, Ordering::AcqRel);
                // A zero-duration timer is an explicit executor turn boundary.
                // It exists only while work is active; empty workers wait on a
                // signal and allow their E-core executor to enter HLT/C-state.
                Timer::after(EmbassyDuration::from_micros(0)).await;
            }
            Some(FontPlanClaimResult::Fail(batch, error)) => {
                finish_batch_error(&batch, error);
            }
            None => FONT_PLAN_WAIT.wait_after(observed).await,
        }
    }
}

/// Spawn exactly 32 permanent, sleeping post-warm producer tasks.
///
/// The complete registered topology is inspected atomically. Efficiency cores
/// are used exclusively when present; only machines with no E-core fall back
/// to the general AP2+ fleet. BSP, AP1, and a reserved last-AP carrier are
/// never eligible.
pub(crate) fn start_font_plan_workers() -> Result<bool, SpawnError> {
    if !crate::workers::all_topology_spawners_registered() {
        return Ok(false);
    }
    let background = crate::workers::background_worker_slots();
    if background.is_empty() {
        return Ok(false);
    }
    let mut selected = background
        .iter()
        .copied()
        .filter(|slot| crate::workers::core_kind_for_slot(*slot) == crate::workers::CORE_KIND_EFF)
        .collect::<Vec<_>>();
    let ecore_policy = !selected.is_empty();
    if selected.is_empty() {
        selected = background;
    }

    let mut spawned_now = 0usize;
    for worker_id in 0..FONT_PLAN_WORKER_COUNT {
        let bit = 1u32 << worker_id;
        if FONT_PLAN_ADMITTED_WORKERS.load(Ordering::Acquire) & bit != 0 {
            continue;
        }
        let slot = selected[worker_id % selected.len()];
        let Some(spawner) = crate::workers::spawner_for_slot(slot) else {
            continue;
        };
        let kind = crate::workers::core_kind_for_slot(slot);
        // Reserve the worker id before creating its must-spawn token. A
        // concurrent/retried autostart can then skip it without ever dropping
        // a SpawnToken (which is a kernel panic by contract).
        if FONT_PLAN_ADMITTED_WORKERS.fetch_or(bit, Ordering::AcqRel) & bit != 0 {
            continue;
        }
        let token = match font_plan_worker_task(worker_id, slot, kind) {
            Ok(token) => token,
            Err(error) => {
                FONT_PLAN_ADMITTED_WORKERS.fetch_and(!bit, Ordering::AcqRel);
                return Err(error);
            }
        };
        let _ = spawner.spawn_and_wake_remote(token);
        spawned_now = spawned_now.saturating_add(1);
    }
    let admitted = FONT_PLAN_ADMITTED_WORKERS
        .load(Ordering::Acquire)
        .count_ones() as usize;
    crate::log_info!(
        target: "render";
        "font-plan-service: pool admitted spawned_now={} admitted={} workers={} selected_slots={} placement={} idle=signal-only\n",
        spawned_now,
        admitted,
        FONT_PLAN_WORKER_COUNT,
        selected.len(),
        if ecore_policy { "ecore-strict" } else { "background-fallback-no-ecore" },
    );
    Ok(admitted == FONT_PLAN_WORKER_COUNT)
}

#[cfg(test)]
mod tests {
    use super::{FontPlanCellRequest, GlyphSelectionState, resolve_warmed_font_recipe};

    #[test]
    fn exact_scalar_is_attempted_once_without_random_fallback() {
        let mut selection = GlyphSelectionState::new(7, 42.0, Some('§'));
        assert_eq!(selection.next_candidate(), Some(('§', 42.0)));
        assert_eq!(selection.next_candidate(), None);
    }

    #[test]
    fn worker_affinity_is_explicit_and_optional() {
        let rolled = FontPlanCellRequest::new([1.0, 2.0], 16.0, 0.0, 10, 11);
        assert_eq!(rolled.worker_affinity(), None);
        assert_eq!(rolled.fixed_scalar(), None);
        let exact =
            FontPlanCellRequest::fixed([1.0, 2.0], 16.0, 0.0, 10, 'T').with_worker_affinity(31);
        assert_eq!(exact.worker_affinity(), Some(31));
        assert_eq!(exact.fixed_scalar(), Some('T'));
    }

    #[test]
    fn repeated_scene_lookup_pins_the_same_shared_recipe() {
        let font = crate::intel::gpu_font::GpuFontFace::Default;
        crate::intel::gpu_font::ensure_font_face_available(font).unwrap();
        let key = crate::intel::gpu_font::gpu_font_centered_glyph_recipe_key(
            'S', font, 24.0, 0.0, 320, 200, 640, 400,
        )
        .unwrap();
        let first = resolve_warmed_font_recipe(key).unwrap();
        let second = resolve_warmed_font_recipe(key).unwrap();
        assert!(first.shares_allocation(&second));
        assert_eq!(first.key(), key);
    }
}
