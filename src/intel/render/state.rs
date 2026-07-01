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
    cps_state_offset_bytes: u32,
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
    DrawScreenSpace,
    DrawScreenSpaceRect,
    VfDraw,
    VfPointDraw,
    VfLineDraw,
    VfRectDraw,
    VfRectClipDraw,
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
    PsSimd16,
    PsEotOnly,
    PsCpsDisabled,
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
    WmHzSampleMask,
    WmLateReemit,
    RasterWmInputOa,
    RasterWmInputOaSurfaceHalign128,
    RasterWmInputOaKillOff,
    RasterWmInputOaSmoothPoint,
    RasterWmInputOaMsRaster,
    RasterWmInputOaMsRasterForced,
    RasterWmInputOaDerefBlock0,
    RasterWmInputOaNoHzOp,
    RasterWmInputOaWmNormalDispatch,
    RasterWmInputOaWmReemitAfterPsExtra,
    RasterWmInputOaOmitHzOp,
    RasterWmInputOaPsDisabled,
    RasterWmInputOaBtCountOne,
    RasterWmInputOaScissorOnly,
    RasterWmInputOaMesaSimpleRect,
    RasterWmInputOaMesaSimpleRectEarly,
    RasterWmInputOaMesaSimpleRectArtificial,
    RasterWmInputOaMesaSimpleRectNoSrcHeader,
    RasterWmInputOaEarlySample,
    RasterWmInputOaEarlyKillOff,
    RasterWmInputOaClipNormal,
    RasterWmInputOaClipPerspective,
    RasterWmInputOaClipDisabled,
    RasterWmInputOaClipDisabledArtificial,
    RasterWmInputOaClipForceMode,
    RasterWmInputOaClipApiD3d,
    RasterWmInputOaClipViewportXy,
    RasterWmInputOaEarlyClipViewportXy,
    RasterWmInputOaEarlyPointWidth1023,
    RasterWmInputOaEarlyMsRasterForced,
    RasterWmInputOaSbeBeforeClip,
    RasterWmInputOaSbeBeforeSf,
    RasterWmInputOaSbeRead0,
    RasterWmInputOaDrawRectEarlyOnly,
    RasterWmInputOaSampleMaskEarlyOnly,
    RasterWmInputOaPipeControlClipSf,
    RasterWmInputOaWmHzOpBeforeWm,
    RasterWmInputOaWmHzOpAfterPsExtra,
    RasterWmInputOaPayloadAttributeEnable,
    RasterWmInputOaPayloadSourceDepthW,
    RasterWmInputOaPayloadBaryPlanes,
    RasterWmInputOaFrontCcw,
    RasterWmInputOaNoPrimitiveReplication,
    RasterWmInputOaVfGeometryDistribution,
    RasterWmInputOaPointWidth8,
    RasterWmInputOaPointWidth8ClipMax,
    RasterWmInputOaPointWidth64,
    RasterWmInputOaPointWidth64SurfaceHalign128,
    RasterWmInputOaPointWidth64ClipMax,
    RasterWmInputOaPointWidth64Early,
    RasterWmInputOaPointWidth64EarlyScissor,
    RasterWmInputOaPointWidth64Screen,
    RasterWmInputOaPointWidth64Artificial,
    RasterWmInputOaPointWidth64WmNormalDispatch,
    RasterWmInputOaPointWidth64WmReemitAfterPsExtra,
    RasterWmInputOaPointWidth64OmitHzOp,
    RasterWmInputOaPointWidth64PsDisabled,
    RasterWmInputOaPointWidth64PayloadAttributeEnable,
    RasterWmInputOaPointWidth64PayloadSourceDepthW,
    RasterWmInputOaPointWidth64PayloadBaryPlanes,
    RasterWmInputOaPointWidth64SbeBeforeClip,
    RasterWmInputOaPointWidth64SbeBeforeSf,
    RasterWmInputOaPointWidth1023,
    RasterWmInputOaPointWidth1023NoWmPoint,
    RasterWmInputOaPointWidth1023Scissor,
    RasterWmInputOaPointWidthVertex,
    RasterWmInputOaHammer,
    RasterWmInputOaScreenHammer,
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
            Self::PsSimd16 => "ps-simd16",
            Self::PsEotOnly => "ps-eot-only",
            Self::PsCpsDisabled => "ps-cps-disabled",
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
            Self::WmHzSampleMask => "wm-hz-sample-mask",
            Self::WmLateReemit => "wm-late-reemit",
            Self::RasterWmInputOa => "raster-wm-input-oa",
            Self::RasterWmInputOaSurfaceHalign128 => "raster-wm-input-oa-surface-halign-128",
            Self::RasterWmInputOaKillOff => "raster-wm-input-oa-killoff",
            Self::RasterWmInputOaSmoothPoint => "raster-wm-input-oa-smooth-point",
            Self::RasterWmInputOaMsRaster => "raster-wm-input-oa-ms-raster",
            Self::RasterWmInputOaMsRasterForced => "raster-wm-input-oa-ms-raster-forced",
            Self::RasterWmInputOaDerefBlock0 => "raster-wm-input-oa-deref-block-0",
            Self::RasterWmInputOaNoHzOp => "raster-wm-input-oa-no-hz-op",
            Self::RasterWmInputOaWmNormalDispatch => "raster-wm-input-oa-wm-normal-dispatch",
            Self::RasterWmInputOaWmReemitAfterPsExtra => {
                "raster-wm-input-oa-wm-reemit-after-ps-extra"
            }
            Self::RasterWmInputOaOmitHzOp => "raster-wm-input-oa-omit-hz-op",
            Self::RasterWmInputOaPsDisabled => "raster-wm-input-oa-ps-disabled",
            Self::RasterWmInputOaBtCountOne => "raster-wm-input-oa-bt-count-1",
            Self::RasterWmInputOaScissorOnly => "raster-wm-input-oa-scissor-only",
            Self::RasterWmInputOaMesaSimpleRect => "raster-wm-input-oa-mesa-simple-rect",
            Self::RasterWmInputOaMesaSimpleRectEarly => "raster-wm-input-oa-mesa-simple-rect-early",
            Self::RasterWmInputOaMesaSimpleRectArtificial => {
                "raster-wm-input-oa-mesa-simple-rect-artificial"
            }
            Self::RasterWmInputOaMesaSimpleRectNoSrcHeader => {
                "raster-wm-input-oa-mesa-simple-rect-nosrc-header"
            }
            Self::RasterWmInputOaEarlySample => "raster-wm-input-oa-early-sample",
            Self::RasterWmInputOaEarlyKillOff => "raster-wm-input-oa-early-killoff",
            Self::RasterWmInputOaClipNormal => "raster-wm-input-oa-clip-normal",
            Self::RasterWmInputOaClipPerspective => "raster-wm-input-oa-clip-perspective",
            Self::RasterWmInputOaClipDisabled => "raster-wm-input-oa-clip-disabled",
            Self::RasterWmInputOaClipDisabledArtificial => {
                "raster-wm-input-oa-clip-disabled-artificial"
            }
            Self::RasterWmInputOaClipForceMode => "raster-wm-input-oa-clip-force-mode",
            Self::RasterWmInputOaClipApiD3d => "raster-wm-input-oa-clip-api-d3d",
            Self::RasterWmInputOaClipViewportXy => "raster-wm-input-oa-clip-viewport-xy",
            Self::RasterWmInputOaEarlyClipViewportXy => "raster-wm-input-oa-early-clip-viewport-xy",
            Self::RasterWmInputOaEarlyPointWidth1023 => "raster-wm-input-oa-early-point-width-1023",
            Self::RasterWmInputOaEarlyMsRasterForced => "raster-wm-input-oa-early-ms-raster-forced",
            Self::RasterWmInputOaSbeBeforeClip => "raster-wm-input-oa-sbe-before-clip",
            Self::RasterWmInputOaSbeBeforeSf => "raster-wm-input-oa-sbe-before-sf",
            Self::RasterWmInputOaSbeRead0 => "raster-wm-input-oa-sbe-read0",
            Self::RasterWmInputOaDrawRectEarlyOnly => "raster-wm-input-oa-draw-rect-early-only",
            Self::RasterWmInputOaSampleMaskEarlyOnly => "raster-wm-input-oa-sample-mask-early-only",
            Self::RasterWmInputOaPipeControlClipSf => "raster-wm-input-oa-pipe-control-clip-sf",
            Self::RasterWmInputOaWmHzOpBeforeWm => "raster-wm-input-oa-wm-hz-op-before-wm",
            Self::RasterWmInputOaWmHzOpAfterPsExtra => "raster-wm-input-oa-wm-hz-op-after-ps-extra",
            Self::RasterWmInputOaPayloadAttributeEnable => {
                "raster-wm-input-oa-payload-attribute-enable"
            }
            Self::RasterWmInputOaPayloadSourceDepthW => "raster-wm-input-oa-payload-source-depth-w",
            Self::RasterWmInputOaPayloadBaryPlanes => "raster-wm-input-oa-payload-bary-planes",
            Self::RasterWmInputOaFrontCcw => "raster-wm-input-oa-front-ccw",
            Self::RasterWmInputOaNoPrimitiveReplication => {
                "raster-wm-input-oa-no-primitive-replication"
            }
            Self::RasterWmInputOaVfGeometryDistribution => {
                "raster-wm-input-oa-vf-geometry-distribution"
            }
            Self::RasterWmInputOaPointWidth8 => "raster-wm-input-oa-point-width-8",
            Self::RasterWmInputOaPointWidth8ClipMax => "raster-wm-input-oa-point-width-8-clipmax",
            Self::RasterWmInputOaPointWidth64 => "raster-wm-input-oa-point-width-64",
            Self::RasterWmInputOaPointWidth64SurfaceHalign128 => {
                "raster-wm-input-oa-point-width-64-surface-halign-128"
            }
            Self::RasterWmInputOaPointWidth64ClipMax => "raster-wm-input-oa-point-width-64-clipmax",
            Self::RasterWmInputOaPointWidth64Early => "raster-wm-input-oa-point-width-64-early",
            Self::RasterWmInputOaPointWidth64EarlyScissor => {
                "raster-wm-input-oa-point-width-64-early-scissor"
            }
            Self::RasterWmInputOaPointWidth64Screen => "raster-wm-input-oa-point-width-64-screen",
            Self::RasterWmInputOaPointWidth64Artificial => {
                "raster-wm-input-oa-point-width-64-artificial"
            }
            Self::RasterWmInputOaPointWidth64WmNormalDispatch => {
                "raster-wm-input-oa-point-width-64-wm-normal-dispatch"
            }
            Self::RasterWmInputOaPointWidth64WmReemitAfterPsExtra => {
                "raster-wm-input-oa-point-width-64-wm-reemit-after-ps-extra"
            }
            Self::RasterWmInputOaPointWidth64OmitHzOp => {
                "raster-wm-input-oa-point-width-64-omit-hz-op"
            }
            Self::RasterWmInputOaPointWidth64PsDisabled => {
                "raster-wm-input-oa-point-width-64-ps-disabled"
            }
            Self::RasterWmInputOaPointWidth64PayloadAttributeEnable => {
                "raster-wm-input-oa-point-width-64-payload-attribute-enable"
            }
            Self::RasterWmInputOaPointWidth64PayloadSourceDepthW => {
                "raster-wm-input-oa-point-width-64-payload-source-depth-w"
            }
            Self::RasterWmInputOaPointWidth64PayloadBaryPlanes => {
                "raster-wm-input-oa-point-width-64-payload-bary-planes"
            }
            Self::RasterWmInputOaPointWidth64SbeBeforeClip => {
                "raster-wm-input-oa-point-width-64-sbe-before-clip"
            }
            Self::RasterWmInputOaPointWidth64SbeBeforeSf => {
                "raster-wm-input-oa-point-width-64-sbe-before-sf"
            }
            Self::RasterWmInputOaPointWidth1023 => "raster-wm-input-oa-point-width-1023",
            Self::RasterWmInputOaPointWidth1023NoWmPoint => {
                "raster-wm-input-oa-point-width-1023-no-wm-point"
            }
            Self::RasterWmInputOaPointWidth1023Scissor => {
                "raster-wm-input-oa-point-width-1023-scissor"
            }
            Self::RasterWmInputOaPointWidthVertex => "raster-wm-input-oa-point-width-vertex",
            Self::RasterWmInputOaHammer => "raster-wm-input-oa-hammer",
            Self::RasterWmInputOaScreenHammer => "raster-wm-input-oa-screen-hammer",
        }
    }

    fn ps_dispatch_slot(self) -> Option<u8> {
        match self {
            Self::PsDispatchSlot0 => Some(0),
            Self::PsDispatchSlot1 | Self::PsSimd16 => Some(1),
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
                | Self::RasterWmInputOaSurfaceHalign128
                | Self::RasterWmInputOaKillOff
                | Self::RasterWmInputOaSmoothPoint
                | Self::RasterWmInputOaMsRaster
                | Self::RasterWmInputOaMsRasterForced
                | Self::RasterWmInputOaDerefBlock0
                | Self::RasterWmInputOaNoHzOp
                | Self::RasterWmInputOaWmNormalDispatch
                | Self::RasterWmInputOaWmReemitAfterPsExtra
                | Self::RasterWmInputOaOmitHzOp
                | Self::RasterWmInputOaPsDisabled
                | Self::RasterWmInputOaBtCountOne
                | Self::RasterWmInputOaScissorOnly
                | Self::RasterWmInputOaMesaSimpleRect
                | Self::RasterWmInputOaMesaSimpleRectEarly
                | Self::RasterWmInputOaMesaSimpleRectArtificial
                | Self::RasterWmInputOaMesaSimpleRectNoSrcHeader
                | Self::RasterWmInputOaEarlySample
                | Self::RasterWmInputOaEarlyKillOff
                | Self::RasterWmInputOaClipNormal
                | Self::RasterWmInputOaClipPerspective
                | Self::RasterWmInputOaClipDisabled
                | Self::RasterWmInputOaClipDisabledArtificial
                | Self::RasterWmInputOaClipForceMode
                | Self::RasterWmInputOaClipApiD3d
                | Self::RasterWmInputOaClipViewportXy
                | Self::RasterWmInputOaEarlyClipViewportXy
                | Self::RasterWmInputOaEarlyPointWidth1023
                | Self::RasterWmInputOaEarlyMsRasterForced
                | Self::RasterWmInputOaSbeBeforeClip
                | Self::RasterWmInputOaSbeBeforeSf
                | Self::RasterWmInputOaSbeRead0
                | Self::RasterWmInputOaDrawRectEarlyOnly
                | Self::RasterWmInputOaSampleMaskEarlyOnly
                | Self::RasterWmInputOaPipeControlClipSf
                | Self::RasterWmInputOaWmHzOpBeforeWm
                | Self::RasterWmInputOaWmHzOpAfterPsExtra
                | Self::RasterWmInputOaPayloadAttributeEnable
                | Self::RasterWmInputOaPayloadSourceDepthW
                | Self::RasterWmInputOaPayloadBaryPlanes
                | Self::RasterWmInputOaFrontCcw
                | Self::RasterWmInputOaNoPrimitiveReplication
                | Self::RasterWmInputOaVfGeometryDistribution
                | Self::RasterWmInputOaPointWidth8
                | Self::RasterWmInputOaPointWidth8ClipMax
                | Self::RasterWmInputOaPointWidth64
                | Self::RasterWmInputOaPointWidth64SurfaceHalign128
                | Self::RasterWmInputOaPointWidth64ClipMax
                | Self::RasterWmInputOaPointWidth64Early
                | Self::RasterWmInputOaPointWidth64EarlyScissor
                | Self::RasterWmInputOaPointWidth64Screen
                | Self::RasterWmInputOaPointWidth64Artificial
                | Self::RasterWmInputOaPointWidth64WmNormalDispatch
                | Self::RasterWmInputOaPointWidth64WmReemitAfterPsExtra
                | Self::RasterWmInputOaPointWidth64OmitHzOp
                | Self::RasterWmInputOaPointWidth64PsDisabled
                | Self::RasterWmInputOaPointWidth64PayloadAttributeEnable
                | Self::RasterWmInputOaPointWidth64PayloadSourceDepthW
                | Self::RasterWmInputOaPointWidth64PayloadBaryPlanes
                | Self::RasterWmInputOaPointWidth64SbeBeforeClip
                | Self::RasterWmInputOaPointWidth64SbeBeforeSf
                | Self::RasterWmInputOaPointWidth1023
                | Self::RasterWmInputOaPointWidth1023NoWmPoint
                | Self::RasterWmInputOaPointWidth1023Scissor
                | Self::RasterWmInputOaPointWidthVertex
                | Self::RasterWmInputOaHammer
                | Self::RasterWmInputOaScreenHammer
        )
    }

    fn force_kill_pixel_off(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaKillOff
                | Self::RasterWmInputOaEarlyKillOff
                | Self::RasterWmInputOaHammer
                | Self::RasterWmInputOaScreenHammer
        )
    }

    fn surface_halign_raw(self, _device_id: u16) -> u32 {
        if matches!(
            self,
            Self::RasterWmInputOaSurfaceHalign128
                | Self::RasterWmInputOaPointWidth64SurfaceHalign128
        ) {
            SURFACE_HALIGN_128_GFX125
        } else {
            SURFACE_HALIGN_4
        }
    }

    fn smooth_point_raster(self) -> bool {
        matches!(self, Self::RasterWmInputOaSmoothPoint)
    }

    fn dx_multisample_raster(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaMsRaster
                | Self::RasterWmInputOaMsRasterForced
                | Self::RasterWmInputOaEarlyMsRasterForced
                | Self::RasterWmInputOaHammer
                | Self::RasterWmInputOaScreenHammer
        )
    }

    fn force_multisample_raster(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaMsRasterForced
                | Self::RasterWmInputOaEarlyMsRasterForced
                | Self::RasterWmInputOaHammer
                | Self::RasterWmInputOaScreenHammer
        )
    }

    fn forced_raster_sample_count(self) -> u32 {
        if self.raster_hammer() { 1 } else { 0 }
    }

    fn multisample_dw1(self) -> u32 {
        if self.raster_hammer() { 1 } else { 0 }
    }

    fn sf_deref_block_zero(self) -> bool {
        matches!(self, Self::RasterWmInputOaDerefBlock0)
    }

    fn suppress_wm_hz_op_sample_mask(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaNoHzOp
                | Self::RasterWmInputOaMesaSimpleRect
                | Self::RasterWmInputOaMesaSimpleRectEarly
                | Self::RasterWmInputOaMesaSimpleRectArtificial
                | Self::RasterWmInputOaMesaSimpleRectNoSrcHeader
        )
    }

    fn omit_wm_hz_op(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaNoHzOp
                | Self::RasterWmInputOaOmitHzOp
                | Self::RasterWmInputOaPointWidth64OmitHzOp
        )
    }

    fn suppress_forced_wm_thread_dispatch(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaWmNormalDispatch
                | Self::RasterWmInputOaPointWidth64WmNormalDispatch
        )
    }

    fn reemit_wm_after_ps_extra(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaWmReemitAfterPsExtra
                | Self::RasterWmInputOaPointWidth64WmReemitAfterPsExtra
        )
    }

    fn disable_ps_contract(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaPsDisabled | Self::RasterWmInputOaPointWidth64PsDisabled
        )
    }

    fn keep_ps_binding_table_count(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaBtCountOne
                | Self::RasterWmInputOaHammer
                | Self::RasterWmInputOaScreenHammer
        )
    }

    fn mesa_simple_rect_stack(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaMesaSimpleRect
                | Self::RasterWmInputOaMesaSimpleRectEarly
                | Self::RasterWmInputOaMesaSimpleRectArtificial
                | Self::RasterWmInputOaMesaSimpleRectNoSrcHeader
        )
    }

    fn mesa_simple_rect_no_src_header(self) -> bool {
        matches!(self, Self::RasterWmInputOaMesaSimpleRectNoSrcHeader)
    }

    fn early_sample_state(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaEarlySample
                | Self::RasterWmInputOaEarlyKillOff
                | Self::RasterWmInputOaEarlyClipViewportXy
                | Self::RasterWmInputOaEarlyPointWidth1023
                | Self::RasterWmInputOaEarlyMsRasterForced
                | Self::RasterWmInputOaPointWidth64Early
                | Self::RasterWmInputOaPointWidth64EarlyScissor
                | Self::RasterWmInputOaMesaSimpleRectEarly
                | Self::RasterWmInputOaHammer
                | Self::RasterWmInputOaScreenHammer
        )
    }

    fn early_sample_mask_only(self) -> bool {
        matches!(self, Self::RasterWmInputOaSampleMaskEarlyOnly)
    }

    fn early_draw_rect_only(self) -> bool {
        matches!(self, Self::RasterWmInputOaDrawRectEarlyOnly)
    }

    fn sample_mask_before_clip(self) -> bool {
        self.early_sample_state() || self.early_sample_mask_only()
    }

    fn draw_rect_before_clip(self) -> bool {
        self.early_sample_state() || self.early_draw_rect_only()
    }

    fn enable_raster_scissor(self) -> bool {
        self.early_sample_state()
            || matches!(
                self,
                Self::RasterWmInputOaPointWidth1023Scissor
                    | Self::RasterWmInputOaScissorOnly
                    | Self::RasterWmInputOaPointWidth64EarlyScissor
                    | Self::RasterWmInputOaHammer
                    | Self::RasterWmInputOaScreenHammer
            )
    }

    fn raster_hammer(self) -> bool {
        matches!(self, Self::RasterWmInputOaHammer | Self::RasterWmInputOaScreenHammer)
    }

    fn clip_accept_all(self) -> bool {
        !matches!(self, Self::RasterWmInputOaClipNormal)
    }

    fn enable_perspective_divide(self) -> bool {
        matches!(self, Self::RasterWmInputOaClipPerspective)
    }

    fn disable_clip_unit(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaClipDisabled | Self::RasterWmInputOaClipDisabledArtificial
        )
    }

    fn force_clip_mode(self) -> bool {
        matches!(self, Self::RasterWmInputOaClipForceMode)
    }

    fn clip_api_d3d(self) -> bool {
        matches!(self, Self::RasterWmInputOaClipApiD3d)
    }

    fn enable_viewport_xy_clip(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaClipViewportXy | Self::RasterWmInputOaEarlyClipViewportXy
        )
    }

    fn sbe_before_clip(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaSbeBeforeClip | Self::RasterWmInputOaPointWidth64SbeBeforeClip
        )
    }

    fn sbe_before_sf(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaSbeBeforeSf | Self::RasterWmInputOaPointWidth64SbeBeforeSf
        )
    }

    fn force_sbe_read0(self) -> bool {
        matches!(self, Self::RasterWmInputOaSbeRead0)
    }

    fn pipe_control_between_clip_sf(self) -> bool {
        matches!(self, Self::RasterWmInputOaPipeControlClipSf)
    }

    fn wm_hz_op_before_wm(self) -> bool {
        matches!(self, Self::RasterWmInputOaWmHzOpBeforeWm)
    }

    fn wm_hz_op_after_ps_extra(self) -> bool {
        matches!(self, Self::RasterWmInputOaWmHzOpAfterPsExtra)
    }

    fn force_ps_attribute_payload(self) -> bool {
        matches!(
            self,
            Self::PsPayloadAttributeEnable
                | Self::RasterWmInputOaPayloadAttributeEnable
                | Self::RasterWmInputOaPayloadSourceDepthW
                | Self::RasterWmInputOaPayloadBaryPlanes
                | Self::RasterWmInputOaPointWidth64PayloadAttributeEnable
                | Self::RasterWmInputOaPointWidth64PayloadSourceDepthW
                | Self::RasterWmInputOaPointWidth64PayloadBaryPlanes
        )
    }

    fn force_ps_source_depth_w(self) -> bool {
        matches!(
            self,
            Self::PsPayloadSourceDepthW
                | Self::RasterWmInputOaPayloadSourceDepthW
                | Self::RasterWmInputOaPointWidth64PayloadSourceDepthW
        )
    }

    fn force_ps_bary_planes(self) -> bool {
        matches!(
            self,
            Self::PsPayloadBaryPlanes
                | Self::RasterWmInputOaPayloadBaryPlanes
                | Self::RasterWmInputOaPointWidth64PayloadBaryPlanes
        )
    }

    fn force_one_sbe_attribute(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaPayloadAttributeEnable
                | Self::RasterWmInputOaPayloadSourceDepthW
                | Self::RasterWmInputOaPayloadBaryPlanes
                | Self::RasterWmInputOaPointWidth64PayloadAttributeEnable
                | Self::RasterWmInputOaPointWidth64PayloadSourceDepthW
                | Self::RasterWmInputOaPointWidth64PayloadBaryPlanes
        )
    }

    fn front_ccw(self) -> bool {
        matches!(self, Self::RasterWmInputOaFrontCcw)
    }

    fn disable_primitive_replication(self) -> bool {
        matches!(self, Self::RasterWmInputOaNoPrimitiveReplication)
    }

    fn force_vf_geometry_distribution(self) -> bool {
        matches!(self, Self::RasterWmInputOaVfGeometryDistribution)
    }

    fn suppress_wm_point_rule(self) -> bool {
        matches!(self, Self::RasterWmInputOaPointWidth1023NoWmPoint)
    }

    fn point_width_raw_override(self) -> Option<u32> {
        match self {
            Self::RasterWmInputOaPointWidth8 | Self::RasterWmInputOaPointWidth8ClipMax => {
                Some(0x008)
            }
            Self::RasterWmInputOaPointWidth64
            | Self::RasterWmInputOaPointWidth64ClipMax
            | Self::RasterWmInputOaPointWidth64Early
            | Self::RasterWmInputOaPointWidth64EarlyScissor
            | Self::RasterWmInputOaPointWidth64Screen
            | Self::RasterWmInputOaPointWidth64Artificial
            | Self::RasterWmInputOaPointWidth64WmNormalDispatch
            | Self::RasterWmInputOaPointWidth64WmReemitAfterPsExtra
            | Self::RasterWmInputOaPointWidth64OmitHzOp
            | Self::RasterWmInputOaPointWidth64PsDisabled
            | Self::RasterWmInputOaPointWidth64PayloadAttributeEnable
            | Self::RasterWmInputOaPointWidth64PayloadSourceDepthW
            | Self::RasterWmInputOaPointWidth64PayloadBaryPlanes
            | Self::RasterWmInputOaPointWidth64SbeBeforeClip
            | Self::RasterWmInputOaPointWidth64SbeBeforeSf => Some(0x200),
            Self::RasterWmInputOaPointWidth1023
            | Self::RasterWmInputOaPointWidth1023NoWmPoint
            | Self::RasterWmInputOaPointWidth1023Scissor
            | Self::RasterWmInputOaEarlyPointWidth1023
            | Self::RasterWmInputOaHammer
            | Self::RasterWmInputOaScreenHammer => Some(0x3FF),
            _ => None,
        }
    }

    fn clip_max_point_width_raw_override(self) -> Option<u32> {
        match self {
            Self::RasterWmInputOaPointWidth8ClipMax => Some(0x008),
            Self::RasterWmInputOaPointWidth64ClipMax => Some(0x200),
            Self::RasterWmInputOaHammer | Self::RasterWmInputOaScreenHammer => Some(0x3FF),
            _ => None,
        }
    }

    fn point_width_from_vertex(self) -> bool {
        matches!(self, Self::RasterWmInputOaPointWidthVertex)
    }

    fn disable_sf_viewport_transform(self) -> bool {
        matches!(self, Self::RasterWmInputOaPointWidth64Screen | Self::RasterWmInputOaScreenHammer)
    }

    fn artificial_fragment_markers(self) -> bool {
        matches!(
            self,
            Self::RasterWmInputOaMesaSimpleRectArtificial
                | Self::RasterWmInputOaClipDisabledArtificial
                | Self::RasterWmInputOaPointWidth64Artificial
        )
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum VfPrimitiveGeometry {
    Canonical,
    Oversized,
    CenterPoint,
    ScreenSpacePoint8x8,
    ScreenSpaceLine8x8,
    ScreenSpace8x8,
    ScreenSpaceRect8x8,
    ScreenSpaceRect8x8OrderB,
    ScreenSpaceRect8x8OrderC,
    NdcTriangleLarge,
    NdcTriangleLargeCw,
    NdcLine,
    NdcRect,
    NdcRectCw,
    NdcRectAlt,
    NdcRectUrLrUl,
    NdcRectSmall,
}

impl VfPrimitiveGeometry {
    fn label(self) -> &'static str {
        match self {
            Self::Canonical => "canonical",
            Self::Oversized => "oversized",
            Self::CenterPoint => "center-point",
            Self::ScreenSpacePoint8x8 => "screen-space-point-8x8",
            Self::ScreenSpaceLine8x8 => "screen-space-line-8x8",
            Self::ScreenSpace8x8 => "screen-space-8x8",
            Self::ScreenSpaceRect8x8 => "screen-space-rect-8x8",
            Self::ScreenSpaceRect8x8OrderB => "screen-space-rect-8x8-order-b",
            Self::ScreenSpaceRect8x8OrderC => "screen-space-rect-8x8-order-c",
            Self::NdcTriangleLarge => "ndc-triangle-large",
            Self::NdcTriangleLargeCw => "ndc-triangle-large-cw",
            Self::NdcLine => "ndc-line",
            Self::NdcRect => "ndc-rect",
            Self::NdcRectCw => "ndc-rect-cw",
            Self::NdcRectAlt => "ndc-rect-alt",
            Self::NdcRectUrLrUl => "ndc-rect-ur-lr-ul",
            Self::NdcRectSmall => "ndc-rect-small",
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
            // Three coincident POINTLIST vertices.  The draw mode, not the
            // vertex upload, decides that these are point primitives.
            Self::CenterPoint => [[0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 0.0]],
            Self::ScreenSpacePoint8x8 => [[4.0, 4.0, 0.0], [4.0, 4.0, 0.0], [4.0, 4.0, 0.0]],
            Self::ScreenSpaceLine8x8 => [[0.5, 0.5, 0.0], [7.5, 7.5, 0.0], [0.5, 7.5, 0.0]],
            // Diagnostic-only screen-space-ish coordinates for a scratch RT
            // with SF viewport transform disabled.
            Self::ScreenSpace8x8 | Self::ScreenSpaceRect8x8 => {
                [[0.5, 0.5, 0.0], [7.5, 0.5, 0.0], [0.5, 7.5, 0.0]]
            }
            Self::ScreenSpaceRect8x8OrderB => [[0.5, 0.5, 0.0], [0.5, 7.5, 0.0], [7.5, 0.5, 0.0]],
            Self::ScreenSpaceRect8x8OrderC => [[7.5, 7.5, 0.0], [7.5, 0.5, 0.0], [0.5, 7.5, 0.0]],
            Self::NdcTriangleLarge => [[-0.75, -0.75, 0.0], [0.75, -0.75, 0.0], [-0.75, 0.75, 0.0]],
            Self::NdcTriangleLargeCw => {
                [[-0.75, -0.75, 0.0], [-0.75, 0.75, 0.0], [0.75, -0.75, 0.0]]
            }
            Self::NdcLine => [[-0.75, -0.75, 0.0], [0.75, 0.75, 0.0], [-0.75, 0.75, 0.0]],
            Self::NdcRect => [[-0.75, -0.75, 0.0], [0.75, -0.75, 0.0], [-0.75, 0.75, 0.0]],
            Self::NdcRectCw => [[-0.75, -0.75, 0.0], [-0.75, 0.75, 0.0], [0.75, -0.75, 0.0]],
            Self::NdcRectAlt => [[0.75, 0.75, 0.0], [-0.75, 0.75, 0.0], [-0.75, -0.75, 0.0]],
            Self::NdcRectUrLrUl => [[0.75, 0.75, 0.0], [0.75, -0.75, 0.0], [-0.75, 0.75, 0.0]],
            Self::NdcRectSmall => [[-0.20, -0.20, 0.0], [0.20, -0.20, 0.0], [-0.20, 0.20, 0.0]],
        }
    }

    fn fullscreen_candidate(self) -> bool {
        matches!(self, Self::Oversized)
    }

    fn point_candidate(self) -> bool {
        matches!(self, Self::CenterPoint | Self::ScreenSpacePoint8x8)
    }

    fn screen_space_candidate(self) -> bool {
        matches!(
            self,
            Self::ScreenSpacePoint8x8
                | Self::ScreenSpaceLine8x8
                | Self::ScreenSpace8x8
                | Self::ScreenSpaceRect8x8
                | Self::ScreenSpaceRect8x8OrderB
                | Self::ScreenSpaceRect8x8OrderC
        )
    }

    fn rect_candidate(self) -> bool {
        matches!(
            self,
            Self::ScreenSpaceRect8x8
                | Self::ScreenSpaceRect8x8OrderB
                | Self::ScreenSpaceRect8x8OrderC
                | Self::NdcRect
                | Self::NdcRectCw
                | Self::NdcRectAlt
                | Self::NdcRectUrLrUl
                | Self::NdcRectSmall
        )
    }

    fn line_candidate(self) -> bool {
        matches!(self, Self::ScreenSpaceLine8x8 | Self::NdcLine)
    }

    fn ndc_rect_candidate(self) -> bool {
        matches!(
            self,
            Self::NdcRect
                | Self::NdcRectCw
                | Self::NdcRectAlt
                | Self::NdcRectUrLrUl
                | Self::NdcRectSmall
        )
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum StreamoutProofExperiment {
    PositionSlot0,
    PositionSlot1,
    HeaderAndPositionSlots01,
    PointSizeSlot0PositionSlot1,
}

const CMD_3DSTATE_VERTEX_ELEMENTS_2: u32 = 3 | (9 << 16) | (3 << 27) | (3 << 29);

impl StreamoutProofExperiment {
    fn label(self) -> &'static str {
        match self {
            Self::PositionSlot0 => "pos-slot0",
            Self::PositionSlot1 => "pos-slot1",
            Self::HeaderAndPositionSlots01 => "header+pos-slots01",
            Self::PointSizeSlot0PositionSlot1 => "point-size-slot0+pos-slot1",
        }
    }

    fn alternate(self) -> Self {
        match self {
            Self::PositionSlot0 => Self::PositionSlot1,
            Self::PositionSlot1 => Self::HeaderAndPositionSlots01,
            Self::HeaderAndPositionSlots01 => Self::PositionSlot0,
            Self::PointSizeSlot0PositionSlot1 => Self::PositionSlot1,
        }
    }

    fn vertex_bytes(self) -> usize {
        match self {
            Self::PositionSlot0 | Self::PositionSlot1 => 16,
            Self::HeaderAndPositionSlots01 | Self::PointSizeSlot0PositionSlot1 => 32,
        }
    }

    fn vertex_read_length(self) -> u32 {
        1
    }

    fn so_decl_header(self) -> u32 {
        match self {
            Self::PositionSlot0 | Self::PositionSlot1 => {
                3 | (23 << 16) | (1 << 24) | (3 << 27) | (3 << 29)
            }
            Self::HeaderAndPositionSlots01 | Self::PointSizeSlot0PositionSlot1 => {
                5 | (23 << 16) | (1 << 24) | (3 << 27) | (3 << 29)
            }
        }
    }

    fn so_decl_buffer_selects(self) -> u32 {
        1
    }

    fn so_decl_num_entries(self) -> u32 {
        match self {
            Self::PositionSlot0 | Self::PositionSlot1 => 1,
            Self::HeaderAndPositionSlots01 | Self::PointSizeSlot0PositionSlot1 => 2,
        }
    }

    fn so_decl_entry_dwords(self) -> [u32; 4] {
        match self {
            Self::PositionSlot0 => [0x0000_000F, 0x0000_0000, 0x0000_0000, 0x0000_0000],
            Self::PositionSlot1 => [0x0000_001F, 0x0000_0000, 0x0000_0000, 0x0000_0000],
            Self::HeaderAndPositionSlots01 => [0x0000_000F, 0x0000_0000, 0x0000_001F, 0x0000_0000],
            Self::PointSizeSlot0PositionSlot1 => {
                [0x0000_000F, 0x0000_0000, 0x0000_001F, 0x0000_0000]
            }
        }
    }

    fn compatible(self) -> bool {
        true
    }

    fn vf_slot_contract(self) -> &'static str {
        match self {
            Self::PositionSlot0 => "slot0=position",
            Self::PositionSlot1 => "slot0=zero slot1=position",
            Self::HeaderAndPositionSlots01 => "slot0=header slot1=position",
            Self::PointSizeSlot0PositionSlot1 => "slot0=point-size slot1=position",
        }
    }

    fn vf_vertex_element_count(self) -> usize {
        match self {
            Self::PositionSlot0 => 1,
            Self::PositionSlot1
            | Self::HeaderAndPositionSlots01
            | Self::PointSizeSlot0PositionSlot1 => 2,
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
            Self::Draw | Self::DrawScreenSpace | Self::VfDraw => TRIANGLE_TOPOLOGY_TRILIST,
            Self::VfLineDraw => TRIANGLE_TOPOLOGY_LINELIST,
            Self::DrawScreenSpaceRect | Self::VfRectDraw | Self::VfRectClipDraw => {
                TRIANGLE_TOPOLOGY_RECTLIST
            }
            Self::VfPointDraw => TRIANGLE_TOPOLOGY_POINTLIST,
            Self::StreamoutProof | Self::VfStreamoutProof | Self::VsStreamoutProof => {
                TRIANGLE_TOPOLOGY_POINTLIST
            }
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Draw => "draw",
            Self::DrawScreenSpace => "draw-screen-space",
            Self::DrawScreenSpaceRect => "draw-screen-space-rect",
            Self::VfDraw => "vf-draw",
            Self::VfPointDraw => "vf-point-draw",
            Self::VfLineDraw => "vf-line-draw",
            Self::VfRectDraw => "vf-rect-draw",
            Self::VfRectClipDraw => "vf-rect-clip-draw",
            Self::StreamoutProof => "streamout-proof",
            Self::VfStreamoutProof => "vf-streamout-proof",
            Self::VsStreamoutProof => "vs-streamout-proof",
        }
    }

    fn vf_synthesized_vue(self) -> bool {
        matches!(
            self,
            Self::VfDraw
                | Self::VfPointDraw
                | Self::VfLineDraw
                | Self::VfRectDraw
                | Self::VfRectClipDraw
        )
    }

    fn point_raster(self) -> bool {
        matches!(self, Self::VfPointDraw)
    }

    fn screen_space_raster(self) -> bool {
        matches!(self, Self::DrawScreenSpace | Self::DrawScreenSpaceRect | Self::VfRectDraw)
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

fn fragment_target_variant_base(submit_name: &str) -> Option<&str> {
    submit_name.strip_suffix("-rt32")
}

fn is_vs_draw_frontier_scratch_submit_name(submit_name: &str) -> bool {
    submit_name == "vs-draw-frontier-scratch"
        || submit_name.starts_with("vs-draw-frontier-scratch-")
}

fn is_scratch_rt_submit_name(submit_name: &str) -> bool {
    if let Some(base) = fragment_target_variant_base(submit_name) {
        return is_scratch_rt_submit_name(base);
    }
    if is_vs_draw_frontier_scratch_submit_name(submit_name) {
        return true;
    }
    matches!(
        submit_name,
        "ps-bt0-scratch-rt"
            | "raster-wm-oa-probe"
            | "point-vf-giant-scratch"
            | "point-vf-giant-oa"
            | "point-vf-giant-oa-pos0"
            | "point-vf-giant-oa-header"
            | "point-vf-giant-oa-killoff"
            | "point-vf-giant-oa-smooth"
            | "point-vf-giant-oa-msrast"
            | "point-vf-giant-oa-msrast-force"
            | "point-vf-giant-oa-deref0"
            | "point-vf-giant-oa-hz0"
            | "point-vf-giant-oa-wm-normal"
            | "point-vf-giant-oa-wm-reemit"
            | "point-vf-giant-oa-hz-omit"
            | "point-vf-giant-oa-ps-off"
            | "point-vf-giant-oa-bt1"
            | "point-vf-giant-oa-early"
            | "point-vf-giant-oa-early-killoff"
            | "point-vf-giant-oa-clip-normal"
            | "point-vf-giant-oa-clip-persp"
            | "point-vf-giant-oa-clip-disable"
            | "point-vf-giant-oa-clip-disable-arm"
            | "point-vf-giant-oa-clip-force"
            | "point-vf-giant-oa-clip-d3d"
            | "point-vf-giant-oa-clip-xy"
            | "point-vf-giant-oa-sbe0"
            | "point-vf-giant-oa-sbe-pre-clip"
            | "point-vf-giant-oa-sbe-pre-sf"
            | "point-vf-giant-oa-no-pr"
            | "point-vf-giant-oa-vfg"
            | "point-vf-giant-oa-w8"
            | "point-vf-giant-oa-w8-clipmax"
            | "point-vf-giant-oa-w64"
            | "point-vf-giant-oa-w64-halign128"
            | "point-vf-giant-oa-w64-clipmax"
            | "point-vf-giant-oa-w64-wm-normal"
            | "point-vf-giant-oa-w64-wm-reemit"
            | "point-vf-giant-oa-w64-hz-omit"
            | "point-vf-giant-oa-w64-ps-off"
            | "point-vf-giant-oa-w64-payload-attr"
            | "point-vf-giant-oa-w64-payload-depthw"
            | "point-vf-giant-oa-w64-payload-bary"
            | "point-vf-giant-oa-w64-sbe-pre-clip"
            | "point-vf-giant-oa-w64-sbe-pre-sf"
            | "point-vf-giant-oa-w64-early"
            | "point-vf-giant-oa-w64-early-scissor"
            | "point-vf-screen-oa-w64"
            | "point-vf-giant-oa-w64-arm"
            | "point-vf-giant-oa-w1023"
            | "point-vf-giant-oa-w1023-nowmpoint"
            | "point-vf-giant-oa-w1023-scissor"
            | "point-vf-giant-oa-vtxw"
            | "point-vf-giant-oa-early-w1023"
            | "point-vf-giant-oa-early-msrast-force"
            | "screen-vs-scratch"
            | "screen-vs-oa"
            | "screen-vs-ndc-oa"
            | "screen-vs-ndc-oa-hz0"
            | "screen-vs-sbe0"
            | "screen-vs-slot0-oa"
            | "screen-vs-urb2-oa"
            | "screen-vs-urb2-slot0-oa"
            | "vf-rect-oa"
            | "vf-rect-oa-pos0"
            | "vf-rect-oa-header"
            | "vf-rect-oa-deref0"
            | "vf-rect-ndc-oa"
            | "vf-rect-ndc-oa-halign128"
            | "vf-rect-ndc-oa-sbe-pre-clip"
            | "vf-rect-ndc-oa-sbe-pre-sf"
            | "vf-rect-ndc-oa-drawrect-early"
            | "vf-rect-ndc-oa-sample-early"
            | "vf-rect-ndc-oa-pc-clip-sf"
            | "vf-rect-ndc-oa-hz-pre-wm"
            | "vf-rect-ndc-oa-hz-post-extra"
            | "vf-rect-ndc-oa-payload-attr"
            | "vf-rect-ndc-oa-payload-depthw"
            | "vf-rect-ndc-oa-payload-bary"
            | "vf-rect-ndc-oa-persp"
            | "vf-rect-ndc-oa-clipxy"
            | "vf-rect-ndc-oa-clip-disable"
            | "vf-rect-ndc-oa-clip-force"
            | "vf-rect-ndc-oa-clip-d3d"
            | "vf-rect-ndc-oa-early-clipxy"
            | "vf-rect-ndc-oa-frontccw"
            | "vf-rect-ndc-oa-hz0"
            | "vf-rect-ndc-oa-early"
            | "vf-rect-ndc-oa-bt1"
            | "vf-rect-ndc-order-b-oa"
            | "vf-rect-ndc-order-c-oa"
            | "vf-rect-ndc-order-c-early-oa"
            | "vf-rect-ndc-order-c-clip-disable-oa"
            | "vf-rect-ndc-mesa-simple-oa"
            | "vf-rect-ndc-mesa-nosrc-header-oa"
            | "vf-rect-ndc-small-oa"
            | "vf-rect-ndc-cw-oa"
            | "vf-rect-ndc-alt-oa"
            | "vf-rect-order-b-oa"
            | "vf-rect-order-b-early-oa"
            | "vf-rect-order-b-scissor-oa"
            | "vf-rect-mesa-simple-oa"
            | "vf-rect-mesa-simple-oa-early"
            | "vf-rect-mesa-simple-oa-arm"
            | "vf-rect-mesa-nosrc-header-oa"
            | "vf-rect-order-c-oa"
            | "vf-tri-ndc-oa"
            | "vf-tri-ndc-oa-early"
            | "vf-tri-ndc-oa-early-clipxy"
            | "vf-tri-ndc-cw-oa-early"
            | "screen-rect-scratch"
            | "screen-rect-oa-early"
    )
}

fn is_raster_wm_oa_submit_name(submit_name: &str) -> bool {
    if let Some(base) = fragment_target_variant_base(submit_name) {
        return is_raster_wm_oa_submit_name(base);
    }
    matches!(
        submit_name,
        "raster-wm-oa-probe"
            | "point-vf-giant-oa"
            | "point-vf-giant-oa-pos0"
            | "point-vf-giant-oa-header"
            | "point-vf-giant-oa-killoff"
            | "point-vf-giant-oa-smooth"
            | "point-vf-giant-oa-msrast"
            | "point-vf-giant-oa-msrast-force"
            | "point-vf-giant-oa-deref0"
            | "point-vf-giant-oa-hz0"
            | "point-vf-giant-oa-wm-normal"
            | "point-vf-giant-oa-wm-reemit"
            | "point-vf-giant-oa-hz-omit"
            | "point-vf-giant-oa-ps-off"
            | "point-vf-giant-oa-bt1"
            | "point-vf-giant-oa-early"
            | "point-vf-giant-oa-early-killoff"
            | "point-vf-giant-oa-clip-normal"
            | "point-vf-giant-oa-clip-persp"
            | "point-vf-giant-oa-clip-disable"
            | "point-vf-giant-oa-clip-disable-arm"
            | "point-vf-giant-oa-clip-force"
            | "point-vf-giant-oa-clip-d3d"
            | "point-vf-giant-oa-clip-xy"
            | "point-vf-giant-oa-sbe0"
            | "point-vf-giant-oa-sbe-pre-clip"
            | "point-vf-giant-oa-sbe-pre-sf"
            | "point-vf-giant-oa-no-pr"
            | "point-vf-giant-oa-vfg"
            | "point-vf-giant-oa-w8"
            | "point-vf-giant-oa-w8-clipmax"
            | "point-vf-giant-oa-w64"
            | "point-vf-giant-oa-w64-halign128"
            | "point-vf-giant-oa-w64-clipmax"
            | "point-vf-giant-oa-w64-wm-normal"
            | "point-vf-giant-oa-w64-wm-reemit"
            | "point-vf-giant-oa-w64-hz-omit"
            | "point-vf-giant-oa-w64-ps-off"
            | "point-vf-giant-oa-w64-payload-attr"
            | "point-vf-giant-oa-w64-payload-depthw"
            | "point-vf-giant-oa-w64-payload-bary"
            | "point-vf-giant-oa-w64-sbe-pre-clip"
            | "point-vf-giant-oa-w64-sbe-pre-sf"
            | "point-vf-giant-oa-w64-early"
            | "point-vf-giant-oa-w64-early-scissor"
            | "point-vf-screen-oa-w64"
            | "point-vf-giant-oa-w64-arm"
            | "point-vf-giant-oa-w1023"
            | "point-vf-giant-oa-w1023-nowmpoint"
            | "point-vf-giant-oa-w1023-scissor"
            | "point-vf-giant-oa-vtxw"
            | "point-vf-giant-oa-early-w1023"
            | "point-vf-giant-oa-early-msrast-force"
            | "screen-vs-oa"
            | "screen-vs-ndc-oa"
            | "screen-vs-ndc-oa-hz0"
            | "screen-vs-sbe0"
            | "screen-vs-slot0-oa"
            | "screen-vs-urb2-oa"
            | "screen-vs-urb2-slot0-oa"
            | "vf-rect-oa"
            | "vf-rect-oa-pos0"
            | "vf-rect-oa-header"
            | "vf-rect-oa-deref0"
            | "vf-rect-ndc-oa"
            | "vf-rect-ndc-oa-halign128"
            | "vf-rect-ndc-oa-sbe-pre-clip"
            | "vf-rect-ndc-oa-sbe-pre-sf"
            | "vf-rect-ndc-oa-drawrect-early"
            | "vf-rect-ndc-oa-sample-early"
            | "vf-rect-ndc-oa-pc-clip-sf"
            | "vf-rect-ndc-oa-hz-pre-wm"
            | "vf-rect-ndc-oa-hz-post-extra"
            | "vf-rect-ndc-oa-payload-attr"
            | "vf-rect-ndc-oa-payload-depthw"
            | "vf-rect-ndc-oa-payload-bary"
            | "vf-rect-ndc-oa-persp"
            | "vf-rect-ndc-oa-clipxy"
            | "vf-rect-ndc-oa-clip-disable"
            | "vf-rect-ndc-oa-clip-force"
            | "vf-rect-ndc-oa-clip-d3d"
            | "vf-rect-ndc-oa-early-clipxy"
            | "vf-rect-ndc-oa-frontccw"
            | "vf-rect-ndc-oa-hz0"
            | "vf-rect-ndc-oa-early"
            | "vf-rect-ndc-oa-bt1"
            | "vf-rect-ndc-order-b-oa"
            | "vf-rect-ndc-order-c-oa"
            | "vf-rect-ndc-order-c-early-oa"
            | "vf-rect-ndc-order-c-clip-disable-oa"
            | "vf-rect-ndc-mesa-simple-oa"
            | "vf-rect-ndc-mesa-nosrc-header-oa"
            | "vf-rect-ndc-small-oa"
            | "vf-rect-ndc-cw-oa"
            | "vf-rect-ndc-alt-oa"
            | "vf-rect-order-b-oa"
            | "vf-rect-order-b-early-oa"
            | "vf-rect-order-b-scissor-oa"
            | "vf-rect-mesa-simple-oa"
            | "vf-rect-mesa-simple-oa-early"
            | "vf-rect-mesa-simple-oa-arm"
            | "vf-rect-mesa-nosrc-header-oa"
            | "vf-rect-order-c-oa"
            | "vf-tri-ndc-oa"
            | "vf-tri-ndc-oa-early"
            | "vf-tri-ndc-oa-early-clipxy"
            | "vf-tri-ndc-cw-oa-early"
            | "screen-rect-oa-early"
    )
}

fn is_surface_draw_submit_name(submit_name: &str) -> bool {
    if let Some(base) = fragment_target_variant_base(submit_name) {
        return is_surface_draw_submit_name(base);
    }
    if is_vs_draw_frontier_scratch_submit_name(submit_name) {
        return true;
    }
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
            | "ps-eot-big-primitive"
            | "ps-eot-big-primitive-retire"
            | "ps-cps-disabled-big-primitive"
            | "ps-cps-disabled-big-primitive-retire"
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
            | "wm-hz-sample-mask-big-primitive"
            | "wm-hz-sample-mask-big-primitive-retire"
            | "wm-late-reemit-big-primitive"
            | "wm-late-reemit-big-primitive-retire"
            | "wm-late-reemit-vs-big-primitive-retire"
            | "wm-late-reemit-vs-slot0-big-primitive-retire"
            | "wm-late-reemit-vs-urb2-big-primitive-retire"
            | "wm-late-reemit-vs-urb2-slot0-big-primitive-retire"
            | "point-vf-giant"
            | "point-vf-giant-scratch"
            | "point-vf-giant-oa"
            | "point-vf-giant-oa-pos0"
            | "point-vf-giant-oa-header"
            | "point-vf-giant-oa-killoff"
            | "point-vf-giant-oa-smooth"
            | "point-vf-giant-oa-msrast"
            | "point-vf-giant-oa-msrast-force"
            | "point-vf-giant-oa-deref0"
            | "point-vf-giant-oa-hz0"
            | "point-vf-giant-oa-wm-normal"
            | "point-vf-giant-oa-wm-reemit"
            | "point-vf-giant-oa-hz-omit"
            | "point-vf-giant-oa-ps-off"
            | "point-vf-giant-oa-bt1"
            | "point-vf-giant-oa-early"
            | "point-vf-giant-oa-early-killoff"
            | "point-vf-giant-oa-clip-normal"
            | "point-vf-giant-oa-clip-persp"
            | "point-vf-giant-oa-clip-disable"
            | "point-vf-giant-oa-clip-disable-arm"
            | "point-vf-giant-oa-clip-force"
            | "point-vf-giant-oa-clip-d3d"
            | "point-vf-giant-oa-clip-xy"
            | "point-vf-giant-oa-sbe0"
            | "point-vf-giant-oa-sbe-pre-clip"
            | "point-vf-giant-oa-sbe-pre-sf"
            | "point-vf-giant-oa-no-pr"
            | "point-vf-giant-oa-vfg"
            | "point-vf-giant-oa-w8"
            | "point-vf-giant-oa-w8-clipmax"
            | "point-vf-giant-oa-w64"
            | "point-vf-giant-oa-w64-halign128"
            | "point-vf-giant-oa-w64-clipmax"
            | "point-vf-giant-oa-w64-wm-normal"
            | "point-vf-giant-oa-w64-wm-reemit"
            | "point-vf-giant-oa-w64-hz-omit"
            | "point-vf-giant-oa-w64-ps-off"
            | "point-vf-giant-oa-w64-payload-attr"
            | "point-vf-giant-oa-w64-payload-depthw"
            | "point-vf-giant-oa-w64-payload-bary"
            | "point-vf-giant-oa-w64-sbe-pre-clip"
            | "point-vf-giant-oa-w64-sbe-pre-sf"
            | "point-vf-giant-oa-w64-early"
            | "point-vf-giant-oa-w64-early-scissor"
            | "point-vf-screen-oa-w64"
            | "point-vf-giant-oa-w64-arm"
            | "point-vf-giant-oa-w1023"
            | "point-vf-giant-oa-w1023-nowmpoint"
            | "point-vf-giant-oa-w1023-scissor"
            | "point-vf-giant-oa-vtxw"
            | "point-vf-giant-oa-early-w1023"
            | "point-vf-giant-oa-early-msrast-force"
            | "point-vf-giant-bt1"
            | "point-vf-giant-slot0"
            | "screen-vs-scratch"
            | "screen-vs-oa"
            | "screen-vs-ndc-oa"
            | "screen-vs-ndc-oa-hz0"
            | "screen-vs-sbe0"
            | "screen-vs-slot0-oa"
            | "screen-vs-urb2-oa"
            | "screen-vs-urb2-slot0-oa"
            | "vf-rect-oa"
            | "vf-rect-oa-pos0"
            | "vf-rect-oa-header"
            | "vf-rect-oa-deref0"
            | "vf-rect-ndc-oa"
            | "vf-rect-ndc-oa-halign128"
            | "vf-rect-ndc-oa-sbe-pre-clip"
            | "vf-rect-ndc-oa-sbe-pre-sf"
            | "vf-rect-ndc-oa-drawrect-early"
            | "vf-rect-ndc-oa-sample-early"
            | "vf-rect-ndc-oa-pc-clip-sf"
            | "vf-rect-ndc-oa-hz-pre-wm"
            | "vf-rect-ndc-oa-hz-post-extra"
            | "vf-rect-ndc-oa-payload-attr"
            | "vf-rect-ndc-oa-payload-depthw"
            | "vf-rect-ndc-oa-payload-bary"
            | "vf-rect-ndc-oa-persp"
            | "vf-rect-ndc-oa-clipxy"
            | "vf-rect-ndc-oa-clip-disable"
            | "vf-rect-ndc-oa-clip-force"
            | "vf-rect-ndc-oa-clip-d3d"
            | "vf-rect-ndc-oa-early-clipxy"
            | "vf-rect-ndc-oa-frontccw"
            | "vf-rect-ndc-oa-hz0"
            | "vf-rect-ndc-oa-early"
            | "vf-rect-ndc-oa-bt1"
            | "vf-rect-ndc-order-b-oa"
            | "vf-rect-ndc-order-c-oa"
            | "vf-rect-ndc-order-c-early-oa"
            | "vf-rect-ndc-order-c-clip-disable-oa"
            | "vf-rect-ndc-mesa-simple-oa"
            | "vf-rect-ndc-mesa-nosrc-header-oa"
            | "vf-rect-ndc-small-oa"
            | "vf-rect-ndc-cw-oa"
            | "vf-rect-ndc-alt-oa"
            | "vf-rect-order-b-oa"
            | "vf-rect-order-b-early-oa"
            | "vf-rect-order-b-scissor-oa"
            | "vf-rect-mesa-simple-oa"
            | "vf-rect-mesa-simple-oa-early"
            | "vf-rect-mesa-simple-oa-arm"
            | "vf-rect-mesa-nosrc-header-oa"
            | "vf-rect-order-c-oa"
            | "vf-tri-ndc-oa"
            | "vf-tri-ndc-oa-early"
            | "vf-tri-ndc-oa-early-clipxy"
            | "vf-tri-ndc-cw-oa-early"
            | "screen-rect-scratch"
            | "screen-rect-oa-early"
            | "postdraw-light-only-retire"
            | "postdraw-flush-bit5"
            | "postdraw-flush-bit7"
            | "postdraw-flush-bit12"
            | "postdraw-flush-bit20"
            | "postdraw-flush-bit26"
            | "postdraw-pc-postsync-no-cs"
            | "postdraw-pc-cs-no-postsync"
            | "raster-wm-oa-probe"
            | "vs-draw-frontier"
    )
}

fn is_fragment_candidate_submit_name(submit_name: &str) -> bool {
    if let Some(base) = fragment_target_variant_base(submit_name) {
        return is_fragment_candidate_submit_name(base);
    }
    if is_vs_draw_frontier_scratch_submit_name(submit_name) {
        return true;
    }
    matches!(
        submit_name,
        "ps-launch-big-primitive"
            | "ps-bt0-scratch-rt"
            | "raster-wm-oa-probe"
            | "ps-bt1-big-primitive"
            | "ps-wm-normal-big-primitive"
            | "ps-dispatch-slot0-big-primitive"
            | "ps-dispatch-slot1-big-primitive"
            | "ps-dispatch-slot2-big-primitive"
            | "ps-eot-big-primitive"
            | "ps-eot-big-primitive-retire"
            | "ps-cps-disabled-big-primitive"
            | "ps-cps-disabled-big-primitive-retire"
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
            | "wm-hz-sample-mask-big-primitive"
            | "wm-hz-sample-mask-big-primitive-retire"
            | "wm-late-reemit-big-primitive"
            | "wm-late-reemit-big-primitive-retire"
            | "wm-late-reemit-vs-big-primitive-retire"
            | "wm-late-reemit-vs-slot0-big-primitive-retire"
            | "wm-late-reemit-vs-urb2-big-primitive-retire"
            | "wm-late-reemit-vs-urb2-slot0-big-primitive-retire"
            | "point-vf-giant"
            | "point-vf-giant-scratch"
            | "point-vf-giant-oa"
            | "point-vf-giant-oa-pos0"
            | "point-vf-giant-oa-header"
            | "point-vf-giant-oa-killoff"
            | "point-vf-giant-oa-smooth"
            | "point-vf-giant-oa-msrast"
            | "point-vf-giant-oa-msrast-force"
            | "point-vf-giant-oa-deref0"
            | "point-vf-giant-oa-hz0"
            | "point-vf-giant-oa-wm-normal"
            | "point-vf-giant-oa-wm-reemit"
            | "point-vf-giant-oa-hz-omit"
            | "point-vf-giant-oa-ps-off"
            | "point-vf-giant-oa-bt1"
            | "point-vf-giant-oa-early"
            | "point-vf-giant-oa-early-killoff"
            | "point-vf-giant-oa-clip-normal"
            | "point-vf-giant-oa-clip-persp"
            | "point-vf-giant-oa-clip-disable"
            | "point-vf-giant-oa-clip-disable-arm"
            | "point-vf-giant-oa-clip-force"
            | "point-vf-giant-oa-clip-d3d"
            | "point-vf-giant-oa-clip-xy"
            | "point-vf-giant-oa-sbe0"
            | "point-vf-giant-oa-sbe-pre-clip"
            | "point-vf-giant-oa-sbe-pre-sf"
            | "point-vf-giant-oa-no-pr"
            | "point-vf-giant-oa-vfg"
            | "point-vf-giant-oa-w8"
            | "point-vf-giant-oa-w8-clipmax"
            | "point-vf-giant-oa-w64"
            | "point-vf-giant-oa-w64-halign128"
            | "point-vf-giant-oa-w64-clipmax"
            | "point-vf-giant-oa-w64-wm-normal"
            | "point-vf-giant-oa-w64-wm-reemit"
            | "point-vf-giant-oa-w64-hz-omit"
            | "point-vf-giant-oa-w64-ps-off"
            | "point-vf-giant-oa-w64-payload-attr"
            | "point-vf-giant-oa-w64-payload-depthw"
            | "point-vf-giant-oa-w64-payload-bary"
            | "point-vf-giant-oa-w64-sbe-pre-clip"
            | "point-vf-giant-oa-w64-sbe-pre-sf"
            | "point-vf-giant-oa-w64-early"
            | "point-vf-giant-oa-w64-early-scissor"
            | "point-vf-screen-oa-w64"
            | "point-vf-giant-oa-w64-arm"
            | "point-vf-giant-oa-w1023"
            | "point-vf-giant-oa-w1023-nowmpoint"
            | "point-vf-giant-oa-w1023-scissor"
            | "point-vf-giant-oa-vtxw"
            | "point-vf-giant-oa-early-w1023"
            | "point-vf-giant-oa-early-msrast-force"
            | "point-vf-giant-bt1"
            | "point-vf-giant-slot0"
            | "screen-vs-scratch"
            | "screen-vs-oa"
            | "screen-vs-ndc-oa"
            | "screen-vs-ndc-oa-hz0"
            | "screen-vs-sbe0"
            | "screen-vs-slot0-oa"
            | "screen-vs-urb2-oa"
            | "screen-vs-urb2-slot0-oa"
            | "vf-rect-oa"
            | "vf-rect-oa-pos0"
            | "vf-rect-oa-header"
            | "vf-rect-oa-deref0"
            | "vf-rect-ndc-oa"
            | "vf-rect-ndc-oa-halign128"
            | "vf-rect-ndc-oa-sbe-pre-clip"
            | "vf-rect-ndc-oa-sbe-pre-sf"
            | "vf-rect-ndc-oa-drawrect-early"
            | "vf-rect-ndc-oa-sample-early"
            | "vf-rect-ndc-oa-pc-clip-sf"
            | "vf-rect-ndc-oa-hz-pre-wm"
            | "vf-rect-ndc-oa-hz-post-extra"
            | "vf-rect-ndc-oa-payload-attr"
            | "vf-rect-ndc-oa-payload-depthw"
            | "vf-rect-ndc-oa-payload-bary"
            | "vf-rect-ndc-oa-persp"
            | "vf-rect-ndc-oa-clipxy"
            | "vf-rect-ndc-oa-clip-disable"
            | "vf-rect-ndc-oa-clip-force"
            | "vf-rect-ndc-oa-clip-d3d"
            | "vf-rect-ndc-oa-early-clipxy"
            | "vf-rect-ndc-oa-frontccw"
            | "vf-rect-ndc-oa-hz0"
            | "vf-rect-ndc-oa-early"
            | "vf-rect-ndc-oa-bt1"
            | "vf-rect-ndc-order-b-oa"
            | "vf-rect-ndc-order-c-oa"
            | "vf-rect-ndc-order-c-early-oa"
            | "vf-rect-ndc-order-c-clip-disable-oa"
            | "vf-rect-ndc-mesa-simple-oa"
            | "vf-rect-ndc-mesa-nosrc-header-oa"
            | "vf-rect-ndc-small-oa"
            | "vf-rect-ndc-cw-oa"
            | "vf-rect-ndc-alt-oa"
            | "vf-rect-order-b-oa"
            | "vf-rect-order-b-early-oa"
            | "vf-rect-order-b-scissor-oa"
            | "vf-rect-mesa-simple-oa"
            | "vf-rect-mesa-simple-oa-early"
            | "vf-rect-mesa-simple-oa-arm"
            | "vf-rect-mesa-nosrc-header-oa"
            | "vf-rect-order-c-oa"
            | "vf-tri-ndc-oa"
            | "vf-tri-ndc-oa-early"
            | "vf-tri-ndc-oa-early-clipxy"
            | "vf-tri-ndc-cw-oa-early"
            | "screen-rect-scratch"
            | "screen-rect-oa-early"
    )
}

fn is_artificial_fragment_marker_submit_name(submit_name: &str) -> bool {
    matches!(
        submit_name,
        "point-vf-giant-oa-clip-disable-arm"
            | "point-vf-giant-oa-w64-arm"
            | "vf-rect-mesa-simple-oa-arm"
    )
}

fn reset_fragment_boundary_probe() {
    FRAGMENT_CANDIDATE_READY.store(false, Ordering::Release);
    FRAGMENT_BOUNDARY_OBSERVED.store(false, Ordering::Release);
}

fn record_fragment_boundary_probe(candidate_ready: bool, fragment_observed: bool) {
    if candidate_ready {
        FRAGMENT_CANDIDATE_READY.store(true, Ordering::Release);
    }
    if fragment_observed {
        FRAGMENT_BOUNDARY_OBSERVED.store(true, Ordering::Release);
    }
}

fn fragment_candidate_ready() -> bool {
    FRAGMENT_CANDIDATE_READY.load(Ordering::Acquire)
}

fn fragment_boundary_observed() -> bool {
    FRAGMENT_BOUNDARY_OBSERVED.load(Ordering::Acquire)
}

unsafe impl Send for RenderWarmState {}
unsafe impl Sync for RenderWarmState {}

static WARM_STATE: Mutex<Option<RenderWarmState>> = Mutex::new(None);
static PRIMARY_TRIANGLE_SUBMITTED: AtomicBool = AtomicBool::new(false);
static PRIMARY_PROBE_IN_FLIGHT: AtomicBool = AtomicBool::new(false);
static PRIMARY_MI_SCANOUT_PROOF_SUBMITTED: AtomicBool = AtomicBool::new(false);
static FRAGMENT_CANDIDATE_READY: AtomicBool = AtomicBool::new(false);
static FRAGMENT_BOUNDARY_OBSERVED: AtomicBool = AtomicBool::new(false);
static WARM_BUFFERS_MAPPED: AtomicBool = AtomicBool::new(false);
static MEMORY_PROOF_LOGGED: AtomicBool = AtomicBool::new(false);
static PRIMARY_STRIPE_X_PHASE: AtomicU32 = AtomicU32::new(0);
static PRIMARY_PROBE_SEQ: AtomicU32 = AtomicU32::new(0);
