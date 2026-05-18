/*
what the god of time gives to you

fast actual time
reliable hardware interrupt callbacks at desired intervalls
normal code
  ->
APIC fires
  ->
Chronos ISR
  ->
update time + mark expired timers
  ->
return to interrupted code
  ->
consumer task wakes and runs

EXAMPLE TIME

time_source / Chronos ISR
  updates latest TimeSnapshot
  increments watch seq
  marks timer A due every 10 ms
  marks timer B due every 1000 ms

consumer_a task
  wakes when A becomes due
  reads latest snapshot
  does 10 ms work

consumer_b task
  wakes when B becomes due
  reads latest snapshot
  does 1 s work
*/
// this is our apic helper
#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::_rdtsc;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use spin::Mutex;
#[cfg(target_arch = "x86_64")]
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame};

pub(crate) const CHRONOS_TIMER_VECTOR: u8 = 0x40;

static CHRONOS_AWAKE: AtomicBool = AtomicBool::new(false);
static WATCH_SEQ: AtomicU64 = AtomicU64::new(0);
static LATEST_SNAPSHOT: Mutex<TimeSnapshot> = Mutex::new(TimeSnapshot::zero());

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct TimeSnapshot {
    pub seq: u64,
    pub mono_ticks: u64,
    pub mono_ms: u64,
    pub tsc: u64,
}

impl TimeSnapshot {
    pub const fn zero() -> Self {
        Self {
            seq: 0,
            mono_ticks: 0,
            mono_ms: 0,
            tsc: 0,
        }
    }
}

#[inline]
fn read_cycle_counter() -> u64 {
    #[cfg(target_arch = "x86_64")]
    {
        return unsafe { _rdtsc() };
    }

    #[cfg(not(target_arch = "x86_64"))]
    {
        // ARMTODO: Non-x86 builds still track monotonic time; a real ARM port
        // can wire this up to CNTVCT_EL0 or another platform cycle counter
        // later.
        0
    }
}

#[inline]
fn capture_snapshot(seq: u64) -> TimeSnapshot {
    let mono_ticks = embassy_time_driver::now();
    let hz = embassy_time_driver::TICK_HZ.max(1);
    TimeSnapshot {
        seq,
        mono_ticks,
        mono_ms: mono_ticks.saturating_mul(1000) / hz,
        tsc: read_cycle_counter(),
    }
}

#[inline]
fn refresh_snapshot(seq: u64) -> TimeSnapshot {
    let snapshot = capture_snapshot(seq);
    *LATEST_SNAPSHOT.lock() = snapshot;
    snapshot
}

#[inline]
pub fn monotonic_nanos() -> u64 {
    let snapshot = latest_snapshot();
    let live_ticks = embassy_time_driver::now();
    let ticks = if snapshot.seq != 0 || is_awake() {
        live_ticks.max(snapshot.mono_ticks)
    } else {
        live_ticks
    };
    let hz = embassy_time_driver::TICK_HZ.max(1) as u128;
    ((ticks as u128) * 1_000_000_000u128 / hz).min(u64::MAX as u128) as u64
}

#[inline]
pub fn best_effort_unix_time_seconds() -> Option<u64> {
    crate::r::net::ntp::current_unix_seconds().or_else(crate::time::unix_time_seconds)
}

#[cfg(target_arch = "x86_64")]
pub(crate) fn interrupt_install(idt: &mut InterruptDescriptorTable) {
    idt[CHRONOS_TIMER_VECTOR].set_handler_fn(CHRONOS_TIMER);
}

pub(crate) fn awake() {
    CHRONOS_AWAKE.store(true, Ordering::Release);
    let seq = WATCH_SEQ.load(Ordering::Acquire);
    let _ = refresh_snapshot(seq);
}

#[inline]
pub fn is_awake() -> bool {
    CHRONOS_AWAKE.load(Ordering::Acquire)
}

#[inline]
pub fn latest_snapshot() -> TimeSnapshot {
    *LATEST_SNAPSHOT.lock()
}

#[allow(non_snake_case)]
#[cfg(target_arch = "x86_64")]
pub(crate) extern "x86-interrupt" fn CHRONOS_TIMER(_stack_frame: InterruptStackFrame) {
    let seq = WATCH_SEQ.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
    let _ = refresh_snapshot(seq);
    unsafe {
        crate::portio::outb(0xE9, b'?');
    }
}
