//! Native Gen12 state for the admitted bfly isoline patch stages.

use super::{
    DispatchMode, ShaderKernelMetadata, TrianglePipeline, TrianglePixelShader,
    TrianglePixelShaderMetadata, TriangleVertexShader, TriangleVertexShaderMetadata,
};

pub(crate) const PANEL_SIDE_INSTANCES: u32 = 16;
const VS_BYTES: usize = 168;
const PS_BYTES: usize = 48;

pub(crate) const SGVS: u32 = 0xe001_4001;
pub(crate) const SGVS_2: [u32; 2] = [0xb001_0001, 2];
pub(crate) const VF_COMPONENT_PACKING: u32 = 0xa7;
pub(crate) const VS_URB_OUTPUT_LENGTH: u8 = 1;
pub(crate) const VS_URB_READ_LENGTH: u8 = 1;

// Mesa ANV 26.0.8, ADL-S 4680 rev 0C, indexed PATCHLIST_4 with 16 instances.
// KSP slots are relocated relative to the renderer's instruction base.
const HS: [u32; 9] = [
    0x781b0007, 0x00080000, 0xa0014f03, 0, 0, 0, 0, 0x01340000, 0,
];
const TE: [u32; 5] = [0x781c0003, 0x00800121, 0x427c0000, 0x42800000, 0];
// Size in 64-byte units, start in 8 KiB units, entry count.
pub(crate) const URB: [(u32, u32, u32); 4] =
    [(1, 4, 2304), (3, 22, 1024), (1, 46, 2304), (1, 4, 0)];
pub(crate) const CLIP: [u32; 3] = [0x00040400, 0xc4000001, 0x0003ffe0];
pub(crate) const RASTER: u32 = 0x04210003;
pub(crate) const WM: u32 = 0x80000040;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Geometry {
    Closed,
    Open,
}

impl Geometry {
    const fn patch_tags(self) -> &'static [f32] {
        match self {
            Self::Closed => &[0.0, 0.0, 1.0, 0.0, 1.0],
            Self::Open => &[0.0, 0.0, 0.0, 1.0],
        }
    }

    pub(crate) const fn control_vertices(self) -> usize {
        self.patch_tags().len() * 4
    }

    pub(crate) fn valid_vertices(self, vertices: &[u8], indices: &[u32]) -> bool {
        let count = self.control_vertices();
        vertices.len() == count * 12
            && indices.len() == count
            && indices.iter().copied().enumerate().all(|(i, idx)| i == idx as usize)
            && vertices.chunks_exact(48).zip(self.patch_tags()).all(|(patch, &tag)| {
                patch.chunks_exact(12).all(|point| {
                    let x = f32::from_le_bytes(point[0..4].try_into().unwrap());
                    let y = f32::from_le_bytes(point[4..8].try_into().unwrap());
                    let edge = f32::from_le_bytes(point[8..12].try_into().unwrap());
                    x.is_finite() && y.is_finite() && x.abs() <= 2.0 && y.abs() <= 2.0 && edge == tag
                })
            })
    }
}

struct Capture {
    package_id: u32,
    geometry: Geometry,
    hs_bytes: usize,
    ds_bytes: usize,
    ds_urb_read: u32,
}

pub(crate) struct Program {
    pipeline: TrianglePipeline,
    pub(crate) tessellation_code: [&'static [u32]; 2],
    capture: &'static Capture,
}

impl Program {
    pub(crate) fn control_vertices(&self) -> usize {
        self.capture.geometry.control_vertices()
    }

    pub(crate) fn stage_packets(&self, ksp: [u32; 2]) -> Result<[u32; 25], &'static str> {
        // Disable the legacy panel planes. The viewport and scissor enforce
        // the UI4 window bounds.
        let ds = [
            0x781d0009,
            0,
            0,
            0,
            0,
            0,
            self.capture.ds_urb_read,
            0x44200609,
            0,
            0,
            0,
        ];
        super::relocate_tessellation_stage_packets(HS, TE, ds, ksp)
    }
}

pub(crate) fn supports_physical_device(vendor: u16, device: u16, revision: u8) -> bool {
    matches!(
        (vendor, device, revision),
        (0x8086, 0x4680, 0x0c) | (0x8086, 0xa780, 0x04)
    )
}

// New meshes prefer the latest package for their layout. Resident meshes retain their capture.
static PROGRAMS: [(Capture, spin::Once<Program>); 3] = [
    (
        Capture {
            package_id: 15,
            geometry: Geometry::Closed,
            hs_bytes: 6224,
            ds_bytes: 544,
            ds_urb_read: 0x0050a800,
        },
        spin::Once::new(),
    ),
    (
        Capture {
            package_id: 14,
            geometry: Geometry::Closed,
            hs_bytes: 4064,
            ds_bytes: 544,
            ds_urb_read: 0x0050a800,
        },
        spin::Once::new(),
    ),
    (
        Capture {
            package_id: 13,
            geometry: Geometry::Closed,
            hs_bytes: 4280,
            ds_bytes: 592,
            ds_urb_read: 0x0050b800,
        },
        spin::Once::new(),
    ),
];

fn load(capture: &'static Capture, slot: &'static spin::Once<Program>) -> Option<&'static Program> {
    if let Some(program) = slot.get() {
        return Some(program);
    }
    let code = |stage| crate::gpu::app_shaders::words(capture.package_id, stage);
    let [vs, hs, ds, ps] = [code(0)?, code(1)?, code(2)?, code(3)?];
    if [vs.len(), hs.len(), ds.len(), ps.len()].map(|words| words * 4)
        != [VS_BYTES, capture.hs_bytes, capture.ds_bytes, PS_BYTES]
    {
        return None;
    }
    Some(slot.call_once(|| Program {
        capture,
        tessellation_code: [hs, ds],
        pipeline: TrianglePipeline {
            vs: TriangleVertexShader {
                code: vs,
                meta: TriangleVertexShaderMetadata {
                    kernel: ShaderKernelMetadata {
                        code_offset_bytes: 0,
                        code_size_bytes: VS_BYTES as u32,
                        code_alignment_bytes: 64,
                        ksp_offset_bytes: 0,
                        dispatch_mode: DispatchMode::Simd8,
                        grf_start_register: 2,
                        grf_used: 128,
                        push_constant_bytes: 0,
                        binding_table_entry_count: 0,
                        sampler_count: 0,
                    },
                    max_threads: 546,
                    urb_entry_output_length: VS_URB_OUTPUT_LENGTH,
                },
            },
            ps: TrianglePixelShader {
                code: ps,
                meta: TrianglePixelShaderMetadata {
                    kernel: ShaderKernelMetadata {
                        code_offset_bytes: VS_BYTES.next_multiple_of(64) as u32,
                        code_size_bytes: PS_BYTES as u32,
                        code_alignment_bytes: 64,
                        ksp_offset_bytes: 0,
                        dispatch_mode: DispatchMode::Simd16,
                        grf_start_register: 2,
                        grf_used: 128,
                        push_constant_bytes: 0,
                        binding_table_entry_count: 1,
                        sampler_count: 0,
                    },
                    num_varying_inputs: 0,
                    uses_vmask: true,
                    computed_stencil: false,
                    persample_dispatch: false,
                    computed_depth_mode: 0,
                    flat_inputs: 0,
                },
            },
        },
    }))
}

pub(crate) fn for_pipeline(pipeline: &TrianglePipeline) -> Option<&'static Program> {
    PROGRAMS
        .iter()
        .filter_map(|(capture, slot)| load(capture, slot))
        .find(|program| core::ptr::eq(pipeline.vs.code, program.pipeline.vs.code))
}

pub(crate) fn pipeline(geometry: Geometry) -> Option<&'static TrianglePipeline> {
    PROGRAMS
        .iter()
        .filter(|(capture, _)| capture.geometry == geometry)
        .find_map(|(capture, slot)| load(capture, slot))
        .map(|program| &program.pipeline)
}
