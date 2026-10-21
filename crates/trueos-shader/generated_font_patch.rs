// Source-only placeholder for tools/font-tessellation/bake_font_patch.py.
// A matched native capture replaces this file; unavailable artifacts must fail closed.
use super::*;

pub(crate) const CONTRACT_VERSION: u32 = 1;
pub(crate) const AVAILABLE: bool = false;
pub(crate) const COMPILED_DEVICE_ID: u16 = 0;
pub(crate) const SOURCE_SHA256: &str =
    "0000000000000000000000000000000000000000000000000000000000000000";
pub(crate) static VERTEX: [u32; 0] = [];
pub(crate) static TESS_CONTROL: [u32; 0] = [];
pub(crate) static TESS_EVAL: [u32; 0] = [];
pub(crate) static FRAGMENT: [u32; 0] = [];
pub(crate) const HS_PACKET: [u32; 9] = [0x781b0007, 0, 0, 0, 0, 0, 0, 0, 0];
pub(crate) const TE_PACKET: [u32; 5] = [0x781c0003, 0, 0, 0, 0];
pub(crate) const DS_PACKET: [u32; 11] = [0x781d0009, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
pub(crate) const URB: [(u32, u32, u32); 4] = [(0, 0, 0); 4];

const fn kernel() -> ShaderKernelMetadata {
    ShaderKernelMetadata {
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
    }
}

pub(crate) static PIPELINE: TrianglePipeline = TrianglePipeline {
    vs: TriangleVertexShader {
        code: &VERTEX,
        meta: TriangleVertexShaderMetadata {
            kernel: kernel(),
            max_threads: 0,
            urb_entry_output_length: 0,
        },
    },
    ps: TrianglePixelShader {
        code: &FRAGMENT,
        meta: TrianglePixelShaderMetadata {
            kernel: kernel(),
            num_varying_inputs: 0,
            uses_vmask: false,
            computed_stencil: false,
            persample_dispatch: false,
            computed_depth_mode: 0,
            flat_inputs: 0,
        },
    },
};

pub(crate) fn matches(_pipeline: &TrianglePipeline) -> bool {
    false
}
