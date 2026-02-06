use alloc::vec::Vec;

use core::cmp::min;
use core::ptr::{read_volatile, write_volatile, NonNull};

use crate::net::core::VendorAdapter;
use crate::net::ring::{DmaRegion, NetRing};
use crate::pci;

const REALTEK_VENDOR_ID: u16 = 0x10EC;
const RTL8125_DEVICE_ID: u16 = 0x8125;

const RX_DESC_COUNT: usize = 64;
const TX_DESC_COUNT: usize = 64;
const RX_BUF_SIZE: usize = 2048;
const TX_BUF_SIZE: usize = 2048;
const RX_POLL_BUDGET: usize = 32;

// MMIO registers (RTL8125 family)
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
}

pub struct R8125Adapter {
    mmio: Mmio,
    mac: [u8; 6],
    ring: Option<*mut NetRing>,

    _rx_desc_mem: DmaRegion,
    rx_desc: *mut RxDesc,
    rx_bufs: Vec<DmaRegion>,
    rx_idx: usize,

    _tx_desc_mem: DmaRegion,
    tx_desc: *mut TxDesc,
    tx_bufs: Vec<DmaRegion>,
    tx_head: usize,
    tx_tail: usize,

    // Bring-up instrumentation (kept lightweight; no high-rate logging)
    dbg_tx_submitted: u64,
    dbg_tx_reclaimed: u64,
    dbg_tx_ring_full: u64,
    dbg_rx_ok: u64,
    dbg_rx_ring_full: u64,
    dbg_rx_bad_flags: u64,
    dbg_rx_len_bad: u64,
    dbg_last_phystat: u8,
    dbg_logged_first_tx: bool,
    dbg_logged_first_rx: bool,
}

// Safety: this adapter is driven by the net task and protected by the global net mutex.
unsafe impl Send for R8125Adapter {}

impl R8125Adapter {
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
        crate::log!("net/r8125: pci cmd=0x{:04x}\n", cmd);

        let (bar_index, bar_phys) = find_mmio_bar_phys(&dev)?;
        let bar_size = pci::bar_size_bytes(dev.bus, dev.slot, dev.function, bar_index)
            .unwrap_or(0);
        let map_size = match usize::try_from(bar_size) {
            Ok(size) if size != 0 => size,
            _ => {
                crate::log!("net/r8125: bar{} size unknown; using 0x1000\n", bar_index);
                0x1000
            }
        };
        if bar_size != 0 {
            crate::log!("net/r8125: bar{} size=0x{:x}\n", bar_index, bar_size);
        }
        let mapped = match pci::mmio::map_mmio_region_exact(bar_phys, map_size) {
            Ok(mapped) => mapped,
            Err(err) => {
                crate::log!(
                    "net/r8125: bar{} mmio map failed: {:?}\n",
                    bar_index,
                    err
                );
                return Err(());
            }
        };
        let mmio = Mmio { base: mapped };

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
        crate::log!("net/r8125: alloc rx_desc bytes=0x{:x}\n", rx_desc_bytes);
        let rx_desc_mem = DmaRegion::alloc(rx_desc_bytes, 16).ok_or(())?;
        crate::log!("net/r8125: alloc tx_desc bytes=0x{:x}\n", tx_desc_bytes);
        let tx_desc_mem = DmaRegion::alloc(tx_desc_bytes, 16).ok_or(())?;

        let rx_desc = rx_desc_mem.virt() as *mut RxDesc;
        let tx_desc = tx_desc_mem.virt() as *mut TxDesc;

        // Allocate buffers and initialize descriptors
        crate::log!("net/r8125: alloc rx bufs count={} size=0x{:x}\n", RX_DESC_COUNT, RX_BUF_SIZE);
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

        crate::log!("net/r8125: alloc tx bufs count={} size=0x{:x}\n", TX_DESC_COUNT, TX_BUF_SIZE);
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
            mmio.write_u16(REG_CPLUS_CMD, cplus | CPLUS_ENABLE | CPLUS_RX_CHKSUM);
            mmio.write_u16(REG_RX_MAX_SIZE, RX_BUF_SIZE as u16);

            // Descriptor ring addresses
            mmio.write_u32(REG_RDSAR, rx_desc_mem.phys() as u32);
            mmio.write_u32(REG_RDSAR_HI, (rx_desc_mem.phys() >> 32) as u32);
            mmio.write_u32(REG_TNPDS, tx_desc_mem.phys() as u32);
            mmio.write_u32(REG_TNPDS_HI, (tx_desc_mem.phys() >> 32) as u32);

            // Basic RX/TX config (promiscuous off; accept broadcast/multicast).
            // Values here are intentionally conservative for bring-up.
            mmio.write_u32(REG_RCR, 0x0000E70F);
            mmio.write_u32(REG_TCR, 0x03000700);

            // Enable Rx/Tx
            mmio.write_u8(REG_CMD, CMD_RX_EN | CMD_TX_EN);
        }

        crate::log!(
            "net/r8125: mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}\n",
            mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
        );

        crate::log!(
            "net/r8125: caps speeds=10/100/1000/2500 duplex=full/half flow=tx/rx ring=rx{} tx{} mtu=1500\n",
            RX_DESC_COUNT,
            TX_DESC_COUNT
        );
        let phy = unsafe { mmio.read_u8(REG_PHYSTAT) };
        crate::log!("net/r8125: phystat=0x{:02x} (raw)\n", phy);

        Ok(Self {
            mmio,
            mac,
            ring: None,
            _rx_desc_mem: rx_desc_mem,
            rx_desc,
            rx_bufs,
            rx_idx: 0,
            _tx_desc_mem: tx_desc_mem,
            tx_desc,
            tx_bufs,
            tx_head: 0,
            tx_tail: 0,

            dbg_tx_submitted: 0,
            dbg_tx_reclaimed: 0,
            dbg_tx_ring_full: 0,
            dbg_rx_ok: 0,
            dbg_rx_ring_full: 0,
            dbg_rx_bad_flags: 0,
            dbg_rx_len_bad: 0,
            dbg_last_phystat: phy,
            dbg_logged_first_tx: false,
            dbg_logged_first_rx: false,
        })
    }

    fn reclaim_tx(&mut self) {
        while self.tx_head != self.tx_tail {
            let idx = self.tx_head;
            let desc = unsafe { read_volatile(self.tx_desc.add(idx)) };
            if (desc.opts1 & DESC_OWN) != 0 {
                break;
            }
            self.tx_head = (self.tx_head + 1) % TX_DESC_COUNT;

            self.dbg_tx_reclaimed = self.dbg_tx_reclaimed.saturating_add(1);
            if self.dbg_tx_reclaimed == 1 {
                crate::log!("net/r8125: first tx reclaim\n");
            }
        }
    }

    fn poll_rx_ring(&mut self) {
        // Track PHY/link changes without spamming: only log on change.
        let phy = unsafe { self.mmio.read_u8(REG_PHYSTAT) };
        if phy != self.dbg_last_phystat {
            self.dbg_last_phystat = phy;
            crate::log!("net/r8125: phystat=0x{:02x} (changed)\n", phy);
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

            if (desc.opts1 & DESC_OWN) != 0 {
                break;
            }

            // Some Realtek variants do not reliably expose FS/LS as we expect.
            // For bring-up, only drop frames when the HW reports an error.
            if (desc.opts1 & RX_ERR_SUM) != 0 {
                self.dbg_rx_bad_flags = self.dbg_rx_bad_flags.saturating_add(1);
                if self.dbg_rx_bad_flags == 1 {
                    crate::log!("net/r8125: rx error opts1=0x{:08x}\n", desc.opts1);
                }
                let eor = desc.opts1 & DESC_EOR;
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

            if (desc.opts1 & (RX_FS | RX_LS)) != (RX_FS | RX_LS) {
                if self.dbg_rx_bad_flags == 0 {
                    crate::log!("net/r8125: rx flags missing fs/ls opts1=0x{:08x} (continuing)\n", desc.opts1);
                }
            }

            let raw_len = (desc.opts1 & 0x3FFF) as usize;
            if raw_len == 0 || raw_len > RX_BUF_SIZE {
                self.dbg_rx_len_bad = self.dbg_rx_len_bad.saturating_add(1);
                if self.dbg_rx_len_bad == 1 {
                    crate::log!(
                        "net/r8125: rx bad len raw_len={} opts1=0x{:08x}\n",
                        raw_len,
                        desc.opts1
                    );
                }
                let eor = desc.opts1 & DESC_EOR;
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
            if len >= 4 {
                // Strip CRC if present
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
                            desc.opts1
                        );
                    }
                }
            }

            // Re-arm descriptor
            let eor = desc.opts1 & DESC_EOR;
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

        let len = min(frame.len(), TX_BUF_SIZE);
        let next_tail = (self.tx_tail + 1) % TX_DESC_COUNT;
        if next_tail == self.tx_head {
            self.dbg_tx_ring_full = self.dbg_tx_ring_full.saturating_add(1);
            if self.dbg_tx_ring_full == 1 {
                crate::log!("net/r8125: tx ring full\n");
            }
            return Err(());
        }

        let idx = self.tx_tail;
        let cur = unsafe { read_volatile(self.tx_desc.add(idx)) };
        if (cur.opts1 & DESC_OWN) != 0 {
            self.dbg_tx_ring_full = self.dbg_tx_ring_full.saturating_add(1);
            return Err(());
        }

        unsafe {
            core::ptr::copy_nonoverlapping(frame.as_ptr(), self.tx_bufs[idx].virt(), len);
        }

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

            // Kick TX (NPQ)
            self.mmio.write_u8(REG_TXPOLL, 0x40);
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
        let (mut bar_lo, mut bar_hi) = pci::read_bar_raw(dev.bus, dev.slot, dev.function, i);
        if bar_lo == 0 {
            i += 1;
            continue;
        }
        if (bar_lo & 0x1) != 0 {
            crate::log!("net/r8125: bar{} is IO (raw=0x{:08x})\n", i, bar_lo);
            i += 1;
            continue;
        }

        let is_64 = ((bar_lo >> 1) & 0x3) == 0x2;
        let lo = (bar_lo as u64) & !0xFu64;
        let hi = bar_hi.unwrap_or(0) as u64;
        let phys = lo | (hi << 32);
        if phys == 0 {
            crate::log!("net/r8125: bar{} is zero\n", i);
            i += 1;
            continue;
        }

        crate::log!(
            "net/r8125: bar{} mmio raw=0x{:08x}{} => 0x{:x}\n",
            i,
            bar_lo,
            if is_64 { " (64)" } else { "" },
            phys
        );

        return Ok((i, phys));
    }
    Err(())
}
