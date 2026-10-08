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
