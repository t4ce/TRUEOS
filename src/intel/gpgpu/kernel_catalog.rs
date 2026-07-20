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
pub(crate) const ALPHA_BLEND_WORKLIST_RGBA8_KERNEL_NAME: &str = "alpha_blend_worklist_rgba8";
pub(crate) const ALPHA_BLEND_WORKLIST_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/alpha_blend_worklist_rgba8.cl");
pub(crate) const GLYPH_MASK_RGBA8_KERNEL_NAME: &str = "glyph_mask_rgba8";
pub(crate) const GLYPH_MASK_RGBA8_OPENCL_SOURCE: &str = include_str!("kernels/glyph_mask_rgba8.cl");
pub(crate) const UI4_NV12_YTILE_TO_PRIMARY_XRGB_KERNEL_NAME: &str =
    "ui4_nv12_ytile_to_primary_xrgb";
pub(crate) const UI4_NV12_YTILE_TO_PRIMARY_XRGB_OPENCL_SOURCE: &str =
    include_str!("kernels/ui4_nv12_ytile_to_primary_xrgb.cl");
pub(crate) const UI4_NV12_TILE64_TO_RGBA8_FRAME_KERNEL_NAME: &str =
    "ui4_nv12_tile64_to_rgba8_frame";
pub(crate) const UI4_NV12_TILE64_TO_RGBA8_FRAME_OPENCL_SOURCE: &str =
    include_str!("kernels/ui4_nv12_tile64_to_rgba8_frame.cl");
pub(crate) const SPRITE_QUAD_WORKLIST_RGBA8_KERNEL_NAME: &str = "sprite_quad_worklist_rgba8";
pub(crate) const SPRITE_QUAD_WORKLIST_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/sprite_quad_worklist_rgba8.cl");
pub(crate) const UI4_COMPOSE_LAYERS_RGBA8_KERNEL_NAME: &str = "ui4_compose_layers_rgba8";
pub(crate) const UI4_COMPOSE_LAYERS_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/ui4_compose_layers_rgba8.cl");
pub(crate) const MANDEL64_WORKLIST_RGBA8_KERNEL_NAME: &str = "mandel64_worklist_rgba8";
pub(crate) const MANDEL64_WORKLIST_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/mandel64_worklist_rgba8.cl");
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
        ALPHA_BLEND_WORKLIST_RGBA8_KERNEL_NAME => Some(ALPHA_BLEND_WORKLIST_RGBA8_OPENCL_SOURCE),
        GLYPH_MASK_RGBA8_KERNEL_NAME => Some(GLYPH_MASK_RGBA8_OPENCL_SOURCE),
        UI4_NV12_YTILE_TO_PRIMARY_XRGB_KERNEL_NAME => {
            Some(UI4_NV12_YTILE_TO_PRIMARY_XRGB_OPENCL_SOURCE)
        }
        UI4_NV12_TILE64_TO_RGBA8_FRAME_KERNEL_NAME => {
            Some(UI4_NV12_TILE64_TO_RGBA8_FRAME_OPENCL_SOURCE)
        }
        SPRITE_QUAD_WORKLIST_RGBA8_KERNEL_NAME => Some(SPRITE_QUAD_WORKLIST_RGBA8_OPENCL_SOURCE),
        UI4_COMPOSE_LAYERS_RGBA8_KERNEL_NAME => Some(UI4_COMPOSE_LAYERS_RGBA8_OPENCL_SOURCE),
        MANDEL64_WORKLIST_RGBA8_KERNEL_NAME => Some(MANDEL64_WORKLIST_RGBA8_OPENCL_SOURCE),
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
        ALPHA_BLEND_WORKLIST_RGBA8_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/alpha_blend_worklist_rgba8.cl")
        }
        GLYPH_MASK_RGBA8_KERNEL_NAME => Some("src/intel/gpgpu/kernels/glyph_mask_rgba8.cl"),
        UI4_NV12_YTILE_TO_PRIMARY_XRGB_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/ui4_nv12_ytile_to_primary_xrgb.cl")
        }
        UI4_NV12_TILE64_TO_RGBA8_FRAME_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/ui4_nv12_tile64_to_rgba8_frame.cl")
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
pub(crate) const UI4_NV12_YTILE_TO_PRIMARY_XRGB_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/ui4_nv12_ytile_to_primary_xrgb.bin");
pub(crate) const UI4_NV12_YTILE_TO_PRIMARY_XRGB_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/ui4_nv12_ytile_to_primary_xrgb.spv");
pub(crate) const UI4_NV12_TILE64_TO_RGBA8_FRAME_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/ui4_nv12_tile64_to_rgba8_frame.bin");
pub(crate) const UI4_NV12_TILE64_TO_RGBA8_FRAME_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/ui4_nv12_tile64_to_rgba8_frame.spv");

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
pub(crate) const UI4_NV12_YTILE_TO_PRIMARY_XRGB_ADLS_BIN_SHA256: [u8; 32] = [
    0x65, 0xEA, 0x37, 0xF3, 0xCE, 0x33, 0xAC, 0x92, 0x67, 0x92, 0x80, 0x34, 0xC3, 0xE5, 0x59, 0xF8,
    0x85, 0x47, 0xE9, 0x02, 0x9D, 0x29, 0x22, 0x31, 0xC9, 0x11, 0x6F, 0x25, 0x85, 0x13, 0x2E, 0x4D,
];
pub(crate) const UI4_NV12_TILE64_TO_RGBA8_FRAME_ADLS_BIN_SHA256: [u8; 32] = [
    0x75, 0xC2, 0xC2, 0x30, 0xCC, 0x45, 0xAE, 0x19, 0x87, 0x91, 0x04, 0x79, 0xA2, 0x1D, 0xA7, 0x92,
    0x26, 0xED, 0x85, 0xCF, 0xF8, 0xE4, 0xCF, 0x8E, 0x2A, 0x61, 0xB6, 0xB6, 0x9C, 0xC0, 0x17, 0xB3,
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
