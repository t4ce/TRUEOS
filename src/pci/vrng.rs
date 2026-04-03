//! Minimal virtio-rng (entropy) driver for QEMU/virtio over PCI (legacy I/O port transport).
//!
//! This is intended as a *fallback entropy source* for seeding the kernel CSPRNG
//! when RDSEED/RDRAND are unavailable.

use crate::wait;
use spin::{Mutex, Once};

const VIRTIO_PCI_VENDOR: u16 = 0x1AF4;
// Transitional (legacy) virtio-rng PCI device id.
const VIRTIO_RNG_DEVICE_LEGACY: u16 = 0x1005;
// Modern virtio-rng PCI device id (0x1040 + virtio device id 4).
const VIRTIO_RNG_DEVICE_MODERN: u16 = 0x1044;

const VIRTIO_PCI_IOBAR_OFFSET: u16 = 0x10;
const VIRTIO_PCI_COMMAND_OFFSET: u16 = 0x04;
const VIRTIO_PCI_COMMAND_IO: u16 = 1 << 0;
const VIRTIO_PCI_COMMAND_BUS_MASTER: u16 = 1 << 2;

// Legacy virtio PCI I/O port register layout.
const VIRTIO_PCI_REG_DEVICE_FEATURES: u16 = 0x00;
const VIRTIO_PCI_REG_GUEST_FEATURES: u16 = 0x04;
const VIRTIO_PCI_REG_QUEUE_ADDRESS: u16 = 0x08;
const VIRTIO_PCI_REG_QUEUE_SIZE: u16 = 0x0C;
const VIRTIO_PCI_REG_QUEUE_SELECT: u16 = 0x0E;
const VIRTIO_PCI_REG_QUEUE_NOTIFY: u16 = 0x10;
const VIRTIO_PCI_REG_DEVICE_STATUS: u16 = 0x12;
const VIRTIO_PCI_REG_GUEST_PAGE_SIZE: u16 = 0x28;

const VIRTIO_STATUS_ACK: u8 = 0x01;
const VIRTIO_STATUS_DRIVER: u8 = 0x02;
const VIRTIO_STATUS_DRIVER_OK: u8 = 0x04;
const VIRTIO_STATUS_FAILED: u8 = 0x80;

const QUEUE_RNG: u16 = 0;
const VIRTQ_DESC_F_WRITE: u16 = 2;

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

struct DmaRegion {
    phys: u64,
    virt: *mut u8,
    len: usize,
}

// Safety: physical memory backing these pointers is stable for the region lifetime.
unsafe impl Send for DmaRegion {}

impl DmaRegion {
    fn alloc(size: usize, align: usize) -> Option<Self> {
        let (phys, virt) = crate::dma::alloc(size, align)?;
        Some(Self {
            phys,
            virt,
            len: size,
        })
    }

    fn phys(&self) -> u64 {
        self.phys
    }

    fn virt(&self) -> *mut u8 {
        self.virt
    }

    fn len(&self) -> usize {
        self.len
    }
}

impl Drop for DmaRegion {
    fn drop(&mut self) {
        if self.len == 0 || self.virt.is_null() {
            return;
        }
        crate::dma::dealloc(self.virt, self.len);
        self.len = 0;
        self.virt = core::ptr::null_mut();
        self.phys = 0;
    }
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

// Safety: guarded by the global mutex.
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

fn align_up(value: usize, align: usize) -> usize {
    if align == 0 {
        return value;
    }
    value.div_ceil(align) * align
}

fn read_io_base(dev: &crate::pci::PciDevice) -> Result<u16, ()> {
    let bar0 =
        crate::pci::config_read_u32(dev.bus, dev.slot, dev.function, VIRTIO_PCI_IOBAR_OFFSET);
    if (bar0 & 0x1) == 0 {
        return Err(());
    }
    Ok((bar0 & 0xFFFF_FFFC) as u16)
}

fn enable_io_and_bus_master(dev: &crate::pci::PciDevice) {
    let mut cmd =
        crate::pci::config_read_u16(dev.bus, dev.slot, dev.function, VIRTIO_PCI_COMMAND_OFFSET);
    cmd |= VIRTIO_PCI_COMMAND_IO | VIRTIO_PCI_COMMAND_BUS_MASTER;
    crate::pci::config_write_u16(dev.bus, dev.slot, dev.function, VIRTIO_PCI_COMMAND_OFFSET, cmd);
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
    // Legacy virtio-pci expects used ring page alignment.
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

#[derive(Debug)]
pub enum Error {
    NotFound,
    Unsupported,
    InitFailed,
    Timeout,
}

pub struct VirtioRng {
    io_base: u16,
    q: VirtQueue,
    buf: DmaRegion,
}

impl VirtioRng {
    pub fn init() -> Result<Self, Error> {
        let dev = find_virtio_rng_device().ok_or(Error::NotFound)?;
        let io_base = read_io_base(&dev).map_err(|_| Error::Unsupported)?;
        enable_io_and_bus_master(&dev);

        crate::log!(
            "pci/vrng: found virtio-rng {:02x}:{:02x}.{} vid={:04x} did={:04x} io_base=0x{:04x}\n",
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
        unsafe { crate::portio::outl(io_base + VIRTIO_PCI_REG_GUEST_PAGE_SIZE, 4096) };

        // No device-specific features for virtio-rng.
        let _features = read_device_features(io_base);
        write_guest_features(io_base, 0);

        let mut q = setup_queue(io_base, QUEUE_RNG).map_err(|_| Error::InitFailed)?;
        let buf = DmaRegion::alloc(4096, 16).ok_or(Error::InitFailed)?;

        // Device is fully configured at this point.
        // Per virtio init sequence, set DRIVER_OK before submitting buffers.
        set_status(io_base, VIRTIO_STATUS_ACK | VIRTIO_STATUS_DRIVER | VIRTIO_STATUS_DRIVER_OK);

        // Program a single writable descriptor (id=0) pointing at our entropy buffer.
        unsafe {
            let desc0 = &mut *q.desc.add(0);
            desc0.addr = buf.phys();
            desc0.len = buf.len() as u32;
            desc0.flags = VIRTQ_DESC_F_WRITE;
            desc0.next = 0;
        }
        q.push_avail(0);
        notify_queue(io_base, QUEUE_RNG);

        Ok(Self { io_base, q, buf })
    }

    fn poll_one_completion(&mut self, spins: usize) -> Result<usize, Error> {
        for _ in 0..spins {
            let used_idx = self.q.used_idx();
            if self.q.last_used_idx != used_idx {
                let elem = self.q.used_elem(self.q.last_used_idx % self.q.size);
                self.q.last_used_idx = self.q.last_used_idx.wrapping_add(1);

                if elem.id != 0 {
                    // Unexpected in our single-descriptor setup; mark failure.
                    set_status(self.io_base, VIRTIO_STATUS_FAILED);
                    return Err(Error::InitFailed);
                }

                let wrote = (elem.len as usize).min(self.buf.len());
                if wrote == 0 {
                    // Spec allows 1+ bytes; treat 0 as transient.
                    break;
                }
                return Ok(wrote);
            }

            wait::spin_step();
        }
        Err(Error::Timeout)
    }

    fn resubmit(&mut self) {
        // Re-submit the same buffer.
        unsafe {
            let desc0 = &mut *self.q.desc.add(0);
            desc0.addr = self.buf.phys();
            desc0.len = self.buf.len() as u32;
            desc0.flags = VIRTQ_DESC_F_WRITE;
            desc0.next = 0;
        }
        self.q.push_avail(0);
        notify_queue(self.io_base, QUEUE_RNG);
    }

    pub fn fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), Error> {
        let mut filled = 0usize;
        while filled < dest.len() {
            let wrote = self.poll_one_completion(50_000)?;
            unsafe {
                let src = core::slice::from_raw_parts(self.buf.virt(), wrote);
                let take = wrote.min(dest.len() - filled);
                dest[filled..filled + take].copy_from_slice(&src[..take]);
                filled += take;
            }

            // Always keep a buffer in-flight.
            // Even if the caller requested fewer bytes than the device wrote,
            // we still resubmit so the next consumer doesn't stall.
            self.resubmit();
        }
        Ok(())
    }
}

static VRNG: Mutex<Option<VirtioRng>> = Mutex::new(None);
static VRNG_SMOKE_ONCE: Once<()> = Once::new();

fn fnv1a64(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in data {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Initialize/attach virtio-rng if present.
///
/// This "claims" the device (enables I/O + bus-mastering and completes virtio init)
/// so it's visible in logs even if we never need to fall back to it.
pub fn init_once() {
    let mut guard = VRNG.lock();
    if guard.is_some() {
        return;
    }

    match VirtioRng::init() {
        Ok(dev) => {
            *guard = Some(dev);
            if crate::logflag::BOOT_INFO_LOGS {
                crate::log!("pci/vrng: attached\n");
            }
        }
        Err(Error::NotFound) => {
            if crate::logflag::BOOT_INFO_LOGS {
                crate::log!("pci/vrng: not present\n");
            }
        }
        Err(e) => {
            crate::log!("pci/vrng: init failed: {:?}\n", e);
        }
    }
}

/// One-time functional test: attempts to read a small buffer from the device.
///
/// Logs only a hash of the bytes (not the bytes themselves) to avoid dumping entropy
/// into the console.
pub fn smoke_test_once() {
    VRNG_SMOKE_ONCE.call_once(|| {
        let mut buf = [0u8; 32];
        match try_fill_bytes(&mut buf) {
            Ok(()) => {
                let h = fnv1a64(&buf);
                crate::log!("pci/vrng: smoke ok (fnv1a64=0x{:016x})\n", h);
            }
            Err(Error::NotFound) => {
                if crate::logflag::BOOT_INFO_LOGS {
                    crate::log!("pci/vrng: smoke skipped (not present)\n");
                }
            }
            Err(e) => {
                crate::log!("pci/vrng: smoke failed: {:?}\n", e);
            }
        }
    });
}

fn find_virtio_rng_device() -> Option<crate::pci::PciDevice> {
    let mut found = None;
    crate::pci::with_devices(|list| {
        for dev in list {
            if dev.vendor == VIRTIO_PCI_VENDOR
                && (dev.device == VIRTIO_RNG_DEVICE_LEGACY
                    || dev.device == VIRTIO_RNG_DEVICE_MODERN)
            {
                found = Some(*dev);
                break;
            }
        }
    });
    found
}

/// Best-effort virtio-rng entropy read.
///
/// Returns `Ok(())` only if the full `dest` is filled.
pub fn try_fill_bytes(dest: &mut [u8]) -> Result<(), Error> {
    let mut guard = VRNG.lock();
    if guard.is_none() {
        *guard = Some(VirtioRng::init()?);
    }
    let Some(dev) = guard.as_mut() else {
        return Err(Error::InitFailed);
    };
    dev.fill_bytes(dest)
}
