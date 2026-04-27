extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_executor::{SendSpawner, Spawner};
use spin::Mutex;

pub const CORE_KIND_UNKNOWN: u8 = 0;
pub const CORE_KIND_PERF: u8 = 1;
pub const CORE_KIND_EFF: u8 = 2;

// Slot 0 is BSP and slot 1 is the UI2/service AP; background carriers start at AP2.
const FIRST_BACKGROUND_SLOT: u32 = 2;

static CORE_SPAWNERS: Mutex<BTreeMap<u32, SendSpawner>> = Mutex::new(BTreeMap::new());
static CORE_KINDS: Mutex<BTreeMap<u32, u8>> = Mutex::new(BTreeMap::new());
static SPAWN_RR: AtomicU32 = AtomicU32::new(0);

pub fn register_core_spawner(cpu_slot: u32, core_kind: u8, spawner: Spawner) {
    CORE_SPAWNERS.lock().insert(cpu_slot, spawner.make_send());
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
    CORE_KINDS
        .lock()
        .get(&cpu_slot)
        .copied()
        .unwrap_or(CORE_KIND_UNKNOWN)
}

#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_kernel_worker_core_kind_for_slot(cpu_slot: u32) -> u8 {
    core_kind_for_slot(cpu_slot)
}

pub fn spawner_for_slot(cpu_slot: u32) -> Option<SendSpawner> {
    CORE_SPAWNERS.lock().get(&cpu_slot).cloned()
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
    let map = CORE_SPAWNERS.lock();
    if map.is_empty() {
        return None;
    }

    let kinds = CORE_KINDS.lock();
    let perf_count = map
        .iter()
        .filter(|(slot, _)| **slot >= FIRST_BACKGROUND_SLOT)
        .filter(|(slot, _)| kinds.get(slot).copied().unwrap_or(CORE_KIND_UNKNOWN) == CORE_KIND_PERF)
        .count();

    if perf_count != 0 {
        let idx = SPAWN_RR.fetch_add(1, Ordering::Relaxed) as usize % perf_count;
        let mut seen = 0;
        for (slot, spawner) in map.iter() {
            if *slot < FIRST_BACKGROUND_SLOT {
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
        .filter(|slot| **slot >= FIRST_BACKGROUND_SLOT)
        .count();
    if eligible_count == 0 {
        return None;
    }

    let idx = SPAWN_RR.fetch_add(1, Ordering::Relaxed) as usize % eligible_count;
    let mut seen = 0;
    for (slot, spawner) in map.iter() {
        if *slot < FIRST_BACKGROUND_SLOT {
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
