//! Aggregate transient-entry timing. No allocation or logging on each exit.
//! entry_cycles includes assembly xstate switching and VM entry/exit overhead;
//! it is deliberately not labelled pure guest instruction time.
use core::sync::atomic::{AtomicU64, Ordering};
struct Counters { count: AtomicU64, setup: AtomicU64, entry: AtomicU64, finish: AtomicU64, maximum: AtomicU64 }
impl Counters {
    const fn new() -> Self { Self { count: AtomicU64::new(0), setup: AtomicU64::new(0), entry: AtomicU64::new(0), finish: AtomicU64::new(0), maximum: AtomicU64::new(0) } }
}
static COUNTERS: [Counters; crate::percpu::CPU_SLOT_LIMIT] = [const { Counters::new() }; crate::percpu::CPU_SLOT_LIMIT];
#[inline]
pub(in crate::hv) fn cycles() -> u64 {
    unsafe { core::arch::x86_64::_mm_lfence(); core::arch::x86_64::_rdtsc() }
}
pub(in crate::hv) fn record(owner: u8, start: u64, enter: u64, leave: u64, end: u64, reason: u64, rip: u64) {
    let slot = crate::percpu::current_slot();
    let Some(c) = COUNTERS.get(slot) else { return; };
    c.setup.fetch_add(enter.wrapping_sub(start), Ordering::Relaxed);
    c.entry.fetch_add(leave.wrapping_sub(enter), Ordering::Relaxed);
    c.finish.fetch_add(end.wrapping_sub(leave), Ordering::Relaxed);
    c.maximum.fetch_max(end.wrapping_sub(start), Ordering::Relaxed);
    if c.count.fetch_add(1, Ordering::Relaxed) + 1 < 2048 { return; }
    let count = c.count.swap(0, Ordering::Relaxed);
    let setup = c.setup.swap(0, Ordering::Relaxed);
    let entry = c.entry.swap(0, Ordering::Relaxed);
    let finish = c.finish.swap(0, Ordering::Relaxed);
    let maximum = c.maximum.swap(0, Ordering::Relaxed);
    crate::log_important!(target: "hv";
        "XPAPP VMX TIME vm={} slot={} samples={} tsc_hz={} setup_cycles={} entry_cycles={} finish_cycles={} max_total_cycles={} reason={} rip=0x{:08x} scope=entry-includes-xstate-and-vmx-overhead\n",
        owner, slot, count, crate::time::tsc_hz(), setup, entry, finish, maximum, reason, rip);
}
