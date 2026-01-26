use alloc::vec::Vec;

use core::cmp::min;
use core::ptr::{read_volatile, write_volatile, NonNull};

use crate::net::core::VendorAdapter;
use crate::net::ring::{DmaRegion, NetRing};
use crate::pci;

const REALTEK_VENDOR_ID: u16 = 0x10EC;

// Common RTL81xx IDs. (QEMU's rtl8139 is *not* this family, but real hw is.)
const RTL8169_DEVICE_IDS: &[u16] = &[0x8169, 0x8168, 0x8136, 0x8167, 0x8161];

const RX_DESC_COUNT: usize = 64;
const TX_DESC_COUNT: usize = 64;
const RX_BUF_SIZE: usize = 2048;
const TX_BUF_SIZE: usize = 2048;

// MMIO registers (RTL8169/8168 family)
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

const CMD_RX_EN: u8 = 1 << 3;
const CMD_TX_EN: u8 = 1 << 2;
const CMD_RST: u8 = 1 << 4;

const CPLUS_RX_CHKSUM: u16 = 1 << 1;
const CPLUS_ENABLE: u16 = 1 << 0;

// Descriptor bits
const DESC_OWN: u32 = 1 << 31;
const DESC_EOR: u32 = 1 << 30;

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
    unsafe fn read_u32(&self, off: u16) -> u32 {
        read_volatile(self.base.as_ptr().add(off as usize) as *const u32)
    }

    #[inline]
    unsafe fn write_u32(&self, off: u16, val: u32) {
        write_volatile(self.base.as_ptr().add(off as usize) as *mut u32, val);
    }
}

pub struct R8169Adapter {
    mmio: Mmio,
    mac: [u8; 6],
    ring: Option<*mut NetRing>,

    rx_desc_mem: DmaRegion,
    rx_desc: *mut RxDesc,
    rx_bufs: Vec<DmaRegion>,
    rx_idx: usize,

    tx_desc_mem: DmaRegion,
    tx_desc: *mut TxDesc,
    tx_bufs: Vec<DmaRegion>,
    tx_head: usize,
    tx_tail: usize,
}

// Safety: this adapter is driven by the net task and protected by the global net mutex.
unsafe impl Send for R8169Adapter {}

impl R8169Adapter {
    pub fn init() -> Result<Self, ()> {
        let dev = find_r8169_device().ok_or(())?;
        pci::enable_mem_and_bus_master(dev.bus, dev.slot, dev.function);

        let bar_phys = read_bar0_phys(&dev)?;
        let mapped = pci::mmio::map_mmio_region_exact(bar_phys, 0x1000).map_err(|_| ())?;
        let mmio = Mmio { base: mapped };

        crate::log!(
            "net/r8169: found {:02x}:{:02x}.{} vid={:04x} did={:04x} bar0=0x{:x}\n",
            dev.bus,
            dev.slot,
            dev.function,
            dev.vendor,
            dev.device,
            bar_phys
        );

        // Reset
        unsafe {
            mmio.write_u8(REG_CMD, CMD_RST);
            for _ in 0..1_000_000 {
                if (mmio.read_u8(REG_CMD) & CMD_RST) == 0 {
                    break;
                }
            }

            // Mask interrupts
            mmio.write_u16(REG_IMR, 0);
            mmio.write_u16(REG_ISR, 0xFFFF);
        }

        let mac = unsafe {
            let mut m = [0u8; 6];
            for i in 0..6 {
                m[i] = mmio.read_u8(REG_IDR0 + i as u16);
            }
            m
        };

        // Allocate descriptor rings
        let rx_desc_mem = DmaRegion::alloc(core::mem::size_of::<RxDesc>() * RX_DESC_COUNT, 16)
            .ok_or(())?;
        let tx_desc_mem = DmaRegion::alloc(core::mem::size_of::<TxDesc>() * TX_DESC_COUNT, 16)
            .ok_or(())?;

        let rx_desc = rx_desc_mem.virt() as *mut RxDesc;
        let tx_desc = tx_desc_mem.virt() as *mut TxDesc;

        // Allocate buffers and initialize descriptors
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
            "net/r8169: mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}\n",
            mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
        );

        Ok(Self {
            mmio,
            mac,
            ring: None,
            rx_desc_mem,
            rx_desc,
            rx_bufs,
            rx_idx: 0,
            tx_desc_mem,
            tx_desc,
            tx_bufs,
            tx_head: 0,
            tx_tail: 0,
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
        }
    }

    fn poll_rx_ring(&mut self) {
        let Some(ring_ptr) = self.ring else {
            // Still reclaim TX even if not bound yet.
            self.reclaim_tx();
            return;
        };

        loop {
            let idx = self.rx_idx;
            let desc = unsafe { read_volatile(self.rx_desc.add(idx)) };

            if (desc.opts1 & DESC_OWN) != 0 {
                break;
            }

            let raw_len = (desc.opts1 & 0x3FFF) as usize;
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
                let _ = ring.push_rx_packet(data);
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
            return Err(());
        }

        let idx = self.tx_tail;
        let cur = unsafe { read_volatile(self.tx_desc.add(idx)) };
        if (cur.opts1 & DESC_OWN) != 0 {
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
        Ok(())
    }
}

impl VendorAdapter for R8169Adapter {
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

fn find_r8169_device() -> Option<pci::PciDevice> {
    pci::with_devices(|list| {
        for dev in list {
            if dev.vendor != REALTEK_VENDOR_ID {
                continue;
            }
            if !RTL8169_DEVICE_IDS.contains(&dev.device) {
                continue;
            }
            if dev.class != 0x02 {
                continue;
            }
            return Some(*dev);
        }
        None
    })
}

fn read_bar0_phys(dev: &pci::PciDevice) -> Result<u64, ()> {
    let (bar_lo, bar_hi) = pci::read_bar0_raw(dev.bus, dev.slot, dev.function);
    if (bar_lo & 0x1) != 0 {
        // IO BAR unsupported for this driver.
        return Err(());
    }
    let lo = (bar_lo as u64) & !0xFu64;
    let hi = bar_hi.unwrap_or(0) as u64;
    Ok(lo | (hi << 32))
}
