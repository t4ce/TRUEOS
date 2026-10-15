//! Optional legacy checkpoint and replication experiment.
//! The normal VMX lifecycle parks live execution without these envelopes.
use super::snapshot::*;
use super::*;

pub(super) fn reset_prepare_pause(vm: &TrueosVmId) {
    vm.lifecycle_phase
        .store(BLUEPRINT_LIFECYCLE_PHASE_RUNNING, Ordering::Release);
    vm.lifecycle_operation.store(0, Ordering::Release);
    vm.lifecycle_deadline_ms.store(0, Ordering::Release);
    vm.lifecycle_checkpoint_version.store(0, Ordering::Release);
}

pub(super) fn clear_blueprint_replication_checkpoint(vm_id: u8) {
    if let Some(slot) = BLUEPRINT_REPLICATION_CHECKPOINTS.get(vm_id as usize) {
        let _ = slot.lock().take();
    }
}

pub(crate) fn submit_blueprint_replication_checkpoint(
    vm_id: u8,
    operation: u64,
    version: u64,
    bytes: &[u8],
) -> bool {
    let Some(vm) = vm_slot(vm_id) else {
        return false;
    };
    if operation == 0
        || version == 0
        || version > u32::MAX as u64
        || bytes.is_empty()
        || bytes.len() > BLUEPRINT_REPLICATION_CHECKPOINT_MAX_BYTES
        || vm.lifecycle_phase.load(Ordering::Acquire) != BLUEPRINT_LIFECYCLE_PHASE_PREPARE_PAUSE
        || vm.lifecycle_operation.load(Ordering::Acquire) != operation
    {
        return false;
    }
    let Some(slot) = BLUEPRINT_REPLICATION_CHECKPOINTS.get(vm_id as usize) else {
        return false;
    };
    *slot.lock() = Some(BlueprintReplicationCheckpoint {
        operation,
        version,
        bytes: bytes.to_vec(),
    });
    hvlogf(format_args!(
        "hv: vm{} lifecycle: application checkpoint accepted operation={} version={} bytes={}",
        vm_id,
        operation,
        version,
        bytes.len()
    ));
    true
}

pub fn blueprint_replication_checkpoint(vm_id: u8) -> Option<BlueprintReplicationCheckpoint> {
    BLUEPRINT_REPLICATION_CHECKPOINTS
        .get(vm_id as usize)?
        .lock()
        .clone()
}

pub fn install_blueprint_replication_checkpoint(
    vm_id: u8,
    checkpoint: BlueprintReplicationCheckpoint,
) -> bool {
    let Some(slot) = BLUEPRINT_REPLICATION_CHECKPOINTS.get(vm_id as usize) else {
        return false;
    };
    *slot.lock() = Some(checkpoint);
    true
}

pub(crate) fn take_blueprint_replication_checkpoint(
    vm_id: u8,
) -> Option<BlueprintReplicationCheckpoint> {
    BLUEPRINT_REPLICATION_CHECKPOINTS
        .get(vm_id as usize)?
        .lock()
        .take()
}

pub(super) fn expire_prepare_pause(vm_id: u8, vm: &TrueosVmId) {
    if vm.lifecycle_phase.load(Ordering::Acquire) != BLUEPRINT_LIFECYCLE_PHASE_PREPARE_PAUSE {
        return;
    }
    let deadline = vm.lifecycle_deadline_ms.load(Ordering::Acquire);
    if deadline == 0 || lifecycle_now_ms() <= deadline {
        return;
    }
    if vm
        .lifecycle_phase
        .compare_exchange(
            BLUEPRINT_LIFECYCLE_PHASE_PREPARE_PAUSE,
            BLUEPRINT_LIFECYCLE_PHASE_RUNNING,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_ok()
    {
        let operation = vm.lifecycle_operation.swap(0, Ordering::AcqRel);
        vm.lifecycle_deadline_ms.store(0, Ordering::Release);
        hvwarnf(format_args!(
            "hv: vm{} lifecycle: PreparePause operation={} timed out; VM left running",
            vm_id, operation
        ));
    }
}

pub(crate) fn blueprint_prepare_pause(vm_id: u8) -> Option<BlueprintPreparePause> {
    let vm = vm_slot(vm_id)?;
    expire_prepare_pause(vm_id, vm);
    if vm.lifecycle_phase.load(Ordering::Acquire) != BLUEPRINT_LIFECYCLE_PHASE_PREPARE_PAUSE {
        return None;
    }
    Some(BlueprintPreparePause {
        operation: vm.lifecycle_operation.load(Ordering::Acquire),
        deadline_ms: vm.lifecycle_deadline_ms.load(Ordering::Acquire),
        reason: BlueprintPauseReason::from_raw(vm.lifecycle_reason.load(Ordering::Acquire)),
    })
}

pub(crate) fn acknowledge_blueprint_ready(
    vm_id: u8,
    operation: u64,
    checkpoint_version: u64,
) -> Option<BlueprintReadyDisposition> {
    let Some(vm) = vm_slot(vm_id) else {
        return None;
    };
    expire_prepare_pause(vm_id, vm);
    if operation == 0
        || vm.lifecycle_operation.load(Ordering::Acquire) != operation
        || vm
            .lifecycle_phase
            .compare_exchange(
                BLUEPRINT_LIFECYCLE_PHASE_PREPARE_PAUSE,
                BLUEPRINT_LIFECYCLE_PHASE_READY,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_err()
    {
        return None;
    }
    let reason = BlueprintPauseReason::from_raw(vm.lifecycle_reason.load(Ordering::Acquire));
    let disposition = match reason {
        BlueprintPauseReason::Pause => BlueprintReadyDisposition::Pause,
        BlueprintPauseReason::Replicate | BlueprintPauseReason::Migrate => {
            BlueprintReadyDisposition::Snapshot
        }
    };
    vm.lifecycle_checkpoint_version
        .store(checkpoint_version, Ordering::Release);
    match prepare_preserve_mode(vm_id, PreserveMode::Pause) {
        Ok(true) => {
            hvlogf(format_args!(
                "hv: vm{} lifecycle: Ready operation={} checkpoint_version={} reason={:?} disposition={:?}",
                vm_id, operation, checkpoint_version, reason, disposition
            ));
            Some(disposition)
        }
        Ok(false) | Err(_) => {
            reset_prepare_pause(vm);
            None
        }
    }
}

pub fn request_replicatable_pause(vm_id: u8) -> Result<bool, StopError> {
    request_blueprint_prepare_pause(vm_id, BlueprintPauseReason::Pause)
}

pub fn request_replicatable_snapshot(vm_id: u8) -> Result<bool, StopError> {
    request_blueprint_prepare_pause(vm_id, BlueprintPauseReason::Replicate)
}

pub fn request_blueprint_prepare_pause(
    vm_id: u8,
    reason: BlueprintPauseReason,
) -> Result<bool, StopError> {
    let Some(vm) = vm_slot(vm_id) else {
        return Err(StopError::UnsupportedVmId);
    };
    let _control = vm.lifecycle_control.lock();
    if vm.cooperative_stop.forced() {
        return Ok(false);
    }
    let running = vm.running.load(Ordering::Acquire);
    let starting = vm.starting.load(Ordering::Acquire);
    if !running && !starting {
        return Ok(false);
    }
    if !vm.replicatable.load(Ordering::Acquire) {
        return Ok(false);
    }
    expire_prepare_pause(vm_id, vm);
    if vm
        .lifecycle_phase
        .compare_exchange(
            BLUEPRINT_LIFECYCLE_PHASE_RUNNING,
            BLUEPRINT_LIFECYCLE_PHASE_ARMING,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_err()
    {
        return Ok(false);
    }

    clear_blueprint_replication_checkpoint(vm_id);

    let operation = BLUEPRINT_LIFECYCLE_OPERATION_SEQ
        .fetch_add(1, Ordering::AcqRel)
        .max(1);
    let deadline = lifecycle_now_ms().saturating_add(BLUEPRINT_PREPARE_PAUSE_TIMEOUT_MS);
    vm.lifecycle_operation.store(operation, Ordering::Release);
    vm.lifecycle_deadline_ms.store(deadline, Ordering::Release);
    vm.lifecycle_reason.store(reason as u8, Ordering::Release);
    vm.lifecycle_checkpoint_version.store(0, Ordering::Release);
    vm.lifecycle_phase
        .store(BLUEPRINT_LIFECYCLE_PHASE_PREPARE_PAUSE, Ordering::Release);
    hvlogf(format_args!(
        "hv: vm{} lifecycle: PreparePause operation={} reason={:?} deadline_ms={}",
        vm_id, operation, reason, deadline
    ));
    nudge_vm_control(vm_id, crate::hv::control_kick::LifecycleKickAction::Nudge, "prepare-pause");
    Ok(true)
}

pub fn request_preserve(vm_id: u8) -> Result<bool, StopError> {
    request_preserve_mode(vm_id, PreserveMode::Stop)
}

pub fn request_preserve_mode(vm_id: u8, mode: PreserveMode) -> Result<bool, StopError> {
    let Some(vm) = vm_slot(vm_id) else {
        return Err(StopError::UnsupportedVmId);
    };

    let running = vm.running.load(Ordering::Acquire);
    let starting = vm.starting.load(Ordering::Acquire);
    if !running && !starting {
        hvwarnf(format_args!(
            "hv: vm{} lifecycle: preserve ignored mode={:?} (not running)",
            vm_id, mode
        ));
        return Ok(false);
    }

    if !prepare_preserve_mode(vm_id, mode)? {
        return Ok(false);
    }

    vm.preserve_req.store(true, Ordering::Release);
    hvlogf(format_args!("hv: vm{} lifecycle: preserve requested mode={:?}", vm_id, mode));
    let action = match mode {
        PreserveMode::Stop => crate::hv::control_kick::LifecycleKickAction::PreserveStop,
        PreserveMode::Pause => crate::hv::control_kick::LifecycleKickAction::PreservePause,
    };
    nudge_vm_control(vm_id, action, "preserve");
    Ok(true)
}

/// Apply the lifecycle half of a preserve request.
///
/// Host requests call this before arming `preserve_req`; guest VMCALL requests
/// call it while the VM is already stopped at a safe VM-exit boundary. Keeping
/// both entry paths here prevents pause-mode resource retention from drifting.
pub(crate) fn prepare_preserve_mode(vm_id: u8, mode: PreserveMode) -> Result<bool, StopError> {
    let Some(vm) = vm_slot(vm_id) else {
        return Err(StopError::UnsupportedVmId);
    };
    let _control = vm.lifecycle_control.lock();
    if vm.cooperative_stop.forced() {
        return Ok(false);
    }

    vm.pause_store_seq
        .store(crate::hv::store::current_committed_seq(vm_id), Ordering::Release);
    if mode == PreserveMode::Pause {
        if !vm.replicatable.load(Ordering::Acquire) {
            return Ok(false);
        }
        vm.pause_latched.store(true, Ordering::Release);
    }
    // Admission closes at the lifecycle boundary; resource suspension happens
    // only after the VM exits and all native guest jobs have drained.
    crate::r::blocking::close_guest_jobs(vm_id);
    Ok(true)
}

pub(super) fn prepare_replicatable_resume(vm_id: u8) {
    if let Some(vm) = vm_slot(vm_id) {
        if vm
            .resume_prepared
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        reset_prepare_pause(vm);
        if let Some(identity_slot) = BLUEPRINT_INSTANCE_IDENTITIES.get(vm_id as usize) {
            let mut identity = identity_slot.lock();
            if let Some(identity) = identity.as_mut() {
                identity.generation = identity.generation.saturating_add(1);
                let generation_text = alloc::format!("{}", identity.generation);
                if let Some(context_slot) = BLUEPRINT_PROCESS_CONTEXTS.get(vm_id as usize)
                    && let Some(context) = context_slot.lock().as_mut()
                {
                    context.vars.insert(
                        AllocString::from("TRUEOS_APP_GENERATION"),
                        generation_text.clone(),
                    );
                }
                crate::std_abi_shim::refresh_reserved_environment(vm_id,"TRUEOS_APP_GENERATION",&generation_text);
            }
        }
        resume_blueprint_process_context(vm_id);
        crate::r::services::gridpaper_service::resume_owner_lifecycle(vm_id);
    }
}

/// Commit an exact-continuation resume only after the guest has returned from
/// its Ready boundary and requested the resumed identity. Until this
/// handshake, teardown retains the reconstructed state so a failed VM entry
/// remains retryable.
pub(crate) fn commit_replicatable_resume(vm_id: u8) {
    let Some(vm) = vm_slot(vm_id) else {
        return;
    };
    if !vm.resume_prepared.swap(false, Ordering::AcqRel) {
        return;
    }
    vm.pause_latched.store(false, Ordering::Release);
    clear_blueprint_replication_checkpoint(vm_id);
    hvlogf(format_args!("hv: vm{} lifecycle: resume committed at guest identity handshake", vm_id));
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub fn save_snapshot(vm_id: u8) -> Result<usize, SaveError> {
    if vm_slot(vm_id).is_none() {
        return Err(SaveError::UnsupportedVmId);
    }

    let bytes = snapshot_bytes(vm_id)?;
    crate::hv::store::save_bytes(vm_id, bytes).map_err(map_store_save_error)
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub fn restore_snapshot(vm_id: u8) -> Result<usize, RestoreError> {
    if vm_slot(vm_id).is_none() {
        return Err(RestoreError::UnsupportedVmId);
    }
    if !crate::gpu::vgpu::hull_guest_storage_reusable(vm_id) {
        return Err(RestoreError::VgpuQuarantined);
    }

    let bytes = crate::hv::store::load_bytes(vm_id).map_err(map_store_restore_error)?;

    restore_snapshot_bytes(vm_id, bytes.as_slice())?;
    Ok(bytes.len())
}

pub async fn restore_snapshot_async(vm_id: u8) -> Result<usize, RestoreError> {
    if vm_slot(vm_id).is_none() {
        return Err(RestoreError::UnsupportedVmId);
    }
    if !crate::gpu::vgpu::hull_guest_storage_reusable(vm_id) {
        return Err(RestoreError::VgpuQuarantined);
    }

    let bytes = crate::hv::store::load_bytes_async(vm_id)
        .await
        .map_err(map_store_restore_error)?;

    restore_snapshot_bytes(vm_id, bytes.as_slice())?;
    Ok(bytes.len())
}

pub fn restore_persistent_image(
    vm_id: u8,
    image: &crate::hv::store::PersistentVmImage,
    console_target: Option<MatrixTarget>,
) -> Result<usize, RestoreError> {
    if vm_slot(vm_id).is_none() {
        return Err(RestoreError::UnsupportedVmId);
    }
    if !crate::gpu::vgpu::hull_guest_storage_reusable(vm_id) {
        return Err(RestoreError::VgpuQuarantined);
    }
    if image.source_vm_id != vm_id && !cross_principal_snapshot_restore_supported() {
        return Err(RestoreError::BadSnapshot);
    }
    let result = (|| {
        crate::allocators::restore_hv_guest_heap(vm_id, image.guest_heap()).map_err(|reason| {
            hvwarnf(format_args!(
                "hv: vm{} persistent restore failed stage=guest-heap reason={}",
                vm_id, reason
            ));
            RestoreError::BadSnapshot
        })?;
        memory::restore_guest_hull_rw_for_vm(vm_id, image.hull_rw()).map_err(|reason| {
            hvwarnf(format_args!(
                "hv: vm{} persistent restore failed stage=hull-rw reason={}",
                vm_id, reason
            ));
            RestoreError::BadSnapshot
        })?;
        restore_blueprint_portable_state(vm_id, image.blueprint(), console_target).map_err(
            |reason| {
                hvwarnf(format_args!(
                    "hv: vm{} persistent restore failed stage=blueprint reason={}",
                    vm_id, reason
                ));
                RestoreError::BadSnapshot
            },
        )?;
        restore_snapshot_bytes(vm_id, image.snapshot())?;
        memory::rebind_restored_guest_memory_for_vm(vm_id, VmBootMode::Hull).map_err(|reason| {
            hvwarnf(format_args!(
                "hv: vm{} persistent restore failed stage=rebind reason={}",
                vm_id, reason
            ));
            RestoreError::BadSnapshot
        })?;
        Ok(image.snapshot().len())
    })();
    if result.is_err() {
        if let Some(vm) = vm_slot(vm_id) {
            vm.restore_inflight.store(false, Ordering::Release);
        }
        let _ = eject(vm_id);
    }
    result
}

pub fn try_begin_restore(vm_id: u8) -> Result<bool, StopError> {
    let Some(vm) = vm_slot(vm_id) else {
        return Err(StopError::UnsupportedVmId);
    };
    if !crate::gpu::vgpu::hull_guest_storage_reusable(vm_id) {
        return Err(StopError::VgpuQuarantined);
    }
    Ok(vm
        .restore_inflight
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok())
}

pub fn finish_restore(vm_id: u8) {
    if let Some(vm) = vm_slot(vm_id) {
        vm.restore_inflight.store(false, Ordering::Release);
    }
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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
        crate::hv::store::VmStoreError::NoPersistentRoot => {
            SaveError::Io(crate::disc::block::Error::NotReady)
        }
        crate::hv::store::VmStoreError::InvalidName
        | crate::hv::store::VmStoreError::BadEnvelope => {
            SaveError::Io(crate::disc::block::Error::InvalidParam)
        }
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
        crate::hv::store::VmStoreError::NoPersistentRoot => {
            RestoreError::Read(crate::disc::block::Error::NotReady)
        }
        crate::hv::store::VmStoreError::InvalidName
        | crate::hv::store::VmStoreError::BadEnvelope => RestoreError::BadSnapshot,
    }
}

pub(super) fn vmexit_is_preserve(vm_id: u8, lr: LaunchResult) -> bool {
    lr.entered != 0
        && lr.launch_failed == 0
        && vm_slot(vm_id)
            .map(|vm| vm.preserve_exit.load(Ordering::Acquire))
            .unwrap_or(false)
}

pub(super) fn snapshot_on_preserve_exit(vm_id: u8) {
    let saved = match snapshot_bytes(vm_id) {
        Ok(bytes) => match crate::hv::store::save_bytes(vm_id, bytes) {
            Ok(saved) => {
                hvlogf(format_args!(
                    "hv: vm{} reporting: preserve snapshot saved store=warm-arc path={} bytes={}",
                    vm_id,
                    snapshot_path(vm_id).as_str(),
                    saved
                ));
                true
            }
            Err(e) => {
                hvwarnf(format_args!(
                    "hv: vm{} reporting: preserve snapshot save failed ({:?})",
                    vm_id, e
                ));
                false
            }
        },
        Err(e) => {
            hvwarnf(format_args!(
                "hv: vm{} reporting: preserve snapshot bytes failed ({:?})",
                vm_id, e
            ));
            false
        }
    };
    if saved {
        if let Some(vm) = vm_slot(vm_id)
            && !vm.pause_latched.swap(true, Ordering::AcqRel)
        {
            suspend_blueprint_process_context(vm_id);
            hvlogf(format_args!(
                "hv: vm{} lifecycle: successful preserve retained exact continuation state",
                vm_id
            ));
        }
    } else {
        if let Some(vm) = vm_slot(vm_id) {
            if vm.pause_latched.swap(false, Ordering::AcqRel) {
                vm.pause_store_seq.store(0, Ordering::Release);
                reset_prepare_pause(vm);
                hvwarnf(format_args!(
                    "hv: vm{} lifecycle: pause latch released after snapshot failure",
                    vm_id
                ));
            }
        }
    }
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

const BLUEPRINT_PORTABLE_MAGIC: u32 = u32::from_le_bytes(*b"BPS1");
const BLUEPRINT_PORTABLE_VERSION_LEGACY: u32 = 1;
const BLUEPRINT_PORTABLE_VERSION_WITHOUT_UNPACKED_ELF: u32 = 2;
const BLUEPRINT_PORTABLE_VERSION: u32 = 3;
const BLUEPRINT_RELAUNCH_MAGIC: u32 = u32::from_le_bytes(*b"BPR1");
const BLUEPRINT_RELAUNCH_VERSION: u32 = 1;

fn portable_push_u32(out: &mut AllocVec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn portable_push_u64(out: &mut AllocVec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn portable_push_bytes(out: &mut AllocVec<u8>, value: &[u8]) {
    portable_push_u64(out, value.len() as u64);
    out.extend_from_slice(value);
}

fn portable_push_optional_string(out: &mut AllocVec<u8>, value: Option<&str>) {
    match value {
        Some(value) => {
            out.push(1);
            portable_push_bytes(out, value.as_bytes());
        }
        None => out.push(0),
    }
}

/// Serialize the host-owned part of a paused Blueprint. Guest execution state
/// and guest-owned allocations live in the other persistent envelope members.
pub fn snapshot_blueprint_portable_state(vm_id: u8) -> Result<AllocVec<u8>, &'static str> {
    let state = blueprint_launch_snapshot(vm_id).ok_or("blueprint launch state missing")?;
    let surface = BLUEPRINT_PROCESS_CONTEXTS
        .get(vm_id as usize)
        .and_then(|slot| slot.lock().as_ref().map(|context| context.console_surface))
        .ok_or("blueprint process context missing")?;
    let mut out = AllocVec::new();
    portable_push_u32(&mut out, BLUEPRINT_PORTABLE_MAGIC);
    portable_push_u32(&mut out, BLUEPRINT_PORTABLE_VERSION);
    out.push(match surface {
        BlueprintConsoleSurface::Text => 0,
        BlueprintConsoleSurface::Terminal => 1,
    });
    let (exec_start, exec_end) = memory::active_guest_rel_exec_range_for_vm(vm_id)
        .ok_or("blueprint executable range missing")?;
    portable_push_u64(&mut out, exec_start);
    portable_push_u64(&mut out, exec_end);
    portable_push_bytes(&mut out, state.archive.as_bytes());
    portable_push_bytes(&mut out, state.module_bytes.as_slice());
    // The unpacked ELF is immutable and fully derivable from module_bytes.
    // Persist the self-contained Blueprint module once and rebuild the normal
    // runtime representation during restore.
    portable_push_u32(&mut out, state.app_args.len() as u32);
    for arg in state.app_args.iter() {
        portable_push_bytes(&mut out, arg.as_bytes());
    }
    portable_push_bytes(&mut out, state.app_fs_root.as_bytes());
    out.extend_from_slice(&state.identity.instance);
    out.extend_from_slice(&state.identity.lineage);
    portable_push_u64(&mut out, state.identity.generation);
    out.push(state.identity.clone as u8);
    portable_push_optional_string(&mut out, state.identity.name.as_deref());
    portable_push_optional_string(&mut out, state.identity.peer.as_deref());
    Ok(out)
}

fn portable_take_u32(bytes: &[u8], offset: &mut usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    let raw = bytes.get(*offset..end)?.try_into().ok()?;
    *offset = end;
    Some(u32::from_le_bytes(raw))
}

fn portable_take_u64(bytes: &[u8], offset: &mut usize) -> Option<u64> {
    let end = offset.checked_add(8)?;
    let raw = bytes.get(*offset..end)?.try_into().ok()?;
    *offset = end;
    Some(u64::from_le_bytes(raw))
}

fn portable_take_bytes<'a>(bytes: &'a [u8], offset: &mut usize) -> Option<&'a [u8]> {
    let len = usize::try_from(portable_take_u64(bytes, offset)?).ok()?;
    let end = offset.checked_add(len)?;
    let value = bytes.get(*offset..end)?;
    *offset = end;
    Some(value)
}

fn portable_take_string(bytes: &[u8], offset: &mut usize) -> Option<AllocString> {
    Some(AllocString::from(core::str::from_utf8(portable_take_bytes(bytes, offset)?).ok()?))
}

fn portable_take_optional_string(bytes: &[u8], offset: &mut usize) -> Option<Option<AllocString>> {
    let present = *bytes.get(*offset)?;
    *offset += 1;
    match present {
        0 => Some(None),
        1 => portable_take_string(bytes, offset).map(Some),
        _ => None,
    }
}

pub fn snapshot_blueprint_relaunch_state(vm_id: u8) -> Result<AllocVec<u8>, &'static str> {
    let state = blueprint_launch_snapshot(vm_id).ok_or("blueprint launch state missing")?;
    let surface = BLUEPRINT_PROCESS_CONTEXTS
        .get(vm_id as usize)
        .and_then(|slot| slot.lock().as_ref().map(|context| context.console_surface))
        .ok_or("blueprint process context missing")?;
    let mut out = AllocVec::new();
    portable_push_u32(&mut out, BLUEPRINT_RELAUNCH_MAGIC);
    portable_push_u32(&mut out, BLUEPRINT_RELAUNCH_VERSION);
    out.push(match surface {
        BlueprintConsoleSurface::Text => 0,
        BlueprintConsoleSurface::Terminal => 1,
    });
    portable_push_bytes(&mut out, state.archive.as_bytes());
    portable_push_bytes(&mut out, state.module_bytes.as_slice());
    portable_push_u32(&mut out, state.app_args.len() as u32);
    for arg in state.app_args.iter() {
        portable_push_bytes(&mut out, arg.as_bytes());
    }
    portable_push_optional_string(&mut out, state.launch_script.as_deref());
    portable_push_bytes(&mut out, state.app_fs_root.as_bytes());
    out.extend_from_slice(&state.identity.instance);
    out.extend_from_slice(&state.identity.lineage);
    portable_push_u64(&mut out, state.identity.generation);
    out.push(state.identity.clone as u8);
    portable_push_optional_string(&mut out, state.identity.name.as_deref());
    portable_push_optional_string(&mut out, state.identity.peer.as_deref());
    Ok(out)
}

pub(crate) fn decode_blueprint_relaunch_state(
    bytes: &[u8],
) -> Result<BlueprintRelaunchState, &'static str> {
    let mut offset = 0usize;
    if portable_take_u32(bytes, &mut offset) != Some(BLUEPRINT_RELAUNCH_MAGIC)
        || portable_take_u32(bytes, &mut offset) != Some(BLUEPRINT_RELAUNCH_VERSION)
    {
        return Err("bad blueprint relaunch image");
    }
    let console_surface = match bytes.get(offset).copied() {
        Some(0) => BlueprintConsoleSurface::Text,
        Some(1) => BlueprintConsoleSurface::Terminal,
        _ => return Err("bad blueprint relaunch console surface"),
    };
    offset += 1;
    let archive = portable_take_string(bytes, &mut offset).ok_or("blueprint relaunch archive")?;
    let module_bytes = portable_take_bytes(bytes, &mut offset)
        .ok_or("blueprint relaunch module")?
        .to_vec();
    let arg_count =
        portable_take_u32(bytes, &mut offset).ok_or("blueprint relaunch args")? as usize;
    let mut app_args = AllocVec::with_capacity(arg_count);
    for _ in 0..arg_count {
        app_args.push(portable_take_string(bytes, &mut offset).ok_or("blueprint relaunch arg")?);
    }
    let launch_script =
        portable_take_optional_string(bytes, &mut offset).ok_or("blueprint relaunch script")?;
    let app_fs_root =
        portable_take_string(bytes, &mut offset).ok_or("blueprint relaunch fs root")?;
    let instance_end = offset
        .checked_add(16)
        .ok_or("blueprint relaunch instance")?;
    let instance = bytes
        .get(offset..instance_end)
        .and_then(|value| value.try_into().ok())
        .ok_or("blueprint relaunch instance")?;
    offset = instance_end;
    let lineage_end = offset.checked_add(16).ok_or("blueprint relaunch lineage")?;
    let lineage = bytes
        .get(offset..lineage_end)
        .and_then(|value| value.try_into().ok())
        .ok_or("blueprint relaunch lineage")?;
    offset = lineage_end;
    let generation = portable_take_u64(bytes, &mut offset)
        .ok_or("blueprint relaunch generation")?
        .saturating_add(1);
    let clone = match bytes.get(offset).copied() {
        Some(0) => false,
        Some(1) => true,
        _ => return Err("blueprint relaunch clone flag"),
    };
    offset += 1;
    let name =
        portable_take_optional_string(bytes, &mut offset).ok_or("blueprint relaunch name")?;
    let peer =
        portable_take_optional_string(bytes, &mut offset).ok_or("blueprint relaunch peer")?;
    if offset != bytes.len() {
        return Err("trailing blueprint relaunch bytes");
    }
    Ok(BlueprintRelaunchState {
        archive,
        module_bytes,
        app_args,
        launch_script,
        app_fs_root,
        identity: BlueprintInstanceIdentity {
            instance,
            lineage,
            generation,
            clone,
            name,
            peer,
        },
        console_surface,
    })
}

pub(crate) fn start_blueprint_replica_vm(
    vm_id: u8,
    spawner: &Spawner,
    state: BlueprintRelaunchState,
    checkpoint: BlueprintReplicationCheckpoint,
) -> Result<(), StartError> {
    let _ = spawner;
    if !install_blueprint_replication_checkpoint(vm_id, checkpoint) {
        return Err(StartError::UnsupportedVmId);
    }
    let instance = BlueprintInstanceRequest {
        name: state.identity.name.clone(),
        peer: state.identity.peer.clone(),
    };
    let result = start_with_mode(
        vm_id,
        VmBootMode::Hull,
        None,
        Some(BlueprintPendingLaunchState {
            archive: state.archive,
            module_bytes: state.module_bytes,
            app_args: state.app_args,
            launch_script: state.launch_script,
            instance,
            console_target: None,
            console_surface: state.console_surface,
            restored_identity: Some(state.identity),
            restored_app_fs_root: Some(state.app_fs_root),
        }),
        false,
    );
    if result.is_err() {
        clear_blueprint_replication_checkpoint(vm_id);
    }
    result
}

pub fn restore_blueprint_portable_state(
    vm_id: u8,
    bytes: &[u8],
    console_target: Option<MatrixTarget>,
) -> Result<(), &'static str> {
    let vm = vm_slot(vm_id).ok_or("unsupported vm id")?;
    if vm.running.load(Ordering::Acquire)
        || vm.starting.load(Ordering::Acquire)
        || blueprint_launch_active(vm_id)
    {
        return Err("vm slot is busy");
    }
    let mut offset = 0usize;
    if portable_take_u32(bytes, &mut offset) != Some(BLUEPRINT_PORTABLE_MAGIC) {
        return Err("bad blueprint state image");
    }
    let version = portable_take_u32(bytes, &mut offset).ok_or("blueprint state version")?;
    if !matches!(
        version,
        BLUEPRINT_PORTABLE_VERSION_LEGACY
            | BLUEPRINT_PORTABLE_VERSION_WITHOUT_UNPACKED_ELF
            | BLUEPRINT_PORTABLE_VERSION
    ) {
        return Err("bad blueprint state image");
    }
    let surface = match bytes.get(offset).copied() {
        Some(0) => BlueprintConsoleSurface::Text,
        Some(1) => BlueprintConsoleSurface::Terminal,
        _ => return Err("bad blueprint console surface"),
    };
    offset += 1;
    let exec_range = if version == BLUEPRINT_PORTABLE_VERSION {
        Some((
            portable_take_u64(bytes, &mut offset).ok_or("blueprint executable start")?,
            portable_take_u64(bytes, &mut offset).ok_or("blueprint executable end")?,
        ))
    } else {
        None
    };
    let archive = portable_take_string(bytes, &mut offset).ok_or("blueprint archive")?;
    let module_bytes = portable_take_bytes(bytes, &mut offset)
        .ok_or("blueprint module")?
        .to_vec();
    let unpacked_bytes = if version == BLUEPRINT_PORTABLE_VERSION_LEGACY {
        portable_take_bytes(bytes, &mut offset)
            .ok_or("blueprint payload")?
            .to_vec()
    } else {
        let module = crate::hv::blueprint::parse_blueprint(module_bytes.as_slice())
            .map_err(|_| "blueprint module decode")?;
        crate::hv::blueprint::unpack_blueprint(&module).map_err(|_| "blueprint payload decode")?
    };
    let arg_count = portable_take_u32(bytes, &mut offset).ok_or("blueprint args")? as usize;
    let mut app_args = AllocVec::with_capacity(arg_count);
    for _ in 0..arg_count {
        app_args.push(portable_take_string(bytes, &mut offset).ok_or("blueprint arg")?);
    }
    let app_fs_root = portable_take_string(bytes, &mut offset).ok_or("blueprint fs root")?;
    let identity_end = offset.checked_add(16).ok_or("blueprint instance")?;
    let instance: [u8; 16] = bytes
        .get(offset..identity_end)
        .and_then(|value| value.try_into().ok())
        .ok_or("blueprint instance")?;
    offset = identity_end;
    let lineage_end = offset.checked_add(16).ok_or("blueprint lineage")?;
    let lineage: [u8; 16] = bytes
        .get(offset..lineage_end)
        .and_then(|value| value.try_into().ok())
        .ok_or("blueprint lineage")?;
    offset = lineage_end;
    let generation = portable_take_u64(bytes, &mut offset).ok_or("blueprint generation")?;
    let clone = match bytes.get(offset).copied() {
        Some(0) => false,
        Some(1) => true,
        _ => return Err("blueprint clone flag"),
    };
    offset += 1;
    let name = portable_take_optional_string(bytes, &mut offset).ok_or("blueprint name")?;
    let peer = portable_take_optional_string(bytes, &mut offset).ok_or("blueprint peer")?;
    if offset != bytes.len() {
        return Err("trailing blueprint state bytes");
    }
    let identity = BlueprintInstanceIdentity {
        instance,
        lineage,
        generation,
        clone,
        name,
        peer,
    };
    if !memory::arm_guest_rel_exec_for_vm(vm_id) {
        return Err("blueprint execute capability unavailable");
    }
    let Some((exec_start, exec_end)) = exec_range else {
        memory::release_guest_rel_exec_for_vm(vm_id);
        return Err("blueprint state predates executable-range persistence");
    };
    if memory::restore_guest_rel_exec_range_for_vm(vm_id, exec_start, exec_end).is_err() {
        memory::release_guest_rel_exec_for_vm(vm_id);
        return Err("blueprint executable range restore failed");
    }
    let state = BlueprintLaunchState {
        archive: archive.clone(),
        module_bytes,
        unpacked_bytes,
        app_args,
        launch_script: None,
        app_fs_root,
        identity: identity.clone(),
    };
    if stage_blueprint_launch(vm_id, state, console_target, surface).is_err() {
        memory::release_guest_rel_exec_for_vm(vm_id);
        return Err("blueprint process reconstruction failed");
    }
    if let Some(slot) = BLUEPRINT_INSTANCE_IDENTITIES.get(vm_id as usize) {
        *slot.lock() = Some(identity);
    }
    if let Some(mode) = VM_BOOT_MODES.get(vm_id as usize) {
        *mode.lock() = VmBootMode::Hull;
    }
    set_blueprint_lifecycle_capability(vm_id, archive.as_str(), true);
    vm.pause_store_seq
        .store(crate::hv::store::current_committed_seq(vm_id), Ordering::Release);
    vm.pause_latched.store(true, Ordering::Release);
    vm.resume_prepared.store(false, Ordering::Release);
    suspend_blueprint_process_context(vm_id);
    Ok(())
}
