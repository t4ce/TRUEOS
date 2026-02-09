extern crate alloc;

use alloc::vec::Vec;
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicU64, Ordering};
use core::task::{Context, Poll};

use embassy_executor::{SendSpawner, SpawnError, SpawnToken, Spawner};
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::{Mutex, Once};

#[derive(Clone, Debug)]
struct TaskStat {
    name: &'static str,
    polls: u64,
    total_tsc: u64,
    last_tsc: u64,
}

static STATS: Once<Mutex<Vec<TaskStat>>> = Once::new();
static TSC_HZ: AtomicU64 = AtomicU64::new(0);
static INIT: Once<()> = Once::new();

fn stats() -> &'static Mutex<Vec<TaskStat>> {
    STATS.call_once(|| Mutex::new(Vec::new()))
}

fn init_once() {
    INIT.call_once(|| {
        let hz = detect_tsc_hz().max(1);
        TSC_HZ.store(hz, Ordering::Relaxed);
    });
}

#[allow(unused_unsafe)]
fn tsc_now() -> u64 {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::x86_64::_rdtsc() as u64
    }

    #[cfg(not(target_arch = "x86_64"))]
    {
        0
    }
}

fn detect_tsc_hz() -> u64 {
    #[cfg(target_arch = "x86_64")]
        let r15 = core::arch::x86_64::__cpuid(0x15);
        let denom = r15.eax as u64;
        let numer = r15.ebx as u64;
        let crystal_hz = r15.ecx as u64;
        if denom != 0 && numer != 0 && crystal_hz != 0 {
            return ((crystal_hz as u128) * (numer as u128) / (denom as u128)) as u64;
        }
        let r16 = core::arch::x86_64::__cpuid(0x16);
        let base_mhz = (r16.eax & 0xFFFF) as u64;
        if base_mhz != 0 {
            return base_mhz * 1_000_000;
        }
    1_000_000_000
}

#[allow(dead_code)]
fn record_poll(name: &'static str, delta_tsc: u64) {
    let mut stats = stats().lock();
    if let Some(entry) = stats.iter_mut().find(|s| s.name == name) {
        entry.polls = entry.polls.saturating_add(1);
        entry.total_tsc = entry.total_tsc.saturating_add(delta_tsc);
        entry.last_tsc = delta_tsc;
        return;
    }

    stats.push(TaskStat {
        name,
        polls: 1,
        total_tsc: delta_tsc,
        last_tsc: delta_tsc,
    });
}

#[allow(dead_code)]
pub struct Monitored<F> {
    name: &'static str,
    inner: F,
}

#[allow(dead_code)]
impl<F> Monitored<F> {
    pub fn new(name: &'static str, inner: F) -> Self {
        Self { name, inner }
    }
}

#[allow(dead_code)]
impl<F> Future for Monitored<F>
where
    F: Future,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let name = self.name;
        let start = tsc_now();
        // Safety: we never move `inner` after it is pinned.
        let inner = unsafe { self.map_unchecked_mut(|s| &mut s.inner) };
        let out = inner.poll(cx);
        let end = tsc_now();
        record_poll(name, end.saturating_sub(start));
        out
    }
}

#[allow(dead_code)]
pub fn wrap<F>(name: &'static str, fut: F) -> Monitored<F>
where
    F: Future,
{
    Monitored::new(name, fut)
}

pub fn spawn<T>(spawner: &Spawner, name: &'static str, token: SpawnToken<T>) -> Result<(), SpawnError> {
    register(name);
    spawner.spawn(token)
}

pub fn spawn_send<T: Send>(
    spawner: &SendSpawner,
    name: &'static str,
    token: SpawnToken<T>,
) -> Result<(), SpawnError> {
    register(name);
    spawner.spawn(token)
}

pub fn register(name: &'static str) {
    let mut stats = stats().lock();
    if stats.iter().any(|s| s.name == name) {
        return;
    }
    stats.push(TaskStat {
        name,
        polls: 0,
        total_tsc: 0,
        last_tsc: 0,
    });
}

pub async fn run<F>(name: &'static str, fut: F) -> F::Output
where
    F: Future,
{
    register(name);
    wrap(name, fut).await
}

#[embassy_executor::task]
pub async fn taskmon_reporter_task() {
    init_once();
    run("taskmon-reporter", async move {
        loop {
            Timer::after(EmbassyDuration::from_secs(10)).await;

            let hz = TSC_HZ.load(Ordering::Relaxed).max(1);
            let tsc_per_ms = hz / 1000;
            let mut lines = Vec::new();
            {
                let stats = stats().lock();
                for s in stats.iter() {
                    let avg_tsc = if s.polls == 0 {
                        0
                    } else {
                        s.total_tsc / s.polls
                    };
                    let last_ms = if tsc_per_ms == 0 {
                        0
                    } else {
                        s.last_tsc / tsc_per_ms
                    };
                    let avg_ms = if tsc_per_ms == 0 {
                        0
                    } else {
                        avg_tsc / tsc_per_ms
                    };
                    lines.push(alloc::format!(
                        "taskmon: {:<24} polls={} last_ms={} avg_ms={}\n",
                        s.name,
                        s.polls,
                        last_ms,
                        avg_ms
                    ));
                }
            }

            if lines.is_empty() {
                crate::log!("taskmon: no stats\n");
            } else {
                for line in lines {
                    crate::log!("{}", line.as_str());
                }
            }
        }
    })
    .await;
}
