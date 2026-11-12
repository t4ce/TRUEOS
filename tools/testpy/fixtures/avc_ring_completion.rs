#![allow(dead_code)]
use core::future::{Future, poll_fn};
use core::pin::Pin;
use core::task::{Context, Poll, Waker};
use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::Wake;
use trueos_time::{Duration, Timer};

@TIMEOUT_NS@

thread_local! {
    static NOW: Cell<u64> = const { Cell::new(0) };
    static TIMERS: Cell<usize> = const { Cell::new(0) };
    static READS: Cell<usize> = const { Cell::new(0) };
    static ON_READ: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
    static ON_REGISTER: RefCell<Option<Box<dyn FnMut()>>> = const { RefCell::new(None) };
}

mod chronos {
    pub fn monotonic_nanos() -> u64 { super::NOW.get() }
}

mod trueos_time {
    use super::*;
    pub struct Duration(u64);
    impl Duration {
        pub fn from_nanos(value: u64) -> Self { Self(value) }
    }
    pub struct Timer { deadline: u64 }
    impl Timer {
        pub fn after(duration: Duration) -> Self {
            TIMERS.set(TIMERS.get() + 1);
            Self { deadline: NOW.get().saturating_add(duration.0) }
        }
    }
    impl Future for Timer {
        type Output = ();
        fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
            if NOW.get() >= self.deadline { Poll::Ready(()) } else { Poll::Pending }
        }
    }
}

mod intel {
    use super::*;
    #[path = "@GENERATION_PATH@"]
    mod generation;
    pub fn dma_flush(_: *mut u8, _: usize) {
        READS.set(READS.get() + 1);
        let hook = ON_READ.take();
        if let Some(hook) = hook { hook(); }
    }
    pub mod interrupts {
        use super::*;
        thread_local! {
            static GENERATION: generation::Generation = const { generation::Generation::new() };
            static ROUTE: Cell<bool> = const { Cell::new(true) };
            static REGISTERED: RefCell<Option<Waker>> = const { RefCell::new(None) };
        }
        struct Generation;
        impl Generation {
            fn changed_after_register(&self, observed: u64, register: impl FnOnce()) -> bool {
                GENERATION.with(|generation| generation.changed_after_register(observed, register))
            }
        }
        struct TestWaker;
        impl TestWaker {
            fn register(&self, waker: &Waker) {
                REGISTERED.set(Some(waker.clone()));
                ON_REGISTER.with_borrow_mut(|hook| { if let Some(hook) = hook { hook(); } });
            }
        }
        static VCS0_GENERATION: Generation = Generation;
        static VCS0_WAKER: TestWaker = TestWaker;
        pub fn vcs0_generation() -> u64 { GENERATION.with(|value| value.observe()) }
        pub fn vcs0_route_ready() -> bool { ROUTE.get() }
        pub fn notify() {
            GENERATION.with(|value| value.notify());
            REGISTERED.with_borrow(|value| { if let Some(waker) = value { waker.wake_by_ref(); } });
        }
        pub fn fault() { ROUTE.set(false); notify(); }
        @IRQ_WAITER@
    }
}

@PRODUCTION@

#[derive(Default)]
struct WakeCount(AtomicUsize);
impl Wake for WakeCount {
    fn wake(self: Arc<Self>) { self.0.fetch_add(1, Ordering::Relaxed); }
    fn wake_by_ref(self: &Arc<Self>) { self.0.fetch_add(1, Ordering::Relaxed); }
}
fn poll_once<T>(future: Pin<&mut impl Future<Output = T>>) -> Poll<T> {
    let waker = Waker::from(Arc::new(WakeCount::default()));
    future.poll(&mut Context::from_waker(&waker))
}

#[test]
fn epilogue_is_in_ring_after_primary_return_with_ggtt_store_before_interrupt() {
    let mut ring = vec![0xdeadbeefu32; 4096];
    let tail = append_ring_batch_start_completion_words(
        ring.as_mut_ptr().cast(), 16 * 1024, 0,
        0x4111_0000, 0x4156_4301, 0x4110_0000, 0x4f00_8000, 37,
        MediaJobMode::AVC_ENCODE_GUC,
    );
    assert_eq!(tail, Some(128));
    // Independently decode the actual HW encodings, length and address bit.
    assert_eq!(&ring[..17], &[
        0x1000_0002, 0x4111_0000 + MEDIA_RESULT_KICKOFF_SLOT as u32, 0, 0x4156_4301,
        0x1880_0101, 0x4110_0000, 0,
        0x1300_0002, 0, 0, 0,
        0x1300_4002, 0x4f00_8004, 0, 37,
        0x0100_0000, 0x0280_0000,
    ]);
    assert!(ring[17..32].iter().all(|word| *word == 0));
    assert!(ring[32..].iter().all(|word| *word == 0xdeadbeef));
    let mut primary = [0xffff_ffffu32; 10];
    assert_eq!(build_primary_second_level_return_words(
        primary.as_mut_ptr().cast(), 40, 0x4111_0000, 0x4110_1000,
        0x4156_4304, MediaJobMode::AVC_ENCODE_GUC,
    ), Some(40));
    assert_eq!(primary[0], 0x18c0_0101); // second-level codec call
    assert_eq!(primary[8], 0x0500_0000); // returns to the ring epilogue
}

#[test]
fn entry_wrap_preserves_neighbors_and_rejects_invalid_inputs_before_writes() {
    let mut ring = vec![0xdeadbeefu32; 4096];
    let ptr = ring.as_mut_ptr().cast::<u8>();
    let append = |ptr, bytes, offset, address, sequence, mode| {
        append_ring_batch_start_completion_words(
            ptr, bytes, offset, 0x4111_0000, 1, 0x4110_0000, address, sequence, mode,
        )
    };
    for (bytes, offset, address, sequence) in [
        (0, 0, 0x4f00_8000, 1), (64, 0, 0x4f00_8000, 1),
        (16_000, 0, 0x4f00_8000, 1), (16_384, 64, 0x4f00_8000, 1),
        (16_384, 16_384, 0x4f00_8000, 1), (16_384, 0, 0, 1),
        (16_384, 0, 0x1_4f00_8000, 1), (16_384, 0, 0x4f00_8004, 1),
        (16_384, 0, 0x4f00_8020, 1), (16_384, 0, 0x4f00_8000, 0),
    ] {
        assert_eq!(append(ptr, bytes, offset, address, sequence, MediaJobMode::AVC_ENCODE_GUC), None);
    }
    assert_eq!(append(core::ptr::null_mut(), 16_384, 0, 0x4f00_8000, 1, MediaJobMode::AVC_ENCODE_GUC), None);
    assert_eq!(append(unsafe { ptr.add(1) }, 16_384, 0, 0x4f00_8000, 1, MediaJobMode::AVC_ENCODE_GUC), None);
    assert_eq!(append(ptr, 16_384, 0, 0x4f00_8000, 1, MediaJobMode::AVC_DECODE_GUC), None);
    assert!(ring.iter().all(|word| *word == 0xdeadbeef));
    assert_eq!(append(ptr, 16_384, 16_256, 0x4f00_8000, 128, MediaJobMode::AVC_ENCODE_GUC), Some(0));
    assert!(ring[..4064].iter().all(|word| *word == 0xdeadbeef));
    assert_eq!(ring[4064 + 14], 128);
    assert_eq!(append(ptr, 16_384, 0, 0x4f00_8000, 129, MediaJobMode::AVC_ENCODE_GUC), Some(128));
    assert_eq!(ring[14], 129);
    assert_eq!(ring[4064 + 14], 128);
}

#[test]
fn sequence_never_reuses_zero_or_wraps_into_an_old_breadcrumb() {
    assert_eq!(next_completion_sequence(0), Some(1));
    assert_eq!(next_completion_sequence(u32::MAX - 1), Some(u32::MAX));
    assert_eq!(next_completion_sequence(u32::MAX), None);
}

#[test]
fn already_completed_picture_needs_no_interrupt_or_registration() {
    let mut page = 19u32;
    let mut waiter = core::pin::pin!(wait_for_ring_completion((&mut page as *mut u32).cast(), 19));
    assert_eq!(poll_once(waiter.as_mut()), Poll::Ready(RingCompletionWait {
        sequence: 19, wakeups: 0, watchdog: false, route_failed: false,
    }));
    assert_eq!(TIMERS.get(), 1);
}

#[test]
fn stalled_encoder_parks_and_stale_interrupt_cannot_retire_new_ring_lap() {
    let mut page = 8u32;
    let ptr = &mut page as *mut u32;
    let mut waiter = core::pin::pin!(wait_for_ring_completion(ptr.cast(), 136));
    assert_eq!(poll_once(waiter.as_mut()), Poll::Pending);
    assert_eq!(READS.get(), 1);
    for _ in 0..8 { assert_eq!(poll_once(waiter.as_mut()), Poll::Pending); }
    assert_eq!(READS.get(), 1); // No periodic memory sampling while parked.
    intel::interrupts::notify();
    assert_eq!(poll_once(waiter.as_mut()), Poll::Pending);
    assert_eq!(READS.get(), 2);
    unsafe { core::ptr::write_volatile(ptr, 136); }
    intel::interrupts::notify();
    assert_eq!(poll_once(waiter.as_mut()), Poll::Ready(RingCompletionWait {
        sequence: 136, wakeups: 2, watchdog: false, route_failed: false,
    }));
    assert_eq!(TIMERS.get(), 1);
}

#[test]
fn completion_during_waker_registration_is_not_lost() {
    let mut page = 0u32;
    let ptr = &mut page as *mut u32;
    ON_REGISTER.set(Some(Box::new(move || {
        unsafe { core::ptr::write_volatile(ptr, 1); }
        intel::interrupts::notify();
    })));
    let mut waiter = core::pin::pin!(wait_for_ring_completion(ptr.cast(), 1));
    assert_eq!(poll_once(waiter.as_mut()), Poll::Ready(RingCompletionWait {
        sequence: 1, wakeups: 1, watchdog: false, route_failed: false,
    }));
}

#[test]
fn completion_between_generation_snapshot_and_memory_read_is_not_lost() {
    let mut page = 0u32;
    let ptr = &mut page as *mut u32;
    ON_READ.set(Some(Box::new(move || {
        unsafe { core::ptr::write_volatile(ptr, 2); }
        intel::interrupts::notify();
    })));
    let mut waiter = core::pin::pin!(wait_for_ring_completion(ptr.cast(), 2));
    assert_eq!(poll_once(waiter.as_mut()), Poll::Ready(RingCompletionWait {
        sequence: 2, wakeups: 0, watchdog: false, route_failed: false,
    }));
}

#[test]
fn missed_irq_is_reported_but_watchdog_final_read_accepts_exact_completion() {
    let mut page = 0u32;
    let ptr = &mut page as *mut u32;
    let mut waiter = core::pin::pin!(wait_for_ring_completion(ptr.cast(), 3));
    assert_eq!(poll_once(waiter.as_mut()), Poll::Pending);
    unsafe { core::ptr::write_volatile(ptr, 3); }
    NOW.set(TIMEOUT_NS);
    assert_eq!(poll_once(waiter.as_mut()), Poll::Ready(RingCompletionWait {
        sequence: 3, wakeups: 0, watchdog: true, route_failed: false,
    }));
    assert_eq!(READS.get(), 2);
    assert_eq!(TIMERS.get(), 1);
}

#[test]
fn absent_completion_times_out_once_and_keeps_old_sequence_unretired() {
    let mut page = 3u32;
    let mut waiter = core::pin::pin!(wait_for_ring_completion((&mut page as *mut u32).cast(), 4));
    assert_eq!(poll_once(waiter.as_mut()), Poll::Pending);
    NOW.set(TIMEOUT_NS);
    assert_eq!(poll_once(waiter.as_mut()), Poll::Ready(RingCompletionWait {
        sequence: 3, wakeups: 0, watchdog: true, route_failed: false,
    }));
    assert_eq!(TIMERS.get(), 1);
}

#[test]
fn route_fault_wakes_owner_and_cannot_fabricate_retirement() {
    let mut page = 0u32;
    let mut waiter = core::pin::pin!(wait_for_ring_completion((&mut page as *mut u32).cast(), 1));
    assert_eq!(poll_once(waiter.as_mut()), Poll::Pending);
    intel::interrupts::fault();
    assert_eq!(poll_once(waiter.as_mut()), Poll::Ready(RingCompletionWait {
        sequence: 0, wakeups: 1, watchdog: false, route_failed: true,
    }));
    assert_eq!(NOW.get(), 0);
}

#[test]
fn continuous_spurious_notifications_cannot_extend_watchdog() {
    let mut page = 0u32;
    ON_REGISTER.set(Some(Box::new(|| {
        NOW.set(NOW.get() + TIMEOUT_NS / 4);
        intel::interrupts::notify();
    })));
    let mut waiter = core::pin::pin!(wait_for_ring_completion((&mut page as *mut u32).cast(), 1));
    assert_eq!(poll_once(waiter.as_mut()), Poll::Ready(RingCompletionWait {
        sequence: 0, wakeups: 4, watchdog: true, route_failed: false,
    }));
    assert_eq!(TIMERS.get(), 1);
    assert_eq!(NOW.get(), TIMEOUT_NS);
}
