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
    if base != 0 && base < INTEL_BAR_SANITY_LIMIT {
        return Some((base, size));
    }

    let (bar_lo, bar_hi) = crate::pci::read_bar_raw(bus, slot, function, 0);
    if (bar_lo & 0x1) != 0 {
        crate::log!(
            "gfx-intel: BAR0 reassign skipped {:02X}:{:02X}.{} raw=0x{:08X} (io BAR)\n",
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
            "gfx-intel: BAR0 reassign alloc failed {:02X}:{:02X}.{} old=0x{:X} size=0x{:X} align=0x{:X}\n",
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
        "gfx-intel: BAR0 reassign {:02X}:{:02X}.{} old=0x{:X} new=0x{:X} size=0x{:X} align=0x{:X}\n",
        bus,
        slot,
        function,
        base,
        new_base,
        size,
        align
    );

    let new_lo = ((new_base as u32) & !0xFu32) | (bar_lo & 0xFu32);
    crate::pci::config_write_u32(bus, slot, function, 0x10, new_lo);
    crate::pci::config_write_u32(bus, slot, function, 0x14, (new_base >> 32) as u32);

    let (new_bar_lo, new_bar_hi) = crate::pci::read_bar_raw(bus, slot, function, 0);
    if new_bar_lo == 0 || new_bar_lo == 0xFFFF_FFFF {
        crate::log!(
            "gfx-intel: BAR0 reassign failed {:02X}:{:02X}.{} reread_lo=0x{:08X}\n",
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
            "gfx-intel: BAR0 reassign produced zero base {:02X}:{:02X}.{}\n",
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

        crate::pci::enable_mem_and_bus_master(dev.bus, dev.slot, dev.function);

        let (aperture_bar_phys, aperture_bar_size) =
            decode_mmio_bar(dev.bus, dev.slot, dev.function, 2).unwrap_or((0, 0));

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
