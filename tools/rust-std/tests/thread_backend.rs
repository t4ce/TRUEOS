//! Exercise the unmodified std sys backend against a native-thread CABI fixture.
#![allow(dead_code)]

pub use std::{cmp, ffi, mem, num, ptr, time};

mod io {
    // std's private constant is unavailable outside its own crate. Keep only
    // the public errno behavior and that one constant in this fixture.
    #[derive(Debug, PartialEq)]
    pub struct Error(Option<i32>);
    pub type Result<T> = std::result::Result<T, Error>;
    impl Error {
        pub const UNKNOWN_THREAD_COUNT: Self = Self(None);
        pub fn from_raw_os_error(error: i32) -> Self {
            Self(Some(error))
        }
        pub fn raw_os_error(&self) -> Option<i32> {
            self.0
        }
    }
    impl std::fmt::Display for Error {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{:?}", self.0)
        }
    }
}

mod thread {
    pub struct ThreadInit(pub Box<dyn FnOnce() + Send>);
    impl ThreadInit {
        pub fn init(self: Box<Self>) -> Box<dyn FnOnce() + Send> {
            super::INITIALIZED.set(true);
            self.0
        }
    }
}

mod backend {
    include!(env!("TRUEOS_STD_THREAD_BACKEND"));
}

use std::collections::HashMap;
use std::ffi::{CStr, c_char, c_int, c_void};
use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::Duration;

std::thread_local! {
    static CURRENT: std::cell::Cell<usize> = const { std::cell::Cell::new(7) };
    static INITIALIZED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static EXIT_MARKER: DropMarker = DropMarker(Arc::new(AtomicUsize::new(0)));
}

static THREADS: OnceLock<Mutex<HashMap<usize, std::thread::JoinHandle<()>>>> = OnceLock::new();
static NEXT_HANDLE: AtomicUsize = AtomicUsize::new(1);
static NEXT_ERROR: AtomicI32 = AtomicI32::new(0);
static LAST_STACK: AtomicUsize = AtomicUsize::new(0);
static JOINED: AtomicUsize = AtomicUsize::new(0);
static DETACHED: AtomicUsize = AtomicUsize::new(0);
static CAPACITY: AtomicUsize = AtomicUsize::new(2);
static YIELDED: AtomicUsize = AtomicUsize::new(0);
static LAST_NAME: OnceLock<Mutex<String>> = OnceLock::new();

fn threads() -> &'static Mutex<HashMap<usize, std::thread::JoinHandle<()>>> {
    THREADS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn trueos_cabi_thread_spawn(
    stack: usize,
    start: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    data: *mut c_void,
    out: *mut usize,
) -> c_int {
    LAST_STACK.store(stack, Ordering::SeqCst);
    let error = NEXT_ERROR.swap(0, Ordering::SeqCst);
    if error != 0 {
        return error;
    }
    let handle = NEXT_HANDLE.fetch_add(1, Ordering::SeqCst);
    let data = data as usize;
    let worker = std::thread::Builder::new()
        .stack_size(stack.max(64 * 1024))
        .spawn(move || {
            CURRENT.set(handle);
            unsafe { start(data as *mut c_void) };
            // The host std exit path now runs the real TLS destructors before join.
        })
        .unwrap();
    threads().lock().unwrap().insert(handle, worker);
    unsafe { *out = handle };
    0
}

#[unsafe(no_mangle)]
extern "C" fn trueos_cabi_thread_join(handle: usize) -> c_int {
    let worker = threads().lock().unwrap().remove(&handle).unwrap();
    worker.join().unwrap();
    JOINED.fetch_add(1, Ordering::SeqCst);
    0
}

#[unsafe(no_mangle)]
extern "C" fn trueos_cabi_thread_detach(handle: usize) -> c_int {
    let worker = threads().lock().unwrap().remove(&handle).unwrap();
    drop(worker);
    DETACHED.fetch_add(1, Ordering::SeqCst);
    0
}

#[unsafe(no_mangle)]
extern "C" fn trueos_cabi_thread_available_parallelism() -> usize {
    CAPACITY.load(Ordering::SeqCst)
}

#[unsafe(no_mangle)]
extern "C" fn trueos_cabi_thread_current_id() -> usize {
    CURRENT.get()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn trueos_cabi_thread_set_name(name: *const c_char) -> c_int {
    *LAST_NAME
        .get_or_init(|| Mutex::new(String::new()))
        .lock()
        .unwrap() = unsafe { CStr::from_ptr(name) }.to_str().unwrap().to_owned();
    0
}

#[unsafe(no_mangle)]
extern "C" fn trueos_cabi_poll_once() {
    YIELDED.fetch_add(1, Ordering::SeqCst);
}

#[unsafe(no_mangle)]
extern "C" fn trueos_cabi_sleep_ms(_millis: u64) {}

struct DropMarker(Arc<AtomicUsize>);
impl Drop for DropMarker {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn failed_spawn_reclaims_entry_and_preserves_errno() {
    let dropped = Arc::new(AtomicUsize::new(0));
    let marker = DropMarker(dropped.clone());
    NEXT_ERROR.store(11, Ordering::SeqCst);
    let init = Box::new(thread::ThreadInit(Box::new(move || {
        drop(marker);
        panic!("rejected entry must never run");
    })));
    let error = unsafe { backend::Thread::new(123_456, init) }
        .err()
        .unwrap();
    assert_eq!(error.raw_os_error(), Some(11));
    assert_eq!(LAST_STACK.load(Ordering::SeqCst), 123_456);
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
}

#[test]
fn successful_spawn_initializes_and_join_waits_for_exit() {
    let joined = JOINED.load(Ordering::SeqCst);
    let detached = DETACHED.load(Ordering::SeqCst);
    let dropped = Arc::new(AtomicUsize::new(0));
    let entry_marker = DropMarker(dropped.clone());
    let (sent, received) = mpsc::channel();
    let init = Box::new(thread::ThreadInit(Box::new(move || {
        assert!(INITIALIZED.get());
        let exit_dropped = EXIT_MARKER.with(|marker| marker.0.clone());
        sent.send((backend::current_os_id().unwrap(), exit_dropped))
            .unwrap();
        drop(entry_marker);
    })));
    let worker = unsafe { backend::Thread::new(backend::DEFAULT_MIN_STACK_SIZE, init) }.unwrap();
    let (id, exit_dropped) = received.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_ne!(id, backend::current_os_id().unwrap());
    worker.join();
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert_eq!(exit_dropped.load(Ordering::SeqCst), 1);
    assert_eq!(JOINED.load(Ordering::SeqCst), joined + 1);
    assert_eq!(DETACHED.load(Ordering::SeqCst), detached);
}

#[test]
fn dropped_handle_detaches_without_stopping_or_waiting_for_entry() {
    let detached = DETACHED.load(Ordering::SeqCst);
    let (release, pending) = mpsc::channel();
    let (finished, completion) = mpsc::channel();
    let init = Box::new(thread::ThreadInit(Box::new(move || {
        pending.recv().unwrap();
        finished.send(()).unwrap();
    })));
    let worker = unsafe { backend::Thread::new(64 * 1024, init) }.unwrap();
    drop(worker);
    assert_eq!(DETACHED.load(Ordering::SeqCst), detached + 1);
    release.send(()).unwrap();
    completion.recv_timeout(Duration::from_secs(5)).unwrap();
}

#[test]
fn runtime_queries_name_and_yield_use_platform_values() {
    CAPACITY.store(3, Ordering::SeqCst);
    assert_eq!(backend::available_parallelism().unwrap().get(), 3);
    CAPACITY.store(0, Ordering::SeqCst);
    assert!(backend::available_parallelism().is_err());
    CAPACITY.store(2, Ordering::SeqCst);
    CURRENT.set(55);
    assert_eq!(backend::current_os_id(), Some(55));
    CURRENT.set(0);
    assert_eq!(backend::current_os_id(), None);
    CURRENT.set(7);
    backend::set_name(c"tokio-worker");
    assert_eq!(&*LAST_NAME.get().unwrap().lock().unwrap(), "tokio-worker");
    let yielded = YIELDED.load(Ordering::SeqCst);
    backend::yield_now();
    assert_eq!(YIELDED.load(Ordering::SeqCst), yielded + 1);
}
