use alloc::vec::Vec;

use libm::{ceilf, floorf};
use trueos_gfx_core::Rgba8;

use super::{Ui3GeometryFrame, Ui3LoweredDraw, Ui3Rect};

const UI3_SMILEY_TEXT_ADVANCE_PX: f32 = 64.0;

#[derive(Copy, Clone, Debug, Default)]
pub(super) struct Ui3IntelPresentSummary {
    pub solid_rects: usize,
    pub mesh_draws: usize,
    pub text_runs: usize,
    pub presented: bool,
    pub fill_descs: usize,
    pub fill_submits: usize,
    pub blend_descs: usize,
    pub blend_submits: usize,
    pub present_ms: u64,
    pub total_ms: u64,
}

pub(super) fn present_ui3_frame_to_intel_primary(
    frame: &Ui3GeometryFrame,
) -> Ui3IntelPresentSummary {
    let mut summary = Ui3IntelPresentSummary::default();
    let mut rects = Vec::new();
    let mut rect_run_opaque = true;
    let mut smileys = Vec::new();

    for draw in &frame.draws {
        match draw {
            Ui3LoweredDraw::SolidRect { rect, color, .. } => {
                flush_smiley_run(&mut smileys, &mut summary);
                if let Some(rect) = lower_rect(*rect) {
                    let color_rgba = rgba8_to_kernel_rgba(*color);
                    let opaque = kernel_rgba_alpha(color_rgba) == 0xff;
                    if !rects.is_empty() && opaque != rect_run_opaque {
                        flush_rect_run(&mut rects, &mut summary);
                    }
                    rect_run_opaque = opaque;
                    rects.push(crate::intel::gpgpu::GpgpuSolidRect { rect, color_rgba });
                }
            }
            Ui3LoweredDraw::Mesh { .. } => {
                flush_rect_run(&mut rects, &mut summary);
                summary.mesh_draws = summary.mesh_draws.saturating_add(1);
            }
            Ui3LoweredDraw::TextRun { origin, text, .. } => {
                flush_rect_run(&mut rects, &mut summary);
                summary.text_runs = summary.text_runs.saturating_add(1);
                push_smiley_text_run(&mut smileys, origin.x, origin.y, text.as_str());
            }
        }
    }

    flush_rect_run(&mut rects, &mut summary);
    flush_smiley_run(&mut smileys, &mut summary);

    summary
}

fn flush_rect_run(
    rects: &mut Vec<crate::intel::gpgpu::GpgpuSolidRect>,
    summary: &mut Ui3IntelPresentSummary,
) {
    if rects.is_empty() {
        return;
    }

    summary.solid_rects = summary.solid_rects.saturating_add(rects.len());
    if let Some(result) = crate::intel::gpgpu::solid_rects_rgba8_over_primary(rects, true) {
        summary.presented |= result.ok && result.presented;
        summary.fill_descs = summary.fill_descs.saturating_add(result.fill_descs);
        summary.fill_submits = summary.fill_submits.saturating_add(result.fill_submits);
        summary.blend_descs = summary.blend_descs.saturating_add(result.blend_descs);
        summary.blend_submits = summary.blend_submits.saturating_add(result.blend_submits);
        summary.present_ms = summary.present_ms.saturating_add(result.present_ms);
        summary.total_ms = summary.total_ms.saturating_add(result.total_ms);
    }
    rects.clear();
}

fn flush_smiley_run(
    placements: &mut Vec<crate::intel::gpgpu::GpgpuTwemojiSprite64Placement>,
    summary: &mut Ui3IntelPresentSummary,
) {
    if placements.is_empty() {
        return;
    }

    if let Some(result) = crate::intel::gpgpu::twemoji_sprite64_worklist_primary(placements, true) {
        summary.presented |= result.ok && result.presented;
        summary.total_ms = summary.total_ms.saturating_add(result.total_ms);
        summary.present_ms = summary.present_ms.saturating_add(result.present_ms);
    }
    placements.clear();
}

fn push_smiley_text_run(
    out: &mut Vec<crate::intel::gpgpu::GpgpuTwemojiSprite64Placement>,
    x: f32,
    y: f32,
    text: &str,
) {
    let slots = crate::gfx::althlasfont::twemoji::twemoji_slot_count().max(1);
    let mut pen_x = x;
    for byte in text.bytes() {
        if matches!(byte, b' ' | b'\t' | b'\n' | b'\r') {
            pen_x += UI3_SMILEY_TEXT_ADVANCE_PX;
            continue;
        }
        push_smiley_slot(out, u16::from(byte) % slots, pen_x, y);
        pen_x += UI3_SMILEY_TEXT_ADVANCE_PX;
    }
}

fn push_smiley_slot(
    out: &mut Vec<crate::intel::gpgpu::GpgpuTwemojiSprite64Placement>,
    slot: u16,
    x: f32,
    y: f32,
) {
    out.push(crate::intel::gpgpu::GpgpuTwemojiSprite64Placement {
        slot,
        dst_x: libm::roundf(x) as i32,
        dst_y: libm::roundf(y) as i32,
    });
}

fn lower_rect(rect: Ui3Rect) -> Option<crate::intel::gpgpu::GpgpuRect> {
    if !rect.x.is_finite()
        || !rect.y.is_finite()
        || !rect.w.is_finite()
        || !rect.h.is_finite()
        || rect.w <= 0.0
        || rect.h <= 0.0
    {
        return None;
    }

    let x0 = floorf(rect.x);
    let y0 = floorf(rect.y);
    let x1 = ceilf(rect.x + rect.w);
    let y1 = ceilf(rect.y + rect.h);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }

    let x = x0.max(i32::MIN as f32).min(i32::MAX as f32) as i32;
    let y = y0.max(i32::MIN as f32).min(i32::MAX as f32) as i32;
    let width = (x1 - x0).max(1.0).min(u32::MAX as f32) as u32;
    let height = (y1 - y0).max(1.0).min(u32::MAX as f32) as u32;

    Some(crate::intel::gpgpu::GpgpuRect::new(x, y, width, height))
}

#[inline]
fn rgba8_to_kernel_rgba(color: Rgba8) -> u32 {
    ((color.a as u32) << 24) | ((color.b as u32) << 16) | ((color.g as u32) << 8) | (color.r as u32)
}

#[inline]
fn kernel_rgba_alpha(color_rgba: u32) -> u8 {
    (color_rgba >> 24) as u8
}
