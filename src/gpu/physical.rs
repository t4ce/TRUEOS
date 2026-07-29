//! Transport-independent physical GPU contract.
//!
//! Virtual devices use this interface without learning Intel MMIO addresses,
//! GuC CTB details, physical pages, or native context identifiers.

use spin::Mutex;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum EngineClass {
    RenderCompute,
    VideoDecode,
    Copy,
}

/// One physical engine, including its hardware instance within the class.
///
/// Keeping the instance in the transport-independent contract prevents two
/// contexts aimed at different VDBOXes from collapsing onto the GuC class
/// default. Integrated Xe-LP platforms commonly expose their second VDBOX as
/// physical instance 2 rather than instance 1.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct PhysicalEngineId {
    pub(crate) class: EngineClass,
    pub(crate) instance: u8,
}

impl PhysicalEngineId {
    pub(crate) const RCS0: Self = Self {
        class: EngineClass::RenderCompute,
        instance: 0,
    };
    pub(crate) const VCS0: Self = Self {
        class: EngineClass::VideoDecode,
        instance: 0,
    };
    pub(crate) const BCS0: Self = Self {
        class: EngineClass::Copy,
        instance: 0,
    };

    pub(crate) const fn video(instance: u8) -> Self {
        Self {
            class: EngineClass::VideoDecode,
            instance,
        }
    }
}

/// Physical scheduler priority for one persistent kernel context.
///
/// Display-critical work is intentionally a separate class from ordinary
/// kernel GPU work so a continuously active compute context cannot add a full
/// scheduler rotation to scanout-facing submissions.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum PhysicalContextPriority {
    KernelHigh,
    KernelNormal,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct PhysicalAdapterInfo {
    pub(crate) name: &'static str,
    pub(crate) vendor_id: u16,
    pub(crate) device_id: u16,
    pub(crate) revision_id: u8,
    pub(crate) render_compute: bool,
    pub(crate) copy: bool,
    pub(crate) guc_submission: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub(crate) struct PhysicalGpuVmHandle(u64);

impl PhysicalGpuVmHandle {
    pub(crate) const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    pub(crate) const fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub(crate) struct PhysicalContextHandle(u64);

impl PhysicalContextHandle {
    pub(crate) const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    pub(crate) const fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct PhysicalContextDescriptor {
    pub(crate) engine: PhysicalEngineId,
    pub(crate) hwlrca_lo: u32,
    pub(crate) hwlrca_hi: u32,
    pub(crate) gpuvm_root_phys: u64,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct PhysicalSubmission {
    pub(crate) context: PhysicalContextHandle,
    pub(crate) serial: u64,
    pub(crate) scheduler_publish_sequence: u64,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct PhysicalBufferSlice {
    pub(crate) gpu: u64,
    pub(crate) bytes: usize,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct PhysicalSceneAabbRequest {
    pub(crate) vm: PhysicalGpuVmHandle,
    pub(crate) bounds: [PhysicalBufferSlice; 6],
    pub(crate) liveness: PhysicalBufferSlice,
    pub(crate) output: PhysicalBufferSlice,
    pub(crate) rows: u32,
    pub(crate) query_min: [f32; 3],
    pub(crate) query_max: [f32; 3],
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct PhysicalSceneAabbCompletion {
    pub(crate) serial: u64,
    pub(crate) hits: u32,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct PhysicalSchedulerStatus {
    pub(crate) context_capacity: usize,
    pub(crate) registered_contexts: usize,
    pub(crate) enabled_contexts: usize,
    pub(crate) submissions: u64,
    pub(crate) registrations: u64,
    pub(crate) deregistrations: u64,
    pub(crate) failures: u64,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum PhysicalGpuError {
    NotReady,
    Unsupported,
    OutOfMemory,
    InvalidGpuVm,
    InvalidContext,
    MapFailed,
    UnmapFailed,
    RegisterFailed,
    SubmitFailed,
    DestroyFailed,
    CompletionTimeout,
}

impl PhysicalGpuError {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::NotReady => "not-ready",
            Self::Unsupported => "unsupported",
            Self::OutOfMemory => "out-of-memory",
            Self::InvalidGpuVm => "invalid-gpuvm",
            Self::InvalidContext => "invalid-context",
            Self::MapFailed => "map-failed",
            Self::UnmapFailed => "unmap-failed",
            Self::RegisterFailed => "register-failed",
            Self::SubmitFailed => "submit-failed",
            Self::DestroyFailed => "destroy-failed",
            Self::CompletionTimeout => "completion-timeout",
        }
    }
}

pub(crate) trait PhysicalGpuDevice: Sync {
    fn adapter_info(&self) -> PhysicalAdapterInfo;
    fn ready(&self) -> bool;
    fn scheduler_status(&self) -> PhysicalSchedulerStatus;

    fn create_gpuvm(&self) -> Result<PhysicalGpuVmHandle, PhysicalGpuError>;
    fn gpuvm_root_phys(&self, vm: PhysicalGpuVmHandle) -> Result<u64, PhysicalGpuError>;
    fn map_gpuvm(
        &self,
        vm: PhysicalGpuVmHandle,
        gpu: u64,
        phys: u64,
        bytes: usize,
    ) -> Result<(), PhysicalGpuError>;
    fn unmap_gpuvm(
        &self,
        vm: PhysicalGpuVmHandle,
        gpu: u64,
        bytes: usize,
    ) -> Result<(), PhysicalGpuError>;
    fn destroy_gpuvm(&self, vm: PhysicalGpuVmHandle) -> Result<(), PhysicalGpuError>;
    fn verify_gpuvm_pages(
        &self,
        vm: PhysicalGpuVmHandle,
        gpu: u64,
        pages: &[u64],
    ) -> Result<bool, PhysicalGpuError>;

    fn submit_scene_aabb(
        &self,
        request: PhysicalSceneAabbRequest,
    ) -> Result<PhysicalSceneAabbCompletion, PhysicalGpuError>;

    fn register_context(
        &self,
        descriptor: PhysicalContextDescriptor,
        priority: PhysicalContextPriority,
    ) -> Result<PhysicalContextHandle, PhysicalGpuError>;
    fn submit_context(
        &self,
        context: PhysicalContextHandle,
    ) -> Result<PhysicalSubmission, PhysicalGpuError>;
    fn destroy_context(&self, context: PhysicalContextHandle) -> Result<(), PhysicalGpuError>;
}

static DEVICE: Mutex<Option<&'static dyn PhysicalGpuDevice>> = Mutex::new(None);

pub(crate) fn register_physical_device(device: &'static dyn PhysicalGpuDevice) -> bool {
    let mut slot = DEVICE.lock();
    if slot.is_some() {
        return false;
    }
    *slot = Some(device);
    true
}

pub(crate) fn physical_device() -> Option<&'static dyn PhysicalGpuDevice> {
    *DEVICE.lock()
}
