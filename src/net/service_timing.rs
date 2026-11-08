//! Bounded phase measurements for the shared network service.
//!
//! Socket readiness can enter the same service synchronously from Mio. Keep
//! those calls separate from the BSP task so one slow phase or repeated native
//! probes can be distinguished without logging each socket or packet.

use core::{
    arch::x86_64::_rdtsc,
    sync::atomic::{AtomicU64, Ordering},
};

const REPORT_NS: u64 = 5_000_000_000;

/// Phase boundaries use TSC cycles; wall time only paces the bounded report.
pub(super) struct Phases {
    pub sampled_at_ns: u64,
    pub started_cycles: u64,
    pub locked_cycles: u64,
    pub tick_done_cycles: u64,
    pub commands_done_cycles: u64,
    pub sockets_done_cycles: u64,
    pub commands: usize,
    pub sockets: usize,
    pub busy: bool,
}

struct Counters {
    report_at: AtomicU64,
    calls: AtomicU64,
    busy: AtomicU64,
    commands: AtomicU64,
    lock_cycles: AtomicU64,
    tick_cycles: AtomicU64,
    command_cycles: AtomicU64,
    socket_cycles: AtomicU64,
    max_cycles: AtomicU64,
    socket_peak: AtomicU64,
}

impl Counters {
    const fn new() -> Self {
        Self {
            report_at: AtomicU64::new(0),
            calls: AtomicU64::new(0),
            busy: AtomicU64::new(0),
            commands: AtomicU64::new(0),
            lock_cycles: AtomicU64::new(0),
            tick_cycles: AtomicU64::new(0),
            command_cycles: AtomicU64::new(0),
            socket_cycles: AtomicU64::new(0),
            max_cycles: AtomicU64::new(0),
            socket_peak: AtomicU64::new(0),
        }
    }
}

static TIMINGS: [[Counters; super::MAX_NET_DEVICES]; 2] =
    [const { [const { Counters::new() }; super::MAX_NET_DEVICES] }; 2];

/// Match BSP task profiling without the platform clock's millisecond quantization.
#[inline]
pub(super) fn timestamp() -> u64 {
    unsafe { _rdtsc() }
}

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
    counters.lock_cycles.fetch_add(
        phases.locked_cycles.saturating_sub(phases.started_cycles),
        Ordering::Relaxed,
    );
    counters.tick_cycles.fetch_add(
        phases.tick_done_cycles.saturating_sub(phases.locked_cycles),
        Ordering::Relaxed,
    );
    counters.command_cycles.fetch_add(
        phases
            .commands_done_cycles
            .saturating_sub(phases.tick_done_cycles),
        Ordering::Relaxed,
    );
    counters.socket_cycles.fetch_add(
        phases
            .sockets_done_cycles
            .saturating_sub(phases.commands_done_cycles),
        Ordering::Relaxed,
    );
    counters.max_cycles.fetch_max(
        phases
            .sockets_done_cycles
            .saturating_sub(phases.started_cycles),
        Ordering::Relaxed,
    );
    counters
        .socket_peak
        .fetch_max(phases.sockets as u64, Ordering::Relaxed);

    let previous = counters.report_at.load(Ordering::Relaxed);
    if previous == 0 {
        let _ = counters.report_at.compare_exchange(
            0,
            phases.sampled_at_ns,
            Ordering::Relaxed,
            Ordering::Relaxed,
        );
        return;
    }
    if phases.sampled_at_ns.saturating_sub(previous) < REPORT_NS
        || counters
            .report_at
            .compare_exchange(
                previous,
                phases.sampled_at_ns,
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
    let lock_cycles = take(&counters.lock_cycles);
    let tick_cycles = take(&counters.tick_cycles);
    let command_cycles = take(&counters.command_cycles);
    let socket_cycles = take(&counters.socket_cycles);
    let max_cycles = take(&counters.max_cycles);
    let socket_peak = take(&counters.socket_peak);
    let app_queues = super::APP_QUEUES.lock().len();
    let tsc_hz = crate::r::time::tsc_hz();
    let micros = |cycles: u64| {
        ((u128::from(cycles) * 1_000_000) / u128::from(tsc_hz.max(1))).min(u128::from(u64::MAX))
            as u64
    };
    crate::log_info!(target: "net";
        "net-service-timing dev={} origin={} dt_ms={} calls={} busy={} commands={} sockets_peak={} app_queues={} lock_us={} tick_us={} command_us={} socket_us={} max_us={}\n",
        device, if synchronous { "sync" } else { "task" },
        phases.sampled_at_ns.saturating_sub(previous) / 1_000_000,
        calls, busy, commands, socket_peak, app_queues,
        micros(lock_cycles), micros(tick_cycles), micros(command_cycles), micros(socket_cycles), micros(max_cycles),
    );
}
