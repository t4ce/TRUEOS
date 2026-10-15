#!/usr/bin/env python3
"""Compile production lock registry, flock, handle drop and descriptor removal.

Mock only realm/mount lookup, VMX dispatch and sleeping. Exercise contention
across realms and independent opens, shared locks, blocking waits, duplicated
descriptors, explicit unlock and forced realm teardown.
"""
from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import item
from test_posix_renameat import function

ROOT = Path(__file__).resolve().parents[2]


def main():
    shim = ROOT / "src/std_abi_shim.rs"
    source = shim.read_text()
    handle = source[source.index("pub(crate) struct FileLockHandle {"):
                    source.index("pub(crate) struct RegularFileWorld {")]
    registry = (ROOT / "src/r/fs/advisory_lock.rs").read_text()
    registry = "\n".join(line for line in registry.splitlines() if not line.startswith("//!"))
    registry = registry.replace("use spin::Mutex;", "use crate::Mutex;")
    definitions = "\n".join(function(shim, name) for name in (
        "file_lock_realm", "file_lock_try_host", "file_lock_release_host", "flock",
        "remove_open_file_for_close",
    ))
    harness = r'''
#![allow(dead_code)]
extern crate alloc;
#[path = "@MAP@"] mod static_map;
use static_map::FixedKeyMap;
use std::ffi::c_int;
use std::sync::{Arc, Mutex as StdMutex};
use std::sync::atomic::{AtomicUsize, Ordering};
struct Mutex<T>(StdMutex<T>);
impl<T> Mutex<T> {
    const fn new(value:T)->Self { Self(StdMutex::new(value)) }
    fn lock(&self)->std::sync::MutexGuard<'_,T> { self.0.lock().unwrap() }
}
std::thread_local! {
    static REALM:std::cell::Cell<Option<u8>>=const { std::cell::Cell::new(None) };
    static HULL:std::cell::Cell<bool>=const { std::cell::Cell::new(false) };
    static ERRNO:std::cell::Cell<i32>=const { std::cell::Cell::new(0) };
}
struct ProcessErrno;
impl ProcessErrno {
    fn store(&self, value:i32, _:Ordering) { ERRNO.with(|e| e.set(value)); }
    fn load(&self, _:Ordering)->i32 { ERRNO.with(|e| e.get()) }
}
static TRUEOS_ERRNO:ProcessErrno=ProcessErrno;
const TRUEOS_EINVAL:i32=22;
const TRUEOS_EBADF:i32=9;
const TRUEOS_EAGAIN:i32=11;
const TRUEOS_EIO:i32=5;
static SLEEPS:AtomicUsize=AtomicUsize::new(0);
static VMCALLS:AtomicUsize=AtomicUsize::new(0);
mod allocators {
    pub fn with_host_alloc_domain_strong<T>(f:impl FnOnce()->T)->T { f() }
}
mod hv {
    pub fn current_guest_execution_context_vm_id()->Option<u8> { crate::REALM.with(|r| r.get()) }
    pub fn current_hull_guest_context_vm_id()->Option<u8> {
        if crate::HULL.with(|r| r.get()) { current_guest_execution_context_vm_id() } else { None }
    }
}
mod r {
    pub mod threads { pub fn current_vm_id()->Option<u8> { crate::REALM.with(|r| r.get()) } }
    pub mod io { pub mod kfs { #[derive(Clone)] pub struct FsStat; } pub mod fs_cabi {
        pub fn trueos_cabi_sleep_ms(ms:u64) {
            crate::SLEEPS.fetch_add(1,std::sync::atomic::Ordering::Relaxed);
            std::thread::sleep(std::time::Duration::from_millis(ms));
        }
    } }
    pub mod fs {
        pub mod volatile_files { pub fn selected_disk(_: &str)->Option<super::trueosfs::Disk> { super::trueosfs::primary_root_handle() } }
        pub mod advisory_lock { @REGISTRY@ }
        pub mod trueosfs {
            pub struct Disk;
            pub struct Id;
            impl Disk { pub fn id(&self)->Id { Id } }
            impl Id { pub fn raw(&self)->u32 { 2 } }
            pub fn primary_root_handle()->Option<Disk> { Some(Disk) }
        }
    }
}
mod trueos_vm { pub mod vmcall {
    pub const OP_BP_POSIX_FLOCK_TRY:u32=0x224;
    pub const OP_BP_POSIX_FLOCK_RELEASE:u32=0x225;
    pub const STATUS_OK:u32=0;
    pub fn call(op:u32, a:u64, b:u64)->(u32,u64) {
        crate::VMCALLS.fetch_add(1,std::sync::atomic::Ordering::Relaxed);
        match op {
            OP_BP_POSIX_FLOCK_TRY => (0,crate::file_lock_try_host(a as i32,b as i32) as u64),
            OP_BP_POSIX_FLOCK_RELEASE => { crate::file_lock_release_host(a); (0,0) }
            _ => panic!("unexpected vmcall"),
        }
    }
} }
#[derive(Default)] struct BytePipe;
struct RegularFileWorld;
@OPENFILE@
@HANDLE@
static OPEN_FILES:Mutex<FixedKeyMap<i32,OpenFile,32>>=Mutex::new(FixedKeyMap::new());
@FUNCTIONS@
fn setup() {
    HULL.with(|r| r.set(false)); REALM.with(|r| r.set(None));
    OPEN_FILES.lock().clear();
    *r::fs::advisory_lock::LOCKS.lock()=r::fs::advisory_lock::Registry::new();
    SLEEPS.store(0,Ordering::Relaxed); VMCALLS.store(0,Ordering::Relaxed);
}
fn open_fd(fd:i32, realm:u64, description:u64, path:&str) {
    assert!(OPEN_FILES.lock().insert(fd,OpenFile::Regular {
        path:Some(path.into()), world:Arc::new(Mutex::new(RegularFileWorld)),
        lock:Arc::new(FileLockHandle { owner:r::fs::advisory_lock::Owner { realm,description },path:path.into() }),
        offset:0, readable:true,writable:true,flags:0,
    }).is_ok());
}
fn call(fd:i32, operation:i32)->i32 { unsafe { flock(fd,operation) } }
fn close_fd(fd:i32) { drop(remove_open_file_for_close(fd)); }
#[test] fn independent_opens_and_realms_contend() {
    setup(); open_fd(3,1,3,"home/lock"); open_fd(4,2,3,"home/lock");
    assert_eq!(call(3,2),0); assert_eq!(call(4,6),-1);
    assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),11);
    assert_eq!(SLEEPS.load(Ordering::Relaxed),0,"nonblocking cannot sleep");
    assert_eq!(call(3,8),0); assert_eq!(call(4,6),0);
}
#[test] fn shared_locks_and_failed_exclusive_conversion() {
    setup(); open_fd(3,1,3,"same"); open_fd(4,1,4,"same"); open_fd(5,1,5,"same");
    assert_eq!(call(3,1),0); assert_eq!(call(4,5),0); assert_eq!(call(5,6),-1);
    assert_eq!(call(3,6),-1); assert_eq!(call(4,8),0);
    assert_eq!(call(3,6),0); assert_eq!(call(3,1),0); assert_eq!(call(5,5),0);
}
#[test] fn duplicate_keeps_ownership_until_final_descriptor_closes() {
    setup(); open_fd(3,1,3,"same"); open_fd(5,2,3,"same");
    let alias=OPEN_FILES.lock().get(3).unwrap().clone(); assert!(OPEN_FILES.lock().insert(4,alias).is_ok());
    assert_eq!(call(3,2),0); close_fd(3); assert_eq!(call(5,6),-1);
    assert_eq!(call(4,2),0); close_fd(4); assert_eq!(call(5,6),0);
}
#[test] fn blocking_wait_sleeps_and_acquires_after_unlock() {
    setup(); open_fd(3,1,3,"same"); open_fd(4,2,3,"same"); assert_eq!(call(3,2),0);
    let waiter=std::thread::spawn(|| call(4,2));
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(2);
    while SLEEPS.load(Ordering::Relaxed)==0 {
        assert!(std::time::Instant::now()<deadline); std::thread::yield_now();
    }
    assert_eq!(call(3,8),0); assert_eq!(waiter.join().unwrap(),0);
}
#[test] fn hull_uses_host_registry_and_final_drop_releases_it() {
    setup(); REALM.with(|r| r.set(Some(0))); HULL.with(|r| r.set(true));
    open_fd(3,1,3,"same"); open_fd(4,2,3,"same");
    assert_eq!(call(3,2),0); assert_eq!(call(4,6),-1); close_fd(3);
    assert_eq!(call(4,6),0); assert!(VMCALLS.load(Ordering::Relaxed)>=4);
}
#[test] fn teardown_releases_only_the_stopped_realm() {
    setup(); open_fd(3,1,3,"a"); open_fd(4,2,3,"b"); open_fd(5,3,3,"a"); open_fd(6,3,4,"b");
    assert_eq!(call(3,2),0); assert_eq!(call(4,2),0);
    r::fs::advisory_lock::LOCKS.lock().release_realm(1);
    assert_eq!(call(5,6),0); assert_eq!(call(6,6),-1);
}
#[test] fn bad_descriptors_and_modes_fail() {
    setup(); open_fd(3,1,3,"same");
    assert_eq!(call(99,2),-1); assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),9);
    for operation in [0,3,16,-1] {
        assert_eq!(call(3,operation),-1); assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),22);
    }
}
#[test] fn root_and_path_namespaces_are_independent() {
    let mut registry=r::fs::advisory_lock::Registry::new();
    let a=r::fs::advisory_lock::Owner {realm:1,description:3};
    let b=r::fs::advisory_lock::Owner {realm:2,description:3};
    assert!(registry.try_set(2,"same",a,Some(true)));
    assert!(registry.try_set(3,"same",b,Some(true)));
    assert!(registry.try_set(2,"other",b,Some(true)));
    assert!(!registry.try_set(2,"same",b,Some(true)));
}
'''
    harness = harness.replace("@REGISTRY@", registry).replace("@HANDLE@", handle)
    harness = harness.replace("@FUNCTIONS@", definitions).replace("@OPENFILE@", item(str(shim), "OpenFile"))
    harness = harness.replace("@MAP@", str(ROOT / "src/r/static_map.rs"))
    with tempfile.TemporaryDirectory(prefix="trueos-flock-") as directory:
        rust = Path(directory) / "test.rs"
        binary = Path(directory) / "test"
        rust.write_text(harness)
        subprocess.run(["rustc", "--edition=2024", "--test", "--target",
                        "x86_64-unknown-linux-gnu", str(rust), "-o", str(binary)], check=True)
        subprocess.run([str(binary), "--test-threads=1"], check=True, timeout=10)


if __name__ == "__main__":
    main()
