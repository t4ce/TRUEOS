use core::{
    ptr,
    sync::atomic::{AtomicBool, Ordering},
};

use spin::Mutex;

use super::IntelDeviceInfo;
use super::intel_770_registers;
use super::intel_guc;
use super::xelp_copy_ngin;
use super::xelp_copy_ngin::{
    COPY_SMOKE_DONE_SLOT, COPY_SMOKE_POST_COPY_SLOT, COPY_SMOKE_PRE_COPY_SLOT,
    COPY_SMOKE_START_SLOT,
};

const INTEL_IGPU770_DEVICE_ID: u16 = 0x4680;
const WARM_RING_BYTES: usize = 4096;
const WARM_CONTEXT_BYTES: usize = 22 * 4096;
const WARM_BATCH_BYTES: usize = 512 * 1024;
const WARM_RESULT_BYTES: usize = 4096;
const WARM_ALIGN: usize = 4096;
const GGTT_ALIAS_BASE_OFF: usize = 0x0080_0000;
const GGTT_ALIAS_BYTES: usize = 0x0080_0000;
const GGTT_PTE_BYTES: usize = 8;
const GGTT_PAGE_BYTES: u64 = 4096;
const GEN8_PAGE_PRESENT: u64 = 1;
const GGTT_MAP_LOG_ALL_THRESHOLD: usize = 16;
const GGTT_MAP_LOG_EDGE_PAGES: usize = 4;
const SMOKE_RECT_W: usize = 320;
const SMOKE_RECT_H: usize = 256;
const SMOKE_COLOR_XRGB8888: u32 = 0x00FF_4A24;
const BCS_RECT_W: usize = 768;
const BCS_RECT_H: usize = 544;
const BCS_COLOR_XRGB8888: u32 = 0x00FF_FFFF;
const GPU_VA_RING_BASE: u64 = 0x0080_0000;
const GPU_VA_CONTEXT_BASE: u64 = 0x0081_0000;
const GPU_VA_BATCH_BASE: u64 = 0x0083_0000;
const GPU_VA_RESULT_BASE: u64 = 0x0084_0000;
const GPU_VA_GUC_FW_BASE: u64 = 0x0085_0000;
const GPU_VA_GUC_ADS_BASE: u64 = 0x0100_0000;
const BCS_RING_BASE: usize = 0x0002_2000;
const BCS_RING_TAIL: usize = BCS_RING_BASE + 0x30;
const BCS_RING_HEAD: usize = BCS_RING_BASE + 0x34;
const BCS_RING_START: usize = BCS_RING_BASE + 0x38;
const BCS_RING_CTL: usize = BCS_RING_BASE + 0x3C;
const BCS_RING_ACTHD: usize = BCS_RING_BASE + 0x74;
const BCS_RING_MI_MODE: usize = BCS_RING_BASE + 0x9C;
const BCS_RING_IMR: usize = BCS_RING_BASE + 0xA8;
const BCS_RING_EIR: usize = BCS_RING_BASE + 0xB0;
const BCS_RING_EMR: usize = BCS_RING_BASE + 0xB4;
const BCS_RING_IPEIR: usize = BCS_RING_BASE + 0x64;
const BCS_RING_IPEHR: usize = BCS_RING_BASE + 0x68;
const BCS_RING_INSTDONE: usize = BCS_RING_BASE + 0x6C;
const BCS_RING_INSTPS: usize = BCS_RING_BASE + 0x70;
const BCS_RING_BBADDR: usize = BCS_RING_BASE + 0x140;
const BCS_RING_BBADDR_UDW: usize = BCS_RING_BASE + 0x168;
const BCS_RING_CONTEXT_CONTROL: usize = BCS_RING_BASE + 0x244;
const BCS_RING_CONTEXT_CONTROL_REF: usize = BCS_RING_BASE + 0x5A0;
const BCS_RING_MODE_GEN7: usize = BCS_RING_BASE + 0x29C;
const BCS_RING_EXECLIST_SUBMIT_PORT: usize = BCS_RING_BASE + 0x230;
const BCS_RING_EXECLIST_STATUS_LO: usize = BCS_RING_BASE + 0x234;
const BCS_RING_EXECLIST_STATUS_HI: usize = BCS_RING_BASE + 0x238;
const BCS_RING_EXECLIST_CONTROL: usize = BCS_RING_BASE + 0x550;
const BCS_RING_EXECLIST_SQ_LO: usize = BCS_RING_BASE + 0x510;
const BCS_RING_EXECLIST_SQ_HI: usize = BCS_RING_BASE + 0x514;
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
const CTX_CONTEXT_CONTROL_DW: usize = 0x02 + 1;
const CTX_RING_HEAD_DW: usize = 0x04 + 1;
const CTX_RING_TAIL_DW: usize = 0x06 + 1;
const CTX_RING_START_DW: usize = 0x08 + 1;
const CTX_RING_CTL_DW: usize = 0x0A + 1;
const CTX_RING_MI_MODE_DW: usize = 0x54 + 1;
const CTX_DESC_FORCE_RESTORE: u32 = 1 << 2;
const STOP_RING: u32 = 1 << 8;
const GFX_FLSH_CNTL_GEN6: usize = 0x101008;
const GFX_FLSH_CNTL_EN: u32 = 1 << 0;
const FORCEWAKE_RENDER_GEN11: usize = 0x0A278;
const FORCEWAKE_MEDIA_GEN11: usize = 0x0A184;
const FORCEWAKE_GT_GEN11: usize = 0x0A188;
const FORCEWAKE_ACK_VDBOX0: usize = 0x0D50;
const FORCEWAKE_ACK_VDBOX1: usize = 0x0D54;
const FORCEWAKE_ACK_VDBOX2: usize = 0x0D58;
const FORCEWAKE_ACK_VDBOX3: usize = 0x0D5C;
const FORCEWAKE_ACK_VEBOX0: usize = 0x0D70;
const FORCEWAKE_ACK_VEBOX1: usize = 0x0D74;
const FORCEWAKE_ACK_VEBOX2: usize = 0x0D78;
const FORCEWAKE_ACK_VEBOX3: usize = 0x0D7C;
const FORCEWAKE_ACK_RENDER: usize = 0x0D84;
const FORCEWAKE_ACK_MEDIA: usize = 0x0D88;
const FORCEWAKE_ACK_GT: usize = 0x130044;
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
const MI_BATCH_NON_SECURE_I965: u32 = 1 << 8;
const MI_USE_GGTT: u32 = 1 << 22;
const MI_STORE_DWORD_IMM_GEN4: u32 = (0x20 << 23) | 2;
const MI_STORE_DWORD_IMM_GEN4_LEN_DW4: u32 = MI_STORE_DWORD_IMM_GEN4 | MI_USE_GGTT | (4 - 2);
const MI_LOAD_REGISTER_IMM: u32 = 0x1100_0000;
const MI_LRI_CS_MMIO: u32 = 1 << 19;
const MI_LRI_FORCE_POSTED: u32 = 1 << 12;
const MI_BATCH_BUFFER_END: u32 = 0x0500_0000;
const MI_NOOP: u32 = 0;
const BLT_RING_DWORDS: usize = 4;
const BLT_RING_TAIL_BYTES: u32 = (BLT_RING_DWORDS * core::mem::size_of::<u32>()) as u32;
const BCS_RING_MARKER_DWORDS: usize = 8;
const BCS_RING_MARKER_TAIL_BYTES: u32 =
    (BCS_RING_MARKER_DWORDS * core::mem::size_of::<u32>()) as u32;
const BLT_POLL_ITERS: usize = 4096;
const BLT_POLL_LOG_STEP: usize = 256;
const FORCEWAKE_POLL_ITERS: usize = 20_000;
const RCS_EXEC_RESULT_DONE: u32 = 0xC0DE_7701;
const BCS_EXEC_RESULT_START: u32 = 0x1CE0_BC50;
const BCS_EXEC_RESULT_PRE_COPY: u32 = 0x1CE0_BC51;
const BCS_EXEC_RESULT_POST_COPY: u32 = 0x1CE0_BC52;
const BCS_EXEC_RESULT_DONE: u32 = 0x1CE0_BC53;
const RCS_PRESENT_RESULT_START_BASE: u32 = 0xC0DE_8F00;
const RCS_PRESENT_RESULT_BASE: u32 = 0xC0DE_9000;
const RCS_PRESENT_MAX_CHUNK_PIXELS: usize = 256;
const INTEL_LEGACY_64B_CONTEXT: u32 = 3;
const GEN8_CTX_VALID: u32 = 1 << 0;
const GEN8_CTX_PRIVILEGE: u32 = 1 << 8;
const GEN12_CTX_PRIORITY_NORMAL: u32 = 1 << 9;
const GEN8_CTX_ADDRESSING_MODE_SHIFT: u32 = 3;
const LRC_STATE_OFFSET_DWORDS: usize = 4096 / core::mem::size_of::<u32>();
const GEN12_CTX_RCS_INDIRECT_CTX_OFFSET_DEFAULT: u32 = 0xD;

const MEDIA_FORCEWAKE_ACK_REGS: [(&str, usize); 8] = [
    ("vdbox0", FORCEWAKE_ACK_VDBOX0),
    ("vdbox1", FORCEWAKE_ACK_VDBOX1),
    ("vdbox2", FORCEWAKE_ACK_VDBOX2),
    ("vdbox3", FORCEWAKE_ACK_VDBOX3),
    ("vebox0", FORCEWAKE_ACK_VEBOX0),
    ("vebox1", FORCEWAKE_ACK_VEBOX1),
    ("vebox2", FORCEWAKE_ACK_VEBOX2),
    ("vebox3", FORCEWAKE_ACK_VEBOX3),
];

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
pub(super) fn ring_ctl_value_for_size(size: usize) -> Option<u32> {
    ring_ctl_value(size)
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

fn log_bcs_regs(warm: Igpu770WarmState, label: &str) {
    crate::log!(
        "intel/igpu770: bcs-regs label={} ctl=0x{:08X} head=0x{:08X} tail=0x{:08X} start=0x{:08X} mi_mode=0x{:08X} mode=0x{:08X} ctx_ctl=0x{:08X} execlist_ctl=0x{:08X} execlist_lo=0x{:08X} execlist_hi=0x{:08X} acthd=0x{:08X} ipeir=0x{:08X} ipehr=0x{:08X} eir=0x{:08X} emr=0x{:08X} instdone=0x{:08X} instps=0x{:08X} bbaddr=0x{:08X} bbaddr_udw=0x{:08X}\n",
        label,
        mmio_read32(warm, BCS_RING_CTL),
        mmio_read32(warm, BCS_RING_HEAD),
        mmio_read32(warm, BCS_RING_TAIL),
        mmio_read32(warm, BCS_RING_START),
        mmio_read32(warm, BCS_RING_MI_MODE),
        mmio_read32(warm, BCS_RING_MODE_GEN7),
        mmio_read32(warm, BCS_RING_CONTEXT_CONTROL),
        mmio_read32(warm, BCS_RING_EXECLIST_CONTROL),
        mmio_read32(warm, BCS_RING_EXECLIST_STATUS_LO),
        mmio_read32(warm, BCS_RING_EXECLIST_STATUS_HI),
        mmio_read32(warm, BCS_RING_ACTHD),
        mmio_read32(warm, BCS_RING_IPEIR),
        mmio_read32(warm, BCS_RING_IPEHR),
        mmio_read32(warm, BCS_RING_EIR),
        mmio_read32(warm, BCS_RING_EMR),
        mmio_read32(warm, BCS_RING_INSTDONE),
        mmio_read32(warm, BCS_RING_INSTPS),
        mmio_read32(warm, BCS_RING_BBADDR),
        mmio_read32(warm, BCS_RING_BBADDR_UDW)
    );
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

fn log_bcs_mode_summary(warm: Igpu770WarmState, label: &str) {
    let mi_mode = mmio_read32(warm, BCS_RING_MI_MODE);
    let mode = mmio_read32(warm, BCS_RING_MODE_GEN7);
    let ctx_ctl = mmio_read32(warm, BCS_RING_CONTEXT_CONTROL);
    let execlist_ctl = mmio_read32(warm, BCS_RING_EXECLIST_CONTROL);
    crate::log!(
        "intel/igpu770: bcs-mode label={} mi_mode=0x{:08X} mode=0x{:08X} ctx_ctl=0x{:08X} execlist_ctl=0x{:08X} mode_idle={} stop_ring={} tlb_invalidate_explicit={} ppgtt_enable={} legacy_disable={}\n",
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
fn read_bcs_result_markers(warm: Igpu770WarmState) -> [u32; 4] {
    let base = warm.result_virt as *const u8;
    let read_slot = |slot_bytes: u64| unsafe {
        core::ptr::read_volatile(base.add(slot_bytes as usize) as *const u32)
    };

    unsafe {
        [
            read_slot(COPY_SMOKE_START_SLOT),
            read_slot(COPY_SMOKE_PRE_COPY_SLOT),
            read_slot(COPY_SMOKE_POST_COPY_SLOT),
            read_slot(COPY_SMOKE_DONE_SLOT),
        ]
    }
}

#[inline]
fn masked_bit_enable(bit: u32) -> u32 {
    bit | (bit << 16)
}

#[inline]
fn masked_bits_update(set_bits: u32, clear_bits: u32) -> u32 {
    set_bits | ((set_bits | clear_bits) << 16)
}

#[inline]
pub(super) fn gen12_lrc_context_control_seed() -> u32 {
    masked_bit_enable(CTX_CTRL_INHIBIT_SYN_CTX_SWITCH | CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT)
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

fn wait_forcewake_domain_ack(
    warm: Igpu770WarmState,
    ack_reg: usize,
    mask: u32,
    expected: u32,
) -> (bool, u32, usize) {
    let mut last = mmio_read32(warm, ack_reg);
    if (last & mask) == expected {
        return (true, last, 0);
    }

    let mut iter = 0usize;
    while iter < FORCEWAKE_POLL_ITERS {
        core::hint::spin_loop();
        last = mmio_read32(warm, ack_reg);
        if (last & mask) == expected {
            return (true, last, iter + 1);
        }
        iter += 1;
    }

    (false, last, FORCEWAKE_POLL_ITERS)
}

fn wait_forcewake_req_latched(
    warm: Igpu770WarmState,
    reg: usize,
    mask: u32,
    expected: u32,
) -> (bool, u32, usize) {
    let mut last = mmio_read32(warm, reg);
    if (last & mask) == expected {
        return (true, last, 0);
    }

    let mut iter = 0usize;
    while iter < FORCEWAKE_POLL_ITERS {
        core::hint::spin_loop();
        last = mmio_read32(warm, reg);
        if (last & mask) == expected {
            return (true, last, iter + 1);
        }
        iter += 1;
    }

    (false, last, FORCEWAKE_POLL_ITERS)
}

fn log_media_forcewake_coverage(warm: Igpu770WarmState, label: &str) -> usize {
    let mut awake = 0usize;
    let mut raw = [0u32; MEDIA_FORCEWAKE_ACK_REGS.len()];

    let mut idx = 0usize;
    while idx < MEDIA_FORCEWAKE_ACK_REGS.len() {
        let value = mmio_read32(warm, MEDIA_FORCEWAKE_ACK_REGS[idx].1);
        if (value & FORCEWAKE_KERNEL) != 0 {
            awake += 1;
        }
        raw[idx] = value;
        idx += 1;
    }

    crate::log!(
        "intel/igpu770: forcewake-media-coverage label={} awake={}/{} vdbox0=0x{:08X} vdbox1=0x{:08X} vdbox2=0x{:08X} vdbox3=0x{:08X} vebox0=0x{:08X} vebox1=0x{:08X} vebox2=0x{:08X} vebox3=0x{:08X}\n",
        label,
        awake,
        MEDIA_FORCEWAKE_ACK_REGS.len(),
        raw[0],
        raw[1],
        raw[2],
        raw[3],
        raw[4],
        raw[5],
        raw[6],
        raw[7]
    );

    awake
}

#[derive(Copy, Clone, Debug)]
pub(super) struct MediaForcewakeRefresh {
    pub req_before: u32,
    pub ack_before: u32,
    pub req_after: u32,
    pub ack_after: u32,
    pub req_latched: bool,
    pub acked: bool,
    pub req_iters: usize,
    pub ack_iters: usize,
    pub awake_count: usize,
}

pub(super) fn forcewake_media_refresh(
    warm: Igpu770WarmState,
    label: &str,
) -> MediaForcewakeRefresh {
    let req_before = mmio_read32(warm, FORCEWAKE_MEDIA_GEN11);
    let ack_before = mmio_read32(warm, FORCEWAKE_ACK_MEDIA);

    let _ = mmio_write32(warm, FORCEWAKE_MEDIA_GEN11, masked_bit_enable(FORCEWAKE_KERNEL));
    let (req_latched, req_after, req_iters) =
        wait_forcewake_req_latched(warm, FORCEWAKE_MEDIA_GEN11, FORCEWAKE_KERNEL, FORCEWAKE_KERNEL);
    let (acked, ack_after, ack_iters) =
        wait_forcewake_domain_ack(warm, FORCEWAKE_ACK_MEDIA, FORCEWAKE_KERNEL, FORCEWAKE_KERNEL);
    let awake_count = log_media_forcewake_coverage(warm, label);

    crate::log!(
        "intel/igpu770: forcewake-media-refresh label={} req_before=0x{:08X} req_after=0x{:08X} ack_before=0x{:08X} ack_after=0x{:08X} req_latched={} acked={} req_iters={} ack_iters={} awake={}/{}\n",
        label,
        req_before,
        req_after,
        ack_before,
        ack_after,
        req_latched as u8,
        acked as u8,
        req_iters,
        ack_iters,
        awake_count,
        MEDIA_FORCEWAKE_ACK_REGS.len()
    );

    MediaForcewakeRefresh {
        req_before,
        ack_before,
        req_after,
        ack_after,
        req_latched,
        acked,
        req_iters,
        ack_iters,
        awake_count,
    }
}

pub(super) fn forcewake_all_acquire(warm: Igpu770WarmState) -> u32 {
    let ack_before = mmio_read32(warm, FORCEWAKE_ACK_RENDER);
    let media_req_before = mmio_read32(warm, FORCEWAKE_MEDIA_GEN11);
    let media_ack_before = mmio_read32(warm, FORCEWAKE_ACK_MEDIA);
    let gt_req_before = mmio_read32(warm, FORCEWAKE_GT_GEN11);
    let gt_ack_before = mmio_read32(warm, FORCEWAKE_ACK_GT);
    crate::log!(
        "intel/igpu770: forcewake-all pre ack=0x{:08X} media_req=0x{:08X} media_ack=0x{:08X} gt_req=0x{:08X} gt_ack=0x{:08X}\n",
        ack_before,
        media_req_before,
        media_ack_before,
        gt_req_before,
        gt_ack_before
    );
    let _ = log_media_forcewake_coverage(warm, "pre");
    let render_awake = (ack_before & FORCEWAKE_KERNEL) != 0;
    let media_awake = (media_ack_before & FORCEWAKE_KERNEL) != 0;
    let gt_awake = (gt_ack_before & FORCEWAKE_KERNEL) != 0;
    if FORCEWAKE_GT_HELD.load(Ordering::Acquire) && render_awake && media_awake && gt_awake {
        crate::log!(
            "intel/igpu770: forcewake-all already-held ack=0x{:08X} media_req=0x{:08X} media_ack=0x{:08X} gt_req=0x{:08X} gt_ack=0x{:08X}\n",
            ack_before,
            media_req_before,
            media_ack_before,
            gt_req_before,
            gt_ack_before
        );
        let _ = log_media_forcewake_coverage(warm, "already-held");
        return ack_before;
    } else if FORCEWAKE_GT_HELD.load(Ordering::Acquire) {
        crate::log!(
            "intel/igpu770: forcewake-all stale-held-state render_awake={} media_awake={} gt_awake={} reissuing-acquire\n",
            render_awake as u8,
            media_awake as u8,
            gt_awake as u8
        );
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

    let _ = mmio_write32(warm, FORCEWAKE_MEDIA_GEN11, masked_bit_enable(FORCEWAKE_KERNEL));
    let (_, media_req, media_req_iters) =
        wait_forcewake_req_latched(warm, FORCEWAKE_MEDIA_GEN11, FORCEWAKE_KERNEL, FORCEWAKE_KERNEL);
    let (media_ok, media_ack, media_ack_iters) =
        wait_forcewake_domain_ack(warm, FORCEWAKE_ACK_MEDIA, FORCEWAKE_KERNEL, FORCEWAKE_KERNEL);
    let _ = mmio_write32(warm, FORCEWAKE_GT_GEN11, masked_bit_enable(FORCEWAKE_KERNEL));
    let (_, gt_req, gt_req_iters) =
        wait_forcewake_req_latched(warm, FORCEWAKE_GT_GEN11, FORCEWAKE_KERNEL, FORCEWAKE_KERNEL);
    let (gt_ok, gt_ack, gt_ack_iters) =
        wait_forcewake_domain_ack(warm, FORCEWAKE_ACK_GT, FORCEWAKE_KERNEL, FORCEWAKE_KERNEL);

    crate::log!(
        "intel/igpu770: forcewake-all acquire req=0x{:08X} ack=0x{:08X} cleared=0x{:08X} clear_iters={} iters={} fallback={} media_req=0x{:08X} media_req_iters={} media_ack=0x{:08X} media_ack_iters={} media_ok={} gt_req=0x{:08X} gt_req_iters={} gt_ack=0x{:08X} gt_ack_iters={} gt_ok={}\n",
        FORCEWAKE_KERNEL,
        ack,
        ack_after_clear,
        clear_iters,
        iter,
        fallback_used as u8,
        media_req,
        media_req_iters,
        media_ack,
        media_ack_iters,
        media_ok as u8,
        gt_req,
        gt_req_iters,
        gt_ack,
        gt_ack_iters,
        gt_ok as u8
    );
    let media_coverage_awake = log_media_forcewake_coverage(warm, "post-acquire");
    crate::log!(
        "intel/igpu770: forcewake-all coverage render_ack={} media_ack={} gt_ack={} media_domains_awake={}\n",
        ((ack & FORCEWAKE_KERNEL) != 0) as u8,
        ((media_ack & FORCEWAKE_KERNEL) != 0) as u8,
        ((gt_ack & FORCEWAKE_KERNEL) != 0) as u8,
        media_coverage_awake
    );
    intel_770_registers::log_engine_wakeup_table("post-forcewake", |off| mmio_read32(warm, off));
    if set_ok && gt_ok {
        if !media_ok {
            crate::log!(
                "intel/igpu770: forcewake-all partial-hold render_gt=1 media=0; caching render/gt wake to avoid repeated media timeout retries\n"
            );
        }
        FORCEWAKE_GT_HELD.store(true, Ordering::Release);
    }
    ack
}

pub(super) fn forcewake_gt_acquire(warm: Igpu770WarmState) -> u32 {
    forcewake_all_acquire(warm)
}

fn forcewake_gt_release(warm: Igpu770WarmState) -> u32 {
    let _ = mmio_write32(
        warm,
        FORCEWAKE_RENDER_GEN11,
        masked_bit_disable(FORCEWAKE_KERNEL | FORCEWAKE_KERNEL_FALLBACK),
    );
    let (_, ack, _) = wait_forcewake_ack(warm, FORCEWAKE_KERNEL | FORCEWAKE_KERNEL_FALLBACK, 0);
    let _ = mmio_write32(warm, FORCEWAKE_MEDIA_GEN11, masked_bit_disable(FORCEWAKE_KERNEL));
    let _ = wait_forcewake_domain_ack(warm, FORCEWAKE_ACK_MEDIA, FORCEWAKE_KERNEL, 0);
    let _ = mmio_write32(warm, FORCEWAKE_GT_GEN11, masked_bit_disable(FORCEWAKE_KERNEL));
    let _ = wait_forcewake_domain_ack(warm, FORCEWAKE_ACK_GT, FORCEWAKE_KERNEL, 0);
    FORCEWAKE_GT_HELD.store(false, Ordering::Release);
    crate::log!(
        "intel/igpu770: forcewake-all release ack=0x{:08X} media_ack=0x{:08X} gt_ack=0x{:08X}\n",
        ack,
        mmio_read32(warm, FORCEWAKE_ACK_MEDIA),
        mmio_read32(warm, FORCEWAKE_ACK_GT)
    );
    let _ = log_media_forcewake_coverage(warm, "release");
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

    (mmio_read32(warm, GEN9_CLKGATE_DIS_5), mmio_read32(warm, GEN8_CHICKEN_DCPR_1))
}

pub(super) fn request_display_power_with_forcewake(warm: Igpu770WarmState) -> bool {
    let _forcewake_ack = forcewake_all_acquire(warm);
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
    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let dwords =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let batch_tail_bytes = match super::xelp_render_ngin::encode_rgb_triangle_store_batch(
        dwords,
        fill.dst_gpu_addr,
        fill.pitch,
        fill.rect_w,
        fill.rect_h,
        GPU_VA_RESULT_BASE,
        RCS_EXEC_RESULT_DONE,
    ) {
        Ok(bytes) => bytes,
        Err(err) => {
            crate::log!(
                "intel/igpu770: rcs-triangle-batch build-failed err={} rect={}x{} pitch=0x{:X}\n",
                err,
                fill.rect_w,
                fill.rect_h,
                fill.pitch
            );
            0
        }
    };
    dma_cache_flush(warm.batch_virt as *const u8, batch_tail_bytes);
}

fn build_bcs_store_pixels_batch(
    batch_dwords: &mut [u32],
    fill: BltFillRectPlan,
) -> Result<usize, &'static str> {
    const RESERVED_END_DWORDS: usize = 2;
    const STORE_DWORDS: usize = 4;
    const RESULT_MARKER_STORES: usize = 4;
    const RECT_W: usize = 80;
    const RECT_H: usize = 40;

    if batch_dwords.len() <= RESERVED_END_DWORDS + 12 {
        return Err("batch-too-small");
    }
    if fill.rect_w < RECT_W || fill.rect_h < RECT_H {
        return Err("shape-too-small");
    }

    batch_dwords.fill(0);
    let mut i = 0usize;
    let writable_limit = batch_dwords.len().saturating_sub(RESERVED_END_DWORDS);
    let pitch = fill.pitch as u64;
    let cmd = xelp_copy_ngin::mi::STORE_DATA_IMM
        | xelp_copy_ngin::mi::SDI_GGTT
        | xelp_copy_ngin::mi::sdi_num_dw(1);

    let emit_store = |batch_dwords: &mut [u32], i: &mut usize, dst_gpu: u64, value: u32| {
        if *i + STORE_DWORDS > writable_limit {
            return false;
        }
        batch_dwords[*i] = cmd;
        batch_dwords[*i + 1] = dst_gpu as u32;
        batch_dwords[*i + 2] = (dst_gpu >> 32) as u32;
        batch_dwords[*i + 3] = value;
        *i += STORE_DWORDS;
        true
    };

    batch_dwords[i] = MI_NOOP;
    i += 1;

    if !emit_store(batch_dwords, &mut i, GPU_VA_RESULT_BASE, BCS_EXEC_RESULT_START) {
        return Err("result-start");
    }
    if !emit_store(
        batch_dwords,
        &mut i,
        GPU_VA_RESULT_BASE + COPY_SMOKE_PRE_COPY_SLOT,
        BCS_EXEC_RESULT_PRE_COPY,
    ) {
        return Err("result-pre");
    }

    let remaining_store_slots = writable_limit
        .saturating_sub(i)
        .saturating_sub(RESULT_MARKER_STORES * STORE_DWORDS)
        / STORE_DWORDS;
    let border_pixels = (RECT_W * 2).saturating_add(RECT_H.saturating_sub(2) * 2);
    let center_dot_pixels = 4usize;
    if remaining_store_slots < border_pixels.saturating_add(center_dot_pixels) {
        return Err("shape-capacity");
    }

    let center_x = fill.rect_w / 2;
    let center_y = fill.rect_h / 2;
    let rect_x0 = center_x.saturating_sub(RECT_W / 2);
    let rect_y0 = center_y.saturating_sub(RECT_H / 2);
    let rect_x1 = rect_x0.saturating_add(RECT_W.saturating_sub(1));
    let rect_y1 = rect_y0.saturating_add(RECT_H.saturating_sub(1));

    for x in rect_x0..=rect_x1 {
        let top_dst = fill
            .dst_gpu_addr
            .saturating_add((rect_y0 as u64).saturating_mul(pitch))
            .saturating_add((x as u64) * 4);
        if !emit_store(batch_dwords, &mut i, top_dst, fill.color) {
            return Err("rect-top");
        }

        let bot_dst = fill
            .dst_gpu_addr
            .saturating_add((rect_y1 as u64).saturating_mul(pitch))
            .saturating_add((x as u64) * 4);
        if !emit_store(batch_dwords, &mut i, bot_dst, 0x0000_FFFF) {
            return Err("rect-bottom");
        }
    }

    for y in rect_y0.saturating_add(1)..rect_y1 {
        let left_dst = fill
            .dst_gpu_addr
            .saturating_add((y as u64).saturating_mul(pitch))
            .saturating_add((rect_x0 as u64) * 4);
        if !emit_store(batch_dwords, &mut i, left_dst, fill.color) {
            return Err("rect-left");
        }

        let right_dst = fill
            .dst_gpu_addr
            .saturating_add((y as u64).saturating_mul(pitch))
            .saturating_add((rect_x1 as u64) * 4);
        if !emit_store(batch_dwords, &mut i, right_dst, 0x0000_FFFF) {
            return Err("rect-right");
        }
    }

    for dy in 0..2usize {
        for dx in 0..2usize {
            let dot_dst = fill
                .dst_gpu_addr
                .saturating_add(((center_y.saturating_add(dy)) as u64).saturating_mul(pitch))
                .saturating_add(((center_x.saturating_add(dx)) as u64) * 4);
            if !emit_store(batch_dwords, &mut i, dot_dst, 0x0000_FFFF) {
                return Err("rect-center-dot");
            }
        }
    }

    if !emit_store(
        batch_dwords,
        &mut i,
        GPU_VA_RESULT_BASE + COPY_SMOKE_POST_COPY_SLOT,
        BCS_EXEC_RESULT_POST_COPY,
    ) {
        return Err("result-post");
    }
    if !emit_store(
        batch_dwords,
        &mut i,
        GPU_VA_RESULT_BASE + COPY_SMOKE_DONE_SLOT,
        BCS_EXEC_RESULT_DONE,
    ) {
        return Err("result-done");
    }

    batch_dwords[i] = MI_BATCH_BUFFER_END;
    batch_dwords[i + 1] = MI_NOOP;
    i += 2;

    Ok(i.saturating_mul(core::mem::size_of::<u32>()))
}

fn build_ring_batch_start(warm: Igpu770WarmState, batch_gpu_addr: u64) -> usize {
    let dwords =
        unsafe { core::slice::from_raw_parts_mut(warm.ring_virt as *mut u32, BLT_RING_DWORDS) };

    dwords[0] = MI_BATCH_BUFFER_START_GEN8 | MI_BATCH_NON_SECURE_I965;
    dwords[1] = batch_gpu_addr as u32;
    dwords[2] = (batch_gpu_addr >> 32) as u32;
    dwords[3] = MI_NOOP;

    dma_cache_flush(warm.ring_virt as *const u8, BLT_RING_TAIL_BYTES as usize);
    BLT_RING_TAIL_BYTES as usize
}

pub(super) fn build_ring_batch_start_words(
    ring_virt: *mut u8,
    ring_len: usize,
    batch_gpu_addr: u64,
) -> Option<usize> {
    if ring_virt.is_null() || ring_len < BLT_RING_DWORDS * core::mem::size_of::<u32>() {
        return None;
    }

    let dwords = unsafe { core::slice::from_raw_parts_mut(ring_virt as *mut u32, BLT_RING_DWORDS) };
    dwords[0] = MI_BATCH_BUFFER_START_GEN8 | MI_BATCH_NON_SECURE_I965;
    dwords[1] = batch_gpu_addr as u32;
    dwords[2] = (batch_gpu_addr >> 32) as u32;
    dwords[3] = MI_NOOP;
    dma_cache_flush(ring_virt as *const u8, BLT_RING_TAIL_BYTES as usize);
    Some(BLT_RING_TAIL_BYTES as usize)
}

fn build_bcs_ring_batch_start(warm: Igpu770WarmState, batch_gpu_addr: u64) -> usize {
    let dwords = unsafe {
        core::slice::from_raw_parts_mut(warm.ring_virt as *mut u32, BCS_RING_MARKER_DWORDS)
    };

    dwords[0] = MI_STORE_DWORD_IMM_GEN4_LEN_DW4;
    dwords[1] = GPU_VA_RESULT_BASE as u32;
    dwords[2] = (GPU_VA_RESULT_BASE >> 32) as u32;
    dwords[3] = BCS_EXEC_RESULT_START;
    dwords[4] = MI_BATCH_BUFFER_START_GEN8;
    dwords[5] = batch_gpu_addr as u32;
    dwords[6] = (batch_gpu_addr >> 32) as u32;
    dwords[7] = MI_NOOP;

    dma_cache_flush(warm.ring_virt as *const u8, BCS_RING_MARKER_TAIL_BYTES as usize);
    BCS_RING_MARKER_TAIL_BYTES as usize
}

#[inline]
fn mi_lri_num_regs(num_regs: u32) -> u32 {
    num_regs.saturating_mul(2).saturating_sub(1)
}

#[inline]
fn mi_lri_cmd(num_regs: u32, flags: u32) -> u32 {
    MI_LOAD_REGISTER_IMM | MI_LRI_CS_MMIO | flags | mi_lri_num_regs(num_regs)
}

#[inline]
fn push_mi_nops(state: &mut [u32], idx: &mut usize, count: usize) {
    for _ in 0..count {
        state[*idx] = MI_NOOP;
        *idx += 1;
    }
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

    dma_cache_flush(warm.context_virt as *const u8, warm.context_len);
    true
}

fn init_gen12_bcs_context_image(
    warm: Igpu770WarmState,
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
    if state.len() < 112 {
        return false;
    }

    let mut idx = 0usize;
    state[idx] = MI_NOOP;
    idx += 1;

    state[idx] = mi_lri_cmd(14, MI_LRI_FORCE_POSTED);
    idx += 1;
    state[idx] = 0x22244;
    state[idx + 1] = 0x0009_0009;
    state[idx + 2] = 0x22034;
    state[idx + 3] = 0;
    state[idx + 4] = 0x22030;
    state[idx + 5] = ring_tail;
    state[idx + 6] = 0x22038;
    state[idx + 7] = ring_start;
    state[idx + 8] = 0x2203C;
    state[idx + 9] = ring_ctl;
    state[idx + 10] = 0x22168;
    state[idx + 11] = 0;
    state[idx + 12] = 0x22140;
    state[idx + 13] = 0;
    state[idx + 14] = 0x22110;
    state[idx + 15] = 0;
    state[idx + 16] = 0x2211C;
    state[idx + 17] = 0;
    state[idx + 18] = 0x22114;
    state[idx + 19] = 0;
    state[idx + 20] = 0x22118;
    state[idx + 21] = 0;
    state[idx + 22] = 0x221C0;
    state[idx + 23] = 0;
    state[idx + 24] = 0x221C4;
    state[idx + 25] = 0;
    state[idx + 26] = 0x221C8;
    state[idx + 27] = 0;
    idx += 28;

    push_mi_nops(state, &mut idx, 3);

    state[idx] = mi_lri_cmd(9, MI_LRI_FORCE_POSTED);
    idx += 1;
    state[idx] = 0x223A8;
    state[idx + 1] = 0;
    state[idx + 2] = 0x2228C;
    state[idx + 3] = 0;
    state[idx + 4] = 0x22288;
    state[idx + 5] = 0;
    state[idx + 6] = 0x22284;
    state[idx + 7] = 0;
    state[idx + 8] = 0x22280;
    state[idx + 9] = 0;
    state[idx + 10] = 0x2227C;
    state[idx + 11] = 0;
    state[idx + 12] = 0x22278;
    state[idx + 13] = 0;
    state[idx + 14] = 0x22274;
    state[idx + 15] = 0;
    state[idx + 16] = 0x22270;
    state[idx + 17] = 0;
    idx += 18;

    push_mi_nops(state, &mut idx, 13);

    state[idx] = mi_lri_cmd(1, 0);
    idx += 1;
    state[idx] = 0x22200;
    state[idx + 1] = 0;
    idx += 2;

    push_mi_nops(state, &mut idx, 12);

    state[idx] = MI_BATCH_BUFFER_END | 1;

    dma_cache_flush(warm.context_virt as *const u8, warm.context_len);
    true
}

pub(super) fn init_gen12_video_context_image(
    context_virt: *mut u8,
    context_len: usize,
    ring_base: usize,
    ring_start: u32,
    ring_tail: u32,
    ring_ctl: u32,
    hws_pga: u32,
) -> bool {
    if context_virt.is_null() {
        return false;
    }

    let total_dwords = context_len / core::mem::size_of::<u32>();
    if total_dwords <= LRC_STATE_OFFSET_DWORDS {
        return false;
    }

    let dwords = unsafe { core::slice::from_raw_parts_mut(context_virt as *mut u32, total_dwords) };
    dwords.fill(0);

    let state = &mut dwords[LRC_STATE_OFFSET_DWORDS..];
    if state.len() < 96 {
        return false;
    }

    let ring_base = ring_base as u32;
    let mut idx = 0usize;
    state[idx] = MI_NOOP;
    idx += 1;

    state[idx] = mi_lri_cmd(15, MI_LRI_FORCE_POSTED);
    idx += 1;
    let ctx_ctl_seed = gen12_lrc_context_control_seed();
    state[idx] = ring_base + 0x244;
    state[idx + 1] = ctx_ctl_seed;
    state[idx + 2] = ring_base + 0x34;
    state[idx + 3] = 0;
    state[idx + 4] = ring_base + 0x30;
    state[idx + 5] = ring_tail;
    state[idx + 6] = ring_base + 0x38;
    state[idx + 7] = ring_start;
    state[idx + 8] = ring_base + 0x3C;
    state[idx + 9] = ring_ctl;
    state[idx + 10] = ring_base + 0x9C;
    state[idx + 11] = STOP_RING << 16;
    state[idx + 12] = ring_base + 0x80;
    state[idx + 13] = hws_pga;
    state[idx + 14] = ring_base + 0x168;
    state[idx + 15] = 0;
    state[idx + 16] = ring_base + 0x140;
    state[idx + 17] = 0;
    state[idx + 18] = ring_base + 0x110;
    state[idx + 19] = 0;
    state[idx + 20] = ring_base + 0x1C0;
    state[idx + 21] = 0;
    state[idx + 22] = ring_base + 0x1C4;
    state[idx + 23] = 0;
    state[idx + 24] = ring_base + 0x1C8;
    state[idx + 25] = 0;
    state[idx + 26] = ring_base + 0x180;
    state[idx + 27] = 0;
    state[idx + 28] = ring_base + 0x2B4;
    state[idx + 29] = 0;
    idx += 30;

    push_mi_nops(state, &mut idx, 5);

    state[idx] = mi_lri_cmd(9, MI_LRI_FORCE_POSTED);
    idx += 1;
    state[idx] = ring_base + 0x3A8;
    state[idx + 1] = 0;
    state[idx + 2] = ring_base + 0x28C;
    state[idx + 3] = 0;
    state[idx + 4] = ring_base + 0x288;
    state[idx + 5] = 0;
    state[idx + 6] = ring_base + 0x284;
    state[idx + 7] = 0;
    state[idx + 8] = ring_base + 0x280;
    state[idx + 9] = 0;
    state[idx + 10] = ring_base + 0x27C;
    state[idx + 11] = 0;
    state[idx + 12] = ring_base + 0x278;
    state[idx + 13] = 0;
    state[idx + 14] = ring_base + 0x274;
    state[idx + 15] = 0;
    state[idx + 16] = ring_base + 0x270;
    state[idx + 17] = 0;
    idx += 18;

    push_mi_nops(state, &mut idx, 13);

    state[idx] = mi_lri_cmd(1, 0);
    idx += 1;
    state[idx] = ring_base + 0x200;
    state[idx + 1] = 0;
    idx += 2;

    push_mi_nops(state, &mut idx, 12);

    state[CTX_CONTEXT_CONTROL_DW] = ctx_ctl_seed;
    state[CTX_RING_HEAD_DW] = 0;
    state[CTX_RING_TAIL_DW] = ring_tail;
    state[CTX_RING_START_DW] = ring_start;
    state[CTX_RING_CTL_DW] = ring_ctl;
    state[CTX_RING_MI_MODE_DW] = STOP_RING << 16;

    state[idx] = MI_BATCH_BUFFER_END | 1;
    dma_cache_flush(context_virt as *const u8, context_len);
    true
}

#[inline]
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

#[inline]
pub(super) fn build_execlist_context_descriptor_for_gpu_addr(context_gpu_addr: u64) -> (u32, u32) {
    build_execlist_context_descriptor(context_gpu_addr)
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

fn bcs_execlist_submit_port_push(
    warm: Igpu770WarmState,
    context0_lo: u32,
    context0_hi: u32,
    context1_lo: u32,
    context1_hi: u32,
) {
    let _ = mmio_write32(warm, BCS_RING_EXECLIST_SUBMIT_PORT, context0_lo);
    let _ = mmio_write32(warm, BCS_RING_EXECLIST_SUBMIT_PORT, context0_hi);
    let _ = mmio_write32(warm, BCS_RING_EXECLIST_SUBMIT_PORT, context1_lo);
    let _ = mmio_write32(warm, BCS_RING_EXECLIST_SUBMIT_PORT, context1_hi);
}

#[derive(Copy, Clone, Debug)]
pub struct Igpu770WarmState {
    pub device_id: u16,
    pub revision_id: u8,
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
    pub guc_ads_phys: u64,
    pub guc_ads_virt: *mut u8,
    pub guc_ads_len: usize,
    pub guc_ads_gpu_addr: u64,
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
static GGTT_BCS_SMOKE_RAN: AtomicBool = AtomicBool::new(false);
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

    let fb = crate::limine::framebuffer_response()?
        .framebuffers()
        .next()?;
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
    dst_x: usize,
    dst_y: usize,
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

fn ggtt_map_plan_aperture_backed(
    phys: u64,
    size: usize,
    aperture_base: u64,
) -> Option<GgttMapPlan> {
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
    let log_all = plan.pages <= GGTT_MAP_LOG_ALL_THRESHOLD;
    let mut omitted_logged = false;
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
        if log_all
            || page < GGTT_MAP_LOG_EDGE_PAGES
            || page >= plan.pages.saturating_sub(GGTT_MAP_LOG_EDGE_PAGES)
        {
            crate::log!(
                "intel/igpu770: ggtt-map label={} page={} gpu=0x{:X} phys=0x{:X} pte=0x{:016X}\n",
                label,
                page,
                gpu_addr,
                phys,
                readback
            );
        } else if !omitted_logged {
            let omitted = plan
                .pages
                .saturating_sub(GGTT_MAP_LOG_EDGE_PAGES.saturating_mul(2));
            crate::log!(
                "intel/igpu770: ggtt-map label={} omitted_pages={} total_pages={} log_policy=head-tail edge_pages={}\n",
                label,
                omitted,
                plan.pages,
                GGTT_MAP_LOG_EDGE_PAGES
            );
            omitted_logged = true;
        }
        page += 1;
    }
    true
}

pub(super) fn ggtt_map_system_ram_range(
    label: &str,
    warm: Igpu770WarmState,
    phys: u64,
    size: usize,
    gpu_addr: u64,
) -> bool {
    let Some(plan) = ggtt_map_plan_system_ram(phys, size, gpu_addr) else {
        crate::log!(
            "intel/igpu770: ggtt-map label={} status=plan-failed gpu=0x{:X} phys=0x{:X} size=0x{:X}\n",
            label,
            gpu_addr,
            phys,
            size
        );
        return false;
    };
    log_ggtt_map_plan(label, plan);
    ggtt_program_plan(label, warm, plan)
}

#[inline]
pub(crate) fn dma_cache_flush_range(ptr: *const u8, len: usize) {
    dma_cache_flush(ptr, len);
}

fn fb_fill_rect_plan(
    warm: Igpu770WarmState,
    rect_w_limit: usize,
    rect_h_limit: usize,
    color: u32,
    centered: bool,
) -> Option<BltFillRectPlan> {
    let bytes_per_pixel = warm.limine_fb_bpp.checked_div(8)?;
    if bytes_per_pixel == 0 {
        return None;
    }

    let rect_w = rect_w_limit.min(warm.limine_fb_width.max(1));
    let rect_h = rect_h_limit.min(warm.limine_fb_height.max(1));
    let dst = ggtt_map_plan_aperture_backed(
        warm.limine_fb_phys,
        warm.limine_fb_size,
        warm.aperture_bar_phys,
    )?;
    let dst_x = if centered {
        warm.limine_fb_width.saturating_sub(rect_w) / 2
    } else {
        0
    };
    let dst_y = if centered {
        warm.limine_fb_height.saturating_sub(rect_h) / 2
    } else {
        0
    };
    let dst_byte_off = dst_y
        .checked_mul(warm.limine_fb_pitch)?
        .checked_add(dst_x.checked_mul(bytes_per_pixel)?)?;
    Some(BltFillRectPlan {
        dst_gpu_addr: dst.gpu_addr.checked_add(dst_byte_off as u64)?,
        dst_phys: warm.limine_fb_phys.checked_add(dst_byte_off as u64)?,
        dst_x,
        dst_y,
        rect_w,
        rect_h,
        pitch: warm.limine_fb_pitch,
        color,
    })
}

#[inline]
fn framebuffer_byte_offset(warm: Igpu770WarmState, x: usize, y: usize) -> Option<usize> {
    let bytes_per_pixel = warm.limine_fb_bpp.checked_div(8)?;
    if bytes_per_pixel < core::mem::size_of::<u32>()
        || x >= warm.limine_fb_width
        || y >= warm.limine_fb_height
    {
        return None;
    }

    y.checked_mul(warm.limine_fb_pitch)?
        .checked_add(x.checked_mul(bytes_per_pixel)?)
        .filter(|off| off.saturating_add(core::mem::size_of::<u32>()) <= warm.limine_fb_size)
}

#[inline]
fn framebuffer_word_at(warm: Igpu770WarmState, x: usize, y: usize) -> Option<u32> {
    let off = framebuffer_byte_offset(warm, x, y)?;
    let ptr = warm.limine_fb_virt.checked_add(off)? as *const u32;
    Some(unsafe { core::ptr::read_volatile(ptr) })
}

fn log_framebuffer_probe(warm: Igpu770WarmState, label: &str, x: usize, y: usize) {
    let Some(off) = framebuffer_byte_offset(warm, x, y) else {
        crate::log!("intel/igpu770: fb-probe label={} xy={}x{} status=out-of-range\n", label, x, y);
        return;
    };
    let Some(value) = framebuffer_word_at(warm, x, y) else {
        crate::log!(
            "intel/igpu770: fb-probe label={} xy={}x{} off=0x{:X} status=unreadable\n",
            label,
            x,
            y,
            off
        );
        return;
    };

    crate::log!(
        "intel/igpu770: fb-probe label={} xy={}x{} off=0x{:X} phys=0x{:X} value=0x{:08X}\n",
        label,
        x,
        y,
        off,
        warm.limine_fb_phys.saturating_add(off as u64),
        value
    );
}

fn log_rcs_triangle_probe_set(warm: Igpu770WarmState, prefix: &str, fill: BltFillRectPlan) {
    let center_x = fill.dst_x.saturating_add(fill.rect_w / 2);
    let center_y = fill.dst_y.saturating_add(fill.rect_h / 2);
    let apex_x = center_x;
    let apex_y = fill.dst_y.saturating_add(fill.rect_h / 2).saturating_sub(9);
    let left_x = center_x.saturating_sub(10);
    let left_y = center_y.saturating_add(6);
    let right_x = center_x
        .saturating_add(10)
        .min(warm.limine_fb_width.saturating_sub(1));
    let right_y = left_y.min(warm.limine_fb_height.saturating_sub(1));

    let mut label = [0u8; 32];

    let apex = format_probe_label(&mut label, prefix, "apex");
    log_framebuffer_probe(warm, apex, apex_x, apex_y);

    let left = format_probe_label(&mut label, prefix, "left");
    log_framebuffer_probe(warm, left, left_x, left_y);

    let right = format_probe_label(&mut label, prefix, "right");
    log_framebuffer_probe(warm, right, right_x, right_y);

    let center = format_probe_label(&mut label, prefix, "center");
    log_framebuffer_probe(warm, center, center_x, center_y);
}

fn format_probe_label<'a>(buf: &'a mut [u8; 32], prefix: &str, suffix: &str) -> &'a str {
    let mut idx = 0usize;
    for b in prefix.as_bytes().iter().copied() {
        if idx >= buf.len() {
            break;
        }
        buf[idx] = b;
        idx += 1;
    }
    if idx < buf.len() {
        buf[idx] = b'-';
        idx += 1;
    }
    for b in suffix.as_bytes().iter().copied() {
        if idx >= buf.len() {
            break;
        }
        buf[idx] = b;
        idx += 1;
    }
    core::str::from_utf8(&buf[..idx]).unwrap_or("probe")
}

fn invalidate_framebuffer_probe(warm: Igpu770WarmState, x: usize, y: usize) {
    let Some(off) = framebuffer_byte_offset(warm, x, y) else {
        return;
    };
    let ptr = warm.limine_fb_virt.saturating_add(off) as *const u8;
    dma_cache_flush(ptr, core::mem::size_of::<u32>());
}

fn cpu_framebuffer_fill_rect(
    warm: Igpu770WarmState,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    color: u32,
) -> bool {
    if warm.limine_fb_virt == 0 || warm.limine_fb_pitch == 0 || warm.limine_fb_bpp < 32 {
        return false;
    }

    let max_w = warm.limine_fb_width.saturating_sub(x);
    let max_h = warm.limine_fb_height.saturating_sub(y);
    let width = width.min(max_w);
    let height = height.min(max_h);
    if width == 0 || height == 0 {
        return false;
    }

    let mut row = 0usize;
    while row < height {
        let Some(row_off) = framebuffer_byte_offset(warm, x, y.saturating_add(row)) else {
            return false;
        };
        let row_ptr = warm.limine_fb_virt.saturating_add(row_off) as *mut u32;
        let mut col = 0usize;
        while col < width {
            unsafe { core::ptr::write_volatile(row_ptr.add(col), color) };
            col += 1;
        }
        row += 1;
    }

    dma_cache_flush(warm.limine_fb_virt as *const u8, warm.limine_fb_size);
    true
}

pub fn cpu_framebuffer_alive_stamp(label: &str) {
    let Some(warm) = warm_state() else {
        crate::log!("intel/igpu770: cpu-fb-stamp label={} status=not-warmed\n", label);
        return;
    };

    let marker_w = 96usize.min(warm.limine_fb_width.max(1));
    let marker_h = 96usize.min(warm.limine_fb_height.max(1));
    let br_x = warm.limine_fb_width.saturating_sub(marker_w);
    let br_y = warm.limine_fb_height.saturating_sub(marker_h);

    let tl_ok = cpu_framebuffer_fill_rect(warm, 0, 0, marker_w, marker_h, 0x00FF_2A00);
    let br_ok = cpu_framebuffer_fill_rect(warm, br_x, br_y, marker_w, marker_h, 0x00FF_FFFF);

    crate::log!(
        "intel/igpu770: cpu-fb-stamp label={} tl={} br={} dims={}x{} pitch=0x{:X}\n",
        label,
        tl_ok as u8,
        br_ok as u8,
        warm.limine_fb_width,
        warm.limine_fb_height,
        warm.limine_fb_pitch
    );
    log_framebuffer_probe(warm, "cpu-stamp-tl", 0, 0);
    log_framebuffer_probe(warm, "cpu-stamp-br", br_x, br_y);
}

pub fn cpu_framebuffer_visualize_bytes_center(
    label: &str,
    bytes: &[u8],
    width: usize,
    height: usize,
) {
    let Some(warm) = warm_state() else {
        crate::log!("intel/igpu770: cpu-fb-visualize label={} status=not-warmed\n", label);
        return;
    };
    if bytes.is_empty() {
        crate::log!("intel/igpu770: cpu-fb-visualize label={} status=empty-bytes\n", label);
        return;
    }
    if warm.limine_fb_virt == 0 || warm.limine_fb_pitch == 0 || warm.limine_fb_bpp < 32 {
        crate::log!("intel/igpu770: cpu-fb-visualize label={} status=fb-unavailable\n", label);
        return;
    }

    let outer_w = width.min(warm.limine_fb_width.max(1)).max(4);
    let outer_h = height.min(warm.limine_fb_height.max(1)).max(4);
    let origin_x = warm.limine_fb_width.saturating_sub(outer_w) / 2;
    let origin_y = warm.limine_fb_height.saturating_sub(outer_h) / 2;
    let inner_x = origin_x.saturating_add(1);
    let inner_y = origin_y.saturating_add(1);
    let inner_w = outer_w.saturating_sub(2).max(1);
    let inner_h = outer_h.saturating_sub(2).max(1);

    if !cpu_framebuffer_fill_rect(warm, origin_x, origin_y, outer_w, outer_h, 0x00F4_F8FF) {
        crate::log!("intel/igpu770: cpu-fb-visualize label={} status=outer-fill-failed\n", label);
        return;
    }
    let _ = cpu_framebuffer_fill_rect(warm, inner_x, inner_y, inner_w, inner_h, 0x0000_0000);

    let mut y = 0usize;
    while y < inner_h {
        let Some(row_off) = framebuffer_byte_offset(warm, inner_x, inner_y.saturating_add(y))
        else {
            crate::log!(
                "intel/igpu770: cpu-fb-visualize label={} status=row-off-failed row={}\n",
                label,
                y
            );
            return;
        };
        let row_ptr = warm.limine_fb_virt.saturating_add(row_off) as *mut u32;
        let mut x = 0usize;
        while x < inner_w {
            let idx = y.saturating_mul(inner_w).saturating_add(x);
            let src0 = bytes[idx % bytes.len()];
            let src1 = bytes[(idx.saturating_mul(17).saturating_add(src0 as usize)) % bytes.len()];
            let lum = src0 ^ src1.rotate_left(1);
            let rgb = ((lum as u32) << 16) | ((lum as u32) << 8) | (lum as u32);
            unsafe { core::ptr::write_volatile(row_ptr.add(x), rgb) };
            x += 1;
        }
        y += 1;
    }

    dma_cache_flush(warm.limine_fb_virt as *const u8, warm.limine_fb_size);
    crate::log!(
        "intel/igpu770: cpu-fb-visualize label={} origin={}x{} dims={}x{} bytes={}\n",
        label,
        origin_x,
        origin_y,
        outer_w,
        outer_h,
        bytes.len()
    );
    log_framebuffer_probe(
        warm,
        "cpu-visualize-center",
        origin_x.saturating_add(outer_w / 2),
        origin_y.saturating_add(outer_h / 2),
    );
}

#[inline]
fn clamp_u8_i32(value: i32) -> u8 {
    value.clamp(0, 255) as u8
}

pub fn cpu_framebuffer_visualize_nv12_center(
    label: &str,
    bytes: &[u8],
    frame_width: usize,
    frame_height: usize,
    pitch: usize,
) {
    let Some(warm) = warm_state() else {
        crate::log!("intel/igpu770: cpu-fb-nv12 label={} status=not-warmed\n", label);
        return;
    };
    if frame_width == 0 || frame_height == 0 || pitch < frame_width {
        crate::log!(
            "intel/igpu770: cpu-fb-nv12 label={} status=bad-dims dims={}x{} pitch={}\n",
            label,
            frame_width,
            frame_height,
            pitch
        );
        return;
    }
    let uv_plane_off = pitch.saturating_mul(frame_height);
    let needed = uv_plane_off.saturating_add((pitch.saturating_mul(frame_height)) / 2);
    if bytes.len() < needed {
        crate::log!(
            "intel/igpu770: cpu-fb-nv12 label={} status=short-surface bytes={} need={}\n",
            label,
            bytes.len(),
            needed
        );
        return;
    }
    if warm.limine_fb_virt == 0 || warm.limine_fb_pitch == 0 || warm.limine_fb_bpp < 32 {
        crate::log!("intel/igpu770: cpu-fb-nv12 label={} status=fb-unavailable\n", label);
        return;
    }

    let scale_x = frame_width.div_ceil(warm.limine_fb_width.max(1));
    let scale_y = frame_height.div_ceil(warm.limine_fb_height.max(1));
    let scale = scale_x.max(scale_y).max(1);
    let outer_w = (frame_width / scale)
        .max(4)
        .min(warm.limine_fb_width.max(1));
    let outer_h = (frame_height / scale)
        .max(4)
        .min(warm.limine_fb_height.max(1));
    let origin_x = warm.limine_fb_width.saturating_sub(outer_w) / 2;
    let origin_y = warm.limine_fb_height.saturating_sub(outer_h) / 2;

    if !cpu_framebuffer_fill_rect(warm, origin_x, origin_y, outer_w, outer_h, 0x0000_0000) {
        crate::log!("intel/igpu770: cpu-fb-nv12 label={} status=outer-fill-failed\n", label);
        return;
    }

    let y_plane = &bytes[..uv_plane_off];
    let uv_plane = &bytes[uv_plane_off..];
    let mut y = 0usize;
    while y < outer_h {
        let Some(row_off) = framebuffer_byte_offset(warm, origin_x, origin_y.saturating_add(y))
        else {
            crate::log!(
                "intel/igpu770: cpu-fb-nv12 label={} status=row-off-failed row={}\n",
                label,
                y
            );
            return;
        };
        let row_ptr = warm.limine_fb_virt.saturating_add(row_off) as *mut u32;
        let src_y = (y.saturating_mul(scale)).min(frame_height.saturating_sub(1));
        let mut x = 0usize;
        while x < outer_w {
            let src_x = (x.saturating_mul(scale)).min(frame_width.saturating_sub(1));
            let y_idx = src_y.saturating_mul(pitch).saturating_add(src_x);
            let uv_x = src_x & !1usize;
            let uv_idx = (src_y / 2).saturating_mul(pitch).saturating_add(uv_x);
            let y_sample = y_plane.get(y_idx).copied().unwrap_or(16) as i32;
            let u_sample = uv_plane.get(uv_idx).copied().unwrap_or(128) as i32;
            let v_sample = uv_plane
                .get(uv_idx.saturating_add(1))
                .copied()
                .unwrap_or(128) as i32;

            let c = (y_sample - 16).max(0);
            let d = u_sample - 128;
            let e = v_sample - 128;
            let r = clamp_u8_i32((298 * c + 409 * e + 128) >> 8);
            let g = clamp_u8_i32((298 * c - 100 * d - 208 * e + 128) >> 8);
            let b = clamp_u8_i32((298 * c + 516 * d + 128) >> 8);
            let rgb = ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
            unsafe { core::ptr::write_volatile(row_ptr.add(x), rgb) };
            x += 1;
        }
        y += 1;
    }

    dma_cache_flush(warm.limine_fb_virt as *const u8, warm.limine_fb_size);
    crate::log!(
        "intel/igpu770: cpu-fb-nv12 label={} origin={}x{} dims={}x{} src={}x{} pitch={} bytes={}\n",
        label,
        origin_x,
        origin_y,
        outer_w,
        outer_h,
        frame_width,
        frame_height,
        pitch,
        bytes.len()
    );
    log_framebuffer_probe(
        warm,
        "cpu-nv12-center",
        origin_x.saturating_add(outer_w / 2),
        origin_y.saturating_add(outer_h / 2),
    );
}

pub fn cpu_framebuffer_media_status_card_center(
    label: &str,
    width: usize,
    height: usize,
    accent_color: u32,
    metric_a: usize,
    metric_b: usize,
    ready: bool,
) {
    let Some(warm) = warm_state() else {
        crate::log!("intel/igpu770: cpu-fb-media-card label={} status=not-warmed\n", label);
        return;
    };
    if warm.limine_fb_virt == 0 || warm.limine_fb_pitch == 0 || warm.limine_fb_bpp < 32 {
        crate::log!("intel/igpu770: cpu-fb-media-card label={} status=fb-unavailable\n", label);
        return;
    }

    let outer_w = width.min(warm.limine_fb_width.max(1)).max(24);
    let outer_h = height.min(warm.limine_fb_height.max(1)).max(24);
    let origin_x = warm.limine_fb_width.saturating_sub(outer_w) / 2;
    let origin_y = warm.limine_fb_height.saturating_sub(outer_h) / 2;
    let inner_x = origin_x.saturating_add(2);
    let inner_y = origin_y.saturating_add(2);
    let inner_w = outer_w.saturating_sub(4).max(1);
    let inner_h = outer_h.saturating_sub(4).max(1);

    if !cpu_framebuffer_fill_rect(warm, origin_x, origin_y, outer_w, outer_h, 0x00F4_F8FF) {
        crate::log!("intel/igpu770: cpu-fb-media-card label={} status=outer-fill-failed\n", label);
        return;
    }

    let bg_color = if ready { 0x000B_1220 } else { 0x0022_1408 };
    let panel_color = if ready { 0x0011_2238 } else { 0x0030_1D10 };
    let meter_bg = 0x0016_1A20;
    let ok_color = if ready { 0x0032_D17C } else { 0x00E0_A12B };

    let _ = cpu_framebuffer_fill_rect(warm, inner_x, inner_y, inner_w, inner_h, bg_color);

    let header_h = (inner_h / 7).max(3).min(inner_h);
    let _ = cpu_framebuffer_fill_rect(warm, inner_x, inner_y, inner_w, header_h, accent_color);

    let poster_w = (inner_w / 3).max(6).min(inner_w);
    let poster_h = inner_h.saturating_sub(header_h).saturating_sub(4).max(4);
    let poster_y = inner_y.saturating_add(header_h).saturating_add(2);
    let _ = cpu_framebuffer_fill_rect(
        warm,
        inner_x.saturating_add(2),
        poster_y,
        poster_w,
        poster_h,
        panel_color,
    );

    let status_size = (header_h.saturating_sub(1)).max(2);
    let status_x = inner_x.saturating_add(inner_w.saturating_sub(status_size).saturating_sub(1));
    let status_y = inner_y.saturating_add((header_h.saturating_sub(status_size)) / 2);
    let _ = cpu_framebuffer_fill_rect(warm, status_x, status_y, status_size, status_size, ok_color);

    let bars_x = inner_x
        .saturating_add(poster_w)
        .saturating_add(6)
        .min(inner_x.saturating_add(inner_w.saturating_sub(1)));
    let bars_w = inner_x
        .saturating_add(inner_w)
        .saturating_sub(bars_x)
        .saturating_sub(3)
        .max(6);
    let bar_h = (inner_h / 10).max(3);
    let gap_h = (bar_h / 2).max(2);
    let base_y = poster_y.saturating_add(2);

    let mut bar_idx = 0usize;
    while bar_idx < 3 {
        let y = base_y.saturating_add(bar_idx.saturating_mul(bar_h.saturating_add(gap_h)));
        if y.saturating_add(bar_h) > inner_y.saturating_add(inner_h) {
            break;
        }
        let _ = cpu_framebuffer_fill_rect(warm, bars_x, y, bars_w, bar_h, meter_bg);
        let selector = match bar_idx {
            0 => metric_a,
            1 => metric_b,
            _ => metric_a ^ metric_b,
        };
        let fill_w = (selector % bars_w.max(1)).max(bar_h).min(bars_w);
        let fill_color = match bar_idx {
            0 => accent_color,
            1 => ok_color,
            _ => 0x00FF_F4D6,
        };
        let _ = cpu_framebuffer_fill_rect(warm, bars_x, y, fill_w, bar_h, fill_color);
        bar_idx += 1;
    }

    dma_cache_flush(warm.limine_fb_virt as *const u8, warm.limine_fb_size);
    crate::log!(
        "intel/igpu770: cpu-fb-media-card label={} origin={}x{} dims={}x{} metric_a={} metric_b={} ready={} accent=0x{:08X}\n",
        label,
        origin_x,
        origin_y,
        outer_w,
        outer_h,
        metric_a,
        metric_b,
        ready as u8,
        accent_color
    );
    log_framebuffer_probe(
        warm,
        "cpu-media-card-center",
        origin_x.saturating_add(outer_w / 2),
        origin_y.saturating_add(outer_h / 2),
    );
}

fn blt_fill_rect_plan(warm: Igpu770WarmState) -> Option<BltFillRectPlan> {
    fb_fill_rect_plan(warm, SMOKE_RECT_W, SMOKE_RECT_H, SMOKE_COLOR_XRGB8888, true)
}

fn bcs_fill_rect_plan(warm: Igpu770WarmState) -> Option<BltFillRectPlan> {
    fb_fill_rect_plan(warm, BCS_RECT_W, BCS_RECT_H, BCS_COLOR_XRGB8888, true)
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
    let guc_ads =
        ggtt_map_plan_system_ram(warm.guc_ads_phys, warm.guc_ads_len, warm.guc_ads_gpu_addr);

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
    if let Some(plan) = guc_ads {
        log_ggtt_map_plan("guc-ads", plan);
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

    let Some(ring) = ggtt_map_plan_system_ram(warm.ring_phys, warm.ring_len, GPU_VA_RING_BASE)
    else {
        crate::log!("intel/igpu770: ggtt-map skipped reason=ring-plan\n");
        return;
    };
    let Some(context) =
        ggtt_map_plan_system_ram(warm.context_phys, warm.context_len, GPU_VA_CONTEXT_BASE)
    else {
        crate::log!("intel/igpu770: ggtt-map skipped reason=context-plan\n");
        return;
    };
    let Some(batch) = ggtt_map_plan_system_ram(warm.batch_phys, warm.batch_len, GPU_VA_BATCH_BASE)
    else {
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
    let guc_ads =
        ggtt_map_plan_system_ram(warm.guc_ads_phys, warm.guc_ads_len, warm.guc_ads_gpu_addr);

    let _ = forcewake_all_acquire(warm);
    crate::log!("intel/igpu770: ggtt-map begin\n");
    let ok_ring = ggtt_program_plan("ring", warm, ring);
    let ok_context = ggtt_program_plan("context", warm, context);
    let ok_batch = ggtt_program_plan("batch", warm, batch);
    let ok_result = ggtt_program_plan("result", warm, result);
    let ok_guc_fw = guc_fw
        .map(|plan| ggtt_program_plan("guc-fw", warm, plan))
        .unwrap_or(false);
    let ok_guc_ads = guc_ads
        .map(|plan| ggtt_program_plan("guc-ads", warm, plan))
        .unwrap_or(false);
    let flsh = ggtt_invalidate(warm);
    crate::log!(
        "intel/igpu770: ggtt-map summary ring={} context={} batch={} result={} guc_fw={} guc_ads={} gfx_flsh_cntl=0x{:08X}\n",
        ok_ring as u8,
        ok_context as u8,
        ok_batch as u8,
        ok_result as u8,
        ok_guc_fw as u8,
        ok_guc_ads as u8,
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

    let Some(ring) = ggtt_map_plan_system_ram(warm.ring_phys, warm.ring_len, GPU_VA_RING_BASE)
    else {
        crate::log!("intel/igpu770: ggtt-blt-smoke skipped reason=ring-plan\n");
        return;
    };
    let Some(context) =
        ggtt_map_plan_system_ram(warm.context_phys, warm.context_len, GPU_VA_CONTEXT_BASE)
    else {
        crate::log!("intel/igpu770: ggtt-blt-smoke skipped reason=context-plan\n");
        return;
    };
    let Some(batch) = ggtt_map_plan_system_ram(warm.batch_phys, warm.batch_len, GPU_VA_BATCH_BASE)
    else {
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
        "intel/igpu770: rcs-triangle-plan dst_gpu=0x{:X} dst_phys=0x{:X} dst_xy={}x{} rect={}x{} pitch=0x{:X} marker=0x{:08X}\n",
        fill.dst_gpu_addr,
        fill.dst_phys,
        fill.dst_x,
        fill.dst_y,
        fill.rect_w,
        fill.rect_h,
        fill.pitch,
        RCS_EXEC_RESULT_DONE
    );
    log_framebuffer_probe(warm, "rcs-pre-origin", fill.dst_x, fill.dst_y);
    log_framebuffer_probe(
        warm,
        "rcs-pre-tail",
        fill.dst_x.saturating_add(fill.rect_w.saturating_sub(1)),
        fill.dst_y.saturating_add(fill.rect_h.saturating_sub(1)),
    );
    log_rcs_triangle_probe_set(warm, "rcs-pre", fill);

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
    let _ = forcewake_all_acquire(warm);
    let _ =
        mmio_write32(warm, RCS_RING_MODE_GEN7, masked_bit_enable(GEN11_GFX_DISABLE_LEGACY_MODE));
    let ctx_ctl_after = masked_bits_update(
        CTX_CTRL_RS_CTX_ENABLE,
        CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT
            | CTX_CTRL_ENGINE_CTX_SAVE_INHIBIT
            | CTX_CTRL_INHIBIT_SYN_CTX_SWITCH,
    );
    let ctx_ctl_ref_after = masked_bits_update(
        CTX_CTRL_RS_CTX_ENABLE,
        CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT
            | CTX_CTRL_ENGINE_CTX_SAVE_INHIBIT
            | CTX_CTRL_INHIBIT_SYN_CTX_SWITCH,
    );
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
    invalidate_framebuffer_probe(warm, fill.dst_x, fill.dst_y);
    invalidate_framebuffer_probe(
        warm,
        fill.dst_x.saturating_add(fill.rect_w.saturating_sub(1)),
        fill.dst_y.saturating_add(fill.rect_h.saturating_sub(1)),
    );
    dma_cache_flush(warm.limine_fb_virt as *const u8, core::mem::size_of::<u32>());
    let result0 = unsafe { core::ptr::read_volatile(warm.result_virt as *const u32) };
    let fb0 = unsafe { core::ptr::read_volatile(warm.limine_fb_virt as *const u32) };
    log_framebuffer_probe(warm, "rcs-post-origin", fill.dst_x, fill.dst_y);
    log_framebuffer_probe(
        warm,
        "rcs-post-tail",
        fill.dst_x.saturating_add(fill.rect_w.saturating_sub(1)),
        fill.dst_y.saturating_add(fill.rect_h.saturating_sub(1)),
    );
    log_rcs_triangle_probe_set(warm, "rcs-post", fill);
    log_rcs_mode_summary(
        warm,
        if completed {
            "post-complete"
        } else {
            "post-timeout"
        },
    );
    log_rcs_regs(
        warm,
        if completed {
            "post-complete"
        } else {
            "post-timeout"
        },
    );
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

pub fn ggtt_bcs_smoke_test_once() {
    let ran_before = GGTT_BCS_SMOKE_RAN.load(Ordering::Acquire);
    crate::log!("intel/igpu770: ggtt-bcs-smoke entry ran_before={}\n", ran_before as u8);
    if GGTT_BCS_SMOKE_RAN.swap(true, Ordering::AcqRel) {
        crate::log!("intel/igpu770: ggtt-bcs-smoke skipped reason=already-ran\n");
        return;
    }

    let Some(warm) = warm_state() else {
        crate::log!("intel/igpu770: ggtt-bcs-smoke skipped reason=not-warmed\n");
        return;
    };
    if !intel_guc::ready() {
        crate::log!(
            "intel/igpu770: ggtt-bcs-smoke skipped reason=guc-not-ready guc_status=0x{:08X}\n",
            intel_guc::status(warm)
        );
        return;
    }

    let Some(ring) = ggtt_map_plan_system_ram(warm.ring_phys, warm.ring_len, GPU_VA_RING_BASE)
    else {
        crate::log!("intel/igpu770: ggtt-bcs-smoke skipped reason=ring-plan\n");
        return;
    };
    let Some(context) =
        ggtt_map_plan_system_ram(warm.context_phys, warm.context_len, GPU_VA_CONTEXT_BASE)
    else {
        crate::log!("intel/igpu770: ggtt-bcs-smoke skipped reason=context-plan\n");
        return;
    };
    let Some(batch) = ggtt_map_plan_system_ram(warm.batch_phys, warm.batch_len, GPU_VA_BATCH_BASE)
    else {
        crate::log!("intel/igpu770: ggtt-bcs-smoke skipped reason=batch-plan\n");
        return;
    };
    let Some(result) =
        ggtt_map_plan_system_ram(warm.result_phys, warm.result_len, GPU_VA_RESULT_BASE)
    else {
        crate::log!("intel/igpu770: ggtt-bcs-smoke skipped reason=result-plan\n");
        return;
    };
    let Some(fill) = bcs_fill_rect_plan(warm) else {
        crate::log!("intel/igpu770: ggtt-bcs-smoke skipped reason=fb-plan\n");
        return;
    };

    unsafe {
        ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        ptr::write_bytes(warm.result_virt, 0, warm.result_len);
    }
    dma_cache_flush(warm.result_virt as *const u8, warm.result_len);

    crate::log!("intel/igpu770: ggtt-bcs-smoke begin\n");
    log_ggtt_map_plan("ring", ring);
    log_ggtt_map_plan("context", context);
    log_ggtt_map_plan("batch", batch);
    log_ggtt_map_plan("result", result);
    crate::log!(
        "intel/igpu770: bcs-fill-plan dst_gpu=0x{:X} dst_phys=0x{:X} dst_xy={}x{} rect={}x{} pitch=0x{:X} color=0x{:08X} result_gpu=0x{:X} start=0x{:08X} pre=0x{:08X} post=0x{:08X} done=0x{:08X}\n",
        fill.dst_gpu_addr,
        fill.dst_phys,
        fill.dst_x,
        fill.dst_y,
        fill.rect_w,
        fill.rect_h,
        fill.pitch,
        fill.color,
        GPU_VA_RESULT_BASE,
        BCS_EXEC_RESULT_START,
        BCS_EXEC_RESULT_PRE_COPY,
        BCS_EXEC_RESULT_POST_COPY,
        BCS_EXEC_RESULT_DONE
    );
    log_framebuffer_probe(warm, "bcs-pre-origin", fill.dst_x, fill.dst_y);
    log_framebuffer_probe(
        warm,
        "bcs-pre-center",
        fill.dst_x.saturating_add(fill.rect_w / 2),
        fill.dst_y.saturating_add(fill.rect_h / 2),
    );

    let batch_dwords = unsafe {
        core::slice::from_raw_parts_mut(
            warm.batch_virt as *mut u32,
            warm.batch_len / core::mem::size_of::<u32>(),
        )
    };
    let Some(surface_byte_off) = fill
        .dst_y
        .checked_mul(fill.pitch)
        .and_then(|off| off.checked_add(fill.dst_x.saturating_mul(4)))
    else {
        crate::log!("intel/igpu770: ggtt-bcs-smoke skipped reason=surface-base-overflow\n");
        return;
    };
    let Some(_surface_gpu_addr) = fill.dst_gpu_addr.checked_sub(surface_byte_off as u64) else {
        crate::log!("intel/igpu770: ggtt-bcs-smoke skipped reason=surface-base-underflow\n");
        return;
    };
    let batch_tail_bytes = match build_bcs_store_pixels_batch(batch_dwords, fill) {
        Ok(bytes) => bytes,
        Err(err) => {
            crate::log!("intel/igpu770: ggtt-bcs-smoke skipped reason=batch-build err={}\n", err);
            return;
        }
    };
    dma_cache_flush(warm.batch_virt as *const u8, batch_tail_bytes);
    let ring_tail_bytes = build_bcs_ring_batch_start(warm, batch.gpu_addr);
    let Some(ring_ctl) = ring_ctl_value(warm.ring_len) else {
        crate::log!(
            "intel/igpu770: ggtt-bcs-smoke skipped reason=ring-ctl ring_len=0x{:X}\n",
            warm.ring_len
        );
        return;
    };
    let ring_start = ring.gpu_addr as u32;
    let context_desc = context.gpu_addr;
    if !init_gen12_bcs_context_image(warm, ring_start, ring_tail_bytes as u32, ring_ctl) {
        crate::log!("intel/igpu770: ggtt-bcs-smoke skipped reason=lrc-context-init\n");
        return;
    }
    let (context_desc_lo, context_desc_hi) = build_execlist_context_descriptor(context_desc);

    crate::log!(
        "intel/igpu770: bcs-submit prep ring_start=0x{:08X} ring_ctl=0x{:08X} ring_tail=0x{:X} batch_tail=0x{:X} batch_gpu=0x{:X} context_gpu=0x{:X} ctx_desc_lo=0x{:08X} ctx_desc_hi=0x{:08X} result_phys=0x{:X}\n",
        ring_start,
        ring_ctl,
        ring_tail_bytes,
        batch_tail_bytes,
        batch.gpu_addr,
        context_desc,
        context_desc_lo,
        context_desc_hi,
        warm.result_phys
    );
    crate::log!(
        "intel/igpu770: bcs-batch-dwords d0=0x{:08X} d1=0x{:08X} d2=0x{:08X} d3=0x{:08X} d4=0x{:08X} d5=0x{:08X} d6=0x{:08X} d7=0x{:08X}\n",
        batch_dwords.get(0).copied().unwrap_or(0),
        batch_dwords.get(1).copied().unwrap_or(0),
        batch_dwords.get(2).copied().unwrap_or(0),
        batch_dwords.get(3).copied().unwrap_or(0),
        batch_dwords.get(4).copied().unwrap_or(0),
        batch_dwords.get(5).copied().unwrap_or(0),
        batch_dwords.get(6).copied().unwrap_or(0),
        batch_dwords.get(7).copied().unwrap_or(0)
    );

    if !xelp_copy_ngin::copy_ngin_enabled() {
        crate::log!("intel/igpu770: ggtt-bcs-smoke skipped reason=copy-ngin-disabled\n");
        return;
    }

    let _ = forcewake_all_acquire(warm);
    let _ =
        mmio_write32(warm, BCS_RING_MODE_GEN7, masked_bit_enable(GEN11_GFX_DISABLE_LEGACY_MODE));
    let ctx_ctl_after = masked_bits_update(
        CTX_CTRL_RS_CTX_ENABLE,
        CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT
            | CTX_CTRL_ENGINE_CTX_SAVE_INHIBIT
            | CTX_CTRL_INHIBIT_SYN_CTX_SWITCH,
    );
    let ctx_ctl_ref_after = masked_bits_update(
        CTX_CTRL_RS_CTX_ENABLE,
        CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT
            | CTX_CTRL_ENGINE_CTX_SAVE_INHIBIT
            | CTX_CTRL_INHIBIT_SYN_CTX_SWITCH,
    );
    let _ = mmio_write32(warm, BCS_RING_CONTEXT_CONTROL, ctx_ctl_after);
    let _ = mmio_write32(warm, BCS_RING_CONTEXT_CONTROL_REF, ctx_ctl_ref_after);

    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    bcs_execlist_submit_port_push(warm, context_desc_lo, context_desc_hi, 0, 0);
    let _ = mmio_write32(warm, BCS_RING_EXECLIST_CONTROL, EL_CTRL_LOAD);
    let sq_lo_rb = mmio_read32(warm, BCS_RING_EXECLIST_SQ_LO);
    let sq_hi_rb = mmio_read32(warm, BCS_RING_EXECLIST_SQ_HI);
    let mode_rb = mmio_read32(warm, BCS_RING_MODE_GEN7);
    let ctx_ctl_rb = mmio_read32(warm, BCS_RING_CONTEXT_CONTROL);
    let ctx_ctl_ref_rb = mmio_read32(warm, BCS_RING_CONTEXT_CONTROL_REF);
    let el_ctl_rb = mmio_read32(warm, BCS_RING_EXECLIST_CONTROL);
    let el_status_lo_rb = mmio_read32(warm, BCS_RING_EXECLIST_STATUS_LO);
    let el_status_hi_rb = mmio_read32(warm, BCS_RING_EXECLIST_STATUS_HI);
    crate::log!(
        "intel/igpu770: bcs-execlist-submit context sq_lo_req=0x{:08X} sq_hi_req=0x{:08X} sq_lo_rb=0x{:08X} sq_hi_rb=0x{:08X} mode_rb=0x{:08X} ctx_ctl_rb=0x{:08X} ctx_ctl_ref_rb=0x{:08X} el_ctl_rb=0x{:08X} el_status_lo_rb=0x{:08X} el_status_hi_rb=0x{:08X}\n",
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

    if let Some(reg) = intel_770_registers::describe_register(BCS_RING_IMR) {
        crate::log!(
            "intel/igpu770: forcewake-bcs sanity-target block={} reg={} off=0x{:05X} desc={}\n",
            reg.block,
            reg.name,
            reg.offset,
            reg.description
        );
    }
    let bcs_imr_before = mmio_read32(warm, BCS_RING_IMR);
    let bcs_imr_toggled = bcs_imr_before ^ 0x0000_0001;
    let _ = mmio_write32(warm, BCS_RING_IMR, bcs_imr_toggled);
    let bcs_imr_after = mmio_read32(warm, BCS_RING_IMR);
    let _ = mmio_write32(warm, BCS_RING_IMR, bcs_imr_before);
    let bcs_imr_restored = mmio_read32(warm, BCS_RING_IMR);
    crate::log!(
        "intel/igpu770: forcewake-bcs sanity reg=BCS_IMR before=0x{:08X} wrote=0x{:08X} after=0x{:08X} restored=0x{:08X}\n",
        bcs_imr_before,
        bcs_imr_toggled,
        bcs_imr_after,
        bcs_imr_restored
    );

    log_bcs_mode_summary(warm, "pre");
    log_bcs_regs(warm, "pre");

    let mut completed = false;
    let mut first_head = 0u32;
    let mut first_tail = 0u32;
    let mut final_head = 0u32;
    let mut final_tail = 0u32;
    let mut iter = 0usize;
    let execlist_lo0 = mmio_read32(warm, BCS_RING_EXECLIST_STATUS_LO);
    let execlist_hi0 = mmio_read32(warm, BCS_RING_EXECLIST_STATUS_HI);
    while iter < BLT_POLL_ITERS {
        let head = mmio_read32(warm, BCS_RING_HEAD);
        let tail = mmio_read32(warm, BCS_RING_TAIL);
        let execlist_lo = mmio_read32(warm, BCS_RING_EXECLIST_STATUS_LO);
        let execlist_hi = mmio_read32(warm, BCS_RING_EXECLIST_STATUS_HI);
        let result = read_bcs_result_markers(warm);
        if iter == 0 {
            first_head = head;
            first_tail = tail;
        }
        final_head = head;
        final_tail = tail;
        if iter == 0 || (iter % BLT_POLL_LOG_STEP) == 0 {
            crate::log!(
                "intel/igpu770: bcs-poll iter={} head=0x{:08X} tail=0x{:08X} acthd=0x{:08X} ipeir=0x{:08X} ipehr=0x{:08X} eir=0x{:08X} mode=0x{:08X} instdone=0x{:08X} execlist_lo=0x{:08X} execlist_hi=0x{:08X} result0=0x{:08X}\n",
                iter,
                head,
                tail,
                mmio_read32(warm, BCS_RING_ACTHD),
                mmio_read32(warm, BCS_RING_IPEIR),
                mmio_read32(warm, BCS_RING_IPEHR),
                mmio_read32(warm, BCS_RING_EIR),
                mmio_read32(warm, BCS_RING_MODE_GEN7),
                mmio_read32(warm, BCS_RING_INSTDONE),
                execlist_lo,
                execlist_hi,
                result[0]
            );
            crate::log!(
                "intel/igpu770: bcs-markers iter={} start=0x{:08X} pre=0x{:08X} post=0x{:08X} done=0x{:08X}\n",
                iter,
                result[0],
                result[1],
                result[2],
                result[3]
            );
        }
        if result[3] == BCS_EXEC_RESULT_DONE {
            completed = true;
            break;
        }
        if execlist_lo != execlist_lo0 || execlist_hi != execlist_hi0 {
            break;
        }
        core::hint::spin_loop();
        iter += 1;
    }

    dma_cache_flush(warm.result_virt as *const u8, warm.result_len);
    invalidate_framebuffer_probe(warm, fill.dst_x, fill.dst_y);
    invalidate_framebuffer_probe(
        warm,
        fill.dst_x.saturating_add(fill.rect_w / 2),
        fill.dst_y.saturating_add(fill.rect_h / 2),
    );
    dma_cache_flush(warm.limine_fb_virt as *const u8, core::mem::size_of::<u32>());
    let result = read_bcs_result_markers(warm);
    let fb0 = unsafe { core::ptr::read_volatile(warm.limine_fb_virt as *const u32) };
    log_framebuffer_probe(warm, "bcs-post-origin", fill.dst_x, fill.dst_y);
    log_framebuffer_probe(
        warm,
        "bcs-post-center",
        fill.dst_x.saturating_add(fill.rect_w / 2),
        fill.dst_y.saturating_add(fill.rect_h / 2),
    );
    log_bcs_mode_summary(
        warm,
        if completed {
            "post-complete"
        } else {
            "post-timeout"
        },
    );
    log_bcs_regs(
        warm,
        if completed {
            "post-complete"
        } else {
            "post-timeout"
        },
    );
    crate::log!(
        "intel/igpu770: bcs-submit result completed={} iters={} head0=0x{:08X} tail0=0x{:08X} headf=0x{:08X} tailf=0x{:08X} start=0x{:08X} pre=0x{:08X} post=0x{:08X} done=0x{:08X} expect_done=0x{:08X} fb0=0x{:08X} execlist_lo0=0x{:08X} execlist_hi0=0x{:08X} execlist_lof=0x{:08X} execlist_hif=0x{:08X} forcewake_held={}\n",
        completed as u8,
        iter,
        first_head,
        first_tail,
        final_head,
        final_tail,
        result[0],
        result[1],
        result[2],
        result[3],
        BCS_EXEC_RESULT_DONE,
        fb0,
        execlist_lo0,
        execlist_hi0,
        mmio_read32(warm, BCS_RING_EXECLIST_STATUS_LO),
        mmio_read32(warm, BCS_RING_EXECLIST_STATUS_HI),
        FORCEWAKE_GT_HELD.load(Ordering::Acquire) as u8
    );
}

pub(crate) fn rcs_present_rgba_frame(rgba: &[u8], width: usize, height: usize) -> bool {
    let Some(warm) = warm_state() else {
        crate::log!("intel/igpu770: rcs-present skipped reason=not-warmed\n");
        return false;
    };
    if !intel_guc::ready() {
        crate::log!(
            "intel/igpu770: rcs-present skipped reason=guc-not-ready guc_status=0x{:08X}\n",
            intel_guc::status(warm)
        );
        return false;
    }
    if width == 0 || height == 0 || rgba.len() < width.saturating_mul(height).saturating_mul(4) {
        crate::log!(
            "intel/igpu770: rcs-present skipped reason=invalid-frame width={} height={} bytes={}\n",
            width,
            height,
            rgba.len()
        );
        return false;
    }

    ggtt_map_smoke_objects_once();

    let Some(ring) = ggtt_map_plan_system_ram(warm.ring_phys, warm.ring_len, GPU_VA_RING_BASE)
    else {
        crate::log!("intel/igpu770: rcs-present skipped reason=ring-plan\n");
        return false;
    };
    let Some(context) =
        ggtt_map_plan_system_ram(warm.context_phys, warm.context_len, GPU_VA_CONTEXT_BASE)
    else {
        crate::log!("intel/igpu770: rcs-present skipped reason=context-plan\n");
        return false;
    };
    let Some(batch) = ggtt_map_plan_system_ram(warm.batch_phys, warm.batch_len, GPU_VA_BATCH_BASE)
    else {
        crate::log!("intel/igpu770: rcs-present skipped reason=batch-plan\n");
        return false;
    };

    let copy_w = width.min(warm.limine_fb_width.max(1));
    let copy_h = height.min(warm.limine_fb_height.max(1));
    let Some(dst) = ggtt_map_plan_aperture_backed(
        warm.limine_fb_phys,
        warm.limine_fb_size,
        warm.aperture_bar_phys,
    ) else {
        crate::log!("intel/igpu770: rcs-present skipped reason=fb-plan\n");
        return false;
    };
    let frame_pixels = copy_w.saturating_mul(copy_h);
    if frame_pixels == 0 {
        crate::log!("intel/igpu770: rcs-present skipped reason=empty-clamped-frame\n");
        return false;
    }

    let Some(ring_ctl) = ring_ctl_value(warm.ring_len) else {
        crate::log!(
            "intel/igpu770: rcs-present skipped reason=ring-ctl ring_len=0x{:X}\n",
            warm.ring_len
        );
        return false;
    };
    let ring_start = ring.gpu_addr as u32;
    if !init_gen12_lrc_context_image(warm, ring_start, BLT_RING_TAIL_BYTES, ring_ctl) {
        crate::log!("intel/igpu770: rcs-present skipped reason=lrc-context-init\n");
        return false;
    }
    let (context_desc_lo, context_desc_hi) = build_execlist_context_descriptor(context.gpu_addr);
    let dst_gpu_addr = dst.gpu_addr;

    let mut completed_pixels = 0usize;
    let mut chunk_idx = 0usize;
    let mut chunk_fail = false;

    while completed_pixels < frame_pixels {
        unsafe {
            ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
            ptr::write_bytes(warm.result_virt, 0, warm.result_len);
        }

        let batch_dwords = unsafe {
            core::slice::from_raw_parts_mut(
                warm.batch_virt as *mut u32,
                warm.batch_len / core::mem::size_of::<u32>(),
            )
        };
        let start_value = RCS_PRESENT_RESULT_START_BASE
            .wrapping_add(chunk_idx as u32)
            .wrapping_add(1);
        let done_value = RCS_PRESENT_RESULT_BASE
            .wrapping_add(chunk_idx as u32)
            .wrapping_add(1);
        let (batch_tail_bytes, chunk_pixels) =
            match super::xelp_render_ngin::encode_rgba_store_batch_chunk(
                batch_dwords,
                rgba,
                copy_w,
                copy_h,
                RCS_PRESENT_MAX_CHUNK_PIXELS,
                dst_gpu_addr,
                warm.limine_fb_pitch,
                completed_pixels,
                GPU_VA_RESULT_BASE,
                start_value,
                done_value,
            ) {
                Ok(v) => v,
                Err(err) => {
                    crate::log!(
                        "intel/igpu770: rcs-present chunk={} build-failed err={} start_pixel={} total_pixels={}\n",
                        chunk_idx,
                        err,
                        completed_pixels,
                        frame_pixels
                    );
                    chunk_fail = true;
                    break;
                }
            };

        dma_cache_flush(warm.batch_virt as *const u8, batch_tail_bytes);
        dma_cache_flush(warm.result_virt as *const u8, warm.result_len);
        let ring_tail_bytes = build_ring_batch_start(warm, batch.gpu_addr);

        let _ = forcewake_all_acquire(warm);
        let _ = mmio_write32(
            warm,
            RCS_RING_MODE_GEN7,
            masked_bit_enable(GEN11_GFX_DISABLE_LEGACY_MODE),
        );
        let ctx_ctl_after = masked_bits_update(
            CTX_CTRL_RS_CTX_ENABLE,
            CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT
                | CTX_CTRL_ENGINE_CTX_SAVE_INHIBIT
                | CTX_CTRL_INHIBIT_SYN_CTX_SWITCH,
        );
        let ctx_ctl_ref_after = masked_bits_update(
            CTX_CTRL_RS_CTX_ENABLE,
            CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT
                | CTX_CTRL_ENGINE_CTX_SAVE_INHIBIT
                | CTX_CTRL_INHIBIT_SYN_CTX_SWITCH,
        );
        let _ = mmio_write32(warm, RCS_RING_CONTEXT_CONTROL, ctx_ctl_after);
        let _ = mmio_write32(warm, RCS_RING_CONTEXT_CONTROL_REF, ctx_ctl_ref_after);

        if !init_gen12_lrc_context_image(warm, ring_start, ring_tail_bytes as u32, ring_ctl) {
            crate::log!(
                "intel/igpu770: rcs-present chunk={} skipped reason=lrc-context-reinit\n",
                chunk_idx
            );
            chunk_fail = true;
            break;
        }

        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        execlist_submit_port_push(warm, context_desc_lo, context_desc_hi, 0, 0);
        let _ = mmio_write32(warm, RCS_RING_EXECLIST_CONTROL, EL_CTRL_LOAD);

        let mut completed = false;
        let mut iter = 0usize;
        while iter < BLT_POLL_ITERS {
            let result0 = unsafe { core::ptr::read_volatile(warm.result_virt as *const u32) };
            if result0 == done_value {
                completed = true;
                break;
            }
            core::hint::spin_loop();
            iter += 1;
        }

        dma_cache_flush(warm.result_virt as *const u8, warm.result_len);
        let result0 = unsafe { core::ptr::read_volatile(warm.result_virt as *const u32) };
        if !completed {
            let phase = if result0 == start_value {
                "start-only"
            } else if result0 == done_value {
                "done"
            } else if result0 == 0 {
                "none"
            } else {
                "other"
            };
            crate::log!(
                "intel/igpu770: rcs-present chunk={} timeout iters={} result0=0x{:08X} start=0x{:08X} expect=0x{:08X} phase={} batch_tail=0x{:X} head=0x{:08X} tail=0x{:08X} acthd=0x{:08X} ipeir=0x{:08X} ipehr=0x{:08X}\n",
                chunk_idx,
                iter,
                result0,
                start_value,
                done_value,
                phase,
                batch_tail_bytes,
                mmio_read32(warm, RCS_RING_HEAD),
                mmio_read32(warm, RCS_RING_TAIL),
                mmio_read32(warm, RCS_RING_ACTHD),
                mmio_read32(warm, RCS_RING_IPEIR),
                mmio_read32(warm, RCS_RING_IPEHR)
            );
            chunk_fail = true;
            break;
        }

        completed_pixels = completed_pixels.saturating_add(chunk_pixels);
        if chunk_idx < 4 || completed_pixels >= frame_pixels {
            crate::log!(
                "intel/igpu770: rcs-present chunk={} pixels={} completed_pixels={} total_pixels={} batch_tail=0x{:X}\n",
                chunk_idx,
                chunk_pixels,
                completed_pixels,
                frame_pixels,
                batch_tail_bytes
            );
        }
        chunk_idx = chunk_idx.saturating_add(1);
    }

    let success = !chunk_fail && completed_pixels >= frame_pixels;
    crate::log!(
        "intel/igpu770: rcs-present summary success={} size={}x{} chunks={} completed_pixels={} total_pixels={} dst_gpu=0x{:X} pitch=0x{:X}\n",
        success as u8,
        copy_w,
        copy_h,
        chunk_idx,
        completed_pixels,
        frame_pixels,
        dst_gpu_addr,
        warm.limine_fb_pitch
    );
    success
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
    let guc_ads_len = if guc_fw.len != 0 {
        intel_guc::minimal_ads_size()
    } else {
        0
    };
    let (guc_ads_phys, guc_ads_virt) = if guc_ads_len != 0 {
        match crate::dma::alloc(guc_ads_len, WARM_ALIGN) {
            Some((phys, virt)) => {
                unsafe {
                    ptr::write_bytes(virt, 0, guc_ads_len);
                }
                (phys, virt)
            }
            None => {
                crate::log!(
                    "intel/igpu770: warm alloc failed part=guc-ads size=0x{:X}\n",
                    guc_ads_len
                );
                (0, ptr::null_mut())
            }
        }
    } else {
        (0, ptr::null_mut())
    };

    let fb = limine_framebuffer_info();

    let warm = Igpu770WarmState {
        device_id: info.device_id,
        revision_id: info.revision_id,
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
        guc_ads_phys,
        guc_ads_virt,
        guc_ads_len,
        guc_ads_gpu_addr: if guc_ads_phys != 0 {
            GPU_VA_GUC_ADS_BASE
        } else {
            0
        },
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
        "intel/igpu770: warm device=0x{:04X} rev=0x{:02X} ring_phys=0x{:X} ring_len=0x{:X} context_phys=0x{:X} context_len=0x{:X} batch_phys=0x{:X} batch_len=0x{:X} result_phys=0x{:X} result_len=0x{:X} guc_fw_phys=0x{:X} guc_fw_len=0x{:X} guc_fw_xfer=0x{:X} guc_fw_gpu=0x{:X} guc_ads_phys=0x{:X} guc_ads_len=0x{:X} guc_ads_gpu=0x{:X} mmio_len=0x{:X} aperture=0x{:X}/0x{:X} limine_fb=0x{:X}/0x{:X} {}x{} pitch=0x{:X} bpp={}\n",
        warm.device_id,
        warm.revision_id,
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
        warm.guc_ads_phys,
        warm.guc_ads_len,
        warm.guc_ads_gpu_addr,
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
