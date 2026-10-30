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
/// Source-free overwrite: `color_rgba` is the complete output pixel.
pub(crate) const ALPHA_BLEND_WORKLIST_FLAG_SOLID: u32 = 1 << 5;

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

