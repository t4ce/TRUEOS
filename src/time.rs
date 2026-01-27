use core::arch::x86_64::{__cpuid, _rdtsc};
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, Ordering};
use core::task::Waker;

use alloc::boxed::Box;
use alloc::sync::Arc;
use embassy_executor::raw::Executor as RawExecutor;
use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};
use embassy_time_driver::{Driver, TICK_HZ};
use heapless::Vec;
use spin::{Mutex, Once};

struct WakeEntry {
    at: u64,
    waker: Waker,
}

const MAX_WAKEUPS: usize = 64;
const MAX_TIMERS: usize = 128;

pub type TimerId = u64;
const INVALID_TIMER_ID: TimerId = 0;

static START_TSC: AtomicU64 = AtomicU64::new(0);
static TSC_HZ: AtomicU64 = AtomicU64::new(0);
static INIT: Once<()> = Once::new();

static QUEUE: Mutex<Vec<WakeEntry, MAX_WAKEUPS>> = Mutex::new(Vec::new());
static EXECUTOR_PTR: AtomicPtr<RawExecutor> = AtomicPtr::new(core::ptr::null_mut());
static NEXT_TIMER_ID: AtomicU64 = AtomicU64::new(1);

struct TimerEntry {
    id: TimerId,
    cancelled: Arc<AtomicBool>,
}

static TIMERS: Mutex<Vec<TimerEntry, MAX_TIMERS>> = Mutex::new(Vec::new());

#[inline]
pub fn uptime_seconds() -> u64 {
    let ticks = embassy_time_driver::now();
    let hz = TICK_HZ as u64;
    if hz == 0 {
        0
    } else {
        ticks / hz
    }
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

#[inline]
pub fn init(executor: &'static RawExecutor) {
    EXECUTOR_PTR.store(executor as *const _ as *mut RawExecutor, Ordering::Release);
}

#[inline]
fn get_spawner() -> Option<Spawner> {
    let ptr = EXECUTOR_PTR.load(Ordering::Acquire);
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { (&*ptr).spawner() })
    }
}

fn init_once() {
    INIT.call_once(|| {
        let start = unsafe { _rdtsc() as u64 };
        START_TSC.store(start, Ordering::Relaxed);
        TSC_HZ.store(detect_tsc_hz().max(1), Ordering::Relaxed);
    });
}

fn detect_tsc_hz() -> u64 {
    unsafe {
        let r15 = __cpuid(0x15);
        let denom = r15.eax as u64;
        let numer = r15.ebx as u64;
        let crystal_hz = r15.ecx as u64;
        if denom != 0 && numer != 0 && crystal_hz != 0 {
            return ((crystal_hz as u128) * (numer as u128) / (denom as u128)) as u64;
        }

        let r16 = __cpuid(0x16);
        let base_mhz = (r16.eax & 0xFFFF) as u64;
        if base_mhz != 0 {
            return base_mhz * 1_000_000;
        }
    }

    1_000_000_000
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

#[inline]
fn register_timer(cancelled: Arc<AtomicBool>) -> TimerId {
    let id = NEXT_TIMER_ID.fetch_add(1, Ordering::Relaxed).max(1);
    let mut timers = TIMERS.lock();
    if timers.push(TimerEntry { id, cancelled }).is_err() {
        return INVALID_TIMER_ID;
    }
    id
}

#[inline]
fn remove_timer(id: TimerId) {
    let mut timers = TIMERS.lock();
    if let Some(pos) = timers.iter().position(|entry| entry.id == id) {
        timers.swap_remove(pos);
    }
}

#[inline]
fn cancel_timer(id: TimerId) -> bool {
    let mut timers = TIMERS.lock();
    if let Some(pos) = timers.iter().position(|entry| entry.id == id) {
        let entry = timers.swap_remove(pos);
        entry.cancelled.store(true, Ordering::Release);
        true
    } else {
        false
    }
}

#[allow(non_snake_case)]
pub fn setTimeout<F>(delay_ms: u64, callback: F) -> TimerId
where
    F: FnMut() + Send + 'static,
{
    let Some(spawner) = get_spawner() else {
        return INVALID_TIMER_ID;
    };

    let cancelled = Arc::new(AtomicBool::new(false));
    let id = register_timer(cancelled.clone());
    if id == INVALID_TIMER_ID {
        return INVALID_TIMER_ID;
    }

    let delay = EmbassyDuration::from_millis(delay_ms);
    if spawner
        .spawn(timeout_task(id, delay, cancelled, Box::new(callback)))
        .is_err()
    {
        cancel_timer(id);
        return INVALID_TIMER_ID;
    }

    id
}

#[allow(non_snake_case)]
pub fn setInterval<F>(period_ms: u64, callback: F) -> TimerId
where
    F: FnMut() + Send + 'static,
{
    let Some(spawner) = get_spawner() else {
        return INVALID_TIMER_ID;
    };

    let cancelled = Arc::new(AtomicBool::new(false));
    let id = register_timer(cancelled.clone());
    if id == INVALID_TIMER_ID {
        return INVALID_TIMER_ID;
    }

    let period = EmbassyDuration::from_millis(period_ms.max(1));
    if spawner
        .spawn(interval_task(id, period, cancelled, Box::new(callback)))
        .is_err()
    {
        cancel_timer(id);
        return INVALID_TIMER_ID;
    }

    id
}

#[allow(non_snake_case)]
pub fn clearTimeout(id: TimerId) -> bool {
    if id == INVALID_TIMER_ID {
        return false;
    }
    cancel_timer(id)
}

#[allow(non_snake_case)]
pub fn clearInterval(id: TimerId) -> bool {
    clearTimeout(id)
}

#[embassy_executor::task]
async fn timeout_task(
    id: TimerId,
    delay: EmbassyDuration,
    cancelled: Arc<AtomicBool>,
    mut callback: Box<dyn FnMut() + Send + 'static>,
) {
    Timer::after(delay).await;
    if !cancelled.load(Ordering::Acquire) {
        callback();
    }
    remove_timer(id);
}

#[embassy_executor::task]
async fn interval_task(
    id: TimerId,
    period: EmbassyDuration,
    cancelled: Arc<AtomicBool>,
    mut callback: Box<dyn FnMut() + Send + 'static>,
) {
    loop {
        Timer::after(period).await;
        if cancelled.load(Ordering::Acquire) {
            break;
        }
        callback();
    }
    remove_timer(id);
}

struct TimeDriver;

impl Driver for TimeDriver {
    fn now(&self) -> u64 {
        init_once();

        let start = START_TSC.load(Ordering::Relaxed);
        let tsc_hz = TSC_HZ.load(Ordering::Relaxed).max(1);
        let tsc = unsafe { _rdtsc() as u64 };
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

        if queue.insert(idx, entry).is_err() {
            if let Some(last) = queue.last() {
                if at < last.at {
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
    }
}

embassy_time_driver::time_driver_impl!(static DRIVER: TimeDriver = TimeDriver);
