//! Minimal virtio-gpu 2D scanout driver for QEMU virtio-gpu over PCI.
//!
//! This supports the modern virtio-pci capability transport (common_cfg/notify_cfg) and will
//! fall back to the legacy I/O-port transport when available.

use crate::{pci, wait};
use core::sync::atomic::{fence, Ordering};

const VIRTIO_PCI_VENDOR: u16 = 0x1AF4;
// Virtio 1.0 GPU device id (0x1040 + virtio device id 16).
const VIRTIO_GPU_DEVICE_MODERN: u16 = 0x1050;
// Some QEMU configs may expose transitional/legacy ids; accept the common one.
const VIRTIO_GPU_DEVICE_LEGACY: u16 = 0x1010;

const VIRTIO_PCI_IOBAR_OFFSET: u16 = 0x10;
const VIRTIO_PCI_COMMAND_OFFSET: u16 = 0x04;
const VIRTIO_PCI_COMMAND_IO: u16 = 1 << 0;
const VIRTIO_PCI_COMMAND_MEM: u16 = 1 << 1;
const VIRTIO_PCI_COMMAND_BUS_MASTER: u16 = 1 << 2;

// PCI capabilities.
const PCI_CAP_PTR: u16 = 0x34;
const PCI_CAP_ID_VENDOR_SPECIFIC: u8 = 0x09;

// Legacy virtio PCI I/O port register layout (transitional devices only).
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
const VIRTIO_STATUS_FEATURES_OK: u8 = 0x08;
const VIRTIO_STATUS_DRIVER_OK: u8 = 0x04;
const VIRTIO_STATUS_FAILED: u8 = 0x80;

const VIRTIO_F_VERSION_1: u64 = 1u64 << 32;

// Virtio PCI capability types (virtio spec).
const VIRTIO_PCI_CAP_COMMON_CFG: u8 = 1;
const VIRTIO_PCI_CAP_NOTIFY_CFG: u8 = 2;
const VIRTIO_PCI_CAP_ISR_CFG: u8 = 3;
const VIRTIO_PCI_CAP_DEVICE_CFG: u8 = 4;

const QUEUE_CONTROL: u16 = 0;

const VIRTQ_DESC_F_NEXT: u16 = 1;
const VIRTQ_DESC_F_WRITE: u16 = 2;

const VIRTIO_GPU_CMD_GET_DISPLAY_INFO: u32 = 0x0100;
const VIRTIO_GPU_CMD_RESOURCE_CREATE_2D: u32 = 0x0101;
const VIRTIO_GPU_CMD_RESOURCE_UNREF: u32 = 0x0102;
const VIRTIO_GPU_CMD_SET_SCANOUT: u32 = 0x0103;
const VIRTIO_GPU_CMD_RESOURCE_FLUSH: u32 = 0x0104;
const VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D: u32 = 0x0105;
const VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING: u32 = 0x0106;

const VIRTIO_GPU_RESP_OK_NODATA: u32 = 0x1100;
const VIRTIO_GPU_RESP_OK_DISPLAY_INFO: u32 = 0x1101;

// Matches Linux virtio_gpu.h
const VIRTIO_GPU_FORMAT_B8G8R8X8_UNORM: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy)]
struct VirtqDesc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct VirtqUsedElem {
    id: u32,
    len: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct VirtioPciCap {
    cap_vndr: u8,
    cap_next: u8,
    cap_len: u8,
    cfg_type: u8,
    bar: u8,
    id: u8,
    padding: [u8; 2],
    offset: u32,
    length: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct VirtioPciNotifyCap {
    cap: VirtioPciCap,
    notify_off_multiplier: u32,
}

#[repr(C)]
struct VirtioPciCommonCfg {
    device_feature_select: u32,
    device_feature: u32,
    driver_feature_select: u32,
    driver_feature: u32,
    msix_config: u16,
    num_queues: u16,
    device_status: u8,
    config_generation: u8,
    queue_select: u16,
    queue_size: u16,
    queue_msix_vector: u16,
    queue_enable: u16,
    queue_notify_off: u16,
    queue_desc: u64,
    queue_avail: u64,
    queue_used: u64,
}

#[derive(Clone, Copy)]
struct VirtioModernCaps {
    common_phys: u64,
    common_len: u32,
    notify_phys: u64,
    notify_len: u32,
    notify_mult: u32,
    isr_phys: Option<u64>,
    isr_len: u32,
    device_phys: Option<u64>,
    device_len: u32,
}

enum Transport {
    LegacyIo { io_base: u16 },
    Modern {
        common: core::ptr::NonNull<VirtioPciCommonCfg>,
        notify: core::ptr::NonNull<u8>,
        notify_mult: u32,
        _isr: Option<core::ptr::NonNull<u8>>,
        _device_cfg: Option<core::ptr::NonNull<u8>>,
    },
}

struct DmaRegion {
    phys: u64,
    virt: *mut u8,
    len: usize,
}

unsafe impl Send for DmaRegion {}

impl DmaRegion {
    fn alloc(size: usize, align: usize) -> Option<Self> {
        let (phys, virt) = crate::pci::dma::alloc(size, align)?;
        Some(Self { phys, virt, len: size })
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
        crate::pci::dma::dealloc(self.virt, self.len);
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
    queue_index: u16,
    notify_off: u16,
}

unsafe impl Send for VirtQueue {}

impl VirtQueue {
    fn new(
        size: u16,
        mem: DmaRegion,
        desc: *mut VirtqDesc,
        avail: *mut u8,
        used: *mut u8,
        queue_index: u16,
        notify_off: u16,
    ) -> Self {
        Self {
            size,
            _mem: mem,
            desc,
            avail,
            used,
            avail_idx: 0,
            last_used_idx: 0,
            queue_index,
            notify_off,
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
    (value + align - 1) / align * align
}

fn read_io_base(dev: &pci::PciDevice) -> Result<u16, ()> {
    let bar0 = pci::config_read_u32(dev.bus, dev.slot, dev.function, VIRTIO_PCI_IOBAR_OFFSET);
    if (bar0 & 0x1) == 0 {
        return Err(());
    }
    Ok((bar0 & 0xFFFF_FFFC) as u16)
}

fn enable_io_and_bus_master(dev: &pci::PciDevice) {
    let mut cmd = pci::config_read_u16(dev.bus, dev.slot, dev.function, VIRTIO_PCI_COMMAND_OFFSET);
    cmd |= VIRTIO_PCI_COMMAND_IO | VIRTIO_PCI_COMMAND_BUS_MASTER;
    pci::config_write_u16(dev.bus, dev.slot, dev.function, VIRTIO_PCI_COMMAND_OFFSET, cmd);
}

fn enable_mem_and_bus_master(dev: &pci::PciDevice) {
    let mut cmd = pci::config_read_u16(dev.bus, dev.slot, dev.function, VIRTIO_PCI_COMMAND_OFFSET);
    cmd |= VIRTIO_PCI_COMMAND_MEM | VIRTIO_PCI_COMMAND_BUS_MASTER;
    pci::config_write_u16(dev.bus, dev.slot, dev.function, VIRTIO_PCI_COMMAND_OFFSET, cmd);
}

fn bar_mem_base(dev: &pci::PciDevice, bar_index: u8) -> Option<u64> {
    let (lo, hi) = pci::read_bar_raw(dev.bus, dev.slot, dev.function, bar_index);
    if (lo & 0x1) != 0 {
        // I/O BAR
        return None;
    }
    let is_64 = ((lo >> 1) & 0x3) == 0x2;
    let base_lo = (lo & 0xFFFF_FFF0) as u64;
    if is_64 {
        let base_hi = (hi? as u64) << 32;
        Some(base_hi | base_lo)
    } else {
        Some(base_lo)
    }
}

fn read_cap_header(dev: &pci::PciDevice, cap_ptr: u8) -> (u8, u8) {
    let id = pci::config_read_u8(dev.bus, dev.slot, dev.function, cap_ptr as u16);
    let next = pci::config_read_u8(dev.bus, dev.slot, dev.function, cap_ptr as u16 + 1);
    (id, next)
}

fn read_virtio_pci_cap(dev: &pci::PciDevice, cap_ptr: u8) -> Option<VirtioPciCap> {
    let cap_len = pci::config_read_u8(dev.bus, dev.slot, dev.function, cap_ptr as u16 + 2);
    if cap_len < core::mem::size_of::<VirtioPciCap>() as u8 {
        return None;
    }

    let mut out = VirtioPciCap::default();
    // Read the fixed 16-byte header/body.
    let base = cap_ptr as u16;
    out.cap_vndr = pci::config_read_u8(dev.bus, dev.slot, dev.function, base + 0);
    out.cap_next = pci::config_read_u8(dev.bus, dev.slot, dev.function, base + 1);
    out.cap_len = cap_len;
    out.cfg_type = pci::config_read_u8(dev.bus, dev.slot, dev.function, base + 3);
    out.bar = pci::config_read_u8(dev.bus, dev.slot, dev.function, base + 4);
    out.id = pci::config_read_u8(dev.bus, dev.slot, dev.function, base + 5);
    out.padding[0] = pci::config_read_u8(dev.bus, dev.slot, dev.function, base + 6);
    out.padding[1] = pci::config_read_u8(dev.bus, dev.slot, dev.function, base + 7);
    out.offset = pci::config_read_u32(dev.bus, dev.slot, dev.function, base + 8);
    out.length = pci::config_read_u32(dev.bus, dev.slot, dev.function, base + 12);
    Some(out)
}

fn read_virtio_notify_cap(dev: &pci::PciDevice, cap_ptr: u8) -> Option<VirtioPciNotifyCap> {
    let cap = read_virtio_pci_cap(dev, cap_ptr)?;
    if cap.cap_len < core::mem::size_of::<VirtioPciNotifyCap>() as u8 {
        return None;
    }
    let mult = pci::config_read_u32(dev.bus, dev.slot, dev.function, cap_ptr as u16 + 16);
    Some(VirtioPciNotifyCap {
        cap,
        notify_off_multiplier: mult,
    })
}

fn parse_modern_caps(dev: &pci::PciDevice) -> Option<VirtioModernCaps> {
    let mut ptr = pci::config_read_u8(dev.bus, dev.slot, dev.function, PCI_CAP_PTR);
    if ptr == 0 {
        return None;
    }

    let mut common: Option<VirtioPciCap> = None;
    let mut notify: Option<VirtioPciNotifyCap> = None;
    let mut isr: Option<VirtioPciCap> = None;
    let mut device_cfg: Option<VirtioPciCap> = None;

    for _ in 0..64 {
        if ptr < 0x40 {
            // Cap list lives in config space; 0x00..0x3F is header.
            break;
        }
        let (cap_id, next) = read_cap_header(dev, ptr);
        if cap_id == 0xFF || cap_id == 0 {
            break;
        }

        if cap_id == PCI_CAP_ID_VENDOR_SPECIFIC {
            if let Some(vcap) = read_virtio_pci_cap(dev, ptr) {
                match vcap.cfg_type {
                    VIRTIO_PCI_CAP_COMMON_CFG => common = Some(vcap),
                    VIRTIO_PCI_CAP_ISR_CFG => isr = Some(vcap),
                    VIRTIO_PCI_CAP_DEVICE_CFG => device_cfg = Some(vcap),
                    VIRTIO_PCI_CAP_NOTIFY_CFG => {
                        if let Some(ncap) = read_virtio_notify_cap(dev, ptr) {
                            notify = Some(ncap);
                        }
                    }
                    _ => {}
                }
            }
        }

        if next == 0 {
            break;
        }
        ptr = next;
    }

    let common = common?;
    let notify = notify?;

    let common_bar = bar_mem_base(dev, common.bar)?;
    let notify_bar = bar_mem_base(dev, notify.cap.bar)?;

    let common_phys = common_bar.checked_add(common.offset as u64)?;
    let notify_phys = notify_bar.checked_add(notify.cap.offset as u64)?;

    let isr_phys = isr.and_then(|c| {
        let bar = bar_mem_base(dev, c.bar)?;
        bar.checked_add(c.offset as u64)
    });

    let device_phys = device_cfg.and_then(|c| {
        let bar = bar_mem_base(dev, c.bar)?;
        bar.checked_add(c.offset as u64)
    });

    Some(VirtioModernCaps {
        common_phys,
        common_len: common.length,
        notify_phys,
        notify_len: notify.cap.length,
        notify_mult: notify.notify_off_multiplier,
        isr_phys,
        isr_len: isr.map(|c| c.length).unwrap_or(0),
        device_phys,
        device_len: device_cfg.map(|c| c.length).unwrap_or(0),
    })
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

fn setup_queue_legacy(io_base: u16, queue_index: u16) -> Result<VirtQueue, ()> {
    select_queue(io_base, queue_index);
    let size = read_queue_size(io_base);
    if size == 0 {
        return Err(());
    }

    let desc_size = size as usize * core::mem::size_of::<VirtqDesc>();
    let avail_size = 4 + (size as usize * 2);
    let used_offset = align_up(desc_size + avail_size, 4096);
    let used_size = 4 + (size as usize * 8);
    let total = align_up(used_offset + used_size, 4096);

    let mem = DmaRegion::alloc(total, 4096).ok_or(())?;
    unsafe { core::ptr::write_bytes(mem.virt(), 0, total) };

    let desc = mem.virt() as *mut VirtqDesc;
    let avail = unsafe { mem.virt().add(desc_size) };
    let used = unsafe { mem.virt().add(used_offset) };

    let pfn = (mem.phys() >> 12) as u32;
    write_queue_addr(io_base, pfn);

    Ok(VirtQueue::new(size, mem, desc, avail, used, queue_index, 0))
}

fn setup_queue_modern(common: core::ptr::NonNull<VirtioPciCommonCfg>, queue_index: u16) -> Result<VirtQueue, ()> {
    let common = common.as_ptr();
    unsafe {
        core::ptr::write_volatile(&mut (*common).queue_select, queue_index);
    }
    let size_max = unsafe { core::ptr::read_volatile(&(*common).queue_size) };
    if size_max == 0 {
        return Err(());
    }

    // We only ever submit a single 2-descriptor chain; keep the ring small.
    let size: u16 = size_max.min(8).max(2);

    let desc_size = size as usize * core::mem::size_of::<VirtqDesc>();
    let avail_size = 4 + (size as usize * 2);
    let used_offset = align_up(desc_size + avail_size, 4096);
    let used_size = 4 + (size as usize * 8);
    let total = align_up(used_offset + used_size, 4096);

    let mem = DmaRegion::alloc(total, 4096).ok_or(())?;
    unsafe { core::ptr::write_bytes(mem.virt(), 0, total) };

    let desc = mem.virt() as *mut VirtqDesc;
    let avail = unsafe { mem.virt().add(desc_size) };
    let used = unsafe { mem.virt().add(used_offset) };

    let desc_phys = mem.phys();
    let avail_phys = mem.phys().saturating_add(desc_size as u64);
    let used_phys = mem.phys().saturating_add(used_offset as u64);

    unsafe {
        // Program the queue.
        core::ptr::write_volatile(&mut (*common).queue_size, size);
        // Disable MSI-X for now.
        core::ptr::write_volatile(&mut (*common).queue_msix_vector, 0xFFFF);
        core::ptr::write_volatile(&mut (*common).queue_desc, desc_phys);
        core::ptr::write_volatile(&mut (*common).queue_avail, avail_phys);
        core::ptr::write_volatile(&mut (*common).queue_used, used_phys);
    }

    // Ensure MMIO writes and queue memory are visible before enabling.
    fence(Ordering::Release);

    unsafe {
        core::ptr::write_volatile(&mut (*common).queue_enable, 1);
        let en = core::ptr::read_volatile(&(*common).queue_enable);
        if en != 1 {
            crate::log!(
                "virtio-gpu: queue_enable did not stick (q={} en={})\n",
                queue_index,
                en
            );
            return Err(());
        }
    }

    let notify_off = unsafe { core::ptr::read_volatile(&(*common).queue_notify_off) };
    Ok(VirtQueue::new(size, mem, desc, avail, used, queue_index, notify_off))
}

fn notify_queue_modern(
    notify_base: core::ptr::NonNull<u8>,
    notify_mult: u32,
    queue_index: u16,
    notify_off: u16,
) {
    let off = (notify_off as u32).saturating_mul(notify_mult) as usize;
    unsafe {
        // Virtio-pci notify is typically a 16-bit write of the queue index.
        let ptr = notify_base.as_ptr().add(off) as *mut u16;
        core::ptr::write_volatile(ptr, queue_index);
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CtrlHdr {
    type_: u32,
    flags: u32,
    fence_id: u64,
    ctx_id: u32,
    padding: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CmdGetDisplayInfo {
    hdr: CtrlHdr,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct DisplayOne {
    r: Rect,
    enabled: u32,
    flags: u32,
}

const MAX_SCANOUTS: usize = 16;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct RespDisplayInfo {
    hdr: CtrlHdr,
    pmodes: [DisplayOne; MAX_SCANOUTS],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CmdResourceCreate2d {
    hdr: CtrlHdr,
    resource_id: u32,
    format: u32,
    width: u32,
    height: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct MemEntry {
    addr: u64,
    length: u32,
    padding: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CmdResourceAttachBacking {
    hdr: CtrlHdr,
    resource_id: u32,
    nr_entries: u32,
    entry: MemEntry,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CmdSetScanout {
    hdr: CtrlHdr,
    r: Rect,
    scanout_id: u32,
    resource_id: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CmdTransferToHost2d {
    hdr: CtrlHdr,
    r: Rect,
    offset: u64,
    resource_id: u32,
    padding: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CmdResourceFlush {
    hdr: CtrlHdr,
    r: Rect,
    resource_id: u32,
    padding: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CmdResourceUnref {
    hdr: CtrlHdr,
    resource_id: u32,
    padding: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct RespOkNoData {
    hdr: CtrlHdr,
}

pub struct VirtioGpu2d {
    transport: Transport,
    ctrlq: VirtQueue,

    resource_id: u32,
    scanout_id: u32,
    extent: (u32, u32),

    backing: DmaRegion,

    req: DmaRegion,
    resp: DmaRegion,
}

unsafe impl Send for VirtioGpu2d {}

impl VirtioGpu2d {
    pub fn init_first() -> Option<Self> {
        let dev = find_virtio_gpu_device()?;

        // Prefer modern virtio-pci capability transport.
        let (transport, ctrlq) = if let Some(caps) = parse_modern_caps(&dev) {
            enable_mem_and_bus_master(&dev);

            let common_map = pci::mmio::map_mmio_region_exact(
                caps.common_phys,
                (caps.common_len as usize).max(core::mem::size_of::<VirtioPciCommonCfg>()),
            )
            .ok()?;
            let notify_map = pci::mmio::map_mmio_region_exact(
                caps.notify_phys,
                (caps.notify_len as usize).max(4),
            )
            .ok()?;

            let common_ptr = core::ptr::NonNull::new(common_map.as_ptr() as *mut VirtioPciCommonCfg)?;
            let notify_ptr = core::ptr::NonNull::new(notify_map.as_ptr() as *mut u8)?;

            let isr_ptr = caps
                .isr_phys
                .and_then(|p| pci::mmio::map_mmio_region_exact(p, caps.isr_len as usize).ok())
                .and_then(|m| core::ptr::NonNull::new(m.as_ptr() as *mut u8));

            let dev_ptr = caps
                .device_phys
                .and_then(|p| pci::mmio::map_mmio_region_exact(p, caps.device_len as usize).ok())
                .and_then(|m| core::ptr::NonNull::new(m.as_ptr() as *mut u8));

            // Reset + status.
            unsafe {
                core::ptr::write_volatile(&mut (*common_ptr.as_ptr()).device_status, 0);
                core::ptr::write_volatile(
                    &mut (*common_ptr.as_ptr()).device_status,
                    VIRTIO_STATUS_ACK | VIRTIO_STATUS_DRIVER,
                );

                // Minimal feature negotiation: accept VERSION_1 when offered.
                core::ptr::write_volatile(&mut (*common_ptr.as_ptr()).device_feature_select, 0);
                let dev_lo = core::ptr::read_volatile(&(*common_ptr.as_ptr()).device_feature) as u64;
                core::ptr::write_volatile(&mut (*common_ptr.as_ptr()).device_feature_select, 1);
                let dev_hi = core::ptr::read_volatile(&(*common_ptr.as_ptr()).device_feature) as u64;
                let dev_features = dev_lo | (dev_hi << 32);
                let guest_features = if (dev_features & VIRTIO_F_VERSION_1) != 0 {
                    VIRTIO_F_VERSION_1
                } else {
                    0
                };

                core::ptr::write_volatile(&mut (*common_ptr.as_ptr()).driver_feature_select, 0);
                core::ptr::write_volatile(&mut (*common_ptr.as_ptr()).driver_feature, (guest_features & 0xFFFF_FFFF) as u32);
                core::ptr::write_volatile(&mut (*common_ptr.as_ptr()).driver_feature_select, 1);
                core::ptr::write_volatile(&mut (*common_ptr.as_ptr()).driver_feature, (guest_features >> 32) as u32);

                let mut status = core::ptr::read_volatile(&(*common_ptr.as_ptr()).device_status);
                status |= VIRTIO_STATUS_FEATURES_OK;
                core::ptr::write_volatile(&mut (*common_ptr.as_ptr()).device_status, status);

                // If the device cleared FEATURES_OK, negotiation failed.
                let readback = core::ptr::read_volatile(&(*common_ptr.as_ptr()).device_status);
                if (readback & VIRTIO_STATUS_FEATURES_OK) == 0 {
                    core::ptr::write_volatile(&mut (*common_ptr.as_ptr()).device_status, VIRTIO_STATUS_FAILED);
                    return None;
                }
            }

            crate::log!(
                "virtio-gpu: {:02x}:{:02x}.{} using modern virtio-pci caps (common_cfg+notify_cfg)\n",
                dev.bus,
                dev.slot,
                dev.function
            );

            let ctrlq = match setup_queue_modern(common_ptr, QUEUE_CONTROL) {
                Ok(q) => q,
                Err(()) => {
                    unsafe { core::ptr::write_volatile(&mut (*common_ptr.as_ptr()).device_status, VIRTIO_STATUS_FAILED) };
                    return None;
                }
            };

            unsafe {
                let mut status = core::ptr::read_volatile(&(*common_ptr.as_ptr()).device_status);
                status |= VIRTIO_STATUS_DRIVER_OK;
                core::ptr::write_volatile(&mut (*common_ptr.as_ptr()).device_status, status);
            }

            let transport = Transport::Modern {
                common: common_ptr,
                notify: notify_ptr,
                notify_mult: caps.notify_mult,
                _isr: isr_ptr,
                _device_cfg: dev_ptr,
            };

            (transport, ctrlq)
        } else {
            // Legacy I/O-port transport (transitional devices).
            let io_base = match read_io_base(&dev) {
                Ok(v) => v,
                Err(()) => {
                    crate::log!(
                        "virtio-gpu: {:02x}:{:02x}.{} has no virtio-pci caps and no legacy I/O BAR\n",
                        dev.bus,
                        dev.slot,
                        dev.function
                    );
                    return None;
                }
            };

            enable_io_and_bus_master(&dev);

            reset_device(io_base);
            set_status(io_base, VIRTIO_STATUS_ACK | VIRTIO_STATUS_DRIVER);

            unsafe { crate::portio::outl(io_base + VIRTIO_PCI_REG_GUEST_PAGE_SIZE, 4096) };

            let _features = read_device_features(io_base);
            write_guest_features(io_base, 0);

            let ctrlq = match setup_queue_legacy(io_base, QUEUE_CONTROL) {
                Ok(q) => q,
                Err(()) => {
                    set_status(io_base, VIRTIO_STATUS_FAILED);
                    return None;
                }
            };

            set_status(io_base, VIRTIO_STATUS_ACK | VIRTIO_STATUS_DRIVER | VIRTIO_STATUS_DRIVER_OK);
            (Transport::LegacyIo { io_base }, ctrlq)
        };

        let req = DmaRegion::alloc(4096, 16)?;
        let resp = DmaRegion::alloc(4096, 16)?;

        let mut gpu = Self {
            transport,
            ctrlq,
            resource_id: 1,
            scanout_id: 0,
            extent: (0, 0),
            backing: DmaRegion::alloc(1, 1)?,
            req,
            resp,
        };

        if !gpu.bootstrap_scanout() {
            return None;
        }

        Some(gpu)
    }

    pub fn extent(&self) -> (u32, u32) {
        self.extent
    }

    pub fn backing_ptr_u32(&mut self) -> *mut u32 {
        self.backing.virt() as *mut u32
    }

    pub fn backing_len_u32(&self) -> usize {
        self.backing.len() / 4
    }

    pub fn transfer_and_flush(&mut self, rect: Rect) -> bool {
        let req = CmdTransferToHost2d {
            hdr: CtrlHdr {
                type_: VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D,
                ..Default::default()
            },
            r: rect,
            offset: 0,
            resource_id: self.resource_id,
            padding: 0,
        };
        if !self.ctrl_submit(&req, core::mem::size_of::<CmdTransferToHost2d>()) {
            return false;
        }

        let req = CmdResourceFlush {
            hdr: CtrlHdr {
                type_: VIRTIO_GPU_CMD_RESOURCE_FLUSH,
                ..Default::default()
            },
            r: rect,
            resource_id: self.resource_id,
            padding: 0,
        };
        self.ctrl_submit(&req, core::mem::size_of::<CmdResourceFlush>())
    }

    fn bootstrap_scanout(&mut self) -> bool {
        let req = CmdGetDisplayInfo {
            hdr: CtrlHdr {
                type_: VIRTIO_GPU_CMD_GET_DISPLAY_INFO,
                ..Default::default()
            },
        };
        if !self.ctrl_submit(&req, core::mem::size_of::<CmdGetDisplayInfo>()) {
            return false;
        }

        let info = unsafe { &*(self.resp.virt() as *const RespDisplayInfo) };
        if info.hdr.type_ != VIRTIO_GPU_RESP_OK_DISPLAY_INFO {
            crate::log!("virtio-gpu: display info bad resp=0x{:X}\n", info.hdr.type_);
            return false;
        }

        let mut chosen = None;
        for (i, m) in info.pmodes.iter().enumerate() {
            if m.enabled != 0 && m.r.width != 0 && m.r.height != 0 {
                chosen = Some((i as u32, m.r));
                break;
            }
        }
        let (scanout_id, r) = chosen.unwrap_or((0, Rect { x: 0, y: 0, width: 1024, height: 768 }));
        self.scanout_id = scanout_id;
        self.extent = (r.width, r.height);

        let create = CmdResourceCreate2d {
            hdr: CtrlHdr {
                type_: VIRTIO_GPU_CMD_RESOURCE_CREATE_2D,
                ..Default::default()
            },
            resource_id: self.resource_id,
            format: VIRTIO_GPU_FORMAT_B8G8R8X8_UNORM,
            width: r.width,
            height: r.height,
        };
        if !self.ctrl_submit(&create, core::mem::size_of::<CmdResourceCreate2d>()) {
            return false;
        }

        let backing_bytes = (r.width as usize)
            .saturating_mul(r.height as usize)
            .saturating_mul(4);
        self.backing = match DmaRegion::alloc(backing_bytes, 4096) {
            Some(m) => m,
            None => {
                crate::log!("virtio-gpu: backing alloc failed bytes={}\n", backing_bytes);
                return false;
            }
        };
        unsafe { core::ptr::write_bytes(self.backing.virt(), 0, backing_bytes) };

        let attach = CmdResourceAttachBacking {
            hdr: CtrlHdr {
                type_: VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING,
                ..Default::default()
            },
            resource_id: self.resource_id,
            nr_entries: 1,
            entry: MemEntry {
                addr: self.backing.phys(),
                length: backing_bytes as u32,
                padding: 0,
            },
        };
        if !self.ctrl_submit(&attach, core::mem::size_of::<CmdResourceAttachBacking>()) {
            return false;
        }

        let set = CmdSetScanout {
            hdr: CtrlHdr {
                type_: VIRTIO_GPU_CMD_SET_SCANOUT,
                ..Default::default()
            },
            r,
            scanout_id: self.scanout_id,
            resource_id: self.resource_id,
        };
        if !self.ctrl_submit(&set, core::mem::size_of::<CmdSetScanout>()) {
            return false;
        }

        true
    }

    fn ctrl_submit<T: Copy>(&mut self, req: &T, req_len: usize) -> bool {
        if req_len == 0 || req_len > self.req.len() {
            return false;
        }
        unsafe {
            core::ptr::copy_nonoverlapping(req as *const T as *const u8, self.req.virt(), req_len);
            core::ptr::write_bytes(self.resp.virt(), 0, self.resp.len());
        }

        // Fixed descriptor pair (0 -> req, 1 -> resp). Single outstanding at a time.
        unsafe {
            let d0 = &mut *self.ctrlq.desc.add(0);
            d0.addr = self.req.phys();
            d0.len = req_len as u32;
            d0.flags = VIRTQ_DESC_F_NEXT;
            d0.next = 1;

            let d1 = &mut *self.ctrlq.desc.add(1);
            d1.addr = self.resp.phys();
            d1.len = self.resp.len() as u32;
            d1.flags = VIRTQ_DESC_F_WRITE;
            d1.next = 0;
        }

        self.ctrlq.push_avail(0);
        // Ensure descriptor/avail writes are visible to the device before we ring the doorbell.
        fence(Ordering::Release);
        match &self.transport {
            Transport::LegacyIo { io_base } => {
                notify_queue(*io_base, self.ctrlq.queue_index);
            }
            Transport::Modern {
                notify,
                notify_mult,
                ..
            } => {
                notify_queue_modern(
                    *notify,
                    *notify_mult,
                    self.ctrlq.queue_index,
                    self.ctrlq.notify_off,
                );
            }
        }

        let ok = wait::spin_until_timeout(1000, || self.ctrlq.used_idx() != self.ctrlq.last_used_idx);
        if !ok {
            crate::log!("virtio-gpu: ctrlq timeout\n");
            return false;
        }

        let used = self.ctrlq.used_elem(self.ctrlq.last_used_idx % self.ctrlq.size);
        self.ctrlq.last_used_idx = self.ctrlq.last_used_idx.wrapping_add(1);
        if used.id != 0 {
            crate::log!("virtio-gpu: ctrlq used id={} (expected 0)\n", used.id);
            return false;
        }

        let hdr = unsafe { &*(self.resp.virt() as *const CtrlHdr) };
        if hdr.type_ != VIRTIO_GPU_RESP_OK_NODATA && hdr.type_ != VIRTIO_GPU_RESP_OK_DISPLAY_INFO {
            crate::log!("virtio-gpu: ctrl resp=0x{:X}\n", hdr.type_);
            return false;
        }

        true
    }
}

impl Drop for VirtioGpu2d {
    fn drop(&mut self) {
        let req = CmdResourceUnref {
            hdr: CtrlHdr {
                type_: VIRTIO_GPU_CMD_RESOURCE_UNREF,
                ..Default::default()
            },
            resource_id: self.resource_id,
            padding: 0,
        };
        let _ = self.ctrl_submit(&req, core::mem::size_of::<CmdResourceUnref>());
    }
}

fn find_virtio_gpu_device() -> Option<pci::PciDevice> {
    let mut found: Option<pci::PciDevice> = None;
    pci::with_devices(|list| {
        for dev in list {
            if dev.vendor != VIRTIO_PCI_VENDOR {
                continue;
            }
            if dev.device != VIRTIO_GPU_DEVICE_MODERN && dev.device != VIRTIO_GPU_DEVICE_LEGACY {
                continue;
            }
            found = Some(*dev);
            break;
        }
    });
    found
}
