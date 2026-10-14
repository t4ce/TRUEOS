//! Passive counters only; ISR paths never allocate, format, log or take locks.
use core::sync::atomic::{AtomicU64, Ordering};

struct Counters {
    poll_begin: AtomicU64,
    poll_end: AtomicU64,
    ipi_sent: AtomicU64,
    ipi_failed: AtomicU64,
    ipi_received: AtomicU64,
    timer_armed: AtomicU64,
    timer_received: AtomicU64,
}
impl Counters {
    const fn new() -> Self {
        Self {
            poll_begin: AtomicU64::new(0),
            poll_end: AtomicU64::new(0),
            ipi_sent: AtomicU64::new(0),
            ipi_failed: AtomicU64::new(0),
            ipi_received: AtomicU64::new(0),
            timer_armed: AtomicU64::new(0),
            timer_received: AtomicU64::new(0),
        }
    }
}
static COUNTERS: [Counters; crate::allcaps::hv::VM_CPU_SLOT_LIMIT] =
    [const { Counters::new() }; crate::allcaps::hv::VM_CPU_SLOT_LIMIT];

#[derive(Clone, Copy, Default)]
pub(crate) struct Snapshot {
    pub poll_begin: u64,
    pub poll_end: u64,
    pub ipi_sent: u64,
    pub ipi_failed: u64,
    pub ipi_received: u64,
    pub timer_armed: u64,
    pub timer_received: u64,
}
pub(crate) fn snapshot(slot: usize) -> Snapshot {
    let Some(c) = COUNTERS.get(slot) else {
        return Snapshot::default();
    };
    Snapshot {
        poll_begin: c.poll_begin.load(Ordering::Relaxed),
        poll_end: c.poll_end.load(Ordering::Relaxed),
        ipi_sent: c.ipi_sent.load(Ordering::Relaxed),
        ipi_failed: c.ipi_failed.load(Ordering::Relaxed),
        ipi_received: c.ipi_received.load(Ordering::Relaxed),
        timer_armed: c.timer_armed.load(Ordering::Relaxed),
        timer_received: c.timer_received.load(Ordering::Relaxed),
    }
}
fn observe(slot: usize, select: impl FnOnce(&Counters) -> &AtomicU64) {
    if !crate::allcaps::hv::CODEX_PROGRESS_PROBES_ENABLED {
        return;
    }
    if let Some(c) = COUNTERS.get(slot) {
        select(c).fetch_add(1, Ordering::Relaxed);
    }
}
pub(crate) fn poll_begin(slot: usize) {
    observe(slot, |c| &c.poll_begin);
}
pub(crate) fn poll_end(slot: usize) {
    observe(slot, |c| &c.poll_end);
}
pub(crate) fn ipi_sent(slot: usize, success: bool) {
    observe(slot, |c| if success { &c.ipi_sent } else { &c.ipi_failed });
}
pub(crate) fn ipi_received(slot: usize) {
    observe(slot, |c| &c.ipi_received);
}
pub(crate) fn timer_armed(slot: usize) {
    observe(slot, |c| &c.timer_armed);
}
pub(crate) fn timer_received(slot: usize) {
    observe(slot, |c| &c.timer_received);
}

/// Read IF without modifying the interrupt state or CPU flags.
pub(crate) fn interrupts_enabled() -> bool {
    let flags: u64;
    unsafe {
        core::arch::asm!("pushfq", "pop {}", out(reg) flags, options(preserves_flags));
    }
    flags & (1 << 9) != 0
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn delivery_and_poll_completion_are_independent_evidence() {
        let slot = 0;
        let before = snapshot(slot);
        poll_begin(slot);
        ipi_sent(slot, true);
        ipi_sent(slot, false);
        ipi_received(slot);
        timer_armed(slot);
        timer_received(slot);
        let active = snapshot(slot);
        assert_eq!(active.poll_begin - before.poll_begin, 1);
        assert_eq!(active.poll_end, before.poll_end);
        assert_eq!(active.ipi_sent - before.ipi_sent, 1);
        assert_eq!(active.ipi_failed - before.ipi_failed, 1);
        assert_eq!(active.ipi_received - before.ipi_received, 1);
        assert_eq!(active.timer_armed - before.timer_armed, 1);
        assert_eq!(active.timer_received - before.timer_received, 1);
        poll_end(slot);
        assert_eq!(snapshot(slot).poll_end - before.poll_end, 1);
        // Invalid topology access remains bounded.
        poll_begin(usize::MAX);
        assert_eq!(snapshot(usize::MAX).poll_begin, 0);
    }
}
