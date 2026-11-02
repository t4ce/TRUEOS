//! Kernel-owned admission, independent of the submitting Hull lifetime.
use alloc::{boxed::Box, format, vec, vec::Vec};
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;
use trueos_time::{Duration, Timer};
#[path = "download_queue.rs"]
mod queue;
use crate::r::cabi_codes::*;
use queue::{Next, Queue};

pub const PREFIX: &str = "vFile:hulldownloads/";
pub const MAX_FRAME: usize = queue::MAX_RECEIVED_FRAME;
static QUEUE: Mutex<Queue> = Mutex::new(Queue::new());

fn worker_identity(owner: u32) -> Result<u64, i32> {
    let vm = owner & !0x8000_0000;
    if owner & 0x8000_0000 == 0
        || vm > u8::MAX as u32
        || crate::hv::app_vm_archive(vm as u8).as_deref() != Some("trauma.bp")
    {
        return Err(FS_ERR_BAD_PARAM);
    }
    crate::hv::vm_run_generation(vm as u8).ok_or(FS_ERR_BAD_PARAM)
}
fn generation(path: &str, operation: &str) -> Result<u64, i32> {
    path.strip_prefix(PREFIX)
        .and_then(|p| p.strip_prefix(operation))
        .and_then(|p| p.parse().ok())
        .ok_or(FS_ERR_BAD_PARAM)
}
pub fn read_for_owner(owner: u32, path: &str) -> Result<Vec<u8>, i32> {
    let generation = generation(path, "next/")?;
    let run = worker_identity(owner)?;
    match QUEUE
        .lock()
        .next(generation, owner, run)
        .map_err(|_| FS_ERR_BAD_PARAM)?
    {
        Next::Job(bytes) => Ok(bytes),
        Next::Wait => Ok(vec![b'W']),
        Next::Drained => Ok(vec![b'D']),
    }
}
pub fn write_for_owner(owner: u32, path: &str, bytes: &[u8]) -> Result<(), i32> {
    if matches!(
        path,
        "vFile:hulldownloads/submit" | "vFile:hulldownloads/received"
    ) {
        let received = path.ends_with("/received");
        if !(if received {
            queue::valid_received(bytes)
        } else {
            queue::valid_submission(bytes)
        }) {
            return Err(FS_ERR_BAD_PARAM);
        }
        let spawner = crate::workers::pick_background_spawner().ok_or(FS_ERR_IO)?;
        let mut queue = QUEUE.lock();
        let id = (if received {
            queue.submit_received(bytes)
        } else {
            queue.submit(bytes)
        })
        .map_err(|_| FS_ERR_NO_SPACE)?;
        if let Some(generation) = queue.begin() {
            let task = match launch_worker(generation, None) {
                Ok(task) => task,
                Err(_) => {
                    queue.cancel_pending(id);
                    queue.failed(generation);
                    return Err(FS_ERR_NO_SPACE);
                }
            };
            spawner.spawn(task);
        }
        return Ok(());
    }
    let generation = generation(path, "done/")?;
    let id = u64::from_le_bytes(bytes.try_into().map_err(|_| FS_ERR_BAD_PARAM)?);
    let run = worker_identity(owner)?;
    QUEUE
        .lock()
        .done(generation, owner, run, id)
        .map_err(|_| FS_ERR_BAD_PARAM)
}

struct Observer {
    generation: u64,
    run: AtomicU64,
}
impl crate::hv::launcher::LaunchObserver for Observer {
    fn started(&self, vm: u8) {
        if let Some(run) = crate::hv::vm_run_generation(vm) {
            self.run.store(run, Ordering::Release);
            QUEUE
                .lock()
                .started(self.generation, 0x8000_0000 | u32::from(vm), run);
        }
    }
    fn finished(
        &self,
        target: &crate::shell3::MatrixTarget,
        vm: Option<u8>,
        error: Option<&str>,
        _reason: Option<&str>,
    ) {
        // Matrix deletion can notify before the guest has drained its native
        // I/O. The successor explicitly waits for this VM incarnation to stop.
        crate::hv::matrix::discard(target);
        let mut queue = QUEUE.lock();
        if queue.failed(self.generation) {
            if let Some(error) = error {
                crate::log!("hull-downloads: worker failed: {}\n", error);
            }
            if let Some(spawner) = crate::workers::pick_background_spawner() {
                let Some(generation) = queue.begin() else {
                    return;
                };
                match launch_worker(
                    generation,
                    vm.map(|vm| (vm, self.run.load(Ordering::Acquire))),
                ) {
                    Ok(task) => spawner.spawn(task),
                    Err(_) => {
                        queue.failed(generation);
                        crate::log!("hull-downloads: restart pool exhausted; work retained\n");
                    }
                }
            }
        }
    }
}

#[trueos_executor::task(pool_size = 16)]
async fn launch_worker(mut generation: u64, predecessor: Option<(u8, u64)>) {
    if let Some((vm, run)) = predecessor {
        loop {
            let state = crate::hv::vm_state(vm);
            // Offline is published only after native I/O and carrier teardown.
            // A changed run means the previous incarnation already retired.
            if crate::hv::vm_run_generation(vm) != Some(run) || (!state.running && !state.starting)
            {
                break;
            }
            Timer::after(Duration::from_millis(100)).await;
        }
        Timer::after(Duration::from_secs(1)).await;
    }
    loop {
        let result = match crate::app_db::get("trauma.bp") {
            Ok(Some(bytes)) => {
                let mut request =
                    crate::hv::launcher::LaunchRequest::new("trauma.bp".into(), bytes);
                request.script = Some(format!("download-worker {generation}\n"));
                crate::hv::launcher::enqueue(
                    request,
                    Some(Box::new(Observer {
                        generation,
                        run: AtomicU64::new(0),
                    })),
                )
                .map(|_| ())
            }
            Ok(None) => Err("trauma.bp is absent from AppDB".into()),
            Err(error) => Err(error),
        };
        match result {
            Ok(()) => return,
            Err(error) => crate::log!("hull-downloads: launch deferred: {}\n", error),
        }
        // Transient launch pressure must not lose already acknowledged jobs.
        Timer::after(Duration::from_secs(1)).await;
        let mut queue = QUEUE.lock();
        queue.failed(generation);
        match queue.begin() {
            Some(next) => generation = next,
            None => return,
        }
    }
}
