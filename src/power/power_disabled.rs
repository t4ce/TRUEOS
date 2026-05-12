#![allow(dead_code)]

use core::sync::atomic::{AtomicU8, Ordering};

use spin::Once;

pub mod turbo {
    use core::sync::atomic::{AtomicU8, Ordering};

    static TURBO_ARMED: AtomicU8 = AtomicU8::new(0);

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum TurboState {
        Turbo,
        NoTurbo,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum TurboStatus {
        Unsupported,
        State(TurboState),
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum TurboSetError {
        Unsupported,
        Disarmed,
    }

    #[derive(Clone, Copy, Debug)]
    pub struct TurboApplyReport {
        pub requested_enable: bool,
        pub total_cpus: usize,
        pub targeted_aps: usize,
        pub submitted_aps: usize,
        pub busy_aps: usize,
        pub seq: u64,
    }

    #[derive(Clone, Copy, Debug)]
    pub struct TurboVerifyReport {
        pub total_cpus: usize,
        pub online_aps: usize,
        pub submitted_aps: usize,
        pub busy_aps: usize,
        pub completed_aps: usize,
        pub turbo_cpus: usize,
        pub noturbo_cpus: usize,
        pub unknown_cpus: usize,
        pub seq: u64,
        pub timed_out: bool,
    }

    #[inline]
    pub fn armed() -> bool {
        TURBO_ARMED.load(Ordering::Acquire) != 0
    }

    pub fn set_armed(v: bool) {
        TURBO_ARMED.store(if v { 1 } else { 0 }, Ordering::Release);
    }

    pub fn local_status() -> TurboStatus {
        TurboStatus::Unsupported
    }

    pub fn local_state() -> Result<TurboState, TurboSetError> {
        Err(TurboSetError::Unsupported)
    }

    pub fn set_enabled_local(_enable: bool) -> Result<TurboState, TurboSetError> {
        Err(TurboSetError::Unsupported)
    }

    pub fn set_enabled_all(_enable: bool) -> Result<TurboApplyReport, TurboSetError> {
        Err(TurboSetError::Unsupported)
    }

    pub fn verify_all(_spins: usize) -> Result<TurboVerifyReport, TurboSetError> {
        Err(TurboSetError::Unsupported)
    }
}

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
