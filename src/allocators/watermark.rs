//! Admission before the diagnostic free-list walk, not just before its log.
use core::sync::atomic::{AtomicU64, Ordering};

const PERIOD_MS: u64 = 1_000;
const LARGE_ALLOCATION_BYTES: usize = 1024 * 1024;

pub(super) struct SampleGate {
    next_ms: AtomicU64,
}

impl SampleGate {
    pub(super) const fn new() -> Self {
        Self { next_ms: AtomicU64::new(0) }
    }

    pub(super) fn admit(&self, now_ms: u64, allocation_bytes: usize) -> bool {
        let next = now_ms.saturating_add(PERIOD_MS);
        // Keep immediate diagnostics for large allocations. Advertise that
        // sample so a following small allocation need not walk the list again.
        if allocation_bytes >= LARGE_ALLOCATION_BYTES {
            self.next_ms.fetch_max(next, Ordering::Relaxed);
            return true;
        }
        let observed = self.next_ms.load(Ordering::Relaxed);
        now_ms >= observed
            && self.next_ms.compare_exchange(observed, next, Ordering::Relaxed, Ordering::Relaxed).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

    #[test]
    fn quiet_allocations_sample_before_expensive_work() {
        let gate = SampleGate::new();
        let mut walks = 0;
        for _ in 0..100_000 { walks += usize::from(gate.admit(0, 64)); }
        assert_eq!(walks, 1);
        assert!(!gate.admit(999, 64));
        assert!(gate.admit(1000, 64));
        assert!(!gate.admit(1000, 64));
    }

    #[test]
    fn one_thread_wins_each_window_and_instances_are_independent() {
        let gate = Arc::new(SampleGate::new());
        let barrier = Arc::new(Barrier::new(16));
        let threads: Vec<_> = (0..16).map(|_| {
            let gate = gate.clone(); let barrier = barrier.clone();
            std::thread::spawn(move || { barrier.wait(); gate.admit(10, 64) })
        }).collect();
        assert_eq!(threads.into_iter().map(|thread| thread.join().unwrap()).filter(|won| *won).count(), 1);
        assert!(SampleGate::new().admit(10, 64));
        assert!(gate.admit(1010, 64));
    }

    #[test]
    fn large_allocations_retain_immediate_samples() {
        let gate = SampleGate::new();
        assert!(gate.admit(0, LARGE_ALLOCATION_BYTES));
        assert!(gate.admit(1, LARGE_ALLOCATION_BYTES));
        assert!(!gate.admit(1000, 64));
        assert!(gate.admit(1001, 64));
    }
}
