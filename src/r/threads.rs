//! Standard-library threads on shared TRUEOS executors.
//!
//! Each admitted thread owns a pinned stack and a stable identity. Synchronous
//! waits suspend that stack and return to the carrier's ordinary executor;
//! persistent Tokio workers therefore do not consume exclusive service lanes.
//! CPU-bound code remains cooperative, like other TRUEOS executor work.

use alloc::boxed::Box;
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicI32, AtomicPtr, AtomicUsize, Ordering};
use core::task::{Context, Poll};

mod context;
mod stack;
#[cfg(test)]
mod tests;
use crate::r::blocking::{BlockingJobFn, GuestJobOwner};
use crate::r::kernel_task_domain::{self, KernelTaskDomain};

const THREAD_LIMIT: usize = 256;
const MAX_STACK_BYTES: usize = 64 * 1024 * 1024;
static THREAD_COUNT: AtomicUsize = AtomicUsize::new(0);
static NEXT_CARRIER: AtomicUsize = AtomicUsize::new(0);
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
    child: usize,
    parent: usize,
    id: usize,
    errno: AtomicI32,
    carrier: u32,
    vm_id: Option<u8>,
    job: Option<Job>,
    done: bool,
    wait: Option<Pin<Box<dyn Future<Output = ()> + Send>>>,
    domain: (u32, u8),
    allocation: [u32; 4],
    wls: (u64, u32),
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

pub(crate) fn current_errno() -> Option<&'static AtomicI32> {
    let ptr = active();
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { &(*ptr).errno })
    }
}

/// Suspend with an owned wait future. The future is polled on the carrier
/// stack after restoring its realm, so no other task observes this thread's
/// TLS or allocation-domain guards while the thread is parked.
fn suspend(wait: Option<Pin<Box<dyn Future<Output = ()> + Send>>>) -> bool {
    let ptr = active();
    if ptr.is_null() {
        return false;
    }
    unsafe {
        (*ptr).wait = wait;
        context::swap(&mut (*ptr).child, &(*ptr).parent);
    }
    true
}

pub(crate) fn yield_now() -> bool {
    suspend(None)
}

pub(crate) fn sleep(ms: u64) -> bool {
    if active().is_null() {
        return false;
    }
    if ms == 0 {
        return yield_now();
    }
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
    suspend(Some(timer))
}

pub(crate) fn wait(queue: &crate::wait::WaitQueue, observed: u32, timeout_ms: u64) -> Option<bool> {
    if active().is_null() {
        return None;
    }
    let queue_ptr = queue as *const crate::wait::WaitQueue as usize;
    let result = alloc::sync::Arc::new(core::sync::atomic::AtomicBool::new(false));
    let out = result.clone();
    // The suspended caller keeps its queue reference and owner alive until
    // this future completes. Thread tasks cannot be cancelled mid-stack.
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
    suspend(Some(future));
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
        context::swap(&mut (*ptr).child, &(*ptr).parent);
    }
    unreachable!("completed thread resumed")
}

struct ThreadTask(Box<Thread>);

impl Future for ThreadTask {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let thread = &mut *self.get_mut().0;
        if let Some(wait) = thread.wait.as_mut() {
            if wait.as_mut().poll(cx).is_pending() {
                return Poll::Pending;
            }
            thread.wait = None;
        }
        let ptr = thread as *mut Thread;
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
        let parent_domain = kernel_task_domain::replace_context(thread.domain);
        let parent_alloc = crate::allocators::replace_thread_context(thread.allocation);
        let parent_wls = crate::wls::replace_thread_context(thread.wls);
        slot.store(ptr, Ordering::Relaxed);
        unsafe { context::swap(&mut (*ptr).parent, &(*ptr).child) };
        slot.store(core::ptr::null_mut(), Ordering::Relaxed);
        thread.wls = crate::wls::replace_thread_context(parent_wls);
        thread.allocation = crate::allocators::replace_thread_context(parent_alloc);
        thread.domain = kernel_task_domain::replace_context(parent_domain);
        if thread.done {
            Poll::Ready(())
        } else {
            if thread.wait.is_none() {
                cx.waker().wake_by_ref();
            } else {
                // Register immediately; a notification before this poll is
                // retained by the wait generation, so no wake can be lost.
                if thread.wait.as_mut().unwrap().as_mut().poll(cx).is_ready() {
                    thread.wait = None;
                    cx.waker().wake_by_ref();
                }
            }
            Poll::Pending
        }
    }
}

#[trueos_executor::task(pool_size = THREAD_LIMIT)]
async fn thread_task(thread: ThreadTask) {
    thread.await
}

fn submit(stack: usize, job: Job, vm_id: Option<u8>, id: usize) -> Result<(), i32> {
    if stack > MAX_STACK_BYTES {
        return Err(22);
    }
    let slots = crate::workers::background_worker_slots()
        .into_iter()
        .filter(|slot| crate::workers::is_general_background_worker_slot(*slot))
        .filter(|slot| (*slot as usize) < crate::allcaps::wls::CPU_TRACK_COUNT)
        .collect::<alloc::vec::Vec<_>>();
    if slots.is_empty() {
        return Err(11);
    }
    let start = NEXT_CARRIER.fetch_add(1, Ordering::Relaxed);
    let slot = slots[start % slots.len()];
    let spawner = crate::workers::spawner_for_slot(slot).ok_or(11)?;
    let owner = match vm_id {
        Some(vm) => Some(crate::r::blocking::reserve_guest_job(vm).ok_or(11)?),
        None => None,
    };
    if THREAD_COUNT
        .try_update(Ordering::AcqRel, Ordering::Acquire, |n| (n < THREAD_LIMIT).then_some(n + 1))
        .is_err()
    {
        return Err(11);
    }
    let admission = Admission { _owner: owner };
    // Scheduler storage belongs to the host even when spawned from guest
    // code. Guest closure execution/destruction uses its retained VM realm.
    let prepared = crate::allocators::with_host_alloc_domain_strong(|| {
        let stack = context::Stack::new(stack).ok_or(11)?;
        let thread = Box::new(Thread {
            stack,
            child: 0,
            parent: 0,
            id,
            errno: AtomicI32::new(0),
            carrier: slot,
            vm_id,
            job: None,
            done: false,
            wait: None,
            domain: (
                if vm_id.is_some() {
                    KernelTaskDomain::VmGuestOwnedAlloc as u32
                } else {
                    KernelTaskDomain::HostService as u32
                },
                vm_id.unwrap_or(u8::MAX),
            ),
            allocation: [0; 4],
            wls: (0, id as u32),
            _admission: admission,
        });
        Ok::<_, i32>(thread)
    });
    let mut thread = prepared?;
    thread.job = Some(job);
    match thread_task(ThreadTask(thread)) {
        Ok(task) => {
            spawner.spawn(task);
            Ok(())
        }
        Err(_) => Err(11),
    }
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
        submit(stack, Job::Owned(job), vm_id, id)
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
    submit(stack, Job::GuestRaw { data, vtable }, Some(vm_id), id)
        .err()
        .unwrap_or(0)
}
