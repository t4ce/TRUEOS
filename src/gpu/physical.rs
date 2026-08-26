//! Transport-independent physical GPU contract.
//!
//! Virtual devices use this interface without learning Intel MMIO addresses,
//! GuC CTB details, physical pages, or native context identifiers.

extern crate alloc;

use alloc::vec::Vec;
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

/// Transport-independent physical fault report. Context handles retain their
/// backend generation tag so upper layers cannot quarantine a reused slot.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum PhysicalContextFaultKind {
    MemoryCat,
    ContextReset,
    LifecycleProtocol,
}

impl PhysicalContextFaultKind {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::MemoryCat => "memory-cat",
            Self::ContextReset => "context-reset",
            Self::LifecycleProtocol => "lifecycle-protocol",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum PhysicalGpuFault {
    Context {
        context: PhysicalContextHandle,
        engine: PhysicalEngineId,
        /// True when the context was registered through this physical vGPU
        /// boundary; false denotes a backend-internal/direct scheduler user.
        mediated: bool,
        kind: PhysicalContextFaultKind,
        /// Opaque platform-defined telemetry; never an engine selector.
        hw_type: Option<u32>,
    },
    UnattributedFault {
        events: u64,
    },
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) struct PhysicalBufferSlice {
    pub(crate) gpu: u64,
    pub(crate) bytes: usize,
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
    pub(crate) faulted_contexts: usize,
    /// Driver-defined physical-engine lane bits quarantined until GT reset.
    pub(crate) quarantined_engine_lanes: u32,
    pub(crate) owner_handoffs_pending: usize,
    pub(crate) memory_cat_faults: u64,
    pub(crate) unattributed_faults: u64,
    pub(crate) lifecycle_timeouts: u64,
    pub(crate) lifecycle_retries: u64,
    pub(crate) gt_faulted: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum PhysicalGpuError {
    NotReady,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    Unsupported,
    OutOfMemory,
    InvalidGpuVm,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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
    /// Map a producer surface that will be read directly by scanout.  This is
    /// deliberately separate from `map_gpuvm`: on Gen12 the leaf PTE must use
    /// PAT3/UC, while ordinary carrier resources remain PAT0/WB.
    fn map_gpuvm_scanout(
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
    /// Return sticky fault state. The mediated pump calls this while holding
    /// its broker lock, establishing the canonical BROKER -> backend order;
    /// implementations must never call back into broker or executor code.
    fn fault_snapshot(&self) -> Vec<PhysicalGpuFault>;
    /// Acknowledge that the mediated ownership layer quarantined this exact
    /// generation-tagged context. This must not reset hardware or release its
    /// registration/backing.
    fn acknowledge_context_fault(&self, context: PhysicalContextHandle) -> bool;
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
