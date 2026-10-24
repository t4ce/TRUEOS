// Exact Voxygen headless WGSL, offline Mesa/Naga bake.
// Device bakes for 8086:9A49 and 8086:4680 produce identical code and pipeline
// state, admitted only on physical rev01 and rev0C respectively. See the
// Blueprint's shaders/tgl/metadata.json compile proof.
use super::*;
pub(crate) const PHYSICAL_TARGETS: [(u16, u16, u8); 2] = [
    (0x8086, 0x9A49, 0x01),
    (0x8086, 0x4680, 0x0C),
];
const VS_CODE: &[u32] = &[];
const PS_CODE: &[u32] = &[];
pub(crate) const SOURCE_FNV1A64: u64 = 0x2EAA72CFCA1B1C77;
const TEMPLATE: TrianglePipeline = TrianglePipeline {
    vs: TriangleVertexShader { code: &VS_CODE, meta: TriangleVertexShaderMetadata {
        kernel: ShaderKernelMetadata { code_offset_bytes: 0, code_size_bytes: 528,
            code_alignment_bytes: 64, ksp_offset_bytes: 0, dispatch_mode: DispatchMode::Simd8,
            grf_start_register: 2, grf_used: 128,
            push_constant_bytes: 96, binding_table_entry_count: 2, sampler_count: 0 },
        max_threads: 546, urb_entry_output_length: 1 } },
    ps: TrianglePixelShader { code: &PS_CODE, meta: TrianglePixelShaderMetadata {
        kernel: ShaderKernelMetadata { code_offset_bytes: 576, code_size_bytes: 176,
            code_alignment_bytes: 64, ksp_offset_bytes: 0, dispatch_mode: DispatchMode::Simd16,
            grf_start_register: 6, grf_used: 128, push_constant_bytes: 0,
            binding_table_entry_count: 1, sampler_count: 0 },
        num_varying_inputs: 1, uses_vmask: true, computed_stencil: false,
        persample_dispatch: false, computed_depth_mode: 0, flat_inputs: 0 } },
};

pub(crate) fn pipeline() -> &'static TrianglePipeline {
    static RESIDENT: spin::Once<TrianglePipeline> = spin::Once::new();
    let (Some(vs), Some(ps)) = (crate::gpu::app_shaders::words(1, 0), crate::gpu::app_shaders::words(1, 1)) else {
        return &TEMPLATE;
    };
    RESIDENT.call_once(|| {
        let mut pipeline = TEMPLATE;
        pipeline.vs.code = vs;
        pipeline.ps.code = ps;
        pipeline
    })
}
