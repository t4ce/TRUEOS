use core::{
    ptr,
    sync::atomic::{AtomicBool, Ordering},
};

use spin::Mutex;

use super::intel_guc;
use super::intel_770_registers;
use super::IntelDeviceInfo;

const INTEL_IGPU770_DEVICE_ID: u16 = 0x4680;
const WARM_RING_BYTES: usize = 4096;
const WARM_CONTEXT_BYTES: usize = 8192;
const WARM_BATCH_BYTES: usize = 4096;
const WARM_RESULT_BYTES: usize = 4096;
const WARM_ALIGN: usize = 4096;
const GGTT_ALIAS_BASE_OFF: usize = 0x0080_0000;
const GGTT_ALIAS_BYTES: usize = 0x0080_0000;
const GGTT_PTE_BYTES: usize = 8;
const GGTT_PAGE_BYTES: u64 = 4096;
const GEN8_PAGE_PRESENT: u64 = 1;
const SMOKE_RECT_W: usize = 64;
const SMOKE_RECT_H: usize = 64;
const SMOKE_COLOR_XRGB8888: u32 = 0x00FF_4A24;
const GPU_VA_RING_BASE: u64 = 0x0080_0000;
const GPU_VA_CONTEXT_BASE: u64 = 0x0081_0000;
const GPU_VA_BATCH_BASE: u64 = 0x0082_0000;
const GPU_VA_RESULT_BASE: u64 = 0x0083_0000;
const RCS_RING_BASE: usize = 0x0000_2000;
const RCS_RING_TAIL: usize = RCS_RING_BASE + 0x30;
const RCS_RING_HEAD: usize = RCS_RING_BASE + 0x34;
const RCS_RING_START: usize = RCS_RING_BASE + 0x38;
const RCS_RING_CTL: usize = RCS_RING_BASE + 0x3C;
const RCS_RING_ACTHD: usize = RCS_RING_BASE + 0x74;
const RCS_RING_HWS_PGA: usize = RCS_RING_BASE + 0x80;
const RCS_RING_HWSTAM: usize = RCS_RING_BASE + 0x98;
const RCS_RING_MI_MODE: usize = RCS_RING_BASE + 0x9C;
const RCS_RING_IMR: usize = RCS_RING_BASE + 0xA8;
const RCS_RING_EIR: usize = RCS_RING_BASE + 0xB0;
const RCS_RING_EMR: usize = RCS_RING_BASE + 0xB4;
const RCS_RING_IPEIR: usize = RCS_RING_BASE + 0x64;
const RCS_RING_IPEHR: usize = RCS_RING_BASE + 0x68;
const RCS_RING_INSTDONE: usize = RCS_RING_BASE + 0x6C;
const RCS_RING_INSTPS: usize = RCS_RING_BASE + 0x70;
const RCS_RING_BBADDR: usize = RCS_RING_BASE + 0x140;
const RCS_RING_BBADDR_UDW: usize = RCS_RING_BASE + 0x168;
const RCS_RING_CONTEXT_CONTROL: usize = RCS_RING_BASE + 0x244;
const RCS_RING_CONTEXT_CONTROL_REF: usize = RCS_RING_BASE + 0x5A0;
const RCS_RING_MODE_GEN7: usize = RCS_RING_BASE + 0x29C;
const RCS_RING_EXECLIST_SUBMIT_PORT: usize = RCS_RING_BASE + 0x230;
const RCS_RING_EXECLIST_STATUS_LO: usize = RCS_RING_BASE + 0x234;
const RCS_RING_EXECLIST_STATUS_HI: usize = RCS_RING_BASE + 0x238;
const RCS_RING_EXECLIST_CONTROL: usize = RCS_RING_BASE + 0x550;
const RING_EXECLIST_SQ_LO: usize = RCS_RING_BASE + 0x510;
const RING_EXECLIST_SQ_HI: usize = RCS_RING_BASE + 0x514;
const EL_CTRL_LOAD: u32 = 1 << 0;
const CTX_CTRL_RS_CTX_ENABLE: u32 = 1 << 1;
const CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT: u32 = 1 << 0;
const CTX_CTRL_ENGINE_CTX_SAVE_INHIBIT: u32 = 1 << 2;
const CTX_CTRL_INHIBIT_SYN_CTX_SWITCH: u32 = 1 << 3;
const GFX_FLSH_CNTL_GEN6: usize = 0x101008;
const GFX_FLSH_CNTL_EN: u32 = 1 << 0;
const FORCEWAKE_RENDER_GEN11: usize = 0x0A278;
const FORCEWAKE_ACK_RENDER: usize = 0x0D84;
const FORCEWAKE_KERNEL: u32 = 1 << 0;
const FORCEWAKE_KERNEL_FALLBACK: u32 = 1 << 15;
const GEN9_CLKGATE_DIS_5: usize = 0x046540;
const DPCE_GATING_DIS: u32 = 1 << 17;
const GEN8_CHICKEN_DCPR_1: usize = 0x046430;
const DDI_CLOCK_REG_ACCESS: u32 = 1 << 7;
const GT_DISP_PWRON: usize = 0x138090;
const GT_DISP_PWRON_PHY_A_REQ: u32 = 1 << 0;
const GT_DISP_PWRON_PHY_B_REQ: u32 = 1 << 1;
const GT_DISP_PWRON_REQ_MASK: u32 = GT_DISP_PWRON_PHY_A_REQ | GT_DISP_PWRON_PHY_B_REQ;
const RING_VALID: u32 = 0x0000_0001;
const RING_CTL_STOP: u32 = 0;
const RING_MI_MODE_STOP_RING: u32 = 1 << 8;
const MODE_IDLE: u32 = 1 << 9;
const GFX_TLB_INVALIDATE_EXPLICIT: u32 = 1 << 13;
const GFX_PPGTT_ENABLE: u32 = 1 << 9;
const GEN11_GFX_DISABLE_LEGACY_MODE: u32 = 1 << 3;
const MI_BATCH_BUFFER_START_GEN8: u32 = (0x31 << 23) | 1;
const MI_BATCH_GTT: u32 = 2 << 6;
const MI_USE_GGTT: u32 = 1 << 22;
const MI_STORE_DWORD_IMM_GEN4: u32 = (0x20 << 23) | 2;
const MI_LOAD_REGISTER_IMM: u32 = 0x1100_0000;
const MI_LRI_FORCE_POSTED: u32 = 1 << 12;
const MI_BATCH_BUFFER_END: u32 = 0x0500_0000;
const MI_NOOP: u32 = 0;
const BLT_RING_DWORDS: usize = 4;
const RCS_BATCH_DWORDS: usize = 24;
const BLT_RING_TAIL_BYTES: u32 = (BLT_RING_DWORDS * core::mem::size_of::<u32>()) as u32;
const BLT_POLL_ITERS: usize = 4096;
const BLT_POLL_LOG_STEP: usize = 256;
const FORCEWAKE_POLL_ITERS: usize = 20_000;
const RCS_EXEC_RESULT_DONE: u32 = 0xC0DE_7701;
const INTEL_LEGACY_64B_CONTEXT: u32 = 3;
const GEN8_CTX_VALID: u32 = 1 << 0;
const GEN8_CTX_PRIVILEGE: u32 = 1 << 8;
const GEN12_CTX_PRIORITY_NORMAL: u32 = 1 << 9;
const GEN8_CTX_ADDRESSING_MODE_SHIFT: u32 = 3;
const LRC_STATE_OFFSET_DWORDS: usize = 4096 / core::mem::size_of::<u32>();

#[cfg(target_arch = "x86_64")]
#[inline]
fn dma_cache_flush(ptr: *const u8, len: usize) {
    unsafe {
        use core::arch::x86_64::{_mm_clflush, _mm_mfence};

        if ptr.is_null() || len == 0 {
            return;
        }

        let line = 64usize;
        let start = (ptr as usize) & !(line - 1);
        let end = (ptr as usize).saturating_add(len);
        let mut cur = start;
        while cur < end {
            _mm_clflush(cur as *const _);
            cur = cur.saturating_add(line);
        }
        _mm_mfence();
    }
}

#[cfg(not(target_arch = "x86_64"))]
#[inline]
fn dma_cache_flush(_ptr: *const u8, _len: usize) {}

#[inline]
pub(super) fn mmio_read32(warm: Igpu770WarmState, off: usize) -> u32 {
    if off.checked_add(4).is_none_or(|end| end > warm.mmio_len) {
        return 0;
    }
    let ptr = (warm.mmio_base + off) as *const u32;
    unsafe { core::ptr::read_volatile(ptr) }
}

#[inline]
pub(super) fn mmio_write32(warm: Igpu770WarmState, off: usize, value: u32) -> bool {
    if off.checked_add(4).is_none_or(|end| end > warm.mmio_len) {
        return false;
    }
    let ptr = (warm.mmio_base + off) as *mut u32;
    unsafe { core::ptr::write_volatile(ptr, value) };
    true
}

#[inline]
fn ring_ctl_value(size: usize) -> Option<u32> {
    let size = u32::try_from(size).ok()?;
    Some(size.checked_sub(GGTT_PAGE_BYTES as u32)? | RING_VALID)
}

#[inline]
fn masked_bit_disable(bit: u32) -> u32 {
    bit << 16
}

#[inline]
fn encode_hws_pga(phys: u64) -> u32 {
    (phys as u32) | (((phys >> 28) as u32) & 0xF0)
}

#[inline]
fn ring_head_addr(value: u32) -> u32 {
    value & 0x001F_FFFC
}

#[inline]
fn ring_tail_addr(value: u32) -> u32 {
    value & 0x001F_FFF8
}

fn log_rcs_regs(warm: Igpu770WarmState, label: &str) {
    crate::log!(
        "intel/igpu770: rcs-regs label={} ctl=0x{:08X} head=0x{:08X} tail=0x{:08X} start=0x{:08X} mi_mode=0x{:08X} mode=0x{:08X} ctx_ctl=0x{:08X} execlist_ctl=0x{:08X} execlist_lo=0x{:08X} execlist_hi=0x{:08X} acthd=0x{:08X} ipeir=0x{:08X} ipehr=0x{:08X} eir=0x{:08X} emr=0x{:08X} instdone=0x{:08X} instps=0x{:08X} bbaddr=0x{:08X} bbaddr_udw=0x{:08X}\n",
        label,
        mmio_read32(warm, RCS_RING_CTL),
        mmio_read32(warm, RCS_RING_HEAD),
        mmio_read32(warm, RCS_RING_TAIL),
        mmio_read32(warm, RCS_RING_START),
        mmio_read32(warm, RCS_RING_MI_MODE),
        mmio_read32(warm, RCS_RING_MODE_GEN7),
        mmio_read32(warm, RCS_RING_CONTEXT_CONTROL),
        mmio_read32(warm, RCS_RING_EXECLIST_CONTROL),
        mmio_read32(warm, RCS_RING_EXECLIST_STATUS_LO),
        mmio_read32(warm, RCS_RING_EXECLIST_STATUS_HI),
        mmio_read32(warm, RCS_RING_ACTHD),
        mmio_read32(warm, RCS_RING_IPEIR),
        mmio_read32(warm, RCS_RING_IPEHR),
        mmio_read32(warm, RCS_RING_EIR),
        mmio_read32(warm, RCS_RING_EMR),
        mmio_read32(warm, RCS_RING_INSTDONE),
        mmio_read32(warm, RCS_RING_INSTPS),
        mmio_read32(warm, RCS_RING_BBADDR),
        mmio_read32(warm, RCS_RING_BBADDR_UDW)
    );
    intel_770_registers::log_engine_wakeup_table(label, |off| mmio_read32(warm, off));
}

fn ggtt_invalidate(warm: Igpu770WarmState) -> u32 {
    let _ = mmio_write32(warm, GFX_FLSH_CNTL_GEN6, GFX_FLSH_CNTL_EN);
    mmio_read32(warm, GFX_FLSH_CNTL_GEN6)
}

fn log_rcs_mode_summary(warm: Igpu770WarmState, label: &str) {
    let mi_mode = mmio_read32(warm, RCS_RING_MI_MODE);
    let mode = mmio_read32(warm, RCS_RING_MODE_GEN7);
    let ctx_ctl = mmio_read32(warm, RCS_RING_CONTEXT_CONTROL);
    let execlist_ctl = mmio_read32(warm, RCS_RING_EXECLIST_CONTROL);
    crate::log!(
        "intel/igpu770: rcs-mode label={} mi_mode=0x{:08X} mode=0x{:08X} ctx_ctl=0x{:08X} execlist_ctl=0x{:08X} mode_idle={} stop_ring={} tlb_invalidate_explicit={} ppgtt_enable={} legacy_disable={}\n",
        label,
        mi_mode,
        mode,
        ctx_ctl,
        execlist_ctl,
        ((mi_mode & MODE_IDLE) != 0) as u8,
        ((mi_mode & RING_MI_MODE_STOP_RING) != 0) as u8,
        ((mode & GFX_TLB_INVALIDATE_EXPLICIT) != 0) as u8,
        ((mode & GFX_PPGTT_ENABLE) != 0) as u8,
        ((mode & GEN11_GFX_DISABLE_LEGACY_MODE) != 0) as u8
    );
}

#[inline]
fn masked_bit_enable(bit: u32) -> u32 {
    bit | (bit << 16)
}

fn wait_forcewake_ack(warm: Igpu770WarmState, mask: u32, expected: u32) -> (bool, u32, usize) {
    let mut last = mmio_read32(warm, FORCEWAKE_ACK_RENDER);
    if (last & mask) == expected {
        return (true, last, 0);
    }

    let mut iter = 0usize;
    while iter < FORCEWAKE_POLL_ITERS {
        core::hint::spin_loop();
        last = mmio_read32(warm, FORCEWAKE_ACK_RENDER);
        if (last & mask) == expected {
            return (true, last, iter + 1);
        }
        iter += 1;
    }

    (false, last, FORCEWAKE_POLL_ITERS)
}

pub(super) fn forcewake_gt_acquire(warm: Igpu770WarmState) -> u32 {
    let ack_before = mmio_read32(warm, FORCEWAKE_ACK_RENDER);
    crate::log!(
        "intel/igpu770: forcewake-render pre ack=0x{:08X}\n",
        ack_before
    );
    if FORCEWAKE_GT_HELD.load(Ordering::Acquire) {
        crate::log!(
            "intel/igpu770: forcewake-render already-held ack=0x{:08X}\n",
            ack_before
        );
        return ack_before;
    }

    let _ = mmio_write32(
        warm,
        FORCEWAKE_RENDER_GEN11,
        masked_bit_disable(FORCEWAKE_KERNEL | FORCEWAKE_KERNEL_FALLBACK),
    );
    let (_, ack_after_clear, clear_iters) =
        wait_forcewake_ack(warm, FORCEWAKE_KERNEL | FORCEWAKE_KERNEL_FALLBACK, 0);

    let _ = mmio_write32(
        warm,
        FORCEWAKE_RENDER_GEN11,
        masked_bit_enable(FORCEWAKE_KERNEL),
    );
    let (mut set_ok, mut ack, mut iter) =
        wait_forcewake_ack(warm, FORCEWAKE_KERNEL, FORCEWAKE_KERNEL);

    let mut fallback_used = false;
    if !set_ok {
        fallback_used = true;
        let _ = mmio_write32(
            warm,
            FORCEWAKE_RENDER_GEN11,
            masked_bit_disable(FORCEWAKE_KERNEL_FALLBACK),
        );
        let _ = wait_forcewake_ack(warm, FORCEWAKE_KERNEL_FALLBACK, 0);
        let _ = mmio_write32(
            warm,
            FORCEWAKE_RENDER_GEN11,
            masked_bit_enable(FORCEWAKE_KERNEL_FALLBACK),
        );
        let _ = wait_forcewake_ack(
            warm,
            FORCEWAKE_KERNEL_FALLBACK,
            FORCEWAKE_KERNEL_FALLBACK,
        );
        let (retry_ok, retry_ack, retry_iters) =
            wait_forcewake_ack(warm, FORCEWAKE_KERNEL, FORCEWAKE_KERNEL);
        set_ok = retry_ok;
        ack = retry_ack;
        iter = retry_iters;
        let _ = mmio_write32(
            warm,
            FORCEWAKE_RENDER_GEN11,
            masked_bit_disable(FORCEWAKE_KERNEL_FALLBACK),
        );
        let _ = wait_forcewake_ack(warm, FORCEWAKE_KERNEL_FALLBACK, 0);
    }

    crate::log!(
        "intel/igpu770: forcewake-render acquire req=0x{:08X} ack=0x{:08X} cleared=0x{:08X} clear_iters={} iters={} fallback={}\n",
        FORCEWAKE_KERNEL,
        ack,
        ack_after_clear,
        clear_iters,
        iter,
        fallback_used as u8
    );
    intel_770_registers::log_engine_wakeup_table("post-forcewake", |off| mmio_read32(warm, off));
    if set_ok {
        FORCEWAKE_GT_HELD.store(true, Ordering::Release);
    }
    ack
}

fn forcewake_gt_release(warm: Igpu770WarmState) -> u32 {
    let _ = mmio_write32(
        warm,
        FORCEWAKE_RENDER_GEN11,
        masked_bit_disable(FORCEWAKE_KERNEL | FORCEWAKE_KERNEL_FALLBACK),
    );
    let (_, ack, _) = wait_forcewake_ack(warm, FORCEWAKE_KERNEL | FORCEWAKE_KERNEL_FALLBACK, 0);
    FORCEWAKE_GT_HELD.store(false, Ordering::Release);
    crate::log!(
        "intel/igpu770: forcewake-render release ack=0x{:08X}\n",
        ack
    );
    ack
}

fn forcewake_gt_mmio_sanity(warm: Igpu770WarmState) {
    if let Some(reg) = intel_770_registers::describe_register(RCS_RING_IMR) {
        crate::log!(
            "intel/igpu770: forcewake-render sanity-target block={} reg={} off=0x{:05X} desc={}\n",
            reg.block,
            reg.name,
            reg.offset,
            reg.description
        );
    }
    let before = mmio_read32(warm, RCS_RING_IMR);
    let toggled = before ^ 0x0000_0001;
    let _ = mmio_write32(warm, RCS_RING_IMR, toggled);
    let after = mmio_read32(warm, RCS_RING_IMR);
    let _ = mmio_write32(warm, RCS_RING_IMR, before);
    let restored = mmio_read32(warm, RCS_RING_IMR);
    crate::log!(
        "intel/igpu770: forcewake-render sanity reg=RCS_IMR before=0x{:08X} wrote=0x{:08X} after=0x{:08X} restored=0x{:08X}\n",
        before,
        toggled,
        after,
        restored
    );
}

fn apply_adlp_display_workarounds(warm: Igpu770WarmState) -> (u32, u32) {
    let clk5_before = mmio_read32(warm, GEN9_CLKGATE_DIS_5);
    let clk5_after = clk5_before | DPCE_GATING_DIS;
    let _ = mmio_write32(warm, GEN9_CLKGATE_DIS_5, clk5_after);

    let dcpr1_before = mmio_read32(warm, GEN8_CHICKEN_DCPR_1);
    let dcpr1_after = dcpr1_before & !DDI_CLOCK_REG_ACCESS;
    let _ = mmio_write32(warm, GEN8_CHICKEN_DCPR_1, dcpr1_after);

    (
        mmio_read32(warm, GEN9_CLKGATE_DIS_5),
        mmio_read32(warm, GEN8_CHICKEN_DCPR_1),
    )
}

pub(super) fn request_display_power_with_forcewake(warm: Igpu770WarmState) -> bool {
    let _forcewake_ack = forcewake_gt_acquire(warm);
    let (clk5_rb, dcpr1_rb) = apply_adlp_display_workarounds(warm);
    let orig = mmio_read32(warm, GT_DISP_PWRON);
    let req = orig | GT_DISP_PWRON_REQ_MASK;
    let wrote = mmio_write32(warm, GT_DISP_PWRON, req);
    let mut rb = mmio_read32(warm, GT_DISP_PWRON);
    let mut poll_iters = 0usize;
    while poll_iters < 1024 && (rb & GT_DISP_PWRON_REQ_MASK) == 0 {
        core::hint::spin_loop();
        rb = mmio_read32(warm, GT_DISP_PWRON);
        poll_iters += 1;
    }
    let latched = wrote && (rb & GT_DISP_PWRON_REQ_MASK) != 0;
    crate::log!(
        "intel/igpu770: display-power-request register=GT_DISP_PWRON orig=0x{:08X} req=0x{:08X} rb=0x{:08X} clk5_rb=0x{:08X} dcpr1_rb=0x{:08X} poll_iters={} latched={}\n",
        orig,
        req,
        rb,
        clk5_rb,
        dcpr1_rb,
        poll_iters,
        latched as u8
    );
    latched
}

fn build_rcs_store_pixels_batch(warm: Igpu770WarmState, fill: BltFillRectPlan) {
    let pitch = fill.pitch as u64;
    let pixels = [
        fill.dst_gpu_addr,
        fill.dst_gpu_addr + 4,
        fill.dst_gpu_addr + pitch,
        fill.dst_gpu_addr + pitch + 4,
    ];
    let dwords =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, RCS_BATCH_DWORDS) };

    let mut i = 0usize;
    for addr in pixels {
        dwords[i] = MI_STORE_DWORD_IMM_GEN4 | MI_USE_GGTT;
        dwords[i + 1] = addr as u32;
        dwords[i + 2] = (addr >> 32) as u32;
        dwords[i + 3] = fill.color;
        i += 4;
    }
    dwords[i] = MI_STORE_DWORD_IMM_GEN4 | MI_USE_GGTT;
    dwords[i + 1] = GPU_VA_RESULT_BASE as u32;
    dwords[i + 2] = (GPU_VA_RESULT_BASE >> 32) as u32;
    dwords[i + 3] = RCS_EXEC_RESULT_DONE;
    i += 4;

    dwords[i] = MI_BATCH_BUFFER_END;
    dwords[i + 1] = MI_NOOP;

    dma_cache_flush(warm.batch_virt as *const u8, RCS_BATCH_DWORDS * core::mem::size_of::<u32>());
}

fn build_ring_batch_start(warm: Igpu770WarmState, batch_gpu_addr: u64) -> usize {
    let dwords =
        unsafe { core::slice::from_raw_parts_mut(warm.ring_virt as *mut u32, BLT_RING_DWORDS) };

    dwords[0] = MI_BATCH_BUFFER_START_GEN8 | MI_BATCH_GTT;
    dwords[1] = batch_gpu_addr as u32;
    dwords[2] = (batch_gpu_addr >> 32) as u32;
    dwords[3] = MI_NOOP;

    dma_cache_flush(warm.ring_virt as *const u8, BLT_RING_TAIL_BYTES as usize);
    BLT_RING_TAIL_BYTES as usize
}

#[inline]
fn mi_lri_num_regs(num_regs: u32) -> u32 {
    num_regs.saturating_mul(2).saturating_sub(1)
}

fn init_gen12_lrc_context_image(
    warm: Igpu770WarmState,
    ring_start: u32,
    ring_tail: u32,
    ring_ctl: u32,
) -> bool {
    let total_dwords = warm.context_len / core::mem::size_of::<u32>();
    if total_dwords <= LRC_STATE_OFFSET_DWORDS {
        return false;
    }

    let dwords = unsafe { core::slice::from_raw_parts_mut(warm.context_virt as *mut u32, total_dwords) };
    dwords.fill(0);

    let state = &mut dwords[LRC_STATE_OFFSET_DWORDS..];
    if state.len() < 32 {
        return false;
    }

    state[0] = MI_NOOP;
    state[1] = MI_LOAD_REGISTER_IMM | MI_LRI_FORCE_POSTED | mi_lri_num_regs(13);

    state[2] = 0x244;
    state[3] = CTX_CTRL_RS_CTX_ENABLE;
    state[4] = 0x034;
    state[5] = 0;
    state[6] = 0x030;
    state[7] = ring_tail;
    state[8] = 0x038;
    state[9] = ring_start;
    state[10] = 0x03c;
    state[11] = ring_ctl;
    state[12] = 0x168;
    state[13] = 0;
    state[14] = 0x140;
    state[15] = 0;
    state[16] = 0x110;
    state[17] = 0;
    state[18] = 0x1c0;
    state[19] = 0;
    state[20] = 0x1c4;
    state[21] = 0;
    state[22] = 0x1c8;
    state[23] = 0;
    state[24] = 0x180;
    state[25] = 0;
    state[26] = 0x2b4;
    state[27] = 0;

    dma_cache_flush(warm.context_virt as *const u8, warm.context_len);
    true
}

#[inline]
fn build_execlist_context_descriptor(context_gpu_addr: u64) -> (u32, u32) {
    let base = (context_gpu_addr as u32) & 0xFFFF_F000;
    let desc = base
        | GEN8_CTX_VALID
        | GEN8_CTX_PRIVILEGE
        | GEN12_CTX_PRIORITY_NORMAL
        | (INTEL_LEGACY_64B_CONTEXT << GEN8_CTX_ADDRESSING_MODE_SHIFT);
    (desc, (context_gpu_addr >> 32) as u32)
}

fn execlist_submit_port_push(
    warm: Igpu770WarmState,
    context0_lo: u32,
    context0_hi: u32,
    context1_lo: u32,
    context1_hi: u32,
) {
    // Execlist submit port consumes two context descriptors in-order.
    let _ = mmio_write32(warm, RCS_RING_EXECLIST_SUBMIT_PORT, context0_lo);
    let _ = mmio_write32(warm, RCS_RING_EXECLIST_SUBMIT_PORT, context0_hi);
    let _ = mmio_write32(warm, RCS_RING_EXECLIST_SUBMIT_PORT, context1_lo);
    let _ = mmio_write32(warm, RCS_RING_EXECLIST_SUBMIT_PORT, context1_hi);
}

#[derive(Copy, Clone, Debug)]
pub struct Igpu770WarmState {
    pub ring_phys: u64,
    pub ring_virt: *mut u8,
    pub ring_len: usize,
    pub context_phys: u64,
    pub context_virt: *mut u8,
    pub context_len: usize,
    pub batch_phys: u64,
    pub batch_virt: *mut u8,
    pub batch_len: usize,
    pub result_phys: u64,
    pub result_virt: *mut u8,
    pub result_len: usize,
    pub guc_fw_phys: u64,
    pub guc_fw_virt: *mut u8,
    pub guc_fw_len: usize,
    pub guc_fw_xfer_len: usize,
    pub guc_fw_gpu_addr: u64,
    pub mmio_base: usize,
    pub mmio_len: usize,
    pub aperture_bar_phys: u64,
    pub aperture_bar_size: u64,
    pub limine_fb_phys: u64,
    pub limine_fb_virt: usize,
    pub limine_fb_size: usize,
    pub limine_fb_pitch: usize,
    pub limine_fb_width: usize,
    pub limine_fb_height: usize,
    pub limine_fb_bpp: usize,
}

unsafe impl Send for Igpu770WarmState {}
unsafe impl Sync for Igpu770WarmState {}

static WARM_STATE: Mutex<Option<Igpu770WarmState>> = Mutex::new(None);
static GGTT_BLT_SMOKE_RAN: AtomicBool = AtomicBool::new(false);
static GGTT_RECON_RAN: AtomicBool = AtomicBool::new(false);
static GGTT_MAPS_RAN: AtomicBool = AtomicBool::new(false);
static FORCEWAKE_GT_HELD: AtomicBool = AtomicBool::new(false);

#[derive(Copy, Clone, Debug)]
struct LimineFramebufferInfo {
    phys: u64,
    virt: usize,
    size: usize,
    pitch: usize,
    width: usize,
    height: usize,
    bpp: usize,
}

fn limine_framebuffer_info() -> Option<LimineFramebufferInfo> {
    use ::limine::framebuffer::MemoryModel;

    let fb = crate::limine::framebuffer_response()?.framebuffers().next()?;
    if fb.memory_model() != MemoryModel::RGB {
        return None;
    }
    let bpp = fb.bpp() as usize;
    if bpp != 32 {
        return None;
    }

    let virt = fb.addr() as usize;
    let phys = crate::phys::virt_to_phys_checked(fb.addr())?;
    let pitch = fb.pitch() as usize;
    let width = fb.width() as usize;
    let height = fb.height() as usize;
    let size = pitch.saturating_mul(height);

    Some(LimineFramebufferInfo {
        phys,
        virt,
        size,
        pitch,
        width,
        height,
        bpp,
    })
}

#[inline]
pub fn warm_state() -> Option<Igpu770WarmState> {
    *WARM_STATE.lock()
}

#[inline]
pub fn warmed() -> bool {
    WARM_STATE.lock().is_some()
}

#[derive(Copy, Clone, Debug)]
struct GgttMapPlan {
    gpu_addr: u64,
    phys: u64,
    size: usize,
    pages: usize,
    aperture_backed: bool,
}

#[derive(Copy, Clone, Debug)]
struct BltFillRectPlan {
    dst_gpu_addr: u64,
    dst_phys: u64,
    rect_w: usize,
    rect_h: usize,
    pitch: usize,
    color: u32,
}

fn ggtt_map_plan_system_ram(phys: u64, size: usize, gpu_addr: u64) -> Option<GgttMapPlan> {
    if phys == 0 || size == 0 {
        return None;
    }
    let pages = size.div_ceil(WARM_ALIGN);
    Some(GgttMapPlan {
        gpu_addr,
        phys,
        size,
        pages,
        aperture_backed: false,
    })
}

fn ggtt_map_plan_aperture_backed(phys: u64, size: usize, aperture_base: u64) -> Option<GgttMapPlan> {
    if phys == 0 || size == 0 || aperture_base == 0 {
        return None;
    }
    if phys < aperture_base {
        return None;
    }
    let offset = phys.checked_sub(aperture_base)?;
    let size_u64 = u64::try_from(size).ok()?;
    let _ = offset.checked_add(size_u64)?;
    let pages = size.div_ceil(WARM_ALIGN);
    Some(GgttMapPlan {
        gpu_addr: offset,
        phys,
        size,
        pages,
        aperture_backed: true,
    })
}

fn log_ggtt_map_plan(label: &str, plan: GgttMapPlan) {
    crate::log!(
        "intel/igpu770: ggtt-plan label={} gpu=0x{:X} phys=0x{:X} size=0x{:X} pages={} aperture_backed={}\n",
        label,
        plan.gpu_addr,
        plan.phys,
        plan.size,
        plan.pages,
        plan.aperture_backed as u8
    );
}

fn ggtt_alias_ptr(warm: Igpu770WarmState, byte_off: usize) -> Option<*mut u64> {
    if byte_off.checked_add(GGTT_PTE_BYTES)? > GGTT_ALIAS_BYTES {
        return None;
    }
    let base = warm.mmio_base.checked_add(GGTT_ALIAS_BASE_OFF)?;
    let ptr = base.checked_add(byte_off)? as *mut u64;
    Some(ptr)
}

fn ggtt_pte_byte_off(gpu_addr: u64) -> Option<usize> {
    if gpu_addr & (GGTT_PAGE_BYTES - 1) != 0 {
        return None;
    }
    let index = gpu_addr / GGTT_PAGE_BYTES;
    usize::try_from(index).ok()?.checked_mul(GGTT_PTE_BYTES)
}

fn ggtt_pte_encode(phys: u64) -> u64 {
    (phys & !0xFFFu64) | GEN8_PAGE_PRESENT
}

fn ggtt_write_pte(warm: Igpu770WarmState, gpu_addr: u64, phys: u64) -> Option<u64> {
    let byte_off = ggtt_pte_byte_off(gpu_addr)?;
    let ptr = ggtt_alias_ptr(warm, byte_off)?;
    let pte = ggtt_pte_encode(phys);
    unsafe {
        core::ptr::write_volatile(ptr, pte);
        Some(core::ptr::read_volatile(ptr))
    }
}

fn ggtt_program_plan(label: &str, warm: Igpu770WarmState, plan: GgttMapPlan) -> bool {
    let mut page = 0usize;
    while page < plan.pages {
        let gpu_addr = plan
            .gpu_addr
            .saturating_add((page as u64).saturating_mul(GGTT_PAGE_BYTES));
        let phys = plan
            .phys
            .saturating_add((page as u64).saturating_mul(GGTT_PAGE_BYTES));
        let Some(readback) = ggtt_write_pte(warm, gpu_addr, phys) else {
            crate::log!(
                "intel/igpu770: ggtt-map label={} page={} gpu=0x{:X} phys=0x{:X} status=write-failed\n",
                label,
                page,
                gpu_addr,
                phys
            );
            return false;
        };
        crate::log!(
            "intel/igpu770: ggtt-map label={} page={} gpu=0x{:X} phys=0x{:X} pte=0x{:016X}\n",
            label,
            page,
            gpu_addr,
            phys,
            readback
        );
        page += 1;
    }
    true
}

fn blt_fill_rect_plan(warm: Igpu770WarmState) -> Option<BltFillRectPlan> {
    let rect_w = SMOKE_RECT_W.min(warm.limine_fb_width.max(1));
    let rect_h = SMOKE_RECT_H.min(warm.limine_fb_height.max(1));
    let dst = ggtt_map_plan_aperture_backed(
        warm.limine_fb_phys,
        warm.limine_fb_size,
        warm.aperture_bar_phys,
    )?;
    Some(BltFillRectPlan {
        dst_gpu_addr: dst.gpu_addr,
        dst_phys: warm.limine_fb_phys,
        rect_w,
        rect_h,
        pitch: warm.limine_fb_pitch,
        color: SMOKE_COLOR_XRGB8888,
    })
}

pub fn ggtt_recon_once() {
    if GGTT_RECON_RAN.swap(true, Ordering::AcqRel) {
        return;
    }

    let Some(warm) = warm_state() else {
        crate::log!("intel/igpu770: ggtt-recon skipped reason=not-warmed\n");
        return;
    };

    let fb = ggtt_map_plan_aperture_backed(
        warm.limine_fb_phys,
        warm.limine_fb_size,
        warm.aperture_bar_phys,
    );
    let ring = ggtt_map_plan_system_ram(warm.ring_phys, warm.ring_len, GPU_VA_RING_BASE);
    let context =
        ggtt_map_plan_system_ram(warm.context_phys, warm.context_len, GPU_VA_CONTEXT_BASE);
    let batch = ggtt_map_plan_system_ram(warm.batch_phys, warm.batch_len, GPU_VA_BATCH_BASE);
    let result = ggtt_map_plan_system_ram(warm.result_phys, warm.result_len, GPU_VA_RESULT_BASE);
    let guc_fw = ggtt_map_plan_system_ram(warm.guc_fw_phys, warm.guc_fw_len, warm.guc_fw_gpu_addr);

    crate::log!(
        "intel/igpu770: ggtt-recon aperture=0x{:X}/0x{:X} limine_fb_phys=0x{:X} limine_fb_size=0x{:X}\n",
        warm.aperture_bar_phys,
        warm.aperture_bar_size,
        warm.limine_fb_phys,
        warm.limine_fb_size
    );

    if let Some(plan) = fb {
        log_ggtt_map_plan("framebuffer", plan);
    } else {
        crate::log!("intel/igpu770: ggtt-recon framebuffer plan unavailable\n");
    }
    if let Some(plan) = ring {
        log_ggtt_map_plan("ring", plan);
    }
    if let Some(plan) = context {
        log_ggtt_map_plan("context", plan);
    }
    if let Some(plan) = batch {
        log_ggtt_map_plan("batch", plan);
    }
    if let Some(plan) = result {
        log_ggtt_map_plan("result", plan);
    }
    if let Some(plan) = guc_fw {
        log_ggtt_map_plan("guc-fw", plan);
    }
    crate::log!(
        "intel/igpu770: ggtt-recon note system-ram objects need explicit GGTT PTEs; framebuffer is aperture-backed\n"
    );
    crate::log!(
        "intel/igpu770: ggtt-recon gtt_alias_off=0x{:X} gtt_alias_bytes=0x{:X}\n",
        GGTT_ALIAS_BASE_OFF,
        GGTT_ALIAS_BYTES
    );
}

pub fn ggtt_map_smoke_objects_once() {
    if GGTT_MAPS_RAN.swap(true, Ordering::AcqRel) {
        return;
    }

    let Some(warm) = warm_state() else {
        crate::log!("intel/igpu770: ggtt-map skipped reason=not-warmed\n");
        return;
    };

    let Some(ring) = ggtt_map_plan_system_ram(warm.ring_phys, warm.ring_len, GPU_VA_RING_BASE) else {
        crate::log!("intel/igpu770: ggtt-map skipped reason=ring-plan\n");
        return;
    };
    let Some(context) =
        ggtt_map_plan_system_ram(warm.context_phys, warm.context_len, GPU_VA_CONTEXT_BASE)
    else {
        crate::log!("intel/igpu770: ggtt-map skipped reason=context-plan\n");
        return;
    };
    let Some(batch) = ggtt_map_plan_system_ram(warm.batch_phys, warm.batch_len, GPU_VA_BATCH_BASE) else {
        crate::log!("intel/igpu770: ggtt-map skipped reason=batch-plan\n");
        return;
    };
    let Some(result) =
        ggtt_map_plan_system_ram(warm.result_phys, warm.result_len, GPU_VA_RESULT_BASE)
    else {
        crate::log!("intel/igpu770: ggtt-map skipped reason=result-plan\n");
        return;
    };
    let guc_fw = ggtt_map_plan_system_ram(warm.guc_fw_phys, warm.guc_fw_len, warm.guc_fw_gpu_addr);

    let _ = forcewake_gt_acquire(warm);
    crate::log!("intel/igpu770: ggtt-map begin\n");
    let ok_ring = ggtt_program_plan("ring", warm, ring);
    let ok_context = ggtt_program_plan("context", warm, context);
    let ok_batch = ggtt_program_plan("batch", warm, batch);
    let ok_result = ggtt_program_plan("result", warm, result);
    let ok_guc_fw = guc_fw
        .map(|plan| ggtt_program_plan("guc-fw", warm, plan))
        .unwrap_or(false);
    let flsh = ggtt_invalidate(warm);
    crate::log!(
        "intel/igpu770: ggtt-map summary ring={} context={} batch={} result={} guc_fw={} gfx_flsh_cntl=0x{:08X}\n",
        ok_ring as u8,
        ok_context as u8,
        ok_batch as u8,
        ok_result as u8,
        ok_guc_fw as u8,
        flsh
    );

    intel_guc::bootstrap_once(warm);
}

pub fn ggtt_blt_smoke_test_once() {
    if GGTT_BLT_SMOKE_RAN.swap(true, Ordering::AcqRel) {
        return;
    }

    let Some(warm) = warm_state() else {
        crate::log!("intel/igpu770: ggtt-blt-smoke skipped reason=not-warmed\n");
        return;
    };
    if !intel_guc::ready() {
        crate::log!(
            "intel/igpu770: ggtt-blt-smoke skipped reason=guc-not-ready guc_status=0x{:08X}\n",
            intel_guc::status(warm)
        );
        return;
    }

    let Some(ring) = ggtt_map_plan_system_ram(warm.ring_phys, warm.ring_len, GPU_VA_RING_BASE) else {
        crate::log!("intel/igpu770: ggtt-blt-smoke skipped reason=ring-plan\n");
        return;
    };
    let Some(context) =
        ggtt_map_plan_system_ram(warm.context_phys, warm.context_len, GPU_VA_CONTEXT_BASE)
    else {
        crate::log!("intel/igpu770: ggtt-blt-smoke skipped reason=context-plan\n");
        return;
    };
    let Some(batch) = ggtt_map_plan_system_ram(warm.batch_phys, warm.batch_len, GPU_VA_BATCH_BASE) else {
        crate::log!("intel/igpu770: ggtt-blt-smoke skipped reason=batch-plan\n");
        return;
    };
    let Some(result) =
        ggtt_map_plan_system_ram(warm.result_phys, warm.result_len, GPU_VA_RESULT_BASE)
    else {
        crate::log!("intel/igpu770: ggtt-blt-smoke skipped reason=result-plan\n");
        return;
    };
    let Some(fill) = blt_fill_rect_plan(warm) else {
        crate::log!("intel/igpu770: ggtt-blt-smoke skipped reason=fb-plan\n");
        return;
    };

    unsafe {
        ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        ptr::write_bytes(warm.result_virt, 0, warm.result_len);
        core::ptr::write_volatile(warm.result_virt as *mut u32, 0xC0DE_7700);
    }
    dma_cache_flush(warm.result_virt as *const u8, warm.result_len);

    crate::log!("intel/igpu770: ggtt-rcs-smoke begin\n");
    log_ggtt_map_plan("ring", ring);
    log_ggtt_map_plan("context", context);
    log_ggtt_map_plan("batch", batch);
    log_ggtt_map_plan("result", result);
    crate::log!(
        "intel/igpu770: rcs-store-plan dst_gpu=0x{:X} dst_phys=0x{:X} rect={}x{} pitch=0x{:X} color=0x{:08X}\n",
        fill.dst_gpu_addr,
        fill.dst_phys,
        fill.rect_w,
        fill.rect_h,
        fill.pitch,
        fill.color
    );

    build_rcs_store_pixels_batch(warm, fill);
    let ring_tail_bytes = build_ring_batch_start(warm, batch.gpu_addr);
    let Some(ring_ctl) = ring_ctl_value(warm.ring_len) else {
        crate::log!(
            "intel/igpu770: ggtt-blt-smoke skipped reason=ring-ctl ring_len=0x{:X}\n",
            warm.ring_len
        );
        return;
    };
    let ring_start = ring.gpu_addr as u32;
    let context_desc = context.gpu_addr;
    if !init_gen12_lrc_context_image(warm, ring_start, ring_tail_bytes as u32, ring_ctl) {
        crate::log!("intel/igpu770: ggtt-blt-smoke skipped reason=lrc-context-init\n");
        return;
    }
    let (context_desc_lo, context_desc_hi) = build_execlist_context_descriptor(context_desc);

    crate::log!(
        "intel/igpu770: rcs-submit prep ring_start=0x{:08X} ring_ctl=0x{:08X} tail=0x{:X} batch_gpu=0x{:X} context_gpu=0x{:X} ctx_desc_lo=0x{:08X} ctx_desc_hi=0x{:08X} result_phys=0x{:X}\n",
        ring_start,
        ring_ctl,
        ring_tail_bytes,
        batch.gpu_addr,
        context_desc,
        context_desc_lo,
        context_desc_hi,
        warm.result_phys
    );
    let _ = forcewake_gt_acquire(warm);
    let _ = mmio_write32(
        warm,
        RCS_RING_MODE_GEN7,
        masked_bit_enable(GEN11_GFX_DISABLE_LEGACY_MODE),
    );
    let ctx_ctl_before = mmio_read32(warm, RCS_RING_CONTEXT_CONTROL);
    let ctx_ctl_ref_before = mmio_read32(warm, RCS_RING_CONTEXT_CONTROL_REF);
    let ctx_ctl_after = (ctx_ctl_before | CTX_CTRL_RS_CTX_ENABLE)
        & !(CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT
            | CTX_CTRL_ENGINE_CTX_SAVE_INHIBIT
            | CTX_CTRL_INHIBIT_SYN_CTX_SWITCH);
    let ctx_ctl_ref_after = (ctx_ctl_ref_before | CTX_CTRL_RS_CTX_ENABLE)
        & !(CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT
            | CTX_CTRL_ENGINE_CTX_SAVE_INHIBIT
            | CTX_CTRL_INHIBIT_SYN_CTX_SWITCH);
    let _ = mmio_write32(warm, RCS_RING_CONTEXT_CONTROL, ctx_ctl_after);
    let _ = mmio_write32(warm, RCS_RING_CONTEXT_CONTROL_REF, ctx_ctl_ref_after);

    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    execlist_submit_port_push(warm, context_desc_lo, context_desc_hi, 0, 0);
    let _ = mmio_write32(warm, RCS_RING_EXECLIST_CONTROL, EL_CTRL_LOAD);
    let sq_lo_rb = mmio_read32(warm, RING_EXECLIST_SQ_LO);
    let sq_hi_rb = mmio_read32(warm, RING_EXECLIST_SQ_HI);
    let mode_rb = mmio_read32(warm, RCS_RING_MODE_GEN7);
    let ctx_ctl_rb = mmio_read32(warm, RCS_RING_CONTEXT_CONTROL);
    let ctx_ctl_ref_rb = mmio_read32(warm, RCS_RING_CONTEXT_CONTROL_REF);
    let el_ctl_rb = mmio_read32(warm, RCS_RING_EXECLIST_CONTROL);
    let el_status_lo_rb = mmio_read32(warm, RCS_RING_EXECLIST_STATUS_LO);
    let el_status_hi_rb = mmio_read32(warm, RCS_RING_EXECLIST_STATUS_HI);
    crate::log!(
        "intel/igpu770: execlist-submit context sq_lo_req=0x{:08X} sq_hi_req=0x{:08X} sq_lo_rb=0x{:08X} sq_hi_rb=0x{:08X} mode_rb=0x{:08X} ctx_ctl_rb=0x{:08X} ctx_ctl_ref_rb=0x{:08X} el_ctl_rb=0x{:08X} el_status_lo_rb=0x{:08X} el_status_hi_rb=0x{:08X}\n",
        context_desc_lo,
        context_desc_hi,
        sq_lo_rb,
        sq_hi_rb,
        mode_rb,
        ctx_ctl_rb,
        ctx_ctl_ref_rb,
        el_ctl_rb,
        el_status_lo_rb,
        el_status_hi_rb
    );
    forcewake_gt_mmio_sanity(warm);
    log_rcs_mode_summary(warm, "pre");
    log_rcs_regs(warm, "pre");
    log_rcs_mode_summary(warm, "submitted");
    log_rcs_regs(warm, "submitted");

    let mut completed = false;
    let mut first_head = 0u32;
    let mut first_tail = 0u32;
    let mut final_head = 0u32;
    let mut final_tail = 0u32;
    let mut iter = 0usize;
    let execlist_lo0 = mmio_read32(warm, RCS_RING_EXECLIST_STATUS_LO);
    let execlist_hi0 = mmio_read32(warm, RCS_RING_EXECLIST_STATUS_HI);
    while iter < BLT_POLL_ITERS {
        let head = mmio_read32(warm, RCS_RING_HEAD);
        let tail = mmio_read32(warm, RCS_RING_TAIL);
        let execlist_lo = mmio_read32(warm, RCS_RING_EXECLIST_STATUS_LO);
        let execlist_hi = mmio_read32(warm, RCS_RING_EXECLIST_STATUS_HI);
        let result0 = unsafe { core::ptr::read_volatile(warm.result_virt as *const u32) };
        if iter == 0 {
            first_head = head;
            first_tail = tail;
        }
        final_head = head;
        final_tail = tail;
        if iter == 0 || (iter % BLT_POLL_LOG_STEP) == 0 {
            crate::log!(
                "intel/igpu770: rcs-poll iter={} head=0x{:08X} tail=0x{:08X} acthd=0x{:08X} ipeir=0x{:08X} ipehr=0x{:08X} eir=0x{:08X} mode=0x{:08X} instdone=0x{:08X} execlist_lo=0x{:08X} execlist_hi=0x{:08X} result0=0x{:08X}\n",
                iter,
                head,
                tail,
                mmio_read32(warm, RCS_RING_ACTHD),
                mmio_read32(warm, RCS_RING_IPEIR),
                mmio_read32(warm, RCS_RING_IPEHR),
                mmio_read32(warm, RCS_RING_EIR),
                mmio_read32(warm, RCS_RING_MODE_GEN7),
                mmio_read32(warm, RCS_RING_INSTDONE),
                execlist_lo,
                execlist_hi,
                result0
            );
        }
        if result0 == RCS_EXEC_RESULT_DONE {
            completed = true;
            break;
        }
        if execlist_lo != execlist_lo0 || execlist_hi != execlist_hi0 {
            completed = true;
            break;
        }
        core::hint::spin_loop();
        iter += 1;
    }

    dma_cache_flush(warm.result_virt as *const u8, warm.result_len);
    let result0 = unsafe { core::ptr::read_volatile(warm.result_virt as *const u32) };
    let fb0 = unsafe { core::ptr::read_volatile(warm.limine_fb_virt as *const u32) };
    log_rcs_mode_summary(warm, if completed { "post-complete" } else { "post-timeout" });
    log_rcs_regs(warm, if completed { "post-complete" } else { "post-timeout" });
    crate::log!(
        "intel/igpu770: rcs-submit result completed={} iters={} head0=0x{:08X} tail0=0x{:08X} headf=0x{:08X} tailf=0x{:08X} result0=0x{:08X} expect=0x{:08X} fb0=0x{:08X} execlist_lo0=0x{:08X} execlist_hi0=0x{:08X} execlist_lof=0x{:08X} execlist_hif=0x{:08X} forcewake_held={}\n",
        completed as u8,
        iter,
        first_head,
        first_tail,
        final_head,
        final_tail,
        result0,
        RCS_EXEC_RESULT_DONE,
        fb0,
        execlist_lo0,
        execlist_hi0,
        mmio_read32(warm, RCS_RING_EXECLIST_STATUS_LO),
        mmio_read32(warm, RCS_RING_EXECLIST_STATUS_HI),
        FORCEWAKE_GT_HELD.load(Ordering::Acquire) as u8
    );
}

pub fn warm_once(info: IntelDeviceInfo) {
    if info.device_id != INTEL_IGPU770_DEVICE_ID {
        return;
    }

    let mut state = WARM_STATE.lock();
    if state.is_some() {
        return;
    }

    let Some((ring_phys, ring_virt)) = crate::dma::alloc(WARM_RING_BYTES, WARM_ALIGN) else {
        crate::log!("intel/igpu770: warm alloc failed part=ring size=0x{:X}\n", WARM_RING_BYTES);
        return;
    };
    let Some((context_phys, context_virt)) = crate::dma::alloc(WARM_CONTEXT_BYTES, WARM_ALIGN)
    else {
        crate::log!(
            "intel/igpu770: warm alloc failed part=context size=0x{:X}\n",
            WARM_CONTEXT_BYTES
        );
        return;
    };
    let Some((batch_phys, batch_virt)) = crate::dma::alloc(WARM_BATCH_BYTES, WARM_ALIGN) else {
        crate::log!("intel/igpu770: warm alloc failed part=batch size=0x{:X}\n", WARM_BATCH_BYTES);
        return;
    };
    let Some((result_phys, result_virt)) = crate::dma::alloc(WARM_RESULT_BYTES, WARM_ALIGN) else {
        crate::log!(
            "intel/igpu770: warm alloc failed part=result size=0x{:X}\n",
            WARM_RESULT_BYTES
        );
        return;
    };

    unsafe {
        ptr::write_bytes(ring_virt, 0, WARM_RING_BYTES);
        ptr::write_bytes(context_virt, 0, WARM_CONTEXT_BYTES);
        ptr::write_bytes(batch_virt, 0, WARM_BATCH_BYTES);
        ptr::write_bytes(result_virt, 0, WARM_RESULT_BYTES);
    }

    let guc_fw = intel_guc::load_firmware_from_module(WARM_ALIGN);

    let fb = limine_framebuffer_info();

    let warm = Igpu770WarmState {
        ring_phys,
        ring_virt,
        ring_len: WARM_RING_BYTES,
        context_phys,
        context_virt,
        context_len: WARM_CONTEXT_BYTES,
        batch_phys,
        batch_virt,
        batch_len: WARM_BATCH_BYTES,
        result_phys,
        result_virt,
        result_len: WARM_RESULT_BYTES,
        guc_fw_phys: guc_fw.phys,
        guc_fw_virt: guc_fw.virt,
        guc_fw_len: guc_fw.len,
        guc_fw_xfer_len: guc_fw.xfer_len,
        guc_fw_gpu_addr: guc_fw.gpu_addr,
        mmio_base: info.mmio_base.as_ptr() as usize,
        mmio_len: info.mmio_len,
        aperture_bar_phys: info.aperture_bar_phys,
        aperture_bar_size: info.aperture_bar_size,
        limine_fb_phys: fb.map(|v| v.phys).unwrap_or(0),
        limine_fb_virt: fb.map(|v| v.virt).unwrap_or(0),
        limine_fb_size: fb.map(|v| v.size).unwrap_or(0),
        limine_fb_pitch: fb.map(|v| v.pitch).unwrap_or(0),
        limine_fb_width: fb.map(|v| v.width).unwrap_or(0),
        limine_fb_height: fb.map(|v| v.height).unwrap_or(0),
        limine_fb_bpp: fb.map(|v| v.bpp).unwrap_or(0),
    };

    crate::log!(
        "intel/igpu770: warm ring_phys=0x{:X} ring_len=0x{:X} context_phys=0x{:X} context_len=0x{:X} batch_phys=0x{:X} batch_len=0x{:X} result_phys=0x{:X} result_len=0x{:X} guc_fw_phys=0x{:X} guc_fw_len=0x{:X} guc_fw_xfer=0x{:X} guc_fw_gpu=0x{:X} mmio_len=0x{:X} aperture=0x{:X}/0x{:X} limine_fb=0x{:X}/0x{:X} {}x{} pitch=0x{:X} bpp={}\n",
        warm.ring_phys,
        warm.ring_len,
        warm.context_phys,
        warm.context_len,
        warm.batch_phys,
        warm.batch_len,
        warm.result_phys,
        warm.result_len,
        warm.guc_fw_phys,
        warm.guc_fw_len,
        warm.guc_fw_xfer_len,
        warm.guc_fw_gpu_addr,
        warm.mmio_len,
        warm.aperture_bar_phys,
        warm.aperture_bar_size,
        warm.limine_fb_phys,
        warm.limine_fb_size,
        warm.limine_fb_width,
        warm.limine_fb_height,
        warm.limine_fb_pitch,
        warm.limine_fb_bpp
    );

    *state = Some(warm);
}
