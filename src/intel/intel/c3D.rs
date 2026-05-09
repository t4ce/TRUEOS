#![allow(dead_code)]

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum CommandStage {
    Pipeline3D,
    VertexFetch,
    VertexShader,
    GeometryShader,
    Clipper,
    StripsAndFans,
    Windower,
    HullShader,
    DomainShader,
    Tesselator,
    HwStreamout,
    Setup,
    PixelShader,
    CoarsePixelShader,
    ResourceStreamer,
    SamplingEngine,
    RenderComputePipeline,
    Pss,
    Wm,
    InternalState,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct CommandEntry {
    pub(crate) major: u8,
    pub(crate) minor: u8,
    pub(crate) name: &'static str,
    pub(crate) stage: CommandStage,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReservedRange {
    pub(crate) major: u8,
    pub(crate) minor_start: u8,
    pub(crate) minor_end: u8,
}

pub(crate) const CMD_3DSTATE_CLEAR_PARAMS: CommandEntry =
    entry(0x0, 0x04, "3DSTATE_CLEAR_PARAMS", CommandStage::Pipeline3D);
pub(crate) const CMD_3DSTATE_DEPTH_BUFFER: CommandEntry =
    entry(0x0, 0x05, "3DSTATE_DEPTH_BUFFER", CommandStage::Pipeline3D);
pub(crate) const CMD_3DSTATE_STENCIL_BUFFER: CommandEntry =
    entry(0x0, 0x06, "3DSTATE_STENCIL_BUFFER", CommandStage::Pipeline3D);
pub(crate) const CMD_3DSTATE_HIER_DEPTH_BUFFER: CommandEntry =
    entry(0x0, 0x07, "3DSTATE_HIER_DEPTH_BUFFER", CommandStage::Pipeline3D);
pub(crate) const CMD_3DSTATE_VERTEX_BUFFERS: CommandEntry =
    entry(0x0, 0x08, "3DSTATE_VERTEX_BUFFERS", CommandStage::VertexFetch);
pub(crate) const CMD_3DSTATE_VERTEX_ELEMENTS: CommandEntry =
    entry(0x0, 0x09, "3DSTATE_VERTEX_ELEMENTS", CommandStage::VertexFetch);
pub(crate) const CMD_3DSTATE_INDEX_BUFFER: CommandEntry =
    entry(0x0, 0x0A, "3DSTATE_INDEX_BUFFER", CommandStage::VertexFetch);
pub(crate) const CMD_3DSTATE_VF_STATISTICS: CommandEntry =
    entry(0x0, 0x0B, "3DSTATE_VF_STATISTICS", CommandStage::VertexFetch);
pub(crate) const CMD_3DSTATE_VF: CommandEntry =
    entry(0x0, 0x0C, "3DSTATE_VF", CommandStage::VertexFetch);
pub(crate) const CMD_3DSTATE_VIEWPORT_STATE_POINTERS: CommandEntry =
    entry(0x0, 0x0D, "3DSTATE_VIEWPORT_STATE_POINTERS", CommandStage::Pipeline3D);
pub(crate) const CMD_3DSTATE_CC_STATE_POINTERS: CommandEntry =
    entry(0x0, 0x0E, "3DSTATE_CC_STATE_POINTERS", CommandStage::Pipeline3D);
pub(crate) const CMD_3DSTATE_VS: CommandEntry =
    entry(0x0, 0x10, "3DSTATE_VS", CommandStage::VertexShader);
pub(crate) const CMD_3DSTATE_GS: CommandEntry =
    entry(0x0, 0x11, "3DSTATE_GS", CommandStage::GeometryShader);
pub(crate) const CMD_3DSTATE_CLIP: CommandEntry =
    entry(0x0, 0x12, "3DSTATE_CLIP", CommandStage::Clipper);
pub(crate) const CMD_3DSTATE_SF: CommandEntry =
    entry(0x0, 0x13, "3DSTATE_SF", CommandStage::StripsAndFans);
pub(crate) const CMD_3DSTATE_WM: CommandEntry =
    entry(0x0, 0x14, "3DSTATE_WM", CommandStage::Windower);
pub(crate) const CMD_3DSTATE_CONSTANT_VS: CommandEntry =
    entry(0x0, 0x15, "3DSTATE_CONSTANT_VS", CommandStage::VertexShader);
pub(crate) const CMD_3DSTATE_CONSTANT_GS: CommandEntry =
    entry(0x0, 0x16, "3DSTATE_CONSTANT_GS", CommandStage::GeometryShader);
pub(crate) const CMD_3DSTATE_CONSTANT_PS: CommandEntry =
    entry(0x0, 0x17, "3DSTATE_CONSTANT_PS", CommandStage::Windower);
pub(crate) const CMD_3DSTATE_SAMPLE_MASK: CommandEntry =
    entry(0x0, 0x18, "3DSTATE_SAMPLE_MASK", CommandStage::Windower);
pub(crate) const CMD_3DSTATE_CONSTANT_HS: CommandEntry =
    entry(0x0, 0x19, "3DSTATE_CONSTANT_HS", CommandStage::HullShader);
pub(crate) const CMD_3DSTATE_CONSTANT_DS: CommandEntry =
    entry(0x0, 0x1A, "3DSTATE_CONSTANT_DS", CommandStage::DomainShader);
pub(crate) const CMD_3DSTATE_HS: CommandEntry =
    entry(0x0, 0x1B, "3DSTATE_HS", CommandStage::HullShader);
pub(crate) const CMD_3DSTATE_TE: CommandEntry =
    entry(0x0, 0x1C, "3DSTATE_TE", CommandStage::Tesselator);
pub(crate) const CMD_3DSTATE_DS: CommandEntry =
    entry(0x0, 0x1D, "3DSTATE_DS", CommandStage::DomainShader);
pub(crate) const CMD_3DSTATE_STREAMOUT: CommandEntry =
    entry(0x0, 0x1E, "3DSTATE_STREAMOUT", CommandStage::HwStreamout);
pub(crate) const CMD_3DSTATE_SBE: CommandEntry =
    entry(0x0, 0x1F, "3DSTATE_SBE", CommandStage::Setup);
pub(crate) const CMD_3DSTATE_PS: CommandEntry =
    entry(0x0, 0x20, "3DSTATE_PS", CommandStage::PixelShader);
pub(crate) const CMD_3DSTATE_VIEWPORT_STATE_POINTERS_SF_CLIP: CommandEntry =
    entry(0x0, 0x21, "3DSTATE_VIEWPORT_STATE_POINTERS_SF_CLIP", CommandStage::StripsAndFans);
pub(crate) const CMD_3DSTATE_CPS_POINTER: CommandEntry =
    entry(0x0, 0x22, "3DSTATE_CPS_POINTER", CommandStage::CoarsePixelShader);
pub(crate) const CMD_3DSTATE_VIEWPORT_STATE_POINTERS_CC: CommandEntry =
    entry(0x0, 0x23, "3DSTATE_VIEWPORT_STATE_POINTERS_CC", CommandStage::Windower);
pub(crate) const CMD_3DSTATE_BLEND_STATE_POINTERS: CommandEntry =
    entry(0x0, 0x24, "3DSTATE_BLEND_STATE_POINTERS", CommandStage::PixelShader);
pub(crate) const CMD_3DSTATE_DEPTH_STENCIL_STATE_POINTERS: CommandEntry =
    entry(0x0, 0x25, "3DSTATE_DEPTH_STENCIL_STATE_POINTERS", CommandStage::PixelShader);
pub(crate) const CMD_3DSTATE_BINDING_TABLE_POINTERS_VS: CommandEntry =
    entry(0x0, 0x26, "3DSTATE_BINDING_TABLE_POINTERS_VS", CommandStage::VertexShader);
pub(crate) const CMD_3DSTATE_BINDING_TABLE_POINTERS_HS: CommandEntry =
    entry(0x0, 0x27, "3DSTATE_BINDING_TABLE_POINTERS_HS", CommandStage::HullShader);
pub(crate) const CMD_3DSTATE_BINDING_TABLE_POINTERS_DS: CommandEntry =
    entry(0x0, 0x28, "3DSTATE_BINDING_TABLE_POINTERS_DS", CommandStage::DomainShader);
pub(crate) const CMD_3DSTATE_BINDING_TABLE_POINTERS_GS: CommandEntry =
    entry(0x0, 0x29, "3DSTATE_BINDING_TABLE_POINTERS_GS", CommandStage::GeometryShader);
pub(crate) const CMD_3DSTATE_BINDING_TABLE_POINTERS_PS: CommandEntry =
    entry(0x0, 0x2A, "3DSTATE_BINDING_TABLE_POINTERS_PS", CommandStage::PixelShader);
pub(crate) const CMD_3DSTATE_SAMPLER_STATE_POINTERS_VS: CommandEntry =
    entry(0x0, 0x2B, "3DSTATE_SAMPLER_STATE_POINTERS_VS", CommandStage::VertexShader);
pub(crate) const CMD_3DSTATE_SAMPLER_STATE_POINTERS_HS: CommandEntry =
    entry(0x0, 0x2C, "3DSTATE_SAMPLER_STATE_POINTERS_HS", CommandStage::HullShader);
pub(crate) const CMD_3DSTATE_SAMPLER_STATE_POINTERS_DS: CommandEntry =
    entry(0x0, 0x2D, "3DSTATE_SAMPLER_STATE_POINTERS_DS", CommandStage::DomainShader);
pub(crate) const CMD_3DSTATE_SAMPLER_STATE_POINTERS_GS: CommandEntry =
    entry(0x0, 0x2E, "3DSTATE_SAMPLER_STATE_POINTERS_GS", CommandStage::GeometryShader);
pub(crate) const CMD_3DSTATE_SAMPLER_STATE_POINTERS_PS: CommandEntry =
    entry(0x0, 0x2F, "3DSTATE_SAMPLER_STATE_POINTERS_PS", CommandStage::PixelShader);
pub(crate) const CMD_3DSTATE_URB_VS: CommandEntry =
    entry(0x0, 0x30, "3DSTATE_URB_VS", CommandStage::VertexShader);
pub(crate) const CMD_3DSTATE_URB_HS: CommandEntry =
    entry(0x0, 0x31, "3DSTATE_URB_HS", CommandStage::HullShader);
pub(crate) const CMD_3DSTATE_URB_DS: CommandEntry =
    entry(0x0, 0x32, "3DSTATE_URB_DS", CommandStage::DomainShader);
pub(crate) const CMD_3DSTATE_URB_GS: CommandEntry =
    entry(0x0, 0x33, "3DSTATE_URB_GS", CommandStage::GeometryShader);
pub(crate) const CMD_3DSTATE_GATHER_CONSTANT_VS: CommandEntry =
    entry(0x0, 0x34, "3DSTATE_GATHER_CONSTANT_VS", CommandStage::VertexShader);
pub(crate) const CMD_3DSTATE_GATHER_CONSTANT_GS: CommandEntry =
    entry(0x0, 0x35, "3DSTATE_GATHER_CONSTANT_GS", CommandStage::GeometryShader);
pub(crate) const CMD_3DSTATE_GATHER_CONSTANT_HS: CommandEntry =
    entry(0x0, 0x36, "3DSTATE_GATHER_CONSTANT_HS", CommandStage::HullShader);
pub(crate) const CMD_3DSTATE_GATHER_CONSTANT_DS: CommandEntry =
    entry(0x0, 0x37, "3DSTATE_GATHER_CONSTANT_DS", CommandStage::DomainShader);
pub(crate) const CMD_3DSTATE_GATHER_CONSTANT_PS: CommandEntry =
    entry(0x0, 0x38, "3DSTATE_GATHER_CONSTANT_PS", CommandStage::PixelShader);
pub(crate) const CMD_3DSTATE_DX9_CONSTANTF_VS: CommandEntry =
    entry(0x0, 0x39, "3DSTATE_DX9_CONSTANTF_VS", CommandStage::VertexShader);
pub(crate) const CMD_3DSTATE_DX9_CONSTANTF_PS: CommandEntry =
    entry(0x0, 0x3A, "3DSTATE_DX9_CONSTANTF_PS", CommandStage::PixelShader);
pub(crate) const CMD_3DSTATE_DX9_CONSTANTI_VS: CommandEntry =
    entry(0x0, 0x3B, "3DSTATE_DX9_CONSTANTI_VS", CommandStage::VertexShader);
pub(crate) const CMD_3DSTATE_DX9_CONSTANTI_PS: CommandEntry =
    entry(0x0, 0x3C, "3DSTATE_DX9_CONSTANTI_PS", CommandStage::PixelShader);
pub(crate) const CMD_3DSTATE_DX9_CONSTANTB_VS: CommandEntry =
    entry(0x0, 0x3D, "3DSTATE_DX9_CONSTANTB_VS", CommandStage::VertexShader);
pub(crate) const CMD_3DSTATE_DX9_CONSTANTB_PS: CommandEntry =
    entry(0x0, 0x3E, "3DSTATE_DX9_CONSTANTB_PS", CommandStage::PixelShader);
pub(crate) const CMD_3DSTATE_DX9_LOCAL_VALID_VS: CommandEntry =
    entry(0x0, 0x3F, "3DSTATE_DX9_LOCAL_VALID_VS", CommandStage::VertexShader);
pub(crate) const CMD_3DSTATE_DX9_LOCAL_VALID_PS: CommandEntry =
    entry(0x0, 0x40, "3DSTATE_DX9_LOCAL_VALID_PS", CommandStage::PixelShader);
pub(crate) const CMD_3DSTATE_DX9_GENERATE_ACTIVE_VS: CommandEntry =
    entry(0x0, 0x41, "3DSTATE_DX9_GENERATE_ACTIVE_VS", CommandStage::VertexShader);
pub(crate) const CMD_3DSTATE_DX9_GENERATE_ACTIVE_PS: CommandEntry =
    entry(0x0, 0x42, "3DSTATE_DX9_GENERATE_ACTIVE_PS", CommandStage::PixelShader);
pub(crate) const CMD_3DSTATE_BINDING_TABLE_EDIT_VS: CommandEntry =
    entry(0x0, 0x43, "3DSTATE_BINDING_TABLE_EDIT_VS", CommandStage::VertexShader);
pub(crate) const CMD_3DSTATE_BINDING_TABLE_EDIT_GS: CommandEntry =
    entry(0x0, 0x44, "3DSTATE_BINDING_TABLE_EDIT_GS", CommandStage::GeometryShader);
pub(crate) const CMD_3DSTATE_BINDING_TABLE_EDIT_HS: CommandEntry =
    entry(0x0, 0x45, "3DSTATE_BINDING_TABLE_EDIT_HS", CommandStage::HullShader);
pub(crate) const CMD_3DSTATE_BINDING_TABLE_EDIT_DS: CommandEntry =
    entry(0x0, 0x46, "3DSTATE_BINDING_TABLE_EDIT_DS", CommandStage::DomainShader);
pub(crate) const CMD_3DSTATE_BINDING_TABLE_EDIT_PS: CommandEntry =
    entry(0x0, 0x47, "3DSTATE_BINDING_TABLE_EDIT_PS", CommandStage::PixelShader);
pub(crate) const CMD_3DSTATE_VF_HASHING: CommandEntry =
    entry(0x0, 0x48, "3DSTATE_VF_HASHING", CommandStage::VertexFetch);
pub(crate) const CMD_3DSTATE_VF_INSTANCING: CommandEntry =
    entry(0x0, 0x49, "3DSTATE_VF_INSTANCING", CommandStage::VertexFetch);
pub(crate) const CMD_3DSTATE_VF_SGVS: CommandEntry =
    entry(0x0, 0x4A, "3DSTATE_VF_SGVS", CommandStage::VertexFetch);
pub(crate) const CMD_3DSTATE_VF_TOPOLOGY: CommandEntry =
    entry(0x0, 0x4B, "3DSTATE_VF_TOPOLOGY", CommandStage::VertexFetch);
pub(crate) const CMD_3DSTATE_WM_CHROMA_KEY: CommandEntry =
    entry(0x0, 0x4C, "3DSTATE_WM_CHROMA_KEY", CommandStage::Windower);
pub(crate) const CMD_3DSTATE_PS_BLEND: CommandEntry =
    entry(0x0, 0x4D, "3DSTATE_PS_BLEND", CommandStage::Windower);
pub(crate) const CMD_3DSTATE_WM_DEPTH_STENCIL: CommandEntry =
    entry(0x0, 0x4E, "3DSTATE_WM_DEPTH_STENCIL", CommandStage::Windower);
pub(crate) const CMD_3DSTATE_PS_EXTRA: CommandEntry =
    entry(0x0, 0x4F, "3DSTATE_PS_EXTRA", CommandStage::Windower);
pub(crate) const CMD_3DSTATE_RASTER: CommandEntry =
    entry(0x0, 0x50, "3DSTATE_RASTER", CommandStage::StripsAndFans);
pub(crate) const CMD_3DSTATE_SBE_SWIZ: CommandEntry =
    entry(0x0, 0x51, "3DSTATE_SBE_SWIZ", CommandStage::StripsAndFans);
pub(crate) const CMD_3DSTATE_WM_HZ_OP: CommandEntry =
    entry(0x0, 0x52, "3DSTATE_WM_HZ_OP", CommandStage::Windower);
pub(crate) const CMD_3DSTATE_INT: CommandEntry =
    entry(0x0, 0x53, "3DSTATE_INT", CommandStage::InternalState);
pub(crate) const CMD_3DSTATE_RS_CONSTANT_POINTER: CommandEntry =
    entry(0x0, 0x54, "3DSTATE_RS_CONSTANT_POINTER", CommandStage::ResourceStreamer);
pub(crate) const CMD_3DSTATE_VF_COMPONENT_PACKING: CommandEntry =
    entry(0x0, 0x55, "3DSTATE_VF_COMPONENT_PACKING", CommandStage::VertexFetch);
pub(crate) const CMD_3DSTATE_VF_SGVS_2: CommandEntry =
    entry(0x0, 0x56, "3DSTATE_VF_SGVS_2", CommandStage::VertexFetch);
pub(crate) const CMD_3DSTATE_URB_ALLOC_VS: CommandEntry =
    entry(0x0, 0x58, "3DSTATE_URB_ALLOC_VS", CommandStage::VertexShader);
pub(crate) const CMD_3DSTATE_URB_ALLOC_HS: CommandEntry =
    entry(0x0, 0x59, "3DSTATE_URB_ALLOC_HS", CommandStage::HullShader);
pub(crate) const CMD_3DSTATE_URB_ALLOC_DS: CommandEntry =
    entry(0x0, 0x5A, "3DSTATE_URB_ALLOC_DS", CommandStage::DomainShader);
pub(crate) const CMD_3DSTATE_URB_ALLOC_GS: CommandEntry =
    entry(0x0, 0x5B, "3DSTATE_URB_ALLOC_GS", CommandStage::GeometryShader);
pub(crate) const CMD_3DSTATE_SO_BUFFER_INDEX_0: CommandEntry =
    entry(0x0, 0x60, "3DSTATE_SO_BUFFER_INDEX_0", CommandStage::HwStreamout);
pub(crate) const CMD_3DSTATE_SO_BUFFER_INDEX_1: CommandEntry =
    entry(0x0, 0x61, "3DSTATE_SO_BUFFER_INDEX_1", CommandStage::HwStreamout);
pub(crate) const CMD_3DSTATE_SO_BUFFER_INDEX_2: CommandEntry =
    entry(0x0, 0x62, "3DSTATE_SO_BUFFER_INDEX_2", CommandStage::HwStreamout);
pub(crate) const CMD_3DSTATE_SO_BUFFER_INDEX_3: CommandEntry =
    entry(0x0, 0x63, "3DSTATE_SO_BUFFER_INDEX_3", CommandStage::HwStreamout);
pub(crate) const CMD_3DSTATE_PTBR_MARKER: CommandEntry =
    entry(0x0, 0x6A, "3DSTATE_PTBR_MARKER", CommandStage::Pipeline3D);
pub(crate) const CMD_3DSTATE_PTBR_TILE_SELECT: CommandEntry =
    entry(0x0, 0x6B, "3DSTATE_PTBR_TILE_SELECT", CommandStage::StripsAndFans);
pub(crate) const CMD_3DSTATE_PRIMITIVE_REPLICATION: CommandEntry =
    entry(0x0, 0x6C, "3DSTATE_PRIMITIVE_REPLICATION", CommandStage::Pipeline3D);
pub(crate) const CMD_3DSTATE_CONSTANT_ALL: CommandEntry =
    entry(0x0, 0x6D, "3DSTATE_CONSTANT_ALL", CommandStage::Pipeline3D);
pub(crate) const CMD_3DSTATE_AMFS: CommandEntry =
    entry(0x0, 0x6F, "3DSTATE_AMFS", CommandStage::Pss);
pub(crate) const CMD_3DSTATE_DEPTH_CNTL_BUFFER: CommandEntry =
    entry(0x0, 0x70, "3DSTATE_DEPTH_CNTL_BUFFER", CommandStage::Wm);
pub(crate) const CMD_3DSTATE_DEPTH_BOUNDS: CommandEntry =
    entry(0x0, 0x71, "3DSTATE_DEPTH_BOUNDS", CommandStage::Wm);
pub(crate) const CMD_3DSTATE_AMFS_TEXTURE_POINTERS: CommandEntry =
    entry(0x0, 0x72, "3DSTATE_AMFS_TEXTURE_POINTERS", CommandStage::Wm);
pub(crate) const CMD_3DSTATE_CONSTANT_TS_POINTER: CommandEntry =
    entry(0x0, 0x73, "3DSTATE_CONSTANT_TS_POINTER", CommandStage::Pss);

pub(crate) const CMD_3DSTATE_DRAWING_RECTANGLE: CommandEntry =
    entry(0x1, 0x00, "3DSTATE_DRAWING_RECTANGLE", CommandStage::StripsAndFans);
pub(crate) const CMD_3DSTATE_CHROMA_KEY: CommandEntry =
    entry(0x1, 0x04, "3DSTATE_CHROMA_KEY", CommandStage::SamplingEngine);
pub(crate) const CMD_3DSTATE_POLY_STIPPLE_OFFSET: CommandEntry =
    entry(0x1, 0x06, "3DSTATE_POLY_STIPPLE_OFFSET", CommandStage::Windower);
pub(crate) const CMD_3DSTATE_POLY_STIPPLE_PATTERN: CommandEntry =
    entry(0x1, 0x07, "3DSTATE_POLY_STIPPLE_PATTERN", CommandStage::Windower);
pub(crate) const CMD_3DSTATE_LINE_STIPPLE: CommandEntry =
    entry(0x1, 0x08, "3DSTATE_LINE_STIPPLE", CommandStage::Windower);
pub(crate) const CMD_3DSTATE_AA_LINE_PARAMS: CommandEntry =
    entry(0x1, 0x0A, "3DSTATE_AA_LINE_PARAMS", CommandStage::Windower);
pub(crate) const CMD_3DSTATE_GS_SVB_INDEX: CommandEntry =
    entry(0x1, 0x0B, "3DSTATE_GS_SVB_INDEX", CommandStage::GeometryShader);
pub(crate) const CMD_3DSTATE_MULTISAMPLE: CommandEntry =
    entry(0x1, 0x0D, "3DSTATE_MULTISAMPLE", CommandStage::Windower);
pub(crate) const CMD_3DSTATE_STENCIL_BUFFER_WM: CommandEntry =
    entry(0x1, 0x0E, "3DSTATE_STENCIL_BUFFER", CommandStage::Windower);
pub(crate) const CMD_3DSTATE_HIER_DEPTH_BUFFER_WM: CommandEntry =
    entry(0x1, 0x0F, "3DSTATE_HIER_DEPTH_BUFFER", CommandStage::Windower);
pub(crate) const CMD_3DSTATE_CLEAR_PARAMS_WM: CommandEntry =
    entry(0x1, 0x10, "3DSTATE_CLEAR_PARAMS", CommandStage::Windower);
pub(crate) const CMD_3DSTATE_MONOFILTER_SIZE: CommandEntry =
    entry(0x1, 0x11, "3DSTATE_MONOFILTER_SIZE", CommandStage::SamplingEngine);
pub(crate) const CMD_3DSTATE_PUSH_CONSTANT_ALLOC_VS: CommandEntry =
    entry(0x1, 0x12, "3DSTATE_PUSH_CONSTANT_ALLOC_VS", CommandStage::VertexShader);
pub(crate) const CMD_3DSTATE_PUSH_CONSTANT_ALLOC_HS: CommandEntry =
    entry(0x1, 0x13, "3DSTATE_PUSH_CONSTANT_ALLOC_HS", CommandStage::HullShader);
pub(crate) const CMD_3DSTATE_PUSH_CONSTANT_ALLOC_DS: CommandEntry =
    entry(0x1, 0x14, "3DSTATE_PUSH_CONSTANT_ALLOC_DS", CommandStage::DomainShader);
pub(crate) const CMD_3DSTATE_PUSH_CONSTANT_ALLOC_GS: CommandEntry =
    entry(0x1, 0x15, "3DSTATE_PUSH_CONSTANT_ALLOC_GS", CommandStage::GeometryShader);
pub(crate) const CMD_3DSTATE_PUSH_CONSTANT_ALLOC_PS: CommandEntry =
    entry(0x1, 0x16, "3DSTATE_PUSH_CONSTANT_ALLOC_PS", CommandStage::PixelShader);
pub(crate) const CMD_3DSTATE_SO_DECL_LIST: CommandEntry =
    entry(0x1, 0x17, "3DSTATE_SO_DECL_LIST", CommandStage::HwStreamout);
pub(crate) const CMD_3DSTATE_SO_BUFFER: CommandEntry =
    entry(0x1, 0x18, "3DSTATE_SO_BUFFER", CommandStage::HwStreamout);
pub(crate) const CMD_3DSTATE_BINDING_TABLE_POOL_ALLOC: CommandEntry =
    entry(0x1, 0x19, "3DSTATE_BINDING_TABLE_POOL_ALLOC", CommandStage::ResourceStreamer);
pub(crate) const CMD_3DSTATE_GATHER_POOL_ALLOC: CommandEntry =
    entry(0x1, 0x1A, "3DSTATE_GATHER_POOL_ALLOC", CommandStage::ResourceStreamer);
pub(crate) const CMD_3DSTATE_DX9_CONSTANT_BUFFER_POOL_ALLOC: CommandEntry =
    entry(0x1, 0x1B, "3DSTATE_DX9_CONSTANT_BUFFER_POOL_ALLOC", CommandStage::ResourceStreamer);
pub(crate) const CMD_3DSTATE_SAMPLE_PATTERN: CommandEntry =
    entry(0x1, 0x1C, "3DSTATE_SAMPLE_PATTERN", CommandStage::Windower);
pub(crate) const CMD_3DSTATE_URB_CLEAR: CommandEntry =
    entry(0x1, 0x1D, "3DSTATE_URB_CLEAR", CommandStage::Pipeline3D);
pub(crate) const CMD_3DSTATE_3D_MODE: CommandEntry =
    entry(0x1, 0x1E, "3DSTATE_3D_MODE", CommandStage::Pipeline3D);
pub(crate) const CMD_3DSTATE_SUBSLICE_HASH_TABLE: CommandEntry =
    entry(0x1, 0x1F, "3DSTATE_SUBSLICE_HASH_TABLE", CommandStage::Pipeline3D);
pub(crate) const CMD_3DSTATE_SLICE_TABLE_STATE_POINTERS: CommandEntry =
    entry(0x1, 0x20, "3DSTATE_SLICE_TABLE_STATE_POINTERS", CommandStage::Pipeline3D);
pub(crate) const CMD_3DSTATE_PTBR_PAGE_POOL_BASE_ADDRESS: CommandEntry =
    entry(0x1, 0x21, "3DSTATE_PTBR_PAGE_POOL_BASE_ADDRESS", CommandStage::Pipeline3D);
pub(crate) const CMD_3DSTATE_PTBR_TILE_PASS_INFO: CommandEntry =
    entry(0x1, 0x22, "3DSTATE_PTBR_TILE_PASS_INFO", CommandStage::Pipeline3D);
pub(crate) const CMD_3DSTATE_PTBR_RENDER_LIST_BASE_ADDRESS: CommandEntry =
    entry(0x1, 0x23, "3DSTATE_PTBR_RENDER_LIST_BASE_ADDRESS", CommandStage::Pipeline3D);
pub(crate) const CMD_3DSTATE_PTBR_FREE_LIST_BASE_ADDRESS: CommandEntry =
    entry(0x1, 0x24, "3DSTATE_PTBR_FREE_LIST_BASE_ADDRESS", CommandStage::Pipeline3D);

pub(crate) const CMD_PIPE_CONTROL: CommandEntry =
    entry(0x2, 0x00, "PIPE_CONTROL", CommandStage::RenderComputePipeline);

pub(crate) const CMD_3DPRIMITIVE: CommandEntry =
    entry(0x3, 0x00, "3DPRIMITIVE", CommandStage::VertexFetch);

pub(crate) const COMMANDS: &[CommandEntry] = &[
    CMD_3DSTATE_CLEAR_PARAMS,
    CMD_3DSTATE_DEPTH_BUFFER,
    CMD_3DSTATE_STENCIL_BUFFER,
    CMD_3DSTATE_HIER_DEPTH_BUFFER,
    CMD_3DSTATE_VERTEX_BUFFERS,
    CMD_3DSTATE_VERTEX_ELEMENTS,
    CMD_3DSTATE_INDEX_BUFFER,
    CMD_3DSTATE_VF_STATISTICS,
    CMD_3DSTATE_VF,
    CMD_3DSTATE_VIEWPORT_STATE_POINTERS,
    CMD_3DSTATE_CC_STATE_POINTERS,
    CMD_3DSTATE_VS,
    CMD_3DSTATE_GS,
    CMD_3DSTATE_CLIP,
    CMD_3DSTATE_SF,
    CMD_3DSTATE_WM,
    CMD_3DSTATE_CONSTANT_VS,
    CMD_3DSTATE_CONSTANT_GS,
    CMD_3DSTATE_CONSTANT_PS,
    CMD_3DSTATE_SAMPLE_MASK,
    CMD_3DSTATE_CONSTANT_HS,
    CMD_3DSTATE_CONSTANT_DS,
    CMD_3DSTATE_HS,
    CMD_3DSTATE_TE,
    CMD_3DSTATE_DS,
    CMD_3DSTATE_STREAMOUT,
    CMD_3DSTATE_SBE,
    CMD_3DSTATE_PS,
    CMD_3DSTATE_VIEWPORT_STATE_POINTERS_SF_CLIP,
    CMD_3DSTATE_CPS_POINTER,
    CMD_3DSTATE_VIEWPORT_STATE_POINTERS_CC,
    CMD_3DSTATE_BLEND_STATE_POINTERS,
    CMD_3DSTATE_DEPTH_STENCIL_STATE_POINTERS,
    CMD_3DSTATE_BINDING_TABLE_POINTERS_VS,
    CMD_3DSTATE_BINDING_TABLE_POINTERS_HS,
    CMD_3DSTATE_BINDING_TABLE_POINTERS_DS,
    CMD_3DSTATE_BINDING_TABLE_POINTERS_GS,
    CMD_3DSTATE_BINDING_TABLE_POINTERS_PS,
    CMD_3DSTATE_SAMPLER_STATE_POINTERS_VS,
    CMD_3DSTATE_SAMPLER_STATE_POINTERS_HS,
    CMD_3DSTATE_SAMPLER_STATE_POINTERS_DS,
    CMD_3DSTATE_SAMPLER_STATE_POINTERS_GS,
    CMD_3DSTATE_SAMPLER_STATE_POINTERS_PS,
    CMD_3DSTATE_URB_VS,
    CMD_3DSTATE_URB_HS,
    CMD_3DSTATE_URB_DS,
    CMD_3DSTATE_URB_GS,
    CMD_3DSTATE_GATHER_CONSTANT_VS,
    CMD_3DSTATE_GATHER_CONSTANT_GS,
    CMD_3DSTATE_GATHER_CONSTANT_HS,
    CMD_3DSTATE_GATHER_CONSTANT_DS,
    CMD_3DSTATE_GATHER_CONSTANT_PS,
    CMD_3DSTATE_DX9_CONSTANTF_VS,
    CMD_3DSTATE_DX9_CONSTANTF_PS,
    CMD_3DSTATE_DX9_CONSTANTI_VS,
    CMD_3DSTATE_DX9_CONSTANTI_PS,
    CMD_3DSTATE_DX9_CONSTANTB_VS,
    CMD_3DSTATE_DX9_CONSTANTB_PS,
    CMD_3DSTATE_DX9_LOCAL_VALID_VS,
    CMD_3DSTATE_DX9_LOCAL_VALID_PS,
    CMD_3DSTATE_DX9_GENERATE_ACTIVE_VS,
    CMD_3DSTATE_DX9_GENERATE_ACTIVE_PS,
    CMD_3DSTATE_BINDING_TABLE_EDIT_VS,
    CMD_3DSTATE_BINDING_TABLE_EDIT_GS,
    CMD_3DSTATE_BINDING_TABLE_EDIT_HS,
    CMD_3DSTATE_BINDING_TABLE_EDIT_DS,
    CMD_3DSTATE_BINDING_TABLE_EDIT_PS,
    CMD_3DSTATE_VF_HASHING,
    CMD_3DSTATE_VF_INSTANCING,
    CMD_3DSTATE_VF_SGVS,
    CMD_3DSTATE_VF_TOPOLOGY,
    CMD_3DSTATE_WM_CHROMA_KEY,
    CMD_3DSTATE_PS_BLEND,
    CMD_3DSTATE_WM_DEPTH_STENCIL,
    CMD_3DSTATE_PS_EXTRA,
    CMD_3DSTATE_RASTER,
    CMD_3DSTATE_SBE_SWIZ,
    CMD_3DSTATE_WM_HZ_OP,
    CMD_3DSTATE_INT,
    CMD_3DSTATE_RS_CONSTANT_POINTER,
    CMD_3DSTATE_VF_COMPONENT_PACKING,
    CMD_3DSTATE_VF_SGVS_2,
    CMD_3DSTATE_URB_ALLOC_VS,
    CMD_3DSTATE_URB_ALLOC_HS,
    CMD_3DSTATE_URB_ALLOC_DS,
    CMD_3DSTATE_URB_ALLOC_GS,
    CMD_3DSTATE_SO_BUFFER_INDEX_0,
    CMD_3DSTATE_SO_BUFFER_INDEX_1,
    CMD_3DSTATE_SO_BUFFER_INDEX_2,
    CMD_3DSTATE_SO_BUFFER_INDEX_3,
    CMD_3DSTATE_PTBR_MARKER,
    CMD_3DSTATE_PTBR_TILE_SELECT,
    CMD_3DSTATE_PRIMITIVE_REPLICATION,
    CMD_3DSTATE_CONSTANT_ALL,
    CMD_3DSTATE_AMFS,
    CMD_3DSTATE_DEPTH_CNTL_BUFFER,
    CMD_3DSTATE_DEPTH_BOUNDS,
    CMD_3DSTATE_AMFS_TEXTURE_POINTERS,
    CMD_3DSTATE_CONSTANT_TS_POINTER,
    CMD_3DSTATE_DRAWING_RECTANGLE,
    CMD_3DSTATE_CHROMA_KEY,
    CMD_3DSTATE_POLY_STIPPLE_OFFSET,
    CMD_3DSTATE_POLY_STIPPLE_PATTERN,
    CMD_3DSTATE_LINE_STIPPLE,
    CMD_3DSTATE_AA_LINE_PARAMS,
    CMD_3DSTATE_GS_SVB_INDEX,
    CMD_3DSTATE_MULTISAMPLE,
    CMD_3DSTATE_STENCIL_BUFFER_WM,
    CMD_3DSTATE_HIER_DEPTH_BUFFER_WM,
    CMD_3DSTATE_CLEAR_PARAMS_WM,
    CMD_3DSTATE_MONOFILTER_SIZE,
    CMD_3DSTATE_PUSH_CONSTANT_ALLOC_VS,
    CMD_3DSTATE_PUSH_CONSTANT_ALLOC_HS,
    CMD_3DSTATE_PUSH_CONSTANT_ALLOC_DS,
    CMD_3DSTATE_PUSH_CONSTANT_ALLOC_GS,
    CMD_3DSTATE_PUSH_CONSTANT_ALLOC_PS,
    CMD_3DSTATE_SO_DECL_LIST,
    CMD_3DSTATE_SO_BUFFER,
    CMD_3DSTATE_BINDING_TABLE_POOL_ALLOC,
    CMD_3DSTATE_GATHER_POOL_ALLOC,
    CMD_3DSTATE_DX9_CONSTANT_BUFFER_POOL_ALLOC,
    CMD_3DSTATE_SAMPLE_PATTERN,
    CMD_3DSTATE_URB_CLEAR,
    CMD_3DSTATE_3D_MODE,
    CMD_3DSTATE_SUBSLICE_HASH_TABLE,
    CMD_3DSTATE_SLICE_TABLE_STATE_POINTERS,
    CMD_3DSTATE_PTBR_PAGE_POOL_BASE_ADDRESS,
    CMD_3DSTATE_PTBR_TILE_PASS_INFO,
    CMD_3DSTATE_PTBR_RENDER_LIST_BASE_ADDRESS,
    CMD_3DSTATE_PTBR_FREE_LIST_BASE_ADDRESS,
    CMD_PIPE_CONTROL,
    CMD_3DPRIMITIVE,
];

pub(crate) const RESERVED_RANGES: &[ReservedRange] = &[
    reserved(0x0, 0x01, 0x03),
    reserved(0x0, 0x0F, 0x0F),
    reserved(0x0, 0x57, 0x57),
    reserved(0x0, 0x5C, 0x5F),
    reserved(0x0, 0x64, 0x69),
    reserved(0x0, 0x6E, 0x6E),
    reserved(0x0, 0x74, 0xFF),
    reserved(0x1, 0x01, 0x03),
    reserved(0x1, 0x05, 0x05),
    reserved(0x1, 0x09, 0x09),
    reserved(0x1, 0x0C, 0x0C),
    reserved(0x1, 0x23, 0x2A),
    reserved(0x1, 0x2B, 0xFF),
    reserved(0x2, 0x01, 0xFF),
    reserved(0x3, 0x01, 0xFF),
    reserved(0x4, 0x00, 0xFF),
    reserved(0x5, 0x00, 0xFF),
    reserved(0x6, 0x00, 0xFF),
    reserved(0x7, 0x00, 0xFF),
];

pub(crate) fn command_name(major: u8, minor: u8) -> Option<&'static str> {
    command_entry(major, minor).map(|entry| entry.name)
}

pub(crate) fn command_entry(major: u8, minor: u8) -> Option<&'static CommandEntry> {
    COMMANDS
        .iter()
        .find(|entry| entry.major == major && entry.minor == minor)
}

pub(crate) fn is_reserved(major: u8, minor: u8) -> bool {
    RESERVED_RANGES
        .iter()
        .any(|range| range.major == major && minor >= range.minor_start && minor <= range.minor_end)
}

const fn entry(major: u8, minor: u8, name: &'static str, stage: CommandStage) -> CommandEntry {
    CommandEntry {
        major,
        minor,
        name,
        stage,
    }
}

const fn reserved(major: u8, minor_start: u8, minor_end: u8) -> ReservedRange {
    ReservedRange {
        major,
        minor_start,
        minor_end,
    }
}
