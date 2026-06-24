use alloc::{collections::BTreeMap, string::String, vec::Vec};

use serde::Deserialize;
use spin::Once;

use super::athlasmetrics::{ATHLAS_BUCKET_COUNT, AthlasBucketAtlasMetrics, AthlasGlyphRegion};

const LUCIDA_METRICS_JSON: &str = include_str!("lucida-metrics.json");
const PALATINO_METRICS_JSON: &str = include_str!("palatino-metrics.json");
const LUCIDA_THIRD_BUCKET_PNGS: [&[u8]; ATHLAS_BUCKET_COUNT] = [
    include_bytes!("lucida-third/atlas-g00.png"),
    include_bytes!("lucida-third/atlas-g01.png"),
    include_bytes!("lucida-third/atlas-g02.png"),
    include_bytes!("lucida-third/atlas-g03.png"),
    include_bytes!("lucida-third/atlas-g04.png"),
    include_bytes!("lucida-third/atlas-g05.png"),
    include_bytes!("lucida-third/atlas-g06.png"),
    include_bytes!("lucida-third/atlas-g07.png"),
];
const LUCIDA_HALF_BUCKET_PNGS: [&[u8]; ATHLAS_BUCKET_COUNT] = [
    include_bytes!("lucida-half/atlas-g00.png"),
    include_bytes!("lucida-half/atlas-g01.png"),
    include_bytes!("lucida-half/atlas-g02.png"),
    include_bytes!("lucida-half/atlas-g03.png"),
    include_bytes!("lucida-half/atlas-g04.png"),
    include_bytes!("lucida-half/atlas-g05.png"),
    include_bytes!("lucida-half/atlas-g06.png"),
    include_bytes!("lucida-half/atlas-g07.png"),
];
const LUCIDA_1X_BUCKET_PNGS: [&[u8]; ATHLAS_BUCKET_COUNT] = [
    include_bytes!("lucida-1x/atlas-g00.png"),
    include_bytes!("lucida-1x/atlas-g01.png"),
    include_bytes!("lucida-1x/atlas-g02.png"),
    include_bytes!("lucida-1x/atlas-g03.png"),
    include_bytes!("lucida-1x/atlas-g04.png"),
    include_bytes!("lucida-1x/atlas-g05.png"),
    include_bytes!("lucida-1x/atlas-g06.png"),
    include_bytes!("lucida-1x/atlas-g07.png"),
];
const PALATINO_THIRD_BUCKET_PNGS: [&[u8]; ATHLAS_BUCKET_COUNT] = [
    include_bytes!("palatino-third/atlas-g00.png"),
    include_bytes!("palatino-third/atlas-g01.png"),
    include_bytes!("palatino-third/atlas-g02.png"),
    include_bytes!("palatino-third/atlas-g03.png"),
    include_bytes!("palatino-third/atlas-g04.png"),
    include_bytes!("palatino-third/atlas-g05.png"),
    include_bytes!("palatino-third/atlas-g06.png"),
    include_bytes!("palatino-third/atlas-g07.png"),
];
const PALATINO_HALF_BUCKET_PNGS: [&[u8]; ATHLAS_BUCKET_COUNT] = [
    include_bytes!("palatino-half/atlas-g00.png"),
    include_bytes!("palatino-half/atlas-g01.png"),
    include_bytes!("palatino-half/atlas-g02.png"),
    include_bytes!("palatino-half/atlas-g03.png"),
    include_bytes!("palatino-half/atlas-g04.png"),
    include_bytes!("palatino-half/atlas-g05.png"),
    include_bytes!("palatino-half/atlas-g06.png"),
    include_bytes!("palatino-half/atlas-g07.png"),
];
const PALATINO_1X_BUCKET_PNGS: [&[u8]; ATHLAS_BUCKET_COUNT] = [
    include_bytes!("palatino-1x/atlas-g00.png"),
    include_bytes!("palatino-1x/atlas-g01.png"),
    include_bytes!("palatino-1x/atlas-g02.png"),
    include_bytes!("palatino-1x/atlas-g03.png"),
    include_bytes!("palatino-1x/atlas-g04.png"),
    include_bytes!("palatino-1x/atlas-g05.png"),
    include_bytes!("palatino-1x/atlas-g06.png"),
    include_bytes!("palatino-1x/atlas-g07.png"),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AthlasFontFamily {
    Lucida,
    #[allow(dead_code)]
    Palatino,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AthlasFontTier {
    Third,
    Half,
    OneX,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AthlasFontFace {
    pub family: AthlasFontFamily,
    pub tier: AthlasFontTier,
}

pub const ATHLAS_FONT_FACE_LUCIDA_THIRD: AthlasFontFace = AthlasFontFace {
    family: AthlasFontFamily::Lucida,
    tier: AthlasFontTier::Third,
};

pub const ATHLAS_FONT_FACE_LUCIDA_HALF: AthlasFontFace = AthlasFontFace {
    family: AthlasFontFamily::Lucida,
    tier: AthlasFontTier::Half,
};

pub const ATHLAS_FONT_FACE_LUCIDA_1X: AthlasFontFace = AthlasFontFace {
    family: AthlasFontFamily::Lucida,
    tier: AthlasFontTier::OneX,
};

pub const ATHLAS_UI3_SPRITE64_FONT_FACES: [AthlasFontFace; 3] = [
    ATHLAS_FONT_FACE_LUCIDA_THIRD,
    ATHLAS_FONT_FACE_LUCIDA_HALF,
    ATHLAS_FONT_FACE_LUCIDA_1X,
];

pub fn athlas_font_bucket_pngs(
    face: AthlasFontFace,
) -> &'static [&'static [u8]; ATHLAS_BUCKET_COUNT] {
    match (face.family, face.tier) {
        (AthlasFontFamily::Lucida, AthlasFontTier::Third) => &LUCIDA_THIRD_BUCKET_PNGS,
        (AthlasFontFamily::Lucida, AthlasFontTier::Half) => &LUCIDA_HALF_BUCKET_PNGS,
        (AthlasFontFamily::Lucida, AthlasFontTier::OneX) => &LUCIDA_1X_BUCKET_PNGS,
        (AthlasFontFamily::Palatino, AthlasFontTier::Third) => &PALATINO_THIRD_BUCKET_PNGS,
        (AthlasFontFamily::Palatino, AthlasFontTier::Half) => &PALATINO_HALF_BUCKET_PNGS,
        (AthlasFontFamily::Palatino, AthlasFontTier::OneX) => &PALATINO_1X_BUCKET_PNGS,
    }
}

#[derive(Debug, Deserialize)]
struct AthlasMetricsSet {
    tables: Vec<AthlasMetricsTable>,
    variants: BTreeMap<String, AthlasMetricsVariant>,
}

#[derive(Debug, Deserialize)]
struct AthlasMetricsVariant {
    tables: Vec<AthlasMetricsTable>,
}

#[derive(Debug, Deserialize)]
struct AthlasMetricsTable {
    bucket: u8,
    slots: Option<Vec<u32>>,
    unplaced: Option<Vec<u32>>,
    cell_w: Option<u16>,
    cell_h: Option<u16>,
    grid_w: Option<u16>,
    grid_h: Option<u16>,
}

static LUCIDA_METRICS: Once<Option<AthlasMetricsSet>> = Once::new();
static PALATINO_METRICS: Once<Option<AthlasMetricsSet>> = Once::new();

#[inline]
pub const fn athlas_font_family_name(family: AthlasFontFamily) -> &'static str {
    match family {
        AthlasFontFamily::Lucida => "lucida",
        AthlasFontFamily::Palatino => "palatino",
    }
}

#[inline]
pub const fn athlas_font_tier_name(tier: AthlasFontTier) -> &'static str {
    match tier {
        AthlasFontTier::Third => "third",
        AthlasFontTier::Half => "half",
        AthlasFontTier::OneX => "1x",
    }
}

fn metrics_set(family: AthlasFontFamily) -> Option<&'static AthlasMetricsSet> {
    let slot = match family {
        AthlasFontFamily::Lucida => &LUCIDA_METRICS,
        AthlasFontFamily::Palatino => &PALATINO_METRICS,
    };
    let json = match family {
        AthlasFontFamily::Lucida => LUCIDA_METRICS_JSON,
        AthlasFontFamily::Palatino => PALATINO_METRICS_JSON,
    };
    let name = athlas_font_family_name(family);

    slot.call_once(|| match serde_json::from_str(json) {
        Ok(set) => Some(set),
        Err(err) => {
            crate::log!("athlas-font: metrics parse failed family={} err={}\n", name, err);
            None
        }
    })
    .as_ref()
}

#[inline]
fn table_by_bucket(tables: &[AthlasMetricsTable], bucket: u8) -> Option<&AthlasMetricsTable> {
    tables.iter().find(|table| table.bucket == bucket)
}

#[inline]
fn variant_for_face(set: &AthlasMetricsSet, face: AthlasFontFace) -> Option<&AthlasMetricsVariant> {
    set.variants.get(athlas_font_tier_name(face.tier))
}

pub fn athlas_font_bucket_atlas_metrics(
    face: AthlasFontFace,
    bucket: usize,
) -> Option<AthlasBucketAtlasMetrics> {
    let bucket = u8::try_from(bucket).ok()?;
    let set = metrics_set(face.family)?;
    let variant = variant_for_face(set, face)?;
    let table = table_by_bucket(&variant.tables, bucket)?;

    Some(AthlasBucketAtlasMetrics {
        cell_w: table.cell_w?,
        cell_h: table.cell_h?,
        grid_w: table.grid_w?,
        grid_h: table.grid_h?,
    })
}

#[inline]
pub fn athlas_font_line_height_px(face: AthlasFontFace) -> Option<u16> {
    athlas_font_bucket_atlas_metrics(face, 0).map(|metrics| metrics.cell_h)
}

pub fn athlas_font_bucket_cell_count(face: AthlasFontFace, bucket: usize) -> Option<u32> {
    let metrics = athlas_font_bucket_atlas_metrics(face, bucket)?;
    Some(u32::from(metrics.grid_w.max(1)).saturating_mul(u32::from(metrics.grid_h.max(1))))
}

pub fn athlas_font_face_cell_count(face: AthlasFontFace) -> Option<u32> {
    let mut count = 0u32;
    for bucket in 0..ATHLAS_BUCKET_COUNT {
        count = count.checked_add(athlas_font_bucket_cell_count(face, bucket)?)?;
    }
    Some(count)
}

pub fn athlas_lookup_glyph_region(face: AthlasFontFace, ch: char) -> Option<AthlasGlyphRegion> {
    let codepoint = u32::from(ch);
    let set = metrics_set(face.family)?;
    let variant = variant_for_face(set, face)?;

    for bucket in 0..ATHLAS_BUCKET_COUNT {
        let bucket_u8 = bucket as u8;
        let variant_table = table_by_bucket(&variant.tables, bucket_u8)?;
        let shared_table = table_by_bucket(&set.tables, bucket_u8)?;
        let slots = variant_table
            .slots
            .as_deref()
            .or(shared_table.slots.as_deref())?;
        let unplaced = variant_table
            .unplaced
            .as_deref()
            .or(shared_table.unplaced.as_deref())
            .unwrap_or(&[]);

        if unplaced.binary_search(&codepoint).is_ok() {
            continue;
        }
        let Some(slot) = slots.iter().position(|slot| *slot == codepoint) else {
            continue;
        };
        let slot = u16::try_from(slot).ok()?;
        let atlas = athlas_font_bucket_atlas_metrics(face, bucket)?;
        let grid_w = u32::from(atlas.grid_w.max(1));
        let slot_u32 = u32::from(slot);
        let src_x = (slot_u32 % grid_w).saturating_mul(u32::from(atlas.cell_w));
        let src_y = (slot_u32 / grid_w).saturating_mul(u32::from(atlas.cell_h));
        return Some(AthlasGlyphRegion {
            bucket: bucket_u8,
            slot,
            src_x: src_x.min(u32::from(u16::MAX)) as u16,
            src_y: src_y.min(u32::from(u16::MAX)) as u16,
            src_w: atlas.cell_w,
            src_h: atlas.cell_h,
            atlas_w: atlas.cell_w.saturating_mul(atlas.grid_w),
            atlas_h: atlas.cell_h.saturating_mul(atlas.grid_h),
        });
    }

    None
}

#[inline]
pub fn athlas_glyph_advance_px(region: AthlasGlyphRegion) -> u16 {
    region.src_w.max(1)
}
