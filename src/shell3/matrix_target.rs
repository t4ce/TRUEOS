//! Matrix identity shared by Shell3 frontends and kernel resource owners.

use alloc::string::String;

/// Identity of one slot lifetime, independent of its selected frontend.
/// Recreating a slot must issue a new generation to reject stale owners.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MatrixSlotLease {
    id: String,
    lifetime_generation: u64,
}

impl MatrixSlotLease {
    pub(crate) fn from_identity(id: String, lifetime_generation: u64) -> Self {
        Self { id, lifetime_generation }
    }

    pub(crate) fn name(&self) -> &str {
        &self.id
    }

    pub(crate) const fn lifetime_generation(&self) -> u64 {
        self.lifetime_generation
    }
}

#[derive(Clone, Debug)]
pub(crate) struct MatrixTarget {
    lease: MatrixSlotLease,
}

impl MatrixTarget {
    pub(crate) fn from_lease(lease: MatrixSlotLease) -> Self {
        Self { lease }
    }
}

pub(crate) fn matrix_target_slot_lease(target: &MatrixTarget) -> MatrixSlotLease {
    target.lease.clone()
}

/// Release only the VM owner accepted by Shell3's frontend routing table.
pub(crate) fn release_matrix_target_terminal_handoff(target: &MatrixTarget, vm_id: u8) -> bool {
    super::tui::release(target, vm_id).unwrap_or(false)
}
