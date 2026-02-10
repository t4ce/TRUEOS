use alloc::vec::Vec;

use core::cmp::min;
use core::ptr::{read_volatile, write_volatile, NonNull};
use core::sync::atomic::{compiler_fence, fence, Ordering};

use crate::net::core::VendorAdapter;
use crate::net::ring::{DmaRegion, NetRing};
use crate::pci;

const REALTEK_VENDOR_ID: u16 = 0x10EC;
const RTL8168_DEVICE_ID: u16 = 0x8168;

const RX_DESC_COUNT: usize = 64;
const TX_DESC_COUNT: usize = 64;
const RX_BUF_SIZE: usize = 2048;
const TX_BUF_SIZE: usize = 2048;
const RX_POLL_BUDGET: usize = 32;

// MMIO registers (RTL8168 family)
const REG_IDR0: u16 = 0x00; // MAC 0..5
const REG_TNPDS: u16 = 0x20; // Tx desc start addr (low)
const REG_TNPDS_HI: u16 = 0x24;
const REG_CMD: u16 = 0x37;
const REG_IMR: u16 = 0x3C;
const REG_ISR: u16 = 0x3E;
const REG_TXPOLL: u16 = 0x38;
const REG_RCR: u16 = 0x44;
const REG_TCR: u16 = 0x40;
const REG_RDSAR: u16 = 0xE4; // Rx desc start addr (low)
const REG_RDSAR_HI: u16 = 0xE8;
const REG_CPLUS_CMD: u16 = 0xE0;
const REG_RX_MAX_SIZE: u16 = 0xDA;
const REG_PHYSTAT: u16 = 0x6C;

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

pub struct R8168Adapter {
    mmio: Mmio,
    mac: [u8; 6],
    ring: Option<*mut NetRing>,

    _rx_desc_mem: DmaRegion,
    rx_desc_phys: u64,
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
    dbg_rx_ok: u64,
    dbg_rx_ring_full: u64,
    dbg_rx_bad_flags: u64,
    dbg_rx_errsum: u64,
    dbg_rx_len_zero: u64,
    dbg_last_phystat: u8,
    dbg_logged_first_tx: bool,
    dbg_logged_first_rx: bool,
    dbg_logged_cfg: bool,
    dbg_idle_polls: u64,
}

// Safety: this adapter is driven by the net task and protected by the global net mutex.
unsafe impl Send for R8168Adapter {}

impl R8168Adapter {
    pub fn init_all() -> alloc::vec::Vec<Self> {
        let mut out = alloc::vec::Vec::new();
        let devs = find_r8168_devices();
        for dev in devs {
            match Self::init_from_device(dev) {
                Ok(adapter) => out.push(adapter),
                Err(()) => {
                    crate::log!(
                        "net/r8168: init failed for {:02x}:{:02x}.{}\n",
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
        crate::log!("net/r8168: pci cmd=0x{:04x}\n", cmd);

        let (bar_index, bar_phys) = find_mmio_bar_phys(&dev)?;
        let bar_size = pci::bar_size_bytes(dev.bus, dev.slot, dev.function, bar_index)
            .unwrap_or(0);
        let map_size = match usize::try_from(bar_size) {
            Ok(size) if size != 0 => size,
            _ => {
                crate::log!("net/r8168: bar{} size unknown; using 0x1000\n", bar_index);
                0x1000
            }
        };
        if bar_size != 0 {
            crate::log!("net/r8168: bar{} size=0x{:x}\n", bar_index, bar_size);
        }
        let mapped = match pci::mmio::map_mmio_region_exact(bar_phys, map_size) {
            Ok(mapped) => mapped,
            Err(err) => {
                crate::log!(
                    "net/r8168: bar{} mmio map failed: {:?}\n",
                    bar_index,
                    err
                );
                return Err(());
            }
        };
        let mmio = Mmio { base: mapped };

        crate::log!(
            "net/r8168: found {:02x}:{:02x}.{} vid={:04x} did={:04x} bar{}=0x{:x}\n",
            dev.bus,
            dev.slot,
            dev.function,
            dev.vendor,
            dev.device,
            bar_index,
            bar_phys
        );

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
            mmio.write_u16(REG_IMR, 0);
            mmio.write_u16(REG_ISR, 0xFFFF);
        }
        if !reset_done {
            crate::log!("net/r8168: reset timeout cmd=0x{:02x}\n", last_cmd);
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
        crate::log!("net/r8168: alloc rx_desc bytes=0x{:x}\n", rx_desc_bytes);
        let rx_desc_mem = DmaRegion::alloc(rx_desc_bytes, 256).ok_or(())?;
        crate::log!("net/r8168: alloc tx_desc bytes=0x{:x}\n", tx_desc_bytes);
        let tx_desc_mem = DmaRegion::alloc(tx_desc_bytes, 256).ok_or(())?;

        let rx_desc_phys = rx_desc_mem.phys();
        let tx_desc_phys = tx_desc_mem.phys();
        crate::log!(
            "net/r8168: rx_desc phys=0x{:x} align256_ok={} tx_desc phys=0x{:x} align256_ok={}\n",
            rx_desc_phys,
            ((rx_desc_phys & 0xFF) == 0) as u8,
            tx_desc_phys,
            ((tx_desc_phys & 0xFF) == 0) as u8
        );

        let rx_desc = rx_desc_mem.virt() as *mut RxDesc;
        let tx_desc = tx_desc_mem.virt() as *mut TxDesc;

        // Allocate buffers and initialize descriptors
        crate::log!("net/r8168: alloc rx bufs count={} size=0x{:x}\n", RX_DESC_COUNT, RX_BUF_SIZE);
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

        crate::log!("net/r8168: alloc tx bufs count={} size=0x{:x}\n", TX_DESC_COUNT, TX_BUF_SIZE);
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
            // C+ mode on (descriptor mode). Keep it minimal.
            let cplus = mmio.read_u16(REG_CPLUS_CMD);
            let mut cplus_new = cplus | CPLUS_ENABLE;
            if ENABLE_RX_CHKSUM_OFFLOAD {
                cplus_new |= CPLUS_RX_CHKSUM;
            }
            mmio.write_u16(REG_CPLUS_CMD, cplus_new);
            mmio.write_u16(REG_RX_MAX_SIZE, RX_BUF_SIZE as u16);

            // Descriptor ring addresses
            mmio.write_u32(REG_RDSAR, rx_desc_phys as u32);
            mmio.write_u32(REG_RDSAR_HI, (rx_desc_phys >> 32) as u32);
            mmio.write_u32(REG_TNPDS, tx_desc_phys as u32);
            mmio.write_u32(REG_TNPDS_HI, (tx_desc_phys >> 32) as u32);

            // Basic RX/TX config (promiscuous off; accept broadcast/multicast).
            // Values here are intentionally conservative for bring-up.
            mmio.write_u32(REG_RCR, 0x0000E70F);
            mmio.write_u32(REG_TCR, 0x03000700);

            // Enable Rx/Tx
            mmio.write_u8(REG_CMD, CMD_RX_EN | CMD_TX_EN);
        }

        crate::log!(
            "net/r8168: mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}\n",
            mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
        );

        crate::log!(
            "net/r8168: caps speeds=10/100/1000 duplex=full/half flow=tx/rx ring=rx{} tx{} mtu=1500\n",
            RX_DESC_COUNT,
            TX_DESC_COUNT
        );
        let phy = unsafe { mmio.read_u8(REG_PHYSTAT) };
        crate::log!("net/r8168: phystat=0x{:02x} (raw)\n", phy);

        Ok(Self {
            mmio,
            mac,
            ring: None,
            _rx_desc_mem: rx_desc_mem,
            rx_desc_phys,
            rx_desc,
            rx_bufs,
            rx_idx: 0,
            _tx_desc_mem: tx_desc_mem,
            tx_desc_phys,
            tx_desc,
            tx_bufs,
            tx_head: 0,
            tx_tail: 0,

            dbg_tx_submitted: 0,
            dbg_tx_reclaimed: 0,
            dbg_tx_ring_full: 0,
            dbg_tx_stall_checks: 0,
            dbg_tx_recovery_kicks: 0,
            dbg_rx_ok: 0,
            dbg_rx_ring_full: 0,
            dbg_rx_bad_flags: 0,
            dbg_rx_errsum: 0,
            dbg_rx_len_zero: 0,
            dbg_last_phystat: phy,
            dbg_logged_first_tx: false,
            dbg_logged_first_rx: false,
            dbg_logged_cfg: false,
            dbg_idle_polls: 0,
        })
    }

    fn kick_tx_engine(&mut self) {
        self.dbg_tx_recovery_kicks = self.dbg_tx_recovery_kicks.saturating_add(1);
        unsafe {
            // Re-assert descriptor base addresses (some variants appear to need this
            // after a reset or transient fault) and re-enable TX.
            self.mmio.write_u32(REG_TNPDS, self.tx_desc_phys as u32);
            self.mmio
                .write_u32(REG_TNPDS_HI, (self.tx_desc_phys >> 32) as u32);

            let cmd = self.mmio.read_u8(REG_CMD);
            self.mmio.write_u8(REG_CMD, cmd | CMD_TX_EN | CMD_RX_EN);

            // Kick TX (NPQ)
            self.mmio.write_u8(REG_TXPOLL, 0x40);
        }
    }

    #[inline]
    fn tx_ring_full(&self) -> bool {
        (self.tx_tail + 1) % TX_DESC_COUNT == self.tx_head
    }

    fn reclaim_tx(&mut self) {
        while self.tx_head != self.tx_tail {
            let idx = self.tx_head;
            let desc = unsafe { read_volatile(self.tx_desc.add(idx)) };
            let desc_opts1 = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(desc.opts1)) };
            if (desc_opts1 & DESC_OWN) != 0 {
                self.dbg_tx_stall_checks = self.dbg_tx_stall_checks.saturating_add(1);
                if self.dbg_tx_submitted != 0 && (self.dbg_tx_stall_checks % 10_000) == 0 {
                    let (cmd, tcr, tn_lo, tn_hi, isr) = unsafe {
                        (
                            self.mmio.read_u8(REG_CMD),
                            self.mmio.read_u32(REG_TCR),
                            self.mmio.read_u32(REG_TNPDS),
                            self.mmio.read_u32(REG_TNPDS_HI),
                            self.mmio.read_u16(REG_ISR),
                        )
                    };
                    crate::log!(
                        "net/r8168: tx stall? checks={} head={} tail={} desc_opts1=0x{:08x} cmd=0x{:02x} tcr=0x{:08x} tnpds=0x{:08x}{:08x} isr=0x{:04x}\n",
                        self.dbg_tx_stall_checks,
                        self.tx_head,
                        self.tx_tail,
                        desc_opts1,
                        cmd,
                        tcr,
                        tn_hi,
                        tn_lo,
                        isr
                    );

                    // Recovery attempt when TX ownership appears stuck.
                    self.kick_tx_engine();
                }
                break;
            }
            self.tx_head = (self.tx_head + 1) % TX_DESC_COUNT;
            self.dbg_tx_stall_checks = 0;

            self.dbg_tx_reclaimed = self.dbg_tx_reclaimed.saturating_add(1);
            if self.dbg_tx_reclaimed == 1 {
                crate::log!("net/r8168: first tx reclaim\n");
            }
        }
    }

    fn poll_rx_ring(&mut self) {
        if !self.dbg_logged_cfg {
            self.dbg_logged_cfg = true;
            let cmd = unsafe { self.mmio.read_u8(REG_CMD) };
            let rcr = unsafe { self.mmio.read_u32(REG_RCR) };
            let tcr = unsafe { self.mmio.read_u32(REG_TCR) };
            let cplus = unsafe { self.mmio.read_u16(REG_CPLUS_CMD) };
            let rms = unsafe { self.mmio.read_u16(REG_RX_MAX_SIZE) };
            crate::log!(
                "net/r8168: cfg cmd=0x{:02x} rcr=0x{:08x} tcr=0x{:08x} cplus=0x{:04x} rx_max=0x{:04x}\n",
                cmd,
                rcr,
                tcr,
                cplus,
                rms
            );
        }

        // Track PHY/link changes without spamming: only log on change.
        let phy = unsafe { self.mmio.read_u8(REG_PHYSTAT) };
        if phy != self.dbg_last_phystat {
            self.dbg_last_phystat = phy;
            crate::log!(
                "net/r8168: phystat=0x{:02x} (changed) link_bit0={}\n",
                phy,
                ((phy & 0x01) != 0) as u8
            );
        }

        let Some(ring_ptr) = self.ring else {
            // Still reclaim TX even if not bound yet.
            self.reclaim_tx();
            return;
        };

        let mut processed = 0usize;
        let mut did_rx = false;
        loop {
            if processed >= RX_POLL_BUDGET {
                break;
            }
            let idx = self.rx_idx;
            let desc = unsafe { read_volatile(self.rx_desc.add(idx)) };

            let opts1 = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(desc.opts1)) };
            let opts2 = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(desc.opts2)) };

            if (opts1 & DESC_OWN) != 0 {
                break;
            }

            did_rx = true;

            let had_errsum = (opts1 & RX_ERR_SUM) != 0;

            if (opts1 & (RX_FS | RX_LS)) != (RX_FS | RX_LS) {
                // Log once but continue anyway.
                if self.dbg_rx_bad_flags == 0 {
                    crate::log!(
                        "net/r8168: rx flags missing fs/ls opts1=0x{:08x} (continuing)\n",
                        opts1
                    );
                }
            }

            let raw_len = (opts1 & 0x3FFF) as usize;

            if had_errsum {
                self.dbg_rx_errsum = self.dbg_rx_errsum.saturating_add(1);
                // On this family, ERR_SUM can represent checksum status metadata.
                // Keep logging sparse and actionable.
                if self.dbg_rx_errsum == 1 || (self.dbg_rx_errsum & 0x3ff) == 0 {
                    crate::log!(
                        "net/r8168: rx errsum seen count={} opts1=0x{:08x} opts2=0x{:08x} raw_len={} (accepted={})\n",
                        self.dbg_rx_errsum,
                        opts1,
                        opts2,
                        raw_len,
                        ACCEPT_RX_ERR_SUM_FRAMES as u8
                    );
                }

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

            if raw_len == 0 {
                self.dbg_rx_len_zero = self.dbg_rx_len_zero.saturating_add(1);
            }
            if raw_len == 0 || raw_len > RX_BUF_SIZE {
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
                        crate::log!("net/r8168: rx ring full (dropping)\n");
                    }
                } else {
                    self.dbg_rx_ok = self.dbg_rx_ok.saturating_add(1);
                    if !self.dbg_logged_first_rx {
                        self.dbg_logged_first_rx = true;
                        crate::log!(
                            "net/r8168: first rx len={} raw_len={} opts1=0x{:08x}\n",
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

        if !did_rx {
            // If RX never produces any completed descriptors, log very rarely
            // to distinguish "no traffic" from "ring not advancing".
            self.dbg_idle_polls = self.dbg_idle_polls.saturating_add(1);
            if self.dbg_idle_polls == 10_000 {
                let d0 = unsafe { read_volatile(self.rx_desc) };
                let o0 = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(d0.opts1)) };
                let phy = unsafe { self.mmio.read_u8(REG_PHYSTAT) };
                crate::log!(
                    "net/r8168: rx idle (10s) desc0_opts1=0x{:08x} phystat=0x{:02x}\n",
                    o0,
                    phy
                );
            }
        } else {
            self.dbg_idle_polls = 0;
        }

        self.reclaim_tx();
    }

    fn transmit_hw(&mut self, frame: &[u8]) -> Result<(), ()> {
        if frame.is_empty() {
            return Ok(());
        }

        // Don't rely on RX polling cadence to free TX descriptors.
        self.reclaim_tx();

        let len = min(frame.len(), TX_BUF_SIZE);
        if self.tx_ring_full() {
            self.dbg_tx_ring_full = self.dbg_tx_ring_full.saturating_add(1);
            // Recovery path: reclaim + kick + reclaim once before dropping.
            self.kick_tx_engine();
            self.reclaim_tx();
            if self.tx_ring_full() {
                if self.dbg_tx_ring_full == 1 || (self.dbg_tx_ring_full & 0xff) == 0 {
                    let (cmd, tcr, tn_lo, tn_hi, isr, phy) = unsafe {
                        (
                            self.mmio.read_u8(REG_CMD),
                            self.mmio.read_u32(REG_TCR),
                            self.mmio.read_u32(REG_TNPDS),
                            self.mmio.read_u32(REG_TNPDS_HI),
                            self.mmio.read_u16(REG_ISR),
                            self.mmio.read_u8(REG_PHYSTAT),
                        )
                    };
                    crate::log!(
                        "net/r8168: tx ring full count={} head={} tail={} cmd=0x{:02x} tcr=0x{:08x} tnpds=0x{:08x}{:08x} isr=0x{:04x} phystat=0x{:02x}\n",
                        self.dbg_tx_ring_full,
                        self.tx_head,
                        self.tx_tail,
                        cmd,
                        tcr,
                        tn_hi,
                        tn_lo,
                        isr,
                        phy
                    );
                }
                return Err(());
            }
        }
        let next_tail = (self.tx_tail + 1) % TX_DESC_COUNT;

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
                if self.dbg_tx_ring_full == 1 || (self.dbg_tx_ring_full & 0xff) == 0 {
                    crate::log!(
                        "net/r8168: tx desc busy count={} idx={} head={} tail={} opts1=0x{:08x}\n",
                        self.dbg_tx_ring_full,
                        idx,
                        self.tx_head,
                        self.tx_tail,
                        cur2_opts1
                    );
                }
                return Err(());
            }
        }

        unsafe {
            core::ptr::copy_nonoverlapping(frame.as_ptr(), self.tx_bufs[idx].virt(), len);
        }

        // Ensure the packet bytes are visible before we set DESC_OWN.
        compiler_fence(Ordering::Release);

        let eor = if idx + 1 == TX_DESC_COUNT { DESC_EOR } else { 0 };
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

            // Kick TX (NPQ)
            self.mmio.write_u8(REG_TXPOLL, 0x40);
        }

        self.tx_tail = next_tail;
        self.dbg_tx_submitted = self.dbg_tx_submitted.saturating_add(1);
        if !self.dbg_logged_first_tx {
            self.dbg_logged_first_tx = true;
            crate::log!(
                "net/r8168: first tx len={} head={} tail={}\n",
                len,
                self.tx_head,
                self.tx_tail
            );
        }
        Ok(())
    }
}

impl VendorAdapter for R8168Adapter {
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

    fn bind_ring(&mut self, ring: *mut NetRing) {
        self.ring = Some(ring);
    }
}

fn find_r8168_devices() -> alloc::vec::Vec<pci::PciDevice> {
    let mut out = alloc::vec::Vec::new();
    pci::with_devices(|list| {
        for dev in list {
            if dev.vendor != REALTEK_VENDOR_ID {
                continue;
            }
            if dev.device != RTL8168_DEVICE_ID {
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
            crate::log!("net/r8168: bar{} is IO (raw=0x{:08x})\n", i, bar_lo);
            i += 1;
            continue;
        }

        let is_64 = ((bar_lo >> 1) & 0x3) == 0x2;
        let lo = (bar_lo as u64) & !0xFu64;
        let hi = bar_hi.unwrap_or(0) as u64;
        let phys = lo | (hi << 32);
        if phys == 0 {
            crate::log!("net/r8168: bar{} is zero\n", i);
            i += 1;
            continue;
        }

        crate::log!(
            "net/r8168: bar{} mmio raw=0x{:08x}{} => 0x{:x}\n",
            i,
            bar_lo,
            if is_64 { " (64)" } else { "" },
            phys
        );

        return Ok((i, phys));
    }
    Err(())
}
