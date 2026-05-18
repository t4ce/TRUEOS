extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU8, AtomicU32, Ordering};

use embassy_executor::{SendSpawner, Spawner};
use spin::Mutex;

pub const CORE_KIND_UNKNOWN: u8 = 0;
pub const CORE_KIND_PERF: u8 = 1;
pub const CORE_KIND_EFF: u8 = 2;

// Slot 0 is BSP and slot 1 is the UI2/service AP; background carriers start at AP2.
const FIRST_BACKGROUND_SLOT: u32 = 2;
const WORKER_SLOT_LIMIT: usize = crate::allcaps::hv::VM_CPU_SLOT_LIMIT;

static CORE_SPAWNERS: Mutex<BTreeMap<u32, SendSpawner>> = Mutex::new(BTreeMap::new());
static CORE_KINDS: Mutex<BTreeMap<u32, u8>> = Mutex::new(BTreeMap::new());
static CORE_SPAWNER_BY_SLOT: [Mutex<Option<SendSpawner>>; WORKER_SLOT_LIMIT] =
    [const { Mutex::new(None) }; WORKER_SLOT_LIMIT];
static CORE_KIND_BY_SLOT: [AtomicU8; WORKER_SLOT_LIMIT] =
    [const { AtomicU8::new(CORE_KIND_UNKNOWN) }; WORKER_SLOT_LIMIT];
static SPAWN_RR: AtomicU32 = AtomicU32::new(0);

pub fn register_core_spawner(cpu_slot: u32, core_kind: u8, spawner: Spawner) {
    let send_spawner = spawner.make_send();
    if let Some(slot) = CORE_SPAWNER_BY_SLOT.get(cpu_slot as usize) {
        *slot.lock() = Some(send_spawner);
        CORE_KIND_BY_SLOT[cpu_slot as usize].store(core_kind, Ordering::Release);
    }
    CORE_SPAWNERS.lock().insert(cpu_slot, send_spawner);
    CORE_KINDS.lock().insert(cpu_slot, core_kind);
}

#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_kernel_worker_register_core_spawner(
    cpu_slot: u32,
    core_kind: u8,
    spawner: Spawner,
) {
    register_core_spawner(cpu_slot, core_kind, spawner);
}

pub fn core_kind_for_slot(cpu_slot: u32) -> u8 {
    CORE_KIND_BY_SLOT
        .get(cpu_slot as usize)
        .map(|kind| kind.load(Ordering::Acquire))
        .unwrap_or(CORE_KIND_UNKNOWN)
}

#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_kernel_worker_core_kind_for_slot(cpu_slot: u32) -> u8 {
    core_kind_for_slot(cpu_slot)
}

pub fn spawner_for_slot(cpu_slot: u32) -> Option<SendSpawner> {
    CORE_SPAWNER_BY_SLOT
        .get(cpu_slot as usize)
        .and_then(|slot| *slot.lock())
}

pub fn background_slot_range() -> core::ops::Range<u32> {
    FIRST_BACKGROUND_SLOT..WORKER_SLOT_LIMIT as u32
}

#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_kernel_worker_spawner_for_slot(cpu_slot: u32) -> Option<SendSpawner> {
    spawner_for_slot(cpu_slot)
}

pub fn background_worker_slots() -> Vec<u32> {
    let map = CORE_SPAWNERS.lock();
    let mut out: Vec<u32> = map
        .keys()
        .copied()
        .filter(|slot| *slot >= FIRST_BACKGROUND_SLOT)
        .collect();
    out.sort_unstable();
    out
}

pub fn is_background_worker_slot(cpu_slot: u32) -> bool {
    cpu_slot >= FIRST_BACKGROUND_SLOT
}

#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_kernel_worker_background_worker_slots() -> Vec<u32> {
    background_worker_slots()
}

pub fn has_background_worker_slot() -> bool {
    CORE_SPAWNERS
        .lock()
        .keys()
        .any(|slot| *slot >= FIRST_BACKGROUND_SLOT)
}

pub fn pick_background_spawner() -> Option<SendSpawner> {
    pick_background_spawner_with_slot().map(|(_, _, spawner)| spawner)
}

pub fn pick_background_spawner_with_slot() -> Option<(u32, u8, SendSpawner)> {
    pick_background_spawner_with_filter(|_| true)
}

pub fn pick_background_spawner_where<F>(accept_slot: F) -> Option<(u32, u8, SendSpawner)>
where
    F: Fn(u32) -> bool,
{
    pick_background_spawner_with_filter(accept_slot)
}

fn pick_background_spawner_with_filter<F>(accept_slot: F) -> Option<(u32, u8, SendSpawner)>
where
    F: Fn(u32) -> bool,
{
    let map = CORE_SPAWNERS.lock();
    if map.is_empty() {
        return None;
    }

    let kinds = CORE_KINDS.lock();
    let perf_count = map
        .iter()
        .filter(|(slot, _)| **slot >= FIRST_BACKGROUND_SLOT && accept_slot(**slot))
        .filter(|(slot, _)| kinds.get(slot).copied().unwrap_or(CORE_KIND_UNKNOWN) == CORE_KIND_PERF)
        .count();

    if perf_count != 0 {
        let idx = SPAWN_RR.fetch_add(1, Ordering::Relaxed) as usize % perf_count;
        let mut seen = 0;
        for (slot, spawner) in map.iter() {
            if *slot < FIRST_BACKGROUND_SLOT || !accept_slot(*slot) {
                continue;
            }
            let kind = kinds.get(slot).copied().unwrap_or(CORE_KIND_UNKNOWN);
            if kind != CORE_KIND_PERF {
                continue;
            }
            if seen == idx {
                return Some((*slot, kind, spawner.clone()));
            }
            seen += 1;
        }
        return None;
    }

    let eligible_count = map
        .keys()
        .filter(|slot| **slot >= FIRST_BACKGROUND_SLOT && accept_slot(**slot))
        .count();
    if eligible_count == 0 {
        return None;
    }

    let idx = SPAWN_RR.fetch_add(1, Ordering::Relaxed) as usize % eligible_count;
    let mut seen = 0;
    for (slot, spawner) in map.iter() {
        if *slot < FIRST_BACKGROUND_SLOT || !accept_slot(*slot) {
            continue;
        }
        if seen == idx {
            let kind = kinds.get(slot).copied().unwrap_or(CORE_KIND_UNKNOWN);
            return Some((*slot, kind, spawner.clone()));
        }
        seen += 1;
    }

    None
}

#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_kernel_worker_pick_background_spawner_with_slot()
-> Option<(u32, u8, SendSpawner)> {
    pick_background_spawner_with_slot()
}
