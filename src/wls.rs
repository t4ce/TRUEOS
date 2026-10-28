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
// Threads share the same reserved slot budget. A static per-VM/per-realm
// partition imposed a 20-thread limit even when almost every slot was idle.
// VM and complete opaque ID identify each lease; neither is a slot index.
const WLS_BLUEPRINT_THREAD_SLOT_COUNT: usize = crate::allcaps::hv::VM_ID_LIMIT * 40;
const WLS_BLUEPRINT_THREAD_BASE: usize =
    WLS_BLUEPRINT_WORKER_BASE + crate::allcaps::hv::VM_ID_LIMIT * WORKER_SLOT_COUNT;
const WLS_HOST_FALLBACK_BASE: usize =
    WLS_BLUEPRINT_THREAD_BASE + WLS_BLUEPRINT_THREAD_SLOT_COUNT;
const NO_BLUEPRINT_THREAD_ID: u32 = 0;
pub(crate) const BLUEPRINT_THREAD_CARRIER_TAG: u32 = 1 << 31;
const _: () = assert!(WORKER_SLOT_COUNT > 0);
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
const NO_BLUEPRINT_THREAD_SLOT: u32 = u32::MAX;
static CURRENT_BLUEPRINT_THREAD_SLOT_BY_CPU: [AtomicU32; crate::allcaps::hv::VM_CPU_SLOT_LIMIT] =
    [const { AtomicU32::new(NO_BLUEPRINT_THREAD_SLOT) }; crate::allcaps::hv::VM_CPU_SLOT_LIMIT];
pub(crate) type ThreadContext = (u64, u32, Option<u32>);

// Admission is infrequent and scans the bounded pool once. The scheduler
// installs the owned slot in its context so running TLS lookup is O(1).
static BLUEPRINT_THREAD_IDENTITIES: [AtomicU64; WLS_BLUEPRINT_THREAD_SLOT_COUNT] =
    [const { AtomicU64::new(0) }; WLS_BLUEPRINT_THREAD_SLOT_COUNT];
static BLUEPRINT_THREAD_ADMISSION: spin::Mutex<()> = spin::Mutex::new(());

pub(crate) struct BlueprintThreadIdentityLease {
    identity: u64,
    index: usize,
}

fn blueprint_thread_identity(vm_id: u8, thread_id: u32) -> Option<u64> {
    if vm_id as usize >= crate::allcaps::hv::VM_ID_LIMIT
        || thread_id & !BLUEPRINT_THREAD_CARRIER_TAG == 0 { return None; }
    Some(((u64::from(vm_id) + 1) << 32) | u64::from(thread_id))
}

pub(crate) fn try_lease_blueprint_thread_identity(vm_id: u8, thread_id: usize) -> Option<BlueprintThreadIdentityLease> {
    let thread_id = u32::try_from(thread_id).ok()?;
    let identity = blueprint_thread_identity(vm_id, thread_id)?;
    let _admission = BLUEPRINT_THREAD_ADMISSION.lock();
    let mut free = None;
    for (index, slot) in BLUEPRINT_THREAD_IDENTITIES.iter().enumerate() {
        match slot.load(Ordering::Acquire) {
            occupied if occupied == identity => return None,
            0 if free.is_none() => free = Some(index),
            _ => {},
        }
    }
    let index = free?;
    BLUEPRINT_THREAD_IDENTITIES[index].store(identity, Ordering::Release);
    Some(BlueprintThreadIdentityLease { identity, index })
}

impl BlueprintThreadIdentityLease {
    pub(crate) fn slot(&self) -> u32 {
        (WLS_BLUEPRINT_THREAD_BASE + self.index) as u32
    }
}

impl Drop for BlueprintThreadIdentityLease {
    fn drop(&mut self) {
        // The Thread owns this lease until its stack/address space and guest
        // frames have retired. pthread detach/join table removal is unrelated.
        // A lease is unique and cannot be cloned. Its slot cannot be reused
        // before this retirement CAS; clearing also requires its exact owner.
        let cleared = BLUEPRINT_THREAD_IDENTITIES[self.index]
            .compare_exchange(self.identity, 0, Ordering::AcqRel, Ordering::Acquire);
        debug_assert!(cleared.is_ok(), "WLS identity retired without its lease");
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


fn current_blueprint_thread_slot(vm_id: u8, thread_id: u32) -> Option<u32> {
    let slot = CURRENT_BLUEPRINT_THREAD_SLOT_BY_CPU.get(cpu_slot_now() as usize)?.load(Ordering::Acquire);
    let index = (slot as usize).checked_sub(WLS_BLUEPRINT_THREAD_BASE)?;
    let identity = blueprint_thread_identity(vm_id, thread_id)?;
    (BLUEPRINT_THREAD_IDENTITIES.get(index)?.load(Ordering::Acquire) == identity).then_some(slot)
}

// Tests inspect all admitted identities without installing each one as the
// current continuation. Runtime TLS lookup uses the installed slot above.
#[cfg(test)]
fn wls_blueprint_thread_slot(vm_id: u8, thread_id: u32) -> Option<u32> {
    let identity = blueprint_thread_identity(vm_id, thread_id)?;
    BLUEPRINT_THREAD_IDENTITIES.iter()
        .position(|slot| slot.load(Ordering::Acquire) == identity)
        .map(|index| (WLS_BLUEPRINT_THREAD_BASE + index) as u32)
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

pub(crate) fn replace_thread_context((worker, thread, identity_slot): ThreadContext) -> ThreadContext {
    let slot = cpu_slot_now() as usize;
    (
        CURRENT_WORKER_TOKEN_BY_CPU[slot].swap(worker, Ordering::AcqRel),
        CURRENT_BLUEPRINT_THREAD_ID_BY_CPU[slot].swap(thread, Ordering::AcqRel),
        match CURRENT_BLUEPRINT_THREAD_SLOT_BY_CPU[slot].swap(identity_slot.unwrap_or(NO_BLUEPRINT_THREAD_SLOT), Ordering::AcqRel) {
            NO_BLUEPRINT_THREAD_SLOT => None,
            previous => Some(previous),
        },
    )
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
            let slot = current_blueprint_thread_slot(vm_id, thread_id)
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
    fn shared_budget_admits_scoped_fanout_without_borrowing_live_identities() {
        let _serial = SERIAL.lock().unwrap();
        // The scheduler permits 256 live threads. These 64+64 identities
        // used to fail on the 21st same-realm admission with 2540 idle slots.
        let mut hull: Vec<_> = (1..=64).map(|id| try_lease_blueprint_thread_identity(61, id).unwrap()).collect();
        let carrier: Vec<_> = (1..=64).map(|id| try_lease_blueprint_thread_identity(61, BLUEPRINT_THREAD_CARRIER_TAG as usize | id).unwrap()).collect();
        let before: Vec<_> = (1..=64).map(|id| wls_blueprint_thread_slot(61, id).unwrap()).collect();
        assert!(try_lease_blueprint_thread_identity(61, 1).is_none(), "duplicate opaque ID must not gain another lease");
        let other_vm = try_lease_blueprint_thread_identity(62, 1).unwrap();
        assert_ne!(wls_blueprint_thread_slot(62, 1), wls_blueprint_thread_slot(61, 1));
        let mut live_slots: Vec<_> = hull.iter().chain(carrier.iter()).chain(core::iter::once(&other_vm))
            .map(BlueprintThreadIdentityLease::slot).collect();
        live_slots.sort_unstable(); live_slots.dedup();
        assert_eq!(live_slots.len(), 129, "a live identity must never be clamped onto another");
        drop(hull.remove(5));
        let replacement = try_lease_blueprint_thread_identity(61, 1000).unwrap();
        assert_eq!(wls_blueprint_thread_slot(61, 1000), Some(before[5]));
        for id in (1..=64).filter(|&id| id != 6) { assert_eq!(wls_blueprint_thread_slot(61, id), Some(before[id as usize - 1])); }
        drop((hull, carrier, other_vm, replacement));
    }

    #[test]
    fn exhaustion_is_bounded_by_the_unchanged_shared_slot_budget() {
        let _serial = SERIAL.lock().unwrap();
        let mut leases: Vec<_> = (1..=WLS_BLUEPRINT_THREAD_SLOT_COUNT)
            .map(|id| try_lease_blueprint_thread_identity(58, id).unwrap()).collect();
        let slots: Vec<_> = leases.iter().map(BlueprintThreadIdentityLease::slot).collect();
        assert_eq!(slots[0] as usize, WLS_BLUEPRINT_THREAD_BASE);
        assert_eq!(slots.last().copied().unwrap() as usize + 1, WLS_HOST_FALLBACK_BASE);
        assert!(try_lease_blueprint_thread_identity(58, WLS_BLUEPRINT_THREAD_SLOT_COUNT + 1).is_none());
        assert!(try_lease_blueprint_thread_identity(57, 1).is_none(), "other VM cannot displace a live owner");
        let released = leases.remove(50);
        let released_slot = released.slot();
        drop(released);
        let replacement = try_lease_blueprint_thread_identity(57, 1).unwrap();
        assert_eq!(replacement.slot(), released_slot);
        drop((leases, replacement));
    }

    #[test]
    fn installed_slot_validates_vm_and_full_opaque_identity_without_scanning() {
        let _serial = SERIAL.lock().unwrap();
        let lease = try_lease_blueprint_thread_identity(56, BLUEPRINT_THREAD_CARRIER_TAG as usize | 777).unwrap();
        let other = try_lease_blueprint_thread_identity(55, BLUEPRINT_THREAD_CARRIER_TAG as usize | 777).unwrap();
        let thread = BLUEPRINT_THREAD_CARRIER_TAG | 777;
        let previous = replace_thread_context((0, thread, Some(lease.slot())));
        assert_eq!(current_blueprint_thread_slot(56, thread), Some(lease.slot()));
        assert_eq!(current_blueprint_thread_slot(55, thread), None);
        assert_eq!(current_blueprint_thread_slot(56, 777), None);
        assert_eq!(current_blueprint_thread_slot(56, thread + 1), None);
        replace_thread_context((0, thread, Some(other.slot())));
        assert_eq!(current_blueprint_thread_slot(55, thread), Some(other.slot()));
        assert_eq!(current_blueprint_thread_slot(56, thread), None);
        replace_thread_context(previous);
        drop((lease, other));
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
