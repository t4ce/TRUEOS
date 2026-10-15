#!/usr/bin/env python3
"""Exercise actual pthread admission and diagnostics with a bounded host scheduler.

The kernel cannot be linked into a normal host test. Extract the production ABI
functions and trace formatter, preserving the real fixed join registry; only
carrier submission, address translation and log delivery are host fixtures.
"""
from pathlib import Path
import re
import subprocess
import tempfile

from test_clip_position3_uv_texture import constant, item

ROOT = Path(__file__).resolve().parents[2]
SOURCE = str(ROOT / "src/std_abi_shim.rs")


def abi_function(source, name):
    match = re.search(
        rf'^pub unsafe extern "C" fn {name}\b.*?^}}',
        source,
        re.MULTILINE | re.DOTALL,
    )
    if match is None:
        raise ValueError(f"missing production ABI function {name}")
    # Do not interpose the host test runner's own libc pthread_create.
    return match.group(0).replace("fn pthread_create(", "fn kernel_pthread_create(")


def main():
    source = Path(SOURCE).read_text()
    definitions = [constant(SOURCE, name) for name in (
        "PTHREAD_THREAD_CAPACITY", "PTHREAD_CREATE_TRACE_LIMIT",
        "TRUEOS_EINVAL", "TRUEOS_EAGAIN",
    )]
    definitions += [item(SOURCE, "pthread_create_trace")]
    definitions += [abi_function(source, name) for name in (
        "pthread_create", "trueos_cabi_thread_spawn",
    )]
    support = f'''#![allow(dead_code)]
extern crate alloc;
use alloc::boxed::Box;
use alloc::sync::Arc;
use core::ffi::{{c_int, c_void}};
use core::sync::atomic::{{AtomicI32, AtomicUsize, Ordering}};
#[path="{ROOT / 'src/r/static_map.rs'}"] mod static_map;
use static_map::FixedKeyMap;
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {{
    const fn new(value: T) -> Self {{ Self(std::sync::Mutex::new(value)) }}
    fn lock(&self) -> std::sync::MutexGuard<'_, T> {{ self.0.lock().unwrap() }}
}}
const PTHREAD_THREAD_CARRIER_TAG: usize = 1 << 31;
const PTHREAD_THREAD_SEQUENCE_MASK: usize = PTHREAD_THREAD_CARRIER_TAG - 1;
static PTHREAD_CREATE_TRACE_COUNT: AtomicUsize = AtomicUsize::new(0);
static NEXT_ID: AtomicUsize = AtomicUsize::new(1);
static PTHREAD_THREADS: Mutex<FixedKeyMap<usize, PthreadThreadState, PTHREAD_THREAD_CAPACITY>> = Mutex::new(FixedKeyMap::new());
static LOGS: Mutex<Vec<String>> = Mutex::new(Vec::new());
struct PthreadThreadState {{ completion: Arc<wait::CompletionCell>, joining: bool }}
fn pthread_current_id() -> usize {{ 9001 }}
fn pthread_next_thread_id() -> usize {{ NEXT_ID.fetch_add(1, Ordering::Relaxed) | PTHREAD_THREAD_CARRIER_TAG }}
fn pthread_run_tls_destructors(_: usize) {{}}
fn pthread_join_wait_key(thread: usize) -> u64 {{ thread as u64 }}
fn abi_read_struct<T: Copy>(ptr: *const T) -> Option<T> {{
    ((ptr as usize) > 1).then(|| unsafe {{ ptr.read_unaligned() }})
}}
fn abi_write_bytes(ptr: *mut u8, len: usize) -> Option<&'static mut [u8]> {{
    ((ptr as usize) > 1).then(|| unsafe {{ core::slice::from_raw_parts_mut(ptr, len) }})
}}
mod wait {{
    pub struct CompletionCell;
    impl CompletionCell {{
        pub fn new() -> Self {{ Self }}
        pub fn complete(&self, _: usize) -> bool {{ true }}
    }}
}}
mod hv {{ pub fn current_guest_execution_context_vm_id() -> Option<u8> {{ Some(7) }} }}
mod r {{
    pub mod threads {{
        use super::super::*;
        pub static CALLS: AtomicUsize = AtomicUsize::new(0);
        pub static RC: AtomicI32 = AtomicI32::new(0);
        pub fn current_vm_id() -> Option<u8> {{ Some(7) }}
        pub fn spawn(_: usize, job: Box<dyn FnOnce() + Send>, _: Option<u8>, _: usize) -> Result<(), i32> {{
            CALLS.fetch_add(1, Ordering::Relaxed);
            drop(job);
            match RC.load(Ordering::Relaxed) {{ 0 => Ok(()), rc => Err(rc) }}
        }}
    }}
    pub mod platform {{ pub fn trueos_tokio_platform_wake_all(_: u64) {{}} }}
}}
mod log_os_core {{ pub enum LogLevel {{ Info }} }}
mod log_os {{
    pub mod flags {{ pub enum LogArea {{ Blueprint }} }}
    pub fn log_with_area_purpose(_: flags::LogArea, _: crate::log_os_core::LogLevel, _: Option<&str>, args: core::fmt::Arguments<'_>) {{
        crate::LOGS.lock().push(args.to_string());
    }}
}}
unsafe extern "C" fn entry(_: *mut c_void) -> *mut c_void {{ panic!("fixture must not execute a guest closure") }}
fn reset() {{
    PTHREAD_THREADS.lock().clear();
    LOGS.lock().clear();
    PTHREAD_CREATE_TRACE_COUNT.store(0, Ordering::Relaxed);
    NEXT_ID.store(1, Ordering::Relaxed);
    r::threads::CALLS.store(0, Ordering::Relaxed);
    r::threads::RC.store(0, Ordering::Relaxed);
}}
#[test] fn exhausted_join_registry_is_logged_after_success_sample_ends() {{
    reset();
    for _ in 0..PTHREAD_THREAD_CAPACITY {{
        let mut id = 0;
        assert_eq!(unsafe {{ trueos_cabi_thread_spawn(2_097_152, entry, core::ptr::null_mut(), &mut id) }}, 0);
    }}
    assert_eq!(LOGS.lock().len(), PTHREAD_CREATE_TRACE_LIMIT);
    let mut id = 0;
    assert_eq!(unsafe {{ trueos_cabi_thread_spawn(2_097_152, entry, core::ptr::null_mut(), &mut id) }}, TRUEOS_EAGAIN);
    assert_eq!(id, 0);
    assert_eq!(r::threads::CALLS.load(Ordering::Relaxed), PTHREAD_THREAD_CAPACITY);
    assert_eq!(PTHREAD_THREADS.lock().values().count(), PTHREAD_THREAD_CAPACITY);
    let logs = LOGS.lock();
    assert_eq!(logs.len(), PTHREAD_CREATE_TRACE_LIMIT + 1);
    let last = logs.last().unwrap();
    for field in ["seq=64", "rc=11", "reason=join_registry_full", "stack_bytes=2097152", "join_live=64", "join_capacity=64"] {{ assert!(last.contains(field), "{{last}}"); }}
}}
#[test] fn scheduler_rejection_rolls_back_registry_and_is_always_logged() {{
    reset();
    PTHREAD_CREATE_TRACE_COUNT.store(1_000, Ordering::Relaxed);
    r::threads::RC.store(11, Ordering::Relaxed);
    let mut id = 0;
    assert_eq!(unsafe {{ trueos_cabi_thread_spawn(65_536, entry, core::ptr::null_mut(), &mut id) }}, TRUEOS_EAGAIN);
    assert_ne!(id, 0);
    assert_eq!(PTHREAD_THREADS.lock().values().count(), 0);
    let logs = LOGS.lock();
    assert_eq!(logs.len(), 1);
    assert!(logs[0].contains("reason=scheduler_rejected stack_bytes=65536 join_live=0"));
}}
#[test] fn invalid_output_rolls_back_before_reporting() {{
    reset();
    assert_eq!(unsafe {{ trueos_cabi_thread_spawn(65_536, entry, core::ptr::null_mut(), 1usize as *mut usize) }}, TRUEOS_EINVAL);
    assert_eq!(PTHREAD_THREADS.lock().values().count(), 0);
    assert_eq!(r::threads::CALLS.load(Ordering::Relaxed), 0);
    assert!(LOGS.lock()[0].contains("reason=invalid_thread_output"));
}}
#[test] fn invalid_entry_and_attributes_report_without_dereferencing_them() {{
    reset();
    let mut id = 0;
    assert_eq!(unsafe {{ kernel_pthread_create(&mut id, core::ptr::null(), core::ptr::null(), core::ptr::null_mut()) }}, TRUEOS_EINVAL);
    assert_eq!(unsafe {{ kernel_pthread_create(&mut id, 1usize as *const c_void, entry as *const c_void, core::ptr::null_mut()) }}, TRUEOS_EINVAL);
    assert_eq!(r::threads::CALLS.load(Ordering::Relaxed), 0);
    let logs = LOGS.lock();
    assert_eq!(logs.len(), 2);
    assert!(logs[0].contains("reason=invalid_entry"));
    assert!(logs[1].contains("reason=invalid_attributes"));
}}
'''
    with tempfile.TemporaryDirectory(prefix="trueos-pthread-spawn-") as directory:
        path = Path(directory) / "harness.rs"
        binary = Path(directory) / "harness"
        path.write_text(support + "\n".join(definitions))
        subprocess.run(["rustc", "--edition=2024", "--test", str(path), "-o", str(binary)], check=True)
        subprocess.run([str(binary), "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
