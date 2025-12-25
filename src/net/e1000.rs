use crate::isr;
use crate::{frame, mmio::MmioRegion, pci};
use alloc::{collections::VecDeque, vec::Vec};
use core::cmp::min;
use core::mem::size_of;
use core::sync::atomic::{AtomicU8, Ordering};
use spin::Mutex;

const E1000_VENDOR_ID: u16 = 0x8086;
const E1000_DEVICE_ID: u16 = 0x100E; // 82540EM

const REG_CTRL: u32 = 0x0000;
const REG_STATUS: u32 = 0x0008;
const REG_EECD: u32 = 0x0010;
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
const REG_IMS: u32 = 0x00D0;
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
const TX_CMD_EOP: u8 = 1 << 0;
const TX_CMD_IFCS: u8 = 1 << 1;
const TX_CMD_RS: u8 = 1 << 3;
const TX_STATUS_DD: u8 = 1 << 0;
const RAH_AV: u32 = 1 << 31;

const RX_RING_SIZE: usize = 64;
const RX_BUF_SIZE: usize = 2048;
const TX_RING_SIZE: usize = 64;
const TX_BUF_SIZE: usize = 2048;
const RX_QUEUE_DEPTH: usize = 32;

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

struct E1000 {
    mmio: MmioRegion,
    rx_ring_phys: u64,
    rx_ring: usize,
    rx_bufs: [u64; RX_RING_SIZE],
    rx_idx: usize,
    pkt_count: u64,
    tx_ring_phys: u64,
    tx_ring: usize,
    tx_bufs: [u64; TX_RING_SIZE],
    tx_idx: usize,
}

impl E1000 {
    fn from_bar(bar: u32) -> Option<Self> {
        if bar & 0x1 != 0 {
            return None; // IO BAR
        }
        let phys = (bar & 0xFFFF_FFF0) as usize;
        let mmio = match MmioRegion::map(phys, 0x20000) {
            Ok(region) => region,
            Err(err) => {
                crate::log_warn!("e1000: failed to map BAR0 0x{:08X}: {}", phys, err);
                return None;
            }
        };
        crate::log_info!(
            "e1000 MMIO: phys=0x{:08X} virt=0x{:016X}",
            mmio.phys_base(),
            mmio.as_ptr() as usize
        );
        Some(Self {
            mmio,
            rx_ring_phys: 0,
            rx_ring: 0,
            rx_bufs: [0; RX_RING_SIZE],
            rx_idx: 0,
            pkt_count: 0,
            tx_ring_phys: 0,
            tx_ring: 0,
            tx_bufs: [0; TX_RING_SIZE],
            tx_idx: 0,
        })
    }

    fn read_reg(&self, offset: u32) -> u32 {
        self.mmio.read_u32(offset as usize)
    }

    fn write_reg(&self, offset: u32, value: u32) {
        self.mmio.write_u32(offset as usize, value)
    }

    fn reset(&self) {
        let ctrl = self.read_reg(REG_CTRL);
        self.write_reg(REG_CTRL, ctrl | CTRL_RST);
        while self.read_reg(REG_CTRL) & CTRL_RST != 0 {}
    }

    fn setup_rx(&mut self) -> Result<(), ()> {
        // Allocate a single 4K frame for descriptors (fits 64 descriptors).
        let ring_phys = frame::alloc_frame_4k().ok_or(())?;
        let ring_virt = crate::phys::phys_to_virt(ring_phys as usize) as usize;
        // Zero descriptors.
        unsafe {
            core::ptr::write_bytes(ring_virt as *mut u8, 0, RX_RING_SIZE * size_of::<RxDesc>());
        }

        // Allocate RX buffers.
        for i in 0..RX_RING_SIZE {
            let buf_phys = frame::alloc_frame_4k().ok_or(())?;
            self.rx_bufs[i] = buf_phys;
            unsafe {
                let desc = (ring_virt as *mut RxDesc).add(i);
                (*desc).addr = buf_phys;
                (*desc).status = 0;
            }
        }

        self.rx_ring_phys = ring_phys;
        self.rx_ring = ring_virt;
        self.rx_idx = 0;

        // Program registers.
        self.write_reg(REG_RDBAL, (ring_phys & 0xFFFF_FFFF) as u32);
        self.write_reg(REG_RDBAH, (ring_phys >> 32) as u32);
        self.write_reg(REG_RDLEN, (RX_RING_SIZE * size_of::<RxDesc>()) as u32);
        self.write_reg(REG_RDH, 0);
        self.write_reg(REG_RDT, (RX_RING_SIZE - 1) as u32);

        // Enable receiver: 2048-byte buffers, broadcast accept, strip CRC.
        let mut rctl = self.read_reg(REG_RCTL);
        rctl |= RCTL_EN | RCTL_BAM | RCTL_SECRC;
        // Clear buffer size bits (00 => 2048).
        rctl &= !((1 << 16) | (1 << 17) | (1 << 25));
        self.write_reg(REG_RCTL, rctl);
        Ok(())
    }

    fn setup_tx(&mut self) -> Result<(), ()> {
        let ring_phys = frame::alloc_frame_4k().ok_or(())?;
        let ring_virt = crate::phys::phys_to_virt(ring_phys as usize) as usize;
        unsafe {
            core::ptr::write_bytes(ring_virt as *mut u8, 0, TX_RING_SIZE * size_of::<TxDesc>());
        }

        for i in 0..TX_RING_SIZE {
            let buf_phys = frame::alloc_frame_4k().ok_or(())?;
            self.tx_bufs[i] = buf_phys;
            unsafe {
                let desc = (ring_virt as *mut TxDesc).add(i);
                (*desc).addr = buf_phys;
                (*desc).status = TX_STATUS_DD;
            }
        }

        self.tx_ring_phys = ring_phys;
        self.tx_ring = ring_virt;
        self.tx_idx = 0;

        self.write_reg(REG_TDBAL, (ring_phys & 0xFFFF_FFFF) as u32);
        self.write_reg(REG_TDBAH, (ring_phys >> 32) as u32);
        self.write_reg(REG_TDLEN, (TX_RING_SIZE * size_of::<TxDesc>()) as u32);
        self.write_reg(REG_TDH, 0);
        self.write_reg(REG_TDT, 0);

        let mut tctl = self.read_reg(REG_TCTL);
        tctl |= TCTL_EN | TCTL_PSP;
        tctl |= 0x10 << TCTL_CT_SHIFT;
        tctl |= 0x40 << TCTL_COLD_SHIFT;
        self.write_reg(REG_TCTL, tctl);
        self.write_reg(REG_TIPG, 0x0060_200A);
        Ok(())
    }

    fn transmit(&mut self, data: &[u8]) -> Result<(), ()> {
        if self.tx_ring == 0 {
            return Err(());
        }
        if data.len() > TX_BUF_SIZE {
            return Err(());
        }
        let idx = self.tx_idx;
        let desc = unsafe { &mut *(self.tx_ring as *mut TxDesc).add(idx) };
        if desc.status & TX_STATUS_DD == 0 {
            return Err(());
        }

        let buf_virt = crate::phys::phys_to_virt(self.tx_bufs[idx] as usize);
        unsafe {
            core::ptr::copy_nonoverlapping(data.as_ptr(), buf_virt as *mut u8, data.len());
        }

        desc.length = data.len() as u16;
        desc.cmd = TX_CMD_EOP | TX_CMD_IFCS | TX_CMD_RS;
        desc.status = 0;

        self.tx_idx = (self.tx_idx + 1) % TX_RING_SIZE;
        self.write_reg(REG_TDT, self.tx_idx as u32);
        Ok(())
    }

    fn poll_rx(&mut self) {
        if self.rx_ring == 0 {
            return;
        }
        let mut processed = 0;
        loop {
            let idx = self.rx_idx;
            let desc = unsafe { &mut *(self.rx_ring as *mut RxDesc).add(idx) };
            let status = desc.status;
            if status & 0x1 == 0 {
                break;
            }
            let len = desc.length as usize;
            self.pkt_count += 1;
            let buf_virt = crate::phys::phys_to_virt(self.rx_bufs[idx] as usize);
            let packet = unsafe {
                core::slice::from_raw_parts(buf_virt as *const u8, min(len, RX_BUF_SIZE))
            };
            enqueue_rx(packet);
            // Return descriptor to NIC.
            desc.status = 0;
            self.rx_idx = (self.rx_idx + 1) % RX_RING_SIZE;
            let rdt = (self.rx_idx + RX_RING_SIZE - 1) % RX_RING_SIZE;
            self.write_reg(REG_RDT, rdt as u32);
            processed += 1;
            if processed >= RX_RING_SIZE {
                break;
            }
        }
    }

    fn mac(&self) -> [u8; 6] {
        let ral = self.read_reg(REG_RAL0);
        let rah = self.read_reg(REG_RAH0);
        if rah & RAH_AV == 0 {
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

static DEV: Mutex<Option<E1000>> = Mutex::new(None);
static IRQ_LINE: AtomicU8 = AtomicU8::new(0xFF);
static RX_QUEUE: Mutex<VecDeque<Vec<u8>>> = Mutex::new(VecDeque::new());

#[allow(clippy::result_unit_err)]
pub fn init() -> Result<(), ()> {
    let device = match pci::find_device(E1000_VENDOR_ID, E1000_DEVICE_ID) {
        Some(dev) => dev,
        None => return Err(()),
    };

    crate::log_info!(
        "e1000 detected at bus {:02X} slot {:02X} func {}",
        device.bus,
        device.slot,
        device.function
    );

    let bar0 = pci::read_config_u32(&device, 0x10);
    crate::log_info!("e1000 BAR0 raw: 0x{:08X}", bar0);
    let mut nic = E1000::from_bar(bar0).ok_or(())?;

    nic.reset();

    let status = nic.read_reg(REG_STATUS);
    let eecd = nic.read_reg(REG_EECD);
    crate::log_info!("e1000 status: 0x{:08X} eecd: 0x{:08X}", status, eecd);

    configure_command_register(&device);

    if nic.setup_rx().is_err() {
        crate::log_warn!("e1000: failed to set up RX ring.");
    } else {
        crate::log_info!(
            "e1000: RX ring initialized ({} desc, {}B buffers).",
            RX_RING_SIZE,
            RX_BUF_SIZE
        );
    }

    if nic.setup_tx().is_err() {
        crate::log_warn!("e1000: failed to set up TX ring.");
    } else {
        crate::log_info!(
            "e1000: TX ring initialized ({} desc, {}B buffers).",
            TX_RING_SIZE,
            TX_BUF_SIZE
        );
    }

    // Enable interrupts (legacy INTx). Mask all, clear pending, then unmask basic RX.
    nic.write_reg(REG_IMC, 0xFFFF_FFFF);
    nic.read_reg(REG_ICR);
    // Enable RX-related interrupts: RXDW (bit 0) and RXO (bit 6).
    nic.write_reg(REG_IMS, 0x0000_0041);

    // Register IRQ handler.
    let irq_line = pci::read_config_u8(&device, 0x3C) & 0x1F;
    IRQ_LINE.store(irq_line, Ordering::Relaxed);
    if let Err(err) = isr::register_irq_handler(irq_line, e1000_irq_handler) {
        crate::log_warn!("e1000: failed to register IRQ handler: {}", err);
    } else {
        isr::unmask_irq(irq_line);
        crate::log_info!("e1000: INTx enabled on IRQ {}", irq_line);
    }

    let mut guard = DEV.lock();
    *guard = Some(nic);

    Ok(())
}

fn configure_command_register(device: &pci::PciDevice) {
    let mask = pci::COMMAND_IO_SPACE | pci::COMMAND_MEMORY_SPACE | pci::COMMAND_BUS_MASTER;
    if let Some((command, changed)) = pci::ensure_command_bits(device, mask) {
        if changed {
            crate::log_info!(
                "e1000: {:02X}:{:02X}.{} PCI command -> 0x{:04X}",
                device.bus,
                device.slot,
                device.function,
                command
            );
        }
    }
}

/// Poll the NIC for received packets.
pub fn poll() {
    if let Some(ref mut nic) = *DEV.lock() {
        nic.poll_rx();
    }
}

pub fn pop_rx_packet() -> Option<Vec<u8>> {
    let mut q = RX_QUEUE.lock();
    q.pop_front()
}

#[allow(clippy::result_unit_err)]
pub fn transmit_packet(data: &[u8]) -> Result<(), ()> {
    let mut guard = DEV.lock();
    if let Some(ref mut nic) = *guard {
        nic.transmit(data)
    } else {
        Err(())
    }
}

pub fn mac_address() -> Option<[u8; 6]> {
    let guard = DEV.lock();
    guard.as_ref().map(|nic| nic.mac())
}

fn irq_line() -> u8 {
    IRQ_LINE.load(Ordering::Relaxed)
}

fn e1000_irq_handler(_ctx: &mut isr::AsyncFrame<'_>) {
    let mut guard = DEV.lock();
    if let Some(ref mut nic) = *guard {
        // Reading ICR acknowledges.
        let _icr = nic.read_reg(REG_ICR);
        nic.poll_rx();
    }
    isr::acknowledge_irq(irq_line());
}

fn enqueue_rx(packet: &[u8]) {
    let mut q = RX_QUEUE.lock();
    if q.len() >= RX_QUEUE_DEPTH {
        q.pop_front();
    }
    let mut vec = Vec::with_capacity(packet.len());
    vec.extend_from_slice(packet);
    q.push_back(vec);
}
