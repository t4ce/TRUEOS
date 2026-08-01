//! Strict build-time contract for Helio's GPU-driven Churn forward pass.
//!
//! The descriptor deliberately contains no captured wgpu object IDs or host
//! pointers. Native code is named by HELIOA section and authenticated by SHA-256;
//! all offsets below are relative to their named section or GPU record.

use core::str;

pub const SECTION_NAME: &str = "render/churn-forward-v1.bin";
pub const SHADER_SOURCE_SECTION: &str = "render/churn-forward.wgsl";
pub const VERTEX_ISA_SECTION: &str = "intel-xe-lp/churn-forward.vs.simd8.bin";
pub const FRAGMENT_ISA_SECTION: &str = "intel-xe-lp/churn-forward.ps.simd8.bin";

pub const MAGIC: [u8; 8] = *b"HCFWD\0\0\0";
pub const FORMAT_VERSION: u16 = 1;
pub const BYTE_LEN: usize = 768;

pub const CAMERA_STRIDE: u32 = 368;
pub const INSTANCE_STRIDE: u32 = 208;
pub const COMPACTED_INDEX_STRIDE: u32 = 4;
pub const INDIRECT_STRIDE: u32 = 20;
pub const VERTEX_STRIDE: u32 = 24;

const FLAGS: u32 = 0x3f;
const STAGE_RECORD_LEN: usize = 160;
const VS_OFFSET: usize = 288;
const PS_OFFSET: usize = VS_OFFSET + STAGE_RECORD_LEN;
const SOURCE_OFFSET: usize = PS_OFFSET + STAGE_RECORD_LEN;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    BadMagic,
    UnsupportedVersion(u16),
    WrongLength,
    InvalidFlags,
    InvalidLayout,
    InvalidVertexFetch,
    InvalidBindings,
    InvalidFixedFunction,
    InvalidStage,
    InvalidSource,
    InvalidUtf8,
    NonzeroReserved,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CameraLayout {
    pub stride: u32,
    pub view: u32,
    pub proj: u32,
    pub view_proj: u32,
    pub inv_view_proj: u32,
    pub position_near: u32,
    pub forward_far: u32,
    pub jitter_frame: u32,
    pub prev_view_proj: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InstanceLayout {
    pub stride: u32,
    pub model: u32,
    pub normal_matrix: u32,
    pub bounds: u32,
    pub prev_model: u32,
    pub mesh_id: u32,
    pub material_id: u32,
    pub flags: u32,
    pub lightmap_index: u32,
    pub model_size: u32,
    pub normal_matrix_size: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IndirectLayout {
    pub compacted_index_stride: u32,
    pub stride: u32,
    pub index_count: u32,
    pub instance_count: u32,
    pub first_index: u32,
    pub base_vertex: u32,
    pub first_instance: u32,
    pub canonical_index_count: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VertexAttribute {
    pub location: u16,
    /// v1 value 1 is Float32x3.
    pub format: u16,
    pub offset: u32,
    /// Bit 0..3 enable X/Y/Z/W in the Intel vertex-fetch element.
    pub vf_component_mask: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VertexFetch {
    pub stride: u32,
    /// v1 value 1 is Uint32.
    pub index_format: u16,
    pub attributes: [VertexAttribute; 2],
    pub vertex_buffer_index: u16,
    /// v1 value 0 is per-vertex stepping.
    pub step_mode: u16,
    /// gfx125 3DSTATE_VF_COMPONENT_PACKING DWORD0.
    pub vf_component_packing_dw0: u32,
    pub packed_vs_input_count: u16,
    pub urb_input_read_length: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResourceBinding {
    pub group: u8,
    pub binding: u8,
    pub intel_bti: u8,
    /// v1 value 1 is a read-only storage buffer.
    pub kind: u8,
    /// Bit 0 is VS and bit 1 is PS.
    pub visibility: u8,
    /// v1 value 1 is read-only.
    pub access: u8,
    pub min_binding_size: u32,
    pub element_stride: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FixedFunctionState {
    pub topology: u16,
    pub front_face: u16,
    pub cull_mode: u16,
    pub color_format: u16,
    pub depth_format: u16,
    pub depth_compare: u16,
    pub index_format: u16,
    pub sample_count: u16,
    pub depth_write: bool,
    pub blend: bool,
    pub color_write_mask: u32,
    pub render_target_bti: u16,
    pub sbe_read_offset: u8,
    pub sbe_read_length: u8,
    pub num_sf_attributes: u16,
}

/// Raw gfx125 SGVS payload derived from pinned Mesa's `genX_shader.c` packing.
/// Churn consumes `@builtin(instance_index)` to address compacted instances.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SgvsState {
    pub vf_sgvs_dw1: u32,
    pub vf_sgvs_2_dw1: u32,
    pub vf_sgvs_2_dw2: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VfInstancing {
    pub element_index: u16,
    pub enabled: bool,
    pub step_rate: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyntheticVertexElement {
    pub element_index: u16,
    pub vertex_buffer_index: u8,
    /// Intel surface format 135 is R32G32_UINT.
    pub surface_format: u16,
    /// Intel VFCOMP_STORE_0 is value 2 for every component.
    pub component_controls: [u8; 4],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShaderStage {
    Vertex,
    Fragment,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StageRef<'a> {
    pub stage: ShaderStage,
    pub simd_width: u16,
    pub code_size_bytes: u32,
    /// Offset of executable bytes inside the referenced HELIOA section.
    pub code_offset_bytes: u32,
    /// Required GPU upload alignment, not an assertion about the file offset.
    pub code_alignment_bytes: u32,
    pub ksp_offset_bytes: u32,
    pub grf_start_register: u16,
    pub grf_used: u16,
    pub max_threads: u16,
    pub binding_table_entry_count: u16,
    pub sampler_count: u16,
    pub push_constant_bytes: u16,
    pub urb_entry_output_length: u16,
    pub num_varying_inputs: u16,
    pub uses_vmask: bool,
    pub computed_stencil: bool,
    pub persample_dispatch: bool,
    pub computed_depth_mode: u8,
    pub flat_inputs: u32,
    pub sha256: [u8; 32],
    pub entry_point: &'a str,
    pub section_name: &'a str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceRef<'a> {
    pub byte_len: u32,
    pub sha256: [u8; 32],
    pub section_name: &'a str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Program<'a> {
    bytes: &'a [u8],
    vertex: StageRef<'a>,
    fragment: StageRef<'a>,
    source: SourceRef<'a>,
}

impl<'a> Program<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, Error> {
        if bytes.get(..8) != Some(MAGIC.as_slice()) {
            return Err(Error::BadMagic);
        }
        let version = u16_at(bytes, 8)?;
        if version != FORMAT_VERSION {
            return Err(Error::UnsupportedVersion(version));
        }
        if bytes.len() != BYTE_LEN
            || usize::from(u16_at(bytes, 10)?) != BYTE_LEN
            || usize::try_from(u32_at(bytes, 12)?).ok() != Some(BYTE_LEN)
        {
            return Err(Error::WrongLength);
        }
        if u32_at(bytes, 16)? != FLAGS
            || u16_at(bytes, 20)? != 2
            || u16_at(bytes, 22)? != 3
            || u16_at(bytes, 24)? != 2
            || bytes[26..32].iter().any(|&byte| byte != 0)
        {
            return Err(Error::InvalidFlags);
        }

        validate_layout(bytes)?;
        validate_vertex_fetch(bytes)?;
        validate_bindings(bytes)?;
        validate_fixed_function(bytes)?;

        let vertex = parse_stage(bytes, VS_OFFSET, ShaderStage::Vertex)?;
        let fragment = parse_stage(bytes, PS_OFFSET, ShaderStage::Fragment)?;
        let source = parse_source(bytes)?;
        if (u32_at(bytes, 704)?, u32_at(bytes, 708)?, u32_at(bytes, 712)?)
            != (0xe002_4002, 0xb002_0002, 3)
        {
            return Err(Error::InvalidVertexFetch);
        }
        if u16_at(bytes, 716)? != 3
            || u16_at(bytes, 718)? != 0
            || parse_instancing(bytes, 720)?
                != (VfInstancing {
                    element_index: 0,
                    enabled: false,
                    step_rate: 0,
                })
            || parse_instancing(bytes, 728)?
                != (VfInstancing {
                    element_index: 1,
                    enabled: false,
                    step_rate: 0,
                })
            || parse_instancing(bytes, 736)?
                != (VfInstancing {
                    element_index: 2,
                    enabled: false,
                    step_rate: 0,
                })
            || parse_synthetic_element(bytes, 744)?
                != (SyntheticVertexElement {
                    element_index: 2,
                    vertex_buffer_index: 31,
                    surface_format: 135,
                    component_controls: [2; 4],
                })
        {
            return Err(Error::InvalidVertexFetch);
        }
        if u32_at(bytes, 756)? != 0x0000_0a77
            || u16_at(bytes, 760)? != 8
            || u16_at(bytes, 762)? != 1
        {
            return Err(Error::InvalidVertexFetch);
        }
        if bytes[764..].iter().any(|&byte| byte != 0) {
            return Err(Error::NonzeroReserved);
        }
        Ok(Self {
            bytes,
            vertex,
            fragment,
            source,
        })
    }

    pub const fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    pub fn camera_layout(&self) -> CameraLayout {
        CameraLayout {
            stride: value(self.bytes, 32),
            view: value(self.bytes, 36),
            proj: value(self.bytes, 40),
            view_proj: value(self.bytes, 44),
            inv_view_proj: value(self.bytes, 48),
            position_near: value(self.bytes, 52),
            forward_far: value(self.bytes, 56),
            jitter_frame: value(self.bytes, 60),
            prev_view_proj: value(self.bytes, 64),
        }
    }

    pub fn instance_layout(&self) -> InstanceLayout {
        InstanceLayout {
            stride: value(self.bytes, 72),
            model: value(self.bytes, 76),
            normal_matrix: value(self.bytes, 80),
            bounds: value(self.bytes, 84),
            prev_model: value(self.bytes, 88),
            mesh_id: value(self.bytes, 92),
            material_id: value(self.bytes, 96),
            flags: value(self.bytes, 100),
            lightmap_index: value(self.bytes, 104),
            model_size: value(self.bytes, 108),
            normal_matrix_size: value(self.bytes, 112),
        }
    }

    pub fn indirect_layout(&self) -> IndirectLayout {
        IndirectLayout {
            compacted_index_stride: value(self.bytes, 120),
            stride: value(self.bytes, 124),
            index_count: value(self.bytes, 128),
            instance_count: value(self.bytes, 132),
            first_index: value(self.bytes, 136),
            base_vertex: value(self.bytes, 140),
            first_instance: value(self.bytes, 144),
            canonical_index_count: value(self.bytes, 148),
        }
    }

    pub fn vertex_fetch(&self) -> VertexFetch {
        VertexFetch {
            stride: value(self.bytes, 160),
            index_format: u16_value(self.bytes, 164),
            attributes: [attribute(self.bytes, 168), attribute(self.bytes, 180)],
            vertex_buffer_index: u16_value(self.bytes, 192),
            step_mode: u16_value(self.bytes, 194),
            vf_component_packing_dw0: value(self.bytes, 756),
            packed_vs_input_count: u16_value(self.bytes, 760),
            urb_input_read_length: u16_value(self.bytes, 762),
        }
    }

    pub fn bindings(&self) -> [ResourceBinding; 3] {
        [
            binding(self.bytes, 208),
            binding(self.bytes, 224),
            binding(self.bytes, 240),
        ]
    }

    pub fn fixed_function(&self) -> FixedFunctionState {
        let flags = value(self.bytes, 272);
        FixedFunctionState {
            topology: u16_value(self.bytes, 256),
            front_face: u16_value(self.bytes, 258),
            cull_mode: u16_value(self.bytes, 260),
            color_format: u16_value(self.bytes, 262),
            depth_format: u16_value(self.bytes, 264),
            depth_compare: u16_value(self.bytes, 266),
            index_format: u16_value(self.bytes, 268),
            sample_count: u16_value(self.bytes, 270),
            depth_write: flags & 1 != 0,
            blend: flags & 2 != 0,
            color_write_mask: value(self.bytes, 276),
            render_target_bti: u16_value(self.bytes, 280),
            sbe_read_offset: self.bytes[282],
            sbe_read_length: self.bytes[283],
            num_sf_attributes: u16_value(self.bytes, 284),
        }
    }

    pub fn sgvs(&self) -> SgvsState {
        SgvsState {
            vf_sgvs_dw1: value(self.bytes, 704),
            vf_sgvs_2_dw1: value(self.bytes, 708),
            vf_sgvs_2_dw2: value(self.bytes, 712),
        }
    }

    pub fn vf_instancing(&self) -> [VfInstancing; 3] {
        [
            parse_instancing(self.bytes, 720).unwrap_or(VfInstancing {
                element_index: 0,
                enabled: false,
                step_rate: 0,
            }),
            parse_instancing(self.bytes, 728).unwrap_or(VfInstancing {
                element_index: 1,
                enabled: false,
                step_rate: 0,
            }),
            parse_instancing(self.bytes, 736).unwrap_or(VfInstancing {
                element_index: 2,
                enabled: false,
                step_rate: 0,
            }),
        ]
    }

    pub fn synthetic_instance_id_element(&self) -> SyntheticVertexElement {
        parse_synthetic_element(self.bytes, 744).unwrap_or(SyntheticVertexElement {
            element_index: 2,
            vertex_buffer_index: 31,
            surface_format: 135,
            component_controls: [2; 4],
        })
    }

    pub const fn vertex_stage(&self) -> StageRef<'a> {
        self.vertex
    }

    pub const fn fragment_stage(&self) -> StageRef<'a> {
        self.fragment
    }

    pub const fn shader_source(&self) -> SourceRef<'a> {
        self.source
    }
}

fn validate_layout(bytes: &[u8]) -> Result<(), Error> {
    let camera = (32..=68).step_by(4).map(|offset| u32_at(bytes, offset));
    let expected_camera = [368, 0, 64, 128, 192, 256, 272, 288, 304, 0];
    for (actual, expected) in camera.zip(expected_camera) {
        if actual? != expected {
            return Err(Error::InvalidLayout);
        }
    }
    let instance = (72..=116).step_by(4).map(|offset| u32_at(bytes, offset));
    let expected_instance = [208, 0, 64, 112, 128, 192, 196, 200, 204, 64, 48, 0];
    for (actual, expected) in instance.zip(expected_instance) {
        if actual? != expected {
            return Err(Error::InvalidLayout);
        }
    }
    let draw = (120..=156).step_by(4).map(|offset| u32_at(bytes, offset));
    let expected_draw = [4, 20, 0, 4, 8, 12, 16, 36, 0, 0];
    for (actual, expected) in draw.zip(expected_draw) {
        if actual? != expected {
            return Err(Error::InvalidLayout);
        }
    }
    Ok(())
}

fn validate_vertex_fetch(bytes: &[u8]) -> Result<(), Error> {
    if u32_at(bytes, 160)? != VERTEX_STRIDE
        || u16_at(bytes, 164)? != 1
        || u16_at(bytes, 166)? != 2
        || attribute(bytes, 168)
            != (VertexAttribute {
                location: 0,
                format: 1,
                offset: 0,
                vf_component_mask: 0x7,
            })
        || attribute(bytes, 180)
            != (VertexAttribute {
                location: 1,
                format: 1,
                offset: 12,
                vf_component_mask: 0x7,
            })
        || u16_at(bytes, 192)? != 0
        || u16_at(bytes, 194)? != 0
        || u32_at(bytes, 196)? != VERTEX_STRIDE
        || bytes[200..208].iter().any(|&byte| byte != 0)
    {
        return Err(Error::InvalidVertexFetch);
    }
    Ok(())
}

fn validate_bindings(bytes: &[u8]) -> Result<(), Error> {
    let expected = [
        ResourceBinding {
            group: 0,
            binding: 0,
            intel_bti: 1,
            kind: 1,
            visibility: 1,
            access: 1,
            min_binding_size: CAMERA_STRIDE,
            element_stride: CAMERA_STRIDE,
        },
        ResourceBinding {
            group: 0,
            binding: 1,
            intel_bti: 2,
            kind: 1,
            visibility: 1,
            access: 1,
            min_binding_size: INSTANCE_STRIDE,
            element_stride: INSTANCE_STRIDE,
        },
        ResourceBinding {
            group: 0,
            binding: 2,
            intel_bti: 3,
            kind: 1,
            visibility: 1,
            access: 1,
            min_binding_size: COMPACTED_INDEX_STRIDE,
            element_stride: COMPACTED_INDEX_STRIDE,
        },
    ];
    for (offset, expected) in [208, 224, 240].into_iter().zip(expected) {
        if binding(bytes, offset) != expected || bytes[offset + 6..offset + 8] != [0, 0] {
            return Err(Error::InvalidBindings);
        }
    }
    Ok(())
}

fn validate_fixed_function(bytes: &[u8]) -> Result<(), Error> {
    if (256..=270)
        .step_by(2)
        .map(|offset| u16_at(bytes, offset))
        .zip([1u16; 8])
        .any(|(actual, expected)| actual != Ok(expected))
        || u32_at(bytes, 272)? != 1
        || u32_at(bytes, 276)? != 0xf
        || u16_at(bytes, 280)? != 0
        || bytes[282] != 1
        || bytes[283] != 1
        || u16_at(bytes, 284)? != 2
        || u16_at(bytes, 286)? != 0
    {
        return Err(Error::InvalidFixedFunction);
    }
    Ok(())
}

fn parse_stage<'a>(
    bytes: &'a [u8],
    offset: usize,
    expected_stage: ShaderStage,
) -> Result<StageRef<'a>, Error> {
    let stage = match u16_at(bytes, offset)? {
        1 => ShaderStage::Vertex,
        2 => ShaderStage::Fragment,
        _ => return Err(Error::InvalidStage),
    };
    let entry_len = usize::from(u16_at(bytes, offset + 80)?);
    let name_len = usize::from(u16_at(bytes, offset + 82)?);
    if stage != expected_stage
        || u16_at(bytes, offset + 2)? != 8
        || u32_at(bytes, offset + 4)? == 0
        || u32_at(bytes, offset + 4)? % 4 != 0
        || u32_at(bytes, offset + 8)? != 0
        || u32_at(bytes, offset + 12)? != 64
        || u32_at(bytes, offset + 16)? != 0
        || u16_at(bytes, offset + 20)? != 2
        || u16_at(bytes, offset + 22)? != 128
        || u16_at(bytes, offset + 24)? != 64
        || u16_at(bytes, offset + 28)? != 0
        || u16_at(bytes, offset + 30)? != 0
        || u32_at(bytes, offset + 44)? != 0
        || u32_at(bytes, offset + 84)? != 0
        || entry_len == 0
        || entry_len > 16
        || name_len == 0
        || name_len > 56
        || bytes[offset + 48..offset + 80]
            .iter()
            .all(|&byte| byte == 0)
        || bytes[offset + 88 + entry_len..offset + 104]
            .iter()
            .any(|&byte| byte != 0)
        || bytes[offset + 104 + name_len..offset + 160]
            .iter()
            .any(|&byte| byte != 0)
    {
        return Err(Error::InvalidStage);
    }
    let entry_point = str::from_utf8(&bytes[offset + 88..offset + 88 + entry_len])
        .map_err(|_| Error::InvalidUtf8)?;
    let section_name = str::from_utf8(&bytes[offset + 104..offset + 104 + name_len])
        .map_err(|_| Error::InvalidUtf8)?;
    let (expected_entry, expected_section) = match stage {
        ShaderStage::Vertex => ("vs_main", VERTEX_ISA_SECTION),
        ShaderStage::Fragment => ("fs_main", FRAGMENT_ISA_SECTION),
    };
    if entry_point != expected_entry || section_name != expected_section {
        return Err(Error::InvalidStage);
    }
    if stage == ShaderStage::Vertex {
        if u16_at(bytes, offset + 26)? != 4
            || u16_at(bytes, offset + 32)? != 1
            || u16_at(bytes, offset + 34)? != 0
            || u32_at(bytes, offset + 36)? != 0
            || u32_at(bytes, offset + 40)? != 0
        {
            return Err(Error::InvalidStage);
        }
    } else if u16_at(bytes, offset + 26)? != 1
        || u16_at(bytes, offset + 32)? != 0
        || u16_at(bytes, offset + 34)? != 2
        || u32_at(bytes, offset + 36)? != 1
        || u32_at(bytes, offset + 40)? != 2
    {
        return Err(Error::InvalidStage);
    }
    let ps_flags = u32_at(bytes, offset + 36)?;
    Ok(StageRef {
        stage,
        simd_width: u16_at(bytes, offset + 2)?,
        code_size_bytes: u32_at(bytes, offset + 4)?,
        code_offset_bytes: u32_at(bytes, offset + 8)?,
        code_alignment_bytes: u32_at(bytes, offset + 12)?,
        ksp_offset_bytes: u32_at(bytes, offset + 16)?,
        grf_start_register: u16_at(bytes, offset + 20)?,
        grf_used: u16_at(bytes, offset + 22)?,
        max_threads: u16_at(bytes, offset + 24)?,
        binding_table_entry_count: u16_at(bytes, offset + 26)?,
        sampler_count: u16_at(bytes, offset + 28)?,
        push_constant_bytes: u16_at(bytes, offset + 30)?,
        urb_entry_output_length: u16_at(bytes, offset + 32)?,
        num_varying_inputs: u16_at(bytes, offset + 34)?,
        uses_vmask: ps_flags & 1 != 0,
        computed_stencil: ps_flags & 2 != 0,
        persample_dispatch: ps_flags & 4 != 0,
        computed_depth_mode: ((ps_flags >> 8) & 3) as u8,
        flat_inputs: u32_at(bytes, offset + 40)?,
        sha256: hash_at(bytes, offset + 48),
        entry_point,
        section_name,
    })
}

fn parse_source(bytes: &[u8]) -> Result<SourceRef<'_>, Error> {
    let byte_len = u32_at(bytes, SOURCE_OFFSET)?;
    let name_len = usize::from(u16_at(bytes, SOURCE_OFFSET + 4)?);
    if byte_len == 0
        || u16_at(bytes, SOURCE_OFFSET + 6)? != 0
        || bytes[SOURCE_OFFSET + 8..SOURCE_OFFSET + 40]
            .iter()
            .all(|&byte| byte == 0)
        || name_len == 0
        || name_len > 56
        || bytes[SOURCE_OFFSET + 96..704].iter().any(|&byte| byte != 0)
        || bytes[SOURCE_OFFSET + 40 + name_len..SOURCE_OFFSET + 96]
            .iter()
            .any(|&byte| byte != 0)
    {
        return Err(Error::InvalidSource);
    }
    let section_name = str::from_utf8(&bytes[SOURCE_OFFSET + 40..SOURCE_OFFSET + 40 + name_len])
        .map_err(|_| Error::InvalidUtf8)?;
    if section_name != SHADER_SOURCE_SECTION {
        return Err(Error::InvalidSource);
    }
    Ok(SourceRef {
        byte_len,
        sha256: hash_at(bytes, SOURCE_OFFSET + 8),
        section_name,
    })
}

fn attribute(bytes: &[u8], offset: usize) -> VertexAttribute {
    VertexAttribute {
        location: u16_value(bytes, offset),
        format: u16_value(bytes, offset + 2),
        offset: value(bytes, offset + 4),
        vf_component_mask: value(bytes, offset + 8),
    }
}

fn binding(bytes: &[u8], offset: usize) -> ResourceBinding {
    ResourceBinding {
        group: bytes[offset],
        binding: bytes[offset + 1],
        intel_bti: bytes[offset + 2],
        kind: bytes[offset + 3],
        visibility: bytes[offset + 4],
        access: bytes[offset + 5],
        min_binding_size: value(bytes, offset + 8),
        element_stride: value(bytes, offset + 12),
    }
}

fn parse_instancing(bytes: &[u8], offset: usize) -> Result<VfInstancing, Error> {
    let enabled = *bytes.get(offset + 2).ok_or(Error::WrongLength)?;
    if enabled > 1 || *bytes.get(offset + 3).ok_or(Error::WrongLength)? != 0 {
        return Err(Error::InvalidVertexFetch);
    }
    Ok(VfInstancing {
        element_index: u16_at(bytes, offset)?,
        enabled: enabled != 0,
        step_rate: u32_at(bytes, offset + 4)?,
    })
}

fn parse_synthetic_element(bytes: &[u8], offset: usize) -> Result<SyntheticVertexElement, Error> {
    if *bytes.get(offset + 3).ok_or(Error::WrongLength)? != 0 || u16_at(bytes, offset + 10)? != 0 {
        return Err(Error::InvalidVertexFetch);
    }
    Ok(SyntheticVertexElement {
        element_index: u16_at(bytes, offset)?,
        vertex_buffer_index: *bytes.get(offset + 2).ok_or(Error::WrongLength)?,
        surface_format: u16_at(bytes, offset + 4)?,
        component_controls: [
            bytes[offset + 6],
            bytes[offset + 7],
            bytes[offset + 8],
            bytes[offset + 9],
        ],
    })
}

fn hash_at(bytes: &[u8], offset: usize) -> [u8; 32] {
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&bytes[offset..offset + 32]);
    hash
}

fn u16_at(bytes: &[u8], offset: usize) -> Result<u16, Error> {
    let raw = bytes.get(offset..offset + 2).ok_or(Error::WrongLength)?;
    Ok(u16::from_le_bytes([raw[0], raw[1]]))
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, Error> {
    let raw = bytes.get(offset..offset + 4).ok_or(Error::WrongLength)?;
    Ok(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn value(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap_or([0; 4]))
}

fn u16_value(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap_or([0; 2]))
}
