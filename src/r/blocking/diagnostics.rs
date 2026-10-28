//! Host-owned lifetime records. No guest pointers are dereferenced by reports.
use alloc::{sync::Arc, vec::Vec, string::String};
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use spin::Mutex;

pub(crate) const QUEUED: usize = 0;
pub(crate) const RUNNING: usize = 1;
pub(crate) const YIELDED: usize = 2;
pub(crate) const WAITING: usize = 3;
pub(crate) const SLEEPING: usize = 4;
pub(crate) const RETAINED: usize = 5;
pub(crate) const CANCELLING: usize = 6;
pub(crate) const COMPLETED: usize = 7;
pub(crate) const POLL_BEGIN: usize = 0;
pub(crate) const CONTROL_CHECK: usize = 1;
pub(crate) const WAIT_POLL: usize = 2;
pub(crate) const WAIT_PENDING: usize = 3;
pub(crate) const CONTEXT_INSTALL: usize = 4;
pub(crate) const GUEST_ENTER: usize = 5;
pub(crate) const GUEST_RETURN: usize = 6;
pub(crate) const CONTEXT_RESTORE: usize = 7;
pub(crate) const CONTEXT_RESTORED: usize = 8;
pub(crate) const POLL_RETURN: usize = 9;

#[path = "abi_breadcrumb.rs"] mod abi_breadcrumb;
pub(crate) use abi_breadcrumb::{AbiOperation, Guard as AbiGuard};
#[path = "thread_metrics.rs"] mod thread_metrics;

// Deliberately attributed to admission role and carrier, not the current
// Tokio activity. A Tokio pool thread can run a scheduler or blocking work.
// Since-boot totals retain short-lived jobs after their roster entry retires.
static TIMING_TOTALS: [[thread_metrics::Counters; thread_metrics::ROLE_COUNT]; crate::allcaps::hv::VM_CPU_SLOT_LIMIT] =
    [const { [const { thread_metrics::Counters::new() }; thread_metrics::ROLE_COUNT] }; crate::allcaps::hv::VM_CPU_SLOT_LIMIT];

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static TIMING_RETIREMENTS: AtomicU64 = AtomicU64::new(0);
static RECORDS: [Mutex<Vec<Arc<Record>>>; crate::allcaps::hv::VM_ID_LIMIT] =
    [const { Mutex::new(Vec::new()) }; crate::allcaps::hv::VM_ID_LIMIT];

fn now_ms() -> u64 {
    embassy_time_driver::now().saturating_mul(1000) / embassy_time_driver::TICK_HZ.max(1)
}

struct Record {
    abi: abi_breadcrumb::State,
    id: u64,
    generation: u64,
    identity: Mutex<(&'static str, usize, heapless::String<31>)>,
    carrier: AtomicUsize,
    role: AtomicUsize,
    activity: AtomicUsize,
    timing: thread_metrics::Counters,
    phase: AtomicUsize,
    since_ms: AtomicU64,
    boundary: AtomicU64,
    wait_object: AtomicUsize,
    timeout_ms: AtomicU64,
    progress_enabled: AtomicUsize,
    progress_stage: AtomicUsize,
    progress_since_ms: AtomicU64,
    progress_sequence: AtomicU64,
    progress_if: AtomicUsize,
}

pub(crate) struct JobDiagnostic {
    vm_id: u8,
    record: Arc<Record>,
}

impl JobDiagnostic {
    pub(crate) fn new(vm_id: u8, generation: u64) -> Self {
        crate::allocators::with_host_alloc_domain_strong(|| {
            let record = Arc::new(Record {
                abi: abi_breadcrumb::State::new(),
                id: NEXT_ID.fetch_add(1, Ordering::Relaxed), generation,
                identity: Mutex::new(("native", 0, heapless::String::new())),
                carrier: AtomicUsize::new(usize::MAX), phase: AtomicUsize::new(QUEUED),
                role: AtomicUsize::new(0), activity: AtomicUsize::new(0),
                timing: thread_metrics::Counters::new(),
                since_ms: AtomicU64::new(now_ms()), boundary: AtomicU64::new(0),
                wait_object: AtomicUsize::new(0), timeout_ms: AtomicU64::new(0),
                progress_enabled: AtomicUsize::new(0), progress_stage: AtomicUsize::new(POLL_BEGIN),
                progress_since_ms: AtomicU64::new(now_ms()), progress_sequence: AtomicU64::new(0),
                progress_if: AtomicUsize::new(0),
            });
            RECORDS[vm_id as usize].lock().push(record.clone());
            Self { vm_id, record }
        })
    }

    /// # Safety
    /// Guard must remain on the owner Thread stack and never outlive the record.
    pub(crate) unsafe fn abi_scope(&self, operation: AbiOperation) -> AbiGuard {
        if self.record.progress_enabled.load(Ordering::Relaxed) == 0 {
            return AbiGuard::inactive();
        }
        // Record is owned by the pinned Thread's admission. A boundary guard
        // lives on that Thread's child stack; task retirement discards the
        // stack before releasing admission, including on forced cancellation.
        unsafe { self.record.abi.enter(operation, now_ms()) }
    }

    pub(crate) fn identify(&self, kind: &'static str, id: usize, carrier: usize) {
        let mut identity = self.record.identity.lock();
        identity.0 = kind;
        identity.1 = id;
        self.record.carrier.store(carrier, Ordering::Relaxed);
    }

    pub(crate) fn name(&self, name: &str) {
        let mut identity = self.record.identity.lock();
        identity.2.clear();
        for ch in name.chars() {
            if identity.2.push(ch).is_err() { break; }
        }
    }

    /// Immutable placement hint, set before the first continuation poll.
    pub(crate) fn role(&self, role: u32) {
        self.record.role.store(if role < thread_metrics::ROLE_COUNT as u32 { role as usize } else { 0 }, Ordering::Relaxed);
    }

    /// Runtime activity is diagnostic metadata, never an admission decision.
    pub(crate) fn activity(&self, activity: u32) -> u32 {
        self.record.activity.swap(activity as usize, Ordering::Relaxed) as u32
    }

    pub(crate) fn timing_probe(&self) -> TimingProbe {
        TimingProbe { record: self.record.clone() }
    }

    pub(crate) fn phase(&self, phase: usize, wait: usize, timeout: u64) {
        // Only the owning task writes. Reports are approximate observations,
        // never synchronization or authority to reclaim guest resources.
        self.record.wait_object.store(wait, Ordering::Relaxed);
        self.record.timeout_ms.store(timeout, Ordering::Relaxed);
        self.record.since_ms.store(now_ms(), Ordering::Relaxed);
        self.record.boundary.fetch_add(1, Ordering::Relaxed);
        self.record.phase.store(phase, Ordering::Release);
    }

    pub(crate) fn enable_progress_probe(&self) {
        self.record.progress_enabled.store(1, Ordering::Release);
    }

    pub(crate) fn progress(&self, stage: usize) {
        if !crate::allcaps::hv::CODEX_PROGRESS_PROBES_ENABLED
            || self.record.progress_enabled.load(Ordering::Relaxed) == 0 { return; }
        // Odd/even sequence lets the independent observer reject a changing
        // snapshot. The owner writes only scalars; no locks/logging here.
        self.record.progress_sequence.fetch_add(1, Ordering::AcqRel);
        self.record.progress_if.store(usize::from(crate::r::progress_probe::interrupts_enabled()), Ordering::Relaxed);
        self.record.progress_since_ms.store(now_ms(), Ordering::Relaxed);
        self.record.progress_stage.store(stage, Ordering::Relaxed);
        self.record.progress_sequence.fetch_add(1, Ordering::Release);
    }
}

#[derive(Clone)]
pub(crate) struct TimingProbe {
    record: Arc<Record>,
}

impl TimingProbe {
    fn record(&self, metric: usize, elapsed_ms: u64) {
        self.record.timing.record(metric, elapsed_ms);
        let carrier = self.record.carrier.load(Ordering::Relaxed);
        let role = self.record.role.load(Ordering::Relaxed);
        if let Some(total) = TIMING_TOTALS.get(carrier).and_then(|roles| roles.get(role)) {
            total.record(metric, elapsed_ms);
        }
    }

    pub(crate) fn polled(&self, elapsed_ms: u64, admission: bool) {
        self.record(if admission { thread_metrics::ADMISSION } else { thread_metrics::READY }, elapsed_ms);
    }

    pub(crate) fn ran(&self, elapsed_ms: u64) { self.record(thread_metrics::SLICE, elapsed_ms); }

    pub(crate) fn parked(&self, elapsed_ms: u64, requested_ms: Option<u64>) {
        self.record(thread_metrics::PARK, elapsed_ms);
        if let Some(requested) = requested_ms {
            self.record(thread_metrics::OVERRUN, elapsed_ms.saturating_sub(requested));
        }
    }
}

/// A compact, fixed topology snapshot. Busy rosters are skipped rather than
/// allowing a stalled bookkeeping lock to stop the independent observer.
pub(crate) struct CarrierSnapshot {
    pub active: [bool; crate::allcaps::hv::VM_CPU_SLOT_LIMIT],
    pub longest_running: Option<usize>,
    pub busy_rosters: usize,
}

pub(crate) fn progress_carriers() -> CarrierSnapshot {
    let mut snapshot = CarrierSnapshot {
        active: [false; crate::allcaps::hv::VM_CPU_SLOT_LIMIT],
        longest_running: None, busy_rosters: 0,
    };
    let now = now_ms();
    let mut longest_age = 0;
    for roster in &RECORDS {
        let Some(records) = roster.try_lock() else {
            snapshot.busy_rosters += 1;
            continue;
        };
        for record in records.iter() {
            if record.progress_enabled.load(Ordering::Acquire) == 0 { continue; }
            let slot = record.carrier.load(Ordering::Relaxed);
            let Some(active) = snapshot.active.get_mut(slot) else { continue; };
            *active = true;
            if record.phase.load(Ordering::Acquire) == RUNNING {
                let age = now.saturating_sub(record.progress_since_ms.load(Ordering::Relaxed));
                if snapshot.longest_running.is_none() || age > longest_age {
                    longest_age = age;
                    snapshot.longest_running = Some(slot);
                }
            }
        }
    }
    snapshot
}

/// At most `limit` role/carrier aggregate lines; rotate through populated
/// buckets so completed startup jobs remain observable with a fixed log cost.
pub(crate) fn timing_lines(cursor: &mut usize, limit: usize) -> Vec<String> {
    let mut sweep = TimingSweep::new();
    sweep.restart();
    sweep.lines(cursor, limit)
}

/// A finite pass over all aggregate buckets. Once idle, continue the pass over
/// subsequent bounded samples instead of exporting only its first four rows.
pub(crate) struct TimingSweep { remaining: usize }

impl TimingSweep {
    pub(crate) const fn new() -> Self { Self { remaining: 0 } }

    pub(crate) fn restart(&mut self) {
        self.remaining = TIMING_TOTALS.len() * thread_metrics::ROLE_COUNT;
    }

    pub(crate) fn lines(&mut self, cursor: &mut usize, limit: usize) -> Vec<String> {
        let capacity = TIMING_TOTALS.len() * thread_metrics::ROLE_COUNT;
        let mut out = Vec::new();
        while self.remaining != 0 && out.len() < limit {
            let index = *cursor % capacity;
            *cursor = (index + 1) % capacity;
            self.remaining -= 1;
            let carrier = index / thread_metrics::ROLE_COUNT;
            let role = index % thread_metrics::ROLE_COUNT;
            let counters = &TIMING_TOTALS[carrier][role];
            let admission = counters.metric(thread_metrics::ADMISSION);
            if admission.count == 0 && counters.retired() == 0 { continue; }
            out.push(timing_line("codex-progress-timing scope=since-boot", carrier, role, counters));
        }
        out
    }
}

pub(crate) fn timing_retirements() -> u64 { TIMING_RETIREMENTS.load(Ordering::Acquire) }

fn timing_line(prefix: &str, carrier: usize, role: usize, counters: &thread_metrics::Counters) -> String {
    let admission = counters.metric(thread_metrics::ADMISSION);
    let ready = counters.metric(thread_metrics::READY);
    let slice = counters.metric(thread_metrics::SLICE);
    let park = counters.metric(thread_metrics::PARK);
    let overrun = counters.metric(thread_metrics::OVERRUN);
    alloc::format!(
        "{} carrier={} role={} admitted={} retired={} admission_max_ms={} ready_n={} ready_max_ms={} ready_ge10={} ready_ge50={} slice_n={} slice_max_ms={} slice_ge10={} slice_ge50={} park_n={} park_max_ms={} overrun_n={} overrun_max_ms={}",
        prefix, carrier, thread_metrics::role_name(role as u32), admission.count, counters.retired(), admission.max_ms,
        ready.count, ready.max_ms, ready.ge10, ready.ge50, slice.count, slice.max_ms, slice.ge10, slice.ge50,
        park.count, park.max_ms, overrun.count, overrun.max_ms,
    )
}

fn progress_stage_name(stage: usize) -> &'static str {
    match stage {
        POLL_BEGIN => "poll-begin", CONTROL_CHECK => "control-check",
        WAIT_POLL => "wait-poll", WAIT_PENDING => "wait-pending",
        CONTEXT_INSTALL => "context-install", GUEST_ENTER => "guest-enter",
        GUEST_RETURN => "guest-return", CONTEXT_RESTORE => "context-restore",
        CONTEXT_RESTORED => "context-restored", POLL_RETURN => "poll-return",
        _ => "unknown",
    }
}

/// Read-only, nonblocking roster snapshots. Prioritize the running record so
/// a bounded report cannot hide it behind a crowd of sleeping workers.
pub(crate) fn progress_lines(vm_id: u8, limit: usize) -> Vec<String> {
    if limit == 0 { return Vec::new(); }
    let Some(roster) = RECORDS.get(vm_id as usize) else { return Vec::new(); };
    let Some(records) = roster.try_lock() else {
        return alloc::vec![alloc::format!("codex-progress-job vm={} roster=busy", vm_id)];
    };
    let selected: Vec<_> = records.iter()
        .filter(|record| record.progress_enabled.load(Ordering::Acquire) != 0)
        .cloned().collect();
    drop(records);
    let mut out = Vec::new();
    let now = now_ms();
    for running in [true, false] {
        for record in &selected {
            if out.len() >= limit { break; }
            let phase = record.phase.load(Ordering::Acquire);
            if (phase == RUNNING) != running { continue; }
            out.push(progress_record_line(vm_id, record, now));
        }
    }
    out
}

fn progress_record_line(vm_id: u8, record: &Record, now: u64) -> String {
    let phase = record.phase.load(Ordering::Acquire);
    let Some(identity) = record.identity.try_lock() else {
        return alloc::format!("codex-progress-job vm={} job={} identity=busy", vm_id, record.id);
    };
    let sequence = record.progress_sequence.load(Ordering::Acquire);
    let stage = record.progress_stage.load(Ordering::Relaxed);
    let age = now.saturating_sub(record.progress_since_ms.load(Ordering::Relaxed));
    let enabled = record.progress_if.load(Ordering::Relaxed);
    core::sync::atomic::fence(Ordering::Acquire);
    if sequence & 1 != 0 || sequence != record.progress_sequence.load(Ordering::Acquire) {
        return alloc::format!("codex-progress-job vm={} id={} snapshot=changing", vm_id, identity.1);
    }
    let abi = record.abi.snapshot();
    let abi_line = if let Some(abi) = abi {
        alloc::format!(" abi={} abi_age_ms={} abi_since_ms={} abi_depth={} abi_entered={} abi_exited={} abi_seq={} abi_last={} abi_last_exit_ms={}",
            AbiOperation::name(abi.operation),
            if abi.operation == 0 { 0 } else { now.saturating_sub(abi.since_ms) },
            abi.since_ms, abi.depth, abi.entered, abi.exited, abi.sequence,
            AbiOperation::name(abi.last), abi.last_exit_ms)
    } else { alloc::string::String::from(" abi=changing") };
    let admission = record.timing.metric(thread_metrics::ADMISSION);
    let ready = record.timing.metric(thread_metrics::READY);
    let slice = record.timing.metric(thread_metrics::SLICE);
    let park = record.timing.metric(thread_metrics::PARK);
    let overrun = record.timing.metric(thread_metrics::OVERRUN);
    alloc::format!(
        "codex-progress-job vm={} gen={} id={} carrier={} stage={} age_ms={} seq={} if={} phase={} wait=0x{:X} name={} role={} activity={} admission_ms={} ready_max_ms={} slice_max_ms={} park_max_ms={} overrun_max_ms={}{}",
        vm_id, record.generation, identity.1, record.carrier.load(Ordering::Relaxed),
        progress_stage_name(stage), age, sequence, enabled, phase,
        record.wait_object.load(Ordering::Relaxed),
        if identity.2.is_empty() { "-" } else { identity.2.as_str() },
        thread_metrics::role_name(record.role.load(Ordering::Relaxed) as u32),
        thread_metrics::activity_name(record.activity.load(Ordering::Relaxed) as u32),
        admission.max_ms, ready.max_ms, slice.max_ms, park.max_ms, overrun.max_ms, abi_line,
    )
}

/// Global budget and cursor avoid low VM IDs or sleeping workers monopolizing
/// reports. Keep the oldest running boundary, then rotate through all jobs.
pub(crate) fn sample_progress_lines(cursor: &mut u64, limit: usize) -> Vec<String> {
    if limit == 0 { return Vec::new(); }
    let mut selected = Vec::new();
    for (vm_id, roster) in RECORDS.iter().enumerate() {
        let Some(records) = roster.try_lock() else { continue; };
        selected.extend(records.iter()
            .filter(|record| record.progress_enabled.load(Ordering::Acquire) != 0)
            .map(|record| (vm_id as u8, record.clone())));
    }
    selected.sort_unstable_by_key(|(_, record)| record.id);
    let now = now_ms();
    let priority = selected.iter()
        .filter(|(_, record)| record.phase.load(Ordering::Acquire) == RUNNING)
        .max_by_key(|(_, record)| now.saturating_sub(record.progress_since_ms.load(Ordering::Relaxed)))
        .map(|(_, record)| record.id);
    let mut out = Vec::new();
    if let Some((vm_id, record)) = selected.iter().find(|(_, record)| Some(record.id) == priority) {
        out.push(progress_record_line(*vm_id, record, now));
    }
    let start = selected.partition_point(|(_, record)| record.id <= *cursor);
    for offset in 0..selected.len() {
        if out.len() >= limit { break; }
        let (vm_id, record) = &selected[(start + offset) % selected.len()];
        if Some(record.id) == priority { continue; }
        out.push(progress_record_line(*vm_id, record, now));
        *cursor = record.id;
    }
    out
}

impl Drop for JobDiagnostic {
    fn drop(&mut self) {
        if self.record.progress_enabled.load(Ordering::Relaxed) != 0 {
            self.record.timing.retire();
            if let Some(total) = TIMING_TOTALS.get(self.record.carrier.load(Ordering::Relaxed))
                .and_then(|roles| roles.get(self.record.role.load(Ordering::Relaxed))) {
                total.retire();
            }
            // Publish the retired bucket before the observer begins its
            // finite idle sweep, including on weakly ordered carriers.
            TIMING_RETIREMENTS.fetch_add(1, Ordering::Release);
        }
        crate::allocators::with_host_alloc_domain_strong(|| {
            RECORDS[self.vm_id as usize].lock().retain(|record| record.id != self.record.id);
        });
    }
}

pub(crate) fn lines(vm_id: u8) -> Vec<String> {
    let Some(records) = RECORDS.get(vm_id as usize) else { return Vec::new(); };
    // Clone under the host-only roster lock, then format without it. A running
    // guest cannot hold that lock while a control-plane report waits on it.
    let records = records.lock().clone();
    let mut out = Vec::new();
    let now = now_ms();
    for record in records.iter().take(32) {
        let phase = match record.phase.load(Ordering::Acquire) {
            QUEUED => "queued", RUNNING => "running", YIELDED => "yielded",
            WAITING => "wait-queue", SLEEPING => "sleep", RETAINED => "realm-unavailable",
            CANCELLING => "cancelling", COMPLETED => "completed", _ => "unknown",
        };
        let identity = record.identity.lock();
        out.push(alloc::format!(
            "vm{} job={} generation={} kind={} id={} name={} carrier={} phase={} age_ms={} boundary={} wait=0x{:X} timeout_ms={}",
            vm_id, record.id, record.generation, identity.0, identity.1,
            if identity.2.is_empty() { "-" } else { identity.2.as_str() },
            record.carrier.load(Ordering::Relaxed), phase,
            now.saturating_sub(record.since_ms.load(Ordering::Relaxed)),
            record.boundary.load(Ordering::Relaxed),
            record.wait_object.load(Ordering::Relaxed), record.timeout_ms.load(Ordering::Relaxed),
        ));
    }
    if records.len() > 32 {
        out.push(alloc::format!("vm{} jobs omitted={}", vm_id, records.len() - 32));
    }
    out
}

pub(crate) fn report(vm_id: u8) {
    for line in lines(vm_id) {
        crate::log_os::blueprint_important_line(format_args!("native-worker: {}\n", line));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_timing_sweep_exports_more_than_one_sample_then_stops() {
        for carrier in 52..58 {
            let job = JobDiagnostic::new(7, 301);
            job.identify("std-thread", 1000 + carrier, carrier);
            job.role(3);
            job.enable_progress_probe();
            job.timing_probe().polled(1, true);
            drop(job);
        }
        let mut sweep = TimingSweep::new();
        let mut cursor = 52 * thread_metrics::ROLE_COUNT + 3;
        assert!(sweep.lines(&mut cursor, 4).is_empty());
        sweep.restart();
        assert!(sweep.lines(&mut cursor, 0).is_empty());
        let mut rows = Vec::new();
        let mut samples = 0;
        while sweep.remaining != 0 {
            let sample = sweep.lines(&mut cursor, 4);
            assert!(sample.len() <= 4);
            rows.extend(sample);
            samples += 1;
            assert!(samples <= TIMING_TOTALS.len() * thread_metrics::ROLE_COUNT / 4 + 1);
        }
        assert!(samples >= 2);
        for carrier in 52..58 {
            let marker = alloc::format!("carrier={} role=dedicated-blocking", carrier);
            assert_eq!(rows.iter().filter(|row| row.contains(&marker)).count(), 1);
        }
        for _ in 0..3 { assert!(sweep.lines(&mut cursor, 4).is_empty()); }
        sweep.restart();
        assert!(!sweep.lines(&mut cursor, 4).is_empty());
    }

    #[test]
    fn per_generation_metrics_are_fresh_and_retired_work_remains_in_carrier_totals() {
        let carrier = 61;
        let role = 2;
        let baseline = TIMING_TOTALS[carrier][role].metric(thread_metrics::ADMISSION).count;
        let retired = TIMING_TOTALS[carrier][role].retired();
        let job = JobDiagnostic::new(5, 101);
        job.identify("std-thread", 901, carrier);
        job.role(role as u32);
        job.activity(2);
        job.enable_progress_probe();
        let probe = job.timing_probe();
        probe.polled(13, true);
        probe.polled(47, false);
        probe.ran(3_000);
        probe.parked(270, Some(250));
        let line = progress_lines(5, 1).remove(0);
        assert!(line.contains("gen=101 id=901 carrier=61"));
        assert!(line.contains("role=tokio-pool activity=async-scheduler admission_ms=13 ready_max_ms=47 slice_max_ms=3000 park_max_ms=270 overrun_max_ms=20"));
        drop(job);
        assert!(progress_lines(5, 1).is_empty());
        let mut cursor = carrier * thread_metrics::ROLE_COUNT + role;
        let totals = timing_lines(&mut cursor, 1).remove(0);
        assert!(totals.contains("scope=since-boot carrier=61 role=tokio-pool"));
        assert!(totals.contains("ready_max_ms=47 ready_ge10=1 ready_ge50=0"));
        assert_eq!(TIMING_TOTALS[carrier][role].metric(thread_metrics::ADMISSION).count, baseline + 1);
        assert_eq!(TIMING_TOTALS[carrier][role].retired(), retired + 1);
        let next = JobDiagnostic::new(5, 102);
        next.identify("std-thread", 902, carrier);
        next.role(role as u32);
        next.enable_progress_probe();
        let fresh = progress_lines(5, 1).remove(0);
        assert!(fresh.contains("gen=102 id=902"));
        assert!(fresh.contains("admission_ms=0 ready_max_ms=0 slice_max_ms=0 park_max_ms=0 overrun_max_ms=0"));
    }

    #[test]
    fn observer_discovers_non_lastap_carriers_without_waiting_for_rosters() {
        let job = JobDiagnostic::new(6, 201);
        job.identify("std-thread", 903, 27);
        job.enable_progress_probe();
        job.phase(RUNNING, 0, 0);
        assert!(progress_carriers().active[27]);
        {
            let _held = RECORDS[6].lock();
            let snapshot = progress_carriers();
            assert!(snapshot.busy_rosters >= 1);
            assert!(!snapshot.active[27]);
        }
        let mut cursor = 0;
        let lines = sample_progress_lines(&mut cursor, 256);
        assert!(lines.iter().any(|line| line.contains("gen=201 id=903 carrier=27")));
        assert!(sample_progress_lines(&mut cursor, 0).is_empty());
    }

    #[test]
    fn progress_probe_prioritizes_running_jobs_and_never_waits_for_identity_locks() {
        let sleeper = JobDiagnostic::new(2, 18);
        sleeper.identify("codex-executor-task", 1, 0);
        sleeper.enable_progress_probe();
        sleeper.phase(WAITING, 0x123, 0);
        sleeper.progress(WAIT_PENDING);
        let runner = JobDiagnostic::new(2, 18);
        runner.identify("codex-executor-task", 2, 0);
        runner.enable_progress_probe();
        runner.phase(RUNNING, 0, 0);
        runner.progress(GUEST_ENTER);
        let selected = progress_lines(2, 1);
        assert_eq!(selected.len(), 1);
        assert!(selected[0].contains("id=2 carrier=0 stage=guest-enter"));
        assert!(selected[0].contains("seq=2"));
        runner.progress(CONTEXT_RESTORE);
        assert!(progress_lines(2, 1)[0].contains("stage=context-restore"));
        {
            let _held = runner.record.identity.lock();
            assert!(progress_lines(2, 1)[0].contains("identity=busy"));
        }
        {
            let _held = RECORDS[2].lock();
            assert!(progress_lines(2, 1)[0].contains("roster=busy"));
        }
        assert!(progress_lines(2, 0).is_empty());
        drop(runner);
        drop(sleeper);
        assert!(progress_lines(2, 4).is_empty());
    }

    #[test]
    fn passive_roster_observes_scope_before_it_returns_then_outside_abi() {
        let job = JobDiagnostic::new(4, 31);
        job.identify("codex-executor-task", 91, 19);
        job.enable_progress_probe();
        let guard = unsafe { job.abi_scope(AbiOperation::Mkdir) };
        job.phase(RUNNING, 0, 0);
        job.progress(GUEST_ENTER);
        let during = progress_lines(4, 1).remove(0);
        assert!(during.contains(" abi=mkdir "));
        assert!(during.contains("abi_depth=1 abi_entered=1 abi_exited=0"));
        drop(guard);
        let after = progress_lines(4, 1).remove(0);
        assert!(after.contains(" abi=none "));
        assert!(after.contains("abi_depth=0 abi_entered=1 abi_exited=1"));
        assert!(after.contains("abi_last=mkdir"));
        drop(job);
        assert!(progress_lines(4, 1).is_empty());
    }

    #[test]
    fn surviving_jobs_keep_identity_wait_and_generation_until_release() {
        let job = JobDiagnostic::new(3, 17);
        job.identify("std-thread", 81, 4);
        job.name("tokio-voxygen-0");
        job.phase(WAITING, 0x1234, 0);
        let report = lines(3).join("\n");
        assert!(report.contains("generation=17 kind=std-thread id=81 name=tokio-voxygen-0 carrier=4 phase=wait-queue"));
        assert!(report.contains("boundary=1 wait=0x1234 timeout_ms=0"));
        job.phase(RUNNING, 0, 0);
        assert!(lines(3)[0].contains("phase=running"));
        drop(job);
        assert!(lines(3).is_empty());
        let next = JobDiagnostic::new(3, 18);
        assert!(lines(3)[0].contains("generation=18"));
        drop(next);
    }
}
