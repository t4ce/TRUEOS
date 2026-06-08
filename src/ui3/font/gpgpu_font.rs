use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use libm::roundf;
use trueos_gfx_core::Rgba8;

use crate::gfx::althlasfont::athlasmetrics;

use super::super::Ui3Point;

const UI3_FONT_LUCIDA_1X_SIZE_CASE: usize = 1;
static UI3_FONT_SPRITE64_NOT_READY_LOGS: AtomicU32 = AtomicU32::new(0);

#[derive(Copy, Clone, Debug, Default)]
pub(in crate::ui3) struct Ui3GpgpuFontRunResult {
    pub(in crate::ui3) submitted: bool,
    pub(in crate::ui3) descriptors: usize,
    pub(in crate::ui3) walkers: usize,
    pub(in crate::ui3) submit_ms: u64,
    pub(in crate::ui3) total_ms: u64,
}

pub(in crate::ui3) fn draw_ui3_text_run_sprite64(
    origin: Ui3Point,
    text: &str,
    color: Rgba8,
) -> Ui3GpgpuFontRunResult {
    if text.is_empty() || !origin.x.is_finite() || !origin.y.is_finite() {
        return Ui3GpgpuFontRunResult::default();
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
        return Ui3GpgpuFontRunResult::default();
    }

    let mut result = Ui3GpgpuFontRunResult::default();
    let mut placements = Vec::new();
    let origin_x = origin.x;
    let mut pen_x = origin.x;
    let mut pen_y = origin.y;
    let color_rgba = rgba8_to_kernel_rgba(color);
    let line_height = lucida1x_line_height_px() as f32;

    for ch in text.chars() {
        match ch {
            '\r' => continue,
            '\n' => {
                flush_sprite64_font_batch(&mut placements, &mut result);
                pen_x = origin_x;
                pen_y += line_height;
                continue;
            }
            _ if is_virtual_spacing_char(ch) => {
                pen_x += f32::from(lucida1x_spacing_advance_px(ch));
                continue;
            }
            _ => {}
        }

        if let Some(glyph) = lucida1x_glyph(ch) {
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
            pen_x += f32::from(region.src_w.max(lucida1x_line_height_px()));
        } else {
            pen_x += f32::from(lucida1x_fallback_advance_px(ch));
        }

        if placements.len() >= 256 {
            flush_sprite64_font_batch(&mut placements, &mut result);
        }
    }

    flush_sprite64_font_batch(&mut placements, &mut result);
    result
}

fn flush_sprite64_font_batch(
    placements: &mut Vec<crate::intel::gpgpu::GpgpuSprite64Placement>,
    out: &mut Ui3GpgpuFontRunResult,
) {
    if placements.is_empty() {
        return;
    }

    if let Some(result) = crate::intel::gpgpu::sprite64_worklist_primary(
        placements.as_slice(),
        false,
        "ui3-font-sprite64-worklist",
    ) {
        out.submitted |= result.submitted;
        out.descriptors = out.descriptors.saturating_add(result.descriptors);
        out.walkers = out.walkers.saturating_add(result.walkers);
        out.submit_ms = out.submit_ms.saturating_add(result.submit_ms);
        out.total_ms = out.total_ms.saturating_add(result.total_ms);
    }
    placements.clear();
}

#[derive(Copy, Clone, Debug)]
struct Lucida1xGlyph {
    sprite64_slot: u16,
    advance_px: u16,
}

fn lucida1x_glyph(ch: char) -> Option<Lucida1xGlyph> {
    let region = athlasmetrics::athlas_lookup_glyph_region(UI3_FONT_LUCIDA_1X_SIZE_CASE, ch)?;
    let sprite64_slot = crate::intel::gpgpu::sprite64_lucida1x_slot_for_region(region)?;
    Some(Lucida1xGlyph {
        sprite64_slot,
        advance_px: region.src_w.max(1),
    })
}

#[inline]
fn lucida1x_line_height_px() -> u16 {
    athlasmetrics::athlas_variant_line_height_px(UI3_FONT_LUCIDA_1X_SIZE_CASE).unwrap_or(64)
}

#[inline]
fn is_virtual_spacing_char(ch: char) -> bool {
    matches!(ch, ' ' | '\t') || (ch.is_whitespace() && ch != '\n')
}

#[inline]
fn lucida1x_spacing_advance_px(ch: char) -> u16 {
    let space_px = athlasmetrics::athlas_bucket_atlas_metrics(UI3_FONT_LUCIDA_1X_SIZE_CASE, 2)
        .map(|metrics| metrics.cell_w.max(1))
        .unwrap_or_else(|| lucida1x_line_height_px().saturating_div(3).max(1));
    match ch {
        '\t' => space_px.saturating_mul(4).max(1),
        _ => space_px,
    }
}

#[inline]
fn lucida1x_fallback_advance_px(ch: char) -> u16 {
    if is_virtual_spacing_char(ch) {
        lucida1x_spacing_advance_px(ch)
    } else {
        lucida1x_line_height_px().saturating_div(2).max(1)
    }
}

#[inline]
fn rgba8_to_kernel_rgba(color: Rgba8) -> u32 {
    ((color.a as u32) << 24) | ((color.b as u32) << 16) | ((color.g as u32) << 8) | (color.r as u32)
}
