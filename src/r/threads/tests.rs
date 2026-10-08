extern crate std;

use super::*;
use alloc::sync::Arc;
use alloc::vec::Vec;
use std::sync::Mutex;

static SERIAL: Mutex<()> = Mutex::new(());

type Realm = ((u32, u8), [u32; 4], (u64, u32));

fn realm() -> Realm {
    let domain = kernel_task_domain::replace_context((0, u8::MAX));
    kernel_task_domain::replace_context(domain);
    let allocation = crate::allocators::replace_thread_context([0; 4]);
    crate::allocators::replace_thread_context(allocation);
    let wls = crate::wls::replace_thread_context((0, 0));
    crate::wls::replace_thread_context(wls);
    (domain, allocation, wls)
}

fn make_thread(id: usize, job: impl FnOnce() + Send + 'static) -> ThreadTask {
    THREAD_COUNT.fetch_add(1, Ordering::AcqRel);
    ThreadTask(Box::new(Thread {
        stack: context::Stack::new(256 * 1024).unwrap(),
        child: 0,
        parent: 0,
        id,
        errno: AtomicI32::new(0),
        carrier: crate::percpu::current_slot() as u32,
        vm_id: Some(id as u8),
        job: Some(Job::Owned(Box::new(job))),
        done: false,
        kill_waker: None,
        wait: None,
        domain: (77, id as u8),
        allocation: [0; 4],
        wls: (0, id as u32),
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

#[test]
fn two_continuations_keep_frames_realm_and_errno_on_one_carrier() {
    let _serial = SERIAL.lock().unwrap();
    kernel_task_domain::replace_context((91, 92));
    crate::allocators::replace_thread_context([1, 2, 3, 4]);
    crate::wls::replace_thread_context((17, 18));
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut tasks = Vec::new();
    for id in [11usize, 12] {
        let events = events.clone();
        tasks.push(make_thread(id, move || {
            assert_eq!(current_id(), Some(id));
            assert_eq!(current_vm_id(), Some(id as u8));
            assert_eq!(realm(), ((77, id as u8), [0; 4], (0, id as u32)));
            let mut frame = [id; 128];
            let frame_address = frame.as_ptr() as usize;
            kernel_task_domain::replace_context((101, id as u8));
            crate::allocators::replace_thread_context([id as u32; 4]);
            crate::wls::replace_thread_context((id as u64 + 100, id as u32));
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
                    ((101, id as u8), [id as u32; 4], (id as u64 + 100, id as u32))
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
        suspend(Some(Box::pin(core::future::pending())));
        panic!("killed guest resumed");
    });
    struct WakeCount(AtomicUsize);
    impl std::task::Wake for WakeCount {
        fn wake(self: Arc<Self>) { self.0.fetch_add(1, Ordering::AcqRel); }
    }
    let wakes = Arc::new(WakeCount(AtomicUsize::new(0)));
    let waker = std::task::Waker::from(wakes.clone());
    let mut cx = Context::from_waker(&waker);
    assert!(Pin::new(&mut task).poll(&mut cx).is_pending());
    assert!(Pin::new(&mut task).poll(&mut cx).is_pending());
    crate::hv::set_guest_kill_for_test(51, true);
    wake_killed_guest(51);
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
    crate::hv::set_guest_kill_for_test(52, true);
    wake_killed_guest(52);
    assert!(poll_thread(&mut task).is_ready());
    drop(task);
    crate::hv::set_guest_kill_for_test(52, false);
}
