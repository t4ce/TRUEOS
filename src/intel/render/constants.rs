#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const FORCEWAKE_RENDER: usize = 0x0A278;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const FORCEWAKE_GT: usize = 0x0A188;
const FORCEWAKE_ACK_RENDER: usize = 0x0D84;
const FORCEWAKE_ACK_GT: usize = 0x130044;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const FORCEWAKE_KERNEL: u32 = 1 << 0;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const FORCEWAKE_FALLBACK: u32 = 1 << 15;
const TBIMR_BATCH_SIZE_OVERRIDE: u32 = 1 << 1;
const TBIMR_OPEN_BATCH_ENABLE: u32 = 1 << 4;
const TBIMR_FAST_CLIP: u32 = 1 << 5;
const FF_DOP_CLOCK_GATE_DISABLE: u32 = 1 << 1;
const GEN9_FFSC_PERCTX_PREEMPT_CTRL: u32 = 1 << 14;
const GEN12_FF_TESSELLATION_DOP_GATE_DISABLE: u32 = 1 << 19;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const FORCEWAKE_POLL_ITERS: usize = 20_000;
const RCS_RING_BASE: usize = 0x0000_2000;
const RCS_FF_THREAD_MODE: usize = RCS_RING_BASE + 0xA0;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const RCS_CS_GPR_REL_BASE: usize = 0x600;
const RCS_CS_GPR_BASE: usize = RCS_RING_BASE + 0x600;
const RCS_CS_GPR_COUNT: usize = 16;
const RCS_RING_TAIL: usize = RCS_RING_BASE + 0x30;
const RCS_RING_HEAD: usize = RCS_RING_BASE + 0x34;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const RCS_RING_IMR: usize = RCS_RING_BASE + 0xA8;
const RCS_CS_DEBUG_MODE1: usize = RCS_RING_BASE + 0xEC;
const RCS_FF_SLICE_CS_CHICKEN1: usize = RCS_RING_BASE + 0xE0;
const RCS_CS_DEBUG_MODE2: usize = RCS_RING_BASE + 0xD8;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const CS_DEBUG_MODE2_CONSTANT_BUFFER_ADDRESS_OFFSET_DISABLE: u32 = 1 << 4;
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
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const RCS_RING_CONTEXT_CONTROL_REF: usize = RCS_RING_BASE + 0x5A0;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const RCS_RING_MODE_GEN7: usize = RCS_RING_BASE + 0x29C;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const RCS_RING_EXECLIST_SUBMIT_PORT: usize = RCS_RING_BASE + 0x230;
const RCS_RING_EXECLIST_STATUS_LO: usize = RCS_RING_BASE + 0x234;
const RCS_RING_EXECLIST_STATUS_HI: usize = RCS_RING_BASE + 0x238;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const RCS_RING_EXECLIST_CONTROL: usize = RCS_RING_BASE + 0x550;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const RCS_RING_EXECLIST_SQ_LO: usize = RCS_RING_BASE + 0x510;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const RCS_RING_EXECLIST_SQ_HI: usize = RCS_RING_BASE + 0x514;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const RCS_RING_HWS_PGA: usize = RCS_RING_BASE + 0x80;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const GDRST: usize = 0x0000_941C;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const CURSOR_A_OFFSET: usize = 0x70080;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const CURSOR_B_OFFSET: usize = 0x71080;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const CURSOR_C_OFFSET: usize = 0x72080;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const CURSOR_D_OFFSET: usize = 0x73080;
const WARM_RING_BYTES: usize = 4096;
const WARM_CONTEXT_BYTES: usize = 22 * 4096;
const WARM_BATCH_BYTES: usize = RESIDENT_SCENE_PRIMARY_BATCH_BYTES
    + (RESIDENT_SCENE_MAX_DRAWS + 1) * RESIDENT_SCENE_SECONDARY_BATCH_BYTES;
const WARM_DRAW_STATE_BYTES: usize = 16 * 4096;
// Reusable transient geometry staging. This is intentionally a bounded warm
// allocation rather than scene-owned storage. Keep the cap visible so a future
// 4K/8K path can replace it with growable staging instead of silently growing
// permanent kernel memory.
const WARM_VERTEX_BYTES: usize = 128 * 4096;
// Optional target-aware retessellation must stay a small quality optimization.
// Raising the upload allocation must not also authorize an 8x increase in CPU
// tessellation work for contour-heavy glyphs.
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const FONT_MESH_REFINEMENT_BUDGET_BYTES: usize = 16 * 4096;
const WARM_RESULT_BYTES: usize = 4096;
const LINEAR_RENDER_TARGET_PITCH_ALIGN: usize = 64;
// Legacy/resident compatibility probes retain their proven square target.
// Shell2's one-shot stamp path bypasses this scale API and uses a fitted
// rectangular target up to the full scanout dimensions below.
const FONT_PROOF_TARGET_SIZE: usize = 512;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) const FONT_STAMP_DEFAULT_NATIVE_SCALE: u32 = 5;
const FONT_STAMP_BASE_SIZE: usize = 64;
pub(crate) const FONT_STAMP_MAX_NATIVE_SCALE: u32 =
    (FONT_PROOF_TARGET_SIZE / FONT_STAMP_BASE_SIZE) as u32;
pub(crate) const RESIDENT_SCENE_TARGET_WIDTH: usize = 2560;
pub(crate) const RESIDENT_SCENE_TARGET_HEIGHT: usize = 1440;
const WARM_STREAMOUT_BYTES: usize =
    RESIDENT_SCENE_TARGET_WIDTH * RESIDENT_SCENE_TARGET_HEIGHT * core::mem::size_of::<u32>();
// The resident-scene hidden-surface path uses one D32_FLOAT surface. Gen12 depth
// is Y0 tiled and gfx12.5 replaces that layout with the byte-compatible Tile4
// 4 KiB tile. The maximum target is already 128-byte pitch and 32-row aligned.
const RESIDENT_SCENE_DEPTH_TILE_WIDTH_BYTES: usize = 128;
const RESIDENT_SCENE_DEPTH_TILE_HEIGHT_ROWS: usize = 32;
const RESIDENT_SCENE_DEPTH_BYTES: usize = WARM_STREAMOUT_BYTES;
const RESIDENT_SCENE_MSAA_COLOR_TILE_WIDTH_PIXELS: usize = 64;
const RESIDENT_SCENE_MSAA_COLOR_TILE_HEIGHT_PIXELS: usize = 64;
const RESIDENT_SCENE_MSAA_DEPTH_TILE_WIDTH_BYTES: usize = 512;
const RESIDENT_SCENE_MSAA_DEPTH_TILE_HEIGHT_SAMPLE_ROWS: usize = 128;
const RENDER_RING_ENTRY_DWORDS: usize = 4;
const RENDER_RING_ENTRY_BYTES: usize = RENDER_RING_ENTRY_DWORDS * core::mem::size_of::<u32>();
const LRC_STATE_OFFSET_DWORDS: usize = 4096 / core::mem::size_of::<u32>();
const GPU_VA_RING_BASE: u64 = 0x0080_0000;
const GPU_VA_CONTEXT_BASE: u64 = 0x0081_0000;
const GPU_VA_BATCH_BASE: u64 = 0x0175_0000;
const GPU_VA_RESULT_BASE: u64 = 0x0084_0000;
const GPU_VA_DRAW_STATE_BASE: u64 = 0x0086_0000;
// One bounded state slot per resident-scene draw plus the full-screen clear. The
// resident renderer owns this mapping for its lifetime; probe state remains at the
// historical warm-state VA above.
const GPU_VA_RESIDENT_SCENE_STATE_BASE: u64 = 0x3000_0000;
// The retained transform secondary state blob occupies 8 KiB. Keep the
// renderer's bounded batch reservation inside the fixed Render1 GGTT window.
const RESIDENT_SCENE_MAX_DRAWS: usize = 340;
// The full material VS/PS plus relocated optional GS code and aligned
// descriptors exceed 8 KiB. Every draw keeps its own complete 16 KiB slot.
const RESIDENT_SCENE_STATE_SLOT_BYTES: usize = 4 * 4096;
const RESIDENT_SCENE_STATE_BYTES: usize =
    (RESIDENT_SCENE_MAX_DRAWS + 1) * RESIDENT_SCENE_STATE_SLOT_BYTES;
const RESIDENT_SCENE_PRIMARY_BATCH_BYTES: usize = 5 * 4096;
const RESIDENT_SCENE_SECONDARY_BATCH_BYTES: usize = 2 * 4096;
const _: () = {
    assert!(GPU_VA_STREAMOUT_BASE + WARM_STREAMOUT_BYTES as u64 <= GPU_VA_BATCH_BASE);
    assert!(GPU_VA_BATCH_BASE + WARM_BATCH_BYTES as u64 <= GPU_VA_RESIDENT_SCENE_DEPTH_BASE);
};
// Render0 PPGTT-only warm geometry staging. The original 64 KiB allocation
// lived directly below STREAMOUT_BASE; the raised 512 KiB cap starts where the
// persistent-resource arena ends. Its numeric overlap with display Slot 3's
// GGTT reservation is intentional: never install this address in the GGTT.
const GPU_VA_VERTEX_BASE: u64 = 0x2800_0000;
const GPU_VA_STREAMOUT_BASE: u64 = 0x0088_0000;
// The 14.0625 MiB D32 scene depth allocation lives above the warm batch and
// below the GPGPU arena. It never aliases the color target or resident meshes.
const GPU_VA_RESIDENT_SCENE_DEPTH_BASE: u64 = 0x0200_0000;
// gfx12.5 Tile64 4x-MSAA surfaces. Each range has 64 MiB of VA headroom;
// physical storage is allocated lazily at the consumer's actual extent.
const GPU_VA_RESIDENT_SCENE_MSAA_COLOR_BASE: u64 = 0x1000_0000;
const GPU_VA_RESIDENT_SCENE_MSAA_DEPTH_BASE: u64 = 0x1400_0000;
// Render0 owns this PPGTT-only alias arena for direct resident-scene UI4
// targets. It is deliberately disjoint from every fixed and persistent Render0
// mapping below 0x3400_0000. Numerically overlapping display GGTT and media
// PPGTT addresses are separate translation domains and do not alias it.
//
// One 16 MiB slot covers the maximum 1440p RGBA target (0xE10000 bytes), and
// thirty slots admit all ten advertised Helio instances with their complete
// triple-buffer rings concurrently. A slot remains stable for the lifetime of
// its UI4 surface and is unmapped/recycled only when UI4 has proved that the
// frame has neither a writer nor a display/compositor reader.
const GPU_VA_RESIDENT_UI4_FRAME_BASE: u64 = 0x6000_0000;
const GPU_VA_RESIDENT_UI4_FRAME_STRIDE: u64 = 0x0100_0000;
pub(crate) const RESIDENT_UI4_DIRECT_MAPPING_COUNT: usize = 30;
const GPU_VA_RESIDENT_UI4_FRAME_LIMIT: u64 = GPU_VA_RESIDENT_UI4_FRAME_BASE
    + RESIDENT_UI4_DIRECT_MAPPING_COUNT as u64 * GPU_VA_RESIDENT_UI4_FRAME_STRIDE;
const _: () = {
    assert!(GPU_VA_RESIDENT_UI4_FRAME_BASE == 0x6000_0000);
    assert!(GPU_VA_RESIDENT_UI4_FRAME_LIMIT == 0x7E00_0000);
    assert!(GPU_VA_RESIDENT_UI4_FRAME_LIMIT <= 0x8000_0000);
    assert!(GPU_VA_RESIDENT_UI4_FRAME_BASE % GPU_VA_RESIDENT_UI4_FRAME_STRIDE == 0);
    assert!(WARM_STREAMOUT_BYTES as u64 <= GPU_VA_RESIDENT_UI4_FRAME_STRIDE);
    assert!(
        GPU_VA_RESIDENT_SCENE_STATE_BASE + RESIDENT_SCENE_STATE_BYTES as u64
            <= GPU_VA_RESIDENT_UI4_FRAME_BASE
    );
};
// Keep the imported 64 KiB compute mesh outside the 14.0625 MiB 1440p scene
// target at 0x0088_0000..0x0169_0000 and below the batch at 0x0175_0000.
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const GPU_VA_GPGPU_TILE_ARENA_BASE: u64 = 0x0400_0000;
// Long-lived render resources share one collision-free VA allocator. Fonts
// were its first client; Spirit's decoded visual assets use the same lifetime
// and mapping contract without borrowing a fixed address from another owner.
const GPU_VA_PERSISTENT_RESOURCE_BASE: u64 = 0x2000_0000;
const GPU_VA_PERSISTENT_RESOURCE_LIMIT: u64 = 0x2800_0000;
const _: () = {
    assert!(GPU_VA_PERSISTENT_RESOURCE_LIMIT == GPU_VA_VERTEX_BASE);
    assert!(GPU_VA_VERTEX_BASE + WARM_VERTEX_BYTES as u64 <= GPU_VA_RESIDENT_SCENE_STATE_BASE);
};
static PERSISTENT_RESOURCE_GPU_VA_CURSOR: AtomicU64 =
    AtomicU64::new(GPU_VA_PERSISTENT_RESOURCE_BASE);
static PERSISTENT_RESOURCE_GPU_VA_FREE: spin::Mutex<alloc::vec::Vec<(u64, u64)>> =
    spin::Mutex::new(alloc::vec::Vec::new());
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const GPGPU_EU_KERNEL_OFFSET_BYTES: usize = 0x3000;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const EL_CTRL_LOAD: u32 = 1 << 0;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const CTX_CTRL_RS_CTX_ENABLE: u32 = 1 << 1;
const CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT: u32 = 1 << 0;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const CTX_CTRL_ENGINE_CTX_SAVE_INHIBIT: u32 = 1 << 2;
const CTX_CTRL_INHIBIT_SYN_CTX_SWITCH: u32 = 1 << 3;
const CTX_CTRL_OAC_CONTEXT_ENABLE: u32 = 1 << 8;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const CTX_DESC_FORCE_RESTORE: u32 = 1 << 2;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const GEN11_GFX_DISABLE_LEGACY_MODE: u32 = 1 << 3;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const GFX_RUN_LIST_ENABLE: u32 = 1 << 15;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const MODE_IDLE: u32 = 1 << 9;
const RING_MI_MODE_STOP_RING: u32 = 1 << 8;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const GRDOM_RENDER: u32 = 1 << 1;
const MI_BATCH_BUFFER_START_GEN8: u32 = (0x31 << 23) | 1;
const MI_BATCH_PPGTT: u32 = 1 << 8;
// MI_BATCH_BUFFER_END returns to the caller only for a second-level batch.
// Resident scenes use one small secondary per object beneath one frame-level primary
// batch, so the render context is submitted exactly once per scene update.
const MI_BATCH_2ND_LEVEL: u32 = 1 << 22;
const _: () = {
    assert!(MI_BATCH_BUFFER_START_GEN8 == 0x1880_0001);
    assert!(MI_BATCH_BUFFER_START_GEN8 | MI_BATCH_PPGTT == 0x1880_0101);
    assert!(MI_BATCH_BUFFER_START_GEN8 | MI_BATCH_2ND_LEVEL == 0x18C0_0001);
    assert!(MI_BATCH_BUFFER_START_GEN8 | MI_BATCH_PPGTT | MI_BATCH_2ND_LEVEL == 0x18C0_0101);
};
// Gen8+ four-DWORD PPGTT load. Helio's draw stream uses this to feed the
// hardware auto-draw registers directly from its resident 20-byte
// DrawIndexedIndirectArgs records.
const MI_LOAD_REGISTER_MEM: u32 = (0x29 << 23) | 2;
const MI_LOAD_REGISTER_IMM: u32 = 0x1100_0000;
const MI_LRI_CS_MMIO: u32 = 1 << 19;
const MI_LRI_FORCE_POSTED: u32 = 1 << 12;
const MI_REPORT_PERF_COUNT_CMD: u32 = (0x28 << 23) | 2;
const MI_REPORT_PERF_COUNT_USE_GLOBAL_GTT: u32 = 1 << 0;
const MI_BATCH_BUFFER_END: u32 = 0x0500_0000;
const MI_NOOP: u32 = 0;
const INTEL_LEGACY_64B_CONTEXT: u32 = 3;
const GEN8_CTX_VALID: u32 = 1 << 0;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const GEN8_CTX_PPGTT_ENABLE: u32 = 1 << 5;
const GEN8_CTX_PRIVILEGE: u32 = 1 << 8;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const GEN12_CTX_PRIORITY_NORMAL: u32 = 1 << 9;
const GEN8_CTX_ADDRESSING_MODE_SHIFT: u32 = 3;
const GEN12_CTX_RCS_INDIRECT_CTX_OFFSET_DEFAULT: u32 = 0xD;
const RCS_EXEC_RESULT_DONE: u32 = 0xC0DE_7701;
const RCS_EXEC_RESULT_SCENE_RCS_RELEASE_DONE_LO: u32 = 0xC0DE_7741;
const RCS_EXEC_RESULT_SCENE_RCS_RELEASE_DONE_HI: u32 = 0xC0DE_7742;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const RCS_EXEC_RESULT_MI_PROBE_DONE: u32 = 0xC0DE_7711;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const RCS_EXEC_RESULT_MI_SCANOUT_DONE: u32 = 0xC0DE_7713;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const RCS_EXEC_RESULT_GPGPU_PREFLIGHT_DONE: u32 = 0xC0DE_7731;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const RCS_EXEC_RESULT_COMPUTE_WALKER_DONE: u32 = 0xC0DE_7732;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const RCS_EXEC_RESULT_GPGPU_EU_C_STORE_DONE: u32 = 0xC0DE_7733;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const RCS_EXEC_RESULT_3D_NO_DRAW_DONE: u32 = 0xC0DE_7712;
const RCS_EXEC_RESULT_DRAW_PRE3D: u32 = 0xC0DE_7721;
const RCS_EXEC_RESULT_DRAW_POST_VF: u32 = 0xC0DE_7723;
const RCS_EXEC_RESULT_DRAW_POST_VS: u32 = 0xC0DE_7724;
const RCS_EXEC_RESULT_DRAW_POST_PS_STATE: u32 = 0xC0DE_7725;
const RCS_EXEC_RESULT_DRAW_POST_CLIP: u32 = 0xC0DE_7726;
const RCS_EXEC_RESULT_DRAW_POST_RASTER: u32 = 0xC0DE_7727;
const RCS_EXEC_RESULT_DRAW_FINAL_AFTER_LIGHT: u32 = 0xC0DE_7728;
const RCS_EXEC_RESULT_DRAW_PRE_LIGHT_PC: u32 = 0xC0DE_7729;
const RCS_EXEC_RESULT_DRAW_BATCH_ENTRY: u32 = 0xC0DE_772A;
const RCS_EXEC_RESULT_DRAW_POST3D: u32 = 0xC0DE_7722;
const RCS_ARTIFICIAL_FRAGMENT_PRE_COLOR: u32 = 0xA17F_1001;
const RCS_ARTIFICIAL_FRAGMENT_POST_COLOR: u32 = 0xA17F_1002;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const PRIMARY_TRIANGLE_SUBMIT_ATTEMPTS: usize = 3;
// Temporary one-boot quiet switch: keep RCS render/GPGPU probes off while
// validating the rest of boot without render-engine traffic.
const PRIMARY_DISABLE_RENDER_BRINGUP: bool = true;
const RENDER_JOKER_SUBMIT_WHEN_PRIMARY_RENDER_DISABLED: bool = true;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const GPGPU_SUBMIT_WHEN_PRIMARY_RENDER_DISABLED: bool = true;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const MI_STRIPE_COUNT: usize = 12;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const MI_STRIPE_WIDTH_PX: usize = 4;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const MI_STRIPE_X_STEP_PX: u32 = 1;
const PRIMARY_PERIODIC_LOG_EVERY: u32 = 30;
const MI_STORE_DATA_IMM_GGTT_DW1: u32 = 0x1040_0002;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const TS_GPGPU_THREADS_DISPATCHED_LO: usize = 0x2290;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const TS_GPGPU_THREADS_DISPATCHED_HI: usize = 0x2294;
const RENDER_MOCS: u32 = 4;
// The verified Gen12 Mesa draw uses MOCS 4 for vertex buffers.  Gen12 also
// requires L3BypassDisable when vertex data is consumed through the L3
// read-only path; PIPE_CONTROL_INVALIDATE_BITS already invalidates that path.
const VERTEX_BUFFER_MOCS: u32 = 4;
const VERTEX_BUFFER_L3_BYPASS_DISABLE: u32 = 1 << 25;
// Vertex/index buffer addresses are absolute on Gen12.  Mesa therefore keeps
// the indirect-object SBA at zero and opens its range to the architectural
// 20-bit maximum instead of rebasing absolute VERTEX_BUFFER_STATE addresses.
const INDIRECT_OBJECT_SBA_BASE: u64 = 0;
const INDIRECT_OBJECT_SBA_SIZE_BYTES: usize = 0xFFFF_F000;
const BINDLESS_SURFACE_STATE_SIZE: u32 = 0xFFFF_F000;
const GEN12_L3ALLOC: usize = 0xB134;
const GEN12_L3ALLOC_ADL_DEFAULT: u32 = (32 << 1) | (88 << 25);
const GFX125_L3ALLOC_FULL_WAYS: u32 = 1 << 9;
const SURFTYPE_2D: u32 = 1;
const SURFTYPE_BUFFER: u32 = 4;
const SURFTYPE_NULL: u32 = 7;
const SURFACE_FORMAT_RAW: u32 = 0x1FF;
const SURFACE_FORMAT_B8G8R8A8_UNORM: u32 = 192;
const SURFACE_FORMAT_R8G8B8A8_UNORM: u32 = 199;
// Mesa ISL / gfx12 SURFACE_FORMAT: decode RGB before sampler filtering.
const SURFACE_FORMAT_R8G8B8A8_UNORM_SRGB: u32 = 200;
const SURFACE_FORMAT_R32G32B32A32_FLOAT: u32 = 0;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const SURFACE_FORMAT_R32G32B32A32_UINT: u32 = 2;
const SURFACE_FORMAT_R32G32B32_FLOAT: u32 = 64;
const SURFACE_FORMAT_R32G32_FLOAT: u32 = 133;
const SURFACE_FORMAT_R32G32_UINT: u32 = 135;
const DEPTH_SURFACE_FORMAT_D32_FLOAT: u32 = 1;
const COMPARE_FUNCTION_ALWAYS: u8 = 0;
const COMPARE_FUNCTION_LESS: u8 = 2;
const COMPARE_FUNCTION_LEQUAL: u8 = 4;

/// The checked-in Churn front end is physically validated on RPL-S 0xA780 and
/// ADL-S 0x4680 revision 0x0C. The ADL-S proof includes sustained concurrent
/// Render0 retained scenes and Spirit GPGPU work on distinct GuC HWLRCAs and
/// PPGTT roots, with zero context loss or memory CAT faults.
const fn device_supports_churn_forward_native(device_id: u16, revision_id: u8) -> bool {
    device_id == 0xA780 || (device_id == 0x4680 && revision_id == 0x0C)
}

/// The retained-transform compute span is a separate hardware capability from
/// the forward path. Admit only the ADL-S stepping whose exact eight-BTI kernel
/// ABI, compute-to-3D handoff, storage-free VF draw, and Spirit coexistence were
/// exercised on the physical machine.
const fn device_supports_churn_retained_transform(device_id: u16, revision_id: u8) -> bool {
    device_id == 0x4680 && revision_id == 0x0C
}

/// The checked-in adjacency geometry-shader binaries target gfx120 Xe-LP.
/// Admit the two known GT1 UHD 770 steppings explicitly: the captured RPL-S
/// device and the physical ADL-S rig. Keep this as an allow-list rather than
/// matching every Intel `??80` device.
pub(crate) const fn device_supports_adjacency_geometry_shader(
    device_id: u16,
    revision_id: u8,
) -> bool {
    matches!((device_id, revision_id), (0xA780, 0x04) | (0x4680, 0x0C))
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum ChurnHardwareAdmission {
    ValidatedProduction,
    Adls4680Rev0cPhysicalProbe,
}

const fn device_admits_churn_forward_native(
    admission: ChurnHardwareAdmission,
    device_id: u16,
    revision_id: u8,
) -> bool {
    match admission {
        ChurnHardwareAdmission::ValidatedProduction => {
            device_supports_churn_forward_native(device_id, revision_id)
        }
        ChurnHardwareAdmission::Adls4680Rev0cPhysicalProbe => {
            device_id == 0x4680 && revision_id == 0x0C
        }
    }
}

const fn device_admits_churn_retained_transform(
    admission: ChurnHardwareAdmission,
    device_id: u16,
    revision_id: u8,
) -> bool {
    match admission {
        ChurnHardwareAdmission::ValidatedProduction => {
            device_supports_churn_retained_transform(device_id, revision_id)
        }
        ChurnHardwareAdmission::Adls4680Rev0cPhysicalProbe => {
            device_id == 0x4680 && revision_id == 0x0C
        }
    }
}

#[cfg(test)]
mod churn_forward_device_admission_tests {
    use super::{
        ChurnHardwareAdmission, device_admits_churn_forward_native,
        device_admits_churn_retained_transform, device_supports_adjacency_geometry_shader,
        device_supports_churn_forward_native, device_supports_churn_retained_transform,
    };

    #[test]
    fn admits_only_physically_validated_native_targets() {
        assert!(device_supports_churn_forward_native(0xA780, 0x00));
        assert!(device_supports_churn_forward_native(0x4680, 0x0C));
        assert!(!device_supports_churn_forward_native(0x4680, 0x0B));
        assert!(!device_supports_churn_forward_native(0x4680, 0x0D));
        assert!(!device_supports_churn_forward_native(0x56A0, 0x08));
    }

    #[test]
    fn retained_transform_requires_its_own_hardware_proof() {
        assert!(!device_supports_churn_retained_transform(0xA780, 0x00));
        assert!(device_supports_churn_retained_transform(0x4680, 0x0C));
        assert!(!device_supports_churn_retained_transform(0x4680, 0x0B));
        assert!(!device_supports_churn_retained_transform(0x4680, 0x0D));
    }

    #[test]
    fn adjacency_geometry_shader_admits_the_known_gfx120_uhd770_family() {
        assert!(device_supports_adjacency_geometry_shader(0xA780, 0x04));
        assert!(device_supports_adjacency_geometry_shader(0x4680, 0x0C));
        assert!(!device_supports_adjacency_geometry_shader(0xA780, 0x03));
        assert!(!device_supports_adjacency_geometry_shader(0x4680, 0x0B));
        assert!(!device_supports_adjacency_geometry_shader(0xA680, 0x04));
    }

    #[test]
    fn explicit_probe_is_exactly_the_bare_metal_adls_stepping() {
        let probe = ChurnHardwareAdmission::Adls4680Rev0cPhysicalProbe;
        assert!(device_admits_churn_forward_native(probe, 0x4680, 0x0C));
        assert!(device_admits_churn_retained_transform(probe, 0x4680, 0x0C));
        assert!(!device_admits_churn_forward_native(probe, 0x4680, 0x0B));
        assert!(!device_admits_churn_retained_transform(probe, 0xA780, 0x00));
    }
}
const SURFACE_HALIGN_4: u32 = 1;
const SURFACE_HALIGN_128_GFX125: u32 = 3;
const SURFACE_VALIGN_4: u32 = 1;
const SHADER_CHANNEL_RED: u32 = 4;
const SHADER_CHANNEL_GREEN: u32 = 5;
const SHADER_CHANNEL_BLUE: u32 = 6;
const SHADER_CHANNEL_ALPHA: u32 = 7;
const SHADER_CHANNEL_ONE: u32 = 1;
const SBE_ACTIVE_COMPONENT_XYZW_MASK_DWORD: u32 = 0xFFFF_FFFF;
const CLIP_FORCE_CLIP_MODE: u32 = 1 << 16;
const CLIP_PERSPECTIVE_DIVIDE_DISABLE: u32 = 1 << 9;
const CLIP_MODE_ACCEPT_ALL: u32 = 4 << 13;
const WM_FORCE_KILL_PIXEL_OFF: u32 = 1;
const PS_VECTOR_MASK_ENABLE: u32 = 1 << 30;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const PS_SINGLE_PROGRAM_FLOW: u32 = 1 << 31;
const PS_PUSH_CONSTANT_ENABLE: u32 = 1 << 11;
const PS_MAX_THREADS_SHIFT: u32 = 23;
const PS_EXTRA_PIXEL_SHADER_COMPUTES_STENCIL: u32 = 1 << 5;
const PS_EXTRA_PIXEL_SHADER_IS_PER_SAMPLE: u32 = 1 << 6;
const PS_EXTRA_ATTRIBUTE_ENABLE: u32 = 1 << 8;
const PS_EXTRA_SIMPLE_PS_HINT: u32 = 1 << 9;
const PS_EXTRA_ENABLE_PS_DEPENDENCY_ON_CPSIZE_CHANGE: u32 = 1 << 17;
const PS_EXTRA_REQUIRES_NONPERSPECTIVE_BARY_PLANE: u32 = 1 << 19;
const PS_EXTRA_REQUIRES_PERSPECTIVE_BARY_PLANE: u32 = 1 << 20;
const PS_EXTRA_REQUIRES_SOURCE_DEPTH_W_PLANE: u32 = 1 << 21;
const PS_EXTRA_USES_SOURCE_W: u32 = 1 << 23;
const PS_EXTRA_USES_SOURCE_DEPTH: u32 = 1 << 24;
const PS_EXTRA_PIXEL_SHADER_VALID: u32 = 1 << 31;
const VFCOMP_STORE_SRC: u32 = 1;
const VFCOMP_STORE_0: u32 = 2;
const VFCOMP_STORE_1_FP: u32 = 3;
// Gfx12 requires the low-field write masks when selecting the 3D pipeline.
// Bit 4 enables media-sampler DOP clock gating and mask 0x13 applies both it
// and PipelineSelection=3D.  Without these bits the command header parses but
// leaves inherited pipeline/power state unchanged.
const PIPELINE_SELECT_3D: u32 =
    (4 << 16) | (1 << 24) | (1 << 27) | (3 << 29) | (0x13 << 8) | (1 << 4);
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
const CMD_3DSTATE_CONSTANT_ALL_EMPTY_VS_PS: u32 = (109 << 16) | (0x11 << 8) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_PUSH_CONSTANT_ALLOC_VS: u32 = (18 << 16) | (1 << 24) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_PUSH_CONSTANT_ALLOC_HS: u32 = (19 << 16) | (1 << 24) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_PUSH_CONSTANT_ALLOC_DS: u32 = (20 << 16) | (1 << 24) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_PUSH_CONSTANT_ALLOC_GS: u32 = (21 << 16) | (1 << 24) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_PUSH_CONSTANT_ALLOC_PS: u32 = (22 << 16) | (1 << 24) | (3 << 27) | (3 << 29);
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
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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
const CMD_3DSTATE_VF_COMPONENT_PACKING: u32 = 3 | (85 << 16) | (3 << 27) | (3 << 29);
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
const CMD_3DSTATE_INDEX_BUFFER: u32 = 3 | (10 << 16) | (3 << 27) | (3 << 29);
const CMD_3DPRIMITIVE: u32 = 5 | (3 << 24) | (3 << 27) | (3 << 29);
const CMD_3DPRIMITIVE_EXTENDED: u32 = 8 | (1 << 11) | (3 << 24) | (3 << 27) | (3 << 29);
const INDEX_BUFFER_FORMAT_DWORD: u32 = 2;
const INDEX_BUFFER_L3_BYPASS_DISABLE: u32 = 1 << 11;
const PRIMITIVE_INDIRECT_PARAMETER_ENABLE: u32 = 1 << 10;
const PRIMITIVE_VERTEX_ACCESS_RANDOM: u32 = 1 << 8;
const RCS_3DPRIM_START_VERTEX: u32 = 0x2430;
const RCS_3DPRIM_VERTEX_COUNT: u32 = 0x2434;
const RCS_3DPRIM_INSTANCE_COUNT: u32 = 0x2438;
const RCS_3DPRIM_START_INSTANCE: u32 = 0x243C;
const RCS_3DPRIM_BASE_VERTEX: u32 = 0x2440;
const RCS_3DPRIM_XP_BASE_VERTEX: u32 = 0x2690;
const RCS_3DPRIM_XP_DRAW_ID: u32 = 0x2698;
const DRAW_INDEXED_INDIRECT_DWORDS: usize = 5;
const DRAW_INDEXED_INDIRECT_BYTES: usize =
    DRAW_INDEXED_INDIRECT_DWORDS * core::mem::size_of::<u32>();
const PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER: u32 = 1 << 9;
const PIPE_CONTROL_DEPTH_CACHE_FLUSH: u32 = 1 << 0;
const PIPE_CONTROL_STALL_AT_SCOREBOARD: u32 = 1 << 1;
const PIPE_CONTROL_DC_FLUSH_ENABLE: u32 = 1 << 5;
const PIPE_CONTROL_FLUSH_ENABLE: u32 = 1 << 7;
const PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH: u32 = 1 << 12;
// Wa_1409600907 applies to gfx12.0..12.5: every Depth Cache Flush must carry
// Depth Stall, independent of the selected post-sync operation.
const PIPE_CONTROL_DEPTH_STALL: u32 = 1 << 13;
// Gen12 DW1 bit26 is Flush LLC, not HDC Pipeline Flush. It requires a
// post-sync immediate write; HDC Pipeline Flush is DW0 bit9 above.
const PIPE_CONTROL_FLUSH_LLC: u32 = 1 << 26;
const PIPE_CONTROL_TILE_CACHE_FLUSH: u32 = 1 << 28;
const PIPE_CONTROL_COMMAND_CACHE_INVALIDATE: u32 = 1 << 29;
const PIPE_CONTROL_L3_FABRIC_FLUSH: u32 = 1 << 30;
const PIPE_CONTROL_TLB_INVALIDATE: u32 = 1 << 18;
const PIPE_CONTROL_FLUSH_BITS: u32 = PIPE_CONTROL_DC_FLUSH_ENABLE
    | PIPE_CONTROL_FLUSH_ENABLE
    | PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH
    | PIPE_CONTROL_CS_STALL;
const PIPE_CONTROL_INVALIDATE_BITS: u32 = (1 << 2)
    | (1 << 3)
    | (1 << 4)
    | (1 << 10)
    | (1 << 11)
    | PIPE_CONTROL_TLB_INVALIDATE
    | (1 << 20);
// DW0 bit11 does not exist until gfx12.5 and is MBZ on the ADL-S path.
const PIPE_CONTROL_BIG_PRE_DRAW_HEADER_BITS: u32 = PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER;
const PIPE_CONTROL_BIG_PRE_DRAW_BITS: u32 = PIPE_CONTROL_DEPTH_CACHE_FLUSH
    | PIPE_CONTROL_STALL_AT_SCOREBOARD
    | PIPE_CONTROL_INVALIDATE_BITS
    | PIPE_CONTROL_DC_FLUSH_ENABLE
    | PIPE_CONTROL_FLUSH_ENABLE
    | PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH
    | PIPE_CONTROL_DEPTH_STALL
    | PIPE_CONTROL_CS_STALL
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
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const PIPE_CONTROL_POST_DRAW_SYNC_BITS: u32 =
    PIPE_CONTROL_FLUSH_BITS | PIPE_CONTROL_POST_SYNC_WRITE_IMMEDIATE | PIPE_CONTROL_DEST_GGTT;
const _: () = {
    assert!(PIPE_CONTROL_FLUSH_BITS == 0x0010_10A0);
    assert!(PIPE_CONTROL_INVALIDATE_BITS == 0x0014_0C1C);
    assert!(PIPE_CONTROL_FLUSH_BITS & PIPE_CONTROL_FLUSH_LLC == 0);
    assert!(PIPE_CONTROL_BIG_PRE_DRAW_BITS & PIPE_CONTROL_FLUSH_LLC == 0);
    // Every first draw after a lifecycle remap invalidates stale Render0 PPGTT
    // translations before VF/3D can consume the recycled alias.
    assert!(PIPE_CONTROL_BIG_PRE_DRAW_BITS & PIPE_CONTROL_TLB_INVALIDATE != 0);
    assert!(PIPE_CONTROL_BIG_PRE_DRAW_HEADER_BITS == 1 << 9);
};
// Gen12 producer release for a color target written by the 3D pixel backend.
// Render-target and tile-cache writeback are the only cache operations needed
// for this path.  Do not add top-of-pipe invalidations here: those execute at
// parse time and are not part of an end-of-pipe producer release.
const PIPE_CONTROL_SCENE_COLOR_RELEASE_HEADER_BITS: u32 = PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER;
const PIPE_CONTROL_SCENE_COLOR_RELEASE_BITS: u32 = PIPE_CONTROL_DEPTH_CACHE_FLUSH
    | PIPE_CONTROL_DC_FLUSH_ENABLE
    | PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH
    | PIPE_CONTROL_DEPTH_STALL
    | PIPE_CONTROL_TILE_CACHE_FLUSH
    | PIPE_CONTROL_FLUSH_ENABLE
    | PIPE_CONTROL_CS_STALL
    | PIPE_CONTROL_L3_FABRIC_FLUSH;
const _: () = {
    assert!(PIPE_CONTROL_CMD | PIPE_CONTROL_SCENE_COLOR_RELEASE_HEADER_BITS == 0x7A00_0204);
    assert!(PIPE_CONTROL_SCENE_COLOR_RELEASE_BITS == 0x5010_30A1);
};
// A separate post-sync packet follows the cache-release packet.  Its PPGTT
// QWord write is the retirement cookie observed by the host. Keep the generic
// PIPE_CONTROL flush bit on this packet as i915 does for its separated Gen12
// RCS breadcrumb write.
const PIPE_CONTROL_SCENE_RELEASE_MARKER_BITS: u32 =
    PIPE_CONTROL_FLUSH_ENABLE | PIPE_CONTROL_POST_SYNC_WRITE_IMMEDIATE | PIPE_CONTROL_CS_STALL;
// Exact successful gfx12 post-draw completion packet from the Mesa capture.
// It targets PPGTT (DEST_GGTT clear) and avoids HDC flush, whose gfx12 form
// additionally requires a header bit.
const PIPE_CONTROL_POST_DRAW_HOST_SYNC_BITS: u32 = PIPE_CONTROL_STALL_AT_SCOREBOARD
    | PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH
    | PIPE_CONTROL_CS_STALL
    | PIPE_CONTROL_POST_SYNC_WRITE_IMMEDIATE;
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
const RESULT_SLOT_BATCH_ENTRY_DWORD: usize = 14;
// Retained transform/scene diagnostics occupy the contiguous range through
// the primary's secondary-return breadcrumb at slot 30.
const RESULT_DEBUG_DWORD_COUNT: usize = 31;
const RESULT_SLOT_GPGPU_PREFLIGHT_MARKER_DWORD: usize = 16;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const RESULT_SLOT_GPGPU_PREFLIGHT_DOT_DWORD: usize = 17;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const RESULT_SLOT_GPGPU_PREFLIGHT_SUM_A_DWORD: usize = 18;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const RESULT_SLOT_GPGPU_PREFLIGHT_SUM_B_DWORD: usize = 19;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const RESULT_SLOT_GPGPU_PREFLIGHT_LANES_DWORD: usize = 20;
const RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD: usize = 21;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const RESULT_SLOT_GPGPU_EU_C_STORE_DWORD: usize = 22;
// The scene release is written by PIPE_CONTROL as a QWord post-sync
// operation. Keep its destination 8-byte aligned; both DWORDs are checked so
// an old or partially observed cookie cannot manufacture a release proof.
const RESULT_SLOT_SCENE_FRAME_DWORD: usize = 24;
const RESULT_OA_REPORT_DWORDS: usize = 64;
const RESULT_OA_BEGIN_DWORD: usize = 64;
const RESULT_OA_END_DWORD: usize = RESULT_OA_BEGIN_DWORD + RESULT_OA_REPORT_DWORDS;
const RESULT_OA_RASTER_WM_BEGIN_ID: u32 = 0x0A0A_2101;
const RESULT_OA_RASTER_WM_END_ID: u32 = 0x0A0A_2102;
const SO_NUM_PRIMS_WRITTEN_0: usize = 0x5200;
const SO_WRITE_OFFSET_0: usize = 0x5280;
// Intel 3DSTATE_VF_TOPOLOGY / legacy 3DPRIMITIVE topology encodings. These
// are hardware primitive-assembly modes, not a request to rewrite the mesh.
const INTEL_TOPOLOGY_POINTLIST: u32 = 0x01;
const INTEL_TOPOLOGY_LINELIST: u32 = 0x02;
const INTEL_TOPOLOGY_LINESTRIP: u32 = 0x03;
const INTEL_TOPOLOGY_TRILIST: u32 = 0x04;
const INTEL_TOPOLOGY_TRISTRIP: u32 = 0x05;
const INTEL_TOPOLOGY_TRIFAN: u32 = 0x06;
const INTEL_TOPOLOGY_QUADLIST: u32 = 0x07;
const INTEL_TOPOLOGY_QUADSTRIP: u32 = 0x08;
const INTEL_TOPOLOGY_LINELIST_ADJ: u32 = 0x09;
const INTEL_TOPOLOGY_LINESTRIP_ADJ: u32 = 0x0a;
const INTEL_TOPOLOGY_TRILIST_ADJ: u32 = 0x0b;
const INTEL_TOPOLOGY_TRISTRIP_ADJ: u32 = 0x0c;
const INTEL_TOPOLOGY_RECTLIST: u32 = 0x0f;
// Existing probe name is kept local to avoid making the proof machinery look
// like a generic mesh conversion path.
const TRIANGLE_TOPOLOGY_POINTLIST: u32 = INTEL_TOPOLOGY_POINTLIST;
const TRIANGLE_TOPOLOGY_RECTLIST: u32 = 15;
const TRIANGLE_PS_MAX_THREADS: u32 = 63;
const TRIANGLE_VS_URB_START: u32 = 4;
// ADL GT1 with the programmed 32-way URB L3 allocation has 512 KiB of URB.
// Mesa reserves the first 32 KiB (four 8-KiB chunks) for push constants and,
// allocates the hardware maximum of 3576 VS entries without tessellation/GS.
// PBR needs 128 bytes per entry; that also fits this 512-KiB partition, but
// exceeds the reset 16-way/256-KiB partition. See test_render_batch_lri.py.
const TRIANGLE_VS_URB_ENTRIES: u32 = 3576;
const _: () = {
    let adls_urb_bytes = ((GEN12_L3ALLOC_ADL_DEFAULT >> 1) & 0x7F) * 4 * 4096;
    let pbr_required_bytes = TRIANGLE_VS_URB_START * 8192 + TRIANGLE_VS_URB_ENTRIES * 128;
    assert!(pbr_required_bytes <= adls_urb_bytes);
};
const TRIANGLE_VS_URB_OUTPUT_LENGTH_OVERRIDE: Option<u8> = None;
const TRIANGLE_DEFAULT_FRONT_END_CONTRACT: TriangleFrontEndContract = TriangleFrontEndContract {
    label: "mesa-like",
    vs_urb_output_length_override: TRIANGLE_VS_URB_OUTPUT_LENGTH_OVERRIDE,
    vs_urb_read_length: 1,
    sbe_read_offset: 1,
    sbe_read_length: 1,
    force_sbe_read_offset: true,
    force_sbe_read_length: true,
    force_vs_with_vf_synthesized_vue: false,
};
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const VS_DRAW_FRONTIER_CONTRACTS: [TriangleFrontEndContract; 4] = [
    TRIANGLE_DEFAULT_FRONT_END_CONTRACT,
    TriangleFrontEndContract {
        label: "slot0-read",
        vs_urb_output_length_override: TRIANGLE_VS_URB_OUTPUT_LENGTH_OVERRIDE,
        vs_urb_read_length: 1,
        sbe_read_offset: 0,
        sbe_read_length: 1,
        force_sbe_read_offset: true,
        force_sbe_read_length: true,
        force_vs_with_vf_synthesized_vue: false,
    },
    TriangleFrontEndContract {
        label: "urb2",
        vs_urb_output_length_override: Some(2),
        vs_urb_read_length: 1,
        sbe_read_offset: 1,
        sbe_read_length: 1,
        force_sbe_read_offset: true,
        force_sbe_read_length: true,
        force_vs_with_vf_synthesized_vue: false,
    },
    TriangleFrontEndContract {
        label: "urb2-slot0-read",
        vs_urb_output_length_override: Some(2),
        vs_urb_read_length: 1,
        sbe_read_offset: 0,
        sbe_read_length: 1,
        force_sbe_read_offset: true,
        force_sbe_read_length: true,
        force_vs_with_vf_synthesized_vue: false,
    },
];
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const VS_DRAW_SBE_READ0_CONTRACT: TriangleFrontEndContract = TriangleFrontEndContract {
    label: "sbe-read0",
    vs_urb_output_length_override: TRIANGLE_VS_URB_OUTPUT_LENGTH_OVERRIDE,
    vs_urb_read_length: 1,
    sbe_read_offset: 0,
    sbe_read_length: 0,
    force_sbe_read_offset: true,
    force_sbe_read_length: true,
    force_vs_with_vf_synthesized_vue: false,
};
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const VF_VUE_REAL_VS_FRONT_END_CONTRACT: TriangleFrontEndContract = TriangleFrontEndContract {
    label: "vf-vue-clip",
    vs_urb_output_length_override: TRIANGLE_VS_URB_OUTPUT_LENGTH_OVERRIDE,
    vs_urb_read_length: 1,
    sbe_read_offset: 1,
    sbe_read_length: 1,
    force_sbe_read_offset: true,
    force_sbe_read_length: true,
    force_vs_with_vf_synthesized_vue: false,
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
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const VF_STREAMOUT_SLICE_HASH_TABLE_OFFSET: usize = 0x1200;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const TRIANGLE_MIN_DIM: usize = 8;
// This proof path emits one MI_STORE_DATA_IMM per covered pixel, so keep the
// triangle intentionally small until we switch to an actual draw pipeline.
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const TRIANGLE_MAX_W: usize = 20;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const TRIANGLE_MAX_H: usize = 18;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const TRIANGLE_DRAW_VERTICES: usize = 3;
const TRIANGLE_DRAW_VERTEX_DWORDS: usize = crate::intel::shader::TRIANGLE_VERTEX_COMPONENTS;
const TRIANGLE_DRAW_VERTEX_STRIDE: usize = crate::intel::shader::TRIANGLE_VERTEX_STRIDE_BYTES;
const TRIANGLE_STATS_LOG: [crate::intel::stats::RenderStat; 3] = [
    crate::intel::stats::RenderStat::IaVerticesCount,
    crate::intel::stats::RenderStat::IaPrimitivesCount,
    crate::intel::stats::RenderStat::VsInvocationCount,
];
