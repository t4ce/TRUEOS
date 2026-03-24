/*
what the god of time gives to you

fast actual time
reliable hardware interrupt callbacks at desired intervalls
easy api
*/
// this is our apic helper
use core::arch::x86_64::_rdtsc;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use heapless::Vec;
use spin::Mutex;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame};

pub(crate) const CHRONOS_TIMER_VECTOR: u8 = 0x40;

const MAX_TIMERS: usize = 64;

static CHRONOS_AWAKE: AtomicBool = AtomicBool::new(false);
static NEXT_TIMER_ID: AtomicU32 = AtomicU32::new(1);
static WATCH_SEQ: AtomicU64 = AtomicU64::new(0);
static LATEST_SNAPSHOT: Mutex<TimeSnapshot> = Mutex::new(TimeSnapshot::zero());
static TIMERS: Mutex<Vec<TimerEntry, MAX_TIMERS>> = Mutex::new(Vec::new());

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

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct TimerHandle {
    id: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct TimerRegistration {
    pub interval_ms: u64,
    pub first_delay_ms: Option<u64>,
    pub only_once: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum RegisterTimerError {
    ZeroInterval,
    RegistryFull,
}

#[derive(Copy, Clone, Debug)]
struct TimerEntry {
    id: u32,
    interval_ticks: u64,
    next_deadline_ticks: u64,
    only_once: bool,
    active: bool,
    fire_seq: u64,
    last_snapshot: TimeSnapshot,
}

#[inline]
fn ms_to_ticks(ms: u64) -> u64 {
    let hz = embassy_time_driver::TICK_HZ;
    if hz == 0 {
        return 0;
    }
    ms.saturating_mul(hz).div_ceil(1000).max(1)
}

#[inline]
fn capture_snapshot(seq: u64) -> TimeSnapshot {
    let mono_ticks = embassy_time_driver::now();
    let hz = embassy_time_driver::TICK_HZ.max(1);
    TimeSnapshot {
        seq,
        mono_ticks,
        mono_ms: mono_ticks.saturating_mul(1000) / hz,
        tsc: unsafe { _rdtsc() },
    }
}

#[inline]
fn refresh_snapshot(seq: u64) -> TimeSnapshot {
    let snapshot = capture_snapshot(seq);
    *LATEST_SNAPSHOT.lock() = snapshot;
    snapshot
}

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

#[inline]
pub fn watch_seq() -> u64 {
    WATCH_SEQ.load(Ordering::Acquire)
}

#[inline]
pub fn watch_snapshot() -> (u64, TimeSnapshot) {
    let snapshot = latest_snapshot();
    (snapshot.seq, snapshot)
}

pub fn register_timer(spec: TimerRegistration) -> Result<TimerHandle, RegisterTimerError> {
    if spec.interval_ms == 0 {
        return Err(RegisterTimerError::ZeroInterval);
    }

    let now = latest_snapshot();
    let interval_ticks = ms_to_ticks(spec.interval_ms);
    let first_ticks = ms_to_ticks(spec.first_delay_ms.unwrap_or(spec.interval_ms));
    let id = NEXT_TIMER_ID.fetch_add(1, Ordering::AcqRel);

    let mut timers = TIMERS.lock();
    if timers.is_full() {
        return Err(RegisterTimerError::RegistryFull);
    }

    let entry = TimerEntry {
        id,
        interval_ticks,
        next_deadline_ticks: now.mono_ticks.saturating_add(first_ticks),
        only_once: spec.only_once,
        active: true,
        fire_seq: 0,
        last_snapshot: now,
    };
    let _ = timers.push(entry);
    Ok(TimerHandle { id })
}

pub fn cancel_timer(handle: TimerHandle) -> bool {
    let mut timers = TIMERS.lock();
    if let Some(entry) = timers.iter_mut().find(|entry| entry.id == handle.id) {
        entry.active = false;
        return true;
    }
    false
}

pub fn poll_timer(handle: TimerHandle, observed_fire_seq: u64) -> Option<(u64, TimeSnapshot)> {
    let timers = TIMERS.lock();
    let entry = timers.iter().find(|entry| entry.id == handle.id)?;
    if entry.fire_seq == observed_fire_seq {
        return None;
    }
    Some((entry.fire_seq, entry.last_snapshot))
}

fn service_timers(snapshot: TimeSnapshot) {
    let mut timers = TIMERS.lock();
    for entry in timers.iter_mut() {
        if !entry.active || snapshot.mono_ticks < entry.next_deadline_ticks {
            continue;
        }

        entry.fire_seq = entry.fire_seq.wrapping_add(1);
        entry.last_snapshot = snapshot;

        if entry.only_once {
            entry.active = false;
            continue;
        }

        let step = entry.interval_ticks.max(1);
        while entry.next_deadline_ticks <= snapshot.mono_ticks {
            entry.next_deadline_ticks = entry.next_deadline_ticks.saturating_add(step);
        }
    }
}

#[allow(non_snake_case)]
pub(crate) extern "x86-interrupt" fn CHRONOS_TIMER(_stack_frame: InterruptStackFrame) {
    let seq = WATCH_SEQ.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
    let snapshot = refresh_snapshot(seq);
    service_timers(snapshot);
    unsafe {
        crate::portio::outb(0xE9, b'?');
    }
}
