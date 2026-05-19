use alloc::string::String as AllocString;
use alloc::vec::Vec as AllocVec;

use embassy_executor::Spawner;

use crate::shell2::{MatrixTarget, ShellBackend2, ShellIo2};

pub mod memory {
    pub const GUEST_STACK_DEFAULT_MIB: usize = crate::allcaps::hv::GUEST_STACK_DEFAULT_MIB;
    pub const GUEST_STACK_MIN_MIB: usize = crate::allcaps::hv::GUEST_STACK_MIN_MIB;
    pub const GUEST_STACK_MAX_MIB: usize = crate::allcaps::hv::GUEST_STACK_MAX_MIB;

    pub const fn guest_stack_default_mb() -> usize {
        GUEST_STACK_DEFAULT_MIB
    }

    pub const fn clamp_guest_stack_mb(stack_mb: usize) -> usize {
        if stack_mb < GUEST_STACK_MIN_MIB {
            GUEST_STACK_MIN_MIB
        } else if stack_mb > GUEST_STACK_MAX_MIB {
            GUEST_STACK_MAX_MIB
        } else {
            stack_mb
        }
    }
}

pub mod store {
    pub fn committed_vm_count() -> usize {
        0
    }

    #[embassy_executor::task(pool_size = 1)]
    pub async fn vm_store_task() {}

    #[embassy_executor::task(pool_size = 1)]
    pub async fn vm_store_replication_task() {}
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
pub enum StopError {
    UnsupportedVmId,
    NotRunning,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SaveError {
    UnsupportedVmId,
    BeginWrite,
    Io(&'static str),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum RestoreError {
    UnsupportedVmId,
    MissingFile,
    Read(&'static str),
    BadMagic,
    BadVersion,
    BadLength,
    GuestMemoryUnavailable,
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
    pub unpacked_bytes: AllocVec<u8>,
    pub app_args: AllocVec<AllocString>,
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
    pub active_vm_ids: [Option<u8>; crate::allcaps::hv::VM_ID_LIMIT],
    pub vm_shared_heap_total_bytes: usize,
    pub vm_shared_heap_free_bytes: usize,
    pub vm_shared_stack_bytes: usize,
    pub vm_shared_vmx_bytes: usize,
}

pub fn hvlogf(_args: core::fmt::Arguments<'_>) {}

pub fn status() -> HvStatus {
    HvStatus {
        vendor_intel: false,
        has_msr: false,
        has_vmx: false,
        feature_control_locked: false,
        feature_control_vmx_outside_smx: false,
        guest_module_present: false,
        stored_vm_count: 0,
        vm_id_limit: crate::allcaps::hv::VM_ID_LIMIT,
        running_count: 0,
        starting_count: 0,
        active_vm_ids: [None; crate::allcaps::hv::VM_ID_LIMIT],
        vm_shared_heap_total_bytes: 0,
        vm_shared_heap_free_bytes: 0,
        vm_shared_stack_bytes: 0,
        vm_shared_vmx_bytes: 0,
    }
}

pub fn start(
    _vm_id: u8,
    _spawner: &Spawner,
    _io: &'static dyn ShellBackend2,
    _stack_mb: Option<usize>,
) -> Result<(), StartError> {
    Err(StartError::VmxUnsupported)
}

pub fn start_full(
    _vm_id: u8,
    _spawner: &Spawner,
    _io: &'static dyn ShellBackend2,
) -> Result<(), StartError> {
    Err(StartError::VmxUnsupported)
}

pub fn stop(_vm_id: u8) -> Result<bool, StopError> {
    Ok(false)
}

pub fn write_logs(_io: &dyn ShellIo2) {}

pub fn save_snapshot(_vm_id: u8) -> Result<usize, SaveError> {
    Err(SaveError::UnsupportedVmId)
}

pub fn restore_snapshot(_vm_id: u8) -> Result<usize, RestoreError> {
    Err(RestoreError::MissingFile)
}

pub fn request_preserve_active_vm() -> bool {
    false
}

pub fn stage_blueprint_launch(
    _vm_id: u8,
    _state: BlueprintLaunchState,
    _console_target: Option<MatrixTarget>,
) -> Result<(), StartError> {
    Err(StartError::VmxUnsupported)
}

pub fn take_blueprint_launch() -> Option<BlueprintLaunchState> {
    None
}

pub fn blueprint_launch_active() -> bool {
    false
}

pub fn log_active_blueprint_console_line(_args: core::fmt::Arguments<'_>) {}

pub fn log_blueprint_app_window_event(_args: core::fmt::Arguments<'_>) {}

pub fn begin_blueprint_app_window_session(_vm_id: u8, _archive: &str) {}

pub fn register_blueprint_app_window(_window_id: u32, _kind: &str, _title: &str) {}

pub fn finish_blueprint_app_window_session(_vm_id: u8, _close_windows: bool) {}
