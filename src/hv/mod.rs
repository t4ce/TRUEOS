pub mod guest_run;
pub mod guest_work;
pub mod memory;
pub mod snapshot;
pub mod store;
pub mod vmcall;
pub mod vmm;
pub mod vmx;
pub mod vnet;

use crate::hv::vmx::*;

pub use trueos_vm::guest;

use alloc::string::String as AllocString;
use alloc::vec::Vec as AllocVec;
use core::arch::x86_64::{__cpuid, __cpuid_count};
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use embassy_executor::{Spawner, task};
use heapless::{Deque, String};
use spin::Mutex;
use x86_64::instructions::tables::{sgdt, sidt};
use x86_64::registers::control::{Cr0, Cr0Flags, Cr3, Cr4, Cr4Flags};
use x86_64::registers::model_specific::Msr;
use x86_64::registers::rflags;
use x86_64::registers::segmentation::{CS, DS, ES, FS, GS, SS, Segment};

use crate::shell2::{ShellBackend2, ShellIo2};

use guest_work::{VmLaneProfile, pick_vm_hull_lane};
use memory::*;
use snapshot::*;
use vmm::VmManager;

const MAIN_LOOP_MARKER: &[u8] = b"main: entering executor loop";
const VMX_PAGE_SIZE: usize = 4096;
const HV_LOG_CAP: usize = 64;
const HV_LOG_LINE: usize = 200;

static VMM: VmManager = VmManager::new();
static VM1_RUNNING: AtomicBool = AtomicBool::new(false);
static VM1_STARTING: AtomicBool = AtomicBool::new(false);
static VM1_STOP_REQ: AtomicBool = AtomicBool::new(false);
static VM1_PRESERVE_REQ: AtomicBool = AtomicBool::new(false);
static VM1_PRESERVE_EXIT: AtomicBool = AtomicBool::new(false);
static VM1_MARKER_SEEN: AtomicBool = AtomicBool::new(false);
static VM1_GUEST_BOOT_ARMED: AtomicBool = AtomicBool::new(false);
static HV_LOG_SEQ: AtomicU64 = AtomicU64::new(0);
static VM_BOOT_MODE: Mutex<VmBootMode> = Mutex::new(VmBootMode::Hull);
static BLUEPRINT_LAUNCH_STATE: Mutex<Option<BlueprintLaunchState>> = Mutex::new(None);
static APP_WINDOW_SESSION: Mutex<Option<AppWindowSession>> = Mutex::new(None);

#[derive(Clone)]
struct HvLogEntry {
    seq: u64,
    msg: String<HV_LOG_LINE>,
}

static HV_LOG_RING: Mutex<Deque<HvLogEntry, HV_LOG_CAP>> = Mutex::new(Deque::new());

pub static mut VMXON_REGION: VmxPage = VmxPage([0u8; VMX_PAGE_SIZE]);
pub static mut VMCS_REGION: VmxPage = VmxPage([0u8; VMX_PAGE_SIZE]);
pub static mut HV_HOST_GDT: [u64; 8] = [0u64; 8];
pub static mut HV_HOST_TSS: [u8; 104] = [0u8; 104];

pub use snapshot::{RestoreError, SaveError};

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

#[derive(Clone)]
pub struct BlueprintLaunchState {
    pub archive: AllocString,
    pub module_bytes: AllocVec<u8>,
    pub app_args: AllocVec<AllocString>,
}

#[derive(Clone)]
struct AppWindowSession {
    archive: AllocString,
    window_ids: AllocVec<u32>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum StopError {
    UnsupportedVmId,
    NotRunning,
}

#[derive(Copy, Clone, Debug)]
pub struct HvStatus {
    pub vendor_intel: bool,
    pub has_msr: bool,
    pub has_vmx: bool,
    pub feature_control_locked: bool,
    pub feature_control_vmx_outside_smx: bool,
    pub vm1_running: bool,
    pub vm1_starting: bool,
    pub vm1_marker_seen: bool,
    pub guest_module_present: bool,
    pub stored_vm_count: usize,
}

#[inline]
fn primary_vm_id() -> u8 {
    VM1_ID
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

    let seq = HV_LOG_SEQ.fetch_add(1, Ordering::AcqRel) + 1;
    {
        let mut ring = HV_LOG_RING.lock();
        if ring.is_full() {
            let _ = ring.pop_front();
        }
        let _ = ring.push_back(HvLogEntry {
            seq,
            msg: line.clone(),
        });
    }

    crate::log!("{}\n", line.as_str());
}

pub fn status() -> HvStatus {
    let (vendor_intel, has_msr, has_vmx, fc_locked, fc_vmx_outside_smx) = vmx_caps();
    HvStatus {
        vendor_intel,
        has_msr,
        has_vmx,
        feature_control_locked: fc_locked,
        feature_control_vmx_outside_smx: fc_vmx_outside_smx,
        vm1_running: VM1_RUNNING.load(Ordering::Acquire),
        vm1_starting: VM1_STARTING.load(Ordering::Acquire),
        vm1_marker_seen: VM1_MARKER_SEEN.load(Ordering::Acquire),
        guest_module_present: crate::limine::guest_kernel_bytes().is_some(),
        stored_vm_count: crate::hv::store::committed_vm_count(),
    }
}

pub fn start(
    vm_id: u8,
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    stack_mb: Option<usize>,
) -> Result<(), StartError> {
    start_with_mode(vm_id, spawner, io, VmBootMode::Hull, stack_mb)
}

pub fn start_full(
    vm_id: u8,
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
) -> Result<(), StartError> {
    start_with_mode(vm_id, spawner, io, VmBootMode::Full, None)
}

fn start_with_mode(
    vm_id: u8,
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    boot_mode: VmBootMode,
    stack_mb: Option<usize>,
) -> Result<(), StartError> {
    if vm_id != VM1_ID {
        return Err(StartError::UnsupportedVmId);
    }

    if VM1_RUNNING.load(Ordering::Acquire) || VM1_STARTING.load(Ordering::Acquire) {
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
        return Err(StartError::VmxUnsupported);
    }

    if boot_mode == VmBootMode::Full && crate::limine::guest_kernel_bytes().is_none() {
        return Err(StartError::MissingGuestModule);
    }

    let requested_stack_mb = stack_mb.unwrap_or(memory::guest_stack_default_mb());
    let active_stack_mb = memory::clamp_guest_stack_mb(requested_stack_mb);
    if memory::prepare_guest_stack_mb(active_stack_mb).is_err() {
        return Err(StartError::GuestMemoryUnavailable);
    }

    VM1_STOP_REQ.store(false, Ordering::Release);
    VM1_MARKER_SEEN.store(false, Ordering::Release);
    VM1_STARTING.store(true, Ordering::Release);
    *VM_BOOT_MODE.lock() = boot_mode;

    let _ = spawner;
    let _ = io;
    let profile = VmLaneProfile::vm_default();
    let target = match pick_vm_hull_lane() {
        Ok(target) => target,
        Err(error) => {
            VM1_STARTING.store(false, Ordering::Release);
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
    // and never on the first AP service lane.
    if !profile.requires_reserved_vm_lane() || !target.supports(profile) {
        VM1_STARTING.store(false, Ordering::Release);
        hvlogf(format_args!(
            "hv: vm{} lane rejected: role={} placement={} slot={} requires reserved VM lane on AP>2",
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
        memory::active_guest_stack_mb()
    ));

    match vm1_task() {
        Ok(token) => target.spawner.spawn(token),
        Err(_) => {
            VM1_STARTING.store(false, Ordering::Release);
            return Err(StartError::SpawnFailed);
        }
    }
    Ok(())
}

pub fn stop(vm_id: u8) -> Result<bool, StopError> {
    if vm_id != VM1_ID {
        return Err(StopError::UnsupportedVmId);
    }

    if VM1_RUNNING.load(Ordering::Acquire) || VM1_STARTING.load(Ordering::Acquire) {
        VM1_STOP_REQ.store(true, Ordering::Release);
        hvlogf(format_args!("hv: vm{} lifecycle: stop requested", primary_vm_id()));
        Ok(true)
    } else {
        hvlogf(format_args!("hv: vm{} lifecycle: stop ignored (not running)", primary_vm_id()));
        Ok(false)
    }
}

pub fn write_logs(io: &dyn ShellIo2) {
    let mut lines: [Option<HvLogEntry>; HV_LOG_CAP] = core::array::from_fn(|_| None);
    let mut n = 0usize;
    {
        let ring = HV_LOG_RING.lock();
        for e in ring.iter() {
            if n >= HV_LOG_CAP {
                break;
            }
            lines[n] = Some(e.clone());
            n += 1;
        }
    }

    if n == 0 {
        io.write_str("hv: log empty\r\n");
        return;
    }

    for i in 0..n {
        if let Some(e) = &lines[i] {
            io.write_fmt(format_args!("hvlog[{}]: {}\r\n", e.seq, e.msg.as_str()));
        }
    }
}

pub fn save_snapshot(vm_id: u8) -> Result<usize, SaveError> {
    if vm_id != VM1_ID {
        return Err(SaveError::UnsupportedVmId);
    }

    let bytes = snapshot_bytes()?;
    crate::hv::store::save_bytes(vm_id, bytes).map_err(map_store_save_error)
}

pub fn restore_snapshot(vm_id: u8) -> Result<usize, RestoreError> {
    if vm_id != VM1_ID {
        return Err(RestoreError::UnsupportedVmId);
    }

    let bytes = crate::hv::store::load_bytes(vm_id).map_err(map_store_restore_error)?;

    restore_snapshot_bytes(bytes.as_slice())?;
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
    lr.entered != 0
        && lr.launch_failed == 0
        && ((lr.exit_reason & 0xFFFF) == VMEXIT_REASON_VMCALL
            || VM1_PRESERVE_EXIT.load(Ordering::Acquire))
}

fn snapshot_on_preserve_exit() {
    match snapshot_bytes() {
        Ok(bytes) => match crate::hv::store::save_bytes(VM1_ID, bytes) {
            Ok(saved) => hvlogf(format_args!(
                "hv: vm{} reporting: preserve snapshot saved store=hv-ramdisk path=vm/vm1.snapshot bytes={}",
                primary_vm_id(),
                saved
            )),
            Err(e) => hvlogf(format_args!(
                "hv: vm{} reporting: preserve snapshot save failed ({:?})",
                primary_vm_id(),
                e
            )),
        },
        Err(e) => hvlogf(format_args!(
            "hv: vm{} reporting: preserve snapshot bytes failed ({:?})",
            primary_vm_id(),
            e
        )),
    }
}

pub fn guest_boot_take() -> bool {
    VM1_GUEST_BOOT_ARMED.swap(false, Ordering::AcqRel)
}

pub fn guest_boot_active() -> bool {
    VM1_GUEST_BOOT_ARMED.load(Ordering::Acquire)
}

pub fn request_preserve_vm1() -> bool {
    let running = VM1_RUNNING.load(Ordering::Acquire);
    let starting = VM1_STARTING.load(Ordering::Acquire);
    if !running && !starting {
        return false;
    }
    VM1_PRESERVE_REQ.store(true, Ordering::Release);
    true
}

pub fn stage_blueprint_launch(state: BlueprintLaunchState) {
    *BLUEPRINT_LAUNCH_STATE.lock() = Some(state);
}

pub fn take_blueprint_launch() -> Option<BlueprintLaunchState> {
    BLUEPRINT_LAUNCH_STATE.lock().take()
}

pub fn blueprint_launch_active() -> bool {
    BLUEPRINT_LAUNCH_STATE.lock().is_some()
}

fn app_window_broker_log(args: core::fmt::Arguments<'_>) {
    let mut line: String<HV_LOG_LINE> = String::new();
    let _ = line.write_fmt(args);
    if line.is_empty() {
        return;
    }

    hvlogf(format_args!("{}", line.as_str()));
}

pub fn log_active_blueprint_console_line(args: core::fmt::Arguments<'_>) {
    let mut line: String<HV_LOG_LINE> = String::new();
    let _ = line.write_fmt(args);
    if line.is_empty() {
        return;
    }
    hvlogf(format_args!("{}", line.as_str()));
}

pub fn log_blueprint_app_window_event(args: core::fmt::Arguments<'_>) {
    app_window_broker_log(args);
}

pub fn begin_blueprint_app_window_session(archive: &str) {
    *APP_WINDOW_SESSION.lock() = Some(AppWindowSession {
        archive: AllocString::from(archive),
        window_ids: AllocVec::with_capacity(4),
    });
    app_window_broker_log(format_args!("app-window-broker: session begin archive={}", archive));
}

pub fn register_blueprint_app_window(window_id: u32, kind: &str, title: &str) {
    let mut sessions = APP_WINDOW_SESSION.lock();
    let Some(session) = sessions.as_mut() else {
        app_window_broker_log(format_args!(
            "app-window-broker: create without active session kind={} window={} title={}",
            kind, window_id, title
        ));
        return;
    };

    if !session.window_ids.contains(&window_id) {
        session.window_ids.push(window_id);
    }

    app_window_broker_log(format_args!(
        "app-window-broker: created archive={} kind={} window={} title={}",
        session.archive.as_str(),
        kind,
        window_id,
        title
    ));
}

pub fn finish_blueprint_app_window_session(close_windows: bool) {
    let Some(session) = APP_WINDOW_SESSION.lock().take() else {
        return;
    };

    app_window_broker_log(format_args!(
        "app-window-broker: session end archive={} windows={} close_windows={}",
        session.archive.as_str(),
        session.window_ids.len(),
        close_windows
    ));

    if close_windows {
        for window_id in session.window_ids {
            let _ = crate::r::ui2::close_window(window_id);
        }
    }
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

#[task(pool_size = 1)]
async fn vm1_task() {
    let lineage_record = LineageRecord::new();
    VM1_STARTING.store(false, Ordering::Release);
    VM1_RUNNING.store(true, Ordering::Release);
    VM1_PRESERVE_REQ.store(false, Ordering::Release);
    VM1_PRESERVE_EXIT.store(false, Ordering::Release);
    let cpu = crate::cpu::CpuProfile::current();
    if let Some(cpu) = cpu {
        hvlogf(format_args!(
            "hv: vm{}-{} lifecycle: starting slot={} lapic={} kind={}",
            primary_vm_id(),
            lineage_record.level,
            cpu.slot(),
            cpu.lapic_id(),
            cpu.core_kind_name()
        ));
    } else {
        hvlogf(format_args!(
            "hv: vm{}-{} lifecycle: starting slot=unknown",
            primary_vm_id(),
            lineage_record.level
        ));
    }

    let boot_mode = *VM_BOOT_MODE.lock();
    let guest = crate::limine::guest_kernel_bytes();
    match boot_mode {
        VmBootMode::Full => {
            let guest_len = guest.map(|b| b.len()).unwrap_or(0);
            hvlogf(format_args!(
                "hv: vm{} lifecycle: full guest bytes={}",
                primary_vm_id(),
                guest_len
            ));
            if let Some(bytes) = guest {
                if let Some(entry) = guest_kernel_elf_entry(bytes) {
                    hvlogf(format_args!(
                        "hv: vm{} reporting: full guest elf entry=0x{:016X} vmx_guest_entry=0x{:016X}",
                        primary_vm_id(),
                        entry,
                        guest_launch_rip()
                    ));
                } else {
                    hvlogf(format_args!(
                        "hv: vm{} reporting: full guest bytes present but ELF entry parse failed; vmx_guest_entry=0x{:016X}",
                        primary_vm_id(),
                        guest_launch_rip()
                    ));
                }
            }
        }
        VmBootMode::Hull => {
            hvlogf(format_args!(
                "hv: vm{} lifecycle: hull guest entry=0x{:016X} stack_mib={}",
                primary_vm_id(),
                guest_launch_rip(),
                memory::active_guest_stack_mb()
            ));
        }
    }
    hvlogf(format_args!("hv: vm{} reporting: vmx preflight ok, stage=m1", primary_vm_id()));
    hvlogf(format_args!("hv: vm{} reporting: vlayer policy=integrity-first", primary_vm_id()));
    let guest_heap_ready = crate::allocators::ensure_hv_guest_heap_ready();
    if guest_heap_ready {
        let stats = crate::allocators::hv_guest_heap_stats();
        hvlogf(format_args!(
            "hv: vm{} reporting: hv-guest-heap virt=0x{:016X}..0x{:016X} src={:?} free_bytes={} blocks={}",
            primary_vm_id(),
            stats.heap_start,
            stats.heap_end,
            stats.source,
            stats.free_bytes,
            stats.free_blocks
        ));
    }
    let _entered_guest_alloc = crate::allocators::enter_hv_guest_domain_current_cpu();
    let launch_result = vmx_launch_once_with_ept(lineage_record);
    crate::allocators::leave_hv_guest_domain_current_cpu();
    match launch_result {
        Ok(lr) => {
            capture_snapshot_meta(lr);
            if vmexit_is_preserve(lr) {
                snapshot_on_preserve_exit();
            }
            hvlogf(format_args!(
                "hv: vm{}-{} reporting: vmlaunch entered={} launch_failed={} exit_reason=0x{:X} exit_qual=0x{:X} guest_rip=0x{:016X}",
                primary_vm_id(),
                lineage_record.level,
                lr.entered,
                lr.launch_failed,
                lr.exit_reason,
                lr.exit_qualification,
                lr.guest_rip
            ));
        }
        Err(e) => hvlogf(format_args!(
            "hv: vm{}-{} reporting: vmlaunch/ept failed ({})",
            primary_vm_id(),
            lineage_record.level,
            e
        )),
    }

    if boot_mode == VmBootMode::Full {
        if let Some(bytes) = guest {
            if contains_bytes(bytes, MAIN_LOOP_MARKER) {
                VM1_MARKER_SEEN.store(true, Ordering::Release);
                hvlogf(format_args!(
                    "hv: vm{} reporting: main: entering executor loop",
                    primary_vm_id()
                ));
            }
        }
    }

    VM1_RUNNING.store(false, Ordering::Release);
    VM1_STARTING.store(false, Ordering::Release);
    VM1_STOP_REQ.store(false, Ordering::Release);
    VM1_PRESERVE_REQ.store(false, Ordering::Release);
    VM1_PRESERVE_EXIT.store(false, Ordering::Release);
    hvlogf(format_args!("hv: vm{} lifecycle: stopped", primary_vm_id()));
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

fn vmx_launch_once_with_ept(lineage_record: LineageRecord) -> Result<LaunchResult, &'static str> {
    let caps = status();
    if !caps.vendor_intel || !caps.has_msr || !caps.has_vmx {
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

    let basic = unsafe { Msr::new(crate::hv::vmx::IA32_VMX_BASIC).read() };
    let revision = (basic & 0x7fff_ffff) as u32;

    let vmxon_va = unsafe { core::ptr::addr_of_mut!(VMXON_REGION.0) } as *mut u8;
    let vmcs_va = unsafe { core::ptr::addr_of_mut!(VMCS_REGION.0) } as *mut u8;
    unsafe {
        core::ptr::write_bytes(vmxon_va, 0, VMX_PAGE_SIZE);
        core::ptr::write_bytes(vmcs_va, 0, VMX_PAGE_SIZE);
        *(vmxon_va as *mut u32) = revision;
        *(vmcs_va as *mut u32) = revision;
    }

    let vmxon_pa = kernel_va_to_pa(vmxon_va as u64).ok_or("vmxon pa")?;
    let vmcs_pa = kernel_va_to_pa(vmcs_va as u64).ok_or("vmcs pa")?;
    hvlogf(format_args!(
        "hv: vm{} reporting: vmlaunch prep revision=0x{:08X} vmxon_pa=0x{:016X} vmcs_pa=0x{:016X}",
        primary_vm_id(),
        revision,
        vmxon_pa,
        vmcs_pa
    ));

    if !crate::hv::vmx::vmxon(vmxon_pa) {
        return Err("vmxon");
    }
    if !crate::hv::vmx::vmclear(vmcs_pa) {
        let _ = crate::hv::vmx::vmxoff();
        return Err("vmclear");
    }
    if !crate::hv::vmx::vmptrld(vmcs_pa) {
        let _ = crate::hv::vmx::vmxoff();
        return Err("vmptrld");
    }

    let eptp = match build_ept_identity_4g() {
        Ok(v) => v,
        Err(e) => {
            let _ = vmxoff();
            return Err(e);
        }
    };
    if let Err(e) = setup_vmcs_for_launch(eptp, lineage_record, *VM_BOOT_MODE.lock()) {
        let _ = vmxoff();
        return Err(e);
    }

    // ── vmexit dispatch loop ──────────────────────────────────────────────────
    let mut lr = LaunchResult::default();
    let mut preserve_requested = false;
    let mut cpuid_leaf0_count = 0u32;
    let mut cpuid_leaf80000000_count = 0u32;
    let mut cpuid_leaf1_count = 0u32;
    let mut cpuid_other_count = 0u32;
    let mut first = true;
    loop {
        if first {
            VM1_GUEST_BOOT_ARMED.store(true, Ordering::Release);
            vmlaunch_once_wrapper(&mut lr);
            VM1_GUEST_BOOT_ARMED.store(false, Ordering::Release);
            first = false;
        } else {
            vmresume_once_wrapper(&mut lr);
        }

        if lr.launch_failed != 0 {
            hvlogf(format_args!(
                "hv: vm{} reporting: vmlaunch/vmresume failed instr_err={} rip=0x{:016X}",
                primary_vm_id(),
                lr.instr_err,
                crate::hv::vmx::current_rip()
            ));
            break;
        }
        if lr.entered == 0 {
            hvlogf(format_args!(
                "hv: vm{} reporting: vmlaunch/vmresume: guest not entered",
                primary_vm_id()
            ));
            break;
        }

        let reason = lr.exit_reason & 0xFFFF;
        crate::hv::vmx::log_vmexit_interrupt_info("vmexit");
        if reason == 0x0 {
            if let Some((vector, vector_name, kind, info, err)) = guest_exception_summary() {
                hvlogf(format_args!(
                    "hv: vm{} reporting: guest exception vector={} name={} type={}({}) err=0x{:X} intr_info=0x{:08X}",
                    primary_vm_id(),
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
                primary_vm_id(),
                intr_err,
                (intr_err & (1 << 0)) != 0,
                (intr_err & (1 << 1)) != 0,
                (intr_err & (1 << 2)) != 0,
                (intr_err & (1 << 3)) != 0,
                (intr_err & (1 << 4)) != 0
            ));
            hvlogf(format_args!(
                "hv: vm{} reporting: guest-state cr0=0x{:016X} cr3=0x{:016X} cr4=0x{:016X} efer=0x{:016X}",
                primary_vm_id(),
                guest_cr0,
                guest_cr3,
                guest_cr4,
                guest_efer
            ));
            crate::hv::memory::log_guest_mapping("fault-linear", guest_linear);
            crate::hv::memory::log_guest_mapping("fault-rsp", guest_rsp);
            crate::hv::memory::log_guest_mapping("fault-rip", lr.guest_rip);
            crate::hv::memory::log_guest_mapping_from_cr3("fault-linear", guest_cr3, guest_linear);
            crate::hv::memory::log_guest_mapping_from_cr3("fault-rip", guest_cr3, lr.guest_rip);
            crate::hv::memory::log_guest_phys_pt_context("fault-linear", guest_cr3, guest_linear);
            crate::hv::memory::log_guest_phys_pt_context("fault-rip", guest_cr3, lr.guest_rip);
            crate::hv::memory::log_guest_pt_context("fault-linear", guest_linear);
            crate::hv::memory::log_guest_pt_context("fault-rip", lr.guest_rip);
            let trace = crate::allocators::last_alloc_trace();
            if trace.seq != 0 {
                hvlogf(format_args!(
                    "hv: vm{} reporting: alloc-trace seq={} caller=0x{:016X} caller1=0x{:016X} caller2=0x{:016X} size={} align={} stage={} head=0x{:016X} block=0x{:016X} block_size={} next=0x{:016X} payload=0x{:016X} aligned_used={}",
                    primary_vm_id(),
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
                if !crate::hv::vmcall::dispatch() {
                    break; // preserve — stop the loop
                }
                // service vmcall — loop → vmresume
            }
            0xC => {
                // HLT — advance past it and continue
                let len = vmread(VMCS_VMEXIT_INSTRUCTION_LEN).ok_or("vmread instr len hlt")?;
                vmwrite(VMCS_GUEST_RIP, lr.guest_rip + len)?;
            }
            0xA => {
                let mut regs = crate::hv::vmx::guest_registers();
                let leaf = regs.rax as u32;
                let subleaf = regs.rcx as u32;
                let out = unsafe { __cpuid_count(leaf, subleaf) };
                regs.rax = out.eax as u64;
                regs.rbx = out.ebx as u64;
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
                            primary_vm_id(),
                            leaf,
                            subleaf,
                            out.eax,
                            out.ebx,
                            out.ecx,
                            out.edx
                        ));
                    }
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
                    primary_vm_id(),
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
                    primary_vm_id(),
                    reason
                ));
                break;
            }
        }
        if VM1_PRESERVE_REQ.swap(false, Ordering::AcqRel) {
            preserve_requested = true;
            VM1_PRESERVE_EXIT.store(true, Ordering::Release);
            hvlogf(format_args!(
                "hv: vm{} reporting: host preserve request armed at rip=0x{:016X}",
                primary_vm_id(),
                lr.guest_rip
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
            primary_vm_id(),
            cpuid_leaf0_count,
            cpuid_leaf80000000_count,
            cpuid_leaf1_count,
            cpuid_other_count
        ));
    }
    // ─────────────────────────────────────────────────────────────────────────
    if !crate::hv::vmx::vmxoff() {
        return Err("vmxoff");
    }
    if !preserve_requested {
        VM1_PRESERVE_EXIT.store(false, Ordering::Release);
    }
    Ok(lr)
}

fn setup_vmcs_for_launch(
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
        primary_vm_id(),
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
            primary_vm_id(),
            lineage_record.level
        ));
        return Err("secondary controls unsupported");
    }
    if (proc2 & PROC2_BASED_ENABLE_EPT) == 0 {
        hvlogf(format_args!(
            "hv: vm{}-{} reporting: vmcs ctrl unsupported: secondary bit ENABLE_EPT not available",
            primary_vm_id(),
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
                primary_vm_id(),
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
                    primary_vm_id(),
                    lineage_record.level,
                    avail_sel
                ));
                return Err("host tr ltr");
            }
            hvlogf(format_args!(
                "hv: vm{}-{} reporting: host-state recovered: loaded tr selector=0x{:04X}",
                primary_vm_id(),
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
                primary_vm_id(),
                lineage_record.level,
                synth.tr_sel,
                synth.tr_base
            ));
            let fs_base = unsafe { Msr::new(crate::hv::vmx::IA32_FS_BASE).read() };
            let gs_base = unsafe { Msr::new(crate::hv::vmx::IA32_GS_BASE).read() };
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
                    primary_vm_id(),
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
                    primary_vm_id(),
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
                primary_vm_id(),
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

            let restored = active_restore_meta();
            let guest_rip = restored
                .map(|m| m.guest_rip)
                .unwrap_or_else(guest_launch_rip);
            let guest_rsp = restored
                .map(|m| m.guest_rsp)
                .unwrap_or_else(guest_stack_top);
            let guest_cr3 = if restored.is_some() {
                current_guest_cr3_pa()
                    .or_else(|_| build_guest_cr3_with_mode(guest_rip, guest_rsp, boot_mode))?
            } else {
                build_guest_cr3_with_mode(guest_rip, guest_rsp, boot_mode)?
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
            vmwrite(VMCS_GUEST_FS_BASE, fs_base)?;
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
            primary_vm_id(),
            lineage_record.level
        ));
        return Err("host tr selector");
    }
    tr_base = match crate::hv::vmx::tss_base_from_gdt(host_gdtr_base, tr_sel) {
        Some(v) => v,
        None => {
            hvlogf(format_args!(
                "hv: vm{}-{} reporting: host-state invalid: unable to resolve tss base from gdt",
                primary_vm_id(),
                lineage_record.level
            ));
            return Err("host tr base");
        }
    };
    let fs_base = unsafe { Msr::new(crate::hv::vmx::IA32_FS_BASE).read() };
    let gs_base = unsafe { Msr::new(crate::hv::vmx::IA32_GS_BASE).read() };
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
            primary_vm_id(),
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
            primary_vm_id(),
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
        primary_vm_id(),
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

    let restored = active_restore_meta();
    let guest_rip = restored
        .map(|m| m.guest_rip)
        .unwrap_or_else(guest_launch_rip);
    let guest_rsp = restored
        .map(|m| m.guest_rsp)
        .unwrap_or_else(guest_stack_top);
    let guest_cr3 = if restored.is_some() {
        current_guest_cr3_pa().or_else(|_| build_guest_cr3(guest_rip, guest_rsp))?
    } else {
        build_guest_cr3(guest_rip, guest_rsp)?
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
    vmwrite(VMCS_GUEST_FS_BASE, fs_base)?;
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

fn vmx_smoke() -> Result<(), &'static str> {
    let caps = status();
    if !caps.vendor_intel || !caps.has_msr || !caps.has_vmx {
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
    let revision = (basic & 0x7fff_ffff) as u32;

    let vmxon_va = unsafe { core::ptr::addr_of_mut!(VMXON_REGION.0) } as *mut u8;
    let vmcs_va = unsafe { core::ptr::addr_of_mut!(VMCS_REGION.0) } as *mut u8;
    unsafe {
        core::ptr::write_bytes(vmxon_va, 0, VMX_PAGE_SIZE);
        core::ptr::write_bytes(vmcs_va, 0, VMX_PAGE_SIZE);
        *(vmxon_va as *mut u32) = revision;
        *(vmcs_va as *mut u32) = revision;
    }

    let vmxon_pa = kernel_va_to_pa(vmxon_va as u64).ok_or("vmxon pa")?;
    let vmcs_pa = kernel_va_to_pa(vmcs_va as u64).ok_or("vmcs pa")?;
    hvlogf(format_args!(
        "hv: vm{} reporting: vmx setup revision=0x{:08X} vmxon_pa=0x{:016X} vmcs_pa=0x{:016X}",
        primary_vm_id(),
        revision,
        vmxon_pa,
        vmcs_pa
    ));

    let rip = vmx::current_rip();
    if !vmxon(vmxon_pa) {
        hvlogf(format_args!(
            "hv: vm{} reporting: vmxon failed rip=0x{:016X} pa=0x{:016X}",
            primary_vm_id(),
            rip,
            vmxon_pa
        ));
        return Err("vmxon");
    }
    hvlogf(format_args!("hv: vm{} reporting: vmxon ok rip=0x{:016X}", primary_vm_id(), rip));

    let rip = vmx::current_rip();
    if !vmclear(vmcs_pa) {
        hvlogf(format_args!(
            "hv: vm{} reporting: vmclear failed rip=0x{:016X} pa=0x{:016X}",
            primary_vm_id(),
            rip,
            vmcs_pa
        ));
        let _ = vmxoff();
        return Err("vmclear");
    }
    hvlogf(format_args!("hv: vm{} reporting: vmclear ok rip=0x{:016X}", primary_vm_id(), rip));

    let rip = vmx::current_rip();
    if !vmptrld(vmcs_pa) {
        hvlogf(format_args!(
            "hv: vm{} reporting: vmptrld failed rip=0x{:016X} pa=0x{:016X}",
            primary_vm_id(),
            rip,
            vmcs_pa
        ));
        let _ = vmxoff();
        return Err("vmptrld");
    }
    hvlogf(format_args!("hv: vm{} reporting: vmptrld ok rip=0x{:016X}", primary_vm_id(), rip));

    let ptr = match vmx::vmptrst() {
        Some(v) => v,
        None => {
            let rip = vmx::current_rip();
            hvlogf(format_args!(
                "hv: vm{} reporting: vmptrst failed rip=0x{:016X}",
                primary_vm_id(),
                rip
            ));
            let _ = vmxoff();
            return Err("vmptrst");
        }
    };
    if ptr != vmcs_pa {
        let rip = vmx::current_rip();
        hvlogf(format_args!(
            "hv: vm{} reporting: vmptrst mismatch rip=0x{:016X} got=0x{:016X} want=0x{:016X}",
            primary_vm_id(),
            rip,
            ptr,
            vmcs_pa
        ));
        let _ = vmxoff();
        return Err("vmptrst mismatch");
    }
    hvlogf(format_args!(
        "hv: vm{} reporting: vmptrst ok current_vmcs=0x{:016X}",
        primary_vm_id(),
        ptr
    ));

    if !vmxoff() {
        let rip = vmx::current_rip();
        hvlogf(format_args!(
            "hv: vm{} reporting: vmxoff failed rip=0x{:016X}",
            primary_vm_id(),
            rip
        ));
        return Err("vmxoff");
    }
    hvlogf(format_args!("hv: vm{} reporting: vmxoff ok", primary_vm_id()));
    Ok(())
}
