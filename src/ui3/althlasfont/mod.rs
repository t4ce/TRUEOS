pub mod athlasmetrics;
pub mod bitmapfont;
pub mod twemoji;

use self::athlasmetrics::{ATHLAS_VARIANT_JSONS, AthlasGlyphRegion};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AthlasBucketTexture {
    pub tex_id: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AthlasResolvedGlyph {
    pub region: AthlasGlyphRegion,
    pub texture: Option<AthlasBucketTexture>,
    pub ready: bool,
    pub ready_seq: u32,
}

pub fn athlas_reset_tier_state(_size_case: usize) -> bool {
    false
}

pub fn athlas_register_bucket_texture(
    _size_case: usize,
    _bucket: usize,
    _tex_id: u32,
    _width: u32,
    _height: u32,
) -> bool {
    false
}

pub fn athlas_mark_tier_ready(_size_case: usize) -> Option<u32> {
    None
}

#[inline]
pub fn athlas_tier_ready_seq(_size_case: usize) -> u32 {
    0
}

pub fn athlas_resolve_glyph(size_case: usize, ch: char) -> Option<AthlasResolvedGlyph> {
    ATHLAS_VARIANT_JSONS.get(size_case)?;
    let region = athlasmetrics::athlas_lookup_glyph_region(size_case, ch)?;
    Some(AthlasResolvedGlyph {
        region,
        texture: None,
        ready: false,
        ready_seq: 0,
    })
}
