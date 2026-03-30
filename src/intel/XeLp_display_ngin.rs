use super::intel::{mmio_read32, IntelDeviceInfo};

pub(crate) mod regs {
	pub const PWR_WELL_CTL: usize = 0x45400;
	pub const PWR_WELL_CTL2: usize = 0x45404;
	pub const DC_STATE_EN: usize = 0x45504;
	pub const DC_STATE_DEBUG: usize = 0x45520;
	pub const BXT_DE_PLL_ENABLE: usize = 0x46070;
	pub const PORT_HOTPLUG_EN: usize = 0x61110;
	pub const GT_DISP_PWRON: usize = 0x138090;

	pub const GT_DISP_PWRON_REQ: u32 = 1 << 0;
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
