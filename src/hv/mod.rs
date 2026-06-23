pub mod app_crash;
pub mod blueprint;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod blueprint_net;
pub mod guest_run;
pub mod guest_work;
pub mod hv_remote_restore_service;
pub mod lane;
pub mod memory;
pub mod security;
pub mod snapshot;
pub mod store;
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

use embassy_executor::{Spawner, task};
use embassy_time::{Duration as EmbassyDuration, Timer};
use heapless::String;
use spin::Mutex;
use x86_64::instructions::tables::{sgdt, sidt};
use x86_64::registers::control::{Cr0, Cr0Flags, Cr3, Cr4, Cr4Flags};
use x86_64::registers::model_specific::Msr;
use x86_64::registers::rflags;
use x86_64::registers::segmentation::{CS, DS, ES, FS, GS, SS, Segment};

use crate::shell2::{MatrixTarget, ShellBackend2};

use guest_work::{VmLaneProfile, pick_vm_hull_lane};
use memory::*;
use snapshot::*;
const MAIN_LOOP_MARKER: &[u8] = b"main: entering executor loop";
const VMX_PAGE_SIZE: usize = 4096;
const MIB: usize = 1024 * 1024;
const HV_LOG_LINE: usize = crate::allcaps::hv::LOG_LINE_BYTES;
pub const TRUEOS_VM_ID_LIMIT: usize = crate::allcaps::hv::VM_ID_LIMIT;
const TRUEOS_VM_CPU_SLOT_LIMIT: usize = crate::allcaps::hv::VM_CPU_SLOT_LIMIT;

struct TrueosVmId {
    running: AtomicBool,
    starting: AtomicBool,
    stop_req: AtomicBool,
    preserve_req: AtomicBool,
    preserve_exit: AtomicBool,
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
static HV_CONTROL_NUDGE_SEQ: AtomicU64 = AtomicU64::new(1);
static VM_BOOT_MODES: [Mutex<VmBootMode>; TRUEOS_VM_ID_LIMIT] =
    [const { Mutex::new(VmBootMode::Hull) }; TRUEOS_VM_ID_LIMIT];
static BLUEPRINT_PENDING_LAUNCH_STATES: [Mutex<Option<BlueprintPendingLaunchState>>;
    TRUEOS_VM_ID_LIMIT] = [const { Mutex::new(None) }; TRUEOS_VM_ID_LIMIT];
static BLUEPRINT_LAUNCH_STATES: [Mutex<Option<BlueprintLaunchState>>; TRUEOS_VM_ID_LIMIT] =
    [const { Mutex::new(None) }; TRUEOS_VM_ID_LIMIT];
static BLUEPRINT_PROCESS_CONTEXTS: [Mutex<Option<BlueprintProcessContext>>; TRUEOS_VM_ID_LIMIT] =
    [const { Mutex::new(None) }; TRUEOS_VM_ID_LIMIT];
static BLUEPRINT_CONSOLE_LOG_BUFFERS: [Mutex<Option<AllocString>>; TRUEOS_VM_ID_LIMIT] =
    [const { Mutex::new(None) }; TRUEOS_VM_ID_LIMIT];

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

    VMX_ROOT_ACTIVE_BY_CPU[slot].store(true, Ordering::Release);
    if slot >= 2 {
        crate::r::readiness::set(crate::r::readiness::VTHREAD_HW_TAG_READY);
    }
    hvlogf(format_args!(
        "hv: vmx core-contract active slot={} revision=0x{:08X} vmxon_pa=0x{:016X}",
        slot, revision, vmxon_pa
    ));
    Ok(())
}

fn hv_control_nudge_ap(arg: u64) -> u64 {
    let vm_id = arg as u8;
    let Some(vm) = vm_slot(vm_id) else {
        return 0;
    };
    let current = current_vm_id();
    let has_request = vm.stop_req.load(Ordering::Acquire)
        || vm.preserve_req.load(Ordering::Acquire)
        || vm.preserve_exit.load(Ordering::Acquire);
    let on_vm_lane = current == Some(vm_id);
    ((on_vm_lane as u64) << 1) | has_request as u64
}

fn nudge_vm_control(vm_id: u8, reason: &'static str) {
    let seq = HV_CONTROL_NUDGE_SEQ.fetch_add(1, Ordering::Relaxed);
    let report = crate::smp::submit_to_all_online_aps(hv_control_nudge_ap, vm_id as u64);
    hvlogf(format_args!(
        "hv: vm{} lifecycle: {} nudge soft-smp seq={} smp_seq={} targeted={} submitted={} busy={}",
        vm_id, reason, seq, report.seq, report.targeted_aps, report.submitted_aps, report.busy_aps
    ));
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum StartError {
    UnsupportedVmId,
    AlreadyRunning,
    VmxUnsupported,
    MissingGuestModule,
    GuestMemoryUnavailable,
    NoVmSpawner,
    SpawnFailed,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum VmBootMode {
    Hull,
    Full,
}

#[derive(Copy, Clone)]
enum BlueprintMemoryClass {
    TokioRuntime,
    NetworkClient,
    ServerRuntime,
    HeavyGraphics,
    Unknown,
}

impl BlueprintMemoryClass {
    const fn label(self) -> &'static str {
        match self {
            Self::TokioRuntime => "tokio-runtime",
            Self::NetworkClient => "network-client",
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
    console_target: Option<MatrixTarget>,
}

#[derive(Clone)]
pub struct BlueprintLaunchState {
    pub archive: AllocString,
    pub module_bytes: AllocVec<u8>,
    pub unpacked_bytes: AllocVec<u8>,
    pub app_args: AllocVec<AllocString>,
}

#[derive(Clone)]
pub(crate) struct BlueprintProcessContext {
    args: AllocVec<AllocString>,
    vars: BTreeMap<AllocString, AllocString>,
    console_target: Option<MatrixTarget>,
    console_input: VecDeque<u8>,
    control_shell_line: AllocVec<u8>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum StopError {
    UnsupportedVmId,
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
    pub id: u8,
    pub supported: bool,
    pub running: bool,
    pub starting: bool,
    pub stop_requested: bool,
    pub preserve_requested: bool,
    pub preserve_exit: bool,
}

#[inline]
fn current_vm_id_for_log() -> u8 {
    current_vm_id().unwrap_or(0)
}

#[inline]
fn vm_slot(vm_id: u8) -> Option<&'static TrueosVmId> {
    trueos_vm_ids.get(vm_id as usize)
}

pub fn first_free_vm_id() -> Option<u8> {
    for (idx, slot) in trueos_vm_ids.iter().enumerate() {
        if !slot.running.load(Ordering::Acquire) && !slot.starting.load(Ordering::Acquire) {
            return Some(idx as u8);
        }
    }
    None
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
        };
    };
    HvVmState {
        id: vm_id,
        supported: true,
        running: vm.running.load(Ordering::Acquire),
        starting: vm.starting.load(Ordering::Acquire),
        stop_requested: vm.stop_req.load(Ordering::Acquire),
        preserve_requested: vm.preserve_req.load(Ordering::Acquire),
        preserve_exit: vm.preserve_exit.load(Ordering::Acquire),
    }
}

pub fn app_vm_archive(vm_id: u8) -> Option<AllocString> {
    if vm_slot(vm_id).is_none() {
        return None;
    }
    blueprint_launch_snapshot(vm_id).map(|state| state.archive)
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
    ) && let Some(vm_id) = domain.vm_id
    {
        return Some(vm_id);
    }

    let snapshot = crate::stackkeeper::current_vm_hull_snapshot()?;
    if snapshot.role != crate::stackkeeper::VM_HULL_RECORD_ROLE_VM_HULL {
        return None;
    }

    let vm_id = snapshot.lane_id as usize;
    if vm_id < crate::allcaps::hv::VM_ID_LIMIT {
        Some(vm_id as u8)
    } else {
        None
    }
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
    let mut line: String<HV_LOG_LINE> = String::new();
    let _ = line.write_fmt(args);
    if line.is_empty() {
        return;
    }

    let level = hvlog_console_level(line.as_str());
    if current_hull_guest_context_vm_id().is_some() {
        if !hvlog_console_enabled(line.as_str(), level) {
            return;
        }
        for &b in line.as_bytes() {
            crate::globalog::debugcon_write_byte_raw(b);
        }
        crate::globalog::debugcon_write_byte_raw(b'\n');
        let _ = trueos_vm::vmcall::net_tcp_write(line.as_bytes());
        let _ = trueos_vm::vmcall::net_tcp_write(b"\n");
        return;
    }

    if hvlog_console_enabled(line.as_str(), level) {
        crate::globalog::log_with_concept_level("hv", level, format_args!("{}\n", line.as_str()));
    }
}

fn hvlog_console_level(line: &str) -> log::Level {
    if line.contains("failed")
        || line.contains("error")
        || line.contains("fault")
        || line.contains("panic")
        || line.contains("unsupported")
        || line.contains("bad ")
    {
        log::Level::Warn
    } else {
        log::Level::Info
    }
}

fn hvlog_console_enabled(line: &str, level: log::Level) -> bool {
    if line.starts_with("portal:") {
        return crate::logflag::PORTAL_LOGS;
    }
    crate::logflag::HV_LOGS && crate::logflag::concept_log_enabled("hv", level)
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

pub fn start(
    vm_id: u8,
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    stack_mb: Option<usize>,
) -> Result<(), StartError> {
    start_with_mode(vm_id, spawner, io, VmBootMode::Hull, stack_mb, None)
}

pub fn start_blueprint_app_vm(
    vm_id: u8,
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    archive: AllocString,
    module_bytes: AllocVec<u8>,
    app_args: AllocVec<AllocString>,
    console_target: Option<MatrixTarget>,
) -> Result<(), StartError> {
    start_with_mode(
        vm_id,
        spawner,
        io,
        VmBootMode::Hull,
        None,
        Some(BlueprintPendingLaunchState {
            archive,
            module_bytes,
            app_args,
            console_target,
        }),
    )
}

fn start_with_mode(
    vm_id: u8,
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    boot_mode: VmBootMode,
    stack_mb: Option<usize>,
    pending_blueprint: Option<BlueprintPendingLaunchState>,
) -> Result<(), StartError> {
    let Some(vm) = vm_slot(vm_id) else {
        return Err(StartError::UnsupportedVmId);
    };

    if vm.running.load(Ordering::Acquire)
        || vm
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
        hvlogf(format_args!(
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

    let requested_stack_mb = stack_mb.unwrap_or(memory::guest_stack_default_mb());
    let active_stack_mb = memory::clamp_guest_stack_mb(requested_stack_mb);
    if memory::prepare_guest_stack_mb_for_vm(vm_id, active_stack_mb).is_err() {
        vm.starting.store(false, Ordering::Release);
        return Err(StartError::GuestMemoryUnavailable);
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

    let _ = spawner;
    let _ = io;
    let profile = VmLaneProfile::vm_default();
    let mut target = match pick_vm_hull_lane() {
        Ok(target) => target,
        Err(error) => {
            clear_blueprint_pending_launch(vm_id);
            vm.starting.store(false, Ordering::Release);
            hvlogf(format_args!(
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
    // and never on the AP1 UI2/service lane.
    if !profile.requires_reserved_vm_lane() || !target.supports(profile) {
        clear_blueprint_pending_launch(vm_id);
        vm.starting.store(false, Ordering::Release);
        hvlogf(format_args!(
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
    target.lease.set_vm_owner(vm_id);

    match vm_task(vm_id, target.lease) {
        Ok(token) => {
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
        vm.stop_req.store(true, Ordering::Release);
        hvlogf(format_args!("hv: vm{} lifecycle: stop requested", vm_id));
        nudge_vm_control(vm_id, "stop");
        Ok(true)
    } else {
        hvlogf(format_args!("hv: vm{} lifecycle: stop ignored (not running)", vm_id));
        Ok(false)
    }
}

pub fn request_preserve(vm_id: u8) -> Result<bool, StopError> {
    let Some(vm) = vm_slot(vm_id) else {
        return Err(StopError::UnsupportedVmId);
    };

    let running = vm.running.load(Ordering::Acquire);
    let starting = vm.starting.load(Ordering::Acquire);
    if !running && !starting {
        hvlogf(format_args!("hv: vm{} lifecycle: preserve ignored (not running)", vm_id));
        return Ok(false);
    }

    vm.preserve_req.store(true, Ordering::Release);
    hvlogf(format_args!("hv: vm{} lifecycle: preserve requested", vm_id));
    nudge_vm_control(vm_id, "preserve");
    Ok(true)
}

pub fn save_snapshot(vm_id: u8) -> Result<usize, SaveError> {
    if vm_slot(vm_id).is_none() {
        return Err(SaveError::UnsupportedVmId);
    }

    let bytes = snapshot_bytes(vm_id)?;
    crate::hv::store::save_bytes(vm_id, bytes).map_err(map_store_save_error)
}

pub fn restore_snapshot(vm_id: u8) -> Result<usize, RestoreError> {
    if vm_slot(vm_id).is_none() {
        return Err(RestoreError::UnsupportedVmId);
    }

    let bytes = crate::hv::store::load_bytes(vm_id).map_err(map_store_restore_error)?;

    restore_snapshot_bytes(vm_id, bytes.as_slice())?;
    Ok(bytes.len())
}

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
    }
}

fn vmexit_is_preserve(lr: LaunchResult) -> bool {
    let vm_id = current_vm_id_for_log();
    lr.entered != 0
        && lr.launch_failed == 0
        && ((lr.exit_reason & 0xFFFF) == VMEXIT_REASON_VMCALL
            || vm_slot(vm_id)
                .map(|vm| vm.preserve_exit.load(Ordering::Acquire))
                .unwrap_or(false))
}

fn snapshot_on_preserve_exit(vm_id: u8) {
    match snapshot_bytes(vm_id) {
        Ok(bytes) => match crate::hv::store::save_bytes(vm_id, bytes) {
            Ok(saved) => hvlogf(format_args!(
                "hv: vm{} reporting: preserve snapshot saved store=hv-ramdisk path={} bytes={}",
                vm_id,
                snapshot_path(vm_id).as_str(),
                saved
            )),
            Err(e) => hvlogf(format_args!(
                "hv: vm{} reporting: preserve snapshot save failed ({:?})",
                vm_id, e
            )),
        },
        Err(e) => hvlogf(format_args!(
            "hv: vm{} reporting: preserve snapshot bytes failed ({:?})",
            vm_id, e
        )),
    }
}

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

fn archive_has(archive: &str, needle: &str) -> bool {
    archive.contains(needle)
}

fn classify_blueprint_memory(
    archive: &str,
    raw_payload_len: usize,
    stats: crate::hv::blueprint::ElfAllocStats,
    imports: &[crate::hv::blueprint::ElfImport<'_>],
) -> BlueprintMemoryClass {
    let server_signal = archive_has(archive, "horizon")
        || archive_has(archive, "server")
        || archive_has(archive, "game")
        || import_name_has(imports, "pthread_");
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

    let tokio_signal = archive_has(archive, "tokio")
        || import_name_has(imports, "trueos_tokio_")
        || import_name_has(imports, "tokio_")
        || import_name_has(imports, "sleep_ms");
    if tokio_signal {
        return BlueprintMemoryClass::TokioRuntime;
    }

    let heavy_graphics_signal = archive_has(archive, "mandelbrot")
        || archive_has(archive, "shader")
        || archive_has(archive, "particle")
        || archive_has(archive, "virgl")
        || stats.alloc_bytes > 4 * MIB
        || raw_payload_len > 8 * MIB;
    if heavy_graphics_signal {
        return BlueprintMemoryClass::HeavyGraphics;
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
            BlueprintMemoryClass::NetworkClient => (
                64,
                round_pow2_mib(base_live_mib.saturating_mul(32).saturating_add(128)).max(512),
                512,
                8,
                16,
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
    crate::log_info!(
        target: "hv";
        "apps: profile {} heap={}/{}/{}MiB stack={}/{}/{}MiB\n",
        profile.class.label(),
        profile.heap_lower_mib,
        profile.heap_recommended_mib,
        profile.heap_upper_mib,
        profile.stack_lower_mib,
        profile.stack_recommended_mib,
        profile.stack_upper_mib
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

fn log_blueprint_launch_line(target: Option<&MatrixTarget>, args: core::fmt::Arguments<'_>) {
    let line = alloc::format!("{}", args);
    if let Some(target) = target {
        crate::shell2::print_matrix_target_line(target, line.as_str());
    }
    hvlogf(format_args!("{}", line.as_str()));
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
    let unpacked_bytes = crate::hv::blueprint::unpack_blueprint(&module)
        .map_err(|err| alloc::format!("app-vm unpack failed: {}", err))?;

    if !unpacked_bytes.starts_with(b"\x7fELF")
        || !matches!(crate::hv::blueprint::elf_type_name(unpacked_bytes.as_slice()), Some("REL"))
    {
        return Err(AllocString::from("only ELF REL blueprints are supported for app-vm launch"));
    }

    let imports = crate::hv::blueprint::elf_imports(unpacked_bytes.as_slice()).unwrap_or_default();
    let profile = estimate_blueprint_memory_profile(
        pending.archive.as_str(),
        &module,
        unpacked_bytes.as_slice(),
        imports.as_slice(),
    );
    log_blueprint_memory_profile_info(profile);
    drop(host_alloc_guard);

    if !crate::allocators::prepare_hv_guest_heap_for_vm(
        vm_id,
        profile.heap_recommended_mib.saturating_mul(MIB),
    ) {
        return Err(AllocString::from("app-vm heap profile allocation failed"));
    }
    if memory::prepare_guest_stack_mb_for_vm(vm_id, profile.stack_recommended_mib).is_err() {
        return Err(AllocString::from("app-vm stack profile allocation failed"));
    }

    stage_blueprint_launch(
        vm_id,
        BlueprintLaunchState {
            archive: pending.archive,
            module_bytes: pending.module_bytes,
            unpacked_bytes,
            app_args: pending.app_args,
        },
        pending.console_target,
    )
    .map_err(|err| alloc::format!("app-vm stage failed: {:?}", err))?;

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
) -> Result<(), StartError> {
    let Some(slot) = BLUEPRINT_LAUNCH_STATES.get(vm_id as usize) else {
        return Err(StartError::UnsupportedVmId);
    };
    let Some(process_slot) = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize) else {
        return Err(StartError::UnsupportedVmId);
    };
    let app_fs_root = crate::hv::blueprint::app_fs_root_for_archive(
        state.archive.as_str(),
        state.module_bytes.as_slice(),
    );
    let process_context = BlueprintProcessContext {
        args: crate::hv::blueprint::build_process_args(
            state.archive.as_str(),
            state.app_args.as_slice(),
        ),
        vars: crate::hv::blueprint::build_process_env(
            state.archive.as_str(),
            Some(app_fs_root.as_str()),
        ),
        console_target,
        console_input: VecDeque::new(),
        control_shell_line: AllocVec::new(),
    };
    let Some(guest_state) = crate::allocators::with_hv_guest_alloc_domain(vm_id, || state.clone())
    else {
        return Err(StartError::GuestMemoryUnavailable);
    };
    let console_target = process_context.console_target.clone();
    *slot.lock() = Some(guest_state);
    *process_slot.lock() = Some(process_context);
    if let Some(log_slot) = BLUEPRINT_CONSOLE_LOG_BUFFERS.get(vm_id as usize) {
        let _ = log_slot.lock().take();
    }
    if let Some(target) = console_target.as_ref() {
        crate::shell2::bind_matrix_target_vm_input(target, vm_id);
    }
    Ok(())
}

pub fn take_blueprint_launch(vm_id: u8) -> Option<BlueprintLaunchState> {
    BLUEPRINT_LAUNCH_STATES.get(vm_id as usize)?.lock().take()
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
    let horizon_worker_lane = blueprint_launch_snapshot(vm_id)
        .map(|state| archive_has(state.archive.as_str(), "horizon"))
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

pub(crate) fn blueprint_console_write(vm_id: u8, data: &[u8]) -> usize {
    let target = {
        let context = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize);
        context.and_then(|slot| slot.lock().as_ref()?.console_target.clone())
    };
    let written = blueprint_console_write_to_target(target.as_ref(), data);
    blueprint_console_mirror_global_log(vm_id, &data[..core::cmp::min(written, data.len())]);
    written
}

fn blueprint_console_write_to_target(target: Option<&MatrixTarget>, data: &[u8]) -> usize {
    if let Some(target) = target {
        return crate::shell2::raw_write_matrix_target(&target, data);
    }
    crate::shell2::uart1_com1::write_bytes(data);
    data.len()
}

fn blueprint_console_mirror_global_log(vm_id: u8, data: &[u8]) {
    if data.is_empty() {
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
        crate::globalog::log_with_purpose(
            Some("blueprint"),
            format_args!("vm{}: {}\n", vm_id, line.as_str()),
        );
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

fn blueprint_control_shell_command(vm_id: u8, raw: &str) {
    let trimmed = raw.trim();
    let mut words = trimmed.splitn(2, char::is_whitespace);
    let cmd = words.next().unwrap_or("");
    let rest = words.next().unwrap_or("").trim_start();
    match cmd {
        "" => {}
        "hostname" => {
            let hostname = blueprint_process_env_var(vm_id, "HOSTNAME")
                .or_else(|| blueprint_process_env_var(vm_id, "TRUEOS_HOSTNAME"))
                .unwrap_or_else(|| AllocString::from("TRUEOS"));
            blueprint_control_shell_line(vm_id, hostname.as_str());
        }
        "homedir" => {
            let home =
                blueprint_process_env_var(vm_id, "HOME").unwrap_or_else(|| AllocString::from("/"));
            blueprint_control_shell_line(vm_id, home.as_str());
        }
        "env" => match blueprint_process_env_text(vm_id) {
            Some(text) if !text.is_empty() => {
                blueprint_control_shell_write_text(vm_id, text.as_str())
            }
            _ => blueprint_control_shell_line(vm_id, "env: unavailable"),
        },
        "file" => {
            let path = if rest.is_empty() {
                blueprint_process_env_var(vm_id, "HOME").unwrap_or_else(|| AllocString::from("/"))
            } else {
                AllocString::from(rest)
            };
            match blueprint_process_file_tree_text(vm_id, path.as_str()) {
                Some(text) if !text.is_empty() => {
                    blueprint_control_shell_write_text(vm_id, text.as_str())
                }
                _ => blueprint_control_shell_line(vm_id, "file: unavailable"),
            }
        }
        "thread" => {
            let record = crate::stackkeeper::vm_hull_record(vm_id);
            let state = vm_state(vm_id);
            blueprint_control_shell_line(
                vm_id,
                alloc::format!(
                    "thread: vm={} vthread={} running={} starting={} async_jobs=not-wired",
                    vm_id,
                    record.vtid(),
                    state.running as u8,
                    state.starting as u8,
                )
                .as_str(),
            );
        }
        "help" => blueprint_control_shell_line(
            vm_id,
            "commands: hostname homedir env disc thread help stop pause preserve",
        ),
        "stop" => match stop(vm_id) {
            Ok(true) => blueprint_control_shell_line(vm_id, "vmx-shell: stop requested"),
            Ok(false) => blueprint_control_shell_line(vm_id, "vmx-shell: vm is not running"),
            Err(err) => blueprint_control_shell_line(
                vm_id,
                alloc::format!("vmx-shell: stop failed: {:?}", err).as_str(),
            ),
        },
        "pause" | "preserve" => match request_preserve(vm_id) {
            Ok(true) => blueprint_control_shell_line(vm_id, "vmx-shell: preserve requested"),
            Ok(false) => blueprint_control_shell_line(vm_id, "vmx-shell: vm is not running"),
            Err(err) => blueprint_control_shell_line(
                vm_id,
                alloc::format!("vmx-shell: preserve failed: {:?}", err).as_str(),
            ),
        },
        "detach" => blueprint_control_shell_line(vm_id, "vm: switch matrix slots with `§<slot>`"),
        _ => blueprint_control_shell_line(vm_id, "unknown command; try `help`"),
    }
}

pub(crate) fn blueprint_console_submit_control_line(vm_id: u8, line: &str) -> bool {
    if BLUEPRINT_PROCESS_CONTEXTS
        .get(vm_id as usize)
        .and_then(|slot| slot.lock().as_ref().map(|_| ()))
        .is_none()
    {
        return false;
    }
    blueprint_control_shell_command(vm_id, line);
    true
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
    for &byte in data {
        if context.console_input.len() >= MAX_CONSOLE_INPUT {
            let _ = context.console_input.pop_front();
        }
        context.console_input.push_back(byte);
    }
    data.len()
}

pub(crate) fn blueprint_console_submit_input(vm_id: u8, data: &[u8]) -> usize {
    const MAX_CONSOLE_INPUT: usize = 64 * 1024;
    if data.is_empty() {
        return 0;
    }
    let target = {
        let context = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize);
        context.and_then(|slot| slot.lock().as_ref()?.console_target.clone())
    };
    let Some(slot) = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize) else {
        return 0;
    };
    let mut guard = slot.lock();
    let Some(context) = guard.as_mut() else {
        return 0;
    };
    for &byte in data {
        match byte {
            b'\r' | b'\n' => {
                let _ = blueprint_console_write_to_target(target.as_ref(), b"\r\n");
                let line_bytes = core::mem::take(&mut context.control_shell_line);
                drop(guard);
                let line = core::str::from_utf8(line_bytes.as_slice()).unwrap_or("");
                blueprint_control_shell_command(vm_id, line);
                return data.len();
            }
            0x08 | 0x7f => {
                if context.control_shell_line.pop().is_some() {
                    let _ = blueprint_console_write_to_target(target.as_ref(), b"\x08 \x08");
                }
            }
            byte if byte.is_ascii_graphic() || byte == b' ' => {
                if context.control_shell_line.len() < 512 {
                    context.control_shell_line.push(byte);
                    let _ = blueprint_console_write_to_target(target.as_ref(), &[byte]);
                }
            }
            _ => {
                if context.console_input.len() >= MAX_CONSOLE_INPUT {
                    let _ = context.console_input.pop_front();
                }
                context.console_input.push_back(byte);
            }
        }
    }
    data.len()
}

pub(crate) fn blueprint_console_read_byte(vm_id: u8) -> Option<u8> {
    if let Some(slot) = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize) {
        let mut guard = slot.lock();
        if let Some(context) = guard.as_mut() {
            return context.console_input.pop_front();
        }
    }
    crate::shell2::uart1_com1::read_byte()
}

pub(crate) fn blueprint_console_print_line(vm_id: u8, line: &str) {
    let target = {
        let context = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize);
        context.and_then(|slot| slot.lock().as_ref()?.console_target.clone())
    };
    if let Some(target) = target {
        crate::shell2::print_matrix_target_line(&target, line);
    } else {
        crate::shell2::print_shell_line(&crate::shell2::UART1_COM1_BACKEND, line);
    }
}

pub(crate) fn blueprint_process_context(vm_id: u8) -> Option<BlueprintProcessContext> {
    BLUEPRINT_PROCESS_CONTEXTS
        .get(vm_id as usize)?
        .lock()
        .as_ref()
        .cloned()
}

fn clear_blueprint_process_context(vm_id: u8) {
    if let Some(log_slot) = BLUEPRINT_CONSOLE_LOG_BUFFERS.get(vm_id as usize) {
        let _ = log_slot.lock().take();
    }
    if let Some(slot) = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize) {
        let previous = slot.lock().take();
        if let Some(context) = previous
            && let Some(target) = context.console_target.as_ref()
        {
            crate::shell2::unbind_matrix_target_vm(target, vm_id);
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

#[task(pool_size = 32)]
async fn vm_task(vm_id: u8, _lane_lease: crate::hv::lane::LaneLease) {
    let Some(vm) = vm_slot(vm_id) else {
        return;
    };
    let lineage_record = LineageRecord::new();
    vm.starting.store(false, Ordering::Release);
    vm.running.store(true, Ordering::Release);
    vm.preserve_req.store(false, Ordering::Release);
    vm.preserve_exit.store(false, Ordering::Release);
    set_current_vm_id(vm_id);
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
                    hvlogf(format_args!(
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
        hvlogf(format_args!("hv: vm{} lifecycle: blueprint prep failed ({})", vm_id, err));
        clear_current_vm_id();
        vm.running.store(false, Ordering::Release);
        vm.starting.store(false, Ordering::Release);
        vm.stop_req.store(false, Ordering::Release);
        vm.preserve_req.store(false, Ordering::Release);
        vm.preserve_exit.store(false, Ordering::Release);
        clear_blueprint_process_context(vm_id);
        return;
    }
    hvlogf(format_args!("hv: vm{} reporting: vmx preflight ok, stage=m1", vm_id));
    hvlogf(format_args!("hv: vm{} reporting: vlayer policy=integrity-first", vm_id));
    if boot_mode == VmBootMode::Hull {
        if let Err(err) = memory::ensure_guest_hull_rw_template_ready() {
            hvlogf(format_args!(
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
        "hv: vm{} reporting: vm-hull fs_base=0x{:016X}",
        vm_id,
        crate::stackkeeper::vm_hull_fs_base(vm_id)
    ));
    crate::log!(
        "app-vm-run-queue: vm launch enter vm={} mode={:?} stack_mib={}\n",
        vm_id,
        boot_mode,
        memory::active_guest_stack_mb_for_vm(vm_id)
    );
    let launch_result = vmx_launch_once_with_ept(lineage_record).await;
    crate::log!("app-vm-run-queue: vm launch returned vm={} mode={:?}\n", vm_id, boot_mode);
    clear_current_vm_id();
    let blueprint_crash_state = blueprint_launch_snapshot(vm_id);
    let mut pending_crash = None;
    crate::allocators::with_host_alloc_domain_strong(|| match launch_result {
        Ok(lr) => {
            capture_snapshot_meta(vm_id, lr);
            let preserve_exit = vmexit_is_preserve(lr);
            if preserve_exit {
                snapshot_on_preserve_exit(vm_id);
            } else if let Some(state) = blueprint_crash_state.as_ref() {
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
            if let Some(state) = blueprint_crash_state.as_ref() {
                pending_crash = Some(crate::hv::app_crash::prepare(
                    vm_id,
                    state,
                    crate::hv::app_crash::CrashOutcome::LaunchError(e),
                ));
            }
            hvlogf(format_args!(
                "hv: vm{}-{} reporting: vmlaunch/ept failed ({})",
                vm_id, lineage_record.level, e
            ));
        }
    });

    if boot_mode == VmBootMode::Full {
        if let Some(bytes) = guest {
            if contains_bytes(bytes, MAIN_LOOP_MARKER) {
                vm.marker_seen.store(true, Ordering::Release);
                hvlogf(format_args!("hv: vm{} reporting: main: entering executor loop", vm_id));
            }
        }
    }

    vm.running.store(false, Ordering::Release);
    vm.starting.store(false, Ordering::Release);
    vm.stop_req.store(false, Ordering::Release);
    vm.preserve_req.store(false, Ordering::Release);
    vm.preserve_exit.store(false, Ordering::Release);
    clear_blueprint_pending_launch(vm_id);
    let _ = take_blueprint_launch(vm_id);
    clear_blueprint_process_context(vm_id);
    hvlogf(format_args!("hv: vm{} lifecycle: stopped", vm_id));
    if let Some(pending) = pending_crash {
        crate::hv::app_crash::write(vm_id, pending).await;
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
    if !crate::hv::vmcall::prepare_for_vm(vm_id) {
        return Err("vmcall comm page");
    }
    if let Err(e) = setup_vmcs_for_launch(vm_id, eptp, lineage_record, boot_mode_for_vm(vm_id)) {
        return Err(e);
    }
    crate::log!("app-vm-run-queue: vmcs ready vm={} entry=0x{:016X}\n", vm_id, guest_launch_rip());

    // ── vmexit dispatch loop ──────────────────────────────────────────────────
    let mut lr = LaunchResult::default();
    let mut preserve_requested = false;
    let mut cpuid_leaf0_count = 0u32;
    let mut cpuid_leaf80000000_count = 0u32;
    let mut cpuid_leaf1_count = 0u32;
    let mut cpuid_other_count = 0u32;
    let mut first = true;
    loop {
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

        if first {
            crate::log!("app-vm-run-queue: vmlaunch begin vm={}\n", vm_id);
            vmlaunch_once_wrapper(&mut lr);
            first = false;
        } else {
            vmresume_once_wrapper(&mut lr);
        }

        if lr.launch_failed != 0 {
            hvlogf(format_args!(
                "hv: vm{} reporting: vmlaunch/vmresume failed instr_err={} rip=0x{:016X}",
                current_vm_id_for_log(),
                lr.instr_err,
                crate::hv::vmx::current_rip()
            ));
            break;
        }
        if lr.entered == 0 {
            hvlogf(format_args!(
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
                hvlogf(format_args!(
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
            let memcpy_addr = trueos_qjs::trueos_shims::memcpy as *const () as usize as u64;
            if lr.guest_rip >= memcpy_addr && lr.guest_rip < memcpy_addr.saturating_add(128) {
                let (vector, vector_name, _, _, err) =
                    guest_exception.unwrap_or((0xFF, "unknown", 0, 0, intr_err));
                hvlogf(format_args!(
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
            VMEXIT_REASON_VMCALL => {
                let len = vmread(VMCS_VMEXIT_INSTRUCTION_LEN).ok_or("vmread instr len")?;
                vmwrite(VMCS_GUEST_RIP, lr.guest_rip + len)?;
                match crate::hv::vmcall::dispatch(vm_id) {
                    crate::hv::vmcall::DispatchOutcome::Resume => {}
                    crate::hv::vmcall::DispatchOutcome::Stop => break,
                    crate::hv::vmcall::DispatchOutcome::Yield => {
                        clear_current_vm_id();
                        Timer::after(EmbassyDuration::from_millis(1)).await;
                        set_current_vm_id(vm_id);
                    }
                    crate::hv::vmcall::DispatchOutcome::SleepMs(ms) => {
                        clear_current_vm_id();
                        if ms == 0 {
                            Timer::after(EmbassyDuration::from_millis(1)).await;
                        } else {
                            Timer::after(EmbassyDuration::from_millis(ms)).await;
                        }
                        set_current_vm_id(vm_id);
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
        hvlogf(format_args!(
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
        hvlogf(format_args!(
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
) -> Result<(), &'static str> {
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

    let pin = crate::hv::vmx::adjust_vmx_ctrl(pin_msr, 0);
    let proc = crate::hv::vmx::adjust_vmx_ctrl(
        proc_msr,
        PROC_BASED_HLT_EXITING
            | PROC_BASED_PAUSE_EXITING
            | PROC_BASED_ACTIVATE_SECONDARY
            | PROC_BASED_VMX_PREEMPTION_TIMER
            | PROC_BASED_USE_TSC_OFFSETTING,
    );
    let proc2 = crate::hv::vmx::adjust_vmx_ctrl(
        crate::hv::vmx::IA32_VMX_PROCBASED_CTLS2,
        PROC2_BASED_ENABLE_EPT | PROC2_BASED_ENABLE_VMFUNC,
    );
    let exit = crate::hv::vmx::adjust_vmx_ctrl(exit_msr, EXIT_CTL_HOST_ADDR_SPACE_SIZE);
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
        hvlogf(format_args!(
            "hv: vm{}-{} reporting: vmcs ctrl unsupported: primary bit ACTIVATE_SECONDARY not available",
            current_vm_id_for_log(),
            lineage_record.level
        ));
        return Err("secondary controls unsupported");
    }
    if (proc & PROC_BASED_PAUSE_EXITING) == 0 {
        hvlogf(format_args!(
            "hv: vm{}-{} reporting: vmcs ctrl unsupported: primary bit PAUSE_EXITING not available",
            current_vm_id_for_log(),
            lineage_record.level
        ));
    }
    if (proc2 & PROC2_BASED_ENABLE_EPT) == 0 {
        hvlogf(format_args!(
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
            let guest_fs_base = crate::stackkeeper::vm_hull_fs_base(vm_id);
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
            let guest_cr3 = if restored.is_some() {
                current_guest_cr3_pa().or_else(|_| {
                    build_guest_cr3_for_vm_with_mode(vm_id, guest_rip, guest_rsp, boot_mode)
                })?
            } else {
                build_guest_cr3_for_vm_with_mode(vm_id, guest_rip, guest_rsp, boot_mode)?
            };
            crate::hv::vmx::reset_guest_registers();
            vmwrite(VMCS_GUEST_CR0, host_cr0)?;
            vmwrite(VMCS_GUEST_CR3, guest_cr3)?;
            vmwrite(VMCS_GUEST_CR4, host_cr4)?;
            vmwrite(VMCS_GUEST_RFLAGS, (guest_rflags | RFLAGS_RESERVED_BIT1) & !RFLAGS_IF)?;
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

            return Ok(());
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
    let guest_fs_base = crate::stackkeeper::vm_hull_fs_base(vm_id);
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
    let guest_cr3 = if restored.is_some() {
        current_guest_cr3_pa().or_else(|_| build_guest_cr3_for_vm(vm_id, guest_rip, guest_rsp))?
    } else {
        build_guest_cr3_for_vm(vm_id, guest_rip, guest_rsp)?
    };
    crate::hv::vmx::reset_guest_registers();
    vmwrite(VMCS_GUEST_CR0, host_cr0)?;
    vmwrite(VMCS_GUEST_CR3, guest_cr3)?;
    vmwrite(VMCS_GUEST_CR4, host_cr4)?;
    vmwrite(VMCS_GUEST_RFLAGS, (guest_rflags | RFLAGS_RESERVED_BIT1) & !RFLAGS_IF)?;
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

    Ok(())
}
