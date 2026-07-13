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

pub(crate) fn triangle_pipeline_ps_eot() -> &'static TrianglePipeline {
    generated_triangle::triangle_pipeline_ps_eot()
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
