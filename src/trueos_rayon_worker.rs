extern crate alloc;
extern crate std;

use alloc::{boxed::Box, format, string::ToString};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

type RayonWorkerJob = Box<dyn FnOnce() + Send + 'static>;

const ENABLE_RAYON_GLOBAL_POOL_EXPERIMENT: bool = true;
const RAYON_GLOBAL_POOL_THREAD_CAP: usize = 1;

static INIT_OK: AtomicBool = AtomicBool::new(false);
static LOGGED_NO_WORKER: AtomicBool = AtomicBool::new(false);
static LOGGED_NO_LANE: AtomicBool = AtomicBool::new(false);
static LOGGED_DISABLED: AtomicBool = AtomicBool::new(false);
static LOGGED_SPAWN: AtomicBool = AtomicBool::new(false);
static LOGGED_BUILD_FAIL: AtomicBool = AtomicBool::new(false);
static TRACE_SEQ: AtomicU32 = AtomicU32::new(1);

fn trace(stage: &str) {
    let seq = TRACE_SEQ.fetch_add(1, Ordering::Relaxed);
    crate::log!(
        "rayon-worker-trace: seq={} stage={} cpu_slot={} lane={}\n",
        seq,
        stage,
        crate::percpu::current_slot(),
        crate::stackkeeper::trueos_tokio_tls_current_slot()
    );
}

#[embassy_executor::task(pool_size = 64)]
async fn rayon_worker_job_task(job: RayonWorkerJob, lane: crate::stackkeeper::TokioLaneLease) {
    let tag = lane.tag();
    crate::log!(
        "rayon-worker-trace: task-enter lane{} cpu_slot={} tag_cpu_slot={} scratch={:#x}+{}\n",
        tag.lane_id,
        crate::percpu::current_slot(),
        tag.cpu_slot,
        tag.scratch_base,
        tag.scratch_len
    );
    let _guard = crate::stackkeeper::enter_tokio_lane(lane, "rayon-worker");
    trace("task-after-enter-lane");
    crate::log!("rayon-worker-trace: task-before-thread-run lane{}\n", tag.lane_id);
    job();
    crate::log!("rayon-worker-trace: task-after-thread-run lane{}\n", tag.lane_id);
    drop(_guard);
    trace("task-after-drop-lane-guard");
    let _ = crate::stackkeeper::release_tokio_lane(lane);
    trace("task-after-release-lane");
}

fn reject_until_background_ap_ready() -> i32 {
    if !LOGGED_NO_WORKER.swap(true, Ordering::AcqRel) {
        crate::log!("rayon-worker: no AP2+ background spawner yet; global pool deferred\n");
    }
    -2
}

fn spawn_on_background_ap(job: RayonWorkerJob) -> i32 {
    trace("spawn-select-begin");
    let Some((cpu_slot, core_kind, spawner)) = crate::workers::pick_background_spawner_with_slot()
    else {
        let _ = job;
        return reject_until_background_ap_ready();
    };
    crate::log!(
        "rayon-worker-trace: spawn-selected cpu_slot={} core_kind={} smp={:?}\n",
        cpu_slot,
        core_kind,
        crate::smp::read(cpu_slot as usize)
    );

    let Some(lane) =
        crate::stackkeeper::try_acquire_tokio_lane(cpu_slot, core_kind, "rayon-worker")
    else {
        let _ = job;
        if !LOGGED_NO_LANE.swap(true, Ordering::AcqRel) {
            crate::log!(
                "rayon-worker: no free TRUEOS worker lane for cpu_slot={}; worker not launched\n",
                cpu_slot
            );
        }
        return -4;
    };

    let tag = lane.tag();
    crate::log!(
        "rayon-worker-trace: lane-acquired lane{} cpu_slot={} core_kind={} generation={} scratch={:#x}+{}\n",
        tag.lane_id,
        tag.cpu_slot,
        tag.core_kind,
        tag.generation,
        tag.scratch_base,
        tag.scratch_len
    );
    let token = match rayon_worker_job_task(job, lane) {
        Ok(token) => token,
        Err(_) => {
            crate::log!("rayon-worker-trace: task-token-build-failed lane{}\n", tag.lane_id);
            let _ = crate::stackkeeper::release_tokio_lane(lane);
            return -3;
        }
    };

    if !LOGGED_SPAWN.swap(true, Ordering::AcqRel) {
        crate::log!(
            "rayon-worker: using TRUEOS AP2+ Embassy workers tag=0x{:08X} vm={} domain={} role={} lane{} cpu_slot={} core_kind={} scratch={:#x}+{}\n",
            tag.magic,
            tag.vm_id,
            tag.domain,
            tag.role,
            tag.lane_id,
            tag.cpu_slot,
            tag.core_kind,
            tag.scratch_base,
            tag.scratch_len
        );
    }

    crate::log!("rayon-worker-trace: spawner-spawn lane{} begin\n", tag.lane_id);
    spawner.spawn(token);
    crate::log!("rayon-worker-trace: spawner-spawn lane{} submitted\n", tag.lane_id);
    0
}

pub fn init_global_pool() -> bool {
    trace("init-enter");
    if !ENABLE_RAYON_GLOBAL_POOL_EXPERIMENT {
        if !LOGGED_DISABLED.swap(true, Ordering::AcqRel) {
            crate::log!(
                "rayon-worker: global pool disabled; Rayon workers park AP lanes permanently\n"
            );
        }
        return false;
    }

    if INIT_OK.load(Ordering::Acquire) {
        return true;
    }

    let background_slots = crate::workers::background_worker_slots();
    crate::log!(
        "rayon-worker-trace: init-background-slots slots={:?} cap={} lanes={}\n",
        background_slots,
        RAYON_GLOBAL_POOL_THREAD_CAP,
        crate::stackkeeper::TOKIO_LANE_COUNT
    );
    for slot in background_slots.iter().copied() {
        crate::log!(
            "rayon-worker-trace: init-slot slot={} smp={:?} core_kind={}\n",
            slot,
            crate::smp::read(slot as usize),
            crate::workers::core_kind_for_slot(slot)
        );
    }

    let thread_count = core::cmp::min(
        core::cmp::min(background_slots.len(), crate::stackkeeper::TOKIO_LANE_COUNT),
        RAYON_GLOBAL_POOL_THREAD_CAP,
    );
    if thread_count == 0 {
        let _ = reject_until_background_ap_ready();
        return false;
    }

    trace("build-global-begin");
    match rayon::ThreadPoolBuilder::new()
        .num_threads(thread_count)
        .thread_name(|idx| format!("trueos-rayon-{}", idx))
        .start_handler(|idx| {
            crate::log!(
                "rayon-worker-trace: start-handler thread{} cpu_slot={} lane={}\n",
                idx,
                crate::percpu::current_slot(),
                crate::stackkeeper::trueos_tokio_tls_current_slot()
            );
        })
        .exit_handler(|idx| {
            crate::log!(
                "rayon-worker-trace: exit-handler thread{} cpu_slot={} lane={}\n",
                idx,
                crate::percpu::current_slot(),
                crate::stackkeeper::trueos_tokio_tls_current_slot()
            );
        })
        .panic_handler(|payload| {
            let _ = payload;
            crate::log!(
                "rayon-worker-trace: panic-handler cpu_slot={} lane={}\n",
                crate::percpu::current_slot(),
                crate::stackkeeper::trueos_tokio_tls_current_slot()
            );
        })
        .spawn_handler(|thread| {
            let index = thread.index();
            crate::log!(
                "rayon-worker-trace: spawn-handler thread{} cpu_slot={} lane={}\n",
                index,
                crate::percpu::current_slot(),
                crate::stackkeeper::trueos_tokio_tls_current_slot()
            );
            let job: RayonWorkerJob = Box::new(move || {
                crate::log!(
                    "rayon-worker-trace: thread-run-enter thread{} cpu_slot={} lane={}\n",
                    index,
                    crate::percpu::current_slot(),
                    crate::stackkeeper::trueos_tokio_tls_current_slot()
                );
                thread.run();
                crate::log!(
                    "rayon-worker-trace: thread-run-return thread{} cpu_slot={} lane={}\n",
                    index,
                    crate::percpu::current_slot(),
                    crate::stackkeeper::trueos_tokio_tls_current_slot()
                );
            });
            let rc = spawn_on_background_ap(job);
            crate::log!("rayon-worker-trace: spawn-handler thread{} rc={}\n", index, rc);
            if rc == 0 {
                Ok(())
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("TRUEOS Rayon worker{} spawn failed rc={}", index, rc),
                ))
            }
        })
        .build_global()
    {
        Ok(()) => {
            trace("build-global-ok");
            INIT_OK.store(true, Ordering::Release);
            crate::log!(
                "rayon-worker: global pool initialized threads={} background_slots={:?} lanes={} cap={}\n",
                thread_count,
                background_slots,
                crate::stackkeeper::TOKIO_LANE_COUNT,
                RAYON_GLOBAL_POOL_THREAD_CAP
            );
            true
        }
        Err(err) => {
            trace("build-global-err");
            if err.to_string().contains("already been initialized") {
                INIT_OK.store(true, Ordering::Release);
                crate::log!("rayon-worker: global pool already initialized\n");
                return true;
            }
            if !LOGGED_BUILD_FAIL.swap(true, Ordering::AcqRel) {
                crate::log!(
                    "rayon-worker: global pool init failed threads={} err={}\n",
                    thread_count,
                    err
                );
            }
            false
        }
    }
}
