//! Bounded host timing summaries. No per-byte logs or guest-owned allocations.
//! Each site/carrier emits its first observation, then at most once per 5 s
//! of activity. Every observation contributes to count/mean/max and tail bins.
use spin::Mutex;

const PERIOD_MS: u64 = 5_000;
pub(crate) fn now_ms() -> u64 {
    embassy_time_driver::now().saturating_mul(1000) / embassy_time_driver::TICK_HZ.max(1)
}

#[derive(Clone, Copy)]
struct Window {
    emitted: bool,
    since: u64,
    count: u64,
    total: u64,
    max: u64,
    over_10: u64,
    over_50: u64,
}
impl Window {
    const fn new() -> Self {
        Self {
            emitted: false,
            since: 0,
            count: 0,
            total: 0,
            max: 0,
            over_10: 0,
            over_50: 0,
        }
    }
    fn observe(&mut self, now: u64, elapsed: u64) -> Option<Self> {
        self.count = self.count.saturating_add(1);
        self.total = self.total.saturating_add(elapsed);
        self.max = self.max.max(elapsed);
        self.over_10 = self.over_10.saturating_add(u64::from(elapsed >= 10));
        self.over_50 = self.over_50.saturating_add(u64::from(elapsed >= 50));
        if self.emitted && now.saturating_sub(self.since) < PERIOD_MS {
            return None;
        }
        let result = *self;
        *self = Self {
            emitted: true,
            since: now,
            ..Self::new()
        };
        Some(result)
    }
}

pub(crate) struct Site {
    name: &'static str,
    windows: [Mutex<Window>; crate::allcaps::wls::CPU_TRACK_COUNT],
}
impl Site {
    pub(crate) const fn new(name: &'static str) -> Self {
        Self {
            name,
            windows: [const { Mutex::new(Window::new()) }; crate::allcaps::wls::CPU_TRACK_COUNT],
        }
    }
    pub(crate) fn record(&self, elapsed_ms: u64) {
        if !crate::allcaps::hv::INPUT_TIMING_ENABLED {
            return;
        }
        let slot = crate::percpu::current_slot();
        let Some(window) = self.windows.get(slot) else {
            return;
        };
        let now = now_ms();
        let summary = window.lock().observe(now, elapsed_ms);
        if let Some(summary) = summary {
            crate::log_info!(target: "blueprint";
                "input-timing site={} carrier={} n={} mean_ms={} max_ms={} ge10={} ge50={} period_ms={}\n",
                self.name, slot, summary.count, summary.total / summary.count,
                summary.max, summary.over_10, summary.over_50, PERIOD_MS);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn busy_sites_are_bounded_and_keep_tail_information() {
        let mut window = Window::new();
        assert_eq!(window.observe(100, 3).unwrap().count, 1);
        for now in 101..5100 {
            assert!(window.observe(now, 60).is_none());
        }
        let summary = window.observe(5100, 1).unwrap();
        assert_eq!(summary.count, 5000);
        assert_eq!(summary.max, 60);
        assert_eq!(summary.over_50, 4999);
        assert!(window.observe(5101, 0).is_none());
        assert_eq!(window.observe(20_000, 2).unwrap().count, 2);
    }
}
