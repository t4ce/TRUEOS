//! Standard-library threads on shared TRUEOS executors.
//!
//! Each admitted thread owns a pinned stack and a stable identity. Synchronous
//! waits suspend that stack and return to the carrier's ordinary executor;
//! persistent Tokio workers therefore do not consume exclusive service lanes.
//! CPU-bound code remains cooperative, like other TRUEOS executor work.

use alloc::boxed::Box;
use core::future::Future;
use core::mem::ManuallyDrop;
use core::pin::Pin;
use core::sync::atomic::{AtomicI32, AtomicPtr, AtomicUsize, Ordering};
use core::task::{Context, Poll, Waker};

mod context;
mod stack;
mod timing;
mod exit_trace;
mod placement;
#[cfg(test)]
mod tests;
use crate::r::blocking::{BlockingJobFn, GuestJobOwner};
use crate::r::blocking::diagnostics;
use crate::r::kernel_task_domain::{self, KernelTaskDomain};
use crate::workers::application_policy::{PlacementMode, ThreadPlacement, ThreadRole};

const THREAD_LIMIT: usize = 256;
const MAX_STACK_BYTES: usize = 64 * 1024 * 1024;
static THREAD_COUNT: AtomicUsize = AtomicUsize::new(0);
static NEXT_CARRIER: AtomicUsize = AtomicUsize::new(0);
static ADMISSION_FAILURE_COUNT: AtomicUsize = AtomicUsize::new(0);

fn admission_rejected(vm_id: Option<u8>, id: usize, stack: usize, reason: &'static str) -> i32 {
    // EAGAIN has several independent causes. Preserve the resource boundary
    // without exposing a thread name, closure, argument, or guest stack data.
    let sequence = ADMISSION_FAILURE_COUNT.fetch_add(1, Ordering::Relaxed);
    if sequence < 32 || sequence % 64 == 0 {
        crate::log_info!(target: "blueprint";
            "thread-admission vm={} thread={} reason={} rc=11 live={} limit={} stack_bytes={} failure_seq={}\n",
            vm_id.map_or(usize::MAX, usize::from), id, reason,
            THREAD_COUNT.load(Ordering::Acquire), THREAD_LIMIT, stack, sequence);
    }
    11
}
// Control wakes even a Condvar/sleep task whose ordinary wait never completes.
// Keep registrations until task retirement so pause and resume can both wake.
static CONTROL_WAKERS: [spin::Mutex<alloc::vec::Vec<Waker>>; crate::allcaps::hv::VM_ID_LIMIT] =
    [const { spin::Mutex::new(alloc::vec::Vec::new()) }; crate::allcaps::hv::VM_ID_LIMIT];

pub(crate) fn wake_guest_control(vm_id: u8) {
    if let Some(wakers) = CONTROL_WAKERS.get(vm_id as usize) {
        let pending = crate::allocators::with_host_alloc_domain_strong(|| wakers.lock().clone());
        for waker in pending {
            waker.wake();
        }
    }
}

static CURRENT: [AtomicPtr<Thread>; crate::allcaps::hv::VM_CPU_SLOT_LIMIT] =
    [const { AtomicPtr::new(core::ptr::null_mut()) }; crate::allcaps::hv::VM_CPU_SLOT_LIMIT];

enum Job {
    Owned(BlockingJobFn),
    GuestRaw { data: usize, vtable: usize },
}

struct Admission {
    _owner: Option<GuestJobOwner>,
}

impl Drop for Admission {
    fn drop(&mut self) {
        THREAD_COUNT.fetch_sub(1, Ordering::AcqRel);
    }
}

struct Thread {
    stack: context::Stack,
    #[cfg(not(target_os = "linux"))]
    address_space: Option<crate::hv::memory::CarrierAddressSpace>,
    child: usize,
    parent: usize,
    id: usize,
    errno: AtomicI32,
    carrier: u32,
    workload_role: ThreadRole,
    child_role_hint: ThreadRole,
    activity: u32,
    vm_id: Option<u8>,
    job: Option<Job>,
    done: bool,
    terminated: bool,
    abandoned: bool,
    kill_waker: Option<Waker>,
    wait: Option<Pin<Box<dyn Future<Output = ()> + Send>>>,
    timed: bool,
    submitted_ms: u64,
    timing: Option<timing::Timing>,
    exit_trace: exit_trace::State,
    domain: (u32, u8),
    allocation: [u32; 4],
    wls: crate::wls::ThreadContext,
    _wls_lease: Option<crate::wls::BlueprintThreadIdentityLease>,
    _admission: Admission,
}

// The task is admitted to one executor and never moves after its first poll.
unsafe impl Send for Thread {}

fn active() -> *mut Thread {
    // The Hull uses a private copy of kernel globals. Its waits use VMCALL;
    // only host-carried continuations can consult this scheduler state.
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return core::ptr::null_mut();
    }
    CURRENT
        .get(crate::percpu::current_slot())
        .map_or(core::ptr::null_mut(), |slot| slot.load(Ordering::Relaxed))
}

pub(crate) fn current_id() -> Option<usize> {
    let ptr = active();
    (!ptr.is_null()).then(|| unsafe { (*ptr).id })
}

pub(crate) fn current_vm_id() -> Option<u8> {
    let ptr = active();
    if ptr.is_null() {
        None
    } else {
        unsafe { (*ptr).vm_id }
    }
}

/// The hint belongs to the spawning continuation, never its AP. It is not
/// inherited by a child, and changing activity never changes placement.
fn swap_native_scope(activity: bool, value: u32) -> Option<u32> {
    if (activity && value > 3) || (!activity && ThreadRole::from_raw(value).is_none()) {
        return None;
    }
    let ptr = active();
    if ptr.is_null() { return None; }
    let thread = unsafe { &mut *ptr };
    if activity {
        let old = core::mem::replace(&mut thread.activity, value);
        if let Some(owner) = &thread._admission._owner { owner.diagnostic.activity(value); }
        Some(old)
    } else {
        Some(core::mem::replace(&mut thread.child_role_hint, ThreadRole::from_raw(value)?) as u32)
    }
}

/// Leave a terminal guest ABI call on a separately owned continuation stack.
/// Returns false when there is no active VM-owned continuation. A successful
/// transfer never returns to the abandoned guest frames: the carrier restores
/// its context before publishing the failure to the VM lifecycle.
///
/// # Safety
/// The caller must be at a terminal guest boundary with no host lock, borrowed
/// host resource, or host RAII cleanup on the abandoned frames. This is not an
/// escape from an arbitrary kernel panic or interrupt handler.
#[inline(never)]
pub(crate) unsafe fn terminate_current_guest() -> bool {
    let ptr = active();
    if ptr.is_null() || unsafe { (*ptr).vm_id.is_none() } {
        return false;
    }
    unsafe {
        (*ptr).terminated = true;
        (*ptr).abandoned = true;
        (*ptr).done = true;
        context::swap(&mut (*ptr).child, &(*ptr).parent);
    }
    unreachable!("terminated guest continuation resumed")
}

pub(crate) fn current_errno() -> Option<&'static AtomicI32> {
    let ptr = active();
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { &(*ptr).errno })
    }
}

/// Static operation breadcrumb for timed Codex native continuations only.
/// No host record is available for Hull-only code; absence is not an ABI fault.
/// # Safety
/// The guard must stay on this continuation stack and be dropped before returning
/// from it. It must never escape into a caller/registry that outlives this Thread.
pub(crate) unsafe fn abi_scope(operation: diagnostics::AbiOperation) -> diagnostics::AbiGuard {
    if !crate::allcaps::hv::CODEX_PROGRESS_PROBES_ENABLED { return diagnostics::AbiGuard::inactive(); }
    if !crate::allcaps::hv::CODEX_ALLOC_ABI_PROBES_ENABLED
        && matches!(operation, diagnostics::AbiOperation::Alloc | diagnostics::AbiOperation::Dealloc)
    {
        return diagnostics::AbiGuard::inactive();
    }
    let ptr = active();
    if ptr.is_null() { return diagnostics::AbiGuard::inactive(); }
    let thread = unsafe { &*ptr };
    if !thread.timed { return diagnostics::AbiGuard::inactive(); }
    match &thread._admission._owner {
        Some(owner) => unsafe { owner.diagnostic.abi_scope(operation) },
        None => diagnostics::AbiGuard::inactive(),
    }
}

pub(crate) fn set_current_name(name: &str) {
    let ptr = active();
    if !ptr.is_null() {
        if let Some(owner) = unsafe { &(*ptr)._admission._owner } {
            owner.diagnostic.name(name);
        }
    }
}

fn diagnostic_phase(thread: &Thread, phase: usize, wait: usize, timeout: u64) {
    if let Some(owner) = &thread._admission._owner {
        owner.diagnostic.phase(phase, wait, timeout);
    }
}

fn progress_boundary(thread: &Thread, stage: usize) {
    if let Some(owner) = &thread._admission._owner { owner.diagnostic.progress(stage); }
}

/// Suspend with an owned wait future. The future is polled on the carrier
/// stack after restoring its realm, so no other task observes this thread's
/// TLS or allocation-domain guards while the thread is parked.
#[inline(never)]
unsafe fn capture_slice_exit(ptr: *mut Thread, reason: exit_trace::Reason) {
    if !unsafe { (*ptr).timed } || !crate::allcaps::hv::CODEX_PROGRESS_PROBES_ENABLED { return; }
    let now = crate::r::input_timing::now_ms();
    let (rsp, rbp): (usize, usize);
    unsafe {
        core::arch::asm!("mov {}, rsp", "mov {}, rbp", out(reg) rsp, out(reg) rbp, options(nomem, nostack, preserves_flags));
        let low = (*ptr).stack.ptr().as_ptr() as usize;
        // This Stack owns this exact mapped region; neither guard is included.
        let high = low.checked_add((*ptr).stack.len()).unwrap_or(low);
        (*ptr).exit_trace.capture(now, reason, low, high, rsp, rbp);
    }
}

fn suspend(wait: Option<Pin<Box<dyn Future<Output = ()> + Send>>>, reason: exit_trace::Reason) -> bool {
    let ptr = active();
    if ptr.is_null() {
        return false;
    }
    unsafe {
        if wait.is_none() {
            diagnostic_phase(&*ptr, diagnostics::YIELDED, 0, 0);
        }
        (*ptr).wait = wait;
        capture_slice_exit(ptr, reason);
        context::swap(&mut (*ptr).child, &(*ptr).parent);
    }
    true
}

pub(crate) fn yield_now() -> bool {
    let _abi = unsafe { abi_scope(diagnostics::AbiOperation::Yield) };
    suspend(None, exit_trace::Reason::Yield)
}

pub(crate) fn sleep(ms: u64) -> bool {
    let _abi = unsafe { abi_scope(diagnostics::AbiOperation::Sleep) };
    if active().is_null() {
        return false;
    }
    if ms == 0 {
        return yield_now();
    }
    unsafe { diagnostic_phase(&*active(), diagnostics::SLEEPING, 0, ms); }
    unsafe { if let Some(timing) = (*active()).timing.as_mut() { timing.begin_wait(Some(ms)); } }
    let timer = crate::allocators::with_host_alloc_domain_strong(|| {
        Box::pin(async move {
            // Keep even very large std durations within the timer driver's
            // representable interval, preserving every requested millisecond.
            let mut remaining = ms;
            while remaining != 0 {
                let chunk = remaining.min(10_000);
                trueos_time::Timer::after_millis(chunk).await;
                remaining -= chunk;
            }
        })
    });
    suspend(Some(timer), exit_trace::Reason::Sleep)
}

pub(crate) fn wait(queue: &crate::wait::WaitQueue, observed: u32, timeout_ms: u64) -> Option<bool> {
    let _abi = unsafe { abi_scope(diagnostics::AbiOperation::Wait) };
    if active().is_null() {
        return None;
    }
    let queue_ptr = queue as *const crate::wait::WaitQueue as usize;
    unsafe { diagnostic_phase(&*active(), diagnostics::WAITING, queue_ptr, timeout_ms); }
    unsafe { if let Some(timing) = (*active()).timing.as_mut() { timing.begin_wait((timeout_ms != 0).then_some(timeout_ms)); } }
    let result = crate::allocators::with_host_alloc_domain_strong(|| {
        alloc::sync::Arc::new(core::sync::atomic::AtomicBool::new(false))
    });
    let out = result.clone();
    // The suspended caller keeps its queue reference and owner alive until
    // this future completes, or the host discards the suspended stack on kill.
    let future = crate::allocators::with_host_alloc_domain_strong(|| {
        Box::pin(async move {
            let queue = unsafe { &*(queue_ptr as *const crate::wait::WaitQueue) };
            let woke = if timeout_ms == 0 {
                queue.wait_after(observed).await;
                true
            } else {
                queue.wait_after_timeout(observed, timeout_ms).await
            };
            out.store(woke, Ordering::Release);
        })
    });
    suspend(Some(future), exit_trace::Reason::Wait);
    Some(result.load(Ordering::Acquire))
}

unsafe extern "C" fn run(data: *mut ()) -> ! {
    let ptr = data.cast::<Thread>();
    let job = unsafe { (*ptr).job.take().expect("thread entry runs once") };
    match job {
        Job::Owned(job) => job(),
        Job::GuestRaw { data, vtable } => unsafe {
            let raw: *mut (dyn FnOnce() + Send + 'static) = core::mem::transmute((data, vtable));
            Box::from_raw(raw)();
        },
    }
    unsafe {
        (*ptr).done = true;
        capture_slice_exit(ptr, exit_trace::Reason::Complete);
        context::swap(&mut (*ptr).child, &(*ptr).parent);
    }
    unreachable!("completed thread resumed")
}

struct ThreadTask(ManuallyDrop<Box<Thread>>);

impl ThreadTask {
    fn new(thread: Box<Thread>) -> Self {
        Self(ManuallyDrop::new(thread))
    }
}

fn guest_control_boundary(thread: &mut Thread) -> Option<Poll<()>> {
    let vm_id = thread.vm_id?;
    // Never poll the ordinary wait or re-enter guest code/destructors after
    // kill. Peers may hold scoped references into this suspended stack. Keep
    // every abandoned stack until the Hull has exited and every remaining
    // native owner has reached its own host boundary.
    if crate::hv::guest_kill_requested(vm_id) {
        diagnostic_phase(thread, diagnostics::CANCELLING, 0, 0);
        thread.abandoned = true;
        if let Some(job) = thread.job.take() {
            core::mem::forget(job);
        }
        thread.done = true;
        // A host wait can borrow a queue on a peer's suspended stack. Finish
        // unregistering before acknowledging retirement: the last such
        // acknowledgment lets other carriers reclaim their stack mappings.
        thread.wait = None;
        if let Some(owner) = thread._admission._owner.as_mut() {
            owner.retire();
            if !owner.reclaimable() {
                return Some(Poll::Pending);
            }
        }
        return Some(Poll::Ready(()));
    }
    if crate::hv::guest_native_pause_requested(vm_id) {
        let parked = thread._admission._owner.as_mut().is_none_or(GuestJobOwner::park);
        if parked {
            return Some(Poll::Pending);
        }
        // Resume or kill may have closed the pause while its flag was read.
        // A failed park cannot leave the continuation stranded.
    }
    if let Some(owner) = thread._admission._owner.as_mut() {
        owner.unpark();
    }
    None
}

impl Future for ThreadTask {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let thread = &mut **self.get_mut().0;
        progress_boundary(thread, diagnostics::POLL_BEGIN);
        if thread.timed && thread.timing.is_none() {
            let probe = thread._admission._owner.as_ref().map(|owner| {
                owner.diagnostic.role(thread.workload_role as u32);
                owner.diagnostic.activity(thread.activity);
                owner.diagnostic.timing_probe()
            });
            thread.timing = Some(timing::Timing::new(cx, thread.submitted_ms, probe));
            if crate::allcaps::hv::CODEX_PROGRESS_PROBES_ENABLED { thread.exit_trace.configure(thread.vm_id); }
        }
        let timed_waker = thread.timing.as_ref().map(|timing| {
            timing.begin_poll(cx);
            timing.waker()
        });
        let mut timed_cx = Context::from_waker(timed_waker.as_ref().unwrap_or(cx.waker()));
        let cx = &mut timed_cx;
        if let Some(vm_id) = thread.vm_id {
            if thread.kill_waker.as_ref().is_none_or(|waker| !waker.will_wake(cx.waker())) {
                if let Some(wakers) = CONTROL_WAKERS.get(vm_id as usize) {
                    let mut wakers = wakers.lock();
                    if let Some(old) = thread.kill_waker.take() {
                        wakers.retain(|registered| !registered.will_wake(&old));
                    }
                    crate::wait::register_waker_list(&mut wakers, cx.waker());
                    thread.kill_waker = Some(cx.waker().clone());
                }
            }
        }
        // Registration precedes the flag read: racing control either wakes
        // this task or is consumed at this host boundary.
        progress_boundary(thread, diagnostics::CONTROL_CHECK);
        if let Some(result) = guest_control_boundary(thread) {
            progress_boundary(thread, diagnostics::POLL_RETURN);
            return result;
        }
        if thread.wait.is_some() {
            progress_boundary(thread, diagnostics::WAIT_POLL);
            let wait = thread.wait.as_mut().unwrap();
            if wait.as_mut().poll(cx).is_pending() {
                progress_boundary(thread, diagnostics::WAIT_PENDING);
                return Poll::Pending;
            }
            thread.wait = None;
        }
        progress_boundary(thread, diagnostics::CONTROL_CHECK);
        if let Some(result) = guest_control_boundary(thread) {
            progress_boundary(thread, diagnostics::POLL_RETURN);
            return result;
        }
        let ptr = thread as *mut Thread;
        if let Some(timing) = thread.timing.as_mut() { timing.resumed(); }
        diagnostic_phase(thread, diagnostics::RUNNING, 0, 0);
        assert_eq!(
            crate::percpu::current_slot() as u32,
            thread.carrier,
            "thread continuation migrated"
        );
        if thread.child == 0 {
            thread.child = unsafe { thread.stack.initialize(ptr.cast(), run) };
        }
        let slot = &CURRENT[crate::percpu::current_slot()];
        assert!(slot.load(Ordering::Relaxed).is_null(), "recursive thread carrier poll");
        progress_boundary(thread, diagnostics::CONTEXT_INSTALL);
        let parent_domain = kernel_task_domain::replace_context(thread.domain);
        let parent_alloc = crate::allocators::replace_thread_context(thread.allocation);
        let parent_wls = crate::wls::replace_thread_context(thread.wls);
        slot.store(ptr, Ordering::Relaxed);
        #[cfg(not(target_os = "linux"))]
        let parent_cr3 = thread
            .address_space
            .as_ref()
            .map(|space| unsafe { context::replace_address_space(space.cr3()) });
        let run_started = crate::r::input_timing::now_ms();
        if thread.timed && crate::allcaps::hv::CODEX_PROGRESS_PROBES_ENABLED { thread.exit_trace.begin(run_started); }
        progress_boundary(thread, diagnostics::GUEST_ENTER);
        unsafe { context::swap(&mut (*ptr).parent, &(*ptr).child) };
        progress_boundary(thread, diagnostics::GUEST_RETURN);
        progress_boundary(thread, diagnostics::CONTEXT_RESTORE);
        #[cfg(not(target_os = "linux"))]
        if let Some(parent_cr3) = parent_cr3 {
            unsafe { context::replace_address_space(parent_cr3) };
        }
        slot.store(core::ptr::null_mut(), Ordering::Relaxed);
        thread.wls = crate::wls::replace_thread_context(parent_wls);
        thread.allocation = crate::allocators::replace_thread_context(parent_alloc);
        thread.domain = kernel_task_domain::replace_context(parent_domain);
        progress_boundary(thread, diagnostics::CONTEXT_RESTORED);
        if core::mem::replace(&mut thread.terminated, false) {
            let vm_id = thread.vm_id.expect("terminated thread is guest-owned");
            let generation = thread._admission._owner.as_ref()
                .map(GuestJobOwner::generation)
                .or_else(|| crate::hv::vm_run_generation(vm_id)).unwrap_or(0);
            crate::hv::native_guest_terminated(vm_id, generation);
        }
        // Snapshot memory belongs to Thread. Guest TLS, WLS, allocation domain
        // and address space are restored before any formatting/logging occurs.
        if let Some(trace) = thread.exit_trace.take(crate::r::input_timing::now_ms()) {
            crate::log_info!(target: "blueprint";
                "codex-slice-exit vm={} id={} carrier={} ended_ms={} duration_ms={} reason={:?} kind=exit-callstack cpu_attribution=none frames={:X?} stop={:?}\n",
                thread.vm_id.unwrap_or(u8::MAX), thread.id, thread.carrier,
                trace.ended_ms, trace.duration_ms, trace.reason,
                &trace.frames.addresses[..trace.frames.count], trace.frames.stop);
        }
        if let Some(timing) = thread.timing.as_ref() { timing.ran(run_started); }
        if thread.abandoned || !thread.done {
            if let Some(result) = guest_control_boundary(thread) {
                progress_boundary(thread, diagnostics::POLL_RETURN);
                return result;
            }
        }
        if thread.done {
            diagnostic_phase(thread, diagnostics::COMPLETED, 0, 0);
            progress_boundary(thread, diagnostics::POLL_RETURN);
            Poll::Ready(())
        } else {
            if thread.wait.is_none() {
                cx.waker().wake_by_ref();
            } else {
                // Register immediately; a notification before this poll is
                // retained by the wait generation, so no wake can be lost.
                progress_boundary(thread, diagnostics::WAIT_POLL);
                if thread.wait.as_mut().unwrap().as_mut().poll(cx).is_ready() {
                    thread.wait = None;
                    cx.waker().wake_by_ref();
                }
            }
            progress_boundary(thread, diagnostics::POLL_RETURN);
            Poll::Pending
        }
    }
}

impl Drop for ThreadTask {
    fn drop(&mut self) {
        let thread = &mut **self.0;
        if let (Some(vm_id), Some(waker)) = (thread.vm_id, thread.kill_waker.take()) {
            if let Some(wakers) = CONTROL_WAKERS.get(vm_id as usize) {
                wakers.lock().retain(|registered| !registered.will_wake(&waker));
            }
        }
        if thread.abandoned && thread._admission._owner.as_ref()
            .is_some_and(|owner| !owner.reclaimable())
        {
            // The private executor task normally stays pending until the
            // barrier opens. If an outer owner nevertheless drops it early,
            // retain the entire reservation and stack rather than invalidate
            // a peer's live reference. Normal retirement runs on its carrier.
            crate::log_warn!(target: "blueprint";
                "native-thread: premature retirement vm={:?} thread={} resources=retained\n",
                thread.vm_id, thread.id);
            return;
        }
        // Stack/address-space storage drops before admission. Guest frames on
        // an abandoned stack are never unwound or re-entered.
        unsafe { ManuallyDrop::drop(&mut self.0) };
    }
}

#[trueos_executor::task(pool_size = THREAD_LIMIT)]
async fn thread_task(thread: ThreadTask) {
    thread.await
}

/// Separate executor task entry, with no std-thread carrier selection.
#[trueos_executor::task(pool_size = THREAD_LIMIT)]
async fn codex_application_task(thread: ThreadTask) {
    thread.await
}

fn submit(stack: usize, job: Job, vm_id: Option<u8>, id: usize, role: ThreadRole) -> Result<(), i32> {
    let submitted_ms = crate::r::input_timing::now_ms();
    if stack > MAX_STACK_BYTES {
        return Err(22);
    }
    let is_codex = vm_id.and_then(crate::hv::app_vm_archive)
        .is_some_and(|archive| crate::workers::application_policy::is_codex_archive(&archive));
    let experimental = crate::allcaps::hv::CODEX_EXECUTOR_EXPERIMENT_ENABLED && is_codex;
    let spawner = if experimental {
        let vm = vm_id.expect("Codex admission is guest-owned");
        let generation = crate::hv::vm_run_generation(vm).ok_or(22)?;
        let plan = placement::admit(vm, generation)?;
        let selected = plan.policy.carrier(role, crate::workers::codex_application_slot(), plan.background(), plan.ordinal)
            .ok_or_else(|| admission_rejected(vm_id, id, stack, "codex_placement_unavailable"))?;
        // Select only registered, eligible carriers before the first poll.
        // Capacity loss is an explicit failure, never a silent policy fallback.
        crate::workers::spawner_for_slot(selected)
            .ok_or_else(|| admission_rejected(vm_id, id, stack, "codex_carrier_unavailable"))?
    } else {
        let slots = crate::workers::background_worker_slots()
            .into_iter()
            .filter(|slot| crate::workers::is_general_background_worker_slot(*slot))
            .filter(|slot| (*slot as usize) < crate::allcaps::wls::CPU_TRACK_COUNT)
            .collect::<alloc::vec::Vec<_>>();
        if slots.is_empty() {
            return Err(admission_rejected(vm_id, id, stack, "carrier_unavailable"));
        }
        let start = NEXT_CARRIER.fetch_add(1, Ordering::Relaxed);
        let slot = slots[start % slots.len()];
        crate::workers::spawner_for_slot(slot)
            .ok_or_else(|| admission_rejected(vm_id, id, stack, "carrier_unavailable"))?
    };
    let slot = spawner.cpu_slot();
    let owner = match vm_id {
        Some(vm) => Some(crate::r::blocking::reserve_guest_job(vm)
            .ok_or_else(|| admission_rejected(vm_id, id, stack, "guest_closed"))?),
        None => None,
    };
    if let Some(owner) = &owner {
        if is_codex { owner.diagnostic.enable_progress_probe(); }
        owner.diagnostic.identify(if experimental { "codex-executor-task" } else { "std-thread" }, id, slot as usize);
        owner.diagnostic.role(role as u32);
    }
    if THREAD_COUNT
        .try_update(Ordering::AcqRel, Ordering::Acquire, |n| (n < THREAD_LIMIT).then_some(n + 1))
        .is_err()
    {
        return Err(admission_rejected(vm_id, id, stack, "global_limit"));
    }
    let admission = Admission { _owner: owner };
    // Scheduler storage belongs to the host even when spawned from guest
    // code. Guest closure execution/destruction uses its retained VM realm.
    let prepared = crate::allocators::with_host_alloc_domain_strong(|| {
        let wls_lease = vm_id.map(|vm| crate::wls::try_lease_blueprint_thread_identity(vm, id)
            .ok_or_else(|| admission_rejected(vm_id, id, stack, "wls_identity"))).transpose()?;
        let requested_stack = stack;
        let stack = context::Stack::new_for_vm(stack, vm_id)
            .ok_or_else(|| admission_rejected(vm_id, id, stack, "stack"))?;
        #[cfg(not(target_os = "linux"))]
        let address_space = vm_id
            .map(|vm| unsafe { crate::hv::memory::CarrierAddressSpace::new(vm) })
            .transpose()
            .map_err(|_| admission_rejected(vm_id, id, requested_stack, "address_space"))?;
        let thread = Box::new(Thread {
            stack,
            #[cfg(not(target_os = "linux"))]
            address_space,
            child: 0,
            parent: 0,
            id,
            errno: AtomicI32::new(0),
            carrier: slot,
            workload_role: role,
            child_role_hint: ThreadRole::Unclassified,
            activity: if role == ThreadRole::Foreground { 1 } else { 0 },
            vm_id,
            job: None,
            done: false,
            terminated: false,
            abandoned: false,
            kill_waker: None,
            wait: None,
            timed: crate::allcaps::hv::INPUT_TIMING_ENABLED && is_codex,
            submitted_ms,
            timing: None,
            exit_trace: exit_trace::State::new(),
            domain: (
                if vm_id.is_some() {
                    KernelTaskDomain::VmGuestOwnedAlloc as u32
                } else {
                    KernelTaskDomain::HostService as u32
                },
                vm_id.unwrap_or(u8::MAX),
            ),
            allocation: [0; 4],
            wls: (0, id as u32, wls_lease.as_ref().map(crate::wls::BlueprintThreadIdentityLease::slot)),
            _wls_lease: wls_lease,
            _admission: admission,
        });
        Ok::<_, i32>(thread)
    });
    let mut thread = prepared?;
    thread.job = Some(job);
    if experimental {
        let task = codex_application_task(ThreadTask::new(thread))
            .map_err(|_| admission_rejected(vm_id, id, stack, "executor_pool"))?;
        crate::log_rate_limited!(target: "blueprint"; level: crate::log_os::LogLevel::Info; first: 2; every: 64;
            "codex-executor: vm={:?} thread={} carrier={} role={} owner=codex task=codex_application_task placement=admission\n",
            vm_id, id, slot, role as u32);
        spawner.spawn(task);
    } else {
        let task = thread_task(ThreadTask::new(thread))
            .map_err(|_| admission_rejected(vm_id, id, stack, "executor_pool"))?;
        spawner.spawn(task);
    }
    Ok(())
}

pub(crate) fn spawn(
    stack: usize,
    job: BlockingJobFn,
    vm_id: Option<u8>,
    id: usize,
) -> Result<(), i32> {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let raw = Box::into_raw(job);
        let (data, vtable): (usize, usize) = unsafe { core::mem::transmute(raw) };
        let mut payload = [0u8; 16];
        payload[..8].copy_from_slice(&(stack as u64).to_le_bytes());
        payload[8..].copy_from_slice(&(id as u64).to_le_bytes());
        let (status, result) = trueos_vm::vmcall::call_with_payload(
            crate::hv::vmcall::OP_BP_THREAD_SUBMIT,
            data as u64,
            vtable as u64,
            &payload,
            &mut [],
        );
        let rc = if status == crate::hv::vmcall::STATUS_OK {
            result as i32
        } else {
            11
        };
        if rc != 0 {
            unsafe { drop(Box::from_raw(raw)) };
            return Err(rc);
        }
        Ok(())
    } else {
        let ptr = active();
        let role = if ptr.is_null() { ThreadRole::Unclassified } else { unsafe { (*ptr).child_role_hint } };
        submit(stack, Job::Owned(job), vm_id, id, role)
    }
}

/// Called only by VMCALL dispatch. Ownership transfers after successful
/// admission; a rejected raw closure remains owned by the Hull caller.
pub(crate) unsafe fn submit_guest_from_raw(
    vm_id: u8,
    stack: usize,
    id: usize,
    data: usize,
    vtable: usize,
) -> i32 {
    if data == 0 || vtable == 0 || id == 0 {
        return 22;
    }
    let generation = match crate::hv::vm_run_generation(vm_id) { Some(value) => value, None => return 22 };
    let role = match placement::hull_role(vm_id, generation) { Ok(value) => value, Err(error) => return error };
    submit(stack, Job::GuestRaw { data, vtable }, Some(vm_id), id, role)
        .err()
        .unwrap_or(0)
}

/// Host-only control behind the authenticated Hull VMCALL. No caller chooses
/// an AP number or changes the placement of an already admitted continuation.
pub(crate) fn hull_thread_control(vm: u8, operation: u64, value: u64) -> Result<u32, i32> {
    let generation = crate::hv::vm_run_generation(vm).ok_or(22)?;
    match operation {
        0 => {
            let mode = value as u32;
            let cap = (value >> 32) as u32;
            let policy = ThreadPlacement::from_raw(mode, cap).ok_or(22)?;
            let is_codex = crate::hv::app_vm_archive(vm)
                .is_some_and(|archive| crate::workers::application_policy::is_codex_archive(&archive));
            if !is_codex || !crate::allcaps::hv::CODEX_EXECUTOR_EXPERIMENT_ENABLED { return Err(95); }
            let foreground = crate::workers::codex_application_slot()
                .filter(|slot| crate::workers::spawner_for_slot(*slot).is_some()).ok_or(11)?;
            // Freeze a topology/profile-selected set for this launch. Later AP
            // registrations cannot expand or silently change its experiment.
            let background = if policy.mode == PlacementMode::SingleCarrier {
                alloc::vec::Vec::new()
            } else {
                crate::workers::pick_background_spawners_with_slots(policy.background_cap)
                    .into_iter().map(|(slot, _, _)| slot).collect::<alloc::vec::Vec<_>>()
            };
            if policy.carrier(ThreadRole::Foreground, Some(foreground), &background, 0).is_none()
                || (policy.mode != PlacementMode::SingleCarrier && policy.carrier(ThreadRole::TokioPool, Some(foreground), &background, 0).is_none()) {
                return Err(11);
            }
            placement::configure(vm, generation, policy, &background)?;
            crate::log_info!(target: "blueprint";
                "thread-placement-config vm={} gen={} mode={} background_cap={} background={:?} foreground={} migration=none\n",
                vm, generation, mode, cap, background, foreground);
            Ok(0)
        },
        1 | 2 if value <= u32::MAX as u64 => placement::swap_hull_scope(vm, generation, operation == 2, value as u32),
        _ => Err(22),
    }
}

fn hull_control(operation: u64, value: u64) -> Option<u32> {
    let (status, result) = trueos_vm::vmcall::call(crate::hv::vmcall::OP_BP_THREAD_CONTROL_V1, operation, value);
    (status == crate::hv::vmcall::STATUS_OK && result <= u32::MAX as u64).then_some(result as u32)
}

pub(crate) fn configure_placement(mode: u32, background_cap: u32) -> i32 {
    if ThreadPlacement::from_raw(mode, background_cap).is_none() { return 22; }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        // Control failures are positive errno in the response, with STATUS_BAD_ARG.
        let (status, result) = trueos_vm::vmcall::call(crate::hv::vmcall::OP_BP_THREAD_CONTROL_V1, 0, (mode as u64) | ((background_cap as u64) << 32));
        return if status == crate::hv::vmcall::STATUS_OK { 0 } else { i32::try_from(result).ok().filter(|rc| *rc > 0).unwrap_or(22) };
    }
    // A native continuation already has immutable placement.
    if current_id().is_some() { return 16; }
    95
}

pub(crate) fn swap_scope(activity: bool, value: u32) -> u32 {
    if (activity && value > 3) || (!activity && ThreadRole::from_raw(value).is_none()) { return u32::MAX; }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        hull_control(if activity { 2 } else { 1 }, value as u64).unwrap_or(u32::MAX)
    } else {
        swap_native_scope(activity, value).unwrap_or(u32::MAX)
    }
}
