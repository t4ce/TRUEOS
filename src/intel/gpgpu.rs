use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use spin::{Mutex, Once};
mod test_gpgpu;

pub(crate) use test_gpgpu::{
    GpgpuCanvas3dUi2TextureFrame, GpgpuShellCube20ProjectResult, shell_cube20_project_spin,
    submit_canvas3d_clip_box_q16_once, submit_canvas3d_plane_fill_rgba8_once,
    submit_canvas3d_plane_patch_fill_cut_rgba8_once,
    submit_canvas3d_plane_patch_worklist_rgba8_once, submit_canvas3d_plane_sample_rgba8_once,
    submit_canvas3d_project_once, submit_canvas3d_transform_smoke_once,
    ui2_canvas3d_archaeology_project_frame, ui2_canvas3d_archaeology_project_frame_in_rect,
    ui2_canvas3d_archaeology_project_texture_frame, ui2_canvas3d_plane_patch_render_surface_frame,
    ui2_canvas3d_plane_patch_texture_frame,
};

pub(crate) const COPY_RECT_RGBA8_KERNEL_NAME: &str = "copy_rect_rgba8";
pub(crate) const COPY_RECT_RGBA8_OPENCL_SOURCE: &str = include_str!("kernels/copy_rect_rgba8.cl");
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
pub(crate) const BLIT_RGBA8_NEAREST_KERNEL_NAME: &str = "blit_rgba8_nearest";
pub(crate) const BLIT_RGBA8_NEAREST_OPENCL_SOURCE: &str =
    include_str!("kernels/blit_rgba8_nearest.cl");
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
pub(crate) const STAMP_MANDEL_RGBA8_KERNEL_NAME: &str = "stamp_mandel_rgba8";
pub(crate) const STAMP_MANDEL_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/stamp_mandel_rgba8.cl");
pub(crate) const SPRITE64_WORKLIST_RGBA8_KERNEL_NAME: &str = "sprite64_worklist_rgba8";
pub(crate) const SPRITE64_WORKLIST_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/sprite64_worklist_rgba8.cl");
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
pub(crate) const COPY_RECT_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/copy_rect_rgba8.bin");
pub(crate) const COPY_RECT_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/copy_rect_rgba8.spv");
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
pub(crate) const FILL_CIRCLE_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/fill_circle_rgba8.bin");
pub(crate) const FILL_CIRCLE_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/fill_circle_rgba8.spv");
pub(crate) const BLIT_RGBA8_NEAREST_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/blit_rgba8_nearest.bin");
pub(crate) const BLIT_RGBA8_NEAREST_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/blit_rgba8_nearest.spv");
pub(crate) const ALPHA_BLEND_RGBA8_OVER_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/alpha_blend_rgba8_over.bin");
pub(crate) const ALPHA_BLEND_RGBA8_OVER_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/alpha_blend_rgba8_over.spv");
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
pub(crate) const STAMP_MANDEL_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/stamp_mandel_rgba8.bin");
pub(crate) const STAMP_MANDEL_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/stamp_mandel_rgba8.spv");
pub(crate) const SPRITE64_WORKLIST_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/sprite64_worklist_rgba8.bin");
pub(crate) const SPRITE64_WORKLIST_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/sprite64_worklist_rgba8.spv");
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
pub(crate) const COPY_RECT_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0x10, 0x86, 0x60, 0x24, 0xAA, 0xFF, 0xAE, 0x96, 0xF9, 0x2C, 0xFC, 0x25, 0xA5, 0xFB, 0x18, 0x8C,
    0xA4, 0x21, 0x99, 0x47, 0x89, 0xAF, 0xBC, 0x4D, 0xBA, 0x3D, 0xDC, 0x29, 0x0B, 0xD5, 0x83, 0xAB,
];
pub(crate) const FILL_RECT_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0xAB, 0x51, 0x9A, 0x0E, 0x4E, 0x47, 0x31, 0xE5, 0x8F, 0xF6, 0x5D, 0x75, 0xBF, 0x92, 0x93, 0x4C,
    0xD7, 0x31, 0xA0, 0x88, 0x23, 0xB0, 0x40, 0x28, 0x62, 0x0E, 0x86, 0x54, 0x9F, 0x45, 0x06, 0xF4,
];
pub(crate) const FILL_RECT_WORKLIST_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0x30, 0x08, 0x5C, 0x66, 0x74, 0x13, 0xC0, 0x8E, 0xD8, 0x12, 0x82, 0x66, 0x6F, 0xB4, 0xEF, 0x47,
    0xFC, 0x07, 0x6F, 0x3C, 0xC0, 0xC2, 0x6C, 0x31, 0x5B, 0x71, 0xB2, 0xA8, 0xE2, 0x70, 0x0A, 0x78,
];
pub(crate) const GRADIENT_RECT_WORKLIST_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0xC0, 0x3A, 0xEE, 0xFC, 0x4D, 0x20, 0x23, 0xD5, 0xEE, 0x70, 0x3C, 0x5D, 0xBB, 0xB3, 0x1E, 0xBC,
    0x20, 0x93, 0xB1, 0x04, 0xBE, 0x00, 0xDB, 0x2B, 0xC7, 0x8D, 0x29, 0xC5, 0x30, 0xF4, 0x27, 0x37,
];
pub(crate) const FILL_CIRCLE_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0xB4, 0x17, 0x29, 0x7F, 0xD1, 0x60, 0x57, 0x21, 0xCC, 0x94, 0xA9, 0x88, 0x09, 0x3D, 0x8B, 0xA9,
    0xD1, 0x21, 0x9E, 0x0E, 0x3E, 0x8F, 0x4B, 0x67, 0xCC, 0xE2, 0x8D, 0x6D, 0x33, 0xE1, 0x5E, 0x3B,
];
pub(crate) const BLIT_RGBA8_NEAREST_ADLS_BIN_SHA256: [u8; 32] = [
    0x55, 0xF9, 0x41, 0xC8, 0x53, 0x71, 0xC0, 0xD1, 0xDF, 0x59, 0x89, 0x50, 0x01, 0xE6, 0x41, 0x23,
    0x36, 0xA8, 0x7D, 0x65, 0xAD, 0x14, 0xD2, 0x7D, 0xB6, 0x4A, 0x31, 0xB3, 0x81, 0x36, 0x02, 0xE3,
];
pub(crate) const ALPHA_BLEND_RGBA8_OVER_ADLS_BIN_SHA256: [u8; 32] = [
    0xDF, 0x30, 0x63, 0xCB, 0x0F, 0x72, 0x78, 0xD9, 0x6D, 0x15, 0xB4, 0xC4, 0x14, 0xC7, 0x9B, 0xA1,
    0x77, 0x15, 0xF4, 0xA7, 0xA3, 0x68, 0xA8, 0x9F, 0xD1, 0x13, 0x87, 0xFB, 0x54, 0x0F, 0x48, 0x1C,
];
pub(crate) const ALPHA_BLEND_WORKLIST_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0x63, 0x6B, 0xD6, 0xDD, 0x2D, 0xDE, 0x9E, 0x18, 0x4D, 0x26, 0xC1, 0x85, 0xEA, 0x04, 0xF6, 0x69,
    0x24, 0x76, 0xC1, 0xDE, 0xC2, 0xC5, 0xFA, 0x26, 0xBF, 0x5F, 0x5B, 0x67, 0x0C, 0xC1, 0xEB, 0x7E,
];
pub(crate) const GLYPH_MASK_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0x90, 0x8D, 0xF0, 0x7D, 0x62, 0xB0, 0x69, 0xF3, 0x1A, 0x04, 0x6D, 0x29, 0x02, 0xDF, 0xF9, 0xA0,
    0xFA, 0x33, 0xE4, 0x9A, 0x1C, 0x25, 0x3B, 0x74, 0xA4, 0xE7, 0xCC, 0x18, 0xDF, 0x66, 0xD3, 0x78,
];
pub(crate) const PRESENT_RGBA8_TO_PRIMARY_XRGB_RECT_ADLS_BIN_SHA256: [u8; 32] = [
    0x74, 0x85, 0xD6, 0x84, 0x1E, 0xAD, 0xBF, 0xA6, 0x2D, 0x40, 0x96, 0x24, 0xC2, 0x23, 0x6E, 0x4A,
    0x7D, 0x27, 0x43, 0x3C, 0xC8, 0xA2, 0xF0, 0x1A, 0x36, 0xFB, 0x6C, 0x75, 0xF0, 0x74, 0x7E, 0x1F,
];
pub(crate) const STAMP_MANDEL_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0x1E, 0x6F, 0xB6, 0xC8, 0x84, 0xCC, 0xB4, 0x19, 0x62, 0x32, 0x48, 0x1F, 0xC0, 0x95, 0xEC, 0xB5,
    0xC9, 0xCC, 0x95, 0xF7, 0x3D, 0xD6, 0x7B, 0x93, 0xF0, 0xC5, 0x9D, 0xEA, 0xC7, 0xAF, 0xF1, 0xE1,
];
pub(crate) const SPRITE64_WORKLIST_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0xF2, 0x43, 0x59, 0x84, 0x03, 0x79, 0x48, 0x82, 0x10, 0x4E, 0x77, 0xEB, 0x0F, 0x3E, 0x14, 0xB4,
    0xF1, 0x2C, 0x10, 0xBF, 0x8B, 0xD0, 0xB6, 0x8A, 0xB5, 0xFD, 0xED, 0x63, 0x04, 0x86, 0x87, 0x67,
];
pub(crate) const MANDEL64_WORKLIST_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0xD3, 0xB2, 0x38, 0x15, 0x2C, 0x11, 0x0D, 0x87, 0x5F, 0x69, 0x0F, 0xE8, 0x5B, 0xE5, 0xA6, 0xA4,
    0x6F, 0xA7, 0xDE, 0x0C, 0x18, 0x29, 0x6A, 0x65, 0xC0, 0xD0, 0x6C, 0x3B, 0x20, 0x0C, 0x51, 0x05,
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
    0x66, 0x12, 0x74, 0xD0, 0xE2, 0xC7, 0x1D, 0x37, 0x53, 0xDE, 0xD2, 0x7A, 0xE9, 0x23, 0x8C, 0xB7,
    0x26, 0x96, 0xC6, 0x6B, 0x99, 0xB6, 0xD3, 0x41, 0x66, 0x7F, 0x0C, 0x39, 0x3E, 0x11, 0xC4, 0xB1,
];

const COPY_RECT_RGBA8_ADLS_GPU: u64 = 0x0D20_0000;
const BLIT_RGBA8_NEAREST_ADLS_GPU: u64 = 0x0D21_0000;
const SPRITE64_WORKLIST_RGBA8_ADLS_GPU: u64 = 0x0D24_0000;
const MANDEL64_WORKLIST_RGBA8_ADLS_GPU: u64 = 0x0D36_0000;
const FILL_RECT_RGBA8_ADLS_GPU: u64 = 0x0D2B_0000;
const ALPHA_BLEND_RGBA8_OVER_ADLS_GPU: u64 = 0x0D2C_0000;
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
const COPY_RECT_RGBA8_TEXT_OFFSET_BYTES: u64 = 0x40;
const BLIT_RGBA8_NEAREST_TEXT_OFFSET_BYTES: u64 = 0x40;
const FILL_RECT_RGBA8_TEXT_OFFSET_BYTES: u64 = 0x40;
const FILL_RECT_WORKLIST_RGBA8_TEXT_OFFSET_BYTES: u64 = 0x40;
const GRADIENT_RECT_WORKLIST_RGBA8_TEXT_OFFSET_BYTES: u64 = 0x40;
const SPRITE64_WORKLIST_RGBA8_TEXT_OFFSET_BYTES: u64 = 0x40;
const MANDEL64_WORKLIST_RGBA8_TEXT_OFFSET_BYTES: u64 = 0x40;
const ALPHA_BLEND_RGBA8_OVER_TEXT_OFFSET_BYTES: u64 = 0x40;
const ALPHA_BLEND_WORKLIST_RGBA8_TEXT_OFFSET_BYTES: u64 = 0x40;
const GLYPH_MASK_RGBA8_TEXT_OFFSET_BYTES: u64 = 0x40;
const PRESENT_RGBA8_TO_PRIMARY_XRGB_RECT_TEXT_OFFSET_BYTES: u64 = 0x40;
const CANVAS3D_PROJECT_RGBA8_TEXT_OFFSET_BYTES: u64 = 0x40;
const CANVAS3D_TRANSFORM_Q16_TEXT_OFFSET_BYTES: u64 = 0x40;
const CANVAS3D_CLIP_BOX_Q16_TEXT_OFFSET_BYTES: u64 = 0x40;
const CANVAS3D_PLANE_SAMPLE_RGBA8_TEXT_OFFSET_BYTES: u64 = 0x40;
const CANVAS3D_PLANE_FILL_RGBA8_TEXT_OFFSET_BYTES: u64 = 0x40;
const CANVAS3D_PLANE_PATCH_FILL_CUT_RGBA8_TEXT_OFFSET_BYTES: u64 = 0x40;
const CANVAS3D_PLANE_PATCH_WORKLIST_RGBA8_TEXT_OFFSET_BYTES: u64 = 0x40;

const RCS_RING_BASE: usize = 0x0000_2000;
const RCS_RING_TAIL: usize = RCS_RING_BASE + 0x30;
const RCS_RING_HEAD: usize = RCS_RING_BASE + 0x34;
const RCS_RING_ACTHD: usize = RCS_RING_BASE + 0x74;
const RCS_RING_IPEIR: usize = RCS_RING_BASE + 0x64;
const RCS_RING_IPEHR: usize = RCS_RING_BASE + 0x68;
const RCS_RING_EIR: usize = RCS_RING_BASE + 0xB0;
const RCS_RING_MI_MODE: usize = RCS_RING_BASE + 0x9C;
const RCS_RING_MODE_GEN7: usize = RCS_RING_BASE + 0x29C;
const RCS_RING_CONTEXT_CONTROL: usize = RCS_RING_BASE + 0x244;
const RCS_RING_CONTEXT_CONTROL_REF: usize = RCS_RING_BASE + 0x5A0;
const RCS_RING_EXECLIST_CONTROL: usize = RCS_RING_BASE + 0x550;
const RCS_RING_EXECLIST_SQ_LO: usize = RCS_RING_BASE + 0x510;
const RCS_RING_EXECLIST_SQ_HI: usize = RCS_RING_BASE + 0x514;
const RCS_RING_HWS_PGA: usize = RCS_RING_BASE + 0x80;
const RCS_CS_DEBUG_MODE1: usize = RCS_RING_BASE + 0xEC;
const GEN12_RCU_MODE: usize = 0x14800;
const GEN12_RCU_MODE_CCS_ENABLE: u32 = 1 << 0;
const FORCEWAKE_RENDER: usize = 0x0A278;
const FORCEWAKE_GT: usize = 0x0A188;
const FORCEWAKE_ACK_RENDER: usize = 0x0D84;
const FORCEWAKE_ACK_GT: usize = 0x130044;
const FORCEWAKE_KERNEL: u32 = 1 << 0;
const FORCEWAKE_FALLBACK: u32 = 1 << 15;
const FORCEWAKE_POLL_ITERS: usize = 20_000;
const FF_DOP_CLOCK_GATE_DISABLE: u32 = 1 << 1;
const RING_VALID: u32 = 1;
const EL_CTRL_LOAD: u32 = 1 << 0;
const CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT: u32 = 1 << 0;
const CTX_CTRL_INHIBIT_SYN_CTX_SWITCH: u32 = 1 << 3;
const CTX_DESC_FORCE_RESTORE: u32 = 1 << 2;
const GEN11_GFX_DISABLE_LEGACY_MODE: u32 = 1 << 3;
const GFX_RUN_LIST_ENABLE: u32 = 1 << 15;
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
const GEN8_CTX_PPGTT_ENABLE: u32 = 1 << 5;
const GEN8_CTX_PRIVILEGE: u32 = 1 << 8;
const GEN12_CTX_PRIORITY_NORMAL: u32 = 1 << 9;
const GEN8_CTX_ADDRESSING_MODE_SHIFT: u32 = 3;
const RENDER_MOCS: u32 = 4;
const PIPE_CONTROL_CMD: u32 = 4 | (2 << 24) | (3 << 27) | (3 << 29);
const STATE_BASE_ADDRESS_CMD: u32 = 20 | (1 << 16) | (1 << 24) | (3 << 29);
const PIPE_CONTROL_DC_FLUSH_ENABLE: u32 = 1 << 5;
const PIPE_CONTROL_FLUSH_ENABLE: u32 = 1 << 7;
const PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH: u32 = 1 << 12;
const PIPE_CONTROL_CS_STALL: u32 = 1 << 20;
const PIPE_CONTROL_FLUSH_HDC: u32 = 1 << 26;
const PIPE_CONTROL_FLUSH_BITS: u32 = PIPE_CONTROL_DC_FLUSH_ENABLE
    | PIPE_CONTROL_FLUSH_ENABLE
    | PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH
    | PIPE_CONTROL_CS_STALL
    | PIPE_CONTROL_FLUSH_HDC;
const PIPE_CONTROL_INVALIDATE_BITS: u32 =
    PIPE_CONTROL_FLUSH_BITS | (1 << 8) | (1 << 10) | (1 << 11) | (1 << 13);
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
const GPGPU_WALKER_GROUP_Z_DIM: u32 = 1;
const GPGPU_WALKER_SIMD16_MASK: u32 = 0x0000_FFFF;
const GPGPU_WALKER_BOTTOM_MASK: u32 = 0xFFFF_FFFF;
const COPY_RECT_IDD_OFFSET_BYTES: usize = 0x300;
const COPY_RECT_BINDING_TABLE_OFFSET_BYTES: usize = 0x340;
const COPY_RECT_SRC_SURFACE_STATE_OFFSET_BYTES: usize = 0x380;
const COPY_RECT_DST_SURFACE_STATE_OFFSET_BYTES: usize = 0x3C0;
const COPY_RECT_PAYLOAD_OFFSET_BYTES: usize = 0x500;
const COPY_RECT_BATCH_IDD_OFFSET_BYTES: usize = 0x1000;
const COPY_RECT_BATCH_BINDING_TABLE_OFFSET_BYTES: usize = 0x1040;
const COPY_RECT_BATCH_SRC_SURFACE_STATE_OFFSET_BYTES: usize = 0x1080;
const COPY_RECT_BATCH_DST_SURFACE_STATE_OFFSET_BYTES: usize = 0x10C0;
const COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES: usize = 0x1200;
const COPY_RECT_PIXELS_PER_LANE: u32 = 2;
const COPY_RECT_SPAN_PIXELS: u32 = 16 * COPY_RECT_PIXELS_PER_LANE;
const COPY_RECT_BATCH_MAX_SPANS: usize = 32;
const COPY_RECT_IDD_BYTES: usize = 8 * core::mem::size_of::<u32>();
const COPY_RECT_SURFACE_STATE_DWORDS: usize = 16;
const COPY_RECT_CROSS_THREAD_BYTES: usize = 96;
const COPY_RECT_PER_THREAD_BYTES: usize = 96;
const COPY_RECT_INDIRECT_BYTES: usize = COPY_RECT_CROSS_THREAD_BYTES + COPY_RECT_PER_THREAD_BYTES;
const GLYPH_MASK_CROSS_THREAD_BYTES: usize = 128;
const GLYPH_MASK_PER_THREAD_BYTES: usize = 96;
const GLYPH_MASK_INDIRECT_BYTES: usize =
    GLYPH_MASK_CROSS_THREAD_BYTES + GLYPH_MASK_PER_THREAD_BYTES;
const PRESENT_RGBA8_TO_PRIMARY_XRGB_CROSS_THREAD_BYTES: usize = 128;
const PRESENT_RGBA8_TO_PRIMARY_XRGB_PER_THREAD_BYTES: usize = 96;
const PRESENT_RGBA8_TO_PRIMARY_XRGB_INDIRECT_BYTES: usize =
    PRESENT_RGBA8_TO_PRIMARY_XRGB_CROSS_THREAD_BYTES
        + PRESENT_RGBA8_TO_PRIMARY_XRGB_PER_THREAD_BYTES;
const SPRITE64_WORKLIST_IDD_OFFSET_BYTES: usize = 0x1000;
const SPRITE64_WORKLIST_BINDING_TABLE_OFFSET_BYTES: usize = 0x1040;
const SPRITE64_WORKLIST_ATLAS_SURFACE_STATE_OFFSET_BYTES: usize = 0x1080;
const SPRITE64_WORKLIST_DST_SURFACE_STATE_OFFSET_BYTES: usize = 0x10C0;
const SPRITE64_WORKLIST_DESC_SURFACE_STATE_OFFSET_BYTES: usize = 0x1100;
const SPRITE64_WORKLIST_PAYLOAD_OFFSET_BYTES: usize = 0x1200;
const SPRITE64_WORKLIST_IDD_BYTES: usize = 8 * core::mem::size_of::<u32>();
const SPRITE64_WORKLIST_CROSS_THREAD_BYTES: usize = 96;
const SPRITE64_WORKLIST_PER_THREAD_BYTES: usize = 96;
const SPRITE64_WORKLIST_INDIRECT_BYTES: usize =
    SPRITE64_WORKLIST_CROSS_THREAD_BYTES + SPRITE64_WORKLIST_PER_THREAD_BYTES;
const SPRITE64_WORKLIST_PRE_MARKER_SLOT: usize = 7;
const SPRITE64_WORKLIST_POST_MARKER_SLOT: usize = 6;
const SPRITE64_WORKLIST_PRE_MARKER: u32 = 0xC0DE_5701;
const SPRITE64_WORKLIST_POST_MARKER: u32 = 0xC0DE_5702;
const SPRITE64_WORKLIST_ATLAS_GPU: u64 = 0x0400_0000;
const SPRITE64_WORKLIST_DESC_GPU: u64 = 0x0580_0000;
const SPRITE64_WORKLIST_CELL_PIXELS: u32 = 64;
const SPRITE64_WORKLIST_ATLAS_COLUMNS: u32 = 16;
const SPRITE64_WORKLIST_MAX_DESCS: usize = 256;
const SPRITE64_WORKLIST_DESCS_PER_WALKER: usize = 16;
const SPRITE64_WORKLIST_MAX_WALKERS: usize =
    SPRITE64_WORKLIST_MAX_DESCS / SPRITE64_WORKLIST_DESCS_PER_WALKER;
const SPRITE64_WORKLIST_DESC_BYTES: usize =
    SPRITE64_WORKLIST_MAX_DESCS * core::mem::size_of::<Sprite64WorklistRgba8Desc>();
const SPRITE64_LUCIDA_THIRD_BUCKET_PNGS: [&[u8];
    crate::gfx::althlasfont::athlasmetrics::ATHLAS_BUCKET_COUNT] = [
    include_bytes!("../gfx/althlasfont/lucida-third/atlas-g00.png"),
    include_bytes!("../gfx/althlasfont/lucida-third/atlas-g01.png"),
    include_bytes!("../gfx/althlasfont/lucida-third/atlas-g02.png"),
    include_bytes!("../gfx/althlasfont/lucida-third/atlas-g03.png"),
    include_bytes!("../gfx/althlasfont/lucida-third/atlas-g04.png"),
    include_bytes!("../gfx/althlasfont/lucida-third/atlas-g05.png"),
    include_bytes!("../gfx/althlasfont/lucida-third/atlas-g06.png"),
    include_bytes!("../gfx/althlasfont/lucida-third/atlas-g07.png"),
];
const SPRITE64_LUCIDA_HALF_BUCKET_PNGS: [&[u8];
    crate::gfx::althlasfont::athlasmetrics::ATHLAS_BUCKET_COUNT] = [
    include_bytes!("../gfx/althlasfont/lucida-half/atlas-g00.png"),
    include_bytes!("../gfx/althlasfont/lucida-half/atlas-g01.png"),
    include_bytes!("../gfx/althlasfont/lucida-half/atlas-g02.png"),
    include_bytes!("../gfx/althlasfont/lucida-half/atlas-g03.png"),
    include_bytes!("../gfx/althlasfont/lucida-half/atlas-g04.png"),
    include_bytes!("../gfx/althlasfont/lucida-half/atlas-g05.png"),
    include_bytes!("../gfx/althlasfont/lucida-half/atlas-g06.png"),
    include_bytes!("../gfx/althlasfont/lucida-half/atlas-g07.png"),
];
const SPRITE64_LUCIDA_1X_BUCKET_PNGS: [&[u8];
    crate::gfx::althlasfont::athlasmetrics::ATHLAS_BUCKET_COUNT] = [
    include_bytes!("../gfx/althlasfont/lucida-1x/atlas-g00.png"),
    include_bytes!("../gfx/althlasfont/lucida-1x/atlas-g01.png"),
    include_bytes!("../gfx/althlasfont/lucida-1x/atlas-g02.png"),
    include_bytes!("../gfx/althlasfont/lucida-1x/atlas-g03.png"),
    include_bytes!("../gfx/althlasfont/lucida-1x/atlas-g04.png"),
    include_bytes!("../gfx/althlasfont/lucida-1x/atlas-g05.png"),
    include_bytes!("../gfx/althlasfont/lucida-1x/atlas-g06.png"),
    include_bytes!("../gfx/althlasfont/lucida-1x/atlas-g07.png"),
];
const SPRITE64_PALATINO_THIRD_BUCKET_PNGS: [&[u8];
    crate::gfx::althlasfont::athlasmetrics::ATHLAS_BUCKET_COUNT] = [
    include_bytes!("../gfx/althlasfont/palatino-third/atlas-g00.png"),
    include_bytes!("../gfx/althlasfont/palatino-third/atlas-g01.png"),
    include_bytes!("../gfx/althlasfont/palatino-third/atlas-g02.png"),
    include_bytes!("../gfx/althlasfont/palatino-third/atlas-g03.png"),
    include_bytes!("../gfx/althlasfont/palatino-third/atlas-g04.png"),
    include_bytes!("../gfx/althlasfont/palatino-third/atlas-g05.png"),
    include_bytes!("../gfx/althlasfont/palatino-third/atlas-g06.png"),
    include_bytes!("../gfx/althlasfont/palatino-third/atlas-g07.png"),
];
const SPRITE64_PALATINO_HALF_BUCKET_PNGS: [&[u8];
    crate::gfx::althlasfont::athlasmetrics::ATHLAS_BUCKET_COUNT] = [
    include_bytes!("../gfx/althlasfont/palatino-half/atlas-g00.png"),
    include_bytes!("../gfx/althlasfont/palatino-half/atlas-g01.png"),
    include_bytes!("../gfx/althlasfont/palatino-half/atlas-g02.png"),
    include_bytes!("../gfx/althlasfont/palatino-half/atlas-g03.png"),
    include_bytes!("../gfx/althlasfont/palatino-half/atlas-g04.png"),
    include_bytes!("../gfx/althlasfont/palatino-half/atlas-g05.png"),
    include_bytes!("../gfx/althlasfont/palatino-half/atlas-g06.png"),
    include_bytes!("../gfx/althlasfont/palatino-half/atlas-g07.png"),
];
const SPRITE64_PALATINO_1X_BUCKET_PNGS: [&[u8];
    crate::gfx::althlasfont::athlasmetrics::ATHLAS_BUCKET_COUNT] = [
    include_bytes!("../gfx/althlasfont/palatino-1x/atlas-g00.png"),
    include_bytes!("../gfx/althlasfont/palatino-1x/atlas-g01.png"),
    include_bytes!("../gfx/althlasfont/palatino-1x/atlas-g02.png"),
    include_bytes!("../gfx/althlasfont/palatino-1x/atlas-g03.png"),
    include_bytes!("../gfx/althlasfont/palatino-1x/atlas-g04.png"),
    include_bytes!("../gfx/althlasfont/palatino-1x/atlas-g05.png"),
    include_bytes!("../gfx/althlasfont/palatino-1x/atlas-g06.png"),
    include_bytes!("../gfx/althlasfont/palatino-1x/atlas-g07.png"),
];
const RECT_WORKLIST_IDD_OFFSET_BYTES: usize = 0x1400;
const RECT_WORKLIST_BINDING_TABLE_OFFSET_BYTES: usize = 0x1440;
const RECT_WORKLIST_SRC_SURFACE_STATE_OFFSET_BYTES: usize = 0x1480;
const RECT_WORKLIST_DST_SURFACE_STATE_OFFSET_BYTES: usize = 0x14C0;
const RECT_WORKLIST_DESC_SURFACE_STATE_OFFSET_BYTES: usize = 0x1500;
const RECT_WORKLIST_PAYLOAD_OFFSET_BYTES: usize = 0x1600;
const RECT_WORKLIST_IDD_BYTES: usize = 8 * core::mem::size_of::<u32>();
const RECT_WORKLIST_CROSS_THREAD_GRFS: u32 = 3;
const RECT_WORKLIST_CROSS_THREAD_BYTES: usize = RECT_WORKLIST_CROSS_THREAD_GRFS as usize * 32;
const RECT_WORKLIST_PER_THREAD_BYTES: usize = 96;
const RECT_WORKLIST_INDIRECT_BYTES: usize =
    RECT_WORKLIST_CROSS_THREAD_BYTES + RECT_WORKLIST_PER_THREAD_BYTES;
const RECT_WORKLIST_PRE_MARKER_SLOT: usize = 15;
const RECT_WORKLIST_POST_MARKER_SLOT: usize = 14;
const FILL_RECT_WORKLIST_PRE_MARKER: u32 = 0xC0DE_5801;
const FILL_RECT_WORKLIST_POST_MARKER: u32 = 0xC0DE_5802;
const ALPHA_BLEND_WORKLIST_PRE_MARKER: u32 = 0xC0DE_5901;
const ALPHA_BLEND_WORKLIST_POST_MARKER: u32 = 0xC0DE_5902;
const GRADIENT_RECT_WORKLIST_PRE_MARKER: u32 = 0xC0DE_5A01;
const GRADIENT_RECT_WORKLIST_POST_MARKER: u32 = 0xC0DE_5A02;
const MANDEL64_WORKLIST_PRE_MARKER: u32 = 0xC0DE_6401;
const MANDEL64_WORKLIST_POST_MARKER: u32 = 0xC0DE_6402;
const RECT_WORKLIST_DESC_GPU: u64 = 0x05A0_0000;
const MANDEL64_WORKLIST_DESC_GPU: u64 = 0x05B0_0000;
const RECT_WORKLIST_MAX_DESCS: usize = 256;
const RECT_WORKLIST_DESCS_PER_WALKER: usize = 16;
const RECT_WORKLIST_MAX_WALKERS: usize = RECT_WORKLIST_MAX_DESCS / RECT_WORKLIST_DESCS_PER_WALKER;
const RECT_WORKLIST_DESC_BYTES: usize = 8192;
const MANDEL64_WORKLIST_CELL_PIXELS: u32 = 64;
const MANDEL64_WORKLIST_BAND_ROWS: u32 = 4;
const MANDEL64_WORKLIST_BANDS_PER_TILE: usize =
    (MANDEL64_WORKLIST_CELL_PIXELS / MANDEL64_WORKLIST_BAND_ROWS) as usize;
const MANDEL64_WORKLIST_MAX_DESCS: usize = 512;
const MANDEL64_WORKLIST_DESCS_PER_WALKER: usize = RECT_WORKLIST_DESCS_PER_WALKER;
const MANDEL64_WORKLIST_MAX_WALKERS: usize =
    MANDEL64_WORKLIST_MAX_DESCS / MANDEL64_WORKLIST_DESCS_PER_WALKER;
const MANDEL64_WORKLIST_FLAG_ROWS_MASK: u32 = 0x0000_00FF;
const MANDEL64_WORKLIST_FLAG_COLS_SHIFT: u32 = 8;
const MANDEL64_WORKLIST_FLAG_MIRROR_HEIGHT_SHIFT: u32 = 16;
pub(crate) const MANDEL64_WORKLIST_DEFAULT_ITERATIONS: u32 = 32;
pub(crate) const MANDEL64_WORKLIST_MAX_ITERATIONS: u32 = 512;
const CANVAS3D_PROJECT_IDD_OFFSET_BYTES: usize = 0x2000;
const CANVAS3D_PROJECT_BINDING_TABLE_OFFSET_BYTES: usize = 0x2040;
const CANVAS3D_PROJECT_VERTICES_SURFACE_STATE_OFFSET_BYTES: usize = 0x2080;
const CANVAS3D_PROJECT_OUT_SURFACE_STATE_OFFSET_BYTES: usize = 0x20C0;
const CANVAS3D_PROJECT_PAYLOAD_OFFSET_BYTES: usize = 0x2200;
const CANVAS3D_PROJECT_IDD_BYTES: usize = 8 * core::mem::size_of::<u32>();
const CANVAS3D_PROJECT_CROSS_THREAD_BYTES: usize = 96;
const CANVAS3D_PROJECT_PER_THREAD_BYTES: usize = 96;
const CANVAS3D_PROJECT_INDIRECT_BYTES: usize =
    CANVAS3D_PROJECT_CROSS_THREAD_BYTES + CANVAS3D_PROJECT_PER_THREAD_BYTES;
const CANVAS3D_PROJECT_PRE_MARKER_SLOT: usize = 9;
const CANVAS3D_PROJECT_POST_MARKER_SLOT: usize = 8;
const CANVAS3D_PROJECT_PRE_MARKER: u32 = 0xC0DE_3501;
const CANVAS3D_PROJECT_POST_MARKER: u32 = 0xC0DE_3502;
const CANVAS3D_PROJECT_VERTEX_COUNT: usize = 128;
const CANVAS3D_PROJECT_SAMPLE_COUNT: usize = 8;
const CANVAS3D_PROJECT_SMOKE_SRC_FIRST: u32 = 10;
const CANVAS3D_PROJECT_SMOKE_OUT_FIRST: u32 = 40;
const CANVAS3D_PROJECT_SMOKE_CANVAS_WIDTH: u32 = 640;
const CANVAS3D_PROJECT_SMOKE_CANVAS_HEIGHT: u32 = 480;
const CANVAS3D_PROJECT_Q16_ONE: i32 = 65_536;
const CANVAS3D_PROJECT_VERTEX_BYTES: usize =
    CANVAS3D_PROJECT_VERTEX_COUNT * core::mem::size_of::<Canvas3dVec3Q16>();
const CANVAS3D_PROJECT_OUT_BYTES: usize =
    CANVAS3D_PROJECT_VERTEX_COUNT * core::mem::size_of::<Canvas3dProjectedRgba8>();
const CANVAS3D_PROJECT_TEST_BYTES: usize = CANVAS3D_PROJECT_VERTEX_BYTES;
const CANVAS3D_PROJECT_OUT_ALLOC_BYTES: usize = 16 * 1024;
const CANVAS3D_PROJECT_OUT_GPU: u64 = DIRECT_RCS_GPU_VA_CANVAS3D_OUT_BASE;
const CANVAS3D_TMP_GPU: u64 = DIRECT_RCS_GPU_VA_CANVAS3D_TMP_BASE;
const CANVAS3D_TRANSFORM_IDD_OFFSET_BYTES: usize = 0x3000;
const CANVAS3D_TRANSFORM_BINDING_TABLE_OFFSET_BYTES: usize = 0x3040;
const CANVAS3D_TRANSFORM_SRC_SURFACE_STATE_OFFSET_BYTES: usize = 0x3080;
const CANVAS3D_TRANSFORM_DST_SURFACE_STATE_OFFSET_BYTES: usize = 0x30C0;
const CANVAS3D_TRANSFORM_PAYLOAD_OFFSET_BYTES: usize = 0x3200;
const CANVAS3D_TRANSFORM_IDD_BYTES: usize = 8 * core::mem::size_of::<u32>();
const CANVAS3D_TRANSFORM_FUSED_CROSS_THREAD_BYTES: usize = 128;
const CANVAS3D_TRANSFORM_FUSED_PER_THREAD_BYTES: usize = 96;
const CANVAS3D_TRANSFORM_FUSED_INDIRECT_BYTES: usize =
    CANVAS3D_TRANSFORM_FUSED_CROSS_THREAD_BYTES + CANVAS3D_TRANSFORM_FUSED_PER_THREAD_BYTES;
const CANVAS3D_TRANSFORM_PRE_MARKER_SLOT: usize = 11;
const CANVAS3D_TRANSFORM_POST_MARKER_SLOT: usize = 10;
const CANVAS3D_TRANSFORM_FUSED_PRE_MARKER: u32 = 0xC0DE_3641;
const CANVAS3D_TRANSFORM_FUSED_POST_MARKER: u32 = 0xC0DE_3642;
const CANVAS3D_CLIP_BOX_IDD_OFFSET_BYTES: usize = 0x3400;
const CANVAS3D_CLIP_BOX_BINDING_TABLE_OFFSET_BYTES: usize = 0x3440;
const CANVAS3D_CLIP_BOX_SRC_SURFACE_STATE_OFFSET_BYTES: usize = 0x3480;
const CANVAS3D_CLIP_BOX_DST_SURFACE_STATE_OFFSET_BYTES: usize = 0x34C0;
const CANVAS3D_CLIP_BOX_PAYLOAD_OFFSET_BYTES: usize = 0x3600;
const CANVAS3D_CLIP_BOX_IDD_BYTES: usize = 8 * core::mem::size_of::<u32>();
const CANVAS3D_CLIP_BOX_CROSS_THREAD_BYTES: usize = 128;
const CANVAS3D_CLIP_BOX_PER_THREAD_BYTES: usize = 96;
const CANVAS3D_CLIP_BOX_INDIRECT_BYTES: usize =
    CANVAS3D_CLIP_BOX_CROSS_THREAD_BYTES + CANVAS3D_CLIP_BOX_PER_THREAD_BYTES;
const CANVAS3D_CLIP_BOX_PRE_MARKER_SLOT: usize = 13;
const CANVAS3D_CLIP_BOX_POST_MARKER_SLOT: usize = 12;
const CANVAS3D_CLIP_BOX_PRE_MARKER: u32 = 0xC0DE_3651;
const CANVAS3D_CLIP_BOX_POST_MARKER: u32 = 0xC0DE_3652;
const CANVAS3D_PLANE_SAMPLE_IDD_OFFSET_BYTES: usize = 0x3800;
const CANVAS3D_PLANE_SAMPLE_BINDING_TABLE_OFFSET_BYTES: usize = 0x3840;
const CANVAS3D_PLANE_SAMPLE_OUT_SURFACE_STATE_OFFSET_BYTES: usize = 0x3880;
const CANVAS3D_PLANE_SAMPLE_SCRATCH_SURFACE_STATE_OFFSET_BYTES: usize = 0x38C0;
const CANVAS3D_PLANE_SAMPLE_PAYLOAD_OFFSET_BYTES: usize = 0x3A00;
const CANVAS3D_PLANE_SAMPLE_IDD_BYTES: usize = 8 * core::mem::size_of::<u32>();
const CANVAS3D_PLANE_SAMPLE_CROSS_THREAD_BYTES: usize = 224;
const CANVAS3D_PLANE_SAMPLE_PER_THREAD_BYTES: usize = 96;
const CANVAS3D_PLANE_SAMPLE_INDIRECT_BYTES: usize =
    CANVAS3D_PLANE_SAMPLE_CROSS_THREAD_BYTES + CANVAS3D_PLANE_SAMPLE_PER_THREAD_BYTES;
const CANVAS3D_PLANE_SAMPLE_PRE_MARKER_SLOT: usize = 17;
const CANVAS3D_PLANE_SAMPLE_POST_MARKER_SLOT: usize = 16;
const CANVAS3D_PLANE_SAMPLE_PRE_MARKER: u32 = 0xC0DE_3661;
const CANVAS3D_PLANE_SAMPLE_POST_MARKER: u32 = 0xC0DE_3662;
const CANVAS3D_PLANE_SAMPLE_COUNT: usize = 16;
const CANVAS3D_PLANE_SAMPLE_COUNT_U32: u32 = 16;
const CANVAS3D_PLANE_SAMPLE_OUT_FIRST: u32 = 24;
const CANVAS3D_PLANE_SAMPLE_GRID_U: u32 = 4;
const CANVAS3D_PLANE_SAMPLE_GRID_V: u32 = 4;
const CANVAS3D_PLANE_SAMPLE_COLOR: u32 = 0xFF44_CC88;
const CANVAS3D_PLANE_FILL_IDD_OFFSET_BYTES: usize = 0x3C00;
const CANVAS3D_PLANE_FILL_BINDING_TABLE_OFFSET_BYTES: usize = 0x3C40;
const CANVAS3D_PLANE_FILL_DST_SURFACE_STATE_OFFSET_BYTES: usize = 0x3C80;
const CANVAS3D_PLANE_FILL_SCRATCH_SURFACE_STATE_OFFSET_BYTES: usize = 0x3CC0;
const CANVAS3D_PLANE_FILL_PAYLOAD_OFFSET_BYTES: usize = 0x3E00;
const CANVAS3D_PLANE_FILL_IDD_BYTES: usize = 8 * core::mem::size_of::<u32>();
const CANVAS3D_PLANE_FILL_CROSS_THREAD_BYTES: usize = 256;
const CANVAS3D_PLANE_FILL_PER_THREAD_BYTES: usize = 96;
const CANVAS3D_PLANE_FILL_INDIRECT_BYTES: usize =
    CANVAS3D_PLANE_FILL_CROSS_THREAD_BYTES + CANVAS3D_PLANE_FILL_PER_THREAD_BYTES;
const CANVAS3D_PLANE_FILL_PRE_MARKER_SLOT: usize = 19;
const CANVAS3D_PLANE_FILL_POST_MARKER_SLOT: usize = 18;
const CANVAS3D_PLANE_FILL_PRE_MARKER: u32 = 0xC0DE_3671;
const CANVAS3D_PLANE_FILL_POST_MARKER: u32 = 0xC0DE_3672;
const CANVAS3D_PLANE_PATCH_FILL_CUT_PRE_MARKER_SLOT: usize = 21;
const CANVAS3D_PLANE_PATCH_FILL_CUT_POST_MARKER_SLOT: usize = 20;
const CANVAS3D_PLANE_PATCH_FILL_CUT_PRE_MARKER: u32 = 0xC0DE_3681;
const CANVAS3D_PLANE_PATCH_FILL_CUT_POST_MARKER: u32 = 0xC0DE_3682;
const CANVAS3D_PLANE_PATCH_WORKLIST_DESC_SURFACE_STATE_OFFSET_BYTES: usize = 0x3D00;
const CANVAS3D_PLANE_PATCH_WORKLIST_CROSS_THREAD_BYTES: usize = 96;
const CANVAS3D_PLANE_PATCH_WORKLIST_PER_THREAD_BYTES: usize = 96;
const CANVAS3D_PLANE_PATCH_WORKLIST_INDIRECT_BYTES: usize =
    CANVAS3D_PLANE_PATCH_WORKLIST_CROSS_THREAD_BYTES
        + CANVAS3D_PLANE_PATCH_WORKLIST_PER_THREAD_BYTES;
const CANVAS3D_PLANE_PATCH_WORKLIST_PRE_MARKER_SLOT: usize = 23;
const CANVAS3D_PLANE_PATCH_WORKLIST_POST_MARKER_SLOT: usize = 22;
const CANVAS3D_PLANE_PATCH_WORKLIST_PRE_MARKER: u32 = 0xC0DE_3691;
const CANVAS3D_PLANE_PATCH_WORKLIST_POST_MARKER: u32 = 0xC0DE_3692;
const CANVAS3D_PLANE_PATCH_WORKLIST_DESC_DWORDS: usize = 40;
const CANVAS3D_PLANE_PATCH_WORKLIST_MAX_DESCS: usize = 16;
const CANVAS3D_PLANE_PATCH_WORKLIST_TEST_DESCS: usize = 3;
const CANVAS3D_PLANE_PATCH_WORKLIST_PIXELS_PER_LANE: u32 = 8;
const CANVAS3D_PLANE_PATCH_WORKLIST_TILE_ROWS: u32 = 16;
const CANVAS3D_PLANE_PATCH_WORKLIST_DESC_BYTES: usize = CANVAS3D_PLANE_PATCH_WORKLIST_MAX_DESCS
    * CANVAS3D_PLANE_PATCH_WORKLIST_DESC_DWORDS
    * core::mem::size_of::<u32>();
const CANVAS3D_PLANE_FILL_TEST_WIDTH: u32 = 64;
const CANVAS3D_PLANE_FILL_TEST_HEIGHT: u32 = 48;
const CANVAS3D_PLANE_FILL_TEST_PITCH_BYTES: u32 =
    CANVAS3D_PLANE_FILL_TEST_WIDTH * core::mem::size_of::<u32>() as u32;
const CANVAS3D_PLANE_FILL_TEST_BYTES: usize =
    CANVAS3D_PLANE_FILL_TEST_PITCH_BYTES as usize * CANVAS3D_PLANE_FILL_TEST_HEIGHT as usize;
const CANVAS3D_PLANE_FILL_TEST_COLOR: u32 = 0xFF66_CCFF;
const CANVAS3D_TRANSFORM_SRC_FIRST: u32 = 10;
const CANVAS3D_TRANSFORM_DST_FIRST: u32 = 40;
const CANVAS3D_TRANSFORM_TEST_COUNT: u32 = 8;
const CANVAS3D_TRANSFORM_TEST_COUNT_USIZE: usize = 8;
const CANVAS3D_TRANSFORM_DST_POISON: Canvas3dVec3Q16 = Canvas3dVec3Q16 {
    x: 0x1357_0001,
    y: 0x2468_0002,
    z: 0x3579_0003,
    pad: 0x468A_0004,
};
const CUBE20_PROJECT_TILE_SIZE: u32 = 128;
const CUBE20_PROJECT_RADIUS_PX: u32 = 64;
const CUBE20_PROJECT_DOT_RADIUS: i32 = 1;
const CUBE20_PROJECT_DEFAULT_CADENCE_US: u64 = 100_000;
const CUBE20_PROJECT_MIN_CADENCE_US: u64 = 100;
const CUBE20_PROJECT_MAX_CADENCE_US: u64 = 200_000;
const CUBE20_PROJECT_MAX_DURATION_MS: u64 = 60_000;
const CUBE20_CORNER_COUNT: usize = 8;
const CUBE20_EDGE_COUNT: usize = 12;
const CUBE20_EDGE_SAMPLE_COUNT: usize = 1;
const CUBE20_VERTEX_COUNT: usize =
    CUBE20_CORNER_COUNT + CUBE20_EDGE_COUNT * CUBE20_EDGE_SAMPLE_COUNT;
const CUBE20_INSTANCE_COUNT: usize = 1;
const CUBE20_VISUAL_VERTEX_COUNT: usize = CUBE20_VERTEX_COUNT * CUBE20_INSTANCE_COUNT;
const TETRA10_CORNER_COUNT: usize = 4;
const TETRA10_EDGE_COUNT: usize = 6;
const TETRA10_EDGE_SAMPLE_COUNT: usize = 1;
const TETRA10_VERTEX_COUNT: usize =
    TETRA10_CORNER_COUNT + TETRA10_EDGE_COUNT * TETRA10_EDGE_SAMPLE_COUNT;
const TETRA10_BASE_VERTEX: usize = CUBE20_VISUAL_VERTEX_COUNT;
const CANVAS3D_VISUAL_VERTEX_COUNT: usize = CUBE20_VISUAL_VERTEX_COUNT + TETRA10_VERTEX_COUNT;
const ICO30_CORNER_COUNT: usize = 30;
const ICO60_EDGE_COUNT: usize = 60;
const ICO90_VERTEX_COUNT: usize = ICO30_CORNER_COUNT + ICO60_EDGE_COUNT;
const CUBE20_HALF_Q16: i32 = CANVAS3D_PROJECT_Q16_ONE / 2;
const CUBE20_SEED_SCALE: i32 = 2;
const CUBE20_SEED_HALF_Q16: i32 = CUBE20_HALF_Q16 * CUBE20_SEED_SCALE;
const CUBE20_PRESENT_COLORS: [u32; CUBE20_INSTANCE_COUNT] = [0xFFFF_3048];
const CUBE20_EDGES: [(usize, usize); CUBE20_EDGE_COUNT] = [
    (0, 1),
    (1, 3),
    (3, 2),
    (2, 0),
    (4, 5),
    (5, 7),
    (7, 6),
    (6, 4),
    (0, 4),
    (1, 5),
    (2, 6),
    (3, 7),
];
const TETRA10_EDGES: [(usize, usize); TETRA10_EDGE_COUNT] =
    [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)];
const COPY_RECT_TEST_WIDTH: u32 = 4;
const COPY_RECT_TEST_HEIGHT: u32 = 1;
const COPY_RECT_TEST_PIXELS: usize =
    (COPY_RECT_TEST_WIDTH as usize) * (COPY_RECT_TEST_HEIGHT as usize);
const COPY_RECT_TEST_ROW_PIXELS: u32 = COPY_RECT_TEST_WIDTH * 2;
const COPY_RECT_TEST_ROW_DWORDS: usize = COPY_RECT_TEST_ROW_PIXELS as usize;
const COPY_RECT_TEST_PITCH_BYTES: u32 =
    COPY_RECT_TEST_ROW_PIXELS * core::mem::size_of::<u32>() as u32;
const COPY_RECT_TEST_DST_X: u32 = COPY_RECT_TEST_WIDTH;
const COPY_RECT_WALKER_RIGHT_MASK: u32 = 0x0000_0003;
const COPY_RECT_PRE_MARKER_SLOT: usize = 5;
const COPY_RECT_POST_MARKER_SLOT: usize = 4;
const COPY_RECT_PRE_MARKER: u32 = 0xC0DE_A701;
const COPY_RECT_POST_MARKER: u32 = 0xC0DE_A702;
const COPY_RECT_DST_POISON0: u32 = 0x1111_1111;
const COPY_RECT_DST_POISON1: u32 = 0x2222_2222;
const COPY_RECT_DST_POISON2: u32 = 0x3333_3333;
const COPY_RECT_DST_POISON3: u32 = 0x4444_4444;
const COPY_RECT_256_WIDTH: u32 = 256;
const COPY_RECT_256_HEIGHT: u32 = 1;
const COPY_RECT_256_SURFACE_WIDTH: u32 = COPY_RECT_256_WIDTH * 2;
const COPY_RECT_256_PITCH_BYTES: u32 =
    COPY_RECT_256_SURFACE_WIDTH * core::mem::size_of::<u32>() as u32;
const COPY_RECT_256_EXPECTED_SPANS: usize = 8;
const COPY_RECT_256_SAMPLE_INDICES: [usize; 10] = [0, 1, 2, 3, 64, 127, 128, 191, 254, 255];
const COPY_RECT_256X2_WIDTH: u32 = 256;
const COPY_RECT_256X2_HEIGHT: u32 = 2;
const COPY_RECT_256X2_SURFACE_WIDTH: u32 = COPY_RECT_256X2_WIDTH * 2;
const COPY_RECT_256X2_PITCH_BYTES: u32 =
    COPY_RECT_256X2_SURFACE_WIDTH * core::mem::size_of::<u32>() as u32;
const COPY_RECT_256X2_EXPECTED_SPANS: usize = 16;
const COPY_RECT_256X2_EXPECTED_SUBMITS: usize = 1;
const COPY_RECT_256X2_SAMPLE_POINTS: [(usize, usize); 12] = [
    (0, 0),
    (1, 0),
    (2, 0),
    (3, 0),
    (127, 0),
    (255, 0),
    (0, 1),
    (1, 1),
    (2, 1),
    (3, 1),
    (127, 1),
    (255, 1),
];
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
const CLEAR_RECT_EXPECTED_WHITE: u32 = 0xFFFF_FFFF;
const CLEAR_RECT_RGBA_RED: u32 = 0xFF00_00FF;
const CLEAR_RECT_RGBA_GREEN: u32 = 0xFF00_FF00;
const CLEAR_RECT_RGBA_BLUE: u32 = 0xFFFF_0000;
const CLEAR_RECT_RGBA_BLACK: u32 = 0xFF00_0000;
const SURFTYPE_BUFFER: u32 = 4;
const SURFACE_FORMAT_RAW: u32 = 0x1FF;

const DIRECT_RCS_ENABLED: bool = true;
const DIRECT_RCS_RING_BYTES: usize = 4096;
const DIRECT_RCS_CONTEXT_BYTES: usize = 22 * 4096;
const DIRECT_RCS_BATCH_BYTES: usize = 64 * 1024;
const DIRECT_RCS_RESULT_BYTES: usize = 4096;
const DIRECT_RCS_PPGTT_PT_COUNT: usize = 128;
const DIRECT_RCS_PPGTT_BYTES: usize = (3 + DIRECT_RCS_PPGTT_PT_COUNT) * 4096;
const DIRECT_RCS_LRC_STATE_OFFSET_DWORDS: usize = 4096 / core::mem::size_of::<u32>();
const DIRECT_RCS_BATCH_START_DWORDS: usize = 4;
const DIRECT_RCS_GPU_VA_RING_BASE: u64 = 0x0080_0000;
const DIRECT_RCS_GPU_VA_CONTEXT_BASE: u64 = 0x0081_0000;
const DIRECT_RCS_GPU_VA_RESULT_BASE: u64 = 0x0084_0000;
const DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE: u64 = 0x0089_0000;
const DIRECT_RCS_GPU_VA_SHELL_SURFACE_BASE: u64 = 0x008A_0000;
const DIRECT_RCS_GPU_VA_CANVAS3D_OUT_BASE: u64 = 0x008F_0000;
const DIRECT_RCS_GPU_VA_CANVAS3D_TMP_BASE: u64 = 0x0090_0000;
const DIRECT_RCS_GPU_VA_PRESENT_STAGING_BASE: u64 = 0x00A0_0000;
const DIRECT_RCS_GPU_VA_SOLID_RECT_SOURCE_BASE: u64 = 0x0400_0000;
const DIRECT_RCS_GPU_VA_BATCH_BASE: u64 = 0x0180_0000;
const DIRECT_RCS_SMOKE_MARKER: u32 = 0xC0DE_5101;
const DIRECT_RCS_SMOKE_POLL_ITERS: usize = 262_144;
pub(crate) const GPGPU_SHELL_SURFACE_WIDTH: u32 = 1024;
pub(crate) const GPGPU_SHELL_SURFACE_HEIGHT: u32 = 64;
pub(crate) const GPGPU_SHELL_SURFACE_PITCH_BYTES: u32 =
    GPGPU_SHELL_SURFACE_WIDTH * core::mem::size_of::<u32>() as u32;
const GPGPU_SHELL_SURFACE_BYTES: usize =
    (GPGPU_SHELL_SURFACE_PITCH_BYTES as usize) * (GPGPU_SHELL_SURFACE_HEIGHT as usize);

static COPY_RECT_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static BLIT_RGBA8_NEAREST_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static FILL_RECT_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static FILL_RECT_WORKLIST_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static GRADIENT_RECT_WORKLIST_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> =
    Mutex::new(None);
static ALPHA_BLEND_RGBA8_OVER_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static ALPHA_BLEND_WORKLIST_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static GLYPH_MASK_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static PRESENT_RGBA8_TO_PRIMARY_XRGB_RECT_UPLOAD: Mutex<Option<UploadedKernelArtifact>> =
    Mutex::new(None);
static SPRITE64_WORKLIST_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
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
static DIRECT_RCS_STATE: Mutex<Option<DirectRcsState>> = Mutex::new(None);
static GPGPU_SHELL_SURFACE: Mutex<Option<GpgpuShellSurface>> = Mutex::new(None);
static GPGPU_PRESENT_STAGING_SURFACE: Mutex<Option<GpgpuPresentStagingSurface>> = Mutex::new(None);
static GPGPU_SOLID_RECT_SOURCE_SURFACE: Mutex<Option<GpgpuSolidRectSourceSurface>> =
    Mutex::new(None);
static GPGPU_SPRITE64_WORKLIST_ATLAS: Once<Option<GpgpuSprite64WorklistAtlasSurface>> = Once::new();
static GPGPU_SPRITE64_WORKLIST_DESC: Mutex<Option<GpgpuSprite64WorklistDescBuffer>> =
    Mutex::new(None);
static GPGPU_RECT_WORKLIST_DESC: Mutex<Option<GpgpuRectWorklistDescBuffer>> = Mutex::new(None);
static GPGPU_MANDEL64_WORKLIST_DESC: Mutex<Option<GpgpuRectWorklistDescBuffer>> = Mutex::new(None);
static RECT_WORKLIST_DESC_SUBMIT_LOCK: Mutex<()> = Mutex::new(());
static GPGPU_TWEMOJI_ATLAS: Once<Option<GpgpuTwemojiAtlasCache>> = Once::new();
static DIRECT_RCS_SUBMIT_LOCK: Mutex<()> = Mutex::new(());
static DIRECT_RCS_SMOKE_RAN: AtomicBool = AtomicBool::new(false);
static COPY_RECT_WALKER_RAN: AtomicBool = AtomicBool::new(false);
static PRESENT_RGBA8_TO_PRIMARY_XRGB_PRESENT_SEQ: AtomicU32 = AtomicU32::new(0);
static COPY_RECT_256_RAN: AtomicBool = AtomicBool::new(false);
static COPY_RECT_256X2_RAN: AtomicBool = AtomicBool::new(false);
static RECT_API_SMOKE_RAN: AtomicBool = AtomicBool::new(false);
static FILL_RECT_WORKLIST_RAN: AtomicBool = AtomicBool::new(false);
static GRADIENT_RECT_WORKLIST_RAN: AtomicBool = AtomicBool::new(false);
static ALPHA_BLEND_WORKLIST_RAN: AtomicBool = AtomicBool::new(false);
static FILL_RECT_WORKLIST_OK: AtomicBool = AtomicBool::new(false);
static GRADIENT_RECT_WORKLIST_OK: AtomicBool = AtomicBool::new(false);
static ALPHA_BLEND_WORKLIST_OK: AtomicBool = AtomicBool::new(false);
static RECT_WORKLIST_NOT_READY_LOGS: AtomicU32 = AtomicU32::new(0);
static SPRITE64_WORKLIST_PRIMARY_PHASE_LOGS: AtomicU32 = AtomicU32::new(0);
static CANVAS3D_PROJECT_RAN: AtomicBool = AtomicBool::new(false);
static CANVAS3D_TRANSFORM_RAN: AtomicBool = AtomicBool::new(false);
static CANVAS3D_CLIP_BOX_RAN: AtomicBool = AtomicBool::new(false);
static CANVAS3D_PLANE_SAMPLE_RAN: AtomicBool = AtomicBool::new(false);
static CANVAS3D_PLANE_FILL_RAN: AtomicBool = AtomicBool::new(false);
static CANVAS3D_PLANE_PATCH_FILL_CUT_RAN: AtomicBool = AtomicBool::new(false);
static CANVAS3D_PLANE_PATCH_WORKLIST_RAN: AtomicBool = AtomicBool::new(false);
static DIRECT_RCS_SUBMIT_COUNTER: AtomicU32 = AtomicU32::new(0);

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
pub(crate) struct StampMandelRgba8Params {
    pub(crate) dst_gpu: u64,
    pub(crate) dst_pitch_bytes: u32,
    pub(crate) dst_x: u32,
    pub(crate) dst_y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

pub(crate) const SPRITE64_WORKLIST_FLAG_SRC_OVER: u32 = 1 << 0;
pub(crate) const SPRITE64_WORKLIST_FLAG_TINT_RGB: u32 = 1 << 1;

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct Sprite64WorklistRgba8Desc {
    pub(crate) atlas_xy: u32,
    pub(crate) dst_xy: u32,
    pub(crate) flags: u32,
    pub(crate) color_rgba: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct Sprite64WorklistRgba8Params {
    pub(crate) atlas_gpu: u64,
    pub(crate) dst_gpu: u64,
    pub(crate) desc_gpu: u64,
    pub(crate) atlas_pitch_bytes: u32,
    pub(crate) dst_pitch_bytes: u32,
    pub(crate) desc_base: u32,
    pub(crate) desc_count: u32,
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

pub(crate) const GRADIENT_RECT_WORKLIST_FLAG_VERTICAL: u32 = 1 << 0;

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GradientRectWorklistRgba8Desc {
    pub(crate) dst_xy: u32,
    pub(crate) size: u32,
    pub(crate) color0_rgba: u32,
    pub(crate) color1_rgba: u32,
    pub(crate) flags: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GradientRectWorklistRgba8Params {
    pub(crate) dst_gpu: u64,
    pub(crate) desc_gpu: u64,
    pub(crate) dst_pitch_bytes: u32,
    pub(crate) desc_base: u32,
    pub(crate) desc_count: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct AlphaBlendWorklistRgba8Desc {
    pub(crate) src_xy: u32,
    pub(crate) dst_xy: u32,
    pub(crate) size: u32,
    pub(crate) flags: u32,
    pub(crate) color_rgba: u32,
}

pub(crate) const COMPOSITE_WORKLIST_FLAG_COPY: u32 = 1 << 0;
pub(crate) const COMPOSITE_WORKLIST_FLAG_SRC_OVER: u32 = 1 << 1;
pub(crate) const COMPOSITE_WORKLIST_FLAG_TINT_RGB: u32 = 1 << 2;
pub(crate) const COMPOSITE_WORKLIST_FLAG_TINT_ALPHA: u32 = 1 << 3;
pub(crate) const COMPOSITE_WORKLIST_FLAG_PREMUL_SRC: u32 = 1 << 4;
pub(crate) const COMPOSITE_WORKLIST_NEUTRAL_COLOR_RGBA: u32 = 0xFFFF_FFFF;

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct AlphaBlendWorklistRgba8Params {
    pub(crate) src_gpu: u64,
    pub(crate) dst_gpu: u64,
    pub(crate) desc_gpu: u64,
    pub(crate) src_pitch_bytes: u32,
    pub(crate) dst_pitch_bytes: u32,
    pub(crate) desc_base: u32,
    pub(crate) desc_count: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Canvas3dVec3Q16 {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) z: i32,
    pub(crate) pad: i32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Canvas3dProjectedRgba8 {
    pub(crate) packed_xy: u32,
    pub(crate) rgba: u32,
    pub(crate) z_q16: u32,
    pub(crate) source_index: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct Canvas3dProjectRgba8Params {
    pub(crate) vertices_gpu: u64,
    pub(crate) out_gpu: u64,
    pub(crate) src_first_vertex: u32,
    pub(crate) out_first_point: u32,
    pub(crate) vertex_count: u32,
    pub(crate) canvas_width: u32,
    pub(crate) canvas_height: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct Canvas3dTransformFusedQ16Params {
    pub(crate) src_gpu: u64,
    pub(crate) dst_gpu: u64,
    pub(crate) src_first_vertex: u32,
    pub(crate) dst_first_vertex: u32,
    pub(crate) vertex_count: u32,
    pub(crate) scale_q16: Canvas3dVec3Q16,
    pub(crate) rotate_q16: Canvas3dVec3Q16,
    pub(crate) translate_q16: Canvas3dVec3Q16,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct Canvas3dClipBoxQ16Params {
    pub(crate) src_gpu: u64,
    pub(crate) dst_gpu: u64,
    pub(crate) src_first_vertex: u32,
    pub(crate) dst_first_vertex: u32,
    pub(crate) vertex_count: u32,
    pub(crate) min_q16: Canvas3dVec3Q16,
    pub(crate) max_q16: Canvas3dVec3Q16,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct Canvas3dPlaneSampleRgba8Params {
    pub(crate) out_gpu: u64,
    pub(crate) out_first_point: u32,
    pub(crate) sample_count: u32,
    pub(crate) canvas_width: u32,
    pub(crate) canvas_height: u32,
    pub(crate) origin_q16: Canvas3dVec3Q16,
    pub(crate) axis_u_q16: Canvas3dVec3Q16,
    pub(crate) axis_v_q16: Canvas3dVec3Q16,
    pub(crate) constraint0_q16: Canvas3dVec3Q16,
    pub(crate) constraint1_q16: Canvas3dVec3Q16,
    pub(crate) constraint2_q16: Canvas3dVec3Q16,
    pub(crate) constraint3_q16: Canvas3dVec3Q16,
    pub(crate) constraint_count: u32,
    pub(crate) u_steps: u32,
    pub(crate) v_steps: u32,
    pub(crate) color_rgba: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct Canvas3dPlaneFillRgba8Params {
    pub(crate) dst_gpu: u64,
    pub(crate) dst_pitch_bytes: u32,
    pub(crate) dst_width: u32,
    pub(crate) dst_height: u32,
    pub(crate) rect_x: u32,
    pub(crate) rect_y: u32,
    pub(crate) rect_width: u32,
    pub(crate) rect_height: u32,
    pub(crate) canvas_width: u32,
    pub(crate) canvas_height: u32,
    pub(crate) origin_q16: Canvas3dVec3Q16,
    pub(crate) axis_u_q16: Canvas3dVec3Q16,
    pub(crate) axis_v_q16: Canvas3dVec3Q16,
    pub(crate) constraint0_q16: Canvas3dVec3Q16,
    pub(crate) constraint1_q16: Canvas3dVec3Q16,
    pub(crate) constraint2_q16: Canvas3dVec3Q16,
    pub(crate) constraint3_q16: Canvas3dVec3Q16,
    pub(crate) constraint_count: u32,
    pub(crate) color_rgba: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct Canvas3dPlanePatchWorklistRgba8Params {
    pub(crate) dst_gpu: u64,
    pub(crate) desc_gpu: u64,
    pub(crate) desc_base: u32,
    pub(crate) desc_count: u32,
    pub(crate) group_x: u32,
    pub(crate) group_y: u32,
    pub(crate) group_z: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct Canvas3dPlanePatchWorklistRgba8Desc {
    pub(crate) dst_pitch_bytes: u32,
    pub(crate) dst_width: u32,
    pub(crate) dst_height: u32,
    pub(crate) rect_x: u32,
    pub(crate) rect_y: u32,
    pub(crate) rect_width: u32,
    pub(crate) rect_height: u32,
    pub(crate) canvas_width: u32,
    pub(crate) canvas_height: u32,
    pub(crate) reserved0: u32,
    pub(crate) origin_q16: Canvas3dVec3Q16,
    pub(crate) axis_u_q16: Canvas3dVec3Q16,
    pub(crate) axis_v_q16: Canvas3dVec3Q16,
    pub(crate) constraint0_q16: Canvas3dVec3Q16,
    pub(crate) constraint1_q16: Canvas3dVec3Q16,
    pub(crate) constraint2_q16: Canvas3dVec3Q16,
    pub(crate) constraint3_q16: Canvas3dVec3Q16,
    pub(crate) constraint_count: u32,
    pub(crate) color_rgba: u32,
}

pub(crate) fn canvas3d_plane_patch_worklist_groups_for_descs(
    descs: &[Canvas3dPlanePatchWorklistRgba8Desc],
) -> (u32, u32, u32) {
    if descs.is_empty() {
        return (1, 1, 1);
    }

    let mut max_width = 1u32;
    let mut max_height = 1u32;
    for desc in descs {
        max_width = max_width.max(desc.rect_width);
        max_height = max_height.max(desc.rect_height);
    }

    let tile_cols = max_width.saturating_add(CANVAS3D_PLANE_PATCH_WORKLIST_PIXELS_PER_LANE - 1)
        / CANVAS3D_PLANE_PATCH_WORKLIST_PIXELS_PER_LANE;
    let tile_rows = max_height.saturating_add(CANVAS3D_PLANE_PATCH_WORKLIST_TILE_ROWS - 1)
        / CANVAS3D_PLANE_PATCH_WORKLIST_TILE_ROWS;
    let work_items = tile_cols.saturating_mul(tile_rows);
    let group_x = work_items.saturating_add(15) / 16;
    let group_y = 1u32;
    let group_z = 1u32;

    (group_x.max(1), group_y.max(1), group_z.max(1))
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

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct FillCircleRgba8Params {
    pub(crate) dst_gpu: u64,
    pub(crate) dst_pitch_bytes: u32,
    pub(crate) dst_x: u32,
    pub(crate) dst_y: u32,
    pub(crate) rect_width: u32,
    pub(crate) rect_height: u32,
    pub(crate) center_x: i32,
    pub(crate) center_y: i32,
    pub(crate) radius: u32,
    pub(crate) color_rgba: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct BlitRgba8NearestParams {
    pub(crate) src_gpu: u64,
    pub(crate) dst_gpu: u64,
    pub(crate) src_pitch_bytes: u32,
    pub(crate) dst_pitch_bytes: u32,
    pub(crate) src_x: u32,
    pub(crate) src_y: u32,
    pub(crate) src_width: u32,
    pub(crate) src_height: u32,
    pub(crate) dst_x: u32,
    pub(crate) dst_y: u32,
    pub(crate) dst_width: u32,
    pub(crate) dst_height: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct AlphaBlendRgba8OverParams {
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
pub(crate) struct GlyphMaskRgba8Params {
    pub(crate) mask_gpu: u64,
    pub(crate) dst_gpu: u64,
    pub(crate) mask_pitch_bytes: u32,
    pub(crate) dst_pitch_bytes: u32,
    pub(crate) mask_x: u32,
    pub(crate) mask_y: u32,
    pub(crate) dst_x: u32,
    pub(crate) dst_y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) color_rgba: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct PresentRgba8ToPrimaryXrgbRectParams {
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
    pub(crate) flip_y: u32,
}

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

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuMask8Surface {
    pub(crate) phys: u64,
    pub(crate) gpu: u64,
    pub(crate) bytes: usize,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pitch_bytes: u32,
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
pub(crate) struct GpgpuCopyRect {
    pub(crate) src: GpgpuRgba8Surface,
    pub(crate) src_rect: GpgpuRect,
    pub(crate) dst: GpgpuRgba8Surface,
    pub(crate) dst_xy: GpgpuPoint,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) enum GpgpuCompositeMode {
    #[default]
    Copy,
    SrcOver,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuCompositeRect {
    pub(crate) src: GpgpuRgba8Surface,
    pub(crate) src_rect: GpgpuRect,
    pub(crate) dst: GpgpuRgba8Surface,
    pub(crate) dst_xy: GpgpuPoint,
    pub(crate) mode: GpgpuCompositeMode,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuGlyphBlit {
    pub(crate) atlas: GpgpuRgba8Surface,
    pub(crate) glyph_rect: GpgpuRect,
    pub(crate) dst: GpgpuRgba8Surface,
    pub(crate) dst_xy: GpgpuPoint,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuGlyphMaskBlit {
    pub(crate) mask: GpgpuMask8Surface,
    pub(crate) mask_rect: GpgpuRect,
    pub(crate) dst: GpgpuRgba8Surface,
    pub(crate) dst_xy: GpgpuPoint,
    pub(crate) color_rgba: u32,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuSubmitStats {
    pub(crate) spans: usize,
    pub(crate) submits: usize,
    pub(crate) submit_ms: u64,
    pub(crate) present_ms: u64,
    pub(crate) total_ms: u64,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuWorklistSubmitStats {
    pub(crate) descs: usize,
    pub(crate) walkers: usize,
    pub(crate) submits: usize,
    pub(crate) submit_ms: u64,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuSolidRect {
    pub(crate) rect: GpgpuRect,
    pub(crate) color_rgba: u32,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuGradientRect {
    pub(crate) rect: GpgpuRect,
    pub(crate) color0_rgba: u32,
    pub(crate) color1_rgba: u32,
    pub(crate) vertical: bool,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuSolidRectOverlayResult {
    pub(crate) ok: bool,
    pub(crate) rects: usize,
    pub(crate) fill_descs: usize,
    pub(crate) fill_walkers: usize,
    pub(crate) fill_submits: usize,
    pub(crate) fill_ms: u64,
    pub(crate) blend_descs: usize,
    pub(crate) blend_walkers: usize,
    pub(crate) blend_submits: usize,
    pub(crate) blend_ms: u64,
    pub(crate) presented: bool,
    pub(crate) present_ms: u64,
    pub(crate) total_ms: u64,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuShellCopyResult {
    pub(crate) ok: bool,
    pub(crate) src_rect: GpgpuRect,
    pub(crate) dst_xy: GpgpuPoint,
    pub(crate) pixels: usize,
    pub(crate) spans: usize,
    pub(crate) expected_spans: usize,
    pub(crate) submits: usize,
    pub(crate) expected_submits: usize,
    pub(crate) copied: usize,
    pub(crate) src_preserved: usize,
    pub(crate) src_head: [u32; 4],
    pub(crate) dst_head: [u32; 4],
    pub(crate) surface: GpgpuRgba8Surface,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuShellScanoutCopyResult {
    pub(crate) ok: bool,
    pub(crate) src_rect: GpgpuRect,
    pub(crate) dst_xy: GpgpuPoint,
    pub(crate) primary_width: u32,
    pub(crate) primary_height: u32,
    pub(crate) primary_pitch_bytes: u32,
    pub(crate) primary_gpu: u64,
    pub(crate) primary_phys: u64,
    pub(crate) pixels: usize,
    pub(crate) spans: usize,
    pub(crate) expected_spans: usize,
    pub(crate) submits: usize,
    pub(crate) expected_submits: usize,
    pub(crate) copied: usize,
    pub(crate) src_preserved: usize,
    pub(crate) src_head: [u32; 4],
    pub(crate) dst_head: [u32; 4],
    pub(crate) presented: bool,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuShellAtlasWorklistResult {
    pub(crate) ok: bool,
    pub(crate) submitted: bool,
    pub(crate) requested: usize,
    pub(crate) descriptors: usize,
    pub(crate) walkers: usize,
    pub(crate) copied_pixels: usize,
    pub(crate) submit_ms: u64,
    pub(crate) present_ms: u64,
    pub(crate) total_ms: u64,
    pub(crate) atlas_gpu: u64,
    pub(crate) desc_gpu: u64,
    pub(crate) primary_width: u32,
    pub(crate) primary_height: u32,
    pub(crate) slots: u16,
    pub(crate) last_slot: u16,
    pub(crate) last_dst_xy: GpgpuPoint,
    pub(crate) presented: bool,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuSprite64AtlasWarmResult {
    pub(crate) ok: bool,
    pub(crate) slots: u16,
    pub(crate) columns: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pitch_bytes: u32,
    pub(crate) bytes: usize,
    pub(crate) atlas_gpu: u64,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuShellMandel64WorklistResult {
    pub(crate) ok: bool,
    pub(crate) submitted: bool,
    pub(crate) requested: usize,
    pub(crate) descriptors: usize,
    pub(crate) walkers: usize,
    pub(crate) pixels: usize,
    pub(crate) submit_ms: u64,
    pub(crate) present_ms: u64,
    pub(crate) total_ms: u64,
    pub(crate) desc_gpu: u64,
    pub(crate) primary_width: u32,
    pub(crate) primary_height: u32,
    pub(crate) last_src_xy: GpgpuPoint,
    pub(crate) last_dst_xy: GpgpuPoint,
    pub(crate) presented: bool,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuTwemojiSprite64Placement {
    pub(crate) slot: u16,
    pub(crate) dst_x: i32,
    pub(crate) dst_y: i32,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuSprite64Placement {
    slot: u16,
    dst_x: i32,
    dst_y: i32,
    flags: u32,
    color_rgba: u32,
}

impl GpgpuSprite64Placement {
    #[inline]
    pub(crate) const fn src_over(slot: u16, dst_x: i32, dst_y: i32) -> Self {
        Self {
            slot,
            dst_x,
            dst_y,
            flags: SPRITE64_WORKLIST_FLAG_SRC_OVER,
            color_rgba: 0x00FF_FFFF,
        }
    }

    #[inline]
    pub(crate) const fn tinted_src_over(
        slot: u16,
        dst_x: i32,
        dst_y: i32,
        color_rgba: u32,
    ) -> Self {
        Self {
            slot,
            dst_x,
            dst_y,
            flags: SPRITE64_WORKLIST_FLAG_SRC_OVER | SPRITE64_WORKLIST_FLAG_TINT_RGB,
            color_rgba,
        }
    }
}

trait Sprite64PlacementDesc {
    fn slot(&self) -> u16;
    fn dst_x(&self) -> i32;
    fn dst_y(&self) -> i32;
    fn flags(&self) -> u32;
    fn color_rgba(&self) -> u32;
}

impl Sprite64PlacementDesc for GpgpuTwemojiSprite64Placement {
    #[inline]
    fn slot(&self) -> u16 {
        self.slot
    }

    #[inline]
    fn dst_x(&self) -> i32 {
        self.dst_x
    }

    #[inline]
    fn dst_y(&self) -> i32 {
        self.dst_y
    }

    #[inline]
    fn flags(&self) -> u32 {
        SPRITE64_WORKLIST_FLAG_SRC_OVER
    }

    #[inline]
    fn color_rgba(&self) -> u32 {
        0x00FF_FFFF
    }
}

impl Sprite64PlacementDesc for GpgpuSprite64Placement {
    #[inline]
    fn slot(&self) -> u16 {
        self.slot
    }

    #[inline]
    fn dst_x(&self) -> i32 {
        self.dst_x
    }

    #[inline]
    fn dst_y(&self) -> i32 {
        self.dst_y
    }

    #[inline]
    fn flags(&self) -> u32 {
        self.flags
    }

    #[inline]
    fn color_rgba(&self) -> u32 {
        self.color_rgba
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
    pub(crate) mirror_height: u32,
    pub(crate) iterations: u32,
}

#[derive(Copy, Clone, Debug)]
struct GpgpuShellSurface {
    surface: GpgpuRgba8Surface,
    virt: *mut u8,
}

unsafe impl Send for GpgpuShellSurface {}
unsafe impl Sync for GpgpuShellSurface {}

#[derive(Copy, Clone, Debug)]
struct GpgpuPresentStagingSurface {
    surface: GpgpuRgba8Surface,
    virt: *mut u8,
}

unsafe impl Send for GpgpuPresentStagingSurface {}
unsafe impl Sync for GpgpuPresentStagingSurface {}

#[derive(Copy, Clone, Debug)]
struct GpgpuSolidRectSourceSurface {
    surface: GpgpuRgba8Surface,
    virt: *mut u8,
}

unsafe impl Send for GpgpuSolidRectSourceSurface {}
unsafe impl Sync for GpgpuSolidRectSourceSurface {}

#[derive(Copy, Clone, Debug)]
struct GpgpuSprite64WorklistAtlasSurface {
    surface: GpgpuRgba8Surface,
    columns: u32,
    slots: u16,
}

unsafe impl Send for GpgpuSprite64WorklistAtlasSurface {}
unsafe impl Sync for GpgpuSprite64WorklistAtlasSurface {}

#[derive(Copy, Clone, Debug)]
struct GpgpuSprite64WorklistDescBuffer {
    phys: u64,
    gpu: u64,
    virt: *mut u8,
    bytes: usize,
}

unsafe impl Send for GpgpuSprite64WorklistDescBuffer {}
unsafe impl Sync for GpgpuSprite64WorklistDescBuffer {}

#[derive(Copy, Clone, Debug)]
struct GpgpuRectWorklistDescBuffer {
    phys: u64,
    gpu: u64,
    virt: *mut u8,
    bytes: usize,
}

unsafe impl Send for GpgpuRectWorklistDescBuffer {}
unsafe impl Sync for GpgpuRectWorklistDescBuffer {}

#[derive(Clone, Debug)]
struct GpgpuTwemojiAtlasCache {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

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
    pub(crate) gpu: u64,
    pub(crate) phys: u64,
    pub(crate) virt: *mut u8,
    pub(crate) bytes: usize,
    pub(crate) mapped_bytes: usize,
    pub(crate) verified: bool,
}

unsafe impl Send for UploadedKernelArtifact {}
unsafe impl Sync for UploadedKernelArtifact {}

#[derive(Copy, Clone, Debug)]
struct CopyRectKernelFlavor {
    upload: UploadedKernelArtifact,
    text_offset_bytes: u64,
    pixels_per_lane: u32,
    span_pixels: u32,
    rows_per_walker: u32,
    name: &'static str,
}

pub(crate) const COPY_RECT_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact {
    name: COPY_RECT_RGBA8_KERNEL_NAME,
    target: "adls",
    bin: COPY_RECT_RGBA8_ADLS_BIN,
    spv: COPY_RECT_RGBA8_ADLS_SPV,
    bin_sha256: COPY_RECT_RGBA8_ADLS_BIN_SHA256,
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

pub(crate) const FILL_CIRCLE_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact {
    name: FILL_CIRCLE_RGBA8_KERNEL_NAME,
    target: "adls",
    bin: FILL_CIRCLE_RGBA8_ADLS_BIN,
    spv: FILL_CIRCLE_RGBA8_ADLS_SPV,
    bin_sha256: FILL_CIRCLE_RGBA8_ADLS_BIN_SHA256,
};

pub(crate) const BLIT_RGBA8_NEAREST_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact {
    name: BLIT_RGBA8_NEAREST_KERNEL_NAME,
    target: "adls",
    bin: BLIT_RGBA8_NEAREST_ADLS_BIN,
    spv: BLIT_RGBA8_NEAREST_ADLS_SPV,
    bin_sha256: BLIT_RGBA8_NEAREST_ADLS_BIN_SHA256,
};

pub(crate) const ALPHA_BLEND_RGBA8_OVER_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact {
    name: ALPHA_BLEND_RGBA8_OVER_KERNEL_NAME,
    target: "adls",
    bin: ALPHA_BLEND_RGBA8_OVER_ADLS_BIN,
    spv: ALPHA_BLEND_RGBA8_OVER_ADLS_SPV,
    bin_sha256: ALPHA_BLEND_RGBA8_OVER_ADLS_BIN_SHA256,
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

pub(crate) const STAMP_MANDEL_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact {
    name: STAMP_MANDEL_RGBA8_KERNEL_NAME,
    target: "adls",
    bin: STAMP_MANDEL_RGBA8_ADLS_BIN,
    spv: STAMP_MANDEL_RGBA8_ADLS_SPV,
    bin_sha256: STAMP_MANDEL_RGBA8_ADLS_BIN_SHA256,
};

pub(crate) const SPRITE64_WORKLIST_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact {
    name: SPRITE64_WORKLIST_RGBA8_KERNEL_NAME,
    target: "adls",
    bin: SPRITE64_WORKLIST_RGBA8_ADLS_BIN,
    spv: SPRITE64_WORKLIST_RGBA8_ADLS_SPV,
    bin_sha256: SPRITE64_WORKLIST_RGBA8_ADLS_BIN_SHA256,
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

pub(crate) fn copy_rect_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *COPY_RECT_RGBA8_UPLOAD.lock()
}

pub(crate) fn blit_rgba8_nearest_upload_status() -> Option<UploadedKernelArtifact> {
    *BLIT_RGBA8_NEAREST_UPLOAD.lock()
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

pub(crate) fn upload_blit_rgba8_nearest_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *BLIT_RGBA8_NEAREST_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: blit-rgba8-nearest upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload =
        upload_artifact(dev, BLIT_RGBA8_NEAREST_ADLS_ARTIFACT, BLIT_RGBA8_NEAREST_ADLS_GPU)?;
    *BLIT_RGBA8_NEAREST_UPLOAD.lock() = Some(upload);
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

pub(crate) fn upload_alpha_blend_rgba8_over_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *ALPHA_BLEND_RGBA8_OVER_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: alpha-blend-rgba8-over upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(
        dev,
        ALPHA_BLEND_RGBA8_OVER_ADLS_ARTIFACT,
        ALPHA_BLEND_RGBA8_OVER_ADLS_GPU,
    )?;
    *ALPHA_BLEND_RGBA8_OVER_UPLOAD.lock() = Some(upload);
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

fn copy_rect_kernel_flavor_narrow() -> Option<CopyRectKernelFlavor> {
    Some(CopyRectKernelFlavor {
        upload: upload_copy_rect_rgba8_kernel()?,
        text_offset_bytes: COPY_RECT_RGBA8_TEXT_OFFSET_BYTES,
        pixels_per_lane: COPY_RECT_PIXELS_PER_LANE,
        span_pixels: COPY_RECT_SPAN_PIXELS,
        rows_per_walker: 1,
        name: COPY_RECT_RGBA8_KERNEL_NAME,
    })
}

fn blit_rgba8_nearest_kernel_flavor() -> Option<CopyRectKernelFlavor> {
    Some(CopyRectKernelFlavor {
        upload: upload_blit_rgba8_nearest_kernel()?,
        text_offset_bytes: BLIT_RGBA8_NEAREST_TEXT_OFFSET_BYTES,
        pixels_per_lane: 1,
        span_pixels: 16,
        rows_per_walker: 1,
        name: BLIT_RGBA8_NEAREST_KERNEL_NAME,
    })
}

fn alpha_blend_kernel_flavor() -> Option<CopyRectKernelFlavor> {
    Some(CopyRectKernelFlavor {
        upload: upload_alpha_blend_rgba8_over_kernel()?,
        text_offset_bytes: ALPHA_BLEND_RGBA8_OVER_TEXT_OFFSET_BYTES,
        pixels_per_lane: 1,
        span_pixels: 16,
        rows_per_walker: 1,
        name: ALPHA_BLEND_RGBA8_OVER_KERNEL_NAME,
    })
}

fn glyph_mask_kernel_flavor() -> Option<CopyRectKernelFlavor> {
    Some(CopyRectKernelFlavor {
        upload: upload_glyph_mask_rgba8_kernel()?,
        text_offset_bytes: GLYPH_MASK_RGBA8_TEXT_OFFSET_BYTES,
        pixels_per_lane: 1,
        span_pixels: 16,
        rows_per_walker: 1,
        name: GLYPH_MASK_RGBA8_KERNEL_NAME,
    })
}

fn present_rgba8_to_primary_xrgb_kernel_flavor() -> Option<CopyRectKernelFlavor> {
    Some(CopyRectKernelFlavor {
        upload: upload_present_rgba8_to_primary_xrgb_rect_kernel()?,
        text_offset_bytes: PRESENT_RGBA8_TO_PRIMARY_XRGB_RECT_TEXT_OFFSET_BYTES,
        pixels_per_lane: 1,
        span_pixels: 16,
        rows_per_walker: 1,
        name: PRESENT_RGBA8_TO_PRIMARY_XRGB_RECT_KERNEL_NAME,
    })
}

pub(crate) fn fill_rect_rgba8(dst: GpgpuRgba8Surface, rect: GpgpuRect, color_rgba: u32) -> usize {
    let Some(params) = lower_fill_rect(dst, rect, color_rgba) else {
        return 0;
    };
    submit_fill_rect_spans(dst, params)
}

#[allow(dead_code)]
pub(crate) fn copy_rect_rgba8(
    src: GpgpuRgba8Surface,
    src_rect: GpgpuRect,
    dst: GpgpuRgba8Surface,
    dst_xy: GpgpuPoint,
) -> usize {
    copy_rect_rgba8_stats(src, src_rect, dst, dst_xy).spans
}

pub(crate) fn copy_rect_rgba8_stats(
    src: GpgpuRgba8Surface,
    src_rect: GpgpuRect,
    dst: GpgpuRgba8Surface,
    dst_xy: GpgpuPoint,
) -> GpgpuSubmitStats {
    let Some(params) = lower_copy_rect(src, src_rect, dst, dst_xy) else {
        return GpgpuSubmitStats::default();
    };
    let Some(flavor) = copy_rect_kernel_flavor_narrow() else {
        return GpgpuSubmitStats::default();
    };
    submit_copy_rect_spans_with_stats(src, dst, params, flavor)
}

pub(crate) fn alpha_blend_rgba8_over_stats(
    src: GpgpuRgba8Surface,
    src_rect: GpgpuRect,
    dst: GpgpuRgba8Surface,
    dst_xy: GpgpuPoint,
) -> GpgpuSubmitStats {
    let Some(params) = lower_copy_rect(src, src_rect, dst, dst_xy) else {
        return GpgpuSubmitStats::default();
    };
    let Some(flavor) = alpha_blend_kernel_flavor() else {
        return GpgpuSubmitStats::default();
    };
    submit_copy_rect_spans_with_stats(src, dst, params, flavor)
}

pub(crate) fn alpha_blend_rects_rgba8_over_stats(copies: &[GpgpuCopyRect]) -> GpgpuSubmitStats {
    let Some(flavor) = alpha_blend_kernel_flavor() else {
        return GpgpuSubmitStats::default();
    };
    copy_rects_rgba8_stats_with_explicit_flavor(copies, flavor)
}

pub(crate) fn fill_rect_worklist_rgba8_stats(
    dst: GpgpuRgba8Surface,
    descs: &[FillRectWorklistRgba8Desc],
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
        if !submit_fill_rect_worklist(dst, desc_buffer, params) {
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

pub(crate) fn fill_rect_worklist_rgba8(
    dst: GpgpuRgba8Surface,
    rect: GpgpuRect,
    color_rgba: u32,
) -> usize {
    let Some(rect) = clip_gpgpu_rect_to_surface(rect, dst.width, dst.height) else {
        return 0;
    };
    let Ok(dst_x) = i16::try_from(rect.x) else {
        return 0;
    };
    let Ok(dst_y) = i16::try_from(rect.y) else {
        return 0;
    };
    if rect.width > u16::MAX as u32 || rect.height > u16::MAX as u32 {
        return 0;
    }
    let desc = FillRectWorklistRgba8Desc {
        dst_xy: pack_i16_pair_u32(dst_x, dst_y),
        size: pack_u16_pair_u32(rect.width as u16, rect.height as u16),
        color_rgba,
    };
    fill_rect_worklist_rgba8_stats(dst, core::slice::from_ref(&desc)).descs
}

pub(crate) fn gradient_rect_worklist_rgba8_stats(
    dst: GpgpuRgba8Surface,
    descs: &[GradientRectWorklistRgba8Desc],
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
            let out = desc_buffer.virt as *mut GradientRectWorklistRgba8Desc;
            for (index, desc) in chunk.iter().copied().enumerate() {
                core::ptr::write_volatile(out.add(index), desc);
            }
        }
        super::dma_flush(desc_buffer.virt, desc_buffer.bytes);

        let params = GradientRectWorklistRgba8Params {
            dst_gpu: dst.gpu,
            desc_gpu: desc_buffer.gpu,
            dst_pitch_bytes: dst.pitch_bytes,
            desc_base: 0,
            desc_count: chunk.len() as u32,
        };
        let submit_start_tick = direct_rcs_now_tick();
        if !submit_gradient_rect_worklist(dst, desc_buffer, params) {
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

pub(crate) fn alpha_blend_worklist_rgba8_over_stats(
    src: GpgpuRgba8Surface,
    dst: GpgpuRgba8Surface,
    descs: &[AlphaBlendWorklistRgba8Desc],
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
            let out = desc_buffer.virt as *mut AlphaBlendWorklistRgba8Desc;
            for (index, desc) in chunk.iter().copied().enumerate() {
                core::ptr::write_volatile(out.add(index), desc);
            }
        }
        super::dma_flush(desc_buffer.virt, desc_buffer.bytes);

        let params = AlphaBlendWorklistRgba8Params {
            src_gpu: src.gpu,
            dst_gpu: dst.gpu,
            desc_gpu: desc_buffer.gpu,
            src_pitch_bytes: src.pitch_bytes,
            dst_pitch_bytes: dst.pitch_bytes,
            desc_base: 0,
            desc_count: chunk.len() as u32,
        };
        let submit_start_tick = direct_rcs_now_tick();
        if !submit_alpha_blend_worklist(src, dst, desc_buffer, params) {
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

pub(crate) fn alpha_blend_worklist_rgba8_over_submit_stats(
    src: GpgpuRgba8Surface,
    src_rect: GpgpuRect,
    dst: GpgpuRgba8Surface,
    dst_xy: GpgpuPoint,
) -> GpgpuSubmitStats {
    alpha_blend_worklist_rgba8_over_submit_stats_with_flags(
        src,
        src_rect,
        dst,
        dst_xy,
        COMPOSITE_WORKLIST_FLAG_SRC_OVER,
        COMPOSITE_WORKLIST_NEUTRAL_COLOR_RGBA,
    )
}

pub(crate) fn alpha_blend_worklist_rgba8_over_submit_stats_with_flags(
    src: GpgpuRgba8Surface,
    src_rect: GpgpuRect,
    dst: GpgpuRgba8Surface,
    dst_xy: GpgpuPoint,
    flags: u32,
    color_rgba: u32,
) -> GpgpuSubmitStats {
    if src_rect.x < 0 || src_rect.y < 0 {
        return GpgpuSubmitStats::default();
    }
    if dst_xy.x < i16::MIN as i32 || dst_xy.x > i16::MAX as i32 {
        return GpgpuSubmitStats::default();
    }
    if dst_xy.y < i16::MIN as i32 || dst_xy.y > i16::MAX as i32 {
        return GpgpuSubmitStats::default();
    }
    if src_rect.width == 0
        || src_rect.height == 0
        || src_rect.width > u16::MAX as u32
        || src_rect.height > u16::MAX as u32
        || src_rect.x as u32 > u16::MAX as u32
        || src_rect.y as u32 > u16::MAX as u32
    {
        return GpgpuSubmitStats::default();
    }
    let desc = AlphaBlendWorklistRgba8Desc {
        src_xy: pack_u16_pair_u32(src_rect.x as u16, src_rect.y as u16),
        dst_xy: pack_i16_pair_u32(dst_xy.x as i16, dst_xy.y as i16),
        size: pack_u16_pair_u32(src_rect.width as u16, src_rect.height as u16),
        flags,
        color_rgba,
    };
    let stats = alpha_blend_worklist_rgba8_over_stats(src, dst, core::slice::from_ref(&desc));
    GpgpuSubmitStats {
        spans: stats.descs,
        submits: stats.submits,
        submit_ms: stats.submit_ms,
        present_ms: 0,
        total_ms: stats.submit_ms,
    }
}

pub(crate) fn composite_rect_rgba8_stats(op: GpgpuCompositeRect) -> GpgpuSubmitStats {
    match op.mode {
        GpgpuCompositeMode::Copy => copy_rect_rgba8_stats(op.src, op.src_rect, op.dst, op.dst_xy),
        GpgpuCompositeMode::SrcOver => {
            alpha_blend_worklist_rgba8_over_submit_stats(op.src, op.src_rect, op.dst, op.dst_xy)
        }
    }
}

pub(crate) fn composite_rects_rgba8_stats(ops: &[GpgpuCompositeRect]) -> GpgpuSubmitStats {
    if alpha_blend_worklist_probe_ok() {
        let stats = composite_rects_rgba8_worklist_stats(ops);
        if stats.submits != 0 || ops.is_empty() {
            return stats;
        }
    }

    let mut stats = GpgpuSubmitStats::default();
    for op in ops {
        let op_stats = composite_rect_rgba8_stats(*op);
        stats.spans = stats.spans.saturating_add(op_stats.spans);
        stats.submits = stats.submits.saturating_add(op_stats.submits);
        stats.submit_ms = stats.submit_ms.saturating_add(op_stats.submit_ms);
        stats.present_ms = stats.present_ms.saturating_add(op_stats.present_ms);
        stats.total_ms = stats.total_ms.saturating_add(op_stats.total_ms);
    }
    stats
}

fn composite_rects_rgba8_worklist_stats(ops: &[GpgpuCompositeRect]) -> GpgpuSubmitStats {
    let total_start_tick = direct_rcs_now_tick();
    let mut stats = GpgpuSubmitStats::default();
    let mut index = 0usize;

    while index < ops.len() {
        let first = ops[index];
        let mut descs = Vec::new();
        let mut dst_rects = Vec::new();

        while index < ops.len() {
            let op = ops[index];
            if !same_rgba8_surface(first.src, op.src) || !same_rgba8_surface(first.dst, op.dst) {
                break;
            }

            let Some((desc, dst_rect)) = lower_composite_worklist_desc(op) else {
                if descs.is_empty() {
                    let op_stats = composite_rect_rgba8_stats(op);
                    stats.spans = stats.spans.saturating_add(op_stats.spans);
                    stats.submits = stats.submits.saturating_add(op_stats.submits);
                    stats.submit_ms = stats.submit_ms.saturating_add(op_stats.submit_ms);
                    stats.present_ms = stats.present_ms.saturating_add(op_stats.present_ms);
                    stats.total_ms = direct_rcs_elapsed_ms_since(total_start_tick);
                    index = index.saturating_add(1);
                }
                break;
            };

            if dst_rects
                .iter()
                .copied()
                .any(|existing| rects_overlap(existing, dst_rect))
            {
                break;
            }

            descs.push(desc);
            dst_rects.push(dst_rect);
            index = index.saturating_add(1);
        }

        if !descs.is_empty() {
            let worklist_stats =
                alpha_blend_worklist_rgba8_over_stats(first.src, first.dst, descs.as_slice());
            if worklist_stats.submits == 0 || worklist_stats.descs != descs.len() {
                break;
            }
            stats.spans = stats.spans.saturating_add(worklist_stats.descs);
            stats.submits = stats.submits.saturating_add(worklist_stats.submits);
            stats.submit_ms = stats.submit_ms.saturating_add(worklist_stats.submit_ms);
            stats.total_ms = direct_rcs_elapsed_ms_since(total_start_tick);
        }
    }

    stats.total_ms = direct_rcs_elapsed_ms_since(total_start_tick);
    stats
}

fn lower_composite_worklist_desc(
    op: GpgpuCompositeRect,
) -> Option<(AlphaBlendWorklistRgba8Desc, GpgpuRect)> {
    if same_rgba8_surface(op.src, op.dst) {
        return None;
    }

    let params = lower_copy_rect(op.src, op.src_rect, op.dst, op.dst_xy)?;
    if params.src_x > u16::MAX as u32
        || params.src_y > u16::MAX as u32
        || params.dst_x > i16::MAX as u32
        || params.dst_y > i16::MAX as u32
        || params.width > u16::MAX as u32
        || params.height > u16::MAX as u32
    {
        return None;
    }

    let flags = match op.mode {
        GpgpuCompositeMode::Copy => COMPOSITE_WORKLIST_FLAG_COPY,
        GpgpuCompositeMode::SrcOver => COMPOSITE_WORKLIST_FLAG_SRC_OVER,
    };
    let dst_rect =
        GpgpuRect::new(params.dst_x as i32, params.dst_y as i32, params.width, params.height);
    Some((
        AlphaBlendWorklistRgba8Desc {
            src_xy: pack_u16_pair_u32(params.src_x as u16, params.src_y as u16),
            dst_xy: pack_i16_pair_u32(params.dst_x as i16, params.dst_y as i16),
            size: pack_u16_pair_u32(params.width as u16, params.height as u16),
            flags,
            color_rgba: COMPOSITE_WORKLIST_NEUTRAL_COLOR_RGBA,
        },
        dst_rect,
    ))
}

pub(crate) fn fill_rect_worklist_probe_ok() -> bool {
    FILL_RECT_WORKLIST_OK.load(Ordering::Acquire)
}

pub(crate) fn fill_rect_worklist_probe_ran() -> bool {
    FILL_RECT_WORKLIST_RAN.load(Ordering::Acquire)
}

pub(crate) fn gradient_rect_worklist_probe_ok() -> bool {
    GRADIENT_RECT_WORKLIST_OK.load(Ordering::Acquire)
}

pub(crate) fn gradient_rect_worklist_probe_ran() -> bool {
    GRADIENT_RECT_WORKLIST_RAN.load(Ordering::Acquire)
}

pub(crate) fn alpha_blend_worklist_probe_ok() -> bool {
    ALPHA_BLEND_WORKLIST_OK.load(Ordering::Acquire)
}

pub(crate) fn alpha_blend_worklist_probe_ran() -> bool {
    ALPHA_BLEND_WORKLIST_RAN.load(Ordering::Acquire)
}

pub(crate) fn rect_worklist_probe_ready() -> bool {
    fill_rect_worklist_probe_ok() && alpha_blend_worklist_probe_ok()
}

pub(crate) fn present_primary_external_write(reason: &str) -> Option<u64> {
    let target = super::display::primary_surface_gpgpu_marker_target()?;
    let present_start_tick = direct_rcs_now_tick();
    if !super::display::notify_primary_surface_external_write(reason, 0, target.byte_len) {
        return None;
    }
    Some(direct_rcs_elapsed_ms_since(present_start_tick))
}

pub(crate) fn cpu_solid_rects_rgba8_over_primary(
    rects: &[GpgpuSolidRect],
) -> Option<GpgpuSolidRectOverlayResult> {
    if rects
        .iter()
        .any(|rect| rgba8_alpha(rect.color_rgba) != 0xFF)
    {
        return None;
    }

    let total_start_tick = direct_rcs_now_tick();
    let target = super::display::primary_surface_gpgpu_marker_target()?;
    if target.virt.is_null() || rects.is_empty() {
        return None;
    }

    let mut clipped = Vec::with_capacity(rects.len());
    for rect in rects {
        let Some(clipped_rect) = clip_gpgpu_rect_to_surface(rect.rect, target.width, target.height)
        else {
            continue;
        };
        if clipped_rect.width == 0 || clipped_rect.height == 0 {
            continue;
        }
        clipped.push((*rect, clipped_rect));
    }

    if clipped.is_empty() {
        return Some(GpgpuSolidRectOverlayResult {
            ok: true,
            ..GpgpuSolidRectOverlayResult::default()
        });
    }

    let fill_start_tick = direct_rcs_now_tick();
    unsafe {
        for (solid, rect) in clipped.iter().copied() {
            let y0 = rect.y.max(0) as usize;
            let x0 = rect.x.max(0) as usize;
            let width = rect.width as usize;
            let height = rect.height as usize;
            for y in 0..height {
                let row = target
                    .virt
                    .add((y0 + y).saturating_mul(target.pitch_bytes as usize))
                    as *mut u32;
                for x in 0..width {
                    core::ptr::write_volatile(row.add(x0 + x), solid.color_rgba);
                }
            }
        }
    }
    let fill_ms = direct_rcs_elapsed_ms_since(fill_start_tick);

    Some(GpgpuSolidRectOverlayResult {
        ok: true,
        rects: clipped.len(),
        fill_descs: clipped.len(),
        fill_walkers: 0,
        fill_submits: 0,
        fill_ms,
        blend_descs: 0,
        blend_walkers: 0,
        blend_submits: 0,
        blend_ms: 0,
        presented: false,
        present_ms: 0,
        total_ms: direct_rcs_elapsed_ms_since(total_start_tick),
    })
}

pub(crate) fn present_rgba8_to_primary_xrgb_rect_stats(
    src: GpgpuRgba8Surface,
    src_rect: GpgpuRect,
    dst: GpgpuRgba8Surface,
    dst_xy: GpgpuPoint,
    flip_y: bool,
) -> GpgpuSubmitStats {
    let Some(params) = lower_present_rgba8_to_primary_xrgb_rect(src, src_rect, dst, dst_xy, flip_y)
    else {
        return GpgpuSubmitStats::default();
    };
    let Some(flavor) = present_rgba8_to_primary_xrgb_kernel_flavor() else {
        return GpgpuSubmitStats::default();
    };
    submit_present_rgba8_to_primary_xrgb_spans_with_stats(src, dst, params, flavor)
}

pub(crate) fn present_rgba8_rect_to_primary_xrgb_stats(
    src: GpgpuRgba8Surface,
    src_rect: GpgpuRect,
    dst_xy: GpgpuPoint,
) -> Option<GpgpuSubmitStats> {
    let target = super::display::primary_surface_gpgpu_marker_target()?;
    let primary = GpgpuRgba8Surface::new(
        target.phys,
        target.gpu,
        target.byte_len,
        target.width,
        target.height,
        target.pitch_bytes,
    )?;
    let stats = present_rgba8_to_primary_xrgb_rect_stats(src, src_rect, primary, dst_xy, true);
    if stats.spans == 0 || stats.submits == 0 {
        return None;
    }

    let x = dst_xy.x.max(0) as usize;
    let y = dst_xy.y.max(0) as usize;
    let flush_offset = y
        .saturating_mul(primary.pitch_bytes as usize)
        .saturating_add(x.saturating_mul(core::mem::size_of::<u32>()));
    let flush_bytes = (src_rect.height as usize)
        .saturating_sub(1)
        .saturating_mul(primary.pitch_bytes as usize)
        .saturating_add((src_rect.width as usize).saturating_mul(core::mem::size_of::<u32>()));
    if !super::display::notify_primary_surface_external_write(
        "gpgpu-present-rgba-rect",
        flush_offset,
        flush_bytes,
    ) {
        return None;
    }
    Some(stats)
}

pub(crate) fn alpha_blend_rgba8_over_primary_stats(
    src: GpgpuRgba8Surface,
    src_rect: GpgpuRect,
    dst_xy: GpgpuPoint,
) -> Option<GpgpuSubmitStats> {
    alpha_blend_rgba8_over_primary_with_flags_stats(
        src,
        src_rect,
        dst_xy,
        COMPOSITE_WORKLIST_FLAG_SRC_OVER,
        COMPOSITE_WORKLIST_NEUTRAL_COLOR_RGBA,
    )
}

pub(crate) fn alpha_blend_rgba8_over_primary_with_flags_stats(
    src: GpgpuRgba8Surface,
    src_rect: GpgpuRect,
    dst_xy: GpgpuPoint,
    flags: u32,
    color_rgba: u32,
) -> Option<GpgpuSubmitStats> {
    alpha_blend_rgba8_tiled_over_primary_with_flags_stats(
        src,
        src_rect,
        dst_xy,
        src_rect.width,
        src_rect.height,
        flags,
        color_rgba,
    )
}

pub(crate) fn blit_rgba8_nearest_to_primary_stats(
    src: GpgpuRgba8Surface,
    src_rect: GpgpuRect,
    dst_rect: GpgpuRect,
) -> Option<GpgpuSubmitStats> {
    let total_start_tick = direct_rcs_now_tick();
    if !src.is_valid()
        || src_rect.is_empty()
        || dst_rect.is_empty()
        || src_rect.x < 0
        || src_rect.y < 0
    {
        return None;
    }

    let target = super::display::primary_surface_gpgpu_marker_target()?;
    let primary = GpgpuRgba8Surface::new(
        target.phys,
        target.gpu,
        target.byte_len,
        target.width,
        target.height,
        target.pitch_bytes,
    )?;

    let dst_x0 = (dst_rect.x as i64).max(0);
    let dst_y0 = (dst_rect.y as i64).max(0);
    let dst_x1 = (dst_rect.x as i64 + dst_rect.width as i64).min(primary.width as i64);
    let dst_y1 = (dst_rect.y as i64 + dst_rect.height as i64).min(primary.height as i64);
    if dst_x1 <= dst_x0 || dst_y1 <= dst_y0 {
        return None;
    }

    let clip_x = (dst_x0 - dst_rect.x as i64) as u32;
    let clip_y = (dst_y0 - dst_rect.y as i64) as u32;
    let clip_w = (dst_x1 - dst_x0) as u32;
    let clip_h = (dst_y1 - dst_y0) as u32;
    let src_x = src_rect.x as u32
        + ((clip_x as u64 * src_rect.width as u64) / dst_rect.width as u64) as u32;
    let src_y = src_rect.y as u32
        + ((clip_y as u64 * src_rect.height as u64) / dst_rect.height as u64) as u32;
    let src_x1 = src_rect.x as u32
        + (((clip_x as u64 + clip_w as u64) * src_rect.width as u64 + dst_rect.width as u64 - 1)
            / dst_rect.width as u64) as u32;
    let src_y1 = src_rect.y as u32
        + (((clip_y as u64 + clip_h as u64) * src_rect.height as u64 + dst_rect.height as u64 - 1)
            / dst_rect.height as u64) as u32;
    let src_w = src_x1.saturating_sub(src_x).max(1);
    let src_h = src_y1.saturating_sub(src_y).max(1);

    if src_x >= src.width
        || src_y >= src.height
        || src_x.saturating_add(src_w) > src.width
        || src_y.saturating_add(src_h) > src.height
    {
        return None;
    }

    let params = BlitRgba8NearestParams {
        src_gpu: src.gpu,
        dst_gpu: primary.gpu,
        src_pitch_bytes: src.pitch_bytes,
        dst_pitch_bytes: primary.pitch_bytes,
        src_x,
        src_y,
        src_width: src_w,
        src_height: src_h,
        dst_x: dst_x0 as u32,
        dst_y: dst_y0 as u32,
        dst_width: clip_w,
        dst_height: clip_h,
    };
    let flavor = blit_rgba8_nearest_kernel_flavor()?;
    let mut stats = submit_blit_rgba8_nearest_spans_with_stats(src, primary, params, flavor);
    if stats.spans == 0 || stats.submits == 0 {
        return None;
    }

    let flush_offset = (dst_y0 as usize)
        .saturating_mul(primary.pitch_bytes as usize)
        .saturating_add((dst_x0 as usize).saturating_mul(core::mem::size_of::<u32>()));
    let flush_bytes = (clip_h as usize)
        .saturating_sub(1)
        .saturating_mul(primary.pitch_bytes as usize)
        .saturating_add((clip_w as usize).saturating_mul(core::mem::size_of::<u32>()));
    let present_start_tick = direct_rcs_now_tick();
    if !super::display::notify_primary_surface_external_write(
        "gpgpu-blit-nearest-primary",
        flush_offset,
        flush_bytes,
    ) {
        return None;
    }
    stats.present_ms = direct_rcs_elapsed_ms_since(present_start_tick);
    stats.total_ms = direct_rcs_elapsed_ms_since(total_start_tick);
    Some(stats)
}

pub(crate) fn alpha_blend_rgba8_tiled_over_primary_with_flags_stats(
    src: GpgpuRgba8Surface,
    src_rect: GpgpuRect,
    dst_xy: GpgpuPoint,
    tile_width: u32,
    tile_height: u32,
    flags: u32,
    color_rgba: u32,
) -> Option<GpgpuSubmitStats> {
    let total_start_tick = direct_rcs_now_tick();
    if src_rect.is_empty()
        || src_rect.x < 0
        || src_rect.y < 0
        || tile_width == 0
        || tile_height == 0
        || dst_xy.x < i16::MIN as i32
        || dst_xy.x > i16::MAX as i32
        || dst_xy.y < i16::MIN as i32
        || dst_xy.y > i16::MAX as i32
    {
        return None;
    }

    let target = super::display::primary_surface_gpgpu_marker_target()?;
    let primary = GpgpuRgba8Surface::new(
        target.phys,
        target.gpu,
        target.byte_len,
        target.width,
        target.height,
        target.pitch_bytes,
    )?;

    let mut descs = Vec::new();
    let mut y = 0u32;
    while y < src_rect.height {
        let h = tile_height.min(src_rect.height.saturating_sub(y));
        let mut x = 0u32;
        while x < src_rect.width {
            let w = tile_width.min(src_rect.width.saturating_sub(x));
            let sx = (src_rect.x as u32).saturating_add(x);
            let sy = (src_rect.y as u32).saturating_add(y);
            let dx = dst_xy.x.saturating_add(x as i32);
            let dy = dst_xy.y.saturating_add(y as i32);
            if sx <= u16::MAX as u32
                && sy <= u16::MAX as u32
                && w <= u16::MAX as u32
                && h <= u16::MAX as u32
                && dx >= i16::MIN as i32
                && dx <= i16::MAX as i32
                && dy >= i16::MIN as i32
                && dy <= i16::MAX as i32
            {
                descs.push(AlphaBlendWorklistRgba8Desc {
                    src_xy: pack_u16_pair_u32(sx as u16, sy as u16),
                    dst_xy: pack_i16_pair_u32(dx as i16, dy as i16),
                    size: pack_u16_pair_u32(w as u16, h as u16),
                    flags,
                    color_rgba,
                });
            }
            x = x.saturating_add(w);
        }
        y = y.saturating_add(h);
    }

    if descs.is_empty() {
        return None;
    }

    let worklist_stats = alpha_blend_worklist_rgba8_over_stats(src, primary, descs.as_slice());
    let mut stats = GpgpuSubmitStats {
        spans: worklist_stats.descs,
        submits: worklist_stats.submits,
        submit_ms: worklist_stats.submit_ms,
        present_ms: 0,
        total_ms: 0,
    };
    if stats.spans == 0 || stats.submits == 0 {
        return None;
    }

    let x = dst_xy.x.max(0) as usize;
    let y = dst_xy.y.max(0) as usize;
    let flush_offset = y
        .saturating_mul(primary.pitch_bytes as usize)
        .saturating_add(x.saturating_mul(core::mem::size_of::<u32>()));
    let flush_bytes = (src_rect.height as usize)
        .saturating_sub(1)
        .saturating_mul(primary.pitch_bytes as usize)
        .saturating_add((src_rect.width as usize).saturating_mul(core::mem::size_of::<u32>()));
    let present_start_tick = direct_rcs_now_tick();
    if !super::display::notify_primary_surface_external_write(
        "gpgpu-alpha-over-primary",
        flush_offset,
        flush_bytes,
    ) {
        return None;
    }
    stats.present_ms = direct_rcs_elapsed_ms_since(present_start_tick);
    stats.total_ms = direct_rcs_elapsed_ms_since(total_start_tick);
    Some(stats)
}

pub(crate) fn solid_rects_rgba8_over_primary(
    rects: &[GpgpuSolidRect],
    present: bool,
) -> Option<GpgpuSolidRectOverlayResult> {
    if !rect_worklist_probe_ready() {
        let log_n = RECT_WORKLIST_NOT_READY_LOGS.fetch_add(1, Ordering::Relaxed);
        if log_n < 8 {
            crate::log!(
                "intel/gpgpu: solid-rects-worklist skipped reason=probe-not-ready fill_ran={} fill_ok={} alpha_ran={} alpha_ok={} rects={} present={}\n",
                fill_rect_worklist_probe_ran() as u8,
                fill_rect_worklist_probe_ok() as u8,
                alpha_blend_worklist_probe_ran() as u8,
                alpha_blend_worklist_probe_ok() as u8,
                rects.len(),
                present as u8
            );
        }
        return None;
    }
    let total_start_tick = direct_rcs_now_tick();
    let target = super::display::primary_surface_gpgpu_marker_target()?;
    if target.virt.is_null() || rects.is_empty() {
        return None;
    }
    let primary = GpgpuRgba8Surface::new(
        target.phys,
        target.gpu,
        target.byte_len,
        target.width,
        target.height,
        target.pitch_bytes,
    )?;

    let mut clipped = Vec::with_capacity(rects.len());
    let mut packed_h = 0u32;
    for rect in rects {
        let Some(clipped_rect) =
            clip_gpgpu_rect_to_surface(rect.rect, primary.width, primary.height)
        else {
            continue;
        };
        if clipped_rect.width == 0 || clipped_rect.height == 0 {
            continue;
        }
        let src_y = packed_h;
        packed_h = packed_h.checked_add(clipped_rect.height)?;
        clipped.push((*rect, clipped_rect, src_y));
    }
    if clipped.is_empty() || packed_h == 0 {
        return Some(GpgpuSolidRectOverlayResult {
            ok: true,
            ..GpgpuSolidRectOverlayResult::default()
        });
    }

    let opaque = clipped
        .iter()
        .all(|(solid, _, _)| rgba8_alpha(solid.color_rgba) == 0xFF);
    if opaque {
        let mut fill_descs = Vec::with_capacity(clipped.len());
        for (solid, dst_rect, _) in clipped.iter().copied() {
            if dst_rect.width > u16::MAX as u32 || dst_rect.height > u16::MAX as u32 {
                continue;
            }
            let Ok(dst_x_i16) = i16::try_from(dst_rect.x) else {
                continue;
            };
            let Ok(dst_y_i16) = i16::try_from(dst_rect.y) else {
                continue;
            };
            fill_descs.push(FillRectWorklistRgba8Desc {
                dst_xy: pack_i16_pair_u32(dst_x_i16, dst_y_i16),
                size: pack_u16_pair_u32(dst_rect.width as u16, dst_rect.height as u16),
                color_rgba: solid.color_rgba,
            });
        }
        if fill_descs.is_empty() {
            return Some(GpgpuSolidRectOverlayResult {
                ok: true,
                ..GpgpuSolidRectOverlayResult::default()
            });
        }

        let fill_start_tick = direct_rcs_now_tick();
        let fill = fill_rect_worklist_rgba8_stats(primary, fill_descs.as_slice());
        let fill_ms = direct_rcs_elapsed_ms_since(fill_start_tick);
        let present_start_tick = direct_rcs_now_tick();
        let presented = if present && fill.submits > 0 {
            super::display::notify_primary_surface_external_write(
                "gpgpu-solid-rects-direct-primary",
                0,
                target.byte_len,
            )
        } else {
            false
        };
        let present_ms = direct_rcs_elapsed_ms_since(present_start_tick);

        return Some(GpgpuSolidRectOverlayResult {
            ok: fill.submits > 0 && (!present || presented),
            rects: fill_descs.len(),
            fill_descs: fill.descs,
            fill_walkers: fill.walkers,
            fill_submits: fill.submits,
            fill_ms,
            blend_descs: 0,
            blend_walkers: 0,
            blend_submits: 0,
            blend_ms: 0,
            presented,
            present_ms,
            total_ms: direct_rcs_elapsed_ms_since(total_start_tick),
        });
    }

    let source = solid_rect_source_surface_once(primary.width, packed_h)?;
    let mut fill_descs = Vec::with_capacity(clipped.len());
    let mut blend_descs = Vec::with_capacity(clipped.len());
    for (solid, dst_rect, src_y) in clipped.iter().copied() {
        if dst_rect.width > u16::MAX as u32 || dst_rect.height > u16::MAX as u32 {
            continue;
        }
        let Ok(src_y_i16) = i16::try_from(src_y) else {
            continue;
        };
        let Ok(src_y_u16) = u16::try_from(src_y) else {
            continue;
        };
        let Ok(dst_x_i16) = i16::try_from(dst_rect.x) else {
            continue;
        };
        let Ok(dst_y_i16) = i16::try_from(dst_rect.y) else {
            continue;
        };
        let width = dst_rect.width as u16;
        let height = dst_rect.height as u16;
        fill_descs.push(FillRectWorklistRgba8Desc {
            dst_xy: pack_i16_pair_u32(0, src_y_i16),
            size: pack_u16_pair_u32(width, height),
            color_rgba: solid.color_rgba,
        });
        blend_descs.push(AlphaBlendWorklistRgba8Desc {
            src_xy: pack_u16_pair_u32(0, src_y_u16),
            dst_xy: pack_i16_pair_u32(dst_x_i16, dst_y_i16),
            size: pack_u16_pair_u32(width, height),
            flags: COMPOSITE_WORKLIST_FLAG_SRC_OVER,
            color_rgba: COMPOSITE_WORKLIST_NEUTRAL_COLOR_RGBA,
        });
    }
    if fill_descs.is_empty() || blend_descs.is_empty() {
        return Some(GpgpuSolidRectOverlayResult {
            ok: true,
            ..GpgpuSolidRectOverlayResult::default()
        });
    }

    let fill_start_tick = direct_rcs_now_tick();
    let fill = fill_rect_worklist_rgba8_stats(source.surface, fill_descs.as_slice());
    let fill_ms = direct_rcs_elapsed_ms_since(fill_start_tick);
    let blend_start_tick = direct_rcs_now_tick();
    let blend =
        alpha_blend_worklist_rgba8_over_stats(source.surface, primary, blend_descs.as_slice());
    let blend_ms = direct_rcs_elapsed_ms_since(blend_start_tick);
    let present_start_tick = direct_rcs_now_tick();
    let presented = if present && blend.submits > 0 {
        super::display::notify_primary_surface_external_write(
            "gpgpu-solid-rects-over-primary",
            0,
            target.byte_len,
        )
    } else {
        false
    };
    let present_ms = direct_rcs_elapsed_ms_since(present_start_tick);

    Some(GpgpuSolidRectOverlayResult {
        ok: fill.submits > 0 && blend.submits > 0 && (!present || presented),
        rects: blend_descs.len(),
        fill_descs: fill.descs,
        fill_walkers: fill.walkers,
        fill_submits: fill.submits,
        fill_ms,
        blend_descs: blend.descs,
        blend_walkers: blend.walkers,
        blend_submits: blend.submits,
        blend_ms,
        presented,
        present_ms,
        total_ms: direct_rcs_elapsed_ms_since(total_start_tick),
    })
}

pub(crate) fn gradient_rects_rgba8_over_primary(
    rects: &[GpgpuGradientRect],
    present: bool,
) -> Option<GpgpuSolidRectOverlayResult> {
    if !rect_worklist_probe_ready() || !gradient_rect_worklist_probe_ok() {
        return None;
    }
    let total_start_tick = direct_rcs_now_tick();
    let target = super::display::primary_surface_gpgpu_marker_target()?;
    if target.virt.is_null() || rects.is_empty() {
        return None;
    }
    let primary = GpgpuRgba8Surface::new(
        target.phys,
        target.gpu,
        target.byte_len,
        target.width,
        target.height,
        target.pitch_bytes,
    )?;

    let mut clipped = Vec::with_capacity(rects.len());
    let mut packed_h = 0u32;
    for rect in rects {
        let Some(clipped_rect) =
            clip_gpgpu_rect_to_surface(rect.rect, primary.width, primary.height)
        else {
            continue;
        };
        if clipped_rect.width == 0 || clipped_rect.height == 0 {
            continue;
        }
        let src_y = packed_h;
        packed_h = packed_h.checked_add(clipped_rect.height)?;
        clipped.push((*rect, clipped_rect, src_y));
    }
    if clipped.is_empty() || packed_h == 0 {
        return Some(GpgpuSolidRectOverlayResult {
            ok: true,
            ..GpgpuSolidRectOverlayResult::default()
        });
    }

    let opaque = clipped.iter().all(|(gradient, _, _)| {
        rgba8_alpha(gradient.color0_rgba) == 0xFF && rgba8_alpha(gradient.color1_rgba) == 0xFF
    });
    if opaque {
        let mut gradient_descs = Vec::with_capacity(clipped.len());
        for (gradient, dst_rect, _) in clipped.iter().copied() {
            if dst_rect.width > u16::MAX as u32 || dst_rect.height > u16::MAX as u32 {
                continue;
            }
            let Ok(dst_x_i16) = i16::try_from(dst_rect.x) else {
                continue;
            };
            let Ok(dst_y_i16) = i16::try_from(dst_rect.y) else {
                continue;
            };
            gradient_descs.push(GradientRectWorklistRgba8Desc {
                dst_xy: pack_i16_pair_u32(dst_x_i16, dst_y_i16),
                size: pack_u16_pair_u32(dst_rect.width as u16, dst_rect.height as u16),
                color0_rgba: gradient.color0_rgba,
                color1_rgba: gradient.color1_rgba,
                flags: if gradient.vertical {
                    GRADIENT_RECT_WORKLIST_FLAG_VERTICAL
                } else {
                    0
                },
            });
        }
        if gradient_descs.is_empty() {
            return Some(GpgpuSolidRectOverlayResult {
                ok: true,
                ..GpgpuSolidRectOverlayResult::default()
            });
        }

        let fill_start_tick = direct_rcs_now_tick();
        let fill = gradient_rect_worklist_rgba8_stats(primary, gradient_descs.as_slice());
        let fill_ms = direct_rcs_elapsed_ms_since(fill_start_tick);
        let present_start_tick = direct_rcs_now_tick();
        let presented = if present && fill.submits > 0 {
            super::display::notify_primary_surface_external_write(
                "gpgpu-gradient-rects-direct-primary",
                0,
                target.byte_len,
            )
        } else {
            false
        };
        let present_ms = direct_rcs_elapsed_ms_since(present_start_tick);

        return Some(GpgpuSolidRectOverlayResult {
            ok: fill.submits > 0 && (!present || presented),
            rects: gradient_descs.len(),
            fill_descs: fill.descs,
            fill_walkers: fill.walkers,
            fill_submits: fill.submits,
            fill_ms,
            blend_descs: 0,
            blend_walkers: 0,
            blend_submits: 0,
            blend_ms: 0,
            presented,
            present_ms,
            total_ms: direct_rcs_elapsed_ms_since(total_start_tick),
        });
    }

    let source = solid_rect_source_surface_once(primary.width, packed_h)?;
    let mut gradient_descs = Vec::with_capacity(clipped.len());
    let mut blend_descs = Vec::with_capacity(clipped.len());
    for (gradient, dst_rect, src_y) in clipped.iter().copied() {
        if dst_rect.width > u16::MAX as u32 || dst_rect.height > u16::MAX as u32 {
            continue;
        }
        let Ok(src_y_i16) = i16::try_from(src_y) else {
            continue;
        };
        let Ok(src_y_u16) = u16::try_from(src_y) else {
            continue;
        };
        let Ok(dst_x_i16) = i16::try_from(dst_rect.x) else {
            continue;
        };
        let Ok(dst_y_i16) = i16::try_from(dst_rect.y) else {
            continue;
        };
        let width = dst_rect.width as u16;
        let height = dst_rect.height as u16;
        gradient_descs.push(GradientRectWorklistRgba8Desc {
            dst_xy: pack_i16_pair_u32(0, src_y_i16),
            size: pack_u16_pair_u32(width, height),
            color0_rgba: gradient.color0_rgba,
            color1_rgba: gradient.color1_rgba,
            flags: if gradient.vertical {
                GRADIENT_RECT_WORKLIST_FLAG_VERTICAL
            } else {
                0
            },
        });
        blend_descs.push(AlphaBlendWorklistRgba8Desc {
            src_xy: pack_u16_pair_u32(0, src_y_u16),
            dst_xy: pack_i16_pair_u32(dst_x_i16, dst_y_i16),
            size: pack_u16_pair_u32(width, height),
            flags: COMPOSITE_WORKLIST_FLAG_SRC_OVER,
            color_rgba: COMPOSITE_WORKLIST_NEUTRAL_COLOR_RGBA,
        });
    }
    if gradient_descs.is_empty() || blend_descs.is_empty() {
        return Some(GpgpuSolidRectOverlayResult {
            ok: true,
            ..GpgpuSolidRectOverlayResult::default()
        });
    }

    let fill_start_tick = direct_rcs_now_tick();
    let fill = gradient_rect_worklist_rgba8_stats(source.surface, gradient_descs.as_slice());
    let fill_ms = direct_rcs_elapsed_ms_since(fill_start_tick);
    let blend_start_tick = direct_rcs_now_tick();
    let blend =
        alpha_blend_worklist_rgba8_over_stats(source.surface, primary, blend_descs.as_slice());
    let blend_ms = direct_rcs_elapsed_ms_since(blend_start_tick);
    let present_start_tick = direct_rcs_now_tick();
    let presented = if present && blend.submits > 0 {
        super::display::notify_primary_surface_external_write(
            "gpgpu-gradient-rects-over-primary",
            0,
            target.byte_len,
        )
    } else {
        false
    };
    let present_ms = direct_rcs_elapsed_ms_since(present_start_tick);

    Some(GpgpuSolidRectOverlayResult {
        ok: fill.submits > 0 && blend.submits > 0 && (!present || presented),
        rects: blend_descs.len(),
        fill_descs: fill.descs,
        fill_walkers: fill.walkers,
        fill_submits: fill.submits,
        fill_ms,
        blend_descs: blend.descs,
        blend_walkers: blend.walkers,
        blend_submits: blend.submits,
        blend_ms,
        presented,
        present_ms,
        total_ms: direct_rcs_elapsed_ms_since(total_start_tick),
    })
}

pub(crate) fn glyph_mask_rgba8_stats(blit: GpgpuGlyphMaskBlit) -> GpgpuSubmitStats {
    let Some(params) = lower_glyph_mask_blit(blit) else {
        return GpgpuSubmitStats::default();
    };
    let Some(flavor) = glyph_mask_kernel_flavor() else {
        return GpgpuSubmitStats::default();
    };
    submit_glyph_mask_spans_with_stats(blit.mask, blit.dst, params, blit.color_rgba, flavor)
}

pub(crate) fn glyph_mask_rgba8_over_primary_stats(
    mask: GpgpuMask8Surface,
    mask_rect: GpgpuRect,
    dst_xy: GpgpuPoint,
    color_rgba: u32,
) -> Option<GpgpuSubmitStats> {
    let total_start_tick = direct_rcs_now_tick();
    let target = super::display::primary_surface_gpgpu_marker_target()?;
    let primary = GpgpuRgba8Surface::new(
        target.phys,
        target.gpu,
        target.byte_len,
        target.width,
        target.height,
        target.pitch_bytes,
    )?;
    let mut stats = glyph_mask_rgba8_stats(GpgpuGlyphMaskBlit {
        mask,
        mask_rect,
        dst: primary,
        dst_xy,
        color_rgba,
    });
    if stats.spans == 0 || stats.submits == 0 {
        return None;
    }

    let x = dst_xy.x.max(0) as usize;
    let y = dst_xy.y.max(0) as usize;
    let flush_offset = y
        .saturating_mul(primary.pitch_bytes as usize)
        .saturating_add(x.saturating_mul(core::mem::size_of::<u32>()));
    let flush_bytes = (mask_rect.height as usize)
        .saturating_sub(1)
        .saturating_mul(primary.pitch_bytes as usize)
        .saturating_add((mask_rect.width as usize).saturating_mul(core::mem::size_of::<u32>()));
    let present_start_tick = direct_rcs_now_tick();
    if !super::display::notify_primary_surface_external_write(
        "gpgpu-glyph-mask-primary",
        flush_offset,
        flush_bytes,
    ) {
        return None;
    }
    stats.present_ms = direct_rcs_elapsed_ms_since(present_start_tick);
    stats.total_ms = direct_rcs_elapsed_ms_since(total_start_tick);
    Some(stats)
}

#[allow(dead_code)]
pub(crate) fn blit_glyph_rgba8(blit: GpgpuGlyphBlit) -> usize {
    blit_glyph_rgba8_stats(blit).spans
}

pub(crate) fn blit_glyph_rgba8_stats(blit: GpgpuGlyphBlit) -> GpgpuSubmitStats {
    copy_rect_rgba8_stats(blit.atlas, blit.glyph_rect, blit.dst, blit.dst_xy)
}

#[allow(dead_code)]
pub(crate) fn copy_rects_rgba8(copies: &[GpgpuCopyRect]) -> usize {
    copy_rects_rgba8_stats(copies).spans
}

pub(crate) fn copy_rects_rgba8_stats(copies: &[GpgpuCopyRect]) -> GpgpuSubmitStats {
    let Some(flavor) = copy_rect_kernel_flavor_narrow() else {
        return GpgpuSubmitStats::default();
    };
    copy_rects_rgba8_stats_with_explicit_flavor(copies, flavor)
}

fn display_rgba8_surface_from_target(
    target: super::display::DisplayRgba8GpgpuSurface,
) -> Option<GpgpuRgba8Surface> {
    GpgpuRgba8Surface::new(
        target.phys,
        target.gpu,
        target.byte_len,
        target.width,
        target.height,
        target.pitch_bytes,
    )
}

fn clear_rgba8_surface_white_stats(dst: GpgpuRgba8Surface) -> GpgpuSubmitStats {
    let Some(rect) = clip_gpgpu_rect_to_surface(
        GpgpuRect::new(0, 0, dst.width, dst.height),
        dst.width,
        dst.height,
    ) else {
        return GpgpuSubmitStats::default();
    };
    if rect.width > u16::MAX as u32 || rect.height > u16::MAX as u32 {
        return GpgpuSubmitStats::default();
    }

    let total_start_tick = direct_rcs_now_tick();
    let mut descs = Vec::new();
    let tile_w = 128u32;
    let tile_h = 32u32;
    let mut y = rect.y;
    let y_end = rect.y.saturating_add(rect.height as i32);
    while y < y_end {
        let mut x = rect.x;
        let x_end = rect.x.saturating_add(rect.width as i32);
        let height = ((y_end - y) as u32).min(tile_h);
        while x < x_end {
            let width = ((x_end - x) as u32).min(tile_w);
            let Ok(dst_x) = i16::try_from(x) else {
                return GpgpuSubmitStats::default();
            };
            let Ok(dst_y) = i16::try_from(y) else {
                return GpgpuSubmitStats::default();
            };
            descs.push(FillRectWorklistRgba8Desc {
                dst_xy: pack_i16_pair_u32(dst_x, dst_y),
                size: pack_u16_pair_u32(width as u16, height as u16),
                color_rgba: 0xFFFF_FFFF,
            });
            x = x.saturating_add(tile_w as i32);
        }
        y = y.saturating_add(tile_h as i32);
    }
    if descs.is_empty() {
        return GpgpuSubmitStats::default();
    }

    let fill = fill_rect_worklist_rgba8_stats(dst, descs.as_slice());
    GpgpuSubmitStats {
        spans: fill.descs,
        submits: fill.submits,
        submit_ms: fill.submit_ms,
        present_ms: 0,
        total_ms: direct_rcs_elapsed_ms_since(total_start_tick),
    }
}

pub(crate) fn clear_ui2_frame_rgba8_white_stats() -> Option<GpgpuSubmitStats> {
    let target = super::display::ui2_frame_surface_gpgpu()?;
    let frame = display_rgba8_surface_from_target(target)?;
    let stats = clear_rgba8_surface_white_stats(frame);
    (stats.submits > 0).then_some(stats)
}

pub(crate) fn clear_primary_rgba8_white_stats() -> Option<GpgpuSubmitStats> {
    let total_start_tick = direct_rcs_now_tick();
    let target = super::display::primary_surface_gpgpu_marker_target()?;
    let primary = GpgpuRgba8Surface::new(
        target.phys,
        target.gpu,
        target.byte_len,
        target.width,
        target.height,
        target.pitch_bytes,
    )?;
    let mut stats = clear_rgba8_surface_white_stats(primary);
    if stats.submits == 0 {
        return None;
    }

    let present_start_tick = direct_rcs_now_tick();
    super::display::notify_primary_surface_external_write(
        "gpgpu-primary-white-clear",
        0,
        target.byte_len,
    )
    .then(|| {
        stats.present_ms = direct_rcs_elapsed_ms_since(present_start_tick);
        stats.total_ms = direct_rcs_elapsed_ms_since(total_start_tick);
        stats
    })
}

pub(crate) fn copy_ui2_base_to_primary_rgba8() -> Option<GpgpuSubmitStats> {
    let total_start_tick = direct_rcs_now_tick();
    let src_target = super::display::ui2_base_surface_gpgpu()?;
    let primary_target = super::display::primary_surface_gpgpu_marker_target()?;
    let src = GpgpuRgba8Surface::new(
        src_target.phys,
        src_target.gpu,
        src_target.byte_len,
        src_target.width,
        src_target.height,
        src_target.pitch_bytes,
    )?;
    let primary = GpgpuRgba8Surface::new(
        primary_target.phys,
        primary_target.gpu,
        primary_target.byte_len,
        primary_target.width,
        primary_target.height,
        primary_target.pitch_bytes,
    )?;
    let width = src.width.min(primary.width);
    let height = src.height.min(primary.height);
    if width == 0 || height == 0 {
        return None;
    }

    let copy = GpgpuCopyRect {
        src,
        src_rect: GpgpuRect::new(0, 0, width, height),
        dst: primary,
        dst_xy: GpgpuPoint::new(0, 0),
    };
    let mut stats = copy_rects_rgba8_stats(core::slice::from_ref(&copy));
    if stats.spans == 0 || stats.submits == 0 {
        return None;
    }
    let present_start_tick = direct_rcs_now_tick();
    super::display::notify_primary_surface_external_write(
        "gpgpu-ui2-base-copy",
        0,
        primary_target.byte_len,
    )
    .then(|| {
        stats.present_ms = direct_rcs_elapsed_ms_since(present_start_tick);
        stats.total_ms = direct_rcs_elapsed_ms_since(total_start_tick);
        stats
    })
}

pub(crate) fn present_rgba_frame_to_primary(src: &[u8], width: u32, height: u32) -> bool {
    let present_seq = PRESENT_RGBA8_TO_PRIMARY_XRGB_PRESENT_SEQ
        .fetch_add(1, Ordering::AcqRel)
        .wrapping_add(1);
    let should_log = present_seq <= 8 || present_seq.is_multiple_of(120);
    let total_start_tick = direct_rcs_now_tick();
    if width == 0 || height == 0 {
        crate::log!(
            "intel/gpgpu: present-rgba8-to-primary-xrgb-rect result=skip seq={} reason=empty-size size={}x{}\n",
            present_seq,
            width,
            height
        );
        return false;
    }
    let Some(src_pitch_bytes) = width.checked_mul(core::mem::size_of::<u32>() as u32) else {
        crate::log!(
            "intel/gpgpu: present-rgba8-to-primary-xrgb-rect result=skip seq={} reason=pitch-overflow size={}x{}\n",
            present_seq,
            width,
            height
        );
        return false;
    };
    let Some(need) = (src_pitch_bytes as usize).checked_mul(height as usize) else {
        crate::log!(
            "intel/gpgpu: present-rgba8-to-primary-xrgb-rect result=skip seq={} reason=byte-count-overflow size={}x{}\n",
            present_seq,
            width,
            height
        );
        return false;
    };
    if src.len() < need {
        crate::log!(
            "intel/gpgpu: present-rgba8-to-primary-xrgb-rect result=skip seq={} reason=short-src src_len=0x{:X} need=0x{:X} size={}x{}\n",
            present_seq,
            src.len(),
            need,
            width,
            height
        );
        return false;
    }

    let Some(staging) = present_staging_surface_once(width, height) else {
        crate::log!(
            "intel/gpgpu: present-rgba8-to-primary-xrgb-rect result=miss seq={} reason=no-staging size={}x{}\n",
            present_seq,
            width,
            height
        );
        return false;
    };
    let Some(target) = super::display::primary_surface_gpgpu_marker_target() else {
        crate::log!(
            "intel/gpgpu: present-rgba8-to-primary-xrgb-rect result=miss seq={} reason=no-primary-target size={}x{}\n",
            present_seq,
            width,
            height
        );
        return false;
    };
    let Some(primary) = GpgpuRgba8Surface::new(
        target.phys,
        target.gpu,
        target.byte_len,
        target.width,
        target.height,
        target.pitch_bytes,
    ) else {
        crate::log!(
            "intel/gpgpu: present-rgba8-to-primary-xrgb-rect result=miss seq={} reason=bad-primary-surface phys=0x{:X} gpu=0x{:X} bytes=0x{:X} size={}x{} pitch=0x{:X}\n",
            present_seq,
            target.phys,
            target.gpu,
            target.byte_len,
            target.width,
            target.height,
            target.pitch_bytes
        );
        return false;
    };

    let copy_w = width.min(primary.width);
    let copy_h = height.min(primary.height);
    if copy_w == 0 || copy_h == 0 || target.virt.is_null() {
        crate::log!(
            "intel/gpgpu: present-rgba8-to-primary-xrgb-rect result=miss seq={} reason=bad-copy-or-primary-virt src={}x{} primary={}x{} virt_null={}\n",
            present_seq,
            width,
            height,
            primary.width,
            primary.height,
            target.virt.is_null() as u8
        );
        return false;
    }

    let stage_start_tick = direct_rcs_now_tick();
    stage_rgba_scene(staging, src, width, height, src_pitch_bytes as usize);
    let stage_ms = direct_rcs_elapsed_ms_since(stage_start_tick);
    let src_rect = GpgpuRect::new(0, 0, copy_w, copy_h);
    let submit_start_tick = direct_rcs_now_tick();
    let stats = present_rgba8_to_primary_xrgb_rect_stats(
        staging.surface,
        src_rect,
        primary,
        GpgpuPoint::new(0, 0),
        true,
    );
    let submit_ms = direct_rcs_elapsed_ms_since(submit_start_tick);
    if stats.spans == 0 || stats.submits == 0 {
        crate::log!(
            "intel/gpgpu: present-rgba8-to-primary-xrgb-rect result=miss seq={} reason=submit-zero size={}x{} primary={}x{} stage_ms={} submit_ms={} total_ms={}\n",
            present_seq,
            copy_w,
            copy_h,
            primary.width,
            primary.height,
            stage_ms,
            submit_ms,
            direct_rcs_elapsed_ms_since(total_start_tick)
        );
        return false;
    }

    let flush_bytes = (copy_h as usize)
        .saturating_sub(1)
        .saturating_mul(primary.pitch_bytes as usize)
        .saturating_add((copy_w as usize).saturating_mul(core::mem::size_of::<u32>()));
    let presented = super::display::notify_primary_surface_external_write(
        "gpgpu-present-rgba-frame",
        0,
        flush_bytes,
    );
    if should_log || !presented {
        crate::log!(
            "intel/gpgpu: present-rgba8-to-primary-xrgb-rect result={} seq={} size={}x{} primary={}x{} spans={} submits={} stage_ms={} submit_ms={} total_ms={} flush=0x{:X}\n",
            if presented { "ok" } else { "notify-failed" },
            present_seq,
            copy_w,
            copy_h,
            primary.width,
            primary.height,
            stats.spans,
            stats.submits,
            stage_ms,
            submit_ms,
            direct_rcs_elapsed_ms_since(total_start_tick),
            flush_bytes
        );
    }
    presented
}

fn copy_rects_rgba8_stats_with_explicit_flavor(
    copies: &[GpgpuCopyRect],
    flavor: CopyRectKernelFlavor,
) -> GpgpuSubmitStats {
    let Some(first) = copies.first().copied() else {
        return GpgpuSubmitStats::default();
    };

    let mut params = Vec::with_capacity(copies.len());
    for copy in copies {
        if !same_rgba8_surface(first.src, copy.src) || !same_rgba8_surface(first.dst, copy.dst) {
            return copy_rects_rgba8_serial_stats_with_explicit_flavor(copies, flavor);
        }
        let Some(copy_params) = lower_copy_rect(copy.src, copy.src_rect, copy.dst, copy.dst_xy)
        else {
            return GpgpuSubmitStats::default();
        };
        params.push(copy_params);
    }
    submit_copy_rect_multi_ops_with_stats(first.src, first.dst, &params, flavor)
}

fn copy_rects_rgba8_serial_stats_with_explicit_flavor(
    copies: &[GpgpuCopyRect],
    flavor: CopyRectKernelFlavor,
) -> GpgpuSubmitStats {
    let mut stats = GpgpuSubmitStats::default();
    for copy in copies {
        let Some(params) = lower_copy_rect(copy.src, copy.src_rect, copy.dst, copy.dst_xy) else {
            continue;
        };
        let copy_stats = submit_copy_rect_spans_with_stats(copy.src, copy.dst, params, flavor);
        stats.spans = stats.spans.saturating_add(copy_stats.spans);
        stats.submits = stats.submits.saturating_add(copy_stats.submits);
        stats.submit_ms = stats.submit_ms.saturating_add(copy_stats.submit_ms);
        stats.present_ms = stats.present_ms.saturating_add(copy_stats.present_ms);
        stats.total_ms = stats.total_ms.saturating_add(copy_stats.total_ms);
    }
    stats
}

pub(crate) fn shell_copy_rgba8(
    src_rect: GpgpuRect,
    dst_xy: GpgpuPoint,
) -> Option<GpgpuShellCopyResult> {
    let shell = shell_surface_once()?;
    if !copy_rect_is_inside(shell.surface, src_rect, dst_xy) {
        return None;
    }
    if rects_overlap(src_rect, GpgpuRect::new(dst_xy.x, dst_xy.y, src_rect.width, src_rect.height))
    {
        return None;
    }

    shell_zero_surface(shell);
    shell_seed_copy(shell, src_rect, dst_xy);
    super::dma_flush(shell.virt, shell.surface.bytes);

    let stats = copy_rect_rgba8_stats(shell.surface, src_rect, shell.surface, dst_xy);
    super::dma_flush(shell.virt, shell.surface.bytes);

    let (src_preserved, copied) = shell_count_copy(shell, src_rect, dst_xy);
    let src_head = shell_read_rect_head(shell, src_rect);
    let dst_rect = GpgpuRect::new(dst_xy.x, dst_xy.y, src_rect.width, src_rect.height);
    let dst_head = shell_read_rect_head(shell, dst_rect);
    let pixels = rect_pixel_count(src_rect);
    let expected_spans = copy_rect_expected_spans(src_rect);
    let expected_submits = expected_spans.div_ceil(COPY_RECT_BATCH_MAX_SPANS);
    Some(GpgpuShellCopyResult {
        ok: stats.spans == expected_spans
            && stats.submits == expected_submits
            && src_preserved == pixels
            && copied == pixels,
        src_rect,
        dst_xy,
        pixels,
        spans: stats.spans,
        expected_spans,
        submits: stats.submits,
        expected_submits,
        copied,
        src_preserved,
        src_head,
        dst_head,
        surface: shell.surface,
    })
}

pub(crate) fn shell_copy_scanout_center_rgba8() -> Option<GpgpuShellScanoutCopyResult> {
    let shell = shell_surface_once()?;
    let target = super::display::primary_surface_gpgpu_marker_target()?;
    let primary = GpgpuRgba8Surface::new(
        target.phys,
        target.gpu,
        target.byte_len,
        target.width,
        target.height,
        target.pitch_bytes,
    )?;
    let width = GPGPU_SHELL_SURFACE_WIDTH.min(primary.width);
    let height = GPGPU_SHELL_SURFACE_HEIGHT.min(primary.height);
    if width == 0 || height == 0 || target.virt.is_null() {
        return None;
    }
    let src_rect = GpgpuRect::new(0, 0, width, height);
    let dst_xy = GpgpuPoint::new(
        primary.width.saturating_sub(width).saturating_div(2) as i32,
        primary.height.saturating_sub(height).saturating_div(2) as i32,
    );

    shell_zero_surface(shell);
    shell_seed_rect(shell, src_rect);
    let src_head = shell_read_rect_head(shell, src_rect);
    super::dma_flush(shell.virt, shell.surface.bytes);

    let stats = copy_rect_rgba8_stats(shell.surface, src_rect, primary, dst_xy);
    let dst_rect = GpgpuRect::new(dst_xy.x, dst_xy.y, width, height);
    let dst_head = primary_read_rect_head(target, dst_rect);
    let (src_preserved, copied) = primary_count_scanout_copy(target, shell, src_rect, dst_xy);
    let expected_spans = copy_rect_expected_spans(src_rect);
    let expected_submits = expected_spans.div_ceil(COPY_RECT_BATCH_MAX_SPANS);
    let flush_offset = (dst_xy.y as usize)
        .saturating_mul(primary.pitch_bytes as usize)
        .saturating_add((dst_xy.x as usize).saturating_mul(core::mem::size_of::<u32>()));
    let flush_bytes = (height as usize)
        .saturating_sub(1)
        .saturating_mul(primary.pitch_bytes as usize)
        .saturating_add((width as usize).saturating_mul(core::mem::size_of::<u32>()));
    let presented = super::display::notify_primary_surface_external_write(
        "gpgpu-copy-center",
        flush_offset,
        flush_bytes,
    );
    let pixels = rect_pixel_count(src_rect);

    Some(GpgpuShellScanoutCopyResult {
        ok: stats.spans == expected_spans
            && stats.submits == expected_submits
            && src_preserved == pixels
            && copied == pixels
            && presented,
        src_rect,
        dst_xy,
        primary_width: primary.width,
        primary_height: primary.height,
        primary_pitch_bytes: primary.pitch_bytes,
        primary_gpu: primary.gpu,
        primary_phys: primary.phys,
        pixels,
        spans: stats.spans,
        expected_spans,
        submits: stats.submits,
        expected_submits,
        copied,
        src_preserved,
        src_head,
        dst_head,
        presented,
    })
}

pub(crate) fn shell_twemoji_atlas_worklist_slot_scanout(
    slot: u16,
    dst_xy_override: Option<GpgpuPoint>,
) -> Option<GpgpuShellAtlasWorklistResult> {
    let target = super::display::primary_surface_gpgpu_marker_target()?;
    if target.virt.is_null()
        || target.width < SPRITE64_WORKLIST_CELL_PIXELS
        || target.height < SPRITE64_WORKLIST_CELL_PIXELS
    {
        return None;
    }

    let dst_xy = dst_xy_override.unwrap_or_else(|| {
        GpgpuPoint::new(
            target
                .width
                .saturating_sub(SPRITE64_WORKLIST_CELL_PIXELS)
                .saturating_div(2) as i32,
            target
                .height
                .saturating_sub(SPRITE64_WORKLIST_CELL_PIXELS)
                .saturating_div(2) as i32,
        )
    });
    twemoji_sprite64_worklist_primary(
        &[GpgpuTwemojiSprite64Placement {
            slot,
            dst_x: dst_xy.x,
            dst_y: dst_xy.y,
        }],
        true,
    )
}

pub(crate) fn shell_twemoji_atlas_worklist_scanout(
    requested_count: u32,
) -> Option<GpgpuShellAtlasWorklistResult> {
    shell_twemoji_atlas_worklist_scanout_present(requested_count, true)
}

pub(crate) fn shell_twemoji_atlas_worklist_scanout_present(
    requested_count: u32,
    present: bool,
) -> Option<GpgpuShellAtlasWorklistResult> {
    let total_start_tick = direct_rcs_now_tick();
    let target = super::display::primary_surface_gpgpu_marker_target()?;
    if target.virt.is_null()
        || target.width < SPRITE64_WORKLIST_CELL_PIXELS
        || target.height < SPRITE64_WORKLIST_CELL_PIXELS
    {
        return None;
    }
    let primary = GpgpuRgba8Surface::new(
        target.phys,
        target.gpu,
        target.byte_len,
        target.width,
        target.height,
        target.pitch_bytes,
    )?;
    let atlas = sprite64_worklist_atlas_once()?;
    let desc = sprite64_worklist_desc_buffer_once()?;
    let count = (requested_count as usize)
        .clamp(1, SPRITE64_WORKLIST_MAX_DESCS)
        .min(atlas.slots as usize);
    if count == 0 {
        return None;
    }

    let mut rng = crate::tyche::soft_rng();
    let mut last_slot = 0u16;
    let mut last_dst_xy = GpgpuPoint::new(0, 0);
    unsafe {
        core::ptr::write_bytes(desc.virt, 0, desc.bytes);
        let descs = desc.virt as *mut Sprite64WorklistRgba8Desc;
        for index in 0..count {
            let slot = rng.usize_below(atlas.slots as usize) as u16;
            let atlas_x = (u32::from(slot) % atlas.columns) * SPRITE64_WORKLIST_CELL_PIXELS;
            let atlas_y = (u32::from(slot) / atlas.columns) * SPRITE64_WORKLIST_CELL_PIXELS;
            let max_x = target.width.saturating_sub(SPRITE64_WORKLIST_CELL_PIXELS);
            let max_y = target.height.saturating_sub(SPRITE64_WORKLIST_CELL_PIXELS);
            let dst_x = rng.usize_below(max_x.saturating_add(1) as usize) as u32;
            let dst_y = rng.usize_below(max_y.saturating_add(1) as usize) as u32;
            let desc_value = Sprite64WorklistRgba8Desc {
                atlas_xy: ((atlas_y & 0xFFFF) << 16) | (atlas_x & 0xFFFF),
                dst_xy: ((dst_y & 0xFFFF) << 16) | (dst_x & 0xFFFF),
                flags: SPRITE64_WORKLIST_FLAG_SRC_OVER,
                color_rgba: 0x00FF_FFFF,
            };
            core::ptr::write_volatile(descs.add(index), desc_value);
            last_slot = slot;
            last_dst_xy = GpgpuPoint::new(dst_x as i32, dst_y as i32);
        }
    }
    super::dma_flush(desc.virt, desc.bytes);

    let params = Sprite64WorklistRgba8Params {
        atlas_gpu: atlas.surface.gpu,
        dst_gpu: primary.gpu,
        desc_gpu: desc.gpu,
        atlas_pitch_bytes: atlas.surface.pitch_bytes,
        dst_pitch_bytes: primary.pitch_bytes,
        desc_base: 0,
        desc_count: count as u32,
    };
    let walkers = sprite64_worklist_walker_count(count);

    let submit_start_tick = direct_rcs_now_tick();
    let submitted = submit_sprite64_worklist(atlas.surface, primary, desc, params);
    let submit_ms = direct_rcs_elapsed_ms_since(submit_start_tick);
    let present_start_tick = direct_rcs_now_tick();
    let presented = if submitted && present {
        super::display::notify_primary_surface_external_write(
            "gpgpu-athlas-worklist",
            0,
            target.byte_len,
        )
    } else {
        false
    };
    let present_ms = direct_rcs_elapsed_ms_since(present_start_tick);

    Some(GpgpuShellAtlasWorklistResult {
        ok: submitted && (!present || presented),
        submitted,
        requested: requested_count as usize,
        descriptors: count,
        walkers,
        copied_pixels: count
            .saturating_mul(SPRITE64_WORKLIST_CELL_PIXELS as usize)
            .saturating_mul(SPRITE64_WORKLIST_CELL_PIXELS as usize),
        submit_ms,
        present_ms,
        total_ms: direct_rcs_elapsed_ms_since(total_start_tick),
        atlas_gpu: atlas.surface.gpu,
        desc_gpu: desc.gpu,
        primary_width: primary.width,
        primary_height: primary.height,
        slots: atlas.slots,
        last_slot,
        last_dst_xy,
        presented,
    })
}

pub(crate) fn sprite64_font_slot_for_region(
    face: crate::gfx::althlasfont::bitmapfont::AthlasFontFace,
    region: crate::gfx::althlasfont::athlasmetrics::AthlasGlyphRegion,
) -> Option<u16> {
    if region.bucket as usize >= crate::gfx::althlasfont::athlasmetrics::ATHLAS_BUCKET_COUNT {
        return None;
    }
    let bucket_cells = crate::gfx::althlasfont::bitmapfont::athlas_font_bucket_cell_count(
        face,
        region.bucket as usize,
    )?;
    if u32::from(region.slot) >= bucket_cells {
        return None;
    }
    let base = sprite64_font_bucket_base(face, region.bucket as usize)?;
    base.checked_add(region.slot)
}

fn sprite64_font_bucket_base(
    face: crate::gfx::althlasfont::bitmapfont::AthlasFontFace,
    bucket: usize,
) -> Option<u16> {
    if bucket >= crate::gfx::althlasfont::athlasmetrics::ATHLAS_BUCKET_COUNT {
        return None;
    }

    let mut base = u32::from(crate::gfx::althlasfont::twemoji::twemoji_slot_count());
    for prior_face in crate::gfx::althlasfont::bitmapfont::ATHLAS_SPRITE64_FONT_FACES {
        if prior_face == face {
            break;
        }
        base = base.checked_add(
            crate::gfx::althlasfont::bitmapfont::athlas_font_face_cell_count(prior_face)?,
        )?;
    }
    for prior_bucket in 0..bucket {
        base = base.checked_add(
            crate::gfx::althlasfont::bitmapfont::athlas_font_bucket_cell_count(face, prior_bucket)?,
        )?;
    }
    u16::try_from(base).ok()
}

fn sprite64_font_bucket_pngs(
    face: crate::gfx::althlasfont::bitmapfont::AthlasFontFace,
) -> Option<&'static [&'static [u8]; crate::gfx::althlasfont::athlasmetrics::ATHLAS_BUCKET_COUNT]> {
    use crate::gfx::althlasfont::bitmapfont::{AthlasFontFamily, AthlasFontTier};

    match (face.family, face.tier) {
        (AthlasFontFamily::Lucida, AthlasFontTier::Third) => {
            Some(&SPRITE64_LUCIDA_THIRD_BUCKET_PNGS)
        }
        (AthlasFontFamily::Lucida, AthlasFontTier::Half) => Some(&SPRITE64_LUCIDA_HALF_BUCKET_PNGS),
        (AthlasFontFamily::Lucida, AthlasFontTier::OneX) => Some(&SPRITE64_LUCIDA_1X_BUCKET_PNGS),
        (AthlasFontFamily::Palatino, AthlasFontTier::Third) => {
            Some(&SPRITE64_PALATINO_THIRD_BUCKET_PNGS)
        }
        (AthlasFontFamily::Palatino, AthlasFontTier::Half) => {
            Some(&SPRITE64_PALATINO_HALF_BUCKET_PNGS)
        }
        (AthlasFontFamily::Palatino, AthlasFontTier::OneX) => {
            Some(&SPRITE64_PALATINO_1X_BUCKET_PNGS)
        }
    }
}

pub(crate) fn sprite64_font_slot_count() -> Option<u32> {
    let mut count = 0u32;
    for face in crate::gfx::althlasfont::bitmapfont::ATHLAS_SPRITE64_FONT_FACES {
        count = count
            .checked_add(crate::gfx::althlasfont::bitmapfont::athlas_font_face_cell_count(face)?)?;
    }
    Some(count)
}

pub(crate) fn warm_sprite64_font_atlas() -> Option<GpgpuSprite64AtlasWarmResult> {
    let atlas = sprite64_worklist_atlas_once()?;
    Some(GpgpuSprite64AtlasWarmResult {
        ok: true,
        slots: atlas.slots,
        columns: atlas.columns,
        width: atlas.surface.width,
        height: atlas.surface.height,
        pitch_bytes: atlas.surface.pitch_bytes,
        bytes: atlas.surface.bytes,
        atlas_gpu: atlas.surface.gpu,
    })
}

pub(crate) fn twemoji_sprite64_worklist_primary(
    placements: &[GpgpuTwemojiSprite64Placement],
    present: bool,
) -> Option<GpgpuShellAtlasWorklistResult> {
    sprite64_worklist_primary_inner(placements, present, "ui2-cursor-sprite64-worklist")
}

pub(crate) fn sprite64_worklist_primary(
    placements: &[GpgpuSprite64Placement],
    present: bool,
    present_reason: &str,
) -> Option<GpgpuShellAtlasWorklistResult> {
    sprite64_worklist_primary_inner(placements, present, present_reason)
}

fn sprite64_worklist_primary_inner<T: Sprite64PlacementDesc>(
    placements: &[T],
    present: bool,
    present_reason: &str,
) -> Option<GpgpuShellAtlasWorklistResult> {
    let total_start_tick = direct_rcs_now_tick();
    let target_start_tick = direct_rcs_now_tick();
    let target = super::display::primary_surface_gpgpu_marker_target()?;
    if target.virt.is_null()
        || target.width < SPRITE64_WORKLIST_CELL_PIXELS
        || target.height < SPRITE64_WORKLIST_CELL_PIXELS
        || placements.is_empty()
    {
        return None;
    }
    let primary = GpgpuRgba8Surface::new(
        target.phys,
        target.gpu,
        target.byte_len,
        target.width,
        target.height,
        target.pitch_bytes,
    )?;
    let target_ms = direct_rcs_elapsed_ms_since(target_start_tick);
    let atlas_start_tick = direct_rcs_now_tick();
    let atlas = sprite64_worklist_atlas_once()?;
    let atlas_ms = direct_rcs_elapsed_ms_since(atlas_start_tick);
    let desc_start_tick = direct_rcs_now_tick();
    let desc = sprite64_worklist_desc_buffer_once()?;
    let desc_ms = direct_rcs_elapsed_ms_since(desc_start_tick);
    let count = placements
        .len()
        .min(SPRITE64_WORKLIST_MAX_DESCS)
        .min(atlas.slots as usize);
    if count == 0 {
        return None;
    }

    let max_x = target.width.saturating_sub(SPRITE64_WORKLIST_CELL_PIXELS) as i32;
    let max_y = target.height.saturating_sub(SPRITE64_WORKLIST_CELL_PIXELS) as i32;
    let mut last_slot = 0u16;
    let mut last_dst_xy = GpgpuPoint::new(0, 0);
    let desc_write_start_tick = direct_rcs_now_tick();
    let _desc_guard = RECT_WORKLIST_DESC_SUBMIT_LOCK.lock();
    unsafe {
        core::ptr::write_bytes(desc.virt, 0, desc.bytes);
        let descs = desc.virt as *mut Sprite64WorklistRgba8Desc;
        for (index, placement) in placements.iter().take(count).enumerate() {
            let slot = placement.slot();
            if slot >= atlas.slots {
                return None;
            }
            let atlas_x = (u32::from(slot) % atlas.columns) * SPRITE64_WORKLIST_CELL_PIXELS;
            let atlas_y = (u32::from(slot) / atlas.columns) * SPRITE64_WORKLIST_CELL_PIXELS;
            let dst_x = placement.dst_x().clamp(0, max_x);
            let dst_y = placement.dst_y().clamp(0, max_y);
            let desc_value = Sprite64WorklistRgba8Desc {
                atlas_xy: ((atlas_y & 0xFFFF) << 16) | (atlas_x & 0xFFFF),
                dst_xy: (((dst_y as u32) & 0xFFFF) << 16) | ((dst_x as u32) & 0xFFFF),
                flags: placement.flags(),
                color_rgba: placement.color_rgba(),
            };
            core::ptr::write_volatile(descs.add(index), desc_value);
            last_slot = slot;
            last_dst_xy = GpgpuPoint::new(dst_x, dst_y);
        }
    }
    super::dma_flush(desc.virt, desc.bytes);
    let desc_write_ms = direct_rcs_elapsed_ms_since(desc_write_start_tick);

    let params = Sprite64WorklistRgba8Params {
        atlas_gpu: atlas.surface.gpu,
        dst_gpu: primary.gpu,
        desc_gpu: desc.gpu,
        atlas_pitch_bytes: atlas.surface.pitch_bytes,
        dst_pitch_bytes: primary.pitch_bytes,
        desc_base: 0,
        desc_count: count as u32,
    };
    let walkers = sprite64_worklist_walker_count(count);

    let submit_start_tick = direct_rcs_now_tick();
    let submitted = submit_sprite64_worklist(atlas.surface, primary, desc, params);
    let submit_ms = direct_rcs_elapsed_ms_since(submit_start_tick);
    let present_start_tick = direct_rcs_now_tick();
    let presented = if submitted && present {
        super::display::notify_primary_surface_external_write(present_reason, 0, target.byte_len)
    } else {
        false
    };
    let present_ms = direct_rcs_elapsed_ms_since(present_start_tick);
    let total_ms = direct_rcs_elapsed_ms_since(total_start_tick);
    let phase_log = SPRITE64_WORKLIST_PRIMARY_PHASE_LOGS.fetch_add(1, Ordering::Relaxed);
    if phase_log < 8 || total_ms >= 50 {
        crate::log!(
            "intel/gpgpu: sprite64-worklist-primary phases requested={} desc={} walkers={} present={} submitted={} target_ms={} atlas_ms={} desc_ms={} desc_write_ms={} submit_ms={} present_ms={} total_ms={} atlas_gpu=0x{:X} desc_gpu=0x{:X} primary={}x{} slots={}\n",
            placements.len(),
            count,
            walkers,
            present as u8,
            submitted as u8,
            target_ms,
            atlas_ms,
            desc_ms,
            desc_write_ms,
            submit_ms,
            present_ms,
            total_ms,
            atlas.surface.gpu,
            desc.gpu,
            primary.width,
            primary.height,
            atlas.slots
        );
    }

    Some(GpgpuShellAtlasWorklistResult {
        ok: submitted && (!present || presented),
        submitted,
        requested: placements.len(),
        descriptors: count,
        walkers,
        copied_pixels: count
            .saturating_mul(SPRITE64_WORKLIST_CELL_PIXELS as usize)
            .saturating_mul(SPRITE64_WORKLIST_CELL_PIXELS as usize),
        submit_ms,
        present_ms,
        total_ms,
        atlas_gpu: atlas.surface.gpu,
        desc_gpu: desc.gpu,
        primary_width: primary.width,
        primary_height: primary.height,
        slots: atlas.slots,
        last_slot,
        last_dst_xy,
        presented,
    })
}

pub(crate) fn shell_mandel64_worklist_scanout(
    iterations: u32,
) -> Option<GpgpuShellMandel64WorklistResult> {
    let target = super::display::primary_surface_gpgpu_marker_target()?;
    if target.virt.is_null()
        || target.width < MANDEL64_WORKLIST_CELL_PIXELS
        || target.height < MANDEL64_WORKLIST_CELL_PIXELS
    {
        return None;
    }

    let columns = target.width.div_ceil(MANDEL64_WORKLIST_CELL_PIXELS).max(1);
    let render_height = target.height.div_ceil(2).max(1);
    let rows = render_height.div_ceil(MANDEL64_WORKLIST_CELL_PIXELS).max(1);
    let count = columns.saturating_mul(rows) as usize;
    if count == 0 {
        return None;
    }
    let iterations = iterations.clamp(1, MANDEL64_WORKLIST_MAX_ITERATIONS);

    let total_start_tick = direct_rcs_now_tick();
    let mut placements = Vec::new();
    let mut submitted = true;
    let mut descriptors = 0usize;
    let mut walkers = 0usize;
    let mut pixels = 0usize;
    let mut submit_ms = 0u64;
    let mut desc_gpu = 0u64;
    let mut last_src_xy = GpgpuPoint::new(0, 0);
    let mut last_dst_xy = GpgpuPoint::new(0, 0);
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
            let width = target
                .width
                .saturating_sub(dst_x)
                .min(MANDEL64_WORKLIST_CELL_PIXELS);
            let height = render_height
                .saturating_sub(dst_y)
                .min(MANDEL64_WORKLIST_CELL_PIXELS);
            placements.push(GpgpuMandel64Placement {
                src_x: dst_x as i32,
                src_y: dst_y as i32,
                dst_x: dst_x as i32,
                dst_y: dst_y as i32,
                width,
                height,
                mirror_height: target.height,
                iterations,
            });
        }

        let result = mandel64_worklist_primary(placements.as_slice(), false)?;
        submitted &= result.submitted;
        submitted_tiles = submitted_tiles.saturating_add(result.requested);
        descriptors = descriptors.saturating_add(result.descriptors);
        walkers = walkers.saturating_add(result.walkers);
        pixels = pixels.saturating_add(result.pixels);
        submit_ms = submit_ms.saturating_add(result.submit_ms);
        desc_gpu = result.desc_gpu;
        last_src_xy = result.last_src_xy;
        last_dst_xy = result.last_dst_xy;
        if !result.ok {
            break;
        }
        index = end;
    }

    let present_start_tick = direct_rcs_now_tick();
    let presented = submitted
        && submitted_tiles == count
        && super::display::notify_primary_surface_external_write(
            "gpgpu-mandel64-worklist",
            0,
            target.byte_len,
        );
    let present_ms = direct_rcs_elapsed_ms_since(present_start_tick);

    Some(GpgpuShellMandel64WorklistResult {
        ok: submitted && submitted_tiles == count && presented,
        submitted,
        requested: count,
        descriptors,
        walkers,
        pixels,
        submit_ms,
        present_ms,
        total_ms: direct_rcs_elapsed_ms_since(total_start_tick),
        desc_gpu,
        primary_width: target.width,
        primary_height: target.height,
        last_src_xy,
        last_dst_xy,
        presented,
    })
}

pub(crate) fn mandel64_worklist_primary(
    placements: &[GpgpuMandel64Placement],
    present: bool,
) -> Option<GpgpuShellMandel64WorklistResult> {
    let total_start_tick = direct_rcs_now_tick();
    let target = super::display::primary_surface_gpgpu_marker_target()?;
    if target.virt.is_null()
        || target.width < MANDEL64_WORKLIST_CELL_PIXELS
        || target.height < MANDEL64_WORKLIST_CELL_PIXELS
        || placements.is_empty()
    {
        return None;
    }
    let primary = GpgpuRgba8Surface::new(
        target.phys,
        target.gpu,
        target.byte_len,
        target.width,
        target.height,
        target.pitch_bytes,
    )?;
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
            let dst_x = placement
                .dst_x
                .clamp(0, target.width.saturating_sub(1) as i32);
            let dst_y = placement
                .dst_y
                .clamp(0, target.height.saturating_sub(1) as i32);
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
                .min(target.width.saturating_sub(dst_x as u32));
            let height = requested_height
                .min(MANDEL64_WORKLIST_CELL_PIXELS)
                .min(target.height.saturating_sub(dst_y as u32));
            let iterations = placement
                .iterations
                .clamp(1, MANDEL64_WORKLIST_MAX_ITERATIONS);
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
                    | (width << MANDEL64_WORKLIST_FLAG_COLS_SHIFT)
                    | (placement.mirror_height.min(u16::MAX as u32)
                        << MANDEL64_WORKLIST_FLAG_MIRROR_HEIGHT_SHIFT);
                let desc_value = Mandel64WorklistRgba8Desc {
                    src_xy: pack_i16_pair_u32(
                        src_x as i16,
                        src_y
                            .saturating_add(band_y)
                            .clamp(i16::MIN as i32, i16::MAX as i32) as i16,
                    ),
                    dst_xy: pack_i16_pair_u32(
                        dst_x as i16,
                        dst_y
                            .saturating_add(band_y)
                            .clamp(0, target.height as i32 - 1) as i16,
                    ),
                    flags,
                    color_rgba: iterations,
                };
                core::ptr::write_volatile(descs.add(desc_count), desc_value);
                desc_count = desc_count.saturating_add(1);
            }
            let computed_pixels = (width as usize).saturating_mul(height as usize);
            let output_pixels = if placement.mirror_height == 0 {
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
        dst_gpu: primary.gpu,
        desc_gpu: desc.gpu,
        dst_pitch_bytes: primary.pitch_bytes,
        desc_base: 0,
        desc_count: desc_count as u32,
    };
    let walkers = mandel64_worklist_walker_count(desc_count);

    let submit_start_tick = direct_rcs_now_tick();
    let submitted = submit_mandel64_worklist(primary, desc, params);
    let submit_ms = direct_rcs_elapsed_ms_since(submit_start_tick);
    let present_start_tick = direct_rcs_now_tick();
    let presented = if submitted && present {
        super::display::notify_primary_surface_external_write(
            "gpgpu-mandel64-worklist",
            0,
            target.byte_len,
        )
    } else {
        false
    };
    let present_ms = direct_rcs_elapsed_ms_since(present_start_tick);

    Some(GpgpuShellMandel64WorklistResult {
        ok: submitted && (!present || presented),
        submitted,
        requested: count,
        descriptors: desc_count,
        walkers,
        pixels: drawn_pixels,
        submit_ms,
        present_ms,
        total_ms: direct_rcs_elapsed_ms_since(total_start_tick),
        desc_gpu: desc.gpu,
        primary_width: primary.width,
        primary_height: primary.height,
        last_src_xy,
        last_dst_xy,
        presented,
    })
}

pub(crate) fn shell_twemoji_atlas_worklist_present_scanout() -> Option<u64> {
    let target = super::display::primary_surface_gpgpu_marker_target()?;
    if target.virt.is_null() || target.byte_len == 0 {
        return None;
    }
    let present_start_tick = direct_rcs_now_tick();
    if super::display::notify_primary_surface_external_write(
        "gpgpu-athlas-worklist-final",
        0,
        target.byte_len,
    ) {
        Some(direct_rcs_elapsed_ms_since(present_start_tick))
    } else {
        None
    }
}

pub(crate) fn submit_direct_rcs_smoke_once() -> bool {
    if !DIRECT_RCS_ENABLED || DIRECT_RCS_SMOKE_RAN.swap(true, Ordering::AcqRel) {
        return false;
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: direct-rcs-smoke skipped reason=no-claimed-device\n"
        );
        return false;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: direct-rcs-smoke failed rung=alloc\n"
        );
        return false;
    };

    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let batch_ok = ppgtt_ok && direct_rcs_encode_smoke_batch(state);
    let submit_start_tick = direct_rcs_now_tick();
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result(state, DIRECT_RCS_SMOKE_MARKER)
    } else {
        0
    };
    let retire_ms = if submitted {
        direct_rcs_elapsed_ms_since(submit_start_tick)
    } else {
        0
    };
    let retired = observed == DIRECT_RCS_SMOKE_MARKER;

    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: direct-rcs-smoke forcewake={} ggtt={} ppgtt={} batch={} submitted={} retired={} retire_ms={} observed=0x{:08X} expected=0x{:08X} ring_gpu=0x{:X} batch_gpu=0x{:X} result_gpu=0x{:X} head=0x{:08X} tail=0x{:08X} acthd=0x{:08X} ipeir=0x{:08X} ipehr=0x{:08X} eir=0x{:08X} path=direct-execlist no_guc_submit=1 next=gpgpu-walker\n",
        forcewake_ok as u8,
        mapped_ok as u8,
        ppgtt_ok as u8,
        batch_ok as u8,
        submitted as u8,
        retired as u8,
        retire_ms,
        observed,
        DIRECT_RCS_SMOKE_MARKER,
        DIRECT_RCS_GPU_VA_RING_BASE,
        DIRECT_RCS_GPU_VA_BATCH_BASE,
        DIRECT_RCS_GPU_VA_RESULT_BASE,
        super::mmio_read(dev, RCS_RING_HEAD),
        super::mmio_read(dev, RCS_RING_TAIL),
        super::mmio_read(dev, RCS_RING_ACTHD),
        super::mmio_read(dev, RCS_RING_IPEIR),
        super::mmio_read(dev, RCS_RING_IPEHR),
        super::mmio_read(dev, RCS_RING_EIR),
    );

    retired
}

pub(crate) fn submit_copy_rect_rgba8_strip_once() -> bool {
    if !DIRECT_RCS_ENABLED || COPY_RECT_WALKER_RAN.swap(true, Ordering::AcqRel) {
        return false;
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: copy-rect-rgba8-strip skipped reason=no-claimed-device\n"
        );
        return false;
    };
    let Some(upload) = upload_copy_rect_rgba8_kernel() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: copy-rect-rgba8-strip skipped reason=no-kernel-upload\n"
        );
        return false;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: copy-rect-rgba8-strip failed rung=alloc\n"
        );
        return false;
    };

    let (src_before, dst_before) = direct_rcs_seed_copy_rect_strip(state);
    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ppgtt_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let test_ppgtt_ok = kernel_ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(
            state,
            DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE,
            state.clear_test_phys,
            CLEAR_RECT_TEST_BYTES,
        );
    let params = CopyRectRgba8Params {
        src_gpu: DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE,
        dst_gpu: DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE,
        src_pitch_bytes: COPY_RECT_TEST_PITCH_BYTES,
        dst_pitch_bytes: COPY_RECT_TEST_PITCH_BYTES,
        src_x: 0,
        src_y: 0,
        dst_x: COPY_RECT_TEST_DST_X,
        dst_y: 0,
        width: COPY_RECT_TEST_WIDTH,
        height: COPY_RECT_TEST_HEIGHT,
    };
    let batch_ok = test_ppgtt_ok
        && direct_rcs_encode_copy_rect_walker_batch(
            state,
            upload,
            params,
            COPY_RECT_TEST_ROW_DWORDS * core::mem::size_of::<u32>(),
            COPY_RECT_TEST_ROW_DWORDS * core::mem::size_of::<u32>(),
            COPY_RECT_WALKER_RIGHT_MASK,
        );
    let submit_start_tick = direct_rcs_now_tick();
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let (observed, retire_ms) = if submitted {
        direct_rcs_poll_result_slot_elapsed(
            state,
            COPY_RECT_POST_MARKER_SLOT,
            COPY_RECT_POST_MARKER,
            submit_start_tick,
        )
    } else {
        (0, 0)
    };
    let pre_marker = direct_rcs_read_result_slot(state, COPY_RECT_PRE_MARKER_SLOT);
    let (src_after, dst_after) = direct_rcs_read_copy_rect_strip(state);
    let retired = observed == COPY_RECT_POST_MARKER;
    let expected_src = [
        CLEAR_RECT_RGBA_RED,
        CLEAR_RECT_RGBA_GREEN,
        CLEAR_RECT_RGBA_BLUE,
        CLEAR_RECT_RGBA_BLACK,
    ];
    let expected_dst_before = [
        COPY_RECT_DST_POISON0,
        COPY_RECT_DST_POISON1,
        COPY_RECT_DST_POISON2,
        COPY_RECT_DST_POISON3,
    ];
    let src_before_ok = src_before == expected_src;
    let dst_before_ok = dst_before == expected_dst_before;
    let src_ok = src_after == expected_src;
    let copy_ok = dst_after == expected_src;
    let copied_count = direct_rcs_count_matching(dst_after, expected_src);

    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: copy-rect-rgba8-strip forcewake={} ggtt={} ppgtt={} kernel_ppgtt={} test_ppgtt={} batch={} submitted={} retired={} retire_ms={} src_before_ok={} dst_before_ok={} src_ok={} copy_ok={} copied={}/{} src_before=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] dst_before=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] src_after=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] dst_after=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] pre_marker=0x{:08X} post_marker=0x{:08X} expected_post=0x{:08X} kernel_gpu=0x{:X} kernel_text_gpu=0x{:X} buf_gpu=0x{:X} rect={}x{} src_xy=0,0 dst_xy={},0 pitch={} idd_off=0x{:X} payload_off=0x{:X} simd=16 groups=1 threads_per_group={} ring_gpu=0x{:X} batch_gpu=0x{:X} result_gpu=0x{:X} head=0x{:08X} tail=0x{:08X} acthd=0x{:08X} ipeir=0x{:08X} ipehr=0x{:08X} eir=0x{:08X} path=direct-execlist no_guc_submit=1 next=recursive-copy-visual\n",
        forcewake_ok as u8,
        mapped_ok as u8,
        ppgtt_ok as u8,
        kernel_ppgtt_ok as u8,
        test_ppgtt_ok as u8,
        batch_ok as u8,
        submitted as u8,
        retired as u8,
        retire_ms,
        src_before_ok as u8,
        dst_before_ok as u8,
        src_ok as u8,
        copy_ok as u8,
        copied_count,
        COPY_RECT_TEST_PIXELS,
        src_before[0],
        src_before[1],
        src_before[2],
        src_before[3],
        dst_before[0],
        dst_before[1],
        dst_before[2],
        dst_before[3],
        src_after[0],
        src_after[1],
        src_after[2],
        src_after[3],
        dst_after[0],
        dst_after[1],
        dst_after[2],
        dst_after[3],
        pre_marker,
        observed,
        COPY_RECT_POST_MARKER,
        upload.gpu,
        upload.gpu + COPY_RECT_RGBA8_TEXT_OFFSET_BYTES,
        DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE,
        COPY_RECT_TEST_WIDTH,
        COPY_RECT_TEST_HEIGHT,
        COPY_RECT_TEST_DST_X,
        COPY_RECT_TEST_PITCH_BYTES,
        COPY_RECT_IDD_OFFSET_BYTES,
        COPY_RECT_PAYLOAD_OFFSET_BYTES,
        GPGPU_WALKER_GROUP_THREADS,
        DIRECT_RCS_GPU_VA_RING_BASE,
        DIRECT_RCS_GPU_VA_BATCH_BASE,
        DIRECT_RCS_GPU_VA_RESULT_BASE,
        super::mmio_read(dev, RCS_RING_HEAD),
        super::mmio_read(dev, RCS_RING_TAIL),
        super::mmio_read(dev, RCS_RING_ACTHD),
        super::mmio_read(dev, RCS_RING_IPEIR),
        super::mmio_read(dev, RCS_RING_IPEHR),
        super::mmio_read(dev, RCS_RING_EIR),
    );

    retired && src_before_ok && dst_before_ok && src_ok && copy_ok
}

pub(crate) fn submit_copy_rect_rgba8_256_once() -> bool {
    if !DIRECT_RCS_ENABLED || COPY_RECT_256_RAN.swap(true, Ordering::AcqRel) {
        return false;
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: copy-rect-rgba8-256 skipped reason=no-claimed-device\n"
        );
        return false;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: copy-rect-rgba8-256 failed rung=alloc\n"
        );
        return false;
    };
    let Some(surface) = GpgpuRgba8Surface::new(
        state.clear_test_phys,
        DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE,
        CLEAR_RECT_TEST_BYTES,
        COPY_RECT_256_SURFACE_WIDTH,
        COPY_RECT_256_HEIGHT,
        COPY_RECT_256_PITCH_BYTES,
    ) else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: copy-rect-rgba8-256 failed rung=surface\n"
        );
        return false;
    };

    direct_rcs_seed_copy_rect_256(state);
    let Some(params) = lower_copy_rect(
        surface,
        GpgpuRect::new(0, 0, COPY_RECT_256_WIDTH, COPY_RECT_256_HEIGHT),
        surface,
        GpgpuPoint::new(COPY_RECT_256_WIDTH as i32, 0),
    ) else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: copy-rect-rgba8-256 failed rung=lower\n"
        );
        return false;
    };
    let copy_start_tick = direct_rcs_now_tick();
    let Some(flavor) = copy_rect_kernel_flavor_narrow() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: copy-rect-rgba8-256 failed rung=kernel\n"
        );
        return false;
    };
    let stats = submit_copy_rect_spans_with_stats(surface, surface, params, flavor);
    let copy_ms = direct_rcs_elapsed_ms_since(copy_start_tick);
    let samples_ok = direct_rcs_copy_rect_256_samples_ok(state);
    let (src_preserved, copied) = direct_rcs_copy_rect_256_full_counts(state);
    let src_head = direct_rcs_read_copy_rect_256_span(state, 0);
    let dst_head = direct_rcs_read_copy_rect_256_span(state, COPY_RECT_256_WIDTH as usize);
    let dst_tail = direct_rcs_read_copy_rect_256_span(
        state,
        COPY_RECT_256_WIDTH as usize + COPY_RECT_256_WIDTH as usize - 4,
    );
    let ok = stats.spans == COPY_RECT_256_EXPECTED_SPANS
        && stats.submits == 1
        && src_preserved == COPY_RECT_256_WIDTH as usize
        && copied == COPY_RECT_256_WIDTH as usize
        && samples_ok == COPY_RECT_256_SAMPLE_INDICES.len();

    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: copy-rect-rgba8-256 ok={} copy_ms={} submits={}/1 spans={}/{} copied={}/{} src_preserved={}/{} samples={}/{} surface_gpu=0x{:X} surface_phys=0x{:X} rect={}x{} src_xy=0,0 dst_xy={},0 pitch={} src_head=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] dst_head=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] dst_tail=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] lowering=row-spans pixels_per_lane={} batched_walkers=1 max_span_px={} artifact={}\n",
        ok as u8,
        copy_ms,
        stats.submits,
        stats.spans,
        COPY_RECT_256_EXPECTED_SPANS,
        copied,
        COPY_RECT_256_WIDTH,
        src_preserved,
        COPY_RECT_256_WIDTH,
        samples_ok,
        COPY_RECT_256_SAMPLE_INDICES.len(),
        surface.gpu,
        surface.phys,
        COPY_RECT_256_WIDTH,
        COPY_RECT_256_HEIGHT,
        COPY_RECT_256_WIDTH,
        COPY_RECT_256_PITCH_BYTES,
        src_head[0],
        src_head[1],
        src_head[2],
        src_head[3],
        dst_head[0],
        dst_head[1],
        dst_head[2],
        dst_head[3],
        dst_tail[0],
        dst_tail[1],
        dst_tail[2],
        dst_tail[3],
        COPY_RECT_PIXELS_PER_LANE,
        COPY_RECT_SPAN_PIXELS,
        COPY_RECT_RGBA8_KERNEL_NAME,
    );

    ok
}

pub(crate) fn submit_copy_rect_rgba8_256x2_once() -> bool {
    if !DIRECT_RCS_ENABLED || COPY_RECT_256X2_RAN.swap(true, Ordering::AcqRel) {
        return false;
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: copy-rect-rgba8-256x2 skipped reason=no-claimed-device\n"
        );
        return false;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: copy-rect-rgba8-256x2 failed rung=alloc\n"
        );
        return false;
    };
    let Some(surface) = GpgpuRgba8Surface::new(
        state.clear_test_phys,
        DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE,
        CLEAR_RECT_TEST_BYTES,
        COPY_RECT_256X2_SURFACE_WIDTH,
        COPY_RECT_256X2_HEIGHT,
        COPY_RECT_256X2_PITCH_BYTES,
    ) else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: copy-rect-rgba8-256x2 failed rung=surface\n"
        );
        return false;
    };

    direct_rcs_seed_copy_rect_256x2(state);
    let Some(params) = lower_copy_rect(
        surface,
        GpgpuRect::new(0, 0, COPY_RECT_256X2_WIDTH, COPY_RECT_256X2_HEIGHT),
        surface,
        GpgpuPoint::new(COPY_RECT_256X2_WIDTH as i32, 0),
    ) else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: copy-rect-rgba8-256x2 failed rung=lower\n"
        );
        return false;
    };
    let copy_start_tick = direct_rcs_now_tick();
    let Some(flavor) = copy_rect_kernel_flavor_narrow() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: copy-rect-rgba8-256x2 failed rung=kernel\n"
        );
        return false;
    };
    let stats = submit_copy_rect_spans_with_stats(surface, surface, params, flavor);
    let copy_ms = direct_rcs_elapsed_ms_since(copy_start_tick);
    let samples_ok = direct_rcs_copy_rect_256x2_samples_ok(state);
    let (src_preserved, copied) = direct_rcs_copy_rect_256x2_full_counts(state);
    let row0_head = direct_rcs_read_copy_rect_256x2_span(state, 0, COPY_RECT_256X2_WIDTH as usize);
    let row1_head = direct_rcs_read_copy_rect_256x2_span(state, 1, COPY_RECT_256X2_WIDTH as usize);
    let row1_tail = direct_rcs_read_copy_rect_256x2_span(
        state,
        1,
        COPY_RECT_256X2_WIDTH as usize + COPY_RECT_256X2_WIDTH as usize - 4,
    );
    let ok = stats.spans == COPY_RECT_256X2_EXPECTED_SPANS
        && stats.submits == COPY_RECT_256X2_EXPECTED_SUBMITS
        && src_preserved == (COPY_RECT_256X2_WIDTH * COPY_RECT_256X2_HEIGHT) as usize
        && copied == (COPY_RECT_256X2_WIDTH * COPY_RECT_256X2_HEIGHT) as usize
        && samples_ok == COPY_RECT_256X2_SAMPLE_POINTS.len();

    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: copy-rect-rgba8-256x2 ok={} copy_ms={} submits={}/{} spans={}/{} copied={}/{} src_preserved={}/{} samples={}/{} surface_gpu=0x{:X} surface_phys=0x{:X} rect={}x{} src_xy=0,0 dst_xy={},0 pitch={} row0_dst_head=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] row1_dst_head=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] row1_dst_tail=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] lowering=row-spans pixels_per_lane={} batched_walkers=1 max_span_px={} artifact={}\n",
        ok as u8,
        copy_ms,
        stats.submits,
        COPY_RECT_256X2_EXPECTED_SUBMITS,
        stats.spans,
        COPY_RECT_256X2_EXPECTED_SPANS,
        copied,
        COPY_RECT_256X2_WIDTH * COPY_RECT_256X2_HEIGHT,
        src_preserved,
        COPY_RECT_256X2_WIDTH * COPY_RECT_256X2_HEIGHT,
        samples_ok,
        COPY_RECT_256X2_SAMPLE_POINTS.len(),
        surface.gpu,
        surface.phys,
        COPY_RECT_256X2_WIDTH,
        COPY_RECT_256X2_HEIGHT,
        COPY_RECT_256X2_WIDTH,
        COPY_RECT_256X2_PITCH_BYTES,
        row0_head[0],
        row0_head[1],
        row0_head[2],
        row0_head[3],
        row1_head[0],
        row1_head[1],
        row1_head[2],
        row1_head[3],
        row1_tail[0],
        row1_tail[1],
        row1_tail[2],
        row1_tail[3],
        COPY_RECT_PIXELS_PER_LANE,
        COPY_RECT_SPAN_PIXELS,
        COPY_RECT_RGBA8_KERNEL_NAME,
    );

    ok
}

pub(crate) fn submit_rect_api_smoke_once() -> bool {
    if !DIRECT_RCS_ENABLED || RECT_API_SMOKE_RAN.swap(true, Ordering::AcqRel) {
        return false;
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu/rect-api: skipped reason=no-claimed-device\n"
        );
        return false;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu/rect-api: failed rung=alloc\n"
        );
        return false;
    };
    let Some(surface) = GpgpuRgba8Surface::new(
        state.clear_test_phys,
        DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE,
        CLEAR_RECT_TEST_BYTES,
        32,
        1,
        32 * core::mem::size_of::<u32>() as u32,
    ) else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu/rect-api: failed rung=surface\n"
        );
        return false;
    };
    let Some(mask_surface) = GpgpuMask8Surface::new(
        state.clear_test_phys,
        DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE,
        CLEAR_RECT_TEST_BYTES,
        4,
        1,
        4,
    ) else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu/rect-api: failed rung=mask-surface\n"
        );
        return false;
    };

    direct_rcs_seed_rect_api_smoke(state);
    direct_rcs_seed_rect_api_glyph_mask(state);
    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu/rect-api: ops=fill_rect_white,fill_rect_color,copy_rect,blit_glyph,glyph_mask,copy_many_rects fill_artifact={} fill_color_artifact={} copy_artifact={} glyph_mask_artifact={} surface_gpu=0x{:X} surface_phys=0x{:X} surface={}x{} pitch={} lowering=row-spans copy_pixels_per_lane={} copy_max_span_px={} fill_max_span_px=16 glyph_mask_max_span_px=16\n",
        FILL_RECT_RGBA8_KERNEL_NAME,
        FILL_RECT_RGBA8_KERNEL_NAME,
        COPY_RECT_RGBA8_KERNEL_NAME,
        GLYPH_MASK_RGBA8_KERNEL_NAME,
        surface.gpu,
        surface.phys,
        surface.width,
        surface.height,
        surface.pitch_bytes,
        COPY_RECT_PIXELS_PER_LANE,
        COPY_RECT_SPAN_PIXELS,
    );

    let rect_api_start_tick = direct_rcs_now_tick();
    let fill_start_tick = direct_rcs_now_tick();
    let fill_spans =
        fill_rect_rgba8(surface, GpgpuRect::new(20, 0, 4, 1), CLEAR_RECT_EXPECTED_WHITE);
    let fill_ms = direct_rcs_elapsed_ms_since(fill_start_tick);
    let fill_after = direct_rcs_read_rect_api_span(state, 20);
    let fill_white = direct_rcs_count_white(fill_after);
    let copy_start_tick = direct_rcs_now_tick();
    let copy_stats =
        copy_rect_rgba8_stats(surface, GpgpuRect::new(0, 0, 4, 1), surface, GpgpuPoint::new(4, 0));
    let copy_ms = direct_rcs_elapsed_ms_since(copy_start_tick);
    let blit_start_tick = direct_rcs_now_tick();
    let blit_stats = blit_glyph_rgba8_stats(GpgpuGlyphBlit {
        atlas: surface,
        glyph_rect: GpgpuRect::new(8, 0, 4, 1),
        dst: surface,
        dst_xy: GpgpuPoint::new(12, 0),
    });
    let blit_ms = direct_rcs_elapsed_ms_since(blit_start_tick);
    let many = [GpgpuCopyRect {
        src: surface,
        src_rect: GpgpuRect::new(16, 0, 4, 1),
        dst: surface,
        dst_xy: GpgpuPoint::new(20, 0),
    }];
    let many_start_tick = direct_rcs_now_tick();
    let many_stats = copy_rects_rgba8_stats(&many);
    let many_ms = direct_rcs_elapsed_ms_since(many_start_tick);
    let fill_color = 0xFFCC_8844;
    let fill_color_start_tick = direct_rcs_now_tick();
    let fill_color_spans = fill_rect_rgba8(surface, GpgpuRect::new(24, 0, 4, 1), fill_color);
    let fill_color_ms = direct_rcs_elapsed_ms_since(fill_color_start_tick);
    let glyph_mask_start_tick = direct_rcs_now_tick();
    let glyph_mask_stats = glyph_mask_rgba8_stats(GpgpuGlyphMaskBlit {
        mask: mask_surface,
        mask_rect: GpgpuRect::new(0, 0, 4, 1),
        dst: surface,
        dst_xy: GpgpuPoint::new(28, 0),
        color_rgba: 0xFFFF_FFFF,
    });
    let glyph_mask_ms = direct_rcs_elapsed_ms_since(glyph_mask_start_tick);
    let total_ms = direct_rcs_elapsed_ms_since(rect_api_start_tick);

    let src_a = direct_rcs_read_rect_api_span(state, 0);
    let dst_a = direct_rcs_read_rect_api_span(state, 4);
    let src_b = direct_rcs_read_rect_api_span(state, 8);
    let dst_b = direct_rcs_read_rect_api_span(state, 12);
    let src_c = direct_rcs_read_rect_api_span(state, 16);
    let dst_c = direct_rcs_read_rect_api_span(state, 20);
    let fill_color_after = direct_rcs_read_rect_api_span(state, 24);
    let glyph_mask_after = direct_rcs_read_rect_api_span(state, 28);
    let copy_ok = dst_a == src_a;
    let blit_ok = dst_b == src_b;
    let many_ok = dst_c == src_c;
    let fill_ok = fill_spans == 1 && fill_white == 4;
    let fill_color_ok = fill_color_spans == 1 && fill_color_after == [fill_color; 4];
    let glyph_mask_expected = [0xFF00_0000, 0xFF55_5555, 0xFFAA_AAAA, 0xFFFF_FFFF];
    let glyph_mask_ok = glyph_mask_stats.spans == 1
        && glyph_mask_stats.submits == 1
        && glyph_mask_after == glyph_mask_expected;
    let ok = fill_ok
        && fill_color_ok
        && glyph_mask_ok
        && copy_stats.spans == 1
        && copy_stats.submits == 1
        && blit_stats.spans == 1
        && blit_stats.submits == 1
        && many_stats.spans == 1
        && many_stats.submits == 1
        && copy_ok
        && blit_ok
        && many_ok;

    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu/rect-api: result ok={} total_ms={} fill_ms={} fill_color_ms={} glyph_mask_ms={} copy_ms={} blit_ms={} many_ms={} fill_spans={} fill_submits={} fill_white={}/4 fill_color_spans={} fill_color_submits={} fill_color_ok={} fill_color=0x{:08X} glyph_mask_spans={} glyph_mask_submits={} glyph_mask_ok={} copy_spans={} copy_submits={} blit_spans={} blit_submits={} many_spans={} many_submits={} copy_ok={} blit_ok={} many_ok={} fill_after=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] fill_color_after=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] glyph_mask_after=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] copy_dst=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] blit_dst=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] many_dst=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}]\n",
        ok as u8,
        total_ms,
        fill_ms,
        fill_color_ms,
        glyph_mask_ms,
        copy_ms,
        blit_ms,
        many_ms,
        fill_spans,
        fill_spans,
        fill_white,
        fill_color_spans,
        fill_color_spans,
        fill_color_ok as u8,
        fill_color,
        glyph_mask_stats.spans,
        glyph_mask_stats.submits,
        glyph_mask_ok as u8,
        copy_stats.spans,
        copy_stats.submits,
        blit_stats.spans,
        blit_stats.submits,
        many_stats.spans,
        many_stats.submits,
        copy_ok as u8,
        blit_ok as u8,
        many_ok as u8,
        fill_after[0],
        fill_after[1],
        fill_after[2],
        fill_after[3],
        fill_color_after[0],
        fill_color_after[1],
        fill_color_after[2],
        fill_color_after[3],
        glyph_mask_after[0],
        glyph_mask_after[1],
        glyph_mask_after[2],
        glyph_mask_after[3],
        dst_a[0],
        dst_a[1],
        dst_a[2],
        dst_a[3],
        dst_b[0],
        dst_b[1],
        dst_b[2],
        dst_b[3],
        dst_c[0],
        dst_c[1],
        dst_c[2],
        dst_c[3],
    );

    ok
}

pub(crate) fn submit_fill_rect_worklist_rgba8_probe_once() -> bool {
    submit_fill_rect_worklist_rgba8_probe(false)
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
    let submitted = submit_fill_rect_worklist(surface, desc, params);
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

pub(crate) fn submit_gradient_rect_worklist_rgba8_probe_once() -> bool {
    submit_gradient_rect_worklist_rgba8_probe(false)
}

pub(crate) fn submit_gradient_rect_worklist_rgba8_probe_now() -> bool {
    submit_gradient_rect_worklist_rgba8_probe(true)
}

fn submit_gradient_rect_worklist_rgba8_probe(force: bool) -> bool {
    if !DIRECT_RCS_ENABLED {
        if force {
            GRADIENT_RECT_WORKLIST_OK.store(false, Ordering::Release);
        }
        return false;
    }
    if !force && GRADIENT_RECT_WORKLIST_RAN.swap(true, Ordering::AcqRel) {
        return false;
    }
    GRADIENT_RECT_WORKLIST_RAN.store(true, Ordering::Release);
    GRADIENT_RECT_WORKLIST_OK.store(false, Ordering::Release);

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: gradient-rect-worklist-rgba8 skipped reason=no-claimed-device\n"
        );
        return false;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: gradient-rect-worklist-rgba8 failed rung=alloc\n"
        );
        return false;
    };
    let Some(desc) = rect_worklist_desc_buffer_once() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: gradient-rect-worklist-rgba8 failed rung=desc-buffer\n"
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
            "intel/gpgpu: gradient-rect-worklist-rgba8 failed rung=surface\n"
        );
        return false;
    };

    let _desc_guard = RECT_WORKLIST_DESC_SUBMIT_LOCK.lock();
    unsafe {
        core::ptr::write_bytes(state.clear_test_virt, 0, CLEAR_RECT_TEST_BYTES);
        core::ptr::write_bytes(desc.virt, 0, desc.bytes);
        let descs = desc.virt as *mut GradientRectWorklistRgba8Desc;
        core::ptr::write_volatile(
            descs,
            GradientRectWorklistRgba8Desc {
                dst_xy: pack_i16_pair_u32(0, 0),
                size: pack_u16_pair_u32(4, 1),
                color0_rgba: 0xFF00_0000,
                color1_rgba: 0xFFFF_FFFF,
                flags: 0,
            },
        );
        core::ptr::write_volatile(
            descs.add(1),
            GradientRectWorklistRgba8Desc {
                dst_xy: pack_i16_pair_u32(8, 0),
                size: pack_u16_pair_u32(4, 4),
                color0_rgba: 0xFF00_0000,
                color1_rgba: 0xFFFF_FFFF,
                flags: GRADIENT_RECT_WORKLIST_FLAG_VERTICAL,
            },
        );
    }
    super::dma_flush(state.clear_test_virt, CLEAR_RECT_TEST_BYTES);
    super::dma_flush(desc.virt, desc.bytes);

    let params = GradientRectWorklistRgba8Params {
        dst_gpu: surface.gpu,
        desc_gpu: desc.gpu,
        dst_pitch_bytes: surface.pitch_bytes,
        desc_base: 0,
        desc_count: 2,
    };
    let start_tick = direct_rcs_now_tick();
    let submitted = submit_gradient_rect_worklist(surface, desc, params);
    let submit_ms = direct_rcs_elapsed_ms_since(start_tick);
    let pre_marker = direct_rcs_read_result_slot(state, RECT_WORKLIST_PRE_MARKER_SLOT);
    let post_marker = direct_rcs_read_result_slot(state, RECT_WORKLIST_POST_MARKER_SLOT);
    let horizontal = direct_rcs_read_worklist_probe_span(state, 0, 0);
    let vertical0 = direct_rcs_read_worklist_probe_span(state, 0, 8);
    let vertical1 = direct_rcs_read_worklist_probe_span(state, 1, 8);
    let vertical2 = direct_rcs_read_worklist_probe_span(state, 2, 8);
    let vertical3 = direct_rcs_read_worklist_probe_span(state, 3, 8);
    let ramp = [0xFF00_0000, 0xFF55_5555, 0xFFAA_AAAA, 0xFFFF_FFFF];
    let ok = submitted
        && pre_marker == GRADIENT_RECT_WORKLIST_PRE_MARKER
        && post_marker == GRADIENT_RECT_WORKLIST_POST_MARKER
        && horizontal == ramp
        && vertical0 == [ramp[0]; 4]
        && vertical1 == [ramp[1]; 4]
        && vertical2 == [ramp[2]; 4]
        && vertical3 == [ramp[3]; 4];

    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: gradient-rect-worklist-rgba8 forcewake=1 ggtt=1 ppgtt=1 kernel_ppgtt=1 dst_ppgtt=1 desc_ppgtt=1 batch=1 submitted={} ok={} submit_ms={} descs=2 walkers={} pre_marker=0x{:08X} post_marker=0x{:08X} expected_post=0x{:08X} kernel_gpu=0x{:X} kernel_text_gpu=0x{:X} dst_gpu=0x{:X} desc_gpu=0x{:X} horizontal=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] vertical0=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] vertical3=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] artifact={}\n",
        submitted as u8,
        ok as u8,
        submit_ms,
        rect_worklist_walker_count(2),
        pre_marker,
        post_marker,
        GRADIENT_RECT_WORKLIST_POST_MARKER,
        GRADIENT_RECT_WORKLIST_RGBA8_ADLS_GPU,
        GRADIENT_RECT_WORKLIST_RGBA8_ADLS_GPU + GRADIENT_RECT_WORKLIST_RGBA8_TEXT_OFFSET_BYTES,
        surface.gpu,
        desc.gpu,
        horizontal[0],
        horizontal[1],
        horizontal[2],
        horizontal[3],
        vertical0[0],
        vertical0[1],
        vertical0[2],
        vertical0[3],
        vertical3[0],
        vertical3[1],
        vertical3[2],
        vertical3[3],
        GRADIENT_RECT_WORKLIST_RGBA8_KERNEL_NAME,
    );

    GRADIENT_RECT_WORKLIST_OK.store(ok, Ordering::Release);
    ok
}

pub(crate) fn submit_alpha_blend_worklist_rgba8_probe_once() -> bool {
    submit_alpha_blend_worklist_rgba8_probe(false)
}

pub(crate) fn submit_alpha_blend_worklist_rgba8_probe_now() -> bool {
    submit_alpha_blend_worklist_rgba8_probe(true)
}

fn submit_alpha_blend_worklist_rgba8_probe(force: bool) -> bool {
    if !DIRECT_RCS_ENABLED {
        if force {
            ALPHA_BLEND_WORKLIST_OK.store(false, Ordering::Release);
        }
        return false;
    }
    if !force && ALPHA_BLEND_WORKLIST_RAN.swap(true, Ordering::AcqRel) {
        return false;
    }
    ALPHA_BLEND_WORKLIST_RAN.store(true, Ordering::Release);
    ALPHA_BLEND_WORKLIST_OK.store(false, Ordering::Release);

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: alpha-blend-worklist-rgba8 skipped reason=no-claimed-device\n"
        );
        return false;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: alpha-blend-worklist-rgba8 failed rung=alloc\n"
        );
        return false;
    };
    let Some(desc) = rect_worklist_desc_buffer_once() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: alpha-blend-worklist-rgba8 failed rung=desc-buffer\n"
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
            "intel/gpgpu: alpha-blend-worklist-rgba8 failed rung=surface\n"
        );
        return false;
    };

    let _desc_guard = RECT_WORKLIST_DESC_SUBMIT_LOCK.lock();
    let src_values = [0x8000_00FF, 0x8000_FF00, 0x80FF_0000, 0x4000_FFFF];
    let copy_values = [0x7F11_2233, 0x8044_5566, 0xC077_8899, 0xFFA0_B0C0];
    let dst_values = [0xFF00_0000, 0xFF00_0000, 0xFF00_0000, 0xFF20_2020];
    let mut expected = [0u32; 4];
    for index in 0..expected.len() {
        expected[index] = src_over_rgba8_u32(src_values[index], dst_values[index]);
    }

    unsafe {
        core::ptr::write_bytes(state.clear_test_virt, 0, CLEAR_RECT_TEST_BYTES);
        core::ptr::write_bytes(desc.virt, 0, desc.bytes);
        let pixels = state.clear_test_virt as *mut u32;
        for index in 0..4usize {
            core::ptr::write_volatile(pixels.add(index), src_values[index]);
            core::ptr::write_volatile(pixels.add(4 + index), copy_values[index]);
            core::ptr::write_volatile(pixels.add(8 + index), dst_values[index]);
            core::ptr::write_volatile(pixels.add(16 + index), 0xCC00_0000 | index as u32);
        }
        let descs = desc.virt as *mut AlphaBlendWorklistRgba8Desc;
        core::ptr::write_volatile(
            descs,
            AlphaBlendWorklistRgba8Desc {
                src_xy: pack_u16_pair_u32(0, 0),
                dst_xy: pack_i16_pair_u32(8, 0),
                size: pack_u16_pair_u32(4, 1),
                flags: COMPOSITE_WORKLIST_FLAG_SRC_OVER,
                color_rgba: COMPOSITE_WORKLIST_NEUTRAL_COLOR_RGBA,
            },
        );
        core::ptr::write_volatile(
            descs.add(1),
            AlphaBlendWorklistRgba8Desc {
                src_xy: pack_u16_pair_u32(4, 0),
                dst_xy: pack_i16_pair_u32(16, 0),
                size: pack_u16_pair_u32(4, 1),
                flags: COMPOSITE_WORKLIST_FLAG_COPY,
                color_rgba: COMPOSITE_WORKLIST_NEUTRAL_COLOR_RGBA,
            },
        );
    }
    super::dma_flush(state.clear_test_virt, CLEAR_RECT_TEST_BYTES);
    super::dma_flush(desc.virt, desc.bytes);

    let params = AlphaBlendWorklistRgba8Params {
        src_gpu: surface.gpu,
        dst_gpu: surface.gpu,
        desc_gpu: desc.gpu,
        src_pitch_bytes: surface.pitch_bytes,
        dst_pitch_bytes: surface.pitch_bytes,
        desc_base: 0,
        desc_count: 2,
    };
    let start_tick = direct_rcs_now_tick();
    let submitted = submit_alpha_blend_worklist(surface, surface, desc, params);
    let submit_ms = direct_rcs_elapsed_ms_since(start_tick);
    let pre_marker = direct_rcs_read_result_slot(state, RECT_WORKLIST_PRE_MARKER_SLOT);
    let post_marker = direct_rcs_read_result_slot(state, RECT_WORKLIST_POST_MARKER_SLOT);
    let src_after = direct_rcs_read_worklist_probe_span(state, 0, 0);
    let dst_after = direct_rcs_read_worklist_probe_span(state, 0, 8);
    let copy_after = direct_rcs_read_worklist_probe_span(state, 0, 16);
    let ok = submitted
        && pre_marker == ALPHA_BLEND_WORKLIST_PRE_MARKER
        && post_marker == ALPHA_BLEND_WORKLIST_POST_MARKER
        && src_after == src_values
        && dst_after == expected
        && copy_after == copy_values;

    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: alpha-blend-worklist-rgba8 forcewake=1 ggtt=1 ppgtt=1 kernel_ppgtt=1 src_ppgtt=1 dst_ppgtt=1 desc_ppgtt=1 batch=1 submitted={} ok={} submit_ms={} descs=2 walkers={} flags_src_over=0x{:X} flags_copy=0x{:X} color=0x{:08X} pre_marker=0x{:08X} post_marker=0x{:08X} expected_post=0x{:08X} kernel_gpu=0x{:X} kernel_text_gpu=0x{:X} src_gpu=0x{:X} dst_gpu=0x{:X} desc_gpu=0x{:X} src_after=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] dst_after=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] expected=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] copy_after=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] artifact={}\n",
        submitted as u8,
        ok as u8,
        submit_ms,
        rect_worklist_walker_count(2),
        COMPOSITE_WORKLIST_FLAG_SRC_OVER,
        COMPOSITE_WORKLIST_FLAG_COPY,
        COMPOSITE_WORKLIST_NEUTRAL_COLOR_RGBA,
        pre_marker,
        post_marker,
        ALPHA_BLEND_WORKLIST_POST_MARKER,
        ALPHA_BLEND_WORKLIST_RGBA8_ADLS_GPU,
        ALPHA_BLEND_WORKLIST_RGBA8_ADLS_GPU + ALPHA_BLEND_WORKLIST_RGBA8_TEXT_OFFSET_BYTES,
        surface.gpu,
        surface.gpu,
        desc.gpu,
        src_after[0],
        src_after[1],
        src_after[2],
        src_after[3],
        dst_after[0],
        dst_after[1],
        dst_after[2],
        dst_after[3],
        expected[0],
        expected[1],
        expected[2],
        expected[3],
        copy_after[0],
        copy_after[1],
        copy_after[2],
        copy_after[3],
        ALPHA_BLEND_WORKLIST_RGBA8_KERNEL_NAME,
    );

    ALPHA_BLEND_WORKLIST_OK.store(ok, Ordering::Release);
    ok
}

fn shell_surface_once() -> Option<GpgpuShellSurface> {
    let mut guard = GPGPU_SHELL_SURFACE.lock();
    if let Some(shell) = *guard {
        return Some(shell);
    }

    let (phys, virt) = crate::dma::alloc(GPGPU_SHELL_SURFACE_BYTES, super::WARM_ALIGN)?;
    unsafe {
        core::ptr::write_bytes(virt, 0, GPGPU_SHELL_SURFACE_BYTES);
    }
    super::dma_flush(virt, GPGPU_SHELL_SURFACE_BYTES);

    let Some(surface) = GpgpuRgba8Surface::new(
        phys,
        DIRECT_RCS_GPU_VA_SHELL_SURFACE_BASE,
        GPGPU_SHELL_SURFACE_BYTES,
        GPGPU_SHELL_SURFACE_WIDTH,
        GPGPU_SHELL_SURFACE_HEIGHT,
        GPGPU_SHELL_SURFACE_PITCH_BYTES,
    ) else {
        crate::dma::dealloc(virt, GPGPU_SHELL_SURFACE_BYTES);
        return None;
    };

    let shell = GpgpuShellSurface { surface, virt };
    *guard = Some(shell);
    Some(shell)
}

fn present_staging_surface_once(width: u32, height: u32) -> Option<GpgpuPresentStagingSurface> {
    let pitch_bytes = width.checked_mul(core::mem::size_of::<u32>() as u32)?;
    let raw_bytes = (pitch_bytes as usize).checked_mul(height as usize)?;
    let bytes = align_up(raw_bytes, super::WARM_ALIGN)?;
    let mut guard = GPGPU_PRESENT_STAGING_SURFACE.lock();
    if let Some(surface) = *guard
        && surface.surface.width == width
        && surface.surface.height == height
        && surface.surface.pitch_bytes == pitch_bytes
        && surface.surface.bytes >= raw_bytes
    {
        return Some(surface);
    }

    if let Some(old) = guard.take() {
        crate::dma::dealloc(old.virt, old.surface.bytes);
    }

    let (phys, virt) = crate::dma::alloc(bytes, super::WARM_ALIGN)?;
    unsafe {
        core::ptr::write_bytes(virt, 0, bytes);
    }
    super::dma_flush(virt, bytes);

    let Some(surface) = GpgpuRgba8Surface::new(
        phys,
        DIRECT_RCS_GPU_VA_PRESENT_STAGING_BASE,
        bytes,
        width,
        height,
        pitch_bytes,
    ) else {
        crate::dma::dealloc(virt, bytes);
        return None;
    };
    let staging = GpgpuPresentStagingSurface { surface, virt };
    *guard = Some(staging);
    Some(staging)
}

fn solid_rect_source_surface_once(width: u32, height: u32) -> Option<GpgpuSolidRectSourceSurface> {
    let pitch_bytes = width.checked_mul(core::mem::size_of::<u32>() as u32)?;
    let raw_bytes = (pitch_bytes as usize).checked_mul(height as usize)?;
    let bytes = align_up(raw_bytes, super::WARM_ALIGN)?;
    let mut guard = GPGPU_SOLID_RECT_SOURCE_SURFACE.lock();
    if let Some(surface) = *guard
        && surface.surface.width == width
        && surface.surface.height == height
        && surface.surface.pitch_bytes == pitch_bytes
        && surface.surface.bytes >= raw_bytes
    {
        return Some(surface);
    }

    if let Some(old) = guard.take() {
        crate::dma::dealloc(old.virt, old.surface.bytes);
    }

    let (phys, virt) = crate::dma::alloc(bytes, super::WARM_ALIGN)?;
    unsafe {
        core::ptr::write_bytes(virt, 0, bytes);
    }
    super::dma_flush(virt, bytes);

    let Some(surface) = GpgpuRgba8Surface::new(
        phys,
        DIRECT_RCS_GPU_VA_SOLID_RECT_SOURCE_BASE,
        bytes,
        width,
        height,
        pitch_bytes,
    ) else {
        crate::dma::dealloc(virt, bytes);
        return None;
    };
    let staging = GpgpuSolidRectSourceSurface { surface, virt };
    *guard = Some(staging);
    Some(staging)
}

fn stage_rgba_as_primary_xrgb(
    staging: GpgpuPresentStagingSurface,
    src: &[u8],
    width: u32,
    height: u32,
    src_pitch_bytes: usize,
) {
    let dst_pitch_pixels = staging.surface.pitch_bytes as usize / core::mem::size_of::<u32>();
    let dst = staging.virt as *mut u32;
    for y in 0..height as usize {
        let src_row_off = y.saturating_mul(src_pitch_bytes);
        let Some(src_row) = src.get(src_row_off..src_row_off.saturating_add(width as usize * 4))
        else {
            break;
        };
        let dst_row = unsafe { dst.add(y.saturating_mul(dst_pitch_pixels)) };
        for x in 0..width as usize {
            let src_off = x.saturating_mul(4);
            let r = src_row[src_off];
            let g = src_row[src_off + 1];
            let b = src_row[src_off + 2];
            let pixel = u32::from_le_bytes([b, g, r, 0]);
            unsafe {
                core::ptr::write_volatile(dst_row.add(x), pixel);
            }
        }
    }
    let flush_bytes = (height as usize)
        .saturating_sub(1)
        .saturating_mul(staging.surface.pitch_bytes as usize)
        .saturating_add((width as usize).saturating_mul(core::mem::size_of::<u32>()));
    super::dma_flush(staging.virt, flush_bytes);
}

fn stage_rgba_scene(
    staging: GpgpuPresentStagingSurface,
    src: &[u8],
    width: u32,
    height: u32,
    src_pitch_bytes: usize,
) {
    let dst_pitch_bytes = staging.surface.pitch_bytes as usize;
    for y in 0..height as usize {
        let src_row_off = y.saturating_mul(src_pitch_bytes);
        let Some(src_row) = src.get(src_row_off..src_row_off.saturating_add(width as usize * 4))
        else {
            break;
        };
        let dst_row = unsafe { staging.virt.add(y.saturating_mul(dst_pitch_bytes)) };
        unsafe {
            core::ptr::copy_nonoverlapping(src_row.as_ptr(), dst_row, src_row.len());
        }
    }
    let flush_bytes = (height as usize)
        .saturating_sub(1)
        .saturating_mul(staging.surface.pitch_bytes as usize)
        .saturating_add((width as usize).saturating_mul(core::mem::size_of::<u32>()));
    super::dma_flush(staging.virt, flush_bytes);
}

fn sprite64_worklist_atlas_once() -> Option<GpgpuSprite64WorklistAtlasSurface> {
    GPGPU_SPRITE64_WORKLIST_ATLAS
        .call_once(|| {
            let twemoji_atlas = twemoji_atlas_cache_once()?;
            let twemoji_slot_count = crate::gfx::althlasfont::twemoji::twemoji_slot_count();
            let font_slot_count = sprite64_font_slot_count()?;
            let slot_count = u32::from(twemoji_slot_count).checked_add(font_slot_count)?;
            if slot_count == 0 || slot_count > u32::from(u16::MAX) {
                return None;
            }
            let columns = SPRITE64_WORKLIST_ATLAS_COLUMNS;
            let rows = slot_count.div_ceil(columns);
            let width = columns.saturating_mul(SPRITE64_WORKLIST_CELL_PIXELS);
            let height = rows.saturating_mul(SPRITE64_WORKLIST_CELL_PIXELS);
            let pitch_bytes = width.checked_mul(core::mem::size_of::<u32>() as u32)?;
            let bytes = (pitch_bytes as usize).checked_mul(height as usize)?;
            let (phys, virt) = crate::dma::alloc(bytes, super::WARM_ALIGN)?;

            unsafe {
                core::ptr::write_bytes(virt, 0, bytes);
            }

            let dst = virt as *mut u32;
            let dst_pitch_pixels = width as usize;
            for slot in 0..twemoji_slot_count {
                let Some(region) =
                    crate::gfx::althlasfont::twemoji::twemoji_lookup_slot_region(slot)
                else {
                    continue;
                };
                let src_w = u32::from(region.src_w)
                    .min(SPRITE64_WORKLIST_CELL_PIXELS)
                    .min(twemoji_atlas.width.saturating_sub(u32::from(region.src_x)));
                let src_h = u32::from(region.src_h)
                    .min(SPRITE64_WORKLIST_CELL_PIXELS)
                    .min(twemoji_atlas.height.saturating_sub(u32::from(region.src_y)));
                if src_w == 0 || src_h == 0 {
                    continue;
                }

                let cell_x = (u32::from(slot) % columns) * SPRITE64_WORKLIST_CELL_PIXELS;
                let cell_y = (u32::from(slot) / columns) * SPRITE64_WORKLIST_CELL_PIXELS;
                let pad_x = (SPRITE64_WORKLIST_CELL_PIXELS - src_w) / 2;
                let pad_y = (SPRITE64_WORKLIST_CELL_PIXELS - src_h) / 2;

                for y in 0..src_h {
                    for x in 0..src_w {
                        let atlas_x = u32::from(region.src_x) + x;
                        let atlas_y = u32::from(region.src_y) + y;
                        let src_idx = ((atlas_y as usize) * (twemoji_atlas.width as usize)
                            + atlas_x as usize)
                            * 4;
                        let r = *twemoji_atlas.rgba.get(src_idx)? as u32;
                        let g = *twemoji_atlas.rgba.get(src_idx + 1)? as u32;
                        let b = *twemoji_atlas.rgba.get(src_idx + 2)? as u32;
                        let a = *twemoji_atlas.rgba.get(src_idx + 3)? as u32;
                        let out_x = cell_x + pad_x + x;
                        let out_y = cell_y + pad_y + y;
                        let dst_idx = (out_y as usize) * dst_pitch_pixels + out_x as usize;
                        unsafe {
                            core::ptr::write_volatile(
                                dst.add(dst_idx),
                                (a << 24) | (r << 16) | (g << 8) | b,
                            );
                        }
                    }
                }
            }

            for face in crate::gfx::althlasfont::bitmapfont::ATHLAS_SPRITE64_FONT_FACES {
                let bucket_pngs = sprite64_font_bucket_pngs(face)?;
                for (bucket, png) in bucket_pngs.iter().enumerate() {
                    let decoded = crate::gfx::png_codec::decode_png_rgba(png).ok()?;
                    let metrics =
                        crate::gfx::althlasfont::bitmapfont::athlas_font_bucket_atlas_metrics(
                            face, bucket,
                        )?;
                    let base_slot = u32::from(sprite64_font_bucket_base(face, bucket)?);
                    let bucket_slots = u32::from(metrics.grid_w.max(1))
                        .saturating_mul(u32::from(metrics.grid_h.max(1)));
                    let src_w = u32::from(metrics.cell_w).min(SPRITE64_WORKLIST_CELL_PIXELS);
                    let src_h = u32::from(metrics.cell_h).min(SPRITE64_WORKLIST_CELL_PIXELS);
                    if src_w == 0 || src_h == 0 {
                        continue;
                    }

                    for slot in 0..bucket_slots {
                        let src_cell_x = (slot % u32::from(metrics.grid_w.max(1)))
                            .saturating_mul(u32::from(metrics.cell_w));
                        let src_cell_y = (slot / u32::from(metrics.grid_w.max(1)))
                            .saturating_mul(u32::from(metrics.cell_h));
                        let global_slot = base_slot.saturating_add(slot);
                        let cell_x = (global_slot % columns) * SPRITE64_WORKLIST_CELL_PIXELS;
                        let cell_y = (global_slot / columns) * SPRITE64_WORKLIST_CELL_PIXELS;

                        for y in 0..src_h {
                            let atlas_y = src_cell_y + y;
                            if atlas_y >= decoded.height {
                                continue;
                            }
                            for x in 0..src_w {
                                let atlas_x = src_cell_x + x;
                                if atlas_x >= decoded.width {
                                    continue;
                                }
                                let src_idx = ((atlas_y as usize) * (decoded.width as usize)
                                    + atlas_x as usize)
                                    * 4;
                                let r = *decoded.rgba.get(src_idx)? as u32;
                                let g = *decoded.rgba.get(src_idx + 1)? as u32;
                                let b = *decoded.rgba.get(src_idx + 2)? as u32;
                                let src_a = *decoded.rgba.get(src_idx + 3)? as u32;
                                let mask = r.max(g).max(b);
                                let a = (mask.saturating_mul(src_a).saturating_add(127)) / 255;
                                if a == 0 {
                                    continue;
                                }
                                let out_x = cell_x + x;
                                let out_y = cell_y + y;
                                let dst_idx = (out_y as usize) * dst_pitch_pixels + out_x as usize;
                                unsafe {
                                    core::ptr::write_volatile(
                                        dst.add(dst_idx),
                                        (a << 24) | 0x00FF_FFFF,
                                    );
                                }
                            }
                        }
                    }
                }
            }

            super::dma_flush(virt, bytes);
            let Some(surface) = GpgpuRgba8Surface::new(
                phys,
                SPRITE64_WORKLIST_ATLAS_GPU,
                bytes,
                width,
                height,
                pitch_bytes,
            ) else {
                crate::dma::dealloc(virt, bytes);
                return None;
            };

            Some(GpgpuSprite64WorklistAtlasSurface {
                surface,
                columns,
                slots: slot_count as u16,
            })
        })
        .as_ref()
        .copied()
}

fn sprite64_worklist_desc_buffer_once() -> Option<GpgpuSprite64WorklistDescBuffer> {
    let mut guard = GPGPU_SPRITE64_WORKLIST_DESC.lock();
    if let Some(buffer) = *guard {
        return Some(buffer);
    }

    let bytes = align_up(SPRITE64_WORKLIST_DESC_BYTES, super::WARM_ALIGN)?;
    let (phys, virt) = crate::dma::alloc(bytes, super::WARM_ALIGN)?;
    unsafe {
        core::ptr::write_bytes(virt, 0, bytes);
    }
    super::dma_flush(virt, bytes);

    let buffer = GpgpuSprite64WorklistDescBuffer {
        phys,
        gpu: SPRITE64_WORKLIST_DESC_GPU,
        virt,
        bytes,
    };
    *guard = Some(buffer);
    Some(buffer)
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

fn shell_zero_surface(shell: GpgpuShellSurface) {
    unsafe {
        core::ptr::write_bytes(shell.virt, 0, shell.surface.bytes);
    }
}

fn shell_seed_rect(shell: GpgpuShellSurface, rect: GpgpuRect) {
    for y in 0..rect.height {
        for x in 0..rect.width {
            shell_write_pixel(
                shell,
                rect.x as u32 + x,
                rect.y as u32 + y,
                shell_pattern_pixel(x, y),
            );
        }
    }
}

fn shell_seed_copy(shell: GpgpuShellSurface, src_rect: GpgpuRect, dst_xy: GpgpuPoint) {
    for y in 0..src_rect.height {
        for x in 0..src_rect.width {
            let src_pixel = shell_pattern_pixel(x, y);
            shell_write_pixel(shell, src_rect.x as u32 + x, src_rect.y as u32 + y, src_pixel);
            shell_write_pixel(
                shell,
                dst_xy.x as u32 + x,
                dst_xy.y as u32 + y,
                shell_poison_pixel(x, y),
            );
        }
    }
}

fn rect_is_inside_atlas(width: u32, height: u32, rect: GpgpuRect) -> bool {
    if rect.is_empty() || rect.x < 0 || rect.y < 0 {
        return false;
    }
    let x2 = rect.x as i64 + rect.width as i64;
    let y2 = rect.y as i64 + rect.height as i64;
    x2 <= width as i64 && y2 <= height as i64
}

fn twemoji_atlas_cache_once() -> Option<&'static GpgpuTwemojiAtlasCache> {
    GPGPU_TWEMOJI_ATLAS
        .call_once(|| {
            let decoded = crate::gfx::png_codec::decode_png_rgba(
                crate::gfx::althlasfont::twemoji::TWEMOJI_ATLAS_PNG,
            )
            .ok()?;
            Some(GpgpuTwemojiAtlasCache {
                width: decoded.width,
                height: decoded.height,
                rgba: decoded.rgba,
            })
        })
        .as_ref()
}

fn shell_read_rect_head(shell: GpgpuShellSurface, rect: GpgpuRect) -> [u32; 4] {
    let mut out = [0u32; 4];
    let count = (rect.width as usize).min(out.len());
    for (index, slot) in out.iter_mut().enumerate().take(count) {
        *slot = shell_read_pixel(shell, rect.x as u32 + index as u32, rect.y as u32);
    }
    out
}

fn shell_count_white(shell: GpgpuShellSurface, rect: GpgpuRect) -> usize {
    let mut white = 0usize;
    for y in 0..rect.height {
        for x in 0..rect.width {
            if shell_read_pixel(shell, rect.x as u32 + x, rect.y as u32 + y)
                == CLEAR_RECT_EXPECTED_WHITE
            {
                white += 1;
            }
        }
    }
    white
}

fn shell_count_copy(
    shell: GpgpuShellSurface,
    src_rect: GpgpuRect,
    dst_xy: GpgpuPoint,
) -> (usize, usize) {
    let mut src_preserved = 0usize;
    let mut copied = 0usize;
    for y in 0..src_rect.height {
        for x in 0..src_rect.width {
            let expected = shell_pattern_pixel(x, y);
            let src = shell_read_pixel(shell, src_rect.x as u32 + x, src_rect.y as u32 + y);
            let dst = shell_read_pixel(shell, dst_xy.x as u32 + x, dst_xy.y as u32 + y);
            if src == expected {
                src_preserved += 1;
            }
            if dst == expected {
                copied += 1;
            }
        }
    }
    (src_preserved, copied)
}

fn primary_read_rect_head(
    target: super::display::PrimarySurfaceGpgpuTarget,
    rect: GpgpuRect,
) -> [u32; 4] {
    let mut out = [0u32; 4];
    let count = (rect.width as usize).min(out.len());
    for (index, slot) in out.iter_mut().enumerate().take(count) {
        *slot = primary_read_pixel(target, rect.x as u32 + index as u32, rect.y as u32);
    }
    out
}

fn primary_count_scanout_copy(
    target: super::display::PrimarySurfaceGpgpuTarget,
    shell: GpgpuShellSurface,
    src_rect: GpgpuRect,
    dst_xy: GpgpuPoint,
) -> (usize, usize) {
    let mut src_preserved = 0usize;
    let mut copied = 0usize;
    for y in 0..src_rect.height {
        for x in 0..src_rect.width {
            let expected = shell_pattern_pixel(x, y);
            let src = shell_read_pixel(shell, src_rect.x as u32 + x, src_rect.y as u32 + y);
            let dst = primary_read_pixel(target, dst_xy.x as u32 + x, dst_xy.y as u32 + y);
            if src == expected {
                src_preserved += 1;
            }
            if dst == expected {
                copied += 1;
            }
        }
    }
    (src_preserved, copied)
}

fn primary_read_pixel(target: super::display::PrimarySurfaceGpgpuTarget, x: u32, y: u32) -> u32 {
    let offset = (y as usize)
        .saturating_mul(target.pitch_bytes as usize)
        .saturating_add((x as usize).saturating_mul(core::mem::size_of::<u32>()));
    if offset.saturating_add(core::mem::size_of::<u32>()) > target.byte_len {
        return 0;
    }
    let ptr = unsafe { target.virt.add(offset) };
    super::dma_flush(ptr, core::mem::size_of::<u32>());
    unsafe { core::ptr::read_volatile(ptr as *const u32) }
}

fn shell_write_pixel(shell: GpgpuShellSurface, x: u32, y: u32, value: u32) {
    let offset = (y as usize)
        .saturating_mul(shell.surface.pitch_bytes as usize)
        .saturating_add((x as usize).saturating_mul(core::mem::size_of::<u32>()));
    unsafe {
        core::ptr::write_volatile(shell.virt.add(offset) as *mut u32, value);
    }
}

fn shell_read_pixel(shell: GpgpuShellSurface, x: u32, y: u32) -> u32 {
    let offset = (y as usize)
        .saturating_mul(shell.surface.pitch_bytes as usize)
        .saturating_add((x as usize).saturating_mul(core::mem::size_of::<u32>()));
    unsafe { core::ptr::read_volatile(shell.virt.add(offset) as *const u32) }
}

fn shell_pattern_pixel(x: u32, y: u32) -> u32 {
    0xFF00_0000
        | (((x.wrapping_mul(3).wrapping_add(y.wrapping_mul(85))) & 0xFF) << 16)
        | (((0xFFu32.wrapping_sub(x).wrapping_add(y.wrapping_mul(17))) & 0xFF) << 8)
        | ((x.wrapping_add(y.wrapping_mul(31))) & 0xFF)
}

fn shell_poison_pixel(x: u32, y: u32) -> u32 {
    0xA500_0000 | ((y & 0xFF) << 16) | ((x & 0xFF) << 8) | 0x5A
}

fn rect_pixel_count(rect: GpgpuRect) -> usize {
    (rect.width as usize).saturating_mul(rect.height as usize)
}

fn copy_rect_expected_spans(rect: GpgpuRect) -> usize {
    copy_rect_expected_spans_for(rect, COPY_RECT_SPAN_PIXELS, 1)
}

fn copy_rect_expected_spans_for(rect: GpgpuRect, span_pixels: u32, rows_per_walker: u32) -> usize {
    (rect.width as usize)
        .div_ceil(span_pixels as usize)
        .saturating_mul((rect.height as usize).div_ceil(rows_per_walker as usize))
}

fn rect_is_inside(surface: GpgpuRgba8Surface, rect: GpgpuRect) -> bool {
    if rect.is_empty() || rect.x < 0 || rect.y < 0 {
        return false;
    }
    let x2 = rect.x as i64 + rect.width as i64;
    let y2 = rect.y as i64 + rect.height as i64;
    x2 <= surface.width as i64 && y2 <= surface.height as i64
}

#[inline]
fn rgba8_alpha(color_rgba: u32) -> u8 {
    (color_rgba >> 24) as u8
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

fn copy_rect_is_inside(
    surface: GpgpuRgba8Surface,
    src_rect: GpgpuRect,
    dst_xy: GpgpuPoint,
) -> bool {
    rect_is_inside(surface, src_rect)
        && rect_is_inside(
            surface,
            GpgpuRect::new(dst_xy.x, dst_xy.y, src_rect.width, src_rect.height),
        )
}

fn rects_overlap(a: GpgpuRect, b: GpgpuRect) -> bool {
    let ax2 = a.x as i64 + a.width as i64;
    let ay2 = a.y as i64 + a.height as i64;
    let bx2 = b.x as i64 + b.width as i64;
    let by2 = b.y as i64 + b.height as i64;
    (a.x as i64) < bx2 && (b.x as i64) < ax2 && (a.y as i64) < by2 && (b.y as i64) < ay2
}

fn same_rgba8_surface(a: GpgpuRgba8Surface, b: GpgpuRgba8Surface) -> bool {
    a.phys == b.phys
        && a.gpu == b.gpu
        && a.bytes == b.bytes
        && a.width == b.width
        && a.height == b.height
        && a.pitch_bytes == b.pitch_bytes
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

fn lower_present_rgba8_to_primary_xrgb_rect(
    src: GpgpuRgba8Surface,
    src_rect: GpgpuRect,
    dst: GpgpuRgba8Surface,
    dst_xy: GpgpuPoint,
    flip_y: bool,
) -> Option<PresentRgba8ToPrimaryXrgbRectParams> {
    let params = lower_copy_rect(src, src_rect, dst, dst_xy)?;
    Some(PresentRgba8ToPrimaryXrgbRectParams {
        src_gpu: params.src_gpu,
        dst_gpu: params.dst_gpu,
        src_pitch_bytes: params.src_pitch_bytes,
        dst_pitch_bytes: params.dst_pitch_bytes,
        src_x: params.src_x,
        src_y: params.src_y,
        dst_x: params.dst_x,
        dst_y: params.dst_y,
        width: params.width,
        height: params.height,
        flip_y: u32::from(flip_y),
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

fn submit_fill_rect_spans(dst: GpgpuRgba8Surface, params: FillRectRgba8Params) -> usize {
    let mut submitted = 0usize;
    for row in 0..params.height {
        let mut x = 0u32;
        while x < params.width {
            let span = (params.width - x).min(16);
            let span_params = FillRectRgba8Params {
                dst_x: params.dst_x + x,
                dst_y: params.dst_y + row,
                width: span,
                height: 1,
                ..params
            };
            if submit_fill_rect_span(dst, span_params) {
                submitted = submitted.saturating_add(1);
            }
            x += span;
        }
    }
    submitted
}

fn submit_copy_rect_spans_with_stats(
    src: GpgpuRgba8Surface,
    dst: GpgpuRgba8Surface,
    params: CopyRectRgba8Params,
    flavor: CopyRectKernelFlavor,
) -> GpgpuSubmitStats {
    let total_start_tick = direct_rcs_now_tick();
    let Some(total_spans) =
        copy_rect_span_count(params, flavor.span_pixels, flavor.rows_per_walker)
    else {
        return GpgpuSubmitStats::default();
    };
    let mut stats = GpgpuSubmitStats::default();
    let mut span_start = 0usize;
    while span_start < total_spans {
        let span_take = (total_spans - span_start).min(COPY_RECT_BATCH_MAX_SPANS);
        let submit_start_tick = direct_rcs_now_tick();
        if !submit_copy_rect_span_batch(src, dst, params, flavor, span_start, span_take) {
            break;
        }
        stats.submit_ms = stats
            .submit_ms
            .saturating_add(direct_rcs_elapsed_ms_since(submit_start_tick));
        stats.spans = stats.spans.saturating_add(span_take);
        stats.submits = stats.submits.saturating_add(1);
        span_start += span_take;
    }
    stats.total_ms = direct_rcs_elapsed_ms_since(total_start_tick);
    stats
}

fn submit_copy_rect_multi_ops_with_stats(
    src: GpgpuRgba8Surface,
    dst: GpgpuRgba8Surface,
    params: &[CopyRectRgba8Params],
    flavor: CopyRectKernelFlavor,
) -> GpgpuSubmitStats {
    let total_start_tick = direct_rcs_now_tick();
    let mut stats = GpgpuSubmitStats::default();
    let mut span_params = Vec::with_capacity(COPY_RECT_BATCH_MAX_SPANS.min(32));

    for op in params {
        let Some(total_spans) =
            copy_rect_span_count(*op, flavor.span_pixels, flavor.rows_per_walker)
        else {
            continue;
        };
        for span_index in 0..total_spans {
            let Some(span) =
                copy_rect_span_params(*op, flavor.span_pixels, flavor.rows_per_walker, span_index)
            else {
                return stats;
            };
            span_params.push(span);
            if span_params.len() == COPY_RECT_BATCH_MAX_SPANS {
                let submit_start_tick = direct_rcs_now_tick();
                if !submit_copy_rect_span_params_batch(src, dst, &span_params, flavor) {
                    stats.total_ms = direct_rcs_elapsed_ms_since(total_start_tick);
                    return stats;
                }
                stats.submit_ms = stats
                    .submit_ms
                    .saturating_add(direct_rcs_elapsed_ms_since(submit_start_tick));
                stats.spans = stats.spans.saturating_add(span_params.len());
                stats.submits = stats.submits.saturating_add(1);
                span_params.clear();
            }
        }
    }

    if !span_params.is_empty() {
        let submit_start_tick = direct_rcs_now_tick();
        if !submit_copy_rect_span_params_batch(src, dst, &span_params, flavor) {
            stats.total_ms = direct_rcs_elapsed_ms_since(total_start_tick);
            return stats;
        }
        stats.submit_ms = stats
            .submit_ms
            .saturating_add(direct_rcs_elapsed_ms_since(submit_start_tick));
        stats.spans = stats.spans.saturating_add(span_params.len());
        stats.submits = stats.submits.saturating_add(1);
    }
    stats.total_ms = direct_rcs_elapsed_ms_since(total_start_tick);
    stats
}

fn submit_blit_rgba8_nearest_spans_with_stats(
    src: GpgpuRgba8Surface,
    dst: GpgpuRgba8Surface,
    params: BlitRgba8NearestParams,
    flavor: CopyRectKernelFlavor,
) -> GpgpuSubmitStats {
    let total_start_tick = direct_rcs_now_tick();
    let Some(total_spans) =
        blit_rgba8_nearest_span_count(params, flavor.span_pixels, flavor.rows_per_walker)
    else {
        return GpgpuSubmitStats::default();
    };
    let mut stats = GpgpuSubmitStats::default();
    let mut span_params = Vec::with_capacity(COPY_RECT_BATCH_MAX_SPANS.min(32));

    for span_index in 0..total_spans {
        let Some(span) = blit_rgba8_nearest_span_params(
            params,
            flavor.span_pixels,
            flavor.rows_per_walker,
            span_index,
        ) else {
            return stats;
        };
        span_params.push(span);
        if span_params.len() == COPY_RECT_BATCH_MAX_SPANS {
            let submit_start_tick = direct_rcs_now_tick();
            if !submit_blit_rgba8_nearest_span_params_batch(src, dst, &span_params, flavor) {
                stats.total_ms = direct_rcs_elapsed_ms_since(total_start_tick);
                return stats;
            }
            stats.submit_ms = stats
                .submit_ms
                .saturating_add(direct_rcs_elapsed_ms_since(submit_start_tick));
            stats.spans = stats.spans.saturating_add(span_params.len());
            stats.submits = stats.submits.saturating_add(1);
            span_params.clear();
        }
    }

    if !span_params.is_empty() {
        let submit_start_tick = direct_rcs_now_tick();
        if !submit_blit_rgba8_nearest_span_params_batch(src, dst, &span_params, flavor) {
            stats.total_ms = direct_rcs_elapsed_ms_since(total_start_tick);
            return stats;
        }
        stats.submit_ms = stats
            .submit_ms
            .saturating_add(direct_rcs_elapsed_ms_since(submit_start_tick));
        stats.spans = stats.spans.saturating_add(span_params.len());
        stats.submits = stats.submits.saturating_add(1);
    }
    stats.total_ms = direct_rcs_elapsed_ms_since(total_start_tick);
    stats
}

fn submit_glyph_mask_spans_with_stats(
    mask: GpgpuMask8Surface,
    dst: GpgpuRgba8Surface,
    params: CopyRectRgba8Params,
    color_rgba: u32,
    flavor: CopyRectKernelFlavor,
) -> GpgpuSubmitStats {
    let Some(total_spans) =
        copy_rect_span_count(params, flavor.span_pixels, flavor.rows_per_walker)
    else {
        return GpgpuSubmitStats::default();
    };
    let mut stats = GpgpuSubmitStats::default();
    let mut span_params = Vec::with_capacity(COPY_RECT_BATCH_MAX_SPANS.min(32));
    let mut span_start = 0usize;
    while span_start < total_spans {
        let Some(span) =
            copy_rect_span_params(params, flavor.span_pixels, flavor.rows_per_walker, span_start)
        else {
            break;
        };
        span_params.push(span);
        if span_params.len() == COPY_RECT_BATCH_MAX_SPANS {
            if !submit_glyph_mask_span_params_batch(mask, dst, &span_params, color_rgba, flavor) {
                return stats;
            }
            stats.spans = stats.spans.saturating_add(span_params.len());
            stats.submits = stats.submits.saturating_add(1);
            span_params.clear();
        }
        span_start = span_start.saturating_add(1);
    }
    if !span_params.is_empty()
        && submit_glyph_mask_span_params_batch(mask, dst, &span_params, color_rgba, flavor)
    {
        stats.spans = stats.spans.saturating_add(span_params.len());
        stats.submits = stats.submits.saturating_add(1);
    }
    stats
}

fn submit_present_rgba8_to_primary_xrgb_spans_with_stats(
    src: GpgpuRgba8Surface,
    dst: GpgpuRgba8Surface,
    params: PresentRgba8ToPrimaryXrgbRectParams,
    flavor: CopyRectKernelFlavor,
) -> GpgpuSubmitStats {
    let Some(total_spans) =
        present_rect_span_count(params, flavor.span_pixels, flavor.rows_per_walker)
    else {
        return GpgpuSubmitStats::default();
    };
    let mut stats = GpgpuSubmitStats::default();
    let mut span_params = Vec::with_capacity(COPY_RECT_BATCH_MAX_SPANS.min(32));
    for span_index in 0..total_spans {
        let Some(span) = present_rect_span_params(
            params,
            flavor.span_pixels,
            flavor.rows_per_walker,
            span_index,
        ) else {
            return stats;
        };
        span_params.push(span);
        if span_params.len() == COPY_RECT_BATCH_MAX_SPANS {
            if !submit_present_rgba8_to_primary_xrgb_span_params_batch(
                src,
                dst,
                &span_params,
                flavor,
            ) {
                return stats;
            }
            stats.spans = stats.spans.saturating_add(span_params.len());
            stats.submits = stats.submits.saturating_add(1);
            span_params.clear();
        }
    }
    if !span_params.is_empty()
        && submit_present_rgba8_to_primary_xrgb_span_params_batch(src, dst, &span_params, flavor)
    {
        stats.spans = stats.spans.saturating_add(span_params.len());
        stats.submits = stats.submits.saturating_add(1);
    }
    stats
}

fn copy_rect_span_count(
    params: CopyRectRgba8Params,
    span_pixels: u32,
    rows_per_walker: u32,
) -> Option<usize> {
    if params.width == 0 || params.height == 0 {
        return None;
    }
    let spans_per_row = (params.width as usize).div_ceil(span_pixels as usize);
    let row_blocks = (params.height as usize).div_ceil(rows_per_walker as usize);
    spans_per_row.checked_mul(row_blocks)
}

fn present_rect_span_count(
    params: PresentRgba8ToPrimaryXrgbRectParams,
    span_pixels: u32,
    rows_per_walker: u32,
) -> Option<usize> {
    if params.width == 0 || params.height == 0 {
        return None;
    }
    let spans_per_row = (params.width as usize).div_ceil(span_pixels as usize);
    let row_blocks = (params.height as usize).div_ceil(rows_per_walker as usize);
    spans_per_row.checked_mul(row_blocks)
}

fn blit_rgba8_nearest_span_count(
    params: BlitRgba8NearestParams,
    span_pixels: u32,
    rows_per_walker: u32,
) -> Option<usize> {
    if params.src_width == 0
        || params.src_height == 0
        || params.dst_width == 0
        || params.dst_height == 0
    {
        return None;
    }
    let spans_per_row = (params.dst_width as usize).div_ceil(span_pixels as usize);
    let row_blocks = (params.dst_height as usize).div_ceil(rows_per_walker as usize);
    spans_per_row.checked_mul(row_blocks)
}

fn copy_rect_span_params(
    params: CopyRectRgba8Params,
    span_pixels: u32,
    rows_per_walker: u32,
    span_index: usize,
) -> Option<CopyRectRgba8Params> {
    let spans_per_row = (params.width as usize).div_ceil(span_pixels as usize);
    if spans_per_row == 0 {
        return None;
    }
    let row_block = span_index / spans_per_row;
    let row = row_block.saturating_mul(rows_per_walker as usize);
    if row >= params.height as usize {
        return None;
    }
    let span_col = span_index % spans_per_row;
    let x = (span_col as u32).saturating_mul(span_pixels);
    if x >= params.width {
        return None;
    }
    let span_width = (params.width - x).min(span_pixels);
    let span_height = (params.height - row as u32).min(rows_per_walker);
    Some(CopyRectRgba8Params {
        src_x: params.src_x + x,
        src_y: params.src_y + row as u32,
        dst_x: params.dst_x + x,
        dst_y: params.dst_y + row as u32,
        width: span_width,
        height: span_height,
        ..params
    })
}

fn present_rect_span_params(
    params: PresentRgba8ToPrimaryXrgbRectParams,
    span_pixels: u32,
    rows_per_walker: u32,
    span_index: usize,
) -> Option<PresentRgba8ToPrimaryXrgbRectParams> {
    let spans_per_row = (params.width as usize).div_ceil(span_pixels as usize);
    if spans_per_row == 0 {
        return None;
    }
    let row_block = span_index / spans_per_row;
    let row = row_block.saturating_mul(rows_per_walker as usize);
    if row >= params.height as usize {
        return None;
    }
    let span_col = span_index % spans_per_row;
    let x = (span_col as u32).saturating_mul(span_pixels);
    if x >= params.width {
        return None;
    }
    let span_width = (params.width - x).min(span_pixels);
    let span_height = (params.height - row as u32).min(rows_per_walker);
    let src_y = if params.flip_y != 0 {
        params.src_y + params.height - row as u32 - span_height
    } else {
        params.src_y + row as u32
    };
    Some(PresentRgba8ToPrimaryXrgbRectParams {
        src_x: params.src_x + x,
        src_y,
        dst_x: params.dst_x + x,
        dst_y: params.dst_y + row as u32,
        width: span_width,
        height: span_height,
        ..params
    })
}

fn blit_rgba8_nearest_span_params(
    params: BlitRgba8NearestParams,
    span_pixels: u32,
    rows_per_walker: u32,
    span_index: usize,
) -> Option<BlitRgba8NearestParams> {
    let spans_per_row = (params.dst_width as usize).div_ceil(span_pixels as usize);
    if spans_per_row == 0 {
        return None;
    }
    let row_block = span_index / spans_per_row;
    let row = row_block.saturating_mul(rows_per_walker as usize);
    if row >= params.dst_height as usize {
        return None;
    }
    let span_col = span_index % spans_per_row;
    let x = (span_col as u32).saturating_mul(span_pixels);
    if x >= params.dst_width {
        return None;
    }

    let span_width = (params.dst_width - x).min(span_pixels);
    let span_height = (params.dst_height - row as u32).min(rows_per_walker);
    let src_x_off = ((x as u64 * params.src_width as u64) / params.dst_width as u64) as u32;
    let src_y_off = ((row as u64 * params.src_height as u64) / params.dst_height as u64) as u32;
    let src_x_end =
        (((x as u64 + span_width as u64) * params.src_width as u64 + params.dst_width as u64 - 1)
            / params.dst_width as u64) as u32;
    let src_y_end = (((row as u64 + span_height as u64) * params.src_height as u64
        + params.dst_height as u64
        - 1)
        / params.dst_height as u64) as u32;
    Some(BlitRgba8NearestParams {
        src_x: params.src_x + src_x_off,
        src_y: params.src_y + src_y_off,
        src_width: src_x_end.saturating_sub(src_x_off).max(1),
        src_height: src_y_end.saturating_sub(src_y_off).max(1),
        dst_x: params.dst_x + x,
        dst_y: params.dst_y + row as u32,
        dst_width: span_width,
        dst_height: span_height,
        ..params
    })
}

fn submit_fill_rect_span(dst: GpgpuRgba8Surface, params: FillRectRgba8Params) -> bool {
    if params.width == 0 || params.width > 16 || params.height != 1 {
        return false;
    }
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
    let batch_ok = dst_ppgtt_ok
        && direct_rcs_encode_fill_rect_walker_batch(
            state,
            upload,
            params,
            dst.bytes,
            clear_rect_walker_right_mask(params.width),
        );
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result_slot(state, CLEAR_RECT_POST_MARKER_SLOT, CLEAR_RECT_POST_MARKER)
    } else {
        0
    };
    observed == CLEAR_RECT_POST_MARKER
}

fn submit_copy_rect_span_batch(
    src: GpgpuRgba8Surface,
    dst: GpgpuRgba8Surface,
    params: CopyRectRgba8Params,
    flavor: CopyRectKernelFlavor,
    span_start: usize,
    span_count: usize,
) -> bool {
    if span_count == 0 || span_count > COPY_RECT_BATCH_MAX_SPANS {
        return false;
    }
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        return false;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        return false;
    };

    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ppgtt_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(
            state,
            flavor.upload.gpu,
            flavor.upload.phys,
            flavor.upload.mapped_bytes,
        );
    let src_ppgtt_ok =
        kernel_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, src.gpu, src.phys, src.bytes);
    let dst_ppgtt_ok =
        src_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, dst.gpu, dst.phys, dst.bytes);
    let batch_ok = dst_ppgtt_ok
        && direct_rcs_encode_copy_rect_multi_walker_batch(
            state, flavor, params, src.bytes, dst.bytes, span_start, span_count,
        );
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result_slot(state, COPY_RECT_POST_MARKER_SLOT, COPY_RECT_POST_MARKER)
    } else {
        0
    };
    observed == COPY_RECT_POST_MARKER
}

fn submit_copy_rect_span_params_batch(
    src: GpgpuRgba8Surface,
    dst: GpgpuRgba8Surface,
    span_params: &[CopyRectRgba8Params],
    flavor: CopyRectKernelFlavor,
) -> bool {
    if span_params.is_empty() || span_params.len() > COPY_RECT_BATCH_MAX_SPANS {
        return false;
    }
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        return false;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        return false;
    };

    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ppgtt_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(
            state,
            flavor.upload.gpu,
            flavor.upload.phys,
            flavor.upload.mapped_bytes,
        );
    let src_ppgtt_ok =
        kernel_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, src.gpu, src.phys, src.bytes);
    let dst_ppgtt_ok =
        src_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, dst.gpu, dst.phys, dst.bytes);
    let batch_ok = dst_ppgtt_ok
        && direct_rcs_encode_copy_rect_span_params_batch(
            state,
            flavor,
            span_params,
            src.bytes,
            dst.bytes,
        );
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result_slot(state, COPY_RECT_POST_MARKER_SLOT, COPY_RECT_POST_MARKER)
    } else {
        0
    };
    observed == COPY_RECT_POST_MARKER
}

fn submit_blit_rgba8_nearest_span_params_batch(
    src: GpgpuRgba8Surface,
    dst: GpgpuRgba8Surface,
    span_params: &[BlitRgba8NearestParams],
    flavor: CopyRectKernelFlavor,
) -> bool {
    if span_params.is_empty() || span_params.len() > COPY_RECT_BATCH_MAX_SPANS {
        return false;
    }
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        return false;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        return false;
    };

    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ppgtt_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(
            state,
            flavor.upload.gpu,
            flavor.upload.phys,
            flavor.upload.mapped_bytes,
        );
    let src_ppgtt_ok =
        kernel_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, src.gpu, src.phys, src.bytes);
    let dst_ppgtt_ok =
        src_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, dst.gpu, dst.phys, dst.bytes);
    let batch_ok = dst_ppgtt_ok
        && direct_rcs_encode_blit_rgba8_nearest_span_params_batch(
            state,
            flavor,
            span_params,
            src.bytes,
            dst.bytes,
        );
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result_slot(state, COPY_RECT_POST_MARKER_SLOT, COPY_RECT_POST_MARKER)
    } else {
        0
    };
    observed == COPY_RECT_POST_MARKER
}

fn submit_glyph_mask_span_params_batch(
    mask: GpgpuMask8Surface,
    dst: GpgpuRgba8Surface,
    span_params: &[CopyRectRgba8Params],
    color_rgba: u32,
    flavor: CopyRectKernelFlavor,
) -> bool {
    if span_params.is_empty() || span_params.len() > COPY_RECT_BATCH_MAX_SPANS {
        return false;
    }
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        return false;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        return false;
    };

    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ppgtt_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(
            state,
            flavor.upload.gpu,
            flavor.upload.phys,
            flavor.upload.mapped_bytes,
        );
    let mask_ppgtt_ok =
        kernel_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, mask.gpu, mask.phys, mask.bytes);
    let dst_ppgtt_ok =
        mask_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, dst.gpu, dst.phys, dst.bytes);
    let batch_ok = dst_ppgtt_ok
        && direct_rcs_encode_glyph_mask_span_params_batch(
            state,
            flavor,
            span_params,
            color_rgba,
            mask.bytes,
            dst.bytes,
        );
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result_slot(state, COPY_RECT_POST_MARKER_SLOT, COPY_RECT_POST_MARKER)
    } else {
        0
    };
    observed == COPY_RECT_POST_MARKER
}

fn submit_present_rgba8_to_primary_xrgb_span_params_batch(
    src: GpgpuRgba8Surface,
    dst: GpgpuRgba8Surface,
    span_params: &[PresentRgba8ToPrimaryXrgbRectParams],
    flavor: CopyRectKernelFlavor,
) -> bool {
    if span_params.is_empty() || span_params.len() > COPY_RECT_BATCH_MAX_SPANS {
        return false;
    }
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        return false;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        return false;
    };

    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ppgtt_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(
            state,
            flavor.upload.gpu,
            flavor.upload.phys,
            flavor.upload.mapped_bytes,
        );
    let src_ppgtt_ok =
        kernel_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, src.gpu, src.phys, src.bytes);
    let dst_ppgtt_ok =
        src_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, dst.gpu, dst.phys, dst.bytes);
    let batch_ok = dst_ppgtt_ok
        && direct_rcs_encode_present_rgba8_to_primary_xrgb_span_params_batch(
            state,
            flavor,
            span_params,
            src.bytes,
            dst.bytes,
        );
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result_slot(state, COPY_RECT_POST_MARKER_SLOT, COPY_RECT_POST_MARKER)
    } else {
        0
    };
    observed == COPY_RECT_POST_MARKER
}

fn submit_sprite64_worklist(
    atlas: GpgpuRgba8Surface,
    dst: GpgpuRgba8Surface,
    desc: GpgpuSprite64WorklistDescBuffer,
    params: Sprite64WorklistRgba8Params,
) -> bool {
    if params.desc_count == 0 || params.desc_count as usize > SPRITE64_WORKLIST_MAX_DESCS {
        return false;
    }
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        return false;
    };
    let Some(upload) = upload_sprite64_worklist_rgba8_kernel() else {
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
    let atlas_ppgtt_ok =
        kernel_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, atlas.gpu, atlas.phys, atlas.bytes);
    let dst_ppgtt_ok =
        atlas_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, dst.gpu, dst.phys, dst.bytes);
    let desc_ppgtt_ok =
        dst_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, desc.gpu, desc.phys, desc.bytes);
    let batch_ok = desc_ppgtt_ok
        && direct_rcs_encode_sprite64_worklist_batch(
            state,
            upload,
            params,
            atlas.bytes,
            dst.bytes,
            desc.bytes,
        );
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result_slot(
            state,
            SPRITE64_WORKLIST_POST_MARKER_SLOT,
            SPRITE64_WORKLIST_POST_MARKER,
        )
    } else {
        0
    };
    observed == SPRITE64_WORKLIST_POST_MARKER
}

fn submit_fill_rect_worklist(
    dst: GpgpuRgba8Surface,
    desc: GpgpuRectWorklistDescBuffer,
    params: FillRectWorklistRgba8Params,
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
    let dst_ppgtt_ok =
        kernel_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, dst.gpu, dst.phys, dst.bytes);
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

fn submit_gradient_rect_worklist(
    dst: GpgpuRgba8Surface,
    desc: GpgpuRectWorklistDescBuffer,
    params: GradientRectWorklistRgba8Params,
) -> bool {
    if params.desc_count == 0 || params.desc_count as usize > RECT_WORKLIST_MAX_DESCS {
        return false;
    }
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        return false;
    };
    let Some(upload) = upload_gradient_rect_worklist_rgba8_kernel() else {
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
    let desc_ppgtt_ok =
        dst_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, desc.gpu, desc.phys, desc.bytes);
    let batch_ok = desc_ppgtt_ok
        && direct_rcs_encode_gradient_rect_worklist_batch(
            state, upload, params, dst.bytes, desc.bytes,
        );
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result_slot(
            state,
            RECT_WORKLIST_POST_MARKER_SLOT,
            GRADIENT_RECT_WORKLIST_POST_MARKER,
        )
    } else {
        0
    };
    observed == GRADIENT_RECT_WORKLIST_POST_MARKER
}

fn submit_mandel64_worklist(
    dst: GpgpuRgba8Surface,
    desc: GpgpuRectWorklistDescBuffer,
    params: Mandel64WorklistRgba8Params,
) -> bool {
    if params.desc_count == 0 || params.desc_count as usize > MANDEL64_WORKLIST_MAX_DESCS {
        return false;
    }
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        return false;
    };
    let Some(upload) = upload_mandel64_worklist_rgba8_kernel() else {
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
    let desc_ppgtt_ok =
        dst_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, desc.gpu, desc.phys, desc.bytes);
    let batch_ok = desc_ppgtt_ok
        && direct_rcs_encode_mandel64_worklist_batch(state, upload, params, dst.bytes, desc.bytes);
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result_slot(
            state,
            RECT_WORKLIST_POST_MARKER_SLOT,
            MANDEL64_WORKLIST_POST_MARKER,
        )
    } else {
        0
    };
    observed == MANDEL64_WORKLIST_POST_MARKER
}

fn submit_alpha_blend_worklist(
    src: GpgpuRgba8Surface,
    dst: GpgpuRgba8Surface,
    desc: GpgpuRectWorklistDescBuffer,
    params: AlphaBlendWorklistRgba8Params,
) -> bool {
    if params.desc_count == 0 || params.desc_count as usize > RECT_WORKLIST_MAX_DESCS {
        return false;
    }
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        return false;
    };
    let Some(upload) = upload_alpha_blend_worklist_rgba8_kernel() else {
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
        && direct_rcs_encode_alpha_blend_worklist_batch(
            state, upload, params, src.bytes, dst.bytes, desc.bytes,
        );
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result_slot(
            state,
            RECT_WORKLIST_POST_MARKER_SLOT,
            ALPHA_BLEND_WORKLIST_POST_MARKER,
        )
    } else {
        0
    };
    observed == ALPHA_BLEND_WORKLIST_POST_MARKER
}

fn sprite64_worklist_walker_count(desc_count: usize) -> usize {
    desc_count
        .div_ceil(SPRITE64_WORKLIST_DESCS_PER_WALKER)
        .min(SPRITE64_WORKLIST_MAX_WALKERS)
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

fn clear_rect_walker_right_mask(width: u32) -> u32 {
    simd16_right_mask(width)
}

fn copy_rect_walker_right_mask(width: u32) -> u32 {
    copy_rect_walker_right_mask_for(width, COPY_RECT_PIXELS_PER_LANE)
}

fn copy_rect_walker_right_mask_for(width: u32, pixels_per_lane: u32) -> u32 {
    let lanes = width.div_ceil(pixels_per_lane);
    simd16_right_mask(lanes)
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
    let mapped_bytes = align_up(artifact.bin.len(), super::WARM_ALIGN)?;
    let (phys, virt) = crate::dma::alloc(mapped_bytes, super::WARM_ALIGN)?;
    unsafe {
        core::ptr::write_bytes(virt, 0, mapped_bytes);
        core::ptr::copy_nonoverlapping(artifact.bin.as_ptr(), virt, artifact.bin.len());
    }
    super::dma_flush(virt, mapped_bytes);

    let uploaded = unsafe { core::slice::from_raw_parts(virt, artifact.bin.len()) };
    let verified = uploaded == artifact.bin;
    if !verified {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: {} upload failed reason=verify phys=0x{:X} gpu=0x{:X} bytes=0x{:X}\n",
            artifact.name,
            phys,
            gpu,
            artifact.bin.len()
        );
        crate::dma::dealloc(virt, mapped_bytes);
        return None;
    }

    if !super::map_ggtt(dev, phys, mapped_bytes, gpu) {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: {} upload failed reason=ggtt-map phys=0x{:X} gpu=0x{:X} bytes=0x{:X}\n",
            artifact.name,
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
        gpu,
        phys,
        virt,
        bytes: artifact.bin.len(),
        mapped_bytes,
        verified,
    };
    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: {} upload ok=1 target={} phys=0x{:X} gpu=0x{:X} bytes=0x{:X} mapped=0x{:X} sha256={:02X}{:02X}{:02X}{:02X}...\n",
        artifact.name,
        artifact.target,
        upload.phys,
        upload.gpu,
        upload.bytes,
        upload.mapped_bytes,
        artifact.bin_sha256[0],
        artifact.bin_sha256[1],
        artifact.bin_sha256[2],
        artifact.bin_sha256[3],
    );
    Some(upload)
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
    canvas3d_tmp_virt: *mut u8,
    ppgtt_phys: u64,
    ppgtt_virt: *mut u8,
}

unsafe impl Send for DirectRcsState {}
unsafe impl Sync for DirectRcsState {}

fn direct_rcs_state_once(_dev: super::Dev) -> Option<DirectRcsState> {
    if let Some(state) = *DIRECT_RCS_STATE.lock() {
        return Some(state);
    }

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
        canvas3d_tmp_virt,
        ppgtt_phys,
        ppgtt_virt,
    };
    *DIRECT_RCS_STATE.lock() = Some(state);
    Some(state)
}

fn direct_rcs_map_state(dev: super::Dev, state: DirectRcsState) -> bool {
    let mapped =
        super::map_ggtt(dev, state.ring_phys, DIRECT_RCS_RING_BYTES, DIRECT_RCS_GPU_VA_RING_BASE)
            && super::map_ggtt(
                dev,
                state.context_phys,
                DIRECT_RCS_CONTEXT_BYTES,
                DIRECT_RCS_GPU_VA_CONTEXT_BASE,
            )
            && super::map_ggtt(
                dev,
                state.batch_phys,
                DIRECT_RCS_BATCH_BYTES,
                DIRECT_RCS_GPU_VA_BATCH_BASE,
            )
            && super::map_ggtt(
                dev,
                state.result_phys,
                DIRECT_RCS_RESULT_BYTES,
                DIRECT_RCS_GPU_VA_RESULT_BASE,
            )
            && super::map_ggtt(
                dev,
                state.clear_test_phys,
                CLEAR_RECT_TEST_BYTES,
                DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE,
            )
            && super::map_ggtt(
                dev,
                state.canvas3d_out_phys,
                CANVAS3D_PROJECT_OUT_ALLOC_BYTES,
                DIRECT_RCS_GPU_VA_CANVAS3D_OUT_BASE,
            )
            && super::map_ggtt(
                dev,
                state.canvas3d_tmp_phys,
                CANVAS3D_PROJECT_OUT_ALLOC_BYTES,
                DIRECT_RCS_GPU_VA_CANVAS3D_TMP_BASE,
            );
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
        DIRECT_RCS_GPU_VA_RING_BASE,
        state.ring_phys,
        DIRECT_RCS_RING_BYTES,
        pte_present_rw,
    ) && direct_rcs_map_ppgtt_region(
        state,
        DIRECT_RCS_GPU_VA_CONTEXT_BASE,
        state.context_phys,
        DIRECT_RCS_CONTEXT_BYTES,
        pte_present_rw,
    ) && direct_rcs_map_ppgtt_region(
        state,
        DIRECT_RCS_GPU_VA_BATCH_BASE,
        state.batch_phys,
        DIRECT_RCS_BATCH_BYTES,
        pte_present_rw,
    ) && direct_rcs_map_ppgtt_region(
        state,
        DIRECT_RCS_GPU_VA_RESULT_BASE,
        state.result_phys,
        DIRECT_RCS_RESULT_BYTES,
        pte_present_rw,
    );

    super::dma_flush(state.ppgtt_virt, DIRECT_RCS_PPGTT_BYTES);
    ok
}

fn direct_rcs_map_ppgtt_kernel(state: DirectRcsState, gpu: u64, phys: u64, len: usize) -> bool {
    let ok = direct_rcs_map_ppgtt_region(state, gpu, phys, len, direct_rcs_ppgtt_pte_flags());
    super::dma_flush(state.ppgtt_virt, DIRECT_RCS_PPGTT_BYTES);
    ok
}

fn direct_rcs_ppgtt_pte_flags() -> u64 {
    super::GEN8_PAGE_PRESENT | GEN8_PAGE_RW
}

fn direct_rcs_map_ppgtt_region(
    state: DirectRcsState,
    gpu: u64,
    phys: u64,
    len: usize,
    entry_flags: u64,
) -> bool {
    let pt_off = 12288usize;
    for page in 0..len.div_ceil(4096) {
        let va_page = (gpu >> 12) + page as u64;
        let pd_index = ((va_page >> 9) & 0x1FF) as usize;
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

fn direct_rcs_encode_smoke_batch(state: DirectRcsState) -> bool {
    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
        let result = state.result_virt as *mut u32;
        core::ptr::write_volatile(result, 0);

        let batch = state.batch_virt as *mut u32;
        core::ptr::write_volatile(batch, MI_STORE_DATA_IMM_GGTT_DW1);
        core::ptr::write_volatile(batch.add(1), DIRECT_RCS_GPU_VA_RESULT_BASE as u32);
        core::ptr::write_volatile(batch.add(2), (DIRECT_RCS_GPU_VA_RESULT_BASE >> 32) as u32);
        core::ptr::write_volatile(batch.add(3), DIRECT_RCS_SMOKE_MARKER);
        core::ptr::write_volatile(batch.add(4), MI_BATCH_BUFFER_END);
        core::ptr::write_volatile(batch.add(5), MI_NOOP);
    }
    super::dma_flush(state.batch_virt, 6 * core::mem::size_of::<u32>());
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    true
}

fn direct_rcs_encode_copy_rect_walker_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: CopyRectRgba8Params,
    src_bytes: usize,
    dst_bytes: usize,
    right_mask: u32,
) -> bool {
    if COPY_RECT_PAYLOAD_OFFSET_BYTES + COPY_RECT_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }

    if !direct_rcs_write_copy_rect_interface_descriptor(state) {
        return false;
    }
    if !direct_rcs_write_copy_rect_surface_states(
        state,
        params.src_gpu,
        src_bytes,
        params.dst_gpu,
        dst_bytes,
    ) {
        return false;
    }
    if !direct_rcs_write_copy_rect_payload(state, params) {
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
    ok &= direct_rcs_push(batch, &mut cursor, COPY_RECT_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, COPY_RECT_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        COPY_RECT_PRE_MARKER_SLOT,
        COPY_RECT_PRE_MARKER,
    );
    ok &= direct_rcs_push(batch, &mut cursor, GPGPU_WALKER_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, COPY_RECT_INDIRECT_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, COPY_RECT_PAYLOAD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push(
        batch,
        &mut cursor,
        (GPGPU_WALKER_SIMD16_SELECT << 30) | (GPGPU_WALKER_GROUP_THREADS - 1),
    );
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 1);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 1);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, GPGPU_WALKER_GROUP_Z_DIM);
    ok &= direct_rcs_push(batch, &mut cursor, right_mask);
    ok &= direct_rcs_push(batch, &mut cursor, GPGPU_WALKER_BOTTOM_MASK);
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

fn direct_rcs_encode_copy_rect_multi_walker_batch(
    state: DirectRcsState,
    flavor: CopyRectKernelFlavor,
    params: CopyRectRgba8Params,
    src_bytes: usize,
    dst_bytes: usize,
    span_start: usize,
    span_count: usize,
) -> bool {
    if span_count == 0 || span_count > COPY_RECT_BATCH_MAX_SPANS {
        return false;
    }
    let payload_end =
        COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES + span_count * COPY_RECT_INDIRECT_BYTES;
    if payload_end > DIRECT_RCS_BATCH_BYTES {
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
        flavor.text_offset_bytes,
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
    for span in 0..span_count {
        let Some(span_params) = copy_rect_span_params(
            params,
            flavor.span_pixels,
            flavor.rows_per_walker,
            span_start + span,
        ) else {
            return false;
        };
        let payload_offset =
            COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES + span * COPY_RECT_INDIRECT_BYTES;
        if !direct_rcs_write_copy_rect_payload_at(state, payload_offset, span_params) {
            return false;
        }
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
        flavor.upload.gpu,
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
    for span in 0..span_count {
        let Some(span_params) = copy_rect_span_params(
            params,
            flavor.span_pixels,
            flavor.rows_per_walker,
            span_start + span,
        ) else {
            return false;
        };
        let payload_offset =
            COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES + span * COPY_RECT_INDIRECT_BYTES;
        ok &= direct_rcs_push_copy_rect_walker(
            batch,
            &mut cursor,
            payload_offset,
            copy_rect_walker_right_mask_for(span_params.width, flavor.pixels_per_lane),
        );
        ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
        ok &= direct_rcs_push(batch, &mut cursor, 0);
    }
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

fn direct_rcs_encode_copy_rect_span_params_batch(
    state: DirectRcsState,
    flavor: CopyRectKernelFlavor,
    span_params: &[CopyRectRgba8Params],
    src_bytes: usize,
    dst_bytes: usize,
) -> bool {
    if span_params.is_empty() || span_params.len() > COPY_RECT_BATCH_MAX_SPANS {
        return false;
    }
    let payload_end =
        COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES + span_params.len() * COPY_RECT_INDIRECT_BYTES;
    if payload_end > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }

    let first = span_params[0];
    if !direct_rcs_write_copy_rect_interface_descriptor_at(
        state,
        COPY_RECT_BATCH_IDD_OFFSET_BYTES,
        COPY_RECT_BATCH_BINDING_TABLE_OFFSET_BYTES,
        flavor.text_offset_bytes,
    ) {
        return false;
    }
    if !direct_rcs_write_copy_rect_surface_states_at(
        state,
        COPY_RECT_BATCH_BINDING_TABLE_OFFSET_BYTES,
        COPY_RECT_BATCH_SRC_SURFACE_STATE_OFFSET_BYTES,
        COPY_RECT_BATCH_DST_SURFACE_STATE_OFFSET_BYTES,
        first.src_gpu,
        src_bytes,
        first.dst_gpu,
        dst_bytes,
    ) {
        return false;
    }
    for (span, params) in span_params.iter().copied().enumerate() {
        let payload_offset =
            COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES + span * COPY_RECT_INDIRECT_BYTES;
        if !direct_rcs_write_copy_rect_payload_at(state, payload_offset, params) {
            return false;
        }
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
        flavor.upload.gpu,
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
    for (span, params) in span_params.iter().copied().enumerate() {
        let payload_offset =
            COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES + span * COPY_RECT_INDIRECT_BYTES;
        ok &= direct_rcs_push_copy_rect_walker(
            batch,
            &mut cursor,
            payload_offset,
            copy_rect_walker_right_mask_for(params.width, flavor.pixels_per_lane),
        );
        ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
        ok &= direct_rcs_push(batch, &mut cursor, 0);
    }
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

fn direct_rcs_encode_blit_rgba8_nearest_span_params_batch(
    state: DirectRcsState,
    flavor: CopyRectKernelFlavor,
    span_params: &[BlitRgba8NearestParams],
    src_bytes: usize,
    dst_bytes: usize,
) -> bool {
    if span_params.is_empty() || span_params.len() > COPY_RECT_BATCH_MAX_SPANS {
        return false;
    }
    let payload_end =
        COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES + span_params.len() * COPY_RECT_INDIRECT_BYTES;
    if payload_end > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }

    let first = span_params[0];
    if !direct_rcs_write_copy_rect_interface_descriptor_at(
        state,
        COPY_RECT_BATCH_IDD_OFFSET_BYTES,
        COPY_RECT_BATCH_BINDING_TABLE_OFFSET_BYTES,
        flavor.text_offset_bytes,
    ) {
        return false;
    }
    if !direct_rcs_write_copy_rect_surface_states_at(
        state,
        COPY_RECT_BATCH_BINDING_TABLE_OFFSET_BYTES,
        COPY_RECT_BATCH_SRC_SURFACE_STATE_OFFSET_BYTES,
        COPY_RECT_BATCH_DST_SURFACE_STATE_OFFSET_BYTES,
        first.src_gpu,
        src_bytes,
        first.dst_gpu,
        dst_bytes,
    ) {
        return false;
    }
    for (span, params) in span_params.iter().copied().enumerate() {
        let payload_offset =
            COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES + span * COPY_RECT_INDIRECT_BYTES;
        if !direct_rcs_write_blit_rgba8_nearest_payload_at(state, payload_offset, params) {
            return false;
        }
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
        flavor.upload.gpu,
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
    for (span, params) in span_params.iter().copied().enumerate() {
        let payload_offset =
            COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES + span * COPY_RECT_INDIRECT_BYTES;
        ok &= direct_rcs_push_copy_rect_walker(
            batch,
            &mut cursor,
            payload_offset,
            copy_rect_walker_right_mask_for(params.dst_width, flavor.pixels_per_lane),
        );
        ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
        ok &= direct_rcs_push(batch, &mut cursor, 0);
    }
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

fn direct_rcs_encode_glyph_mask_span_params_batch(
    state: DirectRcsState,
    flavor: CopyRectKernelFlavor,
    span_params: &[CopyRectRgba8Params],
    color_rgba: u32,
    mask_bytes: usize,
    dst_bytes: usize,
) -> bool {
    if span_params.is_empty() || span_params.len() > COPY_RECT_BATCH_MAX_SPANS {
        return false;
    }
    let payload_end =
        COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES + span_params.len() * GLYPH_MASK_INDIRECT_BYTES;
    if payload_end > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }

    let first = span_params[0];
    if !direct_rcs_write_copy_rect_interface_descriptor_at_with_cross_thread_grfs(
        state,
        COPY_RECT_BATCH_IDD_OFFSET_BYTES,
        COPY_RECT_BATCH_BINDING_TABLE_OFFSET_BYTES,
        flavor.text_offset_bytes,
        4,
    ) {
        return false;
    }
    if !direct_rcs_write_copy_rect_surface_states_at(
        state,
        COPY_RECT_BATCH_BINDING_TABLE_OFFSET_BYTES,
        COPY_RECT_BATCH_SRC_SURFACE_STATE_OFFSET_BYTES,
        COPY_RECT_BATCH_DST_SURFACE_STATE_OFFSET_BYTES,
        first.src_gpu,
        mask_bytes,
        first.dst_gpu,
        dst_bytes,
    ) {
        return false;
    }
    for (span, params) in span_params.iter().copied().enumerate() {
        let payload_offset =
            COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES + span * GLYPH_MASK_INDIRECT_BYTES;
        if !direct_rcs_write_glyph_mask_payload_at(state, payload_offset, params, color_rgba) {
            return false;
        }
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
        flavor.upload.gpu,
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
    for (span, params) in span_params.iter().copied().enumerate() {
        let payload_offset =
            COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES + span * GLYPH_MASK_INDIRECT_BYTES;
        ok &= direct_rcs_push_glyph_mask_walker(
            batch,
            &mut cursor,
            payload_offset,
            copy_rect_walker_right_mask_for(params.width, flavor.pixels_per_lane),
        );
        ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
        ok &= direct_rcs_push(batch, &mut cursor, 0);
    }
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

fn direct_rcs_encode_sprite64_worklist_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: Sprite64WorklistRgba8Params,
    atlas_bytes: usize,
    dst_bytes: usize,
    desc_bytes: usize,
) -> bool {
    let desc_count = params.desc_count as usize;
    let walker_count = sprite64_worklist_walker_count(desc_count);
    if desc_count == 0 || walker_count == 0 {
        return false;
    }
    let payload_end =
        SPRITE64_WORKLIST_PAYLOAD_OFFSET_BYTES + walker_count * SPRITE64_WORKLIST_INDIRECT_BYTES;
    if payload_end > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }

    if !direct_rcs_write_sprite64_worklist_interface_descriptor(state) {
        return false;
    }
    if !direct_rcs_write_sprite64_worklist_surface_states(
        state,
        params.atlas_gpu,
        atlas_bytes,
        params.dst_gpu,
        dst_bytes,
        params.desc_gpu,
        desc_bytes,
    ) {
        return false;
    }
    for walker in 0..walker_count {
        let desc_base = walker.saturating_mul(SPRITE64_WORKLIST_DESCS_PER_WALKER);
        let local_count = desc_count
            .saturating_sub(desc_base)
            .min(SPRITE64_WORKLIST_DESCS_PER_WALKER);
        let payload_offset =
            SPRITE64_WORKLIST_PAYLOAD_OFFSET_BYTES + walker * SPRITE64_WORKLIST_INDIRECT_BYTES;
        let payload_params = Sprite64WorklistRgba8Params {
            desc_base: params.desc_base.saturating_add(desc_base as u32),
            desc_count: local_count as u32,
            ..params
        };
        if !direct_rcs_write_sprite64_worklist_payload_at(state, payload_offset, payload_params) {
            return false;
        }
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
    ok &= direct_rcs_push(batch, &mut cursor, SPRITE64_WORKLIST_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, SPRITE64_WORKLIST_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        SPRITE64_WORKLIST_PRE_MARKER_SLOT,
        SPRITE64_WORKLIST_PRE_MARKER,
    );
    for walker in 0..walker_count {
        let desc_base = walker.saturating_mul(SPRITE64_WORKLIST_DESCS_PER_WALKER);
        let local_count = desc_count
            .saturating_sub(desc_base)
            .min(SPRITE64_WORKLIST_DESCS_PER_WALKER);
        let payload_offset =
            SPRITE64_WORKLIST_PAYLOAD_OFFSET_BYTES + walker * SPRITE64_WORKLIST_INDIRECT_BYTES;
        ok &= direct_rcs_push_sprite64_worklist_walker(
            batch,
            &mut cursor,
            payload_offset,
            simd16_right_mask(local_count as u32),
        );
    }
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_FLUSH_BITS);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        SPRITE64_WORKLIST_POST_MARKER_SLOT,
        SPRITE64_WORKLIST_POST_MARKER,
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
    )
}

fn direct_rcs_encode_gradient_rect_worklist_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: GradientRectWorklistRgba8Params,
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

    if !direct_rcs_write_gradient_rect_worklist_interface_descriptor(state) {
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
        let payload_params = GradientRectWorklistRgba8Params {
            desc_base: params.desc_base.saturating_add(desc_base as u32),
            desc_count: local_count as u32,
            ..params
        };
        if !direct_rcs_write_gradient_rect_worklist_payload_at(
            state,
            payload_offset,
            payload_params,
        ) {
            return false;
        }
    }

    direct_rcs_encode_rect_worklist_command_stream(
        state,
        upload,
        walker_count,
        desc_count,
        GRADIENT_RECT_WORKLIST_PRE_MARKER,
        GRADIENT_RECT_WORKLIST_POST_MARKER,
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
    )
}

fn direct_rcs_encode_alpha_blend_worklist_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: AlphaBlendWorklistRgba8Params,
    src_bytes: usize,
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

    if !direct_rcs_write_alpha_blend_worklist_interface_descriptor(state) {
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
    for walker in 0..walker_count {
        let desc_base = walker.saturating_mul(RECT_WORKLIST_DESCS_PER_WALKER);
        let local_count = desc_count
            .saturating_sub(desc_base)
            .min(RECT_WORKLIST_DESCS_PER_WALKER);
        let payload_offset =
            RECT_WORKLIST_PAYLOAD_OFFSET_BYTES + walker * RECT_WORKLIST_INDIRECT_BYTES;
        let payload_params = AlphaBlendWorklistRgba8Params {
            desc_base: params.desc_base.saturating_add(desc_base as u32),
            desc_count: local_count as u32,
            ..params
        };
        if !direct_rcs_write_alpha_blend_worklist_payload_at(state, payload_offset, payload_params)
        {
            return false;
        }
    }

    direct_rcs_encode_rect_worklist_command_stream(
        state,
        upload,
        walker_count,
        desc_count,
        ALPHA_BLEND_WORKLIST_PRE_MARKER,
        ALPHA_BLEND_WORKLIST_POST_MARKER,
    )
}

fn direct_rcs_encode_rect_worklist_command_stream(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    walker_count: usize,
    desc_count: usize,
    pre_marker: u32,
    post_marker: u32,
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
            simd16_right_mask(local_count as u32),
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

fn direct_rcs_encode_canvas3d_project_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: Canvas3dProjectRgba8Params,
    vertices_bytes: usize,
    out_bytes: usize,
) -> bool {
    if params.vertex_count == 0
        || CANVAS3D_PROJECT_PAYLOAD_OFFSET_BYTES + CANVAS3D_PROJECT_INDIRECT_BYTES
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
        CANVAS3D_PROJECT_IDD_OFFSET_BYTES,
        CANVAS3D_PROJECT_BINDING_TABLE_OFFSET_BYTES,
        CANVAS3D_PROJECT_RGBA8_TEXT_OFFSET_BYTES,
    ) {
        return false;
    }
    if !direct_rcs_write_copy_rect_surface_states_at(
        state,
        CANVAS3D_PROJECT_BINDING_TABLE_OFFSET_BYTES,
        CANVAS3D_PROJECT_VERTICES_SURFACE_STATE_OFFSET_BYTES,
        CANVAS3D_PROJECT_OUT_SURFACE_STATE_OFFSET_BYTES,
        params.vertices_gpu,
        vertices_bytes,
        params.out_gpu,
        out_bytes,
    ) {
        return false;
    }
    if !direct_rcs_write_canvas3d_project_payload(state, params) {
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
    ok &= direct_rcs_push(batch, &mut cursor, CANVAS3D_PROJECT_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, CANVAS3D_PROJECT_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        CANVAS3D_PROJECT_PRE_MARKER_SLOT,
        CANVAS3D_PROJECT_PRE_MARKER,
    );
    ok &= direct_rcs_push_canvas3d_project_walker(batch, &mut cursor);
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_FLUSH_BITS);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        CANVAS3D_PROJECT_POST_MARKER_SLOT,
        CANVAS3D_PROJECT_POST_MARKER,
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

fn direct_rcs_encode_canvas3d_transform_fused_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: Canvas3dTransformFusedQ16Params,
    pre_marker_value: u32,
    post_marker_value: u32,
    src_bytes: usize,
    dst_bytes: usize,
) -> bool {
    if params.vertex_count == 0
        || CANVAS3D_TRANSFORM_PAYLOAD_OFFSET_BYTES + CANVAS3D_TRANSFORM_FUSED_INDIRECT_BYTES
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
        CANVAS3D_TRANSFORM_IDD_OFFSET_BYTES,
        CANVAS3D_TRANSFORM_BINDING_TABLE_OFFSET_BYTES,
        CANVAS3D_TRANSFORM_Q16_TEXT_OFFSET_BYTES,
        4,
    ) {
        return false;
    }
    if !direct_rcs_write_copy_rect_surface_states_at(
        state,
        CANVAS3D_TRANSFORM_BINDING_TABLE_OFFSET_BYTES,
        CANVAS3D_TRANSFORM_SRC_SURFACE_STATE_OFFSET_BYTES,
        CANVAS3D_TRANSFORM_DST_SURFACE_STATE_OFFSET_BYTES,
        params.src_gpu,
        src_bytes,
        params.dst_gpu,
        dst_bytes,
    ) {
        return false;
    }
    if !direct_rcs_write_canvas3d_transform_fused_payload(state, params) {
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
    ok &= direct_rcs_push(batch, &mut cursor, CANVAS3D_TRANSFORM_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, CANVAS3D_TRANSFORM_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        CANVAS3D_TRANSFORM_PRE_MARKER_SLOT,
        pre_marker_value,
    );
    ok &= direct_rcs_push_canvas3d_transform_fused_walker(batch, &mut cursor);
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_FLUSH_BITS);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        CANVAS3D_TRANSFORM_POST_MARKER_SLOT,
        post_marker_value,
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

fn direct_rcs_encode_canvas3d_clip_box_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: Canvas3dClipBoxQ16Params,
    src_bytes: usize,
    dst_bytes: usize,
) -> bool {
    if params.vertex_count == 0
        || CANVAS3D_CLIP_BOX_PAYLOAD_OFFSET_BYTES + CANVAS3D_CLIP_BOX_INDIRECT_BYTES
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
        CANVAS3D_CLIP_BOX_IDD_OFFSET_BYTES,
        CANVAS3D_CLIP_BOX_BINDING_TABLE_OFFSET_BYTES,
        CANVAS3D_CLIP_BOX_Q16_TEXT_OFFSET_BYTES,
        4,
    ) {
        return false;
    }
    if !direct_rcs_write_copy_rect_surface_states_at(
        state,
        CANVAS3D_CLIP_BOX_BINDING_TABLE_OFFSET_BYTES,
        CANVAS3D_CLIP_BOX_SRC_SURFACE_STATE_OFFSET_BYTES,
        CANVAS3D_CLIP_BOX_DST_SURFACE_STATE_OFFSET_BYTES,
        params.src_gpu,
        src_bytes,
        params.dst_gpu,
        dst_bytes,
    ) {
        return false;
    }
    if !direct_rcs_write_canvas3d_clip_box_payload(state, params) {
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
    ok &= direct_rcs_push(batch, &mut cursor, CANVAS3D_CLIP_BOX_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, CANVAS3D_CLIP_BOX_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        CANVAS3D_CLIP_BOX_PRE_MARKER_SLOT,
        CANVAS3D_CLIP_BOX_PRE_MARKER,
    );
    ok &= direct_rcs_push_canvas3d_clip_box_walker(batch, &mut cursor);
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_FLUSH_BITS);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        CANVAS3D_CLIP_BOX_POST_MARKER_SLOT,
        CANVAS3D_CLIP_BOX_POST_MARKER,
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

fn direct_rcs_encode_canvas3d_plane_sample_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: Canvas3dPlaneSampleRgba8Params,
    out_bytes: usize,
) -> bool {
    if params.sample_count == 0
        || CANVAS3D_PLANE_SAMPLE_PAYLOAD_OFFSET_BYTES + CANVAS3D_PLANE_SAMPLE_INDIRECT_BYTES
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
        CANVAS3D_PLANE_SAMPLE_IDD_OFFSET_BYTES,
        CANVAS3D_PLANE_SAMPLE_BINDING_TABLE_OFFSET_BYTES,
        CANVAS3D_PLANE_SAMPLE_RGBA8_TEXT_OFFSET_BYTES,
        7,
    ) {
        return false;
    }
    if !direct_rcs_write_copy_rect_surface_states_at(
        state,
        CANVAS3D_PLANE_SAMPLE_BINDING_TABLE_OFFSET_BYTES,
        CANVAS3D_PLANE_SAMPLE_OUT_SURFACE_STATE_OFFSET_BYTES,
        CANVAS3D_PLANE_SAMPLE_SCRATCH_SURFACE_STATE_OFFSET_BYTES,
        params.out_gpu,
        out_bytes,
        params.out_gpu,
        out_bytes,
    ) {
        return false;
    }
    if !direct_rcs_write_canvas3d_plane_sample_payload(state, params) {
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
    ok &= direct_rcs_push(batch, &mut cursor, CANVAS3D_PLANE_SAMPLE_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, CANVAS3D_PLANE_SAMPLE_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        CANVAS3D_PLANE_SAMPLE_PRE_MARKER_SLOT,
        CANVAS3D_PLANE_SAMPLE_PRE_MARKER,
    );
    ok &= direct_rcs_push_canvas3d_plane_sample_walker(batch, &mut cursor);
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_FLUSH_BITS);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        CANVAS3D_PLANE_SAMPLE_POST_MARKER_SLOT,
        CANVAS3D_PLANE_SAMPLE_POST_MARKER,
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

fn direct_rcs_encode_canvas3d_plane_fill_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: Canvas3dPlaneFillRgba8Params,
    dst_bytes: usize,
) -> bool {
    if params.rect_width == 0
        || params.rect_height == 0
        || CANVAS3D_PLANE_FILL_PAYLOAD_OFFSET_BYTES + CANVAS3D_PLANE_FILL_INDIRECT_BYTES
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
        CANVAS3D_PLANE_FILL_IDD_OFFSET_BYTES,
        CANVAS3D_PLANE_FILL_BINDING_TABLE_OFFSET_BYTES,
        CANVAS3D_PLANE_FILL_RGBA8_TEXT_OFFSET_BYTES,
        8,
    ) {
        return false;
    }
    if !direct_rcs_write_copy_rect_surface_states_at(
        state,
        CANVAS3D_PLANE_FILL_BINDING_TABLE_OFFSET_BYTES,
        CANVAS3D_PLANE_FILL_DST_SURFACE_STATE_OFFSET_BYTES,
        CANVAS3D_PLANE_FILL_SCRATCH_SURFACE_STATE_OFFSET_BYTES,
        params.dst_gpu,
        dst_bytes,
        params.dst_gpu,
        dst_bytes,
    ) {
        return false;
    }
    if !direct_rcs_write_canvas3d_plane_fill_payload(state, params) {
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
    ok &= direct_rcs_push(batch, &mut cursor, CANVAS3D_PLANE_FILL_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, CANVAS3D_PLANE_FILL_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        CANVAS3D_PLANE_FILL_PRE_MARKER_SLOT,
        CANVAS3D_PLANE_FILL_PRE_MARKER,
    );
    ok &= direct_rcs_push_canvas3d_plane_fill_walker(batch, &mut cursor);
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_FLUSH_BITS);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        CANVAS3D_PLANE_FILL_POST_MARKER_SLOT,
        CANVAS3D_PLANE_FILL_POST_MARKER,
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

fn direct_rcs_encode_canvas3d_plane_patch_fill_cut_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: Canvas3dPlaneFillRgba8Params,
    dst_bytes: usize,
) -> bool {
    if params.rect_width == 0
        || params.rect_height == 0
        || CANVAS3D_PLANE_FILL_PAYLOAD_OFFSET_BYTES + CANVAS3D_PLANE_FILL_INDIRECT_BYTES
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
        CANVAS3D_PLANE_FILL_IDD_OFFSET_BYTES,
        CANVAS3D_PLANE_FILL_BINDING_TABLE_OFFSET_BYTES,
        CANVAS3D_PLANE_PATCH_FILL_CUT_RGBA8_TEXT_OFFSET_BYTES,
        8,
    ) {
        return false;
    }
    if !direct_rcs_write_copy_rect_surface_states_at(
        state,
        CANVAS3D_PLANE_FILL_BINDING_TABLE_OFFSET_BYTES,
        CANVAS3D_PLANE_FILL_DST_SURFACE_STATE_OFFSET_BYTES,
        CANVAS3D_PLANE_FILL_SCRATCH_SURFACE_STATE_OFFSET_BYTES,
        params.dst_gpu,
        dst_bytes,
        params.dst_gpu,
        dst_bytes,
    ) {
        return false;
    }
    if !direct_rcs_write_canvas3d_plane_fill_payload(state, params) {
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
    ok &= direct_rcs_push(batch, &mut cursor, CANVAS3D_PLANE_FILL_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, CANVAS3D_PLANE_FILL_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        CANVAS3D_PLANE_PATCH_FILL_CUT_PRE_MARKER_SLOT,
        CANVAS3D_PLANE_PATCH_FILL_CUT_PRE_MARKER,
    );
    ok &= direct_rcs_push_canvas3d_plane_fill_walker(batch, &mut cursor);
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_FLUSH_BITS);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        CANVAS3D_PLANE_PATCH_FILL_CUT_POST_MARKER_SLOT,
        CANVAS3D_PLANE_PATCH_FILL_CUT_POST_MARKER,
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

fn direct_rcs_encode_canvas3d_plane_patch_worklist_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: Canvas3dPlanePatchWorklistRgba8Params,
    dst_bytes: usize,
    desc_bytes: usize,
) -> bool {
    let walker_count = params.group_x as usize;
    let payload_end = CANVAS3D_PLANE_FILL_PAYLOAD_OFFSET_BYTES
        .saturating_add(walker_count.saturating_mul(CANVAS3D_PLANE_PATCH_WORKLIST_INDIRECT_BYTES));
    if params.desc_count == 0
        || params.desc_count as usize > CANVAS3D_PLANE_PATCH_WORKLIST_MAX_DESCS
        || walker_count == 0
        || payload_end > DIRECT_RCS_BATCH_BYTES
    {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }

    if !direct_rcs_write_canvas3d_plane_patch_worklist_interface_descriptor(state) {
        return false;
    }
    if !direct_rcs_write_canvas3d_plane_patch_worklist_surface_states(
        state,
        params.dst_gpu,
        dst_bytes,
        params.desc_gpu,
        desc_bytes,
    ) {
        return false;
    }
    for walker in 0..walker_count {
        let payload_offset = CANVAS3D_PLANE_FILL_PAYLOAD_OFFSET_BYTES
            + walker * CANVAS3D_PLANE_PATCH_WORKLIST_INDIRECT_BYTES;
        let work_base = (walker as u32).saturating_mul(16);
        if !direct_rcs_write_canvas3d_plane_patch_worklist_payload_at(
            state,
            payload_offset,
            params,
            work_base,
        ) {
            return false;
        }
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
    ok &= direct_rcs_push(batch, &mut cursor, CANVAS3D_PLANE_FILL_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, CANVAS3D_PLANE_FILL_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        CANVAS3D_PLANE_PATCH_WORKLIST_PRE_MARKER_SLOT,
        CANVAS3D_PLANE_PATCH_WORKLIST_PRE_MARKER,
    );
    for walker in 0..walker_count {
        let payload_offset = CANVAS3D_PLANE_FILL_PAYLOAD_OFFSET_BYTES
            + walker * CANVAS3D_PLANE_PATCH_WORKLIST_INDIRECT_BYTES;
        ok &= direct_rcs_push_canvas3d_plane_patch_worklist_walker(
            batch,
            &mut cursor,
            payload_offset,
        );
        ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
        ok &= direct_rcs_push(batch, &mut cursor, 0);
    }
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_FLUSH_BITS);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        CANVAS3D_PLANE_PATCH_WORKLIST_POST_MARKER_SLOT,
        CANVAS3D_PLANE_PATCH_WORKLIST_POST_MARKER,
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

fn direct_rcs_encode_fill_rect_walker_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: FillRectRgba8Params,
    dst_bytes: usize,
    right_mask: u32,
) -> bool {
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
    ok &= direct_rcs_push(batch, &mut cursor, GPGPU_WALKER_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, CLEAR_RECT_INDIRECT_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, CLEAR_RECT_PAYLOAD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push(
        batch,
        &mut cursor,
        (GPGPU_WALKER_SIMD16_SELECT << 30) | (GPGPU_WALKER_GROUP_THREADS - 1),
    );
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 1);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 1);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, GPGPU_WALKER_GROUP_Z_DIM);
    ok &= direct_rcs_push(batch, &mut cursor, right_mask);
    ok &= direct_rcs_push(batch, &mut cursor, GPGPU_WALKER_BOTTOM_MASK);
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

fn direct_rcs_encode_present_rgba8_to_primary_xrgb_span_params_batch(
    state: DirectRcsState,
    flavor: CopyRectKernelFlavor,
    span_params: &[PresentRgba8ToPrimaryXrgbRectParams],
    src_bytes: usize,
    dst_bytes: usize,
) -> bool {
    if span_params.is_empty() || span_params.len() > COPY_RECT_BATCH_MAX_SPANS {
        return false;
    }
    let payload_end = COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES
        + span_params.len() * PRESENT_RGBA8_TO_PRIMARY_XRGB_INDIRECT_BYTES;
    if payload_end > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }

    let first = span_params[0];
    if !direct_rcs_write_copy_rect_interface_descriptor_at_with_cross_thread_grfs(
        state,
        COPY_RECT_BATCH_IDD_OFFSET_BYTES,
        COPY_RECT_BATCH_BINDING_TABLE_OFFSET_BYTES,
        flavor.text_offset_bytes,
        4,
    ) {
        return false;
    }
    if !direct_rcs_write_copy_rect_surface_states_at(
        state,
        COPY_RECT_BATCH_BINDING_TABLE_OFFSET_BYTES,
        COPY_RECT_BATCH_SRC_SURFACE_STATE_OFFSET_BYTES,
        COPY_RECT_BATCH_DST_SURFACE_STATE_OFFSET_BYTES,
        first.src_gpu,
        src_bytes,
        first.dst_gpu,
        dst_bytes,
    ) {
        return false;
    }
    for (span, params) in span_params.iter().copied().enumerate() {
        let payload_offset = COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES
            + span * PRESENT_RGBA8_TO_PRIMARY_XRGB_INDIRECT_BYTES;
        if !direct_rcs_write_present_rgba8_to_primary_xrgb_payload_at(state, payload_offset, params)
        {
            return false;
        }
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
        flavor.upload.gpu,
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
    for (span, params) in span_params.iter().copied().enumerate() {
        let payload_offset = COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES
            + span * PRESENT_RGBA8_TO_PRIMARY_XRGB_INDIRECT_BYTES;
        ok &= direct_rcs_push_copy_rect_walker(
            batch,
            &mut cursor,
            payload_offset,
            copy_rect_walker_right_mask_for(params.width, flavor.pixels_per_lane),
        );
        ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
        ok &= direct_rcs_push(batch, &mut cursor, 0);
    }
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

fn direct_rcs_write_copy_rect_interface_descriptor(state: DirectRcsState) -> bool {
    direct_rcs_write_copy_rect_interface_descriptor_at(
        state,
        COPY_RECT_IDD_OFFSET_BYTES,
        COPY_RECT_BINDING_TABLE_OFFSET_BYTES,
        COPY_RECT_RGBA8_TEXT_OFFSET_BYTES,
    )
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
    if idd_offset + COPY_RECT_IDD_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    let idd = unsafe { state.batch_virt.add(idd_offset) as *mut u32 };
    unsafe {
        core::ptr::write_volatile(idd, text_offset_bytes as u32);
        core::ptr::write_volatile(idd.add(1), 0);
        core::ptr::write_volatile(idd.add(2), IDD_THREAD_PREEMPTION_DISABLE);
        core::ptr::write_volatile(idd.add(3), 0);
        core::ptr::write_volatile(idd.add(4), (binding_table_offset as u32) | 2);
        core::ptr::write_volatile(idd.add(5), 3 << 16);
        core::ptr::write_volatile(idd.add(6), GPGPU_WALKER_GROUP_THREADS);
        core::ptr::write_volatile(idd.add(7), cross_thread_grfs);
    }
    true
}

fn direct_rcs_write_copy_rect_surface_states(
    state: DirectRcsState,
    src_gpu: u64,
    src_bytes: usize,
    dst_gpu: u64,
    dst_bytes: usize,
) -> bool {
    direct_rcs_write_copy_rect_surface_states_at(
        state,
        COPY_RECT_BINDING_TABLE_OFFSET_BYTES,
        COPY_RECT_SRC_SURFACE_STATE_OFFSET_BYTES,
        COPY_RECT_DST_SURFACE_STATE_OFFSET_BYTES,
        src_gpu,
        src_bytes,
        dst_gpu,
        dst_bytes,
    )
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

fn direct_rcs_write_copy_rect_payload(state: DirectRcsState, params: CopyRectRgba8Params) -> bool {
    direct_rcs_write_copy_rect_payload_at(state, COPY_RECT_PAYLOAD_OFFSET_BYTES, params)
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

fn direct_rcs_write_blit_rgba8_nearest_payload_at(
    state: DirectRcsState,
    payload_offset: usize,
    params: BlitRgba8NearestParams,
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
        core::ptr::write_volatile(dwords.add(20), params.src_width);
        core::ptr::write_volatile(dwords.add(21), params.src_height);
        core::ptr::write_volatile(dwords.add(22), params.dst_x);
        core::ptr::write_volatile(dwords.add(23), params.dst_y);
        core::ptr::write_volatile(dwords.add(24), params.dst_width);
        core::ptr::write_volatile(dwords.add(25), params.dst_height);

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

fn direct_rcs_write_present_rgba8_to_primary_xrgb_payload_at(
    state: DirectRcsState,
    payload_offset: usize,
    params: PresentRgba8ToPrimaryXrgbRectParams,
) -> bool {
    if payload_offset + PRESENT_RGBA8_TO_PRIMARY_XRGB_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        let payload = state.batch_virt.add(payload_offset);
        core::ptr::write_bytes(payload, 0, PRESENT_RGBA8_TO_PRIMARY_XRGB_INDIRECT_BYTES);
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
        core::ptr::write_volatile(dwords.add(24), params.flip_y);

        let local_ids = payload.add(PRESENT_RGBA8_TO_PRIMARY_XRGB_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_write_sprite64_worklist_interface_descriptor(state: DirectRcsState) -> bool {
    if SPRITE64_WORKLIST_IDD_OFFSET_BYTES + SPRITE64_WORKLIST_IDD_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    let idd = unsafe { state.batch_virt.add(SPRITE64_WORKLIST_IDD_OFFSET_BYTES) as *mut u32 };
    unsafe {
        core::ptr::write_volatile(idd, SPRITE64_WORKLIST_RGBA8_TEXT_OFFSET_BYTES as u32);
        core::ptr::write_volatile(idd.add(1), 0);
        core::ptr::write_volatile(idd.add(2), IDD_THREAD_PREEMPTION_DISABLE);
        core::ptr::write_volatile(idd.add(3), 0);
        core::ptr::write_volatile(
            idd.add(4),
            (SPRITE64_WORKLIST_BINDING_TABLE_OFFSET_BYTES as u32) | 3,
        );
        core::ptr::write_volatile(idd.add(5), 3 << 16);
        core::ptr::write_volatile(idd.add(6), GPGPU_WALKER_GROUP_THREADS);
        core::ptr::write_volatile(idd.add(7), 3);
    }
    true
}

fn direct_rcs_write_sprite64_worklist_surface_states(
    state: DirectRcsState,
    atlas_gpu: u64,
    atlas_bytes: usize,
    dst_gpu: u64,
    dst_bytes: usize,
    desc_gpu: u64,
    desc_bytes: usize,
) -> bool {
    let binding_end =
        SPRITE64_WORKLIST_BINDING_TABLE_OFFSET_BYTES + 3 * core::mem::size_of::<u32>();
    let surface_bytes = COPY_RECT_SURFACE_STATE_DWORDS * core::mem::size_of::<u32>();
    let atlas_surface_end = SPRITE64_WORKLIST_ATLAS_SURFACE_STATE_OFFSET_BYTES + surface_bytes;
    let dst_surface_end = SPRITE64_WORKLIST_DST_SURFACE_STATE_OFFSET_BYTES + surface_bytes;
    let desc_surface_end = SPRITE64_WORKLIST_DESC_SURFACE_STATE_OFFSET_BYTES + surface_bytes;
    if binding_end > DIRECT_RCS_BATCH_BYTES
        || atlas_surface_end > DIRECT_RCS_BATCH_BYTES
        || dst_surface_end > DIRECT_RCS_BATCH_BYTES
        || desc_surface_end > DIRECT_RCS_BATCH_BYTES
    {
        return false;
    }

    unsafe {
        let binding = state
            .batch_virt
            .add(SPRITE64_WORKLIST_BINDING_TABLE_OFFSET_BYTES) as *mut u32;
        core::ptr::write_volatile(
            binding,
            SPRITE64_WORKLIST_ATLAS_SURFACE_STATE_OFFSET_BYTES as u32,
        );
        core::ptr::write_volatile(
            binding.add(1),
            SPRITE64_WORKLIST_DST_SURFACE_STATE_OFFSET_BYTES as u32,
        );
        core::ptr::write_volatile(
            binding.add(2),
            SPRITE64_WORKLIST_DESC_SURFACE_STATE_OFFSET_BYTES as u32,
        );
    }

    direct_rcs_write_buffer_surface_state(
        state,
        SPRITE64_WORKLIST_ATLAS_SURFACE_STATE_OFFSET_BYTES,
        atlas_gpu,
        atlas_bytes,
    ) && direct_rcs_write_buffer_surface_state(
        state,
        SPRITE64_WORKLIST_DST_SURFACE_STATE_OFFSET_BYTES,
        dst_gpu,
        dst_bytes,
    ) && direct_rcs_write_buffer_surface_state(
        state,
        SPRITE64_WORKLIST_DESC_SURFACE_STATE_OFFSET_BYTES,
        desc_gpu,
        desc_bytes,
    )
}

fn direct_rcs_write_sprite64_worklist_payload_at(
    state: DirectRcsState,
    payload_offset: usize,
    params: Sprite64WorklistRgba8Params,
) -> bool {
    if payload_offset + SPRITE64_WORKLIST_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        let payload = state.batch_virt.add(payload_offset);
        core::ptr::write_bytes(payload, 0, SPRITE64_WORKLIST_INDIRECT_BYTES);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        core::ptr::write_volatile(dwords.add(12), params.atlas_gpu as u32);
        core::ptr::write_volatile(dwords.add(13), (params.atlas_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(14), params.dst_gpu as u32);
        core::ptr::write_volatile(dwords.add(15), (params.dst_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(16), params.desc_gpu as u32);
        core::ptr::write_volatile(dwords.add(17), (params.desc_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(18), params.atlas_pitch_bytes);
        core::ptr::write_volatile(dwords.add(19), params.dst_pitch_bytes);
        core::ptr::write_volatile(dwords.add(20), params.desc_base);
        core::ptr::write_volatile(dwords.add(21), params.desc_count);

        let local_ids = payload.add(SPRITE64_WORKLIST_CROSS_THREAD_BYTES) as *mut u16;
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
        RECT_WORKLIST_CROSS_THREAD_GRFS,
    )
}

fn direct_rcs_write_gradient_rect_worklist_interface_descriptor(state: DirectRcsState) -> bool {
    direct_rcs_write_rect_worklist_interface_descriptor(
        state,
        GRADIENT_RECT_WORKLIST_RGBA8_TEXT_OFFSET_BYTES,
        2,
        RECT_WORKLIST_CROSS_THREAD_GRFS,
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

fn direct_rcs_write_alpha_blend_worklist_interface_descriptor(state: DirectRcsState) -> bool {
    direct_rcs_write_rect_worklist_interface_descriptor(
        state,
        ALPHA_BLEND_WORKLIST_RGBA8_TEXT_OFFSET_BYTES,
        3,
        RECT_WORKLIST_CROSS_THREAD_GRFS,
    )
}

fn direct_rcs_write_rect_worklist_interface_descriptor(
    state: DirectRcsState,
    text_offset_bytes: u64,
    binding_table_entries: u32,
    cross_thread_grfs: u32,
) -> bool {
    if RECT_WORKLIST_IDD_OFFSET_BYTES + RECT_WORKLIST_IDD_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    let idd = unsafe { state.batch_virt.add(RECT_WORKLIST_IDD_OFFSET_BYTES) as *mut u32 };
    unsafe {
        core::ptr::write_volatile(idd, text_offset_bytes as u32);
        core::ptr::write_volatile(idd.add(1), 0);
        core::ptr::write_volatile(idd.add(2), IDD_THREAD_PREEMPTION_DISABLE);
        core::ptr::write_volatile(idd.add(3), 0);
        core::ptr::write_volatile(
            idd.add(4),
            (RECT_WORKLIST_BINDING_TABLE_OFFSET_BYTES as u32) | binding_table_entries,
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
    let binding_end = RECT_WORKLIST_BINDING_TABLE_OFFSET_BYTES + 3 * core::mem::size_of::<u32>();
    let surface_bytes = COPY_RECT_SURFACE_STATE_DWORDS * core::mem::size_of::<u32>();
    let src_surface_end = RECT_WORKLIST_SRC_SURFACE_STATE_OFFSET_BYTES + surface_bytes;
    let dst_surface_end = RECT_WORKLIST_DST_SURFACE_STATE_OFFSET_BYTES + surface_bytes;
    let desc_surface_end = RECT_WORKLIST_DESC_SURFACE_STATE_OFFSET_BYTES + surface_bytes;
    if binding_end > DIRECT_RCS_BATCH_BYTES
        || src_surface_end > DIRECT_RCS_BATCH_BYTES
        || dst_surface_end > DIRECT_RCS_BATCH_BYTES
        || desc_surface_end > DIRECT_RCS_BATCH_BYTES
    {
        return false;
    }

    unsafe {
        let binding = state
            .batch_virt
            .add(RECT_WORKLIST_BINDING_TABLE_OFFSET_BYTES) as *mut u32;
        core::ptr::write_volatile(binding, RECT_WORKLIST_SRC_SURFACE_STATE_OFFSET_BYTES as u32);
        core::ptr::write_volatile(
            binding.add(1),
            RECT_WORKLIST_DST_SURFACE_STATE_OFFSET_BYTES as u32,
        );
        core::ptr::write_volatile(
            binding.add(2),
            RECT_WORKLIST_DESC_SURFACE_STATE_OFFSET_BYTES as u32,
        );
    }

    direct_rcs_write_buffer_surface_state(
        state,
        RECT_WORKLIST_SRC_SURFACE_STATE_OFFSET_BYTES,
        src_gpu,
        src_bytes,
    ) && direct_rcs_write_buffer_surface_state(
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

fn direct_rcs_write_gradient_rect_worklist_payload_at(
    state: DirectRcsState,
    payload_offset: usize,
    params: GradientRectWorklistRgba8Params,
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

fn direct_rcs_write_alpha_blend_worklist_payload_at(
    state: DirectRcsState,
    payload_offset: usize,
    params: AlphaBlendWorklistRgba8Params,
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
        core::ptr::write_volatile(dwords.add(12), params.src_gpu as u32);
        core::ptr::write_volatile(dwords.add(13), (params.src_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(14), params.dst_gpu as u32);
        core::ptr::write_volatile(dwords.add(15), (params.dst_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(16), params.desc_gpu as u32);
        core::ptr::write_volatile(dwords.add(17), (params.desc_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(18), params.src_pitch_bytes);
        core::ptr::write_volatile(dwords.add(19), params.dst_pitch_bytes);
        core::ptr::write_volatile(dwords.add(20), params.desc_base);
        core::ptr::write_volatile(dwords.add(21), params.desc_count);

        let local_ids = payload.add(RECT_WORKLIST_CROSS_THREAD_BYTES) as *mut u16;
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

fn direct_rcs_write_canvas3d_project_payload(
    state: DirectRcsState,
    params: Canvas3dProjectRgba8Params,
) -> bool {
    if CANVAS3D_PROJECT_PAYLOAD_OFFSET_BYTES + CANVAS3D_PROJECT_INDIRECT_BYTES
        > DIRECT_RCS_BATCH_BYTES
    {
        return false;
    }

    unsafe {
        let payload = state.batch_virt.add(CANVAS3D_PROJECT_PAYLOAD_OFFSET_BYTES);
        core::ptr::write_bytes(payload, 0, CANVAS3D_PROJECT_INDIRECT_BYTES);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        core::ptr::write_volatile(dwords.add(12), params.vertices_gpu as u32);
        core::ptr::write_volatile(dwords.add(13), (params.vertices_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(14), params.out_gpu as u32);
        core::ptr::write_volatile(dwords.add(15), (params.out_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(16), params.src_first_vertex);
        core::ptr::write_volatile(dwords.add(17), params.out_first_point);
        core::ptr::write_volatile(dwords.add(18), params.vertex_count);
        core::ptr::write_volatile(dwords.add(19), params.canvas_width);
        core::ptr::write_volatile(dwords.add(20), params.canvas_height);

        let local_ids = payload.add(CANVAS3D_PROJECT_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_write_canvas3d_transform_fused_payload(
    state: DirectRcsState,
    params: Canvas3dTransformFusedQ16Params,
) -> bool {
    if CANVAS3D_TRANSFORM_PAYLOAD_OFFSET_BYTES + CANVAS3D_TRANSFORM_FUSED_INDIRECT_BYTES
        > DIRECT_RCS_BATCH_BYTES
    {
        return false;
    }

    unsafe {
        let payload = state
            .batch_virt
            .add(CANVAS3D_TRANSFORM_PAYLOAD_OFFSET_BYTES);
        core::ptr::write_bytes(payload, 0, CANVAS3D_TRANSFORM_FUSED_INDIRECT_BYTES);
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
        core::ptr::write_volatile(dwords.add(16), params.src_first_vertex);
        core::ptr::write_volatile(dwords.add(17), params.dst_first_vertex);
        core::ptr::write_volatile(dwords.add(18), params.vertex_count);
        core::ptr::write_volatile(dwords.add(19), 0);
        core::ptr::write_volatile(dwords.add(20), params.scale_q16.x as u32);
        core::ptr::write_volatile(dwords.add(21), params.scale_q16.y as u32);
        core::ptr::write_volatile(dwords.add(22), params.scale_q16.z as u32);
        core::ptr::write_volatile(dwords.add(23), params.scale_q16.pad as u32);
        core::ptr::write_volatile(dwords.add(24), params.rotate_q16.x as u32);
        core::ptr::write_volatile(dwords.add(25), params.rotate_q16.y as u32);
        core::ptr::write_volatile(dwords.add(26), params.rotate_q16.z as u32);
        core::ptr::write_volatile(dwords.add(27), params.rotate_q16.pad as u32);
        core::ptr::write_volatile(dwords.add(28), params.translate_q16.x as u32);
        core::ptr::write_volatile(dwords.add(29), params.translate_q16.y as u32);
        core::ptr::write_volatile(dwords.add(30), params.translate_q16.z as u32);
        core::ptr::write_volatile(dwords.add(31), params.translate_q16.pad as u32);

        let local_ids = payload.add(CANVAS3D_TRANSFORM_FUSED_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_write_canvas3d_clip_box_payload(
    state: DirectRcsState,
    params: Canvas3dClipBoxQ16Params,
) -> bool {
    if CANVAS3D_CLIP_BOX_PAYLOAD_OFFSET_BYTES + CANVAS3D_CLIP_BOX_INDIRECT_BYTES
        > DIRECT_RCS_BATCH_BYTES
    {
        return false;
    }

    unsafe {
        let payload = state.batch_virt.add(CANVAS3D_CLIP_BOX_PAYLOAD_OFFSET_BYTES);
        core::ptr::write_bytes(payload, 0, CANVAS3D_CLIP_BOX_INDIRECT_BYTES);
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
        core::ptr::write_volatile(dwords.add(16), params.src_first_vertex);
        core::ptr::write_volatile(dwords.add(17), params.dst_first_vertex);
        core::ptr::write_volatile(dwords.add(18), params.vertex_count);
        core::ptr::write_volatile(dwords.add(19), 0);
        core::ptr::write_volatile(dwords.add(20), params.min_q16.x as u32);
        core::ptr::write_volatile(dwords.add(21), params.min_q16.y as u32);
        core::ptr::write_volatile(dwords.add(22), params.min_q16.z as u32);
        core::ptr::write_volatile(dwords.add(23), params.min_q16.pad as u32);
        core::ptr::write_volatile(dwords.add(24), params.max_q16.x as u32);
        core::ptr::write_volatile(dwords.add(25), params.max_q16.y as u32);
        core::ptr::write_volatile(dwords.add(26), params.max_q16.z as u32);
        core::ptr::write_volatile(dwords.add(27), params.max_q16.pad as u32);

        let local_ids = payload.add(CANVAS3D_CLIP_BOX_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_write_canvas3d_plane_sample_payload(
    state: DirectRcsState,
    params: Canvas3dPlaneSampleRgba8Params,
) -> bool {
    if CANVAS3D_PLANE_SAMPLE_PAYLOAD_OFFSET_BYTES + CANVAS3D_PLANE_SAMPLE_INDIRECT_BYTES
        > DIRECT_RCS_BATCH_BYTES
    {
        return false;
    }

    unsafe {
        let payload = state
            .batch_virt
            .add(CANVAS3D_PLANE_SAMPLE_PAYLOAD_OFFSET_BYTES);
        core::ptr::write_bytes(payload, 0, CANVAS3D_PLANE_SAMPLE_INDIRECT_BYTES);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        core::ptr::write_volatile(dwords.add(12), params.out_gpu as u32);
        core::ptr::write_volatile(dwords.add(13), (params.out_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(14), params.out_gpu as u32);
        core::ptr::write_volatile(dwords.add(15), (params.out_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(16), params.out_first_point);
        core::ptr::write_volatile(dwords.add(17), params.sample_count);
        core::ptr::write_volatile(dwords.add(18), params.canvas_width);
        core::ptr::write_volatile(dwords.add(19), params.canvas_height);
        core::ptr::write_volatile(dwords.add(20), params.origin_q16.x as u32);
        core::ptr::write_volatile(dwords.add(21), params.origin_q16.y as u32);
        core::ptr::write_volatile(dwords.add(22), params.origin_q16.z as u32);
        core::ptr::write_volatile(dwords.add(23), params.origin_q16.pad as u32);
        core::ptr::write_volatile(dwords.add(24), params.axis_u_q16.x as u32);
        core::ptr::write_volatile(dwords.add(25), params.axis_u_q16.y as u32);
        core::ptr::write_volatile(dwords.add(26), params.axis_u_q16.z as u32);
        core::ptr::write_volatile(dwords.add(27), params.axis_u_q16.pad as u32);
        core::ptr::write_volatile(dwords.add(28), params.axis_v_q16.x as u32);
        core::ptr::write_volatile(dwords.add(29), params.axis_v_q16.y as u32);
        core::ptr::write_volatile(dwords.add(30), params.axis_v_q16.z as u32);
        core::ptr::write_volatile(dwords.add(31), params.axis_v_q16.pad as u32);
        core::ptr::write_volatile(dwords.add(32), params.constraint0_q16.x as u32);
        core::ptr::write_volatile(dwords.add(33), params.constraint0_q16.y as u32);
        core::ptr::write_volatile(dwords.add(34), params.constraint0_q16.z as u32);
        core::ptr::write_volatile(dwords.add(35), params.constraint0_q16.pad as u32);
        core::ptr::write_volatile(dwords.add(36), params.constraint1_q16.x as u32);
        core::ptr::write_volatile(dwords.add(37), params.constraint1_q16.y as u32);
        core::ptr::write_volatile(dwords.add(38), params.constraint1_q16.z as u32);
        core::ptr::write_volatile(dwords.add(39), params.constraint1_q16.pad as u32);
        core::ptr::write_volatile(dwords.add(40), params.constraint2_q16.x as u32);
        core::ptr::write_volatile(dwords.add(41), params.constraint2_q16.y as u32);
        core::ptr::write_volatile(dwords.add(42), params.constraint2_q16.z as u32);
        core::ptr::write_volatile(dwords.add(43), params.constraint2_q16.pad as u32);
        core::ptr::write_volatile(dwords.add(44), params.constraint3_q16.x as u32);
        core::ptr::write_volatile(dwords.add(45), params.constraint3_q16.y as u32);
        core::ptr::write_volatile(dwords.add(46), params.constraint3_q16.z as u32);
        core::ptr::write_volatile(dwords.add(47), params.constraint3_q16.pad as u32);
        core::ptr::write_volatile(dwords.add(48), params.constraint_count);
        core::ptr::write_volatile(dwords.add(49), params.u_steps);
        core::ptr::write_volatile(dwords.add(50), params.v_steps);
        core::ptr::write_volatile(dwords.add(51), params.color_rgba);

        let local_ids = payload.add(CANVAS3D_PLANE_SAMPLE_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_write_canvas3d_plane_fill_payload(
    state: DirectRcsState,
    params: Canvas3dPlaneFillRgba8Params,
) -> bool {
    if CANVAS3D_PLANE_FILL_PAYLOAD_OFFSET_BYTES + CANVAS3D_PLANE_FILL_INDIRECT_BYTES
        > DIRECT_RCS_BATCH_BYTES
    {
        return false;
    }

    unsafe {
        let payload = state
            .batch_virt
            .add(CANVAS3D_PLANE_FILL_PAYLOAD_OFFSET_BYTES);
        core::ptr::write_bytes(payload, 0, CANVAS3D_PLANE_FILL_INDIRECT_BYTES);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        core::ptr::write_volatile(dwords.add(12), params.dst_gpu as u32);
        core::ptr::write_volatile(dwords.add(13), (params.dst_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(14), params.dst_gpu as u32);
        core::ptr::write_volatile(dwords.add(15), (params.dst_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(16), params.dst_pitch_bytes);
        core::ptr::write_volatile(dwords.add(17), params.dst_width);
        core::ptr::write_volatile(dwords.add(18), params.dst_height);
        core::ptr::write_volatile(dwords.add(19), params.rect_x);
        core::ptr::write_volatile(dwords.add(20), params.rect_y);
        core::ptr::write_volatile(dwords.add(21), params.rect_width);
        core::ptr::write_volatile(dwords.add(22), params.rect_height);
        core::ptr::write_volatile(dwords.add(23), params.canvas_width);
        core::ptr::write_volatile(dwords.add(24), params.canvas_height);
        core::ptr::write_volatile(dwords.add(28), params.origin_q16.x as u32);
        core::ptr::write_volatile(dwords.add(29), params.origin_q16.y as u32);
        core::ptr::write_volatile(dwords.add(30), params.origin_q16.z as u32);
        core::ptr::write_volatile(dwords.add(31), params.origin_q16.pad as u32);
        core::ptr::write_volatile(dwords.add(32), params.axis_u_q16.x as u32);
        core::ptr::write_volatile(dwords.add(33), params.axis_u_q16.y as u32);
        core::ptr::write_volatile(dwords.add(34), params.axis_u_q16.z as u32);
        core::ptr::write_volatile(dwords.add(35), params.axis_u_q16.pad as u32);
        core::ptr::write_volatile(dwords.add(36), params.axis_v_q16.x as u32);
        core::ptr::write_volatile(dwords.add(37), params.axis_v_q16.y as u32);
        core::ptr::write_volatile(dwords.add(38), params.axis_v_q16.z as u32);
        core::ptr::write_volatile(dwords.add(39), params.axis_v_q16.pad as u32);
        core::ptr::write_volatile(dwords.add(40), params.constraint0_q16.x as u32);
        core::ptr::write_volatile(dwords.add(41), params.constraint0_q16.y as u32);
        core::ptr::write_volatile(dwords.add(42), params.constraint0_q16.z as u32);
        core::ptr::write_volatile(dwords.add(43), params.constraint0_q16.pad as u32);
        core::ptr::write_volatile(dwords.add(44), params.constraint1_q16.x as u32);
        core::ptr::write_volatile(dwords.add(45), params.constraint1_q16.y as u32);
        core::ptr::write_volatile(dwords.add(46), params.constraint1_q16.z as u32);
        core::ptr::write_volatile(dwords.add(47), params.constraint1_q16.pad as u32);
        core::ptr::write_volatile(dwords.add(48), params.constraint2_q16.x as u32);
        core::ptr::write_volatile(dwords.add(49), params.constraint2_q16.y as u32);
        core::ptr::write_volatile(dwords.add(50), params.constraint2_q16.z as u32);
        core::ptr::write_volatile(dwords.add(51), params.constraint2_q16.pad as u32);
        core::ptr::write_volatile(dwords.add(52), params.constraint3_q16.x as u32);
        core::ptr::write_volatile(dwords.add(53), params.constraint3_q16.y as u32);
        core::ptr::write_volatile(dwords.add(54), params.constraint3_q16.z as u32);
        core::ptr::write_volatile(dwords.add(55), params.constraint3_q16.pad as u32);
        core::ptr::write_volatile(dwords.add(56), params.constraint_count);
        core::ptr::write_volatile(dwords.add(57), params.color_rgba);

        let local_ids = payload.add(CANVAS3D_PLANE_FILL_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_write_canvas3d_plane_patch_worklist_surface_states(
    state: DirectRcsState,
    dst_gpu: u64,
    dst_bytes: usize,
    desc_gpu: u64,
    desc_bytes: usize,
) -> bool {
    let binding_end =
        CANVAS3D_PLANE_FILL_BINDING_TABLE_OFFSET_BYTES + 2 * core::mem::size_of::<u32>();
    let surface_bytes = COPY_RECT_SURFACE_STATE_DWORDS * core::mem::size_of::<u32>();
    let dst_surface_end = CANVAS3D_PLANE_FILL_DST_SURFACE_STATE_OFFSET_BYTES + surface_bytes;
    let desc_surface_end =
        CANVAS3D_PLANE_PATCH_WORKLIST_DESC_SURFACE_STATE_OFFSET_BYTES + surface_bytes;
    if binding_end > DIRECT_RCS_BATCH_BYTES
        || dst_surface_end > DIRECT_RCS_BATCH_BYTES
        || desc_surface_end > DIRECT_RCS_BATCH_BYTES
    {
        return false;
    }

    unsafe {
        let binding = state
            .batch_virt
            .add(CANVAS3D_PLANE_FILL_BINDING_TABLE_OFFSET_BYTES) as *mut u32;
        core::ptr::write_volatile(
            binding,
            CANVAS3D_PLANE_FILL_DST_SURFACE_STATE_OFFSET_BYTES as u32,
        );
        core::ptr::write_volatile(
            binding.add(1),
            CANVAS3D_PLANE_PATCH_WORKLIST_DESC_SURFACE_STATE_OFFSET_BYTES as u32,
        );
    }

    direct_rcs_write_buffer_surface_state(
        state,
        CANVAS3D_PLANE_FILL_DST_SURFACE_STATE_OFFSET_BYTES,
        dst_gpu,
        dst_bytes,
    ) && direct_rcs_write_buffer_surface_state(
        state,
        CANVAS3D_PLANE_PATCH_WORKLIST_DESC_SURFACE_STATE_OFFSET_BYTES,
        desc_gpu,
        desc_bytes,
    )
}

fn direct_rcs_write_canvas3d_plane_patch_worklist_interface_descriptor(
    state: DirectRcsState,
) -> bool {
    if CANVAS3D_PLANE_FILL_IDD_OFFSET_BYTES + CANVAS3D_PLANE_FILL_IDD_BYTES > DIRECT_RCS_BATCH_BYTES
    {
        return false;
    }
    let idd = unsafe { state.batch_virt.add(CANVAS3D_PLANE_FILL_IDD_OFFSET_BYTES) as *mut u32 };
    unsafe {
        core::ptr::write_volatile(
            idd,
            CANVAS3D_PLANE_PATCH_WORKLIST_RGBA8_TEXT_OFFSET_BYTES as u32,
        );
        core::ptr::write_volatile(idd.add(1), 0);
        core::ptr::write_volatile(idd.add(2), IDD_THREAD_PREEMPTION_DISABLE);
        core::ptr::write_volatile(idd.add(3), 0);
        core::ptr::write_volatile(
            idd.add(4),
            (CANVAS3D_PLANE_FILL_BINDING_TABLE_OFFSET_BYTES as u32) | 2,
        );
        core::ptr::write_volatile(idd.add(5), 3 << 16);
        core::ptr::write_volatile(idd.add(6), GPGPU_WALKER_GROUP_THREADS);
        core::ptr::write_volatile(idd.add(7), 3);
    }
    true
}

fn direct_rcs_write_canvas3d_plane_patch_worklist_payload_at(
    state: DirectRcsState,
    payload_offset: usize,
    params: Canvas3dPlanePatchWorklistRgba8Params,
    work_base: u32,
) -> bool {
    if payload_offset + CANVAS3D_PLANE_PATCH_WORKLIST_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        let payload = state.batch_virt.add(payload_offset);
        core::ptr::write_bytes(payload, 0, CANVAS3D_PLANE_PATCH_WORKLIST_INDIRECT_BYTES);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        core::ptr::write_volatile(dwords.add(12), params.dst_gpu as u32);
        core::ptr::write_volatile(dwords.add(13), (params.dst_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(14), params.dst_gpu as u32);
        core::ptr::write_volatile(dwords.add(15), (params.dst_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(16), params.desc_gpu as u32);
        core::ptr::write_volatile(dwords.add(17), (params.desc_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(18), params.desc_base);
        core::ptr::write_volatile(dwords.add(19), params.desc_count);
        core::ptr::write_volatile(dwords.add(20), work_base);

        let local_ids = payload.add(CANVAS3D_PLANE_PATCH_WORKLIST_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
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

fn direct_rcs_seed_copy_rect_strip(state: DirectRcsState) -> ([u32; 4], [u32; 4]) {
    let src_values = [
        CLEAR_RECT_RGBA_RED,
        CLEAR_RECT_RGBA_GREEN,
        CLEAR_RECT_RGBA_BLUE,
        CLEAR_RECT_RGBA_BLACK,
    ];
    let dst_values = [
        COPY_RECT_DST_POISON0,
        COPY_RECT_DST_POISON1,
        COPY_RECT_DST_POISON2,
        COPY_RECT_DST_POISON3,
    ];
    unsafe {
        core::ptr::write_bytes(state.clear_test_virt, 0, CLEAR_RECT_TEST_BYTES);
        let row = state.clear_test_virt as *mut u32;
        for (index, value) in src_values.iter().copied().enumerate() {
            core::ptr::write_volatile(row.add(index), value);
        }
        for (index, value) in dst_values.iter().copied().enumerate() {
            core::ptr::write_volatile(row.add(index + COPY_RECT_TEST_PIXELS), value);
        }
    }
    super::dma_flush(state.clear_test_virt, CLEAR_RECT_TEST_BYTES);
    (src_values, dst_values)
}

fn direct_rcs_seed_rect_api_smoke(state: DirectRcsState) {
    let values = [
        CLEAR_RECT_RGBA_RED,
        CLEAR_RECT_RGBA_GREEN,
        CLEAR_RECT_RGBA_BLUE,
        CLEAR_RECT_RGBA_BLACK,
        COPY_RECT_DST_POISON0,
        COPY_RECT_DST_POISON1,
        COPY_RECT_DST_POISON2,
        COPY_RECT_DST_POISON3,
        0xFF11_2233,
        0xFF44_5566,
        0xFF77_8899,
        0xFFAA_BBCC,
        0x5555_5555,
        0x6666_6666,
        0x7777_7777,
        0x8888_8888,
        0xFF10_2030,
        0xFF40_5060,
        0xFF70_8090,
        0xFFA0_B0C0,
        0x9999_9999,
        0xAAAA_AAAA,
        0xBBBB_BBBB,
        0xCCCC_CCCC,
        0x1111_1111,
        0x2222_2222,
        0x3333_3333,
        0x4444_4444,
    ];
    unsafe {
        core::ptr::write_bytes(state.clear_test_virt, 0, CLEAR_RECT_TEST_BYTES);
        let row = state.clear_test_virt as *mut u32;
        for (index, value) in values.iter().copied().enumerate() {
            core::ptr::write_volatile(row.add(index), value);
        }
    }
    super::dma_flush(state.clear_test_virt, CLEAR_RECT_TEST_BYTES);
}

fn pack_i16_pair_u32(x: i16, y: i16) -> u32 {
    (u16::from_ne_bytes(x.to_ne_bytes()) as u32)
        | ((u16::from_ne_bytes(y.to_ne_bytes()) as u32) << 16)
}

fn pack_u16_pair_u32(x: u16, y: u16) -> u32 {
    (x as u32) | ((y as u32) << 16)
}

fn div255_u32(value: u32) -> u32 {
    (value + 127) / 255
}

fn blend_channel_u32(src: u32, dst: u32, src_alpha: u32) -> u32 {
    div255_u32(src.saturating_mul(src_alpha) + dst.saturating_mul(255 - src_alpha))
}

fn src_over_rgba8_u32(src: u32, dst: u32) -> u32 {
    let sa = (src >> 24) & 0xFF;
    if sa == 0 {
        return dst;
    }
    if sa == 255 {
        return src;
    }

    let sr = src & 0xFF;
    let sg = (src >> 8) & 0xFF;
    let sb = (src >> 16) & 0xFF;
    let da = (dst >> 24) & 0xFF;
    let dr = dst & 0xFF;
    let dg = (dst >> 8) & 0xFF;
    let db = (dst >> 16) & 0xFF;

    let out_r = blend_channel_u32(sr, dr, sa);
    let out_g = blend_channel_u32(sg, dg, sa);
    let out_b = blend_channel_u32(sb, db, sa);
    let out_a = sa + div255_u32(da.saturating_mul(255 - sa));

    (out_a << 24) | (out_b << 16) | (out_g << 8) | out_r
}

fn direct_rcs_seed_rect_api_glyph_mask(state: DirectRcsState) {
    unsafe {
        let mask = state.clear_test_virt;
        core::ptr::write_volatile(mask, 0x00);
        core::ptr::write_volatile(mask.add(1), 0x55);
        core::ptr::write_volatile(mask.add(2), 0xAA);
        core::ptr::write_volatile(mask.add(3), 0xFF);

        let row = state.clear_test_virt as *mut u32;
        for index in 28..32usize {
            core::ptr::write_volatile(row.add(index), CLEAR_RECT_RGBA_BLACK);
        }
    }
    super::dma_flush(state.clear_test_virt, CLEAR_RECT_TEST_BYTES);
}

fn direct_rcs_copy_rect_256_src_pixel(index: usize) -> u32 {
    let r = (index as u32) & 0xFF;
    let g = 0xFFu32.wrapping_sub(r);
    let b = ((index as u32).wrapping_mul(3)) & 0xFF;
    0xFF00_0000 | (b << 16) | (g << 8) | r
}

fn direct_rcs_copy_rect_256x2_src_pixel(x: usize, y: usize) -> u32 {
    let r = (x as u32) & 0xFF;
    let g = (0xFFu32.wrapping_sub(r)).wrapping_sub((y as u32).wrapping_mul(0x31)) & 0xFF;
    let b = ((x as u32).wrapping_mul(3) + (y as u32).wrapping_mul(0x55)) & 0xFF;
    0xFF00_0000 | (b << 16) | (g << 8) | r
}

fn direct_rcs_copy_rect_256_poison_pixel(index: usize) -> u32 {
    0xA500_0000 | ((index as u32) & 0x0000_FFFF)
}

fn direct_rcs_seed_copy_rect_256(state: DirectRcsState) {
    unsafe {
        core::ptr::write_bytes(state.clear_test_virt, 0, CLEAR_RECT_TEST_BYTES);
        let row = state.clear_test_virt as *mut u32;
        for index in 0..COPY_RECT_256_WIDTH as usize {
            core::ptr::write_volatile(row.add(index), direct_rcs_copy_rect_256_src_pixel(index));
            core::ptr::write_volatile(
                row.add(index + COPY_RECT_256_WIDTH as usize),
                direct_rcs_copy_rect_256_poison_pixel(index),
            );
        }
    }
    super::dma_flush(state.clear_test_virt, CLEAR_RECT_TEST_BYTES);
}

fn direct_rcs_seed_copy_rect_256x2(state: DirectRcsState) {
    unsafe {
        core::ptr::write_bytes(state.clear_test_virt, 0, CLEAR_RECT_TEST_BYTES);
        let surface = state.clear_test_virt as *mut u32;
        let pitch_pixels = COPY_RECT_256X2_SURFACE_WIDTH as usize;
        for y in 0..COPY_RECT_256X2_HEIGHT as usize {
            let row = surface.add(y * pitch_pixels);
            for x in 0..COPY_RECT_256X2_WIDTH as usize {
                core::ptr::write_volatile(row.add(x), direct_rcs_copy_rect_256x2_src_pixel(x, y));
                core::ptr::write_volatile(
                    row.add(x + COPY_RECT_256X2_WIDTH as usize),
                    direct_rcs_copy_rect_256_poison_pixel(y * pitch_pixels + x),
                );
            }
        }
    }
    super::dma_flush(state.clear_test_virt, CLEAR_RECT_TEST_BYTES);
}

fn direct_rcs_read_copy_rect_strip(state: DirectRcsState) -> ([u32; 4], [u32; 4]) {
    super::dma_flush(state.clear_test_virt, CLEAR_RECT_TEST_BYTES);
    let mut src_values = [0u32; 4];
    let mut dst_values = [0u32; 4];
    unsafe {
        let row = state.clear_test_virt as *const u32;
        for (index, value) in src_values.iter_mut().enumerate() {
            *value = core::ptr::read_volatile(row.add(index));
        }
        for (index, value) in dst_values.iter_mut().enumerate() {
            *value = core::ptr::read_volatile(row.add(index + COPY_RECT_TEST_PIXELS));
        }
    }
    (src_values, dst_values)
}

fn direct_rcs_read_rect_api_span(state: DirectRcsState, start_pixel: usize) -> [u32; 4] {
    super::dma_flush(state.clear_test_virt, CLEAR_RECT_TEST_BYTES);
    let mut values = [0u32; 4];
    unsafe {
        let row = state.clear_test_virt as *const u32;
        for (index, value) in values.iter_mut().enumerate() {
            *value = core::ptr::read_volatile(row.add(start_pixel + index));
        }
    }
    values
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

fn direct_rcs_read_copy_rect_256_span(state: DirectRcsState, start_pixel: usize) -> [u32; 4] {
    super::dma_flush(state.clear_test_virt, CLEAR_RECT_TEST_BYTES);
    let mut values = [0u32; 4];
    unsafe {
        let row = state.clear_test_virt as *const u32;
        for (index, value) in values.iter_mut().enumerate() {
            *value = core::ptr::read_volatile(row.add(start_pixel + index));
        }
    }
    values
}

fn direct_rcs_read_copy_rect_256x2_span(
    state: DirectRcsState,
    row_index: usize,
    start_pixel: usize,
) -> [u32; 4] {
    super::dma_flush(state.clear_test_virt, CLEAR_RECT_TEST_BYTES);
    let mut values = [0u32; 4];
    unsafe {
        let surface = state.clear_test_virt as *const u32;
        let row = surface.add(row_index * COPY_RECT_256X2_SURFACE_WIDTH as usize);
        for (index, value) in values.iter_mut().enumerate() {
            *value = core::ptr::read_volatile(row.add(start_pixel + index));
        }
    }
    values
}

fn direct_rcs_copy_rect_256_samples_ok(state: DirectRcsState) -> usize {
    super::dma_flush(state.clear_test_virt, CLEAR_RECT_TEST_BYTES);
    let mut ok = 0usize;
    unsafe {
        let row = state.clear_test_virt as *const u32;
        for sample in COPY_RECT_256_SAMPLE_INDICES {
            let src = core::ptr::read_volatile(row.add(sample));
            let dst = core::ptr::read_volatile(row.add(sample + COPY_RECT_256_WIDTH as usize));
            if src == direct_rcs_copy_rect_256_src_pixel(sample)
                && dst == direct_rcs_copy_rect_256_src_pixel(sample)
            {
                ok += 1;
            }
        }
    }
    ok
}

fn direct_rcs_copy_rect_256_full_counts(state: DirectRcsState) -> (usize, usize) {
    super::dma_flush(state.clear_test_virt, CLEAR_RECT_TEST_BYTES);
    let mut src_preserved = 0usize;
    let mut copied = 0usize;
    unsafe {
        let row = state.clear_test_virt as *const u32;
        for index in 0..COPY_RECT_256_WIDTH as usize {
            let expected = direct_rcs_copy_rect_256_src_pixel(index);
            let src = core::ptr::read_volatile(row.add(index));
            let dst = core::ptr::read_volatile(row.add(index + COPY_RECT_256_WIDTH as usize));
            if src == expected {
                src_preserved += 1;
            }
            if dst == expected {
                copied += 1;
            }
        }
    }
    (src_preserved, copied)
}

fn direct_rcs_copy_rect_256x2_samples_ok(state: DirectRcsState) -> usize {
    super::dma_flush(state.clear_test_virt, CLEAR_RECT_TEST_BYTES);
    let mut ok = 0usize;
    unsafe {
        let surface = state.clear_test_virt as *const u32;
        let pitch_pixels = COPY_RECT_256X2_SURFACE_WIDTH as usize;
        for (x, y) in COPY_RECT_256X2_SAMPLE_POINTS {
            let row = surface.add(y * pitch_pixels);
            let expected = direct_rcs_copy_rect_256x2_src_pixel(x, y);
            let src = core::ptr::read_volatile(row.add(x));
            let dst = core::ptr::read_volatile(row.add(x + COPY_RECT_256X2_WIDTH as usize));
            if src == expected && dst == expected {
                ok += 1;
            }
        }
    }
    ok
}

fn direct_rcs_copy_rect_256x2_full_counts(state: DirectRcsState) -> (usize, usize) {
    super::dma_flush(state.clear_test_virt, CLEAR_RECT_TEST_BYTES);
    let mut src_preserved = 0usize;
    let mut copied = 0usize;
    unsafe {
        let surface = state.clear_test_virt as *const u32;
        let pitch_pixels = COPY_RECT_256X2_SURFACE_WIDTH as usize;
        for y in 0..COPY_RECT_256X2_HEIGHT as usize {
            let row = surface.add(y * pitch_pixels);
            for x in 0..COPY_RECT_256X2_WIDTH as usize {
                let expected = direct_rcs_copy_rect_256x2_src_pixel(x, y);
                let src = core::ptr::read_volatile(row.add(x));
                let dst = core::ptr::read_volatile(row.add(x + COPY_RECT_256X2_WIDTH as usize));
                if src == expected {
                    src_preserved += 1;
                }
                if dst == expected {
                    copied += 1;
                }
            }
        }
    }
    (src_preserved, copied)
}

fn direct_rcs_count_white(values: [u32; 4]) -> usize {
    let mut count = 0usize;
    for value in values {
        if value == CLEAR_RECT_EXPECTED_WHITE {
            count += 1;
        }
    }
    count
}

fn direct_rcs_count_matching(values: [u32; 4], expected: [u32; 4]) -> usize {
    let mut count = 0usize;
    for index in 0..values.len() {
        if values[index] == expected[index] {
            count += 1;
        }
    }
    count
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
    direct_rcs_push_pipe_control_full(batch, cursor, 0, flags)
}

fn direct_rcs_push_store_marker(
    batch: &mut [u32],
    cursor: &mut usize,
    slot: usize,
    value: u32,
) -> bool {
    let dst = DIRECT_RCS_GPU_VA_RESULT_BASE + (slot as u64) * core::mem::size_of::<u32>() as u64;
    direct_rcs_push(batch, cursor, MI_STORE_DATA_IMM_GGTT_DW1)
        && direct_rcs_push(batch, cursor, dst as u32)
        && direct_rcs_push(batch, cursor, (dst >> 32) as u32)
        && direct_rcs_push(batch, cursor, value)
}

fn direct_rcs_push_copy_rect_walker(
    batch: &mut [u32],
    cursor: &mut usize,
    payload_offset: usize,
    right_mask: u32,
) -> bool {
    direct_rcs_push(batch, cursor, GPGPU_WALKER_CMD)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, COPY_RECT_INDIRECT_BYTES as u32)
        && direct_rcs_push(batch, cursor, payload_offset as u32)
        && direct_rcs_push(
            batch,
            cursor,
            (GPGPU_WALKER_SIMD16_SELECT << 30) | (GPGPU_WALKER_GROUP_THREADS - 1),
        )
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 1)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 1)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_GROUP_Z_DIM)
        && direct_rcs_push(batch, cursor, right_mask)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_BOTTOM_MASK)
}

fn direct_rcs_push_glyph_mask_walker(
    batch: &mut [u32],
    cursor: &mut usize,
    payload_offset: usize,
    right_mask: u32,
) -> bool {
    direct_rcs_push(batch, cursor, GPGPU_WALKER_CMD)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, GLYPH_MASK_INDIRECT_BYTES as u32)
        && direct_rcs_push(batch, cursor, payload_offset as u32)
        && direct_rcs_push(
            batch,
            cursor,
            (GPGPU_WALKER_SIMD16_SELECT << 30) | (GPGPU_WALKER_GROUP_THREADS - 1),
        )
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 1)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 1)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_GROUP_Z_DIM)
        && direct_rcs_push(batch, cursor, right_mask)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_BOTTOM_MASK)
}

fn direct_rcs_push_sprite64_worklist_walker(
    batch: &mut [u32],
    cursor: &mut usize,
    payload_offset: usize,
    right_mask: u32,
) -> bool {
    direct_rcs_push(batch, cursor, GPGPU_WALKER_CMD)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, SPRITE64_WORKLIST_INDIRECT_BYTES as u32)
        && direct_rcs_push(batch, cursor, payload_offset as u32)
        && direct_rcs_push(
            batch,
            cursor,
            (GPGPU_WALKER_SIMD16_SELECT << 30) | (GPGPU_WALKER_GROUP_THREADS - 1),
        )
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 1)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 1)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_GROUP_Z_DIM)
        && direct_rcs_push(batch, cursor, right_mask)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_BOTTOM_MASK)
}

fn direct_rcs_push_rect_worklist_walker(
    batch: &mut [u32],
    cursor: &mut usize,
    payload_offset: usize,
    right_mask: u32,
) -> bool {
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
        && direct_rcs_push(batch, cursor, 1)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 1)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_GROUP_Z_DIM)
        && direct_rcs_push(batch, cursor, right_mask)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_BOTTOM_MASK)
}

fn direct_rcs_push_canvas3d_project_walker(batch: &mut [u32], cursor: &mut usize) -> bool {
    direct_rcs_push(batch, cursor, GPGPU_WALKER_CMD)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, CANVAS3D_PROJECT_INDIRECT_BYTES as u32)
        && direct_rcs_push(batch, cursor, CANVAS3D_PROJECT_PAYLOAD_OFFSET_BYTES as u32)
        && direct_rcs_push(
            batch,
            cursor,
            (GPGPU_WALKER_SIMD16_SELECT << 30) | (GPGPU_WALKER_GROUP_THREADS - 1),
        )
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 1)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 1)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_GROUP_Z_DIM)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_SIMD16_MASK)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_BOTTOM_MASK)
}

fn direct_rcs_push_canvas3d_transform_fused_walker(batch: &mut [u32], cursor: &mut usize) -> bool {
    direct_rcs_push(batch, cursor, GPGPU_WALKER_CMD)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, CANVAS3D_TRANSFORM_FUSED_INDIRECT_BYTES as u32)
        && direct_rcs_push(batch, cursor, CANVAS3D_TRANSFORM_PAYLOAD_OFFSET_BYTES as u32)
        && direct_rcs_push(
            batch,
            cursor,
            (GPGPU_WALKER_SIMD16_SELECT << 30) | (GPGPU_WALKER_GROUP_THREADS - 1),
        )
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 1)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 1)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_GROUP_Z_DIM)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_SIMD16_MASK)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_BOTTOM_MASK)
}

fn direct_rcs_push_canvas3d_clip_box_walker(batch: &mut [u32], cursor: &mut usize) -> bool {
    direct_rcs_push(batch, cursor, GPGPU_WALKER_CMD)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, CANVAS3D_CLIP_BOX_INDIRECT_BYTES as u32)
        && direct_rcs_push(batch, cursor, CANVAS3D_CLIP_BOX_PAYLOAD_OFFSET_BYTES as u32)
        && direct_rcs_push(
            batch,
            cursor,
            (GPGPU_WALKER_SIMD16_SELECT << 30) | (GPGPU_WALKER_GROUP_THREADS - 1),
        )
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 1)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 1)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_GROUP_Z_DIM)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_SIMD16_MASK)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_BOTTOM_MASK)
}

fn direct_rcs_push_canvas3d_plane_sample_walker(batch: &mut [u32], cursor: &mut usize) -> bool {
    direct_rcs_push(batch, cursor, GPGPU_WALKER_CMD)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, CANVAS3D_PLANE_SAMPLE_INDIRECT_BYTES as u32)
        && direct_rcs_push(batch, cursor, CANVAS3D_PLANE_SAMPLE_PAYLOAD_OFFSET_BYTES as u32)
        && direct_rcs_push(
            batch,
            cursor,
            (GPGPU_WALKER_SIMD16_SELECT << 30) | (GPGPU_WALKER_GROUP_THREADS - 1),
        )
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 1)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 1)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_GROUP_Z_DIM)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_SIMD16_MASK)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_BOTTOM_MASK)
}

fn direct_rcs_push_canvas3d_plane_fill_walker(batch: &mut [u32], cursor: &mut usize) -> bool {
    direct_rcs_push(batch, cursor, GPGPU_WALKER_CMD)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, CANVAS3D_PLANE_FILL_INDIRECT_BYTES as u32)
        && direct_rcs_push(batch, cursor, CANVAS3D_PLANE_FILL_PAYLOAD_OFFSET_BYTES as u32)
        && direct_rcs_push(
            batch,
            cursor,
            (GPGPU_WALKER_SIMD16_SELECT << 30) | (GPGPU_WALKER_GROUP_THREADS - 1),
        )
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 1)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 1)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_GROUP_Z_DIM)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_SIMD16_MASK)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_BOTTOM_MASK)
}

fn direct_rcs_push_canvas3d_plane_patch_worklist_walker(
    batch: &mut [u32],
    cursor: &mut usize,
    payload_offset: usize,
) -> bool {
    direct_rcs_push(batch, cursor, GPGPU_WALKER_CMD)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, CANVAS3D_PLANE_PATCH_WORKLIST_INDIRECT_BYTES as u32)
        && direct_rcs_push(batch, cursor, payload_offset as u32)
        && direct_rcs_push(
            batch,
            cursor,
            (GPGPU_WALKER_SIMD16_SELECT << 30) | (GPGPU_WALKER_GROUP_THREADS - 1),
        )
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 1)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 1)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_GROUP_Z_DIM)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_SIMD16_MASK)
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
    let ring_tail_bytes = direct_rcs_build_ring_batch_start(state, DIRECT_RCS_GPU_VA_BATCH_BASE);
    let Some(ring_ctl) = direct_rcs_ring_ctl_value(DIRECT_RCS_RING_BYTES) else {
        return false;
    };
    if !direct_rcs_init_lrc_context_image(
        state,
        DIRECT_RCS_GPU_VA_RING_BASE as u32,
        ring_tail_bytes as u32,
        ring_ctl,
    ) {
        return false;
    }
    let (context_desc_lo, context_desc_hi) =
        direct_rcs_context_descriptor(DIRECT_RCS_GPU_VA_CONTEXT_BASE);
    direct_rcs_write_lrc_ring_tail(state, ring_tail_bytes as u32);
    let pphwsp_gpu = (DIRECT_RCS_GPU_VA_CONTEXT_BASE & !0xFFF) as u32;

    super::mmio_write(dev, GEN12_RCU_MODE, direct_rcs_masked_bit_enable(GEN12_RCU_MODE_CCS_ENABLE));
    super::mmio_write(
        dev,
        RCS_RING_MODE_GEN7,
        direct_rcs_masked_bit_enable(GFX_RUN_LIST_ENABLE | GEN11_GFX_DISABLE_LEGACY_MODE),
    );
    let ctx_ctl = direct_rcs_ctx_control_value(false);
    super::mmio_write(dev, RCS_RING_CONTEXT_CONTROL, ctx_ctl);
    super::mmio_write(dev, RCS_RING_CONTEXT_CONTROL_REF, ctx_ctl);
    super::mmio_write(dev, RCS_RING_MI_MODE, direct_rcs_masked_bit_disable(RING_MI_MODE_STOP_RING));
    super::mmio_write(dev, RCS_RING_HWS_PGA, pphwsp_gpu);
    super::ggtt_invalidate(dev);
    core::sync::atomic::fence(Ordering::SeqCst);

    direct_rcs_execlist_submit_port_push(dev, context_desc_lo, context_desc_hi, 0, 0);
    super::mmio_write(dev, RCS_RING_EXECLIST_CONTROL, EL_CTRL_LOAD);
    super::mmio_write(dev, RCS_RING_TAIL, ring_tail_bytes as u32);
    true
}

fn direct_rcs_build_ring_batch_start(state: DirectRcsState, batch_gpu_addr: u64) -> usize {
    let start = 0usize;
    unsafe {
        let dwords = state.ring_virt as *mut u32;
        core::ptr::write_volatile(dwords.add(start), MI_BATCH_BUFFER_START_GEN8 | MI_BATCH_GTT);
        core::ptr::write_volatile(dwords.add(start + 1), batch_gpu_addr as u32);
        core::ptr::write_volatile(dwords.add(start + 2), (batch_gpu_addr >> 32) as u32);
        core::ptr::write_volatile(dwords.add(start + 3), MI_NOOP);
    }
    let tail_bytes = (start + DIRECT_RCS_BATCH_START_DWORDS) * core::mem::size_of::<u32>();
    super::dma_flush(state.ring_virt, DIRECT_RCS_RING_BYTES);
    tail_bytes
}

fn direct_rcs_poll_result(state: DirectRcsState, expected: u32) -> u32 {
    let mut observed = 0;
    for _ in 0..DIRECT_RCS_SMOKE_POLL_ITERS {
        super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
        observed = unsafe { core::ptr::read_volatile(state.result_virt as *const u32) };
        if observed == expected {
            break;
        }
        core::hint::spin_loop();
    }
    observed
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
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    unsafe { core::ptr::read_volatile(state.result_virt.add(offset) as *const u32) }
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

fn direct_rcs_ticks_from_us(us: u64) -> u64 {
    let hz = embassy_time_driver::TICK_HZ;
    if hz == 0 {
        return us.max(1);
    }
    let ticks = ((us as u128)
        .saturating_mul(hz as u128)
        .saturating_add(999_999)
        / 1_000_000) as u64;
    if us == 0 { 0 } else { ticks.max(1) }
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

fn direct_rcs_context_descriptor(context_gpu_addr: u64) -> (u32, u32) {
    let base = (context_gpu_addr as u32) & 0xFFFF_F000;
    let desc = base
        | GEN8_CTX_VALID
        | GEN8_CTX_PPGTT_ENABLE
        | CTX_DESC_FORCE_RESTORE
        | GEN8_CTX_PRIVILEGE
        | GEN12_CTX_PRIORITY_NORMAL
        | (INTEL_LEGACY_64B_CONTEXT << GEN8_CTX_ADDRESSING_MODE_SHIFT);
    let submit_id = DIRECT_RCS_SUBMIT_COUNTER.fetch_add(1, Ordering::Relaxed);
    let base_context_id = (((context_gpu_addr >> 12) as u32) & 0x3FF).max(1);
    let sw_context_id = (((submit_id & 0x3FF) << 1) ^ base_context_id).max(1) & 0x7FF;
    let desc_hi = ((context_gpu_addr >> 32) as u32) | (sw_context_id << 7);
    (desc, desc_hi)
}

fn direct_rcs_execlist_submit_port_push(
    dev: super::Dev,
    context0_lo: u32,
    context0_hi: u32,
    context1_lo: u32,
    context1_hi: u32,
) {
    super::mmio_write(dev, RCS_RING_EXECLIST_SQ_LO, context0_lo);
    super::mmio_write(dev, RCS_RING_EXECLIST_SQ_HI, context0_hi);
    super::mmio_write(dev, RCS_RING_EXECLIST_SQ_LO + 8, context1_lo);
    super::mmio_write(dev, RCS_RING_EXECLIST_SQ_HI + 8, context1_hi);
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
