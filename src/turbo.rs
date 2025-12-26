//! Minimal Turbo Boost toggle per core via IA32_MISC_ENABLE.
//! Uses bit 38 (Turbo Disable) where 0 = turbo allowed, 1 = turbo disabled.

use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use x86::msr;

const MSR_IA32_MISC_ENABLE: u32 = 0x1A0;
const TURBO_DISABLE_BIT: u64 = 1 << 38;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurboState {
    Turbo,
    NoTurbo,
}

impl TurboState {
    #[inline]
    const fn to_u8(self) -> u8 {
        match self {
            TurboState::Turbo => 0,
            TurboState::NoTurbo => 1,
        }
    }

    #[inline]
    const fn from_u8(v: u8) -> Self {
        match v {
            1 => TurboState::NoTurbo,
            _ => TurboState::Turbo,
        }
    }
}

// Default to turbo allowed to avoid surprising perf throttling on boot.
static DESIRED: AtomicU8 = AtomicU8::new(TurboState::Turbo.to_u8());
static LOGGED_ERROR: AtomicBool = AtomicBool::new(false);

/// Set the desired turbo state for subsequent per-core applications.
/// Returns the previous state.
pub fn set_desired(state: TurboState) -> TurboState {
    let prev = DESIRED.swap(state.to_u8(), Ordering::AcqRel);
    TurboState::from_u8(prev)
}

/// Read the desired turbo state.
pub fn desired_state() -> TurboState {
    TurboState::from_u8(DESIRED.load(Ordering::Acquire))
}

/// Apply the desired turbo policy on the current core.
pub fn apply_desired_local() -> Result<(), &'static str> {
    apply_local(desired_state())
}

/// Apply a turbo policy on the current core by flipping IA32_MISC_ENABLE bit 38.
/// Safe to call from the per-core async init path; returns Ok if the write was performed.
pub fn apply_local(state: TurboState) -> Result<(), &'static str> {
    // Reading/writing MSRs is privileged; failure would fault, so keep the body small.
    let mut value = unsafe { msr::rdmsr(MSR_IA32_MISC_ENABLE) };
    let want_disable = state == TurboState::NoTurbo;
    let has_disable = (value & TURBO_DISABLE_BIT) != 0;
    if want_disable == has_disable {
        return Ok(());
    }

    if want_disable {
        value |= TURBO_DISABLE_BIT;
    } else {
        value &= !TURBO_DISABLE_BIT;
    }

    unsafe { msr::wrmsr(MSR_IA32_MISC_ENABLE, value) };
    Ok(())
}

/// Apply turbo policy with a single warning if it fails.
pub fn apply_local_log_warn(tag: &str) {
    if let Err(err) = apply_desired_local() {
        if !LOGGED_ERROR.swap(true, Ordering::Relaxed) {
            crate::log_warn!("turbo: apply failed on {tag}: {err}");
        }
    }
}
