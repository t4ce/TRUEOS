//! Intel implementation of TRUEOS's physical GPU contract.

extern crate alloc;

use alloc::vec::Vec;
use spin::Mutex;

use crate::gpu::physical::{
    PhysicalAdapterInfo, PhysicalContextDescriptor, PhysicalContextFaultKind,
    PhysicalContextHandle, PhysicalContextPriority, PhysicalGpuDevice, PhysicalGpuError,
    PhysicalGpuFault, PhysicalGpuVmHandle, PhysicalSchedulerStatus, PhysicalSubmission,
};

pub(crate) static INTEL_PHYSICAL_GPU: IntelPhysicalGpuDevice = IntelPhysicalGpuDevice;

pub(crate) struct IntelPhysicalGpuDevice;

struct GpuVmSlot {
    generation: u32,
    ppgtt: Option<crate::intel::ppgtt::SparsePpgtt>,
}

static GPUVMS: Mutex<Vec<GpuVmSlot>> = Mutex::new(Vec::new());

impl PhysicalGpuDevice for IntelPhysicalGpuDevice {
    fn adapter_info(&self) -> PhysicalAdapterInfo {
        let dev = crate::intel::claimed_device();
        PhysicalAdapterInfo {
            name: dev
                .map(|dev| crate::intel::display_device_name(dev.device_id))
                .unwrap_or("intel-unavailable"),
            vendor_id: crate::intel::INTEL_VENDOR_ID,
            device_id: dev.map(|dev| dev.device_id).unwrap_or(0),
            revision_id: dev.map(|dev| dev.revision_id).unwrap_or(0),
            render_compute: dev.is_some(),
            copy: dev.is_some(),
            guc_submission: crate::intel::guc_submission::INTEL_GUC_SCHEDULER.ready(),
        }
    }

    fn ready(&self) -> bool {
        crate::intel::claimed_device().is_some()
            && crate::intel::guc_submission::INTEL_GUC_SCHEDULER.ready()
    }

    fn scheduler_status(&self) -> PhysicalSchedulerStatus {
        let status = crate::intel::guc_submission::INTEL_GUC_SCHEDULER.status();
        PhysicalSchedulerStatus {
            context_capacity: status.capacity,
            registered_contexts: status.registered,
            enabled_contexts: status.enabled,
            submissions: status.submissions,
            registrations: status.registrations,
            deregistrations: status.deregistrations,
            failures: status.failures,
            faulted_contexts: status.faulted,
            quarantined_engine_lanes: status.quarantined_engine_lanes,
            owner_handoffs_pending: status.owner_handoffs_pending,
            memory_cat_faults: status.memory_cat_faults,
            unattributed_faults: status.unattributed_faults,
            lifecycle_timeouts: status.lifecycle_timeouts,
            lifecycle_retries: status.lifecycle_retries,
            gt_faulted: status.gt_faulted,
        }
    }

    fn create_gpuvm(&self) -> Result<PhysicalGpuVmHandle, PhysicalGpuError> {
        if !self.ready() {
            return Err(PhysicalGpuError::NotReady);
        }
        let ppgtt = crate::intel::ppgtt::SparsePpgtt::new().ok_or(PhysicalGpuError::OutOfMemory)?;
        let mut slots = GPUVMS.lock();
        if let Some((slot, record)) = slots
            .iter_mut()
            .enumerate()
            .find(|(_, record)| record.ppgtt.is_none())
        {
            record.generation = record.generation.wrapping_add(1).max(1);
            record.ppgtt = Some(ppgtt);
            return Ok(encode_vm_handle(slot, record.generation));
        }
        slots.push(GpuVmSlot {
            generation: 1,
            ppgtt: Some(ppgtt),
        });
        Ok(encode_vm_handle(slots.len() - 1, 1))
    }

    fn gpuvm_root_phys(&self, vm: PhysicalGpuVmHandle) -> Result<u64, PhysicalGpuError> {
        let slots = GPUVMS.lock();
        let (slot, generation) = decode_vm_handle(vm)?;
        let record = slots.get(slot).ok_or(PhysicalGpuError::InvalidGpuVm)?;
        if record.generation != generation {
            return Err(PhysicalGpuError::InvalidGpuVm);
        }
        record
            .ppgtt
            .as_ref()
            .map(crate::intel::ppgtt::SparsePpgtt::pml4_phys)
            .ok_or(PhysicalGpuError::InvalidGpuVm)
    }

    fn map_gpuvm(
        &self,
        vm: PhysicalGpuVmHandle,
        gpu: u64,
        phys: u64,
        bytes: usize,
    ) -> Result<(), PhysicalGpuError> {
        let mut slots = GPUVMS.lock();
        let (slot, generation) = decode_vm_handle(vm)?;
        let record = slots.get_mut(slot).ok_or(PhysicalGpuError::InvalidGpuVm)?;
        if record.generation != generation {
            return Err(PhysicalGpuError::InvalidGpuVm);
        }
        record
            .ppgtt
            .as_mut()
            .ok_or(PhysicalGpuError::InvalidGpuVm)?
            .map_range(crate::intel::ppgtt::PpgttRange { gpu, phys, bytes })
            .ok_or(PhysicalGpuError::MapFailed)
    }

    fn map_gpuvm_scanout(
        &self,
        vm: PhysicalGpuVmHandle,
        gpu: u64,
        phys: u64,
        bytes: usize,
    ) -> Result<(), PhysicalGpuError> {
        let mut slots = GPUVMS.lock();
        let (slot, generation) = decode_vm_handle(vm)?;
        let record = slots.get_mut(slot).ok_or(PhysicalGpuError::InvalidGpuVm)?;
        if record.generation != generation {
            return Err(PhysicalGpuError::InvalidGpuVm);
        }
        record
            .ppgtt
            .as_mut()
            .ok_or(PhysicalGpuError::InvalidGpuVm)?
            .map_scanout_range(crate::intel::ppgtt::PpgttRange { gpu, phys, bytes })
            .ok_or(PhysicalGpuError::MapFailed)
    }

    fn unmap_gpuvm(
        &self,
        vm: PhysicalGpuVmHandle,
        gpu: u64,
        bytes: usize,
    ) -> Result<(), PhysicalGpuError> {
        let mut slots = GPUVMS.lock();
        let (slot, generation) = decode_vm_handle(vm)?;
        let record = slots.get_mut(slot).ok_or(PhysicalGpuError::InvalidGpuVm)?;
        if record.generation != generation {
            return Err(PhysicalGpuError::InvalidGpuVm);
        }
        record
            .ppgtt
            .as_mut()
            .ok_or(PhysicalGpuError::InvalidGpuVm)?
            .unmap_range(gpu, bytes)
            .ok_or(PhysicalGpuError::UnmapFailed)
    }

    fn destroy_gpuvm(&self, vm: PhysicalGpuVmHandle) -> Result<(), PhysicalGpuError> {
        let mut slots = GPUVMS.lock();
        let (slot, generation) = decode_vm_handle(vm)?;
        let record = slots.get_mut(slot).ok_or(PhysicalGpuError::InvalidGpuVm)?;
        if record.generation != generation || record.ppgtt.is_none() {
            return Err(PhysicalGpuError::InvalidGpuVm);
        }
        record.ppgtt.take();
        Ok(())
    }

    fn verify_gpuvm_pages(
        &self,
        vm: PhysicalGpuVmHandle,
        gpu: u64,
        pages: &[u64],
    ) -> Result<bool, PhysicalGpuError> {
        let slots = GPUVMS.lock();
        let (slot, generation) = decode_vm_handle(vm)?;
        let record = slots.get(slot).ok_or(PhysicalGpuError::InvalidGpuVm)?;
        if record.generation != generation {
            return Err(PhysicalGpuError::InvalidGpuVm);
        }
        let ppgtt = record
            .ppgtt
            .as_ref()
            .ok_or(PhysicalGpuError::InvalidGpuVm)?;
        Ok(pages
            .iter()
            .copied()
            .enumerate()
            .all(|(page, phys)| ppgtt.maps_page(gpu + (page * 4096) as u64, phys)))
    }

    fn register_context(
        &self,
        descriptor: PhysicalContextDescriptor,
        priority: PhysicalContextPriority,
    ) -> Result<PhysicalContextHandle, PhysicalGpuError> {
        if descriptor.gpuvm_root_phys == 0 {
            return Err(PhysicalGpuError::InvalidGpuVm);
        }
        let dev = crate::intel::claimed_device().ok_or(PhysicalGpuError::NotReady)?;
        crate::intel::guc_submission::INTEL_GUC_SCHEDULER
            .register_mediated(
                dev,
                descriptor.engine,
                descriptor.hwlrca_lo,
                descriptor.hwlrca_hi,
                priority,
            )
            .map(|token| PhysicalContextHandle::from_raw(token.raw()))
            .map_err(|_| PhysicalGpuError::RegisterFailed)
    }

    fn submit_context(
        &self,
        context: PhysicalContextHandle,
    ) -> Result<PhysicalSubmission, PhysicalGpuError> {
        let dev = crate::intel::claimed_device().ok_or(PhysicalGpuError::NotReady)?;
        crate::intel::guc_submission::INTEL_GUC_SCHEDULER
            .submit(dev, crate::intel::guc_submission::GucContextToken::from_raw(context.raw()))
            .map(|submission| PhysicalSubmission {
                context,
                serial: submission.serial,
                scheduler_publish_sequence: submission.h2g_publish_sequence,
            })
            .map_err(|_| PhysicalGpuError::SubmitFailed)
    }

    fn destroy_context(&self, context: PhysicalContextHandle) -> Result<(), PhysicalGpuError> {
        let dev = crate::intel::claimed_device().ok_or(PhysicalGpuError::NotReady)?;
        crate::intel::guc_submission::INTEL_GUC_SCHEDULER
            .destroy(dev, crate::intel::guc_submission::GucContextToken::from_raw(context.raw()))
            .map_err(|error| match error {
                crate::intel::guc_submission::GucSubmissionError::DisablePending
                | crate::intel::guc_submission::GucSubmissionError::DeregisterPending
                | crate::intel::guc_submission::GucSubmissionError::DeviceFaulted => {
                    PhysicalGpuError::CompletionTimeout
                }
                _ => PhysicalGpuError::DestroyFailed,
            })
    }

    fn fault_snapshot(&self) -> Vec<PhysicalGpuFault> {
        let snapshot = crate::intel::guc_submission::INTEL_GUC_SCHEDULER.fault_snapshot();
        if snapshot.gt_faulted {
            let mut faults = Vec::with_capacity(1);
            faults.push(PhysicalGpuFault::UnattributedFault {
                events: snapshot.unattributed_faults,
            });
            return faults;
        }
        let mut faults = Vec::with_capacity(snapshot.contexts.len());
        faults.extend(
            snapshot
                .contexts
                .into_iter()
                .map(|fault| PhysicalGpuFault::Context {
                    context: PhysicalContextHandle::from_raw(fault.token.raw()),
                    engine: fault.engine,
                    mediated: matches!(
                        fault.origin,
                        crate::intel::guc_submission::GucContextOrigin::Mediated
                    ),
                    kind: match fault.kind {
                        crate::intel::guc_submission::GucContextFaultKind::MemoryCat => {
                            PhysicalContextFaultKind::MemoryCat
                        }
                        crate::intel::guc_submission::GucContextFaultKind::ContextReset => {
                            PhysicalContextFaultKind::ContextReset
                        }
                        crate::intel::guc_submission::GucContextFaultKind::LifecycleProtocol => {
                            PhysicalContextFaultKind::LifecycleProtocol
                        }
                    },
                    hw_type: fault.hw_type,
                }),
        );
        faults
    }

    fn acknowledge_context_fault(&self, context: PhysicalContextHandle) -> bool {
        crate::intel::guc_submission::INTEL_GUC_SCHEDULER.acknowledge_fault(
            crate::intel::guc_submission::GucContextToken::from_raw(context.raw()),
        )
    }
}

fn encode_vm_handle(slot: usize, generation: u32) -> PhysicalGpuVmHandle {
    PhysicalGpuVmHandle::from_raw(((generation as u64) << 32) | slot as u64 + 1)
}

fn decode_vm_handle(vm: PhysicalGpuVmHandle) -> Result<(usize, u32), PhysicalGpuError> {
    let one_based = vm.raw() as u32;
    if one_based == 0 {
        return Err(PhysicalGpuError::InvalidGpuVm);
    }
    Ok(((one_based - 1) as usize, (vm.raw() >> 32) as u32))
}
