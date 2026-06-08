use alloc::vec::Vec;

use libm::{ceilf, floorf};
use trueos_gfx_core::Rgba8;

use super::{Ui3GeometryFrame, Ui3LoweredDraw, Ui3Rect};

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
    pub rect_ms: u64,
    pub sprite_ms: u64,
    pub publish_ms: u64,
    primary_dirty: bool,
}

pub(super) fn present_ui3_frame_to_intel_primary(
    frame: &Ui3GeometryFrame,
) -> Ui3IntelPresentSummary {
    let mut summary = Ui3IntelPresentSummary::default();
    let mut rects = Vec::new();
    let mut rect_run_opaque = true;

    for draw in &frame.draws {
        match draw {
            Ui3LoweredDraw::SolidRect { rect, color, .. } => {
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
            Ui3LoweredDraw::TextRun {
                origin,
                text,
                color,
                ..
            } => {
                flush_rect_run(&mut rects, &mut summary);
                summary.text_runs = summary.text_runs.saturating_add(1);
                draw_font_text_run(origin.x, origin.y, text.as_str(), *color, &mut summary);
            }
        }
    }

    flush_rect_run(&mut rects, &mut summary);
    publish_frame(&mut summary);

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
    let result = crate::intel::gpgpu::cpu_solid_rects_rgba8_over_primary(rects)
        .or_else(|| crate::intel::gpgpu::solid_rects_rgba8_over_primary(rects, false));
    if let Some(result) = result {
        accumulate_rect_result(summary, result);
    }
    rects.clear();
}

fn accumulate_rect_result(
    summary: &mut Ui3IntelPresentSummary,
    result: crate::intel::gpgpu::GpgpuSolidRectOverlayResult,
) {
    summary.primary_dirty |= result.ok && result.rects > 0;
    summary.fill_descs = summary.fill_descs.saturating_add(result.fill_descs);
    summary.fill_submits = summary.fill_submits.saturating_add(result.fill_submits);
    summary.blend_descs = summary.blend_descs.saturating_add(result.blend_descs);
    summary.blend_submits = summary.blend_submits.saturating_add(result.blend_submits);
    summary.present_ms = summary.present_ms.saturating_add(result.present_ms);
    summary.total_ms = summary.total_ms.saturating_add(result.total_ms);
    summary.rect_ms = summary.rect_ms.saturating_add(result.total_ms);
}

fn draw_font_text_run(
    x: f32,
    y: f32,
    text: &str,
    color: Rgba8,
    summary: &mut Ui3IntelPresentSummary,
) {
    if text.is_empty() || !x.is_finite() || !y.is_finite() {
        return;
    }

    let begin_rc =
        unsafe { crate::r::io::cabi::trueos_cabi_gfx_begin_frame_preserve_no_present(0) };
    if begin_rc != 0 {
        return;
    }

    let (view_w, view_h) = crate::intel::active_scanout_dimensions().unwrap_or((2560, 1440));
    let max_width_px = if x < view_w as f32 {
        (view_w as f32 - x.max(0.0)).max(1.0)
    } else {
        view_w as f32
    };
    let drew = crate::r::ui2::ui2_font_draw_text_line_with_tier_rgba_no_present(
        text,
        x,
        y,
        max_width_px,
        crate::r::ui2::Ui2FontTier::OneX,
        view_w,
        view_h,
        (color.r, color.g, color.b, color.a),
    );
    let _ = unsafe { crate::r::io::cabi::trueos_cabi_gfx_end_frame() };
    if drew {
        summary.primary_dirty = true;
    }
}

fn publish_frame(summary: &mut Ui3IntelPresentSummary) {
    if !summary.primary_dirty {
        return;
    }

    if let Some(present_ms) = crate::intel::gpgpu::present_primary_external_write("ui3-frame") {
        summary.presented = true;
        summary.present_ms = summary.present_ms.saturating_add(present_ms);
        summary.total_ms = summary.total_ms.saturating_add(present_ms);
        summary.publish_ms = summary.publish_ms.saturating_add(present_ms);
    }
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
