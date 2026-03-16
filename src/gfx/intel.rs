use core::ptr::{NonNull, write_bytes};

use embassy_time::{Duration as EmbassyDuration, Timer};
use heapless::Vec;
use spin::Mutex;

const INTEL_VENDOR_ID: u16 = 0x8086;
const PCI_CLASS_DISPLAY: u8 = 0x03;
const MAX_INTEL_DEVICES: usize = 4;
const CMD_SCRATCH_BYTES: usize = 8192;
const INTEL_RPL_S_GT1_DEVICE_ID: u16 = 0xA780;
const INTEL_BAR_SANITY_LIMIT: u64 = 0x40_0000_0000;
const INTEL_PLANE_ENABLE: u32 = 1 << 31;
const INTEL_PIPE_A_SRC: usize = 0x6001C;
const INTEL_PIPE_B_SRC: usize = 0x6101C;
const INTEL_PIPE_C_SRC: usize = 0x6201C;
const INTEL_PIPE_D_SRC: usize = 0x6301C;
const INTEL_TRANS_A_DDI_FUNC_CTL: usize = 0x60400;
const INTEL_TRANS_B_DDI_FUNC_CTL: usize = 0x61400;
const INTEL_TRANS_C_DDI_FUNC_CTL: usize = 0x62400;
const INTEL_TRANS_D_DDI_FUNC_CTL: usize = 0x63400;
const INTEL_UNI_PLANE_BASE: usize = 0x70180;
const INTEL_UNI_PLANE_PIPE_STRIDE: usize = 0x1000;
const INTEL_UNI_PLANE_SLOT_STRIDE: usize = 0x100;
const INTEL_UNI_PLANE_STRIDE_OFF: usize = 0x08;
const INTEL_UNI_PLANE_SURF_OFF: usize = 0x1C;
const INTEL_UNI_PLANE_SURFLIVE_OFF: usize = 0x2C;
const INTEL_SCANOUT_RETRIES: u32 = 40;
const INTEL_SCANOUT_RETRY_MS: u64 = 100;
const INTEL_SCANOUT_DRAW_W: usize = 512;
const INTEL_SCANOUT_DRAW_H: usize = 192;
const INTEL_DISPLAY_SWEEP_START: usize = 0x60000;
const INTEL_DISPLAY_SWEEP_END: usize = 0x74000;
const INTEL_DISPLAY_PAGE_STRIDE: usize = 0x1000;
const INTEL_DISPLAY_SWEEP_LOG_LIMIT: usize = 16;
const INTEL_DISPLAY_WINDOW_DWORDS: usize = 8;
const INTEL_PLANE_WRITE_SMOKE_STRIDE_BASE: u32 = 0x200;
const INTEL_PLANE_WRITE_SMOKE_SURF_BASE: u32 = 0x0100_0000;
const INTEL_PCI_BDSM: u16 = 0x5C;
const INTEL_PCI_BGSM: u16 = 0x70;
const INTEL_PCI_ASLS: u16 = 0xFC;
const INTEL_OPREGION_PROBE_BYTES: usize = 0x40;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum IntelPlatform {
    Unknown,
    RaptorLake,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum IntelSubmissionModel {
    LegacyRing,
    Modern,
}

#[derive(Copy, Clone, Debug)]
pub struct IntelGfxInfo {
    pub bus: u8,
    pub slot: u8,
    pub function: u8,
    pub device_id: u16,
    pub platform: IntelPlatform,
    pub submission: IntelSubmissionModel,
    pub bar_phys: u64,
    pub bar_size: u64,
    pub aperture_bar_phys: u64,
    pub aperture_bar_size: u64,
    pub mmio_base: NonNull<u8>,
    pub mmio_len: usize,
    pub cmd_scratch_phys: u64,
    pub cmd_scratch_virt: *mut u8,
    pub cmd_scratch_len: usize,
}

unsafe impl Send for IntelGfxInfo {}
unsafe impl Sync for IntelGfxInfo {}

impl IntelPlatform {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::RaptorLake => "raptorlake",
        }
    }
}

impl IntelSubmissionModel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LegacyRing => "legacy-ring",
            Self::Modern => "modern",
        }
    }
}

impl IntelGfxInfo {
    pub fn platform_name(&self) -> &'static str {
        self.platform.as_str()
    }

    pub fn submission_name(&self) -> &'static str {
        self.submission.as_str()
    }

    pub fn supports_legacy_ring_submission(&self) -> bool {
        matches!(self.submission, IntelSubmissionModel::LegacyRing)
    }

    pub fn requires_modern_submission(&self) -> bool {
        matches!(self.submission, IntelSubmissionModel::Modern)
    }
}

static FIRST_DEVICE: Mutex<Option<IntelGfxInfo>> = Mutex::new(None);
static DEVICES: Mutex<Vec<IntelGfxInfo, MAX_INTEL_DEVICES>> = Mutex::new(Vec::new());

#[repr(C, packed)]
#[derive(Copy, Clone)]
struct RgbVertex {
    x: f32,
    y: f32,
    r: u8,
    g: u8,
    b: u8,
    pad: u8,
}

fn register_intel(info: IntelGfxInfo) {
    let mut list = DEVICES.lock();
    let id = list.len();
    if id >= MAX_INTEL_DEVICES {
        crate::log!(
            "gfx-intel: device list full; dropping {:02X}:{:02X}.{}\n",
            info.bus,
            info.slot,
            info.function
        );
        return;
    }

    let mut first = FIRST_DEVICE.lock();
    if first.is_none() {
        *first = Some(info);
    }

    // Store a stable slot index in the scratch page's first byte for easy diagnostics.
    unsafe {
        if !info.cmd_scratch_virt.is_null() {
            *info.cmd_scratch_virt = id as u8;
        }
    }

    let _ = list.push(info);
}

fn log_intel_bar_inventory(bus: u8, slot: u8, function: u8) {
    // Header type 0 uses BAR0..BAR5. Log each decoded BAR once at claim-time.
    let mut idx = 0u8;
    while idx < 6 {
        let (raw_lo, raw_hi) = crate::pci::read_bar_raw(bus, slot, function, idx);
        if raw_lo == 0 || raw_lo == 0xFFFF_FFFF {
            crate::log!(
                "gfx-intel: BAR{} bdf={:02X}:{:02X}.{} raw=0x{:08X} unassigned\n",
                idx,
                bus,
                slot,
                function,
                raw_lo
            );
            idx += 1;
            continue;
        }

        if (raw_lo & 0x1) != 0 {
            let io_base = (raw_lo & !0x3) as u64;
            crate::log!(
                "gfx-intel: BAR{} bdf={:02X}:{:02X}.{} kind=io raw=0x{:08X} base=0x{:X}\n",
                idx,
                bus,
                slot,
                function,
                raw_lo,
                io_base
            );
            idx += 1;
            continue;
        }

        let is_64 = ((raw_lo >> 1) & 0x3) == 0x2;
        let prefetch = ((raw_lo >> 3) & 0x1) != 0;
        let mut base = (raw_lo & 0xFFFF_FFF0) as u64;
        if is_64 {
            base |= (raw_hi.unwrap_or(0) as u64) << 32;
        }
        let size = crate::pci::bar_size_bytes(bus, slot, function, idx).unwrap_or(0);
        crate::log!(
            "gfx-intel: BAR{} bdf={:02X}:{:02X}.{} kind=mmio{} prefetch={} raw=0x{:08X} raw_hi=0x{:08X} base=0x{:X} size=0x{:X}\n",
            idx,
            bus,
            slot,
            function,
            if is_64 { "64" } else { "32" },
            if prefetch { 1 } else { 0 },
            raw_lo,
            raw_hi.unwrap_or(0),
            base,
            size
        );

        if is_64 {
            idx += 2;
        } else {
            idx += 1;
        }
    }
}

fn log_intel_config_windows(bus: u8, slot: u8, function: u8) {
    let bdsm = crate::pci::config_read_u32(bus, slot, function, INTEL_PCI_BDSM);
    let bgsm = crate::pci::config_read_u32(bus, slot, function, INTEL_PCI_BGSM);
    let asls = crate::pci::config_read_u32(bus, slot, function, INTEL_PCI_ASLS);
    crate::log!(
        "gfx-intel: config {:02X}:{:02X}.{} bdsm=0x{:08X} bgsm=0x{:08X} asls=0x{:08X}\n",
        bus,
        slot,
        function,
        bdsm,
        bgsm,
        asls
    );
}

fn log_intel_opregion_probe(bus: u8, slot: u8, function: u8) {
    let asls = crate::pci::config_read_u32(bus, slot, function, INTEL_PCI_ASLS);
    if asls == 0 || asls == 0xFFFF_FFFF {
        crate::log!(
            "gfx-intel: opregion probe {:02X}:{:02X}.{} skipped asls=0x{:08X}\n",
            bus,
            slot,
            function,
            asls
        );
        return;
    }

    let phys = (asls as u64) & !0xFFFu64;
    let page_off = (asls as usize) & 0xFFFusize;
    let map_len = page_off.saturating_add(INTEL_OPREGION_PROBE_BYTES);
    let Ok(mapped) = crate::pci::mmio::map_mmio_region_exact(phys, map_len) else {
        crate::log!(
            "gfx-intel: opregion probe {:02X}:{:02X}.{} map failed asls=0x{:08X} phys=0x{:X} len=0x{:X}\n",
            bus,
            slot,
            function,
            asls,
            phys,
            map_len
        );
        return;
    };

    let base = unsafe { mapped.as_ptr().add(page_off) };
    let bytes = unsafe { core::slice::from_raw_parts(base as *const u8, INTEL_OPREGION_PROBE_BYTES) };

    let mut sig = [b'.'; 16];
    let mut idx = 0usize;
    while idx < sig.len() {
        let byte = bytes[idx];
        sig[idx] = if (0x20..=0x7E).contains(&byte) {
            byte
        } else {
            b'.'
        };
        idx += 1;
    }

    crate::log!(
        "gfx-intel: opregion probe {:02X}:{:02X}.{} asls=0x{:08X} phys=0x{:X} off=0x{:03X} d0=0x{:08X} d1=0x{:08X} d2=0x{:08X} d3=0x{:08X} sig='{}'\n",
        bus,
        slot,
        function,
        asls,
        phys,
        page_off,
        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
        u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
        u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
        core::str::from_utf8(&sig).unwrap_or("................")
    );
}

fn decode_mmio_bar(bus: u8, slot: u8, function: u8, index: u8) -> Option<(u64, u64)> {
    let (bar_lo, bar_hi) = crate::pci::read_bar_raw(bus, slot, function, index);
    if bar_lo == 0 || bar_lo == 0xFFFF_FFFF {
        return None;
    }
    if (bar_lo & 0x1) != 0 {
        return None;
    }

    let is_64 = ((bar_lo >> 1) & 0x3) == 0x2;
    let mut base = (bar_lo & 0xFFFF_FFF0) as u64;
    if is_64 {
        base |= (bar_hi.unwrap_or(0) as u64) << 32;
    }
    if base == 0 {
        return None;
    }

    let size = crate::pci::bar_size_bytes(bus, slot, function, index).unwrap_or(0);
    Some((base, size))
}

fn maybe_reassign_intel_bar0(
    bus: u8,
    slot: u8,
    function: u8,
    base: u64,
    size: u64,
) -> Option<(u64, u64)> {
    maybe_reassign_intel_mmio_bar(bus, slot, function, 0, "BAR0", base, size)
}

fn maybe_reassign_intel_bar2(
    bus: u8,
    slot: u8,
    function: u8,
    base: u64,
    size: u64,
) -> Option<(u64, u64)> {
    maybe_reassign_intel_mmio_bar(bus, slot, function, 2, "BAR2", base, size)
}

fn maybe_reassign_intel_mmio_bar(
    bus: u8,
    slot: u8,
    function: u8,
    index: u8,
    label: &str,
    base: u64,
    size: u64,
) -> Option<(u64, u64)> {
    if base != 0 && base < INTEL_BAR_SANITY_LIMIT {
        return Some((base, size));
    }

    let (bar_lo, bar_hi) = crate::pci::read_bar_raw(bus, slot, function, index);
    if (bar_lo & 0x1) != 0 {
        crate::log!(
            "gfx-intel: {} reassign skipped {:02X}:{:02X}.{} raw=0x{:08X} (io BAR)\n",
            label,
            bus,
            slot,
            function,
            bar_lo
        );
        return None;
    }

    let size = size.max(0x1000);
    let align = size.max(0x1000);
    let Some(new_base) = crate::pci::alloc_hotplug_mmio_base(bus, size, align) else {
        crate::log!(
            "gfx-intel: {} reassign alloc failed {:02X}:{:02X}.{} old=0x{:X} size=0x{:X} align=0x{:X}\n",
            label,
            bus,
            slot,
            function,
            base,
            size,
            align
        );
        return None;
    };
    crate::log!(
        "gfx-intel: {} reassign {:02X}:{:02X}.{} old=0x{:X} new=0x{:X} size=0x{:X} align=0x{:X}\n",
        label,
        bus,
        slot,
        function,
        base,
        new_base,
        size,
        align
    );

    let new_lo = ((new_base as u32) & !0xFu32) | (bar_lo & 0xFu32);
    let bar_off = 0x10u16 + (index as u16) * 4;
    crate::pci::config_write_u32(bus, slot, function, bar_off, new_lo);
    crate::pci::config_write_u32(bus, slot, function, bar_off + 4, (new_base >> 32) as u32);

    let (new_bar_lo, new_bar_hi) = crate::pci::read_bar_raw(bus, slot, function, index);
    if new_bar_lo == 0 || new_bar_lo == 0xFFFF_FFFF {
        crate::log!(
            "gfx-intel: {} reassign failed {:02X}:{:02X}.{} reread_lo=0x{:08X}\n",
            label,
            bus,
            slot,
            function,
            new_bar_lo
        );
        return None;
    }

    let new_hi = new_bar_hi.or(bar_hi).unwrap_or(0) as u64;
    let decoded = ((new_bar_lo as u64) & !0xFu64) | (new_hi << 32);
    if decoded == 0 {
        crate::log!(
            "gfx-intel: {} reassign produced zero base {:02X}:{:02X}.{}\n",
            label,
            bus,
            slot,
            function
        );
        return None;
    }

    Some((decoded, size))
}

fn classify_intel_platform(device_id: u16) -> IntelPlatform {
    match device_id {
        // Raptor Lake-S GT1 [UHD Graphics 770] from the current VFIO passthrough setup.
        INTEL_RPL_S_GT1_DEVICE_ID => IntelPlatform::RaptorLake,
        _ => IntelPlatform::Unknown,
    }
}

fn classify_submission_model(platform: IntelPlatform) -> IntelSubmissionModel {
    match platform {
        IntelPlatform::RaptorLake => IntelSubmissionModel::Modern,
        IntelPlatform::Unknown => IntelSubmissionModel::LegacyRing,
    }
}

#[inline]
fn is_intel_display(dev: &crate::pci::PciDevice) -> bool {
    dev.vendor == INTEL_VENDOR_ID && dev.class == PCI_CLASS_DISPLAY
}

pub fn init_once() {
    if crate::limine::hhdm_offset().is_none() {
        crate::log!("gfx-intel: no HHDM\n");
        return;
    }

    FIRST_DEVICE.lock().take();
    DEVICES.lock().clear();

    let mut pci_devices: Vec<crate::pci::PciDevice, 256> = Vec::new();
    crate::pci::with_devices(|list| {
        for dev in list {
            let _ = pci_devices.push(*dev);
        }
    });

    let mut did_match = false;
    for dev in pci_devices.iter() {
        if !is_intel_display(dev) {
            continue;
        }
        did_match = true;

        log_intel_bar_inventory(dev.bus, dev.slot, dev.function);
        log_intel_config_windows(dev.bus, dev.slot, dev.function);
        log_intel_opregion_probe(dev.bus, dev.slot, dev.function);

        let Some((bar0_base, bar_size)) =
            decode_mmio_bar(dev.bus, dev.slot, dev.function, 0)
        else {
            crate::log!(
                "gfx-intel: BAR0 not assigned at {:02X}:{:02X}.{}\n",
                dev.bus,
                dev.slot,
                dev.function
            );
            continue;
        };

        let Some((base, bar_size)) =
            maybe_reassign_intel_bar0(dev.bus, dev.slot, dev.function, bar0_base, bar_size)
        else {
            crate::log!(
                "gfx-intel: BAR0 unusable at {:02X}:{:02X}.{} base=0x{:X} size=0x{:X}\n",
                dev.bus,
                dev.slot,
                dev.function,
                bar0_base,
                bar_size
            );
            continue;
        };

        let (aperture_bar_phys, aperture_bar_size) =
            decode_mmio_bar(dev.bus, dev.slot, dev.function, 2).unwrap_or((0, 0));
        let (aperture_bar_phys, aperture_bar_size) = if aperture_bar_phys != 0 {
            maybe_reassign_intel_bar2(
                dev.bus,
                dev.slot,
                dev.function,
                aperture_bar_phys,
                aperture_bar_size,
            )
            .unwrap_or((aperture_bar_phys, aperture_bar_size))
        } else {
            (0, 0)
        };

        crate::pci::enable_mem_and_bus_master(dev.bus, dev.slot, dev.function);

        let mut mmio_len = if bar_size == 0 {
            0x20_000usize
        } else {
            bar_size as usize
        };
        if mmio_len < 0x20_000 {
            mmio_len = 0x20_000;
        }
        if mmio_len > 0x4_00000 {
            mmio_len = 0x4_00000;
        }

        let mmio_base = match crate::pci::mmio::map_mmio_region(base, mmio_len) {
            Ok(ptr) => ptr,
            Err(e) => {
                crate::log!(
                    "gfx-intel: MMIO map failed {:02X}:{:02X}.{} err={:?}\n",
                    dev.bus,
                    dev.slot,
                    dev.function,
                    e
                );
                continue;
            }
        };

        let (cmd_scratch_phys, cmd_scratch_virt) =
            crate::pci::dma::alloc(CMD_SCRATCH_BYTES, CMD_SCRATCH_BYTES)
                .unwrap_or((0, core::ptr::null_mut()));
        if !cmd_scratch_virt.is_null() {
            unsafe { write_bytes(cmd_scratch_virt, 0, CMD_SCRATCH_BYTES) };
        }

        let platform = classify_intel_platform(dev.device);
        let submission = classify_submission_model(platform);

        crate::log!(
            "gfx-intel: claimed {:02X}:{:02X}.{} device=0x{:04X} platform={} submit={} bar0=0x{:X} size=0x{:X} bar2=0x{:X} bar2_size=0x{:X} mmio=0x{:X} scratch=0x{:X}\n",
            dev.bus,
            dev.slot,
            dev.function,
            dev.device,
            platform.as_str(),
            submission.as_str(),
            base,
            bar_size,
            aperture_bar_phys,
            aperture_bar_size,
            mmio_base.as_ptr() as usize,
            cmd_scratch_phys
        );
        if matches!(submission, IntelSubmissionModel::Modern) {
            crate::log!(
                "gfx-intel: {:02X}:{:02X}.{} device=0x{:04X} requires a modern Intel submission path; legacy RCS ring MMIO is disabled for this platform\n",
                dev.bus,
                dev.slot,
                dev.function,
                dev.device
            );
        }

        let info = IntelGfxInfo {
            bus: dev.bus,
            slot: dev.slot,
            function: dev.function,
            device_id: dev.device,
            platform,
            submission,
            bar_phys: base,
            bar_size,
            aperture_bar_phys,
            aperture_bar_size,
            mmio_base,
            mmio_len,
            cmd_scratch_phys,
            cmd_scratch_virt,
            cmd_scratch_len: if cmd_scratch_virt.is_null() {
                0
            } else {
                CMD_SCRATCH_BYTES
            },
        };
        register_intel(info);

        if DEVICES.lock().len() >= MAX_INTEL_DEVICES {
            break;
        }
    }

    if DEVICES.lock().is_empty() {
        if !did_match {
            crate::log!("gfx-intel: no Intel display-class PCI device found\n");
        }
        return;
    }

    crate::v::readiness::set(crate::v::readiness::GFX_INTEL_CLAIMED);
}

#[inline]
pub fn has_claimed_device() -> bool {
    !DEVICES.lock().is_empty()
}

#[inline]
pub fn first_claimed_device() -> Option<IntelGfxInfo> {
    *FIRST_DEVICE.lock()
}

#[derive(Copy, Clone)]
struct IntelScanoutPlane {
    pipe_name: char,
    plane_slot: usize,
    ctl_off: usize,
    stride_off: usize,
    surf_off: usize,
    surf_live_off: usize,
    pipe_src_off: usize,
    trans_ddi_func_ctl_off: usize,
}

#[derive(Copy, Clone)]
struct IntelScanoutSurface {
    plane: IntelScanoutPlane,
    ctl: u32,
    stride: usize,
    surf: u32,
    surf_live: u32,
    width: usize,
    height: usize,
}

const INTEL_SCANOUT_PIPES: [(char, usize, usize); 4] = [
    ('A', INTEL_PIPE_A_SRC, INTEL_TRANS_A_DDI_FUNC_CTL),
    ('B', INTEL_PIPE_B_SRC, INTEL_TRANS_B_DDI_FUNC_CTL),
    ('C', INTEL_PIPE_C_SRC, INTEL_TRANS_C_DDI_FUNC_CTL),
    ('D', INTEL_PIPE_D_SRC, INTEL_TRANS_D_DDI_FUNC_CTL),
];

fn intel_mmio_read32(info: IntelGfxInfo, off: usize) -> u32 {
    if off + 4 > info.mmio_len {
        return 0;
    }
    let ptr = unsafe { info.mmio_base.as_ptr().add(off) as *const u32 };
    unsafe { core::ptr::read_volatile(ptr) }
}

fn intel_mmio_write32(info: IntelGfxInfo, off: usize, value: u32) -> bool {
    if off + 4 > info.mmio_len {
        return false;
    }
    let ptr = unsafe { info.mmio_base.as_ptr().add(off) as *mut u32 };
    unsafe { core::ptr::write_volatile(ptr, value) };
    let _ = intel_mmio_read32(info, off);
    true
}

fn decode_pipe_src(pipe_src: u32) -> (usize, usize) {
    let width = ((pipe_src & 0xFFFF) as usize).saturating_add(1);
    let height = (((pipe_src >> 16) & 0xFFFF) as usize).saturating_add(1);
    (width, height)
}

fn scanout_plane(pipe: usize, plane_slot: usize) -> IntelScanoutPlane {
    let plane_base = INTEL_UNI_PLANE_BASE
        + pipe.saturating_mul(INTEL_UNI_PLANE_PIPE_STRIDE)
        + plane_slot.saturating_mul(INTEL_UNI_PLANE_SLOT_STRIDE);
    let (pipe_name, pipe_src_off, trans_ddi_func_ctl_off) = INTEL_SCANOUT_PIPES[pipe];
    IntelScanoutPlane {
        pipe_name,
        plane_slot: plane_slot + 1,
        ctl_off: plane_base,
        stride_off: plane_base + INTEL_UNI_PLANE_STRIDE_OFF,
        surf_off: plane_base + INTEL_UNI_PLANE_SURF_OFF,
        surf_live_off: plane_base + INTEL_UNI_PLANE_SURFLIVE_OFF,
        pipe_src_off,
        trans_ddi_func_ctl_off,
    }
}

fn log_display_region_sweep(info: IntelGfxInfo) {
    let mut logged = 0usize;
    let mut page = INTEL_DISPLAY_SWEEP_START;
    while page < INTEL_DISPLAY_SWEEP_END {
        let mut found = None;
        let mut off = 0usize;
        while off < INTEL_DISPLAY_PAGE_STRIDE {
            let value = intel_mmio_read32(info, page + off);
            if value != 0 {
                found = Some((off, value));
                break;
            }
            off += 4;
        }
        if let Some((first_off, value)) = found {
            crate::log!(
                "gfx-intel-scanout: display-page page=0x{:05X} first=0x{:03X} value=0x{:08X}\n",
                page,
                first_off,
                value
            );
            if logged < 4 {
                log_display_window(info, page + first_off);
            }
            logged += 1;
            if logged >= INTEL_DISPLAY_SWEEP_LOG_LIMIT {
                break;
            }
        }
        page += INTEL_DISPLAY_PAGE_STRIDE;
    }
    if logged == 0 {
        crate::log!(
            "gfx-intel-scanout: display-page sweep 0x{:05X}..0x{:05X} found no nonzero registers\n",
            INTEL_DISPLAY_SWEEP_START,
            INTEL_DISPLAY_SWEEP_END
        );
    }
}

fn log_display_window(info: IntelGfxInfo, center_off: usize) {
    let aligned = center_off & !0x1Fusize;
    let mut idx = 0usize;
    while idx < INTEL_DISPLAY_WINDOW_DWORDS {
        let off = aligned + idx.saturating_mul(4);
        let value = intel_mmio_read32(info, off);
        crate::log!(
            "gfx-intel-scanout: display-mmio off=0x{:05X} value=0x{:08X}\n",
            off,
            value
        );
        idx += 1;
    }
}

fn probe_scanout_surface(info: IntelGfxInfo) -> Option<IntelScanoutSurface> {
    let mut found = None;
    for pipe in 0..INTEL_SCANOUT_PIPES.len() {
        let plane0 = scanout_plane(pipe, 0);
        let pipe_src = intel_mmio_read32(info, plane0.pipe_src_off);
        let trans_ddi = intel_mmio_read32(info, plane0.trans_ddi_func_ctl_off);
        let (pipe_w, pipe_h) = decode_pipe_src(pipe_src);
        crate::log!(
            "gfx-intel-scanout: pipe={} pipe_src=0x{:08X} size={}x{} ddi=0x{:08X}\n",
            plane0.pipe_name,
            pipe_src,
            pipe_w,
            pipe_h,
            trans_ddi
        );
        for plane_slot in 0..4 {
            let plane = scanout_plane(pipe, plane_slot);
            let ctl = intel_mmio_read32(info, plane.ctl_off);
            let stride = intel_mmio_read32(info, plane.stride_off) as usize;
            let surf = intel_mmio_read32(info, plane.surf_off);
            let surf_live = intel_mmio_read32(info, plane.surf_live_off);
            let enabled = (ctl & INTEL_PLANE_ENABLE) != 0;
            crate::log!(
                "gfx-intel-scanout: plane={}{} ctl=0x{:08X} stride=0x{:08X} surf=0x{:08X} surf_live=0x{:08X} enabled={}\n",
                plane.pipe_name,
                plane.plane_slot,
                ctl,
                stride as u32,
                surf,
                surf_live,
                enabled as u8
            );
            if !enabled || surf == 0 || stride < 64 || pipe_w == 0 || pipe_h == 0 {
                continue;
            }
            found = Some(IntelScanoutSurface {
                plane,
                ctl,
                stride,
                surf,
                surf_live,
                width: pipe_w,
                height: pipe_h,
            });
            break;
        }
        if found.is_some() {
            break;
        }
    }
    found
}

fn write_scanout_test_pattern(
    base: *mut u8,
    stride: usize,
    width: usize,
    height: usize,
) {
    for y in 0..height {
        let row_ptr = unsafe { base.add(y.saturating_mul(stride)) as *mut u32 };
        let row = unsafe { core::slice::from_raw_parts_mut(row_ptr, width) };
        for (x, px) in row.iter_mut().enumerate() {
            let band = (x.saturating_mul(6)) / width.max(1);
            let mut color = match band {
                0 => 0x00002020,
                1 => 0x00FF3030,
                2 => 0x0030FF30,
                3 => 0x003080FF,
                4 => 0x00F0E040,
                _ => 0x00F8F8F8,
            };
            if x < 4 || y < 4 || x + 4 >= width || y + 4 >= height {
                color = 0x00FFFFFF;
            }
            let diag0 = x.saturating_mul(height.max(1)) / width.max(1);
            let diag1 = (width.saturating_sub(1).saturating_sub(x))
                .saturating_mul(height.max(1))
                / width.max(1);
            if y.abs_diff(diag0) <= 2 || y.abs_diff(diag1) <= 2 {
                color = 0x00000000;
            }
            if y > height / 3 && y < (height / 3).saturating_mul(2) && x > width / 3 && x < (width / 3).saturating_mul(2) {
                color = 0x00000000;
            }
            *px = color;
        }
    }
}

fn try_scanout_surface_demo(info: IntelGfxInfo) -> bool {
    let Some(surface) = probe_scanout_surface(info) else {
        crate::log!("gfx-intel-scanout: no enabled primary plane found\n");
        return false;
    };

    if info.aperture_bar_phys == 0 || info.aperture_bar_size == 0 {
        crate::log!(
            "gfx-intel-scanout: aperture unavailable bar2=0x{:X} size=0x{:X}\n",
            info.aperture_bar_phys,
            info.aperture_bar_size
        );
        return false;
    }

    let surf_offset = (surface.surf as usize) & !0xFFFusize;
    let surf_page_off = (surface.surf as usize) & 0xFFFusize;
    let draw_w = surface.width.min(INTEL_SCANOUT_DRAW_W);
    let draw_h = surface.height.min(INTEL_SCANOUT_DRAW_H);
    let stride = surface.stride.max(draw_w.saturating_mul(4));
    let max_width = (stride / 4).max(1);
    let draw_w = draw_w.min(max_width);
    let bytes = surf_page_off.saturating_add(draw_h.saturating_mul(stride));

    if draw_w == 0 || draw_h == 0 {
        crate::log!(
            "gfx-intel-scanout: plane={}{} unusable draw size={}x{} stride=0x{:X}\n",
            surface.plane.pipe_name,
            surface.plane.plane_slot,
            draw_w,
            draw_h,
            stride
        );
        return false;
    }

    if surf_offset.saturating_add(bytes) > info.aperture_bar_size as usize {
        crate::log!(
            "gfx-intel-scanout: plane={}{} surf=0x{:08X} exceeds aperture size=0x{:X} bytes=0x{:X}\n",
            surface.plane.pipe_name,
            surface.plane.plane_slot,
            surface.surf,
            info.aperture_bar_size,
            bytes
        );
        return false;
    }

    let phys = info.aperture_bar_phys.saturating_add(surf_offset as u64);
    let Ok(mapped) = crate::pci::mmio::map_mmio_region_exact(phys, bytes) else {
        crate::log!(
            "gfx-intel-scanout: plane={}{} aperture map failed phys=0x{:X} bytes=0x{:X}\n",
            surface.plane.pipe_name,
            surface.plane.plane_slot,
            phys,
            bytes
        );
        return false;
    };

    let ptr = unsafe { mapped.as_ptr().add(surf_page_off) };
    write_scanout_test_pattern(ptr, stride, draw_w, draw_h);
    crate::log!(
        "gfx-intel-scanout: wrote test card plane={}{} surf=0x{:08X} surf_live=0x{:08X} ctl=0x{:08X} stride=0x{:X} visible={}x{} draw={}x{} aperture=0x{:X}/0x{:X}\n",
        surface.plane.pipe_name,
        surface.plane.plane_slot,
        surface.surf,
        surface.surf_live,
        surface.ctl,
        stride,
        surface.width,
        surface.height,
        draw_w,
        draw_h,
        info.aperture_bar_phys,
        info.aperture_bar_size
    );
    true
}

fn plane_write_smoke_test(info: IntelGfxInfo) {
    let mut writable = 0usize;
    for pipe in 0..INTEL_SCANOUT_PIPES.len() {
        for plane_slot in 0..4 {
            let plane = scanout_plane(pipe, plane_slot);
            let ctl = intel_mmio_read32(info, plane.ctl_off);
            if (ctl & INTEL_PLANE_ENABLE) != 0 {
                continue;
            }

            let orig_stride = intel_mmio_read32(info, plane.stride_off);
            let orig_surf = intel_mmio_read32(info, plane.surf_off);
            let test_stride =
                INTEL_PLANE_WRITE_SMOKE_STRIDE_BASE + (pipe as u32 * 0x40) + (plane_slot as u32 * 0x10);
            let test_surf =
                INTEL_PLANE_WRITE_SMOKE_SURF_BASE + (pipe as u32 * 0x0020_0000) + (plane_slot as u32 * 0x0002_0000);

            let wrote_stride = intel_mmio_write32(info, plane.stride_off, test_stride);
            let wrote_surf = intel_mmio_write32(info, plane.surf_off, test_surf);
            let rb_stride = intel_mmio_read32(info, plane.stride_off);
            let rb_surf = intel_mmio_read32(info, plane.surf_off);

            let _ = intel_mmio_write32(info, plane.stride_off, orig_stride);
            let _ = intel_mmio_write32(info, plane.surf_off, orig_surf);
            let restored_stride = intel_mmio_read32(info, plane.stride_off);
            let restored_surf = intel_mmio_read32(info, plane.surf_off);
            let stride_stuck = wrote_stride && rb_stride == test_stride;
            let surf_stuck = wrote_surf && rb_surf == test_surf;
            if stride_stuck || surf_stuck {
                writable += 1;
            }

            crate::log!(
                "gfx-intel-scanout: plane-write-smoke {}{} ctl=0x{:08X} stride orig=0x{:08X} test=0x{:08X} rb=0x{:08X} restore=0x{:08X} surf orig=0x{:08X} test=0x{:08X} rb=0x{:08X} restore=0x{:08X} stuck_stride={} stuck_surf={}\n",
                plane.pipe_name,
                plane.plane_slot,
                ctl,
                orig_stride,
                test_stride,
                rb_stride,
                restored_stride,
                orig_surf,
                test_surf,
                rb_surf,
                restored_surf,
                stride_stuck as u8,
                surf_stuck as u8
            );
        }
    }

    crate::log!(
        "gfx-intel-scanout: plane-write-smoke writable_planes={}\n",
        writable
    );
}

fn centered_triangle() -> [RgbVertex; 3] {
    [
        RgbVertex {
            x: 0.0,
            y: -0.55,
            r: 0xFF,
            g: 0x40,
            b: 0x40,
            pad: 0,
        },
        RgbVertex {
            x: -0.55,
            y: 0.45,
            r: 0x40,
            g: 0xFF,
            b: 0x70,
            pad: 0,
        },
        RgbVertex {
            x: 0.55,
            y: 0.45,
            r: 0x50,
            g: 0x90,
            b: 0xFF,
            pad: 0,
        },
    ]
}

fn ndc_to_pixel(v: f32, extent: usize) -> i32 {
    if extent <= 1 {
        return 0;
    }
    let max = (extent - 1) as f32;
    let p = ((v * 0.5) + 0.5) * max;
    libm::roundf(p) as i32
}

fn edge2(ax2: i32, ay2: i32, bx2: i32, by2: i32, px2: i32, py2: i32) -> i64 {
    let apx = (px2 - ax2) as i64;
    let apy = (py2 - ay2) as i64;
    let abx = (bx2 - ax2) as i64;
    let aby = (by2 - ay2) as i64;
    apx * aby - apy * abx
}

fn draw_centered_triangle_limine_fallback() -> bool {
    use ::limine::framebuffer::MemoryModel;

    let Some(resp) = crate::limine::framebuffer_response() else {
        return false;
    };
    let Some(fb) = resp.framebuffers().next() else {
        return false;
    };
    if fb.memory_model() != MemoryModel::RGB || fb.bpp() != 32 {
        return false;
    }

    let width = fb.width() as usize;
    let height = fb.height() as usize;
    if width == 0 || height == 0 {
        return false;
    }

    let pitch = fb.pitch() as usize;
    let addr = fb.addr();

    let clear = 0x00101018u32;
    for y in 0..height {
        let row_ptr = unsafe { addr.add(y.saturating_mul(pitch)) as *mut u32 };
        let row = unsafe { core::slice::from_raw_parts_mut(row_ptr, width) };
        row.fill(clear);
    }

    let tri = centered_triangle();
    let p0x = ndc_to_pixel(tri[0].x, width);
    let p0y = ndc_to_pixel(tri[0].y, height);
    let p1x = ndc_to_pixel(tri[1].x, width);
    let p1y = ndc_to_pixel(tri[1].y, height);
    let p2x = ndc_to_pixel(tri[2].x, width);
    let p2y = ndc_to_pixel(tri[2].y, height);

    let min_x = p0x
        .min(p1x.min(p2x))
        .clamp(0, (width as i32).saturating_sub(1));
    let max_x = p0x
        .max(p1x.max(p2x))
        .clamp(0, (width as i32).saturating_sub(1));
    let min_y = p0y
        .min(p1y.min(p2y))
        .clamp(0, (height as i32).saturating_sub(1));
    let max_y = p0y
        .max(p1y.max(p2y))
        .clamp(0, (height as i32).saturating_sub(1));

    let p0x2 = p0x.saturating_mul(2).saturating_add(1);
    let p0y2 = p0y.saturating_mul(2).saturating_add(1);
    let p1x2 = p1x.saturating_mul(2).saturating_add(1);
    let p1y2 = p1y.saturating_mul(2).saturating_add(1);
    let p2x2 = p2x.saturating_mul(2).saturating_add(1);
    let p2y2 = p2y.saturating_mul(2).saturating_add(1);

    let area2 = edge2(p0x2, p0y2, p1x2, p1y2, p2x2, p2y2);
    if area2 == 0 {
        return false;
    }
    let sign = if area2 > 0 { 1i64 } else { -1i64 };

    let fill = 0x004ABCFFu32;
    for y in min_y..=max_y {
        let row_ptr = unsafe { addr.add((y as usize).saturating_mul(pitch)) as *mut u32 };
        let row = unsafe { core::slice::from_raw_parts_mut(row_ptr, width) };
        let py2 = y.saturating_mul(2).saturating_add(1);
        for x in min_x..=max_x {
            let px2 = x.saturating_mul(2).saturating_add(1);
            let w0 = sign * edge2(p1x2, p1y2, p2x2, p2y2, px2, py2);
            let w1 = sign * edge2(p2x2, p2y2, p0x2, p0y2, px2, py2);
            let w2 = sign * edge2(p0x2, p0y2, p1x2, p1y2, px2, py2);
            if w0 >= 0 && w1 >= 0 && w2 >= 0 {
                row[x as usize] = fill;
            }
        }
    }

    true
}

#[embassy_executor::task]
pub async fn centered_triangle_demo_task() {
    if !has_claimed_device() {
        crate::log!("gfx-intel-demo: skipped (no claimed Intel gfx device)\n");
        return;
    }

    crate::gfx::init(crate::limine::framebuffer_response());

    let verts = centered_triangle();
    let ptr = verts.as_ptr() as *const u8;
    let len = core::mem::size_of_val(&verts);

    let mut tries = 0u32;
    loop {
        let rc = unsafe {
            crate::surface::io::cabi::trueos_cabi_gfx_draw_rgb_triangles(0x101018, ptr, len)
        };
        if rc == 0 {
            crate::log!("gfx-intel-demo: centered triangle submitted\n");
            return;
        }

        tries = tries.saturating_add(1);
        if tries == 1 || tries.is_multiple_of(20) {
            crate::log!("gfx-intel-demo: draw retry rc={} tries={}\n", rc, tries);
        }

        if rc == -3 && tries >= 8 {
            if draw_centered_triangle_limine_fallback() {
                crate::log!("gfx-intel-demo: fallback triangle rasterized via Limine fb\n");
            } else {
                crate::log!("gfx-intel-demo: fallback rasterizer unavailable\n");
            }
            return;
        }

        if tries >= 200 {
            if draw_centered_triangle_limine_fallback() {
                crate::log!(
                    "gfx-intel-demo: fallback triangle rasterized after {} retries\n",
                    tries
                );
            } else {
                crate::log!("gfx-intel-demo: giving up after {} retries\n", tries);
            }
            return;
        }

        Timer::after(EmbassyDuration::from_millis(25)).await;
    }
}

#[embassy_executor::task]
pub async fn scanout_smoke_task() {
    let Some(info) = first_claimed_device() else {
        crate::log!("gfx-intel-scanout: skipped (no claimed Intel gfx device)\n");
        return;
    };

    Timer::after(EmbassyDuration::from_millis(1200)).await;
    log_display_region_sweep(info);
    plane_write_smoke_test(info);

    let mut tries = 0u32;
    loop {
        if try_scanout_surface_demo(info) {
            return;
        }

        tries = tries.saturating_add(1);
        if tries == 1 || tries.is_multiple_of(8) {
            crate::log!(
                "gfx-intel-scanout: retrying plane/aperture probe tries={}\n",
                tries
            );
        }
        if tries >= INTEL_SCANOUT_RETRIES {
            crate::log!(
                "gfx-intel-scanout: giving up after {} retries\n",
                tries
            );
            return;
        }
        Timer::after(EmbassyDuration::from_millis(INTEL_SCANOUT_RETRY_MS)).await;
    }
}
