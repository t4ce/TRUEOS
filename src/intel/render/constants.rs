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
const RCS_CS_GPR_REL_BASE: usize = 0x600;
const RCS_CS_GPR_BASE: usize = RCS_RING_BASE + 0x600;
const RCS_CS_GPR_COUNT: usize = 16;
const RCS_RING_TAIL: usize = RCS_RING_BASE + 0x30;
const RCS_RING_HEAD: usize = RCS_RING_BASE + 0x34;
const RCS_RING_START: usize = RCS_RING_BASE + 0x38;
const RCS_RING_CTL: usize = RCS_RING_BASE + 0x3C;
const RCS_RING_PSMI_CTL: usize = RCS_RING_BASE + 0x50;
const RCS_RING_ACTHD_UDW: usize = RCS_RING_BASE + 0x5C;
const RCS_RING_DMA_FADD_UDW: usize = RCS_RING_BASE + 0x60;
const RCS_RING_ACTHD: usize = RCS_RING_BASE + 0x74;
const RCS_RING_INSTPS: usize = RCS_RING_BASE + 0x70;
const RCS_RING_DMA_FADD: usize = RCS_RING_BASE + 0x78;
const RCS_RING_NOPID: usize = RCS_RING_BASE + 0x94;
const RCS_RING_MI_MODE: usize = RCS_RING_BASE + 0x9C;
const RCS_RING_IMR: usize = RCS_RING_BASE + 0xA8;
const RCS_CS_DEBUG_MODE1: usize = RCS_RING_BASE + 0xEC;
const RCS_CS_DEBUG_MODE2: usize = RCS_RING_BASE + 0xD8;
const RCS_RING_EIR: usize = RCS_RING_BASE + 0xB0;
const RCS_RING_ESR: usize = RCS_RING_BASE + 0xB8;
const RCS_RING_IPEIR: usize = RCS_RING_BASE + 0x64;
const RCS_RING_IPEHR: usize = RCS_RING_BASE + 0x68;
const RCS_RING_INSTDONE: usize = RCS_RING_BASE + 0x6C;
const RCS_RING_INSTPM: usize = RCS_RING_BASE + 0xC0;
const RCS_RING_BBSTATE: usize = RCS_RING_BASE + 0x110;
const RCS_RING_BBADDR: usize = RCS_RING_BASE + 0x140;
const RCS_RING_BBADDR_UDW: usize = RCS_RING_BASE + 0x168;
const GEN8_RING_FAULT_REG: usize = 0x4094;
const GEN8_FAULT_TLB_DATA0: usize = 0x4B10;
const GEN8_FAULT_TLB_DATA1: usize = 0x4B14;
const GEN12_FAULT_TLB_DATA0: usize = 0xCEB8;
const GEN12_FAULT_TLB_DATA1: usize = 0xCEBC;
const GEN12_RING_FAULT_REG: usize = 0xCEC4;
const ERROR_GEN6: usize = 0x40A0;
const GFX_MODE: usize = 0x2520;
const GEN12_RCU_MODE: usize = 0x14800;
const GEN12_RCU_MODE_CCS_ENABLE: u32 = 1 << 0;
const CHICKEN_RASTER_2: usize = 0x6208;
const INSTDONE_GEOM: usize = 0x666C;
const SC_INSTDONE: usize = 0x7100;
const SC_INSTDONE_EXTRA: usize = 0x7104;
const SC_INSTDONE_EXTRA2: usize = 0x7108;
const SAMPLER_INSTDONE: usize = 0xE160;
const ROW_INSTDONE: usize = 0xE164;
const TDL_THR_STATUS0: usize = 0xE4B8;
const TDL_THR_DISP_COUNT: usize = 0xE4BC;
const TDL_THR_STATUS1: usize = 0xE5B8;
const TDL_THR_PF_COUNT: usize = 0xE5BC;
const TDL_THR_PF_STATUS0: usize = 0xE6B8;
const TDL_THR_PF_STATUS1: usize = 0xE7B8;
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
const GPU_VA_GPGPU_TILE_ARENA_BASE: u64 = 0x0400_0000;
const GPGPU_EU_KERNEL_OFFSET_BYTES: usize = 0x3000;
const GPGPU_WALKER_SCRATCH_OFFSET_BYTES: usize = 0x3800;
const GPGPU_TILE_ROWS: usize = 256;
const GPGPU_TILE_K_DIM: usize = 2048;
const GPGPU_TILE_WEIGHT_BYTES_PER_ELEM: usize = 2;
const GPGPU_TILE_X_BYTES_PER_ELEM: usize = 4;
const GPGPU_TILE_OUTPUT_BYTES_PER_ELEM: usize = 4;
const GPGPU_TILE_TARGET_TILES: usize = 3;
const GPGPU_WEIGHT_TILE_BYTES: usize =
    GPGPU_TILE_ROWS * GPGPU_TILE_K_DIM * GPGPU_TILE_WEIGHT_BYTES_PER_ELEM;
const GPGPU_X_VECTOR_BYTES: usize = GPGPU_TILE_K_DIM * GPGPU_TILE_X_BYTES_PER_ELEM;
const GPGPU_OUTPUT_TILE_BYTES: usize = GPGPU_TILE_ROWS * GPGPU_TILE_OUTPUT_BYTES_PER_ELEM;
const GPGPU_TILE_ARENA_REQUIRED_BYTES: usize = GPGPU_TILE_TARGET_TILES * GPGPU_WEIGHT_TILE_BYTES
    + GPGPU_X_VECTOR_BYTES
    + GPGPU_TILE_TARGET_TILES * GPGPU_OUTPUT_TILE_BYTES;
const GPGPU_TILE_ARENA_BYTES: usize = (GPGPU_TILE_ARENA_REQUIRED_BYTES + 4095) & !4095;
const RING_VALID: u32 = 1;
const EL_CTRL_LOAD: u32 = 1 << 0;
const CTX_CTRL_RS_CTX_ENABLE: u32 = 1 << 1;
const CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT: u32 = 1 << 0;
const CTX_CTRL_ENGINE_CTX_SAVE_INHIBIT: u32 = 1 << 2;
const CTX_CTRL_INHIBIT_SYN_CTX_SWITCH: u32 = 1 << 3;
const CTX_CTRL_OAC_CONTEXT_ENABLE: u32 = 1 << 8;
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
const MI_REPORT_PERF_COUNT_CMD: u32 = (0x28 << 23) | 2;
const MI_REPORT_PERF_COUNT_USE_GLOBAL_GTT: u32 = 1 << 0;
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
const RCS_EXEC_RESULT_MI_SCANOUT_DONE: u32 = 0xC0DE_7713;
const RCS_EXEC_RESULT_GPGPU_PREFLIGHT_DONE: u32 = 0xC0DE_7731;
const RCS_EXEC_RESULT_COMPUTE_WALKER_DONE: u32 = 0xC0DE_7732;
const RCS_EXEC_RESULT_GPGPU_EU_C_STORE_DONE: u32 = 0xC0DE_7733;
const RCS_EXEC_RESULT_3D_NO_DRAW_DONE: u32 = 0xC0DE_7712;
const RCS_EXEC_RESULT_DRAW_PRE3D: u32 = 0xC0DE_7721;
const RCS_EXEC_RESULT_DRAW_POST_VF: u32 = 0xC0DE_7723;
const RCS_EXEC_RESULT_DRAW_POST_VS: u32 = 0xC0DE_7724;
const RCS_EXEC_RESULT_DRAW_POST_PS_STATE: u32 = 0xC0DE_7725;
const RCS_EXEC_RESULT_DRAW_POST_CLIP: u32 = 0xC0DE_7726;
const RCS_EXEC_RESULT_DRAW_POST_RASTER: u32 = 0xC0DE_7727;
const RCS_EXEC_RESULT_DRAW_FINAL_AFTER_LIGHT: u32 = 0xC0DE_7728;
const RCS_EXEC_RESULT_DRAW_PRE_LIGHT_PC: u32 = 0xC0DE_7729;
const RCS_EXEC_RESULT_DRAW_POST3D: u32 = 0xC0DE_7722;
const RCS_ARTIFICIAL_FRAGMENT_PRE_COLOR: u32 = 0xA17F_1001;
const RCS_ARTIFICIAL_FRAGMENT_POST_COLOR: u32 = 0xA17F_1002;
const PRIMARY_TRIANGLE_SUBMIT_ATTEMPTS: usize = 3;
const PRIMARY_USE_MI_STRIPE_PROBE: bool = false;
const PRIMARY_USE_MI_SCANOUT_PROOF: bool = false;
const PRIMARY_USE_3D_NO_DRAW_PROBE: bool = false;
const PRIMARY_USE_DRAW_PATH_BOOT_ONCE: bool = true;
const PRIMARY_BOOT_3D_PROBES_ENABLED: bool = true;
// Temporary one-boot quiet switch: keep RCS render/GPGPU probes off while
// validating the rest of boot without render-engine traffic.
const PRIMARY_DISABLE_RENDER_BRINGUP: bool = false;
const GPGPU_SUBMIT_WHEN_PRIMARY_RENDER_DISABLED: bool = false;
const MI_STRIPE_COUNT: usize = 12;
const MI_STRIPE_WIDTH_PX: usize = 4;
const MI_STRIPE_X_STEP_PX: u32 = 1;
const PRIMARY_PERIODIC_LOG_EVERY: u32 = 30;
const MI_STORE_DATA_IMM_GGTT_DW1: u32 = 0x1040_0002;
const TS_GPGPU_THREADS_DISPATCHED_LO: usize = 0x2290;
const TS_GPGPU_THREADS_DISPATCHED_HI: usize = 0x2294;
const RENDER_MOCS: u32 = 1;
const GEN12_L3ALLOC: usize = 0xB134;
const GEN12_L3ALLOC_ADL_DEFAULT: u32 = (32 << 1) | (88 << 25);
const GFX125_L3ALLOC_FULL_WAYS: u32 = 1 << 9;
const SURFTYPE_2D: u32 = 1;
const SURFTYPE_NULL: u32 = 7;
const SURFACE_FORMAT_B8G8R8A8_UNORM: u32 = 10;
const SURFACE_FORMAT_R32G32B32A32_FLOAT: u32 = 0;
const SURFACE_FORMAT_R32G32B32A32_UINT: u32 = 2;
const SURFACE_FORMAT_R32G32B32_FLOAT: u32 = 64;
const DEPTH_SURFACE_FORMAT_D32_FLOAT: u32 = 1;
const SURFACE_HALIGN_4: u32 = 1;
const SURFACE_HALIGN_128_GFX125: u32 = 3;
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
const PS_EXTRA_SIMPLE_PS_HINT: u32 = 1 << 9;
const PS_EXTRA_REQUIRES_NONPERSPECTIVE_BARY_PLANE: u32 = 1 << 19;
const PS_EXTRA_REQUIRES_PERSPECTIVE_BARY_PLANE: u32 = 1 << 20;
const PS_EXTRA_REQUIRES_SOURCE_DEPTH_W_PLANE: u32 = 1 << 21;
const PS_EXTRA_USES_SOURCE_W: u32 = 1 << 23;
const PS_EXTRA_USES_SOURCE_DEPTH: u32 = 1 << 24;
const PS_EXTRA_PIXEL_SHADER_VALID: u32 = 1 << 31;
const VFCOMP_STORE_SRC: u32 = 1;
const VFCOMP_STORE_0: u32 = 2;
const VFCOMP_STORE_1_FP: u32 = 3;
const PIPELINE_SELECT_3D: u32 = (4 << 16) | (1 << 24) | (1 << 27) | (3 << 29);
const PIPE_CONTROL_CMD: u32 = 4 | (2 << 24) | (3 << 27) | (3 << 29);
const STATE_BASE_ADDRESS_CMD: u32 = 20 | (1 << 16) | (1 << 24) | (3 << 29);
const BINDING_TABLE_POOL_ENABLE: u32 = 1 << 11;
const BINDING_TABLE_POOL_MOCS_MASK: u32 = 0x7F;
const BINDING_TABLE_POOL_BASE_MASK: u32 = 0xFFFF_F000;
const CMD_3DSTATE_AA_LINE_PARAMETERS: u32 = 1 | (10 << 16) | (1 << 24) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_SAMPLE_PATTERN: u32 = 7 | (28 << 16) | (1 << 24) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_3D_MODE: u32 = 3 | (30 << 16) | (1 << 24) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_SLICE_TABLE_STATE_POINTERS: u32 = (32 << 16) | (1 << 24) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_BINDING_TABLE_POOL_ALLOC: u32 =
    2 | (25 << 16) | (1 << 24) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_CONSTANT_ALL_EMPTY_ALL_STAGES: u32 =
    (109 << 16) | (0x1F << 8) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_VS: u32 = 7 | (16 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_GS: u32 = 8 | (17 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_CLEAR_PARAMS: u32 = 1 | (4 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_DEPTH_BUFFER_GEN12: u32 = 6 | (5 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_DEPTH_BUFFER_GFX125: u32 = 8 | (5 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_STENCIL_BUFFER: u32 = 6 | (6 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_HIER_DEPTH_BUFFER: u32 = 3 | (7 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_CLIP: u32 = 2 | (18 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_SF: u32 = 2 | (19 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_WM: u32 = 0 | (20 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_HS: u32 = 7 | (27 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_TE: u32 = 3 | (28 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_DS: u32 = 9 | (29 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_STREAMOUT: u32 = 3 | (30 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_PRIMITIVE_REPLICATION: u32 = 4 | (108 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_SO_BUFFER_INDEX_0: u32 = 6 | (0x60 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_SO_DECL_LIST_1: u32 = 3 | (23 << 16) | (1 << 24) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_SBE: u32 = 4 | (31 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_PS: u32 = 10 | (32 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_CPS_POINTERS: u32 = (34 << 16) | (3 << 27) | (3 << 29);
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
const CMD_3DSTATE_VFG: u32 = 2 | (87 << 16) | (3 << 27) | (3 << 29);
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
const CMD_3DSTATE_WM_HZ_OP_GEN12: u32 = 3 | (82 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_WM_HZ_OP_GFX125: u32 = 4 | (82 << 16) | (3 << 27) | (3 << 29);
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
const PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER: u32 = 1 << 9;
const PIPE_CONTROL_UNTYPED_DATAPORT_FLUSH_HEADER: u32 = 1 << 11;
const PIPE_CONTROL_DEPTH_CACHE_FLUSH: u32 = 1 << 0;
const PIPE_CONTROL_STALL_AT_SCOREBOARD: u32 = 1 << 1;
const PIPE_CONTROL_DC_FLUSH_ENABLE: u32 = 1 << 5;
const PIPE_CONTROL_FLUSH_ENABLE: u32 = 1 << 7;
const PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH: u32 = 1 << 12;
const PIPE_CONTROL_DEPTH_STALL: u32 = 1 << 13;
const PIPE_CONTROL_FLUSH_HDC: u32 = 1 << 26;
const PIPE_CONTROL_TILE_CACHE_FLUSH: u32 = 1 << 28;
const PIPE_CONTROL_COMMAND_CACHE_INVALIDATE: u32 = 1 << 29;
const PIPE_CONTROL_L3_FABRIC_FLUSH: u32 = 1 << 30;
const PIPE_CONTROL_FLUSH_BITS: u32 = PIPE_CONTROL_DC_FLUSH_ENABLE
    | PIPE_CONTROL_FLUSH_ENABLE
    | PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH
    | PIPE_CONTROL_CS_STALL
    | PIPE_CONTROL_FLUSH_HDC;
const PIPE_CONTROL_INVALIDATE_BITS: u32 =
    (1 << 2) | (1 << 3) | (1 << 4) | (1 << 10) | (1 << 11) | (1 << 18) | (1 << 20);
const PIPE_CONTROL_BIG_PRE_DRAW_HEADER_BITS: u32 =
    PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER | PIPE_CONTROL_UNTYPED_DATAPORT_FLUSH_HEADER;
const PIPE_CONTROL_BIG_PRE_DRAW_BITS: u32 = PIPE_CONTROL_DEPTH_CACHE_FLUSH
    | PIPE_CONTROL_STALL_AT_SCOREBOARD
    | PIPE_CONTROL_INVALIDATE_BITS
    | PIPE_CONTROL_DC_FLUSH_ENABLE
    | PIPE_CONTROL_FLUSH_ENABLE
    | PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH
    | PIPE_CONTROL_DEPTH_STALL
    | PIPE_CONTROL_CS_STALL
    | PIPE_CONTROL_FLUSH_HDC
    | PIPE_CONTROL_TILE_CACHE_FLUSH
    | PIPE_CONTROL_COMMAND_CACHE_INVALIDATE
    | PIPE_CONTROL_L3_FABRIC_FLUSH;
const PIPE_CONTROL_POST_SYNC_WRITE_IMMEDIATE: u32 = 1 << 14;
const PIPE_CONTROL_DEST_GGTT: u32 = 1 << 24;
const PIPE_CONTROL_CS_STALL: u32 = 1 << 20;
const PIPE_CONTROL_POST_DRAW_LIGHT_SYNC_BITS: u32 =
    PIPE_CONTROL_POST_SYNC_WRITE_IMMEDIATE | PIPE_CONTROL_DEST_GGTT | PIPE_CONTROL_CS_STALL;
const PIPE_CONTROL_POST_DRAW_LIGHT_POSTSYNC_NO_STALL_BITS: u32 =
    PIPE_CONTROL_POST_SYNC_WRITE_IMMEDIATE | PIPE_CONTROL_DEST_GGTT;
const PIPE_CONTROL_POST_DRAW_LIGHT_CS_STALL_ONLY_BITS: u32 = PIPE_CONTROL_CS_STALL;
const PIPE_CONTROL_POST_DRAW_SYNC_BITS: u32 =
    PIPE_CONTROL_FLUSH_BITS | PIPE_CONTROL_POST_SYNC_WRITE_IMMEDIATE | PIPE_CONTROL_DEST_GGTT;
const OAR_OACONTROL: usize = 0x2960;
const OAR_OACONTROL_FORMAT_A24_A14_B8_C8: u32 = 5 << 1;
const OAR_OACONTROL_COUNTER_ENABLE: u32 = 1 << 0;
const RCS_OACTXCONTROL: usize = RCS_RING_BASE + 0x360;
const OACTXCONTROL_COUNTER_RESUME: u32 = 1 << 0;
const OAG_OASTARTTRIG1: usize = 0xD900;
const OAG_OASTARTTRIG2: usize = 0xD904;
const OAG_OASTARTTRIG3: usize = 0xD910;
const OAG_OASTARTTRIG4: usize = 0xD914;
const OAG_OAREPORTTRIG1: usize = 0xD920;
const OAG_SPCTR_CNF: usize = 0xDC40;
const OAA_LENABLE_REG: usize = 0xDD40;
const OAG_OA_PESS: usize = 0x2B2C;
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
const RESULT_SLOT_POST3D_LIGHT_PIPE_CONTROL_LO_DWORD: usize = 10;
const RESULT_SLOT_POST3D_LIGHT_PIPE_CONTROL_HI_DWORD: usize = 11;
const RESULT_SLOT_FINAL_AFTER_LIGHT_DWORD: usize = 12;
const RESULT_SLOT_PRE_LIGHT_PC_DWORD: usize = 13;
const RESULT_DEBUG_DWORD_COUNT: usize = RESULT_SLOT_PRE_LIGHT_PC_DWORD + 1;
const RESULT_SLOT_GPGPU_PREFLIGHT_MARKER_DWORD: usize = 16;
const RESULT_SLOT_GPGPU_PREFLIGHT_DOT_DWORD: usize = 17;
const RESULT_SLOT_GPGPU_PREFLIGHT_SUM_A_DWORD: usize = 18;
const RESULT_SLOT_GPGPU_PREFLIGHT_SUM_B_DWORD: usize = 19;
const RESULT_SLOT_GPGPU_PREFLIGHT_LANES_DWORD: usize = 20;
const RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD: usize = 21;
const RESULT_SLOT_GPGPU_EU_C_STORE_DWORD: usize = 22;
const RESULT_OA_REPORT_DWORDS: usize = 64;
const RESULT_OA_BEGIN_DWORD: usize = 64;
const RESULT_OA_END_DWORD: usize = RESULT_OA_BEGIN_DWORD + RESULT_OA_REPORT_DWORDS;
const RESULT_OA_RASTER_WM_BEGIN_ID: u32 = 0x0A0A_2101;
const RESULT_OA_RASTER_WM_END_ID: u32 = 0x0A0A_2102;
const SO_NUM_PRIMS_WRITTEN_0: usize = 0x5200;
const SO_WRITE_OFFSET_0: usize = 0x5280;
const TRIANGLE_TOPOLOGY_POINTLIST: u32 = 1;
const TRIANGLE_TOPOLOGY_LINELIST: u32 = 2;
const TRIANGLE_TOPOLOGY_TRILIST: u32 = 4;
const TRIANGLE_TOPOLOGY_RECTLIST: u32 = 15;
const TRIANGLE_PS_MAX_THREADS: u32 = 63;
const TRIANGLE_VS_URB_START: u32 = 4;
const TRIANGLE_VS_URB_ENTRIES: u32 = 192;
const TRIANGLE_VS_URB_OUTPUT_LENGTH_OVERRIDE: Option<u8> = None;
const TRIANGLE_DEFAULT_FRONT_END_CONTRACT: TriangleFrontEndContract = TriangleFrontEndContract {
    label: "mesa-like",
    vs_urb_output_length_override: TRIANGLE_VS_URB_OUTPUT_LENGTH_OVERRIDE,
    sbe_read_offset: 1,
    sbe_read_length: 1,
    force_sbe_read_offset: true,
    force_sbe_read_length: true,
};
const VS_DRAW_FRONTIER_CONTRACTS: [TriangleFrontEndContract; 4] = [
    TRIANGLE_DEFAULT_FRONT_END_CONTRACT,
    TriangleFrontEndContract {
        label: "slot0-read",
        vs_urb_output_length_override: TRIANGLE_VS_URB_OUTPUT_LENGTH_OVERRIDE,
        sbe_read_offset: 0,
        sbe_read_length: 1,
        force_sbe_read_offset: true,
        force_sbe_read_length: true,
    },
    TriangleFrontEndContract {
        label: "urb2",
        vs_urb_output_length_override: Some(2),
        sbe_read_offset: 1,
        sbe_read_length: 1,
        force_sbe_read_offset: true,
        force_sbe_read_length: true,
    },
    TriangleFrontEndContract {
        label: "urb2-slot0-read",
        vs_urb_output_length_override: Some(2),
        sbe_read_offset: 0,
        sbe_read_length: 1,
        force_sbe_read_offset: true,
        force_sbe_read_length: true,
    },
];
const VS_DRAW_SBE_READ0_CONTRACT: TriangleFrontEndContract = TriangleFrontEndContract {
    label: "sbe-read0",
    vs_urb_output_length_override: TRIANGLE_VS_URB_OUTPUT_LENGTH_OVERRIDE,
    sbe_read_offset: 0,
    sbe_read_length: 0,
    force_sbe_read_offset: true,
    force_sbe_read_length: true,
};
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
