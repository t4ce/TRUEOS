pub mod athlasmetrics;
pub mod bitmapfont;
pub mod twemoji;

use self::athlasmetrics::AthlasGlyphRegion;

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
