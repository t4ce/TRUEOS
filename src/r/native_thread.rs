//! Native thread completion handles. Compatibility pthread calls delegate here.

use alloc::{boxed::Box, collections::BTreeMap, sync::Arc};
use core::ffi::c_void;
use core::sync::atomic::{AtomicUsize, Ordering};
use spin::Mutex;

const WAIT_KEY_TAG: u64 = 0x5448_5244_0000_0000;
const CARRIER_TAG: usize = crate::wls::BLUEPRINT_THREAD_CARRIER_TAG as usize;
const SEQUENCE_MASK: usize = CARRIER_TAG - 1;
const _: () = assert!(CARRIER_TAG.is_power_of_two() && (CARRIER_TAG | SEQUENCE_MASK) <= u32::MAX as usize);

/// Spawned logical executions expose their join handle as their public ID.
/// Main/worker identities occupy a disjoint range above every u32 handle.
pub(crate) fn current_id() -> usize {
    crate::r::threads::current_id()
        .unwrap_or_else(|| 0x1_0000_0000usize | crate::r::native_tls::current_identity())
}

struct Handle {
    completion: Arc<crate::wait::CompletionCell<usize>>,
    joining: bool,
}

pub(crate) struct State {
    handles: Mutex<BTreeMap<usize, Handle>>,
    sequence: AtomicUsize,
}

impl State {
    pub(crate) const fn new() -> Self {
        Self { handles: Mutex::new(BTreeMap::new()), sequence: AtomicUsize::new(0) }
    }

    fn register(&self) -> Result<(usize, Arc<crate::wait::CompletionCell<usize>>), i32> {
        // Never reuse a handle within one live process, even after detach.
        let sequence = self.sequence.try_update(Ordering::AcqRel, Ordering::Acquire,
            |previous| previous.checked_add(1).filter(|next| *next <= SEQUENCE_MASK))
            .map_err(|_| 11)? + 1;
        let id = if crate::hv::current_hull_guest_context_vm_id().is_some() { sequence } else { sequence | CARRIER_TAG };
        let completion = Arc::new(crate::wait::CompletionCell::new());
        self.handles.lock().insert(id, Handle { completion: completion.clone(), joining: false });
        Ok((id, completion))
    }

    fn claim(&self, thread: usize) -> Result<Arc<crate::wait::CompletionCell<usize>>, i32> {
        let mut handles = self.handles.lock();
        let handle = handles.get_mut(&thread).ok_or(3)?;
        if handle.joining { return Err(22); }
        handle.joining = true;
        Ok(handle.completion.clone())
    }

    pub(crate) fn contains(&self, thread: usize) -> bool { self.handles.lock().contains_key(&thread) }
    pub(crate) fn live_count(&self) -> usize { self.handles.lock().len() }

    pub(crate) fn reset(&self) {
        self.handles.lock().clear();
        self.sequence.store(0, Ordering::Release);
    }
}

fn wait_key(thread: usize) -> u64 { WAIT_KEY_TAG | thread as u64 }

pub(crate) fn spawn(
    stack: usize,
    entry: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    arg: *mut c_void,
    publish: impl FnOnce(usize) -> bool,
) -> Result<usize, i32> {
    let (id, completion) = crate::r::native_process::with_current(|state| state.threads.register())??;
    if !publish(id) {
        crate::r::native_process::current().threads.handles.lock().remove(&id);
        return Err(22);
    }
    let arg = arg as usize;
    let job = crate::r::native_process::with_current(|_| Box::new(move || {
        let result = unsafe { entry(arg as *mut c_void) } as usize;
        crate::r::native_tls::run_execution_destructors(crate::r::native_tls::current_identity());
        let _ = completion.complete(result);
        crate::r::platform::trueos_tokio_platform_wake_all(wait_key(id));
    }));
    let admitted = job.and_then(|job| crate::r::threads::spawn(stack, job, crate::r::native_process::current_vm(), id));
    if let Err(error) = admitted {
        crate::r::native_process::current().threads.handles.lock().remove(&id);
        return Err(error);
    }
    Ok(id)
}

pub(crate) fn join(thread: usize) -> Result<usize, i32> {
    if crate::r::threads::current_id() == Some(thread) { return Err(35); }
    let state = &crate::r::native_process::current().threads;
    let completion = state.claim(thread)?;
    let result = loop {
        let observed = crate::r::platform::trueos_tokio_platform_wait_observe(wait_key(thread));
        if let Some(result) = completion.try_take() { break result; }
        crate::r::platform::trueos_tokio_platform_wait_after(wait_key(thread), observed, u64::MAX);
    };
    state.handles.lock().remove(&thread);
    Ok(result)
}

pub(crate) fn detach(thread: usize) -> Result<(), i32> {
    let state = &crate::r::native_process::current().threads;
    let mut handles = state.handles.lock();
    if handles.get(&thread).is_some_and(|handle| handle.joining) { return Err(22); }
    // The admitted job keeps its own Arc. Removing the join resource never
    // cancels execution or forces guest callbacks during process retirement.
    handles.remove(&thread).map(|_| ()).ok_or(3)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opaque_handle_sequence_exhaustion_never_reuses_an_identity() {
        let state = State::new();
        state.sequence.store(SEQUENCE_MASK - 1, Ordering::Relaxed);
        let (last, _) = state.register().unwrap();
        assert!(last <= u32::MAX as usize);
        assert!(matches!(state.register(), Err(11)));
        assert!(state.contains(last));
        assert_eq!(state.live_count(), 1);
    }
}
