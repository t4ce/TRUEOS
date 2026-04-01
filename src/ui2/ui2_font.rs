use crate::gfx::althlasfont;
use crate::gfx::althlasfont::athlasmetrics::{self, ATHLAS_FONT_INFO};
use trueos_gfx_core::Rgba8;

use super::{Ui2Rect, draw_texture_rect_uv_no_present};

const UI2_FONT_DEFAULT_RGBA: Rgba8 = Rgba8::new(0, 0, 0, 255);

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Ui2FontTier {
    Half = 0,
    OneX = 1,
    TwoX = 2,
    Third = 3,
}

impl Ui2FontTier {
    pub(crate) const ALL: [Self; 4] = [Self::Third, Self::Half, Self::OneX, Self::TwoX];

    #[inline]
    pub(crate) const fn size_case(self) -> usize {
        self as usize
    }

    #[inline]
    pub(crate) fn atlas_cell_height_px(self) -> u16 {
        athlasmetrics::athlas_variant_line_height_px(self.size_case()).unwrap_or(1)
    }

    #[inline]
    pub(crate) fn atlas_line_height_px(self) -> u16 {
        self.atlas_cell_height_px()
    }

    #[inline]
    pub(crate) fn display_cell_height_px(self) -> u16 {
        self.atlas_cell_height_px()
    }

    #[inline]
    pub(crate) fn display_line_height_px(self) -> u16 {
        self.display_cell_height_px()
    }

    #[inline]
    pub(crate) fn display_scale_num(self) -> u16 {
        self.display_line_height_px()
    }

    #[inline]
    pub(crate) fn display_scale_den(self) -> u16 {
        self.atlas_line_height_px()
    }

    #[inline]
    pub(crate) fn display_scale(self) -> f32 {
        self.display_scale_num() as f32 / self.display_scale_den() as f32
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
    pub third_ready_seq: u32,
    pub half_ready_seq: u32,
    pub one_x_ready_seq: u32,
    pub two_x_ready_seq: u32,
}

impl Ui2FontReadySnapshot {
    #[inline]
    pub(crate) fn capture() -> Self {
        Self {
            third_ready_seq: Ui2FontTier::Third.ready_seq(),
            half_ready_seq: Ui2FontTier::Half.ready_seq(),
            one_x_ready_seq: Ui2FontTier::OneX.ready_seq(),
            two_x_ready_seq: Ui2FontTier::TwoX.ready_seq(),
        }
    }

    #[inline]
    pub(crate) fn tier_ready_seq(self, tier: Ui2FontTier) -> u32 {
        match tier {
            Ui2FontTier::Third => self.third_ready_seq,
            Ui2FontTier::Half => self.half_ready_seq,
            Ui2FontTier::OneX => self.one_x_ready_seq,
            Ui2FontTier::TwoX => self.two_x_ready_seq,
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Ui2FontCellMetrics {
    pub cell_w_px: u16,
    pub cell_h_px: u16,
    pub glyph_w_px: u16,
    pub glyph_h_px: u16,
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

    let mut best_tier = Ui2FontTier::OneX;
    let mut best_distance = (target - Ui2FontTier::OneX.display_line_height_px() as f32).abs();
    for tier in [Ui2FontTier::Third, Ui2FontTier::Half, Ui2FontTier::TwoX] {
        let distance = (target - tier.display_line_height_px() as f32).abs();
        if distance < best_distance {
            best_tier = tier;
            best_distance = distance;
        }
    }

    best_tier
}

#[inline]
pub(crate) fn ui2_font_native_line_height_px(tier: Ui2FontTier) -> u16 {
    tier.display_line_height_px()
}

#[inline]
pub(crate) fn ui2_font_tier_max_cell_width_px(tier: Ui2FontTier) -> u16 {
    athlasmetrics::athlas_variant_max_cell_width(tier.size_case())
        .unwrap_or_else(|| tier.atlas_cell_height_px().saturating_div(2).max(1))
}

#[inline]
pub(crate) fn ui2_font_glyph_cell_metrics(glyph: &Ui2FontGlyph) -> Ui2FontCellMetrics {
    Ui2FontCellMetrics {
        cell_w_px: glyph.advance_px.max(1),
        cell_h_px: glyph.line_height_px.max(1),
        glyph_w_px: glyph.region.src_w.max(1),
        glyph_h_px: glyph.region.src_h.max(1),
    }
}

#[inline]
pub(crate) fn ui2_font_place_glyph_top_center(glyph: &Ui2FontGlyph, rect: Ui2Rect) -> Ui2Rect {
    let metrics = ui2_font_glyph_cell_metrics(glyph);
    let cell_w = f32::from(metrics.cell_w_px.max(metrics.glyph_w_px).max(1));
    let glyph_w = f32::from(metrics.glyph_w_px.max(1));
    let glyph_h = f32::from(metrics.glyph_h_px.max(1));
    Ui2Rect::new(rect.x + ((rect.w - cell_w) * 0.5), rect.y, glyph_w, glyph_h)
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
        line_height_px: glyph.region.src_h.max(tier.atlas_line_height_px()),
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
    let scale_num = u32::from(tier.display_scale_num());
    let scale_den = u32::from(tier.display_scale_den());
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
        let display_advance_px =
            ((u32::from(advance_px) * scale_num) + (scale_den / 2)) / scale_den;
        let display_advance_px = display_advance_px.max(1);
        line_width = line_width.saturating_add(display_advance_px);
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
    let scale = tier.display_scale();
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
                drew_any |= ui2_font_draw_glyph_rect_no_present(
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
                    alpha,
                );
            }
        }

        pen_x += advance_px;
    }

    drew_any
}

fn ui2_font_draw_glyph_rect_no_present(
    tex_id: u32,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    u0: f32,
    v0: f32,
    u1: f32,
    v1: f32,
    view_w: u32,
    view_h: u32,
    alpha: u8,
) -> bool {
    if tex_id == 0 || !(width > 0.0 && height > 0.0) {
        return false;
    }

    let transform = trueos_gfx_core::ViewTransform::from_extent(view_w, view_h);
    let mut verts = alloc::vec::Vec::with_capacity(6 * trueos_gfx_core::TEX_VERTEX_SIZE);
    trueos_gfx_core::push_tex_quad_px(
        &mut verts,
        transform,
        x,
        y,
        x + width,
        y + height,
        [u0, v0, u1, v1],
        Rgba8::new(
            UI2_FONT_DEFAULT_RGBA.r,
            UI2_FONT_DEFAULT_RGBA.g,
            UI2_FONT_DEFAULT_RGBA.b,
            alpha,
        ),
    );
    let _ = unsafe { crate::r::io::cabi::trueos_cabi_gfx_set_sampler(0, 0, 0, 0) };
    let _ = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_set_blend(1, 0x0302, 0x0303, 0x0302, 0x0303, 0, 0)
    };
    let rc = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_draw_tex_triangles_no_present(
            tex_id,
            verts.as_ptr(),
            verts.len(),
        )
    };
    let _ = unsafe { crate::r::io::cabi::trueos_cabi_gfx_set_blend(0, 1, 0, 1, 0, 0, 0) };
    rc == 0
}

#[inline]
fn fallback_advance_px(tier: Ui2FontTier, ch: char) -> u16 {
    if ch == ' ' {
        (tier.atlas_cell_height_px() / 3).max(1)
    } else {
        tier.atlas_cell_height_px().saturating_div(2).max(1)
    }
}
