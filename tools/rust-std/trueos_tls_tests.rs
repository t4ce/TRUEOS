//! Production native storage, std key adapter, std Storage and cleanup guard.
//! Only kernel context accessors and spin::Mutex's host binding are modeled.
#![feature(decl_macro, rustc_attrs, allow_internal_unstable, allow_internal_unsafe, thread_local_internals)]
#![allow(dead_code, internal_features, unused_macros)]

pub use std::{alloc, cell, marker, mem, ops, panic, ptr};
macro_rules! rtassert { ($condition:expr) => { assert!($condition) }; }

// @PRODUCTION_MODULES@

use std::cell::Cell;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::{AtomicUsize, Ordering};
use sys::thread_local::Storage;

pub struct SpinMutex<T>(std::sync::Mutex<T>);
impl<T> SpinMutex<T> {
    pub const fn new(value: T) -> Self { Self(std::sync::Mutex::new(value)) }
    pub fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
}
thread_local! {
    static EXECUTION: Cell<usize> = const { Cell::new(1) };
    static REALM: Cell<usize> = const { Cell::new(0) };
    static FORCED_DOMAIN: Cell<Option<u8>> = const { Cell::new(None) };
    static GUEST_ARENA_AVAILABLE: Cell<bool> = const { Cell::new(true) };
}
static FORCED_MUTATIONS: AtomicUsize = AtomicUsize::new(0);
mod allocators {
    pub fn with_hv_guest_alloc_domain<R>(vm_id: u8, f: impl FnOnce() -> R) -> Option<R> {
        if !crate::GUEST_ARENA_AVAILABLE.get() { return None; }
        assert_eq!(Some(vm_id), crate::REALM.get().checked_sub(1).map(|v| v as u8));
        let previous = crate::FORCED_DOMAIN.replace(Some(vm_id));
        crate::FORCED_MUTATIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let result = f();
        crate::FORCED_DOMAIN.set(previous);
        Some(result)
    }
}
struct NativeProcessState { tls: r::native_tls::State }
static STATES: [NativeProcessState; 3] = [const { NativeProcessState { tls: r::native_tls::State::new() } }; 3];
mod native_process {
    pub(crate) fn current() -> &'static crate::NativeProcessState { &crate::STATES[crate::REALM.get()] }
    pub(crate) fn with_current<R>(f: impl FnOnce(&crate::NativeProcessState) -> R) -> Result<R, i32> {
        match crate::REALM.get().checked_sub(1).map(|v| v as u8) {
            Some(vm) => crate::allocators::with_hv_guest_alloc_domain(vm, || f(current())).ok_or(12),
            None => Ok(f(current())),
        }
    }
}
mod hv {
    pub fn current_hull_guest_context_vm_id() -> Option<u8> { None }
    pub fn current_guest_execution_context_vm_id() -> Option<u8> { None }
}
mod wls {
    pub const WORKER_SLOT_COUNT: usize = 8;
    pub fn current_blueprint_thread_id() -> Option<u32> { None }
    pub fn current_worker_id() -> Option<usize> { None }
}
mod percpu { pub fn current_slot_via_cpuid() -> usize { 0 } }
mod rt {
    pub fn thread_cleanup() {
        // std must still be able to access its destructor-free current-thread
        // pointer when the deferred cleanup callback eventually runs.
        let pointer = crate::CURRENT.get();
        assert_eq!(pointer as usize, crate::EXECUTION.get() + 0x1000);
        crate::CURRENT.set(std::ptr::null_mut());
        crate::CLEANUPS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}
static CLEANUPS: AtomicUsize = AtomicUsize::new(0);
static DROPS: AtomicUsize = AtomicUsize::new(0);
static NESTED_DROPS: AtomicUsize = AtomicUsize::new(0);
static REINSTALLS: AtomicUsize = AtomicUsize::new(0);
static CURRENT: sys::thread_local::LocalPointer = sys::thread_local::LocalPointer::__new();

fn prepare(realm: usize, execution: usize) {
    REALM.set(realm);
    EXECUTION.set(execution);
    CURRENT.set((execution + 0x1000) as *mut ());
    sys::thread_local::guard::enable();
}
fn finish() {
    run_execution_tls_destructors(r::native_tls::current_identity());
}

struct Tracked(usize);
impl Drop for Tracked {
    fn drop(&mut self) { DROPS.fetch_add(1, Ordering::Relaxed); }
}
static TRACKED: Storage<Tracked, { sys::thread_local::value_align::<Tracked>() }> = Storage::new();
struct Nested;
impl Drop for Nested {
    fn drop(&mut self) {
        NESTED_DROPS.fetch_add(1, Ordering::Relaxed);
        // Accessing a value while its destructor runs must fail, even though
        // the native table removed it before invoking its callback.
        assert!(NESTED.get(None, || panic!("reentered destructor")).is_null());
        TRACKED.get(None, || Tracked(77));
    }
}
static NESTED: Storage<Nested, { sys::thread_local::value_align::<Nested>() }> = Storage::new();

#[test]
fn pure_native_tls_cleanup_does_not_require_a_compatibility_namespace() {
    prepare(0, 11);
    let before = DROPS.load(Ordering::Relaxed);
    TRACKED.get(None, || Tracked(111));
    finish();
    assert_eq!(DROPS.load(Ordering::Relaxed), before + 1);
    assert!(CURRENT.get().is_null());
}

#[test]
fn static_keys_distinguish_values_and_reused_execution_drops_old_storage() {
    prepare(0, 1);
    let before = DROPS.load(Ordering::Relaxed);
    let first = TRACKED.get(None, || Tracked(11));
    assert_eq!(unsafe { (*first).0 }, 11);
    assert_eq!(TRACKED.get(None, || unreachable!()), first);
    finish();
    assert_eq!(DROPS.load(Ordering::Relaxed), before + 1);
    assert!(CURRENT.get().is_null());
    prepare(0, 1);
    assert_eq!(unsafe { (*TRACKED.get(None, || Tracked(22))).0 }, 22);
    finish();
    assert_eq!(DROPS.load(Ordering::Relaxed), before + 2);
}

#[test]
fn native_values_are_logical_thread_local_and_realm_local() {
    let before = DROPS.load(Ordering::Relaxed);
    prepare(0, 2);
    assert_eq!(unsafe { (*TRACKED.get(None, || Tracked(12))).0 }, 12);
    prepare(0, 3);
    assert_eq!(unsafe { (*TRACKED.get(None, || Tracked(13))).0 }, 13);
    prepare(1, 2);
    assert_eq!(unsafe { (*TRACKED.get(None, || Tracked(112))).0 }, 112);
    finish();
    prepare(0, 2);
    assert_eq!(unsafe { (*TRACKED.get(None, || unreachable!())).0 }, 12);
    finish();
    prepare(0, 3);
    assert_eq!(unsafe { (*TRACKED.get(None, || unreachable!())).0 }, 13);
    finish();
    assert_eq!(DROPS.load(Ordering::Relaxed), before + 3);
}

#[test]
fn recursive_initialization_retires_old_value_without_aliasing() {
    prepare(0, 4);
    let before = DROPS.load(Ordering::Relaxed);
    let pointer = TRACKED.get(None, || {
        assert_eq!(unsafe { (*TRACKED.get(None, || Tracked(31))).0 }, 31);
        Tracked(32)
    });
    assert_eq!(unsafe { (*pointer).0 }, 32);
    assert_eq!(DROPS.load(Ordering::Relaxed), before + 1);
    finish();
    assert_eq!(DROPS.load(Ordering::Relaxed), before + 2);
}

#[test]
fn nested_destructor_initialization_and_std_cleanup_complete_before_join() {
    let before = DROPS.load(Ordering::Relaxed);
    let nested_before = NESTED_DROPS.load(Ordering::Relaxed);
    let cleanup_before = CLEANUPS.load(Ordering::Relaxed);
    std::thread::spawn(|| {
        prepare(0, 5);
        NESTED.get(None, || Nested);
        finish();
    }).join().unwrap();
    assert_eq!(DROPS.load(Ordering::Relaxed), before + 1);
    assert_eq!(NESTED_DROPS.load(Ordering::Relaxed), nested_before + 1);
    assert_eq!(CLEANUPS.load(Ordering::Relaxed), cleanup_before + 1);
}

unsafe extern "C" fn reinstall(pointer: *mut u8) {
    assert!(unsafe { sys::thread_local::key::get(REINSTALL.force()) }.is_null());
    REINSTALLS.fetch_add(1, Ordering::Relaxed);
    unsafe { sys::thread_local::key::set(REINSTALL.force(), pointer) };
}
static REINSTALL: sys::thread_local::key::LazyKey = sys::thread_local::key::LazyKey::new(Some(reinstall));

#[test]
fn destructor_reinstallation_is_bounded_and_values_retire() {
    prepare(0, 6);
    let before = REINSTALLS.load(Ordering::Relaxed);
    unsafe { sys::thread_local::key::set(REINSTALL.force(), 1usize as *mut u8) };
    finish();
    assert_eq!(REINSTALLS.load(Ordering::Relaxed), before + 4);
    assert!(unsafe { sys::thread_local::key::get(REINSTALL.force()) }.is_null());
}

#[test]
fn quiescent_reset_removes_old_values_without_calling_retired_code() {
    prepare(2, 7);
    let before = REINSTALLS.load(Ordering::Relaxed);
    unsafe { sys::thread_local::key::set(REINSTALL.force(), 1usize as *mut u8) };
    (&native_process::current().tls).reset();
    assert!(unsafe { sys::thread_local::key::get(REINSTALL.force()) }.is_null());
    assert_eq!(REINSTALLS.load(Ordering::Relaxed), before);
    prepare(2, 7);
    finish();
}

static FIRST_IS_A: AtomicBool = AtomicBool::new(false);
static CROSS_DROPS: AtomicUsize = AtomicUsize::new(0);
struct Cross(usize);
impl Drop for Cross {
    fn drop(&mut self) {
        if self.0 == 1 {
            let other = if FIRST_IS_A.load(Ordering::Relaxed) { &CROSS_B } else { &CROSS_A };
            let pointer = other.get(None, || panic!("pending TLS value was removed before its destructor"));
            assert_eq!(unsafe { (*pointer).0 }, 2);
        }
        CROSS_DROPS.fetch_add(1, Ordering::Relaxed);
    }
}
static CROSS_A: Storage<Cross, { sys::thread_local::value_align::<Cross>() }> = Storage::new();
static CROSS_B: Storage<Cross, { sys::thread_local::value_align::<Cross>() }> = Storage::new();

#[test]
fn destructor_can_read_another_pending_value_without_reinitializing_it() {
    prepare(0, 8);
    // Storage's only nonzero field is its static identity key. Pick the lower
    // address, which the native table visits first, for the accessing dtor.
    let a_first = std::ptr::addr_of!(CROSS_A).addr() < std::ptr::addr_of!(CROSS_B).addr();
    FIRST_IS_A.store(a_first, Ordering::Relaxed);
    CROSS_A.get(None, || Cross(if a_first { 1 } else { 2 }));
    CROSS_B.get(None, || Cross(if a_first { 2 } else { 1 }));
    let before = CROSS_DROPS.load(Ordering::Relaxed);
    finish();
    assert_eq!(CROSS_DROPS.load(Ordering::Relaxed), before + 2);
}

#[test]
fn host_carrier_mutations_force_the_owning_vm_allocation_domain() {
    REALM.set(2);
    EXECUTION.set(10);
    assert_eq!(FORCED_DOMAIN.get(), None, "ambient carrier domain should be host");
    let before = FORCED_MUTATIONS.load(Ordering::Relaxed);
    let result = r::native_tls::trueos_platform_thread_local_set(1234, 11usize as *mut u8, None);
    assert_eq!(result, 0);
    assert_eq!(FORCED_MUTATIONS.load(Ordering::Relaxed), before + 1);
    assert_eq!(FORCED_DOMAIN.get(), None);
    assert_eq!(r::native_tls::trueos_platform_thread_local_get(1234) as usize, 11);
    GUEST_ARENA_AVAILABLE.set(false);
    assert_eq!(r::native_tls::trueos_platform_thread_local_set(1234, 12usize as *mut u8, None), 12);
    assert_eq!(r::native_tls::trueos_platform_thread_local_get(1234) as usize, 11);
    GUEST_ARENA_AVAILABLE.set(true);
    (&native_process::current().tls).reset();
}

// Native runtime metadata must not be an ordinary destructor-owned TLS value:
// real upstream Storage destructors may still need it after earlier TLS keys.
static FINAL_METADATA: sys::thread_local::LocalPointer = sys::thread_local::LocalPointer::__new();
static METADATA_USER_DROPS: AtomicUsize = AtomicUsize::new(0);
static METADATA_FINAL_CLEANUPS: AtomicUsize = AtomicUsize::new(0);
struct MetadataUser;
impl Drop for MetadataUser {
    fn drop(&mut self) {
        assert_eq!(FINAL_METADATA.get() as usize, 99);
        assert!(!CURRENT.get().is_null(), "std's current thread pointer must outlive user TLS");
        METADATA_USER_DROPS.fetch_add(1, Ordering::Relaxed);
    }
}
static METADATA_USER: Storage<MetadataUser, { sys::thread_local::value_align::<MetadataUser>() }> = Storage::new();
unsafe extern "C" fn final_native_metadata_cleanup() {
    assert_eq!(FINAL_METADATA.get() as usize, 99);
    FINAL_METADATA.set(2usize as *mut ());
    METADATA_FINAL_CLEANUPS.fetch_add(1, Ordering::Relaxed);
}
#[test]
fn std_guard_and_native_final_metadata_cleanup_coexist_in_both_registration_orders() {
    let std_before = CLEANUPS.load(Ordering::Relaxed);
    let user_before = METADATA_USER_DROPS.load(Ordering::Relaxed);
    let native_before = METADATA_FINAL_CLEANUPS.load(Ordering::Relaxed);
    for (execution, native_first) in [(12, true), (13, false)] {
        REALM.set(0); EXECUTION.set(execution);
        if native_first {
            assert_eq!(r::native_tls::trueos_platform_thread_local_register_cleanup(final_native_metadata_cleanup), 0);
        }
        prepare(0, execution);
        FINAL_METADATA.set(99usize as *mut ());
        METADATA_USER.get(None, || MetadataUser);
        // Duplicate registration (like guard enable) remains idempotent.
        assert_eq!(r::native_tls::trueos_platform_thread_local_register_cleanup(final_native_metadata_cleanup), 0);
        assert_eq!(r::native_tls::trueos_platform_thread_local_register_cleanup(final_native_metadata_cleanup), 0);
        finish();
        assert!(FINAL_METADATA.get().is_null());
        assert!(CURRENT.get().is_null());
    }
    assert_eq!(CLEANUPS.load(Ordering::Relaxed), std_before + 2);
    assert_eq!(METADATA_USER_DROPS.load(Ordering::Relaxed), user_before + 2);
    assert_eq!(METADATA_FINAL_CLEANUPS.load(Ordering::Relaxed), native_before + 2);
}
