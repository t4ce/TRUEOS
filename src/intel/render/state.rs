#[derive(Copy, Clone, Debug, Default)]
struct TriangleStageStats {
    ia_vertices: u64,
    ia_primitives: u64,
    vs_invocations: u64,
    hs_invocations: u64,
    ds_invocations: u64,
    gs_invocations: u64,
    gs_primitives: u64,
    cl_invocations: u64,
    cl_primitives: u64,
    ps_invocations: u64,
    cps_invocations: u64,
    ps_depth: u64,
    so_prims_written_0: u64,
    so_write_offset_0: u64,
}

impl TriangleStageStats {
    fn delta_since(self, before: Self) -> Self {
        Self {
            ia_vertices: self.ia_vertices.saturating_sub(before.ia_vertices),
            ia_primitives: self.ia_primitives.saturating_sub(before.ia_primitives),
            vs_invocations: self.vs_invocations.saturating_sub(before.vs_invocations),
            hs_invocations: self.hs_invocations.saturating_sub(before.hs_invocations),
            ds_invocations: self.ds_invocations.saturating_sub(before.ds_invocations),
            gs_invocations: self.gs_invocations.saturating_sub(before.gs_invocations),
            gs_primitives: self.gs_primitives.saturating_sub(before.gs_primitives),
            cl_invocations: self.cl_invocations.saturating_sub(before.cl_invocations),
            cl_primitives: self.cl_primitives.saturating_sub(before.cl_primitives),
            ps_invocations: self.ps_invocations.saturating_sub(before.ps_invocations),
            cps_invocations: self.cps_invocations.saturating_sub(before.cps_invocations),
            ps_depth: self.ps_depth.saturating_sub(before.ps_depth),
            so_prims_written_0: self
                .so_prims_written_0
                .saturating_sub(before.so_prims_written_0),
            so_write_offset_0: self
                .so_write_offset_0
                .saturating_sub(before.so_write_offset_0),
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct RenderWarmState {
    pub device_id: u16,
    pub revision_id: u8,
    pub mmio_base: usize,
    pub mmio_len: usize,
    pub ring_phys: u64,
    pub ring_virt: *mut u8,
    pub ring_len: usize,
    pub context_phys: u64,
    pub context_virt: *mut u8,
    pub context_len: usize,
    pub batch_phys: u64,
    pub batch_virt: *mut u8,
    pub batch_len: usize,
    pub draw_state_phys: u64,
    pub draw_state_virt: *mut u8,
    pub draw_state_len: usize,
    pub vertex_phys: u64,
    pub vertex_virt: *mut u8,
    pub vertex_len: usize,
    pub result_phys: u64,
    pub result_virt: *mut u8,
    pub result_len: usize,
    pub streamout_phys: u64,
    pub streamout_virt: *mut u8,
    pub streamout_len: usize,
    pub gpgpu_arena_phys: u64,
    pub gpgpu_arena_virt: *mut u8,
    pub gpgpu_arena_len: usize,
}

#[derive(Copy, Clone)]
struct TriangleDrawPrep {
    vertex_count: u32,
    vertex_stride: u32,
    vertex_gpu_addr: u64,
    state_gpu_addr: u64,
    rt_gpu_addr: u64,
    rt_pitch: u32,
    target_w: u32,
    target_h: u32,
}

#[derive(Copy, Clone)]
struct TriangleVertexUploadProof {
    vertex_count: u32,
    vertex_stride: u32,
    byte_len: usize,
    gpu_addr: u64,
    signed_area_2x: f32,
    cpu_readback_ok: bool,
}

#[derive(Copy, Clone)]
struct TriangleShaderStageLayout {
    code_offset_bytes: u32,
    code_gpu_addr: u64,
    ksp_offset_bytes: u32,
    ksp_gpu_addr: u64,
    code_size_bytes: u32,
}

#[derive(Copy, Clone)]
struct TriangleShaderLayout {
    vs: TriangleShaderStageLayout,
    ps: TriangleShaderStageLayout,
    state_region_gpu_addr: u64,
    state_region_offset_bytes: u32,
    used_bytes: u32,
}

#[derive(Copy, Clone)]
struct TriangleProbeStateLayout {
    binding_table_offset_bytes: u32,
    surface_state_offset_bytes: u32,
    sampler_state_offset_bytes: u32,
    blend_state_offset_bytes: u32,
    color_calc_state_offset_bytes: u32,
    cc_viewport_offset_bytes: u32,
    sf_clip_viewport_offset_bytes: u32,
    scissor_rect_offset_bytes: u32,
    slice_hash_table_offset_bytes: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct TriangleFrontEndContract {
    label: &'static str,
    vs_urb_output_length_override: Option<u8>,
    sbe_read_offset: u8,
    sbe_read_length: u8,
    force_sbe_read_offset: bool,
    force_sbe_read_length: bool,
}

#[derive(Copy, Clone)]
struct VsStreamoutProofConfig {
    pipeline: &'static crate::intel::shader::TrianglePipeline,
    shader_layout: TriangleShaderLayout,
}

#[derive(Copy, Clone)]
enum TriangleBlendProbeMode {
    ExplicitRt0,
    MesaZeroedState,
    MesaZeroedNoBlendPointer,
}

impl TriangleBlendProbeMode {
    fn for_attempt(attempt: usize) -> Self {
        match attempt {
            1 => Self::MesaZeroedState,
            2 => Self::ExplicitRt0,
            3 => Self::MesaZeroedNoBlendPointer,
            _ => Self::MesaZeroedState,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::ExplicitRt0 => "explicit-rt0",
            Self::MesaZeroedState => "mesa-zeroed",
            Self::MesaZeroedNoBlendPointer => "mesa-zeroed-no-blend-ptr",
        }
    }

    fn blend_state_pointer_dword(self, offset_bytes: u32) -> u32 {
        match self {
            Self::MesaZeroedNoBlendPointer => 0,
            _ => offset_bytes | 1,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum TriangleBatchMode {
    Draw,
    VfDraw,
    StreamoutProof,
    VfStreamoutProof,
    VsStreamoutProof,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum BackendProbeMode {
    MesaLike,
    PsBindingTableCountZero,
    PsBindingTableCountOne,
    WmNormalDispatch,
    PsDispatchSlot0,
    PsDispatchSlot1,
    PsDispatchSlot2,
    PsDispatchAllKspSlots,
    PsPayloadPushConstant,
    PsPayloadAttributeEnable,
    PsPayloadSimpleHint,
    PsPayloadSourceDepthW,
    PsPayloadBaryPlanes,
    PsGrfStartR1,
    PsGrfStartR2,
    PsGrfStartR4,
    PsGrfMaxThreads31,
    PsGrfMaxThreads15,
    RasterWmInputOa,
    RasterWmInputOaNdcBlock32,
    RasterWmInputOaNdcMesaActiveBlock,
    RasterWmInputOaNdcMesaActiveBlockNoPrimRepl,
    RasterWmInputOaNdcPerPoly,
    RasterWmInputOaNdcWalk16,
    RasterWmInputOaNdcClipPreconditions,
    RasterWmInputOaNdcNoWmScissor,
    RasterWmInputOaScreenSpace,
    RasterWmInputOaScreenSpaceNoWmHzOpPacket,
    RasterWmInputOaScreenSpaceForceThreadDispatch,
    RasterWmInputOaScreenSpaceSfSaneDefaults,
    RasterWmInputOaScreenSpaceClipBypass,
    RasterWmInputOaScreenSpaceClipBypassRasterPreconditions,
    RasterWmInputOaScreenSpaceClipBypassRasterPreconditionsOpenBounds,
    RasterWmInputOaScreenSpaceClipBypassRasterPreconditionsOpenBoundsSbe1NoSwiz,
    RasterWmInputOaScreenSpaceAcceptAllOpenBoundsSbe1NoSwiz,
    RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderSbe1NoSwiz,
    RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderPreClipSbePsSbe1NoSwiz,
    RasterWmInputOaScreenSpaceSlot0PreClipSbePsNoSwiz,
    RasterWmInputOaScreenSpaceSlot0TightPreClipRasterSbePsNoSwiz,
    RasterWmInputOaScreenSpaceSlot0XyzwTightPreClipRasterSbePsNoSwiz,
    RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderPerPolySbe1NoSwiz,
    RasterWmInputOaScreenSpaceRectListClipBypass,
    RasterWmInputOaScreenSpaceRectListClipBypassSfSane,
    RasterWmInputOaScreenSpaceRectListBlorpLike,
    RasterWmInputOaScreenSpaceRectListSlot0PerPoly,
    RasterWmInputOaScreenSpaceRectListSlot0XyzwTightPreClipPerPoly,
    RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1TightPreClipPerPoly,
    RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrder,
    RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOn,
    RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOnEarlyBackend,
    RasterWmInputOaScreenSpaceClipPreconditions,
    RasterWmInputOaScreenSpaceRasterClipPreconditions,
    RasterWmInputOaScreenSpaceRasterClipPreconditionsHardFence,
    RasterWmInputOaScreenSpaceD3dRasterPreconditionsNoHz,
    RasterWmInputOaScreenSpaceD3dPerPolyNoHz,
    RasterWmInputOaScreenSpacePerPoly,
    RasterWmInputOaScreenSpaceUrb128PerPoly,
    RasterWmInputOaScreenSpaceMesaSimpleOrder,
    RasterWmInputOaScreenSpaceMesaSimpleNoSwizNoScissor,
    RasterWmInputOaScreenSpacePointList,
    RasterWmInputOaScreenSpacePointListOpenBounds,
    RasterWmInputOaScreenSpaceRtIndependent,
    RasterWmInputOaNdcForceOnPattern,
    RasterWmInputOaForceOnPattern,
    RasterWmInputOaForceOffPixel,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum PostDrawSyncVariant {
    HeavyAll,
    LightOnlyRetire,
    LightPostSyncNoCs,
    LightCsNoPostSync,
    FlushBit5Dc,
    FlushBit7,
    FlushBit12Rt,
    FlushBit20Cs,
    FlushBit26Hdc,
}

const POST_DRAW_PC_RETIRE_SPECTRUM: [PostDrawSyncVariant; 3] = [
    PostDrawSyncVariant::LightOnlyRetire,
    PostDrawSyncVariant::LightCsNoPostSync,
    PostDrawSyncVariant::LightPostSyncNoCs,
];

impl PostDrawSyncVariant {
    fn label(self) -> &'static str {
        match self {
            Self::HeavyAll => "heavy-all",
            Self::LightOnlyRetire => "light-only-retire",
            Self::LightPostSyncNoCs => "pc-postsync-no-cs",
            Self::LightCsNoPostSync => "pc-cs-no-postsync",
            Self::FlushBit5Dc => "bit5-dc-flush",
            Self::FlushBit7 => "bit7-flush-enable",
            Self::FlushBit12Rt => "bit12-rt-flush",
            Self::FlushBit20Cs => "bit20-cs-stall",
            Self::FlushBit26Hdc => "bit26-hdc-flush",
        }
    }

    fn submit_name(self) -> &'static str {
        match self {
            Self::HeavyAll => "postdraw-heavy-all",
            Self::LightOnlyRetire => "postdraw-light-only-retire",
            Self::LightPostSyncNoCs => "postdraw-pc-postsync-no-cs",
            Self::LightCsNoPostSync => "postdraw-pc-cs-no-postsync",
            Self::FlushBit5Dc => "postdraw-flush-bit5",
            Self::FlushBit7 => "postdraw-flush-bit7",
            Self::FlushBit12Rt => "postdraw-flush-bit12",
            Self::FlushBit20Cs => "postdraw-flush-bit20",
            Self::FlushBit26Hdc => "postdraw-flush-bit26",
        }
    }

    fn from_submit_name(submit_name: &str) -> Option<Self> {
        match submit_name {
            "postdraw-light-only-retire" => Some(Self::LightOnlyRetire),
            "postdraw-pc-postsync-no-cs" => Some(Self::LightPostSyncNoCs),
            "postdraw-pc-cs-no-postsync" => Some(Self::LightCsNoPostSync),
            "postdraw-flush-bit5" => Some(Self::FlushBit5Dc),
            "postdraw-flush-bit7" => Some(Self::FlushBit7),
            "postdraw-flush-bit12" => Some(Self::FlushBit12Rt),
            "postdraw-flush-bit20" => Some(Self::FlushBit20Cs),
            "postdraw-flush-bit26" => Some(Self::FlushBit26Hdc),
            _ => None,
        }
    }

    fn light_sync_flags(self) -> u32 {
        match self {
            Self::LightPostSyncNoCs => PIPE_CONTROL_POST_DRAW_LIGHT_POSTSYNC_NO_STALL_BITS,
            Self::LightCsNoPostSync => PIPE_CONTROL_POST_DRAW_LIGHT_CS_STALL_ONLY_BITS,
            _ => PIPE_CONTROL_POST_DRAW_LIGHT_SYNC_BITS,
        }
    }

    fn light_post_sync_enabled(self) -> bool {
        (self.light_sync_flags() & PIPE_CONTROL_POST_SYNC_WRITE_IMMEDIATE) != 0
    }

    fn light_cs_stall_enabled(self) -> bool {
        (self.light_sync_flags() & PIPE_CONTROL_CS_STALL) != 0
    }

    fn heavy_sync_flags(self) -> Option<u32> {
        let flags = match self {
            Self::HeavyAll => PIPE_CONTROL_POST_DRAW_SYNC_BITS,
            Self::LightOnlyRetire => return None,
            Self::LightPostSyncNoCs => return None,
            Self::LightCsNoPostSync => return None,
            Self::FlushBit5Dc => {
                PIPE_CONTROL_POST_DRAW_LIGHT_SYNC_BITS | PIPE_CONTROL_DC_FLUSH_ENABLE
            }
            Self::FlushBit7 => PIPE_CONTROL_POST_DRAW_LIGHT_SYNC_BITS | PIPE_CONTROL_FLUSH_ENABLE,
            Self::FlushBit12Rt => {
                PIPE_CONTROL_POST_DRAW_LIGHT_SYNC_BITS | PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH
            }
            Self::FlushBit20Cs => PIPE_CONTROL_POST_DRAW_LIGHT_SYNC_BITS,
            Self::FlushBit26Hdc => PIPE_CONTROL_POST_DRAW_LIGHT_SYNC_BITS | PIPE_CONTROL_FLUSH_HDC,
        };
        Some(flags)
    }
}

impl BackendProbeMode {
    fn label(self) -> &'static str {
        match self {
            Self::MesaLike => "mesa-like",
            Self::PsBindingTableCountZero => "ps-bt-count-0",
            Self::PsBindingTableCountOne => "ps-bt-count-1",
            Self::WmNormalDispatch => "wm-normal-dispatch",
            Self::PsDispatchSlot0 => "ps-dispatch-slot0",
            Self::PsDispatchSlot1 => "ps-dispatch-slot1",
            Self::PsDispatchSlot2 => "ps-dispatch-slot2",
            Self::PsDispatchAllKspSlots => "ps-dispatch-all-ksp-slots",
            Self::PsPayloadPushConstant => "ps-payload-push-constant",
            Self::PsPayloadAttributeEnable => "ps-payload-attribute-enable",
            Self::PsPayloadSimpleHint => "ps-payload-simple-hint",
            Self::PsPayloadSourceDepthW => "ps-payload-source-depth-w",
            Self::PsPayloadBaryPlanes => "ps-payload-bary-planes",
            Self::PsGrfStartR1 => "ps-grf-start-r1",
            Self::PsGrfStartR2 => "ps-grf-start-r2",
            Self::PsGrfStartR4 => "ps-grf-start-r4",
            Self::PsGrfMaxThreads31 => "ps-grf-maxthreads-31",
            Self::PsGrfMaxThreads15 => "ps-grf-maxthreads-15",
            Self::RasterWmInputOa => "raster-wm-input-oa",
            Self::RasterWmInputOaNdcBlock32 => "raster-wm-input-oa-ndc-block32",
            Self::RasterWmInputOaNdcMesaActiveBlock => "raster-wm-input-oa-ndc-mesa-active-block",
            Self::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl => {
                "raster-wm-input-oa-ndc-mesa-active-block-no-prim-repl"
            }
            Self::RasterWmInputOaNdcPerPoly => "raster-wm-input-oa-ndc-per-poly",
            Self::RasterWmInputOaNdcWalk16 => "raster-wm-input-oa-ndc-walk16",
            Self::RasterWmInputOaNdcClipPreconditions => {
                "raster-wm-input-oa-ndc-clip-preconditions"
            }
            Self::RasterWmInputOaNdcNoWmScissor => "raster-wm-input-oa-ndc-no-wm-scissor",
            Self::RasterWmInputOaScreenSpace => "raster-wm-input-oa-screen-space",
            Self::RasterWmInputOaScreenSpaceNoWmHzOpPacket => {
                "raster-wm-input-oa-screen-space-no-wm-hz-op-packet"
            }
            Self::RasterWmInputOaScreenSpaceForceThreadDispatch => {
                "raster-wm-input-oa-screen-space-force-thread-dispatch"
            }
            Self::RasterWmInputOaScreenSpaceSfSaneDefaults => {
                "raster-wm-input-oa-screen-space-sf-sane-defaults"
            }
            Self::RasterWmInputOaScreenSpaceClipBypass => {
                "raster-wm-input-oa-screen-space-clip-bypass"
            }
            Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditions => {
                "raster-wm-input-oa-screen-space-clip-bypass-raster-preconditions"
            }
            Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditionsOpenBounds => {
                "raster-wm-input-oa-screen-space-clip-bypass-raster-preconditions-open-bounds"
            }
            Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditionsOpenBoundsSbe1NoSwiz => {
                "raster-wm-input-oa-screen-space-clip-bypass-raster-preconditions-open-bounds-sbe1-noswiz"
            }
            Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsSbe1NoSwiz => {
                "raster-wm-input-oa-screen-space-accept-all-open-bounds-sbe1-noswiz"
            }
            Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderSbe1NoSwiz => {
                "raster-wm-input-oa-screen-space-accept-all-open-bounds-header-sbe1-noswiz"
            }
            Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderPreClipSbePsSbe1NoSwiz => {
                "raster-wm-input-oa-screen-space-accept-all-open-bounds-header-preclip-sbe-ps-sbe1-noswiz"
            }
            Self::RasterWmInputOaScreenSpaceSlot0PreClipSbePsNoSwiz => {
                "raster-wm-input-oa-screen-space-slot0-preclip-sbe-ps-noswiz"
            }
            Self::RasterWmInputOaScreenSpaceSlot0TightPreClipRasterSbePsNoSwiz => {
                "raster-wm-input-oa-screen-space-slot0-tight-preclip-raster-sbe-ps-noswiz"
            }
            Self::RasterWmInputOaScreenSpaceSlot0XyzwTightPreClipRasterSbePsNoSwiz => {
                "raster-wm-input-oa-screen-space-slot0-xyzw-tight-preclip-raster-sbe-ps-noswiz"
            }
            Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderPerPolySbe1NoSwiz => {
                "raster-wm-input-oa-screen-space-accept-all-open-bounds-header-perpoly-sbe1-noswiz"
            }
            Self::RasterWmInputOaScreenSpaceRectListClipBypass => {
                "raster-wm-input-oa-screen-space-rectlist-clip-bypass"
            }
            Self::RasterWmInputOaScreenSpaceRectListClipBypassSfSane => {
                "raster-wm-input-oa-screen-space-rectlist-clip-bypass-sf-sane"
            }
            Self::RasterWmInputOaScreenSpaceRectListBlorpLike => {
                "raster-wm-input-oa-screen-space-rectlist-blorp-like"
            }
            Self::RasterWmInputOaScreenSpaceRectListSlot0PerPoly => {
                "raster-wm-input-oa-screen-space-rectlist-slot0-per-poly"
            }
            Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwTightPreClipPerPoly => {
                "raster-wm-input-oa-screen-space-rectlist-slot0-xyzw-tight-preclip-per-poly"
            }
            Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1TightPreClipPerPoly => {
                "raster-wm-input-oa-screen-space-rectlist-slot0-xyzw-sbe1-tight-preclip-per-poly"
            }
            Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrder => {
                "raster-wm-input-oa-screen-space-rectlist-slot0-xyzw-sbe1-mesa-order"
            }
            Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOn => {
                "raster-wm-input-oa-screen-space-rectlist-slot0-xyzw-sbe1-mesa-order-clip-on"
            }
            Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOnEarlyBackend => {
                "raster-wm-input-oa-screen-space-rectlist-slot0-xyzw-sbe1-mesa-order-clip-on-early-backend"
            }
            Self::RasterWmInputOaScreenSpaceClipPreconditions => {
                "raster-wm-input-oa-screen-space-clip-preconditions"
            }
            Self::RasterWmInputOaScreenSpaceRasterClipPreconditions => {
                "raster-wm-input-oa-screen-space-raster-clip-preconditions"
            }
            Self::RasterWmInputOaScreenSpaceRasterClipPreconditionsHardFence => {
                "raster-wm-input-oa-screen-space-raster-clip-preconditions-hard-fence"
            }
            Self::RasterWmInputOaScreenSpaceD3dRasterPreconditionsNoHz => {
                "raster-wm-input-oa-screen-space-d3d-raster-no-hz"
            }
            Self::RasterWmInputOaScreenSpaceD3dPerPolyNoHz => {
                "raster-wm-input-oa-screen-space-d3d-per-poly-no-hz"
            }
            Self::RasterWmInputOaScreenSpacePerPoly => "raster-wm-input-oa-screen-space-per-poly",
            Self::RasterWmInputOaScreenSpaceUrb128PerPoly => {
                "raster-wm-input-oa-screen-space-urb128-per-poly"
            }
            Self::RasterWmInputOaScreenSpaceMesaSimpleOrder => {
                "raster-wm-input-oa-screen-space-mesa-simple-order"
            }
            Self::RasterWmInputOaScreenSpaceMesaSimpleNoSwizNoScissor => {
                "raster-wm-input-oa-screen-space-mesa-simple-noswiz-sbe1-tight-scissor-walk16-dx101-sf-dw2-default-clip-accept"
            }
            Self::RasterWmInputOaScreenSpacePointList => {
                "raster-wm-input-oa-screen-space-pointlist-tight-scissor-walk16-sf-dw2-default"
            }
            Self::RasterWmInputOaScreenSpacePointListOpenBounds => {
                "raster-wm-input-oa-screen-space-pointlist-open-bounds-walk16-sf-dw2-default"
            }
            Self::RasterWmInputOaScreenSpaceRtIndependent => {
                "raster-wm-input-oa-screen-space-rt-independent"
            }
            Self::RasterWmInputOaNdcForceOnPattern => "raster-wm-input-oa-ndc-force-on-pattern",
            Self::RasterWmInputOaForceOnPattern => "raster-wm-input-oa-force-on-pattern",
            Self::RasterWmInputOaForceOffPixel => "raster-wm-input-oa-force-off-pixel",
        }
    }

    fn ps_dispatch_slot(self) -> Option<u8> {
        match self {
            Self::PsDispatchSlot0 => Some(0),
            Self::PsDispatchSlot1 => Some(1),
            Self::PsDispatchSlot2 => Some(2),
            _ => None,
        }
    }

    fn is_payload_spectrum(self) -> bool {
        matches!(
            self,
            Self::PsPayloadPushConstant
                | Self::PsPayloadAttributeEnable
                | Self::PsPayloadSimpleHint
                | Self::PsPayloadSourceDepthW
                | Self::PsPayloadBaryPlanes
        )
    }

    fn ps_grf_start_override(self) -> Option<u8> {
        match self {
            Self::PsGrfStartR1 => Some(1),
            Self::PsGrfStartR2 => Some(2),
            Self::PsGrfStartR4 => Some(4),
            _ => None,
        }
    }

    fn ps_max_threads_override(self) -> Option<u32> {
        match self {
            Self::PsGrfMaxThreads31 => Some(31),
            Self::PsGrfMaxThreads15 => Some(15),
            _ => None,
        }
    }

    fn uses_raster_wm_oa(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOa
                | Self::RasterWmInputOaNdcBlock32
                | Self::RasterWmInputOaNdcMesaActiveBlock
                | Self::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl
                | Self::RasterWmInputOaNdcPerPoly
                | Self::RasterWmInputOaNdcWalk16
                | Self::RasterWmInputOaNdcClipPreconditions
                | Self::RasterWmInputOaNdcNoWmScissor
                | Self::RasterWmInputOaScreenSpace
                | Self::RasterWmInputOaScreenSpaceNoWmHzOpPacket
                | Self::RasterWmInputOaScreenSpaceForceThreadDispatch
                | Self::RasterWmInputOaScreenSpaceSfSaneDefaults
                | Self::RasterWmInputOaScreenSpaceClipBypass
                | Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditions
                | Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditionsOpenBounds
                | Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditionsOpenBoundsSbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsSbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderSbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderPreClipSbePsSbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceSlot0PreClipSbePsNoSwiz
                | Self::RasterWmInputOaScreenSpaceSlot0TightPreClipRasterSbePsNoSwiz
                | Self::RasterWmInputOaScreenSpaceSlot0XyzwTightPreClipRasterSbePsNoSwiz
                | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderPerPolySbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceRectListClipBypass
                | Self::RasterWmInputOaScreenSpaceRectListClipBypassSfSane
                | Self::RasterWmInputOaScreenSpaceRectListBlorpLike
                | Self::RasterWmInputOaScreenSpaceRectListSlot0PerPoly
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwTightPreClipPerPoly
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1TightPreClipPerPoly
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrder
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOn
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOnEarlyBackend
                | Self::RasterWmInputOaScreenSpaceClipPreconditions
                | Self::RasterWmInputOaScreenSpaceRasterClipPreconditions
                | Self::RasterWmInputOaScreenSpaceRasterClipPreconditionsHardFence
                | Self::RasterWmInputOaScreenSpaceD3dRasterPreconditionsNoHz
                | Self::RasterWmInputOaScreenSpaceD3dPerPolyNoHz
                | Self::RasterWmInputOaScreenSpacePerPoly
                | Self::RasterWmInputOaScreenSpaceUrb128PerPoly
                | Self::RasterWmInputOaScreenSpaceMesaSimpleOrder
                | Self::RasterWmInputOaScreenSpaceMesaSimpleNoSwizNoScissor
                | Self::RasterWmInputOaScreenSpacePointList
                | Self::RasterWmInputOaScreenSpacePointListOpenBounds
                | Self::RasterWmInputOaScreenSpaceRtIndependent
                | Self::RasterWmInputOaNdcForceOnPattern
                | Self::RasterWmInputOaForceOnPattern
                | Self::RasterWmInputOaForceOffPixel
        )
    }

    fn defer_raster_wm_oa_end_after_fence(self) -> bool {
        matches!(self, Self::RasterWmInputOaScreenSpaceRasterClipPreconditionsHardFence)
    }

    fn forced_ms_raster_mode(self) -> Option<u32> {
        match self {
            Self::RasterWmInputOaNdcForceOnPattern | Self::RasterWmInputOaForceOnPattern => Some(3),
            Self::RasterWmInputOaForceOffPixel => Some(0),
            _ => None,
        }
    }

    fn forced_raster_sample_count(self) -> Option<u32> {
        match self {
            Self::RasterWmInputOaNdcForceOnPattern
            | Self::RasterWmInputOaForceOnPattern
            | Self::RasterWmInputOaForceOffPixel => Some(2),
            Self::RasterWmInputOaScreenSpaceRtIndependent => Some(1),
            _ => None,
        }
    }

    fn force_wm_thread_dispatch(self) -> bool {
        matches!(self, Self::RasterWmInputOaScreenSpaceForceThreadDispatch)
    }

    fn skip_wm_hz_op_packet(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaNdcMesaActiveBlock
                | Self::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl
                | Self::RasterWmInputOaScreenSpaceNoWmHzOpPacket
                | Self::RasterWmInputOaScreenSpaceMesaSimpleOrder
                | Self::RasterWmInputOaScreenSpaceMesaSimpleNoSwizNoScissor
        )
    }

    fn mesa_simple_order(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaNdcMesaActiveBlock
                | Self::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl
                | Self::RasterWmInputOaScreenSpaceMesaSimpleOrder
                | Self::RasterWmInputOaScreenSpaceMesaSimpleNoSwizNoScissor
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrder
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOn
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOnEarlyBackend
                | Self::RasterWmInputOaScreenSpacePointList
                | Self::RasterWmInputOaScreenSpacePointListOpenBounds
        )
    }

    fn mesa_order_with_early_backend(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOnEarlyBackend
        )
    }

    fn primitive_replication_before_sf(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOnEarlyBackend
        )
    }

    fn mesa_active_block_state(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaNdcMesaActiveBlock
                | Self::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl
        )
    }

    fn mesa_active_block_disable_primitive_replication(self) -> bool {
        matches!(self, Self::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl)
    }

    fn mesa_simple_clip_defaults(self) -> bool {
        matches!(self, Self::RasterWmInputOaScreenSpaceMesaSimpleOrder)
    }

    fn skip_sbe_swiz_packet(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaNdcMesaActiveBlock
                | Self::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl
                | Self::RasterWmInputOaScreenSpaceMesaSimpleNoSwizNoScissor
                | Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditionsOpenBoundsSbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsSbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderSbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderPreClipSbePsSbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceSlot0PreClipSbePsNoSwiz
                | Self::RasterWmInputOaScreenSpaceSlot0TightPreClipRasterSbePsNoSwiz
                | Self::RasterWmInputOaScreenSpaceSlot0XyzwTightPreClipRasterSbePsNoSwiz
                | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderPerPolySbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrder
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOn
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOnEarlyBackend
                | Self::RasterWmInputOaScreenSpacePointList
                | Self::RasterWmInputOaScreenSpacePointListOpenBounds
        )
    }

    fn sbe_read_offset_override(self) -> Option<u8> {
        match self {
            Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditionsOpenBoundsSbe1NoSwiz => {
                Some(1)
            }
            Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsSbe1NoSwiz => Some(1),
            Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderSbe1NoSwiz => Some(1),
            Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderPreClipSbePsSbe1NoSwiz => {
                Some(1)
            }
            Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderPerPolySbe1NoSwiz => Some(1),
            Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1TightPreClipPerPoly => Some(1),
            Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrder => Some(1),
            Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOn => Some(1),
            Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOnEarlyBackend => {
                Some(1)
            }
            Self::RasterWmInputOaScreenSpacePointList
            | Self::RasterWmInputOaScreenSpacePointListOpenBounds => Some(0),
            _ => None,
        }
    }

    fn vf_streamout_experiment_override(self) -> Option<StreamoutProofExperiment> {
        match self {
            Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderSbe1NoSwiz
            | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderPreClipSbePsSbe1NoSwiz
            | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderPerPolySbe1NoSwiz => {
                Some(StreamoutProofExperiment::HeaderAndPositionSlots01)
            }
            Self::RasterWmInputOaScreenSpaceSlot0XyzwTightPreClipRasterSbePsNoSwiz => {
                Some(StreamoutProofExperiment::PositionSlot0Xyzw)
            }
            Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwTightPreClipPerPoly
            | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1TightPreClipPerPoly
            | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrder
            | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOn => {
                Some(StreamoutProofExperiment::PositionSlot0Xyzw)
            }
            Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOnEarlyBackend => {
                Some(StreamoutProofExperiment::PositionSlot0Xyzw)
            }
            Self::RasterWmInputOaScreenSpacePointList
            | Self::RasterWmInputOaScreenSpacePointListOpenBounds => {
                Some(StreamoutProofExperiment::PositionSlot0Xyzw)
            }
            _ => None,
        }
    }

    fn pre_clip_sbe_ps_state(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaNdcMesaActiveBlock
                | Self::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl
                | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderPreClipSbePsSbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceSlot0PreClipSbePsNoSwiz
                | Self::RasterWmInputOaScreenSpaceSlot0TightPreClipRasterSbePsNoSwiz
                | Self::RasterWmInputOaScreenSpaceSlot0XyzwTightPreClipRasterSbePsNoSwiz
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwTightPreClipPerPoly
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1TightPreClipPerPoly
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrder
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOn
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOnEarlyBackend
                | Self::RasterWmInputOaScreenSpacePointList
                | Self::RasterWmInputOaScreenSpacePointListOpenBounds
        )
    }

    fn pre_clip_raster_state(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderSbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceSlot0TightPreClipRasterSbePsNoSwiz
                | Self::RasterWmInputOaScreenSpaceSlot0XyzwTightPreClipRasterSbePsNoSwiz
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwTightPreClipPerPoly
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1TightPreClipPerPoly
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOnEarlyBackend
                | Self::RasterWmInputOaScreenSpacePointList
                | Self::RasterWmInputOaScreenSpacePointListOpenBounds
        )
    }

    fn disable_gfx125_tbimr_raster_wa(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaNdcMesaActiveBlock
                | Self::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl
                | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderPreClipSbePsSbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceSlot0PreClipSbePsNoSwiz
                | Self::RasterWmInputOaScreenSpaceSlot0TightPreClipRasterSbePsNoSwiz
                | Self::RasterWmInputOaScreenSpaceSlot0XyzwTightPreClipRasterSbePsNoSwiz
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwTightPreClipPerPoly
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1TightPreClipPerPoly
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOnEarlyBackend
                | Self::RasterWmInputOaScreenSpacePointList
                | Self::RasterWmInputOaScreenSpacePointListOpenBounds
        )
    }

    fn use_3dprimitive_extended(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaNdcMesaActiveBlock
                | Self::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl
                | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderPreClipSbePsSbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceSlot0PreClipSbePsNoSwiz
                | Self::RasterWmInputOaScreenSpaceSlot0TightPreClipRasterSbePsNoSwiz
                | Self::RasterWmInputOaScreenSpaceSlot0XyzwTightPreClipRasterSbePsNoSwiz
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwTightPreClipPerPoly
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1TightPreClipPerPoly
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOnEarlyBackend
                | Self::RasterWmInputOaScreenSpacePointList
                | Self::RasterWmInputOaScreenSpacePointListOpenBounds
        )
    }

    fn enable_sbe_attribute_swizzle(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditions
                | Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditionsOpenBounds
        )
    }

    fn primitive_topology_override(self) -> Option<u32> {
        match self {
            Self::RasterWmInputOaScreenSpaceRectListClipBypass
            | Self::RasterWmInputOaScreenSpaceRectListClipBypassSfSane
            | Self::RasterWmInputOaScreenSpaceRectListBlorpLike
            | Self::RasterWmInputOaScreenSpaceRectListSlot0PerPoly
            | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwTightPreClipPerPoly
            | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1TightPreClipPerPoly
            | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrder
            | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOn
            | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOnEarlyBackend => {
                Some(TRIANGLE_TOPOLOGY_RECTLIST)
            }
            Self::RasterWmInputOaScreenSpacePointList
            | Self::RasterWmInputOaScreenSpacePointListOpenBounds => {
                Some(TRIANGLE_TOPOLOGY_POINTLIST)
            }
            _ => None,
        }
    }

    fn enable_sf_sane_defaults(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaScreenSpaceSfSaneDefaults
                | Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditions
                | Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditionsOpenBounds
                | Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditionsOpenBoundsSbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsSbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderSbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderPreClipSbePsSbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceSlot0PreClipSbePsNoSwiz
                | Self::RasterWmInputOaScreenSpaceSlot0TightPreClipRasterSbePsNoSwiz
                | Self::RasterWmInputOaScreenSpaceSlot0XyzwTightPreClipRasterSbePsNoSwiz
                | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderPerPolySbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceRectListClipBypassSfSane
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwTightPreClipPerPoly
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1TightPreClipPerPoly
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrder
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOn
                | Self::RasterWmInputOaScreenSpaceMesaSimpleNoSwizNoScissor
                | Self::RasterWmInputOaScreenSpacePointList
                | Self::RasterWmInputOaScreenSpacePointListOpenBounds
        )
    }

    fn enable_wm_scissor(self) -> bool {
        !matches!(
            self,
            Self::RasterWmInputOaNdcNoWmScissor
                | Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditionsOpenBounds
                | Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditionsOpenBoundsSbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsSbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderSbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderPreClipSbePsSbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceSlot0PreClipSbePsNoSwiz
                | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderPerPolySbe1NoSwiz
                | Self::RasterWmInputOaScreenSpacePointListOpenBounds
        )
    }

    fn use_full_drawing_rectangle(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditionsOpenBounds
                | Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditionsOpenBoundsSbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsSbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderSbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderPreClipSbePsSbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceSlot0PreClipSbePsNoSwiz
                | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderPerPolySbe1NoSwiz
                | Self::RasterWmInputOaScreenSpacePointListOpenBounds
        )
    }

    fn raster_front_counter_clockwise(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaNdcClipPreconditions
                | Self::RasterWmInputOaScreenSpaceClipPreconditions
                | Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditions
                | Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditionsOpenBounds
                | Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditionsOpenBoundsSbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsSbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderSbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderPreClipSbePsSbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceSlot0PreClipSbePsNoSwiz
                | Self::RasterWmInputOaScreenSpaceSlot0TightPreClipRasterSbePsNoSwiz
                | Self::RasterWmInputOaScreenSpaceSlot0XyzwTightPreClipRasterSbePsNoSwiz
                | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderPerPolySbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceRasterClipPreconditions
                | Self::RasterWmInputOaScreenSpaceRasterClipPreconditionsHardFence
                | Self::RasterWmInputOaScreenSpaceD3dRasterPreconditionsNoHz
                | Self::RasterWmInputOaScreenSpaceD3dPerPolyNoHz
                | Self::RasterWmInputOaScreenSpaceMesaSimpleNoSwizNoScissor
                | Self::RasterWmInputOaScreenSpacePointList
                | Self::RasterWmInputOaScreenSpacePointListOpenBounds
                | Self::RasterWmInputOaScreenSpaceRtIndependent
        )
    }

    fn enable_wm_hz_op_scissor(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaNdcClipPreconditions
                | Self::RasterWmInputOaScreenSpaceClipPreconditions
                | Self::RasterWmInputOaScreenSpaceRasterClipPreconditions
                | Self::RasterWmInputOaScreenSpaceRasterClipPreconditionsHardFence
        )
    }

    fn wm_walk_granularity_override(self) -> Option<u32> {
        match self {
            Self::RasterWmInputOaNdcWalk16
            | Self::RasterWmInputOaNdcClipPreconditions
            | Self::RasterWmInputOaScreenSpaceClipPreconditions
            | Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditions
            | Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditionsOpenBounds
            | Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditionsOpenBoundsSbe1NoSwiz
            | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsSbe1NoSwiz
            | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderSbe1NoSwiz
            | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderPreClipSbePsSbe1NoSwiz
            | Self::RasterWmInputOaScreenSpaceSlot0PreClipSbePsNoSwiz
            | Self::RasterWmInputOaScreenSpaceSlot0TightPreClipRasterSbePsNoSwiz
            | Self::RasterWmInputOaScreenSpaceSlot0XyzwTightPreClipRasterSbePsNoSwiz
            | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwTightPreClipPerPoly
            | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1TightPreClipPerPoly
            | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrder
            | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOn
            | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOnEarlyBackend
            | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderPerPolySbe1NoSwiz
            | Self::RasterWmInputOaScreenSpaceRasterClipPreconditions
            | Self::RasterWmInputOaScreenSpaceRasterClipPreconditionsHardFence
            | Self::RasterWmInputOaScreenSpaceD3dRasterPreconditionsNoHz
            | Self::RasterWmInputOaScreenSpaceD3dPerPolyNoHz
            | Self::RasterWmInputOaScreenSpaceMesaSimpleNoSwizNoScissor
            | Self::RasterWmInputOaScreenSpacePointList
            | Self::RasterWmInputOaScreenSpacePointListOpenBounds
            | Self::RasterWmInputOaScreenSpaceRtIndependent => Some(0),
            _ => None,
        }
    }

    fn enable_clip_preconditions(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaNdcClipPreconditions
                | Self::RasterWmInputOaScreenSpaceClipPreconditions
                | Self::RasterWmInputOaScreenSpaceRasterClipPreconditions
                | Self::RasterWmInputOaScreenSpaceRasterClipPreconditionsHardFence
                | Self::RasterWmInputOaScreenSpaceD3dRasterPreconditionsNoHz
                | Self::RasterWmInputOaScreenSpaceD3dPerPolyNoHz
                | Self::RasterWmInputOaScreenSpaceRtIndependent
        )
    }

    fn force_clip_mode(self) -> bool {
        self.enable_clip_preconditions()
    }

    fn enable_raster_viewport_z_clip_tests(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaScreenSpaceRasterClipPreconditions
                | Self::RasterWmInputOaScreenSpaceRasterClipPreconditionsHardFence
                | Self::RasterWmInputOaScreenSpaceD3dRasterPreconditionsNoHz
                | Self::RasterWmInputOaScreenSpaceD3dPerPolyNoHz
        )
    }

    fn raster_api_mode_override(self) -> Option<u32> {
        match self {
            Self::RasterWmInputOaScreenSpaceRasterClipPreconditions
            | Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditions
            | Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditionsOpenBounds
            | Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditionsOpenBoundsSbe1NoSwiz
            | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsSbe1NoSwiz
            | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderSbe1NoSwiz
            | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderPreClipSbePsSbe1NoSwiz
            | Self::RasterWmInputOaScreenSpaceSlot0PreClipSbePsNoSwiz
            | Self::RasterWmInputOaScreenSpaceSlot0TightPreClipRasterSbePsNoSwiz
            | Self::RasterWmInputOaScreenSpaceSlot0XyzwTightPreClipRasterSbePsNoSwiz
            | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwTightPreClipPerPoly
            | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1TightPreClipPerPoly
            | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrder
            | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOn
            | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOnEarlyBackend
            | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderPerPolySbe1NoSwiz
            | Self::RasterWmInputOaScreenSpaceRasterClipPreconditionsHardFence
            | Self::RasterWmInputOaScreenSpaceD3dRasterPreconditionsNoHz
            | Self::RasterWmInputOaScreenSpaceD3dPerPolyNoHz
            | Self::RasterWmInputOaScreenSpaceMesaSimpleNoSwizNoScissor
            | Self::RasterWmInputOaScreenSpacePointList
            | Self::RasterWmInputOaScreenSpacePointListOpenBounds => Some(2),
            _ => None,
        }
    }

    fn clip_api_mode_d3d(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditions
                | Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditionsOpenBounds
                | Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditionsOpenBoundsSbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsSbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderSbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderPreClipSbePsSbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceSlot0PreClipSbePsNoSwiz
                | Self::RasterWmInputOaScreenSpaceSlot0TightPreClipRasterSbePsNoSwiz
                | Self::RasterWmInputOaScreenSpaceSlot0XyzwTightPreClipRasterSbePsNoSwiz
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1TightPreClipPerPoly
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrder
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOn
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOnEarlyBackend
                | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderPerPolySbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceD3dRasterPreconditionsNoHz
                | Self::RasterWmInputOaScreenSpaceD3dPerPolyNoHz
                | Self::RasterWmInputOaScreenSpaceMesaSimpleNoSwizNoScissor
                | Self::RasterWmInputOaScreenSpacePointList
                | Self::RasterWmInputOaScreenSpacePointListOpenBounds
        )
    }

    fn disable_sf_viewport_transform(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaScreenSpace
                | Self::RasterWmInputOaScreenSpaceNoWmHzOpPacket
                | Self::RasterWmInputOaScreenSpaceForceThreadDispatch
                | Self::RasterWmInputOaScreenSpaceSfSaneDefaults
                | Self::RasterWmInputOaScreenSpaceClipBypass
                | Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditions
                | Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditionsOpenBounds
                | Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditionsOpenBoundsSbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsSbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderSbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderPreClipSbePsSbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceSlot0PreClipSbePsNoSwiz
                | Self::RasterWmInputOaScreenSpaceSlot0TightPreClipRasterSbePsNoSwiz
                | Self::RasterWmInputOaScreenSpaceSlot0XyzwTightPreClipRasterSbePsNoSwiz
                | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderPerPolySbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceRectListClipBypass
                | Self::RasterWmInputOaScreenSpaceRectListClipBypassSfSane
                | Self::RasterWmInputOaScreenSpaceRectListBlorpLike
                | Self::RasterWmInputOaScreenSpaceRectListSlot0PerPoly
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwTightPreClipPerPoly
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1TightPreClipPerPoly
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrder
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOn
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOnEarlyBackend
                | Self::RasterWmInputOaScreenSpaceClipPreconditions
                | Self::RasterWmInputOaScreenSpaceRasterClipPreconditions
                | Self::RasterWmInputOaScreenSpaceRasterClipPreconditionsHardFence
                | Self::RasterWmInputOaScreenSpaceD3dRasterPreconditionsNoHz
                | Self::RasterWmInputOaScreenSpaceD3dPerPolyNoHz
                | Self::RasterWmInputOaScreenSpacePerPoly
                | Self::RasterWmInputOaScreenSpaceUrb128PerPoly
                | Self::RasterWmInputOaScreenSpaceMesaSimpleOrder
                | Self::RasterWmInputOaScreenSpaceMesaSimpleNoSwizNoScissor
                | Self::RasterWmInputOaScreenSpacePointList
                | Self::RasterWmInputOaScreenSpacePointListOpenBounds
                | Self::RasterWmInputOaScreenSpaceRtIndependent
                | Self::RasterWmInputOaForceOnPattern
                | Self::RasterWmInputOaForceOffPixel
        )
    }

    fn sf_deref_block_size_override(self) -> Option<u32> {
        match self {
            Self::RasterWmInputOaNdcPerPoly
            | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderPerPolySbe1NoSwiz
            | Self::RasterWmInputOaScreenSpaceD3dPerPolyNoHz
            | Self::RasterWmInputOaScreenSpacePerPoly
            | Self::RasterWmInputOaScreenSpaceUrb128PerPoly
            | Self::RasterWmInputOaScreenSpaceRectListBlorpLike
            | Self::RasterWmInputOaScreenSpaceRectListSlot0PerPoly
            | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwTightPreClipPerPoly
            | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1TightPreClipPerPoly
            | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrder
            | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOn => Some(1),
            Self::RasterWmInputOaNdcBlock32
            | Self::RasterWmInputOaNdcMesaActiveBlock
            | Self::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl
            | Self::RasterWmInputOaNdcWalk16
            | Self::RasterWmInputOaNdcClipPreconditions
            | Self::RasterWmInputOaNdcNoWmScissor
            | Self::RasterWmInputOaNdcForceOnPattern
            | Self::RasterWmInputOaScreenSpace
            | Self::RasterWmInputOaScreenSpaceNoWmHzOpPacket
            | Self::RasterWmInputOaScreenSpaceForceThreadDispatch
            | Self::RasterWmInputOaScreenSpaceSfSaneDefaults
            | Self::RasterWmInputOaScreenSpaceClipBypass
            | Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditions
            | Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditionsOpenBounds
            | Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditionsOpenBoundsSbe1NoSwiz
            | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsSbe1NoSwiz
            | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderSbe1NoSwiz
            | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderPreClipSbePsSbe1NoSwiz
            | Self::RasterWmInputOaScreenSpaceSlot0PreClipSbePsNoSwiz
            | Self::RasterWmInputOaScreenSpaceSlot0TightPreClipRasterSbePsNoSwiz
            | Self::RasterWmInputOaScreenSpaceSlot0XyzwTightPreClipRasterSbePsNoSwiz
            | Self::RasterWmInputOaScreenSpaceRectListClipBypass
            | Self::RasterWmInputOaScreenSpaceRectListClipBypassSfSane
            | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOnEarlyBackend
            | Self::RasterWmInputOaScreenSpaceClipPreconditions
            | Self::RasterWmInputOaScreenSpaceRasterClipPreconditions
            | Self::RasterWmInputOaScreenSpaceRasterClipPreconditionsHardFence
            | Self::RasterWmInputOaScreenSpaceD3dRasterPreconditionsNoHz
            | Self::RasterWmInputOaScreenSpaceRtIndependent
            | Self::RasterWmInputOaScreenSpaceMesaSimpleOrder
            | Self::RasterWmInputOaScreenSpaceMesaSimpleNoSwizNoScissor
            | Self::RasterWmInputOaScreenSpacePointList
            | Self::RasterWmInputOaScreenSpacePointListOpenBounds
            | Self::RasterWmInputOaForceOnPattern
            | Self::RasterWmInputOaForceOffPixel => Some(0),
            _ => None,
        }
    }

    fn wm_barycentric_mode_override(self) -> Option<u32> {
        match self {
            // GFX125 WM names zero as no barycentric interpolation mode.  Keep
            // the focused SF->WM coverage probe on a legal no-perspective
            // mode so scan conversion is not gated by an all-zero WM packet.
            Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOnEarlyBackend => {
                Some(8)
            }
            _ => None,
        }
    }

    fn enable_clip_non_perspective_barycentric(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOnEarlyBackend
        )
    }

    fn enable_clip_perspective_divide(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaNdcBlock32
                | Self::RasterWmInputOaNdcMesaActiveBlock
                | Self::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl
                | Self::RasterWmInputOaNdcPerPoly
                | Self::RasterWmInputOaNdcWalk16
                | Self::RasterWmInputOaNdcClipPreconditions
                | Self::RasterWmInputOaNdcNoWmScissor
                | Self::RasterWmInputOaNdcForceOnPattern
        )
    }

    fn disable_clip_enable(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaScreenSpaceClipBypass
                | Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditions
                | Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditionsOpenBounds
                | Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditionsOpenBoundsSbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceRectListClipBypass
                | Self::RasterWmInputOaScreenSpaceRectListClipBypassSfSane
                | Self::RasterWmInputOaScreenSpaceRectListBlorpLike
                | Self::RasterWmInputOaScreenSpaceRectListSlot0PerPoly
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwTightPreClipPerPoly
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1TightPreClipPerPoly
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrder
        )
    }

    fn uses_vf_position_slot0(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaScreenSpace
                | Self::RasterWmInputOaScreenSpaceNoWmHzOpPacket
                | Self::RasterWmInputOaScreenSpaceForceThreadDispatch
                | Self::RasterWmInputOaScreenSpaceSfSaneDefaults
                | Self::RasterWmInputOaScreenSpaceClipBypass
                | Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditions
                | Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditionsOpenBounds
                | Self::RasterWmInputOaScreenSpaceClipBypassRasterPreconditionsOpenBoundsSbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsSbe1NoSwiz
                | Self::RasterWmInputOaScreenSpaceSlot0PreClipSbePsNoSwiz
                | Self::RasterWmInputOaScreenSpaceSlot0TightPreClipRasterSbePsNoSwiz
                | Self::RasterWmInputOaScreenSpaceSlot0XyzwTightPreClipRasterSbePsNoSwiz
                | Self::RasterWmInputOaScreenSpaceRectListClipBypass
                | Self::RasterWmInputOaScreenSpaceRectListClipBypassSfSane
                | Self::RasterWmInputOaScreenSpaceRectListSlot0PerPoly
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwTightPreClipPerPoly
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1TightPreClipPerPoly
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrder
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOn
                | Self::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOnEarlyBackend
                | Self::RasterWmInputOaScreenSpaceClipPreconditions
                | Self::RasterWmInputOaScreenSpaceRasterClipPreconditions
                | Self::RasterWmInputOaScreenSpaceRasterClipPreconditionsHardFence
                | Self::RasterWmInputOaScreenSpaceD3dRasterPreconditionsNoHz
                | Self::RasterWmInputOaScreenSpaceD3dPerPolyNoHz
                | Self::RasterWmInputOaScreenSpacePerPoly
                | Self::RasterWmInputOaScreenSpaceUrb128PerPoly
                | Self::RasterWmInputOaScreenSpaceMesaSimpleOrder
                | Self::RasterWmInputOaScreenSpaceMesaSimpleNoSwizNoScissor
                | Self::RasterWmInputOaScreenSpacePointList
                | Self::RasterWmInputOaScreenSpacePointListOpenBounds
                | Self::RasterWmInputOaScreenSpaceRtIndependent
                | Self::RasterWmInputOaNdcBlock32
                | Self::RasterWmInputOaNdcPerPoly
                | Self::RasterWmInputOaNdcWalk16
                | Self::RasterWmInputOaNdcClipPreconditions
                | Self::RasterWmInputOaNdcNoWmScissor
                | Self::RasterWmInputOaNdcForceOnPattern
                | Self::RasterWmInputOaForceOnPattern
                | Self::RasterWmInputOaForceOffPixel
        )
    }

    fn vs_urb_entries_override(self) -> Option<u32> {
        match self {
            Self::RasterWmInputOaScreenSpaceUrb128PerPoly => Some(128),
            _ => None,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum VfPrimitiveGeometry {
    Canonical,
    Oversized,
    ScreenSpace8x8,
    ScreenSpaceInset32,
    ScreenSpaceRectInset32,
}

impl VfPrimitiveGeometry {
    fn label(self) -> &'static str {
        match self {
            Self::Canonical => "canonical",
            Self::Oversized => "oversized",
            Self::ScreenSpace8x8 => "screen-space-8x8",
            Self::ScreenSpaceInset32 => "screen-space-inset-32",
            Self::ScreenSpaceRectInset32 => "screen-space-rect-inset-32",
        }
    }

    fn vertices(self) -> [[f32; 3]; TRIANGLE_DRAW_VERTICES] {
        match self {
            Self::Canonical => [[-0.25, -0.20, 0.0], [0.25, -0.20, 0.0], [0.00, 0.20, 0.0]],
            // Oversized fullscreen-style triangle.  This is intentionally
            // boring geometry for the PS launch proof: if this still does not
            // move PS counters, coverage of the tiny canonical triangle was
            // not the blocker.
            Self::Oversized => [[-1.0, -1.0, 0.0], [3.0, -1.0, 0.0], [-1.0, 3.0, 0.0]],
            // PerspectiveDivideDisable makes position screen-space-like. This
            // variant tests that interpretation directly on the 8x8 scratch RT.
            Self::ScreenSpace8x8 => [[0.0, 0.0, 0.0], [8.0, 0.0, 0.0], [0.0, 8.0, 0.0]],
            // Same coordinate contract, but moved comfortably away from the
            // top/left scissor and draw-rectangle boundaries on a 32x32 RT.
            Self::ScreenSpaceInset32 => [[4.0, 4.0, 0.0], [24.0, 4.0, 0.0], [4.0, 24.0, 0.0]],
            // Mesa/blorp RECTLIST convention: v0 = lower-right, v1 =
            // lower-left, v2 = upper-left, with the fourth vertex implied.
            Self::ScreenSpaceRectInset32 => [[24.0, 24.0, 0.0], [4.0, 24.0, 0.0], [4.0, 4.0, 0.0]],
        }
    }

    fn fullscreen_candidate(self) -> bool {
        matches!(self, Self::Oversized)
    }

    fn pretransformed_screen_space(self) -> bool {
        matches!(
            self,
            Self::ScreenSpace8x8 | Self::ScreenSpaceInset32 | Self::ScreenSpaceRectInset32
        )
    }

    fn rectlist_candidate(self) -> bool {
        matches!(self, Self::ScreenSpaceRectInset32)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum StreamoutProofExperiment {
    PositionSlot0,
    PositionSlot0Xyzw,
    PositionSlot1,
    HeaderAndPositionSlots01,
}

const CMD_3DSTATE_VERTEX_ELEMENTS_2: u32 = 3 | (9 << 16) | (3 << 27) | (3 << 29);

impl StreamoutProofExperiment {
    fn label(self) -> &'static str {
        match self {
            Self::PositionSlot0 => "pos-slot0",
            Self::PositionSlot0Xyzw => "pos-slot0-xyzw",
            Self::PositionSlot1 => "pos-slot1",
            Self::HeaderAndPositionSlots01 => "header+pos-slots01",
        }
    }

    fn alternate(self) -> Self {
        match self {
            Self::PositionSlot0 => Self::PositionSlot0Xyzw,
            Self::PositionSlot0Xyzw => Self::PositionSlot1,
            Self::PositionSlot1 => Self::HeaderAndPositionSlots01,
            Self::HeaderAndPositionSlots01 => Self::PositionSlot0,
        }
    }

    fn vertex_bytes(self) -> usize {
        match self {
            Self::PositionSlot0 | Self::PositionSlot0Xyzw | Self::PositionSlot1 => 16,
            Self::HeaderAndPositionSlots01 => 32,
        }
    }

    fn vertex_read_length(self) -> u32 {
        1
    }

    fn so_decl_header(self) -> u32 {
        match self {
            Self::PositionSlot0 | Self::PositionSlot0Xyzw | Self::PositionSlot1 => {
                3 | (23 << 16) | (1 << 24) | (3 << 27) | (3 << 29)
            }
            Self::HeaderAndPositionSlots01 => 5 | (23 << 16) | (1 << 24) | (3 << 27) | (3 << 29),
        }
    }

    fn so_decl_buffer_selects(self) -> u32 {
        1
    }

    fn so_decl_num_entries(self) -> u32 {
        match self {
            Self::PositionSlot0 | Self::PositionSlot0Xyzw | Self::PositionSlot1 => 1,
            Self::HeaderAndPositionSlots01 => 2,
        }
    }

    fn so_decl_entry_dwords(self) -> [u32; 4] {
        match self {
            Self::PositionSlot0 => [0x0000_000F, 0x0000_0000, 0x0000_0000, 0x0000_0000],
            Self::PositionSlot0Xyzw => [0x0000_000F, 0x0000_0000, 0x0000_0000, 0x0000_0000],
            Self::PositionSlot1 => [0x0000_001F, 0x0000_0000, 0x0000_0000, 0x0000_0000],
            Self::HeaderAndPositionSlots01 => [0x0000_000F, 0x0000_0000, 0x0000_001F, 0x0000_0000],
        }
    }

    fn compatible(self) -> bool {
        true
    }

    fn vf_slot_contract(self) -> &'static str {
        match self {
            Self::PositionSlot0 => "slot0=position",
            Self::PositionSlot0Xyzw => "slot0=position-xyzw",
            Self::PositionSlot1 => "slot0=zero slot1=position",
            Self::HeaderAndPositionSlots01 => "slot0=header slot1=position",
        }
    }

    fn vf_vertex_element_count(self) -> usize {
        match self {
            Self::PositionSlot0 | Self::PositionSlot0Xyzw => 1,
            Self::PositionSlot1 | Self::HeaderAndPositionSlots01 => 2,
        }
    }
}

fn select_streamout_proof_experiment(probe_seq: u32) -> StreamoutProofExperiment {
    match probe_seq % 3 {
        0 => StreamoutProofExperiment::PositionSlot1,
        1 => StreamoutProofExperiment::HeaderAndPositionSlots01,
        _ => StreamoutProofExperiment::PositionSlot0,
    }
}

impl TriangleBatchMode {
    fn topology(self) -> u32 {
        match self {
            Self::Draw | Self::VfDraw => TRIANGLE_TOPOLOGY_TRILIST,
            Self::StreamoutProof | Self::VfStreamoutProof | Self::VsStreamoutProof => {
                TRIANGLE_TOPOLOGY_POINTLIST
            }
        }
    }

    fn streamout_enabled(self) -> bool {
        matches!(self, Self::StreamoutProof | Self::VfStreamoutProof | Self::VsStreamoutProof)
    }
}

fn is_streamout_submit_name(submit_name: &str) -> bool {
    matches!(submit_name, "streamout-proof" | "vf-streamout-proof" | "vs-streamout-proof")
}

fn is_vf_streamout_submit_name(submit_name: &str) -> bool {
    submit_name == "vf-streamout-proof"
}

fn is_triangle_debug_submit_name(submit_name: &str) -> bool {
    is_surface_draw_submit_name(submit_name)
        || is_streamout_submit_name(submit_name)
        || is_scratch_rt_submit_name(submit_name)
}

fn is_scratch_rt_submit_name(submit_name: &str) -> bool {
    matches!(
        submit_name,
        "ps-bt0-scratch-rt"
            | "raster-wm-oa-probe"
            | "real-vs-raster-wm-oa-probe"
            | "real-vs-ndc-raster-wm-oa-probe"
            | "real-vs-ndc-walk16-raster-wm-oa-probe"
            | "real-vs-ndc-32x32-walk16-raster-wm-oa-probe"
            | "real-vs-ndc-mesa-active-block-raster-wm-oa-probe"
            | "real-vs-ndc-clip-preconditions-raster-wm-oa-probe"
            | "real-vs-screen-clip-preconditions-raster-wm-oa-probe"
            | "real-vs-screen-raster-clip-preconditions-raster-wm-oa-probe"
            | "real-vs-screen-d3d-raster-no-hz-raster-wm-oa-probe"
            | "real-vs-screen-d3d-perpoly-raster-no-hz-raster-wm-oa-probe"
            | "real-vs-screen-d3d-slot0-raster-no-hz-raster-wm-oa-probe"
            | "real-vs-screen-d3d-inset-raster-no-hz-raster-wm-oa-probe"
            | "real-vs-screen-inset-boring-raster-wm-oa-probe"
            | "real-vs-screen-inset-no-wm-hz-op-packet-raster-wm-oa-probe"
            | "real-vs-screen-inset-slot0-raster-wm-oa-probe"
            | "real-vs-screen-inset-clip-bypass-raster-wm-oa-probe"
            | "real-vs-screen-rectlist-clip-bypass-raster-wm-oa-probe"
            | "real-vs-screen-rectlist-clip-bypass-sf-sane-raster-wm-oa-probe"
            | "real-vs-screen-rectlist-blorp-like-raster-wm-oa-probe"
            | "real-vs-screen-rectlist-slot0-perpoly-raster-wm-oa-probe"
            | "real-vs-screen-inset-force-wm-thread-raster-wm-oa-probe"
            | "real-vs-screen-inset-sf-sane-raster-wm-oa-probe"
            | "real-vs-screen-inset-urb2-sf-sane-raster-wm-oa-probe"
            | "real-vs-screen-inset-perpoly-raster-wm-oa-probe"
            | "real-vs-screen-inset-urb128-perpoly-raster-wm-oa-probe"
            | "real-vs-screen-inset-mesa-simple-order-raster-wm-oa-probe"
            | "real-vs-screen-inset-mesa-noswiz-noscissor-raster-wm-oa-probe"
            | "real-vs-screen-pointlist-raster-wm-oa-probe"
            | "real-vs-screen-rt-independent-raster-wm-oa-probe"
            | "late-vf-screen-inset-clip-bypass-raster-preconditions-raster-wm-oa-probe"
            | "late-vf-screen-inset-open-bounds-raster-wm-oa-probe"
            | "late-vf-screen-inset-open-bounds-sbe1-noswiz-raster-wm-oa-probe"
            | "late-vf-screen-inset-open-bounds-clip-on-sbe1-noswiz-raster-wm-oa-probe"
            | "late-vf-screen-inset-open-bounds-header-sbe1-noswiz-raster-wm-oa-probe"
            | "late-vf-screen-inset-open-bounds-header-preclip-sbe-ps-sbe1-noswiz-raster-wm-oa-probe"
            | "late-vf-screen-inset-slot0-preclip-sbe-ps-noswiz-raster-wm-oa-probe"
            | "late-vf-screen-inset-slot0-tight-preclip-raster-sbe-ps-noswiz-raster-wm-oa-probe"
            | "late-vf-screen-inset-slot0-xyzw-tight-preclip-raster-sbe-ps-noswiz-raster-wm-oa-probe"
            | "late-vf-screen-rectlist-slot0-xyzw-tight-preclip-perpoly-raster-wm-oa-probe"
            | "late-vf-screen-rectlist-slot0-xyzw-sbe1-tight-preclip-perpoly-raster-wm-oa-probe"
            | "late-vf-screen-rectlist-slot0-xyzw-sbe1-mesa-order-raster-wm-oa-probe"
            | "late-vf-screen-rectlist-slot0-xyzw-sbe1-mesa-order-clip-on-raster-wm-oa-probe"
            | "late-vf-screen-rectlist-slot0-xyzw-sbe1-mesa-order-clip-on-early-backend-raster-wm-oa-probe"
            | "late-vf-ndc-mesa-active-block-raster-wm-oa-probe"
            | "late-vf-ndc-centered-mesa-active-block-raster-wm-oa-probe"
            | "late-vf-ndc-centered-mesa-active-noprimrepl-raster-wm-oa-probe"
            | "late-vf-screen-pointlist-slot0-xyzw-preclip-raster-wm-oa-probe"
            | "late-vf-screen-pointlist-slot0-xyzw-open-bounds-raster-wm-oa-probe"
            | "late-vf-screen-inset-open-bounds-header-perpoly-sbe1-noswiz-raster-wm-oa-probe"
            | "real-vs-screen-inset-raster-clip-preconditions-late-raster-wm-oa-probe"
            | "real-vs-ndc-perpoly-raster-wm-oa-probe"
            | "real-vs-ndc-no-scissor-raster-wm-oa-probe"
            | "real-vs-ndc-ms-raster-wm-oa-probe"
    )
}

fn is_raster_wm_oa_submit_name(submit_name: &str) -> bool {
    matches!(
        submit_name,
        "raster-wm-oa-probe"
            | "real-vs-raster-wm-oa-probe"
            | "real-vs-ndc-raster-wm-oa-probe"
            | "real-vs-ndc-walk16-raster-wm-oa-probe"
            | "real-vs-ndc-32x32-walk16-raster-wm-oa-probe"
            | "real-vs-ndc-mesa-active-block-raster-wm-oa-probe"
            | "real-vs-ndc-clip-preconditions-raster-wm-oa-probe"
            | "real-vs-screen-clip-preconditions-raster-wm-oa-probe"
            | "real-vs-screen-raster-clip-preconditions-raster-wm-oa-probe"
            | "real-vs-screen-d3d-raster-no-hz-raster-wm-oa-probe"
            | "real-vs-screen-d3d-perpoly-raster-no-hz-raster-wm-oa-probe"
            | "real-vs-screen-d3d-slot0-raster-no-hz-raster-wm-oa-probe"
            | "real-vs-screen-d3d-inset-raster-no-hz-raster-wm-oa-probe"
            | "real-vs-screen-inset-boring-raster-wm-oa-probe"
            | "real-vs-screen-inset-no-wm-hz-op-packet-raster-wm-oa-probe"
            | "real-vs-screen-inset-slot0-raster-wm-oa-probe"
            | "real-vs-screen-inset-clip-bypass-raster-wm-oa-probe"
            | "real-vs-screen-rectlist-clip-bypass-raster-wm-oa-probe"
            | "real-vs-screen-rectlist-clip-bypass-sf-sane-raster-wm-oa-probe"
            | "real-vs-screen-rectlist-blorp-like-raster-wm-oa-probe"
            | "real-vs-screen-rectlist-slot0-perpoly-raster-wm-oa-probe"
            | "real-vs-screen-inset-force-wm-thread-raster-wm-oa-probe"
            | "real-vs-screen-inset-sf-sane-raster-wm-oa-probe"
            | "real-vs-screen-inset-urb2-sf-sane-raster-wm-oa-probe"
            | "real-vs-screen-inset-perpoly-raster-wm-oa-probe"
            | "real-vs-screen-inset-urb128-perpoly-raster-wm-oa-probe"
            | "real-vs-screen-inset-mesa-simple-order-raster-wm-oa-probe"
            | "real-vs-screen-inset-mesa-noswiz-noscissor-raster-wm-oa-probe"
            | "real-vs-screen-pointlist-raster-wm-oa-probe"
            | "real-vs-screen-rt-independent-raster-wm-oa-probe"
            | "late-vf-screen-inset-clip-bypass-raster-preconditions-raster-wm-oa-probe"
            | "late-vf-screen-inset-open-bounds-raster-wm-oa-probe"
            | "late-vf-screen-inset-open-bounds-sbe1-noswiz-raster-wm-oa-probe"
            | "late-vf-screen-inset-open-bounds-clip-on-sbe1-noswiz-raster-wm-oa-probe"
            | "late-vf-screen-inset-open-bounds-header-sbe1-noswiz-raster-wm-oa-probe"
            | "late-vf-screen-inset-open-bounds-header-preclip-sbe-ps-sbe1-noswiz-raster-wm-oa-probe"
            | "late-vf-screen-inset-slot0-preclip-sbe-ps-noswiz-raster-wm-oa-probe"
            | "late-vf-screen-inset-slot0-tight-preclip-raster-sbe-ps-noswiz-raster-wm-oa-probe"
            | "late-vf-screen-inset-slot0-xyzw-tight-preclip-raster-sbe-ps-noswiz-raster-wm-oa-probe"
            | "late-vf-screen-rectlist-slot0-xyzw-tight-preclip-perpoly-raster-wm-oa-probe"
            | "late-vf-screen-rectlist-slot0-xyzw-sbe1-tight-preclip-perpoly-raster-wm-oa-probe"
            | "late-vf-screen-rectlist-slot0-xyzw-sbe1-mesa-order-raster-wm-oa-probe"
            | "late-vf-screen-rectlist-slot0-xyzw-sbe1-mesa-order-clip-on-raster-wm-oa-probe"
            | "late-vf-screen-rectlist-slot0-xyzw-sbe1-mesa-order-clip-on-early-backend-raster-wm-oa-probe"
            | "late-vf-ndc-mesa-active-block-raster-wm-oa-probe"
            | "late-vf-ndc-centered-mesa-active-block-raster-wm-oa-probe"
            | "late-vf-ndc-centered-mesa-active-noprimrepl-raster-wm-oa-probe"
            | "late-vf-screen-pointlist-slot0-xyzw-preclip-raster-wm-oa-probe"
            | "late-vf-screen-pointlist-slot0-xyzw-open-bounds-raster-wm-oa-probe"
            | "late-vf-screen-inset-open-bounds-header-perpoly-sbe1-noswiz-raster-wm-oa-probe"
            | "real-vs-screen-inset-raster-clip-preconditions-late-raster-wm-oa-probe"
            | "real-vs-ndc-perpoly-raster-wm-oa-probe"
            | "real-vs-ndc-no-scissor-raster-wm-oa-probe"
            | "real-vs-ndc-ms-raster-wm-oa-probe"
    )
}

fn is_surface_draw_submit_name(submit_name: &str) -> bool {
    matches!(
        submit_name,
        "draw-path"
            | "vf-draw-path"
            | "ps-launch-big-primitive"
            | "ps-bt1-big-primitive"
            | "ps-wm-normal-big-primitive"
            | "ps-dispatch-slot0-big-primitive"
            | "ps-dispatch-slot1-big-primitive"
            | "ps-dispatch-slot2-big-primitive"
            | "ps-dispatch-all-big-primitive"
            | "ps-payload-push-big-primitive"
            | "ps-payload-attr-big-primitive"
            | "ps-payload-simple-big-primitive"
            | "ps-payload-source-depth-w-big-primitive"
            | "ps-payload-bary-big-primitive"
            | "ps-grf-start-r1-big-primitive"
            | "ps-grf-start-r2-big-primitive"
            | "ps-grf-start-r4-big-primitive"
            | "ps-grf-maxthreads-31-big-primitive"
            | "ps-grf-maxthreads-15-big-primitive"
            | "postdraw-light-only-retire"
            | "postdraw-flush-bit5"
            | "postdraw-flush-bit7"
            | "postdraw-flush-bit12"
            | "postdraw-flush-bit20"
            | "postdraw-flush-bit26"
            | "postdraw-pc-postsync-no-cs"
            | "postdraw-pc-cs-no-postsync"
            | "raster-wm-oa-probe"
            | "real-vs-raster-wm-oa-probe"
            | "real-vs-ndc-raster-wm-oa-probe"
            | "real-vs-ndc-walk16-raster-wm-oa-probe"
            | "real-vs-ndc-32x32-walk16-raster-wm-oa-probe"
            | "real-vs-ndc-mesa-active-block-raster-wm-oa-probe"
            | "real-vs-ndc-clip-preconditions-raster-wm-oa-probe"
            | "real-vs-screen-clip-preconditions-raster-wm-oa-probe"
            | "real-vs-screen-raster-clip-preconditions-raster-wm-oa-probe"
            | "real-vs-screen-d3d-raster-no-hz-raster-wm-oa-probe"
            | "real-vs-screen-d3d-perpoly-raster-no-hz-raster-wm-oa-probe"
            | "real-vs-screen-d3d-slot0-raster-no-hz-raster-wm-oa-probe"
            | "real-vs-screen-inset-boring-raster-wm-oa-probe"
            | "real-vs-screen-inset-no-wm-hz-op-packet-raster-wm-oa-probe"
            | "real-vs-screen-inset-slot0-raster-wm-oa-probe"
            | "real-vs-screen-inset-clip-bypass-raster-wm-oa-probe"
            | "real-vs-screen-rectlist-clip-bypass-raster-wm-oa-probe"
            | "real-vs-screen-rectlist-clip-bypass-sf-sane-raster-wm-oa-probe"
            | "real-vs-screen-rectlist-blorp-like-raster-wm-oa-probe"
            | "real-vs-screen-rectlist-slot0-perpoly-raster-wm-oa-probe"
            | "real-vs-screen-inset-force-wm-thread-raster-wm-oa-probe"
            | "real-vs-screen-inset-sf-sane-raster-wm-oa-probe"
            | "real-vs-screen-inset-urb2-sf-sane-raster-wm-oa-probe"
            | "real-vs-screen-inset-perpoly-raster-wm-oa-probe"
            | "real-vs-screen-inset-urb128-perpoly-raster-wm-oa-probe"
            | "real-vs-screen-inset-mesa-simple-order-raster-wm-oa-probe"
            | "real-vs-screen-inset-mesa-noswiz-noscissor-raster-wm-oa-probe"
            | "real-vs-screen-pointlist-raster-wm-oa-probe"
            | "real-vs-screen-rt-independent-raster-wm-oa-probe"
            | "late-vf-screen-inset-open-bounds-raster-wm-oa-probe"
            | "late-vf-screen-inset-open-bounds-sbe1-noswiz-raster-wm-oa-probe"
            | "late-vf-screen-inset-open-bounds-clip-on-sbe1-noswiz-raster-wm-oa-probe"
            | "late-vf-screen-inset-open-bounds-header-sbe1-noswiz-raster-wm-oa-probe"
            | "late-vf-screen-inset-open-bounds-header-preclip-sbe-ps-sbe1-noswiz-raster-wm-oa-probe"
            | "late-vf-screen-inset-slot0-preclip-sbe-ps-noswiz-raster-wm-oa-probe"
            | "late-vf-screen-inset-slot0-tight-preclip-raster-sbe-ps-noswiz-raster-wm-oa-probe"
            | "late-vf-screen-inset-slot0-xyzw-tight-preclip-raster-sbe-ps-noswiz-raster-wm-oa-probe"
            | "late-vf-screen-rectlist-slot0-xyzw-tight-preclip-perpoly-raster-wm-oa-probe"
            | "late-vf-screen-rectlist-slot0-xyzw-sbe1-tight-preclip-perpoly-raster-wm-oa-probe"
            | "late-vf-screen-rectlist-slot0-xyzw-sbe1-mesa-order-raster-wm-oa-probe"
            | "late-vf-screen-rectlist-slot0-xyzw-sbe1-mesa-order-clip-on-raster-wm-oa-probe"
            | "late-vf-screen-rectlist-slot0-xyzw-sbe1-mesa-order-clip-on-early-backend-raster-wm-oa-probe"
            | "late-vf-ndc-mesa-active-block-raster-wm-oa-probe"
            | "late-vf-ndc-centered-mesa-active-block-raster-wm-oa-probe"
            | "late-vf-ndc-centered-mesa-active-noprimrepl-raster-wm-oa-probe"
            | "late-vf-screen-pointlist-slot0-xyzw-preclip-raster-wm-oa-probe"
            | "late-vf-screen-pointlist-slot0-xyzw-open-bounds-raster-wm-oa-probe"
            | "late-vf-screen-inset-open-bounds-header-perpoly-sbe1-noswiz-raster-wm-oa-probe"
            | "real-vs-screen-inset-raster-clip-preconditions-late-raster-wm-oa-probe"
            | "real-vs-ndc-perpoly-raster-wm-oa-probe"
            | "real-vs-ndc-no-scissor-raster-wm-oa-probe"
            | "real-vs-ndc-ms-raster-wm-oa-probe"
            | "vs-draw-frontier"
    )
}

fn is_fragment_candidate_submit_name(submit_name: &str) -> bool {
    matches!(
        submit_name,
        "ps-launch-big-primitive"
            | "ps-bt0-scratch-rt"
            | "raster-wm-oa-probe"
            | "real-vs-raster-wm-oa-probe"
            | "real-vs-ndc-raster-wm-oa-probe"
            | "real-vs-ndc-walk16-raster-wm-oa-probe"
            | "real-vs-ndc-32x32-walk16-raster-wm-oa-probe"
            | "real-vs-ndc-mesa-active-block-raster-wm-oa-probe"
            | "real-vs-ndc-clip-preconditions-raster-wm-oa-probe"
            | "real-vs-screen-clip-preconditions-raster-wm-oa-probe"
            | "real-vs-screen-raster-clip-preconditions-raster-wm-oa-probe"
            | "real-vs-screen-d3d-raster-no-hz-raster-wm-oa-probe"
            | "real-vs-screen-d3d-perpoly-raster-no-hz-raster-wm-oa-probe"
            | "real-vs-screen-d3d-slot0-raster-no-hz-raster-wm-oa-probe"
            | "real-vs-screen-inset-boring-raster-wm-oa-probe"
            | "real-vs-screen-inset-no-wm-hz-op-packet-raster-wm-oa-probe"
            | "real-vs-screen-inset-slot0-raster-wm-oa-probe"
            | "real-vs-screen-inset-clip-bypass-raster-wm-oa-probe"
            | "real-vs-screen-rectlist-clip-bypass-raster-wm-oa-probe"
            | "real-vs-screen-rectlist-clip-bypass-sf-sane-raster-wm-oa-probe"
            | "real-vs-screen-rectlist-blorp-like-raster-wm-oa-probe"
            | "real-vs-screen-rectlist-slot0-perpoly-raster-wm-oa-probe"
            | "real-vs-screen-inset-force-wm-thread-raster-wm-oa-probe"
            | "real-vs-screen-inset-sf-sane-raster-wm-oa-probe"
            | "real-vs-screen-inset-urb2-sf-sane-raster-wm-oa-probe"
            | "real-vs-screen-inset-perpoly-raster-wm-oa-probe"
            | "real-vs-screen-inset-urb128-perpoly-raster-wm-oa-probe"
            | "real-vs-screen-inset-mesa-simple-order-raster-wm-oa-probe"
            | "real-vs-screen-inset-mesa-noswiz-noscissor-raster-wm-oa-probe"
            | "real-vs-screen-pointlist-raster-wm-oa-probe"
            | "real-vs-screen-rt-independent-raster-wm-oa-probe"
            | "late-vf-screen-inset-open-bounds-raster-wm-oa-probe"
            | "late-vf-screen-inset-open-bounds-sbe1-noswiz-raster-wm-oa-probe"
            | "late-vf-screen-inset-open-bounds-clip-on-sbe1-noswiz-raster-wm-oa-probe"
            | "late-vf-screen-inset-open-bounds-header-sbe1-noswiz-raster-wm-oa-probe"
            | "late-vf-screen-inset-open-bounds-header-preclip-sbe-ps-sbe1-noswiz-raster-wm-oa-probe"
            | "late-vf-screen-inset-slot0-preclip-sbe-ps-noswiz-raster-wm-oa-probe"
            | "late-vf-screen-inset-slot0-tight-preclip-raster-sbe-ps-noswiz-raster-wm-oa-probe"
            | "late-vf-screen-inset-slot0-xyzw-tight-preclip-raster-sbe-ps-noswiz-raster-wm-oa-probe"
            | "late-vf-screen-rectlist-slot0-xyzw-tight-preclip-perpoly-raster-wm-oa-probe"
            | "late-vf-screen-rectlist-slot0-xyzw-sbe1-tight-preclip-perpoly-raster-wm-oa-probe"
            | "late-vf-screen-rectlist-slot0-xyzw-sbe1-mesa-order-raster-wm-oa-probe"
            | "late-vf-screen-rectlist-slot0-xyzw-sbe1-mesa-order-clip-on-raster-wm-oa-probe"
            | "late-vf-screen-rectlist-slot0-xyzw-sbe1-mesa-order-clip-on-early-backend-raster-wm-oa-probe"
            | "late-vf-ndc-mesa-active-block-raster-wm-oa-probe"
            | "late-vf-ndc-centered-mesa-active-block-raster-wm-oa-probe"
            | "late-vf-ndc-centered-mesa-active-noprimrepl-raster-wm-oa-probe"
            | "late-vf-screen-pointlist-slot0-xyzw-preclip-raster-wm-oa-probe"
            | "late-vf-screen-pointlist-slot0-xyzw-open-bounds-raster-wm-oa-probe"
            | "late-vf-screen-inset-open-bounds-header-perpoly-sbe1-noswiz-raster-wm-oa-probe"
            | "real-vs-screen-inset-raster-clip-preconditions-late-raster-wm-oa-probe"
            | "real-vs-ndc-perpoly-raster-wm-oa-probe"
            | "real-vs-ndc-no-scissor-raster-wm-oa-probe"
            | "real-vs-ndc-ms-raster-wm-oa-probe"
            | "ps-bt1-big-primitive"
            | "ps-wm-normal-big-primitive"
            | "ps-dispatch-slot0-big-primitive"
            | "ps-dispatch-slot1-big-primitive"
            | "ps-dispatch-slot2-big-primitive"
            | "ps-payload-push-big-primitive"
            | "ps-payload-attr-big-primitive"
            | "ps-payload-simple-big-primitive"
            | "ps-payload-source-depth-w-big-primitive"
            | "ps-payload-bary-big-primitive"
            | "ps-grf-start-r1-big-primitive"
            | "ps-grf-start-r2-big-primitive"
            | "ps-grf-start-r4-big-primitive"
            | "ps-grf-maxthreads-31-big-primitive"
            | "ps-grf-maxthreads-15-big-primitive"
    )
}

fn reset_fragment_boundary_probe() {
    FRAGMENT_CANDIDATE_READY.store(false, Ordering::Release);
    FRAGMENT_BOUNDARY_OBSERVED.store(false, Ordering::Release);
    WM_COVERAGE_OBSERVED.store(false, Ordering::Release);
    PSD_DISPATCH_OBSERVED.store(false, Ordering::Release);
}

fn record_fragment_boundary_probe(candidate_ready: bool, fragment_observed: bool) {
    if candidate_ready {
        FRAGMENT_CANDIDATE_READY.store(true, Ordering::Release);
    }
    if fragment_observed {
        FRAGMENT_BOUNDARY_OBSERVED.store(true, Ordering::Release);
    }
}

fn record_wm_psd_boundary_probe(wm_coverage_observed: bool, psd_dispatch_observed: bool) {
    if wm_coverage_observed {
        WM_COVERAGE_OBSERVED.store(true, Ordering::Release);
    }
    if psd_dispatch_observed {
        PSD_DISPATCH_OBSERVED.store(true, Ordering::Release);
    }
}

fn fragment_candidate_ready() -> bool {
    FRAGMENT_CANDIDATE_READY.load(Ordering::Acquire)
}

fn fragment_boundary_observed() -> bool {
    FRAGMENT_BOUNDARY_OBSERVED.load(Ordering::Acquire)
}

fn wm_coverage_observed() -> bool {
    WM_COVERAGE_OBSERVED.load(Ordering::Acquire)
}

fn psd_dispatch_observed() -> bool {
    PSD_DISPATCH_OBSERVED.load(Ordering::Acquire)
}

unsafe impl Send for RenderWarmState {}
unsafe impl Sync for RenderWarmState {}

static WARM_STATE: Mutex<Option<RenderWarmState>> = Mutex::new(None);
static PRIMARY_TRIANGLE_SUBMITTED: AtomicBool = AtomicBool::new(false);
static PRIMARY_PROBE_IN_FLIGHT: AtomicBool = AtomicBool::new(false);
static PRIMARY_MI_SCANOUT_PROOF_SUBMITTED: AtomicBool = AtomicBool::new(false);
static FRAGMENT_CANDIDATE_READY: AtomicBool = AtomicBool::new(false);
static FRAGMENT_BOUNDARY_OBSERVED: AtomicBool = AtomicBool::new(false);
static WM_COVERAGE_OBSERVED: AtomicBool = AtomicBool::new(false);
static PSD_DISPATCH_OBSERVED: AtomicBool = AtomicBool::new(false);
static WARM_BUFFERS_MAPPED: AtomicBool = AtomicBool::new(false);
static MEMORY_PROOF_LOGGED: AtomicBool = AtomicBool::new(false);
static PRIMARY_STRIPE_X_PHASE: AtomicU32 = AtomicU32::new(0);
static PRIMARY_PROBE_SEQ: AtomicU32 = AtomicU32::new(0);
