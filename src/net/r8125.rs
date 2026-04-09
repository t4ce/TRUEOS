use alloc::vec::Vec;

use core::cmp::min;
use core::ptr::{NonNull, read_volatile, write_volatile};
use core::sync::atomic::{Ordering, compiler_fence, fence};

use crate::net::core::VendorAdapter;
use crate::net::device::LinkState;
use crate::net::ring::{DmaRegion, NetRing};
use crate::pci;

const REALTEK_VENDOR_ID: u16 = 0x10EC;
const RTL8125_DEVICE_ID: u16 = 0x8125;

const RX_DESC_COUNT: usize = 64;
const TX_DESC_COUNT: usize = 64;
const RX_BUF_SIZE: usize = 2048;
const TX_BUF_SIZE: usize = 2048;
const RX_POLL_BUDGET: usize = 32;
const TX_RING_FULL_LOG_EVERY: u64 = 256;
const RX_BAD_FLAGS_LOG_EVERY: u64 = 1024;
const TX_STALL_KICK_THRESHOLD: u64 = 10_000;
const TX_STALL_RESET_THRESHOLD: u64 = 50_000;
const POLL_STATE_LOG_EVERY: u64 = 10_000;
const TX_SUBMIT_DEBUG_FIRST: u64 = 4;
// Logging knobs: keep bring-up diagnostics available, but don't drown the
// console during normal operation.
const EXP_R8125_SKIP_DESC0: bool = false;
const EXP_R8125_TXPOLL_90_ENABLE: bool = true;
const EXP_R8125_TXPOLL_90_VALUE: u16 = 0x0001;
const EXP_R8125_TCR_OVERRIDE: Option<u32> = None;
const TX_DOORBELL_DEBUG_FIRST: u64 = 16;
const EXP_R8125_FORCE_CPLUS_OFF: bool = false;
// If DMA memory is mapped cacheable and the platform/device is not fully
// cache-coherent, we must write back TX descriptors/buffers before ringing the
// doorbell, and we may need to invalidate before reading back descriptor
// ownership during reclaim. This is cheap insurance for bring-up.
const EXP_R8125_CLFLUSH_TX_BUF: bool = false;
const EXP_R8125_CLFLUSH_TX_DESC: bool = true;
const EXP_R8125_CLFLUSH_TX_DESC_ON_RECLAIM: bool = true;
const TX_WEDGE_QUARANTINE_RESETS: u64 = 3;

// MMIO registers (RTL8125 family)
const REG_IDR0: u16 = 0x00; // MAC 0..5
const REG_TNPDS: u16 = 0x20; // Tx desc start addr (low)
const REG_TNPDS_HI: u16 = 0x24;
const REG_THPDS: u16 = 0x28;
const REG_THPDS_HI: u16 = 0x2C;
const REG_CMD: u16 = 0x37;
// RTL8125 uses different interrupt registers than RTL8168.
// See Linux r8169_main.c enum rtl8125_registers.
const REG_INTR_MASK_8125: u16 = 0x38; // u32
const REG_INTR_STATUS_8125: u16 = 0x3C; // u32
const REG_TXPOLL_90: u16 = 0x90; // u16, BIT(0) triggers TX poll
const REG_RCR: u16 = 0x44;
const REG_TCR: u16 = 0x40;
const REG_RDSAR: u16 = 0xE4; // Rx desc start addr (low)
const REG_RDSAR_HI: u16 = 0xE8;
const REG_CPLUS_CMD: u16 = 0xE0;
const REG_RX_MAX_SIZE: u16 = 0xDA;
const REG_PHYSTAT: u16 = 0x6C;
const REG_CFG9346: u16 = 0x50;

// RTL8125 init needs access to the "MCU" byte used for OOB (out-of-band) mode.
// See Linux r8169_main.c: MCU = 0xD3.
const REG_MCU: u16 = 0xD3;
const MCU_NOW_IS_OOB: u8 = 1 << 7;
const MCU_LINK_LIST_RDY: u8 = 1 << 1;

// MAC OCP access window (used heavily by Linux for 8125 bring-up).
const REG_OCPDR: u16 = 0xB0;
const OCPAR_FLAG: u32 = 0x8000_0000;

const CFG9346_LOCK: u8 = 0x00;
const CFG9346_UNLOCK: u8 = 0xC0;

const CMD_RX_EN: u8 = 1 << 3;
const CMD_TX_EN: u8 = 1 << 2;
const CMD_RST: u8 = 1 << 4;

const CPLUS_RX_CHKSUM: u16 = 1 << 1;
const CPLUS_ENABLE: u16 = 1 << 0;

// Bring-up toggles.
// `RX_ERR_SUM` is often raised for checksum-offload reporting; dropping all such
// frames can prevent DHCP from ever seeing offers/acks.
const ENABLE_RX_CHKSUM_OFFLOAD: bool = false;
const ACCEPT_RX_ERR_SUM_FRAMES: bool = true;
const STRIP_RX_CRC: bool = false;

// Descriptor bits
const DESC_OWN: u32 = 1 << 31;
const DESC_EOR: u32 = 1 << 30;
const RX_FS: u32 = 1 << 29;
const RX_LS: u32 = 1 << 28;
const RX_ERR_SUM: u32 = 1 << 27;

const TX_FS: u32 = 1 << 29;
const TX_LS: u32 = 1 << 28;

#[repr(C, packed)]
#[derive(Copy, Clone)]
struct RxDesc {
    opts1: u32,
    opts2: u32,
    addr: u64,
}

#[repr(C, packed)]
#[derive(Copy, Clone)]
struct TxDesc {
    opts1: u32,
    opts2: u32,
    addr: u64,
}

struct Mmio {
    base: NonNull<u8>,
}

// Safety: this is a mapped MMIO pointer used behind the net device mutex.
unsafe impl Send for Mmio {}

impl Mmio {
    #[inline]
    unsafe fn read_u8(&self, off: u16) -> u8 {
        read_volatile(self.base.as_ptr().add(off as usize) as *const u8)
    }

    #[inline]
    unsafe fn write_u8(&self, off: u16, val: u8) {
        write_volatile(self.base.as_ptr().add(off as usize) as *mut u8, val);
    }

    #[inline]
    unsafe fn read_u16(&self, off: u16) -> u16 {
        read_volatile(self.base.as_ptr().add(off as usize) as *const u16)
    }

    #[inline]
    unsafe fn write_u16(&self, off: u16, val: u16) {
        write_volatile(self.base.as_ptr().add(off as usize) as *mut u16, val);
    }

    #[inline]
    unsafe fn write_u32(&self, off: u16, val: u32) {
        write_volatile(self.base.as_ptr().add(off as usize) as *mut u32, val);
    }

    #[inline]
    unsafe fn read_u32(&self, off: u16) -> u32 {
        read_volatile(self.base.as_ptr().add(off as usize) as *const u32)
    }
}

pub struct R8125Adapter {
    mmio: Mmio,
    pci: pci::PciDevice,
    mac: [u8; 6],
    ring: Option<*mut NetRing>,

    _rx_desc_mem: DmaRegion,
    rx_desc: *mut RxDesc,
    rx_bufs: Vec<DmaRegion>,
    rx_idx: usize,

    _tx_desc_mem: DmaRegion,
    tx_desc_phys: u64,
    tx_desc: *mut TxDesc,
    tx_bufs: Vec<DmaRegion>,
    tx_head: usize,
    tx_tail: usize,

    // Bring-up instrumentation (kept lightweight; no high-rate logging)
    dbg_tx_submitted: u64,
    dbg_tx_reclaimed: u64,
    dbg_tx_ring_full: u64,
    dbg_tx_stall_checks: u64,
    dbg_tx_recovery_kicks: u64,
    dbg_tx_resets: u64,
    dbg_rx_ok: u64,
    dbg_rx_ring_full: u64,
    dbg_rx_bad_flags: u64,
    dbg_rx_errsum: u64,
    dbg_rx_len_bad: u64,
    dbg_last_phystat: u8,
    dbg_logged_first_tx: bool,
    dbg_logged_first_rx: bool,
    dbg_poll_ticks: u64,
    dbg_state_dumps: u64,
    dbg_isr_nonzero: u64,
    dbg_last_cmd: u8,
    dbg_last_imr: u32,
    dbg_last_tnpds_lo: u32,
    dbg_last_tnpds_hi: u32,
    dbg_kick_readbacks: u64,
    dbg_doorbells: u64,
    dbg_tx_quarantined: bool,

    dbg_tx_link_down_drops: u64,
}

// Safety: this adapter is driven by the net task and protected by the global net mutex.
unsafe impl Send for R8125Adapter {}

impl R8125Adapter {
    #[inline]
    fn phy_link_up(phystat: u8) -> bool {
        // Keep consistent with r8168 bring-up logging (bit0 = link up).
        (phystat & 0x01) != 0
    }

    #[inline]
    fn clflush_range(ptr: *const u8, len: usize) {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            use core::arch::x86_64::{_mm_clflush, _mm_mfence};

            if ptr.is_null() || len == 0 {
                return;
            }

            let line = 64usize;
            let start = (ptr as usize) & !(line - 1);
            let end = (ptr as usize).saturating_add(len);
            let mut p = start;
            while p < end {
                _mm_clflush(p as *const _);
                p = p.saturating_add(line);
            }
            _mm_mfence();
        }
    }

    #[inline]
    fn maybe_clflush(ptr: *const u8, len: usize, enabled: bool) {
        if enabled {
            Self::clflush_range(ptr, len);
        }
    }

    #[inline]
    fn tx_start_index() -> usize {
        if EXP_R8125_SKIP_DESC0 { 1 } else { 0 }
    }

    fn cplus_programmed(current: u16) -> u16 {
        let mut out = current;
        if EXP_R8125_FORCE_CPLUS_OFF {
            out &= !CPLUS_ENABLE;
            out &= !CPLUS_RX_CHKSUM;
        } else {
            out |= CPLUS_ENABLE;
            if ENABLE_RX_CHKSUM_OFFLOAD {
                out |= CPLUS_RX_CHKSUM;
            } else {
                out &= !CPLUS_RX_CHKSUM;
            }
        }
        out
    }

    fn ring_tx_doorbell(&mut self, reason: &str) {
        unsafe {
            // RTL8125 uses a different doorbell than RTL8168: a 16-bit TxPoll_8125
            // register where bit0 triggers a poll.
            if EXP_R8125_TXPOLL_90_ENABLE {
                self.mmio
                    .write_u16(REG_TXPOLL_90, EXP_R8125_TXPOLL_90_VALUE);
            }

            let poll90_rb = if EXP_R8125_TXPOLL_90_ENABLE {
                self.mmio.read_u16(REG_TXPOLL_90)
            } else {
                0
            };
            let cmd_rb = self.mmio.read_u8(REG_CMD);
            let isr_rb = self.mmio.read_u32(REG_INTR_STATUS_8125);
            let imr_rb = self.mmio.read_u32(REG_INTR_MASK_8125);

            self.dbg_doorbells = self.dbg_doorbells.saturating_add(1);
            if crate::logflag::R8125_VERBOSE_LOGS
                && (self.dbg_doorbells <= TX_DOORBELL_DEBUG_FIRST
                    || (self.dbg_doorbells & 0x3FF) == 0)
            {
                crate::log!(
                    "net/r8125: tx doorbell count={} reason={} poll90_rb=0x{:04x} cmd=0x{:02x} isr=0x{:08x} imr=0x{:08x}\n",
                    self.dbg_doorbells,
                    reason,
                    poll90_rb,
                    cmd_rb,
                    isr_rb,
                    imr_rb
                );
            }
        }
    }

    fn log_tx_window(&self, reason: &str) {
        let h = self.tx_head;
        let t = self.tx_tail;
        let n = (h + 1) % TX_DESC_COUNT;

        let hd = unsafe { read_volatile(self.tx_desc.add(h)) };
        let nd = unsafe { read_volatile(self.tx_desc.add(n)) };
        let td = unsafe { read_volatile(self.tx_desc.add(t)) };

        let hd_opts1 = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(hd.opts1)) };
        let hd_opts2 = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(hd.opts2)) };
        let nd_opts1 = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(nd.opts1)) };
        let td_opts1 = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(td.opts1)) };

        let hd_addr = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(hd.addr)) };
        let nd_addr = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(nd.addr)) };
        let td_addr = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(td.addr)) };

        crate::log!(
            "net/r8125: tx-window reason={} head={} tail={} next={} head[o1=0x{:08x} o2=0x{:08x} a=0x{:016x}] next[o1=0x{:08x} a=0x{:016x}] tail[o1=0x{:08x} a=0x{:016x}]\n",
            reason,
            h,
            t,
            n,
            hd_opts1,
            hd_opts2,
            hd_addr,
            nd_opts1,
            nd_addr,
            td_opts1,
            td_addr
        );
    }

    fn log_hw_state(&mut self, reason: &str) {
        self.dbg_state_dumps = self.dbg_state_dumps.saturating_add(1);

        let (cmd, isr, imr, rcr, tcr, cplus, rms, phy, rds_lo, rds_hi, tnp_lo, tnp_hi) = unsafe {
            (
                self.mmio.read_u8(REG_CMD),
                self.mmio.read_u32(REG_INTR_STATUS_8125),
                self.mmio.read_u32(REG_INTR_MASK_8125),
                self.mmio.read_u32(REG_RCR),
                self.mmio.read_u32(REG_TCR),
                self.mmio.read_u16(REG_CPLUS_CMD),
                self.mmio.read_u16(REG_RX_MAX_SIZE),
                self.mmio.read_u8(REG_PHYSTAT),
                self.mmio.read_u32(REG_RDSAR),
                self.mmio.read_u32(REG_RDSAR_HI),
                self.mmio.read_u32(REG_TNPDS),
                self.mmio.read_u32(REG_TNPDS_HI),
            )
        };

        let head_idx = self.tx_head;
        let tail_idx = self.tx_tail;
        let rx_idx = self.rx_idx;

        let tx_head_desc = unsafe { read_volatile(self.tx_desc.add(head_idx)) };
        let tx_tail_desc = unsafe { read_volatile(self.tx_desc.add(tail_idx)) };
        let rx_desc = unsafe { read_volatile(self.rx_desc.add(rx_idx)) };

        let tx_head_opts1 =
            unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(tx_head_desc.opts1)) };
        let tx_tail_opts1 =
            unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(tx_tail_desc.opts1)) };
        let rx_opts1 = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(rx_desc.opts1)) };

        crate::log!(
            "net/r8125: state reason={} dumps={} poll={} cmd=0x{:02x} isr=0x{:08x} imr=0x{:08x} phy=0x{:02x} rcr=0x{:08x} tcr=0x{:08x} cplus=0x{:04x} rxmax=0x{:04x} rdsar=0x{:08x}{:08x} tnpds=0x{:08x}{:08x} tx_desc_phys=0x{:016x} tx_head={} tx_tail={} tx_head_opts1=0x{:08x} tx_tail_opts1=0x{:08x} rx_idx={} rx_opts1=0x{:08x} tx_sub={} tx_rec={} tx_full={} tx_checks={} kicks={} resets={} rx_ok={} rx_drop={} rx_bad={} rx_errsum={} rx_len_bad={}\n",
            reason,
            self.dbg_state_dumps,
            self.dbg_poll_ticks,
            cmd,
            isr,
            imr,
            phy,
            rcr,
            tcr,
            cplus,
            rms,
            rds_hi,
            rds_lo,
            tnp_hi,
            tnp_lo,
            self.tx_desc_phys,
            head_idx,
            tail_idx,
            tx_head_opts1,
            tx_tail_opts1,
            rx_idx,
            rx_opts1,
            self.dbg_tx_submitted,
            self.dbg_tx_reclaimed,
            self.dbg_tx_ring_full,
            self.dbg_tx_stall_checks,
            self.dbg_tx_recovery_kicks,
            self.dbg_tx_resets,
            self.dbg_rx_ok,
            self.dbg_rx_ring_full,
            self.dbg_rx_bad_flags,
            self.dbg_rx_errsum,
            self.dbg_rx_len_bad
        );
    }

    pub fn init_all() -> alloc::vec::Vec<Self> {
        let mut out = alloc::vec::Vec::new();
        let devs = find_r8125_devices();
        for dev in devs {
            match Self::init_from_device(dev) {
                Ok(adapter) => out.push(adapter),
                Err(()) => {
                    crate::log!(
                        "net/r8125: init failed for {:02x}:{:02x}.{}\n",
                        dev.bus,
                        dev.slot,
                        dev.function
                    );
                }
            }
        }
        out
    }

    fn init_from_device(dev: pci::PciDevice) -> Result<Self, ()> {
        pci::enable_mem_and_bus_master(dev.bus, dev.slot, dev.function);
        let cmd = pci::config_read_u16(dev.bus, dev.slot, dev.function, 0x04);
        if crate::logflag::R8125_VERBOSE_LOGS {
            crate::log!("net/r8125: pci cmd=0x{:04x}\n", cmd);
        }

        let (bar_index, bar_phys) = find_mmio_bar_phys(&dev)?;
        let bar_size = pci::bar_size_bytes(dev.bus, dev.slot, dev.function, bar_index).unwrap_or(0);
        let map_size = match usize::try_from(bar_size) {
            Ok(size) if size != 0 => size,
            _ => {
                if crate::logflag::R8125_VERBOSE_LOGS {
                    crate::log!("net/r8125: bar{} size unknown; using 0x1000\n", bar_index);
                }
                0x1000
            }
        };
        if crate::logflag::R8125_VERBOSE_LOGS && bar_size != 0 {
            crate::log!("net/r8125: bar{} size=0x{:x}\n", bar_index, bar_size);
        }
        let mapped = match pci::mmio::map_mmio_region_exact(bar_phys, map_size) {
            Ok(mapped) => mapped,
            Err(err) => {
                crate::log!("net/r8125: bar{} mmio map failed: {:?}\n", bar_index, err);
                return Err(());
            }
        };
        let mmio = Mmio { base: mapped };

        if crate::logflag::R8125_VERBOSE_LOGS {
            crate::log!(
                "net/r8125: found {:02x}:{:02x}.{} vid={:04x} did={:04x} bar{}=0x{:x}\n",
                dev.bus,
                dev.slot,
                dev.function,
                dev.vendor,
                dev.device,
                bar_index,
                bar_phys
            );
        }

        // Reset
        let mut reset_done = false;
        let mut last_cmd: u8 = 0;
        unsafe {
            mmio.write_u8(REG_CMD, CMD_RST);
            for _ in 0..1_000_000 {
                last_cmd = mmio.read_u8(REG_CMD);
                if (last_cmd & CMD_RST) == 0 {
                    reset_done = true;
                    break;
                }
            }

            // Mask interrupts
            mmio.write_u32(REG_INTR_MASK_8125, 0);
            mmio.write_u32(REG_INTR_STATUS_8125, 0xFFFF_FFFF);
        }
        if !reset_done {
            crate::log!("net/r8125: reset timeout cmd=0x{:02x}\n", last_cmd);
            return Err(());
        }

        let mac = unsafe {
            let mut m = [0u8; 6];
            for i in 0..6 {
                m[i] = mmio.read_u8(REG_IDR0 + i as u16);
            }
            m
        };

        // Allocate descriptor rings
        let rx_desc_bytes = core::mem::size_of::<RxDesc>() * RX_DESC_COUNT;
        let tx_desc_bytes = core::mem::size_of::<TxDesc>() * TX_DESC_COUNT;
        if crate::logflag::R8125_VERBOSE_LOGS {
            crate::log!("net/r8125: alloc rx_desc bytes=0x{:x}\n", rx_desc_bytes);
        }
        let rx_desc_mem = DmaRegion::alloc(rx_desc_bytes, 256).ok_or(())?;
        if crate::logflag::R8125_VERBOSE_LOGS {
            crate::log!("net/r8125: alloc tx_desc bytes=0x{:x}\n", tx_desc_bytes);
        }
        let tx_desc_mem = DmaRegion::alloc(tx_desc_bytes, 256).ok_or(())?;

        let rx_desc_phys = rx_desc_mem.phys();
        let tx_desc_phys = tx_desc_mem.phys();
        if crate::logflag::R8125_VERBOSE_LOGS {
            crate::log!(
                "net/r8125: rx_desc phys=0x{:x} align256_ok={} tx_desc phys=0x{:x} align256_ok={}\n",
                rx_desc_phys,
                ((rx_desc_phys & 0xFF) == 0) as u8,
                tx_desc_phys,
                ((tx_desc_phys & 0xFF) == 0) as u8
            );
        }

        let rx_desc = rx_desc_mem.virt() as *mut RxDesc;
        let tx_desc = tx_desc_mem.virt() as *mut TxDesc;

        // Allocate buffers and initialize descriptors
        if crate::logflag::R8125_VERBOSE_LOGS {
            crate::log!(
                "net/r8125: alloc rx bufs count={} size=0x{:x}\n",
                RX_DESC_COUNT,
                RX_BUF_SIZE
            );
        }
        let mut rx_bufs: Vec<DmaRegion> = Vec::with_capacity(RX_DESC_COUNT);
        for i in 0..RX_DESC_COUNT {
            let buf = DmaRegion::alloc(RX_BUF_SIZE, 16).ok_or(())?;
            let eor = if i + 1 == RX_DESC_COUNT { DESC_EOR } else { 0 };
            unsafe {
                write_volatile(
                    rx_desc.add(i),
                    RxDesc {
                        opts1: DESC_OWN | eor | (RX_BUF_SIZE as u32 & 0x3FFF),
                        opts2: 0,
                        addr: buf.phys(),
                    },
                );
            }
            rx_bufs.push(buf);
        }

        if crate::logflag::R8125_VERBOSE_LOGS {
            crate::log!(
                "net/r8125: alloc tx bufs count={} size=0x{:x}\n",
                TX_DESC_COUNT,
                TX_BUF_SIZE
            );
        }
        let mut tx_bufs: Vec<DmaRegion> = Vec::with_capacity(TX_DESC_COUNT);
        for i in 0..TX_DESC_COUNT {
            let buf = DmaRegion::alloc(TX_BUF_SIZE, 16).ok_or(())?;
            let eor = if i + 1 == TX_DESC_COUNT { DESC_EOR } else { 0 };
            unsafe {
                write_volatile(
                    tx_desc.add(i),
                    TxDesc {
                        opts1: eor,
                        opts2: 0,
                        addr: buf.phys(),
                    },
                );
            }
            tx_bufs.push(buf);
        }

        // Program descriptor bases + enable C+ mode.
        unsafe {
            // Stop engines while programming baseline datapath registers.
            mmio.write_u8(REG_CMD, 0);
            mmio.write_u32(REG_INTR_MASK_8125, 0);
            mmio.write_u32(REG_INTR_STATUS_8125, 0xFFFF_FFFF);

            // Ensure the device is not stuck in OOB mode. When NOW_IS_OOB is
            // set, TX/RX DMA may not behave as expected.
            let mcu0 = mmio.read_u8(REG_MCU);
            mmio.write_u8(REG_MCU, mcu0 & !MCU_NOW_IS_OOB);
            let mut saw_ll = false;
            for _ in 0..200_000 {
                let mcu = mmio.read_u8(REG_MCU);
                if (mcu & MCU_LINK_LIST_RDY) != 0 {
                    saw_ll = true;
                    break;
                }
            }
            let mcu1 = mmio.read_u8(REG_MCU);
            if crate::logflag::R8125_VERBOSE_LOGS {
                crate::log!(
                    "net/r8125: mcu oob mcu0=0x{:02x} mcu1=0x{:02x} llrdy={}\n",
                    mcu0,
                    mcu1,
                    saw_ll as u8
                );
            }

            // Minimal RTL8125 MAC OCP init (from Linux rtl_hw_init_8125):
            // these appear to be required on some boards for stable DMA.
            mmio.write_u32(REG_OCPDR, OCPAR_FLAG | ((0xc0aa_u32) << 15) | 0x07d0);
            mmio.write_u32(REG_OCPDR, OCPAR_FLAG | ((0xc0a6_u32) << 15) | 0x0150);

            // Realtek MAC registers are often write-protected behind CFG9346.
            // If we don't unlock, writes like TCR/RCR may be ignored.
            mmio.write_u8(REG_CFG9346, CFG9346_UNLOCK);

            // C+ mode on (descriptor mode). Keep it minimal.
            let cplus = mmio.read_u16(REG_CPLUS_CMD);
            let cplus_new = Self::cplus_programmed(cplus);
            mmio.write_u16(REG_CPLUS_CMD, cplus_new);
            mmio.write_u16(REG_RX_MAX_SIZE, RX_BUF_SIZE as u16);

            // Descriptor ring addresses
            mmio.write_u32(REG_RDSAR, rx_desc_phys as u32);
            mmio.write_u32(REG_RDSAR_HI, (rx_desc_phys >> 32) as u32);
            mmio.write_u32(REG_TNPDS, tx_desc_phys as u32);
            mmio.write_u32(REG_TNPDS_HI, (tx_desc_phys >> 32) as u32);
            mmio.write_u32(REG_THPDS, tx_desc_phys as u32);
            mmio.write_u32(REG_THPDS_HI, (tx_desc_phys >> 32) as u32);

            // Basic RX/TX config (promiscuous off; accept broadcast/multicast).
            // Values here are intentionally conservative for bring-up.
            mmio.write_u32(REG_RCR, 0x0000E70F);
            let tcr = EXP_R8125_TCR_OVERRIDE.unwrap_or(0x03000700);
            mmio.write_u32(REG_TCR, tcr);

            // Lock config back down.
            mmio.write_u8(REG_CFG9346, CFG9346_LOCK);

            // Enable Rx/Tx
            mmio.write_u8(REG_CMD, CMD_RX_EN | CMD_TX_EN);
            mmio.write_u32(REG_INTR_STATUS_8125, 0xFFFF_FFFF);
        }

        // Confirm key registers took effect (helps diagnose write-protect / wrong offsets).
        let (rcr_rb, tcr_rb, cplus_rb) = unsafe {
            (mmio.read_u32(REG_RCR), mmio.read_u32(REG_TCR), mmio.read_u16(REG_CPLUS_CMD))
        };
        if crate::logflag::R8125_VERBOSE_LOGS {
            crate::log!(
                "net/r8125: cfg rb rcr=0x{:08x} tcr=0x{:08x} cplus=0x{:04x}\n",
                rcr_rb,
                tcr_rb,
                cplus_rb
            );

            crate::log!(
                "net/r8125: mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}\n",
                mac[0],
                mac[1],
                mac[2],
                mac[3],
                mac[4],
                mac[5]
            );

            crate::log!(
                "net/r8125: caps speeds=10/100/1000/2500 duplex=full/half flow=tx/rx ring=rx{} tx{} mtu=1500\n",
                RX_DESC_COUNT,
                TX_DESC_COUNT
            );
        }
        let cplus_after = unsafe { mmio.read_u16(REG_CPLUS_CMD) };
        if crate::logflag::R8125_VERBOSE_LOGS {
            crate::log!(
                "net/r8125: cplus=0x{:04x} force_off={}\n",
                cplus_after,
                EXP_R8125_FORCE_CPLUS_OFF as u8
            );
            crate::log!(
                "net/r8125: tx start idx={} skip_desc0={}\n",
                Self::tx_start_index(),
                EXP_R8125_SKIP_DESC0 as u8
            );
        }
        let phy = unsafe { mmio.read_u8(REG_PHYSTAT) };
        if crate::logflag::R8125_VERBOSE_LOGS {
            crate::log!("net/r8125: phystat=0x{:02x} (raw)\n", phy);
        }

        Ok(Self {
            mmio,
            pci: dev,
            mac,
            ring: None,
            _rx_desc_mem: rx_desc_mem,
            rx_desc,
            rx_bufs,
            rx_idx: 0,
            _tx_desc_mem: tx_desc_mem,
            tx_desc_phys,
            tx_desc,
            tx_bufs,
            tx_head: Self::tx_start_index(),
            tx_tail: Self::tx_start_index(),

            dbg_tx_submitted: 0,
            dbg_tx_reclaimed: 0,
            dbg_tx_ring_full: 0,
            dbg_tx_stall_checks: 0,
            dbg_tx_recovery_kicks: 0,
            dbg_tx_resets: 0,
            dbg_rx_ok: 0,
            dbg_rx_ring_full: 0,
            dbg_rx_bad_flags: 0,
            dbg_rx_errsum: 0,
            dbg_rx_len_bad: 0,
            dbg_last_phystat: phy,
            dbg_logged_first_tx: false,
            dbg_logged_first_rx: false,
            dbg_poll_ticks: 0,
            dbg_state_dumps: 0,
            dbg_isr_nonzero: 0,
            dbg_last_cmd: CMD_RX_EN | CMD_TX_EN,
            dbg_last_imr: 0,
            dbg_last_tnpds_lo: tx_desc_phys as u32,
            dbg_last_tnpds_hi: (tx_desc_phys >> 32) as u32,
            dbg_kick_readbacks: 0,
            dbg_doorbells: 0,
            dbg_tx_quarantined: false,

            dbg_tx_link_down_drops: 0,
        })
    }

    fn kick_tx_engine(&mut self) {
        self.dbg_tx_recovery_kicks = self.dbg_tx_recovery_kicks.saturating_add(1);
        unsafe {
            self.mmio.write_u32(REG_TNPDS, self.tx_desc_phys as u32);
            self.mmio
                .write_u32(REG_TNPDS_HI, (self.tx_desc_phys >> 32) as u32);
            self.mmio.write_u32(REG_THPDS, self.tx_desc_phys as u32);
            self.mmio
                .write_u32(REG_THPDS_HI, (self.tx_desc_phys >> 32) as u32);

            let cmd = self.mmio.read_u8(REG_CMD);
            self.mmio.write_u8(REG_CMD, cmd | CMD_TX_EN | CMD_RX_EN);
        }

        self.ring_tx_doorbell("kick");

        unsafe {
            let rb_cmd = self.mmio.read_u8(REG_CMD);
            let rb_isr = self.mmio.read_u32(REG_INTR_STATUS_8125);
            let rb_tnp_lo = self.mmio.read_u32(REG_TNPDS);
            let rb_tnp_hi = self.mmio.read_u32(REG_TNPDS_HI);

            self.dbg_kick_readbacks = self.dbg_kick_readbacks.saturating_add(1);
            if self.dbg_kick_readbacks <= 8 || (self.dbg_kick_readbacks & 0x3FF) == 0 {
                crate::log!(
                    "net/r8125: tx kick rb count={} cmd=0x{:02x} isr=0x{:08x} tnpds=0x{:08x}{:08x}\n",
                    self.dbg_kick_readbacks,
                    rb_cmd,
                    rb_isr,
                    rb_tnp_hi,
                    rb_tnp_lo
                );
            }
        }
    }

    fn reset_tx_ring_controlled(&mut self, reason: &str) {
        if self.dbg_tx_quarantined {
            return;
        }

        self.dbg_tx_resets = self.dbg_tx_resets.saturating_add(1);

        let (cmd, tcr, tn_lo, tn_hi, isr, phy) = unsafe {
            (
                self.mmio.read_u8(REG_CMD),
                self.mmio.read_u32(REG_TCR),
                self.mmio.read_u32(REG_TNPDS),
                self.mmio.read_u32(REG_TNPDS_HI),
                self.mmio.read_u32(REG_INTR_STATUS_8125),
                self.mmio.read_u8(REG_PHYSTAT),
            )
        };

        crate::log!(
            "net/r8125: tx reset reason={} resets={} head={} tail={} checks={} cmd=0x{:02x} tcr=0x{:08x} tnpds=0x{:08x}{:08x} isr=0x{:08x} phystat=0x{:02x}\n",
            reason,
            self.dbg_tx_resets,
            self.tx_head,
            self.tx_tail,
            self.dbg_tx_stall_checks,
            cmd,
            tcr,
            tn_hi,
            tn_lo,
            isr,
            phy
        );
        self.log_tx_window("tx-reset-pre");
        self.log_hw_state("tx-reset");

        unsafe {
            let cmd_now = self.mmio.read_u8(REG_CMD);
            self.mmio.write_u8(REG_CMD, cmd_now & !CMD_TX_EN);

            for i in 0..TX_DESC_COUNT {
                let eor = if i + 1 == TX_DESC_COUNT { DESC_EOR } else { 0 };
                write_volatile(
                    self.tx_desc.add(i),
                    TxDesc {
                        opts1: eor,
                        opts2: 0,
                        addr: self.tx_bufs[i].phys(),
                    },
                );
            }

            fence(Ordering::Release);

            self.mmio.write_u32(REG_TNPDS, self.tx_desc_phys as u32);
            self.mmio
                .write_u32(REG_TNPDS_HI, (self.tx_desc_phys >> 32) as u32);
            self.mmio.write_u32(REG_THPDS, self.tx_desc_phys as u32);
            self.mmio
                .write_u32(REG_THPDS_HI, (self.tx_desc_phys >> 32) as u32);

            self.tx_head = Self::tx_start_index();
            self.tx_tail = Self::tx_start_index();
            self.dbg_tx_stall_checks = 0;

            let cmd_re = self.mmio.read_u8(REG_CMD);
            self.mmio.write_u8(REG_CMD, cmd_re | CMD_TX_EN | CMD_RX_EN);
        }

        self.ring_tx_doorbell("tx-reset-reinit");

        crate::log!(
            "net/r8125: tx reset reinit head={} tail={} skip_desc0={}\n",
            self.tx_head,
            self.tx_tail,
            EXP_R8125_SKIP_DESC0 as u8
        );

        if self.dbg_tx_reclaimed == 0 && self.dbg_tx_resets >= TX_WEDGE_QUARANTINE_RESETS {
            self.dbg_tx_quarantined = true;
            crate::log!(
                "net/r8125: tx quarantined after resets={} (no reclaims); rx remains active\n",
                self.dbg_tx_resets
            );
            unsafe {
                let cmd_now = self.mmio.read_u8(REG_CMD);
                self.mmio.write_u8(REG_CMD, cmd_now & !CMD_TX_EN);
            }
        }
    }

    fn reclaim_tx(&mut self) {
        if self.dbg_tx_quarantined {
            return;
        }

        while self.tx_head != self.tx_tail {
            let idx = self.tx_head;

            // If the device clears OWN in memory but we keep a cached copy of the
            // descriptor, we will incorrectly believe TX is wedged forever.
            if EXP_R8125_CLFLUSH_TX_DESC_ON_RECLAIM {
                let desc_ptr = unsafe { self.tx_desc.add(idx) } as *const u8;
                Self::maybe_clflush(desc_ptr, core::mem::size_of::<TxDesc>(), true);
            }

            let desc = unsafe { read_volatile(self.tx_desc.add(idx)) };
            let desc_opts1 = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(desc.opts1)) };
            if (desc_opts1 & DESC_OWN) != 0 {
                // Seeing OWN set immediately after submission is normal; only treat it as a
                // stall if it persists across many polls.
                self.dbg_tx_stall_checks = self.dbg_tx_stall_checks.saturating_add(1);
                if self.dbg_tx_submitted != 0
                    && self
                        .dbg_tx_stall_checks
                        .is_multiple_of(TX_STALL_KICK_THRESHOLD)
                {
                    crate::log!(
                        "net/r8125: tx stall checks={} head={} tail={} desc_opts1=0x{:08x} kicks={} resets={}\n",
                        self.dbg_tx_stall_checks,
                        self.tx_head,
                        self.tx_tail,
                        desc_opts1,
                        self.dbg_tx_recovery_kicks,
                        self.dbg_tx_resets
                    );
                    self.log_tx_window("tx-stall");
                    self.log_hw_state("tx-stall");
                    self.kick_tx_engine();
                }

                if self.dbg_tx_submitted != 0
                    && self.dbg_tx_stall_checks >= TX_STALL_RESET_THRESHOLD
                {
                    self.reset_tx_ring_controlled("stall-threshold");
                }
                break;
            }
            self.tx_head = (self.tx_head + 1) % TX_DESC_COUNT;

            self.dbg_tx_stall_checks = 0;

            self.dbg_tx_reclaimed = self.dbg_tx_reclaimed.saturating_add(1);
            if self.dbg_tx_reclaimed == 1 {
                crate::log!("net/r8125: first tx reclaim\n");
            }
        }
    }

    fn poll_rx_ring(&mut self) {
        self.dbg_poll_ticks = self.dbg_poll_ticks.saturating_add(1);

        if crate::logflag::R8125_VERBOSE_LOGS
            && self.dbg_poll_ticks.is_multiple_of(POLL_STATE_LOG_EVERY)
        {
            self.log_hw_state("periodic");
        }

        let cmd_now = unsafe { self.mmio.read_u8(REG_CMD) };
        let imr_now = unsafe { self.mmio.read_u32(REG_INTR_MASK_8125) };
        let tnp_lo_now = unsafe { self.mmio.read_u32(REG_TNPDS) };
        let tnp_hi_now = unsafe { self.mmio.read_u32(REG_TNPDS_HI) };

        if cmd_now != self.dbg_last_cmd
            || imr_now != self.dbg_last_imr
            || tnp_lo_now != self.dbg_last_tnpds_lo
            || tnp_hi_now != self.dbg_last_tnpds_hi
        {
            let old_cmd = self.dbg_last_cmd;
            let old_imr = self.dbg_last_imr;
            let old_tnp_lo = self.dbg_last_tnpds_lo;
            let old_tnp_hi = self.dbg_last_tnpds_hi;

            self.dbg_last_cmd = cmd_now;
            self.dbg_last_imr = imr_now;
            self.dbg_last_tnpds_lo = tnp_lo_now;
            self.dbg_last_tnpds_hi = tnp_hi_now;

            if crate::logflag::R8125_VERBOSE_LOGS {
                crate::log!(
                    "net/r8125: reg change cmd 0x{:02x}->0x{:02x} imr 0x{:08x}->0x{:08x} tnpds 0x{:08x}{:08x}->0x{:08x}{:08x}\n",
                    old_cmd,
                    cmd_now,
                    old_imr,
                    imr_now,
                    old_tnp_hi,
                    old_tnp_lo,
                    tnp_hi_now,
                    tnp_lo_now
                );
                self.log_hw_state("reg-change");
            }
        }

        // Track PHY/link changes without spamming: only log on change.
        let phy = unsafe { self.mmio.read_u8(REG_PHYSTAT) };
        if phy != self.dbg_last_phystat {
            self.dbg_last_phystat = phy;
            if crate::logflag::R8125_VERBOSE_LOGS {
                crate::log!(
                    "net/r8125: phystat=0x{:02x} (changed) link_bit0={}\n",
                    phy,
                    Self::phy_link_up(phy) as u8
                );
                self.log_hw_state("phystat-change");
            }
        }

        let isr = unsafe { self.mmio.read_u32(REG_INTR_STATUS_8125) };
        if isr != 0 {
            self.dbg_isr_nonzero = self.dbg_isr_nonzero.saturating_add(1);
            // ISR can be chatty (e.g. link-related or RX OK); keep a small sample
            // and then only very occasionally.
            if crate::logflag::R8125_VERBOSE_LOGS
                && (self.dbg_isr_nonzero <= 2 || (self.dbg_isr_nonzero & 0xFFF) == 0)
            {
                crate::log!(
                    "net/r8125: isr nonzero count={} isr=0x{:08x}\n",
                    self.dbg_isr_nonzero,
                    isr
                );
                self.log_hw_state("isr-nonzero");
            }
            unsafe {
                self.mmio.write_u32(REG_INTR_STATUS_8125, isr);
            }
        }

        let Some(ring_ptr) = self.ring else {
            // Still reclaim TX even if not bound yet.
            self.reclaim_tx();
            return;
        };

        let mut processed = 0usize;
        loop {
            if processed >= RX_POLL_BUDGET {
                break;
            }
            let idx = self.rx_idx;
            let desc = unsafe { read_volatile(self.rx_desc.add(idx)) };

            let opts1 = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(desc.opts1)) };

            if (opts1 & DESC_OWN) != 0 {
                break;
            }

            let had_errsum = (opts1 & RX_ERR_SUM) != 0;

            if (opts1 & (RX_FS | RX_LS)) != (RX_FS | RX_LS) {
                self.dbg_rx_bad_flags = self.dbg_rx_bad_flags.saturating_add(1);
                if self.dbg_rx_bad_flags == 1
                    || self.dbg_rx_bad_flags.is_multiple_of(RX_BAD_FLAGS_LOG_EVERY)
                {
                    crate::log!(
                        "net/r8125: rx flags missing fs/ls count={} opts1=0x{:08x} (continuing)\n",
                        self.dbg_rx_bad_flags,
                        opts1
                    );
                }
            }

            let raw_len = (opts1 & 0x3FFF) as usize;

            if had_errsum {
                self.dbg_rx_errsum = self.dbg_rx_errsum.saturating_add(1);
                if !ACCEPT_RX_ERR_SUM_FRAMES {
                    let eor = opts1 & DESC_EOR;
                    unsafe {
                        write_volatile(
                            self.rx_desc.add(idx),
                            RxDesc {
                                opts1: DESC_OWN | eor | (RX_BUF_SIZE as u32 & 0x3FFF),
                                opts2: 0,
                                addr: self.rx_bufs[idx].phys(),
                            },
                        );
                    }
                    self.rx_idx = (self.rx_idx + 1) % RX_DESC_COUNT;
                    processed += 1;
                    continue;
                }
            }

            if raw_len == 0 || raw_len > RX_BUF_SIZE {
                self.dbg_rx_len_bad = self.dbg_rx_len_bad.saturating_add(1);
                if self.dbg_rx_len_bad == 1 {
                    crate::log!(
                        "net/r8125: rx bad len raw_len={} opts1=0x{:08x}\n",
                        raw_len,
                        opts1
                    );
                }
                let eor = opts1 & DESC_EOR;
                unsafe {
                    write_volatile(
                        self.rx_desc.add(idx),
                        RxDesc {
                            opts1: DESC_OWN | eor | (RX_BUF_SIZE as u32 & 0x3FFF),
                            opts2: 0,
                            addr: self.rx_bufs[idx].phys(),
                        },
                    );
                }
                self.rx_idx = (self.rx_idx + 1) % RX_DESC_COUNT;
                processed += 1;
                continue;
            }
            let mut len = raw_len;
            if STRIP_RX_CRC && len >= 4 {
                len -= 4;
            }
            len = min(len, RX_BUF_SIZE);

            let buf_ptr = self.rx_bufs[idx].virt();
            let data = unsafe { core::slice::from_raw_parts(buf_ptr as *const u8, len) };

            unsafe {
                let ring = &mut *ring_ptr;
                if ring.push_rx_packet(data).is_err() {
                    self.dbg_rx_ring_full = self.dbg_rx_ring_full.saturating_add(1);
                    if self.dbg_rx_ring_full == 1 {
                        crate::log!("net/r8125: rx ring full (dropping)\n");
                    }
                } else {
                    self.dbg_rx_ok = self.dbg_rx_ok.saturating_add(1);
                    if !self.dbg_logged_first_rx {
                        self.dbg_logged_first_rx = true;
                        crate::log!(
                            "net/r8125: first rx len={} raw_len={} opts1=0x{:08x}\n",
                            len,
                            raw_len,
                            opts1
                        );
                    }
                }
            }

            // Re-arm descriptor
            let eor = opts1 & DESC_EOR;
            unsafe {
                write_volatile(
                    self.rx_desc.add(idx),
                    RxDesc {
                        opts1: DESC_OWN | eor | (RX_BUF_SIZE as u32 & 0x3FFF),
                        opts2: 0,
                        addr: self.rx_bufs[idx].phys(),
                    },
                );
            }

            self.rx_idx = (self.rx_idx + 1) % RX_DESC_COUNT;
            processed += 1;
        }

        self.reclaim_tx();
    }

    fn transmit_hw(&mut self, frame: &[u8]) -> Result<(), ()> {
        if frame.is_empty() {
            return Ok(());
        }

        if self.dbg_tx_quarantined {
            return Err(());
        }

        // When link is down (e.g. cable unplugged), many Realtek parts won't
        // complete TX descriptors. Avoid queueing OWN descriptors in that case.
        let phy = unsafe { self.mmio.read_u8(REG_PHYSTAT) };
        if !Self::phy_link_up(phy) {
            self.dbg_tx_link_down_drops = self.dbg_tx_link_down_drops.saturating_add(1);
            if self.dbg_tx_link_down_drops <= 8 || (self.dbg_tx_link_down_drops & 0x3FF) == 0 {
                crate::log!(
                    "net/r8125: drop tx (link down) count={} phystat=0x{:02x}\n",
                    self.dbg_tx_link_down_drops,
                    phy
                );
            }
            return Err(());
        }

        // Don't rely on RX polling cadence to free TX descriptors.
        self.reclaim_tx();

        let len = min(frame.len(), TX_BUF_SIZE);
        let next_tail = (self.tx_tail + 1) % TX_DESC_COUNT;
        if next_tail == self.tx_head {
            self.dbg_tx_ring_full = self.dbg_tx_ring_full.saturating_add(1);
            self.kick_tx_engine();
            self.reclaim_tx();
            if (self.tx_tail + 1) % TX_DESC_COUNT == self.tx_head {
                if self.dbg_tx_ring_full == 1
                    || self.dbg_tx_ring_full.is_multiple_of(TX_RING_FULL_LOG_EVERY)
                {
                    let (cmd, tcr, tn_lo, tn_hi, isr, phy) = unsafe {
                        (
                            self.mmio.read_u8(REG_CMD),
                            self.mmio.read_u32(REG_TCR),
                            self.mmio.read_u32(REG_TNPDS),
                            self.mmio.read_u32(REG_TNPDS_HI),
                            self.mmio.read_u32(REG_INTR_STATUS_8125),
                            self.mmio.read_u8(REG_PHYSTAT),
                        )
                    };
                    crate::log!(
                        "net/r8125: tx ring full count={} head={} tail={} cmd=0x{:02x} tcr=0x{:08x} tnpds=0x{:08x}{:08x} isr=0x{:08x} phystat=0x{:02x} kicks={}\n",
                        self.dbg_tx_ring_full,
                        self.tx_head,
                        self.tx_tail,
                        cmd,
                        tcr,
                        tn_hi,
                        tn_lo,
                        isr,
                        phy,
                        self.dbg_tx_recovery_kicks
                    );
                    self.log_tx_window("tx-ring-full");
                    self.log_hw_state("tx-ring-full");
                }
                return Err(());
            }
        }

        let idx = self.tx_tail;
        let cur = unsafe { read_volatile(self.tx_desc.add(idx)) };
        let cur_opts1 = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(cur.opts1)) };
        if (cur_opts1 & DESC_OWN) != 0 {
            self.dbg_tx_ring_full = self.dbg_tx_ring_full.saturating_add(1);
            self.kick_tx_engine();
            self.reclaim_tx();

            let cur2 = unsafe { read_volatile(self.tx_desc.add(idx)) };
            let cur2_opts1 = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(cur2.opts1)) };
            if (cur2_opts1 & DESC_OWN) != 0 {
                if self.dbg_tx_ring_full == 1
                    || self.dbg_tx_ring_full.is_multiple_of(TX_RING_FULL_LOG_EVERY)
                {
                    crate::log!(
                        "net/r8125: tx desc busy count={} idx={} head={} tail={} opts1=0x{:08x} kicks={}\n",
                        self.dbg_tx_ring_full,
                        idx,
                        self.tx_head,
                        self.tx_tail,
                        cur2_opts1,
                        self.dbg_tx_recovery_kicks
                    );
                    self.log_tx_window("tx-desc-busy");
                    self.log_hw_state("tx-desc-busy");
                }
                return Err(());
            }
        }

        unsafe {
            core::ptr::copy_nonoverlapping(frame.as_ptr(), self.tx_bufs[idx].virt(), len);
        }

        Self::maybe_clflush(self.tx_bufs[idx].virt() as *const u8, len, EXP_R8125_CLFLUSH_TX_BUF);

        // Ensure the packet bytes are visible before we set DESC_OWN.
        compiler_fence(Ordering::Release);

        let eor = if idx + 1 == TX_DESC_COUNT {
            DESC_EOR
        } else {
            0
        };
        let opts1 = DESC_OWN | eor | TX_FS | TX_LS | (len as u32 & 0x3FFF);
        unsafe {
            write_volatile(
                self.tx_desc.add(idx),
                TxDesc {
                    opts1,
                    opts2: 0,
                    addr: self.tx_bufs[idx].phys(),
                },
            );

            // Ensure descriptor writes are visible before we kick the device.
            fence(Ordering::Release);
        }

        {
            let desc_ptr = unsafe { self.tx_desc.add(idx) } as *const u8;
            Self::maybe_clflush(
                desc_ptr,
                core::mem::size_of::<TxDesc>(),
                EXP_R8125_CLFLUSH_TX_DESC,
            );
            if self.dbg_tx_submitted < TX_SUBMIT_DEBUG_FIRST {
                crate::log!(
                    "net/r8125: tx clflush idx={} len={} buf={} desc={} reclaim_inv={}\n",
                    idx,
                    len,
                    EXP_R8125_CLFLUSH_TX_BUF as u8,
                    EXP_R8125_CLFLUSH_TX_DESC as u8,
                    EXP_R8125_CLFLUSH_TX_DESC_ON_RECLAIM as u8
                );
            }
        }

        self.ring_tx_doorbell("tx-submit");

        if self.dbg_tx_submitted < TX_SUBMIT_DEBUG_FIRST {
            let post = unsafe { read_volatile(self.tx_desc.add(idx)) };
            let post_o1 = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(post.opts1)) };
            let post_o2 = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(post.opts2)) };
            let post_a = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(post.addr)) };
            let (cmd, isr, tnp_lo, tnp_hi) = unsafe {
                (
                    self.mmio.read_u8(REG_CMD),
                    self.mmio.read_u32(REG_INTR_STATUS_8125),
                    self.mmio.read_u32(REG_TNPDS),
                    self.mmio.read_u32(REG_TNPDS_HI),
                )
            };

            crate::log!(
                "net/r8125: tx submit dbg idx={} len={} opts1=0x{:08x} rd[o1=0x{:08x} o2=0x{:08x} a=0x{:016x}] cmd=0x{:02x} isr=0x{:08x} tnpds=0x{:08x}{:08x}\n",
                idx,
                len,
                opts1,
                post_o1,
                post_o2,
                post_a,
                cmd,
                isr,
                tnp_hi,
                tnp_lo
            );
        }

        self.tx_tail = next_tail;
        self.dbg_tx_submitted = self.dbg_tx_submitted.saturating_add(1);
        if !self.dbg_logged_first_tx {
            self.dbg_logged_first_tx = true;
            crate::log!(
                "net/r8125: first tx len={} head={} tail={}\n",
                len,
                self.tx_head,
                self.tx_tail
            );
        }
        Ok(())
    }
}

impl VendorAdapter for R8125Adapter {
    fn mac(&self) -> [u8; 6] {
        self.mac
    }

    fn poll_rx(&mut self) {
        self.poll_rx_ring();
    }

    fn pop_rx(&mut self) -> Option<Vec<u8>> {
        None
    }

    fn transmit(&mut self, frame: &[u8]) -> Result<(), ()> {
        self.transmit_hw(frame)
    }

    fn link_state(&self) -> LinkState {
        let phy = unsafe { self.mmio.read_u8(REG_PHYSTAT) };
        LinkState {
            up: (phy & 0x01) != 0,
            speed_mbps: 0,
            full_duplex: false,
        }
    }

    #[inline]
    fn pci_device(&self) -> Option<pci::PciDevice> {
        Some(self.pci)
    }

    fn bind_ring(&mut self, ring: *mut NetRing) {
        self.ring = Some(ring);
    }
}

fn find_r8125_devices() -> alloc::vec::Vec<pci::PciDevice> {
    let mut out = alloc::vec::Vec::new();
    pci::with_devices(|list| {
        for dev in list {
            if dev.vendor != REALTEK_VENDOR_ID {
                continue;
            }
            if dev.device != RTL8125_DEVICE_ID {
                continue;
            }
            if dev.class != 0x02 {
                continue;
            }
            out.push(*dev);
        }
    });
    out
}

fn find_mmio_bar_phys(dev: &pci::PciDevice) -> Result<(u8, u64), ()> {
    let mut i = 0u8;
    while i < 6 {
        let (bar_lo, bar_hi) = pci::read_bar_raw(dev.bus, dev.slot, dev.function, i);
        if bar_lo == 0 {
            i += 1;
            continue;
        }
        if (bar_lo & 0x1) != 0 {
            if crate::logflag::R8125_VERBOSE_LOGS {
                crate::log!("net/r8125: bar{} is IO (raw=0x{:08x})\n", i, bar_lo);
            }
            i += 1;
            continue;
        }

        let is_64 = ((bar_lo >> 1) & 0x3) == 0x2;
        let lo = (bar_lo as u64) & !0xFu64;
        let hi = bar_hi.unwrap_or(0) as u64;
        let phys = lo | (hi << 32);
        if phys == 0 {
            if crate::logflag::R8125_VERBOSE_LOGS {
                crate::log!("net/r8125: bar{} is zero\n", i);
            }
            i += 1;
            continue;
        }

        if crate::logflag::R8125_VERBOSE_LOGS {
            crate::log!(
                "net/r8125: bar{} mmio raw=0x{:08x}{} => 0x{:x}\n",
                i,
                bar_lo,
                if is_64 { " (64)" } else { "" },
                phys
            );
        }

        return Ok((i, phys));
    }
    Err(())
}
