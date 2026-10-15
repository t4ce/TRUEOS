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
// Keep the complete 64-VM WLS layout within the 4096 slots baked into the
// TRUEOS std/Tokio ports. Runtime and worker identities remain unique per VM;
// Blueprint-created and carrier-created threads each lease 20 live slots.
const WLS_BLUEPRINT_THREAD_SLOTS_PER_VM: usize = 40;
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
static LOGGED_BLUEPRINT_RUNTIME_WLS_SLOT: [AtomicBool; crate::allcaps::hv::VM_ID_LIMIT] =
    [const { AtomicBool::new(false) }; crate::allcaps::hv::VM_ID_LIMIT];
static LOGGED_BLUEPRINT_WORKER_WLS_SLOT: [AtomicBool; crate::allcaps::hv::VM_ID_LIMIT] =
    [const { AtomicBool::new(false) }; crate::allcaps::hv::VM_ID_LIMIT];
static LOGGED_BLUEPRINT_THREAD_WLS_SLOT: [AtomicBool; crate::allcaps::hv::VM_ID_LIMIT] =
    [const { AtomicBool::new(false) }; crate::allcaps::hv::VM_ID_LIMIT];
static LOGGED_HOST_WLS_FALLBACK: AtomicBool = AtomicBool::new(false);
static CURRENT_BLUEPRINT_THREAD_ID_BY_CPU: [AtomicU32; crate::allcaps::hv::VM_CPU_SLOT_LIMIT] =
    [const { AtomicU32::new(NO_BLUEPRINT_THREAD_ID) }; crate::allcaps::hv::VM_CPU_SLOT_LIMIT];

// Opaque pthread sequence numbers are never slot indexes. Admission serializes
// each small realm table; lookup and release use only atomic scalar accesses.
static BLUEPRINT_THREAD_IDS: [[AtomicU32; WLS_BLUEPRINT_THREAD_SLOTS_PER_VM]; crate::allcaps::hv::VM_ID_LIMIT] =
    [const { [const { AtomicU32::new(0) }; WLS_BLUEPRINT_THREAD_SLOTS_PER_VM] }; crate::allcaps::hv::VM_ID_LIMIT];
static BLUEPRINT_THREAD_ADMISSION: [[spin::Mutex<()>; 2]; crate::allcaps::hv::VM_ID_LIMIT] =
    [const { [const { spin::Mutex::new(()) }; 2] }; crate::allcaps::hv::VM_ID_LIMIT];

pub(crate) struct BlueprintThreadIdentityLease {
    vm_id: u8,
    thread_id: u32,
    index: usize,
}

pub(crate) fn try_lease_blueprint_thread_identity(vm_id: u8, thread_id: usize) -> Option<BlueprintThreadIdentityLease> {
    let thread_id = u32::try_from(thread_id).ok()?;
    if thread_id & !BLUEPRINT_THREAD_CARRIER_TAG == 0 { return None; }
    let ids = BLUEPRINT_THREAD_IDS.get(vm_id as usize)?;
    let realm = usize::from(thread_id & BLUEPRINT_THREAD_CARRIER_TAG != 0);
    let _admission = BLUEPRINT_THREAD_ADMISSION[vm_id as usize][realm].lock();
    let base = realm * WLS_BLUEPRINT_THREAD_SLOTS_PER_REALM;
    let realm_ids = &ids[base..base + WLS_BLUEPRINT_THREAD_SLOTS_PER_REALM];
    if realm_ids.iter().any(|id| id.load(Ordering::Acquire) == thread_id) { return None; }
    let index = base + realm_ids.iter().position(|id| id.load(Ordering::Acquire) == 0)?;
    ids[index].store(thread_id, Ordering::Release);
    Some(BlueprintThreadIdentityLease { vm_id, thread_id, index })
}

impl Drop for BlueprintThreadIdentityLease {
    fn drop(&mut self) {
        // The Thread owns this lease until its stack/address space and guest
        // frames have retired. pthread detach/join table removal is unrelated.
        let _ = BLUEPRINT_THREAD_IDS[self.vm_id as usize][self.index]
            .compare_exchange(self.thread_id, 0, Ordering::AcqRel, Ordering::Acquire);
    }
}

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

/// Advisory only: the lease CAS remains authoritative under concurrent admission.
pub(crate) fn available_worker_identities() -> usize {
    WORKER_LEASED
        .iter()
        .filter(|leased| !leased.load(Ordering::Acquire))
        .count()
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
fn wls_blueprint_thread_slot(vm_id: u8, thread_id: u32) -> Option<u32> {
    if thread_id & !BLUEPRINT_THREAD_CARRIER_TAG == 0 { return None; }
    let ids = BLUEPRINT_THREAD_IDS.get(vm_id as usize)?;
    let base = usize::from(thread_id & BLUEPRINT_THREAD_CARRIER_TAG != 0) * WLS_BLUEPRINT_THREAD_SLOTS_PER_REALM;
    let index = base + ids[base..base + WLS_BLUEPRINT_THREAD_SLOTS_PER_REALM].iter()
        .position(|id| id.load(Ordering::Acquire) == thread_id)?;
    Some((WLS_BLUEPRINT_THREAD_BASE + vm_id as usize * WLS_BLUEPRINT_THREAD_SLOTS_PER_VM + index) as u32)
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

pub(crate) fn replace_thread_context((worker, thread): (u64, u32)) -> (u64, u32) {
    let slot = cpu_slot_now() as usize;
    (
        CURRENT_WORKER_TOKEN_BY_CPU[slot].swap(worker, Ordering::AcqRel),
        CURRENT_BLUEPRINT_THREAD_ID_BY_CPU[slot].swap(thread, Ordering::AcqRel),
    )
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
            let slot = wls_blueprint_thread_slot(vm_id, thread_id)
                .expect("running Blueprint thread has no live WLS identity lease");
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

#[cfg(test)]
mod thread_identity_tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn opaque_sequences_above_twenty_keep_distinct_live_slots() {
        let _serial = SERIAL.lock().unwrap();
        let ids = [20, 21, 1000, BLUEPRINT_THREAD_CARRIER_TAG | 20, BLUEPRINT_THREAD_CARRIER_TAG | 21];
        let leases: Vec<_> = ids.iter().map(|&id| try_lease_blueprint_thread_identity(60, id as usize).unwrap()).collect();
        let mut slots: Vec<_> = ids.iter().map(|&id| wls_blueprint_thread_slot(60, id).unwrap()).collect();
        slots.sort_unstable(); slots.dedup();
        assert_eq!(slots.len(), ids.len());
        drop(leases);
        for id in ids { assert!(wls_blueprint_thread_slot(60, id).is_none()); }
    }

    #[test]
    fn exhaustion_is_per_vm_realm_and_recycles_only_released_slots() {
        let _serial = SERIAL.lock().unwrap();
        let mut hull: Vec<_> = (1..=20).map(|id| try_lease_blueprint_thread_identity(61, id).unwrap()).collect();
        let carrier: Vec<_> = (1..=20).map(|id| try_lease_blueprint_thread_identity(61, BLUEPRINT_THREAD_CARRIER_TAG as usize | id).unwrap()).collect();
        let before: Vec<_> = (1..=20).map(|id| wls_blueprint_thread_slot(61, id).unwrap()).collect();
        assert!(try_lease_blueprint_thread_identity(61, 21).is_none());
        assert!(try_lease_blueprint_thread_identity(61, BLUEPRINT_THREAD_CARRIER_TAG as usize | 21).is_none());
        assert!(try_lease_blueprint_thread_identity(61, 1).is_none(), "duplicate opaque ID must not gain another lease");
        let other_vm = try_lease_blueprint_thread_identity(62, 1).unwrap();
        assert_ne!(wls_blueprint_thread_slot(62, 1), wls_blueprint_thread_slot(61, 1));
        drop(hull.remove(5));
        let replacement = try_lease_blueprint_thread_identity(61, 1000).unwrap();
        assert_eq!(wls_blueprint_thread_slot(61, 1000), Some(before[5]));
        for id in (1..=20).filter(|&id| id != 6) { assert_eq!(wls_blueprint_thread_slot(61, id), Some(before[id as usize - 1])); }
        drop((hull, carrier, other_vm, replacement));
    }

    #[test]
    fn thousands_of_sequential_creations_do_not_exhaust_or_alias_live_peer() {
        let _serial = SERIAL.lock().unwrap();
        let peer = try_lease_blueprint_thread_identity(63, BLUEPRINT_THREAD_CARRIER_TAG as usize | 1).unwrap();
        let peer_slot = wls_blueprint_thread_slot(63, BLUEPRINT_THREAD_CARRIER_TAG | 1).unwrap();
        for sequence in 2..2002 {
            let id = BLUEPRINT_THREAD_CARRIER_TAG | sequence;
            let lease = try_lease_blueprint_thread_identity(63, id as usize).unwrap();
            assert_ne!(wls_blueprint_thread_slot(63, id), Some(peer_slot));
            drop(lease);
            assert!(wls_blueprint_thread_slot(63, id).is_none());
            assert_eq!(wls_blueprint_thread_slot(63, BLUEPRINT_THREAD_CARRIER_TAG | 1), Some(peer_slot));
        }
        drop(peer);
        assert!(try_lease_blueprint_thread_identity(63, 0).is_none());
        assert!(try_lease_blueprint_thread_identity(63, BLUEPRINT_THREAD_CARRIER_TAG as usize).is_none());
        assert!(try_lease_blueprint_thread_identity(64, 1).is_none());
        assert!(try_lease_blueprint_thread_identity(63, u32::MAX as usize + 1).is_none());
    }

    #[test]
    fn simultaneous_duplicate_admissions_never_publish_two_live_mappings() {
        let _serial = SERIAL.lock().unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let jobs: Vec<_> = (0..8).map(|_| {
            let barrier = barrier.clone();
            std::thread::spawn(move || { barrier.wait(); try_lease_blueprint_thread_identity(59, 1234) })
        }).collect();
        let leases: Vec<_> = jobs.into_iter().filter_map(|job| job.join().unwrap()).collect();
        assert_eq!(leases.len(), 1);
        assert!(wls_blueprint_thread_slot(59, 1234).is_some());
        drop(leases);
        assert!(wls_blueprint_thread_slot(59, 1234).is_none());
    }
}
