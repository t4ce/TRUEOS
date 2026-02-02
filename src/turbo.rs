use core::sync::atomic::{AtomicU8, Ordering};

use raw_cpuid::CpuId;
use x86_64::registers::model_specific::Msr;
use crate::wait;

const MSR_IA32_MISC_ENABLE: u32 = 0x1A0;
const TURBO_DISABLE_BIT: u64 = 1 << 38;

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

#[inline]
fn supported_cpuid() -> bool {
    let cpuid = CpuId::new();
    let vendor_intel = cpuid
        .get_vendor_info()
        .map(|v| v.as_str() == "GenuineIntel")
        .unwrap_or(false);
    let has_msr = cpuid.get_feature_info().map(|f| f.has_msr()).unwrap_or(false);
    vendor_intel && has_msr
}

pub fn local_status() -> TurboStatus {
    if !supported_cpuid() {
        return TurboStatus::Unsupported;
    }
    TurboStatus::State(local_state_raw())
}

pub fn local_state() -> Result<TurboState, TurboSetError> {
    if !supported_cpuid() {
        return Err(TurboSetError::Unsupported);
    }
    Ok(local_state_raw())
}

fn local_state_raw() -> TurboState {
    let value = unsafe { Msr::new(MSR_IA32_MISC_ENABLE).read() };
    if (value & TURBO_DISABLE_BIT) != 0 {
        TurboState::NoTurbo
    } else {
        TurboState::Turbo
    }
}

const RET_TURBO: u64 = 10;
const RET_NOTURBO: u64 = 11;

fn smp_read_turbo(_arg: u64) -> u64 {
    if !supported_cpuid() {
        return RET_UNSUPPORTED;
    }
    match local_state_raw() {
        TurboState::Turbo => RET_TURBO,
        TurboState::NoTurbo => RET_NOTURBO,
    }
}

pub fn set_enabled_local(enable: bool) -> Result<TurboState, TurboSetError> {
    if !supported_cpuid() {
        return Err(TurboSetError::Unsupported);
    }
    if !armed() {
        return Err(TurboSetError::Disarmed);
    }

    let mut msr = Msr::new(MSR_IA32_MISC_ENABLE);
    let cur = unsafe { msr.read() };
    let want = if enable {
        cur & !TURBO_DISABLE_BIT
    } else {
        cur | TURBO_DISABLE_BIT
    };
    unsafe { msr.write(want) };

    Ok(local_state_raw())
}

const RET_OK: u64 = 0;
const RET_UNSUPPORTED: u64 = 1;
const RET_DISARMED: u64 = 2;

fn smp_apply_turbo(arg: u64) -> u64 {
    let enable = arg != 0;
    if !supported_cpuid() {
        return RET_UNSUPPORTED;
    }
    if !armed() {
        return RET_DISARMED;
    }

    let _ = set_enabled_local(enable);
    RET_OK
}

pub fn set_enabled_all(enable: bool) -> Result<TurboApplyReport, TurboSetError> {
    if !supported_cpuid() {
        return Err(TurboSetError::Unsupported);
    }
    if !armed() {
        return Err(TurboSetError::Disarmed);
    }

    let total = crate::smp::cpu_count().max(1);

    // Apply on BSP immediately.
    let _ = set_enabled_local(enable)?;

    // Schedule on all APs.
    let submit = crate::smp::submit_to_all_online_aps(smp_apply_turbo, if enable { 1 } else { 0 });

    Ok(TurboApplyReport {
        requested_enable: enable,
        total_cpus: total,
        targeted_aps: submit.targeted_aps,
        submitted_aps: submit.submitted_aps,
        busy_aps: submit.busy_aps,
        seq: submit.seq,
    })
}

pub fn verify_all(spins: usize) -> Result<TurboVerifyReport, TurboSetError> {
    if !supported_cpuid() {
        return Err(TurboSetError::Unsupported);
    }
    let total = crate::smp::cpu_count().max(1);

    // Always include BSP state immediately.
    let mut turbo_cpus: usize = 0;
    let mut noturbo_cpus: usize = 0;
    let mut unknown_cpus: usize = 0;

    match local_status() {
        TurboStatus::Unsupported => unknown_cpus += 1,
        TurboStatus::State(TurboState::Turbo) => turbo_cpus += 1,
        TurboStatus::State(TurboState::NoTurbo) => noturbo_cpus += 1,
    }

    let submit = crate::smp::submit_to_all_online_aps(smp_read_turbo, 0);

    // Wait only for APs that actually received this request sequence.
    // If some APs were busy and not overwritten, `wait_all_online_aps()` would
    // never complete because those CPUs will retain an older `seq`.
    let waited = if submit.seq == 0 {
        true
    } else {
        let mut ok = false;
        for _ in 0..spins {
            let mut done = true;
            for slot in 1..total {
                let Some(r) = crate::smp::read(slot) else {
                    done = false;
                    break;
                };
                if !r.online {
                    continue;
                }
                if r.seq != submit.seq {
                    continue;
                }
                if r.state != crate::smp::STATE_DONE {
                    done = false;
                    break;
                }
            }
            if done {
                ok = true;
                break;
            }
            wait::spin_step();
        }
        ok
    };

    let mut completed_aps: usize = 0;

    for slot in 1..total {
        let Some(r) = crate::smp::read(slot) else {
            continue;
        };
        if !r.online {
            continue;
        }
        if r.seq != submit.seq || r.state != crate::smp::STATE_DONE {
            continue;
        }

        completed_aps += 1;
        match r.ret {
            RET_TURBO => turbo_cpus += 1,
            RET_NOTURBO => noturbo_cpus += 1,
            _ => unknown_cpus += 1,
        }
    }

    Ok(TurboVerifyReport {
        total_cpus: total,
        online_aps: submit.targeted_aps,
        submitted_aps: submit.submitted_aps,
        busy_aps: submit.busy_aps,
        completed_aps,
        turbo_cpus,
        noturbo_cpus,
        unknown_cpus,
        seq: submit.seq,
        timed_out: !waited,
    })
}
