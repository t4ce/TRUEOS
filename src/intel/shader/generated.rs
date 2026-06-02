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

static TRIANGLE_VS_HDC_STORE_PROBE_CODE: [u32; 48] = [
    0x80030061, 0x04054660, 0x00000000, 0xC0DE772A, 0x80030061, 0x7F054220, 0x00000000, 0x00840058,
    0x00030131, 0x00000000, 0xCDFA7F0C, 0x009A040C, 0x00030061, 0x77054220, 0x00000000, 0x00000000,
    0x00030061, 0x78054220, 0x00000000, 0x00000000, 0x00030061, 0x79054220, 0x00000000, 0x00000000,
    0x00030061, 0x7A054220, 0x00000000, 0x00000000, 0x80030061, 0x7F050220, 0x00460105, 0x00000000,
    0x617B0061, 0x00100200, 0x617C0061, 0x00100300, 0x617D0061, 0x00100400, 0xA17E0061, 0x3F810000,
    0x80000101, 0x00000000, 0x00000000, 0x00000000, 0x00030131, 0x00000004, 0x600E7F0C, 0x02007744,
];

static TRIANGLE_VS_HDC_BTI34_STORE_PROBE_CODE: [u32; 48] = [
    0x80030061, 0x04054660, 0x00000000, 0xC0DE7734, 0x80030061, 0x7F054220, 0x00000000, 0x00000000,
    0x00030131, 0x00000000, 0xCC687F0C, 0x009A040C, 0x00030061, 0x77054220, 0x00000000, 0x00000000,
    0x00030061, 0x78054220, 0x00000000, 0x00000000, 0x00030061, 0x79054220, 0x00000000, 0x00000000,
    0x00030061, 0x7A054220, 0x00000000, 0x00000000, 0x80030061, 0x7F050220, 0x00460105, 0x00000000,
    0x617B0061, 0x00100200, 0x617C0061, 0x00100300, 0x617D0061, 0x00100400, 0xA17E0061, 0x3F810000,
    0x80000101, 0x00000000, 0x00000000, 0x00000000, 0x00030131, 0x00000004, 0x600E7F0C, 0x02007744,
];

static TRIANGLE_VS_HDC_STORE_TS_EOT_PROBE_CODE: [u32; 20] = [
    0x80030061, 0x04054660, 0x00000000, 0xC0DE7733, 0x80030061, 0x7F054220, 0x00000000, 0x00840058,
    0x00030131, 0x00000000, 0xCDFA7F0C, 0x009A040C, 0x80030061, 0x7E050220, 0x00460005, 0x00000000,
    0x80030131, 0x00000004, 0x70007E0C, 0x00000000,
];

static TRIANGLE_PDOANE_GEN7_PASSTHROUGH_VS_CODE: [u32; 20] = [
    0x00600201, 0x2FA003BD, 0x00200000, 0x00000000, 0x00000201, 0x2FB40061, 0x00000000, 0x0000FF00,
    0x00600001, 0x2FC00061, 0x00000000, 0x00000000, 0x00600101, 0x2FEF03BD, 0x006E0024, 0x00000000,
    0x06600031, 0x20001E3C, 0x00000FA0, 0x86084000,
];

static TRIANGLE_CONST_URB_VS_CODE: [u32; 44] = [
    0x00030061, 0x77054220, 0x00000000, 0x00000000, 0x00030061, 0x78054220, 0x00000000, 0x00000000,
    0x00030061, 0x79054220, 0x00000000, 0x00000000, 0x00030061, 0x7A054220, 0x00000000, 0x00000000,
    0x80030061, 0x7F050220, 0x00460105, 0x00000000, 0x00030061, 0x7B054AA0, 0x00000000, 0xBE800000,
    0x00030061, 0x7C054AA0, 0x00000000, 0xBE800000, 0x00030061, 0x7D054AA0, 0x00000000, 0x00000000,
    0x00030061, 0x7E054AA0, 0x00000000, 0x3F800000, 0x80000001, 0x00000000, 0x00000000, 0x00000000,
    0x00030131, 0x00000004, 0x600E7F0C, 0x02007744,
];

static TRIANGLE_CONST_URB_HANDLE_G0_VS_CODE: [u32; 44] = [
    0x00030061, 0x77054220, 0x00000000, 0x00000000, 0x00030061, 0x78054220, 0x00000000, 0x00000000,
    0x00030061, 0x79054220, 0x00000000, 0x00000000, 0x00030061, 0x7A054220, 0x00000000, 0x00000000,
    0x80030061, 0x7F050220, 0x00460005, 0x00000000, 0x00030061, 0x7B054AA0, 0x00000000, 0xBE800000,
    0x00030061, 0x7C054AA0, 0x00000000, 0xBE800000, 0x00030061, 0x7D054AA0, 0x00000000, 0x00000000,
    0x00030061, 0x7E054AA0, 0x00000000, 0x3F800000, 0x80000001, 0x00000000, 0x00000000, 0x00000000,
    0x00030031, 0x00000004, 0x600E7F0C, 0x02007744,
];

static TRIANGLE_CONST_URB_HANDLE_G1_VS_CODE: [u32; 44] = [
    0x00030061, 0x77054220, 0x00000000, 0x00000000, 0x00030061, 0x78054220, 0x00000000, 0x00000000,
    0x00030061, 0x79054220, 0x00000000, 0x00000000, 0x00030061, 0x7A054220, 0x00000000, 0x00000000,
    0x80030061, 0x7F050220, 0x00460105, 0x00000000, 0x00030061, 0x7B054AA0, 0x00000000, 0xBE800000,
    0x00030061, 0x7C054AA0, 0x00000000, 0xBE800000, 0x00030061, 0x7D054AA0, 0x00000000, 0x00000000,
    0x00030061, 0x7E054AA0, 0x00000000, 0x3F800000, 0x80000001, 0x00000000, 0x00000000, 0x00000000,
    0x00030031, 0x00000004, 0x600E7F0C, 0x02007744,
];

static TRIANGLE_CONST_URB_HANDLE_G2_VS_CODE: [u32; 44] = [
    0x00030061, 0x77054220, 0x00000000, 0x00000000, 0x00030061, 0x78054220, 0x00000000, 0x00000000,
    0x00030061, 0x79054220, 0x00000000, 0x00000000, 0x00030061, 0x7A054220, 0x00000000, 0x00000000,
    0x80030061, 0x7F050220, 0x00460205, 0x00000000, 0x00030061, 0x7B054AA0, 0x00000000, 0xBE800000,
    0x00030061, 0x7C054AA0, 0x00000000, 0xBE800000, 0x00030061, 0x7D054AA0, 0x00000000, 0x00000000,
    0x00030061, 0x7E054AA0, 0x00000000, 0x3F800000, 0x80000001, 0x00000000, 0x00000000, 0x00000000,
    0x00030031, 0x00000004, 0x600E7F0C, 0x02007744,
];

static TRIANGLE_CONST_URB_HANDLE_G3_VS_CODE: [u32; 44] = [
    0x00030061, 0x77054220, 0x00000000, 0x00000000, 0x00030061, 0x78054220, 0x00000000, 0x00000000,
    0x00030061, 0x79054220, 0x00000000, 0x00000000, 0x00030061, 0x7A054220, 0x00000000, 0x00000000,
    0x80030061, 0x7F050220, 0x00460305, 0x00000000, 0x00030061, 0x7B054AA0, 0x00000000, 0xBE800000,
    0x00030061, 0x7C054AA0, 0x00000000, 0xBE800000, 0x00030061, 0x7D054AA0, 0x00000000, 0x00000000,
    0x00030061, 0x7E054AA0, 0x00000000, 0x3F800000, 0x80000001, 0x00000000, 0x00000000, 0x00000000,
    0x00030031, 0x00000004, 0x600E7F0C, 0x02007744,
];

static TRIANGLE_CONST_URB_HANDLE_G4_VS_CODE: [u32; 44] = [
    0x00030061, 0x77054220, 0x00000000, 0x00000000, 0x00030061, 0x78054220, 0x00000000, 0x00000000,
    0x00030061, 0x79054220, 0x00000000, 0x00000000, 0x00030061, 0x7A054220, 0x00000000, 0x00000000,
    0x80030061, 0x7F050220, 0x00460405, 0x00000000, 0x00030061, 0x7B054AA0, 0x00000000, 0xBE800000,
    0x00030061, 0x7C054AA0, 0x00000000, 0xBE800000, 0x00030061, 0x7D054AA0, 0x00000000, 0x00000000,
    0x00030061, 0x7E054AA0, 0x00000000, 0x3F800000, 0x80000001, 0x00000000, 0x00000000, 0x00000000,
    0x00030031, 0x00000004, 0x600E7F0C, 0x02007744,
];

static TRIANGLE_CONST_URB_HANDLE_G5_VS_CODE: [u32; 44] = [
    0x00030061, 0x77054220, 0x00000000, 0x00000000, 0x00030061, 0x78054220, 0x00000000, 0x00000000,
    0x00030061, 0x79054220, 0x00000000, 0x00000000, 0x00030061, 0x7A054220, 0x00000000, 0x00000000,
    0x80030061, 0x7F050220, 0x00460505, 0x00000000, 0x00030061, 0x7B054AA0, 0x00000000, 0xBE800000,
    0x00030061, 0x7C054AA0, 0x00000000, 0xBE800000, 0x00030061, 0x7D054AA0, 0x00000000, 0x00000000,
    0x00030061, 0x7E054AA0, 0x00000000, 0x3F800000, 0x80000001, 0x00000000, 0x00000000, 0x00000000,
    0x00030031, 0x00000004, 0x600E7F0C, 0x02007744,
];

static TRIANGLE_CONST_URB_HANDLE_G6_VS_CODE: [u32; 44] = [
    0x00030061, 0x77054220, 0x00000000, 0x00000000, 0x00030061, 0x78054220, 0x00000000, 0x00000000,
    0x00030061, 0x79054220, 0x00000000, 0x00000000, 0x00030061, 0x7A054220, 0x00000000, 0x00000000,
    0x80030061, 0x7F050220, 0x00460605, 0x00000000, 0x00030061, 0x7B054AA0, 0x00000000, 0xBE800000,
    0x00030061, 0x7C054AA0, 0x00000000, 0xBE800000, 0x00030061, 0x7D054AA0, 0x00000000, 0x00000000,
    0x00030061, 0x7E054AA0, 0x00000000, 0x3F800000, 0x80000001, 0x00000000, 0x00000000, 0x00000000,
    0x00030031, 0x00000004, 0x600E7F0C, 0x02007744,
];

static TRIANGLE_CONST_URB_HANDLE_G7_VS_CODE: [u32; 44] = [
    0x00030061, 0x77054220, 0x00000000, 0x00000000, 0x00030061, 0x78054220, 0x00000000, 0x00000000,
    0x00030061, 0x79054220, 0x00000000, 0x00000000, 0x00030061, 0x7A054220, 0x00000000, 0x00000000,
    0x80030061, 0x7F050220, 0x00460705, 0x00000000, 0x00030061, 0x7B054AA0, 0x00000000, 0xBE800000,
    0x00030061, 0x7C054AA0, 0x00000000, 0xBE800000, 0x00030061, 0x7D054AA0, 0x00000000, 0x00000000,
    0x00030061, 0x7E054AA0, 0x00000000, 0x3F800000, 0x80000001, 0x00000000, 0x00000000, 0x00000000,
    0x00030031, 0x00000004, 0x600E7F0C, 0x02007744,
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
            accesses_uav: false,
            push_constant_bytes: 0,
            grf_used: 128,
        },
        urb_entry_output_length: 1,
        max_threads: 64,
    },
};

static TRIANGLE_VS_HDC_STORE_PROBE: BakedVertexShader = BakedVertexShader {
    code: &TRIANGLE_VS_HDC_STORE_PROBE_CODE,
    meta: VertexShaderMetadata {
        kernel: ShaderKernelMetadata {
            ksp_offset_bytes: 0,
            code_offset_bytes: 64,
            code_size_bytes: 192,
            code_alignment_bytes: 64,
            grf_start_register: 0,
            dispatch_mode: DispatchMode::Simd8,
            sampler_count: 0,
            binding_table_entry_count: 0,
            accesses_uav: true,
            push_constant_bytes: 0,
            grf_used: 128,
        },
        urb_entry_output_length: 1,
        max_threads: 64,
    },
};

static TRIANGLE_VS_HDC_BTI34_STORE_PROBE: BakedVertexShader = BakedVertexShader {
    code: &TRIANGLE_VS_HDC_BTI34_STORE_PROBE_CODE,
    meta: VertexShaderMetadata {
        kernel: ShaderKernelMetadata {
            ksp_offset_bytes: 0,
            code_offset_bytes: 64,
            code_size_bytes: 192,
            code_alignment_bytes: 64,
            grf_start_register: 0,
            dispatch_mode: DispatchMode::Simd8,
            sampler_count: 0,
            binding_table_entry_count: 0x35,
            accesses_uav: true,
            push_constant_bytes: 0,
            grf_used: 128,
        },
        urb_entry_output_length: 1,
        max_threads: 64,
    },
};

static TRIANGLE_VS_HDC_STORE_TS_EOT_PROBE: BakedVertexShader = BakedVertexShader {
    code: &TRIANGLE_VS_HDC_STORE_TS_EOT_PROBE_CODE,
    meta: VertexShaderMetadata {
        kernel: ShaderKernelMetadata {
            ksp_offset_bytes: 0,
            code_offset_bytes: 64,
            code_size_bytes: 80,
            code_alignment_bytes: 64,
            grf_start_register: 0,
            dispatch_mode: DispatchMode::Simd8,
            sampler_count: 0,
            binding_table_entry_count: 0,
            accesses_uav: true,
            push_constant_bytes: 0,
            grf_used: 128,
        },
        urb_entry_output_length: 1,
        max_threads: 64,
    },
};

static TRIANGLE_PDOANE_GEN7_PASSTHROUGH_VS: BakedVertexShader = BakedVertexShader {
    code: &TRIANGLE_PDOANE_GEN7_PASSTHROUGH_VS_CODE,
    meta: VertexShaderMetadata {
        kernel: ShaderKernelMetadata {
            ksp_offset_bytes: 0,
            code_offset_bytes: 64,
            code_size_bytes: 80,
            code_alignment_bytes: 64,
            grf_start_register: 1,
            dispatch_mode: DispatchMode::Simd8,
            sampler_count: 0,
            binding_table_entry_count: 0,
            accesses_uav: false,
            push_constant_bytes: 0,
            grf_used: 128,
        },
        urb_entry_output_length: 1,
        max_threads: 2,
    },
};

static TRIANGLE_CONST_URB_VS: BakedVertexShader = BakedVertexShader {
    code: &TRIANGLE_CONST_URB_VS_CODE,
    meta: VertexShaderMetadata {
        kernel: ShaderKernelMetadata {
            ksp_offset_bytes: 0,
            code_offset_bytes: 64,
            code_size_bytes: 176,
            code_alignment_bytes: 64,
            grf_start_register: 0,
            dispatch_mode: DispatchMode::Simd8,
            sampler_count: 0,
            binding_table_entry_count: 0,
            accesses_uav: false,
            push_constant_bytes: 0,
            grf_used: 128,
        },
        urb_entry_output_length: 1,
        max_threads: 64,
    },
};

static TRIANGLE_CONST_URB_KSP0_VS: BakedVertexShader = BakedVertexShader {
    code: &TRIANGLE_CONST_URB_VS_CODE,
    meta: VertexShaderMetadata {
        kernel: ShaderKernelMetadata {
            ksp_offset_bytes: 0,
            code_offset_bytes: 0,
            code_size_bytes: 176,
            code_alignment_bytes: 64,
            grf_start_register: 0,
            dispatch_mode: DispatchMode::Simd8,
            sampler_count: 0,
            binding_table_entry_count: 0,
            accesses_uav: false,
            push_constant_bytes: 0,
            grf_used: 128,
        },
        urb_entry_output_length: 1,
        max_threads: 64,
    },
};

static TRIANGLE_CONST_URB_HANDLE_G0_VS: BakedVertexShader = BakedVertexShader {
    code: &TRIANGLE_CONST_URB_HANDLE_G0_VS_CODE,
    meta: TRIANGLE_CONST_URB_VS.meta,
};

static TRIANGLE_CONST_URB_HANDLE_G1_VS: BakedVertexShader = BakedVertexShader {
    code: &TRIANGLE_CONST_URB_HANDLE_G1_VS_CODE,
    meta: TRIANGLE_CONST_URB_VS.meta,
};

static TRIANGLE_CONST_URB_HANDLE_G2_VS: BakedVertexShader = BakedVertexShader {
    code: &TRIANGLE_CONST_URB_HANDLE_G2_VS_CODE,
    meta: TRIANGLE_CONST_URB_VS.meta,
};

static TRIANGLE_CONST_URB_HANDLE_G3_VS: BakedVertexShader = BakedVertexShader {
    code: &TRIANGLE_CONST_URB_HANDLE_G3_VS_CODE,
    meta: TRIANGLE_CONST_URB_VS.meta,
};

static TRIANGLE_CONST_URB_HANDLE_G4_VS: BakedVertexShader = BakedVertexShader {
    code: &TRIANGLE_CONST_URB_HANDLE_G4_VS_CODE,
    meta: TRIANGLE_CONST_URB_VS.meta,
};

static TRIANGLE_CONST_URB_HANDLE_G5_VS: BakedVertexShader = BakedVertexShader {
    code: &TRIANGLE_CONST_URB_HANDLE_G5_VS_CODE,
    meta: TRIANGLE_CONST_URB_VS.meta,
};

static TRIANGLE_CONST_URB_HANDLE_G6_VS: BakedVertexShader = BakedVertexShader {
    code: &TRIANGLE_CONST_URB_HANDLE_G6_VS_CODE,
    meta: TRIANGLE_CONST_URB_VS.meta,
};

static TRIANGLE_CONST_URB_HANDLE_G7_VS: BakedVertexShader = BakedVertexShader {
    code: &TRIANGLE_CONST_URB_HANDLE_G7_VS_CODE,
    meta: TRIANGLE_CONST_URB_VS.meta,
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
            accesses_uav: false,
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

static TRIANGLE_HDC_PROBE_PS: BakedFragmentShader = BakedFragmentShader {
    code: &TRIANGLE_PS_CODE,
    meta: FragmentShaderMetadata {
        kernel: ShaderKernelMetadata {
            ksp_offset_bytes: 0,
            code_offset_bytes: 320,
            code_size_bytes: 48,
            code_alignment_bytes: 64,
            grf_start_register: 0,
            dispatch_mode: DispatchMode::Simd8,
            sampler_count: 0,
            binding_table_entry_count: 1,
            accesses_uav: false,
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

static TRIANGLE_HDC_VS_STORE_PROBE_PIPELINE: TrianglePipeline = TrianglePipeline {
    vs: &TRIANGLE_VS_HDC_STORE_PROBE,
    ps: &TRIANGLE_HDC_PROBE_PS,
    vertex_stride_bytes: TRIANGLE_VERTEX_STRIDE_BYTES as u32,
    vertex_count: 3,
    rt_binding_table_index: 0,
};

static TRIANGLE_HDC_VS_BTI34_STORE_PROBE_PIPELINE: TrianglePipeline = TrianglePipeline {
    vs: &TRIANGLE_VS_HDC_BTI34_STORE_PROBE,
    ps: &TRIANGLE_HDC_PROBE_PS,
    vertex_stride_bytes: TRIANGLE_VERTEX_STRIDE_BYTES as u32,
    vertex_count: 3,
    rt_binding_table_index: 0,
};

static TRIANGLE_HDC_VS_STORE_TS_EOT_PROBE_PIPELINE: TrianglePipeline = TrianglePipeline {
    vs: &TRIANGLE_VS_HDC_STORE_TS_EOT_PROBE,
    ps: &TRIANGLE_HDC_PROBE_PS,
    vertex_stride_bytes: TRIANGLE_VERTEX_STRIDE_BYTES as u32,
    vertex_count: 3,
    rt_binding_table_index: 0,
};

static TRIANGLE_PDOANE_GEN7_PASSTHROUGH_PROBE_PIPELINE: TrianglePipeline = TrianglePipeline {
    vs: &TRIANGLE_PDOANE_GEN7_PASSTHROUGH_VS,
    ps: &TRIANGLE_HDC_PROBE_PS,
    vertex_stride_bytes: TRIANGLE_VERTEX_STRIDE_BYTES as u32,
    vertex_count: 3,
    rt_binding_table_index: 0,
};

static TRIANGLE_CONST_URB_VS_PROBE_PIPELINE: TrianglePipeline = TrianglePipeline {
    vs: &TRIANGLE_CONST_URB_VS,
    ps: &TRIANGLE_HDC_PROBE_PS,
    vertex_stride_bytes: TRIANGLE_VERTEX_STRIDE_BYTES as u32,
    vertex_count: 3,
    rt_binding_table_index: 0,
};

static TRIANGLE_CONST_URB_KSP0_VS_PROBE_PIPELINE: TrianglePipeline = TrianglePipeline {
    vs: &TRIANGLE_CONST_URB_KSP0_VS,
    ps: &TRIANGLE_HDC_PROBE_PS,
    vertex_stride_bytes: TRIANGLE_VERTEX_STRIDE_BYTES as u32,
    vertex_count: 3,
    rt_binding_table_index: 0,
};

static TRIANGLE_CONST_URB_HANDLE_G0_PROBE_PIPELINE: TrianglePipeline = TrianglePipeline {
    vs: &TRIANGLE_CONST_URB_HANDLE_G0_VS,
    ps: &TRIANGLE_HDC_PROBE_PS,
    vertex_stride_bytes: TRIANGLE_VERTEX_STRIDE_BYTES as u32,
    vertex_count: 3,
    rt_binding_table_index: 0,
};

static TRIANGLE_CONST_URB_HANDLE_G1_PROBE_PIPELINE: TrianglePipeline = TrianglePipeline {
    vs: &TRIANGLE_CONST_URB_HANDLE_G1_VS,
    ps: &TRIANGLE_HDC_PROBE_PS,
    vertex_stride_bytes: TRIANGLE_VERTEX_STRIDE_BYTES as u32,
    vertex_count: 3,
    rt_binding_table_index: 0,
};

static TRIANGLE_CONST_URB_HANDLE_G2_PROBE_PIPELINE: TrianglePipeline = TrianglePipeline {
    vs: &TRIANGLE_CONST_URB_HANDLE_G2_VS,
    ps: &TRIANGLE_HDC_PROBE_PS,
    vertex_stride_bytes: TRIANGLE_VERTEX_STRIDE_BYTES as u32,
    vertex_count: 3,
    rt_binding_table_index: 0,
};

static TRIANGLE_CONST_URB_HANDLE_G3_PROBE_PIPELINE: TrianglePipeline = TrianglePipeline {
    vs: &TRIANGLE_CONST_URB_HANDLE_G3_VS,
    ps: &TRIANGLE_HDC_PROBE_PS,
    vertex_stride_bytes: TRIANGLE_VERTEX_STRIDE_BYTES as u32,
    vertex_count: 3,
    rt_binding_table_index: 0,
};

static TRIANGLE_CONST_URB_HANDLE_G4_PROBE_PIPELINE: TrianglePipeline = TrianglePipeline {
    vs: &TRIANGLE_CONST_URB_HANDLE_G4_VS,
    ps: &TRIANGLE_HDC_PROBE_PS,
    vertex_stride_bytes: TRIANGLE_VERTEX_STRIDE_BYTES as u32,
    vertex_count: 3,
    rt_binding_table_index: 0,
};

static TRIANGLE_CONST_URB_HANDLE_G5_PROBE_PIPELINE: TrianglePipeline = TrianglePipeline {
    vs: &TRIANGLE_CONST_URB_HANDLE_G5_VS,
    ps: &TRIANGLE_HDC_PROBE_PS,
    vertex_stride_bytes: TRIANGLE_VERTEX_STRIDE_BYTES as u32,
    vertex_count: 3,
    rt_binding_table_index: 0,
};

static TRIANGLE_CONST_URB_HANDLE_G6_PROBE_PIPELINE: TrianglePipeline = TrianglePipeline {
    vs: &TRIANGLE_CONST_URB_HANDLE_G6_VS,
    ps: &TRIANGLE_HDC_PROBE_PS,
    vertex_stride_bytes: TRIANGLE_VERTEX_STRIDE_BYTES as u32,
    vertex_count: 3,
    rt_binding_table_index: 0,
};

static TRIANGLE_CONST_URB_HANDLE_G7_PROBE_PIPELINE: TrianglePipeline = TrianglePipeline {
    vs: &TRIANGLE_CONST_URB_HANDLE_G7_VS,
    ps: &TRIANGLE_HDC_PROBE_PS,
    vertex_stride_bytes: TRIANGLE_VERTEX_STRIDE_BYTES as u32,
    vertex_count: 3,
    rt_binding_table_index: 0,
};

pub(crate) fn triangle_pipeline() -> &'static TrianglePipeline {
    &TRIANGLE_PIPELINE
}

pub(crate) fn triangle_hdc_vs_store_probe_pipeline() -> &'static TrianglePipeline {
    &TRIANGLE_HDC_VS_STORE_PROBE_PIPELINE
}

pub(crate) fn triangle_hdc_vs_bti34_store_probe_pipeline() -> &'static TrianglePipeline {
    &TRIANGLE_HDC_VS_BTI34_STORE_PROBE_PIPELINE
}

pub(crate) fn triangle_hdc_vs_store_ts_eot_probe_pipeline() -> &'static TrianglePipeline {
    &TRIANGLE_HDC_VS_STORE_TS_EOT_PROBE_PIPELINE
}

pub(crate) fn triangle_pdoane_gen7_passthrough_probe_pipeline() -> &'static TrianglePipeline {
    &TRIANGLE_PDOANE_GEN7_PASSTHROUGH_PROBE_PIPELINE
}

pub(crate) fn triangle_const_urb_vs_probe_pipeline() -> &'static TrianglePipeline {
    &TRIANGLE_CONST_URB_VS_PROBE_PIPELINE
}

pub(crate) fn triangle_const_urb_ksp0_vs_probe_pipeline() -> &'static TrianglePipeline {
    &TRIANGLE_CONST_URB_KSP0_VS_PROBE_PIPELINE
}

pub(crate) fn triangle_const_urb_handle_g0_probe_pipeline() -> &'static TrianglePipeline {
    &TRIANGLE_CONST_URB_HANDLE_G0_PROBE_PIPELINE
}

pub(crate) fn triangle_const_urb_handle_g1_probe_pipeline() -> &'static TrianglePipeline {
    &TRIANGLE_CONST_URB_HANDLE_G1_PROBE_PIPELINE
}

pub(crate) fn triangle_const_urb_handle_g2_probe_pipeline() -> &'static TrianglePipeline {
    &TRIANGLE_CONST_URB_HANDLE_G2_PROBE_PIPELINE
}

pub(crate) fn triangle_const_urb_handle_g3_probe_pipeline() -> &'static TrianglePipeline {
    &TRIANGLE_CONST_URB_HANDLE_G3_PROBE_PIPELINE
}

pub(crate) fn triangle_const_urb_handle_g4_probe_pipeline() -> &'static TrianglePipeline {
    &TRIANGLE_CONST_URB_HANDLE_G4_PROBE_PIPELINE
}

pub(crate) fn triangle_const_urb_handle_g5_probe_pipeline() -> &'static TrianglePipeline {
    &TRIANGLE_CONST_URB_HANDLE_G5_PROBE_PIPELINE
}

pub(crate) fn triangle_const_urb_handle_g6_probe_pipeline() -> &'static TrianglePipeline {
    &TRIANGLE_CONST_URB_HANDLE_G6_PROBE_PIPELINE
}

pub(crate) fn triangle_const_urb_handle_g7_probe_pipeline() -> &'static TrianglePipeline {
    &TRIANGLE_CONST_URB_HANDLE_G7_PROBE_PIPELINE
}
