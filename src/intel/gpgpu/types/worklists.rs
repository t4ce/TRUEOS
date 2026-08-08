#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct FillRectWorklistRgba8Desc {
    pub(crate) dst_xy: u32,
    pub(crate) size: u32,
    pub(crate) color_rgba: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct FillRectWorklistRgba8Params {
    pub(crate) dst_gpu: u64,
    pub(crate) desc_gpu: u64,
    pub(crate) dst_pitch_bytes: u32,
    pub(crate) desc_base: u32,
    pub(crate) desc_count: u32,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuSolidRect {
    pub(crate) rect: GpgpuRect,
    pub(crate) color_rgba: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuAlphaBlendWorklistDesc {
    pub(crate) src_xy: u32,
    pub(crate) dst_xy: u32,
    pub(crate) size: u32,
    pub(crate) flags: u32,
    pub(crate) color_rgba: u32,
}

const _: () = assert!(core::mem::size_of::<GpgpuAlphaBlendWorklistDesc>() == 5 * 4);

pub(crate) const ALPHA_BLEND_WORKLIST_FLAG_COPY: u32 = 1 << 0;
pub(crate) const ALPHA_BLEND_WORKLIST_FLAG_SRC_OVER: u32 = 1 << 1;
pub(crate) const ALPHA_BLEND_WORKLIST_FLAG_TINT_RGB: u32 = 1 << 2;
pub(crate) const ALPHA_BLEND_WORKLIST_FLAG_TINT_ALPHA: u32 = 1 << 3;
pub(crate) const ALPHA_BLEND_WORKLIST_FLAG_PREMUL_SRC: u32 = 1 << 4;

pub(crate) const fn alpha_blend_worklist_max_descs() -> usize {
    ALPHA_BLEND_WORKLIST_MAX_DESCS
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct AlphaBlendWorklistRgba8Params {
    pub(crate) src_gpu: u64,
    pub(crate) dst_gpu: u64,
    pub(crate) desc_gpu: u64,
    pub(crate) src_pitch_bytes: u32,
    pub(crate) dst_pitch_bytes: u32,
    pub(crate) desc_base: u32,
    pub(crate) desc_count: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuSpriteQuadWorklistDesc {
    pub(crate) c0_x: f32,
    pub(crate) c0_y: f32,
    pub(crate) c0_u: f32,
    pub(crate) c0_v: f32,
    pub(crate) c1_x: f32,
    pub(crate) c1_y: f32,
    pub(crate) c1_u: f32,
    pub(crate) c1_v: f32,
    pub(crate) c2_x: f32,
    pub(crate) c2_y: f32,
    pub(crate) c2_u: f32,
    pub(crate) c2_v: f32,
    pub(crate) c3_x: f32,
    pub(crate) c3_y: f32,
    pub(crate) c3_u: f32,
    pub(crate) c3_v: f32,
    pub(crate) color_rgba: u32,
    pub(crate) flags: u32,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct GpgpuSpriteQuadWorklistRun<'a> {
    pub(crate) src: GpgpuRgba8Surface,
    pub(crate) descs: &'a [GpgpuSpriteQuadWorklistDesc],
}

/// Result of one Font-owned ordered RGBA sprite worklist.
///
/// `release` is present only when the final cache-draining marker and the
/// Font HW context-save boundary both retired for the exact destination.
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuSpriteQuadWorklistResult {
    pub(crate) stats: GpgpuWorklistSubmitStats,
    pub(crate) outcome: GpgpuSubmissionOutcome,
    pub(crate) release: Option<GpgpuRgba8ReleaseFence>,
}

pub(crate) const SPRITE_QUAD_WORKLIST_FLAG_SRC_OVER: u32 = 1 << 0;
pub(crate) const SPRITE_QUAD_WORKLIST_FLAG_PREMUL_SRC: u32 = 1 << 1;
pub(crate) const SPRITE_QUAD_WORKLIST_FLAG_CLEAR: u32 = 1 << 2;
pub(crate) const SPRITE_QUAD_WORKLIST_FLAG_SOURCE_XRGB: u32 = 1 << 3;
pub(crate) const SPRITE_QUAD_WORKLIST_FLAG_DEST_XRGB: u32 = 1 << 4;
pub(crate) const SPRITE_QUAD_WORKLIST_FLAG_TILEMAP: u32 = 1 << 5;

pub(crate) const SPRITE_QUAD_TILEMAP_MAGIC: u32 = 0x454C_4954;
pub(crate) const SPRITE_QUAD_TILEMAP_VERSION: u32 = 1;
pub(crate) const SPRITE_QUAD_TILEMAP_STATE_MAX_DWORDS: usize = 2_048;

pub(crate) const fn sprite_quad_worklist_max_descs() -> usize {
    SPRITE_QUAD_WORKLIST_MAX_DESCS
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct SpriteQuadWorklistRgba8Params {
    pub(crate) src_gpu: u64,
    pub(crate) dst_gpu: u64,
    pub(crate) desc_gpu: u64,
    pub(crate) src_pitch_bytes: u32,
    pub(crate) dst_pitch_bytes: u32,
    pub(crate) src_width: u32,
    pub(crate) src_height: u32,
    pub(crate) dst_width: u32,
    pub(crate) dst_height: u32,
    pub(crate) desc_base: u32,
    pub(crate) desc_count: u32,
}
