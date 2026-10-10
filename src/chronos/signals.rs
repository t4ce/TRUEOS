//! Civil clock boundaries, published by one BSP task. Consumers own their work.
//!
//! `subscribe(Every::MINUTE)`, `Every::minutes(15)?`, or
//! `Every::hours(3)?.offset_seconds(30)?`. Intervals align to local midnight /
//! the civil epoch, rather than registration time; `.utc()` selects UTC.
//! Missed boundaries and wall-clock corrections coalesce into one latest event.
//! Dropping a subscription cancels it; the service retains only weak references.
use alloc::{sync::{Arc, Weak}, vec::Vec};
use spin::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Every {
    seconds: u64,
    offset: u64,
    local: bool,
}

impl Every {
    pub const MINUTE: Self = Self::new(60);
    pub const HOUR: Self = Self::new(3600);
    pub const THREE_HOURS: Self = Self::new(3 * 3600);
    pub const SIX_HOURS: Self = Self::new(6 * 3600);
    pub const TWELVE_HOURS: Self = Self::new(12 * 3600);

    const fn new(seconds: u64) -> Self { Self { seconds, offset: 0, local: true } }
    pub fn seconds(seconds: u64) -> Option<Self> {
        (seconds != 0).then(|| Self::new(seconds))
    }
    pub fn minutes(minutes: u64) -> Option<Self> { Self::seconds(minutes.checked_mul(60)?) }
    pub fn hours(hours: u64) -> Option<Self> { Self::seconds(hours.checked_mul(3600)?) }
    /// Shift each boundary, e.g. minute signals at :30 instead of :00.
    pub fn offset_seconds(mut self, offset: u64) -> Option<Self> {
        if offset >= self.seconds { return None; }
        self.offset = offset;
        Some(self)
    }
    pub const fn utc(mut self) -> Self { self.local = false; self }
    fn bucket(self, utc: u64, local: u64) -> u64 {
        (if self.local { local } else { utc }).saturating_sub(self.offset) / self.seconds
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tick {
    /// Current clock reading, not an accumulated count of missed events.
    pub unix_seconds: u64,
    pub local_seconds: u64,
}

struct Signal {
    pending: Mutex<Option<Tick>>,
    changed: crate::wait::WaitQueue,
}
struct Entry {
    every: Every,
    bucket: u64,
    signal: Weak<Signal>,
}
static SUBSCRIBERS: Mutex<Vec<Entry>> = Mutex::new(Vec::new());

pub struct Subscription { signal: Arc<Signal> }
impl Subscription {
    /// Consume the latest notification without blocking.
    pub fn take(&mut self) -> Option<Tick> { self.signal.pending.lock().take() }
    /// Cancellation-safe asynchronous notification for consumers without a pump.
    pub async fn wait(&mut self) -> Tick {
        loop {
            let observed = self.signal.changed.observe();
            if let Some(tick) = self.take() { return tick; }
            self.signal.changed.wait_after(observed).await;
        }
    }
}

fn current() -> Tick {
    let unix_seconds = super::best_effort_unix_time_seconds()
        .unwrap_or_else(crate::time::uptime_seconds);
    Tick { unix_seconds, local_seconds: crate::locale::local_unix_time_seconds(unix_seconds) }
}

/// Registration does not emit an initial event. Read the clock immediately if
/// needed, then receive the next aligned boundary (or wall-clock correction).
pub fn subscribe(every: Every) -> Subscription {
    let tick = current();
    let signal = Arc::new(Signal { pending: Mutex::new(None), changed: crate::wait::WaitQueue::new() });
    let mut entries = SUBSCRIBERS.lock();
    entries.retain(|entry| entry.signal.strong_count() != 0);
    entries.push(Entry { every, bucket: every.bucket(tick.unix_seconds, tick.local_seconds), signal: Arc::downgrade(&signal) });
    Subscription { signal }
}

fn publish(tick: Tick) {
    let mut wake = Vec::new();
    {
        let mut entries = SUBSCRIBERS.lock();
        entries.retain_mut(|entry| {
            let Some(signal) = entry.signal.upgrade() else { return false; };
            let bucket = entry.every.bucket(tick.unix_seconds, tick.local_seconds);
            if bucket != entry.bucket {
                entry.bucket = bucket;
                *signal.pending.lock() = Some(tick);
                wake.push(signal);
            }
            true
        });
    }
    // Wake other executors only after releasing the registration lock.
    for signal in wake { signal.changed.notify_all(); }
}

#[trueos_executor::task]
pub async fn service_task() {
    loop {
        publish(current());
        trueos_time::Timer::after(trueos_time::Duration::from_millis(100)).await;
    }
}
