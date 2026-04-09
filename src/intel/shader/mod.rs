mod generated;

pub(crate) const TRIANGLE_VERTEX_SOURCE_PATH: &str = "src/intel/shader/triangle.vert";
pub(crate) const TRIANGLE_FRAGMENT_SOURCE_PATH: &str = "src/intel/shader/triangle.frag";
pub(crate) const TRIANGLE_VERTEX_SOURCE: &str = include_str!("triangle.vert");
pub(crate) const TRIANGLE_FRAGMENT_SOURCE: &str = include_str!("triangle.frag");
pub(crate) const TRIANGLE_VERTEX_COMPONENTS: usize = 3;
pub(crate) const TRIANGLE_VERTEX_STRIDE_BYTES: usize =
    TRIANGLE_VERTEX_COMPONENTS * core::mem::size_of::<f32>();

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum DispatchMode {
    Simd8,
    Simd16,
    Simd32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct ShaderKernelMetadata {
    pub(crate) ksp_offset_bytes: u32,
    pub(crate) prog_offset_bytes: u32,
    pub(crate) grf_start_register: u8,
    pub(crate) dispatch_mode: DispatchMode,
    pub(crate) sampler_count: u8,
    pub(crate) binding_table_entry_count: u8,
    pub(crate) push_constant_bytes: u16,
    pub(crate) grf_used: u16,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct VertexShaderMetadata {
    pub(crate) kernel: ShaderKernelMetadata,
    pub(crate) urb_entry_output_length: u8,
    pub(crate) max_threads: u16,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct FragmentShaderMetadata {
    pub(crate) kernel: ShaderKernelMetadata,
    pub(crate) num_varying_inputs: u8,
    pub(crate) flat_inputs: u32,
    pub(crate) uses_vmask: bool,
    pub(crate) computed_depth_mode: u8,
    pub(crate) computed_stencil: bool,
    pub(crate) persample_dispatch: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct BakedVertexShader {
    pub(crate) code: &'static [u32],
    pub(crate) meta: VertexShaderMetadata,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct BakedFragmentShader {
    pub(crate) code: &'static [u32],
    pub(crate) meta: FragmentShaderMetadata,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct TrianglePipeline {
    pub(crate) vs: BakedVertexShader,
    pub(crate) ps: BakedFragmentShader,
    pub(crate) vertex_stride_bytes: u32,
    pub(crate) vertex_count: u32,
    pub(crate) rt_binding_table_index: u8,
}

pub(crate) fn triangle_pipeline() -> Option<&'static TrianglePipeline> {
    generated::triangle_pipeline()
}

pub(crate) fn triangle_pipeline_note() -> &'static str {
    generated::TRIANGLE_PIPELINE_NOTE
}
