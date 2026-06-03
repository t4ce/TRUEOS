use alloc::vec::Vec;

use serde::Deserialize;
use spin::{Mutex, Once};

use super::{AthlasBucketTexture, AthlasResolvedGlyph};
use crate::gfx::althlasfont::athlasmetrics::AthlasGlyphRegion;

pub const TWEMOJI_TEX_ID: u32 = 4_840;
pub const TWEMOJI_ATLAS_PNG: &[u8] = include_bytes!("twemoji-1x/atlas.png");
const TWEMOJI_ATLAS_SET_JSON: &str = include_str!("twemoji-1x/atlas-set.json");

#[derive(Debug, Deserialize)]
struct TwemojiAtlasSet {
    atlas: TwemojiAtlas,
}

#[derive(Debug, Deserialize)]
struct TwemojiAtlas {
    cell_w: u16,
    cell_h: u16,
    grid_w: u16,
    grid_h: u16,
    slots: Vec<u32>,
    unplaced: Vec<u32>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct TwemojiRuntime {
    ready: bool,
    ready_seq: u32,
    texture: AthlasBucketTexture,
}

static TWEMOJI_ATLAS_SET: Once<Option<TwemojiAtlasSet>> = Once::new();
static TWEMOJI_RUNTIME: Mutex<TwemojiRuntime> = Mutex::new(TwemojiRuntime {
    ready: false,
    ready_seq: 0,
    texture: AthlasBucketTexture {
        tex_id: 0,
        width: 0,
        height: 0,
    },
});

fn atlas_set() -> Option<&'static TwemojiAtlasSet> {
    TWEMOJI_ATLAS_SET
        .call_once(|| match serde_json::from_str(TWEMOJI_ATLAS_SET_JSON) {
            Ok(set) => Some(set),
            Err(err) => {
                crate::log!("twemoji: atlas-set parse failed err={}\n", err);
                None
            }
        })
        .as_ref()
}

#[inline]
pub fn twemoji_cell_height_px() -> u16 {
    atlas_set().map(|set| set.atlas.cell_h).unwrap_or(0)
}

pub fn twemoji_reset_state() {
    let mut runtime = TWEMOJI_RUNTIME.lock();
    *runtime = TwemojiRuntime::default();
}

pub fn twemoji_register_texture(tex_id: u32, width: u32, height: u32) {
    let mut runtime = TWEMOJI_RUNTIME.lock();
    runtime.texture = AthlasBucketTexture {
        tex_id,
        width,
        height,
    };
}

pub fn twemoji_mark_ready() -> Option<u32> {
    let mut runtime = TWEMOJI_RUNTIME.lock();
    if runtime.texture.tex_id == 0 {
        return None;
    }
    runtime.ready = true;
    runtime.ready_seq = runtime.ready_seq.wrapping_add(1).max(1);
    Some(runtime.ready_seq)
}

#[inline]
pub fn twemoji_ready() -> bool {
    TWEMOJI_RUNTIME.lock().ready
}

pub fn twemoji_lookup_glyph_region(ch: char) -> Option<AthlasGlyphRegion> {
    let atlas = &atlas_set()?.atlas;
    let codepoint = u32::from(ch);
    if atlas.unplaced.binary_search(&codepoint).is_ok() {
        return None;
    }
    let slot = atlas.slots.binary_search(&codepoint).ok()? as u16;
    let grid_w = atlas.grid_w.max(1);
    let atlas_w = atlas.cell_w.saturating_mul(grid_w);
    let atlas_h = atlas.cell_h.saturating_mul(atlas.grid_h.max(1));
    let src_x = atlas.cell_w.saturating_mul(slot % grid_w);
    let src_y = atlas.cell_h.saturating_mul(slot / grid_w);
    Some(AthlasGlyphRegion {
        bucket: 0,
        slot,
        src_x,
        src_y,
        src_w: atlas.cell_w,
        src_h: atlas.cell_h,
        atlas_w,
        atlas_h,
    })
}

pub fn twemoji_lookup_slot_region(slot: u16) -> Option<AthlasGlyphRegion> {
    let atlas = &atlas_set()?.atlas;
    if slot as usize >= atlas.slots.len() {
        return None;
    }
    let grid_w = atlas.grid_w.max(1);
    let atlas_w = atlas.cell_w.saturating_mul(grid_w);
    let atlas_h = atlas.cell_h.saturating_mul(atlas.grid_h.max(1));
    let src_x = atlas.cell_w.saturating_mul(slot % grid_w);
    let src_y = atlas.cell_h.saturating_mul(slot / grid_w);
    Some(AthlasGlyphRegion {
        bucket: 0,
        slot,
        src_x,
        src_y,
        src_w: atlas.cell_w,
        src_h: atlas.cell_h,
        atlas_w,
        atlas_h,
    })
}

pub fn twemoji_slot_count() -> u16 {
    atlas_set()
        .map(|set| set.atlas.slots.len().min(u16::MAX as usize) as u16)
        .unwrap_or(0)
}

pub fn twemoji_resolve_glyph(ch: char) -> Option<AthlasResolvedGlyph> {
    let region = twemoji_lookup_glyph_region(ch)?;
    let runtime = TWEMOJI_RUNTIME.lock();
    let texture = (runtime.texture.tex_id != 0).then_some(runtime.texture);
    Some(AthlasResolvedGlyph {
        region,
        texture,
        ready: runtime.ready,
        ready_seq: runtime.ready_seq,
    })
}
