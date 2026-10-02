//! AP-backed worker startup for Shell3 draw execution.

use alloc::vec::Vec;
use trueos_executor::{SpawnError, SpawnToken, Spawner};
use trueos_time::{Duration, Timer};

const TOPOLOGY_TASK_POOL_CAPACITY: usize = crate::percpu::CPU_SLOT_LIMIT;
static DRAW_WORK_AVAILABLE: crate::wait::WaitQueue = crate::wait::WaitQueue::new();

/// Runtime worker limit: at most half of all discovered AP cores.
pub fn worker_limit() -> usize {
    let ap_cores = crate::workers::topology_core_slot_count().saturating_sub(1);
    ap_cores / 2
}

/// Start one caller-provided worker task on each selected AP executor.
///
/// The task factory receives the AP slot so workers can retain CPU affinity.
/// Fewer workers may start when some AP executors are not registered yet.
pub fn start<S: Send>(
    mut task: impl FnMut(usize, u32) -> Result<SpawnToken<S>, SpawnError>,
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
    for (worker_id, (slot, spawner)) in spawners.into_iter().enumerate() {
        let token = task(worker_id, slot)?;
        spawner.spawn(token);
        started += 1;
    }
    Ok(started)
}

/// Notify resident workers that draw work may be available.
pub fn notify_draw_work() {
    DRAW_WORK_AVAILABLE.notify_all();
}

#[trueos_executor::task(pool_size = TOPOLOGY_TASK_POOL_CAPACITY)]
async fn draw_worker_task(worker_id: usize, expected_slot: u32) {
    let actual_slot = u32::try_from(crate::percpu::current_slot()).unwrap_or(u32::MAX);
    if actual_slot != expected_slot {
        crate::log_warn!(target: "service";
            "sh3srv: worker placement mismatch worker={} expected_slot={} actual_slot={} action=retire-worker\n",
            worker_id,
            expected_slot,
            actual_slot,
        );
        return;
    }

    loop {
        let observed = DRAW_WORK_AVAILABLE.observe();
        DRAW_WORK_AVAILABLE.wait_after(observed).await;
        // The draw queue and CPU/render/copy execution paths will be attached here.
    }
}

/// Resident Shell3 service controller. It waits for the full discovered AP
/// topology to register, then starts at most half of those APs as draw workers.
#[trueos_executor::task]
pub async fn sh3srv_service_task(_spawner: Spawner) {
    while !crate::workers::all_topology_spawners_registered() {
        Timer::after(Duration::from_millis(25)).await;
    }

    match start(|worker_id, slot| draw_worker_task(worker_id, slot)) {
        Ok(started) => crate::log_info!(target: "service";
            "sh3srv: online workers={} ap_total={} policy=half-ap-topology\n",
            started,
            crate::workers::topology_core_slot_count().saturating_sub(1),
        ),
        Err(error) => crate::log_error!(target: "service";
            "sh3srv: worker startup failed error={:?}\n",
            error,
        ),
    }
}
