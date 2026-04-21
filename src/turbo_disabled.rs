use core::sync::atomic::{AtomicU8, Ordering};

// ARMTODO: turbo control is currently an Intel/x86 MSR + CPUID path. A real
// non-x86 implementation would need platform-specific policy and firmware hooks
// instead of IA32_MISC_ENABLE / Intel P/E-core assumptions.

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