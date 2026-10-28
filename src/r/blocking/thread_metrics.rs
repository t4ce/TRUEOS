//! Fixed-size passive timing counters. Writers only touch atomics; reporting
//! neither resets counters nor participates in scheduling or guest lifetime.
use core::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub count: u64,
    pub total_ms: u64,
    pub max_ms: u64,
    pub ge10: u64,
    pub ge50: u64,
}

pub(crate) struct Metric {
    count: AtomicU64,
    total_ms: AtomicU64,
    max_ms: AtomicU64,
    ge10: AtomicU64,
    ge50: AtomicU64,
}

impl Metric {
    pub(crate) const fn new() -> Self {
        Self {
            count: AtomicU64::new(0), total_ms: AtomicU64::new(0),
            max_ms: AtomicU64::new(0), ge10: AtomicU64::new(0), ge50: AtomicU64::new(0),
        }
    }

    pub(crate) fn record(&self, elapsed_ms: u64) {
        self.total_ms.fetch_add(elapsed_ms, Ordering::Relaxed);
        self.max_ms.fetch_max(elapsed_ms, Ordering::Relaxed);
        if elapsed_ms >= 10 { self.ge10.fetch_add(1, Ordering::Relaxed); }
        if elapsed_ms >= 50 { self.ge50.fetch_add(1, Ordering::Relaxed); }
        self.count.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn snapshot(&self) -> Snapshot {
        Snapshot {
            count: self.count.load(Ordering::Relaxed),
            total_ms: self.total_ms.load(Ordering::Relaxed),
            max_ms: self.max_ms.load(Ordering::Relaxed),
            ge10: self.ge10.load(Ordering::Relaxed),
            ge50: self.ge50.load(Ordering::Relaxed),
        }
    }
}

pub(crate) const ADMISSION: usize = 0;
pub(crate) const READY: usize = 1;
pub(crate) const SLICE: usize = 2;
pub(crate) const PARK: usize = 3;
pub(crate) const OVERRUN: usize = 4;
pub(crate) const METRIC_COUNT: usize = 5;
pub(crate) const ROLE_COUNT: usize = 4;

pub(crate) fn role_name(role: u32) -> &'static str {
    match role {
        1 => "foreground", 2 => "tokio-pool", 3 => "dedicated-blocking", _ => "unclassified",
    }
}

pub(crate) fn activity_name(activity: u32) -> &'static str {
    match activity {
        1 => "foreground", 2 => "async-scheduler", 3 => "blocking", _ => "unknown",
    }
}

pub(crate) struct Counters {
    metrics: [Metric; METRIC_COUNT],
    retired: AtomicU64,
}

impl Counters {
    pub(crate) const fn new() -> Self {
        Self { metrics: [const { Metric::new() }; METRIC_COUNT], retired: AtomicU64::new(0) }
    }

    pub(crate) fn record(&self, metric: usize, elapsed_ms: u64) {
        if let Some(metric) = self.metrics.get(metric) { metric.record(elapsed_ms); }
    }

    pub(crate) fn metric(&self, metric: usize) -> Snapshot {
        self.metrics.get(metric).map_or_else(Snapshot::default, Metric::snapshot)
    }

    pub(crate) fn retire(&self) { self.retired.fetch_add(1, Ordering::Relaxed); }
    pub(crate) fn retired(&self) -> u64 { self.retired.load(Ordering::Relaxed) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_preserve_tail_thresholds_and_metric_separation() {
        let counters = Counters::new();
        for elapsed in [0, 9, 10, 49, 50, 3000] { counters.record(SLICE, elapsed); }
        assert_eq!(counters.metric(SLICE), Snapshot {
            count: 6, total_ms: 3118, max_ms: 3000, ge10: 4, ge50: 2,
        });
        assert_eq!(counters.metric(READY), Snapshot::default());
        counters.record(usize::MAX, 100);
        assert_eq!(counters.metric(usize::MAX), Snapshot::default());
        counters.retire();
        assert_eq!(counters.retired(), 1);
    }

    #[test]
    fn shared_carrier_writers_do_not_lose_observations() {
        let counters = alloc::sync::Arc::new(Counters::new());
        std::thread::scope(|scope| {
            for _ in 0..4 {
                let counters = counters.clone();
                scope.spawn(move || {
                    for _ in 0..1000 { counters.record(READY, 50); }
                });
            }
        });
        assert_eq!(counters.metric(READY), Snapshot {
            count: 4000, total_ms: 200_000, max_ms: 50, ge10: 4000, ge50: 4000,
        });
    }
}
