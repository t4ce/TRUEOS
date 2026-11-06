//! Compile the actual std algorithms and TRUEOS transport, modeling only the
//! native platform boundary. The Python runner supplies production paths.
#![allow(dead_code, unsafe_op_in_unsafe_fn)]

pub use std::{cell, hint, pin, time};
mod sync {
    pub use std::sync::atomic;
    pub mod once {
        pub(crate) enum OnceExclusiveState { Incomplete, Poisoned, Complete }
    }
    pub struct OnceState { pub(crate) inner: crate::sys::sync::once::OnceState }
}

// @PRODUCTION_MODULES@

use std::cell::{Cell, RefCell, UnsafeCell};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Barrier, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};
use sys::sync::{Condvar as RawCondvar, Mutex as RawMutex, Parker, RwLock};

const LIMIT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Checkpoint { Observed, EnterWait, Registered }
#[derive(Clone, Copy, Debug)]
struct Event { checkpoint: Checkpoint, timeout_ms: u64 }
struct Probe {
    events: mpsc::Sender<Event>,
    pause: Option<(Checkpoint, mpsc::Receiver<()>)>,
}
thread_local! {
    static SCOPE: Cell<u16> = const { Cell::new(0) };
    static PROBE: RefCell<Option<Probe>> = const { RefCell::new(None) };
}
fn event(checkpoint: Checkpoint, timeout_ms: u64) {
    PROBE.with_borrow_mut(|probe| {
        if let Some(probe) = probe {
            let _ = probe.events.send(Event { checkpoint, timeout_ms });
            if probe.pause.as_ref().map(|p| p.0) == Some(checkpoint) {
                let (_, gate) = probe.pause.take().unwrap();
                gate.recv_timeout(LIMIT).expect("unreleased native wait checkpoint");
            }
        }
    });
}
fn await_event(events: &mpsc::Receiver<Event>, checkpoint: Checkpoint) -> Event {
    loop {
        let event = events.recv_timeout(LIMIT).expect("missing native wait checkpoint");
        if event.checkpoint == checkpoint { return event; }
    }
}

#[derive(Default)]
struct QueueState { generation: u32, waiting: usize }
#[derive(Default)]
struct Queue { state: Mutex<QueueState>, changed: Condvar }
fn queue(key: u64) -> Arc<Queue> {
    type Queues = BTreeMap<(u16, u64), Arc<Queue>>;
    static QUEUES: OnceLock<Mutex<Queues>> = OnceLock::new();
    QUEUES.get_or_init(Mutex::default).lock().unwrap()
        .entry((SCOPE.get(), key)).or_default().clone()
}

#[unsafe(no_mangle)]
extern "Rust" fn trueos_tokio_platform_wait_observe(key: u64) -> u32 {
    let generation = queue(key).state.lock().unwrap().generation;
    event(Checkpoint::Observed, 0);
    generation
}
#[unsafe(no_mangle)]
extern "Rust" fn trueos_tokio_platform_wait_after(key: u64, observed: u32, timeout_ms: u64) -> bool {
    event(Checkpoint::EnterWait, timeout_ms);
    let queue = queue(key);
    let mut state = queue.state.lock().unwrap();
    if state.generation != observed || timeout_ms == 0 {
        return state.generation != observed;
    }
    state.waiting += 1;
    event(Checkpoint::Registered, timeout_ms);
    let duration = if timeout_ms == u64::MAX { LIMIT } else { Duration::from_millis(timeout_ms) };
    let (next, timed) = queue.changed.wait_timeout_while(state, duration,
        |state| state.generation == observed).unwrap();
    state = next;
    assert!(timeout_ms != u64::MAX || !timed.timed_out(), "lost native wake");
    state.waiting -= 1;
    state.generation != observed
}
#[unsafe(no_mangle)]
extern "Rust" fn trueos_tokio_platform_wake_one(key: u64) -> bool {
    let queue = queue(key);
    let mut state = queue.state.lock().unwrap();
    state.generation = state.generation.wrapping_add(1);
    queue.changed.notify_one();
    state.waiting != 0
}
#[unsafe(no_mangle)]
extern "Rust" fn trueos_tokio_platform_wake_all(key: u64) -> usize {
    let queue = queue(key);
    let mut state = queue.state.lock().unwrap();
    state.generation = state.generation.wrapping_add(1);
    queue.changed.notify_all();
    state.waiting
}

#[test]
fn timeout_conversion_and_atomic_mismatch() {
    for (timeout, expected) in [
        (None, u64::MAX), (Some(Duration::ZERO), 0),
        (Some(Duration::from_nanos(1)), 1),
        (Some(Duration::from_nanos(1_000_001)), 2),
        (Some(Duration::MAX), u64::MAX),
    ] {
        assert_eq!(sys::futex::timeout_for_test(timeout), expected);
    }
    let atomic = AtomicU32::new(1);
    assert!(sys::futex::futex_wait(&atomic, 0, Some(Duration::ZERO)));
    assert!(!sys::futex::futex_wait(&atomic, 1, Some(Duration::ZERO)));
    let now = Instant::now();
    assert!(!sys::futex::futex_wait(&atomic, 1, Some(Duration::from_micros(500))));
    assert!(now.elapsed() >= Duration::from_micros(500));
}

#[test]
fn wake_at_observation_and_registration_boundaries_is_preserved() {
    for checkpoint in [Checkpoint::Observed, Checkpoint::EnterWait] {
        let atomic = Arc::new(AtomicU32::new(0));
        let other = atomic.clone();
        let (events, event_rx) = mpsc::channel();
        let (gate, gate_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            PROBE.set(Some(Probe { events, pause: Some((checkpoint, gate_rx)) }));
            sys::futex::futex_wait(&other, 0, None)
        });
        await_event(&event_rx, checkpoint);
        atomic.store(1, Ordering::Relaxed);
        sys::futex::futex_wake(&atomic);
        gate.send(()).unwrap();
        assert!(worker.join().unwrap());
    }
}

struct Protected { mutex: RawMutex, value: UnsafeCell<usize> }
unsafe impl Sync for Protected {}
impl Protected {
    fn new() -> Self { Self { mutex: RawMutex::new(), value: UnsafeCell::new(0) } }
}

#[test]
fn mutex_serializes_contended_writers_and_try_lock() {
    let protected = Arc::new(Protected::new());
    assert!(protected.mutex.try_lock());
    assert!(!protected.mutex.try_lock());
    unsafe { protected.mutex.unlock() };
    std::thread::scope(|scope| {
        for _ in 0..8 {
            let protected = &protected;
            scope.spawn(move || {
                for _ in 0..4_000 {
                    protected.mutex.lock();
                    unsafe { *protected.value.get() += 1; protected.mutex.unlock(); }
                }
            });
        }
    });
    assert_eq!(unsafe { *protected.value.get() }, 32_000);
}

#[test]
fn condvar_notify_between_unlock_and_sleep_reacquires_mutex() {
    for checkpoint in [Checkpoint::Observed, Checkpoint::EnterWait] {
        let protected = Arc::new(Protected::new());
        let condvar = Arc::new(RawCondvar::new());
        let (events, event_rx) = mpsc::channel();
        let (gate, gate_rx) = mpsc::channel();
        let other = protected.clone();
        let other_cv = condvar.clone();
        let worker = std::thread::spawn(move || {
            other.mutex.lock();
            PROBE.set(Some(Probe { events, pause: Some((checkpoint, gate_rx)) }));
            while unsafe { *other.value.get() } == 0 {
                unsafe { other_cv.wait(&other.mutex) };
            }
            assert!(!other.mutex.try_lock(), "condvar failed to reacquire");
            unsafe { other.mutex.unlock() };
        });
        await_event(&event_rx, checkpoint);
        protected.mutex.lock();
        unsafe { *protected.value.get() = 1 };
        condvar.notify_one();
        unsafe { protected.mutex.unlock() };
        gate.send(()).unwrap();
        worker.join().unwrap();
    }
}

#[test]
fn condvar_timeout_reacquires_mutex_and_broadcast_releases_all() {
    let protected = Arc::new(Protected::new());
    let cv = Arc::new(RawCondvar::new());
    protected.mutex.lock();
    let now = Instant::now();
    assert!(!unsafe { cv.wait_timeout(&protected.mutex, Duration::from_micros(500)) });
    assert!(now.elapsed() >= Duration::from_micros(500));
    assert!(!protected.mutex.try_lock());
    unsafe { protected.mutex.unlock() };
    let (events, event_rx) = mpsc::channel();
    std::thread::scope(|scope| {
        for _ in 0..6 {
            let protected = &protected;
            let cv = &cv;
            let events = events.clone();
            scope.spawn(move || {
                protected.mutex.lock();
                PROBE.set(Some(Probe { events, pause: None }));
                while unsafe { *protected.value.get() } == 0 {
                    unsafe { cv.wait(&protected.mutex) };
                }
                unsafe { protected.mutex.unlock() };
            });
        }
        for _ in 0..6 { await_event(&event_rx, Checkpoint::Registered); }
        protected.mutex.lock();
        unsafe { *protected.value.get() = 1 };
        cv.notify_all();
        unsafe { protected.mutex.unlock() };
    });
}

fn parker() -> pin::Pin<Box<Parker>> {
    let raw = Box::into_raw(Box::new(std::mem::MaybeUninit::<Parker>::uninit())).cast::<Parker>();
    unsafe { Parker::new_in_place(raw); pin::Pin::new_unchecked(Box::from_raw(raw)) }
}

#[test]
fn parker_consumes_one_token_and_synchronizes_handoffs() {
    let parker = Arc::new(parker());
    parker.as_ref().as_ref().unpark();
    unsafe { parker.as_ref().as_ref().park() };
    let now = Instant::now();
    unsafe { parker.as_ref().as_ref().park_timeout(Duration::from_millis(2)) };
    assert!(now.elapsed() >= Duration::from_millis(2), "token consumed twice");
    let value = Arc::new(AtomicUsize::new(0));
    let (events, event_rx) = mpsc::channel();
    let other = parker.clone();
    let other_value = value.clone();
    let worker = std::thread::spawn(move || {
        PROBE.set(Some(Probe { events, pause: None }));
        unsafe { other.as_ref().as_ref().park() };
        assert_eq!(other_value.load(Ordering::Relaxed), 73);
    });
    await_event(&event_rx, Checkpoint::Registered);
    value.store(73, Ordering::Relaxed);
    parker.as_ref().as_ref().unpark();
    worker.join().unwrap();
}

#[test]
fn parker_timeout_unpark_races_leave_a_valid_token_state() {
    for _ in 0..100 {
        let parker = Arc::new(parker());
        let other = parker.clone();
        let worker = std::thread::spawn(move || {
            unsafe { other.as_ref().as_ref().park_timeout(Duration::from_micros(100)) };
        });
        parker.as_ref().as_ref().unpark();
        worker.join().unwrap();
        // A racing unpark may leave one token, or the previous park consumed it.
        unsafe { parker.as_ref().as_ref().park_timeout(Duration::ZERO) };
        unsafe { parker.as_ref().as_ref().park_timeout(Duration::ZERO) };
    }
}

#[test]
fn once_publishes_exactly_one_initialization_and_preserves_poison() {
    let once = sys::sync::once::Once::new();
    let calls = AtomicUsize::new(0);
    let value = AtomicUsize::new(0);
    let barrier = Barrier::new(8);
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| {
                barrier.wait();
                once.call(false, &mut |_| {
                    calls.fetch_add(1, Ordering::Relaxed);
                    std::thread::sleep(Duration::from_millis(2));
                    value.store(42, Ordering::Relaxed);
                });
                assert_eq!(value.load(Ordering::Relaxed), 42);
            });
        }
    });
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    let poisoned = sys::sync::once::Once::new();
    assert!(std::panic::catch_unwind(|| poisoned.call(false, &mut |_| panic!("initializer failed"))).is_err());
    assert!(std::panic::catch_unwind(|| poisoned.call(false, &mut |_| unreachable!())).is_err());
    poisoned.call(true, &mut |state| assert!(state.inner.is_poisoned()));
    assert!(poisoned.is_completed());
}

struct SharedRw { lock: RwLock, value: UnsafeCell<usize> }
unsafe impl Sync for SharedRw {}
#[test]
fn rwlock_readers_writers_and_downgrade_progress() {
    let state = SharedRw { lock: RwLock::new(), value: UnsafeCell::new(0) };
    std::thread::scope(|scope| {
        for _ in 0..4 {
            let state = &state;
            scope.spawn(move || {
                for _ in 0..1_000 {
                    state.lock.write();
                    unsafe { *state.value.get() += 1; state.lock.write_unlock(); }
                }
            });
            scope.spawn(move || {
                let mut previous = 0;
                for _ in 0..1_000 {
                    state.lock.read();
                    let next = unsafe { *state.value.get() };
                    assert!(next >= previous);
                    previous = next;
                    unsafe { state.lock.read_unlock() };
                }
            });
        }
    });
    assert_eq!(unsafe { *state.value.get() }, 4_000);
    state.lock.write();
    unsafe { state.lock.downgrade() };
    assert!(state.lock.try_read());
    assert!(!state.lock.try_write());
    unsafe { state.lock.read_unlock(); state.lock.read_unlock(); }
    assert!(state.lock.try_write());
    unsafe { state.lock.write_unlock() };
}

#[test]
fn identical_addresses_in_different_realms_do_not_cross_wake() {
    let atomic = Arc::new(AtomicU32::new(0));
    let other = atomic.clone();
    let (events, event_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        SCOPE.set(1);
        PROBE.set(Some(Probe { events, pause: None }));
        assert!(!sys::futex::futex_wait(&other, 0, Some(Duration::from_millis(20))));
    });
    await_event(&event_rx, Checkpoint::Registered);
    SCOPE.set(2);
    assert!(!sys::futex::futex_wake(&atomic));
    sys::futex::futex_wake_all(&atomic);
    SCOPE.set(0);
    worker.join().unwrap();
}
