use alloc::vec::Vec;

use crate::net::core::VendorAdapter;
use crate::net::ring::{DmaRegion, NetRing};
use crate::pci;

const VIRTIO_PCI_VENDOR: u16 = 0x1AF4;
const VIRTIO_NET_DEVICE_LEGACY: u16 = 0x1000;
const VIRTIO_NET_DEVICE_MODERN: u16 = 0x1041;

const VIRTIO_PCI_IOBAR_OFFSET: u16 = 0x10;
const VIRTIO_PCI_COMMAND_OFFSET: u16 = 0x04;
const VIRTIO_PCI_COMMAND_IO: u16 = 1 << 0;
const VIRTIO_PCI_COMMAND_BUS_MASTER: u16 = 1 << 2;

const VIRTIO_PCI_REG_DEVICE_FEATURES: u16 = 0x00;
const VIRTIO_PCI_REG_GUEST_FEATURES: u16 = 0x04;
const VIRTIO_PCI_REG_QUEUE_ADDRESS: u16 = 0x08;
const VIRTIO_PCI_REG_QUEUE_SIZE: u16 = 0x0C;
const VIRTIO_PCI_REG_QUEUE_SELECT: u16 = 0x0E;
const VIRTIO_PCI_REG_QUEUE_NOTIFY: u16 = 0x10;
const VIRTIO_PCI_REG_DEVICE_STATUS: u16 = 0x12;
const VIRTIO_PCI_REG_DEVICE_CFG: u16 = 0x14;
const VIRTIO_PCI_REG_GUEST_PAGE_SIZE: u16 = 0x28;

const VIRTIO_STATUS_ACK: u8 = 0x01;
const VIRTIO_STATUS_DRIVER: u8 = 0x02;
const VIRTIO_STATUS_DRIVER_OK: u8 = 0x04;

const VIRTIO_NET_F_MAC: u32 = 1 << 5;

const QUEUE_RX: u16 = 0;
const QUEUE_TX: u16 = 1;

const VIRTQ_DESC_F_WRITE: u16 = 2;

const VIRTIO_NET_HDR_SIZE: usize = 10;
const RX_BUF_SIZE: usize = 2048 + VIRTIO_NET_HDR_SIZE;
const TX_BUF_SIZE: usize = 2048 + VIRTIO_NET_HDR_SIZE;

#[repr(C, packed)]
struct VirtqDesc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

#[repr(C, packed)]
struct VirtqUsedElem {
    id: u32,
    len: u32,
}

struct VirtQueue {
    size: u16,
    _mem: DmaRegion,
    desc: *mut VirtqDesc,
    avail: *mut u8,
    used: *mut u8,
    avail_idx: u16,
    last_used_idx: u16,
}

// Safety: this queue is only accessed under the net device mutex / single-threaded net task.
unsafe impl Send for VirtQueue {}

impl VirtQueue {
    fn new(size: u16, mem: DmaRegion, desc: *mut VirtqDesc, avail: *mut u8, used: *mut u8) -> Self {
        Self {
            size,
            _mem: mem,
            desc,
            avail,
            used,
            avail_idx: 0,
            last_used_idx: 0,
        }
    }

    fn avail_ring_ptr(&self, index: u16) -> *mut u16 {
        let offset = 4 + (index as usize * 2);
        unsafe { self.avail.add(offset) as *mut u16 }
    }

    fn used_idx(&self) -> u16 {
        unsafe { core::ptr::read_volatile(self.used.add(2) as *const u16) }
    }

    fn used_elem(&self, index: u16) -> VirtqUsedElem {
        let offset = 4 + (index as usize * 8);
        let ptr = unsafe { self.used.add(offset) as *const VirtqUsedElem };
        unsafe { core::ptr::read_volatile(ptr) }
    }

    fn push_avail(&mut self, desc_index: u16) {
        unsafe {
            core::ptr::write_volatile(self.avail_ring_ptr(self.avail_idx % self.size), desc_index);
            let idx_ptr = self.avail.add(2) as *mut u16;
            self.avail_idx = self.avail_idx.wrapping_add(1);
            core::ptr::write_volatile(idx_ptr, self.avail_idx);
        }
    }
}

pub struct VirtioNetAdapter {
    ring: Option<*mut NetRing>,
    pci: pci::PciDevice,
    io_base: u16,
    mac: [u8; 6],
    rxq: VirtQueue,
    txq: VirtQueue,
    rx_bufs: Vec<DmaRegion>,
    tx_bufs: Vec<DmaRegion>,
    tx_free: Vec<u16>,

    // Coalesce port-IO notifies during bursts. We still guarantee an eventual kick
    // from the RX poll path.
    tx_last_notify_avail: u16,
}

// Safety: this adapter is driven by the net task and protected by the global net mutex.
unsafe impl Send for VirtioNetAdapter {}

impl VirtioNetAdapter {
    pub fn init_all() -> alloc::vec::Vec<Self> {
        let mut out = alloc::vec::Vec::new();
        let devs = find_virtio_net_devices();
        for dev in devs {
            match Self::init_from_device(dev) {
                Ok(adapter) => out.push(adapter),
                Err(()) => {
                    crate::log!(
                        "net/vio: init failed for {:02x}:{:02x}.{}\n",
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
        let io_base = read_io_base(&dev)?;
        enable_io_and_bus_master(&dev);

        crate::log!(
            "net/vio: found virtio-net {:02x}:{:02x}.{} vid={:04x} did={:04x} io_base=0x{:04x}\n",
            dev.bus,
            dev.slot,
            dev.function,
            dev.vendor,
            dev.device,
            io_base
        );

        reset_device(io_base);
        set_status(io_base, VIRTIO_STATUS_ACK | VIRTIO_STATUS_DRIVER);

        // Legacy virtio PCI requires the guest to program page size (used by PFN-based queue regs).
        // QEMU typically expects 4096 here.
        unsafe { crate::portio::outl(io_base + VIRTIO_PCI_REG_GUEST_PAGE_SIZE, 4096) };

        let features = read_device_features(io_base);
        let mut guest = 0u32;
        if features & VIRTIO_NET_F_MAC != 0 {
            guest |= VIRTIO_NET_F_MAC;
        }
        write_guest_features(io_base, guest);

        let rxq = setup_queue(io_base, QUEUE_RX)?;
        let mut txq = setup_queue(io_base, QUEUE_TX)?;

        let mac = if guest & VIRTIO_NET_F_MAC != 0 {
            read_mac(io_base)
        } else {
            [0; 6]
        };

        crate::log!(
            "net/vio: features=0x{:08x} guest=0x{:08x} mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}\n",
            features,
            guest,
            mac[0],
            mac[1],
            mac[2],
            mac[3],
            mac[4],
            mac[5]
        );

        let (rx_bufs, rxq) = init_rx_buffers(io_base, rxq)?;
        let (tx_bufs, tx_free) = init_tx_buffers(&mut txq)?;

        set_status(
            io_base,
            VIRTIO_STATUS_ACK | VIRTIO_STATUS_DRIVER | VIRTIO_STATUS_DRIVER_OK,
        );

        Ok(Self {
            ring: None,
            pci: dev,
            io_base,
            mac,
            rxq,
            txq,
            rx_bufs,
            tx_bufs,
            tx_free,

            tx_last_notify_avail: 0,
        })
    }
}

impl VendorAdapter for VirtioNetAdapter {
    fn mac(&self) -> [u8; 6] {
        self.mac
    }

    fn poll_rx(&mut self) {
        self.poll_rx_queue();
        self.reclaim_tx();
    }

    fn pop_rx(&mut self) -> Option<Vec<u8>> {
        None
    }

    fn transmit(&mut self, frame: &[u8]) -> Result<(), ()> {
        self.tx_submit_hw(frame)
    }

    #[inline]
    fn pci_device(&self) -> Option<pci::PciDevice> {
        Some(self.pci)
    }

    fn bind_ring(&mut self, ring: *mut NetRing) {
        self.ring = Some(ring);
    }
}

fn find_virtio_net_devices() -> alloc::vec::Vec<pci::PciDevice> {
    let mut out = alloc::vec::Vec::new();
    pci::with_devices(|list| {
        for dev in list {
            if dev.vendor == VIRTIO_PCI_VENDOR
                && (dev.device == VIRTIO_NET_DEVICE_LEGACY
                    || dev.device == VIRTIO_NET_DEVICE_MODERN)
            {
                out.push(*dev);
            }
        }
    });
    out
}

fn read_io_base(dev: &pci::PciDevice) -> Result<u16, ()> {
    let bar0 = pci::config_read_u32(dev.bus, dev.slot, dev.function, VIRTIO_PCI_IOBAR_OFFSET);
    if (bar0 & 0x1) == 0 {
        return Err(());
    }
    let port = (bar0 & 0xFFFF_FFFC) as u16;
    Ok(port)
}

fn enable_io_and_bus_master(dev: &pci::PciDevice) {
    let mut cmd = pci::config_read_u16(dev.bus, dev.slot, dev.function, VIRTIO_PCI_COMMAND_OFFSET);
    cmd |= VIRTIO_PCI_COMMAND_IO | VIRTIO_PCI_COMMAND_BUS_MASTER;
    pci::config_write_u16(
        dev.bus,
        dev.slot,
        dev.function,
        VIRTIO_PCI_COMMAND_OFFSET,
        cmd,
    );
}

fn reset_device(io_base: u16) {
    unsafe { crate::portio::outb(io_base + VIRTIO_PCI_REG_DEVICE_STATUS, 0) };
}

fn set_status(io_base: u16, status: u8) {
    unsafe { crate::portio::outb(io_base + VIRTIO_PCI_REG_DEVICE_STATUS, status) };
}

fn read_device_features(io_base: u16) -> u32 {
    unsafe { crate::portio::inl(io_base + VIRTIO_PCI_REG_DEVICE_FEATURES) }
}

fn write_guest_features(io_base: u16, features: u32) {
    unsafe { crate::portio::outl(io_base + VIRTIO_PCI_REG_GUEST_FEATURES, features) };
}

fn select_queue(io_base: u16, queue: u16) {
    unsafe { crate::portio::outw(io_base + VIRTIO_PCI_REG_QUEUE_SELECT, queue) };
}

fn read_queue_size(io_base: u16) -> u16 {
    unsafe { crate::portio::inw(io_base + VIRTIO_PCI_REG_QUEUE_SIZE) }
}

fn write_queue_addr(io_base: u16, pfn: u32) {
    unsafe { crate::portio::outl(io_base + VIRTIO_PCI_REG_QUEUE_ADDRESS, pfn) };
}

fn notify_queue(io_base: u16, queue: u16) {
    unsafe { crate::portio::outw(io_base + VIRTIO_PCI_REG_QUEUE_NOTIFY, queue) };
}

fn read_mac(io_base: u16) -> [u8; 6] {
    let mut mac = [0u8; 6];
    for i in 0..6 {
        mac[i] = unsafe { crate::portio::inb(io_base + VIRTIO_PCI_REG_DEVICE_CFG + i as u16) };
    }
    mac
}

fn align_up(value: usize, align: usize) -> usize {
    if align == 0 {
        return value;
    }
    (value + align - 1) / align * align
}

fn setup_queue(io_base: u16, queue_index: u16) -> Result<VirtQueue, ()> {
    select_queue(io_base, queue_index);
    let size = read_queue_size(io_base);
    if size == 0 {
        return Err(());
    }

    let desc_size = size as usize * core::mem::size_of::<VirtqDesc>();
    // Legacy virtqueue layout (no EVENT_IDX negotiated):
    // avail: flags(u16) + idx(u16) + ring[size](u16)
    let avail_size = 4 + (size as usize * 2);
    // For virtio-pci legacy, the used ring is page-aligned (Linux/QEMU use align=PAGE_SIZE).
    let used_offset = align_up(desc_size + avail_size, 4096);
    // used: flags(u16) + idx(u16) + ring[size](VirtqUsedElem)
    let used_size = 4 + (size as usize * 8);
    let total = align_up(used_offset + used_size, 4096);

    let mem = DmaRegion::alloc(total, 4096).ok_or(())?;
    unsafe { core::ptr::write_bytes(mem.virt(), 0, total) };

    let desc = mem.virt() as *mut VirtqDesc;
    let avail = unsafe { mem.virt().add(desc_size) };
    let used = unsafe { mem.virt().add(used_offset) };

    let pfn = (mem.phys() >> 12) as u32;
    write_queue_addr(io_base, pfn);

    Ok(VirtQueue::new(size, mem, desc, avail, used))
}

fn init_rx_buffers(io_base: u16, mut rxq: VirtQueue) -> Result<(Vec<DmaRegion>, VirtQueue), ()> {
    let mut buffers = Vec::with_capacity(rxq.size as usize);
    for i in 0..rxq.size {
        let buf = DmaRegion::alloc(RX_BUF_SIZE, 16).ok_or(())?;
        let desc = unsafe { &mut *rxq.desc.add(i as usize) };
        desc.addr = buf.phys();
        desc.len = RX_BUF_SIZE as u32;
        desc.flags = VIRTQ_DESC_F_WRITE;
        desc.next = 0;
        rxq.push_avail(i);
        buffers.push(buf);
    }
    notify_queue(io_base, QUEUE_RX);
    Ok((buffers, rxq))
}

fn init_tx_buffers(txq: &mut VirtQueue) -> Result<(Vec<DmaRegion>, Vec<u16>), ()> {
    let mut buffers = Vec::with_capacity(txq.size as usize);
    let mut free = Vec::with_capacity(txq.size as usize);
    for i in 0..txq.size {
        let buf = DmaRegion::alloc(TX_BUF_SIZE, 16).ok_or(())?;
        let desc = unsafe { &mut *txq.desc.add(i as usize) };
        desc.addr = buf.phys();
        desc.len = 0;
        desc.flags = 0;
        desc.next = 0;
        buffers.push(buf);
        free.push(i);
    }
    Ok((buffers, free))
}

impl VirtioNetAdapter {
    #[inline]
    fn maybe_kick_tx(&mut self, force: bool) {
        // Notify if we've queued a burst of descriptors, or if forced (e.g. end of poll).
        const TX_NOTIFY_THRESHOLD: u16 = 8;
        let pending = self.txq.avail_idx.wrapping_sub(self.tx_last_notify_avail);
        if force || pending >= TX_NOTIFY_THRESHOLD {
            if self.tx_last_notify_avail != self.txq.avail_idx {
                notify_queue(self.io_base, QUEUE_TX);
                self.tx_last_notify_avail = self.txq.avail_idx;
            }
        }
    }

    fn poll_rx_queue(&mut self) {
        let used_idx = self.rxq.used_idx();
        let mut processed = 0u16;
        let mut guard = 0u16;
        while self.rxq.last_used_idx != used_idx {
            // Safety guard: if indices ever get out of sync, don't spin for 65k iterations.
            // Cap to one ring worth of work per poll.
            if guard >= self.rxq.size {
                crate::log!(
                    "net/vio: rx guard tripped used_idx={} last_used_idx={} size={}\n",
                    used_idx,
                    self.rxq.last_used_idx,
                    self.rxq.size
                );
                break;
            }
            guard = guard.wrapping_add(1);

            let elem = self.rxq.used_elem(self.rxq.last_used_idx % self.rxq.size);
            let elem_len = elem.len;
            let desc_id = elem.id as usize;

            if desc_id >= self.rx_bufs.len() {
                // If this ever happens, we'd otherwise panic on indexing and appear to “halt”.
                crate::log!(
                    "net/vio: rx bad desc id={} (bufs={}) elem.len={} used_idx={} last_used_idx={}\n",
                    desc_id,
                    self.rx_bufs.len(),
                    elem_len,
                    used_idx,
                    self.rxq.last_used_idx
                );
                // Advance so we don't get stuck re-reading the same used element.
                self.rxq.last_used_idx = self.rxq.last_used_idx.wrapping_add(1);
                processed = processed.wrapping_add(1);
                continue;
            }

            if let Some(ring_ptr) = self.ring {
                // Safety: ring pointer is owned and kept alive by NetCore.
                let ring = unsafe { &mut *ring_ptr };
                let rx_ring = ring.rx_ring_mut();
                let slot = rx_ring.push_hw_owned().ok();
                if let Some(slot) = slot {
                    let dst = rx_ring.buffer_mut(slot);
                    let buf = &self.rx_bufs[desc_id];
                    let src = unsafe { core::slice::from_raw_parts(buf.virt(), RX_BUF_SIZE) };
                    let len = (elem_len as usize).saturating_sub(VIRTIO_NET_HDR_SIZE);
                    let copy_len = len.min(dst.len());
                    dst[..copy_len]
                        .copy_from_slice(&src[VIRTIO_NET_HDR_SIZE..VIRTIO_NET_HDR_SIZE + copy_len]);
                    rx_ring.mark_complete(slot, copy_len);
                }
            }

            self.rxq.push_avail(desc_id as u16);
            self.rxq.last_used_idx = self.rxq.last_used_idx.wrapping_add(1);
            processed = processed.wrapping_add(1);
        }

        if processed != 0 {
            notify_queue(self.io_base, QUEUE_RX);
        }

        // Ensure any coalesced TX submissions are kicked regularly.
        self.maybe_kick_tx(true);
    }

    fn reclaim_tx(&mut self) {
        let used_idx = self.txq.used_idx();
        while self.txq.last_used_idx != used_idx {
            let elem = self.txq.used_elem(self.txq.last_used_idx % self.txq.size);
            let desc_id = elem.id as usize;
            if desc_id < self.txq.size as usize {
                self.tx_free.push(desc_id as u16);
            } else {
                crate::log!(
                    "net/vio: tx bad desc id={} (size={}) used_idx={} last_used_idx={}\n",
                    desc_id,
                    self.txq.size,
                    used_idx,
                    self.txq.last_used_idx
                );
            }
            self.txq.last_used_idx = self.txq.last_used_idx.wrapping_add(1);
        }
    }

    fn tx_submit_hw(&mut self, frame: &[u8]) -> Result<(), ()> {
        let desc_id = match self.tx_free.pop() {
            Some(id) => id,
            None => return Err(()),
        };

        let buf = &self.tx_bufs[desc_id as usize];
        unsafe {
            // Zero the virtio-net header directly in the DMA buffer.
            core::ptr::write_bytes(buf.virt(), 0, VIRTIO_NET_HDR_SIZE);
            let dst = core::slice::from_raw_parts_mut(buf.virt(), TX_BUF_SIZE);
            let copy_len = frame.len().min(TX_BUF_SIZE - VIRTIO_NET_HDR_SIZE);
            dst[VIRTIO_NET_HDR_SIZE..VIRTIO_NET_HDR_SIZE + copy_len]
                .copy_from_slice(&frame[..copy_len]);
        }

        let desc = unsafe { &mut *self.txq.desc.add(desc_id as usize) };
        desc.addr = buf.phys();
        desc.len = (frame.len() + VIRTIO_NET_HDR_SIZE) as u32;
        desc.flags = 0;
        desc.next = 0;

        self.txq.push_avail(desc_id);

        // Coalesce notifies across bursts; poll_rx_queue will also force a kick.
        self.maybe_kick_tx(false);
        Ok(())
    }
}
