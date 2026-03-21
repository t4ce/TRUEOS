use super::{HV_LOG_CAP, HvLogEntry, StartError};
use crate::shell2::{ShellBackend2, ShellIo2};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use embassy_executor::Spawner;
use heapless::{Deque, String};
use spin::Mutex;

pub const MAX_VMS: usize = 10;

pub struct Vm {
    id: u8,
    running: AtomicBool,
    starting: AtomicBool,
    stop_req: AtomicBool,
    marker_seen: AtomicBool,
    guest_boot_armed: AtomicBool,
}

impl Vm {
    pub fn new(id: u8) -> Self {
        Self {
            id,
            running: AtomicBool::new(false),
            starting: AtomicBool::new(false),
            stop_req: AtomicBool::new(false),
            marker_seen: AtomicBool::new(false),
            guest_boot_armed: AtomicBool::new(false),
        }
    }
}

pub struct VmManager {
    vms: Mutex<[Option<Vm>; MAX_VMS]>,
}

impl VmManager {
    pub const fn new() -> Self {
        const INIT: Option<Vm> = None;
        Self {
            vms: Mutex::new([INIT; MAX_VMS]),
        }
    }

    pub fn start_vm(
        &self,
        vm_id: u8,
        spawner: &Spawner,
        io: &'static dyn ShellBackend2,
    ) -> Result<(), StartError> {
        if vm_id as usize >= MAX_VMS {
            return Err(StartError::UnsupportedVmId);
        }

        let mut vms = self.vms.lock();
        if vms[vm_id as usize].is_some() {
            return Err(StartError::AlreadyRunning);
        }

        vms[vm_id as usize] = Some(Vm::new(vm_id));

        // The rest of the start logic will go here

        Ok(())
    }
}
