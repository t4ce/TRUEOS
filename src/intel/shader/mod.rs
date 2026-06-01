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
    /// Offset used for the stage's kernel start pointer relative to the base
    /// GPU address where this shader blob is uploaded.
    pub(crate) ksp_offset_bytes: u32,

    /// Offset of the full stage code blob relative to the uploaded shader BO
    /// base. This is the start of the bytes copied from `code`.
    pub(crate) code_offset_bytes: u32,

    /// Total uploaded code size in bytes for this stage blob.
    pub(crate) code_size_bytes: u32,

    /// Required alignment in bytes for the uploaded code blob base.
    pub(crate) code_alignment_bytes: u32,

    /// Compiler-selected dispatch payload start register encoded as a GRF
    /// index, matching the field programmed into stage state packets.
    pub(crate) grf_start_register: u8,

    /// Chosen SIMD dispatch width for this kernel.
    pub(crate) dispatch_mode: DispatchMode,

    /// Number of sampler table entries referenced by this stage.
    pub(crate) sampler_count: u8,

    /// Number of binding table entries the stage expects to be valid.
    pub(crate) binding_table_entry_count: u8,

    /// Total push constant payload consumed by this stage, in bytes.
    pub(crate) push_constant_bytes: u16,

    /// Total GRFs allocated for the compiled program, in hardware GRF units.
    pub(crate) grf_used: u16,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct VertexShaderMetadata {
    pub(crate) kernel: ShaderKernelMetadata,

    /// URB output length programmed for the VS, in 64-byte units.
    pub(crate) urb_entry_output_length: u8,

    /// Maximum thread count field for the uploaded VS, in hardware thread
    /// units as expected by the 3DSTATE_VS packet.
    pub(crate) max_threads: u16,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct FragmentShaderMetadata {
    pub(crate) kernel: ShaderKernelMetadata,

    /// Number of fragment shader varyings consumed from the PS input payload,
    /// in 16-byte attribute slots.
    pub(crate) num_varying_inputs: u8,

    /// Bitmask of varyings that require flat interpolation, indexed by payload
    /// attribute slot.
    pub(crate) flat_inputs: u32,

    /// Whether the compiled shader expects a live vmask input.
    pub(crate) uses_vmask: bool,

    /// Hardware depth-computation mode value programmed into PS extra state.
    pub(crate) computed_depth_mode: u8,

    /// Whether the PS writes stencil.
    pub(crate) computed_stencil: bool,

    /// Whether the PS was compiled for per-sample dispatch.
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
    pub(crate) vs: &'static BakedVertexShader,
    pub(crate) ps: &'static BakedFragmentShader,
    pub(crate) vertex_stride_bytes: u32,
    pub(crate) vertex_count: u32,
    pub(crate) rt_binding_table_index: u8,
}

pub(crate) fn triangle_pipeline() -> &'static TrianglePipeline {
    generated::triangle_pipeline()
}

pub(crate) fn triangle_pipeline_is_placeholder() -> bool {
    let pipeline = triangle_pipeline();
    pipeline.vs.code.is_empty() || pipeline.ps.code.is_empty()
}

pub(crate) fn triangle_pipeline_note() -> &'static str {
    generated::TRIANGLE_PIPELINE_NOTE
}
