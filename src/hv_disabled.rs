use alloc::string::String as AllocString;
use alloc::vec::Vec as AllocVec;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_executor::{Spawner, task};
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

use crate::shell2::ShellBackend2;

const VM1_ID: u8 = 0;

static GUEST_BOOT_ARMED: AtomicBool = AtomicBool::new(false);
static BLUEPRINT_LAUNCH_STATE: Mutex<Option<BlueprintLaunchState>> = Mutex::new(None);

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

#[derive(Copy, Clone, Debug)]
pub enum SaveError {
    UnsupportedVmId,
    NoRoot,
    NoSnapshot,
    BeginWrite,
    Io(crate::disc::block::Error),
}

#[derive(Copy, Clone, Debug)]
pub enum RestoreError {
    UnsupportedVmId,
    NoRoot,
    MissingFile,
    Read(crate::disc::block::Error),
    BadSnapshot,
    CodeMismatch,
}

#[derive(Clone)]
pub struct BlueprintLaunchState {
    pub archive: AllocString,
    pub module_bytes: AllocVec<u8>,
    pub app_args: AllocVec<AllocString>,
    pub console_target: Option<crate::shell2::MatrixTarget>,
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

pub mod guest {
    pub unsafe fn entry() -> ! {
        loop {
            core::hint::spin_loop();
        }
    }
}

pub mod memory {
    pub const fn guest_stack_default_mb() -> usize {
        16
    }

    pub fn clamp_guest_stack_mb(stack_mb: usize) -> usize {
        stack_mb.max(1)
    }
}

pub mod store {
    use super::*;

    #[task]
    pub async fn vm_store_task() {
        loop {
            Timer::after(EmbassyDuration::from_secs(60)).await;
        }
    }

    #[task]
    pub async fn vm_store_replication_task() {
        loop {
            Timer::after(EmbassyDuration::from_secs(60)).await;
        }
    }
}

pub fn guest_boot_take() -> bool {
    GUEST_BOOT_ARMED.swap(false, Ordering::AcqRel)
}

pub fn guest_boot_active() -> bool {
    GUEST_BOOT_ARMED.load(Ordering::Acquire)
}

pub fn hvlogf(args: core::fmt::Arguments<'_>) {
    crate::log!("hv(stub): {}\n", args);
}

pub fn status() -> HvStatus {
    HvStatus {
        vendor_intel: false,
        has_msr: false,
        has_vmx: false,
        feature_control_locked: false,
        feature_control_vmx_outside_smx: false,
        vm1_running: false,
        vm1_starting: false,
        vm1_marker_seen: false,
        guest_module_present: false,
        stored_vm_count: 0,
    }
}

pub fn start(
    vm_id: u8,
    _spawner: &Spawner,
    _io: &'static dyn ShellBackend2,
    _stack_mb: Option<usize>,
) -> Result<(), StartError> {
    if vm_id != VM1_ID {
        return Err(StartError::UnsupportedVmId);
    }
    Err(StartError::VmxUnsupported)
}

pub fn start_full(
    vm_id: u8,
    _spawner: &Spawner,
    _io: &'static dyn ShellBackend2,
) -> Result<(), StartError> {
    if vm_id != VM1_ID {
        return Err(StartError::UnsupportedVmId);
    }
    Err(StartError::VmxUnsupported)
}

pub fn stop(vm_id: u8) -> Result<bool, StopError> {
    if vm_id != VM1_ID {
        return Err(StopError::UnsupportedVmId);
    }
    Ok(false)
}

pub fn request_preserve_vm1() -> bool {
    false
}

pub fn save_snapshot(vm_id: u8) -> Result<usize, SaveError> {
    if vm_id != VM1_ID {
        return Err(SaveError::UnsupportedVmId);
    }
    Err(SaveError::NoSnapshot)
}

pub fn restore_snapshot(vm_id: u8) -> Result<usize, RestoreError> {
    if vm_id != VM1_ID {
        return Err(RestoreError::UnsupportedVmId);
    }
    Err(RestoreError::MissingFile)
}

pub fn stage_blueprint_launch(state: BlueprintLaunchState) {
    *BLUEPRINT_LAUNCH_STATE.lock() = Some(state);
}

pub fn take_blueprint_launch() -> Option<BlueprintLaunchState> {
    BLUEPRINT_LAUNCH_STATE.lock().take()
}