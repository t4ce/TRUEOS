// VM level lineage — bookkeeping for the recursive virtualization stack.
// Each VM in the tree has a level (0=native host, max=MAX_LEVEL) and an accel tier
// determined by what Intel hardware actually supports at that level.
//
// The "house of cards" is maintained here: if hardware offers nested VMX + VMCS shadowing,
// L1/L2 run Nested mode. Beyond that, we gracefully fall to Para (provider ABI only).

use super::caps::VmxCaps;

pub const MAX_LEVEL: u8 = 10;

/// Virtualization acceleration tier for a given level.
/// Only L0 has physical VMX. Nested uses shadow VMCS where available.
/// Para is pure-software provider ABI — same control plane, no hardware pretense.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccelKind {
    /// L0: owns VMX/SVM hardware, ring -1.
    HwVmx,
    /// L1–L2 (when VMCS shadowing is available): nested VMX via shadow VMCS.
    /// VMREAD/VMWRITE in guest operate on shadow VMCS without exiting to L0.
    Nested,
    /// L3+ or when VMCS shadowing is absent: software provider ABI only.
    Para,
}

impl AccelKind {
    fn for_level(level: u8, caps: &VmxCaps) -> Self {
        match level {
            0 => Self::HwVmx,
            1 | 2 if caps.vmcs_shadowing => Self::Nested,
            _ => Self::Para,
        }
    }
}

/// Error returned when a child VM would exceed MAX_LEVEL.
#[derive(Debug)]
pub struct MaxLevelReached;

/// Lineage record carried in every VM context.
/// Sufficient to reconstruct the tree position after snapshot/restore across hosts.
#[derive(Clone, Copy)]
pub struct VmLineage {
    pub level: u8,
    pub vm_id: u8,
    pub parent_vm_id: Option<u8>,
    /// The topmost host VM that initiated this tree (level-0 vm_id).
    pub root_vm_id: u8,
    /// Incremented each time this VM is restored from a snapshot.
    pub restore_count: u32,
    pub accel: AccelKind,
}

impl VmLineage {
    /// Create a fresh root lineage (level 0, native hardware).
    pub fn root(vm_id: u8) -> Self {
        Self {
            level: 0,
            vm_id,
            parent_vm_id: None,
            root_vm_id: vm_id,
            restore_count: 0,
            accel: AccelKind::HwVmx,
        }
    }

    /// Derive a child lineage one level deeper.
    /// Returns `Err(MaxLevelReached)` at MAX_LEVEL; the caller should surface this to the
    /// shell as a friendly message rather than letting anything fault.
    pub fn child(&self, child_vm_id: u8, caps: &VmxCaps) -> Result<VmLineage, MaxLevelReached> {
        let next = self
            .level
            .checked_add(1)
            .filter(|&l| l <= MAX_LEVEL)
            .ok_or(MaxLevelReached)?;
        Ok(VmLineage {
            level: next,
            vm_id: child_vm_id,
            parent_vm_id: Some(self.vm_id),
            root_vm_id: self.root_vm_id,
            restore_count: 0,
            accel: AccelKind::for_level(next, caps),
        })
    }

    /// Call after every successful snapshot restore.
    pub fn on_restored(&mut self) {
        self.restore_count += 1;
    }

    /// VPID for TLB tagging: level-encoded, guaranteed 1..=65534.
    /// Level 0 VM 0 → VPID 1, no two (level, vm_id) pairs collide within the 10-VM budget.
    pub fn vpid(&self) -> u16 {
        let raw = self.level as u16 * crate::hv::vmm::MAX_VMS as u16 + self.vm_id as u16 + 1;
        raw.clamp(1, 65534)
    }

    pub fn log(&self) {
        crate::log!(
            "hvv-lineage: vm={} level={} parent={:?} root={} restores={} accel={:?} vpid={}",
            self.vm_id,
            self.level,
            self.parent_vm_id,
            self.root_vm_id,
            self.restore_count,
            self.accel,
            self.vpid(),
        );
    }
}
