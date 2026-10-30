#![allow(dead_code, unexpected_cfgs)]
extern crate self as x86_64;
use core::{fmt, mem::offset_of, sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering}};

mod allcaps { pub mod hv { pub const VM_ID_LIMIT: usize = 64; pub const VM_CPU_SLOT_LIMIT: usize = 256; } }
mod percpu {
    pub static SLOT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(6);
    pub fn current_slot() -> usize { SLOT.load(std::sync::atomic::Ordering::Acquire) }
    pub struct Cpu;
    impl Cpu { pub fn lapic_id(&self) -> u32 { current_slot() as u32 } }
    pub fn this_cpu() -> Cpu { Cpu }
}
pub mod registers { pub mod model_specific {
    pub struct Msr(u32);
    impl Msr {
        pub fn new(id: u32) -> Self { Self(id) }
        pub unsafe fn read(&self) -> u64 {
            if self.0 == crate::hv::vmx::IA32_VMX_PROCBASED_CTLS2 { 1 << 37 }
            else { (1 << 20) | (1 << 25) | (1 << 32) | (1 << 41) }
        }
    }
} }
mod hv {
    use super::*;
    pub fn hvlogf(_: fmt::Arguments) {}
    pub fn hverrorf(_: fmt::Arguments) {}
    #[path = @VPID_PATH@] pub mod vpid;
    pub mod vmx {
        use super::*;
        pub const IA32_VMX_PROCBASED_CTLS2: u32 = 0x48b;
        pub const IA32_VMX_EPT_VPID_CAP: u32 = 0x48c;
        pub const PROC2_BASED_ENABLE_VPID: u64 = 1 << 5;
        pub static OFF: AtomicU64 = AtomicU64::new(0);
        pub static INVVPID: AtomicU64 = AtomicU64::new(0);
        pub static INVEPT: AtomicU64 = AtomicU64::new(0);
        pub static FAIL_VPID: AtomicBool = AtomicBool::new(false);
        pub static FAIL_EPT: AtomicBool = AtomicBool::new(false);
        pub fn vmxoff() -> bool { OFF.fetch_add(1, Ordering::AcqRel); true }
        pub fn invvpid_single_context(_: u16) -> bool {
            INVVPID.fetch_add(1, Ordering::AcqRel);
            !FAIL_VPID.load(Ordering::Acquire)
        }
        pub fn invept_single_context(_: u64) -> bool {
            INVEPT.fetch_add(1, Ordering::AcqRel);
            !FAIL_EPT.load(Ordering::Acquire)
        }
    }
    pub struct Pause(pub AtomicBool);
    impl Pause { pub fn paused(&self) -> bool { self.0.load(Ordering::Acquire) } }
    pub struct Vm {
        pub run_generation: AtomicU64, pub running: AtomicBool,
        pub starting: AtomicBool, pub stop_req: AtomicBool, pub execution_pause: Pause,
    }
    pub static VM: Vm = Vm {
        run_generation: AtomicU64::new(7), running: AtomicBool::new(true),
        starting: AtomicBool::new(false), stop_req: AtomicBool::new(false),
        execution_pause: Pause(AtomicBool::new(true)),
    };
    pub fn vm_slot(id: u8) -> Option<&'static Vm> { (id == 0).then_some(&VM) }
    pub static VMX_ROOT_ACTIVE_BY_CPU: [AtomicBool; 256] = [const { AtomicBool::new(false) }; 256];
    static VMX_EXTERNAL_INTERRUPT_EXITING_BY_CPU: [AtomicBool; 256] = [const { AtomicBool::new(false) }; 256];
    static VMXON_PA_BY_CPU: [AtomicU64; 256] = [const { AtomicU64::new(0) }; 256];
    pub static ACTIVE_EPTP_BY_VM: [AtomicU64; 64] = [const { AtomicU64::new(0) }; 64];
    fn current_vmx_slot() -> Result<usize, &'static str> { Ok(crate::percpu::current_slot()) }
    pub fn enter_vmx_root_for_current_cpu_contract() -> Result<(), &'static str> {
        vpid::initialize_current_lane()?;
        VMX_ROOT_ACTIVE_BY_CPU[crate::percpu::current_slot()].store(true, Ordering::Release);
        Ok(())
    }
    @HV_CODE@
}

const CPU_SLOT_LIMIT: usize = 256;
const CPU_MASK_WORDS: usize = 4;
const MAX_TRANSITION_GDT_BYTES: usize = 256;
const TRANSITION_PARK: u64 = 1;
const TRANSITION_ABORT: u64 = 2;
const TRANSITION_SWITCH_STACKS: u64 = 3;
const TRANSITION_COMMIT: u64 = 4;
static ACTIVE_CONTROL_HHDM: AtomicU64 = AtomicU64::new(0);
static ACTIVE_AP_SLOT: AtomicU64 = AtomicU64::new(0);
static RENDEZVOUS_ISR_ACTIVE: AtomicU64 = AtomicU64::new(0);
static AP_TRANSITION_ENTERED: [AtomicBool; CPU_SLOT_LIMIT] = [const { AtomicBool::new(false) }; CPU_SLOT_LIMIT];
static AP_ISR_ENTERED_MASK: [AtomicU64; CPU_MASK_WORDS] = [const { AtomicU64::new(0) }; CPU_MASK_WORDS];
static AP_VMX_CONTRACT_DONE_MASK: [AtomicU64; CPU_MASK_WORDS] = [const { AtomicU64::new(0) }; CPU_MASK_WORDS];
static AP_VMXOFF_EXECUTED_MASK: [AtomicU64; CPU_MASK_WORDS] = [const { AtomicU64::new(0) }; CPU_MASK_WORDS];
static AP_VMXOFF_DEFERRED_MASK: [AtomicU64; CPU_MASK_WORDS] = [const { AtomicU64::new(0) }; CPU_MASK_WORDS];
static AP_TRANSITION_ERRORS: [ApTransitionError; CPU_SLOT_LIMIT] = [const { ApTransitionError::new() }; CPU_SLOT_LIMIT];
@LIVE_CODE@

fn reset() {
    percpu::SLOT.store(6, Ordering::Release);
    ACTIVE_CONTROL_HHDM.store(0, Ordering::Release);
    reset_ap_transition_diagnostics();
    hv::vpid::reset_fixture();
    hv::vmx::FAIL_VPID.store(false, Ordering::Release);
    hv::vmx::FAIL_EPT.store(false, Ordering::Release);
    hv::VM.run_generation.store(7, Ordering::Release);
    hv::VM.running.store(true, Ordering::Release);
    hv::VM.starting.store(false, Ordering::Release);
    hv::VM.stop_req.store(false, Ordering::Release);
    hv::VM.execution_pause.0.store(true, Ordering::Release);
    hv::ACTIVE_EPTP_BY_VM[0].store(0x12301e, Ordering::Release);
    hv::enter_vmx_root_for_current_cpu_contract().unwrap();
    hv::vmx::OFF.store(0, Ordering::Release);
    hv::vmx::INVVPID.store(0, Ordering::Release);
    hv::vmx::INVEPT.store(0, Ordering::Release);
}

#[test]
fn paused_assignment_survives_repeated_probes_and_abort() {
    reset();
    let assignment = hv::vpid::assign_current_lane(0, 7, 6).unwrap();
    for _ in 0..2 {
        assert_eq!(hv::prepare_current_cpu_for_live_update(), Ok(hv::LiveUpdateVmxDisposition::Retained));
        assignment.verify_entry_lane().unwrap();
    }
    assert_eq!(hv::vmx::OFF.load(Ordering::Acquire), 0);
    assert!(hv::vmx::INVVPID.load(Ordering::Acquire) >= 128);
    assert_eq!(hv::vmx::INVEPT.load(Ordering::Acquire), 2);
    hv::VM.execution_pause.0.store(false, Ordering::Release);
    assignment.verify_entry_lane().unwrap();
    assignment.retire().unwrap();
    assert!(hv::vpid::lane_reusable(6));
}

#[test]
fn unsafe_owners_are_rejected_without_quarantining_their_assignment() {
    reset();
    let assignment = hv::vpid::assign_current_lane(0, 7, 6).unwrap();
    for flag in [&hv::VM.execution_pause.0, &hv::VM.running] {
        flag.store(false, Ordering::Release);
        assert!(hv::prepare_current_cpu_for_live_update().is_err());
        assignment.verify_entry_lane().unwrap();
        flag.store(true, Ordering::Release);
    }
    for flag in [&hv::VM.starting, &hv::VM.stop_req] {
        flag.store(true, Ordering::Release);
        assert!(hv::prepare_current_cpu_for_live_update().is_err());
        assignment.verify_entry_lane().unwrap();
        flag.store(false, Ordering::Release);
    }
    hv::VM.run_generation.store(8, Ordering::Release);
    assert!(hv::prepare_current_cpu_for_live_update().is_err());
    assignment.verify_entry_lane().unwrap();
    assert_eq!(hv::vmx::OFF.load(Ordering::Acquire), 0);
}

#[test]
fn failed_translation_fences_prevent_transition_and_preserve_ownership() {
    reset();
    let assignment = hv::vpid::assign_current_lane(0, 7, 6).unwrap();
    for flag in [&hv::vmx::FAIL_VPID, &hv::vmx::FAIL_EPT] {
        flag.store(true, Ordering::Release);
        assert!(hv::prepare_current_cpu_for_live_update().is_err());
        assignment.verify_entry_lane().unwrap();
        flag.store(false, Ordering::Release);
    }
    hv::ACTIVE_EPTP_BY_VM[0].store(0, Ordering::Release);
    assert!(hv::prepare_current_cpu_for_live_update().is_err());
    assignment.verify_entry_lane().unwrap();
    assert_eq!(hv::vmx::OFF.load(Ordering::Acquire), 0);
}

extern "C" fn abort_park(control: *const TransitionControl, slot: usize, _: u32, deferred: bool) {
    let control = unsafe { &*control };
    assert_eq!(slot, 6);
    assert_eq!(control.arrived.load(Ordering::Acquire), 1);
    assert!(deferred);
    control.command.store(TRANSITION_ABORT, Ordering::Release);
}

#[test]
fn only_targeted_ap_enters_and_retained_abort_does_not_cycle_vmx() {
    reset();
    let assignment = hv::vpid::assign_current_lane(0, 7, 6).unwrap();
    let mut control: TransitionControl = unsafe { core::mem::zeroed() };
    control.ap_park_hhdm = abort_park as *const () as u64;
    control.command.store(TRANSITION_PARK, Ordering::Release);
    ACTIVE_CONTROL_HHDM.store(&control as *const _ as u64, Ordering::Release);
    ACTIVE_AP_SLOT.store(5, Ordering::Release);
    enter_ap_transition_if_requested();
    assert_eq!(control.arrived.load(Ordering::Acquire), 0);
    assert_eq!(hv::vmx::INVEPT.load(Ordering::Acquire), 0);
    ACTIVE_AP_SLOT.store(6, Ordering::Release);
    enter_ap_transition_if_requested();
    assert_eq!(AP_VMXOFF_DEFERRED_MASK[0].load(Ordering::Acquire), 1 << 6);
    assert_eq!(control.arrived.load(Ordering::Acquire), 0);
    assert_eq!(RENDEZVOUS_ISR_ACTIVE.load(Ordering::Acquire), 0);
    assert!(!AP_TRANSITION_ENTERED[6].load(Ordering::Acquire));
    assert_eq!(hv::vmx::OFF.load(Ordering::Acquire), 0);
    assignment.verify_entry_lane().unwrap();
    ACTIVE_CONTROL_HHDM.store(0, Ordering::Release);
}

#[test]
fn rejection_reports_exact_slot_stage_and_reason_after_abort() {
    reset();
    let assignment = hv::vpid::assign_current_lane(0, 7, 6).unwrap();
    hv::VM.execution_pause.0.store(false, Ordering::Release);
    let control: TransitionControl = unsafe { core::mem::zeroed() };
    control.command.store(TRANSITION_PARK, Ordering::Release);
    ACTIVE_CONTROL_HHDM.store(&control as *const _ as u64, Ordering::Release);
    ACTIVE_AP_SLOT.store(6, Ordering::Release);
    enter_ap_transition_if_requested();
    assert_eq!(control.failures.load(Ordering::Acquire), 1);
    assert_eq!(RENDEZVOUS_ISR_ACTIVE.load(Ordering::Acquire), 0);
    let error = ap_transition_error(6, "fallback").to_string();
    assert!(error.contains("slot=6 stage=VMX-prepare"), "{error}");
    assert!(error.contains("not RAM-paused in this generation"), "{error}");
    assert!(error.contains("VPID-lane=active"), "{error}");
    assignment.verify_entry_lane().unwrap();
    ACTIVE_CONTROL_HHDM.store(0, Ordering::Release);
}

#[test]
fn idle_lane_still_exercises_vmxoff_and_can_reenter() {
    reset();
    assert_eq!(hv::prepare_current_cpu_for_live_update(), Ok(hv::LiveUpdateVmxDisposition::Off));
    assert_eq!(hv::vmx::OFF.load(Ordering::Acquire), 1);
    assert_eq!(hv::vpid::lane_state_name(6), "offline");
    assert_eq!(hv::prepare_current_cpu_for_live_update(), Ok(hv::LiveUpdateVmxDisposition::Inactive));
    hv::enter_vmx_root_for_current_cpu_contract().unwrap();
    assert!(hv::vpid::lane_reusable(6));
}

#[test]
fn native_trampoline_abort_returns_without_executing_deferred_vmxoff() {
    let control: TransitionControl = unsafe { core::mem::zeroed() };
    control.command.store(TRANSITION_ABORT, Ordering::Release);
    for retained in [true, false] {
        unsafe { trueos_live_update_ap_park_trampoline(&control, 6, 6, retained) };
        assert_eq!(control.stacked.load(Ordering::Acquire), 0);
        assert_eq!(control.failures.load(Ordering::Acquire), 0);
    }
}
