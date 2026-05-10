use core::ptr::{NonNull, write_bytes};

use embassy_time::{Duration as EmbassyDuration, Timer};
use heapless::Vec;
use spin::Mutex;

// Bring-up trail markers:
// 1. BAR0 and BAR2 must be reassigned into guest-safe MMIO/aperture space before Intel probing is trustworthy.
// 2. GT/MMIO is alive on UHD 770: forcewake works and modern render-side probes return stable nonzero state.
// 3. The direct demo surface in BAR2/aperture is valid and can hold a known test pattern.
// 4. Final scanout endpoint registers remain inert: hotplug/DDI/pipeconf/transcoder/pipe/plane state stays zero.
// 5. The 0x6C0xx island is real and writable, but only as a local tuple; it never propagates to scanout.
// 6. The 0x45000/0x46000 DC/PLL island is real and writable, but bundle and bridge sequences still do not wake routing.
// 7. The tight power seam is now 0x45500/0x45504/0x45510/0x45520; 0x45520 only latches once the first three are held.
// 8. Even with the expanded 0x455xx seam plus the live GT triplet (0x13807C/0x138088/0x13810C), the display route stays dark.
// 9. Host i915 brings display up as a ladder: PW_1 -> always-on -> DC_off -> PW_2..PW_5 -> connector objects -> AUX/DDI_IO -> route.
// 10. On ADLP/RPL a sinkless non-legacy TC port tends to default toward TBT-alt; DP-alt admission may never surface without live HPD.

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
const INTEL_PORT_HOTPLUG_STAT: usize = 0x61114;
const INTEL_GEN11_DE_HPD_ISR: usize = 0x44470;
const INTEL_SDEISR: usize = 0xC4000;
const INTEL_BXT_DE_PLL_CTL: usize = 0x6D000;
const INTEL_BXT_DE_PLL_ENABLE: usize = 0x46070;
const INTEL_DC_STATE_EN: usize = 0x45504;
const INTEL_DC_STATE_DEBUG: usize = 0x45520;
const INTEL_HSW_PWR_WELL_CTL2: usize = 0x45404;
const INTEL_HSW_PWR_WELL_CTL5: usize = 0x45410;
const INTEL_ICL_PWR_WELL_CTL_AUX2: usize = 0x45444;
const INTEL_ICL_PWR_WELL_CTL_DDI2: usize = 0x45454;
const INTEL_GEN6_PCODE_MAILBOX: usize = 0x138124;
const INTEL_GEN6_PCODE_DATA: usize = 0x138128;
const INTEL_GEN6_PCODE_DATA1: usize = 0x13812C;
const INTEL_GT_DISP_PWRON: usize = 0x138090;
const INTEL_DDI_BUF_CTL_0: usize = 0x64000;
const INTEL_DDI_BUF_CTL_1: usize = 0x64100;
const INTEL_DDI_BUF_CTL_2: usize = 0x64200;
const INTEL_TC_DDI_BUF_CTL_CANDIDATES: [usize; 4] = [0x64300, 0x64400, 0x64500, 0x64600];
const INTEL_TC_DP_AUX_CH_CTL_CANDIDATES: [usize; 4] = [0x64310, 0x64410, 0x64510, 0x64610];
const INTEL_TC3_DDI_BUF_CTL: usize = 0x64500;
const INTEL_TC3_DP_AUX_CH_CTL: usize = 0x64510;
// TC3 is tc_port=2 (TC_PORT_3) on ADLP/RPL. For modular FIA this maps to
// phy_fia=1 and phy_fia_idx=0, while TCSS_DDI_STATUS uses PICK_EVEN(tc_port).
const INTEL_TC3_PORT_INDEX: u32 = 2;
const INTEL_TC3_PHY_FIA: usize = 1;
const INTEL_TC3_PHY_FIA_IDX: u32 = 0;
const INTEL_TCSS_DDI_STATUS_TC3: usize = 0x161500;
const INTEL_TCSS_DDI_STATUS_TC3_CANDIDATES: [usize; 4] = [0x161500, 0x161504, 0x161508, 0x16150C];
const INTEL_TC3_DKL_CMN_UC_DW_27_MMIO: usize = 0x16A36C;
const INTEL_HIP_INDEX_REG0: usize = 0x1010A0;
const INTEL_TC3_DKL_BANK_SHIFT: u32 = 8 * INTEL_TC3_PORT_INDEX;
const INTEL_TC3_DKL_BANK_MASK: u32 = 0xFF << INTEL_TC3_DKL_BANK_SHIFT;
const INTEL_TC3_DKL_BANK_IDX_UC_DW27: u32 = 2;
const INTEL_TC3_DKL_BANK_SHIFT_CANDIDATES: [u32; 4] = [0, 8, 16, 24];
const INTEL_FIA2_DFLEXPA1: usize = 0x16E880;
const INTEL_FIA2_DFLEXDPPMS: usize = 0x16E890;
const INTEL_FIA2_DFLEXDPCSSS: usize = 0x16E894;
const INTEL_FIA2_DFLEXDPSP: usize = 0x16E8A0;
const INTEL_FIA2_DFLEXDPMLE1: usize = 0x16E8C0;
const INTEL_FIA1_DFLEXPA1: usize = 0x163880;
const INTEL_FIA1_DFLEXDPPMS: usize = 0x163890;
const INTEL_FIA1_DFLEXDPCSSS: usize = 0x163894;
const INTEL_FIA1_DFLEXDPSP: usize = 0x1638A0;
const INTEL_FIA1_DFLEXDPMLE1: usize = 0x1638C0;
const INTEL_PIPECONF_A: usize = 0x70008;
const INTEL_PIPECONF_B: usize = 0x71008;
const INTEL_PIPECONF_C: usize = 0x72008;
const INTEL_PIPECONF_D: usize = 0x73008;
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
const INTEL_POWER_FIRST_START: usize = 0x454E0;
const INTEL_POWER_FIRST_END: usize = 0x45540;
const INTEL_POWER_FIRST_HIT_LOG_LIMIT: usize = 8;
const INTEL_COMPACT_CROSS_ISLAND_ONLY: bool = true;
const INTEL_PW_REQ_IDX_PW1: u32 = 0;
const INTEL_PW_REQ_IDX_PW2: u32 = 1;
const INTEL_PW_REQ_IDX_PW3: u32 = 2;
const INTEL_PW_REQ_IDX_PW4: u32 = 3;
const INTEL_PW_REQ_IDX_PW5: u32 = 4;
const INTEL_PW_REQ_IDX_TC3: u32 = 5;
const INTEL_DC_STATE_MASK_COMPACT: u32 = (1 << 0) | (1 << 1) | (1 << 3) | (1 << 30);
const INTEL_DDI_BUF_CTL_TC_PHY_OWNERSHIP: u32 = 1 << 6;
const INTEL_TCSS_DDI_STATUS_READY: u32 = 1 << 2;
const INTEL_TCSS_DDI_STATUS_HPD_ALT: u32 = 1 << 0;
const INTEL_TCSS_DDI_STATUS_HPD_TBT: u32 = 1 << 1;
const INTEL_DP_AUX_CH_CTL_TBT_IO: u32 = 1 << 11;
const INTEL_DKL_CMN_UC_DW27_UC_HEALTH: u32 = 1 << 15;
const INTEL_FIA_TC3_READY: u32 = 1 << INTEL_TC3_PHY_FIA_IDX;
const INTEL_FIA_TC3_OWNED: u32 = 1 << INTEL_TC3_PHY_FIA_IDX;
const INTEL_FIA_TC3_LIVE_TC: u32 = 1 << (INTEL_TC3_PHY_FIA_IDX * 8 + 5);
const INTEL_FIA_TC3_LIVE_TBT: u32 = 1 << (INTEL_TC3_PHY_FIA_IDX * 8 + 6);
const INTEL_GEN6_PCODE_READY: u32 = 1 << 31;
const INTEL_TGL_PCODE_TCCOLD: u32 = 0x26;
const INTEL_TGL_PCODE_TCCOLD_BLOCK_REQ: u32 = 0;
const INTEL_TGL_PCODE_TCCOLD_EXIT_FAILED: u32 = 1 << 0;
const INTEL_DISPLAY_CORE_SWEEP_MAX_IDX: u32 = 5;

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
        crate::log_trace!(
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
            crate::log_trace!(
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
            crate::log_trace!(
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
        crate::log_trace!(
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
    crate::log_trace!(
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
        crate::log_trace!(
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
        crate::log_trace!(
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

    crate::log_trace!(
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
        crate::log_trace!(
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
        crate::log_trace!(
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
    crate::log_trace!(
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
        crate::log_trace!(
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
        crate::log_trace!(
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
        crate::log_trace!("gfx-intel: no HHDM\n");
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
            crate::log_trace!(
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
            crate::log_trace!(
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
                crate::log_trace!(
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

        crate::log_trace!(
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
            crate::log_trace!(
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
            crate::log_trace!("gfx-intel: no Intel display-class PCI device found\n");
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

fn intel_pcode_read32_compact(
    info: IntelGfxInfo,
    mailbox: u32,
    low: &mut u32,
    high: &mut u32,
) -> Option<u32> {
    if (intel_mmio_read32(info, INTEL_GEN6_PCODE_MAILBOX) & INTEL_GEN6_PCODE_READY) != 0 {
        return None;
    }
    let _ = intel_mmio_write32(info, INTEL_GEN6_PCODE_DATA, *low);
    let _ = intel_mmio_write32(info, INTEL_GEN6_PCODE_DATA1, *high);
    let _ = intel_mmio_write32(
        info,
        INTEL_GEN6_PCODE_MAILBOX,
        INTEL_GEN6_PCODE_READY | mailbox,
    );
    for _ in 0..4096 {
        let status = intel_mmio_read32(info, INTEL_GEN6_PCODE_MAILBOX);
        if (status & INTEL_GEN6_PCODE_READY) == 0 {
            *low = intel_mmio_read32(info, INTEL_GEN6_PCODE_DATA);
            *high = intel_mmio_read32(info, INTEL_GEN6_PCODE_DATA1);
            return Some(status);
        }
    }
    None
}

fn intel_tgl_tc_cold_block_compact(info: IntelGfxInfo) -> (bool, u32, u32, u32) {
    let mut low = INTEL_TGL_PCODE_TCCOLD_BLOCK_REQ;
    let mut high = 0u32;
    let status = intel_pcode_read32_compact(info, INTEL_TGL_PCODE_TCCOLD, &mut low, &mut high)
        .unwrap_or(0xFFFF_FFFF);
    let ok = status != 0xFFFF_FFFF && (low & INTEL_TGL_PCODE_TCCOLD_EXIT_FAILED) == 0;
    (ok, status, low, high)
}

fn intel_dkl_tc3_read32(info: IntelGfxInfo, mmio_off: usize, bank_idx: u32) -> u32 {
    let hip_orig = intel_mmio_read32(info, INTEL_HIP_INDEX_REG0);
    let hip_test = (hip_orig & !INTEL_TC3_DKL_BANK_MASK) | (bank_idx << INTEL_TC3_DKL_BANK_SHIFT);
    let _ = intel_mmio_write32(info, INTEL_HIP_INDEX_REG0, hip_test);
    let value = intel_mmio_read32(info, mmio_off);
    let _ = intel_mmio_write32(info, INTEL_HIP_INDEX_REG0, hip_orig);
    value
}

fn intel_dkl_read32_shifted(
    info: IntelGfxInfo,
    mmio_off: usize,
    bank_idx: u32,
    bank_shift: u32,
) -> u32 {
    let bank_mask = 0xFFu32 << bank_shift;
    let hip_orig = intel_mmio_read32(info, INTEL_HIP_INDEX_REG0);
    let hip_test = (hip_orig & !bank_mask) | (bank_idx << bank_shift);
    let _ = intel_mmio_write32(info, INTEL_HIP_INDEX_REG0, hip_test);
    let value = intel_mmio_read32(info, mmio_off);
    let _ = intel_mmio_write32(info, INTEL_HIP_INDEX_REG0, hip_orig);
    value
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

include!("intel_disp.rs");

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
                    crate::log_trace!(
                        "gfx-intel-scanout: write-sweep hit label={} off=0x{:05X}\n",
                        label,
                        off
                    );
                    logged_hits += 1;
                }
                window_successes += 1;
                successes += 1;
                if successes >= INTEL_WRITE_SWEEP_MAX_SUCCESSES {
                    crate::log_trace!(
                        "gfx-intel-scanout: write-sweep stopping after {} total hits\n",
                        successes
                    );
                    return;
                }
            }

            attempts += 1;
            off += 4;
        }

        crate::log_trace!(
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
    crate::log_trace!(
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
                    crate::log_trace!(
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

        crate::log_trace!(
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
    crate::log_trace!(
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
    crate::log_trace!(
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
    crate::log_trace!(
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
    crate::log_trace!(
        "gfx-intel-scanout: tuple-held-surface orig=0x{:08X} hold=0x{:08X} rb=0x{:08X}\n",
        held_orig,
        surf,
        held_rb
    );
    log_tuple_downstream_state(info, "held-baseline");

    for &off in &[0x6C104usize, 0x6C10C, 0x6C114, 0x6C11C, 0x6C120] {
        let orig = intel_mmio_read32(info, off);
        let Some(test) = walker_test_value(orig) else {
            crate::log_trace!(
                "gfx-intel-scanout: tuple-held-probe off=0x{:05X} orig=0x{:08X} skipped\n",
                off,
                orig
            );
            continue;
        };
        let _ = intel_mmio_write32(info, off, test);
        let rb = intel_mmio_read32(info, off);
        crate::log_trace!(
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

        crate::log_trace!(
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
        crate::log_trace!(
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
        crate::log_trace!(
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
            crate::log_trace!(
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
        crate::log_trace!(
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
        crate::log_trace!(
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
        crate::log_trace!(
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
        crate::log_trace!(
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
        crate::log_trace!(
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
    crate::log_trace!(
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

            crate::log_trace!(
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

    crate::log_trace!(
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
        crate::log_trace!("gfx-intel-demo: skipped (no claimed Intel gfx device)\n");
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
            crate::log_trace!("gfx-intel-demo: centered triangle submitted\n");
            return;
        }

        tries = tries.saturating_add(1);
        if tries == 1 || tries.is_multiple_of(20) {
            crate::log_trace!("gfx-intel-demo: draw retry rc={} tries={}\n", rc, tries);
        }

        if rc == -3 && tries >= 8 {
            if draw_centered_triangle_limine_fallback() {
                crate::log_trace!("gfx-intel-demo: fallback triangle rasterized via Limine fb\n");
            } else {
                crate::log_trace!("gfx-intel-demo: fallback rasterizer unavailable\n");
            }
            return;
        }

        if tries >= 200 {
            if draw_centered_triangle_limine_fallback() {
                crate::log_trace!(
                    "gfx-intel-demo: fallback triangle rasterized after {} retries\n",
                    tries
                );
            } else {
                crate::log_trace!("gfx-intel-demo: giving up after {} retries\n", tries);
            }
            return;
        }

        Timer::after(EmbassyDuration::from_millis(25)).await;
    }
}

fn power_first_gate_hunt(info: IntelGfxInfo) {
    let hold_offsets = [0x45500usize, INTEL_DC_STATE_EN, 0x45510usize];
    let mut hold_orig = [0u32; 3];
    let mut hold_rb = [0u32; 3];
    for idx in 0..hold_offsets.len() {
        hold_orig[idx] = intel_mmio_read32(info, hold_offsets[idx]);
        let _ = intel_mmio_write32(info, hold_offsets[idx], hold_orig[idx] | 0x00000001);
        hold_rb[idx] = intel_mmio_read32(info, hold_offsets[idx]);
    }

    let de_pll_enable = intel_mmio_read32(info, INTEL_BXT_DE_PLL_ENABLE);
    let dc_state_debug = intel_mmio_read32(info, INTEL_DC_STATE_DEBUG);
    let gt_disp_pwron = intel_mmio_read32(info, INTEL_GT_DISP_PWRON);
    let hotplug_en = intel_mmio_read32(info, INTEL_PORT_HOTPLUG_EN);
    let hotplug_stat = intel_mmio_read32(info, INTEL_PORT_HOTPLUG_STAT);
    crate::log_trace!(
        "gfx-intel-scanout: power-first hold rb=[0x{:08X},0x{:08X},0x{:08X}] de=0x{:08X} dbg=0x{:08X} gt_pw=0x{:08X} hp=[0x{:08X},0x{:08X}]\n",
        hold_rb[0],
        hold_rb[1],
        hold_rb[2],
        de_pll_enable,
        dc_state_debug,
        gt_disp_pwron,
        hotplug_en,
        hotplug_stat
    );

    let mut attempts = 0usize;
    let mut hits = 0usize;
    let mut first_hit = 0usize;
    for off in (INTEL_POWER_FIRST_START..INTEL_POWER_FIRST_END).step_by(4) {
        if hold_offsets.contains(&off) {
            continue;
        }
        attempts = attempts.saturating_add(1);
        let orig = intel_mmio_read32(info, off);
        let Some(test) = walker_test_value(orig) else {
            continue;
        };
        let _ = intel_mmio_write32(info, off, test);
        let rb = intel_mmio_read32(info, off);
        let latched = rb == test;
        let _ = intel_mmio_write32(info, off, orig);
        if !latched {
            continue;
        }
        hits = hits.saturating_add(1);
        if first_hit == 0 {
            first_hit = off;
        }
        if hits <= INTEL_POWER_FIRST_HIT_LOG_LIMIT {
            let dc_state_en = intel_mmio_read32(info, INTEL_DC_STATE_EN);
            let dc_state_debug = intel_mmio_read32(info, INTEL_DC_STATE_DEBUG);
            let gt_disp_pwron = intel_mmio_read32(info, INTEL_GT_DISP_PWRON);
            let hotplug_stat = intel_mmio_read32(info, INTEL_PORT_HOTPLUG_STAT);
            let ddi0 = intel_mmio_read32(info, INTEL_DDI_BUF_CTL_0);
            let ddi1 = intel_mmio_read32(info, INTEL_DDI_BUF_CTL_1);
            let ddi2 = intel_mmio_read32(info, INTEL_DDI_BUF_CTL_2);
            let pc_a = intel_mmio_read32(info, INTEL_PIPECONF_A);
            let pc_b = intel_mmio_read32(info, INTEL_PIPECONF_B);
            let pc_c = intel_mmio_read32(info, INTEL_PIPECONF_C);
            let pc_d = intel_mmio_read32(info, INTEL_PIPECONF_D);
            crate::log_trace!(
                "gfx-intel-scanout: power-first hit off=0x{:05X} orig=0x{:08X} rb=0x{:08X} dc=[0x{:08X},0x{:08X}] gt_pw=0x{:08X} hp=0x{:08X} ddi=[0x{:08X},0x{:08X},0x{:08X}] pipeconf=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}]\n",
                off,
                orig,
                rb,
                dc_state_en,
                dc_state_debug,
                gt_disp_pwron,
                hotplug_stat,
                ddi0,
                ddi1,
                ddi2,
                pc_a,
                pc_b,
                pc_c,
                pc_d
            );
        }
    }

    crate::log_trace!(
        "gfx-intel-scanout: power-first summary start=0x{:05X} end=0x{:05X} attempts={} hits={} first_hit=0x{:05X}\n",
        INTEL_POWER_FIRST_START,
        INTEL_POWER_FIRST_END,
        attempts,
        hits,
        first_hit
    );

    for idx in (0..hold_offsets.len()).rev() {
        let _ = intel_mmio_write32(info, hold_offsets[idx], hold_orig[idx]);
    }
}

fn minimal_pattern_register_poke(info: IntelGfxInfo) {
    let pw_req_mask = |idx: u32| -> u32 { 0x2u32 << (idx * 2) };
    let pw_state_mask = |idx: u32| -> u32 { 0x1u32 << (idx * 2) };
    let mut read_route_sig = || -> [u32; 12] {
        [
            intel_mmio_read32(info, INTEL_PORT_HOTPLUG_STAT),
            intel_mmio_read32(info, INTEL_DDI_BUF_CTL_0),
            intel_mmio_read32(info, INTEL_DDI_BUF_CTL_1),
            intel_mmio_read32(info, INTEL_DDI_BUF_CTL_2),
            intel_mmio_read32(info, INTEL_PIPECONF_A),
            intel_mmio_read32(info, INTEL_PIPECONF_B),
            intel_mmio_read32(info, INTEL_PIPECONF_C),
            intel_mmio_read32(info, INTEL_PIPECONF_D),
            intel_mmio_read32(info, INTEL_TRANS_A_DDI_FUNC_CTL),
            intel_mmio_read32(info, INTEL_TRANS_B_DDI_FUNC_CTL),
            intel_mmio_read32(info, INTEL_TRANS_C_DDI_FUNC_CTL),
            intel_mmio_read32(info, INTEL_TRANS_D_DDI_FUNC_CTL),
        ]
    };
    let route_changed = |before: &[u32; 12], after: &[u32; 12]| before != after;
    let log_route_hit = |label: &str, before: &[u32; 12], after: &[u32; 12]| {
        crate::log_trace!(
            "gfx-intel-scanout: compact-hit label={} hp=0x{:08X}->0x{:08X} ddi=[0x{:08X},0x{:08X},0x{:08X}]->[0x{:08X},0x{:08X},0x{:08X}] pipeconf=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}]->[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] trans=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}]->[0x{:08X},0x{:08X},0x{:08X},0x{:08X}]\n",
            label,
            before[0],
            after[0],
            before[1],
            before[2],
            before[3],
            after[1],
            after[2],
            after[3],
            before[4],
            before[5],
            before[6],
            before[7],
            after[4],
            after[5],
            after[6],
            after[7],
            before[8],
            before[9],
            before[10],
            before[11],
            after[8],
            after[9],
            after[10],
            after[11]
        );
    };
    let read_dp_route_sig = || -> [u32; 5] {
        [
            intel_mmio_read32(info, INTEL_PORT_HOTPLUG_STAT),
            intel_mmio_read32(info, INTEL_DDI_BUF_CTL_0),
            intel_mmio_read32(info, INTEL_PIPECONF_A),
            intel_mmio_read32(info, INTEL_TRANS_A_DDI_FUNC_CTL),
            intel_mmio_read32(info, INTEL_PIPE_A_SRC),
        ]
    };
    let dp_route_changed = |before: &[u32; 5], after: &[u32; 5]| before != after;
    let apply_bits = |offsets: &[usize], tests: &[u32]| {
        for idx in 0..offsets.len() {
            let _ = intel_mmio_write32(info, offsets[idx], tests[idx]);
        }
    };
    let restore_bits = |offsets: &[usize], orig: &[u32]| {
        for idx in (0..offsets.len()).rev() {
            let _ = intel_mmio_write32(info, offsets[idx], orig[idx]);
        }
    };

    if INTEL_COMPACT_CROSS_ISLAND_ONLY {
        let seam_offsets = [0x45500usize, 0x45510usize, INTEL_DC_STATE_DEBUG];
        let tc3_power_offsets = [
            INTEL_HSW_PWR_WELL_CTL2,
            INTEL_DC_STATE_EN,
            INTEL_ICL_PWR_WELL_CTL_AUX2,
            INTEL_ICL_PWR_WELL_CTL_DDI2,
        ];
        let main_pw_state = pw_state_mask(INTEL_PW_REQ_IDX_PW1)
            | pw_state_mask(INTEL_PW_REQ_IDX_PW2)
            | pw_state_mask(INTEL_PW_REQ_IDX_PW3)
            | pw_state_mask(INTEL_PW_REQ_IDX_PW4)
            | pw_state_mask(INTEL_PW_REQ_IDX_PW5);
        let tc3_aux_req = pw_req_mask(INTEL_PW_REQ_IDX_TC3);
        let tc3_aux_state = pw_state_mask(INTEL_PW_REQ_IDX_TC3);
        let tc3_ddi_req = pw_req_mask(INTEL_PW_REQ_IDX_TC3);
        let tc3_ddi_state = pw_state_mask(INTEL_PW_REQ_IDX_TC3);
        let tc3_watch_offsets = [
            INTEL_HSW_PWR_WELL_CTL2,
            INTEL_DC_STATE_EN,
            INTEL_ICL_PWR_WELL_CTL_AUX2,
            INTEL_ICL_PWR_WELL_CTL_DDI2,
            INTEL_TCSS_DDI_STATUS_TC3,
            INTEL_TC3_DP_AUX_CH_CTL,
            INTEL_TC3_DDI_BUF_CTL,
            INTEL_PORT_HOTPLUG_STAT,
            INTEL_PIPECONF_A,
            INTEL_TRANS_A_DDI_FUNC_CTL,
            INTEL_PIPE_A_SRC,
        ];
        let tc3_watch_names = [
            "PW_CTL2",
            "DC_STATE_EN",
            "AUX2",
            "DDI2",
            "TCSS_TC3",
            "AUX_CTL_TC3",
            "DDI_BUF_TC3",
            "HP_STAT",
            "PIPECONF_A",
            "TRANS_A",
            "PIPE_A",
        ];
        let mut seam_orig = [0u32; 3];
        let mut seam_test = [0u32; 3];
        let mut tc3_power_orig = [0u32; 4];
        let mut tc3_power_test = [0u32; 4];
        let mut owner_preaux_before = 0u32;
        let mut owner_preaux_after = 0u32;
        let mut tcss_preaux_before = 0u32;
        let mut tcss_preaux_after = 0u32;
        let mut owner_candidate_idx = 0xFFFF_FFFFu32;
        let mut owner_candidate_before = 0u32;
        let mut owner_candidate_after = 0u32;
        let mut owner_candidate_tcss = 0u32;
        let mut hpd_cpu_preaux_before = 0u32;
        let mut hpd_cpu_preaux_after = 0u32;
        let mut sde_preaux_before = 0u32;
        let mut sde_preaux_after = 0u32;
        let tc3_aux_ctl_orig = intel_mmio_read32(info, INTEL_TC3_DP_AUX_CH_CTL);
        let tc3_aux_ctl_test = tc3_aux_ctl_orig & !INTEL_DP_AUX_CH_CTL_TBT_IO;
        let mut watch_before = [0u32; 11];
        let mut watch_after = [0u32; 11];
        for idx in 0..seam_offsets.len() {
            seam_orig[idx] = intel_mmio_read32(info, seam_offsets[idx]);
            seam_test[idx] = seam_orig[idx] | 0x00000001;
        }
        tc3_power_orig[0] = intel_mmio_read32(info, tc3_power_offsets[0]);
        tc3_power_test[0] = tc3_power_orig[0];
        tc3_power_orig[1] = intel_mmio_read32(info, tc3_power_offsets[1]);
        tc3_power_test[1] = tc3_power_orig[1] & !INTEL_DC_STATE_MASK_COMPACT;
        tc3_power_orig[2] = intel_mmio_read32(info, tc3_power_offsets[2]);
        tc3_power_test[2] = tc3_power_orig[2] | tc3_aux_req;
        tc3_power_orig[3] = intel_mmio_read32(info, tc3_power_offsets[3]);
        tc3_power_test[3] = tc3_power_orig[3] | tc3_ddi_req;
        for idx in 0..tc3_watch_offsets.len() {
            watch_before[idx] = intel_mmio_read32(info, tc3_watch_offsets[idx]);
        }

        for idx in 0..seam_offsets.len() {
            let _ = intel_mmio_write32(info, seam_offsets[idx], seam_orig[idx] & !0x00000001);
        }
        apply_bits(&seam_offsets, &seam_test);
        let _ = intel_mmio_write32(info, INTEL_TC3_DP_AUX_CH_CTL, tc3_aux_ctl_test);
        let mut main_pw_stage_mask = 0u32;
        for pw_idx in [
            INTEL_PW_REQ_IDX_PW1,
            INTEL_PW_REQ_IDX_PW2,
            INTEL_PW_REQ_IDX_PW3,
            INTEL_PW_REQ_IDX_PW4,
            INTEL_PW_REQ_IDX_PW5,
        ] {
            tc3_power_test[0] |= pw_req_mask(pw_idx);
            let _ = intel_mmio_write32(info, INTEL_HSW_PWR_WELL_CTL2, tc3_power_test[0]);
            let state_bit = pw_state_mask(pw_idx);
            let mut stage_set = false;
            for _ in 0..4096 {
                let v = intel_mmio_read32(info, INTEL_HSW_PWR_WELL_CTL2);
                if (v & state_bit) != 0 {
                    main_pw_stage_mask |= state_bit;
                    stage_set = true;
                    break;
                }
            }
            if !stage_set {
                break;
            }
        }
        let _ = intel_mmio_write32(info, INTEL_DC_STATE_EN, tc3_power_test[1]);
        owner_preaux_before = intel_mmio_read32(info, INTEL_TC3_DDI_BUF_CTL);
        tcss_preaux_before = intel_mmio_read32(info, INTEL_TCSS_DDI_STATUS_TC3);
        hpd_cpu_preaux_before = intel_mmio_read32(info, INTEL_GEN11_DE_HPD_ISR);
        sde_preaux_before = intel_mmio_read32(info, INTEL_SDEISR);
        let _ = intel_mmio_write32(
            info,
            INTEL_TC3_DDI_BUF_CTL,
            owner_preaux_before | INTEL_DDI_BUF_CTL_TC_PHY_OWNERSHIP,
        );
        owner_preaux_after = intel_mmio_read32(info, INTEL_TC3_DDI_BUF_CTL);
        tcss_preaux_after = intel_mmio_read32(info, INTEL_TCSS_DDI_STATUS_TC3);
        hpd_cpu_preaux_after = intel_mmio_read32(info, INTEL_GEN11_DE_HPD_ISR);
        sde_preaux_after = intel_mmio_read32(info, INTEL_SDEISR);
        let _ = intel_mmio_write32(info, INTEL_TC3_DDI_BUF_CTL, owner_preaux_before);
        let _ = intel_mmio_write32(info, INTEL_ICL_PWR_WELL_CTL_AUX2, tc3_power_test[2]);
        let _ = intel_mmio_write32(info, INTEL_ICL_PWR_WELL_CTL_DDI2, tc3_power_test[3]);

        let mut tc3_poll = [0u32; 11];
        let mut poll_hit = false;
        for _ in 0..4096 {
            tc3_poll[0] = intel_mmio_read32(info, INTEL_HSW_PWR_WELL_CTL2);
            tc3_poll[1] = intel_mmio_read32(info, INTEL_DC_STATE_EN);
            tc3_poll[2] = intel_mmio_read32(info, INTEL_ICL_PWR_WELL_CTL_AUX2);
            tc3_poll[3] = intel_mmio_read32(info, INTEL_ICL_PWR_WELL_CTL_DDI2);
            tc3_poll[4] = intel_mmio_read32(info, INTEL_TCSS_DDI_STATUS_TC3);
            tc3_poll[5] = intel_mmio_read32(info, INTEL_TC3_DP_AUX_CH_CTL);
            tc3_poll[6] = intel_mmio_read32(info, INTEL_TC3_DDI_BUF_CTL);
            tc3_poll[7] = intel_mmio_read32(info, INTEL_PORT_HOTPLUG_STAT);
            tc3_poll[8] = intel_mmio_read32(info, INTEL_PIPECONF_A);
            tc3_poll[9] = intel_mmio_read32(info, INTEL_TRANS_A_DDI_FUNC_CTL);
            tc3_poll[10] = intel_mmio_read32(info, INTEL_PIPE_A_SRC);
            if (tc3_poll[0] & main_pw_state) != 0
                || (tc3_poll[2] & tc3_aux_state) != 0
                || (tc3_poll[3] & tc3_ddi_state) != 0
                || tc3_poll[4] != watch_before[4]
                || tc3_poll[5] != watch_before[5]
                || tc3_poll[6] != watch_before[6]
                || tc3_poll[7] != watch_before[7]
                || tc3_poll[8] != watch_before[8]
                || tc3_poll[9] != watch_before[9]
                || tc3_poll[10] != watch_before[10]
            {
                poll_hit = true;
                break;
            }
        }

        let dp_route_before = read_dp_route_sig();
        let tc3_route_before = intel_mmio_read32(info, INTEL_TC3_DDI_BUF_CTL);
        for idx in 0..tc3_watch_offsets.len() {
            watch_after[idx] = intel_mmio_read32(info, tc3_watch_offsets[idx]);
        }
        let dp_route_after = read_dp_route_sig();
        let tc3_route_after = intel_mmio_read32(info, INTEL_TC3_DDI_BUF_CTL);

        if dp_route_changed(&dp_route_before, &dp_route_after)
            || tc3_route_before != tc3_route_after
        {
            let mut first = "route";
            let mut before = tc3_route_before;
            let mut after = tc3_route_after;
            if dp_route_changed(&dp_route_before, &dp_route_after) {
                first = "dp-route";
                before = dp_route_before[1];
                after = dp_route_after[1];
            }
            crate::log_trace!(
                "gfx-intel-scanout: compact-hit tc3-ladder first={} 0x{:08X}->0x{:08X}\n",
                first,
                before,
                after
            );
            restore_bits(&tc3_power_offsets, &tc3_power_orig);
            let _ = intel_mmio_write32(info, INTEL_TC3_DP_AUX_CH_CTL, tc3_aux_ctl_orig);
            restore_bits(&seam_offsets, &seam_orig);
            return;
        }

        for idx in 0..INTEL_TC_DDI_BUF_CTL_CANDIDATES.len() {
            let ddi_off = INTEL_TC_DDI_BUF_CTL_CANDIDATES[idx];
            let aux_off = INTEL_TC_DP_AUX_CH_CTL_CANDIDATES[idx];
            let tcss_off = INTEL_TCSS_DDI_STATUS_TC3_CANDIDATES[idx];
            let before = intel_mmio_read32(info, ddi_off);
            let aux_before = intel_mmio_read32(info, aux_off);
            let tcss_before_cand = intel_mmio_read32(info, tcss_off);
            let aux_test = aux_before & !INTEL_DP_AUX_CH_CTL_TBT_IO;
            let _ = intel_mmio_write32(info, aux_off, aux_test);
            let test = before | INTEL_DDI_BUF_CTL_TC_PHY_OWNERSHIP;
            let _ = intel_mmio_write32(info, ddi_off, test);
            let after = intel_mmio_read32(info, ddi_off);
            let aux_after = intel_mmio_read32(info, aux_off);
            let tcss_after_cand = intel_mmio_read32(info, tcss_off);
            let _ = intel_mmio_write32(info, ddi_off, before);
            let _ = intel_mmio_write32(info, aux_off, aux_before);
            if after != before || aux_after != aux_before || tcss_after_cand != tcss_before_cand {
                owner_candidate_idx = idx as u32;
                owner_candidate_before = before;
                owner_candidate_after = after;
                owner_candidate_tcss = tcss_after_cand;
                crate::log_trace!(
                    "gfx-intel-scanout: compact-hit tc-own-cand idx={} ddi=0x{:08X}->0x{:08X} aux=0x{:08X}->0x{:08X} tcss=0x{:08X}->0x{:08X}\n",
                    owner_candidate_idx,
                    before,
                    after,
                    aux_before,
                    aux_after,
                    tcss_before_cand,
                    tcss_after_cand
                );
                let _ = intel_mmio_write32(info, INTEL_TC3_DP_AUX_CH_CTL, tc3_aux_ctl_orig);
                restore_bits(&tc3_power_offsets, &tc3_power_orig);
                restore_bits(&seam_offsets, &seam_orig);
                return;
            }
        }

        let mut hits = 0usize;
        let mut first = "";
        let mut line = [("", 0u32, 0u32); 4];
        for idx in 0..tc3_watch_offsets.len() {
            if watch_before[idx] != watch_after[idx] {
                if hits == 0 {
                    first = tc3_watch_names[idx];
                }
                if hits < line.len() {
                    line[hits] = (tc3_watch_names[idx], watch_before[idx], watch_after[idx]);
                }
                hits += 1;
            }
        }

        let state_hit = (watch_after[0] & main_pw_state) != (watch_before[0] & main_pw_state)
            || (watch_after[2] & tc3_aux_state) != (watch_before[2] & tc3_aux_state)
            || (watch_after[3] & tc3_ddi_state) != (watch_before[3] & tc3_ddi_state)
            || poll_hit;

        if hits > tc3_power_offsets.len() || state_hit {
            let core_before = intel_mmio_read32(info, INTEL_HSW_PWR_WELL_CTL5);
            let mut core_after = core_before;
            let mut core_hit_idx = 0xFFFF_FFFFu32;
            let mut core_state = 0u32;
            let mut core_tcss = intel_mmio_read32(info, INTEL_TCSS_DDI_STATUS_TC3);
            let mut core_pa1 = intel_mmio_read32(info, INTEL_FIA2_DFLEXPA1);
            for idx in 0..=INTEL_DISPLAY_CORE_SWEEP_MAX_IDX {
                let req = pw_req_mask(idx);
                let state = pw_state_mask(idx);
                let test = core_before | req;
                let _ = intel_mmio_write32(info, INTEL_HSW_PWR_WELL_CTL5, test);
                for _ in 0..1024 {
                    core_after = intel_mmio_read32(info, INTEL_HSW_PWR_WELL_CTL5);
                    core_tcss = intel_mmio_read32(info, INTEL_TCSS_DDI_STATUS_TC3);
                    core_pa1 = intel_mmio_read32(info, INTEL_FIA2_DFLEXPA1);
                    if (core_after & state) != 0
                        || core_tcss != 0xFFFF_FFFF
                        || core_pa1 != 0xFFFF_FFFF
                    {
                        core_hit_idx = idx;
                        core_state = core_after & state;
                        break;
                    }
                }
                let _ = intel_mmio_write32(info, INTEL_HSW_PWR_WELL_CTL5, core_before);
                if core_hit_idx != 0xFFFF_FFFF {
                    break;
                }
            }

            if core_hit_idx != 0xFFFF_FFFF {
                crate::log_trace!(
                    "gfx-intel-scanout: compact-hit display-core idx={} ctl5=0x{:08X}->0x{:08X} state=0x{:08X} tcss=0x{:08X} pa1=0x{:08X}\n",
                    core_hit_idx,
                    core_before,
                    core_after,
                    core_state,
                    core_tcss,
                    core_pa1
                );
                let _ = intel_mmio_write32(info, INTEL_TC3_DP_AUX_CH_CTL, tc3_aux_ctl_orig);
                restore_bits(&tc3_power_offsets, &tc3_power_orig);
                restore_bits(&seam_offsets, &seam_orig);
                return;
            }

            let gt_disp_pwron_before = intel_mmio_read32(info, INTEL_GT_DISP_PWRON);
            let _ = intel_mmio_write32(
                info,
                INTEL_GT_DISP_PWRON,
                gt_disp_pwron_before | INTEL_GT_DISP_PWRON_REQ,
            );
            let mut gt_disp_pwron_after = gt_disp_pwron_before;
            for _ in 0..4096 {
                gt_disp_pwron_after = intel_mmio_read32(info, INTEL_GT_DISP_PWRON);
                if gt_disp_pwron_after != gt_disp_pwron_before {
                    break;
                }
            }

            let (tc_cold_ok, tc_cold_status, tc_cold_low, tc_cold_high) =
                intel_tgl_tc_cold_block_compact(info);
            let dkl_before = intel_dkl_tc3_read32(
                info,
                INTEL_TC3_DKL_CMN_UC_DW_27_MMIO,
                INTEL_TC3_DKL_BANK_IDX_UC_DW27,
            );
            let tcss_before = intel_mmio_read32(info, INTEL_TCSS_DDI_STATUS_TC3);
            let dflexpa1_before = intel_mmio_read32(info, INTEL_FIA2_DFLEXPA1);
            let dppms_before = intel_mmio_read32(info, INTEL_FIA2_DFLEXDPPMS);
            let dpcsss_before = intel_mmio_read32(info, INTEL_FIA2_DFLEXDPCSSS);
            let dpsp_before = intel_mmio_read32(info, INTEL_FIA2_DFLEXDPSP);
            let dpmle1_before = intel_mmio_read32(info, INTEL_FIA2_DFLEXDPMLE1);
            let owner_before = intel_mmio_read32(info, INTEL_TC3_DDI_BUF_CTL);
            let fia_owner_before = dpcsss_before;

            let mut dkl_after = dkl_before;
            let mut tcss_after = tcss_before;
            let mut dflexpa1_after = dflexpa1_before;
            let mut dppms_after = dppms_before;
            let mut dpcsss_after = dpcsss_before;
            let mut dpsp_after = dpsp_before;
            let mut dpmle1_after = dpmle1_before;
            let mut dkl_health = false;
            let mut dkl_revealed = false;
            for _ in 0..4096 {
                dkl_after = intel_dkl_tc3_read32(
                    info,
                    INTEL_TC3_DKL_CMN_UC_DW_27_MMIO,
                    INTEL_TC3_DKL_BANK_IDX_UC_DW27,
                );
                tcss_after = intel_mmio_read32(info, INTEL_TCSS_DDI_STATUS_TC3);
                dflexpa1_after = intel_mmio_read32(info, INTEL_FIA2_DFLEXPA1);
                dppms_after = intel_mmio_read32(info, INTEL_FIA2_DFLEXDPPMS);
                dpcsss_after = intel_mmio_read32(info, INTEL_FIA2_DFLEXDPCSSS);
                dpsp_after = intel_mmio_read32(info, INTEL_FIA2_DFLEXDPSP);
                dpmle1_after = intel_mmio_read32(info, INTEL_FIA2_DFLEXDPMLE1);
                dkl_revealed = dkl_before == 0xFFFF_FFFF && dkl_after != 0xFFFF_FFFF;
                dkl_health =
                    dkl_after != 0xFFFF_FFFF && (dkl_after & INTEL_DKL_CMN_UC_DW27_UC_HEALTH) != 0;
                if dkl_revealed
                    || dkl_health
                    || tcss_after != tcss_before
                    || dflexpa1_after != dflexpa1_before
                    || dppms_after != dppms_before
                    || dpcsss_after != dpcsss_before
                    || dpsp_after != dpsp_before
                    || dpmle1_after != dpmle1_before
                {
                    break;
                }
            }

            let fia_visible = dflexpa1_after != 0xFFFF_FFFF
                || dppms_after != 0xFFFF_FFFF
                || dpcsss_after != 0xFFFF_FFFF
                || dpsp_after != 0xFFFF_FFFF
                || dpmle1_after != 0xFFFF_FFFF;
            let tcss_visible = tcss_after != 0xFFFF_FFFF;

            if dkl_revealed || dkl_health || fia_visible || tcss_visible {
                let fia_owner_test = fia_owner_before | INTEL_FIA_TC3_OWNED;
                let _ = intel_mmio_write32(info, INTEL_FIA2_DFLEXDPCSSS, fia_owner_test);
                let fia_owner_after = intel_mmio_read32(info, INTEL_FIA2_DFLEXDPCSSS);
                let owner_test = owner_before | INTEL_DDI_BUF_CTL_TC_PHY_OWNERSHIP;
                let _ = intel_mmio_write32(info, INTEL_TC3_DDI_BUF_CTL, owner_test);
                let owner_after = intel_mmio_read32(info, INTEL_TC3_DDI_BUF_CTL);
                tcss_after = intel_mmio_read32(info, INTEL_TCSS_DDI_STATUS_TC3);
                dflexpa1_after = intel_mmio_read32(info, INTEL_FIA2_DFLEXPA1);
                dppms_after = intel_mmio_read32(info, INTEL_FIA2_DFLEXDPPMS);
                dpcsss_after = intel_mmio_read32(info, INTEL_FIA2_DFLEXDPCSSS);
                dpsp_after = intel_mmio_read32(info, INTEL_FIA2_DFLEXDPSP);
                dpmle1_after = intel_mmio_read32(info, INTEL_FIA2_DFLEXDPMLE1);

                crate::log_trace!(
                    "gfx-intel-scanout: compact-hit tc3-connect pcode=0x{:08X} low=0x{:08X} gtpwr=0x{:08X}->0x{:08X} dkl=0x{:08X}->0x{:08X} fiaown=0x{:08X}->0x{:08X} owner=0x{:08X}->0x{:08X} tcss=0x{:08X} pa1=0x{:08X} dppms=0x{:08X} dpcsss=0x{:08X} dpsp=0x{:08X} dpmle1=0x{:08X}\n",
                    tc_cold_status,
                    tc_cold_low,
                    gt_disp_pwron_before,
                    gt_disp_pwron_after,
                    dkl_before,
                    dkl_after,
                    fia_owner_before,
                    fia_owner_after,
                    owner_before,
                    owner_after,
                    tcss_after,
                    dflexpa1_after,
                    dppms_after,
                    dpcsss_after,
                    dpsp_after,
                    dpmle1_after
                );
                let _ = intel_mmio_write32(info, INTEL_FIA2_DFLEXDPCSSS, fia_owner_before);
                let _ = intel_mmio_write32(info, INTEL_TC3_DDI_BUF_CTL, owner_before);
            } else {
                let mut tcss_candidates = [0u32; 4];
                for idx in 0..INTEL_TCSS_DDI_STATUS_TC3_CANDIDATES.len() {
                    tcss_candidates[idx] =
                        intel_mmio_read32(info, INTEL_TCSS_DDI_STATUS_TC3_CANDIDATES[idx]);
                }
                let mut dkl_shift_candidates = [0u32; 4];
                for idx in 0..INTEL_TC3_DKL_BANK_SHIFT_CANDIDATES.len() {
                    dkl_shift_candidates[idx] = intel_dkl_read32_shifted(
                        info,
                        INTEL_TC3_DKL_CMN_UC_DW_27_MMIO,
                        INTEL_TC3_DKL_BANK_IDX_UC_DW27,
                        INTEL_TC3_DKL_BANK_SHIFT_CANDIDATES[idx],
                    );
                }
                let sideband_baseline = intel_mmio_read32(info, INTEL_HIP_INDEX_REG0);
                let mut sideband_hit = false;
                let mut sideband_first_shift = 0u32;
                let mut sideband_first_bank = 0u32;
                let mut sideband_first_dkl = 0u32;
                let mut sideband_first_fia = 0u32;
                for shift in INTEL_TC3_DKL_BANK_SHIFT_CANDIDATES {
                    for bank_idx in 0..=3u32 {
                        let bank_mask = 0xFFu32 << shift;
                        let hip_test = (sideband_baseline & !bank_mask) | (bank_idx << shift);
                        let _ = intel_mmio_write32(info, INTEL_HIP_INDEX_REG0, hip_test);
                        let dkl_probe = intel_mmio_read32(info, INTEL_TC3_DKL_CMN_UC_DW_27_MMIO);
                        let fia_probe = intel_mmio_read32(info, INTEL_FIA2_DFLEXDPPMS);
                        if dkl_probe != 0xFFFF_FFFF || fia_probe != 0xFFFF_FFFF {
                            sideband_hit = true;
                            sideband_first_shift = shift;
                            sideband_first_bank = bank_idx;
                            sideband_first_dkl = dkl_probe;
                            sideband_first_fia = fia_probe;
                            break;
                        }
                    }
                    if sideband_hit {
                        break;
                    }
                }
                let _ = intel_mmio_write32(info, INTEL_HIP_INDEX_REG0, sideband_baseline);

                let topo_tcss = [
                    intel_mmio_read32(info, 0x161500),
                    intel_mmio_read32(info, 0x161504),
                ];
                let topo_fia1 = [
                    intel_mmio_read32(info, INTEL_FIA1_DFLEXPA1),
                    intel_mmio_read32(info, INTEL_FIA1_DFLEXDPPMS),
                    intel_mmio_read32(info, INTEL_FIA1_DFLEXDPCSSS),
                    intel_mmio_read32(info, INTEL_FIA1_DFLEXDPSP),
                    intel_mmio_read32(info, INTEL_FIA1_DFLEXDPMLE1),
                ];
                let topo_fia2 = [
                    intel_mmio_read32(info, INTEL_FIA2_DFLEXPA1),
                    intel_mmio_read32(info, INTEL_FIA2_DFLEXDPPMS),
                    intel_mmio_read32(info, INTEL_FIA2_DFLEXDPCSSS),
                    intel_mmio_read32(info, INTEL_FIA2_DFLEXDPSP),
                    intel_mmio_read32(info, INTEL_FIA2_DFLEXDPMLE1),
                ];
                let topo_hit = topo_tcss.iter().any(|&v| v != 0xFFFF_FFFF)
                    || topo_fia1.iter().any(|&v| v != 0xFFFF_FFFF)
                    || topo_fia2.iter().any(|&v| v != 0xFFFF_FFFF);
                let topo_kind = if topo_fia2.iter().any(|&v| v != 0xFFFF_FFFF) {
                    "fia2"
                } else if topo_fia1.iter().any(|&v| v != 0xFFFF_FFFF) {
                    "fia1"
                } else if topo_tcss.iter().any(|&v| v != 0xFFFF_FFFF) {
                    "tcss"
                } else {
                    "none"
                };
                crate::log_trace!(
                    "gfx-intel-scanout: compact-branch sideband={} shift={} bank={} dkl=0x{:08X} fia=0x{:08X} topo={} kind={} tcss=[0x{:08X},0x{:08X}] fia1=[0x{:08X},0x{:08X}] fia2=[0x{:08X},0x{:08X}]\n",
                    if sideband_hit { "hit" } else { "nope" },
                    sideband_first_shift,
                    sideband_first_bank,
                    sideband_first_dkl,
                    sideband_first_fia,
                    if topo_hit { "hit" } else { "nope" },
                    topo_kind,
                    topo_tcss[0],
                    topo_tcss[1],
                    topo_fia1[0],
                    topo_fia1[1],
                    topo_fia2[0],
                    topo_fia2[1]
                );
                let sinkless_tc_hint = hpd_cpu_preaux_after == 0
                    && sde_preaux_after == 0
                    && tcss_after == 0xFFFF_FFFF
                    && dpsp_after == 0xFFFF_FFFF;
                crate::log_trace!(
                    "gfx-intel-scanout: compact-nope tc3-dkl sealed modehint={} pcode={} status=0x{:08X} low=0x{:08X} high=0x{:08X} preown=0x{:08X}->0x{:08X} pretcss=0x{:08X}->0x{:08X} owncand={} cand=0x{:08X}->0x{:08X} candtcss=0x{:08X} prehpd=0x{:08X}->0x{:08X} presde=0x{:08X}->0x{:08X} gtpwr=0x{:08X}->0x{:08X} main=0x{:03X} aux=0x{:08X} ddi=0x{:08X} dkl=0x{:08X} tcss=0x{:08X} tcsswin=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] dklshift=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] pa1=0x{:08X} dppms=0x{:08X} dpcsss=0x{:08X} dpsp=0x{:08X} dpmle1=0x{:08X}\n",
                    if sinkless_tc_hint {
                        "sinkless-tbt-default"
                    } else {
                        "dp-alt-unknown"
                    },
                    if tc_cold_ok { "ok" } else { "fail" },
                    tc_cold_status,
                    tc_cold_low,
                    tc_cold_high,
                    owner_preaux_before,
                    owner_preaux_after,
                    tcss_preaux_before,
                    tcss_preaux_after,
                    if owner_candidate_idx == 0xFFFF_FFFF {
                        -1i32
                    } else {
                        owner_candidate_idx as i32
                    },
                    owner_candidate_before,
                    owner_candidate_after,
                    owner_candidate_tcss,
                    hpd_cpu_preaux_before,
                    hpd_cpu_preaux_after,
                    sde_preaux_before,
                    sde_preaux_after,
                    gt_disp_pwron_before,
                    gt_disp_pwron_after,
                    main_pw_stage_mask,
                    watch_after[2] & tc3_aux_state,
                    watch_after[3] & tc3_ddi_state,
                    dkl_after,
                    tcss_after,
                    tcss_candidates[0],
                    tcss_candidates[1],
                    tcss_candidates[2],
                    tcss_candidates[3],
                    dkl_shift_candidates[0],
                    dkl_shift_candidates[1],
                    dkl_shift_candidates[2],
                    dkl_shift_candidates[3],
                    dflexpa1_after,
                    dppms_after,
                    dpcsss_after,
                    dpsp_after,
                    dpmle1_after
                );
            }

            let _ = intel_mmio_write32(info, INTEL_GT_DISP_PWRON, gt_disp_pwron_before);
        } else {
            crate::log_trace!(
                "gfx-intel-scanout: compact-nope tc3 ladder stayed sealed main=0x{:03X} pwctl=0x{:08X} dc=0x{:08X} aux=0x{:08X} ddi=0x{:08X} tcss=0x{:08X} auxctl=0x{:08X} buf=0x{:08X}\n",
                main_pw_stage_mask,
                tc3_poll[0],
                tc3_poll[1],
                tc3_poll[2] & tc3_aux_state,
                tc3_poll[3] & tc3_ddi_state,
                tc3_poll[4],
                tc3_poll[5],
                tc3_poll[6]
            );
        }
        restore_bits(&tc3_power_offsets, &tc3_power_orig);
        let _ = intel_mmio_write32(info, INTEL_TC3_DP_AUX_CH_CTL, tc3_aux_ctl_orig);
        restore_bits(&seam_offsets, &seam_orig);
        return;
    }

    let ownership_watch_offsets = [
        0x454D0usize,
        0x454D4usize,
        0x454D8usize,
        0x454DCusize,
        0x45500usize,
        INTEL_DC_STATE_EN,
        0x45510usize,
        INTEL_DC_STATE_DEBUG,
        0x45524usize,
        0x45528usize,
        0x4552Cusize,
        0x45530usize,
        0x45534usize,
        0x45538usize,
        0x4553Cusize,
        0x13807Cusize,
        0x138088usize,
        0x13808Cusize,
        0x138090usize,
        0x138094usize,
        INTEL_PORT_HOTPLUG_EN,
        INTEL_PORT_HOTPLUG_STAT,
        INTEL_DDI_BUF_CTL_0,
        INTEL_DDI_BUF_CTL_1,
        INTEL_DDI_BUF_CTL_2,
        INTEL_PIPECONF_A,
        INTEL_PIPECONF_B,
        INTEL_PIPECONF_C,
        INTEL_PIPECONF_D,
        INTEL_TRANS_A_DDI_FUNC_CTL,
        INTEL_TRANS_B_DDI_FUNC_CTL,
        INTEL_TRANS_C_DDI_FUNC_CTL,
        INTEL_TRANS_D_DDI_FUNC_CTL,
    ];
    let ownership_watch_names = [
        "454D0", "454D4", "454D8", "454DC", "45500", "45504", "45510", "45520", "45524", "45528",
        "4552C", "45530", "45534", "45538", "4553C", "13807C", "138088", "13808C", "138090",
        "138094", "HP_EN", "HP_STAT", "DDI0", "DDI1", "DDI2", "PC_A", "PC_B", "PC_C", "PC_D",
        "TRANS_A", "TRANS_B", "TRANS_C", "TRANS_D",
    ];
    let read_watch = |values: &mut [u32; 33]| {
        for (idx, slot) in values.iter_mut().enumerate() {
            *slot = intel_mmio_read32(info, ownership_watch_offsets[idx]);
        }
    };
    let log_watch_delta = |label: &str, before: &[u32; 33], after: &[u32; 33]| {
        let mut hits = 0usize;
        let mut first = "";
        let mut line = [("", 0u32, 0u32); 4];
        for idx in 0..ownership_watch_offsets.len() {
            if before[idx] != after[idx] {
                if hits == 0 {
                    first = ownership_watch_names[idx];
                }
                if hits < line.len() {
                    line[hits] = (ownership_watch_names[idx], before[idx], after[idx]);
                }
                hits += 1;
            }
        }
        crate::log_trace!(
            "gfx-intel-scanout: minimal-pattern ownership-delta label={} hits={} first={} hit0={} 0x{:08X}->0x{:08X} hit1={} 0x{:08X}->0x{:08X} hit2={} 0x{:08X}->0x{:08X} hit3={} 0x{:08X}->0x{:08X}\n",
            label,
            hits,
            first,
            line[0].0,
            line[0].1,
            line[0].2,
            line[1].0,
            line[1].1,
            line[1].2,
            line[2].0,
            line[2].1,
            line[2].2,
            line[3].0,
            line[3].1,
            line[3].2
        );
    };
    let poll_watch_change = |label: &str, baseline: &[u32; 33], spins: usize| {
        let mut current = [0u32; 33];
        let mut changed = false;
        let mut spin_hit = 0usize;
        for spin in 0..spins {
            read_watch(&mut current);
            if current != *baseline {
                changed = true;
                spin_hit = spin + 1;
                break;
            }
            core::hint::spin_loop();
        }
        crate::log_trace!(
            "gfx-intel-scanout: minimal-pattern ownership-poll label={} changed={} spin_hit={}\n",
            label,
            changed as u32,
            spin_hit
        );
        if changed {
            log_watch_delta(label, baseline, &current);
        }
        current
    };

    let preflight_offsets = [
        0x45500usize,
        INTEL_DC_STATE_EN,
        0x45510usize,
        INTEL_DC_STATE_DEBUG,
    ];
    let preflight_names = ["45500", "45504", "45510", "45520"];
    let mut preflight_orig = [0u32; 4];
    let mut preflight_test = [0u32; 4];
    for idx in 0..preflight_offsets.len() {
        preflight_orig[idx] = intel_mmio_read32(info, preflight_offsets[idx]);
        preflight_test[idx] = preflight_orig[idx] | 0x00000001;
    }
    for idx in 0..preflight_offsets.len() {
        let _ = intel_mmio_write32(
            info,
            preflight_offsets[idx],
            preflight_orig[idx] & !0x00000001,
        );
    }
    let mut ownership_before = [0u32; 33];
    read_watch(&mut ownership_before);
    let preflight_low = [
        intel_mmio_read32(info, preflight_offsets[0]),
        intel_mmio_read32(info, preflight_offsets[1]),
        intel_mmio_read32(info, preflight_offsets[2]),
        intel_mmio_read32(info, preflight_offsets[3]),
    ];
    crate::log_trace!(
        "gfx-intel-scanout: minimal-pattern ownership-preflight low rb=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}]\n",
        preflight_low[0],
        preflight_low[1],
        preflight_low[2],
        preflight_low[3]
    );
    let mut ownership_after = [0u32; 33];
    read_watch(&mut ownership_after);
    log_watch_delta("preflight-low", &ownership_before, &ownership_after);
    ownership_before = ownership_after;
    for idx in 0..preflight_offsets.len() {
        let _ = intel_mmio_write32(info, preflight_offsets[idx], preflight_test[idx]);
        let rb = intel_mmio_read32(info, preflight_offsets[idx]);
        let hotplug_stat = intel_mmio_read32(info, INTEL_PORT_HOTPLUG_STAT);
        let ddi0 = intel_mmio_read32(info, INTEL_DDI_BUF_CTL_0);
        let ddi1 = intel_mmio_read32(info, INTEL_DDI_BUF_CTL_1);
        let ddi2 = intel_mmio_read32(info, INTEL_DDI_BUF_CTL_2);
        let pc_a = intel_mmio_read32(info, INTEL_PIPECONF_A);
        let pc_b = intel_mmio_read32(info, INTEL_PIPECONF_B);
        let pc_c = intel_mmio_read32(info, INTEL_PIPECONF_C);
        let pc_d = intel_mmio_read32(info, INTEL_PIPECONF_D);
        crate::log_trace!(
            "gfx-intel-scanout: minimal-pattern ownership-preflight step={} rb=0x{:08X} hp=0x{:08X} ddi=[0x{:08X},0x{:08X},0x{:08X}] pipeconf=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}]\n",
            preflight_names[idx],
            rb,
            hotplug_stat,
            ddi0,
            ddi1,
            ddi2,
            pc_a,
            pc_b,
            pc_c,
            pc_d
        );
        read_watch(&mut ownership_after);
        log_watch_delta(preflight_names[idx], &ownership_before, &ownership_after);
        ownership_before = ownership_after;
    }
    crate::log_trace!("gfx-intel-scanout: minimal-pattern ownership-preflight complete\n");
    let ownership_polled = poll_watch_change("preflight-post", &ownership_before, 2048);
    ownership_before = ownership_polled;
    for idx in (0..preflight_offsets.len()).rev() {
        let _ = intel_mmio_write32(info, preflight_offsets[idx], preflight_orig[idx]);
    }

    // Prepare demo surface with test pattern only after the ownership preflight.
    let Some((_surf, stride, width, height)) = prepare_direct_demo_surface(info) else {
        return;
    };

    crate::log_trace!(
        "gfx-intel-scanout: minimal-pattern dc-pll probe starting stride=0x{:X} {}x{}\n",
        stride,
        width,
        height
    );

    macro_rules! log_control_state {
        ($label:expr) => {{
            let de_pll_enable = intel_mmio_read32(info, INTEL_BXT_DE_PLL_ENABLE);
            let phy_misc_a = intel_mmio_read32(info, INTEL_ICL_PHY_MISC_A);
            let hotplug = intel_mmio_read32(info, INTEL_PORT_HOTPLUG_EN);
            let hotplug_stat = intel_mmio_read32(info, INTEL_PORT_HOTPLUG_STAT);
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
            let ctl_a = intel_mmio_read32(info, plane_a.ctl_off);
            let ctl_b = intel_mmio_read32(info, plane_b.ctl_off);
            let ctl_c = intel_mmio_read32(info, plane_c.ctl_off);
            let ctl_d = intel_mmio_read32(info, plane_d.ctl_off);
            let surf_a = intel_mmio_read32(info, plane_a.surf_off);
            let surf_b = intel_mmio_read32(info, plane_b.surf_off);
            let surf_c = intel_mmio_read32(info, plane_c.surf_off);
            let surf_d = intel_mmio_read32(info, plane_d.surf_off);
            crate::log_trace!(
                "gfx-intel-scanout: minimal-pattern {} de_pll_enable=0x{:08X} phy_misc_a=0x{:08X} hotplug=0x{:08X} hotplug_stat=0x{:08X} trans=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] pipe=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] ctl=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] surf=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}]\n",
                $label,
                de_pll_enable,
                phy_misc_a,
                hotplug,
                hotplug_stat,
                trans_a,
                trans_b,
                trans_c,
                trans_d,
                pipe_a,
                pipe_b,
                pipe_c,
                pipe_d,
                ctl_a,
                ctl_b,
                ctl_c,
                ctl_d,
                surf_a,
                surf_b,
                surf_c,
                surf_d
            );
        }};
    }

    macro_rules! log_route_state {
        ($label:expr) => {{
            let hotplug_stat = intel_mmio_read32(info, INTEL_PORT_HOTPLUG_STAT);
            let ddi0 = intel_mmio_read32(info, INTEL_DDI_BUF_CTL_0);
            let ddi1 = intel_mmio_read32(info, INTEL_DDI_BUF_CTL_1);
            let ddi2 = intel_mmio_read32(info, INTEL_DDI_BUF_CTL_2);
            let pc_a = intel_mmio_read32(info, INTEL_PIPECONF_A);
            let pc_b = intel_mmio_read32(info, INTEL_PIPECONF_B);
            let pc_c = intel_mmio_read32(info, INTEL_PIPECONF_C);
            let pc_d = intel_mmio_read32(info, INTEL_PIPECONF_D);
            crate::log_trace!(
                "gfx-intel-scanout: minimal-pattern route {} hp_stat=0x{:08X} ddi=[0x{:08X},0x{:08X},0x{:08X}] pipeconf=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}]\n",
                $label,
                hotplug_stat,
                ddi0,
                ddi1,
                ddi2,
                pc_a,
                pc_b,
                pc_c,
                pc_d
            );
        }};
    }

    macro_rules! log_power_gate_state {
        ($label:expr) => {{
            let dc_state_en = intel_mmio_read32(info, INTEL_DC_STATE_EN);
            let dc_state_debug = intel_mmio_read32(info, INTEL_DC_STATE_DEBUG);
            let gt_disp_pwron = intel_mmio_read32(info, INTEL_GT_DISP_PWRON);
            let hotplug = intel_mmio_read32(info, INTEL_PORT_HOTPLUG_EN);
            let hotplug_stat = intel_mmio_read32(info, INTEL_PORT_HOTPLUG_STAT);
            crate::log_trace!(
                "gfx-intel-scanout: minimal-pattern gate {} dc_state_en=0x{:08X} dc_state_debug=0x{:08X} gt_disp_pwron=0x{:08X} hotplug=0x{:08X} hotplug_stat=0x{:08X}\n",
                $label,
                dc_state_en,
                dc_state_debug,
                gt_disp_pwron,
                hotplug,
                hotplug_stat
            );
        }};
    }

    log_control_state!("init");

    for &off in &[
        0x45014usize,
        0x45010usize,
        0x45020usize,
        0x46000usize,
        0x46070usize,
    ] {
        let orig = intel_mmio_read32(info, off);
        let test = if off == 0x46070 {
            orig | 0x00000001
        } else {
            0x00000001
        };
        let _ = intel_mmio_write32(info, off, test);
        let rb = intel_mmio_read32(info, off);
        let _ = intel_mmio_write32(info, off, orig);
        crate::log_trace!(
            "gfx-intel-scanout: minimal-pattern dc-pll off=0x{:05X} orig=0x{:08X} test=0x{:08X} rb=0x{:08X}\n",
            off,
            orig,
            test,
            rb
        );
    }

    crate::log_trace!("gfx-intel-scanout: minimal-pattern dc-pll probe complete\n");

    let held_off = INTEL_BXT_DE_PLL_ENABLE;
    let held_orig = intel_mmio_read32(info, held_off);
    let held_test = held_orig | 0x00000001;
    let _ = intel_mmio_write32(info, held_off, held_test);
    let held_rb = intel_mmio_read32(info, held_off);
    crate::log_trace!(
        "gfx-intel-scanout: minimal-pattern dc-pll held orig=0x{:08X} test=0x{:08X} rb=0x{:08X}\n",
        held_orig,
        held_test,
        held_rb
    );
    log_control_state!("held-init");

    for &off in &[
        0x45010usize,
        0x45014usize,
        0x45018usize,
        0x4501Cusize,
        0x45020usize,
        0x45024usize,
        0x46060usize,
        0x46064usize,
        0x46068usize,
        0x4606Cusize,
        0x46074usize,
        0x46078usize,
        0x4607Cusize,
    ] {
        let orig = intel_mmio_read32(info, off);
        let Some(test) = walker_test_value(orig) else {
            crate::log_trace!(
                "gfx-intel-scanout: minimal-pattern held-probe off=0x{:05X} orig=0x{:08X} skipped\n",
                off,
                orig
            );
            continue;
        };
        let _ = intel_mmio_write32(info, off, test);
        let rb = intel_mmio_read32(info, off);
        crate::log_trace!(
            "gfx-intel-scanout: minimal-pattern held-probe off=0x{:05X} orig=0x{:08X} test=0x{:08X} rb=0x{:08X}\n",
            off,
            orig,
            test,
            rb
        );
        let _ = intel_mmio_write32(info, off, orig);
    }

    let bundle_offsets = [
        0x45014usize,
        0x45020usize,
        0x45024usize,
        0x46000usize,
        INTEL_BXT_DE_PLL_ENABLE,
    ];
    let mut bundle_orig = [0u32; 5];
    for (idx, &off) in bundle_offsets.iter().enumerate() {
        bundle_orig[idx] = intel_mmio_read32(info, off);
    }

    let apply_bundle = |label: &str, reverse: bool| {
        let order: [usize; 5] = if reverse {
            [4, 3, 2, 1, 0]
        } else {
            [0, 1, 2, 3, 4]
        };
        let mut bundle_rb = [0u32; 5];
        for &idx in &order {
            let off = bundle_offsets[idx];
            let orig = bundle_orig[idx];
            let test = if off == INTEL_BXT_DE_PLL_ENABLE {
                orig | 0x00000001
            } else {
                orig | 0x00000001
            };
            let _ = intel_mmio_write32(info, off, test);
            bundle_rb[idx] = intel_mmio_read32(info, off);
        }
        crate::log_trace!(
            "gfx-intel-scanout: minimal-pattern bundle label={} rb=[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X}]\n",
            label,
            bundle_rb[0],
            bundle_rb[1],
            bundle_rb[2],
            bundle_rb[3],
            bundle_rb[4]
        );
        log_control_state!(label);
        for &idx in order.iter().rev() {
            let _ = intel_mmio_write32(info, bundle_offsets[idx], bundle_orig[idx]);
        }
    };

    apply_bundle("bundle-fwd", false);
    apply_bundle("bundle-rev", true);

    for (idx, &off) in bundle_offsets.iter().enumerate() {
        let test = bundle_orig[idx] | 0x00000001;
        let _ = intel_mmio_write32(info, off, test);
    }
    crate::log_trace!("gfx-intel-scanout: minimal-pattern bridge bundle asserted\n");
    log_control_state!("bridge-base");
    log_route_state!("bridge-base");

    let bridge_probes = [
        ("6c104", 0x6C104usize, None),
        ("6c108", 0x6C108usize, Some(0x00200000u32)),
        ("6c114", 0x6C114usize, None),
        ("6c120", 0x6C120usize, None),
        ("138088", 0x138088usize, None),
        ("hotplug", INTEL_PORT_HOTPLUG_EN, None),
    ];
    for &(label, off, forced_test) in &bridge_probes {
        let orig = intel_mmio_read32(info, off);
        let test = if let Some(test) = forced_test {
            test
        } else if let Some(test) = walker_test_value(orig) {
            test
        } else {
            crate::log_trace!(
                "gfx-intel-scanout: minimal-pattern bridge-probe label={} off=0x{:05X} orig=0x{:08X} skipped\n",
                label,
                off,
                orig
            );
            continue;
        };
        let _ = intel_mmio_write32(info, off, test);
        let rb = intel_mmio_read32(info, off);
        crate::log_trace!(
            "gfx-intel-scanout: minimal-pattern bridge-probe label={} off=0x{:05X} orig=0x{:08X} test=0x{:08X} rb=0x{:08X}\n",
            label,
            off,
            orig,
            test,
            rb
        );
        let _ = intel_mmio_write32(info, off, orig);
    }
    crate::log_trace!("gfx-intel-scanout: minimal-pattern bridge probe complete\n");

    for &(label, off) in &[
        ("ddi0", INTEL_DDI_BUF_CTL_0),
        ("ddi1", INTEL_DDI_BUF_CTL_1),
        ("ddi2", INTEL_DDI_BUF_CTL_2),
        ("pipeconf-a", INTEL_PIPECONF_A),
        ("pipeconf-b", INTEL_PIPECONF_B),
        ("pipeconf-c", INTEL_PIPECONF_C),
        ("pipeconf-d", INTEL_PIPECONF_D),
    ] {
        let orig = intel_mmio_read32(info, off);
        let Some(test) = walker_test_value(orig) else {
            crate::log_trace!(
                "gfx-intel-scanout: minimal-pattern route-probe label={} off=0x{:05X} orig=0x{:08X} skipped\n",
                label,
                off,
                orig
            );
            continue;
        };
        let _ = intel_mmio_write32(info, off, test);
        let rb = intel_mmio_read32(info, off);
        crate::log_trace!(
            "gfx-intel-scanout: minimal-pattern route-probe label={} off=0x{:05X} orig=0x{:08X} test=0x{:08X} rb=0x{:08X}\n",
            label,
            off,
            orig,
            test,
            rb
        );
        let _ = intel_mmio_write32(info, off, orig);
    }
    crate::log_trace!("gfx-intel-scanout: minimal-pattern route probe complete\n");

    let power_hold_offsets = [
        0x45500usize,
        INTEL_DC_STATE_EN,
        0x45510usize,
        INTEL_DC_STATE_DEBUG,
        0x138088usize,
        INTEL_GT_DISP_PWRON,
    ];
    let power_hold_tests = [
        0x00000001u32,
        0x00000001u32,
        0x00000001u32,
        0x00000001u32,
        0x00000001u32,
        INTEL_GT_DISP_PWRON_REQ,
    ];
    let mut power_hold_orig = [0u32; 6];
    let mut power_hold_rb = [0u32; 6];
    for idx in 0..power_hold_offsets.len() {
        power_hold_orig[idx] = intel_mmio_read32(info, power_hold_offsets[idx]);
        let _ = intel_mmio_write32(info, power_hold_offsets[idx], power_hold_tests[idx]);
        power_hold_rb[idx] = intel_mmio_read32(info, power_hold_offsets[idx]);
    }
    crate::log_trace!(
        "gfx-intel-scanout: minimal-pattern power-hold rb=[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X}]\n",
        power_hold_rb[0],
        power_hold_rb[1],
        power_hold_rb[2],
        power_hold_rb[3],
        power_hold_rb[4],
        power_hold_rb[5]
    );
    log_power_gate_state!("power-base");
    log_route_state!("power-base");
    log_control_state!("power-base");

    for &(label, off, forced_test) in &[
        ("dc-seam-454F0", 0x454F0usize, Some(0x00000001u32)),
        ("dc-seam-454F4", 0x454F4usize, Some(0x00000001u32)),
        ("dc-seam-454F8", 0x454F8usize, Some(0x00000001u32)),
        ("dc-seam-454FC", 0x454FCusize, Some(0x00000001u32)),
        ("dc-seam-45508", 0x45508usize, Some(0x00000001u32)),
        ("dc-seam-4550C", 0x4550Cusize, Some(0x00000001u32)),
        ("dc-seam-45510", 0x45510usize, Some(0x00000001u32)),
        ("dc-seam-45514", 0x45514usize, Some(0x00000001u32)),
        ("dc-seam-45518", 0x45518usize, Some(0x00000001u32)),
        ("dc-seam-4551C", 0x4551Cusize, Some(0x00000001u32)),
        ("gt-disp-near-13808C", 0x13808Cusize, Some(0x00000001u32)),
        ("gt-disp-near-138094", 0x138094usize, Some(0x00000001u32)),
        (
            "hotplug-en",
            INTEL_PORT_HOTPLUG_EN,
            Some(INTEL_PORT_HOTPLUG_TEST_BIT),
        ),
    ] {
        let orig = intel_mmio_read32(info, off);
        let test = if let Some(test) = forced_test {
            test
        } else if let Some(test) = walker_test_value(orig) {
            test
        } else {
            crate::log_trace!(
                "gfx-intel-scanout: minimal-pattern power-probe label={} off=0x{:05X} orig=0x{:08X} skipped\n",
                label,
                off,
                orig
            );
            continue;
        };
        let _ = intel_mmio_write32(info, off, test);
        let rb = intel_mmio_read32(info, off);
        crate::log_trace!(
            "gfx-intel-scanout: minimal-pattern power-probe label={} off=0x{:05X} orig=0x{:08X} test=0x{:08X} rb=0x{:08X}\n",
            label,
            off,
            orig,
            test,
            rb
        );
        let _ = intel_mmio_write32(info, off, orig);
    }
    crate::log_trace!("gfx-intel-scanout: minimal-pattern power probe complete\n");

    let dc_tuple_offsets = [
        0x45500usize,
        INTEL_DC_STATE_EN,
        0x45510usize,
        INTEL_DC_STATE_DEBUG,
    ];
    let dc_tuple_names = ["45500", "45504", "45510", "45520"];
    let dc_tuple_orders = [
        [0usize, 1usize, 2usize, 3usize],
        [0usize, 1usize, 3usize, 2usize],
        [1usize, 0usize, 2usize, 3usize],
        [1usize, 0usize, 3usize, 2usize],
        [2usize, 0usize, 1usize, 3usize],
        [3usize, 0usize, 1usize, 2usize],
    ];
    let mut dc_tuple_orig = [0u32; 4];
    let mut dc_tuple_test = [0u32; 4];
    for idx in 0..dc_tuple_offsets.len() {
        dc_tuple_orig[idx] = intel_mmio_read32(info, dc_tuple_offsets[idx]);
        dc_tuple_test[idx] = dc_tuple_orig[idx] | 0x00000001;
    }

    for order in dc_tuple_orders {
        let label = [
            dc_tuple_names[order[0]],
            dc_tuple_names[order[1]],
            dc_tuple_names[order[2]],
            dc_tuple_names[order[3]],
        ];

        let mut hold_rb = [0u32; 4];
        for &idx in &order {
            let _ = intel_mmio_write32(info, dc_tuple_offsets[idx], dc_tuple_test[idx]);
            hold_rb[idx] = intel_mmio_read32(info, dc_tuple_offsets[idx]);
        }
        crate::log_trace!(
            "gfx-intel-scanout: minimal-pattern dc-tuple hold order={}>{}>{}>{} rb=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}]\n",
            label[0],
            label[1],
            label[2],
            label[3],
            hold_rb[0],
            hold_rb[1],
            hold_rb[2],
            hold_rb[3]
        );

        let commit_idx = order[3];
        let _ = intel_mmio_write32(
            info,
            dc_tuple_offsets[commit_idx],
            dc_tuple_orig[commit_idx],
        );
        let pulse_low = intel_mmio_read32(info, dc_tuple_offsets[commit_idx]);
        let _ = intel_mmio_write32(
            info,
            dc_tuple_offsets[commit_idx],
            dc_tuple_test[commit_idx],
        );
        let pulse_high = intel_mmio_read32(info, dc_tuple_offsets[commit_idx]);
        crate::log_trace!(
            "gfx-intel-scanout: minimal-pattern dc-tuple pulse order={}>{}>{}>{} commit={} low=0x{:08X} high=0x{:08X}\n",
            label[0],
            label[1],
            label[2],
            label[3],
            dc_tuple_names[commit_idx],
            pulse_low,
            pulse_high
        );

        for &idx in order.iter().rev() {
            let _ = intel_mmio_write32(info, dc_tuple_offsets[idx], dc_tuple_orig[idx]);
        }
    }
    crate::log_trace!("gfx-intel-scanout: minimal-pattern dc tuple permutations complete\n");

    let dc_rearm_offsets = [
        0x45500usize,
        INTEL_DC_STATE_EN,
        0x45510usize,
        INTEL_DC_STATE_DEBUG,
    ];
    let dc_rearm_names = ["45500", "45504", "45510", "45520"];
    let mut dc_rearm_orig = [0u32; 4];
    let mut dc_rearm_test = [0u32; 4];
    for idx in 0..dc_rearm_offsets.len() {
        dc_rearm_orig[idx] = intel_mmio_read32(info, dc_rearm_offsets[idx]);
        dc_rearm_test[idx] = dc_rearm_orig[idx] | 0x00000001;
    }

    for idx in 0..dc_rearm_offsets.len() {
        let _ = intel_mmio_write32(
            info,
            dc_rearm_offsets[idx],
            dc_rearm_orig[idx] & !0x00000001,
        );
    }
    let dc_rearm_low = [
        intel_mmio_read32(info, dc_rearm_offsets[0]),
        intel_mmio_read32(info, dc_rearm_offsets[1]),
        intel_mmio_read32(info, dc_rearm_offsets[2]),
        intel_mmio_read32(info, dc_rearm_offsets[3]),
    ];
    crate::log_trace!(
        "gfx-intel-scanout: minimal-pattern dc-rearm low rb=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}]\n",
        dc_rearm_low[0],
        dc_rearm_low[1],
        dc_rearm_low[2],
        dc_rearm_low[3]
    );

    for idx in 0..dc_rearm_offsets.len() {
        let _ = intel_mmio_write32(info, dc_rearm_offsets[idx], dc_rearm_test[idx]);
        let rb = intel_mmio_read32(info, dc_rearm_offsets[idx]);
        let hotplug_stat = intel_mmio_read32(info, INTEL_PORT_HOTPLUG_STAT);
        let ddi0 = intel_mmio_read32(info, INTEL_DDI_BUF_CTL_0);
        let ddi1 = intel_mmio_read32(info, INTEL_DDI_BUF_CTL_1);
        let ddi2 = intel_mmio_read32(info, INTEL_DDI_BUF_CTL_2);
        let pc_a = intel_mmio_read32(info, INTEL_PIPECONF_A);
        let pc_b = intel_mmio_read32(info, INTEL_PIPECONF_B);
        let pc_c = intel_mmio_read32(info, INTEL_PIPECONF_C);
        let pc_d = intel_mmio_read32(info, INTEL_PIPECONF_D);
        let trans_a = intel_mmio_read32(info, INTEL_TRANS_A_DDI_FUNC_CTL);
        let trans_b = intel_mmio_read32(info, INTEL_TRANS_B_DDI_FUNC_CTL);
        let trans_c = intel_mmio_read32(info, INTEL_TRANS_C_DDI_FUNC_CTL);
        let trans_d = intel_mmio_read32(info, INTEL_TRANS_D_DDI_FUNC_CTL);
        let pipe_a = intel_mmio_read32(info, INTEL_PIPE_A_SRC);
        let pipe_b = intel_mmio_read32(info, INTEL_PIPE_B_SRC);
        let pipe_c = intel_mmio_read32(info, INTEL_PIPE_C_SRC);
        let pipe_d = intel_mmio_read32(info, INTEL_PIPE_D_SRC);
        crate::log_trace!(
            "gfx-intel-scanout: minimal-pattern dc-rearm step={} rb=0x{:08X} hp=0x{:08X} ddi=[0x{:08X},0x{:08X},0x{:08X}] pipeconf=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] trans=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] pipe=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}]\n",
            dc_rearm_names[idx],
            rb,
            hotplug_stat,
            ddi0,
            ddi1,
            ddi2,
            pc_a,
            pc_b,
            pc_c,
            pc_d,
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
    crate::log_trace!("gfx-intel-scanout: minimal-pattern dc rearm complete\n");

    let mut dc_adj_attempts = 0usize;
    let mut dc_adj_hits = 0usize;
    let mut dc_adj_first_hit = 0usize;
    for &off in &[
        0x454D0usize,
        0x454D4usize,
        0x454D8usize,
        0x454DCusize,
        0x45524usize,
        0x45528usize,
        0x4552Cusize,
        0x45530usize,
        0x45534usize,
        0x45538usize,
        0x4553Cusize,
    ] {
        let orig = intel_mmio_read32(info, off);
        let test = orig | 0x00000001;
        dc_adj_attempts += 1;
        let _ = intel_mmio_write32(info, off, test);
        let rb = intel_mmio_read32(info, off);
        if rb == test {
            dc_adj_hits += 1;
            if dc_adj_first_hit == 0 {
                dc_adj_first_hit = off;
            }
            let hotplug_stat = intel_mmio_read32(info, INTEL_PORT_HOTPLUG_STAT);
            let ddi0 = intel_mmio_read32(info, INTEL_DDI_BUF_CTL_0);
            let ddi1 = intel_mmio_read32(info, INTEL_DDI_BUF_CTL_1);
            let ddi2 = intel_mmio_read32(info, INTEL_DDI_BUF_CTL_2);
            let pc_a = intel_mmio_read32(info, INTEL_PIPECONF_A);
            let pc_b = intel_mmio_read32(info, INTEL_PIPECONF_B);
            let pc_c = intel_mmio_read32(info, INTEL_PIPECONF_C);
            let pc_d = intel_mmio_read32(info, INTEL_PIPECONF_D);
            crate::log_trace!(
                "gfx-intel-scanout: minimal-pattern dc-adj hit off=0x{:05X} orig=0x{:08X} rb=0x{:08X} hp=0x{:08X} ddi=[0x{:08X},0x{:08X},0x{:08X}] pipeconf=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}]\n",
                off,
                orig,
                rb,
                hotplug_stat,
                ddi0,
                ddi1,
                ddi2,
                pc_a,
                pc_b,
                pc_c,
                pc_d
            );
        }
        let _ = intel_mmio_write32(info, off, orig);
    }
    crate::log_trace!(
        "gfx-intel-scanout: minimal-pattern dc-adj summary attempts={} hits={} first_hit=0x{:05X}\n",
        dc_adj_attempts,
        dc_adj_hits,
        dc_adj_first_hit
    );
    for idx in (0..dc_rearm_offsets.len()).rev() {
        let _ = intel_mmio_write32(info, dc_rearm_offsets[idx], dc_rearm_orig[idx]);
    }

    for &(trigger_mode, dc_trigger_hold, trigger_off) in &[
        (
            "trig-45510",
            [0x45500usize, INTEL_DC_STATE_EN, INTEL_DC_STATE_DEBUG],
            0x45510usize,
        ),
        (
            "trig-45520",
            [0x45500usize, INTEL_DC_STATE_EN, 0x45510usize],
            INTEL_DC_STATE_DEBUG,
        ),
    ] {
        let dc_trigger_test = [0x00000001u32, 0x00000001u32, 0x00000001u32];
        let mut dc_trigger_orig = [0u32; 3];
        for idx in 0..dc_trigger_hold.len() {
            dc_trigger_orig[idx] = intel_mmio_read32(info, dc_trigger_hold[idx]);
            let _ = intel_mmio_write32(info, dc_trigger_hold[idx], dc_trigger_test[idx]);
        }

        let trigger_orig = intel_mmio_read32(info, trigger_off);
        let trigger_test = trigger_orig | 0x00000001;

        for &(label, gate_off, gate_test) in &[
            ("none", 0usize, 0u32),
            ("13807c", 0x13807Cusize, 0x06000001u32),
            ("138088", 0x138088usize, 0x00000001u32),
            ("13810c", 0x13810Cusize, 0x00000001u32),
            ("6c104", 0x6C104usize, 0x81000401u32),
            ("6c108", 0x6C108usize, 0x00200000u32),
            ("6c114", 0x6C114usize, 0x51000001u32),
            ("6c120", 0x6C120usize, 0x000D0281u32),
        ] {
            let gate_orig = if gate_off != 0 {
                Some(intel_mmio_read32(info, gate_off))
            } else {
                None
            };
            if gate_off != 0 {
                let _ = intel_mmio_write32(info, gate_off, gate_test);
            }
            let gate_rb = if gate_off != 0 {
                intel_mmio_read32(info, gate_off)
            } else {
                0
            };

            let _ = intel_mmio_write32(info, trigger_off, trigger_orig);
            let trigger_low = intel_mmio_read32(info, trigger_off);
            let _ = intel_mmio_write32(info, trigger_off, trigger_test);
            let trigger_high = intel_mmio_read32(info, trigger_off);

            let hotplug_stat = intel_mmio_read32(info, INTEL_PORT_HOTPLUG_STAT);
            let ddi0 = intel_mmio_read32(info, INTEL_DDI_BUF_CTL_0);
            let ddi1 = intel_mmio_read32(info, INTEL_DDI_BUF_CTL_1);
            let ddi2 = intel_mmio_read32(info, INTEL_DDI_BUF_CTL_2);
            let pc_a = intel_mmio_read32(info, INTEL_PIPECONF_A);
            let pc_b = intel_mmio_read32(info, INTEL_PIPECONF_B);
            let pc_c = intel_mmio_read32(info, INTEL_PIPECONF_C);
            let pc_d = intel_mmio_read32(info, INTEL_PIPECONF_D);
            let trans_a = intel_mmio_read32(info, INTEL_TRANS_A_DDI_FUNC_CTL);
            let trans_b = intel_mmio_read32(info, INTEL_TRANS_B_DDI_FUNC_CTL);
            let trans_c = intel_mmio_read32(info, INTEL_TRANS_C_DDI_FUNC_CTL);
            let trans_d = intel_mmio_read32(info, INTEL_TRANS_D_DDI_FUNC_CTL);
            let pipe_a = intel_mmio_read32(info, INTEL_PIPE_A_SRC);
            let pipe_b = intel_mmio_read32(info, INTEL_PIPE_B_SRC);
            let pipe_c = intel_mmio_read32(info, INTEL_PIPE_C_SRC);
            let pipe_d = intel_mmio_read32(info, INTEL_PIPE_D_SRC);

            crate::log_trace!(
                "gfx-intel-scanout: minimal-pattern dc-trigger mode={} pair={} gate_rb=0x{:08X} trig_low=0x{:08X} trig_high=0x{:08X} hp=0x{:08X} ddi=[0x{:08X},0x{:08X},0x{:08X}] pipeconf=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] trans=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] pipe=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}]\n",
                trigger_mode,
                label,
                gate_rb,
                trigger_low,
                trigger_high,
                hotplug_stat,
                ddi0,
                ddi1,
                ddi2,
                pc_a,
                pc_b,
                pc_c,
                pc_d,
                trans_a,
                trans_b,
                trans_c,
                trans_d,
                pipe_a,
                pipe_b,
                pipe_c,
                pipe_d
            );

            let _ = intel_mmio_write32(info, trigger_off, trigger_orig);
            if let Some(orig) = gate_orig {
                let _ = intel_mmio_write32(info, gate_off, orig);
            }
        }

        let gt_triplet_offsets = [0x13807Cusize, 0x138088usize, 0x13810Cusize];
        let gt_triplet_tests = [0x06000001u32, 0x00000001u32, 0x00000001u32];
        let mut gt_triplet_orig = [0u32; 3];
        let mut gt_triplet_rb = [0u32; 3];
        for idx in 0..gt_triplet_offsets.len() {
            gt_triplet_orig[idx] = intel_mmio_read32(info, gt_triplet_offsets[idx]);
            let _ = intel_mmio_write32(info, gt_triplet_offsets[idx], gt_triplet_tests[idx]);
            gt_triplet_rb[idx] = intel_mmio_read32(info, gt_triplet_offsets[idx]);
        }
        let _ = intel_mmio_write32(info, trigger_off, trigger_orig);
        let gt_trigger_low = intel_mmio_read32(info, trigger_off);
        let _ = intel_mmio_write32(info, trigger_off, trigger_test);
        let gt_trigger_high = intel_mmio_read32(info, trigger_off);
        let gt_hotplug_stat = intel_mmio_read32(info, INTEL_PORT_HOTPLUG_STAT);
        let gt_ddi0 = intel_mmio_read32(info, INTEL_DDI_BUF_CTL_0);
        let gt_ddi1 = intel_mmio_read32(info, INTEL_DDI_BUF_CTL_1);
        let gt_ddi2 = intel_mmio_read32(info, INTEL_DDI_BUF_CTL_2);
        let gt_pc_a = intel_mmio_read32(info, INTEL_PIPECONF_A);
        let gt_pc_b = intel_mmio_read32(info, INTEL_PIPECONF_B);
        let gt_pc_c = intel_mmio_read32(info, INTEL_PIPECONF_C);
        let gt_pc_d = intel_mmio_read32(info, INTEL_PIPECONF_D);
        let gt_trans_a = intel_mmio_read32(info, INTEL_TRANS_A_DDI_FUNC_CTL);
        let gt_trans_b = intel_mmio_read32(info, INTEL_TRANS_B_DDI_FUNC_CTL);
        let gt_trans_c = intel_mmio_read32(info, INTEL_TRANS_C_DDI_FUNC_CTL);
        let gt_trans_d = intel_mmio_read32(info, INTEL_TRANS_D_DDI_FUNC_CTL);
        let gt_pipe_a = intel_mmio_read32(info, INTEL_PIPE_A_SRC);
        let gt_pipe_b = intel_mmio_read32(info, INTEL_PIPE_B_SRC);
        let gt_pipe_c = intel_mmio_read32(info, INTEL_PIPE_C_SRC);
        let gt_pipe_d = intel_mmio_read32(info, INTEL_PIPE_D_SRC);
        crate::log_trace!(
            "gfx-intel-scanout: minimal-pattern dc-trigger mode={} gt-triplet rb=[0x{:08X},0x{:08X},0x{:08X}] trig_low=0x{:08X} trig_high=0x{:08X} hp=0x{:08X} ddi=[0x{:08X},0x{:08X},0x{:08X}] pipeconf=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] trans=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] pipe=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}]\n",
            trigger_mode,
            gt_triplet_rb[0],
            gt_triplet_rb[1],
            gt_triplet_rb[2],
            gt_trigger_low,
            gt_trigger_high,
            gt_hotplug_stat,
            gt_ddi0,
            gt_ddi1,
            gt_ddi2,
            gt_pc_a,
            gt_pc_b,
            gt_pc_c,
            gt_pc_d,
            gt_trans_a,
            gt_trans_b,
            gt_trans_c,
            gt_trans_d,
            gt_pipe_a,
            gt_pipe_b,
            gt_pipe_c,
            gt_pipe_d
        );
        let _ = intel_mmio_write32(info, trigger_off, trigger_orig);
        for idx in (0..gt_triplet_offsets.len()).rev() {
            let _ = intel_mmio_write32(info, gt_triplet_offsets[idx], gt_triplet_orig[idx]);
        }

        for idx in (0..dc_trigger_hold.len()).rev() {
            let _ = intel_mmio_write32(info, dc_trigger_hold[idx], dc_trigger_orig[idx]);
        }
    }
    crate::log_trace!("gfx-intel-scanout: minimal-pattern dc trigger bridge complete\n");

    for idx in (0..power_hold_offsets.len()).rev() {
        let _ = intel_mmio_write32(info, power_hold_offsets[idx], power_hold_orig[idx]);
    }

    for &idx in [4usize, 3, 2, 1, 0].iter() {
        let _ = intel_mmio_write32(info, bundle_offsets[idx], bundle_orig[idx]);
    }

    let _ = intel_mmio_write32(info, held_off, held_orig);
    crate::log_trace!("gfx-intel-scanout: minimal-pattern dc-pll held probe complete\n");
}

#[embassy_executor::task]
pub async fn scanout_smoke_task() {
    let Some(info) = first_claimed_device() else {
        crate::log_trace!("gfx-intel-scanout: skipped (no claimed Intel gfx device)\n");
        return;
    };

    Timer::after(EmbassyDuration::from_millis(1200)).await;
    if INTEL_COMPACT_CROSS_ISLAND_ONLY {
        minimal_pattern_register_poke(info);
        return;
    }

    log_display_power_probe(info);
    log_hdmi_port_probe(info);
    power_first_gate_hunt(info);
    if !INTEL_PASSIVE_ONLY_DEFAULT {
        let disp_pwron_latched = arm_display_power_smoke(info);
        let hotplug_latched = hotplug_write_smoke(info);
        if disp_pwron_latched {
            Timer::after(EmbassyDuration::from_millis(25)).await;
            crate::log_trace!("gfx-intel-scanout: re-probing after GT_DISP_PWRON latch\n");
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
            crate::log_trace!(
                "gfx-intel-scanout: retrying plane/aperture probe tries={}\n",
                tries
            );
        }
        if tries >= INTEL_SCANOUT_RETRIES {
            crate::log_trace!("gfx-intel-scanout: giving up after {} retries\n", tries);
            return;
        }
        Timer::after(EmbassyDuration::from_millis(INTEL_SCANOUT_RETRY_MS)).await;
    }
}
