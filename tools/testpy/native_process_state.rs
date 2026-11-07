#![allow(dead_code)]
extern crate alloc;
extern crate self as spin;
use std::cell::Cell;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
pub struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    pub const fn new(value: T) -> Self { Self(std::sync::Mutex::new(value)) }
    pub fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
}
std::thread_local! {
    static VM: Cell<Option<u8>> = const { Cell::new(None) };
    static EXECUTION: Cell<Option<usize>> = const { Cell::new(None) };
    static HULL: Cell<bool> = const { Cell::new(false) };
    static DOMAIN: Cell<Option<u8>> = const { Cell::new(None) };
}
static ARENA: AtomicBool = AtomicBool::new(true);
static MUTATIONS: AtomicUsize = AtomicUsize::new(0);
static SCHEDULER_REJECT: AtomicBool = AtomicBool::new(false);
static WAIT_HITS: AtomicUsize = AtomicUsize::new(0);
static COMPAT_INITIALIZATIONS: AtomicUsize = AtomicUsize::new(0);
static HULL_TAG: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
static COMPAT_FLAGS: [std::sync::atomic::AtomicU8; @VM_LIMIT@] =
    [const { std::sync::atomic::AtomicU8::new(2) }; @VM_LIMIT@];
mod allcaps { pub mod hv { pub const VM_ID_LIMIT: usize = @VM_LIMIT@; } }
mod allocators {
    pub fn hv_guest_allocator_state_span(vm: u8) -> Option<(u64, usize)> { Some((0x800000 + u64::from(vm) * 4096, 4096)) }
    pub fn with_host_alloc_domain<R>(f: impl FnOnce() -> R) -> R { f() }
    pub fn with_hv_guest_alloc_domain<R>(vm: u8, f: impl FnOnce() -> R) -> Option<R> {
        if !crate::ARENA.load(crate::Ordering::Relaxed) { return None; }
        let before = crate::DOMAIN.replace(Some(vm));
        crate::MUTATIONS.fetch_add(1, crate::Ordering::Relaxed);
        let result = f();
        crate::DOMAIN.set(before);
        Some(result)
    }
}
mod hv {
    pub fn current_hull_guest_context_vm_id() -> Option<u8> { crate::HULL.get().then(|| crate::VM.get()).flatten() }
    pub fn current_guest_execution_context_vm_id() -> Option<u8> { crate::VM.get() }
    pub fn current_vm_lapic_low_tag_addr() -> u64 { &crate::HULL_TAG as *const _ as u64 }
    pub fn blueprint_launch_active(_: u8) -> bool { false }
    pub fn blueprint_launch_states_span() -> (u64, usize) { (0, 0) }
    pub fn blueprint_process_contexts_span() -> (u64, usize) { (0, 0) }
}
mod wls {
    pub const BLUEPRINT_THREAD_CARRIER_TAG: u32 = 0x8000_0000;
    pub const WORKER_SLOT_COUNT: usize = 32;
    pub fn current_blueprint_thread_id() -> Option<u32> { None }
    pub fn current_worker_id() -> Option<usize> { None }
}
mod percpu { pub fn current_slot_via_cpuid() -> usize { 0 } }
mod std_abi_shim {
    pub(crate) fn blueprint_process_state_span(vm: u8) -> Option<(u64, usize)> { Some((0x900000 + u64::from(vm) * 4096, 4096)) }
    pub(crate) fn blueprint_process_state_ready_flag(vm: u8) -> Option<(u64, u8)> {
        Some((crate::COMPAT_FLAGS.get(vm as usize)? as *const _ as u64, 2))
    }
    pub(crate) fn run_compat_tls_destructor_round(_: usize) -> bool { false }
    pub(crate) fn clear_compat_tls_values(_: usize) {}
}
mod wait {
    use crate::Mutex;
    use core::sync::atomic::{AtomicU32, Ordering};
    pub struct WaitQueue { seq: AtomicU32 }
    impl WaitQueue {
        pub const fn new() -> Self { Self { seq: AtomicU32::new(0) } }
        pub fn notify_all(&self) { self.seq.fetch_add(1, Ordering::Release); }
        pub fn observe(&self) -> u32 { self.seq.load(Ordering::Acquire) }
        pub fn wait_for_event_after_blocking_parked(&self, observed: u32, _: u64) {
            while self.observe() == observed { std::thread::yield_now(); }
        }
    }
    @COMPLETION@
}
type Job = (Option<u8>, usize, Box<dyn FnOnce() + Send>);
static JOBS: Mutex<Vec<Job>> = Mutex::new(Vec::new());
static WAIT_HOOK: Mutex<Option<Box<dyn FnOnce() + Send>>> = Mutex::new(None);
static GENERATIONS: Mutex<BTreeMap<(Option<u8>, u64), u32>> = Mutex::new(BTreeMap::new());
mod r {
    #[path = "@ROOT@/src/r/native_tls.rs"] pub(crate) mod native_tls;
    #[path = "@ROOT@/src/r/native_thread.rs"] pub(crate) mod native_thread;
    #[path = "@ROOT@/src/r/native_process.rs"] pub(crate) mod native_process;
    pub mod io { pub mod env {
        pub fn initial_snapshot() -> std::collections::BTreeMap<String, String> {
            [("HOST".into(), "host-only".into())].into()
        }
    } }
    pub mod threads {
        pub fn current_id() -> Option<usize> { crate::EXECUTION.get() }
        pub fn current_vm_id() -> Option<u8> { crate::EXECUTION.get().and(crate::VM.get()) }
        pub fn spawn(_: usize, job: Box<dyn FnOnce() + Send>, vm: Option<u8>, id: usize) -> Result<(), i32> {
            if crate::SCHEDULER_REJECT.load(crate::Ordering::Relaxed) { return Err(11); }
            crate::JOBS.lock().push((vm, id, job));
            Ok(())
        }
    }
    pub mod platform {
        pub fn trueos_tokio_platform_wait_observe(key: u64) -> u32 {
            *crate::GENERATIONS.lock().entry((crate::VM.get(), key)).or_default()
        }
        pub fn trueos_tokio_platform_wake_all(key: u64) -> usize {
            *crate::GENERATIONS.lock().entry((crate::VM.get(), key)).or_default() += 1;
            0
        }
        pub fn trueos_tokio_platform_wait_after(key: u64, observed: u32, timeout: u64) -> bool {
            assert_eq!(timeout, u64::MAX);
            crate::WAIT_HITS.fetch_add(1, crate::Ordering::Relaxed);
            let hook = crate::WAIT_HOOK.lock().take();
            if let Some(hook) = hook { hook(); }
            if trueos_tokio_platform_wait_observe(key) == observed { crate::run_pending(); }
            assert_ne!(trueos_tokio_platform_wait_observe(key), observed, "completion must publish a generation");
            true
        }
    }
}
fn run_pending() {
    let vm = VM.get();
    let (owner, id, job) = {
        let mut jobs = JOBS.lock();
        let index = jobs.iter().position(|job| job.0 == vm).expect("owned scheduled job");
        jobs.remove(index)
    };
    let previous_execution = EXECUTION.replace(Some(id));
    let previous_vm = VM.replace(owner);
    let previous_hull = HULL.replace(false);
    job();
    HULL.set(previous_hull);
    VM.set(previous_vm);
    EXECUTION.set(previous_execution);
}
fn realm(vm: Option<u8>) { VM.set(vm); EXECUTION.set(None); HULL.set(vm.is_some()); ARENA.store(true, Ordering::Relaxed); }
unsafe extern "C" fn simple(arg: *mut core::ffi::c_void) -> *mut core::ffi::c_void { (arg as usize + 1) as *mut _ }
fn spawn(arg: usize) -> usize { r::native_thread::spawn(64 * 1024, simple, arg as *mut _, |_| true).unwrap() }
mod cabi {
    use core::ffi::{c_int, c_void};
    const TRUEOS_EINVAL: i32 = 22;
    fn pthread_create_trace(_: usize, _: usize, _: i32, _: &'static str) {}
    fn copy_usize_to_abi_out(pointer: *mut usize, value: usize) -> bool {
        if pointer.is_null() { return false; }
        unsafe { pointer.write(value) }; true
    }
    fn copy_to_abi_out(pointer: *mut u8, bytes: &[u8]) -> bool {
        if pointer.is_null() { return false; }
        unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer, bytes.len()) }; true
    }
    @CABI@
}
mod memory {
    const PAGE_SIZE_4K: usize = 4096;
    fn page_align_down(value: u64) -> u64 { value & !4095 }
    fn page_align_up_4k(value: u64) -> u64 { value.div_ceil(4096) * 4096 }
    fn hvlogf(_: core::fmt::Arguments<'_>) {}
    @MAPPING@
    pub fn is_shared(vm: u8, address: u64) -> bool { guest_hull_rw_page_uses_kernel_backing(vm, address) }
    pub fn patch(vm: u8, arena: &mut [u8], guest_start: u64) {
        patch_guest_hull_rw_dynamic_state(vm, arena.as_mut_ptr() as usize, guest_start, arena.len());
    }
}

#[test]
fn native_page_is_small_aligned_disjoint_and_ready() {
    let a = r::native_process::span(0).unwrap(); let b = r::native_process::span(1).unwrap();
    assert_eq!(a.1, 4096); assert_eq!(a.0 % 4096, 0); assert_eq!(b.0 - a.0, 4096);
    let (ready, value) = r::native_process::ready_flag(0).unwrap();
    assert_eq!(unsafe { *(ready as *const u8) }, value);
    assert!(r::native_process::span(u8::MAX).is_none());
    println!("native process state: {} bytes, {} byte alignment", a.1, std::mem::align_of::<r::native_process::State>());
}
#[test]
fn concurrent_native_state_initialization_publishes_one_stable_page() {
    let threads = (0..16).map(|_| std::thread::spawn(|| r::native_process::span(17).unwrap()))
        .collect::<Vec<_>>();
    let spans = threads.into_iter().map(|thread| thread.join().unwrap()).collect::<Vec<_>>();
    assert!(spans.iter().all(|span| *span == spans[0]));
    assert_eq!(spans[0].1, 4096);
}
#[test]
fn ept_shares_only_the_selected_native_page_and_patches_its_private_ready_byte() {
    let vm = 16;
    let (page, length) = r::native_process::span(vm).unwrap();
    assert!(memory::is_shared(vm, page)); assert!(memory::is_shared(vm, page + length as u64 - 1));
    assert!(!memory::is_shared(vm, r::native_process::span(vm + 1).unwrap().0));
    assert!(memory::is_shared(vm, 0x800000 + u64::from(vm) * 4096));
    assert!(memory::is_shared(vm, 0x900000 + u64::from(vm) * 4096));
    let (ready, value) = r::native_process::ready_flag(vm).unwrap();
    let (compat_ready, _) = std_abi_shim::blueprint_process_state_ready_flag(vm).unwrap();
    let tag = hv::current_vm_lapic_low_tag_addr();
    let low = ready.min(compat_ready).min(tag) - 1;
    let high = ready.max(compat_ready).max(tag) + 2;
    assert!(high - low < 8 * 1024 * 1024);
    let mut private = vec![0; (high - low) as usize];
    memory::patch(vm, &mut private, low);
    assert_eq!(private[(ready - low) as usize], value);
    assert_eq!(private[(compat_ready - low) as usize], 2);
    assert_eq!(private[(tag - low) as usize], vm + 1);
    assert_eq!(private[(ready - low) as usize + 1], 0, "other VM ready bytes stay private and uninitialized");
}
#[test]
fn environment_native_access_is_realm_owned_and_host_is_separate() {
    r::native_process::initialize_environment(2, [(b"HOME".as_slice(), b"/two".as_slice())].into_iter()).unwrap();
    r::native_process::initialize_environment(3, [(b"HOME".as_slice(), b"/three".as_slice())].into_iter()).unwrap();
    assert_eq!(r::native_process::environment_bytes(Some(2), b"HOME"), Some(b"/two".to_vec()));
    assert_eq!(r::native_process::environment_bytes(Some(3), b"HOME"), Some(b"/three".to_vec()));
    assert_eq!(r::native_process::environment_bytes(None, b"HOST"), Some(b"host-only".to_vec()));
    assert_eq!(r::native_process::environment_bytes(Some(2), b"HOST"), None);
    assert!(r::native_process::for_vm(2).unwrap().environment.lock().environ.is_null());
    r::native_process::reset(2); r::native_process::reset(3);
}
#[test]
fn native_tls_and_thread_completion_force_guest_pointee_ownership() {
    realm(Some(4));
    let before = MUTATIONS.load(Ordering::Relaxed);
    assert_eq!(r::native_tls::trueos_platform_thread_local_set(1, 55usize as *mut _, None), 0);
    assert_eq!(r::native_tls::trueos_platform_thread_local_get(1) as usize, 55);
    assert!(MUTATIONS.load(Ordering::Relaxed) > before);
    let id = spawn(9); assert_eq!(r::native_thread::join(id), Ok(10));
    assert_eq!(r::native_process::current().threads.live_count(), 0);
    assert_eq!(COMPAT_INITIALIZATIONS.load(Ordering::Relaxed), 0);
    assert_eq!(DOMAIN.get(), None);
    r::native_process::reset(4);
}
#[test]
fn join_observes_before_completion_and_consumes_handle_once() {
    realm(Some(5)); let before = WAIT_HITS.load(Ordering::Relaxed); let id = spawn(41);
    assert_eq!(r::native_thread::join(id), Ok(42));
    assert_eq!(WAIT_HITS.load(Ordering::Relaxed), before + 1);
    assert_eq!(r::native_thread::join(id), Err(3)); assert_eq!(r::native_thread::detach(id), Err(3));
    r::native_process::reset(5);
}
#[test]
fn wrong_realm_cannot_consume_a_live_thread_handle() {
    realm(Some(6)); let id = spawn(10); realm(Some(7));
    assert_eq!(r::native_thread::join(id), Err(3)); assert_eq!(r::native_thread::detach(id), Err(3));
    realm(Some(6)); assert_eq!(r::native_thread::join(id), Ok(11));
    r::native_process::reset(6); r::native_process::reset(7);
}
#[test]
fn dynamic_completed_handles_have_no_fixed_pthread_table_limit() {
    realm(Some(8)); let mut ids = Vec::new();
    for value in 0..96 { ids.push((spawn(value), value + 1)); run_pending(); }
    assert_eq!(r::native_process::current().threads.live_count(), 96);
    for (id, value) in ids { assert_eq!(r::native_thread::join(id), Ok(value)); }
    r::native_process::reset(8);
}
#[test]
fn detach_keeps_execution_alive_and_does_not_reuse_handles() {
    realm(Some(9)); let id = spawn(0); assert_eq!(r::native_thread::detach(id), Ok(()));
    assert_eq!(r::native_thread::join(id), Err(3)); run_pending();
    let next = spawn(1); assert_ne!(next, id); assert_eq!(r::native_thread::join(next), Ok(2));
    r::native_process::reset(9);
}
#[test]
fn admission_or_output_failure_rolls_back_the_native_join_resource() {
    realm(Some(10));
    assert_eq!(r::native_thread::spawn(0, simple, std::ptr::null_mut(), |_| false), Err(22));
    assert_eq!(r::native_process::current().threads.live_count(), 0);
    SCHEDULER_REJECT.store(true, Ordering::Relaxed);
    assert_eq!(r::native_thread::spawn(0, simple, std::ptr::null_mut(), |_| true), Err(11));
    SCHEDULER_REJECT.store(false, Ordering::Relaxed);
    assert_eq!(r::native_process::current().threads.live_count(), 0);
    ARENA.store(false, Ordering::Relaxed);
    assert_eq!(r::native_thread::spawn(0, simple, std::ptr::null_mut(), |_| true), Err(12));
    ARENA.store(true, Ordering::Relaxed); r::native_process::reset(10);
}
#[test]
fn competing_join_or_detach_cannot_consume_a_claimed_handle() {
    realm(Some(13)); let id = spawn(5);
    *WAIT_HOOK.lock() = Some(Box::new(move || {
        assert_eq!(r::native_thread::join(id), Err(22));
        assert_eq!(r::native_thread::detach(id), Err(22));
    }));
    assert_eq!(r::native_thread::join(id), Ok(6)); r::native_process::reset(13);
}
unsafe extern "C" fn self_join(_: *mut core::ffi::c_void) -> *mut core::ffi::c_void {
    let id = r::threads::current_id().unwrap();
    assert_eq!(r::native_thread::join(id), Err(35));
    assert_eq!(cabi::trueos_cabi_thread_current_id(), id);
    std::ptr::null_mut()
}
#[test]
fn public_native_ids_match_spawn_handles_and_self_join_is_rejected() {
    realm(Some(14)); let main_id = cabi::trueos_cabi_thread_current_id();
    assert!(main_id > u32::MAX as usize, "main identity must be outside every admitted handle");
    for value in 0..16 { let id = spawn(value); assert!(id <= u32::MAX as usize); assert_ne!(id, main_id); assert_eq!(r::native_thread::join(id), Ok(value + 1)); }
    let mut id = 0;
    assert_eq!(unsafe { cabi::trueos_cabi_thread_spawn(0, self_join, std::ptr::null_mut(), &mut id) }, 0);
    assert_ne!(id, main_id);
    assert_eq!(unsafe { cabi::trueos_cabi_thread_join(id) }, 0);
    r::native_process::reset(14);
}
#[test]
fn pthread_and_native_abis_share_one_native_handle_lifetime() {
    realm(Some(15)); let mut id = 0;
    assert_eq!(unsafe { cabi::trueos_cabi_thread_spawn(0, simple, 20usize as *mut _, &mut id) }, 0);
    let mut value = std::ptr::null_mut();
    assert_eq!(unsafe { cabi::pthread_join(id, &mut value) }, 0); assert_eq!(value as usize, 21);
    assert_eq!(unsafe { cabi::trueos_cabi_thread_join(id) }, 3);
    let id = spawn(1); assert_eq!(unsafe { cabi::pthread_detach(id) }, 0);
    assert_eq!(unsafe { cabi::trueos_cabi_thread_detach(id) }, 3); run_pending();
    assert_eq!(unsafe { cabi::trueos_cabi_thread_spawn(0, simple, std::ptr::null_mut(), std::ptr::null_mut()) }, 22);
    r::native_process::reset(15);
}
static DTOR: AtomicUsize = AtomicUsize::new(0);
static CLEANUP: AtomicUsize = AtomicUsize::new(0);
unsafe extern "C" fn destroy(_: *mut u8) { DTOR.fetch_add(1, Ordering::Relaxed); }
unsafe extern "C" fn cleanup() { assert_eq!(DTOR.load(Ordering::Relaxed), 1); CLEANUP.store(1, Ordering::Relaxed); }
unsafe extern "C" fn second_cleanup() {
    assert_eq!(DTOR.load(Ordering::Relaxed), 1);
    assert_eq!(CLEANUP.load(Ordering::Relaxed), 1);
    CLEANUP.store(2, Ordering::Relaxed);
}
unsafe extern "C" fn with_tls(_: *mut core::ffi::c_void) -> *mut core::ffi::c_void {
    assert_eq!(r::native_tls::trueos_platform_thread_local_set(77, 1usize as *mut _, Some(destroy)), 0);
    assert_eq!(r::native_tls::trueos_platform_thread_local_register_cleanup(cleanup), 0);
    assert_eq!(r::native_tls::trueos_platform_thread_local_register_cleanup(cleanup), 0);
    assert_eq!(r::native_tls::trueos_platform_thread_local_register_cleanup(second_cleanup), 0);
    std::ptr::null_mut()
}
#[test]
fn tls_cleanup_finishes_before_the_join_result_is_published() {
    realm(Some(11)); DTOR.store(0, Ordering::Relaxed); CLEANUP.store(0, Ordering::Relaxed);
    let id = r::native_thread::spawn(0, with_tls, std::ptr::null_mut(), |_| true).unwrap();
    assert_eq!(r::native_thread::join(id), Ok(0));
    assert_eq!(DTOR.load(Ordering::Relaxed), 1); assert_eq!(CLEANUP.load(Ordering::Relaxed), 2);
    r::native_process::reset(11);
}
#[test]
fn forced_vm_retirement_drops_state_without_calling_guest_destructors() {
    realm(Some(12)); let before = DTOR.load(Ordering::Relaxed);
    r::native_tls::trueos_platform_thread_local_set(77, 1usize as *mut _, Some(destroy));
    r::native_process::initialize_environment(12, [(b"OLD".as_slice(), b"old".as_slice())].into_iter()).unwrap();
    let id = spawn(2); run_pending();
    r::native_process::reset(12);
    assert_eq!(DTOR.load(Ordering::Relaxed), before);
    assert!(r::native_tls::trueos_platform_thread_local_get(77).is_null());
    assert_eq!(r::native_thread::join(id), Err(3));
    assert_eq!(r::native_process::environment_bytes(Some(12), b"OLD"), None);
}
#[test]
fn retirement_leaves_another_vms_tls_environment_and_join_handles_intact() {
    realm(Some(18)); r::native_tls::trueos_platform_thread_local_set(88, 18usize as *mut _, None);
    r::native_process::initialize_environment(18, [(b"REALM".as_slice(), b"18".as_slice())].into_iter()).unwrap();
    let old = spawn(18); run_pending();
    realm(Some(19)); r::native_tls::trueos_platform_thread_local_set(88, 19usize as *mut _, None);
    r::native_process::initialize_environment(19, [(b"REALM".as_slice(), b"19".as_slice())].into_iter()).unwrap();
    let retained = spawn(19); run_pending();
    r::native_process::reset(18);
    assert_eq!(r::native_tls::trueos_platform_thread_local_get(88) as usize, 19);
    assert_eq!(r::native_process::environment_bytes(Some(19), b"REALM"), Some(b"19".to_vec()));
    assert_eq!(r::native_thread::join(retained), Ok(20));
    realm(Some(18)); assert_eq!(r::native_thread::join(old), Err(3));
    assert!(r::native_tls::trueos_platform_thread_local_get(88).is_null());
    r::native_process::reset(19);
}

// Final runtime metadata outlives all ordinary destructor rounds. The real
// production State and exported registration API are used by these callbacks.
const FINAL_METADATA_KEY: usize = 0xfeed;
static FINAL_TRACE: Mutex<Vec<usize>> = Mutex::new(Vec::new());
unsafe extern "C" fn final_ordinary(_: *mut u8) {
    assert_eq!(r::native_tls::trueos_platform_thread_local_get(FINAL_METADATA_KEY) as usize, 99);
    FINAL_TRACE.lock().push(1);
}
unsafe extern "C" fn final_first() {
    assert_eq!(r::native_tls::trueos_platform_thread_local_get(FINAL_METADATA_KEY) as usize, 99);
    FINAL_TRACE.lock().push(2);
}
unsafe extern "C" fn final_second() {
    assert_eq!(r::native_tls::trueos_platform_thread_local_get(FINAL_METADATA_KEY) as usize, 99);
    FINAL_TRACE.lock().push(3);
    assert_eq!(r::native_tls::trueos_platform_thread_local_set(FINAL_METADATA_KEY, 2usize as *mut _, None), 0);
}
#[test]
fn final_cleanup_callbacks_coexist_are_idempotent_and_keep_metadata_live_until_retirement() {
    realm(Some(20)); FINAL_TRACE.lock().clear();
    assert_eq!(r::native_tls::trueos_platform_thread_local_set(FINAL_METADATA_KEY, 99usize as *mut _, None), 0);
    assert_eq!(r::native_tls::trueos_platform_thread_local_set(999, 1usize as *mut _, Some(final_ordinary)), 0);
    for callback in [final_first as r::native_tls::Cleanup, final_first, final_second, final_second] {
        assert_eq!(r::native_tls::trueos_platform_thread_local_register_cleanup(callback), 0);
    }
    let owner = r::native_tls::current_identity();
    r::native_tls::run_execution_destructors(owner);
    assert_eq!(*FINAL_TRACE.lock(), [1, 2, 3]);
    assert!(r::native_tls::trueos_platform_thread_local_get(FINAL_METADATA_KEY).is_null());
    r::native_tls::run_execution_destructors(owner);
    assert_eq!(*FINAL_TRACE.lock(), [1, 2, 3], "consumed callbacks must never run twice");
    r::native_process::reset(20);
}
unsafe extern "C" fn final_late() {
    assert_eq!(r::native_tls::trueos_platform_thread_local_get(FINAL_METADATA_KEY) as usize, 77);
    FINAL_TRACE.lock().push(7);
}
unsafe extern "C" fn final_registering() {
    assert_eq!(r::native_tls::trueos_platform_thread_local_get(FINAL_METADATA_KEY) as usize, 77);
    FINAL_TRACE.lock().push(4);
    assert_eq!(r::native_tls::trueos_platform_thread_local_register_cleanup(final_late), 0);
    assert_eq!(r::native_tls::trueos_platform_thread_local_register_cleanup(final_registering), 0);
    // A nested drain cannot consume pending callbacks before this one finishes.
    r::native_process::current().tls.run_cleanup(r::native_tls::current_identity());
    FINAL_TRACE.lock().push(5);
}
unsafe extern "C" fn final_existing() { FINAL_TRACE.lock().push(6); }
#[test]
fn finalizers_may_register_another_callback_or_reenter_without_deadlock_or_duplicate_runs() {
    realm(Some(21)); FINAL_TRACE.lock().clear();
    assert_eq!(r::native_tls::trueos_platform_thread_local_set(FINAL_METADATA_KEY, 77usize as *mut _, None), 0);
    assert_eq!(r::native_tls::trueos_platform_thread_local_register_cleanup(final_registering), 0);
    assert_eq!(r::native_tls::trueos_platform_thread_local_register_cleanup(final_existing), 0);
    r::native_tls::run_execution_destructors(r::native_tls::current_identity());
    assert_eq!(*FINAL_TRACE.lock(), [4, 5, 6, 7]);
    assert!(r::native_tls::trueos_platform_thread_local_get(FINAL_METADATA_KEY).is_null());
    r::native_process::reset(21);
}
unsafe extern "C" fn final_realm_record() {
    FINAL_TRACE.lock().push(r::native_tls::trueos_platform_thread_local_get(FINAL_METADATA_KEY) as usize);
}
#[test]
fn final_callbacks_are_vm_owned_and_forced_reset_never_calls_retired_guest_code() {
    FINAL_TRACE.lock().clear();
    realm(Some(22));
    assert_eq!(r::native_tls::trueos_platform_thread_local_set(FINAL_METADATA_KEY, 22usize as *mut _, None), 0);
    assert_eq!(r::native_tls::trueos_platform_thread_local_register_cleanup(final_realm_record), 0);
    realm(Some(23));
    assert_eq!(r::native_tls::trueos_platform_thread_local_set(FINAL_METADATA_KEY, 23usize as *mut _, None), 0);
    assert_eq!(r::native_tls::trueos_platform_thread_local_register_cleanup(final_realm_record), 0);
    r::native_process::reset(22);
    assert!(FINAL_TRACE.lock().is_empty(), "forced reset must discard callback bookkeeping only");
    r::native_tls::run_execution_destructors(r::native_tls::current_identity());
    assert_eq!(*FINAL_TRACE.lock(), [23]);
    realm(Some(22)); r::native_tls::run_execution_destructors(r::native_tls::current_identity());
    assert_eq!(*FINAL_TRACE.lock(), [23]);
    r::native_process::reset(23);
}
