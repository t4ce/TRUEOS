const XELP_RENDER_FIRST_DEFER_DISPLAY_DISCOVERY: bool = true;

#[inline]
pub(super) fn defer_display_discovery_for_render_first(intel_igpu770_present: bool) -> bool {
	XELP_RENDER_FIRST_DEFER_DISPLAY_DISCOVERY && intel_igpu770_present
}

#[inline]
pub(super) fn log_display_deferred_for_render_first() {
	crate::log!(
		"intel: display discovery deferred (render-first mode; run display-engine probe later)\n"
	);
}

pub(crate) mod xelp_3dstate {
	pub const OPCODE_GROUP_0: u8 = 0x0;
	pub const OPCODE_GROUP_1: u8 = 0x1;

	// Encodes only the documented opcode fields at bits 26:16.
	#[inline]
	pub const fn opcode_key(opcode_group: u8, sub_opcode: u8) -> u32 {
		(((opcode_group as u32) & 0x7) << 24) | ((sub_opcode as u32) << 16)
	}

	pub const DEPTH_STENCIL_STATE_POINTERS: u32 = opcode_key(OPCODE_GROUP_0, 0x25);
	pub const BINDING_TABLE_POINTERS_VS: u32 = opcode_key(OPCODE_GROUP_0, 0x26);
	pub const BINDING_TABLE_POINTERS_HS: u32 = opcode_key(OPCODE_GROUP_0, 0x27);
	pub const BINDING_TABLE_POINTERS_DS: u32 = opcode_key(OPCODE_GROUP_0, 0x28);
	pub const BINDING_TABLE_POINTERS_GS: u32 = opcode_key(OPCODE_GROUP_0, 0x29);
	pub const BINDING_TABLE_POINTERS_PS: u32 = opcode_key(OPCODE_GROUP_0, 0x2A);
	pub const SAMPLER_STATE_POINTERS_VS: u32 = opcode_key(OPCODE_GROUP_0, 0x2B);
	pub const SAMPLER_STATE_POINTERS_HS: u32 = opcode_key(OPCODE_GROUP_0, 0x2C);
	pub const SAMPLER_STATE_POINTERS_DS: u32 = opcode_key(OPCODE_GROUP_0, 0x2D);
	pub const SAMPLER_STATE_POINTERS_GS: u32 = opcode_key(OPCODE_GROUP_0, 0x2E);
	pub const SAMPLER_STATE_POINTERS_PS: u32 = opcode_key(OPCODE_GROUP_0, 0x2F);
	pub const URB_VS: u32 = opcode_key(OPCODE_GROUP_0, 0x30);
	pub const URB_HS: u32 = opcode_key(OPCODE_GROUP_0, 0x31);
	pub const URB_DS: u32 = opcode_key(OPCODE_GROUP_0, 0x32);
	pub const URB_GS: u32 = opcode_key(OPCODE_GROUP_0, 0x33);
	pub const GATHER_CONSTANT_VS: u32 = opcode_key(OPCODE_GROUP_0, 0x34);
	pub const GATHER_CONSTANT_GS: u32 = opcode_key(OPCODE_GROUP_0, 0x35);
	pub const GATHER_CONSTANT_HS: u32 = opcode_key(OPCODE_GROUP_0, 0x36);
	pub const GATHER_CONSTANT_DS: u32 = opcode_key(OPCODE_GROUP_0, 0x37);
	pub const GATHER_CONSTANT_PS: u32 = opcode_key(OPCODE_GROUP_0, 0x38);
	pub const DX9_CONSTANTF_VS: u32 = opcode_key(OPCODE_GROUP_0, 0x39);
	pub const DX9_CONSTANTF_PS: u32 = opcode_key(OPCODE_GROUP_0, 0x3A);
	pub const DX9_CONSTANTI_VS: u32 = opcode_key(OPCODE_GROUP_0, 0x3B);
	pub const DX9_CONSTANTI_PS: u32 = opcode_key(OPCODE_GROUP_0, 0x3C);
	pub const DX9_CONSTANTB_VS: u32 = opcode_key(OPCODE_GROUP_0, 0x3D);
	pub const DX9_CONSTANTB_PS: u32 = opcode_key(OPCODE_GROUP_0, 0x3E);
	pub const DX9_LOCAL_VALID_VS: u32 = opcode_key(OPCODE_GROUP_0, 0x3F);
	pub const DX9_LOCAL_VALID_PS: u32 = opcode_key(OPCODE_GROUP_0, 0x40);
	pub const DX9_GENERATE_ACTIVE_VS: u32 = opcode_key(OPCODE_GROUP_0, 0x41);
	pub const DX9_GENERATE_ACTIVE_PS: u32 = opcode_key(OPCODE_GROUP_0, 0x42);
	pub const BINDING_TABLE_EDIT_VS: u32 = opcode_key(OPCODE_GROUP_0, 0x43);
	pub const BINDING_TABLE_EDIT_GS: u32 = opcode_key(OPCODE_GROUP_0, 0x44);
	pub const BINDING_TABLE_EDIT_HS: u32 = opcode_key(OPCODE_GROUP_0, 0x45);
	pub const BINDING_TABLE_EDIT_DS: u32 = opcode_key(OPCODE_GROUP_0, 0x46);
	pub const BINDING_TABLE_EDIT_PS: u32 = opcode_key(OPCODE_GROUP_0, 0x47);
	pub const VF_HASHING: u32 = opcode_key(OPCODE_GROUP_0, 0x48);
	pub const VF_INSTANCING: u32 = opcode_key(OPCODE_GROUP_0, 0x49);
	pub const VF_SGVS: u32 = opcode_key(OPCODE_GROUP_0, 0x4A);
	pub const VF_TOPOLOGY: u32 = opcode_key(OPCODE_GROUP_0, 0x4B);
	pub const WM_CHROMA_KEY: u32 = opcode_key(OPCODE_GROUP_0, 0x4C);
	pub const PS_BLEND: u32 = opcode_key(OPCODE_GROUP_0, 0x4D);
	pub const WM_DEPTH_STENCIL: u32 = opcode_key(OPCODE_GROUP_0, 0x4E);
	pub const PS_EXTRA: u32 = opcode_key(OPCODE_GROUP_0, 0x4F);
	pub const RASTER: u32 = opcode_key(OPCODE_GROUP_0, 0x50);
	pub const SBE_SWIZ: u32 = opcode_key(OPCODE_GROUP_0, 0x51);
	pub const WM_HZ_OP: u32 = opcode_key(OPCODE_GROUP_0, 0x52);
	pub const INT: u32 = opcode_key(OPCODE_GROUP_0, 0x53);
	pub const RS_CONSTANT_POINTER: u32 = opcode_key(OPCODE_GROUP_0, 0x54);
	pub const VF_COMPONENT_PACKING: u32 = opcode_key(OPCODE_GROUP_0, 0x55);
	pub const VF_SGVS_2: u32 = opcode_key(OPCODE_GROUP_0, 0x56);
	pub const URB_ALLOC_VS: u32 = opcode_key(OPCODE_GROUP_0, 0x58);
	pub const URB_ALLOC_HS: u32 = opcode_key(OPCODE_GROUP_0, 0x59);
	pub const URB_ALLOC_DS: u32 = opcode_key(OPCODE_GROUP_0, 0x5A);
	pub const URB_ALLOC_GS: u32 = opcode_key(OPCODE_GROUP_0, 0x5B);
	pub const SO_BUFFER_INDEX_0: u32 = opcode_key(OPCODE_GROUP_0, 0x60);
	pub const SO_BUFFER_INDEX_1: u32 = opcode_key(OPCODE_GROUP_0, 0x61);
	pub const SO_BUFFER_INDEX_2: u32 = opcode_key(OPCODE_GROUP_0, 0x62);
	pub const SO_BUFFER_INDEX_3: u32 = opcode_key(OPCODE_GROUP_0, 0x63);
	pub const PTBR_MARKER: u32 = opcode_key(OPCODE_GROUP_0, 0x6A);
	pub const PTBR_TILE_SELECT: u32 = opcode_key(OPCODE_GROUP_0, 0x6B);
	pub const PRIMITIVE_REPLICATION: u32 = opcode_key(OPCODE_GROUP_0, 0x6C);
	pub const CONSTANT_ALL: u32 = opcode_key(OPCODE_GROUP_0, 0x6D);
	pub const AMFS: u32 = opcode_key(OPCODE_GROUP_0, 0x6F);
	pub const DEPTH_CNTL_BUFFER: u32 = opcode_key(OPCODE_GROUP_0, 0x70);
	pub const DEPTH_BOUNDS: u32 = opcode_key(OPCODE_GROUP_0, 0x71);
	pub const AMFS_TEXTURE_POINTERS: u32 = opcode_key(OPCODE_GROUP_0, 0x72);
	pub const CONSTANT_TS_POINTER: u32 = opcode_key(OPCODE_GROUP_0, 0x73);

	pub const DRAWING_RECTANGLE: u32 = opcode_key(OPCODE_GROUP_1, 0x00);
	pub const CHROMA_KEY: u32 = opcode_key(OPCODE_GROUP_1, 0x04);
	pub const POLY_STIPPLE_OFFSET: u32 = opcode_key(OPCODE_GROUP_1, 0x06);
	pub const POLY_STIPPLE_PATTERN: u32 = opcode_key(OPCODE_GROUP_1, 0x07);
	pub const LINE_STIPPLE: u32 = opcode_key(OPCODE_GROUP_1, 0x08);
	pub const AA_LINE_PARAMS: u32 = opcode_key(OPCODE_GROUP_1, 0x0A);
	pub const GS_SVB_INDEX: u32 = opcode_key(OPCODE_GROUP_1, 0x0B);
	pub const MULTISAMPLE: u32 = opcode_key(OPCODE_GROUP_1, 0x0D);
	pub const STENCIL_BUFFER: u32 = opcode_key(OPCODE_GROUP_1, 0x0E);
	pub const HIER_DEPTH_BUFFER: u32 = opcode_key(OPCODE_GROUP_1, 0x0F);
	pub const CLEAR_PARAMS: u32 = opcode_key(OPCODE_GROUP_1, 0x10);
	pub const MONOFILTER_SIZE: u32 = opcode_key(OPCODE_GROUP_1, 0x11);
	pub const PUSH_CONSTANT_ALLOC_VS: u32 = opcode_key(OPCODE_GROUP_1, 0x12);
	pub const PUSH_CONSTANT_ALLOC_HS: u32 = opcode_key(OPCODE_GROUP_1, 0x13);
	pub const PUSH_CONSTANT_ALLOC_DS: u32 = opcode_key(OPCODE_GROUP_1, 0x14);
	pub const PUSH_CONSTANT_ALLOC_GS: u32 = opcode_key(OPCODE_GROUP_1, 0x15);
	pub const PUSH_CONSTANT_ALLOC_PS: u32 = opcode_key(OPCODE_GROUP_1, 0x16);
	pub const SO_DECL_LIST: u32 = opcode_key(OPCODE_GROUP_1, 0x17);
	pub const SO_BUFFER: u32 = opcode_key(OPCODE_GROUP_1, 0x18);
	pub const BINDING_TABLE_POOL_ALLOC: u32 = opcode_key(OPCODE_GROUP_1, 0x19);
	pub const GATHER_POOL_ALLOC: u32 = opcode_key(OPCODE_GROUP_1, 0x1A);
	pub const DX9_CONSTANT_BUFFER_POOL_ALLOC: u32 = opcode_key(OPCODE_GROUP_1, 0x1B);
	pub const SAMPLE_PATTERN: u32 = opcode_key(OPCODE_GROUP_1, 0x1C);
	pub const URB_CLEAR: u32 = opcode_key(OPCODE_GROUP_1, 0x1D);
	pub const MODE_3D: u32 = opcode_key(OPCODE_GROUP_1, 0x1E);
}