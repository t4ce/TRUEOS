pub mod athlasmetrics;

use spin::Mutex;

use self::athlasmetrics::{
    ATHLAS_BUCKET_COUNT, ATHLAS_VARIANT_JSONS, AthlasGlyphRegion, AthlasVariantJson,
};

const ATHLAS_VARIANT_COUNT: usize = ATHLAS_VARIANT_JSONS.len();

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AthlasBucketTexture {
    pub tex_id: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AthlasResolvedGlyph {
    pub variant: &'static AthlasVariantJson,
    pub region: AthlasGlyphRegion,
    pub texture: Option<AthlasBucketTexture>,
    pub ready: bool,
    pub ready_seq: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct AthlasTierRuntime {
    ready: bool,
    ready_seq: u32,
    buckets: [AthlasBucketTexture; ATHLAS_BUCKET_COUNT],
}

static ATHLAS_RUNTIME: Mutex<[AthlasTierRuntime; ATHLAS_VARIANT_COUNT]> = Mutex::new(
    [AthlasTierRuntime {
        ready: false,
        ready_seq: 0,
        buckets: [AthlasBucketTexture {
            tex_id: 0,
            width: 0,
            height: 0,
        }; ATHLAS_BUCKET_COUNT],
    }; ATHLAS_VARIANT_COUNT],
);

#[inline]
pub fn athlas_variant(size_case: usize) -> Option<&'static AthlasVariantJson> {
    ATHLAS_VARIANT_JSONS.get(size_case)
}

pub fn athlas_reset_tier_state(size_case: usize) -> bool {
    let mut runtime = ATHLAS_RUNTIME.lock();
    let Some(tier) = runtime.get_mut(size_case) else {
        return false;
    };
    *tier = AthlasTierRuntime::default();
    true
}

pub fn athlas_register_bucket_texture(
    size_case: usize,
    bucket: usize,
    tex_id: u32,
    width: u32,
    height: u32,
) -> bool {
    let mut runtime = ATHLAS_RUNTIME.lock();
    let Some(tier) = runtime.get_mut(size_case) else {
        return false;
    };
    let Some(slot) = tier.buckets.get_mut(bucket) else {
        return false;
    };
    *slot = AthlasBucketTexture {
        tex_id,
        width,
        height,
    };
    true
}

pub fn athlas_mark_tier_ready(size_case: usize) -> Option<u32> {
    let mut runtime = ATHLAS_RUNTIME.lock();
    let tier = runtime.get_mut(size_case)?;
    if tier.buckets.iter().any(|bucket| bucket.tex_id == 0) {
        return None;
    }
    tier.ready = true;
    tier.ready_seq = tier.ready_seq.wrapping_add(1).max(1);
    Some(tier.ready_seq)
}

#[inline]
pub fn athlas_tier_ready(size_case: usize) -> bool {
    let runtime = ATHLAS_RUNTIME.lock();
    runtime
        .get(size_case)
        .map(|tier| tier.ready)
        .unwrap_or(false)
}

#[inline]
pub fn athlas_tier_ready_seq(size_case: usize) -> u32 {
    let runtime = ATHLAS_RUNTIME.lock();
    runtime
        .get(size_case)
        .map(|tier| tier.ready_seq)
        .unwrap_or(0)
}

#[inline]
pub fn athlas_bucket_texture(size_case: usize, bucket: usize) -> Option<AthlasBucketTexture> {
    let runtime = ATHLAS_RUNTIME.lock();
    runtime
        .get(size_case)
        .and_then(|tier| tier.buckets.get(bucket).copied())
        .filter(|bucket_tex| bucket_tex.tex_id != 0)
}

pub fn athlas_resolve_glyph(size_case: usize, ch: char) -> Option<AthlasResolvedGlyph> {
    let variant = athlas_variant(size_case)?;
    let region = athlasmetrics::athlas_lookup_glyph_region(size_case, ch)?;
    let runtime = ATHLAS_RUNTIME.lock();
    let tier = runtime.get(size_case)?;
    let texture = tier
        .buckets
        .get(region.bucket as usize)
        .copied()
        .filter(|bucket_tex| bucket_tex.tex_id != 0);
    Some(AthlasResolvedGlyph {
        variant,
        region,
        texture,
        ready: tier.ready,
        ready_seq: tier.ready_seq,
    })
}
