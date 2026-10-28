extern crate std;

use super::*;
use alloc::sync::Arc;
use alloc::vec::Vec;
use std::sync::Mutex;

static SERIAL: Mutex<()> = Mutex::new(());

type Realm = ((u32, u8), [u32; 4], crate::wls::ThreadContext);

fn realm() -> Realm {
    let domain = kernel_task_domain::replace_context((0, u8::MAX));
    kernel_task_domain::replace_context(domain);
    let allocation = crate::allocators::replace_thread_context([0; 4]);
    crate::allocators::replace_thread_context(allocation);
    let wls = crate::wls::replace_thread_context((0, 0, None));
    crate::wls::replace_thread_context(wls);
    (domain, allocation, wls)
}

fn make_thread(id: usize, job: impl FnOnce() + Send + 'static) -> ThreadTask {
    THREAD_COUNT.fetch_add(1, Ordering::AcqRel);
    ThreadTask::new(Box::new(Thread {
        stack: context::Stack::new(256 * 1024).unwrap(),
        child: 0,
        parent: 0,
        id,
        errno: AtomicI32::new(0),
        carrier: crate::percpu::current_slot() as u32,
        workload_role: ThreadRole::Unclassified,
        child_role_hint: ThreadRole::Unclassified,
        activity: 0,
        vm_id: Some(id as u8),
        job: Some(Job::Owned(Box::new(job))),
        done: false,
        terminated: false,
        abandoned: false,
        kill_waker: None,
        wait: None,
        timed: false,
        submitted_ms: 0,
        timing: None,
        exit_trace: exit_trace::State::new(),
        domain: (77, id as u8),
        allocation: [0; 4],
        wls: (0, id as u32, None),
        _wls_lease: None,
        _admission: Admission { _owner: None },
    }))
}

fn poll_thread(thread: &mut ThreadTask) -> Poll<()> {
    let parent = realm();
    assert_eq!(current_id(), None);
    let result = Pin::new(thread).poll(&mut Context::from_waker(std::task::Waker::noop()));
    assert_eq!(current_id(), None);
    assert!(current_errno().is_none());
    assert_eq!(realm(), parent, "carrier realm leaked across a suspension");
    result
}

#[cfg(thread_scheduler_harness)]
fn kill_with_hull_stopped(vm_id: u8) {
    crate::hv::set_guest_kill_for_test(vm_id, true);
    crate::r::blocking::close_guest_jobs(vm_id);
    crate::r::blocking::acknowledge_guest_hull_stopped(vm_id, 19);
}

#[test]
#[cfg(thread_scheduler_harness)]
fn native_termination_restores_carrier_before_lifecycle_and_retains_stack_until_hull_exit() {
    let _serial = SERIAL.lock().unwrap();
    const VM: u8 = 58;
    crate::NATIVE_TERMINATIONS.lock().unwrap().clear();
    assert!(!unsafe { terminate_current_guest() });
    assert!(crate::r::blocking::open_guest_jobs(VM, 19));
    let drops = Arc::new(AtomicUsize::new(0));
    let progress = Arc::new(AtomicUsize::new(0));
    struct GuestDrop(Arc<AtomicUsize>);
    impl Drop for GuestDrop {
        fn drop(&mut self) { self.0.fetch_add(1, Ordering::AcqRel); }
    }
    let child_drops = drops.clone();
    let child_progress = progress.clone();
    let mut task = make_thread(VM as usize, move || {
        let _guard = GuestDrop(child_drops);
        kernel_task_domain::replace_context((123, VM));
        crate::allocators::replace_thread_context([9, 8, 7, 6]);
        crate::wls::replace_thread_context((101, 102, None));
        child_progress.store(1, Ordering::Release);
        unsafe { terminate_current_guest() };
        child_progress.store(2, Ordering::Release);
    });
    task.0._admission._owner = Some(crate::r::blocking::reserve_guest_job(VM).unwrap());
    let parent = realm();
    assert!(poll_thread(&mut task).is_pending());
    assert_eq!(progress.load(Ordering::Acquire), 1, "terminal ABI returned to guest");
    assert_eq!(drops.load(Ordering::Acquire), 0, "abandoned frames were unwound");
    assert_eq!(*crate::NATIVE_TERMINATIONS.lock().unwrap(),
        vec![(VM, 19, parent.0, parent.1, parent.2)]);
    assert_eq!(crate::r::blocking::guest_jobs_in_flight(VM), 1);
    assert!(!crate::r::blocking::guest_jobs_reclaimable(VM, 19));
    assert!(!crate::r::blocking::acknowledge_guest_hull_stopped(VM, 18), "stale Hull retired new execution");
    assert!(poll_thread(&mut task).is_pending());
    assert!(crate::r::blocking::acknowledge_guest_hull_stopped(VM, 19));
    assert!(poll_thread(&mut task).is_ready());
    drop(task);
    assert_eq!(crate::r::blocking::guest_jobs_in_flight(VM), 0);
    assert_eq!(drops.load(Ordering::Acquire), 0);
    crate::hv::set_guest_kill_for_test(VM, false);
}

#[test]
#[cfg(thread_scheduler_harness)]
fn killed_stack_remains_readable_until_its_native_borrower_stops() {
    let _serial = SERIAL.lock().unwrap();
    const VM: u8 = 59;
    assert!(crate::r::blocking::open_guest_jobs(VM, 19));
    let pointer = Arc::new(AtomicUsize::new(0));
    let child_pointer = pointer.clone();
    let mut parent = make_thread(VM as usize, move || {
        let borrowed = [0x51a7_0b1eusize; 64];
        child_pointer.store(borrowed.as_ptr() as usize, Ordering::Release);
        assert!(yield_now());
        // A scoped parent would join its borrower before returning normally.
        assert_eq!(borrowed[0], 0x51a7_0b1e);
        panic!("killed parent resumed");
    });
    parent.0._admission._owner = Some(crate::r::blocking::reserve_guest_job(VM).unwrap());
    // This real reservation models an executing service-lane borrower, whose
    // lifetime must protect the cancelled parent's stack until it stops.
    let borrower = crate::r::blocking::reserve_guest_job(VM).unwrap();
    assert!(poll_thread(&mut parent).is_pending());
    let address = pointer.load(Ordering::Acquire);
    assert_ne!(address, 0);
    kill_with_hull_stopped(VM);
    assert!(poll_thread(&mut parent).is_pending());
    assert_eq!(crate::r::blocking::guest_jobs_in_flight(VM), 2);
    assert!(!crate::r::blocking::open_guest_jobs(VM, 20));
    let reader = std::thread::spawn(move || {
        // The parent is already cancelled. Its guarded mapping must still be
        // live while this admitted peer can dereference the shared stack.
        let value = unsafe { (address as *const usize).read_volatile() };
        drop(borrower);
        value
    });
    assert_eq!(reader.join().unwrap(), 0x51a7_0b1e);
    assert!(poll_thread(&mut parent).is_ready());
    drop(parent);
    assert_eq!(crate::r::blocking::guest_jobs_in_flight(VM), 0);
    crate::hv::set_guest_kill_for_test(VM, false);
}

#[test]
#[cfg(thread_scheduler_harness)]
fn killed_native_peers_retire_without_waiting_for_each_others_admission_drop() {
    let _serial = SERIAL.lock().unwrap();
    const VM: u8 = 50;
    assert!(crate::r::blocking::open_guest_jobs(VM, 19));
    let mut first = make_thread(VM as usize, || {
        assert!(yield_now());
        panic!("killed first peer resumed");
    });
    let mut second = make_thread(VM as usize, || {
        assert!(yield_now());
        panic!("killed second peer resumed");
    });
    first.0._admission._owner = Some(crate::r::blocking::reserve_guest_job(VM).unwrap());
    second.0._admission._owner = Some(crate::r::blocking::reserve_guest_job(VM).unwrap());
    assert!(poll_thread(&mut first).is_pending());
    assert!(poll_thread(&mut second).is_pending());
    kill_with_hull_stopped(VM);
    assert!(poll_thread(&mut first).is_pending());
    assert!(poll_thread(&mut second).is_ready());
    drop(second);
    assert!(poll_thread(&mut first).is_ready());
    drop(first);
    assert_eq!(crate::r::blocking::guest_jobs_in_flight(VM), 0);
    crate::hv::set_guest_kill_for_test(VM, false);
}

#[test]
#[cfg(thread_scheduler_harness)]
fn borrowed_peer_stack_wait_unregisters_before_the_collective_reclaim_barrier() {
    let _serial = SERIAL.lock().unwrap();
    const VM: u8 = 49;
    assert!(crate::r::blocking::open_guest_jobs(VM, 19));
    let queue_address = Arc::new(AtomicUsize::new(0));
    let published = queue_address.clone();
    let mut parent = make_thread(VM as usize, move || {
        let queue = crate::wait::WaitQueue::new();
        published.store(&queue as *const crate::wait::WaitQueue as usize, Ordering::Release);
        assert!(yield_now());
        panic!("killed queue owner resumed");
    });
    let mut child = make_thread(VM as usize, move || {
        // Models a scoped child borrowing a wait object in its parent frame.
        let queue = unsafe { &*(queue_address.load(Ordering::Acquire) as *const crate::wait::WaitQueue) };
        wait(queue, queue.observe(), 0);
        panic!("killed queue borrower resumed");
    });
    parent.0._admission._owner = Some(crate::r::blocking::reserve_guest_job(VM).unwrap());
    child.0._admission._owner = Some(crate::r::blocking::reserve_guest_job(VM).unwrap());
    assert!(poll_thread(&mut parent).is_pending());
    assert!(poll_thread(&mut child).is_pending());
    struct CheckedWait {
        wait: Pin<Box<dyn Future<Output = ()> + Send>>,
        dropped: Arc<AtomicUsize>,
    }
    impl Future for CheckedWait {
        type Output = ();
        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
            self.wait.as_mut().poll(cx)
        }
    }
    impl Drop for CheckedWait {
        fn drop(&mut self) {
            assert!(!crate::r::blocking::guest_jobs_reclaimable(VM, 19),
                "peer could reclaim its stack before wait unregistration");
            self.dropped.fetch_add(1, Ordering::AcqRel);
        }
    }
    let cleaned = Arc::new(AtomicUsize::new(0));
    child.0.wait = Some(Box::pin(CheckedWait {
        wait: child.0.wait.take().unwrap(), dropped: cleaned.clone(),
    }));
    kill_with_hull_stopped(VM);
    assert!(poll_thread(&mut parent).is_pending());
    assert!(poll_thread(&mut child).is_ready());
    assert_eq!(cleaned.load(Ordering::Acquire), 1);
    assert!(child.0.wait.is_none());
    assert!(poll_thread(&mut parent).is_ready());
    // Reclaim the queue-owning stack before the borrowing Thread object.
    drop(parent);
    drop(child);
    assert_eq!(crate::r::blocking::guest_jobs_in_flight(VM), 0);
    crate::hv::set_guest_kill_for_test(VM, false);
}

#[test]
#[cfg(thread_scheduler_harness)]
fn slow_slice_captures_live_child_frames_and_formats_after_context_restore() {
    let _serial = SERIAL.lock().unwrap();
    crate::SLICE_LOGS.lock().unwrap().clear();
    let mut thread = make_thread(52, || {
        std::thread::sleep(std::time::Duration::from_millis(115));
        assert!(yield_now());
    });
    thread.0.timed = true;
    assert!(poll_thread(&mut thread).is_pending());
    assert!(thread.0.exit_trace.last_slice_ms >= 110);
    let logs = crate::SLICE_LOGS.lock().unwrap();
    assert_eq!(logs.len(), 1);
    let line = &logs[0];
    assert!(line.contains("vm=52 id=52 carrier=0"));
    assert!(line.contains("reason=Yield kind=exit-callstack cpu_attribution=none"));
    assert!(!line.contains("frames=[]"), "live stack walk lost all executable return frames");
    drop(logs);
    assert!(poll_thread(&mut thread).is_ready());
    assert!(thread.0.exit_trace.last_slice_ms < 100, "latest slice must exclude the preceding sleep/slice");
    assert_eq!(crate::SLICE_LOGS.lock().unwrap().len(), 1);
}

#[test]
#[cfg(thread_scheduler_harness)]
fn wls_lease_survives_suspension_and_completion_until_task_retirement() {
    let _serial = SERIAL.lock().unwrap();
    let mut thread = make_thread(51, || {
        let first = crate::wls::trueos_cabi_wls_current_slot();
        assert!(yield_now());
        assert_eq!(crate::wls::trueos_cabi_wls_current_slot(), first);
    });
    thread.0._wls_lease = Some(crate::wls::try_lease_blueprint_thread_identity(51, 51).unwrap());
    thread.0.wls.2 = thread.0._wls_lease.as_ref().map(crate::wls::BlueprintThreadIdentityLease::slot);
    assert!(poll_thread(&mut thread).is_pending());
    assert!(crate::wls::try_lease_blueprint_thread_identity(51, 51).is_none());
    assert!(poll_thread(&mut thread).is_ready());
    assert!(crate::wls::try_lease_blueprint_thread_identity(51, 51).is_none(), "completion alone cannot release a live Thread's lease");
    drop(thread);
    assert!(crate::wls::try_lease_blueprint_thread_identity(51, 51).is_some());
}

#[test]
#[cfg(thread_scheduler_harness)]
fn scoped_fanout_keeps_sixty_four_live_identities_through_scheduler_retirement() {
    let _serial = SERIAL.lock().unwrap();
    const VM: u8 = 54;
    let baseline = THREAD_COUNT.load(Ordering::Acquire);
    let observed = Arc::new(Mutex::new(Vec::new()));
    let mut tasks = Vec::new();
    for sequence in 1..=64usize {
        let id = crate::wls::BLUEPRINT_THREAD_CARRIER_TAG as usize | sequence;
        let identity = crate::wls::try_lease_blueprint_thread_identity(VM, id).unwrap();
        let expected_slot = identity.slot();
        let observations = observed.clone();
        let mut task = make_thread(sequence, move || {
            assert_eq!(current_id(), Some(id));
            assert_eq!(current_vm_id(), Some(VM));
            for _ in 0..2 {
                assert_eq!(crate::wls::trueos_cabi_wls_current_slot(), expected_slot);
                assert!(yield_now());
            }
            assert_eq!(crate::wls::trueos_cabi_wls_current_slot(), expected_slot);
            observations.lock().unwrap().push(expected_slot);
        });
        task.0.id = id;
        task.0.vm_id = Some(VM);
        task.0.domain = (77, VM);
        task.0.wls = (0, id as u32, Some(expected_slot));
        task.0._wls_lease = Some(identity);
        tasks.push(task);
    }
    assert_eq!(THREAD_COUNT.load(Ordering::Acquire), baseline + 64);
    for _ in 0..2 {
        for task in &mut tasks { assert!(poll_thread(task).is_pending()); }
    }
    for task in &mut tasks { assert!(poll_thread(task).is_ready()); }
    let mut slots = observed.lock().unwrap().clone();
    slots.sort_unstable(); slots.dedup();
    assert_eq!(slots.len(), 64);
    // Completion becomes visible to joiners before task/stack retirement.
    // All identities must remain reserved until that final owner is dropped.
    for sequence in 1..=64usize {
        let id = crate::wls::BLUEPRINT_THREAD_CARRIER_TAG as usize | sequence;
        assert!(crate::wls::try_lease_blueprint_thread_identity(VM, id).is_none());
    }
    drop(tasks);
    assert_eq!(THREAD_COUNT.load(Ordering::Acquire), baseline);
    for sequence in 1..=64usize {
        let id = crate::wls::BLUEPRINT_THREAD_CARRIER_TAG as usize | sequence;
        assert!(crate::wls::try_lease_blueprint_thread_identity(VM, id).is_some());
    }
}

#[test]
#[cfg(thread_scheduler_harness)]
fn diagnostics_identify_the_parked_worker_and_retire_on_force_kill() {
    let _serial = SERIAL.lock().unwrap();
    let queue = Arc::new(crate::wait::WaitQueue::new());
    let mut thread = make_thread(53, move || {
        set_current_name("tokio-voxygen-0");
        let observed = queue.observe();
        assert_eq!(wait(&queue, observed, 0), Some(true));
        panic!("killed continuation resumed");
    });
    assert!(crate::r::blocking::open_guest_jobs(53, 19));
    let owner = crate::r::blocking::reserve_guest_job(53).unwrap();
    owner.diagnostic.identify("std-thread", 53, 0);
    owner.diagnostic.enable_progress_probe();
    thread.0._admission._owner = Some(owner);
    assert!(poll_thread(&mut thread).is_pending());
    let before = diagnostics::lines(53).join("\n");
    assert!(diagnostics::progress_lines(53, 1)[0].contains("stage=poll-return"));
    assert!(before.contains("name=tokio-voxygen-0 carrier=0 phase=wait-queue"));
    // Re-polling a parked task does not falsely count as guest progress.
    assert!(poll_thread(&mut thread).is_pending());
    assert!(diagnostics::progress_lines(53, 1)[0].contains("stage=wait-pending"));
    assert!(diagnostics::lines(53)[0].contains("boundary=2"));
    kill_with_hull_stopped(53);
    wake_guest_control(53);
    assert!(poll_thread(&mut thread).is_ready());
    assert!(diagnostics::lines(53)[0].contains("phase=cancelling"));
    drop(thread);
    assert!(diagnostics::lines(53).is_empty());
    crate::hv::set_guest_kill_for_test(53, false);
}

#[test]
fn two_continuations_keep_frames_realm_and_errno_on_one_carrier() {
    let _serial = SERIAL.lock().unwrap();
    kernel_task_domain::replace_context((91, 92));
    crate::allocators::replace_thread_context([1, 2, 3, 4]);
    crate::wls::replace_thread_context((17, 18, None));
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut tasks = Vec::new();
    for id in [11usize, 12] {
        let events = events.clone();
        tasks.push(make_thread(id, move || {
            assert_eq!(current_id(), Some(id));
            assert_eq!(current_vm_id(), Some(id as u8));
            assert_eq!(realm(), ((77, id as u8), [0; 4], (0, id as u32, None)));
            let mut frame = [id; 128];
            let frame_address = frame.as_ptr() as usize;
            kernel_task_domain::replace_context((101, id as u8));
            crate::allocators::replace_thread_context([id as u32; 4]);
            crate::wls::replace_thread_context((id as u64 + 100, id as u32, None));
            let errno = current_errno().unwrap().as_ptr();
            current_errno()
                .unwrap()
                .store(id as i32 + 30, Ordering::Relaxed);
            for stage in 0..3 {
                events.lock().unwrap().push((id, stage, frame_address));
                assert!(yield_now());
                assert_eq!(current_id(), Some(id));
                assert_eq!(
                    realm(),
                    ((101, id as u8), [id as u32; 4], (id as u64 + 100, id as u32, None))
                );
                assert_eq!(current_errno().unwrap().as_ptr(), errno);
                assert_eq!(current_errno().unwrap().load(Ordering::Relaxed), id as i32 + 30);
                frame[stage] += 1;
                assert_eq!(frame[stage], id + 1);
            }
        }));
    }
    for _ in 0..3 {
        for task in &mut tasks {
            assert!(poll_thread(task).is_pending());
        }
    }
    for task in &mut tasks {
        assert!(poll_thread(task).is_ready());
    }
    let events = events.lock().unwrap();
    assert_eq!(events.len(), 6);
    assert_ne!(events[0].2, events[1].2);
    for (index, &(id, stage, _)) in events.iter().enumerate() {
        assert_eq!((id, stage), (11 + index % 2, index / 2));
    }
}

#[test]
fn notification_before_registration_is_retained() {
    let _serial = SERIAL.lock().unwrap();
    let queue = Arc::new(crate::wait::WaitQueue::new());
    let mut thread = make_thread(21, move || {
        let observed = queue.observe();
        queue.notify_all();
        assert_eq!(wait(&queue, observed, 0), Some(true));
    });
    assert!(poll_thread(&mut thread).is_pending());
    assert!(poll_thread(&mut thread).is_ready());
}

#[test]
fn experimental_continuation_keeps_identity_and_wait_semantics() {
    let _serial = SERIAL.lock().unwrap();
    let queue = Arc::new(crate::wait::WaitQueue::new());
    let waited = queue.clone();
    let mut thread = make_thread(22, move || {
        assert_eq!(current_id(), Some(22));
        assert_eq!(current_vm_id(), Some(22));
        assert_eq!(wait(&waited, waited.observe(), 0), Some(true));
        assert_eq!(current_id(), Some(22));
    });
    thread.0.timed = true;
    thread.0.submitted_ms = crate::r::input_timing::now_ms();
    assert!(poll_thread(&mut thread).is_pending());
    assert!(poll_thread(&mut thread).is_pending());
    queue.notify_all();
    assert!(poll_thread(&mut thread).is_ready());
}

#[test]
fn parked_continuation_leaves_carrier_free_for_the_notifier() {
    let _serial = SERIAL.lock().unwrap();
    let queue = Arc::new(crate::wait::WaitQueue::new());
    let progress = Arc::new(AtomicUsize::new(0));
    let waiter_queue = queue.clone();
    let waiter_progress = progress.clone();
    let mut waiter = make_thread(31, move || {
        let observed = waiter_queue.observe();
        waiter_progress.store(1, Ordering::Release);
        assert_eq!(wait(&waiter_queue, observed, 0), Some(true));
        assert_eq!(waiter_progress.load(Ordering::Acquire), 2);
        waiter_progress.store(3, Ordering::Release);
    });
    let notifier_progress = progress.clone();
    let mut notifier = make_thread(32, move || {
        assert_eq!(notifier_progress.load(Ordering::Acquire), 1);
        notifier_progress.store(2, Ordering::Release);
        queue.notify_one();
    });
    assert!(poll_thread(&mut waiter).is_pending());
    assert!(poll_thread(&mut waiter).is_pending());
    assert!(poll_thread(&mut notifier).is_ready());
    assert!(poll_thread(&mut waiter).is_ready());
    assert_eq!(progress.load(Ordering::Acquire), 3);
}

#[test]
fn external_notification_does_not_preempt_a_running_continuation() {
    // This injects an external host notification, not a hardware interrupt.
    // The production continuation remains on the simulated single carrier.
    let _serial = SERIAL.lock().unwrap();
    use core::sync::atomic::AtomicBool;
    use std::time::{Duration, Instant};
    struct WakeCount(AtomicUsize);
    impl std::task::Wake for WakeCount {
        fn wake(self: Arc<Self>) { self.0.fetch_add(1, Ordering::AcqRel); }
    }
    let wakes = Arc::new(WakeCount(AtomicUsize::new(0)));
    let waker = Waker::from(wakes.clone());
    let entered = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let sibling_progress = Arc::new(AtomicUsize::new(0));
    let progress = sibling_progress.clone();
    let mut sibling = make_thread(36, move || { progress.store(1, Ordering::Release); });
    let running = entered.clone();
    let stop = release.clone();
    let peer = sibling_progress.clone();
    let mut spinner = make_thread(35, move || {
        running.store(true, Ordering::Release);
        let deadline = Instant::now() + Duration::from_secs(2);
        while !stop.load(Ordering::Acquire) && Instant::now() < deadline {
            core::hint::spin_loop();
        }
        assert!(stop.load(Ordering::Acquire), "external probe deadline expired");
        assert_eq!(peer.load(Ordering::Acquire), 0, "sibling ran inside an active poll");
    });
    let notify = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(1);
        while !entered.load(Ordering::Acquire) && Instant::now() < deadline {
            std::thread::yield_now();
        }
        let active = entered.load(Ordering::Acquire);
        waker.wake_by_ref();
        // The observer and release live outside the suspect carrier, ensuring
        // the negative case is bounded instead of deadlocking the harness.
        std::thread::sleep(Duration::from_millis(5));
        release.store(true, Ordering::Release);
        active
    });
    assert!(poll_thread(&mut spinner).is_ready());
    assert!(notify.join().unwrap());
    assert_eq!(wakes.0.load(Ordering::Acquire), 1);
    assert_eq!(sibling_progress.load(Ordering::Acquire), 0);
    assert!(poll_thread(&mut sibling).is_ready());
    assert_eq!(sibling_progress.load(Ordering::Acquire), 1);
}

#[test]
fn nested_child_completion_joins_without_reentering_parent_stack() {
    let _serial = SERIAL.lock().unwrap();
    let children = Arc::new(Mutex::new(Vec::new()));
    let child_queue = children.clone();
    let completed = Arc::new(AtomicUsize::new(0));
    let result = completed.clone();
    let mut parent = make_thread(41, move || {
        let cell = Arc::new(crate::wait::CompletionCell::new());
        let child_cell = cell.clone();
        child_queue.lock().unwrap().push(make_thread(42, move || {
            assert_eq!(current_id(), Some(42));
            assert!(child_cell.complete(123usize).is_ok());
        }));
        let value = cell.join_blocking_parked();
        assert_eq!(current_id(), Some(41));
        result.store(value, Ordering::Release);
    });
    assert!(poll_thread(&mut parent).is_pending());
    let mut child = children.lock().unwrap().pop().unwrap();
    assert!(poll_thread(&mut child).is_ready());
    assert!(poll_thread(&mut parent).is_ready());
    assert_eq!(completed.load(Ordering::Acquire), 123);
}

#[test]
fn timed_wait_resumes_only_after_its_deadline() {
    let _serial = SERIAL.lock().unwrap();
    let queue = Arc::new(crate::wait::WaitQueue::new());
    let result = Arc::new(AtomicUsize::new(0));
    let done = result.clone();
    let mut thread = make_thread(51, move || {
        let observed = queue.observe();
        assert_eq!(wait(&queue, observed, 5), Some(false));
        done.store(1, Ordering::Release);
    });
    assert!(poll_thread(&mut thread).is_pending());
    assert_eq!(result.load(Ordering::Acquire), 0);
    std::thread::sleep(std::time::Duration::from_millis(10));
    assert!(poll_thread(&mut thread).is_ready());
    assert_eq!(result.load(Ordering::Acquire), 1);
}


#[cfg(thread_scheduler_harness)]
#[test]
fn kill_discards_a_parked_stack_without_guest_drops_or_resuming() {
    let _serial = SERIAL.lock().unwrap();
    let drops = Arc::new(AtomicUsize::new(0));
    struct GuestDrop(Arc<AtomicUsize>);
    impl Drop for GuestDrop {
        fn drop(&mut self) { self.0.fetch_add(1, Ordering::AcqRel); }
    }
    let drop_counter = drops.clone();
    let mut task = make_thread(51, move || {
        let _guest = GuestDrop(drop_counter);
        suspend(Some(Box::pin(core::future::pending())), exit_trace::Reason::Wait);
        panic!("killed guest resumed");
    });
    struct WakeCount(AtomicUsize);
    impl std::task::Wake for WakeCount {
        fn wake(self: Arc<Self>) { self.0.fetch_add(1, Ordering::AcqRel); }
    }
    let wakes = Arc::new(WakeCount(AtomicUsize::new(0)));
    let waker = std::task::Waker::from(wakes.clone());
    let mut cx = Context::from_waker(&waker);
    task.0.timed = true;
    assert!(Pin::new(&mut task).poll(&mut cx).is_pending());
    assert!(Pin::new(&mut task).poll(&mut cx).is_pending());
    kill_with_hull_stopped(51);
    wake_guest_control(51);
    assert_eq!(wakes.0.load(Ordering::Acquire), 1, "kill must wake an indefinite park");
    assert!(poll_thread(&mut task).is_ready());
    drop(task);
    assert_eq!(drops.load(Ordering::Acquire), 0);
    crate::hv::set_guest_kill_for_test(51, false);
}

#[cfg(thread_scheduler_harness)]
#[test]
fn kill_before_first_poll_never_calls_the_guest_closure() {
    let _serial = SERIAL.lock().unwrap();
    let mut task = make_thread(52, || panic!("killed job ran"));
    kill_with_hull_stopped(52);
    wake_guest_control(52);
    assert!(poll_thread(&mut task).is_ready());
    drop(task);
    crate::hv::set_guest_kill_for_test(52, false);
}

#[cfg(thread_scheduler_harness)]
#[test]
fn pause_retains_native_frames_and_resumes_the_same_reservation() {
    let _serial = SERIAL.lock().unwrap();
    let progress = Arc::new(AtomicUsize::new(0));
    let guest_progress = progress.clone();
    let mut task = make_thread(54, move || {
        let mut frame = [7usize; 32];
        let address = frame.as_ptr() as usize;
        guest_progress.store(1, Ordering::Release);
        assert!(yield_now());
        assert_eq!(frame.as_ptr() as usize, address);
        frame[0] += 1;
        assert_eq!(frame[0], 8);
        guest_progress.store(2, Ordering::Release);
    });
    assert!(crate::r::blocking::open_guest_jobs(54, 19));
    task.0.timed = true;
    task.0._admission._owner = Some(crate::r::blocking::reserve_guest_job(54).unwrap());
    assert!(poll_thread(&mut task).is_pending());
    assert_eq!(progress.load(Ordering::Acquire), 1);
    assert_eq!(crate::r::blocking::pause_guest_jobs(54), 1);
    assert!(!crate::r::blocking::guest_job_cancellation_requested(54));
    crate::hv::set_guest_pause_for_test(54, true);
    wake_guest_control(54);
    assert!(poll_thread(&mut task).is_pending());
    assert!(poll_thread(&mut task).is_pending());
    assert!(crate::r::blocking::guest_jobs_quiescent(54));
    assert_eq!(progress.load(Ordering::Acquire), 1);
    assert_eq!(crate::r::blocking::guest_jobs_in_flight(54), 1);
    assert!(crate::r::blocking::resume_guest_jobs(54, 19));
    crate::hv::set_guest_pause_for_test(54, false);
    wake_guest_control(54);
    assert!(poll_thread(&mut task).is_ready());
    assert_eq!(progress.load(Ordering::Acquire), 2);
    drop(task);
    assert_eq!(crate::r::blocking::guest_jobs_in_flight(54), 0);
}

#[cfg(thread_scheduler_harness)]
#[test]
fn filesystem_work_reaches_its_boundary_before_native_ram_parking() {
    let _serial = SERIAL.lock().unwrap();
    let progress = Arc::new(AtomicUsize::new(0));
    let guest_progress = progress.clone();
    let mut task = make_thread(55, move || {
        guest_progress.store(1, Ordering::Release);
        assert!(yield_now());
        guest_progress.store(2, Ordering::Release);
        assert!(yield_now());
        guest_progress.store(3, Ordering::Release);
    });
    assert!(crate::r::blocking::open_guest_jobs(55, 19));
    task.0._admission._owner = Some(crate::r::blocking::reserve_guest_job(55).unwrap());
    assert!(poll_thread(&mut task).is_pending());
    crate::r::blocking::pause_guest_jobs(55);
    crate::hv::set_guest_pause_for_test(55, true);
    crate::hv::set_guest_io_for_test(55, true);
    assert!(poll_thread(&mut task).is_pending());
    assert_eq!(progress.load(Ordering::Acquire), 2);
    assert!(!crate::r::blocking::guest_jobs_quiescent(55));
    crate::hv::set_guest_io_for_test(55, false);
    wake_guest_control(55);
    assert!(poll_thread(&mut task).is_pending());
    assert!(crate::r::blocking::guest_jobs_quiescent(55));
    assert_eq!(progress.load(Ordering::Acquire), 2);
    assert!(crate::r::blocking::resume_guest_jobs(55, 19));
    crate::hv::set_guest_pause_for_test(55, false);
    wake_guest_control(55);
    assert!(poll_thread(&mut task).is_ready());
    assert_eq!(progress.load(Ordering::Acquire), 3);
    drop(task);
}

#[cfg(thread_scheduler_harness)]
#[test]
fn control_wakers_survive_repeated_pause_and_resume_notifications() {
    let _serial = SERIAL.lock().unwrap();
    let mut task = make_thread(56, || {
        suspend(Some(Box::pin(core::future::pending())), exit_trace::Reason::Wait);
        panic!("unresolved wait resumed");
    });
    struct WakeCount(AtomicUsize);
    impl std::task::Wake for WakeCount {
        fn wake(self: Arc<Self>) { self.0.fetch_add(1, Ordering::AcqRel); }
    }
    let wakes = Arc::new(WakeCount(AtomicUsize::new(0)));
    let waker = std::task::Waker::from(wakes.clone());
    let mut cx = Context::from_waker(&waker);
    assert!(Pin::new(&mut task).poll(&mut cx).is_pending());
    wake_guest_control(56);
    wake_guest_control(56);
    assert_eq!(wakes.0.load(Ordering::Acquire), 2);
    crate::hv::set_guest_pause_for_test(56, true);
    assert!(Pin::new(&mut task).poll(&mut cx).is_pending());
    wake_guest_control(56);
    assert_eq!(wakes.0.load(Ordering::Acquire), 3);
    crate::hv::set_guest_pause_for_test(56, false);
    kill_with_hull_stopped(56);
    assert!(Pin::new(&mut task).poll(&mut cx).is_ready());
    drop(task);
    wake_guest_control(56);
    assert_eq!(wakes.0.load(Ordering::Acquire), 3, "retired task kept a control waker");
    crate::hv::set_guest_kill_for_test(56, false);
}

#[cfg(thread_scheduler_harness)]
#[test]
fn kill_of_a_paused_native_job_retires_its_counted_park() {
    let _serial = SERIAL.lock().unwrap();
    let mut task = make_thread(57, || {
        assert!(yield_now());
        panic!("killed pause resumed");
    });
    assert!(crate::r::blocking::open_guest_jobs(57, 19));
    task.0._admission._owner = Some(crate::r::blocking::reserve_guest_job(57).unwrap());
    assert!(poll_thread(&mut task).is_pending());
    crate::r::blocking::pause_guest_jobs(57);
    crate::hv::set_guest_pause_for_test(57, true);
    assert!(poll_thread(&mut task).is_pending());
    assert!(crate::r::blocking::guest_jobs_quiescent(57));
    kill_with_hull_stopped(57);
    crate::r::blocking::close_guest_jobs(57);
    assert!(crate::r::blocking::guest_job_cancellation_requested(57));
    wake_guest_control(57);
    assert!(poll_thread(&mut task).is_ready());
    drop(task);
    assert_eq!(crate::r::blocking::guest_jobs_in_flight(57), 0);
    assert!(!crate::r::blocking::resume_guest_jobs(57, 19));
    crate::hv::set_guest_pause_for_test(57, false);
    crate::hv::set_guest_kill_for_test(57, false);
}


#[test]
#[cfg(thread_scheduler_harness)]
fn abi_breadcrumb_follows_nested_parked_stack_and_retires_with_owner() {
    let _serial = SERIAL.lock().unwrap();
    let queue = Arc::new(crate::wait::WaitQueue::new());
    let mut task = make_thread(52, move || {
        let _outer = unsafe { abi_scope(diagnostics::AbiOperation::Condvar) };
        let observed = queue.observe();
        let _ = wait(&queue, observed, 0);
        panic!("killed breadcrumb stack resumed");
    });
    task.0.timed = true;
    assert!(crate::r::blocking::open_guest_jobs(52, 19));
    let owner = crate::r::blocking::reserve_guest_job(52).unwrap();
    owner.diagnostic.identify("codex-executor-task", 52, 0);
    owner.diagnostic.enable_progress_probe();
    task.0._admission._owner = Some(owner);
    assert!(poll_thread(&mut task).is_pending());
    let line = diagnostics::progress_lines(52, 1).remove(0);
    assert!(line.contains(" abi=wait "));
    assert!(line.contains(" abi_depth=2 abi_entered=2 abi_exited=0 "));
    assert!(current_id().is_none());
    // A carrier's next task cannot inherit the parked stack's active scope.
    drop(unsafe { abi_scope(diagnostics::AbiOperation::Alloc) });
    assert!(diagnostics::progress_lines(52, 1)[0].contains(" abi=wait "));
    kill_with_hull_stopped(52);
    assert!(poll_thread(&mut task).is_ready());
    drop(task);
    assert!(diagnostics::progress_lines(52, 1).is_empty());
    crate::hv::set_guest_kill_for_test(52, false);
}
