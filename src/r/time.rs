#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::{__cpuid, _rdtsc};
use core::sync::atomic::{AtomicU64, Ordering};
use core::task::Waker;

use embassy_time_driver::{Driver, TICK_HZ};
use heapless::Vec;
use spin::{Mutex, Once};

struct WakeEntry {
    at: u64,
    waker: Waker,
}

const MAX_WAKEUPS: usize = 9000;

static START_TSC: AtomicU64 = AtomicU64::new(0);
static TSC_HZ: AtomicU64 = AtomicU64::new(0);
static INIT: Once<()> = Once::new();

static QUEUE: Mutex<Vec<WakeEntry, MAX_WAKEUPS>> = Mutex::new(Vec::new());

#[inline]
fn read_cycle_counter() -> u64 {
    #[cfg(target_arch = "x86_64")]
    {
        return unsafe { _rdtsc() };
    }

    #[cfg(not(target_arch = "x86_64"))]
    {
        // ARMTODO: wire this to a real platform cycle counter for non-x86.
        0
    }
}

#[inline]
pub fn uptime_seconds() -> u64 {
    let ticks = embassy_time_driver::now();
    let hz = TICK_HZ;
    if hz == 0 { 0 } else { ticks / hz }
}

#[inline]
pub fn tsc_hz() -> u64 {
    init_once();
    TSC_HZ.load(Ordering::Relaxed).max(1)
}

/// Best-effort Unix time (seconds since epoch).
///
/// Uses Limine's boot timestamp (wall clock at boot) plus monotonic uptime.
/// Returns `None` if the boot timestamp is unavailable (or 0).
#[inline]
pub fn unix_time_seconds() -> Option<u64> {
    let base = crate::limine::boot_timestamp_secs()?;
    if base == 0 {
        return None;
    }
    Some(base.saturating_add(uptime_seconds()))
}

fn init_once() {
    INIT.call_once(|| {
        let start = read_cycle_counter();
        START_TSC.store(start, Ordering::Relaxed);
        let hz = detect_tsc_hz().max(1);
        if crate::logflag::BOOT_INFO_LOGS {
            crate::log!("time: tsc_hz={}\n", hz);
        }
        TSC_HZ.store(hz, Ordering::Relaxed);
    });
}

fn detect_tsc_hz() -> u64 {
    #[cfg(not(target_arch = "x86_64"))]
    {
        return TICK_HZ.max(1);
    }

    #[cfg(target_arch = "x86_64")]
    if let Some(hpet) = crate::efi::acpi::hpet::ensure()
        && let Some(calibrated_hz) = calibrate_tsc_hz_with_hpet(hpet)
    {
        if crate::logflag::BOOT_INFO_LOGS {
            crate::log!("time: tsc_hz calibrated via HPET: {}\n", calibrated_hz);
        }
        return calibrated_hz;
    }

    #[cfg(target_arch = "x86_64")]
    {
        detect_tsc_hz_from_cpuid()
    }
}

#[cfg(target_arch = "x86_64")]
fn detect_tsc_hz_from_cpuid() -> u64 {
    let r15 = __cpuid(0x15);
    let denom = r15.eax as u64;
    let numer = r15.ebx as u64;
    let crystal_hz = r15.ecx as u64;
    if crate::logflag::BOOT_INFO_LOGS {
        crate::log!(
            "time: cpuid 0x15: denom={} numer={} crystal_hz={}\n",
            denom,
            numer,
            crystal_hz
        );
    }

    if denom != 0 && numer != 0 && crystal_hz != 0 {
        let hz = ((crystal_hz as u128) * (numer as u128) / (denom as u128)) as u64;
        if hz >= 1_000_000 {
            return hz;
        }
        // If the value is suspiciously low (e.g. in MHz instead of Hz, common in some virt/TCG quirks),
        // try scaling it.
        if hz > 0 {
            let mhz_estimate = hz * 1_000_000;
            if mhz_estimate >= 100_000_000 {
                return mhz_estimate;
            }
        }
    }

    let r16 = __cpuid(0x16);
    let base_mhz = (r16.eax & 0xFFFF) as u64;
    if crate::logflag::BOOT_INFO_LOGS {
        crate::log!("time: cpuid 0x16: base_mhz={}\n", base_mhz);
    }

    if base_mhz != 0 {
        return base_mhz * 1_000_000;
    }

    1_000_000_000
}

#[cfg(target_arch = "x86_64")]
fn calibrate_tsc_hz_with_hpet(hpet: &crate::efi::acpi::hpet::Hpet) -> Option<u64> {
    let hpet_hz = hpet.frequency_hz();
    if hpet_hz == 0 {
        return None;
    }

    const SAMPLE_MS: u64 = 50;
    let target_hpet_ticks = ((hpet_hz as u128) * (SAMPLE_MS as u128) / 1000u128) as u64;
    if target_hpet_ticks == 0 {
        return None;
    }

    let hpet_start = hpet.main_counter();
    let tsc_start = unsafe { _rdtsc() };

    loop {
        let hpet_now = hpet.main_counter();
        let elapsed = hpet.counter_delta(hpet_start, hpet_now);
        if elapsed >= target_hpet_ticks {
            let tsc_end = read_cycle_counter();
            let tsc_delta = tsc_end.wrapping_sub(tsc_start);
            let hz = ((tsc_delta as u128) * 1000u128 / (SAMPLE_MS as u128)) as u64;
            if hz >= 1_000_000 {
                return Some(hz);
            }
            return None;
        }
        core::hint::spin_loop();
    }
}

fn ticks_from_tsc_delta(delta_tsc: u64, tsc_hz: u64) -> u64 {
    ((delta_tsc as u128) * (TICK_HZ as u128) / (tsc_hz as u128)) as u64
}

pub fn poll() {
    init_once();

    let now = embassy_time_driver::now();
    let mut to_wake: Vec<Waker, MAX_WAKEUPS> = Vec::new();

    {
        let mut queue = QUEUE.lock();
        while let Some(first) = queue.first() {
            if first.at > now {
                break;
            }
            let entry = queue.remove(0);
            let _ = to_wake.push(entry.waker);
        }
    }

    for w in to_wake {
        w.wake();
    }
}

pub fn next_wake_tick() -> Option<u64> {
    QUEUE.lock().first().map(|entry| entry.at)
}

pub fn ticks_until_next_wake() -> Option<u64> {
    let now = embassy_time_driver::now();
    next_wake_tick().map(|at| at.saturating_sub(now))
}

struct TimeDriver;

impl Driver for TimeDriver {
    fn now(&self) -> u64 {
        init_once();

        let start = START_TSC.load(Ordering::Relaxed);
        let tsc_hz = TSC_HZ.load(Ordering::Relaxed).max(1);
        let tsc = read_cycle_counter();
        let delta = tsc.wrapping_sub(start);
        ticks_from_tsc_delta(delta, tsc_hz)
    }

    fn schedule_wake(&self, at: u64, waker: &Waker) {
        let now = self.now();
        if at <= now {
            waker.wake_by_ref();
            return;
        }

        let mut queue = QUEUE.lock();

        let mut idx = 0;
        while idx < queue.len() {
            if at < queue[idx].at {
                break;
            }
            idx += 1;
        }

        let entry = WakeEntry {
            at,
            waker: waker.clone(),
        };

        if queue.insert(idx, entry).is_err()
            && let Some(last) = queue.last()
            && at < last.at
        {
            let _ = queue.pop();
            let insert_idx = idx.min(queue.len());
            let _ = queue.insert(
                insert_idx,
                WakeEntry {
                    at,
                    waker: waker.clone(),
                },
            );
        }
    }
}

embassy_time_driver::time_driver_impl!(static DRIVER: TimeDriver = TimeDriver);
