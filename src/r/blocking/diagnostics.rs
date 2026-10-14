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

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static RECORDS: [Mutex<Vec<Arc<Record>>>; crate::allcaps::hv::VM_ID_LIMIT] =
    [const { Mutex::new(Vec::new()) }; crate::allcaps::hv::VM_ID_LIMIT];

fn now_ms() -> u64 {
    embassy_time_driver::now().saturating_mul(1000) / embassy_time_driver::TICK_HZ.max(1)
}

struct Record {
    id: u64,
    generation: u64,
    identity: Mutex<(&'static str, usize, heapless::String<31>)>,
    carrier: AtomicUsize,
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
                id: NEXT_ID.fetch_add(1, Ordering::Relaxed), generation,
                identity: Mutex::new(("native", 0, heapless::String::new())),
                carrier: AtomicUsize::new(usize::MAX), phase: AtomicUsize::new(QUEUED),
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
            let Some(identity) = record.identity.try_lock() else {
                out.push(alloc::format!("codex-progress-job vm={} job={} identity=busy", vm_id, record.id));
                continue;
            };
            let sequence = record.progress_sequence.load(Ordering::Acquire);
            let stage = record.progress_stage.load(Ordering::Relaxed);
            let age = now.saturating_sub(record.progress_since_ms.load(Ordering::Relaxed));
            let enabled = record.progress_if.load(Ordering::Relaxed);
            core::sync::atomic::fence(Ordering::Acquire);
            if sequence & 1 != 0 || sequence != record.progress_sequence.load(Ordering::Acquire) {
                out.push(alloc::format!("codex-progress-job vm={} id={} snapshot=changing", vm_id, identity.1));
                continue;
            }
            out.push(alloc::format!(
                "codex-progress-job vm={} gen={} id={} carrier={} stage={} age_ms={} seq={} if={} phase={} wait=0x{:X} name={}",
                vm_id, record.generation, identity.1, record.carrier.load(Ordering::Relaxed),
                progress_stage_name(stage), age, sequence, enabled, phase,
                record.wait_object.load(Ordering::Relaxed),
                if identity.2.is_empty() { "-" } else { identity.2.as_str() },
            ));
        }
    }
    out
}

impl Drop for JobDiagnostic {
    fn drop(&mut self) {
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
