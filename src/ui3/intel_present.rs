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
}

pub(super) fn present_ui3_frame_to_intel_primary(
    frame: &Ui3GeometryFrame,
) -> Ui3IntelPresentSummary {
    let mut summary = Ui3IntelPresentSummary::default();
    let mut rects = Vec::new();

    for draw in &frame.draws {
        match draw {
            Ui3LoweredDraw::SolidRect { rect, color, .. } => {
                if let Some(rect) = lower_rect(*rect) {
                    rects.push(crate::intel::gpgpu::GpgpuSolidRect {
                        rect,
                        color_rgba: rgba8_to_kernel_rgba(*color),
                    });
                }
            }
            Ui3LoweredDraw::Mesh { .. } => {
                summary.mesh_draws = summary.mesh_draws.saturating_add(1);
            }
            Ui3LoweredDraw::TextRun { .. } => {
                summary.text_runs = summary.text_runs.saturating_add(1);
            }
        }
    }

    summary.solid_rects = rects.len();
    if rects.is_empty() {
        return summary;
    }

    if let Some(result) = crate::intel::gpgpu::solid_rects_rgba8_over_primary(&rects, true) {
        summary.presented = result.ok && result.presented;
        summary.fill_descs = result.fill_descs;
        summary.fill_submits = result.fill_submits;
        summary.blend_descs = result.blend_descs;
        summary.blend_submits = result.blend_submits;
        summary.present_ms = result.present_ms;
        summary.total_ms = result.total_ms;
    }

    summary
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
