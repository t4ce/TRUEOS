#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum DispatchMode {
    Simd8,
    Simd16,
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

pub(crate) const TRIANGLE_VERTEX_SOURCE_PATH: &str = "igc-or-spec:intel/render/triangle_vs";
pub(crate) const TRIANGLE_FRAGMENT_SOURCE_PATH: &str = "igc-or-spec:intel/render/triangle_ps";
pub(crate) const TRIANGLE_VERTEX_COMPONENTS: usize = 3;
pub(crate) const TRIANGLE_VERTEX_STRIDE_BYTES: usize =
    TRIANGLE_VERTEX_COMPONENTS * core::mem::size_of::<f32>();

const PLACEHOLDER_KERNEL: ShaderKernelMetadata = ShaderKernelMetadata {
    code_offset_bytes: 0,
    code_size_bytes: 0,
    code_alignment_bytes: 64,
    ksp_offset_bytes: 0,
    dispatch_mode: DispatchMode::Simd8,
    grf_start_register: 0,
    grf_used: 0,
    push_constant_bytes: 0,
    binding_table_entry_count: 0,
    sampler_count: 0,
};

static PLACEHOLDER_PIPELINE: TrianglePipeline = TrianglePipeline {
    vs: TriangleVertexShader {
        meta: TriangleVertexShaderMetadata {
            kernel: PLACEHOLDER_KERNEL,
            max_threads: 0,
            urb_entry_output_length: 0,
        },
        code: &[],
    },
    ps: TrianglePixelShader {
        meta: TrianglePixelShaderMetadata {
            kernel: PLACEHOLDER_KERNEL,
            num_varying_inputs: 0,
            uses_vmask: false,
            computed_stencil: false,
            persample_dispatch: false,
            computed_depth_mode: 0,
            flat_inputs: 0,
        },
        code: &[],
    },
};

pub(crate) fn triangle_pipeline() -> &'static TrianglePipeline {
    &PLACEHOLDER_PIPELINE
}

pub(crate) fn triangle_pipeline_is_placeholder() -> bool {
    true
}

pub(crate) fn triangle_pipeline_note() -> &'static str {
    "placeholder: IGC/spec-backed triangle VS/PS artifact is not wired"
}
