use alloc::{string::String, vec::Vec};
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use sha2::{Digest, Sha256};
use spin::{Mutex, Once};
pub(crate) const COPY_RECT_RGBA8_KERNEL_NAME: &str = "copy_rect_rgba8";
pub(crate) const COPY_RECT_RGBA8_OPENCL_SOURCE: &str = include_str!("kernels/copy_rect_rgba8.cl");
pub(crate) const RESOLVE_TILE64_MSAA4_RGBA8_KERNEL_NAME: &str = "resolve_tile64_msaa4_rgba8";
pub(crate) const RESOLVE_TILE64_MSAA4_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/resolve_tile64_msaa4_rgba8.cl");
pub(crate) const FILL_RECT_RGBA8_KERNEL_NAME: &str = "fill_rect_rgba8";
pub(crate) const FILL_RECT_RGBA8_OPENCL_SOURCE: &str = include_str!("kernels/fill_rect_rgba8.cl");
pub(crate) const FILL_RECT_WORKLIST_RGBA8_KERNEL_NAME: &str = "fill_rect_worklist_rgba8";
pub(crate) const FILL_RECT_WORKLIST_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/fill_rect_worklist_rgba8.cl");
pub(crate) const GRADIENT_RECT_WORKLIST_RGBA8_KERNEL_NAME: &str = "gradient_rect_worklist_rgba8";
pub(crate) const GRADIENT_RECT_WORKLIST_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/gradient_rect_worklist_rgba8.cl");
pub(crate) const FILL_CIRCLE_RGBA8_KERNEL_NAME: &str = "fill_circle_rgba8";
pub(crate) const FILL_CIRCLE_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/fill_circle_rgba8.cl");
pub(crate) const ALPHA_BLEND_RGBA8_OVER_KERNEL_NAME: &str = "alpha_blend_rgba8_over";
pub(crate) const ALPHA_BLEND_RGBA8_OVER_OPENCL_SOURCE: &str =
    include_str!("kernels/alpha_blend_rgba8_over.cl");
pub(crate) const ALPHA_BLEND_WORKLIST_RGBA8_KERNEL_NAME: &str = "alpha_blend_worklist_rgba8";
pub(crate) const ALPHA_BLEND_WORKLIST_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/alpha_blend_worklist_rgba8.cl");
pub(crate) const GLYPH_MASK_RGBA8_KERNEL_NAME: &str = "glyph_mask_rgba8";
pub(crate) const GLYPH_MASK_RGBA8_OPENCL_SOURCE: &str = include_str!("kernels/glyph_mask_rgba8.cl");
pub(crate) const PRESENT_RGBA8_TO_PRIMARY_XRGB_RECT_KERNEL_NAME: &str =
    "present_rgba8_to_primary_xrgb_rect";
pub(crate) const PRESENT_RGBA8_TO_PRIMARY_XRGB_RECT_OPENCL_SOURCE: &str =
    include_str!("kernels/present_rgba8_to_primary_xrgb_rect.cl");
pub(crate) const UI4_NV12_YTILE_TO_PRIMARY_XRGB_KERNEL_NAME: &str =
    "ui4_nv12_ytile_to_primary_xrgb";
pub(crate) const UI4_NV12_YTILE_TO_PRIMARY_XRGB_OPENCL_SOURCE: &str =
    include_str!("kernels/ui4_nv12_ytile_to_primary_xrgb.cl");
pub(crate) const UI4_NV12_TILE64_TO_RGBA8_FRAME_KERNEL_NAME: &str =
    "ui4_nv12_tile64_to_rgba8_frame";
pub(crate) const UI4_NV12_TILE64_TO_RGBA8_FRAME_OPENCL_SOURCE: &str =
    include_str!("kernels/ui4_nv12_tile64_to_rgba8_frame.cl");
pub(crate) const STAMP_MANDEL_RGBA8_KERNEL_NAME: &str = "stamp_mandel_rgba8";
pub(crate) const STAMP_MANDEL_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/stamp_mandel_rgba8.cl");
pub(crate) const SPRITE64_WORKLIST_RGBA8_KERNEL_NAME: &str = "sprite64_worklist_rgba8";
pub(crate) const SPRITE64_WORKLIST_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/sprite64_worklist_rgba8.cl");
pub(crate) const SPRITE_QUAD_WORKLIST_RGBA8_KERNEL_NAME: &str = "sprite_quad_worklist_rgba8";
pub(crate) const SPRITE_QUAD_WORKLIST_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/sprite_quad_worklist_rgba8.cl");
pub(crate) const UI4_COMPOSE_LAYERS_RGBA8_KERNEL_NAME: &str = "ui4_compose_layers_rgba8";
pub(crate) const UI4_COMPOSE_LAYERS_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/ui4_compose_layers_rgba8.cl");
pub(crate) const MANDEL64_WORKLIST_RGBA8_KERNEL_NAME: &str = "mandel64_worklist_rgba8";
pub(crate) const MANDEL64_WORKLIST_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/mandel64_worklist_rgba8.cl");
pub(crate) const CANVAS3D_PROJECT_RGBA8_KERNEL_NAME: &str = "canvas3d_project_rgba8";
pub(crate) const CANVAS3D_PROJECT_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/canvas3d_project_rgba8.cl");
pub(crate) const CANVAS3D_TRANSFORM_Q16_KERNEL_NAME: &str = "canvas3d_transform_q16";
pub(crate) const CANVAS3D_TRANSFORM_Q16_OPENCL_SOURCE: &str =
    include_str!("kernels/canvas3d_transform_q16.cl");
pub(crate) const CANVAS3D_CLIP_BOX_Q16_KERNEL_NAME: &str = "canvas3d_clip_box_q16";
pub(crate) const CANVAS3D_CLIP_BOX_Q16_OPENCL_SOURCE: &str =
    include_str!("kernels/canvas3d_clip_box_q16.cl");
pub(crate) const CANVAS3D_PLANE_SAMPLE_RGBA8_KERNEL_NAME: &str = "canvas3d_plane_sample_rgba8";
pub(crate) const CANVAS3D_PLANE_SAMPLE_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/canvas3d_plane_sample_rgba8.cl");
pub(crate) const CANVAS3D_PLANE_FILL_RGBA8_KERNEL_NAME: &str = "canvas3d_plane_fill_rgba8";
pub(crate) const CANVAS3D_PLANE_FILL_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/canvas3d_plane_fill_rgba8.cl");
pub(crate) const CANVAS3D_PLANE_PATCH_FILL_CUT_RGBA8_KERNEL_NAME: &str =
    "canvas3d_plane_patch_fill_cut_rgba8";
pub(crate) const CANVAS3D_PLANE_PATCH_FILL_CUT_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/canvas3d_plane_patch_fill_cut_rgba8.cl");
pub(crate) const CANVAS3D_PLANE_PATCH_WORKLIST_RGBA8_KERNEL_NAME: &str =
    "canvas3d_plane_patch_worklist_rgba8";
pub(crate) const CANVAS3D_PLANE_PATCH_WORKLIST_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/canvas3d_plane_patch_worklist_rgba8.cl");
pub(crate) const SKYBOX_SAMPLE_RGB565_KERNEL_NAME: &str = "skybox_sample_rgb565";
pub(crate) const SKYBOX_SAMPLE_RGB565_OPENCL_SOURCE: &str =
    include_str!("kernels/skybox_sample_rgb565.cl");
pub(crate) const CHART_SINE_RGBA8_KERNEL_NAME: &str = "chart_sine_rgba8";
pub(crate) const CHART_SINE_RGBA8_OPENCL_SOURCE: &str = include_str!("kernels/chart_sine_rgba8.cl");
pub(crate) const PIXEL_PLASMA_RGBA8_KERNEL_NAME: &str = "pixel_plasma_rgba8";
pub(crate) const PIXEL_PLASMA_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/pixel_plasma_rgba8.cl");
pub(crate) const FONT_OUTLINE_MESH_KERNEL_NAME: &str = "font_outline_mesh";
pub(crate) const FONT_OUTLINE_MESH_OPENCL_SOURCE: &str =
    include_str!("kernels/font_outline_mesh.cl");
pub(crate) const FONT_OUTLINE_COVERAGE_R8_KERNEL_NAME: &str = "font_outline_coverage_r8";
pub(crate) const FONT_OUTLINE_COVERAGE_R8_OPENCL_SOURCE: &str =
    include_str!("kernels/font_outline_coverage_r8.cl");

pub(crate) fn kernel_opencl_source(name: &str) -> Option<&'static str> {
    match name {
        COPY_RECT_RGBA8_KERNEL_NAME => Some(COPY_RECT_RGBA8_OPENCL_SOURCE),
        RESOLVE_TILE64_MSAA4_RGBA8_KERNEL_NAME => Some(RESOLVE_TILE64_MSAA4_RGBA8_OPENCL_SOURCE),
        FILL_RECT_RGBA8_KERNEL_NAME => Some(FILL_RECT_RGBA8_OPENCL_SOURCE),
        FILL_RECT_WORKLIST_RGBA8_KERNEL_NAME => Some(FILL_RECT_WORKLIST_RGBA8_OPENCL_SOURCE),
        GRADIENT_RECT_WORKLIST_RGBA8_KERNEL_NAME => {
            Some(GRADIENT_RECT_WORKLIST_RGBA8_OPENCL_SOURCE)
        }
        FILL_CIRCLE_RGBA8_KERNEL_NAME => Some(FILL_CIRCLE_RGBA8_OPENCL_SOURCE),
        ALPHA_BLEND_RGBA8_OVER_KERNEL_NAME => Some(ALPHA_BLEND_RGBA8_OVER_OPENCL_SOURCE),
        ALPHA_BLEND_WORKLIST_RGBA8_KERNEL_NAME => Some(ALPHA_BLEND_WORKLIST_RGBA8_OPENCL_SOURCE),
        GLYPH_MASK_RGBA8_KERNEL_NAME => Some(GLYPH_MASK_RGBA8_OPENCL_SOURCE),
        PRESENT_RGBA8_TO_PRIMARY_XRGB_RECT_KERNEL_NAME => {
            Some(PRESENT_RGBA8_TO_PRIMARY_XRGB_RECT_OPENCL_SOURCE)
        }
        UI4_NV12_YTILE_TO_PRIMARY_XRGB_KERNEL_NAME => {
            Some(UI4_NV12_YTILE_TO_PRIMARY_XRGB_OPENCL_SOURCE)
        }
        UI4_NV12_TILE64_TO_RGBA8_FRAME_KERNEL_NAME => {
            Some(UI4_NV12_TILE64_TO_RGBA8_FRAME_OPENCL_SOURCE)
        }
        STAMP_MANDEL_RGBA8_KERNEL_NAME => Some(STAMP_MANDEL_RGBA8_OPENCL_SOURCE),
        SPRITE64_WORKLIST_RGBA8_KERNEL_NAME => Some(SPRITE64_WORKLIST_RGBA8_OPENCL_SOURCE),
        SPRITE_QUAD_WORKLIST_RGBA8_KERNEL_NAME => Some(SPRITE_QUAD_WORKLIST_RGBA8_OPENCL_SOURCE),
        UI4_COMPOSE_LAYERS_RGBA8_KERNEL_NAME => Some(UI4_COMPOSE_LAYERS_RGBA8_OPENCL_SOURCE),
        MANDEL64_WORKLIST_RGBA8_KERNEL_NAME => Some(MANDEL64_WORKLIST_RGBA8_OPENCL_SOURCE),
        CANVAS3D_PROJECT_RGBA8_KERNEL_NAME => Some(CANVAS3D_PROJECT_RGBA8_OPENCL_SOURCE),
        CANVAS3D_TRANSFORM_Q16_KERNEL_NAME => Some(CANVAS3D_TRANSFORM_Q16_OPENCL_SOURCE),
        CANVAS3D_CLIP_BOX_Q16_KERNEL_NAME => Some(CANVAS3D_CLIP_BOX_Q16_OPENCL_SOURCE),
        CANVAS3D_PLANE_SAMPLE_RGBA8_KERNEL_NAME => Some(CANVAS3D_PLANE_SAMPLE_RGBA8_OPENCL_SOURCE),
        CANVAS3D_PLANE_FILL_RGBA8_KERNEL_NAME => Some(CANVAS3D_PLANE_FILL_RGBA8_OPENCL_SOURCE),
        CANVAS3D_PLANE_PATCH_FILL_CUT_RGBA8_KERNEL_NAME => {
            Some(CANVAS3D_PLANE_PATCH_FILL_CUT_RGBA8_OPENCL_SOURCE)
        }
        CANVAS3D_PLANE_PATCH_WORKLIST_RGBA8_KERNEL_NAME => {
            Some(CANVAS3D_PLANE_PATCH_WORKLIST_RGBA8_OPENCL_SOURCE)
        }
        SKYBOX_SAMPLE_RGB565_KERNEL_NAME => Some(SKYBOX_SAMPLE_RGB565_OPENCL_SOURCE),
        CHART_SINE_RGBA8_KERNEL_NAME => Some(CHART_SINE_RGBA8_OPENCL_SOURCE),
        PIXEL_PLASMA_RGBA8_KERNEL_NAME => Some(PIXEL_PLASMA_RGBA8_OPENCL_SOURCE),
        FONT_OUTLINE_MESH_KERNEL_NAME => Some(FONT_OUTLINE_MESH_OPENCL_SOURCE),
        FONT_OUTLINE_COVERAGE_R8_KERNEL_NAME => Some(FONT_OUTLINE_COVERAGE_R8_OPENCL_SOURCE),
        _ => None,
    }
}

pub(crate) fn kernel_source_path(name: &str) -> Option<&'static str> {
    match name {
        COPY_RECT_RGBA8_KERNEL_NAME => Some("src/intel/gpgpu/kernels/copy_rect_rgba8.cl"),
        RESOLVE_TILE64_MSAA4_RGBA8_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/resolve_tile64_msaa4_rgba8.cl")
        }
        FILL_RECT_RGBA8_KERNEL_NAME => Some("src/intel/gpgpu/kernels/fill_rect_rgba8.cl"),
        FILL_RECT_WORKLIST_RGBA8_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/fill_rect_worklist_rgba8.cl")
        }
        GRADIENT_RECT_WORKLIST_RGBA8_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/gradient_rect_worklist_rgba8.cl")
        }
        FILL_CIRCLE_RGBA8_KERNEL_NAME => Some("src/intel/gpgpu/kernels/fill_circle_rgba8.cl"),
        ALPHA_BLEND_RGBA8_OVER_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/alpha_blend_rgba8_over.cl")
        }
        ALPHA_BLEND_WORKLIST_RGBA8_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/alpha_blend_worklist_rgba8.cl")
        }
        GLYPH_MASK_RGBA8_KERNEL_NAME => Some("src/intel/gpgpu/kernels/glyph_mask_rgba8.cl"),
        PRESENT_RGBA8_TO_PRIMARY_XRGB_RECT_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/present_rgba8_to_primary_xrgb_rect.cl")
        }
        UI4_NV12_YTILE_TO_PRIMARY_XRGB_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/ui4_nv12_ytile_to_primary_xrgb.cl")
        }
        UI4_NV12_TILE64_TO_RGBA8_FRAME_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/ui4_nv12_tile64_to_rgba8_frame.cl")
        }
        STAMP_MANDEL_RGBA8_KERNEL_NAME => Some("src/intel/gpgpu/kernels/stamp_mandel_rgba8.cl"),
        SPRITE64_WORKLIST_RGBA8_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/sprite64_worklist_rgba8.cl")
        }
        SPRITE_QUAD_WORKLIST_RGBA8_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/sprite_quad_worklist_rgba8.cl")
        }
        UI4_COMPOSE_LAYERS_RGBA8_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/ui4_compose_layers_rgba8.cl")
        }
        MANDEL64_WORKLIST_RGBA8_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/mandel64_worklist_rgba8.cl")
        }
        CANVAS3D_PROJECT_RGBA8_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/canvas3d_project_rgba8.cl")
        }
        CANVAS3D_TRANSFORM_Q16_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/canvas3d_transform_q16.cl")
        }
        CANVAS3D_CLIP_BOX_Q16_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/canvas3d_clip_box_q16.cl")
        }
        CANVAS3D_PLANE_SAMPLE_RGBA8_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/canvas3d_plane_sample_rgba8.cl")
        }
        CANVAS3D_PLANE_FILL_RGBA8_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/canvas3d_plane_fill_rgba8.cl")
        }
        CANVAS3D_PLANE_PATCH_FILL_CUT_RGBA8_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/canvas3d_plane_patch_fill_cut_rgba8.cl")
        }
        CANVAS3D_PLANE_PATCH_WORKLIST_RGBA8_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/canvas3d_plane_patch_worklist_rgba8.cl")
        }
        SKYBOX_SAMPLE_RGB565_KERNEL_NAME => Some("src/intel/gpgpu/kernels/skybox_sample_rgb565.cl"),
        CHART_SINE_RGBA8_KERNEL_NAME => Some("src/intel/gpgpu/kernels/chart_sine_rgba8.cl"),
        PIXEL_PLASMA_RGBA8_KERNEL_NAME => Some("src/intel/gpgpu/kernels/pixel_plasma_rgba8.cl"),
        FONT_OUTLINE_MESH_KERNEL_NAME => Some("src/intel/gpgpu/kernels/font_outline_mesh.cl"),
        FONT_OUTLINE_COVERAGE_R8_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/font_outline_coverage_r8.cl")
        }
        _ => None,
    }
}

pub(crate) const COPY_RECT_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/copy_rect_rgba8.bin");
pub(crate) const COPY_RECT_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/copy_rect_rgba8.spv");
pub(crate) const RESOLVE_TILE64_MSAA4_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/resolve_tile64_msaa4_rgba8.bin");
pub(crate) const RESOLVE_TILE64_MSAA4_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/resolve_tile64_msaa4_rgba8.spv");
pub(crate) const FILL_RECT_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/fill_rect_rgba8.bin");
pub(crate) const FILL_RECT_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/fill_rect_rgba8.spv");
pub(crate) const FILL_RECT_WORKLIST_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/fill_rect_worklist_rgba8.bin");
pub(crate) const FILL_RECT_WORKLIST_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/fill_rect_worklist_rgba8.spv");
pub(crate) const GRADIENT_RECT_WORKLIST_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/gradient_rect_worklist_rgba8.bin");
pub(crate) const GRADIENT_RECT_WORKLIST_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/gradient_rect_worklist_rgba8.spv");

pub(crate) const ALPHA_BLEND_WORKLIST_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/alpha_blend_worklist_rgba8.bin");
pub(crate) const ALPHA_BLEND_WORKLIST_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/alpha_blend_worklist_rgba8.spv");
pub(crate) const GLYPH_MASK_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/glyph_mask_rgba8.bin");
pub(crate) const GLYPH_MASK_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/glyph_mask_rgba8.spv");
pub(crate) const PRESENT_RGBA8_TO_PRIMARY_XRGB_RECT_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/present_rgba8_to_primary_xrgb_rect.bin");
pub(crate) const PRESENT_RGBA8_TO_PRIMARY_XRGB_RECT_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/present_rgba8_to_primary_xrgb_rect.spv");
pub(crate) const UI4_NV12_YTILE_TO_PRIMARY_XRGB_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/ui4_nv12_ytile_to_primary_xrgb.bin");
pub(crate) const UI4_NV12_YTILE_TO_PRIMARY_XRGB_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/ui4_nv12_ytile_to_primary_xrgb.spv");
pub(crate) const UI4_NV12_TILE64_TO_RGBA8_FRAME_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/ui4_nv12_tile64_to_rgba8_frame.bin");
pub(crate) const UI4_NV12_TILE64_TO_RGBA8_FRAME_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/ui4_nv12_tile64_to_rgba8_frame.spv");

pub(crate) const SPRITE64_WORKLIST_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/sprite64_worklist_rgba8.bin");
pub(crate) const SPRITE64_WORKLIST_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/sprite64_worklist_rgba8.spv");
pub(crate) const SPRITE_QUAD_WORKLIST_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/sprite_quad_worklist_rgba8.bin");
pub(crate) const SPRITE_QUAD_WORKLIST_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/sprite_quad_worklist_rgba8.spv");
pub(crate) const UI4_COMPOSE_LAYERS_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/ui4_compose_layers_rgba8.bin");
pub(crate) const UI4_COMPOSE_LAYERS_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/ui4_compose_layers_rgba8.spv");
pub(crate) const MANDEL64_WORKLIST_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/mandel64_worklist_rgba8.bin");
pub(crate) const MANDEL64_WORKLIST_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/mandel64_worklist_rgba8.spv");
pub(crate) const CANVAS3D_PROJECT_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/canvas3d_project_rgba8.bin");
pub(crate) const CANVAS3D_PROJECT_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/canvas3d_project_rgba8.spv");
pub(crate) const CANVAS3D_TRANSFORM_Q16_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/canvas3d_transform_q16.bin");
pub(crate) const CANVAS3D_TRANSFORM_Q16_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/canvas3d_transform_q16.spv");
pub(crate) const CANVAS3D_CLIP_BOX_Q16_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/canvas3d_clip_box_q16.bin");
pub(crate) const CANVAS3D_CLIP_BOX_Q16_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/canvas3d_clip_box_q16.spv");
pub(crate) const CANVAS3D_PLANE_SAMPLE_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/canvas3d_plane_sample_rgba8.bin");
pub(crate) const CANVAS3D_PLANE_SAMPLE_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/canvas3d_plane_sample_rgba8.spv");
pub(crate) const CANVAS3D_PLANE_FILL_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/canvas3d_plane_fill_rgba8.bin");
pub(crate) const CANVAS3D_PLANE_FILL_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/canvas3d_plane_fill_rgba8.spv");
pub(crate) const CANVAS3D_PLANE_PATCH_FILL_CUT_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/canvas3d_plane_patch_fill_cut_rgba8.bin");
pub(crate) const CANVAS3D_PLANE_PATCH_FILL_CUT_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/canvas3d_plane_patch_fill_cut_rgba8.spv");
pub(crate) const CANVAS3D_PLANE_PATCH_WORKLIST_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/canvas3d_plane_patch_worklist_rgba8.bin");
pub(crate) const CANVAS3D_PLANE_PATCH_WORKLIST_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/canvas3d_plane_patch_worklist_rgba8.spv");
pub(crate) const SKYBOX_SAMPLE_RGB565_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/skybox_sample_rgb565.bin");
pub(crate) const SKYBOX_SAMPLE_RGB565_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/skybox_sample_rgb565.spv");
pub(crate) const CHART_SINE_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/chart_sine_rgba8.bin");
pub(crate) const CHART_SINE_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/chart_sine_rgba8.spv");
pub(crate) const PIXEL_PLASMA_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/pixel_plasma_rgba8.bin");
pub(crate) const PIXEL_PLASMA_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/pixel_plasma_rgba8.spv");
pub(crate) const FONT_OUTLINE_MESH_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/font_outline_mesh.bin");
pub(crate) const FONT_OUTLINE_MESH_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/font_outline_mesh.spv");
pub(crate) const FONT_OUTLINE_COVERAGE_R8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/font_outline_coverage_r8.bin");
pub(crate) const FONT_OUTLINE_COVERAGE_R8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/font_outline_coverage_r8.spv");
pub(crate) const COPY_RECT_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0x10, 0x86, 0x60, 0x24, 0xAA, 0xFF, 0xAE, 0x96, 0xF9, 0x2C, 0xFC, 0x25, 0xA5, 0xFB, 0x18, 0x8C,
    0xA4, 0x21, 0x99, 0x47, 0x89, 0xAF, 0xBC, 0x4D, 0xBA, 0x3D, 0xDC, 0x29, 0x0B, 0xD5, 0x83, 0xAB,
];
pub(crate) const RESOLVE_TILE64_MSAA4_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0x89, 0xFA, 0xF1, 0x14, 0xBA, 0x35, 0x1D, 0xFE, 0x8C, 0x5A, 0xF8, 0x99, 0x66, 0x00, 0x26, 0xD4,
    0x66, 0xF1, 0x1D, 0xDF, 0x86, 0xF1, 0xE2, 0x8C, 0x0F, 0x18, 0x98, 0xCA, 0x9B, 0x5B, 0xD7, 0xDF,
];
pub(crate) const FILL_RECT_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0xAB, 0x51, 0x9A, 0x0E, 0x4E, 0x47, 0x31, 0xE5, 0x8F, 0xF6, 0x5D, 0x75, 0xBF, 0x92, 0x93, 0x4C,
    0xD7, 0x31, 0xA0, 0x88, 0x23, 0xB0, 0x40, 0x28, 0x62, 0x0E, 0x86, 0x54, 0x9F, 0x45, 0x06, 0xF4,
];
pub(crate) const FILL_RECT_WORKLIST_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0xCF, 0x5B, 0x9B, 0x47, 0xCF, 0xBC, 0x5E, 0xD2, 0x2A, 0x90, 0x4A, 0x37, 0xF2, 0x49, 0xDA, 0x42,
    0xB7, 0xC9, 0x9B, 0x1D, 0x4F, 0x45, 0x28, 0xBC, 0xA2, 0xB8, 0xC6, 0x0E, 0x54, 0x03, 0x62, 0x79,
];
pub(crate) const GRADIENT_RECT_WORKLIST_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0xC0, 0x3A, 0xEE, 0xFC, 0x4D, 0x20, 0x23, 0xD5, 0xEE, 0x70, 0x3C, 0x5D, 0xBB, 0xB3, 0x1E, 0xBC,
    0x20, 0x93, 0xB1, 0x04, 0xBE, 0x00, 0xDB, 0x2B, 0xC7, 0x8D, 0x29, 0xC5, 0x30, 0xF4, 0x27, 0x37,
];

pub(crate) const ALPHA_BLEND_WORKLIST_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0xF9, 0x0C, 0x66, 0xC5, 0xFB, 0xA3, 0xED, 0x22, 0xEB, 0x42, 0xD0, 0x08, 0xAC, 0x94, 0x38, 0x2F,
    0xD7, 0x4D, 0x35, 0xF5, 0x5C, 0x41, 0x04, 0x4C, 0x10, 0x14, 0x49, 0x70, 0x95, 0xAB, 0x3C, 0xD3,
];
pub(crate) const GLYPH_MASK_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0x90, 0x8D, 0xF0, 0x7D, 0x62, 0xB0, 0x69, 0xF3, 0x1A, 0x04, 0x6D, 0x29, 0x02, 0xDF, 0xF9, 0xA0,
    0xFA, 0x33, 0xE4, 0x9A, 0x1C, 0x25, 0x3B, 0x74, 0xA4, 0xE7, 0xCC, 0x18, 0xDF, 0x66, 0xD3, 0x78,
];
pub(crate) const PRESENT_RGBA8_TO_PRIMARY_XRGB_RECT_ADLS_BIN_SHA256: [u8; 32] = [
    0x11, 0xAF, 0xC5, 0x16, 0x53, 0x2B, 0xC0, 0xF4, 0x8E, 0x9B, 0x9E, 0xDE, 0x0E, 0x28, 0x2F, 0xC3,
    0xEB, 0x50, 0xC6, 0x4E, 0xBC, 0x02, 0xDB, 0xA0, 0x6E, 0x38, 0x64, 0x6E, 0x3B, 0x20, 0xE5, 0x4A,
];
pub(crate) const UI4_NV12_YTILE_TO_PRIMARY_XRGB_ADLS_BIN_SHA256: [u8; 32] = [
    0x65, 0xEA, 0x37, 0xF3, 0xCE, 0x33, 0xAC, 0x92, 0x67, 0x92, 0x80, 0x34, 0xC3, 0xE5, 0x59, 0xF8,
    0x85, 0x47, 0xE9, 0x02, 0x9D, 0x29, 0x22, 0x31, 0xC9, 0x11, 0x6F, 0x25, 0x85, 0x13, 0x2E, 0x4D,
];
pub(crate) const UI4_NV12_TILE64_TO_RGBA8_FRAME_ADLS_BIN_SHA256: [u8; 32] = [
    0x35, 0xCD, 0x9F, 0x3C, 0xBA, 0xD1, 0xF2, 0xCE, 0xC7, 0x6B, 0x3E, 0xFB, 0xF9, 0xD8, 0xC9, 0x10,
    0x01, 0xCD, 0x07, 0x8B, 0xE4, 0xD2, 0x44, 0x35, 0xB1, 0x1C, 0x09, 0x12, 0xAF, 0xA7, 0x37, 0x49,
];

pub(crate) const SPRITE64_WORKLIST_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0x79, 0x42, 0xAC, 0xAB, 0x49, 0x7D, 0x8F, 0xD3, 0xB7, 0xD4, 0x06, 0x67, 0x9F, 0x1B, 0x2A, 0x61,
    0x4F, 0x3F, 0x4E, 0xEF, 0x78, 0xDF, 0x2E, 0x66, 0x7B, 0x9F, 0x40, 0x4E, 0x34, 0xA8, 0x22, 0xFB,
];
pub(crate) const SPRITE_QUAD_WORKLIST_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0x8D, 0xFC, 0x62, 0x17, 0xFF, 0x63, 0x46, 0xFE, 0x26, 0x60, 0x07, 0x9F, 0xC9, 0x05, 0xED, 0x5E,
    0x48, 0x18, 0x7A, 0xF4, 0x8B, 0x0C, 0x90, 0xC5, 0xE0, 0xD5, 0xE5, 0x6A, 0x80, 0xEF, 0x34, 0x37,
];
pub(crate) const UI4_COMPOSE_LAYERS_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0xFE, 0x3D, 0xC7, 0xDF, 0x1B, 0x4B, 0x8B, 0x50, 0x31, 0x3C, 0xCE, 0x32, 0xF4, 0xAA, 0xB0, 0xA5,
    0xDE, 0xDA, 0x1A, 0x22, 0x95, 0x68, 0x83, 0x10, 0x9D, 0x9F, 0xF0, 0xD6, 0xF7, 0x82, 0x8B, 0x14,
];
pub(crate) const MANDEL64_WORKLIST_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0x8B, 0x17, 0x46, 0x98, 0x4F, 0x74, 0x15, 0x6C, 0xCD, 0xBE, 0xB9, 0x43, 0x1D, 0xF9, 0xD2, 0x50,
    0x61, 0x28, 0x56, 0x55, 0x06, 0x7D, 0xE8, 0xEB, 0xD5, 0x28, 0x3B, 0x08, 0xDE, 0x00, 0xD9, 0x1F,
];
pub(crate) const CANVAS3D_PROJECT_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0xDA, 0xF0, 0x15, 0xA0, 0xB9, 0x8A, 0x45, 0xF7, 0x02, 0xD5, 0xD7, 0x87, 0xCA, 0x19, 0x59, 0xBA,
    0xAC, 0x7C, 0x02, 0xFE, 0x97, 0x93, 0xAC, 0x6E, 0x48, 0xA7, 0x87, 0x18, 0xAE, 0x3D, 0x3E, 0xB6,
];
pub(crate) const CANVAS3D_TRANSFORM_Q16_ADLS_BIN_SHA256: [u8; 32] = [
    0x2C, 0x94, 0x28, 0x73, 0xA2, 0xB5, 0x4C, 0xA2, 0xBB, 0xBD, 0x17, 0xDA, 0x25, 0xFD, 0x1D, 0x22,
    0x0E, 0x86, 0x34, 0x87, 0xAE, 0xD5, 0x9A, 0xE2, 0xA5, 0xE4, 0xF3, 0x0D, 0x41, 0x8F, 0x1D, 0x4D,
];
pub(crate) const CANVAS3D_CLIP_BOX_Q16_ADLS_BIN_SHA256: [u8; 32] = [
    0x7E, 0x28, 0xD6, 0xB4, 0xF7, 0xF3, 0x7C, 0x95, 0x37, 0x4C, 0x27, 0x4B, 0x37, 0x02, 0x81, 0x30,
    0x11, 0x61, 0xED, 0xF7, 0xD4, 0xA7, 0x17, 0x51, 0x86, 0x8F, 0x9A, 0x2B, 0x56, 0x59, 0xEA, 0x5F,
];
pub(crate) const CANVAS3D_PLANE_SAMPLE_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0x4B, 0x96, 0xF2, 0x00, 0xB5, 0xE2, 0x6B, 0x7C, 0xCA, 0x73, 0xA3, 0x32, 0xC4, 0xF5, 0x9B, 0xC8,
    0xFF, 0x51, 0x1A, 0x73, 0xF3, 0xC9, 0x09, 0xCC, 0x86, 0xAE, 0x8D, 0xE2, 0x21, 0xF8, 0xEF, 0xB7,
];
pub(crate) const CANVAS3D_PLANE_FILL_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0xAB, 0xFB, 0x97, 0xE7, 0x62, 0x27, 0x37, 0x0A, 0xA3, 0xF0, 0x4E, 0x96, 0xC8, 0x5C, 0x99, 0xA1,
    0xA1, 0xBC, 0xCD, 0xC1, 0x25, 0xF8, 0xB5, 0x74, 0xFC, 0xA6, 0xB5, 0x6C, 0x1B, 0x4E, 0x5C, 0x30,
];
pub(crate) const CANVAS3D_PLANE_PATCH_FILL_CUT_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0x11, 0x7D, 0x4A, 0x81, 0x83, 0x11, 0x7D, 0x1D, 0x24, 0x6E, 0x8F, 0xF6, 0xDD, 0xB6, 0x7D, 0x56,
    0x0E, 0xB1, 0xFD, 0xB3, 0x63, 0x49, 0xBE, 0x28, 0xFD, 0x62, 0xD1, 0x36, 0x01, 0xA8, 0x58, 0x07,
];
pub(crate) const CANVAS3D_PLANE_PATCH_WORKLIST_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0x4A, 0xC7, 0xC0, 0xD7, 0xC1, 0x1A, 0xC7, 0x99, 0xC9, 0x74, 0x03, 0x7E, 0x42, 0x89, 0x8C, 0xAE,
    0xAC, 0xC4, 0xB9, 0x2E, 0x3C, 0x52, 0x69, 0x09, 0x4D, 0x28, 0x73, 0xBC, 0xA6, 0x36, 0x31, 0x93,
];
pub(crate) const SKYBOX_SAMPLE_RGB565_ADLS_BIN_SHA256: [u8; 32] = [
    0xC0, 0xBA, 0x26, 0xB3, 0x1C, 0x54, 0xED, 0x16, 0xC8, 0x32, 0x3E, 0x3D, 0xE6, 0x54, 0xAA, 0x90,
    0x8A, 0xB4, 0xBC, 0xDC, 0xCF, 0xC8, 0xBB, 0xD5, 0xF1, 0x05, 0x2F, 0x1B, 0xF7, 0x75, 0x76, 0x38,
];
pub(crate) const CHART_SINE_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0x79, 0xeb, 0x20, 0xbc, 0x33, 0x7e, 0x17, 0x2a, 0x8c, 0xcd, 0xdc, 0xdc, 0x66, 0x54, 0xee, 0xa9,
    0x92, 0xe8, 0x9f, 0xb5, 0xfb, 0x67, 0xb2, 0xf3, 0x2c, 0xaa, 0xd1, 0xc1, 0xaf, 0xa1, 0xc0, 0xe4,
];
pub(crate) const PIXEL_PLASMA_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0x42, 0xfb, 0x1d, 0xd0, 0x56, 0x8b, 0xb2, 0x44, 0xc4, 0x4f, 0x87, 0xd1, 0x46, 0xe0, 0x36, 0xa7,
    0x2d, 0xf6, 0x0c, 0xb8, 0x11, 0x71, 0x5c, 0x37, 0x0e, 0xc9, 0x59, 0xde, 0x6d, 0x3a, 0xf8, 0x93,
];
pub(crate) const FONT_OUTLINE_MESH_ADLS_BIN_SHA256: [u8; 32] = [
    0xbf, 0x78, 0xe5, 0xd6, 0x87, 0x0f, 0x23, 0x03, 0xb7, 0x07, 0xd3, 0x03, 0x20, 0xd8, 0xda, 0xa1,
    0x55, 0x54, 0x08, 0x5a, 0x75, 0xd4, 0x7a, 0x48, 0xb5, 0x1f, 0xb9, 0x32, 0xf4, 0xfa, 0x3d, 0x25,
];
pub(crate) const FONT_OUTLINE_COVERAGE_R8_ADLS_BIN_SHA256: [u8; 32] = [
    0xA4, 0xF0, 0xDD, 0xDC, 0x7F, 0x2A, 0x9D, 0x9D, 0x67, 0xE5, 0xE7, 0x14, 0x59, 0xD5, 0x4D, 0xA2,
    0xE4, 0xA7, 0xAD, 0xE8, 0xCD, 0x1A, 0xF8, 0xC2, 0x72, 0x83, 0xA8, 0x84, 0xF2, 0x21, 0xB8, 0x36,
];

const COPY_RECT_RGBA8_ADLS_GPU: u64 = 0x0D20_0000;
const RESOLVE_TILE64_MSAA4_RGBA8_ADLS_GPU: u64 = 0x0D3C_0000;
const SPRITE64_WORKLIST_RGBA8_ADLS_GPU: u64 = 0x0D24_0000;
const SPRITE_QUAD_WORKLIST_RGBA8_ADLS_GPU: u64 = 0x0D37_0000;
const UI4_COMPOSE_LAYERS_RGBA8_ADLS_GPU: u64 = 0x0D3E_0000;
const MANDEL64_WORKLIST_RGBA8_ADLS_GPU: u64 = 0x0D36_0000;
const FILL_RECT_RGBA8_ADLS_GPU: u64 = 0x0D2B_0000;

const GLYPH_MASK_RGBA8_ADLS_GPU: u64 = 0x0D2D_0000;
const PRESENT_RGBA8_TO_PRIMARY_XRGB_RECT_ADLS_GPU: u64 = 0x0D2E_0000;
const FILL_RECT_WORKLIST_RGBA8_ADLS_GPU: u64 = 0x0D2F_0000;
const ALPHA_BLEND_WORKLIST_RGBA8_ADLS_GPU: u64 = 0x0D30_0000;
const GRADIENT_RECT_WORKLIST_RGBA8_ADLS_GPU: u64 = 0x0D31_0000;
const CANVAS3D_PROJECT_RGBA8_ADLS_GPU: u64 = 0x0D25_0000;
const CANVAS3D_CLIP_BOX_Q16_ADLS_GPU: u64 = 0x0D29_0000;
const CANVAS3D_TRANSFORM_Q16_ADLS_GPU: u64 = 0x0D2A_0000;
const CANVAS3D_PLANE_SAMPLE_RGBA8_ADLS_GPU: u64 = 0x0D32_0000;
const CANVAS3D_PLANE_FILL_RGBA8_ADLS_GPU: u64 = 0x0D33_0000;
const CANVAS3D_PLANE_PATCH_FILL_CUT_RGBA8_ADLS_GPU: u64 = 0x0D34_0000;
const CANVAS3D_PLANE_PATCH_WORKLIST_RGBA8_ADLS_GPU: u64 = 0x0D35_0000;
const SKYBOX_SAMPLE_RGB565_ADLS_GPU: u64 = 0x0D38_0000;
const CHART_SINE_RGBA8_ADLS_GPU: u64 = 0x0D39_0000;
const PIXEL_PLASMA_RGBA8_ADLS_GPU: u64 = 0x0D3A_0000;
const FONT_OUTLINE_MESH_ADLS_GPU: u64 = 0x0D3B_0000;
const FONT_OUTLINE_COVERAGE_R8_ADLS_GPU: u64 = 0x0D3D_0000;
const UI4_NV12_YTILE_TO_PRIMARY_XRGB_ADLS_GPU: u64 = 0x0D3F_0000;
const UI4_NV12_TILE64_TO_RGBA8_FRAME_ADLS_GPU: u64 = 0x0D40_0000;
const COPY_RECT_RGBA8_TEXT_OFFSET_BYTES: u64 = 0x40;
const RESOLVE_TILE64_MSAA4_RGBA8_TEXT_OFFSET_BYTES: u64 = 0x40;
const FONT_OUTLINE_COVERAGE_R8_TEXT_OFFSET_BYTES: u64 = 0x40;
const UI4_NV12_YTILE_TO_PRIMARY_XRGB_TEXT_OFFSET_BYTES: u64 = 0x40;
const UI4_NV12_TILE64_TO_RGBA8_FRAME_TEXT_OFFSET_BYTES: u64 = 0x40;
const FILL_RECT_RGBA8_TEXT_OFFSET_BYTES: u64 = 0x40;
const FILL_RECT_WORKLIST_RGBA8_TEXT_OFFSET_BYTES: u64 = 0x40;

const SPRITE_QUAD_WORKLIST_RGBA8_TEXT_OFFSET_BYTES: u64 = 0x40;
const UI4_COMPOSE_LAYERS_RGBA8_TEXT_OFFSET_BYTES: u64 = 0x40;
const MANDEL64_WORKLIST_RGBA8_TEXT_OFFSET_BYTES: u64 = 0x40;

const GLYPH_MASK_RGBA8_TEXT_OFFSET_BYTES: u64 = 0x40;

const SKYBOX_SAMPLE_RGB565_TEXT_OFFSET_BYTES: u64 = 0x40;
const CHART_SINE_RGBA8_TEXT_OFFSET_BYTES: u64 = 0x40;
const PIXEL_PLASMA_RGBA8_TEXT_OFFSET_BYTES: u64 = 0x40;
const FONT_OUTLINE_MESH_TEXT_OFFSET_BYTES: u64 = 0x40;

const RCS_RING_BASE: usize = 0x0000_2000;
const RCS_RING_TAIL: usize = RCS_RING_BASE + 0x30;
const RCS_RING_HEAD: usize = RCS_RING_BASE + 0x34;
const RCS_RING_ACTHD: usize = RCS_RING_BASE + 0x74;
const RCS_RING_IPEIR: usize = RCS_RING_BASE + 0x64;
const RCS_RING_IPEHR: usize = RCS_RING_BASE + 0x68;
const RCS_RING_EIR: usize = RCS_RING_BASE + 0xB0;

const RCS_CS_DEBUG_MODE1: usize = RCS_RING_BASE + 0xEC;

const FORCEWAKE_RENDER: usize = 0x0A278;
const FORCEWAKE_GT: usize = 0x0A188;
const FORCEWAKE_ACK_RENDER: usize = 0x0D84;
const FORCEWAKE_ACK_GT: usize = 0x130044;
const FORCEWAKE_KERNEL: u32 = 1 << 0;
const FORCEWAKE_FALLBACK: u32 = 1 << 15;
const FORCEWAKE_POLL_ITERS: usize = 20_000;
const FF_DOP_CLOCK_GATE_DISABLE: u32 = 1 << 1;
const RING_VALID: u32 = 1;

const CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT: u32 = 1 << 0;
const CTX_CTRL_INHIBIT_SYN_CTX_SWITCH: u32 = 1 << 3;

const RING_MI_MODE_STOP_RING: u32 = 1 << 8;
const MI_BATCH_BUFFER_START_GEN8: u32 = (0x31 << 23) | 1;
const MI_BATCH_GTT: u32 = 2 << 6;
const MI_STORE_DATA_IMM_GGTT_DW1: u32 = 0x1040_0002;
const MI_LOAD_REGISTER_IMM: u32 = 0x1100_0000;
const MI_LRI_CS_MMIO: u32 = 1 << 19;
const MI_LRI_FORCE_POSTED: u32 = 1 << 12;
const MI_BATCH_BUFFER_END: u32 = 0x0500_0000;
const MI_NOOP: u32 = 0;
const INTEL_LEGACY_64B_CONTEXT: u32 = 3;
const GEN8_PAGE_RW: u64 = 1 << 1;
const GEN8_PAGE_PWT: u64 = 1 << 3;
const GEN8_PAGE_PCD: u64 = 1 << 4;
const GEN8_CTX_VALID: u32 = 1 << 0;

const GEN8_CTX_PRIVILEGE: u32 = 1 << 8;

const GEN8_CTX_ADDRESSING_MODE_SHIFT: u32 = 3;
const RENDER_MOCS: u32 = 4;
const PIPE_CONTROL_CMD: u32 = 4 | (2 << 24) | (3 << 27) | (3 << 29);
const STATE_BASE_ADDRESS_CMD: u32 = 20 | (1 << 16) | (1 << 24) | (3 << 29);
const PIPE_CONTROL_DC_FLUSH_ENABLE: u32 = 1 << 5;
const PIPE_CONTROL_FLUSH_ENABLE: u32 = 1 << 7;
const PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH: u32 = 1 << 12;
// Gen12 PIPE_CONTROL splits HDC pipeline drain from the DW1 cache controls:
// HDC Pipeline Flush is DW0 bit 9.  DW1 bit 26 is Flush LLC and is only legal
// with a post-sync immediate write; treating it as HDC allowed the completion
// marker to retire while data-port writes were still only partially visible to
// the next GuC context.
const PIPE_CONTROL_HDC_PIPELINE_FLUSH: u32 = 1 << 9;
const PIPE_CONTROL_POST_SYNC_WRITE_IMMEDIATE: u32 = 1 << 14;
const PIPE_CONTROL_CS_STALL: u32 = 1 << 20;
const PIPE_CONTROL_L3_FABRIC_FLUSH: u32 = 1 << 30;
const PIPE_CONTROL_TLB_INVALIDATE: u32 = 1 << 18;
const PIPE_CONTROL_STATE_CACHE_INVALIDATE: u32 = 1 << 2;
const PIPE_CONTROL_CONSTANT_CACHE_INVALIDATE: u32 = 1 << 3;
const PIPE_CONTROL_TEXTURE_CACHE_INVALIDATE: u32 = 1 << 10;
const PIPE_CONTROL_INSTRUCTION_CACHE_INVALIDATE: u32 = 1 << 11;
const PIPE_CONTROL_COMMAND_CACHE_INVALIDATE: u32 = 1 << 29;
const PIPE_CONTROL_FLUSH_BITS: u32 = PIPE_CONTROL_DC_FLUSH_ENABLE
    | PIPE_CONTROL_FLUSH_ENABLE
    | PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH
    | PIPE_CONTROL_CS_STALL
    // HDC drains data-port writes into L3. Scanout is a separate observer, so
    // force the stalling flush to carry every pending L3 transaction to the
    // global observation point before the completion marker can retire.
    | PIPE_CONTROL_L3_FABRIC_FLUSH;
const PIPE_CONTROL_INVALIDATE_BITS: u32 = PIPE_CONTROL_FLUSH_BITS
    | PIPE_CONTROL_TLB_INVALIDATE
    | PIPE_CONTROL_STATE_CACHE_INVALIDATE
    | PIPE_CONTROL_CONSTANT_CACHE_INVALIDATE
    | PIPE_CONTROL_TEXTURE_CACHE_INVALIDATE
    | PIPE_CONTROL_INSTRUCTION_CACHE_INVALIDATE
    | PIPE_CONTROL_COMMAND_CACHE_INVALIDATE;
const MEDIA_VFE_STATE_CMD: u32 = (3 << 29) | (2 << 27) | 7;
const MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD: u32 = (3 << 29) | (2 << 27) | (2 << 16) | 2;
const GPGPU_WALKER_CMD: u32 = (3 << 29) | (2 << 27) | (1 << 24) | (5 << 16) | 13;
const MEDIA_STATE_FLUSH_CMD: u32 = (3 << 29) | (2 << 27) | (4 << 16);
const PIPELINE_SELECT_BASE: u32 = (3 << 29) | (1 << 27) | (1 << 24) | (4 << 16);
const PIPELINE_SELECT_GFX12_MASK: u32 = 0x13 << 8;
const PIPELINE_SELECT_MEDIA_SAMPLER_DOP_CG_ENABLE: u32 = 1 << 4;
const PIPELINE_SELECT_3D: u32 =
    PIPELINE_SELECT_BASE | PIPELINE_SELECT_GFX12_MASK | PIPELINE_SELECT_MEDIA_SAMPLER_DOP_CG_ENABLE;
const PIPELINE_SELECT_GPGPU: u32 = PIPELINE_SELECT_3D | 2;
const IDD_THREAD_PREEMPTION_DISABLE: u32 = 1 << 20;
const GPGPU_VFE_DW3_UOS: u32 = 0x00A7_0100;
const GPGPU_VFE_DW5_UOS: u32 = 0x0782_0000;
const GPGPU_WALKER_GROUP_THREADS: u32 = 1;
const GPGPU_WALKER_SIMD16_SELECT: u32 = 1;
const FILL_RECT_PIXELS_PER_GROUP_X: u32 = 16;
const COPY_RECT_2D_COMPLETION_TIMEOUT_MS: u64 = 250;
const FILL_RECT_2D_COMPLETION_TIMEOUT_MS: u64 = 250;

const RESOLVE_TILE64_MSAA4_COMPLETION_TIMEOUT_MS: u64 = 250;
const FONT_OUTLINE_COVERAGE_R8_COMPLETION_TIMEOUT_MS: u64 = 500;
const GPGPU_WALKER_GROUP_Z_DIM: u32 = 1;
const GPGPU_WALKER_SIMD16_MASK: u32 = 0x0000_FFFF;
const GPGPU_WALKER_BOTTOM_MASK: u32 = 0xFFFF_FFFF;

const COPY_RECT_BATCH_IDD_OFFSET_BYTES: usize = 0x1000;
const COPY_RECT_BATCH_BINDING_TABLE_OFFSET_BYTES: usize = 0x1040;
const COPY_RECT_BATCH_SRC_SURFACE_STATE_OFFSET_BYTES: usize = 0x1080;
const COPY_RECT_BATCH_DST_SURFACE_STATE_OFFSET_BYTES: usize = 0x10C0;
const COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES: usize = 0x1200;
const COPY_RECT_PIXELS_PER_LANE: u32 = 2;

// The native UI4 video compositor has exactly three raw buffers and one
// SIMD16 full-output dispatch.  It intentionally owns no descriptor worklist.
const UI4_NV12_PRIMARY_IDD_OFFSET_BYTES: usize = 0x1000;
const UI4_NV12_PRIMARY_BINDING_TABLE_OFFSET_BYTES: usize = 0x1040;
const UI4_NV12_PRIMARY_SRC_SURFACE_STATE_OFFSET_BYTES: usize = 0x1080;
const UI4_NV12_PRIMARY_BASE_SURFACE_STATE_OFFSET_BYTES: usize = 0x10C0;
const UI4_NV12_PRIMARY_DST_SURFACE_STATE_OFFSET_BYTES: usize = 0x1100;
const UI4_NV12_PRIMARY_PAYLOAD_OFFSET_BYTES: usize = 0x1200;
const UI4_NV12_PRIMARY_CROSS_THREAD_GRFS: u32 = 4;
const UI4_NV12_PRIMARY_CROSS_THREAD_BYTES: usize = UI4_NV12_PRIMARY_CROSS_THREAD_GRFS as usize * 32;
const UI4_NV12_PRIMARY_PER_THREAD_BYTES: usize = 96;
const UI4_NV12_PRIMARY_INDIRECT_BYTES: usize =
    UI4_NV12_PRIMARY_CROSS_THREAD_BYTES + UI4_NV12_PRIMARY_PER_THREAD_BYTES;

// GridPaper can retain 17 independently colored layers for each of its three
// font faces (51 total). Keep enough room to submit the complete scene once.
const GLYPH_MASK_BATCH_MAX_LAYERS: usize = 64;
const GLYPH_MASK_BATCH_STATE_BASE_OFFSET_BYTES: usize = 0x3000;
const GLYPH_MASK_BATCH_STATE_BLOCK_BYTES: usize = 0x100;
const GLYPH_MASK_BATCH_IDD_OFFSET_IN_BLOCK_BYTES: usize = 0x00;
const GLYPH_MASK_BATCH_BINDING_TABLE_OFFSET_IN_BLOCK_BYTES: usize = 0x40;
const GLYPH_MASK_BATCH_SRC_SURFACE_OFFSET_IN_BLOCK_BYTES: usize = 0x80;
const GLYPH_MASK_BATCH_DST_SURFACE_OFFSET_IN_BLOCK_BYTES: usize = 0xC0;
const GLYPH_MASK_BATCH_PAYLOAD_BASE_OFFSET_BYTES: usize = 0x8000;
const COPY_RECT_IDD_BYTES: usize = 8 * core::mem::size_of::<u32>();
const COPY_RECT_SURFACE_STATE_DWORDS: usize = 16;
const COPY_RECT_CROSS_THREAD_BYTES: usize = 96;
const COPY_RECT_PER_THREAD_BYTES: usize = 96;
const COPY_RECT_INDIRECT_BYTES: usize = COPY_RECT_CROSS_THREAD_BYTES + COPY_RECT_PER_THREAD_BYTES;

const GLYPH_MASK_CROSS_THREAD_BYTES: usize = 128;
const GLYPH_MASK_PER_THREAD_BYTES: usize = 96;
const GLYPH_MASK_INDIRECT_BYTES: usize =
    GLYPH_MASK_CROSS_THREAD_BYTES + GLYPH_MASK_PER_THREAD_BYTES;

const RECT_WORKLIST_IDD_OFFSET_BYTES: usize = 0x1400;
const RECT_WORKLIST_BINDING_TABLE_OFFSET_BYTES: usize = 0x1440;
const RECT_WORKLIST_SRC_SURFACE_STATE_OFFSET_BYTES: usize = 0x1480;
const RECT_WORKLIST_DST_SURFACE_STATE_OFFSET_BYTES: usize = 0x14C0;
const RECT_WORKLIST_DESC_SURFACE_STATE_OFFSET_BYTES: usize = 0x1500;
const RECT_WORKLIST_PAYLOAD_OFFSET_BYTES: usize = 0x1600;
const RECT_WORKLIST_IDD_BYTES: usize = 8 * core::mem::size_of::<u32>();
const RECT_WORKLIST_CROSS_THREAD_GRFS: u32 = 3;
const RECT_WORKLIST_CROSS_THREAD_BYTES: usize = RECT_WORKLIST_CROSS_THREAD_GRFS as usize * 32;
const FILL_RECT_WORKLIST_CROSS_THREAD_GRFS: u32 = 2;
const FILL_RECT_WORKLIST_CROSS_THREAD_BYTES: usize =
    FILL_RECT_WORKLIST_CROSS_THREAD_GRFS as usize * 32;
const RECT_WORKLIST_PER_THREAD_BYTES: usize = 96;
const RECT_WORKLIST_INDIRECT_BYTES: usize =
    RECT_WORKLIST_CROSS_THREAD_BYTES + RECT_WORKLIST_PER_THREAD_BYTES;
// The 2D kernel consumes global/local/enqueued-local payload fields before its
// three pointers and scalar arguments.  The baked ADL-S artifact therefore
// declares 104 bytes of cross-thread data, rounded to four 32-byte GRFs.
const SPRITE_QUAD_WORKLIST_CROSS_THREAD_GRFS: u32 = 4;
const SPRITE_QUAD_WORKLIST_CROSS_THREAD_BYTES: usize =
    SPRITE_QUAD_WORKLIST_CROSS_THREAD_GRFS as usize * 32;
const SPRITE_QUAD_WORKLIST_PER_THREAD_BYTES: usize = 96;
const SPRITE_QUAD_WORKLIST_INDIRECT_BYTES: usize =
    SPRITE_QUAD_WORKLIST_CROSS_THREAD_BYTES + SPRITE_QUAD_WORKLIST_PER_THREAD_BYTES;
const UI4_COMPOSE_LAYERS_CROSS_THREAD_GRFS: u32 = 4;
const UI4_COMPOSE_LAYERS_CROSS_THREAD_BYTES: usize =
    UI4_COMPOSE_LAYERS_CROSS_THREAD_GRFS as usize * 32;
const UI4_COMPOSE_LAYERS_PER_THREAD_BYTES: usize = 96;
const UI4_COMPOSE_LAYERS_INDIRECT_BYTES: usize =
    UI4_COMPOSE_LAYERS_CROSS_THREAD_BYTES + UI4_COMPOSE_LAYERS_PER_THREAD_BYTES;
const SPRITE_QUAD_WORKLIST_RUN_STATE_BLOCK_BYTES: usize = 0x140;
const SPRITE_QUAD_WORKLIST_RUN_IDD_REL: usize = 0x00;
const SPRITE_QUAD_WORKLIST_RUN_BINDING_REL: usize = 0x40;
const SPRITE_QUAD_WORKLIST_RUN_SRC_SURFACE_REL: usize = 0x80;
const SPRITE_QUAD_WORKLIST_RUN_DST_SURFACE_REL: usize = 0xC0;
const SPRITE_QUAD_WORKLIST_RUN_DESC_SURFACE_REL: usize = 0x100;
const RECT_WORKLIST_PRE_MARKER_SLOT: usize = 15;
const RECT_WORKLIST_POST_MARKER_SLOT: usize = 14;
const FILL_RECT_WORKLIST_PRE_MARKER: u32 = 0xC0DE_5801;
const FILL_RECT_WORKLIST_POST_MARKER: u32 = 0xC0DE_5802;

const SPRITE_QUAD_WORKLIST_PRE_MARKER_SLOT: usize = 25;
const SPRITE_QUAD_WORKLIST_POST_MARKER_SLOT: usize = 24;
const SPRITE_QUAD_WORKLIST_PRE_MARKER: u32 = 0xC0DE_5B01;
const SPRITE_QUAD_WORKLIST_POST_MARKER: u32 = 0xC0DE_5B02;
const UI4_COMPOSE_LAYERS_PRE_MARKER: u32 = 0xC0DE_4C01;
const UI4_COMPOSE_LAYERS_POST_MARKER: u32 = 0xC0DE_4C02;

const MANDEL64_WORKLIST_PRE_MARKER: u32 = 0xC0DE_6401;
const MANDEL64_WORKLIST_POST_MARKER: u32 = 0xC0DE_6402;
const RECT_WORKLIST_DESC_GPU: u64 = 0x05A0_0000;
const MANDEL64_WORKLIST_DESC_GPU: u64 = 0x05B0_0000;
const SPRITE_QUAD_WORKLIST_DESC_GPU: u64 = 0x05C0_0000;
const RECT_WORKLIST_MAX_DESCS: usize = 256;
const RECT_WORKLIST_DESCS_PER_WALKER: usize = 16;
const RECT_WORKLIST_MAX_WALKERS: usize = RECT_WORKLIST_MAX_DESCS / RECT_WORKLIST_DESCS_PER_WALKER;
const RECT_WORKLIST_DESC_BYTES: usize = 8192;
const SPRITE_QUAD_WORKLIST_MAX_DESCS: usize = 256;
const SPRITE_QUAD_WORKLIST_DESCS_PER_WALKER: usize = 1;
const SPRITE_QUAD_WORKLIST_MAX_WALKERS: usize = SPRITE_QUAD_WORKLIST_MAX_DESCS;
const SPRITE_QUAD_WORKLIST_DESC_BYTES: usize =
    SPRITE_QUAD_WORKLIST_MAX_DESCS * core::mem::size_of::<GpgpuSpriteQuadWorklistDesc>();
const SPRITE_QUAD_WORKLIST_MAX_GROUPS_PER_WALKER: usize = SPRITE_QUAD_WORKLIST_MAX_DESCS;
const UI4_COMPOSE_LAYERS_MAX_LAYERS: usize = 32;
// Match the shader's one-work-item-per-pixel contract. A previous 64-row
// serial lane tile was the source of partial horizontal compositor strips.
const SPRITE_QUAD_WORKLIST_TILE_ROWS: u32 = 1;
const MANDEL64_WORKLIST_CELL_PIXELS: u32 = 64;
const MANDEL64_WORKLIST_BAND_ROWS: u32 = 4;
const MANDEL64_WORKLIST_BANDS_PER_TILE: usize =
    (MANDEL64_WORKLIST_CELL_PIXELS / MANDEL64_WORKLIST_BAND_ROWS) as usize;
const MANDEL64_WORKLIST_MAX_DESCS: usize = 512;
const MANDEL64_WORKLIST_DESCS_PER_WALKER: usize = RECT_WORKLIST_DESCS_PER_WALKER;
const MANDEL64_WORKLIST_MAX_WALKERS: usize =
    MANDEL64_WORKLIST_MAX_DESCS / MANDEL64_WORKLIST_DESCS_PER_WALKER;
const MANDEL64_WORKLIST_FLAG_ROWS_MASK: u32 = 0x0000_007F;
const MANDEL64_WORKLIST_FLAG_NO_MIRROR: u32 = 1 << 7;
const MANDEL64_WORKLIST_FLAG_COLS_SHIFT: u32 = 8;
const MANDEL64_WORKLIST_FLAG_VIEW_HEIGHT_SHIFT: u32 = 16;

pub(crate) const MANDEL64_WORKLIST_MAX_ITERATIONS: u32 = 512;

const CANVAS3D_PROJECT_OUT_ALLOC_BYTES: usize = 64 * 1024;

fn pack_mandel64_iterations(iterations: u32) -> u32 {
    let max_iter = iterations.clamp(1, MANDEL64_WORKLIST_MAX_ITERATIONS);
    let gray_scale = (255u32 * 256u32) / max_iter;
    max_iter | (gray_scale << 16)
}

const SKYBOX_SAMPLE_IDD_OFFSET_BYTES: usize = 0x4200;
const SKYBOX_SAMPLE_BINDING_TABLE_OFFSET_BYTES: usize = 0x4240;
const SKYBOX_SAMPLE_SRC_SURFACE_STATE_OFFSET_BYTES: usize = 0x4280;
const SKYBOX_SAMPLE_DST_SURFACE_STATE_OFFSET_BYTES: usize = 0x42C0;
const SKYBOX_SAMPLE_PAYLOAD_OFFSET_BYTES: usize = 0x4400;
const SKYBOX_SAMPLE_IDD_BYTES: usize = 8 * core::mem::size_of::<u32>();
const SKYBOX_SAMPLE_CROSS_THREAD_BYTES: usize = 160;
const SKYBOX_SAMPLE_PER_THREAD_BYTES: usize = 96;
const SKYBOX_SAMPLE_INDIRECT_BYTES: usize =
    SKYBOX_SAMPLE_CROSS_THREAD_BYTES + SKYBOX_SAMPLE_PER_THREAD_BYTES;
const SKYBOX_SAMPLE_PRE_MARKER_SLOT: usize = 27;
const SKYBOX_SAMPLE_POST_MARKER_SLOT: usize = 26;
const SKYBOX_SAMPLE_PRE_MARKER: u32 = 0xC0DE_5C01;
const SKYBOX_SAMPLE_POST_MARKER: u32 = 0xC0DE_5C02;
const CHART_SINE_IDD_OFFSET_BYTES: usize = 0x4600;
const CHART_SINE_BINDING_TABLE_OFFSET_BYTES: usize = 0x4640;
const CHART_SINE_DST_SURFACE_STATE_OFFSET_BYTES: usize = 0x4680;
const CHART_SINE_PAYLOAD_OFFSET_BYTES: usize = 0x4800;
const CHART_SINE_IDD_BYTES: usize = 8 * core::mem::size_of::<u32>();
const CHART_SINE_CROSS_THREAD_BYTES: usize = 128;
const CHART_SINE_PER_THREAD_BYTES: usize = 96;
const CHART_SINE_INDIRECT_BYTES: usize =
    CHART_SINE_CROSS_THREAD_BYTES + CHART_SINE_PER_THREAD_BYTES;
const CHART_SINE_PRE_MARKER_SLOT: usize = 29;
const CHART_SINE_POST_MARKER_SLOT: usize = 28;
const CHART_SINE_PRE_MARKER: u32 = 0xC0DE_C701;
const CHART_SINE_POST_MARKER: u32 = 0xC0DE_C702;
pub(crate) const CHART_SINE_FLAG_GRID: u32 = 1 << 0;
pub(crate) const CHART_SINE_FLAG_AXES: u32 = 1 << 1;
pub(crate) const CHART_SINE_FLAG_GLOW: u32 = 1 << 2;
pub(crate) const CHART_SINE_FLAG_BORDER: u32 = 1 << 3;
const PIXEL_PLASMA_IDD_OFFSET_BYTES: usize = 0x4A00;
const PIXEL_PLASMA_BINDING_TABLE_OFFSET_BYTES: usize = 0x4A40;
const PIXEL_PLASMA_DST_SURFACE_STATE_OFFSET_BYTES: usize = 0x4A80;
const PIXEL_PLASMA_PAYLOAD_OFFSET_BYTES: usize = 0x4C00;
const PIXEL_PLASMA_IDD_BYTES: usize = 8 * core::mem::size_of::<u32>();
const PIXEL_PLASMA_CROSS_THREAD_BYTES: usize = 128;
const PIXEL_PLASMA_PER_THREAD_BYTES: usize = 96;
const PIXEL_PLASMA_INDIRECT_BYTES: usize =
    PIXEL_PLASMA_CROSS_THREAD_BYTES + PIXEL_PLASMA_PER_THREAD_BYTES;
const PIXEL_PLASMA_PRE_MARKER_SLOT: usize = 31;
const PIXEL_PLASMA_POST_MARKER_SLOT: usize = 30;
const PIXEL_PLASMA_PRE_MARKER: u32 = 0xC0DE_A801;
const PIXEL_PLASMA_POST_MARKER: u32 = 0xC0DE_A802;

// A UI4 compute producer may be queued behind the compositor on RCS0. In
// particular, the first use of each primary swap buffer seeds and composes a
// full scanout-sized surface, which is deliberately allowed to take much
// longer than one 33 ms preview cadence. This timeout proves retirement and
// ownership transfer; it is not a frame-time target. Returning early after a
// successful submit would let UI4 cancel/reuse a lease while the GPU can still
// write it.
const UI4_COMPUTE_PRODUCER_RETIRE_TIMEOUT_MS: u64 = 1_000;
const FONT_OUTLINE_MESH_IDD_OFFSET_BYTES: usize = 0x4E00;
const FONT_OUTLINE_MESH_BINDING_TABLE_OFFSET_BYTES: usize = 0x4E40;
const FONT_OUTLINE_MESH_SRC_SURFACE_STATE_OFFSET_BYTES: usize = 0x4E80;
const FONT_OUTLINE_MESH_DST_SURFACE_STATE_OFFSET_BYTES: usize = 0x4EC0;
const FONT_OUTLINE_MESH_PAYLOAD_OFFSET_BYTES: usize = 0x5000;
const FONT_OUTLINE_MESH_IDD_BYTES: usize = 8 * core::mem::size_of::<u32>();
const FONT_OUTLINE_MESH_CROSS_THREAD_BYTES: usize = 128;
const FONT_OUTLINE_MESH_PER_THREAD_BYTES: usize = 96;
const FONT_OUTLINE_MESH_INDIRECT_BYTES: usize =
    FONT_OUTLINE_MESH_CROSS_THREAD_BYTES + FONT_OUTLINE_MESH_PER_THREAD_BYTES;
const FONT_OUTLINE_COVERAGE_R8_CROSS_THREAD_BYTES: usize = 128;
const FONT_OUTLINE_COVERAGE_R8_PER_THREAD_BYTES: usize = 96;
const FONT_OUTLINE_COVERAGE_R8_INDIRECT_BYTES: usize =
    FONT_OUTLINE_COVERAGE_R8_CROSS_THREAD_BYTES + FONT_OUTLINE_COVERAGE_R8_PER_THREAD_BYTES;
const FONT_OUTLINE_MESH_PRE_MARKER_SLOT: usize = 33;
const FONT_OUTLINE_MESH_POST_MARKER_SLOT: usize = 32;
const FONT_OUTLINE_MESH_PRE_MARKER: u32 = 0xC0DE_F701;
const FONT_OUTLINE_MESH_POST_MARKER: u32 = 0xC0DE_F702;
const RGBA8_SCANOUT_RELEASE_MARKER_SLOT: usize = 34;
const RGBA8_SCANOUT_RELEASE_MARKER: u32 = 0xC0DE_D102;
const FONT_OUTLINE_MESH_RESULT_MAGIC_BASE: u32 = 0xF07E_CA00;
const FONT_OUTLINE_MESH_RESULT_DONE: u32 = 0xC001_D00D;
const FONT_OUTLINE_MESH_LAYOUT_VERSION: u32 = 2;
const FONT_OUTLINE_MESH_VERTEX_DWORD_OFFSET: u32 = 64;
const FONT_OUTLINE_MESH_INDEX_DWORD_OFFSET: u32 = 8192;
const FONT_OUTLINE_MESH_MAX_OPS: usize = CLEAR_RECT_TEST_BYTES / (8 * core::mem::size_of::<u32>());
const FONT_OUTLINE_MESH_MAX_VERTICES: u32 = 3072;
const FONT_OUTLINE_MESH_MAX_INDICES: u32 = 4096;
pub(crate) const FONT_OUTLINE_STAGE_AUDIT: u32 = 1;
pub(crate) const FONT_OUTLINE_STAGE_FLATTEN: u32 = 2;
pub(crate) const FONT_OUTLINE_STAGE_STROKE_MESH: u32 = 3;
pub(crate) const PIXEL_PLASMA_FLAG_VIGNETTE: u32 = 1 << 0;
pub(crate) const PIXEL_PLASMA_FLAG_RINGS: u32 = 1 << 1;
pub(crate) const PIXEL_PLASMA_FLAG_SCANLINE: u32 = 1 << 2;
pub(crate) const PIXEL_PLASMA_FLAG_FIELD_PALETTE: u32 = 1 << 3;

const COPY_RECT_PRE_MARKER_SLOT: usize = 5;
const COPY_RECT_POST_MARKER_SLOT: usize = 4;
const COPY_RECT_PRE_MARKER: u32 = 0xC0DE_A701;
const COPY_RECT_POST_MARKER: u32 = 0xC0DE_A702;

const CLEAR_RECT_IDD_OFFSET_BYTES: usize = 0x300;
const CLEAR_RECT_BINDING_TABLE_OFFSET_BYTES: usize = 0x340;
const CLEAR_RECT_SURFACE_STATE_OFFSET_BYTES: usize = 0x380;
const CLEAR_RECT_PAYLOAD_OFFSET_BYTES: usize = 0x500;
const CLEAR_RECT_IDD_BYTES: usize = 8 * core::mem::size_of::<u32>();
const CLEAR_RECT_SURFACE_STATE_DWORDS: usize = 16;
const CLEAR_RECT_CROSS_THREAD_BYTES: usize = 96;
const CLEAR_RECT_PER_THREAD_BYTES: usize = 96;
const CLEAR_RECT_INDIRECT_BYTES: usize =
    CLEAR_RECT_CROSS_THREAD_BYTES + CLEAR_RECT_PER_THREAD_BYTES;
const CLEAR_RECT_TEST_BYTES: usize = 16 * 1024;
const CLEAR_RECT_PRE_MARKER_SLOT: usize = 3;
const CLEAR_RECT_POST_MARKER_SLOT: usize = 2;
const CLEAR_RECT_PRE_MARKER: u32 = 0xC0DE_C701;
const CLEAR_RECT_POST_MARKER: u32 = 0xC0DE_C702;

const SURFTYPE_BUFFER: u32 = 4;
const SURFACE_FORMAT_RAW: u32 = 0x1FF;

const DIRECT_RCS_ENABLED: bool = true;
const DIRECT_RCS_RING_BYTES: usize = 4096;
const DIRECT_RCS_CONTEXT_BYTES: usize = 22 * 4096;
const DIRECT_RCS_BATCH_BYTES: usize = 256 * 1024;
const _: () = assert!(
    512 + GLYPH_MASK_BATCH_MAX_LAYERS * 27
        < GLYPH_MASK_BATCH_STATE_BASE_OFFSET_BYTES / core::mem::size_of::<u32>()
);
const _: () = assert!(
    GLYPH_MASK_BATCH_STATE_BASE_OFFSET_BYTES
        + GLYPH_MASK_BATCH_MAX_LAYERS * GLYPH_MASK_BATCH_STATE_BLOCK_BYTES
        <= GLYPH_MASK_BATCH_PAYLOAD_BASE_OFFSET_BYTES
);
const _: () = assert!(
    GLYPH_MASK_BATCH_PAYLOAD_BASE_OFFSET_BYTES
        + GLYPH_MASK_BATCH_MAX_LAYERS * GLYPH_MASK_INDIRECT_BYTES
        <= DIRECT_RCS_BATCH_BYTES
);
const DIRECT_RCS_RESULT_BYTES: usize = 4096;
const DIRECT_RCS_PPGTT_PT_COUNT: usize = 512;
const DIRECT_RCS_PPGTT_BYTES: usize = (3 + DIRECT_RCS_PPGTT_PT_COUNT) * 4096;
pub(crate) const DIRECT_RCS_PPGTT_LIMIT_BYTES: u64 =
    (DIRECT_RCS_PPGTT_PT_COUNT as u64) * 512 * 4096;
const DIRECT_RCS_LRC_STATE_OFFSET_DWORDS: usize = 4096 / core::mem::size_of::<u32>();
const DIRECT_RCS_BATCH_START_DWORDS: usize = 4;
// GuC registrations require stable HWLRCAs. Keep the GPGPU context window
// distinct from render/font's 0x0080_0000 window instead of remapping another
// allocation underneath the same registered context.
const DIRECT_RCS_GPU_VA_RING_BASE: u64 = 0x01B0_0000;
const DIRECT_RCS_GPU_VA_CONTEXT_BASE: u64 = 0x01B1_0000;
const DIRECT_RCS_GPU_VA_RESULT_BASE: u64 = 0x01B4_0000;
const DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE: u64 = 0x0089_0000;

const DIRECT_RCS_GPU_VA_CANVAS3D_OUT_BASE: u64 = 0x008F_0000;
const DIRECT_RCS_GPU_VA_CANVAS3D_TMP_BASE: u64 = 0x0090_0000;

const DIRECT_RCS_GPU_VA_FONT_COVERAGE_OPS_BASE: u64 = 0x0440_0000;
const DIRECT_RCS_FONT_COVERAGE_OPS_WINDOW_BYTES: usize = 4 * 1024 * 1024;
const DIRECT_RCS_FONT_COVERAGE_MASK_MAX_BYTES: usize = 16 * 1024 * 1024;
// Persistent masks must not alias one another in the direct-RCS PPGTT.  The
// first implementation remapped every simultaneously-live color layer at one
// fixed VA; cached translations could then read or write a different layer's
// physical allocation.  This range is private to the direct-RCS address space
// (the render context may independently use the same numeric addresses).
const DIRECT_RCS_GPU_VA_FONT_COVERAGE_BASE: u64 = 0x0A00_0000;
const DIRECT_RCS_GPU_VA_FONT_COVERAGE_PRIMARY_LIMIT: u64 = 0x0D00_0000;
const DIRECT_RCS_GPU_VA_FONT_COVERAGE_SECONDARY_BASE: u64 = 0x0E00_0000;
const DIRECT_RCS_GPU_VA_FONT_COVERAGE_LIMIT: u64 = 0x1000_0000;
const _: () = assert!(DIRECT_RCS_GPU_VA_FONT_COVERAGE_BASE.is_multiple_of(4096));
const _: () = assert!(DIRECT_RCS_GPU_VA_FONT_COVERAGE_LIMIT.is_multiple_of(4096));
const _: () =
    assert!(DIRECT_RCS_GPU_VA_FONT_COVERAGE_BASE < DIRECT_RCS_GPU_VA_FONT_COVERAGE_PRIMARY_LIMIT);
const _: () = assert!(
    DIRECT_RCS_GPU_VA_FONT_COVERAGE_PRIMARY_LIMIT < DIRECT_RCS_GPU_VA_FONT_COVERAGE_SECONDARY_BASE
);
const _: () =
    assert!(DIRECT_RCS_GPU_VA_FONT_COVERAGE_SECONDARY_BASE < DIRECT_RCS_GPU_VA_FONT_COVERAGE_LIMIT);
const _: () = assert!(DIRECT_RCS_GPU_VA_FONT_COVERAGE_LIMIT <= DIRECT_RCS_PPGTT_LIMIT_BYTES);
const DIRECT_RCS_GPU_VA_BATCH_BASE: u64 = 0x01C0_0000;

// A compositor submission is intentionally allowed to remain in flight while
// the ordinary GPGPU client services video, fonts, and application compute.
// These GGTT addresses back a distinct HWLRCA/ring/batch/result set; the
// compositor also owns a distinct PPGTT root and vGPU principal below.
const UI4_COMPOSITOR_RCS_GPU_VA_RING_BASE: u64 = 0x01D0_0000;
const UI4_COMPOSITOR_RCS_GPU_VA_CONTEXT_BASE: u64 = 0x01D1_0000;
const UI4_COMPOSITOR_RCS_GPU_VA_RESULT_BASE: u64 = 0x01D4_0000;
const UI4_COMPOSITOR_RCS_GPU_VA_BATCH_BASE: u64 = 0x01E0_0000;

const DIRECT_RCS_SMOKE_POLL_ITERS: usize = 262_144;
const DIRECT_RCS_TIMEOUT_POLL_PAUSE_ITERS: usize = 64;

static COPY_RECT_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static RESOLVE_TILE64_MSAA4_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static FILL_RECT_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static FILL_RECT_WORKLIST_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static GRADIENT_RECT_WORKLIST_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> =
    Mutex::new(None);

static ALPHA_BLEND_WORKLIST_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static GLYPH_MASK_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static PRESENT_RGBA8_TO_PRIMARY_XRGB_RECT_UPLOAD: Mutex<Option<UploadedKernelArtifact>> =
    Mutex::new(None);
static UI4_NV12_YTILE_TO_PRIMARY_XRGB_UPLOAD: Mutex<Option<UploadedKernelArtifact>> =
    Mutex::new(None);
static UI4_NV12_TILE64_TO_RGBA8_FRAME_UPLOAD: Mutex<Option<UploadedKernelArtifact>> =
    Mutex::new(None);
static SPRITE64_WORKLIST_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static SPRITE_QUAD_WORKLIST_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static UI4_COMPOSE_LAYERS_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static MANDEL64_WORKLIST_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static CANVAS3D_PROJECT_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static CANVAS3D_TRANSFORM_Q16_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static CANVAS3D_CLIP_BOX_Q16_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static CANVAS3D_PLANE_SAMPLE_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static CANVAS3D_PLANE_FILL_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static CANVAS3D_PLANE_PATCH_FILL_CUT_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> =
    Mutex::new(None);
static CANVAS3D_PLANE_PATCH_WORKLIST_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> =
    Mutex::new(None);
static SKYBOX_SAMPLE_RGB565_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static CHART_SINE_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static PIXEL_PLASMA_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static FONT_OUTLINE_MESH_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static FONT_OUTLINE_COVERAGE_R8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static FONT_COVERAGE_GPU_VA_CURSOR: AtomicU64 =
    AtomicU64::new(DIRECT_RCS_GPU_VA_FONT_COVERAGE_BASE);
static FONT_COVERAGE_GPU_VA_FREE: Mutex<Vec<(u64, u64)>> = Mutex::new(Vec::new());
static FONT_OUTLINE_COVERAGE_R8_SELF_TEST: Once<bool> = Once::new();
static DIRECT_RCS_STATE: Mutex<Option<DirectRcsState>> = Mutex::new(None);
static UI4_COMPOSITOR_RCS_STATE: Mutex<Option<DirectRcsState>> = Mutex::new(None);

static GPGPU_RECT_WORKLIST_DESC: Mutex<Option<GpgpuRectWorklistDescBuffer>> = Mutex::new(None);
static GPGPU_MANDEL64_WORKLIST_DESC: Mutex<Option<GpgpuRectWorklistDescBuffer>> = Mutex::new(None);
static GPGPU_SPRITE_QUAD_WORKLIST_DESC: Mutex<Option<GpgpuRectWorklistDescBuffer>> =
    Mutex::new(None);
static UI4_COMPOSITOR_SPRITE_QUAD_DESC: Mutex<Option<GpgpuRectWorklistDescBuffer>> =
    Mutex::new(None);
static RECT_WORKLIST_DESC_SUBMIT_LOCK: Mutex<()> = Mutex::new(());

static DIRECT_RCS_SUBMIT_LOCK: Mutex<()> = Mutex::new(());
static DIRECT_RCS_CONTEXT_QUARANTINED: AtomicBool = AtomicBool::new(false);
static DIRECT_RCS_SCANOUT_PPGTT_LOGGED: AtomicBool = AtomicBool::new(false);
static DIRECT_RCS_SUBMIT_RUNTIME: Mutex<DirectRcsSubmitRuntime> =
    Mutex::new(DirectRcsSubmitRuntime::new());
static UI4_COMPOSITOR_RUNTIME: Mutex<Ui4CompositorRuntime> =
    Mutex::new(Ui4CompositorRuntime::new());

static PRESENT_RGBA8_TO_PRIMARY_XRGB_LOG_SEQ: AtomicU64 = AtomicU64::new(0);

static COPY_RECT_2D_INCOMPLETE_SEQ: AtomicU64 = AtomicU64::new(0);
static FILL_RECT_2D_INCOMPLETE_SEQ: AtomicU64 = AtomicU64::new(0);

static RESOLVE_TILE64_MSAA4_INCOMPLETE_SEQ: AtomicU64 = AtomicU64::new(0);
static FONT_OUTLINE_COVERAGE_R8_INCOMPLETE_SEQ: AtomicU64 = AtomicU64::new(0);
static GLYPH_MASK_BATCH_INCOMPLETE_SEQ: AtomicU64 = AtomicU64::new(0);

static FILL_RECT_WORKLIST_RAN: AtomicBool = AtomicBool::new(false);

static SPRITE_QUAD_WORKLIST_RAN: AtomicBool = AtomicBool::new(false);
static FILL_RECT_WORKLIST_OK: AtomicBool = AtomicBool::new(false);

static SPRITE_QUAD_WORKLIST_OK: AtomicBool = AtomicBool::new(false);

static SPRITE_QUAD_WORKLIST_SUBMIT_FAIL_LOGS: AtomicU32 = AtomicU32::new(0);

static DIRECT_RCS_SUBMIT_COUNTER: AtomicU32 = AtomicU32::new(0);
static DIRECT_RCS_TIMEOUT_POLL_PROBE_LOGGED: AtomicBool = AtomicBool::new(false);

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuActivitySnapshot {
    pub(crate) available: bool,
    pub(crate) direct_rcs_enabled: bool,
    pub(crate) submit_seq: u32,
    pub(crate) ring_head: u32,
    pub(crate) ring_tail: u32,
    pub(crate) acthd: u32,
    pub(crate) ipeir: u32,
    pub(crate) ipehr: u32,
    pub(crate) eir: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct CopyRectRgba8Params {
    pub(crate) src_gpu: u64,
    pub(crate) dst_gpu: u64,
    pub(crate) src_pitch_bytes: u32,
    pub(crate) dst_pitch_bytes: u32,
    pub(crate) src_x: u32,
    pub(crate) src_y: u32,
    pub(crate) dst_x: u32,
    pub(crate) dst_y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct Ui4Nv12Tile64ToPrimaryXrgbParams {
    pub(crate) nv12_gpu: u64,
    pub(crate) base_gpu: u64,
    pub(crate) dst_gpu: u64,
    pub(crate) src_pitch_bytes: u32,
    pub(crate) src_uv_offset: u32,
    pub(crate) base_pitch_bytes: u32,
    pub(crate) dst_pitch_bytes: u32,
    pub(crate) output_width: u32,
    pub(crate) output_height: u32,
    pub(crate) content_dst_x: u32,
    pub(crate) content_dst_y: u32,
    pub(crate) content_width: u32,
    pub(crate) content_height: u32,
    pub(crate) source_x: u32,
    pub(crate) source_y: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct Mandel64WorklistRgba8Desc {
    pub(crate) src_xy: u32,
    pub(crate) dst_xy: u32,
    pub(crate) flags: u32,
    pub(crate) color_rgba: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct Mandel64WorklistRgba8Params {
    pub(crate) dst_gpu: u64,
    pub(crate) desc_gpu: u64,
    pub(crate) dst_pitch_bytes: u32,
    pub(crate) desc_base: u32,
    pub(crate) desc_count: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct FillRectWorklistRgba8Desc {
    pub(crate) dst_xy: u32,
    pub(crate) size: u32,
    pub(crate) color_rgba: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct FillRectWorklistRgba8Params {
    pub(crate) dst_gpu: u64,
    pub(crate) desc_gpu: u64,
    pub(crate) dst_pitch_bytes: u32,
    pub(crate) desc_base: u32,
    pub(crate) desc_count: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuSpriteQuadWorklistDesc {
    pub(crate) c0_x: f32,
    pub(crate) c0_y: f32,
    pub(crate) c0_u: f32,
    pub(crate) c0_v: f32,
    pub(crate) c1_x: f32,
    pub(crate) c1_y: f32,
    pub(crate) c1_u: f32,
    pub(crate) c1_v: f32,
    pub(crate) c2_x: f32,
    pub(crate) c2_y: f32,
    pub(crate) c2_u: f32,
    pub(crate) c2_v: f32,
    pub(crate) c3_x: f32,
    pub(crate) c3_y: f32,
    pub(crate) c3_u: f32,
    pub(crate) c3_v: f32,
    pub(crate) color_rgba: u32,
    pub(crate) flags: u32,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct GpgpuSpriteQuadWorklistRun<'a> {
    pub(crate) src: GpgpuRgba8Surface,
    pub(crate) descs: &'a [GpgpuSpriteQuadWorklistDesc],
}

/// One axis-aligned premultiplied RGBA source in the stable UI4 compositor
/// contract.  Unlike the exploratory sprite worklist, every layer in a frame
/// is consumed by one kernel invocation and one walker.
#[derive(Copy, Clone, Debug)]
pub(crate) struct GpgpuUi4ComposeLayer {
    pub(crate) src: GpgpuRgba8Surface,
    pub(crate) dst_x: i32,
    pub(crate) dst_y: i32,
    pub(crate) dst_width: u32,
    pub(crate) dst_height: u32,
    pub(crate) opacity: u8,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
struct GpgpuUi4ComposeLayerDesc {
    src_gpu_lo: u32,
    src_gpu_hi: u32,
    src_pitch_bytes: u32,
    src_width: u32,
    src_height: u32,
    dst_x: i32,
    dst_y: i32,
    dst_width: u32,
    dst_height: u32,
    opacity: u32,
    flags: u32,
    reserved: u32,
}

#[derive(Copy, Clone, Debug)]
struct Ui4ComposeLayersParams {
    base_gpu: u64,
    dst_gpu: u64,
    layers_gpu: u64,
    base_pitch_bytes: u32,
    dst_pitch_bytes: u32,
    dst_width: u32,
    dst_height: u32,
    damage_x: u32,
    damage_y: u32,
    damage_width: u32,
    damage_height: u32,
    layer_count: u32,
    flags: u32,
}

pub(crate) const UI4_COMPOSE_FLAG_BASE_XRGB: u32 = 1 << 0;
pub(crate) const UI4_COMPOSE_FLAG_DEST_XRGB: u32 = 1 << 1;

pub(crate) const SPRITE_QUAD_WORKLIST_FLAG_SRC_OVER: u32 = 1 << 0;
pub(crate) const SPRITE_QUAD_WORKLIST_FLAG_PREMUL_SRC: u32 = 1 << 1;
pub(crate) const SPRITE_QUAD_WORKLIST_FLAG_CLEAR: u32 = 1 << 2;
pub(crate) const SPRITE_QUAD_WORKLIST_FLAG_SOURCE_XRGB: u32 = 1 << 3;
pub(crate) const SPRITE_QUAD_WORKLIST_FLAG_DEST_XRGB: u32 = 1 << 4;

pub(crate) const fn sprite_quad_worklist_max_descs() -> usize {
    SPRITE_QUAD_WORKLIST_MAX_DESCS
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct SpriteQuadWorklistRgba8Params {
    pub(crate) src_gpu: u64,
    pub(crate) dst_gpu: u64,
    pub(crate) desc_gpu: u64,
    pub(crate) src_pitch_bytes: u32,
    pub(crate) dst_pitch_bytes: u32,
    pub(crate) src_width: u32,
    pub(crate) src_height: u32,
    pub(crate) dst_width: u32,
    pub(crate) dst_height: u32,
    pub(crate) desc_base: u32,
    pub(crate) desc_count: u32,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct SkyboxSampleRgb565Params {
    pub(crate) sky_gpu: u64,
    pub(crate) dst_gpu: u64,
    pub(crate) sky_pitch_bytes: u32,
    pub(crate) sky_width: u32,
    pub(crate) sky_height: u32,
    pub(crate) dst_pitch_bytes: u32,
    pub(crate) dst_width: u32,
    pub(crate) dst_height: u32,
    pub(crate) rect_x: u32,
    pub(crate) rect_y: u32,
    pub(crate) rect_width: u32,
    pub(crate) rect_height: u32,
    pub(crate) right_x: f32,
    pub(crate) right_y: f32,
    pub(crate) right_z: f32,
    pub(crate) up_x: f32,
    pub(crate) up_y: f32,
    pub(crate) up_z: f32,
    pub(crate) forward_x: f32,
    pub(crate) forward_y: f32,
    pub(crate) forward_z: f32,
    pub(crate) aspect_tan_half_fov_y: f32,
    pub(crate) tan_half_fov_y: f32,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct ChartSineRgba8Params {
    pub(crate) dst_gpu: u64,
    pub(crate) dst_pitch_bytes: u32,
    pub(crate) dst_width: u32,
    pub(crate) dst_height: u32,
    pub(crate) rect_x: u32,
    pub(crate) rect_y: u32,
    pub(crate) rect_width: u32,
    pub(crate) rect_height: u32,
    pub(crate) phase: f32,
    pub(crate) cycles: f32,
    pub(crate) amplitude: f32,
    pub(crate) line_width_px: f32,
    pub(crate) background_rgba: u32,
    pub(crate) minor_grid_rgba: u32,
    pub(crate) major_grid_rgba: u32,
    pub(crate) axis_rgba: u32,
    pub(crate) line_rgba: u32,
    pub(crate) glow_rgba: u32,
    pub(crate) flags: u32,
}

impl ChartSineRgba8Params {
    pub(crate) const fn scope_defaults(phase: f32, flags: u32) -> Self {
        Self {
            dst_gpu: 0,
            dst_pitch_bytes: 0,
            dst_width: 0,
            dst_height: 0,
            rect_x: 0,
            rect_y: 0,
            rect_width: 0,
            rect_height: 0,
            phase,
            cycles: 3.0,
            amplitude: 0.34,
            line_width_px: 2.25,
            background_rgba: 0xFF1F_1107,
            minor_grid_rgba: 0xFF3C_2610,
            major_grid_rgba: 0xFF63_451E,
            axis_rgba: 0xFF98_7D62,
            line_rgba: 0xFFE3_FF86,
            glow_rgba: 0xFFE8_D718,
            flags,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct PixelPlasmaRgba8Params {
    pub(crate) dst_gpu: u64,
    pub(crate) dst_pitch_bytes: u32,
    pub(crate) dst_width: u32,
    pub(crate) dst_height: u32,
    pub(crate) rect_x: u32,
    pub(crate) rect_y: u32,
    pub(crate) rect_width: u32,
    pub(crate) rect_height: u32,
    pub(crate) time: f32,
    pub(crate) spatial_scale: f32,
    pub(crate) intensity: f32,
    pub(crate) low_rgba: u32,
    pub(crate) mid_rgba: u32,
    pub(crate) high_rgba: u32,
    pub(crate) flags: u32,
}

impl PixelPlasmaRgba8Params {
    pub(crate) const fn demo_defaults(time: f32, flags: u32) -> Self {
        Self {
            dst_gpu: 0,
            dst_pitch_bytes: 0,
            dst_width: 0,
            dst_height: 0,
            rect_x: 0,
            rect_y: 0,
            rect_width: 0,
            rect_height: 0,
            time,
            spatial_scale: 1.0,
            intensity: 1.0,
            low_rgba: 0xFF24_0A08,
            mid_rgba: 0xFFE6_D214,
            high_rgba: 0xFF2D_55FF,
            flags,
        }
    }
}

#[derive(Copy, Clone, Debug)]
struct FontOutlineMeshParams {
    src_gpu: u64,
    dst_gpu: u64,
    op_count: u32,
    stage: u32,
    subdivisions: u32,
    max_vertices: u32,
    max_indices: u32,
    scale: f32,
    origin_x: f32,
    origin_y: f32,
    stroke_half_width: f32,
}

#[derive(Copy, Clone, Debug)]
struct FontOutlineCoverageR8Params {
    ops_gpu: u64,
    mask_gpu: u64,
    op_count: u32,
    subdivisions: u32,
    mask_pitch_bytes: u32,
    mask_width: u32,
    mask_height: u32,
    rect_x: u32,
    rect_y: u32,
    rect_width: u32,
    rect_height: u32,
    optical_bias_px: f32,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuFontOutlineMesh {
    pub(crate) storage_phys: u64,
    pub(crate) storage_bytes: usize,
    pub(crate) vertex_offset_bytes: u32,
    pub(crate) vertex_count: u32,
    pub(crate) vertex_stride: u32,
    pub(crate) index_offset_bytes: u32,
    pub(crate) index_count: u32,
    pub(crate) min_x: f32,
    pub(crate) min_y: f32,
    pub(crate) max_x: f32,
    pub(crate) max_y: f32,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuFontOutlineProbeResult {
    pub(crate) available: bool,
    pub(crate) forcewake_ok: bool,
    pub(crate) mapped_ok: bool,
    pub(crate) ppgtt_ok: bool,
    pub(crate) kernel_ppgtt_ok: bool,
    pub(crate) src_ppgtt_ok: bool,
    pub(crate) dst_ppgtt_ok: bool,
    pub(crate) batch_ok: bool,
    pub(crate) submitted: bool,
    pub(crate) retired: bool,
    pub(crate) kernel_done: bool,
    pub(crate) ok: bool,
    pub(crate) retire_ms: u64,
    pub(crate) op_count: u32,
    pub(crate) move_count: u32,
    pub(crate) line_count: u32,
    pub(crate) quad_count: u32,
    pub(crate) cubic_count: u32,
    pub(crate) close_count: u32,
    pub(crate) vertices: u32,
    pub(crate) segments: u32,
    pub(crate) indices: u32,
    pub(crate) generated_mesh: Option<GpgpuFontOutlineMesh>,
    pub(crate) checksum: u32,
    pub(crate) expected_checksum: u32,
    pub(crate) invalid: u32,
    pub(crate) truncated: bool,
    pub(crate) indices_in_range: bool,
    pub(crate) min_x: f32,
    pub(crate) min_y: f32,
    pub(crate) max_x: f32,
    pub(crate) max_y: f32,
    pub(crate) pre_marker: u32,
    pub(crate) post_marker: u32,
    pub(crate) report_marker: u32,
    pub(crate) done_marker: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct FillRectRgba8Params {
    pub(crate) dst_gpu: u64,
    pub(crate) dst_pitch_bytes: u32,
    pub(crate) dst_x: u32,
    pub(crate) dst_y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) color_rgba: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct FillRect2dDispatch {
    group_x: u32,
    group_y: u32,
    right_mask: u32,
}

const fn fill_rect_2d_dispatch(width: u32, height: u32) -> Option<FillRect2dDispatch> {
    if width == 0 || height == 0 {
        return None;
    }
    let full_groups = width / FILL_RECT_PIXELS_PER_GROUP_X;
    let tail_pixels = width % FILL_RECT_PIXELS_PER_GROUP_X;
    let group_x = full_groups + if tail_pixels == 0 { 0 } else { 1 };
    // GPGPU_WALKER's RightExecutionMask applies to every SIMD hardware
    // thread, not just the final X workgroup.  A tail-derived mask therefore
    // removes the same lanes from every 16-pixel block and turns glyphs into
    // periodic vertical fragments.  All callers using this dispatch have an
    // explicit x/width guard, so run every group with all lanes enabled and
    // let the final padded lanes return without touching the surface.
    let right_mask = GPGPU_WALKER_SIMD16_MASK;
    Some(FillRect2dDispatch {
        group_x,
        group_y: height,
        right_mask,
    })
}

const fn copy_rect_2d_dispatch(width: u32, height: u32) -> Option<FillRect2dDispatch> {
    if width == 0 || height == 0 {
        return None;
    }
    // copy_rect_rgba8 handles two adjacent pixels per SIMD lane, whereas the
    // other 2D kernels handle one. Dispatch in work items, not pixels.
    let work_item_width = width.div_ceil(COPY_RECT_PIXELS_PER_LANE);
    fill_rect_2d_dispatch(work_item_width, height)
}

const fn sprite_quad_2d_dispatch(width: u32, height: u32) -> Option<FillRect2dDispatch> {
    let Some(mut dispatch) = fill_rect_2d_dispatch(width, height) else {
        return None;
    };
    dispatch.group_y = height.div_ceil(SPRITE_QUAD_WORKLIST_TILE_ROWS);
    Some(dispatch)
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct SpriteQuadDescriptorDispatch {
    walker: FillRect2dDispatch,
    global_x: u32,
    global_tile_y: u32,
}

fn sprite_quad_descriptor_dispatch(
    desc: GpgpuSpriteQuadWorklistDesc,
    dst_width: u32,
    dst_height: u32,
) -> Option<SpriteQuadDescriptorDispatch> {
    if dst_width == 0 || dst_height == 0 {
        return None;
    }
    let xs = [desc.c0_x, desc.c1_x, desc.c2_x, desc.c3_x];
    let ys = [desc.c0_y, desc.c1_y, desc.c2_y, desc.c3_y];
    if xs.iter().chain(ys.iter()).any(|value| !value.is_finite()) {
        return None;
    }
    let mut left = xs[0];
    let mut right = xs[0];
    let mut top = ys[0];
    let mut bottom = ys[0];
    for value in xs.into_iter().skip(1) {
        left = left.min(value);
        right = right.max(value);
    }
    for value in ys.into_iter().skip(1) {
        top = top.min(value);
        bottom = bottom.max(value);
    }

    let min_x = (libm::floorf(left).max(0.0) as u32).min(dst_width - 1);
    let max_x = (libm::ceilf(right).max(0.0) as u32).min(dst_width - 1);
    let min_y = (libm::floorf(top).max(0.0) as u32).min(dst_height - 1);
    let max_y = (libm::ceilf(bottom).max(0.0) as u32).min(dst_height - 1);
    if max_x < min_x || max_y < min_y || right < 0.0 || bottom < 0.0 {
        return None;
    }

    let global_tile_y = min_y / SPRITE_QUAD_WORKLIST_TILE_ROWS;
    let final_tile_y = max_y / SPRITE_QUAD_WORKLIST_TILE_ROWS;
    Some(SpriteQuadDescriptorDispatch {
        walker: FillRect2dDispatch {
            group_x: max_x.saturating_sub(min_x).saturating_add(1).div_ceil(16),
            group_y: final_tile_y.saturating_sub(global_tile_y).saturating_add(1),
            right_mask: GPGPU_WALKER_SIMD16_MASK,
        },
        global_x: min_x,
        global_tile_y,
    })
}

const _: () = {
    let exact = fill_rect_2d_dispatch(16, 1).unwrap();
    assert!(exact.group_x == 1);
    assert!(exact.group_y == 1);
    assert!(exact.right_mask == GPGPU_WALKER_SIMD16_MASK);
    let tail = fill_rect_2d_dispatch(17, 3).unwrap();
    assert!(tail.group_x == 2);
    assert!(tail.group_y == 3);
    assert!(tail.right_mask == GPGPU_WALKER_SIMD16_MASK);
    let scanout = fill_rect_2d_dispatch(2560, 1440).unwrap();
    assert!(scanout.group_x == 160);
    assert!(scanout.group_y == 1440);
    assert!(scanout.right_mask == GPGPU_WALKER_SIMD16_MASK);
    let copy_exact = copy_rect_2d_dispatch(32, 1).unwrap();
    assert!(copy_exact.group_x == 1);
    let copy_tail = copy_rect_2d_dispatch(33, 3).unwrap();
    assert!(copy_tail.group_x == 2);
    assert!(copy_tail.group_y == 3);
    let sprite_scanout = sprite_quad_2d_dispatch(2560, 1440).unwrap();
    assert!(sprite_scanout.group_x == 160);
    assert!(sprite_scanout.group_y == 1440);
};

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct GpgpuPoint {
    pub(crate) x: i32,
    pub(crate) y: i32,
}

impl GpgpuPoint {
    pub(crate) const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct GpgpuRect {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl GpgpuRect {
    pub(crate) const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub(crate) const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuRgba8Surface {
    pub(crate) phys: u64,
    pub(crate) gpu: u64,
    pub(crate) bytes: usize,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pitch_bytes: u32,
}

impl GpgpuRgba8Surface {
    pub(crate) fn new(
        phys: u64,
        gpu: u64,
        bytes: usize,
        width: u32,
        height: u32,
        pitch_bytes: u32,
    ) -> Option<Self> {
        let surface = Self {
            phys,
            gpu,
            bytes,
            width,
            height,
            pitch_bytes,
        };
        if surface.is_valid() {
            Some(surface)
        } else {
            None
        }
    }

    pub(crate) fn is_valid(self) -> bool {
        if self.width == 0 || self.height == 0 {
            return false;
        }
        if (self.phys & 0xFFF) != 0 {
            return false;
        }
        let min_pitch = self
            .width
            .saturating_mul(core::mem::size_of::<u32>() as u32);
        if self.pitch_bytes < min_pitch {
            return false;
        }
        let Some(last_row) = (self.height as usize)
            .checked_sub(1)
            .and_then(|row| row.checked_mul(self.pitch_bytes as usize))
        else {
            return false;
        };
        let Some(min_bytes) = last_row.checked_add(min_pitch as usize) else {
            return false;
        };
        min_bytes <= self.bytes
    }

    pub(crate) const fn bounds(self) -> GpgpuRect {
        GpgpuRect::new(0, 0, self.width, self.height)
    }
}

/// Decoder-owned Xe media Tile64 NV12 storage mapped read-only by convention into the
/// compositor's private PPGTT.  The media engine's VA is only an opaque alias;
/// direct RCS installs its own PTEs for the same physical picture.
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuNv12Tile64Surface {
    pub(crate) phys: u64,
    pub(crate) gpu: u64,
    pub(crate) bytes: usize,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pitch_bytes: u32,
    pub(crate) uv_offset: u32,
}

impl GpgpuNv12Tile64Surface {
    pub(crate) fn new(
        phys: u64,
        gpu: u64,
        bytes: usize,
        width: u32,
        height: u32,
        pitch_bytes: u32,
        uv_offset: u32,
    ) -> Option<Self> {
        let surface = Self {
            phys,
            gpu,
            bytes,
            width,
            height,
            pitch_bytes,
            uv_offset,
        };
        surface.is_valid().then_some(surface)
    }

    pub(crate) fn is_valid(self) -> bool {
        if self.phys == 0
            || self.gpu == 0
            || !self.phys.is_multiple_of(4096)
            || !self.gpu.is_multiple_of(4096)
            || self.width == 0
            || self.height == 0
            || self.pitch_bytes < self.width
            || !self.pitch_bytes.is_multiple_of(256)
            || self.uv_offset == 0
            || !self.uv_offset.is_multiple_of(self.pitch_bytes)
        {
            return false;
        }
        let chroma_row = self.uv_offset / self.pitch_bytes;
        if !chroma_row.is_multiple_of(256) {
            return false;
        }
        let Some(total_rows) = chroma_row
            .checked_add(self.height.div_ceil(2))
            .map(|rows| rows.next_multiple_of(256))
        else {
            return false;
        };
        let Some(required) = u64::from(total_rows).checked_mul(u64::from(self.pitch_bytes)) else {
            return false;
        };
        required <= self.bytes as u64
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuRgb565Surface {
    pub(crate) phys: u64,
    pub(crate) gpu: u64,
    pub(crate) bytes: usize,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pitch_bytes: u32,
}

impl GpgpuRgb565Surface {
    pub(crate) fn new(
        phys: u64,
        gpu: u64,
        bytes: usize,
        width: u32,
        height: u32,
        pitch_bytes: u32,
    ) -> Option<Self> {
        let surface = Self {
            phys,
            gpu,
            bytes,
            width,
            height,
            pitch_bytes,
        };
        if surface.is_valid() {
            Some(surface)
        } else {
            None
        }
    }

    pub(crate) fn is_valid(self) -> bool {
        if self.width == 0 || self.height == 0 {
            return false;
        }
        if (self.phys & 0xFFF) != 0 {
            return false;
        }
        let min_pitch = self
            .width
            .saturating_mul(core::mem::size_of::<u16>() as u32);
        if self.pitch_bytes < min_pitch {
            return false;
        }
        let Some(last_row) = (self.height as usize)
            .checked_sub(1)
            .and_then(|row| row.checked_mul(self.pitch_bytes as usize))
        else {
            return false;
        };
        let Some(min_bytes) = last_row.checked_add(min_pitch as usize) else {
            return false;
        };
        min_bytes <= self.bytes
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuMask8Surface {
    pub(crate) phys: u64,
    pub(crate) gpu: u64,
    pub(crate) bytes: usize,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pitch_bytes: u32,
}

pub(crate) struct GpgpuOwnedMask8Surface {
    surface: GpgpuMask8Surface,
    virt: *mut u8,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct GpgpuMask8Audit {
    pub(crate) nonzero_pixels: usize,
    pub(crate) bounds: GpgpuRect,
}

unsafe impl Send for GpgpuOwnedMask8Surface {}
unsafe impl Sync for GpgpuOwnedMask8Surface {}

impl GpgpuOwnedMask8Surface {
    pub(crate) const fn surface(&self) -> GpgpuMask8Surface {
        self.surface
    }

    /// Read back the persistent mask once after generation.  This is a cold
    /// path integrity check, not part of frame composition.
    pub(crate) fn nonzero_audit(&self) -> Option<GpgpuMask8Audit> {
        if !self.surface.is_valid() || self.virt.is_null() {
            return None;
        }
        super::dma_flush(self.virt, self.surface.bytes);
        let mut min_x = self.surface.width;
        let mut min_y = self.surface.height;
        let mut max_x = 0u32;
        let mut max_y = 0u32;
        let mut nonzero_pixels = 0usize;
        for y in 0..self.surface.height {
            let row_offset = (y as usize).checked_mul(self.surface.pitch_bytes as usize)?;
            for x in 0..self.surface.width {
                let offset = row_offset.checked_add(x as usize)?;
                let coverage = unsafe { core::ptr::read_volatile(self.virt.add(offset)) };
                if coverage == 0 {
                    continue;
                }
                nonzero_pixels = nonzero_pixels.saturating_add(1);
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
        (nonzero_pixels != 0).then_some(GpgpuMask8Audit {
            nonzero_pixels,
            bounds: GpgpuRect::new(
                min_x as i32,
                min_y as i32,
                max_x.saturating_sub(min_x).saturating_add(1),
                max_y.saturating_sub(min_y).saturating_add(1),
            ),
        })
    }
}

impl Drop for GpgpuOwnedMask8Surface {
    fn drop(&mut self) {
        crate::dma::dealloc(self.virt, self.surface.bytes);
        recycle_font_coverage_gpu_va(self.surface.gpu, self.surface.bytes);
    }
}

impl GpgpuMask8Surface {
    pub(crate) fn new(
        phys: u64,
        gpu: u64,
        bytes: usize,
        width: u32,
        height: u32,
        pitch_bytes: u32,
    ) -> Option<Self> {
        let surface = Self {
            phys,
            gpu,
            bytes,
            width,
            height,
            pitch_bytes,
        };
        surface.is_valid().then_some(surface)
    }

    pub(crate) fn is_valid(self) -> bool {
        if self.width == 0 || self.height == 0 || self.pitch_bytes < self.width {
            return false;
        }
        if (self.phys & 0xFFF) != 0 {
            return false;
        }
        let Some(last_row) = (self.height as usize)
            .checked_sub(1)
            .and_then(|row| row.checked_mul(self.pitch_bytes as usize))
        else {
            return false;
        };
        let Some(min_bytes) = last_row.checked_add(self.width as usize) else {
            return false;
        };
        min_bytes <= self.bytes
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuGlyphMaskBlit {
    pub(crate) mask: GpgpuMask8Surface,
    pub(crate) mask_rect: GpgpuRect,
    pub(crate) dst: GpgpuRgba8Surface,
    pub(crate) dst_xy: GpgpuPoint,
    pub(crate) color_rgba: u32,
}

/// One independently positioned/colorized persistent R8 coverage layer.
/// The destination is supplied once for the complete scene-level batch.
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuGlyphMaskLayer {
    pub(crate) mask: GpgpuMask8Surface,
    pub(crate) mask_rect: GpgpuRect,
    pub(crate) dst_xy: GpgpuPoint,
    pub(crate) color_rgba: u32,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuGlyphMaskBatchResult {
    pub(crate) ok: bool,
    /// True once the command buffer reached the hardware submission boundary.
    /// An incomplete submitted batch must not be replayed over the same target.
    pub(crate) submitted: bool,
    pub(crate) requested_layers: usize,
    pub(crate) active_walkers: usize,
    pub(crate) submits: usize,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuSubmitStats {
    pub(crate) spans: usize,
    pub(crate) submits: usize,
    pub(crate) submit_ms: u64,
    pub(crate) total_ms: u64,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct GpgpuWorklistSubmitStats {
    pub(crate) descs: usize,
    pub(crate) walkers: usize,
    pub(crate) submits: usize,
    pub(crate) submit_ms: u64,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum GpgpuSubmissionOutcome {
    /// The request failed before crossing the hardware submission boundary.
    Unavailable,
    /// The post marker retired, so all destination writes are complete.
    Complete,
    /// Hardware accepted the request but its post marker did not retire.
    SubmittedIncomplete,
}

impl Default for GpgpuSubmissionOutcome {
    fn default() -> Self {
        Self::Unavailable
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuWorklistSubmitResult {
    pub(crate) stats: GpgpuWorklistSubmitStats,
    pub(crate) outcome: GpgpuSubmissionOutcome,
}

/// Opaque serial and executor submission for the one-deep persistent UI4
/// compositor queue.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct Ui4CompositorSubmission {
    serial: u64,
    gpu: crate::gpu::executor::KernelSubmission,
}

impl Ui4CompositorSubmission {
    /// Create a future for the exact vGPU timeline point backing this UI4 job.
    pub(crate) fn fence(self) -> crate::gpu::executor::GpuFence {
        self.gpu.fence()
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Ui4CompositorSubmitError {
    Busy,
    Unavailable,
    InvalidWorklist,
    SubmissionRejected,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Ui4CompositorCompletion {
    Pending,
    Complete(GpgpuWorklistSubmitStats),
    Failed,
    InvalidSubmission,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuSolidRect {
    pub(crate) rect: GpgpuRect,
    pub(crate) color_rgba: u32,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuShellMandel64WorklistResult {
    pub(crate) ok: bool,
    pub(crate) submitted: bool,
    pub(crate) marker: u32,
    pub(crate) requested: usize,
    pub(crate) descriptors: usize,
    pub(crate) walkers: usize,
    pub(crate) pixels: usize,
    pub(crate) submit_ms: u64,
    pub(crate) desc_gpu: u64,
    pub(crate) last_src_xy: GpgpuPoint,
    pub(crate) last_dst_xy: GpgpuPoint,
    /// Present only for a complete direct-scanout render whose final
    /// PIPE_CONTROL and post-sync marker retired for this exact allocation.
    pub(crate) release: Option<GpgpuRgba8ReleaseFence>,
}

/// Common result for a full-surface compute node that does not own
/// presentation. UI4 consumers decide whether and when to publish the frame.
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuRgba8KernelResult {
    pub(crate) ok: bool,
    pub(crate) submitted: bool,
    pub(crate) marker: u32,
    pub(crate) submit_ms: u64,
    /// Exact-surface producer release, minted only after the kernel's final
    /// cache-draining PIPE_CONTROL and post-sync marker have retired.
    pub(crate) release: Option<GpgpuRgba8ReleaseFence>,
}

/// Proof that one full-surface compute dispatch retired its producer-release
/// packet for one exact allocation. The fields stay private so consumers
/// cannot manufacture display eligibility from an address alone.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct GpgpuRgba8ReleaseFence {
    phys: u64,
    byte_len: usize,
    sequence: u64,
}

impl GpgpuRgba8ReleaseFence {
    pub(crate) const fn matches(self, phys: u64, byte_len: usize) -> bool {
        self.phys == phys && self.byte_len == byte_len
    }

    pub(crate) const fn sequence(self) -> u64 {
        self.sequence
    }
}

static GPGPU_RGBA8_RELEASE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Copy, Clone, Debug, Default)]
struct DirectRcsDispatchOutcome {
    submitted: bool,
    observed: u32,
}

/// Establish the final producer-to-display boundary for an RGBA8 allocation.
///
/// Earlier resolve/coverage/decorations may use different completion packets;
/// this dedicated batch remaps the exact destination PAT3/UC, drains HDC/L3
/// and render-target writes, and proves retirement with an ordered
/// PIPE_CONTROL post-sync cookie. No pixel shader or surface copy runs here.
pub(crate) fn release_rgba8_surface_for_scanout(dst: GpgpuRgba8Surface) -> GpgpuRgba8KernelResult {
    let started = direct_rcs_now_tick();
    if !dst.is_valid() {
        return GpgpuRgba8KernelResult::default();
    }
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        return GpgpuRgba8KernelResult::default();
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        return GpgpuRgba8KernelResult::default();
    };
    let prepared = direct_rcs_forcewake(dev)
        && direct_rcs_map_state(dev, state)
        && direct_rcs_init_ppgtt(state)
        && direct_rcs_map_ppgtt_scanout(state, dst.gpu, dst.phys, dst.bytes)
        && direct_rcs_encode_rgba8_scanout_release_batch(state);
    let submitted = prepared && direct_rcs_submit_batch(dev, state);
    let marker = if submitted {
        direct_rcs_poll_result_slot_timeout_ms(
            state,
            RGBA8_SCANOUT_RELEASE_MARKER_SLOT,
            RGBA8_SCANOUT_RELEASE_MARKER,
            UI4_COMPUTE_PRODUCER_RETIRE_TIMEOUT_MS,
        )
    } else {
        0
    };
    let ok = marker == RGBA8_SCANOUT_RELEASE_MARKER;
    GpgpuRgba8KernelResult {
        ok,
        submitted,
        marker,
        submit_ms: direct_rcs_elapsed_ms_since(started),
        release: ok.then(|| gpgpu_rgba8_release(dst)),
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuMandel64Placement {
    pub(crate) src_x: i32,
    pub(crate) src_y: i32,
    pub(crate) dst_x: i32,
    pub(crate) dst_y: i32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) view_height: u32,
    pub(crate) mirror_at_center: bool,
    pub(crate) iterations: u32,
}

#[derive(Copy, Clone, Debug)]
struct GpgpuRectWorklistDescBuffer {
    phys: u64,
    gpu: u64,
    virt: *mut u8,
    bytes: usize,
}

unsafe impl Send for GpgpuRectWorklistDescBuffer {}
unsafe impl Sync for GpgpuRectWorklistDescBuffer {}

#[derive(Copy, Clone, Debug)]
pub(crate) struct GpgpuKernelArtifact {
    pub(crate) name: &'static str,
    pub(crate) target: &'static str,
    pub(crate) bin: &'static [u8],
    pub(crate) spv: &'static [u8],
    pub(crate) bin_sha256: [u8; 32],
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct UploadedKernelArtifact {
    pub(crate) name: &'static str,
    pub(crate) target: &'static str,
    pub(crate) source: &'static str,
    pub(crate) gpu: u64,
    pub(crate) phys: u64,
    pub(crate) bytes: usize,
    pub(crate) mapped_bytes: usize,
    pub(crate) verified: bool,
    pub(crate) bin_sha256: [u8; 32],
}

unsafe impl Send for UploadedKernelArtifact {}
unsafe impl Sync for UploadedKernelArtifact {}

pub(crate) const COPY_RECT_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact {
    name: COPY_RECT_RGBA8_KERNEL_NAME,
    target: "adls",
    bin: COPY_RECT_RGBA8_ADLS_BIN,
    spv: COPY_RECT_RGBA8_ADLS_SPV,
    bin_sha256: COPY_RECT_RGBA8_ADLS_BIN_SHA256,
};

pub(crate) const RESOLVE_TILE64_MSAA4_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact {
        name: RESOLVE_TILE64_MSAA4_RGBA8_KERNEL_NAME,
        target: "adls",
        bin: RESOLVE_TILE64_MSAA4_RGBA8_ADLS_BIN,
        spv: RESOLVE_TILE64_MSAA4_RGBA8_ADLS_SPV,
        bin_sha256: RESOLVE_TILE64_MSAA4_RGBA8_ADLS_BIN_SHA256,
    };

pub(crate) const FILL_RECT_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact {
    name: FILL_RECT_RGBA8_KERNEL_NAME,
    target: "adls",
    bin: FILL_RECT_RGBA8_ADLS_BIN,
    spv: FILL_RECT_RGBA8_ADLS_SPV,
    bin_sha256: FILL_RECT_RGBA8_ADLS_BIN_SHA256,
};

pub(crate) const FILL_RECT_WORKLIST_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact {
        name: FILL_RECT_WORKLIST_RGBA8_KERNEL_NAME,
        target: "adls",
        bin: FILL_RECT_WORKLIST_RGBA8_ADLS_BIN,
        spv: FILL_RECT_WORKLIST_RGBA8_ADLS_SPV,
        bin_sha256: FILL_RECT_WORKLIST_RGBA8_ADLS_BIN_SHA256,
    };

pub(crate) const GRADIENT_RECT_WORKLIST_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact {
        name: GRADIENT_RECT_WORKLIST_RGBA8_KERNEL_NAME,
        target: "adls",
        bin: GRADIENT_RECT_WORKLIST_RGBA8_ADLS_BIN,
        spv: GRADIENT_RECT_WORKLIST_RGBA8_ADLS_SPV,
        bin_sha256: GRADIENT_RECT_WORKLIST_RGBA8_ADLS_BIN_SHA256,
    };

pub(crate) const ALPHA_BLEND_WORKLIST_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact {
        name: ALPHA_BLEND_WORKLIST_RGBA8_KERNEL_NAME,
        target: "adls",
        bin: ALPHA_BLEND_WORKLIST_RGBA8_ADLS_BIN,
        spv: ALPHA_BLEND_WORKLIST_RGBA8_ADLS_SPV,
        bin_sha256: ALPHA_BLEND_WORKLIST_RGBA8_ADLS_BIN_SHA256,
    };

pub(crate) const GLYPH_MASK_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact {
    name: GLYPH_MASK_RGBA8_KERNEL_NAME,
    target: "adls",
    bin: GLYPH_MASK_RGBA8_ADLS_BIN,
    spv: GLYPH_MASK_RGBA8_ADLS_SPV,
    bin_sha256: GLYPH_MASK_RGBA8_ADLS_BIN_SHA256,
};

pub(crate) const PRESENT_RGBA8_TO_PRIMARY_XRGB_RECT_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact {
        name: PRESENT_RGBA8_TO_PRIMARY_XRGB_RECT_KERNEL_NAME,
        target: "adls",
        bin: PRESENT_RGBA8_TO_PRIMARY_XRGB_RECT_ADLS_BIN,
        spv: PRESENT_RGBA8_TO_PRIMARY_XRGB_RECT_ADLS_SPV,
        bin_sha256: PRESENT_RGBA8_TO_PRIMARY_XRGB_RECT_ADLS_BIN_SHA256,
    };

pub(crate) const UI4_NV12_YTILE_TO_PRIMARY_XRGB_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact {
        name: UI4_NV12_YTILE_TO_PRIMARY_XRGB_KERNEL_NAME,
        target: "adls",
        bin: UI4_NV12_YTILE_TO_PRIMARY_XRGB_ADLS_BIN,
        spv: UI4_NV12_YTILE_TO_PRIMARY_XRGB_ADLS_SPV,
        bin_sha256: UI4_NV12_YTILE_TO_PRIMARY_XRGB_ADLS_BIN_SHA256,
    };

pub(crate) const UI4_NV12_TILE64_TO_RGBA8_FRAME_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact {
        name: UI4_NV12_TILE64_TO_RGBA8_FRAME_KERNEL_NAME,
        target: "adls",
        bin: UI4_NV12_TILE64_TO_RGBA8_FRAME_ADLS_BIN,
        spv: UI4_NV12_TILE64_TO_RGBA8_FRAME_ADLS_SPV,
        bin_sha256: UI4_NV12_TILE64_TO_RGBA8_FRAME_ADLS_BIN_SHA256,
    };

pub(crate) const SPRITE64_WORKLIST_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact {
    name: SPRITE64_WORKLIST_RGBA8_KERNEL_NAME,
    target: "adls",
    bin: SPRITE64_WORKLIST_RGBA8_ADLS_BIN,
    spv: SPRITE64_WORKLIST_RGBA8_ADLS_SPV,
    bin_sha256: SPRITE64_WORKLIST_RGBA8_ADLS_BIN_SHA256,
};

pub(crate) const SPRITE_QUAD_WORKLIST_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact {
        name: SPRITE_QUAD_WORKLIST_RGBA8_KERNEL_NAME,
        target: "adls",
        bin: SPRITE_QUAD_WORKLIST_RGBA8_ADLS_BIN,
        spv: SPRITE_QUAD_WORKLIST_RGBA8_ADLS_SPV,
        bin_sha256: SPRITE_QUAD_WORKLIST_RGBA8_ADLS_BIN_SHA256,
    };

pub(crate) const UI4_COMPOSE_LAYERS_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact {
        name: UI4_COMPOSE_LAYERS_RGBA8_KERNEL_NAME,
        target: "adls",
        bin: UI4_COMPOSE_LAYERS_RGBA8_ADLS_BIN,
        spv: UI4_COMPOSE_LAYERS_RGBA8_ADLS_SPV,
        bin_sha256: UI4_COMPOSE_LAYERS_RGBA8_ADLS_BIN_SHA256,
    };

pub(crate) const MANDEL64_WORKLIST_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact {
    name: MANDEL64_WORKLIST_RGBA8_KERNEL_NAME,
    target: "adls",
    bin: MANDEL64_WORKLIST_RGBA8_ADLS_BIN,
    spv: MANDEL64_WORKLIST_RGBA8_ADLS_SPV,
    bin_sha256: MANDEL64_WORKLIST_RGBA8_ADLS_BIN_SHA256,
};

pub(crate) const CANVAS3D_PROJECT_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact {
    name: CANVAS3D_PROJECT_RGBA8_KERNEL_NAME,
    target: "adls",
    bin: CANVAS3D_PROJECT_RGBA8_ADLS_BIN,
    spv: CANVAS3D_PROJECT_RGBA8_ADLS_SPV,
    bin_sha256: CANVAS3D_PROJECT_RGBA8_ADLS_BIN_SHA256,
};

pub(crate) const CANVAS3D_TRANSFORM_Q16_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact {
    name: CANVAS3D_TRANSFORM_Q16_KERNEL_NAME,
    target: "adls",
    bin: CANVAS3D_TRANSFORM_Q16_ADLS_BIN,
    spv: CANVAS3D_TRANSFORM_Q16_ADLS_SPV,
    bin_sha256: CANVAS3D_TRANSFORM_Q16_ADLS_BIN_SHA256,
};

pub(crate) const CANVAS3D_CLIP_BOX_Q16_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact {
    name: CANVAS3D_CLIP_BOX_Q16_KERNEL_NAME,
    target: "adls",
    bin: CANVAS3D_CLIP_BOX_Q16_ADLS_BIN,
    spv: CANVAS3D_CLIP_BOX_Q16_ADLS_SPV,
    bin_sha256: CANVAS3D_CLIP_BOX_Q16_ADLS_BIN_SHA256,
};

pub(crate) const CANVAS3D_PLANE_SAMPLE_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact {
        name: CANVAS3D_PLANE_SAMPLE_RGBA8_KERNEL_NAME,
        target: "adls",
        bin: CANVAS3D_PLANE_SAMPLE_RGBA8_ADLS_BIN,
        spv: CANVAS3D_PLANE_SAMPLE_RGBA8_ADLS_SPV,
        bin_sha256: CANVAS3D_PLANE_SAMPLE_RGBA8_ADLS_BIN_SHA256,
    };

pub(crate) const CANVAS3D_PLANE_FILL_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact {
        name: CANVAS3D_PLANE_FILL_RGBA8_KERNEL_NAME,
        target: "adls",
        bin: CANVAS3D_PLANE_FILL_RGBA8_ADLS_BIN,
        spv: CANVAS3D_PLANE_FILL_RGBA8_ADLS_SPV,
        bin_sha256: CANVAS3D_PLANE_FILL_RGBA8_ADLS_BIN_SHA256,
    };

pub(crate) const CANVAS3D_PLANE_PATCH_FILL_CUT_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact {
        name: CANVAS3D_PLANE_PATCH_FILL_CUT_RGBA8_KERNEL_NAME,
        target: "adls",
        bin: CANVAS3D_PLANE_PATCH_FILL_CUT_RGBA8_ADLS_BIN,
        spv: CANVAS3D_PLANE_PATCH_FILL_CUT_RGBA8_ADLS_SPV,
        bin_sha256: CANVAS3D_PLANE_PATCH_FILL_CUT_RGBA8_ADLS_BIN_SHA256,
    };

pub(crate) const CANVAS3D_PLANE_PATCH_WORKLIST_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact {
        name: CANVAS3D_PLANE_PATCH_WORKLIST_RGBA8_KERNEL_NAME,
        target: "adls",
        bin: CANVAS3D_PLANE_PATCH_WORKLIST_RGBA8_ADLS_BIN,
        spv: CANVAS3D_PLANE_PATCH_WORKLIST_RGBA8_ADLS_SPV,
        bin_sha256: CANVAS3D_PLANE_PATCH_WORKLIST_RGBA8_ADLS_BIN_SHA256,
    };

pub(crate) const SKYBOX_SAMPLE_RGB565_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact {
    name: SKYBOX_SAMPLE_RGB565_KERNEL_NAME,
    target: "adls",
    bin: SKYBOX_SAMPLE_RGB565_ADLS_BIN,
    spv: SKYBOX_SAMPLE_RGB565_ADLS_SPV,
    bin_sha256: SKYBOX_SAMPLE_RGB565_ADLS_BIN_SHA256,
};

pub(crate) const CHART_SINE_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact {
    name: CHART_SINE_RGBA8_KERNEL_NAME,
    target: "adls",
    bin: CHART_SINE_RGBA8_ADLS_BIN,
    spv: CHART_SINE_RGBA8_ADLS_SPV,
    bin_sha256: CHART_SINE_RGBA8_ADLS_BIN_SHA256,
};

pub(crate) const PIXEL_PLASMA_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact {
    name: PIXEL_PLASMA_RGBA8_KERNEL_NAME,
    target: "adls",
    bin: PIXEL_PLASMA_RGBA8_ADLS_BIN,
    spv: PIXEL_PLASMA_RGBA8_ADLS_SPV,
    bin_sha256: PIXEL_PLASMA_RGBA8_ADLS_BIN_SHA256,
};

pub(crate) const FONT_OUTLINE_MESH_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact {
    name: FONT_OUTLINE_MESH_KERNEL_NAME,
    target: "adls",
    bin: FONT_OUTLINE_MESH_ADLS_BIN,
    spv: FONT_OUTLINE_MESH_ADLS_SPV,
    bin_sha256: FONT_OUTLINE_MESH_ADLS_BIN_SHA256,
};

pub(crate) const FONT_OUTLINE_COVERAGE_R8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact {
        name: FONT_OUTLINE_COVERAGE_R8_KERNEL_NAME,
        target: "adls",
        bin: FONT_OUTLINE_COVERAGE_R8_ADLS_BIN,
        spv: FONT_OUTLINE_COVERAGE_R8_ADLS_SPV,
        bin_sha256: FONT_OUTLINE_COVERAGE_R8_ADLS_BIN_SHA256,
    };

pub(crate) fn copy_rect_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *COPY_RECT_RGBA8_UPLOAD.lock()
}

pub(crate) fn fill_rect_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *FILL_RECT_RGBA8_UPLOAD.lock()
}

pub(crate) fn fill_rect_worklist_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *FILL_RECT_WORKLIST_RGBA8_UPLOAD.lock()
}

pub(crate) fn gradient_rect_worklist_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *GRADIENT_RECT_WORKLIST_RGBA8_UPLOAD.lock()
}

pub(crate) fn alpha_blend_worklist_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *ALPHA_BLEND_WORKLIST_RGBA8_UPLOAD.lock()
}

pub(crate) fn glyph_mask_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *GLYPH_MASK_RGBA8_UPLOAD.lock()
}

pub(crate) fn present_rgba8_to_primary_xrgb_rect_upload_status() -> Option<UploadedKernelArtifact> {
    *PRESENT_RGBA8_TO_PRIMARY_XRGB_RECT_UPLOAD.lock()
}

pub(crate) fn sprite64_worklist_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *SPRITE64_WORKLIST_RGBA8_UPLOAD.lock()
}

pub(crate) fn sprite_quad_worklist_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *SPRITE_QUAD_WORKLIST_RGBA8_UPLOAD.lock()
}

pub(crate) fn ui4_compose_layers_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *UI4_COMPOSE_LAYERS_RGBA8_UPLOAD.lock()
}

pub(crate) fn mandel64_worklist_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *MANDEL64_WORKLIST_RGBA8_UPLOAD.lock()
}

pub(crate) fn canvas3d_project_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *CANVAS3D_PROJECT_RGBA8_UPLOAD.lock()
}

pub(crate) fn canvas3d_transform_q16_upload_status() -> Option<UploadedKernelArtifact> {
    *CANVAS3D_TRANSFORM_Q16_UPLOAD.lock()
}

pub(crate) fn canvas3d_clip_box_q16_upload_status() -> Option<UploadedKernelArtifact> {
    *CANVAS3D_CLIP_BOX_Q16_UPLOAD.lock()
}

pub(crate) fn canvas3d_plane_sample_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *CANVAS3D_PLANE_SAMPLE_RGBA8_UPLOAD.lock()
}

pub(crate) fn canvas3d_plane_fill_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *CANVAS3D_PLANE_FILL_RGBA8_UPLOAD.lock()
}

pub(crate) fn canvas3d_plane_patch_fill_cut_rgba8_upload_status() -> Option<UploadedKernelArtifact>
{
    *CANVAS3D_PLANE_PATCH_FILL_CUT_RGBA8_UPLOAD.lock()
}

pub(crate) fn canvas3d_plane_patch_worklist_rgba8_upload_status() -> Option<UploadedKernelArtifact>
{
    *CANVAS3D_PLANE_PATCH_WORKLIST_RGBA8_UPLOAD.lock()
}

pub(crate) fn skybox_sample_rgb565_upload_status() -> Option<UploadedKernelArtifact> {
    *SKYBOX_SAMPLE_RGB565_UPLOAD.lock()
}

pub(crate) fn chart_sine_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *CHART_SINE_RGBA8_UPLOAD.lock()
}

pub(crate) fn pixel_plasma_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *PIXEL_PLASMA_RGBA8_UPLOAD.lock()
}

pub(crate) fn font_outline_mesh_upload_status() -> Option<UploadedKernelArtifact> {
    *FONT_OUTLINE_MESH_UPLOAD.lock()
}

pub(crate) fn font_outline_coverage_r8_upload_status() -> Option<UploadedKernelArtifact> {
    *FONT_OUTLINE_COVERAGE_R8_UPLOAD.lock()
}

pub(crate) fn upload_copy_rect_rgba8_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *COPY_RECT_RGBA8_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: copy-rect-rgba8 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(dev, COPY_RECT_RGBA8_ADLS_ARTIFACT, COPY_RECT_RGBA8_ADLS_GPU)?;
    *COPY_RECT_RGBA8_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_resolve_tile64_msaa4_rgba8_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *RESOLVE_TILE64_MSAA4_RGBA8_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: resolve-tile64-msaa4-rgba8 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(
        dev,
        RESOLVE_TILE64_MSAA4_RGBA8_ADLS_ARTIFACT,
        RESOLVE_TILE64_MSAA4_RGBA8_ADLS_GPU,
    )?;
    *RESOLVE_TILE64_MSAA4_RGBA8_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_fill_rect_rgba8_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *FILL_RECT_RGBA8_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: fill-rect-rgba8 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(dev, FILL_RECT_RGBA8_ADLS_ARTIFACT, FILL_RECT_RGBA8_ADLS_GPU)?;
    *FILL_RECT_RGBA8_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_fill_rect_worklist_rgba8_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *FILL_RECT_WORKLIST_RGBA8_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: fill-rect-worklist-rgba8 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(
        dev,
        FILL_RECT_WORKLIST_RGBA8_ADLS_ARTIFACT,
        FILL_RECT_WORKLIST_RGBA8_ADLS_GPU,
    )?;
    *FILL_RECT_WORKLIST_RGBA8_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_gradient_rect_worklist_rgba8_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *GRADIENT_RECT_WORKLIST_RGBA8_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: gradient-rect-worklist-rgba8 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(
        dev,
        GRADIENT_RECT_WORKLIST_RGBA8_ADLS_ARTIFACT,
        GRADIENT_RECT_WORKLIST_RGBA8_ADLS_GPU,
    )?;
    *GRADIENT_RECT_WORKLIST_RGBA8_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_alpha_blend_worklist_rgba8_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *ALPHA_BLEND_WORKLIST_RGBA8_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: alpha-blend-worklist-rgba8 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(
        dev,
        ALPHA_BLEND_WORKLIST_RGBA8_ADLS_ARTIFACT,
        ALPHA_BLEND_WORKLIST_RGBA8_ADLS_GPU,
    )?;
    *ALPHA_BLEND_WORKLIST_RGBA8_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_glyph_mask_rgba8_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *GLYPH_MASK_RGBA8_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: glyph-mask-rgba8 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(dev, GLYPH_MASK_RGBA8_ADLS_ARTIFACT, GLYPH_MASK_RGBA8_ADLS_GPU)?;
    *GLYPH_MASK_RGBA8_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_present_rgba8_to_primary_xrgb_rect_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *PRESENT_RGBA8_TO_PRIMARY_XRGB_RECT_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: present-rgba8-to-primary-xrgb-rect upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(
        dev,
        PRESENT_RGBA8_TO_PRIMARY_XRGB_RECT_ADLS_ARTIFACT,
        PRESENT_RGBA8_TO_PRIMARY_XRGB_RECT_ADLS_GPU,
    )?;
    *PRESENT_RGBA8_TO_PRIMARY_XRGB_RECT_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_ui4_nv12_ytile_to_primary_xrgb_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *UI4_NV12_YTILE_TO_PRIMARY_XRGB_UPLOAD.lock() {
        return Some(upload);
    }
    let dev = super::claimed_device()?;
    let upload = upload_artifact(
        dev,
        UI4_NV12_YTILE_TO_PRIMARY_XRGB_ADLS_ARTIFACT,
        UI4_NV12_YTILE_TO_PRIMARY_XRGB_ADLS_GPU,
    )?;
    *UI4_NV12_YTILE_TO_PRIMARY_XRGB_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_ui4_nv12_tile64_to_rgba8_frame_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *UI4_NV12_TILE64_TO_RGBA8_FRAME_UPLOAD.lock() {
        return Some(upload);
    }
    let dev = super::claimed_device()?;
    let upload = upload_artifact(
        dev,
        UI4_NV12_TILE64_TO_RGBA8_FRAME_ADLS_ARTIFACT,
        UI4_NV12_TILE64_TO_RGBA8_FRAME_ADLS_GPU,
    )?;
    *UI4_NV12_TILE64_TO_RGBA8_FRAME_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_sprite64_worklist_rgba8_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *SPRITE64_WORKLIST_RGBA8_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: sprite64-worklist-rgba8 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(
        dev,
        SPRITE64_WORKLIST_RGBA8_ADLS_ARTIFACT,
        SPRITE64_WORKLIST_RGBA8_ADLS_GPU,
    )?;
    *SPRITE64_WORKLIST_RGBA8_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_sprite_quad_worklist_rgba8_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *SPRITE_QUAD_WORKLIST_RGBA8_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: sprite-quad-worklist-rgba8 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(
        dev,
        SPRITE_QUAD_WORKLIST_RGBA8_ADLS_ARTIFACT,
        SPRITE_QUAD_WORKLIST_RGBA8_ADLS_GPU,
    )?;
    *SPRITE_QUAD_WORKLIST_RGBA8_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_ui4_compose_layers_rgba8_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *UI4_COMPOSE_LAYERS_RGBA8_UPLOAD.lock() {
        return Some(upload);
    }

    let dev = super::claimed_device()?;
    let upload = upload_artifact(
        dev,
        UI4_COMPOSE_LAYERS_RGBA8_ADLS_ARTIFACT,
        UI4_COMPOSE_LAYERS_RGBA8_ADLS_GPU,
    )?;
    *UI4_COMPOSE_LAYERS_RGBA8_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_mandel64_worklist_rgba8_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *MANDEL64_WORKLIST_RGBA8_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: mandel64-worklist-rgba8 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(
        dev,
        MANDEL64_WORKLIST_RGBA8_ADLS_ARTIFACT,
        MANDEL64_WORKLIST_RGBA8_ADLS_GPU,
    )?;
    *MANDEL64_WORKLIST_RGBA8_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_canvas3d_project_rgba8_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *CANVAS3D_PROJECT_RGBA8_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: canvas3d-project-rgba8 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(
        dev,
        CANVAS3D_PROJECT_RGBA8_ADLS_ARTIFACT,
        CANVAS3D_PROJECT_RGBA8_ADLS_GPU,
    )?;
    *CANVAS3D_PROJECT_RGBA8_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_canvas3d_transform_q16_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *CANVAS3D_TRANSFORM_Q16_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: canvas3d-transform-q16 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(
        dev,
        CANVAS3D_TRANSFORM_Q16_ADLS_ARTIFACT,
        CANVAS3D_TRANSFORM_Q16_ADLS_GPU,
    )?;
    *CANVAS3D_TRANSFORM_Q16_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_canvas3d_clip_box_q16_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *CANVAS3D_CLIP_BOX_Q16_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: canvas3d-clip-box-q16 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload =
        upload_artifact(dev, CANVAS3D_CLIP_BOX_Q16_ADLS_ARTIFACT, CANVAS3D_CLIP_BOX_Q16_ADLS_GPU)?;
    *CANVAS3D_CLIP_BOX_Q16_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_canvas3d_plane_sample_rgba8_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *CANVAS3D_PLANE_SAMPLE_RGBA8_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: canvas3d-plane-sample-rgba8 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(
        dev,
        CANVAS3D_PLANE_SAMPLE_RGBA8_ADLS_ARTIFACT,
        CANVAS3D_PLANE_SAMPLE_RGBA8_ADLS_GPU,
    )?;
    *CANVAS3D_PLANE_SAMPLE_RGBA8_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_canvas3d_plane_fill_rgba8_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *CANVAS3D_PLANE_FILL_RGBA8_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: canvas3d-plane-fill-rgba8 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(
        dev,
        CANVAS3D_PLANE_FILL_RGBA8_ADLS_ARTIFACT,
        CANVAS3D_PLANE_FILL_RGBA8_ADLS_GPU,
    )?;
    *CANVAS3D_PLANE_FILL_RGBA8_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_canvas3d_plane_patch_fill_cut_rgba8_kernel() -> Option<UploadedKernelArtifact>
{
    if let Some(upload) = *CANVAS3D_PLANE_PATCH_FILL_CUT_RGBA8_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: canvas3d-plane-patch-fill-cut-rgba8 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(
        dev,
        CANVAS3D_PLANE_PATCH_FILL_CUT_RGBA8_ADLS_ARTIFACT,
        CANVAS3D_PLANE_PATCH_FILL_CUT_RGBA8_ADLS_GPU,
    )?;
    *CANVAS3D_PLANE_PATCH_FILL_CUT_RGBA8_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_canvas3d_plane_patch_worklist_rgba8_kernel() -> Option<UploadedKernelArtifact>
{
    if let Some(upload) = *CANVAS3D_PLANE_PATCH_WORKLIST_RGBA8_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: canvas3d-plane-patch-worklist-rgba8 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(
        dev,
        CANVAS3D_PLANE_PATCH_WORKLIST_RGBA8_ADLS_ARTIFACT,
        CANVAS3D_PLANE_PATCH_WORKLIST_RGBA8_ADLS_GPU,
    )?;
    *CANVAS3D_PLANE_PATCH_WORKLIST_RGBA8_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_skybox_sample_rgb565_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *SKYBOX_SAMPLE_RGB565_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: skybox-sample-rgb565 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload =
        upload_artifact(dev, SKYBOX_SAMPLE_RGB565_ADLS_ARTIFACT, SKYBOX_SAMPLE_RGB565_ADLS_GPU)?;
    *SKYBOX_SAMPLE_RGB565_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_chart_sine_rgba8_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *CHART_SINE_RGBA8_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_warn!(
            target: "gpgpu";
            "intel/gpgpu: chart-sine-rgba8 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(dev, CHART_SINE_RGBA8_ADLS_ARTIFACT, CHART_SINE_RGBA8_ADLS_GPU)?;
    *CHART_SINE_RGBA8_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_pixel_plasma_rgba8_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *PIXEL_PLASMA_RGBA8_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_warn!(
            target: "gpgpu";
            "intel/gpgpu: pixel-plasma-rgba8 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload =
        upload_artifact(dev, PIXEL_PLASMA_RGBA8_ADLS_ARTIFACT, PIXEL_PLASMA_RGBA8_ADLS_GPU)?;
    *PIXEL_PLASMA_RGBA8_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_font_outline_mesh_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *FONT_OUTLINE_MESH_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_warn!(
            target: "gpgpu";
            "intel/gpgpu: font-outline-mesh upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(dev, FONT_OUTLINE_MESH_ADLS_ARTIFACT, FONT_OUTLINE_MESH_ADLS_GPU)?;
    *FONT_OUTLINE_MESH_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_font_outline_coverage_r8_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *FONT_OUTLINE_COVERAGE_R8_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_warn!(
            target: "gpgpu";
            "intel/gpgpu: font-outline-coverage-r8 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(
        dev,
        FONT_OUTLINE_COVERAGE_R8_ADLS_ARTIFACT,
        FONT_OUTLINE_COVERAGE_R8_ADLS_GPU,
    )?;
    *FONT_OUTLINE_COVERAGE_R8_UPLOAD.lock() = Some(upload);
    Some(upload)
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum GpgpuArtifactReloadError {
    UnknownKernel,
    NoClaimedDevice,
    UploadFailed,
}

impl GpgpuArtifactReloadError {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::UnknownKernel => "unknown-kernel",
            Self::NoClaimedDevice => "no-claimed-device",
            Self::UploadFailed => "upload-failed",
        }
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct GpgpuArtifactReloadSummary {
    pub(crate) attempted: usize,
    pub(crate) reloaded: usize,
    pub(crate) failed: usize,
}

struct GpgpuKnownArtifactSlot {
    artifact: GpgpuKernelArtifact,
    gpu: u64,
    upload: &'static Mutex<Option<UploadedKernelArtifact>>,
}

const GPGPU_KNOWN_ARTIFACT_NAMES: &[&str] = &[
    COPY_RECT_RGBA8_KERNEL_NAME,
    RESOLVE_TILE64_MSAA4_RGBA8_KERNEL_NAME,
    FILL_RECT_RGBA8_KERNEL_NAME,
    FILL_RECT_WORKLIST_RGBA8_KERNEL_NAME,
    GRADIENT_RECT_WORKLIST_RGBA8_KERNEL_NAME,
    ALPHA_BLEND_WORKLIST_RGBA8_KERNEL_NAME,
    GLYPH_MASK_RGBA8_KERNEL_NAME,
    PRESENT_RGBA8_TO_PRIMARY_XRGB_RECT_KERNEL_NAME,
    SPRITE64_WORKLIST_RGBA8_KERNEL_NAME,
    SPRITE_QUAD_WORKLIST_RGBA8_KERNEL_NAME,
    UI4_COMPOSE_LAYERS_RGBA8_KERNEL_NAME,
    MANDEL64_WORKLIST_RGBA8_KERNEL_NAME,
    CANVAS3D_PROJECT_RGBA8_KERNEL_NAME,
    CANVAS3D_TRANSFORM_Q16_KERNEL_NAME,
    CANVAS3D_CLIP_BOX_Q16_KERNEL_NAME,
    CANVAS3D_PLANE_SAMPLE_RGBA8_KERNEL_NAME,
    CANVAS3D_PLANE_FILL_RGBA8_KERNEL_NAME,
    CANVAS3D_PLANE_PATCH_FILL_CUT_RGBA8_KERNEL_NAME,
    CANVAS3D_PLANE_PATCH_WORKLIST_RGBA8_KERNEL_NAME,
    SKYBOX_SAMPLE_RGB565_KERNEL_NAME,
    CHART_SINE_RGBA8_KERNEL_NAME,
    PIXEL_PLASMA_RGBA8_KERNEL_NAME,
    FONT_OUTLINE_MESH_KERNEL_NAME,
    FONT_OUTLINE_COVERAGE_R8_KERNEL_NAME,
];

pub(crate) fn reload_known_kernel_artifact(
    name: &str,
) -> Result<UploadedKernelArtifact, GpgpuArtifactReloadError> {
    let Some(slot) = known_artifact_slot(name) else {
        return Err(GpgpuArtifactReloadError::UnknownKernel);
    };
    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: {} reload failed reason=no-claimed-device\n",
            slot.artifact.name
        );
        return Err(GpgpuArtifactReloadError::NoClaimedDevice);
    };

    let Some(upload) = upload_artifact_from_sources(dev, slot.artifact, slot.gpu, true) else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: {} reload failed reason=upload-failed previous=kept\n",
            slot.artifact.name
        );
        return Err(GpgpuArtifactReloadError::UploadFailed);
    };

    *slot.upload.lock() = Some(upload);
    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: {} reload ok=1 source={} gpu=0x{:X} bytes=0x{:X} sha256={}\n",
        upload.name,
        upload.source,
        upload.gpu,
        upload.bytes,
        digest_hex(&upload.bin_sha256).as_str()
    );
    Ok(upload)
}

pub(crate) fn reload_all_known_kernel_artifacts() -> GpgpuArtifactReloadSummary {
    let mut summary = GpgpuArtifactReloadSummary::default();
    for name in GPGPU_KNOWN_ARTIFACT_NAMES {
        summary.attempted = summary.attempted.saturating_add(1);
        match reload_known_kernel_artifact(name) {
            Ok(_) => summary.reloaded = summary.reloaded.saturating_add(1),
            Err(_) => summary.failed = summary.failed.saturating_add(1),
        }
    }
    summary
}

fn known_artifact_slot(name: &str) -> Option<GpgpuKnownArtifactSlot> {
    match name {
        COPY_RECT_RGBA8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: COPY_RECT_RGBA8_ADLS_ARTIFACT,
            gpu: COPY_RECT_RGBA8_ADLS_GPU,
            upload: &COPY_RECT_RGBA8_UPLOAD,
        }),
        RESOLVE_TILE64_MSAA4_RGBA8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: RESOLVE_TILE64_MSAA4_RGBA8_ADLS_ARTIFACT,
            gpu: RESOLVE_TILE64_MSAA4_RGBA8_ADLS_GPU,
            upload: &RESOLVE_TILE64_MSAA4_RGBA8_UPLOAD,
        }),
        FILL_RECT_RGBA8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: FILL_RECT_RGBA8_ADLS_ARTIFACT,
            gpu: FILL_RECT_RGBA8_ADLS_GPU,
            upload: &FILL_RECT_RGBA8_UPLOAD,
        }),
        FILL_RECT_WORKLIST_RGBA8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: FILL_RECT_WORKLIST_RGBA8_ADLS_ARTIFACT,
            gpu: FILL_RECT_WORKLIST_RGBA8_ADLS_GPU,
            upload: &FILL_RECT_WORKLIST_RGBA8_UPLOAD,
        }),
        GRADIENT_RECT_WORKLIST_RGBA8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: GRADIENT_RECT_WORKLIST_RGBA8_ADLS_ARTIFACT,
            gpu: GRADIENT_RECT_WORKLIST_RGBA8_ADLS_GPU,
            upload: &GRADIENT_RECT_WORKLIST_RGBA8_UPLOAD,
        }),
        ALPHA_BLEND_WORKLIST_RGBA8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: ALPHA_BLEND_WORKLIST_RGBA8_ADLS_ARTIFACT,
            gpu: ALPHA_BLEND_WORKLIST_RGBA8_ADLS_GPU,
            upload: &ALPHA_BLEND_WORKLIST_RGBA8_UPLOAD,
        }),
        GLYPH_MASK_RGBA8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: GLYPH_MASK_RGBA8_ADLS_ARTIFACT,
            gpu: GLYPH_MASK_RGBA8_ADLS_GPU,
            upload: &GLYPH_MASK_RGBA8_UPLOAD,
        }),
        PRESENT_RGBA8_TO_PRIMARY_XRGB_RECT_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: PRESENT_RGBA8_TO_PRIMARY_XRGB_RECT_ADLS_ARTIFACT,
            gpu: PRESENT_RGBA8_TO_PRIMARY_XRGB_RECT_ADLS_GPU,
            upload: &PRESENT_RGBA8_TO_PRIMARY_XRGB_RECT_UPLOAD,
        }),
        SPRITE64_WORKLIST_RGBA8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: SPRITE64_WORKLIST_RGBA8_ADLS_ARTIFACT,
            gpu: SPRITE64_WORKLIST_RGBA8_ADLS_GPU,
            upload: &SPRITE64_WORKLIST_RGBA8_UPLOAD,
        }),
        SPRITE_QUAD_WORKLIST_RGBA8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: SPRITE_QUAD_WORKLIST_RGBA8_ADLS_ARTIFACT,
            gpu: SPRITE_QUAD_WORKLIST_RGBA8_ADLS_GPU,
            upload: &SPRITE_QUAD_WORKLIST_RGBA8_UPLOAD,
        }),
        UI4_COMPOSE_LAYERS_RGBA8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: UI4_COMPOSE_LAYERS_RGBA8_ADLS_ARTIFACT,
            gpu: UI4_COMPOSE_LAYERS_RGBA8_ADLS_GPU,
            upload: &UI4_COMPOSE_LAYERS_RGBA8_UPLOAD,
        }),
        MANDEL64_WORKLIST_RGBA8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: MANDEL64_WORKLIST_RGBA8_ADLS_ARTIFACT,
            gpu: MANDEL64_WORKLIST_RGBA8_ADLS_GPU,
            upload: &MANDEL64_WORKLIST_RGBA8_UPLOAD,
        }),
        CANVAS3D_PROJECT_RGBA8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: CANVAS3D_PROJECT_RGBA8_ADLS_ARTIFACT,
            gpu: CANVAS3D_PROJECT_RGBA8_ADLS_GPU,
            upload: &CANVAS3D_PROJECT_RGBA8_UPLOAD,
        }),
        CANVAS3D_TRANSFORM_Q16_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: CANVAS3D_TRANSFORM_Q16_ADLS_ARTIFACT,
            gpu: CANVAS3D_TRANSFORM_Q16_ADLS_GPU,
            upload: &CANVAS3D_TRANSFORM_Q16_UPLOAD,
        }),
        CANVAS3D_CLIP_BOX_Q16_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: CANVAS3D_CLIP_BOX_Q16_ADLS_ARTIFACT,
            gpu: CANVAS3D_CLIP_BOX_Q16_ADLS_GPU,
            upload: &CANVAS3D_CLIP_BOX_Q16_UPLOAD,
        }),
        CANVAS3D_PLANE_SAMPLE_RGBA8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: CANVAS3D_PLANE_SAMPLE_RGBA8_ADLS_ARTIFACT,
            gpu: CANVAS3D_PLANE_SAMPLE_RGBA8_ADLS_GPU,
            upload: &CANVAS3D_PLANE_SAMPLE_RGBA8_UPLOAD,
        }),
        CANVAS3D_PLANE_FILL_RGBA8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: CANVAS3D_PLANE_FILL_RGBA8_ADLS_ARTIFACT,
            gpu: CANVAS3D_PLANE_FILL_RGBA8_ADLS_GPU,
            upload: &CANVAS3D_PLANE_FILL_RGBA8_UPLOAD,
        }),
        CANVAS3D_PLANE_PATCH_FILL_CUT_RGBA8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: CANVAS3D_PLANE_PATCH_FILL_CUT_RGBA8_ADLS_ARTIFACT,
            gpu: CANVAS3D_PLANE_PATCH_FILL_CUT_RGBA8_ADLS_GPU,
            upload: &CANVAS3D_PLANE_PATCH_FILL_CUT_RGBA8_UPLOAD,
        }),
        CANVAS3D_PLANE_PATCH_WORKLIST_RGBA8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: CANVAS3D_PLANE_PATCH_WORKLIST_RGBA8_ADLS_ARTIFACT,
            gpu: CANVAS3D_PLANE_PATCH_WORKLIST_RGBA8_ADLS_GPU,
            upload: &CANVAS3D_PLANE_PATCH_WORKLIST_RGBA8_UPLOAD,
        }),
        SKYBOX_SAMPLE_RGB565_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: SKYBOX_SAMPLE_RGB565_ADLS_ARTIFACT,
            gpu: SKYBOX_SAMPLE_RGB565_ADLS_GPU,
            upload: &SKYBOX_SAMPLE_RGB565_UPLOAD,
        }),
        CHART_SINE_RGBA8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: CHART_SINE_RGBA8_ADLS_ARTIFACT,
            gpu: CHART_SINE_RGBA8_ADLS_GPU,
            upload: &CHART_SINE_RGBA8_UPLOAD,
        }),
        PIXEL_PLASMA_RGBA8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: PIXEL_PLASMA_RGBA8_ADLS_ARTIFACT,
            gpu: PIXEL_PLASMA_RGBA8_ADLS_GPU,
            upload: &PIXEL_PLASMA_RGBA8_UPLOAD,
        }),
        FONT_OUTLINE_MESH_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: FONT_OUTLINE_MESH_ADLS_ARTIFACT,
            gpu: FONT_OUTLINE_MESH_ADLS_GPU,
            upload: &FONT_OUTLINE_MESH_UPLOAD,
        }),
        FONT_OUTLINE_COVERAGE_R8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: FONT_OUTLINE_COVERAGE_R8_ADLS_ARTIFACT,
            gpu: FONT_OUTLINE_COVERAGE_R8_ADLS_GPU,
            upload: &FONT_OUTLINE_COVERAGE_R8_UPLOAD,
        }),
        _ => None,
    }
}

pub(crate) fn fill_rect_rgba8_stats(
    dst: GpgpuRgba8Surface,
    rect: GpgpuRect,
    color_rgba: u32,
) -> GpgpuSubmitStats {
    let Some(params) = lower_fill_rect(dst, rect, color_rgba) else {
        return GpgpuSubmitStats::default();
    };
    submit_fill_rect_2d_with_stats(dst, params)
}

/// Copy one rectangle with one two-dimensional submission and report success
/// only after that dispatch retired.
pub(crate) fn copy_rect_rgba8_complete(
    src: GpgpuRgba8Surface,
    src_rect: GpgpuRect,
    dst: GpgpuRgba8Surface,
    dst_xy: GpgpuPoint,
) -> bool {
    copy_rect_rgba8_complete_mode(src, src_rect, dst, dst_xy, false)
}

pub(crate) fn copy_rect_rgba8_complete_mode(
    src: GpgpuRgba8Surface,
    src_rect: GpgpuRect,
    dst: GpgpuRgba8Surface,
    dst_xy: GpgpuPoint,
    direct_scanout: bool,
) -> bool {
    let Some(params) = lower_copy_rect(src, src_rect, dst, dst_xy) else {
        return false;
    };
    submit_copy_rect_2d(src, dst, params, direct_scanout)
}

/// Resolve one gfx12.5 Tile64 R8G8B8A8 4x-MSAA surface into linear RGBA8.
///
/// This is deliberately a single two-dimensional SIMD16 dispatch: resident
/// scenes pay one GPU resolve per complete frame rather than one submission
/// per scanline/span.
pub(crate) fn resolve_tile64_msaa4_rgba8(
    src: GpgpuRgba8Surface,
    dst: GpgpuRgba8Surface,
    width: u32,
    height: u32,
) -> bool {
    resolve_tile64_msaa4_rgba8_mode(src, dst, width, height, false)
}

pub(crate) fn resolve_tile64_msaa4_rgba8_mode(
    src: GpgpuRgba8Surface,
    dst: GpgpuRgba8Surface,
    width: u32,
    height: u32,
    direct_scanout: bool,
) -> bool {
    let Some(params) =
        lower_copy_rect(src, GpgpuRect::new(0, 0, width, height), dst, GpgpuPoint::new(0, 0))
    else {
        return false;
    };
    submit_resolve_tile64_msaa4_2d(src, dst, params, direct_scanout)
}

fn reserve_font_coverage_gpu_va(bytes: usize) -> Option<u64> {
    let bytes = align_up(bytes, super::WARM_ALIGN)? as u64;
    {
        let mut free = FONT_COVERAGE_GPU_VA_FREE.lock();
        if let Some(index) = free
            .iter()
            .position(|(start, end)| end.saturating_sub(*start) >= bytes)
        {
            let (start, end) = free[index];
            let next = start.checked_add(bytes)?;
            if next == end {
                free.swap_remove(index);
            } else {
                free[index].0 = next;
            }
            return Some(start);
        }
    }
    loop {
        let current = FONT_COVERAGE_GPU_VA_CURSOR.load(Ordering::Acquire);
        let aligned = current.checked_add((super::WARM_ALIGN - 1) as u64)?
            & !((super::WARM_ALIGN - 1) as u64);
        let next = aligned.checked_add(bytes)?;
        if aligned < DIRECT_RCS_GPU_VA_FONT_COVERAGE_PRIMARY_LIMIT
            && next > DIRECT_RCS_GPU_VA_FONT_COVERAGE_PRIMARY_LIMIT
        {
            let _ = FONT_COVERAGE_GPU_VA_CURSOR.compare_exchange(
                current,
                DIRECT_RCS_GPU_VA_FONT_COVERAGE_SECONDARY_BASE,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            continue;
        }
        if (DIRECT_RCS_GPU_VA_FONT_COVERAGE_PRIMARY_LIMIT
            ..DIRECT_RCS_GPU_VA_FONT_COVERAGE_SECONDARY_BASE)
            .contains(&aligned)
        {
            let _ = FONT_COVERAGE_GPU_VA_CURSOR.compare_exchange(
                current,
                DIRECT_RCS_GPU_VA_FONT_COVERAGE_SECONDARY_BASE,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            continue;
        }
        if next > DIRECT_RCS_GPU_VA_FONT_COVERAGE_LIMIT {
            return None;
        }
        if FONT_COVERAGE_GPU_VA_CURSOR
            .compare_exchange(current, next, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            return Some(aligned);
        }
    }
}

fn recycle_font_coverage_gpu_va(gpu: u64, bytes: usize) {
    let Some(bytes) = align_up(bytes, super::WARM_ALIGN).map(|value| value as u64) else {
        return;
    };
    let Some(end) = gpu.checked_add(bytes) else {
        return;
    };
    let in_primary = gpu >= DIRECT_RCS_GPU_VA_FONT_COVERAGE_BASE
        && end <= DIRECT_RCS_GPU_VA_FONT_COVERAGE_PRIMARY_LIMIT;
    let in_secondary = gpu >= DIRECT_RCS_GPU_VA_FONT_COVERAGE_SECONDARY_BASE
        && end <= DIRECT_RCS_GPU_VA_FONT_COVERAGE_LIMIT;
    if !in_primary && !in_secondary {
        return;
    }
    let mut free = FONT_COVERAGE_GPU_VA_FREE.lock();
    free.push((gpu, end));
    free.sort_unstable_by_key(|range| range.0);
    let mut write = 0usize;
    for read in 0..free.len() {
        let range = free[read];
        if write != 0 && range.0 <= free[write - 1].1 {
            free[write - 1].1 = free[write - 1].1.max(range.1);
        } else {
            free[write] = range;
            write += 1;
        }
    }
    free.truncate(write);
}

/// Allocate one persistent linear R8 mask with its own PPGTT virtual range.
/// Distinct simultaneously-live masks are never remapped over one another.
pub(crate) fn allocate_font_coverage_mask(
    width: u32,
    height: u32,
) -> Option<GpgpuOwnedMask8Surface> {
    if width == 0 || height == 0 {
        return None;
    }
    let pitch_bytes = u32::try_from(align_up(width as usize, 64)?).ok()?;
    let raw_bytes = (pitch_bytes as usize).checked_mul(height as usize)?;
    let bytes = align_up(raw_bytes, super::WARM_ALIGN)?;
    if bytes > DIRECT_RCS_FONT_COVERAGE_MASK_MAX_BYTES {
        return None;
    }
    let (phys, virt) = crate::dma::alloc(bytes, super::WARM_ALIGN)?;
    let Some(gpu) = reserve_font_coverage_gpu_va(bytes) else {
        crate::dma::dealloc(virt, bytes);
        return None;
    };
    unsafe {
        core::ptr::write_bytes(virt, 0, bytes);
    }
    super::dma_flush(virt, bytes);
    let Some(surface) = GpgpuMask8Surface::new(phys, gpu, bytes, width, height, pitch_bytes) else {
        crate::dma::dealloc(virt, bytes);
        recycle_font_coverage_gpu_va(gpu, bytes);
        return None;
    };
    Some(GpgpuOwnedMask8Surface { surface, virt })
}

fn run_font_outline_coverage_r8_self_test() -> bool {
    const WIDTH: u32 = 19;
    const HEIGHT: u32 = 11;
    const OPS: [[u32; 8]; 5] = [
        [0, 2.0f32.to_bits(), 2.0f32.to_bits(), 0, 0, 0, 0, 0],
        [1, 17.0f32.to_bits(), 2.0f32.to_bits(), 0, 0, 0, 0, 0],
        [1, 17.0f32.to_bits(), 9.0f32.to_bits(), 0, 0, 0, 0, 0],
        [1, 2.0f32.to_bits(), 9.0f32.to_bits(), 0, 0, 0, 0, 0],
        [4, 0, 0, 0, 0, 0, 0, 0],
    ];
    let Some(mask) = allocate_font_coverage_mask(WIDTH, HEIGHT) else {
        return false;
    };
    let input_bytes = OPS.len() * core::mem::size_of::<[u32; 8]>();
    let Some(mapped_bytes) = align_up(input_bytes, super::WARM_ALIGN) else {
        return false;
    };
    let Some((ops_phys, ops_virt)) = crate::dma::alloc(mapped_bytes, super::WARM_ALIGN) else {
        return false;
    };
    unsafe {
        core::ptr::write_bytes(ops_virt, 0, mapped_bytes);
        core::ptr::copy_nonoverlapping(OPS.as_ptr().cast::<u8>(), ops_virt, input_bytes);
    }
    super::dma_flush(ops_virt, mapped_bytes);
    let surface = mask.surface();
    let params = FontOutlineCoverageR8Params {
        ops_gpu: DIRECT_RCS_GPU_VA_FONT_COVERAGE_OPS_BASE,
        mask_gpu: surface.gpu,
        op_count: OPS.len() as u32,
        subdivisions: 1,
        mask_pitch_bytes: surface.pitch_bytes,
        mask_width: WIDTH,
        mask_height: HEIGHT,
        rect_x: 0,
        rect_y: 0,
        rect_width: WIDTH,
        rect_height: HEIGHT,
        optical_bias_px: 0.0,
    };
    let submitted = submit_font_outline_coverage_r8_2d(ops_phys, mapped_bytes, surface, params);
    crate::dma::dealloc(ops_virt, mapped_bytes);
    if !submitted {
        return false;
    }
    let Some(audit) = mask.nonzero_audit() else {
        return false;
    };
    let mut solid_interior = true;
    for y in 3..8usize {
        // Include x=16 from the odd-width tail workgroup.  This catches a
        // walker that incorrectly applies a three-lane tail mask to every
        // SIMD16 group while still appearing to complete successfully.
        for x in 3..17usize {
            let offset = y * surface.pitch_bytes as usize + x;
            let coverage = unsafe { core::ptr::read_volatile(mask.virt.add(offset)) };
            solid_interior &= coverage == u8::MAX;
        }
    }
    let corner = unsafe { core::ptr::read_volatile(mask.virt) };
    let ok = solid_interior && corner == 0 && audit.nonzero_pixels >= 65;
    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: font-outline-coverage-r8 self-test={} mask_gpu=0x{:X} nonzero={} bounds={},{},{}x{} tail_width={} right_mask=full-simd16 invariant=solid-interior-including-tail+empty-corner+unique-va\n",
        if ok { "pass" } else { "fail" },
        surface.gpu,
        audit.nonzero_pixels,
        audit.bounds.x,
        audit.bounds.y,
        audit.bounds.width,
        audit.bounds.height,
        WIDTH % FILL_RECT_PIXELS_PER_GROUP_X,
    );
    ok
}

fn font_outline_coverage_r8_self_test() -> bool {
    *FONT_OUTLINE_COVERAGE_R8_SELF_TEST.call_once(run_font_outline_coverage_r8_self_test)
}

/// Add one positioned Skrifa outline stream into a persistent R8 mask.
/// Existing coverage is retained with `max`, allowing bold duplicate runs and
/// multiple glyphs to share one color-layer mask without CPU mask blending.
pub(crate) fn font_outline_coverage_r8(
    mask: &GpgpuOwnedMask8Surface,
    outline_ops: &[[u32; 8]],
    rect: GpgpuRect,
    subdivisions: u32,
    optical_bias_px: f32,
) -> bool {
    let surface = mask.surface();
    if outline_ops.is_empty()
        || outline_ops.len() > u32::MAX as usize
        || rect.x < 0
        || rect.y < 0
        || !rect_is_inside_mask(surface, rect)
        || !(1..=16).contains(&subdivisions)
        || !optical_bias_px.is_finite()
        || !(0.0..=0.35).contains(&optical_bias_px)
    {
        return false;
    }
    if !font_outline_coverage_r8_self_test() {
        return false;
    }
    let input_bytes = match outline_ops
        .len()
        .checked_mul(core::mem::size_of::<[u32; 8]>())
    {
        Some(bytes) => bytes,
        None => return false,
    };
    let mapped_bytes = match align_up(input_bytes, super::WARM_ALIGN) {
        Some(bytes) => bytes,
        None => return false,
    };
    if mapped_bytes > DIRECT_RCS_FONT_COVERAGE_OPS_WINDOW_BYTES {
        return false;
    }
    let Some((ops_phys, ops_virt)) = crate::dma::alloc(mapped_bytes, super::WARM_ALIGN) else {
        return false;
    };
    unsafe {
        core::ptr::write_bytes(ops_virt, 0, mapped_bytes);
        core::ptr::copy_nonoverlapping(outline_ops.as_ptr().cast::<u8>(), ops_virt, input_bytes);
    }
    super::dma_flush(ops_virt, mapped_bytes);
    let params = FontOutlineCoverageR8Params {
        ops_gpu: DIRECT_RCS_GPU_VA_FONT_COVERAGE_OPS_BASE,
        mask_gpu: surface.gpu,
        op_count: outline_ops.len() as u32,
        subdivisions,
        mask_pitch_bytes: surface.pitch_bytes,
        mask_width: surface.width,
        mask_height: surface.height,
        rect_x: rect.x as u32,
        rect_y: rect.y as u32,
        rect_width: rect.width,
        rect_height: rect.height,
        optical_bias_px,
    };
    let completed = submit_font_outline_coverage_r8_2d(ops_phys, mapped_bytes, surface, params);
    crate::dma::dealloc(ops_virt, mapped_bytes);
    completed
}

/// Composite one R8 glyph layer in a single native two-dimensional dispatch.
/// A valid layer that is fully outside the destination is already complete:
/// panning a resident scene must not turn an empty clip into a GPU failure and
/// demote all of its other analytical layers to triangle rendering.
pub(crate) fn glyph_mask_rgba8_2d(blit: GpgpuGlyphMaskBlit) -> bool {
    glyph_mask_rgba8_2d_mode(blit, false)
}

pub(crate) fn glyph_mask_rgba8_2d_mode(blit: GpgpuGlyphMaskBlit, direct_scanout: bool) -> bool {
    if !blit.mask.is_valid()
        || !blit.dst.is_valid()
        || !rect_is_inside_mask(blit.mask, blit.mask_rect)
    {
        return false;
    }
    let Some(params) = lower_glyph_mask_blit(blit) else {
        return true;
    };
    submit_glyph_mask_2d(blit.mask, blit.dst, params, blit.color_rgba, direct_scanout)
}

/// Composite all persistent R8 coverage layers into one RGBA destination with
/// one RCS submission and one retirement marker. Each active layer retains an
/// independent stateless mask address, clip, destination point, and RGBA
/// payload; fully clipped layers are successful no-ops.
pub(crate) fn glyph_mask_layers_rgba8_2d(
    layers: &[GpgpuGlyphMaskLayer],
    dst: GpgpuRgba8Surface,
) -> GpgpuGlyphMaskBatchResult {
    glyph_mask_layers_rgba8_2d_mode(layers, dst, false)
}

pub(crate) fn glyph_mask_layers_rgba8_2d_mode(
    layers: &[GpgpuGlyphMaskLayer],
    dst: GpgpuRgba8Surface,
    direct_scanout: bool,
) -> GpgpuGlyphMaskBatchResult {
    let mut result = GpgpuGlyphMaskBatchResult {
        requested_layers: layers.len(),
        ..GpgpuGlyphMaskBatchResult::default()
    };
    if !dst.is_valid() || layers.len() > GLYPH_MASK_BATCH_MAX_LAYERS {
        return result;
    }
    for layer in layers {
        if !layer.mask.is_valid() || !rect_is_inside_mask(layer.mask, layer.mask_rect) {
            return result;
        }
        let blit = GpgpuGlyphMaskBlit {
            mask: layer.mask,
            mask_rect: layer.mask_rect,
            dst,
            dst_xy: layer.dst_xy,
            color_rgba: layer.color_rgba,
        };
        if lower_glyph_mask_blit(blit).is_some() {
            result.active_walkers += 1;
        }
    }
    if result.active_walkers == 0 {
        result.ok = true;
        return result;
    }
    let (submitted, completed) = submit_glyph_mask_layers_2d(layers, dst, direct_scanout);
    result.submitted = submitted;
    result.ok = completed;
    result.submits = usize::from(submitted);
    result
}

pub(crate) fn fill_rect_worklist_rgba8_stats(
    dst: GpgpuRgba8Surface,
    descs: &[FillRectWorklistRgba8Desc],
) -> GpgpuWorklistSubmitStats {
    fill_rect_worklist_rgba8_stats_mode(dst, descs, false)
}

fn fill_rect_worklist_rgba8_stats_mode(
    dst: GpgpuRgba8Surface,
    descs: &[FillRectWorklistRgba8Desc],
    direct_scanout: bool,
) -> GpgpuWorklistSubmitStats {
    let Some(desc_buffer) = rect_worklist_desc_buffer_once() else {
        return GpgpuWorklistSubmitStats::default();
    };
    let mut stats = GpgpuWorklistSubmitStats::default();
    for chunk in descs.chunks(RECT_WORKLIST_MAX_DESCS) {
        if chunk.is_empty() {
            continue;
        }
        let _desc_guard = RECT_WORKLIST_DESC_SUBMIT_LOCK.lock();
        unsafe {
            core::ptr::write_bytes(desc_buffer.virt, 0, desc_buffer.bytes);
            let out = desc_buffer.virt as *mut FillRectWorklistRgba8Desc;
            for (index, desc) in chunk.iter().copied().enumerate() {
                core::ptr::write_volatile(out.add(index), desc);
            }
        }
        super::dma_flush(desc_buffer.virt, desc_buffer.bytes);

        let params = FillRectWorklistRgba8Params {
            dst_gpu: dst.gpu,
            desc_gpu: desc_buffer.gpu,
            dst_pitch_bytes: dst.pitch_bytes,
            desc_base: 0,
            desc_count: chunk.len() as u32,
        };
        let submit_start_tick = direct_rcs_now_tick();
        if !submit_fill_rect_worklist(dst, desc_buffer, params, direct_scanout) {
            break;
        }
        stats.submit_ms = stats
            .submit_ms
            .saturating_add(direct_rcs_elapsed_ms_since(submit_start_tick));
        stats.descs = stats.descs.saturating_add(chunk.len());
        stats.walkers = stats
            .walkers
            .saturating_add(rect_worklist_walker_count(chunk.len()));
        stats.submits = stats.submits.saturating_add(1);
    }
    stats
}

/// Fill a small set of solid rectangles in one worklist submission.
///
/// This is the retained-UI overlay path: callers can add cursors and other
/// simple decorations to a GPU-owned frame without mapping or touching its
/// pixels on the CPU. Rectangles are clipped to `dst`; a fully clipped set is
/// a successful no-op.
pub(crate) fn fill_solid_rects_rgba8(dst: GpgpuRgba8Surface, rects: &[GpgpuSolidRect]) -> bool {
    fill_solid_rects_rgba8_mode(dst, rects, false)
}

pub(crate) fn fill_solid_rects_rgba8_scanout(
    dst: GpgpuRgba8Surface,
    rects: &[GpgpuSolidRect],
) -> bool {
    fill_solid_rects_rgba8_mode(dst, rects, true)
}

fn fill_solid_rects_rgba8_mode(
    dst: GpgpuRgba8Surface,
    rects: &[GpgpuSolidRect],
    direct_scanout: bool,
) -> bool {
    const INLINE_RECTS: usize = 16;
    if !dst.is_valid() {
        return false;
    }
    if rects.is_empty() {
        return true;
    }
    if rects.len() > INLINE_RECTS {
        return false;
    }
    let mut descs = [FillRectWorklistRgba8Desc::default(); INLINE_RECTS];
    let mut desc_count = 0usize;
    for solid in rects {
        let Some(rect) = clip_gpgpu_rect_to_surface(solid.rect, dst.width, dst.height) else {
            continue;
        };
        let Ok(dst_x) = i16::try_from(rect.x) else {
            return false;
        };
        let Ok(dst_y) = i16::try_from(rect.y) else {
            return false;
        };
        if rect.width > u16::MAX as u32 || rect.height > u16::MAX as u32 {
            return false;
        }
        descs[desc_count] = FillRectWorklistRgba8Desc {
            dst_xy: pack_i16_pair_u32(dst_x, dst_y),
            size: pack_u16_pair_u32(rect.width as u16, rect.height as u16),
            color_rgba: solid.color_rgba,
        };
        desc_count += 1;
    }
    if desc_count == 0 {
        return true;
    }
    let stats = fill_rect_worklist_rgba8_stats_mode(dst, &descs[..desc_count], direct_scanout);
    stats.descs == desc_count && stats.submits == 1
}

pub(crate) fn sprite_quad_worklist_rgba8_runs_over_result(
    dst: GpgpuRgba8Surface,
    runs: &[GpgpuSpriteQuadWorklistRun<'_>],
) -> GpgpuWorklistSubmitResult {
    if !sprite_quad_worklist_ready() {
        return GpgpuWorklistSubmitResult::default();
    }
    let Some(desc_buffer) = sprite_quad_worklist_desc_buffer_once() else {
        return GpgpuWorklistSubmitResult::default();
    };
    let total_descs = runs
        .iter()
        .try_fold(0usize, |total, run| total.checked_add(run.descs.len()));
    let Some(total_descs) = total_descs else {
        return GpgpuWorklistSubmitResult::default();
    };
    if total_descs == 0 || total_descs > SPRITE_QUAD_WORKLIST_MAX_DESCS {
        return GpgpuWorklistSubmitResult::default();
    }
    if runs.iter().any(|run| run.descs.is_empty()) {
        return GpgpuWorklistSubmitResult::default();
    }

    let mut stats = GpgpuWorklistSubmitStats::default();
    let _desc_guard = RECT_WORKLIST_DESC_SUBMIT_LOCK.lock();
    unsafe {
        core::ptr::write_bytes(desc_buffer.virt, 0, desc_buffer.bytes);
        let out = desc_buffer.virt as *mut GpgpuSpriteQuadWorklistDesc;
        let mut index = 0usize;
        for run in runs {
            for desc in run.descs.iter().copied() {
                core::ptr::write_volatile(out.add(index), desc);
                index = index.saturating_add(1);
            }
        }
    }
    super::dma_flush(desc_buffer.virt, desc_buffer.bytes);

    let submit_start_tick = direct_rcs_now_tick();
    let outcome = submit_sprite_quad_worklist_runs(dst, desc_buffer, runs);
    if outcome != GpgpuSubmissionOutcome::Complete {
        return GpgpuWorklistSubmitResult { stats, outcome };
    }
    stats.submit_ms = stats
        .submit_ms
        .saturating_add(direct_rcs_elapsed_ms_since(submit_start_tick));
    stats.descs = total_descs;
    stats.walkers = runs.iter().fold(0usize, |total, run| {
        total.saturating_add(sprite_quad_worklist_walker_count(run.descs.len()))
    });
    stats.submits = 1;
    GpgpuWorklistSubmitResult { stats, outcome }
}

/// Queue one UI4 blend without waiting for its post marker.  Every mutable GPU
/// object used here is compositor-private: LRC/ring, batch, result page,
/// descriptor page, PPGTT root, vGPU device, and timeline.
pub(crate) fn queue_ui4_compositor_layers(
    base: Option<GpgpuRgba8Surface>,
    dst: GpgpuRgba8Surface,
    layers: &[GpgpuUi4ComposeLayer],
    damage: GpgpuRect,
    flags: u32,
) -> Result<Ui4CompositorSubmission, Ui4CompositorSubmitError> {
    if !dst.is_valid()
        || damage.x < 0
        || damage.y < 0
        || damage.width == 0
        || damage.height == 0
        || damage.x as u32 >= dst.width
        || damage.y as u32 >= dst.height
        || layers.len() > UI4_COMPOSE_LAYERS_MAX_LAYERS
    {
        return Err(Ui4CompositorSubmitError::InvalidWorklist);
    }
    let base = base.unwrap_or(dst);
    if !base.is_valid()
        || ((flags & UI4_COMPOSE_FLAG_BASE_XRGB) != 0
            && (base.width != dst.width || base.height != dst.height))
        || layers
            .iter()
            .any(|layer| !layer.src.is_valid() || layer.dst_width == 0 || layer.dst_height == 0)
    {
        return Err(Ui4CompositorSubmitError::InvalidWorklist);
    }

    let damage_x = damage.x as u32;
    let damage_y = damage.y as u32;
    let damage_width = damage.width.min(dst.width - damage_x);
    let damage_height = damage.height.min(dst.height - damage_y);
    let mut runtime = UI4_COMPOSITOR_RUNTIME.lock();
    if runtime.pending.is_some() {
        return Err(Ui4CompositorSubmitError::Busy);
    }
    let dev = super::claimed_device().ok_or(Ui4CompositorSubmitError::Unavailable)?;
    let upload =
        upload_ui4_compose_layers_rgba8_kernel().ok_or(Ui4CompositorSubmitError::Unavailable)?;
    let state = ui4_compositor_rcs_state_once(dev).ok_or(Ui4CompositorSubmitError::Unavailable)?;
    let desc = ui4_compositor_sprite_quad_desc_buffer_once()
        .ok_or(Ui4CompositorSubmitError::Unavailable)?;

    unsafe {
        core::ptr::write_bytes(desc.virt, 0, desc.bytes);
        let out = desc.virt as *mut GpgpuUi4ComposeLayerDesc;
        for (index, layer) in layers.iter().enumerate() {
            core::ptr::write_volatile(
                out.add(index),
                GpgpuUi4ComposeLayerDesc {
                    src_gpu_lo: layer.src.gpu as u32,
                    src_gpu_hi: (layer.src.gpu >> 32) as u32,
                    src_pitch_bytes: layer.src.pitch_bytes,
                    src_width: layer.src.width,
                    src_height: layer.src.height,
                    dst_x: layer.dst_x,
                    dst_y: layer.dst_y,
                    dst_width: layer.dst_width,
                    dst_height: layer.dst_height,
                    opacity: layer.opacity as u32,
                    flags: 0,
                    reserved: 0,
                },
            );
        }
    }
    super::dma_flush(desc.virt, desc.bytes);

    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && (runtime.state_mapped || direct_rcs_map_state(dev, state));
    if mapped_ok {
        runtime.state_mapped = true;
    }
    let ppgtt_ok = mapped_ok && (runtime.ppgtt_initialized || direct_rcs_init_ppgtt(state));
    if ppgtt_ok {
        runtime.ppgtt_initialized = true;
    }
    let kernel_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let base_ok = kernel_ok && direct_rcs_map_ppgtt_kernel(state, base.gpu, base.phys, base.bytes);
    let dst_ok = base_ok && direct_rcs_map_ppgtt_kernel(state, dst.gpu, dst.phys, dst.bytes);
    let desc_ok = dst_ok && direct_rcs_map_ppgtt_kernel(state, desc.gpu, desc.phys, desc.bytes);
    let mut sources_ok = desc_ok;
    for layer in layers {
        if sources_ok
            && !direct_rcs_map_ppgtt_kernel(state, layer.src.gpu, layer.src.phys, layer.src.bytes)
        {
            sources_ok = false;
        }
    }
    let params = Ui4ComposeLayersParams {
        base_gpu: base.gpu,
        dst_gpu: dst.gpu,
        layers_gpu: desc.gpu,
        base_pitch_bytes: base.pitch_bytes,
        dst_pitch_bytes: dst.pitch_bytes,
        dst_width: dst.width,
        dst_height: dst.height,
        damage_x,
        damage_y,
        damage_width,
        damage_height,
        layer_count: layers.len() as u32,
        flags,
    };
    let batch_ok = sources_ok
        && direct_rcs_encode_ui4_compose_layers_batch(
            state, upload, params, base.bytes, dst.bytes, desc.bytes,
        );
    if !batch_ok {
        crate::log_error!(target: "ui4";
            "ui4/guc-compositor: layer queue rejected forcewake={} mapped={} ppgtt={} kernel={} base={} dst={} desc={} sources={} layers={} damage={}x{}@{},{}\n",
            forcewake_ok as u8,
            mapped_ok as u8,
            ppgtt_ok as u8,
            kernel_ok as u8,
            base_ok as u8,
            dst_ok as u8,
            desc_ok as u8,
            sources_ok as u8,
            layers.len(),
            damage_width,
            damage_height,
            damage_x,
            damage_y,
        );
        return Err(Ui4CompositorSubmitError::InvalidWorklist);
    }

    let started_tick = direct_rcs_now_tick();
    if !direct_rcs_submit_batch_for(
        dev,
        state,
        &mut runtime.submit,
        crate::gpu::vgpu::KernelClient::Ui4Compositor,
    ) {
        return Err(Ui4CompositorSubmitError::SubmissionRejected);
    }
    runtime.next_serial = runtime.next_serial.wrapping_add(1).max(1);
    let serial = runtime.next_serial;
    let gpu = runtime
        .submit
        .pending
        .expect("accepted UI4 submission must have an executor token");
    let submission = Ui4CompositorSubmission { serial, gpu };
    runtime.last_completion = None;
    runtime.pending = Some(Ui4CompositorPending {
        submission,
        started_tick,
        marker_slot: SPRITE_QUAD_WORKLIST_POST_MARKER_SLOT,
        marker_value: UI4_COMPOSE_LAYERS_POST_MARKER,
        kernel: "ui4-compose-layers",
        stats: GpgpuWorklistSubmitStats {
            descs: layers.len(),
            walkers: 1,
            submits: 1,
            submit_ms: 0,
        },
        overdue_logged: false,
    });
    crate::log_trace!(target: "ui4";
        "ui4/guc-compositor: queued serial={} kernel=ui4-compose-layers layers={} walkers=1 damage={}x{}@{},{} dst_gpu=0x{:X} context=isolated persistent=1 wait=none\n",
        serial,
        layers.len(),
        damage_width,
        damage_height,
        damage_x,
        damage_y,
        dst.gpu,
    );
    Ok(submission)
}

pub(crate) fn queue_ui4_compositor_sprite_quad_runs(
    dst: GpgpuRgba8Surface,
    runs: &[GpgpuSpriteQuadWorklistRun<'_>],
) -> Result<Ui4CompositorSubmission, Ui4CompositorSubmitError> {
    // Do not call `sprite_quad_worklist_ready()` here. That helper runs the
    // legacy synchronous smoke probe and polls its marker on the caller's CPU.
    // UI4 calls this entry point from an Embassy task and owns an asynchronous
    // GuC completion path below; making admission depend on the synchronous
    // probe can time out a successfully admitted GuC request, poison the
    // one-shot readiness flag, and prevent the real compositor request from
    // ever being queued. Preparing and admitting this request is the capability
    // check; its marker is validated by `poll_ui4_compositor_submission()`.
    if !dst.is_valid() {
        return Err(Ui4CompositorSubmitError::Unavailable);
    }
    let total_descs = runs
        .iter()
        .try_fold(0usize, |total, run| total.checked_add(run.descs.len()))
        .ok_or(Ui4CompositorSubmitError::InvalidWorklist)?;
    if runs.is_empty()
        || total_descs == 0
        || total_descs > SPRITE_QUAD_WORKLIST_MAX_DESCS
        || runs.iter().any(|run| run.descs.is_empty())
    {
        return Err(Ui4CompositorSubmitError::InvalidWorklist);
    }

    let mut runtime = UI4_COMPOSITOR_RUNTIME.lock();
    if runtime.pending.is_some() {
        return Err(Ui4CompositorSubmitError::Busy);
    }
    let dev = super::claimed_device().ok_or(Ui4CompositorSubmitError::Unavailable)?;
    let upload =
        upload_sprite_quad_worklist_rgba8_kernel().ok_or(Ui4CompositorSubmitError::Unavailable)?;
    let state = ui4_compositor_rcs_state_once(dev).ok_or(Ui4CompositorSubmitError::Unavailable)?;
    let desc = ui4_compositor_sprite_quad_desc_buffer_once()
        .ok_or(Ui4CompositorSubmitError::Unavailable)?;

    unsafe {
        core::ptr::write_bytes(desc.virt, 0, desc.bytes);
        let out = desc.virt as *mut GpgpuSpriteQuadWorklistDesc;
        let mut index = 0usize;
        for run in runs {
            for descriptor in run.descs.iter().copied() {
                core::ptr::write_volatile(out.add(index), descriptor);
                index = index.saturating_add(1);
            }
        }
    }
    super::dma_flush(desc.virt, desc.bytes);

    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && (runtime.state_mapped || direct_rcs_map_state(dev, state));
    if mapped_ok {
        runtime.state_mapped = true;
    }
    let ppgtt_ok = mapped_ok && (runtime.ppgtt_initialized || direct_rcs_init_ppgtt(state));
    if ppgtt_ok {
        runtime.ppgtt_initialized = true;
    }
    let kernel_ppgtt_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let dst_ppgtt_ok =
        kernel_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, dst.gpu, dst.phys, dst.bytes);
    let desc_ppgtt_ok =
        dst_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, desc.gpu, desc.phys, desc.bytes);
    let mut src_ppgtt_ok = desc_ppgtt_ok;
    if src_ppgtt_ok {
        for run in runs {
            if !direct_rcs_map_ppgtt_kernel(state, run.src.gpu, run.src.phys, run.src.bytes) {
                src_ppgtt_ok = false;
                break;
            }
        }
    }
    let batch_ok = src_ppgtt_ok
        && direct_rcs_encode_sprite_quad_worklist_runs_batch(state, upload, dst, desc, runs);
    if !batch_ok {
        crate::log_error!(target: "ui4";
            "ui4/guc-compositor: queue rejected stage=prepare forcewake={} mapped={} ppgtt={} kernel={} dst={} desc={} src={} batch={} descs={}\n",
            forcewake_ok as u8,
            mapped_ok as u8,
            ppgtt_ok as u8,
            kernel_ppgtt_ok as u8,
            dst_ppgtt_ok as u8,
            desc_ppgtt_ok as u8,
            src_ppgtt_ok as u8,
            batch_ok as u8,
            total_descs,
        );
        return Err(Ui4CompositorSubmitError::InvalidWorklist);
    }
    let started_tick = direct_rcs_now_tick();
    if !direct_rcs_submit_batch_for(
        dev,
        state,
        &mut runtime.submit,
        crate::gpu::vgpu::KernelClient::Ui4Compositor,
    ) {
        return Err(Ui4CompositorSubmitError::SubmissionRejected);
    }
    runtime.next_serial = runtime.next_serial.wrapping_add(1).max(1);
    let serial = runtime.next_serial;
    let gpu = runtime
        .submit
        .pending
        .expect("accepted UI4 submission must have an executor token");
    let submission = Ui4CompositorSubmission { serial, gpu };
    runtime.last_completion = None;
    runtime.pending = Some(Ui4CompositorPending {
        submission,
        started_tick,
        marker_slot: SPRITE_QUAD_WORKLIST_POST_MARKER_SLOT,
        marker_value: SPRITE_QUAD_WORKLIST_POST_MARKER,
        kernel: "sprite-quad-runs",
        stats: GpgpuWorklistSubmitStats {
            descs: total_descs,
            walkers: total_descs,
            submits: 1,
            submit_ms: 0,
        },
        overdue_logged: false,
    });
    crate::log_trace!(target: "ui4";
        "ui4/guc-compositor: queued serial={} descs={} dst_gpu=0x{:X} context=isolated persistent=1 wait=none\n",
        serial,
        total_descs,
        dst.gpu,
    );
    Ok(submission)
}

/// Queue the complete native-video primary rebuild as one GuC-owned RCS job.
/// No CPU pixel conversion, intermediate RGBA frame, descriptor worklist, or
/// post-submit fallback is part of this contract.
pub(crate) fn queue_ui4_compositor_nv12_tile64_to_primary(
    source: GpgpuNv12Tile64Surface,
    base: GpgpuRgba8Surface,
    dst: GpgpuRgba8Surface,
    content_dst_x: u32,
    content_dst_y: u32,
    content_width: u32,
    content_height: u32,
    source_x: u32,
    source_y: u32,
) -> Result<Ui4CompositorSubmission, Ui4CompositorSubmitError> {
    let destination_valid = content_width != 0
        && content_height != 0
        && content_dst_x
            .checked_add(content_width)
            .is_some_and(|right| right <= dst.width)
        && content_dst_y
            .checked_add(content_height)
            .is_some_and(|bottom| bottom <= dst.height);
    let source_valid = source_x
        .checked_add(content_width)
        .is_some_and(|right| right <= source.width)
        && source_y
            .checked_add(content_height)
            .is_some_and(|bottom| bottom <= source.height);
    let layouts_match = base.is_valid()
        && dst.is_valid()
        && source.is_valid()
        && base.width == dst.width
        && base.height == dst.height;
    let ranges_distinct = !gpu_ranges_overlap(source.gpu, source.bytes, base.gpu, base.bytes)
        && !gpu_ranges_overlap(source.gpu, source.bytes, dst.gpu, dst.bytes)
        && !gpu_ranges_overlap(base.gpu, base.bytes, dst.gpu, dst.bytes)
        && !gpu_ranges_overlap(source.phys, source.bytes, base.phys, base.bytes)
        && !gpu_ranges_overlap(source.phys, source.bytes, dst.phys, dst.bytes)
        && !gpu_ranges_overlap(base.phys, base.bytes, dst.phys, dst.bytes);
    if !destination_valid || !source_valid || !layouts_match || !ranges_distinct {
        return Err(Ui4CompositorSubmitError::InvalidWorklist);
    }
    let params = Ui4Nv12Tile64ToPrimaryXrgbParams {
        nv12_gpu: source.gpu,
        base_gpu: base.gpu,
        dst_gpu: dst.gpu,
        src_pitch_bytes: source.pitch_bytes,
        src_uv_offset: source.uv_offset,
        base_pitch_bytes: base.pitch_bytes,
        dst_pitch_bytes: dst.pitch_bytes,
        output_width: dst.width,
        output_height: dst.height,
        content_dst_x,
        content_dst_y,
        content_width,
        content_height,
        source_x,
        source_y,
    };

    let mut runtime = UI4_COMPOSITOR_RUNTIME.lock();
    if runtime.pending.is_some() {
        return Err(Ui4CompositorSubmitError::Busy);
    }
    let dev = super::claimed_device().ok_or(Ui4CompositorSubmitError::Unavailable)?;
    let upload = upload_ui4_nv12_ytile_to_primary_xrgb_kernel()
        .ok_or(Ui4CompositorSubmitError::Unavailable)?;
    let state = ui4_compositor_rcs_state_once(dev).ok_or(Ui4CompositorSubmitError::Unavailable)?;

    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && (runtime.state_mapped || direct_rcs_map_state(dev, state));
    if mapped_ok {
        runtime.state_mapped = true;
    }
    let ppgtt_ok = mapped_ok && (runtime.ppgtt_initialized || direct_rcs_init_ppgtt(state));
    if ppgtt_ok {
        runtime.ppgtt_initialized = true;
    }
    let kernel_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let source_ok =
        kernel_ok && direct_rcs_map_ppgtt_kernel(state, source.gpu, source.phys, source.bytes);
    let base_ok = source_ok && direct_rcs_map_ppgtt_kernel(state, base.gpu, base.phys, base.bytes);
    let dst_ok = base_ok && direct_rcs_map_ppgtt_scanout(state, dst.gpu, dst.phys, dst.bytes);
    let batch_ok = dst_ok
        && direct_rcs_encode_ui4_nv12_tile64_to_primary_batch(
            state,
            upload,
            params,
            source.bytes,
            base.bytes,
            dst.bytes,
        );
    if !batch_ok {
        crate::log_error!(target: "ui4";
            "ui4/guc-video-compositor: queue rejected forcewake={} state={} ppgtt={} kernel={} source={} base={} dst={} batch={} source_gpu=0x{:X} base_gpu=0x{:X} dst_gpu=0x{:X}\n",
            forcewake_ok as u8,
            mapped_ok as u8,
            ppgtt_ok as u8,
            kernel_ok as u8,
            source_ok as u8,
            base_ok as u8,
            dst_ok as u8,
            batch_ok as u8,
            source.gpu,
            base.gpu,
            dst.gpu,
        );
        return Err(Ui4CompositorSubmitError::InvalidWorklist);
    }
    let started_tick = direct_rcs_now_tick();
    if !direct_rcs_submit_batch_for(
        dev,
        state,
        &mut runtime.submit,
        crate::gpu::vgpu::KernelClient::Ui4Compositor,
    ) {
        return Err(Ui4CompositorSubmitError::SubmissionRejected);
    }
    runtime.next_serial = runtime.next_serial.wrapping_add(1).max(1);
    let serial = runtime.next_serial;
    let gpu = runtime
        .submit
        .pending
        .expect("accepted UI4 submission must have an executor token");
    let submission = Ui4CompositorSubmission { serial, gpu };
    runtime.last_completion = None;
    runtime.pending = Some(Ui4CompositorPending {
        submission,
        started_tick,
        marker_slot: SPRITE_QUAD_WORKLIST_POST_MARKER_SLOT,
        marker_value: SPRITE_QUAD_WORKLIST_POST_MARKER,
        kernel: "nv12-tile64-primary",
        stats: GpgpuWorklistSubmitStats {
            descs: 1,
            walkers: 1,
            submits: 1,
            submit_ms: 0,
        },
        overdue_logged: false,
    });
    crate::log_trace!(target: "ui4";
        "ui4/guc-video-compositor: queued serial={} native=tile64-nv12 output={}x{} content={}x{}@{},{} source={},{} dst_gpu=0x{:X} ppgtt=source-pat0-wb,base-pat0-wb,dst-pat3-uc display_plane_writes=0\n",
        serial,
        dst.width,
        dst.height,
        content_width,
        content_height,
        content_dst_x,
        content_dst_y,
        source_x,
        source_y,
        dst.gpu,
    );
    Ok(submission)
}

fn gpu_ranges_overlap(left: u64, left_bytes: usize, right: u64, right_bytes: usize) -> bool {
    let Some(left_end) = left.checked_add(left_bytes as u64) else {
        return true;
    };
    let Some(right_end) = right.checked_add(right_bytes as u64) else {
        return true;
    };
    left < right_end && right < left_end
}

/// Observe one compositor marker exactly once.  This function never spins.
pub(crate) fn poll_ui4_compositor_submission(
    submission: Ui4CompositorSubmission,
) -> Ui4CompositorCompletion {
    const FAILURE_TIMEOUT_MS: u64 = 1_000;

    let mut runtime = UI4_COMPOSITOR_RUNTIME.lock();
    let Some(mut pending) = runtime.pending else {
        if let Some((retired, completion)) = runtime.last_completion
            && retired == submission
        {
            return completion;
        }
        return Ui4CompositorCompletion::InvalidSubmission;
    };
    if pending.submission != submission {
        return Ui4CompositorCompletion::InvalidSubmission;
    }
    let Some(state) = *UI4_COMPOSITOR_RCS_STATE.lock() else {
        runtime.pending = None;
        runtime.submit.pending = None;
        let completion = Ui4CompositorCompletion::Failed;
        runtime.last_completion = Some((submission, completion));
        drop(runtime);
        let _ = crate::gpu::executor::complete_kernel_submission(submission.gpu, false);
        return completion;
    };
    let observed = direct_rcs_read_result_slot(state, pending.marker_slot);
    if observed == pending.marker_value {
        pending.stats.submit_ms = direct_rcs_elapsed_ms_since(pending.started_tick);
        runtime.pending = None;
        runtime.submit.pending = None;
        let completion = Ui4CompositorCompletion::Complete(pending.stats);
        runtime.last_completion = Some((submission, completion));
        drop(runtime);
        let _ = crate::gpu::executor::complete_kernel_submission(submission.gpu, true);
        crate::log_trace!(target: "ui4";
            "ui4/guc-compositor: complete serial={} kernel={} descs={} walkers={} elapsed_ms={} poll=single\n",
            pending.submission.serial,
            pending.kernel,
            pending.stats.descs,
            pending.stats.walkers,
            pending.stats.submit_ms,
        );
        return completion;
    }
    if direct_rcs_elapsed_ms_since(pending.started_tick) >= FAILURE_TIMEOUT_MS {
        // A software timeout is not a GuC cancellation. Releasing this token
        // would let the next request overwrite the same batch/result storage
        // while the old context can still execute, and its shared marker could
        // then falsely retire the replacement request. Keep ownership pinned
        // until the marker arrives or a future real context-reset path proves
        // that execution stopped.
        if !pending.overdue_logged {
            pending.overdue_logged = true;
            runtime.pending = Some(pending);
            drop(runtime);
            crate::log_error!(target: "ui4";
                "ui4/guc-compositor: completion overdue serial={} observed=0x{:08X} want=0x{:08X} threshold_ms={} action=keep-pending-no-reuse cancellation=unavailable log=once\n",
                pending.submission.serial,
                observed,
                pending.marker_value,
                FAILURE_TIMEOUT_MS,
            );
        }
        return Ui4CompositorCompletion::Pending;
    }
    Ui4CompositorCompletion::Pending
}

/// Backend completion driver for awaitable UI4 GPU fences. Polling remains
/// dormant while there is no in-flight compositor job. The reaper itself owns
/// a fence waiter, proving the same wake path that future UI callers consume.
#[embassy_executor::task]
pub(crate) async fn gpu_completion_reaper_task() {
    use core::future::{Future, poll_fn};
    use core::pin::Pin;
    use core::task::Poll;
    use embassy_time::{Duration, Timer};

    let mut active: Option<(Ui4CompositorSubmission, crate::gpu::executor::GpuFence)> = None;
    loop {
        let pending = UI4_COMPOSITOR_RUNTIME
            .lock()
            .pending
            .map(|pending| pending.submission);
        let Some(submission) = pending else {
            active = None;
            Timer::after(Duration::from_millis(4)).await;
            continue;
        };
        if active
            .as_ref()
            .is_none_or(|(current, _)| *current != submission)
        {
            active = Some((submission, submission.fence()));
        }
        if let Some((_, fence)) = active.as_mut() {
            // Poll exactly once to register this task's waker without blocking
            // the backend marker probe that is responsible for completing it.
            let _ready = poll_fn(|cx| Poll::Ready(Pin::new(&mut *fence).poll(cx).is_ready())).await;
        }
        if !matches!(poll_ui4_compositor_submission(submission), Ui4CompositorCompletion::Pending) {
            active = None;
        }
        Timer::after(Duration::from_millis(1)).await;
    }
}

/// Render a complete Mandelbrot image into a trusted UI4 direct-scanout
/// surface. Parameters 1..=512 retain the existing descriptor worklist and
/// GuC submission; only the destination mapping and ownership result use the
/// display handoff contract.
pub(crate) fn mandel64_worklist_surface_full(
    dst: GpgpuRgba8Surface,
    iterations: u32,
) -> Option<GpgpuShellMandel64WorklistResult> {
    mandel64_worklist_surface_view_mode(dst, dst.bounds(), iterations, true, true)
}

/// Render the analytical chart node into an arbitrary trusted RGBA surface.
/// This is compute-only: the caller owns frame publication and cadence.
pub(crate) fn chart_sine_rgba8_surface_full(
    dst: GpgpuRgba8Surface,
    phase: f32,
    flags: u32,
) -> GpgpuRgba8KernelResult {
    let start_tick = direct_rcs_now_tick();
    let mut params = ChartSineRgba8Params::scope_defaults(phase, flags);
    params.rect_width = dst.width;
    params.rect_height = dst.height;
    let outcome = submit_chart_sine_rgba8(dst, params);
    let ok = outcome.observed == CHART_SINE_POST_MARKER;
    GpgpuRgba8KernelResult {
        ok,
        submitted: outcome.submitted,
        marker: outcome.observed,
        submit_ms: direct_rcs_elapsed_ms_since(start_tick),
        release: ok.then(|| gpgpu_rgba8_release(dst)),
    }
}

/// Render the procedural plasma node into an arbitrary trusted RGBA surface.
/// This is compute-only: the caller owns frame publication and cadence.
pub(crate) fn pixel_plasma_rgba8_surface_full(
    dst: GpgpuRgba8Surface,
    time: f32,
    flags: u32,
) -> GpgpuRgba8KernelResult {
    let start_tick = direct_rcs_now_tick();
    let mut params = PixelPlasmaRgba8Params::demo_defaults(time, flags);
    params.rect_width = dst.width;
    params.rect_height = dst.height;
    let outcome = submit_pixel_plasma_rgba8(dst, params);
    let ok = outcome.observed == PIXEL_PLASMA_POST_MARKER;
    GpgpuRgba8KernelResult {
        ok,
        submitted: outcome.submitted,
        marker: outcome.observed,
        submit_ms: direct_rcs_elapsed_ms_since(start_tick),
        release: ok.then(|| gpgpu_rgba8_release(dst)),
    }
}

fn gpgpu_rgba8_release(dst: GpgpuRgba8Surface) -> GpgpuRgba8ReleaseFence {
    let sequence = GPGPU_RGBA8_RELEASE_SEQUENCE
        .fetch_add(1, Ordering::Relaxed)
        .wrapping_add(1)
        .max(1);
    GpgpuRgba8ReleaseFence {
        phys: dst.phys,
        byte_len: dst.bytes,
        sequence,
    }
}

fn mandel64_worklist_surface_view_mode(
    dst: GpgpuRgba8Surface,
    view: GpgpuRect,
    iterations: u32,
    mirror_at_center: bool,
    direct_scanout: bool,
) -> Option<GpgpuShellMandel64WorklistResult> {
    if !dst.is_valid()
        || dst.width < MANDEL64_WORKLIST_CELL_PIXELS
        || dst.height < MANDEL64_WORKLIST_CELL_PIXELS
        || view.is_empty()
    {
        return None;
    }

    if iterations == 0 && direct_scanout {
        // Direct publication requires an exact producer-release token. The
        // preview never requests zero iterations, and the ordinary fill path
        // deliberately does not manufacture one.
        return None;
    }

    if iterations == 0 {
        let stats = fill_rect_rgba8_stats(dst, dst.bounds(), 0xFF00_0000);
        let submitted = stats.submits != 0;
        return Some(GpgpuShellMandel64WorklistResult {
            ok: submitted,
            submitted,
            requested: 0,
            descriptors: 0,
            walkers: 0,
            pixels: (dst.width as usize).saturating_mul(dst.height as usize),
            submit_ms: stats.submit_ms,
            ..GpgpuShellMandel64WorklistResult::default()
        });
    }

    let render_width = view.width.min(dst.width);
    let view_height = view.height.min(dst.height);
    let columns = render_width.div_ceil(MANDEL64_WORKLIST_CELL_PIXELS).max(1);
    let render_height = if mirror_at_center {
        view_height.div_ceil(2)
    } else {
        view_height
    }
    .max(1);
    let rows = render_height.div_ceil(MANDEL64_WORKLIST_CELL_PIXELS).max(1);
    let count = columns.saturating_mul(rows) as usize;
    if count == 0 {
        return None;
    }
    let iterations = iterations.clamp(1, MANDEL64_WORKLIST_MAX_ITERATIONS);
    let mut placements = Vec::new();
    let mut submitted = true;
    let mut descriptors = 0usize;
    let mut walkers = 0usize;
    let mut pixels = 0usize;
    let mut submit_ms = 0u64;
    let mut desc_gpu = 0u64;
    let mut last_src_xy = GpgpuPoint::new(0, 0);
    let mut last_dst_xy = GpgpuPoint::new(0, 0);
    let mut last_marker = 0u32;
    let mut submitted_tiles = 0usize;
    let mut index = 0usize;
    while index < count {
        let tile_batch = MANDEL64_WORKLIST_MAX_DESCS / MANDEL64_WORKLIST_BANDS_PER_TILE;
        let end = index.saturating_add(tile_batch).min(count);
        placements.clear();
        for tile_index in index..end {
            let tile_x = (tile_index as u32) % columns;
            let tile_y = (tile_index as u32) / columns;
            let dst_x = tile_x.saturating_mul(MANDEL64_WORKLIST_CELL_PIXELS);
            let dst_y = tile_y.saturating_mul(MANDEL64_WORKLIST_CELL_PIXELS);
            let width = render_width
                .saturating_sub(dst_x)
                .min(MANDEL64_WORKLIST_CELL_PIXELS);
            let height = render_height
                .saturating_sub(dst_y)
                .min(MANDEL64_WORKLIST_CELL_PIXELS);
            placements.push(GpgpuMandel64Placement {
                src_x: view.x.saturating_add(dst_x as i32),
                src_y: view.y.saturating_add(dst_y as i32),
                dst_x: dst_x as i32,
                dst_y: dst_y as i32,
                width,
                height,
                view_height,
                mirror_at_center,
                iterations,
            });
        }

        let result =
            mandel64_worklist_surface_with_policy(dst, placements.as_slice(), direct_scanout)?;
        submitted &= result.submitted;
        submitted_tiles = submitted_tiles.saturating_add(result.requested);
        descriptors = descriptors.saturating_add(result.descriptors);
        walkers = walkers.saturating_add(result.walkers);
        pixels = pixels.saturating_add(result.pixels);
        submit_ms = submit_ms.saturating_add(result.submit_ms);
        desc_gpu = result.desc_gpu;
        last_src_xy = result.last_src_xy;
        last_dst_xy = result.last_dst_xy;
        last_marker = result.marker;
        if !result.ok {
            break;
        }
        index = end;
    }

    let ok = submitted && submitted_tiles == count && last_marker == MANDEL64_WORKLIST_POST_MARKER;
    Some(GpgpuShellMandel64WorklistResult {
        ok,
        submitted,
        marker: last_marker,
        requested: count,
        descriptors,
        walkers,
        pixels,
        submit_ms,
        desc_gpu,
        last_src_xy,
        last_dst_xy,
        release: (ok && direct_scanout).then(|| gpgpu_rgba8_release(dst)),
    })
}

pub(crate) fn mandel64_worklist_surface(
    dst: GpgpuRgba8Surface,
    placements: &[GpgpuMandel64Placement],
) -> Option<GpgpuShellMandel64WorklistResult> {
    mandel64_worklist_surface_with_policy(dst, placements, false)
}

fn mandel64_worklist_surface_with_policy(
    dst: GpgpuRgba8Surface,
    placements: &[GpgpuMandel64Placement],
    direct_scanout: bool,
) -> Option<GpgpuShellMandel64WorklistResult> {
    if !dst.is_valid()
        || dst.width < MANDEL64_WORKLIST_CELL_PIXELS
        || dst.height < MANDEL64_WORKLIST_CELL_PIXELS
        || placements.is_empty()
    {
        return None;
    }
    let desc = mandel64_worklist_desc_buffer_once()?;
    let max_placements = MANDEL64_WORKLIST_MAX_DESCS / MANDEL64_WORKLIST_BANDS_PER_TILE;
    let count = placements.len().min(max_placements);
    if count == 0 {
        return None;
    }

    let mut last_src_xy = GpgpuPoint::new(0, 0);
    let mut last_dst_xy = GpgpuPoint::new(0, 0);
    let mut desc_count = 0usize;
    let mut drawn_pixels = 0usize;
    let _desc_guard = RECT_WORKLIST_DESC_SUBMIT_LOCK.lock();
    unsafe {
        core::ptr::write_bytes(desc.virt, 0, desc.bytes);
        let descs = desc.virt as *mut Mandel64WorklistRgba8Desc;
        for placement in placements.iter().take(count) {
            let src_x = placement.src_x.clamp(i16::MIN as i32, i16::MAX as i32);
            let src_y = placement.src_y.clamp(i16::MIN as i32, i16::MAX as i32);
            let dst_x = placement.dst_x.clamp(0, dst.width.saturating_sub(1) as i32);
            let dst_y = placement
                .dst_y
                .clamp(0, dst.height.saturating_sub(1) as i32);
            let requested_width = if placement.width == 0 {
                MANDEL64_WORKLIST_CELL_PIXELS
            } else {
                placement.width
            };
            let requested_height = if placement.height == 0 {
                MANDEL64_WORKLIST_CELL_PIXELS
            } else {
                placement.height
            };
            let width = requested_width
                .min(MANDEL64_WORKLIST_CELL_PIXELS)
                .min(dst.width.saturating_sub(dst_x as u32));
            let height = requested_height
                .min(MANDEL64_WORKLIST_CELL_PIXELS)
                .min(dst.height.saturating_sub(dst_y as u32));
            let iterations = placement
                .iterations
                .clamp(1, MANDEL64_WORKLIST_MAX_ITERATIONS);
            let iteration_payload = pack_mandel64_iterations(iterations);
            if width == 0 || height == 0 {
                continue;
            }
            let bands = height
                .div_ceil(MANDEL64_WORKLIST_BAND_ROWS)
                .min(MANDEL64_WORKLIST_BANDS_PER_TILE as u32);
            for band in 0..bands {
                if desc_count >= MANDEL64_WORKLIST_MAX_DESCS {
                    break;
                }
                let band_y = (band as i32).saturating_mul(MANDEL64_WORKLIST_BAND_ROWS as i32);
                let band_rows = height
                    .saturating_sub(band.saturating_mul(MANDEL64_WORKLIST_BAND_ROWS))
                    .min(MANDEL64_WORKLIST_BAND_ROWS);
                let flags = (band_rows & MANDEL64_WORKLIST_FLAG_ROWS_MASK)
                    | if placement.mirror_at_center {
                        0
                    } else {
                        MANDEL64_WORKLIST_FLAG_NO_MIRROR
                    }
                    | (width << MANDEL64_WORKLIST_FLAG_COLS_SHIFT)
                    | (placement.view_height.min(u16::MAX as u32)
                        << MANDEL64_WORKLIST_FLAG_VIEW_HEIGHT_SHIFT);
                let desc_value = Mandel64WorklistRgba8Desc {
                    src_xy: pack_i16_pair_u32(
                        src_x as i16,
                        src_y
                            .saturating_add(band_y)
                            .clamp(i16::MIN as i32, i16::MAX as i32) as i16,
                    ),
                    dst_xy: pack_i16_pair_u32(
                        dst_x as i16,
                        dst_y.saturating_add(band_y).clamp(0, dst.height as i32 - 1) as i16,
                    ),
                    flags,
                    color_rgba: iteration_payload,
                };
                core::ptr::write_volatile(descs.add(desc_count), desc_value);
                desc_count = desc_count.saturating_add(1);
            }
            let computed_pixels = (width as usize).saturating_mul(height as usize);
            let output_pixels = if !placement.mirror_at_center || placement.view_height == 0 {
                computed_pixels
            } else {
                computed_pixels.saturating_mul(2)
            };
            drawn_pixels = drawn_pixels.saturating_add(output_pixels);
            last_src_xy = GpgpuPoint::new(src_x, src_y);
            last_dst_xy = GpgpuPoint::new(dst_x, dst_y);
        }
    }
    if desc_count == 0 {
        return None;
    }
    super::dma_flush(desc.virt, desc.bytes);

    let params = Mandel64WorklistRgba8Params {
        dst_gpu: dst.gpu,
        desc_gpu: desc.gpu,
        dst_pitch_bytes: dst.pitch_bytes,
        desc_base: 0,
        desc_count: desc_count as u32,
    };
    let walkers = mandel64_worklist_walker_count(desc_count);

    let submit_start_tick = direct_rcs_now_tick();
    let outcome = submit_mandel64_worklist(dst, desc, params, direct_scanout);
    let submit_ms = direct_rcs_elapsed_ms_since(submit_start_tick);
    let ok = outcome.observed == MANDEL64_WORKLIST_POST_MARKER;

    Some(GpgpuShellMandel64WorklistResult {
        ok,
        submitted: outcome.submitted,
        marker: outcome.observed,
        requested: count,
        descriptors: desc_count,
        walkers,
        pixels: drawn_pixels,
        submit_ms,
        desc_gpu: desc.gpu,
        last_src_xy,
        last_dst_xy,
        release: None,
    })
}

pub(crate) fn activity_snapshot() -> GpgpuActivitySnapshot {
    let submit_seq = DIRECT_RCS_SUBMIT_COUNTER.load(Ordering::Relaxed);
    let Some(dev) = super::claimed_device() else {
        return GpgpuActivitySnapshot {
            direct_rcs_enabled: DIRECT_RCS_ENABLED,
            submit_seq,
            ..GpgpuActivitySnapshot::default()
        };
    };

    GpgpuActivitySnapshot {
        available: true,
        direct_rcs_enabled: DIRECT_RCS_ENABLED,
        submit_seq,
        ring_head: super::mmio_read(dev, RCS_RING_HEAD),
        ring_tail: super::mmio_read(dev, RCS_RING_TAIL),
        acthd: super::mmio_read(dev, RCS_RING_ACTHD),
        ipeir: super::mmio_read(dev, RCS_RING_IPEIR),
        ipehr: super::mmio_read(dev, RCS_RING_IPEHR),
        eir: super::mmio_read(dev, RCS_RING_EIR),
    }
}

pub(crate) fn submit_fill_rect_worklist_rgba8_probe_now() -> bool {
    submit_fill_rect_worklist_rgba8_probe(true)
}

fn submit_fill_rect_worklist_rgba8_probe(force: bool) -> bool {
    if !DIRECT_RCS_ENABLED {
        if force {
            FILL_RECT_WORKLIST_OK.store(false, Ordering::Release);
        }
        return false;
    }
    if !force && FILL_RECT_WORKLIST_RAN.swap(true, Ordering::AcqRel) {
        return false;
    }
    FILL_RECT_WORKLIST_RAN.store(true, Ordering::Release);
    FILL_RECT_WORKLIST_OK.store(false, Ordering::Release);

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: fill-rect-worklist-rgba8 skipped reason=no-claimed-device\n"
        );
        return false;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: fill-rect-worklist-rgba8 failed rung=alloc\n"
        );
        return false;
    };
    let Some(desc) = rect_worklist_desc_buffer_once() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: fill-rect-worklist-rgba8 failed rung=desc-buffer\n"
        );
        return false;
    };
    let Some(surface) = GpgpuRgba8Surface::new(
        state.clear_test_phys,
        DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE,
        CLEAR_RECT_TEST_BYTES,
        64,
        4,
        64 * core::mem::size_of::<u32>() as u32,
    ) else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: fill-rect-worklist-rgba8 failed rung=surface\n"
        );
        return false;
    };

    let _desc_guard = RECT_WORKLIST_DESC_SUBMIT_LOCK.lock();
    unsafe {
        core::ptr::write_bytes(state.clear_test_virt, 0, CLEAR_RECT_TEST_BYTES);
        core::ptr::write_bytes(desc.virt, 0, desc.bytes);
        let descs = desc.virt as *mut FillRectWorklistRgba8Desc;
        core::ptr::write_volatile(
            descs,
            FillRectWorklistRgba8Desc {
                dst_xy: pack_i16_pair_u32(0, 0),
                size: pack_u16_pair_u32(4, 1),
                color_rgba: 0xFFCC_8844,
            },
        );
        core::ptr::write_volatile(
            descs.add(1),
            FillRectWorklistRgba8Desc {
                dst_xy: pack_i16_pair_u32(8, 1),
                size: pack_u16_pair_u32(4, 2),
                color_rgba: 0xFF10_2030,
            },
        );
    }
    super::dma_flush(state.clear_test_virt, CLEAR_RECT_TEST_BYTES);
    super::dma_flush(desc.virt, desc.bytes);

    let params = FillRectWorklistRgba8Params {
        dst_gpu: surface.gpu,
        desc_gpu: desc.gpu,
        dst_pitch_bytes: surface.pitch_bytes,
        desc_base: 0,
        desc_count: 2,
    };
    let start_tick = direct_rcs_now_tick();
    let submitted = submit_fill_rect_worklist(surface, desc, params, false);
    let submit_ms = direct_rcs_elapsed_ms_since(start_tick);
    let pre_marker = direct_rcs_read_result_slot(state, RECT_WORKLIST_PRE_MARKER_SLOT);
    let post_marker = direct_rcs_read_result_slot(state, RECT_WORKLIST_POST_MARKER_SLOT);
    let row0 = direct_rcs_read_worklist_probe_span(state, 0, 0);
    let row1 = direct_rcs_read_worklist_probe_span(state, 1, 8);
    let row2 = direct_rcs_read_worklist_probe_span(state, 2, 8);
    let ok = submitted
        && pre_marker == FILL_RECT_WORKLIST_PRE_MARKER
        && post_marker == FILL_RECT_WORKLIST_POST_MARKER
        && row0 == [0xFFCC_8844; 4]
        && row1 == [0xFF10_2030; 4]
        && row2 == [0xFF10_2030; 4];

    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: fill-rect-worklist-rgba8 forcewake=1 ggtt=1 ppgtt=1 kernel_ppgtt=1 dst_ppgtt=1 desc_ppgtt=1 batch=1 submitted={} ok={} submit_ms={} descs=2 walkers={} pre_marker=0x{:08X} post_marker=0x{:08X} expected_post=0x{:08X} kernel_gpu=0x{:X} kernel_text_gpu=0x{:X} dst_gpu=0x{:X} desc_gpu=0x{:X} row0=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] row1=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] row2=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] artifact={}\n",
        submitted as u8,
        ok as u8,
        submit_ms,
        rect_worklist_walker_count(2),
        pre_marker,
        post_marker,
        FILL_RECT_WORKLIST_POST_MARKER,
        FILL_RECT_WORKLIST_RGBA8_ADLS_GPU,
        FILL_RECT_WORKLIST_RGBA8_ADLS_GPU + FILL_RECT_WORKLIST_RGBA8_TEXT_OFFSET_BYTES,
        surface.gpu,
        desc.gpu,
        row0[0],
        row0[1],
        row0[2],
        row0[3],
        row1[0],
        row1[1],
        row1[2],
        row1[3],
        row2[0],
        row2[1],
        row2[2],
        row2[3],
        FILL_RECT_WORKLIST_RGBA8_KERNEL_NAME,
    );

    FILL_RECT_WORKLIST_OK.store(ok, Ordering::Release);
    ok
}

pub(crate) fn sprite_quad_worklist_ready() -> bool {
    if SPRITE_QUAD_WORKLIST_OK.load(Ordering::Acquire) {
        return true;
    }
    let _ = submit_sprite_quad_worklist_rgba8_probe_once();
    SPRITE_QUAD_WORKLIST_OK.load(Ordering::Acquire)
}

pub(crate) fn submit_sprite_quad_worklist_rgba8_probe_once() -> bool {
    submit_sprite_quad_worklist_rgba8_probe(false)
}

fn submit_sprite_quad_worklist_rgba8_probe(force: bool) -> bool {
    if !DIRECT_RCS_ENABLED {
        if force {
            SPRITE_QUAD_WORKLIST_OK.store(false, Ordering::Release);
        }
        return false;
    }
    if !force && SPRITE_QUAD_WORKLIST_RAN.swap(true, Ordering::AcqRel) {
        return false;
    }
    SPRITE_QUAD_WORKLIST_RAN.store(true, Ordering::Release);
    SPRITE_QUAD_WORKLIST_OK.store(false, Ordering::Release);

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: sprite-quad-worklist-rgba8 skipped reason=no-claimed-device\n"
        );
        return false;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: sprite-quad-worklist-rgba8 failed rung=alloc\n"
        );
        return false;
    };
    let Some(desc) = sprite_quad_worklist_desc_buffer_once() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: sprite-quad-worklist-rgba8 failed rung=desc-buffer\n"
        );
        return false;
    };
    let Some(surface) = GpgpuRgba8Surface::new(
        state.clear_test_phys,
        DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE,
        CLEAR_RECT_TEST_BYTES,
        64,
        4,
        64 * core::mem::size_of::<u32>() as u32,
    ) else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: sprite-quad-worklist-rgba8 failed rung=surface\n"
        );
        return false;
    };

    let _desc_guard = RECT_WORKLIST_DESC_SUBMIT_LOCK.lock();
    let src00 = 0xFF00_00FF;
    let src01 = 0xFF00_FF00;
    let src10 = 0xFFFF_0000;
    let src11 = 0xFFFF_FFFF;
    unsafe {
        core::ptr::write_bytes(state.clear_test_virt, 0, CLEAR_RECT_TEST_BYTES);
        core::ptr::write_bytes(desc.virt, 0, desc.bytes);
        let pixels = state.clear_test_virt as *mut u32;
        core::ptr::write_volatile(pixels, src00);
        core::ptr::write_volatile(pixels.add(1), src01);
        core::ptr::write_volatile(pixels.add(64), src10);
        core::ptr::write_volatile(pixels.add(65), src11);
        let descs = desc.virt as *mut GpgpuSpriteQuadWorklistDesc;
        core::ptr::write_volatile(
            descs,
            GpgpuSpriteQuadWorklistDesc {
                c0_x: 10.0,
                c0_y: 1.0,
                c0_u: 0.0,
                c0_v: 0.0,
                c1_x: 12.0,
                c1_y: 1.0,
                c1_u: 2.0 / 64.0,
                c1_v: 0.0,
                c2_x: 12.0,
                c2_y: 3.0,
                c2_u: 2.0 / 64.0,
                c2_v: 2.0 / 4.0,
                c3_x: 10.0,
                c3_y: 3.0,
                c3_u: 0.0,
                c3_v: 2.0 / 4.0,
                color_rgba: 0xFFFF_FFFF,
                flags: SPRITE_QUAD_WORKLIST_FLAG_SRC_OVER,
            },
        );
    }
    super::dma_flush(state.clear_test_virt, CLEAR_RECT_TEST_BYTES);
    super::dma_flush(desc.virt, desc.bytes);

    let params = SpriteQuadWorklistRgba8Params {
        src_gpu: surface.gpu,
        dst_gpu: surface.gpu,
        desc_gpu: desc.gpu,
        src_pitch_bytes: surface.pitch_bytes,
        dst_pitch_bytes: surface.pitch_bytes,
        src_width: surface.width,
        src_height: surface.height,
        dst_width: surface.width,
        dst_height: surface.height,
        desc_base: 0,
        desc_count: 1,
    };
    let start_tick = direct_rcs_now_tick();
    let submitted = submit_sprite_quad_worklist(surface, surface, desc, params);
    let submit_ms = direct_rcs_elapsed_ms_since(start_tick);
    let pre_marker = direct_rcs_read_result_slot(state, SPRITE_QUAD_WORKLIST_PRE_MARKER_SLOT);
    let post_marker = direct_rcs_read_result_slot(state, SPRITE_QUAD_WORKLIST_POST_MARKER_SLOT);
    let row1 = direct_rcs_read_worklist_probe_span(state, 1, 10);
    let row2 = direct_rcs_read_worklist_probe_span(state, 2, 10);
    let ok = submitted
        && pre_marker == SPRITE_QUAD_WORKLIST_PRE_MARKER
        && post_marker == SPRITE_QUAD_WORKLIST_POST_MARKER
        && row1[0] == src00
        && row1[1] == src01
        && row2[0] == src10
        && row2[1] == src11;

    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: sprite-quad-worklist-rgba8 forcewake=1 ggtt=1 ppgtt=1 kernel_ppgtt=1 src_ppgtt=1 dst_ppgtt=1 desc_ppgtt=1 batch=1 submitted={} ok={} submit_ms={} descs=1 walkers={} pre_marker=0x{:08X} post_marker=0x{:08X} expected_post=0x{:08X} kernel_gpu=0x{:X} kernel_text_gpu=0x{:X} src_gpu=0x{:X} dst_gpu=0x{:X} desc_gpu=0x{:X} row1=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] row2=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] artifact={}\n",
        submitted as u8,
        ok as u8,
        submit_ms,
        sprite_quad_worklist_walker_count(1),
        pre_marker,
        post_marker,
        SPRITE_QUAD_WORKLIST_POST_MARKER,
        SPRITE_QUAD_WORKLIST_RGBA8_ADLS_GPU,
        SPRITE_QUAD_WORKLIST_RGBA8_ADLS_GPU + SPRITE_QUAD_WORKLIST_RGBA8_TEXT_OFFSET_BYTES,
        surface.gpu,
        surface.gpu,
        desc.gpu,
        row1[0],
        row1[1],
        row1[2],
        row1[3],
        row2[0],
        row2[1],
        row2[2],
        row2[3],
        SPRITE_QUAD_WORKLIST_RGBA8_KERNEL_NAME,
    );

    SPRITE_QUAD_WORKLIST_OK.store(ok, Ordering::Release);
    ok
}

fn rect_worklist_desc_buffer_once() -> Option<GpgpuRectWorklistDescBuffer> {
    let mut guard = GPGPU_RECT_WORKLIST_DESC.lock();
    if let Some(buffer) = *guard {
        return Some(buffer);
    }

    let bytes = align_up(RECT_WORKLIST_DESC_BYTES, super::WARM_ALIGN)?;
    let (phys, virt) = crate::dma::alloc(bytes, super::WARM_ALIGN)?;
    unsafe {
        core::ptr::write_bytes(virt, 0, bytes);
    }
    super::dma_flush(virt, bytes);

    let buffer = GpgpuRectWorklistDescBuffer {
        phys,
        gpu: RECT_WORKLIST_DESC_GPU,
        virt,
        bytes,
    };
    *guard = Some(buffer);
    Some(buffer)
}

fn sprite_quad_worklist_desc_buffer_once() -> Option<GpgpuRectWorklistDescBuffer> {
    let mut guard = GPGPU_SPRITE_QUAD_WORKLIST_DESC.lock();
    if let Some(buffer) = *guard {
        return Some(buffer);
    }

    let bytes = align_up(SPRITE_QUAD_WORKLIST_DESC_BYTES, super::WARM_ALIGN)?;
    let (phys, virt) = crate::dma::alloc(bytes, super::WARM_ALIGN)?;
    unsafe {
        core::ptr::write_bytes(virt, 0, bytes);
    }
    super::dma_flush(virt, bytes);

    let buffer = GpgpuRectWorklistDescBuffer {
        phys,
        gpu: SPRITE_QUAD_WORKLIST_DESC_GPU,
        virt,
        bytes,
    };
    *guard = Some(buffer);
    Some(buffer)
}

fn ui4_compositor_sprite_quad_desc_buffer_once() -> Option<GpgpuRectWorklistDescBuffer> {
    let mut guard = UI4_COMPOSITOR_SPRITE_QUAD_DESC.lock();
    if let Some(buffer) = *guard {
        return Some(buffer);
    }

    let bytes = align_up(SPRITE_QUAD_WORKLIST_DESC_BYTES, super::WARM_ALIGN)?;
    let (phys, virt) = crate::dma::alloc(bytes, super::WARM_ALIGN)?;
    unsafe {
        core::ptr::write_bytes(virt, 0, bytes);
    }
    super::dma_flush(virt, bytes);

    // This numeric VA may match the ordinary descriptor VA because the UI4
    // compositor owns a distinct PPGTT root.  The physical page is separate
    // so an ordinary GPGPU submission cannot overwrite an in-flight frame.
    let buffer = GpgpuRectWorklistDescBuffer {
        phys,
        gpu: SPRITE_QUAD_WORKLIST_DESC_GPU,
        virt,
        bytes,
    };
    *guard = Some(buffer);
    Some(buffer)
}

fn mandel64_worklist_desc_buffer_once() -> Option<GpgpuRectWorklistDescBuffer> {
    let mut guard = GPGPU_MANDEL64_WORKLIST_DESC.lock();
    if let Some(buffer) = *guard {
        return Some(buffer);
    }

    let bytes = align_up(RECT_WORKLIST_DESC_BYTES, super::WARM_ALIGN)?;
    let (phys, virt) = crate::dma::alloc(bytes, super::WARM_ALIGN)?;
    unsafe {
        core::ptr::write_bytes(virt, 0, bytes);
    }
    super::dma_flush(virt, bytes);

    let buffer = GpgpuRectWorklistDescBuffer {
        phys,
        gpu: MANDEL64_WORKLIST_DESC_GPU,
        virt,
        bytes,
    };
    *guard = Some(buffer);
    Some(buffer)
}

fn rect_is_inside_mask(surface: GpgpuMask8Surface, rect: GpgpuRect) -> bool {
    if rect.is_empty() || rect.x < 0 || rect.y < 0 {
        return false;
    }
    let x2 = rect.x as i64 + rect.width as i64;
    let y2 = rect.y as i64 + rect.height as i64;
    x2 <= surface.width as i64 && y2 <= surface.height as i64
}

fn clip_gpgpu_rect_to_surface(rect: GpgpuRect, width: u32, height: u32) -> Option<GpgpuRect> {
    if rect.is_empty() || width == 0 || height == 0 {
        return None;
    }
    let x0 = (rect.x as i64).max(0);
    let y0 = (rect.y as i64).max(0);
    let x1 = (rect.x as i64 + rect.width as i64).min(width as i64);
    let y1 = (rect.y as i64 + rect.height as i64).min(height as i64);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some(GpgpuRect::new(x0 as i32, y0 as i32, (x1 - x0) as u32, (y1 - y0) as u32))
}

fn lower_fill_rect(
    dst: GpgpuRgba8Surface,
    rect: GpgpuRect,
    color_rgba: u32,
) -> Option<FillRectRgba8Params> {
    if !dst.is_valid() || rect.is_empty() {
        return None;
    }
    let clipped = clip_rect_to_surface(rect, dst)?;
    Some(FillRectRgba8Params {
        dst_gpu: dst.gpu,
        dst_pitch_bytes: dst.pitch_bytes,
        dst_x: clipped.x as u32,
        dst_y: clipped.y as u32,
        width: clipped.width,
        height: clipped.height,
        color_rgba,
    })
}

fn lower_copy_rect(
    src: GpgpuRgba8Surface,
    src_rect: GpgpuRect,
    dst: GpgpuRgba8Surface,
    dst_xy: GpgpuPoint,
) -> Option<CopyRectRgba8Params> {
    if !src.is_valid() || !dst.is_valid() || src_rect.is_empty() {
        return None;
    }

    let mut sx = src_rect.x as i64;
    let mut sy = src_rect.y as i64;
    let mut dx = dst_xy.x as i64;
    let mut dy = dst_xy.y as i64;
    let mut width = src_rect.width as i64;
    let mut height = src_rect.height as i64;

    clip_copy_axis(&mut sx, &mut dx, &mut width, src.width as i64, dst.width as i64)?;
    clip_copy_axis(&mut sy, &mut dy, &mut height, src.height as i64, dst.height as i64)?;

    Some(CopyRectRgba8Params {
        src_gpu: src.gpu,
        dst_gpu: dst.gpu,
        src_pitch_bytes: src.pitch_bytes,
        dst_pitch_bytes: dst.pitch_bytes,
        src_x: sx as u32,
        src_y: sy as u32,
        dst_x: dx as u32,
        dst_y: dy as u32,
        width: width as u32,
        height: height as u32,
    })
}

fn lower_glyph_mask_blit(blit: GpgpuGlyphMaskBlit) -> Option<CopyRectRgba8Params> {
    if !blit.mask.is_valid() || !blit.dst.is_valid() || blit.mask_rect.is_empty() {
        return None;
    }

    let mut sx = blit.mask_rect.x as i64;
    let mut sy = blit.mask_rect.y as i64;
    let mut dx = blit.dst_xy.x as i64;
    let mut dy = blit.dst_xy.y as i64;
    let mut width = blit.mask_rect.width as i64;
    let mut height = blit.mask_rect.height as i64;

    clip_copy_axis(&mut sx, &mut dx, &mut width, blit.mask.width as i64, blit.dst.width as i64)?;
    clip_copy_axis(&mut sy, &mut dy, &mut height, blit.mask.height as i64, blit.dst.height as i64)?;

    Some(CopyRectRgba8Params {
        src_gpu: blit.mask.gpu,
        dst_gpu: blit.dst.gpu,
        src_pitch_bytes: blit.mask.pitch_bytes,
        dst_pitch_bytes: blit.dst.pitch_bytes,
        src_x: sx as u32,
        src_y: sy as u32,
        dst_x: dx as u32,
        dst_y: dy as u32,
        width: width as u32,
        height: height as u32,
    })
}

fn clip_rect_to_surface(rect: GpgpuRect, surface: GpgpuRgba8Surface) -> Option<GpgpuRect> {
    let mut x = rect.x as i64;
    let mut y = rect.y as i64;
    let mut width = rect.width as i64;
    let mut height = rect.height as i64;

    if x < 0 {
        width += x;
        x = 0;
    }
    if y < 0 {
        height += y;
        y = 0;
    }
    width = width.min(surface.width as i64 - x);
    height = height.min(surface.height as i64 - y);
    if width <= 0 || height <= 0 {
        return None;
    }
    Some(GpgpuRect::new(x as i32, y as i32, width as u32, height as u32))
}

fn clip_copy_axis(
    src_pos: &mut i64,
    dst_pos: &mut i64,
    len: &mut i64,
    src_limit: i64,
    dst_limit: i64,
) -> Option<()> {
    if *src_pos < 0 {
        let delta = -*src_pos;
        *src_pos = 0;
        *dst_pos += delta;
        *len -= delta;
    }
    if *dst_pos < 0 {
        let delta = -*dst_pos;
        *dst_pos = 0;
        *src_pos += delta;
        *len -= delta;
    }
    *len = (*len).min(src_limit - *src_pos).min(dst_limit - *dst_pos);
    if *len <= 0 { None } else { Some(()) }
}

fn submit_fill_rect_2d_with_stats(
    dst: GpgpuRgba8Surface,
    params: FillRectRgba8Params,
) -> GpgpuSubmitStats {
    let total_start_tick = direct_rcs_now_tick();
    let Some(dispatch) = fill_rect_2d_dispatch(params.width, params.height) else {
        return GpgpuSubmitStats::default();
    };
    let Some(total_spans) = (dispatch.group_x as usize).checked_mul(dispatch.group_y as usize)
    else {
        return GpgpuSubmitStats::default();
    };
    let submit_start_tick = direct_rcs_now_tick();
    if !submit_fill_rect_2d(dst, params) {
        return GpgpuSubmitStats {
            total_ms: direct_rcs_elapsed_ms_since(total_start_tick),
            ..GpgpuSubmitStats::default()
        };
    }
    GpgpuSubmitStats {
        spans: total_spans,
        submits: 1,
        submit_ms: direct_rcs_elapsed_ms_since(submit_start_tick),
        total_ms: direct_rcs_elapsed_ms_since(total_start_tick),
        ..GpgpuSubmitStats::default()
    }
}

fn submit_copy_rect_2d(
    src: GpgpuRgba8Surface,
    dst: GpgpuRgba8Surface,
    params: CopyRectRgba8Params,
    direct_scanout: bool,
) -> bool {
    if params.width == 0 || params.height == 0 {
        return false;
    }
    let Some(dispatch) = copy_rect_2d_dispatch(params.width, params.height) else {
        return false;
    };
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        return false;
    };
    let Some(upload) = upload_copy_rect_rgba8_kernel() else {
        return false;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        return false;
    };

    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ppgtt_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let src_ppgtt_ok =
        kernel_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, params.src_gpu, src.phys, src.bytes);
    let dst_ppgtt_ok = src_ppgtt_ok
        && direct_rcs_map_ppgtt_destination(
            state,
            params.dst_gpu,
            dst.phys,
            dst.bytes,
            direct_scanout,
        );
    let batch_ok = dst_ppgtt_ok
        && direct_rcs_encode_copy_rect_2d_batch(state, upload, params, src.bytes, dst.bytes);
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result_slot_timeout_ms(
            state,
            COPY_RECT_POST_MARKER_SLOT,
            COPY_RECT_POST_MARKER,
            COPY_RECT_2D_COMPLETION_TIMEOUT_MS,
        )
    } else {
        0
    };
    let completed = observed == COPY_RECT_POST_MARKER;
    if !completed {
        let occurrence = COPY_RECT_2D_INCOMPLETE_SEQ.fetch_add(1, Ordering::Relaxed) + 1;
        if occurrence <= 8 || occurrence.is_multiple_of(20) {
            let pre_marker = direct_rcs_read_result_slot(state, COPY_RECT_PRE_MARKER_SLOT);
            let potential_reason = if !batch_ok {
                "batch-prepare"
            } else if !submitted {
                "guc-submit"
            } else if pre_marker != COPY_RECT_PRE_MARKER {
                "batch-not-started"
            } else {
                "walker-not-retired-before-timeout"
            };
            crate::log_warn!(
                target: "intel-gpgpu";
                "copy_rect_rgba8 2d incomplete occurrence={} rect={}x{} groups={}x{} pre=0x{:08X} post=0x{:08X} timeout_ms={} potential_reason={} action=fail-closed\n",
                occurrence,
                params.width,
                params.height,
                dispatch.group_x,
                dispatch.group_y,
                pre_marker,
                observed,
                COPY_RECT_2D_COMPLETION_TIMEOUT_MS,
                potential_reason,
            );
        }
    }
    completed
}

fn submit_fill_rect_2d(dst: GpgpuRgba8Surface, params: FillRectRgba8Params) -> bool {
    if params.width == 0 || params.height == 0 {
        return false;
    }
    let Some(dispatch) = fill_rect_2d_dispatch(params.width, params.height) else {
        return false;
    };
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        return false;
    };
    let Some(upload) = upload_fill_rect_rgba8_kernel() else {
        return false;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        return false;
    };

    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ppgtt_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let dst_ppgtt_ok =
        kernel_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, dst.gpu, dst.phys, dst.bytes);
    let batch_ok =
        dst_ppgtt_ok && direct_rcs_encode_fill_rect_2d_batch(state, upload, params, dst.bytes);
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result_slot_timeout_ms(
            state,
            CLEAR_RECT_POST_MARKER_SLOT,
            CLEAR_RECT_POST_MARKER,
            FILL_RECT_2D_COMPLETION_TIMEOUT_MS,
        )
    } else {
        0
    };
    let completed = observed == CLEAR_RECT_POST_MARKER;
    if !completed {
        let occurrence = FILL_RECT_2D_INCOMPLETE_SEQ.fetch_add(1, Ordering::Relaxed) + 1;
        if occurrence <= 8 || occurrence.is_multiple_of(20) {
            let pre_marker = direct_rcs_read_result_slot(state, CLEAR_RECT_PRE_MARKER_SLOT);
            let potential_reason = if !batch_ok {
                "batch-prepare"
            } else if !submitted {
                "guc-submit"
            } else if pre_marker != CLEAR_RECT_PRE_MARKER {
                "batch-not-started"
            } else {
                "walker-not-retired-before-timeout"
            };
            crate::log_warn!(
                target: "intel-gpgpu";
                "fill_rect_rgba8 2d incomplete occurrence={} rect={}x{} groups={}x{} pre=0x{:08X} post=0x{:08X} timeout_ms={} potential_reason={} action=fail-closed\n",
                occurrence,
                params.width,
                params.height,
                dispatch.group_x,
                dispatch.group_y,
                pre_marker,
                observed,
                FILL_RECT_2D_COMPLETION_TIMEOUT_MS,
                potential_reason,
            );
        }
    }
    completed
}

fn submit_resolve_tile64_msaa4_2d(
    src: GpgpuRgba8Surface,
    dst: GpgpuRgba8Surface,
    params: CopyRectRgba8Params,
    direct_scanout: bool,
) -> bool {
    if params.width == 0 || params.height == 0 {
        return false;
    }
    let Some(dispatch) = fill_rect_2d_dispatch(params.width, params.height) else {
        return false;
    };
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        return false;
    };
    let Some(upload) = upload_resolve_tile64_msaa4_rgba8_kernel() else {
        return false;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        return false;
    };

    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ppgtt_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let src_ppgtt_ok =
        kernel_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, params.src_gpu, src.phys, src.bytes);
    let dst_ppgtt_ok = src_ppgtt_ok
        && direct_rcs_map_ppgtt_destination(
            state,
            params.dst_gpu,
            dst.phys,
            dst.bytes,
            direct_scanout,
        );
    let batch_ok = dst_ppgtt_ok
        && direct_rcs_encode_resolve_tile64_msaa4_2d_batch(
            state, upload, params, src.bytes, dst.bytes,
        );
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result_slot_timeout_ms(
            state,
            COPY_RECT_POST_MARKER_SLOT,
            COPY_RECT_POST_MARKER,
            RESOLVE_TILE64_MSAA4_COMPLETION_TIMEOUT_MS,
        )
    } else {
        0
    };
    let completed = observed == COPY_RECT_POST_MARKER;
    if !completed {
        let occurrence = RESOLVE_TILE64_MSAA4_INCOMPLETE_SEQ.fetch_add(1, Ordering::Relaxed) + 1;
        if occurrence <= 8 || occurrence.is_multiple_of(20) {
            let pre_marker = direct_rcs_read_result_slot(state, COPY_RECT_PRE_MARKER_SLOT);
            let potential_reason = if !batch_ok {
                "batch-prepare"
            } else if !submitted {
                "guc-submit"
            } else if pre_marker != COPY_RECT_PRE_MARKER {
                "batch-not-started"
            } else {
                "walker-not-retired-before-timeout"
            };
            crate::log_warn!(
                target: "intel-gpgpu";
                "resolve_tile64_msaa4_rgba8 2d incomplete occurrence={} rect={}x{} groups={}x{} pre=0x{:08X} post=0x{:08X} timeout_ms={} potential_reason={} action=fail-closed\n",
                occurrence,
                params.width,
                params.height,
                dispatch.group_x,
                dispatch.group_y,
                pre_marker,
                observed,
                RESOLVE_TILE64_MSAA4_COMPLETION_TIMEOUT_MS,
                potential_reason,
            );
        }
    }
    completed
}

fn submit_font_outline_coverage_r8_2d(
    ops_phys: u64,
    ops_bytes: usize,
    mask: GpgpuMask8Surface,
    params: FontOutlineCoverageR8Params,
) -> bool {
    let Some(dispatch) = fill_rect_2d_dispatch(params.rect_width, params.rect_height) else {
        return false;
    };
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        return false;
    };
    let Some(upload) = upload_font_outline_coverage_r8_kernel() else {
        return false;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        return false;
    };
    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ppgtt_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let ops_ppgtt_ok =
        kernel_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, params.ops_gpu, ops_phys, ops_bytes);
    let mask_ppgtt_ok =
        ops_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, params.mask_gpu, mask.phys, mask.bytes);
    let batch_ok = mask_ppgtt_ok
        && direct_rcs_encode_font_outline_coverage_r8_2d_batch(
            state, upload, params, ops_bytes, mask.bytes,
        );
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result_slot_timeout_ms(
            state,
            COPY_RECT_POST_MARKER_SLOT,
            COPY_RECT_POST_MARKER,
            FONT_OUTLINE_COVERAGE_R8_COMPLETION_TIMEOUT_MS,
        )
    } else {
        0
    };
    let completed = observed == COPY_RECT_POST_MARKER;
    if !completed {
        let occurrence =
            FONT_OUTLINE_COVERAGE_R8_INCOMPLETE_SEQ.fetch_add(1, Ordering::Relaxed) + 1;
        if occurrence <= 8 || occurrence.is_multiple_of(20) {
            crate::log_warn!(
                target: "intel-gpgpu";
                "font_outline_coverage_r8 incomplete occurrence={} ops={} rect={}x{} groups={}x{} submitted={} post=0x{:08X} timeout_ms={} action=triangle-fallback\n",
                occurrence,
                params.op_count,
                params.rect_width,
                params.rect_height,
                dispatch.group_x,
                dispatch.group_y,
                submitted as u8,
                observed,
                FONT_OUTLINE_COVERAGE_R8_COMPLETION_TIMEOUT_MS,
            );
        }
    }
    completed
}

fn submit_glyph_mask_2d(
    mask: GpgpuMask8Surface,
    dst: GpgpuRgba8Surface,
    params: CopyRectRgba8Params,
    color_rgba: u32,
    direct_scanout: bool,
) -> bool {
    if fill_rect_2d_dispatch(params.width, params.height).is_none() {
        return false;
    }
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        return false;
    };
    let Some(upload) = upload_glyph_mask_rgba8_kernel() else {
        return false;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        return false;
    };
    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ppgtt_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let mask_ppgtt_ok = kernel_ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, params.src_gpu, mask.phys, mask.bytes);
    let dst_ppgtt_ok = mask_ppgtt_ok
        && direct_rcs_map_ppgtt_destination(
            state,
            params.dst_gpu,
            dst.phys,
            dst.bytes,
            direct_scanout,
        );
    let batch_ok = dst_ppgtt_ok
        && direct_rcs_encode_glyph_mask_2d_batch(
            state, upload, params, color_rgba, mask.bytes, dst.bytes,
        );
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result_slot_timeout_ms(
            state,
            COPY_RECT_POST_MARKER_SLOT,
            COPY_RECT_POST_MARKER,
            RESOLVE_TILE64_MSAA4_COMPLETION_TIMEOUT_MS,
        )
    } else {
        0
    };
    observed == COPY_RECT_POST_MARKER
}

fn submit_glyph_mask_layers_2d(
    layers: &[GpgpuGlyphMaskLayer],
    dst: GpgpuRgba8Surface,
    direct_scanout: bool,
) -> (bool, bool) {
    if layers.is_empty() || layers.len() > GLYPH_MASK_BATCH_MAX_LAYERS {
        return (false, false);
    }
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        return (false, false);
    };
    let Some(upload) = upload_glyph_mask_rgba8_kernel() else {
        return (false, false);
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        return (false, false);
    };
    if !direct_rcs_forcewake(dev)
        || !direct_rcs_map_state(dev, state)
        || !direct_rcs_init_ppgtt(state)
        || !direct_rcs_map_ppgtt_region(
            state,
            upload.gpu,
            upload.phys,
            upload.mapped_bytes,
            direct_rcs_ppgtt_pte_flags(),
        )
        || !direct_rcs_map_ppgtt_destination(state, dst.gpu, dst.phys, dst.bytes, direct_scanout)
    {
        return (false, false);
    }
    for layer in layers {
        let blit = GpgpuGlyphMaskBlit {
            mask: layer.mask,
            mask_rect: layer.mask_rect,
            dst,
            dst_xy: layer.dst_xy,
            color_rgba: layer.color_rgba,
        };
        if lower_glyph_mask_blit(blit).is_none() {
            continue;
        }
        if !direct_rcs_map_ppgtt_region(
            state,
            layer.mask.gpu,
            layer.mask.phys,
            layer.mask.bytes,
            direct_rcs_ppgtt_pte_flags(),
        ) {
            return (false, false);
        }
    }
    // All scene masks share one address space and one submission. Publishing
    // their PTEs together avoids flushing the complete 2 MiB page table once
    // per layer.
    super::dma_flush(state.ppgtt_virt, DIRECT_RCS_PPGTT_BYTES);
    if !direct_rcs_encode_glyph_mask_layers_2d_batch(state, upload, layers, dst) {
        return (false, false);
    }
    let submitted = direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result_slot_timeout_ms(
            state,
            COPY_RECT_POST_MARKER_SLOT,
            COPY_RECT_POST_MARKER,
            RESOLVE_TILE64_MSAA4_COMPLETION_TIMEOUT_MS,
        )
    } else {
        0
    };
    let completed = observed == COPY_RECT_POST_MARKER;
    if !completed {
        let occurrence = GLYPH_MASK_BATCH_INCOMPLETE_SEQ.fetch_add(1, Ordering::Relaxed) + 1;
        if occurrence <= 8 || occurrence.is_multiple_of(60) {
            crate::log_warn!(
                target: "intel-gpgpu";
                "glyph_mask_rgba8 batch incomplete occurrence={} layers={} submitted={} pre=0x{:08X} post=0x{:08X} timeout_ms={} action=fail-closed-and-rerender-scene\n",
                occurrence,
                layers.len(),
                submitted as u8,
                direct_rcs_read_result_slot(state, COPY_RECT_PRE_MARKER_SLOT),
                observed,
                RESOLVE_TILE64_MSAA4_COMPLETION_TIMEOUT_MS,
            );
        }
    }
    (submitted, completed)
}

pub(crate) fn skybox_sample_rgb565_to_rgba8(
    skybox: GpgpuRgb565Surface,
    dst: GpgpuRgba8Surface,
    mut params: SkyboxSampleRgb565Params,
) -> GpgpuRgba8KernelResult {
    let started = direct_rcs_now_tick();
    if !skybox.is_valid() || !dst.is_valid() || params.rect_width == 0 || params.rect_height == 0 {
        return GpgpuRgba8KernelResult::default();
    }
    if params.rect_x >= dst.width || params.rect_y >= dst.height {
        return GpgpuRgba8KernelResult::default();
    }
    params.sky_gpu = skybox.gpu;
    params.dst_gpu = dst.gpu;
    params.sky_pitch_bytes = skybox.pitch_bytes;
    params.sky_width = skybox.width;
    params.sky_height = skybox.height;
    params.dst_pitch_bytes = dst.pitch_bytes;
    params.dst_width = dst.width;
    params.dst_height = dst.height;
    params.rect_width = params.rect_width.min(dst.width - params.rect_x);
    params.rect_height = params.rect_height.min(dst.height - params.rect_y);

    let seq = PRESENT_RGBA8_TO_PRIMARY_XRGB_LOG_SEQ.fetch_add(1, Ordering::Relaxed) + 1;
    let trace = seq <= 8 || seq % 120 == 0;
    if trace {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: skybox-sample-rgb565 begin seq={} rect={}x{} dst={}x{} sky={}x{} sky_gpu=0x{:X} dst_gpu=0x{:X}\n",
            seq,
            params.rect_width,
            params.rect_height,
            dst.width,
            dst.height,
            skybox.width,
            skybox.height,
            skybox.gpu,
            dst.gpu
        );
    }

    // The skybox owns one UI4 write lease. Queue behind the shared RCS lane
    // instead of converting transient engine contention into a permanent CPU
    // fallback for the Blueprint.
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        if trace {
            crate::log_info!(target: "gpgpu"; "intel/gpgpu: skybox-sample-rgb565 no claimed device seq={}\n", seq);
        }
        return GpgpuRgba8KernelResult::default();
    };
    let Some(upload) = upload_skybox_sample_rgb565_kernel() else {
        if trace {
            crate::log_info!(target: "gpgpu"; "intel/gpgpu: skybox-sample-rgb565 kernel upload unavailable seq={}\n", seq);
        }
        return GpgpuRgba8KernelResult::default();
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        if trace {
            crate::log_info!(target: "gpgpu"; "intel/gpgpu: skybox-sample-rgb565 direct state unavailable seq={}\n", seq);
        }
        return GpgpuRgba8KernelResult::default();
    };

    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ppgtt_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let sky_ppgtt_ok = kernel_ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, skybox.gpu, skybox.phys, skybox.bytes);
    let dst_ppgtt_ok =
        sky_ppgtt_ok && direct_rcs_map_ppgtt_scanout(state, dst.gpu, dst.phys, dst.bytes);
    let batch_ok = dst_ppgtt_ok
        && direct_rcs_encode_skybox_sample_rgb565_batch(
            state,
            upload,
            params,
            skybox.bytes,
            dst.bytes,
        );
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result_slot_timeout_ms(
            state,
            SKYBOX_SAMPLE_POST_MARKER_SLOT,
            SKYBOX_SAMPLE_POST_MARKER,
            UI4_COMPUTE_PRODUCER_RETIRE_TIMEOUT_MS,
        )
    } else {
        0
    };
    let ok = observed == SKYBOX_SAMPLE_POST_MARKER;
    if ok {
        if trace {
            crate::log_info!(
                target: "gpgpu";
                "intel/gpgpu: skybox-sample-rgb565 submitted=1 seq={} size={}x{} dst={}x{} marker=0x{:X}\n",
                seq,
                params.rect_width,
                params.rect_height,
                dst.width,
                dst.height,
                observed
            );
        }
    } else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: skybox-sample-rgb565 failed seq={} forcewake={} mapped={} ppgtt={} kernel={} sky={} dst={} batch={} submitted={} observed=0x{:X} want=0x{:X} upload_gpu=0x{:X} sky_gpu=0x{:X} dst_gpu=0x{:X} sky_bytes=0x{:X} dst_bytes=0x{:X}\n",
            seq,
            forcewake_ok as u8,
            mapped_ok as u8,
            ppgtt_ok as u8,
            kernel_ppgtt_ok as u8,
            sky_ppgtt_ok as u8,
            dst_ppgtt_ok as u8,
            batch_ok as u8,
            submitted as u8,
            observed,
            SKYBOX_SAMPLE_POST_MARKER,
            upload.gpu,
            skybox.gpu,
            dst.gpu,
            skybox.bytes,
            dst.bytes
        );
    }
    GpgpuRgba8KernelResult {
        ok,
        submitted,
        marker: observed,
        submit_ms: direct_rcs_elapsed_ms_since(started),
        release: ok.then(|| gpgpu_rgba8_release(dst)),
    }
}

fn submit_chart_sine_rgba8(
    dst: GpgpuRgba8Surface,
    mut params: ChartSineRgba8Params,
) -> DirectRcsDispatchOutcome {
    if !dst.is_valid()
        || params.rect_width == 0
        || params.rect_height == 0
        || !params.phase.is_finite()
        || !params.cycles.is_finite()
        || !params.amplitude.is_finite()
        || !params.line_width_px.is_finite()
    {
        return DirectRcsDispatchOutcome::default();
    }
    if params.rect_x >= dst.width || params.rect_y >= dst.height {
        return DirectRcsDispatchOutcome::default();
    }
    params.dst_gpu = dst.gpu;
    params.dst_pitch_bytes = dst.pitch_bytes;
    params.dst_width = dst.width;
    params.dst_height = dst.height;
    params.rect_width = params.rect_width.min(dst.width - params.rect_x);
    params.rect_height = params.rect_height.min(dst.height - params.rect_y);
    params.cycles = params.cycles.clamp(0.25, 32.0);
    params.amplitude = params.amplitude.clamp(0.0, 0.48);
    params.line_width_px = params.line_width_px.clamp(0.75, 8.0);

    // Chart work shares RCS0. Back-pressure behind an in-flight dispatch.
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        crate::log_warn!(
            target: "gpgpu";
            "intel/gpgpu: chart-sine-rgba8 submit rejected reason=no-claimed-device\n"
        );
        return DirectRcsDispatchOutcome::default();
    };
    let Some(upload) = upload_chart_sine_rgba8_kernel() else {
        return DirectRcsDispatchOutcome::default();
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        return DirectRcsDispatchOutcome::default();
    };

    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ppgtt_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let dst_ppgtt_ok =
        kernel_ppgtt_ok && direct_rcs_map_ppgtt_scanout(state, dst.gpu, dst.phys, dst.bytes);
    let batch_ok =
        dst_ppgtt_ok && direct_rcs_encode_chart_sine_rgba8_batch(state, upload, params, dst.bytes);
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result_slot_timeout_ms(
            state,
            CHART_SINE_POST_MARKER_SLOT,
            CHART_SINE_POST_MARKER,
            UI4_COMPUTE_PRODUCER_RETIRE_TIMEOUT_MS,
        )
    } else {
        0
    };
    if observed != CHART_SINE_POST_MARKER {
        if submitted {
            quarantine_direct_rcs_context("chart-sine-marker-timeout");
        }
        crate::log_error!(
            target: "gpgpu";
            "intel/gpgpu: chart-sine-rgba8 failed forcewake={} mapped={} ppgtt={} kernel={} dst={} batch={} submitted={} observed=0x{:08X} want=0x{:08X} size={}x{} kernel_gpu=0x{:X} dst_gpu=0x{:X}\n",
            forcewake_ok as u8,
            mapped_ok as u8,
            ppgtt_ok as u8,
            kernel_ppgtt_ok as u8,
            dst_ppgtt_ok as u8,
            batch_ok as u8,
            submitted as u8,
            observed,
            CHART_SINE_POST_MARKER,
            params.rect_width,
            params.rect_height,
            upload.gpu,
            dst.gpu
        );
        return DirectRcsDispatchOutcome {
            submitted,
            observed,
        };
    }
    DirectRcsDispatchOutcome {
        submitted,
        observed,
    }
}

fn submit_pixel_plasma_rgba8(
    dst: GpgpuRgba8Surface,
    mut params: PixelPlasmaRgba8Params,
) -> DirectRcsDispatchOutcome {
    if !dst.is_valid()
        || params.rect_width == 0
        || params.rect_height == 0
        || !params.time.is_finite()
        || !params.spatial_scale.is_finite()
        || !params.intensity.is_finite()
    {
        return DirectRcsDispatchOutcome::default();
    }
    if params.rect_x >= dst.width || params.rect_y >= dst.height {
        return DirectRcsDispatchOutcome::default();
    }
    params.dst_gpu = dst.gpu;
    params.dst_pitch_bytes = dst.pitch_bytes;
    params.dst_width = dst.width;
    params.dst_height = dst.height;
    params.rect_width = params.rect_width.min(dst.width - params.rect_x);
    params.rect_height = params.rect_height.min(dst.height - params.rect_y);
    params.spatial_scale = params.spatial_scale.clamp(0.25, 8.0);
    params.intensity = params.intensity.clamp(0.25, 2.0);

    let Some(_guard) = DIRECT_RCS_SUBMIT_LOCK.try_lock() else {
        crate::log_warn!(
            target: "gpgpu";
            "intel/gpgpu: pixel-plasma-rgba8 submit rejected reason=direct-submit-busy\n"
        );
        return DirectRcsDispatchOutcome::default();
    };
    let Some(dev) = super::claimed_device() else {
        crate::log_warn!(
            target: "gpgpu";
            "intel/gpgpu: pixel-plasma-rgba8 submit rejected reason=no-claimed-device\n"
        );
        return DirectRcsDispatchOutcome::default();
    };
    let Some(upload) = upload_pixel_plasma_rgba8_kernel() else {
        return DirectRcsDispatchOutcome::default();
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        return DirectRcsDispatchOutcome::default();
    };

    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ppgtt_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let dst_ppgtt_ok =
        kernel_ppgtt_ok && direct_rcs_map_ppgtt_scanout(state, dst.gpu, dst.phys, dst.bytes);
    let batch_ok = dst_ppgtt_ok
        && direct_rcs_encode_pixel_plasma_rgba8_batch(state, upload, params, dst.bytes);
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result_slot_timeout_ms(
            state,
            PIXEL_PLASMA_POST_MARKER_SLOT,
            PIXEL_PLASMA_POST_MARKER,
            UI4_COMPUTE_PRODUCER_RETIRE_TIMEOUT_MS,
        )
    } else {
        0
    };
    if observed != PIXEL_PLASMA_POST_MARKER {
        if submitted {
            quarantine_direct_rcs_context("pixel-plasma-marker-timeout");
        }
        crate::log_error!(
            target: "gpgpu";
            "intel/gpgpu: pixel-plasma-rgba8 failed forcewake={} mapped={} ppgtt={} kernel={} dst={} batch={} submitted={} observed=0x{:08X} want=0x{:08X} size={}x{} kernel_gpu=0x{:X} dst_gpu=0x{:X}\n",
            forcewake_ok as u8,
            mapped_ok as u8,
            ppgtt_ok as u8,
            kernel_ppgtt_ok as u8,
            dst_ppgtt_ok as u8,
            batch_ok as u8,
            submitted as u8,
            observed,
            PIXEL_PLASMA_POST_MARKER,
            params.rect_width,
            params.rect_height,
            upload.gpu,
            dst.gpu
        );
        return DirectRcsDispatchOutcome {
            submitted,
            observed,
        };
    }
    DirectRcsDispatchOutcome {
        submitted,
        observed,
    }
}

pub(crate) fn shell_font_outline_probe(
    ops: &[[u32; 8]],
    expected_checksum: u32,
    stage: u32,
    units_per_em: u16,
) -> GpgpuFontOutlineProbeResult {
    let mut result = GpgpuFontOutlineProbeResult {
        op_count: ops.len().min(u32::MAX as usize) as u32,
        expected_checksum,
        ..GpgpuFontOutlineProbeResult::default()
    };
    if ops.is_empty()
        || ops.len() > FONT_OUTLINE_MESH_MAX_OPS
        || !(FONT_OUTLINE_STAGE_AUDIT..=FONT_OUTLINE_STAGE_STROKE_MESH).contains(&stage)
    {
        crate::log_error!(
            target: "gpgpu";
            "intel/gpgpu: font-outline rejected stage={} ops={} max_ops={} reason=invalid-request\n",
            stage,
            ops.len(),
            FONT_OUTLINE_MESH_MAX_OPS,
        );
        return result;
    }
    let Some(_guard) = DIRECT_RCS_SUBMIT_LOCK.try_lock() else {
        crate::log_warn!(
            target: "gpgpu";
            "intel/gpgpu: font-outline-mesh rejected stage={} reason=direct-submit-busy\n",
            stage
        );
        return result;
    };
    let Some(dev) = super::claimed_device() else {
        return result;
    };
    result.available = true;
    let Some(upload) = upload_font_outline_mesh_kernel() else {
        return result;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        return result;
    };

    let input_bytes = ops.len() * core::mem::size_of::<[u32; 8]>();
    unsafe {
        core::ptr::write_bytes(state.clear_test_virt, 0, CLEAR_RECT_TEST_BYTES);
        core::ptr::copy_nonoverlapping(
            ops.as_ptr().cast::<u8>(),
            state.clear_test_virt,
            input_bytes,
        );
        core::ptr::write_bytes(state.canvas3d_out_virt, 0, CANVAS3D_PROJECT_OUT_ALLOC_BYTES);
    }
    super::dma_flush(state.clear_test_virt, input_bytes);
    super::dma_flush(state.canvas3d_out_virt, CANVAS3D_PROJECT_OUT_ALLOC_BYTES);

    let params = FontOutlineMeshParams {
        src_gpu: DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE,
        dst_gpu: DIRECT_RCS_GPU_VA_CANVAS3D_OUT_BASE,
        op_count: ops.len() as u32,
        stage,
        subdivisions: 8,
        max_vertices: FONT_OUTLINE_MESH_MAX_VERTICES,
        max_indices: FONT_OUTLINE_MESH_MAX_INDICES,
        // Fit the complete sample string into clip space. The kernel keeps
        // font Y-up orientation; the render viewport performs the screen flip.
        scale: 0.32 / f32::from(units_per_em.max(1)),
        origin_x: -0.85,
        origin_y: -0.25,
        stroke_half_width: 0.008,
    };
    result.forcewake_ok = direct_rcs_forcewake(dev);
    result.mapped_ok = result.forcewake_ok && direct_rcs_map_state(dev, state);
    result.ppgtt_ok = result.mapped_ok && direct_rcs_init_ppgtt(state);
    result.kernel_ppgtt_ok = result.ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    result.src_ppgtt_ok = result.kernel_ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(
            state,
            params.src_gpu,
            state.clear_test_phys,
            CLEAR_RECT_TEST_BYTES,
        );
    result.dst_ppgtt_ok = result.src_ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(
            state,
            params.dst_gpu,
            state.canvas3d_out_phys,
            CANVAS3D_PROJECT_OUT_ALLOC_BYTES,
        );
    result.batch_ok = result.dst_ppgtt_ok
        && direct_rcs_encode_font_outline_mesh_batch(
            state,
            upload,
            params,
            input_bytes,
            CANVAS3D_PROJECT_OUT_ALLOC_BYTES,
        );
    let submit_start_tick = direct_rcs_now_tick();
    result.submitted = result.batch_ok && direct_rcs_submit_batch(dev, state);
    let (observed, retire_ms) = if result.submitted {
        direct_rcs_poll_result_slot_elapsed(
            state,
            FONT_OUTLINE_MESH_POST_MARKER_SLOT,
            FONT_OUTLINE_MESH_POST_MARKER,
            submit_start_tick,
        )
    } else {
        (0, 0)
    };
    result.retire_ms = retire_ms;
    result.pre_marker = direct_rcs_read_result_slot(state, FONT_OUTLINE_MESH_PRE_MARKER_SLOT);
    result.post_marker = observed;
    result.retired = observed == FONT_OUTLINE_MESH_POST_MARKER;

    super::dma_flush(state.canvas3d_out_virt, CANVAS3D_PROJECT_OUT_ALLOC_BYTES);
    let report = unsafe { core::slice::from_raw_parts(state.canvas3d_out_virt as *const u32, 25) };
    result.report_marker = report[0];
    result.done_marker = report[24];
    result.kernel_done = report[24] == FONT_OUTLINE_MESH_RESULT_DONE;
    result.op_count = report[3];
    result.move_count = report[4];
    result.line_count = report[5];
    result.quad_count = report[6];
    result.cubic_count = report[7];
    result.close_count = report[8];
    result.vertices = report[9];
    result.segments = report[10];
    result.indices = report[12];
    result.checksum = report[13];
    result.invalid = report[14];
    result.truncated = report[15] != 0;
    result.min_x = f32::from_bits(report[16]);
    result.min_y = f32::from_bits(report[17]);
    result.max_x = f32::from_bits(report[18]);
    result.max_y = f32::from_bits(report[19]);
    let layout_ok = report[21] == FONT_OUTLINE_MESH_LAYOUT_VERSION
        && report[22] == FONT_OUTLINE_MESH_VERTEX_DWORD_OFFSET
        && report[23] == FONT_OUTLINE_MESH_INDEX_DWORD_OFFSET;
    result.indices_in_range = if stage == FONT_OUTLINE_STAGE_STROKE_MESH
        && result.indices <= FONT_OUTLINE_MESH_MAX_INDICES
    {
        let indices = unsafe {
            core::slice::from_raw_parts(
                (state.canvas3d_out_virt as *const u32)
                    .add(FONT_OUTLINE_MESH_INDEX_DWORD_OFFSET as usize),
                result.indices as usize,
            )
        };
        indices.iter().all(|index| *index < result.vertices)
    } else {
        result.indices == 0
    };
    result.ok = result.retired
        && result.pre_marker == FONT_OUTLINE_MESH_PRE_MARKER
        && result.report_marker == (FONT_OUTLINE_MESH_RESULT_MAGIC_BASE | stage)
        && result.kernel_done
        && layout_ok
        && report[1] & 1 != 0
        && result.op_count == ops.len() as u32
        && result.checksum == expected_checksum
        && result.invalid == 0
        && !result.truncated
        && result.indices_in_range;
    if result.ok && stage == FONT_OUTLINE_STAGE_STROKE_MESH {
        result.generated_mesh = Some(GpgpuFontOutlineMesh {
            storage_phys: state.canvas3d_out_phys,
            storage_bytes: CANVAS3D_PROJECT_OUT_ALLOC_BYTES,
            vertex_offset_bytes: FONT_OUTLINE_MESH_VERTEX_DWORD_OFFSET * 4,
            vertex_count: result.vertices,
            vertex_stride: 2 * core::mem::size_of::<f32>() as u32,
            index_offset_bytes: FONT_OUTLINE_MESH_INDEX_DWORD_OFFSET * 4,
            index_count: result.indices,
            min_x: result.min_x,
            min_y: result.min_y,
            max_x: result.max_x,
            max_y: result.max_y,
        });
    }

    let level_ok = result.ok;
    let message = alloc::format!(
        "intel/gpgpu: font-outline stage={} ok={} retired={} kernel_done={} ops={} counts=[{},{},{},{},{}] vertices={} segments={} indices={} checksum=0x{:08X}/0x{:08X} invalid={} truncated={} index_range={} markers=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] bounds=[{:.2},{:.2}..{:.2},{:.2}] retire_ms={} residency=probe-scratch fill_tessellation=0",
        stage,
        result.ok as u8,
        result.retired as u8,
        result.kernel_done as u8,
        result.op_count,
        result.move_count,
        result.line_count,
        result.quad_count,
        result.cubic_count,
        result.close_count,
        result.vertices,
        result.segments,
        result.indices,
        result.checksum,
        result.expected_checksum,
        result.invalid,
        result.truncated as u8,
        result.indices_in_range as u8,
        result.pre_marker,
        result.post_marker,
        result.report_marker,
        result.done_marker,
        result.min_x,
        result.min_y,
        result.max_x,
        result.max_y,
        result.retire_ms,
    );
    if level_ok {
        crate::log_info!(target: "gpgpu"; "{}\n", message.as_str());
    } else {
        crate::log_error!(target: "gpgpu"; "{}\n", message.as_str());
    }
    result
}

fn submit_sprite_quad_worklist(
    src: GpgpuRgba8Surface,
    dst: GpgpuRgba8Surface,
    desc: GpgpuRectWorklistDescBuffer,
    params: SpriteQuadWorklistRgba8Params,
) -> bool {
    submit_known_descriptor_worklist_sprite_quad(src, dst, desc, params)
}

fn submit_known_descriptor_worklist_sprite_quad(
    src: GpgpuRgba8Surface,
    dst: GpgpuRgba8Surface,
    desc: GpgpuRectWorklistDescBuffer,
    params: SpriteQuadWorklistRgba8Params,
) -> bool {
    if params.desc_count == 0 || params.desc_count as usize > SPRITE_QUAD_WORKLIST_MAX_DESCS {
        return false;
    }
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        return false;
    };
    let Some(upload) = upload_sprite_quad_worklist_rgba8_kernel() else {
        return false;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        return false;
    };

    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ppgtt_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let src_ppgtt_ok =
        kernel_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, src.gpu, src.phys, src.bytes);
    let dst_ppgtt_ok =
        src_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, dst.gpu, dst.phys, dst.bytes);
    let desc_ppgtt_ok =
        dst_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, desc.gpu, desc.phys, desc.bytes);
    let batch_ok = desc_ppgtt_ok
        && direct_rcs_encode_sprite_quad_worklist_batch(
            state, upload, params, src.bytes, dst.bytes, desc.bytes,
        );
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result_slot(
            state,
            SPRITE_QUAD_WORKLIST_POST_MARKER_SLOT,
            SPRITE_QUAD_WORKLIST_POST_MARKER,
        )
    } else {
        0
    };
    if observed != SPRITE_QUAD_WORKLIST_POST_MARKER {
        let fail_count = SPRITE_QUAD_WORKLIST_SUBMIT_FAIL_LOGS.fetch_add(1, Ordering::Relaxed) + 1;
        if fail_count <= 16 || fail_count.is_power_of_two() {
            crate::log!(
                "intel/gpgpu: sprite-quad-worklist submit failed count={} forcewake={} mapped={} ppgtt={} kernel={} src={} dst={} desc={} batch={} submitted={} observed=0x{:X} want=0x{:X} ppgtt_limit=0x{:X} upload_gpu=0x{:X} src_gpu=0x{:X} src_end=0x{:X} dst_gpu=0x{:X} dst_end=0x{:X} dst_bytes=0x{:X} desc_gpu=0x{:X} desc_end=0x{:X} desc_count={}\n",
                fail_count,
                forcewake_ok as u8,
                mapped_ok as u8,
                ppgtt_ok as u8,
                kernel_ppgtt_ok as u8,
                src_ppgtt_ok as u8,
                dst_ppgtt_ok as u8,
                desc_ppgtt_ok as u8,
                batch_ok as u8,
                submitted as u8,
                observed,
                SPRITE_QUAD_WORKLIST_POST_MARKER,
                direct_rcs_ppgtt_limit_bytes(),
                upload.gpu,
                src.gpu,
                src.gpu.saturating_add(src.bytes as u64),
                dst.gpu,
                dst.gpu.saturating_add(dst.bytes as u64),
                dst.bytes,
                desc.gpu,
                desc.gpu.saturating_add(desc.bytes as u64),
                params.desc_count
            );
        }
    }
    observed == SPRITE_QUAD_WORKLIST_POST_MARKER
}

fn submit_sprite_quad_worklist_runs(
    dst: GpgpuRgba8Surface,
    desc: GpgpuRectWorklistDescBuffer,
    runs: &[GpgpuSpriteQuadWorklistRun<'_>],
) -> GpgpuSubmissionOutcome {
    if runs.is_empty() {
        return GpgpuSubmissionOutcome::Unavailable;
    }
    let total_descs = runs
        .iter()
        .try_fold(0usize, |total, run| total.checked_add(run.descs.len()));
    let Some(total_descs) = total_descs else {
        return GpgpuSubmissionOutcome::Unavailable;
    };
    if total_descs == 0 || total_descs > SPRITE_QUAD_WORKLIST_MAX_DESCS {
        return GpgpuSubmissionOutcome::Unavailable;
    }
    if runs.iter().any(|run| run.descs.is_empty()) {
        return GpgpuSubmissionOutcome::Unavailable;
    }

    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        return GpgpuSubmissionOutcome::Unavailable;
    };
    let Some(upload) = upload_sprite_quad_worklist_rgba8_kernel() else {
        return GpgpuSubmissionOutcome::Unavailable;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        return GpgpuSubmissionOutcome::Unavailable;
    };

    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ppgtt_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let dst_ppgtt_ok =
        kernel_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, dst.gpu, dst.phys, dst.bytes);
    let desc_ppgtt_ok =
        dst_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, desc.gpu, desc.phys, desc.bytes);
    let mut src_ppgtt_ok = desc_ppgtt_ok;
    if src_ppgtt_ok {
        for run in runs {
            if !direct_rcs_map_ppgtt_kernel(state, run.src.gpu, run.src.phys, run.src.bytes) {
                src_ppgtt_ok = false;
                break;
            }
        }
    }
    let batch_ok = src_ppgtt_ok
        && direct_rcs_encode_sprite_quad_worklist_runs_batch(state, upload, dst, desc, runs);
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result_slot(
            state,
            SPRITE_QUAD_WORKLIST_POST_MARKER_SLOT,
            SPRITE_QUAD_WORKLIST_POST_MARKER,
        )
    } else {
        0
    };
    if observed != SPRITE_QUAD_WORKLIST_POST_MARKER {
        let fail_count = SPRITE_QUAD_WORKLIST_SUBMIT_FAIL_LOGS.fetch_add(1, Ordering::Relaxed) + 1;
        if fail_count <= 16 || fail_count.is_power_of_two() {
            crate::log!(
                "intel/gpgpu: sprite-quad-worklist-runs submit failed count={} forcewake={} mapped={} ppgtt={} kernel={} dst={} desc={} src={} batch={} submitted={} observed=0x{:X} want=0x{:X} runs={} descs={} ppgtt_limit=0x{:X} upload_gpu=0x{:X} dst_gpu=0x{:X} dst_end=0x{:X} desc_gpu=0x{:X} desc_end=0x{:X}\n",
                fail_count,
                forcewake_ok as u8,
                mapped_ok as u8,
                ppgtt_ok as u8,
                kernel_ppgtt_ok as u8,
                dst_ppgtt_ok as u8,
                desc_ppgtt_ok as u8,
                src_ppgtt_ok as u8,
                batch_ok as u8,
                submitted as u8,
                observed,
                SPRITE_QUAD_WORKLIST_POST_MARKER,
                runs.len(),
                total_descs,
                direct_rcs_ppgtt_limit_bytes(),
                upload.gpu,
                dst.gpu,
                dst.gpu.saturating_add(dst.bytes as u64),
                desc.gpu,
                desc.gpu.saturating_add(desc.bytes as u64)
            );
        }
    }
    if observed == SPRITE_QUAD_WORKLIST_POST_MARKER {
        GpgpuSubmissionOutcome::Complete
    } else if submitted {
        GpgpuSubmissionOutcome::SubmittedIncomplete
    } else {
        GpgpuSubmissionOutcome::Unavailable
    }
}

fn submit_fill_rect_worklist(
    dst: GpgpuRgba8Surface,
    desc: GpgpuRectWorklistDescBuffer,
    params: FillRectWorklistRgba8Params,
    direct_scanout: bool,
) -> bool {
    if params.desc_count == 0 || params.desc_count as usize > RECT_WORKLIST_MAX_DESCS {
        return false;
    }
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        return false;
    };
    let Some(upload) = upload_fill_rect_worklist_rgba8_kernel() else {
        return false;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        return false;
    };

    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ppgtt_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let dst_ppgtt_ok = kernel_ppgtt_ok
        && direct_rcs_map_ppgtt_destination(state, dst.gpu, dst.phys, dst.bytes, direct_scanout);
    let desc_ppgtt_ok =
        dst_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, desc.gpu, desc.phys, desc.bytes);
    let batch_ok = desc_ppgtt_ok
        && direct_rcs_encode_fill_rect_worklist_batch(state, upload, params, dst.bytes, desc.bytes);
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result_slot(
            state,
            RECT_WORKLIST_POST_MARKER_SLOT,
            FILL_RECT_WORKLIST_POST_MARKER,
        )
    } else {
        0
    };
    observed == FILL_RECT_WORKLIST_POST_MARKER
}

fn submit_mandel64_worklist(
    dst: GpgpuRgba8Surface,
    desc: GpgpuRectWorklistDescBuffer,
    params: Mandel64WorklistRgba8Params,
    direct_scanout: bool,
) -> DirectRcsDispatchOutcome {
    if params.desc_count == 0 || params.desc_count as usize > MANDEL64_WORKLIST_MAX_DESCS {
        return DirectRcsDispatchOutcome::default();
    }
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        return DirectRcsDispatchOutcome::default();
    };
    let Some(upload) = upload_mandel64_worklist_rgba8_kernel() else {
        return DirectRcsDispatchOutcome::default();
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        return DirectRcsDispatchOutcome::default();
    };

    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ppgtt_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let dst_ppgtt_ok = kernel_ppgtt_ok
        && if direct_scanout {
            direct_rcs_map_ppgtt_scanout(state, dst.gpu, dst.phys, dst.bytes)
        } else {
            direct_rcs_map_ppgtt_kernel(state, dst.gpu, dst.phys, dst.bytes)
        };
    let desc_ppgtt_ok =
        dst_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, desc.gpu, desc.phys, desc.bytes);
    let batch_ok = desc_ppgtt_ok
        && direct_rcs_encode_mandel64_worklist_batch(state, upload, params, dst.bytes, desc.bytes);
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted && direct_scanout {
        direct_rcs_poll_result_slot_timeout_ms(
            state,
            RECT_WORKLIST_POST_MARKER_SLOT,
            MANDEL64_WORKLIST_POST_MARKER,
            UI4_COMPUTE_PRODUCER_RETIRE_TIMEOUT_MS,
        )
    } else if submitted {
        direct_rcs_poll_result_slot(
            state,
            RECT_WORKLIST_POST_MARKER_SLOT,
            MANDEL64_WORKLIST_POST_MARKER,
        )
    } else {
        0
    };
    if observed != MANDEL64_WORKLIST_POST_MARKER {
        if submitted && direct_scanout {
            quarantine_direct_rcs_context("mandel64-worklist-marker-timeout");
        }
        crate::log_error!(
            target: "gpgpu";
            "intel/gpgpu: mandel64-worklist failed direct_scanout={} mapped={} ppgtt={} kernel={} dst={} desc={} batch={} submitted={} observed=0x{:08X} want=0x{:08X} descs={} dst_gpu=0x{:X}\n",
            direct_scanout as u8,
            mapped_ok as u8,
            ppgtt_ok as u8,
            kernel_ppgtt_ok as u8,
            dst_ppgtt_ok as u8,
            desc_ppgtt_ok as u8,
            batch_ok as u8,
            submitted as u8,
            observed,
            MANDEL64_WORKLIST_POST_MARKER,
            params.desc_count,
            dst.gpu,
        );
    }
    DirectRcsDispatchOutcome {
        submitted,
        observed,
    }
}

fn sprite_quad_worklist_walker_count(desc_count: usize) -> usize {
    desc_count
        .div_ceil(SPRITE_QUAD_WORKLIST_DESCS_PER_WALKER)
        .min(SPRITE_QUAD_WORKLIST_MAX_WALKERS)
}

fn rect_worklist_walker_count(desc_count: usize) -> usize {
    desc_count
        .div_ceil(RECT_WORKLIST_DESCS_PER_WALKER)
        .min(RECT_WORKLIST_MAX_WALKERS)
}

fn mandel64_worklist_walker_count(desc_count: usize) -> usize {
    desc_count
        .div_ceil(MANDEL64_WORKLIST_DESCS_PER_WALKER)
        .min(MANDEL64_WORKLIST_MAX_WALKERS)
}

fn simd16_right_mask(lanes: u32) -> u32 {
    if lanes >= 16 {
        GPGPU_WALKER_SIMD16_MASK
    } else if lanes == 0 {
        0
    } else {
        (1u32 << lanes) - 1
    }
}

fn upload_artifact(
    dev: super::Dev,
    artifact: GpgpuKernelArtifact,
    gpu: u64,
) -> Option<UploadedKernelArtifact> {
    upload_artifact_from_sources(dev, artifact, gpu, false)
}

fn upload_artifact_from_sources(
    dev: super::Dev,
    artifact: GpgpuKernelArtifact,
    gpu: u64,
    strict_runtime_artifact: bool,
) -> Option<UploadedKernelArtifact> {
    // `kfs::read_file` is a synchronous wrapper around a future queued on the
    // current executor. Calling it from an Embassy task cannot make progress:
    // the executor re-entry guard rejects the recursive poll. UI4 reaches
    // first-use uploads from its compositor and producer tasks, so those paths
    // must use the build-embedded artifact instead of freezing the whole UI
    // core. Runtime-artifact overrides remain available to callers outside an
    // executor poll; a strict reload attempted inside one is rejected instead
    // of deadlocking and must eventually be exposed through an async loader.
    if !crate::percpu::in_executor_poll() {
        match read_runtime_artifact_bytes(artifact.name) {
            Ok(Some(bytes)) if !bytes.is_empty() => {
                let path = runtime_artifact_display_path(artifact.name);
                let spv_bytes = read_runtime_spv_len(artifact.name).unwrap_or(artifact.spv.len());
                return upload_artifact_bytes(
                    dev,
                    artifact,
                    gpu,
                    bytes.as_slice(),
                    "fs",
                    path.as_str(),
                    spv_bytes,
                );
            }
            Ok(Some(_)) => {
                crate::log_info!(
                    target: "gpgpu";
                    "intel/gpgpu: {} runtime artifact rejected reason=empty path={}\n",
                    artifact.name,
                    runtime_artifact_display_path(artifact.name)
                );
                if strict_runtime_artifact {
                    return None;
                }
            }
            Ok(None) => {}
            Err(err) => {
                crate::log_info!(
                    target: "gpgpu";
                    "intel/gpgpu: {} runtime artifact read failed path={} err={:?}\n",
                    artifact.name,
                    runtime_artifact_display_path(artifact.name),
                    err
                );
                if strict_runtime_artifact {
                    return None;
                }
            }
        }
    } else if strict_runtime_artifact {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: {} runtime artifact reload rejected reason=executor-context-would-deadlock path={}\n",
            artifact.name,
            runtime_artifact_display_path(artifact.name),
        );
        return None;
    } else {
        static EXECUTOR_EMBEDDED_FALLBACK_LOGGED: AtomicBool = AtomicBool::new(false);
        if !EXECUTOR_EMBEDDED_FALLBACK_LOGGED.swap(true, Ordering::AcqRel) {
            crate::log_info!(
                target: "gpgpu";
                "intel/gpgpu: runtime artifact lookup bypassed kernel={} reason=executor-context-would-deadlock fallback=embedded\n",
                artifact.name,
            );
        }
    }

    let source_path = kernel_source_path(artifact.name).unwrap_or("embedded");
    upload_artifact_bytes(
        dev,
        artifact,
        gpu,
        artifact.bin,
        "embedded",
        source_path,
        artifact.spv.len(),
    )
}

fn upload_artifact_bytes(
    dev: super::Dev,
    artifact: GpgpuKernelArtifact,
    gpu: u64,
    bin: &[u8],
    source: &'static str,
    source_path: &str,
    spv_bytes: usize,
) -> Option<UploadedKernelArtifact> {
    if bin.is_empty() {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: {} upload failed reason=empty source={} path={}\n",
            artifact.name,
            source,
            source_path
        );
        return None;
    }
    if let Err(reason) = validate_kernel_artifact_bytes(bin) {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: {} upload failed reason={} source={} path={} bytes=0x{:X}\n",
            artifact.name,
            reason,
            source,
            source_path,
            bin.len()
        );
        return None;
    }
    let actual_sha256 = sha256_digest(bin);
    let requires_allowlisted_sha = matches!(
        artifact.name,
        ALPHA_BLEND_RGBA8_OVER_KERNEL_NAME
            | CHART_SINE_RGBA8_KERNEL_NAME
            | PIXEL_PLASMA_RGBA8_KERNEL_NAME
            | FONT_OUTLINE_MESH_KERNEL_NAME
            | FONT_OUTLINE_COVERAGE_R8_KERNEL_NAME
    );
    if requires_allowlisted_sha && actual_sha256 != artifact.bin_sha256 {
        crate::log_error!(
            target: "gpgpu";
            "intel/gpgpu: {} upload rejected reason=sha256-not-allowlisted source={} path={} expected={} actual={}\n",
            artifact.name,
            source,
            source_path,
            digest_hex(&artifact.bin_sha256).as_str(),
            digest_hex(&actual_sha256).as_str()
        );
        return None;
    }

    let mapped_bytes = align_up(bin.len(), super::WARM_ALIGN)?;
    let (phys, virt) = crate::dma::alloc(mapped_bytes, super::WARM_ALIGN)?;
    unsafe {
        core::ptr::write_bytes(virt, 0, mapped_bytes);
        core::ptr::copy_nonoverlapping(bin.as_ptr(), virt, bin.len());
    }
    super::dma_flush(virt, mapped_bytes);

    let uploaded = unsafe { core::slice::from_raw_parts(virt, bin.len()) };
    let verified = uploaded == bin;
    if !verified {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: {} upload failed reason=verify source={} path={} phys=0x{:X} gpu=0x{:X} bytes=0x{:X}\n",
            artifact.name,
            source,
            source_path,
            phys,
            gpu,
            bin.len()
        );
        crate::dma::dealloc(virt, mapped_bytes);
        return None;
    }

    if !super::map_ggtt(dev, phys, mapped_bytes, gpu) {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: {} upload failed reason=ggtt-map source={} path={} phys=0x{:X} gpu=0x{:X} bytes=0x{:X}\n",
            artifact.name,
            source,
            source_path,
            phys,
            gpu,
            mapped_bytes
        );
        crate::dma::dealloc(virt, mapped_bytes);
        return None;
    }
    super::ggtt_invalidate(dev);

    let upload = UploadedKernelArtifact {
        name: artifact.name,
        target: artifact.target,
        source,
        gpu,
        phys,
        bytes: bin.len(),
        mapped_bytes,
        verified,
        bin_sha256: actual_sha256,
    };
    let source_bytes = kernel_opencl_source(artifact.name)
        .map(|source| source.len())
        .unwrap_or(0);
    let sha256 = digest_hex(&upload.bin_sha256);
    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: {} upload ok=1 target={} source={} path={} source_bytes=0x{:X} spv_bytes=0x{:X} phys=0x{:X} gpu=0x{:X} bytes=0x{:X} mapped=0x{:X} sha256={}\n",
        artifact.name,
        upload.target,
        upload.source,
        source_path,
        source_bytes,
        spv_bytes,
        upload.phys,
        upload.gpu,
        upload.bytes,
        upload.mapped_bytes,
        sha256.as_str(),
    );
    Some(upload)
}

fn runtime_artifact_rel_path(name: &str, ext: &str) -> String {
    alloc::format!("gpgpu/adls/{name}.{ext}")
}

fn runtime_artifact_display_path(name: &str) -> String {
    alloc::format!("/{}", runtime_artifact_rel_path(name, "bin"))
}

fn read_runtime_artifact_bytes(name: &str) -> Result<Option<Vec<u8>>, crate::io::kfs::FsError> {
    match crate::io::kfs::read_file(runtime_artifact_rel_path(name, "bin").as_str()) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(crate::io::kfs::FsError::NoRoot | crate::io::kfs::FsError::NotFound) => Ok(None),
        Err(err) => Err(err),
    }
}

fn read_runtime_spv_len(name: &str) -> Option<usize> {
    match crate::io::kfs::read_file_len(runtime_artifact_rel_path(name, "spv").as_str()) {
        Ok(len) => Some(len),
        Err(_) => None,
    }
}

fn validate_kernel_artifact_bytes(bytes: &[u8]) -> Result<(), &'static str> {
    const ELF64_HEADER_BYTES: usize = 64;
    const ELF_MACHINE_INTEL_GT: u16 = 0x00CD;
    if bytes.len() < ELF64_HEADER_BYTES {
        return Err("truncated-elf");
    }
    if &bytes[0..4] != b"\x7FELF" {
        return Err("not-elf");
    }
    if bytes[4] != 2 {
        return Err("not-elf64");
    }
    if bytes[5] != 1 {
        return Err("not-little-endian");
    }
    let machine = u16::from_le_bytes([bytes[18], bytes[19]]);
    if machine != ELF_MACHINE_INTEL_GT {
        return Err("wrong-machine");
    }
    Ok(())
}

fn sha256_digest(bytes: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

fn digest_hex(digest: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for byte in digest {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[derive(Copy, Clone, Debug)]
struct DirectRcsState {
    ring_phys: u64,
    ring_virt: *mut u8,
    context_phys: u64,
    context_virt: *mut u8,
    batch_phys: u64,
    batch_virt: *mut u8,
    result_phys: u64,
    result_virt: *mut u8,
    clear_test_phys: u64,
    clear_test_virt: *mut u8,
    canvas3d_out_phys: u64,
    canvas3d_out_virt: *mut u8,
    canvas3d_tmp_phys: u64,
    ppgtt_phys: u64,
    ppgtt_virt: *mut u8,
    gpu_va: DirectRcsGpuVa,
}

#[derive(Copy, Clone, Debug)]
struct DirectRcsGpuVa {
    ring: u64,
    context: u64,
    batch: u64,
    result: u64,
    map_general_auxiliary: bool,
}

const DIRECT_RCS_GPU_VA: DirectRcsGpuVa = DirectRcsGpuVa {
    ring: DIRECT_RCS_GPU_VA_RING_BASE,
    context: DIRECT_RCS_GPU_VA_CONTEXT_BASE,
    batch: DIRECT_RCS_GPU_VA_BATCH_BASE,
    result: DIRECT_RCS_GPU_VA_RESULT_BASE,
    map_general_auxiliary: true,
};

const UI4_COMPOSITOR_RCS_GPU_VA: DirectRcsGpuVa = DirectRcsGpuVa {
    ring: UI4_COMPOSITOR_RCS_GPU_VA_RING_BASE,
    context: UI4_COMPOSITOR_RCS_GPU_VA_CONTEXT_BASE,
    batch: UI4_COMPOSITOR_RCS_GPU_VA_BATCH_BASE,
    result: UI4_COMPOSITOR_RCS_GPU_VA_RESULT_BASE,
    map_general_auxiliary: false,
};

#[derive(Copy, Clone, Debug)]
struct DirectRcsSubmitRuntime {
    context_initialized: bool,
    ring_tail_bytes: usize,
    pending: Option<crate::gpu::executor::KernelSubmission>,
}

impl DirectRcsSubmitRuntime {
    const fn new() -> Self {
        Self {
            context_initialized: false,
            ring_tail_bytes: 0,
            pending: None,
        }
    }
}

#[derive(Copy, Clone, Debug)]
struct Ui4CompositorPending {
    submission: Ui4CompositorSubmission,
    started_tick: u64,
    marker_slot: usize,
    marker_value: u32,
    kernel: &'static str,
    stats: GpgpuWorklistSubmitStats,
    overdue_logged: bool,
}

#[derive(Copy, Clone, Debug)]
struct Ui4CompositorRuntime {
    submit: DirectRcsSubmitRuntime,
    next_serial: u64,
    pending: Option<Ui4CompositorPending>,
    last_completion: Option<(Ui4CompositorSubmission, Ui4CompositorCompletion)>,
    state_mapped: bool,
    ppgtt_initialized: bool,
}

impl Ui4CompositorRuntime {
    const fn new() -> Self {
        Self {
            submit: DirectRcsSubmitRuntime::new(),
            next_serial: 0,
            pending: None,
            last_completion: None,
            state_mapped: false,
            ppgtt_initialized: false,
        }
    }
}

unsafe impl Send for DirectRcsState {}
unsafe impl Sync for DirectRcsState {}

fn direct_rcs_state_once(_dev: super::Dev) -> Option<DirectRcsState> {
    if let Some(state) = *DIRECT_RCS_STATE.lock() {
        return Some(state);
    }

    let state = allocate_direct_rcs_state(DIRECT_RCS_GPU_VA)?;
    *DIRECT_RCS_STATE.lock() = Some(state);
    Some(state)
}

fn ui4_compositor_rcs_state_once(_dev: super::Dev) -> Option<DirectRcsState> {
    if let Some(state) = *UI4_COMPOSITOR_RCS_STATE.lock() {
        return Some(state);
    }

    let state = allocate_direct_rcs_state(UI4_COMPOSITOR_RCS_GPU_VA)?;
    *UI4_COMPOSITOR_RCS_STATE.lock() = Some(state);
    Some(state)
}

fn allocate_direct_rcs_state(gpu_va: DirectRcsGpuVa) -> Option<DirectRcsState> {
    let (ring_phys, ring_virt) = crate::dma::alloc(DIRECT_RCS_RING_BYTES, super::WARM_ALIGN)?;
    let (context_phys, context_virt) =
        crate::dma::alloc(DIRECT_RCS_CONTEXT_BYTES, super::WARM_ALIGN)?;
    let (batch_phys, batch_virt) = crate::dma::alloc(DIRECT_RCS_BATCH_BYTES, super::WARM_ALIGN)?;
    let (result_phys, result_virt) = crate::dma::alloc(DIRECT_RCS_RESULT_BYTES, super::WARM_ALIGN)?;
    let (clear_test_phys, clear_test_virt) =
        crate::dma::alloc(CLEAR_RECT_TEST_BYTES, super::WARM_ALIGN)?;
    let (canvas3d_out_phys, canvas3d_out_virt) =
        crate::dma::alloc(CANVAS3D_PROJECT_OUT_ALLOC_BYTES, super::WARM_ALIGN)?;
    let (canvas3d_tmp_phys, canvas3d_tmp_virt) =
        crate::dma::alloc(CANVAS3D_PROJECT_OUT_ALLOC_BYTES, super::WARM_ALIGN)?;
    let (ppgtt_phys, ppgtt_virt) = crate::dma::alloc(DIRECT_RCS_PPGTT_BYTES, super::WARM_ALIGN)?;

    unsafe {
        core::ptr::write_bytes(ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(context_virt, 0, DIRECT_RCS_CONTEXT_BYTES);
        core::ptr::write_bytes(batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(result_virt, 0, DIRECT_RCS_RESULT_BYTES);
        core::ptr::write_bytes(clear_test_virt, 0, CLEAR_RECT_TEST_BYTES);
        core::ptr::write_bytes(canvas3d_out_virt, 0, CANVAS3D_PROJECT_OUT_ALLOC_BYTES);
        core::ptr::write_bytes(canvas3d_tmp_virt, 0, CANVAS3D_PROJECT_OUT_ALLOC_BYTES);
        core::ptr::write_bytes(ppgtt_virt, 0, DIRECT_RCS_PPGTT_BYTES);
    }

    let state = DirectRcsState {
        ring_phys,
        ring_virt,
        context_phys,
        context_virt,
        batch_phys,
        batch_virt,
        result_phys,
        result_virt,
        clear_test_phys,
        clear_test_virt,
        canvas3d_out_phys,
        canvas3d_out_virt,
        canvas3d_tmp_phys,
        ppgtt_phys,
        ppgtt_virt,
        gpu_va,
    };
    Some(state)
}

fn direct_rcs_map_state(dev: super::Dev, state: DirectRcsState) -> bool {
    let core_mapped =
        super::map_ggtt(dev, state.ring_phys, DIRECT_RCS_RING_BYTES, state.gpu_va.ring)
            && super::map_ggtt(
                dev,
                state.context_phys,
                DIRECT_RCS_CONTEXT_BYTES,
                state.gpu_va.context,
            )
            && super::map_ggtt(dev, state.batch_phys, DIRECT_RCS_BATCH_BYTES, state.gpu_va.batch)
            && super::map_ggtt(
                dev,
                state.result_phys,
                DIRECT_RCS_RESULT_BYTES,
                state.gpu_va.result,
            );
    let auxiliary_mapped = !state.gpu_va.map_general_auxiliary
        || (super::map_ggtt(
            dev,
            state.clear_test_phys,
            CLEAR_RECT_TEST_BYTES,
            DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE,
        ) && super::map_ggtt(
            dev,
            state.canvas3d_out_phys,
            CANVAS3D_PROJECT_OUT_ALLOC_BYTES,
            DIRECT_RCS_GPU_VA_CANVAS3D_OUT_BASE,
        ) && super::map_ggtt(
            dev,
            state.canvas3d_tmp_phys,
            CANVAS3D_PROJECT_OUT_ALLOC_BYTES,
            DIRECT_RCS_GPU_VA_CANVAS3D_TMP_BASE,
        ));
    let mapped = core_mapped && auxiliary_mapped;
    if mapped {
        super::ggtt_invalidate(dev);
    }
    mapped
}

fn direct_rcs_init_ppgtt(state: DirectRcsState) -> bool {
    let pml4_off = 0usize;
    let pdp_off = 4096usize;
    let pd_off = 8192usize;
    let pt_off = 12288usize;
    let pte_present_rw = super::GEN8_PAGE_PRESENT | GEN8_PAGE_RW;
    let pde_present_rw_uc = pte_present_rw | GEN8_PAGE_PWT | GEN8_PAGE_PCD;

    unsafe {
        core::ptr::write_bytes(state.ppgtt_virt, 0, DIRECT_RCS_PPGTT_BYTES);
        let pml4 = state.ppgtt_virt.add(pml4_off) as *mut u64;
        let pdp = state.ppgtt_virt.add(pdp_off) as *mut u64;
        let pd = state.ppgtt_virt.add(pd_off) as *mut u64;
        core::ptr::write_volatile(pml4, (state.ppgtt_phys + pdp_off as u64) | pde_present_rw_uc);
        core::ptr::write_volatile(pdp, (state.ppgtt_phys + pd_off as u64) | pde_present_rw_uc);
        for index in 0..DIRECT_RCS_PPGTT_PT_COUNT {
            let pt_phys = state.ppgtt_phys + pt_off as u64 + (index as u64) * 4096;
            core::ptr::write_volatile(pd.add(index), pt_phys | pde_present_rw_uc);
        }
    }

    let ok = direct_rcs_map_ppgtt_region(
        state,
        state.gpu_va.ring,
        state.ring_phys,
        DIRECT_RCS_RING_BYTES,
        pte_present_rw,
    ) && direct_rcs_map_ppgtt_region(
        state,
        state.gpu_va.context,
        state.context_phys,
        DIRECT_RCS_CONTEXT_BYTES,
        pte_present_rw,
    ) && direct_rcs_map_ppgtt_region(
        state,
        state.gpu_va.batch,
        state.batch_phys,
        DIRECT_RCS_BATCH_BYTES,
        pte_present_rw,
    ) && direct_rcs_map_ppgtt_region(
        state,
        state.gpu_va.result,
        state.result_phys,
        DIRECT_RCS_RESULT_BYTES,
        pte_present_rw,
    );

    super::dma_flush(state.ppgtt_virt, DIRECT_RCS_PPGTT_BYTES);
    ok
}

fn direct_rcs_map_ppgtt_kernel(state: DirectRcsState, gpu: u64, phys: u64, len: usize) -> bool {
    let ok = direct_rcs_map_ppgtt_region(state, gpu, phys, len, direct_rcs_ppgtt_pte_flags());
    ok && direct_rcs_flush_ppgtt_pte_range(state, gpu, len)
}

fn direct_rcs_map_ppgtt_destination(
    state: DirectRcsState,
    gpu: u64,
    phys: u64,
    len: usize,
    direct_scanout: bool,
) -> bool {
    if direct_scanout {
        direct_rcs_map_ppgtt_scanout(state, gpu, phys, len)
    } else {
        direct_rcs_map_ppgtt_kernel(state, gpu, phys, len)
    }
}

/// Map a full-surface compute destination that will transfer directly to the
/// display engine. PAT3/UC is the same producer-side cache contract used by
/// Draw3D direct targets; ordinary kernels and resources remain PAT0/WB.
fn direct_rcs_map_ppgtt_scanout(state: DirectRcsState, gpu: u64, phys: u64, len: usize) -> bool {
    if !super::gen12_integrated_pat_ready() {
        return false;
    }
    let pte_present_rw_pat3_uc = direct_rcs_ppgtt_pte_flags() | GEN8_PAGE_PWT | GEN8_PAGE_PCD;
    let ok = direct_rcs_map_ppgtt_region(state, gpu, phys, len, pte_present_rw_pat3_uc)
        && direct_rcs_flush_ppgtt_pte_range(state, gpu, len);
    if ok && !DIRECT_RCS_SCANOUT_PPGTT_LOGGED.swap(true, Ordering::AcqRel) {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: direct-rgba8 scanout target mapped gpu=0x{:X} phys=0x{:X} bytes=0x{:X} ppgtt_pat=3 ppgtt_cache=uc ordinary_resources=pat0-wb\n",
            gpu,
            phys,
            len,
        );
    }
    ok
}

/// Publish only the PTEs changed by one mapping. The PML4/PDP/PD topology is
/// initialized and flushed once for the persistent context; flushing the full
/// PPGTT allocation for every source and destination remap made UI4 submit
/// preparation scale with page-table capacity instead of with changed PTEs.
fn direct_rcs_flush_ppgtt_pte_range(state: DirectRcsState, gpu: u64, len: usize) -> bool {
    if len == 0 || gpu & 0xFFF != 0 {
        return false;
    }
    let pages = len.div_ceil(4096);
    let va_page = gpu >> 12;
    let pd_index = (va_page >> 9) as usize;
    let pt_index = (va_page & 0x1FF) as usize;
    if pd_index >= DIRECT_RCS_PPGTT_PT_COUNT {
        return false;
    }
    let pt_off = 12288usize;
    let Some(start) = pt_off
        .checked_add(pd_index.saturating_mul(4096))
        .and_then(|offset| {
            offset.checked_add(pt_index.saturating_mul(core::mem::size_of::<u64>()))
        })
    else {
        return false;
    };
    let Some(bytes) = pages.checked_mul(core::mem::size_of::<u64>()) else {
        return false;
    };
    let Some(end) = start.checked_add(bytes) else {
        return false;
    };
    if end > DIRECT_RCS_PPGTT_BYTES {
        return false;
    }
    super::dma_flush(unsafe { state.ppgtt_virt.add(start) }, bytes);
    true
}

fn direct_rcs_ppgtt_pte_flags() -> u64 {
    super::GEN8_PAGE_PRESENT | GEN8_PAGE_RW
}

fn direct_rcs_ppgtt_limit_bytes() -> u64 {
    DIRECT_RCS_PPGTT_LIMIT_BYTES
}

fn direct_rcs_map_ppgtt_region(
    state: DirectRcsState,
    gpu: u64,
    phys: u64,
    len: usize,
    entry_flags: u64,
) -> bool {
    let Some(end) = u64::try_from(len).ok().and_then(|len| gpu.checked_add(len)) else {
        return false;
    };
    if end > DIRECT_RCS_PPGTT_LIMIT_BYTES {
        return false;
    }

    let pt_off = 12288usize;
    for page in 0..len.div_ceil(4096) {
        let va_page = (gpu >> 12) + page as u64;
        let pd_index = (va_page >> 9) as usize;
        let pt_index = (va_page & 0x1FF) as usize;
        if pd_index >= DIRECT_RCS_PPGTT_PT_COUNT {
            return false;
        }
        let pte_off = pt_off + pd_index * 4096 + pt_index * core::mem::size_of::<u64>();
        let pte = (phys + (page as u64) * 4096) & !0xFFF;
        unsafe {
            core::ptr::write_volatile(state.ppgtt_virt.add(pte_off) as *mut u64, pte | entry_flags);
        }
    }
    true
}

fn direct_rcs_forcewake(dev: super::Dev) -> bool {
    super::mmio_write(
        dev,
        FORCEWAKE_RENDER,
        super::mask_dis(FORCEWAKE_KERNEL | FORCEWAKE_FALLBACK),
    );
    let _ = direct_rcs_wait_eq(
        dev,
        FORCEWAKE_ACK_RENDER,
        FORCEWAKE_KERNEL | FORCEWAKE_FALLBACK,
        0,
        FORCEWAKE_POLL_ITERS,
    );

    super::mmio_write(dev, FORCEWAKE_RENDER, super::mask_en(FORCEWAKE_KERNEL));
    let render_ok = direct_rcs_wait_eq(
        dev,
        FORCEWAKE_ACK_RENDER,
        FORCEWAKE_KERNEL,
        FORCEWAKE_KERNEL,
        FORCEWAKE_POLL_ITERS,
    );
    super::mmio_write(dev, FORCEWAKE_GT, super::mask_en(FORCEWAKE_KERNEL));
    let gt_ok = direct_rcs_wait_eq(
        dev,
        FORCEWAKE_ACK_GT,
        FORCEWAKE_KERNEL,
        FORCEWAKE_KERNEL,
        FORCEWAKE_POLL_ITERS,
    );
    super::mmio_write(
        dev,
        RCS_CS_DEBUG_MODE1,
        direct_rcs_masked_bit_enable(FF_DOP_CLOCK_GATE_DISABLE),
    );
    render_ok && gt_ok
}

fn direct_rcs_encode_fill_rect_worklist_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: FillRectWorklistRgba8Params,
    dst_bytes: usize,
    desc_bytes: usize,
) -> bool {
    let desc_count = params.desc_count as usize;
    let walker_count = rect_worklist_walker_count(desc_count);
    if desc_count == 0 || walker_count == 0 {
        return false;
    }
    let payload_end =
        RECT_WORKLIST_PAYLOAD_OFFSET_BYTES + walker_count * RECT_WORKLIST_INDIRECT_BYTES;
    if payload_end > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }

    if !direct_rcs_write_fill_rect_worklist_interface_descriptor(state) {
        return false;
    }
    if !direct_rcs_write_fill_rect_worklist_surface_states(
        state,
        params.dst_gpu,
        dst_bytes,
        params.desc_gpu,
        desc_bytes,
    ) {
        return false;
    }
    for walker in 0..walker_count {
        let desc_base = walker.saturating_mul(RECT_WORKLIST_DESCS_PER_WALKER);
        let local_count = desc_count
            .saturating_sub(desc_base)
            .min(RECT_WORKLIST_DESCS_PER_WALKER);
        let payload_offset =
            RECT_WORKLIST_PAYLOAD_OFFSET_BYTES + walker * RECT_WORKLIST_INDIRECT_BYTES;
        let payload_params = FillRectWorklistRgba8Params {
            desc_base: params.desc_base.saturating_add(desc_base as u32),
            desc_count: local_count as u32,
            ..params
        };
        if !direct_rcs_write_fill_rect_worklist_payload_at(state, payload_offset, payload_params) {
            return false;
        }
    }

    direct_rcs_encode_rect_worklist_command_stream(
        state,
        upload,
        walker_count,
        desc_count,
        FILL_RECT_WORKLIST_PRE_MARKER,
        FILL_RECT_WORKLIST_POST_MARKER,
        true,
    )
}

fn direct_rcs_encode_mandel64_worklist_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: Mandel64WorklistRgba8Params,
    dst_bytes: usize,
    desc_bytes: usize,
) -> bool {
    let desc_count = params.desc_count as usize;
    let walker_count = mandel64_worklist_walker_count(desc_count);
    if desc_count == 0 || walker_count == 0 {
        return false;
    }
    let payload_end =
        RECT_WORKLIST_PAYLOAD_OFFSET_BYTES + walker_count * RECT_WORKLIST_INDIRECT_BYTES;
    if payload_end > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }

    if !direct_rcs_write_mandel64_worklist_interface_descriptor(state) {
        return false;
    }
    if !direct_rcs_write_fill_rect_worklist_surface_states(
        state,
        params.dst_gpu,
        dst_bytes,
        params.desc_gpu,
        desc_bytes,
    ) {
        return false;
    }
    for walker in 0..walker_count {
        let desc_base = walker.saturating_mul(RECT_WORKLIST_DESCS_PER_WALKER);
        let local_count = desc_count
            .saturating_sub(desc_base)
            .min(RECT_WORKLIST_DESCS_PER_WALKER);
        let payload_offset =
            RECT_WORKLIST_PAYLOAD_OFFSET_BYTES + walker * RECT_WORKLIST_INDIRECT_BYTES;
        let payload_params = Mandel64WorklistRgba8Params {
            desc_base: params.desc_base.saturating_add(desc_base as u32),
            desc_count: local_count as u32,
            ..params
        };
        if !direct_rcs_write_mandel64_worklist_payload_at(state, payload_offset, payload_params) {
            return false;
        }
    }

    direct_rcs_encode_rect_worklist_command_stream(
        state,
        upload,
        walker_count,
        desc_count,
        MANDEL64_WORKLIST_PRE_MARKER,
        MANDEL64_WORKLIST_POST_MARKER,
        false,
    )
}

fn direct_rcs_encode_ui4_compose_layers_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: Ui4ComposeLayersParams,
    base_bytes: usize,
    dst_bytes: usize,
    desc_bytes: usize,
) -> bool {
    if params.damage_width == 0
        || params.damage_height == 0
        || params.layer_count as usize > UI4_COMPOSE_LAYERS_MAX_LAYERS
        || RECT_WORKLIST_PAYLOAD_OFFSET_BYTES + UI4_COMPOSE_LAYERS_INDIRECT_BYTES
            > DIRECT_RCS_BATCH_BYTES
    {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }
    if !direct_rcs_write_interface_descriptor_at(
        state,
        RECT_WORKLIST_IDD_OFFSET_BYTES,
        RECT_WORKLIST_BINDING_TABLE_OFFSET_BYTES,
        UI4_COMPOSE_LAYERS_RGBA8_TEXT_OFFSET_BYTES,
        3,
        UI4_COMPOSE_LAYERS_CROSS_THREAD_GRFS,
    ) || !direct_rcs_write_alpha_blend_worklist_surface_states_at(
        state,
        RECT_WORKLIST_BINDING_TABLE_OFFSET_BYTES,
        RECT_WORKLIST_SRC_SURFACE_STATE_OFFSET_BYTES,
        RECT_WORKLIST_DST_SURFACE_STATE_OFFSET_BYTES,
        RECT_WORKLIST_DESC_SURFACE_STATE_OFFSET_BYTES,
        params.base_gpu,
        base_bytes,
        params.dst_gpu,
        dst_bytes,
        params.layers_gpu,
        desc_bytes,
    ) || !direct_rcs_write_ui4_compose_layers_payload_at(
        state,
        RECT_WORKLIST_PAYLOAD_OFFSET_BYTES,
        params,
    ) {
        return false;
    }

    let batch_len = DIRECT_RCS_BATCH_BYTES / core::mem::size_of::<u32>();
    let batch = unsafe { core::slice::from_raw_parts_mut(state.batch_virt as *mut u32, batch_len) };
    let mut cursor = 0usize;
    let group_x = params.damage_width.div_ceil(16).max(1);
    let group_y = params.damage_height.max(1);
    let mut ok =
        direct_rcs_push_gpgpu_dispatch_prologue(batch, &mut cursor, upload, state.gpu_va.batch);
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, RECT_WORKLIST_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, RECT_WORKLIST_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_store_marker_at(
        batch,
        &mut cursor,
        state.gpu_va.result,
        SPRITE_QUAD_WORKLIST_PRE_MARKER_SLOT,
        UI4_COMPOSE_LAYERS_PRE_MARKER,
    );
    ok &= direct_rcs_push_gpgpu_walker_2d(
        batch,
        &mut cursor,
        RECT_WORKLIST_PAYLOAD_OFFSET_BYTES,
        UI4_COMPOSE_LAYERS_INDIRECT_BYTES,
        group_x,
        group_y,
        GPGPU_WALKER_SIMD16_MASK,
    );
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push_gpgpu_dispatch_epilogue(
        batch,
        &mut cursor,
        state.gpu_va.result,
        SPRITE_QUAD_WORKLIST_POST_MARKER_SLOT,
        UI4_COMPOSE_LAYERS_POST_MARKER,
    );
    if !ok {
        return false;
    }
    super::dma_flush(state.batch_virt, DIRECT_RCS_BATCH_BYTES);
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    true
}

fn direct_rcs_write_ui4_compose_layers_payload_at(
    state: DirectRcsState,
    payload_offset: usize,
    params: Ui4ComposeLayersParams,
) -> bool {
    if payload_offset + UI4_COMPOSE_LAYERS_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    let Some(known) =
        super::opencl::registry::known_aot_kernel(UI4_COMPOSE_LAYERS_RGBA8_KERNEL_NAME)
    else {
        return false;
    };
    unsafe {
        let payload = state.batch_virt.add(payload_offset);
        core::ptr::write_bytes(payload, 0, UI4_COMPOSE_LAYERS_INDIRECT_BYTES);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        core::ptr::write_volatile(dwords.add(12), params.base_gpu as u32);
        core::ptr::write_volatile(dwords.add(13), (params.base_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(14), params.dst_gpu as u32);
        core::ptr::write_volatile(dwords.add(15), (params.dst_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(16), params.layers_gpu as u32);
        core::ptr::write_volatile(dwords.add(17), (params.layers_gpu >> 32) as u32);

        let cross_thread =
            core::slice::from_raw_parts_mut(payload, UI4_COMPOSE_LAYERS_CROSS_THREAD_BYTES);
        let values = (|| {
            let mut writer = super::opencl::KernelValueWriter::new(known.contract, cross_thread)?;
            writer.set_u32(3, params.base_pitch_bytes)?;
            writer.set_u32(4, params.dst_pitch_bytes)?;
            writer.set_u32(5, params.dst_width)?;
            writer.set_u32(6, params.dst_height)?;
            writer.set_u32(7, params.damage_x)?;
            writer.set_u32(8, params.damage_y)?;
            writer.set_u32(9, params.damage_width)?;
            writer.set_u32(10, params.damage_height)?;
            writer.set_u32(11, params.layer_count)?;
            writer.set_u32(12, params.flags)?;
            writer.finish()?;
            Ok::<(), super::opencl::KernelValueError>(())
        })();
        if values.is_err() {
            return false;
        }

        let local_ids = payload.add(UI4_COMPOSE_LAYERS_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_encode_sprite_quad_worklist_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: SpriteQuadWorklistRgba8Params,
    src_bytes: usize,
    dst_bytes: usize,
    desc_bytes: usize,
) -> bool {
    let desc_count = params.desc_count as usize;
    if desc_count == 0 || sprite_quad_worklist_walker_count(desc_count) != desc_count {
        return false;
    }
    let payload_end =
        RECT_WORKLIST_PAYLOAD_OFFSET_BYTES + desc_count * SPRITE_QUAD_WORKLIST_INDIRECT_BYTES;
    if payload_end > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }

    if !direct_rcs_write_sprite_quad_worklist_interface_descriptor(state) {
        return false;
    }
    if !direct_rcs_write_alpha_blend_worklist_surface_states(
        state,
        params.src_gpu,
        src_bytes,
        params.dst_gpu,
        dst_bytes,
        params.desc_gpu,
        desc_bytes,
    ) {
        return false;
    }
    for descriptor in 0..desc_count {
        let payload_offset =
            RECT_WORKLIST_PAYLOAD_OFFSET_BYTES + descriptor * SPRITE_QUAD_WORKLIST_INDIRECT_BYTES;
        let payload_params = SpriteQuadWorklistRgba8Params {
            desc_base: params.desc_base.saturating_add(descriptor as u32),
            desc_count: 1,
            ..params
        };
        if !direct_rcs_write_sprite_quad_worklist_payload_at(
            state,
            payload_offset,
            payload_params,
            0,
            0,
        ) {
            return false;
        }
    }

    direct_rcs_encode_sprite_quad_worklist_command_stream(
        state,
        upload,
        params.dst_width,
        params.dst_height,
        desc_count,
    )
}

fn direct_rcs_encode_sprite_quad_worklist_runs_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    dst: GpgpuRgba8Surface,
    desc: GpgpuRectWorklistDescBuffer,
    runs: &[GpgpuSpriteQuadWorklistRun<'_>],
) -> bool {
    if runs.is_empty() || runs.len() > SPRITE_QUAD_WORKLIST_MAX_DESCS {
        return false;
    }
    let total_descs = runs
        .iter()
        .try_fold(0usize, |total, run| total.checked_add(run.descs.len()));
    let Some(total_descs) = total_descs else {
        return false;
    };
    if total_descs == 0 || total_descs > SPRITE_QUAD_WORKLIST_MAX_DESCS {
        return false;
    }
    if runs.iter().any(|run| run.descs.is_empty()) {
        return false;
    }

    let state_bytes = runs
        .len()
        .checked_mul(SPRITE_QUAD_WORKLIST_RUN_STATE_BLOCK_BYTES);
    let Some(state_bytes) = state_bytes else {
        return false;
    };
    let Some(payload_base) =
        align_up(RECT_WORKLIST_IDD_OFFSET_BYTES.saturating_add(state_bytes), 0x40)
    else {
        return false;
    };
    let payload_end = payload_base
        .saturating_add(total_descs.saturating_mul(SPRITE_QUAD_WORKLIST_INDIRECT_BYTES));
    if payload_end > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }

    let mut desc_base = 0usize;
    for (run_index, run) in runs.iter().enumerate() {
        let run_base =
            RECT_WORKLIST_IDD_OFFSET_BYTES + run_index * SPRITE_QUAD_WORKLIST_RUN_STATE_BLOCK_BYTES;
        let idd_offset = run_base + SPRITE_QUAD_WORKLIST_RUN_IDD_REL;
        let binding_offset = run_base + SPRITE_QUAD_WORKLIST_RUN_BINDING_REL;
        let src_surface_offset = run_base + SPRITE_QUAD_WORKLIST_RUN_SRC_SURFACE_REL;
        let dst_surface_offset = run_base + SPRITE_QUAD_WORKLIST_RUN_DST_SURFACE_REL;
        let desc_surface_offset = run_base + SPRITE_QUAD_WORKLIST_RUN_DESC_SURFACE_REL;
        if !direct_rcs_write_sprite_quad_worklist_interface_descriptor_at(
            state,
            idd_offset,
            binding_offset,
        ) {
            return false;
        }
        if !direct_rcs_write_alpha_blend_worklist_surface_states_at(
            state,
            binding_offset,
            src_surface_offset,
            dst_surface_offset,
            desc_surface_offset,
            run.src.gpu,
            run.src.bytes,
            dst.gpu,
            dst.bytes,
            desc.gpu,
            desc.bytes,
        ) {
            return false;
        }
        for descriptor in 0..run.descs.len() {
            let payload_offset = payload_base
                + desc_base.saturating_add(descriptor) * SPRITE_QUAD_WORKLIST_INDIRECT_BYTES;
            let params = SpriteQuadWorklistRgba8Params {
                src_gpu: run.src.gpu,
                dst_gpu: dst.gpu,
                desc_gpu: desc.gpu,
                src_pitch_bytes: run.src.pitch_bytes,
                dst_pitch_bytes: dst.pitch_bytes,
                src_width: run.src.width,
                src_height: run.src.height,
                dst_width: dst.width,
                dst_height: dst.height,
                desc_base: desc_base.saturating_add(descriptor) as u32,
                desc_count: 1,
            };
            let Some(dispatch) =
                sprite_quad_descriptor_dispatch(run.descs[descriptor], dst.width, dst.height)
            else {
                return false;
            };
            if !direct_rcs_write_sprite_quad_worklist_payload_at(
                state,
                payload_offset,
                params,
                dispatch.global_x,
                dispatch.global_tile_y,
            ) {
                return false;
            }
        }
        desc_base = desc_base.saturating_add(run.descs.len());
    }

    direct_rcs_encode_sprite_quad_worklist_runs_command_stream(
        state,
        upload,
        dst,
        runs,
        payload_base,
    )
}

fn direct_rcs_encode_rect_worklist_command_stream(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    walker_count: usize,
    desc_count: usize,
    pre_marker: u32,
    post_marker: u32,
    one_group_per_descriptor: bool,
) -> bool {
    let batch_len = DIRECT_RCS_BATCH_BYTES / core::mem::size_of::<u32>();
    let batch = unsafe { core::slice::from_raw_parts_mut(state.batch_virt as *mut u32, batch_len) };
    let mut cursor = 0usize;
    let mut ok = true;

    ok &= direct_rcs_push_pipe_control_full(
        batch,
        &mut cursor,
        (1 << 9) | (1 << 11),
        PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL | 1,
    );
    ok &= direct_rcs_push(batch, &mut cursor, PIPELINE_SELECT_GPGPU);
    ok &= direct_rcs_push_pipe_control_full(batch, &mut cursor, 1 << 9, PIPE_CONTROL_CS_STALL);
    ok &= direct_rcs_push(batch, &mut cursor, PIPELINE_SELECT_3D);
    ok &= direct_rcs_push_pipe_control_full(
        batch,
        &mut cursor,
        (1 << 9) | (1 << 11),
        PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL,
    );
    ok &= direct_rcs_push_state_base_address(
        batch,
        &mut cursor,
        DIRECT_RCS_GPU_VA_BATCH_BASE,
        DIRECT_RCS_GPU_VA_BATCH_BASE,
        upload.gpu,
    );
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_INVALIDATE_BITS);
    ok &= direct_rcs_push(batch, &mut cursor, PIPELINE_SELECT_GPGPU);
    ok &= direct_rcs_push_pipe_control_full(batch, &mut cursor, 1 << 9, PIPE_CONTROL_CS_STALL);
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_VFE_STATE_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, GPGPU_VFE_DW3_UOS);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, GPGPU_VFE_DW5_UOS);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, RECT_WORKLIST_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, RECT_WORKLIST_IDD_OFFSET_BYTES as u32);
    ok &=
        direct_rcs_push_store_marker(batch, &mut cursor, RECT_WORKLIST_PRE_MARKER_SLOT, pre_marker);
    for walker in 0..walker_count {
        let desc_base = walker.saturating_mul(RECT_WORKLIST_DESCS_PER_WALKER);
        let local_count = desc_count
            .saturating_sub(desc_base)
            .min(RECT_WORKLIST_DESCS_PER_WALKER);
        let payload_offset =
            RECT_WORKLIST_PAYLOAD_OFFSET_BYTES + walker * RECT_WORKLIST_INDIRECT_BYTES;
        ok &= direct_rcs_push_rect_worklist_walker(
            batch,
            &mut cursor,
            payload_offset,
            if one_group_per_descriptor {
                local_count as u32
            } else {
                1
            },
            if one_group_per_descriptor {
                GPGPU_WALKER_SIMD16_MASK
            } else {
                simd16_right_mask(local_count as u32)
            },
        );
    }
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_FLUSH_BITS);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        RECT_WORKLIST_POST_MARKER_SLOT,
        post_marker,
    );
    ok &= direct_rcs_push(batch, &mut cursor, MI_BATCH_BUFFER_END);
    ok &= direct_rcs_push(batch, &mut cursor, MI_NOOP);

    if !ok {
        return false;
    }

    super::dma_flush(state.batch_virt, DIRECT_RCS_BATCH_BYTES);
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    true
}

fn direct_rcs_push_gpgpu_dispatch_prologue(
    batch: &mut [u32],
    cursor: &mut usize,
    upload: UploadedKernelArtifact,
    batch_gpu: u64,
) -> bool {
    direct_rcs_push_pipe_control_full(
        batch,
        cursor,
        (1 << 9) | (1 << 11),
        PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL | 1,
    ) && direct_rcs_push(batch, cursor, PIPELINE_SELECT_GPGPU)
        && direct_rcs_push_pipe_control_full(batch, cursor, 1 << 9, PIPE_CONTROL_CS_STALL)
        && direct_rcs_push(batch, cursor, PIPELINE_SELECT_3D)
        && direct_rcs_push_pipe_control_full(
            batch,
            cursor,
            (1 << 9) | (1 << 11),
            PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL,
        )
        && direct_rcs_push_state_base_address(batch, cursor, batch_gpu, batch_gpu, upload.gpu)
        && direct_rcs_push_pipe_control(batch, cursor, PIPE_CONTROL_INVALIDATE_BITS)
        && direct_rcs_push(batch, cursor, PIPELINE_SELECT_GPGPU)
        && direct_rcs_push_pipe_control_full(batch, cursor, 1 << 9, PIPE_CONTROL_CS_STALL)
        && direct_rcs_push(batch, cursor, MEDIA_VFE_STATE_CMD)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, GPGPU_VFE_DW3_UOS)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, GPGPU_VFE_DW5_UOS)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
}

fn direct_rcs_push_gpgpu_dispatch_epilogue(
    batch: &mut [u32],
    cursor: &mut usize,
    result_gpu: u64,
    post_marker_slot: usize,
    post_marker: u32,
) -> bool {
    // The CPU and display must not infer dispatch completion from a later
    // MI_STORE_DATA_IMM.  That store can become observable independently of
    // the dataport/cache release which makes the destination usable.  Keep the
    // full Gen12 HDC/L3 drain as a separate producer release, then make its
    // retirement cookie the post-sync write of an ordered PIPE_CONTROL.  The
    // result allocation is addressed through this context's PPGTT, so
    // PIPE_CONTROL_DEST_GGTT deliberately remains clear.
    direct_rcs_push_pipe_control(batch, cursor, PIPE_CONTROL_FLUSH_BITS)
        && direct_rcs_push_pipe_control_post_sync_marker_at(
            batch,
            cursor,
            result_gpu,
            post_marker_slot,
            post_marker,
        )
        && direct_rcs_push(batch, cursor, MI_BATCH_BUFFER_END)
        && direct_rcs_push(batch, cursor, MI_NOOP)
}

fn direct_rcs_encode_rgba8_scanout_release_batch(state: DirectRcsState) -> bool {
    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }
    let batch_len = DIRECT_RCS_BATCH_BYTES / core::mem::size_of::<u32>();
    let batch = unsafe { core::slice::from_raw_parts_mut(state.batch_virt as *mut u32, batch_len) };
    let mut cursor = 0usize;
    let ok = direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_FLUSH_BITS)
        && direct_rcs_push_pipe_control_post_sync_marker_at(
            batch,
            &mut cursor,
            state.gpu_va.result,
            RGBA8_SCANOUT_RELEASE_MARKER_SLOT,
            RGBA8_SCANOUT_RELEASE_MARKER,
        )
        && direct_rcs_push(batch, &mut cursor, MI_BATCH_BUFFER_END)
        && direct_rcs_push(batch, &mut cursor, MI_NOOP);
    if !ok {
        return false;
    }
    super::dma_flush(state.batch_virt, DIRECT_RCS_BATCH_BYTES);
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    true
}

fn direct_rcs_encode_sprite_quad_worklist_runs_command_stream(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    dst: GpgpuRgba8Surface,
    runs: &[GpgpuSpriteQuadWorklistRun<'_>],
    payload_base: usize,
) -> bool {
    let batch_len = DIRECT_RCS_BATCH_BYTES / core::mem::size_of::<u32>();
    let batch = unsafe { core::slice::from_raw_parts_mut(state.batch_virt as *mut u32, batch_len) };
    let mut cursor = 0usize;
    let mut ok = true;

    ok &= direct_rcs_push_gpgpu_dispatch_prologue(batch, &mut cursor, upload, state.gpu_va.batch);
    ok &= direct_rcs_push_store_marker_at(
        batch,
        &mut cursor,
        state.gpu_va.result,
        SPRITE_QUAD_WORKLIST_PRE_MARKER_SLOT,
        SPRITE_QUAD_WORKLIST_PRE_MARKER,
    );
    let mut descriptor_base = 0usize;
    let total_descriptors = runs
        .iter()
        .fold(0usize, |total, run| total.saturating_add(run.descs.len()));
    let mut submitted_descriptors = 0usize;
    for (run_index, run) in runs.iter().enumerate() {
        let idd_offset =
            RECT_WORKLIST_IDD_OFFSET_BYTES + run_index * SPRITE_QUAD_WORKLIST_RUN_STATE_BLOCK_BYTES;
        ok &= direct_rcs_push(batch, &mut cursor, MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD);
        ok &= direct_rcs_push(batch, &mut cursor, 0);
        ok &= direct_rcs_push(batch, &mut cursor, RECT_WORKLIST_IDD_BYTES as u32);
        ok &= direct_rcs_push(batch, &mut cursor, idd_offset as u32);
        for descriptor in 0..run.descs.len() {
            let Some(dispatch) =
                sprite_quad_descriptor_dispatch(run.descs[descriptor], dst.width, dst.height)
            else {
                return false;
            };
            let payload_offset = payload_base
                + descriptor_base.saturating_add(descriptor) * SPRITE_QUAD_WORKLIST_INDIRECT_BYTES;
            ok &= direct_rcs_push_sprite_quad_worklist_walker(
                batch,
                &mut cursor,
                payload_offset,
                dispatch.walker.group_x,
                dispatch.walker.group_y,
                dispatch.walker.right_mask,
            );
            ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
            ok &= direct_rcs_push(batch, &mut cursor, 0);
            submitted_descriptors = submitted_descriptors.saturating_add(1);
            if submitted_descriptors < total_descriptors {
                ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_FLUSH_BITS);
            }
        }
        descriptor_base = descriptor_base.saturating_add(run.descs.len());
    }
    ok &= direct_rcs_push_gpgpu_dispatch_epilogue(
        batch,
        &mut cursor,
        state.gpu_va.result,
        SPRITE_QUAD_WORKLIST_POST_MARKER_SLOT,
        SPRITE_QUAD_WORKLIST_POST_MARKER,
    );

    if !ok {
        return false;
    }

    super::dma_flush(state.batch_virt, DIRECT_RCS_BATCH_BYTES);
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    true
}

fn direct_rcs_encode_sprite_quad_worklist_command_stream(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    dst_width: u32,
    dst_height: u32,
    desc_count: usize,
) -> bool {
    let batch_len = DIRECT_RCS_BATCH_BYTES / core::mem::size_of::<u32>();
    let batch = unsafe { core::slice::from_raw_parts_mut(state.batch_virt as *mut u32, batch_len) };
    let mut cursor = 0usize;
    let mut ok = true;

    ok &= direct_rcs_push_gpgpu_dispatch_prologue(batch, &mut cursor, upload, state.gpu_va.batch);
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, RECT_WORKLIST_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, RECT_WORKLIST_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_store_marker_at(
        batch,
        &mut cursor,
        state.gpu_va.result,
        SPRITE_QUAD_WORKLIST_PRE_MARKER_SLOT,
        SPRITE_QUAD_WORKLIST_PRE_MARKER,
    );
    let Some(dispatch) = sprite_quad_2d_dispatch(dst_width, dst_height) else {
        return false;
    };
    for descriptor in 0..desc_count {
        let payload_offset =
            RECT_WORKLIST_PAYLOAD_OFFSET_BYTES + descriptor * SPRITE_QUAD_WORKLIST_INDIRECT_BYTES;
        ok &= direct_rcs_push_sprite_quad_worklist_walker(
            batch,
            &mut cursor,
            payload_offset,
            dispatch.group_x,
            dispatch.group_y,
            dispatch.right_mask,
        );
        ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
        ok &= direct_rcs_push(batch, &mut cursor, 0);
        if descriptor + 1 < desc_count {
            ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_FLUSH_BITS);
        }
    }
    ok &= direct_rcs_push_gpgpu_dispatch_epilogue(
        batch,
        &mut cursor,
        state.gpu_va.result,
        SPRITE_QUAD_WORKLIST_POST_MARKER_SLOT,
        SPRITE_QUAD_WORKLIST_POST_MARKER,
    );

    if !ok {
        return false;
    }

    super::dma_flush(state.batch_virt, DIRECT_RCS_BATCH_BYTES);
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    true
}

fn direct_rcs_encode_ui4_nv12_tile64_to_primary_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: Ui4Nv12Tile64ToPrimaryXrgbParams,
    source_bytes: usize,
    base_bytes: usize,
    dst_bytes: usize,
) -> bool {
    if params.output_width == 0
        || params.output_height == 0
        || UI4_NV12_PRIMARY_PAYLOAD_OFFSET_BYTES + UI4_NV12_PRIMARY_INDIRECT_BYTES
            > DIRECT_RCS_BATCH_BYTES
    {
        return false;
    }
    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }
    if !direct_rcs_write_interface_descriptor_at(
        state,
        UI4_NV12_PRIMARY_IDD_OFFSET_BYTES,
        UI4_NV12_PRIMARY_BINDING_TABLE_OFFSET_BYTES,
        UI4_NV12_YTILE_TO_PRIMARY_XRGB_TEXT_OFFSET_BYTES,
        3,
        UI4_NV12_PRIMARY_CROSS_THREAD_GRFS,
    ) || !direct_rcs_write_alpha_blend_worklist_surface_states_at(
        state,
        UI4_NV12_PRIMARY_BINDING_TABLE_OFFSET_BYTES,
        UI4_NV12_PRIMARY_SRC_SURFACE_STATE_OFFSET_BYTES,
        UI4_NV12_PRIMARY_BASE_SURFACE_STATE_OFFSET_BYTES,
        UI4_NV12_PRIMARY_DST_SURFACE_STATE_OFFSET_BYTES,
        params.nv12_gpu,
        source_bytes,
        params.base_gpu,
        base_bytes,
        params.dst_gpu,
        dst_bytes,
    ) || !direct_rcs_write_ui4_nv12_primary_payload_at(
        state,
        UI4_NV12_PRIMARY_PAYLOAD_OFFSET_BYTES,
        params,
    ) {
        return false;
    }

    let batch_len = DIRECT_RCS_BATCH_BYTES / core::mem::size_of::<u32>();
    let batch = unsafe { core::slice::from_raw_parts_mut(state.batch_virt as *mut u32, batch_len) };
    let mut cursor = 0usize;
    let mut ok =
        direct_rcs_push_gpgpu_dispatch_prologue(batch, &mut cursor, upload, state.gpu_va.batch);
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, COPY_RECT_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, UI4_NV12_PRIMARY_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_store_marker_at(
        batch,
        &mut cursor,
        state.gpu_va.result,
        SPRITE_QUAD_WORKLIST_PRE_MARKER_SLOT,
        SPRITE_QUAD_WORKLIST_PRE_MARKER,
    );
    ok &= direct_rcs_push_gpgpu_walker_2d(
        batch,
        &mut cursor,
        UI4_NV12_PRIMARY_PAYLOAD_OFFSET_BYTES,
        UI4_NV12_PRIMARY_INDIRECT_BYTES,
        params.output_width.div_ceil(16),
        params.output_height,
        GPGPU_WALKER_SIMD16_MASK,
    );
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push_gpgpu_dispatch_epilogue(
        batch,
        &mut cursor,
        state.gpu_va.result,
        SPRITE_QUAD_WORKLIST_POST_MARKER_SLOT,
        SPRITE_QUAD_WORKLIST_POST_MARKER,
    );
    if !ok {
        return false;
    }
    super::dma_flush(state.batch_virt, DIRECT_RCS_BATCH_BYTES);
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    true
}

fn direct_rcs_encode_copy_rect_2d_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: CopyRectRgba8Params,
    src_bytes: usize,
    dst_bytes: usize,
) -> bool {
    if params.width == 0
        || params.height == 0
        || COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES + COPY_RECT_INDIRECT_BYTES
            > DIRECT_RCS_BATCH_BYTES
    {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }

    if !direct_rcs_write_copy_rect_interface_descriptor_at(
        state,
        COPY_RECT_BATCH_IDD_OFFSET_BYTES,
        COPY_RECT_BATCH_BINDING_TABLE_OFFSET_BYTES,
        COPY_RECT_RGBA8_TEXT_OFFSET_BYTES,
    ) || !direct_rcs_write_copy_rect_surface_states_at(
        state,
        COPY_RECT_BATCH_BINDING_TABLE_OFFSET_BYTES,
        COPY_RECT_BATCH_SRC_SURFACE_STATE_OFFSET_BYTES,
        COPY_RECT_BATCH_DST_SURFACE_STATE_OFFSET_BYTES,
        params.src_gpu,
        src_bytes,
        params.dst_gpu,
        dst_bytes,
    ) || !direct_rcs_write_copy_rect_payload_at(
        state,
        COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES,
        params,
    ) {
        return false;
    }

    let Some(dispatch) = copy_rect_2d_dispatch(params.width, params.height) else {
        return false;
    };
    direct_rcs_finish_two_buffer_dispatch_batch(
        state,
        upload,
        COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES,
        COPY_RECT_INDIRECT_BYTES,
        dispatch,
    )
}

fn direct_rcs_encode_resolve_tile64_msaa4_2d_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: CopyRectRgba8Params,
    src_bytes: usize,
    dst_bytes: usize,
) -> bool {
    if params.width == 0 || params.height == 0 {
        return false;
    }
    if COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES + COPY_RECT_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES
    {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }

    if !direct_rcs_write_copy_rect_interface_descriptor_at_with_cross_thread_grfs(
        state,
        COPY_RECT_BATCH_IDD_OFFSET_BYTES,
        COPY_RECT_BATCH_BINDING_TABLE_OFFSET_BYTES,
        RESOLVE_TILE64_MSAA4_RGBA8_TEXT_OFFSET_BYTES,
        3,
    ) {
        return false;
    }
    if !direct_rcs_write_copy_rect_surface_states_at(
        state,
        COPY_RECT_BATCH_BINDING_TABLE_OFFSET_BYTES,
        COPY_RECT_BATCH_SRC_SURFACE_STATE_OFFSET_BYTES,
        COPY_RECT_BATCH_DST_SURFACE_STATE_OFFSET_BYTES,
        params.src_gpu,
        src_bytes,
        params.dst_gpu,
        dst_bytes,
    ) {
        return false;
    }
    if !direct_rcs_write_copy_rect_payload_at(
        state,
        COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES,
        params,
    ) {
        return false;
    }

    let batch_len = DIRECT_RCS_BATCH_BYTES / core::mem::size_of::<u32>();
    let batch = unsafe { core::slice::from_raw_parts_mut(state.batch_virt as *mut u32, batch_len) };
    let mut cursor = 0usize;
    let mut ok = true;
    let Some(dispatch) = fill_rect_2d_dispatch(params.width, params.height) else {
        return false;
    };

    ok &= direct_rcs_push_pipe_control_full(
        batch,
        &mut cursor,
        (1 << 9) | (1 << 11),
        PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL | 1,
    );
    ok &= direct_rcs_push(batch, &mut cursor, PIPELINE_SELECT_GPGPU);
    ok &= direct_rcs_push_pipe_control_full(batch, &mut cursor, 1 << 9, PIPE_CONTROL_CS_STALL);
    ok &= direct_rcs_push(batch, &mut cursor, PIPELINE_SELECT_3D);
    ok &= direct_rcs_push_pipe_control_full(
        batch,
        &mut cursor,
        (1 << 9) | (1 << 11),
        PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL,
    );
    ok &= direct_rcs_push_state_base_address(
        batch,
        &mut cursor,
        DIRECT_RCS_GPU_VA_BATCH_BASE,
        DIRECT_RCS_GPU_VA_BATCH_BASE,
        upload.gpu,
    );
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_INVALIDATE_BITS);
    ok &= direct_rcs_push(batch, &mut cursor, PIPELINE_SELECT_GPGPU);
    ok &= direct_rcs_push_pipe_control_full(batch, &mut cursor, 1 << 9, PIPE_CONTROL_CS_STALL);
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_VFE_STATE_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, GPGPU_VFE_DW3_UOS);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, GPGPU_VFE_DW5_UOS);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, COPY_RECT_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, COPY_RECT_BATCH_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        COPY_RECT_PRE_MARKER_SLOT,
        COPY_RECT_PRE_MARKER,
    );
    ok &= direct_rcs_push_gpgpu_walker_2d(
        batch,
        &mut cursor,
        COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES,
        COPY_RECT_INDIRECT_BYTES,
        dispatch.group_x,
        dispatch.group_y,
        dispatch.right_mask,
    );
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_FLUSH_BITS);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        COPY_RECT_POST_MARKER_SLOT,
        COPY_RECT_POST_MARKER,
    );
    ok &= direct_rcs_push(batch, &mut cursor, MI_BATCH_BUFFER_END);
    ok &= direct_rcs_push(batch, &mut cursor, MI_NOOP);

    if !ok {
        return false;
    }

    super::dma_flush(state.batch_virt, DIRECT_RCS_BATCH_BYTES);
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    true
}

fn direct_rcs_encode_font_outline_coverage_r8_2d_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: FontOutlineCoverageR8Params,
    ops_bytes: usize,
    mask_bytes: usize,
) -> bool {
    if params.rect_width == 0
        || params.rect_height == 0
        || COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES + FONT_OUTLINE_COVERAGE_R8_INDIRECT_BYTES
            > DIRECT_RCS_BATCH_BYTES
    {
        return false;
    }
    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }
    if !direct_rcs_write_copy_rect_interface_descriptor_at_with_cross_thread_grfs(
        state,
        COPY_RECT_BATCH_IDD_OFFSET_BYTES,
        COPY_RECT_BATCH_BINDING_TABLE_OFFSET_BYTES,
        FONT_OUTLINE_COVERAGE_R8_TEXT_OFFSET_BYTES,
        4,
    ) || !direct_rcs_write_copy_rect_surface_states_at(
        state,
        COPY_RECT_BATCH_BINDING_TABLE_OFFSET_BYTES,
        COPY_RECT_BATCH_SRC_SURFACE_STATE_OFFSET_BYTES,
        COPY_RECT_BATCH_DST_SURFACE_STATE_OFFSET_BYTES,
        params.ops_gpu,
        ops_bytes,
        params.mask_gpu,
        mask_bytes,
    ) || !direct_rcs_write_font_outline_coverage_r8_payload_at(
        state,
        COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES,
        params,
    ) {
        return false;
    }
    direct_rcs_finish_two_buffer_2d_batch(
        state,
        upload,
        COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES,
        FONT_OUTLINE_COVERAGE_R8_INDIRECT_BYTES,
        params.rect_width,
        params.rect_height,
    )
}

fn direct_rcs_encode_glyph_mask_2d_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: CopyRectRgba8Params,
    color_rgba: u32,
    mask_bytes: usize,
    dst_bytes: usize,
) -> bool {
    if params.width == 0
        || params.height == 0
        || COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES + GLYPH_MASK_INDIRECT_BYTES
            > DIRECT_RCS_BATCH_BYTES
    {
        return false;
    }
    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }
    if !direct_rcs_write_copy_rect_interface_descriptor_at_with_cross_thread_grfs(
        state,
        COPY_RECT_BATCH_IDD_OFFSET_BYTES,
        COPY_RECT_BATCH_BINDING_TABLE_OFFSET_BYTES,
        GLYPH_MASK_RGBA8_TEXT_OFFSET_BYTES,
        4,
    ) || !direct_rcs_write_copy_rect_surface_states_at(
        state,
        COPY_RECT_BATCH_BINDING_TABLE_OFFSET_BYTES,
        COPY_RECT_BATCH_SRC_SURFACE_STATE_OFFSET_BYTES,
        COPY_RECT_BATCH_DST_SURFACE_STATE_OFFSET_BYTES,
        params.src_gpu,
        mask_bytes,
        params.dst_gpu,
        dst_bytes,
    ) || !direct_rcs_write_glyph_mask_payload_at(
        state,
        COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES,
        params,
        color_rgba,
    ) {
        return false;
    }
    direct_rcs_finish_two_buffer_2d_batch(
        state,
        upload,
        COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES,
        GLYPH_MASK_INDIRECT_BYTES,
        params.width,
        params.height,
    )
}

fn direct_rcs_encode_glyph_mask_layers_2d_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    layers: &[GpgpuGlyphMaskLayer],
    dst: GpgpuRgba8Surface,
) -> bool {
    let mut active_walkers = 0usize;
    for layer in layers {
        let blit = GpgpuGlyphMaskBlit {
            mask: layer.mask,
            mask_rect: layer.mask_rect,
            dst,
            dst_xy: layer.dst_xy,
            color_rgba: layer.color_rgba,
        };
        if lower_glyph_mask_blit(blit).is_some() {
            active_walkers += 1;
        }
    }
    if active_walkers == 0 {
        return false;
    }
    if active_walkers > GLYPH_MASK_BATCH_MAX_LAYERS {
        return false;
    }
    let payload_end = GLYPH_MASK_BATCH_PAYLOAD_BASE_OFFSET_BYTES
        .saturating_add(active_walkers.saturating_mul(GLYPH_MASK_INDIRECT_BYTES));
    if payload_end > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }
    let mut walker_index = 0usize;
    for layer in layers {
        let blit = GpgpuGlyphMaskBlit {
            mask: layer.mask,
            mask_rect: layer.mask_rect,
            dst,
            dst_xy: layer.dst_xy,
            color_rgba: layer.color_rgba,
        };
        let Some(params) = lower_glyph_mask_blit(blit) else {
            continue;
        };
        let state_block = GLYPH_MASK_BATCH_STATE_BASE_OFFSET_BYTES
            + walker_index * GLYPH_MASK_BATCH_STATE_BLOCK_BYTES;
        let idd_offset = state_block + GLYPH_MASK_BATCH_IDD_OFFSET_IN_BLOCK_BYTES;
        let binding_table_offset =
            state_block + GLYPH_MASK_BATCH_BINDING_TABLE_OFFSET_IN_BLOCK_BYTES;
        let src_surface_offset = state_block + GLYPH_MASK_BATCH_SRC_SURFACE_OFFSET_IN_BLOCK_BYTES;
        let dst_surface_offset = state_block + GLYPH_MASK_BATCH_DST_SURFACE_OFFSET_IN_BLOCK_BYTES;
        if !direct_rcs_write_copy_rect_interface_descriptor_at_with_cross_thread_grfs(
            state,
            idd_offset,
            binding_table_offset,
            GLYPH_MASK_RGBA8_TEXT_OFFSET_BYTES,
            4,
        ) || !direct_rcs_write_copy_rect_surface_states_at(
            state,
            binding_table_offset,
            src_surface_offset,
            dst_surface_offset,
            params.src_gpu,
            layer.mask.bytes,
            params.dst_gpu,
            dst.bytes,
        ) {
            return false;
        }
        let payload_offset =
            GLYPH_MASK_BATCH_PAYLOAD_BASE_OFFSET_BYTES + walker_index * GLYPH_MASK_INDIRECT_BYTES;
        if !direct_rcs_write_glyph_mask_payload_at(state, payload_offset, params, layer.color_rgba)
        {
            return false;
        }
        walker_index += 1;
    }

    let batch_len = DIRECT_RCS_BATCH_BYTES / core::mem::size_of::<u32>();
    let batch = unsafe { core::slice::from_raw_parts_mut(state.batch_virt as *mut u32, batch_len) };
    let mut cursor = 0usize;
    let mut ok = true;
    ok &= direct_rcs_push_gpgpu_dispatch_prologue(batch, &mut cursor, upload, state.gpu_va.batch);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        COPY_RECT_PRE_MARKER_SLOT,
        COPY_RECT_PRE_MARKER,
    );

    walker_index = 0;
    for layer in layers {
        let blit = GpgpuGlyphMaskBlit {
            mask: layer.mask,
            mask_rect: layer.mask_rect,
            dst,
            dst_xy: layer.dst_xy,
            color_rgba: layer.color_rgba,
        };
        let Some(params) = lower_glyph_mask_blit(blit) else {
            continue;
        };
        let Some(dispatch) = fill_rect_2d_dispatch(params.width, params.height) else {
            return false;
        };
        let state_block = GLYPH_MASK_BATCH_STATE_BASE_OFFSET_BYTES
            + walker_index * GLYPH_MASK_BATCH_STATE_BLOCK_BYTES;
        let idd_offset = state_block + GLYPH_MASK_BATCH_IDD_OFFSET_IN_BLOCK_BYTES;
        let payload_offset =
            GLYPH_MASK_BATCH_PAYLOAD_BASE_OFFSET_BYTES + walker_index * GLYPH_MASK_INDIRECT_BYTES;
        ok &= direct_rcs_push(batch, &mut cursor, MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD);
        ok &= direct_rcs_push(batch, &mut cursor, 0);
        ok &= direct_rcs_push(batch, &mut cursor, COPY_RECT_IDD_BYTES as u32);
        ok &= direct_rcs_push(batch, &mut cursor, idd_offset as u32);
        ok &= direct_rcs_push_gpgpu_walker_2d(
            batch,
            &mut cursor,
            payload_offset,
            GLYPH_MASK_INDIRECT_BYTES,
            dispatch.group_x,
            dispatch.group_y,
            dispatch.right_mask,
        );
        ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
        ok &= direct_rcs_push(batch, &mut cursor, 0);
        walker_index += 1;
    }
    ok &= direct_rcs_push_gpgpu_dispatch_epilogue(
        batch,
        &mut cursor,
        state.gpu_va.result,
        COPY_RECT_POST_MARKER_SLOT,
        COPY_RECT_POST_MARKER,
    );
    if !ok
        || cursor.saturating_mul(core::mem::size_of::<u32>())
            > GLYPH_MASK_BATCH_STATE_BASE_OFFSET_BYTES
    {
        return false;
    }
    super::dma_flush(state.batch_virt, DIRECT_RCS_BATCH_BYTES);
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    true
}

fn direct_rcs_finish_two_buffer_2d_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    payload_offset: usize,
    indirect_bytes: usize,
    width: u32,
    height: u32,
) -> bool {
    let Some(dispatch) = fill_rect_2d_dispatch(width, height) else {
        return false;
    };
    direct_rcs_finish_two_buffer_dispatch_batch(
        state,
        upload,
        payload_offset,
        indirect_bytes,
        dispatch,
    )
}

fn direct_rcs_finish_two_buffer_dispatch_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    payload_offset: usize,
    indirect_bytes: usize,
    dispatch: FillRect2dDispatch,
) -> bool {
    let batch_len = DIRECT_RCS_BATCH_BYTES / core::mem::size_of::<u32>();
    let batch = unsafe { core::slice::from_raw_parts_mut(state.batch_virt as *mut u32, batch_len) };
    let mut cursor = 0usize;
    let mut ok = true;
    ok &= direct_rcs_push_pipe_control_full(
        batch,
        &mut cursor,
        (1 << 9) | (1 << 11),
        PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL | 1,
    );
    ok &= direct_rcs_push(batch, &mut cursor, PIPELINE_SELECT_GPGPU);
    ok &= direct_rcs_push_pipe_control_full(batch, &mut cursor, 1 << 9, PIPE_CONTROL_CS_STALL);
    ok &= direct_rcs_push(batch, &mut cursor, PIPELINE_SELECT_3D);
    ok &= direct_rcs_push_pipe_control_full(
        batch,
        &mut cursor,
        (1 << 9) | (1 << 11),
        PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL,
    );
    ok &= direct_rcs_push_state_base_address(
        batch,
        &mut cursor,
        DIRECT_RCS_GPU_VA_BATCH_BASE,
        DIRECT_RCS_GPU_VA_BATCH_BASE,
        upload.gpu,
    );
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_INVALIDATE_BITS);
    ok &= direct_rcs_push(batch, &mut cursor, PIPELINE_SELECT_GPGPU);
    ok &= direct_rcs_push_pipe_control_full(batch, &mut cursor, 1 << 9, PIPE_CONTROL_CS_STALL);
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_VFE_STATE_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, GPGPU_VFE_DW3_UOS);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, GPGPU_VFE_DW5_UOS);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, COPY_RECT_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, COPY_RECT_BATCH_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        COPY_RECT_PRE_MARKER_SLOT,
        COPY_RECT_PRE_MARKER,
    );
    ok &= direct_rcs_push_gpgpu_walker_2d(
        batch,
        &mut cursor,
        payload_offset,
        indirect_bytes,
        dispatch.group_x,
        dispatch.group_y,
        dispatch.right_mask,
    );
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_FLUSH_BITS);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        COPY_RECT_POST_MARKER_SLOT,
        COPY_RECT_POST_MARKER,
    );
    ok &= direct_rcs_push(batch, &mut cursor, MI_BATCH_BUFFER_END);
    ok &= direct_rcs_push(batch, &mut cursor, MI_NOOP);
    if !ok {
        return false;
    }
    super::dma_flush(state.batch_virt, DIRECT_RCS_BATCH_BYTES);
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    true
}

fn direct_rcs_encode_fill_rect_2d_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: FillRectRgba8Params,
    dst_bytes: usize,
) -> bool {
    if params.width == 0 || params.height == 0 {
        return false;
    }
    if CLEAR_RECT_PAYLOAD_OFFSET_BYTES + CLEAR_RECT_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }

    if !direct_rcs_write_fill_rect_interface_descriptor(state) {
        return false;
    }
    if !direct_rcs_write_clear_rect_surface_state(state, params.dst_gpu, dst_bytes) {
        return false;
    }
    if !direct_rcs_write_fill_rect_payload(state, params) {
        return false;
    }

    let batch_len = DIRECT_RCS_BATCH_BYTES / core::mem::size_of::<u32>();
    let batch = unsafe { core::slice::from_raw_parts_mut(state.batch_virt as *mut u32, batch_len) };
    let mut cursor = 0usize;
    let mut ok = true;
    let Some(dispatch) = fill_rect_2d_dispatch(params.width, params.height) else {
        return false;
    };

    ok &= direct_rcs_push_pipe_control_full(
        batch,
        &mut cursor,
        (1 << 9) | (1 << 11),
        PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL | 1,
    );
    ok &= direct_rcs_push(batch, &mut cursor, PIPELINE_SELECT_GPGPU);
    ok &= direct_rcs_push_pipe_control_full(batch, &mut cursor, 1 << 9, PIPE_CONTROL_CS_STALL);
    ok &= direct_rcs_push(batch, &mut cursor, PIPELINE_SELECT_3D);
    ok &= direct_rcs_push_pipe_control_full(
        batch,
        &mut cursor,
        (1 << 9) | (1 << 11),
        PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL,
    );
    ok &= direct_rcs_push_state_base_address(
        batch,
        &mut cursor,
        DIRECT_RCS_GPU_VA_BATCH_BASE,
        DIRECT_RCS_GPU_VA_BATCH_BASE,
        upload.gpu,
    );
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_INVALIDATE_BITS);
    ok &= direct_rcs_push(batch, &mut cursor, PIPELINE_SELECT_GPGPU);
    ok &= direct_rcs_push_pipe_control_full(batch, &mut cursor, 1 << 9, PIPE_CONTROL_CS_STALL);
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_VFE_STATE_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, GPGPU_VFE_DW3_UOS);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, GPGPU_VFE_DW5_UOS);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, CLEAR_RECT_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, CLEAR_RECT_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        CLEAR_RECT_PRE_MARKER_SLOT,
        CLEAR_RECT_PRE_MARKER,
    );
    ok &= direct_rcs_push_gpgpu_walker_2d(
        batch,
        &mut cursor,
        CLEAR_RECT_PAYLOAD_OFFSET_BYTES,
        CLEAR_RECT_INDIRECT_BYTES,
        dispatch.group_x,
        dispatch.group_y,
        dispatch.right_mask,
    );
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_FLUSH_BITS);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        CLEAR_RECT_POST_MARKER_SLOT,
        CLEAR_RECT_POST_MARKER,
    );
    ok &= direct_rcs_push(batch, &mut cursor, MI_BATCH_BUFFER_END);
    ok &= direct_rcs_push(batch, &mut cursor, MI_NOOP);

    if !ok {
        return false;
    }

    super::dma_flush(state.batch_virt, DIRECT_RCS_BATCH_BYTES);
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    true
}

fn direct_rcs_encode_skybox_sample_rgb565_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: SkyboxSampleRgb565Params,
    skybox_bytes: usize,
    dst_bytes: usize,
) -> bool {
    if params.rect_width == 0 || params.rect_height == 0 {
        return false;
    }
    if SKYBOX_SAMPLE_PAYLOAD_OFFSET_BYTES + SKYBOX_SAMPLE_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }

    if !direct_rcs_write_copy_rect_interface_descriptor_at_with_cross_thread_grfs(
        state,
        SKYBOX_SAMPLE_IDD_OFFSET_BYTES,
        SKYBOX_SAMPLE_BINDING_TABLE_OFFSET_BYTES,
        SKYBOX_SAMPLE_RGB565_TEXT_OFFSET_BYTES,
        5,
    ) {
        return false;
    }
    if !direct_rcs_write_copy_rect_surface_states_at(
        state,
        SKYBOX_SAMPLE_BINDING_TABLE_OFFSET_BYTES,
        SKYBOX_SAMPLE_SRC_SURFACE_STATE_OFFSET_BYTES,
        SKYBOX_SAMPLE_DST_SURFACE_STATE_OFFSET_BYTES,
        params.sky_gpu,
        skybox_bytes,
        params.dst_gpu,
        dst_bytes,
    ) {
        return false;
    }
    if !direct_rcs_write_skybox_sample_rgb565_payload_at(
        state,
        SKYBOX_SAMPLE_PAYLOAD_OFFSET_BYTES,
        params,
    ) {
        return false;
    }

    let batch_len = DIRECT_RCS_BATCH_BYTES / core::mem::size_of::<u32>();
    let batch = unsafe { core::slice::from_raw_parts_mut(state.batch_virt as *mut u32, batch_len) };
    let mut cursor = 0usize;
    let mut ok = true;
    let group_x = params.rect_width.div_ceil(16).max(1);
    let group_y = params.rect_height.max(1);
    let last_group_pixels = ((params.rect_width - 1) % 16) + 1;
    let right_mask = if last_group_pixels >= 16 {
        GPGPU_WALKER_SIMD16_MASK
    } else {
        (1u32 << last_group_pixels) - 1
    };

    ok &= direct_rcs_push_pipe_control_full(
        batch,
        &mut cursor,
        (1 << 9) | (1 << 11),
        PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL | 1,
    );
    ok &= direct_rcs_push(batch, &mut cursor, PIPELINE_SELECT_GPGPU);
    ok &= direct_rcs_push_pipe_control_full(batch, &mut cursor, 1 << 9, PIPE_CONTROL_CS_STALL);
    ok &= direct_rcs_push(batch, &mut cursor, PIPELINE_SELECT_3D);
    ok &= direct_rcs_push_pipe_control_full(
        batch,
        &mut cursor,
        (1 << 9) | (1 << 11),
        PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL,
    );
    ok &= direct_rcs_push_state_base_address(
        batch,
        &mut cursor,
        DIRECT_RCS_GPU_VA_BATCH_BASE,
        DIRECT_RCS_GPU_VA_BATCH_BASE,
        upload.gpu,
    );
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_INVALIDATE_BITS);
    ok &= direct_rcs_push(batch, &mut cursor, PIPELINE_SELECT_GPGPU);
    ok &= direct_rcs_push_pipe_control_full(batch, &mut cursor, 1 << 9, PIPE_CONTROL_CS_STALL);
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_VFE_STATE_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, GPGPU_VFE_DW3_UOS);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, GPGPU_VFE_DW5_UOS);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, SKYBOX_SAMPLE_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, SKYBOX_SAMPLE_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        SKYBOX_SAMPLE_PRE_MARKER_SLOT,
        SKYBOX_SAMPLE_PRE_MARKER,
    );
    ok &= direct_rcs_push_gpgpu_walker_2d(
        batch,
        &mut cursor,
        SKYBOX_SAMPLE_PAYLOAD_OFFSET_BYTES,
        SKYBOX_SAMPLE_INDIRECT_BYTES,
        group_x,
        group_y,
        right_mask,
    );
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_FLUSH_BITS);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        SKYBOX_SAMPLE_POST_MARKER_SLOT,
        SKYBOX_SAMPLE_POST_MARKER,
    );
    ok &= direct_rcs_push(batch, &mut cursor, MI_BATCH_BUFFER_END);
    ok &= direct_rcs_push(batch, &mut cursor, MI_NOOP);

    if !ok {
        return false;
    }

    super::dma_flush(state.batch_virt, DIRECT_RCS_BATCH_BYTES);
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    true
}

fn direct_rcs_encode_chart_sine_rgba8_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: ChartSineRgba8Params,
    dst_bytes: usize,
) -> bool {
    if params.rect_width == 0 || params.rect_height == 0 {
        return false;
    }
    if CHART_SINE_PAYLOAD_OFFSET_BYTES + CHART_SINE_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }

    if !direct_rcs_write_interface_descriptor_at(
        state,
        CHART_SINE_IDD_OFFSET_BYTES,
        CHART_SINE_BINDING_TABLE_OFFSET_BYTES,
        CHART_SINE_RGBA8_TEXT_OFFSET_BYTES,
        1,
        4,
    ) {
        return false;
    }
    let binding_end = CHART_SINE_BINDING_TABLE_OFFSET_BYTES + core::mem::size_of::<u32>();
    if binding_end > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    unsafe {
        let binding = state.batch_virt.add(CHART_SINE_BINDING_TABLE_OFFSET_BYTES) as *mut u32;
        core::ptr::write_volatile(binding, CHART_SINE_DST_SURFACE_STATE_OFFSET_BYTES as u32);
    }
    if !direct_rcs_write_buffer_surface_state(
        state,
        CHART_SINE_DST_SURFACE_STATE_OFFSET_BYTES,
        params.dst_gpu,
        dst_bytes,
    ) || !direct_rcs_write_chart_sine_rgba8_payload_at(
        state,
        CHART_SINE_PAYLOAD_OFFSET_BYTES,
        params,
    ) {
        return false;
    }

    let batch_len = DIRECT_RCS_BATCH_BYTES / core::mem::size_of::<u32>();
    let batch = unsafe { core::slice::from_raw_parts_mut(state.batch_virt as *mut u32, batch_len) };
    let mut cursor = 0usize;
    let mut ok = true;
    let group_x = params.rect_width.div_ceil(16).max(1);
    let group_y = params.rect_height.max(1);
    // RightExecutionMask describes the SIMD lanes in every hardware thread,
    // not merely the final X workgroup. Each group here is one full SIMD16
    // thread; the shader's x >= rect_width guard safely rejects padded lanes
    // in the final group.
    let right_mask = GPGPU_WALKER_SIMD16_MASK;

    ok &= direct_rcs_push_pipe_control_full(
        batch,
        &mut cursor,
        (1 << 9) | (1 << 11),
        PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL | 1,
    );
    ok &= direct_rcs_push(batch, &mut cursor, PIPELINE_SELECT_GPGPU);
    ok &= direct_rcs_push_pipe_control_full(batch, &mut cursor, 1 << 9, PIPE_CONTROL_CS_STALL);
    ok &= direct_rcs_push(batch, &mut cursor, PIPELINE_SELECT_3D);
    ok &= direct_rcs_push_pipe_control_full(
        batch,
        &mut cursor,
        (1 << 9) | (1 << 11),
        PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL,
    );
    ok &= direct_rcs_push_state_base_address(
        batch,
        &mut cursor,
        DIRECT_RCS_GPU_VA_BATCH_BASE,
        DIRECT_RCS_GPU_VA_BATCH_BASE,
        upload.gpu,
    );
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_INVALIDATE_BITS);
    ok &= direct_rcs_push(batch, &mut cursor, PIPELINE_SELECT_GPGPU);
    ok &= direct_rcs_push_pipe_control_full(batch, &mut cursor, 1 << 9, PIPE_CONTROL_CS_STALL);
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_VFE_STATE_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, GPGPU_VFE_DW3_UOS);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, GPGPU_VFE_DW5_UOS);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, CHART_SINE_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, CHART_SINE_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        CHART_SINE_PRE_MARKER_SLOT,
        CHART_SINE_PRE_MARKER,
    );
    ok &= direct_rcs_push_gpgpu_walker_2d(
        batch,
        &mut cursor,
        CHART_SINE_PAYLOAD_OFFSET_BYTES,
        CHART_SINE_INDIRECT_BYTES,
        group_x,
        group_y,
        right_mask,
    );
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_FLUSH_BITS);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        CHART_SINE_POST_MARKER_SLOT,
        CHART_SINE_POST_MARKER,
    );
    ok &= direct_rcs_push(batch, &mut cursor, MI_BATCH_BUFFER_END);
    ok &= direct_rcs_push(batch, &mut cursor, MI_NOOP);
    if !ok {
        return false;
    }

    super::dma_flush(state.batch_virt, DIRECT_RCS_BATCH_BYTES);
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    true
}

fn direct_rcs_encode_pixel_plasma_rgba8_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: PixelPlasmaRgba8Params,
    dst_bytes: usize,
) -> bool {
    if params.rect_width == 0 || params.rect_height == 0 {
        return false;
    }
    if PIXEL_PLASMA_PAYLOAD_OFFSET_BYTES + PIXEL_PLASMA_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }

    if !direct_rcs_write_interface_descriptor_at(
        state,
        PIXEL_PLASMA_IDD_OFFSET_BYTES,
        PIXEL_PLASMA_BINDING_TABLE_OFFSET_BYTES,
        PIXEL_PLASMA_RGBA8_TEXT_OFFSET_BYTES,
        1,
        4,
    ) {
        return false;
    }
    let binding_end = PIXEL_PLASMA_BINDING_TABLE_OFFSET_BYTES + core::mem::size_of::<u32>();
    if binding_end > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    unsafe {
        let binding = state
            .batch_virt
            .add(PIXEL_PLASMA_BINDING_TABLE_OFFSET_BYTES) as *mut u32;
        core::ptr::write_volatile(binding, PIXEL_PLASMA_DST_SURFACE_STATE_OFFSET_BYTES as u32);
    }
    if !direct_rcs_write_buffer_surface_state(
        state,
        PIXEL_PLASMA_DST_SURFACE_STATE_OFFSET_BYTES,
        params.dst_gpu,
        dst_bytes,
    ) || !direct_rcs_write_pixel_plasma_rgba8_payload_at(
        state,
        PIXEL_PLASMA_PAYLOAD_OFFSET_BYTES,
        params,
    ) {
        return false;
    }

    let batch_len = DIRECT_RCS_BATCH_BYTES / core::mem::size_of::<u32>();
    let batch = unsafe { core::slice::from_raw_parts_mut(state.batch_virt as *mut u32, batch_len) };
    let mut cursor = 0usize;
    let mut ok = true;
    let group_x = params.rect_width.div_ceil(16).max(1);
    let group_y = params.rect_height.max(1);
    let last_group_pixels = ((params.rect_width - 1) % 16) + 1;
    let right_mask = if last_group_pixels >= 16 {
        GPGPU_WALKER_SIMD16_MASK
    } else {
        (1u32 << last_group_pixels) - 1
    };

    ok &= direct_rcs_push_pipe_control_full(
        batch,
        &mut cursor,
        (1 << 9) | (1 << 11),
        PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL | 1,
    );
    ok &= direct_rcs_push(batch, &mut cursor, PIPELINE_SELECT_GPGPU);
    ok &= direct_rcs_push_pipe_control_full(batch, &mut cursor, 1 << 9, PIPE_CONTROL_CS_STALL);
    ok &= direct_rcs_push(batch, &mut cursor, PIPELINE_SELECT_3D);
    ok &= direct_rcs_push_pipe_control_full(
        batch,
        &mut cursor,
        (1 << 9) | (1 << 11),
        PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL,
    );
    ok &= direct_rcs_push_state_base_address(
        batch,
        &mut cursor,
        DIRECT_RCS_GPU_VA_BATCH_BASE,
        DIRECT_RCS_GPU_VA_BATCH_BASE,
        upload.gpu,
    );
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_INVALIDATE_BITS);
    ok &= direct_rcs_push(batch, &mut cursor, PIPELINE_SELECT_GPGPU);
    ok &= direct_rcs_push_pipe_control_full(batch, &mut cursor, 1 << 9, PIPE_CONTROL_CS_STALL);
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_VFE_STATE_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, GPGPU_VFE_DW3_UOS);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, GPGPU_VFE_DW5_UOS);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, PIXEL_PLASMA_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, PIXEL_PLASMA_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        PIXEL_PLASMA_PRE_MARKER_SLOT,
        PIXEL_PLASMA_PRE_MARKER,
    );
    ok &= direct_rcs_push_gpgpu_walker_2d(
        batch,
        &mut cursor,
        PIXEL_PLASMA_PAYLOAD_OFFSET_BYTES,
        PIXEL_PLASMA_INDIRECT_BYTES,
        group_x,
        group_y,
        right_mask,
    );
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_FLUSH_BITS);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        PIXEL_PLASMA_POST_MARKER_SLOT,
        PIXEL_PLASMA_POST_MARKER,
    );
    ok &= direct_rcs_push(batch, &mut cursor, MI_BATCH_BUFFER_END);
    ok &= direct_rcs_push(batch, &mut cursor, MI_NOOP);
    if !ok {
        return false;
    }

    super::dma_flush(state.batch_virt, DIRECT_RCS_BATCH_BYTES);
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    true
}

fn direct_rcs_encode_font_outline_mesh_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: FontOutlineMeshParams,
    src_bytes: usize,
    dst_bytes: usize,
) -> bool {
    if params.op_count == 0
        || FONT_OUTLINE_MESH_PAYLOAD_OFFSET_BYTES + FONT_OUTLINE_MESH_INDIRECT_BYTES
            > DIRECT_RCS_BATCH_BYTES
    {
        return false;
    }
    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }
    if !direct_rcs_write_interface_descriptor_at(
        state,
        FONT_OUTLINE_MESH_IDD_OFFSET_BYTES,
        FONT_OUTLINE_MESH_BINDING_TABLE_OFFSET_BYTES,
        FONT_OUTLINE_MESH_TEXT_OFFSET_BYTES,
        2,
        4,
    ) || !direct_rcs_write_copy_rect_surface_states_at(
        state,
        FONT_OUTLINE_MESH_BINDING_TABLE_OFFSET_BYTES,
        FONT_OUTLINE_MESH_SRC_SURFACE_STATE_OFFSET_BYTES,
        FONT_OUTLINE_MESH_DST_SURFACE_STATE_OFFSET_BYTES,
        params.src_gpu,
        src_bytes,
        params.dst_gpu,
        dst_bytes,
    ) || !direct_rcs_write_font_outline_mesh_payload_at(
        state,
        FONT_OUTLINE_MESH_PAYLOAD_OFFSET_BYTES,
        params,
    ) {
        return false;
    }

    let batch_len = DIRECT_RCS_BATCH_BYTES / core::mem::size_of::<u32>();
    let batch = unsafe { core::slice::from_raw_parts_mut(state.batch_virt as *mut u32, batch_len) };
    let mut cursor = 0usize;
    let mut ok = true;
    ok &= direct_rcs_push_pipe_control_full(
        batch,
        &mut cursor,
        (1 << 9) | (1 << 11),
        PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL | 1,
    );
    ok &= direct_rcs_push(batch, &mut cursor, PIPELINE_SELECT_GPGPU);
    ok &= direct_rcs_push_pipe_control_full(batch, &mut cursor, 1 << 9, PIPE_CONTROL_CS_STALL);
    ok &= direct_rcs_push(batch, &mut cursor, PIPELINE_SELECT_3D);
    ok &= direct_rcs_push_pipe_control_full(
        batch,
        &mut cursor,
        (1 << 9) | (1 << 11),
        PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL,
    );
    ok &= direct_rcs_push_state_base_address(
        batch,
        &mut cursor,
        DIRECT_RCS_GPU_VA_BATCH_BASE,
        DIRECT_RCS_GPU_VA_BATCH_BASE,
        upload.gpu,
    );
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_INVALIDATE_BITS);
    ok &= direct_rcs_push(batch, &mut cursor, PIPELINE_SELECT_GPGPU);
    ok &= direct_rcs_push_pipe_control_full(batch, &mut cursor, 1 << 9, PIPE_CONTROL_CS_STALL);
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_VFE_STATE_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, GPGPU_VFE_DW3_UOS);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, GPGPU_VFE_DW5_UOS);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, FONT_OUTLINE_MESH_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, FONT_OUTLINE_MESH_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        FONT_OUTLINE_MESH_PRE_MARKER_SLOT,
        FONT_OUTLINE_MESH_PRE_MARKER,
    );
    ok &= direct_rcs_push_gpgpu_walker_2d(
        batch,
        &mut cursor,
        FONT_OUTLINE_MESH_PAYLOAD_OFFSET_BYTES,
        FONT_OUTLINE_MESH_INDIRECT_BYTES,
        1,
        1,
        1,
    );
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_FLUSH_BITS);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        FONT_OUTLINE_MESH_POST_MARKER_SLOT,
        FONT_OUTLINE_MESH_POST_MARKER,
    );
    ok &= direct_rcs_push(batch, &mut cursor, MI_BATCH_BUFFER_END);
    ok &= direct_rcs_push(batch, &mut cursor, MI_NOOP);
    if !ok {
        return false;
    }
    super::dma_flush(state.batch_virt, DIRECT_RCS_BATCH_BYTES);
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    true
}

fn direct_rcs_write_copy_rect_interface_descriptor_at(
    state: DirectRcsState,
    idd_offset: usize,
    binding_table_offset: usize,
    text_offset_bytes: u64,
) -> bool {
    direct_rcs_write_copy_rect_interface_descriptor_at_with_cross_thread_grfs(
        state,
        idd_offset,
        binding_table_offset,
        text_offset_bytes,
        3,
    )
}

fn direct_rcs_write_copy_rect_interface_descriptor_at_with_cross_thread_grfs(
    state: DirectRcsState,
    idd_offset: usize,
    binding_table_offset: usize,
    text_offset_bytes: u64,
    cross_thread_grfs: u32,
) -> bool {
    direct_rcs_write_interface_descriptor_at(
        state,
        idd_offset,
        binding_table_offset,
        text_offset_bytes,
        2,
        cross_thread_grfs,
    )
}

fn direct_rcs_write_interface_descriptor_at(
    state: DirectRcsState,
    idd_offset: usize,
    binding_table_offset: usize,
    text_offset_bytes: u64,
    binding_count: u32,
    cross_thread_grfs: u32,
) -> bool {
    if idd_offset + COPY_RECT_IDD_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    let idd = unsafe { state.batch_virt.add(idd_offset) as *mut u32 };
    unsafe {
        core::ptr::write_volatile(idd, text_offset_bytes as u32);
        core::ptr::write_volatile(idd.add(1), 0);
        core::ptr::write_volatile(idd.add(2), IDD_THREAD_PREEMPTION_DISABLE);
        core::ptr::write_volatile(idd.add(3), 0);
        core::ptr::write_volatile(
            idd.add(4),
            (binding_table_offset as u32) | binding_count.min(31),
        );
        core::ptr::write_volatile(idd.add(5), 3 << 16);
        core::ptr::write_volatile(idd.add(6), GPGPU_WALKER_GROUP_THREADS);
        core::ptr::write_volatile(idd.add(7), cross_thread_grfs);
    }
    true
}

fn direct_rcs_write_copy_rect_surface_states_at(
    state: DirectRcsState,
    binding_table_offset: usize,
    src_surface_offset: usize,
    dst_surface_offset: usize,
    src_gpu: u64,
    src_bytes: usize,
    dst_gpu: u64,
    dst_bytes: usize,
) -> bool {
    let binding_end = binding_table_offset + 2 * core::mem::size_of::<u32>();
    let surface_bytes = COPY_RECT_SURFACE_STATE_DWORDS * core::mem::size_of::<u32>();
    let src_surface_end = src_surface_offset + surface_bytes;
    let dst_surface_end = dst_surface_offset + surface_bytes;
    if binding_end > DIRECT_RCS_BATCH_BYTES
        || src_surface_end > DIRECT_RCS_BATCH_BYTES
        || dst_surface_end > DIRECT_RCS_BATCH_BYTES
    {
        return false;
    }

    unsafe {
        let binding = state.batch_virt.add(binding_table_offset) as *mut u32;
        core::ptr::write_volatile(binding, src_surface_offset as u32);
        core::ptr::write_volatile(binding.add(1), dst_surface_offset as u32);
    }
    direct_rcs_write_buffer_surface_state(state, src_surface_offset, src_gpu, src_bytes)
        && direct_rcs_write_buffer_surface_state(state, dst_surface_offset, dst_gpu, dst_bytes)
}

fn direct_rcs_write_buffer_surface_state(
    state: DirectRcsState,
    surface_offset: usize,
    gpu: u64,
    target_bytes: usize,
) -> bool {
    let surface_bytes = COPY_RECT_SURFACE_STATE_DWORDS * core::mem::size_of::<u32>();
    let surface_end = surface_offset + surface_bytes;
    if surface_end > DIRECT_RCS_BATCH_BYTES || target_bytes == 0 {
        return false;
    }

    let extent = target_bytes.saturating_sub(1);
    let surface_width_minus1 = (extent & 0x7F) as u32;
    let surface_height_minus1 = ((extent >> 7) & 0x3FFF) as u32;
    let surface_depth_minus1 = ((extent >> 21) & 0x7FF) as u32;
    let surface_dword0 = (SURFTYPE_BUFFER << 29) | (SURFACE_FORMAT_RAW << 18);
    let surface_dword2 = (surface_height_minus1 << 16) | surface_width_minus1;
    let surface_dword3 = surface_depth_minus1 << 21;

    unsafe {
        let surface = state.batch_virt.add(surface_offset) as *mut u32;
        for index in 0..COPY_RECT_SURFACE_STATE_DWORDS {
            core::ptr::write_volatile(surface.add(index), 0);
        }
        core::ptr::write_volatile(surface, surface_dword0);
        core::ptr::write_volatile(surface.add(1), RENDER_MOCS << 24);
        core::ptr::write_volatile(surface.add(2), surface_dword2);
        core::ptr::write_volatile(surface.add(3), surface_dword3);
        core::ptr::write_volatile(surface.add(8), gpu as u32);
        core::ptr::write_volatile(surface.add(9), (gpu >> 32) as u32);
    }
    true
}

fn direct_rcs_write_ui4_nv12_primary_payload_at(
    state: DirectRcsState,
    payload_offset: usize,
    params: Ui4Nv12Tile64ToPrimaryXrgbParams,
) -> bool {
    if payload_offset + UI4_NV12_PRIMARY_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    unsafe {
        let payload = state.batch_virt.add(payload_offset);
        core::ptr::write_bytes(payload, 0, UI4_NV12_PRIMARY_INDIRECT_BYTES);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords, params.output_width);
        core::ptr::write_volatile(dwords.add(1), params.output_height);
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        core::ptr::write_volatile(dwords.add(12), params.nv12_gpu as u32);
        core::ptr::write_volatile(dwords.add(13), (params.nv12_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(14), params.base_gpu as u32);
        core::ptr::write_volatile(dwords.add(15), (params.base_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(16), params.dst_gpu as u32);
        core::ptr::write_volatile(dwords.add(17), (params.dst_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(18), params.src_pitch_bytes);
        core::ptr::write_volatile(dwords.add(19), params.src_uv_offset);
        core::ptr::write_volatile(dwords.add(20), params.base_pitch_bytes);
        core::ptr::write_volatile(dwords.add(21), params.dst_pitch_bytes);
        core::ptr::write_volatile(dwords.add(22), params.output_width);
        core::ptr::write_volatile(dwords.add(23), params.output_height);
        core::ptr::write_volatile(dwords.add(24), params.content_dst_x);
        core::ptr::write_volatile(dwords.add(25), params.content_dst_y);
        core::ptr::write_volatile(dwords.add(26), params.content_width);
        core::ptr::write_volatile(dwords.add(27), params.content_height);
        core::ptr::write_volatile(dwords.add(28), params.source_x);
        core::ptr::write_volatile(dwords.add(29), params.source_y);

        let local_ids = payload.add(UI4_NV12_PRIMARY_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_write_copy_rect_payload_at(
    state: DirectRcsState,
    payload_offset: usize,
    params: CopyRectRgba8Params,
) -> bool {
    if payload_offset + COPY_RECT_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        let payload = state.batch_virt.add(payload_offset);
        core::ptr::write_bytes(payload, 0, COPY_RECT_INDIRECT_BYTES);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        core::ptr::write_volatile(dwords.add(12), params.src_gpu as u32);
        core::ptr::write_volatile(dwords.add(13), (params.src_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(14), params.dst_gpu as u32);
        core::ptr::write_volatile(dwords.add(15), (params.dst_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(16), params.src_pitch_bytes);
        core::ptr::write_volatile(dwords.add(17), params.dst_pitch_bytes);
        core::ptr::write_volatile(dwords.add(18), params.src_x);
        core::ptr::write_volatile(dwords.add(19), params.src_y);
        core::ptr::write_volatile(dwords.add(20), params.dst_x);
        core::ptr::write_volatile(dwords.add(21), params.dst_y);
        core::ptr::write_volatile(dwords.add(22), params.width);
        core::ptr::write_volatile(dwords.add(23), params.height);

        let local_ids = payload.add(COPY_RECT_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_write_glyph_mask_payload_at(
    state: DirectRcsState,
    payload_offset: usize,
    params: CopyRectRgba8Params,
    color_rgba: u32,
) -> bool {
    if payload_offset + GLYPH_MASK_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        let payload = state.batch_virt.add(payload_offset);
        core::ptr::write_bytes(payload, 0, GLYPH_MASK_INDIRECT_BYTES);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        core::ptr::write_volatile(dwords.add(12), params.src_gpu as u32);
        core::ptr::write_volatile(dwords.add(13), (params.src_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(14), params.dst_gpu as u32);
        core::ptr::write_volatile(dwords.add(15), (params.dst_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(16), params.src_pitch_bytes);
        core::ptr::write_volatile(dwords.add(17), params.dst_pitch_bytes);
        core::ptr::write_volatile(dwords.add(18), params.src_x);
        core::ptr::write_volatile(dwords.add(19), params.src_y);
        core::ptr::write_volatile(dwords.add(20), params.dst_x);
        core::ptr::write_volatile(dwords.add(21), params.dst_y);
        core::ptr::write_volatile(dwords.add(22), params.width);
        core::ptr::write_volatile(dwords.add(23), params.height);
        core::ptr::write_volatile(dwords.add(24), color_rgba);

        let local_ids = payload.add(GLYPH_MASK_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_write_skybox_sample_rgb565_payload_at(
    state: DirectRcsState,
    payload_offset: usize,
    params: SkyboxSampleRgb565Params,
) -> bool {
    if payload_offset + SKYBOX_SAMPLE_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        let payload = state.batch_virt.add(payload_offset);
        core::ptr::write_bytes(payload, 0, SKYBOX_SAMPLE_INDIRECT_BYTES);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        core::ptr::write_volatile(dwords.add(12), params.sky_gpu as u32);
        core::ptr::write_volatile(dwords.add(13), (params.sky_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(14), params.dst_gpu as u32);
        core::ptr::write_volatile(dwords.add(15), (params.dst_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(16), params.sky_pitch_bytes);
        core::ptr::write_volatile(dwords.add(17), params.sky_width);
        core::ptr::write_volatile(dwords.add(18), params.sky_height);
        core::ptr::write_volatile(dwords.add(19), params.dst_pitch_bytes);
        core::ptr::write_volatile(dwords.add(20), params.dst_width);
        core::ptr::write_volatile(dwords.add(21), params.dst_height);
        core::ptr::write_volatile(dwords.add(22), params.rect_x);
        core::ptr::write_volatile(dwords.add(23), params.rect_y);
        core::ptr::write_volatile(dwords.add(24), params.rect_width);
        core::ptr::write_volatile(dwords.add(25), params.rect_height);
        core::ptr::write_volatile(dwords.add(26), params.right_x.to_bits());
        core::ptr::write_volatile(dwords.add(27), params.right_y.to_bits());
        core::ptr::write_volatile(dwords.add(28), params.right_z.to_bits());
        core::ptr::write_volatile(dwords.add(29), params.up_x.to_bits());
        core::ptr::write_volatile(dwords.add(30), params.up_y.to_bits());
        core::ptr::write_volatile(dwords.add(31), params.up_z.to_bits());
        core::ptr::write_volatile(dwords.add(32), params.forward_x.to_bits());
        core::ptr::write_volatile(dwords.add(33), params.forward_y.to_bits());
        core::ptr::write_volatile(dwords.add(34), params.forward_z.to_bits());
        core::ptr::write_volatile(dwords.add(35), params.aspect_tan_half_fov_y.to_bits());
        core::ptr::write_volatile(dwords.add(36), params.tan_half_fov_y.to_bits());

        let local_ids = payload.add(SKYBOX_SAMPLE_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_write_chart_sine_rgba8_payload_at(
    state: DirectRcsState,
    payload_offset: usize,
    params: ChartSineRgba8Params,
) -> bool {
    if payload_offset + CHART_SINE_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    let Some(known) = super::opencl::registry::known_aot_kernel(CHART_SINE_RGBA8_KERNEL_NAME)
    else {
        crate::log_error!(
            target: "gpgpu";
            "intel/gpgpu: chart-sine-rgba8 payload rejected reason=missing-opencl-contract\n"
        );
        return false;
    };

    unsafe {
        let payload = state.batch_virt.add(payload_offset);
        core::ptr::write_bytes(payload, 0, CHART_SINE_INDIRECT_BYTES);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        core::ptr::write_volatile(dwords.add(12), params.dst_gpu as u32);
        core::ptr::write_volatile(dwords.add(13), (params.dst_gpu >> 32) as u32);

        let cross_thread = core::slice::from_raw_parts_mut(payload, CHART_SINE_CROSS_THREAD_BYTES);
        let values = (|| {
            let mut writer = super::opencl::KernelValueWriter::new(known.contract, cross_thread)?;
            writer.set_u32(1, params.dst_pitch_bytes)?;
            writer.set_u32(2, params.dst_width)?;
            writer.set_u32(3, params.dst_height)?;
            writer.set_u32(4, params.rect_x)?;
            writer.set_u32(5, params.rect_y)?;
            writer.set_u32(6, params.rect_width)?;
            writer.set_u32(7, params.rect_height)?;
            writer.set_f32(8, params.phase)?;
            writer.set_f32(9, params.cycles)?;
            writer.set_f32(10, params.amplitude)?;
            writer.set_f32(11, params.line_width_px)?;
            writer.set_u32(12, params.background_rgba)?;
            writer.set_u32(13, params.minor_grid_rgba)?;
            writer.set_u32(14, params.major_grid_rgba)?;
            writer.set_u32(15, params.axis_rgba)?;
            writer.set_u32(16, params.line_rgba)?;
            writer.set_u32(17, params.glow_rgba)?;
            writer.set_u32(18, params.flags)?;
            writer.finish()?;
            Ok::<(), super::opencl::KernelValueError>(())
        })();
        if let Err(err) = values {
            crate::log_error!(
                target: "gpgpu";
                "intel/gpgpu: chart-sine-rgba8 payload rejected reason=value-contract error={:?}\n",
                err
            );
            return false;
        }

        let local_ids = payload.add(CHART_SINE_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_write_pixel_plasma_rgba8_payload_at(
    state: DirectRcsState,
    payload_offset: usize,
    params: PixelPlasmaRgba8Params,
) -> bool {
    if payload_offset + PIXEL_PLASMA_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    let Some(known) = super::opencl::registry::known_aot_kernel(PIXEL_PLASMA_RGBA8_KERNEL_NAME)
    else {
        crate::log_error!(
            target: "gpgpu";
            "intel/gpgpu: pixel-plasma-rgba8 payload rejected reason=missing-opencl-contract\n"
        );
        return false;
    };

    unsafe {
        let payload = state.batch_virt.add(payload_offset);
        core::ptr::write_bytes(payload, 0, PIXEL_PLASMA_INDIRECT_BYTES);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        core::ptr::write_volatile(dwords.add(12), params.dst_gpu as u32);
        core::ptr::write_volatile(dwords.add(13), (params.dst_gpu >> 32) as u32);

        let cross_thread =
            core::slice::from_raw_parts_mut(payload, PIXEL_PLASMA_CROSS_THREAD_BYTES);
        let values = (|| {
            let mut writer = super::opencl::KernelValueWriter::new(known.contract, cross_thread)?;
            writer.set_u32(1, params.dst_pitch_bytes)?;
            writer.set_u32(2, params.dst_width)?;
            writer.set_u32(3, params.dst_height)?;
            writer.set_u32(4, params.rect_x)?;
            writer.set_u32(5, params.rect_y)?;
            writer.set_u32(6, params.rect_width)?;
            writer.set_u32(7, params.rect_height)?;
            writer.set_f32(8, params.time)?;
            writer.set_f32(9, params.spatial_scale)?;
            writer.set_f32(10, params.intensity)?;
            writer.set_u32(11, params.low_rgba)?;
            writer.set_u32(12, params.mid_rgba)?;
            writer.set_u32(13, params.high_rgba)?;
            writer.set_u32(14, params.flags)?;
            writer.finish()?;
            Ok::<(), super::opencl::KernelValueError>(())
        })();
        if let Err(err) = values {
            crate::log_error!(
                target: "gpgpu";
                "intel/gpgpu: pixel-plasma-rgba8 payload rejected reason=value-contract error={:?}\n",
                err
            );
            return false;
        }

        let local_ids = payload.add(PIXEL_PLASMA_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_write_font_outline_coverage_r8_payload_at(
    state: DirectRcsState,
    payload_offset: usize,
    params: FontOutlineCoverageR8Params,
) -> bool {
    if payload_offset + FONT_OUTLINE_COVERAGE_R8_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    let Some(known) =
        super::opencl::registry::known_aot_kernel(FONT_OUTLINE_COVERAGE_R8_KERNEL_NAME)
    else {
        crate::log_error!(
            target: "gpgpu";
            "intel/gpgpu: font-outline-coverage-r8 payload rejected reason=missing-opencl-contract\n"
        );
        return false;
    };
    unsafe {
        let payload = state.batch_virt.add(payload_offset);
        core::ptr::write_bytes(payload, 0, FONT_OUTLINE_COVERAGE_R8_INDIRECT_BYTES);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        core::ptr::write_volatile(dwords.add(12), params.ops_gpu as u32);
        core::ptr::write_volatile(dwords.add(13), (params.ops_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(14), params.mask_gpu as u32);
        core::ptr::write_volatile(dwords.add(15), (params.mask_gpu >> 32) as u32);

        let cross_thread =
            core::slice::from_raw_parts_mut(payload, FONT_OUTLINE_COVERAGE_R8_CROSS_THREAD_BYTES);
        let values = (|| {
            let mut writer = super::opencl::KernelValueWriter::new(known.contract, cross_thread)?;
            writer.set_u32(2, params.op_count)?;
            writer.set_u32(3, params.subdivisions)?;
            writer.set_u32(4, params.mask_pitch_bytes)?;
            writer.set_u32(5, params.mask_width)?;
            writer.set_u32(6, params.mask_height)?;
            writer.set_u32(7, params.rect_x)?;
            writer.set_u32(8, params.rect_y)?;
            writer.set_u32(9, params.rect_width)?;
            writer.set_u32(10, params.rect_height)?;
            writer.set_f32(11, params.optical_bias_px)?;
            writer.finish()?;
            Ok::<(), super::opencl::KernelValueError>(())
        })();
        if let Err(err) = values {
            crate::log_error!(
                target: "gpgpu";
                "intel/gpgpu: font-outline-coverage-r8 payload rejected reason=value-contract error={:?}\n",
                err
            );
            return false;
        }
        let local_ids = payload.add(FONT_OUTLINE_COVERAGE_R8_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_write_font_outline_mesh_payload_at(
    state: DirectRcsState,
    payload_offset: usize,
    params: FontOutlineMeshParams,
) -> bool {
    if payload_offset + FONT_OUTLINE_MESH_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    let Some(known) = super::opencl::registry::known_aot_kernel(FONT_OUTLINE_MESH_KERNEL_NAME)
    else {
        crate::log_error!(
            target: "gpgpu";
            "intel/gpgpu: font-outline-mesh payload rejected reason=missing-opencl-contract\n"
        );
        return false;
    };

    unsafe {
        let payload = state.batch_virt.add(payload_offset);
        core::ptr::write_bytes(payload, 0, FONT_OUTLINE_MESH_INDIRECT_BYTES);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        core::ptr::write_volatile(dwords.add(12), params.src_gpu as u32);
        core::ptr::write_volatile(dwords.add(13), (params.src_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(14), params.dst_gpu as u32);
        core::ptr::write_volatile(dwords.add(15), (params.dst_gpu >> 32) as u32);

        let cross_thread =
            core::slice::from_raw_parts_mut(payload, FONT_OUTLINE_MESH_CROSS_THREAD_BYTES);
        let values = (|| {
            let mut writer = super::opencl::KernelValueWriter::new(known.contract, cross_thread)?;
            writer.set_u32(2, params.op_count)?;
            writer.set_u32(3, params.stage)?;
            writer.set_u32(4, params.subdivisions)?;
            writer.set_u32(5, params.max_vertices)?;
            writer.set_u32(6, params.max_indices)?;
            writer.set_f32(7, params.scale)?;
            writer.set_f32(8, params.origin_x)?;
            writer.set_f32(9, params.origin_y)?;
            writer.set_f32(10, params.stroke_half_width)?;
            writer.finish()?;
            Ok::<(), super::opencl::KernelValueError>(())
        })();
        if let Err(err) = values {
            crate::log_error!(
                target: "gpgpu";
                "intel/gpgpu: font-outline-mesh payload rejected reason=value-contract error={:?}\n",
                err
            );
            return false;
        }

        let local_ids = payload.add(FONT_OUTLINE_MESH_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_write_fill_rect_worklist_interface_descriptor(state: DirectRcsState) -> bool {
    direct_rcs_write_rect_worklist_interface_descriptor(
        state,
        FILL_RECT_WORKLIST_RGBA8_TEXT_OFFSET_BYTES,
        2,
        FILL_RECT_WORKLIST_CROSS_THREAD_GRFS,
    )
}

fn direct_rcs_write_mandel64_worklist_interface_descriptor(state: DirectRcsState) -> bool {
    direct_rcs_write_rect_worklist_interface_descriptor(
        state,
        MANDEL64_WORKLIST_RGBA8_TEXT_OFFSET_BYTES,
        2,
        RECT_WORKLIST_CROSS_THREAD_GRFS,
    )
}

fn direct_rcs_write_sprite_quad_worklist_interface_descriptor(state: DirectRcsState) -> bool {
    direct_rcs_write_rect_worklist_interface_descriptor(
        state,
        SPRITE_QUAD_WORKLIST_RGBA8_TEXT_OFFSET_BYTES,
        3,
        SPRITE_QUAD_WORKLIST_CROSS_THREAD_GRFS,
    )
}

fn direct_rcs_write_sprite_quad_worklist_interface_descriptor_at(
    state: DirectRcsState,
    idd_offset: usize,
    binding_table_offset: usize,
) -> bool {
    direct_rcs_write_rect_worklist_interface_descriptor_at(
        state,
        idd_offset,
        binding_table_offset,
        SPRITE_QUAD_WORKLIST_RGBA8_TEXT_OFFSET_BYTES,
        3,
        SPRITE_QUAD_WORKLIST_CROSS_THREAD_GRFS,
    )
}

fn direct_rcs_write_rect_worklist_interface_descriptor(
    state: DirectRcsState,
    text_offset_bytes: u64,
    binding_table_entries: u32,
    cross_thread_grfs: u32,
) -> bool {
    direct_rcs_write_rect_worklist_interface_descriptor_at(
        state,
        RECT_WORKLIST_IDD_OFFSET_BYTES,
        RECT_WORKLIST_BINDING_TABLE_OFFSET_BYTES,
        text_offset_bytes,
        binding_table_entries,
        cross_thread_grfs,
    )
}

fn direct_rcs_write_rect_worklist_interface_descriptor_at(
    state: DirectRcsState,
    idd_offset: usize,
    binding_table_offset: usize,
    text_offset_bytes: u64,
    binding_table_entries: u32,
    cross_thread_grfs: u32,
) -> bool {
    if idd_offset + RECT_WORKLIST_IDD_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    let idd = unsafe { state.batch_virt.add(idd_offset) as *mut u32 };
    unsafe {
        core::ptr::write_volatile(idd, text_offset_bytes as u32);
        core::ptr::write_volatile(idd.add(1), 0);
        core::ptr::write_volatile(idd.add(2), IDD_THREAD_PREEMPTION_DISABLE);
        core::ptr::write_volatile(idd.add(3), 0);
        core::ptr::write_volatile(
            idd.add(4),
            (binding_table_offset as u32) | binding_table_entries,
        );
        core::ptr::write_volatile(idd.add(5), 3 << 16);
        core::ptr::write_volatile(idd.add(6), GPGPU_WALKER_GROUP_THREADS);
        core::ptr::write_volatile(idd.add(7), cross_thread_grfs);
    }
    true
}

fn direct_rcs_write_fill_rect_worklist_surface_states(
    state: DirectRcsState,
    dst_gpu: u64,
    dst_bytes: usize,
    desc_gpu: u64,
    desc_bytes: usize,
) -> bool {
    let binding_end = RECT_WORKLIST_BINDING_TABLE_OFFSET_BYTES + 2 * core::mem::size_of::<u32>();
    let surface_bytes = COPY_RECT_SURFACE_STATE_DWORDS * core::mem::size_of::<u32>();
    let dst_surface_end = RECT_WORKLIST_DST_SURFACE_STATE_OFFSET_BYTES + surface_bytes;
    let desc_surface_end = RECT_WORKLIST_DESC_SURFACE_STATE_OFFSET_BYTES + surface_bytes;
    if binding_end > DIRECT_RCS_BATCH_BYTES
        || dst_surface_end > DIRECT_RCS_BATCH_BYTES
        || desc_surface_end > DIRECT_RCS_BATCH_BYTES
    {
        return false;
    }

    unsafe {
        let binding = state
            .batch_virt
            .add(RECT_WORKLIST_BINDING_TABLE_OFFSET_BYTES) as *mut u32;
        core::ptr::write_volatile(binding, RECT_WORKLIST_DST_SURFACE_STATE_OFFSET_BYTES as u32);
        core::ptr::write_volatile(
            binding.add(1),
            RECT_WORKLIST_DESC_SURFACE_STATE_OFFSET_BYTES as u32,
        );
    }

    direct_rcs_write_buffer_surface_state(
        state,
        RECT_WORKLIST_DST_SURFACE_STATE_OFFSET_BYTES,
        dst_gpu,
        dst_bytes,
    ) && direct_rcs_write_buffer_surface_state(
        state,
        RECT_WORKLIST_DESC_SURFACE_STATE_OFFSET_BYTES,
        desc_gpu,
        desc_bytes,
    )
}

fn direct_rcs_write_alpha_blend_worklist_surface_states(
    state: DirectRcsState,
    src_gpu: u64,
    src_bytes: usize,
    dst_gpu: u64,
    dst_bytes: usize,
    desc_gpu: u64,
    desc_bytes: usize,
) -> bool {
    direct_rcs_write_alpha_blend_worklist_surface_states_at(
        state,
        RECT_WORKLIST_BINDING_TABLE_OFFSET_BYTES,
        RECT_WORKLIST_SRC_SURFACE_STATE_OFFSET_BYTES,
        RECT_WORKLIST_DST_SURFACE_STATE_OFFSET_BYTES,
        RECT_WORKLIST_DESC_SURFACE_STATE_OFFSET_BYTES,
        src_gpu,
        src_bytes,
        dst_gpu,
        dst_bytes,
        desc_gpu,
        desc_bytes,
    )
}

fn direct_rcs_write_alpha_blend_worklist_surface_states_at(
    state: DirectRcsState,
    binding_table_offset: usize,
    src_surface_state_offset: usize,
    dst_surface_state_offset: usize,
    desc_surface_state_offset: usize,
    src_gpu: u64,
    src_bytes: usize,
    dst_gpu: u64,
    dst_bytes: usize,
    desc_gpu: u64,
    desc_bytes: usize,
) -> bool {
    let binding_end = binding_table_offset + 3 * core::mem::size_of::<u32>();
    let surface_bytes = COPY_RECT_SURFACE_STATE_DWORDS * core::mem::size_of::<u32>();
    let src_surface_end = src_surface_state_offset + surface_bytes;
    let dst_surface_end = dst_surface_state_offset + surface_bytes;
    let desc_surface_end = desc_surface_state_offset + surface_bytes;
    if binding_end > DIRECT_RCS_BATCH_BYTES
        || src_surface_end > DIRECT_RCS_BATCH_BYTES
        || dst_surface_end > DIRECT_RCS_BATCH_BYTES
        || desc_surface_end > DIRECT_RCS_BATCH_BYTES
    {
        return false;
    }

    unsafe {
        let binding = state.batch_virt.add(binding_table_offset) as *mut u32;
        core::ptr::write_volatile(binding, src_surface_state_offset as u32);
        core::ptr::write_volatile(binding.add(1), dst_surface_state_offset as u32);
        core::ptr::write_volatile(binding.add(2), desc_surface_state_offset as u32);
    }

    direct_rcs_write_buffer_surface_state(state, src_surface_state_offset, src_gpu, src_bytes)
        && direct_rcs_write_buffer_surface_state(
            state,
            dst_surface_state_offset,
            dst_gpu,
            dst_bytes,
        )
        && direct_rcs_write_buffer_surface_state(
            state,
            desc_surface_state_offset,
            desc_gpu,
            desc_bytes,
        )
}

fn direct_rcs_write_fill_rect_worklist_payload_at(
    state: DirectRcsState,
    payload_offset: usize,
    params: FillRectWorklistRgba8Params,
) -> bool {
    if payload_offset + RECT_WORKLIST_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        let payload = state.batch_virt.add(payload_offset);
        core::ptr::write_bytes(payload, 0, RECT_WORKLIST_INDIRECT_BYTES);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        core::ptr::write_volatile(dwords.add(8), params.dst_gpu as u32);
        core::ptr::write_volatile(dwords.add(9), (params.dst_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(10), params.desc_gpu as u32);
        core::ptr::write_volatile(dwords.add(11), (params.desc_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(12), params.dst_pitch_bytes);
        core::ptr::write_volatile(dwords.add(13), params.desc_base);
        core::ptr::write_volatile(dwords.add(14), params.desc_count);

        let local_ids = payload.add(FILL_RECT_WORKLIST_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_write_mandel64_worklist_payload_at(
    state: DirectRcsState,
    payload_offset: usize,
    params: Mandel64WorklistRgba8Params,
) -> bool {
    if payload_offset + RECT_WORKLIST_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        let payload = state.batch_virt.add(payload_offset);
        core::ptr::write_bytes(payload, 0, RECT_WORKLIST_INDIRECT_BYTES);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        core::ptr::write_volatile(dwords.add(12), params.dst_gpu as u32);
        core::ptr::write_volatile(dwords.add(13), (params.dst_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(14), params.desc_gpu as u32);
        core::ptr::write_volatile(dwords.add(15), (params.desc_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(16), params.dst_pitch_bytes);
        core::ptr::write_volatile(dwords.add(17), params.desc_base);
        core::ptr::write_volatile(dwords.add(18), params.desc_count);

        let local_ids = payload.add(RECT_WORKLIST_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_write_sprite_quad_worklist_payload_at(
    state: DirectRcsState,
    payload_offset: usize,
    params: SpriteQuadWorklistRgba8Params,
    global_x: u32,
    global_tile_y: u32,
) -> bool {
    if payload_offset + SPRITE_QUAD_WORKLIST_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        let payload = state.batch_virt.add(payload_offset);
        core::ptr::write_bytes(payload, 0, SPRITE_QUAD_WORKLIST_INDIRECT_BYTES);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords, global_x);
        core::ptr::write_volatile(dwords.add(1), global_tile_y);
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        core::ptr::write_volatile(dwords.add(12), params.src_gpu as u32);
        core::ptr::write_volatile(dwords.add(13), (params.src_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(14), params.dst_gpu as u32);
        core::ptr::write_volatile(dwords.add(15), (params.dst_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(16), params.desc_gpu as u32);
        core::ptr::write_volatile(dwords.add(17), (params.desc_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(18), params.src_pitch_bytes);
        core::ptr::write_volatile(dwords.add(19), params.dst_pitch_bytes);
        core::ptr::write_volatile(dwords.add(20), params.src_width);
        core::ptr::write_volatile(dwords.add(21), params.src_height);
        core::ptr::write_volatile(dwords.add(22), params.dst_width);
        core::ptr::write_volatile(dwords.add(23), params.dst_height);
        core::ptr::write_volatile(dwords.add(24), params.desc_base);
        core::ptr::write_volatile(dwords.add(25), params.desc_count);

        let local_ids = payload.add(SPRITE_QUAD_WORKLIST_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_write_fill_rect_interface_descriptor(state: DirectRcsState) -> bool {
    if CLEAR_RECT_IDD_OFFSET_BYTES + CLEAR_RECT_IDD_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    let idd = unsafe { state.batch_virt.add(CLEAR_RECT_IDD_OFFSET_BYTES) as *mut u32 };
    unsafe {
        core::ptr::write_volatile(idd, FILL_RECT_RGBA8_TEXT_OFFSET_BYTES as u32);
        core::ptr::write_volatile(idd.add(1), 0);
        core::ptr::write_volatile(idd.add(2), IDD_THREAD_PREEMPTION_DISABLE);
        core::ptr::write_volatile(idd.add(3), 0);
        core::ptr::write_volatile(idd.add(4), (CLEAR_RECT_BINDING_TABLE_OFFSET_BYTES as u32) | 1);
        core::ptr::write_volatile(idd.add(5), 3 << 16);
        core::ptr::write_volatile(idd.add(6), GPGPU_WALKER_GROUP_THREADS);
        core::ptr::write_volatile(idd.add(7), 3);
    }
    true
}

fn direct_rcs_write_clear_rect_surface_state(
    state: DirectRcsState,
    dst_gpu: u64,
    dst_bytes: usize,
) -> bool {
    let binding_end = CLEAR_RECT_BINDING_TABLE_OFFSET_BYTES + core::mem::size_of::<u32>();
    let surface_bytes = CLEAR_RECT_SURFACE_STATE_DWORDS * core::mem::size_of::<u32>();
    let surface_end = CLEAR_RECT_SURFACE_STATE_OFFSET_BYTES + surface_bytes;
    if binding_end > DIRECT_RCS_BATCH_BYTES || surface_end > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    if dst_bytes == 0 {
        return false;
    }

    let extent = dst_bytes.saturating_sub(1);
    let surface_width_minus1 = (extent & 0x7F) as u32;
    let surface_height_minus1 = ((extent >> 7) & 0x3FFF) as u32;
    let surface_depth_minus1 = ((extent >> 21) & 0x7FF) as u32;
    let surface_dword0 = (SURFTYPE_BUFFER << 29) | (SURFACE_FORMAT_RAW << 18);
    let surface_dword2 = (surface_height_minus1 << 16) | surface_width_minus1;
    let surface_dword3 = surface_depth_minus1 << 21;

    unsafe {
        let binding = state.batch_virt.add(CLEAR_RECT_BINDING_TABLE_OFFSET_BYTES) as *mut u32;
        core::ptr::write_volatile(binding, CLEAR_RECT_SURFACE_STATE_OFFSET_BYTES as u32);

        let surface = state.batch_virt.add(CLEAR_RECT_SURFACE_STATE_OFFSET_BYTES) as *mut u32;
        for index in 0..CLEAR_RECT_SURFACE_STATE_DWORDS {
            core::ptr::write_volatile(surface.add(index), 0);
        }
        core::ptr::write_volatile(surface, surface_dword0);
        core::ptr::write_volatile(surface.add(1), RENDER_MOCS << 24);
        core::ptr::write_volatile(surface.add(2), surface_dword2);
        core::ptr::write_volatile(surface.add(3), surface_dword3);
        core::ptr::write_volatile(surface.add(8), dst_gpu as u32);
        core::ptr::write_volatile(surface.add(9), (dst_gpu >> 32) as u32);
    }
    true
}

fn direct_rcs_write_fill_rect_payload(state: DirectRcsState, params: FillRectRgba8Params) -> bool {
    if CLEAR_RECT_PAYLOAD_OFFSET_BYTES + CLEAR_RECT_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        let payload = state.batch_virt.add(CLEAR_RECT_PAYLOAD_OFFSET_BYTES);
        core::ptr::write_bytes(payload, 0, CLEAR_RECT_INDIRECT_BYTES);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        core::ptr::write_volatile(dwords.add(12), params.dst_gpu as u32);
        core::ptr::write_volatile(dwords.add(13), (params.dst_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(14), params.dst_pitch_bytes);
        core::ptr::write_volatile(dwords.add(15), params.dst_x);
        core::ptr::write_volatile(dwords.add(16), params.dst_y);
        core::ptr::write_volatile(dwords.add(17), params.width);
        core::ptr::write_volatile(dwords.add(18), params.height);
        core::ptr::write_volatile(dwords.add(19), params.color_rgba);

        let local_ids = payload.add(CLEAR_RECT_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn pack_i16_pair_u32(x: i16, y: i16) -> u32 {
    (u16::from_ne_bytes(x.to_ne_bytes()) as u32)
        | ((u16::from_ne_bytes(y.to_ne_bytes()) as u32) << 16)
}

fn pack_u16_pair_u32(x: u16, y: u16) -> u32 {
    (x as u32) | ((y as u32) << 16)
}

fn direct_rcs_read_worklist_probe_span(
    state: DirectRcsState,
    row_index: usize,
    start_pixel: usize,
) -> [u32; 4] {
    super::dma_flush(state.clear_test_virt, CLEAR_RECT_TEST_BYTES);
    let mut values = [0u32; 4];
    unsafe {
        let surface = state.clear_test_virt as *const u32;
        let row = surface.add(row_index * 64);
        for (index, value) in values.iter_mut().enumerate() {
            *value = core::ptr::read_volatile(row.add(start_pixel + index));
        }
    }
    values
}

fn direct_rcs_push(batch: &mut [u32], cursor: &mut usize, value: u32) -> bool {
    if *cursor >= batch.len() {
        return false;
    }
    batch[*cursor] = value;
    *cursor += 1;
    true
}

fn direct_rcs_push_pipe_control_full(
    batch: &mut [u32],
    cursor: &mut usize,
    header_flags: u32,
    dw1_flags: u32,
) -> bool {
    direct_rcs_push(batch, cursor, PIPE_CONTROL_CMD | header_flags)
        && direct_rcs_push(batch, cursor, dw1_flags)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
}

fn direct_rcs_push_pipe_control(batch: &mut [u32], cursor: &mut usize, flags: u32) -> bool {
    // Every caller is a GPGPU cache flush/invalidate boundary.  Drain HDC/LSC
    // in DW0 as required before the DW1 cache operation can be considered a
    // producer/consumer fence across GuC contexts.
    direct_rcs_push_pipe_control_full(batch, cursor, PIPE_CONTROL_HDC_PIPELINE_FLUSH, flags)
}

fn direct_rcs_push_pipe_control_post_sync_marker_at(
    batch: &mut [u32],
    cursor: &mut usize,
    result_gpu: u64,
    slot: usize,
    value: u32,
) -> bool {
    // PIPE_CONTROL post-sync writes a QWord. Keep the destination naturally
    // aligned and reserve the following result slot for its high DWORD.
    if slot & 1 != 0 {
        return false;
    }
    let dst = result_gpu + (slot as u64) * core::mem::size_of::<u32>() as u64;
    direct_rcs_push(batch, cursor, PIPE_CONTROL_CMD)
        && direct_rcs_push(
            batch,
            cursor,
            PIPE_CONTROL_FLUSH_ENABLE
                | PIPE_CONTROL_CS_STALL
                | PIPE_CONTROL_POST_SYNC_WRITE_IMMEDIATE,
        )
        && direct_rcs_push(batch, cursor, dst as u32)
        && direct_rcs_push(batch, cursor, (dst >> 32) as u32)
        && direct_rcs_push(batch, cursor, value)
        && direct_rcs_push(batch, cursor, 0)
}

fn direct_rcs_push_store_marker(
    batch: &mut [u32],
    cursor: &mut usize,
    slot: usize,
    value: u32,
) -> bool {
    direct_rcs_push_store_marker_at(batch, cursor, DIRECT_RCS_GPU_VA_RESULT_BASE, slot, value)
}

fn direct_rcs_push_store_marker_at(
    batch: &mut [u32],
    cursor: &mut usize,
    result_gpu: u64,
    slot: usize,
    value: u32,
) -> bool {
    let dst = result_gpu + (slot as u64) * core::mem::size_of::<u32>() as u64;
    direct_rcs_push(batch, cursor, MI_STORE_DATA_IMM_GGTT_DW1)
        && direct_rcs_push(batch, cursor, dst as u32)
        && direct_rcs_push(batch, cursor, (dst >> 32) as u32)
        && direct_rcs_push(batch, cursor, value)
}

fn direct_rcs_push_gpgpu_walker_2d(
    batch: &mut [u32],
    cursor: &mut usize,
    payload_offset: usize,
    indirect_bytes: usize,
    group_x: u32,
    group_y: u32,
    right_mask: u32,
) -> bool {
    if group_x == 0 || group_y == 0 || right_mask == 0 {
        return false;
    }
    direct_rcs_push(batch, cursor, GPGPU_WALKER_CMD)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, indirect_bytes as u32)
        && direct_rcs_push(batch, cursor, payload_offset as u32)
        && direct_rcs_push(
            batch,
            cursor,
            (GPGPU_WALKER_SIMD16_SELECT << 30) | (GPGPU_WALKER_GROUP_THREADS - 1),
        )
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, group_x)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, group_y)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_GROUP_Z_DIM)
        && direct_rcs_push(batch, cursor, right_mask)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_BOTTOM_MASK)
}

fn direct_rcs_push_rect_worklist_walker(
    batch: &mut [u32],
    cursor: &mut usize,
    payload_offset: usize,
    group_x: u32,
    right_mask: u32,
) -> bool {
    if group_x == 0 || group_x as usize > RECT_WORKLIST_DESCS_PER_WALKER {
        return false;
    }
    direct_rcs_push(batch, cursor, GPGPU_WALKER_CMD)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, RECT_WORKLIST_INDIRECT_BYTES as u32)
        && direct_rcs_push(batch, cursor, payload_offset as u32)
        && direct_rcs_push(
            batch,
            cursor,
            (GPGPU_WALKER_SIMD16_SELECT << 30) | (GPGPU_WALKER_GROUP_THREADS - 1),
        )
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, group_x)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 1)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_GROUP_Z_DIM)
        && direct_rcs_push(batch, cursor, right_mask)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_BOTTOM_MASK)
}

fn direct_rcs_push_sprite_quad_worklist_walker(
    batch: &mut [u32],
    cursor: &mut usize,
    payload_offset: usize,
    group_x: u32,
    group_y: u32,
    right_mask: u32,
) -> bool {
    if group_x == 0 || group_y == 0 || group_x as usize > SPRITE_QUAD_WORKLIST_MAX_GROUPS_PER_WALKER
    {
        return false;
    }
    direct_rcs_push(batch, cursor, GPGPU_WALKER_CMD)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, SPRITE_QUAD_WORKLIST_INDIRECT_BYTES as u32)
        && direct_rcs_push(batch, cursor, payload_offset as u32)
        && direct_rcs_push(
            batch,
            cursor,
            (GPGPU_WALKER_SIMD16_SELECT << 30) | (GPGPU_WALKER_GROUP_THREADS - 1),
        )
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, group_x)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, group_y)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_GROUP_Z_DIM)
        && direct_rcs_push(batch, cursor, right_mask)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_BOTTOM_MASK)
}

fn direct_rcs_push_state_base_address(
    batch: &mut [u32],
    cursor: &mut usize,
    indirect_object_base: u64,
    dynamic_state_base: u64,
    instruction_base: u64,
) -> bool {
    direct_rcs_push(batch, cursor, STATE_BASE_ADDRESS_CMD)
        && direct_rcs_push_sba_address(batch, cursor, true, RENDER_MOCS, indirect_object_base)
        && direct_rcs_push(batch, cursor, RENDER_MOCS << 16)
        && direct_rcs_push_sba_address(batch, cursor, true, RENDER_MOCS, dynamic_state_base)
        && direct_rcs_push_sba_address(batch, cursor, true, RENDER_MOCS, dynamic_state_base)
        && direct_rcs_push_sba_address(batch, cursor, true, RENDER_MOCS, indirect_object_base)
        && direct_rcs_push_sba_address(batch, cursor, true, RENDER_MOCS, instruction_base)
        && direct_rcs_push_sba_size(batch, cursor, true, 0xFFFF_F000)
        && direct_rcs_push_sba_size(batch, cursor, true, 0xFFFF_F000)
        && direct_rcs_push_sba_size(batch, cursor, true, 0xFFFF_F000)
        && direct_rcs_push_sba_size(batch, cursor, true, 0xFFFF_F000)
        && direct_rcs_push_sba_address(batch, cursor, true, RENDER_MOCS, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push_sba_address(batch, cursor, true, RENDER_MOCS, 0)
        && direct_rcs_push(batch, cursor, 0)
}

fn direct_rcs_push_sba_address(
    batch: &mut [u32],
    cursor: &mut usize,
    enable: bool,
    mocs: u32,
    address: u64,
) -> bool {
    let low = ((address as u32) & 0xFFFF_F000) | (mocs << 4) | u32::from(enable);
    direct_rcs_push(batch, cursor, low) && direct_rcs_push(batch, cursor, (address >> 32) as u32)
}

fn direct_rcs_push_sba_size(
    batch: &mut [u32],
    cursor: &mut usize,
    enable: bool,
    size_bytes: usize,
) -> bool {
    let Some(size_bytes) = align_up(size_bytes, 4096) else {
        return false;
    };
    let Ok(size_bytes) = u32::try_from(size_bytes) else {
        return false;
    };
    direct_rcs_push(batch, cursor, (size_bytes & 0xFFFF_F000) | u32::from(enable))
}

fn direct_rcs_submit_batch(dev: super::Dev, state: DirectRcsState) -> bool {
    if DIRECT_RCS_CONTEXT_QUARANTINED.load(Ordering::Acquire) {
        return false;
    }
    let mut runtime = DIRECT_RCS_SUBMIT_RUNTIME.lock();
    direct_rcs_submit_batch_for(dev, state, &mut runtime, crate::gpu::vgpu::KernelClient::Gpgpu)
}

fn quarantine_direct_rcs_context(reason: &'static str) {
    if DIRECT_RCS_CONTEXT_QUARANTINED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        crate::log_error!(
            target: "gpgpu";
            "intel/gpgpu: direct-rcs context quarantined reason={} action=reject-future-direct-submits-until-reboot late-batch-reuse=forbidden\n",
            reason,
        );
    }
}

fn direct_rcs_submit_batch_for(
    dev: super::Dev,
    state: DirectRcsState,
    runtime: &mut DirectRcsSubmitRuntime,
    client: crate::gpu::vgpu::KernelClient,
) -> bool {
    if runtime.pending.is_some() {
        return false;
    }
    // The GuC owns one persistent logical context for the direct-RCS client.
    // Its ring must therefore be persistent as well: publishing the same tail
    // for every request does not describe new work once the first request has
    // advanced the saved ring head. Append one BBS entry and advance the tail
    // instead of rebuilding the registered context at offset zero.
    let old_tail_bytes = runtime.ring_tail_bytes;
    let ring_tail_bytes =
        direct_rcs_append_ring_batch_start(state, old_tail_bytes, state.gpu_va.batch);
    let Some(ring_ctl) = direct_rcs_ring_ctl_value(DIRECT_RCS_RING_BYTES) else {
        return false;
    };
    if !runtime.context_initialized {
        if !direct_rcs_init_lrc_context_image(
            state,
            state.gpu_va.ring as u32,
            ring_tail_bytes as u32,
            ring_ctl,
        ) {
            return false;
        }
        runtime.context_initialized = true;
    } else {
        direct_rcs_write_lrc_ring_tail(state, ring_tail_bytes as u32);
    }
    let (context_desc_lo, context_desc_hi) = guc_rcs_context_descriptor(state.gpu_va.context);
    super::ggtt_invalidate(dev);
    core::sync::atomic::fence(Ordering::SeqCst);
    let descriptor = crate::gpu::physical::PhysicalContextDescriptor {
        engine: crate::gpu::physical::EngineClass::RenderCompute,
        hwlrca_lo: context_desc_lo,
        hwlrca_hi: context_desc_hi,
        gpuvm_root_phys: state.ppgtt_phys,
    };
    match crate::gpu::executor::submit_kernel_context(client, descriptor) {
        Ok(submission) => {
            runtime.ring_tail_bytes = ring_tail_bytes;
            runtime.pending = Some(submission);
            true
        }
        Err(error) => {
            // The entry was not admitted. Keep the software tail at the last
            // accepted position so a retry cannot silently skip ring space.
            direct_rcs_write_lrc_ring_tail(state, old_tail_bytes as u32);
            crate::log!(
                "gpgpu/vgpu: submit failed error={:?} submission_owner=gpu-executor/vgpu/guc direct_elsp=0\n",
                error
            );
            false
        }
    }
}

fn complete_direct_rcs_submission(completed: bool) {
    let submission = DIRECT_RCS_SUBMIT_RUNTIME.lock().pending.take();
    if let Some(submission) = submission {
        let _ = crate::gpu::executor::complete_kernel_submission(submission, completed);
    }
}

fn direct_rcs_append_ring_batch_start(
    state: DirectRcsState,
    ring_tail_bytes: usize,
    batch_gpu_addr: u64,
) -> usize {
    debug_assert_eq!(ring_tail_bytes % (DIRECT_RCS_BATCH_START_DWORDS * 4), 0);
    debug_assert!(ring_tail_bytes < DIRECT_RCS_RING_BYTES);
    let start = ring_tail_bytes / core::mem::size_of::<u32>();
    unsafe {
        let dwords = state.ring_virt as *mut u32;
        core::ptr::write_volatile(dwords.add(start), MI_BATCH_BUFFER_START_GEN8 | MI_BATCH_GTT);
        core::ptr::write_volatile(dwords.add(start + 1), batch_gpu_addr as u32);
        core::ptr::write_volatile(dwords.add(start + 2), (batch_gpu_addr >> 32) as u32);
        core::ptr::write_volatile(dwords.add(start + 3), MI_NOOP);
    }
    let tail_bytes = (ring_tail_bytes
        + DIRECT_RCS_BATCH_START_DWORDS * core::mem::size_of::<u32>())
        % DIRECT_RCS_RING_BYTES;
    unsafe {
        super::dma_flush(
            state.ring_virt.add(ring_tail_bytes),
            DIRECT_RCS_BATCH_START_DWORDS * core::mem::size_of::<u32>(),
        );
    }
    tail_bytes
}

fn direct_rcs_poll_result_slot(state: DirectRcsState, slot: usize, expected: u32) -> u32 {
    let mut observed = 0;
    for _ in 0..DIRECT_RCS_SMOKE_POLL_ITERS {
        observed = direct_rcs_read_result_slot(state, slot);
        if observed == expected {
            break;
        }
        core::hint::spin_loop();
    }
    complete_direct_rcs_submission(observed == expected);
    observed
}

fn direct_rcs_poll_result_slot_timeout_ms(
    state: DirectRcsState,
    slot: usize,
    expected: u32,
    timeout_ms: u64,
) -> u32 {
    let started = direct_rcs_now_tick();
    let deadline = started.saturating_add(direct_rcs_ticks_from_ms(timeout_ms));
    let log_probe = DIRECT_RCS_TIMEOUT_POLL_PROBE_LOGGED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok();
    if log_probe {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: marker-poll begin slot={} expected=0x{:08X} timeout_ms={} completion_limit=deadline cache_flush_bytes=4 pause_iters={} worker_slot={}\n",
            slot,
            expected,
            timeout_ms,
            DIRECT_RCS_TIMEOUT_POLL_PAUSE_ITERS,
            crate::percpu::current_slot(),
        );
    }
    let mut iterations = 0usize;
    let observed = loop {
        iterations = iterations.saturating_add(1);
        let observed = direct_rcs_read_result_slot(state, slot);
        if observed == expected {
            break observed;
        }
        if direct_rcs_now_tick() >= deadline {
            break observed;
        }
        for _ in 0..DIRECT_RCS_TIMEOUT_POLL_PAUSE_ITERS {
            core::hint::spin_loop();
        }
    };
    if log_probe {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: marker-poll end slot={} observed=0x{:08X} expected=0x{:08X} matched={} iterations={} elapsed_ms={}\n",
            slot,
            observed,
            expected,
            (observed == expected) as u8,
            iterations,
            direct_rcs_elapsed_ms_since(started),
        );
    }
    complete_direct_rcs_submission(observed == expected);
    observed
}

fn direct_rcs_poll_result_slot_elapsed(
    state: DirectRcsState,
    slot: usize,
    expected: u32,
    start_tick: u64,
) -> (u32, u64) {
    let observed = direct_rcs_poll_result_slot(state, slot, expected);
    (observed, direct_rcs_elapsed_ms_since(start_tick))
}

fn direct_rcs_read_result_slot(state: DirectRcsState, slot: usize) -> u32 {
    let offset = slot.saturating_mul(core::mem::size_of::<u32>());
    if offset + core::mem::size_of::<u32>() > DIRECT_RCS_RESULT_BYTES {
        return 0;
    }
    let marker = unsafe { state.result_virt.add(offset) };
    // CLFLUSH rounds to a cache-line boundary. Invalidating this one marker is
    // sufficient; flushing the full 4 KiB result page on every poll multiplied
    // each check into 64 CLFLUSH operations plus an MFENCE and starved sibling
    // tasks on the same executor core.
    super::dma_flush(marker, core::mem::size_of::<u32>());
    unsafe { core::ptr::read_volatile(marker as *const u32) }
}

fn direct_rcs_now_tick() -> u64 {
    embassy_time_driver::now()
}

fn direct_rcs_ticks_from_ms(ms: u64) -> u64 {
    let hz = embassy_time_driver::TICK_HZ;
    if hz == 0 {
        return ms.max(1);
    }
    let ticks = ((ms as u128).saturating_mul(hz as u128).saturating_add(999) / 1000) as u64;
    if ms == 0 { 0 } else { ticks.max(1) }
}

fn direct_rcs_elapsed_ms_since(start_tick: u64) -> u64 {
    let elapsed = direct_rcs_now_tick().saturating_sub(start_tick);
    let hz = embassy_time_driver::TICK_HZ;
    if hz == 0 {
        0
    } else {
        elapsed.saturating_mul(1000) / hz
    }
}

fn direct_rcs_init_lrc_context_image(
    state: DirectRcsState,
    ring_start: u32,
    ring_tail: u32,
    ring_ctl: u32,
) -> bool {
    let total_dwords = DIRECT_RCS_CONTEXT_BYTES / core::mem::size_of::<u32>();
    let dwords =
        unsafe { core::slice::from_raw_parts_mut(state.context_virt as *mut u32, total_dwords) };
    dwords.fill(0);

    let lrc = &mut dwords[DIRECT_RCS_LRC_STATE_OFFSET_DWORDS..];
    if lrc.len() < 192 {
        return false;
    }

    lrc[0] = MI_NOOP;
    let mut idx = 1usize;

    lrc[idx] = direct_rcs_mi_lri_cmd(13, MI_LRI_FORCE_POSTED);
    idx += 1;
    lrc[idx] = 0x2244;
    lrc[idx + 1] = direct_rcs_ctx_control_value(false);
    lrc[idx + 2] = 0x2034;
    lrc[idx + 3] = 0;
    lrc[idx + 4] = 0x2030;
    lrc[idx + 5] = ring_tail;
    lrc[idx + 6] = 0x2038;
    lrc[idx + 7] = ring_start;
    lrc[idx + 8] = 0x203C;
    lrc[idx + 9] = ring_ctl;
    lrc[idx + 10] = 0x2168;
    lrc[idx + 11] = 0;
    lrc[idx + 12] = 0x2140;
    lrc[idx + 13] = 0;
    lrc[idx + 14] = 0x2110;
    lrc[idx + 15] = 0;
    lrc[idx + 16] = 0x211C;
    lrc[idx + 17] = 0;
    lrc[idx + 18] = 0x2114;
    lrc[idx + 19] = 0;
    lrc[idx + 20] = 0x2118;
    lrc[idx + 21] = 0;
    lrc[idx + 22] = 0x21C0;
    lrc[idx + 23] = 0;
    lrc[idx + 24] = 0x21C4;
    lrc[idx + 25] = 0;
    lrc[idx + 26] = 0x21C8;
    lrc[idx + 27] = 0;
    lrc[idx + 28] = 0x2180;
    lrc[idx + 29] = 0;
    idx += 30;

    direct_rcs_push_nops(lrc, &mut idx, 5);

    lrc[idx] = direct_rcs_mi_lri_cmd(9, MI_LRI_FORCE_POSTED);
    idx += 1;
    lrc[idx] = 0x23A8;
    lrc[idx + 1] = 0;
    lrc[idx + 2] = 0x228C;
    lrc[idx + 3] = 0;
    lrc[idx + 4] = 0x2288;
    lrc[idx + 5] = 0;
    lrc[idx + 6] = 0x2284;
    lrc[idx + 7] = 0;
    lrc[idx + 8] = 0x2280;
    lrc[idx + 9] = 0;
    lrc[idx + 10] = 0x227C;
    lrc[idx + 11] = 0;
    lrc[idx + 12] = 0x2278;
    lrc[idx + 13] = 0;
    lrc[idx + 14] = 0x2274;
    lrc[idx + 15] = (state.ppgtt_phys >> 32) as u32;
    lrc[idx + 16] = 0x2270;
    lrc[idx + 17] = state.ppgtt_phys as u32;
    idx += 18;

    lrc[idx] = direct_rcs_mi_lri_cmd(3, MI_LRI_FORCE_POSTED);
    idx += 1;
    lrc[idx] = 0x21B0;
    lrc[idx + 1] = 0;
    lrc[idx + 2] = 0x25A8;
    lrc[idx + 3] = 0;
    lrc[idx + 4] = 0x25AC;
    lrc[idx + 5] = 0;
    idx += 6;

    direct_rcs_push_nops(lrc, &mut idx, 6);

    lrc[idx] = direct_rcs_mi_lri_cmd(1, 0);
    idx += 1;
    lrc[idx] = 0x20C8;
    lrc[idx + 1] = 0x7FFF_FFFF;
    idx += 2;

    direct_rcs_push_nops(lrc, &mut idx, 13);

    lrc[idx] = direct_rcs_mi_lri_cmd(51, MI_LRI_FORCE_POSTED);
    idx += 1;
    lrc[idx] = 0x2588;
    lrc[idx + 1] = 0;
    lrc[idx + 2] = 0x2588;
    lrc[idx + 3] = 0;
    lrc[idx + 4] = 0x2588;
    lrc[idx + 5] = 0;
    lrc[idx + 6] = 0x2588;
    lrc[idx + 7] = 0;
    lrc[idx + 8] = 0x2588;
    lrc[idx + 9] = 0;
    lrc[idx + 10] = 0x2588;
    lrc[idx + 11] = 0;
    lrc[idx + 12] = 0x2028;
    lrc[idx + 13] = 0;
    lrc[idx + 14] = 0x209C;
    lrc[idx + 15] = direct_rcs_masked_bit_disable(RING_MI_MODE_STOP_RING);
    lrc[idx + 16] = 0x20C0;
    lrc[idx + 17] = 0;
    lrc[idx + 18] = 0x2178;
    lrc[idx + 19] = 0;
    lrc[idx + 20] = 0x217C;
    lrc[idx + 21] = 0;
    lrc[idx + 22] = 0x2358;
    lrc[idx + 23] = 0;
    lrc[idx + 24] = 0x2170;
    lrc[idx + 25] = 0;
    lrc[idx + 26] = 0x2150;
    lrc[idx + 27] = 0;
    lrc[idx + 28] = 0x2154;
    lrc[idx + 29] = 0;
    lrc[idx + 30] = 0x2158;
    lrc[idx + 31] = 0;
    lrc[idx + 32] = 0x241C;
    lrc[idx + 33] = 0;
    lrc[idx + 34] = 0x2600;
    lrc[idx + 35] = 0;
    lrc[idx + 36] = 0x2604;
    lrc[idx + 37] = 0;
    lrc[idx + 38] = 0x2608;
    lrc[idx + 39] = 0;
    lrc[idx + 40] = 0x260C;
    lrc[idx + 41] = 0;
    lrc[idx + 42] = 0x2610;
    lrc[idx + 43] = 0;
    lrc[idx + 44] = 0x2614;
    lrc[idx + 45] = 0;
    lrc[idx + 46] = 0x2618;
    lrc[idx + 47] = 0;
    lrc[idx + 48] = 0x261C;
    lrc[idx + 49] = 0;
    lrc[idx + 50] = 0x2620;
    lrc[idx + 51] = 0;
    lrc[idx + 52] = 0x2624;
    lrc[idx + 53] = 0;
    lrc[idx + 54] = 0x2628;
    lrc[idx + 55] = 0;
    lrc[idx + 56] = 0x262C;
    lrc[idx + 57] = 0;
    lrc[idx + 58] = 0x2630;
    lrc[idx + 59] = 0;
    lrc[idx + 60] = 0x2634;
    lrc[idx + 61] = 0;
    lrc[idx + 62] = 0x2638;
    lrc[idx + 63] = 0;
    lrc[idx + 64] = 0x263C;
    lrc[idx + 65] = 0;
    lrc[idx + 66] = 0x2640;
    lrc[idx + 67] = 0;
    lrc[idx + 68] = 0x2644;
    lrc[idx + 69] = 0;
    lrc[idx + 70] = 0x2648;
    lrc[idx + 71] = 0;
    lrc[idx + 72] = 0x264C;
    lrc[idx + 73] = 0;
    lrc[idx + 74] = 0x2650;
    lrc[idx + 75] = 0;
    lrc[idx + 76] = 0x2654;
    lrc[idx + 77] = 0;
    lrc[idx + 78] = 0x2658;
    lrc[idx + 79] = 0;
    lrc[idx + 80] = 0x265C;
    lrc[idx + 81] = 0;
    lrc[idx + 82] = 0x2660;
    lrc[idx + 83] = 0;
    lrc[idx + 84] = 0x2664;
    lrc[idx + 85] = 0;
    lrc[idx + 86] = 0x2668;
    lrc[idx + 87] = 0;
    lrc[idx + 88] = 0x266C;
    lrc[idx + 89] = 0;
    lrc[idx + 90] = 0x2670;
    lrc[idx + 91] = 0;
    lrc[idx + 92] = 0x2674;
    lrc[idx + 93] = 0;
    lrc[idx + 94] = 0x2678;
    lrc[idx + 95] = 0;
    lrc[idx + 96] = 0x267C;
    lrc[idx + 97] = 0;
    lrc[idx + 98] = 0x2068;
    lrc[idx + 99] = 0;
    lrc[idx + 100] = 0x2084;
    lrc[idx + 101] = 0;
    idx += 102;

    lrc[idx] = MI_NOOP;
    idx += 1;
    lrc[idx] = MI_BATCH_BUFFER_END | 1;

    super::dma_flush(state.context_virt, DIRECT_RCS_CONTEXT_BYTES);
    true
}

fn direct_rcs_write_lrc_ring_tail(state: DirectRcsState, ring_tail: u32) {
    const LRC_CONTEXT_CONTROL_VALUE_DW: usize = 3;
    const LRC_RING_TAIL_VALUE_DW: usize = 7;

    let total_dwords = DIRECT_RCS_CONTEXT_BYTES / core::mem::size_of::<u32>();
    if total_dwords <= DIRECT_RCS_LRC_STATE_OFFSET_DWORDS + LRC_RING_TAIL_VALUE_DW {
        return;
    }
    let dwords =
        unsafe { core::slice::from_raw_parts_mut(state.context_virt as *mut u32, total_dwords) };
    let ctx_ctl = dwords[DIRECT_RCS_LRC_STATE_OFFSET_DWORDS + LRC_CONTEXT_CONTROL_VALUE_DW];
    dwords[DIRECT_RCS_LRC_STATE_OFFSET_DWORDS + LRC_RING_TAIL_VALUE_DW] = ring_tail;
    dwords[DIRECT_RCS_LRC_STATE_OFFSET_DWORDS + LRC_CONTEXT_CONTROL_VALUE_DW] = ctx_ctl;
    super::dma_flush(state.context_virt, DIRECT_RCS_CONTEXT_BYTES);
}

fn guc_rcs_context_descriptor(context_gpu_addr: u64) -> (u32, u32) {
    let base = (context_gpu_addr as u32) & 0xFFFF_F000;
    let descriptor = base
        | GEN8_CTX_VALID
        | GEN8_CTX_PRIVILEGE
        | (INTEL_LEGACY_64B_CONTEXT << GEN8_CTX_ADDRESSING_MODE_SHIFT);
    (descriptor, (context_gpu_addr >> 32) as u32)
}

fn direct_rcs_ring_ctl_value(size: usize) -> Option<u32> {
    let size = u32::try_from(size).ok()?;
    Some(size.checked_sub(4096)? | RING_VALID)
}

fn direct_rcs_ctx_control_value(inhibit_restore: bool) -> u32 {
    let mut ctl = direct_rcs_masked_bits_update(
        CTX_CTRL_INHIBIT_SYN_CTX_SWITCH,
        CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT,
    );
    if inhibit_restore {
        ctl |= CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT;
    }
    ctl
}

fn direct_rcs_wait_eq(dev: super::Dev, reg: usize, mask: u32, want: u32, n: usize) -> bool {
    for _ in 0..n {
        if (super::mmio_read(dev, reg) & mask) == want {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

fn direct_rcs_mi_lri_cmd(num_regs: u32, flags: u32) -> u32 {
    MI_LOAD_REGISTER_IMM | MI_LRI_CS_MMIO | flags | num_regs.saturating_mul(2).saturating_sub(1)
}

fn direct_rcs_push_nops(state: &mut [u32], idx: &mut usize, count: usize) {
    for _ in 0..count {
        state[*idx] = MI_NOOP;
        *idx += 1;
    }
}

fn direct_rcs_masked_bit_enable(bit: u32) -> u32 {
    bit | (bit << 16)
}

fn direct_rcs_masked_bit_disable(bit: u32) -> u32 {
    bit << 16
}

fn direct_rcs_masked_bits_update(set_bits: u32, clear_bits: u32) -> u32 {
    let update = set_bits | clear_bits;
    set_bits | (update << 16)
}

fn align_up(value: usize, align: usize) -> Option<usize> {
    let mask = align.checked_sub(1)?;
    value.checked_add(mask).map(|v| v & !mask)
}

// These signatures are still named by dead callers outside this cleanup's file scope.
// Keep inert compatibility definitions so pruning this module does not break the crate.
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuSprite64Placement;

pub(crate) fn present_rgba8_to_primary_xrgb_rect_stats(
    _src: GpgpuRgba8Surface,
    _src_rect: GpgpuRect,
    _dst: GpgpuRgba8Surface,
    _dst_xy: GpgpuPoint,
    _flip_y: bool,
) -> GpgpuSubmitStats {
    let stats = GpgpuSubmitStats::default();
    let _ = (stats.spans, stats.total_ms);
    stats
}

pub(crate) fn present_rgba8_rect_to_primary_xrgb_stats_with_flip(
    _src: GpgpuRgba8Surface,
    _src_rect: GpgpuRect,
    _dst_xy: GpgpuPoint,
    _flip_y: bool,
) -> Option<GpgpuSubmitStats> {
    None
}

pub(crate) fn present_rgba_frame_to_primary(_src: &[u8], _width: u32, _height: u32) -> bool {
    false
}

// The compatibility definitions above are intentionally retained until their dead callers
// can be removed in a broader cleanup.
const _: Option<GpgpuSprite64Placement> = Some(GpgpuSprite64Placement);
const _: fn(GpgpuRgba8Surface, GpgpuRect, GpgpuRgba8Surface, GpgpuPoint, bool) -> GpgpuSubmitStats =
    present_rgba8_to_primary_xrgb_rect_stats;
const _: fn(GpgpuRgba8Surface, GpgpuRect, GpgpuPoint, bool) -> Option<GpgpuSubmitStats> =
    present_rgba8_rect_to_primary_xrgb_stats_with_flip;
const _: fn(&[u8], u32, u32) -> bool = present_rgba_frame_to_primary;
