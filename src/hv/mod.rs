pub mod app_crash;
pub mod blueprint;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod blueprint_net;
pub mod control_kick;
pub mod guest_run;
pub mod guest_work;
pub mod hv_remote_restore_service;
pub mod lane;
pub mod memory;
pub mod security;
pub mod snapshot;
pub mod store;
pub mod sync;
pub mod vmcall;
pub mod vmx;
pub mod vnet;

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

use crate::shell2::MatrixTarget;

use guest_work::{VmLaneProfile, pick_vm_hull_lane};
use memory::*;
use snapshot::*;
const MAIN_LOOP_MARKER: &[u8] = b"main: entering executor loop";
const VMX_PAGE_SIZE: usize = 4096;
const MIB: usize = 1024 * 1024;
const HV_LOG_LINE: usize = crate::allcaps::hv::LOG_LINE_BYTES;
pub const TRUEOS_VM_ID_LIMIT: usize = crate::allcaps::hv::VM_ID_LIMIT;
const TRUEOS_VM_CPU_SLOT_LIMIT: usize = crate::allcaps::hv::VM_CPU_SLOT_LIMIT;
const GUEST_FS_BASE_RESET: u64 = 0;
const BLUEPRINT_PREPARE_PAUSE_TIMEOUT_MS: u64 = 15_000;
const BLUEPRINT_LIFECYCLE_PHASE_RUNNING: u8 = 0;
const BLUEPRINT_LIFECYCLE_PHASE_PREPARE_PAUSE: u8 = 1;
const BLUEPRINT_LIFECYCLE_PHASE_READY: u8 = 2;
const BLUEPRINT_LIFECYCLE_PHASE_ARMING: u8 = 3;
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

/// A raw snapshot still contains source-principal guest-heap pointers and page
/// mappings. Keep cross-slot/cross-host restore unavailable until the v-layer
/// can relocate every guest-writable backing.
pub const fn cross_principal_snapshot_restore_supported() -> bool {
    false
}

struct TrueosVmId {
    running: AtomicBool,
    starting: AtomicBool,
    stop_req: AtomicBool,
    preserve_req: AtomicBool,
    preserve_exit: AtomicBool,
    clean_exit: AtomicBool,
    replicatable: AtomicBool,
    pause_latched: AtomicBool,
    pause_store_seq: AtomicU64,
    lifecycle_phase: AtomicU8,
    lifecycle_operation: AtomicU64,
    lifecycle_deadline_ms: AtomicU64,
    lifecycle_reason: AtomicU8,
    lifecycle_checkpoint_version: AtomicU64,
    run_generation: AtomicU64,
    restore_inflight: AtomicBool,
    marker_seen: AtomicBool,
}

impl TrueosVmId {
    const fn new() -> Self {
        Self {
            running: AtomicBool::new(false),
            starting: AtomicBool::new(false),
            stop_req: AtomicBool::new(false),
            preserve_req: AtomicBool::new(false),
            preserve_exit: AtomicBool::new(false),
            clean_exit: AtomicBool::new(false),
            replicatable: AtomicBool::new(false),
            pause_latched: AtomicBool::new(false),
            pause_store_seq: AtomicU64::new(0),
            lifecycle_phase: AtomicU8::new(BLUEPRINT_LIFECYCLE_PHASE_RUNNING),
            lifecycle_operation: AtomicU64::new(0),
            lifecycle_deadline_ms: AtomicU64::new(0),
            lifecycle_reason: AtomicU8::new(BlueprintPauseReason::Pause as u8),
            lifecycle_checkpoint_version: AtomicU64::new(0),
            run_generation: AtomicU64::new(0),
            restore_inflight: AtomicBool::new(false),
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
        "hv: vmx core-contract summary slots={}..{} active={}/{} revision=0x{:08X} vmxon_pa_span=0x{:016X}-0x{:016X}",
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
    if !vmx::vmxoff() {
        return Err("vmxoff");
    }
    VMX_EXTERNAL_INTERRUPT_EXITING_BY_CPU[slot].store(false, Ordering::Release);
    VMXON_PA_BY_CPU[slot].store(0, Ordering::Release);
    VMX_ROOT_ACTIVE_BY_CPU[slot].store(false, Ordering::Release);
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
    ConsoleBusy,
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
    Releasing {
        epoch: u64,
    },
    Parked {
        ticket: u64,
    },
    ReentryRequested {
        ticket: u64,
        epoch: u64,
    },
    Claiming {
        ticket: u64,
        epoch: u64,
        direct: bool,
    },
}

impl BlueprintTerminalLeaseState {
    const fn for_surface(surface: BlueprintConsoleSurface) -> Self {
        match surface {
            BlueprintConsoleSurface::Text => Self::Unsupported,
            BlueprintConsoleSurface::Terminal => Self::Reserved,
        }
    }

    /// A parked/reentry transition deliberately leaves Shell2 interactive.
    /// Guest terminal paint must therefore never be routed into its Matrix
    /// surface until the lease becomes Active again.
    const fn suppresses_terminal_output(self) -> bool {
        matches!(
            self,
            Self::Reserved
                | Self::Releasing { .. }
                | Self::Parked { .. }
                | Self::ReentryRequested { .. }
                | Self::Claiming { .. }
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

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum BlueprintTuiDemoEscape {
    None,
    Escape,
    Csi,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum BlueprintTuiDemoStatus {
    Ready,
    Inspected,
    Reset,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct BlueprintTuiDemo {
    selected: u8,
    escape: BlueprintTuiDemoEscape,
    escape_idle_ticks: u8,
    status: BlueprintTuiDemoStatus,
}

impl BlueprintTuiDemo {
    const fn new() -> Self {
        Self {
            selected: 0,
            escape: BlueprintTuiDemoEscape::None,
            escape_idle_ticks: 0,
            status: BlueprintTuiDemoStatus::Ready,
        }
    }
}

impl BlueprintConsoleRoute {
    const fn is_net_shell_direct(self) -> bool {
        matches!(self, Self::NetShellDirect)
    }
}

fn blueprint_uses_net_shell_direct_path(
    console_surface: BlueprintConsoleSurface,
    console_target: Option<&MatrixTarget>,
) -> bool {
    console_surface.is_terminal()
        && console_target.is_some_and(|target| {
            crate::shell2::matrix_target_routes_to(target, crate::shell2::OUTPUT_NET_TCP_MASK)
        })
}

fn blueprint_uses_local_terminal_handoff(
    console_surface: BlueprintConsoleSurface,
    console_target: Option<&MatrixTarget>,
) -> bool {
    console_surface.is_terminal()
        && console_target.is_some_and(crate::shell2::matrix_target_supports_terminal_handoff)
}

#[derive(Clone)]
pub(crate) struct BlueprintProcessContext {
    args: AllocVec<AllocString>,
    vars: BTreeMap<AllocString, AllocString>,
    console_target: Option<MatrixTarget>,
    console_surface: BlueprintConsoleSurface,
    console_route: BlueprintConsoleRoute,
    console_attached: bool,
    console_attach_generation: u64,
    console_attach_inflight: bool,
    app_command_passthrough: bool,
    terminal_lease: BlueprintTerminalLeaseState,
    terminal_surface_generation: u64,
    console_input: VecDeque<u8>,
    control_shell_line: AllocVec<u8>,
    tui_demo: Option<BlueprintTuiDemo>,
    exit_reason: Option<AllocString>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct BlueprintTerminalCleanup {
    context_present: bool,
    backend_release_expected: bool,
    backend_released: bool,
    matrix_unbind_expected: bool,
    matrix_unbind_result: Option<crate::shell2::MatrixVmUnbindResult>,
}

impl BlueprintTerminalCleanup {
    const fn empty() -> Self {
        Self {
            context_present: false,
            backend_release_expected: false,
            backend_released: true,
            matrix_unbind_expected: false,
            matrix_unbind_result: None,
        }
    }

    const fn complete(self) -> bool {
        (!self.backend_release_expected || self.backend_released)
            && (!self.matrix_unbind_expected
                || matches!(
                    self.matrix_unbind_result,
                    Some(result) if result.owner_absent()
                ))
    }

    const fn matrix_unbind_marker(self) -> &'static str {
        match self.matrix_unbind_result {
            Some(result) => result.marker(),
            None => "not-needed",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum StopError {
    UnsupportedVmId,
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
    Pause,
}

/// Why the host is asking a Blueprint to enter its quiescent checkpoint boundary.
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BlueprintPauseReason {
    Pause = 1,
    Replicate = 2,
    Migrate = 3,
}

/// Host action to take after a Blueprint reaches its exact Ready boundary.
///
/// A pause retains the quiesced VM directly in memory. Snapshot-capable
/// lifecycle reasons additionally serialize that retained state into the
/// warm per-VM store. `apps store` is the explicit reboot-persistent commit.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum BlueprintReadyDisposition {
    Pause,
    Snapshot,
}

impl BlueprintPauseReason {
    const fn from_raw(raw: u8) -> Self {
        match raw {
            2 => Self::Replicate,
            3 => Self::Migrate,
            _ => Self::Pause,
        }
    }
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

#[derive(Copy, Clone, Debug)]
pub struct HvStatus {
    pub vendor_intel: bool,
    pub has_msr: bool,
    pub has_vmx: bool,
    pub feature_control_locked: bool,
    pub feature_control_vmx_outside_smx: bool,
    pub guest_module_present: bool,
    pub stored_vm_count: usize,
    pub vm_id_limit: usize,
    pub running_count: usize,
    pub starting_count: usize,
    pub active_vm_ids: [Option<u8>; TRUEOS_VM_ID_LIMIT],
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
    pub preserve_requested: bool,
    pub preserve_exit: bool,
    pub replicatable: bool,
    pub pause_latched: bool,
    pub pause_snapshot_ready: bool,
    pub prepare_pause_pending: bool,
    pub lifecycle_ready: bool,
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

fn reset_prepare_pause(vm: &TrueosVmId) {
    vm.lifecycle_phase
        .store(BLUEPRINT_LIFECYCLE_PHASE_RUNNING, Ordering::Release);
    vm.lifecycle_operation.store(0, Ordering::Release);
    vm.lifecycle_deadline_ms.store(0, Ordering::Release);
    vm.lifecycle_checkpoint_version.store(0, Ordering::Release);
}

fn expire_prepare_pause(vm_id: u8, vm: &TrueosVmId) {
    if vm.lifecycle_phase.load(Ordering::Acquire) != BLUEPRINT_LIFECYCLE_PHASE_PREPARE_PAUSE {
        return;
    }
    let deadline = vm.lifecycle_deadline_ms.load(Ordering::Acquire);
    if deadline == 0 || lifecycle_now_ms() <= deadline {
        return;
    }
    if vm
        .lifecycle_phase
        .compare_exchange(
            BLUEPRINT_LIFECYCLE_PHASE_PREPARE_PAUSE,
            BLUEPRINT_LIFECYCLE_PHASE_RUNNING,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_ok()
    {
        let operation = vm.lifecycle_operation.swap(0, Ordering::AcqRel);
        vm.lifecycle_deadline_ms.store(0, Ordering::Release);
        hvwarnf(format_args!(
            "hv: vm{} lifecycle: PreparePause operation={} timed out; VM left running",
            vm_id, operation
        ));
    }
}

pub(crate) fn blueprint_prepare_pause(vm_id: u8) -> Option<BlueprintPreparePause> {
    let vm = vm_slot(vm_id)?;
    expire_prepare_pause(vm_id, vm);
    if vm.lifecycle_phase.load(Ordering::Acquire) != BLUEPRINT_LIFECYCLE_PHASE_PREPARE_PAUSE {
        return None;
    }
    Some(BlueprintPreparePause {
        operation: vm.lifecycle_operation.load(Ordering::Acquire),
        deadline_ms: vm.lifecycle_deadline_ms.load(Ordering::Acquire),
        reason: BlueprintPauseReason::from_raw(vm.lifecycle_reason.load(Ordering::Acquire)),
    })
}

pub(crate) fn acknowledge_blueprint_ready(
    vm_id: u8,
    operation: u64,
    checkpoint_version: u64,
) -> Option<BlueprintReadyDisposition> {
    let Some(vm) = vm_slot(vm_id) else {
        return None;
    };
    expire_prepare_pause(vm_id, vm);
    if operation == 0
        || vm.lifecycle_operation.load(Ordering::Acquire) != operation
        || vm
            .lifecycle_phase
            .compare_exchange(
                BLUEPRINT_LIFECYCLE_PHASE_PREPARE_PAUSE,
                BLUEPRINT_LIFECYCLE_PHASE_READY,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_err()
    {
        return None;
    }
    let reason = BlueprintPauseReason::from_raw(vm.lifecycle_reason.load(Ordering::Acquire));
    let disposition = match reason {
        BlueprintPauseReason::Pause => BlueprintReadyDisposition::Pause,
        BlueprintPauseReason::Replicate | BlueprintPauseReason::Migrate => {
            BlueprintReadyDisposition::Snapshot
        }
    };
    vm.lifecycle_checkpoint_version
        .store(checkpoint_version, Ordering::Release);
    match prepare_preserve_mode(vm_id, PreserveMode::Pause) {
        Ok(true) => {
            hvlogf(format_args!(
                "hv: vm{} lifecycle: Ready operation={} checkpoint_version={} reason={:?} disposition={:?}",
                vm_id, operation, checkpoint_version, reason, disposition
            ));
            Some(disposition)
        }
        Ok(false) | Err(_) => {
            reset_prepare_pause(vm);
            None
        }
    }
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
    };
    let Some(link_slot) = BLUEPRINT_CHILD_LINKS.get(child_vm_id as usize) else {
        return Err(BLUEPRINT_CHILD_ERR_UNAVAILABLE);
    };
    *link_slot.lock() = Some(link);

    let result = start_blueprint_child_vm(
        child_vm_id,
        template.archive,
        template.module_bytes,
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
        if vm
            .starting
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
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

fn vm_activity_snapshot() -> (usize, usize, [Option<u8>; TRUEOS_VM_ID_LIMIT]) {
    let mut active_vm_ids = [None; TRUEOS_VM_ID_LIMIT];
    let mut running_count = 0usize;
    let mut starting_count = 0usize;

    for (idx, slot) in trueos_vm_ids.iter().enumerate() {
        let running = slot.running.load(Ordering::Acquire);
        let starting = slot.starting.load(Ordering::Acquire);
        if running || starting {
            active_vm_ids[idx] = Some(idx as u8);
        }
        if running {
            running_count = running_count.saturating_add(1);
        }
        if starting {
            starting_count = starting_count.saturating_add(1);
        }
    }

    (running_count, starting_count, active_vm_ids)
}

pub fn vm_state(vm_id: u8) -> HvVmState {
    let Some(vm) = vm_slot(vm_id) else {
        return HvVmState {
            id: vm_id,
            supported: false,
            running: false,
            starting: false,
            stop_requested: false,
            preserve_requested: false,
            preserve_exit: false,
            replicatable: false,
            pause_latched: false,
            pause_snapshot_ready: false,
            prepare_pause_pending: false,
            lifecycle_ready: false,
            restore_inflight: false,
        };
    };
    expire_prepare_pause(vm_id, vm);
    let pause_latched = vm.pause_latched.load(Ordering::Acquire);
    let pause_snapshot_ready = pause_latched
        && crate::hv::store::current_committed_seq(vm_id)
            > vm.pause_store_seq.load(Ordering::Acquire);
    HvVmState {
        id: vm_id,
        supported: true,
        running: vm.running.load(Ordering::Acquire),
        starting: vm.starting.load(Ordering::Acquire),
        stop_requested: vm.stop_req.load(Ordering::Acquire),
        preserve_requested: vm.preserve_req.load(Ordering::Acquire),
        preserve_exit: vm.preserve_exit.load(Ordering::Acquire),
        replicatable: vm.replicatable.load(Ordering::Acquire),
        pause_latched,
        pause_snapshot_ready,
        prepare_pause_pending: vm.lifecycle_phase.load(Ordering::Acquire)
            == BLUEPRINT_LIFECYCLE_PHASE_PREPARE_PAUSE,
        lifecycle_ready: vm.lifecycle_phase.load(Ordering::Acquire)
            == BLUEPRINT_LIFECYCLE_PHASE_READY,
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

pub fn default_app_instance_vm(archive: &str) -> Option<u8> {
    for vm_id in 0..TRUEOS_VM_ID_LIMIT {
        let vm_id = vm_id as u8;
        let vm = vm_slot(vm_id)?;
        if !vm.running.load(Ordering::Acquire)
            && !vm.starting.load(Ordering::Acquire)
            && !vm.pause_latched.load(Ordering::Acquire)
        {
            continue;
        }
        let running_default = BLUEPRINT_LAUNCH_STATES
            .get(vm_id as usize)
            .is_some_and(|slot| {
                let state = slot.lock();
                state.as_ref().is_some_and(|state| {
                    state.archive.eq_ignore_ascii_case(archive)
                        && state.identity.name.is_none()
                        && state.identity.peer.is_none()
                })
            });
        let pending_default = BLUEPRINT_PENDING_LAUNCH_STATES
            .get(vm_id as usize)
            .is_some_and(|slot| {
                let pending = slot.lock();
                pending.as_ref().is_some_and(|pending| {
                    pending.archive.eq_ignore_ascii_case(archive) && pending.instance.is_default()
                })
            });
        if running_default || pending_default {
            return Some(vm_id);
        }
    }
    None
}

/// Live named instances of `archive`, as `(vm_id, label)` pairs. The default
/// instance is excluded: it is the one this archive can only hold once, so the
/// caller is listing the alternatives that already exist beside it.
pub fn named_app_instance_vms(archive: &str) -> AllocVec<(u8, AllocString)> {
    let mut out = AllocVec::new();
    for vm_id in 0..TRUEOS_VM_ID_LIMIT {
        let vm_id = vm_id as u8;
        let Some(vm) = vm_slot(vm_id) else {
            continue;
        };
        if !vm.running.load(Ordering::Acquire)
            && !vm.starting.load(Ordering::Acquire)
            && !vm.pause_latched.load(Ordering::Acquire)
        {
            continue;
        }
        let running_name = BLUEPRINT_LAUNCH_STATES
            .get(vm_id as usize)
            .and_then(|slot| {
                let state = slot.lock();
                state.as_ref().and_then(|state| {
                    state
                        .archive
                        .eq_ignore_ascii_case(archive)
                        .then(|| {
                            named_instance_label(
                                state.identity.name.as_deref(),
                                state.identity.peer.as_deref(),
                            )
                        })
                        .flatten()
                })
            });
        let label = running_name.or_else(|| {
            BLUEPRINT_PENDING_LAUNCH_STATES
                .get(vm_id as usize)
                .and_then(|slot| {
                    let pending = slot.lock();
                    pending.as_ref().and_then(|pending| {
                        pending
                            .archive
                            .eq_ignore_ascii_case(archive)
                            .then(|| {
                                named_instance_label(
                                    pending.instance.name.as_deref(),
                                    pending.instance.peer.as_deref(),
                                )
                            })
                            .flatten()
                    })
                })
        });
        if let Some(label) = label {
            out.push((vm_id, label));
        }
    }
    out
}

fn named_instance_label(name: Option<&str>, peer: Option<&str>) -> Option<AllocString> {
    match (peer, name) {
        (Some(peer), Some(name)) => Some(alloc::format!("peer:{} / {}", peer, name)),
        (Some(peer), None) => Some(alloc::format!("peer:{} / unnamed", peer)),
        (None, Some(name)) => Some(AllocString::from(name)),
        (None, None) => None,
    }
}

fn set_blueprint_lifecycle_capability(vm_id: u8, archive: &str, replicatable: bool) {
    let Some(vm) = vm_slot(vm_id) else {
        return;
    };
    vm.replicatable.store(replicatable, Ordering::Release);
    vm.pause_latched.store(false, Ordering::Release);
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
    let (running_count, starting_count, active_vm_ids) = vm_activity_snapshot();
    let vm_heap = crate::allocators::hv_guest_heap_stats_total();
    HvStatus {
        vendor_intel,
        has_msr,
        has_vmx,
        feature_control_locked: fc_locked,
        feature_control_vmx_outside_smx: fc_vmx_outside_smx,
        guest_module_present: crate::limine::guest_kernel_bytes().is_some(),
        stored_vm_count: crate::hv::store::committed_vm_count(),
        vm_id_limit: TRUEOS_VM_ID_LIMIT,
        running_count,
        starting_count,
        active_vm_ids,
        vm_shared_heap_total_bytes: vm_heap.usable_total,
        vm_shared_heap_free_bytes: vm_heap.free_bytes,
        vm_shared_stack_bytes: memory::active_guest_stack_bytes_total(),
        vm_shared_vmx_bytes: core::mem::size_of::<VmxPage>() * 2 * TRUEOS_VM_CPU_SLOT_LIMIT,
    }
}

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
        }),
        false,
    )
}

fn start_blueprint_child_vm(
    vm_id: u8,
    archive: AllocString,
    module_bytes: AllocVec<u8>,
    instance: BlueprintInstanceRequest,
) -> Result<(), StartError> {
    start_with_mode(
        vm_id,
        VmBootMode::Hull,
        None,
        Some(BlueprintPendingLaunchState {
            archive,
            module_bytes,
            app_args: alloc::vec![AllocString::from(BLUEPRINT_CHILD_WORKER_ARG)],
            launch_script: None,
            instance,
            console_target: None,
            console_surface: BlueprintConsoleSurface::Text,
        }),
        true,
    )
}

fn start_with_mode(
    vm_id: u8,
    boot_mode: VmBootMode,
    stack_mb: Option<usize>,
    pending_blueprint: Option<BlueprintPendingLaunchState>,
    already_reserved: bool,
) -> Result<(), StartError> {
    let Some(vm) = vm_slot(vm_id) else {
        return Err(StartError::UnsupportedVmId);
    };

    if vm.running.load(Ordering::Acquire) {
        return Err(StartError::AlreadyRunning);
    }
    if !crate::gpu::vgpu::hull_guest_storage_reusable(vm_id) {
        hvwarnf(format_args!(
            "hv: vm{} lifecycle: start rejected reason=vgpu-storage-quarantined action=retain-vm-slot-and-guest-pages-until-reset",
            vm_id
        ));
        return Err(StartError::VgpuQuarantined);
    }

    if already_reserved {
        if !vm.starting.load(Ordering::Acquire) {
            return Err(StartError::AlreadyRunning);
        }
    } else if vm
        .starting
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err(StartError::AlreadyRunning);
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
    if is_blueprint_start && vm.pause_latched.load(Ordering::Acquire) {
        hvwarnf(format_args!(
            "hv: vm{} lifecycle: fresh blueprint start rejected while pause is retained",
            vm_id
        ));
        vm.starting.store(false, Ordering::Release);
        return Err(StartError::AlreadyRunning);
    }
    if is_blueprint_start {
        memory::clear_restore_meta_for_vm(vm_id);
        hvlogf(format_args!(
            "hv: vm{} lifecycle: fresh blueprint start disarmed restore metadata",
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

    vm.stop_req.store(false, Ordering::Release);
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
                mark_replicatable_resumed(vm_id);
                hvlogf(format_args!("hv: vm{} lifecycle: resume committed before VM wake", vm_id));
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
    Ok(())
}

pub fn stop(vm_id: u8) -> Result<bool, StopError> {
    let Some(vm) = vm_slot(vm_id) else {
        return Err(StopError::UnsupportedVmId);
    };

    if vm.running.load(Ordering::Acquire) || vm.starting.load(Ordering::Acquire) {
        clear_blueprint_lifecycle_capability(vm_id);
        vm.stop_req.store(true, Ordering::Release);
        hvlogf(format_args!("hv: vm{} lifecycle: stop requested", vm_id));
        nudge_vm_control(vm_id, crate::hv::control_kick::LifecycleKickAction::Stop, "stop");
        Ok(true)
    } else {
        hvwarnf(format_args!("hv: vm{} lifecycle: stop ignored (not running)", vm_id));
        Ok(false)
    }
}

/// Destroy an offline retained VM and its warm checkpoint. Named persistent
/// images are independent and remain on TRUEOSFS.
pub fn eject(vm_id: u8) -> Result<bool, EjectError> {
    let Some(vm) = vm_slot(vm_id) else {
        return Err(EjectError::UnsupportedVmId);
    };
    if vm.running.load(Ordering::Acquire)
        || vm.starting.load(Ordering::Acquire)
        || vm.restore_inflight.load(Ordering::Acquire)
    {
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
    let had_state = vm.pause_latched.load(Ordering::Acquire)
        || blueprint_launch_active(vm_id)
        || crate::hv::store::has_committed_vm(vm_id);
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
    let _ = crate::ai::lumen_service::close(vm_id);
    let _ = crate::r::gridpaper_service::release_owner_lifecycle(vm_id);
    let _ = crate::r::media_service::release_vm(vm_id);
    let _ = crate::ui4::release_owner_resources(crate::ui4::WindowOwner::Vm(vm_id));
    memory::clear_snapshot_state_for_vm(vm_id);
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
    let _ = crate::hv::store::eject_warm(vm_id);
    vm.pause_latched.store(false, Ordering::Release);
    vm.pause_store_seq.store(0, Ordering::Release);
    reset_prepare_pause(vm);
    Ok(had_state)
}

pub fn kick(vm_id: u8) -> Result<bool, StopError> {
    let Some(vm) = vm_slot(vm_id) else {
        return Err(StopError::UnsupportedVmId);
    };
    if !vm.running.load(Ordering::Acquire) {
        return Ok(false);
    }
    Ok(nudge_vm_control(vm_id, crate::hv::control_kick::LifecycleKickAction::Nudge, "manual"))
}

pub fn request_replicatable_pause(vm_id: u8) -> Result<bool, StopError> {
    request_blueprint_prepare_pause(vm_id, BlueprintPauseReason::Pause)
}

pub fn request_replicatable_snapshot(vm_id: u8) -> Result<bool, StopError> {
    request_blueprint_prepare_pause(vm_id, BlueprintPauseReason::Replicate)
}

pub fn request_blueprint_prepare_pause(
    vm_id: u8,
    reason: BlueprintPauseReason,
) -> Result<bool, StopError> {
    let Some(vm) = vm_slot(vm_id) else {
        return Err(StopError::UnsupportedVmId);
    };
    let running = vm.running.load(Ordering::Acquire);
    let starting = vm.starting.load(Ordering::Acquire);
    if !running && !starting {
        return Ok(false);
    }
    if !vm.replicatable.load(Ordering::Acquire) {
        return Ok(false);
    }
    expire_prepare_pause(vm_id, vm);
    if vm
        .lifecycle_phase
        .compare_exchange(
            BLUEPRINT_LIFECYCLE_PHASE_RUNNING,
            BLUEPRINT_LIFECYCLE_PHASE_ARMING,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_err()
    {
        return Ok(false);
    }

    let operation = BLUEPRINT_LIFECYCLE_OPERATION_SEQ
        .fetch_add(1, Ordering::AcqRel)
        .max(1);
    let deadline = lifecycle_now_ms().saturating_add(BLUEPRINT_PREPARE_PAUSE_TIMEOUT_MS);
    vm.lifecycle_operation.store(operation, Ordering::Release);
    vm.lifecycle_deadline_ms.store(deadline, Ordering::Release);
    vm.lifecycle_reason.store(reason as u8, Ordering::Release);
    vm.lifecycle_checkpoint_version.store(0, Ordering::Release);
    vm.lifecycle_phase
        .store(BLUEPRINT_LIFECYCLE_PHASE_PREPARE_PAUSE, Ordering::Release);
    hvlogf(format_args!(
        "hv: vm{} lifecycle: PreparePause operation={} reason={:?} deadline_ms={}",
        vm_id, operation, reason, deadline
    ));
    nudge_vm_control(vm_id, crate::hv::control_kick::LifecycleKickAction::Nudge, "prepare-pause");
    Ok(true)
}

pub fn request_preserve(vm_id: u8) -> Result<bool, StopError> {
    request_preserve_mode(vm_id, PreserveMode::Stop)
}

pub fn request_preserve_mode(vm_id: u8, mode: PreserveMode) -> Result<bool, StopError> {
    let Some(vm) = vm_slot(vm_id) else {
        return Err(StopError::UnsupportedVmId);
    };

    let running = vm.running.load(Ordering::Acquire);
    let starting = vm.starting.load(Ordering::Acquire);
    if !running && !starting {
        hvwarnf(format_args!(
            "hv: vm{} lifecycle: preserve ignored mode={:?} (not running)",
            vm_id, mode
        ));
        return Ok(false);
    }

    if !prepare_preserve_mode(vm_id, mode)? {
        return Ok(false);
    }

    vm.preserve_req.store(true, Ordering::Release);
    hvlogf(format_args!("hv: vm{} lifecycle: preserve requested mode={:?}", vm_id, mode));
    let action = match mode {
        PreserveMode::Stop => crate::hv::control_kick::LifecycleKickAction::PreserveStop,
        PreserveMode::Pause => crate::hv::control_kick::LifecycleKickAction::PreservePause,
    };
    nudge_vm_control(vm_id, action, "preserve");
    Ok(true)
}

/// Apply the lifecycle half of a preserve request.
///
/// Host requests call this before arming `preserve_req`; guest VMCALL requests
/// call it while the VM is already stopped at a safe VM-exit boundary. Keeping
/// both entry paths here prevents pause-mode resource retention from drifting.
pub(crate) fn prepare_preserve_mode(vm_id: u8, mode: PreserveMode) -> Result<bool, StopError> {
    let Some(vm) = vm_slot(vm_id) else {
        return Err(StopError::UnsupportedVmId);
    };

    if mode == PreserveMode::Pause {
        if !vm.replicatable.load(Ordering::Acquire) {
            return Ok(false);
        }
        vm.pause_store_seq
            .store(crate::hv::store::current_committed_seq(vm_id), Ordering::Release);
        vm.pause_latched.store(true, Ordering::Release);
        suspend_blueprint_process_context(vm_id);
        crate::r::gridpaper_service::pause_owner_lifecycle(vm_id);
    }

    Ok(true)
}

pub fn mark_replicatable_resumed(vm_id: u8) {
    if let Some(vm) = vm_slot(vm_id) {
        vm.pause_latched.store(false, Ordering::Release);
        reset_prepare_pause(vm);
        if let Some(identity_slot) = BLUEPRINT_INSTANCE_IDENTITIES.get(vm_id as usize) {
            let mut identity = identity_slot.lock();
            if let Some(identity) = identity.as_mut() {
                identity.generation = identity.generation.saturating_add(1);
                if let Some(context_slot) = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize)
                    && let Some(context) = context_slot.lock().as_mut()
                {
                    context.vars.insert(
                        AllocString::from("TRUEOS_APP_GENERATION"),
                        alloc::format!("{}", identity.generation),
                    );
                }
            }
        }
        resume_blueprint_process_context(vm_id);
        crate::r::gridpaper_service::resume_owner_lifecycle(vm_id);
    }
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub fn save_snapshot(vm_id: u8) -> Result<usize, SaveError> {
    if vm_slot(vm_id).is_none() {
        return Err(SaveError::UnsupportedVmId);
    }

    let bytes = snapshot_bytes(vm_id)?;
    crate::hv::store::save_bytes(vm_id, bytes).map_err(map_store_save_error)
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub fn restore_snapshot(vm_id: u8) -> Result<usize, RestoreError> {
    if vm_slot(vm_id).is_none() {
        return Err(RestoreError::UnsupportedVmId);
    }
    if !crate::gpu::vgpu::hull_guest_storage_reusable(vm_id) {
        return Err(RestoreError::VgpuQuarantined);
    }

    let bytes = crate::hv::store::load_bytes(vm_id).map_err(map_store_restore_error)?;

    restore_snapshot_bytes(vm_id, bytes.as_slice())?;
    Ok(bytes.len())
}

pub async fn restore_snapshot_async(vm_id: u8) -> Result<usize, RestoreError> {
    if vm_slot(vm_id).is_none() {
        return Err(RestoreError::UnsupportedVmId);
    }
    if !crate::gpu::vgpu::hull_guest_storage_reusable(vm_id) {
        return Err(RestoreError::VgpuQuarantined);
    }

    let bytes = crate::hv::store::load_bytes_async(vm_id)
        .await
        .map_err(map_store_restore_error)?;

    restore_snapshot_bytes(vm_id, bytes.as_slice())?;
    Ok(bytes.len())
}

pub fn restore_persistent_image(
    vm_id: u8,
    image: &crate::hv::store::PersistentVmImage,
    console_target: Option<MatrixTarget>,
) -> Result<usize, RestoreError> {
    if vm_slot(vm_id).is_none() {
        return Err(RestoreError::UnsupportedVmId);
    }
    if !crate::gpu::vgpu::hull_guest_storage_reusable(vm_id) {
        return Err(RestoreError::VgpuQuarantined);
    }
    let result = (|| {
        crate::allocators::restore_hv_guest_heap(vm_id, image.guest_heap.as_slice())
            .map_err(|_| RestoreError::BadSnapshot)?;
        memory::restore_guest_hull_rw_for_vm(vm_id, image.hull_rw.as_slice())
            .map_err(|_| RestoreError::BadSnapshot)?;
        restore_blueprint_portable_state(vm_id, image.blueprint.as_slice(), console_target)
            .map_err(|_| RestoreError::BadSnapshot)?;
        restore_snapshot_bytes(vm_id, image.snapshot.as_slice())?;
        memory::rebind_restored_guest_memory_for_vm(vm_id, VmBootMode::Hull)
            .map_err(|_| RestoreError::BadSnapshot)?;
        Ok(image.snapshot.len())
    })();
    if result.is_err() {
        if let Some(vm) = vm_slot(vm_id) {
            vm.restore_inflight.store(false, Ordering::Release);
        }
        let _ = eject(vm_id);
    }
    result
}

pub fn try_begin_restore(vm_id: u8) -> Result<bool, StopError> {
    let Some(vm) = vm_slot(vm_id) else {
        return Err(StopError::UnsupportedVmId);
    };
    if !crate::gpu::vgpu::hull_guest_storage_reusable(vm_id) {
        return Err(StopError::VgpuQuarantined);
    }
    Ok(vm
        .restore_inflight
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok())
}

pub fn finish_restore(vm_id: u8) {
    if let Some(vm) = vm_slot(vm_id) {
        vm.restore_inflight.store(false, Ordering::Release);
    }
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
fn map_store_save_error(err: crate::hv::store::VmStoreError) -> SaveError {
    match err {
        crate::hv::store::VmStoreError::ServiceOffline => {
            SaveError::Io(crate::disc::block::Error::NotReady)
        }
        crate::hv::store::VmStoreError::QueueFull => {
            SaveError::Io(crate::disc::block::Error::NotReady)
        }
        crate::hv::store::VmStoreError::Create(e)
        | crate::hv::store::VmStoreError::Format(e)
        | crate::hv::store::VmStoreError::BeginWrite(e)
        | crate::hv::store::VmStoreError::Write(e)
        | crate::hv::store::VmStoreError::Read(e) => SaveError::Io(e),
        crate::hv::store::VmStoreError::MissingSnapshot => SaveError::BeginWrite,
        crate::hv::store::VmStoreError::NoPersistentRoot => {
            SaveError::Io(crate::disc::block::Error::NotReady)
        }
        crate::hv::store::VmStoreError::InvalidName
        | crate::hv::store::VmStoreError::BadEnvelope => {
            SaveError::Io(crate::disc::block::Error::InvalidParam)
        }
    }
}

fn map_store_restore_error(err: crate::hv::store::VmStoreError) -> RestoreError {
    match err {
        crate::hv::store::VmStoreError::MissingSnapshot => RestoreError::MissingFile,
        crate::hv::store::VmStoreError::ServiceOffline => {
            RestoreError::Read(crate::disc::block::Error::NotReady)
        }
        crate::hv::store::VmStoreError::QueueFull => {
            RestoreError::Read(crate::disc::block::Error::NotReady)
        }
        crate::hv::store::VmStoreError::Create(e)
        | crate::hv::store::VmStoreError::Format(e)
        | crate::hv::store::VmStoreError::BeginWrite(e)
        | crate::hv::store::VmStoreError::Read(e)
        | crate::hv::store::VmStoreError::Write(e) => RestoreError::Read(e),
        crate::hv::store::VmStoreError::NoPersistentRoot => {
            RestoreError::Read(crate::disc::block::Error::NotReady)
        }
        crate::hv::store::VmStoreError::InvalidName
        | crate::hv::store::VmStoreError::BadEnvelope => RestoreError::BadSnapshot,
    }
}

fn vmexit_is_preserve(vm_id: u8, lr: LaunchResult) -> bool {
    lr.entered != 0
        && lr.launch_failed == 0
        && vm_slot(vm_id)
            .map(|vm| vm.preserve_exit.load(Ordering::Acquire))
            .unwrap_or(false)
}

fn snapshot_on_preserve_exit(vm_id: u8) {
    let saved = match snapshot_bytes(vm_id) {
        Ok(bytes) => match crate::hv::store::save_bytes(vm_id, bytes) {
            Ok(saved) => {
                hvlogf(format_args!(
                    "hv: vm{} reporting: preserve snapshot saved store=warm-arc path={} bytes={}",
                    vm_id,
                    snapshot_path(vm_id).as_str(),
                    saved
                ));
                true
            }
            Err(e) => {
                hvwarnf(format_args!(
                    "hv: vm{} reporting: preserve snapshot save failed ({:?})",
                    vm_id, e
                ));
                false
            }
        },
        Err(e) => {
            hvwarnf(format_args!(
                "hv: vm{} reporting: preserve snapshot bytes failed ({:?})",
                vm_id, e
            ));
            false
        }
    };
    if !saved {
        if let Some(vm) = vm_slot(vm_id) {
            if vm.pause_latched.swap(false, Ordering::AcqRel) {
                vm.pause_store_seq.store(0, Ordering::Release);
                reset_prepare_pause(vm);
                hvwarnf(format_args!(
                    "hv: vm{} lifecycle: pause latch released after snapshot failure",
                    vm_id
                ));
            }
        }
    }
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub fn request_preserve_active_vm() -> bool {
    if let Some(vm_id) = current_vm_id() {
        return request_preserve(vm_id).unwrap_or(false);
    }
    trueos_vm_ids
        .iter()
        .enumerate()
        .find(|(_, vm)| vm.running.load(Ordering::Acquire) || vm.starting.load(Ordering::Acquire))
        .map(|(vm_id, _)| request_preserve(vm_id as u8).unwrap_or(false))
        .unwrap_or(false)
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

    let heavy_graphics_signal = archive_has(archive, "mandelbrot")
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

    let (heap_lower, heap_recommended, heap_upper, stack_lower, stack_recommended, stack_upper) =
        match class {
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
                128,
                round_pow2_mib(base_live_mib.saturating_mul(16).saturating_add(128)).max(256),
                512,
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
    crate::log_os::blueprint_line(
        log_os_core::LogLevel::Info,
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

    let identity = assign_fresh_blueprint_identity(vm_id, &pending.instance)
        .ok_or_else(|| AllocString::from("app-vm instance identity unavailable"))?;
    let instance_guid = format_blueprint_uuid(&identity.instance);
    let app_fs_root = if pending.instance.is_default() {
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
    let direct_terminal_handoff =
        blueprint_uses_net_shell_direct_path(console_surface, console_target.as_ref());
    let local_terminal_handoff = !direct_terminal_handoff
        && blueprint_uses_local_terminal_handoff(console_surface, console_target.as_ref());
    if console_surface.is_terminal() && !direct_terminal_handoff && !local_terminal_handoff {
        hvwarnf(format_args!(
            "hv: vm{} console route: terminal surface has no exact-owner backend",
            vm_id
        ));
        return Err(StartError::ConsoleUnsupported);
    }
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
    // Staging validates the exact terminal backend above, but intentionally
    // does not claim it or bind input.  A guest may fail during ordinary
    // startup before it reaches its TUI; Shell2 must remain usable in that
    // interval.  `terminal_lease_current(0)` performs the owner claim later,
    // under the same per-VM transition gate used for reentry.
    // Text-only Blueprints retain their existing Matrix input route; deferred
    // ownership applies solely to terminal-capable surfaces.
    if !console_surface.is_terminal()
        && let Some(target) = console_target.as_ref()
        && !crate::shell2::bind_matrix_target_vm_input(target, vm_id)
    {
        hvwarnf(format_args!("hv: vm{} console route: matrix input bind busy", vm_id));
        return Err(StartError::ConsoleBusy);
    }
    let process_context = BlueprintProcessContext {
        args: crate::hv::blueprint::build_process_args(
            state.archive.as_str(),
            state.app_args.as_slice(),
        ),
        vars: crate::hv::blueprint::build_process_env(
            state.archive.as_str(),
            Some(app_fs_root.as_str()),
            Some(&state.identity),
            state.launch_script.as_deref(),
            trueosfs_scope,
        ),
        console_target,
        console_surface,
        console_route,
        console_attached: true,
        console_attach_generation: 0,
        console_attach_inflight: false,
        app_command_passthrough,
        terminal_lease: BlueprintTerminalLeaseState::for_surface(console_surface),
        terminal_surface_generation: 1,
        console_input: VecDeque::new(),
        control_shell_line: AllocVec::new(),
        tui_demo: None,
        exit_reason: None,
    };
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

const BLUEPRINT_PORTABLE_MAGIC: u32 = u32::from_le_bytes(*b"BPS1");
const BLUEPRINT_PORTABLE_VERSION: u32 = 1;

fn portable_push_u32(out: &mut AllocVec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn portable_push_u64(out: &mut AllocVec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn portable_push_bytes(out: &mut AllocVec<u8>, value: &[u8]) {
    portable_push_u64(out, value.len() as u64);
    out.extend_from_slice(value);
}

fn portable_push_optional_string(out: &mut AllocVec<u8>, value: Option<&str>) {
    match value {
        Some(value) => {
            out.push(1);
            portable_push_bytes(out, value.as_bytes());
        }
        None => out.push(0),
    }
}

/// Serialize the host-owned part of a paused Blueprint. Guest execution state
/// and guest-owned allocations live in the other persistent envelope members.
pub fn snapshot_blueprint_portable_state(vm_id: u8) -> Result<AllocVec<u8>, &'static str> {
    let state = blueprint_launch_snapshot(vm_id).ok_or("blueprint launch state missing")?;
    let surface = BLUEPRINT_PROCESS_CONTEXTS
        .get(vm_id as usize)
        .and_then(|slot| slot.lock().as_ref().map(|context| context.console_surface))
        .ok_or("blueprint process context missing")?;
    let mut out = AllocVec::new();
    portable_push_u32(&mut out, BLUEPRINT_PORTABLE_MAGIC);
    portable_push_u32(&mut out, BLUEPRINT_PORTABLE_VERSION);
    out.push(match surface {
        BlueprintConsoleSurface::Text => 0,
        BlueprintConsoleSurface::Terminal => 1,
    });
    portable_push_bytes(&mut out, state.archive.as_bytes());
    portable_push_bytes(&mut out, state.module_bytes.as_slice());
    portable_push_bytes(&mut out, state.unpacked_bytes.as_slice());
    portable_push_u32(&mut out, state.app_args.len() as u32);
    for arg in state.app_args.iter() {
        portable_push_bytes(&mut out, arg.as_bytes());
    }
    portable_push_bytes(&mut out, state.app_fs_root.as_bytes());
    out.extend_from_slice(&state.identity.instance);
    out.extend_from_slice(&state.identity.lineage);
    portable_push_u64(&mut out, state.identity.generation);
    out.push(state.identity.clone as u8);
    portable_push_optional_string(&mut out, state.identity.name.as_deref());
    portable_push_optional_string(&mut out, state.identity.peer.as_deref());
    Ok(out)
}

fn portable_take_u32(bytes: &[u8], offset: &mut usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    let raw = bytes.get(*offset..end)?.try_into().ok()?;
    *offset = end;
    Some(u32::from_le_bytes(raw))
}

fn portable_take_u64(bytes: &[u8], offset: &mut usize) -> Option<u64> {
    let end = offset.checked_add(8)?;
    let raw = bytes.get(*offset..end)?.try_into().ok()?;
    *offset = end;
    Some(u64::from_le_bytes(raw))
}

fn portable_take_bytes<'a>(bytes: &'a [u8], offset: &mut usize) -> Option<&'a [u8]> {
    let len = usize::try_from(portable_take_u64(bytes, offset)?).ok()?;
    let end = offset.checked_add(len)?;
    let value = bytes.get(*offset..end)?;
    *offset = end;
    Some(value)
}

fn portable_take_string(bytes: &[u8], offset: &mut usize) -> Option<AllocString> {
    Some(AllocString::from(core::str::from_utf8(portable_take_bytes(bytes, offset)?).ok()?))
}

fn portable_take_optional_string(bytes: &[u8], offset: &mut usize) -> Option<Option<AllocString>> {
    let present = *bytes.get(*offset)?;
    *offset += 1;
    match present {
        0 => Some(None),
        1 => portable_take_string(bytes, offset).map(Some),
        _ => None,
    }
}

pub fn restore_blueprint_portable_state(
    vm_id: u8,
    bytes: &[u8],
    console_target: Option<MatrixTarget>,
) -> Result<(), &'static str> {
    let vm = vm_slot(vm_id).ok_or("unsupported vm id")?;
    if vm.running.load(Ordering::Acquire)
        || vm.starting.load(Ordering::Acquire)
        || blueprint_launch_active(vm_id)
    {
        return Err("vm slot is busy");
    }
    let mut offset = 0usize;
    if portable_take_u32(bytes, &mut offset) != Some(BLUEPRINT_PORTABLE_MAGIC)
        || portable_take_u32(bytes, &mut offset) != Some(BLUEPRINT_PORTABLE_VERSION)
    {
        return Err("bad blueprint state image");
    }
    let surface = match bytes.get(offset).copied() {
        Some(0) => BlueprintConsoleSurface::Text,
        Some(1) => BlueprintConsoleSurface::Terminal,
        _ => return Err("bad blueprint console surface"),
    };
    offset += 1;
    let archive = portable_take_string(bytes, &mut offset).ok_or("blueprint archive")?;
    let module_bytes = portable_take_bytes(bytes, &mut offset)
        .ok_or("blueprint module")?
        .to_vec();
    let unpacked_bytes = portable_take_bytes(bytes, &mut offset)
        .ok_or("blueprint payload")?
        .to_vec();
    let arg_count = portable_take_u32(bytes, &mut offset).ok_or("blueprint args")? as usize;
    let mut app_args = AllocVec::with_capacity(arg_count);
    for _ in 0..arg_count {
        app_args.push(portable_take_string(bytes, &mut offset).ok_or("blueprint arg")?);
    }
    let app_fs_root = portable_take_string(bytes, &mut offset).ok_or("blueprint fs root")?;
    let identity_end = offset.checked_add(16).ok_or("blueprint instance")?;
    let instance: [u8; 16] = bytes
        .get(offset..identity_end)
        .and_then(|value| value.try_into().ok())
        .ok_or("blueprint instance")?;
    offset = identity_end;
    let lineage_end = offset.checked_add(16).ok_or("blueprint lineage")?;
    let lineage: [u8; 16] = bytes
        .get(offset..lineage_end)
        .and_then(|value| value.try_into().ok())
        .ok_or("blueprint lineage")?;
    offset = lineage_end;
    let generation = portable_take_u64(bytes, &mut offset).ok_or("blueprint generation")?;
    let clone = match bytes.get(offset).copied() {
        Some(0) => false,
        Some(1) => true,
        _ => return Err("blueprint clone flag"),
    };
    offset += 1;
    let name = portable_take_optional_string(bytes, &mut offset).ok_or("blueprint name")?;
    let peer = portable_take_optional_string(bytes, &mut offset).ok_or("blueprint peer")?;
    if offset != bytes.len() {
        return Err("trailing blueprint state bytes");
    }
    let identity = BlueprintInstanceIdentity {
        instance,
        lineage,
        generation,
        clone,
        name,
        peer,
    };
    if !memory::arm_guest_rel_exec_for_vm(vm_id) {
        return Err("blueprint execute capability unavailable");
    }
    let state = BlueprintLaunchState {
        archive: archive.clone(),
        module_bytes,
        unpacked_bytes,
        app_args,
        launch_script: None,
        app_fs_root,
        identity: identity.clone(),
    };
    if stage_blueprint_launch(vm_id, state, console_target, surface).is_err() {
        memory::release_guest_rel_exec_for_vm(vm_id);
        return Err("blueprint process reconstruction failed");
    }
    if let Some(slot) = BLUEPRINT_INSTANCE_IDENTITIES.get(vm_id as usize) {
        *slot.lock() = Some(identity);
    }
    if let Some(mode) = VM_BOOT_MODES.get(vm_id as usize) {
        *mode.lock() = VmBootMode::Hull;
    }
    set_blueprint_lifecycle_capability(vm_id, archive.as_str(), true);
    vm.pause_store_seq
        .store(crate::hv::store::current_committed_seq(vm_id), Ordering::Release);
    vm.pause_latched.store(true, Ordering::Release);
    suspend_blueprint_process_context(vm_id);
    Ok(())
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

pub(crate) fn blueprint_process_env_var(vm_id: u8, key: &str) -> Option<AllocString> {
    let context = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize)?.lock();
    context.as_ref()?.vars.get(key).cloned()
}

pub(crate) fn blueprint_process_env_text(vm_id: u8) -> Option<AllocString> {
    let context = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize)?.lock();
    let context = context.as_ref()?;
    let mut out = AllocString::new();
    for (key, value) in context.vars.iter() {
        let _ = writeln!(out, "{}={}", key, value);
    }
    Some(out)
}

fn blueprint_context_path(vm_id: u8, requested: &str) -> Option<AllocString> {
    let context = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize)?.lock();
    let context = context.as_ref()?;
    let root = context.vars.get("TRUEOS_APP_FS_ROOT").cloned();
    let home = context
        .vars
        .get("HOME")
        .cloned()
        .unwrap_or_else(|| AllocString::from("/"));

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

fn blueprint_console_hunt_log(vm_id: u8, data: &[u8]) -> bool {
    const CROSSTERM_HEADER: &str = "[crossterm-resize-probe:INFO] ";
    const REBELS_NEW_GAME_HEADER: &str = "[rebels-new-game-probe:INFO] ";
    const REBELS_MULTI_RT_HEADER: &str = "[rebels-multi-rt-probe:INFO] ";

    let Ok(text) = core::str::from_utf8(data) else {
        return false;
    };
    let (purpose, message) = if let Some(message) = text.strip_prefix(CROSSTERM_HEADER) {
        ("crossterm-resize-probe", message)
    } else if let Some(message) = text.strip_prefix(REBELS_NEW_GAME_HEADER) {
        ("rebels-new-game-probe", message)
    } else if let Some(message) = text.strip_prefix(REBELS_MULTI_RT_HEADER) {
        ("rebels-multi-rt-probe", message)
    } else {
        return false;
    };
    let message = message.trim_end_matches(&['\r', '\n'][..]);
    if purpose == "crossterm-resize-probe" && message == "typed surface change -> resize event" {
        crate::log_os::blueprint_important_line(format_args!(
            "terminal-lifecycle: vm={} phase=surface-change state=resize-delivered\n",
            vm_id
        ));
    }
    crate::log_os::log_with_area_purpose(
        crate::log_os::flags::LogArea::Blueprint,
        log_os_core::LogLevel::Info,
        Some(purpose),
        format_args!("vm{}: {}\n", vm_id, message),
    );
    true
}

pub(crate) fn blueprint_console_write(vm_id: u8, data: &[u8]) -> usize {
    let (target, surface, route, lease) = {
        let context = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize);
        context
            .and_then(|slot| {
                let guard = slot.lock();
                let context = guard.as_ref()?;
                Some((
                    context.console_target.clone(),
                    context.console_surface,
                    context.console_route,
                    context.terminal_lease,
                ))
            })
            .unwrap_or((
                None,
                BlueprintConsoleSurface::Text,
                BlueprintConsoleRoute::Matrix,
                BlueprintTerminalLeaseState::Unsupported,
            ))
    };
    if blueprint_console_hunt_log(vm_id, data) {
        return data.len();
    }
    if lease.suppresses_terminal_output() {
        // A parked Blueprint may keep working headlessly, but its ordinary
        // text belongs in LogOs; Shell2 owns the visible prompt. Raw terminal
        // bytes are accepted and deliberately sunk below.
        blueprint_console_text_lines(vm_id, None, data);
        return data.len();
    }
    if route.is_net_shell_direct() {
        return if crate::shell2::backends::net_tcp::net_shell_direct_write(vm_id, data) {
            data.len()
        } else {
            0
        };
    }
    if surface.is_terminal() {
        let written = blueprint_console_write_raw_to_target(vm_id, target.as_ref(), data);
        blueprint_console_text_lines(vm_id, None, &data[..core::cmp::min(written, data.len())]);
        written
    } else {
        blueprint_console_text_lines(vm_id, target.as_ref(), data);
        data.len()
    }
}

pub(crate) fn blueprint_console_raw_write(vm_id: u8, data: &[u8]) -> usize {
    let (target, surface, route, lease) = {
        let context = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize);
        context
            .and_then(|slot| {
                let guard = slot.lock();
                let context = guard.as_ref()?;
                Some((
                    context.console_target.clone(),
                    context.console_surface,
                    context.console_route,
                    context.terminal_lease,
                ))
            })
            .unwrap_or((
                None,
                BlueprintConsoleSurface::Text,
                BlueprintConsoleRoute::Matrix,
                BlueprintTerminalLeaseState::Unsupported,
            ))
    };
    if lease.suppresses_terminal_output() {
        // Keep guest terminal guards deterministic while Shell2 owns the
        // surface: raw paint reports completion but cannot corrupt its prompt.
        return data.len();
    }
    if route.is_net_shell_direct() {
        return if crate::shell2::backends::net_tcp::net_shell_direct_write(vm_id, data) {
            data.len()
        } else {
            0
        };
    }
    if surface.is_terminal() {
        blueprint_console_write_raw_to_target(vm_id, target.as_ref(), data)
    } else {
        blueprint_console_text_lines(vm_id, target.as_ref(), data);
        data.len()
    }
}

pub(crate) fn blueprint_console_konsole_size(vm_id: u8) -> (u32, u32) {
    let (target, route, suppress_terminal_output) = {
        let context = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize);
        context
            .and_then(|slot| {
                let guard = slot.lock();
                let context = guard.as_ref()?;
                Some((
                    context.console_target.clone(),
                    context.console_route,
                    context.terminal_lease.suppresses_terminal_output(),
                ))
            })
            .unwrap_or((None, BlueprintConsoleRoute::Matrix, false))
    };
    if suppress_terminal_output {
        return (180, 24);
    }
    if route.is_net_shell_direct() {
        let (cols, rows) = crate::shell2::net_shell_terminal_size();
        return (cols.min(u32::MAX as usize) as u32, rows.min(u32::MAX as usize) as u32);
    }
    if let Some(target) = target.as_ref() {
        let (cols, rows) = crate::shell2::konsole_viewport_size_for_target(target);
        return (cols.min(u32::MAX as usize) as u32, rows.min(u32::MAX as usize) as u32);
    }
    (180, 24)
}

pub(crate) fn blueprint_console_konsole_begin_frame(
    vm_id: u8,
    cols: usize,
    rows: usize,
    terminal_handoff: bool,
) -> (u32, u32) {
    let (target, route, suppress_terminal_output) = {
        let context = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize);
        context
            .and_then(|slot| {
                let guard = slot.lock();
                let context = guard.as_ref()?;
                Some((
                    context.console_target.clone(),
                    context.console_route,
                    context.terminal_lease.suppresses_terminal_output(),
                ))
            })
            .unwrap_or((None, BlueprintConsoleRoute::Matrix, false))
    };
    if suppress_terminal_output {
        return (
            cols.max(1).min(u32::MAX as usize) as u32,
            rows.max(1).min(u32::MAX as usize) as u32,
        );
    }
    if route.is_net_shell_direct() {
        let (cols, rows) = crate::shell2::net_shell_terminal_size();
        return (cols.min(u32::MAX as usize) as u32, rows.min(u32::MAX as usize) as u32);
    }
    if let Some(target) = target.as_ref() {
        let (cols, rows) =
            crate::shell2::konsole_begin_frame_for_target(target, cols, rows, terminal_handoff);
        return (cols.min(u32::MAX as usize) as u32, rows.min(u32::MAX as usize) as u32);
    }
    (cols.max(1).min(u32::MAX as usize) as u32, rows.max(1).min(u32::MAX as usize) as u32)
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
    let Some(slot) = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize) else {
        return Err(BlueprintTerminalLeaseError::Unsupported);
    };
    let Some(transition_slot) = BLUEPRINT_TERMINAL_TRANSITIONS.get(vm_id as usize) else {
        return Err(BlueprintTerminalLeaseError::Unsupported);
    };
    let _transition = transition_slot.lock();

    // The initial `0` call is not merely an observation: it is the precise
    // point at which a launch-reserved terminal becomes visible to the guest.
    // Keep the provisional state published while taking backend locks, so
    // teardown/resume cannot mistake an in-flight claim for an unowned route.
    if ready_epoch == 0 {
        let (target, direct, local_handoff) = {
            let mut guard = slot.lock();
            let Some(context) = guard.as_mut() else {
                return Err(BlueprintTerminalLeaseError::Unsupported);
            };
            if !context.console_attached {
                return Err(BlueprintTerminalLeaseError::Detached);
            }
            match context.terminal_lease {
                BlueprintTerminalLeaseState::Reserved => {
                    let target = context.console_target.clone();
                    let direct = context.console_route.is_net_shell_direct();
                    let local_handoff = !direct
                        && blueprint_uses_local_terminal_handoff(
                            context.console_surface,
                            target.as_ref(),
                        );
                    context.terminal_lease = BlueprintTerminalLeaseState::Claiming {
                        // Ticket zero is reserved for the initial claim;
                        // parked/reentry tickets are always real epochs.
                        ticket: 0,
                        epoch: 1,
                        direct,
                    };
                    (target, direct, local_handoff)
                }
                BlueprintTerminalLeaseState::Active { epoch, .. } => return Ok(epoch),
                BlueprintTerminalLeaseState::Unsupported => {
                    return Err(BlueprintTerminalLeaseError::Unsupported);
                }
                _ => return Err(BlueprintTerminalLeaseError::NotActive),
            }
        };

        let backend_claimed = if direct {
            crate::shell2::backends::net_tcp::claim_net_shell_direct(vm_id)
        } else if local_handoff {
            target.as_ref().is_some_and(|target| {
                crate::shell2::claim_matrix_target_terminal_handoff(target, vm_id)
            })
        } else {
            true
        };
        let input_bound = backend_claimed
            && (direct
                || target
                    .as_ref()
                    .map(|target| crate::shell2::bind_matrix_target_vm_input(target, vm_id))
                    .unwrap_or(true));

        if !backend_claimed || !input_bound {
            let backend_rolled_back = if backend_claimed && local_handoff {
                if let Some(target) = target.as_ref() {
                    crate::shell2::release_matrix_target_terminal_handoff(target, vm_id)
                } else {
                    false
                }
            } else {
                true
            };
            if !backend_rolled_back {
                // Do not lie by returning to Reserved while the exact owner
                // may still be installed. Claiming suppresses all guest
                // terminal I/O and lets teardown retry the exact release.
                crate::log_os::blueprint_important_line(format_args!(
                    "terminal-lifecycle: vm={} phase=initial-claim-failed state=claiming epoch=1 reason=rollback-failed\n",
                    vm_id,
                ));
                return Err(BlueprintTerminalLeaseError::Busy);
            }
            let restored = {
                let mut guard = slot.lock();
                if let Some(context) = guard.as_mut()
                    && context.console_attached
                    && context.terminal_lease
                        == (BlueprintTerminalLeaseState::Claiming {
                            ticket: 0,
                            epoch: 1,
                            direct,
                        })
                {
                    context.terminal_lease = BlueprintTerminalLeaseState::Reserved;
                    true
                } else {
                    false
                }
            };
            if !restored {
                if input_bound
                    && !direct
                    && let Some(target) = target.as_ref()
                {
                    let _ = crate::shell2::unbind_matrix_target_vm(target, vm_id);
                }
                if backend_claimed && direct {
                    let _ = crate::shell2::backends::net_tcp::release_net_shell_direct(vm_id);
                }
                return Err(BlueprintTerminalLeaseError::Detached);
            }
            return Err(BlueprintTerminalLeaseError::Busy);
        }

        let committed = {
            let mut guard = slot.lock();
            if let Some(context) = guard.as_mut()
                && context.console_attached
                && context.terminal_lease
                    == (BlueprintTerminalLeaseState::Claiming {
                        ticket: 0,
                        epoch: 1,
                        direct,
                    })
            {
                context.terminal_lease = BlueprintTerminalLeaseState::Active {
                    epoch: 1,
                    observed: true,
                    ready: false,
                };
                context.terminal_surface_generation =
                    context.terminal_surface_generation.saturating_add(1).max(1);
                context.console_input.clear();
                context.control_shell_line.clear();
                true
            } else {
                false
            }
        };
        if !committed {
            if direct {
                let _ = crate::shell2::backends::net_tcp::release_net_shell_direct(vm_id);
            }
            if local_handoff && let Some(target) = target.as_ref() {
                let _ = crate::shell2::release_matrix_target_terminal_handoff(target, vm_id);
            }
            if !direct && let Some(target) = target.as_ref() {
                let _ = crate::shell2::unbind_matrix_target_vm(target, vm_id);
            }
            return Err(BlueprintTerminalLeaseError::Detached);
        }
        crate::log_os::blueprint_important_line(format_args!(
            "terminal-lifecycle: vm={} phase=initial-claim state=active epoch=1 handoff=cli->terminal route={} target={}\n",
            vm_id,
            if direct { "net-shell-direct" } else { "matrix" },
            target.is_some() as u8,
        ));
        crate::log_os::blueprint_important_line(format_args!(
            "terminal-lifecycle: vm={} phase=app-observed state=active epoch=1\n",
            vm_id,
        ));
        return Ok(1);
    }

    let marker = {
        let mut guard = slot.lock();
        let Some(context) = guard.as_mut() else {
            return Err(BlueprintTerminalLeaseError::Unsupported);
        };
        if !context.console_attached {
            return Err(BlueprintTerminalLeaseError::Detached);
        }
        let BlueprintTerminalLeaseState::Active {
            epoch,
            observed,
            ready,
        } = &mut context.terminal_lease
        else {
            return Err(match context.terminal_lease {
                BlueprintTerminalLeaseState::Unsupported => {
                    BlueprintTerminalLeaseError::Unsupported
                }
                _ => BlueprintTerminalLeaseError::NotActive,
            });
        };
        if ready_epoch != 0 && ready_epoch != *epoch {
            return Err(BlueprintTerminalLeaseError::Stale);
        }
        let marker = if ready_epoch == 0 && !*observed {
            *observed = true;
            Some("app-observed")
        } else if ready_epoch != 0 && !*ready {
            *observed = true;
            *ready = true;
            Some("app-ready")
        } else {
            None
        };
        (*epoch, marker)
    };
    if let Some(phase) = marker.1 {
        crate::log_os::blueprint_important_line(format_args!(
            "terminal-lifecycle: vm={} phase={} state=active epoch={}\n",
            vm_id, phase, marker.0
        ));
    }
    Ok(marker.0)
}

pub(crate) fn blueprint_terminal_surface_snapshot(
    vm_id: u8,
) -> Result<BlueprintTerminalSurfaceSnapshot, BlueprintTerminalLeaseError> {
    let Some(slot) = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize) else {
        return Err(BlueprintTerminalLeaseError::Unsupported);
    };
    let (route, target, generation) = {
        let guard = slot.lock();
        let Some(context) = guard.as_ref() else {
            return Err(BlueprintTerminalLeaseError::Unsupported);
        };
        if !context.console_attached {
            return Err(BlueprintTerminalLeaseError::Detached);
        }
        match context.terminal_lease {
            BlueprintTerminalLeaseState::Active { .. } => {}
            BlueprintTerminalLeaseState::Unsupported => {
                return Err(BlueprintTerminalLeaseError::Unsupported);
            }
            _ => return Err(BlueprintTerminalLeaseError::NotActive),
        }
        (
            context.console_route,
            context.console_target.clone(),
            context.terminal_surface_generation.max(1),
        )
    };

    // Backend state is sampled only after dropping the process-context lock.
    // Terminal ownership code relies on that lock order during claim/release.
    if route.is_net_shell_direct() {
        let snapshot = crate::shell2::backends::net_tcp::net_shell_direct_surface_snapshot(vm_id)
            .ok_or(BlueprintTerminalLeaseError::Busy)?;
        return Ok(BlueprintTerminalSurfaceSnapshot {
            generation: snapshot.generation,
            cols: snapshot.cols,
            rows: snapshot.rows,
        });
    }

    let (cols, rows) = target
        .as_ref()
        .map(crate::shell2::konsole_viewport_size_for_target)
        .unwrap_or((180, 24));
    Ok(BlueprintTerminalSurfaceSnapshot {
        generation,
        cols: cols.max(1).min(u32::MAX as usize) as u32,
        rows: rows.max(1).min(u32::MAX as usize) as u32,
    })
}

pub(crate) fn blueprint_terminal_lease_release(
    vm_id: u8,
    expected_epoch: u64,
) -> Result<u64, BlueprintTerminalLeaseError> {
    let Some(slot) = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize) else {
        return Err(BlueprintTerminalLeaseError::Unsupported);
    };
    let Some(transition_slot) = BLUEPRINT_TERMINAL_TRANSITIONS.get(vm_id as usize) else {
        return Err(BlueprintTerminalLeaseError::Unsupported);
    };
    let _transition = transition_slot.lock();
    let (ticket, target, was_direct, was_local_handoff) = {
        let mut guard = slot.lock();
        let Some(context) = guard.as_mut() else {
            return Err(BlueprintTerminalLeaseError::Unsupported);
        };
        if !context.console_attached {
            return Err(BlueprintTerminalLeaseError::Detached);
        }
        let epoch = match context.terminal_lease {
            BlueprintTerminalLeaseState::Active { epoch, .. } => epoch,
            BlueprintTerminalLeaseState::Unsupported => {
                return Err(BlueprintTerminalLeaseError::Unsupported);
            }
            BlueprintTerminalLeaseState::Reserved
            | BlueprintTerminalLeaseState::Releasing { .. }
            | BlueprintTerminalLeaseState::Parked { .. }
            | BlueprintTerminalLeaseState::ReentryRequested { .. }
            | BlueprintTerminalLeaseState::Claiming { .. } => {
                return Err(BlueprintTerminalLeaseError::NotActive);
            }
        };
        if expected_epoch != 0 && expected_epoch != epoch {
            return Err(BlueprintTerminalLeaseError::Stale);
        }
        let was_direct = context.console_route.is_net_shell_direct();
        let was_local_handoff = !was_direct
            && blueprint_uses_local_terminal_handoff(
                context.console_surface,
                context.console_target.as_ref(),
            );
        // Keep the active presentation recorded until the exact backend owner
        // acknowledges release. Teardown can therefore still identify and
        // clean a release that races suspension or process exit.
        context.terminal_lease = BlueprintTerminalLeaseState::Releasing { epoch };
        (epoch, context.console_target.clone(), was_direct, was_local_handoff)
    };

    // The VM remains alive while terminal ownership returns to Shell2. The
    // parking ticket is the only authority that can accept a later reentry.
    let released = if was_direct {
        crate::shell2::backends::net_tcp::release_net_shell_direct(vm_id)
    } else if was_local_handoff {
        target.as_ref().is_some_and(|target| {
            crate::shell2::release_matrix_target_terminal_handoff(target, vm_id)
        })
    } else {
        true
    };
    let shell_bound = released
        && target
            .as_ref()
            .map(|target| crate::shell2::bind_matrix_target_vm(target, vm_id))
            .unwrap_or(true);
    let committed = {
        let mut guard = slot.lock();
        match guard.as_mut() {
            Some(context)
                if context.console_attached
                    && context.terminal_lease
                        == (BlueprintTerminalLeaseState::Releasing { epoch: ticket }) =>
            {
                if released && shell_bound {
                    context.console_surface = BlueprintConsoleSurface::Text;
                    context.console_route = BlueprintConsoleRoute::Matrix;
                    context.terminal_lease = BlueprintTerminalLeaseState::Parked { ticket };
                    context.console_input.clear();
                    context.control_shell_line.clear();
                    Ok(())
                } else {
                    // Never retain a false Active claim after the backend
                    // rejected our owner. The app receives a hard error and
                    // cannot wait forever on a ticket never established.
                    context.console_surface = BlueprintConsoleSurface::Text;
                    context.console_route = BlueprintConsoleRoute::Matrix;
                    context.terminal_lease = BlueprintTerminalLeaseState::Unsupported;
                    Err(BlueprintTerminalLeaseError::Busy)
                }
            }
            _ => Err(BlueprintTerminalLeaseError::Detached),
        }
    };
    if let Err(error) = committed {
        // Binding happens outside the process-context lock by design. If
        // suspension won the transition, remove that late bind rather than
        // re-binding Shell2 to a detached VM.
        if shell_bound && let Some(target) = target.as_ref() {
            crate::shell2::unbind_matrix_target_vm(target, vm_id);
        }
        crate::log_os::blueprint_important_line(format_args!(
            "terminal-lifecycle: vm={} phase=park-failed state=lease-lost epoch={} released={} shell_bound={}\n",
            vm_id, ticket, released as u8, shell_bound as u8
        ));
        return Err(error);
    }
    crate::log_os::blueprint_important_line(format_args!(
        "terminal-lifecycle: vm={} phase=park-ack state=parked ticket={} handoff=terminal->shell2 target={}\n",
        vm_id,
        ticket,
        target.is_some() as u8
    ));
    Ok(ticket)
}

pub(crate) fn blueprint_console_return_to_cli(vm_id: u8) -> bool {
    match blueprint_terminal_lease_release(vm_id, 0) {
        Ok(_) => true,
        Err(BlueprintTerminalLeaseError::NotActive) => BLUEPRINT_PROCESS_CONTEXTS
            .get(vm_id as usize)
            .and_then(|slot| {
                let guard = slot.lock();
                let context = guard.as_ref()?;
                Some(matches!(
                    context.terminal_lease,
                    BlueprintTerminalLeaseState::Reserved
                        | BlueprintTerminalLeaseState::Parked { .. }
                        | BlueprintTerminalLeaseState::ReentryRequested { .. }
                ))
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
            BlueprintTerminalLeaseState::Claiming { .. } => {
                BlueprintTerminalReentryRequest::AlreadyRequested
            }
            BlueprintTerminalLeaseState::Active { .. }
            | BlueprintTerminalLeaseState::Releasing { .. } => {
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
    }
    request
}

fn log_blueprint_terminal_reentry_failed(vm_id: u8, ticket: u64, epoch: u64, reason: &str) {
    crate::log_os::blueprint_important_line(format_args!(
        "terminal-lifecycle: vm={} phase=reentry-failed state=terminal ticket={} epoch={} reason={}\n",
        vm_id, ticket, epoch, reason
    ));
}

pub(crate) fn blueprint_terminal_lease_poll_reentry(
    vm_id: u8,
    ticket: u64,
) -> Result<BlueprintTerminalReentryPoll, BlueprintTerminalLeaseError> {
    let Some(slot) = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize) else {
        return Err(BlueprintTerminalLeaseError::Unsupported);
    };
    let Some(transition_slot) = BLUEPRINT_TERMINAL_TRANSITIONS.get(vm_id as usize) else {
        return Err(BlueprintTerminalLeaseError::Unsupported);
    };
    let _transition = transition_slot.lock();
    let (epoch, target, direct) = {
        let mut guard = slot.lock();
        let Some(context) = guard.as_mut() else {
            return Err(BlueprintTerminalLeaseError::Unsupported);
        };
        if !context.console_attached {
            return Err(BlueprintTerminalLeaseError::Detached);
        }
        match context.terminal_lease {
            BlueprintTerminalLeaseState::Parked { ticket: current } => {
                return if current == ticket {
                    Ok(BlueprintTerminalReentryPoll::Pending)
                } else {
                    Err(BlueprintTerminalLeaseError::Stale)
                };
            }
            BlueprintTerminalLeaseState::ReentryRequested {
                ticket: current,
                epoch,
            } if current == ticket => {
                let target = context.console_target.clone();
                let direct = blueprint_uses_net_shell_direct_path(
                    BlueprintConsoleSurface::Terminal,
                    target.as_ref(),
                );
                context.terminal_lease = BlueprintTerminalLeaseState::Claiming {
                    ticket,
                    epoch,
                    direct,
                };
                (epoch, target, direct)
            }
            BlueprintTerminalLeaseState::ReentryRequested { .. } => {
                return Err(BlueprintTerminalLeaseError::Stale);
            }
            BlueprintTerminalLeaseState::Claiming {
                ticket: current, ..
            } if current == ticket => {
                return Err(BlueprintTerminalLeaseError::Busy);
            }
            BlueprintTerminalLeaseState::Claiming { .. } => {
                return Err(BlueprintTerminalLeaseError::Stale);
            }
            BlueprintTerminalLeaseState::Active { epoch, .. } if epoch > ticket => {
                return Ok(BlueprintTerminalReentryPoll::Ready(epoch));
            }
            BlueprintTerminalLeaseState::Active { .. } => {
                return Err(BlueprintTerminalLeaseError::Stale);
            }
            BlueprintTerminalLeaseState::Releasing { .. } => {
                return Err(BlueprintTerminalLeaseError::NotActive);
            }
            BlueprintTerminalLeaseState::Reserved => {
                return Err(BlueprintTerminalLeaseError::NotActive);
            }
            BlueprintTerminalLeaseState::Unsupported => {
                return Err(BlueprintTerminalLeaseError::Unsupported);
            }
        }
    };

    // Reentry is two-phase: Shell2 records the request, but this guest poll is
    // what actually claims and commits terminal ownership. If the app never
    // polls, Shell2 remains interactive instead of handing input away.
    let restore_request = || {
        let mut guard = slot.lock();
        let Some(context) = guard.as_mut() else {
            return false;
        };
        if !context.console_attached
            || context.terminal_lease
                != (BlueprintTerminalLeaseState::Claiming {
                    ticket,
                    epoch,
                    direct,
                })
        {
            return false;
        }
        context.terminal_lease = BlueprintTerminalLeaseState::ReentryRequested { ticket, epoch };
        true
    };
    if direct && !crate::shell2::backends::net_tcp::claim_net_shell_direct(vm_id) {
        return if restore_request() {
            log_blueprint_terminal_reentry_failed(vm_id, ticket, epoch, "claim-busy");
            Err(BlueprintTerminalLeaseError::Busy)
        } else {
            Err(BlueprintTerminalLeaseError::Detached)
        };
    }
    let local_handoff = !direct
        && blueprint_uses_local_terminal_handoff(
            BlueprintConsoleSurface::Terminal,
            target.as_ref(),
        );
    if local_handoff
        && !target.as_ref().is_some_and(|target| {
            crate::shell2::claim_matrix_target_terminal_handoff(target, vm_id)
        })
    {
        return if restore_request() {
            log_blueprint_terminal_reentry_failed(vm_id, ticket, epoch, "claim-busy");
            Err(BlueprintTerminalLeaseError::Busy)
        } else {
            Err(BlueprintTerminalLeaseError::Detached)
        };
    }
    let input_bound = direct
        || target
            .as_ref()
            .map(|target| crate::shell2::bind_matrix_target_vm_input(target, vm_id))
            .unwrap_or(true);
    if !input_bound {
        if local_handoff && let Some(target) = target.as_ref() {
            let _ = crate::shell2::release_matrix_target_terminal_handoff(target, vm_id);
        }
        let failed = {
            let mut guard = slot.lock();
            if let Some(context) = guard.as_mut()
                && context.console_attached
                && context.terminal_lease
                    == (BlueprintTerminalLeaseState::Claiming {
                        ticket,
                        epoch,
                        direct,
                    })
            {
                context.terminal_lease = BlueprintTerminalLeaseState::Unsupported;
                true
            } else {
                false
            }
        };
        if failed {
            log_blueprint_terminal_reentry_failed(vm_id, ticket, epoch, "input-bind");
            return Err(BlueprintTerminalLeaseError::Busy);
        }
        return Err(BlueprintTerminalLeaseError::Detached);
    }

    let committed = {
        let mut guard = slot.lock();
        if let Some(context) = guard.as_mut() {
            if context.console_attached
                && context.terminal_lease
                    == (BlueprintTerminalLeaseState::Claiming {
                        ticket,
                        epoch,
                        direct,
                    })
            {
                context.console_surface = BlueprintConsoleSurface::Terminal;
                context.console_route = if direct {
                    BlueprintConsoleRoute::NetShellDirect
                } else {
                    BlueprintConsoleRoute::Matrix
                };
                context.terminal_lease = BlueprintTerminalLeaseState::Active {
                    epoch,
                    observed: true,
                    ready: false,
                };
                context.terminal_surface_generation =
                    context.terminal_surface_generation.saturating_add(1).max(1);
                context.console_input.clear();
                context.control_shell_line.clear();
                true
            } else {
                false
            }
        } else {
            false
        }
    };
    if !committed {
        if direct {
            let _ = crate::shell2::backends::net_tcp::release_net_shell_direct(vm_id);
        }
        if local_handoff && let Some(target) = target.as_ref() {
            let _ = crate::shell2::release_matrix_target_terminal_handoff(target, vm_id);
        }
        if !direct && let Some(target) = target.as_ref() {
            crate::shell2::unbind_matrix_target_vm(target, vm_id);
        }
        log_blueprint_terminal_reentry_failed(vm_id, ticket, epoch, "commit-stale");
        return Err(BlueprintTerminalLeaseError::Stale);
    }
    crate::log_os::blueprint_important_line(format_args!(
        "terminal-lifecycle: vm={} phase=reentry-claim state=active ticket={} epoch={} handoff=shell2->terminal target={}\n",
        vm_id,
        ticket,
        epoch,
        target.is_some() as u8
    ));
    Ok(BlueprintTerminalReentryPoll::Ready(epoch))
}

fn blueprint_console_write_raw_to_target(
    vm_id: u8,
    target: Option<&MatrixTarget>,
    data: &[u8],
) -> usize {
    if let Some(target) = target {
        return crate::shell2::raw_write_matrix_target_owned(&target, vm_id, data);
    }
    data.len()
}

fn blueprint_console_text_lines(vm_id: u8, target: Option<&MatrixTarget>, data: &[u8]) {
    if data.is_empty() {
        return;
    }
    if let Some(progress) = data.strip_prefix(b"\r")
        && !progress.is_empty()
        && !progress.contains(&b'\r')
        && !progress.contains(&b'\n')
        && let Ok(progress) = core::str::from_utf8(progress)
    {
        if let Some(target) = target {
            crate::shell2::print_matrix_target_progress_line(target, progress);
        }
        return;
    }
    let Some(slot) = BLUEPRINT_CONSOLE_LOG_BUFFERS.get(vm_id as usize) else {
        return;
    };

    let text = AllocString::from_utf8_lossy(data);
    let mut ready = VecDeque::new();
    {
        let mut guard = slot.lock();
        let pending = guard.get_or_insert_with(AllocString::new);
        pending.push_str(text.as_ref());

        while let Some(newline_idx) = pending.find('\n') {
            let mut line = AllocString::from(&pending[..newline_idx]);
            if line.ends_with('\r') {
                line.pop();
            }
            ready.push_back(line);
            pending.drain(..=newline_idx);
        }

        if pending.len() > HV_LOG_LINE {
            let mut line = AllocString::new();
            core::mem::swap(pending, &mut line);
            ready.push_back(line);
        }
    }

    for line in ready {
        if line.is_empty() {
            continue;
        }
        crate::log_os::log_with_area_purpose(
            crate::log_os::flags::LogArea::Blueprint,
            log_os_core::LogLevel::Info,
            Some("blueprint"),
            format_args!("vm{}: {}\n", vm_id, line.as_str()),
        );
        if let Some(target) = target {
            crate::shell2::print_matrix_target_line(target, line.as_str());
        }
    }
}

fn blueprint_control_shell_line(vm_id: u8, line: &str) {
    blueprint_console_print_line(vm_id, line);
}

fn blueprint_control_shell_write_text(vm_id: u8, text: &str) {
    let mut wrote = false;
    for line in text.lines() {
        blueprint_console_print_line(vm_id, line);
        wrote = true;
    }
    if !wrote {
        blueprint_console_print_line(vm_id, "");
    }
}

fn blueprint_tui_demo_status_text(status: BlueprintTuiDemoStatus) -> &'static str {
    match status {
        BlueprintTuiDemoStatus::Ready => "Ready: move the cursor and activate a button.",
        BlueprintTuiDemoStatus::Inspected => {
            "Inspect: vmx-shell owns this preview, not the Blueprint."
        }
        BlueprintTuiDemoStatus::Reset => "Reset: the demo cursor and state were restored.",
    }
}

fn blueprint_tui_demo_button(selected: u8, index: u8, label: &str) -> AllocString {
    if selected == index {
        alloc::format!("▶ [ {} ] ◀", label)
    } else {
        alloc::format!("  [ {} ]  ", label)
    }
}

fn blueprint_console_render_tui_demo(vm_id: u8) {
    let presentation = BLUEPRINT_PROCESS_CONTEXTS
        .get(vm_id as usize)
        .and_then(|slot| {
            let guard = slot.lock();
            let context = guard.as_ref()?;
            Some((context.console_target.clone(), context.tui_demo?))
        });
    let Some((Some(target), demo)) = presentation else {
        return;
    };

    let buttons = alloc::format!(
        "{}    {}    {}",
        blueprint_tui_demo_button(demo.selected, 0, "Inspect"),
        blueprint_tui_demo_button(demo.selected, 1, "Reset"),
        blueprint_tui_demo_button(demo.selected, 2, "Exit"),
    );
    let lines = alloc::vec![
        AllocString::from("╭─ vmx-shell · TUI demo ─────────────────────────────────────────╮"),
        AllocString::from("│ Built-in preview; this panel is not supplied by the Blueprint. │"),
        alloc::format!("│ {:<62} │", blueprint_tui_demo_status_text(demo.status)),
        alloc::format!("│ {:<62} │", buttons),
        alloc::format!(
            "│ {:<62} │",
            "←/→ or Tab: move · Enter: activate · Esc: return to vmx-shell"
        ),
        AllocString::from("╰────────────────────────────────────────────────────────────────╯"),
    ];
    crate::shell2::replace_matrix_target_transient_lines(&target, lines.as_slice());
}

fn blueprint_console_start_tui_demo(vm_id: u8) -> bool {
    let Some(slot) = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize) else {
        return false;
    };
    {
        let mut guard = slot.lock();
        let Some(context) = guard.as_mut() else {
            return false;
        };
        if !context.console_attached || context.console_target.is_none() {
            return false;
        }
        context.tui_demo = Some(BlueprintTuiDemo::new());
        context.control_shell_line.clear();
    }
    blueprint_console_render_tui_demo(vm_id);
    true
}

fn blueprint_console_exit_tui_demo(vm_id: u8, message: &str) -> bool {
    let target = BLUEPRINT_PROCESS_CONTEXTS
        .get(vm_id as usize)
        .and_then(|slot| {
            let mut guard = slot.lock();
            let context = guard.as_mut()?;
            context.tui_demo.take()?;
            Some(context.console_target.clone())
        });
    let Some(target) = target else {
        return false;
    };
    if let Some(target) = target.as_ref() {
        crate::shell2::clear_matrix_target_transient_lines(target);
    }
    blueprint_control_shell_line(vm_id, message);
    true
}

pub(crate) fn blueprint_console_submit_tui_demo_input(vm_id: u8, byte: u8) -> bool {
    enum DemoAction {
        None,
        Render,
        Exit,
    }

    // Preserve vmx-shell's global Ctrl-C stop behavior while the preview owns
    // the remaining input stream.
    if byte == 0x03 {
        return false;
    }
    let Some(slot) = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize) else {
        return false;
    };
    let action = {
        let mut guard = slot.lock();
        let Some(context) = guard.as_mut() else {
            return false;
        };
        let Some(demo) = context.tui_demo.as_mut() else {
            return false;
        };

        match demo.escape {
            BlueprintTuiDemoEscape::Escape => {
                demo.escape_idle_ticks = 0;
                if matches!(byte, b'[' | b'O') {
                    demo.escape = BlueprintTuiDemoEscape::Csi;
                    DemoAction::None
                } else {
                    DemoAction::Exit
                }
            }
            BlueprintTuiDemoEscape::Csi => {
                demo.escape = BlueprintTuiDemoEscape::None;
                match byte {
                    b'A' | b'D' | b'Z' => {
                        demo.selected = demo.selected.checked_sub(1).unwrap_or(2);
                        DemoAction::Render
                    }
                    b'B' | b'C' => {
                        demo.selected = (demo.selected + 1) % 3;
                        DemoAction::Render
                    }
                    _ => DemoAction::None,
                }
            }
            BlueprintTuiDemoEscape::None => match byte {
                0x1b => {
                    demo.escape = BlueprintTuiDemoEscape::Escape;
                    demo.escape_idle_ticks = 0;
                    DemoAction::None
                }
                // TRUE OS maps the local Escape key to a byte that cannot be
                // mistaken for the start of a terminal escape sequence.
                crate::shell2::LOCAL_ESCAPE_KEY_BYTE | 0x11 | b'q' | b'Q' => DemoAction::Exit,
                b'\t' | b'l' | b'j' => {
                    demo.selected = (demo.selected + 1) % 3;
                    DemoAction::Render
                }
                b'h' | b'k' => {
                    demo.selected = demo.selected.checked_sub(1).unwrap_or(2);
                    DemoAction::Render
                }
                b'\r' | b'\n' | b' ' => match demo.selected {
                    0 => {
                        demo.status = BlueprintTuiDemoStatus::Inspected;
                        DemoAction::Render
                    }
                    1 => {
                        *demo = BlueprintTuiDemo::new();
                        demo.status = BlueprintTuiDemoStatus::Reset;
                        DemoAction::Render
                    }
                    _ => DemoAction::Exit,
                },
                _ => DemoAction::None,
            },
        }
    };

    match action {
        DemoAction::None => {}
        DemoAction::Render => blueprint_console_render_tui_demo(vm_id),
        DemoAction::Exit => {
            let _ = blueprint_console_exit_tui_demo(vm_id, "vmx-shell: tui demo exited");
        }
    }
    true
}

pub(crate) fn blueprint_console_tui_demo_idle(vm_id: u8) -> bool {
    const ESCAPE_IDLE_TICKS: u8 = 5;

    let Some(slot) = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize) else {
        return false;
    };
    let should_exit = {
        let mut guard = slot.lock();
        let Some(context) = guard.as_mut() else {
            return false;
        };
        let Some(demo) = context.tui_demo.as_mut() else {
            return false;
        };
        if demo.escape != BlueprintTuiDemoEscape::Escape {
            return true;
        }
        demo.escape_idle_ticks = demo.escape_idle_ticks.saturating_add(1);
        demo.escape_idle_ticks >= ESCAPE_IDLE_TICKS
    };
    if should_exit {
        let _ = blueprint_console_exit_tui_demo(vm_id, "vmx-shell: tui demo exited");
    }
    true
}

fn blueprint_app_command_passthrough_enabled(vm_id: u8) -> bool {
    BLUEPRINT_PROCESS_CONTEXTS
        .get(vm_id as usize)
        .and_then(|slot| {
            slot.lock().as_ref().map(|context| {
                context.console_attached
                    && !context.terminal_lease.suppresses_terminal_output()
                    && context.app_command_passthrough
            })
        })
        .unwrap_or(false)
}

fn blueprint_forward_app_command(vm_id: u8, raw: &str) -> bool {
    const MAX_CONSOLE_INPUT: usize = 64 * 1024;

    let Some(slot) = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize) else {
        return false;
    };
    let mut guard = slot.lock();
    let Some(context) = guard.as_mut() else {
        return false;
    };
    if !context.console_attached
        || context.terminal_lease.suppresses_terminal_output()
        || !context.app_command_passthrough
    {
        return false;
    }
    for byte in raw.bytes().chain(core::iter::once(b'\n')) {
        if context.console_input.len() >= MAX_CONSOLE_INPUT {
            let _ = context.console_input.pop_front();
        }
        context.console_input.push_back(byte);
    }
    drop(guard);
    notify_blueprint_console_input(vm_id);
    true
}

fn blueprint_control_shell_command(vm_id: u8, raw: &str) {
    let trimmed = raw.trim();
    let vmx_command = trimmed.strip_prefix("vmx_");
    if blueprint_app_command_passthrough_enabled(vm_id) {
        if let Some(command) = vmx_command {
            blueprint_control_shell_vmx_command(vm_id, command);
        } else if !trimmed.is_empty() && !blueprint_forward_app_command(vm_id, trimmed) {
            blueprint_control_shell_line(vm_id, "vmx-shell: Blueprint command channel unavailable");
        }
        return;
    }
    let Some(command) = vmx_command else {
        blueprint_control_shell_line(vm_id, "VMX controls require the `vmx_` prefix");
        return;
    };
    blueprint_control_shell_vmx_command(vm_id, command);
}

fn blueprint_control_shell_vmx_command(vm_id: u8, raw: &str) {
    let trimmed = raw.trim();
    let mut words = trimmed.split_whitespace();
    let cmd = words.next().unwrap_or("");
    let argument = words.next();
    let has_extra_arguments = words.next().is_some();
    match cmd {
        "" => {}
        "env" => match blueprint_process_env_text(vm_id) {
            Some(text) if !text.is_empty() => {
                blueprint_control_shell_write_text(vm_id, text.as_str())
            }
            _ => blueprint_control_shell_line(vm_id, "env: unavailable"),
        },
        "smp" => {
            let state = vm_state(vm_id);
            blueprint_control_shell_line(
                vm_id,
                alloc::format!(
                    "smp: vm={} running={} starting={} async_jobs=not-wired",
                    vm_id,
                    state.running as u8,
                    state.starting as u8,
                )
                .as_str(),
            );
        }
        "help" | "?" => {
            let text = if blueprint_app_command_passthrough_enabled(vm_id) {
                "VM controls: vmx_tui vmx_env vmx_smp vmx_leave vmx_stop vmx_pause vmx_snapshot vmx_preserve\n\
                 Blueprint commands are entered without a prefix\n\
                 stop: stop without a checkpoint\n\
                 pause: preserve-pause; resume by vmid from F2 pause\n\
                 snapshot: Blueprint Ready checkpoint; durable and resumable\n\
                 preserve: preserve-stop; checkpoint first, then tear down"
            } else {
                "VMX controls: vmx_tui vmx_env vmx_smp vmx_leave vmx_stop vmx_pause vmx_snapshot vmx_preserve\n\
                 vmx_tui: re-enter this Blueprint's terminal UI\n\
                 vmx_leave: return to the default Matrix slot\n\
                 vmx_stop: stop without a checkpoint\n\
                 vmx_pause: preserve-pause; resume by vmid from F2 pause\n\
                 vmx_snapshot: Blueprint Ready checkpoint; durable and resumable\n\
                 vmx_preserve: preserve-stop; checkpoint first, then tear down"
            };
            blueprint_control_shell_write_text(vm_id, text);
        }
        "tui" => {
            if blueprint_app_command_passthrough_enabled(vm_id) {
                blueprint_control_shell_line(
                    vm_id,
                    "vmx-shell: terminal TUI disabled for this launch; use Player commands directly",
                );
            } else if argument == Some("demo") && !has_extra_arguments {
                if !blueprint_console_start_tui_demo(vm_id) {
                    blueprint_control_shell_line(vm_id, "vmx-shell: tui demo is not available");
                }
            } else if argument.is_some() {
                blueprint_control_shell_line(vm_id, "usage: vmx_tui [demo]");
            } else {
                match blueprint_console_request_tui(vm_id) {
                    BlueprintTerminalReentryRequest::Requested { epoch, .. } => {
                        blueprint_control_shell_line(
                            vm_id,
                            alloc::format!(
                                "vmx-shell: terminal reentry requested epoch={epoch}; waiting for Blueprint acknowledgement"
                            )
                            .as_str(),
                        );
                    }
                    BlueprintTerminalReentryRequest::AlreadyRequested => {
                        blueprint_control_shell_line(
                            vm_id,
                            "vmx-shell: terminal reentry is already awaiting Blueprint acknowledgement",
                        );
                    }
                    BlueprintTerminalReentryRequest::NotParked => {
                        blueprint_control_shell_line(
                            vm_id,
                            "vmx-shell: the Blueprint terminal UI is already active",
                        );
                    }
                    BlueprintTerminalReentryRequest::Detached => {
                        blueprint_control_shell_line(
                            vm_id,
                            "vmx-shell: the Blueprint console is detached",
                        );
                    }
                    BlueprintTerminalReentryRequest::Unsupported => {
                        blueprint_control_shell_line(
                            vm_id,
                            "vmx-shell: this Blueprint has not implemented the TUI capability; run \"tui demo\" for the built-in terminal UI preview",
                        );
                    }
                }
            }
        }
        "stop" => match stop(vm_id) {
            Ok(true) => blueprint_control_shell_line(vm_id, "vmx-shell: stop requested"),
            Ok(false) => blueprint_control_shell_line(vm_id, "vmx-shell: vm is not running"),
            Err(err) => blueprint_control_shell_line(
                vm_id,
                alloc::format!("vmx-shell: stop failed: {:?}", err).as_str(),
            ),
        },
        "pause" => {
            let state = vm_state(vm_id);
            if !state.replicatable {
                blueprint_control_shell_line(
                    vm_id,
                    "vmx-shell: app is not tagged replicatable; use preserve for a raw checkpoint",
                );
            } else if !state.running && !state.starting {
                blueprint_control_shell_line(vm_id, "vmx-shell: vm is not running");
            } else {
                // A successful replicatable pause detaches this console before
                // returning, so publish the acknowledgement first.
                blueprint_control_shell_line(
                    vm_id,
                    "vmx-shell: requesting replicatable pause; resume it from F2 pause by vmid",
                );
                match request_replicatable_pause(vm_id) {
                    Ok(true) => {}
                    Ok(false) => blueprint_control_shell_line(
                        vm_id,
                        "vmx-shell: replicatable pause was not accepted",
                    ),
                    Err(err) => blueprint_control_shell_line(
                        vm_id,
                        alloc::format!("vmx-shell: pause failed: {:?}", err).as_str(),
                    ),
                }
            }
        }
        "snapshot" => {
            let state = vm_state(vm_id);
            if !state.replicatable {
                blueprint_control_shell_line(
                    vm_id,
                    "vmx-shell: app is not tagged replicatable; use preserve for a raw checkpoint",
                );
            } else if !state.running && !state.starting {
                blueprint_control_shell_line(vm_id, "vmx-shell: vm is not running");
            } else {
                blueprint_control_shell_line(
                    vm_id,
                    "vmx-shell: requesting replicatable snapshot; waiting for Blueprint Ready",
                );
                match request_replicatable_snapshot(vm_id) {
                    Ok(true) => {}
                    Ok(false) => blueprint_control_shell_line(
                        vm_id,
                        "vmx-shell: replicatable snapshot was not accepted",
                    ),
                    Err(err) => blueprint_control_shell_line(
                        vm_id,
                        alloc::format!("vmx-shell: snapshot failed: {:?}", err).as_str(),
                    ),
                }
            }
        }
        "preserve" => match request_preserve(vm_id) {
            Ok(true) => blueprint_control_shell_line(vm_id, "vmx-shell: preserve requested"),
            Ok(false) => blueprint_control_shell_line(vm_id, "vmx-shell: vm is not running"),
            Err(err) => blueprint_control_shell_line(
                vm_id,
                alloc::format!("vmx-shell: preserve failed: {:?}", err).as_str(),
            ),
        },
        _ => blueprint_control_shell_line(vm_id, "unknown vmx command"),
    }
}

pub(crate) fn blueprint_console_submit_control_line(vm_id: u8, line: &str) -> bool {
    if BLUEPRINT_PROCESS_CONTEXTS
        .get(vm_id as usize)
        .and_then(|slot| {
            slot.lock()
                .as_ref()
                // VMX controls must remain callable while a terminal lease is
                // parked in Shell2. Ordinary Blueprint command passthrough is
                // still gated by `blueprint_app_command_passthrough_enabled`.
                .filter(|context| context.console_attached)
                .map(|_| ())
        })
        .is_none()
    {
        return false;
    }
    blueprint_control_shell_command(vm_id, line);
    true
}

pub(crate) fn notify_blueprint_console_input(vm_id: u8) {
    if let Some(signal) = BLUEPRINT_CONSOLE_INPUT_READY.get(vm_id as usize) {
        signal.signal(());
    }
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

pub(crate) fn blueprint_console_submit_stdin(vm_id: u8, data: &[u8]) -> usize {
    const MAX_CONSOLE_INPUT: usize = 64 * 1024;
    if data.is_empty() {
        return 0;
    }
    let Some(slot) = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize) else {
        return 0;
    };
    let mut guard = slot.lock();
    let Some(context) = guard.as_mut() else {
        return 0;
    };
    if !context.console_attached || context.terminal_lease.suppresses_terminal_output() {
        return 0;
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

pub(crate) fn blueprint_console_submit_text_app_line(vm_id: u8, line: &str) -> bool {
    let Some(slot) = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize) else {
        return false;
    };
    let mut guard = slot.lock();
    let Some(context) = guard.as_mut() else {
        return false;
    };
    if !context.console_attached
        || context.console_surface != BlueprintConsoleSurface::Text
        || !context.app_command_passthrough
    {
        return false;
    }
    const MAX_CONSOLE_INPUT: usize = 64 * 1024;
    for byte in line.bytes().chain(core::iter::once(b'\n')) {
        if context.console_input.len() >= MAX_CONSOLE_INPUT {
            let _ = context.console_input.pop_front();
        }
        context.console_input.push_back(byte);
    }
    drop(guard);
    notify_blueprint_console_input(vm_id);
    true
}

pub(crate) fn blueprint_console_read_byte(vm_id: u8) -> Option<u8> {
    let mut byte = [0u8; 1];
    (blueprint_console_read(vm_id, &mut byte) == 1).then_some(byte[0])
}

pub(crate) fn blueprint_console_read(vm_id: u8, out: &mut [u8]) -> usize {
    if out.is_empty() {
        return 0;
    }
    if let Some(slot) = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize) {
        let mut guard = slot.lock();
        if let Some(context) = guard.as_mut() {
            if !context.console_attached || context.terminal_lease.suppresses_terminal_output() {
                return 0;
            }
            if context.console_route.is_net_shell_direct() {
                drop(guard);
                return crate::shell2::backends::net_tcp::net_shell_direct_read(vm_id, out);
            }
            if blueprint_uses_local_terminal_handoff(
                context.console_surface,
                context.console_target.as_ref(),
            ) {
                let target = context.console_target.clone();
                let mut read = 0usize;
                while read < out.len() {
                    let Some(byte) = context.console_input.pop_front() else {
                        break;
                    };
                    out[read] = byte;
                    read += 1;
                }
                drop(guard);
                if read < out.len() {
                    read += target
                        .as_ref()
                        .map(|target| {
                            crate::shell2::read_matrix_target_terminal_handoff(
                                target,
                                vm_id,
                                &mut out[read..],
                            )
                        })
                        .unwrap_or(0);
                }
                return read;
            }
            let mut read = 0usize;
            while read < out.len() {
                let Some(byte) = context.console_input.pop_front() else {
                    break;
                };
                out[read] = byte;
                read += 1;
            }
            return read;
        }
    }
    0
}

pub(crate) fn blueprint_console_readable_len(vm_id: u8) -> usize {
    let snapshot = BLUEPRINT_PROCESS_CONTEXTS
        .get(vm_id as usize)
        .and_then(|slot| {
            let guard = slot.lock();
            let context = guard.as_ref()?;
            if !context.console_attached || context.terminal_lease.suppresses_terminal_output() {
                return None;
            }
            let local_target = blueprint_uses_local_terminal_handoff(
                context.console_surface,
                context.console_target.as_ref(),
            )
            .then(|| context.console_target.clone())
            .flatten();
            Some((
                context.console_input.len(),
                context.console_route.is_net_shell_direct(),
                local_target,
            ))
        });
    let Some((buffered, direct, local_target)) = snapshot else {
        return 0;
    };
    if direct {
        return crate::shell2::backends::net_tcp::net_shell_direct_readable_len(vm_id);
    }
    if let Some(target) = local_target.as_ref() {
        return buffered.saturating_add(
            crate::shell2::matrix_target_terminal_handoff_readable_len(target, vm_id),
        );
    }
    buffered
}

pub(crate) fn blueprint_console_print_line(vm_id: u8, line: &str) {
    let (target, suppress_terminal_output) = {
        let context = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize);
        context.and_then(|slot| {
            let guard = slot.lock();
            let context = guard.as_ref()?;
            context.console_attached.then(|| {
                (
                    context.console_target.clone(),
                    context.terminal_lease.suppresses_terminal_output(),
                )
            })
        })
    }
    .unwrap_or((None, false));
    if suppress_terminal_output {
        blueprint_console_text_lines(vm_id, None, line.as_bytes());
    } else if let Some(target) = target {
        crate::shell2::print_matrix_target_line(&target, line);
    }
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
    let mut cleanup = BlueprintTerminalCleanup::empty();
    let Some(transition_slot) = BLUEPRINT_TERMINAL_TRANSITIONS.get(vm_id as usize) else {
        return cleanup;
    };
    let _transition = transition_slot.lock();
    if let Some(slot) = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize) {
        let previous = slot.lock().take();
        if let Some(context) = previous {
            cleanup.context_present = true;
            if context.tui_demo.is_some()
                && let Some(target) = context.console_target.as_ref()
            {
                crate::shell2::clear_matrix_target_transient_lines(target);
            }
            let claiming_direct = match context.terminal_lease {
                BlueprintTerminalLeaseState::Claiming { direct, .. } => Some(direct),
                _ => None,
            };
            let prelease = matches!(context.terminal_lease, BlueprintTerminalLeaseState::Reserved);
            let ownership_may_be_inflight =
                !prelease && (context.console_attached || context.console_attach_inflight);
            if ownership_may_be_inflight
                && (context.console_route.is_net_shell_direct() || claiming_direct == Some(true))
            {
                cleanup.backend_release_expected = true;
                cleanup.backend_released =
                    crate::shell2::backends::net_tcp::release_net_shell_direct(vm_id);
            } else if ownership_may_be_inflight
                && !context.console_route.is_net_shell_direct()
                && (claiming_direct == Some(false)
                    || blueprint_uses_local_terminal_handoff(
                        context.console_surface,
                        context.console_target.as_ref(),
                    ))
                && let Some(target) = context.console_target.as_ref()
            {
                cleanup.backend_release_expected = true;
                cleanup.backend_released =
                    crate::shell2::release_matrix_target_terminal_handoff(target, vm_id);
            }
            if ownership_may_be_inflight && let Some(target) = context.console_target.as_ref() {
                cleanup.matrix_unbind_expected = true;
                cleanup.matrix_unbind_result =
                    Some(crate::shell2::unbind_matrix_target_vm(target, vm_id));
            }
        }
    }
    // Release an exact terminal handoff before closing its local session. The
    // close path clears the owner record, so reversing this order would turn a
    // real cleanup into an unobservable false failure and could drop its reset.
    let _ = crate::shell2::backends::session_pool::close_owner(vm_id);
    crate::std_abi_shim::reset_blueprint_process_state(vm_id);
    if let Some(log_slot) = BLUEPRINT_CONSOLE_LOG_BUFFERS.get(vm_id as usize) {
        let _ = log_slot.lock().take();
    }
    cleanup
}

fn suspend_blueprint_process_context(vm_id: u8) {
    let Some(slot) = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize) else {
        return;
    };
    let Some(transition_slot) = BLUEPRINT_TERMINAL_TRANSITIONS.get(vm_id as usize) else {
        return;
    };
    let _transition = transition_slot.lock();
    let detached = {
        let mut guard = slot.lock();
        let Some(context) = guard.as_mut() else {
            return;
        };
        if !context.console_attached {
            if context.console_attach_inflight {
                context.console_attach_generation =
                    context.console_attach_generation.wrapping_add(1).max(1);
                context.console_attach_inflight = false;
            }
            return;
        }
        let route = context.console_route;
        let surface = context.console_surface;
        let prelease = matches!(context.terminal_lease, BlueprintTerminalLeaseState::Reserved);
        let claiming_direct = match context.terminal_lease {
            BlueprintTerminalLeaseState::Claiming {
                ticket,
                epoch,
                direct,
            } => {
                context.terminal_lease =
                    BlueprintTerminalLeaseState::ReentryRequested { ticket, epoch };
                Some(direct)
            }
            BlueprintTerminalLeaseState::Releasing { epoch } => {
                context.console_surface = BlueprintConsoleSurface::Text;
                context.console_route = BlueprintConsoleRoute::Matrix;
                context.terminal_lease = BlueprintTerminalLeaseState::Parked { ticket: epoch };
                None
            }
            _ => None,
        };
        context.console_attached = false;
        context.console_attach_inflight = false;
        (route, surface, context.console_target.clone(), claiming_direct, prelease)
    };
    let mut cleanup = BlueprintTerminalCleanup::empty();
    cleanup.context_present = true;
    if !detached.4 && (detached.0.is_net_shell_direct() || detached.3 == Some(true)) {
        cleanup.backend_release_expected = true;
        cleanup.backend_released =
            crate::shell2::backends::net_tcp::release_net_shell_direct(vm_id);
    } else if !detached.4
        && let Some(target) = detached.2.as_ref()
    {
        if detached.3 == Some(false)
            || blueprint_uses_local_terminal_handoff(detached.1, Some(target))
        {
            cleanup.backend_release_expected = true;
            cleanup.backend_released =
                crate::shell2::release_matrix_target_terminal_handoff(target, vm_id);
        }
        cleanup.matrix_unbind_expected = true;
        cleanup.matrix_unbind_result = Some(crate::shell2::unbind_matrix_target_vm(target, vm_id));
    }
    crate::log_os::blueprint_important_line(format_args!(
        "terminal-lifecycle: vm={} phase=terminal-cleanup state=retained owner_returned={} backend_expected={} backend_released={} matrix_expected={} matrix_result={}\n",
        vm_id,
        cleanup.complete() as u8,
        cleanup.backend_release_expected as u8,
        cleanup.backend_released as u8,
        cleanup.matrix_unbind_expected as u8,
        cleanup.matrix_unbind_marker(),
    ));
}

fn resume_blueprint_process_context(vm_id: u8) {
    let Some(slot) = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize) else {
        return;
    };
    let Some(transition_slot) = BLUEPRINT_TERMINAL_TRANSITIONS.get(vm_id as usize) else {
        return;
    };
    let _transition = transition_slot.lock();
    let presentation = {
        let mut guard = slot.lock();
        let Some(context) = guard.as_mut() else {
            return;
        };
        if context.console_attached || context.console_attach_inflight {
            return;
        }
        context.console_attach_generation =
            context.console_attach_generation.wrapping_add(1).max(1);
        context.console_attach_inflight = true;
        (
            context.console_attach_generation,
            context.console_route,
            context.console_surface,
            context.console_target.clone(),
            context.terminal_lease,
        )
    };
    // A pre-lease terminal has deliberately never owned a backend or Matrix
    // input route.  Reattaching a retained VM must preserve that fact; the
    // guest's initial typed lease call remains the only activation point.
    let (attached, backend_claimed, matrix_bound) =
        if matches!(presentation.4, BlueprintTerminalLeaseState::Reserved) {
            (true, false, false)
        } else if presentation.1.is_net_shell_direct() {
            let claimed = crate::shell2::backends::net_tcp::claim_net_shell_direct(vm_id);
            (claimed, claimed, false)
        } else {
            let local_handoff =
                blueprint_uses_local_terminal_handoff(presentation.2, presentation.3.as_ref());
            let claimed = !local_handoff
                || presentation.3.as_ref().is_some_and(|target| {
                    crate::shell2::claim_matrix_target_terminal_handoff(target, vm_id)
                });
            let bound = claimed
                && presentation
                    .3
                    .as_ref()
                    .map(|target| {
                        if presentation.2.is_terminal() {
                            crate::shell2::bind_matrix_target_vm_input(target, vm_id)
                        } else {
                            crate::shell2::bind_matrix_target_vm(target, vm_id)
                        }
                    })
                    .unwrap_or(true);
            if claimed
                && !bound
                && local_handoff
                && let Some(target) = presentation.3.as_ref()
            {
                let _ = crate::shell2::release_matrix_target_terminal_handoff(target, vm_id);
            }
            (bound, bound && local_handoff, bound && presentation.3.is_some())
        };

    if !attached {
        let mut guard = slot.lock();
        if let Some(context) = guard.as_mut()
            && context.console_attach_inflight
            && context.console_attach_generation == presentation.0
        {
            context.console_attach_inflight = false;
        }
        hvwarnf(format_args!(
            "hv: vm{} lifecycle: retained console reattach pending (route busy)",
            vm_id
        ));
        return;
    }

    let committed = {
        let mut guard = slot.lock();
        if let Some(context) = guard.as_mut() {
            let target_matches = match (context.console_target.as_ref(), presentation.3.as_ref()) {
                (None, None) => true,
                (Some(current), Some(expected)) => {
                    crate::shell2::matrix_targets_same_slot_lifetime(current, expected)
                }
                _ => false,
            };
            if !context.console_attached
                && context.console_attach_inflight
                && context.console_attach_generation == presentation.0
                && context.console_route == presentation.1
                && context.console_surface == presentation.2
                && target_matches
                && context.terminal_lease == presentation.4
            {
                context.console_attached = true;
                context.console_attach_inflight = false;
                if context.console_surface.is_terminal()
                    && !context.console_route.is_net_shell_direct()
                {
                    context.terminal_surface_generation =
                        context.terminal_surface_generation.saturating_add(1).max(1);
                }
                true
            } else {
                false
            }
        } else {
            false
        }
    };
    if committed {
        return;
    }

    // A clear/suspend/remutation won after the external claim. Roll back the
    // exact resources acquired from the detached snapshot; never attach them
    // to whatever context might now occupy the VM slot.
    if backend_claimed {
        if presentation.1.is_net_shell_direct() {
            let _ = crate::shell2::backends::net_tcp::release_net_shell_direct(vm_id);
        } else if let Some(target) = presentation.3.as_ref() {
            let _ = crate::shell2::release_matrix_target_terminal_handoff(target, vm_id);
        }
    }
    if matrix_bound && let Some(target) = presentation.3.as_ref() {
        let _ = crate::shell2::unbind_matrix_target_vm(target, vm_id);
    }
    // A non-lifecycle remutation (for example a concurrent readiness update)
    // can still invalidate the snapshot while this transition gate is held.
    // Clear only our own single-flight marker after its resources are gone so
    // a later resume can retry cleanly.
    {
        let mut guard = slot.lock();
        if let Some(context) = guard.as_mut()
            && context.console_attach_inflight
            && context.console_attach_generation == presentation.0
        {
            context.console_attach_inflight = false;
        }
    }
    hvwarnf(format_args!("hv: vm{} lifecycle: discarded stale console reattach", vm_id));
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
    vm.starting.store(false, Ordering::Release);
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
        vm.running.store(false, Ordering::Release);
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
        vm_id, GUEST_FS_BASE_RESET
    ));
    crate::log!(
        "app-vm-run-queue: vm launch enter vm={} mode={:?} stack_mib={}\n",
        vm_id,
        boot_mode,
        memory::active_guest_stack_mb_for_vm(vm_id)
    );
    let launch_result = vmx_launch_once_with_ept(lineage_record).await;
    if let Ok(lr) = launch_result {
        capture_snapshot_meta(vm_id, lr);
    }
    crate::log!("app-vm-run-queue: vm launch returned vm={} mode={:?}\n", vm_id, boot_mode);
    clear_current_vm_id();
    let mut pending_crash = None;
    let clean_exit = vm.clean_exit.swap(false, Ordering::AcqRel);
    crate::allocators::with_host_alloc_domain_strong(|| match launch_result {
        Ok(lr) => {
            let preserve_exit = vmexit_is_preserve(vm_id, lr);
            if preserve_exit {
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
        let gridpaper_released = crate::r::gridpaper_service::release_owner_lifecycle(vm_id);
        if gridpaper_released != 0 {
            hvlogf(format_args!(
                "hv: vm{} lifecycle: gridpaper cleanup released={}",
                vm_id, gridpaper_released
            ));
        }
        let media_released = crate::r::media_service::release_vm(vm_id);
        if media_released != 0 {
            hvlogf(format_args!(
                "hv: vm{} lifecycle: vmedia cleanup released_operations={}",
                vm_id, media_released
            ));
        }
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
        let cursors = crate::r::mouse_motion_service::release_principal(
            crate::r::mouse_motion_service::MouseControlPrincipal::Vm(vm_id),
        );
        let keyboards = crate::r::keyboard_control_service::release_principal(
            crate::r::keyboard_control_service::KeyboardControlPrincipal::Vm(vm_id),
        );
        let gamepads = crate::r::gamepad_control_service::release_principal(
            crate::r::gamepad_control_service::GamepadControlPrincipal::Vm(vm_id),
        );
        if cursors != 0 || keyboards != 0 || gamepads != 0 {
            hvlogf(format_args!(
                "hv: vm{} lifecycle: virtual-input cleanup cursors={} keyboards={} gamepads={}",
                vm_id, cursors, keyboards, gamepads,
            ));
        }
    }

    let (vgpu_released, vgpu_quarantined, vgpu_epoch) = crate::gpu::vgpu::release_hull_guest(vm_id);
    if vgpu_released != 0 || vgpu_quarantined != 0 {
        hvlogf(format_args!(
            "hv: vm{} lifecycle: vgpu cleanup released={} quarantined={} epoch={}",
            vm_id, vgpu_released, vgpu_quarantined, vgpu_epoch
        ));
    }

    let blueprint_net_closed = crate::hv::blueprint_net::release_vm(vm_id);
    let mio_closed = crate::mio_compat::close_sockets_for_vm(vm_id);
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
    vm.starting.store(false, Ordering::Release);
    vm.stop_req.store(false, Ordering::Release);
    vm.preserve_req.store(false, Ordering::Release);
    vm.preserve_exit.store(false, Ordering::Release);
    vm.clean_exit.store(false, Ordering::Release);
    if let Some(pending) = pending_crash {
        crate::hv::app_crash::write(vm_id, pending).await;
    }
    hvlogf(format_args!("hv: vm{} lifecycle: stopped", vm_id));
    // Publish the VM slot as reusable only after its carrier lease is free.
    // This prevents F2/start from queueing a second Hull behind teardown on
    // the same AP while reporting the first VM as already offline.
    lane_lease.release_now();
    vm.running.store(false, Ordering::Release);
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
) -> Result<LaunchResult, &'static str> {
    let vm_id = current_vm_id().ok_or("vm context missing")?;
    let vm = vm_slot(vm_id);
    if !current_vmx_root_active()? {
        hvlogf(format_args!(
            "hv: vm{} reporting: vmx launch aborted: core contract not active slot={}",
            current_vm_id_for_log(),
            current_vmx_slot().unwrap_or(usize::MAX)
        ));
        return Err("vmx core contract inactive");
    }

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
    let reset_vmcall_transport = active_restore_meta(vm_id).is_none();
    if !crate::hv::vmcall::prepare_for_vm(vm_id, reset_vmcall_transport) {
        return Err("vmcall comm page");
    }
    let preemption_timer_enabled =
        setup_vmcs_for_launch(vm_id, eptp, lineage_record, boot_mode_for_vm(vm_id))?;
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
    crate::log!("app-vm-run-queue: vmcs ready vm={} entry=0x{:016X}\n", vm_id, guest_launch_rip());

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
        if vm
            .map(|vm| vm.stop_req.load(Ordering::Acquire))
            .unwrap_or(false)
        {
            hvlogf(format_args!(
                "hv: vm{} reporting: host stop request consumed before guest entry/resume",
                vm_id
            ));
            break;
        }

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
            hvlogf(format_args!(
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
                            Timer::after(EmbassyDuration::from_millis(1)).await;
                            set_current_vm_id(vm_id);
                            break 'vmcall;
                        }
                        crate::hv::vmcall::DispatchOutcome::SleepMs(ms) => {
                            clear_current_vm_id();
                            if ms == 0 {
                                Timer::after(EmbassyDuration::from_millis(1)).await;
                            } else {
                                Timer::after(EmbassyDuration::from_millis(ms)).await;
                            }
                            set_current_vm_id(vm_id);
                            break 'vmcall;
                        }
                        crate::hv::vmcall::DispatchOutcome::WaitConsoleInput {
                            seq,
                            timeout_ms,
                        } => {
                            clear_current_vm_id();
                            let woke = wait_blueprint_console_input(vm_id, timeout_ms).await;
                            set_current_vm_id(vm_id);
                            crate::hv::vmcall::complete_console_input_wait(vm_id, seq, woke);
                            break 'vmcall;
                        }
                        crate::hv::vmcall::DispatchOutcome::RetryAfterMs(ms) => {
                            clear_current_vm_id();
                            Timer::after(EmbassyDuration::from_millis(ms.max(1))).await;
                            set_current_vm_id(vm_id);
                            crate::smp::poll();
                            if vm
                                .map(|vm| vm.stop_req.load(Ordering::Acquire))
                                .unwrap_or(false)
                            {
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
        if vm
            .map(|vm| vm.stop_req.load(Ordering::Acquire))
            .unwrap_or(false)
        {
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
    if !preserve_requested {
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

fn setup_vmcs_for_launch(
    vm_id: u8,
    eptp: u64,
    lineage_record: LineageRecord,
    boot_mode: VmBootMode,
) -> Result<bool, &'static str> {
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
    let proc = crate::hv::vmx::adjust_vmx_ctrl(
        proc_msr,
        PROC_BASED_HLT_EXITING
            | PROC_BASED_PAUSE_EXITING
            | PROC_BASED_ACTIVATE_SECONDARY
            | PROC_BASED_USE_TSC_OFFSETTING,
    );
    let proc2 = crate::hv::vmx::adjust_vmx_ctrl(
        crate::hv::vmx::IA32_VMX_PROCBASED_CTLS2,
        PROC2_BASED_ENABLE_EPT | PROC2_BASED_ENABLE_VMFUNC,
    );
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
    let entry = crate::hv::vmx::adjust_vmx_ctrl(entry_msr, ENTRY_CTL_IA32E_MODE_GUEST);
    hvlogf(format_args!(
        "hv: vm{}-{} reporting: vmcs controls pin=0x{:08X} proc=0x{:08X} proc2=0x{:08X} exit=0x{:08X} entry=0x{:08X}",
        current_vm_id_for_log(),
        lineage_record.level,
        pin as u32,
        proc as u32,
        proc2 as u32,
        exit as u32,
        entry as u32
    ));

    if (proc & PROC_BASED_ACTIVATE_SECONDARY) == 0 {
        hvwarnf(format_args!(
            "hv: vm{}-{} reporting: vmcs ctrl unsupported: primary bit ACTIVATE_SECONDARY not available",
            current_vm_id_for_log(),
            lineage_record.level
        ));
        return Err("secondary controls unsupported");
    }
    if (proc & PROC_BASED_PAUSE_EXITING) == 0 {
        hvwarnf(format_args!(
            "hv: vm{}-{} reporting: vmcs ctrl unsupported: primary bit PAUSE_EXITING not available",
            current_vm_id_for_log(),
            lineage_record.level
        ));
    }
    let preemption_timer_enabled = (pin & PIN_BASED_VMX_PREEMPTION_TIMER) != 0;
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

    vmwrite(VMCS_CTRL_PIN_BASED, pin)?;
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
            let guest_fs_base = GUEST_FS_BASE_RESET;
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
            hvlogf(format_args!(
                "hv: vm{}-{} reporting: host-state cs=0x{:04X} ss=0x{:04X} tr=0x{:04X} tr_base=0x{:016X}",
                current_vm_id_for_log(),
                lineage_record.level,
                host_cs as u16,
                host_ss as u16,
                host_tr as u16,
                tr_base
            ));

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
                .unwrap_or_else(guest_launch_rip);
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

            let cs = host_cs;
            let ss = host_ss;
            let ds = host_ds;
            let es = host_es;
            let fs = host_fs;
            let gs = host_gs;
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

            vmwrite(VMCS_GUEST_CS_AR, 0xA09B)?;
            vmwrite(VMCS_GUEST_SS_AR, 0xC093)?;
            vmwrite(VMCS_GUEST_DS_AR, 0xC093)?;
            vmwrite(VMCS_GUEST_ES_AR, 0xC093)?;
            vmwrite(VMCS_GUEST_FS_AR, 0x10000)?;
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
    let guest_fs_base = GUEST_FS_BASE_RESET;
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
    hvlogf(format_args!(
        "hv: vm{}-{} reporting: host-state cs=0x{:04X} ss=0x{:04X} tr=0x{:04X} tr_base=0x{:016X}",
        current_vm_id_for_log(),
        lineage_record.level,
        host_cs as u16,
        host_ss as u16,
        host_tr as u16,
        tr_base
    ));

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
        .unwrap_or_else(guest_launch_rip);
    let guest_rsp = restored
        .map(|m| m.guest_rsp)
        .unwrap_or_else(|| guest_stack_top_for_vm(vm_id));
    // Never reuse physical addresses embedded in the stored page tables. The
    // stack was copied into a new arena, so rebuild mappings for current VM
    // backings while preserving the checkpoint's logical execution state.
    let guest_cr3 = build_guest_cr3_for_vm(vm_id, guest_rip, guest_rsp)?;
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

    let cs = CS::get_reg().0 as u64;
    let ss = SS::get_reg().0 as u64;
    let ds = DS::get_reg().0 as u64;
    let es = ES::get_reg().0 as u64;
    let fs = FS::get_reg().0 as u64;
    let gs = GS::get_reg().0 as u64;
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

    vmwrite(VMCS_GUEST_CS_AR, 0xA09B)?;
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
