extern crate alloc;

use core::sync::atomic::{AtomicU8, Ordering, fence};

use embassy_executor::{SpawnError, SpawnToken};
use embassy_time::{Duration as EmbassyDuration, Timer};

use crate::{pci, wait};

const LOGO_JPEG: &[u8] = include_bytes!("../logo.jpg");

const VIRTIO_PCI_VENDOR: u16 = 0x1AF4;
const VIRTIO_GPU_DEVICE_MODERN: u16 = 0x1050;
const VIRTIO_GPU_DEVICE_TRANSITIONAL: u16 = 0x1010;

const PCI_CAP_PTR: u16 = 0x34;
const PCI_CAP_ID_VENDOR_SPECIFIC: u8 = 0x09;

const VIRTIO_PCI_COMMAND_OFFSET: u16 = 0x04;
const VIRTIO_PCI_COMMAND_MEM: u16 = 1 << 1;
const VIRTIO_PCI_COMMAND_BUS_MASTER: u16 = 1 << 2;

const VIRTIO_STATUS_ACK: u8 = 0x01;
const VIRTIO_STATUS_DRIVER: u8 = 0x02;
const VIRTIO_STATUS_DRIVER_OK: u8 = 0x04;
const VIRTIO_STATUS_FEATURES_OK: u8 = 0x08;
const VIRTIO_STATUS_FAILED: u8 = 0x80;

const VIRTIO_F_VERSION_1: u64 = 1u64 << 32;

const VIRTIO_PCI_CAP_COMMON_CFG: u8 = 1;
const VIRTIO_PCI_CAP_NOTIFY_CFG: u8 = 2;
const VIRTIO_PCI_CAP_ISR_CFG: u8 = 3;
const VIRTIO_PCI_CAP_DEVICE_CFG: u8 = 4;

const VIRTIO_GPU_CMD_GET_DISPLAY_INFO: u32 = 0x0100;
const VIRTIO_GPU_CMD_RESOURCE_CREATE_2D: u32 = 0x0101;
const VIRTIO_GPU_CMD_SET_SCANOUT: u32 = 0x0103;
const VIRTIO_GPU_CMD_RESOURCE_FLUSH: u32 = 0x0104;
const VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D: u32 = 0x0105;
const VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING: u32 = 0x0106;
const VIRTIO_GPU_CMD_UPDATE_CURSOR: u32 = 0x0300;

const VIRTIO_GPU_RESP_OK_NODATA: u32 = 0x1100;
const VIRTIO_GPU_RESP_OK_DISPLAY_INFO: u32 = 0x1101;

const VIRTIO_GPU_FORMAT_B8G8R8A8_UNORM: u32 = 1;

const QUEUE_CONTROL: u16 = 0;
const QUEUE_CURSOR: u16 = 1;

const VIRTQ_DESC_F_NEXT: u16 = 1;
const VIRTQ_DESC_F_WRITE: u16 = 2;

const SCANOUT_RESOURCE_ID: u32 = 1;
const CURSOR_RESOURCE_ID: u32 = 2;
const CURSOR_DIM: u32 = 64;
const CURSOR_HOTSPOT: u32 = 32;
const CURSOR_ALPHA: u8 = 0x80;
const CURSOR_UPDATE_MS: u64 = 16;

static VIRTIO_GPU_PRESENT_CACHE: AtomicU8 = AtomicU8::new(0);

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

    fn flush(&self) {
        crate::intel::dma_cache_flush_range(self.virt as *const u8, self.len);
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

#[derive(Clone, Copy)]
struct VirtioModernCaps {
    common_phys: u64,
    common_len: u32,
    notify_phys: u64,
    notify_len: u32,
    notify_mult: u32,
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
struct CursorPos {
    scanout_id: u32,
    x: u32,
    y: u32,
    padding: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CmdUpdateCursor {
    hdr: CtrlHdr,
    pos: CursorPos,
    resource_id: u32,
    hot_x: u32,
    hot_y: u32,
    padding: u32,
}

struct VirtioGpuLogo {
    notify: core::ptr::NonNull<u8>,
    notify_mult: u32,
    ctrlq: VirtQueue,
    cursorq: VirtQueue,
    req: DmaRegion,
    resp: DmaRegion,
}

unsafe impl Send for VirtioGpuLogo {}

impl VirtioGpuLogo {
    fn init_first() -> Option<Self> {
        let dev = find_device().or_else(|| {
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
        let notify = core::ptr::NonNull::new(notify_map.as_ptr())?;

        if !negotiate_modern_2d(common) {
            return None;
        }

        let ctrlq = setup_queue_modern(common, QUEUE_CONTROL).ok()?;
        let cursorq = setup_queue_modern(common, QUEUE_CURSOR).ok()?;

        unsafe {
            let c = common.as_ptr();
            let mut status = core::ptr::read_volatile(&(*c).device_status);
            status |= VIRTIO_STATUS_DRIVER_OK;
            core::ptr::write_volatile(&mut (*c).device_status, status);
        }

        let req = DmaRegion::alloc(4096, 16)?;
        let resp = DmaRegion::alloc(4096, 16)?;

        Some(Self {
            notify,
            notify_mult: caps.notify_mult,
            ctrlq,
            cursorq,
            req,
            resp,
        })
    }

    fn get_display_info(&mut self) -> Option<(u32, u32, u32)> {
        let req = CmdGetDisplayInfo {
            hdr: CtrlHdr {
                type_: VIRTIO_GPU_CMD_GET_DISPLAY_INFO,
                ..CtrlHdr::default()
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
        info.pmodes
            .iter()
            .enumerate()
            .find(|(_, m)| m.r.width != 0 && m.r.height != 0)
            .map(|(i, m)| (i as u32, m.r.width, m.r.height))
    }

    fn resource_create_2d(&mut self, resource_id: u32, width: u32, height: u32) -> bool {
        let req = CmdResourceCreate2d {
            hdr: CtrlHdr {
                type_: VIRTIO_GPU_CMD_RESOURCE_CREATE_2D,
                ..CtrlHdr::default()
            },
            resource_id,
            format: VIRTIO_GPU_FORMAT_B8G8R8A8_UNORM,
            width,
            height,
        };
        self.ctrl_submit_bytes(as_bytes(&req))
    }

    fn resource_attach_backing(&mut self, resource_id: u32, backing: &DmaRegion) -> bool {
        let Ok(backing_len) = u32::try_from(backing.len()) else {
            return false;
        };
        let header = CmdResourceAttachBacking {
            hdr: CtrlHdr {
                type_: VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING,
                ..CtrlHdr::default()
            },
            resource_id,
            nr_entries: 1,
        };
        let entry = MemEntry {
            addr: backing.phys(),
            length: backing_len,
            padding: 0,
        };

        let header_bytes = as_bytes(&header);
        let entry_bytes = as_bytes(&entry);
        let total = header_bytes.len().saturating_add(entry_bytes.len());
        if total > self.req.len() {
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

    fn set_scanout(&mut self, scanout_id: u32, resource_id: u32, width: u32, height: u32) -> bool {
        let req = CmdSetScanout {
            hdr: CtrlHdr {
                type_: VIRTIO_GPU_CMD_SET_SCANOUT,
                ..CtrlHdr::default()
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
        self.ctrl_submit_bytes(as_bytes(&req))
    }

    fn transfer_to_host_2d(&mut self, resource_id: u32, width: u32, height: u32) -> bool {
        let req = CmdTransferToHost2d {
            hdr: CtrlHdr {
                type_: VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D,
                ..CtrlHdr::default()
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

    fn resource_flush(&mut self, resource_id: u32, width: u32, height: u32) -> bool {
        let req = CmdResourceFlush {
            hdr: CtrlHdr {
                type_: VIRTIO_GPU_CMD_RESOURCE_FLUSH,
                ..CtrlHdr::default()
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

    fn update_cursor(&mut self, scanout_id: u32, resource_id: u32, x: u32, y: u32) -> bool {
        let req = CmdUpdateCursor {
            hdr: CtrlHdr {
                type_: VIRTIO_GPU_CMD_UPDATE_CURSOR,
                ..CtrlHdr::default()
            },
            pos: CursorPos {
                scanout_id,
                x,
                y,
                padding: 0,
            },
            resource_id,
            hot_x: CURSOR_HOTSPOT,
            hot_y: CURSOR_HOTSPOT,
            padding: 0,
        };
        self.cursor_submit_bytes(as_bytes(&req))
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
            Some(&[VIRTIO_GPU_RESP_OK_DISPLAY_INFO]),
        ) {
            return None;
        }
        Some(unsafe { core::ptr::read_unaligned(self.resp.virt() as *const u32) })
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
            Some(&[VIRTIO_GPU_RESP_OK_NODATA]),
        )
    }

    fn cursor_submit_bytes(&mut self, req_bytes: &[u8]) -> bool {
        if req_bytes.is_empty() || req_bytes.len() > self.req.len() {
            return false;
        }
        unsafe {
            core::ptr::copy_nonoverlapping(req_bytes.as_ptr(), self.req.virt(), req_bytes.len());
            core::ptr::write_bytes(self.resp.virt(), 0, self.resp.len());
        }
        Self::submit_desc_chain_on(
            self.notify,
            self.notify_mult,
            &mut self.cursorq,
            &self.req,
            &self.resp,
            req_bytes.len(),
            "cursorq",
            None,
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
        req.flush();
        let wants_response = expected_resp_types.is_some();
        unsafe {
            let d0 = &mut *queue.desc.add(0);
            d0.addr = req.phys();
            d0.len = req_len as u32;
            d0.flags = if wants_response { VIRTQ_DESC_F_NEXT } else { 0 };
            d0.next = if wants_response { 1 } else { 0 };

            if wants_response {
                let d1 = &mut *queue.desc.add(1);
                d1.addr = resp.phys();
                d1.len = resp.len() as u32;
                d1.flags = VIRTQ_DESC_F_WRITE;
                d1.next = 0;
            }
        }

        queue.push_avail(0);
        queue._mem.flush();
        fence(Ordering::Release);
        notify_queue_modern(notify, notify_mult, queue.queue_index, queue.notify_off);

        let ok = wait::spin_until_timeout_no_exec(1000, || queue.used_idx() != queue.last_used_idx);
        if !ok {
            crate::log!("virtio-gpu-ui: {} timeout\n", queue_label);
            return false;
        }

        fence(Ordering::Acquire);
        let used = queue.used_elem(queue.last_used_idx % queue.size);
        queue.last_used_idx = queue.last_used_idx.wrapping_add(1);
        if used.id != 0 {
            crate::log!("virtio-gpu-ui: {} used id={} expected=0\n", queue_label, used.id);
            return false;
        }

        let Some(expected) = expected_resp_types else {
            return true;
        };
        let resp_type = unsafe { core::ptr::read_unaligned(resp.virt() as *const u32) };
        let ok = expected.contains(&resp_type);
        if !ok {
            let req_type = unsafe { core::ptr::read_unaligned(req.virt() as *const u32) };
            crate::log!(
                "virtio-gpu-ui: {} bad resp req=0x{:08X} type=0x{:08X} req_len={}\n",
                queue_label,
                req_type,
                resp_type,
                req_len
            );
        }
        ok
    }
}

struct EmulatorUi {
    gpu: VirtioGpuLogo,
    scanout_id: u32,
    width: u32,
    height: u32,
    _scanout_backing: DmaRegion,
    _cursor_backing: DmaRegion,
    last_cursor: Option<(u32, u32, u32, u32)>,
}

unsafe impl Send for EmulatorUi {}

impl EmulatorUi {
    fn init() -> Option<Self> {
        let mut gpu = VirtioGpuLogo::init_first()?;
        let (scanout_id, width, height) = gpu.get_display_info()?;
        let scanout_bytes = bytes_for_surface(width, height)?;
        let scanout_backing = DmaRegion::alloc(scanout_bytes, 4096)?;

        let logo = match crate::ui3::img::jpeg_codec::decode_jpeg_rgba(LOGO_JPEG) {
            Ok(logo) => logo,
            Err(err) => {
                crate::log!("virtio-gpu-ui: logo decode failed code={}\n", err.code());
                return None;
            }
        };
        let copy = draw_centered_logo(scanout_backing.virt(), width, height, &logo);
        scanout_backing.flush();

        if !gpu.resource_create_2d(SCANOUT_RESOURCE_ID, width, height)
            || !gpu.resource_attach_backing(SCANOUT_RESOURCE_ID, &scanout_backing)
            || !gpu.set_scanout(scanout_id, SCANOUT_RESOURCE_ID, width, height)
            || !gpu.transfer_to_host_2d(SCANOUT_RESOURCE_ID, width, height)
            || !gpu.resource_flush(SCANOUT_RESOURCE_ID, width, height)
        {
            crate::log!("virtio-gpu-ui: scanout present failed\n");
            return None;
        }

        let cursor_bytes = bytes_for_surface(CURSOR_DIM, CURSOR_DIM)?;
        let cursor_backing = DmaRegion::alloc(cursor_bytes, 4096)?;
        fill_cursor_sprite(cursor_backing.virt());
        cursor_backing.flush();
        let cursor_ok = gpu.resource_create_2d(CURSOR_RESOURCE_ID, CURSOR_DIM, CURSOR_DIM)
            && gpu.resource_attach_backing(CURSOR_RESOURCE_ID, &cursor_backing)
            && gpu.transfer_to_host_2d(CURSOR_RESOURCE_ID, CURSOR_DIM, CURSOR_DIM);

        crate::log!(
            "virtio-gpu-ui: logo presented scanout={} size={}x{} logo={}x{} copy={}x{} src={},{} dst={},{} cursor={}\n",
            scanout_id,
            width,
            height,
            logo.width,
            logo.height,
            copy.copy_w,
            copy.copy_h,
            copy.src_x,
            copy.src_y,
            copy.dst_x,
            copy.dst_y,
            cursor_ok as u8
        );

        Some(Self {
            gpu,
            scanout_id,
            width,
            height,
            _scanout_backing: scanout_backing,
            _cursor_backing: cursor_backing,
            last_cursor: None,
        })
    }

    fn update_cursor(&mut self) {
        let (slot, x, y, buttons) = cursor_snapshot_to_pixels(self.width, self.height).unwrap_or((
            0,
            self.width / 2,
            self.height / 2,
            0,
        ));
        let state = (slot, x, y, buttons);
        if self.last_cursor == Some(state) {
            return;
        }
        if self
            .gpu
            .update_cursor(self.scanout_id, CURSOR_RESOURCE_ID, x, y)
        {
            self.last_cursor = Some(state);
        }
    }
}

#[embassy_executor::task]
async fn emulator_ui_service_task() {
    crate::log_info!(
        target: "gfx";
        "boot-probe: virtio-gpu-ui task start\n"
    );
    let Some(mut ui) = EmulatorUi::init() else {
        crate::log_warn!(
            target: "gfx";
            "virtio-gpu-ui: init failed\n"
        );
        return;
    };

    loop {
        ui.update_cursor();
        Timer::after(EmbassyDuration::from_millis(CURSOR_UPDATE_MS)).await;
    }
}

pub(crate) fn emulator_ui_task() -> Result<SpawnToken<impl Send>, SpawnError> {
    emulator_ui_service_task()
}

pub(crate) fn present() -> bool {
    match VIRTIO_GPU_PRESENT_CACHE.load(Ordering::Acquire) {
        1 => true,
        2 => false,
        _ => {
            let present = find_device().is_some();
            VIRTIO_GPU_PRESENT_CACHE.store(if present { 1 } else { 2 }, Ordering::Release);
            present
        }
    }
}

fn cursor_snapshot_to_pixels(width: u32, height: u32) -> Option<(u32, u32, u32, u32)> {
    let (slot, nx, ny, buttons) =
        crate::r::cursor::preferred_kernel_hw_cursor_snapshot_with_slot_buttons()?;
    let x = normalized_cursor_to_px(nx, width);
    let y = normalized_cursor_to_px(ny, height);
    Some((slot, x, y, buttons))
}

fn normalized_cursor_to_px(norm: f64, extent: u32) -> u32 {
    let limit = extent.saturating_sub(1) as f64;
    let pixel = (norm.clamp(0.0, 1.0) * limit + 0.5) as i64 - i64::from(CURSOR_HOTSPOT);
    pixel.clamp(0, i64::from(extent.saturating_sub(1))) as u32
}

#[derive(Clone, Copy)]
struct LogoCopy {
    src_x: u32,
    src_y: u32,
    dst_x: u32,
    dst_y: u32,
    copy_w: u32,
    copy_h: u32,
}

fn draw_centered_logo(
    dst: *mut u8,
    dst_w: u32,
    dst_h: u32,
    logo: &crate::ui3::img::jpeg_codec::DecodedJpeg,
) -> LogoCopy {
    let dst_len = bytes_for_surface(dst_w, dst_h).unwrap_or(0);
    unsafe { core::ptr::write_bytes(dst, 0, dst_len) };

    let copy_w = dst_w.min(logo.width);
    let copy_h = dst_h.min(logo.height);
    let src_x = logo.width.saturating_sub(copy_w) / 2;
    let src_y = logo.height.saturating_sub(copy_h) / 2;
    let dst_x = dst_w.saturating_sub(copy_w) / 2;
    let dst_y = dst_h.saturating_sub(copy_h) / 2;

    for y in 0..copy_h {
        let src_row = ((src_y + y) as usize)
            .saturating_mul(logo.width as usize)
            .saturating_add(src_x as usize)
            .saturating_mul(4);
        let dst_row = ((dst_y + y) as usize)
            .saturating_mul(dst_w as usize)
            .saturating_add(dst_x as usize)
            .saturating_mul(4);
        for x in 0..copy_w as usize {
            let si = src_row + x * 4;
            let di = dst_row + x * 4;
            let r = logo.rgba[si];
            let g = logo.rgba[si + 1];
            let b = logo.rgba[si + 2];
            let a = logo.rgba[si + 3] as u16;
            let pr = ((r as u16 * a) / 255) as u8;
            let pg = ((g as u16 * a) / 255) as u8;
            let pb = ((b as u16 * a) / 255) as u8;
            unsafe {
                *dst.add(di) = pb;
                *dst.add(di + 1) = pg;
                *dst.add(di + 2) = pr;
                *dst.add(di + 3) = 0xFF;
            }
        }
    }

    LogoCopy {
        src_x,
        src_y,
        dst_x,
        dst_y,
        copy_w,
        copy_h,
    }
}

fn fill_cursor_sprite(dst: *mut u8) {
    let pitch = CURSOR_DIM as usize * 4;
    for y in 0..CURSOR_DIM as usize {
        for x in 0..CURSOR_DIM as usize {
            let off = y * pitch + x * 4;
            let v = if x < CURSOR_DIM as usize / 2 { 0 } else { 0xFF };
            unsafe {
                *dst.add(off) = v;
                *dst.add(off + 1) = v;
                *dst.add(off + 2) = v;
                *dst.add(off + 3) = CURSOR_ALPHA;
            }
        }
    }
}

fn bytes_for_surface(width: u32, height: u32) -> Option<usize> {
    (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)
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

fn as_bytes<T: Sized>(v: &T) -> &[u8] {
    unsafe { core::slice::from_raw_parts((v as *const T) as *const u8, core::mem::size_of::<T>()) }
}

fn bar_mem_base(dev: &pci::PciDevice, bar_index: u8) -> Option<u64> {
    let (lo, hi) = pci::read_bar_raw(dev.bus, dev.slot, dev.function, bar_index);
    if (lo & 0x1) != 0 {
        return None;
    }
    let is_64 = ((lo >> 1) & 0x3) == 0x2;
    let base_lo = (lo & 0xFFFF_FFF0) as u64;
    if is_64 {
        Some(((hi? as u64) << 32) | base_lo)
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

fn parse_modern_caps(dev: &pci::PciDevice) -> Option<VirtioModernCaps> {
    let mut ptr = pci::config_read_u8(dev.bus, dev.slot, dev.function, PCI_CAP_PTR);
    if ptr == 0 {
        return None;
    }

    let mut common: Option<VirtioPciCap> = None;
    let mut notify: Option<VirtioPciNotifyCap> = None;

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
                VIRTIO_PCI_CAP_NOTIFY_CFG => {
                    if let Some(ncap) = read_virtio_notify_cap(dev, ptr) {
                        notify = Some(ncap);
                    }
                }
                VIRTIO_PCI_CAP_ISR_CFG | VIRTIO_PCI_CAP_DEVICE_CFG => {}
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
    let common_phys = bar_mem_base(dev, common.bar)?.checked_add(common.offset as u64)?;
    let notify_phys = bar_mem_base(dev, notify.cap.bar)?.checked_add(notify.cap.offset as u64)?;

    Some(VirtioModernCaps {
        common_phys,
        common_len: common.length,
        notify_phys,
        notify_len: notify.cap.length,
        notify_mult: notify.notify_off_multiplier,
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

    let size = size_max.min(8).max(2);
    let desc_size = size as usize * core::mem::size_of::<VirtqDesc>();
    let avail_size = 4 + (size as usize * 2);
    let used_offset = align_up(desc_size + avail_size, 4096);
    let used_size = 4 + (size as usize * 8);
    let total = align_up(used_offset + used_size, 4096);

    let mem = DmaRegion::alloc(total, 4096).ok_or(())?;
    unsafe { core::ptr::write_bytes(mem.virt(), 0, total) };
    mem.flush();

    let desc = mem.virt() as *mut VirtqDesc;
    let avail = unsafe { mem.virt().add(desc_size) };
    let used = unsafe { mem.virt().add(used_offset) };

    unsafe {
        core::ptr::write_volatile(&mut (*common).queue_size, size);
        core::ptr::write_volatile(&mut (*common).queue_msix_vector, 0xFFFF);
        core::ptr::write_volatile(&mut (*common).queue_desc, mem.phys());
        core::ptr::write_volatile(
            &mut (*common).queue_avail,
            mem.phys().saturating_add(desc_size as u64),
        );
        core::ptr::write_volatile(
            &mut (*common).queue_used,
            mem.phys().saturating_add(used_offset as u64),
        );
    }

    fence(Ordering::Release);

    unsafe {
        core::ptr::write_volatile(&mut (*common).queue_enable, 1);
        if core::ptr::read_volatile(&(*common).queue_enable) != 1 {
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
        let ptr = notify_base.as_ptr().add(off) as *mut u16;
        core::ptr::write_volatile(ptr, queue_index);
    }
}

fn negotiate_modern_2d(common: core::ptr::NonNull<VirtioPciCommonCfg>) -> bool {
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

        let guest_features = dev_features & VIRTIO_F_VERSION_1;

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
    let mut found = None;
    pci::with_devices(|list| {
        for dev in list {
            if dev.vendor == VIRTIO_PCI_VENDOR
                && (dev.device == VIRTIO_GPU_DEVICE_MODERN
                    || dev.device == VIRTIO_GPU_DEVICE_TRANSITIONAL)
            {
                found = Some(*dev);
                break;
            }
        }
    });
    found
}
