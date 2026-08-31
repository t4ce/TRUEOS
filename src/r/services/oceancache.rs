//! Bounded retained-R8 fast path for semi-persistent font producers.
//!
//! OceanCache is deliberately registration-sealed and colorless. Its entries
//! are position-independent glyph masks, while placement and RGBA remain cheap
//! restamp parameters in FontKernel. A full ocean falls back to ordinary
//! coverage production; its final registration claim dropping retires it
//! naturally without manufacturing a producer credit.

extern crate alloc;

use alloc::{
    sync::{Arc, Weak},
    vec::Vec,
};
use spin::Mutex;

use crate::intel::gpu_font::{GpuFontGlyphRecipeKey, GpuFontRetainedScene};

use super::font_producer_service::FontProducerRegistration;

/// Complete per-registration-seal budget, including the GPU R8 masks and their
/// CPU key/stamp metadata.
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

    pub(crate) fn get(&self, key: &GpuFontGlyphRecipeKey) -> Option<Arc<GpuFontRetainedScene>> {
        self.ocean.lock().get(key)
    }

    pub(crate) fn insert(&self, key: GpuFontGlyphRecipeKey, scene: Arc<GpuFontRetainedScene>) {
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

struct OceanCacheEntry {
    key: GpuFontGlyphRecipeKey,
    scene: Arc<GpuFontRetainedScene>,
    accounted_bytes: usize,
}

/// First-fill/no-eviction ocean shared by one registration seal.
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

    pub(crate) fn get(&mut self, key: &GpuFontGlyphRecipeKey) -> Option<Arc<GpuFontRetainedScene>> {
        let index = self.entries.iter().position(|entry| &entry.key == key)?;
        if self.entries[index].scene.quarantined() {
            let retired = self.entries.remove(index);
            self.used_bytes = self.used_bytes.saturating_sub(retired.accounted_bytes);
            return None;
        }
        self.hits = self.hits.saturating_add(1);
        Some(Arc::clone(&self.entries[index].scene))
    }

    pub(crate) fn insert(&mut self, key: GpuFontGlyphRecipeKey, scene: Arc<GpuFontRetainedScene>) {
        self.misses = self.misses.saturating_add(1);
        let bytes = core::mem::size_of::<GpuFontGlyphRecipeKey>()
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
