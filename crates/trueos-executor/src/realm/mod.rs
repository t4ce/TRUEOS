//! Nested executor lanes for TRUEOS runtime scheduling.
//!
//! A realm is a raw executor with its own wake flag. Parent code decides how much
//! queued work to poll from the realm each turn.

use core::sync::atomic::{AtomicBool, Ordering};

use crate::raw::{Executor, PollStats};
use crate::{SendSpawner, Spawner};

const REALM_CONTEXT_TAG: usize = 1;
const REALM_CONTEXT_MASK: usize = !REALM_CONTEXT_TAG;
const MIN_TAGGED_REALM_CONTEXT: usize = 4096;

/// Scheduling policy for a realm.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Policy {
    /// Maximum delay, in embassy-time ticks, allowed for timer wake coalescing.
    pub timer_slack_ticks: u64,
    /// Maximum queued tasks this realm may poll per bounded pass.
    ///
    /// A value of zero means no realm-local poll limit.
    pub poll_limit_tasks: usize,
}

impl Policy {
    /// Policy with exact timer wakes.
    pub const fn exact() -> Self {
        Self {
            timer_slack_ticks: 0,
            poll_limit_tasks: 0,
        }
    }

    /// Policy with timer wake slack.
    pub const fn with_timer_slack_ticks(timer_slack_ticks: u64) -> Self {
        Self {
            timer_slack_ticks,
            poll_limit_tasks: 0,
        }
    }

    /// Policy with a per-pass task poll limit.
    pub const fn with_poll_limit_tasks(poll_limit_tasks: usize) -> Self {
        Self {
            timer_slack_ticks: 0,
            poll_limit_tasks,
        }
    }

    /// Return this policy with timer wake slack set.
    pub const fn and_timer_slack_ticks(mut self, timer_slack_ticks: u64) -> Self {
        self.timer_slack_ticks = timer_slack_ticks;
        self
    }

    /// Return this policy with a per-pass task poll limit set.
    pub const fn and_poll_limit_tasks(mut self, poll_limit_tasks: usize) -> Self {
        self.poll_limit_tasks = poll_limit_tasks;
        self
    }
}

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
    if raw < MIN_TAGGED_REALM_CONTEXT {
        return false;
    }

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
        Self::new_with_policy(wake, Policy::exact())
    }

    /// Create a realm backed by `wake` and `policy`.
    pub fn new_with_policy(wake: &'static WakeFlag, policy: Policy) -> Self {
        let executor = Executor::new(wake.context());
        executor.set_timer_slack_ticks(policy.timer_slack_ticks);
        executor.set_poll_limit_tasks(policy.poll_limit_tasks);
        Self {
            wake,
            executor,
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

    /// Set this realm's timer slack in embassy-time ticks.
    pub fn set_timer_slack_ticks(&self, ticks: u64) {
        self.executor.set_timer_slack_ticks(ticks);
    }

    /// Return this realm's timer slack in embassy-time ticks.
    pub fn timer_slack_ticks(&self) -> u64 {
        self.executor.timer_slack_ticks()
    }

    /// Set this realm's per-pass task poll limit.
    ///
    /// A value of zero means no realm-local limit.
    pub fn set_poll_limit_tasks(&self, tasks: usize) {
        self.executor.set_poll_limit_tasks(tasks);
    }

    /// Return this realm's per-pass task poll limit.
    ///
    /// A value of zero means no realm-local limit.
    pub fn poll_limit_tasks(&self) -> usize {
        self.executor.poll_limit_tasks()
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
