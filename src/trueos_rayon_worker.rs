extern crate alloc;
extern crate std;

use alloc::{boxed::Box, format, string::ToString};
use core::sync::atomic::{AtomicBool, Ordering};

type RayonWorkerJob = Box<dyn FnOnce() + Send + 'static>;

const ENABLE_RAYON_GLOBAL_POOL_EXPERIMENT: bool = true;
const RAYON_GLOBAL_POOL_THREAD_CAP: usize = 5;

static INIT_OK: AtomicBool = AtomicBool::new(false);
static LOGGED_NO_WORKER: AtomicBool = AtomicBool::new(false);
static LOGGED_DISABLED: AtomicBool = AtomicBool::new(false);
static LOGGED_SPAWN: AtomicBool = AtomicBool::new(false);
static LOGGED_BUILD_FAIL: AtomicBool = AtomicBool::new(false);

fn reject_until_background_ap_ready() -> i32 {
    if !LOGGED_NO_WORKER.swap(true, Ordering::AcqRel) {
        crate::log!("rayon-worker: TRUEOS Tokio threads not ready; global pool deferred\n");
    }
    -2
}

fn spawn_on_tokio_thread(job: RayonWorkerJob) -> i32 {
    if !crate::workers::has_background_worker_slot() {
        let _ = job;
        return reject_until_background_ap_ready();
    }

    if !LOGGED_SPAWN.swap(true, Ordering::AcqRel) {
        crate::log!("rayon-worker: delegating Rayon workers to TRUEOS Tokio blocking threads\n");
    }

    crate::trueos_tokio_worker::spawn_blocking_job_with_purpose(job, "rayon-worker")
}

pub fn init_global_pool() -> bool {
    if !ENABLE_RAYON_GLOBAL_POOL_EXPERIMENT {
        if !LOGGED_DISABLED.swap(true, Ordering::AcqRel) {
            crate::log!(
                "rayon-worker: global pool disabled; Rayon workers need dedicated carriers, not Tokio blocking lanes\n"
            );
        }
        return false;
    }

    if INIT_OK.load(Ordering::Acquire) {
        return true;
    }

    let background_slots = crate::workers::background_worker_slots();
    let thread_count = core::cmp::min(
        core::cmp::min(background_slots.len(), crate::stackkeeper::TOKIO_LANE_COUNT),
        RAYON_GLOBAL_POOL_THREAD_CAP,
    );
    if thread_count == 0 {
        let _ = reject_until_background_ap_ready();
        return false;
    }

    match rayon::ThreadPoolBuilder::new()
        .num_threads(thread_count)
        .thread_name(|idx| format!("trueos-rayon-{}", idx))
        .panic_handler(|payload| {
            let _ = payload;
            crate::log!("rayon-worker: worker panic reported by Rayon\n");
        })
        .spawn_handler(|thread| {
            let index = thread.index();
            let job: RayonWorkerJob = Box::new(move || thread.run());
            let rc = spawn_on_tokio_thread(job);
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
            INIT_OK.store(true, Ordering::Release);
            crate::log!(
                "rayon-worker: global pool initialized via TRUEOS Tokio threads threads={} background_slots={:?} lanes={} cap={}\n",
                thread_count,
                background_slots,
                crate::stackkeeper::TOKIO_LANE_COUNT,
                RAYON_GLOBAL_POOL_THREAD_CAP
            );
            true
        }
        Err(err) => {
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
