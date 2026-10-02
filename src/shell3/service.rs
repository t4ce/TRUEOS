//! AP-backed worker startup for Shell3 draw execution.

use alloc::vec::Vec;
use trueos_executor::{SpawnError, SpawnToken};

/// Compile-time upper bound for this service's task pool.
pub const MAX_WORKERS: usize = 256;

/// Runtime worker limit: at most half of all discovered AP cores.
pub fn worker_limit() -> usize {
    let ap_cores = crate::workers::topology_core_slot_count().saturating_sub(1);
    (ap_cores / 2).min(MAX_WORKERS)
}

/// Start one caller-provided worker task on each selected AP executor.
///
/// The task factory receives the AP slot so workers can retain CPU affinity.
/// Fewer workers may start when some AP executors are not registered yet.
pub fn start<S: Send>(
    mut task: impl FnMut(u32) -> Result<SpawnToken<S>, SpawnError>,
) -> Result<usize, SpawnError> {
    let target = worker_limit();
    if target == 0 {
        return Ok(0);
    }

    let ap_count = crate::workers::topology_core_slot_count().saturating_sub(1);
    let mut spawners = Vec::with_capacity(target);
    for slot in 1..=ap_count as u32 {
        if let Some(spawner) = crate::workers::spawner_for_slot(slot) {
            spawners.push((slot, spawner));
            if spawners.len() == target {
                break;
            }
        }
    }

    let mut started = 0;
    for (slot, spawner) in spawners {
        let token = task(slot)?;
        spawner.spawn(token);
        started += 1;
    }
    Ok(started)
}
