//! Instrumented host implementations of the native ABI, for algorithm tests.
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    ffi::{c_char, c_void},
    sync::{
        Arc, Condvar, Mutex, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
#[derive(Default)]
struct Generation {
    value: Mutex<u32>,
    changed: Condvar,
}
static GENERATIONS: OnceLock<Mutex<HashMap<u64, Arc<Generation>>>> = OnceLock::new();
fn generation(key: u64) -> Arc<Generation> {
    GENERATIONS
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .entry(key)
        .or_default()
        .clone()
}
#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_tokio_platform_wait_observe(key: u64) -> u32 {
    *generation(key).value.lock().unwrap()
}
#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_tokio_platform_wait_after(
    key: u64,
    observed: u32,
    timeout_ms: u64,
) -> bool {
    let g = generation(key);
    let mut value = g.value.lock().unwrap();
    if *value != observed {
        return true;
    }
    if timeout_ms == u64::MAX {
        while *value == observed {
            value = g.changed.wait(value).unwrap();
        }
        true
    } else {
        let (v, _) = g
            .changed
            .wait_timeout_while(value, Duration::from_millis(timeout_ms), |v| *v == observed)
            .unwrap();
        *v != observed
    }
}
#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_tokio_platform_wake_one(key: u64) -> bool {
    let g = generation(key);
    let mut v = g.value.lock().unwrap();
    *v = v.wrapping_add(1);
    drop(v);
    g.changed.notify_one();
    true
}
#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_tokio_platform_wake_all(key: u64) -> usize {
    let g = generation(key);
    let mut v = g.value.lock().unwrap();
    *v = v.wrapping_add(1);
    drop(v);
    g.changed.notify_all();
    1
}
static ORIGIN: OnceLock<Instant> = OnceLock::new();
#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_platform_monotonic_nanos() -> u64 {
    ORIGIN.get_or_init(Instant::now).elapsed().as_nanos() as u64
}
struct Slot {
    value: usize,
    destructor: Option<unsafe extern "C" fn(*mut u8)>,
}
thread_local! {static KEYS:RefCell<HashMap<usize,Slot>>=RefCell::new(HashMap::new());static ID:Cell<usize>=const{Cell::new(0x1_0000_0000)};}
#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_platform_thread_local_get(key: usize) -> *mut u8 {
    KEYS.with(|m| m.borrow().get(&key).map_or(0, |s| s.value) as *mut u8)
}
#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_platform_thread_local_set(
    key: usize,
    value: *mut u8,
    destructor: Option<unsafe extern "C" fn(*mut u8)>,
) -> i32 {
    KEYS.with(|m| {
        if value.is_null() {
            m.borrow_mut().remove(&key);
        } else {
            m.borrow_mut().insert(
                key,
                Slot {
                    value: value.addr(),
                    destructor,
                },
            );
        }
    });
    0
}
thread_local! {
 static CLEANUPS:RefCell<Vec<unsafe extern "C" fn()>>=const{RefCell::new(Vec::new())};
 static CLEANUP_CURSOR:Cell<usize>=const{Cell::new(0)};
}
#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_platform_thread_local_register_cleanup(
    callback: unsafe extern "C" fn(),
) -> i32 {
    CLEANUPS.with(|list| {
        let mut list = list.borrow_mut();
        if !list.iter().any(|&f| f as usize == callback as usize) {
            list.push(callback);
        }
    });
    0
}
pub fn cleanup_tls() {
    // Same immutable-key order and remove-immediately-before-callback rule as
    // native_tls::State, including callbacks that replace another key's value.
    for _ in 0..4 {
        let mut keys = KEYS.with(|m| {
            m.borrow()
                .iter()
                .filter(|(_, s)| s.destructor.is_some())
                .map(|(&k, _)| k)
                .collect::<Vec<_>>()
        });
        if keys.is_empty() {
            break;
        }
        keys.sort_unstable();
        for key in keys {
            let slot = KEYS.with(|m| m.borrow_mut().remove(&key));
            if let Some(Slot {
                value,
                destructor: Some(dtor),
            }) = slot
            {
                unsafe {
                    dtor(value as *mut u8);
                }
            }
        }
    }
    loop {
        let callback = CLEANUPS.with(|list| {
            CLEANUP_CURSOR.with(|cursor| {
                let i = cursor.get();
                let callback = list.borrow().get(i).copied();
                if callback.is_some() {
                    cursor.set(i + 1);
                }
                callback
            })
        });
        let Some(callback) = callback else {
            break;
        };
        unsafe {
            callback();
        }
    }
    KEYS.with(|m| m.borrow_mut().clear());
    CLEANUPS.with(|v| v.borrow_mut().clear());
    CLEANUP_CURSOR.with(|v| v.set(0));
}

static NEXT_THREAD: AtomicUsize = AtomicUsize::new(1);
static THREADS: OnceLock<Mutex<HashMap<usize, std::thread::JoinHandle<()>>>> = OnceLock::new();
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_thread_spawn(
    _stack: usize,
    entry: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    arg: *mut c_void,
    out: *mut usize,
) -> i32 {
    let id = NEXT_THREAD.fetch_add(1, Ordering::Relaxed);
    let arg = arg.addr();
    let child = std::thread::spawn(move || {
        ID.with(|s| s.set(id));
        unsafe {
            entry(arg as *mut c_void);
        }
        cleanup_tls();
    });
    THREADS
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .insert(id, child);
    unsafe {
        out.write(id);
    }
    0
}
#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_thread_join(handle: usize) -> i32 {
    let child = THREADS
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .remove(&handle)
        .unwrap();
    child.join().unwrap();
    0
}
#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_thread_detach(handle: usize) -> i32 {
    drop(
        THREADS
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .remove(&handle),
    );
    0
}
#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_thread_current_id() -> usize {
    ID.with(Cell::get)
}
#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_thread_available_parallelism() -> usize {
    2
}
#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_thread_set_name(_name: *const c_char) -> i32 {
    0
}
#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_poll_once() {
    std::thread::yield_now();
}
#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_sleep_ms(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_env_var(
    key: *const u8,
    len: usize,
    out: *mut u8,
    cap: usize,
) -> isize {
    let key = unsafe { std::str::from_utf8(std::slice::from_raw_parts(key, len)).unwrap() };
    let Ok(value) = std::env::var(key) else {
        return -1;
    };
    if !out.is_null() && cap >= value.len() {
        unsafe {
            std::ptr::copy_nonoverlapping(value.as_ptr(), out, value.len());
        }
    }
    value.len() as isize
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_rand(out: *mut u32, len: usize) {
    static SEED: AtomicUsize = AtomicUsize::new(12345);
    for i in 0..len {
        unsafe {
            out.add(i)
                .write(SEED.fetch_add(7919, Ordering::Relaxed) as u32);
        }
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_blueprint_shutdown(_ptr: *const u8, _len: usize) -> i32 {
    std::process::abort()
}
thread_local! {static CHILD_ROLE:Cell<u32>=const{Cell::new(0)};static ACTIVITY:Cell<u32>=const{Cell::new(0)};}
#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_thread_swap_child_role_v1(role: u32) -> u32 {
    CHILD_ROLE.with(|r| r.replace(role))
}
#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_thread_swap_activity_v1(activity: u32) -> u32 {
    ACTIVITY.with(|r| r.replace(activity))
}
