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

/// A resource retired when its exact slot lifetime disappears.
pub(crate) trait MatrixSlotAttachment: Send + Sync {
    fn on_matrix_slot_freed(&self, lease: &MatrixSlotLease);
}

pub(crate) fn matrix_slot_is_live(lease: &MatrixSlotLease) -> bool {
    super::matrix_slots().lock().lifetimes.iter()
        .any(|(name, generation)| name == lease.name() && *generation == lease.lifetime_generation())
}

pub(crate) fn attach_matrix_slot_resource(
    lease: &MatrixSlotLease,
    resource: alloc::sync::Arc<dyn MatrixSlotAttachment>,
) -> Result<(), ()> {
    let mut slots = super::matrix_slots().lock();
    if !slots.lifetimes.iter().any(|(name, generation)|
        name == lease.name() && *generation == lease.lifetime_generation()) {
        return Err(());
    }
    slots.attachments.push((lease.clone(), resource));
    Ok(())
}

/// Callbacks run after dropping the slot registry lock.
pub(super) fn retire_expired_attachments(slots: &mut super::MatrixSlotsState)
    -> alloc::vec::Vec<(MatrixSlotLease, alloc::sync::Arc<dyn MatrixSlotAttachment>)>
{
    let mut retired = alloc::vec::Vec::new();
    let mut index = 0;
    while index < slots.attachments.len() {
        let lease = &slots.attachments[index].0;
        if slots.lifetimes.iter().any(|(name, generation)|
            name == lease.name() && *generation == lease.lifetime_generation()) {
            index += 1;
        } else {
            retired.push(slots.attachments.remove(index));
        }
    }
    retired
}

pub(crate) const TRANSPORT_NET_TCP_SCOPE: u8 = 1;
pub(crate) const TRANSPORT_LOCAL_SCOPE: u8 = 2;

/// Report host-service output without changing terminal ownership.
pub(crate) fn matrix_target_print_line(target: &MatrixTarget, text: &str) {
    super::MatrixSlots::echo_output(&target.lease, text.into());
}
