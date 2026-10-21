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

    const fn new(seconds: u64) -> Self { Self { seconds, offset: 0, local: true } }
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
