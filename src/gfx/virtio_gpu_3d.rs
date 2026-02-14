extern crate alloc;

use alloc::vec::Vec;
use core::sync::atomic::{fence, AtomicBool, AtomicU32, Ordering};

use crate::{pci, wait};

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
const VIRTIO_GPU_CMD_CTX_CREATE: u32 = 0x0200;
const VIRTIO_GPU_CMD_RESOURCE_CREATE_3D: u32 = 0x0201;
const VIRTIO_GPU_CMD_SUBMIT_3D: u32 = 0x0203;
const VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE: u32 = 0x0204;
const VIRTIO_GPU_CMD_CTX_DETACH_RESOURCE: u32 = 0x0205;
const VIRTIO_GPU_CMD_RESOURCE_UNREF: u32 = 0x0102;

const VIRTIO_GPU_RESP_OK_NODATA: u32 = 0x1100;
const VIRTIO_GPU_RESP_OK_DISPLAY_INFO: u32 = 0x1101;

const VIRTIO_GPU_FLAG_FENCE: u32 = 1;
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
    (cmd as u32) | ((obj as u32) << 8) | ((len_dwords as u32) << 16)
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
            core::slice::from_raw_parts(
                self.dwords.as_ptr() as *const u8,
                self.dwords.len() * 4,
            )
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
        cap_vndr: pci::config_read_u8(dev.bus, dev.slot, dev.function, base + 0),
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
        _isr_phys: isr_phys,
        _isr_len: isr.map(|c| c.length).unwrap_or(0),
        _device_phys: device_phys,
        _device_len: device_cfg.map(|c| c.length).unwrap_or(0),
    })
}

fn enable_mem_and_bus_master(dev: &pci::PciDevice) {
    let mut cmd = pci::config_read_u16(dev.bus, dev.slot, dev.function, VIRTIO_PCI_COMMAND_OFFSET);
    cmd |= VIRTIO_PCI_COMMAND_MEM | VIRTIO_PCI_COMMAND_BUS_MASTER;
    pci::config_write_u16(dev.bus, dev.slot, dev.function, VIRTIO_PCI_COMMAND_OFFSET, cmd);
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

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CmdResourceUnref {
    hdr: CtrlHdr,
    resource_id: u32,
    padding: u32,
}

pub struct VirtioGpu3d {
    common: core::ptr::NonNull<VirtioPciCommonCfg>,
    notify: core::ptr::NonNull<u8>,
    notify_mult: u32,
    ctrlq: VirtQueue,
    req: DmaRegion,
    resp: DmaRegion,
}

unsafe impl Send for VirtioGpu3d {}

impl VirtioGpu3d {
    pub fn init_first() -> Option<Self> {
        let dev = find_device()?;
        let caps = parse_modern_caps(&dev)?;
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

    pub fn set_scanout(&mut self, scanout_id: u32, resource_id: u32, width: u32, height: u32) -> bool {
        let req = CmdSetScanout {
            hdr: CtrlHdr {
                type_: VIRTIO_GPU_CMD_SET_SCANOUT,
                flags: 0,
                fence_id: 0,
                ctx_id: 0,
                padding: 0,
            },
            r: Rect { x: 0, y: 0, width, height },
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
            r: Rect { x: 0, y: 0, width, height },
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

    pub fn resource_unref(&mut self, ctx_id: u32, resource_id: u32) -> bool {
        let req = CmdResourceUnref {
            hdr: CtrlHdr {
                type_: VIRTIO_GPU_CMD_RESOURCE_UNREF,
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
            core::ptr::copy_nonoverlapping(header_bytes.as_ptr(), self.req.virt(), header_bytes.len());
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
        notify_queue_modern(self.notify, self.notify_mult, self.ctrlq.queue_index, self.ctrlq.notify_off);

        let ok = wait::spin_until_timeout(1000, || self.ctrlq.used_idx() != self.ctrlq.last_used_idx);
        if !ok {
            crate::log!("virtio-gpu3d: ctrlq timeout\n");
            return false;
        }

        let used = self.ctrlq.used_elem(self.ctrlq.last_used_idx % self.ctrlq.size);
        self.ctrlq.last_used_idx = self.ctrlq.last_used_idx.wrapping_add(1);
        if used.id != 0 {
            crate::log!("virtio-gpu3d: ctrlq used id={} (expected 0)\n", used.id);
            return false;
        }

        let resp_hdr = unsafe { &*(self.resp.virt() as *const CtrlHdr) };
        resp_hdr.type_ == VIRTIO_GPU_RESP_OK_NODATA || resp_hdr.type_ == VIRTIO_GPU_RESP_OK_DISPLAY_INFO
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Vertex {
    pos: [f32; 4],
    color: [f32; 4],
}

const VS_TEXT: &str =
    "VERT\n\
DCL IN[0]\n\
DCL IN[1]\n\
DCL OUT[0], POSITION\n\
DCL OUT[1], COLOR\n\
  0: MOV OUT[1], IN[1]\n\
  1: MOV OUT[0], IN[0]\n\
  2: END\n";

const FS_TEXT: &str =
    "FRAG\n\
DCL IN[0], COLOR, LINEAR\n\
DCL OUT[0], COLOR\n\
  0: MOV OUT[0], IN[0]\n\
  1: END\n";

fn encode_shader(buf: &mut VirglCmdBuf, handle: u32, shader_type: u32, text: &str) {
    // Matches virgl_encode_shader_state() with a provided shad_str: num_tokens is a dummy.
    let mut bytes = Vec::new();
    bytes.extend_from_slice(text.as_bytes());
    bytes.push(0);

    let shader_len = bytes.len() as u32;
    let num_tokens = 300u32;
    let offlen = shader_len & 0x7fff_ffff;

    // Base header size=5 dwords: handle, type, offlen, num_tokens, num_outputs.
    let len_dwords = 5 + ((bytes.len() as u32 + 3) / 4);
    buf.push(virgl_cmd0(VIRGL_CCMD_CREATE_OBJECT, VIRGL_OBJECT_SHADER, len_dwords));
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
    buf.push(virgl_cmd0(VIRGL_CCMD_LINK_SHADER, 0, VIRGL_LINK_SHADER_SIZE));
    buf.push(vs);
    buf.push(fs);
    buf.push(0);
    buf.push(0);
    buf.push(0);
    buf.push(0);
}

fn encode_create_surface(buf: &mut VirglCmdBuf, surf_handle: u32, res_handle: u32, format: u32) {
    buf.push(virgl_cmd0(VIRGL_CCMD_CREATE_OBJECT, VIRGL_OBJECT_SURFACE, VIRGL_OBJ_SURFACE_SIZE));
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
    buf.push(virgl_cmd0(VIRGL_CCMD_CREATE_OBJECT, VIRGL_OBJECT_VERTEX_ELEMENTS, len));
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
    let data_dwords = ((data.len() as u32) + 3) / 4;
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
    buf.push(virgl_cmd0(VIRGL_CCMD_CREATE_OBJECT, VIRGL_OBJECT_BLEND, VIRGL_OBJ_BLEND_SIZE));
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
    buf.push(virgl_cmd0(VIRGL_CCMD_CREATE_OBJECT, VIRGL_OBJECT_DSA, VIRGL_OBJ_DSA_SIZE));
    buf.push(dsa_handle);
    buf.push(0);
    buf.push(0);
    buf.push(0);
}

fn encode_create_rasterizer(buf: &mut VirglCmdBuf, rs_handle: u32) {
    buf.push(virgl_cmd0(VIRGL_CCMD_CREATE_OBJECT, VIRGL_OBJECT_RASTERIZER, VIRGL_OBJ_RS_SIZE));
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

fn encode_resource_copy_region(
    buf: &mut VirglCmdBuf,
    dst_res: u32,
    src_res: u32,
    width: u32,
    height: u32,
) {
    // VIRGL_CMD_RESOURCE_COPY_REGION_SIZE = 13
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

/// Manual bring-up helper: create a virgl context and issue a single DRAW_VBO triangle.
///
/// This is intentionally not wired into the default boot path; call it from a debug hook.
pub fn demo_issue_draw_once() {
    let mut gpu = match VirtioGpu3d::init_first() {
        Some(g) => g,
        None => {
            crate::log!("virgl: virtio-gpu 3d init failed\n");
            return;
        }
    };

    let (scanout, w, h) = match gpu.get_display_info() {
        Some(v) => v,
        None => {
            crate::log!("virgl: display info unavailable\n");
            return;
        }
    };

    let ctx_id = alloc_ctx_id();
    if !gpu.ctx_create(ctx_id) {
        crate::log!("virgl: ctx_create failed\n");
        return;
    }

    // Resources: 2D scanout target + 3D render target + VBO.
    let (scanout_res, rt_res, vbo_res) = alloc_res_triple();

    if !gpu.resource_create_2d(scanout_res, VIRGL_FORMAT_B8G8R8X8_UNORM, w, h) {
        crate::log!("virgl: scanout resource_create_2d failed\n");
        return;
    }

    let rt_bind = PIPE_BIND_RENDER_TARGET | PIPE_BIND_BLENDABLE;
    if !gpu.resource_create_3d(
        ctx_id,
        rt_res,
        PIPE_TEXTURE_2D,
        VIRGL_FORMAT_B8G8R8X8_UNORM,
        rt_bind,
        w,
        h,
        1,
        1,
        0,
        0,
        0,
    ) {
        crate::log!("virgl: rt resource_create_3d failed\n");
        return;
    }

    if !gpu.resource_create_3d(
        ctx_id,
        vbo_res,
        PIPE_BUFFER,
        VIRGL_FORMAT_R8_UNORM,
        PIPE_BIND_VERTEX_BUFFER,
        core::mem::size_of::<[Vertex; 3]>() as u32,
        1,
        1,
        1,
        0,
        0,
        0,
    ) {
        crate::log!("virgl: vbo resource_create_3d failed\n");
        return;
    }

    let _ = gpu.ctx_attach_resource(ctx_id, scanout_res);
    let _ = gpu.ctx_attach_resource(ctx_id, rt_res);
    let _ = gpu.ctx_attach_resource(ctx_id, vbo_res);

    // Present target is the 2D scanout resource.
    if !gpu.set_scanout(scanout, scanout_res, w, h) {
        crate::log!("virgl: scanout not active; aborting\n");
        return;
    }

    // Build and submit virgl command stream.
    let mut cmd = VirglCmdBuf::new();

    // Create render surface and framebuffer.
    let surf_handle = 10u32;
    encode_create_surface(&mut cmd, surf_handle, rt_res, VIRGL_FORMAT_B8G8R8X8_UNORM);
    encode_set_framebuffer(&mut cmd, surf_handle);

    encode_clear_color(&mut cmd, 0.0, 0.1, 0.2, 1.0);

    let ve_handle = 11u32;
    encode_create_vertex_elements(&mut cmd, ve_handle);
    encode_bind_object(&mut cmd, VIRGL_OBJECT_VERTEX_ELEMENTS, ve_handle);

    // Upload vertex data.
    let verts = [
        Vertex {
            pos: [0.0, 0.65, 0.0, 1.0],
            color: [1.0, 0.0, 0.0, 1.0],
        },
        Vertex {
            pos: [-0.7, -0.55, 0.0, 1.0],
            color: [0.0, 1.0, 0.0, 1.0],
        },
        Vertex {
            pos: [0.7, -0.55, 0.0, 1.0],
            color: [0.0, 0.0, 1.0, 1.0],
        },
    ];
    let vert_bytes: &[u8] = unsafe {
        core::slice::from_raw_parts(
            verts.as_ptr() as *const u8,
            core::mem::size_of_val(&verts),
        )
    };
    encode_inline_write_buffer(&mut cmd, vbo_res, vert_bytes);
    encode_set_vertex_buffer(
        &mut cmd,
        core::mem::size_of::<Vertex>() as u32,
        0,
        vbo_res,
    );

    // Shaders.
    let vs_handle = 20u32;
    let fs_handle = 21u32;
    encode_shader(&mut cmd, vs_handle, PIPE_SHADER_VERTEX, VS_TEXT);
    encode_bind_shader(&mut cmd, vs_handle, PIPE_SHADER_VERTEX);
    encode_shader(&mut cmd, fs_handle, PIPE_SHADER_FRAGMENT, FS_TEXT);
    encode_bind_shader(&mut cmd, fs_handle, PIPE_SHADER_FRAGMENT);
    encode_link_shader(&mut cmd, vs_handle, fs_handle);

    // Fixed-ish state.
    let blend_handle = 30u32;
    encode_create_blend(&mut cmd, blend_handle);
    encode_bind_object(&mut cmd, VIRGL_OBJECT_BLEND, blend_handle);
    let dsa_handle = 31u32;
    encode_create_dsa(&mut cmd, dsa_handle);
    encode_bind_object(&mut cmd, VIRGL_OBJECT_DSA, dsa_handle);
    let rs_handle = 32u32;
    encode_create_rasterizer(&mut cmd, rs_handle);
    encode_bind_object(&mut cmd, VIRGL_OBJECT_RASTERIZER, rs_handle);

    encode_set_viewport(&mut cmd, w, h);
    encode_draw_vbo(&mut cmd);
    // Present: copy rendered texture into scanout resource.
    encode_resource_copy_region(&mut cmd, scanout_res, rt_res, w, h);

    if !gpu.submit_3d(ctx_id, cmd.as_bytes(), 1) {
        crate::log!("virgl: submit_3d failed\n");
        return;
    }

    let _ = gpu.resource_flush(scanout_res, w, h);
    crate::log!("virgl: issued DRAW_VBO triangle (w={} h={})\n", w, h);
}

/// Manual bring-up helper: render a rotating triangle at ~60 Hz.
///
/// Rotation is clockwise, 360 degrees every 3 seconds.
pub fn demo_spin_triangle_60hz() {
    let mut gpu = match VirtioGpu3d::init_first() {
        Some(g) => g,
        None => {
            crate::log!("virgl: virtio-gpu 3d init failed\n");
            return;
        }
    };

    let (scanout, w, h) = match gpu.get_display_info() {
        Some(v) => v,
        None => {
            crate::log!("virgl: display info unavailable\n");
            return;
        }
    };

    let ctx_id = alloc_ctx_id();
    if !gpu.ctx_create(ctx_id) {
        crate::log!("virgl: ctx_create failed\n");
        return;
    }

    // Resources: 2D scanout target + 3D render target + VBO.
    let (scanout_res, rt_res, vbo_res) = alloc_res_triple();

    if !gpu.resource_create_2d(scanout_res, VIRGL_FORMAT_B8G8R8X8_UNORM, w, h) {
        crate::log!("virgl: scanout resource_create_2d failed\n");
        return;
    }

    let rt_bind = PIPE_BIND_RENDER_TARGET | PIPE_BIND_BLENDABLE;
    if !gpu.resource_create_3d(
        ctx_id,
        rt_res,
        PIPE_TEXTURE_2D,
        VIRGL_FORMAT_B8G8R8X8_UNORM,
        rt_bind,
        w,
        h,
        1,
        1,
        0,
        0,
        0,
    ) {
        crate::log!("virgl: rt resource_create_3d failed\n");
        return;
    }

    if !gpu.resource_create_3d(
        ctx_id,
        vbo_res,
        PIPE_BUFFER,
        VIRGL_FORMAT_R8_UNORM,
        PIPE_BIND_VERTEX_BUFFER,
        core::mem::size_of::<[Vertex; 3]>() as u32,
        1,
        1,
        1,
        0,
        0,
        0,
    ) {
        crate::log!("virgl: vbo resource_create_3d failed\n");
        return;
    }

    let _ = gpu.ctx_attach_resource(ctx_id, scanout_res);
    let _ = gpu.ctx_attach_resource(ctx_id, rt_res);
    let _ = gpu.ctx_attach_resource(ctx_id, vbo_res);
    if !gpu.set_scanout(scanout, scanout_res, w, h) {
        crate::log!("virgl: scanout not active; aborting spin\n");
        return;
    }

    // One-time state/program setup.
    let mut init = VirglCmdBuf::new();
    let surf_handle = 10u32;
    encode_create_surface(&mut init, surf_handle, rt_res, VIRGL_FORMAT_B8G8R8X8_UNORM);
    encode_set_framebuffer(&mut init, surf_handle);

    let ve_handle = 11u32;
    encode_create_vertex_elements(&mut init, ve_handle);
    encode_bind_object(&mut init, VIRGL_OBJECT_VERTEX_ELEMENTS, ve_handle);

    encode_set_vertex_buffer(&mut init, core::mem::size_of::<Vertex>() as u32, 0, vbo_res);

    let vs_handle = 20u32;
    let fs_handle = 21u32;
    encode_shader(&mut init, vs_handle, PIPE_SHADER_VERTEX, VS_TEXT);
    encode_bind_shader(&mut init, vs_handle, PIPE_SHADER_VERTEX);
    encode_shader(&mut init, fs_handle, PIPE_SHADER_FRAGMENT, FS_TEXT);
    encode_bind_shader(&mut init, fs_handle, PIPE_SHADER_FRAGMENT);
    encode_link_shader(&mut init, vs_handle, fs_handle);

    let blend_handle = 30u32;
    encode_create_blend(&mut init, blend_handle);
    encode_bind_object(&mut init, VIRGL_OBJECT_BLEND, blend_handle);
    let dsa_handle = 31u32;
    encode_create_dsa(&mut init, dsa_handle);
    encode_bind_object(&mut init, VIRGL_OBJECT_DSA, dsa_handle);
    let rs_handle = 32u32;
    encode_create_rasterizer(&mut init, rs_handle);
    encode_bind_object(&mut init, VIRGL_OBJECT_RASTERIZER, rs_handle);

    encode_set_viewport(&mut init, w, h);

    if !gpu.submit_3d(ctx_id, init.as_bytes(), 1) {
        crate::log!("virgl: submit_3d init failed\n");
        return;
    }

    // 60 Hz frame pacing based on embassy ticks (typically 1kHz): schedule frame N at
    // start + floor(N * TICK_HZ / 60), which yields an exact long-term average.
    let start_ticks = embassy_time_driver::now();
    let tick_hz = embassy_time_driver::TICK_HZ as u64;
    let omega = (2.0 * core::f32::consts::PI) / 3.0; // rad/s

    crate::log!("virgl: spinning triangle @60Hz (cw, 360deg/3s)\n");
    let mut frame: u64 = 0;
    loop {
        frame = frame.wrapping_add(1);
        let target = start_ticks.saturating_add(frame.saturating_mul(tick_hz) / 60);
        while embassy_time_driver::now() < target {
            core::hint::spin_loop();
        }

        let t = (frame as f32) * (1.0 / 60.0);
        let angle = -omega * t;
        let c = libm::cosf(angle);
        let s = libm::sinf(angle);

        let base = [
            (0.0f32, 0.65f32),
            (-0.7f32, -0.55f32),
            (0.7f32, -0.55f32),
        ];
        let colors = [
            [1.0f32, 0.0f32, 0.0f32, 1.0f32],
            [0.0f32, 1.0f32, 0.0f32, 1.0f32],
            [0.0f32, 0.0f32, 1.0f32, 1.0f32],
        ];

        let mut verts = [
            Vertex {
                pos: [0.0, 0.0, 0.0, 1.0],
                color: colors[0],
            },
            Vertex {
                pos: [0.0, 0.0, 0.0, 1.0],
                color: colors[1],
            },
            Vertex {
                pos: [0.0, 0.0, 0.0, 1.0],
                color: colors[2],
            },
        ];

        for (i, (x, y)) in base.iter().copied().enumerate() {
            let xr = x * c - y * s;
            let yr = x * s + y * c;
            verts[i].pos = [xr, yr, 0.0, 1.0];
        }

        let vert_bytes: &[u8] = unsafe {
            core::slice::from_raw_parts(
                verts.as_ptr() as *const u8,
                core::mem::size_of_val(&verts),
            )
        };

        let mut cmd = VirglCmdBuf::new();
        encode_clear_color(&mut cmd, 0.0, 0.1, 0.2, 1.0);
        encode_inline_write_buffer(&mut cmd, vbo_res, vert_bytes);
        encode_draw_vbo(&mut cmd);
        encode_resource_copy_region(&mut cmd, scanout_res, rt_res, w, h);

        // Keep fence ids monotonic so the device/host can track progress.
        if !gpu.submit_3d(ctx_id, cmd.as_bytes(), frame.wrapping_add(1)) {
            crate::log!("virgl: submit_3d failed\n");
            return;
        }
        let _ = gpu.resource_flush(scanout_res, w, h);
    }
}

static VIRGL_SPIN_RUNNING: AtomicBool = AtomicBool::new(false);
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

/// Spawn the rotating virgl triangle as a background task.
///
/// This avoids freezing the BSP executor (shell command handlers are synchronous).
pub fn spawn_spin_triangle_60hz(spawner: &embassy_executor::Spawner) {
    if VIRGL_SPIN_RUNNING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        crate::log!("virgl: spin already running\n");
        return;
    }

    // Prefer AP1 so BSP remains responsive.
    if let Some(ap) = crate::runtime::first_ap_spawner() {
        if ap.spawn(virgl_spin_task()).is_ok() {
            crate::log!("virgl: spin task spawned on AP1\n");
            return;
        }
        crate::log!("virgl: failed to spawn on AP1, falling back to BSP\n");
    }

    if spawner.spawn(virgl_spin_task()).is_err() {
        VIRGL_SPIN_RUNNING.store(false, Ordering::Release);
        crate::log!("virgl: spin task spawn failed\n");
    } else {
        crate::log!("virgl: spin task spawned on BSP\n");
    }
}

#[embassy_executor::task]
async fn virgl_spin_task() {
    use embassy_time::{Duration as EmbassyDuration, Timer};

    // If anything fails during init, allow retry.
    let mut gpu = match VirtioGpu3d::init_first() {
        Some(g) => g,
        None => {
            crate::log!("virgl: virtio-gpu 3d init failed\n");
            VIRGL_SPIN_RUNNING.store(false, Ordering::Release);
            return;
        }
    };

    let (scanout, w, h) = match gpu.get_display_info() {
        Some(v) => v,
        None => {
            crate::log!("virgl: display info unavailable\n");
            VIRGL_SPIN_RUNNING.store(false, Ordering::Release);
            return;
        }
    };

    let ctx_id = alloc_ctx_id();
    if !gpu.ctx_create(ctx_id) {
        crate::log!("virgl: ctx_create failed\n");
        VIRGL_SPIN_RUNNING.store(false, Ordering::Release);
        return;
    }

    let (scanout_res, rt_res, vbo_res) = alloc_res_triple();

    if !gpu.resource_create_2d(scanout_res, VIRGL_FORMAT_B8G8R8X8_UNORM, w, h) {
        crate::log!("virgl: scanout resource_create_2d failed\n");
        VIRGL_SPIN_RUNNING.store(false, Ordering::Release);
        return;
    }

    let rt_bind = PIPE_BIND_RENDER_TARGET | PIPE_BIND_BLENDABLE;
    if !gpu.resource_create_3d(
        ctx_id,
        rt_res,
        PIPE_TEXTURE_2D,
        VIRGL_FORMAT_B8G8R8X8_UNORM,
        rt_bind,
        w,
        h,
        1,
        1,
        0,
        0,
        0,
    ) {
        crate::log!("virgl: rt resource_create_3d failed\n");
        VIRGL_SPIN_RUNNING.store(false, Ordering::Release);
        return;
    }

    if !gpu.resource_create_3d(
        ctx_id,
        vbo_res,
        PIPE_BUFFER,
        VIRGL_FORMAT_R8_UNORM,
        PIPE_BIND_VERTEX_BUFFER,
        core::mem::size_of::<[Vertex; 3]>() as u32,
        1,
        1,
        1,
        0,
        0,
        0,
    ) {
        crate::log!("virgl: vbo resource_create_3d failed\n");
        VIRGL_SPIN_RUNNING.store(false, Ordering::Release);
        return;
    }

    let _ = gpu.ctx_attach_resource(ctx_id, scanout_res);
    let _ = gpu.ctx_attach_resource(ctx_id, rt_res);
    let _ = gpu.ctx_attach_resource(ctx_id, vbo_res);
    if !gpu.set_scanout(scanout, scanout_res, w, h) {
        crate::log!("virgl: scanout not active; stopping spin\n");
        VIRGL_SPIN_RUNNING.store(false, Ordering::Release);
        return;
    }

    // One-time state/program setup.
    let mut init = VirglCmdBuf::new();
    let surf_handle = 10u32;
    encode_create_surface(&mut init, surf_handle, rt_res, VIRGL_FORMAT_B8G8R8X8_UNORM);
    encode_set_framebuffer(&mut init, surf_handle);

    let ve_handle = 11u32;
    encode_create_vertex_elements(&mut init, ve_handle);
    encode_bind_object(&mut init, VIRGL_OBJECT_VERTEX_ELEMENTS, ve_handle);

    encode_set_vertex_buffer(&mut init, core::mem::size_of::<Vertex>() as u32, 0, vbo_res);

    let vs_handle = 20u32;
    let fs_handle = 21u32;
    encode_shader(&mut init, vs_handle, PIPE_SHADER_VERTEX, VS_TEXT);
    encode_bind_shader(&mut init, vs_handle, PIPE_SHADER_VERTEX);
    encode_shader(&mut init, fs_handle, PIPE_SHADER_FRAGMENT, FS_TEXT);
    encode_bind_shader(&mut init, fs_handle, PIPE_SHADER_FRAGMENT);
    encode_link_shader(&mut init, vs_handle, fs_handle);

    let blend_handle = 30u32;
    encode_create_blend(&mut init, blend_handle);
    encode_bind_object(&mut init, VIRGL_OBJECT_BLEND, blend_handle);
    let dsa_handle = 31u32;
    encode_create_dsa(&mut init, dsa_handle);
    encode_bind_object(&mut init, VIRGL_OBJECT_DSA, dsa_handle);
    let rs_handle = 32u32;
    encode_create_rasterizer(&mut init, rs_handle);
    encode_bind_object(&mut init, VIRGL_OBJECT_RASTERIZER, rs_handle);

    encode_set_viewport(&mut init, w, h);

    if !gpu.submit_3d(ctx_id, init.as_bytes(), 1) {
        crate::log!("virgl: submit_3d init failed\n");
        VIRGL_SPIN_RUNNING.store(false, Ordering::Release);
        return;
    }

    // 60 Hz frame pacing based on embassy ticks (typically 1kHz): schedule frame N at
    // start + floor(N * TICK_HZ / 60), which yields an exact long-term average.
    let start_ticks = embassy_time_driver::now();
    let tick_hz = embassy_time_driver::TICK_HZ as u64;
    let omega = (2.0 * core::f32::consts::PI) / 3.0; // rad/s

    crate::log!("virgl: spinning triangle @60Hz (cw, 360deg/3s)\n");
    let mut frame: u64 = 0;
    loop {
        frame = frame.wrapping_add(1);
        let target = start_ticks.saturating_add(frame.saturating_mul(tick_hz) / 60);
        while embassy_time_driver::now() < target {
            // Yield to keep the executor responsive.
            Timer::after(EmbassyDuration::from_millis(1)).await;
        }

        let t = (frame as f32) * (1.0 / 60.0);
        let angle = -omega * t;
        let c = libm::cosf(angle);
        let s = libm::sinf(angle);

        let base = [(0.0f32, 0.65f32), (-0.7f32, -0.55f32), (0.7f32, -0.55f32)];
        let colors = [
            [1.0f32, 0.0f32, 0.0f32, 1.0f32],
            [0.0f32, 1.0f32, 0.0f32, 1.0f32],
            [0.0f32, 0.0f32, 1.0f32, 1.0f32],
        ];

        let mut verts = [
            Vertex {
                pos: [0.0, 0.0, 0.0, 1.0],
                color: colors[0],
            },
            Vertex {
                pos: [0.0, 0.0, 0.0, 1.0],
                color: colors[1],
            },
            Vertex {
                pos: [0.0, 0.0, 0.0, 1.0],
                color: colors[2],
            },
        ];

        for (i, (x, y)) in base.iter().copied().enumerate() {
            let xr = x * c - y * s;
            let yr = x * s + y * c;
            verts[i].pos = [xr, yr, 0.0, 1.0];
        }

        let vert_bytes: &[u8] = unsafe {
            core::slice::from_raw_parts(
                verts.as_ptr() as *const u8,
                core::mem::size_of_val(&verts),
            )
        };

        let mut cmd = VirglCmdBuf::new();
        encode_clear_color(&mut cmd, 0.0, 0.1, 0.2, 1.0);
        encode_inline_write_buffer(&mut cmd, vbo_res, vert_bytes);
        encode_draw_vbo(&mut cmd);
        encode_resource_copy_region(&mut cmd, scanout_res, rt_res, w, h);

        if !gpu.submit_3d(ctx_id, cmd.as_bytes(), frame.wrapping_add(1)) {
            crate::log!("virgl: submit_3d failed\n");
            break;
        }
        let _ = gpu.resource_flush(scanout_res, w, h);

        if frame % 60 == 0 {
            crate::log!("virgl: spin alive frame={}\n", frame);
        }
    }
}

fn modern_negotiate_minimal(common: core::ptr::NonNull<VirtioPciCommonCfg>) -> bool {
    unsafe {
        let c = common.as_ptr();
        core::ptr::write_volatile(&mut (*c).device_status, 0);
        core::ptr::write_volatile(&mut (*c).device_status, VIRTIO_STATUS_ACK | VIRTIO_STATUS_DRIVER);

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
        core::ptr::write_volatile(&mut (*c).driver_feature, (guest_features & 0xFFFF_FFFF) as u32);
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
            if dev.device != VIRTIO_GPU_DEVICE_MODERN && dev.device != VIRTIO_GPU_DEVICE_TRANSITIONAL {
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
        core::slice::from_raw_parts(
            (value as *const T) as *const u8,
            core::mem::size_of::<T>(),
        )
    }
}
