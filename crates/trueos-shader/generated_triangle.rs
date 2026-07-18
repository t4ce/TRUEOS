use super::{
    DispatchMode, ShaderKernelMetadata, TrianglePipeline, TrianglePixelShader,
    TrianglePixelShaderMetadata, TriangleVertexShader, TriangleVertexShaderMetadata,
};

// @generated from crates/trueos-shader/host_shader_validation/cache_extract.
// Runtime contract: VS emits clip-space position only; PS writes a constant color to RT0.

pub(crate) const TRIANGLE_PIPELINE_NOTE: &str = "mesa-intel-vulkan simple-triangle dump target=gfx125 provisional=1 vs_sha=d648c75e7e36bc926b927c3700bd514f81d286db ps_sha=81edb0a9ed24ccfdfb1a2c3202f1008b202868df verified=0";
pub(crate) const TRIANGLE_PIPELINE_SIMD16_NOTE: &str = "mesa-intel-vulkan simple-triangle dump target=gfx125 provisional=1 ps_variant=simd16 vs_sha=d648c75e7e36bc926b927c3700bd514f81d286db ps_sha=eb4817ff5338fb86574e325ff857d41228bc244a verified=0";
pub(crate) const TRIANGLE_PIPELINE_PS_EOT_NOTE: &str = "trueos ps launch probe target=gfx125 ps_variant=simd8-ts-eot-only source=crates/trueos-shader/gfx12_eot_g126_tgl.hex verified=0";
pub(crate) const TRIANGLE_PIPELINE_PUSH_COLOR_NOTE: &str = "mesa-intel-vulkan push-color target=gfx125 ps_variant=simd8 push_bytes=16 source=crates/trueos-shader/xe_lp_shader_bake/push_color.frag host_verified=1";

static TRIANGLE_VS_CODE: [u32; 36] = [
    0x00030061, 0x77054220, 0x00000000, 0x00000000, 0x00030061, 0x78054220, 0x00000000, 0x00000000,
    0x00030061, 0x79054220, 0x00000000, 0x00000000, 0x00030061, 0x7A054220, 0x00000000, 0x00000000,
    0x80030061, 0x7F050220, 0x00460105, 0x00000000, 0x617B0061, 0x00100200, 0x617C0061, 0x00100300,
    0x617D0061, 0x00100400, 0xA17E0061, 0x3F810000, 0x80000101, 0x00000000, 0x00000000, 0x00000000,
    0x00030131, 0x00000004, 0x600E7F0C, 0x02007744,
];

static TRIANGLE_PS_CODE: [u32; 12] = [
    0xA07E0061, 0x00010000, 0xA0780061, 0x3E810000, 0xA07A0061, 0x3F810000, 0xA07C0061, 0x3F810000,
    0x00040132, 0x00000004, 0x50007E14, 0x00C47834,
];

static TRIANGLE_PS_SIMD16_CODE: [u32; 12] = [
    0xA17F0061, 0x00010000, 0xA17C0061, 0x3E810000, 0xA17D0061, 0x3F810000, 0xA17E0061, 0x3F810000,
    0x00030132, 0x00000004, 0x58007F0C, 0x00C47C1C,
];

static TRIANGLE_PS_EOT_CODE: [u32; 8] = [
    0x80030061, 0x7E050220, 0x00460005, 0x00000000, 0x80030131, 0x00000004, 0x70007E0C, 0x00000000,
];

// Static pixel shader. g2 contains the first 16 bytes of Vulkan/graphics push
// data as four full-precision floats. Unlike TRIANGLE_PS_CODE, this code is
// never rewritten to bind a draw color.
static TRIANGLE_PS_PUSH_COLOR_CODE: [u32; 12] = [
    0x617C_0061,
    0x0011_0200,
    0x617D_0061,
    0x0011_0220,
    0x617E_0061,
    0x0011_0240,
    0x617F_0061,
    0x0011_0278,
    0x0003_0132,
    0x0000_0004,
    0x5800_7C24,
    0x00C4_0000,
];

static TRIANGLE_PS_PUSH_COLOR_SIMD16_CODE: [u32; 12] = [
    0x6078_0061,
    0x0011_0200,
    0x607A_0061,
    0x0011_0220,
    0x607C_0061,
    0x0011_0240,
    0x607E_0061,
    0x0011_0278,
    0x0004_0132,
    0x0000_0004,
    0x5000_7844,
    0x00C4_0000,
];

static TRIANGLE_PIPELINE: TrianglePipeline = TrianglePipeline {
    vs: TriangleVertexShader {
        code: &TRIANGLE_VS_CODE,
        meta: TriangleVertexShaderMetadata {
            kernel: ShaderKernelMetadata {
                code_offset_bytes: 0,
                code_size_bytes: 144,
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
            urb_entry_output_length: 1,
        },
    },
    ps: TrianglePixelShader {
        code: &TRIANGLE_PS_CODE,
        meta: TrianglePixelShaderMetadata {
            kernel: ShaderKernelMetadata {
                code_offset_bytes: 256,
                code_size_bytes: 48,
                code_alignment_bytes: 64,
                ksp_offset_bytes: 0,
                dispatch_mode: DispatchMode::Simd8,
                grf_start_register: 2,
                grf_used: 128,
                push_constant_bytes: 0,
                binding_table_entry_count: 1,
                sampler_count: 0,
            },
            num_varying_inputs: 0,
            uses_vmask: true,
            computed_stencil: false,
            persample_dispatch: false,
            computed_depth_mode: 0,
            flat_inputs: 0,
        },
    },
};

static TRIANGLE_PIPELINE_SIMD16: TrianglePipeline = TrianglePipeline {
    vs: TRIANGLE_PIPELINE.vs,
    ps: TrianglePixelShader {
        code: &TRIANGLE_PS_SIMD16_CODE,
        meta: TrianglePixelShaderMetadata {
            kernel: ShaderKernelMetadata {
                code_offset_bytes: 192,
                code_size_bytes: 48,
                code_alignment_bytes: 64,
                ksp_offset_bytes: 0,
                dispatch_mode: DispatchMode::Simd16,
                grf_start_register: 2,
                grf_used: 128,
                push_constant_bytes: 0,
                binding_table_entry_count: 1,
                sampler_count: 0,
            },
            num_varying_inputs: 0,
            uses_vmask: true,
            computed_stencil: false,
            persample_dispatch: false,
            computed_depth_mode: 0,
            flat_inputs: 0,
        },
    },
};

static TRIANGLE_PIPELINE_PS_EOT: TrianglePipeline = TrianglePipeline {
    vs: TRIANGLE_PIPELINE.vs,
    ps: TrianglePixelShader {
        code: &TRIANGLE_PS_EOT_CODE,
        meta: TrianglePixelShaderMetadata {
            kernel: ShaderKernelMetadata {
                code_offset_bytes: 192,
                code_size_bytes: 32,
                code_alignment_bytes: 64,
                ksp_offset_bytes: 0,
                dispatch_mode: DispatchMode::Simd8,
                grf_start_register: 0,
                grf_used: 128,
                push_constant_bytes: 0,
                binding_table_entry_count: 0,
                sampler_count: 0,
            },
            num_varying_inputs: 0,
            uses_vmask: false,
            computed_stencil: false,
            persample_dispatch: false,
            computed_depth_mode: 0,
            flat_inputs: 0,
        },
    },
};

static TRIANGLE_PIPELINE_PUSH_COLOR: TrianglePipeline = TrianglePipeline {
    vs: TRIANGLE_PIPELINE.vs,
    ps: TrianglePixelShader {
        code: &TRIANGLE_PS_PUSH_COLOR_CODE,
        meta: TrianglePixelShaderMetadata {
            kernel: ShaderKernelMetadata {
                code_offset_bytes: 256,
                code_size_bytes: 48,
                code_alignment_bytes: 64,
                ksp_offset_bytes: 0,
                dispatch_mode: DispatchMode::Simd8,
                grf_start_register: 2,
                grf_used: 128,
                push_constant_bytes: 16,
                binding_table_entry_count: 1,
                sampler_count: 0,
            },
            num_varying_inputs: 0,
            uses_vmask: false,
            computed_stencil: false,
            persample_dispatch: false,
            computed_depth_mode: 0,
            flat_inputs: 0,
        },
    },
};

static TRIANGLE_PIPELINE_PUSH_COLOR_SIMD16: TrianglePipeline = TrianglePipeline {
    vs: TRIANGLE_PIPELINE.vs,
    ps: TrianglePixelShader {
        code: &TRIANGLE_PS_PUSH_COLOR_SIMD16_CODE,
        meta: TrianglePixelShaderMetadata {
            kernel: ShaderKernelMetadata {
                code_offset_bytes: 192,
                code_size_bytes: 48,
                code_alignment_bytes: 64,
                ksp_offset_bytes: 0,
                dispatch_mode: DispatchMode::Simd16,
                grf_start_register: 2,
                grf_used: 128,
                push_constant_bytes: 16,
                binding_table_entry_count: 1,
                sampler_count: 0,
            },
            num_varying_inputs: 0,
            uses_vmask: false,
            computed_stencil: false,
            persample_dispatch: false,
            computed_depth_mode: 0,
            flat_inputs: 0,
        },
    },
};

pub(crate) fn triangle_pipeline() -> &'static TrianglePipeline {
    &TRIANGLE_PIPELINE
}

pub(crate) fn triangle_pipeline_simd16() -> &'static TrianglePipeline {
    &TRIANGLE_PIPELINE_SIMD16
}

pub(crate) fn triangle_pipeline_ps_eot() -> &'static TrianglePipeline {
    &TRIANGLE_PIPELINE_PS_EOT
}

pub(crate) fn triangle_pipeline_push_color() -> &'static TrianglePipeline {
    &TRIANGLE_PIPELINE_PUSH_COLOR
}

pub(crate) fn triangle_pipeline_push_color_simd16() -> &'static TrianglePipeline {
    &TRIANGLE_PIPELINE_PUSH_COLOR_SIMD16
}
