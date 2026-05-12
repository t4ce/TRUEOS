#![allow(dead_code)]

use core::sync::atomic::{AtomicU8, Ordering};

use spin::Once;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdlePolicy {
    Spin = 0,
    Halt = 1,
}

impl IdlePolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            IdlePolicy::Spin => "spin",
            IdlePolicy::Halt => "halt",
        }
    }

    fn from_u8(value: u8) -> Self {
        match value {
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
    CAPS.call_once(|| {
        Some(PowerCaps {
            vendor_intel: false,
            has_msr: false,
            has_eist: false,
            has_hwp: false,
        })
    });
    MSR_DETAILS.call_once(|| None);
}

pub fn caps() -> Option<&'static PowerCaps> {
    CAPS.get().and_then(|caps| caps.as_ref())
}

pub fn msr_armed() -> bool {
    false
}

pub fn msr_details() -> Option<&'static PowerMsrDetails> {
    MSR_DETAILS.get().and_then(|details| details.as_ref())
}

pub unsafe fn probe_msr_details() -> Option<&'static PowerMsrDetails> {
    None
}

pub fn idle_policy() -> IdlePolicy {
    IdlePolicy::from_u8(IDLE_POLICY.load(Ordering::Acquire))
}

pub fn set_idle_policy(policy: IdlePolicy) -> IdlePolicy {
    let previous = IDLE_POLICY.swap(policy as u8, Ordering::AcqRel);
    IdlePolicy::from_u8(previous)
}

#[inline(always)]
pub fn idle_hint() {
    core::hint::spin_loop()
}

pub fn current_ratio() -> Option<u8> {
    None
}

pub fn set_pstate_ratio(_requested: u8) -> Result<u8, &'static str> {
    Err("unsupported")
}
