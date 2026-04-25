extern crate alloc;

use alloc::boxed::Box;
use core::sync::atomic::{AtomicBool, Ordering};

type TokioBlockingJob = Box<dyn FnOnce() + Send + 'static>;

static LOGGED_NO_WORKER: AtomicBool = AtomicBool::new(false);
static LOGGED_SPAWN: AtomicBool = AtomicBool::new(false);

#[embassy_executor::task(pool_size = 64)]
async fn tokio_blocking_job_task(job: TokioBlockingJob) {
    job();
}

fn reject_until_background_ap_ready() -> i32 {
    if !LOGGED_NO_WORKER.swap(true, Ordering::AcqRel) {
        crate::log!("tokio-worker: no AP>2 background spawner yet; blocking job not launched\n");
    }
    -2
}

fn spawn_on_background_ap(job: TokioBlockingJob) -> i32 {
    let Some(spawner) = crate::workers::pick_background_spawner() else {
        let _ = job;
        return reject_until_background_ap_ready();
    };

    let token = match tokio_blocking_job_task(job) {
        Ok(token) => token,
        Err(_) => return -3,
    };

    if !LOGGED_SPAWN.swap(true, Ordering::AcqRel) {
        crate::log!("tokio-worker: using TRUEOS AP>2 background spawners for blocking jobs\n");
    }

    spawner.spawn(token);
    0
}

#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_tokio_spawn_blocking_job(job: TokioBlockingJob) -> i32 {
    spawn_on_background_ap(job)
}
