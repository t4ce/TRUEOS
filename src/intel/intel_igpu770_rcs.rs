use core::sync::atomic::{AtomicBool, Ordering};

use super::intel_770_registers;
use super::intel_igpu770::{Igpu770WarmState, warm_state};

const GGTT_PAGE_BYTES: u64 = 4096;
const SMOKE_RECT_W: usize = 64;
const SMOKE_RECT_H: usize = 64;
const SMOKE_COLOR_XRGB8888: u32 = 0x00FF_4A24;
const GPU_VA_RING_BASE: u64 = 0x0080_0000;
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
const RCS_RING_MODE_GEN7: usize = RCS_RING_BASE + 0x29C;
const RCS_RING_EXECLIST_STATUS_LO: usize = RCS_RING_BASE + 0x234;
const RCS_RING_EXECLIST_STATUS_HI: usize = RCS_RING_BASE + 0x238;
const RCS_RING_EXECLIST_CONTROL: usize = RCS_RING_BASE + 0x550;
const FORCEWAKE_GT: usize = 0x0A188;
const FORCEWAKE_RENDER_GEN11: usize = 0x0A278;
const FORCEWAKE_ACK_RENDER: usize = 0x0D84;
const FORCEWAKE_KERNEL: u32 = 1 << 0;
const FORCEWAKE_KERNEL_FALLBACK: u32 = 1 << 15;
const RING_MI_MODE_STOP_RING: u32 = 1 << 8;
const MODE_IDLE: u32 = 1 << 9;
const GFX_TLB_INVALIDATE_EXPLICIT: u32 = 1 << 13;
const GFX_PPGTT_ENABLE: u32 = 1 << 9;
const GEN11_GFX_DISABLE_LEGACY_MODE: u32 = 1 << 3;
const FORCEWAKE_POLL_ITERS: usize = 20_000;

static FORCEWAKE_RENDER_HELD: AtomicBool = AtomicBool::new(false);
static FORCEWAKE_GT_HELD: AtomicBool = AtomicBool::new(false);

#[derive(Copy, Clone, Debug)]
struct GgttMapPlan {
    gpu_addr: u64,
    phys: u64,
    size: usize,
    pages: usize,
    aperture_backed: bool,
}

#[derive(Copy, Clone, Debug)]
struct RcsStorePlan {
    dst_gpu_addr: u64,
    dst_phys: u64,
    rect_w: usize,
    rect_h: usize,
    pitch: usize,
    color: u32,
}

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
fn mmio_read32(warm: Igpu770WarmState, off: usize) -> u32 {
    if off.checked_add(4).is_none_or(|end| end > warm.mmio_len) {
        return 0;
    }
    let ptr = (warm.mmio_base + off) as *const u32;
    unsafe { core::ptr::read_volatile(ptr) }
}

#[inline]
fn mmio_write32(warm: Igpu770WarmState, off: usize, value: u32) -> bool {
    if off.checked_add(4).is_none_or(|end| end > warm.mmio_len) {
        return false;
    }
    let ptr = (warm.mmio_base + off) as *mut u32;
    unsafe { core::ptr::write_volatile(ptr, value) };
    true
}

#[inline]
fn masked_bit_enable(bit: u32) -> u32 {
    bit | (bit << 16)
}

#[inline]
fn masked_bit_disable(bit: u32) -> u32 {
    bit << 16
}

fn ggtt_map_plan_system_ram(phys: u64, size: usize, gpu_addr: u64) -> Option<GgttMapPlan> {
    if phys == 0 || size == 0 {
        return None;
    }
    Some(GgttMapPlan {
        gpu_addr,
        phys,
        size,
        pages: size.div_ceil(4096),
        aperture_backed: false,
    })
}

fn ggtt_map_plan_aperture_backed(
    phys: u64,
    size: usize,
    aperture_base: u64,
) -> Option<GgttMapPlan> {
    if phys == 0 || size == 0 || aperture_base == 0 || phys < aperture_base {
        return None;
    }
    Some(GgttMapPlan {
        gpu_addr: phys.checked_sub(aperture_base)?,
        phys,
        size,
        pages: size.div_ceil(4096),
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

fn rcs_store_plan(warm: Igpu770WarmState) -> Option<RcsStorePlan> {
    let rect_w = SMOKE_RECT_W.min(warm.limine_fb_width.max(1));
    let rect_h = SMOKE_RECT_H.min(warm.limine_fb_height.max(1));
    let dst = ggtt_map_plan_aperture_backed(
        warm.limine_fb_phys,
        warm.limine_fb_size,
        warm.aperture_bar_phys,
    )?;
    Some(RcsStorePlan {
        dst_gpu_addr: dst.gpu_addr,
        dst_phys: warm.limine_fb_phys,
        rect_w,
        rect_h,
        pitch: warm.limine_fb_pitch,
        color: SMOKE_COLOR_XRGB8888,
    })
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

fn wait_forcewake_req_latch(
    warm: Igpu770WarmState,
    off: usize,
    mask: u32,
    expected: u32,
) -> (bool, u32, usize) {
    let mut last = mmio_read32(warm, off);
    if (last & mask) == expected {
        return (true, last, 0);
    }
    let mut iter = 0usize;
    while iter < FORCEWAKE_POLL_ITERS {
        core::hint::spin_loop();
        last = mmio_read32(warm, off);
        if (last & mask) == expected {
            return (true, last, iter + 1);
        }
        iter += 1;
    }
    (false, last, FORCEWAKE_POLL_ITERS)
}

fn forcewake_render_acquire(warm: Igpu770WarmState) -> u32 {
    let ack_before = mmio_read32(warm, FORCEWAKE_ACK_RENDER);
    let gt_before = mmio_read32(warm, FORCEWAKE_GT);
    crate::log!(
        "intel/igpu770: forcewake-render pre ack=0x{:08X} gt_req=0x{:08X}\n",
        ack_before,
        gt_before
    );
    if FORCEWAKE_RENDER_HELD.load(Ordering::Acquire) && FORCEWAKE_GT_HELD.load(Ordering::Acquire) {
        crate::log!(
            "intel/igpu770: forcewake-render already-held ack=0x{:08X} gt_req=0x{:08X}\n",
            ack_before,
            gt_before
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

    let _ = mmio_write32(warm, FORCEWAKE_RENDER_GEN11, masked_bit_enable(FORCEWAKE_KERNEL));
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
        let _ = wait_forcewake_ack(warm, FORCEWAKE_KERNEL_FALLBACK, FORCEWAKE_KERNEL_FALLBACK);
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
    let _ = mmio_write32(warm, FORCEWAKE_GT, masked_bit_disable(FORCEWAKE_KERNEL));
    let (_, gt_clear, gt_clear_iters) =
        wait_forcewake_req_latch(warm, FORCEWAKE_GT, FORCEWAKE_KERNEL, 0);
    let _ = mmio_write32(warm, FORCEWAKE_GT, masked_bit_enable(FORCEWAKE_KERNEL));
    let (gt_ok, gt_req, gt_iters) =
        wait_forcewake_req_latch(warm, FORCEWAKE_GT, FORCEWAKE_KERNEL, FORCEWAKE_KERNEL);
    crate::log!(
        "intel/igpu770: forcewake-gt acquire req=0x{:08X} reg=0x{:08X} cleared=0x{:08X} clear_iters={} iters={} held={}\n",
        FORCEWAKE_KERNEL,
        gt_req,
        gt_clear,
        gt_clear_iters,
        gt_iters,
        gt_ok as u8
    );
    intel_770_registers::log_engine_wakeup_table("post-forcewake", |off| mmio_read32(warm, off));
    if set_ok {
        FORCEWAKE_RENDER_HELD.store(true, Ordering::Release);
    }
    FORCEWAKE_GT_HELD.store(gt_ok, Ordering::Release);
    ack
}

fn forcewake_render_mmio_sanity(warm: Igpu770WarmState) {
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
    let toggled = before ^ 1;
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
