use alloc::vec::Vec;

use core::cmp::min;
use core::mem::size_of;
use core::ptr::{NonNull, read_volatile, write_volatile};
use core::sync::atomic::{Ordering, fence};

use crate::net::core::VendorAdapter;
use crate::net::device::LinkState;
use crate::net::ring::{DmaRegion, NetRing};
use crate::pci;

const INTEL_VENDOR_ID: u16 = 0x8086;
const E1000_82540EM_DEVICE_ID: u16 = 0x100E; // QEMU `-device e1000`

// Intel PCH LAN device IDs handled by Linux e1000e.  I219 is the first
// bare-metal target, while I217/I218 use the same conservative PCH datapath
// profile and are cheap to keep in the probe table.
const PCH_LAN_DEVICE_IDS: &[u16] = &[
    // I217 / I218
    0x153A, 0x153B, 0x155A, 0x1559, 0x15A0, 0x15A1, 0x15A2, 0x15A3,
    // I219 SPT through CNP/ICP/CMP
    0x156F, 0x1570, 0x15B7, 0x15B8, 0x15B9, 0x15D7, 0x15D8, 0x15E3, 0x15D6, 0x15BD, 0x15BE, 0x15BB,
    0x15BC, 0x15DF, 0x15E0, 0x15E1, 0x15E2, 0x0D4E, 0x0D4F, 0x0D4C, 0x0D4D, 0x0D53, 0x0D55,
    // I219 TGP / ADP / RPL / MTP / LNP / ARL / PTP / NVL
    0x15FB, 0x15FC, 0x15F9, 0x15FA, 0x15F4, 0x15F5, 0x0DC5, 0x0DC6, 0x1A1E, 0x1A1F, 0x1A1C, 0x1A1D,
    0x0DC7, 0x0DC8, 0x550A, 0x550B, 0x550C, 0x550D, 0x550E, 0x550F, 0x5510, 0x5511, 0x57A0, 0x57A1,
    0x57B3, 0x57B4, 0x57B7, 0x57B8, 0x57B9, 0x57BA,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IntelNicKind {
    Legacy82540Em,
    PchLan,
}

impl IntelNicKind {
    fn name(self) -> &'static str {
        match self {
            Self::Legacy82540Em => "82540EM",
            Self::PchLan => "I217/I218/I219 PCH-LAN",
        }
    }
}

const REG_CTRL: u32 = 0x0000;
const REG_STATUS: u32 = 0x0008;
const REG_CTRL_EXT: u32 = 0x0018;
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
const REG_TXDCTL: u32 = 0x3828;
const REG_ICR: u32 = 0x00C0;
const REG_IMC: u32 = 0x00D8;
const REG_RFCTL: u32 = 0x5008;
const REG_RAL0: u32 = 0x5400;
const REG_RAH0: u32 = 0x5404;

const CTRL_RST: u32 = 1 << 26;
const CTRL_EXT_RO_DIS: u32 = 1 << 17;
const CTRL_EXT_DRV_LOAD: u32 = 1 << 28;

const STATUS_FD: u32 = 1 << 0;
const STATUS_LU: u32 = 1 << 1;
const STATUS_SPEED_MASK: u32 = 0xC0;
const STATUS_SPEED_100: u32 = 0x40;
const STATUS_SPEED_1000: u32 = 0x80;

const RCTL_EN: u32 = 1 << 1;
const RCTL_BAM: u32 = 1 << 15;
const RCTL_SECRC: u32 = 1 << 26;
const RCTL_DTYP_PS: u32 = 1 << 10;
const RCTL_BSIZE_MASK: u32 = (1 << 16) | (1 << 17) | (1 << 25);

const RFCTL_EXTEN: u32 = 1 << 15;

const TCTL_EN: u32 = 1 << 1;
const TCTL_PSP: u32 = 1 << 3;
const TCTL_CT_MASK: u32 = 0xFF << 4;
const TCTL_CT_SHIFT: u32 = 4;
const TCTL_COLD_MASK: u32 = 0x3FF << 12;
const TCTL_COLD_SHIFT: u32 = 12;
const TCTL_RTLC: u32 = 1 << 24;

const TXDCTL_PTHRESH_MASK: u32 = 0x3F;
const TXDCTL_HTHRESH_MASK: u32 = 0x3F << 8;
const TXDCTL_WTHRESH_MASK: u32 = 0x3F << 16;
const TXDCTL_GRAN: u32 = 1 << 24;

const RX_STATUS_DD: u8 = 1 << 0;
const RX_STATUS_EOP: u8 = 1 << 1;

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
    kind: IntelNicKind,
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
        let kind = nic_kind(dev.device).ok_or(())?;
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
            "net/e1000: found {} {:02x}:{:02x}.{} vid={:04x} did={:04x} mmio=bar{}@0x{:x}\n",
            kind.name(),
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
            kind,
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

        // Integrated PCH LAN reset also resets the PHY and requires the full
        // e1000e software/firmware semaphore and PHY post-reset sequence.  For
        // the thin I219 adapter, preserve the link configured by firmware and
        // only take ownership of the host DMA engines.  The emulated 82540EM
        // has no such firmware relationship and keeps the simple global reset.
        match adapter.kind {
            IntelNicKind::Legacy82540Em => {
                adapter.reset_legacy()?;
                // Allow EEPROM autoload to repopulate RAR0 after reset.
                let _ = crate::wait::spin_until_timeout_no_exec(10, || false);
            }
            IntelNicKind::PchLan => adapter.claim_pch(),
        }

        adapter.mac = adapter.read_mac();
        if !valid_mac(adapter.mac) {
            crate::log_warn!(target: "net";
                "net/e1000: {} has no valid MAC in RAR0; refusing active DMA\n",
                adapter.kind.name()
            );
            return Err(());
        }

        adapter.quiesce();

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

        adapter.program_mac();
        crate::log!(
            "net/e1000: {} mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} link={}Mbps mode=polling\n",
            adapter.kind.name(),
            adapter.mac[0],
            adapter.mac[1],
            adapter.mac[2],
            adapter.mac[3],
            adapter.mac[4],
            adapter.mac[5],
            adapter.link_state_hw().speed_mbps
        );

        Ok(adapter)
    }

    fn reset_legacy(&mut self) -> Result<(), ()> {
        unsafe {
            let ctrl = self.mmio.read_u32(REG_CTRL);
            self.mmio.write_u32(REG_CTRL, ctrl | CTRL_RST);
        }
        if crate::wait::spin_until_timeout_no_exec(100, || unsafe {
            (self.mmio.read_u32(REG_CTRL) & CTRL_RST) == 0
        }) {
            Ok(())
        } else {
            crate::log_warn!(target: "net"; "net/e1000: 82540EM reset timed out\n");
            Err(())
        }
    }

    fn claim_pch(&mut self) {
        unsafe {
            let ctrl_ext = self.mmio.read_u32(REG_CTRL_EXT);
            self.mmio
                .write_u32(REG_CTRL_EXT, ctrl_ext | CTRL_EXT_DRV_LOAD | CTRL_EXT_RO_DIS);
        }
    }

    fn quiesce(&mut self) {
        unsafe {
            let rctl = self.mmio.read_u32(REG_RCTL);
            self.mmio.write_u32(REG_RCTL, rctl & !RCTL_EN);
            let tctl = self.mmio.read_u32(REG_TCTL);
            self.mmio.write_u32(REG_TCTL, tctl & !TCTL_EN);
            let _ = self.mmio.read_u32(REG_STATUS); // flush posted writes
        }
        // The hardware can still own old descriptors briefly after EN clears.
        let _ = crate::wait::spin_until_timeout_no_exec(10, || false);
    }

    fn program_mac(&self) {
        let ral = u32::from(self.mac[0])
            | (u32::from(self.mac[1]) << 8)
            | (u32::from(self.mac[2]) << 16)
            | (u32::from(self.mac[3]) << 24);
        let rah = u32::from(self.mac[4]) | (u32::from(self.mac[5]) << 8) | RAH_AV;
        unsafe {
            self.mmio.write_u32(REG_RAL0, ral);
            self.mmio.write_u32(REG_RAH0, rah);
        }
    }

    fn link_state_hw(&self) -> LinkState {
        let status = unsafe { self.mmio.read_u32(REG_STATUS) };
        let speed_mbps = match status & STATUS_SPEED_MASK {
            STATUS_SPEED_100 => 100,
            STATUS_SPEED_1000 => 1000,
            _ => 10,
        };
        LinkState {
            up: (status & STATUS_LU) != 0,
            speed_mbps,
            full_duplex: (status & STATUS_FD) != 0,
        }
    }

    fn tx_descriptor_ready(&self) -> bool {
        let desc = unsafe { read_volatile(self.tx_desc.add(self.tx_idx)) };
        (desc.status & TX_STATUS_DD) != 0
    }

    fn configure_legacy_rx_descriptors(&self) {
        unsafe {
            // Linux normally selects extended descriptors on e1000e.  TRUEOS
            // intentionally reuses its smaller 82540-compatible 16-byte RX
            // descriptors, so make that choice explicit after firmware handoff.
            let rfctl = self.mmio.read_u32(REG_RFCTL);
            self.mmio.write_u32(REG_RFCTL, rfctl & !RFCTL_EXTEN);

            let rctl = self.mmio.read_u32(REG_RCTL);
            self.mmio.write_u32(REG_RCTL, rctl & !RCTL_DTYP_PS);
        }
    }

    fn configure_tx_descriptor_policy(&self) {
        if self.kind != IntelNicKind::PchLan {
            return;
        }
        unsafe {
            // Conservative e1000e PCH write-back/prefetch policy: one completed
            // descriptor per write-back and up to 31 descriptors prefetched.
            let mut txdctl = self.mmio.read_u32(REG_TXDCTL);
            txdctl &= !(TXDCTL_PTHRESH_MASK | TXDCTL_HTHRESH_MASK | TXDCTL_WTHRESH_MASK);
            txdctl |= TXDCTL_GRAN | 0x1F | (1 << 16);
            self.mmio.write_u32(REG_TXDCTL, txdctl);
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

        self.configure_legacy_rx_descriptors();

        unsafe {
            self.mmio
                .write_u32(REG_RDBAL, self.rx_desc_mem.phys() as u32);
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
            rctl &= !RCTL_BSIZE_MASK;
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

        self.configure_tx_descriptor_policy();

        unsafe {
            self.mmio
                .write_u32(REG_TDBAL, self.tx_desc_mem.phys() as u32);
            self.mmio
                .write_u32(REG_TDBAH, (self.tx_desc_mem.phys() >> 32) as u32);
            self.mmio
                .write_u32(REG_TDLEN, (TX_RING_SIZE * size_of::<TxDesc>()) as u32);
            self.mmio.write_u32(REG_TDH, 0);
            self.mmio.write_u32(REG_TDT, 0);

            let mut tctl = self.mmio.read_u32(REG_TCTL);
            tctl &= !(TCTL_CT_MASK | TCTL_COLD_MASK);
            tctl |= TCTL_EN | TCTL_PSP | TCTL_RTLC;
            tctl |= 0x10 << TCTL_CT_SHIFT;
            tctl |= 0x40 << TCTL_COLD_SHIFT;
            self.mmio.write_u32(REG_TCTL, tctl);
            if self.kind == IntelNicKind::Legacy82540Em {
                self.mmio.write_u32(REG_TIPG, 0x0060_200A);
            }
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
            if desc.errors == 0
                && (desc.status & RX_STATUS_EOP) != 0
                && len != 0
                && let Some(ring_ptr) = ring_ptr
            {
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
                write_volatile(core::ptr::addr_of_mut!((*self.rx_desc.add(idx)).status), 0);
            }

            self.rx_idx = (self.rx_idx + 1) % RX_RING_SIZE;
            let rdt = (self.rx_idx + RX_RING_SIZE - 1) % RX_RING_SIZE;
            fence(Ordering::Release);
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
        fence(Ordering::Release);
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

    fn transmit_ready(&mut self) -> bool {
        self.link_state_hw().up && self.tx_descriptor_ready()
    }

    fn link_state(&self) -> LinkState {
        self.link_state_hw()
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
            if dev.vendor != INTEL_VENDOR_ID {
                continue;
            }
            if nic_kind(dev.device).is_none() {
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

fn nic_kind(device_id: u16) -> Option<IntelNicKind> {
    if device_id == E1000_82540EM_DEVICE_ID {
        Some(IntelNicKind::Legacy82540Em)
    } else if PCH_LAN_DEVICE_IDS.contains(&device_id) {
        Some(IntelNicKind::PchLan)
    } else {
        None
    }
}

fn valid_mac(mac: [u8; 6]) -> bool {
    mac != [0; 6] && mac != [0xFF; 6] && (mac[0] & 1) == 0
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
            crate::log!("net/e1000: bar{} is IO (raw=0x{:08x})\n", i, bar_lo);
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
