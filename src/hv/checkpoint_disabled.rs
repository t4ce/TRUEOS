//! ABI shim for Blueprints built against the retired replication experiment.
//! Checkpoints are unavailable; a guest pause request uses live RAM parking.
use super::*;

pub(super) fn reset_prepare_pause(vm: &TrueosVmId) {
    vm.lifecycle_phase
        .store(BLUEPRINT_LIFECYCLE_PHASE_RUNNING, Ordering::Release);
    vm.lifecycle_operation.store(0, Ordering::Release);
    vm.lifecycle_deadline_ms.store(0, Ordering::Release);
    vm.lifecycle_checkpoint_version.store(0, Ordering::Release);
}

pub(super) fn expire_prepare_pause(_: u8, _: &TrueosVmId) {}
pub(super) fn clear_blueprint_replication_checkpoint(_: u8) {}
pub fn blueprint_replication_checkpoint(_: u8) -> Option<BlueprintReplicationCheckpoint> {
    None
}
pub(crate) fn submit_blueprint_replication_checkpoint(_: u8, _: u64, _: u64, _: &[u8]) -> bool {
    false
}
pub(crate) fn take_blueprint_replication_checkpoint(
    _: u8,
) -> Option<BlueprintReplicationCheckpoint> {
    None
}
pub(crate) fn blueprint_prepare_pause(_: u8) -> Option<BlueprintPreparePause> {
    None
}
pub(crate) fn acknowledge_blueprint_ready(
    _: u8,
    _: u64,
    _: u64,
) -> Option<BlueprintReadyDisposition> {
    None
}
pub(crate) fn commit_replicatable_resume(_: u8) {}
pub(super) fn prepare_replicatable_resume(_: u8) {}
pub(super) fn vmexit_is_preserve(_: u8, _: LaunchResult) -> bool {
    false
}
pub(super) fn snapshot_on_preserve_exit(_: u8) {}

pub fn request_blueprint_prepare_pause(
    vm_id: u8,
    reason: BlueprintPauseReason,
) -> Result<bool, StopError> {
    if reason == BlueprintPauseReason::Pause {
        pause(vm_id)
    } else {
        Ok(false)
    }
}

pub(crate) fn prepare_preserve_mode(_: u8, _: PreserveMode) -> Result<bool, StopError> {
    Ok(false)
}
