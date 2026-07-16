//! Intel implementation of TRUEOS's physical GPU contract.

extern crate alloc;

use alloc::vec::Vec;
use spin::Mutex;

use crate::gpu::physical::{
    EngineClass, PhysicalAdapterInfo, PhysicalContextDescriptor, PhysicalContextHandle,
    PhysicalGpuDevice, PhysicalGpuError, PhysicalGpuVmHandle, PhysicalSchedulerStatus,
    PhysicalSubmission,
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
            guc_submission: crate::intel::guc_submission::ready(),
        }
    }

    fn ready(&self) -> bool {
        crate::intel::claimed_device().is_some() && crate::intel::guc_submission::ready()
    }

    fn scheduler_status(&self) -> PhysicalSchedulerStatus {
        let status = crate::intel::guc_submission::scheduler_status();
        PhysicalSchedulerStatus {
            context_capacity: status.capacity,
            registered_contexts: status.registered,
            enabled_contexts: status.enabled,
            submissions: status.submissions,
            registrations: status.registrations,
            deregistrations: status.deregistrations,
            failures: status.failures,
        }
    }

    fn create_gpuvm(&self) -> Result<PhysicalGpuVmHandle, PhysicalGpuError> {
        if !self.ready() {
            return Err(PhysicalGpuError::NotReady);
        }
        let ppgtt = crate::intel::ppgtt::SparsePpgtt::new()
            .ok_or(PhysicalGpuError::OutOfMemory)?;
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

    fn register_context(
        &self,
        descriptor: PhysicalContextDescriptor,
    ) -> Result<PhysicalContextHandle, PhysicalGpuError> {
        if descriptor.engine != EngineClass::RenderCompute {
            return Err(PhysicalGpuError::Unsupported);
        }
        if descriptor.gpuvm_root_phys == 0 {
            return Err(PhysicalGpuError::InvalidGpuVm);
        }
        let dev = crate::intel::claimed_device().ok_or(PhysicalGpuError::NotReady)?;
        crate::intel::guc_submission::register_rcs_context(
            dev,
            descriptor.hwlrca_lo,
            descriptor.hwlrca_hi,
        )
        .map(|token| PhysicalContextHandle::from_raw(token.raw()))
        .map_err(|_| PhysicalGpuError::RegisterFailed)
    }

    fn submit_context(
        &self,
        context: PhysicalContextHandle,
    ) -> Result<PhysicalSubmission, PhysicalGpuError> {
        let dev = crate::intel::claimed_device().ok_or(PhysicalGpuError::NotReady)?;
        crate::intel::guc_submission::submit_rcs_context(
            dev,
            crate::intel::guc_submission::GucContextToken::from_raw(context.raw()),
        )
        .map(|submission| PhysicalSubmission {
            context,
            serial: submission.serial,
        })
        .map_err(|_| PhysicalGpuError::SubmitFailed)
    }

    fn destroy_context(&self, context: PhysicalContextHandle) -> Result<(), PhysicalGpuError> {
        let dev = crate::intel::claimed_device().ok_or(PhysicalGpuError::NotReady)?;
        crate::intel::guc_submission::destroy_rcs_context(
            dev,
            crate::intel::guc_submission::GucContextToken::from_raw(context.raw()),
        )
        .map_err(|_| PhysicalGpuError::DestroyFailed)
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
