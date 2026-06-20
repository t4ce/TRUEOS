// Shared Athlas font metric types used by the bitmap font, Twemoji, and GPGPU paths.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AthlasBucketAtlasMetrics {
    pub cell_w: u16,
    pub cell_h: u16,
    pub grid_w: u16,
    pub grid_h: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AthlasGlyphRegion {
    pub bucket: u8,
    pub slot: u16,
    pub src_x: u16,
    pub src_y: u16,
    pub src_w: u16,
    pub src_h: u16,
    pub atlas_w: u16,
    pub atlas_h: u16,
}

pub const ATHLAS_BUCKET_COUNT: usize = 8;
