#!/usr/bin/env python3
"""Run production pthread mutex, TLS-exit and deadline tests on the host.

The freestanding kernel has test=false. This harness extracts its actual pure
items and existing Rust tests; host mutexes stand in for the registry locks.
It does not claim hardware/runtime acceptance.
"""
from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import item, constant

ROOT = Path(__file__).resolve().parents[2]
SOURCE = str(ROOT / "src/std_abi_shim.rs")


def main():
    definitions = [constant(SOURCE, name) for name in (
        "PTHREAD_KEY_CAPACITY", "PTHREAD_TLS_VALUE_CAPACITY",
        "PTHREAD_DESTRUCTOR_ITERATIONS", "PTHREAD_MUTEX_SPIN_TRACE_START",
        "TRUEOS_PTHREAD_MUTEX_NORMAL", "TRUEOS_PTHREAD_MUTEX_RECURSIVE",
        "TRUEOS_PTHREAD_MUTEX_ERRORCHECK", "TRUEOS_EINVAL", "TRUEOS_EPERM",
        "TRUEOS_EAGAIN", "TRUEOS_EBUSY", "TRUEOS_EDEADLK",
        "TRUEOS_CLOCK_REALTIME", "TRUEOS_CLOCK_MONOTONIC", "TRUEOS_ETIMEDOUT",
    )]
    definitions += [item(SOURCE, name) for name in (
        "PthreadMutexStorage", "PthreadTlsSlot", "PthreadKeyState",
        "PthreadTlsValue", "PthreadTimespec", "pthread_deadline_millis", "PthreadCondStorage",
        "pthread_mutex_lock_state", "pthread_mutex_trylock_state",
        "pthread_mutex_unlock_state", "pthread_run_tls_destructors",
        "pthread_clock_nanos", "pthread_mutex_storage", "pthread_cond_storage",
        "pthread_mutex_lock_key", "pthread_mutex_unlock_key", "pthread_cond_generation",
        "pthread_cond_notify_key", "pthread_cond_wait_key",
        "pthread_mutex_tests", "pthread_thread_semantics_tests",
    )]
    support = f'''#![allow(dead_code)]
extern crate alloc;
use alloc::vec::Vec;
use alloc::sync::Arc;
use core::ffi::{{c_int, c_void}};
use core::ptr;
use core::sync::atomic::{{AtomicI32, AtomicU64, AtomicUsize, Ordering}};
#[path="{ROOT / 'src/r/static_map.rs'}"] mod static_map;
use static_map::FixedKeyMap;
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {{
    const fn new(value: T) -> Self {{ Self(std::sync::Mutex::new(value)) }}
    fn lock(&self) -> std::sync::MutexGuard<'_, T> {{ self.0.lock().unwrap() }}
}}
static PTHREAD_KEYS: Mutex<FixedKeyMap<usize, PthreadKeyState, PTHREAD_KEY_CAPACITY>> = Mutex::new(FixedKeyMap::new());
static PTHREAD_TLS_VALUES: Mutex<FixedKeyMap<PthreadTlsSlot, PthreadTlsValue, PTHREAD_TLS_VALUE_CAPACITY>> = Mutex::new(FixedKeyMap::new());
#[macro_export] macro_rules! log_warn {{ ($($tokens:tt)*) => {{}} }}
mod wait {{ pub fn spin_step() {{ std::thread::yield_now(); }} }}
mod percpu {{ pub fn current_slot() -> usize {{ 0 }} }}
fn pthread_object_host_ptr(ptr: *mut u8, _len: usize) -> Option<*mut u8> {{
    (!ptr.is_null()).then_some(ptr)
}}
fn pthread_sync_probe_log() {{}}
fn pthread_sync_trace(_operation: &str, _key: usize) {{}}
fn pthread_current_id() -> usize {{
    static NEXT: AtomicUsize = AtomicUsize::new(1);
    std::thread_local! {{ static ID: usize = NEXT.fetch_add(1, Ordering::Relaxed); }}
    ID.with(|id| *id)
}}
fn trueos_cabi_boot_timestamp_secs() -> u64 {{ 10 }}
mod embassy_time_driver {{
    pub const TICK_HZ: u64 = 1000;
    pub fn now() -> u64 {{
        static START: std::sync::LazyLock<std::time::Instant> = std::sync::LazyLock::new(std::time::Instant::now);
        START.elapsed().as_millis() as u64
    }}
}}
mod r {{ pub mod platform {{
    use std::sync::{{Arc, Mutex, Condvar, LazyLock}};
    use std::collections::HashMap;
    struct Queue {{ generation: Mutex<u32>, wait: Condvar }}
    static QUEUES: LazyLock<Mutex<HashMap<u64, Arc<Queue>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
    fn queue(key: u64) -> Arc<Queue> {{
        QUEUES.lock().unwrap().entry(key).or_insert_with(|| Arc::new(Queue {{
            generation: Mutex::new(0), wait: Condvar::new(),
        }})).clone()
    }}
    pub fn trueos_tokio_platform_wait_observe(key: u64) -> u32 {{ *queue(key).generation.lock().unwrap() }}
    pub fn trueos_tokio_platform_wait_after(key: u64, observed: u32, millis: u64) -> bool {{
        let queue = queue(key);
        let mut generation = queue.generation.lock().unwrap();
        while *generation == observed {{
            if millis == u64::MAX {{ generation = queue.wait.wait(generation).unwrap(); }}
            else {{
                let (guard, timeout) = queue.wait.wait_timeout(generation, std::time::Duration::from_millis(millis)).unwrap();
                generation = guard;
                if timeout.timed_out() {{ break; }}
            }}
        }}
        *generation != observed
    }}
    pub fn trueos_tokio_platform_wake_one(key: u64) -> bool {{
        let queue = queue(key); *queue.generation.lock().unwrap() += 1; queue.wait.notify_one(); true
    }}
    pub fn trueos_tokio_platform_wake_all(key: u64) -> usize {{
        let queue = queue(key); *queue.generation.lock().unwrap() += 1; queue.wait.notify_all(); 1
    }}
}} }}
'''
    with tempfile.TemporaryDirectory(prefix="trueos-pthread-semantics-") as directory:
        folder = Path(directory)
        (folder / "src").mkdir()
        (folder / "Cargo.toml").write_text(
            '[package]\nname="pthread-semantics-tests"\nversion="0.1.0"\n'
            'edition="2024"\n[workspace]\n'
        )
        (folder / "src/lib.rs").write_text(support + "\n".join(definitions))
        subprocess.run([
            "cargo", "test", "--offline", "--target", "x86_64-unknown-linux-gnu",
            "--target-dir", str(ROOT / "bld/pthread-semantics-host-tests"),
        ], cwd=folder, check=True)


if __name__ == "__main__":
    main()
