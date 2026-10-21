pub mod app_crash;
pub mod blueprint;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod blueprint_net;
pub mod control_kick;
mod cooperative_stop;
mod depthchecker_debug;
pub(crate) mod launcher;
mod lifecycle;
pub(crate) mod matrix;
mod teardown_diagnostics;
use teardown_diagnostics::Stage as TeardownStage;
#[cfg(feature = "legacy_vm_checkpoint")]
#[path = "checkpoint_legacy.rs"]
mod checkpoint;
#[cfg(not(feature = "legacy_vm_checkpoint"))]
#[path = "checkpoint_disabled.rs"]
mod checkpoint;
pub(crate) mod execution_policy;
pub mod guest_run;
pub mod guest_work;
pub mod lane;
pub mod memory;
pub mod security;
#[cfg(feature = "legacy_vm_checkpoint")]
pub mod snapshot;
#[cfg(feature = "legacy_vm_checkpoint")]
pub mod store;
pub use checkpoint::*;
pub mod sync;
pub mod vmcall;
pub mod vmx;
pub mod vnet;
pub mod vpid;

use crate::hv::vmx::*;

pub use trueos_vm::guest;

use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::String as AllocString;
use alloc::vec::Vec as AllocVec;
use core::arch::x86_64::{__cpuid, __cpuid_count};
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

use embassy_sync::signal::Signal;
use heapless::String;
use spin::Mutex;
use trueos_executor::{Spawner, task};
use trueos_time::{Duration as EmbassyDuration, Timer, with_timeout};
use x86_64::instructions::tables::{sgdt, sidt};
use x86_64::registers::control::{Cr0, Cr0Flags, Cr3, Cr4, Cr4Flags};
use x86_64::registers::model_specific::Msr;
use x86_64::registers::rflags;
use x86_64::registers::segmentation::{CS, DS, ES, FS, GS, SS, Segment};

use crate::shell3::MatrixTarget;

use guest_work::{VmLaneProfile, pick_vm_hull_lane};
use memory::*;
const MAIN_LOOP_MARKER: &[u8] = b"main: entering executor loop";
const VMX_PAGE_SIZE: usize = 4096;
const MIB: usize = 1024 * 1024;
const HV_LOG_LINE: usize = crate::allcaps::hv::LOG_LINE_BYTES;
pub const TRUEOS_VM_ID_LIMIT: usize = crate::allcaps::hv::VM_ID_LIMIT;
const TRUEOS_VM_CPU_SLOT_LIMIT: usize = crate::allcaps::hv::VM_CPU_SLOT_LIMIT;

const GUEST_FS_BASE_RESET: u64 = 0;
const STOP_CLEANUP_TIMEOUT_MS: u64 = 2_000;
const PAUSE_CHECKPOINT_TIMEOUT_MS: u64 = 15_000;
#[cfg(feature = "legacy_vm_checkpoint")]
const BLUEPRINT_PREPARE_PAUSE_TIMEOUT_MS: u64 = 15_000;
#[cfg(feature = "legacy_vm_checkpoint")]
const BLUEPRINT_LIFECYCLE_PHASE_ARMING: u8 = 3;
#[cfg(feature = "legacy_vm_checkpoint")]
const BLUEPRINT_REPLICATION_CHECKPOINT_MAX_BYTES: usize = trueos_vm::vmcall::PAYLOAD_CAP;
const BLUEPRINT_LIFECYCLE_PHASE_RUNNING: u8 = 0;
#[cfg(feature = "legacy_vm_checkpoint")]
const BLUEPRINT_LIFECYCLE_PHASE_PREPARE_PAUSE: u8 = 1;
#[cfg(feature = "legacy_vm_checkpoint")]
const BLUEPRINT_LIFECYCLE_PHASE_READY: u8 = 2;
static BLUEPRINT_LIFECYCLE_OPERATION_SEQ: AtomicU64 = AtomicU64::new(1);
/// A child Hull deliberately has no presentation or terminal authority.  The
/// Blueprint chooses this argv marker as its internal worker entry mode.
pub const BLUEPRINT_CHILD_WORKER_ARG: &str = "--trueos-child-worker";
const BLUEPRINT_CHILD_QUEUE_LIMIT: usize = 16;
const BLUEPRINT_CHILD_MESSAGE_LIMIT: usize = trueos_vm::vmcall::PAYLOAD_CAP;
const BLUEPRINT_CHILD_STATE_STARTING: u8 = 1;
const BLUEPRINT_CHILD_STATE_RUNNING: u8 = 2;
const BLUEPRINT_CHILD_STATE_STOPPING: u8 = 3;
const BLUEPRINT_CHILD_STATE_EXITED: u8 = 4;
const BLUEPRINT_CHILD_ERR_INVALID: i32 = -1;
const BLUEPRINT_CHILD_ERR_NOT_FOUND: i32 = -2;
const BLUEPRINT_CHILD_ERR_QUEUE_FULL: i32 = -3;
const BLUEPRINT_CHILD_ERR_UNAVAILABLE: i32 = -4;


// VMX tasks can be parked in host waits rather than executing VMRESUME. An IPI
// alone cannot finish those futures; stop/kill also wakes this host wait path.
static VM_CONTROL_WAITS: [crate::wait::WaitQueue; TRUEOS_VM_ID_LIMIT] =
    [const { crate::wait::WaitQueue::new() }; TRUEOS_VM_ID_LIMIT];

async fn await_vm_control_boundary<F: core::future::Future>(
    vm_id: u8,
    future: F,
) -> Option<F::Output> {
    let queue = &VM_CONTROL_WAITS[vm_id as usize];
    let mut future = core::pin::pin!(future);
    loop {
        let observed = queue.observe();
        if !park_vm_execution(vm_id).await {
            return None;
        }
        // A cooperative stop must return a sleeping guest to its cleanup
        // code. Pause alone retains the original wait and its result.
        if vm_slot(vm_id).is_some_and(|vm| vm.stop_req.load(Ordering::Acquire)) {
            return None;
        }
        let mut control = core::pin::pin!(queue.wait_after(observed));
        let deadline_ms = vm_slot(vm_id)
            .filter(|vm| vm.execution_pause.requested())
            .map(|vm| vm.execution_pause_deadline_ms.load(Ordering::Acquire));
        let mut deadline = core::pin::pin!(async {
            match deadline_ms {
                Some(deadline) => {
                    Timer::after(EmbassyDuration::from_millis(
                        deadline.saturating_sub(lifecycle_now_ms()).max(1),
                    ))
                    .await
                }
                None => core::future::pending::<()>().await,
            }
        });
        let result = core::future::poll_fn(|cx| {
            // Poll control first. A paused wait retains its original future
            // and request; it is never cancelled or completed with a fake result.
            if control.as_mut().poll(cx).is_ready() {
                return core::task::Poll::Ready(None);
            }
            if deadline.as_mut().poll(cx).is_ready() {
                return core::task::Poll::Ready(None);
            }
            future.as_mut().poll(cx).map(Some)
        })
        .await;
        if let Some(result) = result {
            return Some(result);
        }
    }
}

#[cfg(feature = "legacy_vm_checkpoint")]
pub const fn cross_principal_snapshot_restore_supported() -> bool {
    false
}

struct TrueosVmId {
    running: AtomicBool,
    starting: AtomicBool,
    stop_req: AtomicBool,
    execution_pause: lifecycle::PauseControl,
    execution_pause_deadline_ms: AtomicU64,
    execution_pause_io_wait_logged: AtomicBool,
    cooperative_stop: cooperative_stop::CooperativeStop,
    lifecycle_control: spin::Mutex<()>,
    lifecycle_generation: AtomicU64,
    matrix_owner: spin::Mutex<Option<crate::shell3::MatrixSlotLease>>,
    preserve_req: AtomicBool,
    preserve_exit: AtomicBool,
    clean_exit: AtomicBool,
    replicatable: AtomicBool,
    pause_latched: AtomicBool,
    pause_store_seq: AtomicU64,
    lifecycle_phase: AtomicU8,
    lifecycle_operation: AtomicU64,
    lifecycle_deadline_ms: AtomicU64,
    #[cfg(feature = "legacy_vm_checkpoint")]
    lifecycle_reason: AtomicU8,
    lifecycle_checkpoint_version: AtomicU64,
    run_generation: AtomicU64,
    restore_inflight: AtomicBool,
    resume_prepared: AtomicBool,
    marker_seen: AtomicBool,
}

impl TrueosVmId {
    const fn new() -> Self {
        Self {
            running: AtomicBool::new(false),
            starting: AtomicBool::new(false),
            stop_req: AtomicBool::new(false),
            execution_pause: lifecycle::PauseControl::new(),
            execution_pause_deadline_ms: AtomicU64::new(0),
            execution_pause_io_wait_logged: AtomicBool::new(false),
            cooperative_stop: cooperative_stop::CooperativeStop::new(),
            lifecycle_control: spin::Mutex::new(()),
            lifecycle_generation: AtomicU64::new(0),
            matrix_owner: spin::Mutex::new(None),
            preserve_req: AtomicBool::new(false),
            preserve_exit: AtomicBool::new(false),
            clean_exit: AtomicBool::new(false),
            replicatable: AtomicBool::new(false),
            pause_latched: AtomicBool::new(false),
            pause_store_seq: AtomicU64::new(0),
            lifecycle_phase: AtomicU8::new(BLUEPRINT_LIFECYCLE_PHASE_RUNNING),
            lifecycle_operation: AtomicU64::new(0),
            lifecycle_deadline_ms: AtomicU64::new(0),
            #[cfg(feature = "legacy_vm_checkpoint")]
            lifecycle_reason: AtomicU8::new(BlueprintPauseReason::Pause as u8),
            lifecycle_checkpoint_version: AtomicU64::new(0),
            run_generation: AtomicU64::new(0),
            restore_inflight: AtomicBool::new(false),
            resume_prepared: AtomicBool::new(false),
            marker_seen: AtomicBool::new(false),
        }
    }
}

#[allow(non_upper_case_globals)]
static trueos_vm_ids: [TrueosVmId; TRUEOS_VM_ID_LIMIT] =
    [const { TrueosVmId::new() }; TRUEOS_VM_ID_LIMIT];
static CURRENT_VM_ID_BY_CPU: [AtomicU8; TRUEOS_VM_CPU_SLOT_LIMIT] =
    [const { AtomicU8::new(0) }; TRUEOS_VM_CPU_SLOT_LIMIT];
static CURRENT_VM_ID_BY_LAPIC_LOW: [AtomicU8; 256] = [const { AtomicU8::new(0) }; 256];
static CURRENT_GUEST_BROKER_VM_ID_BY_CPU: [AtomicU8; TRUEOS_VM_CPU_SLOT_LIMIT] =
    [const { AtomicU8::new(0) }; TRUEOS_VM_CPU_SLOT_LIMIT];
static GUEST_KERNEL_GS_BASE_BY_VM: [AtomicU64; TRUEOS_VM_ID_LIMIT] =
    [const { AtomicU64::new(0) }; TRUEOS_VM_ID_LIMIT];
// The Hull establishes the per-Blueprint EPT trust boundary.
static ACTIVE_EPTP_BY_VM: [AtomicU64; TRUEOS_VM_ID_LIMIT] =
    [const { AtomicU64::new(0) }; TRUEOS_VM_ID_LIMIT];
static VMX_ROOT_ACTIVE_BY_CPU: [AtomicBool; TRUEOS_VM_CPU_SLOT_LIMIT] =
    [const { AtomicBool::new(false) }; TRUEOS_VM_CPU_SLOT_LIMIT];
static VMX_EXTERNAL_INTERRUPT_EXITING_BY_CPU: [AtomicBool; TRUEOS_VM_CPU_SLOT_LIMIT] =
    [const { AtomicBool::new(false) }; TRUEOS_VM_CPU_SLOT_LIMIT];
static VMXON_PA_BY_CPU: [AtomicU64; TRUEOS_VM_CPU_SLOT_LIMIT] =
    [const { AtomicU64::new(0) }; TRUEOS_VM_CPU_SLOT_LIMIT];
static VMX_CORE_CONTRACT_SUMMARY_LOGGED: AtomicBool = AtomicBool::new(false);
static VM_BOOT_MODES: [Mutex<VmBootMode>; TRUEOS_VM_ID_LIMIT] =
    [const { Mutex::new(VmBootMode::Hull) }; TRUEOS_VM_ID_LIMIT];
static BLUEPRINT_PENDING_LAUNCH_STATES: [Mutex<Option<BlueprintPendingLaunchState>>;
    TRUEOS_VM_ID_LIMIT] = [const { Mutex::new(None) }; TRUEOS_VM_ID_LIMIT];
static BLUEPRINT_LAUNCH_STATES: [Mutex<Option<BlueprintLaunchState>>; TRUEOS_VM_ID_LIMIT] =
    [const { Mutex::new(None) }; TRUEOS_VM_ID_LIMIT];
// The loader consumes BLUEPRINT_LAUNCH_STATES at guest entry. Keep the
// one-shot VMX minishell script separately so the guest can read it through
// its virtual filesystem after entry.
static BLUEPRINT_VMX_LAUNCH_SCRIPTS: [Mutex<Option<AllocString>>; TRUEOS_VM_ID_LIMIT] =
    [const { Mutex::new(None) }; TRUEOS_VM_ID_LIMIT];
static BLUEPRINT_PROCESS_CONTEXTS: [Mutex<Option<BlueprintProcessContext>>; TRUEOS_VM_ID_LIMIT] =
    [const { Mutex::new(None) }; TRUEOS_VM_ID_LIMIT];
// Exit reasons are control-plane results as well as lifecycle diagnostics.
// Keep the last result outside the process context so a waiter cannot lose it
// when a short-lived Blueprint reports an action and tears down immediately.
// Staging the next generation clears the mailbox before publishing its context.
static BLUEPRINT_EXIT_REASON_MAILBOXES: [Mutex<Option<AllocString>>; TRUEOS_VM_ID_LIMIT] =
    [const { Mutex::new(None) }; TRUEOS_VM_ID_LIMIT];
static BLUEPRINT_CONSOLE_INPUT_READY: [Signal<crate::wait::EmbassySpinRawMutex, ()>;
    TRUEOS_VM_ID_LIMIT] = [const { Signal::new() }; TRUEOS_VM_ID_LIMIT];
// Serialize every host-side terminal ownership transition for one VM across
// context publication, lifecycle detach/reattach, and lease park/reentry.
// Backend owner identities are intentionally compact (VM id plus target
// lifetime), so allowing two external claims for the same VM to overlap would
// make a stale rollback indistinguishable from the newer valid claim.
static BLUEPRINT_TERMINAL_TRANSITIONS: [Mutex<()>; TRUEOS_VM_ID_LIMIT] =
    [const { Mutex::new(()) }; TRUEOS_VM_ID_LIMIT];
static BLUEPRINT_CONSOLE_LOG_BUFFERS: [Mutex<Option<AllocString>>; TRUEOS_VM_ID_LIMIT] =
    [const { Mutex::new(None) }; TRUEOS_VM_ID_LIMIT];
static BLUEPRINT_LIFECYCLE_ARCHIVES: [Mutex<Option<AllocString>>; TRUEOS_VM_ID_LIMIT] =
    [const { Mutex::new(None) }; TRUEOS_VM_ID_LIMIT];
static BLUEPRINT_INSTANCE_IDENTITIES: [Mutex<Option<BlueprintInstanceIdentity>>;
    TRUEOS_VM_ID_LIMIT] = [const { Mutex::new(None) }; TRUEOS_VM_ID_LIMIT];
#[cfg(feature = "legacy_vm_checkpoint")]
static BLUEPRINT_REPLICATION_CHECKPOINTS: [Mutex<Option<BlueprintReplicationCheckpoint>>;
    TRUEOS_VM_ID_LIMIT] = [const { Mutex::new(None) }; TRUEOS_VM_ID_LIMIT];
// This host-owned template survives the guest's one-shot consumption of its
// launch state.  A same-archive child must never recover its ELF bytes by
// borrowing pointers from the parent Hull's private guest heap.
static BLUEPRINT_CHILD_TEMPLATES: [Mutex<Option<BlueprintChildTemplate>>; TRUEOS_VM_ID_LIMIT] =
    [const { Mutex::new(None) }; TRUEOS_VM_ID_LIMIT];
// The child VM owns the relationship record.  This makes lifecycle cleanup
// unambiguous when VM ids are reused: both sides are checked with generation.
static BLUEPRINT_CHILD_LINKS: [Mutex<Option<BlueprintChildLink>>; TRUEOS_VM_ID_LIMIT] =
    [const { Mutex::new(None) }; TRUEOS_VM_ID_LIMIT];
static BLUEPRINT_CHILD_HANDLE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

pub static mut VMXON_REGIONS: [VmxPage; TRUEOS_VM_CPU_SLOT_LIMIT] =
    [const { VmxPage([0u8; VMX_PAGE_SIZE]) }; TRUEOS_VM_CPU_SLOT_LIMIT];
pub static mut VMCS_REGIONS: [VmxPage; TRUEOS_VM_CPU_SLOT_LIMIT] =
    [const { VmxPage([0u8; VMX_PAGE_SIZE]) }; TRUEOS_VM_CPU_SLOT_LIMIT];
pub static mut HV_HOST_GDTS: [[u64; 8]; TRUEOS_VM_CPU_SLOT_LIMIT] =
    [[0u64; 8]; TRUEOS_VM_CPU_SLOT_LIMIT];
pub static mut HV_HOST_TSSS: [[u8; 104]; TRUEOS_VM_CPU_SLOT_LIMIT] =
    [[0u8; 104]; TRUEOS_VM_CPU_SLOT_LIMIT];

#[cfg(feature = "legacy_vm_checkpoint")]
pub use snapshot::{RestoreError, SaveError};

fn current_vmx_slot() -> Result<usize, &'static str> {
    let slot = crate::percpu::current_slot();
    if slot < TRUEOS_VM_CPU_SLOT_LIMIT {
        Ok(slot)
    } else {
        hvlogf(format_args!(
            "hv: vm{} reporting: vmx abort unresolved cpu slot={} limit={}",
            current_vm_id_for_log(),
            slot,
            TRUEOS_VM_CPU_SLOT_LIMIT
        ));
        Err("vmx cpu slot unresolved")
    }
}

fn current_vmx_pages() -> Result<(*mut u8, *mut u8), &'static str> {
    let slot = current_vmx_slot()?;
    unsafe {
        Ok((
            core::ptr::addr_of_mut!(VMXON_REGIONS[slot].0) as *mut u8,
            core::ptr::addr_of_mut!(VMCS_REGIONS[slot].0) as *mut u8,
        ))
    }
}

fn current_vmcs_page() -> Result<*mut u8, &'static str> {
    let slot = current_vmx_slot()?;
    unsafe { Ok(core::ptr::addr_of_mut!(VMCS_REGIONS[slot].0) as *mut u8) }
}

fn current_vmx_root_active() -> Result<bool, &'static str> {
    let slot = current_vmx_slot()?;
    Ok(VMX_ROOT_ACTIVE_BY_CPU[slot].load(Ordering::Acquire))
}


fn clear_active_eptp_for_vm(vm_id: u8) {
    if let Some(eptp) = ACTIVE_EPTP_BY_VM.get(vm_id as usize) {
        eptp.store(0, Ordering::Release);
    }
}

fn prepare_vmx_control_registers() -> Result<u32, &'static str> {
    let (compatible, has_msr, _, locked, _) = vmx_caps();
    if compatible && has_msr && !locked {
        unsafe {
            let mut val = Msr::new(vmx::IA32_FEATURE_CONTROL).read();
            val |= vmx::IA32_FEATURE_CONTROL_LOCK | vmx::IA32_FEATURE_CONTROL_VMX_OUTSIDE_SMX;
            Msr::new(vmx::IA32_FEATURE_CONTROL).write(val);
        }
    }

    let caps = status();
    if !caps.vendor_intel
        || !caps.has_msr
        || !caps.has_vmx
        || !caps.feature_control_locked
        || !caps.feature_control_vmx_outside_smx
    {
        return Err("vmx unsupported");
    }

    let cr0_fixed0 = unsafe { Msr::new(vmx::IA32_VMX_CR0_FIXED0).read() };
    let cr0_fixed1 = unsafe { Msr::new(vmx::IA32_VMX_CR0_FIXED1).read() };
    let cr4_fixed0 = unsafe { Msr::new(vmx::IA32_VMX_CR4_FIXED0).read() };
    let cr4_fixed1 = unsafe { Msr::new(vmx::IA32_VMX_CR4_FIXED1).read() };

    let mut cr0 = Cr0::read().bits();
    let mut cr4 = Cr4::read().bits();
    cr0 = (cr0 | cr0_fixed0) & cr0_fixed1;
    cr4 = (cr4 | cr4_fixed0) & cr4_fixed1;
    cr4 |= Cr4Flags::VIRTUAL_MACHINE_EXTENSIONS.bits();
    unsafe {
        Cr0::write(Cr0Flags::from_bits_truncate(cr0));
        Cr4::write(Cr4Flags::from_bits_truncate(cr4));
    }

    let basic = unsafe { Msr::new(vmx::IA32_VMX_BASIC).read() };
    Ok((basic & 0x7fff_ffff) as u32)
}

fn maybe_log_vmx_core_contract_summary(revision: u32) {
    const FIRST_VMX_SLOT: usize = 2;

    let topology_slots = crate::percpu::total_slots().min(TRUEOS_VM_CPU_SLOT_LIMIT);
    if topology_slots <= FIRST_VMX_SLOT {
        return;
    }

    let expected = topology_slots - FIRST_VMX_SLOT;
    let active = VMX_ROOT_ACTIVE_BY_CPU[FIRST_VMX_SLOT..topology_slots]
        .iter()
        .filter(|state| state.load(Ordering::Acquire))
        .count();
    if active != expected {
        return;
    }

    let mut min_pa = u64::MAX;
    let mut max_pa = 0;
    for pa in &VMXON_PA_BY_CPU[FIRST_VMX_SLOT..topology_slots] {
        let pa = pa.load(Ordering::Acquire);
        min_pa = min_pa.min(pa);
        max_pa = max_pa.max(pa);
    }

    if VMX_CORE_CONTRACT_SUMMARY_LOGGED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    hvlogf(format_args!(
        "hv: vmx core-contract summary slots={}..{} active={}/{} revision=0x{:08X} vmxon_pa_span=0x{:016X}-0x{:016X} vpid=required",
        FIRST_VMX_SLOT,
        topology_slots - 1,
        active,
        expected,
        revision,
        min_pa,
        max_pa
    ));
}

pub fn enter_vmx_root_for_current_cpu_contract() -> Result<(), &'static str> {
    let slot = current_vmx_slot()?;
    if slot <= 1 {
        return Ok(());
    }
    if VMX_ROOT_ACTIVE_BY_CPU[slot].load(Ordering::Acquire) {
        return Ok(());
    }

    let revision = prepare_vmx_control_registers()?;
    let (vmxon_va, _) = current_vmx_pages()?;
    unsafe {
        core::ptr::write_bytes(vmxon_va, 0, VMX_PAGE_SIZE);
        *(vmxon_va as *mut u32) = revision;
    }
    let vmxon_pa = kernel_va_to_pa(vmxon_va as u64).ok_or("vmxon pa")?;
    if !vmx::vmxon(vmxon_pa) {
        hvlogf(format_args!(
            "hv: vmx core-contract failed slot={} vmxon_pa=0x{:016X}",
            slot, vmxon_pa
        ));
        return Err("vmxon");
    }

    if let Err(error) = vpid::initialize_current_lane() {
        let vmxoff = vmx::vmxoff();
        hverrorf(format_args!(
            "hv: vmx core-contract failed slot={} stage=vpid reason={} cleanup_vmxoff={}",
            slot, error, vmxoff as u8
        ));
        return Err(error);
    }

    VMXON_PA_BY_CPU[slot].store(vmxon_pa, Ordering::Release);
    VMX_ROOT_ACTIVE_BY_CPU[slot].store(true, Ordering::Release);
    if slot >= 2 {
        crate::r::readiness::set(crate::r::readiness::VTHREAD_HW_TAG_READY);
    }
    maybe_log_vmx_core_contract_summary(revision);
    Ok(())
}

/// Tear down this CPU's generation-local VMX root state before FULLFORGET.
///
/// VM snapshots are portable envelopes on TRUEOSFS; VMXON and VMCS pages are
/// deliberately not carried into the replacement kernel.
pub fn leave_vmx_root_for_current_cpu_contract() -> Result<bool, &'static str> {
    let slot = current_vmx_slot()?;
    if slot <= 1 || !VMX_ROOT_ACTIVE_BY_CPU[slot].load(Ordering::Acquire) {
        return Ok(false);
    }
    vpid::prepare_current_lane_for_vmxoff()?;
    if !vmx::vmxoff() {
        return Err("vmxoff");
    }
    VMX_EXTERNAL_INTERRUPT_EXITING_BY_CPU[slot].store(false, Ordering::Release);
    VMXON_PA_BY_CPU[slot].store(0, Ordering::Release);
    VMX_ROOT_ACTIVE_BY_CPU[slot].store(false, Ordering::Release);
    vpid::mark_current_lane_offline()?;
    Ok(true)
}

fn vm_owner_cpu_slot(vm_id: u8) -> Option<u32> {
    let tagged = vm_id.checked_add(1)?;
    CURRENT_VM_ID_BY_CPU
        .iter()
        .position(|owner| owner.load(Ordering::Acquire) == tagged)
        .and_then(|slot| u32::try_from(slot).ok())
}

fn nudge_vm_control(
    vm_id: u8,
    action: crate::hv::control_kick::LifecycleKickAction,
    reason: &'static str,
) -> bool {
    let Some(vm) = vm_slot(vm_id) else {
        return false;
    };
    // The Hull can be outside VMX in a platform wait. Make lifecycle control
    // runnable before attempting the resident-guest interrupt path.
    let _ = crate::wait::platform_wake_vm_scope(vm_id);
    let Some(cpu_slot) = vm_owner_cpu_slot(vm_id) else {
        hvlogf(format_args!(
            "hv: vm{} lifecycle: {} kick deferred owner=none timer_fallback=1",
            vm_id, reason
        ));
        return false;
    };
    if !VMX_EXTERNAL_INTERRUPT_EXITING_BY_CPU
        .get(cpu_slot as usize)
        .map(|enabled| enabled.load(Ordering::Acquire))
        .unwrap_or(false)
    {
        hvlogf(format_args!(
            "hv: vm{} lifecycle: {} kick deferred slot={} extint_exit=0 timer_fallback=1",
            vm_id, reason, cpu_slot
        ));
        return false;
    }

    let generation = vm.run_generation.load(Ordering::Acquire);
    match crate::hv::control_kick::publish_and_send(cpu_slot, vm_id as usize, generation, action) {
        Ok(sequence) => {
            hvlogf(format_args!(
                "hv: vm{} lifecycle: {} kick targeted slot={} generation={} seq={}",
                vm_id, reason, cpu_slot, generation, sequence
            ));
            true
        }
        Err(error) => {
            hvwarnf(format_args!(
                "hv: vm{} lifecycle: {} kick failed slot={} generation={} error={:?} timer_fallback=1",
                vm_id, reason, cpu_slot, generation, error
            ));
            false
        }
    }
}

fn handle_external_interrupt_vmexit(vm_id: u8) -> Result<(), &'static str> {
    let info =
        crate::hv::vmx::read_vmexit_interruption_info().ok_or("external interrupt info missing")?;
    if !info.is_external_interrupt() {
        return Err("external interrupt exit carried non-external interruption info");
    }

    let vector = info.vector();
    if crate::hv::control_kick::mark_vmexit_delivery(vector) {
        if let Some(kick) = crate::hv::control_kick::pending_for_current_cpu() {
            let expected_generation = vm_slot(vm_id)
                .map(|vm| vm.run_generation.load(Ordering::Acquire))
                .unwrap_or(0);
            let valid = kick.vm_id == vm_id as usize && kick.generation == expected_generation;
            let _ = crate::hv::control_kick::consume_for_current_cpu(kick.sequence);
            if valid {
                hvlogf(format_args!(
                    "hv: vm{} lifecycle: kick consumed vector=0x{:02X} action={:?} generation={} seq={}",
                    vm_id, vector, kick.action, kick.generation, kick.sequence
                ));
            } else {
                hvwarnf(format_args!(
                    "hv: vm{} lifecycle: stale kick ignored vector=0x{:02X} kick_vm={} generation={}/{} seq={}",
                    vm_id, vector, kick.vm_id, kick.generation, expected_generation, kick.sequence
                ));
            }
        }
        return Ok(());
    }

    if crate::remote_work_wake::replay_vmexit_interrupt_through_host(vector) {
        return Ok(());
    }

    Err("external interrupt vector dispatch unavailable")
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum StartError {
    UnsupportedVmId,
    AlreadyRunning,
    ConsoleUnsupported,
    VmxUnsupported,
    MissingGuestModule,
    GuestMemoryUnavailable,
    NoVmSpawner,
    SpawnFailed,
    VgpuQuarantined,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum VmBootMode {
    Hull,
    Full,
}

#[derive(Copy, Clone)]
enum BlueprintMemoryClass {
    RamAssets,
    TokioRuntime,
    AudioPlayer,
    NetworkClient,
    NetworkServer,
    ServerRuntime,
    HeavyGraphics,
    Unknown,
}

impl BlueprintMemoryClass {
    const fn label(self) -> &'static str {
        match self {
            Self::RamAssets => "ram-assets",
            Self::TokioRuntime => "tokio-runtime",
            Self::AudioPlayer => "audio-player",
            Self::NetworkClient => "network-client",
            Self::NetworkServer => "network-server",
            Self::ServerRuntime => "server-runtime",
            Self::HeavyGraphics => "heavy-graphics",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Copy, Clone)]
struct BlueprintVmMemoryProfile {
    class: BlueprintMemoryClass,
    heap_lower_mib: usize,
    heap_recommended_mib: usize,
    heap_upper_mib: usize,
    stack_lower_mib: usize,
    stack_recommended_mib: usize,
    stack_upper_mib: usize,
}

#[derive(Clone)]
struct BlueprintPendingLaunchState {
    archive: AllocString,
    module_bytes: AllocVec<u8>,
    app_args: AllocVec<AllocString>,
    launch_script: Option<AllocString>,
    instance: BlueprintInstanceRequest,
    console_target: Option<MatrixTarget>,
    console_surface: BlueprintConsoleSurface,
    restored_identity: Option<BlueprintInstanceIdentity>,
    restored_app_fs_root: Option<AllocString>,
}

#[derive(Clone)]
pub struct BlueprintLaunchState {
    pub archive: AllocString,
    pub module_bytes: AllocVec<u8>,
    pub unpacked_bytes: AllocVec<u8>,
    pub app_args: AllocVec<AllocString>,
    /// Kernel-owned, one-shot VMX minishell input. It is exposed only through
    /// the per-VM virtual `vFile:launch` stream and never through argv.
    pub launch_script: Option<AllocString>,
    pub app_fs_root: AllocString,
    pub identity: BlueprintInstanceIdentity,
}

#[cfg(feature = "legacy_vm_checkpoint")]
pub(crate) struct BlueprintRelaunchState {
    archive: AllocString,
    module_bytes: AllocVec<u8>,
    app_args: AllocVec<AllocString>,
    launch_script: Option<AllocString>,
    app_fs_root: AllocString,
    identity: BlueprintInstanceIdentity,
    console_surface: BlueprintConsoleSurface,
}

#[derive(Clone)]
struct BlueprintChildTemplate {
    generation: u64,
    archive: AllocString,
    module_bytes: AllocVec<u8>,
}

struct BlueprintChildLink {
    handle: u64,
    parent_vm_id: u8,
    parent_generation: u64,
    child_generation: u64,
    state: u8,
    parent_to_child: VecDeque<AllocVec<u8>>,
    child_to_parent: VecDeque<AllocVec<u8>>,
    process_launch: Option<BlueprintChildProcessLaunch>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BlueprintInstanceRequest {
    pub name: Option<AllocString>,
    pub peer: Option<AllocString>,
}

impl BlueprintInstanceRequest {
    pub fn named(name: impl Into<AllocString>) -> Self {
        Self {
            name: Some(name.into()),
            peer: None,
        }
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub fn from_peer(name: impl Into<AllocString>, peer: impl Into<AllocString>) -> Self {
        Self {
            name: Some(name.into()),
            peer: Some(peer.into()),
        }
    }

    pub fn is_default(&self) -> bool {
        self.name.is_none() && self.peer.is_none()
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum BlueprintConsoleSurface {
    Text,
    Terminal,
}

pub(crate) const BLUEPRINT_VMX_MINISHELL_ARG: &str = "--vmx-minishell";

impl BlueprintConsoleSurface {
    fn is_terminal(self) -> bool {
        matches!(self, Self::Terminal)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum BlueprintConsoleRoute {
    Matrix,
    NetShellDirect,
}

/// Host-authoritative ownership state for a Blueprint terminal surface.
///
/// The epoch identifies one active terminal session. Parking preserves that
/// epoch as an opaque ticket. A Shell2 `tui` request allocates the next epoch,
/// but Shell2 keeps ownership until the guest poll accepts it.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum BlueprintTerminalLeaseState {
    Unsupported,
    /// Launch admission has selected a terminal-capable route, but Shell2 is
    /// still the visible/input owner.  The guest must explicitly claim this
    /// state through `terminal_lease_current(0)` before terminal bytes can
    /// cross the handoff boundary.
    Reserved,
    Active {
        epoch: u64,
        observed: bool,
        ready: bool,
    },
    Parked {
        ticket: u64,
    },
    ReentryRequested {
        ticket: u64,
        epoch: u64,
    },
}

impl BlueprintTerminalLeaseState {
    const fn for_surface(surface: BlueprintConsoleSurface) -> Self {
        match surface {
            BlueprintConsoleSurface::Text => Self::Unsupported,
            BlueprintConsoleSurface::Terminal => Self::Reserved,
        }
    }

    /// Line-input apps such as Hull keep the shell prompt instead of claiming
    /// a terminal UI. Their launch reservation must not block ShellIO stdin.
    const fn accepts_console_input(self, app_command_passthrough: bool) -> bool {
        !self.suppresses_terminal_output()
            || (matches!(self, Self::Reserved) && app_command_passthrough)
    }

    /// A parked/reentry transition deliberately leaves Shell2 interactive.
    /// Guest terminal paint must therefore never be routed into its Matrix
    /// surface until the lease becomes Active again.
    const fn suppresses_terminal_output(self) -> bool {
        matches!(
            self,
            Self::Reserved
                | Self::Parked { .. }
                | Self::ReentryRequested { .. }
        )
    }
}

#[repr(u64)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum BlueprintTerminalLeaseError {
    Unsupported = 1,
    NotActive = 2,
    Stale = 3,
    Detached = 4,
    Busy = 5,
}

impl BlueprintTerminalLeaseError {
    pub(crate) const fn code(self) -> u64 {
        self as u64
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum BlueprintTerminalReentryPoll {
    Pending,
    Ready(u64),
}

/// Pointer-free V1 terminal-surface record returned through the VMCall payload.
/// Its generation describes presentation identity, independently of the
/// terminal-lease epoch that describes ownership authority.
#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct BlueprintTerminalSurfaceSnapshot {
    pub(crate) generation: u64,
    pub(crate) cols: u32,
    pub(crate) rows: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum BlueprintTerminalReentryRequest {
    Requested { ticket: u64, epoch: u64 },
    AlreadyRequested,
    NotParked,
    Detached,
    Unsupported,
}

impl BlueprintConsoleRoute {
    const fn is_net_shell_direct(self) -> bool {
        matches!(self, Self::NetShellDirect)
    }
}

fn blueprint_uses_local_terminal_handoff(
    console_surface: BlueprintConsoleSurface,
    console_target: Option<&MatrixTarget>,
) -> bool {
    console_surface.is_terminal() && console_target.is_some_and(crate::shell3::tui::supports)
}

#[derive(Clone)]
pub(crate) struct BlueprintProcessContext {
    args: AllocVec<AllocString>,
    vars: BTreeMap<AllocString, AllocString>,
    console_target: Option<MatrixTarget>,
    #[cfg(feature = "legacy_vm_checkpoint")]
    console_surface: BlueprintConsoleSurface,
    console_attached: bool,
    app_command_passthrough: bool,
    terminal_lease: BlueprintTerminalLeaseState,
    console_input: VecDeque<u8>,
    console_input_since_ms: Option<u64>,
    exit_reason: Option<AllocString>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct BlueprintTerminalCleanup {
    context_present: bool,
    backend_release_expected: bool,
    backend_released: bool,
    matrix_unbind_expected: bool,
}

impl BlueprintTerminalCleanup {
    const fn empty() -> Self {
        Self {
            context_present: false,
            backend_release_expected: false,
            backend_released: true,
            matrix_unbind_expected: false,
        }
    }

    const fn complete(self) -> bool {
        !self.backend_release_expected || self.backend_released
    }

    const fn matrix_unbind_marker(self) -> &'static str {
        "not-needed"
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum StopError {
    UnsupportedVmId,
    #[cfg(feature = "legacy_vm_checkpoint")]
    VgpuQuarantined,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum EjectError {
    UnsupportedVmId,
    VmBusy,
    VgpuQuarantined,
}

/// How a live VM is preserved before its hull stops.
///
/// `Stop` writes a raw checkpoint and performs normal teardown. `Pause`
/// additionally retains the Blueprint lifecycle state needed to resume a
/// replicatable app through the F2 Apps surface.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum PreserveMode {
    Stop,
    #[cfg(feature = "legacy_vm_checkpoint")]
    Pause,
}

/// Why the host is asking a Blueprint to enter its quiescent checkpoint boundary.
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BlueprintPauseReason {
    Pause = 1,
    Replicate = 2,
    #[cfg(feature = "legacy_vm_checkpoint")]
    Migrate = 3,
}

#[cfg(feature = "legacy_vm_checkpoint")]
impl BlueprintPauseReason {
    const fn from_raw(raw: u8) -> Self {
        match raw { 2 => Self::Replicate, 3 => Self::Migrate, _ => Self::Pause }
    }
}

/// Host action to take after a Blueprint reaches its exact Ready boundary.
///
/// A pause retains the quiesced VM directly in memory. Snapshot-capable
/// lifecycle reasons additionally serialize that retained state into the
/// warm per-VM store. `apps store` is the explicit reboot-persistent commit.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum BlueprintReadyDisposition {
    #[cfg(feature = "legacy_vm_checkpoint")]
    Pause,
    #[cfg(feature = "legacy_vm_checkpoint")]
    Snapshot,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct BlueprintPreparePause {
    pub operation: u64,
    pub deadline_ms: u64,
    pub reason: BlueprintPauseReason,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlueprintInstanceIdentity {
    pub instance: [u8; 16],
    pub lineage: [u8; 16],
    pub generation: u64,
    pub clone: bool,
    pub name: Option<AllocString>,
    pub peer: Option<AllocString>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlueprintReplicationCheckpoint {
    pub operation: u64,
    pub version: u64,
    pub bytes: AllocVec<u8>,
}

#[derive(Copy, Clone, Debug)]
pub struct HvStatus {
    pub vendor_intel: bool,
    pub has_msr: bool,
    pub has_vmx: bool,
    pub feature_control_locked: bool,
    pub feature_control_vmx_outside_smx: bool,
    pub guest_module_present: bool,
    pub vm_id_limit: usize,
    pub running_count: usize,
    pub paused_count: usize,
    pub starting_count: usize,
    pub vm_shared_heap_total_bytes: usize,
    pub vm_shared_heap_free_bytes: usize,
    pub vm_shared_stack_bytes: usize,
    pub vm_shared_vmx_bytes: usize,
}

#[derive(Copy, Clone, Debug)]
pub struct HvVmState {
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub id: u8,
    pub supported: bool,
    pub running: bool,
    pub starting: bool,
    pub stop_requested: bool,
    pub pause_requested: bool,
    pub paused: bool,
    pub preserve_requested: bool,
    pub preserve_exit: bool,
    pub replicatable: bool,
    pub pause_latched: bool,
    pub pause_snapshot_ready: bool,
    #[cfg(feature = "legacy_vm_checkpoint")]
    pub prepare_pause_pending: bool,
    pub restore_inflight: bool,
}

#[inline]
fn current_vm_id_for_log() -> u8 {
    current_vm_id().unwrap_or(0)
}

#[inline]
fn vm_slot(vm_id: u8) -> Option<&'static TrueosVmId> {
    trueos_vm_ids.get(vm_id as usize)
}

/// Current incarnation of a VM id. Subsystems that retain per-Blueprint state
/// must pair this with the small numeric id so a later VM reuse cannot inherit
/// an older owner's lease.
pub(crate) fn vm_run_generation(vm_id: u8) -> Option<u64> {
    vm_slot(vm_id).map(|vm| vm.run_generation.load(Ordering::Acquire))
}


fn lifecycle_now_ms() -> u64 {
    let hz = embassy_time_driver::TICK_HZ.max(1);
    embassy_time_driver::now().saturating_mul(1000) / hz
}

pub(crate) fn blueprint_instance_identity(vm_id: u8) -> Option<BlueprintInstanceIdentity> {
    BLUEPRINT_INSTANCE_IDENTITIES
        .get(vm_id as usize)?
        .lock()
        .clone()
}

fn new_blueprint_uuid() -> [u8; 16] {
    let mut bytes = [0u8; 16];
    if !crate::tyche::fill_bytes(&mut bytes) {
        let seq = BLUEPRINT_LIFECYCLE_OPERATION_SEQ.fetch_add(1, Ordering::Relaxed);
        bytes[..8].copy_from_slice(&lifecycle_now_ms().to_le_bytes());
        bytes[8..].copy_from_slice(&seq.to_le_bytes());
    }
    // RFC 9562 UUIDv4 variant/version bits. Identity is opaque to the VM
    // protocol, but canonical UUID text makes filesystem inspection pleasant.
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    bytes
}

pub(crate) fn format_blueprint_uuid(bytes: &[u8; 16]) -> AllocString {
    alloc::format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15],
    )
}

fn assign_fresh_blueprint_identity(
    vm_id: u8,
    request: &BlueprintInstanceRequest,
) -> Option<BlueprintInstanceIdentity> {
    let uuid = new_blueprint_uuid();
    let identity = BlueprintInstanceIdentity {
        instance: uuid,
        lineage: uuid,
        generation: 1,
        clone: false,
        name: request.name.clone(),
        peer: request.peer.clone(),
    };
    *BLUEPRINT_INSTANCE_IDENTITIES.get(vm_id as usize)?.lock() = Some(identity.clone());
    Some(identity)
}

#[inline]
pub(crate) fn lifecycle_request_pending(vm_id: u8) -> bool {
    vm_slot(vm_id)
        .map(|vm| {
            vm.stop_req.load(Ordering::Acquire)
                || vm.preserve_req.load(Ordering::Acquire)
                || vm.preserve_exit.load(Ordering::Acquire)
        })
        .unwrap_or(true)
}

pub fn first_free_vm_id() -> Option<u8> {
    let vgpu_fence = crate::gpu::vgpu::hull_guest_reuse_fence_mask();
    for (idx, slot) in trueos_vm_ids.iter().enumerate() {
        if !slot.running.load(Ordering::Acquire)
            && !slot.starting.load(Ordering::Acquire)
            && !slot.pause_latched.load(Ordering::Acquire)
            && !slot.restore_inflight.load(Ordering::Acquire)
            && vgpu_fence & (1u64 << idx) == 0
        {
            return Some(idx as u8);
        }
    }
    None
}

fn blueprint_child_template(vm_id: u8, generation: u64) -> Option<BlueprintChildTemplate> {
    BLUEPRINT_CHILD_TEMPLATES
        .get(vm_id as usize)?
        .lock()
        .as_ref()
        .filter(|template| template.generation == generation)
        .cloned()
}

fn clear_blueprint_child_template(vm_id: u8) {
    if let Some(slot) = BLUEPRINT_CHILD_TEMPLATES.get(vm_id as usize) {
        let _ = slot.lock().take();
    }
}

fn blueprint_child_actor_is_parent(link: &BlueprintChildLink, vm_id: u8, generation: u64) -> bool {
    link.parent_vm_id == vm_id && link.parent_generation == generation
}

/// A process-shaped request whose execution boundary is a fresh VMX Hull.
#[derive(Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BlueprintChildProcessLaunch {
    pub program: AllocString,
    #[serde(default)]
    pub args: AllocVec<AllocString>,
    #[serde(default)]
    pub env: BTreeMap<AllocString, AllocString>,
    pub cwd: Option<AllocString>,
}

pub(crate) fn blueprint_child_spawn_program(parent_vm_id: u8, payload: &[u8]) -> Result<u64, i32> {
    let mut request: BlueprintChildProcessLaunch =
        serde_json::from_slice(payload).map_err(|_| BLUEPRINT_CHILD_ERR_INVALID)?;
    if request.program.is_empty()
        || request.program.contains('\0')
        || request.args.iter().any(|arg| arg.contains('\0'))
        || request
            .env
            .iter()
            .any(|(key, value)| key.is_empty() || key.contains(['=', '\0']) || value.contains('\0'))
    {
        return Err(BLUEPRINT_CHILD_ERR_INVALID);
    }
    crate::hv::with_guest_broker_context(parent_vm_id, || {
        if let Some(cwd) = request.cwd.as_ref() {
            request.cwd = Some(
                crate::r::io::env::resolve_fs_path(cwd, true).ok_or(BLUEPRINT_CHILD_ERR_INVALID)?,
            );
        }
        let name = request
            .program
            .rsplit('/')
            .next()
            .ok_or(BLUEPRINT_CHILD_ERR_INVALID)?;
        let archive = if name.ends_with(".bp") {
            AllocString::from(name)
        } else {
            alloc::format!("{name}.bp")
        };
        let bytes = if request.program.contains('/') {
            let path = crate::r::io::env::resolve_fs_path(&request.program, false)
                .ok_or(BLUEPRINT_CHILD_ERR_INVALID)?;
            let len = crate::r::io::kfs::read_file_len(&path)
                .map_err(|_| BLUEPRINT_CHILD_ERR_NOT_FOUND)?;
            if len > 512 * 1024 * 1024 {
                return Err(BLUEPRINT_CHILD_ERR_INVALID);
            }
            crate::r::io::kfs::read_file(&path).map_err(|_| BLUEPRINT_CHILD_ERR_NOT_FOUND)?
        } else {
            crate::app_db::get(&archive)
                .map_err(|_| BLUEPRINT_CHILD_ERR_NOT_FOUND)?
                .ok_or(BLUEPRINT_CHILD_ERR_NOT_FOUND)?
        };
        blueprint_child_spawn_archive(parent_vm_id, archive, bytes, &[], Some(request))
    })
}

/// Start the same Blueprint archive in a headless child Hull.  The returned
/// handle is opaque and generation-bound; `0` is reserved for the worker's
/// own parent endpoint in the other child calls.
pub(crate) fn blueprint_child_spawn(parent_vm_id: u8, initial_message: &[u8]) -> Result<u64, i32> {
    if initial_message.len() > BLUEPRINT_CHILD_MESSAGE_LIMIT {
        return Err(BLUEPRINT_CHILD_ERR_INVALID);
    }
    let Some(parent_generation) = vm_run_generation(parent_vm_id) else {
        return Err(BLUEPRINT_CHILD_ERR_INVALID);
    };
    let parent = vm_slot(parent_vm_id).ok_or(BLUEPRINT_CHILD_ERR_INVALID)?;
    if !parent.running.load(Ordering::Acquire) || parent.stop_req.load(Ordering::Acquire) {
        return Err(BLUEPRINT_CHILD_ERR_UNAVAILABLE);
    }
    let template = blueprint_child_template(parent_vm_id, parent_generation)
        .ok_or(BLUEPRINT_CHILD_ERR_UNAVAILABLE)?;
    blueprint_child_spawn_archive(
        parent_vm_id,
        template.archive,
        template.module_bytes,
        initial_message,
        None,
    )
}

fn blueprint_child_spawn_archive(
    parent_vm_id: u8,
    archive: AllocString,
    module_bytes: AllocVec<u8>,
    initial_message: &[u8],
    process_launch: Option<BlueprintChildProcessLaunch>,
) -> Result<u64, i32> {
    let parent_generation = vm_run_generation(parent_vm_id).ok_or(BLUEPRINT_CHILD_ERR_INVALID)?;
    let parent = vm_slot(parent_vm_id).ok_or(BLUEPRINT_CHILD_ERR_INVALID)?;
    if !parent.running.load(Ordering::Acquire) || parent.stop_req.load(Ordering::Acquire) {
        return Err(BLUEPRINT_CHILD_ERR_UNAVAILABLE);
    }
    let app_args = process_launch
        .as_ref()
        .map(|request| request.args.clone())
        .unwrap_or_else(|| alloc::vec![AllocString::from(BLUEPRINT_CHILD_WORKER_ARG)]);
    // Claim the VM's existing `starting` bit before publishing a relationship.
    // `first_free_vm_id` is only an observation; a concurrent Shell2 launch
    // could otherwise claim the same id between that observation and link
    // installation.
    let child_vm_id = reserve_blueprint_child_vm_id().ok_or(BLUEPRINT_CHILD_ERR_UNAVAILABLE)?;
    let child_generation = vm_run_generation(child_vm_id)
        .unwrap_or(0)
        .wrapping_add(1)
        .max(1);
    let handle = BLUEPRINT_CHILD_HANDLE_SEQUENCE
        .fetch_add(1, Ordering::AcqRel)
        .max(1);
    let mut parent_to_child = VecDeque::new();
    if !initial_message.is_empty() {
        parent_to_child.push_back(AllocVec::from(initial_message));
    }
    let link = BlueprintChildLink {
        handle,
        parent_vm_id,
        parent_generation,
        child_generation,
        state: BLUEPRINT_CHILD_STATE_STARTING,
        parent_to_child,
        child_to_parent: VecDeque::new(),
        process_launch,
    };
    let Some(link_slot) = BLUEPRINT_CHILD_LINKS.get(child_vm_id as usize) else {
        return Err(BLUEPRINT_CHILD_ERR_UNAVAILABLE);
    };
    *link_slot.lock() = Some(link);

    let result = start_blueprint_child_vm(
        child_vm_id,
        archive,
        module_bytes,
        app_args,
        BlueprintInstanceRequest::default(),
    );
    if result.is_err() {
        let _ = link_slot.lock().take();
        if let Some(vm) = vm_slot(child_vm_id) {
            vm.starting.store(false, Ordering::Release);
        }
        return Err(BLUEPRINT_CHILD_ERR_UNAVAILABLE);
    }
    Ok(handle)
}

fn reserve_blueprint_child_vm_id() -> Option<u8> {
    loop {
        let vm_id = first_free_vm_id()?;
        let vm = vm_slot(vm_id)?;
        let _control = vm.lifecycle_control.lock();
        if !vm.running.load(Ordering::Acquire) && !vm.starting.load(Ordering::Acquire) {
            vm.cooperative_stop.reset();
            teardown_diagnostics::set(vm_id, TeardownStage::Preparing);
            vm.stop_req.store(false, Ordering::Release);
            *vm.matrix_owner.lock() = None;
            vm.lifecycle_generation.fetch_add(1, Ordering::AcqRel);
            vm.starting.store(true, Ordering::Release);
            return Some(vm_id);
        }
    }
}

fn blueprint_child_find_link_for_actor(
    actor_vm_id: u8,
    actor_generation: u64,
    handle: u64,
) -> Option<(u8, bool)> {
    if handle == 0 {
        let link = BLUEPRINT_CHILD_LINKS.get(actor_vm_id as usize)?.lock();
        let link = link.as_ref()?;
        return (link.child_generation == actor_generation).then_some((actor_vm_id, false));
    }
    for (child_id, slot) in BLUEPRINT_CHILD_LINKS.iter().enumerate() {
        let link = slot.lock();
        let Some(link) = link.as_ref() else {
            continue;
        };
        if link.handle == handle
            && blueprint_child_actor_is_parent(link, actor_vm_id, actor_generation)
        {
            return Some((child_id as u8, true));
        }
    }
    None
}

pub(crate) fn blueprint_child_send(
    actor_vm_id: u8,
    handle: u64,
    bytes: &[u8],
) -> Result<usize, i32> {
    if bytes.len() > BLUEPRINT_CHILD_MESSAGE_LIMIT {
        return Err(BLUEPRINT_CHILD_ERR_INVALID);
    }
    let generation = vm_run_generation(actor_vm_id).ok_or(BLUEPRINT_CHILD_ERR_INVALID)?;
    let (child_vm_id, parent_side) =
        blueprint_child_find_link_for_actor(actor_vm_id, generation, handle)
            .ok_or(BLUEPRINT_CHILD_ERR_NOT_FOUND)?;
    let Some(slot) = BLUEPRINT_CHILD_LINKS.get(child_vm_id as usize) else {
        return Err(BLUEPRINT_CHILD_ERR_NOT_FOUND);
    };
    let mut guard = slot.lock();
    let link = guard.as_mut().ok_or(BLUEPRINT_CHILD_ERR_NOT_FOUND)?;
    if link.state >= BLUEPRINT_CHILD_STATE_EXITED {
        return Err(BLUEPRINT_CHILD_ERR_UNAVAILABLE);
    }
    let queue = if parent_side {
        &mut link.parent_to_child
    } else {
        &mut link.child_to_parent
    };
    if queue.len() >= BLUEPRINT_CHILD_QUEUE_LIMIT {
        return Err(BLUEPRINT_CHILD_ERR_QUEUE_FULL);
    }
    queue.push_back(AllocVec::from(bytes));
    Ok(bytes.len())
}

pub(crate) fn blueprint_child_receive(
    actor_vm_id: u8,
    handle: u64,
    out: &mut [u8],
) -> Result<usize, i32> {
    let generation = vm_run_generation(actor_vm_id).ok_or(BLUEPRINT_CHILD_ERR_INVALID)?;
    let (child_vm_id, parent_side) =
        blueprint_child_find_link_for_actor(actor_vm_id, generation, handle)
            .ok_or(BLUEPRINT_CHILD_ERR_NOT_FOUND)?;
    let Some(slot) = BLUEPRINT_CHILD_LINKS.get(child_vm_id as usize) else {
        return Err(BLUEPRINT_CHILD_ERR_NOT_FOUND);
    };
    let mut guard = slot.lock();
    let link = guard.as_mut().ok_or(BLUEPRINT_CHILD_ERR_NOT_FOUND)?;
    let queue = if parent_side {
        &mut link.child_to_parent
    } else {
        &mut link.parent_to_child
    };
    let Some(message) = queue.front() else {
        return Ok(0);
    };
    if out.len() < message.len() {
        return Ok(message.len());
    }
    let message = queue
        .pop_front()
        .expect("front message disappeared while link locked");
    out[..message.len()].copy_from_slice(message.as_slice());
    Ok(message.len())
}

pub(crate) fn blueprint_child_receive_stream(
    actor_vm_id: u8,
    handle: u64,
    out: &mut [u8],
) -> Result<usize, i32> {
    let generation = vm_run_generation(actor_vm_id).ok_or(BLUEPRINT_CHILD_ERR_INVALID)?;
    let (child_vm_id, parent_side) =
        blueprint_child_find_link_for_actor(actor_vm_id, generation, handle)
            .ok_or(BLUEPRINT_CHILD_ERR_NOT_FOUND)?;
    let mut guard = BLUEPRINT_CHILD_LINKS[child_vm_id as usize].lock();
    let link = guard.as_mut().ok_or(BLUEPRINT_CHILD_ERR_NOT_FOUND)?;
    let queue = if parent_side {
        &mut link.child_to_parent
    } else {
        &mut link.parent_to_child
    };
    let Some(message) = queue.front_mut() else {
        return Ok(0);
    };
    let len = out.len().min(message.len());
    out[..len].copy_from_slice(&message[..len]);
    if len == message.len() {
        queue.pop_front();
    } else {
        message.drain(..len);
    }
    Ok(len)
}

pub(crate) fn blueprint_child_status(actor_vm_id: u8, handle: u64) -> Result<u8, i32> {
    let generation = vm_run_generation(actor_vm_id).ok_or(BLUEPRINT_CHILD_ERR_INVALID)?;
    if handle == 0 {
        let link = BLUEPRINT_CHILD_LINKS
            .get(actor_vm_id as usize)
            .ok_or(BLUEPRINT_CHILD_ERR_NOT_FOUND)?
            .lock();
        let link = link.as_ref().ok_or(BLUEPRINT_CHILD_ERR_NOT_FOUND)?;
        if link.child_generation != generation {
            return Err(BLUEPRINT_CHILD_ERR_NOT_FOUND);
        }
        let Some(parent) = vm_slot(link.parent_vm_id) else {
            return Ok(BLUEPRINT_CHILD_STATE_EXITED);
        };
        if vm_run_generation(link.parent_vm_id) != Some(link.parent_generation) {
            return Ok(BLUEPRINT_CHILD_STATE_EXITED);
        }
        return Ok(if parent.running.load(Ordering::Acquire) {
            BLUEPRINT_CHILD_STATE_RUNNING
        } else if parent.starting.load(Ordering::Acquire) {
            BLUEPRINT_CHILD_STATE_STARTING
        } else {
            BLUEPRINT_CHILD_STATE_EXITED
        });
    }
    let (child_vm_id, _) = blueprint_child_find_link_for_actor(actor_vm_id, generation, handle)
        .ok_or(BLUEPRINT_CHILD_ERR_NOT_FOUND)?;
    let link = BLUEPRINT_CHILD_LINKS
        .get(child_vm_id as usize)
        .ok_or(BLUEPRINT_CHILD_ERR_NOT_FOUND)?
        .lock();
    Ok(link.as_ref().ok_or(BLUEPRINT_CHILD_ERR_NOT_FOUND)?.state)
}

pub(crate) fn blueprint_child_terminate(actor_vm_id: u8, handle: u64) -> Result<(), i32> {
    let generation = vm_run_generation(actor_vm_id).ok_or(BLUEPRINT_CHILD_ERR_INVALID)?;
    let (child_vm_id, parent_side) =
        blueprint_child_find_link_for_actor(actor_vm_id, generation, handle)
            .ok_or(BLUEPRINT_CHILD_ERR_NOT_FOUND)?;
    if !parent_side {
        return Err(BLUEPRINT_CHILD_ERR_INVALID);
    }
    let Some(slot) = BLUEPRINT_CHILD_LINKS.get(child_vm_id as usize) else {
        return Err(BLUEPRINT_CHILD_ERR_NOT_FOUND);
    };
    let mut link = slot.lock();
    let link = link.as_mut().ok_or(BLUEPRINT_CHILD_ERR_NOT_FOUND)?;
    if link.state < BLUEPRINT_CHILD_STATE_EXITED {
        link.state = BLUEPRINT_CHILD_STATE_STOPPING;
        let _ = stop(child_vm_id);
    }
    Ok(())
}

fn blueprint_child_lifecycle_cleanup(vm_id: u8, generation: u64, retain_for_resume: bool) {
    if !retain_for_resume {
        clear_blueprint_child_template(vm_id);
    }
    if let Some(slot) = BLUEPRINT_CHILD_LINKS.get(vm_id as usize) {
        if let Some(link) = slot.lock().as_mut()
            && link.child_generation == generation
        {
            link.state = BLUEPRINT_CHILD_STATE_EXITED;
            link.parent_to_child.clear();
            // Keep child-to-parent messages available after guest exit.  The
            // parent may drain a final result before it releases its own VM;
            // the record is reclaimed on parent teardown or VM-id reuse.
        }
    }
    // A parent owns the lifetime of its hidden children.  Requesting stop is
    // nonblocking; each child still performs the ordinary VM teardown path.
    for (child_id, slot) in BLUEPRINT_CHILD_LINKS.iter().enumerate() {
        let owned = slot
            .lock()
            .as_ref()
            .is_some_and(|link| blueprint_child_actor_is_parent(link, vm_id, generation));
        if owned {
            // No principal remains to observe this endpoint, so release the
            // queues now rather than retaining guest-provided bytes until the
            // numeric VM slot happens to be reused.
            let _ = slot.lock().take();
            let _ = stop(child_id as u8);
        }
    }
}

fn boot_mode_for_vm(vm_id: u8) -> VmBootMode {
    VM_BOOT_MODES
        .get(vm_id as usize)
        .map(|mode| *mode.lock())
        .unwrap_or(VmBootMode::Hull)
}

fn guest_entry_for_boot_mode(_boot_mode: VmBootMode) -> u64 {
    guest_launch_rip()
}

fn guest_fs_base_for_boot_mode(_boot_mode: VmBootMode) -> u64 {
    GUEST_FS_BASE_RESET
}

fn vm_activity_snapshot() -> (usize, usize) {
    let mut running_count = 0usize;
    let mut starting_count = 0usize;

    for slot in trueos_vm_ids.iter() {
        let running = slot.running.load(Ordering::Acquire);
        let starting = slot.starting.load(Ordering::Acquire);
        if running && !slot.execution_pause.paused() {
            running_count = running_count.saturating_add(1);
        }
        if starting {
            starting_count = starting_count.saturating_add(1);
        }
    }

    (running_count, starting_count)
}

pub fn vm_state(vm_id: u8) -> HvVmState {
    let Some(vm) = vm_slot(vm_id) else {
        return HvVmState {
            id: vm_id,
            supported: false,
            running: false,
            starting: false,
            stop_requested: false,
            pause_requested: false,
            paused: false,
            preserve_requested: false,
            preserve_exit: false,
            replicatable: false,
            pause_latched: false,
            pause_snapshot_ready: false,
            #[cfg(feature = "legacy_vm_checkpoint")]
            prepare_pause_pending: false,
            restore_inflight: false,
        };
    };
    expire_prepare_pause(vm_id, vm);
    let pause_latched = vm.pause_latched.load(Ordering::Acquire);
    #[cfg(feature = "legacy_vm_checkpoint")]
    let pause_snapshot_ready = pause_latched
        && crate::hv::store::current_committed_seq(vm_id)
            > vm.pause_store_seq.load(Ordering::Acquire);
    #[cfg(not(feature = "legacy_vm_checkpoint"))]
    let pause_snapshot_ready = false;
    HvVmState {
        id: vm_id,
        supported: true,
        running: vm.running.load(Ordering::Acquire),
        starting: vm.starting.load(Ordering::Acquire),
        stop_requested: vm.stop_req.load(Ordering::Acquire),
        pause_requested: vm.execution_pause.requested(),
        paused: vm.execution_pause.paused(),
        preserve_requested: vm.preserve_req.load(Ordering::Acquire),
        preserve_exit: vm.preserve_exit.load(Ordering::Acquire),
        replicatable: vm.replicatable.load(Ordering::Acquire),
        pause_latched,
        pause_snapshot_ready,
        #[cfg(feature = "legacy_vm_checkpoint")]
        prepare_pause_pending: vm.lifecycle_phase.load(Ordering::Acquire)
            == BLUEPRINT_LIFECYCLE_PHASE_PREPARE_PAUSE,
        restore_inflight: vm.restore_inflight.load(Ordering::Acquire),
    }
}

pub fn app_vm_archive(vm_id: u8) -> Option<AllocString> {
    if vm_slot(vm_id).is_none() {
        return None;
    }
    BLUEPRINT_LAUNCH_STATES
        .get(vm_id as usize)
        .and_then(|slot| slot.lock().as_ref().map(|state| state.archive.clone()))
        .or_else(|| {
            BLUEPRINT_LIFECYCLE_ARCHIVES
                .get(vm_id as usize)?
                .lock()
                .clone()
        })
}

pub fn app_vm_display_label(vm_id: u8) -> Option<AllocString> {
    let archive = app_vm_archive(vm_id)?;
    let identity = blueprint_instance_identity(vm_id);
    match identity {
        Some(identity) if identity.peer.is_some() || identity.name.is_some() => {
            let name = identity.name.as_deref().unwrap_or("unnamed");
            if let Some(peer) = identity.peer.as_deref() {
                Some(alloc::format!("{} [peer:{} / {}]", archive, peer, name))
            } else {
                Some(alloc::format!("{} [{}]", archive, name))
            }
        }
        _ => Some(archive),
    }
}




fn set_blueprint_lifecycle_capability(vm_id: u8, archive: &str, replicatable: bool) {
    let Some(vm) = vm_slot(vm_id) else {
        return;
    };
    let replicatable = cfg!(feature = "legacy_vm_checkpoint") && replicatable;
    vm.replicatable.store(replicatable, Ordering::Release);
    vm.pause_latched.store(false, Ordering::Release);
    vm.resume_prepared.store(false, Ordering::Release);
    vm.pause_store_seq.store(0, Ordering::Release);
    reset_prepare_pause(vm);
    if let Some(slot) = BLUEPRINT_LIFECYCLE_ARCHIVES.get(vm_id as usize) {
        *slot.lock() = replicatable.then(|| AllocString::from(archive));
    }
}

fn clear_blueprint_lifecycle_capability(vm_id: u8) {
    let Some(vm) = vm_slot(vm_id) else {
        return;
    };
    vm.replicatable.store(false, Ordering::Release);
    vm.pause_latched.store(false, Ordering::Release);
    vm.resume_prepared.store(false, Ordering::Release);
    vm.pause_store_seq.store(0, Ordering::Release);
    reset_prepare_pause(vm);
    if let Some(slot) = BLUEPRINT_LIFECYCLE_ARCHIVES.get(vm_id as usize) {
        let _ = slot.lock().take();
    }
}

pub(crate) fn current_vm_id() -> Option<u8> {
    // Guest-safe fast path: Hull guests share the host image but not the host
    // heap/percpu pages. The LAPIC-low table is fixed storage populated before
    // VM entry, so this avoids dereferencing GS-backed PerCpu state in guest.
    if let Some(vm_id) = current_vm_id_by_lapic_low() {
        return Some(vm_id);
    }

    let slot = crate::percpu::current_slot();
    let tagged = CURRENT_VM_ID_BY_CPU.get(slot)?.load(Ordering::Acquire);
    tagged.checked_sub(1)
}

pub fn vm_id_for_cpu_slot(slot: usize) -> Option<u8> {
    let tagged = CURRENT_VM_ID_BY_CPU.get(slot)?.load(Ordering::Acquire);
    tagged.checked_sub(1)
}

pub(crate) fn current_vm_id_by_lapic_low() -> Option<u8> {
    let lapic_id = crate::percpu::current_lapic_id_via_cpuid();
    let tagged = CURRENT_VM_ID_BY_LAPIC_LOW[(lapic_id & 0xFF) as usize].load(Ordering::Acquire);
    tagged.checked_sub(1)
}

pub(crate) fn current_hull_guest_context_vm_id() -> Option<u8> {
    let rsp: u64;
    unsafe {
        core::arch::asm!(
            "mov {}, rsp",
            out(reg) rsp,
            options(nomem, nostack, preserves_flags)
        );
    }
    if rsp < memory::GUEST_STACK_VA_BASE || rsp >= memory::GUEST_COMM_PAGE_VA {
        return None;
    }

    if let Some(tag) = crate::hv::vmcall::guest_comm_page_vm_id_tag()
        && tag != 0
    {
        let vm_id = tag.saturating_sub(1) as u8;
        if (vm_id as usize) < TRUEOS_VM_ID_LIMIT {
            return Some(vm_id);
        }
    }

    current_vm_id_by_lapic_low()
}

pub(crate) fn current_guest_execution_context_vm_id() -> Option<u8> {
    if let Some(vm_id) = current_hull_guest_context_vm_id() {
        return Some(vm_id);
    }

    if crate::r::threads::current_id().is_some() {
        return crate::r::threads::current_vm_id();
    }

    let slot = crate::percpu::current_slot();
    if let Some(tagged) = CURRENT_GUEST_BROKER_VM_ID_BY_CPU
        .get(slot)
        .map(|slot| slot.load(Ordering::Acquire))
        && let Some(vm_id) = tagged.checked_sub(1)
    {
        return Some(vm_id);
    }

    let domain = crate::r::kernel_task_domain::current();
    if matches!(
        domain.domain,
        crate::r::kernel_task_domain::KernelTaskDomain::VmBroker
            | crate::r::kernel_task_domain::KernelTaskDomain::TokioCarrier
            | crate::r::kernel_task_domain::KernelTaskDomain::VmGuestOwnedAlloc
    ) && let Some(vm_id) = domain.vm_id
    {
        return Some(vm_id);
    }

    None
}

pub(crate) fn with_guest_broker_context<R>(vm_id: u8, f: impl FnOnce() -> R) -> R {
    let slot = crate::percpu::current_slot();
    let Some(owner_slot) = CURRENT_GUEST_BROKER_VM_ID_BY_CPU.get(slot) else {
        return f();
    };
    let previous = owner_slot.swap(vm_id.saturating_add(1), Ordering::AcqRel);
    let result = f();
    owner_slot.store(previous, Ordering::Release);
    result
}

pub(crate) fn current_vm_lapic_low_tag_addr() -> u64 {
    let lapic_id = crate::percpu::current_lapic_id_via_cpuid();
    (&CURRENT_VM_ID_BY_LAPIC_LOW[(lapic_id & 0xFF) as usize] as *const AtomicU8) as u64
}

fn set_current_vm_id(vm_id: u8) {
    let slot_idx = crate::percpu::current_slot();
    if let Some(slot) = CURRENT_VM_ID_BY_CPU.get(slot_idx) {
        slot.store(vm_id.saturating_add(1), Ordering::Release);
    }
    let lapic_id = crate::percpu::current_lapic_id_via_cpuid();
    CURRENT_VM_ID_BY_LAPIC_LOW[(lapic_id & 0xFF) as usize]
        .store(vm_id.saturating_add(1), Ordering::Release);
}

fn clear_current_vm_id() {
    let slot_idx = crate::percpu::current_slot();
    if let Some(slot) = CURRENT_VM_ID_BY_CPU.get(slot_idx) {
        slot.store(0, Ordering::Release);
    }
    let lapic_id = crate::percpu::current_lapic_id_via_cpuid();
    CURRENT_VM_ID_BY_LAPIC_LOW[(lapic_id & 0xFF) as usize].store(0, Ordering::Release);
}

pub(crate) fn mark_blueprint_clean_exit(vm_id: u8) {
    if let Some(vm) = vm_slot(vm_id) {
        vm.clean_exit.store(true, Ordering::Release);
    }
}

#[inline]
fn guest_exception_summary() -> Option<(u8, &'static str, u64, u64, u64)> {
    let info = vmread(VMCS_VMEXIT_INTERRUPTION_INFO)?;
    if ((info >> 31) & 1) == 0 {
        return None;
    }

    let vector = (info & 0xFF) as u8;
    let kind = (info >> 8) & 0x7;
    if kind != 3 && kind != 5 && kind != 6 {
        return None;
    }

    let err_valid = ((info >> 11) & 1) != 0;
    let err = if err_valid {
        vmread(VMCS_VMEXIT_INTERRUPTION_ERROR_CODE).unwrap_or(0)
    } else {
        0
    };
    Some((vector, crate::hv::vmx::decode_exception_vector(vector), kind, info, err))
}

pub fn hvlogf(args: core::fmt::Arguments<'_>) {
    hvlog_at(log_os_core::LogLevel::Info, args);
}

pub fn hvtracef(args: core::fmt::Arguments<'_>) {
    hvlog_at(log_os_core::LogLevel::Trace, args);
}

pub fn hvwarnf(args: core::fmt::Arguments<'_>) {
    hvlog_at(log_os_core::LogLevel::Warn, args);
}

fn hvimportantf(args: core::fmt::Arguments<'_>) {
    hvlog_at(log_os_core::LogLevel::Important, args);
}

pub fn hverrorf(args: core::fmt::Arguments<'_>) {
    hvlog_at(log_os_core::LogLevel::Error, args);
}

fn hvlog_at(level: log_os_core::LogLevel, args: core::fmt::Arguments<'_>) {
    let mut line: String<HV_LOG_LINE> = String::new();
    let _ = line.write_fmt(args);
    if line.is_empty() {
        return;
    }

    if current_hull_guest_context_vm_id().is_some() {
        if !hvlog_console_enabled(level) {
            return;
        }
        hvlog_guest_context_write(level, line.as_str());
        return;
    }

    if hvlog_console_enabled(level) {
        crate::log_os::hypervisor_line(level, format_args!("{}\n", line.as_str()));
    }
}

fn hvlog_guest_context_write(level: log_os_core::LogLevel, line: &str) {
    let _ = trueos_vm::vmcall::net_tcp_write(b"[hv] [");
    let _ = trueos_vm::vmcall::net_tcp_write(crate::log_os::purpose_for_level(level).as_bytes());
    let _ = trueos_vm::vmcall::net_tcp_write(b"] ");
    let _ = trueos_vm::vmcall::net_tcp_write(line.as_bytes());
    let _ = trueos_vm::vmcall::net_tcp_write(b"\n");
}

fn hvlog_console_enabled(level: log_os_core::LogLevel) -> bool {
    crate::log_os::flags::HV_LOGS
        && crate::log_os::flags::area_log_enabled(crate::log_os::flags::LogArea::Hv, level)
}

pub fn status() -> HvStatus {
    let (vendor_intel, has_msr, has_vmx, fc_locked, fc_vmx_outside_smx) = vmx_caps();
    let (running_count, starting_count) = vm_activity_snapshot();
    let paused_count = trueos_vm_ids
        .iter()
        .filter(|vm| vm.running.load(Ordering::Acquire) && vm.execution_pause.paused())
        .count();
    let vm_heap = crate::allocators::hv_guest_heap_stats_total();
    HvStatus {
        vendor_intel,
        has_msr,
        has_vmx,
        feature_control_locked: fc_locked,
        feature_control_vmx_outside_smx: fc_vmx_outside_smx,
        guest_module_present: crate::limine::guest_kernel_bytes().is_some(),
        vm_id_limit: TRUEOS_VM_ID_LIMIT,
        running_count,
        paused_count,
        starting_count,
        vm_shared_heap_total_bytes: vm_heap.usable_total,
        vm_shared_heap_free_bytes: vm_heap.free_bytes,
        vm_shared_stack_bytes: memory::active_guest_stack_bytes_total(),
        vm_shared_vmx_bytes: core::mem::size_of::<VmxPage>() * 2 * TRUEOS_VM_CPU_SLOT_LIMIT,
    }
}

pub(crate) fn shutdown_diagnostic_lines(vm_id: u8) -> alloc::vec::Vec<AllocString> {
    let Some(vm) = vm_slot(vm_id) else {
        return alloc::vec::Vec::new();
    };
    let (stage, age_ms) = teardown_diagnostics::describe(vm_id);
    let mut lines = alloc::vec![alloc::format!(
        "apps: vm{} shutdown forced={} cleanup_owner={} guest_ack={} stage={} age_ms={} native_jobs={}",
        vm_id,
        vm.cooperative_stop.forced() as u8,
        vm.cooperative_stop.registered() as u8,
        vm.clean_exit.load(Ordering::Acquire) as u8,
        stage,
        age_ms,
        crate::r::blocking::guest_jobs_in_flight(vm_id),
    )];
    lines.extend(crate::r::blocking::diagnostics::lines(vm_id));
    lines
}

fn report_shutdown_diagnostics(vm_id: u8) {
    for line in shutdown_diagnostic_lines(vm_id) {
        crate::log_os::blueprint_important_line(format_args!("{}\n", line));
    }
}


#[cfg(feature = "legacy_vm_checkpoint")]
pub fn start(vm_id: u8, spawner: &Spawner, stack_mb: Option<usize>) -> Result<(), StartError> {
    let _ = spawner;
    start_with_mode(vm_id, VmBootMode::Hull, stack_mb, None, false)
}

pub fn start_blueprint_app_vm(
    vm_id: u8,
    spawner: &Spawner,
    archive: AllocString,
    module_bytes: AllocVec<u8>,
    app_args: AllocVec<AllocString>,
    launch_script: Option<AllocString>,
    instance: BlueprintInstanceRequest,
    console_target: Option<MatrixTarget>,
    console_surface: BlueprintConsoleSurface,
) -> Result<(), StartError> {
    let _ = spawner;
    start_with_mode(
        vm_id,
        VmBootMode::Hull,
        None,
        Some(BlueprintPendingLaunchState {
            archive,
            module_bytes,
            app_args,
            launch_script,
            instance,
            console_target,
            console_surface,
            restored_identity: None,
            restored_app_fs_root: None,
        }),
        false,
    )
}

fn start_blueprint_child_vm(
    vm_id: u8,
    archive: AllocString,
    module_bytes: AllocVec<u8>,
    app_args: AllocVec<AllocString>,
    instance: BlueprintInstanceRequest,
) -> Result<(), StartError> {
    start_with_mode(
        vm_id,
        VmBootMode::Hull,
        None,
        Some(BlueprintPendingLaunchState {
            archive,
            module_bytes,
            app_args,
            launch_script: None,
            instance,
            console_target: None,
            console_surface: BlueprintConsoleSurface::Text,
            restored_identity: None,
            restored_app_fs_root: None,
        }),
        true,
    )
}

fn start_with_mode(
    vm_id: u8,
    boot_mode: VmBootMode,
    stack_mb: Option<usize>,
    mut pending_blueprint: Option<BlueprintPendingLaunchState>,
    already_reserved: bool,
) -> Result<(), StartError> {
    let Some(vm) = vm_slot(vm_id) else {
        return Err(StartError::UnsupportedVmId);
    };

    if vm.running.load(Ordering::Acquire) || crate::r::blocking::guest_jobs_in_flight(vm_id) != 0 {
        return Err(StartError::AlreadyRunning);
    }
    if !crate::gpu::vgpu::hull_guest_storage_reusable(vm_id) {
        hvwarnf(format_args!(
            "hv: vm{} lifecycle: start rejected reason=vgpu-storage-quarantined action=retain-vm-slot-and-guest-pages-until-reset",
            vm_id
        ));
        return Err(StartError::VgpuQuarantined);
    }

    let mut matrix_admission = matrix::Admission::new(vm_id);
    {
        let _control = vm.lifecycle_control.lock();
        if vm.running.load(Ordering::Acquire) {
            return Err(StartError::AlreadyRunning);
        }
        if already_reserved {
            if !vm.starting.load(Ordering::Acquire) {
                return Err(StartError::AlreadyRunning);
            }
        } else {
            if vm.starting.load(Ordering::Acquire) {
                return Err(StartError::AlreadyRunning);
            }
        }
        if pending_blueprint.is_some() && vm.pause_latched.load(Ordering::Acquire) {
            hvwarnf(format_args!(
                "hv: vm{} lifecycle: fresh blueprint start rejected while pause is retained",
                vm_id
            ));
            return Err(StartError::AlreadyRunning);
        }
        if let Some(pending) = pending_blueprint.as_mut() {
            // Identity is established under lifecycle admission, before the VM
            // worker can execute. Child and replica launches enter here too.
            let target = matrix::bind(
                vm_id,
                &pending.archive,
                &pending.module_bytes,
                pending.console_target.take(),
            )
            .map_err(|_| StartError::ConsoleUnsupported)?;
            matrix_admission.bound(&target);
            pending.console_target = Some(target);
        }
        if pending_blueprint.is_none() && !blueprint_launch_active(vm_id) {
            *vm.matrix_owner.lock() = None;
        }
        if !already_reserved {
            vm.cooperative_stop.reset();
            vm.stop_req.store(false, Ordering::Release);
            teardown_diagnostics::set(vm_id, TeardownStage::Preparing);
            vm.lifecycle_generation.fetch_add(1, Ordering::AcqRel);
            vm.starting.store(true, Ordering::Release);
        }
    }

    let (compatible, has_msr, _, locked, _) = vmx_caps();
    if compatible && has_msr && !locked {
        unsafe {
            let mut val = Msr::new(vmx::IA32_FEATURE_CONTROL).read();
            val |= vmx::IA32_FEATURE_CONTROL_LOCK | vmx::IA32_FEATURE_CONTROL_VMX_OUTSIDE_SMX;
            Msr::new(vmx::IA32_FEATURE_CONTROL).write(val);
        }
    }

    let caps = status();
    if !caps.vendor_intel
        || !caps.has_msr
        || !caps.has_vmx
        || !caps.feature_control_locked
        || !caps.feature_control_vmx_outside_smx
    {
        hvwarnf(format_args!(
            "hv: start failed: vendor={} msr={} vmx={} locked={} outside_smx={}",
            caps.vendor_intel,
            caps.has_msr,
            caps.has_vmx,
            caps.feature_control_locked,
            caps.feature_control_vmx_outside_smx
        ));
        let r0 = __cpuid(0);
        hvlogf(format_args!("hv: cpuid0 ebx=0x{:X} ecx=0x{:X} edx=0x{:X}", r0.ebx, r0.ecx, r0.edx));
        let r1 = __cpuid(1);
        hvlogf(format_args!("hv: cpuid1 ecx=0x{:X} edx=0x{:X}", r1.ecx, r1.edx));
        vm.starting.store(false, Ordering::Release);
        return Err(StartError::VmxUnsupported);
    }

    if boot_mode == VmBootMode::Full && crate::limine::guest_kernel_bytes().is_none() {
        vm.starting.store(false, Ordering::Release);
        return Err(StartError::MissingGuestModule);
    }

    let is_blueprint_start = pending_blueprint.is_some();
    if is_blueprint_start {
        // `starting` is the fresh-incarnation latch: no other launcher can
        // claim this VM id after it is set.  Retain an offline incarnation's
        // resources for recovery until this point, then destroy them before
        // applying the new Blueprint's memory profile.  In particular, an
        // initialized guest heap cannot be resized in place and would
        // otherwise make launch success depend on the previous occupant's
        // profile.
        if let Err(error) = eject_offline_vm(vm_id, true) {
            hvwarnf(format_args!(
                "hv: vm{} lifecycle: fresh blueprint resource reset failed ({:?})",
                vm_id, error
            ));
            vm.starting.store(false, Ordering::Release);
            return Err(match error {
                EjectError::UnsupportedVmId => StartError::UnsupportedVmId,
                EjectError::VmBusy => StartError::AlreadyRunning,
                EjectError::VgpuQuarantined => StartError::VgpuQuarantined,
            });
        }
        hvlogf(format_args!(
            "hv: vm{} lifecycle: fresh blueprint start reset offline resources",
            vm_id
        ));
    }

    if vm.pause_latched.load(Ordering::Acquire) && memory::active_restore_meta(vm_id).is_none() {
        hvwarnf(format_args!(
            "hv: vm{} lifecycle: retained pause has no active restore metadata",
            vm_id
        ));
        vm.starting.store(false, Ordering::Release);
        return Err(StartError::GuestMemoryUnavailable);
    }

    if memory::active_restore_meta(vm_id).is_none() {
        let requested_stack_mb = stack_mb.unwrap_or(memory::guest_stack_default_mb());
        let active_stack_mb = memory::clamp_guest_stack_mb(requested_stack_mb);
        if memory::prepare_guest_stack_mb_for_vm(vm_id, active_stack_mb).is_err() {
            vm.starting.store(false, Ordering::Release);
            return Err(StartError::GuestMemoryUnavailable);
        }
    } else {
        hvlogf(format_args!(
            "hv: vm{} lifecycle: retained restored stack bytes={}",
            vm_id,
            memory::active_guest_stack_bytes_for_vm(vm_id)
        ));
    }

    vm.marker_seen.store(false, Ordering::Release);
    if let Some(mode) = VM_BOOT_MODES.get(vm_id as usize) {
        *mode.lock() = boot_mode;
    }
    if let Some(pending) = pending_blueprint {
        if let Some(slot) = BLUEPRINT_PENDING_LAUNCH_STATES.get(vm_id as usize) {
            *slot.lock() = Some(pending);
        } else {
            vm.starting.store(false, Ordering::Release);
            return Err(StartError::UnsupportedVmId);
        }
    }

    let profile = VmLaneProfile::vm_default();
    let mut target = match pick_vm_hull_lane() {
        Ok(target) => target,
        Err(error) => {
            clear_blueprint_pending_launch(vm_id);
            vm.starting.store(false, Ordering::Release);
            hvwarnf(format_args!(
                "hv: vm{} lane pick failed: role={} placement={} reason={}",
                vm_id,
                profile.role_name(),
                profile.placement_name(),
                error.as_str()
            ));
            return Err(StartError::NoVmSpawner);
        }
    };

    // Preserve the VM hull execution contract:
    // actual guest work must stay on HV-reserved VM lanes only, never on BSP
    // and never on the AP1 UI/service lane.
    if !profile.requires_reserved_vm_lane() || !target.supports(profile) {
        clear_blueprint_pending_launch(vm_id);
        vm.starting.store(false, Ordering::Release);
        hvwarnf(format_args!(
            "hv: vm{} lane rejected: role={} placement={} slot={} requires reserved VM lane on AP2+",
            vm_id,
            profile.role_name(),
            profile.placement_name(),
            target.slot
        ));
        return Err(StartError::NoVmSpawner);
    }

    hvlogf(format_args!(
        "hv: vm{} lane: mode={:?} role={} placement={} slot={} kind={} stack_mib={}",
        vm_id,
        boot_mode,
        profile.role_name(),
        profile.placement_name(),
        target.slot,
        target.core_kind_name(),
        memory::active_guest_stack_mb_for_vm(vm_id)
    ));
    crate::log!(
        "app-vm-run-queue: lane picked vm={} mode={:?} slot={} kind={} stack_mib={}\n",
        vm_id,
        boot_mode,
        target.slot,
        target.core_kind_name(),
        memory::active_guest_stack_mb_for_vm(vm_id)
    );
    let generation = vm
        .run_generation
        .fetch_add(1, Ordering::AcqRel)
        .wrapping_add(1);
    hvlogf(format_args!(
        "hv: vm{} lifecycle: generation={} assigned slot={}",
        vm_id, generation, target.slot
    ));
    target.lease.set_vm_owner(vm_id);

    match vm_task(vm_id, target.lease) {
        Ok(token) => {
            if memory::active_restore_meta(vm_id).is_some()
                && vm.pause_latched.load(Ordering::Acquire)
            {
                if vm.replicatable.load(Ordering::Acquire) {
                    prepare_replicatable_resume(vm_id);
                    hvlogf(format_args!(
                        "hv: vm{} lifecycle: resume prepared; retention held until guest identity handshake",
                        vm_id
                    ));
                } else {
                    vm.pause_latched.store(false, Ordering::Release);
                    hvlogf(format_args!(
                        "hv: vm{} lifecycle: non-Blueprint resume committed before VM wake",
                        vm_id
                    ));
                }
            }
            let wake_sent = target.spawner.spawn_and_wake_remote(token);
            hvlogf(format_args!(
                "hv: vm{} lane spawn submitted: role={} placement={} slot={} wake={}",
                vm_id,
                profile.role_name(),
                profile.placement_name(),
                target.slot,
                wake_sent as u8
            ));
            crate::log!(
                "app-vm-run-queue: vm task submitted vm={} slot={} wake={}\n",
                vm_id,
                target.slot,
                wake_sent as u8
            );
        }
        Err(_) => {
            clear_blueprint_pending_launch(vm_id);
            vm.starting.store(false, Ordering::Release);
            return Err(StartError::SpawnFailed);
        }
    }
    matrix_admission.commit();
    Ok(())
}

pub fn stop(vm_id: u8) -> Result<bool, StopError> {
    stop_matching_generation(vm_id, None)
}


/// Park the live Hull at a VM-exit boundary. The task, VMCS, VPID, EPT,
/// native stacks and carrier lease remain owned by the same incarnation.
pub fn pause(vm_id: u8) -> Result<bool, StopError> {
    pause_matching_owner(vm_id, None)
}

pub(crate) fn pause_for_matrix_slot(
    vm_id: u8,
    owner: &crate::shell3::MatrixSlotLease,
) -> Result<bool, StopError> {
    pause_matching_owner(vm_id, Some(owner))
}

fn pause_matching_owner(
    vm_id: u8,
    owner: Option<&crate::shell3::MatrixSlotLease>,
) -> Result<bool, StopError> {
    let Some(vm) = vm_slot(vm_id) else {
        return Err(StopError::UnsupportedVmId);
    };
    let _control = vm.lifecycle_control.lock();
    if owner.is_some_and(|owner| vm.matrix_owner.lock().as_ref() != Some(owner)) {
        return Ok(false);
    }
    if !vm.running.load(Ordering::Acquire)
        || vm.starting.load(Ordering::Acquire)
        || vm.stop_req.load(Ordering::Acquire)
        || vm.pause_latched.load(Ordering::Acquire)
        || !vm.execution_pause.request()
    {
        return Ok(false);
    }
    crate::r::blocking::pause_guest_jobs(vm_id);
    vm.execution_pause_deadline_ms
        .store(lifecycle_now_ms().saturating_add(PAUSE_CHECKPOINT_TIMEOUT_MS), Ordering::Release);
    vm.execution_pause_io_wait_logged
        .store(false, Ordering::Release);
    VM_CONTROL_WAITS[vm_id as usize].notify_all();
    crate::r::threads::wake_guest_control(vm_id);
    nudge_vm_control(vm_id, control_kick::LifecycleKickAction::Nudge, "pause");
    hvimportantf(format_args!(
        "hv: vm{} lifecycle: pause requested contract=finish-or-abort-streamed-writes checkpoint=vm-exit-and-filesystem timeout_ms={} slot=retained",
        vm_id, PAUSE_CHECKPOINT_TIMEOUT_MS
    ));
    Ok(true)
}

/// Continue the same retained execution. No loader, restore, new identity or
/// new carrier allocation participates in this transition.
pub fn resume(vm_id: u8) -> Result<bool, StopError> {
    resume_matching_owner(vm_id, None)
}

pub(crate) fn resume_for_matrix_slot(
    vm_id: u8,
    owner: &crate::shell3::MatrixSlotLease,
) -> Result<bool, StopError> {
    resume_matching_owner(vm_id, Some(owner))
}

fn resume_matching_owner(
    vm_id: u8,
    owner: Option<&crate::shell3::MatrixSlotLease>,
) -> Result<bool, StopError> {
    let Some(vm) = vm_slot(vm_id) else {
        return Err(StopError::UnsupportedVmId);
    };
    let _control = vm.lifecycle_control.lock();
    if owner.is_some_and(|owner| vm.matrix_owner.lock().as_ref() != Some(owner)) {
        return Ok(false);
    }
    if !vm.running.load(Ordering::Acquire)
        || vm.stop_req.load(Ordering::Acquire)
        || !vm.execution_pause.requested()
    {
        return Ok(false);
    }
    assert!(crate::r::blocking::resume_guest_jobs(
        vm_id,
        vm.run_generation.load(Ordering::Acquire)
    ));
    vm.execution_pause.resume();
    crate::r::threads::wake_guest_control(vm_id);
    VM_CONTROL_WAITS[vm_id as usize].notify_all();
    hvlogf(format_args!("hv: vm{} lifecycle: resume requested same-vmcs=1 same-slot=1", vm_id));
    Ok(true)
}

pub(crate) fn guest_pause_requested(vm_id: u8) -> bool {
    vm_slot(vm_id).is_some_and(|vm| vm.execution_pause.requested())
}

pub(crate) fn guest_native_pause_requested(vm_id: u8) -> bool {
    if vm_slot(vm_id).is_some_and(|vm| vm.execution_pause.paused()) {
        return true;
    }
    guest_pause_requested(vm_id)
        && crate::r::fs::request_broker::pending_for_vm(vm_id) == 0
        && crate::r::fs::request_broker::open_writes_for_vm(vm_id) == 0
}

async fn park_vm_execution(vm_id: u8) -> bool {
    let Some(vm) = vm_slot(vm_id) else {
        return false;
    };
    loop {
        let queue = &VM_CONTROL_WAITS[vm_id as usize];
        let observed = queue.observe();
        if immediate_stop_requested(vm) {
            return false;
        }
        if !vm.execution_pause.requested() {
            return true;
        }
        if !vm.execution_pause.paused() {
            crate::r::threads::wake_guest_control(vm_id);
            let _control = vm.lifecycle_control.lock();
            if !vm.execution_pause.requested() {
                continue;
            }
            let fs_pending = crate::r::fs::request_broker::pending_for_vm(vm_id);
            let fs_writes = crate::r::fs::request_broker::open_writes_for_vm(vm_id);
            let async_pending = crate::r::io::async_fs_cabi::pending_for_owner(
                crate::r::io::async_fs_cabi::owner_for_vm(vm_id),
            );
            if lifecycle_now_ms() >= vm.execution_pause_deadline_ms.load(Ordering::Acquire) {
                if !vm.stop_req.load(Ordering::Acquire) {
                    assert!(crate::r::blocking::resume_guest_jobs(
                        vm_id,
                        vm.run_generation.load(Ordering::Acquire)
                    ));
                    vm.execution_pause.resume();
                    crate::r::threads::wake_guest_control(vm_id);
                    hvimportantf(format_args!(
                        "hv: vm{} lifecycle: pause cancelled reason=checkpoint-timeout native_jobs={} fs_pending={} fs_streams={} async_fs_pending={} state=running contract=finish-or-abort-before-pause",
                        vm_id,
                        crate::r::blocking::guest_jobs_in_flight(vm_id),
                        fs_pending,
                        fs_writes,
                        async_pending
                    ));
                }
                return true;
            }
            if fs_pending != 0 || fs_writes != 0 || async_pending != 0 {
                if !vm
                    .execution_pause_io_wait_logged
                    .swap(true, Ordering::AcqRel)
                {
                    hvimportantf(format_args!(
                        "hv: vm{} lifecycle: pause checkpoint waiting reason=filesystem fs_pending={} fs_streams={} async_fs_pending={} contract=stream-must-finish-or-abort guest=continues slot=retained",
                        vm_id, fs_pending, fs_writes, async_pending
                    ));
                }
                // A streamed write may belong to the Hull itself. It must
                // execute its remaining write/finish/abort calls; parking it
                // here would freeze the owner of the root write lease.
                return true;
            }
            if crate::r::blocking::guest_jobs_quiescent(vm_id) {
                if vm.execution_pause.acknowledge() {
                    if vm.execution_pause_io_wait_logged.load(Ordering::Acquire) {
                        hvimportantf(format_args!(
                            "hv: vm{} lifecycle: pause filesystem checkpoint cleared fs_pending=0 fs_streams=0 async_fs_pending=0",
                            vm_id
                        ));
                    }
                    hvimportantf(format_args!(
                        "hv: vm{} lifecycle: paused ram=retained carrier=retained vmcs=retained filesystem=quiescent",
                        vm_id
                    ));
                }
            }
        }
        if vm.execution_pause.paused() {
            if vm.execution_pause.requested() && !immediate_stop_requested(vm) {
                queue.wait_after(observed).await;
            }
        } else {
            Timer::after(EmbassyDuration::from_millis(1)).await;
        }
    }
}

#[task(pool_size = 64)]
async fn stop_cleanup_deadline(vm_id: u8, generation: u64) {
    Timer::after(EmbassyDuration::from_millis(STOP_CLEANUP_TIMEOUT_MS)).await;
    let Some(vm) = vm_slot(vm_id) else {
        return;
    };
    let _control = vm.lifecycle_control.lock();
    if vm.lifecycle_generation.load(Ordering::Acquire) == generation
        && vm.stop_req.load(Ordering::Acquire)
        && (vm.running.load(Ordering::Acquire) || vm.starting.load(Ordering::Acquire))
    {
        hvwarnf(format_args!(
            "hv: vm{} lifecycle: stop cleanup deadline reached; forcing discard",
            vm_id
        ));
        force_stop(vm_id, vm);
    }
}

fn force_stop(vm_id: u8, vm: &TrueosVmId) {
    vm.cooperative_stop.force();
    vm.stop_req.store(true, Ordering::Release);
    vm.execution_pause.resume();
    vm.preserve_req.store(false, Ordering::Release);
    vm.pause_latched.store(false, Ordering::Release);
    vm.preserve_exit.store(false, Ordering::Release);
    reset_prepare_pause(vm);
    clear_blueprint_lifecycle_capability(vm_id);
    crate::r::blocking::close_guest_jobs(vm_id);
    let streams = crate::r::fs::request_broker::request_release_vm(vm_id);
    if streams != 0 {
        hvimportantf(format_args!(
            "hv: vm{} lifecycle: discard filesystem rollback requested fs_streams={} owner=bsp slot=retained-until-abort",
            vm_id, streams
        ));
    }
    crate::r::threads::wake_guest_control(vm_id);
    VM_CONTROL_WAITS[vm_id as usize].notify_all();
    nudge_vm_control(vm_id, control_kick::LifecycleKickAction::Stop, "kill");
}

fn stop_matching_generation(vm_id: u8, generation: Option<u64>) -> Result<bool, StopError> {
    stop_matching_control(vm_id, generation, None)
}

pub(crate) fn stop_for_matrix_slot(
    vm_id: u8,
    owner: &crate::shell3::MatrixSlotLease,
) -> Result<bool, StopError> {
    stop_matching_control(vm_id, None, Some(owner))
}

fn stop_matching_control(
    vm_id: u8,
    generation: Option<u64>,
    owner: Option<&crate::shell3::MatrixSlotLease>,
) -> Result<bool, StopError> {
    let Some(vm) = vm_slot(vm_id) else {
        return Err(StopError::UnsupportedVmId);
    };

    let _control = vm.lifecycle_control.lock();
    if owner.is_some_and(|owner| vm.matrix_owner.lock().as_ref() != Some(owner)) {
        return Ok(false);
    }
    if generation.is_some_and(|run| vm.lifecycle_generation.load(Ordering::Acquire) != run) {
        return Ok(false);
    }
    if vm.running.load(Ordering::Acquire) || vm.starting.load(Ordering::Acquire) {
        let cooperative = vm.cooperative_stop.request();
        let first = !vm.stop_req.swap(true, Ordering::AcqRel);
        if cooperative && vm.execution_pause.requested() {
            assert!(crate::r::blocking::resume_guest_jobs(
                vm_id,
                vm.run_generation.load(Ordering::Acquire)
            ));
            vm.execution_pause.resume();
            crate::r::threads::wake_guest_control(vm_id);
        }
        if cooperative && first {
            let deadline =
                stop_cleanup_deadline(vm_id, vm.lifecycle_generation.load(Ordering::Acquire));
            if let (Some(worker), Ok(task)) = (crate::workers::pick_background_spawner(), deadline)
            {
                worker.spawn(task);
            } else {
                force_stop(vm_id, vm);
            }
        } else if !cooperative {
            force_stop(vm_id, vm);
        }
        VM_CONTROL_WAITS[vm_id as usize].notify_all();
        crate::log_os::blueprint_important_line(format_args!(
            "hv: vm{} lifecycle: stop requested cooperative={} native_jobs={} cleanup=guest-before-drain\n",
            vm_id,
            cooperative as u8,
            crate::r::blocking::guest_jobs_in_flight(vm_id)
        ));
        nudge_vm_control(vm_id, crate::hv::control_kick::LifecycleKickAction::Stop, "stop");
        report_shutdown_diagnostics(vm_id);
        Ok(true)
    } else {
        hvwarnf(format_args!("hv: vm{} lifecycle: stop ignored (not running)", vm_id));
        Ok(false)
    }
}


pub(crate) fn kill_for_matrix_slot(
    vm_id: u8,
    owner: &crate::shell3::MatrixSlotLease,
) -> Result<bool, EjectError> {
    kill_matching_owner(vm_id, Some(owner))
}

fn kill_matching_owner(
    vm_id: u8,
    owner: Option<&crate::shell3::MatrixSlotLease>,
) -> Result<bool, EjectError> {
    let Some(vm) = vm_slot(vm_id) else {
        return Err(EjectError::UnsupportedVmId);
    };
    let _control = vm.lifecycle_control.lock();
    // Slot attachment retirement may race an old guest finishing and another
    // slot reusing its vmid. Match the Matrix lifetime, never just that vmid.
    if owner.is_some_and(|owner| vm.matrix_owner.lock().as_ref() != Some(owner)) {
        return Ok(false);
    }
    if !vm.running.load(Ordering::Acquire) && !vm.starting.load(Ordering::Acquire) {
        return eject_offline_vm(vm_id, false);
    }
    force_stop(vm_id, vm);
    crate::log_os::blueprint_important_line(format_args!(
        "hv: vm{} lifecycle: kill requested cleanup=skipped checkpoint=none native_jobs={}\n",
        vm_id,
        crate::r::blocking::guest_jobs_in_flight(vm_id)
    ));
    nudge_vm_control(vm_id, crate::hv::control_kick::LifecycleKickAction::Stop, "kill");
    report_shutdown_diagnostics(vm_id);
    Ok(true)
}

pub(crate) fn guest_kill_requested(vm_id: u8) -> bool {
    vm_slot(vm_id).is_some_and(|vm| vm.cooperative_stop.forced())
}

/// Control registration/polling is scoped to the current VM incarnation.
pub(crate) fn blueprint_stop_control(vm_id: u8, operation: u32) -> i32 {
    let Some(vm) = vm_slot(vm_id) else {
        return -1;
    };
    if !vm.running.load(Ordering::Acquire) {
        return -1;
    }
    match operation {
        0 => {
            if vm.cooperative_stop.register() {
                0
            } else {
                -1
            }
        }
        1 => vm.cooperative_stop.requested() as i32,
        _ => -1,
    }
}

fn immediate_stop_requested(vm: &TrueosVmId) -> bool {
    vm.stop_req.load(Ordering::Acquire) && !vm.cooperative_stop.registered()
}

/// Destroy an offline retained VM and its warm checkpoint. Named persistent
/// images are independent and remain on TRUEOSFS.
#[cfg(feature = "legacy_vm_checkpoint")]
pub fn eject(vm_id: u8) -> Result<bool, EjectError> {
    eject_offline_vm(vm_id, false)
}

/// Destroy one offline VM incarnation. A fresh Blueprint start calls this
/// after atomically claiming the slot with `starting`; interactive ejects must
/// still reject every starting VM.
fn eject_offline_vm(vm_id: u8, allow_starting: bool) -> Result<bool, EjectError> {
    let Some(vm) = vm_slot(vm_id) else {
        return Err(EjectError::UnsupportedVmId);
    };
    if vm.running.load(Ordering::Acquire)
        || (!allow_starting && vm.starting.load(Ordering::Acquire))
        || vm.restore_inflight.load(Ordering::Acquire)
    {
        return Err(EjectError::VmBusy);
    }
    if crate::r::blocking::close_guest_jobs(vm_id) != 0 {
        return Err(EjectError::VmBusy);
    }
    let (_, vgpu_quarantined, _) = crate::gpu::vgpu::release_hull_guest(vm_id);
    if vgpu_quarantined != 0 || !crate::gpu::vgpu::hull_guest_storage_reusable(vm_id) {
        hvwarnf(format_args!(
            "hv: vm{} lifecycle: eject rejected reason=vgpu-storage-quarantined retained_devices={} action=retain-vm-slot-and-guest-pages-until-reset",
            vm_id, vgpu_quarantined
        ));
        return Err(EjectError::VgpuQuarantined);
    }
    let had_state = vm.pause_latched.load(Ordering::Acquire) || blueprint_launch_active(vm_id);
    #[cfg(feature = "legacy_vm_checkpoint")]
    let had_state = had_state || crate::hv::store::has_committed_vm(vm_id);
    clear_blueprint_pending_launch(vm_id);
    memory::release_guest_rel_exec_for_vm(vm_id);
    let launch = take_blueprint_launch(vm_id);
    drop(launch);
    clear_blueprint_launch_script(vm_id);
    clear_blueprint_process_context(vm_id);
    if let Some(slot) = BLUEPRINT_INSTANCE_IDENTITIES.get(vm_id as usize) {
        let _ = slot.lock().take();
    }
    clear_blueprint_lifecycle_capability(vm_id);
    clear_blueprint_replication_checkpoint(vm_id);
    let _ = crate::ai::lumen_service::close(vm_id);
    #[cfg(feature = "gridpaper")]
    let _ = crate::r::services::gridpaper_service::release_owner_lifecycle(vm_id);
    let _ = crate::r::services::media_service::release_vm(vm_id);
    let _ = crate::ui4::release_owner_resources(crate::ui4::WindowOwner::Vm(vm_id));
    memory::clear_snapshot_state_for_vm(vm_id);
    clear_active_eptp_for_vm(vm_id);
    let _ = memory::release_guest_hull_rw_for_vm(vm_id);
    let _ = memory::release_guest_stack_for_vm(vm_id);
    let heap_configured = crate::allocators::hv_guest_heap_stats_if_configured(vm_id).is_some();
    if heap_configured && !crate::allocators::release_hv_guest_heap_for_vm(vm_id) {
        // Keep pause/store state latched so this VM slot cannot be selected for
        // a new occupant after an allocator-level GPU pin or arena-release
        // failure. An unconfigured heap needs no release and is not an error.
        vm.pause_latched.store(true, Ordering::Release);
        hvwarnf(format_args!(
            "hv: vm{} lifecycle: eject rejected reason=vgpu-storage-quarantined action=retain-vm-slot-and-guest-pages-until-reset",
            vm_id
        ));
        return Err(EjectError::VgpuQuarantined);
    }
    #[cfg(feature = "legacy_vm_checkpoint")]
    let _ = crate::hv::store::eject_warm(vm_id);
    vm.pause_latched.store(false, Ordering::Release);
    vm.resume_prepared.store(false, Ordering::Release);
    vm.pause_store_seq.store(0, Ordering::Release);
    reset_prepare_pause(vm);
    if !allow_starting {
        matrix::retire_vm(vm_id);
    }
    Ok(had_state)
}

fn ceil_mib(bytes: usize) -> usize {
    bytes.saturating_add(MIB - 1) / MIB
}

fn clamp_mib(value: usize, lower: usize, upper: usize) -> usize {
    value.max(lower).min(upper)
}

fn round_pow2_mib(value: usize) -> usize {
    value.max(1).next_power_of_two()
}

fn import_name_has(imports: &[crate::hv::blueprint::ElfImport<'_>], needle: &str) -> bool {
    imports.iter().any(|import| import.name.contains(needle))
}

fn imports_libc_tcp_listener(imports: &[crate::hv::blueprint::ElfImport<'_>]) -> bool {
    let has = |name| imports.iter().any(|import| import.name == name);
    has("bind") && has("listen") && (has("accept") || has("accept4"))
}

fn archive_has(archive: &str, needle: &str) -> bool {
    archive.contains(needle)
}

fn classify_blueprint_memory(
    archive: &str,
    raw_payload_len: usize,
    stats: crate::hv::blueprint::ElfAllocStats,
    imports: &[crate::hv::blueprint::ElfImport<'_>],
) -> BlueprintMemoryClass {
    // RAM asset ingestion retains a decoded TAR while redb builds its database.
    // The codec import identifies that peak independently of executable size or
    // incidental listener symbols pulled in by the networking runtime.
    if import_name_has(imports, "trueos_cabi_archive_lz4_decode_start_v1") {
        return BlueprintMemoryClass::RamAssets;
    }

    let audio_player_signal = archive_has(archive, "scope-tui")
        || archive_has(archive, "scope_tui")
        || archive_has(archive, "aud-player-scope-tui")
        || archive_has(archive, "aud_player_scope_tui")
        || import_name_has(imports, "trueos_cabi_audio_")
        || import_name_has(imports, "audio_open_playback")
        || import_name_has(imports, "audio_write_i16");
    if audio_player_signal {
        return BlueprintMemoryClass::AudioPlayer;
    }

    let network_server_signal = archive_has(archive, "server")
        || import_name_has(imports, "trueos_mio_tcp_listener_")
        || import_name_has(imports, "tcp_listener")
        || imports_libc_tcp_listener(imports);
    if network_server_signal {
        return BlueprintMemoryClass::NetworkServer;
    }

    let server_signal = archive_has(archive, "horizon")
        || archive_has(archive, "game")
        || import_name_has(imports, "pthread_create")
        || import_name_has(imports, "pthread_join");
    if server_signal {
        return BlueprintMemoryClass::ServerRuntime;
    }

    let network_signal = archive_has(archive, "weather")
        || archive_has(archive, "currency")
        || archive_has(archive, "reqwest")
        || archive_has(archive, "http")
        || archive_has(archive, "https")
        || import_name_has(imports, "trueos_mio_")
        || import_name_has(imports, "dns_resolve")
        || import_name_has(imports, "net_fetch")
        || import_name_has(imports, "tcp_stream")
        || import_name_has(imports, "tokio_spawn_blocking");
    if network_signal {
        return BlueprintMemoryClass::NetworkClient;
    }

    // `img.bp` has a tiny executable but expands compressed source images into
    // RGBA8 guest allocations. Classify the viewer by role so its heap is not
    // derived from the Blueprint artifact size alone.
    let heavy_graphics_signal = archive_has(archive, "img.bp")
        || archive_has(archive, "mandelbrot")
        || archive_has(archive, "skybox")
        || archive_has(archive, "particle")
        || archive_has(archive, "virgl")
        || stats.alloc_bytes > 4 * MIB
        || raw_payload_len > 8 * MIB;
    if heavy_graphics_signal {
        return BlueprintMemoryClass::HeavyGraphics;
    }

    let tokio_signal = archive_has(archive, "tokio")
        || import_name_has(imports, "trueos_tokio_")
        || import_name_has(imports, "tokio_");
    if tokio_signal {
        return BlueprintMemoryClass::TokioRuntime;
    }

    BlueprintMemoryClass::Unknown
}

fn estimate_blueprint_memory_profile(
    archive: &str,
    module: &crate::hv::blueprint::BlueprintModule<'_>,
    unpacked: &[u8],
    imports: &[crate::hv::blueprint::ElfImport<'_>],
) -> BlueprintVmMemoryProfile {
    let stats = crate::hv::blueprint::elf_alloc_stats(unpacked).unwrap_or_default();
    let class = classify_blueprint_memory(archive, module.raw_payload_len, stats, imports);
    let base_live_mib = ceil_mib(module.raw_payload_len).max(ceil_mib(stats.alloc_bytes));
    // A retained-scene process simultaneously owns the packed module, its
    // relocated REL image, and Picasso's RAM database. Scale the minimum with
    // the image so a large GLB cannot fall back to a heap that cannot retain
    // those three live representations.
    let heavy_graphics_lower_mib =
        round_pow2_mib(base_live_mib.saturating_mul(4).saturating_add(128))
            .max(128)
            .min(1024);

    let (heap_lower, heap_recommended, heap_upper, stack_lower, stack_recommended, stack_upper) =
        match class {
            // Admit the 1 GiB whole-file read ceiling plus runtime/loader
            // headroom. Refuse a smaller arena instead of failing mid-preload.
            // This is Blueprint backing RAM, not XP process virtual memory.

            // Voxygen's ~190 MiB image, ~424 MiB staging TAR, and redb's old
            // and new backing buffers overlap during growth. Reserve room for
            // the full import; do not accept the former 1 GiB server budget.
            BlueprintMemoryClass::RamAssets => (3072, 3072, 3072, 16, 32, 128),
            BlueprintMemoryClass::TokioRuntime => (
                64,
                round_pow2_mib(base_live_mib.saturating_mul(12).saturating_add(64)).max(128),
                256,
                8,
                16,
                64,
            ),
            BlueprintMemoryClass::AudioPlayer => (
                128,
                round_pow2_mib(base_live_mib.saturating_mul(24).saturating_add(128)).max(256),
                256,
                8,
                16,
                64,
            ),
            BlueprintMemoryClass::NetworkClient => (
                64,
                round_pow2_mib(base_live_mib.saturating_mul(32).saturating_add(128)).max(512),
                512,
                8,
                16,
                64,
            ),
            BlueprintMemoryClass::NetworkServer => (
                128,
                round_pow2_mib(base_live_mib.saturating_mul(64).saturating_add(256)).max(512),
                1024,
                16,
                32,
                64,
            ),
            BlueprintMemoryClass::ServerRuntime => (
                512,
                round_pow2_mib(base_live_mib.saturating_mul(96).saturating_add(1024)).max(4096),
                4096,
                16,
                64,
                128,
            ),
            BlueprintMemoryClass::HeavyGraphics => (
                heavy_graphics_lower_mib,
                round_pow2_mib(base_live_mib.saturating_mul(16).saturating_add(128)).max(256),
                crate::allcaps::blueprint::HEAVY_GRAPHICS_HEAP_UPPER_MIB,
                16,
                32,
                128,
            ),
            BlueprintMemoryClass::Unknown => (64, 128, 512, 8, 16, 64),
        };

    BlueprintVmMemoryProfile {
        class,
        heap_lower_mib: heap_lower,
        heap_recommended_mib: clamp_mib(heap_recommended, heap_lower, heap_upper),
        heap_upper_mib: heap_upper,
        stack_lower_mib: stack_lower,
        stack_recommended_mib: clamp_mib(stack_recommended, stack_lower, stack_upper),
        stack_upper_mib: stack_upper,
    }
}

fn log_blueprint_memory_profile_info(profile: BlueprintVmMemoryProfile) {
    let level = if matches!(profile.class, BlueprintMemoryClass::RamAssets) {
        log_os_core::LogLevel::Important
    } else {
        log_os_core::LogLevel::Info
    };
    crate::log_os::blueprint_line(
        level,
        format_args!(
            "apps: profile {} heap={}/{}/{}MiB stack={}/{}/{}MiB\n",
            profile.class.label(),
            profile.heap_lower_mib,
            profile.heap_recommended_mib,
            profile.heap_upper_mib,
            profile.stack_lower_mib,
            profile.stack_recommended_mib,
            profile.stack_upper_mib
        ),
    );
}

fn take_blueprint_pending_launch(vm_id: u8) -> Option<BlueprintPendingLaunchState> {
    BLUEPRINT_PENDING_LAUNCH_STATES
        .get(vm_id as usize)?
        .lock()
        .take()
}

fn clear_blueprint_pending_launch(vm_id: u8) {
    if let Some(slot) = BLUEPRINT_PENDING_LAUNCH_STATES.get(vm_id as usize) {
        let _ = slot.lock().take();
    }
}

fn log_blueprint_launch_line(_target: Option<&MatrixTarget>, args: core::fmt::Arguments<'_>) {
    let line = alloc::format!("{}", args);
    crate::log_os::blueprint_line(log_os_core::LogLevel::Info, format_args!("{}\n", line.as_str()));
}

fn prepare_blueprint_launch_on_lane(
    vm_id: u8,
    pending: BlueprintPendingLaunchState,
) -> Result<(), AllocString> {
    let target = pending.console_target.clone();
    let log = |args: core::fmt::Arguments<'_>| log_blueprint_launch_line(target.as_ref(), args);
    log(format_args!("apps: vm{} preparing {} on AP lane", vm_id, pending.archive.as_str()));

    let host_alloc_guard = crate::allocators::enter_host_alloc_domain_current_cpu();
    let module = crate::hv::blueprint::parse_blueprint(pending.module_bytes.as_slice())
        .map_err(|err| alloc::format!("app-vm parse failed: {}", err))?;
    let replicatable_tagged = module.is_replicatable();
    let unpacked_bytes = crate::hv::blueprint::unpack_blueprint(&module)
        .map_err(|err| alloc::format!("app-vm unpack failed: {}", err))?;

    if !unpacked_bytes.starts_with(b"\x7fELF")
        || !matches!(crate::hv::blueprint::elf_type_name(unpacked_bytes.as_slice()), Some("REL"))
    {
        return Err(AllocString::from("only ELF REL blueprints are supported for app-vm launch"));
    }

    let imports = crate::hv::blueprint::elf_imports(unpacked_bytes.as_slice()).unwrap_or_default();
    let lifecycle_protocol = import_name_has(imports.as_slice(), "trueos_cabi_lifecycle_poll")
        && import_name_has(imports.as_slice(), "trueos_cabi_lifecycle_ready")
        && import_name_has(imports.as_slice(), "trueos_cabi_lifecycle_identity");
    let replicatable = replicatable_tagged && lifecycle_protocol;
    if replicatable_tagged && !lifecycle_protocol {
        log(format_args!(
            "apps: replicatable tag ignored: Blueprint does not import lifecycle poll/Ready/identity ABI"
        ));
    }
    let profile = estimate_blueprint_memory_profile(
        pending.archive.as_str(),
        &module,
        unpacked_bytes.as_slice(),
        imports.as_slice(),
    );
    let console_surface = pending.console_surface;
    log(format_args!("apps: console surface {:?}", console_surface));
    log_blueprint_memory_profile_info(profile);

    // Reserve the guest's large contiguous arenas before decoding and copying
    // embedded assets into the host filesystem. Toolchain Blueprints carry
    // hundreds of MiB of assets, and materializing those first can fragment the
    // PMM enough that even the profile's lower-bound arena becomes unavailable.
    if !crate::allocators::prepare_hv_guest_heap_for_vm(
        vm_id,
        profile.heap_recommended_mib.saturating_mul(MIB),
        profile.heap_lower_mib.saturating_mul(MIB),
    ) {
        return Err(AllocString::from("app-vm heap profile allocation failed"));
    }
    if memory::prepare_guest_stack_mb_for_vm(vm_id, profile.stack_recommended_mib).is_err() {
        return Err(AllocString::from("app-vm stack profile allocation failed"));
    }

    let identity = if let Some(identity) = pending.restored_identity.clone() {
        let Some(slot) = BLUEPRINT_INSTANCE_IDENTITIES.get(vm_id as usize) else {
            return Err(AllocString::from("app-vm instance identity unavailable"));
        };
        *slot.lock() = Some(identity.clone());
        identity
    } else {
        assign_fresh_blueprint_identity(vm_id, &pending.instance)
            .ok_or_else(|| AllocString::from("app-vm instance identity unavailable"))?
    };
    let instance_guid = format_blueprint_uuid(&identity.instance);
    let app_fs_root = if let Some(root) = pending.restored_app_fs_root.clone() {
        root
    } else if pending.instance.is_default() {
        crate::hv::blueprint::app_fs_root_for_archive(
            pending.archive.as_str(),
            pending.module_bytes.as_slice(),
        )
    } else {
        crate::hv::blueprint::app_fs_root_for_named_instance(
            pending.archive.as_str(),
            pending.instance.name.as_deref().unwrap_or("unnamed"),
            pending.instance.peer.as_deref(),
            instance_guid.as_str(),
        )
    };
    let asset_stats = {
        let result = crate::hv::blueprint::materialize_embedded_assets(
            unpacked_bytes.as_slice(),
            app_fs_root.as_str(),
        )
        .map_err(|err| alloc::format!("app-vm asset materialization failed: {}", err));
        result?
    };
    if let Some(stats) = asset_stats {
        log(format_args!(
            "apps: embedded assets materialized entries={} bytes={}",
            stats.entries, stats.bytes
        ));
    }
    if crate::hv::blueprint::uses_volatile_codex_files(pending.archive.as_str()) {
        log(format_args!(
            "apps: vm{} preparing boot-local Codex files from {}",
            vm_id, app_fs_root
        ));
        crate::r::io::kfs::prepare_volatile_files(app_fs_root.as_str())
            .map_err(|err| alloc::format!("Codex RAM files preparation failed: {:?}", err))?;
    }
    drop(host_alloc_guard);

    if !memory::arm_guest_rel_exec_for_vm(vm_id) {
        return Err(AllocString::from("app-vm REL execute policy unavailable"));
    }

    let lifecycle_archive = pending.archive.clone();
    if let Err(err) = stage_blueprint_launch(
        vm_id,
        BlueprintLaunchState {
            archive: pending.archive,
            module_bytes: pending.module_bytes,
            unpacked_bytes,
            app_args: pending.app_args,
            launch_script: pending.launch_script,
            app_fs_root,
            identity,
        },
        pending.console_target,
        console_surface,
    ) {
        memory::release_guest_rel_exec_for_vm(vm_id);
        return Err(alloc::format!("app-vm stage failed: {:?}", err));
    }
    set_blueprint_lifecycle_capability(vm_id, lifecycle_archive.as_str(), replicatable);

    crate::log!(
        "app-vm-run-queue: AP prep ok vm={} stack_mib={}\n",
        vm_id,
        profile.stack_recommended_mib
    );
    Ok(())
}

pub fn stage_blueprint_launch(
    vm_id: u8,
    state: BlueprintLaunchState,
    console_target: Option<MatrixTarget>,
    console_surface: BlueprintConsoleSurface,
) -> Result<(), StartError> {
    let Some(slot) = BLUEPRINT_LAUNCH_STATES.get(vm_id as usize) else {
        return Err(StartError::UnsupportedVmId);
    };
    let Some(process_slot) = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize) else {
        return Err(StartError::UnsupportedVmId);
    };
    let Some(exit_reason_mailbox) = BLUEPRINT_EXIT_REASON_MAILBOXES.get(vm_id as usize) else {
        return Err(StartError::UnsupportedVmId);
    };
    let Some(transition_slot) = BLUEPRINT_TERMINAL_TRANSITIONS.get(vm_id as usize) else {
        return Err(StartError::UnsupportedVmId);
    };
    let target = {
        let vm = vm_slot(vm_id).ok_or(StartError::UnsupportedVmId)?;
        let _control = vm.lifecycle_control.lock();
        matrix::bind(vm_id, &state.archive, &state.module_bytes, console_target)
            .map_err(|_| StartError::ConsoleUnsupported)?
    };
    let console_target = Some(target);
    // This outer gate is held only across synchronous state/backend calls; no
    // await or guest execution is allowed while it is held.
    let _transition = transition_slot.lock();
    let Some(launch_script_slot) = BLUEPRINT_VMX_LAUNCH_SCRIPTS.get(vm_id as usize) else {
        return Err(StartError::UnsupportedVmId);
    };
    let app_fs_root = state.app_fs_root.clone();
    let mut app_command_passthrough = state
        .app_args
        .iter()
        .any(|arg| arg == BLUEPRINT_VMX_MINISHELL_ARG);
    let mut trueosfs_scope = false;
    if let Ok(module) = crate::hv::blueprint::parse_blueprint(state.module_bytes.as_slice()) {
        trueosfs_scope = module.has_trueosfs_scope();
        if let Ok(unpacked) = crate::hv::blueprint::unpack_blueprint(&module)
            && let Ok(imports) = crate::hv::blueprint::elf_imports(unpacked.as_slice())
        {
            app_command_passthrough |= imports
                .iter()
                .any(|import| import.name == "trueos_cabi_shell_attached_read_byte");
        }
    }
    let local_terminal_handoff =
        blueprint_uses_local_terminal_handoff(console_surface, console_target.as_ref());
    if console_surface.is_terminal() && !local_terminal_handoff {
        hvwarnf(format_args!(
            "hv: vm{} console route: terminal surface has no exact-owner backend",
            vm_id
        ));
        return Err(StartError::ConsoleUnsupported);
    }
    let direct_terminal_handoff = false;
    let console_route = if direct_terminal_handoff {
        BlueprintConsoleRoute::NetShellDirect
    } else {
        BlueprintConsoleRoute::Matrix
    };
    let console_target_present = console_target.is_some();
    let generation = vm_run_generation(vm_id).ok_or(StartError::UnsupportedVmId)?;
    let Some(template_slot) = BLUEPRINT_CHILD_TEMPLATES.get(vm_id as usize) else {
        return Err(StartError::UnsupportedVmId);
    };
    // Retain only archive bytes, not guest pointers or presentation state.
    // The child service later uses this immutable host copy to relaunch the
    // same archive on another Hull/VMX lane.
    crate::allocators::with_host_alloc_domain(|| {
        *template_slot.lock() = Some(BlueprintChildTemplate {
            generation,
            archive: state.archive.clone(),
            module_bytes: state.module_bytes.clone(),
        });
    });
    let Some(guest_state) = crate::allocators::with_hv_guest_alloc_domain(vm_id, || state.clone())
    else {
        clear_blueprint_child_template(vm_id);
        return Err(StartError::GuestMemoryUnavailable);
    };
    let mut process_context = BlueprintProcessContext {
        args: crate::hv::blueprint::build_process_args(
            state.archive.as_str(),
            state.app_args.as_slice(),
        ),
        vars: crate::hv::blueprint::build_process_env(
            state.archive.as_str(),
            state.module_bytes.as_slice(),
            Some(app_fs_root.as_str()),
            Some(&state.identity),
            state.launch_script.as_deref(),
            trueosfs_scope,
        ),
        console_target,
        #[cfg(feature = "legacy_vm_checkpoint")]
        console_surface,
        console_attached: true,
        app_command_passthrough,
        terminal_lease: BlueprintTerminalLeaseState::for_surface(console_surface),
        console_input: VecDeque::new(),
        console_input_since_ms: None,
        exit_reason: None,
    };
    if let Some(link) = BLUEPRINT_CHILD_LINKS.get(vm_id as usize).and_then(|slot| {
        let guard = slot.lock();
        guard
            .as_ref()
            .filter(|link| link.child_generation == generation)
            .and_then(|link| link.process_launch.clone())
    }) {
        // Platform identity, access scope and channel selection are kernel-owned.
        for (key, value) in link.env {
            if !key.starts_with("TRUEOS_") {
                process_context.vars.insert(key, value);
            }
        }
        if let Some(cwd) = link.cwd {
            process_context
                .vars
                .insert(AllocString::from("TRUEOS_APP_FS_ROOT"), cwd);
        }
        process_context
            .vars
            .insert(AllocString::from("TRUEOS_MCP_STDIO"), AllocString::from("1"));
    }
    // Materialize the C environment while host-owned launch map nodes are
    // readable, before any guest loader or CABI can request getenv/environ.
    if crate::std_abi_shim::initialize_blueprint_environment(
        vm_id,
        process_context
            .vars
            .iter()
            .map(|(key, value)| (key.as_bytes(), value.as_bytes())),
    )
    .is_err()
    {
        clear_blueprint_child_template(vm_id);
        return Err(StartError::GuestMemoryUnavailable);
    }
    *launch_script_slot.lock() = state.launch_script.clone();
    *slot.lock() = Some(guest_state);
    let _ = exit_reason_mailbox.lock().take();
    *process_slot.lock() = Some(process_context);
    if let Some(log_slot) = BLUEPRINT_CONSOLE_LOG_BUFFERS.get(vm_id as usize) {
        let _ = log_slot.lock().take();
    }
    crate::log_os::blueprint_important_line(format_args!(
        "terminal-lifecycle: vm={} phase=launch-reserved state={} epoch=0 handoff=none intent={} route={} target={}\n",
        vm_id,
        if console_surface.is_terminal() {
            "prelease"
        } else {
            "unsupported"
        },
        if console_surface.is_terminal() {
            "cli->terminal"
        } else {
            "none"
        },
        if console_route.is_net_shell_direct() {
            "net-shell-direct"
        } else {
            "matrix"
        },
        console_target_present as u8
    ));
    Ok(())
}

pub fn take_blueprint_launch(vm_id: u8) -> Option<BlueprintLaunchState> {
    BLUEPRINT_LAUNCH_STATES.get(vm_id as usize)?.lock().take()
}

/// Consume the VMX minishell launch script for one guest filesystem read.
/// The value deliberately is not serialized into a portable VM snapshot.
pub(crate) fn take_blueprint_launch_script(vm_id: u8) -> Option<AllocString> {
    BLUEPRINT_VMX_LAUNCH_SCRIPTS
        .get(vm_id as usize)?
        .lock()
        .take()
}

fn clear_blueprint_launch_script(vm_id: u8) {
    if let Some(slot) = BLUEPRINT_VMX_LAUNCH_SCRIPTS.get(vm_id as usize) {
        let _ = slot.lock().take();
    }
}

fn blueprint_launch_snapshot(vm_id: u8) -> Option<BlueprintLaunchState> {
    BLUEPRINT_LAUNCH_STATES.get(vm_id as usize)?.lock().clone()
}

pub fn blueprint_launch_active(vm_id: u8) -> bool {
    BLUEPRINT_LAUNCH_STATES
        .get(vm_id as usize)
        .map(|slot| slot.lock().is_some())
        .unwrap_or(false)
}

pub(crate) fn blueprint_exposed_cpu_count(vm_id: u8) -> usize {
    let horizon_worker_lane = BLUEPRINT_LAUNCH_STATES
        .get(vm_id as usize)
        .and_then(|slot| {
            let state = slot.lock();
            state
                .as_ref()
                .map(|state| archive_has(state.archive.as_str(), "horizon"))
        })
        .unwrap_or(false);

    if horizon_worker_lane {
        crate::workers::app_visible_parallelism().max(1)
    } else {
        1
    }
}

pub(crate) fn blueprint_process_arg_count(vm_id: u8) -> Option<usize> {
    let context = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize)?.lock();
    Some(context.as_ref()?.args.len())
}

pub(crate) fn blueprint_process_arg(vm_id: u8, index: usize) -> Option<AllocString> {
    let context = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize)?.lock();
    context.as_ref()?.args.get(index).cloned()
}


pub(crate) fn blueprint_console_target(vm_id: u8) -> Option<crate::shell3::MatrixTarget> {
    let context = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize)?.lock();
    context.as_ref()?.console_target.clone()
}

pub(crate) fn blueprint_process_env_var(vm_id: u8, key: &str) -> Option<AllocString> {
    if key.starts_with("TRUEOS_") {
        // Authorization consumes the kernel-owned launch map, never mutable
        // getenv/environ buffers. Hull code cannot walk host map nodes.
        if current_hull_guest_context_vm_id().is_some() {
            return None;
        }
        let context = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize)?.lock();
        return context.as_ref()?.vars.get(key).cloned();
    }
    crate::std_abi_shim::environment_bytes(Some(vm_id), key.as_bytes())
        .and_then(|value| AllocString::from_utf8(value).ok())
}

pub(crate) fn blueprint_process_env_text(vm_id: u8) -> Option<AllocString> {
    let pairs = crate::std_abi_shim::environment_pairs(vm_id)?;
    let mut out = AllocString::new();
    for (key, value) in pairs.iter() {
        let (Ok(key), Ok(value)) = (core::str::from_utf8(key), core::str::from_utf8(value)) else {
            continue;
        };
        let _ = writeln!(out, "{}={}", key, value);
    }
    Some(out)
}

fn blueprint_context_path(vm_id: u8, requested: &str) -> Option<AllocString> {
    let root = {
        let context = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize)?.lock();
        context.as_ref()?.vars.get("TRUEOS_APP_FS_ROOT").cloned()
    };
    // Release the launch-context lock before lazy environment initialization
    // takes its initial snapshot from that same context.
    let home = blueprint_process_env_var(vm_id, "HOME").unwrap_or_else(|| AllocString::from("/"));

    let logical = if requested.trim().is_empty() {
        home.as_str()
    } else {
        requested.trim()
    };
    let rel = crate::r::path::FsPath::parse(logical, true)
        .ok()
        .map(|path| path.to_relative_string())?;
    let Some(root) = root else {
        return Some(rel);
    };
    let root_rel = crate::r::path::FsPath::parse(root.as_str(), true)
        .ok()
        .map(|path| path.to_relative_string())?;
    if rel.is_empty() {
        Some(root_rel)
    } else if rel == root_rel || rel.starts_with(root_rel.as_str()) {
        Some(rel)
    } else {
        Some(alloc::format!("{}/{}", root_rel.trim_end_matches('/'), rel))
    }
}

fn blueprint_tree_children(paths: &[AllocString], parent: &str) -> AllocVec<AllocString> {
    let prefix = if parent.is_empty() {
        AllocString::new()
    } else {
        alloc::format!("{}/", parent.trim_end_matches('/'))
    };
    let mut children = AllocVec::new();
    for path in paths.iter() {
        let rest = if prefix.is_empty() {
            path.as_str()
        } else if let Some(rest) = path.strip_prefix(prefix.as_str()) {
            rest
        } else {
            continue;
        };
        let seg = rest.split('/').next().unwrap_or("");
        if !seg.is_empty() {
            children.push(AllocString::from(seg));
        }
    }
    children.sort();
    children.dedup();
    children
}

fn blueprint_tree_has_descendant(paths: &[AllocString], path: &str) -> bool {
    let prefix = alloc::format!("{}/", path.trim_end_matches('/'));
    paths.iter().any(|p| p.starts_with(prefix.as_str()))
}

pub(crate) fn blueprint_process_file_tree_text(vm_id: u8, requested: &str) -> Option<AllocString> {
    const MAX_DEPTH: usize = 3;
    const MAX_CHILDREN: usize = 24;
    const MAX_LINES: usize = 160;

    let root_path = blueprint_context_path(vm_id, requested)?;
    let roots = crate::r::fs::trueosfs::list_roots();
    if roots.is_empty() {
        return Some(AllocString::from("file: no TRUEOSFS roots mounted\n"));
    }

    for root in roots.iter().copied() {
        if !root.index_ready {
            crate::r::fs::trueosfs::request_warm_index(root.disk_id);
            continue;
        }
        let Some(paths) = crate::r::fs::trueosfs::root_index_paths(root.disk_id, MAX_LINES * 8)
        else {
            continue;
        };
        let in_tree = paths
            .iter()
            .any(|path| path == root_path.as_str() || path.starts_with(root_path.as_str()));
        if !in_tree && !root_path.is_empty() {
            continue;
        }

        let mut out = alloc::format!(
            "file: {}\n",
            if root_path.is_empty() {
                "/"
            } else {
                root_path.as_str()
            }
        );
        let mut stack: AllocVec<(AllocString, usize)> = AllocVec::new();
        stack.push((root_path.clone(), 0));
        let mut lines = 0usize;
        while let Some((parent, depth)) = stack.pop() {
            if depth >= MAX_DEPTH || lines >= MAX_LINES {
                continue;
            }
            let children = blueprint_tree_children(paths.as_slice(), parent.as_str());
            if children.is_empty() && depth == 0 {
                let _ = writeln!(out, "  (empty)");
                lines = lines.saturating_add(1);
                continue;
            }
            for child in children.iter().take(MAX_CHILDREN).rev() {
                let full = if parent.is_empty() {
                    child.clone()
                } else {
                    alloc::format!("{}/{}", parent.trim_end_matches('/'), child)
                };
                let is_dir = blueprint_tree_has_descendant(paths.as_slice(), full.as_str());
                let indent = "  ".repeat(depth + 1);
                let _ = writeln!(out, "{}{}{}", indent, child, if is_dir { "/" } else { "" });
                lines = lines.saturating_add(1);
                if is_dir {
                    stack.push((full, depth + 1));
                }
                if lines >= MAX_LINES {
                    break;
                }
            }
            if children.len() > MAX_CHILDREN && lines < MAX_LINES {
                let indent = "  ".repeat(depth + 1);
                let _ =
                    writeln!(out, "{}... {} more entries", indent, children.len() - MAX_CHILDREN);
                lines = lines.saturating_add(1);
            }
        }
        return Some(out);
    }

    Some(alloc::format!(
        "file: index cold or path not found; warming indexes for {}\n",
        if root_path.is_empty() {
            "/"
        } else {
            root_path.as_str()
        }
    ))
}


pub(crate) fn blueprint_console_write(vm_id: u8, data: &[u8]) -> usize {
    blueprint_console_text_lines(vm_id, None, data);
    let Some(target) = blueprint_console_target(vm_id) else {
        return data.len();
    };
    crate::shell3::tui::write_stdout(&target, vm_id, data)
}

pub(crate) fn blueprint_console_raw_write(vm_id: u8, data: &[u8]) -> usize {
    let Some(target) = blueprint_console_target(vm_id) else {
        return 0;
    };
    crate::shell3::tui::write(&target, vm_id, data).unwrap_or(0)
}

pub(crate) fn blueprint_console_konsole_size(vm_id: u8) -> (u32, u32) {
    blueprint_console_target(vm_id)
        .and_then(|target| crate::shell3::tui::surface(&target))
        .map(|s| (s.cols, s.rows))
        .unwrap_or((0, 0))
}

pub(crate) fn blueprint_console_konsole_begin_frame(
    vm_id: u8,
    cols: usize,
    rows: usize,
    terminal_handoff: bool,
) -> (u32, u32) {
    blueprint_console_konsole_size(vm_id)
}

pub(crate) fn blueprint_console_set_exit_reason(vm_id: u8, reason: &str) -> bool {
    let Some(slot) = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize) else {
        return false;
    };
    let mut guard = slot.lock();
    let Some(context) = guard.as_mut() else {
        return false;
    };
    let clipped = reason.trim();
    if clipped.is_empty() {
        return false;
    }
    let mut stored = AllocString::new();
    for ch in clipped.chars().take(160) {
        stored.push(ch);
    }
    context.exit_reason = Some(stored.clone());
    drop(guard);
    if let Some(mailbox) = BLUEPRINT_EXIT_REASON_MAILBOXES.get(vm_id as usize) {
        *mailbox.lock() = Some(stored.clone());
    }
    hvlogf(format_args!("hv: vm{} lifecycle: blueprint exit reason={}", vm_id, stored));
    crate::log_os::blueprint_important_line(format_args!(
        "terminal-lifecycle: vm={} phase=exit-request reason={}\n",
        vm_id, stored
    ));
    true
}

pub(crate) fn blueprint_console_exit_reason(vm_id: u8) -> Option<AllocString> {
    let active = BLUEPRINT_PROCESS_CONTEXTS
        .get(vm_id as usize)
        .and_then(|slot| slot.lock().as_ref()?.exit_reason.clone());
    active.or_else(|| {
        BLUEPRINT_EXIT_REASON_MAILBOXES
            .get(vm_id as usize)
            .and_then(|slot| slot.lock().clone())
    })
}

pub(crate) fn blueprint_terminal_lease_current(
    vm_id: u8,
    ready_epoch: u64,
) -> Result<u64, BlueprintTerminalLeaseError> {
    let _gate = BLUEPRINT_TERMINAL_TRANSITIONS
        .get(vm_id as usize)
        .ok_or(BlueprintTerminalLeaseError::Unsupported)?
        .lock();
    let slot = BLUEPRINT_PROCESS_CONTEXTS
        .get(vm_id as usize)
        .ok_or(BlueprintTerminalLeaseError::Unsupported)?;
    let (target, epoch) = {
        let guard = slot.lock();
        let c = guard
            .as_ref()
            .ok_or(BlueprintTerminalLeaseError::Detached)?;
        if !c.console_attached {
            return Err(BlueprintTerminalLeaseError::Detached);
        }
        match c.terminal_lease {
            BlueprintTerminalLeaseState::Active { epoch, .. } => {
                if ready_epoch != 0 && ready_epoch != epoch {
                    return Err(BlueprintTerminalLeaseError::Stale);
                }
                return Ok(epoch);
            }
            BlueprintTerminalLeaseState::Reserved if ready_epoch == 0 => (
                c.console_target
                    .clone()
                    .ok_or(BlueprintTerminalLeaseError::Unsupported)?,
                1,
            ),
            _ => return Err(BlueprintTerminalLeaseError::NotActive),
        }
    };
    if crate::shell3::tui::claim(&target, vm_id) != Some(true) {
        return Err(BlueprintTerminalLeaseError::Busy);
    }
    let mut guard = slot.lock();
    let c = guard
        .as_mut()
        .ok_or(BlueprintTerminalLeaseError::Detached)?;
    c.terminal_lease = BlueprintTerminalLeaseState::Active {
        epoch,
        observed: true,
        ready: true,
    };
    crate::log_os::blueprint_important_line(format_args!(
        "terminal-lifecycle: vm={} phase=lease-claim state=active epoch={}\n",
        vm_id, epoch,
    ));
    Ok(epoch)
}

pub(crate) fn blueprint_terminal_surface_snapshot(
    vm_id: u8,
) -> Result<BlueprintTerminalSurfaceSnapshot, BlueprintTerminalLeaseError> {
    match blueprint_terminal_lease_current(vm_id, 0) {
        Ok(_) => {}
        Err(BlueprintTerminalLeaseError::NotActive) => {
            // Crossterm polls this surface from its event loop. Treat that
            // guest poll as acknowledgement of an explicit Shell3 `tui`
            // request, using the same ticket/epoch validation as typed apps.
            // A merely parked app must never reclaim the shell's input.
            let ticket = {
                let slot = BLUEPRINT_PROCESS_CONTEXTS
                    .get(vm_id as usize)
                    .ok_or(BlueprintTerminalLeaseError::Unsupported)?;
                let guard = slot.lock();
                let c = guard
                    .as_ref()
                    .ok_or(BlueprintTerminalLeaseError::Detached)?;
                if !c.console_attached {
                    return Err(BlueprintTerminalLeaseError::Detached);
                }
                match c.terminal_lease {
                    BlueprintTerminalLeaseState::ReentryRequested { ticket, .. } => ticket,
                    _ => return Err(BlueprintTerminalLeaseError::NotActive),
                }
            };
            match blueprint_terminal_lease_poll_reentry(vm_id, ticket)? {
                BlueprintTerminalReentryPoll::Ready(_) => {}
                BlueprintTerminalReentryPoll::Pending => {
                    return Err(BlueprintTerminalLeaseError::Busy);
                }
            }
        }
        Err(error) => return Err(error),
    }
    let target = blueprint_console_target(vm_id).ok_or(BlueprintTerminalLeaseError::Detached)?;
    crate::shell3::tui::surface(&target).ok_or(BlueprintTerminalLeaseError::Unsupported)
}

pub(crate) fn blueprint_terminal_lease_release(
    vm_id: u8,
    expected_epoch: u64,
) -> Result<u64, BlueprintTerminalLeaseError> {
    let _gate = BLUEPRINT_TERMINAL_TRANSITIONS
        .get(vm_id as usize)
        .ok_or(BlueprintTerminalLeaseError::Unsupported)?
        .lock();
    let slot = BLUEPRINT_PROCESS_CONTEXTS
        .get(vm_id as usize)
        .ok_or(BlueprintTerminalLeaseError::Unsupported)?;
    let (target, epoch) = {
        let guard = slot.lock();
        let c = guard
            .as_ref()
            .ok_or(BlueprintTerminalLeaseError::Detached)?;
        let BlueprintTerminalLeaseState::Active { epoch, .. } = c.terminal_lease else {
            return Err(BlueprintTerminalLeaseError::NotActive);
        };
        if expected_epoch != 0 && expected_epoch != epoch {
            return Err(BlueprintTerminalLeaseError::Stale);
        }
        (
            c.console_target
                .clone()
                .ok_or(BlueprintTerminalLeaseError::Unsupported)?,
            epoch,
        )
    };
    if crate::shell3::tui::release(&target, vm_id) != Some(true) {
        return Err(BlueprintTerminalLeaseError::Busy);
    }
    if let Some(c) = slot.lock().as_mut() {
        c.terminal_lease = BlueprintTerminalLeaseState::Parked { ticket: epoch };
    }
    Ok(epoch)
}

pub(crate) fn blueprint_console_return_to_cli(vm_id: u8) -> bool {
    match blueprint_terminal_lease_release(vm_id, 0) {
        Ok(_) => true,
        Err(BlueprintTerminalLeaseError::NotActive) => BLUEPRINT_PROCESS_CONTEXTS
            .get(vm_id as usize)
            .and_then(|slot| {
                let _transition = BLUEPRINT_TERMINAL_TRANSITIONS.get(vm_id as usize)?.lock();
                let mut guard = slot.lock();
                let context = guard.as_mut()?;
                match context.terminal_lease {
                    BlueprintTerminalLeaseState::ReentryRequested { epoch, .. } => {
                        // Navigation cancels an unacknowledged `tui`; a late
                        // guest poll must not reacquire the shell's input.
                        context.terminal_lease =
                            BlueprintTerminalLeaseState::Parked { ticket: epoch };
                        Some(true)
                    }
                    BlueprintTerminalLeaseState::Reserved
                    | BlueprintTerminalLeaseState::Parked { .. } => Some(true),
                    _ => Some(false),
                }
            })
            .unwrap_or(false),
        Err(_) => false,
    }
}

fn blueprint_console_request_tui(vm_id: u8) -> BlueprintTerminalReentryRequest {
    let Some(slot) = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize) else {
        return BlueprintTerminalReentryRequest::Unsupported;
    };
    let Some(transition_slot) = BLUEPRINT_TERMINAL_TRANSITIONS.get(vm_id as usize) else {
        return BlueprintTerminalReentryRequest::Unsupported;
    };
    let _transition = transition_slot.lock();
    let request = {
        let mut guard = slot.lock();
        let Some(context) = guard.as_mut() else {
            return BlueprintTerminalReentryRequest::Unsupported;
        };
        if !context.console_attached {
            return BlueprintTerminalReentryRequest::Detached;
        }
        match context.terminal_lease {
            BlueprintTerminalLeaseState::Parked { ticket } => {
                let epoch = ticket.wrapping_add(1).max(1);
                context.terminal_lease =
                    BlueprintTerminalLeaseState::ReentryRequested { ticket, epoch };
                BlueprintTerminalReentryRequest::Requested { ticket, epoch }
            }
            BlueprintTerminalLeaseState::ReentryRequested { .. } => {
                BlueprintTerminalReentryRequest::AlreadyRequested
            }
            BlueprintTerminalLeaseState::Reserved => BlueprintTerminalReentryRequest::NotParked,
            BlueprintTerminalLeaseState::Active { .. } => {
                BlueprintTerminalReentryRequest::NotParked
            }
            BlueprintTerminalLeaseState::Unsupported => {
                BlueprintTerminalReentryRequest::Unsupported
            }
        }
    };
    if let BlueprintTerminalReentryRequest::Requested { ticket, epoch } = request {
        crate::log_os::blueprint_important_line(format_args!(
            "terminal-lifecycle: vm={} phase=reentry-request state=pending ticket={} epoch={} owner=shell2\n",
            vm_id, ticket, epoch
        ));
        notify_blueprint_console_input(vm_id);
    }
    request
}

pub(crate) fn blueprint_terminal_reentry_requested(vm_id: u8) -> bool {
    BLUEPRINT_PROCESS_CONTEXTS
        .get(vm_id as usize)
        .is_some_and(|slot| {
            slot.lock().as_ref().is_some_and(|context| {
                context.console_attached
                    && matches!(
                        context.terminal_lease,
                        BlueprintTerminalLeaseState::ReentryRequested { .. }
                    )
            })
        })
}

/// Request the same epoch/ticket handshake used by vmx_tui, without CLI routing.
pub(crate) fn blueprint_terminal_request_reentry(vm_id: u8) -> Result<(), &'static str> {
    match blueprint_console_request_tui(vm_id) {
        BlueprintTerminalReentryRequest::Requested { .. }
        | BlueprintTerminalReentryRequest::AlreadyRequested
        | BlueprintTerminalReentryRequest::NotParked => Ok(()),
        BlueprintTerminalReentryRequest::Detached => Err("tui: Blueprint console detached"),
        BlueprintTerminalReentryRequest::Unsupported => Err("tui: Blueprint has no terminal UI"),
    }
}


pub(crate) fn blueprint_terminal_lease_poll_reentry(
    vm_id: u8,
    ticket: u64,
) -> Result<BlueprintTerminalReentryPoll, BlueprintTerminalLeaseError> {
    let _gate = BLUEPRINT_TERMINAL_TRANSITIONS
        .get(vm_id as usize)
        .ok_or(BlueprintTerminalLeaseError::Unsupported)?
        .lock();
    let slot = BLUEPRINT_PROCESS_CONTEXTS
        .get(vm_id as usize)
        .ok_or(BlueprintTerminalLeaseError::Unsupported)?;
    let (target, epoch) = {
        let guard = slot.lock();
        let c = guard
            .as_ref()
            .ok_or(BlueprintTerminalLeaseError::Detached)?;
        match c.terminal_lease {
            BlueprintTerminalLeaseState::Parked { ticket: current } if current == ticket => {
                return Ok(BlueprintTerminalReentryPoll::Pending);
            }
            BlueprintTerminalLeaseState::ReentryRequested {
                ticket: current,
                epoch,
            } if current == ticket => (
                c.console_target
                    .clone()
                    .ok_or(BlueprintTerminalLeaseError::Unsupported)?,
                epoch,
            ),
            BlueprintTerminalLeaseState::Active { epoch, .. }
                if epoch == ticket.wrapping_add(1).max(1) =>
            {
                return Ok(BlueprintTerminalReentryPoll::Ready(epoch));
            }
            _ => return Err(BlueprintTerminalLeaseError::Stale),
        }
    };
    if crate::shell3::tui::claim(&target, vm_id) != Some(true) {
        return Ok(BlueprintTerminalReentryPoll::Pending);
    }
    if let Some(c) = slot.lock().as_mut() {
        c.terminal_lease = BlueprintTerminalLeaseState::Active {
            epoch,
            observed: true,
            ready: true,
        };
    }
    Ok(BlueprintTerminalReentryPoll::Ready(epoch))
}


fn blueprint_console_text_lines(vm_id: u8, target: Option<&MatrixTarget>, data: &[u8]) {
    crate::log!("blueprint vm{}: {}\n", vm_id, AllocString::from_utf8_lossy(data));
}








pub(crate) fn notify_blueprint_console_input(vm_id: u8) {
    if let Some(signal) = BLUEPRINT_CONSOLE_INPUT_READY.get(vm_id as usize) {
        signal.signal(());
    }
    let _ = crate::wait::platform_wake_blueprint_io_for_vm(vm_id);
}

pub(crate) async fn wait_blueprint_console_input(vm_id: u8, timeout_ms: u64) -> bool {
    let Some(signal) = BLUEPRINT_CONSOLE_INPUT_READY.get(vm_id as usize) else {
        return false;
    };
    if blueprint_console_readable_len(vm_id) != 0 {
        signal.reset();
        return true;
    }
    with_timeout(EmbassyDuration::from_millis(timeout_ms.max(1)), signal.wait())
        .await
        .is_ok()
}


pub(crate) fn blueprint_console_submit_stdin_for_target(
    vm_id: u8,
    target: &MatrixTarget,
    run: u64,
    data: &[u8],
) -> usize {
    blueprint_console_submit_stdin_for_lease(
        vm_id,
        &crate::shell3::matrix_target_slot_lease(target),
        run,
        data,
    )
}

pub(crate) fn blueprint_console_submit_stdin_for_lease(
    vm_id: u8,
    lease: &crate::shell3::MatrixSlotLease,
    run: u64,
    data: &[u8],
) -> usize {
    blueprint_console_submit_stdin_inner(vm_id, data, Some((lease, run)))
}

fn blueprint_console_submit_stdin_inner(
    vm_id: u8,
    data: &[u8],
    expected: Option<(&crate::shell3::MatrixSlotLease, u64)>,
) -> usize {
    const MAX_CONSOLE_INPUT: usize = 64 * 1024;
    if data.is_empty()
        || expected
            .is_some_and(|(lease, _)| !crate::shell3::matrix_target::matrix_slot_is_live(lease))
    {
        return 0;
    }
    let Some(slot) = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize) else {
        return 0;
    };
    let mut guard = slot.lock();
    let Some(context) = guard.as_mut() else {
        return 0;
    };
    if !context.console_attached
        || !context
            .terminal_lease
            .accepts_console_input(context.app_command_passthrough)
    {
        return 0;
    }
    if let Some((lease, run)) = expected {
        if vm_run_generation(vm_id) != Some(run)
            || context
                .console_target
                .as_ref()
                .map(crate::shell3::matrix_target_slot_lease)
                .as_ref()
                != Some(lease)
        {
            return 0;
        }
    }
    if context.console_input.is_empty() {
        context.console_input_since_ms = Some(crate::r::input_timing::now_ms());
    }
    for &byte in data {
        if context.console_input.len() >= MAX_CONSOLE_INPUT {
            let _ = context.console_input.pop_front();
        }
        context.console_input.push_back(byte);
    }
    drop(guard);
    notify_blueprint_console_input(vm_id);
    data.len()
}


pub(crate) fn blueprint_console_read_byte(vm_id: u8) -> Option<u8> {
    let mut byte = [0u8; 1];
    (blueprint_console_read(vm_id, &mut byte) == 1).then_some(byte[0])
}

pub(crate) fn blueprint_console_read(vm_id: u8, out: &mut [u8]) -> usize {
    static AGE: crate::r::input_timing::Site =
        crate::r::input_timing::Site::new("app-stdin-queue-age");
    let Some(slot) = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize) else {
        return 0;
    };
    let mut guard = slot.lock();
    let Some(context) = guard.as_mut() else {
        return 0;
    };
    if !context.console_attached
        || !context
            .terminal_lease
            .accepts_console_input(context.app_command_passthrough)
    {
        return 0;
    }
    let count = out.len().min(context.console_input.len());
    let since = context.console_input_since_ms;
    for byte in &mut out[..count] {
        *byte = context.console_input.pop_front().unwrap();
    }
    if context.console_input.is_empty() {
        context.console_input_since_ms = None;
    }
    drop(guard);
    if count != 0 {
        if let Some(since) = since {
            AGE.record(crate::r::input_timing::now_ms().saturating_sub(since));
        }
    }
    count
}

pub(crate) fn blueprint_console_readable_len(vm_id: u8) -> usize {
    BLUEPRINT_PROCESS_CONTEXTS
        .get(vm_id as usize)
        .and_then(|slot| {
            slot.lock().as_ref().map(|c| {
                if c.console_attached
                    && c.terminal_lease
                        .accepts_console_input(c.app_command_passthrough)
                {
                    c.console_input.len()
                } else {
                    0
                }
            })
        })
        .unwrap_or(0)
}

pub(crate) fn blueprint_console_print_line(vm_id: u8, line: &str) {
    let output = alloc::format!("{line}\n");
    blueprint_console_write(vm_id, output.as_bytes());
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn blueprint_process_context(vm_id: u8) -> Option<BlueprintProcessContext> {
    BLUEPRINT_PROCESS_CONTEXTS
        .get(vm_id as usize)?
        .lock()
        .as_ref()
        .cloned()
}

fn clear_blueprint_process_context(vm_id: u8) -> BlueprintTerminalCleanup {
    if let Some(target) = blueprint_console_target(vm_id) {
        let _ = crate::shell3::tui::release(&target, vm_id);
    }
    let mut cleanup = BlueprintTerminalCleanup::empty();
    if let Some(slot) = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize) {
        cleanup.context_present = slot.lock().take().is_some();
    }

    crate::std_abi_shim::reset_blueprint_process_state(vm_id);
    if let Some(slot) = BLUEPRINT_CONSOLE_LOG_BUFFERS.get(vm_id as usize) {
        let _ = slot.lock().take();
    }
    cleanup
}

fn suspend_blueprint_process_context(vm_id: u8) {
    if let Some(slot) = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize) {
        if let Some(context) = slot.lock().as_mut() {
            context.console_attached = false;
        }
    }
}


#[cfg(feature = "legacy_vm_checkpoint")]
fn resume_blueprint_process_context(vm_id: u8) {
    if let Some(slot) = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize) {
        if let Some(context) = slot.lock().as_mut() {
            context.console_attached = false;
        }
    }
}

pub(crate) fn blueprint_launch_states_span() -> (u64, usize) {
    (
        (&BLUEPRINT_LAUNCH_STATES as *const _) as u64,
        core::mem::size_of_val(&BLUEPRINT_LAUNCH_STATES),
    )
}

pub(crate) fn blueprint_process_contexts_span() -> (u64, usize) {
    (
        (&BLUEPRINT_PROCESS_CONTEXTS as *const _) as u64,
        core::mem::size_of_val(&BLUEPRINT_PROCESS_CONTEXTS),
    )
}

pub fn log_active_blueprint_console_line(args: core::fmt::Arguments<'_>) {
    let mut line: String<HV_LOG_LINE> = String::new();
    let _ = line.write_fmt(args);
    if line.is_empty() {
        return;
    }
    if let Some(vm_id) = current_hull_guest_context_vm_id().or_else(current_vm_id) {
        blueprint_console_print_line(vm_id, line.as_str());
    }
    hvlogf(format_args!("{}", line.as_str()));
}

#[derive(Copy, Clone)]
struct LineageRecord {
    level: u8,
}

impl LineageRecord {
    const fn new() -> Self {
        Self { level: 1 }
    }
}

#[task(pool_size = 64)]
async fn vm_task(vm_id: u8, mut lane_lease: crate::hv::lane::LaneLease) {
    let Some(vm) = vm_slot(vm_id) else {
        return;
    };
    let lineage_record = LineageRecord::new();
    vm.running.store(true, Ordering::Release);
    vm.preserve_req.store(false, Ordering::Release);
    vm.preserve_exit.store(false, Ordering::Release);
    vm.clean_exit.store(false, Ordering::Release);
    set_current_vm_id(vm_id);
    if let Some(slot) = BLUEPRINT_CHILD_LINKS.get(vm_id as usize)
        && let Some(link) = slot.lock().as_mut()
        && link.child_generation == vm.run_generation.load(Ordering::Acquire)
        && link.state == BLUEPRINT_CHILD_STATE_STARTING
    {
        link.state = BLUEPRINT_CHILD_STATE_RUNNING;
    }
    let cpu = crate::cpu::CpuProfile::current();
    if let Some(cpu) = cpu {
        hvlogf(format_args!(
            "hv: vm{}-{} lifecycle: starting slot={} lapic={} kind={}",
            vm_id,
            lineage_record.level,
            cpu.slot(),
            cpu.lapic_id(),
            cpu.core_kind_name()
        ));
    } else {
        hvlogf(format_args!(
            "hv: vm{}-{} lifecycle: starting slot=unknown",
            vm_id, lineage_record.level
        ));
    }
    crate::log!(
        "app-vm-run-queue: vm task running vm={} lineage={}\n",
        vm_id,
        lineage_record.level
    );

    let boot_mode = boot_mode_for_vm(vm_id);
    let pending_blueprint = take_blueprint_pending_launch(vm_id);
    let guest = crate::limine::guest_kernel_bytes();
    match boot_mode {
        VmBootMode::Full => {
            let guest_len = guest.map(|b| b.len()).unwrap_or(0);
            hvlogf(format_args!("hv: vm{} lifecycle: full guest bytes={}", vm_id, guest_len));
            if let Some(bytes) = guest {
                if let Some(entry) = guest_kernel_elf_entry(bytes) {
                    hvlogf(format_args!(
                        "hv: vm{} reporting: full guest elf entry=0x{:016X} vmx_guest_entry=0x{:016X}",
                        vm_id,
                        entry,
                        guest_launch_rip()
                    ));
                } else {
                    hvwarnf(format_args!(
                        "hv: vm{} reporting: full guest bytes present but ELF entry parse failed; vmx_guest_entry=0x{:016X}",
                        vm_id,
                        guest_launch_rip()
                    ));
                }
            }
        }
        VmBootMode::Hull => {
            hvlogf(format_args!(
                "hv: vm{} lifecycle: hull guest entry=0x{:016X} stack_mib={}",
                vm_id,
                guest_launch_rip(),
                memory::active_guest_stack_mb_for_vm(vm_id)
            ));
        }
    }
    if let Some(pending) = pending_blueprint
        && let Err(err) = prepare_blueprint_launch_on_lane(vm_id, pending)
    {
        hvwarnf(format_args!("hv: vm{} lifecycle: blueprint prep failed ({})", vm_id, err));
        clear_current_vm_id();
        vm.starting.store(false, Ordering::Release);
        vm.stop_req.store(false, Ordering::Release);
        vm.preserve_req.store(false, Ordering::Release);
        vm.preserve_exit.store(false, Ordering::Release);
        vm.clean_exit.store(false, Ordering::Release);
        clear_blueprint_process_context(vm_id);
        blueprint_child_lifecycle_cleanup(vm_id, vm.run_generation.load(Ordering::Acquire), false);
        lane_lease.release_now();
        publish_vm_offline(vm_id, vm);
        return;
    }
    hvlogf(format_args!("hv: vm{} reporting: vmx preflight ok, stage=m1", vm_id));
    hvlogf(format_args!("hv: vm{} reporting: vlayer policy=integrity-first", vm_id));
    if boot_mode == VmBootMode::Hull {
        if let Err(err) = memory::ensure_guest_hull_rw_template_ready() {
            hvwarnf(format_args!(
                "hv: vm{} reporting: hull rw template prepare failed ({})",
                vm_id, err
            ));
        }
    }
    let guest_heap_ready = crate::allocators::ensure_hv_guest_heap_ready(vm_id);
    if guest_heap_ready {
        let stats = crate::allocators::hv_guest_heap_stats(vm_id);
        hvlogf(format_args!(
            "hv: vm{} reporting: hv-guest-heap virt=0x{:016X}..0x{:016X} src={:?} free_bytes={} blocks={}",
            vm_id,
            stats.heap_start,
            stats.heap_end,
            stats.source,
            stats.free_bytes,
            stats.free_blocks
        ));
    }
    hvlogf(format_args!(
        "hv: vm{} reporting: initial guest fs_base=0x{:016X}",
        vm_id,
        guest_fs_base_for_boot_mode(boot_mode)
    ));
    crate::log!(
        "app-vm-run-queue: vm launch enter vm={} mode={:?} stack_mib={}\n",
        vm_id,
        boot_mode,
        memory::active_guest_stack_mb_for_vm(vm_id)
    );
    // No guest code can execute before admission is tied to this run. An
    // outstanding reservation here is an invariant failure, never permission
    // to replace live executable/heap state with a new generation.
    assert!(crate::r::blocking::open_guest_jobs(vm_id, vm.run_generation.load(Ordering::Acquire)));
    assert!(crate::r::fs::request_broker::open_vm(vm_id));
    // Publish pause eligibility only after native admission belongs to this
    // incarnation. Earlier requests must see a starting VM, not a live Hull.
    {
        let _control = vm.lifecycle_control.lock();
        vm.starting.store(false, Ordering::Release);
    }
    if immediate_stop_requested(vm) || vm.preserve_req.load(Ordering::Acquire) {
        crate::r::blocking::close_guest_jobs(vm_id);
    }
    teardown_diagnostics::set(vm_id, TeardownStage::Hull);
    let launch_result = vmx_launch_once_with_ept(
        lineage_record,
        lane_lease.slot() as usize,
        vm.run_generation.load(Ordering::Acquire),
    )
    .await;
    #[cfg(feature = "legacy_vm_checkpoint")]
    if let Ok(lr) = launch_result {
        snapshot::capture_snapshot_meta(vm_id, lr);
    }
    crate::log!("app-vm-run-queue: vm launch returned vm={} mode={:?}\n", vm_id, boot_mode);
    clear_current_vm_id();
    teardown_diagnostics::set(vm_id, TeardownStage::NativeDrain);
    if !vm.pause_latched.load(Ordering::Acquire) {
        crate::r::fs::request_broker::request_release_vm(vm_id);
    }
    crate::r::blocking::drain_guest_jobs(vm_id).await;
    // A killed continuation may have abandoned a streamed write. The BSP
    // rolls it back before this incarnation's ownership can be forgotten.
    let fs_pending = crate::r::fs::request_broker::pending_for_vm(vm_id);
    let fs_writes = crate::r::fs::request_broker::open_writes_for_vm(vm_id);
    let async_pending = crate::r::io::async_fs_cabi::pending_for_owner(
        crate::r::io::async_fs_cabi::owner_for_vm(vm_id),
    );
    if fs_pending != 0 || fs_writes != 0 || async_pending != 0 {
        hvimportantf(format_args!(
            "hv: vm{} lifecycle: stop filesystem checkpoint waiting fs_pending={} fs_streams={} async_fs_pending={} owner=bsp slot=retained-until-complete",
            vm_id, fs_pending, fs_writes, async_pending
        ));
    }
    while crate::r::fs::request_broker::pending_for_vm(vm_id) != 0
        || crate::r::fs::request_broker::open_writes_for_vm(vm_id) != 0
        || crate::r::io::async_fs_cabi::pending_for_owner(
            crate::r::io::async_fs_cabi::owner_for_vm(vm_id),
        ) != 0
    {
        Timer::after(EmbassyDuration::from_millis(1)).await;
    }
    if !vm.pause_latched.load(Ordering::Acquire) {
        assert!(
            crate::r::io::async_fs_cabi::release_owner(crate::r::io::async_fs_cabi::owner_for_vm(
                vm_id
            ),)
            .is_ok()
        );
    }
    if vm.cooperative_stop.forced() {
        vm.pause_latched.store(false, Ordering::Release);
        vm.preserve_exit.store(false, Ordering::Release);
        vm.clean_exit.store(true, Ordering::Release);
    }
    if vm.pause_latched.load(Ordering::Acquire) {
        suspend_blueprint_process_context(vm_id);
        #[cfg(feature = "gridpaper")]
        crate::r::services::gridpaper_service::pause_owner_lifecycle(vm_id);
    }
    let mut pending_crash = None;
    let clean_exit = vm.clean_exit.swap(false, Ordering::AcqRel);
    crate::allocators::with_host_alloc_domain_strong(|| match launch_result {
        Ok(lr) => {
            let preserve_exit = vmexit_is_preserve(vm_id, lr);
            if preserve_exit && !vm.cooperative_stop.forced() {
                snapshot_on_preserve_exit(vm_id);
            } else if vm.pause_latched.load(Ordering::Acquire) {
                hvlogf(format_args!(
                    "hv: vm{} lifecycle: retained in-memory pause reached at rip=0x{:016X}",
                    vm_id, lr.guest_rip
                ));
            } else if !clean_exit && let Some(state) = blueprint_launch_snapshot(vm_id).as_ref() {
                pending_crash = Some(crate::hv::app_crash::prepare(
                    vm_id,
                    state,
                    crate::hv::app_crash::CrashOutcome::Vmexit(lr),
                ));
            }
            hvlogf(format_args!(
                "hv: vm{}-{} reporting: vmlaunch entered={} launch_failed={} exit_reason=0x{:X} exit_qual=0x{:X} guest_rip=0x{:016X}",
                vm_id,
                lineage_record.level,
                lr.entered,
                lr.launch_failed,
                lr.exit_reason,
                lr.exit_qualification,
                lr.guest_rip
            ));
            hvlogf(format_args!(
                "hv: vm{}-{} reporting: symbolize_hint=addr2line -e TRUEOS.full.elf 0x{:016X}",
                vm_id, lineage_record.level, lr.guest_rip
            ));
        }
        Err(e) => {
            if let Some(state) = blueprint_launch_snapshot(vm_id).as_ref() {
                pending_crash = Some(crate::hv::app_crash::prepare(
                    vm_id,
                    state,
                    crate::hv::app_crash::CrashOutcome::LaunchError(e),
                ));
            }
            hverrorf(format_args!(
                "hv: vm{}-{} reporting: vmlaunch/ept failed ({})",
                vm_id, lineage_record.level, e
            ));
        }
    });
    hvlogf(format_args!(
        "hv: vm{} lifecycle: teardown begin clean_exit={} preserve_exit={} pause_latched={}",
        vm_id,
        clean_exit as u8,
        vm.preserve_exit.load(Ordering::Acquire) as u8,
        vm.pause_latched.load(Ordering::Acquire) as u8
    ));

    if boot_mode == VmBootMode::Full {
        if let Some(bytes) = guest {
            if contains_bytes(bytes, MAIN_LOOP_MARKER) {
                vm.marker_seen.store(true, Ordering::Release);
                hvlogf(format_args!("hv: vm{} reporting: main: entering executor loop", vm_id));
            }
        }
    }

    if !vm.pause_latched.load(Ordering::Acquire) {
        teardown_diagnostics::set(vm_id, TeardownStage::Archive);
        let archive_released =
            crate::r::codec::release_owner(crate::r::io::async_fs_cabi::owner_for_vm(vm_id));
        if archive_released != 0 {
            hvlogf(format_args!(
                "hv: vm{} lifecycle: archive cleanup released_operations={}",
                vm_id, archive_released
            ));
        }
        #[cfg(feature = "gridpaper")]
        {
            teardown_diagnostics::set(vm_id, TeardownStage::Gridpaper);
            let gridpaper_released =
                crate::r::services::gridpaper_service::release_owner_lifecycle(vm_id);
            if gridpaper_released != 0 {
                hvlogf(format_args!(
                    "hv: vm{} lifecycle: gridpaper cleanup released={}",
                    vm_id, gridpaper_released
                ));
            }
        }
        teardown_diagnostics::set(vm_id, TeardownStage::Media);
        let media_released = crate::r::services::media_service::release_vm(vm_id);
        if media_released != 0 {
            hvlogf(format_args!(
                "hv: vm{} lifecycle: vmedia cleanup released_operations={}",
                vm_id, media_released
            ));
        }
        teardown_diagnostics::set(vm_id, TeardownStage::Ui4);
        let released = crate::ui4::release_owner_resources(crate::ui4::WindowOwner::Vm(vm_id));
        if released != crate::ui4::OwnerReleaseSummary::default() {
            hvlogf(format_args!(
                "hv: vm{} lifecycle: ui4 owner release surfaces={} input_routes={} input_events={} context_menus={}",
                vm_id,
                released.surfaces,
                released.input_routes,
                released.input_events,
                released.context_menus,
            ));
        }
        teardown_diagnostics::set(vm_id, TeardownStage::Input);
        let cursors = crate::r::services::mouse_motion_service::release_principal(
            crate::r::services::mouse_motion_service::MouseControlPrincipal::Vm(vm_id),
        );
        let keyboards = crate::r::services::keyboard_control_service::release_principal(
            crate::r::services::keyboard_control_service::KeyboardControlPrincipal::Vm(vm_id),
        );
        let gamepads = crate::r::services::gamepad_control_service::release_principal(
            crate::r::services::gamepad_control_service::GamepadControlPrincipal::Vm(vm_id),
        );
        if cursors != 0 || keyboards != 0 || gamepads != 0 {
            hvlogf(format_args!(
                "hv: vm{} lifecycle: virtual-input cleanup cursors={} keyboards={} gamepads={}",
                vm_id, cursors, keyboards, gamepads,
            ));
        }
    }

    teardown_diagnostics::set(vm_id, TeardownStage::Vgpu);
    let (vgpu_released, vgpu_quarantined, vgpu_epoch) = crate::gpu::vgpu::release_hull_guest(vm_id);
    if vgpu_released != 0 || vgpu_quarantined != 0 {
        hvlogf(format_args!(
            "hv: vm{} lifecycle: vgpu cleanup released={} quarantined={} epoch={}",
            vm_id, vgpu_released, vgpu_quarantined, vgpu_epoch
        ));
    }

    teardown_diagnostics::set(vm_id, TeardownStage::Net);
    let blueprint_net_closed = crate::hv::blueprint_net::release_vm(vm_id);
    let mio_closed = crate::mio_compat::close_sockets_for_vm(vm_id);
    crate::shell3::service::clear_window_spawn(vm_id);
    let cabi_closed = crate::r::net::socket_cabi::close_sockets_for_vm(vm_id);
    hvlogf(format_args!(
        "hv: vm{} lifecycle: net cleanup complete blueprint_net={} mio={} socket_cabi={}",
        vm_id, blueprint_net_closed, mio_closed, cabi_closed
    ));
    if let Some(reason) = BLUEPRINT_PROCESS_CONTEXTS
        .get(vm_id as usize)
        .and_then(|slot| slot.lock().as_ref()?.exit_reason.clone())
    {
        hvlogf(format_args!("hv: vm{} lifecycle: exit reason={}", vm_id, reason));
    }
    hvlogf(format_args!(
        "hv: vm{} lifecycle: state cleanup begin retain_for_resume={}",
        vm_id,
        vm.pause_latched.load(Ordering::Acquire) as u8
    ));
    teardown_diagnostics::set(vm_id, TeardownStage::State);
    let retained_for_resume = vm.pause_latched.load(Ordering::Acquire);
    blueprint_child_lifecycle_cleanup(
        vm_id,
        vm.run_generation.load(Ordering::Acquire),
        retained_for_resume,
    );
    let preserve_exit = vm.preserve_exit.load(Ordering::Acquire);
    clear_blueprint_pending_launch(vm_id);
    let terminal_cleanup = if retained_for_resume {
        hvlogf(format_args!(
            "hv: vm{} lifecycle: retained blueprint launch/process context for resume",
            vm_id
        ));
        None
    } else {
        clear_active_eptp_for_vm(vm_id);
        memory::release_guest_rel_exec_for_vm(vm_id);
        let _ = take_blueprint_launch(vm_id);
        clear_blueprint_launch_script(vm_id);
        let cleanup = clear_blueprint_process_context(vm_id);
        if let Some(identity) = BLUEPRINT_INSTANCE_IDENTITIES.get(vm_id as usize) {
            let _ = identity.lock().take();
        }
        Some(cleanup)
    };
    hvlogf(format_args!("hv: vm{} lifecycle: state cleanup complete", vm_id));
    crate::log_os::blueprint_important_line(format_args!(
        "terminal-lifecycle: vm={} phase=process-stop state={} clean_exit={} preserve_exit={}\n",
        vm_id,
        if retained_for_resume {
            "retained"
        } else {
            "stopped"
        },
        clean_exit as u8,
        preserve_exit as u8,
    ));
    if let Some(cleanup) = terminal_cleanup {
        crate::log_os::blueprint_important_line(format_args!(
            "terminal-lifecycle: vm={} phase=terminal-cleanup state=stopped owner_returned={} context={} backend_expected={} backend_released={} matrix_expected={} matrix_result={}\n",
            vm_id,
            cleanup.complete() as u8,
            cleanup.context_present as u8,
            cleanup.backend_release_expected as u8,
            cleanup.backend_released as u8,
            cleanup.matrix_unbind_expected as u8,
            cleanup.matrix_unbind_marker(),
        ));
    }
    teardown_diagnostics::set(vm_id, TeardownStage::WaitQueues);
    let wait_queues_retired =
        if !vm.pause_latched.load(Ordering::Acquire) && !vm.preserve_exit.load(Ordering::Acquire) {
            crate::wait::retire_platform_vm_waits(vm_id)
        } else {
            0
        };
    vm.starting.store(false, Ordering::Release);
    vm.stop_req.store(false, Ordering::Release);
    vm.preserve_req.store(false, Ordering::Release);
    vm.preserve_exit.store(false, Ordering::Release);
    vm.clean_exit.store(false, Ordering::Release);
    if let Some(pending) = pending_crash {
        teardown_diagnostics::set(vm_id, TeardownStage::CrashReport);
        crate::hv::app_crash::write(vm_id, pending).await;
    }
    hvlogf(format_args!("hv: vm{} lifecycle: stopped", vm_id));
    // Publish the VM slot as reusable only after its carrier lease is free.
    // This prevents F2/start from queueing a second Hull behind teardown on
    // the same AP while reporting the first VM as already offline.
    let native_jobs = crate::r::blocking::guest_jobs_in_flight(vm_id);
    teardown_diagnostics::set(vm_id, TeardownStage::CarrierRelease);
    lane_lease.release_now();
    publish_vm_offline(vm_id, vm);
    crate::log_os::blueprint_important_line(format_args!(
        "hv: vm{} lifecycle: offline native_jobs={} carrier=released wait_queues_retired={}\n",
        vm_id, native_jobs, wait_queues_retired
    ));
}

fn publish_vm_offline(vm_id: u8, vm: &TrueosVmId) {
    // Called only after native jobs drained and the Hull carrier was released.
    // Serialize destruction with a new claim, including failed preparation.
    let _control = vm.lifecycle_control.lock();
    vm.running.store(false, Ordering::Release);
    vm.execution_pause.resume();
    teardown_diagnostics::set(vm_id, TeardownStage::Offline);
    if vm.cooperative_stop.forced() || !vm.pause_latched.load(Ordering::Acquire) {
        if let Err(error) = eject_offline_vm(vm_id, false) {
            hvwarnf(format_args!(
                "hv: vm{} lifecycle: resource release retained reason={:?}",
                vm_id, error
            ));
        }
    }
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    if haystack.len() < needle.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn vmx_caps() -> (bool, bool, bool, bool, bool) {
    let r0 = __cpuid(0);
    let vendor_intel = r0.ebx == 0x756e6547 && r0.edx == 0x49656e69 && r0.ecx == 0x6c65746e;

    let known_compatible = vendor_intel
        || (r0.ebx == 0x54474354 && r0.edx == 0x47435447 && r0.ecx == 0x43544743)
        || (r0.ebx == 0x68747541 && r0.edx == 0x69746e65 && r0.ecx == 0x444d4163);

    let r1 = __cpuid(1);
    let has_msr = (r1.edx & (1 << 5)) != 0;
    let has_vmx = (r1.ecx & (1 << 5)) != 0;

    let (mut feature_control_locked, mut feature_control_vmx_outside_smx) = (false, false);
    if (known_compatible || has_vmx) && has_msr {
        let val = unsafe { Msr::new(vmx::IA32_FEATURE_CONTROL).read() };
        feature_control_locked = (val & vmx::IA32_FEATURE_CONTROL_LOCK) != 0;
        feature_control_vmx_outside_smx = (val & vmx::IA32_FEATURE_CONTROL_VMX_OUTSIDE_SMX) != 0;
    }

    (known_compatible, has_msr, has_vmx, feature_control_locked, feature_control_vmx_outside_smx)
}

async fn vmx_launch_once_with_ept(
    lineage_record: LineageRecord,
    expected_lane_slot: usize,
    generation: u64,
) -> Result<LaunchResult, &'static str> {
    let vm_id = current_vm_id().ok_or("vm context missing")?;
    if !current_vmx_root_active()? {
        hvlogf(format_args!(
            "hv: vm{} reporting: vmx launch aborted: core contract not active slot={}",
            current_vm_id_for_log(),
            current_vmx_slot().unwrap_or(usize::MAX)
        ));
        return Err("vmx core contract inactive");
    }

    let assignment = vpid::assign_current_lane(vm_id, generation, expected_lane_slot)?;
    let run_result = vmx_launch_once_with_ept_vpid(lineage_record, &assignment).await;
    if let Err(retire_error) = assignment.retire() {
        if let Err(run_error) = &run_result {
            hverrorf(format_args!(
                "hv: vm{} reporting: VPID retirement superseded VM run error={} retirement_error={}",
                vm_id, run_error, retire_error
            ));
        }
        return Err(retire_error);
    }
    run_result
}

async fn vmx_launch_once_with_ept_vpid(
    lineage_record: LineageRecord,
    assignment: &vpid::VpidAssignment,
) -> Result<LaunchResult, &'static str> {
    let vm_id = current_vm_id().ok_or("vm context missing")?;
    let vm = vm_slot(vm_id);
    let vpid = assignment.vpid();

    let basic = unsafe { Msr::new(crate::hv::vmx::IA32_VMX_BASIC).read() };
    let revision = (basic & 0x7fff_ffff) as u32;

    let vmcs_va = current_vmcs_page()?;
    unsafe {
        core::ptr::write_bytes(vmcs_va, 0, VMX_PAGE_SIZE);
        *(vmcs_va as *mut u32) = revision;
    }

    let vmcs_pa = kernel_va_to_pa(vmcs_va as u64).ok_or("vmcs pa")?;
    hvlogf(format_args!(
        "hv: vm{} reporting: vmlaunch prep revision=0x{:08X} vmcs_pa=0x{:016X} root=core-contract",
        current_vm_id_for_log(),
        revision,
        vmcs_pa
    ));

    if !crate::hv::vmx::vmclear(vmcs_pa) {
        return Err("vmclear");
    }
    if !crate::hv::vmx::vmptrld(vmcs_pa) {
        return Err("vmptrld");
    }

    let eptp = match build_ept_identity_4g() {
        Ok(v) => v,
        Err(e) => return Err(e),
    };
    ACTIVE_EPTP_BY_VM[vm_id as usize].store(eptp, Ordering::Release);
    assignment.fence_ept(eptp)?;
    let reset_vmcall_transport = active_restore_meta(vm_id).is_none();
    if !crate::hv::vmcall::prepare_for_vm(vm_id, reset_vmcall_transport) {
        return Err("vmcall comm page");
    }
    let preemption_timer_enabled = setup_vmcs_host_and_controls(
        vm_id,
        Some(vpid),
        eptp,
        lineage_record,
        boot_mode_for_vm(vm_id),
    )?;
    let preemption_timer_ticks = preemption_timer_enabled.then(|| {
        let (ticks, rate_shift) =
            vmx_preemption_timer_ticks(crate::allcaps::hv::VMX_LIFECYCLE_PREEMPTION_QUANTUM_MS);
        hvlogf(format_args!(
            "hv: vm{} reporting: vmx preemption timer quantum_ms={} rate_shift={} ticks={}",
            vm_id,
            crate::allcaps::hv::VMX_LIFECYCLE_PREEMPTION_QUANTUM_MS,
            rate_shift,
            ticks
        ));
        ticks
    });
    crate::log!(
        "app-vm-run-queue: vmcs ready vm={} entry=0x{:016X}\n",
        vm_id,
        guest_entry_for_boot_mode(boot_mode_for_vm(vm_id))
    );

    // ── vmexit dispatch loop ──────────────────────────────────────────────────
    let mut lr = LaunchResult::default();
    let mut preserve_requested = false;
    let mut cpuid_leaf0_count = 0u32;
    let mut cpuid_leaf80000000_count = 0u32;
    let mut cpuid_leaf1_count = 0u32;
    let mut cpuid_other_count = 0u32;
    let mut first = true;
    'vmexit: loop {
        crate::smp::poll();
        if vm.is_some_and(|vm| vm.execution_pause.requested()) {
            clear_current_vm_id();
            let keep_running = park_vm_execution(vm_id).await;
            set_current_vm_id(vm_id);
            if !keep_running {
                break;
            }
        }
        if vm.map(immediate_stop_requested).unwrap_or(false) {
            hvlogf(format_args!(
                "hv: vm{} reporting: host stop request consumed before guest entry/resume",
                vm_id
            ));
            break;
        }

        assignment.verify_entry_lane()?;
        if let Some(ticks) = preemption_timer_ticks {
            vmwrite(VMCS_GUEST_VMCS_PREEMPT_TIMER, ticks as u64)?;
        }

        if first {
            crate::log!("app-vm-run-queue: vmlaunch begin vm={}\n", vm_id);
            vmlaunch_once_wrapper(vm_id, &mut lr);
            first = false;
        } else {
            vmresume_once_wrapper(vm_id, &mut lr);
        }

        if lr.launch_failed != 0 {
            hverrorf(format_args!(
                "hv: vm{} reporting: vmlaunch/vmresume failed instr_err={} rip=0x{:016X}",
                current_vm_id_for_log(),
                lr.instr_err,
                crate::hv::vmx::current_rip()
            ));
            break;
        }
        if lr.entered == 0 {
            hvwarnf(format_args!(
                "hv: vm{} reporting: vmlaunch/vmresume: guest not entered",
                current_vm_id_for_log()
            ));
            break;
        }

        crate::hv::security::before_host_handles_vmexit(vm_id);
        let reason = lr.exit_reason & 0xFFFF;
        crate::hv::vmx::log_vmexit_interrupt_info("vmexit");
        if reason == 0x0 {
            if depthchecker_debug::try_handoff(vm_id, &lr)? {
                continue;
            }
            let guest_exception = guest_exception_summary();
            if let Some((vector, vector_name, kind, info, err)) = guest_exception {
                hverrorf(format_args!(
                    "hv: vm{} fault-exc v={} {} type={}({}) err=0x{:X} info=0x{:08X}",
                    current_vm_id_for_log(),
                    vector,
                    vector_name,
                    kind,
                    crate::hv::vmx::decode_vmexit_int_type(kind),
                    err,
                    info as u32
                ));
            }
            let guest_rsp = vmread(VMCS_GUEST_RSP).unwrap_or(0);
            let guest_cr3 = vmread(VMCS_GUEST_CR3).unwrap_or(0);
            let guest_cr0 = vmread(VMCS_GUEST_CR0).unwrap_or(0);
            let guest_cr4 = vmread(VMCS_GUEST_CR4).unwrap_or(0);
            let guest_efer = vmread(VMCS_GUEST_IA32_EFER).unwrap_or(0);
            let guest_linear = vmread(VMCS_GUEST_LINEAR_ADDRESS).unwrap_or(0);
            let intr_err = vmread(VMCS_VMEXIT_INTERRUPTION_ERROR_CODE).unwrap_or(0);
            hvlogf(format_args!(
                "hv: vm{} reporting: pf-like err=0x{:X} present={} write={} user={} rsvd={} exec={}",
                current_vm_id_for_log(),
                intr_err,
                (intr_err & (1 << 0)) != 0,
                (intr_err & (1 << 1)) != 0,
                (intr_err & (1 << 2)) != 0,
                (intr_err & (1 << 3)) != 0,
                (intr_err & (1 << 4)) != 0
            ));
            hvlogf(format_args!(
                "hv: vm{} reporting: guest-state cr0=0x{:016X} cr3=0x{:016X} cr4=0x{:016X} efer=0x{:016X}",
                current_vm_id_for_log(),
                guest_cr0,
                guest_cr3,
                guest_cr4,
                guest_efer
            ));
            let regs = crate::hv::vmx::guest_registers();
            hverrorf(format_args!(
                "hv: vm{} fault-regs rip=0x{:016X} rsp=0x{:016X} rsi=0x{:016X} rdi=0x{:016X} rcx=0x{:016X} qual=0x{:016X}",
                current_vm_id_for_log(),
                lr.guest_rip,
                guest_rsp,
                regs.rsi,
                regs.rdi,
                regs.rcx,
                lr.exit_qualification
            ));
            hvlogf(format_args!(
                "hv: vm{} fault-regs2 rax=0x{:016X} rbx=0x{:016X} rcx=0x{:016X} rdx=0x{:016X} rbp=0x{:016X} r8=0x{:016X} r9=0x{:016X} r10=0x{:016X} r11=0x{:016X} r12=0x{:016X} r13=0x{:016X} r14=0x{:016X} r15=0x{:016X}",
                current_vm_id_for_log(),
                regs.rax,
                regs.rbx,
                regs.rcx,
                regs.rdx,
                regs.rbp,
                regs.r8,
                regs.r9,
                regs.r10,
                regs.r11,
                regs.r12,
                regs.r13,
                regs.r14,
                regs.r15
            ));
            hvlogf(format_args!(
                "hv: vm{} fault-regs3 r10=0x{:016X} r11=0x{:016X} r12=0x{:016X} r13=0x{:016X} r14=0x{:016X} r15=0x{:016X}",
                current_vm_id_for_log(),
                regs.r10,
                regs.r11,
                regs.r12,
                regs.r13,
                regs.r14,
                regs.r15
            ));
            let host_heap = crate::allocators::heap_stats();
            if host_heap.initialized && host_heap.heap_end > host_heap.heap_start {
                let in_host_heap = |addr: u64| {
                    let addr = addr as usize;
                    addr >= host_heap.heap_start && addr < host_heap.heap_end
                };
                hvlogf(format_args!(
                    "hv: vm{} reporting: pf-host-heap-risk src={} dst={} qual={} heap=0x{:016X}..0x{:016X} risk=HVSR-0002",
                    current_vm_id_for_log(),
                    in_host_heap(regs.rsi) as u8,
                    in_host_heap(regs.rdi) as u8,
                    in_host_heap(lr.exit_qualification) as u8,
                    host_heap.heap_start as u64,
                    host_heap.heap_end as u64
                ));
            }
            let memcpy_addr = crate::blueprint_shims::memcpy as *const () as usize as u64;
            if lr.guest_rip >= memcpy_addr && lr.guest_rip < memcpy_addr.saturating_add(128) {
                let (vector, vector_name, _, _, err) =
                    guest_exception.unwrap_or((0xFF, "unknown", 0, 0, intr_err));
                hverrorf(format_args!(
                    "hv: vm{} memcpy-fault v={} {} err=0x{:X} rip=0x{:016X} dst=0x{:016X} src=0x{:016X} len={} lin=0x{:016X}",
                    current_vm_id_for_log(),
                    vector,
                    vector_name,
                    err,
                    lr.guest_rip,
                    regs.rdi,
                    regs.rsi,
                    regs.rcx,
                    guest_linear
                ));
                crate::hv::memory::log_guest_mapping("fault-memcpy-dst", regs.rdi);
                crate::hv::memory::log_guest_mapping("fault-memcpy-src", regs.rsi);
            }
            crate::hv::memory::log_guest_mapping("fault-linear", guest_linear);
            crate::hv::memory::log_guest_mapping("fault-rsp", guest_rsp);
            crate::hv::memory::log_guest_mapping("fault-rip", lr.guest_rip);
            hvlogf(format_args!(
                "hv: vm{} reporting: fault-rip symbolize_hint=addr2line -e TRUEOS.full.elf 0x{:016X}",
                current_vm_id_for_log(),
                lr.guest_rip
            ));
            crate::hv::memory::log_guest_mapping_from_cr3("fault-linear", guest_cr3, guest_linear);
            crate::hv::memory::log_guest_mapping_from_cr3("fault-rip", guest_cr3, lr.guest_rip);
            crate::hv::memory::log_guest_code_bytes_from_cr3("fault-rip", guest_cr3, lr.guest_rip);
            crate::hv::memory::log_guest_phys_pt_context("fault-linear", guest_cr3, guest_linear);
            crate::hv::memory::log_guest_phys_pt_context("fault-rip", guest_cr3, lr.guest_rip);
            crate::hv::memory::log_guest_pt_context("fault-linear", guest_linear);
            crate::hv::memory::log_guest_pt_context("fault-rip", lr.guest_rip);
            let trace = crate::allocators::last_alloc_trace();
            if trace.seq != 0 {
                hvlogf(format_args!(
                    "hv: vm{} reporting: alloc-trace seq={} caller=0x{:016X} caller1=0x{:016X} caller2=0x{:016X} size={} align={} stage={} head=0x{:016X} block=0x{:016X} block_size={} next=0x{:016X} payload=0x{:016X} aligned_used={}",
                    current_vm_id_for_log(),
                    trace.seq,
                    trace.caller_rip,
                    trace.caller_rip_1,
                    trace.caller_rip_2,
                    trace.layout_size,
                    trace.layout_align,
                    trace.stage,
                    trace.head_ptr,
                    trace.block_ptr,
                    trace.block_size,
                    trace.block_next,
                    trace.payload_start,
                    trace.aligned_used
                ));
            }
        }

        match reason {
            VMEXIT_REASON_EXTERNAL_INTERRUPT => {
                // External-interrupt exiting is paired with acknowledge-on-exit.
                // The dedicated lifecycle vector is consumed here; every
                // other vector is replayed through the normal host IDT before
                // guest re-entry so its existing handler retains EOI ownership.
                handle_external_interrupt_vmexit(vm_id)?;
            }
            VMEXIT_REASON_VMCALL => {
                let len = vmread(VMCS_VMEXIT_INSTRUCTION_LEN).ok_or("vmread instr len")?;
                vmwrite(VMCS_GUEST_RIP, lr.guest_rip + len)?;

                let mut outcome = crate::hv::vmcall::dispatch(vm_id);
                'vmcall: loop {
                    match outcome {
                        crate::hv::vmcall::DispatchOutcome::Resume => break 'vmcall,
                        crate::hv::vmcall::DispatchOutcome::Stop => break 'vmexit,
                        #[cfg(feature = "legacy_vm_checkpoint")]
                        crate::hv::vmcall::DispatchOutcome::Pause => {
                            hvlogf(format_args!(
                                "hv: vm{} reporting: cooperative pause retained at rip=0x{:016X}",
                                vm_id, lr.guest_rip
                            ));
                            break 'vmexit;
                        }
                        crate::hv::vmcall::DispatchOutcome::Preserve => {
                            preserve_requested = true;
                            if let Some(vm) = vm {
                                vm.preserve_exit.store(true, Ordering::Release);
                            }
                            break 'vmexit;
                        }
                        crate::hv::vmcall::DispatchOutcome::Yield => {
                            clear_current_vm_id();
                            execution_policy::yield_executor_turn().await;
                            set_current_vm_id(vm_id);
                            break 'vmcall;
                        }
                        crate::hv::vmcall::DispatchOutcome::SleepMs(ms) => {
                            clear_current_vm_id();
                            if ms == 0 {
                                execution_policy::yield_executor_turn().await;
                            } else {
                                await_vm_control_boundary(
                                    vm_id,
                                    Timer::after(EmbassyDuration::from_millis(ms)),
                                )
                                .await;
                            }
                            set_current_vm_id(vm_id);
                            break 'vmcall;
                        }
                        crate::hv::vmcall::DispatchOutcome::WaitConsoleInput {
                            seq,
                            timeout_ms,
                        } => {
                            clear_current_vm_id();
                            let woke = await_vm_control_boundary(
                                vm_id,
                                wait_blueprint_console_input(vm_id, timeout_ms),
                            )
                            .await
                            .unwrap_or(false);
                            set_current_vm_id(vm_id);
                            crate::hv::vmcall::complete_console_input_wait(vm_id, seq, woke);
                            break 'vmcall;
                        }
                        crate::hv::vmcall::DispatchOutcome::PlatformWait {
                            seq,
                            key,
                            observed,
                            timeout_ms,
                        } => {
                            clear_current_vm_id();
                            let notified = await_vm_control_boundary(
                                vm_id,
                                crate::wait::platform_wait_after_for_vm_async(
                                    vm_id, key, observed, timeout_ms,
                                ),
                            )
                            .await
                            .unwrap_or(false);
                            set_current_vm_id(vm_id);
                            crate::smp::poll();
                            if vm.map(immediate_stop_requested).unwrap_or(false) {
                                hvlogf(format_args!(
                                    "hv: vm{} reporting: host stop request consumed during platform wait",
                                    vm_id
                                ));
                                break 'vmexit;
                            }
                            crate::hv::vmcall::complete_platform_wait(vm_id, seq, notified);
                            break 'vmcall;
                        }
                        crate::hv::vmcall::DispatchOutcome::RetryAfterMs(ms) => {
                            clear_current_vm_id();
                            await_vm_control_boundary(
                                vm_id,
                                Timer::after(EmbassyDuration::from_millis(ms.max(1))),
                            )
                            .await;
                            set_current_vm_id(vm_id);
                            crate::smp::poll();
                            if vm.map(immediate_stop_requested).unwrap_or(false) {
                                hvlogf(format_args!(
                                    "hv: vm{} reporting: host stop request consumed during pending vmcall",
                                    vm_id
                                ));
                                break 'vmexit;
                            }
                            outcome = crate::hv::vmcall::dispatch(vm_id);
                        }
                    }
                }
                // service vmcall — loop → vmresume
            }
            0xC => {
                // HLT — advance past it and continue
                let len = vmread(VMCS_VMEXIT_INSTRUCTION_LEN).ok_or("vmread instr len hlt")?;
                vmwrite(VMCS_GUEST_RIP, lr.guest_rip + len)?;
            }
            VMEXIT_REASON_PAUSE => {
                let len = vmread(VMCS_VMEXIT_INSTRUCTION_LEN).ok_or("vmread instr len pause")?;
                vmwrite(VMCS_GUEST_RIP, lr.guest_rip + len)?;
                clear_current_vm_id();
                Timer::after(EmbassyDuration::from_millis(1)).await;
                set_current_vm_id(vm_id);
            }
            VMEXIT_REASON_VMX_PREEMPTION_TIMER => {
                // Do not advance RIP. The timer exists to return control to
                // this loop so host stop/preserve requests are observed even
                // when guest code misses its cooperative yield point.
                // Native PAUSE loops also need bounded fairness for other
                // tasks on this AP, without imposing a sleep on every spin.
                clear_current_vm_id();
                execution_policy::yield_executor_turn().await;
                set_current_vm_id(vm_id);
            }
            0xA => {
                let mut regs = crate::hv::vmx::guest_registers();
                let leaf = regs.rax as u32;
                let subleaf = regs.rcx as u32;
                let out = __cpuid_count(leaf, subleaf);
                regs.rax = out.eax as u64;
                regs.rbx = guest_cpuid_ebx(leaf, subleaf, out.ebx) as u64;
                regs.rcx = out.ecx as u64;
                regs.rdx = out.edx as u64;
                crate::hv::vmx::set_guest_registers(regs);
                let len = vmread(VMCS_VMEXIT_INSTRUCTION_LEN).ok_or("vmread instr len cpuid")?;
                vmwrite(VMCS_GUEST_RIP, lr.guest_rip + len)?;
                match (leaf, subleaf) {
                    (0x0000_0000, 0) => cpuid_leaf0_count = cpuid_leaf0_count.saturating_add(1),
                    (0x8000_0000, 0) => {
                        cpuid_leaf80000000_count = cpuid_leaf80000000_count.saturating_add(1)
                    }
                    (0x0000_0001, 0) => cpuid_leaf1_count = cpuid_leaf1_count.saturating_add(1),
                    _ => {
                        cpuid_other_count = cpuid_other_count.saturating_add(1);
                        hvlogf(format_args!(
                            "hv: vm{} reporting: cpuid leaf=0x{:08X} subleaf=0x{:08X} -> eax=0x{:08X} ebx=0x{:08X} ecx=0x{:08X} edx=0x{:08X}",
                            current_vm_id_for_log(),
                            leaf,
                            subleaf,
                            out.eax,
                            regs.rbx as u32,
                            out.ecx,
                            out.edx
                        ));
                    }
                }
            }
            0x1F => {
                if !handle_guest_rdmsr(vm_id, lr.guest_rip)? {
                    break;
                }
            }
            0x20 => {
                if !handle_guest_wrmsr(vm_id, lr.guest_rip)? {
                    break;
                }
            }
            0x30 => {
                let guest_physical = vmread(VMCS_GUEST_PHYSICAL_ADDRESS).unwrap_or(0);
                let read = (lr.exit_qualification & (1 << 0)) != 0;
                let write = (lr.exit_qualification & (1 << 1)) != 0;
                let exec = (lr.exit_qualification & (1 << 2)) != 0;
                let gpa = (lr.exit_qualification & (1 << 8)) != 0;
                let gla = (lr.exit_qualification & (1 << 9)) != 0;
                hvlogf(format_args!(
                    "hv: vm{} reporting: ept violation qual=0x{:X} guest_physical=0x{:016X} access={}{}{} gpa_valid={} gla_valid={}",
                    current_vm_id_for_log(),
                    lr.exit_qualification,
                    guest_physical,
                    if read { "r" } else { "" },
                    if write { "w" } else { "" },
                    if exec { "x" } else { "" },
                    gpa as u8,
                    gla as u8
                ));
                break;
            }
            _ => {
                hvlogf(format_args!(
                    "hv: vm{} reporting: unhandled vmexit reason=0x{:X}, stopping",
                    current_vm_id_for_log(),
                    reason
                ));
                break;
            }
        }
        if vm
            .map(|vm| vm.preserve_req.swap(false, Ordering::AcqRel))
            .unwrap_or(false)
        {
            preserve_requested = true;
            if let Some(vm) = vm {
                vm.preserve_exit.store(true, Ordering::Release);
            }
            hvlogf(format_args!(
                "hv: vm{} reporting: host preserve request armed at rip=0x{:016X}",
                vm_id, lr.guest_rip
            ));
            break;
        }
        if vm.map(immediate_stop_requested).unwrap_or(false) {
            hvlogf(format_args!(
                "hv: vm{} reporting: host stop request consumed at rip=0x{:016X}",
                vm_id, lr.guest_rip
            ));
            break;
        }
    }
    if cpuid_leaf0_count != 0
        || cpuid_leaf80000000_count != 0
        || cpuid_leaf1_count != 0
        || cpuid_other_count != 0
    {
        hvlogf(format_args!(
            "hv: vm{} reporting: cpuid summary leaf0={} leaf80000000={} leaf1={} other={}",
            current_vm_id_for_log(),
            cpuid_leaf0_count,
            cpuid_leaf80000000_count,
            cpuid_leaf1_count,
            cpuid_other_count
        ));
    }
    if !preserve_requested || guest_kill_requested(vm_id) {
        if let Some(vm) = vm {
            vm.preserve_exit.store(false, Ordering::Release);
        }
    }
    Ok(lr)
}

fn guest_cpuid_ebx(leaf: u32, subleaf: u32, ebx: u32) -> u32 {
    if leaf != 0x0000_0001 || subleaf != 0 {
        return ebx;
    }

    let slot = crate::percpu::current_slot() as u32;
    let Some(profile) = crate::cpu::CpuProfile::for_slot(slot) else {
        return ebx;
    };
    (ebx & 0x00FF_FFFF) | ((profile.lapic_id() & 0xFF) << 24)
}

fn guest_rdmsr_value(vm_id: u8, msr: u32) -> Option<u64> {
    match msr {
        IA32_SYSENTER_CS => vmread(VMCS_GUEST_SYSENTER_CS),
        IA32_SYSENTER_ESP => vmread(VMCS_GUEST_SYSENTER_ESP),
        IA32_SYSENTER_EIP => vmread(VMCS_GUEST_SYSENTER_EIP),
        IA32_DEBUGCTL => vmread(VMCS_GUEST_IA32_DEBUGCTL),
        IA32_PAT => vmread(VMCS_GUEST_IA32_PAT),
        IA32_PERF_GLOBAL_CTRL => vmread(VMCS_GUEST_IA32_PERF_GLOBAL_CTRL),
        IA32_FS_BASE => vmread(VMCS_GUEST_FS_BASE),
        IA32_GS_BASE => vmread(VMCS_GUEST_GS_BASE),
        IA32_KERNEL_GS_BASE => Some(
            GUEST_KERNEL_GS_BASE_BY_VM
                .get(vm_id as usize)?
                .load(Ordering::Acquire),
        ),
        IA32_EFER => vmread(VMCS_GUEST_IA32_EFER),
        _ => None,
    }
}

fn write_guest_msr_value(vm_id: u8, msr: u32, value: u64) -> bool {
    match msr {
        IA32_SYSENTER_CS => vmwrite(VMCS_GUEST_SYSENTER_CS, value).is_ok(),
        IA32_SYSENTER_ESP => vmwrite(VMCS_GUEST_SYSENTER_ESP, value).is_ok(),
        IA32_SYSENTER_EIP => vmwrite(VMCS_GUEST_SYSENTER_EIP, value).is_ok(),
        IA32_DEBUGCTL => vmwrite(VMCS_GUEST_IA32_DEBUGCTL, value).is_ok(),
        IA32_PAT => vmwrite(VMCS_GUEST_IA32_PAT, value).is_ok(),
        IA32_PERF_GLOBAL_CTRL => vmwrite(VMCS_GUEST_IA32_PERF_GLOBAL_CTRL, value).is_ok(),
        IA32_FS_BASE => vmwrite(VMCS_GUEST_FS_BASE, value).is_ok(),
        IA32_GS_BASE => vmwrite(VMCS_GUEST_GS_BASE, value).is_ok(),
        IA32_KERNEL_GS_BASE => {
            let Some(slot) = GUEST_KERNEL_GS_BASE_BY_VM.get(vm_id as usize) else {
                return false;
            };
            slot.store(value, Ordering::Release);
            true
        }
        IA32_EFER => vmwrite(VMCS_GUEST_IA32_EFER, value).is_ok(),
        _ => false,
    }
}

fn handle_guest_rdmsr(vm_id: u8, guest_rip: u64) -> Result<bool, &'static str> {
    let mut regs = crate::hv::vmx::guest_registers();
    let msr = regs.rcx as u32;
    let Some(value) = guest_rdmsr_value(vm_id, msr) else {
        hvwarnf(format_args!(
            "hv: vm{} reporting: rdmsr unsupported msr=0x{:08X} rip=0x{:016X} risk={}",
            current_vm_id_for_log(),
            msr,
            guest_rip,
            crate::hv::security::HVSR_0004_GUEST_MSR_SURFACE
        ));
        return Ok(false);
    };

    // Securit Risk and a Id to it: HVSR-0004.
    // Keep guest RDMSR on an allowlist backed by VMCS guest state. This avoids
    // accidentally forwarding host-private MSRs while still letting shared-image
    // guest code read its FS/GS/percpu bases.
    regs.rax = value & 0xFFFF_FFFF;
    regs.rdx = value >> 32;
    crate::hv::vmx::set_guest_registers(regs);
    let len = vmread(VMCS_VMEXIT_INSTRUCTION_LEN).ok_or("vmread instr len rdmsr")?;
    vmwrite(VMCS_GUEST_RIP, guest_rip + len)?;
    Ok(true)
}

fn handle_guest_wrmsr(vm_id: u8, guest_rip: u64) -> Result<bool, &'static str> {
    let regs = crate::hv::vmx::guest_registers();
    let msr = regs.rcx as u32;
    let value = (regs.rax & 0xFFFF_FFFF) | ((regs.rdx & 0xFFFF_FFFF) << 32);
    if !write_guest_msr_value(vm_id, msr, value) {
        hvwarnf(format_args!(
            "hv: vm{} reporting: wrmsr unsupported msr=0x{:08X} value=0x{:016X} rip=0x{:016X} risk={}",
            current_vm_id_for_log(),
            msr,
            value,
            guest_rip,
            crate::hv::security::HVSR_0004_GUEST_MSR_SURFACE
        ));
        return Ok(false);
    }

    let len = vmread(VMCS_VMEXIT_INSTRUCTION_LEN).ok_or("vmread instr len wrmsr")?;
    vmwrite(VMCS_GUEST_RIP, guest_rip + len)?;
    Ok(true)
}

/// Populate the complete host state, legal controls, and Hull guest state.
fn setup_vmcs_host_and_controls(
    vm_id: u8,
    vpid: Option<u16>,
    eptp: u64,
    lineage_record: LineageRecord,
    boot_mode: VmBootMode,
) -> Result<bool, &'static str> {
    if vpid == Some(0) {
        return Err("zero VPID is forbidden");
    }
    let current_cpu_slot = crate::percpu::current_slot();
    if let Some(slot) = VMX_EXTERNAL_INTERRUPT_EXITING_BY_CPU.get(current_cpu_slot) {
        slot.store(false, Ordering::Release);
    }

    let basic = unsafe { Msr::new(crate::hv::vmx::IA32_VMX_BASIC).read() };
    let true_ctls = ((basic >> 55) & 1) != 0;
    let pin_msr = if true_ctls {
        crate::hv::vmx::IA32_VMX_TRUE_PINBASED_CTLS
    } else {
        0x481
    };
    let proc_msr = if true_ctls {
        crate::hv::vmx::IA32_VMX_TRUE_PROCBASED_CTLS
    } else {
        0x482
    };
    let exit_msr = if true_ctls {
        crate::hv::vmx::IA32_VMX_TRUE_EXIT_CTLS
    } else {
        0x483
    };
    let entry_msr = if true_ctls {
        crate::hv::vmx::IA32_VMX_TRUE_ENTRY_CTLS
    } else {
        0x484
    };

    let requested_pin = crate::hv::vmx::adjust_vmx_ctrl(
        pin_msr,
        PIN_BASED_VMX_PREEMPTION_TIMER | PIN_BASED_EXTERNAL_INTERRUPT_EXITING,
    );
    let requested_proc2 = PROC2_BASED_ENABLE_EPT
        | if vpid.is_some() {
            PROC2_BASED_ENABLE_VPID
        } else {
            0
        }
        | PROC2_BASED_ENABLE_VMFUNC;
    let proc2 =
        crate::hv::vmx::adjust_vmx_ctrl(crate::hv::vmx::IA32_VMX_PROCBASED_CTLS2, requested_proc2);
    let requested_exit = crate::hv::vmx::adjust_vmx_ctrl(
        exit_msr,
        EXIT_CTL_HOST_ADDR_SPACE_SIZE | EXIT_CTL_ACKNOWLEDGE_INTERRUPT_ON_EXIT,
    );
    let external_interrupt_exiting_enabled = (requested_pin & PIN_BASED_EXTERNAL_INTERRUPT_EXITING)
        != 0
        && (requested_exit & EXIT_CTL_ACKNOWLEDGE_INTERRUPT_ON_EXIT) != 0;
    let (pin, exit) = if external_interrupt_exiting_enabled {
        (requested_pin, requested_exit)
    } else {
        let fallback_pin = crate::hv::vmx::adjust_vmx_ctrl(pin_msr, PIN_BASED_VMX_PREEMPTION_TIMER);
        let fallback_exit =
            crate::hv::vmx::adjust_vmx_ctrl(exit_msr, EXIT_CTL_HOST_ADDR_SPACE_SIZE);
        if (fallback_pin & PIN_BASED_EXTERNAL_INTERRUPT_EXITING) != 0
            || (fallback_exit & EXIT_CTL_ACKNOWLEDGE_INTERRUPT_ON_EXIT) != 0
        {
            return Err("external-interrupt VM-exit controls cannot be paired");
        }
        (fallback_pin, fallback_exit)
    };
    let preemption_timer_enabled = (pin & PIN_BASED_VMX_PREEMPTION_TIMER) != 0;
    let pause_exiting_requested = execution_policy::intercept_pause(
        preemption_timer_enabled,
        crate::allcaps::hv::VMX_NATIVE_PAUSE_WITH_TIMER,
    );
    // PAUSE is emitted by Rust synchronization and completion waits as well
    // as guest spin loops. With the hardware timer active, execute that hint
    // natively instead of converting every spin into a VM exit + 1 ms sleep.
    // Explicit OP_YIELD, positive sleeps, HLT and lifecycle exits remain live.
    let proc = crate::hv::vmx::adjust_vmx_ctrl(
        proc_msr,
        PROC_BASED_HLT_EXITING
            | if pause_exiting_requested {
                PROC_BASED_PAUSE_EXITING
            } else {
                0
            }
            | PROC_BASED_ACTIVATE_SECONDARY
            | PROC_BASED_USE_TSC_OFFSETTING,
    );
    let requested_entry = { ENTRY_CTL_IA32E_MODE_GUEST };
    let entry = crate::hv::vmx::adjust_vmx_ctrl(entry_msr, requested_entry);
    {
        hvlogf(format_args!(
            "hv: vm{}-{} reporting: vmcs controls pin=0x{:08X} proc=0x{:08X} proc2=0x{:08X} exit=0x{:08X} entry=0x{:08X} vpid={} pause_exit={} timer={}",
            current_vm_id_for_log(),
            lineage_record.level,
            pin as u32,
            proc as u32,
            proc2 as u32,
            exit as u32,
            entry as u32,
            vpid.unwrap_or(0),
            u8::from(proc & PROC_BASED_PAUSE_EXITING != 0),
            u8::from(preemption_timer_enabled),
        ));
    }

    if (proc & PROC_BASED_ACTIVATE_SECONDARY) == 0 {
        hvwarnf(format_args!(
            "hv: vm{}-{} reporting: vmcs ctrl unsupported: primary bit ACTIVATE_SECONDARY not available",
            current_vm_id_for_log(),
            lineage_record.level
        ));
        return Err("secondary controls unsupported");
    }
    if pause_exiting_requested && (proc & PROC_BASED_PAUSE_EXITING) == 0 {
        hvwarnf(format_args!(
            "hv: vm{}-{} reporting: vmcs ctrl unsupported: primary bit PAUSE_EXITING not available",
            current_vm_id_for_log(),
            lineage_record.level
        ));
    }
    if !preemption_timer_enabled {
        hvwarnf(format_args!(
            "hv: vm{}-{} reporting: vmcs ctrl unsupported: pin bit VMX_PREEMPTION_TIMER not available; lifecycle stop remains cooperative",
            current_vm_id_for_log(),
            lineage_record.level
        ));
    }
    if !external_interrupt_exiting_enabled {
        hvwarnf(format_args!(
            "hv: vm{}-{} reporting: vmcs ctrl unsupported: external-interrupt exiting/acknowledge pair unavailable; targeted lifecycle kick disabled",
            current_vm_id_for_log(),
            lineage_record.level
        ));
    }
    if (proc2 & PROC2_BASED_ENABLE_EPT) == 0 {
        hvwarnf(format_args!(
            "hv: vm{}-{} reporting: vmcs ctrl unsupported: secondary bit ENABLE_EPT not available",
            current_vm_id_for_log(),
            lineage_record.level
        ));
        return Err("ept unsupported");
    }
    if vpid.is_some() && (proc2 & PROC2_BASED_ENABLE_VPID) == 0 {
        hvwarnf(format_args!(
            "hv: vm{}-{} reporting: vmcs ctrl unsupported: secondary bit ENABLE_VPID not available",
            current_vm_id_for_log(),
            lineage_record.level
        ));
        return Err("vpid unsupported");
    }
    if vpid.is_none() && (proc2 & PROC2_BASED_ENABLE_VPID) != 0 {
        return Err("unexpected VPID control");
    }

    vmwrite(VMCS_CTRL_PIN_BASED, pin)?;
    if let Some(vpid) = vpid {
        vmwrite(VMCS_CTRL_VPID, u64::from(vpid))?;
    }
    vmwrite(VMCS_CTRL_CPU_BASED, proc)?;
    vmwrite(VMCS_CTRL_SECONDARY, proc2)?;
    vmwrite(VMCS_CTRL_EXCEPTION_BITMAP, EXCEPTION_BITMAP_ALL)?;
    vmwrite(VMCS_CTRL_EXIT, exit)?;
    vmwrite(VMCS_CTRL_ENTRY, entry)?;
    vmwrite(VMCS_CTRL_EPT_POINTER, eptp)?;
    vmwrite(VMCS_CTRL_VMCS_LINK_POINTER, !0u64)?;
    // TSC offset: 0 = transparent pass-through; snapshot-restore can set delta later
    vmwrite(VMCS_TSC_OFFSET, 0u64)?;
    // EPTP switching: slot 0 = identity EPT; guest uses vmfunc(0, idx) to switch namespaces
    if (proc2 & PROC2_BASED_ENABLE_VMFUNC) != 0 {
        let eptp_list_pa = memory::init_eptp_list(eptp)?;
        vmwrite(VMCS_CTRL_VMFUNC_CONTROLS, VMFUNC_EPTP_SWITCHING)?;
        vmwrite(VMCS_CTRL_EPTP_LIST_ADDR, eptp_list_pa)?;
    }
    if let Some(slot) = VMX_EXTERNAL_INTERRUPT_EXITING_BY_CPU.get(current_cpu_slot) {
        slot.store(external_interrupt_exiting_enabled, Ordering::Release);
    }

    let (host_cr3, _) = Cr3::read();
    let host_cr0 = Cr0::read().bits();
    let host_cr4 = Cr4::read().bits();
    let guest_rflags = rflags::read().bits();
    let mut tr_sel = crate::hv::vmx::read_tr_selector();
    let gdtr = sgdt();
    let idtr = sidt();
    let mut host_gdtr_base = gdtr.base.as_u64();
    let mut host_cs = (CS::get_reg().0 & !0x7) as u64;
    let mut host_ss = (SS::get_reg().0 & !0x7) as u64;
    let mut host_ds = (DS::get_reg().0 & !0x7) as u64;
    let mut host_es = (ES::get_reg().0 & !0x7) as u64;
    let mut host_fs = (FS::get_reg().0 & !0x7) as u64;
    let mut host_gs = (GS::get_reg().0 & !0x7) as u64;
    let tr_base: u64;

    if tr_sel == 0 {
        if let Some((busy_sel, 0xB)) =
            crate::hv::vmx::find_tss_selector(gdtr.base.as_u64(), gdtr.limit)
        {
            tr_sel = busy_sel;
            hvlogf(format_args!(
                "hv: vm{}-{} reporting: host-state recovered: adopted busy tss selector=0x{:04X}",
                current_vm_id_for_log(),
                lineage_record.level,
                tr_sel
            ));
        } else if let Some((avail_sel, 0x9)) =
            crate::hv::vmx::find_tss_selector(gdtr.base.as_u64(), gdtr.limit)
        {
            crate::hv::vmx::load_tr_selector(avail_sel);
            tr_sel = crate::hv::vmx::read_tr_selector();
            if tr_sel == 0 {
                hvlogf(format_args!(
                    "hv: vm{}-{} reporting: host-state invalid: tr selector null after ltr candidate=0x{:04X}",
                    current_vm_id_for_log(),
                    lineage_record.level,
                    avail_sel
                ));
                return Err("host tr ltr");
            }
            hvlogf(format_args!(
                "hv: vm{}-{} reporting: host-state recovered: loaded tr selector=0x{:04X}",
                current_vm_id_for_log(),
                lineage_record.level,
                tr_sel
            ));
        } else {
            let synth = crate::hv::vmx::synthesize_host_gdt_tss();
            tr_sel = synth.tr_sel;
            host_gdtr_base = synth.gdt_base;
            host_cs = synth.cs_sel as u64;
            host_ss = synth.data_sel as u64;
            host_ds = synth.data_sel as u64;
            host_es = synth.data_sel as u64;
            host_fs = 0;
            host_gs = 0;
            tr_base = synth.tr_base;
            hvlogf(format_args!(
                "hv: vm{}-{} reporting: host-state recovered: using synthetic hv gdt+tss tr=0x{:04X} tr_base=0x{:016X}",
                current_vm_id_for_log(),
                lineage_record.level,
                synth.tr_sel,
                synth.tr_base
            ));
            let fs_base = unsafe { Msr::new(crate::hv::vmx::IA32_FS_BASE).read() };
            let gs_base = unsafe { Msr::new(crate::hv::vmx::IA32_GS_BASE).read() };
            let guest_fs_base = guest_fs_base_for_boot_mode(boot_mode);
            let sysenter_cs = unsafe { Msr::new(crate::hv::vmx::IA32_SYSENTER_CS).read() };
            let sysenter_esp = unsafe { Msr::new(crate::hv::vmx::IA32_SYSENTER_ESP).read() };
            let sysenter_eip = unsafe { Msr::new(crate::hv::vmx::IA32_SYSENTER_EIP).read() };
            let r0 = __cpuid(0);
            let r1 = __cpuid(1);
            let has_pat = (r1.edx & (1 << 16)) != 0;
            let has_perfmon = r0.eax >= 0xA && (__cpuid(0xA).eax & 0xFF) != 0;
            let pat = if has_pat {
                unsafe { Msr::new(crate::hv::vmx::IA32_PAT).read() }
            } else {
                0x0007_0406_0007_0406
            };
            let perf_global = if has_perfmon {
                unsafe { Msr::new(crate::hv::vmx::IA32_PERF_GLOBAL_CTRL).read() }
            } else {
                0
            };
            let efer = unsafe { Msr::new(crate::hv::vmx::IA32_EFER).read() };
            let host_tr = (tr_sel & !0x7) as u64;
            let host_sysenter_cs = sysenter_cs & 0xFFFF;

            if host_cs == 0 || host_ss == 0 || host_tr == 0 {
                hvlogf(format_args!(
                    "hv: vm{}-{} reporting: host-state invalid selectors cs=0x{:04X} ss=0x{:04X} tr=0x{:04X}",
                    current_vm_id_for_log(),
                    lineage_record.level,
                    host_cs as u16,
                    host_ss as u16,
                    host_tr as u16
                ));
                return Err("host selectors");
            }
            if !crate::hv::vmx::is_canonical(tr_base)
                || !crate::hv::vmx::is_canonical(fs_base)
                || !crate::hv::vmx::is_canonical(gs_base)
                || !crate::hv::vmx::is_canonical(host_gdtr_base)
                || !crate::hv::vmx::is_canonical(idtr.base.as_u64())
            {
                hvlogf(format_args!(
                    "hv: vm{}-{} reporting: host-state invalid bases tr=0x{:016X} fs=0x{:016X} gs=0x{:016X} gdtr=0x{:016X} idtr=0x{:016X}",
                    current_vm_id_for_log(),
                    lineage_record.level,
                    tr_base,
                    fs_base,
                    gs_base,
                    host_gdtr_base,
                    idtr.base.as_u64()
                ));
                return Err("host bases");
            }
            {
                hvlogf(format_args!(
                    "hv: vm{}-{} reporting: host-state cs=0x{:04X} ss=0x{:04X} tr=0x{:04X} tr_base=0x{:016X}",
                    current_vm_id_for_log(),
                    lineage_record.level,
                    host_cs as u16,
                    host_ss as u16,
                    host_tr as u16,
                    tr_base
                ));
            }

            vmwrite(VMCS_HOST_CR0, host_cr0)?;
            vmwrite(VMCS_HOST_CR3, host_cr3.start_address().as_u64())?;
            vmwrite(VMCS_HOST_CR4, host_cr4)?;
            vmwrite(VMCS_HOST_CS_SELECTOR, host_cs)?;
            vmwrite(VMCS_HOST_SS_SELECTOR, host_ss)?;
            vmwrite(VMCS_HOST_DS_SELECTOR, host_ds)?;
            vmwrite(VMCS_HOST_ES_SELECTOR, host_es)?;
            vmwrite(VMCS_HOST_FS_SELECTOR, host_fs)?;
            vmwrite(VMCS_HOST_GS_SELECTOR, host_gs)?;
            vmwrite(VMCS_HOST_TR_SELECTOR, host_tr)?;
            vmwrite(VMCS_HOST_FS_BASE, fs_base)?;
            vmwrite(VMCS_HOST_GS_BASE, gs_base)?;
            vmwrite(VMCS_HOST_TR_BASE, tr_base)?;
            vmwrite(VMCS_HOST_GDTR_BASE, host_gdtr_base)?;
            vmwrite(VMCS_HOST_IDTR_BASE, idtr.base.as_u64())?;
            vmwrite(VMCS_HOST_SYSENTER_CS, host_sysenter_cs)?;
            vmwrite(VMCS_HOST_SYSENTER_ESP, sysenter_esp)?;
            vmwrite(VMCS_HOST_SYSENTER_EIP, sysenter_eip)?;
            vmwrite(VMCS_HOST_IA32_PAT, pat)?;
            vmwrite(VMCS_HOST_IA32_EFER, efer)?;
            vmwrite(VMCS_HOST_IA32_PERF_GLOBAL_CTRL, perf_global)?;

            let restored = active_restore_meta(vm_id);
            let guest_rip = restored
                .map(|m| m.guest_rip)
                .unwrap_or_else(|| guest_entry_for_boot_mode(boot_mode));
            let guest_rsp = restored
                .map(|m| m.guest_rsp)
                .unwrap_or_else(|| guest_stack_top_for_vm(vm_id));
            // Snapshot page tables describe the old physical stack arena.
            // Rebuild the deterministic mappings around the restored logical
            // RIP/RSP so the copied stack and current per-VM backings are used.
            let guest_cr3 =
                build_guest_cr3_for_vm_with_mode(vm_id, guest_rip, guest_rsp, boot_mode)?;
            let launch_guest_rflags = if let Some(restored) = restored {
                crate::hv::vmx::set_guest_registers(restored.guest_registers);
                crate::hv::vmx::restore_guest_extended_state(
                    vm_id,
                    restored.guest_extended_state_mask,
                    &restored.guest_extended_state,
                )?;
                restored.guest_rflags
            } else {
                crate::hv::vmx::reset_guest_registers();
                crate::hv::vmx::reset_guest_extended_state(vm_id)?;
                guest_rflags
            };
            vmwrite(VMCS_GUEST_CR0, host_cr0)?;
            vmwrite(VMCS_GUEST_CR3, guest_cr3)?;
            vmwrite(VMCS_GUEST_CR4, host_cr4)?;
            vmwrite(VMCS_GUEST_RFLAGS, (launch_guest_rflags | RFLAGS_RESERVED_BIT1) & !RFLAGS_IF)?;
            vmwrite(VMCS_GUEST_RIP, guest_rip)?;
            vmwrite(VMCS_GUEST_RSP, guest_rsp)?;
            vmwrite(VMCS_GUEST_DR7, 0x400)?;
            vmwrite(VMCS_GUEST_IA32_DEBUGCTL, 0)?;
            vmwrite(VMCS_GUEST_SYSENTER_CS, sysenter_cs)?;
            vmwrite(VMCS_GUEST_SYSENTER_ESP, sysenter_esp)?;
            vmwrite(VMCS_GUEST_SYSENTER_EIP, sysenter_eip)?;
            vmwrite(VMCS_GUEST_IA32_PAT, pat)?;
            vmwrite(VMCS_GUEST_IA32_EFER, efer)?;
            vmwrite(VMCS_GUEST_IA32_PERF_GLOBAL_CTRL, perf_global)?;
            vmwrite(VMCS_GUEST_ACTIVITY_STATE, 0)?;
            vmwrite(VMCS_GUEST_INTERRUPTIBILITY, 0)?;
            vmwrite(VMCS_GUEST_PENDING_DBG, 0)?;
            vmwrite(VMCS_GUEST_VMCS_PREEMPT_TIMER, 0)?;

            let cs = { host_cs };
            let ss = { host_ss };
            let ds = { host_ds };
            let es = { host_es };
            let fs = { host_fs };
            let gs = { host_gs };
            let tr = tr_sel as u64;
            vmwrite(VMCS_GUEST_CS_SELECTOR, cs)?;
            vmwrite(VMCS_GUEST_SS_SELECTOR, ss)?;
            vmwrite(VMCS_GUEST_DS_SELECTOR, ds)?;
            vmwrite(VMCS_GUEST_ES_SELECTOR, es)?;
            vmwrite(VMCS_GUEST_FS_SELECTOR, fs)?;
            vmwrite(VMCS_GUEST_GS_SELECTOR, gs)?;
            vmwrite(VMCS_GUEST_TR_SELECTOR, tr)?;
            vmwrite(VMCS_GUEST_LDTR_SELECTOR, 0)?;

            vmwrite(VMCS_GUEST_CS_LIMIT, 0xFFFF_FFFF)?;
            vmwrite(VMCS_GUEST_SS_LIMIT, 0xFFFF_FFFF)?;
            vmwrite(VMCS_GUEST_DS_LIMIT, 0xFFFF_FFFF)?;
            vmwrite(VMCS_GUEST_ES_LIMIT, 0xFFFF_FFFF)?;
            vmwrite(VMCS_GUEST_FS_LIMIT, 0xFFFF_FFFF)?;
            vmwrite(VMCS_GUEST_GS_LIMIT, 0xFFFF_FFFF)?;
            vmwrite(VMCS_GUEST_TR_LIMIT, 0xFFFF)?;
            vmwrite(VMCS_GUEST_LDTR_LIMIT, 0)?;
            vmwrite(VMCS_GUEST_GDTR_LIMIT, gdtr.limit as u64)?;
            vmwrite(VMCS_GUEST_IDTR_LIMIT, idtr.limit as u64)?;

            vmwrite(VMCS_GUEST_CS_BASE, 0)?;
            vmwrite(VMCS_GUEST_SS_BASE, 0)?;
            vmwrite(VMCS_GUEST_DS_BASE, 0)?;
            vmwrite(VMCS_GUEST_ES_BASE, 0)?;
            vmwrite(VMCS_GUEST_FS_BASE, guest_fs_base)?;
            vmwrite(VMCS_GUEST_GS_BASE, gs_base)?;
            vmwrite(VMCS_GUEST_TR_BASE, tr_base)?;
            vmwrite(VMCS_GUEST_LDTR_BASE, 0)?;
            vmwrite(VMCS_GUEST_GDTR_BASE, gdtr.base.as_u64())?;
            vmwrite(VMCS_GUEST_IDTR_BASE, idtr.base.as_u64())?;

            vmwrite(VMCS_GUEST_CS_AR, { 0xA09B })?;
            vmwrite(VMCS_GUEST_SS_AR, 0xC093)?;
            vmwrite(VMCS_GUEST_DS_AR, 0xC093)?;
            vmwrite(VMCS_GUEST_ES_AR, 0xC093)?;
            vmwrite(VMCS_GUEST_FS_AR, { 0x10000 })?;
            vmwrite(VMCS_GUEST_GS_AR, 0x10000)?;
            vmwrite(VMCS_GUEST_TR_AR, 0x008B)?;
            vmwrite(VMCS_GUEST_LDTR_AR, 0x10000)?;

            return Ok(preemption_timer_enabled);
        }
    }
    if tr_sel == 0 {
        hvlogf(format_args!(
            "hv: vm{}-{} reporting: host-state invalid: tr selector remains null after recovery",
            current_vm_id_for_log(),
            lineage_record.level
        ));
        return Err("host tr selector");
    }
    tr_base = match crate::hv::vmx::tss_base_from_gdt(host_gdtr_base, tr_sel) {
        Some(v) => v,
        None => {
            hvlogf(format_args!(
                "hv: vm{}-{} reporting: host-state invalid: unable to resolve tss base from gdt",
                current_vm_id_for_log(),
                lineage_record.level
            ));
            return Err("host tr base");
        }
    };
    let fs_base = unsafe { Msr::new(crate::hv::vmx::IA32_FS_BASE).read() };
    let gs_base = unsafe { Msr::new(crate::hv::vmx::IA32_GS_BASE).read() };
    let guest_fs_base = guest_fs_base_for_boot_mode(boot_mode);
    let sysenter_cs = unsafe { Msr::new(crate::hv::vmx::IA32_SYSENTER_CS).read() };
    let sysenter_esp = unsafe { Msr::new(crate::hv::vmx::IA32_SYSENTER_ESP).read() };
    let sysenter_eip = unsafe { Msr::new(crate::hv::vmx::IA32_SYSENTER_EIP).read() };
    let r0 = __cpuid(0);
    let r1 = __cpuid(1);
    let has_pat = (r1.edx & (1 << 16)) != 0;
    let has_perfmon = r0.eax >= 0xA && (__cpuid(0xA).eax & 0xFF) != 0;
    let pat = if has_pat {
        unsafe { Msr::new(crate::hv::vmx::IA32_PAT).read() }
    } else {
        0x0007_0406_0007_0406
    };
    let perf_global = if has_perfmon {
        unsafe { Msr::new(crate::hv::vmx::IA32_PERF_GLOBAL_CTRL).read() }
    } else {
        0
    };
    let efer = unsafe { Msr::new(crate::hv::vmx::IA32_EFER).read() };

    let host_tr = (tr_sel & !0x7) as u64;
    let host_sysenter_cs = sysenter_cs & 0xFFFF;

    if host_cs == 0 || host_ss == 0 || host_tr == 0 {
        hvlogf(format_args!(
            "hv: vm{}-{} reporting: host-state invalid selectors cs=0x{:04X} ss=0x{:04X} tr=0x{:04X}",
            current_vm_id_for_log(),
            lineage_record.level,
            host_cs as u16,
            host_ss as u16,
            host_tr as u16
        ));
        return Err("host selectors");
    }
    if !crate::hv::vmx::is_canonical(tr_base)
        || !crate::hv::vmx::is_canonical(fs_base)
        || !crate::hv::vmx::is_canonical(gs_base)
        || !crate::hv::vmx::is_canonical(host_gdtr_base)
        || !crate::hv::vmx::is_canonical(idtr.base.as_u64())
    {
        hvlogf(format_args!(
            "hv: vm{}-{} reporting: host-state invalid bases tr=0x{:016X} fs=0x{:016X} gs=0x{:016X} gdtr=0x{:016X} idtr=0x{:016X}",
            current_vm_id_for_log(),
            lineage_record.level,
            tr_base,
            fs_base,
            gs_base,
            host_gdtr_base,
            idtr.base.as_u64()
        ));
        return Err("host bases");
    }
    {
        hvlogf(format_args!(
            "hv: vm{}-{} reporting: host-state cs=0x{:04X} ss=0x{:04X} tr=0x{:04X} tr_base=0x{:016X}",
            current_vm_id_for_log(),
            lineage_record.level,
            host_cs as u16,
            host_ss as u16,
            host_tr as u16,
            tr_base
        ));
    }

    let (host_cr3, _) = Cr3::read();
    vmwrite(VMCS_HOST_CR0, host_cr0)?;
    vmwrite(VMCS_HOST_CR3, host_cr3.start_address().as_u64())?;
    vmwrite(VMCS_HOST_CR4, host_cr4)?;
    vmwrite(VMCS_HOST_CS_SELECTOR, host_cs)?;
    vmwrite(VMCS_HOST_SS_SELECTOR, host_ss)?;
    vmwrite(VMCS_HOST_DS_SELECTOR, host_ds)?;
    vmwrite(VMCS_HOST_ES_SELECTOR, host_es)?;
    vmwrite(VMCS_HOST_FS_SELECTOR, host_fs)?;
    vmwrite(VMCS_HOST_GS_SELECTOR, host_gs)?;
    vmwrite(VMCS_HOST_TR_SELECTOR, host_tr)?;
    vmwrite(VMCS_HOST_FS_BASE, fs_base)?;
    vmwrite(VMCS_HOST_GS_BASE, gs_base)?;
    vmwrite(VMCS_HOST_TR_BASE, tr_base)?;
    vmwrite(VMCS_HOST_GDTR_BASE, host_gdtr_base)?;
    vmwrite(VMCS_HOST_IDTR_BASE, idtr.base.as_u64())?;
    vmwrite(VMCS_HOST_SYSENTER_CS, host_sysenter_cs)?;
    vmwrite(VMCS_HOST_SYSENTER_ESP, sysenter_esp)?;
    vmwrite(VMCS_HOST_SYSENTER_EIP, sysenter_eip)?;
    vmwrite(VMCS_HOST_IA32_PAT, pat)?;
    vmwrite(VMCS_HOST_IA32_EFER, efer)?;
    vmwrite(VMCS_HOST_IA32_PERF_GLOBAL_CTRL, perf_global)?;

    let restored = active_restore_meta(vm_id);
    let guest_rip = restored
        .map(|m| m.guest_rip)
        .unwrap_or_else(|| guest_entry_for_boot_mode(boot_mode));
    let guest_rsp = restored
        .map(|m| m.guest_rsp)
        .unwrap_or_else(|| guest_stack_top_for_vm(vm_id));
    // Never reuse physical addresses embedded in the stored page tables. The
    // stack was copied into a new arena, so rebuild mappings for current VM
    // backings while preserving the checkpoint's logical execution state.
    let guest_cr3 = build_guest_cr3_for_vm_with_mode(vm_id, guest_rip, guest_rsp, boot_mode)?;
    let launch_guest_rflags = if let Some(restored) = restored {
        crate::hv::vmx::set_guest_registers(restored.guest_registers);
        crate::hv::vmx::restore_guest_extended_state(
            vm_id,
            restored.guest_extended_state_mask,
            &restored.guest_extended_state,
        )?;
        restored.guest_rflags
    } else {
        crate::hv::vmx::reset_guest_registers();
        crate::hv::vmx::reset_guest_extended_state(vm_id)?;
        guest_rflags
    };
    vmwrite(VMCS_GUEST_CR0, host_cr0)?;
    vmwrite(VMCS_GUEST_CR3, guest_cr3)?;
    vmwrite(VMCS_GUEST_CR4, host_cr4)?;
    vmwrite(VMCS_GUEST_RFLAGS, (launch_guest_rflags | RFLAGS_RESERVED_BIT1) & !RFLAGS_IF)?;
    vmwrite(VMCS_GUEST_RIP, guest_rip)?;
    vmwrite(VMCS_GUEST_RSP, guest_rsp)?;
    vmwrite(VMCS_GUEST_DR7, 0x400)?;
    vmwrite(VMCS_GUEST_IA32_DEBUGCTL, 0)?;
    vmwrite(VMCS_GUEST_SYSENTER_CS, sysenter_cs)?;
    vmwrite(VMCS_GUEST_SYSENTER_ESP, sysenter_esp)?;
    vmwrite(VMCS_GUEST_SYSENTER_EIP, sysenter_eip)?;
    vmwrite(VMCS_GUEST_IA32_PAT, pat)?;
    vmwrite(VMCS_GUEST_IA32_EFER, efer)?;
    vmwrite(VMCS_GUEST_IA32_PERF_GLOBAL_CTRL, perf_global)?;
    vmwrite(VMCS_GUEST_ACTIVITY_STATE, 0)?;
    vmwrite(VMCS_GUEST_INTERRUPTIBILITY, 0)?;
    vmwrite(VMCS_GUEST_PENDING_DBG, 0)?;
    vmwrite(VMCS_GUEST_VMCS_PREEMPT_TIMER, 0)?;

    let cs = { CS::get_reg().0 as u64 };
    let ss = { SS::get_reg().0 as u64 };
    let ds = { DS::get_reg().0 as u64 };
    let es = { ES::get_reg().0 as u64 };
    let fs = { FS::get_reg().0 as u64 };
    let gs = { GS::get_reg().0 as u64 };
    let tr = tr_sel as u64;
    vmwrite(VMCS_GUEST_CS_SELECTOR, cs)?;
    vmwrite(VMCS_GUEST_SS_SELECTOR, ss)?;
    vmwrite(VMCS_GUEST_DS_SELECTOR, ds)?;
    vmwrite(VMCS_GUEST_ES_SELECTOR, es)?;
    vmwrite(VMCS_GUEST_FS_SELECTOR, fs)?;
    vmwrite(VMCS_GUEST_GS_SELECTOR, gs)?;
    vmwrite(VMCS_GUEST_TR_SELECTOR, tr)?;
    vmwrite(VMCS_GUEST_LDTR_SELECTOR, 0)?;

    vmwrite(VMCS_GUEST_CS_LIMIT, 0xFFFF_FFFF)?;
    vmwrite(VMCS_GUEST_SS_LIMIT, 0xFFFF_FFFF)?;
    vmwrite(VMCS_GUEST_DS_LIMIT, 0xFFFF_FFFF)?;
    vmwrite(VMCS_GUEST_ES_LIMIT, 0xFFFF_FFFF)?;
    vmwrite(VMCS_GUEST_FS_LIMIT, 0xFFFF_FFFF)?;
    vmwrite(VMCS_GUEST_GS_LIMIT, 0xFFFF_FFFF)?;
    vmwrite(VMCS_GUEST_TR_LIMIT, 0xFFFF)?;
    vmwrite(VMCS_GUEST_LDTR_LIMIT, 0)?;
    vmwrite(VMCS_GUEST_GDTR_LIMIT, gdtr.limit as u64)?;
    vmwrite(VMCS_GUEST_IDTR_LIMIT, idtr.limit as u64)?;

    vmwrite(VMCS_GUEST_CS_BASE, 0)?;
    vmwrite(VMCS_GUEST_SS_BASE, 0)?;
    vmwrite(VMCS_GUEST_DS_BASE, 0)?;
    vmwrite(VMCS_GUEST_ES_BASE, 0)?;
    vmwrite(VMCS_GUEST_FS_BASE, guest_fs_base)?;
    vmwrite(VMCS_GUEST_GS_BASE, gs_base)?;
    vmwrite(VMCS_GUEST_TR_BASE, tr_base)?;
    vmwrite(VMCS_GUEST_LDTR_BASE, 0)?;
    vmwrite(VMCS_GUEST_GDTR_BASE, gdtr.base.as_u64())?;
    vmwrite(VMCS_GUEST_IDTR_BASE, idtr.base.as_u64())?;

    vmwrite(VMCS_GUEST_CS_AR, { 0xA09B })?;
    vmwrite(VMCS_GUEST_SS_AR, if ss == 0 { 0x10000 } else { 0xC093 })?;
    vmwrite(VMCS_GUEST_DS_AR, if ds == 0 { 0x10000 } else { 0xC093 })?;
    vmwrite(VMCS_GUEST_ES_AR, if es == 0 { 0x10000 } else { 0xC093 })?;
    vmwrite(VMCS_GUEST_FS_AR, if fs == 0 { 0x10000 } else { 0xC093 })?;
    vmwrite(VMCS_GUEST_GS_AR, if gs == 0 { 0x10000 } else { 0xC093 })?;
    vmwrite(VMCS_GUEST_TR_AR, 0x008B)?;
    vmwrite(VMCS_GUEST_LDTR_AR, 0x10000)?;

    Ok(preemption_timer_enabled)
}

fn vmx_preemption_timer_ticks(quantum_ms: u64) -> (u32, u8) {
    let misc = unsafe { Msr::new(crate::hv::vmx::IA32_VMX_MISC).read() };
    let rate_shift = (misc & 0x1F) as u8;
    let divisor = 1u128 << rate_shift;
    let tsc_ticks = (crate::time::tsc_hz() as u128)
        .saturating_mul(quantum_ms as u128)
        .saturating_add(999)
        / 1_000;
    let timer_ticks = tsc_ticks.saturating_add(divisor - 1) / divisor;
    // Some Intel parts document an erratum for a programmed value of one.
    let timer_ticks = timer_ticks.clamp(2, u32::MAX as u128) as u32;
    (timer_ticks, rate_shift)
}
