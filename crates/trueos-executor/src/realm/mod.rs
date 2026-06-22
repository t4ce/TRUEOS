//! Nested executor lanes for TRUEOS runtime scheduling.
//!
//! A realm is a raw executor with its own wake flag. Parent code decides how much
//! queued work to poll from the realm each turn.

use core::sync::atomic::{AtomicBool, Ordering};

use crate::raw::{Executor, PollStats};
use crate::{SendSpawner, Spawner};

const REALM_CONTEXT_TAG: usize = 1;
const REALM_CONTEXT_MASK: usize = !REALM_CONTEXT_TAG;

/// Wake flag used as the pender context for a nested executor realm.
#[repr(C, align(2))]
pub struct WakeFlag {
    pending: AtomicBool,
}

impl WakeFlag {
    /// Create a clear wake flag.
    pub const fn new() -> Self {
        Self {
            pending: AtomicBool::new(false),
        }
    }

    /// Mark the realm as needing service.
    pub fn wake(&self) {
        self.pending.store(true, Ordering::Release);
    }

    /// Clear the pending bit.
    pub fn clear(&self) {
        self.pending.store(false, Ordering::Release);
    }

    /// Return true if the realm has requested service.
    pub fn is_pending(&self) -> bool {
        self.pending.load(Ordering::Acquire)
    }

    /// Atomically consume the pending bit.
    pub fn take(&self) -> bool {
        self.pending.swap(false, Ordering::AcqRel)
    }

    /// Return the tagged raw executor pender context for this flag.
    pub fn context(&'static self) -> *mut () {
        ((self as *const Self as usize) | REALM_CONTEXT_TAG) as *mut ()
    }
}

impl Default for WakeFlag {
    fn default() -> Self {
        Self::new()
    }
}

/// Handle a raw executor pender context if it belongs to a realm.
///
/// Returns false for normal TRUEOS CPU-slot contexts.
pub(crate) fn try_pend_context(context: *mut ()) -> bool {
    let raw = context as usize;
    if raw & REALM_CONTEXT_TAG == 0 {
        return false;
    }

    let ptr = (raw & REALM_CONTEXT_MASK) as *const WakeFlag;
    if ptr.is_null() {
        return false;
    }

    unsafe { &*ptr }.wake();
    true
}

/// A nested executor lane with an externally visible wake flag.
///
/// Construct the realm, place it in stable storage, then use [`Realm::spawner`]
/// to seed tasks and [`Realm::poll_budget`] to service bounded work.
pub struct Realm {
    wake: &'static WakeFlag,
    executor: Executor,
}

impl Realm {
    /// Create a realm backed by `wake`.
    pub fn new(wake: &'static WakeFlag) -> Self {
        Self {
            wake,
            executor: Executor::new(wake.context()),
        }
    }

    /// Return this realm's wake flag.
    pub const fn wake_flag(&self) -> &'static WakeFlag {
        self.wake
    }

    /// Return a spawner for tasks that should run inside this realm.
    pub fn spawner(&'static self) -> Spawner {
        self.executor.spawner()
    }

    /// Return a sendable spawner for tasks that should run inside this realm.
    pub fn send_spawner(&'static self) -> SendSpawner {
        self.spawner().make_send()
    }

    /// Poll up to `max_tasks` queued tasks from this realm.
    ///
    /// # Safety
    ///
    /// The realm executor must not be polled reentrantly, and this must be called
    /// from the runtime lane that owns the realm.
    pub unsafe fn poll_budget(&'static self, max_tasks: usize) -> PollStats {
        self.wake.clear();
        let stats = self.executor.poll_budget(max_tasks);
        if stats.has_ready_tasks() {
            self.wake.wake();
        }
        stats
    }

    /// Return the number of spawned tasks still attached to this realm.
    pub fn spawned_task_count(&'static self) -> usize {
        self.executor.spawned_task_count()
    }

    /// Return the number of tasks queued to be polled in this realm.
    pub fn ready_task_count(&'static self) -> usize {
        self.executor.ready_task_count()
    }
}
