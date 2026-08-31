//! Bounded retained-R8 fast path for semi-persistent font producers.
//!
//! OceanCache is deliberately generation-local and colorless.  Its key owns
//! every layout input that changes analytical glyph coverage, while RGBA stays
//! a cheap restamp parameter in FontKernel. A full ocean falls back to
//! ordinary coverage production; its final registration claim dropping
//! retires it naturally without manufacturing a producer credit.

extern crate alloc;

use alloc::{
    string::String,
    sync::{Arc, Weak},
    vec::Vec,
};
use spin::Mutex;

use crate::intel::gpu_font::{GpuFontFace, GpuFontRetainedScene};

use super::{
    font_kernel_service::{RetainSceneRequest, RetainedFontPositioning},
    font_producer_service::FontProducerRegistration,
};

/// Complete per-producer-generation budget, including the GPU R8 masks and
/// their CPU key/stamp metadata.
pub(crate) const OCEAN_CACHE_BYTES: usize = 512 * 1024;
pub(crate) const OCEAN_CACHE_THEORETICAL_32_BYTES: usize = OCEAN_CACHE_BYTES * 32;
const OCEAN_CACHE_RASTER_POLICY_VERSION: u16 = 1;

/// Registration-time identity shared by producers with the same immutable
/// font coverage contract. Row geometry, output color, producer generation,
/// and ACK ownership do not affect a colorless glyph mask and stay outside
/// this seal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OceanCacheSeal {
    face: u16,
    font_pixels_milli: u32,
    raster_policy_version: u16,
}

impl OceanCacheSeal {
    pub(crate) const fn from_registration(registration: FontProducerRegistration) -> Self {
        Self {
            face: registration.face,
            font_pixels_milli: registration.font_pixels_milli,
            raster_policy_version: OCEAN_CACHE_RASTER_POLICY_VERSION,
        }
    }
}

struct OceanCacheDomain {
    seal: OceanCacheSeal,
    ocean: Weak<Mutex<OceanCache>>,
}

static OCEAN_CACHE_DOMAINS: Mutex<Vec<OceanCacheDomain>> = Mutex::new(Vec::new());

/// One producer's claim on the shared OceanCache selected by registration.
/// The final claim dropping destroys the cache naturally; the bank keeps only
/// weak references and therefore cannot extend GPU-mask lifetime.
pub(crate) struct OceanCacheClaim {
    ocean: Arc<Mutex<OceanCache>>,
}

impl OceanCacheClaim {
    pub(crate) fn claim(registration: FontProducerRegistration) -> Self {
        let seal = OceanCacheSeal::from_registration(registration);
        let mut domains = OCEAN_CACHE_DOMAINS.lock();
        domains.retain(|domain| domain.ocean.strong_count() != 0);
        if let Some(ocean) = domains
            .iter()
            .find(|domain| domain.seal == seal)
            .and_then(|domain| domain.ocean.upgrade())
        {
            return Self { ocean };
        }
        let ocean = Arc::new(Mutex::new(OceanCache::new()));
        domains.push(OceanCacheDomain {
            seal,
            ocean: Arc::downgrade(&ocean),
        });
        Self { ocean }
    }

    pub(crate) fn claim_count(&self) -> usize {
        Arc::strong_count(&self.ocean)
    }

    pub(crate) fn get(&self, key: &OceanCacheKey) -> Option<Arc<GpuFontRetainedScene>> {
        self.ocean.lock().get(key)
    }

    pub(crate) fn insert(&self, key: OceanCacheKey, scene: Arc<GpuFontRetainedScene>) {
        self.ocean.lock().insert(key, scene);
    }

    pub(crate) fn diagnostics(&self) -> (usize, usize, u64, u64, u64) {
        self.ocean.lock().diagnostics()
    }

    #[cfg(test)]
    fn shares_domain_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.ocean, &other.ocean)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OceanCacheKey {
    font: GpuFontFace,
    viewport_width: u32,
    viewport_height: u32,
    raster_width: u32,
    raster_height: u32,
    positioning: RetainedFontPositioning,
    runs: Vec<OceanCacheRunKey>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct OceanCacheRunKey {
    text: String,
    position_bits: [u32; 2],
    font_pixels_bits: u32,
    slant_bits: u32,
}

impl OceanCacheKey {
    pub(crate) fn from_scene(scene: &RetainSceneRequest) -> Self {
        Self {
            font: scene.font,
            viewport_width: scene.viewport_width,
            viewport_height: scene.viewport_height,
            raster_width: scene.raster_width,
            raster_height: scene.raster_height,
            positioning: scene.positioning,
            runs: scene
                .runs
                .iter()
                .map(|run| OceanCacheRunKey {
                    text: run.text.clone(),
                    position_bits: run.position.map(f32::to_bits),
                    font_pixels_bits: run.font_pixels.to_bits(),
                    slant_bits: run.slant.to_bits(),
                })
                .collect(),
        }
    }

    fn retained_bytes(&self) -> usize {
        core::mem::size_of::<Self>()
            .saturating_add(
                self.runs
                    .capacity()
                    .saturating_mul(core::mem::size_of::<OceanCacheRunKey>()),
            )
            .saturating_add(
                self.runs
                    .iter()
                    .fold(0usize, |bytes, run| bytes.saturating_add(run.text.capacity())),
            )
    }
}

struct OceanCacheEntry {
    key: OceanCacheKey,
    scene: Arc<GpuFontRetainedScene>,
    accounted_bytes: usize,
}

/// First-fill/no-eviction ocean owned by exactly one producer generation.
pub(crate) struct OceanCache {
    entries: Vec<OceanCacheEntry>,
    used_bytes: usize,
    hits: u64,
    misses: u64,
    uncached: u64,
}

impl OceanCache {
    pub(crate) const fn new() -> Self {
        Self {
            entries: Vec::new(),
            used_bytes: 0,
            hits: 0,
            misses: 0,
            uncached: 0,
        }
    }

    pub(crate) fn get(&mut self, key: &OceanCacheKey) -> Option<Arc<GpuFontRetainedScene>> {
        let entry = self.entries.iter().find(|entry| &entry.key == key)?;
        self.hits = self.hits.saturating_add(1);
        Some(Arc::clone(&entry.scene))
    }

    pub(crate) fn insert(&mut self, key: OceanCacheKey, scene: Arc<GpuFontRetainedScene>) {
        self.misses = self.misses.saturating_add(1);
        let bytes = key
            .retained_bytes()
            .saturating_add(scene.identity_cache_bytes())
            .saturating_add(core::mem::size_of::<OceanCacheEntry>());
        if bytes == 0 || bytes > OCEAN_CACHE_BYTES.saturating_sub(self.used_bytes) {
            self.uncached = self.uncached.saturating_add(1);
            return;
        }
        self.used_bytes = self.used_bytes.saturating_add(bytes);
        self.entries.push(OceanCacheEntry {
            key,
            scene,
            accounted_bytes: bytes,
        });
    }

    pub(crate) fn diagnostics(&self) -> (usize, usize, u64, u64, u64) {
        debug_assert_eq!(
            self.used_bytes,
            self.entries
                .iter()
                .fold(0usize, |total, entry| total.saturating_add(entry.accounted_bytes))
        );
        (self.entries.len(), self.used_bytes, self.hits, self.misses, self.uncached)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::r::services::font_kernel_service::RetainedFontRun;

    fn request() -> RetainSceneRequest {
        RetainSceneRequest {
            runs: alloc::vec![RetainedFontRun {
                text: String::from("Ocean"),
                position: [4.0, 24.0],
                font_pixels: 24.0,
                slant: 0.0,
            }],
            font: GpuFontFace::Inconsolata,
            viewport_width: 320,
            viewport_height: 64,
            raster_width: 320,
            raster_height: 64,
            positioning: RetainedFontPositioning::SceneOrigin,
        }
    }

    #[test]
    fn ocean_key_is_exact_and_colorless_by_construction() {
        let scene = request();
        let key = OceanCacheKey::from_scene(&scene);
        assert_eq!(key, OceanCacheKey::from_scene(&scene));

        let mut moved = scene.clone();
        moved.runs[0].position[0] += 1.0;
        assert_ne!(key, OceanCacheKey::from_scene(&moved));
    }

    #[test]
    fn equal_registration_seals_claim_the_same_ocean() {
        let first = OceanCacheClaim::claim(registration(1, 36_001, 1));
        let second = OceanCacheClaim::claim(registration(1, 36_001, 5));
        assert!(first.shares_domain_with(&second));
        assert_eq!(first.claim_count(), 2);
    }

    #[test]
    fn face_or_native_size_selects_another_ocean() {
        let first = OceanCacheClaim::claim(registration(1, 24_003, 1));
        let other_size = OceanCacheClaim::claim(registration(1, 36_003, 1));
        let other_face = OceanCacheClaim::claim(registration(3, 24_003, 1));
        assert!(!first.shares_domain_with(&other_size));
        assert!(!first.shares_domain_with(&other_face));
    }

    #[test]
    fn ocean_budget_remains_bounded_per_shared_seal() {
        let cache = OceanCache::new();
        assert_eq!(OCEAN_CACHE_THEORETICAL_32_BYTES, 16 * 1024 * 1024);
        assert_eq!(cache.diagnostics(), (0, 0, 0, 0, 0));
    }

    fn registration(face: u16, font_pixels_milli: u32, tier: u16) -> FontProducerRegistration {
        FontProducerRegistration {
            face,
            tier,
            font_pixels_milli,
            row_width_px: 320,
            row_height_px: 64,
            format: super::super::font_producer_service::FontProducerFormat::Rgba8Premultiplied,
            max_chars: 80,
            row_ring_depth: 2,
        }
    }
}
