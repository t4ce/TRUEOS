use alloc::vec::Vec;

use libm::{ceilf, floorf};
use trueos_gfx_core::{Rgba8, TEX_VERTEX_SIZE, ViewTransform, push_tex_quad_px};

use super::{Ui3GeometryFrame, Ui3LoweredDraw, Ui3Rect};

#[derive(Copy, Clone, Debug, Default)]
pub(super) struct Ui3IntelPresentSummary {
    pub solid_rects: usize,
    pub mesh_draws: usize,
    pub texture_draws: usize,
    pub text_runs: usize,
    pub presented: bool,
    pub fill_descs: usize,
    pub fill_submits: usize,
    pub blend_descs: usize,
    pub blend_submits: usize,
    pub present_ms: u64,
    pub total_ms: u64,
    pub rect_ms: u64,
    pub mesh_ms: u64,
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
            Ui3LoweredDraw::SolidRect {
                rect, color, clip, ..
            } => {
                if let Some(rect) = clipped_rect(*rect, *clip).and_then(lower_rect) {
                    let color_rgba = rgba8_to_kernel_rgba(*color);
                    let opaque = kernel_rgba_alpha(color_rgba) == 0xff;
                    if !rects.is_empty() && opaque != rect_run_opaque {
                        flush_rect_run(&mut rects, &mut summary);
                    }
                    rect_run_opaque = opaque;
                    rects.push(crate::intel::gpgpu::GpgpuSolidRect { rect, color_rgba });
                }
            }
            Ui3LoweredDraw::Mesh {
                vertices,
                indices,
                clip,
                ..
            } => {
                if let Some(clip) = clip
                    && !mesh_intersects_clip(vertices, *clip)
                {
                    continue;
                }
                flush_rect_run(&mut rects, &mut summary);
                draw_rgb_mesh(vertices, indices, &mut summary);
            }
            Ui3LoweredDraw::TextureRect {
                tex_id,
                rect,
                alpha,
                clip,
                ..
            } => {
                flush_rect_run(&mut rects, &mut summary);
                draw_texture_rect(*tex_id, *rect, *alpha, *clip, &mut summary);
            }
            Ui3LoweredDraw::TextRun {
                origin,
                text,
                color,
                clip,
                ..
            } => {
                if !text_intersects_clip(*origin, text.as_str(), *clip) {
                    continue;
                }
                flush_rect_run(&mut rects, &mut summary);
                summary.text_runs = summary.text_runs.saturating_add(1);
                draw_sprite64_text_run(*origin, text.as_str(), *color, &mut summary);
            }
        }
    }

    flush_rect_run(&mut rects, &mut summary);
    publish_frame(&mut summary);

    summary
}

fn draw_texture_rect(
    tex_id: u32,
    rect: super::Ui3Rect,
    alpha: f32,
    clip: Option<super::Ui3Rect>,
    summary: &mut Ui3IntelPresentSummary,
) {
    if tex_id == 0 || rect.w <= 0.0 || rect.h <= 0.0 {
        return;
    }
    let Some(dst_rect) = clipped_rect(rect, clip) else {
        return;
    };
    if crate::r::io::cabi::trueos_cabi_gfx_texture_status(tex_id) != 2 {
        return;
    }

    let (view_w, view_h) = crate::intel::active_scanout_dimensions()
        .map(|(w, h)| (w.max(1), h.max(1)))
        .unwrap_or((1920, 1080));
    let alpha_u8 = (alpha.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    let u0 = ((dst_rect.x - rect.x) / rect.w).clamp(0.0, 1.0);
    let v0 = ((dst_rect.y - rect.y) / rect.h).clamp(0.0, 1.0);
    let u1 = ((dst_rect.x + dst_rect.w - rect.x) / rect.w).clamp(0.0, 1.0);
    let v1 = ((dst_rect.y + dst_rect.h - rect.y) / rect.h).clamp(0.0, 1.0);
    let mut verts = Vec::with_capacity(6 * TEX_VERTEX_SIZE);
    push_tex_quad_px(
        &mut verts,
        ViewTransform::from_extent(view_w, view_h),
        dst_rect.x,
        dst_rect.y,
        dst_rect.x + dst_rect.w,
        dst_rect.y + dst_rect.h,
        [u0, v0, u1, v1],
        Rgba8::new(255, 255, 255, alpha_u8),
    );
    let begin_rc = unsafe { crate::r::io::cabi::trueos_cabi_gfx_begin_frame_preserve(0) };
    if begin_rc != 0 {
        return;
    }
    let _ = unsafe { crate::r::io::cabi::trueos_cabi_gfx_set_sampler(0, 0, 0, 0) };
    let _ = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_set_blend(
            if alpha_u8 < 255 { 1 } else { 0 },
            0x0302,
            0x0303,
            0x0302,
            0x0303,
            0,
            0,
        )
    };
    let rc = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_draw_tex_triangles_no_present(
            tex_id,
            verts.as_ptr(),
            verts.len(),
        )
    };
    let _ = unsafe { crate::r::io::cabi::trueos_cabi_gfx_set_blend(0, 1, 0, 1, 0, 0, 0) };
    let end_rc = unsafe { crate::r::io::cabi::trueos_cabi_gfx_end_frame() };
    if rc == 0 && end_rc == 0 {
        summary.texture_draws = summary.texture_draws.saturating_add(1);
        summary.primary_dirty = true;
    }
}

fn draw_rgb_mesh(
    vertices: &[trueos_gfx_core::RgbVertexPx],
    indices: &[u16],
    summary: &mut Ui3IntelPresentSummary,
) {
    if vertices.is_empty() || indices.len() < 3 {
        return;
    }

    summary.mesh_draws = summary.mesh_draws.saturating_add(1);
    let mesh = crate::intel::gpgpu::GpgpuRgbMesh {
        vertices: vertices.to_vec(),
        indices: indices.to_vec(),
    };
    let Some(result) = crate::intel::gpgpu::cpu_rgb_meshes_rgba8_over_primary(&[mesh]) else {
        return;
    };
    summary.primary_dirty |= result.ok && result.pixels > 0;
    summary.total_ms = summary.total_ms.saturating_add(result.total_ms);
    summary.mesh_ms = summary.mesh_ms.saturating_add(result.total_ms);
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

fn draw_sprite64_text_run(
    origin: super::Ui3Point,
    text: &str,
    color: Rgba8,
    summary: &mut Ui3IntelPresentSummary,
) {
    let batches =
        super::font::gpgpu_font::collect_ui3_text_run_sprite64_batches(origin, text, color);
    for batch in batches {
        let Some(result) = crate::intel::gpgpu::sprite64_worklist_primary(
            batch.placements.as_slice(),
            false,
            "ui3-font-sprite64-worklist",
        ) else {
            continue;
        };
        if result.submitted && result.descriptors > 0 {
            summary.primary_dirty = true;
        }
        summary.sprite_ms = summary.sprite_ms.saturating_add(result.total_ms);
        summary.total_ms = summary.total_ms.saturating_add(result.total_ms);
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

fn clipped_rect(rect: Ui3Rect, clip: Option<Ui3Rect>) -> Option<Ui3Rect> {
    match clip {
        Some(clip) => intersect_rect(rect, clip),
        None => Some(rect),
    }
}

fn intersect_rect(a: Ui3Rect, b: Ui3Rect) -> Option<Ui3Rect> {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = (a.x + a.w).min(b.x + b.w);
    let y1 = (a.y + a.h).min(b.y + b.h);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some(Ui3Rect {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    })
}

fn mesh_intersects_clip(vertices: &[trueos_gfx_core::RgbVertexPx], clip: Ui3Rect) -> bool {
    let Some(bounds) = mesh_bounds(vertices) else {
        return false;
    };
    intersect_rect(bounds, clip).is_some()
}

fn mesh_bounds(vertices: &[trueos_gfx_core::RgbVertexPx]) -> Option<Ui3Rect> {
    let first = vertices.first()?;
    let mut x0 = first.x;
    let mut y0 = first.y;
    let mut x1 = first.x;
    let mut y1 = first.y;
    for vertex in vertices.iter().skip(1) {
        x0 = x0.min(vertex.x);
        y0 = y0.min(vertex.y);
        x1 = x1.max(vertex.x);
        y1 = y1.max(vertex.y);
    }
    Some(Ui3Rect {
        x: x0,
        y: y0,
        w: (x1 - x0).max(0.01),
        h: (y1 - y0).max(0.01),
    })
}

fn text_intersects_clip(origin: super::Ui3Point, text: &str, clip: Option<Ui3Rect>) -> bool {
    let Some(clip) = clip else {
        return true;
    };
    let rect = Ui3Rect {
        x: origin.x,
        y: origin.y,
        w: (text.len() as f32 * 9.0).max(1.0),
        h: 16.0,
    };
    intersect_rect(rect, clip).is_some()
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
