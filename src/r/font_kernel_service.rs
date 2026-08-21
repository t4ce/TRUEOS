//! Shared multi-consumer GPU-font service.
//!
//! `RetainScene` yields GPU-VM-resident Skrifa coverage that a caller may
//! restamp repeatedly. `Stamp` is the one-shot path: the worker creates the
//! same retained representation temporarily. Sealed single-glyph plans take a
//! shorter route through the shared Font-VM outline and immutable R8 tile
//! caches; overlapping placements retain the proven max-union representation.
//! The service composites ordered font/color layers into either a new
//! GPU-visible premultiplied RGBA8 buffer or a leased UI4 frame, and returns
//! the owned buffer or exact producer-release proof asynchronously. Stamp
//! callers may preserve a canvas or request an exact coverage-union crop; both
//! obey the UHD/4K pixel and 4096-glyph soft caps.
//! The lane is deliberately local to real font retain/stamp work. Unrelated
//! GPU clients own admission through the GPU executor and GuC contexts.

use alloc::{boxed::Box, collections::VecDeque, string::String, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use embassy_sync::semaphore::{FairSemaphore, Semaphore, SemaphoreReleaser};
use embassy_sync::signal::Signal;
use trueos_time::{Duration as EmbassyDuration, Instant, Timer};
use spin::Mutex;

use crate::intel::gpu_font::{
    GpuFontFace, GpuFontGlyphRecipe, GpuFontGlyphRecipeKey, GpuFontJobEntry,
    GpuFontPreparedCenteredGlyph, GpuFontRetainedScene, GpuFontRetainedSceneError, GpuFontRgba,
    GpuFontTextRequest, MAX_DYNAMIC_TEXT_CHARS, classify_gpu_font_prepared_placements,
    ensure_font_face_available, font_face_is_available, font_face_supports_text,
    retain_gpu_font_centered_scene_at_raster, retain_gpu_font_prepared_centered_scene,
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
// Fourteen MiB leaves one contiguous 32 MiB extent in each Font-VA region for
// two worst-case legacy masks. Oversized individual glyphs simply retain the
// proven union path instead of growing the shared cache backing.
const FONT_GPU_CACHE_MAX_ENTRIES: usize = 4096;
const FONT_GPU_CACHE_ATLAS_WIDTH: u32 = 4096;
const FONT_GPU_CACHE_ATLAS_HEIGHT: u32 = 2560;
const FONT_GPU_CACHE_OUTLINE_BYTES: usize = 4 * 1024 * 1024;
pub(crate) const FONT_RUSH_RGBA8_CACHE_CLASSES: usize = 4;
pub(crate) const FONT_RUSH_RGBA8_CACHE_BATCHES_PER_CLASS: usize = 4;
pub(crate) const FONT_RUSH_RGBA8_CACHE_GLYPHS_PER_BATCH: usize = 32;
pub(crate) const FONT_RUSH_RGBA8_CACHE_GLYPHS_PER_CLASS: usize =
    FONT_RUSH_RGBA8_CACHE_BATCHES_PER_CLASS * FONT_RUSH_RGBA8_CACHE_GLYPHS_PER_BATCH;
pub(crate) const FONT_RUSH_RGBA8_CACHE_TILE_PX: u32 = 128;
pub(crate) const FONT_RUSH_RGBA8_CACHE_COLUMNS: u32 = 16;
pub(crate) const FONT_RUSH_RGBA8_CACHE_ROWS: u32 = 8;
pub(crate) const FONT_RUSH_RGBA8_BLAST_GLYPHS: usize = 64;
const _: () = assert!(
    FONT_RUSH_RGBA8_CACHE_COLUMNS as usize * FONT_RUSH_RGBA8_CACHE_ROWS as usize
        == FONT_RUSH_RGBA8_CACHE_GLYPHS_PER_CLASS
);

static NEXT_TICKET: AtomicU64 = AtomicU64::new(1);
#[expect(
    dead_code,
    reason = "raw Font Rush now bypasses the retained RGBA8 cache experiment"
)]
static NEXT_FONT_RUSH_RGBA8_CACHE_ID: AtomicU64 = AtomicU64::new(1);
static ONLINE: AtomicBool = AtomicBool::new(false);
static IN_FLIGHT: AtomicBool = AtomicBool::new(false);
static GPU_RETRY_DELAY_PENDING: AtomicBool = AtomicBool::new(false);
static LAST_RETAIN_PARTITION_LOG_TICKET: AtomicU64 = AtomicU64::new(0);
static LAST_STAMP_PARTITION_LOG_TICKET: AtomicU64 = AtomicU64::new(0);
static WORK_AVAILABLE: Signal<crate::wait::EmbassySpinRawMutex, ()> = Signal::new();
static REQUESTS: Mutex<VecDeque<QueuedFontRequest>> = Mutex::new(VecDeque::new());
static STATUS: Mutex<FontKernelServiceStatus> = Mutex::new(FontKernelServiceStatus::new());
static FONT_GPU_CACHE: Mutex<FontGpuCache> = Mutex::new(FontGpuCache::new());
static GPU_LANE: FairSemaphore<crate::wait::EmbassySpinRawMutex, FONT_KERNEL_GPU_WAITERS> =
    FairSemaphore::new(1);

struct FontGpuResidentOutline {
    key: GpuFontGlyphRecipeKey,
    ops: crate::intel::gpgpu::GpgpuFontOutlineOps,
    last_touch: u64,
}

struct FontGpuResidentTile {
    key: GpuFontGlyphRecipeKey,
    tile: crate::intel::gpgpu::GpgpuMask8AtlasTile,
    last_touch: u64,
}

struct FontGpuCache {
    outline_arena: Option<crate::intel::gpgpu::GpgpuOwnedFontOutlineOpsArena>,
    coverage_atlas: Option<crate::intel::gpgpu::GpgpuOwnedMask8Atlas>,
    outlines: Vec<FontGpuResidentOutline>,
    tiles: Vec<FontGpuResidentTile>,
    touch: u64,
    outline_hits: u64,
    outline_misses: u64,
    tile_hits: u64,
    tile_misses: u64,
    evictions: u64,
    poisoned: bool,
}

impl FontGpuCache {
    const fn new() -> Self {
        Self {
            outline_arena: None,
            coverage_atlas: None,
            outlines: Vec::new(),
            tiles: Vec::new(),
            touch: 0,
            outline_hits: 0,
            outline_misses: 0,
            tile_hits: 0,
            tile_misses: 0,
            evictions: 0,
            poisoned: false,
        }
    }

    fn next_touch(&mut self) -> u64 {
        self.touch = self.touch.wrapping_add(1).max(1);
        self.touch
    }

    fn evict_oldest_outline(&mut self) -> bool {
        let Some(index) = self
            .outlines
            .iter()
            .enumerate()
            .min_by_key(|(_, entry)| entry.last_touch)
            .map(|(index, _)| index)
        else {
            return false;
        };
        self.outlines.swap_remove(index);
        self.evictions = self.evictions.saturating_add(1);
        true
    }

    fn evict_oldest_tile_except(&mut self, protected: &[CachedPreparedGlyph]) -> bool {
        let Some(index) = self
            .tiles
            .iter()
            .enumerate()
            .filter(|(_, entry)| !protected.iter().any(|used| used.key == entry.key))
            .min_by_key(|(_, entry)| entry.last_touch)
            .map(|(index, _)| index)
        else {
            return false;
        };
        self.tiles.swap_remove(index);
        self.evictions = self.evictions.saturating_add(1);
        true
    }

    fn poison_coverage_build(&mut self) {
        self.poisoned = true;
        self.tiles.clear();
        self.outlines.clear();
        self.coverage_atlas = None;
        self.outline_arena = None;
    }

    fn quarantine_atlas_after_ambiguous_read(&mut self, used_tiles: &[CachedPreparedGlyph]) {
        self.poisoned = true;
        for used in used_tiles {
            used.tile.quarantine();
        }
        self.tiles.clear();
        self.coverage_atlas = None;
    }

    fn mark_poisoned(&mut self) {
        self.poisoned = true;
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct FontGpuFrameCacheStats {
    outline_hits: usize,
    outline_misses: usize,
    tile_hits: usize,
    tile_misses: usize,
    evictions: usize,
    resident_outlines: usize,
    resident_tiles: usize,
    coverage_build_ms: u64,
    coverage_audit_ms: u64,
    coverage_submits: usize,
}

struct FontGpuCachePreparationError {
    error: FontKernelError,
    stats: FontGpuFrameCacheStats,
}

struct CachedPreparedGlyph {
    key: GpuFontGlyphRecipeKey,
    tile: crate::intel::gpgpu::GpgpuMask8AtlasTile,
    destination_xy: [i32; 2],
}

/// Run-owned cache of fully shaded, size-specific RGBA8 glyph tiles.
///
/// This deliberately does not participate in the ordinary boot-persistent
/// outline/R8 cache. Font Rush allocates all four atlases after its visible
/// showcase, fills each fixed cell once, seals the set, and drops the complete
/// 32 MiB object when that one demo run ends.
pub(crate) struct FontRushRgba8Cache {
    id: u64,
    atlases: [crate::intel::gpgpu::GpgpuOwnedFontRushRgba8Atlas; FONT_RUSH_RGBA8_CACHE_CLASSES],
    reserved_batches: AtomicU32,
    ready_batches: AtomicU32,
    poisoned: AtomicBool,
    terminal_path_warm: AtomicBool,
    sealed: AtomicBool,
}

impl FontRushRgba8Cache {
    pub(crate) const fn id(&self) -> u64 {
        self.id
    }

    #[expect(
        dead_code,
        reason = "raw Font Rush now bypasses the retained RGBA8 cache experiment"
    )]
    pub(crate) const fn atlas_extent(&self) -> (u32, u32) {
        (
            crate::intel::gpgpu::GPGPU_FONT_RUSH_RGBA8_ATLAS_WIDTH,
            crate::intel::gpgpu::GPGPU_FONT_RUSH_RGBA8_ATLAS_HEIGHT,
        )
    }

    pub(crate) fn atlas_surface(
        &self,
        class: u8,
    ) -> Option<crate::intel::gpgpu::GpgpuRgba8Surface> {
        self.atlases
            .get(usize::from(class))
            .map(|atlas| atlas.surface())
    }

    fn batch_bit(class: u8, batch: u8) -> Option<u32> {
        let class = usize::from(class);
        let batch = usize::from(batch);
        if class >= FONT_RUSH_RGBA8_CACHE_CLASSES
            || batch >= FONT_RUSH_RGBA8_CACHE_BATCHES_PER_CLASS
        {
            return None;
        }
        Some(1u32 << (class * FONT_RUSH_RGBA8_CACHE_BATCHES_PER_CLASS + batch))
    }

    pub(crate) fn batch_ready(&self, class: u8, batch: u8) -> bool {
        Self::batch_bit(class, batch)
            .is_some_and(|bit| self.ready_batches.load(Ordering::Acquire) & bit != 0)
    }

    fn reserve_batch(&self, class: u8, batch: u8) -> Result<(), FontKernelError> {
        let bit = Self::batch_bit(class, batch)
            .ok_or(FontKernelError::InvalidRequest("font-rush-cache-batch"))?;
        if self.sealed.load(Ordering::Acquire) || self.poisoned.load(Ordering::Acquire) {
            return Err(FontKernelError::InvalidRequest("font-rush-cache-closed"));
        }
        if self.ready_batches.load(Ordering::Acquire) & bit != 0 {
            return Err(FontKernelError::InvalidRequest("font-rush-cache-batch-ready"));
        }
        let mut reserved = self.reserved_batches.load(Ordering::Acquire);
        loop {
            if reserved & bit != 0 {
                return Err(FontKernelError::InvalidRequest("font-rush-cache-batch-in-flight"));
            }
            match self.reserved_batches.compare_exchange_weak(
                reserved,
                reserved | bit,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(current) => reserved = current,
            }
        }
        if self.sealed.load(Ordering::Acquire) || self.poisoned.load(Ordering::Acquire) {
            self.reserved_batches.fetch_and(!bit, Ordering::AcqRel);
            return Err(FontKernelError::InvalidRequest("font-rush-cache-closed"));
        }
        Ok(())
    }

    fn batch_reserved(&self, class: u8, batch: u8) -> bool {
        Self::batch_bit(class, batch)
            .is_some_and(|bit| self.reserved_batches.load(Ordering::Acquire) & bit != 0)
    }

    fn release_batch_reservation(&self, class: u8, batch: u8) {
        if let Some(bit) = Self::batch_bit(class, batch) {
            self.reserved_batches.fetch_and(!bit, Ordering::AcqRel);
        }
    }

    fn commit_batch(&self, class: u8, batch: u8) -> Result<(), FontKernelError> {
        if self.sealed.load(Ordering::Acquire) || self.poisoned.load(Ordering::Acquire) {
            return Err(FontKernelError::InvalidRequest("font-rush-cache-closed"));
        }
        let bit = Self::batch_bit(class, batch)
            .ok_or(FontKernelError::InvalidRequest("font-rush-cache-batch"))?;
        if self.reserved_batches.load(Ordering::Acquire) & bit == 0
            || self.ready_batches.load(Ordering::Acquire) & bit != 0
        {
            return Err(FontKernelError::InvalidRequest("font-rush-cache-batch-state"));
        }
        self.ready_batches.fetch_or(bit, Ordering::AcqRel);
        self.reserved_batches.fetch_and(!bit, Ordering::AcqRel);
        Ok(())
    }

    fn fail_batch(&self, class: u8, batch: u8, error: FontKernelError) {
        if matches!(error, FontKernelError::SubmittedIncomplete(_)) {
            // An accepted GPU request may still address this atlas. Keep its
            // reservation visible and close the complete cache permanently.
            self.poisoned.store(true, Ordering::Release);
        } else {
            self.release_batch_reservation(class, batch);
        }
    }

    fn mark_terminal_path_warm(&self) {
        self.terminal_path_warm.store(true, Ordering::Release);
    }

    fn terminal_path_is_warm(&self) -> bool {
        self.terminal_path_warm.load(Ordering::Acquire)
    }

    pub(crate) fn seal(&self) -> Result<(), FontKernelError> {
        let expected =
            (1u32 << (FONT_RUSH_RGBA8_CACHE_CLASSES * FONT_RUSH_RGBA8_CACHE_BATCHES_PER_CLASS)) - 1;
        if self.ready_batches.load(Ordering::Acquire) != expected
            || self.reserved_batches.load(Ordering::Acquire) != 0
            || self.poisoned.load(Ordering::Acquire)
            || !self.terminal_path_is_warm()
        {
            return Err(FontKernelError::InvalidRequest("font-rush-cache-incomplete"));
        }
        self.sealed.store(true, Ordering::Release);
        Ok(())
    }

    pub(crate) fn is_sealed(&self) -> bool {
        self.sealed.load(Ordering::Acquire)
    }
}

#[expect(
    dead_code,
    reason = "raw Font Rush now bypasses the retained RGBA8 cache experiment"
)]
pub(crate) fn allocate_font_rush_rgba8_cache() -> Result<Arc<FontRushRgba8Cache>, FontKernelError> {
    let atlases = [
        crate::intel::gpgpu::allocate_font_rush_rgba8_atlas(0),
        crate::intel::gpgpu::allocate_font_rush_rgba8_atlas(1),
        crate::intel::gpgpu::allocate_font_rush_rgba8_atlas(2),
        crate::intel::gpgpu::allocate_font_rush_rgba8_atlas(3),
    ];
    let [Some(atlas0), Some(atlas1), Some(atlas2), Some(atlas3)] = atlases else {
        return Err(FontKernelError::Unavailable("font-rush-rgba8-cache-allocation"));
    };
    let id = loop {
        let id = NEXT_FONT_RUSH_RGBA8_CACHE_ID.fetch_add(1, Ordering::AcqRel);
        if id != 0 {
            break id;
        }
    };
    Ok(Arc::new(FontRushRgba8Cache {
        id,
        atlases: [atlas0, atlas1, atlas2, atlas3],
        reserved_batches: AtomicU32::new(0),
        ready_batches: AtomicU32::new(0),
        poisoned: AtomicBool::new(false),
        terminal_path_warm: AtomicBool::new(false),
        sealed: AtomicBool::new(false),
    }))
}

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
    #[expect(
        dead_code,
        reason = "raw Font Rush now routes terminal waves through prepared plans"
    )]
    FontRushRgba8Blast {
        cache: Arc<FontRushRgba8Cache>,
        wave: u64,
        raster_width: u32,
        raster_height: u32,
    },
}

impl FrameStampInput {
    fn fit(&self) -> FontStampFit {
        match self {
            Self::Request(request) => request.fit,
            Self::Prepared(plan) => plan.fit(),
            Self::FontRushClear { .. }
            | Self::FontRushRgba8Sprites { .. }
            | Self::FontRushRgba8Blast { .. } => FontStampFit::Canvas,
        }
    }

    fn raster_extent(&self) -> Option<(u32, u32)> {
        match self {
            Self::Request(request) => request
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
            }
            | Self::FontRushRgba8Blast {
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

/// A Font Rush charge plan was not admitted; the exact plan remains owned by
/// the caller and may be retried without rebuilding it.
pub(crate) struct PreparedFontRushCacheChargeRejection {
    error: FontKernelError,
    plan: PreparedGlyphPlan,
}

impl PreparedFontRushCacheChargeRejection {
    pub(crate) fn into_parts(self) -> (FontKernelError, PreparedGlyphPlan) {
        (self.error, self.plan)
    }
}

pub(crate) struct FontRushCacheCharge {
    ticket: FontKernelTicket,
    cache_id: u64,
    class: u8,
    batch: u8,
    glyphs: usize,
    submits: usize,
    active_walkers: usize,
    total_service_ms: u64,
}

impl FontRushCacheCharge {
    pub(crate) const fn ticket(&self) -> FontKernelTicket {
        self.ticket
    }

    pub(crate) const fn cache_id(&self) -> u64 {
        self.cache_id
    }

    pub(crate) const fn class(&self) -> u8 {
        self.class
    }

    pub(crate) const fn batch(&self) -> u8 {
        self.batch
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

    pub(crate) const fn total_service_ms(&self) -> u64 {
        self.total_service_ms
    }
}

/// One logical retained scene backed by bounded analytical coverage masks.
///
/// Gridpaper uses the same low-level model: keep independently admitted R8
/// masks resident, then composite them together as one draw-time layer batch.
pub(crate) struct FontKernelRetainedScene {
    masks: Vec<GpuFontRetainedScene>,
    /// CPU recipe resources resolved from the shared warmed-outline cache.
    /// Holding these leases makes their exact policy revision available to a
    /// future RenderTicket without copying outline ops into SceneDB. The R8
    /// atlas tile remains a later renderer-owned pin because this retained
    /// compatibility path still produces its own union masks.
    _lookup_recipes: Vec<crate::r::font_plan_service::WarmedFontRecipeLease>,
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
    gpu_outline_cache_hits: usize,
    gpu_outline_cache_misses: usize,
    gpu_tile_cache_hits: usize,
    gpu_tile_cache_misses: usize,
    gpu_cache_evictions: usize,
    gpu_resident_outlines: usize,
    gpu_resident_tiles: usize,
    instance_release_ms: u64,
    total_service_ms: u64,
    release: crate::intel::gpgpu::GpgpuRgba8ReleaseFence,
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

    pub(crate) const fn gpu_outline_cache_hits(&self) -> usize {
        self.gpu_outline_cache_hits
    }

    pub(crate) const fn gpu_outline_cache_misses(&self) -> usize {
        self.gpu_outline_cache_misses
    }

    pub(crate) const fn gpu_tile_cache_hits(&self) -> usize {
        self.gpu_tile_cache_hits
    }

    pub(crate) const fn gpu_tile_cache_misses(&self) -> usize {
        self.gpu_tile_cache_misses
    }

    pub(crate) const fn gpu_cache_evictions(&self) -> usize {
        self.gpu_cache_evictions
    }

    pub(crate) const fn gpu_resident_outlines(&self) -> usize {
        self.gpu_resident_outlines
    }

    pub(crate) const fn gpu_resident_tiles(&self) -> usize {
        self.gpu_resident_tiles
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

pub(crate) struct PendingFontRushCacheCharge {
    ticket: FontKernelTicket,
    queued_ahead: usize,
    reply:
        Arc<Signal<crate::wait::EmbassySpinRawMutex, Result<FontRushCacheCharge, FontKernelError>>>,
}

impl PendingFontRushCacheCharge {
    pub(crate) const fn ticket(&self) -> FontKernelTicket {
        self.ticket
    }

    pub(crate) const fn queued_ahead(&self) -> usize {
        self.queued_ahead
    }

    pub(crate) fn try_take(&mut self) -> Option<Result<FontRushCacheCharge, FontKernelError>> {
        self.reply.try_take()
    }

    pub(crate) async fn wait(self) -> Result<FontRushCacheCharge, FontKernelError> {
        self.reply.wait().await
    }
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
        enqueued_ms: u64,
        reply:
            Arc<Signal<crate::wait::EmbassySpinRawMutex, Result<FontFrameStamp, FontKernelError>>>,
    },
    FontRushCacheCharge {
        ticket: FontKernelTicket,
        plan: PreparedGlyphPlan,
        cache: Arc<FontRushRgba8Cache>,
        class: u8,
        batch: u8,
        enqueued_ms: u64,
        reply: Arc<
            Signal<crate::wait::EmbassySpinRawMutex, Result<FontRushCacheCharge, FontKernelError>>,
        >,
    },
}

impl QueuedFontRequest {
    const fn ticket(&self) -> FontKernelTicket {
        match self {
            Self::Retain { ticket, .. }
            | Self::Stamp { ticket, .. }
            | Self::FrameStamp { ticket, .. }
            | Self::FontRushCacheCharge { ticket, .. } => *ticket,
        }
    }

    const fn consumer(&self) -> FontKernelConsumer {
        let path = match self {
            Self::Retain { .. } => FontKernelConsumerPath::RetainScene,
            Self::Stamp { .. } | Self::FrameStamp { .. } | Self::FontRushCacheCharge { .. } => {
                FontKernelConsumerPath::Stamp
            }
        };
        FontKernelConsumer::new(path, self.ticket().raw())
    }

    /// Requests built from raw text must not enter the service lane until all
    /// their faces have a complete registered outline. Prepared plans have
    /// already crossed that boundary; Font Rush cache operations contain no
    /// raw face lookup at all.
    async fn wait_for_fonts(&self) {
        match self {
            Self::Retain { request, .. } => {
                wait_for_font_registration(self.ticket(), request.font).await;
            }
            Self::Stamp { request, .. }
            | Self::FrameStamp {
                input: FrameStampInput::Request(request),
                ..
            } => {
                for layer in &request.layers {
                    wait_for_font_registration(self.ticket(), layer.scene.font).await;
                }
            }
            Self::FrameStamp { .. } | Self::FontRushCacheCharge { .. } => {}
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
    queue_frame_stamp(FrameStampInput::Request(request), destination, None)
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
    queue_frame_stamp(FrameStampInput::Prepared(plan), destination, Some(clear_rgba)).map_err(
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
    queue_frame_stamp(FrameStampInput::Prepared(plan), destination, None).map_err(|rejection| {
        let FrameStampInput::Prepared(plan) = rejection.input else {
            unreachable!("prepared frame admission returned a request input")
        };
        PreparedFrameStampRejection {
            error: rejection.error,
            plan,
        }
    })
}

/// Queue one 32-glyph charge slice into a private, transparent RGBA8 atlas.
///
/// The destination is an ordinary PAT0 Font resource, not scanout. Completion
/// therefore proves the materialized pixels retired but intentionally mints no
/// display release token. Queue rejection returns the exact sealed plan.
pub(crate) fn submit_prepared_font_rush_cache_charge(
    plan: PreparedGlyphPlan,
    cache: Arc<FontRushRgba8Cache>,
    class: u8,
    batch: u8,
) -> Result<PendingFontRushCacheCharge, PreparedFontRushCacheChargeRejection> {
    let reject = |error, plan| PreparedFontRushCacheChargeRejection { error, plan };
    let Some(destination) = cache.atlas_surface(class) else {
        return Err(reject(FontKernelError::InvalidRequest("font-rush-cache-class"), plan));
    };
    if cache.is_sealed()
        || cache.batch_ready(class, batch)
        || usize::from(batch) >= FONT_RUSH_RGBA8_CACHE_BATCHES_PER_CLASS
        || plan.fit() != FontStampFit::Canvas
        || plan.glyph_count() != FONT_RUSH_RGBA8_CACHE_GLYPHS_PER_BATCH
        || (plan.raster_width(), plan.raster_height()) != (destination.width, destination.height)
    {
        return Err(reject(
            FontKernelError::InvalidRequest("font-rush-cache-charge-contract"),
            plan,
        ));
    }
    if let Err(error) = cache.reserve_batch(class, batch) {
        return Err(reject(error, plan));
    }

    let ticket = next_ticket();
    let reply = Arc::new(Signal::new());
    let queued_ahead = {
        let mut queue = REQUESTS.lock();
        if queue.len() >= FONT_KERNEL_QUEUE_CAPACITY {
            cache.release_batch_reservation(class, batch);
            return Err(reject(FontKernelError::QueueFull, plan));
        }
        let queued_ahead = queue.len();
        queue.push_back(QueuedFontRequest::FontRushCacheCharge {
            ticket,
            plan,
            cache,
            class,
            batch,
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
    Ok(PendingFontRushCacheCharge {
        ticket,
        queued_ahead,
        reply,
    })
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
    )
    .map_err(|rejection| rejection.error)
}

/// Queue one terminal Font Rush wave using only sealed RGBA8 tiles.
#[expect(
    dead_code,
    reason = "raw Font Rush now routes terminal waves through prepared plans"
)]
pub(crate) fn submit_font_rush_rgba8_cache_blast(
    cache: Arc<FontRushRgba8Cache>,
    destination: crate::intel::gpgpu::GpgpuRgba8Surface,
    wave: u64,
) -> Result<PendingFontFrameStamp, FontKernelError> {
    if !cache.is_sealed() {
        return Err(FontKernelError::InvalidRequest("font-rush-cache-not-sealed"));
    }
    queue_frame_stamp(
        FrameStampInput::FontRushRgba8Blast {
            cache,
            wave,
            raster_width: destination.width,
            raster_height: destination.height,
        },
        destination,
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
    let lookup_recipes = resolve_scene_lookup_recipes(request, glyph_runs.as_slice());
    let mut masks = Vec::new();
    process_retain_scene_partition(ticket, request, glyph_runs.as_slice(), &mut masks)?;
    Ok(FontKernelRetainedScene {
        masks,
        _lookup_recipes: lookup_recipes,
    })
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
    for run in &request.runs {
        let mut pen_x = 0.0f32;
        for ch in run.text.chars() {
            let mut glyph = String::new();
            glyph.push(ch);
            let advance = crate::graphics::font::text_advance_width(
                request.font.registry_name(),
                glyph.as_str(),
                run.font_pixels,
            )
            .map_err(FontKernelError::Unavailable)?;
            if !ch.is_whitespace() && font_face_supports_text(request.font, glyph.as_str()) {
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
        "font-kernel-service: bounded glyph layout ticket={} source_runs={} glyph_entries={} positioning=scene-origin policy=per-glyph-analytical-coverage\n",
        ticket.raw(),
        request.runs.len(),
        glyph_runs.len(),
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
    // A Picasso FontLookup row remains Unicode/style data. Resolve its exact
    // glyph recipe identity here, on FontKernel's leased blocking lane, and
    // seed the same shared recipe cache used by prepared producers before the
    // compatibility retained-canvas draw. No outline ops or atlas bytes cross
    // the SceneDB boundary. The visual fallback below deliberately retains its
    // existing single release/fence contract.
    set_active_stage(ticket, "scene-lookup-prewarm");
    let _ = prewarm_scene_lookup_recipes(request, runs);
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

fn prewarm_scene_lookup_recipes(request: &RetainSceneRequest, runs: &[RetainedFontRun]) -> usize {
    let mut warmed = 0usize;
    for run in runs {
        for scalar in run.text.chars() {
            if scalar.is_control() || scalar.is_whitespace() {
                continue;
            }
            let Ok(key) = crate::intel::gpu_font::gpu_font_centered_glyph_recipe_key(
                scalar,
                request.font,
                run.font_pixels,
                run.slant,
                request.viewport_width,
                request.viewport_height,
                request.raster_width,
                request.raster_height,
            ) else {
                // Analytical lookup is an optional acceleration. The retained
                // canvas below still owns validation and its mesh/union
                // fallback, including sizes outside the R8 recipe range.
                continue;
            };
            if crate::r::font_plan_service::resolve_warmed_font_recipe(key).is_ok() {
                warmed = warmed.saturating_add(1);
            }
            // Wait/saturation/build failures must never reject an otherwise
            // valid text request. A later frame can hit the shared cache, and
            // the compatibility retained-canvas path remains authoritative.
        }
    }
    warmed
}

fn resolve_scene_lookup_recipes(
    request: &RetainSceneRequest,
    runs: &[RetainedFontRun],
) -> Vec<crate::r::font_plan_service::WarmedFontRecipeLease> {
    let mut leases: Vec<crate::r::font_plan_service::WarmedFontRecipeLease> = Vec::new();
    for run in runs {
        for scalar in run.text.chars() {
            if scalar.is_control() || scalar.is_whitespace() {
                continue;
            }
            let Ok(key) = crate::intel::gpu_font::gpu_font_centered_glyph_recipe_key(
                scalar,
                request.font,
                run.font_pixels,
                run.slant,
                request.viewport_width,
                request.viewport_height,
                request.raster_width,
                request.raster_height,
            ) else {
                continue;
            };
            if leases.iter().any(|lease| lease.key() == key) {
                continue;
            }
            if let Ok(lease) = crate::r::font_plan_service::resolve_warmed_font_recipe(key) {
                leases.push(lease);
            }
        }
    }
    leases
}

fn collect_stamp_scenes(
    ticket: FontKernelTicket,
    layer: &FontStampLayer,
    runs: &[RetainedFontRun],
    scenes: &mut Vec<(GpuFontRetainedScene, GpuFontRgba)>,
) -> Result<(), FontKernelError> {
    let scene = match process_retain_scene_runs(ticket, &layer.scene, runs) {
        Ok(scene) => scene,
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
    for layer in &request.layers {
        glyphs = layer
            .scene
            .runs
            .iter()
            .fold(glyphs, |total, run| total.saturating_add(run.text.chars().count()));
        let glyph_runs = expand_origin_runs(ticket, &layer.scene)?;
        collect_stamp_scenes(ticket, layer, glyph_runs.as_slice(), &mut scenes)?;
    }
    Ok((scenes, glyphs))
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

fn font_gpu_tile_audit_covers_recipe(
    recipe: &GpuFontGlyphRecipe,
    tile: &crate::intel::gpgpu::GpgpuMask8AtlasTile,
) -> bool {
    let expected = recipe.local_audit_rect();
    let actual = tile.audit();
    if expected.is_empty() || actual.nonzero_pixels == 0 || actual.bounds.is_empty() {
        return false;
    }
    let expected_right = i64::from(expected.x) + i64::from(expected.width);
    let expected_bottom = i64::from(expected.y) + i64::from(expected.height);
    let actual_right = i64::from(actual.bounds.x) + i64::from(actual.bounds.width);
    let actual_bottom = i64::from(actual.bounds.y) + i64::from(actual.bounds.height);
    const EDGE_AUDIT_SLOP_PX: i64 = 2;
    i64::from(actual.bounds.x) <= i64::from(expected.x) + EDGE_AUDIT_SLOP_PX
        && i64::from(actual.bounds.y) <= i64::from(expected.y) + EDGE_AUDIT_SLOP_PX
        && actual_right + EDGE_AUDIT_SLOP_PX >= expected_right
        && actual_bottom + EDGE_AUDIT_SLOP_PX >= expected_bottom
}

fn acquire_font_gpu_resident_outline(
    cache: &mut FontGpuCache,
    recipe: &GpuFontGlyphRecipe,
) -> Result<crate::intel::gpgpu::GpgpuFontOutlineOps, FontKernelError> {
    let key = recipe.key();
    if let Some(index) = cache.outlines.iter().position(|entry| entry.key == key) {
        let touch = cache.next_touch();
        cache.outlines[index].last_touch = touch;
        cache.outline_hits = cache.outline_hits.saturating_add(1);
        return Ok(cache.outlines[index].ops.clone());
    }
    cache.outline_misses = cache.outline_misses.saturating_add(1);
    if cache.outline_arena.is_none() {
        cache.outline_arena = crate::intel::gpgpu::allocate_font_outline_ops_arena(
            FONT_GPU_CACHE_OUTLINE_BYTES,
            FONT_GPU_CACHE_MAX_ENTRIES,
        );
    }
    let arena = cache
        .outline_arena
        .as_ref()
        .cloned()
        .ok_or(FontKernelError::Unavailable("font-gpu-outline-arena"))?;
    while cache.outlines.len() >= FONT_GPU_CACHE_MAX_ENTRIES {
        if !cache.evict_oldest_outline() {
            return Err(FontKernelError::Unavailable("font-gpu-outline-cache-cap"));
        }
    }
    let mut eviction_budget = cache.outlines.len();
    let ops = loop {
        if let Some(ops) = arena.insert(recipe.outline_ops()) {
            break ops;
        }
        if eviction_budget == 0 || !cache.evict_oldest_outline() {
            return Err(FontKernelError::Unavailable("font-gpu-outline-cache-space"));
        }
        eviction_budget -= 1;
    };
    let touch = cache.next_touch();
    cache.outlines.push(FontGpuResidentOutline {
        key,
        ops: ops.clone(),
        last_touch: touch,
    });
    Ok(ops)
}

fn acquire_font_gpu_coverage_tile(
    recipe: &GpuFontGlyphRecipe,
    stats: &mut FontGpuFrameCacheStats,
    protected: &[CachedPreparedGlyph],
) -> Result<crate::intel::gpgpu::GpgpuMask8AtlasTile, FontKernelError> {
    let acquire_started_ms = Instant::now().as_millis();
    let key = recipe.key();
    let (width, height) = recipe.mask_extent();
    let (resident_ops, reservation) = {
        let mut cache = FONT_GPU_CACHE.lock();
        if cache.poisoned {
            return Err(FontKernelError::Unavailable("font-gpu-cache-poisoned"));
        }
        if let Some(index) = cache.tiles.iter().position(|entry| entry.key == key) {
            let touch = cache.next_touch();
            cache.tiles[index].last_touch = touch;
            cache.tile_hits = cache.tile_hits.saturating_add(1);
            stats.tile_hits = stats.tile_hits.saturating_add(1);
            return Ok(cache.tiles[index].tile.clone());
        }
        cache.tile_misses = cache.tile_misses.saturating_add(1);
        stats.tile_misses = stats.tile_misses.saturating_add(1);

        let outline_hits_before = cache.outline_hits;
        let outline_misses_before = cache.outline_misses;
        let resident_ops_result = acquire_font_gpu_resident_outline(&mut cache, recipe);
        stats.outline_hits = stats
            .outline_hits
            .saturating_add(cache.outline_hits.saturating_sub(outline_hits_before) as usize);
        stats.outline_misses = stats
            .outline_misses
            .saturating_add(cache.outline_misses.saturating_sub(outline_misses_before) as usize);
        let resident_ops = resident_ops_result?;

        if cache.coverage_atlas.is_none() {
            cache.coverage_atlas = crate::intel::gpgpu::allocate_font_coverage_atlas(
                FONT_GPU_CACHE_ATLAS_WIDTH,
                FONT_GPU_CACHE_ATLAS_HEIGHT,
                FONT_GPU_CACHE_MAX_ENTRIES,
            );
        }
        let atlas = cache
            .coverage_atlas
            .as_ref()
            .cloned()
            .ok_or(FontKernelError::Unavailable("font-gpu-coverage-atlas"))?;
        while cache.tiles.len() >= FONT_GPU_CACHE_MAX_ENTRIES {
            if !cache.evict_oldest_tile_except(protected) {
                return Err(FontKernelError::Unavailable("font-gpu-tile-cache-cap"));
            }
        }
        let mut eviction_budget = cache.tiles.len();
        let reservation = loop {
            if let Some(reservation) = atlas.reserve(width, height) {
                break reservation;
            }
            if eviction_budget == 0 || !cache.evict_oldest_tile_except(protected) {
                return Err(FontKernelError::Unavailable("font-gpu-tile-cache-space"));
            }
            eviction_budget -= 1;
        };
        (resident_ops, reservation)
    };

    // The cache spin lock is deliberately released before the bounded GPU
    // wait and one-time CPU audit. FontKernel serializes builders already.
    let local_rect = crate::intel::gpgpu::GpgpuRect::new(0, 0, width, height);
    let tile = match crate::intel::gpgpu::font_outline_coverage_atlas_tile_r8(
        reservation,
        &resident_ops,
        local_rect,
        recipe.coverage_subdivisions(),
        recipe.optical_bias_px(),
    ) {
        Ok(tile) => tile,
        Err(crate::intel::gpgpu::GpgpuDispatchRetirement::SubmittedIncomplete) => {
            FONT_GPU_CACHE.lock().poison_coverage_build();
            return Err(FontKernelError::SubmittedIncomplete("font-gpu-tile-coverage-incomplete"));
        }
        Err(crate::intel::gpgpu::GpgpuDispatchRetirement::NotSubmitted) => {
            return Err(FontKernelError::Unavailable("font-gpu-tile-coverage"));
        }
        Err(crate::intel::gpgpu::GpgpuDispatchRetirement::Complete) => {
            stats.coverage_build_ms = stats.coverage_build_ms.saturating_add(
                Instant::now()
                    .as_millis()
                    .saturating_sub(acquire_started_ms),
            );
            stats.coverage_submits = stats.coverage_submits.saturating_add(1);
            return Err(FontKernelError::Unavailable("font-gpu-tile-audit"));
        }
    };
    let total_build_ms = Instant::now()
        .as_millis()
        .saturating_sub(acquire_started_ms);
    let audit_ms = tile.coverage_audit_ms();
    stats.coverage_build_ms = stats
        .coverage_build_ms
        .saturating_add(total_build_ms.saturating_sub(audit_ms));
    stats.coverage_audit_ms = stats.coverage_audit_ms.saturating_add(audit_ms);
    stats.coverage_submits = stats.coverage_submits.saturating_add(1);
    if !font_gpu_tile_audit_covers_recipe(recipe, &tile) {
        return Err(FontKernelError::Unavailable("font-gpu-tile-audit"));
    }
    let mut cache = FONT_GPU_CACHE.lock();
    let touch = cache.next_touch();
    cache.tiles.push(FontGpuResidentTile {
        key,
        tile: tile.clone(),
        last_touch: touch,
    });
    Ok(tile)
}

fn prepare_cached_font_gpu_glyphs(
    prepared: &[GpuFontPreparedCenteredGlyph],
    unique_indices: &[usize],
) -> Result<(Vec<CachedPreparedGlyph>, FontGpuFrameCacheStats), FontGpuCachePreparationError> {
    let evictions_before = FONT_GPU_CACHE.lock().evictions;
    let mut stats = FontGpuFrameCacheStats::default();
    let mut cached = Vec::with_capacity(unique_indices.len());
    for &index in unique_indices {
        let Some(glyph) = prepared.get(index) else {
            snapshot_font_gpu_cache_stats(&mut stats, evictions_before);
            return Err(FontGpuCachePreparationError {
                error: FontKernelError::InvalidRequest("font-prepared-index"),
                stats,
            });
        };
        let tile =
            match acquire_font_gpu_coverage_tile(glyph.recipe(), &mut stats, cached.as_slice()) {
                Ok(tile) => tile,
                Err(error) => {
                    snapshot_font_gpu_cache_stats(&mut stats, evictions_before);
                    return Err(FontGpuCachePreparationError { error, stats });
                }
            };
        cached.push(CachedPreparedGlyph {
            key: glyph.recipe().key(),
            tile,
            destination_xy: glyph.destination_xy(),
        });
    }
    snapshot_font_gpu_cache_stats(&mut stats, evictions_before);
    Ok((cached, stats))
}

fn snapshot_font_gpu_cache_stats(stats: &mut FontGpuFrameCacheStats, evictions_before: u64) {
    let cache = FONT_GPU_CACHE.lock();
    stats.evictions = cache.evictions.saturating_sub(evictions_before) as usize;
    stats.resident_outlines = cache.outlines.len();
    stats.resident_tiles = cache.tiles.len();
}

fn stamp_cached_font_gpu_glyphs(
    cached: &[CachedPreparedGlyph],
    foreground: GpuFontRgba,
    destination: crate::intel::gpgpu::GpgpuRgba8Surface,
    frame_already_irreversible: bool,
) -> Result<(usize, usize, crate::intel::gpgpu::GpgpuRgba8ReleaseFence), FontKernelError> {
    let color_rgba = u32::from_le_bytes([foreground.r, foreground.g, foreground.b, foreground.a]);
    let layers = cached
        .iter()
        .map(|glyph| crate::intel::gpgpu::GpgpuGlyphMaskLayer {
            mask: glyph.tile.surface(),
            mask_rect: glyph.tile.rect(),
            dst_xy: crate::intel::gpgpu::GpgpuPoint::new(
                glyph.destination_xy[0],
                glyph.destination_xy[1],
            ),
            color_rgba,
        })
        .collect::<Vec<_>>();
    let mut submits = 0usize;
    let mut active_walkers = 0usize;
    let mut release = None;
    for chunk in layers.chunks(crate::intel::gpgpu::GLYPH_MASK_BATCH_MAX_LAYERS) {
        let rendered =
            crate::intel::gpgpu::glyph_mask_layers_rgba8_2d_mode(chunk, destination, true);
        if !rendered.ok {
            if rendered.submitted {
                FONT_GPU_CACHE
                    .lock()
                    .quarantine_atlas_after_ambiguous_read(cached);
                return Err(FontKernelError::SubmittedIncomplete("font-gpu-tile-stamp-incomplete"));
            }
            return Err(if frame_already_irreversible || submits != 0 {
                FontKernelError::SubmittedIncomplete("font-gpu-tile-frame-partial")
            } else {
                FontKernelError::Unavailable("font-gpu-tile-stamp")
            });
        }
        submits = submits.saturating_add(rendered.submits);
        active_walkers = active_walkers.saturating_add(rendered.active_walkers);
        if rendered.release.is_some() {
            release = rendered.release;
        }
    }
    if release.is_none() {
        let finalized = crate::intel::gpgpu::font_release_rgba8_surface_for_scanout(destination);
        if !finalized.ok {
            if finalized.submitted || crate::intel::gpgpu::font_rcs_context_is_quarantined() {
                FONT_GPU_CACHE.lock().mark_poisoned();
            }
            return Err(if finalized.submitted || frame_already_irreversible || submits != 0 {
                FontKernelError::SubmittedIncomplete("font-gpu-tile-release-incomplete")
            } else {
                FontKernelError::Unavailable("font-gpu-tile-release")
            });
        }
        submits = submits.saturating_add(usize::from(finalized.submitted));
        release = finalized.release;
    }
    let release =
        release.ok_or(FontKernelError::SubmittedIncomplete("font-gpu-tile-release-missing"))?;
    Ok((submits, active_walkers, release))
}

fn materialize_cached_font_gpu_glyphs(
    cached: &[CachedPreparedGlyph],
    foreground: GpuFontRgba,
    destination: crate::intel::gpgpu::GpgpuRgba8Surface,
) -> Result<(usize, usize), FontKernelError> {
    let color_rgba = u32::from_le_bytes([foreground.r, foreground.g, foreground.b, foreground.a]);
    let layers = cached
        .iter()
        .map(|glyph| crate::intel::gpgpu::GpgpuGlyphMaskLayer {
            mask: glyph.tile.surface(),
            mask_rect: glyph.tile.rect(),
            dst_xy: crate::intel::gpgpu::GpgpuPoint::new(
                glyph.destination_xy[0],
                glyph.destination_xy[1],
            ),
            color_rgba,
        })
        .collect::<Vec<_>>();
    let mut submits = 0usize;
    let mut active_walkers = 0usize;
    for chunk in layers.chunks(crate::intel::gpgpu::GLYPH_MASK_BATCH_MAX_LAYERS) {
        // The atlas remains a PAT0 Font resource. It becomes a direct-scanout
        // source only indirectly, when the later cache-only worklist reads it
        // and writes the caller's PAT3 frame.
        let rendered =
            crate::intel::gpgpu::glyph_mask_layers_rgba8_2d_mode(chunk, destination, false);
        if !rendered.ok {
            if rendered.submitted {
                FONT_GPU_CACHE
                    .lock()
                    .quarantine_atlas_after_ambiguous_read(cached);
                return Err(FontKernelError::SubmittedIncomplete(
                    "font-rush-cache-charge-incomplete",
                ));
            }
            return Err(if submits != 0 {
                FontKernelError::SubmittedIncomplete("font-rush-cache-charge-partial")
            } else {
                FontKernelError::Unavailable("font-rush-cache-charge")
            });
        }
        submits = submits.saturating_add(rendered.submits);
        active_walkers = active_walkers.saturating_add(rendered.active_walkers);
    }
    Ok((submits, active_walkers))
}

fn font_rush_cache_plan_cells_are_contained(
    prepared: &[GpuFontPreparedCenteredGlyph],
    batch: u8,
) -> bool {
    if prepared.len() != FONT_RUSH_RGBA8_CACHE_GLYPHS_PER_BATCH
        || usize::from(batch) >= FONT_RUSH_RGBA8_CACHE_BATCHES_PER_CLASS
    {
        return false;
    }
    let first_entry = usize::from(batch) * FONT_RUSH_RGBA8_CACHE_GLYPHS_PER_BATCH;
    prepared.iter().enumerate().all(|(local, glyph)| {
        let entry = first_entry + local;
        let column = entry % FONT_RUSH_RGBA8_CACHE_COLUMNS as usize;
        let row = entry / FONT_RUSH_RGBA8_CACHE_COLUMNS as usize;
        let cell_left = (column as i64) * i64::from(FONT_RUSH_RGBA8_CACHE_TILE_PX);
        let cell_top = (row as i64) * i64::from(FONT_RUSH_RGBA8_CACHE_TILE_PX);
        let cell_right = cell_left + i64::from(FONT_RUSH_RGBA8_CACHE_TILE_PX);
        let cell_bottom = cell_top + i64::from(FONT_RUSH_RGBA8_CACHE_TILE_PX);
        let rect = glyph.destination_rect();
        let left = i64::from(rect.x);
        let top = i64::from(rect.y);
        let right = left + i64::from(rect.width);
        let bottom = top + i64::from(rect.height);
        !rect.is_empty()
            && left >= cell_left
            && top >= cell_top
            && right <= cell_right
            && bottom <= cell_bottom
    })
}

fn process_font_rush_cache_charge(
    ticket: FontKernelTicket,
    plan: PreparedGlyphPlan,
    cache: &FontRushRgba8Cache,
    class: u8,
    batch: u8,
    enqueued_ms: u64,
) -> Result<FontRushCacheCharge, FontKernelError> {
    let service_started_ms = Instant::now().as_millis();
    ensure_font_rcs_lane_available()?;
    if cache.is_sealed() || cache.batch_ready(class, batch) || !cache.batch_reserved(class, batch) {
        return Err(FontKernelError::InvalidRequest("font-rush-cache-charge-state"));
    }
    if !cache.terminal_path_is_warm() {
        if !crate::intel::gpgpu::prepare_font_sprite_quad_worklist_rgba8() {
            return Err(FontKernelError::Unavailable("font-rush-cache-terminal-warm"));
        }
        cache.mark_terminal_path_warm();
    }
    let destination = cache
        .atlas_surface(class)
        .ok_or(FontKernelError::InvalidRequest("font-rush-cache-class"))?;
    if plan.fit() != FontStampFit::Canvas
        || plan.glyph_count() != FONT_RUSH_RGBA8_CACHE_GLYPHS_PER_BATCH
        || (plan.raster_width(), plan.raster_height()) != (destination.width, destination.height)
    {
        return Err(FontKernelError::InvalidRequest("font-rush-cache-charge-contract"));
    }

    set_active_stage(ticket, "font-rush-cache-coverage");
    let glyphs = plan.glyph_count();
    let foreground = plan.foreground();
    let prepared = plan.into_prepared();
    if !font_rush_cache_plan_cells_are_contained(prepared.as_slice(), batch) {
        return Err(FontKernelError::InvalidRequest("font-rush-cache-cell-containment"));
    }
    let classification = classify_gpu_font_prepared_placements(prepared.as_slice());
    if classification.requires_union_coverage()
        || classification.unique_indices().len() != FONT_RUSH_RGBA8_CACHE_GLYPHS_PER_BATCH
    {
        return Err(FontKernelError::InvalidRequest("font-rush-cache-cell-overlap"));
    }
    let (cached, _) =
        prepare_cached_font_gpu_glyphs(prepared.as_slice(), classification.unique_indices())
            .map_err(|failure| failure.error)?;

    set_active_stage(ticket, "font-rush-cache-rgba8-materialize");
    let (submits, active_walkers) =
        materialize_cached_font_gpu_glyphs(cached.as_slice(), foreground, destination)?;
    cache.commit_batch(class, batch)?;
    let completed_ms = Instant::now().as_millis();
    crate::log_info!(
        target: "render";
        "font-kernel-service: font-rush cache charged ticket={} cache={} class={} batch={} glyphs={} atlas={}x{} tile={}x{} submits={} walkers={} queue_wait_ms={} service_ms={} storage=finished-premultiplied-rgba8 policy=run-owned+fixed+no-eviction ppgtt=font-pat0\n",
        ticket.raw(),
        cache.id(),
        class,
        batch,
        glyphs,
        destination.width,
        destination.height,
        FONT_RUSH_RGBA8_CACHE_TILE_PX,
        FONT_RUSH_RGBA8_CACHE_TILE_PX,
        submits,
        active_walkers,
        service_started_ms.saturating_sub(enqueued_ms),
        completed_ms.saturating_sub(service_started_ms),
    );
    Ok(FontRushCacheCharge {
        ticket,
        cache_id: cache.id(),
        class,
        batch,
        glyphs,
        submits,
        active_walkers,
        total_service_ms: completed_ms.saturating_sub(service_started_ms),
    })
}

enum FontFrameCoverage {
    Retained(Vec<(GpuFontRetainedScene, GpuFontRgba)>),
    Cached {
        glyphs: Vec<CachedPreparedGlyph>,
        foreground: GpuFontRgba,
    },
}

fn font_rush_cache_mix(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

fn font_rush_cache_bounded_offset(seed: u64, extent: u32) -> u32 {
    if extent == 0 {
        return 0;
    }
    ((u128::from(seed) * u128::from(extent)) >> 64) as u32
}

fn font_rush_cache_sprite_descriptor(
    destination: crate::intel::gpgpu::GpgpuRgba8Surface,
    class: usize,
    worker: usize,
    anchor: usize,
    wave: u64,
) -> crate::intel::gpgpu::GpgpuSpriteQuadWorklistDesc {
    let placement_seed = font_rush_cache_mix(
        wave.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ ((worker as u64) << 17) ^ class as u64,
    );
    let source_seed = font_rush_cache_mix(
        placement_seed
            ^ (anchor as u64)
                .wrapping_add(1)
                .wrapping_mul(0xD6E8_FEB8_6659_FD93),
    );
    let entry = (source_seed as usize) % FONT_RUSH_RGBA8_CACHE_GLYPHS_PER_CLASS;
    let source_x =
        (entry % FONT_RUSH_RGBA8_CACHE_COLUMNS as usize) as u32 * FONT_RUSH_RGBA8_CACHE_TILE_PX;
    let source_y =
        (entry / FONT_RUSH_RGBA8_CACHE_COLUMNS as usize) as u32 * FONT_RUSH_RGBA8_CACHE_TILE_PX;
    let source_width = crate::intel::gpgpu::GPGPU_FONT_RUSH_RGBA8_ATLAS_WIDTH as f32;
    let source_height = crate::intel::gpgpu::GPGPU_FONT_RUSH_RGBA8_ATLAS_HEIGHT as f32;
    let u0 = (source_x as f32 - 0.5) / source_width;
    let v0 = (source_y as f32 - 0.5) / source_height;
    let u1 = (source_x as f32 + FONT_RUSH_RGBA8_CACHE_TILE_PX as f32 - 0.5) / source_width;
    let v1 = (source_y as f32 + FONT_RUSH_RGBA8_CACHE_TILE_PX as f32 - 0.5) / source_height;

    let column = worker % 8;
    let row = worker / 8;
    let region_x0 = (u64::from(destination.width) * column as u64 / 8) as i32;
    let region_x1 = (u64::from(destination.width) * (column + 1) as u64 / 8) as i32;
    let region_y0 = (u64::from(destination.height) * row as u64 / 4) as i32;
    let region_y1 = (u64::from(destination.height) * (row + 1) as u64 / 4) as i32;
    let region_width = region_x1.saturating_sub(region_x0) as u32;
    let region_height = region_y1.saturating_sub(region_y0) as u32;
    // A worker owns one 8x4 region, but its cached glyphs may spill across the
    // region edge on this intentionally exotic single-plane stage.  Sample
    // the complete region instead of jittering around its center: small glyph
    // faces then explore the same full placement area as large faces. Anchor
    // one is a half-region torus shift from anchor zero, guaranteeing useful
    // separation without excluding any position over later waves.
    let base_x = font_rush_cache_bounded_offset(
        font_rush_cache_mix(placement_seed ^ 0xA076_1D64_78BD_642F),
        region_width,
    );
    let base_y = font_rush_cache_bounded_offset(
        font_rush_cache_mix(placement_seed ^ 0xE703_7ED1_A0B4_28DB),
        region_height,
    );
    let offset_x = if anchor == 0 {
        base_x
    } else {
        (base_x + region_width.div_ceil(2)) % region_width.max(1)
    };
    let offset_y = if anchor == 0 {
        base_y
    } else {
        (base_y + region_height.div_ceil(2)) % region_height.max(1)
    };
    let half = (FONT_RUSH_RGBA8_CACHE_TILE_PX / 2) as i32;
    let left = region_x0
        .saturating_add(offset_x as i32)
        .saturating_sub(half) as f32;
    let top = region_y0
        .saturating_add(offset_y as i32)
        .saturating_sub(half) as f32;
    let right = left + FONT_RUSH_RGBA8_CACHE_TILE_PX as f32;
    let bottom = top + FONT_RUSH_RGBA8_CACHE_TILE_PX as f32;
    crate::intel::gpgpu::GpgpuSpriteQuadWorklistDesc {
        c0_x: left,
        c0_y: top,
        c0_u: u0,
        c0_v: v0,
        c1_x: right,
        c1_y: top,
        c1_u: u1,
        c1_v: v0,
        c2_x: right,
        c2_y: bottom,
        c2_u: u1,
        c2_v: v1,
        c3_x: left,
        c3_y: bottom,
        c3_u: u0,
        c3_v: v1,
        color_rgba: u32::MAX,
        flags: crate::intel::gpgpu::SPRITE_QUAD_WORKLIST_FLAG_SRC_OVER
            | crate::intel::gpgpu::SPRITE_QUAD_WORKLIST_FLAG_PREMUL_SRC,
    }
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
        gpu_outline_cache_hits: 0,
        gpu_outline_cache_misses: 0,
        gpu_tile_cache_hits: 0,
        gpu_tile_cache_misses: 0,
        gpu_cache_evictions: 0,
        gpu_resident_outlines: 0,
        gpu_resident_tiles: 0,
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
        gpu_outline_cache_hits: 0,
        gpu_outline_cache_misses: 0,
        gpu_tile_cache_hits: 0,
        gpu_tile_cache_misses: 0,
        gpu_cache_evictions: 0,
        gpu_resident_outlines: 0,
        gpu_resident_tiles: 0,
        instance_release_ms: completed_ms.saturating_sub(sprite_started_ms),
        total_service_ms: completed_ms.saturating_sub(service_started_ms),
        release,
    })
}

fn process_font_rush_rgba8_cache_blast(
    ticket: FontKernelTicket,
    cache: Arc<FontRushRgba8Cache>,
    wave: u64,
    destination: crate::intel::gpgpu::GpgpuRgba8Surface,
    enqueued_ms: u64,
) -> Result<FontFrameStamp, FontKernelError> {
    let service_started_ms = Instant::now().as_millis();
    ensure_font_rcs_lane_available()?;
    if !cache.is_sealed() || !destination.is_valid() {
        return Err(FontKernelError::InvalidRequest("font-rush-cache-blast-contract"));
    }
    set_active_stage(ticket, "font-rush-cache-blit-only");

    let mut descriptors: [Vec<crate::intel::gpgpu::GpgpuSpriteQuadWorklistDesc>;
        FONT_RUSH_RGBA8_CACHE_CLASSES] = core::array::from_fn(|_| Vec::with_capacity(16));
    for worker in 0..crate::r::font_plan_service::FONT_PLAN_WORKER_COUNT {
        let class = worker / 8;
        for anchor in 0..2 {
            descriptors[class].push(font_rush_cache_sprite_descriptor(
                destination,
                class,
                worker,
                anchor,
                wave,
            ));
        }
    }
    if descriptors.iter().any(|run| run.len() != 16)
        || descriptors.iter().map(Vec::len).sum::<usize>() != FONT_RUSH_RGBA8_BLAST_GLYPHS
    {
        return Err(FontKernelError::InvalidRequest("font-rush-cache-blit-layout"));
    }
    let sources = [
        cache.atlas_surface(0),
        cache.atlas_surface(1),
        cache.atlas_surface(2),
        cache.atlas_surface(3),
    ];
    let [Some(source0), Some(source1), Some(source2), Some(source3)] = sources else {
        return Err(FontKernelError::InvalidRequest("font-rush-cache-source"));
    };
    let runs = [
        crate::intel::gpgpu::GpgpuSpriteQuadWorklistRun {
            src: source0,
            descs: descriptors[0].as_slice(),
        },
        crate::intel::gpgpu::GpgpuSpriteQuadWorklistRun {
            src: source1,
            descs: descriptors[1].as_slice(),
        },
        crate::intel::gpgpu::GpgpuSpriteQuadWorklistRun {
            src: source2,
            descs: descriptors[2].as_slice(),
        },
        crate::intel::gpgpu::GpgpuSpriteQuadWorklistRun {
            src: source3,
            descs: descriptors[3].as_slice(),
        },
    ];
    let rendered = crate::intel::gpgpu::font_sprite_quad_worklist_rgba8_runs_over_result(
        destination,
        runs.as_slice(),
    );
    match rendered.outcome {
        crate::intel::gpgpu::GpgpuSubmissionOutcome::SubmittedIncomplete => {
            return Err(FontKernelError::SubmittedIncomplete("font-rush-cache-blit-incomplete"));
        }
        crate::intel::gpgpu::GpgpuSubmissionOutcome::Unavailable => {
            return Err(FontKernelError::Unavailable("font-rush-cache-blit"));
        }
        crate::intel::gpgpu::GpgpuSubmissionOutcome::Complete => {}
    }
    let release = rendered
        .release
        .ok_or(FontKernelError::SubmittedIncomplete("font-rush-cache-blit-release-missing"))?;
    if rendered.stats.descs != FONT_RUSH_RGBA8_BLAST_GLYPHS {
        return Err(FontKernelError::InvalidRequest("font-rush-cache-blit-count-mismatch"));
    }
    let completed_ms = Instant::now().as_millis();
    crate::log_info!(
        target: "render";
        "font-kernel-service: font-rush blast complete ticket={} cache={} wave={} glyphs={} size_classes={} submits={} walkers={} pre_service_ms={} service_ms={} cache_blit_only=1 pixel_op=ordered-premultiplied-source-over clear=0 planning=0 skrifa=0 tessellation=0 coverage=0 shading=0 mutation=0 eviction=0 fallback=0\n",
        ticket.raw(),
        cache.id(),
        wave,
        rendered.stats.descs,
        FONT_RUSH_RGBA8_CACHE_CLASSES,
        rendered.stats.submits,
        rendered.stats.walkers,
        service_started_ms.saturating_sub(enqueued_ms),
        completed_ms.saturating_sub(service_started_ms),
    );
    Ok(FontFrameStamp {
        ticket,
        glyphs: rendered.stats.descs,
        submits: rendered.stats.submits,
        clear_submits: 0,
        active_walkers: rendered.stats.walkers,
        pre_service_ms: service_started_ms.saturating_sub(enqueued_ms),
        clear_ms: 0,
        prepare_coverage_ms: 0,
        coverage_build_ms: 0,
        coverage_audit_ms: 0,
        coverage_submits: 0,
        gpu_outline_cache_hits: 0,
        gpu_outline_cache_misses: 0,
        gpu_tile_cache_hits: 0,
        gpu_tile_cache_misses: 0,
        gpu_cache_evictions: 0,
        gpu_resident_outlines: 0,
        gpu_resident_tiles: 0,
        instance_release_ms: completed_ms.saturating_sub(service_started_ms),
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
        FrameStampInput::FontRushRgba8Blast { cache, wave, .. } => {
            return process_font_rush_rgba8_cache_blast(
                ticket,
                cache,
                wave,
                destination,
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
    let (coverage, glyphs, coverage_input, cache_stats) = match input {
        FrameStampInput::Prepared(plan) => {
            let glyphs = plan.glyph_count();
            let foreground = plan.foreground();
            let prepared = plan.into_prepared();
            let classification = classify_gpu_font_prepared_placements(prepared.as_slice());
            if classification.requires_union_coverage() {
                let scene = retain_gpu_font_prepared_centered_scene(prepared)
                    .map_err(FontKernelError::Unavailable)?;
                (
                    FontFrameCoverage::Retained(alloc::vec![(scene, foreground)]),
                    glyphs,
                    "prepared-plan-overlap-union",
                    FontGpuFrameCacheStats::default(),
                )
            } else {
                match prepare_cached_font_gpu_glyphs(
                    prepared.as_slice(),
                    classification.unique_indices(),
                ) {
                    Ok((cached, cache_stats)) => (
                        FontFrameCoverage::Cached {
                            glyphs: cached,
                            foreground,
                        },
                        glyphs,
                        "prepared-plan-resident-cache",
                        cache_stats,
                    ),
                    Err(failure) => {
                        if matches!(failure.error, FontKernelError::SubmittedIncomplete(_)) {
                            return Err(failure.error);
                        }
                        let error = failure.error;
                        crate::log_info!(
                            target: "render";
                            "font-kernel-service: resident-cache fallback ticket={} glyphs={} reason={:?} action=prepared-union-before-clear\n",
                            ticket.raw(),
                            glyphs,
                            error,
                        );
                        let scene = retain_gpu_font_prepared_centered_scene(prepared)
                            .map_err(FontKernelError::Unavailable)?;
                        (
                            FontFrameCoverage::Retained(alloc::vec![(scene, foreground)]),
                            glyphs,
                            "prepared-plan-cache-fallback",
                            failure.stats,
                        )
                    }
                }
            }
        }
        FrameStampInput::Request(request) => {
            let (scenes, glyphs) = prepare_stamp_scenes(ticket, &request)?;
            (
                FontFrameCoverage::Retained(scenes),
                glyphs,
                "request-outline",
                FontGpuFrameCacheStats::default(),
            )
        }
        FrameStampInput::FontRushClear { .. }
        | FrameStampInput::FontRushRgba8Sprites { .. }
        | FrameStampInput::FontRushRgba8Blast { .. } => {
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
                .fold(cache_stats.coverage_build_ms, |total, (scene, _)| {
                    total.saturating_add(scene.coverage_build_ms())
                }),
            scenes
                .iter()
                .fold(cache_stats.coverage_audit_ms, |total, (scene, _)| {
                    total.saturating_add(scene.coverage_audit_ms())
                }),
            scenes
                .iter()
                .fold(cache_stats.coverage_submits, |total, (scene, _)| {
                    total.saturating_add(scene.coverage_submits())
                }),
            scenes.len(),
        ),
        FontFrameCoverage::Cached { glyphs, .. } => (
            cache_stats.coverage_build_ms,
            cache_stats.coverage_audit_ms,
            cache_stats.coverage_submits,
            glyphs.len(),
        ),
    };
    crate::log_info!(
        target: "render";
        "font-kernel-service: frame coverage ticket={} input={} glyphs={} scenes={} prepare_coverage_ms={} coverage_build_ms={} coverage_audit_ms={} coverage_submits={} outline_cache_hits={} outline_cache_misses={} tile_cache_hits={} tile_cache_misses={} cache_evictions={} resident_outlines={} resident_tiles={} cache=shared-font-vm-bounded\n",
        ticket.raw(),
        coverage_input,
        glyphs,
        scene_count,
        prepare_coverage_ms,
        coverage_build_ms,
        coverage_audit_ms,
        coverage_submits,
        cache_stats.outline_hits,
        cache_stats.outline_misses,
        cache_stats.tile_hits,
        cache_stats.tile_misses,
        cache_stats.evictions,
        cache_stats.resident_outlines,
        cache_stats.resident_tiles,
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
        FontFrameCoverage::Cached {
            glyphs: cached,
            foreground,
        } => {
            set_active_stage(ticket, "frame-instance-cached");
            let rendered = stamp_cached_font_gpu_glyphs(
                cached.as_slice(),
                foreground,
                destination,
                clear_rgba.is_some(),
            )?;
            submits = rendered.0;
            active_walkers = rendered.1;
            release = Some(rendered.2);
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
        gpu_outline_cache_hits: cache_stats.outline_hits,
        gpu_outline_cache_misses: cache_stats.outline_misses,
        gpu_tile_cache_hits: cache_stats.tile_hits,
        gpu_tile_cache_misses: cache_stats.tile_misses,
        gpu_cache_evictions: cache_stats.evictions,
        gpu_resident_outlines: cache_stats.resident_outlines,
        gpu_resident_tiles: cache_stats.resident_tiles,
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
            enqueued_ms,
            reply,
        } => {
            set_active_stage(ticket, "dispatch");
            let result = process_frame_stamp(ticket, input, destination, clear_rgba, enqueued_ms);
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
        QueuedFontRequest::FontRushCacheCharge {
            ticket,
            plan,
            cache,
            class,
            batch,
            enqueued_ms,
            reply,
        } => {
            set_active_stage(ticket, "dispatch");
            let result = process_font_rush_cache_charge(
                ticket,
                plan,
                cache.as_ref(),
                class,
                batch,
                enqueued_ms,
            );
            if let Err(error) = &result {
                cache.fail_batch(class, batch, *error);
                log_failure(ticket, "font-rush-cache-charge", error);
            }
            complete_status(ticket, false, result.is_ok());
            crate::log_info!(
                target: "render";
                "font-kernel-service: font-rush cache charge complete ticket={} cache={} class={} batch={} ok={} queued={}\n",
                ticket.raw(),
                cache.id(),
                class,
                batch,
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
        "font-kernel-service: online paths=retain-scene+async-stamp+async-frame-stamp+prepared-frame-stamp+font-rush-clear-only+font-rush-showcase-rgba8-sprite+font-rush-rgba8-charge+sealed-rgba8-cache-blast controller=bsp worker=leased-blocking-service-lane font_lane=fair-fifo-font-only gpu_context=kernel-gpgpu-font queue_capacity={} retained_storage=gpu-vm-r8 prepared_storage=bounded-transient-move-once rush_cache=run-owned-4x8MiB-pat0-final-rgba8 rush_terminal=prewarm-descriptor+kernel+ppgtt/ordered-source-over-copy-only stamp_output=owned-or-ui4-leased-gpu-vm-rgba8 completion=signal\n",
        FONT_KERNEL_QUEUE_CAPACITY,
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
    fn font_rush_cached_anchors_cover_complete_integer_worker_regions() {
        let width = 2_559u32;
        let height = 1_439u32;
        let pitch = width * 4;
        let bytes = pitch as usize * height as usize;
        let destination = crate::intel::gpgpu::GpgpuRgba8Surface::new(
            0x2000_0000,
            0x1200_0000,
            bytes,
            width,
            height,
            pitch,
        )
        .unwrap();

        assert_eq!(font_rush_cache_bounded_offset(0, 257), 0);
        assert_eq!(font_rush_cache_bounded_offset(u64::MAX, 257), 256);
        for worker in 0..crate::r::font_plan_service::FONT_PLAN_WORKER_COUNT {
            let column = worker % 8;
            let row = worker / 8;
            let x0 = (u64::from(width) * column as u64 / 8) as i32;
            let x1 = (u64::from(width) * (column + 1) as u64 / 8) as i32;
            let y0 = (u64::from(height) * row as u64 / 4) as i32;
            let y1 = (u64::from(height) * (row + 1) as u64 / 4) as i32;
            let region_width = x1 - x0;
            let region_height = y1 - y0;
            let class = worker / 8;
            let first = font_rush_cache_sprite_descriptor(destination, class, worker, 0, 37);
            let second = font_rush_cache_sprite_descriptor(destination, class, worker, 1, 37);
            for descriptor in [first, second] {
                assert_eq!(descriptor.c0_x.fract(), 0.0);
                assert_eq!(descriptor.c0_y.fract(), 0.0);
                assert_eq!(descriptor.c1_x - descriptor.c0_x, 128.0);
                assert_eq!(descriptor.c3_y - descriptor.c0_y, 128.0);
                let center_x = descriptor.c0_x as i32 + 64;
                let center_y = descriptor.c0_y as i32 + 64;
                assert!((x0..x1).contains(&center_x));
                assert!((y0..y1).contains(&center_y));

                let source_x = descriptor.c0_u * 2_048.0 + 0.5;
                let source_y = descriptor.c0_v * 1_024.0 + 0.5;
                assert_eq!(source_x.fract(), 0.0);
                assert_eq!(source_y.fract(), 0.0);
            }
            let first_x = first.c0_x as i32 + 64 - x0;
            let second_x = second.c0_x as i32 + 64 - x0;
            let first_y = first.c0_y as i32 + 64 - y0;
            let second_y = second.c0_y as i32 + 64 - y0;
            assert_eq!((second_x - first_x).rem_euclid(region_width), (region_width + 1) / 2,);
            assert_eq!((second_y - first_y).rem_euclid(region_height), (region_height + 1) / 2,);
        }
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
