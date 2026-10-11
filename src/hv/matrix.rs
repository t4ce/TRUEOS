//! Blueprint Matrix identities. VM ownership is the lease stored in TrueosVmId;
//! this registry holds only archive metadata, including queued and exited apps.
use crate::shell3::matrix_target::{
    MatrixSlotAttachment, attach_matrix_slot_resource, matrix_slot_is_live,
};
use crate::shell3::{MatrixSlotLease, MatrixTarget, matrix_target_slot_lease};
use alloc::{string::String, sync::Arc, vec::Vec};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug)]
pub(crate) struct AppReceipt {
    pub slot: String,
    pub lifetime: u64,
    pub app: String,
    pub sha256: [u8; 32],
}
struct Entry {
    lease: MatrixSlotLease,
    receipt: AppReceipt,
}
static APPS: spin::Mutex<Vec<Entry>> = spin::Mutex::new(Vec::new());

struct Attachment;
impl MatrixSlotAttachment for Attachment {
    fn on_matrix_slot_freed(&self, lease: &MatrixSlotLease) {
        APPS.lock().retain(|entry| entry.lease != *lease);
        // Drop registry locks before lifecycle control. A reused vmid is never
        // sufficient authority: kill checks this exact slot lifetime again.
        if let Some(vm) = vm_for_lease(lease) {
            let _ = super::kill_for_matrix_slot(vm, lease);
        }
    }
}

pub(crate) fn reserve(archive: &str, bytes: &[u8]) -> Result<(MatrixTarget, AppReceipt), String> {
    let (name, lifetime) = crate::shell3::MatrixSlots::fresh_slot(None);
    let lease = MatrixSlotLease::from_identity(name.clone(), lifetime);
    let receipt = AppReceipt {
        slot: name,
        lifetime,
        app: archive.into(),
        sha256: Sha256::digest(bytes).into(),
    };
    APPS.lock().push(Entry {
        lease: lease.clone(),
        receipt: receipt.clone(),
    });
    if attach_matrix_slot_resource(&lease, Arc::new(Attachment)).is_err() {
        APPS.lock().retain(|entry| entry.lease != lease);
        crate::shell3::MatrixSlots::drop_lease(&lease);
        return Err("apps: Matrix reservation expired".into());
    }
    let target = MatrixTarget::from_lease(lease);
    if let Err(error) = crate::shell3::tui::reserve(&target) {
        discard(&target);
        return Err(error);
    }
    Ok((target, receipt))
}

pub(crate) fn discard(target: &MatrixTarget) {
    let lease = matrix_target_slot_lease(target);
    crate::shell3::MatrixSlots::drop_lease(&lease);
}

/// Both queued launches and reconstruction enter through this identity check.
/// An existing app lifetime is reused; a restore in a new kernel reserves one.
fn target_for_vm(
    vm: u8,
    archive: &str,
    bytes: &[u8],
    requested: Option<MatrixTarget>,
) -> Result<MatrixTarget, String> {
    let owner = super::vm_slot(vm).ok_or("apps: unsupported VM ID")?;
    let target = requested.or_else(|| {
        owner
            .matrix_owner
            .lock()
            .clone()
            .map(MatrixTarget::from_lease)
    });
    if let Some(target) = target {
        let lease = matrix_target_slot_lease(&target);
        if !matrix_slot_is_live(&lease)
            || !APPS
                .lock()
                .iter()
                .any(|entry| entry.lease == lease && entry.receipt.app == archive)
        {
            return Err("apps: Matrix identity expired or archive mismatch".into());
        }
        return Ok(target);
    }
    reserve(archive, bytes).map(|(target, _)| target)
}

/// Called under VM lifecycle admission. Slot deletion cannot interleave with
/// publication of its owner, and the same identity cannot own two VM IDs.
pub(crate) fn bind(
    vm: u8,
    archive: &str,
    bytes: &[u8],
    requested: Option<MatrixTarget>,
) -> Result<MatrixTarget, String> {
    let target = target_for_vm(vm, archive, bytes, requested)?;
    let lease = matrix_target_slot_lease(&target);
    let bound = crate::shell3::matrix_target::with_live_matrix_slot(&lease, || {
        if vm_for_lease(&lease).is_some_and(|owner| owner != vm) {
            return false;
        }
        let Some(vm) = super::vm_slot(vm) else {
            return false;
        };
        *vm.matrix_owner.lock() = Some(lease.clone());
        true
    });
    if bound != Some(true) {
        return Err("apps: Matrix identity expired or already bound".into());
    }
    crate::shell3::tui::bind_vm(&target, vm);
    Ok(target)
}

pub(crate) fn app(name: &str, lifetime: Option<u64>) -> Option<AppReceipt> {
    APPS.lock()
        .iter()
        .find(|entry| {
            entry.lease.name() == name
                && lifetime.is_none_or(|generation| entry.lease.lifetime_generation() == generation)
        })
        .map(|entry| entry.receipt.clone())
}
pub(crate) fn vm_for_lease(lease: &MatrixSlotLease) -> Option<u8> {
    super::trueos_vm_ids
        .iter()
        .enumerate()
        .find_map(|(id, vm)| (vm.matrix_owner.lock().as_ref() == Some(lease)).then_some(id as u8))
}
pub(crate) fn vm_for_slot(name: &str, lifetime: u64) -> Option<u8> {
    vm_for_lease(&MatrixSlotLease::from_identity(name.into(), lifetime))
}
pub(crate) fn working_slots() -> Vec<String> {
    super::trueos_vm_ids
        .iter()
        .filter_map(|vm| {
            if !vm.running.load(core::sync::atomic::Ordering::Acquire)
                || vm.pause_latched.load(core::sync::atomic::Ordering::Acquire)
            {
                return None;
            }
            vm.matrix_owner
                .lock()
                .as_ref()
                .map(|lease| lease.name().into())
        })
        .collect()
}
pub(crate) fn retire_vm(vm: u8) {
    let lease = super::vm_slot(vm).and_then(|vm| vm.matrix_owner.lock().take());
    if let Some(lease) = lease {
        crate::shell3::MatrixSlots::drop_lease(&lease);
    }
}

/// Synchronous admission failure rolls back only the lease this attempt bound.
/// A successful worker submission transfers ownership to the VM lifecycle.
pub(super) struct Admission {
    vm: u8,
    target: Option<MatrixTarget>,
}
impl Admission {
    pub fn new(vm: u8) -> Self {
        Self { vm, target: None }
    }
    pub fn bound(&mut self, target: &MatrixTarget) {
        self.target = Some(target.clone());
    }
    pub fn commit(&mut self) {
        self.target = None;
    }
}
impl Drop for Admission {
    fn drop(&mut self) {
        let Some(target) = self.target.take() else {
            return;
        };
        let lease = matrix_target_slot_lease(&target);
        if let Some(vm) = super::vm_slot(self.vm) {
            let mut owner = vm.matrix_owner.lock();
            if owner.as_ref() == Some(&lease) {
                *owner = None;
            }
        }
        discard(&target);
    }
}
