use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use spin::Mutex;

const FORCEWAKE_RENDER: usize = 0x0A278;
const FORCEWAKE_GT: usize = 0x0A188;
const FORCEWAKE_ACK_RENDER: usize = 0x0D84;
const FORCEWAKE_ACK_GT: usize = 0x130044;
const FORCEWAKE_KERNEL: u32 = 1 << 0;
const FORCEWAKE_FALLBACK: u32 = 1 << 15;
const FORCEWAKE_POLL_ITERS: usize = 20_000;
const FF_DOP_CLOCK_GATE_DISABLE: u32 = 1 << 1;
const RCS_RING_BASE: usize = 0x0000_2000;
const RCS_CS_GPR_REL_BASE: usize = 0x600;
const RCS_CS_GPR_BASE: usize = RCS_RING_BASE + RCS_CS_GPR_REL_BASE;
const RCS_CS_GPR_COUNT: usize = 16;
const RCS_RING_TAIL: usize = RCS_RING_BASE + 0x30;
const RCS_RING_HEAD: usize = RCS_RING_BASE + 0x34;
const RCS_RING_ACTHD_UDW: usize = RCS_RING_BASE + 0x5C;
const RCS_RING_DMA_FADD_UDW: usize = RCS_RING_BASE + 0x60;
const RCS_RING_ACTHD: usize = RCS_RING_BASE + 0x74;
const RCS_RING_INSTPS: usize = RCS_RING_BASE + 0x70;
const RCS_RING_DMA_FADD: usize = RCS_RING_BASE + 0x78;
const RCS_RING_NOPID: usize = RCS_RING_BASE + 0x94;
const RCS_RING_PSMI_CTL: usize = RCS_RING_BASE + 0x50;
const RCS_RING_MI_MODE: usize = RCS_RING_BASE + 0x9C;
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
const RCS_RING_EXECLIST_STATUS_LO: usize = RCS_RING_BASE + 0x234;
const RCS_RING_EXECLIST_STATUS_HI: usize = RCS_RING_BASE + 0x238;
const RCS_RING_EXECLIST_CONTROL: usize = RCS_RING_BASE + 0x550;
const RCS_RING_EXECLIST_SQ_LO: usize = RCS_RING_BASE + 0x510;
const RCS_RING_EXECLIST_SQ_HI: usize = RCS_RING_BASE + 0x514;
const RCS_RING_HWS_PGA: usize = RCS_RING_BASE + 0x80;
const GDRST: usize = 0x0000_941C;
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
const CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT: u32 = 1 << 0;
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
const RCS_EXEC_RESULT_GPGPU_PREFLIGHT_DONE: u32 = 0xC0DE_7731;
const RCS_EXEC_RESULT_COMPUTE_WALKER_DONE: u32 = 0xC0DE_7732;
const PRIMARY_DISABLE_RENDER_BRINGUP: bool = true;
const GPGPU_SUBMIT_WHEN_PRIMARY_RENDER_DISABLED: bool = true;
const MI_STORE_DATA_IMM_GGTT_DW1: u32 = 0x1040_0002;
const TS_GPGPU_THREADS_DISPATCHED_LO: usize = 0x2290;
const TS_GPGPU_THREADS_DISPATCHED_HI: usize = 0x2294;
const RENDER_MOCS: u32 = 1;
const PIPE_CONTROL_CMD: u32 = 4 | (2 << 24) | (3 << 27) | (3 << 29);
const STATE_BASE_ADDRESS_CMD: u32 = 20 | (1 << 16) | (1 << 24) | (3 << 29);
const PIPE_CONTROL_DC_FLUSH_ENABLE: u32 = 1 << 5;
const PIPE_CONTROL_FLUSH_ENABLE: u32 = 1 << 7;
const PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH: u32 = 1 << 12;
const PIPE_CONTROL_FLUSH_HDC: u32 = 1 << 26;
const PIPE_CONTROL_CS_STALL: u32 = 1 << 20;
const PIPE_CONTROL_FLUSH_BITS: u32 = PIPE_CONTROL_DC_FLUSH_ENABLE
    | PIPE_CONTROL_FLUSH_ENABLE
    | PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH
    | PIPE_CONTROL_CS_STALL
    | PIPE_CONTROL_FLUSH_HDC;
const PIPE_CONTROL_INVALIDATE_BITS: u32 =
    PIPE_CONTROL_FLUSH_BITS | (1 << 8) | (1 << 10) | (1 << 11) | (1 << 13);
const RESULT_DEBUG_SENTINEL: u32 = 0xC0DE_7700;
const RESULT_DEBUG_DWORD_COUNT: usize = 29;
const RESULT_SLOT_GPGPU_PREFLIGHT_MARKER_DWORD: usize = 16;
const RESULT_SLOT_GPGPU_PREFLIGHT_DOT_DWORD: usize = 17;
const RESULT_SLOT_GPGPU_PREFLIGHT_SUM_A_DWORD: usize = 18;
const RESULT_SLOT_GPGPU_PREFLIGHT_SUM_B_DWORD: usize = 19;
const RESULT_SLOT_GPGPU_PREFLIGHT_LANES_DWORD: usize = 20;
const RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD: usize = 21;
const RESULT_SLOT_GPGPU_EU_C_STORE_DWORD: usize = 22;
const GPGPU_WALKER_IPEHR_LEN9: u32 = (3 << 29) | (2 << 27) | (1 << 24) | (5 << 16) | 9;

#[derive(Copy, Clone, Debug)]
struct RenderWarmState {
    device_id: u16,
    revision_id: u8,
    mmio_base: usize,
    mmio_len: usize,
    ring_phys: u64,
    ring_virt: *mut u8,
    ring_len: usize,
    context_phys: u64,
    context_virt: *mut u8,
    context_len: usize,
    batch_phys: u64,
    batch_virt: *mut u8,
    batch_len: usize,
    draw_state_phys: u64,
    draw_state_virt: *mut u8,
    draw_state_len: usize,
    vertex_phys: u64,
    vertex_virt: *mut u8,
    vertex_len: usize,
    result_phys: u64,
    result_virt: *mut u8,
    result_len: usize,
    streamout_phys: u64,
    streamout_virt: *mut u8,
    streamout_len: usize,
    gpgpu_arena_phys: u64,
    gpgpu_arena_virt: *mut u8,
    gpgpu_arena_len: usize,
}

unsafe impl Send for RenderWarmState {}
unsafe impl Sync for RenderWarmState {}

static WARM_STATE: Mutex<Option<RenderWarmState>> = Mutex::new(None);

fn empty_warm(dev: crate::intel::Dev) -> RenderWarmState {
    RenderWarmState {
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
        gpgpu_arena_phys: 0,
        gpgpu_arena_virt: core::ptr::null_mut(),
        gpgpu_arena_len: 0,
    }
}

fn warm_once(dev: crate::intel::Dev) -> RenderWarmState {
    if let Some(warm) = *WARM_STATE.lock() {
        return warm;
    }

    let mut warm = empty_warm(dev);
    macro_rules! alloc_part {
        ($field_phys:ident, $field_virt:ident, $field_len:ident, $size:expr, $part:literal) => {
            match crate::dma::alloc($size, crate::intel::WARM_ALIGN) {
                Some((phys, virt)) => {
                    warm.$field_phys = phys;
                    warm.$field_virt = virt;
                    warm.$field_len = $size;
                }
                None => {
                    *WARM_STATE.lock() = Some(warm);
                    crate::log!(
                        "intel/gpgpu: warm alloc failed part={} size=0x{:X}\n",
                        $part,
                        $size,
                    );
                    return warm;
                }
            }
        };
    }

    alloc_part!(ring_phys, ring_virt, ring_len, WARM_RING_BYTES, "ring");
    alloc_part!(context_phys, context_virt, context_len, WARM_CONTEXT_BYTES, "context");
    alloc_part!(batch_phys, batch_virt, batch_len, WARM_BATCH_BYTES, "batch");
    alloc_part!(
        draw_state_phys,
        draw_state_virt,
        draw_state_len,
        WARM_DRAW_STATE_BYTES,
        "draw-state"
    );
    alloc_part!(vertex_phys, vertex_virt, vertex_len, WARM_VERTEX_BYTES, "vertex");
    alloc_part!(result_phys, result_virt, result_len, WARM_RESULT_BYTES, "result");
    alloc_part!(streamout_phys, streamout_virt, streamout_len, WARM_STREAMOUT_BYTES, "streamout");

    match crate::dma::alloc(GPGPU_TILE_ARENA_BYTES, crate::intel::WARM_ALIGN) {
        Some((phys, virt)) => {
            warm.gpgpu_arena_phys = phys;
            warm.gpgpu_arena_virt = virt;
            warm.gpgpu_arena_len = GPGPU_TILE_ARENA_BYTES;
        }
        None => {
            crate::log!(
                "intel/gpgpu: arena alloc failed arena_bytes=0x{:X} tile_rows={} max_tiles=0 enough_for_shape=0\n",
                GPGPU_TILE_ARENA_BYTES,
                GPGPU_TILE_ROWS,
            );
        }
    }

    unsafe {
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.context_virt, 0, warm.context_len);
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.draw_state_virt, 0, warm.draw_state_len);
        core::ptr::write_bytes(warm.vertex_virt, 0, warm.vertex_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
        core::ptr::write_bytes(warm.streamout_virt, 0, warm.streamout_len);
        if !warm.gpgpu_arena_virt.is_null() {
            core::ptr::write_bytes(warm.gpgpu_arena_virt, 0, warm.gpgpu_arena_len);
        }
    }

    *WARM_STATE.lock() = Some(warm);
    warm
}

fn warm_state() -> Option<RenderWarmState> {
    *WARM_STATE.lock()
}

fn forcewake_render_acquire(warm: RenderWarmState) -> bool {
    let dev = warm_dev(warm);
    crate::intel::mmio_write(
        dev,
        FORCEWAKE_RENDER,
        crate::intel::mask_dis(FORCEWAKE_KERNEL | FORCEWAKE_FALLBACK),
    );
    let render_cleared = wait_eq_bool(
        dev,
        FORCEWAKE_ACK_RENDER,
        FORCEWAKE_KERNEL | FORCEWAKE_FALLBACK,
        0,
        FORCEWAKE_POLL_ITERS,
    );

    crate::intel::mmio_write(dev, FORCEWAKE_RENDER, crate::intel::mask_en(FORCEWAKE_KERNEL));
    let render_ok = wait_eq_bool(
        dev,
        FORCEWAKE_ACK_RENDER,
        FORCEWAKE_KERNEL,
        FORCEWAKE_KERNEL,
        FORCEWAKE_POLL_ITERS,
    );

    crate::intel::mmio_write(dev, FORCEWAKE_GT, crate::intel::mask_en(FORCEWAKE_KERNEL));
    let gt_ok = wait_eq_bool(
        dev,
        FORCEWAKE_ACK_GT,
        FORCEWAKE_KERNEL,
        FORCEWAKE_KERNEL,
        FORCEWAKE_POLL_ITERS,
    );
    crate::intel::mmio_write(
        dev,
        RCS_CS_DEBUG_MODE1,
        crate::intel::mask_en(FF_DOP_CLOCK_GATE_DISABLE),
    );
    let cs_debug_mode1 = crate::intel::mmio_read(dev, RCS_CS_DEBUG_MODE1);
    crate::log!(
        "intel/gpgpu: forcewake render_cleared={} render_ack=0x{:08X} gt_ack=0x{:08X} cs_debug_mode1=0x{:08X} ff_dop_cg_disable={} ok={}\n",
        render_cleared as u8,
        crate::intel::mmio_read(dev, FORCEWAKE_ACK_RENDER),
        crate::intel::mmio_read(dev, FORCEWAKE_ACK_GT),
        cs_debug_mode1,
        ((cs_debug_mode1 & FF_DOP_CLOCK_GATE_DISABLE) != 0) as u8,
        (render_ok && gt_ok) as u8,
    );

    render_ok && gt_ok
}

fn warm_dev(warm: RenderWarmState) -> crate::intel::Dev {
    crate::intel::Dev {
        bus: 0,
        slot: 0,
        function: 0,
        device_id: warm.device_id,
        revision_id: warm.revision_id,
        mmio: warm.mmio_base as *mut u8,
        mmio_len: warm.mmio_len,
    }
}

fn wait_eq_bool(dev: crate::intel::Dev, reg: usize, mask: u32, want: u32, n: usize) -> bool {
    for _ in 0..n {
        if (crate::intel::mmio_read(dev, reg) & mask) == want {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

fn guc_status_for_warm(warm: RenderWarmState) -> u32 {
    crate::intel::guc_status(crate::intel::RenderWarmState {
        device_id: warm.device_id,
        revision_id: warm.revision_id,
        mmio_base: warm.mmio_base,
        mmio_len: warm.mmio_len,
    })
}

fn submit_warm_render_batch(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    expected_result: u32,
    expected_result_slot_dword: usize,
    submit_name: &'static str,
) -> bool {
    if is_gpgpu_submit_name(submit_name) {
        recover_render_engine_after_nonretired_submit(dev, warm, "gpgpu-pre-submit");
        crate::intel::mmio_write(dev, GEN12_RCU_MODE, masked_bit_enable(GEN12_RCU_MODE_CCS_ENABLE));
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
    let ctx_ctl_after = rcs_ctx_control_value(false);
    crate::intel::mmio_write(dev, RCS_RING_CONTEXT_CONTROL, ctx_ctl_after);
    crate::intel::mmio_write(dev, RCS_RING_CONTEXT_CONTROL_REF, ctx_ctl_after);
    crate::intel::mmio_write(dev, RCS_RING_MI_MODE, masked_bit_disable(RING_MI_MODE_STOP_RING));
    crate::intel::mmio_write(dev, RCS_RING_HWS_PGA, pphwsp_gpu);

    super::ggtt_invalidate(dev);
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    execlist_submit_port_push(dev, context_desc_lo, context_desc_hi, 0, 0);
    crate::intel::mmio_write(dev, RCS_RING_EXECLIST_CONTROL, EL_CTRL_LOAD);

    crate::log!(
        "intel/gpgpu: {} execlist-start desc=0x{:08X}:0x{:08X} hws=0x{:08X} sq0=0x{:08X}:0x{:08X} sq1=0x{:08X}:0x{:08X} ctx_ctl=0x{:08X} mi_mode=0x{:08X} tail_req=0x{:08X} tail_rb=0x{:08X} gen12_sq_load=1\n",
        submit_name,
        context_desc_hi,
        context_desc_lo,
        crate::intel::mmio_read(dev, RCS_RING_HWS_PGA),
        crate::intel::mmio_read(dev, RCS_RING_EXECLIST_SQ_HI),
        crate::intel::mmio_read(dev, RCS_RING_EXECLIST_SQ_LO),
        crate::intel::mmio_read(dev, RCS_RING_EXECLIST_SQ_HI + 8),
        crate::intel::mmio_read(dev, RCS_RING_EXECLIST_SQ_LO + 8),
        crate::intel::mmio_read(dev, RCS_RING_CONTEXT_CONTROL),
        crate::intel::mmio_read(dev, RCS_RING_MI_MODE),
        ring_tail_bytes as u32,
        crate::intel::mmio_read(dev, RCS_RING_TAIL),
    );

    let mut completed = false;
    let mut iter = 0usize;
    while iter < 4096 {
        let observed = read_result_dword(warm, expected_result_slot_dword);
        if observed == expected_result {
            completed = true;
            break;
        }
        if iter == 0 || iter == 256 || iter == 1024 || iter == 4095 {
            crate::log!(
                "intel/gpgpu: {} poll iter={} head=0x{:08X} tail=0x{:08X} acthd=0x{:08X} ipeir=0x{:08X} ipehr=0x{:08X} eir=0x{:08X} execlist_lo=0x{:08X} execlist_hi=0x{:08X} observed_slot={} observed=0x{:08X} expected=0x{:08X}\n",
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
                expected_result_slot_dword,
                observed,
                expected_result,
            );
        }
        core::hint::spin_loop();
        iter += 1;
    }

    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    let observed = read_result_dword(warm, expected_result_slot_dword);
    crate::log!(
        "intel/gpgpu: {} batch-submit-proof completed={} expected_slot={} expected=0x{:08X} observed=0x{:08X} acthd=0x{:08X} ipehr=0x{:08X} does_not_prove=eu_thread_retire_or_matmul\n",
        submit_name,
        completed as u8,
        expected_result_slot_dword,
        expected_result,
        observed,
        crate::intel::mmio_read(dev, RCS_RING_ACTHD),
        crate::intel::mmio_read(dev, RCS_RING_IPEHR),
    );
    if !completed && is_gpgpu_submit_name(submit_name) {
        log_gpgpu_stall_detail(dev, submit_name);
    }

    completed
}

fn log_gpgpu_stall_detail(dev: crate::intel::Dev, submit_name: &'static str) {
    let acthd = crate::intel::mmio_read(dev, RCS_RING_ACTHD);
    let acthd_batch_off = acthd.saturating_sub(GPU_VA_BATCH_BASE as u32);
    let ipeir = crate::intel::mmio_read(dev, RCS_RING_IPEIR);
    let ipehr = crate::intel::mmio_read(dev, RCS_RING_IPEHR);
    let eir = crate::intel::mmio_read(dev, RCS_RING_EIR);
    let fault_gen8 = crate::intel::mmio_read(dev, GEN8_RING_FAULT_REG);
    let fault_gen12 = crate::intel::mmio_read(dev, GEN12_RING_FAULT_REG);
    let fault_active = if fault_gen12 & 1 != 0 {
        fault_gen12
    } else {
        fault_gen8
    };
    let fault_valid = fault_active & 1;
    let fault_type = (fault_active >> 1) & 0x3;
    let fault_srcid = (fault_active >> 3) & 0xFF;
    let fault_engine = (fault_active >> 12) & 0x1F;
    let sc_instdone = crate::intel::mmio_read(dev, SC_INSTDONE);
    let sampler_instdone = crate::intel::mmio_read(dev, SAMPLER_INSTDONE);
    let row_instdone = crate::intel::mmio_read(dev, ROW_INSTDONE);
    let acthd_hi = crate::intel::mmio_read(dev, RCS_RING_ACTHD_UDW);
    let bbaddr_lo = crate::intel::mmio_read(dev, RCS_RING_BBADDR);
    let bbaddr_hi = crate::intel::mmio_read(dev, RCS_RING_BBADDR_UDW);
    let dma_fadd_lo = crate::intel::mmio_read(dev, RCS_RING_DMA_FADD);
    let dma_fadd_hi = crate::intel::mmio_read(dev, RCS_RING_DMA_FADD_UDW);
    let acthd64 = ((acthd_hi as u64) << 32) | acthd as u64;
    let bbaddr64 = ((bbaddr_hi as u64) << 32) | bbaddr_lo as u64;
    let dma_fadd64 = ((dma_fadd_hi as u64) << 32) | dma_fadd_lo as u64;
    let tdl_thr_status0 = crate::intel::mmio_read(dev, TDL_THR_STATUS0);
    let tdl_thr_status1 = crate::intel::mmio_read(dev, TDL_THR_STATUS1);
    let tdl_thr_disp_count = crate::intel::mmio_read(dev, TDL_THR_DISP_COUNT);
    let tdl_thr_pf_count = crate::intel::mmio_read(dev, TDL_THR_PF_COUNT);
    let tdl_thr_pf_status0 = crate::intel::mmio_read(dev, TDL_THR_PF_STATUS0);
    let tdl_thr_pf_status1 = crate::intel::mmio_read(dev, TDL_THR_PF_STATUS1);
    let row_eu00_ss0_done = (row_instdone >> 16) & 1;
    let row_eu00_ss1_done = (row_instdone >> 7) & 1;
    let walker_header_seen =
        ipehr == GPGPU_WALKER_IPEHR_LEN9 || (ipehr & 0xFFFF_0000) == 0x7105_0000;
    let eu_row_waiting = row_eu00_ss0_done == 0 || row_eu00_ss1_done == 0;
    let cs_fault_seen = ipeir != 0 || eir != 0 || fault_valid != 0;

    crate::log!(
        "intel/gpgpu: {} gpgpu-stall-detail acthd_batch_off=0x{:08X} ipeir=0x{:08X} ipehr=0x{:08X} eir=0x{:08X} instdone=0x{:08X} instpm=0x{:08X} fault_gen8=0x{:08X} fault_gen12=0x{:08X} fault_valid={} fault_type={} fault_srcid={} fault_engine={} fault8_data0=0x{:08X} fault8_data1=0x{:08X} fault12_data0=0x{:08X} fault12_data1=0x{:08X} error=0x{:08X} gfx_mode=0x{:08X} rcu_mode=0x{:08X} cs_debug1=0x{:08X} cs_debug2=0x{:08X} sc_instdone=0x{:08X} sc_extra=0x{:08X} sc_extra2=0x{:08X} sampler_instdone=0x{:08X} row_instdone=0x{:08X}\n",
        submit_name,
        acthd_batch_off,
        ipeir,
        ipehr,
        eir,
        crate::intel::mmio_read(dev, RCS_RING_INSTDONE),
        crate::intel::mmio_read(dev, RCS_RING_INSTPM),
        fault_gen8,
        fault_gen12,
        fault_valid,
        fault_type,
        fault_srcid,
        fault_engine,
        crate::intel::mmio_read(dev, GEN8_FAULT_TLB_DATA0),
        crate::intel::mmio_read(dev, GEN8_FAULT_TLB_DATA1),
        crate::intel::mmio_read(dev, GEN12_FAULT_TLB_DATA0),
        crate::intel::mmio_read(dev, GEN12_FAULT_TLB_DATA1),
        crate::intel::mmio_read(dev, ERROR_GEN6),
        crate::intel::mmio_read(dev, GFX_MODE),
        crate::intel::mmio_read(dev, GEN12_RCU_MODE),
        crate::intel::mmio_read(dev, RCS_CS_DEBUG_MODE1),
        crate::intel::mmio_read(dev, RCS_CS_DEBUG_MODE2),
        sc_instdone,
        crate::intel::mmio_read(dev, SC_INSTDONE_EXTRA),
        crate::intel::mmio_read(dev, SC_INSTDONE_EXTRA2),
        sampler_instdone,
        row_instdone,
    );
    crate::log!(
        "intel/gpgpu: {} gpgpu-engine-snapshot acthd64=0x{:016X} bbaddr64=0x{:016X} dma_fadd64=0x{:016X} bbstate=0x{:08X} esr=0x{:08X} instps=0x{:08X} psmi_ctl=0x{:08X} nopid=0x{:08X}\n",
        submit_name,
        acthd64,
        bbaddr64,
        dma_fadd64,
        crate::intel::mmio_read(dev, RCS_RING_BBSTATE),
        crate::intel::mmio_read(dev, RCS_RING_ESR),
        crate::intel::mmio_read(dev, RCS_RING_INSTPS),
        crate::intel::mmio_read(dev, RCS_RING_PSMI_CTL),
        crate::intel::mmio_read(dev, RCS_RING_NOPID),
    );
    crate::log!(
        "intel/gpgpu: {} gpgpu-tdl-status thr_status0=0x{:08X} thr_status1=0x{:08X} disp_count=0x{:08X} pf_count=0x{:08X} pf_status0=0x{:08X} pf_status1=0x{:08X}\n",
        submit_name,
        tdl_thr_status0,
        tdl_thr_status1,
        tdl_thr_disp_count,
        tdl_thr_pf_count,
        tdl_thr_pf_status0,
        tdl_thr_pf_status1,
    );
    crate::log!(
        "intel/gpgpu: {} gpgpu-middle-state walker_header_seen={} cs_parked_at_walker={} eu_row_waiting={} cs_fault_seen={} row_eu00_ss0_done={} row_eu00_ss1_done={} plain=\"threads were dispatched and the command streamer is waiting at GPGPU_WALKER; public regs show EU row not done, but not per-thread GRFs\"\n",
        submit_name,
        walker_header_seen as u8,
        walker_header_seen as u8,
        eu_row_waiting as u8,
        cs_fault_seen as u8,
        row_eu00_ss0_done,
        row_eu00_ss1_done,
    );
    log_rcs_cs_gprs(dev, submit_name);
}

fn log_rcs_cs_gprs(dev: crate::intel::Dev, submit_name: &'static str) {
    let mut gpr = [0u64; RCS_CS_GPR_COUNT];
    for (idx, value) in gpr.iter_mut().enumerate() {
        *value = read_rcs_cs_gpr(dev, idx);
    }
    crate::log!(
        "intel/gpgpu: {} rcs-cs-gpr base=0x{:05X} count={} gpr0=0x{:016X} gpr1=0x{:016X} gpr2=0x{:016X} gpr3=0x{:016X}\n",
        submit_name,
        RCS_CS_GPR_BASE,
        RCS_CS_GPR_COUNT,
        gpr[0],
        gpr[1],
        gpr[2],
        gpr[3],
    );
    crate::log!(
        "intel/gpgpu: {} rcs-cs-gpr gpr4=0x{:016X} gpr5=0x{:016X} gpr6=0x{:016X} gpr7=0x{:016X}\n",
        submit_name,
        gpr[4],
        gpr[5],
        gpr[6],
        gpr[7],
    );
    crate::log!(
        "intel/gpgpu: {} rcs-cs-gpr gpr8=0x{:016X} gpr9=0x{:016X} gpr10=0x{:016X} gpr11=0x{:016X}\n",
        submit_name,
        gpr[8],
        gpr[9],
        gpr[10],
        gpr[11],
    );
    crate::log!(
        "intel/gpgpu: {} rcs-cs-gpr gpr12=0x{:016X} gpr13=0x{:016X} gpr14=0x{:016X} gpr15=0x{:016X}\n",
        submit_name,
        gpr[12],
        gpr[13],
        gpr[14],
        gpr[15],
    );
}

fn read_rcs_cs_gpr(dev: crate::intel::Dev, index: usize) -> u64 {
    let off = RCS_CS_GPR_BASE + index * core::mem::size_of::<u64>();
    let lo = crate::intel::mmio_read(dev, off) as u64;
    let hi = crate::intel::mmio_read(dev, off + 4) as u64;
    (hi << 32) | lo
}

fn read_result_dword(warm: RenderWarmState, index: usize) -> u32 {
    unsafe { core::ptr::read_volatile((warm.result_virt as *const u32).add(index)) }
}

fn is_gpgpu_submit_name(name: &str) -> bool {
    matches!(name, "gpgpu-preflight" | "gpgpu-compute-walker" | "gpgpu-pre-submit")
}

fn seed_result_debug_slots(warm: RenderWarmState) {
    unsafe {
        for i in 0..RESULT_DEBUG_DWORD_COUNT {
            core::ptr::write_volatile((warm.result_virt as *mut u32).add(i), RESULT_DEBUG_SENTINEL);
        }
    }
}

fn shader_word_signature(words: &[u32]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for &word in words {
        hash ^= word as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn recover_render_engine_after_nonretired_submit(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    submit_name: &'static str,
) {
    crate::log!(
        "intel/gpgpu: {} recovery begin execlist_lo=0x{:08X} mi_mode=0x{:08X} acthd=0x{:08X}\n",
        submit_name,
        crate::intel::mmio_read(dev, RCS_RING_EXECLIST_STATUS_LO),
        crate::intel::mmio_read(dev, RCS_RING_MI_MODE),
        crate::intel::mmio_read(dev, RCS_RING_ACTHD),
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
    super::ggtt_invalidate(dev);
    crate::intel::mmio_write(
        dev,
        RCS_RING_MODE_GEN7,
        masked_bit_enable(GFX_RUN_LIST_ENABLE | GEN11_GFX_DISABLE_LEGACY_MODE),
    );
    let forcewake_ok = forcewake_render_acquire(warm);

    crate::log!(
        "intel/gpgpu: {} recovery end gdrst=0x{:08X} execlist_lo=0x{:08X} mi_mode=0x{:08X} mode=0x{:08X} forcewake_ok={}\n",
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
    static RCS_SUBMIT_COUNTER: AtomicU32 = AtomicU32::new(0);

    let base = (context_gpu_addr as u32) & 0xFFFF_F000;
    let desc = base
        | GEN8_CTX_VALID
        | CTX_DESC_FORCE_RESTORE
        | GEN8_CTX_PRIVILEGE
        | GEN12_CTX_PRIORITY_NORMAL
        | (INTEL_LEGACY_64B_CONTEXT << GEN8_CTX_ADDRESSING_MODE_SHIFT);
    let sw_context_id = (((context_gpu_addr >> 12) as u32) & 0x7FF).max(1);
    let _ = RCS_SUBMIT_COUNTER.fetch_add(1, Ordering::Relaxed);
    let desc_hi = ((context_gpu_addr >> 32) as u32) | (sw_context_id << 7);
    (desc, desc_hi)
}

fn rcs_ctx_control_value(inhibit_restore: bool) -> u32 {
    let mut ctl =
        masked_bits_update(CTX_CTRL_INHIBIT_SYN_CTX_SWITCH, CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT);
    if inhibit_restore {
        ctl |= CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT;
    }
    ctl
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
    crate::intel::mmio_write(dev, RCS_RING_EXECLIST_SQ_LO + 8, context1_lo);
    crate::intel::mmio_write(dev, RCS_RING_EXECLIST_SQ_HI + 8, context1_hi);
}

fn write_lrc_ring_tail(warm: RenderWarmState, ring_tail: u32) {
    const LRC_CONTEXT_CONTROL_VALUE_DW: usize = 3;
    const LRC_RING_TAIL_VALUE_DW: usize = 7;

    let total_dwords = warm.context_len / core::mem::size_of::<u32>();
    if total_dwords <= LRC_STATE_OFFSET_DWORDS + LRC_RING_TAIL_VALUE_DW {
        return;
    }

    let dwords =
        unsafe { core::slice::from_raw_parts_mut(warm.context_virt as *mut u32, total_dwords) };
    let ctx_ctl = dwords[LRC_STATE_OFFSET_DWORDS + LRC_CONTEXT_CONTROL_VALUE_DW];
    dwords[LRC_STATE_OFFSET_DWORDS + LRC_RING_TAIL_VALUE_DW] = ring_tail;
    dwords[LRC_STATE_OFFSET_DWORDS + LRC_CONTEXT_CONTROL_VALUE_DW] = ctx_ctl;
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
    state[idx + 1] = rcs_ctx_control_value(false);
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

const GPGPU_PREFLIGHT_LANES: usize = 4;
const GPGPU_BURN_MIN_ROWS: usize = 512;
const GPGPU_BURN_MIN_K_DIM: usize = 512;
const GPU_PROGRAM_SHARED_RAM_WRITE_EXPECTED: u32 = trueos_eu::gfx12::STORE_SENTINEL_U32;
const GPGPU_LOAD_DUMMY_CURBE: bool = false;
const GPGPU_DUMMY_CURBE_BYTES: usize = 64;
const GPGPU_CONTIGUOUS_VFE_IDD_WALKER: bool = true;
const GPGPU_MESA_POST_VFE_PIPE_CONTROL: bool = false;
// ADL-S 8086:4680 is Gfx12.0/Xe-LP.  The TS_EOT descriptor explicitly says
// SFID_TS ends GPGPU/Media threads.  Gateway EOT was also tested because the
// Gateway section owns barrier/event/SLM cleanup, but it reached the same
// "threads started, no retire" frontier on this walker path.
const ACTIVE_GFX12_EOT_VARIANT: trueos_eu::gfx12::Gfx12EotVariant =
    trueos_eu::gfx12::Gfx12EotVariant::TsR0ToG127;

#[derive(Copy, Clone)]
struct GpgpuEuProgram {
    name: &'static str,
    words: &'static [u32],
    expects_store: bool,
}

// Legacy diagnostic dataport probe.  This hand-written EU blob is not the final
// Burn/matmul kernel path; it is only a bounded oscilloscope for the current
// phase: if the dispatched EU thread can write shared RAM, then we know it
// decoded enough instructions to issue a dataport side effect before/around EOT.
static GPU_PROGRAM_SHARED_RAM_WRITE_CODE: [u32; 12] = [
    0xA07E0061,
    0x00010000,
    0xA0780061,
    GPU_PROGRAM_SHARED_RAM_WRITE_EXPECTED,
    0xA07A0061,
    0x3F810000,
    0xA07C0061,
    0x3F810000,
    0x00040132,
    0x00000004,
    0x50007E14,
    0x00C47834,
];

fn selected_gpgpu_eu_program() -> GpgpuEuProgram {
    let artifact = trueos_eu::gfx12::eot_artifact(ACTIVE_GFX12_EOT_VARIANT);
    GpgpuEuProgram {
        name: artifact.name,
        words: artifact.words,
        expects_store: artifact.expects_store,
    }
}

fn gpgpu_store_eot_program() -> GpgpuEuProgram {
    let artifact = trueos_eu::gfx12::HDC1_BTI34_STORE_THEN_TS_EOT;
    GpgpuEuProgram {
        name: artifact.name,
        words: artifact.words,
        expects_store: artifact.expects_store,
    }
}

const GPGPU_C_STORE_KERNEL_SEND_DWORD: usize = trueos_eu::gfx12::HDC1_BTI34_STORE_SEND_DWORD;
const GPGPU_C_STORE_KERNEL_IMM_DWORD: usize = trueos_eu::gfx12::HDC1_BTI34_STORE_IMM_DWORD;
const GPGPU_STORE_BINDING_TABLE_OFFSET_BYTES: usize = 0x3400;
const GPGPU_STORE_SURFACE_STATE_OFFSET_BYTES: usize = 0x3500;
const GPGPU_STORE_BINDING_TABLE_INDEX: usize = 0x34;
const GPGPU_STORE_BINDING_TABLE_ENTRIES: usize = GPGPU_STORE_BINDING_TABLE_INDEX + 1;
const GPGPU_STORE_SURFACE_DWORDS: usize = 16;
const SURFTYPE_BUFFER: u32 = 4;
const SURFACE_FORMAT_RAW: u32 = 0x1FF;

static GPGPU_PREFLIGHT_SUBMITTED: AtomicBool = AtomicBool::new(false);
static GPGPU_PREFLIGHT_ACCEPTED: AtomicBool = AtomicBool::new(false);
static GPGPU_PREFLIGHT_COMPLETED: AtomicBool = AtomicBool::new(false);
static GPGPU_PREFLIGHT_MARKER: AtomicU32 = AtomicU32::new(0);
static GPGPU_PREFLIGHT_DOT: AtomicU32 = AtomicU32::new(0);
static GPGPU_PREFLIGHT_SUM_A: AtomicU32 = AtomicU32::new(0);
static GPGPU_PREFLIGHT_SUM_B: AtomicU32 = AtomicU32::new(0);
static GPGPU_PREFLIGHT_LANES_OBSERVED: AtomicU32 = AtomicU32::new(0);
static GPGPU_WARM_BUFFERS_MAPPED: AtomicBool = AtomicBool::new(false);
static GPGPU_TILE_ARENA_MAPPED: AtomicBool = AtomicBool::new(false);
static GPGPU_TILE_ARENA_STATUS_LOGGED: AtomicBool = AtomicBool::new(false);
static GPGPU_EU_KERNEL_UPLOADED: AtomicBool = AtomicBool::new(false);
static GPGPU_EU_WALKER_ENCODED: AtomicBool = AtomicBool::new(false);
static GPGPU_EU_WALKER_SUBMITTED: AtomicBool = AtomicBool::new(false);
static GPGPU_EU_WALKER_RETIRED: AtomicBool = AtomicBool::new(false);
static GPGPU_EU_DISPATCH_DELTA: AtomicU32 = AtomicU32::new(0);
static GPGPU_EU_C_STORE_VALUE: AtomicU32 = AtomicU32::new(0);

#[derive(Copy, Clone, Debug)]
pub(crate) struct GpgpuPreflightStatus {
    pub(crate) submitted: bool,
    pub(crate) accepted: bool,
    pub(crate) completed: bool,
    pub(crate) guc_ready: bool,
    pub(crate) marker: u32,
    pub(crate) dot: u32,
    pub(crate) sum_a: u32,
    pub(crate) sum_b: u32,
    pub(crate) lanes: u32,
    pub(crate) min_burn_rows: usize,
    pub(crate) min_burn_k_dim: usize,
    pub(crate) arena_gpu_base: u64,
    pub(crate) arena_bytes: usize,
    pub(crate) tile_rows: usize,
    pub(crate) max_tiles: usize,
    pub(crate) enough_for_shape: bool,
    pub(crate) eu_kernel_uploaded: bool,
    pub(crate) eu_walker_encoded: bool,
    pub(crate) eu_walker_submitted: bool,
    pub(crate) eu_walker_retired: bool,
    pub(crate) eu_dispatch_delta: u32,
    pub(crate) eu_c_store_value: u32,
    pub(crate) result_c_changed_by_eu: bool,
}

pub(crate) fn gpgpu_preflight_status() -> GpgpuPreflightStatus {
    let warm = warm_state();
    let arena_bytes = warm.map_or(0, |warm| warm.gpgpu_arena_len);
    let eu_dispatch_delta = GPGPU_EU_DISPATCH_DELTA.load(Ordering::Acquire);
    let eu_c_store_value = GPGPU_EU_C_STORE_VALUE.load(Ordering::Acquire);
    GpgpuPreflightStatus {
        submitted: GPGPU_PREFLIGHT_SUBMITTED.load(Ordering::Acquire),
        accepted: GPGPU_PREFLIGHT_ACCEPTED.load(Ordering::Acquire),
        completed: GPGPU_PREFLIGHT_COMPLETED.load(Ordering::Acquire),
        guc_ready: crate::intel::guc_ready(),
        marker: GPGPU_PREFLIGHT_MARKER.load(Ordering::Acquire),
        dot: GPGPU_PREFLIGHT_DOT.load(Ordering::Acquire),
        sum_a: GPGPU_PREFLIGHT_SUM_A.load(Ordering::Acquire),
        sum_b: GPGPU_PREFLIGHT_SUM_B.load(Ordering::Acquire),
        lanes: GPGPU_PREFLIGHT_LANES_OBSERVED.load(Ordering::Acquire),
        min_burn_rows: GPGPU_BURN_MIN_ROWS,
        min_burn_k_dim: GPGPU_BURN_MIN_K_DIM,
        arena_gpu_base: gpgpu_arena_gpu_base(arena_bytes),
        arena_bytes,
        tile_rows: GPGPU_TILE_ROWS,
        max_tiles: gpgpu_arena_max_tiles(arena_bytes),
        enough_for_shape: gpgpu_arena_enough_for_shape(arena_bytes),
        eu_kernel_uploaded: GPGPU_EU_KERNEL_UPLOADED.load(Ordering::Acquire),
        eu_walker_encoded: GPGPU_EU_WALKER_ENCODED.load(Ordering::Acquire),
        eu_walker_submitted: GPGPU_EU_WALKER_SUBMITTED.load(Ordering::Acquire),
        eu_walker_retired: GPGPU_EU_WALKER_RETIRED.load(Ordering::Acquire),
        eu_dispatch_delta,
        eu_c_store_value,
        result_c_changed_by_eu: eu_dispatch_delta != 0
            && eu_c_store_value == GPU_PROGRAM_SHARED_RAM_WRITE_EXPECTED,
    }
}

pub(crate) fn submit_gpgpu_preflight_once() {
    if GPGPU_PREFLIGHT_SUBMITTED.swap(true, Ordering::AcqRel) {
        return;
    }

    let Some(dev) = crate::intel::claimed_device() else {
        crate::log!("intel/gpgpu: preflight skipped reason=no-device\n");
        return;
    };

    let warm = warm_once(dev);
    if warm.ring_len == 0
        || warm.context_len == 0
        || warm.batch_len == 0
        || warm.vertex_len < GPGPU_PREFLIGHT_LANES * core::mem::size_of::<u32>()
        || warm.streamout_len < GPGPU_PREFLIGHT_LANES * core::mem::size_of::<u32>()
        || warm.result_len < (RESULT_SLOT_GPGPU_EU_C_STORE_DWORD + 1) * core::mem::size_of::<u32>()
    {
        crate::log!("intel/gpgpu: preflight skipped reason=warm-buffers\n");
        return;
    }

    if PRIMARY_DISABLE_RENDER_BRINGUP && !GPGPU_SUBMIT_WHEN_PRIMARY_RENDER_DISABLED {
        let arena_mapped = ensure_gpgpu_tile_arena_mapped(dev, warm);
        log_gpgpu_tile_arena_status(warm, arena_mapped);
        let eu_artifact = prepare_gpgpu_program_artifact(warm, false);
        log_gpgpu_program_artifact_status(eu_artifact);
        crate::log!(
            "intel/gpgpu: preflight skipped reason=render-bringup-disabled artifact_only=1 gpu_program_uploaded={} start_command_encoded={}\n",
            eu_artifact.program_uploaded as u8,
            eu_artifact.walker_encoded as u8,
        );
        return;
    }
    if PRIMARY_DISABLE_RENDER_BRINGUP {
        crate::log!(
            "intel/gpgpu: primary-render-disabled-but-gpgpu-submit-enabled artifact_only=0\n"
        );
    }

    if !forcewake_render_acquire(warm) {
        crate::log!("intel/gpgpu: preflight skipped reason=forcewake\n");
        return;
    }

    if !ensure_gpgpu_warm_buffers_mapped(dev, warm) {
        crate::log!("intel/gpgpu: preflight skipped reason=warm-buffer-ggtt-map\n");
        return;
    }
    let arena_mapped = ensure_gpgpu_tile_arena_mapped(dev, warm);
    log_gpgpu_tile_arena_status(warm, arena_mapped);
    crate::intel::log_guc_submission_contract(dev, "gpgpu-preflight");
    let accepted = submit_gpgpu_preflight(dev, warm);
    if !accepted {
        recover_render_engine_after_nonretired_submit(dev, warm, "gpgpu-preflight");
    }
    let eu_artifact = prepare_gpgpu_program_artifact(warm, accepted);
    log_gpgpu_program_artifact_status(eu_artifact);
    if eu_artifact.walker_encoded {
        let walker = submit_gpgpu_compute_walker_probe(dev, warm);
        if !walker.retired {
            recover_render_engine_after_nonretired_submit(dev, warm, "gpgpu-compute-walker");
        }
        log_gpgpu_compute_walker_status(walker);
    }
}

fn gpgpu_arena_gpu_base(arena_bytes: usize) -> u64 {
    if arena_bytes == 0 {
        0
    } else {
        GPU_VA_GPGPU_TILE_ARENA_BASE
    }
}

fn gpgpu_arena_max_tiles(arena_bytes: usize) -> usize {
    if arena_bytes <= GPGPU_X_VECTOR_BYTES {
        return 0;
    }
    (arena_bytes - GPGPU_X_VECTOR_BYTES) / (GPGPU_WEIGHT_TILE_BYTES + GPGPU_OUTPUT_TILE_BYTES)
}

fn gpgpu_arena_enough_for_shape(arena_bytes: usize) -> bool {
    gpgpu_arena_max_tiles(arena_bytes) >= GPGPU_TILE_TARGET_TILES
}

fn ensure_gpgpu_warm_buffers_mapped(dev: crate::intel::Dev, warm: RenderWarmState) -> bool {
    if GPGPU_WARM_BUFFERS_MAPPED.load(Ordering::Acquire) {
        return true;
    }

    let mapped = super::map_ggtt(dev, warm.ring_phys, warm.ring_len, GPU_VA_RING_BASE)
        && super::map_ggtt(dev, warm.context_phys, warm.context_len, GPU_VA_CONTEXT_BASE)
        && super::map_ggtt(dev, warm.batch_phys, warm.batch_len, GPU_VA_BATCH_BASE)
        && super::map_ggtt(dev, warm.draw_state_phys, warm.draw_state_len, GPU_VA_DRAW_STATE_BASE)
        && super::map_ggtt(dev, warm.vertex_phys, warm.vertex_len, GPU_VA_VERTEX_BASE)
        && super::map_ggtt(dev, warm.result_phys, warm.result_len, GPU_VA_RESULT_BASE)
        && super::map_ggtt(dev, warm.streamout_phys, warm.streamout_len, GPU_VA_STREAMOUT_BASE);
    if mapped {
        super::ggtt_invalidate(dev);
        GPGPU_WARM_BUFFERS_MAPPED.store(true, Ordering::Release);
    }
    crate::log!(
        "intel/gpgpu: warm-buffers mapped={} ring=0x{:X} context=0x{:X} batch=0x{:X} result=0x{:X}\n",
        mapped as u8,
        GPU_VA_RING_BASE,
        GPU_VA_CONTEXT_BASE,
        GPU_VA_BATCH_BASE,
        GPU_VA_RESULT_BASE,
    );
    mapped
}

fn ensure_gpgpu_tile_arena_mapped(dev: crate::intel::Dev, warm: RenderWarmState) -> bool {
    if GPGPU_TILE_ARENA_MAPPED.load(Ordering::Acquire) {
        return true;
    }
    if warm.gpgpu_arena_len == 0 {
        return false;
    }

    let mapped = super::map_ggtt(
        dev,
        warm.gpgpu_arena_phys,
        warm.gpgpu_arena_len,
        GPU_VA_GPGPU_TILE_ARENA_BASE,
    );
    if mapped {
        super::ggtt_invalidate(dev);
        GPGPU_TILE_ARENA_MAPPED.store(true, Ordering::Release);
    }
    mapped
}

fn log_gpgpu_tile_arena_status(warm: RenderWarmState, mapped: bool) {
    if GPGPU_TILE_ARENA_STATUS_LOGGED.swap(true, Ordering::AcqRel) {
        return;
    }

    let arena_bytes = warm.gpgpu_arena_len;
    crate::log!(
        "intel/gpgpu: arena mapped={} arena_gpu_base=0x{:X} arena_bytes=0x{:X} tile_rows={} max_tiles={} enough_for_shape={} tile_k={} weight_tile_bytes=0x{:X} x_bytes=0x{:X} output_tile_bytes=0x{:X} target_tiles={} does_not_prove=eu_thread_execution_or_matvec\n",
        mapped as u8,
        gpgpu_arena_gpu_base(arena_bytes),
        arena_bytes,
        GPGPU_TILE_ROWS,
        gpgpu_arena_max_tiles(arena_bytes),
        gpgpu_arena_enough_for_shape(arena_bytes) as u8,
        GPGPU_TILE_K_DIM,
        GPGPU_WEIGHT_TILE_BYTES,
        GPGPU_X_VECTOR_BYTES,
        GPGPU_OUTPUT_TILE_BYTES,
        GPGPU_TILE_TARGET_TILES,
    );
}

fn submit_gpgpu_preflight(dev: crate::intel::Dev, warm: RenderWarmState) -> bool {
    let a = [1u32, 2, 3, 4];
    let b = [10u32, 20, 30, 40];
    let sum_a = a.iter().copied().fold(0u32, u32::wrapping_add);
    let sum_b = b.iter().copied().fold(0u32, u32::wrapping_add);
    let dot = a
        .iter()
        .copied()
        .zip(b.iter().copied())
        .fold(0u32, |acc, (lhs, rhs)| acc.wrapping_add(lhs.wrapping_mul(rhs)));

    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);

        let input_a = warm.vertex_virt as *mut u32;
        let input_b = warm.streamout_virt as *mut u32;
        for i in 0..GPGPU_PREFLIGHT_LANES {
            core::ptr::write_volatile(input_a.add(i), a[i]);
            core::ptr::write_volatile(input_b.add(i), b[i]);
        }
    }
    seed_result_debug_slots(warm);
    crate::intel::dma_flush(warm.vertex_virt, GPGPU_PREFLIGHT_LANES * core::mem::size_of::<u32>());
    crate::intel::dma_flush(
        warm.streamout_virt,
        GPGPU_PREFLIGHT_LANES * core::mem::size_of::<u32>(),
    );
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let batch_tail_bytes = match encode_gpgpu_preflight_batch(batch, dot, sum_a, sum_b) {
        Ok(bytes) => bytes,
        Err(reason) => {
            crate::log!("intel/gpgpu: preflight accepted=0 reason={}\n", reason);
            return false;
        }
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);

    let completed = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_GPGPU_PREFLIGHT_DONE,
        RESULT_SLOT_GPGPU_PREFLIGHT_MARKER_DWORD,
        "gpgpu-preflight",
    );
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let marker = read_result_dword(warm, RESULT_SLOT_GPGPU_PREFLIGHT_MARKER_DWORD);
    let gpu_dot = read_result_dword(warm, RESULT_SLOT_GPGPU_PREFLIGHT_DOT_DWORD);
    let gpu_sum_a = read_result_dword(warm, RESULT_SLOT_GPGPU_PREFLIGHT_SUM_A_DWORD);
    let gpu_sum_b = read_result_dword(warm, RESULT_SLOT_GPGPU_PREFLIGHT_SUM_B_DWORD);
    let gpu_lanes = read_result_dword(warm, RESULT_SLOT_GPGPU_PREFLIGHT_LANES_DWORD);
    let guc_status = guc_status_for_warm(warm);
    let accepted = completed
        && marker == RCS_EXEC_RESULT_GPGPU_PREFLIGHT_DONE
        && gpu_dot == dot
        && gpu_sum_a == sum_a
        && gpu_sum_b == sum_b
        && gpu_lanes == GPGPU_PREFLIGHT_LANES as u32;
    GPGPU_PREFLIGHT_COMPLETED.store(completed, Ordering::Release);
    GPGPU_PREFLIGHT_ACCEPTED.store(accepted, Ordering::Release);
    GPGPU_PREFLIGHT_MARKER.store(marker, Ordering::Release);
    GPGPU_PREFLIGHT_DOT.store(gpu_dot, Ordering::Release);
    GPGPU_PREFLIGHT_SUM_A.store(gpu_sum_a, Ordering::Release);
    GPGPU_PREFLIGHT_SUM_B.store(gpu_sum_b, Ordering::Release);
    GPGPU_PREFLIGHT_LANES_OBSERVED.store(gpu_lanes, Ordering::Release);

    crate::log!(
        "intel/gpgpu: preflight-readback accepted={} completed={} result_gpu=0x{:X} marker_slot={} marker_expected=0x{:08X} marker_observed=0x{:08X} dot_slot={} dot_expected={} dot_observed={} sum_a_slot={} sum_a_expected={} sum_a_observed={} sum_b_slot={} sum_b_expected={} sum_b_observed={} lanes_slot={} lanes_expected={} lanes_observed={} batch_bytes=0x{:X} does_not_prove=eu_thread_execution_or_matmul_or_guc_scheduling\n",
        accepted as u8,
        completed as u8,
        GPU_VA_RESULT_BASE,
        RESULT_SLOT_GPGPU_PREFLIGHT_MARKER_DWORD,
        RCS_EXEC_RESULT_GPGPU_PREFLIGHT_DONE,
        marker,
        RESULT_SLOT_GPGPU_PREFLIGHT_DOT_DWORD,
        dot,
        gpu_dot,
        RESULT_SLOT_GPGPU_PREFLIGHT_SUM_A_DWORD,
        sum_a,
        gpu_sum_a,
        RESULT_SLOT_GPGPU_PREFLIGHT_SUM_B_DWORD,
        sum_b,
        gpu_sum_b,
        RESULT_SLOT_GPGPU_PREFLIGHT_LANES_DWORD,
        GPGPU_PREFLIGHT_LANES,
        gpu_lanes,
        batch_tail_bytes,
    );

    crate::log!(
        "intel/gpgpu: preflight accepted={} completed={} backend=rcs-mi-store-constants guc_ready={} guc_status=0x{:08X} lanes={} marker=0x{:08X} dot={} sum_a={} sum_b={} batch_bytes=0x{:X} input_a_gpu=0x{:X} input_b_gpu=0x{:X} result_gpu=0x{:X} next=eu-kernel-dispatch does_not_prove=eu_thread_execution_or_matmul_or_guc_scheduling\n",
        accepted as u8,
        completed as u8,
        crate::intel::guc_ready() as u8,
        guc_status,
        gpu_lanes,
        marker,
        gpu_dot,
        gpu_sum_a,
        gpu_sum_b,
        batch_tail_bytes,
        GPU_VA_VERTEX_BASE,
        GPU_VA_STREAMOUT_BASE,
        GPU_VA_RESULT_BASE,
    );

    accepted
}

#[derive(Copy, Clone)]
struct GpgpuProgramArtifactProof {
    program_name: &'static str,
    expects_store: bool,
    program_uploaded: bool,
    walker_encoded: bool,
    result_changed_by_current_backend: bool,
    program_gpu: u64,
    program_bytes: usize,
    program_sig: u64,
    walker_gpu: u64,
    walker_bytes: usize,
}

#[derive(Copy, Clone)]
struct GpgpuStoreSurfaceState {
    ready: bool,
    binding_table_offset: usize,
    surface_state_offset: usize,
    binding_table_index: usize,
    surface_gpu: u64,
    target_gpu: u64,
    surface_dword0: u32,
    binding_entry: u32,
}

fn prepare_gpgpu_program_artifact(
    warm: RenderWarmState,
    result_changed_by_current_backend: bool,
) -> GpgpuProgramArtifactProof {
    let program = selected_gpgpu_eu_program();
    let program_bytes = program.words.len() * core::mem::size_of::<u32>();
    let program_gpu = GPU_VA_DRAW_STATE_BASE + GPGPU_EU_KERNEL_OFFSET_BYTES as u64;
    let walker_gpu = GPU_VA_BATCH_BASE + GPGPU_WALKER_SCRATCH_OFFSET_BYTES as u64;

    let program_uploaded = program_bytes != 0
        && GPGPU_EU_KERNEL_OFFSET_BYTES
            .checked_add(program_bytes)
            .is_some_and(|end| end <= warm.draw_state_len)
        && upload_and_verify_gpu_program(warm, program.words);
    GPGPU_EU_KERNEL_UPLOADED.store(program_uploaded, Ordering::Release);

    let walker_bytes = core::mem::size_of::<GpgpuWalkerCandidate>();
    let walker_encoded = program_uploaded
        && GPGPU_WALKER_SCRATCH_OFFSET_BYTES
            .checked_add(walker_bytes)
            .is_some_and(|end| end <= warm.batch_len)
        && encode_gpgpu_walker_candidate(warm, program_gpu, program_bytes as u32);
    GPGPU_EU_WALKER_ENCODED.store(walker_encoded, Ordering::Release);

    GpgpuProgramArtifactProof {
        program_name: program.name,
        expects_store: program.expects_store,
        program_uploaded,
        walker_encoded,
        result_changed_by_current_backend,
        program_gpu,
        program_bytes,
        program_sig: shader_word_signature(program.words),
        walker_gpu,
        walker_bytes,
    }
}

fn upload_and_verify_gpu_program(warm: RenderWarmState, program: &'static [u32]) -> bool {
    unsafe {
        core::ptr::copy_nonoverlapping(
            program.as_ptr() as *const u8,
            warm.draw_state_virt.add(GPGPU_EU_KERNEL_OFFSET_BYTES),
            core::mem::size_of_val(program),
        );
    }
    crate::intel::dma_flush(
        unsafe { warm.draw_state_virt.add(GPGPU_EU_KERNEL_OFFSET_BYTES) },
        core::mem::size_of_val(program),
    );
    let uploaded = unsafe {
        core::slice::from_raw_parts(
            warm.draw_state_virt.add(GPGPU_EU_KERNEL_OFFSET_BYTES) as *const u32,
            program.len(),
        )
    };
    uploaded == program
}

#[repr(C)]
#[derive(Copy, Clone)]
struct GpgpuWalkerCandidate {
    magic: u32,
    version: u32,
    simd_lanes: u32,
    kernel_gpu_lo: u32,
    kernel_gpu_hi: u32,
    kernel_bytes: u32,
    input_a_gpu_lo: u32,
    input_a_gpu_hi: u32,
    input_b_gpu_lo: u32,
    input_b_gpu_hi: u32,
    result_c_gpu_lo: u32,
    result_c_gpu_hi: u32,
    lanes: u32,
    reserved: [u32; 3],
}

fn encode_gpgpu_walker_candidate(
    warm: RenderWarmState,
    kernel_gpu: u64,
    kernel_bytes: u32,
) -> bool {
    let candidate = GpgpuWalkerCandidate {
        magic: 0x4750_4757,
        version: 1,
        simd_lanes: 8,
        kernel_gpu_lo: kernel_gpu as u32,
        kernel_gpu_hi: (kernel_gpu >> 32) as u32,
        kernel_bytes,
        input_a_gpu_lo: GPU_VA_VERTEX_BASE as u32,
        input_a_gpu_hi: (GPU_VA_VERTEX_BASE >> 32) as u32,
        input_b_gpu_lo: GPU_VA_STREAMOUT_BASE as u32,
        input_b_gpu_hi: (GPU_VA_STREAMOUT_BASE >> 32) as u32,
        result_c_gpu_lo: GPU_VA_RESULT_BASE as u32,
        result_c_gpu_hi: (GPU_VA_RESULT_BASE >> 32) as u32,
        lanes: GPGPU_PREFLIGHT_LANES as u32,
        reserved: [0; 3],
    };
    unsafe {
        core::ptr::copy_nonoverlapping(
            core::ptr::addr_of!(candidate) as *const u8,
            warm.batch_virt.add(GPGPU_WALKER_SCRATCH_OFFSET_BYTES),
            core::mem::size_of::<GpgpuWalkerCandidate>(),
        );
    }
    crate::intel::dma_flush(
        unsafe { warm.batch_virt.add(GPGPU_WALKER_SCRATCH_OFFSET_BYTES) },
        core::mem::size_of::<GpgpuWalkerCandidate>(),
    );
    true
}

fn log_gpgpu_program_artifact_status(proof: GpgpuProgramArtifactProof) {
    crate::log!(
        "intel/gpgpu: gpu-shared-ram-ladder input_buffer_a_in_ggtt=1 input_buffer_b_in_ggtt=1 input_a_gpu=0x{:X} input_b_gpu=0x{:X} gpu_program_uploaded={} gpu_start_command_encoded={} gpu_program_started=0 shared_ram_c_gpu=0x{:X} shared_ram_c_changed_by_current_backend={} shared_ram_c_changed_by_program=0 cpu_reads_c_back=1 current_backend=rcs-command-store-constants start_submitted=0 blocker=start-gpu-program next=start-program-and-compare-shared-ram does_not_prove=program_body_or_matmul\n",
        GPU_VA_VERTEX_BASE,
        GPU_VA_STREAMOUT_BASE,
        proof.program_uploaded as u8,
        proof.walker_encoded as u8,
        GPU_VA_RESULT_BASE,
        proof.result_changed_by_current_backend as u8,
    );

    crate::log!(
        "intel/gpgpu: gpu-program-artifact gpu_program_uploaded={} gpu_start_command_encoded={} program_source={} expects_store={} program_gpu=0x{:X} program_bytes=0x{:X} program_sig=0x{:016X} start_command_gpu=0x{:X} start_command_bytes=0x{:X} shared_ram_slot={} shared_ram_expected=0x{:08X} submitted=0 started=0 wrote_shared_ram=0 next=start-program-and-compare-shared-ram does_not_prove=program_body_or_matmul\n",
        proof.program_uploaded as u8,
        proof.walker_encoded as u8,
        proof.program_name,
        proof.expects_store as u8,
        proof.program_gpu,
        proof.program_bytes,
        proof.program_sig,
        proof.walker_gpu,
        proof.walker_bytes,
        RESULT_SLOT_GPGPU_EU_C_STORE_DWORD,
        GPU_PROGRAM_SHARED_RAM_WRITE_EXPECTED,
    );
    log_gpgpu_program_contract(proof);
}

fn log_gpgpu_program_contract(proof: GpgpuProgramArtifactProof) {
    let active_program = selected_gpgpu_eu_program();
    let program = active_program.words;
    let immediate = program
        .get(GPGPU_C_STORE_KERNEL_IMM_DWORD)
        .copied()
        .unwrap_or(0);
    crate::log!(
        "intel/gpgpu: gpu-program-contract source={} uploaded={} expects_store={} program_gpu=0x{:X} words={} w0=0x{:08X} w1=0x{:08X} w2=0x{:08X} w3=0x{:08X} w4=0x{:08X} w5=0x{:08X} w6=0x{:08X} w7=0x{:08X} active_send_w8=0x{:08X} active_send_w9=0x{:08X} active_send_desc_w10=0x{:08X} active_send_exdesc_w11=0x{:08X} immediate_expected=0x{:08X} shared_ram_c_gpu=0x{:X} shared_ram_slot={} binding_table_present={} surface_state_present={} curbe_present={} curbe_bytes=0x{:X} expected_failure_if_send_needs_surface={} microscope=program-store-contract does_not_prove=shared_ram_store_or_matmul\n",
        proof.program_name,
        proof.program_uploaded as u8,
        proof.expects_store as u8,
        proof.program_gpu,
        program.len(),
        program.first().copied().unwrap_or(0),
        program.get(1).copied().unwrap_or(0),
        program.get(2).copied().unwrap_or(0),
        immediate,
        program.get(4).copied().unwrap_or(0),
        program.get(5).copied().unwrap_or(0),
        program.get(6).copied().unwrap_or(0),
        program.get(7).copied().unwrap_or(0),
        program.get(8).copied().unwrap_or(0),
        program.get(9).copied().unwrap_or(0),
        program.get(10).copied().unwrap_or(0),
        program.get(11).copied().unwrap_or(0),
        GPU_PROGRAM_SHARED_RAM_WRITE_EXPECTED,
        GPU_VA_RESULT_BASE
            + (RESULT_SLOT_GPGPU_EU_C_STORE_DWORD as u64) * core::mem::size_of::<u32>() as u64,
        RESULT_SLOT_GPGPU_EU_C_STORE_DWORD,
        proof.expects_store as u8,
        proof.expects_store as u8,
        GPGPU_LOAD_DUMMY_CURBE as u8,
        if GPGPU_LOAD_DUMMY_CURBE {
            GPGPU_DUMMY_CURBE_BYTES
        } else {
            0
        },
        proof.expects_store as u8,
    );
    log_gpgpu_eot_send_contract(proof.program_name, proof.program_uploaded, program);
}

fn log_gpgpu_eot_send_contract(
    program_name: &'static str,
    uploaded: bool,
    program: &'static [u32],
) {
    if program.len() < 4 {
        return;
    }
    let send_base = program.len() - 4;
    let send_w0 = program[send_base];
    let send_w1 = program[send_base + 1];
    let send_w2 = program[send_base + 2];
    let send_w3 = program[send_base + 3];
    let eot_bit = (send_w1 >> 2) & 1;
    let desc_is_reg = (send_w1 >> 16) & 1;
    let exdesc_is_reg = (send_w1 >> 17) & 1;
    let dst_reg_file = (send_w1 >> 18) & 1;
    let response_len = (send_w1 >> 19) & 0x1F;
    let dst_reg_num = (send_w1 >> 24) & 0xFF;
    let src0_mlen = (send_w2 >> 3) & 0xF;
    let src0_reg_num = (send_w2 >> 8) & 0xFF;
    let sfid = (send_w2 >> 28) & 0xF;
    let sfid_gateway = sfid == 3;
    let sfid_ts = sfid == 7;
    let dst_null_like = dst_reg_file == 0 && dst_reg_num == 0 && response_len == 0;
    let desc_immediate = desc_is_reg == 0;
    let exdesc_immediate = exdesc_is_reg == 0;
    let src_in_eot_safe_window = (112..=127).contains(&src0_reg_num);
    let target_supports_eot = sfid_gateway || sfid_ts;
    crate::log!(
        "intel/gpgpu: eot-send-contract source={} uploaded={} send_word_off={} send_w0=0x{:08X} send_w1=0x{:08X} send_w2=0x{:08X} send_w3=0x{:08X} eot_bit={} desc_is_reg={} exdesc_is_reg={} desc_immediate={} exdesc_immediate={} dst_reg_file={} dst_reg_num={} response_len={} dst_null_like={} src0_g={} src0_mlen={} src_in_eot_safe_window={} sfid={} sfid_gateway={} sfid_ts={} target_supports_eot={} prm_rules_ok={} probe=selected-pure-eot-artifact expected_good=post_walker_marker-or-eot_retired failure_disproves=selected-eot-payload-shape note=decoded-from-gfx12-send-format\n",
        program_name,
        uploaded as u8,
        send_base,
        send_w0,
        send_w1,
        send_w2,
        send_w3,
        eot_bit,
        desc_is_reg,
        exdesc_is_reg,
        desc_immediate as u8,
        exdesc_immediate as u8,
        dst_reg_file,
        dst_reg_num,
        response_len,
        dst_null_like as u8,
        src0_reg_num,
        src0_mlen,
        src_in_eot_safe_window as u8,
        sfid,
        sfid_gateway as u8,
        sfid_ts as u8,
        target_supports_eot as u8,
        (eot_bit == 1
            && desc_immediate
            && exdesc_immediate
            && dst_null_like
            && src_in_eot_safe_window
            && target_supports_eot) as u8,
    );
}

fn prepare_gpgpu_store_surface_state(warm: RenderWarmState) -> GpgpuStoreSurfaceState {
    let target_gpu = GPU_VA_RESULT_BASE
        + (RESULT_SLOT_GPGPU_EU_C_STORE_DWORD as u64) * core::mem::size_of::<u32>() as u64;
    let binding_table_bytes = GPGPU_STORE_BINDING_TABLE_ENTRIES * core::mem::size_of::<u32>();
    let surface_bytes = GPGPU_STORE_SURFACE_DWORDS * core::mem::size_of::<u32>();
    let binding_end = GPGPU_STORE_BINDING_TABLE_OFFSET_BYTES.saturating_add(binding_table_bytes);
    let surface_end = GPGPU_STORE_SURFACE_STATE_OFFSET_BYTES.saturating_add(surface_bytes);
    let binding_table_aligned = GPGPU_STORE_BINDING_TABLE_OFFSET_BYTES & 0x3F == 0;
    let surface_aligned = GPGPU_STORE_SURFACE_STATE_OFFSET_BYTES & 0x3F == 0;
    let ready = binding_table_aligned
        && surface_aligned
        && binding_end <= warm.draw_state_len
        && surface_end <= warm.draw_state_len;
    if !ready {
        crate::log!(
            "intel/gpgpu: gpu-program-surface-state ready=0 reason=draw-state-bounds bt_off=0x{:X} bt_bytes=0x{:X} surf_off=0x{:X} surf_bytes=0x{:X} draw_state_len=0x{:X}\n",
            GPGPU_STORE_BINDING_TABLE_OFFSET_BYTES,
            binding_table_bytes,
            GPGPU_STORE_SURFACE_STATE_OFFSET_BYTES,
            surface_bytes,
            warm.draw_state_len,
        );
        return GpgpuStoreSurfaceState {
            ready: false,
            binding_table_offset: GPGPU_STORE_BINDING_TABLE_OFFSET_BYTES,
            surface_state_offset: GPGPU_STORE_SURFACE_STATE_OFFSET_BYTES,
            binding_table_index: GPGPU_STORE_BINDING_TABLE_INDEX,
            surface_gpu: GPU_VA_DRAW_STATE_BASE + GPGPU_STORE_SURFACE_STATE_OFFSET_BYTES as u64,
            target_gpu,
            surface_dword0: 0,
            binding_entry: 0,
        };
    }

    let binding_entry = GPGPU_STORE_SURFACE_STATE_OFFSET_BYTES as u32;
    let surface_dword0 = (SURFTYPE_BUFFER << 29) | (SURFACE_FORMAT_RAW << 18);
    unsafe {
        let binding_table = warm
            .draw_state_virt
            .add(GPGPU_STORE_BINDING_TABLE_OFFSET_BYTES) as *mut u32;
        for index in 0..GPGPU_STORE_BINDING_TABLE_ENTRIES {
            core::ptr::write_volatile(binding_table.add(index), binding_entry);
        }

        let surface = warm
            .draw_state_virt
            .add(GPGPU_STORE_SURFACE_STATE_OFFSET_BYTES) as *mut u32;
        for index in 0..GPGPU_STORE_SURFACE_DWORDS {
            core::ptr::write_volatile(surface.add(index), 0);
        }
        core::ptr::write_volatile(surface.add(0), surface_dword0);
        core::ptr::write_volatile(surface.add(1), RENDER_MOCS << 24);
        core::ptr::write_volatile(surface.add(2), 3);
        core::ptr::write_volatile(surface.add(3), 0);
        core::ptr::write_volatile(surface.add(8), target_gpu as u32);
        core::ptr::write_volatile(surface.add(9), (target_gpu >> 32) as u32);
    }
    crate::intel::dma_flush(
        unsafe {
            warm.draw_state_virt
                .add(GPGPU_STORE_BINDING_TABLE_OFFSET_BYTES)
        },
        binding_table_bytes,
    );
    crate::intel::dma_flush(
        unsafe {
            warm.draw_state_virt
                .add(GPGPU_STORE_SURFACE_STATE_OFFSET_BYTES)
        },
        surface_bytes,
    );
    crate::log!(
        "intel/gpgpu: gpu-program-surface-state ready=1 bti=0x{:02X} bt_off=0x{:X} bt_entries={} bt_entry=0x{:08X} surf_off=0x{:X} surf_gpu=0x{:X} target_gpu=0x{:X} surf0=0x{:08X} surf1=0x{:08X} surf2=0x{:08X} surf3=0x{:08X} note=bind-send-bti-to-result-raw-buffer\n",
        GPGPU_STORE_BINDING_TABLE_INDEX,
        GPGPU_STORE_BINDING_TABLE_OFFSET_BYTES,
        GPGPU_STORE_BINDING_TABLE_ENTRIES,
        binding_entry,
        GPGPU_STORE_SURFACE_STATE_OFFSET_BYTES,
        GPU_VA_DRAW_STATE_BASE + GPGPU_STORE_SURFACE_STATE_OFFSET_BYTES as u64,
        target_gpu,
        surface_dword0,
        RENDER_MOCS << 24,
        3,
        0,
    );

    GpgpuStoreSurfaceState {
        ready: true,
        binding_table_offset: GPGPU_STORE_BINDING_TABLE_OFFSET_BYTES,
        surface_state_offset: GPGPU_STORE_SURFACE_STATE_OFFSET_BYTES,
        binding_table_index: GPGPU_STORE_BINDING_TABLE_INDEX,
        surface_gpu: GPU_VA_DRAW_STATE_BASE + GPGPU_STORE_SURFACE_STATE_OFFSET_BYTES as u64,
        target_gpu,
        surface_dword0,
        binding_entry,
    }
}

fn disabled_gpgpu_store_surface_state() -> GpgpuStoreSurfaceState {
    GpgpuStoreSurfaceState {
        ready: false,
        binding_table_offset: GPGPU_STORE_BINDING_TABLE_OFFSET_BYTES,
        surface_state_offset: GPGPU_STORE_SURFACE_STATE_OFFSET_BYTES,
        binding_table_index: GPGPU_STORE_BINDING_TABLE_INDEX,
        surface_gpu: GPU_VA_DRAW_STATE_BASE + GPGPU_STORE_SURFACE_STATE_OFFSET_BYTES as u64,
        target_gpu: GPU_VA_RESULT_BASE
            + (RESULT_SLOT_GPGPU_EU_C_STORE_DWORD as u64) * core::mem::size_of::<u32>() as u64,
        surface_dword0: 0,
        binding_entry: 0,
    }
}

#[derive(Copy, Clone)]
struct GpgpuComputeWalkerProof {
    program_name: &'static str,
    expects_store: bool,
    submitted: bool,
    retired: bool,
    marker: u32,
    dispatch_before: u64,
    dispatch_after: u64,
    dispatch_delta: u64,
    c_value: u32,
    result_c_changed_by_eu: bool,
    expected_hits_mask: u64,
    post_pipeline: u32,
    post_sba: u32,
    post_scm: u32,
    post_cfe: u32,
    post_pre_midl_msf: u32,
    post_curbe_load: u32,
    batch_bytes: usize,
}

#[derive(Copy, Clone)]
struct GpgpuThreadDebugSnapshot {
    ts_dispatched: u64,
    tdl_status0: u32,
    tdl_status1: u32,
    tdl_disp_count: u32,
    tdl_pf_count: u32,
    tdl_pf_status0: u32,
    tdl_pf_status1: u32,
    row_instdone: u32,
    sampler_instdone: u32,
    sc_instdone: u32,
    ring_instdone: u32,
    ring_instps: u32,
    ring_ipehr: u32,
    ring_eir: u32,
}

impl GpgpuThreadDebugSnapshot {
    fn read(dev: crate::intel::Dev) -> Self {
        Self {
            ts_dispatched: read_gpgpu_threads_dispatched(dev),
            tdl_status0: crate::intel::mmio_read(dev, TDL_THR_STATUS0),
            tdl_status1: crate::intel::mmio_read(dev, TDL_THR_STATUS1),
            tdl_disp_count: crate::intel::mmio_read(dev, TDL_THR_DISP_COUNT),
            tdl_pf_count: crate::intel::mmio_read(dev, TDL_THR_PF_COUNT),
            tdl_pf_status0: crate::intel::mmio_read(dev, TDL_THR_PF_STATUS0),
            tdl_pf_status1: crate::intel::mmio_read(dev, TDL_THR_PF_STATUS1),
            row_instdone: crate::intel::mmio_read(dev, ROW_INSTDONE),
            sampler_instdone: crate::intel::mmio_read(dev, SAMPLER_INSTDONE),
            sc_instdone: crate::intel::mmio_read(dev, SC_INSTDONE),
            ring_instdone: crate::intel::mmio_read(dev, RCS_RING_INSTDONE),
            ring_instps: crate::intel::mmio_read(dev, RCS_RING_INSTPS),
            ring_ipehr: crate::intel::mmio_read(dev, RCS_RING_IPEHR),
            ring_eir: crate::intel::mmio_read(dev, RCS_RING_EIR),
        }
    }
}

fn submit_gpgpu_compute_walker_probe(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
) -> GpgpuComputeWalkerProof {
    let program = selected_gpgpu_eu_program();
    let dispatch_before = read_gpgpu_threads_dispatched(dev);
    let marker_slot = RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD;
    unsafe {
        let slot = warm
            .result_virt
            .add(marker_slot * core::mem::size_of::<u32>()) as *mut u32;
        core::ptr::write_volatile(slot, 0);
        let c_slot = warm
            .result_virt
            .add(RESULT_SLOT_GPGPU_EU_C_STORE_DWORD * core::mem::size_of::<u32>())
            as *mut u32;
        core::ptr::write_volatile(c_slot, 0);
        for breadcrumb_slot in 23..=28 {
            let slot =
                warm.result_virt
                    .add(breadcrumb_slot * core::mem::size_of::<u32>()) as *mut u32;
            core::ptr::write_volatile(slot, 0);
        }
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
    }
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let store_surface = if program.expects_store {
        prepare_gpgpu_store_surface_state(warm)
    } else {
        disabled_gpgpu_store_surface_state()
    };
    let batch_bytes =
        match encode_gfx12_gpgpu_walker_probe_batch(warm, batch, store_surface, program) {
            Ok(bytes) => bytes,
            Err(reason) => {
                crate::log!("intel/gpgpu: compute-walker accepted=0 reason={}\n", reason);
                return GpgpuComputeWalkerProof {
                    program_name: program.name,
                    expects_store: program.expects_store,
                    submitted: false,
                    retired: false,
                    marker: 0,
                    dispatch_before,
                    dispatch_after: dispatch_before,
                    dispatch_delta: 0,
                    c_value: 0,
                    result_c_changed_by_eu: false,
                    expected_hits_mask: 0,
                    post_pipeline: 0,
                    post_sba: 0,
                    post_scm: 0,
                    post_cfe: 0,
                    post_pre_midl_msf: 0,
                    post_curbe_load: 0,
                    batch_bytes: 0,
                };
            }
        };
    crate::intel::dma_flush(warm.batch_virt, batch_bytes);
    let debug_before = GpgpuThreadDebugSnapshot::read(dev);

    GPGPU_EU_WALKER_SUBMITTED.store(true, Ordering::Release);
    let retired = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD,
        "gpgpu-compute-walker",
    );
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let marker = read_result_dword(warm, RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD);
    let c_value = read_result_dword(warm, RESULT_SLOT_GPGPU_EU_C_STORE_DWORD);
    let post_pipeline = read_result_dword(warm, 23);
    let post_sba = read_result_dword(warm, 24);
    let post_scm = read_result_dword(warm, 25);
    let post_cfe = read_result_dword(warm, 26);
    let post_pre_midl_msf = read_result_dword(warm, 27);
    let post_curbe_load = read_result_dword(warm, 28);
    let mut expected_hits_mask = 0u64;
    for slot in 0..64 {
        if read_result_dword(warm, slot) == GPU_PROGRAM_SHARED_RAM_WRITE_EXPECTED {
            expected_hits_mask |= 1u64 << slot;
        }
    }
    let dispatch_after = read_gpgpu_threads_dispatched(dev);
    let debug_after = GpgpuThreadDebugSnapshot::read(dev);
    let dispatch_delta = dispatch_after.saturating_sub(dispatch_before);
    let debug_ts_delta = debug_after
        .ts_dispatched
        .saturating_sub(debug_before.ts_dispatched);
    let tdl_disp_delta = debug_after
        .tdl_disp_count
        .wrapping_sub(debug_before.tdl_disp_count);
    let tdl_pf_delta = debug_after
        .tdl_pf_count
        .wrapping_sub(debug_before.tdl_pf_count);
    let row_changed = debug_before.row_instdone != debug_after.row_instdone;
    let sampler_changed = debug_before.sampler_instdone != debug_after.sampler_instdone;
    let sc_changed = debug_before.sc_instdone != debug_after.sc_instdone;
    let ring_changed = debug_before.ring_instdone != debug_after.ring_instdone
        || debug_before.ring_instps != debug_after.ring_instps
        || debug_before.ring_ipehr != debug_after.ring_ipehr
        || debug_before.ring_eir != debug_after.ring_eir;
    crate::log!(
        "intel/gpgpu: eu-fetch-visible-counters program_source={} ts_before={} ts_after={} ts_delta={} tdl_disp_before=0x{:08X} tdl_disp_after=0x{:08X} tdl_disp_delta=0x{:08X} tdl_pf_before=0x{:08X} tdl_pf_after=0x{:08X} tdl_pf_delta=0x{:08X} tdl_status0_before=0x{:08X} tdl_status0_after=0x{:08X} tdl_status1_before=0x{:08X} tdl_status1_after=0x{:08X} pf_status0_before=0x{:08X} pf_status0_after=0x{:08X} pf_status1_before=0x{:08X} pf_status1_after=0x{:08X} row_before=0x{:08X} row_after=0x{:08X} row_changed={} sampler_before=0x{:08X} sampler_after=0x{:08X} sampler_changed={} sc_before=0x{:08X} sc_after=0x{:08X} sc_changed={} ring_instdone_before=0x{:08X} ring_instdone_after=0x{:08X} instps_before=0x{:08X} instps_after=0x{:08X} ipehr_before=0x{:08X} ipehr_after=0x{:08X} eir_before=0x{:08X} eir_after=0x{:08X} ring_changed={} meaning=\"TS is allocation/dispatch accounting; TDL/ROW/SAMPLER/SC deltas are the stronger public clues for thread load, fault, and EU-side progress\"\n",
        program.name,
        debug_before.ts_dispatched,
        debug_after.ts_dispatched,
        debug_ts_delta,
        debug_before.tdl_disp_count,
        debug_after.tdl_disp_count,
        tdl_disp_delta,
        debug_before.tdl_pf_count,
        debug_after.tdl_pf_count,
        tdl_pf_delta,
        debug_before.tdl_status0,
        debug_after.tdl_status0,
        debug_before.tdl_status1,
        debug_after.tdl_status1,
        debug_before.tdl_pf_status0,
        debug_after.tdl_pf_status0,
        debug_before.tdl_pf_status1,
        debug_after.tdl_pf_status1,
        debug_before.row_instdone,
        debug_after.row_instdone,
        row_changed as u8,
        debug_before.sampler_instdone,
        debug_after.sampler_instdone,
        sampler_changed as u8,
        debug_before.sc_instdone,
        debug_after.sc_instdone,
        sc_changed as u8,
        debug_before.ring_instdone,
        debug_after.ring_instdone,
        debug_before.ring_instps,
        debug_after.ring_instps,
        debug_before.ring_ipehr,
        debug_after.ring_ipehr,
        debug_before.ring_eir,
        debug_after.ring_eir,
        ring_changed as u8,
    );
    let result_c_changed_by_eu = program.expects_store
        && c_value == GPU_PROGRAM_SHARED_RAM_WRITE_EXPECTED
        && dispatch_delta != 0;
    GPGPU_EU_WALKER_RETIRED.store(retired, Ordering::Release);
    GPGPU_EU_DISPATCH_DELTA.store(dispatch_delta.min(u32::MAX as u64) as u32, Ordering::Release);
    GPGPU_EU_C_STORE_VALUE.store(c_value, Ordering::Release);
    let breadcrumbs_ok = if GPGPU_CONTIGUOUS_VFE_IDD_WALKER {
        post_pipeline == 0xC0DE_7801 && post_sba == 0xC0DE_7802 && post_scm == 0xC0DE_7803
    } else {
        post_pipeline == 0xC0DE_7801
            && post_sba == 0xC0DE_7802
            && post_scm == 0xC0DE_7803
            && post_cfe == 0xC0DE_7804
            && post_pre_midl_msf == 0xC0DE_7805
            && post_curbe_load == 0xC0DE_7806
    };
    crate::log!(
        "intel/gpgpu: result-store-scan expected=0x{:08X} hits_mask_lo64=0x{:016X} target_slot={} target_gpu=0x{:X} target_value=0x{:08X} breadcrumbs_ok={} contiguous_vfe_idd_walker={} post_pipeline=0x{:08X} post_sba=0x{:08X} post_scm=0x{:08X} post_cfe=0x{:08X} post_pre_midl_msf=0x{:08X} post_curbe_load=0x{:08X} note=scans-result-slots-0-63-for-misplaced-eu-store\n",
        GPU_PROGRAM_SHARED_RAM_WRITE_EXPECTED,
        expected_hits_mask,
        RESULT_SLOT_GPGPU_EU_C_STORE_DWORD,
        GPU_VA_RESULT_BASE
            + (RESULT_SLOT_GPGPU_EU_C_STORE_DWORD as u64) * core::mem::size_of::<u32>() as u64,
        c_value,
        breadcrumbs_ok as u8,
        GPGPU_CONTIGUOUS_VFE_IDD_WALKER as u8,
        post_pipeline,
        post_sba,
        post_scm,
        post_cfe,
        post_pre_midl_msf,
        post_curbe_load,
    );

    GpgpuComputeWalkerProof {
        program_name: program.name,
        expects_store: program.expects_store,
        submitted: true,
        retired,
        marker,
        dispatch_before,
        dispatch_after,
        dispatch_delta,
        c_value,
        result_c_changed_by_eu,
        expected_hits_mask,
        post_pipeline,
        post_sba,
        post_scm,
        post_cfe,
        post_pre_midl_msf,
        post_curbe_load,
        batch_bytes,
    }
}

fn read_gpgpu_threads_dispatched(dev: crate::intel::Dev) -> u64 {
    let lo = crate::intel::mmio_read(dev, TS_GPGPU_THREADS_DISPATCHED_LO) as u64;
    let hi = crate::intel::mmio_read(dev, TS_GPGPU_THREADS_DISPATCHED_HI) as u64;
    (hi << 32) | lo
}

// Disabled reference only.  COMPUTE_WALKER/CFE_STATE is for GFX12.5+ (for
// example DG2); the current baremetal target is 8086:4680 ADL-S GT1/UHD 770,
// a GFX12.0 part.  Submitting this path on that device pins at CFE_STATE before
// any EU thread starts, so runtime dispatch intentionally never calls it.
#[allow(dead_code)]
fn encode_gfx125_compute_walker_probe_batch(
    batch_dwords: &mut [u32],
    store_surface: GpgpuStoreSurfaceState,
    program: GpgpuEuProgram,
) -> Result<usize, &'static str> {
    const STATE_COMPUTE_MODE_CMD: u32 = (3 << 29) | (1 << 24) | (5 << 16);
    const CFE_STATE_CMD: u32 = (3 << 29) | (2 << 27) | (2 << 24) | 4;
    const COMPUTE_WALKER_CMD: u32 = (3 << 29) | (2 << 27) | (2 << 24) | (2 << 18) | 37;
    const PIPELINE_SELECT_BASE: u32 = (3 << 29) | (1 << 27) | (1 << 24) | (4 << 16);
    const PIPELINE_SELECT_GFX125_MASK: u32 = 0x93 << 8;
    const PIPELINE_SELECT_MEDIA_SAMPLER_DOP_CG_ENABLE: u32 = 1 << 4;
    const PIPELINE_SELECT_GPGPU: u32 = PIPELINE_SELECT_BASE
        | PIPELINE_SELECT_GFX125_MASK
        | PIPELINE_SELECT_MEDIA_SAMPLER_DOP_CG_ENABLE
        | 2;
    const COMPUTE_SBA_SPAN_BYTES: usize = 0x1000_0000;
    const CS_GPR_STAMP_HI: u32 = 0x0000_0001;
    const CS_GPR0_STAMP_LO: u32 = 0xC5A0_2650;
    const CS_GPR1_STAMP_LO: u32 = 0xC5A0_2658;
    const COMPUTE_WALKER_BODY_DWORDS: usize = 38;
    const COMPUTE_WALKER_DWORDS: usize = 1 + COMPUTE_WALKER_BODY_DWORDS;
    const BODY_INTERFACE_DESCRIPTOR_DWORD: usize = 17;
    const BODY_POSTSYNC_DWORD: usize = 25;

    fn push(batch_dwords: &mut [u32], cursor: &mut usize, value: u32) -> Result<(), &'static str> {
        if *cursor >= batch_dwords.len() {
            return Err("compute-walker-batch-exhausted");
        }
        batch_dwords[*cursor] = value;
        *cursor += 1;
        Ok(())
    }

    fn push_pipe_control_full(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        header_flags: u32,
        dw1_flags: u32,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, PIPE_CONTROL_CMD | header_flags)?;
        push(batch_dwords, cursor, dw1_flags)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)
    }

    fn push_pipe_control(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        flags: u32,
    ) -> Result<(), &'static str> {
        push_pipe_control_full(batch_dwords, cursor, 0, flags)
    }

    fn push_store_marker(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        slot: usize,
        value: u32,
    ) -> Result<(), &'static str> {
        let dst = GPU_VA_RESULT_BASE + (slot as u64) * core::mem::size_of::<u32>() as u64;
        push(batch_dwords, cursor, MI_STORE_DATA_IMM_GGTT_DW1)?;
        push(batch_dwords, cursor, dst as u32)?;
        push(batch_dwords, cursor, (dst >> 32) as u32)?;
        push(batch_dwords, cursor, value)
    }

    fn push_cs_gpr_stamp(batch_dwords: &mut [u32], cursor: &mut usize) -> Result<(), &'static str> {
        push(batch_dwords, cursor, mi_lri_cmd(4, MI_LRI_FORCE_POSTED))?;
        push(batch_dwords, cursor, RCS_CS_GPR_REL_BASE as u32)?;
        push(batch_dwords, cursor, CS_GPR0_STAMP_LO)?;
        push(batch_dwords, cursor, (RCS_CS_GPR_REL_BASE + 4) as u32)?;
        push(batch_dwords, cursor, CS_GPR_STAMP_HI)?;
        push(batch_dwords, cursor, (RCS_CS_GPR_REL_BASE + 8) as u32)?;
        push(batch_dwords, cursor, CS_GPR1_STAMP_LO)?;
        push(batch_dwords, cursor, (RCS_CS_GPR_REL_BASE + 12) as u32)?;
        push(batch_dwords, cursor, CS_GPR_STAMP_HI)
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
            crate::intel::align_up(size_bytes, 4096).ok_or("compute-sba-size-align")?;
        let size_bytes = u32::try_from(size_bytes).map_err(|_| "compute-sba-size-convert")?;
        push(batch_dwords, cursor, (size_bytes & 0xFFFF_F000) | u32::from(enable))
    }

    batch_dwords.fill(0);
    let mut cursor = 0usize;

    const PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER: u32 = 1 << 9;
    const PIPE_CONTROL_UNTYPED_DATAPORT_FLUSH_HEADER: u32 = 1 << 11;
    const PIPE_CONTROL_GPGPU_SELECT_DW1: u32 =
        (1 << 0) | PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL;

    push_pipe_control_full(
        batch_dwords,
        &mut cursor,
        PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER | PIPE_CONTROL_UNTYPED_DATAPORT_FLUSH_HEADER,
        PIPE_CONTROL_GPGPU_SELECT_DW1,
    )?;
    push(batch_dwords, &mut cursor, PIPELINE_SELECT_GPGPU)?;
    push_store_marker(batch_dwords, &mut cursor, 23, 0xC0DE_7901)?;
    push_pipe_control_full(
        batch_dwords,
        &mut cursor,
        PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER | PIPE_CONTROL_UNTYPED_DATAPORT_FLUSH_HEADER,
        PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL,
    )?;

    push(batch_dwords, &mut cursor, STATE_BASE_ADDRESS_CMD)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, 0)?;
    push(batch_dwords, &mut cursor, RENDER_MOCS << 16)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, 0)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, 0)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, 0)?;
    push_sba_size(batch_dwords, &mut cursor, true, COMPUTE_SBA_SPAN_BYTES)?;
    push_sba_size(batch_dwords, &mut cursor, true, COMPUTE_SBA_SPAN_BYTES)?;
    push_sba_size(batch_dwords, &mut cursor, true, COMPUTE_SBA_SPAN_BYTES)?;
    push_sba_size(batch_dwords, &mut cursor, true, COMPUTE_SBA_SPAN_BYTES)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push_store_marker(batch_dwords, &mut cursor, 24, 0xC0DE_7902)?;
    push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_INVALIDATE_BITS)?;

    push(batch_dwords, &mut cursor, STATE_COMPUTE_MODE_CMD)?;
    push(batch_dwords, &mut cursor, 0xFFFF_0000)?;
    push_store_marker(batch_dwords, &mut cursor, 25, 0xC0DE_7903)?;
    push_pipe_control_full(
        batch_dwords,
        &mut cursor,
        PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER | PIPE_CONTROL_UNTYPED_DATAPORT_FLUSH_HEADER,
        PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL,
    )?;

    let cfe_start = cursor;
    push(batch_dwords, &mut cursor, CFE_STATE_CMD)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, (63 << 16) | (1 << 3))?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push_pipe_control_full(
        batch_dwords,
        &mut cursor,
        PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER,
        PIPE_CONTROL_CS_STALL,
    )?;
    push_cs_gpr_stamp(batch_dwords, &mut cursor)?;

    let walker_start = cursor;
    push(batch_dwords, &mut cursor, COMPUTE_WALKER_CMD)?;
    let body_start = cursor;
    for _ in 0..COMPUTE_WALKER_BODY_DWORDS {
        push(batch_dwords, &mut cursor, 0)?;
    }

    let kernel_gpu = GPU_VA_DRAW_STATE_BASE + GPGPU_EU_KERNEL_OFFSET_BYTES as u64;
    batch_dwords[body_start + 4] = 0xFFFF_FFFF;
    batch_dwords[body_start + 6] = 1;
    batch_dwords[body_start + 7] = 1;
    batch_dwords[body_start + 8] = 1;

    let idd = body_start + BODY_INTERFACE_DESCRIPTOR_DWORD;
    batch_dwords[idd] = (kernel_gpu as u32) & 0xFFFF_FFC0;
    batch_dwords[idd + 4] = if program.expects_store && store_surface.ready {
        ((store_surface.binding_table_offset as u32) & 0x001F_FFE0) | 31
    } else {
        0
    };
    batch_dwords[idd + 5] = 1 | (3 << 26);

    let post_sync = body_start + BODY_POSTSYNC_DWORD;
    batch_dwords[post_sync] = RENDER_MOCS << 4;

    push_store_marker(
        batch_dwords,
        &mut cursor,
        RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
    )?;

    push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_FLUSH_BITS)?;
    push(batch_dwords, &mut cursor, MI_BATCH_BUFFER_END)?;
    push(batch_dwords, &mut cursor, MI_NOOP)?;

    let command_bytes = cursor * core::mem::size_of::<u32>();
    crate::log!(
        "intel/gpgpu: compute-walker-layout program_source={} expects_store={} cfe_off=0x{:X} cfe_cmd=0x{:08X} cfe_dw3=0x{:08X} walker_off=0x{:X} walker_cmd=0x{:08X} body0=0x{:08X} exec_mask=0x{:08X} tg_dims={}x{}x{} idd0=0x{:08X} idd4=0x{:08X} idd5=0x{:08X} post_sync0=0x{:08X} surface_base=0x{:X} tail_off=0x{:X} cs_marker=0x{:08X} note=gen125-cfe-compute-walker-embedded-idd-no-post-cfe-mi-store\n",
        program.name,
        program.expects_store as u8,
        cfe_start * core::mem::size_of::<u32>(),
        batch_dwords[cfe_start],
        batch_dwords[cfe_start + 3],
        walker_start * core::mem::size_of::<u32>(),
        batch_dwords[walker_start],
        batch_dwords[body_start],
        batch_dwords[body_start + 4],
        batch_dwords[body_start + 6],
        batch_dwords[body_start + 7],
        batch_dwords[body_start + 8],
        batch_dwords[idd],
        batch_dwords[idd + 4],
        batch_dwords[idd + 5],
        batch_dwords[post_sync],
        GPU_VA_DRAW_STATE_BASE,
        command_bytes,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
    );
    crate::log!(
        "intel/gpgpu: compute-walker-store-contract program_source={} expects_store={} compact_send_desc_word=0x{:08X} compact_send_exdesc_word=0x{:08X} expected_bti=0x{:02X} binding_ready={} bt_off=0x{:X} bt_entry=0x{:08X} surf_off=0x{:X} surf_gpu=0x{:X} target_gpu=0x{:X} surf0=0x{:08X} note=compact-send-raw-words-not-direct-bti-decode\n",
        program.name,
        program.expects_store as u8,
        gpgpu_store_eot_program().words[GPGPU_C_STORE_KERNEL_SEND_DWORD - 1],
        gpgpu_store_eot_program().words[GPGPU_C_STORE_KERNEL_SEND_DWORD],
        store_surface.binding_table_index,
        store_surface.ready as u8,
        store_surface.binding_table_offset,
        store_surface.binding_entry,
        store_surface.surface_state_offset,
        store_surface.surface_gpu,
        store_surface.target_gpu,
        store_surface.surface_dword0,
    );
    crate::log!(
        "intel/gpgpu: compute-walker-dwords w0=0x{:08X} w1=0x{:08X} w2=0x{:08X} w3=0x{:08X} w4=0x{:08X} w5=0x{:08X} w6=0x{:08X} w7=0x{:08X} w8=0x{:08X} w9=0x{:08X} w10=0x{:08X} w11=0x{:08X} w12=0x{:08X} w13=0x{:08X} w14=0x{:08X} w15=0x{:08X} w16=0x{:08X} w17=0x{:08X} w18=0x{:08X} idd0=0x{:08X} idd1=0x{:08X} idd2=0x{:08X} idd3=0x{:08X} idd4=0x{:08X} idd5=0x{:08X} idd6=0x{:08X} idd7=0x{:08X}\n",
        batch_dwords[walker_start],
        batch_dwords[walker_start + 1],
        batch_dwords[walker_start + 2],
        batch_dwords[walker_start + 3],
        batch_dwords[walker_start + 4],
        batch_dwords[walker_start + 5],
        batch_dwords[walker_start + 6],
        batch_dwords[walker_start + 7],
        batch_dwords[walker_start + 8],
        batch_dwords[walker_start + 9],
        batch_dwords[walker_start + 10],
        batch_dwords[walker_start + 11],
        batch_dwords[walker_start + 12],
        batch_dwords[walker_start + 13],
        batch_dwords[walker_start + 14],
        batch_dwords[walker_start + 15],
        batch_dwords[walker_start + 16],
        batch_dwords[walker_start + 17],
        batch_dwords[walker_start + 18],
        batch_dwords[idd],
        batch_dwords[idd + 1],
        batch_dwords[idd + 2],
        batch_dwords[idd + 3],
        batch_dwords[idd + 4],
        batch_dwords[idd + 5],
        batch_dwords[idd + 6],
        batch_dwords[idd + 7],
    );

    debug_assert_eq!(cursor - walker_start, COMPUTE_WALKER_DWORDS + 4 + 6 + 2);
    Ok(command_bytes)
}

#[allow(dead_code)]
fn encode_gfx12_gpgpu_walker_probe_batch(
    warm: RenderWarmState,
    batch_dwords: &mut [u32],
    store_surface: GpgpuStoreSurfaceState,
    program: GpgpuEuProgram,
) -> Result<usize, &'static str> {
    const MEDIA_VFE_STATE_CMD: u32 = (3 << 29) | (2 << 27) | 7;
    const MEDIA_CURBE_LOAD_CMD: u32 = (3 << 29) | (2 << 27) | (1 << 16) | 2;
    const MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD: u32 = (3 << 29) | (2 << 27) | (2 << 16) | 2;
    const GPGPU_WALKER_CMD: u32 = (3 << 29) | (2 << 27) | (1 << 24) | (5 << 16) | 9;
    const MEDIA_STATE_FLUSH_CMD: u32 = (3 << 29) | (2 << 27) | (4 << 16);
    const PIPELINE_SELECT_BASE: u32 = (3 << 29) | (1 << 27) | (1 << 24) | (4 << 16);
    const PIPELINE_SELECT_GFX12_MASK: u32 = 0x13 << 8;
    const PIPELINE_SELECT_MEDIA_SAMPLER_DOP_CG_ENABLE: u32 = 1 << 4;
    const PIPELINE_SELECT_3D: u32 = PIPELINE_SELECT_BASE
        | PIPELINE_SELECT_GFX12_MASK
        | PIPELINE_SELECT_MEDIA_SAMPLER_DOP_CG_ENABLE;
    const PIPELINE_SELECT_GPGPU: u32 = PIPELINE_SELECT_3D | 2;
    const COMPUTE_SBA_SPAN_BYTES: usize = 0x1000_0000;
    const CS_GPR_STAMP_HI: u32 = 0x0000_0001;
    const CS_GPR0_STAMP_LO: u32 = 0xC5A0_2600;
    const CS_GPR1_STAMP_LO: u32 = 0xC5A0_2608;
    const IDD_STATE_OFFSET_BYTES: usize = GPGPU_WALKER_SCRATCH_OFFSET_BYTES;
    const CURBE_STATE_OFFSET_BYTES: usize = GPGPU_WALKER_SCRATCH_OFFSET_BYTES + 0x100;
    const IDD_PAYLOAD_DWORDS: usize = 8;
    const IDD_LOAD_DWORDS: usize = IDD_PAYLOAD_DWORDS;
    const CURBE_READ_LENGTH_8DW: u32 = 0;
    const GPGPU_THREADS_IN_GROUP: u32 = 1;
    const CURBE_TOTAL_BYTES: usize = if GPGPU_LOAD_DUMMY_CURBE {
        GPGPU_DUMMY_CURBE_BYTES
    } else {
        0
    };
    const VFE_CURBE_ALLOCATION_32B: u32 = if GPGPU_LOAD_DUMMY_CURBE {
        (GPGPU_DUMMY_CURBE_BYTES / 32) as u32
    } else {
        0
    };
    const GPGPU_VFE_MAX_THREADS: u32 = 64;
    const GPGPU_VFE_URB_ENTRIES: u32 = 2;
    const GPGPU_VFE_FUSED_EU_DISPATCH_LEGACY_MODE: u32 = 1 << 6;
    const GPGPU_VFE_URB_ENTRY_ALLOCATION_32B: u32 = 2;
    const GPGPU_DYNAMIC_STATE_BASE: u64 = 0;
    const IDD_DYNAMIC_OFFSET_BYTES: usize =
        GPU_VA_DRAW_STATE_BASE as usize + IDD_STATE_OFFSET_BYTES;
    const CURBE_DYNAMIC_OFFSET_BYTES: usize =
        GPU_VA_DRAW_STATE_BASE as usize + CURBE_STATE_OFFSET_BYTES;
    const GPGPU_KERNEL_GPU: u64 = GPU_VA_DRAW_STATE_BASE + GPGPU_EU_KERNEL_OFFSET_BYTES as u64;
    // Mesa's tiny Gfx12 executor programs Instruction Base to 0 and uses the
    // IDD Kernel Start Pointer as the kernel's GPU offset.  Keep this probe in
    // that shape so the EU fetch address no longer depends on a non-zero
    // instruction-base latch.
    const GPGPU_INSTRUCTION_BASE: u64 = 0;
    const GPGPU_KSP_NEGATIVE_CONTROL: bool = false;
    const GPGPU_BAD_KERNEL_START_POINTER: u64 = 0x00F0_0000;
    // Gen12 legacy GPGPU_WALKER is an 11-dword packet. Keep the right mask to
    // the live SIMD8 lanes so the one-thread EOT probe does not depend on
    // undefined high mask bits.
    const GPGPU_WALKER_SIMD8_RIGHT_MASK: u32 = 0x0000_00FF;
    const GPGPU_WALKER_BOTTOM_MASK: u32 = 0xFFFF_FFFF;
    const STATE_SIP_CMD: u32 = 0x6102_0001;
    const GPGPU_ENABLE_SIP_EXCEPTIONS: bool = false;
    // The repeatable stall sits at GPGPU_WALKER with no visible TDL dispatch.
    // Restore the older active path's non-preemptible root thread policy while
    // keeping exception/SIP routing as a separate diagnostic knob.
    const IDD_THREAD_PREEMPTION_DISABLE: u32 = 1 << 20;
    const IDD_ILLEGAL_OPCODE_EXCEPTION_ENABLE: u32 = 1 << 13;
    const IDD_SOFTWARE_EXCEPTION_ENABLE: u32 = 1 << 7;

    fn push(batch_dwords: &mut [u32], cursor: &mut usize, value: u32) -> Result<(), &'static str> {
        if *cursor >= batch_dwords.len() {
            return Err("compute-walker-batch-exhausted");
        }
        batch_dwords[*cursor] = value;
        *cursor += 1;
        Ok(())
    }

    fn push_pipe_control_full(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        header_flags: u32,
        dw1_flags: u32,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, PIPE_CONTROL_CMD | header_flags)?;
        push(batch_dwords, cursor, dw1_flags)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)
    }

    fn push_pipe_control(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        flags: u32,
    ) -> Result<(), &'static str> {
        push_pipe_control_full(batch_dwords, cursor, 0, flags)
    }

    fn push_store_marker(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        slot: usize,
        value: u32,
    ) -> Result<(), &'static str> {
        let dst = GPU_VA_RESULT_BASE + (slot as u64) * core::mem::size_of::<u32>() as u64;
        push(batch_dwords, cursor, MI_STORE_DATA_IMM_GGTT_DW1)?;
        push(batch_dwords, cursor, dst as u32)?;
        push(batch_dwords, cursor, (dst >> 32) as u32)?;
        push(batch_dwords, cursor, value)
    }

    fn push_cs_gpr_stamp(batch_dwords: &mut [u32], cursor: &mut usize) -> Result<(), &'static str> {
        push(batch_dwords, cursor, mi_lri_cmd(4, MI_LRI_FORCE_POSTED))?;
        push(batch_dwords, cursor, RCS_CS_GPR_REL_BASE as u32)?;
        push(batch_dwords, cursor, CS_GPR0_STAMP_LO)?;
        push(batch_dwords, cursor, (RCS_CS_GPR_REL_BASE + 4) as u32)?;
        push(batch_dwords, cursor, CS_GPR_STAMP_HI)?;
        push(batch_dwords, cursor, (RCS_CS_GPR_REL_BASE + 8) as u32)?;
        push(batch_dwords, cursor, CS_GPR1_STAMP_LO)?;
        push(batch_dwords, cursor, (RCS_CS_GPR_REL_BASE + 12) as u32)?;
        push(batch_dwords, cursor, CS_GPR_STAMP_HI)
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
            crate::intel::align_up(size_bytes, 4096).ok_or("compute-sba-size-align")?;
        let size_bytes = u32::try_from(size_bytes).map_err(|_| "compute-sba-size-convert")?;
        push(batch_dwords, cursor, (size_bytes & 0xFFFF_F000) | u32::from(enable))
    }

    batch_dwords.fill(0);
    if GPGPU_LOAD_DUMMY_CURBE {
        let curbe_index = CURBE_STATE_OFFSET_BYTES / core::mem::size_of::<u32>();
        let curbe_dwords = GPGPU_DUMMY_CURBE_BYTES / core::mem::size_of::<u32>();
        if curbe_index
            .checked_add(curbe_dwords)
            .is_none_or(|end| end * core::mem::size_of::<u32>() > warm.draw_state_len)
        {
            return Err("gpgpu-curbe-scratch-exhausted");
        }
        unsafe {
            let curbe = warm.draw_state_virt.add(CURBE_STATE_OFFSET_BYTES) as *mut u32;
            for index in 0..curbe_dwords {
                core::ptr::write_volatile(curbe.add(index), 0x5A5A_5A5A);
            }
        }
        crate::intel::dma_flush(
            unsafe { warm.draw_state_virt.add(CURBE_STATE_OFFSET_BYTES) },
            GPGPU_DUMMY_CURBE_BYTES,
        );
    }
    let mut cursor = 0usize;

    const PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER: u32 = 1 << 9;
    const PIPE_CONTROL_UNTYPED_DATAPORT_FLUSH_HEADER: u32 = 1 << 11;
    const PIPE_CONTROL_GPGPU_SELECT_DW1: u32 =
        (1 << 0) | PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL;

    push_pipe_control_full(
        batch_dwords,
        &mut cursor,
        PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER | PIPE_CONTROL_UNTYPED_DATAPORT_FLUSH_HEADER,
        PIPE_CONTROL_GPGPU_SELECT_DW1,
    )?;
    push(batch_dwords, &mut cursor, PIPELINE_SELECT_GPGPU)?;
    push_store_marker(batch_dwords, &mut cursor, 23, 0xC0DE_7801)?;
    push_pipe_control_full(
        batch_dwords,
        &mut cursor,
        PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER,
        PIPE_CONTROL_CS_STALL,
    )?;

    // Wa_1607854226/TGL: non-pipelined state may not latch when emitted under
    // GPGPU pipeline select.  Program SBA/SCM while temporarily in 3D, then
    // switch back before MEDIA_VFE_STATE and GPGPU_WALKER.
    push(batch_dwords, &mut cursor, PIPELINE_SELECT_3D)?;
    push_pipe_control_full(
        batch_dwords,
        &mut cursor,
        PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER | PIPE_CONTROL_UNTYPED_DATAPORT_FLUSH_HEADER,
        PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL,
    )?;
    let idd_index = IDD_STATE_OFFSET_BYTES / core::mem::size_of::<u32>();
    if idd_index
        .checked_add(IDD_LOAD_DWORDS)
        .is_none_or(|end| end * core::mem::size_of::<u32>() > warm.draw_state_len)
    {
        return Err("gpgpu-idd-state-exhausted");
    }
    // KSP negative-control probe: when enabled, point well past the uploaded EU
    // artifact. If dispatch/stall/debug output remains identical, our current
    // public signals are not proving EU instruction fetch from this artifact.
    let kernel_start_pointer = if GPGPU_KSP_NEGATIVE_CONTROL {
        GPGPU_BAD_KERNEL_START_POINTER
    } else {
        GPGPU_KERNEL_GPU
    };
    let mut idd_words = [0u32; IDD_PAYLOAD_DWORDS];
    idd_words[0] = kernel_start_pointer as u32;
    idd_words[1] = (kernel_start_pointer >> 32) as u32;
    // Minimal EOT probe policy: keep tiny EU probes non-preemptible.  The
    // SIP/exception path is a separate diagnostic knob because pointing SIP at
    // the EOT artifact did not explain the missing return.
    idd_words[2] = IDD_THREAD_PREEMPTION_DISABLE
        | if GPGPU_ENABLE_SIP_EXCEPTIONS {
            IDD_ILLEGAL_OPCODE_EXCEPTION_ENABLE | IDD_SOFTWARE_EXCEPTION_ENABLE
        } else {
            0
        };
    idd_words[3] = 0;
    idd_words[4] = if program.expects_store && store_surface.ready {
        (store_surface.binding_table_offset as u32) | 31
    } else {
        0
    };
    idd_words[5] = CURBE_READ_LENGTH_8DW << 16;
    idd_words[6] = GPGPU_THREADS_IN_GROUP;
    idd_words[7] = 0;
    unsafe {
        let idd_dst = warm.draw_state_virt.add(IDD_STATE_OFFSET_BYTES) as *mut u32;
        for (index, word) in idd_words.iter().enumerate() {
            core::ptr::write_volatile(idd_dst.add(index), *word);
        }
    }
    crate::intel::dma_flush(
        unsafe { warm.draw_state_virt.add(IDD_STATE_OFFSET_BYTES) },
        IDD_LOAD_DWORDS * core::mem::size_of::<u32>(),
    );
    crate::log!(
        "intel/gpgpu: idd-debug-policy program_source={} idd_dw2=0x{:08X} software_exception_enable={} illegal_opcode_exception_enable={} mask_stack_exception_enable={} sip_programmed={} sip_offset=0x00000000 ksp_negative_control={} note=prm-idd-dw2-loads-eu-cr0-exception-enable-bits\n",
        program.name,
        idd_words[2],
        (idd_words[2] >> 7) & 1,
        (idd_words[2] >> 13) & 1,
        (idd_words[2] >> 11) & 1,
        GPGPU_ENABLE_SIP_EXCEPTIONS as u8,
        GPGPU_KSP_NEGATIVE_CONTROL as u8,
    );
    crate::log!(
        "intel/gpgpu: eu-ksp-placement-proof program_source={} instruction_base=0x{:X} ksp=0x{:X} ksp_resolves_to=0x{:X} uploaded_gpu=0x{:X} ksp_unit=byte-offset-low6-mbz ksp_64b_aligned={} instruction_base_4k_aligned={} artifact_bytes=0x{:X} crosses_64b_boundary={} placement_shape=mesa-base0-ksp-absolute-offset expected_delta=\"if fetch base was the bug, illegal/eot signature changes without EU byte changes\"\n",
        program.name,
        GPGPU_INSTRUCTION_BASE,
        kernel_start_pointer,
        GPGPU_INSTRUCTION_BASE + kernel_start_pointer,
        GPGPU_KERNEL_GPU,
        (kernel_start_pointer & 0x3F == 0) as u8,
        (GPGPU_INSTRUCTION_BASE & 0xFFF == 0) as u8,
        program.words.len() * core::mem::size_of::<u32>(),
        (((kernel_start_pointer & 0x3F)
            + (program.words.len() * core::mem::size_of::<u32>()) as u64)
            > 0x40) as u8,
    );

    push(batch_dwords, &mut cursor, STATE_BASE_ADDRESS_CMD)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, 0)?;
    push(batch_dwords, &mut cursor, RENDER_MOCS << 16)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPGPU_DYNAMIC_STATE_BASE)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, 0)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPGPU_INSTRUCTION_BASE)?;
    push_sba_size(batch_dwords, &mut cursor, true, COMPUTE_SBA_SPAN_BYTES)?;
    push_sba_size(batch_dwords, &mut cursor, true, COMPUTE_SBA_SPAN_BYTES)?;
    push_sba_size(batch_dwords, &mut cursor, true, COMPUTE_SBA_SPAN_BYTES)?;
    push_sba_size(batch_dwords, &mut cursor, true, COMPUTE_SBA_SPAN_BYTES)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    if GPGPU_ENABLE_SIP_EXCEPTIONS {
        push(batch_dwords, &mut cursor, STATE_SIP_CMD)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
        crate::log!(
            "intel/gpgpu: state-sip-policy program_source={} cmd=0x{:08X} instruction_base=0x{:X} sip_offset=0x00000000 sip_resolves_to=0x{:X} exception_target=same-eot-artifact note=diagnostic-only\n",
            program.name,
            STATE_SIP_CMD,
            GPGPU_INSTRUCTION_BASE,
            GPGPU_INSTRUCTION_BASE,
        );
    } else {
        crate::log!(
            "intel/gpgpu: state-sip-policy program_source={} cmd=0x{:08X} instruction_base=0x{:X} sip_offset=0x00000000 sip_resolves_to=0x00000000 exception_target=disabled note=minimal-eot-probe\n",
            program.name,
            STATE_SIP_CMD,
            GPGPU_INSTRUCTION_BASE,
        );
    }
    push_store_marker(batch_dwords, &mut cursor, 24, 0xC0DE_7802)?;
    push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_INVALIDATE_BITS)?;
    push_store_marker(batch_dwords, &mut cursor, 25, 0xC0DE_7803)?;
    push_pipe_control_full(
        batch_dwords,
        &mut cursor,
        PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER | PIPE_CONTROL_UNTYPED_DATAPORT_FLUSH_HEADER,
        PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL,
    )?;
    push(batch_dwords, &mut cursor, PIPELINE_SELECT_GPGPU)?;
    push_pipe_control_full(
        batch_dwords,
        &mut cursor,
        PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER,
        PIPE_CONTROL_CS_STALL,
    )?;
    push_cs_gpr_stamp(batch_dwords, &mut cursor)?;
    let vfe_start = cursor;
    push(batch_dwords, &mut cursor, MEDIA_VFE_STATE_CMD)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(
        batch_dwords,
        &mut cursor,
        (GPGPU_VFE_MAX_THREADS << 16)
            | (GPGPU_VFE_URB_ENTRIES << 8)
            | GPGPU_VFE_FUSED_EU_DISPATCH_LEGACY_MODE,
    )?;
    push(batch_dwords, &mut cursor, 0)?;
    push(
        batch_dwords,
        &mut cursor,
        (GPGPU_VFE_URB_ENTRY_ALLOCATION_32B << 16) | VFE_CURBE_ALLOCATION_32B,
    )?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    if !GPGPU_CONTIGUOUS_VFE_IDD_WALKER {
        push_pipe_control_full(
            batch_dwords,
            &mut cursor,
            PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER,
            PIPE_CONTROL_CS_STALL,
        )?;
        push_store_marker(batch_dwords, &mut cursor, 26, 0xC0DE_7804)?;
        push(batch_dwords, &mut cursor, MEDIA_STATE_FLUSH_CMD)?;
        push(batch_dwords, &mut cursor, 0)?;
    }
    if GPGPU_LOAD_DUMMY_CURBE {
        push(batch_dwords, &mut cursor, MEDIA_CURBE_LOAD_CMD)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, CURBE_TOTAL_BYTES as u32)?;
        push(batch_dwords, &mut cursor, CURBE_DYNAMIC_OFFSET_BYTES as u32)?;
    }
    if GPGPU_MESA_POST_VFE_PIPE_CONTROL {
        push_pipe_control_full(
            batch_dwords,
            &mut cursor,
            PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER,
            PIPE_CONTROL_FLUSH_ENABLE | PIPE_CONTROL_CS_STALL,
        )?;
    }
    if !GPGPU_CONTIGUOUS_VFE_IDD_WALKER {
        push_store_marker(batch_dwords, &mut cursor, 28, 0xC0DE_7806)?;
    }
    // This exact-Mesa-shell pass keeps CURBE disabled and loads only the 32B
    // IDD structure; earlier CURBE/64B-IDL probes did not move the frontier.
    if !GPGPU_CONTIGUOUS_VFE_IDD_WALKER {
        push_store_marker(batch_dwords, &mut cursor, 27, 0xC0DE_7805)?;
    }
    let id_load_start = cursor;
    push(batch_dwords, &mut cursor, MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, (IDD_LOAD_DWORDS * core::mem::size_of::<u32>()) as u32)?;
    push(batch_dwords, &mut cursor, IDD_DYNAMIC_OFFSET_BYTES as u32)?;
    let walker_start = cursor;
    push(batch_dwords, &mut cursor, GPGPU_WALKER_CMD)?;
    push(batch_dwords, &mut cursor, 0)?; // Interface Descriptor Offset
    push(batch_dwords, &mut cursor, 0)?; // thread counters max = 0, SIMD8
    push(batch_dwords, &mut cursor, 0)?; // Thread Group ID Starting X
    push(batch_dwords, &mut cursor, 1)?; // Thread Group ID X Dimension
    push(batch_dwords, &mut cursor, 0)?; // Thread Group ID Starting Y
    push(batch_dwords, &mut cursor, 1)?; // Thread Group ID Y Dimension
    push(batch_dwords, &mut cursor, 0)?; // Thread Group ID Starting Z
    push(batch_dwords, &mut cursor, 1)?; // Thread Group ID Z Dimension
    push(batch_dwords, &mut cursor, GPGPU_WALKER_SIMD8_RIGHT_MASK)?;
    push(batch_dwords, &mut cursor, GPGPU_WALKER_BOTTOM_MASK)?;
    push(batch_dwords, &mut cursor, MEDIA_STATE_FLUSH_CMD)?;
    push(batch_dwords, &mut cursor, 0)?;
    push_store_marker(
        batch_dwords,
        &mut cursor,
        RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
    )?;

    push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_FLUSH_BITS)?;
    push(batch_dwords, &mut cursor, MI_BATCH_BUFFER_END)?;
    push(batch_dwords, &mut cursor, MI_NOOP)?;
    let command_bytes = cursor * core::mem::size_of::<u32>();
    let batch_bytes = command_bytes;
    let walker_dw2 = batch_dwords[walker_start + 2];
    let walker_x_dim = batch_dwords[walker_start + 4];
    let walker_y_dim = batch_dwords[walker_start + 6];
    let walker_z_dim = batch_dwords[walker_start + 8];
    let thread_width = (walker_dw2 & 0x3F) + 1;
    let thread_height = ((walker_dw2 >> 8) & 0x3F) + 1;
    let thread_depth = ((walker_dw2 >> 16) & 0x3F) + 1;
    let walker_group_threads = thread_width * thread_height * thread_depth;
    let idd_dw6 = idd_words[6];
    let idd_barrier_enable = (idd_dw6 >> 21) & 1;
    let idd_slm_size = (idd_dw6 >> 16) & 0x1F;
    let idd_threads_in_group = idd_dw6 & 0x3FF;
    let simd_mask_bits = match (walker_dw2 >> 30) & 0x3 {
        0 => 8,
        1 => 16,
        2 => 32,
        _ => 0,
    };
    let simd_mask = if simd_mask_bits == 32 {
        u32::MAX
    } else {
        (1u32 << simd_mask_bits) - 1
    };
    let right_lanes_consumed = (batch_dwords[walker_start + 9] & simd_mask).count_ones();
    let bottom_lanes_consumed = (batch_dwords[walker_start + 10] & simd_mask).count_ones();

    crate::log!(
        "intel/gpgpu: compute-walker-layout program_source={} expects_store={} vfe_off=0x{:X} vfe_dw3=0x{:08X} vfe_dw5=0x{:08X} fused_eu_dispatch_legacy={} urb_entry_alloc_32b={} curbe_present={} curbe_bytes=0x{:X} curbe_read_len_8dw={} id_load_off=0x{:X} id_load_bytes=0x{:X} idd_payload_bytes=0x{:X} walker_off=0x{:X} walker_cmd=0x{:08X} exec_mask=0x{:08X} idd_gpu=0x{:X} idd_dynamic_offset=0x{:X} idd_ksp=0x{:08X} instruction_base=0x{:X} ksp_resolves_to=0x{:X} idd_dw2=0x{:08X} idd_dw4=0x{:08X} idd_dw6=0x{:08X} surface_base=0x{:X} dynamic_state_base=0x{:X} contiguous_vfe_idd_walker={} tail_off=0x{:X} cs_marker=0x{:08X} note=legacy-vfe-dispatch-with-len9-walker\n",
        program.name,
        program.expects_store as u8,
        vfe_start * core::mem::size_of::<u32>(),
        batch_dwords[vfe_start + 3],
        batch_dwords[vfe_start + 5],
        ((batch_dwords[vfe_start + 3] & GPGPU_VFE_FUSED_EU_DISPATCH_LEGACY_MODE) != 0) as u8,
        GPGPU_VFE_URB_ENTRY_ALLOCATION_32B,
        GPGPU_LOAD_DUMMY_CURBE as u8,
        CURBE_TOTAL_BYTES,
        CURBE_READ_LENGTH_8DW,
        id_load_start * core::mem::size_of::<u32>(),
        IDD_LOAD_DWORDS * core::mem::size_of::<u32>(),
        IDD_PAYLOAD_DWORDS * core::mem::size_of::<u32>(),
        walker_start * core::mem::size_of::<u32>(),
        batch_dwords[walker_start],
        batch_dwords[walker_start + 9],
        GPGPU_DYNAMIC_STATE_BASE + IDD_DYNAMIC_OFFSET_BYTES as u64,
        IDD_DYNAMIC_OFFSET_BYTES,
        idd_words[0],
        GPGPU_INSTRUCTION_BASE,
        GPGPU_INSTRUCTION_BASE + kernel_start_pointer,
        idd_words[2],
        idd_words[4],
        idd_words[6],
        GPU_VA_DRAW_STATE_BASE,
        GPGPU_DYNAMIC_STATE_BASE,
        GPGPU_CONTIGUOUS_VFE_IDD_WALKER as u8,
        command_bytes,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
    );
    crate::log!(
        "intel/gpgpu: compute-walker-resource-contract program_source={} walker_dw2=0x{:08X} simd_size={} simd_mask_bits={} thread_width={} thread_height={} thread_depth={} walker_group_threads={} idd_threads_in_group={} group_count_matches_idd={} idd_barrier_enable={} idd_slm_size={} x_dim={} y_dim={} z_dim={} right_mask=0x{:08X} bottom_mask=0x{:08X} right_lanes_consumed={} bottom_lanes_consumed={} raw_right_mask_bits={} raw_bottom_mask_bits={} expected_hw_threads=1 probe=len9-one-group-legacy-vfe expected_good=post-walker-marker-or-eot-retired failure_disproves=legacy-walker-dword-layout note=one-group-simd8-mask\n",
        program.name,
        walker_dw2,
        (walker_dw2 >> 30) & 0x3,
        simd_mask_bits,
        thread_width,
        thread_height,
        thread_depth,
        walker_group_threads,
        idd_threads_in_group,
        (walker_group_threads == idd_threads_in_group) as u8,
        idd_barrier_enable,
        idd_slm_size,
        walker_x_dim,
        walker_y_dim,
        walker_z_dim,
        batch_dwords[walker_start + 9],
        batch_dwords[walker_start + 10],
        right_lanes_consumed,
        bottom_lanes_consumed,
        batch_dwords[walker_start + 9].count_ones(),
        batch_dwords[walker_start + 10].count_ones(),
    );
    crate::log!(
        "intel/gpgpu: mesa-shaped-shell program_source={} pure_eot={} eu_bytes=0x{:X} idd_dw2=0x{:08X} idd_dw3=0x{:08X} idd_dw4=0x{:08X} idd_dw5=0x{:08X} idd_dw6=0x{:08X} idd_dw7=0x{:08X} vfe_dw1=0x{:08X} vfe_dw3=0x{:08X} vfe_dw5=0x{:08X} curbe_present={} curbe_bytes=0x{:X} sampler_state_pointer=0x{:X} sampler_count=0 binding_table_pointer=0x{:X} binding_table_count={} scratch_disabled=1 slm_size={} barrier_enable={} walker_simd_size={} walker_threads={} right_mask=0x{:08X} bottom_mask=0x{:08X} contiguous_vfe_idd_walker={} expected_good=\"post_walker_marker-or-eot_retired\" failure_disproves=\"post-vfe-command-gap-before-midl\"\n",
        program.name,
        (!program.expects_store) as u8,
        program.words.len() * core::mem::size_of::<u32>(),
        idd_words[2],
        idd_words[3],
        idd_words[4],
        idd_words[5],
        idd_words[6],
        idd_words[7],
        batch_dwords[vfe_start + 1],
        batch_dwords[vfe_start + 3],
        batch_dwords[vfe_start + 5],
        GPGPU_LOAD_DUMMY_CURBE as u8,
        CURBE_TOTAL_BYTES,
        idd_words[3] & 0xFFFF_FFE0,
        idd_words[4] & 0x0000_FFE0,
        idd_words[4] & 0x1F,
        idd_slm_size,
        idd_barrier_enable,
        (walker_dw2 >> 30) & 0x3,
        walker_group_threads,
        batch_dwords[walker_start + 9],
        batch_dwords[walker_start + 10],
        GPGPU_CONTIGUOUS_VFE_IDD_WALKER as u8,
    );
    crate::log!(
        "intel/gpgpu: compute-walker-store-contract program_source={} expects_store={} compact_send_desc_word=0x{:08X} compact_send_exdesc_word=0x{:08X} expected_bti=0x{:02X} binding_ready={} bt_off=0x{:X} bt_entry=0x{:08X} surf_off=0x{:X} surf_gpu=0x{:X} target_gpu=0x{:X} surf0=0x{:08X} note=compact-send-raw-words-not-direct-bti-decode\n",
        program.name,
        program.expects_store as u8,
        gpgpu_store_eot_program().words[GPGPU_C_STORE_KERNEL_SEND_DWORD - 1],
        gpgpu_store_eot_program().words[GPGPU_C_STORE_KERNEL_SEND_DWORD],
        store_surface.binding_table_index,
        store_surface.ready as u8,
        store_surface.binding_table_offset,
        store_surface.binding_entry,
        store_surface.surface_state_offset,
        store_surface.surface_gpu,
        store_surface.target_gpu,
        store_surface.surface_dword0,
    );
    crate::log!(
        "intel/gpgpu: compute-walker-dwords w0=0x{:08X} w1=0x{:08X} w2=0x{:08X} w3=0x{:08X} w4=0x{:08X} w5=0x{:08X} w6=0x{:08X} w7=0x{:08X} w8=0x{:08X} w9=0x{:08X} w10=0x{:08X} idd0=0x{:08X} idd1=0x{:08X} idd2=0x{:08X} idd3=0x{:08X} idd4=0x{:08X} idd5=0x{:08X} idd6=0x{:08X} idd7=0x{:08X} midl0=0x{:08X} midl2=0x{:08X} midl3=0x{:08X}\n",
        batch_dwords[walker_start],
        batch_dwords[walker_start + 1],
        batch_dwords[walker_start + 2],
        batch_dwords[walker_start + 3],
        batch_dwords[walker_start + 4],
        batch_dwords[walker_start + 5],
        batch_dwords[walker_start + 6],
        batch_dwords[walker_start + 7],
        batch_dwords[walker_start + 8],
        batch_dwords[walker_start + 9],
        batch_dwords[walker_start + 10],
        idd_words[0],
        idd_words[1],
        idd_words[2],
        idd_words[3],
        idd_words[4],
        idd_words[5],
        idd_words[6],
        idd_words[7],
        batch_dwords[id_load_start],
        batch_dwords[id_load_start + 2],
        batch_dwords[id_load_start + 3],
    );

    Ok(batch_bytes)
}

fn log_gpgpu_compute_walker_status(proof: GpgpuComputeWalkerProof) {
    let gpu_program_started = proof.dispatch_delta != 0;
    let eot_only_retired = !proof.expects_store && gpu_program_started && proof.retired;
    let store_landed_anywhere = proof.expected_hits_mask != 0;
    let breadcrumbs_ok = if GPGPU_CONTIGUOUS_VFE_IDD_WALKER {
        proof.post_pipeline == 0xC0DE_7801
            && proof.post_sba == 0xC0DE_7802
            && proof.post_scm == 0xC0DE_7803
    } else {
        proof.post_pipeline == 0xC0DE_7801
            && proof.post_sba == 0xC0DE_7802
            && proof.post_scm == 0xC0DE_7803
            && proof.post_cfe == 0xC0DE_7804
            && proof.post_pre_midl_msf == 0xC0DE_7805
            && proof.post_curbe_load == 0xC0DE_7806
    };
    let post_walker_marker_retired = proof.marker == RCS_EXEC_RESULT_COMPUTE_WALKER_DONE;
    let failure_class = if eot_only_retired {
        "thread-eot-retired-proven"
    } else if proof.result_c_changed_by_eu {
        "shared-ram-write-proven"
    } else if gpu_program_started && !proof.retired && !store_landed_anywhere {
        "eu-thread-started-no-eot-no-store-hit"
    } else if gpu_program_started && !proof.retired && proof.c_value == 0 {
        "program-started-did-not-finish-or-store"
    } else if gpu_program_started && proof.retired && proof.c_value == 0 {
        "program-started-no-shared-ram-write"
    } else if !gpu_program_started {
        "program-not-started"
    } else {
        "unexpected-shared-ram-value"
    };
    let next = if proof.result_c_changed_by_eu {
        "add-read-alu-write-kernel"
    } else if eot_only_retired {
        "add-dataport-store-after-clean-eot"
    } else if gpu_program_started && !proof.retired {
        "fix-ts-eot-or-enable-sip-exception-capture-before-dataport-store"
    } else {
        "fix-walker-thread-start"
    };
    crate::log!(
        "intel/gpgpu: eu-frontier program_source={} command_breadcrumbs_ok={} post_walker_marker={} thread_dispatch_delta={} store_expected=0x{:08X} store_target_slot={} store_target_value=0x{:08X} store_hits_mask_lo64=0x{:016X} eot_retired={} frontier={} next={}\n",
        proof.program_name,
        breadcrumbs_ok as u8,
        post_walker_marker_retired as u8,
        proof.dispatch_delta,
        GPU_PROGRAM_SHARED_RAM_WRITE_EXPECTED,
        RESULT_SLOT_GPGPU_EU_C_STORE_DWORD,
        proof.c_value,
        proof.expected_hits_mask,
        eot_only_retired as u8,
        failure_class,
        next,
    );
    let started_plain = if gpu_program_started {
        "gpu accepted the walker and TS counted launched EU threads; command stream is waiting for those workers to say done before the post-walker marker can execute"
    } else {
        "gpu accepted the walker and command stream is parked there; TS/TDL counters did not show launched EU threads"
    };
    crate::log!(
        "intel/gpgpu: started-thread-snapshot started={} command_stream_reached_walker={} threads_started={} worker_done_signal_seen={} command_after_worker_ran={} store_seen={} plain=\"{}\"\n",
        gpu_program_started as u8,
        breadcrumbs_ok as u8,
        proof.dispatch_delta,
        eot_only_retired as u8,
        post_walker_marker_retired as u8,
        store_landed_anywhere as u8,
        started_plain,
    );
    crate::log!(
        "intel/gpgpu: gpu-program-proof program_source={} expects_store={} start_submitted={} finished={} finish_marker=0x{:08X} finish_expected=0x{:08X} starts_before={} starts_after={} starts_delta={} start_command_bytes=0x{:X} gpu_program_started={} shared_ram_slot={} shared_ram_value=0x{:08X} shared_ram_expected=0x{:08X} wrote_shared_ram={} store_landed_anywhere={} eot_retired={} command_breadcrumbs_ok={} post_walker_marker={} failure_class={} cpu_reads_c_back=1 backend=gfx12-gpgpu-start-command next={} does_not_prove=matmul\n",
        proof.program_name,
        proof.expects_store as u8,
        proof.submitted as u8,
        proof.retired as u8,
        proof.marker,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        proof.dispatch_before,
        proof.dispatch_after,
        proof.dispatch_delta,
        proof.batch_bytes,
        gpu_program_started as u8,
        RESULT_SLOT_GPGPU_EU_C_STORE_DWORD,
        proof.c_value,
        GPU_PROGRAM_SHARED_RAM_WRITE_EXPECTED,
        proof.result_c_changed_by_eu as u8,
        store_landed_anywhere as u8,
        eot_only_retired as u8,
        breadcrumbs_ok as u8,
        post_walker_marker_retired as u8,
        failure_class,
        next,
    );
}

fn encode_gpgpu_preflight_batch(
    batch_dwords: &mut [u32],
    dot: u32,
    sum_a: u32,
    sum_b: u32,
) -> Result<usize, &'static str> {
    const STORES: [(usize, fn(u32, u32, u32) -> u32); 5] = [
        (RESULT_SLOT_GPGPU_PREFLIGHT_MARKER_DWORD, |_, _, _| RCS_EXEC_RESULT_GPGPU_PREFLIGHT_DONE),
        (RESULT_SLOT_GPGPU_PREFLIGHT_DOT_DWORD, |dot, _, _| dot),
        (RESULT_SLOT_GPGPU_PREFLIGHT_SUM_A_DWORD, |_, sum_a, _| sum_a),
        (RESULT_SLOT_GPGPU_PREFLIGHT_SUM_B_DWORD, |_, _, sum_b| sum_b),
        (RESULT_SLOT_GPGPU_PREFLIGHT_LANES_DWORD, |_, _, _| GPGPU_PREFLIGHT_LANES as u32),
    ];
    const STORE_DWORDS: usize = 4;
    const END_DWORDS: usize = 2;

    if batch_dwords.len() < STORES.len() * STORE_DWORDS + END_DWORDS {
        return Err("batch-too-small");
    }

    let mut idx = 0;
    for (slot, value_fn) in STORES {
        let dst = GPU_VA_RESULT_BASE + (slot as u64) * core::mem::size_of::<u32>() as u64;
        batch_dwords[idx] = MI_STORE_DATA_IMM_GGTT_DW1;
        batch_dwords[idx + 1] = dst as u32;
        batch_dwords[idx + 2] = (dst >> 32) as u32;
        batch_dwords[idx + 3] = value_fn(dot, sum_a, sum_b);
        idx += STORE_DWORDS;
    }
    batch_dwords[idx] = MI_BATCH_BUFFER_END;
    batch_dwords[idx + 1] = MI_NOOP;
    idx += END_DWORDS;

    Ok(idx * core::mem::size_of::<u32>())
}
