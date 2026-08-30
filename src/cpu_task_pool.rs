//! Bounded, policy-directed CPU work for AP executors.
//!
//! This is the kernel boundary for coarse CPU work such as VNNI row shards:
//! the caller owns immutable inputs plus output/join state (normally an
//! `Arc<Mutex<Option<T>>>` or a pre-partitioned output slice), while this module
//! owns admission, AP placement, and the lifetime of the exclusive compute
//! carrier claim. It intentionally has no unbounded queue: rejected work is
//! visible to the caller and can be retried at a point that preserves model
//! latency and output ownership.

extern crate alloc;

use alloc::boxed::Box;
use core::arch::x86_64::{__cpuid, __cpuid_count};
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use crate::r::kernel_task_domain::{self, KernelTaskDomain};
use crate::workers::{ComputeWorkerLease, ComputeWorkerPolicy, ComputeWorkerSnapshot};

pub type CpuTaskJob = Box<dyn FnOnce(CpuTaskContext) + Send + 'static>;

/// A static CPU work pool. Construct it as a `static` so a dispatched job can
/// retain its kernel admission permit until the closure has returned.
pub struct CpuTaskPool {
    policy: ComputeWorkerPolicy,
    configured_cap: u32,
    active: AtomicU32,
    submitted: AtomicU64,
    completed: AtomicU64,
    rejected_cap: AtomicU64,
    rejected_no_worker: AtomicU64,
    rejected_task_storage: AtomicU64,
}

impl CpuTaskPool {
    pub const fn new(policy: ComputeWorkerPolicy, concurrency_cap: usize) -> Self {
        let cap = if concurrency_cap == 0 {
            1
        } else if concurrency_cap > crate::allcaps::cpu_task_pool::TASK_STORAGE_SLOTS {
            crate::allcaps::cpu_task_pool::TASK_STORAGE_SLOTS
        } else {
            concurrency_cap
        } as u32;
        Self {
            policy,
            configured_cap: cap,
            active: AtomicU32::new(0),
            submitted: AtomicU64::new(0),
            completed: AtomicU64::new(0),
            rejected_cap: AtomicU64::new(0),
            rejected_no_worker: AtomicU64::new(0),
            rejected_task_storage: AtomicU64::new(0),
        }
    }

    pub const fn configured_cap(&self) -> usize {
        self.configured_cap as usize
    }

    /// Admit and place one finite synchronous compute closure on an AP.
    ///
    /// The closure owns all row-shard inputs and completion state. For a
    /// fan-out, capture one disjoint mutable output range (or an `Arc`-backed
    /// join cell) per closure, then wait for those owner-managed completions;
    /// no borrowed data crosses the executor boundary. `context.vnni_supported`
    /// verifies the actual worker before native VNNI work starts.
    pub fn try_dispatch(
        &'static self,
        label: &'static str,
        job: CpuTaskJob,
    ) -> Result<CpuTaskReceipt, CpuTaskDispatchError> {
        if !self.try_reserve() {
            self.rejected_cap.fetch_add(1, Ordering::Relaxed);
            return Err(CpuTaskDispatchError::PoolCap);
        }

        let Some(worker) = crate::workers::try_claim_compute_worker(self.policy) else {
            self.active.fetch_sub(1, Ordering::AcqRel);
            self.rejected_no_worker.fetch_add(1, Ordering::Relaxed);
            return Err(CpuTaskDispatchError::NoWorkerAvailable);
        };

        let receipt = CpuTaskReceipt {
            id: NEXT_JOB_ID.fetch_add(1, Ordering::Relaxed),
            cpu_slot: worker.cpu_slot(),
            core_kind: worker.core_kind(),
        };
        let context = CpuTaskContext {
            id: receipt.id,
            label,
            cpu_slot: receipt.cpu_slot,
            core_kind: receipt.core_kind,
        };
        let spawner = worker.spawner();
        let permit = CpuTaskPermit {
            pool: self,
            _worker: worker,
        };

        match cpu_task_pool_worker(permit, context, job) {
            Ok(task) => {
                spawner.spawn(task);
                self.submitted.fetch_add(1, Ordering::Relaxed);
                Ok(receipt)
            }
            Err(_) => {
                // The macro drops `permit` when allocation of its bounded task
                // storage fails, returning the AP claim and active admission.
                self.rejected_task_storage.fetch_add(1, Ordering::Relaxed);
                Err(CpuTaskDispatchError::TaskStorageBusy)
            }
        }
    }

    pub fn snapshot(&self) -> CpuTaskPoolSnapshot {
        let workers = crate::workers::compute_worker_snapshot(self.policy);
        let runtime_cap = self
            .configured_cap()
            .min(crate::allcaps::cpu_task_pool::TASK_STORAGE_SLOTS)
            .min(workers.eligible);
        let active = self.active.load(Ordering::Acquire) as usize;
        CpuTaskPoolSnapshot {
            policy: self.policy,
            configured_cap: self.configured_cap(),
            runtime_cap,
            active,
            worker: workers,
            submitted: self.submitted.load(Ordering::Acquire),
            completed: self.completed.load(Ordering::Acquire),
            rejected_cap: self.rejected_cap.load(Ordering::Acquire),
            rejected_no_worker: self.rejected_no_worker.load(Ordering::Acquire),
            rejected_task_storage: self.rejected_task_storage.load(Ordering::Acquire),
        }
    }

    fn try_reserve(&self) -> bool {
        let mut observed = self.active.load(Ordering::Acquire);
        loop {
            if observed >= self.configured_cap {
                return false;
            }
            match self.active.compare_exchange_weak(
                observed,
                observed + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return true,
                Err(current) => observed = current,
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CpuTaskDispatchError {
    /// The pool's explicit concurrency limit has been reached.
    PoolCap,
    /// No eligible AP is registered and idle under the selected policy.
    NoWorkerAvailable,
    /// The kernel's bounded task frame store is temporarily full.
    TaskStorageBusy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CpuTaskReceipt {
    pub id: u64,
    pub cpu_slot: u32,
    pub core_kind: u8,
}

/// Metadata observed inside the closure on the AP that executes it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CpuTaskContext {
    pub id: u64,
    pub label: &'static str,
    pub cpu_slot: u32,
    pub core_kind: u8,
}

impl CpuTaskContext {
    /// Confirm that the executor did not lose the AP affinity selected at
    /// admission. Current TRUEOS executors are per-CPU, but keeping this check
    /// at the public boundary makes a future scheduler change fail closed.
    pub fn is_running_on_assigned_worker(self) -> bool {
        crate::percpu::current_slot() as u32 == self.cpu_slot
    }

    /// Recheck AVX2/FMA/YMM state and AVX-VNNI on this actual AP.
    ///
    /// The LFM projector performs its own admission as well; this early query
    /// lets a row-shard owner avoid writing partial output on an unsupported
    /// lane if a heterogeneous future CPU exposes feature differences.
    pub fn vnni_status(self) -> CpuVnniStatus {
        CpuVnniStatus::detect_current()
    }

    pub fn vnni_supported(self) -> bool {
        self.is_running_on_assigned_worker() && self.vnni_status().supported
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CpuVnniStatus {
    pub ymm_state: bool,
    pub avx2_fma: bool,
    pub avx_vnni: bool,
    pub supported: bool,
}

impl CpuVnniStatus {
    fn detect_current() -> Self {
        let simd = crate::cpu::simd_status();
        let max_basic_leaf = __cpuid(0).eax;
        let avx_vnni = if max_basic_leaf < 7 {
            false
        } else {
            // CPUID.(EAX=7, ECX=0):EAX reports the largest supported subleaf.
            let max_subleaf = __cpuid_count(7, 0).eax;
            max_subleaf >= 1 && (__cpuid_count(7, 1).eax & (1 << 4)) != 0
        };
        let ymm_state = simd.avx_state_enabled;
        let avx2_fma = simd.avx2_fma_ready;
        Self {
            ymm_state,
            avx2_fma,
            avx_vnni,
            supported: ymm_state && avx2_fma && avx_vnni,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CpuTaskPoolSnapshot {
    pub policy: ComputeWorkerPolicy,
    pub configured_cap: usize,
    /// `min(configured_cap, task_storage_slots, registered eligible APs)`.
    pub runtime_cap: usize,
    pub active: usize,
    pub worker: ComputeWorkerSnapshot,
    pub submitted: u64,
    pub completed: u64,
    pub rejected_cap: u64,
    pub rejected_no_worker: u64,
    pub rejected_task_storage: u64,
}

struct CpuTaskPermit {
    pool: &'static CpuTaskPool,
    _worker: ComputeWorkerLease,
}

impl Drop for CpuTaskPermit {
    fn drop(&mut self) {
        self.pool.active.fetch_sub(1, Ordering::AcqRel);
        // `ComputeWorkerLease` then releases this AP's compute claim.
    }
}

static NEXT_JOB_ID: AtomicU64 = AtomicU64::new(1);

#[trueos_executor::task(pool_size = crate::allcaps::cpu_task_pool::TASK_STORAGE_SLOTS)]
async fn cpu_task_pool_worker(permit: CpuTaskPermit, context: CpuTaskContext, job: CpuTaskJob) {
    kernel_task_domain::with(KernelTaskDomain::ComputeWorker, None, || job(context));
    permit.pool.completed.fetch_add(1, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_cap_is_clamped_to_bounded_task_storage() {
        let zero = CpuTaskPool::new(ComputeWorkerPolicy::PerformanceFirst, 0);
        assert_eq!(zero.configured_cap(), 1);
        let large = CpuTaskPool::new(
            ComputeWorkerPolicy::PerformanceFirst,
            crate::allcaps::cpu_task_pool::TASK_STORAGE_SLOTS + 1,
        );
        assert_eq!(large.configured_cap(), crate::allcaps::cpu_task_pool::TASK_STORAGE_SLOTS);
    }
}
