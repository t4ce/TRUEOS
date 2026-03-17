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
const INTEL_DISPLAY_DENSE_WINDOW_DWORDS: usize = 32;
const INTEL_DISPLAY_EXTRA_DENSE_WINDOW_DWORDS: usize = 64;
const INTEL_DISPLAY_CENSUS_GROUP_LIMIT: usize = 24;
const INTEL_DISPLAY_CENSUS_RUN_LIMIT: usize = 16;
const INTEL_DISPLAY_SIGNATURE_TOP_PAGES: usize = 8;
const INTEL_DISPLAY_SIGNATURE_WINDOW_DWORDS: usize = 8;
const INTEL_SIGNATURE_SMOKE_CTL_OFF: usize = 0x82000;
const INTEL_SIGNATURE_SMOKE_SURF_OFF: usize = 0x82014;
const INTEL_SIGNATURE_SMOKE_PIPE_SRC_OFF: usize = 0x82020;
const INTEL_SIGNATURE_SMOKE_STRIDE_OFF: usize = 0x82FC0;
const INTEL_PLANE_WRITE_SMOKE_STRIDE_BASE: u32 = 0x200;
const INTEL_PLANE_WRITE_SMOKE_SURF_BASE: u32 = 0x0100_0000;
const INTEL_PCI_BDSM: u16 = 0x5C;
const INTEL_PCI_BGSM: u16 = 0x70;
const INTEL_PCI_ASLS: u16 = 0xFC;
const INTEL_OPREGION_PROBE_BYTES: usize = 0x40;
const INTEL_OPREGION_SCAN_BYTES: usize = 0x1000;
const INTEL_PORT_HOTPLUG_EN: usize = 0x61110;
const INTEL_BXT_DE_PLL_CTL: usize = 0x6D000;
const INTEL_BXT_DE_PLL_ENABLE: usize = 0x46070;
const INTEL_DC_STATE_EN: usize = 0x45504;
const INTEL_DC_STATE_DEBUG: usize = 0x45520;
const INTEL_GT_DISP_PWRON: usize = 0x138090;
const INTEL_ICL_PHY_MISC_A: usize = 0x64C00;
const INTEL_ICL_PHY_MISC_B: usize = 0x64C04;
const INTEL_DISPIO_CR_TX_BMU_CR0: usize = 0x6C00C;
const INTEL_GT_DISP_PWRON_REQ: u32 = 0x00000001;
const INTEL_PORT_HOTPLUG_TEST_BIT: u32 = 0x00000001;
const INTEL_PASSIVE_ONLY_DEFAULT: bool = true;
const INTEL_DIRECT_DEMO_SURF_OFF: u32 = 0x0020_0000;
const INTEL_DIRECT_DEMO_STRIDE: u32 = 4096;
const INTEL_DIRECT_DEMO_WIDTH: usize = 1024;
const INTEL_DIRECT_DEMO_HEIGHT: usize = 768;
const INTEL_WRITE_SWEEP_TEST_VALUE: u32 = 0x00000001;
const INTEL_WRITE_SWEEP_MAX_ATTEMPTS_PER_WINDOW: usize = 16;
const INTEL_WRITE_SWEEP_MAX_SUCCESSES: usize = 8;
const INTEL_PATTERN_WALK_LOG_LIMIT: usize = 8;
const INTEL_TUPLE_PROBE_START: usize = 0x6C07C;
const INTEL_TUPLE_PROBE_END: usize = 0x6C120;
const INTEL_WRITE_SWEEP_HIT_LOG_LIMIT: usize = 1;

const INTEL_WRITE_SWEEP_WINDOWS: &[(usize, usize, &str)] = &[
    (0x45000, 0x45080, "dc-pll-45000"),
    (0x46000, 0x46080, "dc-pll-46000"),
    (0x61100, 0x61120, "hotplug-61100"),
    (0x64C00, 0x64C40, "phy-misc-64C00"),
    (0x6C000, 0x6C080, "tx-bmu-6C000"),
    (0x6D040, 0x6D080, "de-pll-tail-6D040"),
    (0x138040, 0x138090, "gt-disp-tail-138040"),
];

const INTEL_PATTERN_WALK_RANGES: &[(usize, usize, &str)] = &[
    (0x6BC00, 0x6C200, "tx-bmu-neighborhood"),
    (0x137FC0, 0x138120, "gt-disp-neighborhood"),
];

const INTEL_DISPLAY_CENSUS_RANGES: &[(usize, usize, &str)] = &[
    (0x44000, 0x47000, "dc/pll"),
    (0x47000, 0x4A000, "dc/pll-near"),
    (0x4A000, 0x50000, "dc/pll-far"),
    (0x60000, 0x74000, "pipes-planes-phy"),
    (0x74000, 0x78000, "pipes-planes-tail"),
    (0x138000, 0x139000, "gt-disp-pw"),
    (0x139000, 0x13C000, "gt-disp-near"),
];

const INTEL_DISPLAY_DENSE_CENTERS: &[(usize, &str)] = &[
    (0x45000, "dc/pll-page-45000"),
    (0x46000, "dc/pll-page-46000"),
    (0x64000, "phy-page-64000"),
    (0x6C000, "phy-page-6C000"),
    (0x6D000, "phy-page-6D000"),
    (0x80000, "sig-page-80000"),
    (0x81000, "sig-page-81000"),
    (0x82000, "sig-page-82000"),
    (0x83000, "sig-page-83000"),
    (0x86000, "sig-page-86000"),
    (0x138000, "gt-disp-page-138000"),
];

const INTEL_DISPLAY_EXTRA_DENSE_WINDOWS: &[(usize, &str)] = &[
    (0x6D040, "phy-tail-6D040"),
    (0x80180, "sig-tail-80180"),
    (0x82FC0, "sig-tail-82FC0"),
    (0x833C0, "sig-tail-833C0"),
    (0x860C0, "sig-tail-860C0"),
    (0x603E0, "ddi-a-tail-603E0"),
    (0x613E0, "ddi-b-tail-613E0"),
    (0x623E0, "ddi-c-tail-623E0"),
    (0x633E0, "ddi-d-tail-633E0"),
    (0x610E0, "hotplug-tail-610E0"),
    (0x138040, "gt-disp-tail-138040"),
];

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
    let map_len =
        INTEL_OPREGION_SCAN_BYTES.max(page_off.saturating_add(INTEL_OPREGION_PROBE_BYTES));
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

    let page_bytes = unsafe { core::slice::from_raw_parts(mapped.as_ptr() as *const u8, map_len) };
    let base_bytes = &page_bytes[..INTEL_OPREGION_PROBE_BYTES.min(page_bytes.len())];
    let off_bytes = if page_off < page_bytes.len() {
        &page_bytes[page_off..(page_off + INTEL_OPREGION_PROBE_BYTES).min(page_bytes.len())]
    } else {
        &page_bytes[..0]
    };

    fn ascii_sig(bytes: &[u8]) -> [u8; 16] {
        let mut sig = [b'.'; 16];
        let mut idx = 0usize;
        while idx < sig.len() && idx < bytes.len() {
            let byte = bytes[idx];
            sig[idx] = if (0x20..=0x7E).contains(&byte) {
                byte
            } else {
                b'.'
            };
            idx += 1;
        }
        sig
    }

    fn probe_dword(bytes: &[u8], idx: usize) -> u32 {
        let start = idx.saturating_mul(4);
        if start + 4 > bytes.len() {
            return 0;
        }
        u32::from_le_bytes([
            bytes[start],
            bytes[start + 1],
            bytes[start + 2],
            bytes[start + 3],
        ])
    }

    fn find_needle(bytes: &[u8], needle: &[u8]) -> Option<usize> {
        if needle.is_empty() || bytes.len() < needle.len() {
            return None;
        }
        let mut idx = 0usize;
        while idx + needle.len() <= bytes.len() {
            if &bytes[idx..idx + needle.len()] == needle {
                return Some(idx);
            }
            idx += 1;
        }
        None
    }

    let base_sig = ascii_sig(base_bytes);
    let off_sig = ascii_sig(off_bytes);
    let opregion_hdr = find_needle(page_bytes, b"IntelGraphicsMem");
    let vbt_hdr = find_needle(page_bytes, b"$VBT");

    crate::log!(
        "gfx-intel: opregion probe {:02X}:{:02X}.{} asls=0x{:08X} phys=0x{:X} off=0x{:03X} base_d0=0x{:08X} base_d1=0x{:08X} base_d2=0x{:08X} base_d3=0x{:08X} base_sig='{}' off_d0=0x{:08X} off_d1=0x{:08X} off_d2=0x{:08X} off_d3=0x{:08X} off_sig='{}' hdr={} vbt={}\n",
        bus,
        slot,
        function,
        asls,
        phys,
        page_off,
        probe_dword(base_bytes, 0),
        probe_dword(base_bytes, 1),
        probe_dword(base_bytes, 2),
        probe_dword(base_bytes, 3),
        core::str::from_utf8(&base_sig).unwrap_or("................"),
        probe_dword(off_bytes, 0),
        probe_dword(off_bytes, 1),
        probe_dword(off_bytes, 2),
        probe_dword(off_bytes, 3),
        core::str::from_utf8(&off_sig).unwrap_or("................"),
        opregion_hdr.map(|v| v as isize).unwrap_or(-1),
        vbt_hdr.map(|v| v as isize).unwrap_or(-1)
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

        let Some((bar0_base, bar_size)) = decode_mmio_bar(dev.bus, dev.slot, dev.function, 0)
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

#[derive(Copy, Clone)]
struct IntelDisplaySignatureCandidate {
    page: usize,
    score: u32,
    nonzero_dwords: u16,
    stride_off: usize,
    stride_value: u32,
    surf_off: usize,
    surf_value: u32,
    pipe_src_off: usize,
    pipe_src_value: u32,
    ctl_off: usize,
    ctl_value: u32,
}

const INTEL_SCANOUT_PIPES: [(char, usize, usize); 4] = [
    ('A', INTEL_PIPE_A_SRC, INTEL_TRANS_A_DDI_FUNC_CTL),
    ('B', INTEL_PIPE_B_SRC, INTEL_TRANS_B_DDI_FUNC_CTL),
    ('C', INTEL_PIPE_C_SRC, INTEL_TRANS_C_DDI_FUNC_CTL),
    ('D', INTEL_PIPE_D_SRC, INTEL_TRANS_D_DDI_FUNC_CTL),
];

impl IntelDisplaySignatureCandidate {
    const fn empty() -> Self {
        Self {
            page: 0,
            score: 0,
            nonzero_dwords: 0,
            stride_off: usize::MAX,
            stride_value: 0,
            surf_off: usize::MAX,
            surf_value: 0,
            pipe_src_off: usize::MAX,
            pipe_src_value: 0,
            ctl_off: usize::MAX,
            ctl_value: 0,
        }
    }
}

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

fn plausible_pipe_src(value: u32) -> Option<(usize, usize)> {
    if value == 0 || value == u32::MAX {
        return None;
    }
    let (width, height) = decode_pipe_src(value);
    if !(320..=8192).contains(&width) || !(200..=4320).contains(&height) {
        return None;
    }
    Some((width, height))
}

fn plausible_scanout_stride(value: u32) -> bool {
    if value == 0 || value == u32::MAX {
        return false;
    }
    let stride = value as usize;
    (256..=0x20_000).contains(&stride) && stride.is_multiple_of(64)
}

fn plausible_scanout_surface(value: u32, aperture_bar_size: u64) -> bool {
    if value == 0 || value == u32::MAX || aperture_bar_size == 0 {
        return false;
    }
    let offset = value as u64;
    offset < aperture_bar_size && (offset & 0xFFF) == 0
}

fn insert_signature_candidate(
    top: &mut [IntelDisplaySignatureCandidate; INTEL_DISPLAY_SIGNATURE_TOP_PAGES],
    cand: IntelDisplaySignatureCandidate,
) {
    if cand.score == 0 {
        return;
    }
    let mut slot = None;
    let mut idx = 0usize;
    while idx < top.len() {
        if cand.score > top[idx].score {
            slot = Some(idx);
            break;
        }
        idx += 1;
    }
    let Some(slot_idx) = slot else {
        return;
    };
    let mut move_idx = top.len() - 1;
    while move_idx > slot_idx {
        top[move_idx] = top[move_idx - 1];
        move_idx -= 1;
    }
    top[slot_idx] = cand;
}

fn log_signature_window(info: IntelGfxInfo, page: usize, label: &str) {
    crate::log!(
        "gfx-intel-scanout: signature-window label={} base=0x{:05X} dwords={}\n",
        label,
        page,
        INTEL_DISPLAY_SIGNATURE_WINDOW_DWORDS
    );
    let mut idx = 0usize;
    while idx < INTEL_DISPLAY_SIGNATURE_WINDOW_DWORDS {
        let off = page + idx.saturating_mul(4);
        let value = intel_mmio_read32(info, off);
        crate::log!(
            "gfx-intel-scanout: signature-mmio label={} off=0x{:05X} value=0x{:08X}\n",
            label,
            off,
            value
        );
        idx += 1;
    }
}

fn log_display_signature_sweep(info: IntelGfxInfo) {
    let mut top = [IntelDisplaySignatureCandidate::empty(); INTEL_DISPLAY_SIGNATURE_TOP_PAGES];
    let mut page = 0usize;
    while page + INTEL_DISPLAY_PAGE_STRIDE <= info.mmio_len {
        let mut cand = IntelDisplaySignatureCandidate {
            page,
            ..IntelDisplaySignatureCandidate::empty()
        };
        let mut off = 0usize;
        while off < INTEL_DISPLAY_PAGE_STRIDE {
            let mmio_off = page + off;
            let value = intel_mmio_read32(info, mmio_off);
            if value != 0 {
                cand.nonzero_dwords = cand.nonzero_dwords.saturating_add(1);
            }
            if cand.pipe_src_off == usize::MAX && plausible_pipe_src(value).is_some() {
                cand.pipe_src_off = mmio_off;
                cand.pipe_src_value = value;
                cand.score = cand.score.saturating_add(7);
            }
            if cand.stride_off == usize::MAX && plausible_scanout_stride(value) {
                cand.stride_off = mmio_off;
                cand.stride_value = value;
                cand.score = cand.score.saturating_add(5);
            }
            if cand.surf_off == usize::MAX
                && plausible_scanout_surface(value, info.aperture_bar_size)
            {
                cand.surf_off = mmio_off;
                cand.surf_value = value;
                cand.score = cand.score.saturating_add(6);
            }
            if cand.ctl_off == usize::MAX && (value & INTEL_PLANE_ENABLE) != 0 && value != u32::MAX
            {
                cand.ctl_off = mmio_off;
                cand.ctl_value = value;
                cand.score = cand.score.saturating_add(3);
            }
            off += 4;
        }
        if cand.pipe_src_off != usize::MAX && cand.stride_off != usize::MAX {
            cand.score = cand.score.saturating_add(4);
        }
        if cand.surf_off != usize::MAX && cand.stride_off != usize::MAX {
            cand.score = cand.score.saturating_add(3);
        }
        if cand.surf_off != usize::MAX && cand.ctl_off != usize::MAX {
            cand.score = cand.score.saturating_add(2);
        }
        insert_signature_candidate(&mut top, cand);
        page += INTEL_DISPLAY_PAGE_STRIDE;
    }

    crate::log!(
        "gfx-intel-scanout: signature-sweep begin mmio_len=0x{:X} aperture=0x{:X}\n",
        info.mmio_len,
        info.aperture_bar_size
    );
    let mut rank = 0usize;
    while rank < top.len() && top[rank].score != 0 {
        let cand = top[rank];
        let (pipe_w, pipe_h) = plausible_pipe_src(cand.pipe_src_value).unwrap_or((0, 0));
        crate::log!(
            "gfx-intel-scanout: signature-candidate rank={} page=0x{:05X} score={} nonzero={} pipe_src_off={} pipe_src=0x{:08X} size={}x{} stride_off={} stride=0x{:08X} surf_off={} surf=0x{:08X} ctl_off={} ctl=0x{:08X}\n",
            rank + 1,
            cand.page,
            cand.score,
            cand.nonzero_dwords,
            if cand.pipe_src_off == usize::MAX {
                -1isize
            } else {
                cand.pipe_src_off as isize
            },
            cand.pipe_src_value,
            pipe_w,
            pipe_h,
            if cand.stride_off == usize::MAX {
                -1isize
            } else {
                cand.stride_off as isize
            },
            cand.stride_value,
            if cand.surf_off == usize::MAX {
                -1isize
            } else {
                cand.surf_off as isize
            },
            cand.surf_value,
            if cand.ctl_off == usize::MAX {
                -1isize
            } else {
                cand.ctl_off as isize
            },
            cand.ctl_value
        );
        if rank < 3 {
            log_signature_window(info, cand.page, "signature-top");
        }
        rank += 1;
    }
    if rank == 0 {
        crate::log!("gfx-intel-scanout: signature-sweep found no plausible scanout pages\n");
    }
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

fn log_display_range_census(info: IntelGfxInfo) {
    for &(start, end, name) in INTEL_DISPLAY_CENSUS_RANGES {
        let mut page = start;
        let mut logged = 0usize;
        let mut run_logged = 0usize;
        let mut nonzero_pages = 0usize;
        let mut ffff_pages = 0usize;
        let mut zero_pages = 0usize;
        let mut run_class = "";
        let mut run_start = start;
        let mut run_pages = 0usize;
        crate::log!(
            "gfx-intel-scanout: census begin name={} start=0x{:05X} end=0x{:05X}\n",
            name,
            start,
            end
        );
        while page < end {
            let mut sample_or = 0u32;
            let mut sample_and = u32::MAX;
            let mut first_nonzero = None;
            let mut first_nonffff = None;
            let mut off = 0usize;
            while off < INTEL_DISPLAY_PAGE_STRIDE {
                let value = intel_mmio_read32(info, page + off);
                sample_or |= value;
                sample_and &= value;
                if first_nonzero.is_none() && value != 0 {
                    first_nonzero = Some((off, value));
                }
                if first_nonffff.is_none() && value != u32::MAX {
                    first_nonffff = Some((off, value));
                }
                off += 4;
            }

            let class = if sample_or == 0 {
                zero_pages += 1;
                "zero"
            } else if sample_and == u32::MAX {
                ffff_pages += 1;
                if logged < INTEL_DISPLAY_CENSUS_GROUP_LIMIT {
                    crate::log!(
                        "gfx-intel-scanout: census page=0x{:05X} class=ffff name={}\n",
                        page,
                        name
                    );
                    logged += 1;
                }
                "ffff"
            } else {
                nonzero_pages += 1;
                if logged < INTEL_DISPLAY_CENSUS_GROUP_LIMIT {
                    let (nz_off, nz_val) = first_nonzero.unwrap_or((0, 0));
                    let (nf_off, nf_val) = first_nonffff.unwrap_or((0, u32::MAX));
                    crate::log!(
                        "gfx-intel-scanout: census page=0x{:05X} class=mixed name={} or=0x{:08X} and=0x{:08X} first_nz=0x{:03X}/0x{:08X} first_nonffff=0x{:03X}/0x{:08X}\n",
                        page,
                        name,
                        sample_or,
                        sample_and,
                        nz_off,
                        nz_val,
                        nf_off,
                        nf_val
                    );
                    logged += 1;
                }
                "mixed"
            };

            if run_pages == 0 {
                run_class = class;
                run_start = page;
                run_pages = 1;
            } else if run_class == class {
                run_pages += 1;
            } else {
                if run_logged < INTEL_DISPLAY_CENSUS_RUN_LIMIT {
                    crate::log!(
                        "gfx-intel-scanout: census run name={} class={} start=0x{:05X} end=0x{:05X} pages={}\n",
                        name,
                        run_class,
                        run_start,
                        page,
                        run_pages
                    );
                    run_logged += 1;
                }
                run_class = class;
                run_start = page;
                run_pages = 1;
            }

            page += INTEL_DISPLAY_PAGE_STRIDE;
        }
        if run_pages != 0 && run_logged < INTEL_DISPLAY_CENSUS_RUN_LIMIT {
            crate::log!(
                "gfx-intel-scanout: census run name={} class={} start=0x{:05X} end=0x{:05X} pages={}\n",
                name,
                run_class,
                run_start,
                end,
                run_pages
            );
        }
        crate::log!(
            "gfx-intel-scanout: census end name={} mixed={} ffff={} zero={}\n",
            name,
            nonzero_pages,
            ffff_pages,
            zero_pages
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

fn log_display_dense_window(info: IntelGfxInfo, center_off: usize, label: &str) {
    let aligned = center_off & !(INTEL_DISPLAY_PAGE_STRIDE - 1);
    crate::log!(
        "gfx-intel-scanout: dense-window label={} base=0x{:05X} dwords={}\n",
        label,
        aligned,
        INTEL_DISPLAY_DENSE_WINDOW_DWORDS
    );
    let mut idx = 0usize;
    while idx < INTEL_DISPLAY_DENSE_WINDOW_DWORDS {
        let off = aligned + idx.saturating_mul(4);
        let value = intel_mmio_read32(info, off);
        crate::log!(
            "gfx-intel-scanout: dense-mmio label={} off=0x{:05X} value=0x{:08X}\n",
            label,
            off,
            value
        );
        idx += 1;
    }
}

fn log_display_dense_windows(info: IntelGfxInfo) {
    for &(center, label) in INTEL_DISPLAY_DENSE_CENTERS {
        log_display_dense_window(info, center, label);
    }
}

fn log_display_extra_dense_window(info: IntelGfxInfo, start_off: usize, label: &str) {
    let aligned = start_off & !(INTEL_DISPLAY_PAGE_STRIDE - 1);
    crate::log!(
        "gfx-intel-scanout: extra-dense-window label={} start=0x{:05X} dwords={}\n",
        label,
        start_off,
        INTEL_DISPLAY_EXTRA_DENSE_WINDOW_DWORDS
    );
    let mut idx = 0usize;
    while idx < INTEL_DISPLAY_EXTRA_DENSE_WINDOW_DWORDS {
        let off = aligned + idx.saturating_mul(4);
        let value = intel_mmio_read32(info, off);
        crate::log!(
            "gfx-intel-scanout: extra-dense-mmio label={} off=0x{:05X} value=0x{:08X}\n",
            label,
            off,
            value
        );
        idx += 1;
    }
}

fn log_display_extra_dense_windows(info: IntelGfxInfo) {
    for &(start, label) in INTEL_DISPLAY_EXTRA_DENSE_WINDOWS {
        log_display_extra_dense_window(info, start, label);
    }
}

fn log_display_power_probe(info: IntelGfxInfo) {
    let phy_misc_a = intel_mmio_read32(info, INTEL_ICL_PHY_MISC_A);
    let phy_misc_b = intel_mmio_read32(info, INTEL_ICL_PHY_MISC_B);
    let tx_bmu = intel_mmio_read32(info, INTEL_DISPIO_CR_TX_BMU_CR0);
    let de_pll_ctl = intel_mmio_read32(info, INTEL_BXT_DE_PLL_CTL);
    let de_pll_enable = intel_mmio_read32(info, INTEL_BXT_DE_PLL_ENABLE);
    let dc_state_en = intel_mmio_read32(info, INTEL_DC_STATE_EN);
    let dc_state_debug = intel_mmio_read32(info, INTEL_DC_STATE_DEBUG);
    let hotplug = intel_mmio_read32(info, INTEL_PORT_HOTPLUG_EN);
    let gt_disp_pwron = intel_mmio_read32(info, INTEL_GT_DISP_PWRON);

    crate::log!(
        "gfx-intel-scanout: power-probe phy_misc_a=0x{:08X} phy_misc_b=0x{:08X} tx_bmu=0x{:08X} de_pll_ctl=0x{:08X} de_pll_enable=0x{:08X} dc_state_en=0x{:08X} dc_state_debug=0x{:08X} hotplug=0x{:08X} gt_disp_pwron=0x{:08X}\n",
        phy_misc_a,
        phy_misc_b,
        tx_bmu,
        de_pll_ctl,
        de_pll_enable,
        dc_state_en,
        dc_state_debug,
        hotplug,
        gt_disp_pwron
    );
}

fn log_hdmi_port_probe(info: IntelGfxInfo) {
    let hotplug_en = intel_mmio_read32(info, INTEL_PORT_HOTPLUG_EN);
    let trans_a = intel_mmio_read32(info, INTEL_TRANS_A_DDI_FUNC_CTL);
    let trans_b = intel_mmio_read32(info, INTEL_TRANS_B_DDI_FUNC_CTL);
    let trans_c = intel_mmio_read32(info, INTEL_TRANS_C_DDI_FUNC_CTL);
    let trans_d = intel_mmio_read32(info, INTEL_TRANS_D_DDI_FUNC_CTL);
    let pipe_a = intel_mmio_read32(info, INTEL_PIPE_A_SRC);
    let pipe_b = intel_mmio_read32(info, INTEL_PIPE_B_SRC);
    let pipe_c = intel_mmio_read32(info, INTEL_PIPE_C_SRC);
    let pipe_d = intel_mmio_read32(info, INTEL_PIPE_D_SRC);
    crate::log!(
        "gfx-intel-scanout: hdmi-probe hotplug_en=0x{:08X} trans_a=0x{:08X} trans_b=0x{:08X} trans_c=0x{:08X} trans_d=0x{:08X} pipe_a=0x{:08X} pipe_b=0x{:08X} pipe_c=0x{:08X} pipe_d=0x{:08X}\n",
        hotplug_en,
        trans_a,
        trans_b,
        trans_c,
        trans_d,
        pipe_a,
        pipe_b,
        pipe_c,
        pipe_d
    );
}

fn log_display_focus_windows(info: IntelGfxInfo) {
    for &center in &[
        INTEL_BXT_DE_PLL_ENABLE,
        INTEL_PORT_HOTPLUG_EN,
        INTEL_TRANS_A_DDI_FUNC_CTL,
        INTEL_TRANS_B_DDI_FUNC_CTL,
        INTEL_TRANS_C_DDI_FUNC_CTL,
        INTEL_TRANS_D_DDI_FUNC_CTL,
        INTEL_GT_DISP_PWRON,
    ] {
        crate::log!("gfx-intel-scanout: focus-window center=0x{:05X}\n", center);
        log_display_window(info, center);
    }
}

fn arm_display_power_smoke(info: IntelGfxInfo) -> bool {
    let orig_pwron = intel_mmio_read32(info, INTEL_GT_DISP_PWRON);
    let req_pwron = orig_pwron | INTEL_GT_DISP_PWRON_REQ;
    let wrote = intel_mmio_write32(info, INTEL_GT_DISP_PWRON, req_pwron);
    let rb_pwron = intel_mmio_read32(info, INTEL_GT_DISP_PWRON);
    let hotplug = intel_mmio_read32(info, INTEL_PORT_HOTPLUG_EN);
    let de_pll_enable = intel_mmio_read32(info, INTEL_BXT_DE_PLL_ENABLE);
    let phy_misc_a = intel_mmio_read32(info, INTEL_ICL_PHY_MISC_A);
    let latched = wrote && (rb_pwron & INTEL_GT_DISP_PWRON_REQ) != 0;
    crate::log!(
        "gfx-intel-scanout: disp-pwron-smoke orig=0x{:08X} req=0x{:08X} rb=0x{:08X} hotplug=0x{:08X} de_pll_enable=0x{:08X} phy_misc_a=0x{:08X} latched={}\n",
        orig_pwron,
        req_pwron,
        rb_pwron,
        hotplug,
        de_pll_enable,
        phy_misc_a,
        latched as u8
    );
    crate::log!(
        "gfx-intel-scanout: post-pwron-window center=0x{:05X}\n",
        INTEL_GT_DISP_PWRON
    );
    log_display_window(info, INTEL_GT_DISP_PWRON);
    latched
}

fn hotplug_write_smoke(info: IntelGfxInfo) -> bool {
    let orig = intel_mmio_read32(info, INTEL_PORT_HOTPLUG_EN);
    let test = orig | INTEL_PORT_HOTPLUG_TEST_BIT;
    let wrote = intel_mmio_write32(info, INTEL_PORT_HOTPLUG_EN, test);
    let rb = intel_mmio_read32(info, INTEL_PORT_HOTPLUG_EN);
    let _ = intel_mmio_write32(info, INTEL_PORT_HOTPLUG_EN, orig);
    let restored = intel_mmio_read32(info, INTEL_PORT_HOTPLUG_EN);
    let latched = wrote && (rb & INTEL_PORT_HOTPLUG_TEST_BIT) != 0;
    crate::log!(
        "gfx-intel-scanout: hotplug-smoke orig=0x{:08X} test=0x{:08X} rb=0x{:08X} restore=0x{:08X} latched={}\n",
        orig,
        test,
        rb,
        restored,
        latched as u8
    );
    crate::log!(
        "gfx-intel-scanout: post-hotplug-window center=0x{:05X}\n",
        INTEL_PORT_HOTPLUG_EN
    );
    log_display_window(info, INTEL_PORT_HOTPLUG_EN);
    latched
}

fn signature_candidate_surface_smoke(info: IntelGfxInfo) {
    let ctl = intel_mmio_read32(info, INTEL_SIGNATURE_SMOKE_CTL_OFF);
    let surf = intel_mmio_read32(info, INTEL_SIGNATURE_SMOKE_SURF_OFF);
    let pipe_src = intel_mmio_read32(info, INTEL_SIGNATURE_SMOKE_PIPE_SRC_OFF);
    let stride = intel_mmio_read32(info, INTEL_SIGNATURE_SMOKE_STRIDE_OFF);
    let (width, height) = plausible_pipe_src(pipe_src).unwrap_or((0, 0));
    let surf_ok = plausible_scanout_surface(surf, info.aperture_bar_size);
    let stride_ok = plausible_scanout_stride(stride);
    let ctl_ok = ctl == INTEL_PLANE_ENABLE;
    if !ctl_ok || !surf_ok || !stride_ok || width == 0 || height == 0 {
        crate::log!(
            "gfx-intel-scanout: signature-smoke skip ctl=0x{:08X} surf=0x{:08X} pipe_src=0x{:08X} size={}x{} stride=0x{:08X} ctl_ok={} surf_ok={} stride_ok={}\n",
            ctl,
            surf,
            pipe_src,
            width,
            height,
            stride,
            ctl_ok as u8,
            surf_ok as u8,
            stride_ok as u8
        );
        return;
    }

    let test_surf = if (surf as u64).saturating_add(0x1000) < info.aperture_bar_size {
        surf.saturating_add(0x1000)
    } else if surf >= 0x1000 {
        surf.saturating_sub(0x1000)
    } else {
        surf
    };
    if test_surf == surf || !plausible_scanout_surface(test_surf, info.aperture_bar_size) {
        crate::log!(
            "gfx-intel-scanout: signature-smoke skip surf=0x{:08X} no alternate in aperture=0x{:X}\n",
            surf,
            info.aperture_bar_size
        );
        return;
    }

    let wrote = intel_mmio_write32(info, INTEL_SIGNATURE_SMOKE_SURF_OFF, test_surf);
    let rb = intel_mmio_read32(info, INTEL_SIGNATURE_SMOKE_SURF_OFF);
    let _ = intel_mmio_write32(info, INTEL_SIGNATURE_SMOKE_SURF_OFF, surf);
    let restored = intel_mmio_read32(info, INTEL_SIGNATURE_SMOKE_SURF_OFF);
    let latched = wrote && rb == test_surf && restored == surf;
    crate::log!(
        "gfx-intel-scanout: signature-smoke page=0x82000 ctl=0x{:08X} pipe_src=0x{:08X} size={}x{} stride=0x{:08X} surf orig=0x{:08X} test=0x{:08X} rb=0x{:08X} restore=0x{:08X} latched={}\n",
        ctl,
        pipe_src,
        width,
        height,
        stride,
        surf,
        test_surf,
        rb,
        restored,
        latched as u8
    );
}

fn probe_scanout_surface(info: IntelGfxInfo) -> Option<IntelScanoutSurface> {
    let mut found = None;
    let mut nonzero_pipes = 0usize;
    let mut nonzero_planes = 0usize;
    let mut enabled_planes = 0usize;
    for pipe in 0..INTEL_SCANOUT_PIPES.len() {
        let plane0 = scanout_plane(pipe, 0);
        let pipe_src = intel_mmio_read32(info, plane0.pipe_src_off);
        let trans_ddi = intel_mmio_read32(info, plane0.trans_ddi_func_ctl_off);
        let (pipe_w, pipe_h) = decode_pipe_src(pipe_src);
        if pipe_src != 0 || trans_ddi != 0 {
            nonzero_pipes += 1;
        }
        for plane_slot in 0..4 {
            let plane = scanout_plane(pipe, plane_slot);
            let ctl = intel_mmio_read32(info, plane.ctl_off);
            let stride = intel_mmio_read32(info, plane.stride_off) as usize;
            let surf = intel_mmio_read32(info, plane.surf_off);
            let surf_live = intel_mmio_read32(info, plane.surf_live_off);
            let enabled = (ctl & INTEL_PLANE_ENABLE) != 0;
            if enabled {
                enabled_planes += 1;
            }
            if ctl != 0 || stride != 0 || surf != 0 || surf_live != 0 {
                nonzero_planes += 1;
                crate::log!(
                    "gfx-intel-scanout: plane-live {}{} ctl=0x{:08X} stride=0x{:08X} surf=0x{:08X} surf_live=0x{:08X} enabled={}\n",
                    plane.pipe_name,
                    plane.plane_slot,
                    ctl,
                    stride as u32,
                    surf,
                    surf_live,
                    enabled as u8
                );
            }
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
    if found.is_none() && (nonzero_pipes != 0 || nonzero_planes != 0 || enabled_planes != 0) {
        crate::log!(
            "gfx-intel-scanout: plane-scan summary nonzero_pipes={} nonzero_planes={} enabled_planes={}\n",
            nonzero_pipes,
            nonzero_planes,
            enabled_planes
        );
    }
    found
}

fn write_scanout_test_pattern(base: *mut u8, stride: usize, width: usize, height: usize) {
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
            let diag1 = (width.saturating_sub(1).saturating_sub(x)).saturating_mul(height.max(1))
                / width.max(1);
            if y.abs_diff(diag0) <= 2 || y.abs_diff(diag1) <= 2 {
                color = 0x00000000;
            }
            if y > height / 3
                && y < (height / 3).saturating_mul(2)
                && x > width / 3
                && x < (width / 3).saturating_mul(2)
            {
                color = 0x00000000;
            }
            *px = color;
        }
    }
}

fn prepare_direct_demo_surface(info: IntelGfxInfo) -> Option<(u32, usize, usize, usize)> {
    if info.aperture_bar_phys == 0 || info.aperture_bar_size == 0 {
        crate::log!(
            "gfx-intel-scanout: direct-demo aperture unavailable bar2=0x{:X} size=0x{:X}\n",
            info.aperture_bar_phys,
            info.aperture_bar_size
        );
        return None;
    }

    let surf = INTEL_DIRECT_DEMO_SURF_OFF;
    let stride = INTEL_DIRECT_DEMO_STRIDE as usize;
    let width = INTEL_DIRECT_DEMO_WIDTH.min((stride / 4).max(1));
    let height = INTEL_DIRECT_DEMO_HEIGHT;
    let bytes = height.saturating_mul(stride);
    if (surf as u64).saturating_add(bytes as u64) > info.aperture_bar_size {
        crate::log!(
            "gfx-intel-scanout: direct-demo surf=0x{:08X} stride=0x{:X} bytes=0x{:X} exceeds aperture=0x{:X}\n",
            surf,
            stride,
            bytes,
            info.aperture_bar_size
        );
        return None;
    }

    let phys = info.aperture_bar_phys.saturating_add(surf as u64);
    let Ok(mapped) = crate::pci::mmio::map_mmio_region_exact(phys, bytes) else {
        crate::log!(
            "gfx-intel-scanout: direct-demo aperture map failed phys=0x{:X} bytes=0x{:X}\n",
            phys,
            bytes
        );
        return None;
    };

    write_scanout_test_pattern(mapped.as_ptr(), stride, width, height);
    let sample0 = unsafe { core::ptr::read_volatile(mapped.as_ptr() as *const u32) };
    let sample1 = unsafe { core::ptr::read_volatile(mapped.as_ptr().add(4) as *const u32) };
    crate::log!(
        "gfx-intel-scanout: direct-demo surface ready surf=0x{:08X} stride=0x{:X} size={}x{} phys=0x{:X} sample0=0x{:08X} sample1=0x{:08X}\n",
        surf,
        stride,
        width,
        height,
        phys,
        sample0,
        sample1
    );
    Some((surf, stride, width, height))
}

fn try_direct_plane_demo(info: IntelGfxInfo) -> bool {
    let Some((surf, stride, width, height)) = prepare_direct_demo_surface(info) else {
        return false;
    };
    let pipe_src = (((height.saturating_sub(1)) as u32) << 16) | ((width.saturating_sub(1)) as u32);
    let mut armed = false;

    for pipe in 0..INTEL_SCANOUT_PIPES.len() {
        let plane = scanout_plane(pipe, 0);
        let orig_ctl = intel_mmio_read32(info, plane.ctl_off);
        let orig_stride = intel_mmio_read32(info, plane.stride_off);
        let orig_surf = intel_mmio_read32(info, plane.surf_off);
        let orig_pipe_src = intel_mmio_read32(info, plane.pipe_src_off);
        let orig_ddi = intel_mmio_read32(info, plane.trans_ddi_func_ctl_off);

        let _ = intel_mmio_write32(info, plane.pipe_src_off, pipe_src);
        let rb_pipe_src = intel_mmio_read32(info, plane.pipe_src_off);
        let _ = intel_mmio_write32(info, plane.stride_off, stride as u32);
        let rb_stride = intel_mmio_read32(info, plane.stride_off);
        let _ = intel_mmio_write32(info, plane.surf_off, surf);
        let rb_surf = intel_mmio_read32(info, plane.surf_off);
        let _ = intel_mmio_write32(info, plane.ctl_off, INTEL_PLANE_ENABLE);
        let rb_ctl = intel_mmio_read32(info, plane.ctl_off);

        let pipe_stuck = rb_pipe_src == pipe_src;
        let stride_stuck = rb_stride == stride as u32;
        let surf_stuck = rb_surf == surf;
        let ctl_stuck = rb_ctl == INTEL_PLANE_ENABLE;

        crate::log!(
            "gfx-intel-scanout: direct-demo attempt pipe={} plane={} pipe_src orig=0x{:08X} rb=0x{:08X} stride orig=0x{:08X} rb=0x{:08X} surf orig=0x{:08X} rb=0x{:08X} ctl orig=0x{:08X} rb=0x{:08X} ddi=0x{:08X} stuck pipe={} stride={} surf={} ctl={}\n",
            plane.pipe_name,
            plane.plane_slot,
            orig_pipe_src,
            rb_pipe_src,
            orig_stride,
            rb_stride,
            orig_surf,
            rb_surf,
            orig_ctl,
            rb_ctl,
            orig_ddi,
            pipe_stuck as u8,
            stride_stuck as u8,
            surf_stuck as u8,
            ctl_stuck as u8
        );

        if pipe_stuck && stride_stuck && surf_stuck {
            crate::log!(
                "gfx-intel-scanout: direct-demo armed pipe={} plane={} surf=0x{:08X} stride=0x{:X} size={}x{}\n",
                plane.pipe_name,
                plane.plane_slot,
                surf,
                stride,
                width,
                height
            );
            armed = true;
            break;
        }

        let _ = intel_mmio_write32(info, plane.ctl_off, orig_ctl);
        let _ = intel_mmio_write32(info, plane.surf_off, orig_surf);
        let _ = intel_mmio_write32(info, plane.stride_off, orig_stride);
        let _ = intel_mmio_write32(info, plane.pipe_src_off, orig_pipe_src);
    }

    if !armed {
        crate::log!(
            "gfx-intel-scanout: direct-demo no candidate plane latched raw scanout state\n"
        );
    }
    armed
}

fn narrow_display_writeability_sweep(info: IntelGfxInfo) {
    let mut successes = 0usize;
    for &(start, end, label) in INTEL_WRITE_SWEEP_WINDOWS {
        let mut attempts = 0usize;
        let mut window_successes = 0usize;
        let mut logged_hits = 0usize;
        let mut first_hit_off = None;
        let mut off = start;
        while off + 4 <= end && attempts < INTEL_WRITE_SWEEP_MAX_ATTEMPTS_PER_WINDOW {
            let orig = intel_mmio_read32(info, off);
            if orig != 0 {
                off += 4;
                continue;
            }

            let wrote = intel_mmio_write32(info, off, INTEL_WRITE_SWEEP_TEST_VALUE);
            let rb = intel_mmio_read32(info, off);
            let _ = intel_mmio_write32(info, off, orig);
            let restored = intel_mmio_read32(info, off);
            let latched = wrote && rb == INTEL_WRITE_SWEEP_TEST_VALUE && restored == orig;
            if latched {
                if first_hit_off.is_none() {
                    first_hit_off = Some(off);
                }
                if logged_hits < INTEL_WRITE_SWEEP_HIT_LOG_LIMIT {
                    crate::log!(
                        "gfx-intel-scanout: write-sweep hit label={} off=0x{:05X}\n",
                        label,
                        off
                    );
                    logged_hits += 1;
                }
                window_successes += 1;
                successes += 1;
                if successes >= INTEL_WRITE_SWEEP_MAX_SUCCESSES {
                    crate::log!(
                        "gfx-intel-scanout: write-sweep stopping after {} total hits\n",
                        successes
                    );
                    return;
                }
            }

            attempts += 1;
            off += 4;
        }

        crate::log!(
            "gfx-intel-scanout: write-sweep label={} attempts={} hits={} first_hit={} start=0x{:05X} end=0x{:05X}\n",
            label,
            attempts,
            window_successes,
            first_hit_off.unwrap_or(0),
            start,
            end
        );
    }
}

fn walker_test_value(orig: u32) -> Option<u32> {
    if orig == u32::MAX {
        return None;
    }
    if orig == 0 {
        return Some(1);
    }
    if (orig & 1) == 0 {
        return Some(orig | 1);
    }
    if (orig & 2) == 0 {
        return Some(orig | 2);
    }
    None
}

fn log_pattern_window(info: IntelGfxInfo, center_off: usize, label: &str) {
    let prev = intel_mmio_read32(info, center_off.saturating_sub(4));
    let cur = intel_mmio_read32(info, center_off);
    let next = intel_mmio_read32(info, center_off.saturating_add(4));
    crate::log!(
        "gfx-intel-scanout: walker-near label={} center=0x{:05X} prev=0x{:08X} cur=0x{:08X} next=0x{:08X}\n",
        label,
        center_off,
        prev,
        cur,
        next
    );
}

fn walk_predicted_ranges(info: IntelGfxInfo) {
    for &(start, end, label) in INTEL_PATTERN_WALK_RANGES {
        let mut zero = 0usize;
        let mut nonzero = 0usize;
        let mut ffff = 0usize;
        let mut toggled = 0usize;
        let mut logged = 0usize;
        let mut off = start;
        while off + 4 <= end {
            let orig = intel_mmio_read32(info, off);
            if orig == 0 {
                zero += 1;
            } else if orig == u32::MAX {
                ffff += 1;
            } else {
                nonzero += 1;
            }

            let Some(test) = walker_test_value(orig) else {
                off += 4;
                continue;
            };
            let wrote = intel_mmio_write32(info, off, test);
            let rb = intel_mmio_read32(info, off);
            let _ = intel_mmio_write32(info, off, orig);
            let restored = intel_mmio_read32(info, off);
            let latched = wrote && rb == test && restored == orig;
            if latched {
                toggled += 1;
                if logged < INTEL_PATTERN_WALK_LOG_LIMIT {
                    crate::log!(
                        "gfx-intel-scanout: walker-hit label={} off=0x{:05X} orig=0x{:08X} test=0x{:08X} rb=0x{:08X} restored=0x{:08X}\n",
                        label,
                        off,
                        orig,
                        test,
                        rb,
                        restored
                    );
                    log_pattern_window(info, off, label);
                    logged += 1;
                }
            }
            off += 4;
        }

        crate::log!(
            "gfx-intel-scanout: walker-summary label={} start=0x{:05X} end=0x{:05X} zero={} nonzero={} ffff={} toggled={}\n",
            label,
            start,
            end,
            zero,
            nonzero,
            ffff,
            toggled
        );
    }
}

fn log_tuple_neighbor_changes(
    label: &str,
    target_off: usize,
    before: &[u32],
    after: &[u32],
    start: usize,
) {
    let mut changed = 0usize;
    let mut hit0_off = 0usize;
    let mut hit0_before = 0u32;
    let mut hit0_after = 0u32;
    let mut hit1_off = 0usize;
    let mut hit1_before = 0u32;
    let mut hit1_after = 0u32;
    let mut hit2_off = 0usize;
    let mut hit2_before = 0u32;
    let mut hit2_after = 0u32;
    let mut idx = 0usize;
    while idx < before.len() && idx < after.len() {
        if before[idx] != after[idx] {
            let off = start + idx.saturating_mul(4);
            if changed == 0 {
                hit0_off = off;
                hit0_before = before[idx];
                hit0_after = after[idx];
            } else if changed == 1 {
                hit1_off = off;
                hit1_before = before[idx];
                hit1_after = after[idx];
            } else if changed == 2 {
                hit2_off = off;
                hit2_before = before[idx];
                hit2_after = after[idx];
            }
            changed += 1;
        }
        idx += 1;
    }
    crate::log!(
        "gfx-intel-scanout: tuple-probe summary label={} target=0x{:05X} changed={} hit0=0x{:05X}:0x{:08X}->0x{:08X} hit1=0x{:05X}:0x{:08X}->0x{:08X} hit2=0x{:05X}:0x{:08X}->0x{:08X}\n",
        label,
        target_off,
        changed,
        hit0_off,
        hit0_before,
        hit0_after,
        hit1_off,
        hit1_before,
        hit1_after,
        hit2_off,
        hit2_before,
        hit2_after
    );
}

fn log_tuple_focus(info: IntelGfxInfo, label: &str) {
    let v_6c080 = intel_mmio_read32(info, 0x6C080);
    let v_6c084 = intel_mmio_read32(info, 0x6C084);
    let v_6c100 = intel_mmio_read32(info, 0x6C100);
    let v_6c104 = intel_mmio_read32(info, 0x6C104);
    let v_6c108 = intel_mmio_read32(info, 0x6C108);
    let v_6c110 = intel_mmio_read32(info, 0x6C110);
    let v_6c118 = intel_mmio_read32(info, 0x6C118);
    let v_6c120 = intel_mmio_read32(info, 0x6C120);
    crate::log!(
        "gfx-intel-scanout: tuple-focus label={} 080=0x{:08X} 084=0x{:08X} 100=0x{:08X} 104=0x{:08X} 108=0x{:08X} 110=0x{:08X} 118=0x{:08X} 120=0x{:08X}\n",
        label,
        v_6c080,
        v_6c084,
        v_6c100,
        v_6c104,
        v_6c108,
        v_6c110,
        v_6c118,
        v_6c120
    );
}

fn tuple_key_values(info: IntelGfxInfo) -> (u32, u32, u32, u32, u32) {
    (
        intel_mmio_read32(info, 0x6C100),
        intel_mmio_read32(info, 0x6C104),
        intel_mmio_read32(info, 0x6C108),
        intel_mmio_read32(info, 0x6C118),
        intel_mmio_read32(info, 0x6C120),
    )
}

fn log_tuple_downstream_state(info: IntelGfxInfo, label: &str) {
    let (k100, k104, k108, k118, k120) = tuple_key_values(info);
    let hotplug = intel_mmio_read32(info, INTEL_PORT_HOTPLUG_EN);
    let trans_a = intel_mmio_read32(info, INTEL_TRANS_A_DDI_FUNC_CTL);
    let trans_b = intel_mmio_read32(info, INTEL_TRANS_B_DDI_FUNC_CTL);
    let trans_c = intel_mmio_read32(info, INTEL_TRANS_C_DDI_FUNC_CTL);
    let trans_d = intel_mmio_read32(info, INTEL_TRANS_D_DDI_FUNC_CTL);
    let pipe_a = intel_mmio_read32(info, INTEL_PIPE_A_SRC);
    let pipe_b = intel_mmio_read32(info, INTEL_PIPE_B_SRC);
    let pipe_c = intel_mmio_read32(info, INTEL_PIPE_C_SRC);
    let pipe_d = intel_mmio_read32(info, INTEL_PIPE_D_SRC);
    let plane_a = scanout_plane(0, 0);
    let plane_b = scanout_plane(1, 0);
    let plane_c = scanout_plane(2, 0);
    let plane_d = scanout_plane(3, 0);
    let pa_ctl = intel_mmio_read32(info, plane_a.ctl_off);
    let pb_ctl = intel_mmio_read32(info, plane_b.ctl_off);
    let pc_ctl = intel_mmio_read32(info, plane_c.ctl_off);
    let pd_ctl = intel_mmio_read32(info, plane_d.ctl_off);
    let pa_surf = intel_mmio_read32(info, plane_a.surf_off);
    let pb_surf = intel_mmio_read32(info, plane_b.surf_off);
    let pc_surf = intel_mmio_read32(info, plane_c.surf_off);
    let pd_surf = intel_mmio_read32(info, plane_d.surf_off);
    crate::log!(
        "gfx-intel-scanout: tuple-downstream label={} k100=0x{:08X} k104=0x{:08X} k108=0x{:08X} k118=0x{:08X} k120=0x{:08X} hp=0x{:08X} trans=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] pipe=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] surf=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] ctl=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}]\n",
        label,
        k100,
        k104,
        k108,
        k118,
        k120,
        hotplug,
        trans_a,
        trans_b,
        trans_c,
        trans_d,
        pipe_a,
        pipe_b,
        pipe_c,
        pipe_d,
        pa_surf,
        pb_surf,
        pc_surf,
        pd_surf,
        pa_ctl,
        pb_ctl,
        pc_ctl,
        pd_ctl
    );
}

fn probe_tx_bmu_held_surface(info: IntelGfxInfo, surf: u32) {
    let held_off = 0x6C108usize;
    let held_orig = intel_mmio_read32(info, held_off);
    let _ = intel_mmio_write32(info, held_off, surf);
    let held_rb = intel_mmio_read32(info, held_off);
    crate::log!(
        "gfx-intel-scanout: tuple-held-surface orig=0x{:08X} hold=0x{:08X} rb=0x{:08X}\n",
        held_orig,
        surf,
        held_rb
    );
    log_tuple_downstream_state(info, "held-baseline");

    for &off in &[0x6C104usize, 0x6C10C, 0x6C114, 0x6C11C, 0x6C120] {
        let orig = intel_mmio_read32(info, off);
        let Some(test) = walker_test_value(orig) else {
            crate::log!(
                "gfx-intel-scanout: tuple-held-probe off=0x{:05X} orig=0x{:08X} skipped\n",
                off,
                orig
            );
            continue;
        };
        let _ = intel_mmio_write32(info, off, test);
        let rb = intel_mmio_read32(info, off);
        crate::log!(
            "gfx-intel-scanout: tuple-held-probe off=0x{:05X} orig=0x{:08X} test=0x{:08X} rb=0x{:08X}\n",
            off,
            orig,
            test,
            rb
        );
        log_tuple_downstream_state(info, "held-probe");
        let _ = intel_mmio_write32(info, off, orig);
    }

    let _ = intel_mmio_write32(info, held_off, held_orig);
}

fn probe_tx_bmu_tuple(info: IntelGfxInfo) {
    let Some((surf, stride, width, height)) = prepare_direct_demo_surface(info) else {
        return;
    };
    let pipe_src = (((height.saturating_sub(1)) as u32) << 16) | ((width.saturating_sub(1)) as u32);
    let snapshot_dwords = (INTEL_TUPLE_PROBE_END.saturating_sub(INTEL_TUPLE_PROBE_START)) / 4;
    let mut before = [0u32; (INTEL_TUPLE_PROBE_END - INTEL_TUPLE_PROBE_START) / 4];
    let mut after = [0u32; (INTEL_TUPLE_PROBE_END - INTEL_TUPLE_PROBE_START) / 4];

    let mut snap_idx = 0usize;
    while snap_idx < snapshot_dwords {
        before[snap_idx] =
            intel_mmio_read32(info, INTEL_TUPLE_PROBE_START + snap_idx.saturating_mul(4));
        snap_idx += 1;
    }

    let probes = [
        ("pipe-src-a", 0x6C080usize, pipe_src),
        ("stride-a", 0x6C084usize, stride as u32),
        ("surf-a", 0x6C108usize, surf),
        (
            "tuple-a",
            0x6C110usize,
            (pipe_src & 0xFFFF_0000) | ((stride as u32) & 0xFFFF),
        ),
    ];

    log_tuple_focus(info, "baseline");
    for &(label, off, test) in &probes {
        let orig = intel_mmio_read32(info, off);
        let _ = intel_mmio_write32(info, off, test);
        let rb = intel_mmio_read32(info, off);

        let mut idx = 0usize;
        while idx < snapshot_dwords {
            after[idx] = intel_mmio_read32(info, INTEL_TUPLE_PROBE_START + idx.saturating_mul(4));
            idx += 1;
        }

        crate::log!(
            "gfx-intel-scanout: tuple-probe write label={} off=0x{:05X} orig=0x{:08X} test=0x{:08X} rb=0x{:08X}\n",
            label,
            off,
            orig,
            test,
            rb
        );
        log_tuple_neighbor_changes(
            label,
            off,
            &before[..snapshot_dwords],
            &after[..snapshot_dwords],
            INTEL_TUPLE_PROBE_START,
        );

        let _ = intel_mmio_write32(info, off, orig);
    }

    let stride_tests = [
        ("stride-orig", 0x6C084usize, INTEL_DIRECT_DEMO_STRIDE >> 2),
        ("stride-demo", 0x6C084usize, stride as u32),
        (
            "stride-wide",
            0x6C084usize,
            (stride as u32).saturating_mul(2),
        ),
        ("stride-thin", 0x6C084usize, 0x00000200),
    ];
    for &(label, off, test) in &stride_tests {
        let orig = intel_mmio_read32(info, off);
        let _ = intel_mmio_write32(info, off, test);
        let rb = intel_mmio_read32(info, off);
        let (v100, v104, v108, v118, v120) = tuple_key_values(info);
        crate::log!(
            "gfx-intel-scanout: tuple-matrix label={} off=0x{:05X} orig=0x{:08X} test=0x{:08X} rb=0x{:08X} k100=0x{:08X} k104=0x{:08X} k108=0x{:08X} k118=0x{:08X} k120=0x{:08X}\n",
            label,
            off,
            orig,
            test,
            rb,
            v100,
            v104,
            v108,
            v118,
            v120
        );
        let _ = intel_mmio_write32(info, off, orig);
    }

    let surf_tests = [
        ("surf-zero", 0x6C108usize, 0x00000000),
        ("surf-demo", 0x6C108usize, surf),
        (
            "surf-next-page",
            0x6C108usize,
            surf.saturating_add(INTEL_DIRECT_DEMO_STRIDE),
        ),
        ("surf-far", 0x6C108usize, surf.saturating_add(0x0010_0000)),
    ];
    for &(label, off, test) in &surf_tests {
        let orig = intel_mmio_read32(info, off);
        let _ = intel_mmio_write32(info, off, test);
        let rb = intel_mmio_read32(info, off);
        let (v100, v104, v108, v118, v120) = tuple_key_values(info);
        crate::log!(
            "gfx-intel-scanout: tuple-matrix label={} off=0x{:05X} orig=0x{:08X} test=0x{:08X} rb=0x{:08X} k100=0x{:08X} k104=0x{:08X} k108=0x{:08X} k118=0x{:08X} k120=0x{:08X}\n",
            label,
            off,
            orig,
            test,
            rb,
            v100,
            v104,
            v108,
            v118,
            v120
        );
        let _ = intel_mmio_write32(info, off, orig);
    }

    let derived_tests = [
        ("derived-6C100", 0x6C100usize),
        ("derived-6C118", 0x6C118usize),
    ];
    for &(label, off) in &derived_tests {
        let orig = intel_mmio_read32(info, off);
        let Some(test) = walker_test_value(orig) else {
            crate::log!(
                "gfx-intel-scanout: tuple-matrix label={} off=0x{:05X} orig=0x{:08X} skipped\n",
                label,
                off,
                orig
            );
            continue;
        };
        let _ = intel_mmio_write32(info, off, test);
        let rb = intel_mmio_read32(info, off);
        let (v100, v104, v108, v118, v120) = tuple_key_values(info);
        crate::log!(
            "gfx-intel-scanout: tuple-matrix label={} off=0x{:05X} orig=0x{:08X} test=0x{:08X} rb=0x{:08X} k100=0x{:08X} k104=0x{:08X} k108=0x{:08X} k118=0x{:08X} k120=0x{:08X}\n",
            label,
            off,
            orig,
            test,
            rb,
            v100,
            v104,
            v108,
            v118,
            v120
        );
        let _ = intel_mmio_write32(info, off, orig);
    }

    probe_tx_bmu_held_surface(info, surf);
}

fn try_scanout_surface_demo(info: IntelGfxInfo) -> bool {
    let Some(surface) = probe_scanout_surface(info) else {
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
            let test_stride = INTEL_PLANE_WRITE_SMOKE_STRIDE_BASE
                + (pipe as u32 * 0x40)
                + (plane_slot as u32 * 0x10);
            let test_surf = INTEL_PLANE_WRITE_SMOKE_SURF_BASE
                + (pipe as u32 * 0x0020_0000)
                + (plane_slot as u32 * 0x0002_0000);

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

fn minimal_pattern_register_poke(info: IntelGfxInfo) {
    // Prepare demo surface with test pattern
    let Some((surf, stride, width, height)) = prepare_direct_demo_surface(info) else {
        return;
    };

    crate::log!(
        "gfx-intel-scanout: minimal-pattern poke starting surf=0x{:08X} stride=0x{:X} {}x{}\n",
        surf,
        stride,
        width,
        height
    );

    // Helper macro to log current state of key registers
    macro_rules! log_state {
        ($label:expr) => {{
            let de_pll_enable = intel_mmio_read32(info, 0x46070);
            let phy_misc_a = intel_mmio_read32(info, 0x64C00);
            let trans_a = intel_mmio_read32(info, 0x60400);
            let trans_b = intel_mmio_read32(info, 0x61400);
            let trans_c = intel_mmio_read32(info, 0x62400);
            let trans_d = intel_mmio_read32(info, 0x63400);
            let pipe_a = intel_mmio_read32(info, 0x6001C);
            let pipe_b = intel_mmio_read32(info, 0x6101C);
            let pipe_c = intel_mmio_read32(info, 0x6201C);
            let pipe_d = intel_mmio_read32(info, 0x6301C);
            crate::log!(
                "gfx-intel-scanout: minimal-pattern {} de_pll_en=0x{:08X} phy_misc_a=0x{:08X} trans=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] pipe=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}]\n",
                $label,
                de_pll_enable,
                phy_misc_a,
                trans_a,
                trans_b,
                trans_c,
                trans_d,
                pipe_a,
                pipe_b,
                pipe_c,
                pipe_d
            );
        }};
    }

    // Log initial state
    log_state!("init");

    // Poke 0x45014 (dc-pll hit)
    crate::log!("gfx-intel-scanout: minimal-pattern probing 0x45014\n");
    let orig_45014 = intel_mmio_read32(info, 0x45014);
    let _ = intel_mmio_write32(info, 0x45014, 0x00000001);
    let rb_45014 = intel_mmio_read32(info, 0x45014);
    log_state!("after-0x45014-write");
    let _ = intel_mmio_write32(info, 0x45014, orig_45014);
    crate::log!(
        "gfx-intel-scanout: minimal-pattern 0x45014 orig=0x{:08X} test=0x00000001 rb=0x{:08X}\n",
        orig_45014,
        rb_45014
    );

    // Poke 0x45010 (nearby)
    crate::log!("gfx-intel-scanout: minimal-pattern probing 0x45010\n");
    let orig_45010 = intel_mmio_read32(info, 0x45010);
    let _ = intel_mmio_write32(info, 0x45010, 0x00000001);
    let rb_45010 = intel_mmio_read32(info, 0x45010);
    log_state!("after-0x45010-write");
    let _ = intel_mmio_write32(info, 0x45010, orig_45010);
    crate::log!(
        "gfx-intel-scanout: minimal-pattern 0x45010 orig=0x{:08X} test=0x00000001 rb=0x{:08X}\n",
        orig_45010,
        rb_45010
    );

    // Poke 0x45020 (nearby)
    crate::log!("gfx-intel-scanout: minimal-pattern probing 0x45020\n");
    let orig_45020 = intel_mmio_read32(info, 0x45020);
    let _ = intel_mmio_write32(info, 0x45020, 0x00000001);
    let rb_45020 = intel_mmio_read32(info, 0x45020);
    log_state!("after-0x45020-write");
    let _ = intel_mmio_write32(info, 0x45020, orig_45020);
    crate::log!(
        "gfx-intel-scanout: minimal-pattern 0x45020 orig=0x{:08X} test=0x00000001 rb=0x{:08X}\n",
        orig_45020,
        rb_45020
    );

    // Poke 0x46000 (dc-pll hit)
    crate::log!("gfx-intel-scanout: minimal-pattern probing 0x46000\n");
    let orig_46000 = intel_mmio_read32(info, 0x46000);
    let _ = intel_mmio_write32(info, 0x46000, 0x00000001);
    let rb_46000 = intel_mmio_read32(info, 0x46000);
    log_state!("after-0x46000-write");
    let _ = intel_mmio_write32(info, 0x46000, orig_46000);
    crate::log!(
        "gfx-intel-scanout: minimal-pattern 0x46000 orig=0x{:08X} test=0x00000001 rb=0x{:08X}\n",
        orig_46000,
        rb_46000
    );

    // Poke 0x46070 (de_pll_enable)
    crate::log!("gfx-intel-scanout: minimal-pattern probing 0x46070\n");
    let orig_46070 = intel_mmio_read32(info, 0x46070);
    let test_46070 = orig_46070 | 0x00000001;
    let _ = intel_mmio_write32(info, 0x46070, test_46070);
    let rb_46070 = intel_mmio_read32(info, 0x46070);
    log_state!("after-0x46070-write");
    let _ = intel_mmio_write32(info, 0x46070, orig_46070);
    crate::log!(
        "gfx-intel-scanout: minimal-pattern 0x46070 orig=0x{:08X} test=0x{:08X} rb=0x{:08X}\n",
        orig_46070,
        test_46070,
        rb_46070
    );

    crate::log!("gfx-intel-scanout: minimal-pattern poke complete\n");
}

#[embassy_executor::task]
pub async fn scanout_smoke_task() {
    let Some(info) = first_claimed_device() else {
        crate::log!("gfx-intel-scanout: skipped (no claimed Intel gfx device)\n");
        return;
    };

    Timer::after(EmbassyDuration::from_millis(1200)).await;
    log_display_power_probe(info);
    log_hdmi_port_probe(info);
    if !INTEL_PASSIVE_ONLY_DEFAULT {
        let disp_pwron_latched = arm_display_power_smoke(info);
        let hotplug_latched = hotplug_write_smoke(info);
        if disp_pwron_latched {
            Timer::after(EmbassyDuration::from_millis(25)).await;
            crate::log!("gfx-intel-scanout: re-probing after GT_DISP_PWRON latch\n");
            log_display_power_probe(info);
        }
        plane_write_smoke_test(info);
        if disp_pwron_latched || hotplug_latched {
            plane_write_smoke_test(info);
        }
    }

    let _ = try_direct_plane_demo(info);
    minimal_pattern_register_poke(info);
    narrow_display_writeability_sweep(info);
    walk_predicted_ranges(info);
    probe_tx_bmu_tuple(info);

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
            crate::log!("gfx-intel-scanout: giving up after {} retries\n", tries);
            return;
        }
        Timer::after(EmbassyDuration::from_millis(INTEL_SCANOUT_RETRY_MS)).await;
    }
}
