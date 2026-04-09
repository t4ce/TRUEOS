#![allow(dead_code)]
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

static IDLE_POLICY: AtomicU8 = AtomicU8::new(IdlePolicy::Halt as u8);

static MSR_ARMED: AtomicU8 = AtomicU8::new(0);

#[derive(Clone, Copy, Debug)]
pub struct PowerCaps {
    pub vendor_intel: bool,
    pub has_msr: bool,
    pub has_eist: bool,
    pub has_hwp: bool,
}

static CAPS: Once<Option<PowerCaps>> = Once::new();

#[derive(Clone, Copy, Debug)]
pub struct PowerMsrDetails {
    pub min_ratio: Option<u8>,
    pub max_ratio: Option<u8>,
    pub hwp_lowest: Option<u8>,
    pub hwp_highest: Option<u8>,
}

static MSR_DETAILS: Once<Option<PowerMsrDetails>> = Once::new();

pub fn init() {
    // Safety note:
    // Even if CPUID reports that the RDMSR/WRMSR instructions exist, reading a
    // *specific* MSR that isn't implemented will #GP. On many bare-metal setups
    // (especially early boot, before robust exception handling), that can become
    // a triple-fault and immediate reboot.
    //
    // Therefore init() is CPUID-only: it will never touch MSRs.
    CAPS.call_once(detect_caps_cpuid_only);

    if let Some(caps) = caps() {
        if crate::logflag::BOOT_INFO_LOGS {
            crate::log!(
                "POWER: intel={} msr={} eist={} hwp={} msr_armed={} idle={}\n",
                caps.vendor_intel,
                caps.has_msr,
                caps.has_eist,
                caps.has_hwp,
                msr_armed(),
                idle_policy().as_str(),
            );
        }

        if crate::logflag::BOOT_INFO_LOGS
            && let Some(d) = msr_details()
        {
            crate::log!(
                "POWER: msr_details min={} max={} hwp_lowest={} hwp_highest={}\n",
                opt_u8(d.min_ratio),
                opt_u8(d.max_ratio),
                opt_u8(d.hwp_lowest),
                opt_u8(d.hwp_highest),
            );
        }
    } else {
        crate::log!("POWER: caps unavailable\n");
    }
}

pub fn caps() -> Option<&'static PowerCaps> {
    CAPS.get().and_then(|caps| caps.as_ref())
}

pub fn msr_armed() -> bool {
    MSR_ARMED.load(Ordering::Acquire) != 0
}

pub fn msr_details() -> Option<&'static PowerMsrDetails> {
    MSR_DETAILS.get().and_then(|d| d.as_ref())
}

/// Probes Intel MSR-only detail fields (platform ratios, HWP caps).
///
/// # Safety
/// If the MSRs being probed are not implemented by the CPU/firmware, the reads
/// will raise #GP. If your exception path is not safe, this can reboot the
/// machine. Keep this opt-in and only call when you're prepared.
pub unsafe fn probe_msr_details() -> Option<&'static PowerMsrDetails> {
    if !msr_armed() {
        return None;
    }

    MSR_DETAILS.call_once(detect_msr_details);
    msr_details()
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
        IdlePolicy::Halt => {
            if x86_64::instructions::interrupts::are_enabled() {
                x86_64::instructions::hlt()
            } else {
                core::hint::spin_loop()
            }
        }
    }
}

pub fn current_ratio() -> Option<u8> {
    let caps = caps()?;
    if !msr_armed() || !caps.has_msr || !caps.has_eist {
        return None;
    }
    let value = unsafe { Msr::new(IA32_MSR_PERF_STATUS).read() };
    Some(((value >> 8) & 0xff) as u8)
}

pub fn set_pstate_ratio(requested: u8) -> Result<u8, &'static str> {
    let caps = caps().ok_or("no power caps")?;
    if !msr_armed() || !caps.has_msr || !caps.has_eist {
        return Err("EIST/MSR unsupported");
    }
    let details = msr_details().ok_or("msr details not probed")?;
    let min = details.min_ratio.ok_or("min ratio unknown")?;
    let max = details.max_ratio.ok_or("max ratio unknown")?;
    let ratio = requested.clamp(min, max);

    let value = (ratio as u64) << 8;
    unsafe { Msr::new(IA32_MSR_PERF_CTL).write(value) };
    Ok(ratio)
}

fn detect_caps_cpuid_only() -> Option<PowerCaps> {
    let r0 = __cpuid(0x0);
    let max_leaf = r0.eax;
    let vendor_intel = r0.ebx == 0x756e6547 && r0.edx == 0x49656e69 && r0.ecx == 0x6c65746e;

    let r1 = __cpuid(0x1);
    let has_msr = (r1.edx & (1 << 5)) != 0;
    // EIST/HWP probing is Intel-specific. On other vendors, treating these bits as
    // authoritative can lead to invalid MSR reads and a #GP (which currently reboots).
    let has_eist = vendor_intel && (r1.ecx & (1 << 7)) != 0;

    let mut has_hwp = false;
    if vendor_intel && max_leaf >= 0x6 {
        let r6 = __cpuid(0x6);
        has_hwp = (r6.eax & (1 << 7)) != 0;
    }

    Some(PowerCaps {
        vendor_intel,
        has_msr,
        has_eist,
        has_hwp,
    })
}

fn detect_msr_details() -> Option<PowerMsrDetails> {
    let caps = caps()?;
    if !caps.vendor_intel || !caps.has_msr {
        return None;
    }

    let mut min_ratio = None;
    let mut max_ratio = None;
    let mut hwp_lowest = None;
    let mut hwp_highest = None;

    if caps.has_eist {
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

    if caps.has_hwp {
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

    Some(PowerMsrDetails {
        min_ratio,
        max_ratio,
        hwp_lowest,
        hwp_highest,
    })
}

fn opt_u8(v: Option<u8>) -> u8 {
    v.unwrap_or(0)
}
