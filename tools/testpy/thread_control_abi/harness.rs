#![allow(dead_code, unused_imports)]
extern crate alloc;
extern crate self as spin;
extern crate self as trueos_vm;
use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicU64, Ordering};

pub struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    pub const fn new(value: T) -> Self { Self(std::sync::Mutex::new(value)) }
    pub fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
}
#[macro_export]
macro_rules! log_info { (target: $target:literal; $($args:tt)*) => { let _ = format_args!($($args)*); }; }
mod allcaps {
    pub mod hv {
        pub const VM_ID_LIMIT: usize = 64;
        pub const CODEX_EXECUTOR_EXPERIMENT_ENABLED: bool = !cfg!(experiment_disabled);
    }
}
std::thread_local! {
    static HULL_VM: Cell<Option<u8>> = const { Cell::new(None) };
    static NATIVE: Cell<*mut r::threads::Thread> = const { Cell::new(std::ptr::null_mut()) };
    static PAYLOAD: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static RESPONSE: Cell<(u32, u64)> = const { Cell::new((0, 0)) };
    static BACKGROUND: RefCell<Vec<u32>> = RefCell::new(vec![2, 3, 4, 5]);
    static FOREGROUND_AVAILABLE: Cell<bool> = const { Cell::new(true) };
}
static GENERATIONS: [AtomicU64; 64] = [const { AtomicU64::new(1) }; 64];
mod workers {
    #[path = @POLICY@] pub mod application_policy;
    pub fn codex_application_slot() -> Option<u32> { Some(7) }
    pub fn spawner_for_slot(slot: u32) -> Option<()> {
        (slot >= 2 && (slot != 7 || crate::FOREGROUND_AVAILABLE.with(|v| v.get()))).then_some(())
    }
    pub fn pick_background_spawners_with_slots(cap: usize) -> Vec<(u32, (), ())> {
        crate::BACKGROUND.with(|slots| slots.borrow().iter().copied().take(cap).map(|slot| (slot, (), ())).collect())
    }
}
mod hv {
    pub fn current_hull_guest_context_vm_id() -> Option<u8> { crate::HULL_VM.with(|vm| vm.get()) }
    pub fn vm_run_generation(vm: u8) -> Option<u64> {
        crate::GENERATIONS.get(vm as usize).map(|v| v.load(crate::Ordering::Relaxed)).filter(|v| *v != 0)
    }
    pub fn app_vm_archive(vm: u8) -> Option<String> {
        Some(if vm == 10 { "renamed-codex.bp" } else { "codex.bp" }.into())
    }
    pub mod vmcall {
        @CONSTANTS@
    }
}
mod r {
    pub mod blocking { pub type BlockingJobFn = Box<dyn FnOnce() + Send>; }
    pub mod threads {
        use super::blocking::BlockingJobFn;
        use crate::workers::application_policy::{PlacementMode, ThreadPlacement, ThreadRole};
        #[path = @PLACEMENT@] mod placement;
        pub struct Thread {
            id: usize,
            child_role_hint: ThreadRole,
            activity: u32,
            _admission: Admission,
        }
        struct Admission { _owner: Option<Owner> }
        struct Owner { diagnostic: Diagnostic }
        struct Diagnostic;
        impl Diagnostic { fn activity(&self, _: u32) {} }
        enum Job { Owned(BlockingJobFn), GuestRaw { data: usize, vtable: usize } }
        #[derive(Debug)]
        pub struct Submission { pub vm: Option<u8>, pub stack: usize, pub id: usize, pub role: ThreadRole, pub slot: u32 }
        pub static SUBMISSIONS: crate::Mutex<Vec<Submission>> = crate::Mutex::new(Vec::new());
        fn active() -> *mut Thread { crate::NATIVE.with(|current| current.get()) }
        fn submit(stack: usize, job: Job, vm: Option<u8>, id: usize, role: ThreadRole) -> Result<(), i32> {
            // Carrier admission/execution is the fake platform boundary. The
            // caller scope, payload decoding, generation state, and policy are
            // retained from production source.
            if stack > 64 * 1024 * 1024 { return Err(22); }
            let vm_id = vm.ok_or(22)?;
            let plan = placement::admit(vm_id, crate::hv::vm_run_generation(vm_id).ok_or(22)?)?;
            let slot = plan.policy.carrier(role, Some(7), plan.background(), plan.ordinal).ok_or(11)?;
            SUBMISSIONS.lock().push(Submission { vm, stack, id, role, slot });
            if let Job::GuestRaw { data, vtable } = job {
                let raw: *mut (dyn FnOnce() + Send) = unsafe { core::mem::transmute((data, vtable)) };
                unsafe { drop(Box::from_raw(raw)); }
            }
            Ok(())
        }
        pub fn with_native(f: impl FnOnce()) {
            let mut native = Thread { id: 91, child_role_hint: ThreadRole::Unclassified, activity: 0,
                _admission: Admission { _owner: Some(Owner { diagnostic: Diagnostic }) } };
            let previous = crate::NATIVE.with(|slot| slot.replace(&mut native));
            f();
            crate::NATIVE.with(|slot| slot.set(previous));
        }
        @THREAD_FUNCTIONS@
    }
}
mod vmcall {
    use crate::hv::vmcall::*;
    enum DispatchOutcome { Resume }
    fn request_payload(_: u8, _: u32) -> Option<Vec<u8>> { Some(crate::PAYLOAD.with(|p| p.borrow().clone())) }
    fn write_response(_: u8, _: u32, status: u32, result: u64, _: u32) { crate::RESPONSE.with(|r| r.set((status, result))); }
    pub fn call(op: u32, arg0: u64, arg1: u64) -> (u32, u64) { call_with_payload(op, arg0, arg1, &[], &mut []) }
    pub fn call_with_payload(op: u32, arg0: u64, arg1: u64, payload: &[u8], _: &mut [u8]) -> (u32, u64) {
        let vm_id = crate::HULL_VM.with(|vm| vm.get()).expect("authenticated Hull caller");
        let req_len = payload.len() as u32;
        let seq = 1;
        crate::PAYLOAD.with(|p| *p.borrow_mut() = payload.to_vec());
        match op {
            @DISPATCH_ARMS@
            _ => panic!("unexpected opcode {op}"),
        };
        crate::RESPONSE.with(|r| r.get())
    }
}
@CABI_FUNCTIONS@

#[cfg(test)]
mod tests {
    use super::*;
    use hv::vmcall::*;
    use workers::application_policy::ThreadRole;
    fn hull(vm: u8) {
        HULL_VM.with(|current| current.set(Some(vm)));
        BACKGROUND.with(|slots| *slots.borrow_mut() = vec![2, 3, 4, 5]);
        FOREGROUND_AVAILABLE.with(|available| available.set(true));
        r::threads::SUBMISSIONS.lock().clear();
    }

    #[test]
    #[cfg(not(experiment_disabled))]
    fn actual_cabi_vmcall_and_legacy_spawn_payload_capture_role() {
        hull(1);
        assert_eq!(trueos_cabi_thread_configure_placement_v1(1, 2), 0);
        assert_eq!(trueos_cabi_thread_swap_child_role_v1(1), 0);
        r::threads::spawn(8192, Box::new(|| {}), Some(1), 41).unwrap();
        assert_eq!(trueos_cabi_thread_swap_child_role_v1(2), 1);
        assert_eq!(trueos_cabi_thread_swap_activity_v1(3), 0);
        r::threads::spawn(16384, Box::new(|| {}), Some(1), 42).unwrap();
        let entries = r::threads::SUBMISSIONS.lock();
        assert_eq!((entries[0].vm, entries[0].stack, entries[0].id, entries[0].role, entries[0].slot),
            (Some(1), 8192, 41, ThreadRole::Foreground, 7));
        assert_eq!((entries[1].vm, entries[1].stack, entries[1].id, entries[1].role),
            (Some(1), 16384, 42, ThreadRole::TokioPool));
        assert!([2, 3].contains(&entries[1].slot));
        drop(entries);
        assert_eq!(trueos_cabi_thread_configure_placement_v1(0, 2), 16);
        HULL_VM.with(|vm| vm.set(None));
    }

    #[test]
    #[cfg(not(experiment_disabled))]
    fn generations_reset_hint_policy_and_frozen_background_set() {
        hull(2);
        assert_eq!(trueos_cabi_thread_configure_placement_v1(1, 1), 0);
        assert_eq!(trueos_cabi_thread_swap_child_role_v1(2), 0);
        BACKGROUND.with(|slots| *slots.borrow_mut() = vec![4, 5]);
        r::threads::spawn(4096, Box::new(|| {}), Some(2), 51).unwrap();
        assert_eq!(r::threads::SUBMISSIONS.lock()[0].slot, 2, "late AP selection changed frozen plan");
        GENERATIONS[2].store(2, Ordering::Relaxed);
        assert_eq!(trueos_cabi_thread_swap_child_role_v1(1), 0, "hint leaked across generation");
        assert_eq!(trueos_cabi_thread_configure_placement_v1(0, 2), 0);
        r::threads::spawn(4096, Box::new(|| {}), Some(2), 52).unwrap();
        assert_eq!(r::threads::SUBMISSIONS.lock()[1].slot, 7);
        HULL_VM.with(|vm| vm.set(None));
    }

    #[test]
    fn controls_validate_scalar_domains_and_preserve_state_on_error() {
        hull(3);
        assert_eq!(trueos_cabi_thread_swap_child_role_v1(2), 0);
        assert_eq!(trueos_cabi_thread_swap_child_role_v1(7), u32::MAX, "AP number accepted as role");
        assert_eq!(trueos_cabi_thread_swap_child_role_v1(0), 2);
        assert_eq!(trueos_cabi_thread_swap_activity_v1(3), 0);
        assert_eq!(trueos_cabi_thread_swap_activity_v1(4), u32::MAX);
        assert_eq!(trueos_cabi_thread_swap_activity_v1(0), 3);
        for (operation, value, payload) in [(3, 0, vec![]), (1, 1 << 32, vec![]), (0, 0, vec![0]), (1, 0, vec![0])] {
            assert_eq!(vmcall::call_with_payload(OP_BP_THREAD_CONTROL_V1, operation, value, &payload, &mut []), (STATUS_BAD_ARG, 22));
        }
        for size in [0, 15, 17, 24] {
            assert_eq!(vmcall::call_with_payload(OP_BP_THREAD_SUBMIT, 0, 0, &vec![0; size], &mut []), (STATUS_OK, 22));
        }
        assert_eq!(vmcall::call_with_payload(OP_BP_THREAD_SUBMIT, 0, 0, &[0; 16], &mut []), (STATUS_OK, 22));
        assert_eq!(trueos_cabi_thread_configure_placement_v1(3, 2), 22);
        assert_eq!(trueos_cabi_thread_configure_placement_v1(1, 0), 22);
        assert_eq!(trueos_cabi_thread_configure_placement_v1(1, 5), 22);
        HULL_VM.with(|vm| vm.set(None));
    }

    #[test]
    fn unsupported_archive_or_feature_reports_errno_without_harming_scope() {
        hull(10);
        assert_eq!(trueos_cabi_thread_configure_placement_v1(0, 2), 95);
        assert_eq!(trueos_cabi_thread_swap_child_role_v1(1), 0);
        assert_eq!(trueos_cabi_thread_swap_child_role_v1(0), 1);
        if cfg!(experiment_disabled) {
            hull(11);
            assert_eq!(trueos_cabi_thread_configure_placement_v1(0, 2), 95);
        }
        HULL_VM.with(|vm| vm.set(None));
    }

    #[test]
    #[cfg(not(experiment_disabled))]
    fn absent_carrier_and_generation_fail_before_admission() {
        hull(4);
        FOREGROUND_AVAILABLE.with(|available| available.set(false));
        assert_eq!(trueos_cabi_thread_configure_placement_v1(0, 2), 11);
        FOREGROUND_AVAILABLE.with(|available| available.set(true));
        BACKGROUND.with(|slots| slots.borrow_mut().clear());
        assert_eq!(trueos_cabi_thread_configure_placement_v1(1, 2), 11);
        assert_eq!(trueos_cabi_thread_configure_placement_v1(0, 2), 0);
        GENERATIONS[4].store(0, Ordering::Relaxed);
        assert_eq!(trueos_cabi_thread_swap_child_role_v1(1), u32::MAX);
        assert_eq!(trueos_cabi_thread_configure_placement_v1(0, 2), 22);
        HULL_VM.with(|vm| vm.set(None));
    }

    #[test]
    fn native_scopes_are_local_and_configuration_cannot_move_live_threads() {
        hull(5);
        HULL_VM.with(|vm| vm.set(None));
        assert_eq!(trueos_cabi_thread_configure_placement_v1(0, 2), 95);
        assert_eq!(trueos_cabi_thread_swap_child_role_v1(1), u32::MAX);
        r::threads::with_native(|| {
            assert_eq!(trueos_cabi_thread_configure_placement_v1(1, 2), 16);
            assert_eq!(trueos_cabi_thread_swap_child_role_v1(2), 0);
            assert_eq!(trueos_cabi_thread_swap_activity_v1(3), 0);
            std::thread::spawn(|| r::threads::with_native(|| {
                assert_eq!(trueos_cabi_thread_swap_child_role_v1(1), 0);
                assert_eq!(trueos_cabi_thread_swap_activity_v1(2), 0);
            })).join().unwrap();
            assert_eq!(trueos_cabi_thread_swap_child_role_v1(0), 2);
            assert_eq!(trueos_cabi_thread_swap_activity_v1(0), 3);
        });
    }
}
