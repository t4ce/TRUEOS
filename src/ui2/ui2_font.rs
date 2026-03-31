use crate::gfx::althlasfont;
use crate::gfx::althlasfont::athlasmetrics::{self, ATHLAS_FONT_INFO};

use super::draw_texture_rect_uv_no_present;

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Ui2FontTier {
    Half = 0,
    OneX = 1,
    ThreeX = 2,
}

impl Ui2FontTier {
    pub(crate) const ALL: [Self; 3] = [Self::Half, Self::OneX, Self::ThreeX];

    #[inline]
    pub(crate) const fn size_case(self) -> usize {
        self as usize
    }

    #[inline]
    pub(crate) const fn native_cell_height_px(self) -> u16 {
        match self {
            Self::Half => 32,
            Self::OneX => 64,
            Self::ThreeX => 192,
        }
    }

    #[inline]
    pub(crate) const fn native_line_height_px(self) -> u16 {
        self.native_cell_height_px()
    }

    #[inline]
    pub(crate) fn ready(self) -> bool {
        althlasfont::athlas_tier_ready(self.size_case())
    }

    #[inline]
    pub(crate) fn ready_seq(self) -> u32 {
        althlasfont::athlas_tier_ready_seq(self.size_case())
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Ui2FontReadySnapshot {
    pub half_ready_seq: u32,
    pub one_x_ready_seq: u32,
    pub three_x_ready_seq: u32,
}

impl Ui2FontReadySnapshot {
    #[inline]
    pub(crate) fn capture() -> Self {
        Self {
            half_ready_seq: Ui2FontTier::Half.ready_seq(),
            one_x_ready_seq: Ui2FontTier::OneX.ready_seq(),
            three_x_ready_seq: Ui2FontTier::ThreeX.ready_seq(),
        }
    }

    #[inline]
    pub(crate) fn tier_ready_seq(self, tier: Ui2FontTier) -> u32 {
        match tier {
            Ui2FontTier::Half => self.half_ready_seq,
            Ui2FontTier::OneX => self.one_x_ready_seq,
            Ui2FontTier::ThreeX => self.three_x_ready_seq,
        }
    }

    #[inline]
    pub(crate) fn changed_since(self, earlier: Self) -> bool {
        self != earlier
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Ui2FontGlyph {
    pub ch: char,
    pub tier: Ui2FontTier,
    pub advance_px: u16,
    pub line_height_px: u16,
    pub ready: bool,
    pub ready_seq: u32,
    pub texture: Option<althlasfont::AthlasBucketTexture>,
    pub region: athlasmetrics::AthlasGlyphRegion,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Ui2FontTextMetrics {
    pub width_px: u32,
    pub height_px: u32,
    pub max_line_width_px: u32,
    pub line_count: u16,
    pub line_height_px: u16,
}

#[inline]
pub(crate) fn ui2_font_ready_snapshot() -> Ui2FontReadySnapshot {
    Ui2FontReadySnapshot::capture()
}

#[inline]
pub(crate) fn ui2_font_pick_tier_for_px(px_h: f32) -> Ui2FontTier {
    if !(px_h.is_finite() && px_h > 0.0) {
        return Ui2FontTier::OneX;
    }

    let target = px_h.max(1.0);
    let variants = [
        (Ui2FontTier::Half, 32.0f32),
        (Ui2FontTier::OneX, 64.0f32),
        (Ui2FontTier::ThreeX, 192.0f32),
    ];
    let mut best_tier = Ui2FontTier::Half;
    let mut best_err = f32::MAX;
    for (tier, native_px_h) in variants {
        let err = libm::fabsf(native_px_h - target);
        if err < best_err {
            best_err = err;
            best_tier = tier;
        }
    }
    best_tier
}

#[inline]
pub(crate) fn ui2_font_native_line_height_px(tier: Ui2FontTier) -> u16 {
    tier.native_line_height_px()
}

#[inline]
pub(crate) fn ui2_font_units_per_em() -> u16 {
    ATHLAS_FONT_INFO.units_per_em
}

#[inline]
pub(crate) fn ui2_font_line_height_units() -> u16 {
    ATHLAS_FONT_INFO.line_height
}

pub(crate) fn ui2_font_resolve_glyph(tier: Ui2FontTier, ch: char) -> Option<Ui2FontGlyph> {
    let glyph = althlasfont::athlas_resolve_glyph(tier.size_case(), ch)?;
    Some(Ui2FontGlyph {
        ch,
        tier,
        advance_px: glyph.region.src_w.max(1),
        line_height_px: glyph.region.src_h.max(tier.native_line_height_px()),
        ready: glyph.ready,
        ready_seq: glyph.ready_seq,
        texture: glyph.texture,
        region: glyph.region,
    })
}

#[inline]
fn ui2_font_resolve_glyph_or_fallback(tier: Ui2FontTier, ch: char) -> Option<Ui2FontGlyph> {
    ui2_font_resolve_glyph(tier, ch).or_else(|| ui2_font_resolve_glyph(tier, '?'))
}

pub(crate) fn ui2_font_measure_text(tier: Ui2FontTier, text: &str) -> Ui2FontTextMetrics {
    let line_height_px = ui2_font_native_line_height_px(tier);
    if text.is_empty() {
        return Ui2FontTextMetrics {
            width_px: 0,
            height_px: u32::from(line_height_px),
            max_line_width_px: 0,
            line_count: 1,
            line_height_px,
        };
    }

    let mut line_count = 1u16;
    let mut line_width = 0u32;
    let mut max_line_width = 0u32;

    for ch in text.chars() {
        if ch == '\n' {
            max_line_width = max_line_width.max(line_width);
            line_width = 0;
            line_count = line_count.saturating_add(1);
            continue;
        }

        let advance_px = ui2_font_resolve_glyph_or_fallback(tier, ch)
            .map(|glyph| glyph.advance_px)
            .unwrap_or_else(|| fallback_advance_px(tier, ch));
        line_width = line_width.saturating_add(u32::from(advance_px));
    }

    max_line_width = max_line_width.max(line_width);

    Ui2FontTextMetrics {
        width_px: max_line_width,
        height_px: u32::from(line_count).saturating_mul(u32::from(line_height_px)),
        max_line_width_px: max_line_width,
        line_count,
        line_height_px,
    }
}

pub(crate) fn ui2_font_draw_text_line_no_present(
    text: &str,
    x: f32,
    y: f32,
    max_width_px: f32,
    px_h: f32,
    view_w: u32,
    view_h: u32,
    alpha: u8,
) -> bool {
    if text.is_empty() || !(px_h.is_finite() && px_h > 0.0) || max_width_px <= 0.0 {
        return false;
    }

    let tier = ui2_font_pick_tier_for_px(px_h);
    let native_line_h = tier.native_line_height_px().max(1) as f32;
    let scale = px_h / native_line_h;
    if !(scale.is_finite() && scale > 0.0) {
        return false;
    }

    let mut pen_x = x;
    let right = x + max_width_px;
    let mut drew_any = false;

    for ch in text.chars() {
        if ch == '\n' || pen_x >= right {
            break;
        }

        let Some(glyph) = ui2_font_resolve_glyph_or_fallback(tier, ch) else {
            pen_x += f32::from(fallback_advance_px(tier, ch)) * scale;
            continue;
        };

        let advance_px = f32::from(glyph.advance_px) * scale;
        if pen_x + advance_px > right {
            break;
        }

        if glyph.ready {
            if let Some(texture) = glyph.texture {
                let draw_w = f32::from(glyph.region.src_w) * scale;
                let draw_h = f32::from(glyph.region.src_h) * scale;
                let atlas_w = f32::from(glyph.region.atlas_w.max(1));
                let atlas_h = f32::from(glyph.region.atlas_h.max(1));
                let src_x = f32::from(glyph.region.src_x);
                let src_y = f32::from(glyph.region.src_y);
                drew_any |= draw_texture_rect_uv_no_present(
                    texture.tex_id,
                    pen_x,
                    y,
                    draw_w,
                    draw_h,
                    src_x / atlas_w,
                    src_y / atlas_h,
                    (src_x + f32::from(glyph.region.src_w)) / atlas_w,
                    (src_y + f32::from(glyph.region.src_h)) / atlas_h,
                    view_w,
                    view_h,
                    true,
                    alpha,
                );
            }
        }

        pen_x += advance_px;
    }

    drew_any
}

#[inline]
fn fallback_advance_px(tier: Ui2FontTier, ch: char) -> u16 {
    if ch == ' ' {
        (tier.native_cell_height_px() / 3).max(1)
    } else {
        tier.native_cell_height_px().saturating_div(2).max(1)
    }
}
