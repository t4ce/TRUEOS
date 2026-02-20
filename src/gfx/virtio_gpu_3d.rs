extern crate alloc;

use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering, fence};
use libm::{floorf, roundf};

use crate::{pci, wait};
use embassy_time_driver::{TICK_HZ, now};
// a Rectangle is just two triangles
const VIRTIO_PCI_VENDOR: u16 = 0x1AF4;
// Virtio 1.0 GPU device id (0x1040 + virtio device id 16).
const VIRTIO_GPU_DEVICE_MODERN: u16 = 0x1050;
// Transitional/legacy id sometimes used by QEMU configs.
const VIRTIO_GPU_DEVICE_TRANSITIONAL: u16 = 0x1010;

// PCI config offsets.
const PCI_CAP_PTR: u16 = 0x34;
const PCI_CAP_ID_VENDOR_SPECIFIC: u8 = 0x09;

// Command register bits.
const VIRTIO_PCI_COMMAND_OFFSET: u16 = 0x04;
const VIRTIO_PCI_COMMAND_MEM: u16 = 1 << 1;
const VIRTIO_PCI_COMMAND_BUS_MASTER: u16 = 1 << 2;

// Virtio status bits.
const VIRTIO_STATUS_ACK: u8 = 0x01;
const VIRTIO_STATUS_DRIVER: u8 = 0x02;
const VIRTIO_STATUS_DRIVER_OK: u8 = 0x04;
const VIRTIO_STATUS_FEATURES_OK: u8 = 0x08;
const VIRTIO_STATUS_FAILED: u8 = 0x80;

// Feature bits.
const VIRTIO_F_VERSION_1: u64 = 1u64 << 32;

// Virtio-gpu device feature bits (subset).
const VIRTIO_GPU_F_VIRGL: u64 = 1u64 << 0;

// Virtio PCI capability types.
const VIRTIO_PCI_CAP_COMMON_CFG: u8 = 1;
const VIRTIO_PCI_CAP_NOTIFY_CFG: u8 = 2;
const VIRTIO_PCI_CAP_ISR_CFG: u8 = 3;
const VIRTIO_PCI_CAP_DEVICE_CFG: u8 = 4;

// Virtio-gpu command/response ids (subset).
const VIRTIO_GPU_CMD_GET_DISPLAY_INFO: u32 = 0x0100;
const VIRTIO_GPU_CMD_RESOURCE_CREATE_2D: u32 = 0x0101;
const VIRTIO_GPU_CMD_SET_SCANOUT: u32 = 0x0103;
const VIRTIO_GPU_CMD_RESOURCE_FLUSH: u32 = 0x0104;
const VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D: u32 = 0x0105;
const VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING: u32 = 0x0106;
const VIRTIO_GPU_CMD_CTX_CREATE: u32 = 0x0200;
const VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE: u32 = 0x0202;
const VIRTIO_GPU_CMD_CTX_DETACH_RESOURCE: u32 = 0x0203;
const VIRTIO_GPU_CMD_RESOURCE_CREATE_3D: u32 = 0x0204;
const VIRTIO_GPU_CMD_SUBMIT_3D: u32 = 0x0207;

const VIRTIO_GPU_RESP_OK_NODATA: u32 = 0x1100;
const VIRTIO_GPU_RESP_OK_DISPLAY_INFO: u32 = 0x1101;
// --- Minimal virgl/Gallium constants we need for a triangle ---
// From virglrenderer 1.0.0 headers (Gallium subset):
const PIPE_SHADER_VERTEX: u32 = 0;
const PIPE_SHADER_FRAGMENT: u32 = 1;
const PIPE_PRIM_TRIANGLES: u32 = 4;

const PIPE_CLEAR_COLOR0: u32 = 1 << 2;
const PIPE_MASK_RGBA: u32 = 0xF;

const PIPE_BUFFER: u32 = 0;
const PIPE_TEXTURE_2D: u32 = 2;

const PIPE_BIND_RENDER_TARGET: u32 = 1 << 1;
const PIPE_BIND_BLENDABLE: u32 = 1 << 2;
const PIPE_BIND_VERTEX_BUFFER: u32 = 1 << 4;
const PIPE_BIND_DISPLAY_TARGET: u32 = 1 << 8;
const PIPE_BIND_SCANOUT: u32 = 1 << 14;

// Virgl format IDs (see virgl_hw.h):
const VIRGL_FORMAT_B8G8R8X8_UNORM: u32 = 2;
const VIRGL_FORMAT_R8_UNORM: u32 = 64;
const VIRGL_FORMAT_R32G32B32A32_FLOAT: u32 = 31;

// --- Virgl protocol (see virgl_protocol.h) ---
const VIRGL_OBJECT_BLEND: u8 = 1;
const VIRGL_OBJECT_RASTERIZER: u8 = 2;
const VIRGL_OBJECT_DSA: u8 = 3;
const VIRGL_OBJECT_SHADER: u8 = 4;
const VIRGL_OBJECT_VERTEX_ELEMENTS: u8 = 5;
const VIRGL_OBJECT_SURFACE: u8 = 8;

const VIRGL_CCMD_CREATE_OBJECT: u8 = 1;
const VIRGL_CCMD_BIND_OBJECT: u8 = 2;
const VIRGL_CCMD_SET_VIEWPORT_STATE: u8 = 4;
const VIRGL_CCMD_SET_FRAMEBUFFER_STATE: u8 = 5;
const VIRGL_CCMD_SET_VERTEX_BUFFERS: u8 = 6;
const VIRGL_CCMD_CLEAR: u8 = 7;
const VIRGL_CCMD_DRAW_VBO: u8 = 8;
const VIRGL_CCMD_RESOURCE_INLINE_WRITE: u8 = 9;
const VIRGL_CCMD_RESOURCE_COPY_REGION: u8 = 17;
// NOTE: Values must match virglrenderer `enum virgl_context_cmd`.
const VIRGL_CCMD_BIND_SHADER: u8 = 31;
const VIRGL_CCMD_LINK_SHADER: u8 = 52;

const VIRGL_LINK_SHADER_SIZE: u32 = 6;

const VIRGL_OBJ_BLEND_SIZE: u32 = 11;
const VIRGL_OBJ_DSA_SIZE: u32 = 5;
const VIRGL_OBJ_RS_SIZE: u32 = 9;
const VIRGL_OBJ_SURFACE_SIZE: u32 = 5;

fn virgl_cmd0(cmd: u8, obj: u8, len_dwords: u32) -> u32 {
    (cmd as u32) | ((obj as u32) << 8) | (len_dwords << 16)
}

fn fui(v: f32) -> u32 {
    u32::from_le_bytes(v.to_le_bytes())
}

struct VirglCmdBuf {
    dwords: Vec<u32>,
}

impl VirglCmdBuf {
    fn new() -> Self {
        Self { dwords: Vec::new() }
    }

    fn push(&mut self, v: u32) {
        self.dwords.push(v);
    }

    fn push_bytes_padded(&mut self, bytes: &[u8]) {
        let mut i = 0;
        while i < bytes.len() {
            let mut chunk = [0u8; 4];
            let take = (bytes.len() - i).min(4);
            chunk[..take].copy_from_slice(&bytes[i..i + take]);
            self.dwords.push(u32::from_le_bytes(chunk));
            i += take;
        }
    }

    fn as_bytes(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(self.dwords.as_ptr() as *const u8, self.dwords.len() * 4)
        }
    }
}

// Virtio queues.
const QUEUE_CONTROL: u16 = 0;

const VIRTQ_DESC_F_NEXT: u16 = 1;
const VIRTQ_DESC_F_WRITE: u16 = 2;

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

struct DmaRegion {
    phys: u64,
    virt: *mut u8,
    len: usize,
}

unsafe impl Send for DmaRegion {}

impl DmaRegion {
    fn alloc(size: usize, align: usize) -> Option<Self> {
        let (phys, virt) = crate::pci::dma::alloc(size, align)?;
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
        crate::pci::dma::dealloc(self.virt, self.len);
        self.len = 0;
        self.virt = core::ptr::null_mut();
        self.phys = 0;
    }
}

enum ScanoutBacking {
    Owned(DmaRegion),
    Borrowed {
        phys: u64,
        virt: *mut u8,
        len: usize,
    },
}

unsafe impl Send for ScanoutBacking {}

impl ScanoutBacking {
    fn phys(&self) -> u64 {
        match self {
            ScanoutBacking::Owned(r) => r.phys(),
            ScanoutBacking::Borrowed { phys, .. } => *phys,
        }
    }

    fn virt(&self) -> *mut u8 {
        match self {
            ScanoutBacking::Owned(r) => r.virt(),
            ScanoutBacking::Borrowed { virt, .. } => *virt,
        }
    }

    fn len(&self) -> usize {
        match self {
            ScanoutBacking::Owned(r) => r.len(),
            ScanoutBacking::Borrowed { len, .. } => *len,
        }
    }
}

struct BorrowedScanout {
    backing: ScanoutBacking,
    res_width: u32,
    res_height: u32,
    res_bytes: usize,

    fb_width: u32,
    fb_height: u32,
    fb_pitch: usize,
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
    if align <= 1 {
        return value;
    }
    let rem = value % align;
    if rem == 0 {
        value
    } else {
        value + (align - rem)
    }
}

fn bar_mem_base(dev: &pci::PciDevice, bar_index: u8) -> Option<u64> {
    let (lo, hi) = pci::read_bar_raw(dev.bus, dev.slot, dev.function, bar_index);
    if (lo & 0x1) != 0 {
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

    let base = cap_ptr as u16;
    Some(VirtioPciCap {
        cap_vndr: pci::config_read_u8(dev.bus, dev.slot, dev.function, base),
        cap_next: pci::config_read_u8(dev.bus, dev.slot, dev.function, base + 1),
        cap_len,
        cfg_type: pci::config_read_u8(dev.bus, dev.slot, dev.function, base + 3),
        bar: pci::config_read_u8(dev.bus, dev.slot, dev.function, base + 4),
        id: pci::config_read_u8(dev.bus, dev.slot, dev.function, base + 5),
        padding: [
            pci::config_read_u8(dev.bus, dev.slot, dev.function, base + 6),
            pci::config_read_u8(dev.bus, dev.slot, dev.function, base + 7),
        ],
        offset: pci::config_read_u32(dev.bus, dev.slot, dev.function, base + 8),
        length: pci::config_read_u32(dev.bus, dev.slot, dev.function, base + 12),
    })
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

#[derive(Clone, Copy)]
struct VirtioModernCaps {
    common_phys: u64,
    common_len: u32,
    notify_phys: u64,
    notify_len: u32,
    notify_mult: u32,
    _isr_phys: Option<u64>,
    _isr_len: u32,
    _device_phys: Option<u64>,
    _device_len: u32,
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
            break;
        }
        let (cap_id, next) = read_cap_header(dev, ptr);
        if cap_id == 0xFF || cap_id == 0 {
            break;
        }
        if cap_id == PCI_CAP_ID_VENDOR_SPECIFIC
            && let Some(vcap) = read_virtio_pci_cap(dev, ptr) {
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
        _isr_phys: isr_phys,
        _isr_len: isr.map(|c| c.length).unwrap_or(0),
        _device_phys: device_phys,
        _device_len: device_cfg.map(|c| c.length).unwrap_or(0),
    })
}

fn enable_mem_and_bus_master(dev: &pci::PciDevice) {
    let mut cmd = pci::config_read_u16(dev.bus, dev.slot, dev.function, VIRTIO_PCI_COMMAND_OFFSET);
    cmd |= VIRTIO_PCI_COMMAND_MEM | VIRTIO_PCI_COMMAND_BUS_MASTER;
    pci::config_write_u16(
        dev.bus,
        dev.slot,
        dev.function,
        VIRTIO_PCI_COMMAND_OFFSET,
        cmd,
    );
}

fn setup_queue_modern(
    common: core::ptr::NonNull<VirtioPciCommonCfg>,
    queue_index: u16,
) -> Result<VirtQueue, ()> {
    let common = common.as_ptr();
    unsafe {
        core::ptr::write_volatile(&mut (*common).queue_select, queue_index);
    }
    let size_max = unsafe { core::ptr::read_volatile(&(*common).queue_size) };
    if size_max == 0 {
        return Err(());
    }

    // Keep small: we only do a single in-flight control command.
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
        core::ptr::write_volatile(&mut (*common).queue_size, size);
        core::ptr::write_volatile(&mut (*common).queue_msix_vector, 0xFFFF);
        core::ptr::write_volatile(&mut (*common).queue_desc, desc_phys);
        core::ptr::write_volatile(&mut (*common).queue_avail, avail_phys);
        core::ptr::write_volatile(&mut (*common).queue_used, used_phys);
    }

    fence(Ordering::Release);

    unsafe {
        core::ptr::write_volatile(&mut (*common).queue_enable, 1);
        let en = core::ptr::read_volatile(&(*common).queue_enable);
        if en != 1 {
            return Err(());
        }
    }

    let notify_off = unsafe { core::ptr::read_volatile(&(*common).queue_notify_off) };
    Ok(VirtQueue::new(
        size,
        mem,
        desc,
        avail,
        used,
        queue_index,
        notify_off,
    ))
}

fn notify_queue_modern(
    notify_base: core::ptr::NonNull<u8>,
    notify_mult: u32,
    queue_index: u16,
    notify_off: u16,
) {
    let off = (notify_off as u32).saturating_mul(notify_mult) as usize;
    unsafe {
        let ptr = notify_base.as_ptr().add(off) as *mut u16;
        core::ptr::write_volatile(ptr, queue_index);
    }
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
struct Rect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

const MAX_SCANOUTS: usize = 16;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct DisplayOne {
    r: Rect,
    enabled: u32,
    flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct RespDisplayInfo {
    hdr: CtrlHdr,
    pmodes: [DisplayOne; MAX_SCANOUTS],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CmdGetDisplayInfo {
    hdr: CtrlHdr,
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
struct CmdSetScanout {
    hdr: CtrlHdr,
    r: Rect,
    scanout_id: u32,
    resource_id: u32,
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
    // followed by `nr_entries` MemEntry entries
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
#[derive(Clone, Copy)]
struct CmdCtxCreate {
    hdr: CtrlHdr,
    // Linux virtio_gpu.h: nlen/context_init + fixed debug_name[64]
    debug_name_len: u32,
    context_init: u32,
    debug_name: [u8; 64],
}

impl Default for CmdCtxCreate {
    fn default() -> Self {
        Self {
            hdr: CtrlHdr::default(),
            debug_name_len: 0,
            context_init: 0,
            debug_name: [0u8; 64],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CmdCtxResource {
    hdr: CtrlHdr,
    resource_id: u32,
    padding: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CmdResourceCreate3d {
    hdr: CtrlHdr,
    resource_id: u32,
    target: u32,
    format: u32,
    bind: u32,
    width: u32,
    height: u32,
    depth: u32,
    array_size: u32,
    last_level: u32,
    nr_samples: u32,
    flags: u32,
    padding: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CmdSubmit3d {
    hdr: CtrlHdr,
    size: u32,
    padding: u32,
    // followed by `size` bytes of virgl command stream
}

pub struct VirtioGpu3d {
    common: core::ptr::NonNull<VirtioPciCommonCfg>,
    notify: core::ptr::NonNull<u8>,
    notify_mult: u32,
    ctrlq: VirtQueue,
    req: DmaRegion,
    resp: DmaRegion,
}

use spin::Mutex;

// -------------------------------------------------------------------------------------------------
// Serialized virtio-gpu access ("GPU actor")
// -------------------------------------------------------------------------------------------------
//
// The virtio-gpu device has a single control queue and this driver is written assuming a single
// outstanding ctrlq descriptor chain. If multiple subsystems call into VirtioGpu3d concurrently
// (scanout/mirror paths, shell gfx switching), and especially if any of those
// paths poll the async executor while holding locks, we can hit lock inversion and apparent BSP
// freezes.
//
// To avoid this, we serialize all virtio-gpu operations through a small in-kernel "actor":
// - A single global VirtioGpu3d instance (no external mutex exposure)
// - A command queue + response map
// - A `gpu_service_step()` function that executes at most one command
//
// This is intentionally synchronous: callers can drive progress by calling `gpu_service_step()`
// while waiting for their response, without polling the async executor.

static GPU_ACTOR_GPU: Mutex<Option<VirtioGpu3d>> = Mutex::new(None);
static GPU_ACTOR_QUEUE: Mutex<VecDeque<(u32, GpuCmd)>> = Mutex::new(VecDeque::new());
static GPU_ACTOR_RESP: Mutex<BTreeMap<u32, GpuResp>> = Mutex::new(BTreeMap::new());
static GPU_ACTOR_NEXT_ID: AtomicU32 = AtomicU32::new(1);
static GPU_ACTOR_PROCESSING: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

#[derive(Clone, Copy, Debug)]
enum GpuCmd {
    GetDisplayInfo,
    ResourceCreate2D {
        resource_id: u32,
        format: u32,
        width: u32,
        height: u32,
    },
    ResourceAttachBacking {
        resource_id: u32,
        backing_phys: u64,
        backing_len: u32,
    },
    SetScanout {
        scanout_id: u32,
        resource_id: u32,
        width: u32,
        height: u32,
    },
    TransferToHost2D {
        resource_id: u32,
        width: u32,
        height: u32,
    },
    ResourceFlush {
        resource_id: u32,
        width: u32,
        height: u32,
    },
}

#[derive(Clone, Copy, Debug)]
enum GpuResp {
    DisplayInfo(Option<(u32, u32, u32)>),
    Bool(bool),
}

fn gpu_submit(cmd: GpuCmd) -> u32 {
    let id = GPU_ACTOR_NEXT_ID
        .fetch_add(1, Ordering::Relaxed)
        .wrapping_add(1);
    GPU_ACTOR_QUEUE.lock().push_back((id, cmd));
    id
}

fn gpu_take_resp(id: u32) -> Option<GpuResp> {
    GPU_ACTOR_RESP.lock().remove(&id)
}

fn gpu_ensure_inited_locked(gpu_slot: &mut Option<VirtioGpu3d>) -> bool {
    if gpu_slot.is_some() {
        return true;
    }
    *gpu_slot = VirtioGpu3d::init_first();
    gpu_slot.is_some()
}

/// Execute at most one queued GPU command.
fn gpu_service_step() {
    if GPU_ACTOR_PROCESSING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    let maybe_cmd = GPU_ACTOR_QUEUE.lock().pop_front();
    if let Some((id, cmd)) = maybe_cmd {
        let mut gpu_guard = GPU_ACTOR_GPU.lock();
        if !gpu_ensure_inited_locked(&mut gpu_guard) {
            GPU_ACTOR_RESP.lock().insert(
                id,
                match cmd {
                    GpuCmd::GetDisplayInfo => GpuResp::DisplayInfo(None),
                    _ => GpuResp::Bool(false),
                },
            );
        } else {
            let gpu = gpu_guard.as_mut().expect("gpu init");
            let resp = match cmd {
                GpuCmd::GetDisplayInfo => GpuResp::DisplayInfo(gpu.get_display_info()),
                GpuCmd::ResourceCreate2D {
                    resource_id,
                    format,
                    width,
                    height,
                } => GpuResp::Bool(gpu.resource_create_2d(resource_id, format, width, height)),
                GpuCmd::ResourceAttachBacking {
                    resource_id,
                    backing_phys,
                    backing_len,
                } => GpuResp::Bool(gpu.resource_attach_backing(
                    resource_id,
                    backing_phys,
                    backing_len,
                )),
                GpuCmd::SetScanout {
                    scanout_id,
                    resource_id,
                    width,
                    height,
                } => GpuResp::Bool(gpu.set_scanout(scanout_id, resource_id, width, height)),
                GpuCmd::TransferToHost2D {
                    resource_id,
                    width,
                    height,
                } => GpuResp::Bool(gpu.transfer_to_host_2d(resource_id, width, height)),
                GpuCmd::ResourceFlush {
                    resource_id,
                    width,
                    height,
                } => GpuResp::Bool(gpu.resource_flush(resource_id, width, height)),
            };
            GPU_ACTOR_RESP.lock().insert(id, resp);
        }
    }

    GPU_ACTOR_PROCESSING.store(false, Ordering::Release);
}

fn gpu_wait_resp(id: u32, timeout_ms: u64) -> Option<GpuResp> {
    let hz = TICK_HZ;
    let ticks = if hz == 0 {
        0
    } else {
        timeout_ms.saturating_mul(hz).div_ceil(1000).max(1)
    };
    let deadline = now().saturating_add(ticks);

    loop {
        if let Some(r) = gpu_take_resp(id) {
            return Some(r);
        }

        // Drive one GPU step locally.
        gpu_service_step();

        if let Some(r) = gpu_take_resp(id) {
            return Some(r);
        }
        if timeout_ms != 0 && now() >= deadline {
            return None;
        }

        // Low-level wait: do not poll the async executor here.
        wait::spin_step_no_exec();
    }
}

// Public wrappers used by scanout/mirror/backends.

pub fn gpu_get_display_info(timeout_ms: u64) -> Option<(u32, u32, u32)> {
    let id = gpu_submit(GpuCmd::GetDisplayInfo);
    match gpu_wait_resp(id, timeout_ms)? {
        GpuResp::DisplayInfo(v) => v,
        _ => None,
    }
}

pub fn gpu_resource_create_2d(
    resource_id: u32,
    format: u32,
    width: u32,
    height: u32,
    timeout_ms: u64,
) -> bool {
    let id = gpu_submit(GpuCmd::ResourceCreate2D {
        resource_id,
        format,
        width,
        height,
    });
    matches!(gpu_wait_resp(id, timeout_ms), Some(GpuResp::Bool(true)))
}

pub fn gpu_resource_attach_backing(
    resource_id: u32,
    backing_phys: u64,
    backing_len: u32,
    timeout_ms: u64,
) -> bool {
    let id = gpu_submit(GpuCmd::ResourceAttachBacking {
        resource_id,
        backing_phys,
        backing_len,
    });
    matches!(gpu_wait_resp(id, timeout_ms), Some(GpuResp::Bool(true)))
}

pub fn gpu_set_scanout(
    scanout_id: u32,
    resource_id: u32,
    width: u32,
    height: u32,
    timeout_ms: u64,
) -> bool {
    let id = gpu_submit(GpuCmd::SetScanout {
        scanout_id,
        resource_id,
        width,
        height,
    });
    matches!(gpu_wait_resp(id, timeout_ms), Some(GpuResp::Bool(true)))
}

pub fn gpu_transfer_to_host_2d(resource_id: u32, width: u32, height: u32, timeout_ms: u64) -> bool {
    let id = gpu_submit(GpuCmd::TransferToHost2D {
        resource_id,
        width,
        height,
    });
    matches!(gpu_wait_resp(id, timeout_ms), Some(GpuResp::Bool(true)))
}

pub fn gpu_resource_flush(resource_id: u32, width: u32, height: u32, timeout_ms: u64) -> bool {
    let id = gpu_submit(GpuCmd::ResourceFlush {
        resource_id,
        width,
        height,
    });
    matches!(gpu_wait_resp(id, timeout_ms), Some(GpuResp::Bool(true)))
}

// NOTE: `with_global_gpu` is intentionally removed from cross-subsystem usage.
// All scanout/mirror callers must use the serialized `gpu_*` wrappers above.

unsafe impl Send for VirtioGpu3d {}

impl VirtioGpu3d {
    pub fn init_first() -> Option<Self> {
        let dev = find_device().or_else(|| {
            // Backend switching can be invoked from the shell; if the PCI list is empty or stale,
            // re-enumerate once before failing.
            pci::enumerate_impl();
            find_device()
        })?;
        let caps = parse_modern_caps(&dev)?;
        enable_mem_and_bus_master(&dev);

        let common_map = pci::mmio::map_mmio_region_exact(
            caps.common_phys,
            (caps.common_len as usize).max(core::mem::size_of::<VirtioPciCommonCfg>()),
        )
        .ok()?;
        let notify_map =
            pci::mmio::map_mmio_region_exact(caps.notify_phys, (caps.notify_len as usize).max(4))
                .ok()?;

        let common = core::ptr::NonNull::new(common_map.as_ptr() as *mut VirtioPciCommonCfg)?;
        let notify = core::ptr::NonNull::new(notify_map.as_ptr() as *mut u8)?;

        if !modern_negotiate_minimal(common) {
            return None;
        }

        let ctrlq = setup_queue_modern(common, QUEUE_CONTROL).ok()?;

        unsafe {
            let c = common.as_ptr();
            let mut status = core::ptr::read_volatile(&(*c).device_status);
            status |= VIRTIO_STATUS_DRIVER_OK;
            core::ptr::write_volatile(&mut (*c).device_status, status);
        }

        let req = DmaRegion::alloc(64 * 1024, 16)?;
        let resp = DmaRegion::alloc(4 * 1024, 16)?;

        Some(Self {
            common,
            notify,
            notify_mult: caps.notify_mult,
            ctrlq,
            req,
            resp,
        })
    }

    pub fn ctx_create(&mut self, ctx_id: u32) -> bool {
        let req = CmdCtxCreate {
            hdr: CtrlHdr {
                type_: VIRTIO_GPU_CMD_CTX_CREATE,
                flags: 0,
                fence_id: 0,
                ctx_id,
                padding: 0,
            },
            debug_name_len: 0,
            context_init: 0,
            debug_name: [0u8; 64],
        };
        self.ctrl_submit_bytes(as_bytes(&req))
    }

    pub fn ctx_attach_resource(&mut self, ctx_id: u32, resource_id: u32) -> bool {
        let req = CmdCtxResource {
            hdr: CtrlHdr {
                type_: VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE,
                flags: 0,
                fence_id: 0,
                ctx_id,
                padding: 0,
            },
            resource_id,
            padding: 0,
        };
        self.ctrl_submit_bytes(as_bytes(&req))
    }

    pub fn ctx_detach_resource(&mut self, ctx_id: u32, resource_id: u32) -> bool {
        let req = CmdCtxResource {
            hdr: CtrlHdr {
                type_: VIRTIO_GPU_CMD_CTX_DETACH_RESOURCE,
                flags: 0,
                fence_id: 0,
                ctx_id,
                padding: 0,
            },
            resource_id,
            padding: 0,
        };
        self.ctrl_submit_bytes(as_bytes(&req))
    }

    pub fn get_display_info(&mut self) -> Option<(u32, u32, u32)> {
        let req = CmdGetDisplayInfo {
            hdr: CtrlHdr {
                type_: VIRTIO_GPU_CMD_GET_DISPLAY_INFO,
                flags: 0,
                fence_id: 0,
                ctx_id: 0,
                padding: 0,
            },
        };
        let resp_type = self.ctrl_submit_bytes_ret_type(as_bytes(&req))?;
        if resp_type != VIRTIO_GPU_RESP_OK_DISPLAY_INFO {
            return None;
        }
        let info = unsafe { &*(self.resp.virt() as *const RespDisplayInfo) };
        for (i, m) in info.pmodes.iter().enumerate() {
            if m.enabled != 0 && m.r.width != 0 && m.r.height != 0 {
                return Some((i as u32, m.r.width, m.r.height));
            }
        }
        Some((0, 1024, 768))
    }

    pub fn set_scanout(
        &mut self,
        scanout_id: u32,
        resource_id: u32,
        width: u32,
        height: u32,
    ) -> bool {
        let req = CmdSetScanout {
            hdr: CtrlHdr {
                type_: VIRTIO_GPU_CMD_SET_SCANOUT,
                flags: 0,
                fence_id: 0,
                ctx_id: 0,
                padding: 0,
            },
            r: Rect {
                x: 0,
                y: 0,
                width,
                height,
            },
            scanout_id,
            resource_id,
        };
        let ok = self.ctrl_submit_bytes(as_bytes(&req));
        if !ok {
            let resp_hdr = unsafe { &*(self.resp.virt() as *const CtrlHdr) };
            crate::log!(
                "virgl: set_scanout failed scanout={} res={} resp=0x{:04X}\n",
                scanout_id,
                resource_id,
                resp_hdr.type_
            );
        }
        ok
    }

    pub fn resource_create_2d(
        &mut self,
        resource_id: u32,
        format: u32,
        width: u32,
        height: u32,
    ) -> bool {
        let req = CmdResourceCreate2d {
            hdr: CtrlHdr {
                type_: VIRTIO_GPU_CMD_RESOURCE_CREATE_2D,
                flags: 0,
                fence_id: 0,
                ctx_id: 0,
                padding: 0,
            },
            resource_id,
            format,
            width,
            height,
        };
        self.ctrl_submit_bytes(as_bytes(&req))
    }

    pub fn resource_flush(&mut self, resource_id: u32, width: u32, height: u32) -> bool {
        let req = CmdResourceFlush {
            hdr: CtrlHdr {
                type_: VIRTIO_GPU_CMD_RESOURCE_FLUSH,
                flags: 0,
                fence_id: 0,
                ctx_id: 0,
                padding: 0,
            },
            r: Rect {
                x: 0,
                y: 0,
                width,
                height,
            },
            resource_id,
            padding: 0,
        };
        self.ctrl_submit_bytes(as_bytes(&req))
    }

    pub fn resource_attach_backing(
        &mut self,
        resource_id: u32,
        backing_phys: u64,
        backing_len: u32,
    ) -> bool {
        let hdr = CtrlHdr {
            type_: VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING,
            flags: 0,
            fence_id: 0,
            ctx_id: 0,
            padding: 0,
        };
        let header = CmdResourceAttachBacking {
            hdr,
            resource_id,
            nr_entries: 1,
        };
        let entry = MemEntry {
            addr: backing_phys,
            length: backing_len,
            padding: 0,
        };

        let header_bytes = as_bytes(&header);
        let entry_bytes = as_bytes(&entry);
        let total = header_bytes.len().saturating_add(entry_bytes.len());
        if total == 0 || total > self.req.len() {
            return false;
        }
        unsafe {
            core::ptr::copy_nonoverlapping(
                header_bytes.as_ptr(),
                self.req.virt(),
                header_bytes.len(),
            );
            core::ptr::copy_nonoverlapping(
                entry_bytes.as_ptr(),
                self.req.virt().add(header_bytes.len()),
                entry_bytes.len(),
            );
            core::ptr::write_bytes(self.resp.virt(), 0, self.resp.len());
        }
        self.ctrl_submit_desc_chain(total)
    }

    pub fn transfer_to_host_2d(&mut self, resource_id: u32, width: u32, height: u32) -> bool {
        let req = CmdTransferToHost2d {
            hdr: CtrlHdr {
                type_: VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D,
                flags: 0,
                fence_id: 0,
                ctx_id: 0,
                padding: 0,
            },
            r: Rect {
                x: 0,
                y: 0,
                width,
                height,
            },
            offset: 0,
            resource_id,
            padding: 0,
        };
        self.ctrl_submit_bytes(as_bytes(&req))
    }

    pub fn resource_create_3d(
        &mut self,
        ctx_id: u32,
        resource_id: u32,
        target: u32,
        format: u32,
        bind: u32,
        width: u32,
        height: u32,
        depth: u32,
        array_size: u32,
        last_level: u32,
        nr_samples: u32,
        flags: u32,
    ) -> bool {
        let req = CmdResourceCreate3d {
            hdr: CtrlHdr {
                type_: VIRTIO_GPU_CMD_RESOURCE_CREATE_3D,
                flags: 0,
                fence_id: 0,
                ctx_id,
                padding: 0,
            },
            resource_id,
            target,
            format,
            bind,
            width,
            height,
            depth,
            array_size,
            last_level,
            nr_samples,
            flags,
            padding: 0,
        };
        self.ctrl_submit_bytes(as_bytes(&req))
    }

    /// Submit a virgl 3D command stream to the device.
    ///
    /// Note: We intentionally do NOT use virtio-gpu fences here. Without a fence-wait
    /// mechanism, a fenced submit can complete before rendering finishes, leading to
    /// stale frames being flushed to scanout.
    pub fn submit_3d(&mut self, ctx_id: u32, cmd_stream: &[u8], _fence_id: u64) -> bool {
        let hdr = CtrlHdr {
            type_: VIRTIO_GPU_CMD_SUBMIT_3D,
            flags: 0,
            fence_id: 0,
            ctx_id,
            padding: 0,
        };
        let header = CmdSubmit3d {
            hdr,
            size: cmd_stream.len().min(u32::MAX as usize) as u32,
            padding: 0,
        };

        let header_bytes = as_bytes(&header);
        let total = header_bytes.len().saturating_add(cmd_stream.len());
        if total == 0 || total > self.req.len() {
            return false;
        }

        unsafe {
            core::ptr::copy_nonoverlapping(
                header_bytes.as_ptr(),
                self.req.virt(),
                header_bytes.len(),
            );
            core::ptr::copy_nonoverlapping(
                cmd_stream.as_ptr(),
                self.req.virt().add(header_bytes.len()),
                cmd_stream.len(),
            );
            core::ptr::write_bytes(self.resp.virt(), 0, self.resp.len());
        }

        self.ctrl_submit_desc_chain(total)
    }

    fn ctrl_submit_bytes(&mut self, req_bytes: &[u8]) -> bool {
        if req_bytes.is_empty() || req_bytes.len() > self.req.len() {
            return false;
        }

        unsafe {
            core::ptr::copy_nonoverlapping(req_bytes.as_ptr(), self.req.virt(), req_bytes.len());
            core::ptr::write_bytes(self.resp.virt(), 0, self.resp.len());
        }

        self.ctrl_submit_desc_chain(req_bytes.len())
    }

    fn ctrl_submit_bytes_ret_type(&mut self, req_bytes: &[u8]) -> Option<u32> {
        if req_bytes.is_empty() || req_bytes.len() > self.req.len() {
            return None;
        }

        unsafe {
            core::ptr::copy_nonoverlapping(req_bytes.as_ptr(), self.req.virt(), req_bytes.len());
            core::ptr::write_bytes(self.resp.virt(), 0, self.resp.len());
        }

        if !self.ctrl_submit_desc_chain(req_bytes.len()) {
            return None;
        }
        let resp_hdr = unsafe { &*(self.resp.virt() as *const CtrlHdr) };
        Some(resp_hdr.type_)
    }

    fn ctrl_submit_desc_chain(&mut self, req_len: usize) -> bool {
        // Fixed descriptor pair (0 -> req, 1 -> resp). Single outstanding.
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
        fence(Ordering::Release);
        notify_queue_modern(
            self.notify,
            self.notify_mult,
            self.ctrlq.queue_index,
            self.ctrlq.notify_off,
        );

        // IMPORTANT: do not poll the executor while waiting for ctrlq progress.
        // This function is often called under the global virtio-gpu mutex, and can also be
        // reached while higher-level gfx locks are held. Polling the executor here can re-enter
        // the shell/gfx stack and wedge on those locks (observed as `gfx: SYSTEM lock timeout`).
        let ok = wait::spin_until_timeout_no_exec(1000, || {
            self.ctrlq.used_idx() != self.ctrlq.last_used_idx
        });
        if !ok {
            crate::log!("virtio-gpu3d: ctrlq timeout\n");
            return false;
        }

        let used = self
            .ctrlq
            .used_elem(self.ctrlq.last_used_idx % self.ctrlq.size);
        self.ctrlq.last_used_idx = self.ctrlq.last_used_idx.wrapping_add(1);
        if used.id != 0 {
            crate::log!("virtio-gpu3d: ctrlq used id={} (expected 0)\n", used.id);
            return false;
        }

        let resp_hdr = unsafe { &*(self.resp.virt() as *const CtrlHdr) };
        resp_hdr.type_ == VIRTIO_GPU_RESP_OK_NODATA
            || resp_hdr.type_ == VIRTIO_GPU_RESP_OK_DISPLAY_INFO
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Vertex {
    pos: [f32; 4],
    color: [f32; 4],
}

const VS_TEXT: &str = "VERT\n\
DCL IN[0]\n\
DCL IN[1]\n\
DCL OUT[0], POSITION\n\
DCL OUT[1], COLOR\n\
  0: MOV OUT[1], IN[1]\n\
  1: MOV OUT[0], IN[0]\n\
  2: END\n";

const FS_TEXT: &str = "FRAG\n\
DCL IN[0], COLOR, LINEAR\n\
DCL OUT[0], COLOR\n\
  0: MOV OUT[0], IN[0]\n\
  1: END\n";

// --- gfx-core backend (virgl) ---

use trueos_gfx_core::{
    BufferDesc, BufferId, ColorFormat, Command, CommandBuffer, DeviceCaps, Error, FenceId,
    GfxDevice, GfxPresent, ImageDesc, ImageFormat, ImageId, MapMode, MappedRange, MemoryType,
    PipelineDesc, PipelineId, SamplerDesc, SamplerFilter, SamplerWrap, ShaderDesc, ShaderId,
    SwapchainDesc, TexCoordFormat, VertexLayout, Viewport,
};

// NOTE: Do not import `trueos_gfx_core::Result` as `Result` at module scope.
// This file already uses `Result<T, ()>` in virtio setup code.
use trueos_gfx_core::Result as GfxResult;

#[derive(Clone)]
struct HostBuffer {
    desc: BufferDesc,
    bytes: Vec<u8>,
    mapped: bool,
    revision: u32,
}

struct HostPipeline {
    desc: PipelineDesc,
}

#[derive(Clone)]
struct HostImage {
    desc: ImageDesc,
    bytes: Vec<u8>,
    revision: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ConvertedVertexCacheKey {
    pipeline_id: u32,
    buffer_id: u32,
    buffer_rev: u32,
    binding_offset: u64,
    first_vertex: u32,
    vertex_count: u32,
    layout_stride: u16,
    layout_pos_offset: u16,
    layout_color_offset: u16,
    layout_color_format: ColorFormat,
    layout_texcoord_offset: u16,
    layout_texcoord_format: TexCoordFormat,
    image_id: u32,
    image_rev: u32,
}

struct ConvertedVertexCache {
    key: Option<ConvertedVertexCacheKey>,
    bytes: Vec<u8>,
    vertex_count: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SubmittedFrameKey {
    viewport_w: u32,
    viewport_h: u32,
    clear_rgb: u32,
    draw: Option<(ConvertedVertexCacheKey, u32)>,
}

#[derive(Clone, Copy, Debug)]
struct VirglDrawState {
    pipeline: PipelineId,
    vertex: VirglBufferBinding,
    image: ImageId,
    sampler: SamplerDesc,
    viewport: Viewport,
    clear_rgb: u32,
}

#[derive(Clone, Copy, Debug)]
struct VirglBufferBinding {
    id: BufferId,
    offset: u64,
}

impl Default for VirglDrawState {
    fn default() -> Self {
        Self {
            pipeline: PipelineId::invalid(),
            vertex: VirglBufferBinding {
                id: BufferId::invalid(),
                offset: 0,
            },
            image: ImageId::invalid(),
            sampler: SamplerDesc::default_2d(),
            viewport: Viewport {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            clear_rgb: 0x00_08_18_30,
        }
    }
}

/// Minimal virgl-backed gfx-core context.
///
/// Scope: supports the current gfx-core command set (colored triangles) so the WebGL shim can
/// draw moving rectangles (two triangles) without caring which backend is active.
pub struct VirglGfxBackend {
    gpu: VirtioGpu3d,
    ctx_id: u32,

    // Present via 2D scanout for compatibility.
    scanout_id: u32,
    width: u32,
    height: u32,
    scanout_res: u32,
    scanout_backing: ScanoutBacking,
    rt_res: u32,

    // VBO resource for virgl vertices.
    vbo_res: u32,
    vbo_cap_bytes: u32,

    // One-time virgl state handles.
    surf_handle: u32,
    ve_handle: u32,
    vs_handle: u32,
    fs_handle: u32,
    blend_handle: u32,
    dsa_handle: u32,
    rs_handle: u32,

    swapchain: SwapchainDesc,

    buffers: Vec<Option<HostBuffer>>,
    shaders: Vec<Option<Vec<u8>>>,
    pipelines: Vec<Option<HostPipeline>>,
    images: Vec<Option<HostImage>>,

    state: VirglDrawState,
    converted_cache: ConvertedVertexCache,
    uploaded_cache_key: Option<ConvertedVertexCacheKey>,
    uploaded_cache_vbo_generation: u32,
    vbo_generation: u32,
    last_submitted_frame: Option<SubmittedFrameKey>,

    next_fence: u64,
    completed_fence: u64,
}

impl VirglGfxBackend {
    pub fn init(
        framebuffers: Option<&'static ::limine::response::FramebufferResponse>,
    ) -> Option<Self> {
        let mut gpu = VirtioGpu3d::init_first()?;
        let (scanout_id, disp_w, disp_h) = gpu.get_display_info()?;

        let ctx_id = alloc_ctx_id();
        if !gpu.ctx_create(ctx_id) {
            crate::log!("virgl-backend: ctx_create failed\n");
            return None;
        }

        // Allocate resource ids.
        let (scanout_res, rt_res) = alloc_res_pair();
        let vbo_res = alloc_res_pair().0;

        // 2D scanout + backing.
        // If we can borrow the Limine framebuffer memory as backing, we create a scanout
        // resource wide enough to match the framebuffer pitch (pitch/4 pixels).
        // The scanout rect remains the virtio-gpu display width/height.
        // Prefer borrowing Limine FB backing (shared memory) so toggling back to LimineFB is visible.
        // IMPORTANT: Limine's chosen framebuffer size can differ from virtio-gpu display-info size.
        // If we borrow, we drive the scanout size from the Limine FB (clamped to display size).
        let (scanout_backing, res_w, res_h, res_bytes, present_w, present_h) = if let Some(b) =
            Self::try_borrow_limine_scanout(framebuffers)
        {
            let pw = b.fb_width.min(disp_w).max(1);
            let ph = b.fb_height.min(disp_h).max(1);
            crate::log!(
                "virgl-backend: scanout backing=limine-fb fb={}x{} pitch={} present={}x{} res={}x{} bytes={}\n",
                b.fb_width,
                b.fb_height,
                b.fb_pitch,
                pw,
                ph,
                b.res_width,
                b.res_height,
                b.res_bytes
            );
            (b.backing, b.res_width, b.res_height, b.res_bytes, pw, ph)
        } else {
            let bytes = (disp_w as usize)
                .saturating_mul(disp_h as usize)
                .saturating_mul(4)
                .max(4096);
            crate::log!(
                "virgl-backend: scanout backing=dma display={}x{} bytes={}\n",
                disp_w,
                disp_h,
                bytes
            );
            (
                ScanoutBacking::Owned(DmaRegion::alloc(bytes, 4096)?),
                disp_w,
                disp_h,
                bytes,
                disp_w,
                disp_h,
            )
        };

        if !gpu.resource_create_2d(scanout_res, VIRGL_FORMAT_B8G8R8X8_UNORM, res_w, res_h) {
            crate::log!("virgl-backend: resource_create_2d scanout failed\n");
            return None;
        }

        // Clear the scanout backing so we start from a known state.
        let clear_len = res_bytes.min(scanout_backing.len());
        unsafe { core::ptr::write_bytes(scanout_backing.virt(), 0, clear_len) };

        if scanout_backing.len() < res_bytes {
            crate::log!(
                "virgl-backend: scanout backing too small len={} need={}\n",
                scanout_backing.len(),
                res_bytes
            );
            return None;
        }

        if !gpu.resource_attach_backing(scanout_res, scanout_backing.phys(), res_bytes as u32) {
            crate::log!("virgl-backend: attach_backing failed\n");
            return None;
        }
        if !gpu.set_scanout(scanout_id, scanout_res, present_w, present_h) {
            crate::log!("virgl-backend: set_scanout failed\n");
            return None;
        }

        // Render target (3D texture).
        let rt_bind = PIPE_BIND_RENDER_TARGET | PIPE_BIND_BLENDABLE;
        if !gpu.resource_create_3d(
            ctx_id,
            rt_res,
            PIPE_TEXTURE_2D,
            VIRGL_FORMAT_B8G8R8X8_UNORM,
            rt_bind,
            present_w,
            present_h,
            1,
            1,
            0,
            0,
            0,
        ) {
            crate::log!("virgl-backend: resource_create_3d rt failed\n");
            return None;
        }

        // VBO resource (PIPE_BUFFER). Start with a modest capacity and grow as needed.
        let vbo_cap_bytes = 64 * 1024u32;
        if !gpu.resource_create_3d(
            ctx_id,
            vbo_res,
            PIPE_BUFFER,
            VIRGL_FORMAT_R8_UNORM,
            PIPE_BIND_VERTEX_BUFFER,
            vbo_cap_bytes,
            1,
            1,
            1,
            0,
            0,
            0,
        ) {
            crate::log!("virgl-backend: resource_create_3d vbo failed\n");
            return None;
        }

        let _ = gpu.ctx_attach_resource(ctx_id, scanout_res);
        let _ = gpu.ctx_attach_resource(ctx_id, rt_res);
        let _ = gpu.ctx_attach_resource(ctx_id, vbo_res);

        // One-time state/program setup.
        let surf_handle = 10u32;
        let ve_handle = 11u32;
        let vs_handle = 20u32;
        let fs_handle = 21u32;
        let blend_handle = 30u32;
        let dsa_handle = 31u32;
        let rs_handle = 32u32;

        let mut init = VirglCmdBuf::new();
        encode_create_surface(&mut init, surf_handle, rt_res, VIRGL_FORMAT_B8G8R8X8_UNORM);
        encode_set_framebuffer(&mut init, surf_handle);

        encode_create_vertex_elements(&mut init, ve_handle);
        encode_bind_object(&mut init, VIRGL_OBJECT_VERTEX_ELEMENTS, ve_handle);

        // Virgl vertex format is fixed for this minimal backend.
        encode_set_vertex_buffer(&mut init, core::mem::size_of::<Vertex>() as u32, 0, vbo_res);

        encode_shader(&mut init, vs_handle, PIPE_SHADER_VERTEX, VS_TEXT);
        encode_bind_shader(&mut init, vs_handle, PIPE_SHADER_VERTEX);
        encode_shader(&mut init, fs_handle, PIPE_SHADER_FRAGMENT, FS_TEXT);
        encode_bind_shader(&mut init, fs_handle, PIPE_SHADER_FRAGMENT);
        encode_link_shader(&mut init, vs_handle, fs_handle);

        encode_create_blend(&mut init, blend_handle);
        encode_bind_object(&mut init, VIRGL_OBJECT_BLEND, blend_handle);
        encode_create_dsa(&mut init, dsa_handle);
        encode_bind_object(&mut init, VIRGL_OBJECT_DSA, dsa_handle);
        encode_create_rasterizer(&mut init, rs_handle);
        encode_bind_object(&mut init, VIRGL_OBJECT_RASTERIZER, rs_handle);

        encode_set_viewport(&mut init, present_w, present_h);

        if !gpu.submit_3d(ctx_id, init.as_bytes(), 1) {
            crate::log!("virgl-backend: submit_3d init failed\n");
            return None;
        }

        let swapchain = SwapchainDesc {
            format: ImageFormat::Rgbx8888,
            extent: trueos_gfx_core::Extent2D {
                width: present_w,
                height: present_h,
            },
        };

        Some(Self {
            gpu,
            ctx_id,
            scanout_id,
            width: present_w,
            height: present_h,
            scanout_res,
            scanout_backing,
            rt_res,
            vbo_res,
            vbo_cap_bytes,
            surf_handle,
            ve_handle,
            vs_handle,
            fs_handle,
            blend_handle,
            dsa_handle,
            rs_handle,
            swapchain,
            buffers: Vec::new(),
            shaders: Vec::new(),
            pipelines: Vec::new(),
            images: Vec::new(),
            state: VirglDrawState {
                viewport: Viewport {
                    x: 0,
                    y: 0,
                    width: present_w as i32,
                    height: present_h as i32,
                },
                ..VirglDrawState::default()
            },
            converted_cache: ConvertedVertexCache {
                key: None,
                bytes: Vec::new(),
                vertex_count: 0,
            },
            uploaded_cache_key: None,
            uploaded_cache_vbo_generation: 0,
            vbo_generation: 1,
            last_submitted_frame: None,
            next_fence: 1,
            completed_fence: 0,
        })
    }

    fn try_borrow_limine_scanout(
        framebuffers: Option<&'static ::limine::response::FramebufferResponse>,
    ) -> Option<BorrowedScanout> {
        let fb_resp = framebuffers?;
        let fb = fb_resp.framebuffers().next()?;

        if fb.bpp() != 32 {
            crate::log!(
                "virgl-backend: borrow-limine: bpp != 32 (bpp={})\n",
                fb.bpp()
            );
            return None;
        }
        let pitch = fb.pitch() as usize;

        let fb_h = fb.height() as usize;
        let len = pitch.saturating_mul(fb_h);
        if len == 0 {
            crate::log!(
                "virgl-backend: borrow-limine: len==0 pitch={} h={}\n",
                pitch,
                fb_h
            );
            return None;
        }

        let virt = fb.addr();
        if virt.is_null() {
            crate::log!("virgl-backend: borrow-limine: fb.addr is null\n");
            return None;
        }
        let Some(phys) = crate::phys::virt_to_phys_checked(virt as *const u8) else {
            crate::log!(
                "virgl-backend: borrow-limine: virt_to_phys failed addr=0x{:X}\n",
                virt as usize
            );
            return None;
        };

        if !pitch.is_multiple_of(4) {
            crate::log!(
                "virgl-backend: borrow-limine: pitch not multiple of 4 (pitch={})\n",
                pitch
            );
            return None;
        }

        let stride_pixels = (pitch / 4) as u32;
        let fb_width = fb.width() as u32;
        let fb_height = fb.height() as u32;

        if stride_pixels == 0 || fb_height == 0 {
            crate::log!(
                "virgl-backend: borrow-limine: bad dims stride={} fb={}x{}\n",
                stride_pixels,
                fb_width,
                fb_height
            );
            return None;
        }

        Some(BorrowedScanout {
            backing: ScanoutBacking::Borrowed { phys, virt, len },
            res_width: stride_pixels,
            res_height: fb_h as u32,
            res_bytes: len,
            fb_width,
            fb_height,
            fb_pitch: pitch,
        })
    }

    fn alloc_slot<T>(slots: &mut Vec<Option<T>>, value: T) -> usize {
        for (i, slot) in slots.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(value);
                return i;
            }
        }
        slots.push(Some(value));
        slots.len() - 1
    }

    fn ensure_vbo_capacity(&mut self, need: usize) -> bool {
        let need_u32 = need.min(u32::MAX as usize) as u32;
        if need_u32 <= self.vbo_cap_bytes {
            return true;
        }

        // Allocate a new resource id and bind it as the vertex buffer.
        let new_vbo_res = alloc_res_pair().0;
        let new_cap = need_u32.next_power_of_two().max(64 * 1024);

        if !self.gpu.resource_create_3d(
            self.ctx_id,
            new_vbo_res,
            PIPE_BUFFER,
            VIRGL_FORMAT_R8_UNORM,
            PIPE_BIND_VERTEX_BUFFER,
            new_cap,
            1,
            1,
            1,
            0,
            0,
            0,
        ) {
            return false;
        }
        let _ = self.gpu.ctx_attach_resource(self.ctx_id, new_vbo_res);

        // Re-bind the vertex buffer state.
        let mut cmd = VirglCmdBuf::new();
        encode_set_vertex_buffer(
            &mut cmd,
            core::mem::size_of::<Vertex>() as u32,
            0,
            new_vbo_res,
        );
        if !self.gpu.submit_3d(self.ctx_id, cmd.as_bytes(), 0) {
            return false;
        }

        self.vbo_res = new_vbo_res;
        self.vbo_cap_bytes = new_cap;
        self.vbo_generation = self.vbo_generation.wrapping_add(1);
        if self.vbo_generation == 0 {
            self.vbo_generation = 1;
        }
        self.uploaded_cache_key = None;
        self.uploaded_cache_vbo_generation = 0;
        self.last_submitted_frame = None;
        true
    }

    fn rgb_to_f32(rgb: u32) -> (f32, f32, f32) {
        let r = ((rgb >> 16) & 0xFF) as f32 / 255.0;
        let g = ((rgb >> 8) & 0xFF) as f32 / 255.0;
        let b = (rgb & 0xFF) as f32 / 255.0;
        (r, g, b)
    }

    fn build_virgl_vertices(
        &self,
        buf: &[u8],
        pipe: &HostPipeline,
        binding: VirglBufferBinding,
        draw: (u32, u32),
        image: Option<&HostImage>,
    ) -> Option<Vec<Vertex>> {
        let PipelineDesc { vertex_layout, .. } = pipe.desc;
        let VertexLayout {
            stride,
            pos_offset,
            color_offset,
            color_format,
            texcoord_offset,
            texcoord_format,
        } = vertex_layout;

        let stride = stride as usize;
        if stride == 0 {
            return None;
        }

        let first = draw.1 as usize;
        let count = draw.0 as usize;

        let base = (binding.offset as usize).saturating_add(first.saturating_mul(stride));
        let needed = count.saturating_mul(stride);
        if base >= buf.len() {
            return None;
        }
        let end = base.saturating_add(needed).min(buf.len());
        let available = end.saturating_sub(base);
        let vcount = available / stride;
        if vcount == 0 {
            return None;
        }

        let mut out: Vec<Vertex> = Vec::with_capacity(vcount);
        for i in 0..vcount {
            let off = base + i * stride;
            if off + stride > buf.len() {
                break;
            }
            let pos_off = off + (pos_offset as usize);
            if pos_off + 8 > buf.len() {
                break;
            }
            let x = f32::from_le_bytes(buf[pos_off..pos_off + 4].try_into().ok()?);
            let y = f32::from_le_bytes(buf[pos_off + 4..pos_off + 8].try_into().ok()?);

            let mut r_u8 = 255u8;
            let mut g_u8 = 255u8;
            let mut b_u8 = 255u8;
            let mut a_u8 = 255u8;
            let col_off = off + (color_offset as usize);
            match color_format {
                ColorFormat::RgbU8 => {
                    if col_off + 3 <= buf.len() {
                        r_u8 = buf[col_off];
                        g_u8 = buf[col_off + 1];
                        b_u8 = buf[col_off + 2];
                    }
                }
                ColorFormat::RgbaU8 => {
                    if col_off + 4 <= buf.len() {
                        r_u8 = buf[col_off];
                        g_u8 = buf[col_off + 1];
                        b_u8 = buf[col_off + 2];
                        a_u8 = buf[col_off + 3];
                    }
                }
            }

            if texcoord_format == TexCoordFormat::UvF32 {
                let img = image?;
                let tex_off = off + (texcoord_offset as usize);
                if tex_off + 8 > buf.len() {
                    break;
                }
                let u = f32::from_le_bytes(buf[tex_off..tex_off + 4].try_into().ok()?);
                let v = f32::from_le_bytes(buf[tex_off + 4..tex_off + 8].try_into().ok()?);
                let tw = img.desc.width.max(1) as usize;
                let th = img.desc.height.max(1) as usize;
                let sampler = self.state.sampler;

                let wrap_coord = |c: f32, w: SamplerWrap| -> f32 {
                    match w {
                        SamplerWrap::ClampToEdge => c.clamp(0.0, 1.0),
                        SamplerWrap::Repeat => {
                            let mut f = c - floorf(c);
                            if f < 0.0 {
                                f += 1.0;
                            }
                            if f >= 1.0 { 0.0 } else { f }
                        }
                    }
                };

                let clamp_idx = |i: isize, len: usize| -> usize {
                    if len == 0 {
                        return 0;
                    }
                    if i <= 0 {
                        0
                    } else if (i as usize) >= len {
                        len - 1
                    } else {
                        i as usize
                    }
                };

                let repeat_idx = |i: isize, len: usize| -> usize {
                    if len == 0 {
                        return 0;
                    }
                    let m = len as isize;
                    (((i % m) + m) % m) as usize
                };

                let axis_idx = |i: isize, len: usize, w: SamplerWrap| -> usize {
                    match w {
                        SamplerWrap::ClampToEdge => clamp_idx(i, len),
                        SamplerWrap::Repeat => repeat_idx(i, len),
                    }
                };

                let sample = |x: isize, y: isize| -> (u8, u8, u8, u8) {
                    let xi = axis_idx(x, tw, sampler.wrap_s);
                    let yi = axis_idx(y, th, sampler.wrap_t);
                    let t_idx = yi.saturating_mul(tw).saturating_add(xi).saturating_mul(4);
                    if t_idx + 3 <= img.bytes.len() {
                        (
                            img.bytes[t_idx],
                            img.bytes[t_idx + 1],
                            img.bytes[t_idx + 2],
                            img.bytes[t_idx + 3],
                        )
                    } else {
                        (0, 0, 0, 0)
                    }
                };

                let u = wrap_coord(u, sampler.wrap_s);
                let v = wrap_coord(v, sampler.wrap_t);

                let (tr, tg, tb, ta) = if sampler.mag_filter == SamplerFilter::Linear
                    || sampler.min_filter == SamplerFilter::Linear
                {
                    let fx = u * (tw.saturating_sub(1) as f32);
                    let fy = v * (th.saturating_sub(1) as f32);
                    let x0 = floorf(fx) as isize;
                    let y0 = floorf(fy) as isize;
                    let x1 = x0 + 1;
                    let y1 = y0 + 1;
                    let tx = fx - (x0 as f32);
                    let ty = fy - (y0 as f32);

                    let (c00r, c00g, c00b, c00a) = sample(x0, y0);
                    let (c10r, c10g, c10b, c10a) = sample(x1, y0);
                    let (c01r, c01g, c01b, c01a) = sample(x0, y1);
                    let (c11r, c11g, c11b, c11a) = sample(x1, y1);

                    let lerp = |a: f32, b: f32, t: f32| -> f32 { a + (b - a) * t };

                    let r0 = lerp(c00r as f32, c10r as f32, tx);
                    let r1 = lerp(c01r as f32, c11r as f32, tx);
                    let g0 = lerp(c00g as f32, c10g as f32, tx);
                    let g1 = lerp(c01g as f32, c11g as f32, tx);
                    let b0 = lerp(c00b as f32, c10b as f32, tx);
                    let b1 = lerp(c01b as f32, c11b as f32, tx);
                    let a0 = lerp(c00a as f32, c10a as f32, tx);
                    let a1 = lerp(c01a as f32, c11a as f32, tx);

                    (
                        roundf(lerp(r0, r1, ty)).clamp(0.0, 255.0) as u8,
                        roundf(lerp(g0, g1, ty)).clamp(0.0, 255.0) as u8,
                        roundf(lerp(b0, b1, ty)).clamp(0.0, 255.0) as u8,
                        roundf(lerp(a0, a1, ty)).clamp(0.0, 255.0) as u8,
                    )
                } else {
                    let tx = (roundf(u * (tw.saturating_sub(1) as f32)) as isize)
                        .clamp(0, tw.saturating_sub(1) as isize);
                    let ty = (roundf(v * (th.saturating_sub(1) as f32)) as isize)
                        .clamp(0, th.saturating_sub(1) as isize);
                    sample(tx, ty)
                };

                r_u8 = ((r_u8 as u16).saturating_mul(tr as u16) / 255) as u8;
                g_u8 = ((g_u8 as u16).saturating_mul(tg as u16) / 255) as u8;
                b_u8 = ((b_u8 as u16).saturating_mul(tb as u16) / 255) as u8;
                a_u8 = ((a_u8 as u16).saturating_mul(ta as u16) / 255) as u8;
            }

            let rf = (r_u8 as f32) / 255.0;
            let gf = (g_u8 as f32) / 255.0;
            let bf = (b_u8 as f32) / 255.0;
            let af = (a_u8 as f32) / 255.0;

            out.push(Vertex {
                pos: [x, y, 0.0, 1.0],
                color: [rf, gf, bf, af],
            });
        }
        Some(out)
    }
}

impl GfxDevice for VirglGfxBackend {
    fn caps(&self) -> DeviceCaps {
        // For now, advertise the minimal set that matches our host-visible buffer strategy.
        DeviceCaps::minimal_software()
    }

    fn create_buffer(&mut self, desc: BufferDesc) -> GfxResult<BufferId> {
        if desc.size == 0 {
            return Err(Error::Invalid);
        }
        if desc.memory != MemoryType::HostVisible {
            return Err(Error::Unsupported);
        }
        let mut bytes = Vec::new();
        let size = desc.size.min(usize::MAX as u64) as usize;
        bytes.resize(size, 0);
        let slot = Self::alloc_slot(
            &mut self.buffers,
            HostBuffer {
                desc,
                bytes,
                mapped: false,
                revision: 1,
            },
        );
        self.uploaded_cache_key = None;
        self.uploaded_cache_vbo_generation = 0;
        self.last_submitted_frame = None;
        Ok(BufferId::from_raw(slot as u32 + 1))
    }

    fn destroy_buffer(&mut self, id: BufferId) {
        let raw = id.raw();
        if raw == 0 {
            return;
        }
        let idx = (raw - 1) as usize;
        if idx < self.buffers.len() {
            self.buffers[idx] = None;
            if self
                .converted_cache
                .key
                .map(|k| k.buffer_id == raw)
                .unwrap_or(false)
            {
                self.converted_cache.key = None;
                self.converted_cache.bytes.clear();
                self.converted_cache.vertex_count = 0;
            }
            if self
                .uploaded_cache_key
                .map(|k| k.buffer_id == raw)
                .unwrap_or(false)
            {
                self.uploaded_cache_key = None;
                self.uploaded_cache_vbo_generation = 0;
                self.last_submitted_frame = None;
            }
        }
    }

    fn create_shader(&mut self, desc: ShaderDesc<'_>) -> GfxResult<ShaderId> {
        // Shaders are currently ignored; we use the fixed TGSI pair.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(desc.bytes);
        let slot = Self::alloc_slot(&mut self.shaders, bytes);
        Ok(ShaderId::from_raw(slot as u32 + 1))
    }

    fn destroy_shader(&mut self, id: ShaderId) {
        let raw = id.raw();
        if raw == 0 {
            return;
        }
        let idx = (raw - 1) as usize;
        if idx < self.shaders.len() {
            self.shaders[idx] = None;
        }
    }

    fn create_pipeline(&mut self, desc: PipelineDesc) -> GfxResult<PipelineId> {
        // Accept the pipeline but only support simple pos+color layouts.
        if desc.vertex_layout.stride == 0 {
            return Err(Error::Invalid);
        }
        let slot = Self::alloc_slot(&mut self.pipelines, HostPipeline { desc });
        Ok(PipelineId::from_raw(slot as u32 + 1))
    }

    fn destroy_pipeline(&mut self, id: PipelineId) {
        let raw = id.raw();
        if raw == 0 {
            return;
        }
        let idx = (raw - 1) as usize;
        if idx < self.pipelines.len() {
            self.pipelines[idx] = None;
            if self
                .converted_cache
                .key
                .map(|k| k.pipeline_id == raw)
                .unwrap_or(false)
            {
                self.converted_cache.key = None;
                self.converted_cache.bytes.clear();
                self.converted_cache.vertex_count = 0;
            }
            if self
                .uploaded_cache_key
                .map(|k| k.pipeline_id == raw)
                .unwrap_or(false)
            {
                self.uploaded_cache_key = None;
                self.uploaded_cache_vbo_generation = 0;
                self.last_submitted_frame = None;
            }
        }
    }

    fn create_image(&mut self, desc: ImageDesc) -> GfxResult<ImageId> {
        if desc.width == 0 || desc.height == 0 {
            return Err(Error::Invalid);
        }
        if desc.format != ImageFormat::Rgba8888 {
            return Err(Error::Unsupported);
        }
        let bytes_len = (desc.width as usize)
            .saturating_mul(desc.height as usize)
            .saturating_mul(4);
        if bytes_len == 0 {
            return Err(Error::Invalid);
        }
        let mut bytes = vec![0; bytes_len];
        let slot = Self::alloc_slot(
            &mut self.images,
            HostImage {
                desc,
                bytes,
                revision: 1,
            },
        );
        Ok(ImageId::from_raw(slot as u32 + 1))
    }

    fn destroy_image(&mut self, id: ImageId) {
        let raw = id.raw();
        if raw == 0 {
            return;
        }
        let idx = (raw - 1) as usize;
        if idx < self.images.len() {
            self.images[idx] = None;
            self.converted_cache.key = None;
            self.uploaded_cache_key = None;
            self.uploaded_cache_vbo_generation = 0;
            self.last_submitted_frame = None;
        }
        if self.state.image.raw() == raw {
            self.state.image = ImageId::invalid();
        }
    }

    fn write_image(&mut self, id: ImageId, data: &[u8]) -> GfxResult<()> {
        let raw = id.raw();
        if raw == 0 {
            return Err(Error::Invalid);
        }
        let idx = (raw - 1) as usize;
        let Some(img) = self.images.get_mut(idx).and_then(|i| i.as_mut()) else {
            return Err(Error::NotFound);
        };
        let expected = (img.desc.width as usize)
            .saturating_mul(img.desc.height as usize)
            .saturating_mul(4);
        if data.len() < expected || img.bytes.len() < expected {
            return Err(Error::Invalid);
        }
        img.bytes[..expected].copy_from_slice(&data[..expected]);
        img.revision = img.revision.wrapping_add(1);
        if img.revision == 0 {
            img.revision = 1;
        }
        self.converted_cache.key = None;
        self.uploaded_cache_key = None;
        self.uploaded_cache_vbo_generation = 0;
        self.last_submitted_frame = None;
        Ok(())
    }

    fn write_buffer(&mut self, id: BufferId, offset: u64, data: &[u8]) -> GfxResult<()> {
        let raw = id.raw();
        if raw == 0 {
            return Err(Error::Invalid);
        }
        let idx = (raw - 1) as usize;
        let Some(buf) = self.buffers.get_mut(idx).and_then(|b| b.as_mut()) else {
            return Err(Error::NotFound);
        };
        let off = offset.min(usize::MAX as u64) as usize;
        if off > buf.bytes.len() {
            return Err(Error::Invalid);
        }
        let end = off.saturating_add(data.len());
        if end > buf.bytes.len() {
            return Err(Error::Invalid);
        }
        let dst = &mut buf.bytes[off..end];
        if dst != data {
            dst.copy_from_slice(data);
            buf.revision = buf.revision.wrapping_add(1);
            if buf.revision == 0 {
                buf.revision = 1;
            }
        }
        Ok(())
    }

    fn map_buffer(&mut self, id: BufferId, _mode: MapMode) -> GfxResult<MappedRange> {
        let raw = id.raw();
        if raw == 0 {
            return Err(Error::Invalid);
        }
        let idx = (raw - 1) as usize;
        let Some(buf) = self.buffers.get_mut(idx).and_then(|b| b.as_mut()) else {
            return Err(Error::NotFound);
        };
        if buf.mapped {
            return Err(Error::Invalid);
        }
        buf.mapped = true;
        Ok(MappedRange {
            ptr: buf.bytes.as_mut_ptr(),
            len: buf.bytes.len(),
        })
    }

    fn unmap_buffer(&mut self, id: BufferId) -> GfxResult<()> {
        let raw = id.raw();
        if raw == 0 {
            return Err(Error::Invalid);
        }
        let idx = (raw - 1) as usize;
        let Some(buf) = self.buffers.get_mut(idx).and_then(|b| b.as_mut()) else {
            return Err(Error::NotFound);
        };
        buf.mapped = false;
        buf.revision = buf.revision.wrapping_add(1);
        if buf.revision == 0 {
            buf.revision = 1;
        }
        Ok(())
    }

    fn submit(&mut self, cmds: CommandBuffer<'_>) -> GfxResult<FenceId> {
        // Translate gfx-core commands into a single virgl command stream per submit.
        let mut draw_vertex_count: Option<(u32, u32)> = None;
        for cmd in cmds.commands {
            match *cmd {
                Command::SetViewport(vp) => {
                    self.state.viewport = vp;
                }
                Command::ClearColor { rgb } => {
                    self.state.clear_rgb = rgb;
                }
                Command::ClearRect { rgb, .. } => {
                    // Rect clears are not supported yet; approximate with full clear.
                    self.state.clear_rgb = rgb;
                }
                Command::BindPipeline(p) => {
                    self.state.pipeline = p;
                }
                Command::BindVertexBuffer { buffer, offset } => {
                    self.state.vertex = VirglBufferBinding { id: buffer, offset };
                }
                Command::BindImage(image) => {
                    self.state.image = image;
                }
                Command::SetSampler(s) => {
                    self.state.sampler = s;
                }
                Command::Draw {
                    vertex_count,
                    first_vertex,
                } => {
                    draw_vertex_count = Some((vertex_count, first_vertex));
                }
                Command::Present => {
                    // handled after loop
                }
            }
        }

        let (vertex_count, first_vertex) = draw_vertex_count.unwrap_or((0, 0));
        if vertex_count == 0 {
            // Still allow present-only clears.
        }

        let mut cmd = VirglCmdBuf::new();
        let mut submitted_draw: Option<(ConvertedVertexCacheKey, u32)> = None;
        let mut draw_upload_needed = false;

        // Ensure viewport matches our swapchain.
        let vp_w = self.state.viewport.width.max(0) as u32;
        let vp_h = self.state.viewport.height.max(0) as u32;
        let vp_w = if vp_w == 0 {
            self.width
        } else {
            vp_w.min(self.width)
        };
        let vp_h = if vp_h == 0 {
            self.height
        } else {
            vp_h.min(self.height)
        };
        encode_set_viewport(&mut cmd, vp_w, vp_h);

        // Clear.
        let (r, g, b) = Self::rgb_to_f32(self.state.clear_rgb);
        encode_clear_color(&mut cmd, r, g, b, 1.0);

        // Draw.
        if vertex_count != 0 {
            let pipeline_raw = self.state.pipeline.raw();
            let pipe_idx = pipeline_raw.saturating_sub(1) as usize;
            let Some(pipe) = self.pipelines.get(pipe_idx).and_then(|p| p.as_ref()) else {
                return Err(Error::NotFound);
            };

            let vraw = self.state.vertex.id.raw();
            let vidx = vraw.saturating_sub(1) as usize;
            let Some(vb) = self.buffers.get(vidx).and_then(|b| b.as_ref()) else {
                return Err(Error::NotFound);
            };

            let cache_key = ConvertedVertexCacheKey {
                pipeline_id: pipeline_raw,
                buffer_id: vraw,
                buffer_rev: vb.revision,
                binding_offset: self.state.vertex.offset,
                first_vertex,
                vertex_count,
                layout_stride: pipe.desc.vertex_layout.stride,
                layout_pos_offset: pipe.desc.vertex_layout.pos_offset,
                layout_color_offset: pipe.desc.vertex_layout.color_offset,
                layout_color_format: pipe.desc.vertex_layout.color_format,
                layout_texcoord_offset: pipe.desc.vertex_layout.texcoord_offset,
                layout_texcoord_format: pipe.desc.vertex_layout.texcoord_format,
                image_id: self.state.image.raw(),
                image_rev: if self.state.image.is_valid() {
                    self.images
                        .get(self.state.image.raw().saturating_sub(1) as usize)
                        .and_then(|i| i.as_ref())
                        .map(|i| i.revision)
                        .unwrap_or(0)
                } else {
                    0
                },
            };

            if self.converted_cache.key != Some(cache_key) {
                let bound_image =
                    if pipe.desc.vertex_layout.texcoord_format == TexCoordFormat::UvF32 {
                        if !self.state.image.is_valid() {
                            return Err(Error::Invalid);
                        }
                        self.images
                            .get(self.state.image.raw().saturating_sub(1) as usize)
                            .and_then(|i| i.as_ref())
                    } else {
                        None
                    };
                let verts = self
                    .build_virgl_vertices(
                        &vb.bytes,
                        pipe,
                        self.state.vertex,
                        (vertex_count, first_vertex),
                        bound_image,
                    )
                    .ok_or(Error::Invalid)?;
                let vbytes: &[u8] = unsafe {
                    core::slice::from_raw_parts(
                        verts.as_ptr() as *const u8,
                        core::mem::size_of_val(verts.as_slice()),
                    )
                };
                self.converted_cache.key = Some(cache_key);
                self.converted_cache.bytes.clear();
                self.converted_cache.bytes.extend_from_slice(vbytes);
                self.converted_cache.vertex_count = verts.len() as u32;
            }

            if self.converted_cache.bytes.is_empty() || self.converted_cache.vertex_count == 0 {
                return Err(Error::Invalid);
            }

            if !self.ensure_vbo_capacity(self.converted_cache.bytes.len()) {
                return Err(Error::OutOfMemory);
            }

            let need_upload = self.uploaded_cache_key != Some(cache_key)
                || self.uploaded_cache_vbo_generation != self.vbo_generation;
            if need_upload {
                encode_inline_write_buffer(
                    &mut cmd,
                    self.vbo_res,
                    self.converted_cache.bytes.as_slice(),
                );
                self.uploaded_cache_key = Some(cache_key);
                self.uploaded_cache_vbo_generation = self.vbo_generation;
                draw_upload_needed = true;
            }
            submitted_draw = Some((cache_key, self.vbo_generation));
            encode_draw_vbo_count(&mut cmd, self.converted_cache.vertex_count);
        }

        let frame_key = SubmittedFrameKey {
            viewport_w: vp_w,
            viewport_h: vp_h,
            clear_rgb: self.state.clear_rgb,
            draw: submitted_draw,
        };
        if !draw_upload_needed && self.last_submitted_frame == Some(frame_key) {
            let fence = self.next_fence;
            self.next_fence = self.next_fence.wrapping_add(1);
            self.completed_fence = fence;
            return Ok(FenceId::from_raw(fence));
        }

        // Present: copy to 2D scanout and flush.
        // NOTE: `transfer_to_host_2d` copies *guest* backing into the host resource.
        // Our scanout is produced via virgl/host-side rendering + copy, so a transfer-to-host
        // here can overwrite the freshly rendered image with stale (often zeroed) guest memory,
        // resulting in a persistent black screen.
        encode_resource_copy_region(
            &mut cmd,
            self.scanout_res,
            self.rt_res,
            self.width,
            self.height,
        );
        let fence = self.next_fence;
        self.next_fence = self.next_fence.wrapping_add(1);

        if !self.gpu.submit_3d(self.ctx_id, cmd.as_bytes(), fence) {
            return Err(Error::Unsupported);
        }

        let _ = self
            .gpu
            .resource_flush(self.scanout_res, self.width, self.height);

        self.last_submitted_frame = Some(frame_key);
        self.completed_fence = fence;
        Ok(FenceId::from_raw(fence))
    }

    fn poll(&mut self, fence: FenceId) -> bool {
        fence.raw() <= self.completed_fence
    }

    fn device_idle(&mut self) {
        // No fences/waits yet.
        self.completed_fence = self.next_fence.saturating_sub(1);
    }
}

impl GfxPresent for VirglGfxBackend {
    fn configure_swapchain(&mut self, desc: SwapchainDesc) -> GfxResult<()> {
        // Keep the stored swapchain; virgl swapchain is fixed at init for now.
        self.swapchain = desc;
        Ok(())
    }

    fn swapchain_desc(&self) -> SwapchainDesc {
        self.swapchain
    }
}

fn encode_draw_vbo_count(buf: &mut VirglCmdBuf, count: u32) {
    // VIRGL_DRAW_VBO_SIZE = 12
    buf.push(virgl_cmd0(VIRGL_CCMD_DRAW_VBO, 0, 12));
    buf.push(0); // start
    buf.push(count); // count
    buf.push(PIPE_PRIM_TRIANGLES);
    buf.push(0); // indexed
    buf.push(1); // instance_count
    buf.push(0); // index_bias
    buf.push(0); // start_instance
    buf.push(0); // primitive_restart
    buf.push(0); // restart_index
    buf.push(0); // min_index
    buf.push(0); // max_index
    buf.push(0); // count_from_so
}

fn encode_shader(buf: &mut VirglCmdBuf, handle: u32, shader_type: u32, text: &str) {
    // Matches virgl_encode_shader_state() with a provided shad_str: num_tokens is a dummy.
    let mut bytes = Vec::new();
    bytes.extend_from_slice(text.as_bytes());
    bytes.push(0);

    let shader_len = bytes.len() as u32;
    let num_tokens = 300u32;
    let offlen = shader_len & 0x7fff_ffff;

    // Base header size=5 dwords: handle, type, offlen, num_tokens, num_outputs.
    let len_dwords = 5 + (bytes.len() as u32).div_ceil(4);
    buf.push(virgl_cmd0(
        VIRGL_CCMD_CREATE_OBJECT,
        VIRGL_OBJECT_SHADER,
        len_dwords,
    ));
    buf.push(handle);
    buf.push(shader_type);
    buf.push(offlen);
    buf.push(num_tokens);
    buf.push(0); // num streamout outputs
    buf.push_bytes_padded(&bytes);
}

fn encode_bind_shader(buf: &mut VirglCmdBuf, handle: u32, shader_type: u32) {
    buf.push(virgl_cmd0(VIRGL_CCMD_BIND_SHADER, 0, 2));
    buf.push(handle);
    buf.push(shader_type);
}

fn encode_link_shader(buf: &mut VirglCmdBuf, vs: u32, fs: u32) {
    buf.push(virgl_cmd0(
        VIRGL_CCMD_LINK_SHADER,
        0,
        VIRGL_LINK_SHADER_SIZE,
    ));
    buf.push(vs);
    buf.push(fs);
    buf.push(0);
    buf.push(0);
    buf.push(0);
    buf.push(0);
}

fn encode_create_surface(buf: &mut VirglCmdBuf, surf_handle: u32, res_handle: u32, format: u32) {
    buf.push(virgl_cmd0(
        VIRGL_CCMD_CREATE_OBJECT,
        VIRGL_OBJECT_SURFACE,
        VIRGL_OBJ_SURFACE_SIZE,
    ));
    buf.push(surf_handle);
    buf.push(res_handle);
    buf.push(format);
    buf.push(0); // level
    buf.push(0); // first_layer | (last_layer<<16)
}

fn encode_set_framebuffer(buf: &mut VirglCmdBuf, surf_handle: u32) {
    // VIRGL_SET_FRAMEBUFFER_STATE_SIZE(nr_cbufs) = nr + 2
    let len = 1 + 2;
    buf.push(virgl_cmd0(VIRGL_CCMD_SET_FRAMEBUFFER_STATE, 0, len));
    buf.push(1); // nr_cbufs
    buf.push(0); // zsbuf
    buf.push(surf_handle);
}

fn encode_clear_color(buf: &mut VirglCmdBuf, r: f32, g: f32, b: f32, a: f32) {
    // VIRGL_OBJ_CLEAR_SIZE = 8 dwords payload:
    // buffers (1) + color[4] (4) + depth(double)=2 dwords + stencil (1).
    buf.push(virgl_cmd0(VIRGL_CCMD_CLEAR, 0, 8));
    buf.push(PIPE_CLEAR_COLOR0);
    buf.push(fui(r));
    buf.push(fui(g));
    buf.push(fui(b));
    buf.push(fui(a));
    // depth is a double in the original encoder; we don't clear depth/stencil.
    buf.push(0);
    buf.push(0);
    buf.push(0); // stencil
}

fn encode_create_vertex_elements(buf: &mut VirglCmdBuf, ve_handle: u32) {
    // VIRGL_OBJ_VERTEX_ELEMENTS_SIZE(num) = num*4 + 1
    let num = 2u32;
    let len = 1 + num * 4;
    buf.push(virgl_cmd0(
        VIRGL_CCMD_CREATE_OBJECT,
        VIRGL_OBJECT_VERTEX_ELEMENTS,
        len,
    ));
    buf.push(ve_handle);

    // element 0: position vec4 at offset 0 from vbo
    buf.push(0);
    buf.push(0);
    buf.push(0);
    buf.push(VIRGL_FORMAT_R32G32B32A32_FLOAT);

    // element 1: color vec4 at offset 16
    buf.push(16);
    buf.push(0);
    buf.push(0);
    buf.push(VIRGL_FORMAT_R32G32B32A32_FLOAT);
}

fn encode_bind_object(buf: &mut VirglCmdBuf, object: u8, handle: u32) {
    buf.push(virgl_cmd0(VIRGL_CCMD_BIND_OBJECT, object, 1));
    buf.push(handle);
}

fn encode_set_vertex_buffer(buf: &mut VirglCmdBuf, stride: u32, offset: u32, res_handle: u32) {
    // VIRGL_SET_VERTEX_BUFFERS_SIZE(num) = num*3
    buf.push(virgl_cmd0(VIRGL_CCMD_SET_VERTEX_BUFFERS, 0, 3));
    buf.push(stride);
    buf.push(offset);
    buf.push(res_handle);
}

fn encode_inline_write_buffer(buf: &mut VirglCmdBuf, res_handle: u32, data: &[u8]) {
    // Matches virgl_encoder_inline_send_box for a PIPE_BUFFER upload.
    // cmd length is data_dwords + 11
    let data_dwords = (data.len() as u32).div_ceil(4);
    buf.push(virgl_cmd0(
        VIRGL_CCMD_RESOURCE_INLINE_WRITE,
        0,
        data_dwords + 11,
    ));
    buf.push(res_handle);
    buf.push(0); // level
    buf.push(0); // usage
    buf.push(data.len() as u32); // stride
    buf.push(0); // layer_stride
    buf.push(0); // box x
    buf.push(0); // box y
    buf.push(0); // box z
    buf.push(data.len() as u32); // box width
    buf.push(1); // box height
    buf.push(1); // box depth
    buf.push_bytes_padded(data);
}

fn encode_resource_copy_region(
    buf: &mut VirglCmdBuf,
    dst_res: u32,
    src_res: u32,
    width: u32,
    height: u32,
) {
    // VIRGL_CMD_RESOURCE_COPY_REGION_SIZE = 13 dwords payload.
    buf.push(virgl_cmd0(VIRGL_CCMD_RESOURCE_COPY_REGION, 0, 13));
    buf.push(dst_res);
    buf.push(0); // dst_level
    buf.push(0); // dst_x
    buf.push(0); // dst_y
    buf.push(0); // dst_z
    buf.push(src_res);
    buf.push(0); // src_level
    buf.push(0); // src_x
    buf.push(0); // src_y
    buf.push(0); // src_z
    buf.push(width);
    buf.push(height);
    buf.push(1); // depth
}

fn encode_set_viewport(buf: &mut VirglCmdBuf, width: u32, height: u32) {
    // VIRGL_SET_VIEWPORT_STATE_SIZE(num) = 6*num + 1
    buf.push(virgl_cmd0(VIRGL_CCMD_SET_VIEWPORT_STATE, 0, 7));
    buf.push(0); // start_slot

    let half_w = width as f32 / 2.0;
    let half_h = height as f32 / 2.0;
    let half_d = 0.5;

    // scale[3]
    buf.push(fui(half_w));
    buf.push(fui(half_h));
    buf.push(fui(half_d));
    // translate[3]
    buf.push(fui(half_w));
    buf.push(fui(half_h));
    buf.push(fui(half_d));
}

fn encode_create_blend(buf: &mut VirglCmdBuf, blend_handle: u32) {
    buf.push(virgl_cmd0(
        VIRGL_CCMD_CREATE_OBJECT,
        VIRGL_OBJECT_BLEND,
        VIRGL_OBJ_BLEND_SIZE,
    ));
    buf.push(blend_handle);
    buf.push(0); // s0
    buf.push(0); // s1
    for i in 0..8u32 {
        let mut rt = 0u32;
        if i == 0 {
            rt |= (PIPE_MASK_RGBA & 0xF) << 27;
        }
        buf.push(rt);
    }
}

fn encode_create_dsa(buf: &mut VirglCmdBuf, dsa_handle: u32) {
    buf.push(virgl_cmd0(
        VIRGL_CCMD_CREATE_OBJECT,
        VIRGL_OBJECT_DSA,
        VIRGL_OBJ_DSA_SIZE,
    ));
    buf.push(dsa_handle);
    buf.push(0);
    buf.push(0);
    buf.push(0);
    buf.push(0); // alpha_ref
}

fn encode_create_rasterizer(buf: &mut VirglCmdBuf, rs_handle: u32) {
    buf.push(virgl_cmd0(
        VIRGL_CCMD_CREATE_OBJECT,
        VIRGL_OBJECT_RASTERIZER,
        VIRGL_OBJ_RS_SIZE,
    ));
    buf.push(rs_handle);

    let mut s0 = 0u32;
    // depth_clip=1
    s0 |= 1 << 1;
    // cull_face=PIPE_FACE_NONE(0) at bits 8..9 -> 0
    // half_pixel_center=1
    s0 |= 1 << 29;
    // bottom_edge_rule=1
    s0 |= 1 << 30;
    buf.push(s0);
    buf.push(fui(1.0)); // point_size
    buf.push(0); // sprite_coord_enable
    buf.push(0); // s3
    buf.push(fui(1.0)); // line_width
    buf.push(fui(0.0)); // offset_units
    buf.push(fui(0.0)); // offset_scale
    buf.push(fui(0.0)); // offset_clamp
}

fn encode_draw_vbo(buf: &mut VirglCmdBuf) {
    // VIRGL_DRAW_VBO_SIZE = 12
    buf.push(virgl_cmd0(VIRGL_CCMD_DRAW_VBO, 0, 12));
    buf.push(0); // start
    buf.push(3); // count
    buf.push(PIPE_PRIM_TRIANGLES);
    buf.push(0); // indexed
    buf.push(1); // instance_count
    buf.push(0); // index_bias
    buf.push(0); // start_instance
    buf.push(0); // primitive_restart
    buf.push(0); // restart_index
    buf.push(0); // min_index
    buf.push(0); // max_index
    buf.push(0); // count_from_so
}

static VIRGL_NEXT_CTX_ID: AtomicU32 = AtomicU32::new(1);
static VIRGL_NEXT_RES_ID: AtomicU32 = AtomicU32::new(1);

fn alloc_ctx_id() -> u32 {
    // ctx_id 0 is reserved.
    let id = VIRGL_NEXT_CTX_ID.fetch_add(1, Ordering::Relaxed);
    if id == 0 { 1 } else { id }
}

fn alloc_res_pair() -> (u32, u32) {
    // resource_id 0 is reserved.
    let base = VIRGL_NEXT_RES_ID.fetch_add(2, Ordering::Relaxed);
    let base = if base == 0 { 1 } else { base };
    (base, base.wrapping_add(1))
}

fn alloc_res_triple() -> (u32, u32, u32) {
    // resource_id 0 is reserved.
    let base = VIRGL_NEXT_RES_ID.fetch_add(3, Ordering::Relaxed);
    let base = if base == 0 { 1 } else { base };
    (base, base.wrapping_add(1), base.wrapping_add(2))
}

fn modern_negotiate_minimal(common: core::ptr::NonNull<VirtioPciCommonCfg>) -> bool {
    unsafe {
        let c = common.as_ptr();
        core::ptr::write_volatile(&mut (*c).device_status, 0);
        core::ptr::write_volatile(
            &mut (*c).device_status,
            VIRTIO_STATUS_ACK | VIRTIO_STATUS_DRIVER,
        );

        core::ptr::write_volatile(&mut (*c).device_feature_select, 0);
        let dev_lo = core::ptr::read_volatile(&(*c).device_feature) as u64;
        core::ptr::write_volatile(&mut (*c).device_feature_select, 1);
        let dev_hi = core::ptr::read_volatile(&(*c).device_feature) as u64;
        let dev_features = dev_lo | (dev_hi << 32);

        let mut guest_features: u64 = 0;
        if (dev_features & VIRTIO_F_VERSION_1) != 0 {
            guest_features |= VIRTIO_F_VERSION_1;
        }
        if (dev_features & VIRTIO_GPU_F_VIRGL) != 0 {
            guest_features |= VIRTIO_GPU_F_VIRGL;
        }

        core::ptr::write_volatile(&mut (*c).driver_feature_select, 0);
        core::ptr::write_volatile(
            &mut (*c).driver_feature,
            (guest_features & 0xFFFF_FFFF) as u32,
        );
        core::ptr::write_volatile(&mut (*c).driver_feature_select, 1);
        core::ptr::write_volatile(&mut (*c).driver_feature, (guest_features >> 32) as u32);

        let mut status = core::ptr::read_volatile(&(*c).device_status);
        status |= VIRTIO_STATUS_FEATURES_OK;
        core::ptr::write_volatile(&mut (*c).device_status, status);

        let readback = core::ptr::read_volatile(&(*c).device_status);
        if (readback & VIRTIO_STATUS_FEATURES_OK) == 0 {
            core::ptr::write_volatile(&mut (*c).device_status, VIRTIO_STATUS_FAILED);
            return false;
        }
    }
    true
}

fn find_device() -> Option<pci::PciDevice> {
    let mut found: Option<pci::PciDevice> = None;
    pci::with_devices(|list| {
        for dev in list {
            if dev.vendor != VIRTIO_PCI_VENDOR {
                continue;
            }
            if dev.device != VIRTIO_GPU_DEVICE_MODERN
                && dev.device != VIRTIO_GPU_DEVICE_TRANSITIONAL
            {
                continue;
            }
            found = Some(*dev);
            break;
        }
    });
    found
}

fn as_bytes<T: Copy>(value: &T) -> &[u8] {
    unsafe {
        core::slice::from_raw_parts((value as *const T) as *const u8, core::mem::size_of::<T>())
    }
}
