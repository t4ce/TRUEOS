use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use spin::Mutex;

use super::intel::{IntelDeviceInfo, mmio_read32, mmio_write32};
use super::intel_guc;

pub(crate) mod regs {
    pub const PWR_WELL_CTL: usize = 0x45400;
    pub const PWR_WELL_CTL2: usize = 0x45404;
    pub const DC_STATE_EN: usize = 0x45504;
    pub const DC_STATE_DEBUG: usize = 0x45520;
    pub const BXT_DE_PLL_ENABLE: usize = 0x46070;
    pub const PORT_HOTPLUG_EN: usize = 0x61110;
    pub const GT_DISP_PWRON: usize = 0x138090;
    pub const PIPE_A_SRC: usize = 0x7001C;
    pub const PIPE_B_SRC: usize = 0x7101C;
    pub const PIPE_C_SRC: usize = 0x7201C;
    pub const PIPE_D_SRC: usize = 0x7301C;
    pub const TRANS_A_DDI_FUNC_CTL: usize = 0x60400;
    pub const TRANS_B_DDI_FUNC_CTL: usize = 0x61400;
    pub const TRANS_C_DDI_FUNC_CTL: usize = 0x62400;
    pub const TRANS_D_DDI_FUNC_CTL: usize = 0x63400;
    pub const UNI_PLANE_BASE: usize = 0x70180;
    pub const UNI_PLANE_PIPE_STRIDE: usize = 0x1000;
    pub const UNI_PLANE_SLOT_STRIDE: usize = 0x100;
    pub const UNI_PLANE_STRIDE_OFF: usize = 0x08;
    pub const UNI_PLANE_SURF_OFF: usize = 0x1C;
    pub const UNI_PLANE_SURFLIVE_OFF: usize = 0x2C;
    pub const CURSOR_BASE: usize = 0x70080;
    pub const CURSOR_PIPE_STRIDE: usize = 0x1000;
    pub const CURSOR_CTL_OFF: usize = 0x00;
    pub const CURSOR_SURF_OFF: usize = 0x04;

    pub const GT_DISP_PWRON_REQ: u32 = 1 << 0;
    pub const TRANS_DDI_FUNC_ENABLE: u32 = 1 << 31;
    pub const PLANE_CTL_ENABLE: u32 = 1 << 31;
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct DisplayRegisterStub {
    pub name: &'static str,
    pub offset: usize,
}

pub(crate) const EARLY_DISPLAY_REGS: [DisplayRegisterStub; 7] = [
    DisplayRegisterStub {
        name: "PWR_WELL_CTL",
        offset: regs::PWR_WELL_CTL,
    },
    DisplayRegisterStub {
        name: "PWR_WELL_CTL2",
        offset: regs::PWR_WELL_CTL2,
    },
    DisplayRegisterStub {
        name: "DC_STATE_EN",
        offset: regs::DC_STATE_EN,
    },
    DisplayRegisterStub {
        name: "DC_STATE_DEBUG",
        offset: regs::DC_STATE_DEBUG,
    },
    DisplayRegisterStub {
        name: "BXT_DE_PLL_ENABLE",
        offset: regs::BXT_DE_PLL_ENABLE,
    },
    DisplayRegisterStub {
        name: "PORT_HOTPLUG_EN",
        offset: regs::PORT_HOTPLUG_EN,
    },
    DisplayRegisterStub {
        name: "GT_DISP_PWRON",
        offset: regs::GT_DISP_PWRON,
    },
];

#[derive(Copy, Clone, Debug)]
pub(crate) struct EarlyDisplaySnapshot {
    pub pwr_well_ctl: u32,
    pub pwr_well_ctl2: u32,
    pub dc_state_en: u32,
    pub dc_state_debug: u32,
    pub de_pll_enable: u32,
    pub hotplug_en: u32,
    pub gt_disp_pwron: u32,
}

impl EarlyDisplaySnapshot {
    #[inline]
    pub const fn power_well_mask(self) -> u32 {
        self.pwr_well_ctl | self.pwr_well_ctl2
    }

    #[inline]
    pub const fn has_visible_display_power(self) -> bool {
        self.power_well_mask() != 0 || self.gt_disp_pwron != 0
    }

    #[inline]
    pub const fn next_gt_disp_pwron_request(self) -> u32 {
        self.gt_disp_pwron | regs::GT_DISP_PWRON_REQ
    }

    #[inline]
    pub const fn dc_state_blocking_mask(self) -> u32 {
        self.dc_state_en
    }

    #[inline]
    pub const fn pll_seeded(self) -> bool {
        self.de_pll_enable != 0 && self.de_pll_enable != u32::MAX
    }

    #[inline]
    pub const fn hotplug_configured(self) -> bool {
        self.hotplug_en != 0 && self.hotplug_en != u32::MAX
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct DisplayPowerRequestPlan {
    pub reg: DisplayRegisterStub,
    pub before: u32,
    pub request: u32,
    pub request_mask: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum DisplayPowerState {
    Absent,
    Visible,
    VisibleIgnoredRequest,
    SoftwareLatched,
}

impl DisplayPowerState {
    #[inline]
    pub(crate) fn classify(visible: bool, latched: bool, request_class: &str) -> Self {
        if latched {
            Self::SoftwareLatched
        } else if visible && matches!(request_class, "ignored") {
            Self::VisibleIgnoredRequest
        } else if visible {
            Self::Visible
        } else {
            Self::Absent
        }
    }

    #[inline]
    pub(crate) const fn has_effective_power(self) -> bool {
        !matches!(self, Self::Absent)
    }

    #[inline]
    pub(crate) const fn is_software_latched(self) -> bool {
        matches!(self, Self::SoftwareLatched)
    }

    #[inline]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::Visible => "visible",
            Self::VisibleIgnoredRequest => "visible-ignored-request",
            Self::SoftwareLatched => "software-latched",
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct DisplayPowerSmokeResult {
    pub plan: DisplayPowerRequestPlan,
    pub wrote: bool,
    pub readback: u32,
    pub latched: bool,
    pub request_class: &'static str,
    pub power_state: DisplayPowerState,
}

#[inline]
pub(crate) fn capture_early_display_snapshot(info: IntelDeviceInfo) -> EarlyDisplaySnapshot {
    EarlyDisplaySnapshot {
        pwr_well_ctl: mmio_read32(info, regs::PWR_WELL_CTL),
        pwr_well_ctl2: mmio_read32(info, regs::PWR_WELL_CTL2),
        dc_state_en: mmio_read32(info, regs::DC_STATE_EN),
        dc_state_debug: mmio_read32(info, regs::DC_STATE_DEBUG),
        de_pll_enable: mmio_read32(info, regs::BXT_DE_PLL_ENABLE),
        hotplug_en: mmio_read32(info, regs::PORT_HOTPLUG_EN),
        gt_disp_pwron: mmio_read32(info, regs::GT_DISP_PWRON),
    }
}

#[inline]
pub(crate) const fn build_display_power_request_plan(
    snapshot: EarlyDisplaySnapshot,
) -> DisplayPowerRequestPlan {
    DisplayPowerRequestPlan {
        reg: DisplayRegisterStub {
            name: "GT_DISP_PWRON",
            offset: regs::GT_DISP_PWRON,
        },
        before: snapshot.gt_disp_pwron,
        request: snapshot.next_gt_disp_pwron_request(),
        request_mask: regs::GT_DISP_PWRON_REQ,
    }
}

pub(crate) fn request_display_power_smoke(info: IntelDeviceInfo) -> DisplayPowerSmokeResult {
    let snapshot = capture_early_display_snapshot(info);
    let plan = build_display_power_request_plan(snapshot);
    let wrote = mmio_write32(info, plan.reg.offset, plan.request);
    let readback = mmio_read32(info, plan.reg.offset);
    let latched = wrote && (readback & plan.request_mask) != 0;
    let request_class = if !wrote {
        "write-failed"
    } else if readback == plan.before {
        "ignored"
    } else if latched {
        "latched"
    } else {
        "pending"
    };
    let power_state =
        DisplayPowerState::classify(snapshot.has_visible_display_power(), latched, request_class);

    crate::log!(
        "intel/display-ngin: power-smoke register={} orig=0x{:08X} req=0x{:08X} rb=0x{:08X} latched={} class={} state={}\n",
        plan.reg.name,
        plan.before,
        plan.request,
        readback,
        latched as u8,
        request_class,
        power_state.as_str()
    );

    DisplayPowerSmokeResult {
        plan,
        wrote,
        readback,
        latched,
        request_class,
        power_state,
    }
}

pub(crate) fn log_early_display_stub(info: IntelDeviceInfo, label: &str) -> EarlyDisplaySnapshot {
    let snapshot = capture_early_display_snapshot(info);
    let plan = build_display_power_request_plan(snapshot);

    crate::log!(
        "intel/display-ngin: early label={} power_mask=0x{:08X} dc_state_en=0x{:08X} dc_state_debug=0x{:08X} pll=0x{:08X} hotplug=0x{:08X} gt_disp_pwron=0x{:08X} power_visible={} pll_seeded={} hotplug_configured={} next_gt_disp_pwron=0x{:08X}\n",
        label,
        snapshot.power_well_mask(),
        snapshot.dc_state_en,
        snapshot.dc_state_debug,
        snapshot.de_pll_enable,
        snapshot.hotplug_en,
        snapshot.gt_disp_pwron,
        snapshot.has_visible_display_power() as u8,
        snapshot.pll_seeded() as u8,
        snapshot.hotplug_configured() as u8,
        plan.request
    );

    if crate::logflag::INTEL_GFX_DEBUG_LOGFLAG {
        for reg in EARLY_DISPLAY_REGS {
            crate::log!(
                "intel/display-ngin: reg label={} name={} off=0x{:05X} value=0x{:08X}\n",
                label,
                reg.name,
                reg.offset,
                mmio_read32(info, reg.offset)
            );
        }
    }

    snapshot
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum DisplayPipeId {
    PipeA,
    PipeB,
    PipeC,
    PipeD,
}

impl DisplayPipeId {
    const fn as_str(self) -> &'static str {
        match self {
            Self::PipeA => "pipe-a",
            Self::PipeB => "pipe-b",
            Self::PipeC => "pipe-c",
            Self::PipeD => "pipe-d",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum DisplayProvisioning {
    Kickoff,
    Reserve,
    Disabled,
}

impl DisplayProvisioning {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Kickoff => "kickoff",
            Self::Reserve => "reserve",
            Self::Disabled => "disabled",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum DisplayWorkloadKind {
    PrimaryPresent,
    PlaneRebind,
    ModesetBootstrap,
    HotplugSense,
    Snapshot,
}

impl DisplayWorkloadKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::PrimaryPresent => "primary-present",
            Self::PlaneRebind => "plane-rebind",
            Self::ModesetBootstrap => "modeset-bootstrap",
            Self::HotplugSense => "hotplug-sense",
            Self::Snapshot => "snapshot",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum DisplayStagingEngine {
    None,
    Copy,
    Render,
}

impl DisplayStagingEngine {
    const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Copy => "copy",
            Self::Render => "render",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum DisplayCommitPath {
    ObserveOnly,
    PrimaryPlaneMmio,
    FastsetFlip,
    FullModeset,
}

impl DisplayCommitPath {
    const fn as_str(self) -> &'static str {
        match self {
            Self::ObserveOnly => "observe-only",
            Self::PrimaryPlaneMmio => "primary-plane-mmio",
            Self::FastsetFlip => "fastset-flip",
            Self::FullModeset => "full-modeset",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum DisplayAsyncModel {
    Inline,
    GuCReserved,
    Deferred,
}

impl DisplayAsyncModel {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Inline => "inline",
            Self::GuCReserved => "guc-reserved",
            Self::Deferred => "deferred",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum DisplayKickoffStage {
    Discovery,
    PowerArmed,
    TopologyPlanned,
    PlaneBinding,
    PresentApi,
    ModesetSkeleton,
}

impl DisplayKickoffStage {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Discovery => "discovery",
            Self::PowerArmed => "power-armed",
            Self::TopologyPlanned => "topology-planned",
            Self::PlaneBinding => "plane-binding",
            Self::PresentApi => "present-api",
            Self::ModesetSkeleton => "modeset-skeleton",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum DisplayPixelFormat {
    Xrgb8888,
    Argb8888,
    Unknown,
}

impl DisplayPixelFormat {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Xrgb8888 => "xrgb8888",
            Self::Argb8888 => "argb8888",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum DisplaySurfaceLayout {
    Linear,
    Tile4,
    Unknown,
}

impl DisplaySurfaceLayout {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Linear => "linear",
            Self::Tile4 => "tile4",
            Self::Unknown => "unknown",
        }
    }
}

const MAX_DISPLAY_PIPES: usize = 4;
const MAX_DISPLAY_API_ROUTES: usize = 5;
const MAX_DISPLAY_OBSERVE_REGS: usize = 6;

const DISPLAY_PIPE_GPU_ADDR_BASE: u64 = 0x0200_0000;
const DISPLAY_PIPE_GPU_ADDR_STRIDE: u64 = 0x0080_0000;
const DISPLAY_PIPE_STAGING_GPU_OFFSET: u64 = 0x0000_0000;
const DISPLAY_PIPE_SHADOW_GPU_OFFSET: u64 = 0x0040_0000;
const DISPLAY_PIPE_CURSOR_GPU_OFFSET: u64 = 0x0060_0000;
const DISPLAY_DEFAULT_WIDTH: u32 = 1280;
const DISPLAY_DEFAULT_HEIGHT: u32 = 720;
const DISPLAY_DEFAULT_BPP: u32 = 32;
const DISPLAY_DEFAULT_ALIGNMENT: u32 = 4096;

const DISPLAY_PIPES: [DisplayPipeId; MAX_DISPLAY_PIPES] = [
    DisplayPipeId::PipeA,
    DisplayPipeId::PipeB,
    DisplayPipeId::PipeC,
    DisplayPipeId::PipeD,
];

const EMPTY_DISPLAY_REG: DisplayRegisterStub = DisplayRegisterStub {
    name: "unused",
    offset: 0,
};

#[derive(Copy, Clone, Debug)]
struct PipeRegisterLayout {
    name: &'static str,
    pipe_src_off: usize,
    trans_ddi_func_ctl_off: usize,
    plane_ctl_off: usize,
    plane_stride_off: usize,
    plane_surf_off: usize,
    plane_surf_live_off: usize,
}

#[derive(Copy, Clone, Debug)]
struct BootFramebufferHint {
    width: u32,
    height: u32,
    pitch_bytes: u32,
    bpp: u32,
    format: DisplayPixelFormat,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct DisplayPipeDescriptor {
    pub id: DisplayPipeId,
    pub name: &'static str,
    pub pipe_src_off: usize,
    pub trans_ddi_func_ctl_off: usize,
    pub plane_ctl_off: usize,
    pub plane_stride_off: usize,
    pub plane_surf_off: usize,
    pub plane_surf_live_off: usize,
    pub provisioning: DisplayProvisioning,
    pub default_workload: DisplayWorkloadKind,
}

impl DisplayPipeDescriptor {
    const fn unused() -> Self {
        Self {
            id: DisplayPipeId::PipeA,
            name: "unused",
            pipe_src_off: 0,
            trans_ddi_func_ctl_off: 0,
            plane_ctl_off: 0,
            plane_stride_off: 0,
            plane_surf_off: 0,
            plane_surf_live_off: 0,
            provisioning: DisplayProvisioning::Disabled,
            default_workload: DisplayWorkloadKind::Snapshot,
        }
    }

    const fn supports_workload(self, workload: DisplayWorkloadKind) -> bool {
        match workload {
            DisplayWorkloadKind::PrimaryPresent | DisplayWorkloadKind::PlaneRebind => {
                match self.provisioning {
                    DisplayProvisioning::Disabled => false,
                    DisplayProvisioning::Kickoff | DisplayProvisioning::Reserve => true,
                }
            }
            DisplayWorkloadKind::ModesetBootstrap
            | DisplayWorkloadKind::HotplugSense
            | DisplayWorkloadKind::Snapshot => true,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct DisplayRuntimeSnapshot {
    pub name: &'static str,
    pub pipe: DisplayPipeId,
    pub observed: bool,
    pub active_scanout: bool,
    pub transcoder_enabled: bool,
    pub plane_enabled: bool,
    pub pipe_src: u32,
    pub width: u32,
    pub height: u32,
    pub trans_ddi_func_ctl: u32,
    pub plane_ctl: u32,
    pub plane_stride: u32,
    pub plane_surf: u32,
    pub plane_surf_live: u32,
}

impl DisplayRuntimeSnapshot {
    const fn empty() -> Self {
        Self {
            name: "unused",
            pipe: DisplayPipeId::PipeA,
            observed: false,
            active_scanout: false,
            transcoder_enabled: false,
            plane_enabled: false,
            pipe_src: 0,
            width: 0,
            height: 0,
            trans_ddi_func_ctl: 0,
            plane_ctl: 0,
            plane_stride: 0,
            plane_surf: 0,
            plane_surf_live: 0,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct DisplayGpuWindowLayout {
    pub staging_gpu_addr: u64,
    pub shadow_state_gpu_addr: u64,
    pub cursor_gpu_addr: u64,
}

impl DisplayGpuWindowLayout {
    const fn empty() -> Self {
        Self {
            staging_gpu_addr: 0,
            shadow_state_gpu_addr: 0,
            cursor_gpu_addr: 0,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct DisplaySurfacePlan {
    pub width: u32,
    pub height: u32,
    pub pitch_bytes: u32,
    pub format: DisplayPixelFormat,
    pub layout: DisplaySurfaceLayout,
    pub alignment_bytes: u32,
    pub surface_bytes: u64,
    pub windows: DisplayGpuWindowLayout,
}

impl DisplaySurfacePlan {
    const fn empty() -> Self {
        Self {
            width: 0,
            height: 0,
            pitch_bytes: 0,
            format: DisplayPixelFormat::Unknown,
            layout: DisplaySurfaceLayout::Unknown,
            alignment_bytes: 0,
            surface_bytes: 0,
            windows: DisplayGpuWindowLayout::empty(),
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct DisplayProgrammingPlan {
    pub staging: DisplayStagingEngine,
    pub commit: DisplayCommitPath,
    pub async_model: DisplayAsyncModel,
    pub completion_readback_reg: usize,
}

impl DisplayProgrammingPlan {
    const fn empty() -> Self {
        Self {
            staging: DisplayStagingEngine::None,
            commit: DisplayCommitPath::ObserveOnly,
            async_model: DisplayAsyncModel::Deferred,
            completion_readback_reg: 0,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct DisplayObservabilityPlan {
    pub label: &'static str,
    pub reg_count: usize,
    pub regs: [DisplayRegisterStub; MAX_DISPLAY_OBSERVE_REGS],
}

impl DisplayObservabilityPlan {
    const fn empty() -> Self {
        Self {
            label: "unused",
            reg_count: 0,
            regs: [EMPTY_DISPLAY_REG; MAX_DISPLAY_OBSERVE_REGS],
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct DisplayPipePlan {
    pub descriptor: DisplayPipeDescriptor,
    pub surface: DisplaySurfacePlan,
    pub programming: DisplayProgrammingPlan,
    pub observability: DisplayObservabilityPlan,
    pub next_stage: DisplayKickoffStage,
}

impl DisplayPipePlan {
    const fn empty() -> Self {
        Self {
            descriptor: DisplayPipeDescriptor::unused(),
            surface: DisplaySurfacePlan::empty(),
            programming: DisplayProgrammingPlan::empty(),
            observability: DisplayObservabilityPlan::empty(),
            next_stage: DisplayKickoffStage::Discovery,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct DisplayTopology {
    pub sku_name: &'static str,
    pub active_pipe_count: usize,
    pub planned_pipe_count: usize,
    pub routed_pipe_count: usize,
    pub pipes: [DisplayPipeDescriptor; MAX_DISPLAY_PIPES],
    pub default_present_pipe: Option<DisplayPipeId>,
}

impl DisplayTopology {
    const fn empty() -> Self {
        Self {
            sku_name: "uninitialized",
            active_pipe_count: 0,
            planned_pipe_count: 0,
            routed_pipe_count: 0,
            pipes: [DisplayPipeDescriptor::unused(); MAX_DISPLAY_PIPES],
            default_present_pipe: None,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct DisplayApiRoute {
    pub name: &'static str,
    pub workload: DisplayWorkloadKind,
    pub preferred_pipe: Option<DisplayPipeId>,
    pub staging: DisplayStagingEngine,
    pub commit: DisplayCommitPath,
    pub async_model: DisplayAsyncModel,
    pub summary: &'static str,
}

impl DisplayApiRoute {
    const fn empty() -> Self {
        Self {
            name: "unused",
            workload: DisplayWorkloadKind::Snapshot,
            preferred_pipe: None,
            staging: DisplayStagingEngine::None,
            commit: DisplayCommitPath::ObserveOnly,
            async_model: DisplayAsyncModel::Deferred,
            summary: "",
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct DisplayApiShape {
    pub route_count: usize,
    pub routes: [DisplayApiRoute; MAX_DISPLAY_API_ROUTES],
}

impl DisplayApiShape {
    const fn empty() -> Self {
        Self {
            route_count: 0,
            routes: [DisplayApiRoute::empty(); MAX_DISPLAY_API_ROUTES],
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct DisplayKickoffState {
    pub early: EarlyDisplaySnapshot,
    pub power_request: DisplayPowerRequestPlan,
    pub power_state: DisplayPowerState,
    pub power_latched: bool,
    pub topology: DisplayTopology,
    pub runtime_count: usize,
    pub runtimes: [DisplayRuntimeSnapshot; MAX_DISPLAY_PIPES],
    pub plan_count: usize,
    pub plans: [DisplayPipePlan; MAX_DISPLAY_PIPES],
    pub api: DisplayApiShape,
    pub guc_ready: bool,
    pub stage: DisplayKickoffStage,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct DisplayWorkloadDraft {
    pub descriptor: DisplayPipeDescriptor,
    pub surface: DisplaySurfacePlan,
    pub programming: DisplayProgrammingPlan,
    pub observability: DisplayObservabilityPlan,
    pub workload: DisplayWorkloadKind,
    pub next_stage: DisplayKickoffStage,
}

static DISPLAY_KICKOFF_RAN: AtomicBool = AtomicBool::new(false);
static DISPLAY_KICKOFF_STATE: Mutex<Option<DisplayKickoffState>> = Mutex::new(None);
static PRIMARY_PRESENT_VISIBLE_SURFACE_SLOT: AtomicU8 = AtomicU8::new(0);

const PRIMARY_PRESENT_SLOT_STAGING: u8 = 0;
const PRIMARY_PRESENT_SLOT_SHADOW: u8 = 1;

fn pipe_layout(pipe: DisplayPipeId) -> PipeRegisterLayout {
    let pipe_slot = match pipe {
        DisplayPipeId::PipeA => 0usize,
        DisplayPipeId::PipeB => 1,
        DisplayPipeId::PipeC => 2,
        DisplayPipeId::PipeD => 3,
    };
    let plane_base = regs::UNI_PLANE_BASE
        + pipe_slot.saturating_mul(regs::UNI_PLANE_PIPE_STRIDE)
        + 0usize.saturating_mul(regs::UNI_PLANE_SLOT_STRIDE);

    match pipe {
        DisplayPipeId::PipeA => PipeRegisterLayout {
            name: "pipe-a",
            pipe_src_off: regs::PIPE_A_SRC,
            trans_ddi_func_ctl_off: regs::TRANS_A_DDI_FUNC_CTL,
            plane_ctl_off: plane_base,
            plane_stride_off: plane_base + regs::UNI_PLANE_STRIDE_OFF,
            plane_surf_off: plane_base + regs::UNI_PLANE_SURF_OFF,
            plane_surf_live_off: plane_base + regs::UNI_PLANE_SURFLIVE_OFF,
        },
        DisplayPipeId::PipeB => PipeRegisterLayout {
            name: "pipe-b",
            pipe_src_off: regs::PIPE_B_SRC,
            trans_ddi_func_ctl_off: regs::TRANS_B_DDI_FUNC_CTL,
            plane_ctl_off: plane_base,
            plane_stride_off: plane_base + regs::UNI_PLANE_STRIDE_OFF,
            plane_surf_off: plane_base + regs::UNI_PLANE_SURF_OFF,
            plane_surf_live_off: plane_base + regs::UNI_PLANE_SURFLIVE_OFF,
        },
        DisplayPipeId::PipeC => PipeRegisterLayout {
            name: "pipe-c",
            pipe_src_off: regs::PIPE_C_SRC,
            trans_ddi_func_ctl_off: regs::TRANS_C_DDI_FUNC_CTL,
            plane_ctl_off: plane_base,
            plane_stride_off: plane_base + regs::UNI_PLANE_STRIDE_OFF,
            plane_surf_off: plane_base + regs::UNI_PLANE_SURF_OFF,
            plane_surf_live_off: plane_base + regs::UNI_PLANE_SURFLIVE_OFF,
        },
        DisplayPipeId::PipeD => PipeRegisterLayout {
            name: "pipe-d",
            pipe_src_off: regs::PIPE_D_SRC,
            trans_ddi_func_ctl_off: regs::TRANS_D_DDI_FUNC_CTL,
            plane_ctl_off: plane_base,
            plane_stride_off: plane_base + regs::UNI_PLANE_STRIDE_OFF,
            plane_surf_off: plane_base + regs::UNI_PLANE_SURF_OFF,
            plane_surf_live_off: plane_base + regs::UNI_PLANE_SURFLIVE_OFF,
        },
    }
}

fn boot_framebuffer_hint() -> Option<BootFramebufferHint> {
    use ::limine::framebuffer::MemoryModel;

    let fb = crate::limine::framebuffer_response()?
        .framebuffers()
        .next()?;
    if fb.memory_model() != MemoryModel::RGB {
        return None;
    }

    let bpp = fb.bpp() as u32;
    let format = match bpp {
        32 => DisplayPixelFormat::Xrgb8888,
        _ => DisplayPixelFormat::Unknown,
    };

    Some(BootFramebufferHint {
        width: fb.width() as u32,
        height: fb.height() as u32,
        pitch_bytes: fb.pitch() as u32,
        bpp,
        format,
    })
}

fn decode_pipe_src(value: u32) -> Option<(u32, u32)> {
    if value == 0 || value == u32::MAX {
        return None;
    }

    let width = (value & 0xFFFF).saturating_add(1);
    let height = ((value >> 16) & 0xFFFF).saturating_add(1);
    if !(320..=8192).contains(&width) || !(200..=4320).contains(&height) {
        return None;
    }
    Some((width, height))
}

fn plausible_stride(value: u32) -> bool {
    value != 0
        && value != u32::MAX
        && (256..=0x20_000).contains(&(value as usize))
        && (value & 63) == 0
}

fn plausible_surface(value: u32) -> bool {
    value != 0 && value != u32::MAX
}

fn transcoder_ddi_enabled(value: u32) -> bool {
    (value & regs::TRANS_DDI_FUNC_ENABLE) != 0
}

fn capture_pipe_runtime(info: IntelDeviceInfo, pipe: DisplayPipeId) -> DisplayRuntimeSnapshot {
    let layout = pipe_layout(pipe);
    let pipe_src = mmio_read32(info, layout.pipe_src_off);
    let trans_ddi_func_ctl = mmio_read32(info, layout.trans_ddi_func_ctl_off);
    let plane_ctl = mmio_read32(info, layout.plane_ctl_off);
    let plane_stride = mmio_read32(info, layout.plane_stride_off);
    let plane_surf = mmio_read32(info, layout.plane_surf_off);
    let plane_surf_live = mmio_read32(info, layout.plane_surf_live_off);
    let (width, height) = decode_pipe_src(pipe_src).unwrap_or((0, 0));
    let transcoder_enabled = transcoder_ddi_enabled(trans_ddi_func_ctl);
    let plane_enabled = (plane_ctl & regs::PLANE_CTL_ENABLE) != 0;
    let active_scanout = plane_enabled
        || width != 0
        || height != 0
        || plausible_surface(plane_surf)
        || plausible_surface(plane_surf_live)
        || transcoder_enabled;
    let observed = active_scanout
        || pipe_src != 0
        || trans_ddi_func_ctl != 0
        || plane_ctl != 0
        || plane_stride != 0
        || plane_surf != 0
        || plane_surf_live != 0;

    DisplayRuntimeSnapshot {
        name: layout.name,
        pipe,
        observed,
        active_scanout,
        transcoder_enabled,
        plane_enabled,
        pipe_src,
        width,
        height,
        trans_ddi_func_ctl,
        plane_ctl,
        plane_stride,
        plane_surf,
        plane_surf_live,
    }
}

fn default_present_pipe(
    runtimes: &[DisplayRuntimeSnapshot; MAX_DISPLAY_PIPES],
) -> Option<DisplayPipeId> {
    let mut idx = 0usize;
    while idx < MAX_DISPLAY_PIPES {
        if runtimes[idx].active_scanout {
            return Some(runtimes[idx].pipe);
        }
        idx += 1;
    }

    let mut idx = 0usize;
    while idx < MAX_DISPLAY_PIPES {
        if runtimes[idx].observed {
            return Some(runtimes[idx].pipe);
        }
        idx += 1;
    }

    Some(DisplayPipeId::PipeA)
}

fn build_pipe_descriptor(
    pipe: DisplayPipeId,
    runtime: DisplayRuntimeSnapshot,
    is_default_present: bool,
) -> DisplayPipeDescriptor {
    let layout = pipe_layout(pipe);
    let provisioning = if runtime.active_scanout || is_default_present {
        DisplayProvisioning::Kickoff
    } else {
        DisplayProvisioning::Reserve
    };

    DisplayPipeDescriptor {
        id: pipe,
        name: layout.name,
        pipe_src_off: layout.pipe_src_off,
        trans_ddi_func_ctl_off: layout.trans_ddi_func_ctl_off,
        plane_ctl_off: layout.plane_ctl_off,
        plane_stride_off: layout.plane_stride_off,
        plane_surf_off: layout.plane_surf_off,
        plane_surf_live_off: layout.plane_surf_live_off,
        provisioning,
        default_workload: if runtime.active_scanout {
            DisplayWorkloadKind::PrimaryPresent
        } else {
            DisplayWorkloadKind::ModesetBootstrap
        },
    }
}

fn pipe_gpu_window(slot: usize) -> DisplayGpuWindowLayout {
    let base =
        DISPLAY_PIPE_GPU_ADDR_BASE + (slot as u64).saturating_mul(DISPLAY_PIPE_GPU_ADDR_STRIDE);
    DisplayGpuWindowLayout {
        staging_gpu_addr: base + DISPLAY_PIPE_STAGING_GPU_OFFSET,
        shadow_state_gpu_addr: base + DISPLAY_PIPE_SHADOW_GPU_OFFSET,
        cursor_gpu_addr: base + DISPLAY_PIPE_CURSOR_GPU_OFFSET,
    }
}

fn build_surface_plan(
    slot: usize,
    runtime: DisplayRuntimeSnapshot,
    fb: Option<BootFramebufferHint>,
) -> DisplaySurfacePlan {
    let width = if runtime.width != 0 {
        runtime.width
    } else {
        fb.map(|v| v.width).unwrap_or(DISPLAY_DEFAULT_WIDTH)
    };
    let height = if runtime.height != 0 {
        runtime.height
    } else {
        fb.map(|v| v.height).unwrap_or(DISPLAY_DEFAULT_HEIGHT)
    };
    let pitch_bytes = if plausible_stride(runtime.plane_stride) {
        runtime.plane_stride
    } else {
        let default_bpp = fb.map(|v| v.bpp).unwrap_or(DISPLAY_DEFAULT_BPP);
        fb.map(|v| v.pitch_bytes)
            .unwrap_or(width.saturating_mul(default_bpp.saturating_div(8)))
    };
    let format = fb.map(|v| v.format).unwrap_or(DisplayPixelFormat::Xrgb8888);
    let layout =
        if plausible_surface(runtime.plane_surf) || plausible_surface(runtime.plane_surf_live) {
            DisplaySurfaceLayout::Linear
        } else {
            fb.map(|_| DisplaySurfaceLayout::Linear)
                .unwrap_or(DisplaySurfaceLayout::Unknown)
        };

    DisplaySurfacePlan {
        width,
        height,
        pitch_bytes,
        format,
        layout,
        alignment_bytes: DISPLAY_DEFAULT_ALIGNMENT,
        surface_bytes: u64::from(pitch_bytes).saturating_mul(u64::from(height)),
        windows: pipe_gpu_window(slot),
    }
}

fn build_programming_plan(
    descriptor: DisplayPipeDescriptor,
    runtime: DisplayRuntimeSnapshot,
    guc_ready: bool,
) -> DisplayProgrammingPlan {
    let commit = if runtime.active_scanout {
        if runtime.plane_enabled {
            DisplayCommitPath::FastsetFlip
        } else {
            DisplayCommitPath::PrimaryPlaneMmio
        }
    } else {
        DisplayCommitPath::FullModeset
    };

    DisplayProgrammingPlan {
        staging: if descriptor.provisioning == DisplayProvisioning::Disabled {
            DisplayStagingEngine::None
        } else {
            DisplayStagingEngine::Copy
        },
        commit,
        async_model: if descriptor.provisioning == DisplayProvisioning::Disabled {
            DisplayAsyncModel::Deferred
        } else if guc_ready {
            DisplayAsyncModel::GuCReserved
        } else {
            DisplayAsyncModel::Inline
        },
        completion_readback_reg: descriptor.plane_surf_live_off,
    }
}

fn build_observability_plan(descriptor: DisplayPipeDescriptor) -> DisplayObservabilityPlan {
    let mut regs = [EMPTY_DISPLAY_REG; MAX_DISPLAY_OBSERVE_REGS];
    regs[0] = DisplayRegisterStub {
        name: "PIPE_SRC",
        offset: descriptor.pipe_src_off,
    };
    regs[1] = DisplayRegisterStub {
        name: "TRANS_DDI_FUNC_CTL",
        offset: descriptor.trans_ddi_func_ctl_off,
    };
    regs[2] = DisplayRegisterStub {
        name: "PLANE_CTL",
        offset: descriptor.plane_ctl_off,
    };
    regs[3] = DisplayRegisterStub {
        name: "PLANE_STRIDE",
        offset: descriptor.plane_stride_off,
    };
    regs[4] = DisplayRegisterStub {
        name: "PLANE_SURF",
        offset: descriptor.plane_surf_off,
    };
    regs[5] = DisplayRegisterStub {
        name: "PLANE_SURF_LIVE",
        offset: descriptor.plane_surf_live_off,
    };

    DisplayObservabilityPlan {
        label: descriptor.name,
        reg_count: regs.len(),
        regs,
    }
}

fn build_pipe_plan(
    slot: usize,
    descriptor: DisplayPipeDescriptor,
    runtime: DisplayRuntimeSnapshot,
    fb: Option<BootFramebufferHint>,
    guc_ready: bool,
    power_available: bool,
) -> DisplayPipePlan {
    let surface = build_surface_plan(slot, runtime, fb);
    let programming = build_programming_plan(descriptor, runtime, guc_ready);
    let next_stage = if !power_available && !runtime.active_scanout {
        DisplayKickoffStage::PowerArmed
    } else if runtime.active_scanout && runtime.plane_enabled {
        DisplayKickoffStage::PresentApi
    } else if descriptor.provisioning == DisplayProvisioning::Reserve {
        DisplayKickoffStage::ModesetSkeleton
    } else {
        DisplayKickoffStage::PlaneBinding
    };

    DisplayPipePlan {
        descriptor,
        surface,
        programming,
        observability: build_observability_plan(descriptor),
        next_stage,
    }
}

fn current_api_shape(topology: DisplayTopology, guc_ready: bool) -> DisplayApiShape {
    let mut api = DisplayApiShape::empty();
    let async_present = if guc_ready {
        DisplayAsyncModel::GuCReserved
    } else {
        DisplayAsyncModel::Inline
    };

    api.route_count = 5;
    api.routes[0] = DisplayApiRoute {
        name: "display.present.primary",
        workload: DisplayWorkloadKind::PrimaryPresent,
        preferred_pipe: topology.default_present_pipe,
        staging: DisplayStagingEngine::Copy,
        commit: DisplayCommitPath::FastsetFlip,
        async_model: async_present,
        summary: "stage a boot-time scanout surface through the proven BCS path and arm a primary-plane flip",
    };
    api.routes[1] = DisplayApiRoute {
        name: "display.plane.rebind",
        workload: DisplayWorkloadKind::PlaneRebind,
        preferred_pipe: topology.default_present_pipe,
        staging: DisplayStagingEngine::Copy,
        commit: DisplayCommitPath::PrimaryPlaneMmio,
        async_model: async_present,
        summary: "reuse a powered pipe/transcoder pair and only retarget the primary plane surface",
    };
    api.routes[2] = DisplayApiRoute {
        name: "display.modeset.bootstrap",
        workload: DisplayWorkloadKind::ModesetBootstrap,
        preferred_pipe: topology.default_present_pipe,
        staging: DisplayStagingEngine::None,
        commit: DisplayCommitPath::FullModeset,
        async_model: DisplayAsyncModel::Inline,
        summary: "seed a cold pipe/transcoder/plane sequence once link training is modeled",
    };
    api.routes[3] = DisplayApiRoute {
        name: "display.connector.observe",
        workload: DisplayWorkloadKind::HotplugSense,
        preferred_pipe: None,
        staging: DisplayStagingEngine::None,
        commit: DisplayCommitPath::ObserveOnly,
        async_model: DisplayAsyncModel::Inline,
        summary: "keep connector bring-up read-only while harvesting hotplug and routing state",
    };
    api.routes[4] = DisplayApiRoute {
        name: "display.debug.snapshot",
        workload: DisplayWorkloadKind::Snapshot,
        preferred_pipe: None,
        staging: DisplayStagingEngine::None,
        commit: DisplayCommitPath::ObserveOnly,
        async_model: DisplayAsyncModel::Inline,
        summary: "snapshot the planned display planes, MMIO touchpoints, and present defaults",
    };
    api
}

fn build_kickoff_state(
    info: IntelDeviceInfo,
    power_state: DisplayPowerState,
) -> DisplayKickoffState {
    let early = capture_early_display_snapshot(info);
    let power_request = build_display_power_request_plan(early);
    let power_latched = power_state.is_software_latched();
    let power_available = power_state.has_effective_power() || early.has_visible_display_power();
    let guc_ready = intel_guc::ready();
    let fb = boot_framebuffer_hint();
    let mut runtimes = [DisplayRuntimeSnapshot::empty(); MAX_DISPLAY_PIPES];

    let mut idx = 0usize;
    while idx < MAX_DISPLAY_PIPES {
        runtimes[idx] = capture_pipe_runtime(info, DISPLAY_PIPES[idx]);
        idx += 1;
    }

    let default_present_pipe = default_present_pipe(&runtimes);
    let mut topology = DisplayTopology::empty();
    topology.sku_name = "xelp-display-kickoff";
    topology.planned_pipe_count = MAX_DISPLAY_PIPES;
    topology.default_present_pipe = default_present_pipe;

    let mut plans = [DisplayPipePlan::empty(); MAX_DISPLAY_PIPES];
    let mut active_pipe_count = 0usize;
    let mut routed_pipe_count = 0usize;
    let mut idx = 0usize;
    while idx < MAX_DISPLAY_PIPES {
        let runtime = runtimes[idx];
        if runtime.active_scanout {
            active_pipe_count += 1;
        }
        if runtime.transcoder_enabled {
            routed_pipe_count += 1;
        }

        let pipe = DISPLAY_PIPES[idx];
        let descriptor = build_pipe_descriptor(
            pipe,
            runtime,
            default_present_pipe.map(|v| v == pipe).unwrap_or(false),
        );
        topology.pipes[idx] = descriptor;
        plans[idx] = build_pipe_plan(idx, descriptor, runtime, fb, guc_ready, power_available);
        idx += 1;
    }

    topology.active_pipe_count = active_pipe_count;
    topology.routed_pipe_count = routed_pipe_count;

    let stage = if !power_available {
        DisplayKickoffStage::PowerArmed
    } else if active_pipe_count != 0 {
        DisplayKickoffStage::PresentApi
    } else if early.pll_seeded() || early.hotplug_configured() {
        DisplayKickoffStage::PlaneBinding
    } else {
        DisplayKickoffStage::TopologyPlanned
    };

    DisplayKickoffState {
        early,
        power_request,
        power_state,
        power_latched,
        topology,
        runtime_count: MAX_DISPLAY_PIPES,
        runtimes,
        plan_count: MAX_DISPLAY_PIPES,
        plans,
        api: current_api_shape(topology, guc_ready),
        guc_ready,
        stage,
    }
}

fn stored_kickoff_state() -> Option<DisplayKickoffState> {
    *DISPLAY_KICKOFF_STATE.lock()
}

fn select_pipe_for_workload(
    topology: DisplayTopology,
    workload: DisplayWorkloadKind,
) -> Option<(usize, DisplayPipeDescriptor)> {
    let mut kickoff = None;
    let mut reserve = None;
    let mut idx = 0usize;
    while idx < topology.planned_pipe_count {
        let descriptor = topology.pipes[idx];
        if !descriptor.supports_workload(workload) {
            idx += 1;
            continue;
        }

        if workload != DisplayWorkloadKind::ModesetBootstrap
            && topology
                .default_present_pipe
                .map(|pipe| pipe == descriptor.id)
                .unwrap_or(false)
        {
            return Some((idx, descriptor));
        }
        if kickoff.is_none() && descriptor.provisioning == DisplayProvisioning::Kickoff {
            kickoff = Some((idx, descriptor));
        }
        if reserve.is_none() && descriptor.provisioning == DisplayProvisioning::Reserve {
            reserve = Some((idx, descriptor));
        }
        idx += 1;
    }

    if workload == DisplayWorkloadKind::ModesetBootstrap {
        reserve.or(kickoff)
    } else {
        kickoff.or(reserve)
    }
}

pub(crate) fn kickoff_state() -> Option<DisplayKickoffState> {
    stored_kickoff_state()
}

pub(crate) fn draft_workload(workload: DisplayWorkloadKind) -> Option<DisplayWorkloadDraft> {
    let state = stored_kickoff_state()?;
    let (slot, descriptor) = select_pipe_for_workload(state.topology, workload)?;
    let mut plan = state.plans[slot];

    match workload {
        DisplayWorkloadKind::PrimaryPresent => {
            plan.programming.commit = DisplayCommitPath::FastsetFlip;
            plan.programming.staging = DisplayStagingEngine::Copy;
        }
        DisplayWorkloadKind::PlaneRebind => {
            plan.programming.commit = DisplayCommitPath::PrimaryPlaneMmio;
            plan.programming.staging = DisplayStagingEngine::Copy;
        }
        DisplayWorkloadKind::ModesetBootstrap => {
            plan.programming.commit = DisplayCommitPath::FullModeset;
            plan.programming.staging = DisplayStagingEngine::None;
            plan.programming.async_model = DisplayAsyncModel::Inline;
        }
        DisplayWorkloadKind::HotplugSense | DisplayWorkloadKind::Snapshot => {
            plan.programming.commit = DisplayCommitPath::ObserveOnly;
            plan.programming.staging = DisplayStagingEngine::None;
            plan.programming.async_model = DisplayAsyncModel::Inline;
        }
    }

    let next_stage = match workload {
        DisplayWorkloadKind::PrimaryPresent => DisplayKickoffStage::PresentApi,
        DisplayWorkloadKind::PlaneRebind => DisplayKickoffStage::PlaneBinding,
        DisplayWorkloadKind::ModesetBootstrap => DisplayKickoffStage::ModesetSkeleton,
        DisplayWorkloadKind::HotplugSense | DisplayWorkloadKind::Snapshot => {
            DisplayKickoffStage::Discovery
        }
    };

    Some(DisplayWorkloadDraft {
        descriptor,
        surface: plan.surface,
        programming: plan.programming,
        observability: plan.observability,
        workload,
        next_stage,
    })
}

#[inline]
fn plane_stride_reg_value(pitch_bytes: u32) -> Option<u32> {
    if pitch_bytes == 0 || !pitch_bytes.is_multiple_of(64) {
        return None;
    }
    Some(pitch_bytes / 64)
}

pub(crate) fn primary_present_surface_gpu_addr() -> Option<u64> {
    let draft = draft_workload(DisplayWorkloadKind::PrimaryPresent)?;
    let windows = draft.surface.windows;
    let visible_slot = PRIMARY_PRESENT_VISIBLE_SURFACE_SLOT.load(Ordering::Acquire);
    Some(if visible_slot == PRIMARY_PRESENT_SLOT_SHADOW {
        windows.staging_gpu_addr
    } else {
        windows.shadow_state_gpu_addr
    })
}

pub(crate) fn primary_present_shadow_surface_gpu_addr() -> Option<u64> {
    let draft = draft_workload(DisplayWorkloadKind::PrimaryPresent)?;
    Some(draft.surface.windows.shadow_state_gpu_addr)
}

pub(crate) fn owned_triangle_disable_non_primary_planes_pipe_a() -> bool {
    let info = match super::intel::first_claimed_device() {
        Some(v) => v,
        None => return false,
    };

    let mut changed = false;
    let pipe_slot = 0usize;
    let mut plane_slot = 1usize;
    while plane_slot < 4 {
        let plane_base = regs::UNI_PLANE_BASE
            + pipe_slot.saturating_mul(regs::UNI_PLANE_PIPE_STRIDE)
            + plane_slot.saturating_mul(regs::UNI_PLANE_SLOT_STRIDE);
        let plane_ctl_off = plane_base;
        let plane_surf_off = plane_base + regs::UNI_PLANE_SURF_OFF;
        let plane_ctl = mmio_read32(info, plane_ctl_off);
        let plane_surf = mmio_read32(info, plane_surf_off);
        if plane_ctl != 0 || plane_surf != 0 {
            let _ = mmio_write32(info, plane_ctl_off, plane_ctl & !regs::PLANE_CTL_ENABLE);
            let _ = mmio_write32(info, plane_surf_off, 0);
            changed = true;
            crate::log!(
                "intel/display-ngin: owned-proof clamp pipe=pipe-a plane={} ctl=0x{:08X}->0x{:08X} surf=0x{:08X}->0x00000000\n",
                plane_slot + 1,
                plane_ctl,
                plane_ctl & !regs::PLANE_CTL_ENABLE,
                plane_surf
            );
        }
        plane_slot += 1;
    }

    let cursor_base = regs::CURSOR_BASE + pipe_slot.saturating_mul(regs::CURSOR_PIPE_STRIDE);
    let cursor_ctl_off = cursor_base + regs::CURSOR_CTL_OFF;
    let cursor_surf_off = cursor_base + regs::CURSOR_SURF_OFF;
    let cursor_ctl = mmio_read32(info, cursor_ctl_off);
    let cursor_surf = mmio_read32(info, cursor_surf_off);
    if cursor_ctl != 0 || cursor_surf != 0 {
        let _ = mmio_write32(info, cursor_ctl_off, 0);
        let _ = mmio_write32(info, cursor_surf_off, 0);
        changed = true;
        crate::log!(
            "intel/display-ngin: owned-proof clamp pipe=pipe-a cursor ctl=0x{:08X}->0x00000000 surf=0x{:08X}->0x00000000\n",
            cursor_ctl,
            cursor_surf
        );
    }

    if !changed {
        crate::log!(
            "intel/display-ngin: owned-proof clamp pipe=pipe-a status=already-primary-only\n"
        );
    }

    true
}

fn record_primary_present_visible_surface(surface_gpu_addr: u64) {
    let Some(draft) = draft_workload(DisplayWorkloadKind::PrimaryPresent) else {
        return;
    };
    let windows = draft.surface.windows;
    if surface_gpu_addr == windows.staging_gpu_addr {
        PRIMARY_PRESENT_VISIBLE_SURFACE_SLOT.store(PRIMARY_PRESENT_SLOT_STAGING, Ordering::Release);
    } else if surface_gpu_addr == windows.shadow_state_gpu_addr {
        PRIMARY_PRESENT_VISIBLE_SURFACE_SLOT.store(PRIMARY_PRESENT_SLOT_SHADOW, Ordering::Release);
    }
}

pub(crate) fn plane_rebind_present_surface(
    surface_gpu_addr: u64,
    width: u32,
    height: u32,
    pitch_bytes: u32,
) -> bool {
    let draft = match draft_workload(DisplayWorkloadKind::PlaneRebind) {
        Some(v) => v,
        None => return false,
    };
    let info = match super::intel::first_claimed_device() {
        Some(v) => v,
        None => return false,
    };
    if width != draft.surface.width || height != draft.surface.height {
        return false;
    }
    let Some(stride_reg) = plane_stride_reg_value(pitch_bytes) else {
        return false;
    };
    let surface_reg = match u32::try_from(surface_gpu_addr) {
        Ok(v) => v,
        Err(_) => return false,
    };

    let _ = mmio_write32(info, draft.descriptor.plane_stride_off, stride_reg);
    let _ = mmio_write32(info, draft.descriptor.plane_surf_off, surface_reg);

    let mut iter = 0usize;
    let mut live = mmio_read32(info, draft.descriptor.plane_surf_live_off);
    while iter < 4096 {
        if live == surface_reg {
            record_primary_present_visible_surface(surface_gpu_addr);
            return true;
        }
        core::hint::spin_loop();
        live = mmio_read32(info, draft.descriptor.plane_surf_live_off);
        iter += 1;
    }

    let armed = mmio_read32(info, draft.descriptor.plane_surf_off);
    let success = live == surface_reg || armed == surface_reg;
    if success {
        record_primary_present_visible_surface(surface_gpu_addr);
    }
    success
}

fn log_workload_draft(label: &str, draft: DisplayWorkloadDraft) {
    crate::log!(
        "intel/display-ngin: {} pipe={} workload={} staging={} commit={} async={} surface={}x{} pitch=0x{:X} format={} layout={} next_stage={} staging_gpu=0x{:X}\n",
        label,
        draft.descriptor.id.as_str(),
        draft.workload.as_str(),
        draft.programming.staging.as_str(),
        draft.programming.commit.as_str(),
        draft.programming.async_model.as_str(),
        draft.surface.width,
        draft.surface.height,
        draft.surface.pitch_bytes,
        draft.surface.format.as_str(),
        draft.surface.layout.as_str(),
        draft.next_stage.as_str(),
        draft.surface.windows.staging_gpu_addr
    );
}

fn log_kickoff_state(info: IntelDeviceInfo, state: DisplayKickoffState) {
    crate::log!(
        "intel/display-ngin: kickoff summary sku={} stage={} guc_ready={} active={} planned={} routed={} default_pipe={} power_visible={} power_state={} power_latched={} pll_seeded={} hotplug_configured={} next_gt_disp_pwron=0x{:08X}\n",
        state.topology.sku_name,
        state.stage.as_str(),
        state.guc_ready as u8,
        state.topology.active_pipe_count,
        state.topology.planned_pipe_count,
        state.topology.routed_pipe_count,
        state
            .topology
            .default_present_pipe
            .map(DisplayPipeId::as_str)
            .unwrap_or("none"),
        state.early.has_visible_display_power() as u8,
        state.power_state.as_str(),
        state.power_latched as u8,
        state.early.pll_seeded() as u8,
        state.early.hotplug_configured() as u8,
        state.power_request.request
    );

    let mut route_idx = 0usize;
    while route_idx < state.api.route_count {
        let route = state.api.routes[route_idx];
        crate::log!(
            "intel/display-ngin: api route={} workload={} pipe={} staging={} commit={} async={} summary={}\n",
            route.name,
            route.workload.as_str(),
            route
                .preferred_pipe
                .map(DisplayPipeId::as_str)
                .unwrap_or("any"),
            route.staging.as_str(),
            route.commit.as_str(),
            route.async_model.as_str(),
            route.summary
        );
        route_idx += 1;
    }

    let mut idx = 0usize;
    while idx < state.plan_count {
        let plan = state.plans[idx];
        let runtime = state.runtimes[idx];
        crate::log!(
            "intel/display-ngin: plan pipe={} provisioning={} next_stage={} observed={} active={} trans_enabled={} plane_enabled={} pipe_src=0x{:08X} size={}x{} trans=0x{:08X} ctl=0x{:08X} stride=0x{:08X} surf=0x{:08X} surf_live=0x{:08X} staging_gpu=0x{:X} shadow_gpu=0x{:X} cursor_gpu=0x{:X} surface_bytes=0x{:X} staging={} commit={} async={}\n",
            plan.descriptor.id.as_str(),
            plan.descriptor.provisioning.as_str(),
            plan.next_stage.as_str(),
            runtime.observed as u8,
            runtime.active_scanout as u8,
            runtime.transcoder_enabled as u8,
            runtime.plane_enabled as u8,
            runtime.pipe_src,
            runtime.width,
            runtime.height,
            runtime.trans_ddi_func_ctl,
            runtime.plane_ctl,
            runtime.plane_stride,
            runtime.plane_surf,
            runtime.plane_surf_live,
            plan.surface.windows.staging_gpu_addr,
            plan.surface.windows.shadow_state_gpu_addr,
            plan.surface.windows.cursor_gpu_addr,
            plan.surface.surface_bytes,
            plan.programming.staging.as_str(),
            plan.programming.commit.as_str(),
            plan.programming.async_model.as_str()
        );

        if crate::logflag::INTEL_GFX_DEBUG_LOGFLAG {
            let mut reg_idx = 0usize;
            while reg_idx < plan.observability.reg_count {
                let reg = plan.observability.regs[reg_idx];
                crate::log!(
                    "intel/display-ngin: observe pipe={} name={} off=0x{:05X} value=0x{:08X}\n",
                    plan.descriptor.id.as_str(),
                    reg.name,
                    reg.offset,
                    mmio_read32(info, reg.offset)
                );
                reg_idx += 1;
            }
        }
        idx += 1;
    }
}

pub(crate) fn kickoff_once(info: IntelDeviceInfo, power_state: DisplayPowerState) {
    if DISPLAY_KICKOFF_RAN.swap(true, Ordering::AcqRel) {
        return;
    }

    let state = build_kickoff_state(info, power_state);
    {
        let mut slot = DISPLAY_KICKOFF_STATE.lock();
        *slot = Some(state);
    }

    log_kickoff_state(info, state);

    if let Some(draft) = draft_workload(DisplayWorkloadKind::PrimaryPresent) {
        log_workload_draft("draft-present", draft);
    }
    if let Some(draft) = draft_workload(DisplayWorkloadKind::ModesetBootstrap) {
        log_workload_draft("draft-modeset", draft);
    }
}
