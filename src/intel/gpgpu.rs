use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use spin::{Mutex, Once};
mod test_gpgpu;

pub(crate) use test_gpgpu::{
    GpgpuShellCube20ProjectResult, shell_cube20_project_spin, submit_canvas3d_clip_box_q16_once,
    submit_canvas3d_project_once, submit_canvas3d_transform_smoke_once,
};

pub(crate) const COPY_RECT_RGBA8_KERNEL_NAME: &str = "copy_rect_rgba8";
pub(crate) const COPY_RECT_RGBA8_OPENCL_SOURCE: &str = include_str!("kernels/copy_rect_rgba8.cl");
pub(crate) const COPY_RECT_RGBA8_WIDE_KERNEL_NAME: &str = "copy_rect_rgba8_wide";
pub(crate) const COPY_RECT_RGBA8_WIDE_OPENCL_SOURCE: &str =
    include_str!("kernels/copy_rect_rgba8_wide.cl");
pub(crate) const CLEAR_RECT_RGBA8_WHITE_KERNEL_NAME: &str = "clear_rect_rgba8_white";
pub(crate) const CLEAR_RECT_RGBA8_WHITE_OPENCL_SOURCE: &str =
    include_str!("kernels/clear_rect_rgba8_white.cl");
pub(crate) const EMPTY_EOT_KERNEL_NAME: &str = "empty_eot";
pub(crate) const EMPTY_EOT_OPENCL_SOURCE: &str = include_str!("kernels/empty_eot.cl");
pub(crate) const FILL_RECT_RGBA8_KERNEL_NAME: &str = "fill_rect_rgba8";
pub(crate) const FILL_RECT_RGBA8_OPENCL_SOURCE: &str = include_str!("kernels/fill_rect_rgba8.cl");
pub(crate) const FILL_CIRCLE_RGBA8_KERNEL_NAME: &str = "fill_circle_rgba8";
pub(crate) const FILL_CIRCLE_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/fill_circle_rgba8.cl");
pub(crate) const BLIT_RGBA8_NEAREST_KERNEL_NAME: &str = "blit_rgba8_nearest";
pub(crate) const BLIT_RGBA8_NEAREST_OPENCL_SOURCE: &str =
    include_str!("kernels/blit_rgba8_nearest.cl");
pub(crate) const ALPHA_BLEND_RGBA8_OVER_KERNEL_NAME: &str = "alpha_blend_rgba8_over";
pub(crate) const ALPHA_BLEND_RGBA8_OVER_OPENCL_SOURCE: &str =
    include_str!("kernels/alpha_blend_rgba8_over.cl");
pub(crate) const GLYPH_MASK_RGBA8_KERNEL_NAME: &str = "glyph_mask_rgba8";
pub(crate) const GLYPH_MASK_RGBA8_OPENCL_SOURCE: &str = include_str!("kernels/glyph_mask_rgba8.cl");
pub(crate) const STAMP_MANDEL_RGBA8_KERNEL_NAME: &str = "stamp_mandel_rgba8";
pub(crate) const STAMP_MANDEL_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/stamp_mandel_rgba8.cl");
pub(crate) const SPRITE64_WORKLIST_RGBA8_KERNEL_NAME: &str = "sprite64_worklist_rgba8";
pub(crate) const SPRITE64_WORKLIST_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/sprite64_worklist_rgba8.cl");
pub(crate) const CANVAS3D_PROJECT_RGBA8_KERNEL_NAME: &str = "canvas3d_project_rgba8";
pub(crate) const CANVAS3D_PROJECT_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/canvas3d_project_rgba8.cl");
pub(crate) const CANVAS3D_TRANSLATE_Q16_KERNEL_NAME: &str = "canvas3d_translate_q16";
pub(crate) const CANVAS3D_TRANSLATE_Q16_OPENCL_SOURCE: &str =
    include_str!("kernels/canvas3d_translate_q16.cl");
pub(crate) const CANVAS3D_SCALE_Q16_KERNEL_NAME: &str = "canvas3d_scale_q16";
pub(crate) const CANVAS3D_SCALE_Q16_OPENCL_SOURCE: &str =
    include_str!("kernels/canvas3d_scale_q16.cl");
pub(crate) const CANVAS3D_ROTATE_QUAT_Q16_KERNEL_NAME: &str = "canvas3d_rotate_quat_q16";
pub(crate) const CANVAS3D_ROTATE_QUAT_Q16_OPENCL_SOURCE: &str =
    include_str!("kernels/canvas3d_rotate_quat_q16.cl");
pub(crate) const CANVAS3D_TRANSFORM_Q16_KERNEL_NAME: &str = "canvas3d_transform_q16";
pub(crate) const CANVAS3D_TRANSFORM_Q16_OPENCL_SOURCE: &str =
    include_str!("kernels/canvas3d_transform_q16.cl");
pub(crate) const CANVAS3D_CLIP_BOX_Q16_KERNEL_NAME: &str = "canvas3d_clip_box_q16";
pub(crate) const CANVAS3D_CLIP_BOX_Q16_OPENCL_SOURCE: &str =
    include_str!("kernels/canvas3d_clip_box_q16.cl");
pub(crate) const COPY_RECT_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/copy_rect_rgba8.bin");
pub(crate) const COPY_RECT_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/copy_rect_rgba8.spv");
pub(crate) const COPY_RECT_RGBA8_WIDE_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/copy_rect_rgba8_wide.bin");
pub(crate) const COPY_RECT_RGBA8_WIDE_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/copy_rect_rgba8_wide.spv");
pub(crate) const CLEAR_RECT_RGBA8_WHITE_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/clear_rect_rgba8_white.bin");
pub(crate) const CLEAR_RECT_RGBA8_WHITE_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/clear_rect_rgba8_white.spv");
pub(crate) const EMPTY_EOT_ADLS_BIN: &[u8] = include_bytes!("kernels/artifacts/adls/empty_eot.bin");
pub(crate) const EMPTY_EOT_ADLS_SPV: &[u8] = include_bytes!("kernels/artifacts/adls/empty_eot.spv");
pub(crate) const FILL_RECT_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/fill_rect_rgba8.bin");
pub(crate) const FILL_RECT_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/fill_rect_rgba8.spv");
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
pub(crate) const GLYPH_MASK_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/glyph_mask_rgba8.bin");
pub(crate) const GLYPH_MASK_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/glyph_mask_rgba8.spv");
pub(crate) const STAMP_MANDEL_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/stamp_mandel_rgba8.bin");
pub(crate) const STAMP_MANDEL_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/stamp_mandel_rgba8.spv");
pub(crate) const SPRITE64_WORKLIST_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/sprite64_worklist_rgba8.bin");
pub(crate) const SPRITE64_WORKLIST_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/sprite64_worklist_rgba8.spv");
pub(crate) const CANVAS3D_PROJECT_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/canvas3d_project_rgba8.bin");
pub(crate) const CANVAS3D_PROJECT_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/canvas3d_project_rgba8.spv");
pub(crate) const CANVAS3D_TRANSLATE_Q16_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/canvas3d_translate_q16.bin");
pub(crate) const CANVAS3D_TRANSLATE_Q16_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/canvas3d_translate_q16.spv");
pub(crate) const CANVAS3D_SCALE_Q16_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/canvas3d_scale_q16.bin");
pub(crate) const CANVAS3D_SCALE_Q16_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/canvas3d_scale_q16.spv");
pub(crate) const CANVAS3D_ROTATE_QUAT_Q16_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/canvas3d_rotate_quat_q16.bin");
pub(crate) const CANVAS3D_ROTATE_QUAT_Q16_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/canvas3d_rotate_quat_q16.spv");
pub(crate) const CANVAS3D_TRANSFORM_Q16_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/canvas3d_transform_q16.bin");
pub(crate) const CANVAS3D_TRANSFORM_Q16_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/canvas3d_transform_q16.spv");
pub(crate) const CANVAS3D_CLIP_BOX_Q16_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/canvas3d_clip_box_q16.bin");
pub(crate) const CANVAS3D_CLIP_BOX_Q16_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/canvas3d_clip_box_q16.spv");
pub(crate) const COPY_RECT_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0x10, 0x86, 0x60, 0x24, 0xAA, 0xFF, 0xAE, 0x96, 0xF9, 0x2C, 0xFC, 0x25, 0xA5, 0xFB, 0x18, 0x8C,
    0xA4, 0x21, 0x99, 0x47, 0x89, 0xAF, 0xBC, 0x4D, 0xBA, 0x3D, 0xDC, 0x29, 0x0B, 0xD5, 0x83, 0xAB,
];
pub(crate) const COPY_RECT_RGBA8_WIDE_ADLS_BIN_SHA256: [u8; 32] = [
    0xC9, 0x48, 0x53, 0x56, 0x0F, 0xDC, 0xAD, 0x31, 0x70, 0x3B, 0x8D, 0x55, 0x6F, 0x30, 0x3D, 0xF1,
    0x92, 0x2E, 0xC6, 0x45, 0xC2, 0x36, 0xB5, 0x51, 0x13, 0xA0, 0x8B, 0x1A, 0xC3, 0x67, 0xBA, 0xDD,
];
pub(crate) const CLEAR_RECT_RGBA8_WHITE_ADLS_BIN_SHA256: [u8; 32] = [
    0x96, 0xB9, 0x6D, 0x64, 0x58, 0xBA, 0xDA, 0x38, 0x28, 0xB5, 0xE0, 0x5D, 0xD0, 0xD4, 0xDE, 0xCE,
    0xD6, 0x11, 0x93, 0xCE, 0x33, 0x6F, 0xEC, 0x2F, 0x7C, 0xE0, 0xBC, 0xF0, 0xDF, 0x4D, 0x57, 0xC0,
];
pub(crate) const EMPTY_EOT_ADLS_BIN_SHA256: [u8; 32] = [
    0x72, 0x73, 0x17, 0x3D, 0xC0, 0xE3, 0xDE, 0x30, 0xED, 0x9B, 0xFA, 0x28, 0xC9, 0x03, 0xD6, 0xDB,
    0xAF, 0x49, 0x42, 0xF2, 0xF1, 0xAD, 0x1F, 0x20, 0xCC, 0xA3, 0x19, 0xCB, 0xFD, 0xD1, 0x4E, 0xAC,
];
pub(crate) const FILL_RECT_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0xAB, 0x51, 0x9A, 0x0E, 0x4E, 0x47, 0x31, 0xE5, 0x8F, 0xF6, 0x5D, 0x75, 0xBF, 0x92, 0x93, 0x4C,
    0xD7, 0x31, 0xA0, 0x88, 0x23, 0xB0, 0x40, 0x28, 0x62, 0x0E, 0x86, 0x54, 0x9F, 0x45, 0x06, 0xF4,
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
pub(crate) const GLYPH_MASK_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0x90, 0x8D, 0xF0, 0x7D, 0x62, 0xB0, 0x69, 0xF3, 0x1A, 0x04, 0x6D, 0x29, 0x02, 0xDF, 0xF9, 0xA0,
    0xFA, 0x33, 0xE4, 0x9A, 0x1C, 0x25, 0x3B, 0x74, 0xA4, 0xE7, 0xCC, 0x18, 0xDF, 0x66, 0xD3, 0x78,
];
pub(crate) const STAMP_MANDEL_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0x1E, 0x6F, 0xB6, 0xC8, 0x84, 0xCC, 0xB4, 0x19, 0x62, 0x32, 0x48, 0x1F, 0xC0, 0x95, 0xEC, 0xB5,
    0xC9, 0xCC, 0x95, 0xF7, 0x3D, 0xD6, 0x7B, 0x93, 0xF0, 0xC5, 0x9D, 0xEA, 0xC7, 0xAF, 0xF1, 0xE1,
];
pub(crate) const SPRITE64_WORKLIST_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0xF2, 0x43, 0x59, 0x84, 0x03, 0x79, 0x48, 0x82, 0x10, 0x4E, 0x77, 0xEB, 0x0F, 0x3E, 0x14, 0xB4,
    0xF1, 0x2C, 0x10, 0xBF, 0x8B, 0xD0, 0xB6, 0x8A, 0xB5, 0xFD, 0xED, 0x63, 0x04, 0x86, 0x87, 0x67,
];
pub(crate) const CANVAS3D_PROJECT_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0xDA, 0xF0, 0x15, 0xA0, 0xB9, 0x8A, 0x45, 0xF7, 0x02, 0xD5, 0xD7, 0x87, 0xCA, 0x19, 0x59, 0xBA,
    0xAC, 0x7C, 0x02, 0xFE, 0x97, 0x93, 0xAC, 0x6E, 0x48, 0xA7, 0x87, 0x18, 0xAE, 0x3D, 0x3E, 0xB6,
];
pub(crate) const CANVAS3D_TRANSLATE_Q16_ADLS_BIN_SHA256: [u8; 32] = [
    0x1C, 0x3A, 0xF3, 0x11, 0xC1, 0xEE, 0x40, 0xC1, 0xAD, 0x34, 0x47, 0xFD, 0x9E, 0xE7, 0x1C, 0xA7,
    0x7C, 0x5D, 0x47, 0x74, 0xA2, 0x00, 0x2D, 0x74, 0xEA, 0xD9, 0xBB, 0x50, 0x05, 0x6A, 0xEE, 0xB0,
];
pub(crate) const CANVAS3D_SCALE_Q16_ADLS_BIN_SHA256: [u8; 32] = [
    0x43, 0xBF, 0xA5, 0x1A, 0x06, 0x84, 0xD4, 0xF6, 0xA0, 0x07, 0x6A, 0x10, 0x70, 0x5C, 0xB3, 0x50,
    0xC9, 0x72, 0x00, 0x29, 0xC4, 0x61, 0x6F, 0x6C, 0xE5, 0xC4, 0x49, 0x84, 0xB2, 0x8F, 0xFB, 0x67,
];
pub(crate) const CANVAS3D_ROTATE_QUAT_Q16_ADLS_BIN_SHA256: [u8; 32] = [
    0xCA, 0xCF, 0x30, 0x74, 0x80, 0x3E, 0x8A, 0x40, 0xB7, 0x7A, 0xCE, 0x13, 0x23, 0x42, 0x47, 0x75,
    0x9E, 0x6A, 0x8E, 0xDD, 0x27, 0xF5, 0x69, 0x88, 0x89, 0x08, 0x21, 0x43, 0x9F, 0xB5, 0x04, 0x50,
];
pub(crate) const CANVAS3D_TRANSFORM_Q16_ADLS_BIN_SHA256: [u8; 32] = [
    0x2C, 0x94, 0x28, 0x73, 0xA2, 0xB5, 0x4C, 0xA2, 0xBB, 0xBD, 0x17, 0xDA, 0x25, 0xFD, 0x1D, 0x22,
    0x0E, 0x86, 0x34, 0x87, 0xAE, 0xD5, 0x9A, 0xE2, 0xA5, 0xE4, 0xF3, 0x0D, 0x41, 0x8F, 0x1D, 0x4D,
];
pub(crate) const CANVAS3D_CLIP_BOX_Q16_ADLS_BIN_SHA256: [u8; 32] = [
    0x7E, 0x28, 0xD6, 0xB4, 0xF7, 0xF3, 0x7C, 0x95, 0x37, 0x4C, 0x27, 0x4B, 0x37, 0x02, 0x81, 0x30,
    0x11, 0x61, 0xED, 0xF7, 0xD4, 0xA7, 0x17, 0x51, 0x86, 0x8F, 0x9A, 0x2B, 0x56, 0x59, 0xEA, 0x5F,
];

const COPY_RECT_RGBA8_ADLS_GPU: u64 = 0x0D20_0000;
const COPY_RECT_RGBA8_WIDE_ADLS_GPU: u64 = 0x0D23_0000;
const CLEAR_RECT_RGBA8_WHITE_ADLS_GPU: u64 = 0x0D21_0000;
const EMPTY_EOT_ADLS_GPU: u64 = 0x0D22_0000;
const SPRITE64_WORKLIST_RGBA8_ADLS_GPU: u64 = 0x0D24_0000;
const CANVAS3D_PROJECT_RGBA8_ADLS_GPU: u64 = 0x0D25_0000;
const CANVAS3D_TRANSLATE_Q16_ADLS_GPU: u64 = 0x0D26_0000;
const CANVAS3D_SCALE_Q16_ADLS_GPU: u64 = 0x0D27_0000;
const CANVAS3D_ROTATE_QUAT_Q16_ADLS_GPU: u64 = 0x0D28_0000;
const CANVAS3D_CLIP_BOX_Q16_ADLS_GPU: u64 = 0x0D29_0000;
const CANVAS3D_TRANSFORM_Q16_ADLS_GPU: u64 = 0x0D2A_0000;
const COPY_RECT_RGBA8_TEXT_OFFSET_BYTES: u64 = 0x40;
const CLEAR_RECT_RGBA8_WHITE_TEXT_OFFSET_BYTES: u64 = 0x40;
const EMPTY_EOT_TEXT_OFFSET_BYTES: u64 = 0x40;
const SPRITE64_WORKLIST_RGBA8_TEXT_OFFSET_BYTES: u64 = 0x40;
const CANVAS3D_PROJECT_RGBA8_TEXT_OFFSET_BYTES: u64 = 0x40;
const CANVAS3D_TRANSLATE_Q16_TEXT_OFFSET_BYTES: u64 = 0x40;
const CANVAS3D_SCALE_Q16_TEXT_OFFSET_BYTES: u64 = 0x40;
const CANVAS3D_ROTATE_QUAT_Q16_TEXT_OFFSET_BYTES: u64 = 0x40;
const CANVAS3D_TRANSFORM_Q16_TEXT_OFFSET_BYTES: u64 = 0x40;
const CANVAS3D_CLIP_BOX_Q16_TEXT_OFFSET_BYTES: u64 = 0x40;

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
const COPY_RECT_WIDE_PIXELS_PER_LANE: u32 = 4;
const COPY_RECT_WIDE_SPAN_PIXELS: u32 = 16 * COPY_RECT_WIDE_PIXELS_PER_LANE;
const COPY_RECT_WIDE_ROWS_PER_WALKER: u32 = 64;
const COPY_RECT_BATCH_MAX_SPANS: usize = 32;
const COPY_RECT_IDD_BYTES: usize = 8 * core::mem::size_of::<u32>();
const COPY_RECT_SURFACE_STATE_DWORDS: usize = 16;
const COPY_RECT_CROSS_THREAD_BYTES: usize = 96;
const COPY_RECT_PER_THREAD_BYTES: usize = 96;
const COPY_RECT_INDIRECT_BYTES: usize = COPY_RECT_CROSS_THREAD_BYTES + COPY_RECT_PER_THREAD_BYTES;
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
const CANVAS3D_PROJECT_VERTEX_COUNT: usize = 64;
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
const CANVAS3D_TRANSFORM_CROSS_THREAD_BYTES: usize = 96;
const CANVAS3D_TRANSFORM_PER_THREAD_BYTES: usize = 96;
const CANVAS3D_TRANSFORM_INDIRECT_BYTES: usize =
    CANVAS3D_TRANSFORM_CROSS_THREAD_BYTES + CANVAS3D_TRANSFORM_PER_THREAD_BYTES;
const CANVAS3D_TRANSFORM_FUSED_CROSS_THREAD_BYTES: usize = 128;
const CANVAS3D_TRANSFORM_FUSED_PER_THREAD_BYTES: usize = 96;
const CANVAS3D_TRANSFORM_FUSED_INDIRECT_BYTES: usize =
    CANVAS3D_TRANSFORM_FUSED_CROSS_THREAD_BYTES + CANVAS3D_TRANSFORM_FUSED_PER_THREAD_BYTES;
const CANVAS3D_TRANSFORM_PRE_MARKER_SLOT: usize = 11;
const CANVAS3D_TRANSFORM_POST_MARKER_SLOT: usize = 10;
const CANVAS3D_TRANSFORM_TRANSLATE_PRE_MARKER: u32 = 0xC0DE_3611;
const CANVAS3D_TRANSFORM_TRANSLATE_POST_MARKER: u32 = 0xC0DE_3612;
const CANVAS3D_TRANSFORM_SCALE_PRE_MARKER: u32 = 0xC0DE_3621;
const CANVAS3D_TRANSFORM_SCALE_POST_MARKER: u32 = 0xC0DE_3622;
const CANVAS3D_TRANSFORM_ROTATE_PRE_MARKER: u32 = 0xC0DE_3631;
const CANVAS3D_TRANSFORM_ROTATE_POST_MARKER: u32 = 0xC0DE_3632;
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
const CUBE20_HALF_Q16: i32 = CANVAS3D_PROJECT_Q16_ONE / 2;
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
const COPY_RECT_WIDE_256X2_EXPECTED_SPANS: usize = 4;
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
const EMPTY_EOT_IDD_OFFSET_BYTES: usize = 0x300;
const EMPTY_EOT_IDD_BYTES: usize = 8 * core::mem::size_of::<u32>();
const EMPTY_EOT_PRE_MARKER_SLOT: usize = 1;
const EMPTY_EOT_POST_MARKER_SLOT: usize = 0;
const EMPTY_EOT_PRE_MARKER: u32 = 0xC0DE_E701;
const EMPTY_EOT_POST_MARKER: u32 = 0xC0DE_E702;
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
const CLEAR_RECT_TEST_WIDTH: u32 = 4;
const CLEAR_RECT_TEST_HEIGHT: u32 = 1;
const CLEAR_RECT_TEST_PIXELS: usize =
    (CLEAR_RECT_TEST_WIDTH as usize) * (CLEAR_RECT_TEST_HEIGHT as usize);
const CLEAR_RECT_TEST_PITCH_BYTES: u32 = CLEAR_RECT_TEST_WIDTH * core::mem::size_of::<u32>() as u32;
const CLEAR_RECT_WALKER_RIGHT_MASK: u32 = 0x0000_000F;
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
static COPY_RECT_RGBA8_WIDE_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static CLEAR_RECT_RGBA8_WHITE_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static EMPTY_EOT_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static SPRITE64_WORKLIST_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static CANVAS3D_PROJECT_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static CANVAS3D_TRANSLATE_Q16_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static CANVAS3D_SCALE_Q16_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static CANVAS3D_ROTATE_QUAT_Q16_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static CANVAS3D_TRANSFORM_Q16_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static CANVAS3D_CLIP_BOX_Q16_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static DIRECT_RCS_STATE: Mutex<Option<DirectRcsState>> = Mutex::new(None);
static GPGPU_SHELL_SURFACE: Mutex<Option<GpgpuShellSurface>> = Mutex::new(None);
static GPGPU_SPRITE64_WORKLIST_ATLAS: Once<Option<GpgpuSprite64WorklistAtlasSurface>> = Once::new();
static GPGPU_SPRITE64_WORKLIST_DESC: Mutex<Option<GpgpuSprite64WorklistDescBuffer>> =
    Mutex::new(None);
static GPGPU_TWEMOJI_ATLAS: Once<Option<GpgpuTwemojiAtlasCache>> = Once::new();
static DIRECT_RCS_SUBMIT_LOCK: Mutex<()> = Mutex::new(());
static DIRECT_RCS_SMOKE_RAN: AtomicBool = AtomicBool::new(false);
static EMPTY_EOT_WALKER_RAN: AtomicBool = AtomicBool::new(false);
static CLEAR_RECT_WALKER_RAN: AtomicBool = AtomicBool::new(false);
static COPY_RECT_WALKER_RAN: AtomicBool = AtomicBool::new(false);
static COPY_RECT_256_RAN: AtomicBool = AtomicBool::new(false);
static COPY_RECT_256X2_RAN: AtomicBool = AtomicBool::new(false);
static COPY_RECT_WIDE_256X2_RAN: AtomicBool = AtomicBool::new(false);
static RECT_API_SMOKE_RAN: AtomicBool = AtomicBool::new(false);
static CANVAS3D_PROJECT_RAN: AtomicBool = AtomicBool::new(false);
static CANVAS3D_TRANSFORM_RAN: AtomicBool = AtomicBool::new(false);
static CANVAS3D_CLIP_BOX_RAN: AtomicBool = AtomicBool::new(false);
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
pub(crate) struct ClearRectRgba8WhiteParams {
    pub(crate) dst_gpu: u64,
    pub(crate) dst_pitch_bytes: u32,
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
pub(crate) struct Canvas3dTransformQ16Params {
    pub(crate) src_gpu: u64,
    pub(crate) dst_gpu: u64,
    pub(crate) src_first_vertex: u32,
    pub(crate) dst_first_vertex: u32,
    pub(crate) vertex_count: u32,
    pub(crate) param_q16: Canvas3dVec3Q16,
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
pub(crate) struct GpgpuCopyRect {
    pub(crate) src: GpgpuRgba8Surface,
    pub(crate) src_rect: GpgpuRect,
    pub(crate) dst: GpgpuRgba8Surface,
    pub(crate) dst_xy: GpgpuPoint,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuGlyphBlit {
    pub(crate) atlas: GpgpuRgba8Surface,
    pub(crate) glyph_rect: GpgpuRect,
    pub(crate) dst: GpgpuRgba8Surface,
    pub(crate) dst_xy: GpgpuPoint,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuSubmitStats {
    pub(crate) spans: usize,
    pub(crate) submits: usize,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuShellClearResult {
    pub(crate) ok: bool,
    pub(crate) rect: GpgpuRect,
    pub(crate) pixels: usize,
    pub(crate) spans: usize,
    pub(crate) expected_spans: usize,
    pub(crate) white: usize,
    pub(crate) before_head: [u32; 4],
    pub(crate) after_head: [u32; 4],
    pub(crate) surface: GpgpuRgba8Surface,
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
pub(crate) struct GpgpuShellAtlasCopyResult {
    pub(crate) ok: bool,
    pub(crate) slot: u16,
    pub(crate) atlas_width: u32,
    pub(crate) atlas_height: u32,
    pub(crate) atlas_src_rect: GpgpuRect,
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
    pub(crate) staged: usize,
    pub(crate) copied: usize,
    pub(crate) src_preserved: usize,
    pub(crate) src_head: [u32; 4],
    pub(crate) dst_head: [u32; 4],
    pub(crate) presented: bool,
    pub(crate) total_ms: u64,
    pub(crate) stage_ms: u64,
    pub(crate) copy_ms: u64,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuShellAtlasHotCopyResult {
    pub(crate) ok: bool,
    pub(crate) slot: u16,
    pub(crate) atlas_src_rect: GpgpuRect,
    pub(crate) dst_xy: GpgpuPoint,
    pub(crate) primary_width: u32,
    pub(crate) primary_height: u32,
    pub(crate) pixels: usize,
    pub(crate) spans: usize,
    pub(crate) expected_spans: usize,
    pub(crate) submits: usize,
    pub(crate) expected_submits: usize,
    pub(crate) staged: usize,
    pub(crate) presented: bool,
    pub(crate) total_ms: u64,
    pub(crate) stage_ms: u64,
    pub(crate) copy_ms: u64,
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

#[derive(Copy, Clone, Debug)]
struct GpgpuShellSurface {
    surface: GpgpuRgba8Surface,
    virt: *mut u8,
}

unsafe impl Send for GpgpuShellSurface {}
unsafe impl Sync for GpgpuShellSurface {}

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

#[derive(Clone, Debug)]
struct GpgpuTwemojiAtlasCache {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    xrgb: Vec<u32>,
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

pub(crate) const COPY_RECT_RGBA8_WIDE_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact {
    name: COPY_RECT_RGBA8_WIDE_KERNEL_NAME,
    target: "adls",
    bin: COPY_RECT_RGBA8_WIDE_ADLS_BIN,
    spv: COPY_RECT_RGBA8_WIDE_ADLS_SPV,
    bin_sha256: COPY_RECT_RGBA8_WIDE_ADLS_BIN_SHA256,
};

pub(crate) const CLEAR_RECT_RGBA8_WHITE_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact {
    name: CLEAR_RECT_RGBA8_WHITE_KERNEL_NAME,
    target: "adls",
    bin: CLEAR_RECT_RGBA8_WHITE_ADLS_BIN,
    spv: CLEAR_RECT_RGBA8_WHITE_ADLS_SPV,
    bin_sha256: CLEAR_RECT_RGBA8_WHITE_ADLS_BIN_SHA256,
};

pub(crate) const EMPTY_EOT_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact {
    name: EMPTY_EOT_KERNEL_NAME,
    target: "adls",
    bin: EMPTY_EOT_ADLS_BIN,
    spv: EMPTY_EOT_ADLS_SPV,
    bin_sha256: EMPTY_EOT_ADLS_BIN_SHA256,
};

pub(crate) const FILL_RECT_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact {
    name: FILL_RECT_RGBA8_KERNEL_NAME,
    target: "adls",
    bin: FILL_RECT_RGBA8_ADLS_BIN,
    spv: FILL_RECT_RGBA8_ADLS_SPV,
    bin_sha256: FILL_RECT_RGBA8_ADLS_BIN_SHA256,
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

pub(crate) const GLYPH_MASK_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact {
    name: GLYPH_MASK_RGBA8_KERNEL_NAME,
    target: "adls",
    bin: GLYPH_MASK_RGBA8_ADLS_BIN,
    spv: GLYPH_MASK_RGBA8_ADLS_SPV,
    bin_sha256: GLYPH_MASK_RGBA8_ADLS_BIN_SHA256,
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

pub(crate) const CANVAS3D_PROJECT_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact {
    name: CANVAS3D_PROJECT_RGBA8_KERNEL_NAME,
    target: "adls",
    bin: CANVAS3D_PROJECT_RGBA8_ADLS_BIN,
    spv: CANVAS3D_PROJECT_RGBA8_ADLS_SPV,
    bin_sha256: CANVAS3D_PROJECT_RGBA8_ADLS_BIN_SHA256,
};

pub(crate) const CANVAS3D_TRANSLATE_Q16_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact {
    name: CANVAS3D_TRANSLATE_Q16_KERNEL_NAME,
    target: "adls",
    bin: CANVAS3D_TRANSLATE_Q16_ADLS_BIN,
    spv: CANVAS3D_TRANSLATE_Q16_ADLS_SPV,
    bin_sha256: CANVAS3D_TRANSLATE_Q16_ADLS_BIN_SHA256,
};

pub(crate) const CANVAS3D_SCALE_Q16_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact {
    name: CANVAS3D_SCALE_Q16_KERNEL_NAME,
    target: "adls",
    bin: CANVAS3D_SCALE_Q16_ADLS_BIN,
    spv: CANVAS3D_SCALE_Q16_ADLS_SPV,
    bin_sha256: CANVAS3D_SCALE_Q16_ADLS_BIN_SHA256,
};

pub(crate) const CANVAS3D_ROTATE_QUAT_Q16_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact {
        name: CANVAS3D_ROTATE_QUAT_Q16_KERNEL_NAME,
        target: "adls",
        bin: CANVAS3D_ROTATE_QUAT_Q16_ADLS_BIN,
        spv: CANVAS3D_ROTATE_QUAT_Q16_ADLS_SPV,
        bin_sha256: CANVAS3D_ROTATE_QUAT_Q16_ADLS_BIN_SHA256,
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

pub(crate) fn copy_rect_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *COPY_RECT_RGBA8_UPLOAD.lock()
}

pub(crate) fn copy_rect_rgba8_wide_upload_status() -> Option<UploadedKernelArtifact> {
    *COPY_RECT_RGBA8_WIDE_UPLOAD.lock()
}

pub(crate) fn clear_rect_rgba8_white_upload_status() -> Option<UploadedKernelArtifact> {
    *CLEAR_RECT_RGBA8_WHITE_UPLOAD.lock()
}

pub(crate) fn empty_eot_upload_status() -> Option<UploadedKernelArtifact> {
    *EMPTY_EOT_UPLOAD.lock()
}

pub(crate) fn sprite64_worklist_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *SPRITE64_WORKLIST_RGBA8_UPLOAD.lock()
}

pub(crate) fn canvas3d_project_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *CANVAS3D_PROJECT_RGBA8_UPLOAD.lock()
}

pub(crate) fn canvas3d_translate_q16_upload_status() -> Option<UploadedKernelArtifact> {
    *CANVAS3D_TRANSLATE_Q16_UPLOAD.lock()
}

pub(crate) fn canvas3d_scale_q16_upload_status() -> Option<UploadedKernelArtifact> {
    *CANVAS3D_SCALE_Q16_UPLOAD.lock()
}

pub(crate) fn canvas3d_rotate_quat_q16_upload_status() -> Option<UploadedKernelArtifact> {
    *CANVAS3D_ROTATE_QUAT_Q16_UPLOAD.lock()
}

pub(crate) fn canvas3d_transform_q16_upload_status() -> Option<UploadedKernelArtifact> {
    *CANVAS3D_TRANSFORM_Q16_UPLOAD.lock()
}

pub(crate) fn canvas3d_clip_box_q16_upload_status() -> Option<UploadedKernelArtifact> {
    *CANVAS3D_CLIP_BOX_Q16_UPLOAD.lock()
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

pub(crate) fn upload_copy_rect_rgba8_wide_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *COPY_RECT_RGBA8_WIDE_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: copy-rect-rgba8-wide upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload =
        upload_artifact(dev, COPY_RECT_RGBA8_WIDE_ADLS_ARTIFACT, COPY_RECT_RGBA8_WIDE_ADLS_GPU)?;
    *COPY_RECT_RGBA8_WIDE_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_clear_rect_rgba8_white_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *CLEAR_RECT_RGBA8_WHITE_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: clear-rect-rgba8-white upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(
        dev,
        CLEAR_RECT_RGBA8_WHITE_ADLS_ARTIFACT,
        CLEAR_RECT_RGBA8_WHITE_ADLS_GPU,
    )?;
    *CLEAR_RECT_RGBA8_WHITE_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_empty_eot_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *EMPTY_EOT_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: empty-eot upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(dev, EMPTY_EOT_ADLS_ARTIFACT, EMPTY_EOT_ADLS_GPU)?;
    *EMPTY_EOT_UPLOAD.lock() = Some(upload);
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

pub(crate) fn upload_canvas3d_translate_q16_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *CANVAS3D_TRANSLATE_Q16_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: canvas3d-translate-q16 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(
        dev,
        CANVAS3D_TRANSLATE_Q16_ADLS_ARTIFACT,
        CANVAS3D_TRANSLATE_Q16_ADLS_GPU,
    )?;
    *CANVAS3D_TRANSLATE_Q16_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_canvas3d_scale_q16_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *CANVAS3D_SCALE_Q16_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: canvas3d-scale-q16 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload =
        upload_artifact(dev, CANVAS3D_SCALE_Q16_ADLS_ARTIFACT, CANVAS3D_SCALE_Q16_ADLS_GPU)?;
    *CANVAS3D_SCALE_Q16_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_canvas3d_rotate_quat_q16_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *CANVAS3D_ROTATE_QUAT_Q16_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: canvas3d-rotate-quat-q16 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(
        dev,
        CANVAS3D_ROTATE_QUAT_Q16_ADLS_ARTIFACT,
        CANVAS3D_ROTATE_QUAT_Q16_ADLS_GPU,
    )?;
    *CANVAS3D_ROTATE_QUAT_Q16_UPLOAD.lock() = Some(upload);
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

fn copy_rect_kernel_flavor_wide() -> Option<CopyRectKernelFlavor> {
    Some(CopyRectKernelFlavor {
        upload: upload_copy_rect_rgba8_wide_kernel()?,
        text_offset_bytes: COPY_RECT_RGBA8_TEXT_OFFSET_BYTES,
        pixels_per_lane: COPY_RECT_WIDE_PIXELS_PER_LANE,
        span_pixels: COPY_RECT_WIDE_SPAN_PIXELS,
        rows_per_walker: COPY_RECT_WIDE_ROWS_PER_WALKER,
        name: COPY_RECT_RGBA8_WIDE_KERNEL_NAME,
    })
}

pub(crate) fn fill_rect_white_rgba8(dst: GpgpuRgba8Surface, rect: GpgpuRect) -> usize {
    let Some(params) = lower_clear_rect_white(dst, rect) else {
        return 0;
    };
    submit_clear_rect_white_spans(dst, params)
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

pub(crate) fn copy_rect_rgba8_wide_stats(
    src: GpgpuRgba8Surface,
    src_rect: GpgpuRect,
    dst: GpgpuRgba8Surface,
    dst_xy: GpgpuPoint,
) -> GpgpuSubmitStats {
    let Some(params) = lower_copy_rect(src, src_rect, dst, dst_xy) else {
        return GpgpuSubmitStats::default();
    };
    let Some(flavor) = copy_rect_kernel_flavor_wide() else {
        return GpgpuSubmitStats::default();
    };
    submit_copy_rect_spans_with_stats(src, dst, params, flavor)
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
    copy_rects_rgba8_stats_with_flavor(copies, false)
}

pub(crate) fn copy_rects_rgba8_wide_stats(copies: &[GpgpuCopyRect]) -> GpgpuSubmitStats {
    copy_rects_rgba8_stats_with_flavor(copies, true)
}

fn copy_rects_rgba8_stats_with_flavor(copies: &[GpgpuCopyRect], wide: bool) -> GpgpuSubmitStats {
    let Some(first) = copies.first().copied() else {
        return GpgpuSubmitStats::default();
    };
    let Some(flavor) = (if wide {
        copy_rect_kernel_flavor_wide()
    } else {
        copy_rect_kernel_flavor_narrow()
    }) else {
        return GpgpuSubmitStats::default();
    };

    let mut params = Vec::with_capacity(copies.len());
    for copy in copies {
        if !same_rgba8_surface(first.src, copy.src) || !same_rgba8_surface(first.dst, copy.dst) {
            return copy_rects_rgba8_serial_stats_with_flavor(copies, wide);
        }
        let Some(copy_params) = lower_copy_rect(copy.src, copy.src_rect, copy.dst, copy.dst_xy)
        else {
            return GpgpuSubmitStats::default();
        };
        params.push(copy_params);
    }
    submit_copy_rect_multi_ops_with_stats(first.src, first.dst, &params, flavor)
}

fn copy_rects_rgba8_serial_stats_with_flavor(
    copies: &[GpgpuCopyRect],
    wide: bool,
) -> GpgpuSubmitStats {
    let mut stats = GpgpuSubmitStats::default();
    for copy in copies {
        let copy_stats = if wide {
            copy_rect_rgba8_wide_stats(copy.src, copy.src_rect, copy.dst, copy.dst_xy)
        } else {
            copy_rect_rgba8_stats(copy.src, copy.src_rect, copy.dst, copy.dst_xy)
        };
        stats.spans = stats.spans.saturating_add(copy_stats.spans);
        stats.submits = stats.submits.saturating_add(copy_stats.submits);
    }
    stats
}

pub(crate) fn shell_clear_white_rgba8(rect: GpgpuRect) -> Option<GpgpuShellClearResult> {
    let shell = shell_surface_once()?;
    if !rect_is_inside(shell.surface, rect) {
        return None;
    }

    shell_zero_surface(shell);
    shell_seed_rect(shell, rect);
    let before_head = shell_read_rect_head(shell, rect);
    super::dma_flush(shell.virt, shell.surface.bytes);

    let spans = fill_rect_white_rgba8(shell.surface, rect);
    super::dma_flush(shell.virt, shell.surface.bytes);

    let after_head = shell_read_rect_head(shell, rect);
    let white = shell_count_white(shell, rect);
    let pixels = rect_pixel_count(rect);
    let expected_spans = clear_rect_expected_spans(rect);
    Some(GpgpuShellClearResult {
        ok: spans == expected_spans && white == pixels,
        rect,
        pixels,
        spans,
        expected_spans,
        white,
        before_head,
        after_head,
        surface: shell.surface,
    })
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

pub(crate) fn shell_copy_twemoji_atlas_slot_scanout(
    slot: u16,
    dst_xy_override: Option<GpgpuPoint>,
) -> Option<GpgpuShellAtlasCopyResult> {
    let total_start_tick = direct_rcs_now_tick();
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
    if target.virt.is_null() {
        return None;
    }

    let atlas = twemoji_atlas_cache_once()?;
    let region = crate::gfx::althlasfont::twemoji::twemoji_lookup_slot_region(slot)?;
    let src_rect = GpgpuRect::new(
        region.src_x as i32,
        region.src_y as i32,
        u32::from(region.src_w.max(1)),
        u32::from(region.src_h.max(1)),
    );
    if !rect_is_inside_atlas(atlas.width, atlas.height, src_rect) {
        return None;
    }
    if src_rect.width > shell.surface.width
        || src_rect.height > shell.surface.height
        || src_rect.width > primary.width
        || src_rect.height > primary.height
    {
        return None;
    }

    let dst_xy = dst_xy_override.unwrap_or_else(|| {
        GpgpuPoint::new(
            primary
                .width
                .saturating_sub(src_rect.width)
                .saturating_div(2) as i32,
            primary
                .height
                .saturating_sub(src_rect.height)
                .saturating_div(2) as i32,
        )
    });
    if !rect_is_inside(primary, GpgpuRect::new(dst_xy.x, dst_xy.y, src_rect.width, src_rect.height))
    {
        return None;
    }
    let stage_rect = GpgpuRect::new(0, 0, src_rect.width, src_rect.height);

    shell_zero_surface(shell);
    let stage_start_tick = direct_rcs_now_tick();
    let staged = shell_stage_atlas_over_scanout_xrgb(shell, atlas, src_rect, target, dst_xy)?;
    let stage_ms = direct_rcs_elapsed_ms_since(stage_start_tick);
    let src_head = shell_read_rect_head(shell, stage_rect);
    super::dma_flush(shell.virt, shell.surface.bytes);

    let copy_start_tick = direct_rcs_now_tick();
    let stats = copy_rect_rgba8_stats(shell.surface, stage_rect, primary, dst_xy);
    let copy_ms = direct_rcs_elapsed_ms_since(copy_start_tick);
    let dst_rect = GpgpuRect::new(dst_xy.x, dst_xy.y, src_rect.width, src_rect.height);
    let dst_head = primary_read_rect_head(target, dst_rect);
    let (src_preserved, copied) = primary_count_shell_raw_copy(target, shell, stage_rect, dst_xy);
    let expected_spans = copy_rect_expected_spans(stage_rect);
    let expected_submits = expected_spans.div_ceil(COPY_RECT_BATCH_MAX_SPANS);
    let flush_offset = (dst_xy.y as usize)
        .saturating_mul(primary.pitch_bytes as usize)
        .saturating_add((dst_xy.x as usize).saturating_mul(core::mem::size_of::<u32>()));
    let flush_bytes = (src_rect.height as usize)
        .saturating_sub(1)
        .saturating_mul(primary.pitch_bytes as usize)
        .saturating_add((src_rect.width as usize).saturating_mul(core::mem::size_of::<u32>()));
    let presented = super::display::notify_primary_surface_external_write(
        "gpgpu-twemoji-atlas-center",
        flush_offset,
        flush_bytes,
    );
    let pixels = rect_pixel_count(stage_rect);

    Some(GpgpuShellAtlasCopyResult {
        ok: staged == pixels
            && stats.spans == expected_spans
            && stats.submits == expected_submits
            && src_preserved == pixels
            && copied == pixels
            && presented,
        slot,
        atlas_width: atlas.width,
        atlas_height: atlas.height,
        atlas_src_rect: src_rect,
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
        staged,
        copied,
        src_preserved,
        src_head,
        dst_head,
        presented,
        total_ms: direct_rcs_elapsed_ms_since(total_start_tick),
        stage_ms,
        copy_ms,
    })
}

pub(crate) fn shell_copy_twemoji_atlas_slot_scanout_hot(
    slot: u16,
    dst_xy_override: Option<GpgpuPoint>,
) -> Option<GpgpuShellAtlasHotCopyResult> {
    shell_copy_twemoji_atlas_slot_scanout_hot_with(slot, dst_xy_override, true)
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

fn shell_copy_twemoji_atlas_slot_scanout_hot_with(
    slot: u16,
    dst_xy_override: Option<GpgpuPoint>,
    wide: bool,
) -> Option<GpgpuShellAtlasHotCopyResult> {
    let total_start_tick = direct_rcs_now_tick();
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
    if target.virt.is_null() {
        return None;
    }

    let atlas = twemoji_atlas_cache_once()?;
    let region = crate::gfx::althlasfont::twemoji::twemoji_lookup_slot_region(slot)?;
    let src_rect = GpgpuRect::new(
        region.src_x as i32,
        region.src_y as i32,
        u32::from(region.src_w.max(1)),
        u32::from(region.src_h.max(1)),
    );
    if !rect_is_inside_atlas(atlas.width, atlas.height, src_rect) {
        return None;
    }
    if src_rect.width > shell.surface.width
        || src_rect.height > shell.surface.height
        || src_rect.width > primary.width
        || src_rect.height > primary.height
    {
        return None;
    }

    let dst_xy = dst_xy_override.unwrap_or_else(|| {
        GpgpuPoint::new(
            primary
                .width
                .saturating_sub(src_rect.width)
                .saturating_div(2) as i32,
            primary
                .height
                .saturating_sub(src_rect.height)
                .saturating_div(2) as i32,
        )
    });
    if !rect_is_inside(primary, GpgpuRect::new(dst_xy.x, dst_xy.y, src_rect.width, src_rect.height))
    {
        return None;
    }
    let stage_rect = GpgpuRect::new(0, 0, src_rect.width, src_rect.height);

    shell_zero_surface(shell);
    let stage_start_tick = direct_rcs_now_tick();
    let staged = shell_stage_atlas_over_scanout_xrgb(shell, atlas, src_rect, target, dst_xy)?;
    let stage_ms = direct_rcs_elapsed_ms_since(stage_start_tick);
    super::dma_flush(shell.virt, shell.surface.bytes);

    let copy_start_tick = direct_rcs_now_tick();
    let stats = if wide {
        copy_rect_rgba8_wide_stats(shell.surface, stage_rect, primary, dst_xy)
    } else {
        copy_rect_rgba8_stats(shell.surface, stage_rect, primary, dst_xy)
    };
    let copy_ms = direct_rcs_elapsed_ms_since(copy_start_tick);
    let expected_spans = if wide {
        copy_rect_wide_expected_spans(stage_rect)
    } else {
        copy_rect_expected_spans(stage_rect)
    };
    let expected_submits = expected_spans.div_ceil(COPY_RECT_BATCH_MAX_SPANS);
    let flush_offset = (dst_xy.y as usize)
        .saturating_mul(primary.pitch_bytes as usize)
        .saturating_add((dst_xy.x as usize).saturating_mul(core::mem::size_of::<u32>()));
    let flush_bytes = (src_rect.height as usize)
        .saturating_sub(1)
        .saturating_mul(primary.pitch_bytes as usize)
        .saturating_add((src_rect.width as usize).saturating_mul(core::mem::size_of::<u32>()));
    let presented = super::display::notify_primary_surface_external_write(
        "gpgpu-twemoji-atlas-go",
        flush_offset,
        flush_bytes,
    );
    let pixels = rect_pixel_count(stage_rect);

    Some(GpgpuShellAtlasHotCopyResult {
        ok: staged == pixels
            && stats.spans == expected_spans
            && stats.submits == expected_submits
            && presented,
        slot,
        atlas_src_rect: src_rect,
        dst_xy,
        primary_width: primary.width,
        primary_height: primary.height,
        pixels,
        spans: stats.spans,
        expected_spans,
        submits: stats.submits,
        expected_submits,
        staged,
        presented,
        total_ms: direct_rcs_elapsed_ms_since(total_start_tick),
        stage_ms,
        copy_ms,
    })
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
    let Some(upload) = copy_rect_rgba8_upload_status() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: direct-rcs-smoke skipped reason=no-kernel-upload\n"
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
        "intel/gpgpu: direct-rcs-smoke forcewake={} ggtt={} ppgtt={} batch={} submitted={} retired={} retire_ms={} observed=0x{:08X} expected=0x{:08X} kernel_gpu=0x{:X} kernel_text_gpu=0x{:X} ring_gpu=0x{:X} batch_gpu=0x{:X} result_gpu=0x{:X} head=0x{:08X} tail=0x{:08X} acthd=0x{:08X} ipeir=0x{:08X} ipehr=0x{:08X} eir=0x{:08X} path=direct-execlist no_guc_submit=1 next=gpgpu-walker\n",
        forcewake_ok as u8,
        mapped_ok as u8,
        ppgtt_ok as u8,
        batch_ok as u8,
        submitted as u8,
        retired as u8,
        retire_ms,
        observed,
        DIRECT_RCS_SMOKE_MARKER,
        upload.gpu,
        upload.gpu + COPY_RECT_RGBA8_TEXT_OFFSET_BYTES,
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

pub(crate) fn submit_empty_eot_walker_once() -> bool {
    if !DIRECT_RCS_ENABLED || EMPTY_EOT_WALKER_RAN.swap(true, Ordering::AcqRel) {
        return false;
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: empty-eot-walker skipped reason=no-claimed-device\n"
        );
        return false;
    };
    let Some(upload) = upload_empty_eot_kernel() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: empty-eot-walker skipped reason=no-kernel-upload\n"
        );
        return false;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: empty-eot-walker failed rung=alloc\n"
        );
        return false;
    };

    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ppgtt_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let batch_ok = kernel_ppgtt_ok && direct_rcs_encode_empty_eot_walker_batch(state, upload);
    let submit_start_tick = direct_rcs_now_tick();
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result_slot(state, EMPTY_EOT_POST_MARKER_SLOT, EMPTY_EOT_POST_MARKER)
    } else {
        0
    };
    let retire_ms = if submitted {
        direct_rcs_elapsed_ms_since(submit_start_tick)
    } else {
        0
    };
    let pre_marker = direct_rcs_read_result_slot(state, EMPTY_EOT_PRE_MARKER_SLOT);
    let retired = observed == EMPTY_EOT_POST_MARKER;

    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: empty-eot-walker forcewake={} ggtt={} ppgtt={} kernel_ppgtt={} batch={} submitted={} retired={} retire_ms={} pre_marker=0x{:08X} post_marker=0x{:08X} expected_post=0x{:08X} kernel_gpu=0x{:X} kernel_text_gpu=0x{:X} idd_off=0x{:X} simd=16 groups=1 threads_per_group={} ring_gpu=0x{:X} batch_gpu=0x{:X} result_gpu=0x{:X} head=0x{:08X} tail=0x{:08X} acthd=0x{:08X} ipeir=0x{:08X} ipehr=0x{:08X} eir=0x{:08X} path=direct-execlist no_guc_submit=1 next=clear-rect-writes\n",
        forcewake_ok as u8,
        mapped_ok as u8,
        ppgtt_ok as u8,
        kernel_ppgtt_ok as u8,
        batch_ok as u8,
        submitted as u8,
        retired as u8,
        retire_ms,
        pre_marker,
        observed,
        EMPTY_EOT_POST_MARKER,
        upload.gpu,
        upload.gpu + EMPTY_EOT_TEXT_OFFSET_BYTES,
        EMPTY_EOT_IDD_OFFSET_BYTES,
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

    retired
}

pub(crate) fn submit_clear_rect_rgba8_white_strip_once() -> bool {
    if !DIRECT_RCS_ENABLED || CLEAR_RECT_WALKER_RAN.swap(true, Ordering::AcqRel) {
        return false;
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: clear-rect-rgba8-white-strip skipped reason=no-claimed-device\n"
        );
        return false;
    };
    let Some(upload) = upload_clear_rect_rgba8_white_kernel() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: clear-rect-rgba8-white-strip skipped reason=no-kernel-upload\n"
        );
        return false;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: clear-rect-rgba8-white-strip failed rung=alloc\n"
        );
        return false;
    };

    let before = direct_rcs_seed_clear_rect_strip(state);
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
    let params = ClearRectRgba8WhiteParams {
        dst_gpu: DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE,
        dst_pitch_bytes: CLEAR_RECT_TEST_PITCH_BYTES,
        dst_x: 0,
        dst_y: 0,
        width: CLEAR_RECT_TEST_WIDTH,
        height: CLEAR_RECT_TEST_HEIGHT,
    };
    let batch_ok = test_ppgtt_ok
        && direct_rcs_encode_clear_rect_walker_batch(
            state,
            upload,
            params,
            CLEAR_RECT_TEST_PIXELS * core::mem::size_of::<u32>(),
            CLEAR_RECT_WALKER_RIGHT_MASK,
        );
    let submit_start_tick = direct_rcs_now_tick();
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let (observed, retire_ms) = if submitted {
        direct_rcs_poll_result_slot_elapsed(
            state,
            CLEAR_RECT_POST_MARKER_SLOT,
            CLEAR_RECT_POST_MARKER,
            submit_start_tick,
        )
    } else {
        (0, 0)
    };
    let pre_marker = direct_rcs_read_result_slot(state, CLEAR_RECT_PRE_MARKER_SLOT);
    let after = direct_rcs_read_clear_rect_strip(state);
    let retired = observed == CLEAR_RECT_POST_MARKER;
    let before_ok = before
        == [
            CLEAR_RECT_RGBA_RED,
            CLEAR_RECT_RGBA_GREEN,
            CLEAR_RECT_RGBA_BLUE,
            CLEAR_RECT_RGBA_BLACK,
        ];
    let white_count = direct_rcs_count_white(after);
    let white_ok = white_count == CLEAR_RECT_TEST_PIXELS;

    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: clear-rect-rgba8-white-strip forcewake={} ggtt={} ppgtt={} kernel_ppgtt={} test_ppgtt={} batch={} submitted={} retired={} retire_ms={} before_ok={} white_ok={} white={}/{} before=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] after=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] pre_marker=0x{:08X} post_marker=0x{:08X} expected_post=0x{:08X} kernel_gpu=0x{:X} kernel_text_gpu=0x{:X} dst_gpu=0x{:X} rect={}x{} pitch={} idd_off=0x{:X} payload_off=0x{:X} simd=16 groups=1 threads_per_group={} ring_gpu=0x{:X} batch_gpu=0x{:X} result_gpu=0x{:X} head=0x{:08X} tail=0x{:08X} acthd=0x{:08X} ipeir=0x{:08X} ipehr=0x{:08X} eir=0x{:08X} path=direct-execlist no_guc_submit=1 next=copy-rect-or-scanout\n",
        forcewake_ok as u8,
        mapped_ok as u8,
        ppgtt_ok as u8,
        kernel_ppgtt_ok as u8,
        test_ppgtt_ok as u8,
        batch_ok as u8,
        submitted as u8,
        retired as u8,
        retire_ms,
        before_ok as u8,
        white_ok as u8,
        white_count,
        CLEAR_RECT_TEST_PIXELS,
        before[0],
        before[1],
        before[2],
        before[3],
        after[0],
        after[1],
        after[2],
        after[3],
        pre_marker,
        observed,
        CLEAR_RECT_POST_MARKER,
        upload.gpu,
        upload.gpu + CLEAR_RECT_RGBA8_WHITE_TEXT_OFFSET_BYTES,
        DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE,
        CLEAR_RECT_TEST_WIDTH,
        CLEAR_RECT_TEST_HEIGHT,
        CLEAR_RECT_TEST_PITCH_BYTES,
        CLEAR_RECT_IDD_OFFSET_BYTES,
        CLEAR_RECT_PAYLOAD_OFFSET_BYTES,
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

    retired && before_ok && white_ok
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

pub(crate) fn submit_copy_rect_rgba8_wide_256x2_once() -> bool {
    if !DIRECT_RCS_ENABLED || COPY_RECT_WIDE_256X2_RAN.swap(true, Ordering::AcqRel) {
        return false;
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: copy-rect-rgba8-wide-256x2 skipped reason=no-claimed-device\n"
        );
        return false;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: copy-rect-rgba8-wide-256x2 failed rung=alloc\n"
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
            "intel/gpgpu: copy-rect-rgba8-wide-256x2 failed rung=surface\n"
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
            "intel/gpgpu: copy-rect-rgba8-wide-256x2 failed rung=lower\n"
        );
        return false;
    };
    let Some(flavor) = copy_rect_kernel_flavor_wide() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: copy-rect-rgba8-wide-256x2 failed rung=kernel\n"
        );
        return false;
    };
    let copy_start_tick = direct_rcs_now_tick();
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
    let ok = stats.spans == COPY_RECT_WIDE_256X2_EXPECTED_SPANS
        && stats.submits == COPY_RECT_256X2_EXPECTED_SUBMITS
        && src_preserved == (COPY_RECT_256X2_WIDTH * COPY_RECT_256X2_HEIGHT) as usize
        && copied == (COPY_RECT_256X2_WIDTH * COPY_RECT_256X2_HEIGHT) as usize
        && samples_ok == COPY_RECT_256X2_SAMPLE_POINTS.len();

    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: copy-rect-rgba8-wide-256x2 ok={} copy_ms={} submits={}/{} spans={}/{} copied={}/{} src_preserved={}/{} samples={}/{} surface_gpu=0x{:X} surface_phys=0x{:X} rect={}x{} src_xy=0,0 dst_xy={},0 pitch={} row0_dst_head=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] row1_dst_head=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] row1_dst_tail=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] lowering=row-blocks pixels_per_lane={} rows_per_walker={} batched_walkers=1 max_span_px={} artifact={}\n",
        ok as u8,
        copy_ms,
        stats.submits,
        COPY_RECT_256X2_EXPECTED_SUBMITS,
        stats.spans,
        COPY_RECT_WIDE_256X2_EXPECTED_SPANS,
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
        COPY_RECT_WIDE_PIXELS_PER_LANE,
        COPY_RECT_WIDE_ROWS_PER_WALKER,
        COPY_RECT_WIDE_SPAN_PIXELS,
        COPY_RECT_RGBA8_WIDE_KERNEL_NAME,
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
        24,
        1,
        24 * core::mem::size_of::<u32>() as u32,
    ) else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu/rect-api: failed rung=surface\n"
        );
        return false;
    };

    direct_rcs_seed_rect_api_smoke(state);
    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu/rect-api: ops=fill_rect_white,copy_rect,blit_glyph,copy_many_rects fill_artifact={} copy_artifact={} surface_gpu=0x{:X} surface_phys=0x{:X} surface={}x{} pitch={} lowering=row-spans copy_pixels_per_lane={} copy_max_span_px={} clear_max_span_px=16\n",
        CLEAR_RECT_RGBA8_WHITE_KERNEL_NAME,
        COPY_RECT_RGBA8_KERNEL_NAME,
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
    let fill_spans = fill_rect_white_rgba8(surface, GpgpuRect::new(20, 0, 4, 1));
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
    let total_ms = direct_rcs_elapsed_ms_since(rect_api_start_tick);

    let src_a = direct_rcs_read_rect_api_span(state, 0);
    let dst_a = direct_rcs_read_rect_api_span(state, 4);
    let src_b = direct_rcs_read_rect_api_span(state, 8);
    let dst_b = direct_rcs_read_rect_api_span(state, 12);
    let src_c = direct_rcs_read_rect_api_span(state, 16);
    let dst_c = direct_rcs_read_rect_api_span(state, 20);
    let copy_ok = dst_a == src_a;
    let blit_ok = dst_b == src_b;
    let many_ok = dst_c == src_c;
    let fill_ok = fill_spans == 1 && fill_white == 4;
    let ok = fill_ok
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
        "intel/gpgpu/rect-api: result ok={} total_ms={} fill_ms={} copy_ms={} blit_ms={} many_ms={} fill_spans={} fill_submits={} fill_white={}/4 copy_spans={} copy_submits={} blit_spans={} blit_submits={} many_spans={} many_submits={} copy_ok={} blit_ok={} many_ok={} fill_after=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] copy_dst=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] blit_dst=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] many_dst=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}]\n",
        ok as u8,
        total_ms,
        fill_ms,
        copy_ms,
        blit_ms,
        many_ms,
        fill_spans,
        fill_spans,
        fill_white,
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

fn sprite64_worklist_atlas_once() -> Option<GpgpuSprite64WorklistAtlasSurface> {
    GPGPU_SPRITE64_WORKLIST_ATLAS
        .call_once(|| {
            let atlas = twemoji_atlas_cache_once()?;
            let slot_count = crate::gfx::althlasfont::twemoji::twemoji_slot_count();
            if slot_count == 0 {
                return None;
            }
            let columns = SPRITE64_WORKLIST_ATLAS_COLUMNS;
            let rows = (u32::from(slot_count)).div_ceil(columns);
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
            for slot in 0..slot_count {
                let Some(region) =
                    crate::gfx::althlasfont::twemoji::twemoji_lookup_slot_region(slot)
                else {
                    continue;
                };
                let src_w = u32::from(region.src_w)
                    .min(SPRITE64_WORKLIST_CELL_PIXELS)
                    .min(atlas.width.saturating_sub(u32::from(region.src_x)));
                let src_h = u32::from(region.src_h)
                    .min(SPRITE64_WORKLIST_CELL_PIXELS)
                    .min(atlas.height.saturating_sub(u32::from(region.src_y)));
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
                        let src_idx =
                            ((atlas_y as usize) * (atlas.width as usize) + atlas_x as usize) * 4;
                        let r = *atlas.rgba.get(src_idx)? as u32;
                        let g = *atlas.rgba.get(src_idx + 1)? as u32;
                        let b = *atlas.rgba.get(src_idx + 2)? as u32;
                        let a = *atlas.rgba.get(src_idx + 3)? as u32;
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
                slots: slot_count,
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
            let pixels = (decoded.width as usize).checked_mul(decoded.height as usize)?;
            let mut xrgb = Vec::with_capacity(pixels);
            for idx in 0..pixels {
                let off = idx.saturating_mul(4);
                let r = *decoded.rgba.get(off)?;
                let g = *decoded.rgba.get(off + 1)?;
                let b = *decoded.rgba.get(off + 2)?;
                xrgb.push(((r as u32) << 16) | ((g as u32) << 8) | b as u32);
            }
            Some(GpgpuTwemojiAtlasCache {
                width: decoded.width,
                height: decoded.height,
                rgba: decoded.rgba,
                xrgb,
            })
        })
        .as_ref()
}

fn shell_stage_atlas_over_scanout_xrgb(
    shell: GpgpuShellSurface,
    atlas: &GpgpuTwemojiAtlasCache,
    src_rect: GpgpuRect,
    target: super::display::PrimarySurfaceGpgpuTarget,
    dst_xy: GpgpuPoint,
) -> Option<usize> {
    shell_stage_atlas_over_scanout_xrgb_at(
        shell,
        atlas,
        src_rect,
        target,
        dst_xy,
        GpgpuPoint::new(0, 0),
    )
}

fn shell_stage_atlas_over_scanout_xrgb_at(
    shell: GpgpuShellSurface,
    atlas: &GpgpuTwemojiAtlasCache,
    src_rect: GpgpuRect,
    target: super::display::PrimarySurfaceGpgpuTarget,
    dst_xy: GpgpuPoint,
    stage_xy: GpgpuPoint,
) -> Option<usize> {
    let stage_rect = GpgpuRect::new(stage_xy.x, stage_xy.y, src_rect.width, src_rect.height);
    if !rect_is_inside(shell.surface, stage_rect)
        || !rect_is_inside_atlas(atlas.width, atlas.height, src_rect)
    {
        return None;
    }

    let mut staged = 0usize;
    for y in 0..src_rect.height {
        for x in 0..src_rect.width {
            let a = atlas_alpha(atlas, src_rect, x, y)?;
            let xrgb = atlas_xrgb_pixel(atlas, src_rect, x, y)?;
            let pixel = if a == 0xFF {
                xrgb
            } else {
                let dst = primary_read_pixel(target, dst_xy.x as u32 + x, dst_xy.y as u32 + y);
                blend_xrgb_over_xrgb(xrgb, a, dst)
            };
            shell_write_pixel(shell, stage_xy.x as u32 + x, stage_xy.y as u32 + y, pixel);
            staged += 1;
        }
    }
    Some(staged)
}

fn atlas_xrgb_pixel(
    atlas: &GpgpuTwemojiAtlasCache,
    src_rect: GpgpuRect,
    x: u32,
    y: u32,
) -> Option<u32> {
    let atlas_x = src_rect.x as u32 + x;
    let atlas_y = src_rect.y as u32 + y;
    let idx = (atlas_y as usize)
        .saturating_mul(atlas.width as usize)
        .saturating_add(atlas_x as usize);
    atlas.xrgb.get(idx).copied()
}

fn atlas_alpha(atlas: &GpgpuTwemojiAtlasCache, src_rect: GpgpuRect, x: u32, y: u32) -> Option<u8> {
    let atlas_width = atlas.width as usize;
    let atlas_x = src_rect.x as u32 + x;
    let atlas_y = src_rect.y as u32 + y;
    let src_idx = ((atlas_y as usize).saturating_mul(atlas_width) + atlas_x as usize) * 4;
    atlas.rgba.get(src_idx + 3).copied()
}

fn blend_xrgb_over_xrgb(src: u32, a: u8, dst: u32) -> u32 {
    if a == 0 {
        return dst & 0x00FF_FFFF;
    }
    if a == 0xFF {
        return src & 0x00FF_FFFF;
    }

    let alpha = a as u32;
    let inv_alpha = 255u32.saturating_sub(alpha);
    let src_r = (src >> 16) & 0xFF;
    let src_g = (src >> 8) & 0xFF;
    let src_b = src & 0xFF;
    let dst_r = (dst >> 16) & 0xFF;
    let dst_g = (dst >> 8) & 0xFF;
    let dst_b = dst & 0xFF;
    let out_r = ((src_r * alpha) + (dst_r * inv_alpha) + 127) / 255;
    let out_g = ((src_g * alpha) + (dst_g * inv_alpha) + 127) / 255;
    let out_b = ((src_b * alpha) + (dst_b * inv_alpha) + 127) / 255;
    (out_r << 16) | (out_g << 8) | out_b
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

fn primary_count_shell_raw_copy(
    target: super::display::PrimarySurfaceGpgpuTarget,
    shell: GpgpuShellSurface,
    src_rect: GpgpuRect,
    dst_xy: GpgpuPoint,
) -> (usize, usize) {
    let mut src_preserved = 0usize;
    let mut copied = 0usize;
    for y in 0..src_rect.height {
        for x in 0..src_rect.width {
            let expected = shell_read_pixel(shell, src_rect.x as u32 + x, src_rect.y as u32 + y);
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

fn clear_rect_expected_spans(rect: GpgpuRect) -> usize {
    (rect.width as usize)
        .div_ceil(16)
        .saturating_mul(rect.height as usize)
}

fn copy_rect_expected_spans(rect: GpgpuRect) -> usize {
    copy_rect_expected_spans_for(rect, COPY_RECT_SPAN_PIXELS, 1)
}

fn copy_rect_wide_expected_spans(rect: GpgpuRect) -> usize {
    copy_rect_expected_spans_for(rect, COPY_RECT_WIDE_SPAN_PIXELS, COPY_RECT_WIDE_ROWS_PER_WALKER)
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

fn lower_clear_rect_white(
    dst: GpgpuRgba8Surface,
    rect: GpgpuRect,
) -> Option<ClearRectRgba8WhiteParams> {
    if !dst.is_valid() || rect.is_empty() {
        return None;
    }
    let clipped = clip_rect_to_surface(rect, dst)?;
    Some(ClearRectRgba8WhiteParams {
        dst_gpu: dst.gpu,
        dst_pitch_bytes: dst.pitch_bytes,
        dst_x: clipped.x as u32,
        dst_y: clipped.y as u32,
        width: clipped.width,
        height: clipped.height,
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

fn submit_clear_rect_white_spans(
    dst: GpgpuRgba8Surface,
    params: ClearRectRgba8WhiteParams,
) -> usize {
    let mut submitted = 0usize;
    for row in 0..params.height {
        let mut x = 0u32;
        while x < params.width {
            let span = (params.width - x).min(16);
            let span_params = ClearRectRgba8WhiteParams {
                dst_x: params.dst_x + x,
                dst_y: params.dst_y + row,
                width: span,
                height: 1,
                ..params
            };
            if submit_clear_rect_white_span(dst, span_params) {
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
    let Some(total_spans) =
        copy_rect_span_count(params, flavor.span_pixels, flavor.rows_per_walker)
    else {
        return GpgpuSubmitStats::default();
    };
    let mut stats = GpgpuSubmitStats::default();
    let mut span_start = 0usize;
    while span_start < total_spans {
        let span_take = (total_spans - span_start).min(COPY_RECT_BATCH_MAX_SPANS);
        if !submit_copy_rect_span_batch(src, dst, params, flavor, span_start, span_take) {
            break;
        }
        stats.spans = stats.spans.saturating_add(span_take);
        stats.submits = stats.submits.saturating_add(1);
        span_start += span_take;
    }
    stats
}

fn submit_copy_rect_multi_ops_with_stats(
    src: GpgpuRgba8Surface,
    dst: GpgpuRgba8Surface,
    params: &[CopyRectRgba8Params],
    flavor: CopyRectKernelFlavor,
) -> GpgpuSubmitStats {
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
                if !submit_copy_rect_span_params_batch(src, dst, &span_params, flavor) {
                    return stats;
                }
                stats.spans = stats.spans.saturating_add(span_params.len());
                stats.submits = stats.submits.saturating_add(1);
                span_params.clear();
            }
        }
    }

    if !span_params.is_empty() {
        if !submit_copy_rect_span_params_batch(src, dst, &span_params, flavor) {
            return stats;
        }
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

fn submit_clear_rect_white_span(dst: GpgpuRgba8Surface, params: ClearRectRgba8WhiteParams) -> bool {
    if params.width == 0 || params.width > 16 || params.height != 1 {
        return false;
    }
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        return false;
    };
    let Some(upload) = upload_clear_rect_rgba8_white_kernel() else {
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
        && direct_rcs_encode_clear_rect_walker_batch(
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

fn sprite64_worklist_walker_count(desc_count: usize) -> usize {
    desc_count
        .div_ceil(SPRITE64_WORKLIST_DESCS_PER_WALKER)
        .min(SPRITE64_WORKLIST_MAX_WALKERS)
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

fn direct_rcs_encode_empty_eot_walker_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
) -> bool {
    if EMPTY_EOT_IDD_OFFSET_BYTES + EMPTY_EOT_IDD_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }

    let idd = unsafe { state.batch_virt.add(EMPTY_EOT_IDD_OFFSET_BYTES) as *mut u32 };
    unsafe {
        core::ptr::write_volatile(idd, EMPTY_EOT_TEXT_OFFSET_BYTES as u32);
        core::ptr::write_volatile(idd.add(1), 0);
        core::ptr::write_volatile(idd.add(2), IDD_THREAD_PREEMPTION_DISABLE);
        core::ptr::write_volatile(idd.add(3), 0);
        core::ptr::write_volatile(idd.add(4), 0);
        core::ptr::write_volatile(idd.add(5), 0);
        core::ptr::write_volatile(idd.add(6), GPGPU_WALKER_GROUP_THREADS);
        core::ptr::write_volatile(idd.add(7), 0);
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
    ok &= direct_rcs_push(batch, &mut cursor, EMPTY_EOT_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, EMPTY_EOT_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        EMPTY_EOT_PRE_MARKER_SLOT,
        EMPTY_EOT_PRE_MARKER,
    );
    ok &= direct_rcs_push(batch, &mut cursor, GPGPU_WALKER_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
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
    ok &= direct_rcs_push(batch, &mut cursor, GPGPU_WALKER_SIMD16_MASK);
    ok &= direct_rcs_push(batch, &mut cursor, GPGPU_WALKER_BOTTOM_MASK);
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_FLUSH_BITS);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        EMPTY_EOT_POST_MARKER_SLOT,
        EMPTY_EOT_POST_MARKER,
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

fn direct_rcs_encode_canvas3d_transform_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: Canvas3dTransformQ16Params,
    pre_marker_value: u32,
    post_marker_value: u32,
    src_bytes: usize,
    dst_bytes: usize,
) -> bool {
    if params.vertex_count == 0
        || CANVAS3D_TRANSFORM_PAYLOAD_OFFSET_BYTES + CANVAS3D_TRANSFORM_INDIRECT_BYTES
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
        CANVAS3D_TRANSFORM_IDD_OFFSET_BYTES,
        CANVAS3D_TRANSFORM_BINDING_TABLE_OFFSET_BYTES,
        0x40,
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
    if !direct_rcs_write_canvas3d_transform_payload(state, params) {
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
    ok &= direct_rcs_push_canvas3d_transform_walker(batch, &mut cursor);
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

fn direct_rcs_encode_clear_rect_walker_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: ClearRectRgba8WhiteParams,
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

    if !direct_rcs_write_clear_rect_interface_descriptor(state) {
        return false;
    }
    if !direct_rcs_write_clear_rect_surface_state(state, params.dst_gpu, dst_bytes) {
        return false;
    }
    if !direct_rcs_write_clear_rect_payload(state, params) {
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

fn direct_rcs_write_clear_rect_interface_descriptor(state: DirectRcsState) -> bool {
    if CLEAR_RECT_IDD_OFFSET_BYTES + CLEAR_RECT_IDD_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    let idd = unsafe { state.batch_virt.add(CLEAR_RECT_IDD_OFFSET_BYTES) as *mut u32 };
    unsafe {
        core::ptr::write_volatile(idd, CLEAR_RECT_RGBA8_WHITE_TEXT_OFFSET_BYTES as u32);
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

fn direct_rcs_write_canvas3d_transform_payload(
    state: DirectRcsState,
    params: Canvas3dTransformQ16Params,
) -> bool {
    if CANVAS3D_TRANSFORM_PAYLOAD_OFFSET_BYTES + CANVAS3D_TRANSFORM_INDIRECT_BYTES
        > DIRECT_RCS_BATCH_BYTES
    {
        return false;
    }

    unsafe {
        let payload = state
            .batch_virt
            .add(CANVAS3D_TRANSFORM_PAYLOAD_OFFSET_BYTES);
        core::ptr::write_bytes(payload, 0, CANVAS3D_TRANSFORM_INDIRECT_BYTES);
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
        core::ptr::write_volatile(dwords.add(20), params.param_q16.x as u32);
        core::ptr::write_volatile(dwords.add(21), params.param_q16.y as u32);
        core::ptr::write_volatile(dwords.add(22), params.param_q16.z as u32);
        core::ptr::write_volatile(dwords.add(23), params.param_q16.pad as u32);

        let local_ids = payload.add(CANVAS3D_TRANSFORM_CROSS_THREAD_BYTES) as *mut u16;
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

fn direct_rcs_write_clear_rect_payload(
    state: DirectRcsState,
    params: ClearRectRgba8WhiteParams,
) -> bool {
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

        let local_ids = payload.add(CLEAR_RECT_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_seed_clear_rect_strip(state: DirectRcsState) -> [u32; 4] {
    let values = [
        CLEAR_RECT_RGBA_RED,
        CLEAR_RECT_RGBA_GREEN,
        CLEAR_RECT_RGBA_BLUE,
        CLEAR_RECT_RGBA_BLACK,
    ];
    unsafe {
        core::ptr::write_bytes(state.clear_test_virt, 0, CLEAR_RECT_TEST_BYTES);
        let dst = state.clear_test_virt as *mut u32;
        for (index, value) in values.iter().copied().enumerate() {
            core::ptr::write_volatile(dst.add(index), value);
        }
    }
    super::dma_flush(state.clear_test_virt, CLEAR_RECT_TEST_BYTES);
    values
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

fn direct_rcs_read_clear_rect_strip(state: DirectRcsState) -> [u32; 4] {
    super::dma_flush(state.clear_test_virt, CLEAR_RECT_TEST_BYTES);
    let mut values = [0u32; 4];
    unsafe {
        let src = state.clear_test_virt as *const u32;
        for (index, value) in values.iter_mut().enumerate() {
            *value = core::ptr::read_volatile(src.add(index));
        }
    }
    values
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

fn direct_rcs_push_canvas3d_transform_walker(batch: &mut [u32], cursor: &mut usize) -> bool {
    direct_rcs_push(batch, cursor, GPGPU_WALKER_CMD)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, CANVAS3D_TRANSFORM_INDIRECT_BYTES as u32)
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
