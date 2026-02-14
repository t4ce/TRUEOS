use alloc::vec::Vec;

use core::cmp::min;
use core::mem::size_of;
use core::ptr::{read_volatile, write_volatile, NonNull};

use crate::net::core::VendorAdapter;
use crate::net::ring::{DmaRegion, NetRing};
use crate::pci;

const E1000_VENDOR_ID: u16 = 0x8086;
const E1000_DEVICE_ID: u16 = 0x100E; // 82540EM (QEMU e1000)

const REG_CTRL: u32 = 0x0000;
const REG_RCTL: u32 = 0x0100;
const REG_TCTL: u32 = 0x0400;
const REG_TIPG: u32 = 0x0410;
const REG_RDBAL: u32 = 0x2800;
const REG_RDBAH: u32 = 0x2804;
const REG_RDLEN: u32 = 0x2808;
const REG_RDH: u32 = 0x2810;
const REG_RDT: u32 = 0x2818;
const REG_TDBAL: u32 = 0x3800;
const REG_TDBAH: u32 = 0x3804;
const REG_TDLEN: u32 = 0x3808;
const REG_TDH: u32 = 0x3810;
const REG_TDT: u32 = 0x3818;
const REG_ICR: u32 = 0x00C0;
const REG_IMC: u32 = 0x00D8;
const REG_RAL0: u32 = 0x5400;
const REG_RAH0: u32 = 0x5404;

const CTRL_RST: u32 = 1 << 26;

const RCTL_EN: u32 = 1 << 1;
const RCTL_BAM: u32 = 1 << 15;
const RCTL_SECRC: u32 = 1 << 26;

const TCTL_EN: u32 = 1 << 1;
const TCTL_PSP: u32 = 1 << 3;
const TCTL_CT_SHIFT: u32 = 4;
const TCTL_COLD_SHIFT: u32 = 12;

const RX_STATUS_DD: u8 = 1 << 0;

const TX_CMD_EOP: u8 = 1 << 0;
const TX_CMD_IFCS: u8 = 1 << 1;
const TX_CMD_RS: u8 = 1 << 3;
const TX_STATUS_DD: u8 = 1 << 0;

const RAH_AV: u32 = 1 << 31;

const RX_RING_SIZE: usize = 64;
const RX_BUF_SIZE: usize = 2048;
const TX_RING_SIZE: usize = 64;
const TX_BUF_SIZE: usize = 2048;

#[repr(C, packed)]
struct RxDesc {
    addr: u64,
    length: u16,
    csum: u16,
    status: u8,
    errors: u8,
    special: u16,
}

#[repr(C, packed)]
struct TxDesc {
    addr: u64,
    length: u16,
    cso: u8,
    cmd: u8,
    status: u8,
    css: u8,
    special: u16,
}
struct Mmio {
    base: NonNull<u8>,
}

// Safety: mapped MMIO pointer stored behind net device mutex.
unsafe impl Send for Mmio {}

impl Mmio {
    #[inline]
    unsafe fn read_u32(&self, off: u32) -> u32 {
        read_volatile(self.base.as_ptr().add(off as usize) as *const u32)
    }

    #[inline]
    unsafe fn write_u32(&self, off: u32, val: u32) {
        write_volatile(self.base.as_ptr().add(off as usize) as *mut u32, val);
    }
}

pub struct E1000Adapter {
    mmio: Mmio,
    pci: pci::PciDevice,
    mac: [u8; 6],
    ring: Option<*mut NetRing>,

    rx_desc_mem: DmaRegion,
    rx_desc: *mut RxDesc,
    rx_bufs: Vec<DmaRegion>,
    rx_idx: usize,

    tx_desc_mem: DmaRegion,
    tx_desc: *mut TxDesc,
    tx_bufs: Vec<DmaRegion>,
    tx_idx: usize,
}

// Safety: this adapter is driven by the net task and protected by the global net mutex.
unsafe impl Send for E1000Adapter {}

impl E1000Adapter {
    pub fn init_all() -> alloc::vec::Vec<Self> {
        let mut out = alloc::vec::Vec::new();
        let devs = find_e1000_devices();
        if devs.is_empty() {
            return out;
        }

        for dev in devs {
            match Self::init_from_device(dev) {
                Ok(adapter) => out.push(adapter),
                Err(()) => {
                    crate::log!(
                        "net/e1000: init failed for {:02x}:{:02x}.{}\n",
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

        let (bar_index, bar_phys) = match find_mmio_bar_phys(&dev) {
            Ok(v) => v,
            Err(()) => {
                crate::log!(
                    "net/e1000: no MMIO BAR found at {:02x}:{:02x}.{}\n",
                    dev.bus,
                    dev.slot,
                    dev.function
                );
                return Err(());
            }
        };

        let mapped = match pci::mmio::map_mmio_region_exact(bar_phys, 0x20000) {
            Ok(v) => v,
            Err(_) => {
                crate::log!(
                    "net/e1000: failed to map MMIO (bar{} @ 0x{:x})\n",
                    bar_index,
                    bar_phys
                );
                return Err(());
            }
        };
        let mmio = Mmio { base: mapped };

        crate::log!(
            "net/e1000: found {:02x}:{:02x}.{} vid={:04x} did={:04x} mmio=bar{}@0x{:x}\n",
            dev.bus,
            dev.slot,
            dev.function,
            dev.vendor,
            dev.device,
            bar_index,
            bar_phys
        );

        let rx_desc_mem = match DmaRegion::alloc(size_of::<RxDesc>() * RX_RING_SIZE, 16) {
            Some(r) => r,
            None => {
                crate::log!("net/e1000: DMA alloc failed for RX desc ring\n");
                return Err(());
            }
        };
        let tx_desc_mem = match DmaRegion::alloc(size_of::<TxDesc>() * TX_RING_SIZE, 16) {
            Some(r) => r,
            None => {
                crate::log!("net/e1000: DMA alloc failed for TX desc ring\n");
                return Err(());
            }
        };

            let mut adapter = Self {
                mmio,
                pci: dev,
                mac: [0; 6],
            ring: None,
            rx_desc: rx_desc_mem.virt() as *mut RxDesc,
            rx_desc_mem,
            rx_bufs: Vec::new(),
            rx_idx: 0,
            tx_desc: tx_desc_mem.virt() as *mut TxDesc,
            tx_desc_mem,
            tx_bufs: Vec::new(),
            tx_idx: 0,
        };

        adapter.reset();

        // Disable interrupts for now (polling)
        unsafe {
            adapter.mmio.write_u32(REG_IMC, 0xFFFF_FFFF);
            let _ = adapter.mmio.read_u32(REG_ICR);
        }

        if adapter.setup_rx().is_err() {
            crate::log!("net/e1000: setup_rx failed\n");
            return Err(());
        }
        if adapter.setup_tx().is_err() {
            crate::log!("net/e1000: setup_tx failed\n");
            return Err(());
        }

        adapter.mac = adapter.read_mac();
        crate::log!(
            "net/e1000: mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}\n",
            adapter.mac[0],
            adapter.mac[1],
            adapter.mac[2],
            adapter.mac[3],
            adapter.mac[4],
            adapter.mac[5]
        );

        Ok(adapter)
    }

    fn reset(&mut self) {
        unsafe {
            let ctrl = self.mmio.read_u32(REG_CTRL);
            self.mmio.write_u32(REG_CTRL, ctrl | CTRL_RST);
            for _ in 0..1_000_000 {
                if (self.mmio.read_u32(REG_CTRL) & CTRL_RST) == 0 {
                    break;
                }
            }
        }
    }

    fn read_mac(&self) -> [u8; 6] {
        unsafe {
            let ral = self.mmio.read_u32(REG_RAL0);
            let rah = self.mmio.read_u32(REG_RAH0);
            if (rah & RAH_AV) == 0 {
                return [0; 6];
            }
            [
                (ral & 0xFF) as u8,
                ((ral >> 8) & 0xFF) as u8,
                ((ral >> 16) & 0xFF) as u8,
                ((ral >> 24) & 0xFF) as u8,
                (rah & 0xFF) as u8,
                ((rah >> 8) & 0xFF) as u8,
            ]
        }
    }

    fn setup_rx(&mut self) -> Result<(), ()> {
        unsafe {
            core::ptr::write_bytes(self.rx_desc as *mut u8, 0, size_of::<RxDesc>() * RX_RING_SIZE);
        }

        let mut rx_bufs: Vec<DmaRegion> = Vec::with_capacity(RX_RING_SIZE);
        for i in 0..RX_RING_SIZE {
            let buf = DmaRegion::alloc(RX_BUF_SIZE, 16).ok_or(())?;
            unsafe {
                write_volatile(
                    self.rx_desc.add(i),
                    RxDesc {
                        addr: buf.phys(),
                        length: 0,
                        csum: 0,
                        status: 0,
                        errors: 0,
                        special: 0,
                    },
                );
            }
            rx_bufs.push(buf);
        }
        self.rx_bufs = rx_bufs;
        self.rx_idx = 0;

        unsafe {
            self.mmio.write_u32(REG_RDBAL, self.rx_desc_mem.phys() as u32);
            self.mmio
                .write_u32(REG_RDBAH, (self.rx_desc_mem.phys() >> 32) as u32);
            self.mmio
                .write_u32(REG_RDLEN, (RX_RING_SIZE * size_of::<RxDesc>()) as u32);
            self.mmio.write_u32(REG_RDH, 0);
            self.mmio.write_u32(REG_RDT, (RX_RING_SIZE - 1) as u32);

            // Enable receiver: 2048-byte buffers, broadcast accept, strip CRC.
            let mut rctl = self.mmio.read_u32(REG_RCTL);
            rctl |= RCTL_EN | RCTL_BAM | RCTL_SECRC;
            // Clear buffer size bits (00 => 2048).
            rctl &= !((1 << 16) | (1 << 17) | (1 << 25));
            self.mmio.write_u32(REG_RCTL, rctl);
        }

        Ok(())
    }

    fn setup_tx(&mut self) -> Result<(), ()> {
        unsafe {
            core::ptr::write_bytes(self.tx_desc as *mut u8, 0, size_of::<TxDesc>() * TX_RING_SIZE);
        }

        let mut tx_bufs: Vec<DmaRegion> = Vec::with_capacity(TX_RING_SIZE);
        for i in 0..TX_RING_SIZE {
            let buf = DmaRegion::alloc(TX_BUF_SIZE, 16).ok_or(())?;
            unsafe {
                write_volatile(
                    self.tx_desc.add(i),
                    TxDesc {
                        addr: buf.phys(),
                        length: 0,
                        cso: 0,
                        cmd: 0,
                        status: TX_STATUS_DD,
                        css: 0,
                        special: 0,
                    },
                );
            }
            tx_bufs.push(buf);
        }
        self.tx_bufs = tx_bufs;
        self.tx_idx = 0;

        unsafe {
            self.mmio.write_u32(REG_TDBAL, self.tx_desc_mem.phys() as u32);
            self.mmio
                .write_u32(REG_TDBAH, (self.tx_desc_mem.phys() >> 32) as u32);
            self.mmio
                .write_u32(REG_TDLEN, (TX_RING_SIZE * size_of::<TxDesc>()) as u32);
            self.mmio.write_u32(REG_TDH, 0);
            self.mmio.write_u32(REG_TDT, 0);

            let mut tctl = self.mmio.read_u32(REG_TCTL);
            tctl |= TCTL_EN | TCTL_PSP;
            tctl |= 0x10 << TCTL_CT_SHIFT;
            tctl |= 0x40 << TCTL_COLD_SHIFT;
            self.mmio.write_u32(REG_TCTL, tctl);
            self.mmio.write_u32(REG_TIPG, 0x0060_200A);
        }

        Ok(())
    }

    fn poll_rx_ring(&mut self) {
        let ring_ptr = self.ring;
        let mut processed = 0;

        loop {
            if processed >= RX_RING_SIZE {
                break;
            }

            let idx = self.rx_idx;
            let desc = unsafe { read_volatile(self.rx_desc.add(idx)) };
            if (desc.status & RX_STATUS_DD) == 0 {
                break;
            }

            let len = min(desc.length as usize, RX_BUF_SIZE);
            if let Some(ring_ptr) = ring_ptr {
                let data = unsafe {
                    core::slice::from_raw_parts(self.rx_bufs[idx].virt() as *const u8, len)
                };
                unsafe {
                    let ring = &mut *ring_ptr;
                    let _ = ring.push_rx_packet(data);
                }
            }

            // Return descriptor to NIC.
            unsafe {
                let d = self.rx_desc.add(idx);
                (*d).status = 0;
            }

            self.rx_idx = (self.rx_idx + 1) % RX_RING_SIZE;
            let rdt = (self.rx_idx + RX_RING_SIZE - 1) % RX_RING_SIZE;
            unsafe {
                self.mmio.write_u32(REG_RDT, rdt as u32);
            }

            processed += 1;
        }
    }

    fn transmit_hw(&mut self, frame: &[u8]) -> Result<(), ()> {
        if frame.is_empty() {
            return Ok(());
        }
        if frame.len() > TX_BUF_SIZE {
            return Err(());
        }

        let idx = self.tx_idx;
        let cur = unsafe { read_volatile(self.tx_desc.add(idx)) };
        if (cur.status & TX_STATUS_DD) == 0 {
            return Err(());
        }

        unsafe {
            core::ptr::copy_nonoverlapping(frame.as_ptr(), self.tx_bufs[idx].virt(), frame.len());
        }

        unsafe {
            write_volatile(
                self.tx_desc.add(idx),
                TxDesc {
                    addr: self.tx_bufs[idx].phys(),
                    length: frame.len() as u16,
                    cso: 0,
                    cmd: TX_CMD_EOP | TX_CMD_IFCS | TX_CMD_RS,
                    status: 0,
                    css: 0,
                    special: 0,
                },
            );
        }

        self.tx_idx = (self.tx_idx + 1) % TX_RING_SIZE;
        unsafe {
            self.mmio.write_u32(REG_TDT, self.tx_idx as u32);
        }

        Ok(())
    }
}

impl VendorAdapter for E1000Adapter {
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

    #[inline]
    fn pci_device(&self) -> Option<pci::PciDevice> {
        Some(self.pci)
    }

    fn bind_ring(&mut self, ring: *mut NetRing) {
        self.ring = Some(ring);
    }
}

fn find_e1000_devices() -> alloc::vec::Vec<pci::PciDevice> {
    let mut out = alloc::vec::Vec::new();
    pci::with_devices(|list| {
        for dev in list {
            if dev.vendor != E1000_VENDOR_ID {
                continue;
            }
            if dev.device != E1000_DEVICE_ID {
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
    // Scan BAR0..BAR5 for the first memory BAR. QEMU e1000 can expose BAR0 as an IO BAR.
    let mut i = 0u8;
    while i < 6 {
        let off = 0x10u16 + (i as u16) * 4;
        let bar_lo = pci::config_read_u32(dev.bus, dev.slot, dev.function, off);
        if bar_lo == 0 {
            i += 1;
            continue;
        }

        // IO BAR?
        if (bar_lo & 0x1) != 0 {
            crate::log!(
                "net/e1000: bar{} is IO (raw=0x{:08x})\n",
                i,
                bar_lo
            );
            i += 1;
            continue;
        }

        let is_64 = ((bar_lo >> 1) & 0x3) == 0x2;
        let lo = (bar_lo as u64) & !0xFu64;
        let hi = if is_64 {
            let bar_hi = pci::config_read_u32(dev.bus, dev.slot, dev.function, off + 4);
            (bar_hi as u64) << 32
        } else {
            0
        };

        crate::log!(
            "net/e1000: bar{} mmio raw=0x{:08x}{} => 0x{:x}\n",
            i,
            bar_lo,
            if is_64 { " (64)" } else { "" },
            lo | hi
        );

        return Ok((i, lo | hi));
    }
    Err(())
}
