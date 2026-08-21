use alloc::string::String;
use core::arch::x86_64::_rdtsc;
use core::cell::UnsafeCell;
use core::fmt::Write as _;

use trueos_executor::Spawner;
use embassy_sync::watch::Watch;
use trueos_time::{Duration, Instant, Timer};
use spin::Mutex;

const TASK_SLOTS: usize = crate::allcaps::executor::BSP_TASK_PROFILE_SLOTS;
const HISTORY_SLOTS: usize = crate::allcaps::executor::BSP_TASK_PROFILE_HISTORY_SLOTS;
const HISTORY_WATCHERS: usize = crate::allcaps::executor::BSP_TASK_PROFILE_WATCHERS;

#[derive(Clone, Copy)]
struct TaskEntry {
    id: usize,
    name: &'static str,
    polls: u64,
    total_cycles: u64,
    max_cycles: u64,
    slow_polls: u64,
}

const EMPTY_TASK_ENTRY: TaskEntry = TaskEntry {
    id: 0,
    name: "unregistered-task",
    polls: 0,
    total_cycles: 0,
    max_cycles: 0,
    slow_polls: 0,
};

struct ProfileState {
    bsp_executor_id: usize,
    active_executor_id: usize,
    active_task_id: usize,
    active_started_tsc: u64,
    slow_poll_cycles: u64,
    entries: [TaskEntry; TASK_SLOTS],
    total_polls: u64,
    total_cycles: u64,
    dropped_tasks: u64,
    mismatched_hooks: u64,
}

impl ProfileState {
    const fn new() -> Self {
        Self {
            bsp_executor_id: 0,
            active_executor_id: 0,
            active_task_id: 0,
            active_started_tsc: 0,
            slow_poll_cycles: 0,
            entries: [EMPTY_TASK_ENTRY; TASK_SLOTS],
            total_polls: 0,
            total_cycles: 0,
            dropped_tasks: 0,
            mismatched_hooks: 0,
        }
    }

    fn entry_index(&self, task_id: usize) -> Option<usize> {
        if task_id == 0 || TASK_SLOTS == 0 {
            return None;
        }
        let start = (task_id >> 4) % TASK_SLOTS;
        for offset in 0..TASK_SLOTS {
            let index = (start + offset) % TASK_SLOTS;
            let id = self.entries[index].id;
            if id == task_id || id == 0 {
                return Some(index);
            }
        }
        None
    }

    fn register_task(&mut self, task_id: usize, task_name: &'static str) -> bool {
        let Some(index) = self.entry_index(task_id) else {
            self.dropped_tasks = self.dropped_tasks.saturating_add(1);
            return false;
        };
        let is_new = self.entries[index].id == 0;
        if is_new {
            self.entries[index] = TaskEntry {
                id: task_id,
                name: task_name,
                ..EMPTY_TASK_ENTRY
            };
        } else {
            self.entries[index].name = task_name;
        }
        is_new
    }

    fn record_poll(&mut self, task_id: usize, elapsed_cycles: u64) {
        let Some(index) = self.entry_index(task_id) else {
            self.dropped_tasks = self.dropped_tasks.saturating_add(1);
            return;
        };
        if self.entries[index].id == 0 {
            self.entries[index].id = task_id;
        }
        let entry = &mut self.entries[index];
        entry.polls = entry.polls.saturating_add(1);
        entry.total_cycles = entry.total_cycles.saturating_add(elapsed_cycles);
        entry.max_cycles = entry.max_cycles.max(elapsed_cycles);
        if self.slow_poll_cycles != 0 && elapsed_cycles >= self.slow_poll_cycles {
            entry.slow_polls = entry.slow_polls.saturating_add(1);
        }
        self.total_polls = self.total_polls.saturating_add(1);
        self.total_cycles = self.total_cycles.saturating_add(elapsed_cycles);
    }
}

struct ProfileCell(UnsafeCell<ProfileState>);

// The state is touched only while CPU slot 0 is synchronously polling its local
// executor. Executor poll hooks do not run from interrupts or concurrently on BSP.
unsafe impl Sync for ProfileCell {}

static PROFILE: ProfileCell = ProfileCell(UnsafeCell::new(ProfileState::new()));

#[inline]
fn enabled_on_bsp() -> bool {
    crate::allcaps::executor::BSP_TASK_PROFILE_ENABLED && crate::percpu::current_slot() == 0
}

#[inline]
fn read_tsc() -> u64 {
    unsafe { _rdtsc() }
}

#[inline]
fn state_mut() -> &'static mut ProfileState {
    unsafe { &mut *PROFILE.0.get() }
}

fn clean_task_name(name: &'static str) -> &'static str {
    let name = name.strip_suffix('\0').unwrap_or(name);
    name.strip_suffix("::{{closure}}").unwrap_or(name)
}

#[unsafe(no_mangle)]
pub extern "Rust" fn __trueos_executor_task_new(
    executor_id: usize,
    task_id: usize,
    task_name: &'static str,
) {
    if !enabled_on_bsp() {
        return;
    }
    let state = state_mut();
    if state.bsp_executor_id == 0 {
        state.bsp_executor_id = executor_id;
    }
    if state.bsp_executor_id == executor_id {
        let task_name = clean_task_name(task_name);
        if state.register_task(task_id, task_name) {
            crate::log_trace!(target: "boot";
                "bsp-taskmon: registered executor=0x{:X} task=0x{:X} name={}\n",
                executor_id,
                task_id,
                task_name,
            );
        }
    }
}

#[unsafe(no_mangle)]
pub extern "Rust" fn __trueos_executor_task_poll_begin(executor_id: usize, task_id: usize) {
    if !enabled_on_bsp() {
        return;
    }
    let state = state_mut();
    if state.bsp_executor_id == 0 {
        state.bsp_executor_id = executor_id;
    }
    if state.bsp_executor_id != executor_id {
        return;
    }
    if state.active_task_id != 0 {
        state.mismatched_hooks = state.mismatched_hooks.saturating_add(1);
    }
    state.active_executor_id = executor_id;
    state.active_task_id = task_id;
    state.active_started_tsc = read_tsc();
}

#[unsafe(no_mangle)]
pub extern "Rust" fn __trueos_executor_task_poll_end(executor_id: usize, task_id: usize) {
    if !enabled_on_bsp() {
        return;
    }
    let finished_tsc = read_tsc();
    let state = state_mut();
    if state.bsp_executor_id != executor_id {
        return;
    }
    if state.active_executor_id != executor_id || state.active_task_id != task_id {
        state.mismatched_hooks = state.mismatched_hooks.saturating_add(1);
        state.active_executor_id = 0;
        state.active_task_id = 0;
        state.active_started_tsc = 0;
        return;
    }
    let elapsed_cycles = finished_tsc.wrapping_sub(state.active_started_tsc);
    state.active_executor_id = 0;
    state.active_task_id = 0;
    state.active_started_tsc = 0;
    state.record_poll(task_id, elapsed_cycles);
}

struct ProfileSnapshot {
    executor_id: usize,
    total_polls: u64,
    total_cycles: u64,
    top_total_name: &'static str,
    top_total_id: usize,
    top_total_cycles: u64,
    top_total_polls: u64,
    longest_name: &'static str,
    longest_id: usize,
    longest_cycles: u64,
    slow_polls: u64,
    dropped_tasks: u64,
    mismatched_hooks: u64,
}

#[derive(Clone, Copy)]
struct PublishedSnapshot {
    sequence: u64,
    now_ms: u64,
    heartbeat_gap_ms: u64,
    executor_id: usize,
    spawned: usize,
    ready: usize,
    polls: u64,
    busy_us: u64,
    busy_permille: u64,
    top_total_name: &'static str,
    top_total_id: usize,
    top_total_polls: u64,
    top_total_us: u64,
    longest_name: &'static str,
    longest_id: usize,
    longest_poll_us: u64,
    slow_polls: u64,
    dropped_tasks: u64,
    mismatched_hooks: u64,
    readiness: u32,
}

const EMPTY_PUBLISHED_SNAPSHOT: PublishedSnapshot = PublishedSnapshot {
    sequence: 0,
    now_ms: 0,
    heartbeat_gap_ms: 0,
    executor_id: 0,
    spawned: 0,
    ready: 0,
    polls: 0,
    busy_us: 0,
    busy_permille: 0,
    top_total_name: "none",
    top_total_id: 0,
    top_total_polls: 0,
    top_total_us: 0,
    longest_name: "none",
    longest_id: 0,
    longest_poll_us: 0,
    slow_polls: 0,
    dropped_tasks: 0,
    mismatched_hooks: 0,
    readiness: 0,
};

struct SnapshotHistory {
    entries: [PublishedSnapshot; HISTORY_SLOTS],
    next: usize,
    len: usize,
}

impl SnapshotHistory {
    const fn new() -> Self {
        Self {
            entries: [EMPTY_PUBLISHED_SNAPSHOT; HISTORY_SLOTS],
            next: 0,
            len: 0,
        }
    }

    fn push(&mut self, snapshot: PublishedSnapshot) {
        self.entries[self.next] = snapshot;
        self.next = (self.next + 1) % HISTORY_SLOTS;
        self.len = self.len.saturating_add(1).min(HISTORY_SLOTS);
    }
}

static SNAPSHOT_HISTORY: Mutex<SnapshotHistory> = Mutex::new(SnapshotHistory::new());

/// Sequence notification for low-cost in-kernel consumers. The history itself
/// stays in the bounded ring and can be read after a receiver observes a change.
pub static BSP_TASK_PROFILE_SEQUENCE_WATCH: Watch<
    crate::wait::EmbassySpinRawMutex,
    u64,
    HISTORY_WATCHERS,
> = Watch::new_with(0);

fn publish_snapshot(snapshot: PublishedSnapshot) {
    SNAPSHOT_HISTORY.lock().push(snapshot);
    BSP_TASK_PROFILE_SEQUENCE_WATCH
        .sender()
        .send(snapshot.sequence);
}

/// Append the shared chronological BSP execution history to a vlayer snapshot.
/// Recording is allocation-free; text exists only while a consumer reads it.
pub fn append_snapshot_history_text(out: &mut String) {
    let history = SNAPSHOT_HISTORY.lock();
    let _ = writeln!(out, "task_profile_history_capacity={HISTORY_SLOTS}");
    let _ = writeln!(out, "task_profile_history_count={}", history.len);
    let _ = writeln!(
        out,
        "task-profile\tsequence\tnow_ms\theartbeat_gap_ms\texecutor\tspawned\tready\tpolls\tbusy_us\tbusy_permille\ttop_task\ttop_task_id\ttop_polls\ttop_total_us\tlongest_task\tlongest_task_id\tlongest_poll_us\tslow_polls\tdropped\tmismatches\treadiness"
    );
    let start = (history.next + HISTORY_SLOTS - history.len) % HISTORY_SLOTS;
    for offset in 0..history.len {
        let snapshot = history.entries[(start + offset) % HISTORY_SLOTS];
        let _ = writeln!(
            out,
            "task-profile\t{}\t{}\t{}\t0x{:X}\t{}\t{}\t{}\t{}\t{}\t{}\t0x{:X}\t{}\t{}\t{}\t0x{:X}\t{}\t{}\t{}\t{}\t0x{:08X}",
            snapshot.sequence,
            snapshot.now_ms,
            snapshot.heartbeat_gap_ms,
            snapshot.executor_id,
            snapshot.spawned,
            snapshot.ready,
            snapshot.polls,
            snapshot.busy_us,
            snapshot.busy_permille,
            snapshot.top_total_name,
            snapshot.top_total_id,
            snapshot.top_total_polls,
            snapshot.top_total_us,
            snapshot.longest_name,
            snapshot.longest_id,
            snapshot.longest_poll_us,
            snapshot.slow_polls,
            snapshot.dropped_tasks,
            snapshot.mismatched_hooks,
            snapshot.readiness,
        );
    }
}

fn take_snapshot(tsc_hz: u64) -> ProfileSnapshot {
    let state = state_mut();
    state.slow_poll_cycles =
        cycles_from_us(crate::allcaps::executor::BSP_TASK_PROFILE_SLOW_POLL_US, tsc_hz);

    let mut snapshot = ProfileSnapshot {
        executor_id: state.bsp_executor_id,
        total_polls: state.total_polls,
        total_cycles: state.total_cycles,
        top_total_name: "none",
        top_total_id: 0,
        top_total_cycles: 0,
        top_total_polls: 0,
        longest_name: "none",
        longest_id: 0,
        longest_cycles: 0,
        slow_polls: 0,
        dropped_tasks: state.dropped_tasks,
        mismatched_hooks: state.mismatched_hooks,
    };

    for entry in state.entries.iter_mut() {
        if entry.polls != 0 && entry.total_cycles >= snapshot.top_total_cycles {
            snapshot.top_total_name = entry.name;
            snapshot.top_total_id = entry.id;
            snapshot.top_total_cycles = entry.total_cycles;
            snapshot.top_total_polls = entry.polls;
        }
        if entry.max_cycles >= snapshot.longest_cycles {
            snapshot.longest_name = entry.name;
            snapshot.longest_id = entry.id;
            snapshot.longest_cycles = entry.max_cycles;
        }
        snapshot.slow_polls = snapshot.slow_polls.saturating_add(entry.slow_polls);
        entry.polls = 0;
        entry.total_cycles = 0;
        entry.max_cycles = 0;
        entry.slow_polls = 0;
    }
    state.total_polls = 0;
    state.total_cycles = 0;
    state.dropped_tasks = 0;
    state.mismatched_hooks = 0;
    snapshot
}

#[inline]
fn cycles_from_us(us: u64, tsc_hz: u64) -> u64 {
    ((us as u128).saturating_mul(tsc_hz.max(1) as u128) / 1_000_000u128).min(u64::MAX as u128)
        as u64
}

#[inline]
fn cycles_to_us(cycles: u64, tsc_hz: u64) -> u64 {
    ((cycles as u128).saturating_mul(1_000_000u128) / tsc_hz.max(1) as u128).min(u64::MAX as u128)
        as u64
}

#[trueos_executor::task]
pub async fn bsp_task_profile_reporter_task(spawner: Spawner) {
    let report_ms = crate::allcaps::executor::BSP_TASK_PROFILE_REPORT_MS;
    let tsc_hz = crate::r::time::tsc_hz();
    let mut sequence = 0u64;
    let mut previous_ms = Instant::now().as_millis();

    // Configure the cycle threshold before the first complete reporting window.
    state_mut().slow_poll_cycles =
        cycles_from_us(crate::allcaps::executor::BSP_TASK_PROFILE_SLOW_POLL_US, tsc_hz);

    loop {
        Timer::after(Duration::from_millis(report_ms)).await;
        let now_ms = Instant::now().as_millis();
        let heartbeat_gap_ms = now_ms.saturating_sub(previous_ms);
        previous_ms = now_ms;
        sequence = sequence.saturating_add(1);

        let snapshot = take_snapshot(tsc_hz);
        let busy_us = cycles_to_us(snapshot.total_cycles, tsc_hz);
        let window_us = heartbeat_gap_ms.saturating_mul(1_000).max(1);
        let busy_permille = busy_us.saturating_mul(1_000) / window_us;
        let spawned = spawner.spawned_task_count();
        let ready = spawner.ready_task_count();
        let top_total_us = cycles_to_us(snapshot.top_total_cycles, tsc_hz);
        let longest_poll_us = cycles_to_us(snapshot.longest_cycles, tsc_hz);
        let readiness = crate::r::readiness::mask();
        publish_snapshot(PublishedSnapshot {
            sequence,
            now_ms,
            heartbeat_gap_ms,
            executor_id: snapshot.executor_id,
            spawned,
            ready,
            polls: snapshot.total_polls,
            busy_us,
            busy_permille,
            top_total_name: snapshot.top_total_name,
            top_total_id: snapshot.top_total_id,
            top_total_polls: snapshot.top_total_polls,
            top_total_us,
            longest_name: snapshot.longest_name,
            longest_id: snapshot.longest_id,
            longest_poll_us,
            slow_polls: snapshot.slow_polls,
            dropped_tasks: snapshot.dropped_tasks,
            mismatched_hooks: snapshot.mismatched_hooks,
            readiness,
        });
        crate::log_info!(target: "boot";
            "bsp-taskmon: seq={} now_ms={} heartbeat_gap_ms={} executor=0x{:X} spawned={} ready={} polls={} busy_us={} busy_pct={}.{} top_total={} task=0x{:X} polls={} total_us={} longest={} task=0x{:X} poll_us={} slow_polls_ge_{}us={} dropped={} mismatches={} readiness=0x{:08X}\n",
            sequence,
            now_ms,
            heartbeat_gap_ms,
            snapshot.executor_id,
            spawned,
            ready,
            snapshot.total_polls,
            busy_us,
            busy_permille / 10,
            busy_permille % 10,
            snapshot.top_total_name,
            snapshot.top_total_id,
            snapshot.top_total_polls,
            top_total_us,
            snapshot.longest_name,
            snapshot.longest_id,
            longest_poll_us,
            crate::allcaps::executor::BSP_TASK_PROFILE_SLOW_POLL_US,
            snapshot.slow_polls,
            snapshot.dropped_tasks,
            snapshot.mismatched_hooks,
            readiness,
        );
    }
}
