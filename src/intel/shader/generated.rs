use super::{
    BakedFragmentShader, BakedVertexShader, DispatchMode, FragmentShaderMetadata,
    ShaderKernelMetadata, TRIANGLE_VERTEX_STRIDE_BYTES, TrianglePipeline, VertexShaderMetadata,
};

// @generated from tools/xe_lp_shader_bake/simple_triangle_dump.c host dump.
// See src/intel/shader/bake_format.md for the runtime contract.

pub(crate) const TRIANGLE_PIPELINE_NOTE: &str = "mesa-intel-vulkan simple-triangle dump target=gfx125 provisional=1 vs_sha=d648c75e7e36bc926b927c3700bd514f81d286db ps_sha=81edb0a9ed24ccfdfb1a2c3202f1008b202868df verified=0";

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

static TRIANGLE_VS: BakedVertexShader = BakedVertexShader {
    code: &TRIANGLE_VS_CODE,
    meta: VertexShaderMetadata {
        kernel: ShaderKernelMetadata {
            ksp_offset_bytes: 0,
            code_offset_bytes: 0,
            code_size_bytes: 144,
            code_alignment_bytes: 64,
            grf_start_register: 0,
            dispatch_mode: DispatchMode::Simd8,
            sampler_count: 0,
            binding_table_entry_count: 0,
            push_constant_bytes: 0,
            grf_used: 128,
        },
        urb_entry_output_length: 1,
        max_threads: 64,
    },
};

static TRIANGLE_PS: BakedFragmentShader = BakedFragmentShader {
    code: &TRIANGLE_PS_CODE,
    meta: FragmentShaderMetadata {
        kernel: ShaderKernelMetadata {
            ksp_offset_bytes: 0,
            code_offset_bytes: 192,
            code_size_bytes: 48,
            code_alignment_bytes: 64,
            grf_start_register: 0,
            dispatch_mode: DispatchMode::Simd8,
            sampler_count: 0,
            binding_table_entry_count: 1,
            push_constant_bytes: 0,
            grf_used: 128,
        },
        num_varying_inputs: 0,
        flat_inputs: 0,
        uses_vmask: false,
        computed_depth_mode: 0,
        computed_stencil: false,
        persample_dispatch: false,
    },
};

static TRIANGLE_PIPELINE: TrianglePipeline = TrianglePipeline {
    vs: &TRIANGLE_VS,
    ps: &TRIANGLE_PS,
    vertex_stride_bytes: TRIANGLE_VERTEX_STRIDE_BYTES as u32,
    vertex_count: 3,
    rt_binding_table_index: 0,
};

pub(crate) fn triangle_pipeline() -> &'static TrianglePipeline {
    &TRIANGLE_PIPELINE
}
