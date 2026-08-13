#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum DispatchMode {
    Simd8,
    Simd16,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    Simd32,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct ShaderKernelMetadata {
    pub(crate) code_offset_bytes: u32,
    pub(crate) code_size_bytes: u32,
    pub(crate) code_alignment_bytes: u32,
    pub(crate) ksp_offset_bytes: u32,
    pub(crate) dispatch_mode: DispatchMode,
    pub(crate) grf_start_register: u8,
    pub(crate) grf_used: u8,
    pub(crate) push_constant_bytes: u16,
    pub(crate) binding_table_entry_count: u8,
    pub(crate) sampler_count: u8,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct TriangleVertexShaderMetadata {
    pub(crate) kernel: ShaderKernelMetadata,
    pub(crate) max_threads: u16,
    pub(crate) urb_entry_output_length: u8,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct TrianglePixelShaderMetadata {
    pub(crate) kernel: ShaderKernelMetadata,
    pub(crate) num_varying_inputs: u8,
    pub(crate) uses_vmask: bool,
    pub(crate) computed_stencil: bool,
    pub(crate) persample_dispatch: bool,
    pub(crate) computed_depth_mode: u8,
    pub(crate) flat_inputs: u32,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct TriangleVertexShader {
    pub(crate) meta: TriangleVertexShaderMetadata,
    pub(crate) code: &'static [u32],
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct TrianglePixelShader {
    pub(crate) meta: TrianglePixelShaderMetadata,
    pub(crate) code: &'static [u32],
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct TrianglePipeline {
    pub(crate) vs: TriangleVertexShader,
    pub(crate) ps: TrianglePixelShader,
}

#[path = "../../crates/trueos-shader/generated_triangle.rs"]
mod generated_triangle;

pub(crate) const TRIANGLE_VERTEX_SOURCE_PATH: &str =
    "crates/trueos-shader/generated_triangle.rs:TRIANGLE_VS_CODE";
pub(crate) const TRIANGLE_FRAGMENT_SOURCE_PATH: &str =
    "crates/trueos-shader/generated_triangle.rs:TRIANGLE_PS_CODE";
pub(crate) const TRIANGLE_VERTEX_COMPONENTS: usize = 3;
pub(crate) const TRIANGLE_VERTEX_STRIDE_BYTES: usize =
    TRIANGLE_VERTEX_COMPONENTS * core::mem::size_of::<f32>();

pub(crate) fn triangle_pipeline() -> &'static TrianglePipeline {
    generated_triangle::triangle_pipeline()
}

pub(crate) fn triangle_pipeline_simd16() -> &'static TrianglePipeline {
    generated_triangle::triangle_pipeline_simd16()
}

// Reproducibly baked by tools/heliov-texture-shader-bake from the exact WGSL
// admitted by the public VMX shader-module digest. This is an authenticated
// WGPU interface package, not application dispatch logic.
static HELIOV_TEXTURED_VS: [u32; 44] = [
    0x00030061, 0x07054220, 0x00000000, 0x00000000, 0x00030061, 0x08054220, 0x00000000, 0x00000000,
    0x00030061, 0x09054220, 0x00000000, 0x00000000, 0x00030061, 0x0a054220, 0x00000000, 0x00000000,
    0x610b0061, 0x00100200, 0x610c0061, 0x00120300, 0x610d0061, 0x00100400, 0xa10e0061, 0x3f810000,
    0x00039031, 0x00000000, 0x600e010c, 0x02000744, 0x617b0061, 0x00100500, 0x617c0061, 0x00100600,
    0x80033061, 0x7f050220, 0x00460105, 0x00000000, 0x80000101, 0x00000000, 0x00000000, 0x00000000,
    0x00030131, 0x00000004, 0x604e7f0c, 0x02007b24,
];

static HELIOV_TEXTURED_PS_SIMD16: [u32; 40] = [
    0x640b0061, 0x00100200, 0x640c0061, 0x00100400, 0x64000061, 0x00100300, 0x64010061, 0x00100500,
    0x80000101, 0x00000000, 0x00000000, 0x00000000, 0x0004005b, 0x020402a0, 0x02020664, 0x00050624,
    0x0004005b, 0x070402a0, 0x020206e4, 0x000506a4, 0x80000201, 0x00000000, 0x00000000, 0x00000000,
    0x2004005b, 0x060b0201, 0x2009025b, 0x060b0771, 0x00049031, 0x78440000, 0x20040414, 0x01000914,
    0x80002001, 0x00000000, 0x00000000, 0x00000000, 0x00040132, 0x00000004, 0x50007844, 0x00c40000,
];

// Diagnostic rung baked from apps/HelioV/src/texture_probe_load.wgsl. Unlike
// the material shader above, this emits `ld_lz`: no implicit derivatives and
// no filtering. Keeping it as a separate authenticated package lets a
// bare-metal run decide whether the failure is in sampled-surface access or in
// the higher-level filtered pixel contract.
static HELIOV_TEXEL_LOAD_PS_SIMD16: [u32; 64] = [
    0x641e0061, 0x00100200, 0x641f0061, 0x00100400, 0x64000061, 0x00100300, 0x64010061, 0x00100500,
    0x80000101, 0x00000000, 0x00000000, 0x00000000, 0x0004005b, 0x020402a0, 0x02020664, 0x00050624,
    0x0004005b, 0x070402a0, 0x020206e4, 0x000506a4, 0x80000201, 0x00000000, 0x00000000, 0x00000000,
    0x2004005b, 0x061e0201, 0x2009025b, 0x061e0771, 0x600b0243, 0x00100400, 0x600d0243, 0x00100900,
    0xe00f0241, 0x41800b00, 0xe0110241, 0x41800d00, 0xe5130262, 0xcf000f00, 0xe5150262, 0xcf001100,
    0x00040262, 0x17058aa0, 0x5a461305, 0x4effffff, 0x00040262, 0x19058aa0, 0x5a461505, 0x4effffff,
    0xe01b0261, 0x00101705, 0xe01d0261, 0x00101905, 0x00049031, 0x78440000, 0x20041b14, 0x01681d14,
    0x80002001, 0x00000000, 0x00000000, 0x00000000, 0x00040132, 0x00000004, 0x50007844, 0x00c40000,
];

static HELIOV_TEXTURED_PIPELINE: TrianglePipeline = TrianglePipeline {
    vs: TriangleVertexShader {
        code: &HELIOV_TEXTURED_VS,
        meta: TriangleVertexShaderMetadata {
            kernel: ShaderKernelMetadata {
                code_offset_bytes: 0,
                code_size_bytes: 176,
                code_alignment_bytes: 64,
                ksp_offset_bytes: 0,
                dispatch_mode: DispatchMode::Simd8,
                grf_start_register: 2,
                grf_used: 128,
                push_constant_bytes: 0,
                binding_table_entry_count: 0,
                sampler_count: 0,
            },
            max_threads: 64,
            urb_entry_output_length: 2,
        },
    },
    ps: TrianglePixelShader {
        code: &HELIOV_TEXTURED_PS_SIMD16,
        meta: TrianglePixelShaderMetadata {
            kernel: ShaderKernelMetadata {
                code_offset_bytes: 192,
                code_size_bytes: 160,
                code_alignment_bytes: 64,
                ksp_offset_bytes: 0,
                // ADL-S's established standalone pixel contract is one
                // SIMD16 executable in KSP0. The host bake emits both widths;
                // never select its SIMD8 companion for the physical 0x4680.
                dispatch_mode: DispatchMode::Simd16,
                grf_start_register: 2,
                grf_used: 128,
                push_constant_bytes: 0,
                binding_table_entry_count: 3,
                sampler_count: 1,
            },
            num_varying_inputs: 1,
            uses_vmask: true,
            computed_stencil: false,
            persample_dispatch: false,
            computed_depth_mode: 0,
            flat_inputs: 0,
        },
    },
};

pub(crate) fn heliov_textured_pipeline() -> &'static TrianglePipeline {
    &HELIOV_TEXTURED_PIPELINE
}

static HELIOV_TEXEL_LOAD_PIPELINE: TrianglePipeline = TrianglePipeline {
    vs: HELIOV_TEXTURED_PIPELINE.vs,
    ps: TrianglePixelShader {
        code: &HELIOV_TEXEL_LOAD_PS_SIMD16,
        meta: TrianglePixelShaderMetadata {
            kernel: ShaderKernelMetadata {
                code_offset_bytes: 192,
                code_size_bytes: 256,
                code_alignment_bytes: 64,
                ksp_offset_bytes: 0,
                dispatch_mode: DispatchMode::Simd16,
                grf_start_register: 2,
                grf_used: 128,
                push_constant_bytes: 0,
                binding_table_entry_count: 3,
                sampler_count: 1,
            },
            num_varying_inputs: 1,
            uses_vmask: true,
            computed_stencil: false,
            persample_dispatch: false,
            computed_depth_mode: 0,
            flat_inputs: 0,
        },
    },
};

pub(crate) fn heliov_texel_load_pipeline() -> &'static TrianglePipeline {
    &HELIOV_TEXEL_LOAD_PIPELINE
}

#[cfg(test)]
mod heliov_textured_pipeline_tests {
    use super::{DispatchMode, heliov_texel_load_pipeline, heliov_textured_pipeline};

    #[test]
    fn uses_the_adls_standalone_simd16_pixel_contract() {
        let pipeline = heliov_textured_pipeline();
        assert!(matches!(pipeline.ps.meta.kernel.dispatch_mode, DispatchMode::Simd16));
        assert_eq!(pipeline.ps.meta.kernel.code_size_bytes, 160);
        assert_eq!(pipeline.ps.meta.kernel.sampler_count, 1);
        assert_eq!(pipeline.ps.meta.kernel.binding_table_entry_count, 3);
    }

    #[test]
    fn fixed_texel_probe_is_a_distinct_simd16_sampler_package() {
        let filtered = heliov_textured_pipeline();
        let fixed = heliov_texel_load_pipeline();
        assert!(matches!(fixed.ps.meta.kernel.dispatch_mode, DispatchMode::Simd16));
        assert_eq!(fixed.ps.meta.kernel.code_size_bytes, 256);
        assert_eq!(fixed.ps.meta.kernel.sampler_count, 1);
        assert_eq!(fixed.ps.meta.kernel.binding_table_entry_count, 3);
        assert_ne!(fixed.ps.code, filtered.ps.code);
    }
}

pub(crate) fn triangle_pipeline_ps_eot() -> &'static TrianglePipeline {
    generated_triangle::triangle_pipeline_ps_eot()
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn triangle_pipeline_push_color() -> &'static TrianglePipeline {
    generated_triangle::triangle_pipeline_push_color()
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn triangle_pipeline_push_color_simd16() -> &'static TrianglePipeline {
    generated_triangle::triangle_pipeline_push_color_simd16()
}

pub(crate) fn triangle_pipeline_is_placeholder() -> bool {
    false
}

pub(crate) fn triangle_pipeline_note() -> &'static str {
    generated_triangle::TRIANGLE_PIPELINE_NOTE
}

pub(crate) fn triangle_pipeline_simd16_note() -> &'static str {
    generated_triangle::TRIANGLE_PIPELINE_SIMD16_NOTE
}

pub(crate) fn triangle_pipeline_ps_eot_note() -> &'static str {
    generated_triangle::TRIANGLE_PIPELINE_PS_EOT_NOTE
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn triangle_pipeline_push_color_note() -> &'static str {
    generated_triangle::TRIANGLE_PIPELINE_PUSH_COLOR_NOTE
}
