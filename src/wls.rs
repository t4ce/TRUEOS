//! Stable worker-local storage identities for host service work and Blueprint execution.
//!
//! The returned slot indexes the 4096-entry backing arrays in the TRUEOS std and Tokio ports.
//! Physical carrier ownership remains in `hv::lane`; this module only assigns logical identities.

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

pub const WORKER_SLOT_COUNT: usize = crate::allcaps::wls::WORKER_SLOT_COUNT;
const CPU_TRACK_COUNT: usize = crate::allcaps::wls::CPU_TRACK_COUNT;

const NO_CPU_SLOT: u32 = u32::MAX;
const WLS_HOST_WORKER_BASE: usize = 0;
const WLS_BLUEPRINT_RUNTIME_BASE: usize = WLS_HOST_WORKER_BASE + WORKER_SLOT_COUNT;
const WLS_BLUEPRINT_WORKER_BASE: usize =
    WLS_BLUEPRINT_RUNTIME_BASE + crate::allcaps::hv::VM_ID_LIMIT;
const WLS_BLUEPRINT_THREAD_SLOTS_PER_VM: usize = 64;
const WLS_BLUEPRINT_THREAD_SLOTS_PER_REALM: usize = WLS_BLUEPRINT_THREAD_SLOTS_PER_VM / 2;
const WLS_BLUEPRINT_THREAD_BASE: usize =
    WLS_BLUEPRINT_WORKER_BASE + crate::allcaps::hv::VM_ID_LIMIT * WORKER_SLOT_COUNT;
const WLS_HOST_FALLBACK_BASE: usize =
    WLS_BLUEPRINT_THREAD_BASE + crate::allcaps::hv::VM_ID_LIMIT * WLS_BLUEPRINT_THREAD_SLOTS_PER_VM;
const NO_BLUEPRINT_THREAD_ID: u32 = 0;
pub(crate) const BLUEPRINT_THREAD_CARRIER_TAG: u32 = 1 << 31;
const _: () = assert!(WORKER_SLOT_COUNT > 0);
const _: () = assert!(WLS_BLUEPRINT_THREAD_SLOTS_PER_VM % 2 == 0);
const _: () = assert!(
    WLS_HOST_FALLBACK_BASE + crate::allcaps::hv::VM_CPU_SLOT_LIMIT
        <= crate::allcaps::wls::TLS_SLOT_COUNT
);

static WORKER_LEASED: [AtomicBool; WORKER_SLOT_COUNT] =
    [const { AtomicBool::new(false) }; WORKER_SLOT_COUNT];
static WORKER_GENERATION: [AtomicU32; WORKER_SLOT_COUNT] =
    [const { AtomicU32::new(1) }; WORKER_SLOT_COUNT];
static CURRENT_WORKER_TOKEN_BY_CPU: [AtomicU64; CPU_TRACK_COUNT] =
    [const { AtomicU64::new(0) }; CPU_TRACK_COUNT];
static LOGGED_WORKER_POOL_BUSY: AtomicBool = AtomicBool::new(false);
static LOGGED_BLUEPRINT_RUNTIME_WLS_SLOT: [AtomicBool; 32] = [const { AtomicBool::new(false) }; 32];
static LOGGED_BLUEPRINT_WORKER_WLS_SLOT: [AtomicBool; 32] = [const { AtomicBool::new(false) }; 32];
static LOGGED_BLUEPRINT_THREAD_WLS_SLOT: [AtomicBool; 32] = [const { AtomicBool::new(false) }; 32];
static LOGGED_HOST_WLS_FALLBACK: AtomicBool = AtomicBool::new(false);
static CURRENT_BLUEPRINT_THREAD_ID_BY_CPU: [AtomicU32; crate::allcaps::hv::VM_CPU_SLOT_LIMIT] =
    [const { AtomicU32::new(NO_BLUEPRINT_THREAD_ID) }; crate::allcaps::hv::VM_CPU_SLOT_LIMIT];

#[derive(Debug)]
pub(crate) struct WorkerIdentityLease {
    worker_id: u32,
    cpu_slot: u32,
    generation: u32,
    armed: bool,
}

pub(crate) struct WorkerIdentityGuard {
    cpu_slot: Option<usize>,
    installed_token: u64,
    previous_token: u64,
}

#[inline]
fn cpu_slot_now() -> u32 {
    let slot = crate::percpu::current_slot();
    if slot > u32::MAX as usize {
        NO_CPU_SLOT
    } else {
        slot as u32
    }
}

#[inline]
fn worker_token(worker_id: u32, generation: u32) -> u64 {
    ((generation as u64) << 32) | worker_id.saturating_add(1) as u64
}

#[inline]
fn worker_id_from_token(token: u64) -> Option<usize> {
    let worker_id = (token as u32).checked_sub(1)? as usize;
    if worker_id < WORKER_SLOT_COUNT {
        Some(worker_id)
    } else {
        None
    }
}

pub(crate) fn try_lease_worker_identity(cpu_slot: u32) -> Option<WorkerIdentityLease> {
    for worker_id in 0..WORKER_SLOT_COUNT {
        if WORKER_LEASED[worker_id]
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            continue;
        }

        let generation = WORKER_GENERATION[worker_id]
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1);
        return Some(WorkerIdentityLease {
            worker_id: worker_id as u32,
            cpu_slot,
            generation,
            armed: true,
        });
    }

    if !LOGGED_WORKER_POOL_BUSY.swap(true, Ordering::AcqRel) {
        crate::log_warn!(
            target: "service";
            "wls: all {} worker identities busy\n",
            WORKER_SLOT_COUNT
        );
    }
    None
}

impl WorkerIdentityLease {
    pub(crate) fn enter(&self) -> WorkerIdentityGuard {
        let cpu_slot = self.cpu_slot as usize;
        if !self.armed
            || cpu_slot >= CURRENT_WORKER_TOKEN_BY_CPU.len()
            || self.worker_id as usize >= WORKER_SLOT_COUNT
        {
            return WorkerIdentityGuard {
                cpu_slot: None,
                installed_token: 0,
                previous_token: 0,
            };
        }

        let token = worker_token(self.worker_id, self.generation);
        let previous_token = CURRENT_WORKER_TOKEN_BY_CPU[cpu_slot].swap(token, Ordering::AcqRel);
        WorkerIdentityGuard {
            cpu_slot: Some(cpu_slot),
            installed_token: token,
            previous_token,
        }
    }
}

impl Drop for WorkerIdentityLease {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }

        let worker_id = self.worker_id as usize;
        if worker_id >= WORKER_SLOT_COUNT
            || WORKER_GENERATION[worker_id].load(Ordering::Acquire) != self.generation
        {
            self.armed = false;
            return;
        }

        let cpu_slot = self.cpu_slot as usize;
        if cpu_slot < CURRENT_WORKER_TOKEN_BY_CPU.len() {
            let _ = CURRENT_WORKER_TOKEN_BY_CPU[cpu_slot].compare_exchange(
                worker_token(self.worker_id, self.generation),
                0,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
        WORKER_LEASED[worker_id].store(false, Ordering::Release);
        LOGGED_WORKER_POOL_BUSY.store(false, Ordering::Release);
        self.armed = false;
    }
}

impl Drop for WorkerIdentityGuard {
    fn drop(&mut self) {
        let Some(cpu_slot) = self.cpu_slot else {
            return;
        };
        let _ = CURRENT_WORKER_TOKEN_BY_CPU[cpu_slot].compare_exchange(
            self.installed_token,
            self.previous_token,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
}

pub fn current_worker_id() -> Option<usize> {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return None;
    }

    let cpu_slot = cpu_slot_now() as usize;
    let token = CURRENT_WORKER_TOKEN_BY_CPU
        .get(cpu_slot)?
        .load(Ordering::Acquire);
    let worker_id = worker_id_from_token(token)?;
    let generation = (token >> 32) as u32;
    if WORKER_LEASED[worker_id].load(Ordering::Acquire)
        && WORKER_GENERATION[worker_id].load(Ordering::Acquire) == generation
    {
        Some(worker_id)
    } else {
        None
    }
}

#[inline]
fn wls_host_worker_slot(worker_id: usize) -> u32 {
    WLS_HOST_WORKER_BASE.saturating_add(worker_id.min(WORKER_SLOT_COUNT - 1)) as u32
}

#[inline]
fn wls_blueprint_runtime_slot(vm_id: u8) -> u32 {
    WLS_BLUEPRINT_RUNTIME_BASE.saturating_add(vm_id as usize) as u32
}

#[inline]
fn wls_blueprint_worker_slot(vm_id: u8, worker_id: usize) -> u32 {
    WLS_BLUEPRINT_WORKER_BASE
        .saturating_add((vm_id as usize).saturating_mul(WORKER_SLOT_COUNT))
        .saturating_add(worker_id.min(WORKER_SLOT_COUNT - 1)) as u32
}

#[inline]
fn wls_blueprint_thread_slot(vm_id: u8, thread_id: u32) -> u32 {
    // Hull-created and carrier-created pthread handles use independent counters,
    // so keep them in separate halves of each VM's thread range.
    let carrier = (thread_id & BLUEPRINT_THREAD_CARRIER_TAG) != 0;
    let sequence = thread_id & !BLUEPRINT_THREAD_CARRIER_TAG;
    let realm_base = if carrier {
        WLS_BLUEPRINT_THREAD_SLOTS_PER_REALM
    } else {
        0
    };
    let thread_index = realm_base
        + sequence
            .saturating_sub(1)
            .min((WLS_BLUEPRINT_THREAD_SLOTS_PER_REALM - 1) as u32) as usize;
    WLS_BLUEPRINT_THREAD_BASE
        .saturating_add((vm_id as usize).saturating_mul(WLS_BLUEPRINT_THREAD_SLOTS_PER_VM))
        .saturating_add(thread_index) as u32
}

#[inline]
fn wls_host_fallback_slot(cpu_slot: u32) -> u32 {
    if cpu_slot == NO_CPU_SLOT {
        WLS_HOST_FALLBACK_BASE.saturating_add(CPU_TRACK_COUNT) as u32
    } else {
        WLS_HOST_FALLBACK_BASE.saturating_add(cpu_slot as usize) as u32
    }
}

#[inline]
fn current_blueprint_worker_id() -> Option<usize> {
    current_worker_id().or_else(|| {
        let cpu_slot = cpu_slot_now();
        if cpu_slot == NO_CPU_SLOT {
            None
        } else {
            Some((cpu_slot as usize) % WORKER_SLOT_COUNT)
        }
    })
}

pub fn current_blueprint_thread_id() -> Option<u32> {
    let cpu_slot = cpu_slot_now();
    if cpu_slot == NO_CPU_SLOT {
        return None;
    }
    let thread_id = CURRENT_BLUEPRINT_THREAD_ID_BY_CPU
        .get(cpu_slot as usize)?
        .load(Ordering::Acquire);
    if thread_id == NO_BLUEPRINT_THREAD_ID {
        None
    } else {
        Some(thread_id)
    }
}

pub fn with_current_blueprint_thread_id<R>(thread_id: usize, f: impl FnOnce() -> R) -> R {
    let cpu_slot = cpu_slot_now();
    if cpu_slot == NO_CPU_SLOT {
        return f();
    }
    let Some(slot) = CURRENT_BLUEPRINT_THREAD_ID_BY_CPU.get(cpu_slot as usize) else {
        return f();
    };
    let thread_id = thread_id.min(u32::MAX as usize) as u32;
    let previous = slot.swap(thread_id.max(1), Ordering::AcqRel);
    let result = f();
    slot.store(previous, Ordering::Release);
    result
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_wls_current_slot() -> u32 {
    if let Some(vm_id) = crate::hv::current_hull_guest_context_vm_id() {
        let slot = wls_blueprint_runtime_slot(vm_id);
        let vm_index = vm_id as usize;
        if vm_index < LOGGED_BLUEPRINT_RUNTIME_WLS_SLOT.len()
            && !LOGGED_BLUEPRINT_RUNTIME_WLS_SLOT[vm_index].swap(true, Ordering::AcqRel)
        {
            crate::log!("wls: blueprint runtime vm={} worker=main slot={}\n", vm_id, slot);
        }
        return slot;
    }

    if let Some(vm_id) = crate::hv::current_guest_execution_context_vm_id() {
        if let Some(thread_id) = current_blueprint_thread_id() {
            let slot = wls_blueprint_thread_slot(vm_id, thread_id);
            let vm_index = vm_id as usize;
            if vm_index < LOGGED_BLUEPRINT_THREAD_WLS_SLOT.len()
                && !LOGGED_BLUEPRINT_THREAD_WLS_SLOT[vm_index].swap(true, Ordering::AcqRel)
            {
                crate::log!(
                    "wls: blueprint thread vm={} thread={} slot={}\n",
                    vm_id,
                    thread_id,
                    slot
                );
            }
            return slot;
        }
        let worker_id = current_blueprint_worker_id().unwrap_or(0);
        let slot = wls_blueprint_worker_slot(vm_id, worker_id);
        let vm_index = vm_id as usize;
        if vm_index < LOGGED_BLUEPRINT_WORKER_WLS_SLOT.len()
            && !LOGGED_BLUEPRINT_WORKER_WLS_SLOT[vm_index].swap(true, Ordering::AcqRel)
        {
            crate::log!("wls: blueprint worker vm={} worker={} slot={}\n", vm_id, worker_id, slot);
        }
        return slot;
    }

    if let Some(worker_id) = current_worker_id() {
        return wls_host_worker_slot(worker_id);
    }

    let cpu_slot = cpu_slot_now();
    let slot = wls_host_fallback_slot(cpu_slot);
    if !LOGGED_HOST_WLS_FALLBACK.swap(true, Ordering::AcqRel) {
        crate::log!("wls: host fallback source=cpu cpu_slot={} slot={}\n", cpu_slot, slot);
    }
    slot
}
