use alloc::vec::Vec;

use core::cmp::min;
use core::ptr::{NonNull, read_volatile, write_volatile};
use core::sync::atomic::{Ordering, compiler_fence, fence};
use spin::Mutex;

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
const RX_DESC_UNAVAILABLE_WARN_EVERY: u64 = 65_536;
const TX_SUBMIT_DEBUG_FIRST: u64 = 4;
// Logging knobs: keep bring-up diagnostics available, but don't drown the
// console during normal operation.
const EXP_R8125_SKIP_DESC0: bool = false;
const EXP_R8125_TXPOLL_90_ENABLE: bool = true;
const EXP_R8125_TXPOLL_90_VALUE: u16 = 0x0001;
const EXP_R8125_TCR_OVERRIDE: Option<u32> = None;
const TX_DOORBELL_DEBUG_FIRST: u64 = 16;
const RX_TRACE_EARLY_POLLS: u64 = 8;
const RX_TRACE_POLL_EVERY: u64 = 10_000;
const RX_TRACE_EARLY_FRAMES: u64 = 8;
const MAC_TRACE_POLL_EVERY: u64 = 10_000;
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
const REG_MAR0: u16 = 0x08; // Multicast hash bits 0..31
const REG_MAR4: u16 = 0x0C; // Multicast hash bits 32..63
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
const REG_CONFIG3: u16 = 0x54;
const REG_CONFIG5: u16 = 0x56;

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

// RTL8125 interrupt-status bit 4 is named RxDescUnavail by Realtek's vendor
// driver (the generic Linux r8169 enum calls the same bit RxOverflow).
const ISR_RX_DESC_UNAVAILABLE: u32 = 1 << 4;

const CPLUS_RX_CHKSUM: u16 = 1 << 1;
const CPLUS_ENABLE: u16 = 1 << 0;

// Receive configuration. RTL8125 uses the fetch field at bits 30:27 instead
// of the legacy FIFO threshold used by older RTL8168-family controllers.
const RCR_RX_FETCH_DFLT_8125: u32 = 8 << 27;
const RCR_RX_FETCH_MASK: u32 = 0x0f << 27;
const RCR_COMPAT_FIFO_MASK: u32 = 7 << 13;
const RCR_VLAN_DETAG_MASK: u32 = (1 << 23) | (1 << 22);
const RCR_RX_PAUSE_SLOT_ON: u32 = 1 << 11; // RTL8125B and later
const RCR_RX_DMA_BURST: u32 = 7 << 8;
const RCR_RX_DMA_BURST_MASK: u32 = 7 << 8;
const RCR_ACCEPT_ERROR_MASK: u32 = (1 << 5) | (1 << 4);
const RCR_ACCEPT_BROADCAST: u32 = 1 << 3;
const RCR_ACCEPT_MULTICAST: u32 = 1 << 2;
const RCR_ACCEPT_MY_PHYS: u32 = 1 << 1;
const RCR_ACCEPT_ALL_PHYS: u32 = 1 << 0;
const RCR_ACCEPT_NORMAL: u32 = RCR_ACCEPT_BROADCAST | RCR_ACCEPT_MULTICAST | RCR_ACCEPT_MY_PHYS;
// The original TRUEOS bring-up profile delivered low-latency RX on the tested
// RTL8125B. Retain its high fields while keeping AcceptAllPhys cleared. The
// upstream family fetch profile remains available for a future complete
// RTL8125 MAC/firmware initialization sequence.
const RCR_COMPAT_BASELINE: u32 = 0x0000_e700;
const USE_FAMILY_RCR_PROFILE: bool = false;
// Some RTL8125 revisions preserve hardware-owned/reserved RCR bits (the tested
// RTL8125B rev 05 reports bit 17 set). Validate only fields the driver owns.
const RCR_DRIVER_OWNED_MASK: u32 = RCR_RX_FETCH_MASK
    | RCR_COMPAT_FIFO_MASK
    | RCR_VLAN_DETAG_MASK
    | RCR_RX_PAUSE_SLOT_ON
    | RCR_RX_DMA_BURST_MASK
    | RCR_ACCEPT_ERROR_MASK
    | RCR_ACCEPT_BROADCAST
    | RCR_ACCEPT_MULTICAST
    | RCR_ACCEPT_MY_PHYS
    | RCR_ACCEPT_ALL_PHYS;

// Multicast groups used unconditionally during network bring-up. Keep the
// hardware mask narrow: opening every MAR bucket lets unrelated/high-rate UDP
// multicast consume the polled RX path before smoltcp can reject it.
const MCAST_MDNS: [u8; 6] = [0x01, 0x00, 0x5e, 0x00, 0x00, 0xfb];
const MCAST_IPV6_ALL_NODES: [u8; 6] = [0x33, 0x33, 0x00, 0x00, 0x00, 0x01];
const MCAST_DHCPV6_SERVERS: [u8; 6] = [0x33, 0x33, 0x00, 0x01, 0x00, 0x02];

// Bring-up toggles.
const ENABLE_RX_CHKSUM_OFFLOAD: bool = false;
const STRIP_RX_CRC: bool = false;

// Descriptor bits
const DESC_OWN: u32 = 1 << 31;
const DESC_EOR: u32 = 1 << 30;
const RX_FS: u32 = 1 << 29;
const RX_LS: u32 = 1 << 28;
const RX_RWT: u32 = 1 << 22;
const RX_ERR_SUM: u32 = 1 << 21;
const RX_RUNT: u32 = 1 << 20;
const RX_CRC: u32 = 1 << 19;

const TX_FS: u32 = 1 << 29;
const TX_LS: u32 = 1 << 28;

static SNAPSHOTS: Mutex<Vec<R8125Snapshot>> = Mutex::new(Vec::new());

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum R8125Family {
    A,
    B,
    D,
    K,
    Bp,
    Cp,
    Rtl9151A,
    Unknown,
}

impl R8125Family {
    const fn from_xid(xid: u16) -> Self {
        match xid {
            0x609 => Self::A,
            0x641 => Self::B,
            0x688 | 0x689 => Self::D,
            0x68a => Self::K,
            0x681 => Self::Bp,
            0x708 => Self::Cp,
            0x68b => Self::Rtl9151A,
            _ => Self::Unknown,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::A => "RTL8125A",
            Self::B => "RTL8125B",
            Self::D => "RTL8125D",
            Self::K => "RTL8125K",
            Self::Bp => "RTL8125BP",
            Self::Cp => "RTL8125CP",
            Self::Rtl9151A => "RTL9151A",
            Self::Unknown => "RTL8125-unknown",
        }
    }

    const fn firmware_hint(self) -> &'static str {
        match self {
            Self::A => "rtl8125a-3",
            Self::B => "rtl8125b-2",
            Self::D => "rtl8125d-1/2",
            Self::K => "rtl8125k-1",
            Self::Bp => "rtl8125bp-2",
            Self::Cp => "rtl8125cp-1",
            Self::Rtl9151A => "rtl9151a-1",
            Self::Unknown => "unknown",
        }
    }

    const fn rcr_baseline(self) -> u32 {
        if USE_FAMILY_RCR_PROFILE {
            let mut value = RCR_RX_FETCH_DFLT_8125 | RCR_RX_DMA_BURST;
            if !matches!(self, Self::A | Self::Unknown) {
                value |= RCR_RX_PAUSE_SLOT_ON;
            }
            value
        } else {
            RCR_COMPAT_BASELINE
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct R8125Snapshot {
    pub(crate) bus: u8,
    pub(crate) slot: u8,
    pub(crate) function: u8,
    pub(crate) mac_after_reset: [u8; 6],
    pub(crate) revision: u8,
    pub(crate) subsystem_vendor: u16,
    pub(crate) subsystem_device: u16,
    pub(crate) xid: u16,
    pub(crate) family: &'static str,
    pub(crate) firmware_hint: &'static str,
    pub(crate) initial_tcr: u32,
    pub(crate) rcr: u32,
    pub(crate) multicast_hash: u64,
    pub(crate) cplus: u16,
    pub(crate) mcu_before: u8,
    pub(crate) mcu_after: u8,
    pub(crate) config3: u8,
    pub(crate) config5: u8,
}

impl R8125Snapshot {
    pub(crate) const fn promiscuous(self) -> bool {
        (self.rcr & RCR_ACCEPT_ALL_PHYS) != 0
    }

    pub(crate) const fn accepts_own_mac(self) -> bool {
        (self.rcr & RCR_ACCEPT_MY_PHYS) != 0
    }

    pub(crate) const fn accepts_multicast(self) -> bool {
        (self.rcr & RCR_ACCEPT_MULTICAST) != 0
    }

    pub(crate) const fn accepts_broadcast(self) -> bool {
        (self.rcr & RCR_ACCEPT_BROADCAST) != 0
    }
}

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
    fn write_overlaps_station_mac(off: u16, width: usize) -> bool {
        let start = off as usize;
        let end = start.saturating_add(width);
        let mac_start = REG_IDR0 as usize;
        let mac_end = mac_start + 6;
        start < mac_end && end > mac_start
    }

    fn reject_station_mac_write(off: u16, width: usize, val: u32) -> bool {
        if !Self::write_overlaps_station_mac(off, width) {
            return false;
        }

        crate::log_warn!(
            target: "net";
            "net/r8125: BLOCKED station-MAC MMIO write off=0x{:04x} width={} val=0x{:08x}\n",
            off,
            width,
            val
        );
        true
    }

    #[inline]
    unsafe fn read_u8(&self, off: u16) -> u8 {
        read_volatile(self.base.as_ptr().add(off as usize) as *const u8)
    }

    #[inline]
    unsafe fn write_u8(&self, off: u16, val: u8) {
        if Self::reject_station_mac_write(off, 1, u32::from(val)) {
            return;
        }
        write_volatile(self.base.as_ptr().add(off as usize) as *mut u8, val);
    }

    #[inline]
    unsafe fn read_u16(&self, off: u16) -> u16 {
        read_volatile(self.base.as_ptr().add(off as usize) as *const u16)
    }

    #[inline]
    unsafe fn write_u16(&self, off: u16, val: u16) {
        if Self::reject_station_mac_write(off, 2, u32::from(val)) {
            return;
        }
        write_volatile(self.base.as_ptr().add(off as usize) as *mut u16, val);
    }

    #[inline]
    unsafe fn write_u32(&self, off: u16, val: u32) {
        if Self::reject_station_mac_write(off, 4, val) {
            return;
        }
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
    snapshot: R8125Snapshot,
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
    dbg_rx_rwt: u64,
    dbg_rx_runt: u64,
    dbg_rx_crc: u64,
    dbg_rx_len_bad: u64,
    dbg_last_phystat: u8,
    dbg_logged_first_tx: bool,
    dbg_logged_first_rx: bool,
    dbg_poll_ticks: u64,
    dbg_state_dumps: u64,
    dbg_isr_nonzero: u64,
    dbg_isr_rx_desc_unavailable: u64,
    dbg_last_cmd: u8,
    dbg_last_imr: u32,
    dbg_last_tnpds_lo: u32,
    dbg_last_tnpds_hi: u32,
    dbg_kick_readbacks: u64,
    dbg_doorbells: u64,
    dbg_tx_quarantined: bool,
    dbg_mac_checks: u64,
    dbg_mac_changes: u64,

    dbg_tx_link_down_drops: u64,
}

// Safety: this adapter is driven by the net task and protected by the global net mutex.
unsafe impl Send for R8125Adapter {}

impl R8125Adapter {
    unsafe fn read_hw_mac(mmio: &Mmio) -> [u8; 6] {
        let mut mac = [0u8; 6];
        for (i, octet) in mac.iter_mut().enumerate() {
            *octet = mmio.read_u8(REG_IDR0 + i as u16);
        }
        mac
    }

    #[inline]
    fn mac_is_invalid(mac: [u8; 6]) -> bool {
        mac == [0; 6] || mac == [0xff; 6] || (mac[0] & 1) != 0
    }

    #[inline]
    const fn xid_from_tcr(tcr: u32) -> u16 {
        ((tcr >> 20) & 0x0fcf) as u16
    }

    /// Linux-compatible Ethernet multicast CRC used by r8169/RTL8125 MAR.
    fn multicast_crc(mac: [u8; 6]) -> u32 {
        let mut crc = u32::MAX;
        for mut octet in mac {
            for _ in 0..8 {
                let carry = ((crc >> 31) ^ u32::from(octet & 1)) & 1;
                crc <<= 1;
                octet >>= 1;
                if carry != 0 {
                    crc ^= 0x04c1_1db7;
                }
            }
        }
        crc
    }

    fn multicast_hash(mac: [u8; 6]) -> u64 {
        let bit = Self::multicast_crc(mac) >> 26;
        let logical = 1u64 << bit;
        // RTL8125 uses the post-RTL8169-v6 MAR word/byte ordering used by
        // Linux r8169: swap the 32-bit halves, then byte-swap each word.
        let mar0 = ((logical >> 32) as u32).swap_bytes();
        let mar4 = (logical as u32).swap_bytes();
        ((mar4 as u64) << 32) | u64::from(mar0)
    }

    fn bringup_multicast_hash(mac: [u8; 6]) -> u64 {
        // The solicited-node address for our EUI-64 link-local address retains
        // the low 24 bits of the NIC MAC.
        let solicited_node = [0x33, 0x33, 0xff, mac[3], mac[4], mac[5]];
        Self::multicast_hash(MCAST_MDNS)
            | Self::multicast_hash(MCAST_IPV6_ALL_NODES)
            | Self::multicast_hash(MCAST_DHCPV6_SERVERS)
            | Self::multicast_hash(solicited_node)
    }

    /// Publish an RX descriptor to the NIC with ownership last.
    ///
    /// The device must never observe `DESC_OWN` before the buffer address and
    /// secondary options are visible. This is the DMA equivalent of Linux's
    /// `dma_wmb()` followed by `WRITE_ONCE(desc->opts1, DescOwn | ...)`.
    unsafe fn publish_rx_descriptor(desc: *mut RxDesc, phys: u64, eor: u32) {
        unsafe {
            write_volatile(core::ptr::addr_of_mut!((*desc).addr), phys);
            write_volatile(core::ptr::addr_of_mut!((*desc).opts2), 0);
        }
        compiler_fence(Ordering::Release);
        fence(Ordering::Release);
        unsafe {
            write_volatile(
                core::ptr::addr_of_mut!((*desc).opts1),
                DESC_OWN | eor | (RX_BUF_SIZE as u32 & 0x3fff),
            );
        }
    }

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

    fn refresh_snapshot(&mut self) {
        let (rcr, mar0, mar4, cplus, mcu, config3, config5) = unsafe {
            (
                self.mmio.read_u32(REG_RCR),
                self.mmio.read_u32(REG_MAR0),
                self.mmio.read_u32(REG_MAR4),
                self.mmio.read_u16(REG_CPLUS_CMD),
                self.mmio.read_u8(REG_MCU),
                self.mmio.read_u8(REG_CONFIG3),
                self.mmio.read_u8(REG_CONFIG5),
            )
        };
        let multicast_hash = ((mar4 as u64) << 32) | mar0 as u64;
        let security_changed = rcr != self.snapshot.rcr
            || multicast_hash != self.snapshot.multicast_hash
            || mcu != self.snapshot.mcu_after;

        if security_changed {
            crate::log_warn!(
                target: "net";
                "net/r8125: filter state changed bdf={:02x}:{:02x}.{} rcr=0x{:08x}->0x{:08x} promisc={} mar=0x{:016x}->0x{:016x} mcu=0x{:02x}->0x{:02x}\n",
                self.snapshot.bus,
                self.snapshot.slot,
                self.snapshot.function,
                self.snapshot.rcr,
                rcr,
                ((rcr & RCR_ACCEPT_ALL_PHYS) != 0) as u8,
                self.snapshot.multicast_hash,
                multicast_hash,
                self.snapshot.mcu_after,
                mcu
            );
        }

        self.snapshot.rcr = rcr;
        self.snapshot.multicast_hash = multicast_hash;
        self.snapshot.cplus = cplus;
        self.snapshot.mcu_after = mcu;
        self.snapshot.config3 = config3;
        self.snapshot.config5 = config5;
        publish_snapshot(self.snapshot);
    }

    fn ring_tx_doorbell(&mut self, reason: &str) {
        unsafe {
            // RTL8125 uses a different doorbell than RTL8168: a 16-bit TxPoll_8125
            // register where bit0 triggers a poll.
            if EXP_R8125_TXPOLL_90_ENABLE {
                self.mmio
                    .write_u16(REG_TXPOLL_90, EXP_R8125_TXPOLL_90_VALUE);
            }

            self.dbg_doorbells = self.dbg_doorbells.saturating_add(1);
            if crate::log_os::flags::R8125_VERBOSE_LOGS
                && (self.dbg_doorbells <= TX_DOORBELL_DEBUG_FIRST
                    || (self.dbg_doorbells & 0x3FF) == 0)
            {
                // Readbacks are useful while diagnosing a wedge, but PCIe MMIO
                // reads on every normal TX submission materially tax this
                // polling driver. Keep them entirely out of the quiet path.
                let poll90_rb = if EXP_R8125_TXPOLL_90_ENABLE {
                    self.mmio.read_u16(REG_TXPOLL_90)
                } else {
                    0
                };
                let cmd_rb = self.mmio.read_u8(REG_CMD);
                let isr_rb = self.mmio.read_u32(REG_INTR_STATUS_8125);
                let imr_rb = self.mmio.read_u32(REG_INTR_MASK_8125);
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

    fn rx_ring_ownership(&self) -> (usize, usize, usize) {
        let mut nic_owned = 0usize;
        let mut host_ready = 0usize;
        let mut zero_length = 0usize;
        for idx in 0..RX_DESC_COUNT {
            let desc = unsafe { read_volatile(self.rx_desc.add(idx)) };
            let opts1 = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(desc.opts1)) };
            if (opts1 & DESC_OWN) != 0 {
                nic_owned += 1;
            } else {
                host_ready += 1;
                if (opts1 & 0x3fff) == 0 {
                    zero_length += 1;
                }
            }
        }
        (nic_owned, host_ready, zero_length)
    }

    fn log_poll_snapshot(&self, reason: &str, isr: u32) {
        let idx = self.rx_idx;
        let next_idx = (idx + 1) % RX_DESC_COUNT;
        let desc = unsafe { read_volatile(self.rx_desc.add(idx)) };
        let next = unsafe { read_volatile(self.rx_desc.add(next_idx)) };
        let opts1 = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(desc.opts1)) };
        let next_opts1 = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(next.opts1)) };
        let (nic_owned, host_ready, zero_length) = self.rx_ring_ownership();
        let (cmd, imr, phy, rcr, rds_lo, rds_hi) = unsafe {
            (
                self.mmio.read_u8(REG_CMD),
                self.mmio.read_u32(REG_INTR_MASK_8125),
                self.mmio.read_u8(REG_PHYSTAT),
                self.mmio.read_u32(REG_RCR),
                self.mmio.read_u32(REG_RDSAR),
                self.mmio.read_u32(REG_RDSAR_HI),
            )
        };
        crate::log_trace!(
            target: "net";
            "net/r8125: poll-snapshot reason={} poll={} isr=0x{:08x} imr=0x{:08x} cmd=0x{:02x} phy=0x{:02x} rcr=0x{:08x} rdsar=0x{:08x}{:08x} cursor={} opts1=0x{:08x} next={} next_opts1=0x{:08x} nic_owned={} host_ready={} zero_len={} tx_head={} tx_tail={} rx_ok={}\n",
            reason,
            self.dbg_poll_ticks,
            isr,
            imr,
            cmd,
            phy,
            rcr,
            rds_hi,
            rds_lo,
            idx,
            opts1,
            next_idx,
            next_opts1,
            nic_owned,
            host_ready,
            zero_length,
            self.tx_head,
            self.tx_tail,
            self.dbg_rx_ok
        );
    }

    fn trace_mac_integrity(&mut self, reason: &str, force_log: bool) {
        self.dbg_mac_checks = self.dbg_mac_checks.saturating_add(1);
        let hw_mac = unsafe { Self::read_hw_mac(&self.mmio) };
        if hw_mac != self.mac || Self::mac_is_invalid(hw_mac) {
            self.dbg_mac_changes = self.dbg_mac_changes.saturating_add(1);
            crate::log_warn!(
                target: "net";
                "net/r8125: MAC INTEGRITY VIOLATION reason={} poll={} checks={} changes={} expected={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} hardware={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} invalid={}\n",
                reason,
                self.dbg_poll_ticks,
                self.dbg_mac_checks,
                self.dbg_mac_changes,
                self.mac[0], self.mac[1], self.mac[2], self.mac[3], self.mac[4], self.mac[5],
                hw_mac[0], hw_mac[1], hw_mac[2], hw_mac[3], hw_mac[4], hw_mac[5],
                Self::mac_is_invalid(hw_mac) as u8
            );
        } else if force_log {
            crate::log_trace!(
                target: "net";
                "net/r8125: mac-integrity reason={} poll={} checks={} mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}\n",
                reason,
                self.dbg_poll_ticks,
                self.dbg_mac_checks,
                hw_mac[0], hw_mac[1], hw_mac[2], hw_mac[3], hw_mac[4], hw_mac[5]
            );
        }
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
            "net/r8125: state reason={} dumps={} poll={} cmd=0x{:02x} isr=0x{:08x} imr=0x{:08x} phy=0x{:02x} rcr=0x{:08x} tcr=0x{:08x} cplus=0x{:04x} rxmax=0x{:04x} rdsar=0x{:08x}{:08x} tnpds=0x{:08x}{:08x} tx_desc_phys=0x{:016x} tx_head={} tx_tail={} tx_head_opts1=0x{:08x} tx_tail_opts1=0x{:08x} rx_idx={} rx_opts1=0x{:08x} tx_sub={} tx_rec={} tx_full={} tx_checks={} kicks={} resets={} rx_ok={} rx_drop={} rx_bad={} rx_errsum={} rx_rwt={} rx_runt={} rx_crc={} rx_len_bad={}\n",
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
            self.dbg_rx_rwt,
            self.dbg_rx_runt,
            self.dbg_rx_crc,
            self.dbg_rx_len_bad
        );
        let hw_mac = unsafe { Self::read_hw_mac(&self.mmio) };
        let (mar0, mar4, mcu, cfg9346, cfg3, cfg5) = unsafe {
            (
                self.mmio.read_u32(REG_MAR0),
                self.mmio.read_u32(REG_MAR4),
                self.mmio.read_u8(REG_MCU),
                self.mmio.read_u8(REG_CFG9346),
                self.mmio.read_u8(REG_CONFIG3),
                self.mmio.read_u8(REG_CONFIG5),
            )
        };
        crate::log_trace!(
            target: "net";
            "net/r8125: state-extra reason={} expected_mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} hw_mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} mar=0x{:08x}{:08x} mcu=0x{:02x} cfg9346=0x{:02x} cfg3=0x{:02x} cfg5=0x{:02x}\n",
            reason,
            self.mac[0], self.mac[1], self.mac[2], self.mac[3], self.mac[4], self.mac[5],
            hw_mac[0], hw_mac[1], hw_mac[2], hw_mac[3], hw_mac[4], hw_mac[5],
            mar4,
            mar0,
            mcu,
            cfg9346,
            cfg3,
            cfg5
        );
    }

    pub fn init_all() -> alloc::vec::Vec<Self> {
        SNAPSHOTS.lock().clear();
        let mut out = alloc::vec::Vec::new();
        let devs = find_r8125_devices();
        for dev in devs {
            match Self::init_from_device(dev) {
                Ok(adapter) => out.push(adapter),
                Err(()) => {
                    crate::log_warn!(
                        target: "net";
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
        if crate::log_os::flags::R8125_VERBOSE_LOGS {
            crate::log!("net/r8125: pci cmd=0x{:04x}\n", cmd);
        }

        let (bar_index, bar_phys) = find_mmio_bar_phys(&dev)?;
        let bar_size = pci::bar_size_bytes(dev.bus, dev.slot, dev.function, bar_index).unwrap_or(0);
        let map_size = match usize::try_from(bar_size) {
            Ok(size) if size != 0 => size,
            _ => {
                if crate::log_os::flags::R8125_VERBOSE_LOGS {
                    crate::log!("net/r8125: bar{} size unknown; using 0x1000\n", bar_index);
                }
                0x1000
            }
        };
        if crate::log_os::flags::R8125_VERBOSE_LOGS && bar_size != 0 {
            crate::log!("net/r8125: bar{} size=0x{:x}\n", bar_index, bar_size);
        }
        let mapped = match pci::mmio::map_mmio_region_exact(bar_phys, map_size) {
            Ok(mapped) => mapped,
            Err(err) => {
                crate::log_warn!(
                    target: "net";
                    "net/r8125: bar{} mmio map failed: {:?}\n",
                    bar_index,
                    err
                );
                return Err(());
            }
        };
        let mmio = Mmio { base: mapped };
        let revision = pci::config_read_u8(dev.bus, dev.slot, dev.function, 0x08);
        let subsystem_vendor = pci::config_read_u16(dev.bus, dev.slot, dev.function, 0x2c);
        let subsystem_device = pci::config_read_u16(dev.bus, dev.slot, dev.function, 0x2e);
        // TxConfig carries the Realtek MAC XID. Capture it before reset or any
        // driver write so 10ec:8125 can be resolved to its actual MAC family.
        let initial_tcr = unsafe { mmio.read_u32(REG_TCR) };
        let mac_before_reset = unsafe { Self::read_hw_mac(&mmio) };
        let (
            cmd_before_reset,
            isr_before_reset,
            imr_before_reset,
            phy_before_reset,
            rcr_before_reset,
        ) = unsafe {
            (
                mmio.read_u8(REG_CMD),
                mmio.read_u32(REG_INTR_STATUS_8125),
                mmio.read_u32(REG_INTR_MASK_8125),
                mmio.read_u8(REG_PHYSTAT),
                mmio.read_u32(REG_RCR),
            )
        };
        crate::log_trace!(
            target: "net";
            "net/r8125: phase=pre-reset bdf={:02x}:{:02x}.{} mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} invalid={} cmd=0x{:02x} isr=0x{:08x} imr=0x{:08x} phy=0x{:02x} rcr=0x{:08x} tcr=0x{:08x}\n",
            dev.bus, dev.slot, dev.function,
            mac_before_reset[0], mac_before_reset[1], mac_before_reset[2],
            mac_before_reset[3], mac_before_reset[4], mac_before_reset[5],
            Self::mac_is_invalid(mac_before_reset) as u8,
            cmd_before_reset, isr_before_reset, imr_before_reset, phy_before_reset,
            rcr_before_reset, initial_tcr
        );
        let xid = Self::xid_from_tcr(initial_tcr);
        let family = R8125Family::from_xid(xid);
        if family == R8125Family::Unknown {
            crate::log_warn!(
                target: "net";
                "net/r8125: unknown MAC XID bdf={:02x}:{:02x}.{} xid={:03x} initial_tcr=0x{:08x}; using RTL8125A-compatible RCR baseline\n",
                dev.bus,
                dev.slot,
                dev.function,
                xid,
                initial_tcr
            );
        }

        if crate::log_os::flags::R8125_VERBOSE_LOGS {
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
        let mut reset_spins: u32 = 0;
        unsafe {
            mmio.write_u8(REG_CMD, CMD_RST);
            for spin in 0..1_000_000 {
                reset_spins = spin + 1;
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
            crate::log_warn!(target: "net"; "net/r8125: reset timeout cmd=0x{:02x}\n", last_cmd);
            return Err(());
        }

        let mac = unsafe { Self::read_hw_mac(&mmio) };
        crate::log_trace!(
            target: "net";
            "net/r8125: phase=post-reset spins={} cmd=0x{:02x} mac_before={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} mac_after={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} changed={} invalid={}\n",
            reset_spins,
            last_cmd,
            mac_before_reset[0], mac_before_reset[1], mac_before_reset[2],
            mac_before_reset[3], mac_before_reset[4], mac_before_reset[5],
            mac[0], mac[1], mac[2], mac[3], mac[4], mac[5],
            (mac != mac_before_reset) as u8,
            Self::mac_is_invalid(mac) as u8
        );
        if mac != mac_before_reset || Self::mac_is_invalid(mac) {
            crate::log_warn!(
                target: "net";
                "net/r8125: MAC changed across reset bdf={:02x}:{:02x}.{} before={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} after={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}\n",
                dev.bus, dev.slot, dev.function,
                mac_before_reset[0], mac_before_reset[1], mac_before_reset[2],
                mac_before_reset[3], mac_before_reset[4], mac_before_reset[5],
                mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
            );
        }

        // Allocate descriptor rings
        let rx_desc_bytes = core::mem::size_of::<RxDesc>() * RX_DESC_COUNT;
        let tx_desc_bytes = core::mem::size_of::<TxDesc>() * TX_DESC_COUNT;
        if crate::log_os::flags::R8125_VERBOSE_LOGS {
            crate::log!("net/r8125: alloc rx_desc bytes=0x{:x}\n", rx_desc_bytes);
        }
        let rx_desc_mem = DmaRegion::alloc(rx_desc_bytes, 256).ok_or(())?;
        if crate::log_os::flags::R8125_VERBOSE_LOGS {
            crate::log!("net/r8125: alloc tx_desc bytes=0x{:x}\n", tx_desc_bytes);
        }
        let tx_desc_mem = DmaRegion::alloc(tx_desc_bytes, 256).ok_or(())?;

        let rx_desc_phys = rx_desc_mem.phys();
        let tx_desc_phys = tx_desc_mem.phys();
        if crate::log_os::flags::R8125_VERBOSE_LOGS {
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
        if crate::log_os::flags::R8125_VERBOSE_LOGS {
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
                Self::publish_rx_descriptor(rx_desc.add(i), buf.phys(), eor);
            }
            rx_bufs.push(buf);
        }
        if crate::log_os::flags::R8125_VERBOSE_LOGS {
            for idx in [0, 1, RX_DESC_COUNT - 2, RX_DESC_COUNT - 1] {
                let desc = unsafe { read_volatile(rx_desc.add(idx)) };
                let opts1 = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(desc.opts1)) };
                let opts2 = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(desc.opts2)) };
                let addr = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(desc.addr)) };
                crate::log_trace!(
                    target: "net";
                    "net/r8125: phase=rx-ring-published desc={} own={} eor={} opts1=0x{:08x} opts2=0x{:08x} addr=0x{:016x}\n",
                    idx,
                    ((opts1 & DESC_OWN) != 0) as u8,
                    ((opts1 & DESC_EOR) != 0) as u8,
                    opts1,
                    opts2,
                    addr
                );
            }
        }

        if crate::log_os::flags::R8125_VERBOSE_LOGS {
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
        let mcu_before: u8;
        let mcu_after: u8;
        let rcr_programmed = family.rcr_baseline() | RCR_ACCEPT_NORMAL;
        let multicast_hash_programmed = Self::bringup_multicast_hash(mac);
        if !USE_FAMILY_RCR_PROFILE {
            crate::log_warn!(
                target: "net";
                "net/r8125: using proven compatibility RCR profile bdf={:02x}:{:02x}.{} family={} rcr=0x{:08x}; family fetch profile deferred until full MAC/firmware init\n",
                dev.bus,
                dev.slot,
                dev.function,
                family.name(),
                rcr_programmed
            );
        }
        unsafe {
            // Stop engines while programming baseline datapath registers.
            mmio.write_u8(REG_CMD, 0);
            mmio.write_u32(REG_INTR_MASK_8125, 0);
            mmio.write_u32(REG_INTR_STATUS_8125, 0xFFFF_FFFF);

            // Ensure the device is not stuck in OOB mode. When NOW_IS_OOB is
            // set, TX/RX DMA may not behave as expected.
            let mcu0 = mmio.read_u8(REG_MCU);
            mcu_before = mcu0;
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
            mcu_after = mcu1;
            if crate::log_os::flags::R8125_VERBOSE_LOGS {
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

            // Deterministic, narrow bring-up filter. Future dynamic multicast
            // users must add a membership callback before opening their bucket.
            mmio.write_u32(REG_MAR4, (multicast_hash_programmed >> 32) as u32);
            mmio.write_u32(REG_MAR0, multicast_hash_programmed as u32);

            // RTL8125-specific receive baseline with promiscuous mode off.
            mmio.write_u32(REG_RCR, rcr_programmed);
            let tcr = EXP_R8125_TCR_OVERRIDE.unwrap_or(0x03000700);
            mmio.write_u32(REG_TCR, tcr);

            // Lock config back down.
            mmio.write_u8(REG_CFG9346, CFG9346_LOCK);

            // Enable Rx/Tx
            mmio.write_u8(REG_CMD, CMD_RX_EN | CMD_TX_EN);
            mmio.write_u32(REG_INTR_STATUS_8125, 0xFFFF_FFFF);
        }

        // Confirm key registers took effect (helps diagnose write-protect / wrong offsets).
        let (rcr_rb, tcr_rb, cplus_rb, mar0_rb, mar4_rb, config3, config5) = unsafe {
            (
                mmio.read_u32(REG_RCR),
                mmio.read_u32(REG_TCR),
                mmio.read_u16(REG_CPLUS_CMD),
                mmio.read_u32(REG_MAR0),
                mmio.read_u32(REG_MAR4),
                mmio.read_u8(REG_CONFIG3),
                mmio.read_u8(REG_CONFIG5),
            )
        };
        let mac_after_program = unsafe { Self::read_hw_mac(&mmio) };
        crate::log_trace!(
            target: "net";
            "net/r8125: phase=engine-enabled cmd=0x{:02x} isr=0x{:08x} imr=0x{:08x} phy=0x{:02x} mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} changed_since_reset={} invalid={} rcr=0x{:08x} tcr=0x{:08x} cplus=0x{:04x} rdsar=0x{:08x}{:08x} tnpds=0x{:08x}{:08x}\n",
            unsafe { mmio.read_u8(REG_CMD) },
            unsafe { mmio.read_u32(REG_INTR_STATUS_8125) },
            unsafe { mmio.read_u32(REG_INTR_MASK_8125) },
            unsafe { mmio.read_u8(REG_PHYSTAT) },
            mac_after_program[0], mac_after_program[1], mac_after_program[2],
            mac_after_program[3], mac_after_program[4], mac_after_program[5],
            (mac_after_program != mac) as u8,
            Self::mac_is_invalid(mac_after_program) as u8,
            rcr_rb,
            tcr_rb,
            cplus_rb,
            unsafe { mmio.read_u32(REG_RDSAR_HI) },
            unsafe { mmio.read_u32(REG_RDSAR) },
            unsafe { mmio.read_u32(REG_TNPDS_HI) },
            unsafe { mmio.read_u32(REG_TNPDS) }
        );
        if mac_after_program != mac || Self::mac_is_invalid(mac_after_program) {
            crate::log_warn!(
                target: "net";
                "net/r8125: MAC changed while programming datapath bdf={:02x}:{:02x}.{} expected={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} hardware={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}\n",
                dev.bus, dev.slot, dev.function,
                mac[0], mac[1], mac[2], mac[3], mac[4], mac[5],
                mac_after_program[0], mac_after_program[1], mac_after_program[2],
                mac_after_program[3], mac_after_program[4], mac_after_program[5]
            );
        }
        if crate::log_os::flags::R8125_VERBOSE_LOGS {
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
        if crate::log_os::flags::R8125_VERBOSE_LOGS {
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
        if crate::log_os::flags::R8125_VERBOSE_LOGS {
            crate::log!("net/r8125: phystat=0x{:02x} (raw)\n", phy);
        }

        let multicast_hash = ((mar4_rb as u64) << 32) | mar0_rb as u64;
        let snapshot = R8125Snapshot {
            bus: dev.bus,
            slot: dev.slot,
            function: dev.function,
            mac_after_reset: mac,
            revision,
            subsystem_vendor,
            subsystem_device,
            xid,
            family: family.name(),
            firmware_hint: family.firmware_hint(),
            initial_tcr,
            rcr: rcr_rb,
            multicast_hash,
            cplus: cplus_rb,
            mcu_before,
            mcu_after,
            config3,
            config5,
        };
        if ((rcr_rb ^ rcr_programmed) & RCR_DRIVER_OWNED_MASK) != 0
            || multicast_hash != multicast_hash_programmed
        {
            crate::log_warn!(
                target: "net";
                "net/r8125: filter readback mismatch bdf={:02x}:{:02x}.{} rcr_want=0x{:08x} rcr_got=0x{:08x} mar_want=0x{:016x} mar_got=0x{:016x}\n",
                snapshot.bus,
                snapshot.slot,
                snapshot.function,
                rcr_programmed,
                rcr_rb,
                multicast_hash_programmed,
                multicast_hash
            );
        }
        publish_snapshot(snapshot);
        crate::log_info!(
            target: "net";
            "net/r8125: hw bdf={:02x}:{:02x}.{} rev={:02x} subsys={:04x}:{:04x} xid={:03x} family={} fw_hint={} initial_tcr=0x{:08x} rcr=0x{:08x} accept_own={} accept_bcast={} accept_mcast={} promisc={} mar=0x{:016x} cplus=0x{:04x} mcu=0x{:02x}->0x{:02x} cfg3=0x{:02x} cfg5=0x{:02x}\n",
            snapshot.bus,
            snapshot.slot,
            snapshot.function,
            snapshot.revision,
            snapshot.subsystem_vendor,
            snapshot.subsystem_device,
            snapshot.xid,
            snapshot.family,
            snapshot.firmware_hint,
            snapshot.initial_tcr,
            snapshot.rcr,
            snapshot.accepts_own_mac() as u8,
            snapshot.accepts_broadcast() as u8,
            snapshot.accepts_multicast() as u8,
            snapshot.promiscuous() as u8,
            snapshot.multicast_hash,
            snapshot.cplus,
            snapshot.mcu_before,
            snapshot.mcu_after,
            snapshot.config3,
            snapshot.config5
        );

        Ok(Self {
            mmio,
            pci: dev,
            mac,
            snapshot,
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
            dbg_rx_rwt: 0,
            dbg_rx_runt: 0,
            dbg_rx_crc: 0,
            dbg_rx_len_bad: 0,
            dbg_last_phystat: phy,
            dbg_logged_first_tx: false,
            dbg_logged_first_rx: false,
            dbg_poll_ticks: 0,
            dbg_state_dumps: 0,
            dbg_isr_nonzero: 0,
            dbg_isr_rx_desc_unavailable: 0,
            dbg_last_cmd: CMD_RX_EN | CMD_TX_EN,
            dbg_last_imr: 0,
            dbg_last_tnpds_lo: tx_desc_phys as u32,
            dbg_last_tnpds_hi: (tx_desc_phys >> 32) as u32,
            dbg_kick_readbacks: 0,
            dbg_doorbells: 0,
            dbg_tx_quarantined: false,
            dbg_mac_checks: 0,
            dbg_mac_changes: 0,

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
                crate::log_trace!(
                    target: "net";
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

        crate::log_warn!(
            target: "net";
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

        crate::log_info!(
            target: "net";
            "net/r8125: tx reset reinit head={} tail={} skip_desc0={}\n",
            self.tx_head,
            self.tx_tail,
            EXP_R8125_SKIP_DESC0 as u8
        );

        if self.dbg_tx_reclaimed == 0 && self.dbg_tx_resets >= TX_WEDGE_QUARANTINE_RESETS {
            self.dbg_tx_quarantined = true;
            crate::log_warn!(
                target: "net";
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
                    crate::log_warn!(
                        target: "net";
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
                crate::log_trace!(target: "net"; "net/r8125: first tx reclaim\n");
            }
        }
    }

    fn poll_rx_ring(&mut self) {
        self.dbg_poll_ticks = self.dbg_poll_ticks.saturating_add(1);

        let trace_poll = self.dbg_poll_ticks <= RX_TRACE_EARLY_POLLS
            || self.dbg_poll_ticks.is_multiple_of(RX_TRACE_POLL_EVERY);
        if self.dbg_poll_ticks <= RX_TRACE_EARLY_POLLS
            || self.dbg_poll_ticks.is_multiple_of(MAC_TRACE_POLL_EVERY)
        {
            self.trace_mac_integrity("poll", trace_poll);
        }

        let early_isr = unsafe { self.mmio.read_u32(REG_INTR_STATUS_8125) };
        if trace_poll {
            self.log_poll_snapshot("poll-entry", early_isr);
        }

        if self.dbg_poll_ticks.is_multiple_of(POLL_STATE_LOG_EVERY) {
            self.refresh_snapshot();
        }

        if crate::log_os::flags::R8125_VERBOSE_LOGS
            && self.dbg_poll_ticks.is_multiple_of(POLL_STATE_LOG_EVERY)
        {
            self.log_hw_state("periodic");
        }

        if crate::log_os::flags::R8125_VERBOSE_LOGS {
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

            // PHY polling here is diagnostic only; link_state() performs its
            // own required read. Avoid duplicating it on every RX poll.
            let phy = unsafe { self.mmio.read_u8(REG_PHYSTAT) };
            if phy != self.dbg_last_phystat {
                self.dbg_last_phystat = phy;
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
            if (isr & ISR_RX_DESC_UNAVAILABLE) != 0 {
                self.dbg_isr_rx_desc_unavailable =
                    self.dbg_isr_rx_desc_unavailable.saturating_add(1);
                if (self.dbg_isr_rx_desc_unavailable <= 64
                    && self.dbg_isr_rx_desc_unavailable.is_power_of_two())
                    || self.dbg_isr_rx_desc_unavailable == 4_096
                    || self
                        .dbg_isr_rx_desc_unavailable
                        .is_multiple_of(RX_DESC_UNAVAILABLE_WARN_EVERY)
                {
                    crate::log_warn!(
                        target: "net";
                        "net/r8125: rx descriptor unavailable/overflow count={} poll={} rx_ok={} rx_idx={} ring_full={} isr=0x{:08x}\n",
                        self.dbg_isr_rx_desc_unavailable,
                        self.dbg_poll_ticks,
                        self.dbg_rx_ok,
                        self.rx_idx,
                        self.dbg_rx_ring_full,
                        isr
                    );
                    self.log_poll_snapshot("rxdu-before-ack", isr);
                }
            }
            // ISR can be chatty (e.g. link-related or RX OK); keep a small sample
            // and then only very occasionally.
            if crate::log_os::flags::R8125_VERBOSE_LOGS
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
            if crate::log_os::flags::R8125_VERBOSE_LOGS && (isr & ISR_RX_DESC_UNAVAILABLE) != 0 {
                let isr_after_ack = unsafe { self.mmio.read_u32(REG_INTR_STATUS_8125) };
                crate::log_trace!(
                    target: "net";
                    "net/r8125: isr-ack poll={} wrote=0x{:08x} readback=0x{:08x}\n",
                    self.dbg_poll_ticks,
                    isr,
                    isr_after_ack
                );
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
            // Once ownership clears, order subsequent reads of the DMA buffer
            // after the device's descriptor completion write.
            fence(Ordering::Acquire);

            let had_errsum = (opts1 & RX_ERR_SUM) != 0;

            if (opts1 & (RX_FS | RX_LS)) != (RX_FS | RX_LS) {
                self.dbg_rx_bad_flags = self.dbg_rx_bad_flags.saturating_add(1);
                if self.dbg_rx_bad_flags == 1
                    || self.dbg_rx_bad_flags.is_multiple_of(RX_BAD_FLAGS_LOG_EVERY)
                {
                    crate::log_trace!(
                        target: "net";
                        "net/r8125: rx flags missing fs/ls count={} opts1=0x{:08x} (continuing)\n",
                        self.dbg_rx_bad_flags,
                        opts1
                    );
                }
            }

            let raw_len = (opts1 & 0x3FFF) as usize;

            if had_errsum {
                self.dbg_rx_errsum = self.dbg_rx_errsum.saturating_add(1);
                if (opts1 & RX_RWT) != 0 {
                    self.dbg_rx_rwt = self.dbg_rx_rwt.saturating_add(1);
                }
                if (opts1 & RX_RUNT) != 0 {
                    self.dbg_rx_runt = self.dbg_rx_runt.saturating_add(1);
                }
                if (opts1 & RX_CRC) != 0 {
                    self.dbg_rx_crc = self.dbg_rx_crc.saturating_add(1);
                }
                if self.dbg_rx_errsum == 1
                    || self.dbg_rx_errsum.is_multiple_of(RX_BAD_FLAGS_LOG_EVERY)
                {
                    crate::log_warn!(
                        target: "net";
                        "net/r8125: rx hardware error count={} opts1=0x{:08x} rwt={} runt={} crc={} (dropping)\n",
                        self.dbg_rx_errsum,
                        opts1,
                        ((opts1 & RX_RWT) != 0) as u8,
                        ((opts1 & RX_RUNT) != 0) as u8,
                        ((opts1 & RX_CRC) != 0) as u8
                    );
                }

                let eor = if idx + 1 == RX_DESC_COUNT {
                    DESC_EOR
                } else {
                    0
                };
                unsafe {
                    Self::publish_rx_descriptor(
                        self.rx_desc.add(idx),
                        self.rx_bufs[idx].phys(),
                        eor,
                    );
                }
                self.rx_idx = (self.rx_idx + 1) % RX_DESC_COUNT;
                processed += 1;
                continue;
            }

            if raw_len == 0 || raw_len > RX_BUF_SIZE {
                self.dbg_rx_len_bad = self.dbg_rx_len_bad.saturating_add(1);
                if self.dbg_rx_len_bad == 1 {
                    crate::log_trace!(
                        target: "net";
                        "net/r8125: rx bad len raw_len={} opts1=0x{:08x}\n",
                        raw_len,
                        opts1
                    );
                }
                let eor = if idx + 1 == RX_DESC_COUNT {
                    DESC_EOR
                } else {
                    0
                };
                unsafe {
                    Self::publish_rx_descriptor(
                        self.rx_desc.add(idx),
                        self.rx_bufs[idx].phys(),
                        eor,
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

            if self.dbg_rx_ok < RX_TRACE_EARLY_FRAMES && len >= 14 {
                crate::log_trace!(
                    target: "net";
                    "net/r8125: rx-l2 seq={} poll={} desc={} len={} opts1=0x{:08x} dst={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} src={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} ethertype=0x{:02x}{:02x}\n",
                    self.dbg_rx_ok + 1,
                    self.dbg_poll_ticks,
                    idx,
                    len,
                    opts1,
                    data[0], data[1], data[2], data[3], data[4], data[5],
                    data[6], data[7], data[8], data[9], data[10], data[11],
                    data[12], data[13]
                );
            }

            unsafe {
                let ring = &mut *ring_ptr;
                if ring.push_rx_packet(data).is_err() {
                    self.dbg_rx_ring_full = self.dbg_rx_ring_full.saturating_add(1);
                    if self.dbg_rx_ring_full == 1 {
                        crate::log_trace!(target: "net"; "net/r8125: rx ring full (dropping)\n");
                    }
                } else {
                    self.dbg_rx_ok = self.dbg_rx_ok.saturating_add(1);
                    if !self.dbg_logged_first_rx {
                        self.dbg_logged_first_rx = true;
                        crate::log_trace!(
                            target: "net";
                            "net/r8125: first rx len={} raw_len={} opts1=0x{:08x}\n",
                            len,
                            raw_len,
                            opts1
                        );
                    }
                }
            }

            // Re-arm descriptor
            let eor = if idx + 1 == RX_DESC_COUNT {
                DESC_EOR
            } else {
                0
            };
            unsafe {
                Self::publish_rx_descriptor(self.rx_desc.add(idx), self.rx_bufs[idx].phys(), eor);
            }

            self.rx_idx = (self.rx_idx + 1) % RX_DESC_COUNT;
            processed += 1;
        }

        if (processed != 0 && trace_poll) || (isr & ISR_RX_DESC_UNAVAILABLE) != 0 {
            let isr_after_drain = unsafe { self.mmio.read_u32(REG_INTR_STATUS_8125) };
            crate::log_trace!(
                target: "net";
                "net/r8125: rx-drain poll={} processed={} cursor={} isr_before=0x{:08x} isr_after=0x{:08x} rx_ok={} ring_full={}\n",
                self.dbg_poll_ticks,
                processed,
                self.rx_idx,
                isr,
                isr_after_drain,
                self.dbg_rx_ok,
                self.dbg_rx_ring_full
            );
            self.log_poll_snapshot("rx-drain-end", isr_after_drain);
        }

        self.reclaim_tx();
    }

    fn transmit_ready_hw(&mut self) -> bool {
        if self.dbg_tx_quarantined {
            return false;
        }

        let phy = unsafe { self.mmio.read_u8(REG_PHYSTAT) };
        if !Self::phy_link_up(phy) {
            return false;
        }

        self.reclaim_tx();
        if (self.tx_tail + 1) % TX_DESC_COUNT == self.tx_head {
            self.kick_tx_engine();
            self.reclaim_tx();
            if (self.tx_tail + 1) % TX_DESC_COUNT == self.tx_head {
                return false;
            }
        }

        let current = unsafe { read_volatile(self.tx_desc.add(self.tx_tail)) };
        let opts1 = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(current.opts1)) };
        (opts1 & DESC_OWN) == 0
    }

    fn transmit_hw_with(&mut self, len: usize, fill: &mut dyn FnMut(&mut [u8])) -> Result<(), ()> {
        if len == 0 {
            fill(&mut []);
            return Ok(());
        }
        if len > TX_BUF_SIZE {
            return Err(());
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
                crate::log_trace!(
                    target: "net";
                    "net/r8125: drop tx (link down) count={} phystat=0x{:02x}\n",
                    self.dbg_tx_link_down_drops,
                    phy
                );
            }
            return Err(());
        }

        // Don't rely on RX polling cadence to free TX descriptors.
        self.reclaim_tx();

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
                    crate::log_warn!(
                        target: "net";
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
                    crate::log_warn!(
                        target: "net";
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

        let tx_buffer = unsafe { core::slice::from_raw_parts_mut(self.tx_bufs[idx].virt(), len) };
        fill(tx_buffer);

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
                crate::log_trace!(
                    target: "net";
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

            crate::log_trace!(
                target: "net";
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
            crate::log_trace!(
                target: "net";
                "net/r8125: first tx len={} head={} tail={}\n",
                len,
                self.tx_head,
                self.tx_tail
            );
        }
        Ok(())
    }

    fn transmit_hw(&mut self, frame: &[u8]) -> Result<(), ()> {
        let len = min(frame.len(), TX_BUF_SIZE);
        let mut fill = |tx_buffer: &mut [u8]| {
            tx_buffer.copy_from_slice(&frame[..len]);
        };
        self.transmit_hw_with(len, &mut fill)
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

    fn transmit_with(&mut self, len: usize, fill: &mut dyn FnMut(&mut [u8])) -> Result<(), ()> {
        self.transmit_hw_with(len, fill)
    }

    fn transmit_ready(&mut self) -> bool {
        self.transmit_ready_hw()
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

fn publish_snapshot(snapshot: R8125Snapshot) {
    let mut snapshots = SNAPSHOTS.lock();
    if let Some(existing) = snapshots.iter_mut().find(|existing| {
        (existing.bus, existing.slot, existing.function)
            == (snapshot.bus, snapshot.slot, snapshot.function)
    }) {
        *existing = snapshot;
    } else {
        snapshots.push(snapshot);
    }
}

pub(crate) fn snapshot_for_bdf(bus: u8, slot: u8, function: u8) -> Option<R8125Snapshot> {
    SNAPSHOTS
        .lock()
        .iter()
        .copied()
        .find(|snapshot| (snapshot.bus, snapshot.slot, snapshot.function) == (bus, slot, function))
}

pub(crate) fn snapshots() -> Vec<R8125Snapshot> {
    SNAPSHOTS.lock().clone()
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
            if crate::log_os::flags::R8125_VERBOSE_LOGS {
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
            if crate::log_os::flags::R8125_VERBOSE_LOGS {
                crate::log!("net/r8125: bar{} is zero\n", i);
            }
            i += 1;
            continue;
        }

        if crate::log_os::flags::R8125_VERBOSE_LOGS {
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
