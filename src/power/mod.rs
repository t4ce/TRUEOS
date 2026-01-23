use core::arch::x86_64::__cpuid;
use core::sync::atomic::{AtomicU8, Ordering};

use spin::Once;
use x86_64::registers::model_specific::Msr;

const IA32_MSR_PLATFORM_INFO: u32 = 0xCE;
const IA32_MSR_PERF_STATUS: u32 = 0x198;
const IA32_MSR_PERF_CTL: u32 = 0x199;
const IA32_MSR_HWP_CAPABILITIES: u32 = 0x771;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdlePolicy {
    Spin = 0,
    Halt = 1,
}

impl IdlePolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            IdlePolicy::Spin => "spin",
            IdlePolicy::Halt => "hlt",
        }
    }

    fn from_u8(v: u8) -> Self {
        match v {
            1 => IdlePolicy::Halt,
            _ => IdlePolicy::Spin,
        }
    }
}

static IDLE_POLICY: AtomicU8 = AtomicU8::new(IdlePolicy::Spin as u8);

#[derive(Clone, Copy, Debug)]
pub struct PowerCaps {
    pub vendor_intel: bool,
    pub has_msr: bool,
    pub has_eist: bool,
    pub has_hwp: bool,
    pub min_ratio: Option<u8>,
    pub max_ratio: Option<u8>,
    pub hwp_lowest: Option<u8>,
    pub hwp_highest: Option<u8>,
}

static CAPS: Once<Option<PowerCaps>> = Once::new();

pub fn init() {
    CAPS.call_once(|| detect_caps());

    if let Some(caps) = caps() {
        crate::log!(
            "POWER: intel={} msr={} eist={} hwp={} min={} max={} hwp_lowest={} hwp_highest={} idle={}\n",
            caps.vendor_intel,
            caps.has_msr,
            caps.has_eist,
            caps.has_hwp,
            opt_u8(caps.min_ratio),
            opt_u8(caps.max_ratio),
            opt_u8(caps.hwp_lowest),
            opt_u8(caps.hwp_highest),
            idle_policy().as_str(),
        );
    } else {
        crate::log!("POWER: caps unavailable\n");
    }
}

pub fn caps() -> Option<&'static PowerCaps> {
    CAPS.get().and_then(|caps| caps.as_ref())
}

pub fn idle_policy() -> IdlePolicy {
    IdlePolicy::from_u8(IDLE_POLICY.load(Ordering::Acquire))
}

pub fn set_idle_policy(policy: IdlePolicy) -> IdlePolicy {
    let prev = IDLE_POLICY.swap(policy as u8, Ordering::AcqRel);
    IdlePolicy::from_u8(prev)
}

#[inline(always)]
pub fn idle_hint() {
    match idle_policy() {
        IdlePolicy::Spin => core::hint::spin_loop(),
        IdlePolicy::Halt => x86_64::instructions::hlt(),
    }
}

pub fn current_ratio() -> Option<u8> {
    let caps = caps()?;
    if !caps.has_msr || !caps.has_eist {
        return None;
    }
    let value = unsafe { Msr::new(IA32_MSR_PERF_STATUS).read() };
    Some(((value >> 8) & 0xff) as u8)
}

pub fn set_pstate_ratio(requested: u8) -> Result<u8, &'static str> {
    let caps = caps().ok_or("no power caps")?;
    if !caps.has_msr || !caps.has_eist {
        return Err("EIST/MSR unsupported");
    }
    let min = caps.min_ratio.ok_or("min ratio unknown")?;
    let max = caps.max_ratio.ok_or("max ratio unknown")?;
    let ratio = requested.clamp(min, max);

    let value = (ratio as u64) << 8;
    unsafe { Msr::new(IA32_MSR_PERF_CTL).write(value) };
    Ok(ratio)
}

fn detect_caps() -> Option<PowerCaps> {
    let r0 = unsafe { __cpuid(0x0) };
    let max_leaf = r0.eax;
    let vendor_intel = r0.ebx == 0x756e6547 && r0.edx == 0x49656e69 && r0.ecx == 0x6c65746e;

    let r1 = unsafe { __cpuid(0x1) };
    let has_msr = (r1.edx & (1 << 5)) != 0;
    let has_eist = (r1.ecx & (1 << 7)) != 0;

    let mut has_hwp = false;
    if max_leaf >= 0x6 {
        let r6 = unsafe { __cpuid(0x6) };
        has_hwp = (r6.eax & (1 << 7)) != 0;
    }

    let mut min_ratio = None;
    let mut max_ratio = None;
    let mut hwp_lowest = None;
    let mut hwp_highest = None;

    if has_msr && has_eist {
        let value = unsafe { Msr::new(IA32_MSR_PLATFORM_INFO).read() };
        let min = ((value >> 40) & 0xff) as u8;
        let max = ((value >> 8) & 0xff) as u8;
        if min > 0 {
            min_ratio = Some(min);
        }
        if max > 0 {
            max_ratio = Some(max);
        }
    }

    if has_msr && has_hwp {
        let caps = unsafe { Msr::new(IA32_MSR_HWP_CAPABILITIES).read() };
        let lowest = (caps & 0xff) as u8;
        let highest = ((caps >> 24) & 0xff) as u8;
        if lowest > 0 {
            hwp_lowest = Some(lowest);
        }
        if highest > 0 {
            hwp_highest = Some(highest);
        }
    }

    Some(PowerCaps {
        vendor_intel,
        has_msr,
        has_eist,
        has_hwp,
        min_ratio,
        max_ratio,
        hwp_lowest,
        hwp_highest,
    })
}

fn opt_u8(v: Option<u8>) -> u8 {
    v.unwrap_or(0)
}