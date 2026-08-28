use super::{
    AdjacencyGeometryShader, AdjacencyGeometryShaderMetadata, DispatchMode, ShaderKernelMetadata,
};

// @generated from pinned Mesa 6fb261147bbb4cc488ea9f16fb3b6fe02105332e on
// physical Intel RPL-S 8086:a780 revision 04 (gfx120). The line shader consumes
// adj0/real0/real1/adj1 and emits 1/2. The triangle shader consumes
// real0/adj0/real1/adj1/real2/adj2 and emits 0/2/4.
pub(crate) const ADJACENCY_GS_CAPTURE_NOTE: &str = "mesa-anv gfx120 rpls-a780-r04 line_sha256=eb17f372e6e07d316b978642441b494904d0b96c48f1796c8b238379fbc36517 triangle_sha256=8f32b6ef6b3132e490169bc18bd48bcc2f3210748af0862def90972984cc5175 verified=1";

static LINE_ADJACENCY_GS_CODE: [u32; 96] = [
    0x00030065, 0x7f058220, 0x02460105, 0x0000ffff, 0x00034031, 0x09240000, 0x6030030c, 0x02000000,
    0x00030061, 0x05054660, 0x00000000, 0x00000000, 0x00030061, 0x06054660, 0x00000000, 0x00000000,
    0x00030061, 0x07054660, 0x00000000, 0x00000000, 0x00030061, 0x08054660, 0x00000000, 0x00000000,
    0x80002001, 0x00000000, 0x00000000, 0x00000000, 0x00039131, 0x00000000, 0x604e7f0c, 0x02000544,
    0x80030061, 0x00054660, 0x00000000, 0x00000002, 0x80003101, 0x00000000, 0x00000000, 0x00000000,
    0x00034231, 0x0a240000, 0x6030040c, 0x02000000, 0x00030061, 0x06054660, 0x00000000, 0x00000000,
    0x00030061, 0x07054660, 0x00000000, 0x00000000, 0x00030061, 0x08054660, 0x00000000, 0x00000000,
    0x00030061, 0x09054660, 0x00000000, 0x00000000, 0x80002201, 0x00000000, 0x00000000, 0x00000000,
    0x00039331, 0x00000000, 0x608e7f0c, 0x02000644, 0x80000501, 0x00000000, 0x00000000, 0x00000000,
    0x00030040, 0x01058220, 0x02000004, 0xffffffff, 0x80003361, 0x07054660, 0x00000000, 0x00000001,
    0x80000101, 0x00000000, 0x00000000, 0x00000000, 0x21020069, 0x01010703, 0x00030161, 0x7e050220,
    0x00460205, 0x00000000, 0x00030131, 0x00000004, 0x600e7f0c, 0x02007e0c, 0x20000060, 0x00000000,
];

static TRIANGLE_ADJACENCY_GS_CODE: [u32; 128] = [
    0x00030065, 0x7f058220, 0x02460105, 0x0000ffff, 0x00034031, 0x0b240000, 0x6030020c, 0x02000000,
    0x00030061, 0x07054660, 0x00000000, 0x00000000, 0x00030061, 0x08054660, 0x00000000, 0x00000000,
    0x00030061, 0x09054660, 0x00000000, 0x00000000, 0x00030061, 0x0a054660, 0x00000000, 0x00000000,
    0x80002001, 0x00000000, 0x00000000, 0x00000000, 0x00039131, 0x00000000, 0x604e7f0c, 0x02000744,
    0x80003101, 0x00000000, 0x00000000, 0x00000000, 0x00034231, 0x0c240000, 0x6030040c, 0x02000000,
    0x00030061, 0x08054660, 0x00000000, 0x00000000, 0x00030061, 0x09054660, 0x00000000, 0x00000000,
    0x00030061, 0x0a054660, 0x00000000, 0x00000000, 0x00030061, 0x0b054660, 0x00000000, 0x00000000,
    0x80002201, 0x00000000, 0x00000000, 0x00000000, 0x00039331, 0x00000000, 0x608e7f0c, 0x02000844,
    0x80003301, 0x00000000, 0x00000000, 0x00000000, 0x00034431, 0x0d240000, 0x6030060c, 0x02000000,
    0x00030061, 0x09054660, 0x00000000, 0x00000000, 0x00030061, 0x0a054660, 0x00000000, 0x00000000,
    0x00030061, 0x0b054660, 0x00000000, 0x00000000, 0x00030061, 0x0c054660, 0x00000000, 0x00000000,
    0x80002401, 0x00000000, 0x00000000, 0x00000000, 0x00039531, 0x00000000, 0x60ce7f0c, 0x02000944,
    0x80030061, 0x00054660, 0x00000000, 0x00000003, 0x80003561, 0x0a054660, 0x00000000, 0x00000001,
    0x80000201, 0x00000000, 0x00000000, 0x00000000, 0x00030040, 0x01058220, 0x02000004, 0xffffffff,
    0x80000101, 0x00000000, 0x00000000, 0x00000000, 0x21020069, 0x01010a03, 0x00030161, 0x7e050220,
    0x00460205, 0x00000000, 0x00030131, 0x00000004, 0x600e7f0c, 0x02007e0c, 0x20000060, 0x00000000,
];

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

static LINE_ADJACENCY_GS: AdjacencyGeometryShader = AdjacencyGeometryShader {
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

static TRIANGLE_ADJACENCY_GS: AdjacencyGeometryShader = AdjacencyGeometryShader {
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
    &LINE_ADJACENCY_GS
}

pub(crate) fn triangle_adjacency_geometry_shader() -> &'static AdjacencyGeometryShader {
    &TRIANGLE_ADJACENCY_GS
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
