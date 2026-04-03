use core::{
    ptr::NonNull,
    sync::atomic::{AtomicBool, Ordering},
};

use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use spin::Mutex;
use trueos_gfx_core::{
    BufferDesc, BufferId, BufferUsage, Command, CommandBuffer, FenceId, GfxContext, ImageDesc,
    ImageFormat, ImageId, MemoryType, PipelineDesc, PipelineId, RGB_VERTEX_SIZE, RgbVertex, Rgba8,
    TexCoordFormat, VertexLayout, Viewport,
};

const INTEL_VENDOR_ID: u16 = 0x8086;
const INTEL_IGPU770_DEVICE_ID: u16 = 0x4680;
const PCI_CLASS_DISPLAY: u8 = 0x03;
const INTEL_ASYNC_PROBE_DELAY_MS: u64 = 0;

const INTEL_BXT_DE_PLL_CTL: usize = 0x6D000;
const INTEL_BXT_DE_PLL_ENABLE: usize = 0x46070;
const INTEL_DC_STATE_EN: usize = 0x45504;
const INTEL_DC_STATE_DEBUG: usize = 0x45520;
const INTEL_DISPLAY_SWEEP_START: usize = 0x60000;
const INTEL_DISPLAY_SWEEP_END: usize = 0x74000;
const INTEL_DISPLAY_PAGE_STRIDE: usize = 0x1000;
const INTEL_DISPLAY_SWEEP_LOG_LIMIT: usize = 8;
const INTEL_DISPLAY_WINDOW_DWORDS: usize = 8;
const INTEL_DISPLAY_SIGNATURE_TOP_PAGES: usize = 6;
const INTEL_DISPLAY_SIGNATURE_WINDOW_DWORDS: usize = 8;
const INTEL_GT_DISP_PWRON: usize = 0x138090;
const INTEL_GT_DISP_PWRON_REQ: u32 = 0x0000_0001;
const INTEL_ICL_PHY_MISC_A: usize = 0x64C00;
const INTEL_ICL_PHY_MISC_B: usize = 0x64C04;
const INTEL_OWNED_TRIANGLE_CLEAR_RGB: u32 = 0x10141A;
const INTEL_OWNED_TRIANGLE_FRAME_MS: u64 = 16;
const INTEL_OWNED_TRIANGLE_PHASE_STEP_RAD: f32 = 0.18;
const INTEL_OWNED_TRIANGLE_PROOF_ENABLED: bool = true;
const INTEL_OWNED_TRIANGLE_PROOF_TIMEOUT_MS: u64 = 750;
const INTEL_PLANE_ENABLE: u32 = 1 << 31;
const INTEL_PORT_HOTPLUG_EN: usize = 0x61110;
const INTEL_PIPE_A_SRC: usize = 0x7001C;
const INTEL_PIPE_B_SRC: usize = 0x7101C;
const INTEL_PIPE_C_SRC: usize = 0x7201C;
const INTEL_PIPE_D_SRC: usize = 0x7301C;
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
const INTEL_DISPIO_CR_TX_BMU_CR0: usize = 0x6C00C;

const INTEL_SCANOUT_PIPES: [(char, usize, usize); 4] = [
    ('A', INTEL_PIPE_A_SRC, INTEL_TRANS_A_DDI_FUNC_CTL),
    ('B', INTEL_PIPE_B_SRC, INTEL_TRANS_B_DDI_FUNC_CTL),
    ('C', INTEL_PIPE_C_SRC, INTEL_TRANS_C_DDI_FUNC_CTL),
    ('D', INTEL_PIPE_D_SRC, INTEL_TRANS_D_DDI_FUNC_CTL),
];

#[derive(Copy, Clone, Debug)]
pub struct IntelDeviceInfo {
    pub bus: u8,
    pub slot: u8,
    pub function: u8,
    pub device_id: u16,
    pub revision_id: u8,
    pub bar0_phys: u64,
    pub bar0_size: u64,
    pub aperture_bar_phys: u64,
    pub aperture_bar_size: u64,
    pub mmio_base: NonNull<u8>,
    pub mmio_len: usize,
}

unsafe impl Send for IntelDeviceInfo {}
unsafe impl Sync for IntelDeviceInfo {}

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

static FIRST_DEVICE: Mutex<Option<IntelDeviceInfo>> = Mutex::new(None);
static INTEL_IGPU770_PRESENT: AtomicBool = AtomicBool::new(false);
static INTEL_OWNED_TRIANGLE_PROOF_DISABLED: AtomicBool = AtomicBool::new(false);
static INTEL_OWNED_TRIANGLE_PROOF_LATCH_LOGGED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy)]
struct OwnedTriangleProofResources {
    render_target: ImageId,
    rgb_pipeline: PipelineId,
    rgb_buffer: BufferId,
    screen_w: u32,
    screen_h: u32,
}

impl OwnedTriangleProofResources {
    const fn invalid() -> Self {
        Self {
            render_target: ImageId::invalid(),
            rgb_pipeline: PipelineId::invalid(),
            rgb_buffer: BufferId::invalid(),
            screen_w: 0,
            screen_h: 0,
        }
    }
}

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

#[inline]
fn is_intel_display(dev: &crate::pci::PciDevice) -> bool {
    dev.vendor == INTEL_VENDOR_ID && dev.class == PCI_CLASS_DISPLAY
}

fn decode_mmio_bar(bus: u8, slot: u8, function: u8, index: u8) -> Option<(u64, u64)> {
    let (bar_lo, bar_hi) = crate::pci::read_bar_raw(bus, slot, function, index);
    if bar_lo == 0 || bar_lo == 0xFFFF_FFFF || (bar_lo & 0x1) != 0 {
        return None;
    }

    let size = crate::pci::bar_size_bytes(bus, slot, function, index)?;
    let base = if let Some(hi) = bar_hi {
        (((hi as u64) << 32) | (bar_lo as u64)) & !0xFu64
    } else {
        (bar_lo as u64) & !0xFu64
    };
    Some((base, size))
}

#[inline]
pub(crate) fn mmio_read32(info: IntelDeviceInfo, off: usize) -> u32 {
    if off + 4 > info.mmio_len {
        return 0;
    }
    let ptr = unsafe { info.mmio_base.as_ptr().add(off) as *const u32 };
    unsafe { core::ptr::read_volatile(ptr) }
}

#[inline]
pub(crate) fn mmio_write32(info: IntelDeviceInfo, off: usize, value: u32) -> bool {
    if off + 4 > info.mmio_len {
        return false;
    }
    let ptr = unsafe { info.mmio_base.as_ptr().add(off) as *mut u32 };
    unsafe { core::ptr::write_volatile(ptr, value) };
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

fn log_display_window(info: IntelDeviceInfo, center_off: usize, label: &str) {
    let aligned = center_off & !0x1Fusize;
    crate::log!("intel: window label={} base=0x{:05X}\n", label, aligned);
    let mut idx = 0usize;
    while idx < INTEL_DISPLAY_WINDOW_DWORDS {
        let off = aligned + idx.saturating_mul(4);
        let value = mmio_read32(info, off);
        crate::log!("intel: window-mmio label={} off=0x{:05X} value=0x{:08X}\n", label, off, value);
        idx += 1;
    }
}

fn log_display_focus_windows(info: IntelDeviceInfo) {
    if !crate::logflag::INTEL_GFX_DEBUG_LOGFLAG {
        return;
    }
    log_display_window(info, INTEL_BXT_DE_PLL_ENABLE, "de_pll_enable");
    log_display_window(info, INTEL_PORT_HOTPLUG_EN, "hotplug");
    log_display_window(info, INTEL_TRANS_A_DDI_FUNC_CTL, "trans_a");
    log_display_window(info, INTEL_TRANS_B_DDI_FUNC_CTL, "trans_b");
    log_display_window(info, INTEL_GT_DISP_PWRON, "gt_disp_pwron");
}

fn log_display_region_sweep(info: IntelDeviceInfo) {
    if !crate::logflag::INTEL_GFX_DEBUG_LOGFLAG {
        return;
    }
    let mut logged = 0usize;
    let mut page = INTEL_DISPLAY_SWEEP_START;
    while page < INTEL_DISPLAY_SWEEP_END {
        let mut found = None;
        let mut off = 0usize;
        while off < INTEL_DISPLAY_PAGE_STRIDE {
            let value = mmio_read32(info, page + off);
            if value != 0 {
                found = Some((off, value));
                break;
            }
            off += 4;
        }
        if let Some((first_off, value)) = found {
            crate::log!(
                "intel: display-page page=0x{:05X} first=0x{:03X} value=0x{:08X}\n",
                page,
                first_off,
                value
            );
            logged += 1;
            if logged >= INTEL_DISPLAY_SWEEP_LOG_LIMIT {
                break;
            }
        }
        page += INTEL_DISPLAY_PAGE_STRIDE;
    }
    if logged == 0 {
        crate::log!(
            "intel: display-page sweep 0x{:05X}..0x{:05X} found no nonzero registers\n",
            INTEL_DISPLAY_SWEEP_START,
            INTEL_DISPLAY_SWEEP_END
        );
    }
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

fn log_signature_window(info: IntelDeviceInfo, page: usize) {
    if !crate::logflag::INTEL_GFX_DEBUG_LOGFLAG {
        return;
    }
    let mut idx = 0usize;
    while idx < INTEL_DISPLAY_SIGNATURE_WINDOW_DWORDS {
        let off = page + idx.saturating_mul(4);
        let value = mmio_read32(info, off);
        crate::log!("intel: signature-mmio off=0x{:05X} value=0x{:08X}\n", off, value);
        idx += 1;
    }
}

fn log_display_signature_sweep(info: IntelDeviceInfo) {
    if !crate::logflag::INTEL_GFX_DEBUG_LOGFLAG {
        return;
    }
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
            let value = mmio_read32(info, mmio_off);
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
        "intel: signature-sweep begin mmio_len=0x{:X} aperture=0x{:X}\n",
        info.mmio_len,
        info.aperture_bar_size
    );
    let mut rank = 0usize;
    while rank < top.len() && top[rank].score != 0 {
        let cand = top[rank];
        let (pipe_w, pipe_h) = plausible_pipe_src(cand.pipe_src_value).unwrap_or((0, 0));
        crate::log!(
            "intel: signature-candidate rank={} page=0x{:05X} score={} nonzero={} pipe_src_off={} pipe_src=0x{:08X} size={}x{} stride_off={} stride=0x{:08X} surf_off={} surf=0x{:08X} ctl_off={} ctl=0x{:08X}\n",
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
        if rank < 2 {
            log_signature_window(info, cand.page);
        }
        rank += 1;
    }
    if rank == 0 {
        crate::log!("intel: signature-sweep found no plausible scanout pages\n");
    }
}

fn log_plane_inventory(info: IntelDeviceInfo) {
    if !crate::logflag::INTEL_GFX_DEBUG_LOGFLAG {
        return;
    }
    let mut nonzero_pipes = 0usize;
    let mut nonzero_planes = 0usize;
    let mut enabled_planes = 0usize;

    for pipe in 0..INTEL_SCANOUT_PIPES.len() {
        let plane0 = scanout_plane(pipe, 0);
        let pipe_src = mmio_read32(info, plane0.pipe_src_off);
        let trans_ddi = mmio_read32(info, plane0.trans_ddi_func_ctl_off);
        let (pipe_w, pipe_h) = decode_pipe_src(pipe_src);
        if pipe_src != 0 || trans_ddi != 0 {
            nonzero_pipes += 1;
        }
        crate::log!(
            "intel: pipe-live pipe={} pipe_src=0x{:08X} size={}x{} ddi=0x{:08X}\n",
            plane0.pipe_name,
            pipe_src,
            pipe_w,
            pipe_h,
            trans_ddi
        );

        for plane_slot in 0..4 {
            let plane = scanout_plane(pipe, plane_slot);
            let ctl = mmio_read32(info, plane.ctl_off);
            let stride = mmio_read32(info, plane.stride_off);
            let surf = mmio_read32(info, plane.surf_off);
            let surf_live = mmio_read32(info, plane.surf_live_off);
            let enabled = (ctl & INTEL_PLANE_ENABLE) != 0;
            if enabled {
                enabled_planes += 1;
            }
            if ctl != 0 || stride != 0 || surf != 0 || surf_live != 0 {
                nonzero_planes += 1;
                crate::log!(
                    "intel: plane-live {}{} ctl=0x{:08X} stride=0x{:08X} surf=0x{:08X} surf_live=0x{:08X} enabled={}\n",
                    plane.pipe_name,
                    plane.plane_slot,
                    ctl,
                    stride,
                    surf,
                    surf_live,
                    enabled as u8
                );
            }
        }
    }

    crate::log!(
        "intel: plane-scan summary nonzero_pipes={} nonzero_planes={} enabled_planes={}\n",
        nonzero_pipes,
        nonzero_planes,
        enabled_planes
    );
}

fn log_display_power_probe(info: IntelDeviceInfo) {
    let phy_misc_a = mmio_read32(info, INTEL_ICL_PHY_MISC_A);
    let phy_misc_b = mmio_read32(info, INTEL_ICL_PHY_MISC_B);
    let tx_bmu = mmio_read32(info, INTEL_DISPIO_CR_TX_BMU_CR0);
    let de_pll_ctl = mmio_read32(info, INTEL_BXT_DE_PLL_CTL);
    let de_pll_enable = mmio_read32(info, INTEL_BXT_DE_PLL_ENABLE);
    let dc_state_en = mmio_read32(info, INTEL_DC_STATE_EN);
    let dc_state_debug = mmio_read32(info, INTEL_DC_STATE_DEBUG);
    let hotplug = mmio_read32(info, INTEL_PORT_HOTPLUG_EN);
    let gt_disp_pwron = mmio_read32(info, INTEL_GT_DISP_PWRON);

    crate::log!(
        "intel: display-power probe phy_misc_a=0x{:08X} phy_misc_b=0x{:08X} tx_bmu=0x{:08X} de_pll_ctl=0x{:08X} de_pll_enable=0x{:08X} dc_state_en=0x{:08X} dc_state_debug=0x{:08X} hotplug=0x{:08X} gt_disp_pwron=0x{:08X}\n",
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

fn log_display_routing_probe(info: IntelDeviceInfo) {
    let hotplug_en = mmio_read32(info, INTEL_PORT_HOTPLUG_EN);
    let trans_a = mmio_read32(info, INTEL_TRANS_A_DDI_FUNC_CTL);
    let trans_b = mmio_read32(info, INTEL_TRANS_B_DDI_FUNC_CTL);
    let trans_c = mmio_read32(info, INTEL_TRANS_C_DDI_FUNC_CTL);
    let trans_d = mmio_read32(info, INTEL_TRANS_D_DDI_FUNC_CTL);
    let pipe_a = mmio_read32(info, INTEL_PIPE_A_SRC);
    let pipe_b = mmio_read32(info, INTEL_PIPE_B_SRC);
    let pipe_c = mmio_read32(info, INTEL_PIPE_C_SRC);
    let pipe_d = mmio_read32(info, INTEL_PIPE_D_SRC);

    crate::log!(
        "intel: display-routing probe hotplug_en=0x{:08X} trans_a=0x{:08X} trans_b=0x{:08X} trans_c=0x{:08X} trans_d=0x{:08X} pipe_a=0x{:08X} pipe_b=0x{:08X} pipe_c=0x{:08X} pipe_d=0x{:08X}\n",
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

fn arm_display_power_smoke(info: IntelDeviceInfo) -> super::xelp_display_ngin::DisplayPowerState {
    super::xelp_display_ngin::request_display_power_smoke(info).power_state
}

fn run_display_power_discovery(info: IntelDeviceInfo) {
    crate::log!(
        "intel: display discovery begin bdf={:02X}:{:02X}.{} device=0x{:04X} bar0=0x{:X} mmio_len=0x{:X} aperture=0x{:X}\n",
        info.bus,
        info.slot,
        info.function,
        info.device_id,
        info.bar0_phys,
        info.mmio_len,
        info.aperture_bar_size
    );
    let early_stub = super::xelp_display_ngin::log_early_display_stub(info, "discovery");
    crate::log!(
        "intel: display-ngin stub summary power_visible={} dc_state_mask=0x{:08X} next_gt_disp_pwron=0x{:08X}\n",
        early_stub.has_visible_display_power() as u8,
        early_stub.dc_state_blocking_mask(),
        early_stub.next_gt_disp_pwron_request()
    );
    crate::log!("intel: display discovery step=probe scope=power+routing\n");
    log_display_power_probe(info);
    log_display_routing_probe(info);
    log_display_focus_windows(info);
    log_display_region_sweep(info);
    log_display_signature_sweep(info);
    log_plane_inventory(info);
    crate::log!("intel: display discovery step=smoke action=request-display-power\n");

    // Request display power using igpu770 helper if available (requires forcewake)
    let power_state = if intel_igpu770_present() {
        if let Some(warm) = super::intel_igpu770::warm_state() {
            super::intel_igpu770::request_display_power_with_forcewake(warm)
        } else {
            arm_display_power_smoke(info)
        }
    } else {
        arm_display_power_smoke(info)
    };

    crate::log!(
        "intel: display discovery result gt_disp_pwron_latched={} power_state={}\n",
        power_state.is_software_latched() as u8,
        power_state.as_str()
    );
    super::xelp_display_ngin::kickoff_once(info, power_state);
}

pub fn init_once() {
    if crate::limine::hhdm_offset().is_none() {
        crate::log!("intel: init skipped (no HHDM)\n");
        return;
    }

    let mut claimed = None;
    crate::pci::with_devices(|list| {
        for dev in list {
            if !is_intel_display(dev) {
                continue;
            }

            let Some((bar0_phys, bar0_size)) = decode_mmio_bar(dev.bus, dev.slot, dev.function, 0)
            else {
                crate::log!(
                    "intel: skip {:02X}:{:02X}.{} device=0x{:04X} (BAR0 MMIO unavailable)\n",
                    dev.bus,
                    dev.slot,
                    dev.function,
                    dev.device
                );
                continue;
            };

            let (aperture_bar_phys, aperture_bar_size) =
                decode_mmio_bar(dev.bus, dev.slot, dev.function, 2).unwrap_or((0, 0));

            crate::pci::enable_mem_and_bus_master(dev.bus, dev.slot, dev.function);

            let mmio_len = usize::try_from(bar0_size).unwrap_or(usize::MAX);
            let Ok(mmio_base) = crate::pci::mmio::map_mmio_region_exact(bar0_phys, mmio_len) else {
                crate::log!(
                    "intel: skip {:02X}:{:02X}.{} device=0x{:04X} (MMIO map failed)\n",
                    dev.bus,
                    dev.slot,
                    dev.function,
                    dev.device
                );
                continue;
            };

            claimed = Some(IntelDeviceInfo {
                bus: dev.bus,
                slot: dev.slot,
                function: dev.function,
                device_id: dev.device,
                revision_id: crate::pci::config_read_u8(dev.bus, dev.slot, dev.function, 0x08),
                bar0_phys,
                bar0_size,
                aperture_bar_phys,
                aperture_bar_size,
                mmio_base,
                mmio_len,
            });
            break;
        }
    });

    let mut first = FIRST_DEVICE.lock();
    *first = claimed;
    INTEL_IGPU770_PRESENT.store(
        claimed
            .map(|info| info.device_id == INTEL_IGPU770_DEVICE_ID)
            .unwrap_or(false),
        Ordering::Release,
    );

    if let Some(info) = *first {
        crate::log!(
            "intel: claimed {:02X}:{:02X}.{} device=0x{:04X} rev=0x{:02X} bar0=0x{:X} size=0x{:X} bar2=0x{:X} bar2_size=0x{:X} mmio=0x{:X}\n",
            info.bus,
            info.slot,
            info.function,
            info.device_id,
            info.revision_id,
            info.bar0_phys,
            info.bar0_size,
            info.aperture_bar_phys,
            info.aperture_bar_size,
            info.mmio_base.as_ptr() as usize
        );
        crate::r::readiness::set(crate::r::readiness::GFX_INTEL_CLAIMED);
        crate::log!("intel: readiness=claimed\n");
        crate::log!("intel: intel_igpu770_present={}\n", intel_igpu770_present() as u8);
        if intel_igpu770_present() {
            super::intel_igpu770::warm_once(info);
        }
    } else {
        crate::log!("intel: no Intel display-class PCI device claimed\n");
    }
}

#[inline]
pub fn has_claimed_device() -> bool {
    FIRST_DEVICE.lock().is_some()
}

#[inline]
pub fn intel_igpu770_present() -> bool {
    INTEL_IGPU770_PRESENT.load(Ordering::Acquire)
}

#[inline]
fn owned_triangle_proof_mode_active() -> bool {
    INTEL_OWNED_TRIANGLE_PROOF_ENABLED
        && intel_igpu770_present()
        && !INTEL_OWNED_TRIANGLE_PROOF_DISABLED.load(Ordering::Acquire)
}

fn disable_owned_triangle_proof_once(reason: &'static str) {
    INTEL_OWNED_TRIANGLE_PROOF_DISABLED.store(true, Ordering::Release);
    if !INTEL_OWNED_TRIANGLE_PROOF_LATCH_LOGGED.swap(true, Ordering::AcqRel) {
        crate::log!("intel: owned-triangle-proof disabled reason={} timeout_latched=1\n", reason);
    }
}

pub fn isolated_triangle_mode_active() -> bool {
    owned_triangle_proof_mode_active()
}

fn owned_triangle_vertices(phase: f32) -> [RgbVertex; 3] {
    let cos_p = libm::cosf(phase);
    let sin_p = libm::sinf(phase);
    let rotate =
        |x: f32, y: f32| -> (f32, f32) { ((x * cos_p) - (y * sin_p), (x * sin_p) + (y * cos_p)) };
    let (x0, y0) = rotate(0.0, -0.65);
    let (x1, y1) = rotate(-0.7, 0.55);
    let (x2, y2) = rotate(0.7, 0.55);
    [
        RgbVertex {
            x: x0,
            y: y0,
            color: Rgba8::new(0xFF, 0x52, 0x52, 0xFF),
        },
        RgbVertex {
            x: x1,
            y: y1,
            color: Rgba8::new(0x40, 0xE3, 0x92, 0xFF),
        },
        RgbVertex {
            x: x2,
            y: y2,
            color: Rgba8::new(0x5A, 0x9C, 0xFF, 0xFF),
        },
    ]
}

#[inline]
fn rgb_vertex_bytes(vertices: &[RgbVertex; 3]) -> &[u8] {
    unsafe {
        core::slice::from_raw_parts(
            vertices.as_ptr() as *const u8,
            core::mem::size_of_val(vertices),
        )
    }
}

fn destroy_owned_triangle_proof_resources(
    ctx: &mut dyn GfxContext,
    resources: OwnedTriangleProofResources,
) {
    if resources.rgb_buffer.is_valid() {
        ctx.destroy_buffer(resources.rgb_buffer);
    }
    if resources.rgb_pipeline.is_valid() {
        ctx.destroy_pipeline(resources.rgb_pipeline);
    }
    if resources.render_target.is_valid() {
        ctx.destroy_image(resources.render_target);
    }
}

fn ensure_owned_triangle_proof_resources(
    ctx: &mut dyn GfxContext,
    resources: &mut OwnedTriangleProofResources,
) -> Option<OwnedTriangleProofResources> {
    let swap = ctx.swapchain_desc();
    let screen_w = swap.extent.width.max(1);
    let screen_h = swap.extent.height.max(1);
    if resources.render_target.is_valid()
        && resources.screen_w == screen_w
        && resources.screen_h == screen_h
    {
        return Some(*resources);
    }

    if resources.render_target.is_valid() {
        destroy_owned_triangle_proof_resources(ctx, *resources);
        *resources = OwnedTriangleProofResources::invalid();
    }

    let rgb_pipeline = ctx
        .create_pipeline(PipelineDesc {
            vertex_layout: VertexLayout {
                stride: RGB_VERTEX_SIZE as u16,
                pos_offset: 0,
                color_offset: 8,
                color_format: trueos_gfx_core::ColorFormat::RgbaU8,
                texcoord_offset: 0,
                texcoord_format: TexCoordFormat::None,
            },
            vs: None,
            fs: None,
        })
        .ok()?;
    let rgb_buffer = ctx
        .create_buffer(BufferDesc {
            size: core::mem::size_of::<RgbVertex>() as u64 * 3,
            usage: BufferUsage::Vertex,
            memory: MemoryType::HostVisible,
        })
        .ok()?;
    let render_target = ctx
        .create_image(ImageDesc {
            width: screen_w,
            height: screen_h,
            format: ImageFormat::Rgba8888,
        })
        .ok()?;

    *resources = OwnedTriangleProofResources {
        render_target,
        rgb_pipeline,
        rgb_buffer,
        screen_w,
        screen_h,
    };
    Some(*resources)
}

fn submit_owned_triangle_proof_frame(
    ctx: &mut dyn GfxContext,
    resources: &mut OwnedTriangleProofResources,
    phase: f32,
) -> Option<FenceId> {
    let resources = ensure_owned_triangle_proof_resources(ctx, resources)?;
    let vertices = owned_triangle_vertices(phase);
    if ctx
        .write_buffer(resources.rgb_buffer, 0, rgb_vertex_bytes(&vertices))
        .is_err()
    {
        return None;
    }
    let viewport = Viewport {
        x: 0,
        y: 0,
        width: resources.screen_w as i32,
        height: resources.screen_h as i32,
    };
    let cmds = [
        Command::SetViewport(viewport),
        Command::SetRenderTarget(Some(resources.render_target)),
        Command::ClearColor {
            rgb: INTEL_OWNED_TRIANGLE_CLEAR_RGB,
        },
        Command::BindPipeline(resources.rgb_pipeline),
        Command::BindVertexBuffer {
            buffer: resources.rgb_buffer,
            offset: 0,
        },
        Command::Draw {
            vertex_count: 3,
            first_vertex: 0,
        },
        Command::Present,
    ];
    ctx.submit(CommandBuffer { commands: &cmds }).ok()
}

async fn wait_owned_triangle_fence(fence: FenceId) -> bool {
    if !fence.is_valid() {
        return false;
    }
    let deadline =
        Instant::now() + EmbassyDuration::from_millis(INTEL_OWNED_TRIANGLE_PROOF_TIMEOUT_MS);
    loop {
        let ready = crate::gfx::with_context_tag(crate::gfx::SystemLockOwner::Unknown, |ctx| {
            ctx.poll(fence)
        })
        .unwrap_or(false);
        if ready {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        Timer::after(EmbassyDuration::from_millis(1)).await;
    }
}

async fn run_owned_triangle_proof_loop() {
    let mut resources = OwnedTriangleProofResources::invalid();
    let mut phase = super::xelp_render_ngin::default_rgb_triangle_rotation();
    let mut frame_seq = 0u32;

    crate::log!(
        "intel: owned-triangle-proof start frame_ms={} timeout_ms={} clear=0x{:06X}\n",
        INTEL_OWNED_TRIANGLE_FRAME_MS,
        INTEL_OWNED_TRIANGLE_PROOF_TIMEOUT_MS,
        INTEL_OWNED_TRIANGLE_CLEAR_RGB & 0x00FF_FFFF
    );

    loop {
        let fence = crate::gfx::with_context_tag(crate::gfx::SystemLockOwner::Unknown, |ctx| {
            submit_owned_triangle_proof_frame(ctx, &mut resources, phase)
        })
        .flatten();
        let Some(fence) = fence else {
            disable_owned_triangle_proof_once("submit");
            break;
        };
        if !wait_owned_triangle_fence(fence).await {
            disable_owned_triangle_proof_once("poll-timeout");
            break;
        }
        frame_seq = frame_seq.wrapping_add(1);
        if frame_seq <= 8 || frame_seq.is_multiple_of(240) {
            crate::log!(
                "intel: owned-triangle-proof frame={} phase_millirad={}\n",
                frame_seq,
                (phase * 1000.0) as i32
            );
        }
        phase += INTEL_OWNED_TRIANGLE_PHASE_STEP_RAD;
        if phase > core::f32::consts::TAU {
            phase -= core::f32::consts::TAU;
        }
        Timer::after(EmbassyDuration::from_millis(INTEL_OWNED_TRIANGLE_FRAME_MS)).await;
    }

    let _ = crate::gfx::with_context_tag(crate::gfx::SystemLockOwner::Unknown, |ctx| {
        destroy_owned_triangle_proof_resources(ctx, resources);
    });
}

#[inline]
pub fn first_claimed_device() -> Option<IntelDeviceInfo> {
    *FIRST_DEVICE.lock()
}

pub fn active_scanout_dimensions() -> Option<(u32, u32)> {
    let info = first_claimed_device()?;
    let mut fallback = None;

    for pipe in 0..INTEL_SCANOUT_PIPES.len() {
        let plane0 = scanout_plane(pipe, 0);
        let pipe_src = mmio_read32(info, plane0.pipe_src_off);
        let trans_ddi = mmio_read32(info, plane0.trans_ddi_func_ctl_off);
        let dims = plausible_pipe_src(pipe_src).and_then(|(width, height)| {
            Some((u32::try_from(width).ok()?, u32::try_from(height).ok()?))
        });
        let Some((width, height)) = dims else {
            continue;
        };
        if fallback.is_none() {
            fallback = Some((width, height));
        }
        if trans_ddi != 0 {
            return Some((width, height));
        }
    }

    fallback
}

#[embassy_executor::task]
pub async fn scanout_smoke_task() {
    let Some(info) = first_claimed_device() else {
        crate::log!("intel: display discovery skipped (no claimed Intel device)\n");
        return;
    };

    crate::log!("intel: async probe delayed by {}ms (non-blocking)\n", INTEL_ASYNC_PROBE_DELAY_MS);
    Timer::after(EmbassyDuration::from_millis(INTEL_ASYNC_PROBE_DELAY_MS)).await;

    if intel_igpu770_present() {
        super::intel_igpu770::warm_once(info);
    }

    crate::gfx::init(crate::limine::framebuffer_response());
    let intel_backend_active = if crate::gfx::is_intel_active() {
        crate::log!("intel: gfx backend already active=Intel\n");
        true
    } else {
        let switched = crate::gfx::switch_to_intel();
        crate::log!("intel: gfx backend switch_to_intel={}\n", switched as u8);
        switched && crate::gfx::is_intel_active()
    };

    if !intel_backend_active {
        disable_owned_triangle_proof_once("backend-switch");
        crate::log!("intel: owned-triangle-proof skipped reason=backend-inactive\n");
        return;
    }

    Timer::after(EmbassyDuration::from_millis(1200)).await;
    if intel_igpu770_present() {
        crate::log!("intel: owned-triangle-proof selected; legacy smoke branches disabled\n");
    }

    let defer_display =
        super::xelp_render_ngin::defer_display_discovery_for_render_first(intel_igpu770_present());
    if defer_display {
        super::xelp_render_ngin::log_display_deferred_for_render_first();
        super::xelp_render_ngin::log_display_render_first_complete();
    }

    run_display_power_discovery(info);
    Timer::after(EmbassyDuration::from_millis(25)).await;
    crate::log!("intel: display discovery follow-up probe after proof-bringup\n");
    log_display_power_probe(info);

    if owned_triangle_proof_mode_active() {
        run_owned_triangle_proof_loop().await;
    }
}
