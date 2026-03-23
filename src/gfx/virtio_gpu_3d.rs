extern crate alloc;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU8, AtomicU32, Ordering, fence};

use crate::{pci, wait};
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
const VIRTIO_GPU_CMD_GET_EDID: u32 = 0x010A;
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
const CTRL_RESP_OK_NODATA: [u32; 1] = [VIRTIO_GPU_RESP_OK_NODATA];
const CTRL_RESP_OK_QUERY: [u32; 3] = [
    VIRTIO_GPU_RESP_OK_NODATA,
    VIRTIO_GPU_RESP_OK_DISPLAY_INFO,
    VIRTIO_GPU_RESP_OK_EDID,
];

// Control queue request buffer capacity for inline submit_3d payloads.
// Keep this modest but above typical UI frame bursts.
const SUBMIT_3D_REQ_CAP_BYTES: usize = 32 * 1024 * 1024; // 32 MiB
const VIRTIO_GPU_RESP_OK_EDID: u32 = 0x1104;
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
// Gallium bind flag used for textures sampled by a shader.
const PIPE_BIND_SAMPLER_VIEW: u32 = 1 << 3;
const PIPE_BIND_VERTEX_BUFFER: u32 = 1 << 4;
const PIPE_BIND_DISPLAY_TARGET: u32 = 1 << 8;
const PIPE_BIND_SCANOUT: u32 = 1 << 14;

// Virgl format IDs (see virgl_hw.h):
const VIRGL_FORMAT_B8G8R8A8_UNORM: u32 = 1;
const VIRGL_FORMAT_B8G8R8X8_UNORM: u32 = 2;
// virgl_hw.h: VIRGL_FORMAT_R8G8B8A8_UNORM = 67
const VIRGL_FORMAT_R8G8B8A8_UNORM: u32 = 67;
const VIRGL_FORMAT_R8_UNORM: u32 = 64;
const VIRGL_FORMAT_R32G32B32A32_FLOAT: u32 = 31;

// --- Virgl protocol (see virgl_protocol.h) ---
const VIRGL_OBJECT_BLEND: u8 = 1;
const VIRGL_OBJECT_RASTERIZER: u8 = 2;
const VIRGL_OBJECT_DSA: u8 = 3;
const VIRGL_OBJECT_SHADER: u8 = 4;
const VIRGL_OBJECT_VERTEX_ELEMENTS: u8 = 5;
// From virgl_protocol.h enum virgl_object_type.
const VIRGL_OBJECT_SAMPLER_VIEW: u8 = 6;
const VIRGL_OBJECT_SAMPLER_STATE: u8 = 7;
const VIRGL_OBJECT_SURFACE: u8 = 8;

const VIRGL_CCMD_CREATE_OBJECT: u8 = 1;
const VIRGL_CCMD_BIND_OBJECT: u8 = 2;
const VIRGL_CCMD_SET_VIEWPORT_STATE: u8 = 4;
const VIRGL_CCMD_SET_FRAMEBUFFER_STATE: u8 = 5;
const VIRGL_CCMD_SET_VERTEX_BUFFERS: u8 = 6;
const VIRGL_CCMD_CLEAR: u8 = 7;
const VIRGL_CCMD_DRAW_VBO: u8 = 8;
const VIRGL_CCMD_RESOURCE_INLINE_WRITE: u8 = 9;
const VIRGL_CCMD_SET_SAMPLER_VIEWS: u8 = 10;
const VIRGL_CCMD_SET_SCISSOR_STATE: u8 = 15;
const VIRGL_CCMD_RESOURCE_COPY_REGION: u8 = 17;
const VIRGL_CCMD_BIND_SAMPLER_STATES: u8 = 18;
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
            && let Some(vcap) = read_virtio_pci_cap(dev, ptr)
        {
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

const EDID_MAX_BYTES: usize = 1024;

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
#[derive(Clone, Copy)]
struct CmdGetEdid {
    hdr: CtrlHdr,
    scanout_id: u32,
    padding: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct RespEdid {
    hdr: CtrlHdr,
    size: u32,
    padding: u32,
    edid: [u8; EDID_MAX_BYTES],
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

        // The control request DMA buffer must hold the entire VIRTIO_GPU_CMD_SUBMIT_3D
        // payload (header + virgl command stream). Large UI/text texture uploads can
        // exceed 256KiB, so keep a 1MiB request buffer to avoid immediate overflows.
        let req = DmaRegion::alloc(SUBMIT_3D_REQ_CAP_BYTES, 16)?;
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
        let mut first_mode: Option<(u32, u32, u32, u32, u32)> = None;
        for (i, m) in info.pmodes.iter().enumerate() {
            if first_mode.is_none() && m.r.width != 0 && m.r.height != 0 {
                first_mode = Some((i as u32, m.r.width, m.r.height, m.enabled, m.flags));
            }
            if m.enabled != 0 && m.r.width != 0 && m.r.height != 0 {
                return Some((i as u32, m.r.width, m.r.height));
            }
        }
        if let Some((i, w, h, enabled, flags)) = first_mode {
            crate::log!(
                "virgl: get_display_info no enabled scanout (first mode id={} {}x{} enabled={} flags=0x{:08X})\n",
                i,
                w,
                h,
                enabled,
                flags
            );
        } else {
            crate::log!("virgl: get_display_info no scanout modes reported\n");
        }
        None
    }

    pub fn get_edid(&mut self, scanout_id: u32, out: &mut [u8]) -> Option<usize> {
        if out.is_empty() {
            return Some(0);
        }

        let req = CmdGetEdid {
            hdr: CtrlHdr {
                type_: VIRTIO_GPU_CMD_GET_EDID,
                flags: 0,
                fence_id: 0,
                ctx_id: 0,
                padding: 0,
            },
            scanout_id,
            padding: 0,
        };

        let resp_type = self.ctrl_submit_bytes_ret_type(as_bytes(&req))?;
        if resp_type != VIRTIO_GPU_RESP_OK_EDID {
            return None;
        }

        let resp = unsafe { &*(self.resp.virt() as *const RespEdid) };
        let n = (resp.size as usize).min(EDID_MAX_BYTES).min(out.len());

        unsafe {
            core::ptr::copy_nonoverlapping(resp.edid.as_ptr(), out.as_mut_ptr(), n);
        }
        Some(n)
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
        if total == 0 {
            crate::log!("virtio-gpu3d: submit_3d empty cmd stream\n");
            return false;
        }
        if total > self.req.len() {
            crate::log!(
                "virtio-gpu3d: submit_3d overflow total={} req_cap={}\n",
                total,
                self.req.len()
            );
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

        if !Self::submit_desc_chain_on(
            self.notify,
            self.notify_mult,
            &mut self.ctrlq,
            &self.req,
            &self.resp,
            req_bytes.len(),
            "ctrlq",
            Some(&CTRL_RESP_OK_QUERY),
        ) {
            return None;
        }
        let resp_hdr = unsafe { &*(self.resp.virt() as *const CtrlHdr) };
        Some(resp_hdr.type_)
    }

    fn ctrl_submit_desc_chain(&mut self, req_len: usize) -> bool {
        Self::submit_desc_chain_on(
            self.notify,
            self.notify_mult,
            &mut self.ctrlq,
            &self.req,
            &self.resp,
            req_len,
            "ctrlq",
            Some(&CTRL_RESP_OK_NODATA),
        )
    }

    fn submit_desc_chain_on(
        notify: core::ptr::NonNull<u8>,
        notify_mult: u32,
        queue: &mut VirtQueue,
        req: &DmaRegion,
        resp: &DmaRegion,
        req_len: usize,
        queue_label: &str,
        expected_resp_types: Option<&[u32]>,
    ) -> bool {
        // Fixed descriptor pair (0 -> req, 1 -> resp). Single outstanding.
        unsafe {
            let d0 = &mut *queue.desc.add(0);
            d0.addr = req.phys();
            d0.len = req_len as u32;
            d0.flags = VIRTQ_DESC_F_NEXT;
            d0.next = 1;

            let d1 = &mut *queue.desc.add(1);
            d1.addr = resp.phys();
            d1.len = resp.len() as u32;
            d1.flags = VIRTQ_DESC_F_WRITE;
            d1.next = 0;
        }

        queue.push_avail(0);
        fence(Ordering::Release);
        notify_queue_modern(notify, notify_mult, queue.queue_index, queue.notify_off);

        // IMPORTANT: do not poll the executor while waiting for ctrlq progress.
        // This function is often called under the global virtio-gpu mutex, and can also be
        // reached while higher-level gfx locks are held. Polling the executor here can re-enter
        // the shell/gfx stack and wedge on those locks (observed as `gfx: SYSTEM lock timeout`).
        let ok = wait::spin_until_timeout_no_exec(1000, || queue.used_idx() != queue.last_used_idx);
        if !ok {
            crate::log!("virtio-gpu3d: {} timeout\n", queue_label);
            return false;
        }

        // Ensure host DMA writes for used/resp are visible before we read them.
        fence(Ordering::Acquire);

        let used = queue.used_elem(queue.last_used_idx % queue.size);
        queue.last_used_idx = queue.last_used_idx.wrapping_add(1);

        if used.id != 0 {
            crate::log!(
                "virtio-gpu3d: {} used id={} (expected 0)\n",
                queue_label,
                used.id
            );
            return false;
        }

        let Some(expected) = expected_resp_types else {
            return true;
        };

        // Read response type directly from DMA memory without forming a typed reference.
        // This avoids alignment-sensitive UB on platforms/allocators that do not guarantee
        // `CtrlHdr` alignment for the response ring buffer mapping.
        let resp_type = unsafe { core::ptr::read_unaligned(resp.virt() as *const u32) };
        let ok = expected.contains(&resp_type);
        if !ok {
            let req_type = unsafe { *(req.virt() as *const u32) };
            crate::log!(
                "virtio-gpu3d: {} bad resp req=0x{:08X} type=0x{:08X} req_len={}\n",
                queue_label,
                req_type,
                resp_type,
                req_len
            );
        }
        ok
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Vertex {
    pos: [f32; 4],
    uv: [f32; 4],
    color: [f32; 4],
}

// Untextured pipeline (pos + color).
const VS_COLOR: &str = "VERT\n\
DCL IN[0]\n\
DCL IN[2]\n\
DCL OUT[0], POSITION\n\
DCL OUT[1], COLOR\n\
    0: MOV OUT[1], IN[2]\n\
    1: MOV OUT[0], IN[0]\n\
    2: END\n";

const FS_COLOR: &str = "FRAG\n\
DCL IN[0], COLOR, LINEAR\n\
DCL OUT[0], COLOR\n\
    0: MOV OUT[0], IN[0]\n\
    1: END\n";

// Bring-up toggles for deterministic texture diagnostics.
const VIRGL_DEBUG_TEXTURE_ID_RAW: u32 = 0x00D3_B600;
const VIRGL_FORCE_DEBUG_TEXTURE: bool = false;
// Correctness-first switch: force per-draw vertex conversion/upload and bypass
// converted vertex cache reuse until multi-draw cache keys are proven robust.
const VIRGL_DISABLE_CONVERTED_VERTEX_CACHE: bool = true;
const VIRGL_DEBUG_TEXTURE_RGBA_2X2: [u8; 16] = [
    255, 0, 0, 255, // red
    0, 255, 0, 255, // green
    0, 0, 255, 255, // blue
    255, 255, 255, 255, // white
];

// Textured pipeline (pos + uv + color).
const VS_TEX: &str = "VERT\n\
DCL IN[0]\n\
DCL IN[1]\n\
DCL IN[2]\n\
DCL OUT[0], POSITION\n\
DCL OUT[1], TEXCOORD[0]\n\
DCL OUT[2], COLOR\n\
    0: MOV OUT[2], IN[2]\n\
    1: MOV OUT[1], IN[1]\n\
    2: MOV OUT[0], IN[0]\n\
    3: END\n";

const FS_TEX_MASK: &str = "FRAG\n\
DCL IN[0], TEXCOORD[0], LINEAR\n\
DCL IN[1], COLOR, LINEAR\n\
DCL SAMP[0]\n\
DCL TEMP[0]\n\
DCL TEMP[1]\n\
DCL OUT[0], COLOR\n\
    0: TEX TEMP[0], IN[0], SAMP[0], 2D\n\
    1: MIN TEMP[1], TEMP[0].xxxx, TEMP[0].wwww\n\
    2: MUL OUT[0].xyz, IN[1], TEMP[1].xxxx\n\
    3: MUL OUT[0].w, IN[1].wwww, TEMP[1].xxxx\n\
    4: END\n";

const FS_TEX_RGBA: &str = "FRAG\n\
DCL IN[0], TEXCOORD[0], LINEAR\n\
DCL IN[1], COLOR, LINEAR\n\
DCL SAMP[0]\n\
DCL TEMP[0]\n\
DCL OUT[0], COLOR\n\
    0: TEX TEMP[0], IN[0], SAMP[0], 2D\n\
    1: MUL OUT[0], TEMP[0], IN[1]\n\
    2: END\n";

const TEX_PIPELINE_FS_MASK_TAG_RAW: u32 = 0x4D41_534B;
const TEX_PIPELINE_FS_RGBA_TAG_RAW: u32 = 0x5247_4241;

// --- gfx-core backend (virgl) ---

use trueos_gfx_core::{
    BlendDesc, BlendFactor, BufferDesc, BufferId, ColorFormat, Command, CommandBuffer, DeviceCaps,
    Error, FenceId, GfxDevice, GfxPresent, ImageDesc, ImageFormat, ImageId, MapMode, MappedRange,
    MemoryType, PipelineDesc, PipelineId, SamplerDesc, ScissorRect, ShaderDesc, ShaderId,
    SwapchainDesc, TexCoordFormat, VertexLayout, Viewport,
};

// NOTE: Do not import `trueos_gfx_core::Result` as `Result` at module scope.
// This file already uses `Result<T, ()>` in virtio setup code.
use trueos_gfx_core::Result as GfxResult;

#[inline]
fn vertex_blob_bounds(blob: &[u8]) -> Option<(f32, f32, f32, f32, f32, f32, f32, f32, f32, f32)> {
    let stride = core::mem::size_of::<Vertex>();
    if stride == 0 || blob.len() < stride {
        return None;
    }
    let mut min_x = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    let mut min_a = f32::INFINITY;
    let mut max_a = f32::NEG_INFINITY;
    let mut min_u = f32::INFINITY;
    let mut max_u = f32::NEG_INFINITY;
    let mut min_v = f32::INFINITY;
    let mut max_v = f32::NEG_INFINITY;
    let mut i = 0usize;
    while i + stride <= blob.len() {
        let px = i;
        let py = i + 4;
        let pu = i + 16;
        let pv = i + 20;
        let pa = i + 44;
        let x = f32::from_le_bytes(blob[px..px + 4].try_into().ok()?);
        let y = f32::from_le_bytes(blob[py..py + 4].try_into().ok()?);
        let u = f32::from_le_bytes(blob[pu..pu + 4].try_into().ok()?);
        let v = f32::from_le_bytes(blob[pv..pv + 4].try_into().ok()?);
        let a = f32::from_le_bytes(blob[pa..pa + 4].try_into().ok()?);
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
        min_u = min_u.min(u);
        max_u = max_u.max(u);
        min_v = min_v.min(v);
        max_v = max_v.max(v);
        min_a = min_a.min(a);
        max_a = max_a.max(a);
        i += stride;
    }
    Some((
        min_x, max_x, min_y, max_y, min_a, max_a, min_u, max_u, min_v, max_v,
    ))
}

#[derive(Clone)]
struct HostBuffer {
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

    // virgl-side handles for real GPU texturing.
    virgl_res: u32,
    virgl_view: u32,
    virgl_uploaded_rev: u32,
    virgl_view_created: bool,
    virgl_surface: u32,
    virgl_surface_created: bool,
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
    render_target: ImageId,
    sampler: SamplerDesc,
    blend: BlendDesc,
    viewport: Viewport,
    scissor: Option<ScissorRect>,
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
            render_target: ImageId::invalid(),
            // WebGL defaults are LINEAR/LINEAR. This also makes stretched
            // low-res textures (like the 2x1 background gradient) look correct.
            sampler: SamplerDesc {
                wrap_s: trueos_gfx_core::SamplerWrap::ClampToEdge,
                wrap_t: trueos_gfx_core::SamplerWrap::ClampToEdge,
                min_filter: trueos_gfx_core::SamplerFilter::Linear,
                mag_filter: trueos_gfx_core::SamplerFilter::Linear,
            },
            blend: BlendDesc::disabled(),
            viewport: Viewport {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            scissor: None,
            clear_rgb: 0x00FF_FFFF,
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
    // Keep backing ownership alive for the lifetime of the backend.
    scanout_backing: DmaRegion,
    rt_res: u32,
    rt_surf_handle: u32,

    // VBO resource for virgl vertices.
    vbo_res: u32,
    vbo_cap_bytes: u32,

    // One-time virgl state handles.
    vs_color_handle: u32,
    fs_color_handle: u32,
    vs_tex_handle: u32,
    fs_tex_mask_handle: u32,
    fs_tex_rgba_handle: u32,
    fs_mandelbrot_handle: u32,
    sampler_state_nearest_handle: u32,
    sampler_state_linear_handle: u32,
    debug_tex_res: u32,
    debug_tex_view: u32,
    debug_tex_uploaded: bool,
    debug_tex_view_created: bool,
    blend_handle_disabled: u32,
    blend_handle_straight: u32,
    blend_handle_additive: u32,
    blend_handle_multiply: u32,
    blend_handle_screen: u32,
    blend_handle_premult: u32,

    swapchain: SwapchainDesc,

    refresh_millihz: Option<u32>,

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
    frame_counter: u64,
}

const VIRGL_SCANOUT_REASSERT_EARLY_FRAMES: u64 = 16;
const VIRGL_SCANOUT_REASSERT_PERIOD_FRAMES: u64 = 120;

fn edid_preferred_refresh_millihz(edid: &[u8]) -> Option<u32> {
    // EDID base block is 128 bytes. We only parse the first block.
    if edid.len() < 128 {
        return None;
    }
    const HDR: [u8; 8] = [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00];
    if edid[..8] != HDR {
        return None;
    }

    // Preferred timing is typically the first Detailed Timing Descriptor at offset 54.
    // Each DTD is 18 bytes; skip entries with pixel clock = 0 (monitor descriptor).
    for i in 0..4usize {
        let off = 54 + i * 18;
        let b = &edid[off..off + 18];
        let pclk_10khz = u16::from_le_bytes([b[0], b[1]]) as u64;
        if pclk_10khz == 0 {
            continue;
        }

        let h_active = (b[2] as u64) | (((b[4] as u64) & 0xF0) << 4);
        let h_blank = (b[3] as u64) | (((b[4] as u64) & 0x0F) << 8);
        let v_active = (b[5] as u64) | (((b[7] as u64) & 0xF0) << 4);
        let v_blank = (b[6] as u64) | (((b[7] as u64) & 0x0F) << 8);

        let h_total = h_active.saturating_add(h_blank);
        let v_total = v_active.saturating_add(v_blank);
        if h_total == 0 || v_total == 0 {
            continue;
        }

        let pixel_clock_hz = pclk_10khz.saturating_mul(10_000);
        let denom = h_total.saturating_mul(v_total);
        if denom == 0 {
            continue;
        }

        let mhz = pixel_clock_hz.saturating_mul(1000) / denom;
        return Some(mhz.min(u32::MAX as u64) as u32);
    }

    None
}

impl VirglGfxBackend {
    #[inline]
    fn should_reassert_scanout(&self, frame_no: u64) -> bool {
        frame_no <= VIRGL_SCANOUT_REASSERT_EARLY_FRAMES
            || frame_no.is_multiple_of(VIRGL_SCANOUT_REASSERT_PERIOD_FRAMES)
    }

    fn resolved_viewport_extent(&self, target: ImageId, vp: Viewport) -> (u32, u32) {
        let (target_w, target_h) = self.render_target_extent(target);
        let mut vp_w = vp.width.max(0) as u32;
        let mut vp_h = vp.height.max(0) as u32;
        if vp_w == 0 {
            vp_w = target_w;
        } else {
            vp_w = vp_w.min(target_w);
        }
        if vp_h == 0 {
            vp_h = target_h;
        } else {
            vp_h = vp_h.min(target_h);
        }
        (vp_w, vp_h)
    }

    fn resolved_scissor_rect(
        &self,
        target: ImageId,
        vp: Viewport,
        scissor: Option<ScissorRect>,
    ) -> (u32, u32, u32, u32) {
        let (target_w, target_h) = self.render_target_extent(target);
        let vp_x = vp.x.max(0) as u32;
        let vp_y = vp.y.max(0) as u32;
        let (vp_w, vp_h) = self.resolved_viewport_extent(target, vp);
        let vp_max_x = vp_x.saturating_add(vp_w).min(target_w);
        let vp_max_y = vp_y.saturating_add(vp_h).min(target_h);

        let top_left_to_bottom_left = |min_y_top: u32, max_y_top: u32| {
            let min_y = target_h.saturating_sub(max_y_top.min(target_h));
            let max_y = target_h.saturating_sub(min_y_top.min(target_h));
            (min_y, max_y)
        };

        match scissor {
            Some(rect) => {
                let min_x = rect.x.min(target_w);
                let max_x = rect.x.saturating_add(rect.width).min(target_w);
                let min_y_top = rect.y.min(target_h);
                let max_y_top = rect.y.saturating_add(rect.height).min(target_h);
                let (min_y, max_y) = top_left_to_bottom_left(min_y_top, max_y_top);
                (min_x, min_y, max_x, max_y)
            }
            None => {
                let (min_y, max_y) = top_left_to_bottom_left(vp_y.min(target_h), vp_max_y);
                (vp_x.min(target_w), min_y, vp_max_x, max_y)
            }
        }
    }

    pub fn init(
        _framebuffers: Option<&'static ::limine::response::FramebufferResponse>,
    ) -> Option<Self> {
        let mut gpu = VirtioGpu3d::init_first()?;
        let mut display = None;
        for attempt in 1..=10u32 {
            if let Some(mode) = gpu.get_display_info() {
                display = Some(mode);
                break;
            }
            if attempt == 1 || attempt == 10 || (attempt % 3) == 0 {
                crate::log!(
                    "virgl-backend: waiting for enabled scanout (attempt {}/{})\n",
                    attempt,
                    10
                );
            }
            let _ = wait::spin_until_timeout_no_exec(20, || false);
        }
        let (scanout_id, disp_w, disp_h) = display?;

        // Best-effort refresh estimation via EDID.
        let refresh_millihz = {
            let mut buf = [0u8; EDID_MAX_BYTES];
            gpu.get_edid(scanout_id, &mut buf)
                .and_then(|n| edid_preferred_refresh_millihz(&buf[..n]))
        };

        let ctx_id = alloc_ctx_id();
        if !gpu.ctx_create(ctx_id) {
            crate::log!("virgl-backend: ctx_create failed\n");
            return None;
        }

        // Allocate resource ids.
        let (scanout_res, rt_res) = alloc_res_pair();
        let vbo_res = alloc_res_pair().0;
        let debug_tex_res = alloc_res_id();

        // 2D scanout + backing.
        // Architectural decision: keep scanout backing owned by the gfx backend (DMA memory).
        // We do not borrow the Limine framebuffer backing.
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
        let scanout_backing = DmaRegion::alloc(bytes, 4096)?;
        let (res_w, res_h, res_bytes, present_w, present_h) =
            (disp_w, disp_h, bytes, disp_w, disp_h);

        if !gpu.resource_create_2d(scanout_res, VIRGL_FORMAT_B8G8R8A8_UNORM, res_w, res_h) {
            crate::log!("virgl-backend: resource_create_2d scanout failed\n");
            return None;
        }

        if scanout_backing.len() < res_bytes {
            crate::log!(
                "virgl-backend: scanout backing too small len={} need={}\n",
                scanout_backing.len(),
                res_bytes
            );
            return None;
        }

        let clear_len = res_bytes.min(scanout_backing.len());
        unsafe { core::ptr::write_bytes(scanout_backing.virt(), 0, clear_len) };
        crate::log!("virgl-backend: scanout backing cleared\n");

        if !gpu.resource_attach_backing(scanout_res, scanout_backing.phys(), res_bytes as u32) {
            crate::log!("virgl-backend: attach_backing failed\n");
            return None;
        }
        if !gpu.set_scanout(scanout_id, scanout_res, present_w, present_h) {
            crate::log!("virgl-backend: set_scanout failed\n");
            return None;
        }
        // Bringup guardrail: ensure the scanout upload path works immediately after scanout bind.
        if !gpu.transfer_to_host_2d(scanout_res, present_w, present_h) {
            crate::log!("virgl-backend: initial transfer_to_host_2d failed\n");
            return None;
        }
        if !gpu.resource_flush(scanout_res, present_w, present_h) {
            crate::log!("virgl-backend: initial resource_flush failed\n");
            return None;
        }

        // Render target (3D texture).
        let rt_bind = PIPE_BIND_RENDER_TARGET | PIPE_BIND_BLENDABLE;
        if !gpu.resource_create_3d(
            ctx_id,
            rt_res,
            PIPE_TEXTURE_2D,
            VIRGL_FORMAT_B8G8R8A8_UNORM,
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
        if !gpu.resource_create_3d(
            ctx_id,
            debug_tex_res,
            PIPE_TEXTURE_2D,
            VIRGL_FORMAT_R8G8B8A8_UNORM,
            PIPE_BIND_SAMPLER_VIEW,
            2,
            2,
            1,
            1,
            0,
            0,
            0,
        ) {
            crate::log!("virgl-backend: resource_create_3d debug tex failed\n");
            return None;
        }

        let _ = gpu.ctx_attach_resource(ctx_id, scanout_res);
        let _ = gpu.ctx_attach_resource(ctx_id, rt_res);
        let _ = gpu.ctx_attach_resource(ctx_id, vbo_res);
        let _ = gpu.ctx_attach_resource(ctx_id, debug_tex_res);

        // One-time state/program setup.
        let surf_handle = 10u32;
        let ve_handle = 11u32;
        let vs_color_handle = 20u32;
        let fs_color_handle = 21u32;
        let vs_tex_handle = 22u32;
        let fs_tex_mask_handle = 23u32;
        let fs_tex_rgba_handle = 24u32;
        let fs_mandelbrot_handle = 25u32;
        let sampler_state_handle = 26u32;
        let sampler_state_linear_handle = 27u32;
        let debug_tex_view = alloc_obj_handle();
        // Blend object handles.
        // 30.. are reserved for our fixed state objects.
        let blend_handle_disabled = 30u32;
        let blend_handle_straight = 33u32;
        let blend_handle_additive = 35u32;
        let blend_handle_multiply = 36u32;
        let blend_handle_screen = 37u32;
        let blend_handle_premult = 34u32;
        let dsa_handle = 31u32;
        let rs_handle = 32u32;

        let mut init = VirglCmdBuf::new();
        encode_create_surface(&mut init, surf_handle, rt_res, VIRGL_FORMAT_B8G8R8A8_UNORM);
        encode_set_framebuffer(&mut init, surf_handle);

        encode_create_vertex_elements(&mut init, ve_handle);
        encode_bind_object(&mut init, VIRGL_OBJECT_VERTEX_ELEMENTS, ve_handle);

        // Virgl vertex format is fixed for this minimal backend.
        encode_set_vertex_buffer(&mut init, core::mem::size_of::<Vertex>() as u32, 0, vbo_res);

        let fs_mandelbrot = crate::gfx::mandelbrot::build_fragment_shader_tgsi_unrolled(64);

        // Color pipeline program.
        encode_shader(&mut init, vs_color_handle, PIPE_SHADER_VERTEX, VS_COLOR);
        encode_bind_shader(&mut init, vs_color_handle, PIPE_SHADER_VERTEX);
        encode_shader(&mut init, fs_color_handle, PIPE_SHADER_FRAGMENT, FS_COLOR);
        encode_bind_shader(&mut init, fs_color_handle, PIPE_SHADER_FRAGMENT);
        encode_link_shader(&mut init, vs_color_handle, fs_color_handle);

        // Textured pipeline program.
        encode_shader(&mut init, vs_tex_handle, PIPE_SHADER_VERTEX, VS_TEX);
        encode_shader(
            &mut init,
            fs_tex_mask_handle,
            PIPE_SHADER_FRAGMENT,
            FS_TEX_MASK,
        );
        encode_shader(
            &mut init,
            fs_tex_rgba_handle,
            PIPE_SHADER_FRAGMENT,
            FS_TEX_RGBA,
        );
        encode_bind_shader(&mut init, vs_tex_handle, PIPE_SHADER_VERTEX);
        encode_bind_shader(&mut init, fs_tex_mask_handle, PIPE_SHADER_FRAGMENT);
        encode_link_shader(&mut init, vs_tex_handle, fs_tex_mask_handle);
        encode_bind_shader(&mut init, fs_tex_rgba_handle, PIPE_SHADER_FRAGMENT);
        encode_link_shader(&mut init, vs_tex_handle, fs_tex_rgba_handle);
        encode_shader(
            &mut init,
            fs_mandelbrot_handle,
            PIPE_SHADER_FRAGMENT,
            fs_mandelbrot.as_str(),
        );
        encode_bind_shader(&mut init, fs_mandelbrot_handle, PIPE_SHADER_FRAGMENT);
        encode_link_shader(&mut init, vs_tex_handle, fs_mandelbrot_handle);

        // Shared sampler states for 2D textures (sampler views are per-image).
        encode_create_sampler_state(
            &mut init,
            sampler_state_handle,
            trueos_gfx_core::SamplerDesc {
                wrap_s: trueos_gfx_core::SamplerWrap::ClampToEdge,
                wrap_t: trueos_gfx_core::SamplerWrap::ClampToEdge,
                min_filter: trueos_gfx_core::SamplerFilter::Nearest,
                mag_filter: trueos_gfx_core::SamplerFilter::Nearest,
            },
        );
        encode_create_sampler_state(
            &mut init,
            sampler_state_linear_handle,
            trueos_gfx_core::SamplerDesc {
                wrap_s: trueos_gfx_core::SamplerWrap::ClampToEdge,
                wrap_t: trueos_gfx_core::SamplerWrap::ClampToEdge,
                min_filter: trueos_gfx_core::SamplerFilter::Linear,
                mag_filter: trueos_gfx_core::SamplerFilter::Linear,
            },
        );

        // Gallium blend factors (Mesa pipe/p_defines.h).
        const PIPE_BLENDFACTOR_ONE: u32 = 1;
        const PIPE_BLENDFACTOR_DST_COLOR: u32 = 5;
        const PIPE_BLENDFACTOR_INV_SRC_COLOR: u32 = 0x12;
        const PIPE_BLENDFACTOR_SRC_ALPHA: u32 = 3;
        const PIPE_BLENDFACTOR_INV_SRC_ALPHA: u32 = 0x13;

        // Disabled blending (opaque path).
        encode_create_blend(&mut init, blend_handle_disabled, false, 0, 0);
        // Straight alpha: src*srcA + dst*(1-srcA)
        encode_create_blend(
            &mut init,
            blend_handle_straight,
            true,
            PIPE_BLENDFACTOR_SRC_ALPHA,
            PIPE_BLENDFACTOR_INV_SRC_ALPHA,
        );
        // Additive: src*srcA + dst*1
        encode_create_blend(
            &mut init,
            blend_handle_additive,
            true,
            PIPE_BLENDFACTOR_SRC_ALPHA,
            PIPE_BLENDFACTOR_ONE,
        );
        // Multiply: src*dst + dst*(1-srcA)
        encode_create_blend(
            &mut init,
            blend_handle_multiply,
            true,
            PIPE_BLENDFACTOR_DST_COLOR,
            PIPE_BLENDFACTOR_INV_SRC_ALPHA,
        );
        // Screen: src*1 + dst*(1-srcColor)
        encode_create_blend(
            &mut init,
            blend_handle_screen,
            true,
            PIPE_BLENDFACTOR_ONE,
            PIPE_BLENDFACTOR_INV_SRC_COLOR,
        );
        // Premult alpha: src*1 + dst*(1-srcA)
        encode_create_blend(
            &mut init,
            blend_handle_premult,
            true,
            PIPE_BLENDFACTOR_ONE,
            PIPE_BLENDFACTOR_INV_SRC_ALPHA,
        );

        // Bind disabled by default to match WebGL state.
        encode_bind_object(&mut init, VIRGL_OBJECT_BLEND, blend_handle_disabled);
        encode_create_dsa(&mut init, dsa_handle);
        encode_bind_object(&mut init, VIRGL_OBJECT_DSA, dsa_handle);
        encode_create_rasterizer(&mut init, rs_handle, true);
        encode_bind_object(&mut init, VIRGL_OBJECT_RASTERIZER, rs_handle);

        encode_set_viewport(&mut init, present_w, present_h);
        encode_set_scissor(&mut init, 0, 0, present_w, present_h);

        if !gpu.submit_3d(ctx_id, init.as_bytes(), 1) {
            crate::log!("virgl-backend: submit_3d init failed\n");
            return None;
        }

        crate::log!(
            "virgl-backend: init ok scanout={} display={}x{} refresh_millihz={:?} ctx={} scanout_res={} rt_res={}\n",
            scanout_id,
            present_w,
            present_h,
            refresh_millihz,
            ctx_id,
            scanout_res,
            rt_res
        );

        let swapchain = SwapchainDesc {
            format: ImageFormat::Rgba8888,
            extent: trueos_gfx_core::Extent2D {
                width: present_w,
                height: present_h,
            },
        };

        let mut backend = Self {
            gpu,
            ctx_id,
            scanout_id,
            width: present_w,
            height: present_h,
            scanout_res,
            scanout_backing,
            rt_res,
            rt_surf_handle: surf_handle,
            vbo_res,
            vbo_cap_bytes,
            vs_color_handle,
            fs_color_handle,
            vs_tex_handle,
            fs_tex_mask_handle,
            fs_tex_rgba_handle,
            fs_mandelbrot_handle,
            sampler_state_nearest_handle: sampler_state_handle,
            sampler_state_linear_handle,
            debug_tex_res,
            debug_tex_view,
            debug_tex_uploaded: false,
            debug_tex_view_created: false,
            blend_handle_disabled,
            blend_handle_straight,
            blend_handle_additive,
            blend_handle_multiply,
            blend_handle_screen,
            blend_handle_premult,
            swapchain,

            refresh_millihz,
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
            frame_counter: 1,
        };

        let bootstrap_cmds = [
            Command::SetViewport(Viewport {
                x: 0,
                y: 0,
                width: present_w as i32,
                height: present_h as i32,
            }),
            Command::Present,
        ];
        if backend
            .submit(CommandBuffer {
                commands: &bootstrap_cmds,
            })
            .is_err()
        {
            crate::log!("virgl-backend: bootstrap present failed\n");
            return None;
        }
        crate::log!(
            "virgl-backend: bootstrap present ok display={}x{}\n",
            present_w,
            present_h
        );

        Some(backend)
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

    fn render_target_extent(&self, target: ImageId) -> (u32, u32) {
        if !target.is_valid() {
            return (self.width, self.height);
        }
        let idx = target.raw().saturating_sub(1) as usize;
        self.images
            .get(idx)
            .and_then(|entry| entry.as_ref())
            .map(|img| (img.desc.width, img.desc.height))
            .unwrap_or((self.width, self.height))
    }

    fn ensure_render_target_surface(
        &mut self,
        target: ImageId,
        cmd: &mut VirglCmdBuf,
    ) -> GfxResult<u32> {
        if !target.is_valid() {
            return Ok(self.rt_surf_handle);
        }
        let idx = target.raw().saturating_sub(1) as usize;
        let Some(img) = self.images.get_mut(idx).and_then(|entry| entry.as_mut()) else {
            return Err(Error::NotFound);
        };
        if !img.virgl_surface_created {
            if img.virgl_surface == 0 {
                img.virgl_surface = alloc_obj_handle();
            }
            encode_create_surface(
                cmd,
                img.virgl_surface,
                img.virgl_res,
                VIRGL_FORMAT_R8G8B8A8_UNORM,
            );
            img.virgl_surface_created = true;
        }
        Ok(img.virgl_surface)
    }

    fn build_virgl_vertices(
        &self,
        buf: &[u8],
        pipe_desc: PipelineDesc,
        binding: VirglBufferBinding,
        draw: (u32, u32),
        image: Option<&HostImage>,
    ) -> Option<Vec<Vertex>> {
        let PipelineDesc { vertex_layout, .. } = pipe_desc;
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

            let (u_f, v_f) = if texcoord_format == TexCoordFormat::UvF32 {
                let tex_off = off + (texcoord_offset as usize);
                if tex_off + 8 > buf.len() {
                    break;
                }
                let u = f32::from_le_bytes(buf[tex_off..tex_off + 4].try_into().ok()?);
                let v = f32::from_le_bytes(buf[tex_off + 4..tex_off + 8].try_into().ok()?);
                (u, v)
            } else {
                (0.0, 0.0)
            };

            let _ = image;

            let rf = (r_u8 as f32) / 255.0;
            let gf = (g_u8 as f32) / 255.0;
            let bf = (b_u8 as f32) / 255.0;
            let af = (a_u8 as f32) / 255.0;

            out.push(Vertex {
                pos: [x, y, 0.0, 1.0],
                uv: [u_f, v_f, 0.0, 1.0],
                color: [rf, gf, bf, af],
            });
        }
        Some(out)
    }
}

impl GfxDevice for VirglGfxBackend {
    fn caps(&self) -> DeviceCaps {
        DeviceCaps {
            supports_scissor: true,
            ..DeviceCaps::minimal_software()
        }
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
        let bytes = vec![0; bytes_len];

        // Create a virgl texture resource for real GPU sampling.
        let virgl_res = alloc_res_id();
        if !self.gpu.resource_create_3d(
            self.ctx_id,
            virgl_res,
            PIPE_TEXTURE_2D,
            VIRGL_FORMAT_R8G8B8A8_UNORM,
            PIPE_BIND_SAMPLER_VIEW | PIPE_BIND_RENDER_TARGET | PIPE_BIND_BLENDABLE,
            desc.width,
            desc.height,
            1,
            1,
            0,
            0,
            0,
        ) {
            return Err(Error::Unsupported);
        }
        let _ = self.gpu.ctx_attach_resource(self.ctx_id, virgl_res);

        let virgl_view = alloc_obj_handle();
        let slot = Self::alloc_slot(
            &mut self.images,
            HostImage {
                desc,
                bytes,
                revision: 1,
                virgl_res,
                virgl_view,
                virgl_uploaded_rev: 0,
                virgl_view_created: false,
                virgl_surface: 0,
                virgl_surface_created: false,
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
            if let Some(Some(img)) = self.images.get(idx) {
                if img.virgl_res != 0 {
                    let _ = self.gpu.ctx_detach_resource(self.ctx_id, img.virgl_res);
                }
            }
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
        {
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
        let frame_no = self.frame_counter;
        self.frame_counter = self.frame_counter.wrapping_add(1);
        if self.frame_counter == 0 {
            self.frame_counter = 1;
        }

        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        enum DrawKind {
            Color,
            TexturedMask,
            TexturedRgba,
            Mandelbrot,
        }

        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        struct DrawStateKey {
            kind: DrawKind,
            pipeline: u32,
            vertex_buffer: u32,
            vertex_offset: u64,
            render_target: u32,
            blend: BlendDesc,
            sampler: SamplerDesc,
            image: u32,
        }

        #[derive(Clone, Copy)]
        struct DrawOp {
            state: VirglDrawState,
            vertex_count: u32,
            first_vertex: u32,
        }

        #[derive(Clone, Copy)]
        enum Op {
            SetViewport(Viewport),
            SetScissor(Option<ScissorRect>),
            SetRenderTarget(ImageId),
            ClearColor(u32),
            Draw(DrawOp),
            Present,
        }

        // Capture commands in-order with draw-state snapshots.
        let mut ops: Vec<Op> = Vec::new();
        for c in cmds.commands {
            match *c {
                Command::SetViewport(vp) => {
                    self.state.viewport = vp;
                    ops.push(Op::SetViewport(vp));
                }
                Command::SetScissor(scissor) => {
                    self.state.scissor = scissor;
                    ops.push(Op::SetScissor(scissor));
                }
                Command::SetRenderTarget(target) => {
                    self.state.render_target = target.unwrap_or(ImageId::invalid());
                    ops.push(Op::SetRenderTarget(self.state.render_target));
                }
                Command::ClearColor { rgb } => {
                    self.state.clear_rgb = rgb;
                    ops.push(Op::ClearColor(rgb));
                }
                Command::ClearRect { rgb, .. } => {
                    // Rect clears are not supported yet; approximate with full clear.
                    self.state.clear_rgb = rgb;
                    ops.push(Op::ClearColor(rgb));
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
                Command::SetBlend(b) => {
                    self.state.blend = b;
                }
                Command::Draw {
                    vertex_count,
                    first_vertex,
                } => {
                    if vertex_count != 0 {
                        ops.push(Op::Draw(DrawOp {
                            state: self.state,
                            vertex_count,
                            first_vertex,
                        }));
                    }
                }
                Command::Present => ops.push(Op::Present),
            }
        }

        let mut cmd = VirglCmdBuf::new();
        let mut did_work = false;
        let mut did_present = false;
        let mut clear_ops: u32 = 0;
        let mut draw_ops: u32 = 0;
        let mut present_ops: u32 = 0;
        let mut last_viewport: Option<(u32, u32)> = None;
        let mut last_scissor: Option<(u32, u32, u32, u32)> = None;
        let mut last_draw_key: Option<DrawStateKey> = None;
        let mut last_bound_blend: Option<u32> = None;
        let mut last_bound_sampler_view: Option<u32> = None;
        let mut last_bound_sampler_state: Option<u32> = None;
        let mut last_bound_shader_kind: Option<DrawKind> = None;
        let mut last_bound_vbo: Option<(u32, u32)> = None;
        let mut last_framebuffer_surface: Option<u32> = None;
        let mut frame_vbo_write_offset: u32 = 0;

        for op in ops {
            match op {
                Op::SetViewport(vp) => {
                    self.state.viewport = vp;
                    let (vp_w, vp_h) = self.resolved_viewport_extent(self.state.render_target, vp);
                    if last_viewport != Some((vp_w, vp_h)) {
                        encode_set_viewport(&mut cmd, vp_w, vp_h);
                        last_viewport = Some((vp_w, vp_h));
                        did_work = true;
                    }
                    if self.state.scissor.is_none() {
                        let scissor =
                            self.resolved_scissor_rect(self.state.render_target, vp, None);
                        if last_scissor != Some(scissor) {
                            encode_set_scissor(
                                &mut cmd, scissor.0, scissor.1, scissor.2, scissor.3,
                            );
                            last_scissor = Some(scissor);
                            did_work = true;
                        }
                    }
                }
                Op::SetScissor(scissor) => {
                    self.state.scissor = scissor;
                    let scissor = self.resolved_scissor_rect(
                        self.state.render_target,
                        self.state.viewport,
                        scissor,
                    );
                    if last_scissor != Some(scissor) {
                        encode_set_scissor(&mut cmd, scissor.0, scissor.1, scissor.2, scissor.3);
                        last_scissor = Some(scissor);
                        did_work = true;
                    }
                }
                Op::SetRenderTarget(target) => {
                    self.state.render_target = target;
                    let surf = self.ensure_render_target_surface(target, &mut cmd)?;
                    if last_framebuffer_surface != Some(surf) {
                        encode_set_framebuffer(&mut cmd, surf);
                        last_framebuffer_surface = Some(surf);
                        last_viewport = None;
                        last_scissor = None;
                        did_work = true;
                    }
                }
                Op::ClearColor(rgb) => {
                    let surf =
                        self.ensure_render_target_surface(self.state.render_target, &mut cmd)?;
                    if last_framebuffer_surface != Some(surf) {
                        encode_set_framebuffer(&mut cmd, surf);
                        last_framebuffer_surface = Some(surf);
                        did_work = true;
                    }
                    clear_ops = clear_ops.saturating_add(1);
                    let (r, g, b) = Self::rgb_to_f32(rgb);
                    encode_clear_color(&mut cmd, r, g, b, 1.0);
                    did_work = true;
                }
                Op::Draw(draw) => {
                    draw_ops = draw_ops.saturating_add(1);
                    self.state = draw.state;
                    let vertex_count = draw.vertex_count;
                    let first_vertex = draw.first_vertex;

                    let pipeline_raw = self.state.pipeline.raw();
                    let pipe_idx = pipeline_raw.saturating_sub(1) as usize;
                    let Some(pipe) = self.pipelines.get(pipe_idx).and_then(|p| p.as_ref()) else {
                        return Err(Error::NotFound);
                    };
                    let pipe_desc = pipe.desc;
                    let draw_kind = if pipe_desc.vertex_layout.texcoord_format
                        != TexCoordFormat::UvF32
                    {
                        DrawKind::Color
                    } else if pipe_desc.fs.map(|id| id.raw()) == Some(TEX_PIPELINE_FS_RGBA_TAG_RAW)
                    {
                        DrawKind::TexturedRgba
                    } else if pipe_desc.fs.map(|id| id.raw())
                        == Some(crate::gfx::mandelbrot::MANDELBROT_PIPELINE_FS_TAG_RAW)
                    {
                        DrawKind::Mandelbrot
                    } else {
                        DrawKind::TexturedMask
                    };

                    let draw_key = DrawStateKey {
                        kind: draw_kind,
                        pipeline: pipeline_raw,
                        vertex_buffer: self.state.vertex.id.raw(),
                        vertex_offset: self.state.vertex.offset,
                        render_target: self.state.render_target.raw(),
                        blend: self.state.blend,
                        sampler: self.state.sampler,
                        image: self.state.image.raw(),
                    };

                    if let Some(prev) = last_draw_key
                        && prev.kind != draw_key.kind
                    {
                        let n = crate::logflag::VIRGL_STATE_TRANSITION_LOGS
                            .fetch_add(1, Ordering::Relaxed);
                        if n < 8 {
                            crate::log!(
                                "virgl-backend: draw-kind transition {:?} -> {:?} frame={}\n",
                                prev.kind,
                                draw_key.kind,
                                frame_no
                            );
                        }
                    }
                    last_draw_key = Some(draw_key);

                    // Ensure current viewport is encoded before drawing.
                    let (vp_w, vp_h) = self
                        .resolved_viewport_extent(self.state.render_target, self.state.viewport);
                    if last_viewport != Some((vp_w, vp_h)) {
                        encode_set_viewport(&mut cmd, vp_w, vp_h);
                        last_viewport = Some((vp_w, vp_h));
                    }
                    let scissor = self.resolved_scissor_rect(
                        self.state.render_target,
                        self.state.viewport,
                        self.state.scissor,
                    );
                    if last_scissor != Some(scissor) {
                        encode_set_scissor(&mut cmd, scissor.0, scissor.1, scissor.2, scissor.3);
                        last_scissor = Some(scissor);
                    }
                    let surf =
                        self.ensure_render_target_surface(self.state.render_target, &mut cmd)?;
                    if last_framebuffer_surface != Some(surf) {
                        encode_set_framebuffer(&mut cmd, surf);
                        last_framebuffer_surface = Some(surf);
                    }

                    let blend_handle = if !self.state.blend.enabled {
                        self.blend_handle_disabled
                    } else {
                        match (self.state.blend.src, self.state.blend.dst) {
                            (BlendFactor::SrcAlpha, BlendFactor::OneMinusSrcAlpha) => {
                                self.blend_handle_straight
                            }
                            (BlendFactor::SrcAlpha, BlendFactor::One) => self.blend_handle_additive,
                            (BlendFactor::DstColor, BlendFactor::OneMinusSrcAlpha) => {
                                self.blend_handle_multiply
                            }
                            (BlendFactor::One, BlendFactor::OneMinusSrcColor) => {
                                self.blend_handle_screen
                            }
                            (BlendFactor::One, BlendFactor::OneMinusSrcAlpha) => {
                                self.blend_handle_premult
                            }
                            (BlendFactor::One, BlendFactor::Zero) => self.blend_handle_disabled,
                            other => {
                                let n = crate::logflag::VIRGL_BLEND_UNSUPPORTED_LOGS
                                    .fetch_add(1, Ordering::Relaxed);
                                if n < 8 {
                                    crate::log!(
                                        "virgl-backend: unsupported blend {:?}; failing draw\n",
                                        other
                                    );
                                }
                                return Err(Error::Unsupported);
                            }
                        }
                    };
                    if last_bound_blend != Some(blend_handle) {
                        let n =
                            crate::logflag::VIRGL_BLEND_BIND_LOGS.fetch_add(1, Ordering::Relaxed);
                        if n < 8 {
                            crate::log!(
                                "virgl-backend: bind blend {:?} -> handle={}\n",
                                self.state.blend,
                                blend_handle
                            );
                        }
                        encode_bind_object(&mut cmd, VIRGL_OBJECT_BLEND, blend_handle);
                        last_bound_blend = Some(blend_handle);
                    }
                    // Defensive isolation between colored and textured draw paths:
                    // if draw kind flips, invalidate converted/uploaded caches so
                    // no previously converted vertex stream can bleed across paths.
                    if let Some(prev) = self.converted_cache.key
                        && prev.layout_texcoord_format != pipe_desc.vertex_layout.texcoord_format
                    {
                        self.converted_cache.key = None;
                        self.converted_cache.bytes.clear();
                        self.converted_cache.vertex_count = 0;
                        self.uploaded_cache_key = None;
                        self.uploaded_cache_vbo_generation = 0;
                    }

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
                        layout_stride: pipe_desc.vertex_layout.stride,
                        layout_pos_offset: pipe_desc.vertex_layout.pos_offset,
                        layout_color_offset: pipe_desc.vertex_layout.color_offset,
                        layout_color_format: pipe_desc.vertex_layout.color_format,
                        layout_texcoord_offset: pipe_desc.vertex_layout.texcoord_offset,
                        layout_texcoord_format: pipe_desc.vertex_layout.texcoord_format,
                        image_id: if draw_kind == DrawKind::Mandelbrot {
                            0
                        } else if pipe_desc.vertex_layout.texcoord_format == TexCoordFormat::UvF32
                            && (VIRGL_FORCE_DEBUG_TEXTURE
                                || self.state.image.raw() == VIRGL_DEBUG_TEXTURE_ID_RAW)
                        {
                            VIRGL_DEBUG_TEXTURE_ID_RAW
                        } else {
                            self.state.image.raw()
                        },
                        image_rev: if draw_kind == DrawKind::Mandelbrot {
                            0
                        } else if pipe_desc.vertex_layout.texcoord_format == TexCoordFormat::UvF32
                            && (VIRGL_FORCE_DEBUG_TEXTURE
                                || self.state.image.raw() == VIRGL_DEBUG_TEXTURE_ID_RAW)
                        {
                            1
                        } else if self.state.image.is_valid() {
                            self.images
                                .get(self.state.image.raw().saturating_sub(1) as usize)
                                .and_then(|i| i.as_ref())
                                .map(|i| i.revision)
                                .unwrap_or(0)
                        } else {
                            0
                        },
                    };

                    let bypass_cache_for_color =
                        pipe_desc.vertex_layout.texcoord_format == TexCoordFormat::None;
                    if VIRGL_DISABLE_CONVERTED_VERTEX_CACHE
                        || bypass_cache_for_color
                        || self.converted_cache.key != Some(cache_key)
                    {
                        let bound_image = if draw_kind == DrawKind::Mandelbrot {
                            None
                        } else if pipe_desc.vertex_layout.texcoord_format == TexCoordFormat::UvF32 {
                            let use_debug_texture = VIRGL_FORCE_DEBUG_TEXTURE
                                || self.state.image.raw() == VIRGL_DEBUG_TEXTURE_ID_RAW;
                            if !use_debug_texture && !self.state.image.is_valid() {
                                return Err(Error::Invalid);
                            }
                            if use_debug_texture {
                                None
                            } else {
                                self.images
                                    .get(self.state.image.raw().saturating_sub(1) as usize)
                                    .and_then(|i| i.as_ref())
                            }
                        } else {
                            None
                        };
                        let verts = self
                            .build_virgl_vertices(
                                &vb.bytes,
                                pipe_desc,
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

                    if self.converted_cache.bytes.is_empty()
                        || self.converted_cache.vertex_count == 0
                    {
                        return Err(Error::Invalid);
                    }

                    let draw_bytes = self.converted_cache.bytes.len();
                    let draw_bytes_u32 = draw_bytes.min(u32::MAX as usize) as u32;
                    let Some(need_end) = frame_vbo_write_offset.checked_add(draw_bytes_u32) else {
                        return Err(Error::OutOfMemory);
                    };
                    if !self.ensure_vbo_capacity(need_end as usize) {
                        return Err(Error::OutOfMemory);
                    }

                    let need_upload = true;
                    if need_upload {
                        encode_inline_write_buffer_at(
                            &mut cmd,
                            self.vbo_res,
                            frame_vbo_write_offset,
                            self.converted_cache.bytes.as_slice(),
                        );
                        self.uploaded_cache_key = Some(cache_key);
                        self.uploaded_cache_vbo_generation = self.vbo_generation;
                    }

                    if last_bound_vbo != Some((self.vbo_res, frame_vbo_write_offset)) {
                        encode_set_vertex_buffer(
                            &mut cmd,
                            core::mem::size_of::<Vertex>() as u32,
                            frame_vbo_write_offset,
                            self.vbo_res,
                        );
                        last_bound_vbo = Some((self.vbo_res, frame_vbo_write_offset));
                    }

                    if draw_kind == DrawKind::Mandelbrot {
                        if last_bound_sampler_view != Some(0) {
                            encode_set_sampler_views(&mut cmd, PIPE_SHADER_FRAGMENT, 0, &[0]);
                            last_bound_sampler_view = Some(0);
                        }
                        if last_bound_sampler_state != Some(0) {
                            encode_bind_sampler_states(&mut cmd, PIPE_SHADER_FRAGMENT, 0, &[0]);
                            last_bound_sampler_state = Some(0);
                        }
                        if last_bound_shader_kind != Some(DrawKind::Mandelbrot) {
                            encode_bind_shader(&mut cmd, self.vs_tex_handle, PIPE_SHADER_VERTEX);
                            encode_bind_shader(
                                &mut cmd,
                                self.fs_mandelbrot_handle,
                                PIPE_SHADER_FRAGMENT,
                            );
                            last_bound_shader_kind = Some(DrawKind::Mandelbrot);
                        }
                    } else if pipe_desc.vertex_layout.texcoord_format == TexCoordFormat::UvF32 {
                        let use_debug_texture = VIRGL_FORCE_DEBUG_TEXTURE
                            || self.state.image.raw() == VIRGL_DEBUG_TEXTURE_ID_RAW;
                        let (img_raw, virgl_res, virgl_view, samp_handle) = if use_debug_texture {
                            if !self.debug_tex_uploaded {
                                encode_inline_write_texture(
                                    &mut cmd,
                                    self.debug_tex_res,
                                    2,
                                    2,
                                    &VIRGL_DEBUG_TEXTURE_RGBA_2X2,
                                );
                                self.debug_tex_uploaded = true;
                            }
                            if !self.debug_tex_view_created {
                                encode_create_sampler_view(
                                    &mut cmd,
                                    self.debug_tex_view,
                                    self.debug_tex_res,
                                    VIRGL_FORMAT_R8G8B8A8_UNORM,
                                );
                                self.debug_tex_view_created = true;
                            }
                            (
                                VIRGL_DEBUG_TEXTURE_ID_RAW,
                                self.debug_tex_res,
                                self.debug_tex_view,
                                self.sampler_state_nearest_handle,
                            )
                        } else {
                            let img_raw = self.state.image.raw();
                            let img_idx = img_raw.saturating_sub(1) as usize;
                            let Some(img) = self.images.get_mut(img_idx).and_then(|i| i.as_mut())
                            else {
                                return Err(Error::NotFound);
                            };
                            if img.virgl_res == 0 {
                                return Err(Error::Invalid);
                            }

                            if img.virgl_uploaded_rev != img.revision {
                                let n = crate::logflag::VIRGL_TEX_DEBUG_LOGS
                                    .fetch_add(1, Ordering::Relaxed);
                                if n < 8 {
                                    crate::log!(
                                        "virgl-backend: tex upload img={} {}x{} rev {}->{} res={} view={}\n",
                                        img_raw,
                                        img.desc.width,
                                        img.desc.height,
                                        img.virgl_uploaded_rev,
                                        img.revision,
                                        img.virgl_res,
                                        img.virgl_view
                                    );
                                }
                                encode_inline_write_texture(
                                    &mut cmd,
                                    img.virgl_res,
                                    img.desc.width,
                                    img.desc.height,
                                    img.bytes.as_slice(),
                                );
                                img.virgl_uploaded_rev = img.revision;
                            }

                            if !img.virgl_view_created {
                                let n = crate::logflag::VIRGL_TEX_DEBUG_LOGS
                                    .fetch_add(1, Ordering::Relaxed);
                                if n < 8 {
                                    crate::log!(
                                        "virgl-backend: tex create_view img={} res={} view={} fmt={}\n",
                                        img_raw,
                                        img.virgl_res,
                                        img.virgl_view,
                                        VIRGL_FORMAT_R8G8B8A8_UNORM
                                    );
                                }
                                encode_create_sampler_view(
                                    &mut cmd,
                                    img.virgl_view,
                                    img.virgl_res,
                                    VIRGL_FORMAT_R8G8B8A8_UNORM,
                                );
                                img.virgl_view_created = true;
                            }

                            let samp_handle = if self.state.sampler.min_filter
                                == trueos_gfx_core::SamplerFilter::Linear
                                || self.state.sampler.mag_filter
                                    == trueos_gfx_core::SamplerFilter::Linear
                            {
                                self.sampler_state_linear_handle
                            } else {
                                self.sampler_state_nearest_handle
                            };
                            (img_raw, img.virgl_res, img.virgl_view, samp_handle)
                        };

                        if crate::logflag::VIRGL_DRAW_DIAGNOSTICS_LOGS && frame_no % 100 == 0 {
                            crate::log!(
                                "virgl-diag: frame={} img={} res={} view={} samp_state={} sampler=({:?},{:?},{:?},{:?})\n",
                                frame_no,
                                img_raw,
                                virgl_res,
                                virgl_view,
                                samp_handle,
                                self.state.sampler.wrap_s,
                                self.state.sampler.wrap_t,
                                self.state.sampler.min_filter,
                                self.state.sampler.mag_filter
                            );
                        }

                        if last_bound_sampler_view != Some(virgl_view) {
                            encode_set_sampler_views(
                                &mut cmd,
                                PIPE_SHADER_FRAGMENT,
                                0,
                                &[virgl_view],
                            );
                            last_bound_sampler_view = Some(virgl_view);
                        }
                        if last_bound_sampler_state != Some(samp_handle) {
                            encode_bind_sampler_states(
                                &mut cmd,
                                PIPE_SHADER_FRAGMENT,
                                0,
                                &[samp_handle],
                            );
                            last_bound_sampler_state = Some(samp_handle);
                        }
                        if last_bound_shader_kind != Some(draw_kind) {
                            encode_bind_shader(&mut cmd, self.vs_tex_handle, PIPE_SHADER_VERTEX);
                            let fs_handle = if draw_kind == DrawKind::TexturedRgba {
                                self.fs_tex_rgba_handle
                            } else {
                                self.fs_tex_mask_handle
                            };
                            encode_bind_shader(&mut cmd, fs_handle, PIPE_SHADER_FRAGMENT);
                            last_bound_shader_kind = Some(draw_kind);
                        }
                    } else {
                        // Keep color-only draws isolated from prior textured draws:
                        // explicitly null-bind slot 0. Some virgl paths treat an
                        // empty list as no-op and keep previous bindings alive.
                        if last_bound_sampler_view != Some(0) {
                            encode_set_sampler_views(&mut cmd, PIPE_SHADER_FRAGMENT, 0, &[0]);
                            last_bound_sampler_view = Some(0);
                        }
                        if last_bound_sampler_state != Some(0) {
                            encode_bind_sampler_states(&mut cmd, PIPE_SHADER_FRAGMENT, 0, &[0]);
                            last_bound_sampler_state = Some(0);
                        }
                        if last_bound_shader_kind != Some(DrawKind::Color) {
                            encode_bind_shader(&mut cmd, self.vs_color_handle, PIPE_SHADER_VERTEX);
                            encode_bind_shader(
                                &mut cmd,
                                self.fs_color_handle,
                                PIPE_SHADER_FRAGMENT,
                            );
                            last_bound_shader_kind = Some(DrawKind::Color);
                        }
                    }

                    if crate::logflag::VIRGL_DRAW_DIAGNOSTICS_LOGS
                        && (frame_no <= 5 || (frame_no % 100) == 0)
                    {
                        if let Some((
                            min_x,
                            max_x,
                            min_y,
                            max_y,
                            min_a,
                            max_a,
                            min_u,
                            max_u,
                            min_v,
                            max_v,
                        )) = vertex_blob_bounds(self.converted_cache.bytes.as_slice())
                        {
                            crate::log!(
                                "virgl-draw: frame={} verts={} img={} ndc_x=[{:.3},{:.3}] ndc_y=[{:.3},{:.3}] a=[{:.3},{:.3}] uv_u=[{:.3},{:.3}] uv_v=[{:.3},{:.3}] vp={}x{}\n",
                                frame_no,
                                self.converted_cache.vertex_count,
                                self.state.image.raw(),
                                min_x,
                                max_x,
                                min_y,
                                max_y,
                                min_a,
                                max_a,
                                min_u,
                                max_u,
                                min_v,
                                max_v,
                                vp_w,
                                vp_h
                            );
                        }
                    }

                    encode_draw_vbo_count(&mut cmd, self.converted_cache.vertex_count);
                    did_work = true;

                    // Keep each draw in a unique VBO range for this submit so driver
                    // scheduling cannot alias vertices across neighboring draws.
                    let advanced = need_end.saturating_add(63) & !63;
                    frame_vbo_write_offset = advanced;
                }
                Op::Present => {
                    if self.state.render_target.is_valid() {
                        continue;
                    }
                    present_ops = present_ops.saturating_add(1);
                    encode_resource_copy_region(
                        &mut cmd,
                        self.scanout_res,
                        self.rt_res,
                        self.width,
                        self.height,
                    );
                    did_present = true;
                    did_work = true;
                }
            }
        }

        if !did_work {
            let fence = self.next_fence;
            self.next_fence = self.next_fence.wrapping_add(1);
            self.completed_fence = fence;
            return Ok(FenceId::from_raw(fence));
        }

        // Disable single-draw frame-cache shortcut until a multi-draw key is implemented.
        self.last_submitted_frame = None;

        let fence = self.next_fence;
        self.next_fence = self.next_fence.wrapping_add(1);

        if !self.gpu.submit_3d(self.ctx_id, cmd.as_bytes(), fence) {
            crate::log!(
                "virgl-present: submit_3d failed frame={} clears={} draws={} presents={} bytes={} ctx={}\n",
                frame_no,
                clear_ops,
                draw_ops,
                present_ops,
                cmd.as_bytes().len(),
                self.ctx_id
            );
            return Err(Error::Unsupported);
        }

        if did_present {
            let mut reassert_ok = true;
            if self.should_reassert_scanout(frame_no) {
                reassert_ok = self.gpu.set_scanout(
                    self.scanout_id,
                    self.scanout_res,
                    self.width,
                    self.height,
                );
                if !reassert_ok {
                    crate::log!(
                        "virgl-present: set_scanout reassert failed frame={} scanout={} res={} size={}x{}\n",
                        frame_no,
                        self.scanout_id,
                        self.scanout_res,
                        self.width,
                        self.height
                    );
                }
            }
            let flush_ok = self
                .gpu
                .resource_flush(self.scanout_res, self.width, self.height);
            // Present copies the rendered host resource into the host-side scanout resource.
            // Do not upload guest backing here: scanout_backing is only zeroed/attached during
            // bringup and uploading it would overwrite the freshly copied frame with stale pixels.
            let n = crate::logflag::VIRGL_PRESENT_DIAG_LOGS.fetch_add(1, Ordering::Relaxed);
            if crate::logflag::GFX_FRAME_PROGRESS_LOGS && n < 12 {
                crate::log!(
                    "virgl-present: frame={} clears={} draws={} presents={} reassert_ok={} flush_ok={} size={}x{} fence={}\n",
                    frame_no,
                    clear_ops,
                    draw_ops,
                    present_ops,
                    reassert_ok,
                    flush_ok,
                    self.width,
                    self.height,
                    fence
                );
            }
        }

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

    fn display_refresh_millihz(&mut self) -> Option<u32> {
        if self.refresh_millihz.is_some() {
            return self.refresh_millihz;
        }

        // Lazy retry: EDID may become available after initial scanout setup.
        let mut buf = [0u8; EDID_MAX_BYTES];
        let mhz = self
            .gpu
            .get_edid(self.scanout_id, &mut buf)
            .and_then(|n| edid_preferred_refresh_millihz(&buf[..n]));
        self.refresh_millihz = mhz;
        mhz
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
    // For TGSI text shaders virgl still expects a sensible token count in the header.
    // Small fixed shaders tolerated an arbitrary constant, but larger generated shaders do not.
    let mut bytes = Vec::new();
    bytes.extend_from_slice(text.as_bytes());
    bytes.push(0);

    let shader_len = bytes.len() as u32;
    let num_tokens = ((bytes.len() as u32).div_ceil(4)).max(1);
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
    let num = 3u32;
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

    // element 1: uv vec4 at offset 16
    buf.push(16);
    buf.push(0);
    buf.push(0);
    buf.push(VIRGL_FORMAT_R32G32B32A32_FLOAT);

    // element 2: color vec4 at offset 32
    buf.push(32);
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

fn encode_create_sampler_state(buf: &mut VirglCmdBuf, sampler_handle: u32, desc: SamplerDesc) {
    // virgl_protocol.h:
    // VIRGL_OBJ_SAMPLER_STATE_SIZE = 9
    // s0 bitfields:
    // - wrap_s/t/r: 3 bits each
    // - min_img_filter/min_mip_filter/mag_img_filter: 2 bits each
    // We intentionally keep this minimal and WebGL-ish.
    const VIRGL_OBJ_SAMPLER_STATE_SIZE: u32 = 9;
    // Enum values from Mesa pipe/p_defines.h:
    // enum pipe_tex_wrap: REPEAT=0, CLAMP=1, CLAMP_TO_EDGE=2, ...
    // enum pipe_tex_filter: NEAREST=0, LINEAR=1
    // enum pipe_tex_mipfilter: NEAREST=0, LINEAR=1, NONE=2
    const PIPE_TEX_WRAP_REPEAT: u32 = 0;
    const PIPE_TEX_WRAP_CLAMP_TO_EDGE: u32 = 2;
    const PIPE_TEX_FILTER_NEAREST: u32 = 0;
    const PIPE_TEX_FILTER_LINEAR: u32 = 1;
    const PIPE_TEX_MIPFILTER_NONE: u32 = 2;

    let wrap_s = match desc.wrap_s {
        trueos_gfx_core::SamplerWrap::ClampToEdge => PIPE_TEX_WRAP_CLAMP_TO_EDGE,
        trueos_gfx_core::SamplerWrap::Repeat => PIPE_TEX_WRAP_REPEAT,
    };
    let wrap_t = match desc.wrap_t {
        trueos_gfx_core::SamplerWrap::ClampToEdge => PIPE_TEX_WRAP_CLAMP_TO_EDGE,
        trueos_gfx_core::SamplerWrap::Repeat => PIPE_TEX_WRAP_REPEAT,
    };
    let wrap_r = PIPE_TEX_WRAP_CLAMP_TO_EDGE;
    let min_img = match desc.min_filter {
        trueos_gfx_core::SamplerFilter::Nearest => PIPE_TEX_FILTER_NEAREST,
        trueos_gfx_core::SamplerFilter::Linear => PIPE_TEX_FILTER_LINEAR,
    };
    let mag_img = match desc.mag_filter {
        trueos_gfx_core::SamplerFilter::Nearest => PIPE_TEX_FILTER_NEAREST,
        trueos_gfx_core::SamplerFilter::Linear => PIPE_TEX_FILTER_LINEAR,
    };
    let min_mip = PIPE_TEX_MIPFILTER_NONE;

    let mut s0 = 0u32;
    s0 |= (wrap_s & 0x7) << 0;
    s0 |= (wrap_t & 0x7) << 3;
    s0 |= (wrap_r & 0x7) << 6;
    s0 |= (min_img & 0x3) << 9;
    s0 |= (min_mip & 0x3) << 11;
    s0 |= (mag_img & 0x3) << 13;

    buf.push(virgl_cmd0(
        VIRGL_CCMD_CREATE_OBJECT,
        VIRGL_OBJECT_SAMPLER_STATE,
        VIRGL_OBJ_SAMPLER_STATE_SIZE,
    ));
    buf.push(sampler_handle);
    buf.push(s0);
    buf.push(fui(0.0)); // lod_bias
    buf.push(fui(0.0)); // min_lod
    buf.push(fui(1000.0)); // max_lod
    // border color (rgba) - not used for clamp-to-edge.
    buf.push(0);
    buf.push(0);
    buf.push(0);
    buf.push(0);

    let _ = PIPE_TEX_WRAP_REPEAT;
    let _ = PIPE_TEX_WRAP_CLAMP_TO_EDGE;
    let _ = PIPE_TEX_FILTER_NEAREST;
    let _ = PIPE_TEX_FILTER_LINEAR;
    let _ = PIPE_TEX_MIPFILTER_NONE;
}

fn encode_create_sampler_view(
    buf: &mut VirglCmdBuf,
    view_handle: u32,
    res_handle: u32,
    format: u32,
) {
    // virgl_protocol.h: VIRGL_OBJ_SAMPLER_VIEW_SIZE = 6
    const VIRGL_OBJ_SAMPLER_VIEW_SIZE: u32 = 6;

    // virgl_protocol.h packs 4 swizzles (3 bits each). Values match Gallium's
    // enum pipe_swizzle: X=0, Y=1, Z=2, W=3, 0=4, 1=5, NONE=6.
    // Identity RGBA swizzle is required for correct sampling.
    const PIPE_SWIZZLE_X: u32 = 0;
    const PIPE_SWIZZLE_Y: u32 = 1;
    const PIPE_SWIZZLE_Z: u32 = 2;
    const PIPE_SWIZZLE_W: u32 = 3;
    let swizzle = ((PIPE_SWIZZLE_X & 0x7) << 0)
        | ((PIPE_SWIZZLE_Y & 0x7) << 3)
        | ((PIPE_SWIZZLE_Z & 0x7) << 6)
        | ((PIPE_SWIZZLE_W & 0x7) << 9);

    buf.push(virgl_cmd0(
        VIRGL_CCMD_CREATE_OBJECT,
        VIRGL_OBJECT_SAMPLER_VIEW,
        VIRGL_OBJ_SAMPLER_VIEW_SIZE,
    ));
    buf.push(view_handle);
    buf.push(res_handle);
    buf.push(format);
    buf.push(0); // texture_layer / first element
    buf.push(0); // texture_level / last element
    buf.push(swizzle);
}

fn encode_set_sampler_views(
    buf: &mut VirglCmdBuf,
    shader_type: u32,
    start_slot: u32,
    views: &[u32],
) {
    let num = views.len().min(32) as u32;
    // virgl expects: shader_type, start_slot, then view handles.
    // Count is encoded in command length (VIRGL_SET_SAMPLER_VIEWS_SIZE).
    let len = num + 2;
    buf.push(virgl_cmd0(VIRGL_CCMD_SET_SAMPLER_VIEWS, 0, len));
    buf.push(shader_type);
    buf.push(start_slot);
    for i in 0..(num as usize) {
        buf.push(views[i]);
    }
}

fn encode_bind_sampler_states(
    buf: &mut VirglCmdBuf,
    shader_type: u32,
    start_slot: u32,
    states: &[u32],
) {
    let num = states.len().min(32) as u32;
    // virgl expects: shader_type, start_slot, then state handles.
    // Count is encoded in command length (VIRGL_BIND_SAMPLER_STATES).
    let len = num + 2;
    buf.push(virgl_cmd0(VIRGL_CCMD_BIND_SAMPLER_STATES, 0, len));
    buf.push(shader_type);
    buf.push(start_slot);
    for i in 0..(num as usize) {
        buf.push(states[i]);
    }
}

fn encode_inline_write_buffer_at(
    buf: &mut VirglCmdBuf,
    res_handle: u32,
    dst_offset: u32,
    data: &[u8],
) {
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
    buf.push(dst_offset); // box x (byte offset for PIPE_BUFFER)
    buf.push(0); // box y
    buf.push(0); // box z
    buf.push(data.len() as u32); // box width
    buf.push(1); // box height
    buf.push(1); // box depth
    buf.push_bytes_padded(data);
}

fn encode_inline_write_texture(
    buf: &mut VirglCmdBuf,
    res_handle: u32,
    width: u32,
    height: u32,
    rgba: &[u8],
) {
    // Matches virgl_encoder_inline_write() with a provided box for a 2D texture.
    // VIRGL_CMD0 stores payload length in 16 bits (dwords), so large textures must
    // be split into multiple RESOURCE_INLINE_WRITE commands.
    let expected = (width as usize)
        .saturating_mul(height as usize)
        .saturating_mul(4);
    let data = if rgba.len() >= expected {
        &rgba[..expected]
    } else {
        rgba
    };
    let stride = width.saturating_mul(4);
    if stride == 0 || height == 0 || data.is_empty() {
        return;
    }

    // VIRGL RESOURCE_INLINE_WRITE command has 11 dwords before inline payload.
    // cmd0 length field is 16-bit dwords, so keep each command at <= 0xFFFF dwords.
    let max_payload_dwords = 0xFFFFu32.saturating_sub(11);
    let max_payload_bytes = (max_payload_dwords as usize).saturating_mul(4);
    let rows_per_chunk = (max_payload_bytes / stride as usize).max(1);
    let total_rows = (data.len() / stride as usize).min(height as usize);

    let mut row = 0usize;
    while row < total_rows {
        let chunk_rows = (total_rows - row).min(rows_per_chunk);
        let chunk_bytes = chunk_rows.saturating_mul(stride as usize);
        let byte_off = row.saturating_mul(stride as usize);
        let chunk = &data[byte_off..byte_off + chunk_bytes];
        let data_dwords = (chunk.len() as u32).div_ceil(4);

        buf.push(virgl_cmd0(
            VIRGL_CCMD_RESOURCE_INLINE_WRITE,
            0,
            data_dwords + 11,
        ));
        buf.push(res_handle);
        buf.push(0); // level
        buf.push(0); // usage
        buf.push(stride); // stride
        buf.push(stride.saturating_mul(chunk_rows as u32)); // layer_stride
        buf.push(0); // box x
        buf.push(row as u32); // box y
        buf.push(0); // box z
        buf.push(width); // box width
        buf.push(chunk_rows as u32); // box height
        buf.push(1); // box depth
        buf.push_bytes_padded(chunk);

        row += chunk_rows;
    }
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

fn encode_set_scissor(buf: &mut VirglCmdBuf, min_x: u32, min_y: u32, max_x: u32, max_y: u32) {
    buf.push(virgl_cmd0(VIRGL_CCMD_SET_SCISSOR_STATE, 0, 3));
    buf.push(0); // start_slot
    buf.push((min_x & 0xffff) | ((min_y & 0xffff) << 16));
    buf.push((max_x & 0xffff) | ((max_y & 0xffff) << 16));
}

fn encode_create_blend(
    buf: &mut VirglCmdBuf,
    blend_handle: u32,
    enabled: bool,
    src_factor: u32,
    dst_factor: u32,
) {
    buf.push(virgl_cmd0(
        VIRGL_CCMD_CREATE_OBJECT,
        VIRGL_OBJECT_BLEND,
        VIRGL_OBJ_BLEND_SIZE,
    ));
    // Values from Mesa pipe/p_defines.h (virgl uses the Gallium enums).
    // NOTE: We only rely on a tiny subset that we have validated with virglrenderer.
    const PIPE_BLEND_ADD: u32 = 0;
    const PIPE_BLENDFACTOR_ONE: u32 = 1;

    buf.push(blend_handle);
    buf.push(0); // s0
    buf.push(0); // s1
    for i in 0..8u32 {
        let mut rt = 0u32;
        if i == 0 {
            if enabled {
                // Enable blending on RT0.
                rt |= 1 << 0;
                rt |= (PIPE_BLEND_ADD & 0x7) << 1;
                rt |= (src_factor & 0x1f) << 4;
                rt |= (dst_factor & 0x1f) << 9;
                // Alpha blend: mirror RGB factors.
                rt |= (PIPE_BLEND_ADD & 0x7) << 14;
                rt |= (src_factor & 0x1f) << 17;
                rt |= (dst_factor & 0x1f) << 22;
            }
            // Write all color channels.
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

fn encode_create_rasterizer(buf: &mut VirglCmdBuf, rs_handle: u32, scissor_enable: bool) {
    buf.push(virgl_cmd0(
        VIRGL_CCMD_CREATE_OBJECT,
        VIRGL_OBJECT_RASTERIZER,
        VIRGL_OBJ_RS_SIZE,
    ));
    buf.push(rs_handle);

    let mut s0 = 0u32;
    // depth_clip=1
    s0 |= 1 << 1;
    if scissor_enable {
        s0 |= 1 << 14;
    }
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

static VIRGL_NEXT_CTX_ID: AtomicU32 = AtomicU32::new(1);
static VIRGL_NEXT_RES_ID: AtomicU32 = AtomicU32::new(1);
static VIRGL_NEXT_OBJ_HANDLE: AtomicU32 = AtomicU32::new(64);

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

fn alloc_res_id() -> u32 {
    // resource_id 0 is reserved.
    let id = VIRGL_NEXT_RES_ID.fetch_add(1, Ordering::Relaxed);
    if id == 0 { 1 } else { id }
}

fn alloc_obj_handle() -> u32 {
    // object handle 0 is reserved.
    let id = VIRGL_NEXT_OBJ_HANDLE.fetch_add(1, Ordering::Relaxed);
    if id == 0 { 64 } else { id }
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

// Best-effort virtio-gpu presence cache.
//
// Boot code may want to wait for virtio-gpu to appear before switching to the virgl backend.
// We keep this probe cheap and avoid re-enumerating PCI on every poll.
static VIRTIO_GPU_PRESENT_CACHE: AtomicU8 = AtomicU8::new(0);

/// Returns true if a virtio-gpu device is present.
///
/// This performs a single PCI re-enumeration the first time it is called while the device list
/// is empty or stale, then caches the result.
pub fn is_present_cached() -> bool {
    match VIRTIO_GPU_PRESENT_CACHE.load(Ordering::Acquire) {
        2 => return true,
        1 => return false,
        _ => {}
    }

    let mut present = find_device().is_some();
    if !present {
        // The PCI list can be empty early in boot. Re-enumerate once before deciding.
        pci::enumerate_impl();
        present = find_device().is_some();
    }

    VIRTIO_GPU_PRESENT_CACHE.store(if present { 2 } else { 1 }, Ordering::Release);
    present
}

fn as_bytes<T: Copy>(value: &T) -> &[u8] {
    unsafe {
        core::slice::from_raw_parts((value as *const T) as *const u8, core::mem::size_of::<T>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_write_texture_splits_512x512_rgba_without_len_overflow() {
        let width = 512u32;
        let height = 512u32;
        let bytes = vec![0xABu8; (width as usize) * (height as usize) * 4];

        let mut cmd = VirglCmdBuf::new();
        encode_inline_write_texture(&mut cmd, 123, width, height, &bytes);

        let dwords = cmd.dwords.as_slice();
        assert!(!dwords.is_empty());

        let mut i = 0usize;
        let mut commands = 0usize;
        let mut total_payload_bytes = 0usize;
        let mut expected_box_y = 0u32;
        while i < dwords.len() {
            let cmd0 = dwords[i];
            let cmd_type = (cmd0 & 0xFF) as u8;
            let len_dwords = cmd0 >> 16;
            assert_eq!(cmd_type, VIRGL_CCMD_RESOURCE_INLINE_WRITE);
            assert!(len_dwords <= 0xFFFF);
            assert!(len_dwords >= 11);

            let payload_start = i + 1;
            let payload_end = payload_start + (len_dwords as usize);
            assert!(payload_end <= dwords.len());

            let stride = dwords[payload_start + 3];
            let box_y = dwords[payload_start + 6];
            let box_w = dwords[payload_start + 8];
            let box_h = dwords[payload_start + 9];
            assert_eq!(stride, width * 4);
            assert_eq!(box_w, width);
            assert!(box_h > 0);
            assert_eq!(box_y, expected_box_y);

            let data_dwords = len_dwords as usize - 11;
            let payload_bytes = data_dwords * 4;
            assert_eq!(payload_bytes, (box_h as usize) * (stride as usize));

            total_payload_bytes += payload_bytes;
            expected_box_y += box_h;
            commands += 1;
            i = payload_end;
        }

        assert!(commands > 1);
        assert_eq!(expected_box_y, height);
        assert_eq!(total_payload_bytes, bytes.len());
    }
}
