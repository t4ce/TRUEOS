use super::{
    AdjacencyGeometryShader, AdjacencyGeometryShaderMetadata, DispatchMode, ShaderKernelMetadata,
};

// @generated from pinned Mesa 6fb261147bbb4cc488ea9f16fb3b6fe02105332e on
// physical Intel RPL-S 8086:a780 revision 04 (gfx120). The line shader consumes
// adj0/real0/real1/adj1 and emits 1/2. The triangle shader consumes
// real0/adj0/real1/adj1/real2/adj2 and emits 0/2/4.
pub(crate) const ADJACENCY_GS_CAPTURE_NOTE: &str = "mesa-anv gfx120 rpls-a780-r04 line_sha256=eb17f372e6e07d316b978642441b494904d0b96c48f1796c8b238379fbc36517 triangle_sha256=8f32b6ef6b3132e490169bc18bd48bcc2f3210748af0862def90972984cc5175 verified=1";

const LINE_ADJACENCY_GS_CODE: &[u32] = &[];

const TRIANGLE_ADJACENCY_GS_CODE: &[u32] = &[];

const fn kernel(
    code_offset_bytes: u32,
    code_size_bytes: u32,
    grf_start_register: u8,
) -> ShaderKernelMetadata {
    ShaderKernelMetadata {
        code_offset_bytes,
        code_size_bytes,
        code_alignment_bytes: 64,
        ksp_offset_bytes: 0,
        dispatch_mode: DispatchMode::Simd8,
        grf_start_register,
        grf_used: 128,
        push_constant_bytes: 0,
        binding_table_entry_count: 0,
        sampler_count: 0,
    }
}

const LINE_ADJACENCY_GS: AdjacencyGeometryShader = AdjacencyGeometryShader {
    code: &LINE_ADJACENCY_GS_CODE,
    meta: AdjacencyGeometryShaderMetadata {
        kernel: kernel(2048, 384, 6),
        max_threads: 336,
        expected_vertex_count: 4,
        output_topology: 3,
        output_vertex_size: 1,
        control_data_header_size: 1,
        static_output_vertex_count: 2,
        urb_entry_size: 2,
        urb_start: 32,
        urb_entries: 1024,
        sf_deref_block_size: 1,
    },
};

const TRIANGLE_ADJACENCY_GS: AdjacencyGeometryShader = AdjacencyGeometryShader {
    code: &TRIANGLE_ADJACENCY_GS_CODE,
    meta: AdjacencyGeometryShaderMetadata {
        kernel: kernel(2432, 512, 8),
        max_threads: 336,
        expected_vertex_count: 6,
        output_topology: 5,
        output_vertex_size: 1,
        control_data_header_size: 1,
        static_output_vertex_count: 3,
        urb_entry_size: 3,
        urb_start: 32,
        urb_entries: 1024,
        sf_deref_block_size: 1,
    },
};

pub(crate) fn line_adjacency_geometry_shader() -> &'static AdjacencyGeometryShader {
    static RESIDENT: spin::Once<AdjacencyGeometryShader> = spin::Once::new();
    let Some(code) = crate::gpu::app_shaders::words(7, 0) else { return &LINE_ADJACENCY_GS; };
    RESIDENT.call_once(|| { let mut shader = LINE_ADJACENCY_GS; shader.code = code; shader })
}

pub(crate) fn triangle_adjacency_geometry_shader() -> &'static AdjacencyGeometryShader {
    static RESIDENT: spin::Once<AdjacencyGeometryShader> = spin::Once::new();
    let Some(code) = crate::gpu::app_shaders::words(7, 1) else { return &TRIANGLE_ADJACENCY_GS; };
    RESIDENT.call_once(|| { let mut shader = TRIANGLE_ADJACENCY_GS; shader.code = code; shader })
}

#[cfg(test)]
mod tests {
    use super::{LINE_ADJACENCY_GS, TRIANGLE_ADJACENCY_GS};

    #[test]
    fn captured_geometry_kernels_and_urb_contracts_are_self_consistent() {
        assert_eq!(LINE_ADJACENCY_GS.code.len() * 4, 384);
        assert_eq!(TRIANGLE_ADJACENCY_GS.code.len() * 4, 512);
        assert_eq!(LINE_ADJACENCY_GS.meta.expected_vertex_count, 4);
        assert_eq!(LINE_ADJACENCY_GS.meta.static_output_vertex_count, 2);
        assert_eq!(LINE_ADJACENCY_GS.meta.urb_entry_size, 2);
        assert_eq!(TRIANGLE_ADJACENCY_GS.meta.expected_vertex_count, 6);
        assert_eq!(TRIANGLE_ADJACENCY_GS.meta.static_output_vertex_count, 3);
        assert_eq!(TRIANGLE_ADJACENCY_GS.meta.urb_entry_size, 3);
        assert_eq!(LINE_ADJACENCY_GS.meta.urb_start, 32);
        assert_eq!(TRIANGLE_ADJACENCY_GS.meta.urb_entries, 1024);
        assert_eq!(LINE_ADJACENCY_GS.meta.kernel.code_offset_bytes % 64, 0);
        assert_eq!(TRIANGLE_ADJACENCY_GS.meta.kernel.code_offset_bytes % 64, 0);
        assert!(
            LINE_ADJACENCY_GS.meta.kernel.code_offset_bytes
                + LINE_ADJACENCY_GS.meta.kernel.code_size_bytes
                <= TRIANGLE_ADJACENCY_GS.meta.kernel.code_offset_bytes
        );
    }
}
