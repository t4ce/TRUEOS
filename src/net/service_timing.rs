//! Bounded phase measurements for the shared network service.
//!
//! Socket readiness can enter the same service synchronously from Mio. Keep
//! those calls separate from the BSP task so one slow phase or repeated native
//! probes can be distinguished without logging each socket or packet.

use core::sync::atomic::{AtomicU64, Ordering};

const REPORT_NS: u64 = 5_000_000_000;

pub(super) struct Phases {
    pub started: u64,
    pub locked: u64,
    pub tick_done: u64,
    pub commands_done: u64,
    pub sockets_done: u64,
    pub commands: usize,
    pub sockets: usize,
    pub busy: bool,
}

struct Counters {
    report_at: AtomicU64,
    calls: AtomicU64,
    busy: AtomicU64,
    commands: AtomicU64,
    lock_ns: AtomicU64,
    tick_ns: AtomicU64,
    command_ns: AtomicU64,
    socket_ns: AtomicU64,
    max_ns: AtomicU64,
    socket_peak: AtomicU64,
}

impl Counters {
    const fn new() -> Self {
        Self {
            report_at: AtomicU64::new(0),
            calls: AtomicU64::new(0),
            busy: AtomicU64::new(0),
            commands: AtomicU64::new(0),
            lock_ns: AtomicU64::new(0),
            tick_ns: AtomicU64::new(0),
            command_ns: AtomicU64::new(0),
            socket_ns: AtomicU64::new(0),
            max_ns: AtomicU64::new(0),
            socket_peak: AtomicU64::new(0),
        }
    }
}

static TIMINGS: [[Counters; super::MAX_NET_DEVICES]; 2] =
    [const { [const { Counters::new() }; super::MAX_NET_DEVICES] }; 2];

pub(super) fn record(device: usize, synchronous: bool, phases: Phases) {
    let Some(counters) = TIMINGS[usize::from(synchronous)].get(device) else {
        return;
    };
    counters.calls.fetch_add(1, Ordering::Relaxed);
    counters
        .busy
        .fetch_add(u64::from(phases.busy), Ordering::Relaxed);
    counters
        .commands
        .fetch_add(phases.commands as u64, Ordering::Relaxed);
    counters.lock_ns.fetch_add(
        phases.locked.saturating_sub(phases.started),
        Ordering::Relaxed,
    );
    counters.tick_ns.fetch_add(
        phases.tick_done.saturating_sub(phases.locked),
        Ordering::Relaxed,
    );
    counters.command_ns.fetch_add(
        phases.commands_done.saturating_sub(phases.tick_done),
        Ordering::Relaxed,
    );
    counters.socket_ns.fetch_add(
        phases.sockets_done.saturating_sub(phases.commands_done),
        Ordering::Relaxed,
    );
    counters.max_ns.fetch_max(
        phases.sockets_done.saturating_sub(phases.started),
        Ordering::Relaxed,
    );
    counters
        .socket_peak
        .fetch_max(phases.sockets as u64, Ordering::Relaxed);

    let previous = counters.report_at.load(Ordering::Relaxed);
    if previous == 0 {
        let _ = counters.report_at.compare_exchange(
            0,
            phases.sockets_done,
            Ordering::Relaxed,
            Ordering::Relaxed,
        );
        return;
    }
    if phases.sockets_done.saturating_sub(previous) < REPORT_NS
        || counters
            .report_at
            .compare_exchange(
                previous,
                phases.sockets_done,
                Ordering::Relaxed,
                Ordering::Relaxed,
            )
            .is_err()
    {
        return;
    }

    // The service lock is released before recording. Atomics keep the observer
    // independent of callers on other cores; a racing call may land in either
    // adjacent sample, without losing its phase totals.
    let take = |counter: &AtomicU64| counter.swap(0, Ordering::Relaxed);
    let calls = take(&counters.calls);
    let busy = take(&counters.busy);
    let commands = take(&counters.commands);
    let lock_ns = take(&counters.lock_ns);
    let tick_ns = take(&counters.tick_ns);
    let command_ns = take(&counters.command_ns);
    let socket_ns = take(&counters.socket_ns);
    let max_ns = take(&counters.max_ns);
    let socket_peak = take(&counters.socket_peak);
    let app_queues = super::APP_QUEUES.lock().len();
    crate::log_info!(target: "net";
        "net-service-timing dev={} origin={} dt_ms={} calls={} busy={} commands={} sockets_peak={} app_queues={} lock_us={} tick_us={} command_us={} socket_us={} max_us={}\n",
        device, if synchronous { "sync" } else { "task" },
        phases.sockets_done.saturating_sub(previous) / 1_000_000,
        calls, busy, commands, socket_peak, app_queues,
        lock_ns / 1_000, tick_ns / 1_000, command_ns / 1_000, socket_ns / 1_000, max_ns / 1_000,
    );
}
