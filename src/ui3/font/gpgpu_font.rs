use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use libm::roundf;
use trueos_gfx_core::Rgba8;

use crate::gfx::althlasfont::bitmapfont::{
    ATHLAS_FONT_FACE_LUCIDA_1X, ATHLAS_FONT_FACE_LUCIDA_HALF, ATHLAS_FONT_FACE_LUCIDA_THIRD,
    AthlasFontFace, athlas_font_bucket_atlas_metrics, athlas_font_line_height_px,
    athlas_lookup_glyph_region,
};

use super::super::Ui3Point;

static UI3_FONT_SPRITE64_NOT_READY_LOGS: AtomicU32 = AtomicU32::new(0);

pub(in crate::ui3) struct Ui3Sprite64TextBatch {
    pub(in crate::ui3) placements: Vec<crate::intel::gpgpu::GpgpuSprite64Placement>,
}

pub(in crate::ui3) fn collect_ui3_text_run_sprite64_batches(
    origin: Ui3Point,
    text: &str,
    color: Rgba8,
    font_tier: u8,
) -> Vec<Ui3Sprite64TextBatch> {
    collect_ui3_text_run_sprite64_batches_for_face(
        font_face_for_tier(font_tier),
        origin,
        text,
        color,
    )
}

pub(in crate::ui3) fn collect_ui3_text_run_sprite64_batches_for_face(
    face: AthlasFontFace,
    origin: Ui3Point,
    text: &str,
    color: Rgba8,
) -> Vec<Ui3Sprite64TextBatch> {
    if text.is_empty() || !origin.x.is_finite() || !origin.y.is_finite() {
        return Vec::new();
    }
    if !super::super::ui3_asset_service::ui3_font_sprite64_assets_ready() {
        let count = UI3_FONT_SPRITE64_NOT_READY_LOGS.fetch_add(1, Ordering::Relaxed) + 1;
        if count <= 8 || count.is_multiple_of(64) {
            crate::log!(
                "ui3-font: sprite64 draw skipped reason=assets-not-ready count={} chars={}\n",
                count,
                text.chars().count()
            );
        }
        return Vec::new();
    }

    let mut batches = Vec::new();
    let mut placements = Vec::new();
    let origin_x = origin.x;
    let mut pen_x = origin.x;
    let mut pen_y = origin.y;
    let color_rgba = rgba8_to_kernel_rgba(color);
    let line_height = font_line_height_px(face) as f32;

    for ch in text.chars() {
        match ch {
            '\r' => continue,
            '\n' => {
                flush_sprite64_font_batch(&mut placements, &mut batches);
                pen_x = origin_x;
                pen_y += line_height;
                continue;
            }
            _ if is_virtual_spacing_char(ch) => {
                pen_x += f32::from(font_spacing_advance_px(face, ch));
                continue;
            }
            _ => {}
        }

        if let Some(glyph) = font_glyph(face, ch) {
            placements.push(crate::intel::gpgpu::GpgpuSprite64Placement::tinted_src_over(
                glyph.sprite64_slot,
                roundf(pen_x) as i32,
                roundf(pen_y) as i32,
                color_rgba,
            ));
            pen_x += f32::from(glyph.advance_px);
        } else if let Some(region) =
            crate::gfx::althlasfont::twemoji::twemoji_lookup_glyph_region(ch)
        {
            placements.push(crate::intel::gpgpu::GpgpuSprite64Placement::src_over(
                region.slot,
                roundf(pen_x) as i32,
                roundf(pen_y) as i32,
            ));
            pen_x += f32::from(region.src_w.max(font_line_height_px(face)));
        } else {
            pen_x += f32::from(font_fallback_advance_px(face, ch));
        }

        if placements.len() >= 256 {
            flush_sprite64_font_batch(&mut placements, &mut batches);
        }
    }

    flush_sprite64_font_batch(&mut placements, &mut batches);
    batches
}

fn flush_sprite64_font_batch(
    placements: &mut Vec<crate::intel::gpgpu::GpgpuSprite64Placement>,
    out: &mut Vec<Ui3Sprite64TextBatch>,
) {
    if placements.is_empty() {
        return;
    }
    out.push(Ui3Sprite64TextBatch {
        placements: core::mem::take(placements),
    });
}

#[inline]
fn font_face_for_tier(tier: u8) -> AthlasFontFace {
    match tier {
        0 => ATHLAS_FONT_FACE_LUCIDA_THIRD,
        2 => ATHLAS_FONT_FACE_LUCIDA_1X,
        _ => ATHLAS_FONT_FACE_LUCIDA_HALF,
    }
}

#[derive(Copy, Clone, Debug)]
struct Sprite64FontGlyph {
    sprite64_slot: u16,
    advance_px: u16,
}

fn font_glyph(face: AthlasFontFace, ch: char) -> Option<Sprite64FontGlyph> {
    let region = athlas_lookup_glyph_region(face, ch)?;
    let sprite64_slot = crate::intel::gpgpu::sprite64_font_slot_for_region(face, region)?;
    Some(Sprite64FontGlyph {
        sprite64_slot,
        advance_px: region.src_w.max(1),
    })
}

#[inline]
fn font_line_height_px(face: AthlasFontFace) -> u16 {
    athlas_font_line_height_px(face).unwrap_or(64)
}

#[inline]
fn is_virtual_spacing_char(ch: char) -> bool {
    matches!(ch, ' ' | '\t') || (ch.is_whitespace() && ch != '\n')
}

#[inline]
fn font_spacing_advance_px(face: AthlasFontFace, ch: char) -> u16 {
    let space_px = athlas_font_bucket_atlas_metrics(face, 2)
        .map(|metrics| metrics.cell_w.max(1))
        .unwrap_or_else(|| font_line_height_px(face).saturating_div(3).max(1));
    match ch {
        '\t' => space_px.saturating_mul(4).max(1),
        _ => space_px,
    }
}

#[inline]
fn font_fallback_advance_px(face: AthlasFontFace, ch: char) -> u16 {
    if is_virtual_spacing_char(ch) {
        font_spacing_advance_px(face, ch)
    } else {
        font_line_height_px(face).saturating_div(2).max(1)
    }
}

#[inline]
fn rgba8_to_kernel_rgba(color: Rgba8) -> u32 {
    ((color.a as u32) << 24) | ((color.b as u32) << 16) | ((color.g as u32) << 8) | (color.r as u32)
}
