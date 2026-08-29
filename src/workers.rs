extern crate alloc;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};

use spin::Mutex;
use trueos_executor::{SendSpawner, SpawnToken, Spawner};

pub const CORE_KIND_UNKNOWN: u8 = 0;
pub const CORE_KIND_PERF: u8 = 1;
pub const CORE_KIND_EFF: u8 = 2;
pub const AP1_UI_SERVICE_SLOT: u32 = 1;

// Slot 0 is BSP and slot 1 is the UI/service AP; background carriers start at AP2.
const FIRST_BACKGROUND_SLOT: u32 = 2;
const APP_PARALLELISM_NO_UI: bool = false;
const WORKER_SLOT_LIMIT: usize = crate::allcaps::hv::VM_CPU_SLOT_LIMIT;
const REGISTERED_SLOT_WORD_BITS: usize = u64::BITS as usize;
const REGISTERED_SLOT_WORDS: usize =
    (WORKER_SLOT_LIMIT + REGISTERED_SLOT_WORD_BITS - 1) / REGISTERED_SLOT_WORD_BITS;
// Live scanout capture and H.264 encode still share a synchronous, single-frame
// ownership boundary which makes sharing a cooperative executor unsafe. Keep
// that media-side exception on the topology's final AP. Complete encoded access
// units cross a bounded, one-way handoff into ordinary asynchronous UDP egress;
// network ownership is not part of the private-carrier contract.
const LAST_AP_SERVICE_RESERVED: bool = cfg!(feature = "trueos_h264_encode_stream");

static CORE_SPAWNER_BY_SLOT: [Mutex<Option<SendSpawner>>; WORKER_SLOT_LIMIT] =
    [const { Mutex::new(None) }; WORKER_SLOT_LIMIT];
static CORE_KIND_BY_SLOT: [AtomicU8; WORKER_SLOT_LIMIT] =
    [const { AtomicU8::new(CORE_KIND_UNKNOWN) }; WORKER_SLOT_LIMIT];
// Worker registration is monotonic. Publish a bit only after the per-slot
// spawner and kind are initialized, so readers can discover workers without a
// global registry lock or a second copy of the same topology state.
static REGISTERED_SLOTS: [AtomicU64; REGISTERED_SLOT_WORDS] =
    [const { AtomicU64::new(0) }; REGISTERED_SLOT_WORDS];
// Exclusive end of the registered slot span. This keeps placement scans bounded
// by discovered topology rather than the full 256-slot policy ceiling.
static REGISTERED_SLOT_END: AtomicU32 = AtomicU32::new(0);
static SPAWN_RR: AtomicU32 = AtomicU32::new(0);
static WORKER_SUMMARY_LOGGED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy)]
pub struct WorkerSpawner {
    cpu_slot: u32,
    spawner: SendSpawner,
}

impl WorkerSpawner {
    #[inline]
    pub const fn cpu_slot(self) -> u32 {
        self.cpu_slot
    }

    #[inline]
    pub fn spawn<S: Send>(&self, token: SpawnToken<S>) {
        let _ = self.spawn_and_wake_remote(token);
    }

    #[inline]
    pub fn spawn_and_wake_remote<S: Send>(&self, token: SpawnToken<S>) -> bool {
        self.spawner.spawn(token);
        crate::remote_work_wake::wake_cpu_for_remote_work(self.cpu_slot)
    }

    #[inline]
    pub fn spawned_task_count(&self) -> usize {
        self.spawner.spawned_task_count()
    }

    #[inline]
    pub fn ready_task_count(&self) -> usize {
        self.spawner.ready_task_count()
    }

    #[inline]
    pub const fn raw(self) -> SendSpawner {
        self.spawner
    }
}

#[inline]
fn worker_spawner(cpu_slot: u32, spawner: SendSpawner) -> WorkerSpawner {
    WorkerSpawner { cpu_slot, spawner }
}

#[inline]
fn registered_slot_word_and_mask(cpu_slot: u32) -> Option<(usize, u64)> {
    let slot = cpu_slot as usize;
    if slot >= WORKER_SLOT_LIMIT {
        return None;
    }
    let word = slot / REGISTERED_SLOT_WORD_BITS;
    let mask = 1u64 << (slot % REGISTERED_SLOT_WORD_BITS);
    Some((word, mask))
}

#[inline]
fn mark_slot_registered(cpu_slot: u32) {
    let Some((word, mask)) = registered_slot_word_and_mask(cpu_slot) else {
        return;
    };
    REGISTERED_SLOTS[word].fetch_or(mask, Ordering::Release);
    REGISTERED_SLOT_END.fetch_max(cpu_slot.saturating_add(1), Ordering::Release);
}

#[inline]
fn is_slot_registered(cpu_slot: u32) -> bool {
    let Some((word, mask)) = registered_slot_word_and_mask(cpu_slot) else {
        return false;
    };
    REGISTERED_SLOTS[word].load(Ordering::Acquire) & mask != 0
}

#[inline]
fn registered_slot_end() -> u32 {
    REGISTERED_SLOT_END
        .load(Ordering::Acquire)
        .min(WORKER_SLOT_LIMIT as u32)
}

#[inline]
fn registered_background_slot_range() -> core::ops::Range<u32> {
    FIRST_BACKGROUND_SLOT..registered_slot_end().max(FIRST_BACKGROUND_SLOT)
}

fn maybe_log_worker_summary(registered: usize) {
    let topology_slots = topology_core_slot_count();
    if topology_slots == 0 || registered < topology_slots {
        return;
    }

    if WORKER_SUMMARY_LOGGED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    let mut perf = 0;
    let mut eff = 0;
    let mut unknown = 0;
    for kind in CORE_KIND_BY_SLOT.iter().take(topology_slots) {
        match kind.load(Ordering::Acquire) {
            CORE_KIND_PERF => perf += 1,
            CORE_KIND_EFF => eff += 1,
            _ => unknown += 1,
        }
    }

    crate::log!(
        "workers: registration summary slots=0..{} registered={}/{} kinds(perf/eff/unknown)={}/{}/{} app_visible={} lastap_service_slot={:?}\n",
        topology_slots - 1,
        registered,
        topology_slots,
        perf,
        eff,
        unknown,
        app_visible_parallelism(),
        last_ap_service_slot(),
    );
}

pub fn register_core_spawner(cpu_slot: u32, core_kind: u8, spawner: Spawner) {
    let send_spawner = spawner.make_send();
    let Some(slot) = CORE_SPAWNER_BY_SLOT.get(cpu_slot as usize) else {
        crate::log!(
            "workers: ignoring registration outside slot limit slot={} limit={}\n",
            cpu_slot,
            WORKER_SLOT_LIMIT,
        );
        return;
    };

    *slot.lock() = Some(send_spawner);
    CORE_KIND_BY_SLOT[cpu_slot as usize].store(core_kind, Ordering::Release);
    mark_slot_registered(cpu_slot);

    let registered = registered_core_spawner_count();
    maybe_log_worker_summary(registered);
    if is_general_background_worker_slot(cpu_slot) {
        crate::r::blocking::start_service_lane_for_slot(cpu_slot);
    } else if is_last_ap_service_slot(cpu_slot) {
        crate::log_info!(target: "service";
            "lastap: reserved slot={} core_kind={} owner=ui4-scanout-h264 excluded_from=vm-hull+blocking-lanes+background-round-robin network_egress=one-way-ordinary-executor lifecycle=temporary-until-ap1-media-integration\n",
            cpu_slot,
            core_kind,
        );
    }
}

pub fn core_kind_for_slot(cpu_slot: u32) -> u8 {
    CORE_KIND_BY_SLOT
        .get(cpu_slot as usize)
        .map(|kind| kind.load(Ordering::Acquire))
        .unwrap_or(CORE_KIND_UNKNOWN)
}

pub fn raw_spawner_for_slot(cpu_slot: u32) -> Option<SendSpawner> {
    CORE_SPAWNER_BY_SLOT
        .get(cpu_slot as usize)
        .and_then(|slot| *slot.lock())
}

pub fn spawner_for_slot(cpu_slot: u32) -> Option<WorkerSpawner> {
    raw_spawner_for_slot(cpu_slot).map(|spawner| worker_spawner(cpu_slot, spawner))
}

pub fn ap1_ui_core_spawner() -> Option<WorkerSpawner> {
    spawner_for_slot(AP1_UI_SERVICE_SLOT)
}

/// Temporary exclusive service carrier at the final topology slot.
///
/// The identity is derived from topology rather than registration order, so
/// early AP registration cannot make the reservation migrate between cores.
pub fn last_ap_service_slot() -> Option<u32> {
    if !LAST_AP_SERVICE_RESERVED {
        return None;
    }
    let slot = topology_core_slot_count().checked_sub(1)?;
    if slot < FIRST_BACKGROUND_SLOT as usize || slot >= WORKER_SLOT_LIMIT {
        return None;
    }
    Some(slot as u32)
}

pub fn is_last_ap_service_slot(cpu_slot: u32) -> bool {
    last_ap_service_slot() == Some(cpu_slot)
}

pub fn is_general_background_worker_slot(cpu_slot: u32) -> bool {
    is_background_worker_slot(cpu_slot) && !is_last_ap_service_slot(cpu_slot)
}

pub fn last_ap_service_worker() -> Option<(u32, u8, WorkerSpawner)> {
    let slot = last_ap_service_slot()?;
    let spawner = spawner_for_slot(slot)?;
    Some((slot, core_kind_for_slot(slot), spawner))
}

pub fn background_slot_range() -> core::ops::Range<u32> {
    FIRST_BACKGROUND_SLOT..WORKER_SLOT_LIMIT as u32
}

pub fn background_worker_slots() -> Vec<u32> {
    registered_background_slot_range()
        .filter(|slot| is_slot_registered(*slot) && is_general_background_worker_slot(*slot))
        .collect()
}

/// Report whether a strict AP2+ performance-core worker is registered without
/// advancing the round-robin selector used by actual task placement.
pub fn has_perf_background_worker_slot() -> bool {
    registered_background_slot_range().any(|slot| {
        is_slot_registered(slot)
            && is_general_background_worker_slot(slot)
            && core_kind_for_slot(slot) == CORE_KIND_PERF
    })
}

pub fn registered_core_spawner_count() -> usize {
    REGISTERED_SLOTS
        .iter()
        .map(|word| word.load(Ordering::Acquire).count_ones() as usize)
        .sum()
}

pub fn topology_core_slot_count() -> usize {
    crate::smp::cpu_count().max(crate::percpu::total_slots())
}

pub fn all_topology_spawners_registered() -> bool {
    let topology_slots = topology_core_slot_count();
    topology_slots == 0 || registered_core_spawner_count() >= topology_slots
}

pub fn app_visible_parallelism() -> usize {
    let first_app_slot = if APP_PARALLELISM_NO_UI {
        1
    } else {
        FIRST_BACKGROUND_SLOT
    };
    let topology_slots = topology_core_slot_count();
    if topology_slots != 0 {
        let background = topology_slots.saturating_sub(first_app_slot as usize);
        let reserved = usize::from(
            last_ap_service_slot().is_some_and(|slot| slot as usize >= first_app_slot as usize),
        );
        return background.saturating_sub(reserved).max(1);
    }

    (first_app_slot..registered_slot_end().max(first_app_slot))
        .filter(|slot| is_slot_registered(*slot) && !is_last_ap_service_slot(*slot))
        .count()
        .max(1)
}

pub fn is_background_worker_slot(cpu_slot: u32) -> bool {
    cpu_slot >= FIRST_BACKGROUND_SLOT
}

pub fn has_background_worker_slot() -> bool {
    registered_background_slot_range()
        .any(|slot| is_slot_registered(slot) && is_general_background_worker_slot(slot))
}

pub fn pick_background_spawner() -> Option<WorkerSpawner> {
    pick_background_spawner_with_slot().map(|(_, _, spawner)| spawner)
}

pub fn pick_background_spawner_with_slot() -> Option<(u32, u8, WorkerSpawner)> {
    pick_background_spawner_with_filter(|_| true)
}

/// Pick up to `limit` distinct AP2+ workers using the kernel's core-profile
/// policy.
///
/// Profile-identified performance cores are preferred, then efficient or
/// unknown cores fill any remaining places. Callers that need a fixed-size
/// task pool may share the returned workers when the eligible fleet is smaller
/// than the pool.
pub fn pick_background_spawners_with_slots(limit: usize) -> Vec<(u32, u8, WorkerSpawner)> {
    let eligible = background_worker_slots().len().min(limit);
    let mut selected = Vec::with_capacity(eligible);
    while selected.len() < eligible {
        let Some(worker) = pick_background_spawner_with_filter(|slot| {
            !selected
                .iter()
                .any(|(selected_slot, _, _)| *selected_slot == slot)
        }) else {
            break;
        };
        selected.push(worker);
    }
    selected
}

pub fn pick_perf_background_spawner_with_slot() -> Option<(u32, u8, WorkerSpawner)> {
    pick_background_spawner_with_filter(|slot| core_kind_for_slot(slot) == CORE_KIND_PERF)
}

/// Pick an AP2+ efficiency-core worker.
///
/// This is a strict core-profile selector: it does not fall back to the BSP,
/// AP1, or a performance core. Callers may explicitly add an AP-only fallback
/// with `pick_background_spawner_with_slot` when running on machines without
/// profile-identified E-cores.
pub fn pick_eff_background_spawner_with_slot() -> Option<(u32, u8, WorkerSpawner)> {
    pick_background_spawner_with_filter(|slot| core_kind_for_slot(slot) == CORE_KIND_EFF)
}

fn pick_background_spawner_with_filter<F>(accept_slot: F) -> Option<(u32, u8, WorkerSpawner)>
where
    F: Fn(u32) -> bool,
{
    let perf_count = registered_background_slot_range()
        .filter(|slot| {
            is_slot_registered(*slot)
                && is_general_background_worker_slot(*slot)
                && accept_slot(*slot)
        })
        .filter(|slot| core_kind_for_slot(*slot) == CORE_KIND_PERF)
        .count();

    if perf_count != 0 {
        let idx = SPAWN_RR.fetch_add(1, Ordering::Relaxed) as usize % perf_count;
        let mut seen = 0;
        for slot in registered_background_slot_range() {
            if !is_slot_registered(slot)
                || !is_general_background_worker_slot(slot)
                || !accept_slot(slot)
            {
                continue;
            }
            let kind = core_kind_for_slot(slot);
            if kind != CORE_KIND_PERF {
                continue;
            }
            if seen == idx {
                let spawner = raw_spawner_for_slot(slot)?;
                return Some((slot, kind, worker_spawner(slot, spawner)));
            }
            seen += 1;
        }
        return None;
    }

    let eligible_count = registered_background_slot_range()
        .filter(|slot| {
            is_slot_registered(*slot)
                && is_general_background_worker_slot(*slot)
                && accept_slot(*slot)
        })
        .count();
    if eligible_count == 0 {
        return None;
    }

    let idx = SPAWN_RR.fetch_add(1, Ordering::Relaxed) as usize % eligible_count;
    let mut seen = 0;
    for slot in registered_background_slot_range() {
        if !is_slot_registered(slot)
            || !is_general_background_worker_slot(slot)
            || !accept_slot(slot)
        {
            continue;
        }
        if seen == idx {
            let kind = core_kind_for_slot(slot);
            let spawner = raw_spawner_for_slot(slot)?;
            return Some((slot, kind, worker_spawner(slot, spawner)));
        }
        seen += 1;
    }

    None
}
