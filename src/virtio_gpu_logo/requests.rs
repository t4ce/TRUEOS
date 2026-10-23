//! Serialized offscreen work on the same fenced virgl context as UI4.
//! Jobs own their input pixels. Only a live caller writes returned pixels to
//! its leased destination, so cancelling a waiter cannot leave borrowed DMA.
use super::{VirtioGpuLogo, virgl::Compositor};
use alloc::{boxed::Box, collections::VecDeque, sync::Arc};
use core::sync::atomic::{AtomicBool, Ordering};

type Job = Box<dyn FnOnce(Option<(&mut Compositor, &mut VirtioGpuLogo)>) + Send>;
static JOBS: spin::Mutex<VecDeque<Job>> = spin::Mutex::new(VecDeque::new());
static READY: AtomicBool = AtomicBool::new(false);
const MAX_PENDING: usize = 32;

struct Completion<T> {
    value: spin::Mutex<Option<Result<T, &'static str>>>,
    wait: crate::wait::WaitQueue,
}
impl<T> Completion<T> {
    fn take(&self) -> Option<Result<T, &'static str>> { self.value.lock().take() }
    fn complete(&self, value: Result<T, &'static str>) {
        *self.value.lock() = Some(value);
        self.wait.notify_all();
    }
}

pub(super) fn available() -> bool { READY.load(Ordering::Acquire) }
pub(super) fn start() { READY.store(true, Ordering::Release); }

fn enqueue<T: Send + 'static>(
    operation: impl FnOnce(&mut Compositor, &mut VirtioGpuLogo) -> Result<T, &'static str> + Send + 'static,
) -> Result<Arc<Completion<T>>, &'static str> {
    let result = Arc::new(Completion { value: spin::Mutex::new(None), wait: crate::wait::WaitQueue::new() });
    let completion = Arc::clone(&result);
    let mut queue = JOBS.lock();
    if !available() { return Err("virgl-offscreen-unavailable"); }
    if queue.len() >= MAX_PENDING { return Err("virgl-offscreen-busy"); }
    queue.push_back(Box::new(move |context| {
        let value = match context {
            Some((compositor, gpu)) => operation(compositor, gpu),
            None => Err("virgl-offscreen-device-failed"),
        };
        completion.complete(value);
    }));
    Ok(result)
}

pub(super) async fn dispatch<T: Send + 'static>(
    operation: impl FnOnce(&mut Compositor, &mut VirtioGpuLogo) -> Result<T, &'static str> + Send + 'static,
) -> Result<T, &'static str> {
    let completion = enqueue(operation)?;
    loop {
        let observed = completion.wait.observe();
        if let Some(value) = completion.take() { return value; }
        completion.wait.wait_after(observed).await;
    }
}

pub(super) fn dispatch_blocking<T: Send + 'static>(
    operation: impl FnOnce(&mut Compositor, &mut VirtioGpuLogo) -> Result<T, &'static str> + Send + 'static,
) -> Result<T, &'static str> {
    let completion = enqueue(operation)?;
    loop {
        let observed = completion.wait.observe();
        if let Some(value) = completion.take() { return value; }
        completion.wait.wait_for_event_after_blocking_parked(observed, 0);
    }
}

pub(super) fn service(compositor: &mut Compositor, gpu: &mut VirtioGpuLogo) {
    // Bound one turn so offscreen clients cannot starve scanout and input.
    for _ in 0..4 {
        let Some(job) = JOBS.lock().pop_front() else { break; };
        if gpu.failed { job(None); } else { job(Some((compositor, gpu))); }
    }
}

pub(super) fn stop() {
    READY.store(false, Ordering::Release);
    loop {
        let Some(job) = JOBS.lock().pop_front() else { break; };
        job(None);
    }
}
