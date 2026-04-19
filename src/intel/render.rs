use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use spin::Mutex;

const FORCEWAKE_RENDER: usize = 0x0A278;
const FORCEWAKE_GT: usize = 0x0A188;
const FORCEWAKE_ACK_RENDER: usize = 0x0D84;
const FORCEWAKE_ACK_GT: usize = 0x130044;
const FORCEWAKE_KERNEL: u32 = 1 << 0;
const FORCEWAKE_FALLBACK: u32 = 1 << 15;
const TBIMR_BATCH_SIZE_OVERRIDE: u32 = 1 << 1;
const TBIMR_OPEN_BATCH_ENABLE: u32 = 1 << 4;
const TBIMR_FAST_CLIP: u32 = 1 << 5;
const FF_DOP_CLOCK_GATE_DISABLE: u32 = 1 << 1;
const FORCEWAKE_POLL_ITERS: usize = 20_000;
const RCS_RING_BASE: usize = 0x0000_2000;
const RCS_RING_TAIL: usize = RCS_RING_BASE + 0x30;
const RCS_RING_HEAD: usize = RCS_RING_BASE + 0x34;
const RCS_RING_START: usize = RCS_RING_BASE + 0x38;
const RCS_RING_CTL: usize = RCS_RING_BASE + 0x3C;
const RCS_RING_ACTHD: usize = RCS_RING_BASE + 0x74;
const RCS_RING_MI_MODE: usize = RCS_RING_BASE + 0x9C;
const RCS_RING_IMR: usize = RCS_RING_BASE + 0xA8;
const RCS_CS_DEBUG_MODE1: usize = RCS_RING_BASE + 0xEC;
const RCS_RING_EIR: usize = RCS_RING_BASE + 0xB0;
const RCS_RING_IPEIR: usize = RCS_RING_BASE + 0x64;
const RCS_RING_IPEHR: usize = RCS_RING_BASE + 0x68;
const RCS_RING_INSTDONE: usize = RCS_RING_BASE + 0x6C;
const CHICKEN_RASTER_2: usize = 0x6208;
const INSTDONE_GEOM: usize = 0x666C;
const SC_INSTDONE: usize = 0x7100;
const SC_INSTDONE_EXTRA: usize = 0x7104;
const SC_INSTDONE_EXTRA2: usize = 0x7108;
const RCS_RING_CONTEXT_CONTROL: usize = RCS_RING_BASE + 0x244;
const RCS_RING_CONTEXT_CONTROL_REF: usize = RCS_RING_BASE + 0x5A0;
const RCS_RING_MODE_GEN7: usize = RCS_RING_BASE + 0x29C;
const RCS_RING_EXECLIST_SUBMIT_PORT: usize = RCS_RING_BASE + 0x230;
const RCS_RING_EXECLIST_STATUS_LO: usize = RCS_RING_BASE + 0x234;
const RCS_RING_EXECLIST_STATUS_HI: usize = RCS_RING_BASE + 0x238;
const RCS_RING_EXECLIST_CONTROL: usize = RCS_RING_BASE + 0x550;
const RCS_RING_EXECLIST_SQ_LO: usize = RCS_RING_BASE + 0x510;
const RCS_RING_EXECLIST_SQ_HI: usize = RCS_RING_BASE + 0x514;
const RCS_RING_HWS_PGA: usize = RCS_RING_BASE + 0x80;
const GDRST: usize = 0x0000_941C;
const CURSOR_A_OFFSET: usize = 0x70080;
const CURSOR_B_OFFSET: usize = 0x71080;
const CURSOR_C_OFFSET: usize = 0x72080;
const CURSOR_D_OFFSET: usize = 0x73080;
const WARM_RING_BYTES: usize = 4096;
const WARM_CONTEXT_BYTES: usize = 22 * 4096;
const WARM_BATCH_BYTES: usize = 512 * 4096;
const WARM_DRAW_STATE_BYTES: usize = 16 * 4096;
const WARM_VERTEX_BYTES: usize = 4096;
const WARM_RESULT_BYTES: usize = 4096;
const WARM_STREAMOUT_BYTES: usize = 4096;
const BLT_RING_DWORDS: usize = 4;
const BLT_RING_TAIL_BYTES: usize = BLT_RING_DWORDS * core::mem::size_of::<u32>();
const LRC_STATE_OFFSET_DWORDS: usize = 4096 / core::mem::size_of::<u32>();
const GPU_VA_RING_BASE: u64 = 0x0080_0000;
const GPU_VA_CONTEXT_BASE: u64 = 0x0081_0000;
const GPU_VA_BATCH_BASE: u64 = 0x0180_0000;
const GPU_VA_RESULT_BASE: u64 = 0x0084_0000;
const GPU_VA_DRAW_STATE_BASE: u64 = 0x0086_0000;
const GPU_VA_VERTEX_BASE: u64 = 0x0087_0000;
const GPU_VA_STREAMOUT_BASE: u64 = 0x0088_0000;
const RING_VALID: u32 = 1;
const EL_CTRL_LOAD: u32 = 1 << 0;
const CTX_CTRL_RS_CTX_ENABLE: u32 = 1 << 1;
const CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT: u32 = 1 << 0;
const CTX_CTRL_ENGINE_CTX_SAVE_INHIBIT: u32 = 1 << 2;
const CTX_CTRL_INHIBIT_SYN_CTX_SWITCH: u32 = 1 << 3;
const CTX_DESC_FORCE_RESTORE: u32 = 1 << 2;
const GEN11_GFX_DISABLE_LEGACY_MODE: u32 = 1 << 3;
const GFX_RUN_LIST_ENABLE: u32 = 1 << 15;
const MODE_IDLE: u32 = 1 << 9;
const RING_MI_MODE_STOP_RING: u32 = 1 << 8;
const GRDOM_RENDER: u32 = 1 << 1;
const MI_BATCH_BUFFER_START_GEN8: u32 = (0x31 << 23) | 1;
const MI_BATCH_GTT: u32 = 2 << 6;
const MI_LOAD_REGISTER_IMM: u32 = 0x1100_0000;
const MI_LRI_CS_MMIO: u32 = 1 << 19;
const MI_LRI_FORCE_POSTED: u32 = 1 << 12;
const MI_BATCH_BUFFER_END: u32 = 0x0500_0000;
const MI_NOOP: u32 = 0;
const INTEL_LEGACY_64B_CONTEXT: u32 = 3;
const GEN8_CTX_VALID: u32 = 1 << 0;
const GEN8_CTX_PRIVILEGE: u32 = 1 << 8;
const GEN12_CTX_PRIORITY_NORMAL: u32 = 1 << 9;
const GEN8_CTX_ADDRESSING_MODE_SHIFT: u32 = 3;
const GEN12_CTX_RCS_INDIRECT_CTX_OFFSET_DEFAULT: u32 = 0xD;
const RCS_EXEC_RESULT_DONE: u32 = 0xC0DE_7701;
const RCS_EXEC_RESULT_MI_PROBE_DONE: u32 = 0xC0DE_7711;
const RCS_EXEC_RESULT_3D_NO_DRAW_DONE: u32 = 0xC0DE_7712;
const RCS_EXEC_RESULT_DRAW_PRE3D: u32 = 0xC0DE_7721;
const RCS_EXEC_RESULT_DRAW_POST_VF: u32 = 0xC0DE_7723;
const RCS_EXEC_RESULT_DRAW_POST_VS: u32 = 0xC0DE_7724;
const RCS_EXEC_RESULT_DRAW_POST_PS_STATE: u32 = 0xC0DE_7725;
const RCS_EXEC_RESULT_DRAW_POST_CLIP: u32 = 0xC0DE_7726;
const RCS_EXEC_RESULT_DRAW_POST_RASTER: u32 = 0xC0DE_7727;
const RCS_EXEC_RESULT_DRAW_POST3D: u32 = 0xC0DE_7722;
const PRIMARY_TRIANGLE_SUBMIT_ATTEMPTS: usize = 3;
const PRIMARY_USE_MI_STRIPE_PROBE: bool = false;
const PRIMARY_USE_3D_NO_DRAW_PROBE: bool = false;
const PRIMARY_USE_DRAW_PATH_BOOT_ONCE: bool = true;
const PRIMARY_DISABLE_RENDER_BRINGUP: bool = false;
const MI_STRIPE_COUNT: usize = 12;
const MI_STRIPE_WIDTH_PX: usize = 4;
const MI_STRIPE_X_STEP_PX: u32 = 1;
const PRIMARY_PERIODIC_LOG_EVERY: u32 = 30;
const MI_STORE_DATA_IMM_GGTT_DW1: u32 = 0x1040_0002;
const RENDER_MOCS: u32 = 1;
const SURFTYPE_2D: u32 = 1;
const SURFTYPE_NULL: u32 = 7;
const SURFACE_FORMAT_B8G8R8A8_UNORM: u32 = 10;
const SURFACE_FORMAT_R32G32B32A32_UINT: u32 = 2;
const SURFACE_FORMAT_R32G32B32_FLOAT: u32 = 64;
const DEPTH_SURFACE_FORMAT_D32_FLOAT: u32 = 1;
const SURFACE_HALIGN_4: u32 = 1;
const SURFACE_VALIGN_4: u32 = 1;
const SHADER_CHANNEL_RED: u32 = 4;
const SHADER_CHANNEL_GREEN: u32 = 5;
const SHADER_CHANNEL_BLUE: u32 = 6;
const SHADER_CHANNEL_ALPHA: u32 = 7;
const SBE_ACTIVE_COMPONENT_XYZW_MASK_DWORD: u32 = 0xFFFF_FFFF;
const CLIP_FORCE_CLIP_MODE: u32 = 1 << 16;
const CLIP_PERSPECTIVE_DIVIDE_DISABLE: u32 = 1 << 9;
const CLIP_MODE_ACCEPT_ALL: u32 = 4 << 13;
const WM_FORCE_KILL_PIXEL_OFF: u32 = 1;
const PS_VECTOR_MASK_ENABLE: u32 = 1 << 30;
const PS_SINGLE_PROGRAM_FLOW: u32 = 1 << 31;
const PS_PUSH_CONSTANT_ENABLE: u32 = 1 << 11;
const PS_MAX_THREADS_SHIFT: u32 = 23;
const PS_EXTRA_PIXEL_SHADER_COMPUTES_STENCIL: u32 = 1 << 5;
const PS_EXTRA_PIXEL_SHADER_IS_PER_SAMPLE: u32 = 1 << 6;
const PS_EXTRA_ATTRIBUTE_ENABLE: u32 = 1 << 8;
const PS_EXTRA_PIXEL_SHADER_VALID: u32 = 1 << 31;
const VFCOMP_STORE_SRC: u32 = 1;
const VFCOMP_STORE_0: u32 = 2;
const VFCOMP_STORE_1_FP: u32 = 3;
const PIPELINE_SELECT_3D: u32 = (4 << 16) | (1 << 24) | (1 << 27) | (3 << 29);
const PIPE_CONTROL_CMD: u32 = 4 | (2 << 24) | (3 << 27) | (3 << 29);
const STATE_BASE_ADDRESS_CMD: u32 = 20 | (1 << 16) | (1 << 24) | (3 << 29);
const CMD_3DSTATE_AA_LINE_PARAMETERS: u32 = 1 | (10 << 16) | (1 << 24) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_SAMPLE_PATTERN: u32 = 7 | (28 << 16) | (1 << 24) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_3D_MODE: u32 = 3 | (30 << 16) | (1 << 24) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_SLICE_TABLE_STATE_POINTERS: u32 = (32 << 16) | (1 << 24) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_BINDING_TABLE_POOL_ALLOC: u32 =
    2 | (25 << 16) | (1 << 24) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_VS: u32 = 7 | (16 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_GS: u32 = 8 | (17 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_CLEAR_PARAMS: u32 = 1 | (4 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_DEPTH_BUFFER: u32 = 8 | (5 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_STENCIL_BUFFER: u32 = 6 | (6 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_HIER_DEPTH_BUFFER: u32 = 3 | (7 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_CLIP: u32 = 2 | (18 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_SF: u32 = 2 | (19 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_WM: u32 = 0 | (20 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_HS: u32 = 7 | (27 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_TE: u32 = 3 | (28 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_DS: u32 = 9 | (29 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_STREAMOUT: u32 = 3 | (30 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_SO_BUFFER_INDEX_0: u32 = 6 | (0x60 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_SO_DECL_LIST_1: u32 = 3 | (23 << 16) | (1 << 24) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_SBE: u32 = 4 | (31 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_PS: u32 = 10 | (32 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_VIEWPORT_STATE_POINTERS_SF_CLIP: u32 = (33 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_VIEWPORT_STATE_POINTERS_CC: u32 = (35 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_SCISSOR_STATE_POINTERS: u32 = (15 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_BLEND_STATE_POINTERS: u32 = (36 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_BINDING_TABLE_POINTERS_VS: u32 = (38 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_BINDING_TABLE_POINTERS_HS: u32 = (39 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_BINDING_TABLE_POINTERS_DS: u32 = (40 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_BINDING_TABLE_POINTERS_GS: u32 = (41 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_BINDING_TABLE_POINTERS_PS: u32 = (42 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_CC_STATE_POINTERS: u32 = (14 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_SAMPLER_STATE_POINTERS_VS: u32 = (43 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_SAMPLER_STATE_POINTERS_PS: u32 = (47 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_VF_STATISTICS: u32 = (11 << 16) | (1 << 27) | (3 << 29);
const CMD_3DSTATE_VF: u32 = (12 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_MULTISAMPLE: u32 = (13 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_DRAWING_RECTANGLE: u32 = 2 | (1 << 24) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_SAMPLE_MASK: u32 = (24 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_WM_CHROMA_KEY: u32 = (76 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_PS_BLEND: u32 = (77 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_WM_DEPTH_STENCIL: u32 = 2 | (78 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_PS_EXTRA: u32 = (79 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_DEPTH_BOUNDS: u32 = 2 | (113 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_RASTER: u32 = 3 | (80 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_SBE_SWIZ: u32 = 9 | (81 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_WM_HZ_OP: u32 = 4 | (82 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_URB_ALLOC_HS: u32 = 1 | (89 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_URB_ALLOC_DS: u32 = 1 | (90 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_URB_ALLOC_GS: u32 = 1 | (91 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_URB_ALLOC_VS: u32 = 1 | (88 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_VF_INSTANCING: u32 = 1 | (73 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_VF_SGVS: u32 = (74 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_VF_TOPOLOGY: u32 = (75 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_VF_SGVS_2: u32 = 1 | (86 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_VERTEX_BUFFERS_1: u32 = 3 | (8 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_VERTEX_ELEMENTS_1: u32 = 1 | (9 << 16) | (3 << 27) | (3 << 29);
const CMD_3DPRIMITIVE: u32 = 5 | (3 << 24) | (3 << 27) | (3 << 29);
const PIPE_CONTROL_FLUSH_BITS: u32 = (1 << 5) | (1 << 7) | (1 << 12) | (1 << 20) | (1 << 26);
const PIPE_CONTROL_INVALIDATE_BITS: u32 =
    (1 << 2) | (1 << 3) | (1 << 4) | (1 << 10) | (1 << 11) | (1 << 18) | (1 << 20);
const PIPE_CONTROL_POST_SYNC_WRITE_IMMEDIATE: u32 = 1 << 14;
const PIPE_CONTROL_DEST_GGTT: u32 = 1 << 24;
const PIPE_CONTROL_CS_STALL: u32 = 1 << 20;
const PIPE_CONTROL_POST_DRAW_SYNC_BITS: u32 =
    PIPE_CONTROL_FLUSH_BITS | PIPE_CONTROL_POST_SYNC_WRITE_IMMEDIATE | PIPE_CONTROL_DEST_GGTT;
const RESULT_DEBUG_SENTINEL: u32 = 0xC0DE_7700;
const RESULT_SLOT_PRE3D_DWORD: usize = 0;
// Keep this legacy DWORD slot around for MI-only probes. PIPE_CONTROL post-sync
// writes a QWord and therefore must target an 8-byte-aligned destination.
const RESULT_SLOT_POST3D_DWORD: usize = 1;
const RESULT_SLOT_FINAL_DWORD: usize = 2;
const RESULT_SLOT_POST_VF_DWORD: usize = 3;
const RESULT_SLOT_POST_VS_DWORD: usize = 4;
const RESULT_SLOT_POST_PS_STATE_DWORD: usize = 5;
const RESULT_SLOT_POST_CLIP_DWORD: usize = 6;
const RESULT_SLOT_POST_RASTER_DWORD: usize = 7;
const RESULT_SLOT_POST3D_PIPE_CONTROL_LO_DWORD: usize = 8;
const RESULT_SLOT_POST3D_PIPE_CONTROL_HI_DWORD: usize = 9;
const RESULT_DEBUG_DWORD_COUNT: usize = RESULT_SLOT_POST3D_PIPE_CONTROL_HI_DWORD + 1;
const SO_NUM_PRIMS_WRITTEN_0: usize = 0x5200;
const SO_WRITE_OFFSET_0: usize = 0x5280;
const TRIANGLE_TOPOLOGY_POINTLIST: u32 = 1;
const TRIANGLE_TOPOLOGY_TRILIST: u32 = 4;
const TRIANGLE_PS_MAX_THREADS: u32 = 63;
const TRIANGLE_VS_URB_START: u32 = 4;
const TRIANGLE_VS_URB_ENTRIES: u32 = 192;
const TRIANGLE_VS_URB_OUTPUT_LENGTH_OVERRIDE: Option<u8> = None;
const GFX125_GEOMETRY_DSS_ENABLE: usize = 0x913C;
const GFX125_PIXEL_PIPES: usize = 3;
const GFX125_DUAL_SUBSLICES_PER_PIXEL_PIPE: usize = 2;
const GFX125_SLICE_HASH_TABLES: usize = 7;
const GFX125_SLICE_HASH_DIM: usize = 16;
const GFX125_SLICE_HASH_TABLE_ENTRIES: usize = GFX125_SLICE_HASH_DIM * GFX125_SLICE_HASH_DIM;
const GFX125_SLICE_HASH_TABLE_DWORDS_PER_TABLE: usize = GFX125_SLICE_HASH_TABLE_ENTRIES / 8;
const GFX125_SLICE_HASH_TABLE_DWORDS: usize = 224;
const GFX125_SLICE_HASH_TABLE_BYTES: usize = GFX125_SLICE_HASH_TABLE_DWORDS * 4;
const GFX125_3D_MODE_CROSS_SLICE_HASHING_32X32: u32 = 3;
const VF_STREAMOUT_SLICE_HASH_TABLE_OFFSET: usize = 0x1200;
const TRIANGLE_MIN_DIM: usize = 8;
// This proof path emits one MI_STORE_DATA_IMM per covered pixel, so keep the
// triangle intentionally small until we switch to an actual draw pipeline.
const TRIANGLE_MAX_W: usize = 20;
const TRIANGLE_MAX_H: usize = 18;
const TRIANGLE_DRAW_VERTICES: usize = 3;
const TRIANGLE_DRAW_VERTEX_DWORDS: usize = crate::intel::shader::TRIANGLE_VERTEX_COMPONENTS;
const TRIANGLE_DRAW_VERTEX_STRIDE: usize = crate::intel::shader::TRIANGLE_VERTEX_STRIDE_BYTES;
const TRIANGLE_STATS_LOG: [crate::intel::stats::RenderStat; 3] = [
    crate::intel::stats::RenderStat::IaVerticesCount,
    crate::intel::stats::RenderStat::IaPrimitivesCount,
    crate::intel::stats::RenderStat::VsInvocationCount,
];

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
    StreamoutProof,
    VfStreamoutProof,
    VsStreamoutProof,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum StreamoutProofExperiment {
    PositionSlot1,
    HeaderAndPositionSlots01,
}

impl StreamoutProofExperiment {
    fn label(self) -> &'static str {
        match self {
            Self::PositionSlot1 => "pos-slot1",
            Self::HeaderAndPositionSlots01 => "header+pos-slots01",
        }
    }

    fn alternate(self) -> Self {
        match self {
            Self::PositionSlot1 => Self::HeaderAndPositionSlots01,
            Self::HeaderAndPositionSlots01 => Self::PositionSlot1,
        }
    }

    fn vertex_bytes(self) -> usize {
        match self {
            Self::PositionSlot1 => 16,
            Self::HeaderAndPositionSlots01 => 32,
        }
    }

    fn vertex_read_length(self) -> u32 {
        1
    }

    fn so_decl_header(self) -> u32 {
        match self {
            Self::PositionSlot1 => 3 | (23 << 16) | (1 << 24) | (3 << 27) | (3 << 29),
            Self::HeaderAndPositionSlots01 => 5 | (23 << 16) | (1 << 24) | (3 << 27) | (3 << 29),
        }
    }

    fn so_decl_buffer_selects(self) -> u32 {
        1
    }

    fn so_decl_num_entries(self) -> u32 {
        match self {
            Self::PositionSlot1 => 1,
            Self::HeaderAndPositionSlots01 => 2,
        }
    }

    fn so_decl_entry_dwords(self) -> [u32; 4] {
        match self {
            Self::PositionSlot1 => [0x0000_001F, 0x0000_0000, 0x0000_0000, 0x0000_0000],
            Self::HeaderAndPositionSlots01 => [0x0000_000F, 0x0000_0000, 0x0000_001F, 0x0000_0000],
        }
    }

    fn compatible(self) -> bool {
        true
    }

    fn vf_slot_contract(self) -> &'static str {
        match self {
            Self::PositionSlot1 => "slot0=zero slot1=position",
            Self::HeaderAndPositionSlots01 => "slot0=header slot1=position",
        }
    }
}

fn select_streamout_proof_experiment(probe_seq: u32) -> StreamoutProofExperiment {
    if (probe_seq & 1) != 0 {
        StreamoutProofExperiment::HeaderAndPositionSlots01
    } else {
        StreamoutProofExperiment::PositionSlot1
    }
}

impl TriangleBatchMode {
    fn topology(self) -> u32 {
        match self {
            Self::Draw => TRIANGLE_TOPOLOGY_TRILIST,
            Self::StreamoutProof | Self::VfStreamoutProof | Self::VsStreamoutProof => {
                TRIANGLE_TOPOLOGY_POINTLIST
            }
        }
    }

    fn streamout_enabled(self) -> bool {
        matches!(
            self,
            Self::StreamoutProof | Self::VfStreamoutProof | Self::VsStreamoutProof
        )
    }
}

fn is_streamout_submit_name(submit_name: &str) -> bool {
    matches!(
        submit_name,
        "streamout-proof" | "vf-streamout-proof" | "vs-streamout-proof"
    )
}

fn is_vf_streamout_submit_name(submit_name: &str) -> bool {
    submit_name == "vf-streamout-proof"
}

fn is_triangle_debug_submit_name(submit_name: &str) -> bool {
    submit_name == "draw-path" || is_streamout_submit_name(submit_name)
}

unsafe impl Send for RenderWarmState {}
unsafe impl Sync for RenderWarmState {}

static WARM_STATE: Mutex<Option<RenderWarmState>> = Mutex::new(None);
static PRIMARY_TRIANGLE_SUBMITTED: AtomicBool = AtomicBool::new(false);
static PRIMARY_PROBE_IN_FLIGHT: AtomicBool = AtomicBool::new(false);
static WARM_BUFFERS_MAPPED: AtomicBool = AtomicBool::new(false);
static PRIMARY_STRIPE_X_PHASE: AtomicU32 = AtomicU32::new(0);
static PRIMARY_PROBE_SEQ: AtomicU32 = AtomicU32::new(0);

pub(crate) fn warm_once(dev: crate::intel::Dev) -> RenderWarmState {
    if let Some(warm) = *WARM_STATE.lock() {
        return warm;
    }

    let Some((ring_phys, ring_virt)) = crate::dma::alloc(WARM_RING_BYTES, crate::intel::WARM_ALIGN)
    else {
        let warm = RenderWarmState {
            device_id: dev.device_id,
            revision_id: dev.revision_id,
            mmio_base: dev.mmio as usize,
            mmio_len: dev.mmio_len,
            ring_phys: 0,
            ring_virt: core::ptr::null_mut(),
            ring_len: 0,
            context_phys: 0,
            context_virt: core::ptr::null_mut(),
            context_len: 0,
            batch_phys: 0,
            batch_virt: core::ptr::null_mut(),
            batch_len: 0,
            draw_state_phys: 0,
            draw_state_virt: core::ptr::null_mut(),
            draw_state_len: 0,
            vertex_phys: 0,
            vertex_virt: core::ptr::null_mut(),
            vertex_len: 0,
            result_phys: 0,
            result_virt: core::ptr::null_mut(),
            result_len: 0,
            streamout_phys: 0,
            streamout_virt: core::ptr::null_mut(),
            streamout_len: 0,
        };
        *WARM_STATE.lock() = Some(warm);
        crate::log!("intel/render: warm alloc failed part=ring size=0x{:X}\n", WARM_RING_BYTES);
        return warm;
    };
    let Some((context_phys, context_virt)) =
        crate::dma::alloc(WARM_CONTEXT_BYTES, crate::intel::WARM_ALIGN)
    else {
        let warm = RenderWarmState {
            device_id: dev.device_id,
            revision_id: dev.revision_id,
            mmio_base: dev.mmio as usize,
            mmio_len: dev.mmio_len,
            ring_phys,
            ring_virt,
            ring_len: WARM_RING_BYTES,
            context_phys: 0,
            context_virt: core::ptr::null_mut(),
            context_len: 0,
            batch_phys: 0,
            batch_virt: core::ptr::null_mut(),
            batch_len: 0,
            draw_state_phys: 0,
            draw_state_virt: core::ptr::null_mut(),
            draw_state_len: 0,
            vertex_phys: 0,
            vertex_virt: core::ptr::null_mut(),
            vertex_len: 0,
            result_phys: 0,
            result_virt: core::ptr::null_mut(),
            result_len: 0,
            streamout_phys: 0,
            streamout_virt: core::ptr::null_mut(),
            streamout_len: 0,
        };
        *WARM_STATE.lock() = Some(warm);
        crate::log!(
            "intel/render: warm alloc failed part=context size=0x{:X}\n",
            WARM_CONTEXT_BYTES
        );
        return warm;
    };
    let Some((batch_phys, batch_virt)) =
        crate::dma::alloc(WARM_BATCH_BYTES, crate::intel::WARM_ALIGN)
    else {
        let warm = RenderWarmState {
            device_id: dev.device_id,
            revision_id: dev.revision_id,
            mmio_base: dev.mmio as usize,
            mmio_len: dev.mmio_len,
            ring_phys,
            ring_virt,
            ring_len: WARM_RING_BYTES,
            context_phys,
            context_virt,
            context_len: WARM_CONTEXT_BYTES,
            batch_phys: 0,
            batch_virt: core::ptr::null_mut(),
            batch_len: 0,
            draw_state_phys: 0,
            draw_state_virt: core::ptr::null_mut(),
            draw_state_len: 0,
            vertex_phys: 0,
            vertex_virt: core::ptr::null_mut(),
            vertex_len: 0,
            result_phys: 0,
            result_virt: core::ptr::null_mut(),
            result_len: 0,
            streamout_phys: 0,
            streamout_virt: core::ptr::null_mut(),
            streamout_len: 0,
        };
        *WARM_STATE.lock() = Some(warm);
        crate::log!("intel/render: warm alloc failed part=batch size=0x{:X}\n", WARM_BATCH_BYTES);
        return warm;
    };
    let Some((draw_state_phys, draw_state_virt)) =
        crate::dma::alloc(WARM_DRAW_STATE_BYTES, crate::intel::WARM_ALIGN)
    else {
        let warm = RenderWarmState {
            device_id: dev.device_id,
            revision_id: dev.revision_id,
            mmio_base: dev.mmio as usize,
            mmio_len: dev.mmio_len,
            ring_phys,
            ring_virt,
            ring_len: WARM_RING_BYTES,
            context_phys,
            context_virt,
            context_len: WARM_CONTEXT_BYTES,
            batch_phys,
            batch_virt,
            batch_len: WARM_BATCH_BYTES,
            draw_state_phys: 0,
            draw_state_virt: core::ptr::null_mut(),
            draw_state_len: 0,
            vertex_phys: 0,
            vertex_virt: core::ptr::null_mut(),
            vertex_len: 0,
            result_phys: 0,
            result_virt: core::ptr::null_mut(),
            result_len: 0,
            streamout_phys: 0,
            streamout_virt: core::ptr::null_mut(),
            streamout_len: 0,
        };
        *WARM_STATE.lock() = Some(warm);
        crate::log!(
            "intel/render: warm alloc failed part=draw-state size=0x{:X}\n",
            WARM_DRAW_STATE_BYTES
        );
        return warm;
    };
    let Some((vertex_phys, vertex_virt)) =
        crate::dma::alloc(WARM_VERTEX_BYTES, crate::intel::WARM_ALIGN)
    else {
        let warm = RenderWarmState {
            device_id: dev.device_id,
            revision_id: dev.revision_id,
            mmio_base: dev.mmio as usize,
            mmio_len: dev.mmio_len,
            ring_phys,
            ring_virt,
            ring_len: WARM_RING_BYTES,
            context_phys,
            context_virt,
            context_len: WARM_CONTEXT_BYTES,
            batch_phys,
            batch_virt,
            batch_len: WARM_BATCH_BYTES,
            draw_state_phys,
            draw_state_virt,
            draw_state_len: WARM_DRAW_STATE_BYTES,
            vertex_phys: 0,
            vertex_virt: core::ptr::null_mut(),
            vertex_len: 0,
            result_phys: 0,
            result_virt: core::ptr::null_mut(),
            result_len: 0,
            streamout_phys: 0,
            streamout_virt: core::ptr::null_mut(),
            streamout_len: 0,
        };
        *WARM_STATE.lock() = Some(warm);
        crate::log!("intel/render: warm alloc failed part=vertex size=0x{:X}\n", WARM_VERTEX_BYTES);
        return warm;
    };
    let Some((result_phys, result_virt)) =
        crate::dma::alloc(WARM_RESULT_BYTES, crate::intel::WARM_ALIGN)
    else {
        let warm = RenderWarmState {
            device_id: dev.device_id,
            revision_id: dev.revision_id,
            mmio_base: dev.mmio as usize,
            mmio_len: dev.mmio_len,
            ring_phys,
            ring_virt,
            ring_len: WARM_RING_BYTES,
            context_phys,
            context_virt,
            context_len: WARM_CONTEXT_BYTES,
            batch_phys,
            batch_virt,
            batch_len: WARM_BATCH_BYTES,
            draw_state_phys,
            draw_state_virt,
            draw_state_len: WARM_DRAW_STATE_BYTES,
            vertex_phys,
            vertex_virt,
            vertex_len: WARM_VERTEX_BYTES,
            result_phys: 0,
            result_virt: core::ptr::null_mut(),
            result_len: 0,
            streamout_phys: 0,
            streamout_virt: core::ptr::null_mut(),
            streamout_len: 0,
        };
        *WARM_STATE.lock() = Some(warm);
        crate::log!("intel/render: warm alloc failed part=result size=0x{:X}\n", WARM_RESULT_BYTES);
        return warm;
    };
    let Some((streamout_phys, streamout_virt)) =
        crate::dma::alloc(WARM_STREAMOUT_BYTES, crate::intel::WARM_ALIGN)
    else {
        let warm = RenderWarmState {
            device_id: dev.device_id,
            revision_id: dev.revision_id,
            mmio_base: dev.mmio as usize,
            mmio_len: dev.mmio_len,
            ring_phys,
            ring_virt,
            ring_len: WARM_RING_BYTES,
            context_phys,
            context_virt,
            context_len: WARM_CONTEXT_BYTES,
            batch_phys,
            batch_virt,
            batch_len: WARM_BATCH_BYTES,
            draw_state_phys,
            draw_state_virt,
            draw_state_len: WARM_DRAW_STATE_BYTES,
            vertex_phys,
            vertex_virt,
            vertex_len: WARM_VERTEX_BYTES,
            result_phys,
            result_virt,
            result_len: WARM_RESULT_BYTES,
            streamout_phys: 0,
            streamout_virt: core::ptr::null_mut(),
            streamout_len: 0,
        };
        *WARM_STATE.lock() = Some(warm);
        crate::log!(
            "intel/render: warm alloc failed part=streamout size=0x{:X}\n",
            WARM_STREAMOUT_BYTES
        );
        return warm;
    };

    unsafe {
        core::ptr::write_bytes(ring_virt, 0, WARM_RING_BYTES);
        core::ptr::write_bytes(context_virt, 0, WARM_CONTEXT_BYTES);
        core::ptr::write_bytes(batch_virt, 0, WARM_BATCH_BYTES);
        core::ptr::write_bytes(draw_state_virt, 0, WARM_DRAW_STATE_BYTES);
        core::ptr::write_bytes(vertex_virt, 0, WARM_VERTEX_BYTES);
        core::ptr::write_bytes(result_virt, 0, WARM_RESULT_BYTES);
        core::ptr::write_bytes(streamout_virt, 0, WARM_STREAMOUT_BYTES);
    }

    let warm = RenderWarmState {
        device_id: dev.device_id,
        revision_id: dev.revision_id,
        mmio_base: dev.mmio as usize,
        mmio_len: dev.mmio_len,
        ring_phys,
        ring_virt,
        ring_len: WARM_RING_BYTES,
        context_phys,
        context_virt,
        context_len: WARM_CONTEXT_BYTES,
        batch_phys,
        batch_virt,
        batch_len: WARM_BATCH_BYTES,
        draw_state_phys,
        draw_state_virt,
        draw_state_len: WARM_DRAW_STATE_BYTES,
        vertex_phys,
        vertex_virt,
        vertex_len: WARM_VERTEX_BYTES,
        result_phys,
        result_virt,
        result_len: WARM_RESULT_BYTES,
        streamout_phys,
        streamout_virt,
        streamout_len: WARM_STREAMOUT_BYTES,
    };
    *WARM_STATE.lock() = Some(warm);
    warm
}

pub fn warm_state() -> Option<RenderWarmState> {
    *WARM_STATE.lock()
}

pub fn log_cursor_plane_info(warm: RenderWarmState) {
    let caps = cursor_plane_caps(warm.device_id);
    crate::log!(
        "intel/display: cursor-plane platform={} rev=0x{:02X} max={}x{} pipes={} layout={} regs=A:0x{:X},B:0x{:X},C:0x{:X},D:0x{:X}\n",
        caps.platform,
        warm.revision_id,
        caps.max_width,
        caps.max_height,
        caps.pipe_count,
        caps.layout,
        CURSOR_A_OFFSET,
        CURSOR_B_OFFSET,
        CURSOR_C_OFFSET,
        CURSOR_D_OFFSET
    );
}

pub fn log_sprite_plane_info(warm: RenderWarmState) {
    let caps = sprite_plane_caps(warm.device_id);
    crate::log!(
        "intel/display: sprite-planes platform={} display_ver={} pipes={} overlays/pipe={} type=universal props=rotation:{} reflect_x:{} alpha:1 blend:pixel-none|premulti|coverage zpos:immutable csc:{} range:limited|full scaler:{} damage_clips:{}\n",
        caps.platform,
        caps.display_ver,
        caps.pipe_count,
        caps.overlays_per_pipe,
        caps.rotation,
        caps.reflect_x as u8,
        caps.csc,
        caps.scaling_filter,
        caps.damage_clips as u8
    );
}

pub fn forcewake_render_acquire(warm: RenderWarmState) -> bool {
    let dev = crate::intel::Dev {
        bus: 0,
        slot: 0,
        function: 0,
        device_id: warm.device_id,
        revision_id: warm.revision_id,
        mmio: warm.mmio_base as *mut u8,
        mmio_len: warm.mmio_len,
    };

    crate::intel::mmio_write(
        dev,
        FORCEWAKE_RENDER,
        crate::intel::mask_dis(FORCEWAKE_KERNEL | FORCEWAKE_FALLBACK),
    );
    let render_cleared = wait_eq(
        dev,
        FORCEWAKE_ACK_RENDER,
        FORCEWAKE_KERNEL | FORCEWAKE_FALLBACK,
        0,
        FORCEWAKE_POLL_ITERS,
    );

    crate::intel::mmio_write(dev, FORCEWAKE_RENDER, crate::intel::mask_en(FORCEWAKE_KERNEL));
    let render_ok = wait_eq(
        dev,
        FORCEWAKE_ACK_RENDER,
        FORCEWAKE_KERNEL,
        FORCEWAKE_KERNEL,
        FORCEWAKE_POLL_ITERS,
    );

    crate::intel::mmio_write(dev, FORCEWAKE_GT, crate::intel::mask_en(FORCEWAKE_KERNEL));
    let gt_ok =
        wait_eq(dev, FORCEWAKE_ACK_GT, FORCEWAKE_KERNEL, FORCEWAKE_KERNEL, FORCEWAKE_POLL_ITERS);
    crate::intel::mmio_write(
        dev,
        RCS_CS_DEBUG_MODE1,
        crate::intel::mask_en(FF_DOP_CLOCK_GATE_DISABLE),
    );
    let cs_debug_mode1 = crate::intel::mmio_read(dev, RCS_CS_DEBUG_MODE1);
    apply_gfx125_raster_workarounds(dev);

    if should_log_primary_probe_detail() {
        crate::log!(
            "intel/render: forcewake render_cleared={} render_ack=0x{:08X} gt_ack=0x{:08X} cs_debug_mode1=0x{:08X} ff_dop_cg_disable={} ok={}\n",
            render_cleared as u8,
            crate::intel::mmio_read(dev, FORCEWAKE_ACK_RENDER),
            crate::intel::mmio_read(dev, FORCEWAKE_ACK_GT),
            cs_debug_mode1,
            ((cs_debug_mode1 & FF_DOP_CLOCK_GATE_DISABLE) != 0) as u8,
            (render_ok && gt_ok) as u8
        );
    }

    render_ok && gt_ok
}

fn apply_gfx125_raster_workarounds(dev: crate::intel::Dev) {
    if !device_is_gfx125(dev.device_id) {
        return;
    }

    // Mesa's gfx125 init path enables these TBIMR-related raster controls up
    // front. Keep the bring-up path aligned so the first primitive does not
    // depend on whatever the boot context happened to leave behind.
    let before = crate::intel::mmio_read(dev, CHICKEN_RASTER_2);
    crate::intel::mmio_write(dev, CHICKEN_RASTER_2, gfx125_chicken_raster_2_value());
    let after = crate::intel::mmio_read(dev, CHICKEN_RASTER_2);

    if should_log_primary_probe_detail() {
        crate::log!(
            "intel/render: gfx125-raster-wa chicken_raster_2 before=0x{:08X} after=0x{:08X} tbimr_batch_override={} tbimr_open_batch={} tbimr_fast_clip={}\n",
            before,
            after,
            ((after & TBIMR_BATCH_SIZE_OVERRIDE) != 0) as u8,
            ((after & TBIMR_OPEN_BATCH_ENABLE) != 0) as u8,
            ((after & TBIMR_FAST_CLIP) != 0) as u8,
        );
    }
}

fn gfx125_chicken_raster_2_value() -> u32 {
    let bits = TBIMR_BATCH_SIZE_OVERRIDE | TBIMR_OPEN_BATCH_ENABLE | TBIMR_FAST_CLIP;
    crate::intel::mask_en(bits)
}

#[derive(Copy, Clone)]
struct Gfx125SliceHashConfig {
    geometry_dss_enable: u32,
    ppipe_subslices: [u8; GFX125_PIXEL_PIPES],
    ppipe_mask1: u32,
    ppipe_mask2: u32,
    cross_slice_hashing_mode: u32,
}

fn gfx125_slice_hash_config(warm: RenderWarmState) -> Gfx125SliceHashConfig {
    let dev = crate::intel::Dev {
        bus: 0,
        slot: 0,
        function: 0,
        device_id: warm.device_id,
        revision_id: warm.revision_id,
        mmio: warm.mmio_base as *mut u8,
        mmio_len: warm.mmio_len,
    };
    let geometry_dss_enable = crate::intel::mmio_read(dev, GFX125_GEOMETRY_DSS_ENABLE);
    let mut ppipe_subslices = [0u8; GFX125_PIXEL_PIPES];
    let ppipe_mask = (1u32 << GFX125_DUAL_SUBSLICES_PER_PIXEL_PIPE) - 1;

    for (ppipe, count) in ppipe_subslices.iter_mut().enumerate() {
        let shift = ppipe * GFX125_DUAL_SUBSLICES_PER_PIXEL_PIPE;
        *count = ((geometry_dss_enable >> shift) & ppipe_mask).count_ones() as u8;
    }

    let mut ppipe_mask1 = 0u32;
    let mut ppipe_mask2 = 0u32;
    for (ppipe, count) in ppipe_subslices.iter().copied().enumerate() {
        if count > 0 {
            ppipe_mask1 |= 1u32 << ppipe;
        }
        if count > 1 {
            ppipe_mask2 |= 1u32 << ppipe;
        }
    }

    if ppipe_mask1 == 0 {
        ppipe_subslices[0] = 1;
        ppipe_mask1 = 1;
    }

    let cross_slice_hashing_mode = if ppipe_mask1.count_ones() > 1 {
        GFX125_3D_MODE_CROSS_SLICE_HASHING_32X32
    } else {
        0
    };

    Gfx125SliceHashConfig {
        geometry_dss_enable,
        ppipe_subslices,
        ppipe_mask1,
        ppipe_mask2,
        cross_slice_hashing_mode,
    }
}

fn gfx125_logbase2_ceil(value: usize) -> usize {
    if value <= 1 {
        0
    } else {
        (usize::BITS - (value - 1).leading_zeros()) as usize
    }
}

fn gfx125_compute_pixel_hash_table_nway(
    mask1: u32,
    mask2: u32,
    table: &mut [u8; GFX125_SLICE_HASH_TABLE_ENTRIES],
) {
    let mut mask2 = mask2;
    if mask1 == mask2 {
        mask2 = 0;
    }

    let mut phys_ids = [0usize; 64];
    let mut num_ids = 0usize;
    for bit in 0..u32::BITS as usize {
        let bit_mask = 1u32 << bit;
        if (mask1 & bit_mask) != 0 {
            phys_ids[num_ids] = bit;
            num_ids += 1;
        }
        if (mask2 & bit_mask) != 0 {
            phys_ids[num_ids] = bit;
            num_ids += 1;
        }
    }

    if num_ids == 0 {
        table.fill(0);
        return;
    }

    let bits = gfx125_logbase2_ceil(num_ids);
    let mut swzy = [0usize; 64];
    for (k, slot) in swzy.iter_mut().enumerate().take(num_ids) {
        let mut t = num_ids;
        let mut s = 0usize;

        for l in 0..bits {
            if (k & (1usize << l)) != 0 {
                s += (t + 1) >> 1;
                t >>= 1;
            } else {
                t = (t + 1) >> 1;
            }
        }

        *slot = s;
    }

    let mut swzx = [0usize; 64];
    if mask1 != 0 && mask2 != 0 {
        for (k, slot) in swzx.iter_mut().enumerate().take(num_ids) {
            let mut l = k;
            let mut t = num_ids;
            let mut s = 0usize;
            let mut in_range = false;

            while t > 1 {
                let first_in_range = t <= GFX125_SLICE_HASH_DIM && !in_range;
                in_range |= first_in_range;

                if l >= ((t + 1) >> 1) {
                    if !in_range {
                        s += (t + 1) >> 1;
                    } else if first_in_range {
                        s += 1;
                    } else {
                        s += ((t + 1) >> 1) << 1;
                    }

                    l -= (t + 1) >> 1;
                    t >>= 1;
                } else {
                    t = (t + 1) >> 1;
                }
            }

            *slot = s;
        }
    } else {
        for (k, slot) in swzx.iter_mut().enumerate().take(num_ids) {
            *slot = k;
        }
    }

    for y in 0..GFX125_SLICE_HASH_DIM {
        let row = y * GFX125_SLICE_HASH_DIM;
        let k = y % num_ids;
        for x in 0..GFX125_SLICE_HASH_DIM {
            let l = x % num_ids;
            table[row + x] = phys_ids[(swzx[l] + swzy[k]) % num_ids] as u8;
        }
    }
}

fn gfx125_pack_slice_hash_tables(
    config: Gfx125SliceHashConfig,
    dwords: &mut [u32; GFX125_SLICE_HASH_TABLE_DWORDS],
) {
    let mut entries = [0u8; GFX125_SLICE_HASH_TABLE_ENTRIES];
    gfx125_compute_pixel_hash_table_nway(config.ppipe_mask1, config.ppipe_mask2, &mut entries);
    dwords.fill(0);

    for table_idx in 0..GFX125_SLICE_HASH_TABLES {
        let table_base = table_idx * GFX125_SLICE_HASH_TABLE_DWORDS_PER_TABLE;
        for (entry_idx, entry) in entries.iter().copied().enumerate() {
            let dword_idx = table_base + (entry_idx / 8);
            let shift = (entry_idx % 8) * 4;
            dwords[dword_idx] |= (entry as u32) << shift;
        }
    }
}

fn gfx125_3d_mode_dw1(config: Gfx125SliceHashConfig) -> u32 {
    config.cross_slice_hashing_mode | (0b11 << 16) | (1 << 6) | (1 << 22)
}

fn gfx125_3d_mode_dw3() -> u32 {
    // Keep RHWO disabled for bring-up so the first render proof does not depend
    // on an optimization state that Mesa conditionally toggles later.
    (1 << 15) | (1 << 31)
}

pub fn forcewake_render_sanity(warm: RenderWarmState) {
    let dev = crate::intel::Dev {
        bus: 0,
        slot: 0,
        function: 0,
        device_id: warm.device_id,
        revision_id: warm.revision_id,
        mmio: warm.mmio_base as *mut u8,
        mmio_len: warm.mmio_len,
    };
    let before = crate::intel::mmio_read(dev, RCS_RING_IMR);
    let toggled = before ^ 0x0000_0001;
    crate::intel::mmio_write(dev, RCS_RING_IMR, toggled);
    let after = crate::intel::mmio_read(dev, RCS_RING_IMR);
    crate::intel::mmio_write(dev, RCS_RING_IMR, before);
    let restored = crate::intel::mmio_read(dev, RCS_RING_IMR);
    crate::log!(
        "intel/render: sanity reg=RCS_IMR before=0x{:08X} wrote=0x{:08X} after=0x{:08X} restored=0x{:08X}\n",
        before,
        toggled,
        after,
        restored
    );
}

pub(crate) fn submit_primary_triangle_once() {
    if PRIMARY_TRIANGLE_SUBMITTED.swap(true, Ordering::AcqRel) {
        return;
    }

    let _ = submit_primary_probe_now("boot-once");
}

pub(crate) fn submit_primary_probe_periodic() {
    let _ = submit_primary_probe_now("periodic");
}

fn submit_primary_probe_now(reason: &'static str) -> bool {
    let probe_seq = PRIMARY_PROBE_SEQ.fetch_add(1, Ordering::AcqRel) + 1;
    if PRIMARY_PROBE_IN_FLIGHT
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        crate::log!("intel/render: primary-probe skipped reason=in-flight trigger={}\n", reason);
        return false;
    }

    if PRIMARY_DISABLE_RENDER_BRINGUP {
        crate::log!(
            "intel/render: primary-probe skipped reason=disabled trigger={} seq={}\n",
            reason,
            probe_seq
        );
        PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
        return false;
    }

    let Some(dev) = crate::intel::claimed_device() else {
        crate::log!("intel/render: primary-triangle skipped reason=no-device\n");
        PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
        return false;
    };
    let Some(surface_gpu) = crate::intel::display::primary_surface_gpu_addr() else {
        crate::log!("intel/render: primary-triangle skipped reason=no-surface\n");
        PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
        return false;
    };
    let Some((width, height)) = crate::intel::display::active_scanout_dimensions() else {
        crate::log!("intel/render: primary-triangle skipped reason=no-dimensions\n");
        PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
        return false;
    };
    let Some(pitch_bytes) = width
        .checked_mul(4)
        .and_then(|v| crate::intel::align_up(v as usize, 64))
    else {
        crate::log!("intel/render: primary-triangle skipped reason=bad-pitch width={}\n", width);
        PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
        return false;
    };

    let warm = warm_once(dev);
    if warm.ring_len == 0
        || warm.context_len == 0
        || warm.batch_len == 0
        || warm.draw_state_len == 0
        || warm.vertex_len == 0
        || warm.result_len == 0
        || warm.streamout_len == 0
    {
        crate::log!("intel/render: primary-triangle skipped reason=warm-buffers\n");
        PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
        return false;
    }
    if !forcewake_render_acquire(warm) {
        crate::log!("intel/render: primary-triangle skipped reason=forcewake\n");
        PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
        return false;
    }
    if !ensure_smoke_buffers_mapped(dev, warm) {
        crate::log!("intel/render: primary-triangle skipped reason=ggtt-map\n");
        PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
        return false;
    }
    let completed = if PRIMARY_USE_DRAW_PATH_BOOT_ONCE && reason == "boot-once" {
        let completed = submit_primary_triangle_with_retries(
            dev,
            warm,
            surface_gpu,
            pitch_bytes,
            width as usize,
            height as usize,
        );
        if !completed {
            crate::log!(
                "intel/render: primary-draw-path submit failed trigger={} mode=clean-boot-once\n",
                reason
            );
        }
        completed
    } else if PRIMARY_USE_MI_STRIPE_PROBE {
        let completed = submit_vertical_stripes_to_surface(
            dev,
            warm,
            surface_gpu,
            pitch_bytes,
            width as usize,
            height as usize,
        );
        if !completed {
            crate::log!("intel/render: primary-mi-stripes submit failed trigger={}\n", reason);
        }
        completed
    } else if PRIMARY_USE_3D_NO_DRAW_PROBE {
        let completed = submit_3d_no_draw_probe(dev, warm);
        if !completed {
            crate::log!("intel/render: primary-3d-no-draw submit failed trigger={}\n", reason);
        }
        completed
    } else if submit_primary_triangle_with_retries(
        dev,
        warm,
        surface_gpu,
        pitch_bytes,
        width as usize,
        height as usize,
    ) {
        true
    } else {
        let completed = submit_triangle_to_surface(
            dev,
            warm,
            surface_gpu,
            pitch_bytes,
            width as usize,
            height as usize,
        );
        if !completed {
            crate::log!("intel/render: primary-triangle submit failed trigger={}\n", reason);
        }
        completed
    };
    if should_log_primary_probe(reason, probe_seq) {
        crate::log!(
            "intel/render: primary-probe seq={} trigger={} completed={} mode={}\n",
            probe_seq,
            reason,
            completed as u8,
            if PRIMARY_USE_MI_STRIPE_PROBE {
                "mi-stripes"
            } else if PRIMARY_USE_DRAW_PATH_BOOT_ONCE && reason == "boot-once" {
                "draw-path"
            } else if PRIMARY_USE_3D_NO_DRAW_PROBE {
                "3d-no-draw"
            } else {
                "3d"
            }
        );
    }
    PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
    completed
}

fn submit_primary_triangle_with_retries(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    surface_gpu: u64,
    pitch_bytes: usize,
    width: usize,
    height: usize,
) -> bool {
    let streamout_experiment =
        select_streamout_proof_experiment(PRIMARY_PROBE_SEQ.load(Ordering::Acquire));
    let vf_streamout_precheck = submit_triangle_vf_streamout_proof(
        dev,
        warm,
        surface_gpu,
        pitch_bytes,
        width,
        height,
        streamout_experiment,
    );
    crate::log!(
        "intel/render: primary-vf-streamout-precheck experiment={} accepted={}\n",
        streamout_experiment.label(),
        vf_streamout_precheck as u8,
    );
    if !vf_streamout_precheck {
        return false;
    }

    let vs_streamout_precheck = submit_triangle_vs_streamout_proof(
        dev,
        warm,
        surface_gpu,
        pitch_bytes,
        width,
        height,
        streamout_experiment,
    );
    crate::log!(
        "intel/render: primary-vs-streamout-precheck experiment={} accepted={}\n",
        streamout_experiment.label(),
        vs_streamout_precheck as u8,
    );
    if !vs_streamout_precheck {
        return false;
    }

    let streamout_precheck = submit_triangle_streamout_proof(
        dev,
        warm,
        surface_gpu,
        pitch_bytes,
        width,
        height,
        streamout_experiment,
    );
    crate::log!(
        "intel/render: primary-streamout-precheck experiment={} accepted={}\n",
        streamout_experiment.label(),
        streamout_precheck as u8,
    );
    if !streamout_precheck {
        return false;
    }

    let mut completed_any = false;
    for attempt in 1..=PRIMARY_TRIANGLE_SUBMIT_ATTEMPTS {
        let blend_mode = TriangleBlendProbeMode::for_attempt(attempt);
        let completed = submit_triangle_draw_to_surface(
            dev,
            warm,
            surface_gpu,
            pitch_bytes,
            width,
            height,
            blend_mode,
        );
        crate::log!(
            "intel/render: primary-triangle attempt={}/{} target=0x{:X} blend_probe={} completed={}\n",
            attempt,
            PRIMARY_TRIANGLE_SUBMIT_ATTEMPTS,
            surface_gpu,
            blend_mode.label(),
            completed as u8
        );
        completed_any |= completed;
        if !completed {
            crate::log!(
                "intel/render: primary-streamout-proof skipped trigger=draw-fail attempt={} reason=post-hang-state-not-clean\n",
                attempt,
            );
            break;
        }
    }
    completed_any
}

fn submit_triangle_vf_streamout_proof(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    experiment: StreamoutProofExperiment,
) -> bool {
    let Some(draw) =
        prepare_vf_streamout_proof_resources(warm, dst_gpu_addr, pitch, rect_w, rect_h, experiment)
    else {
        crate::log!(
            "intel/render: vf-streamout-proof skipped reason=resource-layout size={}x{} pitch=0x{:X}\n",
            rect_w,
            rect_h,
            pitch
        );
        return false;
    };
    let slice_hash_table_offset = match write_vf_streamout_probe_state(warm) {
        Ok(offset) => offset,
        Err(reason) => {
            crate::log!(
                "intel/render: vf-streamout-proof skipped reason=probe-state detail={}\n",
                reason
            );
            return false;
        }
    };

    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
        core::ptr::write_bytes(warm.streamout_virt, 0, warm.streamout_len);
    }
    seed_result_debug_slots(warm);
    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    crate::intel::dma_flush(warm.streamout_virt, warm.streamout_len);

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let batch_tail_bytes = match encode_vf_streamout_proof_batch(
        batch,
        warm,
        draw,
        GPU_VA_RESULT_BASE,
        RCS_EXEC_RESULT_DRAW_PRE3D,
        RCS_EXEC_RESULT_DRAW_POST3D,
        RCS_EXEC_RESULT_DONE,
        experiment,
        slice_hash_table_offset,
    ) {
        Ok(bytes) => bytes,
        Err(reason) => {
            crate::log!(
                "intel/render: vf-streamout-proof batch build failed detail={}\n",
                reason
            );
            return false;
        }
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);
    crate::log!(
        "intel/render: vf-streamout-proof batch-ready experiment={} bytes=0x{:X} so_gpu=0x{:X} so_pitch={} vertices={}\n",
        experiment.label(),
        batch_tail_bytes,
        GPU_VA_STREAMOUT_BASE,
        experiment.vertex_bytes(),
        draw.vertex_count
    );

    let stats_before = capture_triangle_stage_stats(dev);
    let completed = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_DONE,
        RESULT_SLOT_FINAL_DWORD,
        "vf-streamout-proof",
    );
    let stats_after = capture_triangle_stage_stats(dev);
    let accepted = completed
        || maybe_soft_accept_streamout_submit(
            "vf-streamout-proof",
            warm,
            stats_before,
            stats_after,
            false,
            experiment.vertex_bytes() * draw.vertex_count as usize,
        );
    log_streamout_proof_result(
        "vf-streamout-proof",
        warm,
        completed,
        draw.vertex_count as usize,
        experiment,
    );
    if accepted && !completed {
        recover_render_engine_after_nonretired_submit(dev, warm, "vf-streamout-proof");
    }
    accepted
}

fn submit_triangle_vs_streamout_proof(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    experiment: StreamoutProofExperiment,
) -> bool {
    let Some(draw) = prepare_triangle_draw_resources(warm, dst_gpu_addr, pitch, rect_w, rect_h)
    else {
        crate::log!(
            "intel/render: vs-streamout-proof skipped reason=resource-layout size={}x{} pitch=0x{:X}\n",
            rect_w,
            rect_h,
            pitch
        );
        return false;
    };
    let pipeline = crate::intel::shader::triangle_pipeline();
    if crate::intel::shader::triangle_pipeline_is_placeholder() {
        crate::log!("intel/render: vs-streamout-proof skipped reason=placeholder-pipeline\n");
        return false;
    }
    let slice_hash_table_offset = match write_vf_streamout_probe_state(warm) {
        Ok(offset) => offset,
        Err(reason) => {
            crate::log!(
                "intel/render: vs-streamout-proof skipped reason=probe-state detail={}\n",
                reason
            );
            return false;
        }
    };
    let shader_layout = match upload_triangle_shader_pipeline(warm, pipeline) {
        Ok(layout) => layout,
        Err(reason) => {
            crate::log!(
                "intel/render: vs-streamout-proof skipped reason=shader-layout detail={}\n",
                reason
            );
            return false;
        }
    };
    if slice_hash_table_offset != 0
        && usize::try_from(shader_layout.used_bytes)
            .ok()
            .unwrap_or(usize::MAX)
            > slice_hash_table_offset as usize
    {
        crate::log!(
            "intel/render: vs-streamout-proof skipped reason=slice-hash-overlap used_end=0x{:X} slice_hash_off=0x{:X}\n",
            shader_layout.used_bytes,
            slice_hash_table_offset
        );
        return false;
    }

    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
        core::ptr::write_bytes(warm.streamout_virt, 0, warm.streamout_len);
    }
    seed_result_debug_slots(warm);
    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    crate::intel::dma_flush(warm.streamout_virt, warm.streamout_len);

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let batch_tail_bytes = match encode_vs_streamout_proof_batch(
        batch,
        warm,
        draw,
        GPU_VA_RESULT_BASE,
        RCS_EXEC_RESULT_DRAW_PRE3D,
        RCS_EXEC_RESULT_DRAW_POST3D,
        RCS_EXEC_RESULT_DONE,
        experiment,
        slice_hash_table_offset,
        VsStreamoutProofConfig {
            pipeline,
            shader_layout,
        },
    ) {
        Ok(bytes) => bytes,
        Err(reason) => {
            crate::log!(
                "intel/render: vs-streamout-proof batch build failed detail={}\n",
                reason
            );
            return false;
        }
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);
    crate::log!(
        "intel/render: vs-streamout-proof batch-ready experiment={} bytes=0x{:X} so_gpu=0x{:X} so_pitch={} vertices={}\n",
        experiment.label(),
        batch_tail_bytes,
        GPU_VA_STREAMOUT_BASE,
        experiment.vertex_bytes(),
        draw.vertex_count
    );

    let stats_before = capture_triangle_stage_stats(dev);
    let completed = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_DONE,
        RESULT_SLOT_FINAL_DWORD,
        "vs-streamout-proof",
    );
    let stats_after = capture_triangle_stage_stats(dev);
    let accepted = completed
        || maybe_soft_accept_streamout_submit(
            "vs-streamout-proof",
            warm,
            stats_before,
            stats_after,
            true,
            experiment.vertex_bytes() * draw.vertex_count as usize,
        );
    log_streamout_proof_result(
        "vs-streamout-proof",
        warm,
        completed,
        draw.vertex_count as usize,
        experiment,
    );
    if accepted && !completed {
        recover_render_engine_after_nonretired_submit(dev, warm, "vs-streamout-proof");
    }
    accepted
}

fn submit_triangle_streamout_proof(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    experiment: StreamoutProofExperiment,
) -> bool {
    let Some(draw) = prepare_triangle_draw_resources(warm, dst_gpu_addr, pitch, rect_w, rect_h)
    else {
        crate::log!(
            "intel/render: streamout-proof skipped reason=resource-layout size={}x{} pitch=0x{:X}\n",
            rect_w,
            rect_h,
            pitch
        );
        return false;
    };
    let pipeline = crate::intel::shader::triangle_pipeline();
    if crate::intel::shader::triangle_pipeline_is_placeholder() {
        crate::log!("intel/render: streamout-proof skipped reason=placeholder-pipeline\n");
        return false;
    }
    let shader_layout = match upload_triangle_shader_pipeline(warm, pipeline) {
        Ok(layout) => layout,
        Err(reason) => {
            crate::log!(
                "intel/render: streamout-proof skipped reason=shader-layout detail={}\n",
                reason
            );
            return false;
        }
    };
    let probe_state = match write_triangle_probe_state(
        warm,
        draw,
        shader_layout,
        TriangleBlendProbeMode::ExplicitRt0,
    ) {
        Ok(layout) => layout,
        Err(reason) => {
            crate::log!(
                "intel/render: streamout-proof skipped reason=probe-state detail={}\n",
                reason
            );
            return false;
        }
    };

    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
        core::ptr::write_bytes(warm.streamout_virt, 0, warm.streamout_len);
    }
    seed_result_debug_slots(warm);
    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    crate::intel::dma_flush(warm.streamout_virt, warm.streamout_len);

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let batch_tail_bytes = match encode_triangle_probe_batch(
        batch,
        warm,
        draw,
        TriangleBlendProbeMode::ExplicitRt0,
        pipeline,
        shader_layout,
        probe_state,
        GPU_VA_RESULT_BASE,
        RCS_EXEC_RESULT_DRAW_PRE3D,
        RCS_EXEC_RESULT_DRAW_POST3D,
        RCS_EXEC_RESULT_DONE,
        TriangleBatchMode::StreamoutProof,
        experiment,
    ) {
        Ok(bytes) => bytes,
        Err(reason) => {
            crate::log!("intel/render: streamout-proof batch build failed detail={}\n", reason);
            return false;
        }
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);
    crate::log!(
        "intel/render: streamout-proof batch-ready experiment={} bytes=0x{:X} so_gpu=0x{:X} so_pitch={} vertices={}\n",
        experiment.label(),
        batch_tail_bytes,
        GPU_VA_STREAMOUT_BASE,
        experiment.vertex_bytes(),
        draw.vertex_count
    );

    let stats_before = capture_triangle_stage_stats(dev);
    let completed = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_DONE,
        RESULT_SLOT_FINAL_DWORD,
        "streamout-proof",
    );
    let stats_after = capture_triangle_stage_stats(dev);
    let accepted = completed
        || maybe_soft_accept_streamout_submit(
            "streamout-proof",
            warm,
            stats_before,
            stats_after,
            true,
            experiment.vertex_bytes() * draw.vertex_count as usize,
        );
    log_streamout_proof_result(
        "streamout-proof",
        warm,
        completed,
        draw.vertex_count as usize,
        experiment,
    );
    if accepted && !completed {
        recover_render_engine_after_nonretired_submit(dev, warm, "streamout-proof");
    }
    accepted
}

fn wait_eq(dev: crate::intel::Dev, reg: usize, mask: u32, want: u32, n: usize) -> bool {
    for _ in 0..n {
        if (crate::intel::mmio_read(dev, reg) & mask) == want {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

fn map_smoke_buffers(dev: crate::intel::Dev, warm: RenderWarmState) -> bool {
    let ok_ring = super::map_ggtt(dev, warm.ring_phys, warm.ring_len, GPU_VA_RING_BASE);
    let ok_context = super::map_ggtt(dev, warm.context_phys, warm.context_len, GPU_VA_CONTEXT_BASE);
    let ok_batch = super::map_ggtt(dev, warm.batch_phys, warm.batch_len, GPU_VA_BATCH_BASE);
    let ok_draw_state =
        super::map_ggtt(dev, warm.draw_state_phys, warm.draw_state_len, GPU_VA_DRAW_STATE_BASE);
    let ok_vertex = super::map_ggtt(dev, warm.vertex_phys, warm.vertex_len, GPU_VA_VERTEX_BASE);
    let ok_result = super::map_ggtt(dev, warm.result_phys, warm.result_len, GPU_VA_RESULT_BASE);
    let ok_streamout =
        super::map_ggtt(dev, warm.streamout_phys, warm.streamout_len, GPU_VA_STREAMOUT_BASE);
    if ok_ring
        && ok_context
        && ok_batch
        && ok_draw_state
        && ok_vertex
        && ok_result
        && ok_streamout
    {
        super::ggtt_invalidate(dev);
        true
    } else {
        false
    }
}

fn ensure_smoke_buffers_mapped(dev: crate::intel::Dev, warm: RenderWarmState) -> bool {
    if WARM_BUFFERS_MAPPED.load(Ordering::Acquire) {
        return true;
    }
    if !map_smoke_buffers(dev, warm) {
        return false;
    }
    WARM_BUFFERS_MAPPED.store(true, Ordering::Release);
    true
}

fn should_log_primary_probe(reason: &str, seq: u32) -> bool {
    reason == "boot-once" || seq <= 3 || seq.is_multiple_of(PRIMARY_PERIODIC_LOG_EVERY)
}

fn should_log_primary_probe_detail() -> bool {
    let seq = PRIMARY_PROBE_SEQ.load(Ordering::Acquire);
    seq <= 3 || seq.is_multiple_of(PRIMARY_PERIODIC_LOG_EVERY)
}

fn submit_triangle_draw_to_surface(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    blend_mode: TriangleBlendProbeMode,
) -> bool {
    let Some(draw) = prepare_triangle_draw_resources(warm, dst_gpu_addr, pitch, rect_w, rect_h)
    else {
        crate::log!(
            "intel/render: draw-path staging skipped reason=resource-layout size={}x{} pitch=0x{:X}\n",
            rect_w,
            rect_h,
            pitch
        );
        return false;
    };

    let pipeline = crate::intel::shader::triangle_pipeline();
    log_render_buffer_layout(warm, Some(dst_gpu_addr));
    log_render_packet_encodings();
    if crate::intel::shader::triangle_pipeline_is_placeholder() {
        crate::log!(
            "intel/render: draw-path staged rt=0x{:X} vb=0x{:X} state=0x{:X} size={}x{} pitch=0x{:X} vertices={} stride={} status=awaiting-baked-shaders vs_src={} ps_src={} note={}\n",
            draw.rt_gpu_addr,
            draw.vertex_gpu_addr,
            draw.state_gpu_addr,
            draw.target_w,
            draw.target_h,
            draw.rt_pitch,
            draw.vertex_count,
            draw.vertex_stride,
            crate::intel::shader::TRIANGLE_VERTEX_SOURCE_PATH,
            crate::intel::shader::TRIANGLE_FRAGMENT_SOURCE_PATH,
            crate::intel::shader::triangle_pipeline_note()
        );
        return false;
    }

    crate::log!(
        "intel/render: ps-meta dispatch={:?} grf_start={} grf_used={} ksp_off=0x{:X} size={} header_only={} note={}\n",
        pipeline.ps.meta.kernel.dispatch_mode,
        pipeline.ps.meta.kernel.grf_start_register,
        pipeline.ps.meta.kernel.grf_used,
        pipeline.ps.meta.kernel.ksp_offset_bytes,
        pipeline.ps.meta.kernel.code_size_bytes,
        (pipeline.ps.meta.num_varying_inputs == 0
            && pipeline.ps.meta.kernel.push_constant_bytes == 0) as u8,
        crate::intel::shader::triangle_pipeline_note()
    );

    let shader_layout = match upload_triangle_shader_pipeline(warm, pipeline) {
        Ok(layout) => layout,
        Err(reason) => {
            crate::log!(
                "intel/render: draw-path staging skipped reason=shader-layout-error detail={} note={}\n",
                reason,
                crate::intel::shader::triangle_pipeline_note()
            );
            return false;
        }
    };

    crate::log!(
        "intel/render: draw-path staged rt=0x{:X} vb=0x{:X} state=0x{:X} used_end=0x{:X} state_off=0x{:X} state_region=0x{:X} free=0x{:X} size={}x{} pitch=0x{:X} vertices={} stride={} status=pipeline-ready vs_bytes={} vs_off=0x{:X} vs_gpu=0x{:X} vs_ksp_off=0x{:X} vs_ksp=0x{:X} ps_bytes={} ps_off=0x{:X} ps_gpu=0x{:X} ps_ksp_off=0x{:X} ps_ksp=0x{:X} varyings={} ps_dispatch={:?}\n",
        draw.rt_gpu_addr,
        draw.vertex_gpu_addr,
        draw.state_gpu_addr,
        shader_layout.used_bytes,
        shader_layout.state_region_offset_bytes,
        shader_layout.state_region_gpu_addr,
        warm.draw_state_len
            .saturating_sub(shader_layout.state_region_offset_bytes as usize),
        draw.target_w,
        draw.target_h,
        draw.rt_pitch,
        draw.vertex_count,
        draw.vertex_stride,
        shader_layout.vs.code_size_bytes,
        shader_layout.vs.code_offset_bytes,
        shader_layout.vs.code_gpu_addr,
        shader_layout.vs.ksp_offset_bytes,
        shader_layout.vs.ksp_gpu_addr,
        shader_layout.ps.code_size_bytes,
        shader_layout.ps.code_offset_bytes,
        shader_layout.ps.code_gpu_addr,
        shader_layout.ps.ksp_offset_bytes,
        shader_layout.ps.ksp_gpu_addr,
        pipeline.ps.meta.num_varying_inputs,
        pipeline.ps.meta.kernel.dispatch_mode
    );

    let probe_state = match write_triangle_probe_state(warm, draw, shader_layout, blend_mode) {
        Ok(layout) => layout,
        Err(reason) => {
            crate::log!(
                "intel/render: draw-path staging skipped reason=probe-state-error detail={}\n",
                reason
            );
            return false;
        }
    };

    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
    }
    seed_result_debug_slots(warm);
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let batch_tail_bytes = match encode_triangle_probe_batch(
        batch,
        warm,
        draw,
        blend_mode,
        pipeline,
        shader_layout,
        probe_state,
        GPU_VA_RESULT_BASE,
        RCS_EXEC_RESULT_DRAW_PRE3D,
        RCS_EXEC_RESULT_DRAW_POST3D,
        RCS_EXEC_RESULT_DONE,
        TriangleBatchMode::Draw,
        StreamoutProofExperiment::PositionSlot1,
    ) {
        Ok(bytes) => bytes,
        Err(reason) => {
            crate::log!(
                "intel/render: draw-path staging skipped reason=probe-batch-error detail={}\n",
                reason
            );
            return false;
        }
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);

    crate::log!(
        "intel/render: draw-path batch-ready bytes=0x{:X} bt_off=0x{:X} samp_off=0x{:X} blend_off=0x{:X} cc_state_off=0x{:X} cc_vp_off=0x{:X} sf_vp_off=0x{:X}\n",
        batch_tail_bytes,
        probe_state.binding_table_offset_bytes,
        probe_state.sampler_state_offset_bytes,
        probe_state.blend_state_offset_bytes,
        probe_state.color_calc_state_offset_bytes,
        probe_state.cc_viewport_offset_bytes,
        probe_state.sf_clip_viewport_offset_bytes
    );
    crate::log!("intel/render: draw-path blend-probe={}\n", blend_mode.label());
    log_triangle_probe_state(warm, shader_layout, probe_state);

    submit_warm_render_batch(dev, warm, RCS_EXEC_RESULT_DONE, RESULT_SLOT_FINAL_DWORD, "draw-path")
}

fn submit_result_store_probe(dev: crate::intel::Dev, warm: RenderWarmState) -> bool {
    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
        core::ptr::write_volatile(warm.result_virt as *mut u32, 0xC0DE_7700);
    }
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let Ok(batch_tail_bytes) =
        encode_result_store_probe_batch(batch, GPU_VA_RESULT_BASE, RCS_EXEC_RESULT_MI_PROBE_DONE)
    else {
        crate::log!("intel/render: mi-store-probe batch build failed\n");
        return false;
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);
    submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_MI_PROBE_DONE,
        RESULT_SLOT_PRE3D_DWORD,
        "mi-store-probe",
    )
}

fn submit_3d_no_draw_probe(dev: crate::intel::Dev, warm: RenderWarmState) -> bool {
    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
        core::ptr::write_volatile(warm.result_virt as *mut u32, 0xC0DE_7700);
        core::ptr::write_volatile((warm.result_virt as *mut u32).add(1), 0xC0DE_7700);
        core::ptr::write_volatile((warm.result_virt as *mut u32).add(2), 0xC0DE_7700);
    }
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let Ok(batch_tail_bytes) = encode_3d_no_draw_probe_batch(
        batch,
        warm,
        GPU_VA_RESULT_BASE + (RESULT_SLOT_POST3D_DWORD as u64) * 4,
        RCS_EXEC_RESULT_3D_NO_DRAW_DONE,
    ) else {
        crate::log!("intel/render: 3d-no-draw-probe batch build failed\n");
        return false;
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);
    submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_3D_NO_DRAW_DONE,
        RESULT_SLOT_POST3D_DWORD,
        "3d-no-draw",
    )
}

fn log_render_buffer_layout(warm: RenderWarmState, rt_gpu_addr: Option<u64>) {
    let rt_gpu_addr = rt_gpu_addr.unwrap_or(0);
    crate::log!(
        "intel/render: buffers ring phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} context phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} batch phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} result phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} streamout phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} state phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} vertex phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} rt_ggtt=0x{:X}\n",
        warm.ring_phys,
        GPU_VA_RING_BASE,
        warm.ring_len,
        warm.context_phys,
        GPU_VA_CONTEXT_BASE,
        warm.context_len,
        warm.batch_phys,
        GPU_VA_BATCH_BASE,
        warm.batch_len,
        warm.result_phys,
        GPU_VA_RESULT_BASE,
        warm.result_len,
        warm.streamout_phys,
        GPU_VA_STREAMOUT_BASE,
        warm.streamout_len,
        warm.draw_state_phys,
        GPU_VA_DRAW_STATE_BASE,
        warm.draw_state_len,
        warm.vertex_phys,
        GPU_VA_VERTEX_BASE,
        warm.vertex_len,
        rt_gpu_addr
    );
}

fn log_render_packet_encodings() {
    let (ctx_desc_lo, ctx_desc_hi) = build_execlist_context_descriptor(GPU_VA_CONTEXT_BASE);
    crate::log!(
        "intel/render: encodings mi_store_data_imm=0x{:08X} ctx_desc=0x{:08X}:0x{:08X} state_base_address=0x{:08X} pipe_control=0x{:08X} pc_post_sync_immediate=0x{:08X} pc_dest_ggtt=0x{:08X}\n",
        MI_STORE_DATA_IMM_GGTT_DW1,
        ctx_desc_hi,
        ctx_desc_lo,
        STATE_BASE_ADDRESS_CMD,
        PIPE_CONTROL_CMD,
        PIPE_CONTROL_POST_SYNC_WRITE_IMMEDIATE,
        PIPE_CONTROL_DEST_GGTT
    );
}
fn log_triangle_probe_state(
    warm: RenderWarmState,
    shader_layout: TriangleShaderLayout,
    probe_state: TriangleProbeStateLayout,
) {
    let dwords = unsafe {
        core::slice::from_raw_parts(warm.draw_state_virt as *const u32, warm.draw_state_len / 4)
    };
    let bt_ptr = probe_state
        .binding_table_offset_bytes
        .saturating_sub(shader_layout.state_region_offset_bytes);
    let bt_entry = dwords[probe_state.binding_table_offset_bytes as usize / 4];
    let surface = &dwords[probe_state.surface_state_offset_bytes as usize / 4
        ..probe_state.surface_state_offset_bytes as usize / 4 + 16];
    let blend = &dwords[probe_state.blend_state_offset_bytes as usize / 4
        ..probe_state.blend_state_offset_bytes as usize / 4 + 16];
    let color_calc = &dwords[probe_state.color_calc_state_offset_bytes as usize / 4
        ..probe_state.color_calc_state_offset_bytes as usize / 4 + 16];
    crate::log!(
        "intel/render: probe-state bt_off=0x{:X} bt_entry0=0x{:08X} surf_off=0x{:X} ps_ptr=bt:0x{:X} blend_ptr=0x{:X} cc_ptr=0x{:X}\n",
        probe_state.binding_table_offset_bytes,
        bt_entry,
        probe_state.surface_state_offset_bytes,
        bt_ptr,
        probe_state.blend_state_offset_bytes | 1,
        probe_state.color_calc_state_offset_bytes | 1
    );
    crate::log!(
        "intel/render: probe-surface d0=0x{:08X} d1=0x{:08X} d2=0x{:08X} d3=0x{:08X} d4=0x{:08X} d5=0x{:08X} d6=0x{:08X} d7=0x{:08X}\n",
        surface[0],
        surface[1],
        surface[2],
        surface[3],
        surface[4],
        surface[5],
        surface[6],
        surface[7]
    );
    crate::log!(
        "intel/render: probe-surface d8=0x{:08X} d9=0x{:08X} d10=0x{:08X} d11=0x{:08X} d12=0x{:08X} d13=0x{:08X} d14=0x{:08X} d15=0x{:08X}\n",
        surface[8],
        surface[9],
        surface[10],
        surface[11],
        surface[12],
        surface[13],
        surface[14],
        surface[15]
    );
    crate::log!(
        "intel/render: probe-blend d0=0x{:08X} d1=0x{:08X} d2=0x{:08X} d3=0x{:08X} d4=0x{:08X} d5=0x{:08X} d6=0x{:08X} d7=0x{:08X}\n",
        blend[0],
        blend[1],
        blend[2],
        blend[3],
        blend[4],
        blend[5],
        blend[6],
        blend[7]
    );
    crate::log!(
        "intel/render: probe-blend d8=0x{:08X} d9=0x{:08X} d10=0x{:08X} d11=0x{:08X} d12=0x{:08X} d13=0x{:08X} d14=0x{:08X} d15=0x{:08X}\n",
        blend[8],
        blend[9],
        blend[10],
        blend[11],
        blend[12],
        blend[13],
        blend[14],
        blend[15]
    );
    crate::log!(
        "intel/render: probe-cc d0=0x{:08X} d1=0x{:08X} d2=0x{:08X} d3=0x{:08X} d4=0x{:08X} d5=0x{:08X} d6=0x{:08X} d7=0x{:08X}\n",
        color_calc[0],
        color_calc[1],
        color_calc[2],
        color_calc[3],
        color_calc[4],
        color_calc[5],
        color_calc[6],
        color_calc[7]
    );
    crate::log!(
        "intel/render: probe-cc d8=0x{:08X} d9=0x{:08X} d10=0x{:08X} d11=0x{:08X} d12=0x{:08X} d13=0x{:08X} d14=0x{:08X} d15=0x{:08X}\n",
        color_calc[8],
        color_calc[9],
        color_calc[10],
        color_calc[11],
        color_calc[12],
        color_calc[13],
        color_calc[14],
        color_calc[15]
    );
}

fn write_triangle_probe_state(
    warm: RenderWarmState,
    draw: TriangleDrawPrep,
    shader_layout: TriangleShaderLayout,
    blend_mode: TriangleBlendProbeMode,
) -> Result<TriangleProbeStateLayout, &'static str> {
    let mut cursor = shader_layout.state_region_offset_bytes as usize;
    let binding_table_offset = cursor;
    cursor = crate::intel::align_up(binding_table_offset + 4, 64).ok_or("probe-state-align")?;
    let surface_state_offset = cursor;
    cursor = crate::intel::align_up(surface_state_offset + 64, 32).ok_or("probe-state-align")?;
    let sampler_state_offset = cursor;
    cursor = crate::intel::align_up(sampler_state_offset + 16, 64).ok_or("probe-state-align")?;
    let blend_state_offset = cursor;
    cursor = crate::intel::align_up(blend_state_offset + 64, 64).ok_or("probe-state-align")?;
    let color_calc_state_offset = cursor;
    cursor = crate::intel::align_up(color_calc_state_offset + 64, 64).ok_or("probe-state-align")?;
    let cc_viewport_offset = cursor;
    cursor = crate::intel::align_up(cc_viewport_offset + 8, 64).ok_or("probe-state-align")?;
    let sf_clip_viewport_offset = cursor;
    cursor = crate::intel::align_up(sf_clip_viewport_offset + 64, 64).ok_or("probe-state-align")?;
    let scissor_rect_offset = cursor;
    cursor = scissor_rect_offset
        .checked_add(8)
        .ok_or("probe-state-overflow")?;
    let slice_hash_table_offset = if device_is_gfx125(warm.device_id) {
        let offset = crate::intel::align_up(cursor, 64).ok_or("probe-state-align")?;
        cursor = offset
            .checked_add(GFX125_SLICE_HASH_TABLE_BYTES)
            .ok_or("probe-state-overflow")?;
        offset
    } else {
        0
    };
    let end_offset = cursor;
    if end_offset > warm.draw_state_len {
        return Err("probe-state-exceeds-state-bo");
    }

    let dwords = unsafe {
        core::slice::from_raw_parts_mut(warm.draw_state_virt as *mut u32, warm.draw_state_len / 4)
    };
    dwords[binding_table_offset / 4] = surface_state_offset as u32;

    let surface = &mut dwords[surface_state_offset / 4..surface_state_offset / 4 + 16];
    surface.fill(0);
    surface[0] = (SURFTYPE_2D << 29)
        | (SURFACE_FORMAT_B8G8R8A8_UNORM << 18)
        | (SURFACE_HALIGN_4 << 14)
        | (SURFACE_VALIGN_4 << 16);
    surface[1] = RENDER_MOCS << 24;
    surface[2] = draw.target_w.saturating_sub(1) | (draw.target_h.saturating_sub(1) << 16);
    surface[3] = draw.rt_pitch.saturating_sub(1);
    surface[7] = (SHADER_CHANNEL_ALPHA << 16)
        | (SHADER_CHANNEL_BLUE << 19)
        | (SHADER_CHANNEL_GREEN << 22)
        | (SHADER_CHANNEL_RED << 25);
    surface[8] = draw.rt_gpu_addr as u32;
    surface[9] = (draw.rt_gpu_addr >> 32) as u32;

    let sampler = &mut dwords[sampler_state_offset / 4..sampler_state_offset / 4 + 4];
    sampler.fill(0);

    let blend = &mut dwords[blend_state_offset / 4..blend_state_offset / 4 + 16];
    blend.fill(0);
    match blend_mode {
        // Keep the existing explicit RT0 setup as the baseline attempt.
        TriangleBlendProbeMode::ExplicitRt0 => {
            blend[0] = 0;
            blend[1] = (1 << 0) | (1 << 1) | (2 << 2);
        }
        // Mesa's trivial path mainly relies on PS_BLEND HasWriteableRT with a
        // boring zeroed blend-state payload.
        TriangleBlendProbeMode::MesaZeroedState
        | TriangleBlendProbeMode::MesaZeroedNoBlendPointer => {}
    }

    let color_calc = &mut dwords[color_calc_state_offset / 4..color_calc_state_offset / 4 + 16];
    color_calc.fill(0);

    let cc_viewport = &mut dwords[cc_viewport_offset / 4..cc_viewport_offset / 4 + 2];
    cc_viewport[0] = 0.0f32.to_bits();
    cc_viewport[1] = 1.0f32.to_bits();

    let sf_clip_viewport =
        &mut dwords[sf_clip_viewport_offset / 4..sf_clip_viewport_offset / 4 + 16];
    sf_clip_viewport.fill(0);
    sf_clip_viewport[0] = (draw.target_w as f32 * 0.5).to_bits();
    sf_clip_viewport[1] = (-(draw.target_h as f32) * 0.5).to_bits();
    sf_clip_viewport[2] = 1.0f32.to_bits();
    sf_clip_viewport[3] = (draw.target_w as f32 * 0.5).to_bits();
    sf_clip_viewport[4] = (draw.target_h as f32 * 0.5).to_bits();
    sf_clip_viewport[5] = 0.0f32.to_bits();
    sf_clip_viewport[8] = (-32768.0f32).to_bits();
    sf_clip_viewport[9] = 32768.0f32.to_bits();
    sf_clip_viewport[10] = (-32768.0f32).to_bits();
    sf_clip_viewport[11] = 32768.0f32.to_bits();

    let scissor_rect = &mut dwords[scissor_rect_offset / 4..scissor_rect_offset / 4 + 2];
    scissor_rect[0] = 0;
    scissor_rect[1] = draw.target_w.saturating_sub(1) | (draw.target_h.saturating_sub(1) << 16);

    if slice_hash_table_offset != 0 {
        let slice_hash = &mut dwords[slice_hash_table_offset / 4
            ..slice_hash_table_offset / 4 + GFX125_SLICE_HASH_TABLE_DWORDS];
        let mut packed = [0u32; GFX125_SLICE_HASH_TABLE_DWORDS];
        gfx125_pack_slice_hash_tables(gfx125_slice_hash_config(warm), &mut packed);
        slice_hash.copy_from_slice(&packed);
    }

    let flush_ptr = unsafe {
        warm.draw_state_virt
            .add(shader_layout.state_region_offset_bytes as usize)
    };
    crate::intel::dma_flush(
        flush_ptr,
        end_offset - shader_layout.state_region_offset_bytes as usize,
    );

    Ok(TriangleProbeStateLayout {
        binding_table_offset_bytes: binding_table_offset as u32,
        surface_state_offset_bytes: surface_state_offset as u32,
        sampler_state_offset_bytes: sampler_state_offset as u32,
        blend_state_offset_bytes: blend_state_offset as u32,
        color_calc_state_offset_bytes: color_calc_state_offset as u32,
        cc_viewport_offset_bytes: cc_viewport_offset as u32,
        sf_clip_viewport_offset_bytes: sf_clip_viewport_offset as u32,
        scissor_rect_offset_bytes: scissor_rect_offset as u32,
        slice_hash_table_offset_bytes: slice_hash_table_offset as u32,
    })
}

fn encode_triangle_probe_batch(
    batch_dwords: &mut [u32],
    warm: RenderWarmState,
    draw: TriangleDrawPrep,
    blend_mode: TriangleBlendProbeMode,
    pipeline: &'static crate::intel::shader::TrianglePipeline,
    shader_layout: TriangleShaderLayout,
    probe_state: TriangleProbeStateLayout,
    result_gpu_addr: u64,
    pre3d_value: u32,
    post3d_value: u32,
    done_value: u32,
    batch_mode: TriangleBatchMode,
    streamout_experiment: StreamoutProofExperiment,
) -> Result<usize, &'static str> {
    let mut cursor = 0usize;

    fn log_batch_offset(cursor: usize, label: &str) {
        if crate::logflag::INTEL_RENDER_NGIN_BATCH_LOGS {
            crate::log!(
                "intel/render: batch-off 0x{:03X} {}\n",
                cursor * core::mem::size_of::<u32>(),
                label
            );
        }
    }

    fn push(batch_dwords: &mut [u32], cursor: &mut usize, value: u32) -> Result<(), &'static str> {
        if *cursor >= batch_dwords.len() {
            return Err("probe-batch-exhausted");
        }
        batch_dwords[*cursor] = value;
        *cursor += 1;
        Ok(())
    }

    fn push_addr(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        value: u64,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, value as u32)?;
        push(batch_dwords, cursor, (value >> 32) as u32)
    }

    fn sampler_count_encoding(count: u8) -> u32 {
        match count {
            0 => 0,
            1..=4 => 1,
            5..=8 => 2,
            9..=12 => 3,
            _ => 4,
        }
    }

    fn binding_table_entry_count_encoding(count: u8) -> u32 {
        count.saturating_sub(1) as u32
    }

    fn stage_dispatch_bits(mode: crate::intel::shader::DispatchMode) -> (u32, u32, u32) {
        match mode {
            crate::intel::shader::DispatchMode::Simd8 => (1, 0, 0),
            crate::intel::shader::DispatchMode::Simd16 => (0, 1, 0),
            crate::intel::shader::DispatchMode::Simd32 => (0, 0, 1),
        }
    }

    fn push_pipe_control(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        flags: u32,
    ) -> Result<(), &'static str> {
        push_pipe_control_full(batch_dwords, cursor, 0, flags)
    }

    fn push_pipe_control_full(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        flags_dw0: u32,
        flags_dw1: u32,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, PIPE_CONTROL_CMD)?;
        push(batch_dwords, cursor, flags_dw1)?;
        if let Some(slot) = batch_dwords.get_mut(cursor.saturating_sub(2)) {
            *slot |= flags_dw0;
        } else {
            return Err("probe-pipe-control-header");
        }
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)
    }

    fn push_pipe_control_post_sync_imm(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        flags_dw0: u32,
        flags_dw1: u32,
        address: u64,
        value: u32,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, PIPE_CONTROL_CMD)?;
        push(batch_dwords, cursor, flags_dw1)?;
        if let Some(slot) = batch_dwords.get_mut(cursor.saturating_sub(2)) {
            *slot |= flags_dw0;
        } else {
            return Err("probe-pipe-control-header");
        }
        push(batch_dwords, cursor, address as u32)?;
        push(batch_dwords, cursor, (address >> 32) as u32)?;
        push(batch_dwords, cursor, value)?;
        push(batch_dwords, cursor, 0)
    }

    fn push_store_data_imm(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        address: u64,
        value: u32,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, MI_STORE_DATA_IMM_GGTT_DW1)?;
        push_addr(batch_dwords, cursor, address)?;
        push(batch_dwords, cursor, value)
    }

    fn push_load_register_imm(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        reg: usize,
        value: u32,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, mi_lri_cmd(1, MI_LRI_FORCE_POSTED))?;
        push(batch_dwords, cursor, reg as u32)?;
        push(batch_dwords, cursor, value)
    }

    fn push_sba_address(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        enable: bool,
        mocs: u32,
        address: u64,
    ) -> Result<(), &'static str> {
        let low = ((address as u32) & 0xFFFF_F000) | (mocs << 4) | u32::from(enable);
        push(batch_dwords, cursor, low)?;
        push(batch_dwords, cursor, (address >> 32) as u32)
    }

    fn push_sba_size(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        enable: bool,
        size_bytes: usize,
    ) -> Result<(), &'static str> {
        let size_bytes = crate::intel::align_up(size_bytes, 4096).ok_or("probe-sba-size-align")?;
        let size_bytes = u32::try_from(size_bytes).map_err(|_| "probe-sba-size-convert")?;
        push(batch_dwords, cursor, (size_bytes & 0xFFFF_F000) | u32::from(enable))
    }

    let binding_table_pool_size = warm
        .draw_state_len
        .saturating_sub(shader_layout.state_region_offset_bytes as usize);
    let vs_ksp_offset = shader_layout.vs.code_offset_bytes + shader_layout.vs.ksp_offset_bytes;
    let ps_ksp_offset = shader_layout.ps.code_offset_bytes + shader_layout.ps.ksp_offset_bytes;
    // Mesa still forces a one-slot URB read even when the PS consumes no
    // varyings. A zero read length appears legal on paper but is a strong
    // suspect for our "batch completes, no color writes" failure mode.
    let sbe_vertex_read_length =
        core::cmp::max((pipeline.ps.meta.num_varying_inputs as u32).div_ceil(2), 1);
    let sbe_dw1 = (1 << 5)
        | (1 << 21)
        | ((pipeline.ps.meta.num_varying_inputs as u32) << 22)
        | (1 << 28)
        | (1 << 29)
        | (sbe_vertex_read_length << 11);
    let (ps_dispatch_8, ps_dispatch_16, ps_dispatch_32) =
        stage_dispatch_bits(pipeline.ps.meta.kernel.dispatch_mode);
    // Keep CLIP close to Mesa's trivial path, but explicitly arm the clipper
    // counters for bring-up. ACCEPT_ALL keeps the unit logically enabled so
    // CL_INVOCATION/CL_PRIMITIVES can advance without depending on actual
    // clipping behaviour for this all-inside triangle.
    let clip_dw1 = 1 << 10;
    let clip_dw2 = CLIP_PERSPECTIVE_DIVIDE_DISABLE | CLIP_MODE_ACCEPT_ALL | (1 << 31);
    let clip_dw3 = 1 << 5;
    // Keep SF close to the trivial host path: viewport transform enabled,
    // gfx125 per-poly deref mode, and statistics armed so CL_PRIMITIVES_COUNT
    // is meaningful when the clipper is left enabled for debug.
    let sf_dw1 = (1 << 1) | (1 << 10);
    let sf_dw2 = 1 << 29;
    let sf_dw3 = 0;
    // Mirror Mesa's simple-shader path here as literally as possible: cull
    // none, and otherwise leave raster defaults boring until we have visual
    // proof that a more opinionated packet is required.
    let raster_dw1 = 1 << 16;
    let raster_dw2 = 0;
    let raster_dw3 = 0;
    let raster_dw4 = 0;
    // Mesa's simple-shader path emits a nearly all-default WM packet here.
    // Keep this dedicated triangle path equally boring rather than forcing
    // point-rule / line-AA bits that the host reference never asked for.
    let wm_dw1 = 1 << 31;
    let wm_depth_stencil_dw1 = 0;
    let wm_depth_stencil_dw2 = 0;
    let wm_depth_stencil_dw3 = 0;
    let wm_chroma_key_dw1 = 0;
    let ps_blend_dw1 = 1 << 30;
    let streamout_dw1 = (1 << 25) | (1 << 30) | (1 << 31);
    let streamout_dw2 = streamout_experiment.vertex_read_length();
    let streamout_dw3 = streamout_experiment.vertex_bytes() as u32;
    let streamout_dw4 = 0;
    let streamout_surface_size_dwords = (warm.streamout_len / 4).saturating_sub(1) as u32;
    let so_buffer_index_dw1 = (RENDER_MOCS << 22) | (1 << 21) | (1 << 31);
    let so_buffer_stream_offset_dw = 0u32;
    // Mesa zeros this packet during init to clear any inherited clear/resolve
    // overrides; do the same in the probe path so backend behaviour is fully
    // under our control.
    let wm_hz_op_dw1 = 0;
    let wm_hz_op_dw2 = 0;
    let wm_hz_op_dw3 = 0;
    let wm_hz_op_dw4 = 0;
    let gfx125_sample_pattern_dw = 0x8888_8888;
    let gfx125_slice_hash =
        device_is_gfx125(warm.device_id).then(|| gfx125_slice_hash_config(warm));
    let gfx125_3d_mode_dw1 = gfx125_slice_hash.map(gfx125_3d_mode_dw1).unwrap_or(0);
    let gfx125_3d_mode_dw2 = 0;
    let gfx125_3d_mode_dw3 = gfx125_3d_mode_dw3();
    let ps_dw3 =
        (binding_table_entry_count_encoding(pipeline.ps.meta.kernel.binding_table_entry_count)
            << 18)
            | (sampler_count_encoding(pipeline.ps.meta.kernel.sampler_count) << 27)
            | (u32::from(pipeline.ps.meta.uses_vmask) * PS_VECTOR_MASK_ENABLE);
    let ps_dw6 = ps_dispatch_8
        | (ps_dispatch_16 << 1)
        | (ps_dispatch_32 << 2)
        | (u32::from(pipeline.ps.meta.kernel.push_constant_bytes > 0) * PS_PUSH_CONSTANT_ENABLE)
        | ((TRIANGLE_PS_MAX_THREADS as u32) << PS_MAX_THREADS_SHIFT);
    let ps_dw7 = (pipeline.ps.meta.kernel.grf_start_register as u32)
        | ((pipeline.ps.meta.kernel.grf_start_register as u32) << 8)
        | ((pipeline.ps.meta.kernel.grf_start_register as u32) << 16);
    let ps_extra_dw1 = (u32::from(pipeline.ps.meta.computed_stencil)
        * PS_EXTRA_PIXEL_SHADER_COMPUTES_STENCIL)
        | (u32::from(pipeline.ps.meta.persample_dispatch) * PS_EXTRA_PIXEL_SHADER_IS_PER_SAMPLE)
        | (u32::from(pipeline.ps.meta.num_varying_inputs > 0) * PS_EXTRA_ATTRIBUTE_ENABLE)
        | ((pipeline.ps.meta.computed_depth_mode as u32) << 26)
        | PS_EXTRA_PIXEL_SHADER_VALID;

    batch_dwords.fill(0);

    log_batch_offset(cursor, "PIPE_CONTROL flush");
    push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_FLUSH_BITS)?;
    log_batch_offset(cursor, "PIPE_CONTROL invalidate");
    push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_INVALIDATE_BITS)?;

    log_batch_offset(cursor, "PIPELINE_SELECT");
    push(batch_dwords, &mut cursor, PIPELINE_SELECT_3D)?;

    if device_is_gfx125(warm.device_id) {
        let chicken_raster_2_value = gfx125_chicken_raster_2_value();
        log_batch_offset(cursor, "MI_LOAD_REGISTER_IMM CHICKEN_RASTER_2");
        push_load_register_imm(
            batch_dwords,
            &mut cursor,
            CHICKEN_RASTER_2,
            chicken_raster_2_value,
        )?;
        crate::log!(
            "intel/render: gfx125-raster-wa-batch chicken_raster_2=0x{:08X} tbimr_batch_override=1 tbimr_open_batch=1 tbimr_fast_clip=1\n",
            chicken_raster_2_value,
        );
    }

    log_batch_offset(cursor, "STATE_BASE_ADDRESS");
    push(batch_dwords, &mut cursor, STATE_BASE_ADDRESS_CMD)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push(batch_dwords, &mut cursor, 0)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_VERTEX_BASE)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.draw_state_len)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.draw_state_len)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.vertex_len)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.draw_state_len)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;

    log_batch_offset(cursor, "3DSTATE_AA_LINE_PARAMETERS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_AA_LINE_PARAMETERS)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;

    if device_is_gfx125(warm.device_id) {
        log_batch_offset(cursor, "3DSTATE_SAMPLE_PATTERN");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_SAMPLE_PATTERN)?;
        for _ in 0..8 {
            push(batch_dwords, &mut cursor, gfx125_sample_pattern_dw)?;
        }

        log_batch_offset(cursor, "3DSTATE_SLICE_TABLE_STATE_POINTERS");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_SLICE_TABLE_STATE_POINTERS)?;
        push(batch_dwords, &mut cursor, probe_state.slice_hash_table_offset_bytes | 1)?;

        log_batch_offset(cursor, "3DSTATE_3D_MODE");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_3D_MODE)?;
        push(batch_dwords, &mut cursor, gfx125_3d_mode_dw1)?;
        push(batch_dwords, &mut cursor, gfx125_3d_mode_dw2)?;
        push(batch_dwords, &mut cursor, gfx125_3d_mode_dw3)?;
        let slice_hash = gfx125_slice_hash.expect("gfx125 slice hash config");
        crate::log!(
            "intel/render: gfx125-svl-init sample_pattern=center slice_hash_ptr=0x{:X} geom_dss=0x{:08X} ppipe_dss={}/{}/{} mask1=0x{:X} mask2=0x{:X} mode_dw1=0x{:08X} mode_dw3=0x{:08X} cross_slice_mode={}({}) rhwo_disable=1\n",
            probe_state.slice_hash_table_offset_bytes,
            slice_hash.geometry_dss_enable,
            slice_hash.ppipe_subslices[0],
            slice_hash.ppipe_subslices[1],
            slice_hash.ppipe_subslices[2],
            slice_hash.ppipe_mask1,
            slice_hash.ppipe_mask2,
            gfx125_3d_mode_dw1,
            gfx125_3d_mode_dw3,
            slice_hash.cross_slice_hashing_mode,
            if slice_hash.cross_slice_hashing_mode == GFX125_3D_MODE_CROSS_SLICE_HASHING_32X32 {
                "hashing32x32"
            } else {
                "normal"
            },
        );
    }

    log_batch_offset(cursor, "3DSTATE_BINDING_TABLE_POOL_ALLOC");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_BINDING_TABLE_POOL_ALLOC)?;
    push_addr(batch_dwords, &mut cursor, shader_layout.state_region_gpu_addr)?;
    push(
        batch_dwords,
        &mut cursor,
        (u32::try_from(
            crate::intel::align_up(binding_table_pool_size, 4096)
                .ok_or("probe-binding-pool-align")?,
        )
        .map_err(|_| "probe-binding-pool-convert")?
            & 0xFFFF_F000),
    )?;

    log_batch_offset(cursor, "3DSTATE_SAMPLER_STATE_POINTERS_VS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_SAMPLER_STATE_POINTERS_VS)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_SAMPLER_STATE_POINTERS_PS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_SAMPLER_STATE_POINTERS_PS)?;
    push(batch_dwords, &mut cursor, probe_state.sampler_state_offset_bytes)?;

    let binding_table_pool_base_offset = shader_layout.state_region_offset_bytes;
    let binding_table_pointer_offset = probe_state
        .binding_table_offset_bytes
        .saturating_sub(binding_table_pool_base_offset);

    log_batch_offset(cursor, "3DSTATE_BINDING_TABLE_POINTERS_VS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_BINDING_TABLE_POINTERS_VS)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_BINDING_TABLE_POINTERS_HS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_BINDING_TABLE_POINTERS_HS)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_BINDING_TABLE_POINTERS_DS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_BINDING_TABLE_POINTERS_DS)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_BINDING_TABLE_POINTERS_GS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_BINDING_TABLE_POINTERS_GS)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_BINDING_TABLE_POINTERS_PS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_BINDING_TABLE_POINTERS_PS)?;
    push(batch_dwords, &mut cursor, binding_table_pointer_offset)?;

    log_batch_offset(cursor, "3DSTATE_VIEWPORT_STATE_POINTERS_CC");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VIEWPORT_STATE_POINTERS_CC)?;
    push(batch_dwords, &mut cursor, probe_state.cc_viewport_offset_bytes)?;
    log_batch_offset(cursor, "3DSTATE_VIEWPORT_STATE_POINTERS_SF_CLIP");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VIEWPORT_STATE_POINTERS_SF_CLIP)?;
    push(batch_dwords, &mut cursor, probe_state.sf_clip_viewport_offset_bytes)?;
    log_batch_offset(cursor, "3DSTATE_SCISSOR_STATE_POINTERS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_SCISSOR_STATE_POINTERS)?;
    push(batch_dwords, &mut cursor, probe_state.scissor_rect_offset_bytes)?;

    log_batch_offset(cursor, "3DSTATE_VERTEX_BUFFERS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VERTEX_BUFFERS_1)?;
    push(batch_dwords, &mut cursor, draw.vertex_stride | (1 << 14) | (RENDER_MOCS << 16))?;
    push_addr(batch_dwords, &mut cursor, draw.vertex_gpu_addr)?;
    push(batch_dwords, &mut cursor, draw.vertex_count.saturating_mul(draw.vertex_stride))?;

    log_batch_offset(cursor, "3DSTATE_VERTEX_ELEMENTS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VERTEX_ELEMENTS_1)?;
    push(batch_dwords, &mut cursor, (SURFACE_FORMAT_R32G32B32_FLOAT << 16) | (1 << 25))?;
    push(
        batch_dwords,
        &mut cursor,
        (VFCOMP_STORE_SRC << 28)
            | (VFCOMP_STORE_SRC << 24)
            | (VFCOMP_STORE_SRC << 20)
            | (VFCOMP_STORE_1_FP << 16),
    )?;

    log_batch_offset(cursor, "3DSTATE_VF_STATISTICS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_STATISTICS | 1)?;
    log_batch_offset(cursor, "3DSTATE_VF");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VF)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_VF_SGVS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_SGVS)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_VF_SGVS_2");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_SGVS_2)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_VF_INSTANCING");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_INSTANCING)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_VF_TOPOLOGY");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_TOPOLOGY)?;
    push(batch_dwords, &mut cursor, batch_mode.topology())?;
    log_batch_offset(cursor, "MI_STORE_DATA_IMM post-vf");
    push_store_data_imm(
        batch_dwords,
        &mut cursor,
        result_gpu_addr + (RESULT_SLOT_POST_VF_DWORD as u64) * 4,
        RCS_EXEC_RESULT_DRAW_POST_VF,
    )?;

    log_batch_offset(cursor, "3DSTATE_URB_ALLOC_HS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_URB_ALLOC_HS)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_URB_ALLOC_DS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_URB_ALLOC_DS)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_URB_ALLOC_GS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_URB_ALLOC_GS)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    let baked_vs_urb_output_length = pipeline.vs.meta.urb_entry_output_length;
    let programmed_vs_urb_output_length =
        TRIANGLE_VS_URB_OUTPUT_LENGTH_OVERRIDE.unwrap_or(baked_vs_urb_output_length);

    log_batch_offset(cursor, "3DSTATE_URB_ALLOC_VS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_URB_ALLOC_VS)?;
    push(
        batch_dwords,
        &mut cursor,
        // Gfx12 encodes URB allocation size as "size in 64B units minus 1".
        // A position-only VUE is one 64B slot, so the programmed value must
        // be 0 rather than 1 or clipper sees the wrong VS allocation contract.
        (programmed_vs_urb_output_length.saturating_sub(1) as u32)
            | (TRIANGLE_VS_URB_START << 10)
            | (TRIANGLE_VS_URB_START << 21),
    )?;
    push(batch_dwords, &mut cursor, TRIANGLE_VS_URB_ENTRIES | (TRIANGLE_VS_URB_ENTRIES << 16))?;

    let vs_dw3 = ((pipeline.vs.meta.kernel.binding_table_entry_count as u32) << 18)
        | (sampler_count_encoding(pipeline.vs.meta.kernel.sampler_count) << 27);
    let vs_dw6 = (1 << 11) | ((pipeline.vs.meta.kernel.grf_start_register as u32) << 20);
    let vs_dw7 = 1
        | (1 << 2)
        | (1 << 10)
        | (triangle_vs_max_threads_field(warm.device_id, pipeline.vs.meta.max_threads) << 22);
    let vs_dw8 = (programmed_vs_urb_output_length as u32) << 16;
    log_batch_offset(cursor, "3DSTATE_VS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VS)?;
    push(batch_dwords, &mut cursor, vs_ksp_offset & !0x3F)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, vs_dw3)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, vs_dw6)?;
    push(batch_dwords, &mut cursor, vs_dw7)?;
    push(batch_dwords, &mut cursor, vs_dw8)?;
    crate::log!(
        "intel/render: probe-vs ksp=0x{:08X} dw3=0x{:08X} dw6=0x{:08X} dw7=0x{:08X} dw8=0x{:08X} baked_max_threads={} applied_max_threads_field={} baked_urb_out_len={} programmed_urb_out_len={} grf_start={} dispatch={:?}\n",
        vs_ksp_offset & !0x3F,
        vs_dw3,
        vs_dw6,
        vs_dw7,
        vs_dw8,
        pipeline.vs.meta.max_threads,
        triangle_vs_max_threads_field(warm.device_id, pipeline.vs.meta.max_threads),
        baked_vs_urb_output_length,
        programmed_vs_urb_output_length,
        pipeline.vs.meta.kernel.grf_start_register,
        pipeline.vs.meta.kernel.dispatch_mode,
    );
    crate::log!(
        "intel/render: probe-vs-export note={} position_only={} generic_attrs=0 baked_urb_bytes={} programmed_urb_bytes={} expected_vue=header+position-only\n",
        crate::intel::shader::triangle_pipeline_note(),
        (pipeline.ps.meta.num_varying_inputs == 0) as u8,
        (baked_vs_urb_output_length as u32) * 64,
        (programmed_vs_urb_output_length as u32) * 64,
    );
    log_batch_offset(cursor, "MI_STORE_DATA_IMM post-vs");
    push_store_data_imm(
        batch_dwords,
        &mut cursor,
        result_gpu_addr + (RESULT_SLOT_POST_VS_DWORD as u64) * 4,
        RCS_EXEC_RESULT_DRAW_POST_VS,
    )?;

    log_batch_offset(cursor, "3DSTATE_HS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_HS)?;
    for _ in 0..8 {
        push(batch_dwords, &mut cursor, 0)?;
    }
    log_batch_offset(cursor, "3DSTATE_TE");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_TE)?;
    for _ in 0..4 {
        push(batch_dwords, &mut cursor, 0)?;
    }
    log_batch_offset(cursor, "3DSTATE_DS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_DS)?;
    for _ in 0..10 {
        push(batch_dwords, &mut cursor, 0)?;
    }
    log_batch_offset(cursor, "3DSTATE_STREAMOUT");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_STREAMOUT)?;
    if batch_mode.streamout_enabled() {
        push(batch_dwords, &mut cursor, streamout_dw1)?;
        push(batch_dwords, &mut cursor, streamout_dw2)?;
        push(batch_dwords, &mut cursor, streamout_dw3)?;
        push(batch_dwords, &mut cursor, streamout_dw4)?;

        log_batch_offset(cursor, "PIPE_CONTROL pre-so-buffer");
        push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_CS_STALL)?;
        log_batch_offset(cursor, "3DSTATE_SO_BUFFER_INDEX_0");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_SO_BUFFER_INDEX_0)?;
        push(batch_dwords, &mut cursor, so_buffer_index_dw1)?;
        push_addr(batch_dwords, &mut cursor, GPU_VA_STREAMOUT_BASE)?;
        push(batch_dwords, &mut cursor, streamout_surface_size_dwords)?;
        push_addr(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, so_buffer_stream_offset_dw)?;
        log_batch_offset(cursor, "PIPE_CONTROL post-so-buffer");
        push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_CS_STALL)?;

        log_batch_offset(cursor, "3DSTATE_SO_DECL_LIST");
        let streamout_decl_dword0 = streamout_experiment.so_decl_buffer_selects();
        let streamout_decl_dword1 = streamout_experiment.so_decl_num_entries();
        let [streamout_decl_dword2, streamout_decl_dword3, streamout_decl_dword4, streamout_decl_dword5] =
            streamout_experiment.so_decl_entry_dwords();
        push(batch_dwords, &mut cursor, streamout_experiment.so_decl_header())?;
        push(batch_dwords, &mut cursor, streamout_decl_dword0)?;
        push(batch_dwords, &mut cursor, streamout_decl_dword1)?;
        push(batch_dwords, &mut cursor, streamout_decl_dword2)?;
        push(batch_dwords, &mut cursor, streamout_decl_dword3)?;
        if matches!(streamout_experiment, StreamoutProofExperiment::HeaderAndPositionSlots01) {
            push(batch_dwords, &mut cursor, streamout_decl_dword4)?;
            push(batch_dwords, &mut cursor, streamout_decl_dword5)?;
        }
        crate::log!(
            "intel/render: probe-streamout-decl experiment={} read_len={} so_pitch={} decl=[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X}] vs_position_only={} ps_varyings={} generic_attrs=0 compatible={}\n",
            streamout_experiment.label(),
            streamout_experiment.vertex_read_length(),
            streamout_experiment.vertex_bytes(),
            streamout_decl_dword0,
            streamout_decl_dword1,
            streamout_decl_dword2,
            streamout_decl_dword3,
            streamout_decl_dword4,
            streamout_decl_dword5,
            (pipeline.ps.meta.num_varying_inputs == 0) as u8,
            pipeline.ps.meta.num_varying_inputs,
            streamout_experiment.compatible() as u8,
        );
        crate::log!(
            "intel/render: probe-streamout-config experiment={} so[function_enable={} statistics_enable={} rendering_disable={} render_stream={} reorder={} read_offset={} read_length_field={} buffer0_pitch={}] sobuf0[enable={} write_enable={} offset_addr_enable={} offset_mode={} mocs=0x{:X} surface=0x{:X} size_dwords=0x{:X} stream_offset=0x{:08X}] slot_contract=psiz-slot0 position-slot1\n",
            streamout_experiment.label(),
            (streamout_dw1 >> 31) & 0x1,
            (streamout_dw1 >> 25) & 0x1,
            (streamout_dw1 >> 30) & 0x1,
            (streamout_dw1 >> 27) & 0x3,
            (streamout_dw1 >> 26) & 0x1,
            (streamout_dw2 >> 5) & 0x1,
            streamout_dw2 & 0x1F,
            streamout_dw3 & 0xFFF,
            (so_buffer_index_dw1 >> 31) & 0x1,
            (so_buffer_index_dw1 >> 21) & 0x1,
            (so_buffer_index_dw1 >> 20) & 0x1,
            decode_streamout_offset_mode_name(
                (so_buffer_index_dw1 >> 21) & 0x1,
                (so_buffer_index_dw1 >> 20) & 0x1,
            ),
            (so_buffer_index_dw1 >> 22) & 0x7F,
            GPU_VA_STREAMOUT_BASE,
            streamout_surface_size_dwords,
            so_buffer_stream_offset_dw,
        );
        log_batch_offset(cursor, "PIPE_CONTROL post-so-decl");
        push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_CS_STALL)?;
    } else {
        for _ in 0..4 {
            push(batch_dwords, &mut cursor, 0)?;
        }
    }
    log_batch_offset(cursor, "3DSTATE_GS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_GS)?;
    for _ in 0..9 {
        push(batch_dwords, &mut cursor, 0)?;
    }

    // Program explicit null depth/stencil state instead of relying on any
    // inherited render context defaults before the first primitive launches.
    let depth_buffer_dw1 = (DEPTH_SURFACE_FORMAT_D32_FLOAT << 24) | (SURFTYPE_NULL << 29);
    let depth_buffer_dw5 = RENDER_MOCS;
    log_batch_offset(cursor, "3DSTATE_CLEAR_PARAMS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_CLEAR_PARAMS)?;
    push(batch_dwords, &mut cursor, 0.0f32.to_bits())?;
    push(batch_dwords, &mut cursor, 0)?;

    log_batch_offset(cursor, "3DSTATE_DEPTH_BUFFER");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_DEPTH_BUFFER)?;
    push(batch_dwords, &mut cursor, depth_buffer_dw1)?;
    push_addr(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, depth_buffer_dw5)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;

    log_batch_offset(cursor, "3DSTATE_STENCIL_BUFFER");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_STENCIL_BUFFER)?;
    push(batch_dwords, &mut cursor, SURFTYPE_NULL << 29)?;
    push_addr(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, RENDER_MOCS)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;

    log_batch_offset(cursor, "3DSTATE_HIER_DEPTH_BUFFER");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_HIER_DEPTH_BUFFER)?;
    push(batch_dwords, &mut cursor, RENDER_MOCS << 25)?;
    push_addr(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;

    log_batch_offset(cursor, "3DSTATE_CLIP");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_CLIP)?;
    push(batch_dwords, &mut cursor, clip_dw1)?;
    push(batch_dwords, &mut cursor, clip_dw2)?;
    push(batch_dwords, &mut cursor, clip_dw3)?;
    log_batch_offset(cursor, "MI_STORE_DATA_IMM post-clip");
    push_store_data_imm(
        batch_dwords,
        &mut cursor,
        result_gpu_addr + (RESULT_SLOT_POST_CLIP_DWORD as u64) * 4,
        RCS_EXEC_RESULT_DRAW_POST_CLIP,
    )?;

    log_batch_offset(cursor, "3DSTATE_SF");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_SF)?;
    push(batch_dwords, &mut cursor, sf_dw1)?;
    push(batch_dwords, &mut cursor, sf_dw2)?;
    push(batch_dwords, &mut cursor, sf_dw3)?;

    log_batch_offset(cursor, "3DSTATE_RASTER");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_RASTER)?;
    push(batch_dwords, &mut cursor, raster_dw1)?;
    push(batch_dwords, &mut cursor, raster_dw2)?;
    push(batch_dwords, &mut cursor, raster_dw3)?;
    push(batch_dwords, &mut cursor, raster_dw4)?;
    log_batch_offset(cursor, "MI_STORE_DATA_IMM post-raster");
    push_store_data_imm(
        batch_dwords,
        &mut cursor,
        result_gpu_addr + (RESULT_SLOT_POST_RASTER_DWORD as u64) * 4,
        RCS_EXEC_RESULT_DRAW_POST_RASTER,
    )?;

    log_batch_offset(cursor, "3DSTATE_SBE");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_SBE)?;
    push(batch_dwords, &mut cursor, sbe_dw1)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, pipeline.ps.meta.flat_inputs)?;
    push(batch_dwords, &mut cursor, SBE_ACTIVE_COMPONENT_XYZW_MASK_DWORD)?;
    push(batch_dwords, &mut cursor, SBE_ACTIVE_COMPONENT_XYZW_MASK_DWORD)?;

    // Gen12/Xe-LP keeps attribute swizzle state in a separate packet.
    log_batch_offset(cursor, "3DSTATE_SBE_SWIZ");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_SBE_SWIZ)?;
    for _ in 0..10 {
        push(batch_dwords, &mut cursor, 0)?;
    }

    log_batch_offset(cursor, "3DSTATE_WM");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_WM)?;
    push(batch_dwords, &mut cursor, wm_dw1)?;

    log_batch_offset(cursor, "3DSTATE_WM_DEPTH_STENCIL");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_WM_DEPTH_STENCIL)?;
    push(batch_dwords, &mut cursor, wm_depth_stencil_dw1)?;
    push(batch_dwords, &mut cursor, wm_depth_stencil_dw2)?;
    push(batch_dwords, &mut cursor, wm_depth_stencil_dw3)?;

    log_batch_offset(cursor, "3DSTATE_WM_CHROMA_KEY");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_WM_CHROMA_KEY)?;
    push(batch_dwords, &mut cursor, wm_chroma_key_dw1)?;

    // Match Mesa's gfx12 trivial path and avoid relying on inherited depth
    // bounds state from earlier firmware or display bring-up.
    log_batch_offset(cursor, "3DSTATE_DEPTH_BOUNDS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_DEPTH_BOUNDS)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0.0f32.to_bits())?;
    push(batch_dwords, &mut cursor, 1.0f32.to_bits())?;

    log_batch_offset(cursor, "3DSTATE_CC_STATE_POINTERS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_CC_STATE_POINTERS)?;
    push(batch_dwords, &mut cursor, probe_state.color_calc_state_offset_bytes | 1)?;

    log_batch_offset(cursor, "3DSTATE_BLEND_STATE_POINTERS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_BLEND_STATE_POINTERS)?;
    push(
        batch_dwords,
        &mut cursor,
        blend_mode.blend_state_pointer_dword(probe_state.blend_state_offset_bytes),
    )?;

    log_batch_offset(cursor, "3DSTATE_PS_BLEND");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_PS_BLEND)?;
    push(batch_dwords, &mut cursor, ps_blend_dw1)?;

    log_batch_offset(cursor, "3DSTATE_MULTISAMPLE");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_MULTISAMPLE)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_SAMPLE_MASK");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_SAMPLE_MASK)?;
    push(batch_dwords, &mut cursor, 1)?;

    // Clear inherited WM_HZ_OP clear/resolve overrides so PS dispatch only
    // depends on the explicit probe state we log below.
    log_batch_offset(cursor, "3DSTATE_WM_HZ_OP");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_WM_HZ_OP)?;
    push(batch_dwords, &mut cursor, wm_hz_op_dw1)?;
    push(batch_dwords, &mut cursor, wm_hz_op_dw2)?;
    push(batch_dwords, &mut cursor, wm_hz_op_dw3)?;
    push(batch_dwords, &mut cursor, wm_hz_op_dw4)?;

    log_batch_offset(cursor, "3DSTATE_PS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_PS)?;
    push(batch_dwords, &mut cursor, ps_ksp_offset & !0x3F)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, ps_dw3)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, ps_dw6)?;
    push(batch_dwords, &mut cursor, ps_dw7)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;

    log_batch_offset(cursor, "3DSTATE_PS_EXTRA");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_PS_EXTRA)?;
    push(batch_dwords, &mut cursor, ps_extra_dw1)?;
    log_batch_offset(cursor, "MI_STORE_DATA_IMM post-ps-state");
    push_store_data_imm(
        batch_dwords,
        &mut cursor,
        result_gpu_addr + (RESULT_SLOT_POST_PS_STATE_DWORD as u64) * 4,
        RCS_EXEC_RESULT_DRAW_POST_PS_STATE,
    )?;

    log_batch_offset(cursor, "3DSTATE_DRAWING_RECTANGLE");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_DRAWING_RECTANGLE)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(
        batch_dwords,
        &mut cursor,
        draw.target_w.saturating_sub(1) | (draw.target_h.saturating_sub(1) << 16),
    )?;
    push(batch_dwords, &mut cursor, 0)?;

    log_batch_offset(cursor, "MI_STORE_DATA_IMM pre-3d");
    push_store_data_imm(
        batch_dwords,
        &mut cursor,
        result_gpu_addr + (RESULT_SLOT_PRE3D_DWORD as u64) * 4,
        pre3d_value,
    )?;

    log_batch_offset(cursor, "3DPRIMITIVE");
    push(batch_dwords, &mut cursor, CMD_3DPRIMITIVE)?;
    push(batch_dwords, &mut cursor, batch_mode.topology())?;
    push(batch_dwords, &mut cursor, draw.vertex_count)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 1)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;

    log_batch_offset(cursor, "PIPE_CONTROL post-3d-eop-sync");
    push_pipe_control_post_sync_imm(
        batch_dwords,
        &mut cursor,
        0,
        PIPE_CONTROL_POST_DRAW_SYNC_BITS,
        result_gpu_addr + (RESULT_SLOT_POST3D_PIPE_CONTROL_LO_DWORD as u64) * 4,
        post3d_value,
    )?;

    log_batch_offset(cursor, "MI_STORE_DATA_IMM final");
    push_store_data_imm(
        batch_dwords,
        &mut cursor,
        result_gpu_addr + (RESULT_SLOT_FINAL_DWORD as u64) * 4,
        done_value,
    )?;
    log_batch_offset(cursor, "MI_BATCH_BUFFER_END");
    push(batch_dwords, &mut cursor, MI_BATCH_BUFFER_END)?;
    push(batch_dwords, &mut cursor, MI_NOOP)?;

    crate::log!(
        "intel/render: probe-3d sbe=0x{:08X} clip=[0x{:08X},0x{:08X},0x{:08X}] sf=[0x{:08X},0x{:08X},0x{:08X}] raster=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] wm=0x{:08X} ps3=0x{:08X} ps6=0x{:08X} ps7=0x{:08X} ps_extra=0x{:08X}\n",
        sbe_dw1,
        clip_dw1,
        clip_dw2,
        clip_dw3,
        sf_dw1,
        sf_dw2,
        sf_dw3,
        raster_dw1,
        raster_dw2,
        raster_dw3,
        raster_dw4,
        wm_dw1,
        ps_dw3,
        ps_dw6,
        ps_dw7,
        ps_extra_dw1
    );
    crate::log!(
        "intel/render: probe-backend ps_blend=0x{:08X} wm_depth=[0x{:08X},0x{:08X},0x{:08X}] wm_hz_op=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}]\n",
        ps_blend_dw1,
        wm_depth_stencil_dw1,
        wm_depth_stencil_dw2,
        wm_depth_stencil_dw3,
        wm_hz_op_dw1,
        wm_hz_op_dw2,
        wm_hz_op_dw3,
        wm_hz_op_dw4,
    );
    log_mesa_spec_cross_compare(
        warm,
        pipeline,
        sbe_dw1,
        clip_dw1,
        clip_dw2,
        sf_dw1,
        raster_dw1,
        ps_dw3,
        ps_dw6,
        ps_extra_dw1,
    );
    log_backend_dispatch_contract(
        wm_dw1,
        ps_blend_dw1,
        wm_depth_stencil_dw1,
        wm_depth_stencil_dw2,
        wm_depth_stencil_dw3,
        wm_hz_op_dw1,
        wm_hz_op_dw2,
        wm_hz_op_dw3,
        wm_hz_op_dw4,
        ps_extra_dw1,
    );
    let clip_mode = (clip_dw2 >> 13) & 0x7;
    let api_mode = (clip_dw2 >> 30) & 0x1;
    let provoking_tri_fan = clip_dw2 & 0x3;
    let provoking_line = (clip_dw2 >> 2) & 0x3;
    let provoking_tri_strip = (clip_dw2 >> 4) & 0x3;
    let guardband_enable = (clip_dw2 >> 26) & 0x1;
    let viewport_xy_clip_enable = (clip_dw2 >> 28) & 0x1;
    let clip_enable = (clip_dw2 >> 31) & 0x1;
    let force_clip_mode = ((clip_dw1 & CLIP_FORCE_CLIP_MODE) != 0) as u8;
    let early_cull_enable = (clip_dw1 >> 18) & 0x1;
    let statistics_enable = (clip_dw1 >> 10) & 0x1;
    let vertex_subpixel_precision = (clip_dw1 >> 19) & 0x1;
    let max_vp_idx = clip_dw3 & 0xF;
    let force_zero_rta_index = (clip_dw3 >> 5) & 0x1;
    crate::log!(
        "intel/render: probe-clip-decoded topo={} patchlist=0 gs_active=0 ClipMode={}({}) APIMode={}({}) GuardbandClipTestEnable={} ViewportXYClipTestEnable={} ClipEnable={} PerspectiveDivideDisable={} ForceClipMode={} EarlyCullEnable={} StatisticsEnable={} VertexSubPixelPrecisionSelect={} TriangleFanProvokingVertexSelect={} LineStripListProvokingVertexSelect={} TriangleStripListProvokingVertexSelect={} MaximumVPIndex={} ForceZeroRTAIndexEnable={}\n",
        primitive_topology_label(batch_mode.topology()),
        clip_mode,
        decode_clip_mode_name(clip_mode),
        api_mode,
        decode_api_mode_name(api_mode),
        guardband_enable,
        viewport_xy_clip_enable,
        clip_enable,
        ((clip_dw2 & CLIP_PERSPECTIVE_DIVIDE_DISABLE) != 0) as u8,
        force_clip_mode,
        early_cull_enable,
        statistics_enable,
        decode_vertex_subpixel_precision_name(vertex_subpixel_precision),
        provoking_tri_fan,
        provoking_line,
        provoking_tri_strip,
        max_vp_idx,
        force_zero_rta_index,
    );
    crate::log!(
        "intel/render: probe-sf-decoded ViewportTransformEnable={} StatisticsEnable={} LegacyGlobalDepthBiasEnable={} DerefBlockSize={}({}) LineWidth=0x{:X} PointWidth=0x{:X} LastPixelEnable={} TriangleStripListProvokingVertexSelect={} LineStripListProvokingVertexSelect={} TriangleFanProvokingVertexSelect={}\n",
        (sf_dw1 >> 1) & 0x1,
        (sf_dw1 >> 10) & 0x1,
        (sf_dw1 >> 11) & 0x1,
        (sf_dw2 >> 29) & 0x3,
        decode_deref_block_size_name((sf_dw2 >> 29) & 0x3),
        (sf_dw1 >> 12) & 0x3FFFF,
        sf_dw3 & 0x7FF,
        (sf_dw3 >> 31) & 0x1,
        (sf_dw3 >> 29) & 0x3,
        (sf_dw3 >> 27) & 0x3,
        (sf_dw3 >> 25) & 0x3,
    );
    crate::log!(
        "intel/render: probe-raster-decoded sf_viewport=0x{:X} cc_viewport=0x{:X} scissor_ptr=0x{:X} cull={} fill_front={} fill_back={} front={} scissor_enable={} aa_enable={} forced_samples={} sample_mask=0x1\n",
        probe_state.sf_clip_viewport_offset_bytes,
        probe_state.cc_viewport_offset_bytes,
        probe_state.scissor_rect_offset_bytes,
        decode_cull_mode_name((raster_dw1 >> 16) & 0x3),
        decode_fill_mode_name((raster_dw1 >> 5) & 0x3),
        decode_fill_mode_name((raster_dw1 >> 3) & 0x3),
        decode_front_winding_name((raster_dw1 >> 21) & 0x1),
        (raster_dw1 >> 1) & 0x1,
        (raster_dw1 >> 2) & 0x1,
        (raster_dw1 >> 18) & 0x7,
    );
    crate::log!(
        "intel/render: probe-handoff-decoded clip_out=sf vue_in_urb=1 baked_vs_urb_out_len={} programmed_vs_urb_out_len={} sbe_read_len={} ps_varyings={} streamout={}\n",
        baked_vs_urb_output_length,
        programmed_vs_urb_output_length,
        sbe_vertex_read_length,
        pipeline.ps.meta.num_varying_inputs,
        batch_mode.streamout_enabled() as u8,
    );
    crate::log!(
        "intel/render: 3dprimitive-setup mode={:?} topo={} vertices={} start_vertex=0 instances={} start_instance=0 base_vertex=0 vb=0x{:X} stride={} rt=0x{:X} pitch=0x{:X} rect={}x{}\n",
        batch_mode,
        primitive_topology_label(batch_mode.topology()),
        draw.vertex_count,
        1,
        draw.vertex_gpu_addr,
        draw.vertex_stride,
        draw.rt_gpu_addr,
        draw.rt_pitch,
        draw.target_w,
        draw.target_h
    );

    Ok(cursor * core::mem::size_of::<u32>())
}

fn encode_minimal_streamout_proof_batch(
    batch_dwords: &mut [u32],
    warm: RenderWarmState,
    draw: TriangleDrawPrep,
    result_gpu_addr: u64,
    pre3d_value: u32,
    post3d_value: u32,
    done_value: u32,
    streamout_experiment: StreamoutProofExperiment,
    slice_hash_table_offset_bytes: u32,
    vs_config: Option<VsStreamoutProofConfig>,
) -> Result<usize, &'static str> {
    let mut cursor = 0usize;
    let batch_mode = if vs_config.is_some() {
        TriangleBatchMode::VsStreamoutProof
    } else {
        TriangleBatchMode::VfStreamoutProof
    };
    let submit_label = if vs_config.is_some() {
        "vs-streamout-proof"
    } else {
        "vf-streamout-proof"
    };

    fn push(batch_dwords: &mut [u32], cursor: &mut usize, value: u32) -> Result<(), &'static str> {
        if *cursor >= batch_dwords.len() {
            return Err("vf-streamout-batch-exhausted");
        }
        batch_dwords[*cursor] = value;
        *cursor += 1;
        Ok(())
    }

    fn push_addr(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        value: u64,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, value as u32)?;
        push(batch_dwords, cursor, (value >> 32) as u32)
    }

    fn push_store_data_imm(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        address: u64,
        value: u32,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, MI_STORE_DATA_IMM_GGTT_DW1)?;
        push_addr(batch_dwords, cursor, address)?;
        push(batch_dwords, cursor, value)
    }

    fn push_pipe_control(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        flags: u32,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, PIPE_CONTROL_CMD)?;
        push(batch_dwords, cursor, flags)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)
    }

    fn push_pipe_control_post_sync_imm(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        flags: u32,
        address: u64,
        value: u32,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, PIPE_CONTROL_CMD)?;
        push(batch_dwords, cursor, flags)?;
        push(batch_dwords, cursor, address as u32)?;
        push(batch_dwords, cursor, (address >> 32) as u32)?;
        push(batch_dwords, cursor, value)?;
        push(batch_dwords, cursor, 0)
    }

    fn push_load_register_imm(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        reg: usize,
        value: u32,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, mi_lri_cmd(1, MI_LRI_FORCE_POSTED))?;
        push(batch_dwords, cursor, reg as u32)?;
        push(batch_dwords, cursor, value)
    }

    fn push_sba_address(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        enable: bool,
        mocs: u32,
        address: u64,
    ) -> Result<(), &'static str> {
        let low = ((address as u32) & 0xFFFF_F000) | (mocs << 4) | u32::from(enable);
        push(batch_dwords, cursor, low)?;
        push(batch_dwords, cursor, (address >> 32) as u32)
    }

    fn push_sba_size(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        enable: bool,
        size_bytes: usize,
    ) -> Result<(), &'static str> {
        let size_bytes =
            crate::intel::align_up(size_bytes, 4096).ok_or("vf-streamout-sba-align")?;
        let size_bytes = u32::try_from(size_bytes).map_err(|_| "vf-streamout-sba-convert")?;
        push(batch_dwords, cursor, (size_bytes & 0xFFFF_F000) | u32::from(enable))
    }

    fn sampler_count_encoding(count: u8) -> u32 {
        match count {
            0 => 0,
            1..=4 => 1,
            5..=8 => 2,
            9..=12 => 3,
            _ => 4,
        }
    }

    fn log_batch_offset(cursor: usize, label: &str) {
        crate::log!(
            "intel/render: batch-off 0x{:03X} {}\n",
            cursor * core::mem::size_of::<u32>(),
            label
        );
    }

    fn cmd_3dstate_vertex_buffers(count: usize) -> Result<u32, &'static str> {
        let body_dwords = count
            .checked_mul(4)
            .and_then(|n| n.checked_sub(1))
            .ok_or("vf-streamout-vb-count-overflow")?;
        let body_dwords =
            u32::try_from(body_dwords).map_err(|_| "vf-streamout-vb-count-convert")?;
        Ok(body_dwords | (8 << 16) | (3 << 27) | (3 << 29))
    }

    fn cmd_3dstate_vertex_elements(count: usize) -> Result<u32, &'static str> {
        let body_dwords = count
            .checked_mul(2)
            .and_then(|n| n.checked_sub(1))
            .ok_or("vf-streamout-ve-count-overflow")?;
        let body_dwords =
            u32::try_from(body_dwords).map_err(|_| "vf-streamout-ve-count-convert")?;
        Ok(body_dwords | (9 << 16) | (3 << 27) | (3 << 29))
    }

    fn push_vertex_buffer_state(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        vertex_buffer_index: u32,
        pitch: u32,
        start_addr: u64,
        size_bytes: u32,
    ) -> Result<(), &'static str> {
        push(
            batch_dwords,
            cursor,
            (pitch & 0xFFF)
                | (1 << 14)
                | (RENDER_MOCS << 16)
                | (1 << 25)
                | (vertex_buffer_index << 26),
        )?;
        push_addr(batch_dwords, cursor, start_addr)?;
        push(batch_dwords, cursor, size_bytes)
    }

    fn push_vertex_element_state(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        vertex_buffer_index: u32,
        source_offset: u32,
        source_format: u32,
        component0: u32,
        component1: u32,
        component2: u32,
        component3: u32,
    ) -> Result<(), &'static str> {
        push(
            batch_dwords,
            cursor,
            (source_offset & 0xFFF)
                | (source_format << 16)
                | (1 << 25)
                | (vertex_buffer_index << 26),
        )?;
        push(
            batch_dwords,
            cursor,
            (component0 << 28) | (component1 << 24) | (component2 << 20) | (component3 << 16),
        )
    }

    let streamout_surface_size_dwords = (warm.streamout_len / 4).saturating_sub(1) as u32;
    let streamout_dw1 = (1 << 25) | (1 << 30) | (1 << 31);
    let streamout_dw2 = streamout_experiment.vertex_read_length();
    let streamout_dw3 = streamout_experiment.vertex_bytes() as u32;
    let streamout_dw4 = 0u32;
    let so_buffer_index_dw1 = (RENDER_MOCS << 22) | (1 << 21) | (1 << 31);
    let sbe_dw1 = (1 << 5) | (1 << 11) | (1 << 21) | (1 << 22) | (1 << 28) | (1 << 29);
    let programmed_vs_urb_output_length = vs_config
        .map(|config| {
            TRIANGLE_VS_URB_OUTPUT_LENGTH_OVERRIDE
                .unwrap_or(config.pipeline.vs.meta.urb_entry_output_length)
        })
        .unwrap_or(1);
    let urb_vs_alloc_dw1 = (programmed_vs_urb_output_length.saturating_sub(1) as u32)
        | (TRIANGLE_VS_URB_START << 10)
        | (TRIANGLE_VS_URB_START << 21);
    let urb_vs_alloc_dw2 = TRIANGLE_VS_URB_ENTRIES | (TRIANGLE_VS_URB_ENTRIES << 16);
    let gfx125_sample_pattern_dw = 0x8888_8888;
    let gfx125_slice_hash =
        device_is_gfx125(warm.device_id).then(|| gfx125_slice_hash_config(warm));
    let gfx125_3d_mode_dw1 = gfx125_slice_hash.map(gfx125_3d_mode_dw1).unwrap_or(0);
    let gfx125_3d_mode_dw3 = gfx125_3d_mode_dw3();
    let vb_size_bytes = draw.vertex_count.saturating_mul(draw.vertex_stride);
    let vb_cmd = cmd_3dstate_vertex_buffers(1)?;
    let ve_cmd = cmd_3dstate_vertex_elements(if vs_config.is_some() { 1 } else { 2 })?;

    batch_dwords.fill(0);

    log_batch_offset(cursor, "PIPE_CONTROL flush");
    push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_FLUSH_BITS)?;
    log_batch_offset(cursor, "PIPE_CONTROL invalidate");
    push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_INVALIDATE_BITS)?;

    log_batch_offset(cursor, "PIPELINE_SELECT");
    push(batch_dwords, &mut cursor, PIPELINE_SELECT_3D)?;

    if device_is_gfx125(warm.device_id) {
        let chicken_raster_2_value = gfx125_chicken_raster_2_value();
        log_batch_offset(cursor, "MI_LOAD_REGISTER_IMM CHICKEN_RASTER_2");
        push_load_register_imm(
            batch_dwords,
            &mut cursor,
            CHICKEN_RASTER_2,
            chicken_raster_2_value,
        )?;
        crate::log!(
            "intel/render: gfx125-raster-wa-batch chicken_raster_2=0x{:08X} tbimr_batch_override=1 tbimr_open_batch=1 tbimr_fast_clip=1\n",
            chicken_raster_2_value,
        );
    }

    log_batch_offset(cursor, "STATE_BASE_ADDRESS");
    push(batch_dwords, &mut cursor, STATE_BASE_ADDRESS_CMD)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push(batch_dwords, &mut cursor, 0)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_VERTEX_BASE)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.draw_state_len)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.draw_state_len)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.vertex_len)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.draw_state_len)?;
    for _ in 0..6 {
        push(batch_dwords, &mut cursor, 0)?;
    }

    if device_is_gfx125(warm.device_id) {
        log_batch_offset(cursor, "3DSTATE_SAMPLE_PATTERN");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_SAMPLE_PATTERN)?;
        for _ in 0..8 {
            push(batch_dwords, &mut cursor, gfx125_sample_pattern_dw)?;
        }

        log_batch_offset(cursor, "3DSTATE_SLICE_TABLE_STATE_POINTERS");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_SLICE_TABLE_STATE_POINTERS)?;
        push(
            batch_dwords,
            &mut cursor,
            slice_hash_table_offset_bytes | u32::from(slice_hash_table_offset_bytes != 0),
        )?;

        log_batch_offset(cursor, "3DSTATE_3D_MODE");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_3D_MODE)?;
        push(batch_dwords, &mut cursor, gfx125_3d_mode_dw1)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, gfx125_3d_mode_dw3)?;
        let slice_hash = gfx125_slice_hash.expect("gfx125 slice hash config");
        crate::log!(
            "intel/render: gfx125-svl-init sample_pattern=center slice_hash_ptr=0x{:X} geom_dss=0x{:08X} ppipe_dss={}/{}/{} mask1=0x{:X} mask2=0x{:X} mode_dw1=0x{:08X} mode_dw3=0x{:08X} cross_slice_mode={}({}) rhwo_disable=1\n",
            slice_hash_table_offset_bytes,
            slice_hash.geometry_dss_enable,
            slice_hash.ppipe_subslices[0],
            slice_hash.ppipe_subslices[1],
            slice_hash.ppipe_subslices[2],
            slice_hash.ppipe_mask1,
            slice_hash.ppipe_mask2,
            gfx125_3d_mode_dw1,
            gfx125_3d_mode_dw3,
            slice_hash.cross_slice_hashing_mode,
            if slice_hash.cross_slice_hashing_mode == GFX125_3D_MODE_CROSS_SLICE_HASHING_32X32 {
                "hashing32x32"
            } else {
                "normal"
            },
        );
    }

    log_batch_offset(cursor, "3DSTATE_VF_INSTANCING");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_INSTANCING)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_VF_STATISTICS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_STATISTICS | 1)?;
    log_batch_offset(cursor, "3DSTATE_VF");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VF)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_VF_SGVS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_SGVS)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_VF_SGVS_2");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_SGVS_2)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;

    log_batch_offset(cursor, "3DSTATE_VERTEX_BUFFERS");
    push(batch_dwords, &mut cursor, vb_cmd)?;
    push_vertex_buffer_state(
        batch_dwords,
        &mut cursor,
        0,
        draw.vertex_stride,
        draw.vertex_gpu_addr,
        vb_size_bytes,
    )?;

    log_batch_offset(cursor, "3DSTATE_VERTEX_ELEMENTS");
    push(batch_dwords, &mut cursor, ve_cmd)?;
    if vs_config.is_some() {
        push_vertex_element_state(
            batch_dwords,
            &mut cursor,
            0,
            0,
            SURFACE_FORMAT_R32G32B32_FLOAT,
            VFCOMP_STORE_SRC,
            VFCOMP_STORE_SRC,
            VFCOMP_STORE_SRC,
            VFCOMP_STORE_1_FP,
        )?;
    } else {
        match streamout_experiment {
            StreamoutProofExperiment::PositionSlot1 => {
                push_vertex_element_state(
                    batch_dwords,
                    &mut cursor,
                    0,
                    0,
                    SURFACE_FORMAT_R32G32B32A32_UINT,
                    VFCOMP_STORE_0,
                    VFCOMP_STORE_0,
                    VFCOMP_STORE_0,
                    VFCOMP_STORE_0,
                )?;
                push_vertex_element_state(
                    batch_dwords,
                    &mut cursor,
                    0,
                    0,
                    SURFACE_FORMAT_R32G32B32A32_UINT,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                )?;
            }
            StreamoutProofExperiment::HeaderAndPositionSlots01 => {
                push_vertex_element_state(
                    batch_dwords,
                    &mut cursor,
                    0,
                    0,
                    SURFACE_FORMAT_R32G32B32A32_UINT,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                )?;
                push_vertex_element_state(
                    batch_dwords,
                    &mut cursor,
                    0,
                    16,
                    SURFACE_FORMAT_R32G32B32A32_UINT,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                )?;
            }
        }
    }

    log_batch_offset(cursor, "3DSTATE_VF_TOPOLOGY");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_TOPOLOGY)?;
    push(batch_dwords, &mut cursor, batch_mode.topology())?;
    log_batch_offset(cursor, "MI_STORE_DATA_IMM post-vf");
    push_store_data_imm(
        batch_dwords,
        &mut cursor,
        result_gpu_addr + (RESULT_SLOT_POST_VF_DWORD as u64) * 4,
        RCS_EXEC_RESULT_DRAW_POST_VF,
    )?;

    log_batch_offset(cursor, "3DSTATE_URB_ALLOC_HS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_URB_ALLOC_HS)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_URB_ALLOC_DS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_URB_ALLOC_DS)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_URB_ALLOC_GS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_URB_ALLOC_GS)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_URB_ALLOC_VS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_URB_ALLOC_VS)?;
    push(batch_dwords, &mut cursor, urb_vs_alloc_dw1)?;
    push(batch_dwords, &mut cursor, urb_vs_alloc_dw2)?;

    if let Some(config) = vs_config {
        let pipeline = config.pipeline;
        let shader_layout = config.shader_layout;
        let vs_ksp_offset = shader_layout.vs.code_offset_bytes + shader_layout.vs.ksp_offset_bytes;
        let baked_vs_urb_output_length = pipeline.vs.meta.urb_entry_output_length;
        let vs_dw3 = ((pipeline.vs.meta.kernel.binding_table_entry_count as u32) << 18)
            | (sampler_count_encoding(pipeline.vs.meta.kernel.sampler_count) << 27);
        let vs_dw6 = (1 << 11) | ((pipeline.vs.meta.kernel.grf_start_register as u32) << 20);
        let vs_dw7 = 1
            | (1 << 2)
            | (1 << 10)
            | (triangle_vs_max_threads_field(warm.device_id, pipeline.vs.meta.max_threads) << 22);
        let vs_dw8 = (programmed_vs_urb_output_length as u32) << 16;
        log_batch_offset(cursor, "3DSTATE_VS");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_VS)?;
        push(batch_dwords, &mut cursor, vs_ksp_offset & !0x3F)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, vs_dw3)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, vs_dw6)?;
        push(batch_dwords, &mut cursor, vs_dw7)?;
        push(batch_dwords, &mut cursor, vs_dw8)?;
        crate::log!(
            "intel/render: probe-vs ksp=0x{:08X} dw3=0x{:08X} dw6=0x{:08X} dw7=0x{:08X} dw8=0x{:08X} baked_max_threads={} applied_max_threads_field={} baked_urb_out_len={} programmed_urb_out_len={} grf_start={} dispatch={:?}\n",
            vs_ksp_offset & !0x3F,
            vs_dw3,
            vs_dw6,
            vs_dw7,
            vs_dw8,
            pipeline.vs.meta.max_threads,
            triangle_vs_max_threads_field(warm.device_id, pipeline.vs.meta.max_threads),
            baked_vs_urb_output_length,
            programmed_vs_urb_output_length,
            pipeline.vs.meta.kernel.grf_start_register,
            pipeline.vs.meta.kernel.dispatch_mode,
        );
        crate::log!(
            "intel/render: probe-vs-export note={} position_only={} generic_attrs=0 baked_urb_bytes={} programmed_urb_bytes={} expected_vue=header+position-only\n",
            crate::intel::shader::triangle_pipeline_note(),
            (pipeline.ps.meta.num_varying_inputs == 0) as u8,
            (baked_vs_urb_output_length as u32) * 64,
            (programmed_vs_urb_output_length as u32) * 64,
        );
    } else {
        log_batch_offset(cursor, "3DSTATE_VS disabled");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_VS)?;
        for _ in 0..8 {
            push(batch_dwords, &mut cursor, 0)?;
        }
    }
    log_batch_offset(cursor, "MI_STORE_DATA_IMM post-vs");
    push_store_data_imm(
        batch_dwords,
        &mut cursor,
        result_gpu_addr + (RESULT_SLOT_POST_VS_DWORD as u64) * 4,
        RCS_EXEC_RESULT_DRAW_POST_VS,
    )?;

    log_batch_offset(cursor, "3DSTATE_HS disabled");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_HS)?;
    for _ in 0..8 {
        push(batch_dwords, &mut cursor, 0)?;
    }
    log_batch_offset(cursor, "3DSTATE_TE disabled");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_TE)?;
    for _ in 0..4 {
        push(batch_dwords, &mut cursor, 0)?;
    }
    log_batch_offset(cursor, "3DSTATE_DS disabled");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_DS)?;
    for _ in 0..10 {
        push(batch_dwords, &mut cursor, 0)?;
    }
    log_batch_offset(cursor, "3DSTATE_GS disabled");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_GS)?;
    for _ in 0..9 {
        push(batch_dwords, &mut cursor, 0)?;
    }
    log_batch_offset(cursor, "3DSTATE_PS disabled");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_PS)?;
    for _ in 0..11 {
        push(batch_dwords, &mut cursor, 0)?;
    }
    log_batch_offset(cursor, "MI_STORE_DATA_IMM post-ps-state");
    push_store_data_imm(
        batch_dwords,
        &mut cursor,
        result_gpu_addr + (RESULT_SLOT_POST_PS_STATE_DWORD as u64) * 4,
        RCS_EXEC_RESULT_DRAW_POST_PS_STATE,
    )?;

    log_batch_offset(cursor, "3DSTATE_SBE");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_SBE)?;
    push(batch_dwords, &mut cursor, sbe_dw1)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, SBE_ACTIVE_COMPONENT_XYZW_MASK_DWORD)?;
    push(batch_dwords, &mut cursor, SBE_ACTIVE_COMPONENT_XYZW_MASK_DWORD)?;

    log_batch_offset(cursor, "3DSTATE_STREAMOUT");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_STREAMOUT)?;
    push(batch_dwords, &mut cursor, streamout_dw1)?;
    push(batch_dwords, &mut cursor, streamout_dw2)?;
    push(batch_dwords, &mut cursor, streamout_dw3)?;
    push(batch_dwords, &mut cursor, streamout_dw4)?;

    log_batch_offset(cursor, "PIPE_CONTROL pre-so-buffer");
    push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_CS_STALL)?;
    log_batch_offset(cursor, "3DSTATE_SO_BUFFER_INDEX_0");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_SO_BUFFER_INDEX_0)?;
    push(batch_dwords, &mut cursor, so_buffer_index_dw1)?;
    push_addr(batch_dwords, &mut cursor, GPU_VA_STREAMOUT_BASE)?;
    push(batch_dwords, &mut cursor, streamout_surface_size_dwords)?;
    push_addr(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "PIPE_CONTROL post-so-buffer");
    push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_CS_STALL)?;

    log_batch_offset(cursor, "3DSTATE_SO_DECL_LIST");
    let streamout_decl_dword0 = streamout_experiment.so_decl_buffer_selects();
    let streamout_decl_dword1 = streamout_experiment.so_decl_num_entries();
    let [streamout_decl_dword2, streamout_decl_dword3, streamout_decl_dword4, streamout_decl_dword5] =
        streamout_experiment.so_decl_entry_dwords();
    push(batch_dwords, &mut cursor, streamout_experiment.so_decl_header())?;
    push(batch_dwords, &mut cursor, streamout_decl_dword0)?;
    push(batch_dwords, &mut cursor, streamout_decl_dword1)?;
    push(batch_dwords, &mut cursor, streamout_decl_dword2)?;
    push(batch_dwords, &mut cursor, streamout_decl_dword3)?;
    if matches!(
        streamout_experiment,
        StreamoutProofExperiment::HeaderAndPositionSlots01
    ) {
        push(batch_dwords, &mut cursor, streamout_decl_dword4)?;
        push(batch_dwords, &mut cursor, streamout_decl_dword5)?;
    }
    crate::log!(
        "intel/render: {} decl experiment={} read_len={} so_pitch={} decl=[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X}] slot_contract={}\n",
        submit_label,
        streamout_experiment.label(),
        streamout_experiment.vertex_read_length(),
        streamout_experiment.vertex_bytes(),
        streamout_decl_dword0,
        streamout_decl_dword1,
        streamout_decl_dword2,
        streamout_decl_dword3,
        streamout_decl_dword4,
        streamout_decl_dword5,
        streamout_experiment.vf_slot_contract(),
    );
    crate::log!(
        "intel/render: {} contract experiment={} stages_disabled={} sbe[read_offset=1 read_length=1 num_sf_attrs=1 force_offset=1 force_length=1] urb_vs[alloc_len={} start={} entries={}] vb[index=0 pitch={} size=0x{:X}] streamout[read_offset=0 read_length_field={} rendering_disable={} stats_enable={} pitch={} so_gpu=0x{:X} size_dwords=0x{:X}] topo={}\n",
        submit_label,
        streamout_experiment.label(),
        if vs_config.is_some() {
            "hs|te|ds|gs|ps"
        } else {
            "vs|hs|te|ds|gs|ps"
        },
        programmed_vs_urb_output_length,
        TRIANGLE_VS_URB_START,
        TRIANGLE_VS_URB_ENTRIES,
        draw.vertex_stride,
        vb_size_bytes,
        streamout_dw2 & 0x1F,
        (streamout_dw1 >> 30) & 0x1,
        (streamout_dw1 >> 25) & 0x1,
        streamout_dw3 & 0xFFF,
        GPU_VA_STREAMOUT_BASE,
        streamout_surface_size_dwords,
        primitive_topology_label(batch_mode.topology()),
    );
    log_batch_offset(cursor, "PIPE_CONTROL post-so-decl");
    push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_CS_STALL)?;

    log_batch_offset(cursor, "MI_STORE_DATA_IMM pre-3d");
    push_store_data_imm(
        batch_dwords,
        &mut cursor,
        result_gpu_addr + (RESULT_SLOT_PRE3D_DWORD as u64) * 4,
        pre3d_value,
    )?;

    log_batch_offset(cursor, "3DPRIMITIVE");
    push(batch_dwords, &mut cursor, CMD_3DPRIMITIVE)?;
    push(batch_dwords, &mut cursor, batch_mode.topology())?;
    push(batch_dwords, &mut cursor, draw.vertex_count)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 1)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;

    log_batch_offset(cursor, "PIPE_CONTROL post-3d-eop-sync");
    push_pipe_control_post_sync_imm(
        batch_dwords,
        &mut cursor,
        PIPE_CONTROL_POST_DRAW_SYNC_BITS,
        result_gpu_addr + (RESULT_SLOT_POST3D_PIPE_CONTROL_LO_DWORD as u64) * 4,
        post3d_value,
    )?;

    log_batch_offset(cursor, "MI_STORE_DATA_IMM final");
    push_store_data_imm(
        batch_dwords,
        &mut cursor,
        result_gpu_addr + (RESULT_SLOT_FINAL_DWORD as u64) * 4,
        done_value,
    )?;
    log_batch_offset(cursor, "MI_BATCH_BUFFER_END");
    push(batch_dwords, &mut cursor, MI_BATCH_BUFFER_END)?;
    push(batch_dwords, &mut cursor, MI_NOOP)?;

    crate::log!(
        "intel/render: 3dprimitive-setup mode={:?} topo={} vertices={} start_vertex=0 instances=1 start_instance=0 base_vertex=0 vb=0x{:X} stride={} rt=0x{:X} pitch=0x{:X} rect={}x{}\n",
        batch_mode,
        primitive_topology_label(batch_mode.topology()),
        draw.vertex_count,
        draw.vertex_gpu_addr,
        draw.vertex_stride,
        draw.rt_gpu_addr,
        draw.rt_pitch,
        draw.target_w,
        draw.target_h
    );

    Ok(cursor * core::mem::size_of::<u32>())
}

fn encode_vf_streamout_proof_batch(
    batch_dwords: &mut [u32],
    warm: RenderWarmState,
    draw: TriangleDrawPrep,
    result_gpu_addr: u64,
    pre3d_value: u32,
    post3d_value: u32,
    done_value: u32,
    streamout_experiment: StreamoutProofExperiment,
    slice_hash_table_offset_bytes: u32,
) -> Result<usize, &'static str> {
    encode_minimal_streamout_proof_batch(
        batch_dwords,
        warm,
        draw,
        result_gpu_addr,
        pre3d_value,
        post3d_value,
        done_value,
        streamout_experiment,
        slice_hash_table_offset_bytes,
        None,
    )
}

fn encode_vs_streamout_proof_batch(
    batch_dwords: &mut [u32],
    warm: RenderWarmState,
    draw: TriangleDrawPrep,
    result_gpu_addr: u64,
    pre3d_value: u32,
    post3d_value: u32,
    done_value: u32,
    streamout_experiment: StreamoutProofExperiment,
    slice_hash_table_offset_bytes: u32,
    vs_config: VsStreamoutProofConfig,
) -> Result<usize, &'static str> {
    encode_minimal_streamout_proof_batch(
        batch_dwords,
        warm,
        draw,
        result_gpu_addr,
        pre3d_value,
        post3d_value,
        done_value,
        streamout_experiment,
        slice_hash_table_offset_bytes,
        Some(vs_config),
    )
}


fn encode_3d_no_draw_probe_batch(
    batch_dwords: &mut [u32],
    warm: RenderWarmState,
    result_gpu_addr: u64,
    done_value: u32,
) -> Result<usize, &'static str> {
    let mut cursor = 0usize;

    fn push(batch_dwords: &mut [u32], cursor: &mut usize, value: u32) -> Result<(), &'static str> {
        if *cursor >= batch_dwords.len() {
            return Err("3d-no-draw-batch-exhausted");
        }
        batch_dwords[*cursor] = value;
        *cursor += 1;
        Ok(())
    }

    fn push_addr(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        value: u64,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, value as u32)?;
        push(batch_dwords, cursor, (value >> 32) as u32)
    }

    fn push_store_data_imm(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        address: u64,
        value: u32,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, MI_STORE_DATA_IMM_GGTT_DW1)?;
        push_addr(batch_dwords, cursor, address)?;
        push(batch_dwords, cursor, value)
    }

    fn push_pipe_control_full(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        flags_dw0: u32,
        flags_dw1: u32,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, PIPE_CONTROL_CMD)?;
        push(batch_dwords, cursor, flags_dw1)?;
        if let Some(slot) = batch_dwords.get_mut(cursor.saturating_sub(2)) {
            *slot |= flags_dw0;
        } else {
            return Err("3d-no-draw-pipe-control-header");
        }
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)
    }

    fn push_sba_address(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        enable: bool,
        mocs: u32,
        address: u64,
    ) -> Result<(), &'static str> {
        let low = ((address as u32) & 0xFFFF_F000) | (mocs << 4) | u32::from(enable);
        push(batch_dwords, cursor, low)?;
        push(batch_dwords, cursor, (address >> 32) as u32)
    }

    fn push_sba_size(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        enable: bool,
        size_bytes: usize,
    ) -> Result<(), &'static str> {
        let size_bytes = crate::intel::align_up(size_bytes, 4096).ok_or("3d-no-draw-sba-align")?;
        let size_bytes = u32::try_from(size_bytes).map_err(|_| "3d-no-draw-sba-convert")?;
        push(batch_dwords, cursor, (size_bytes & 0xFFFF_F000) | u32::from(enable))
    }

    batch_dwords.fill(0);
    push_pipe_control_full(batch_dwords, &mut cursor, 0, PIPE_CONTROL_FLUSH_BITS)?;
    push_pipe_control_full(batch_dwords, &mut cursor, 0, PIPE_CONTROL_INVALIDATE_BITS)?;
    push(batch_dwords, &mut cursor, PIPELINE_SELECT_3D)?;
    push(batch_dwords, &mut cursor, STATE_BASE_ADDRESS_CMD)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push(batch_dwords, &mut cursor, 0)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_VERTEX_BASE)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.draw_state_len)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.draw_state_len)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.vertex_len)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.draw_state_len)?;
    for _ in 0..6 {
        push(batch_dwords, &mut cursor, 0)?;
    }
    push_store_data_imm(batch_dwords, &mut cursor, result_gpu_addr, done_value)?;
    push(batch_dwords, &mut cursor, MI_BATCH_BUFFER_END)?;
    push(batch_dwords, &mut cursor, MI_NOOP)?;
    Ok(cursor * core::mem::size_of::<u32>())
}

fn upload_triangle_shader_pipeline(
    warm: RenderWarmState,
    pipeline: &'static crate::intel::shader::TrianglePipeline,
) -> Result<TriangleShaderLayout, &'static str> {
    let vs = stage_range("vs", pipeline.vs.meta.kernel, pipeline.vs.code)?;
    let ps = stage_range("ps", pipeline.ps.meta.kernel, pipeline.ps.code)?;

    if pipeline.vs.meta.kernel.grf_used == 0 {
        return Err("vs-shader-grf-used-zero");
    }
    if pipeline.ps.meta.kernel.grf_used == 0 {
        return Err("ps-shader-grf-used-zero");
    }
    if pipeline.vs.meta.max_threads == 0 {
        return Err("vs-max-threads-zero");
    }

    if ranges_overlap(
        vs.code_offset_bytes,
        vs.code_size_bytes,
        ps.code_offset_bytes,
        ps.code_size_bytes,
    ) {
        return Err("shader-code-overlap");
    }

    let used_end = core::cmp::max(
        stage_end(vs.code_offset_bytes, vs.code_size_bytes).ok_or("shader-code-overflow")?,
        stage_end(ps.code_offset_bytes, ps.code_size_bytes).ok_or("shader-code-overflow")?,
    );
    if used_end > warm.draw_state_len {
        return Err("shader-code-exceeds-state-bo");
    }

    upload_stage_code(warm.draw_state_virt, vs.code_offset_bytes, pipeline.vs.code)?;
    upload_stage_code(warm.draw_state_virt, ps.code_offset_bytes, pipeline.ps.code)?;

    crate::intel::dma_flush(warm.draw_state_virt, used_end);

    let state_region_offset_bytes =
        crate::intel::align_up(used_end, crate::intel::WARM_ALIGN).ok_or("state-region-align")?;
    if state_region_offset_bytes > warm.draw_state_len {
        return Err("state-region-exceeds-state-bo");
    }

    let bo_gpu_base = GPU_VA_DRAW_STATE_BASE;
    let vs_gpu = bo_gpu_base + vs.code_offset_bytes as u64;
    let ps_gpu = bo_gpu_base + ps.code_offset_bytes as u64;

    Ok(TriangleShaderLayout {
        vs: TriangleShaderStageLayout {
            code_offset_bytes: vs.code_offset_bytes as u32,
            code_gpu_addr: vs_gpu,
            ksp_offset_bytes: pipeline.vs.meta.kernel.ksp_offset_bytes,
            ksp_gpu_addr: vs_gpu + pipeline.vs.meta.kernel.ksp_offset_bytes as u64,
            code_size_bytes: vs.code_size_bytes as u32,
        },
        ps: TriangleShaderStageLayout {
            code_offset_bytes: ps.code_offset_bytes as u32,
            code_gpu_addr: ps_gpu,
            ksp_offset_bytes: pipeline.ps.meta.kernel.ksp_offset_bytes,
            ksp_gpu_addr: ps_gpu + pipeline.ps.meta.kernel.ksp_offset_bytes as u64,
            code_size_bytes: ps.code_size_bytes as u32,
        },
        state_region_gpu_addr: bo_gpu_base + state_region_offset_bytes as u64,
        state_region_offset_bytes: state_region_offset_bytes as u32,
        used_bytes: used_end as u32,
    })
}

#[derive(Copy, Clone)]
struct StageUploadRange {
    code_offset_bytes: usize,
    code_size_bytes: usize,
}

fn stage_range(
    stage_name: &'static str,
    meta: crate::intel::shader::ShaderKernelMetadata,
    code: &'static [u32],
) -> Result<StageUploadRange, &'static str> {
    if meta.code_size_bytes == 0 || code.is_empty() {
        return Err(stage_error(stage_name, "shader-empty"));
    }

    let code_len_bytes = code
        .len()
        .checked_mul(core::mem::size_of::<u32>())
        .ok_or(stage_error(stage_name, "shader-code-len-overflow"))?;
    let declared_size = usize::try_from(meta.code_size_bytes)
        .map_err(|_| stage_error(stage_name, "shader-size-convert"))?;
    if declared_size != code_len_bytes {
        return Err(stage_error(stage_name, "shader-size-mismatch"));
    }

    let code_offset = usize::try_from(meta.code_offset_bytes)
        .map_err(|_| stage_error(stage_name, "shader-offset-convert"))?;
    let code_alignment = usize::try_from(meta.code_alignment_bytes)
        .map_err(|_| stage_error(stage_name, "shader-align-convert"))?;
    if code_alignment == 0 || code_offset % code_alignment != 0 {
        return Err(stage_error(stage_name, "shader-offset-alignment"));
    }

    let ksp_offset = usize::try_from(meta.ksp_offset_bytes)
        .map_err(|_| stage_error(stage_name, "shader-ksp-convert"))?;
    if ksp_offset % 64 != 0 {
        return Err(stage_error(stage_name, "shader-ksp-alignment"));
    }
    if ksp_offset >= declared_size {
        return Err(stage_error(stage_name, "shader-ksp-range"));
    }

    Ok(StageUploadRange {
        code_offset_bytes: code_offset,
        code_size_bytes: declared_size,
    })
}

fn upload_stage_code(
    dst_base: *mut u8,
    offset_bytes: usize,
    code: &'static [u32],
) -> Result<(), &'static str> {
    let len_bytes = code
        .len()
        .checked_mul(core::mem::size_of::<u32>())
        .ok_or("shader-copy-len-overflow")?;
    if len_bytes == 0 {
        return Ok(());
    }

    unsafe {
        core::ptr::copy_nonoverlapping(
            code.as_ptr() as *const u8,
            dst_base.add(offset_bytes),
            len_bytes,
        );
    }
    Ok(())
}

fn stage_end(offset_bytes: usize, size_bytes: usize) -> Option<usize> {
    offset_bytes.checked_add(size_bytes)
}

fn ranges_overlap(a_offset: usize, a_size: usize, b_offset: usize, b_size: usize) -> bool {
    let Some(a_end) = stage_end(a_offset, a_size) else {
        return true;
    };
    let Some(b_end) = stage_end(b_offset, b_size) else {
        return true;
    };
    a_offset < b_end && b_offset < a_end
}

fn stage_error(stage_name: &'static str, reason: &'static str) -> &'static str {
    match (stage_name, reason) {
        ("vs", "shader-empty") => "vs-shader-empty",
        ("vs", "shader-code-len-overflow") => "vs-shader-code-len-overflow",
        ("vs", "shader-size-convert") => "vs-shader-size-convert",
        ("vs", "shader-size-mismatch") => "vs-shader-size-mismatch",
        ("vs", "shader-offset-convert") => "vs-shader-offset-convert",
        ("vs", "shader-align-convert") => "vs-shader-align-convert",
        ("vs", "shader-offset-alignment") => "vs-shader-offset-alignment",
        ("vs", "shader-ksp-convert") => "vs-shader-ksp-convert",
        ("vs", "shader-ksp-alignment") => "vs-shader-ksp-alignment",
        ("vs", "shader-ksp-range") => "vs-shader-ksp-range",
        ("ps", "shader-empty") => "ps-shader-empty",
        ("ps", "shader-code-len-overflow") => "ps-shader-code-len-overflow",
        ("ps", "shader-size-convert") => "ps-shader-size-convert",
        ("ps", "shader-size-mismatch") => "ps-shader-size-mismatch",
        ("ps", "shader-offset-convert") => "ps-shader-offset-convert",
        ("ps", "shader-align-convert") => "ps-shader-align-convert",
        ("ps", "shader-offset-alignment") => "ps-shader-offset-alignment",
        ("ps", "shader-ksp-convert") => "ps-shader-ksp-convert",
        ("ps", "shader-ksp-alignment") => "ps-shader-ksp-alignment",
        ("ps", "shader-ksp-range") => "ps-shader-ksp-range",
        _ => "shader-stage-error",
    }
}

fn prepare_triangle_draw_resources(
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
) -> Option<TriangleDrawPrep> {
    let target_w = u32::try_from(rect_w).ok()?;
    let target_h = u32::try_from(rect_h).ok()?;
    let rt_pitch = u32::try_from(pitch).ok()?;
    if warm.vertex_len < TRIANGLE_DRAW_VERTICES * TRIANGLE_DRAW_VERTEX_STRIDE {
        return None;
    }
    if warm.draw_state_len == 0 {
        return None;
    }

    let vertices = unsafe {
        core::slice::from_raw_parts_mut(
            warm.vertex_virt as *mut f32,
            (warm.vertex_len / core::mem::size_of::<f32>())
                .max(TRIANGLE_DRAW_VERTICES * TRIANGLE_DRAW_VERTEX_DWORDS),
        )
    };
    vertices.fill(0.0);

    // Keep the geometry comfortably inside the canonical clip volume so this
    // proof path is a trivial-accept case for clipper rather than a test of
    // clipping behavior itself.
    let tri = [
        [-0.25f32, -0.20, 0.0],
        [0.25, -0.20, 0.0],
        [0.00, 0.20, 0.0],
    ];
    let signed_area_2x = (tri[1][0] - tri[0][0]) * (tri[2][1] - tri[0][1])
        - (tri[2][0] - tri[0][0]) * (tri[1][1] - tri[0][1]);
    for (dst, src) in vertices
        .chunks_exact_mut(TRIANGLE_DRAW_VERTEX_DWORDS)
        .take(TRIANGLE_DRAW_VERTICES)
        .zip(tri.iter())
    {
        dst.copy_from_slice(src);
    }
    crate::intel::dma_flush(warm.vertex_virt, TRIANGLE_DRAW_VERTICES * TRIANGLE_DRAW_VERTEX_STRIDE);
    crate::log!(
        "intel/render: draw-verts v0=[{:.3},{:.3},{:.3}] v1=[{:.3},{:.3},{:.3}] v2=[{:.3},{:.3},{:.3}] stride={} gpu=0x{:X} signed_area2={:.3} winding={}\n",
        tri[0][0],
        tri[0][1],
        tri[0][2],
        tri[1][0],
        tri[1][1],
        tri[1][2],
        tri[2][0],
        tri[2][1],
        tri[2][2],
        TRIANGLE_DRAW_VERTEX_STRIDE,
        GPU_VA_VERTEX_BASE,
        signed_area_2x,
        if signed_area_2x >= 0.0 { "ccw" } else { "cw" }
    );

    unsafe {
        core::ptr::write_bytes(warm.draw_state_virt, 0, warm.draw_state_len);
    }
    crate::intel::dma_flush(warm.draw_state_virt, warm.draw_state_len);

    Some(TriangleDrawPrep {
        vertex_count: TRIANGLE_DRAW_VERTICES as u32,
        vertex_stride: TRIANGLE_DRAW_VERTEX_STRIDE as u32,
        vertex_gpu_addr: GPU_VA_VERTEX_BASE,
        state_gpu_addr: GPU_VA_DRAW_STATE_BASE,
        rt_gpu_addr: dst_gpu_addr,
        rt_pitch,
        target_w,
        target_h,
    })
}

fn prepare_vf_streamout_proof_resources(
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    experiment: StreamoutProofExperiment,
) -> Option<TriangleDrawPrep> {
    let target_w = u32::try_from(rect_w).ok()?;
    let target_h = u32::try_from(rect_h).ok()?;
    let rt_pitch = u32::try_from(pitch).ok()?;
    let vertex_stride = experiment.vertex_bytes();
    if warm.vertex_len < TRIANGLE_DRAW_VERTICES * vertex_stride {
        return None;
    }

    let tri = [
        [-0.25f32, -0.20, 0.0],
        [0.25, -0.20, 0.0],
        [0.00, 0.20, 0.0],
    ];
    let words =
        unsafe { core::slice::from_raw_parts_mut(warm.vertex_virt as *mut u32, warm.vertex_len / 4) };
    words.fill(0);

    for (idx, pos) in tri.iter().enumerate() {
        match experiment {
            StreamoutProofExperiment::PositionSlot1 => {
                let base = idx * 4;
                words[base + 0] = pos[0].to_bits();
                words[base + 1] = pos[1].to_bits();
                words[base + 2] = pos[2].to_bits();
                words[base + 3] = 1.0f32.to_bits();
                crate::log!(
                    "intel/render: vf-streamout-source v{} experiment={} raw=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] pos=[{:.3},{:.3},{:.3},{:.3}]\n",
                    idx,
                    experiment.label(),
                    words[base + 0],
                    words[base + 1],
                    words[base + 2],
                    words[base + 3],
                    f32::from_bits(words[base + 0]),
                    f32::from_bits(words[base + 1]),
                    f32::from_bits(words[base + 2]),
                    f32::from_bits(words[base + 3]),
                );
            }
            StreamoutProofExperiment::HeaderAndPositionSlots01 => {
                let base = idx * 8;
                words[base + 0] = 0x5155_0000 | idx as u32;
                words[base + 1] = 0x5155_1000 | idx as u32;
                words[base + 2] = 0x5155_2000 | idx as u32;
                words[base + 3] = 0x5155_3000 | idx as u32;
                words[base + 4] = pos[0].to_bits();
                words[base + 5] = pos[1].to_bits();
                words[base + 6] = pos[2].to_bits();
                words[base + 7] = 1.0f32.to_bits();
                crate::log!(
                    "intel/render: vf-streamout-source v{} experiment={} hdr=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] pos=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] pos_f=[{:.3},{:.3},{:.3},{:.3}]\n",
                    idx,
                    experiment.label(),
                    words[base + 0],
                    words[base + 1],
                    words[base + 2],
                    words[base + 3],
                    words[base + 4],
                    words[base + 5],
                    words[base + 6],
                    words[base + 7],
                    f32::from_bits(words[base + 4]),
                    f32::from_bits(words[base + 5]),
                    f32::from_bits(words[base + 6]),
                    f32::from_bits(words[base + 7]),
                );
            }
        }
    }

    crate::intel::dma_flush(warm.vertex_virt, TRIANGLE_DRAW_VERTICES * vertex_stride);

    Some(TriangleDrawPrep {
        vertex_count: TRIANGLE_DRAW_VERTICES as u32,
        vertex_stride: vertex_stride as u32,
        vertex_gpu_addr: GPU_VA_VERTEX_BASE,
        state_gpu_addr: GPU_VA_DRAW_STATE_BASE,
        rt_gpu_addr: dst_gpu_addr,
        rt_pitch,
        target_w,
        target_h,
    })
}

fn write_vf_streamout_probe_state(warm: RenderWarmState) -> Result<u32, &'static str> {
    unsafe {
        core::ptr::write_bytes(warm.draw_state_virt, 0, warm.draw_state_len);
    }

    if !device_is_gfx125(warm.device_id) {
        return Ok(0);
    }

    let slice_hash_table_offset = VF_STREAMOUT_SLICE_HASH_TABLE_OFFSET;
    let end_offset = slice_hash_table_offset
        .checked_add(GFX125_SLICE_HASH_TABLE_BYTES)
        .ok_or("vf-streamout-state-overflow")?;
    if end_offset > warm.draw_state_len {
        return Err("vf-streamout-state-exceeds-state-bo");
    }

    let dwords = unsafe {
        core::slice::from_raw_parts_mut(warm.draw_state_virt as *mut u32, warm.draw_state_len / 4)
    };
    let slice_hash = &mut dwords[slice_hash_table_offset / 4
        ..slice_hash_table_offset / 4 + GFX125_SLICE_HASH_TABLE_DWORDS];
    let mut packed = [0u32; GFX125_SLICE_HASH_TABLE_DWORDS];
    gfx125_pack_slice_hash_tables(gfx125_slice_hash_config(warm), &mut packed);
    slice_hash.copy_from_slice(&packed);

    crate::intel::dma_flush(
        unsafe { warm.draw_state_virt.add(slice_hash_table_offset) },
        GFX125_SLICE_HASH_TABLE_BYTES,
    );

    Ok(slice_hash_table_offset as u32)
}

fn submit_triangle_to_surface(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
) -> bool {
    unsafe {
        core::ptr::write_volatile(warm.result_virt as *mut u32, 0xC0DE_7700);
    }
    crate::intel::dma_flush(warm.result_virt, core::mem::size_of::<u32>());

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let Ok(batch_tail_bytes) = encode_rgb_triangle_store_batch(
        batch,
        dst_gpu_addr,
        pitch,
        rect_w,
        rect_h,
        GPU_VA_RESULT_BASE,
        RCS_EXEC_RESULT_DONE,
    ) else {
        crate::log!(
            "intel/render: primary-triangle batch build failed size={}x{} pitch=0x{:X}\n",
            rect_w,
            rect_h,
            pitch
        );
        return false;
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);

    submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_DONE,
        RESULT_SLOT_PRE3D_DWORD,
        "mi-triangle",
    )
}

fn submit_vertical_stripes_to_surface(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
) -> bool {
    let stripe_x_phase = PRIMARY_STRIPE_X_PHASE.fetch_add(MI_STRIPE_X_STEP_PX, Ordering::AcqRel);

    unsafe {
        core::ptr::write_volatile(warm.result_virt as *mut u32, 0xC0DE_7700);
    }
    crate::intel::dma_flush(warm.result_virt, core::mem::size_of::<u32>());

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let Ok(batch_tail_bytes) = encode_vertical_stripe_store_batch(
        batch,
        dst_gpu_addr,
        pitch,
        rect_w,
        rect_h,
        stripe_x_phase,
        GPU_VA_RESULT_BASE,
        RCS_EXEC_RESULT_DONE,
    ) else {
        crate::log!(
            "intel/render: primary-mi-stripes batch build failed size={}x{} pitch=0x{:X} batch=0x{:X} phase={}\n",
            rect_w,
            rect_h,
            pitch,
            warm.batch_len,
            stripe_x_phase
        );
        return false;
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);

    if should_log_primary_probe("periodic", PRIMARY_PROBE_SEQ.load(Ordering::Acquire)) {
        crate::log!(
            "intel/render: primary-mi-stripes phase={} step={} stripes={} width={}\n",
            stripe_x_phase,
            MI_STRIPE_X_STEP_PX,
            MI_STRIPE_COUNT,
            MI_STRIPE_WIDTH_PX
        );
    }

    submit_warm_render_batch(dev, warm, RCS_EXEC_RESULT_DONE, RESULT_SLOT_PRE3D_DWORD, "mi-stripes")
}

fn submit_warm_render_batch(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    expected_result: u32,
    expected_result_slot_dword: usize,
    submit_name: &'static str,
) -> bool {
    let stats_before = capture_triangle_stage_stats(dev);
    let surface_samples_before = if submit_name == "draw-path" {
        crate::intel::display::capture_primary_surface_samples()
    } else {
        None
    };
    if is_triangle_debug_submit_name(submit_name) {
        log_triangle_stage_stats(submit_name, "before-submit", true, stats_before, None);
    }
    let ring_tail_bytes = build_ring_batch_start(warm, GPU_VA_BATCH_BASE);
    let Some(ring_ctl) = ring_ctl_value(warm.ring_len) else {
        return false;
    };
    if !init_gen12_lrc_context_image(
        warm,
        GPU_VA_RING_BASE as u32,
        ring_tail_bytes as u32,
        ring_ctl,
    ) {
        return false;
    }
    let (context_desc_lo, context_desc_hi) = build_execlist_context_descriptor(GPU_VA_CONTEXT_BASE);
    write_lrc_ring_tail(warm, ring_tail_bytes as u32);
    let pphwsp_gpu = (GPU_VA_CONTEXT_BASE & !0xFFF) as u32;

    crate::intel::mmio_write(
        dev,
        RCS_RING_MODE_GEN7,
        masked_bit_enable(GFX_RUN_LIST_ENABLE | GEN11_GFX_DISABLE_LEGACY_MODE),
    );
    let ctx_ctl_after = masked_bits_update(
        CTX_CTRL_RS_CTX_ENABLE,
        CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT
            | CTX_CTRL_ENGINE_CTX_SAVE_INHIBIT
            | CTX_CTRL_INHIBIT_SYN_CTX_SWITCH,
    );
    crate::intel::mmio_write(dev, RCS_RING_CONTEXT_CONTROL, ctx_ctl_after);
    crate::intel::mmio_write(dev, RCS_RING_CONTEXT_CONTROL_REF, ctx_ctl_after);
    crate::intel::mmio_write(dev, RCS_RING_HWS_PGA, pphwsp_gpu);
    let hws_after = crate::intel::mmio_read(dev, RCS_RING_HWS_PGA);

    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    execlist_submit_port_push(dev, context_desc_lo, context_desc_hi, 0, 0);
    crate::intel::mmio_write(dev, RCS_RING_EXECLIST_CONTROL, EL_CTRL_LOAD);

    if should_log_primary_probe_detail() {
        crate::log!(
            "intel/render: {} execlist-start desc=0x{:08X}:0x{:08X} hws=0x{:08X} sq=0x{:08X}:0x{:08X} ctx_ctl=0x{:08X}\n",
            submit_name,
            context_desc_hi,
            context_desc_lo,
            hws_after,
            crate::intel::mmio_read(dev, RCS_RING_EXECLIST_SQ_HI),
            crate::intel::mmio_read(dev, RCS_RING_EXECLIST_SQ_LO),
            crate::intel::mmio_read(dev, RCS_RING_CONTEXT_CONTROL)
        );
    }

    let mut completed = false;
    let mut iter = 0usize;
    while iter < 4096 {
        let result0 = read_result_dword(warm, RESULT_SLOT_PRE3D_DWORD);
        let result1 = read_result_dword(warm, RESULT_SLOT_POST3D_DWORD);
        let result2 = read_result_dword(warm, RESULT_SLOT_FINAL_DWORD);
        let result3 = read_result_dword(warm, RESULT_SLOT_POST_VF_DWORD);
        let result4 = read_result_dword(warm, RESULT_SLOT_POST_VS_DWORD);
        let result5 = read_result_dword(warm, RESULT_SLOT_POST_PS_STATE_DWORD);
        let result6 = read_result_dword(warm, RESULT_SLOT_POST_CLIP_DWORD);
        let result7 = read_result_dword(warm, RESULT_SLOT_POST_RASTER_DWORD);
        let result_post3d_eop =
            read_result_dword(warm, RESULT_SLOT_POST3D_PIPE_CONTROL_LO_DWORD);
        let result_post3d_eop_hi =
            read_result_dword(warm, RESULT_SLOT_POST3D_PIPE_CONTROL_HI_DWORD);
        let observed = match expected_result_slot_dword {
            RESULT_SLOT_PRE3D_DWORD => result0,
            RESULT_SLOT_POST3D_DWORD => result1,
            RESULT_SLOT_FINAL_DWORD => result2,
            RESULT_SLOT_POST_VF_DWORD => result3,
            RESULT_SLOT_POST_VS_DWORD => result4,
            RESULT_SLOT_POST_PS_STATE_DWORD => result5,
            RESULT_SLOT_POST_CLIP_DWORD => result6,
            RESULT_SLOT_POST_RASTER_DWORD => result7,
            RESULT_SLOT_POST3D_PIPE_CONTROL_LO_DWORD => result_post3d_eop,
            RESULT_SLOT_POST3D_PIPE_CONTROL_HI_DWORD => result_post3d_eop_hi,
            _ => result0,
        };
        if observed == expected_result {
            completed = true;
            break;
        }
        if should_log_primary_probe_detail()
            && (iter == 0 || iter == 256 || iter == 1024 || iter == 4095)
        {
            let poll_stats = capture_triangle_stage_stats(dev);
            crate::log!(
                "intel/render: {} poll iter={} head=0x{:08X} tail=0x{:08X} acthd=0x{:08X} ipeir=0x{:08X} ipehr=0x{:08X} eir=0x{:08X} execlist_lo=0x{:08X} execlist_hi=0x{:08X} result0=0x{:08X} result1=0x{:08X} result2=0x{:08X}\n",
                submit_name,
                iter,
                crate::intel::mmio_read(dev, RCS_RING_HEAD),
                crate::intel::mmio_read(dev, RCS_RING_TAIL),
                crate::intel::mmio_read(dev, RCS_RING_ACTHD),
                crate::intel::mmio_read(dev, RCS_RING_IPEIR),
                crate::intel::mmio_read(dev, RCS_RING_IPEHR),
                crate::intel::mmio_read(dev, RCS_RING_EIR),
                crate::intel::mmio_read(dev, RCS_RING_EXECLIST_STATUS_LO),
                crate::intel::mmio_read(dev, RCS_RING_EXECLIST_STATUS_HI),
                result0,
                result1,
                result2
            );
            crate::log!(
                "intel/render: {} poll-stage iter={} post_vf=0x{:08X} post_vs=0x{:08X} post_ps_state=0x{:08X} post_clip=0x{:08X} post_raster=0x{:08X} post3d_eop=0x{:08X} post3d_hi=0x{:08X}\n",
                submit_name,
                iter,
                result3,
                result4,
                result5,
                result6,
                result7,
                result_post3d_eop,
                result_post3d_eop_hi
            );
            crate::log!(
                "intel/render: {} poll-counters iter={} ia_vtx={} ia_prim={} vs={} hs={} ds={} gs={} gs_prim={} cl={} cl_prim={} ps={} cps={} ps_depth={} so0={} so_write0={}\n",
                submit_name,
                iter,
                poll_stats.ia_vertices,
                poll_stats.ia_primitives,
                poll_stats.vs_invocations,
                poll_stats.hs_invocations,
                poll_stats.ds_invocations,
                poll_stats.gs_invocations,
                poll_stats.gs_primitives,
                poll_stats.cl_invocations,
                poll_stats.cl_primitives,
                poll_stats.ps_invocations,
                poll_stats.cps_invocations,
                poll_stats.ps_depth,
                poll_stats.so_prims_written_0,
                poll_stats.so_write_offset_0,
            );
        }
        core::hint::spin_loop();
        iter += 1;
    }

    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    let result0 = read_result_dword(warm, RESULT_SLOT_PRE3D_DWORD);
    let result1 = read_result_dword(warm, RESULT_SLOT_POST3D_DWORD);
    let result2 = read_result_dword(warm, RESULT_SLOT_FINAL_DWORD);
    let result3 = read_result_dword(warm, RESULT_SLOT_POST_VF_DWORD);
    let result4 = read_result_dword(warm, RESULT_SLOT_POST_VS_DWORD);
    let result5 = read_result_dword(warm, RESULT_SLOT_POST_PS_STATE_DWORD);
    let result6 = read_result_dword(warm, RESULT_SLOT_POST_CLIP_DWORD);
    let result7 = read_result_dword(warm, RESULT_SLOT_POST_RASTER_DWORD);
    let result_post3d_eop = read_result_dword(warm, RESULT_SLOT_POST3D_PIPE_CONTROL_LO_DWORD);
    let result_post3d_eop_hi =
        read_result_dword(warm, RESULT_SLOT_POST3D_PIPE_CONTROL_HI_DWORD);
    if should_log_primary_probe_detail() {
        crate::log!(
            "intel/render: {} complete={} result0=0x{:08X} result1=0x{:08X} result2=0x{:08X} post_vf=0x{:08X} post_vs=0x{:08X} post_ps_state=0x{:08X} post_clip=0x{:08X} post_raster=0x{:08X} post3d_eop=0x{:08X} post3d_hi=0x{:08X} ctl=0x{:08X} instdone=0x{:08X}\n",
            submit_name,
            completed as u8,
            result0,
            result1,
            result2,
            result3,
            result4,
            result5,
            result6,
            result7,
            result_post3d_eop,
            result_post3d_eop_hi,
            crate::intel::mmio_read(dev, RCS_RING_CTL),
            crate::intel::mmio_read(dev, RCS_RING_INSTDONE)
        );
        crate::intel::display::log_primary_surface_samples("post-render");
    }
    if is_triangle_debug_submit_name(submit_name) {
        crate::log!(
            "intel/render: 3dprimitive-result completed={} pre3d={} post3d={} final={} vf={} vs={} ps_state={} clip={} raster={} pre_draw_packet_markers={} clip_raster_packet_markers={} post_draw_retire_markers={} acthd=0x{:08X} ipehr=0x{:08X}\n",
            completed as u8,
            result0 == RCS_EXEC_RESULT_DRAW_PRE3D,
            result_post3d_eop == RCS_EXEC_RESULT_DRAW_POST3D,
            result2 == RCS_EXEC_RESULT_DONE,
            result3 == RCS_EXEC_RESULT_DRAW_POST_VF,
            result4 == RCS_EXEC_RESULT_DRAW_POST_VS,
            result5 == RCS_EXEC_RESULT_DRAW_POST_PS_STATE,
            result6 == RCS_EXEC_RESULT_DRAW_POST_CLIP,
            result7 == RCS_EXEC_RESULT_DRAW_POST_RASTER,
            ((result3 == RCS_EXEC_RESULT_DRAW_POST_VF) && (result4 == RCS_EXEC_RESULT_DRAW_POST_VS))
                as u8,
            ((result6 == RCS_EXEC_RESULT_DRAW_POST_CLIP)
                && (result7 == RCS_EXEC_RESULT_DRAW_POST_RASTER)) as u8,
            ((result_post3d_eop == RCS_EXEC_RESULT_DRAW_POST3D)
                && (result2 == RCS_EXEC_RESULT_DONE)) as u8,
            crate::intel::mmio_read(dev, RCS_RING_ACTHD),
            crate::intel::mmio_read(dev, RCS_RING_IPEHR)
        );
    }
    if !completed && is_triangle_debug_submit_name(submit_name) {
        let acthd = crate::intel::mmio_read(dev, RCS_RING_ACTHD);
        let acthd_batch_off = acthd.saturating_sub(GPU_VA_BATCH_BASE as u32);
        let instdone_geom = crate::intel::mmio_read(dev, INSTDONE_GEOM);
        let sc_instdone = crate::intel::mmio_read(dev, SC_INSTDONE);
        let sc_extra = crate::intel::mmio_read(dev, SC_INSTDONE_EXTRA);
        let sc_extra2 = crate::intel::mmio_read(dev, SC_INSTDONE_EXTRA2);
        crate::log!(
            "intel/render: {} stall-detail acthd_batch_off=0x{:08X} ipehr=0x{:08X} instdone_geom=0x{:08X} sc_instdone=0x{:08X} sc_extra=0x{:08X} sc_extra2=0x{:08X}\n",
            submit_name,
            acthd_batch_off,
            crate::intel::mmio_read(dev, RCS_RING_IPEHR),
            instdone_geom,
            sc_instdone,
            sc_extra,
            sc_extra2,
        );
        log_not_done_units(submit_name, "stall-geom-not-done", instdone_geom, GEOM_INSTDONE_BITS);
        log_not_done_units(submit_name, "stall-sc-not-done", sc_instdone, SC_INSTDONE_BITS);
        log_not_done_units(
            submit_name,
            "stall-sc-extra-not-done",
            sc_extra,
            SC_INSTDONE_EXTRA_BITS,
        );
        log_not_done_units(
            submit_name,
            "stall-sc-extra2-not-done",
            sc_extra2,
            SC_INSTDONE_EXTRA2_BITS,
        );
    }
    if is_triangle_debug_submit_name(submit_name) {
        let stats_after = capture_triangle_stage_stats(dev);
        log_triangle_stage_stats(
            submit_name,
            "after-submit",
            completed,
            stats_after,
            Some(stats_before),
        );
        log_triangle_stage_frontier(
            submit_name,
            completed,
            stats_before,
            stats_after,
            result_post3d_eop,
            result2,
            result3,
            result4,
            result5,
            result6,
            result7,
        );
        log_triangle_stage_diagnosis(submit_name, completed, stats_before, stats_after);
    }
    if submit_name == "draw-path" {
        if let (Some(before), Some(after)) =
            (surface_samples_before, crate::intel::display::capture_primary_surface_samples())
        {
            crate::log!(
                "intel/render: draw-path render-target completed={} any_change={} triangle_change={} apex={}=>{} centroid={}=>{} left={}=>{} right={}=>{} center={}=>{}\n",
                completed as u8,
                after.any_changed_since(before) as u8,
                after.triangle_points_changed_since(before) as u8,
                before.apex,
                after.apex,
                before.centroid,
                after.centroid,
                before.left,
                after.left,
                before.right,
                after.right,
                before.center,
                after.center,
            );
        }
    }
    if submit_name == "draw-path" {
        log_triangle_demo_stats(dev, completed);
    }
    if completed && submit_name == "draw-path" {
        let kicked = crate::intel::display::kick_primary_surface_scanout("post-draw-path");
        crate::log!("intel/render: draw-path scanout-kick={}\n", kicked as u8);
        crate::intel::display::log_pipe_live_scanout_state("post-draw-path");
    }
    completed
}

fn log_triangle_demo_stats(dev: crate::intel::Dev, completed: bool) {
    let mut values = [0u32; TRIANGLE_STATS_LOG.len()];
    for (idx, stat) in TRIANGLE_STATS_LOG.iter().copied().enumerate() {
        let Some(offset) = stat.mmio_offset() else {
            continue;
        };
        values[idx] = crate::intel::mmio_read(dev, offset);
    }

    crate::log!(
        "intel/render: triangle-stats completed={} {}={} {}={} {}={}\n",
        completed as u8,
        TRIANGLE_STATS_LOG[0].symbol(),
        values[0],
        TRIANGLE_STATS_LOG[1].symbol(),
        values[1],
        TRIANGLE_STATS_LOG[2].symbol(),
        values[2]
    );
}

fn triangle_vs_max_threads_field(device_id: u16, baked_max_threads: u16) -> u32 {
    if device_is_gfx125(device_id) {
        // Mesa advertises ADL-S gfx12 max_vs_threads = 546 and programs the
        // packet with max_vs_threads - 1.
        545
    } else {
        baked_max_threads.saturating_sub(1) as u32
    }
}

fn device_is_gfx125(device_id: u16) -> bool {
    matches!(device_id, 0x4680 | 0x4682 | 0x4688 | 0x468A | 0x468B | 0x4690 | 0x4692 | 0x4693)
}

fn decode_clip_mode_name(mode: u32) -> &'static str {
    match mode {
        0 => "CLIPMODE_NORMAL",
        3 => "CLIPMODE_REJECT_ALL",
        4 => "CLIPMODE_ACCEPT_ALL",
        _ => "unknown",
    }
}

fn decode_api_mode_name(mode: u32) -> &'static str {
    match mode {
        0 => "APIMODE_OGL",
        1 => "APIMODE_D3D",
        _ => "unknown",
    }
}

fn decode_vertex_subpixel_precision_name(bit: u32) -> &'static str {
    match bit {
        0 => "_8Bit",
        1 => "_4Bit",
        _ => "unknown",
    }
}

fn decode_deref_block_size_name(mode: u32) -> &'static str {
    match mode {
        0 => "Block32",
        1 => "PerPoly",
        2 => "Block8",
        _ => "unknown",
    }
}

fn decode_cull_mode_name(mode: u32) -> &'static str {
    match mode {
        0 => "both",
        1 => "none",
        2 => "front",
        3 => "back",
        _ => "unknown",
    }
}

fn decode_fill_mode_name(mode: u32) -> &'static str {
    match mode {
        0 => "solid",
        1 => "wireframe",
        2 => "point",
        _ => "unknown",
    }
}

fn decode_front_winding_name(bit: u32) -> &'static str {
    match bit {
        0 => "cw",
        1 => "ccw",
        _ => "unknown",
    }
}

fn decode_wm_force_thread_dispatch_name(mode: u32) -> &'static str {
    match mode {
        0 => "normal",
        1 => "force-off",
        2 => "force-on",
        _ => "reserved",
    }
}

fn decode_wm_early_depth_stencil_control_name(mode: u32) -> &'static str {
    match mode {
        0 => "EDSC_NORMAL",
        1 => "EDSC_PSEXEC",
        2 => "EDSC_PREPS",
        _ => "reserved",
    }
}

const GEOM_INSTDONE_BITS: &[(u32, &str)] = &[
    (1 << 1, "VFL"),
    (1 << 2, "VS"),
    (1 << 3, "HS"),
    (1 << 4, "TE"),
    (1 << 5, "DS"),
    (1 << 6, "GS"),
    (1 << 7, "SOL"),
    (1 << 8, "CL"),
    (1 << 9, "SF"),
    (1 << 11, "TDG1"),
    (1 << 13, "URBM"),
    (1 << 14, "SVG"),
    (1 << 17, "TSG0"),
    (1 << 22, "SDE"),
];

const SC_INSTDONE_BITS: &[(u32, &str)] = &[
    (1 << 0, "SVL"),
    (1 << 1, "WMFE"),
    (1 << 2, "WMBE"),
    (1 << 3, "HIZ"),
    (1 << 5, "IZFE"),
    (1 << 6, "SBE"),
    (1 << 9, "RCC"),
    (1 << 10, "RCPBE"),
    (1 << 11, "RCPFE"),
    (1 << 12, "DAPB"),
    (1 << 13, "DAPRBE"),
    (1 << 15, "SARB"),
    (1 << 16, "DC0"),
    (1 << 17, "DC1"),
    (1 << 18, "DC2"),
    (1 << 19, "DC3"),
    (1 << 20, "GW0"),
    (1 << 21, "GW1"),
    (1 << 22, "GW2"),
    (1 << 23, "GW3"),
    (1 << 24, "TDC"),
    (1 << 25, "SFBE"),
    (1 << 26, "PSS"),
    (1 << 27, "AMFS"),
];

const SC_INSTDONE_EXTRA_BITS: &[(u32, &str)] = &[
    (1 << 9, "RCC1"),
    (1 << 10, "RCPBE1"),
    (1 << 11, "RCPFE1"),
    (1 << 12, "DAPB1"),
    (1 << 13, "DAPRBE1"),
    (1 << 16, "DC4"),
    (1 << 17, "DC5"),
    (1 << 18, "DC6"),
    (1 << 19, "DC7"),
    (1 << 20, "GW4"),
    (1 << 21, "GW5"),
    (1 << 22, "GW6"),
    (1 << 23, "GW7"),
    (1 << 24, "TDC1"),
    (1 << 26, "PSS1"),
];

const SC_INSTDONE_EXTRA2_BITS: &[(u32, &str)] = &[
    (1 << 9, "RCC2"),
    (1 << 10, "RCPBE2"),
    (1 << 11, "RCPFE2"),
    (1 << 12, "DAPB2"),
    (1 << 13, "DAPRBE2"),
];

fn log_not_done_units(submit_name: &str, label: &str, value: u32, bits: &[(u32, &'static str)]) {
    crate::log!("intel/render: {} {}=", submit_name, label);
    let mut any = false;
    for &(mask, name) in bits {
        if (value & mask) == 0 {
            if any {
                crate::log!("|");
            }
            crate::log!("{}", name);
            any = true;
        }
    }
    if !any {
        crate::log!("none");
    }
    crate::log!("\n");
}

fn log_backend_dispatch_contract(
    wm_dw1: u32,
    ps_blend_dw1: u32,
    wm_depth_stencil_dw1: u32,
    _wm_depth_stencil_dw2: u32,
    _wm_depth_stencil_dw3: u32,
    wm_hz_op_dw1: u32,
    wm_hz_op_dw2: u32,
    wm_hz_op_dw3: u32,
    wm_hz_op_dw4: u32,
    ps_extra_dw1: u32,
) {
    let wm_statistics_enable = (wm_dw1 >> 31) & 0x1;
    let wm_force_thread_dispatch = (wm_dw1 >> 19) & 0x3;
    let wm_edsc = (wm_dw1 >> 21) & 0x3;
    let ps_blend_alpha_test = (ps_blend_dw1 >> 8) & 0x1;
    let ps_blend_color_enable = (ps_blend_dw1 >> 29) & 0x1;
    let ps_blend_has_writeable_rt = (ps_blend_dw1 >> 30) & 0x1;
    let ps_blend_alpha_to_coverage = (ps_blend_dw1 >> 31) & 0x1;
    let wm_depth_test_enable = (wm_depth_stencil_dw1 >> 1) & 0x1;
    let wm_stencil_write_enable = (wm_depth_stencil_dw1 >> 2) & 0x1;
    let wm_stencil_test_enable = (wm_depth_stencil_dw1 >> 3) & 0x1;
    let wm_double_sided_stencil = (wm_depth_stencil_dw1 >> 4) & 0x1;
    let wm_depth_write_enable = (wm_depth_stencil_dw1 >> 28) & 0x1;
    let wm_hz_partial_resolve = (wm_hz_op_dw1 >> 9) & 0x1;
    let wm_hz_samples = (wm_hz_op_dw1 >> 13) & 0x7;
    let wm_hz_stencil_resolve = (wm_hz_op_dw1 >> 24) & 0x1;
    let wm_hz_full_surface_clear = (wm_hz_op_dw1 >> 25) & 0x1;
    let wm_hz_hier_resolve = (wm_hz_op_dw1 >> 27) & 0x1;
    let wm_hz_depth_resolve = (wm_hz_op_dw1 >> 28) & 0x1;
    let wm_hz_scissor = (wm_hz_op_dw1 >> 29) & 0x1;
    let wm_hz_depth_clear = (wm_hz_op_dw1 >> 30) & 0x1;
    let wm_hz_stencil_clear = (wm_hz_op_dw1 >> 31) & 0x1;
    let wm_hz_sample_mask = wm_hz_op_dw4 & 0xFFFF;
    let wm_hz_op_active = ((wm_hz_op_dw1 | wm_hz_op_dw2 | wm_hz_op_dw3 | wm_hz_op_dw4) != 0) as u32;
    let ps_valid = (ps_extra_dw1 >> 31) & 0x1;
    let ps_has_uav = (ps_extra_dw1 >> 2) & 0x1;
    let ps_computes_stencil = (ps_extra_dw1 >> 5) & 0x1;
    let ps_per_sample = (ps_extra_dw1 >> 6) & 0x1;
    let ps_attribute_enable = (ps_extra_dw1 >> 8) & 0x1;
    let ps_computed_depth = (ps_extra_dw1 >> 26) & 0x3;
    let ps_kills = (ps_extra_dw1 >> 28) & 0x1;
    let dispatch_reason = if wm_force_thread_dispatch == 1 {
        "force-thread-dispatch-off"
    } else if ps_valid == 0 {
        "ps-invalid"
    } else if wm_force_thread_dispatch == 2 {
        "force-thread-dispatch-on"
    } else if wm_hz_op_active != 0 {
        "wm-hz-op-active"
    } else if ps_blend_has_writeable_rt != 0 {
        "writeable-rt"
    } else if ps_has_uav != 0 {
        "ps-uav"
    } else if ps_kills != 0 {
        "ps-kill"
    } else if ps_computed_depth != 0 && (wm_depth_test_enable != 0 || wm_depth_write_enable != 0) {
        "computed-depth"
    } else if ps_computes_stencil != 0 && wm_stencil_test_enable != 0 {
        "computed-stencil"
    } else {
        "no-ps-dispatch-qualifier"
    };
    let dispatch_armed = matches!(
        dispatch_reason,
        "force-thread-dispatch-on"
            | "writeable-rt"
            | "ps-uav"
            | "ps-kill"
            | "computed-depth"
            | "computed-stencil"
    ) as u32;

    crate::log!(
        "intel/render: probe-backend-decoded wm[stats={} force_thread_dispatch={}({}) edsc={}({})] ps_blend[writeable_rt={} blend_enable={} alpha_test={} alpha_to_coverage={}] wm_depth_stencil[depth_test={} depth_write={} stencil_test={} stencil_write={} double_sided_stencil={}]\n",
        wm_statistics_enable,
        wm_force_thread_dispatch,
        decode_wm_force_thread_dispatch_name(wm_force_thread_dispatch),
        wm_edsc,
        decode_wm_early_depth_stencil_control_name(wm_edsc),
        ps_blend_has_writeable_rt,
        ps_blend_color_enable,
        ps_blend_alpha_test,
        ps_blend_alpha_to_coverage,
        wm_depth_test_enable,
        wm_depth_write_enable,
        wm_stencil_test_enable,
        wm_stencil_write_enable,
        wm_double_sided_stencil,
    );
    crate::log!(
        "intel/render: probe-backend-gate wm_hz_op[active={} depth_clear={} depth_resolve={} hier_resolve={} stencil_clear={} stencil_resolve={} full_surface_clear={} partial_resolve={} scissor={} samples={} sample_mask=0x{:X}] ps_extra[valid={} attribute_enable={} per_sample={} has_uav={} kills={} computed_depth={} computes_stencil={}] dispatch_armed={} reason={}\n",
        wm_hz_op_active,
        wm_hz_depth_clear,
        wm_hz_depth_resolve,
        wm_hz_hier_resolve,
        wm_hz_stencil_clear,
        wm_hz_stencil_resolve,
        wm_hz_full_surface_clear,
        wm_hz_partial_resolve,
        wm_hz_scissor,
        wm_hz_samples,
        wm_hz_sample_mask,
        ps_valid,
        ps_attribute_enable,
        ps_per_sample,
        ps_has_uav,
        ps_kills,
        ps_computed_depth,
        ps_computes_stencil,
        dispatch_armed,
        dispatch_reason,
    );
}

fn log_mesa_spec_cross_compare(
    warm: RenderWarmState,
    pipeline: &'static crate::intel::shader::TrianglePipeline,
    sbe_dw1: u32,
    clip_dw1: u32,
    clip_dw2: u32,
    sf_dw1: u32,
    raster_dw1: u32,
    ps_dw3: u32,
    ps_dw6: u32,
    ps_extra_dw1: u32,
) {
    let trueos_sbe_read_offset = (sbe_dw1 >> 5) & 0x3F;
    let trueos_sbe_read_length = (sbe_dw1 >> 11) & 0x1F;
    let trueos_sbe_force_read_offset = (sbe_dw1 >> 28) & 0x1;
    let trueos_sbe_force_read_length = (sbe_dw1 >> 29) & 0x1;
    let trueos_sbe_num_sf_attrs = (sbe_dw1 >> 22) & 0x3F;
    let trueos_ps_vector_mask = (ps_dw3 >> 30) & 0x1;
    let trueos_ps_binding_table_entry_count = (ps_dw3 >> 18) & 0x1F;
    let trueos_ps_push_constants = (ps_dw6 >> 11) & 0x1;
    let trueos_ps_max_threads_per_psd = (ps_dw6 >> PS_MAX_THREADS_SHIFT) & 0x7F;
    let trueos_clip_perspective_divide_disable =
        ((clip_dw2 & CLIP_PERSPECTIVE_DIVIDE_DISABLE) != 0) as u32;
    let trueos_clip_mode = (clip_dw2 >> 13) & 0x7;
    let trueos_clip_enable = (clip_dw2 >> 31) & 0x1;
    let trueos_clip_stats = (clip_dw1 >> 10) & 0x1;
    let trueos_sf_stats = (sf_dw1 >> 10) & 0x1;
    let trueos_raster_cull_mode = (raster_dw1 >> 16) & 0x3;
    let trueos_ps_attribute_enable = (ps_extra_dw1 >> 8) & 0x1;
    let trueos_ps_per_sample = (ps_extra_dw1 >> 6) & 0x1;
    let trueos_ps_computed_depth = (ps_extra_dw1 >> 26) & 0x3;
    let trueos_ps_computes_stencil = (ps_extra_dw1 >> 5) & 0x1;
    let trueos_vs_urb_out_len = pipeline.vs.meta.urb_entry_output_length as u32;
    let trueos_ps_dispatch = match pipeline.ps.meta.kernel.dispatch_mode {
        crate::intel::shader::DispatchMode::Simd8 => "simd8",
        crate::intel::shader::DispatchMode::Simd16 => "simd16",
        crate::intel::shader::DispatchMode::Simd32 => "simd32",
    };

    crate::log!(
        "intel/render: mesa-compare target=device=0x{:04X} note={} host_sbe[read_offset=1 read_length=1 force_read_offset=1 force_read_length=1 num_sf_attrs=0] trueos_sbe[read_offset={} read_length={} force_read_offset={} force_read_length={} num_sf_attrs={}] host_clip[perspective_divide_disable=1] trueos_clip[perspective_divide_disable={} clip_mode={}({}) clip_enable={} statistics={}] debug_sf[statistics={}] host_raster[cull_mode=none sample_mask=0x1] trueos_raster[cull_mode={}({}) sample_mask=1]\n",
        warm.device_id,
        crate::intel::shader::triangle_pipeline_note(),
        trueos_sbe_read_offset,
        trueos_sbe_read_length,
        trueos_sbe_force_read_offset,
        trueos_sbe_force_read_length,
        trueos_sbe_num_sf_attrs,
        trueos_clip_perspective_divide_disable,
        trueos_clip_mode,
        decode_clip_mode_name(trueos_clip_mode),
        trueos_clip_enable,
        trueos_clip_stats,
        trueos_sf_stats,
        trueos_raster_cull_mode,
        decode_cull_mode_name(trueos_raster_cull_mode),
    );
    crate::log!(
        "intel/render: mesa-compare host_ps[vector_mask=0 binding_table_entry_count=0 push_constants=0 dispatch=simd8 max_threads_per_psd=63] trueos_ps[vector_mask={} binding_table_entry_count={} push_constants={} dispatch={} max_threads_per_psd={}] host_ps_extra[attribute_enable=0 per_sample=0 computed_depth=0 computes_stencil=0] trueos_ps_extra[attribute_enable={} per_sample={} computed_depth={} computes_stencil={}] spec_pre_raster[vs_urb_output_len={} sbe_read_offset={} sbe_read_length={}]\n",
        trueos_ps_vector_mask,
        trueos_ps_binding_table_entry_count,
        trueos_ps_push_constants,
        trueos_ps_dispatch,
        trueos_ps_max_threads_per_psd,
        trueos_ps_attribute_enable,
        trueos_ps_per_sample,
        trueos_ps_computed_depth,
        trueos_ps_computes_stencil,
        trueos_vs_urb_out_len,
        trueos_sbe_read_offset,
        trueos_sbe_read_length,
    );
}

fn read_stat_counter64(dev: crate::intel::Dev, reg: usize) -> u64 {
    let low = crate::intel::mmio_read(dev, reg) as u64;
    let high = crate::intel::mmio_read(dev, reg + 4) as u64;
    low | (high << 32)
}

fn capture_triangle_stage_stats(dev: crate::intel::Dev) -> TriangleStageStats {
    TriangleStageStats {
        ia_vertices: read_stat_counter64(
            dev,
            crate::intel::stats::RenderStat::IaVerticesCount
                .mmio_offset()
                .unwrap_or(0),
        ),
        ia_primitives: read_stat_counter64(
            dev,
            crate::intel::stats::RenderStat::IaPrimitivesCount
                .mmio_offset()
                .unwrap_or(0),
        ),
        vs_invocations: read_stat_counter64(
            dev,
            crate::intel::stats::RenderStat::VsInvocationCount
                .mmio_offset()
                .unwrap_or(0),
        ),
        hs_invocations: read_stat_counter64(
            dev,
            crate::intel::stats::RenderStat::HsInvocationCount
                .mmio_offset()
                .unwrap_or(0),
        ),
        ds_invocations: read_stat_counter64(
            dev,
            crate::intel::stats::RenderStat::DsInvocationCount
                .mmio_offset()
                .unwrap_or(0),
        ),
        gs_invocations: read_stat_counter64(
            dev,
            crate::intel::stats::RenderStat::GsInvocationCount
                .mmio_offset()
                .unwrap_or(0),
        ),
        gs_primitives: read_stat_counter64(
            dev,
            crate::intel::stats::RenderStat::GsPrimitivesCount
                .mmio_offset()
                .unwrap_or(0),
        ),
        cl_invocations: read_stat_counter64(
            dev,
            crate::intel::stats::RenderStat::ClInvocationCount
                .mmio_offset()
                .unwrap_or(0),
        ),
        cl_primitives: read_stat_counter64(
            dev,
            crate::intel::stats::RenderStat::ClPrimitivesCount
                .mmio_offset()
                .unwrap_or(0),
        ),
        ps_invocations: read_stat_counter64(
            dev,
            crate::intel::stats::RenderStat::PsInvocationCount
                .mmio_offset()
                .unwrap_or(0),
        ),
        cps_invocations: read_stat_counter64(
            dev,
            crate::intel::stats::RenderStat::CpsInvocationCount
                .mmio_offset()
                .unwrap_or(0),
        ),
        ps_depth: read_stat_counter64(
            dev,
            crate::intel::stats::RenderStat::PsDepthCount
                .mmio_offset()
                .unwrap_or(0),
        ),
        so_prims_written_0: read_stat_counter64(dev, SO_NUM_PRIMS_WRITTEN_0),
        so_write_offset_0: crate::intel::mmio_read(dev, SO_WRITE_OFFSET_0) as u64,
    }
}

fn log_triangle_stage_stats(
    submit_name: &str,
    label: &str,
    completed: bool,
    stats: TriangleStageStats,
    before: Option<TriangleStageStats>,
) {
    if let Some(before) = before {
        let delta = stats.delta_since(before);
        crate::log!(
            "intel/render: {} stage-stats label={} completed={} ia_vtx={} ia_prim={} vs={} hs={} ds={} gs={} gs_prim={} cl={} cl_prim={} ps={} cps={} ps_depth={} so0={} so_write0={} delta_ia_vtx={} delta_ia_prim={} delta_vs={} delta_hs={} delta_ds={} delta_gs={} delta_gs_prim={} delta_cl={} delta_cl_prim={} delta_ps={} delta_cps={} delta_ps_depth={} delta_so0={} delta_so_write0={}\n",
            submit_name,
            label,
            completed as u8,
            stats.ia_vertices,
            stats.ia_primitives,
            stats.vs_invocations,
            stats.hs_invocations,
            stats.ds_invocations,
            stats.gs_invocations,
            stats.gs_primitives,
            stats.cl_invocations,
            stats.cl_primitives,
            stats.ps_invocations,
            stats.cps_invocations,
            stats.ps_depth,
            stats.so_prims_written_0,
            stats.so_write_offset_0,
            delta.ia_vertices,
            delta.ia_primitives,
            delta.vs_invocations,
            delta.hs_invocations,
            delta.ds_invocations,
            delta.gs_invocations,
            delta.gs_primitives,
            delta.cl_invocations,
            delta.cl_primitives,
            delta.ps_invocations,
            delta.cps_invocations,
            delta.ps_depth,
            delta.so_prims_written_0,
            delta.so_write_offset_0
        );
    } else {
        crate::log!(
            "intel/render: {} stage-stats label={} completed={} ia_vtx={} ia_prim={} vs={} hs={} ds={} gs={} gs_prim={} cl={} cl_prim={} ps={} cps={} ps_depth={} so0={} so_write0={}\n",
            submit_name,
            label,
            completed as u8,
            stats.ia_vertices,
            stats.ia_primitives,
            stats.vs_invocations,
            stats.hs_invocations,
            stats.ds_invocations,
            stats.gs_invocations,
            stats.gs_primitives,
            stats.cl_invocations,
            stats.cl_primitives,
            stats.ps_invocations,
            stats.cps_invocations,
            stats.ps_depth,
            stats.so_prims_written_0,
            stats.so_write_offset_0
        );
    }
}

fn log_triangle_stage_diagnosis(
    submit_name: &str,
    completed: bool,
    before: TriangleStageStats,
    after: TriangleStageStats,
) {
    let delta = after.delta_since(before);
    let verdict = if is_vf_streamout_submit_name(submit_name)
        && delta.vs_invocations > 0
        && !completed
    {
        "vf-proof-unexpected-vs-counters"
    } else if is_vf_streamout_submit_name(submit_name)
        && (delta.ia_vertices > 0 || delta.ia_primitives > 0)
        && delta.vs_invocations == 0
        && delta.so_prims_written_0 == 0
        && delta.so_write_offset_0 == 0
        && !completed
    {
        "vf-progress-no-sol-write-or-offset"
    } else if delta.ia_vertices == 0 && delta.vs_invocations == 0 {
        "no-front-end-progress"
    } else if is_streamout_submit_name(submit_name)
        && delta.vs_invocations > 0
        && delta.so_prims_written_0 == 0
        && delta.so_write_offset_0 == 0
        && !completed
    {
        "vs-progress-no-sol-write-or-offset"
    } else if delta.cl_primitives > 0 && delta.ps_invocations == 0 && !completed {
        "clip-stage-produced-primitives-no-ps"
    } else if delta.vs_invocations > 0
        && delta.cl_invocations == 0
        && delta.cl_primitives == 0
        && delta.ps_invocations == 0
        && !completed
    {
        "vs-progress-no-clipper-or-ps-counters"
    } else if delta.cl_invocations > 0 && delta.ps_invocations == 0 {
        "stops-between-clipper-and-ps"
    } else if delta.ps_invocations > 0
        && delta.ps_depth > 0
        && delta.so_prims_written_0 == 0
        && delta.so_write_offset_0 == 0
        && !completed
    {
        "ps-depth-ran-no-streamout-or-retire"
    } else if delta.ps_invocations > 0 && delta.so_prims_written_0 == 0 && !completed {
        "ps-ran-no-retire-or-export"
    } else if (delta.so_prims_written_0 > 0 || delta.so_write_offset_0 > 0) && !completed {
        "streamout-wrote-no-retire"
    } else if completed {
        "completed"
    } else {
        "late-backend-stall"
    };
    crate::log!(
        "intel/render: {} stage-diagnosis completed={} verdict={} delta_vs={} delta_hs={} delta_ds={} delta_gs={} delta_gs_prim={} delta_cl={} delta_cl_prim={} delta_ps={} delta_cps={} delta_ps_depth={} delta_so0={} delta_so_write0={}\n",
        submit_name,
        completed as u8,
        verdict,
        delta.vs_invocations,
        delta.hs_invocations,
        delta.ds_invocations,
        delta.gs_invocations,
        delta.gs_primitives,
        delta.cl_invocations,
        delta.cl_primitives,
        delta.ps_invocations,
        delta.cps_invocations,
        delta.ps_depth,
        delta.so_prims_written_0,
        delta.so_write_offset_0
    );
}

fn maybe_soft_accept_streamout_submit(
    submit_name: &'static str,
    warm: RenderWarmState,
    before: TriangleStageStats,
    after: TriangleStageStats,
    require_vs: bool,
    min_streamout_bytes: usize,
) -> bool {
    let delta = after.delta_since(before);
    let post3d_eop = read_result_dword(warm, RESULT_SLOT_POST3D_PIPE_CONTROL_LO_DWORD);
    let post3d_hi = read_result_dword(warm, RESULT_SLOT_POST3D_PIPE_CONTROL_HI_DWORD);
    let expected_reason = if require_vs {
        "post3d-eop+vs+streamout-counters"
    } else {
        "post3d-eop+vf-streamout-counters"
    };
    let accept = post3d_eop == RCS_EXEC_RESULT_DRAW_POST3D
        && post3d_hi == 0
        && (delta.so_prims_written_0 > 0
            || usize::try_from(delta.so_write_offset_0).ok().unwrap_or(0) >= min_streamout_bytes)
        && if require_vs {
            delta.vs_invocations > 0
        } else {
            delta.vs_invocations == 0 && (delta.ia_vertices > 0 || delta.ia_primitives > 0)
        };
    crate::log!(
        "intel/render: {} soft-accept accepted={} reason={} post3d_eop=0x{:08X} post3d_hi=0x{:08X} delta_ia_vtx={} delta_ia_prim={} delta_vs={} delta_so0={} delta_so_write0={} min_streamout_bytes={}\n",
        submit_name,
        accept as u8,
        expected_reason,
        post3d_eop,
        post3d_hi,
        delta.ia_vertices,
        delta.ia_primitives,
        delta.vs_invocations,
        delta.so_prims_written_0,
        delta.so_write_offset_0,
        min_streamout_bytes,
    );
    accept
}

fn log_triangle_stage_frontier(
    submit_name: &str,
    completed: bool,
    before: TriangleStageStats,
    after: TriangleStageStats,
    result_post3d_eop: u32,
    result2: u32,
    result3: u32,
    result4: u32,
    result5: u32,
    result6: u32,
    result7: u32,
) {
    let delta = after.delta_since(before);
    let pre_raster_packets = ((result3 == RCS_EXEC_RESULT_DRAW_POST_VF)
        && (result4 == RCS_EXEC_RESULT_DRAW_POST_VS)) as u8;
    let ps_state_packet = (result5 == RCS_EXEC_RESULT_DRAW_POST_PS_STATE) as u8;
    let clip_raster_packets = ((result6 == RCS_EXEC_RESULT_DRAW_POST_CLIP)
        && (result7 == RCS_EXEC_RESULT_DRAW_POST_RASTER)) as u8;
    let post_draw_retire =
        ((result_post3d_eop == RCS_EXEC_RESULT_DRAW_POST3D)
            && (result2 == RCS_EXEC_RESULT_DONE)) as u8;
    let counter_frontier = if delta.ps_invocations > 0 {
        "ps-thread"
    } else if delta.cl_invocations > 0 || delta.cl_primitives > 0 {
        "clipper-thread"
    } else if is_vf_streamout_submit_name(submit_name)
        && (delta.ia_vertices > 0 || delta.ia_primitives > 0)
        && delta.vs_invocations == 0
    {
        "vf-only-counters"
    } else if delta.gs_invocations > 0
        || delta.ds_invocations > 0
        || delta.hs_invocations > 0
        || delta.gs_primitives > 0
    {
        "pre-raster-shader-thread"
    } else if delta.vs_invocations > 0 || delta.ia_vertices > 0 || delta.ia_primitives > 0 {
        "vs-only-counters"
    } else {
        "no-draw-counters"
    };
    let note = if clip_raster_packets != 0
        && delta.cl_invocations == 0
        && delta.cl_primitives == 0
        && delta.ps_invocations == 0
    {
        "state_packets_retired_through_raster_counters_only_show_no_clipper_or_ps_threads"
    } else if ps_state_packet != 0 && delta.ps_invocations == 0 {
        "ps_state_programmed_but_no_ps_threads"
    } else if post_draw_retire == 0 {
        "draw_not_retired"
    } else {
        "draw_retired"
    };
    crate::log!(
        "intel/render: {} stage-frontier completed={} pre_raster_packets={} ps_state_packet={} clip_raster_packets={} post_draw_retire={} counter_frontier={} note={}\n",
        submit_name,
        completed as u8,
        pre_raster_packets,
        ps_state_packet,
        clip_raster_packets,
        post_draw_retire,
        counter_frontier,
        note,
    );
}

fn log_streamout_proof_result(
    submit_name: &str,
    warm: RenderWarmState,
    completed: bool,
    vertex_count: usize,
    experiment: StreamoutProofExperiment,
) {
    let flush_bytes = experiment
        .vertex_bytes()
        .saturating_mul(vertex_count)
        .min(warm.streamout_len);
    if flush_bytes != 0 {
        crate::intel::dma_flush(warm.streamout_virt, flush_bytes);
    }
    let base = warm.streamout_virt as *const u32;
    let count = core::cmp::min(vertex_count, 3);
    let stride_words = experiment.vertex_bytes() / 4;
    for idx in 0..count {
        let words =
            unsafe { core::slice::from_raw_parts(base.add(idx * stride_words), stride_words) };
        match experiment {
            StreamoutProofExperiment::PositionSlot1 => {
                crate::log!(
                    "intel/render: {} v{} experiment={} completed={} raw=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] pos=[{:.3},{:.3},{:.3},{:.3}]\n",
                    submit_name,
                    idx,
                    experiment.label(),
                    completed as u8,
                    words[0],
                    words[1],
                    words[2],
                    words[3],
                    f32::from_bits(words[0]),
                    f32::from_bits(words[1]),
                    f32::from_bits(words[2]),
                    f32::from_bits(words[3])
                );
            }
            StreamoutProofExperiment::HeaderAndPositionSlots01 => {
                crate::log!(
                    "intel/render: {} v{} experiment={} completed={} hdr=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] pos=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] pos_f=[{:.3},{:.3},{:.3},{:.3}]\n",
                    submit_name,
                    idx,
                    experiment.label(),
                    completed as u8,
                    words[0],
                    words[1],
                    words[2],
                    words[3],
                    words[4],
                    words[5],
                    words[6],
                    words[7],
                    f32::from_bits(words[4]),
                    f32::from_bits(words[5]),
                    f32::from_bits(words[6]),
                    f32::from_bits(words[7])
                );
            }
        }
    }
}

fn primitive_topology_label(topology: u32) -> &'static str {
    match topology {
        0x01 => "pointlist",
        0x02 => "linelist",
        0x03 => "linestrip",
        0x04 => "trilist",
        0x05 => "tristrip",
        0x06 => "trifan",
        0x09 => "linelist_adj",
        _ => "unknown",
    }
}

fn decode_streamout_offset_mode_name(
    stream_offset_write_enable: u32,
    offset_addr_enable: u32,
) -> &'static str {
    match ((stream_offset_write_enable & 0x1) << 1) | (offset_addr_enable & 0x1) {
        0 => "legacy-mmio-only",
        1 => "store-mmio-to-memory",
        2 => "load-from-immediate-or-address",
        3 => "load-and-store",
        _ => "unknown",
    }
}

fn read_result_dword(warm: RenderWarmState, index: usize) -> u32 {
    unsafe { core::ptr::read_volatile((warm.result_virt as *const u32).add(index)) }
}

fn seed_result_debug_slots(warm: RenderWarmState) {
    unsafe {
        for i in 0..RESULT_DEBUG_DWORD_COUNT {
            core::ptr::write_volatile((warm.result_virt as *mut u32).add(i), RESULT_DEBUG_SENTINEL);
        }
    }
}

fn recover_render_engine_after_nonretired_submit(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    submit_name: &'static str,
) {
    let el_pre = crate::intel::mmio_read(dev, RCS_RING_EXECLIST_STATUS_LO);
    let mi_mode_pre = crate::intel::mmio_read(dev, RCS_RING_MI_MODE);
    let acthd_pre = crate::intel::mmio_read(dev, RCS_RING_ACTHD);
    crate::log!(
        "intel/render: {} recovery begin execlist_lo=0x{:08X} mi_mode=0x{:08X} acthd=0x{:08X}\n",
        submit_name,
        el_pre,
        mi_mode_pre,
        acthd_pre,
    );

    for _ in 0..200_000u32 {
        let el = crate::intel::mmio_read(dev, RCS_RING_EXECLIST_STATUS_LO);
        if (el >> 30) == 0 {
            break;
        }
        core::hint::spin_loop();
    }

    crate::intel::mmio_write(
        dev,
        RCS_RING_MI_MODE,
        RING_MI_MODE_STOP_RING | (RING_MI_MODE_STOP_RING << 16),
    );
    for _ in 0..50_000u32 {
        if crate::intel::mmio_read(dev, RCS_RING_MI_MODE) & MODE_IDLE != 0 {
            break;
        }
        core::hint::spin_loop();
    }

    crate::intel::mmio_write(dev, GDRST, GRDOM_RENDER);
    for _ in 0..500_000u32 {
        if crate::intel::mmio_read(dev, GDRST) & GRDOM_RENDER == 0 {
            break;
        }
        core::hint::spin_loop();
    }

    crate::intel::mmio_write(dev, RCS_RING_MI_MODE, RING_MI_MODE_STOP_RING << 16);
    crate::intel::ggtt_invalidate(dev);

    let mode_bits = GFX_RUN_LIST_ENABLE | GEN11_GFX_DISABLE_LEGACY_MODE;
    crate::intel::mmio_write(dev, RCS_RING_MODE_GEN7, masked_bit_enable(mode_bits));
    let forcewake_ok = forcewake_render_acquire(warm);

    crate::log!(
        "intel/render: {} recovery end gdrst=0x{:08X} execlist_lo=0x{:08X} mi_mode=0x{:08X} mode=0x{:08X} forcewake_ok={}\n",
        submit_name,
        crate::intel::mmio_read(dev, GDRST),
        crate::intel::mmio_read(dev, RCS_RING_EXECLIST_STATUS_LO),
        crate::intel::mmio_read(dev, RCS_RING_MI_MODE),
        crate::intel::mmio_read(dev, RCS_RING_MODE_GEN7),
        forcewake_ok as u8,
    );
}

fn build_ring_batch_start(warm: RenderWarmState, batch_gpu_addr: u64) -> usize {
    let dwords =
        unsafe { core::slice::from_raw_parts_mut(warm.ring_virt as *mut u32, BLT_RING_DWORDS) };
    dwords[0] = MI_BATCH_BUFFER_START_GEN8 | MI_BATCH_GTT;
    dwords[1] = batch_gpu_addr as u32;
    dwords[2] = (batch_gpu_addr >> 32) as u32;
    dwords[3] = MI_NOOP;
    crate::intel::dma_flush(warm.ring_virt, BLT_RING_TAIL_BYTES);
    BLT_RING_TAIL_BYTES
}

fn ring_ctl_value(size: usize) -> Option<u32> {
    let size = u32::try_from(size).ok()?;
    Some(size.checked_sub(4096)? | RING_VALID)
}

fn masked_bit_enable(bit: u32) -> u32 {
    bit | (bit << 16)
}

fn masked_bit_disable(bit: u32) -> u32 {
    bit << 16
}

fn masked_bits_update(set_bits: u32, clear_bits: u32) -> u32 {
    let update = set_bits | clear_bits;
    set_bits | (update << 16)
}

fn build_execlist_context_descriptor(context_gpu_addr: u64) -> (u32, u32) {
    let base = (context_gpu_addr as u32) & 0xFFFF_F000;
    let desc = base
        | GEN8_CTX_VALID
        | CTX_DESC_FORCE_RESTORE
        | GEN8_CTX_PRIVILEGE
        | GEN12_CTX_PRIORITY_NORMAL
        | (INTEL_LEGACY_64B_CONTEXT << GEN8_CTX_ADDRESSING_MODE_SHIFT);
    (desc, (context_gpu_addr >> 32) as u32)
}

fn execlist_submit_port_push(
    dev: crate::intel::Dev,
    context0_lo: u32,
    context0_hi: u32,
    context1_lo: u32,
    context1_hi: u32,
) {
    crate::intel::mmio_write(dev, RCS_RING_EXECLIST_SQ_LO, context0_lo);
    crate::intel::mmio_write(dev, RCS_RING_EXECLIST_SQ_HI, context0_hi);
    crate::intel::mmio_write(dev, RCS_RING_EXECLIST_SUBMIT_PORT, context0_lo);
    crate::intel::mmio_write(dev, RCS_RING_EXECLIST_SUBMIT_PORT, context0_hi);
    crate::intel::mmio_write(dev, RCS_RING_EXECLIST_SUBMIT_PORT, context1_lo);
    crate::intel::mmio_write(dev, RCS_RING_EXECLIST_SUBMIT_PORT, context1_hi);
}

fn write_lrc_ring_tail(warm: RenderWarmState, ring_tail: u32) {
    let total_dwords = warm.context_len / core::mem::size_of::<u32>();
    if total_dwords <= LRC_STATE_OFFSET_DWORDS + 3 {
        return;
    }

    let dwords =
        unsafe { core::slice::from_raw_parts_mut(warm.context_virt as *mut u32, total_dwords) };
    dwords[LRC_STATE_OFFSET_DWORDS + 3] = ring_tail;
    crate::intel::dma_flush(warm.context_virt, warm.context_len);
}

fn mi_lri_num_regs(num_regs: u32) -> u32 {
    num_regs.saturating_mul(2).saturating_sub(1)
}

fn mi_lri_cmd(num_regs: u32, flags: u32) -> u32 {
    MI_LOAD_REGISTER_IMM | MI_LRI_CS_MMIO | flags | mi_lri_num_regs(num_regs)
}

fn push_mi_nops(state: &mut [u32], idx: &mut usize, count: usize) {
    for _ in 0..count {
        state[*idx] = MI_NOOP;
        *idx += 1;
    }
}

fn init_gen12_lrc_context_image(
    warm: RenderWarmState,
    ring_start: u32,
    ring_tail: u32,
    ring_ctl: u32,
) -> bool {
    let total_dwords = warm.context_len / core::mem::size_of::<u32>();
    if total_dwords <= LRC_STATE_OFFSET_DWORDS {
        return false;
    }

    let dwords =
        unsafe { core::slice::from_raw_parts_mut(warm.context_virt as *mut u32, total_dwords) };
    dwords.fill(0);

    let state = &mut dwords[LRC_STATE_OFFSET_DWORDS..];
    if state.len() < 192 {
        return false;
    }

    state[0] = MI_NOOP;
    let mut idx = 1usize;

    state[idx] = mi_lri_cmd(13, MI_LRI_FORCE_POSTED);
    idx += 1;
    state[idx] = 0x2244;
    state[idx + 1] = 0x0009_0009;
    state[idx + 2] = 0x2034;
    state[idx + 3] = 0;
    state[idx + 4] = 0x2030;
    state[idx + 5] = ring_tail;
    state[idx + 6] = 0x2038;
    state[idx + 7] = ring_start;
    state[idx + 8] = 0x203C;
    state[idx + 9] = ring_ctl;
    state[idx + 10] = 0x2168;
    state[idx + 11] = 0;
    state[idx + 12] = 0x2140;
    state[idx + 13] = 0;
    state[idx + 14] = 0x2110;
    state[idx + 15] = 0;
    state[idx + 16] = 0x211C;
    state[idx + 17] = 0;
    state[idx + 18] = 0x2114;
    state[idx + 19] = 0;
    state[idx + 20] = 0x2118;
    state[idx + 21] = 0;
    state[idx + 22] = 0x21C0;
    state[idx + 23] = 0;
    state[idx + 24] = 0x21C4;
    state[idx + 25] = 0;
    state[idx + 26] = 0x21C8;
    state[idx + 27] = GEN12_CTX_RCS_INDIRECT_CTX_OFFSET_DEFAULT;
    state[idx + 28] = 0x2180;
    state[idx + 29] = 0;
    idx += 30;

    push_mi_nops(state, &mut idx, 5);

    state[idx] = mi_lri_cmd(9, MI_LRI_FORCE_POSTED);
    idx += 1;
    state[idx] = 0x23A8;
    state[idx + 1] = 0;
    state[idx + 2] = 0x228C;
    state[idx + 3] = 0;
    state[idx + 4] = 0x2288;
    state[idx + 5] = 0;
    state[idx + 6] = 0x2284;
    state[idx + 7] = 0;
    state[idx + 8] = 0x2280;
    state[idx + 9] = 0;
    state[idx + 10] = 0x227C;
    state[idx + 11] = 0;
    state[idx + 12] = 0x2278;
    state[idx + 13] = 0;
    state[idx + 14] = 0x2274;
    state[idx + 15] = 0;
    state[idx + 16] = 0x2270;
    state[idx + 17] = 0;
    idx += 18;

    state[idx] = mi_lri_cmd(3, MI_LRI_FORCE_POSTED);
    idx += 1;
    state[idx] = 0x21B0;
    state[idx + 1] = 0;
    state[idx + 2] = 0x25A8;
    state[idx + 3] = 0;
    state[idx + 4] = 0x25AC;
    state[idx + 5] = 0;
    idx += 6;

    push_mi_nops(state, &mut idx, 6);

    state[idx] = mi_lri_cmd(1, 0);
    idx += 1;
    state[idx] = 0x20C8;
    state[idx + 1] = 0x7FFF_FFFF;
    idx += 2;

    push_mi_nops(state, &mut idx, 13);

    state[idx] = mi_lri_cmd(51, MI_LRI_FORCE_POSTED);
    idx += 1;
    state[idx] = 0x2588;
    state[idx + 1] = 0;
    state[idx + 2] = 0x2588;
    state[idx + 3] = 0;
    state[idx + 4] = 0x2588;
    state[idx + 5] = 0;
    state[idx + 6] = 0x2588;
    state[idx + 7] = 0;
    state[idx + 8] = 0x2588;
    state[idx + 9] = 0;
    state[idx + 10] = 0x2588;
    state[idx + 11] = 0;
    state[idx + 12] = 0x2028;
    state[idx + 13] = 0;
    state[idx + 14] = 0x209C;
    state[idx + 15] = masked_bit_disable(RING_MI_MODE_STOP_RING);
    state[idx + 16] = 0x20C0;
    state[idx + 17] = 0;
    state[idx + 18] = 0x2178;
    state[idx + 19] = 0;
    state[idx + 20] = 0x217C;
    state[idx + 21] = 0;
    state[idx + 22] = 0x2358;
    state[idx + 23] = 0;
    state[idx + 24] = 0x2170;
    state[idx + 25] = 0;
    state[idx + 26] = 0x2150;
    state[idx + 27] = 0;
    state[idx + 28] = 0x2154;
    state[idx + 29] = 0;
    state[idx + 30] = 0x2158;
    state[idx + 31] = 0;
    state[idx + 32] = 0x241C;
    state[idx + 33] = 0;
    state[idx + 34] = 0x2600;
    state[idx + 35] = 0;
    state[idx + 36] = 0x2604;
    state[idx + 37] = 0;
    state[idx + 38] = 0x2608;
    state[idx + 39] = 0;
    state[idx + 40] = 0x260C;
    state[idx + 41] = 0;
    state[idx + 42] = 0x2610;
    state[idx + 43] = 0;
    state[idx + 44] = 0x2614;
    state[idx + 45] = 0;
    state[idx + 46] = 0x2618;
    state[idx + 47] = 0;
    state[idx + 48] = 0x261C;
    state[idx + 49] = 0;
    state[idx + 50] = 0x2620;
    state[idx + 51] = 0;
    state[idx + 52] = 0x2624;
    state[idx + 53] = 0;
    state[idx + 54] = 0x2628;
    state[idx + 55] = 0;
    state[idx + 56] = 0x262C;
    state[idx + 57] = 0;
    state[idx + 58] = 0x2630;
    state[idx + 59] = 0;
    state[idx + 60] = 0x2634;
    state[idx + 61] = 0;
    state[idx + 62] = 0x2638;
    state[idx + 63] = 0;
    state[idx + 64] = 0x263C;
    state[idx + 65] = 0;
    state[idx + 66] = 0x2640;
    state[idx + 67] = 0;
    state[idx + 68] = 0x2644;
    state[idx + 69] = 0;
    state[idx + 70] = 0x2648;
    state[idx + 71] = 0;
    state[idx + 72] = 0x264C;
    state[idx + 73] = 0;
    state[idx + 74] = 0x2650;
    state[idx + 75] = 0;
    state[idx + 76] = 0x2654;
    state[idx + 77] = 0;
    state[idx + 78] = 0x2658;
    state[idx + 79] = 0;
    state[idx + 80] = 0x265C;
    state[idx + 81] = 0;
    state[idx + 82] = 0x2660;
    state[idx + 83] = 0;
    state[idx + 84] = 0x2664;
    state[idx + 85] = 0;
    state[idx + 86] = 0x2668;
    state[idx + 87] = 0;
    state[idx + 88] = 0x266C;
    state[idx + 89] = 0;
    state[idx + 90] = 0x2670;
    state[idx + 91] = 0;
    state[idx + 92] = 0x2674;
    state[idx + 93] = 0;
    state[idx + 94] = 0x2678;
    state[idx + 95] = 0;
    state[idx + 96] = 0x267C;
    state[idx + 97] = 0;
    state[idx + 98] = 0x2068;
    state[idx + 99] = 0;
    state[idx + 100] = 0x2084;
    state[idx + 101] = 0;
    idx += 102;

    state[idx] = MI_NOOP;
    idx += 1;
    state[idx] = MI_BATCH_BUFFER_END | 1;

    crate::intel::dma_flush(warm.context_virt, warm.context_len);
    true
}

fn encode_rgb_triangle_store_batch(
    batch_dwords: &mut [u32],
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    result_gpu_addr: u64,
    done_value: u32,
) -> Result<usize, &'static str> {
    const RESERVED_END_DWORDS: usize = 2;
    const STORE_DWORDS: usize = 4;

    if batch_dwords.len() <= RESERVED_END_DWORDS + STORE_DWORDS {
        return Err("batch-too-small");
    }
    if rect_w < TRIANGLE_MIN_DIM || rect_h < TRIANGLE_MIN_DIM {
        return Err("triangle-too-small");
    }

    let tri_w = rect_w.min(TRIANGLE_MAX_W).max(TRIANGLE_MIN_DIM);
    let tri_h = rect_h.min(TRIANGLE_MAX_H).max(TRIANGLE_MIN_DIM);
    let origin_x = rect_w.saturating_sub(tri_w) / 2;
    let origin_y = rect_h.saturating_sub(tri_h) / 2;
    let v0x = origin_x as i32 + (tri_w as i32 / 2);
    let v0y = origin_y as i32;
    let v1x = origin_x as i32;
    let v1y = origin_y as i32 + tri_h.saturating_sub(1) as i32;
    let v2x = origin_x as i32 + tri_w.saturating_sub(1) as i32;
    let v2y = v1y;
    let area = edge_fn(v0x, v0y, v1x, v1y, v2x, v2y);
    if area == 0 {
        return Err("triangle-degenerate");
    }

    let min_x = v0x.min(v1x).min(v2x).max(0) as usize;
    let max_x = (v0x.max(v1x).max(v2x) + 1).min(rect_w as i32) as usize;
    let min_y = v0y.min(v1y).min(v2y).max(0) as usize;
    let max_y = (v0y.max(v1y).max(v2y) + 1).min(rect_h as i32) as usize;

    let writable_limit = batch_dwords
        .len()
        .saturating_sub(RESERVED_END_DWORDS + STORE_DWORDS);
    let mut idx = 0usize;

    for y in min_y..max_y {
        for x in min_x..max_x {
            let px = (x as i32) * 2 + 1;
            let py = (y as i32) * 2 + 1;
            let w0 = edge_fn2(v1x, v1y, v2x, v2y, px, py);
            let w1 = edge_fn2(v2x, v2y, v0x, v0y, px, py);
            let w2 = edge_fn2(v0x, v0y, v1x, v1y, px, py);
            if !same_sign_or_zero(area, w0)
                || !same_sign_or_zero(area, w1)
                || !same_sign_or_zero(area, w2)
            {
                continue;
            }
            if idx + STORE_DWORDS > writable_limit {
                return Err("batch-exhausted");
            }

            let r = bary_to_u8(w0, area);
            let g = bary_to_u8(w1, area);
            let b = bary_to_u8(w2, area);
            let color = pack_xrgb8888(r, g, b);
            let dst = dst_gpu_addr
                .saturating_add((y as u64).saturating_mul(pitch as u64))
                .saturating_add((x as u64).saturating_mul(4));

            batch_dwords[idx] = MI_STORE_DATA_IMM_GGTT_DW1;
            batch_dwords[idx + 1] = dst as u32;
            batch_dwords[idx + 2] = (dst >> 32) as u32;
            batch_dwords[idx + 3] = color;
            idx += STORE_DWORDS;
        }
    }

    if idx == 0 {
        return Err("triangle-empty");
    }
    if idx + STORE_DWORDS > batch_dwords.len().saturating_sub(RESERVED_END_DWORDS) {
        return Err("batch-no-result-slot");
    }

    batch_dwords[idx] = MI_STORE_DATA_IMM_GGTT_DW1;
    batch_dwords[idx + 1] = result_gpu_addr as u32;
    batch_dwords[idx + 2] = (result_gpu_addr >> 32) as u32;
    batch_dwords[idx + 3] = done_value;
    idx += STORE_DWORDS;

    batch_dwords[idx] = MI_BATCH_BUFFER_END;
    batch_dwords[idx + 1] = MI_NOOP;
    idx += RESERVED_END_DWORDS;
    Ok(idx * core::mem::size_of::<u32>())
}

fn encode_result_store_probe_batch(
    batch_dwords: &mut [u32],
    result_gpu_addr: u64,
    done_value: u32,
) -> Result<usize, &'static str> {
    if batch_dwords.len() < 6 {
        return Err("batch-too-small");
    }

    batch_dwords[0] = MI_STORE_DATA_IMM_GGTT_DW1;
    batch_dwords[1] = result_gpu_addr as u32;
    batch_dwords[2] = (result_gpu_addr >> 32) as u32;
    batch_dwords[3] = done_value;
    batch_dwords[4] = MI_BATCH_BUFFER_END;
    batch_dwords[5] = MI_NOOP;
    Ok(6 * core::mem::size_of::<u32>())
}

fn encode_vertical_stripe_store_batch(
    batch_dwords: &mut [u32],
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    x_phase: u32,
    result_gpu_addr: u64,
    done_value: u32,
) -> Result<usize, &'static str> {
    const RESERVED_END_DWORDS: usize = 2;
    const STORE_DWORDS: usize = 4;

    if batch_dwords.len() <= RESERVED_END_DWORDS + STORE_DWORDS {
        return Err("batch-too-small");
    }
    if rect_w == 0 || rect_h == 0 {
        return Err("stripe-empty-target");
    }

    let writable_limit = batch_dwords
        .len()
        .saturating_sub(RESERVED_END_DWORDS + STORE_DWORDS);
    let colors = [
        pack_xrgb8888(0xFF, 0x00, 0x00),
        pack_xrgb8888(0xFF, 0x80, 0x00),
        pack_xrgb8888(0xFF, 0xFF, 0x00),
        pack_xrgb8888(0x00, 0xFF, 0x00),
        pack_xrgb8888(0x00, 0xA0, 0xFF),
        pack_xrgb8888(0xFF, 0x00, 0xFF),
    ];
    let mut idx = 0usize;
    let phase = if rect_w == 0 {
        0
    } else {
        (x_phase as usize) % rect_w
    };

    for stripe_idx in 0..MI_STRIPE_COUNT {
        let center = ((((stripe_idx + 1) * rect_w) / (MI_STRIPE_COUNT + 1)) + phase) % rect_w;
        let x0 = center + rect_w - (MI_STRIPE_WIDTH_PX / 2);
        let color = colors[stripe_idx % colors.len()];
        for y in 0..rect_h {
            for stripe_dx in 0..MI_STRIPE_WIDTH_PX {
                let x = (x0 + stripe_dx) % rect_w;
                if idx + STORE_DWORDS > writable_limit {
                    return Err("stripe-batch-exhausted");
                }
                let dst = dst_gpu_addr
                    .saturating_add((y as u64).saturating_mul(pitch as u64))
                    .saturating_add((x as u64).saturating_mul(4));
                batch_dwords[idx] = MI_STORE_DATA_IMM_GGTT_DW1;
                batch_dwords[idx + 1] = dst as u32;
                batch_dwords[idx + 2] = (dst >> 32) as u32;
                batch_dwords[idx + 3] = color;
                idx += STORE_DWORDS;
            }
        }
    }

    if idx == 0 {
        return Err("stripe-empty");
    }
    if idx + STORE_DWORDS > batch_dwords.len().saturating_sub(RESERVED_END_DWORDS) {
        return Err("stripe-no-result-slot");
    }

    batch_dwords[idx] = MI_STORE_DATA_IMM_GGTT_DW1;
    batch_dwords[idx + 1] = result_gpu_addr as u32;
    batch_dwords[idx + 2] = (result_gpu_addr >> 32) as u32;
    batch_dwords[idx + 3] = done_value;
    idx += STORE_DWORDS;

    batch_dwords[idx] = MI_BATCH_BUFFER_END;
    batch_dwords[idx + 1] = MI_NOOP;
    idx += RESERVED_END_DWORDS;
    Ok(idx * core::mem::size_of::<u32>())
}

fn edge_fn(ax: i32, ay: i32, bx: i32, by: i32, px: i32, py: i32) -> i64 {
    let ax = ax as i64;
    let ay = ay as i64;
    let bx = bx as i64;
    let by = by as i64;
    let px = px as i64;
    let py = py as i64;
    (px - ax) * (by - ay) - (py - ay) * (bx - ax)
}

fn edge_fn2(ax: i32, ay: i32, bx: i32, by: i32, px2: i32, py2: i32) -> i64 {
    let ax2 = (ax * 2) as i64;
    let ay2 = (ay * 2) as i64;
    let bx2 = (bx * 2) as i64;
    let by2 = (by * 2) as i64;
    let px2 = px2 as i64;
    let py2 = py2 as i64;
    (px2 - ax2) * (by2 - ay2) - (py2 - ay2) * (bx2 - ax2)
}

fn same_sign_or_zero(area: i64, value: i64) -> bool {
    if area >= 0 {
        value >= 0
    } else {
        value <= 0
    }
}

fn bary_to_u8(weight: i64, area: i64) -> u32 {
    let num = weight.unsigned_abs().saturating_mul(255);
    let den = area.unsigned_abs().max(1);
    ((num + (den / 2)) / den).min(255) as u32
}

fn pack_xrgb8888(r: u32, g: u32, b: u32) -> u32 {
    (r << 16) | (g << 8) | b
}

struct CursorPlaneCaps {
    platform: &'static str,
    layout: &'static str,
    max_width: u16,
    max_height: u16,
    pipe_count: u8,
}

struct SpritePlaneCaps {
    platform: &'static str,
    display_ver: u8,
    pipe_count: u8,
    overlays_per_pipe: u8,
    rotation: &'static str,
    reflect_x: bool,
    csc: &'static str,
    scaling_filter: &'static str,
    damage_clips: bool,
}

fn cursor_plane_caps(device_id: u16) -> CursorPlaneCaps {
    match device_id {
        0x4680 | 0x4682 | 0x4688 | 0x468A | 0x468B | 0x4690 | 0x4692 | 0x4693 => CursorPlaneCaps {
            platform: "ADL-S",
            layout: "TGL/XE_D",
            max_width: 256,
            max_height: 256,
            pipe_count: 4,
        },
        0x46A0 | 0x46A1 | 0x46A2 | 0x46A3 | 0x46A6 | 0x46A8 | 0x46AA | 0x462A | 0x4626 | 0x4628
        | 0x46B0 | 0x46B1 | 0x46B2 | 0x46B3 => CursorPlaneCaps {
            platform: "ADL-P/N",
            layout: "TGL/XE_LPD",
            max_width: 256,
            max_height: 256,
            pipe_count: 4,
        },
        _ => CursorPlaneCaps {
            platform: "unknown",
            layout: "generic",
            max_width: 256,
            max_height: 256,
            pipe_count: 4,
        },
    }
}

fn sprite_plane_caps(device_id: u16) -> SpritePlaneCaps {
    match device_id {
        0x4680 | 0x4682 | 0x4688 | 0x468A | 0x468B | 0x4690 | 0x4692 | 0x4693 => SpritePlaneCaps {
            platform: "ADL-S",
            display_ver: 13,
            pipe_count: 4,
            overlays_per_pipe: 4,
            rotation: "0|180",
            reflect_x: true,
            csc: "BT601|BT709|BT2020",
            scaling_filter: "default|nearest",
            damage_clips: true,
        },
        0x46A0 | 0x46A1 | 0x46A2 | 0x46A3 | 0x46A6 | 0x46A8 | 0x46AA | 0x462A | 0x4626 | 0x4628
        | 0x46B0 | 0x46B1 | 0x46B2 | 0x46B3 => SpritePlaneCaps {
            platform: "ADL-P/N",
            display_ver: 13,
            pipe_count: 4,
            overlays_per_pipe: 4,
            rotation: "0|180",
            reflect_x: true,
            csc: "BT601|BT709|BT2020",
            scaling_filter: "default|nearest",
            damage_clips: true,
        },
        _ => SpritePlaneCaps {
            platform: "unknown",
            display_ver: 13,
            pipe_count: 4,
            overlays_per_pipe: 4,
            rotation: "0|180",
            reflect_x: true,
            csc: "BT601|BT709|BT2020",
            scaling_filter: "default|nearest",
            damage_clips: true,
        },
    }
}
