//! Software-mediated virtual GPU devices.
//!
//! This is TRUEOS's policy and ownership boundary.  Every caller receives a
//! principal-scoped, generation-tagged device with its own GPUVM, resource
//! handles, queues, quotas, and virtual timelines.  Existing kernel render and
//! GPGPU contexts are adopted as privileged devices while retaining their
//! validated LRC/ring/PPGTT layouts.

extern crate alloc;

use alloc::vec::Vec;
use spin::Mutex;

use super::physical::{
    PhysicalBufferSlice, PhysicalContextDescriptor, PhysicalContextHandle, PhysicalContextPriority,
    PhysicalGpuDevice, PhysicalGpuError, PhysicalGpuVmHandle, PhysicalSceneAabbRequest,
    PhysicalSchedulerStatus, physical_device,
};

const PAGE_BYTES: usize = 4096;
const CLIENT_GPU_VA_BASE: u64 = 0x1_0000_0000;
const CLIENT_GPU_VA_LIMIT: u64 = 0x0000_7FFF_0000_0000;
pub(crate) const BUFFER_USAGE_MAP_READ: u32 = 1 << 0;
pub(crate) const BUFFER_USAGE_MAP_WRITE: u32 = 1 << 1;
pub(crate) const BUFFER_USAGE_STORAGE: u32 = 1 << 2;
pub(crate) const BUFFER_USAGE_COPY_SRC: u32 = 1 << 3;
pub(crate) const BUFFER_USAGE_COPY_DST: u32 = 1 << 4;
pub(crate) const BUFFER_INFO_FLAG_VVIDEO_MEM: u32 = 1 << 0;
const BUFFER_USAGE_ALL: u32 = BUFFER_USAGE_MAP_READ
    | BUFFER_USAGE_MAP_WRITE
    | BUFFER_USAGE_STORAGE
    | BUFFER_USAGE_COPY_SRC
    | BUFFER_USAGE_COPY_DST;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct Capabilities(u64);

impl Capabilities {
    pub(crate) const BUFFER: Self = Self(1 << 0);
    pub(crate) const QUEUE: Self = Self(1 << 1);
    pub(crate) const TIMELINE: Self = Self(1 << 2);
    pub(crate) const COMPUTE: Self = Self(1 << 3);
    pub(crate) const RENDER: Self = Self(1 << 4);
    pub(crate) const COPY: Self = Self(1 << 5);
    pub(crate) const PRESENT: Self = Self(1 << 6);
    pub(crate) const KERNEL_CONTEXT: Self = Self(1 << 63);
    pub(crate) const CLIENT_BASE: Self =
        Self(Self::BUFFER.0 | Self::QUEUE.0 | Self::TIMELINE.0 | Self::COMPUTE.0 | Self::RENDER.0);

    pub(crate) const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

    pub(crate) const fn bits(self) -> u64 {
        self.0
    }

    pub(crate) const fn contains(self, required: Self) -> bool {
        self.0 & required.0 == required.0
    }

    const fn intersect(self, allowed: Self) -> Self {
        Self(self.0 & allowed.0)
    }

    const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum KernelClient {
    /// Legacy/default Render carrier. Its identity is permanently bound to
    /// one RCS0 HWLRCA and one PPGTT root on first admission.
    Render,
    /// Additional Render carriers are distinct principals, not aliases for
    /// the legacy Render context. Callers may use them only with independently
    /// allocated LRC/ring/batch/result storage and a distinct PPGTT root.
    Render1,
    Render2,
    /// Ordered compute lane for kernel system services such as fonts, retained
    /// UI producers, and general-purpose synchronous operations.
    GpgpuSystem,
    /// Independent compute lane for continuously executing GPU programs. Its
    /// context may remain in flight without blocking system-service compute.
    GpgpuExecution,
    /// Fixed-model compute lane with its own persistent PPGTT and GuC context.
    Lfm25,
    /// Persistent UI4 composition queue.  This is deliberately a separate
    /// virtual device/principal from general kernel GPGPU: UI4 may leave one
    /// frame in flight while video conversion, fonts, and application compute
    /// continue to submit through `GpgpuSystem`.
    Ui4Compositor,
    /// Persistent GuC-owned BCS0 lane for UI4 copies and composition staging.
    /// Keeping it separate from the RCS compositor lane gives copy work its
    /// own backpressure and completion timeline.
    Ui4Blitter,
}

impl KernelClient {
    pub(crate) const RENDER_CARRIERS: [Self; 3] = [Self::Render, Self::Render1, Self::Render2];

    pub(crate) const fn render_carrier(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::Render),
            1 => Some(Self::Render1),
            2 => Some(Self::Render2),
            _ => None,
        }
    }

    pub(crate) const fn render_carrier_index(self) -> Option<usize> {
        match self {
            Self::Render => Some(0),
            Self::Render1 => Some(1),
            Self::Render2 => Some(2),
            _ => None,
        }
    }

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Render => "kernel-render-0",
            Self::Render1 => "kernel-render-1",
            Self::Render2 => "kernel-render-2",
            Self::GpgpuSystem => "kernel-gpgpu-system",
            Self::GpgpuExecution => "kernel-gpgpu-execution",
            Self::Lfm25 => "kernel-lfm25",
            Self::Ui4Compositor => "kernel-ui4-compositor",
            Self::Ui4Blitter => "kernel-ui4-blitter",
        }
    }

    const fn principal(self) -> Principal {
        match self {
            Self::Render => Principal::KernelRender,
            Self::Render1 => Principal::KernelRender1,
            Self::Render2 => Principal::KernelRender2,
            Self::GpgpuSystem => Principal::KernelGpgpuSystem,
            Self::GpgpuExecution => Principal::KernelGpgpuExecution,
            Self::Lfm25 => Principal::KernelLfm25,
            Self::Ui4Compositor => Principal::KernelUi4Compositor,
            Self::Ui4Blitter => Principal::KernelUi4Blitter,
        }
    }

    const fn queue_class(self) -> QueueClass {
        match self {
            Self::Render | Self::Render1 | Self::Render2 => QueueClass::Render,
            Self::GpgpuSystem | Self::GpgpuExecution | Self::Lfm25 => QueueClass::Compute,
            Self::Ui4Compositor => QueueClass::Compute,
            Self::Ui4Blitter => QueueClass::Copy,
        }
    }

    const fn physical_priority(self) -> PhysicalContextPriority {
        match self {
            // This context feeds a visible video frame into UI4.  It must be
            // able to preempt ordinary persistent GPGPU work instead of
            // waiting through an entire GuC scheduler rotation.
            //
            // LFM submissions are likewise bounded interactive batches. A
            // normal-priority context added one repeatable scheduler quantum
            // to every model projection on ADL-S, overwhelming the actual
            // kernel time. Each LFM batch remains bounded to at most three
            // projection walkers, so it cannot turn into a persistent
            // high-priority program.
            // GPGPU system submissions are likewise bounded synchronous
            // kernels. They produce visible retained UI surfaces (Gridpaper
            // and GPU fonts), so leaving them at normal priority adds one
            // complete GuC scheduler quantum to every copy/coverage/release
            // stage while UI4 window motion itself remains crisp.
            // The execution lane is currently exclusive to Spirit VFX and
            // Lab256. Both are bounded 256x256, single-pending producers; the
            // GuC policy gives every context a 1 ms execution quantum, so this
            // scanout-facing lane can be high priority without admitting an
            // unbounded high-priority queue.
            Self::GpgpuSystem | Self::GpgpuExecution | Self::Ui4Compositor | Self::Lfm25 => {
                PhysicalContextPriority::KernelHigh
            }
            _ => PhysicalContextPriority::KernelNormal,
        }
    }

    /// Current physical ABI placement for privileged kernel contexts. ADL-S
    /// exposes RCS0 for render/compute and BCS0 for copies; there is no fake
    /// EU/CCS affinity hidden behind a software carrier number.
    const fn accepts_engine(self, engine: super::physical::PhysicalEngineId) -> bool {
        use super::physical::EngineClass;

        match self {
            Self::Render
            | Self::Render1
            | Self::Render2
            | Self::GpgpuSystem
            | Self::GpgpuExecution
            | Self::Lfm25
            | Self::Ui4Compositor => {
                matches!(engine.class, EngineClass::RenderCompute) && engine.instance == 0
            }
            Self::Ui4Blitter => matches!(engine.class, EngineClass::Copy) && engine.instance == 0,
        }
    }
}

const _: () = {
    assert!(matches!(
        KernelClient::Ui4Compositor.physical_priority(),
        PhysicalContextPriority::KernelHigh
    ));
    assert!(matches!(
        KernelClient::Render.physical_priority(),
        PhysicalContextPriority::KernelNormal
    ));
    assert!(matches!(
        KernelClient::GpgpuSystem.physical_priority(),
        PhysicalContextPriority::KernelHigh
    ));
    assert!(matches!(
        KernelClient::GpgpuExecution.physical_priority(),
        PhysicalContextPriority::KernelHigh
    ));
    assert!(matches!(KernelClient::Lfm25.physical_priority(), PhysicalContextPriority::KernelHigh));
    assert!(matches!(
        KernelClient::Ui4Blitter.physical_priority(),
        PhysicalContextPriority::KernelNormal
    ));
};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Principal {
    KernelRender,
    KernelRender1,
    KernelRender2,
    KernelGpgpuSystem,
    KernelGpgpuExecution,
    KernelLfm25,
    KernelUi4Compositor,
    KernelUi4Blitter,
    HostRuntime,
    HullGuest(u16),
    RuntimeTest(u16),
}

impl Principal {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::KernelRender => "kernel-render-0",
            Self::KernelRender1 => "kernel-render-1",
            Self::KernelRender2 => "kernel-render-2",
            Self::KernelGpgpuSystem => "kernel-gpgpu-system",
            Self::KernelGpgpuExecution => "kernel-gpgpu-execution",
            Self::KernelLfm25 => "kernel-lfm25",
            Self::KernelUi4Compositor => "kernel-ui4-compositor",
            Self::KernelUi4Blitter => "kernel-ui4-blitter",
            Self::HostRuntime => "host-runtime",
            Self::HullGuest(_) => "hull-guest",
            Self::RuntimeTest(_) => "runtime-test",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum QueueClass {
    Render,
    Compute,
    Copy,
}

impl QueueClass {
    pub(crate) const fn from_raw(raw: u32) -> Option<Self> {
        match raw {
            1 => Some(Self::Render),
            2 => Some(Self::Compute),
            3 => Some(Self::Copy),
            _ => None,
        }
    }

    pub(crate) const fn raw(self) -> u32 {
        match self {
            Self::Render => 1,
            Self::Compute => 2,
            Self::Copy => 3,
        }
    }

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Render => "render",
            Self::Compute => "compute",
            Self::Copy => "copy",
        }
    }

    const fn capability(self) -> Capabilities {
        match self {
            Self::Render => Capabilities::RENDER,
            Self::Compute => Capabilities::COMPUTE,
            Self::Copy => Capabilities::COPY,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub(crate) struct DeviceHandle(u64);

impl DeviceHandle {
    pub(crate) const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    pub(crate) const fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub(crate) struct BufferHandle(u64);

impl BufferHandle {
    pub(crate) const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    pub(crate) const fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub(crate) struct QueueHandle(u64);

impl QueueHandle {
    pub(crate) const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    pub(crate) const fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct TimelinePoint {
    pub(crate) queue: QueueHandle,
    pub(crate) value: u64,
    pub(crate) physical_serial: u64,
    pub(crate) physical_publish_sequence: u64,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct TimelineStatus {
    pub(crate) submitted: u64,
    pub(crate) completed: u64,
    pub(crate) failures: u64,
    pub(crate) last_physical_serial: u64,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct BufferInfo {
    pub(crate) bytes: usize,
    pub(crate) usage: u32,
    pub(crate) flags: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct BufferSlice {
    pub(crate) buffer: BufferHandle,
    pub(crate) offset: usize,
    pub(crate) bytes: usize,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct SceneAabbDispatch {
    pub(crate) bounds: [BufferSlice; 6],
    pub(crate) liveness: BufferSlice,
    pub(crate) output: BufferSlice,
    pub(crate) rows: u32,
    pub(crate) query_min: [f32; 3],
    pub(crate) query_max: [f32; 3],
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct SceneAabbResult {
    pub(crate) point: TimelinePoint,
    pub(crate) hits: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct DeviceInfo {
    pub(crate) capabilities: Capabilities,
    pub(crate) epoch: u64,
    pub(crate) memory_used: usize,
    pub(crate) memory_quota: usize,
    pub(crate) buffer_count: usize,
    pub(crate) queue_count: usize,
    pub(crate) lost: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct DeviceDiagnostics {
    pub(crate) copied_upload_bytes: u64,
    pub(crate) flushed_vvideo_bytes: u64,
    pub(crate) mapping_digest: u64,
    pub(crate) vvideo_buffers: usize,
    pub(crate) mapping_identity: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum VgpuError {
    NoPhysicalDevice,
    DeviceNotReady,
    InvalidHandle,
    PermissionDenied,
    DeviceLost,
    Unsupported,
    QuotaExceeded,
    OutOfMemory,
    Busy,
    NotComplete,
    Physical(PhysicalGpuError),
}

impl VgpuError {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::NoPhysicalDevice => "no-physical-device",
            Self::DeviceNotReady => "device-not-ready",
            Self::InvalidHandle => "invalid-handle",
            Self::PermissionDenied => "permission-denied",
            Self::DeviceLost => "device-lost",
            Self::Unsupported => "unsupported",
            Self::QuotaExceeded => "quota-exceeded",
            Self::OutOfMemory => "out-of-memory",
            Self::Busy => "busy",
            Self::NotComplete => "not-complete",
            Self::Physical(error) => error.name(),
        }
    }

    pub(crate) const fn errno(self) -> i32 {
        match self {
            Self::NoPhysicalDevice | Self::DeviceNotReady => -19,
            Self::InvalidHandle => -9,
            Self::PermissionDenied => -13,
            Self::DeviceLost => -32,
            Self::Unsupported => -95,
            Self::QuotaExceeded | Self::OutOfMemory => -12,
            Self::Busy | Self::NotComplete => -16,
            Self::Physical(_) => -5,
        }
    }
}

impl From<PhysicalGpuError> for VgpuError {
    fn from(error: PhysicalGpuError) -> Self {
        Self::Physical(error)
    }
}

#[derive(Copy, Clone)]
struct Quota {
    memory_bytes: usize,
    buffers: usize,
    queues: usize,
    contexts: usize,
}

impl Quota {
    const KERNEL: Self = Self {
        memory_bytes: usize::MAX,
        buffers: 256,
        queues: 8,
        contexts: 8,
    };
    const HOST: Self = Self {
        memory_bytes: 64 * 1024 * 1024,
        buffers: 128,
        queues: 8,
        contexts: 4,
    };
    const GUEST: Self = Self {
        memory_bytes: 32 * 1024 * 1024,
        buffers: 64,
        queues: 4,
        contexts: 2,
    };
    const TEST: Self = Self {
        memory_bytes: 4 * 1024 * 1024,
        buffers: 8,
        queues: 4,
        contexts: 2,
    };
}

enum BufferBacking {
    Dma {
        phys: u64,
        virt: *mut u8,
    },
    GuestPages {
        vm_id: u8,
        guest_va: u64,
        pages: Vec<u64>,
    },
}

struct BufferRecord {
    backing: BufferBacking,
    bytes: usize,
    gpu: u64,
    usage: u32,
    epoch: u64,
    in_flight: u32,
    mapping_digest: u64,
}

unsafe impl Send for BufferRecord {}

struct BufferSlot {
    generation: u32,
    record: Option<BufferRecord>,
}

struct QueueRecord {
    class: QueueClass,
    timeline: TimelineStatus,
    failed_points: Vec<u64>,
}

struct QueueSlot {
    generation: u32,
    record: Option<QueueRecord>,
}

struct ContextBinding {
    queue: QueueHandle,
    descriptor: PhysicalContextDescriptor,
    context: PhysicalContextHandle,
}

enum GpuVmBinding {
    Owned(PhysicalGpuVmHandle),
    Borrowed { root_phys: u64 },
}

struct VirtualDevice {
    principal: Principal,
    capabilities: Capabilities,
    quota: Quota,
    epoch: u64,
    lost: bool,
    gpuvm: GpuVmBinding,
    /// First successfully registered privileged context descriptor. Kernel
    /// client identity is a capability for this exact engine, HWLRCA and
    /// PPGTT root; it may never be rebound in-place.
    kernel_context_capability: Option<PhysicalContextDescriptor>,
    next_gpu_va: u64,
    memory_used: usize,
    copied_upload_bytes: u64,
    flushed_vvideo_bytes: u64,
    buffers: Vec<BufferSlot>,
    queues: Vec<QueueSlot>,
    contexts: Vec<ContextBinding>,
}

struct DeviceSlot {
    generation: u32,
    record: Option<VirtualDevice>,
}

struct Broker {
    epoch: u64,
    devices: Vec<DeviceSlot>,
}

static BROKER: Mutex<Broker> = Mutex::new(Broker {
    epoch: 1,
    devices: Vec::new(),
});

/// Accounting-only view of one broker device.
///
/// Unlike [`broker_status`], producing this record never asks the physical GPU
/// to verify mappings or reads scheduler/adapter state.  It is intended for
/// cheap, best-effort observability of the allocations the vGPU broker itself
/// currently owns.
#[derive(Clone, Debug)]
pub(crate) struct DeviceMemoryAccounting {
    pub(crate) handle: DeviceHandle,
    pub(crate) principal: Principal,
    pub(crate) epoch: u64,
    pub(crate) lost: bool,
    pub(crate) mapped_bytes: usize,
    pub(crate) memory_quota: usize,
    pub(crate) buffer_count: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct BrokerMemoryAccounting {
    pub(crate) epoch: u64,
    pub(crate) total_mapped_bytes: usize,
    pub(crate) total_buffer_count: usize,
    pub(crate) devices: Vec<DeviceMemoryAccounting>,
}

#[derive(Clone, Debug)]
pub(crate) struct DeviceStatusSnapshot {
    pub(crate) handle: DeviceHandle,
    pub(crate) principal: Principal,
    pub(crate) capabilities: Capabilities,
    pub(crate) epoch: u64,
    pub(crate) lost: bool,
    pub(crate) memory_used: usize,
    pub(crate) memory_quota: usize,
    pub(crate) buffers: usize,
    pub(crate) queues: usize,
    pub(crate) contexts: usize,
    pub(crate) kernel_context_capability: Option<PhysicalContextDescriptor>,
    pub(crate) vvideo_buffers: usize,
    pub(crate) vvideo_mapping_identity: bool,
    pub(crate) vvideo_mapping_digest: u64,
    pub(crate) copied_upload_bytes: u64,
    pub(crate) flushed_vvideo_bytes: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct BrokerStatus {
    pub(crate) physical_ready: bool,
    pub(crate) physical_name: &'static str,
    pub(crate) physical_device_id: u16,
    pub(crate) physical_revision_id: u8,
    pub(crate) guc_submission: bool,
    pub(crate) epoch: u64,
    pub(crate) scheduler: PhysicalSchedulerStatus,
    pub(crate) kernel_context_boundaries: KernelContextBoundaryStatus,
    pub(crate) devices: Vec<DeviceStatusSnapshot>,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct KernelContextBoundaryStatus {
    pub(crate) bound: usize,
    pub(crate) active: usize,
    pub(crate) lost_bound: usize,
    pub(crate) coherent: bool,
    pub(crate) unique_hwlrcas: bool,
    pub(crate) unique_ppgtt_roots: bool,
}

impl KernelContextBoundaryStatus {
    pub(crate) const fn valid(self) -> bool {
        self.bound != 0
            && self.bound == self.active
            && self.lost_bound == 0
            && self.coherent
            && self.unique_hwlrcas
            && self.unique_ppgtt_roots
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct BrokerSelfTestReport {
    pub(crate) opened: bool,
    pub(crate) separate_gpuvms: bool,
    pub(crate) buffer_lifecycle: bool,
    pub(crate) cross_principal_rejected: bool,
    pub(crate) quota_rejected: bool,
    pub(crate) timeline_monotonic: bool,
    pub(crate) device_loss_propagated: bool,
    pub(crate) stale_handle_rejected: bool,
    pub(crate) cleanup: bool,
}

impl BrokerSelfTestReport {
    pub(crate) const fn passed(self) -> bool {
        self.opened
            && self.separate_gpuvms
            && self.buffer_lifecycle
            && self.cross_principal_rejected
            && self.quota_rejected
            && self.timeline_monotonic
            && self.device_loss_propagated
            && self.stale_handle_rejected
            && self.cleanup
    }
}

pub(crate) fn open(
    principal: Principal,
    requested: Capabilities,
) -> Result<DeviceHandle, VgpuError> {
    let physical = require_physical()?;
    let allowed = allowed_capabilities(principal, physical);
    let capabilities = requested.intersect(allowed);
    if !capabilities.contains(Capabilities::BUFFER)
        || !capabilities.contains(Capabilities::QUEUE)
        || !capabilities.contains(Capabilities::TIMELINE)
    {
        return Err(VgpuError::PermissionDenied);
    }
    let gpuvm = physical.create_gpuvm()?;
    let mut broker = BROKER.lock();
    let epoch = broker.epoch;
    let record = VirtualDevice {
        principal,
        capabilities,
        quota: quota_for(principal),
        epoch,
        lost: false,
        gpuvm: GpuVmBinding::Owned(gpuvm),
        kernel_context_capability: None,
        next_gpu_va: CLIENT_GPU_VA_BASE,
        memory_used: 0,
        copied_upload_bytes: 0,
        flushed_vvideo_bytes: 0,
        buffers: Vec::new(),
        queues: Vec::new(),
        contexts: Vec::new(),
    };
    Ok(insert_device(&mut broker, record))
}

pub(crate) fn close(principal: Principal, handle: DeviceHandle) -> Result<(), VgpuError> {
    let physical = require_physical()?;
    let mut broker = BROKER.lock();
    let (slot, generation) = decode_handle(handle.raw())?;
    let device_slot = broker
        .devices
        .get_mut(slot)
        .ok_or(VgpuError::InvalidHandle)?;
    if device_slot.generation != generation {
        return Err(VgpuError::InvalidHandle);
    }
    let mut device = device_slot.record.take().ok_or(VgpuError::InvalidHandle)?;
    if device.principal != principal {
        device_slot.record = Some(device);
        return Err(VgpuError::PermissionDenied);
    }

    if let Err(error) = destroy_device_resources(physical, &mut device) {
        device_slot.record = Some(device);
        return Err(error);
    }
    Ok(())
}

/// Tear down every vGPU device owned by a Hull VM at its VMX lifetime
/// boundary. A subsequent occupant of the same VM slot receives fresh handle
/// generations and a new broker epoch. Resources whose GPU ownership is
/// uncertain remain installed, lost, and pinned rather than being reused.
pub(crate) fn release_hull_guest(vm_id: u8) -> (usize, usize, u64) {
    let Some(physical) = physical_device().filter(|device| device.ready()) else {
        return (0, 0, 0);
    };
    let principal = Principal::HullGuest(vm_id as u16);
    let mut broker = BROKER.lock();
    broker.epoch = broker.epoch.wrapping_add(1).max(1);
    let epoch = broker.epoch;
    let mut released = 0usize;
    let mut quarantined = 0usize;
    for slot in &mut broker.devices {
        let Some(mut device) = slot.record.take() else {
            continue;
        };
        if device.principal != principal {
            slot.record = Some(device);
            continue;
        }
        match destroy_device_resources(physical, &mut device) {
            Ok(()) => released = released.saturating_add(1),
            Err(_) => {
                device.epoch = epoch;
                device.lost = true;
                slot.record = Some(device);
                quarantined = quarantined.saturating_add(1);
            }
        }
    }
    (released, quarantined, epoch)
}

pub(crate) fn device_info(
    principal: Principal,
    handle: DeviceHandle,
) -> Result<DeviceInfo, VgpuError> {
    let broker = BROKER.lock();
    let device = lookup_device(&broker, handle, principal)?;
    Ok(DeviceInfo {
        capabilities: device.capabilities,
        epoch: device.epoch,
        memory_used: device.memory_used,
        memory_quota: device.quota.memory_bytes,
        buffer_count: device
            .buffers
            .iter()
            .filter(|slot| slot.record.is_some())
            .count(),
        queue_count: device
            .queues
            .iter()
            .filter(|slot| slot.record.is_some())
            .count(),
        lost: device.lost,
    })
}

pub(crate) fn device_diagnostics(
    principal: Principal,
    handle: DeviceHandle,
) -> Result<DeviceDiagnostics, VgpuError> {
    let physical = require_physical()?;
    let broker = BROKER.lock();
    let device = lookup_device(&broker, handle, principal)?;
    ensure_live(device)?;
    let vm = match device.gpuvm {
        GpuVmBinding::Owned(vm) => vm,
        GpuVmBinding::Borrowed { .. } => return Err(VgpuError::Unsupported),
    };
    let mut mapping_digest = 0u64;
    let mut vvideo_buffers = 0usize;
    let mut mapping_identity = true;
    for record in device
        .buffers
        .iter()
        .filter_map(|slot| slot.record.as_ref())
    {
        let BufferBacking::GuestPages { pages, .. } = &record.backing else {
            continue;
        };
        vvideo_buffers = vvideo_buffers.saturating_add(1);
        mapping_digest ^= record
            .mapping_digest
            .rotate_left((vvideo_buffers & 63) as u32);
        mapping_identity &= physical.verify_gpuvm_pages(vm, record.gpu, pages)?;
    }
    Ok(DeviceDiagnostics {
        copied_upload_bytes: device.copied_upload_bytes,
        flushed_vvideo_bytes: device.flushed_vvideo_bytes,
        mapping_digest,
        vvideo_buffers,
        mapping_identity,
    })
}

pub(crate) fn create_buffer(
    principal: Principal,
    device_handle: DeviceHandle,
    bytes: usize,
    usage: u32,
) -> Result<BufferHandle, VgpuError> {
    if bytes == 0 {
        return Err(VgpuError::Unsupported);
    }
    let physical = require_physical()?;
    let alloc_bytes = align_up(bytes, PAGE_BYTES).ok_or(VgpuError::OutOfMemory)?;
    let mut broker = BROKER.lock();
    let device = lookup_device_mut(&mut broker, device_handle, principal)?;
    ensure_live(device)?;
    if !device.capabilities.contains(Capabilities::BUFFER) {
        return Err(VgpuError::PermissionDenied);
    }
    let buffer_count = device
        .buffers
        .iter()
        .filter(|slot| slot.record.is_some())
        .count();
    if buffer_count >= device.quota.buffers
        || device.memory_used.saturating_add(alloc_bytes) > device.quota.memory_bytes
    {
        return Err(VgpuError::QuotaExceeded);
    }
    let gpu = align_up_u64(device.next_gpu_va, PAGE_BYTES as u64).ok_or(VgpuError::OutOfMemory)?;
    let next = gpu
        .checked_add(alloc_bytes as u64)
        .ok_or(VgpuError::OutOfMemory)?;
    if next > CLIENT_GPU_VA_LIMIT {
        return Err(VgpuError::QuotaExceeded);
    }
    let Some((phys, virt)) = crate::dma::alloc(alloc_bytes, PAGE_BYTES) else {
        return Err(VgpuError::OutOfMemory);
    };
    unsafe {
        core::ptr::write_bytes(virt, 0, alloc_bytes);
    }
    crate::intel::dma_flush(virt, alloc_bytes);
    let vm = match device.gpuvm {
        GpuVmBinding::Owned(vm) => vm,
        GpuVmBinding::Borrowed { .. } => {
            crate::dma::dealloc(virt, alloc_bytes);
            return Err(VgpuError::Unsupported);
        }
    };
    if let Err(error) = physical.map_gpuvm(vm, gpu, phys, alloc_bytes) {
        crate::dma::dealloc(virt, alloc_bytes);
        return Err(error.into());
    }
    device.next_gpu_va = next;
    device.memory_used = device.memory_used.saturating_add(alloc_bytes);
    Ok(insert_buffer(
        device,
        BufferRecord {
            backing: BufferBacking::Dma { phys, virt },
            bytes: alloc_bytes,
            gpu,
            usage,
            epoch: device.epoch,
            in_flight: 0,
            mapping_digest: 0,
        },
    ))
}

/// Register page-granular storage already owned by a Hull guest as
/// vVideoMem. The CPU mapping stays in the guest while the same physical
/// pages become one contiguous virtual range in that guest's PPGTT.
pub(crate) fn create_vvideo_mem(
    principal: Principal,
    device_handle: DeviceHandle,
    guest_va: u64,
    bytes: usize,
    usage: u32,
) -> Result<BufferHandle, VgpuError> {
    let Principal::HullGuest(raw_vm_id) = principal else {
        return Err(VgpuError::PermissionDenied);
    };
    let vm_id = u8::try_from(raw_vm_id).map_err(|_| VgpuError::PermissionDenied)?;
    if bytes == 0
        || guest_va & (PAGE_BYTES as u64 - 1) != 0
        || bytes & (PAGE_BYTES - 1) != 0
        || usage & !BUFFER_USAGE_ALL != 0
        || usage & (BUFFER_USAGE_MAP_READ | BUFFER_USAGE_MAP_WRITE | BUFFER_USAGE_STORAGE) == 0
    {
        return Err(VgpuError::Unsupported);
    }
    let page_count = bytes / PAGE_BYTES;
    let mut pages = Vec::with_capacity(page_count);
    for page in 0..page_count {
        let gva = guest_va
            .checked_add((page * PAGE_BYTES) as u64)
            .ok_or(VgpuError::Unsupported)?;
        let phys = crate::hv::memory::guest_heap_page_phys_for_vm(vm_id, gva)
            .ok_or(VgpuError::PermissionDenied)?;
        pages.push(phys);
    }

    let physical = require_physical()?;
    let mut broker = BROKER.lock();
    let device = lookup_device_mut(&mut broker, device_handle, principal)?;
    ensure_live(device)?;
    if !device.capabilities.contains(Capabilities::BUFFER) {
        return Err(VgpuError::PermissionDenied);
    }
    if device
        .buffers
        .iter()
        .filter_map(|slot| slot.record.as_ref())
        .any(|record| match &record.backing {
            BufferBacking::GuestPages {
                guest_va: existing, ..
            } => ranges_overlap(*existing, record.bytes, guest_va, bytes),
            BufferBacking::Dma { .. } => false,
        })
    {
        return Err(VgpuError::Busy);
    }
    let buffer_count = device
        .buffers
        .iter()
        .filter(|slot| slot.record.is_some())
        .count();
    if buffer_count >= device.quota.buffers
        || device.memory_used.saturating_add(bytes) > device.quota.memory_bytes
    {
        return Err(VgpuError::QuotaExceeded);
    }
    let gpu = align_up_u64(device.next_gpu_va, PAGE_BYTES as u64).ok_or(VgpuError::OutOfMemory)?;
    let next = gpu
        .checked_add(bytes as u64)
        .ok_or(VgpuError::OutOfMemory)?;
    if next > CLIENT_GPU_VA_LIMIT {
        return Err(VgpuError::QuotaExceeded);
    }
    let vm = match device.gpuvm {
        GpuVmBinding::Owned(vm) => vm,
        GpuVmBinding::Borrowed { .. } => return Err(VgpuError::Unsupported),
    };
    let mut mapped = 0usize;
    for (page, phys) in pages.iter().copied().enumerate() {
        let page_gpu = gpu + (page * PAGE_BYTES) as u64;
        if let Err(error) = physical.map_gpuvm(vm, page_gpu, phys, PAGE_BYTES) {
            if mapped != 0 {
                let _ = physical.unmap_gpuvm(vm, gpu, mapped * PAGE_BYTES);
            }
            return Err(error.into());
        }
        mapped += 1;
    }
    match physical.verify_gpuvm_pages(vm, gpu, &pages) {
        Ok(true) => {}
        Ok(false) => {
            let _ = physical.unmap_gpuvm(vm, gpu, bytes);
            return Err(VgpuError::Physical(PhysicalGpuError::MapFailed));
        }
        Err(error) => {
            let _ = physical.unmap_gpuvm(vm, gpu, bytes);
            return Err(error.into());
        }
    }
    let mapping_digest = vvideo_mapping_digest(device.epoch, guest_va, gpu, pages.len());
    device.next_gpu_va = next;
    device.memory_used = device.memory_used.saturating_add(bytes);
    Ok(insert_buffer(
        device,
        BufferRecord {
            backing: BufferBacking::GuestPages {
                vm_id,
                guest_va,
                pages,
            },
            bytes,
            gpu,
            usage,
            epoch: device.epoch,
            in_flight: 0,
            mapping_digest,
        },
    ))
}

pub(crate) fn flush_vvideo_mem(
    principal: Principal,
    device_handle: DeviceHandle,
    buffer_handle: BufferHandle,
    offset: usize,
    bytes: usize,
) -> Result<usize, VgpuError> {
    cache_maintain_vvideo(
        principal,
        device_handle,
        buffer_handle,
        offset,
        bytes,
        BUFFER_USAGE_MAP_WRITE,
    )
}

pub(crate) fn invalidate_vvideo_mem(
    principal: Principal,
    device_handle: DeviceHandle,
    buffer_handle: BufferHandle,
    offset: usize,
    bytes: usize,
) -> Result<usize, VgpuError> {
    cache_maintain_vvideo(
        principal,
        device_handle,
        buffer_handle,
        offset,
        bytes,
        BUFFER_USAGE_MAP_READ,
    )
}

fn cache_maintain_vvideo(
    principal: Principal,
    device_handle: DeviceHandle,
    buffer_handle: BufferHandle,
    offset: usize,
    bytes: usize,
    required_usage: u32,
) -> Result<usize, VgpuError> {
    let mut broker = BROKER.lock();
    let device = lookup_device_mut(&mut broker, device_handle, principal)?;
    ensure_live(device)?;
    {
        let record = lookup_buffer(device, buffer_handle)?;
        if record.epoch != device.epoch {
            return Err(VgpuError::DeviceLost);
        }
        if record.in_flight != 0 {
            return Err(VgpuError::Busy);
        }
        if record.usage & required_usage == 0 {
            return Err(VgpuError::PermissionDenied);
        }
        let end = offset.checked_add(bytes).ok_or(VgpuError::Unsupported)?;
        if end > record.bytes {
            return Err(VgpuError::Unsupported);
        }
        let BufferBacking::GuestPages { pages, .. } = &record.backing else {
            return Err(VgpuError::Unsupported);
        };
        let mut cursor = offset;
        let mut remaining = bytes;
        while remaining != 0 {
            let page = cursor / PAGE_BYTES;
            let in_page = cursor % PAGE_BYTES;
            let count = core::cmp::min(PAGE_BYTES - in_page, remaining);
            let virt = crate::phys::phys_to_virt(pages[page] as usize) as *mut u8;
            crate::intel::dma_flush(unsafe { virt.add(in_page) }, count);
            cursor += count;
            remaining -= count;
        }
    }
    device.flushed_vvideo_bytes = device.flushed_vvideo_bytes.saturating_add(bytes as u64);
    Ok(bytes)
}

pub(crate) fn buffer_info(
    principal: Principal,
    device_handle: DeviceHandle,
    buffer_handle: BufferHandle,
) -> Result<BufferInfo, VgpuError> {
    let broker = BROKER.lock();
    let device = lookup_device(&broker, device_handle, principal)?;
    ensure_live(device)?;
    let record = lookup_buffer(device, buffer_handle)?;
    Ok(BufferInfo {
        bytes: record.bytes,
        usage: record.usage,
        flags: if matches!(&record.backing, BufferBacking::GuestPages { .. }) {
            BUFFER_INFO_FLAG_VVIDEO_MEM
        } else {
            0
        },
    })
}

pub(crate) fn write_buffer(
    principal: Principal,
    device_handle: DeviceHandle,
    buffer_handle: BufferHandle,
    offset: usize,
    bytes: &[u8],
) -> Result<usize, VgpuError> {
    let mut broker = BROKER.lock();
    let device = lookup_device_mut(&mut broker, device_handle, principal)?;
    ensure_live(device)?;
    {
        let record = lookup_buffer(device, buffer_handle)?;
        if record.usage & BUFFER_USAGE_MAP_WRITE == 0 {
            return Err(VgpuError::PermissionDenied);
        }
        let end = offset
            .checked_add(bytes.len())
            .ok_or(VgpuError::Unsupported)?;
        if end > record.bytes {
            return Err(VgpuError::Unsupported);
        }
        let virt = match &record.backing {
            BufferBacking::Dma { virt, .. } => *virt,
            BufferBacking::GuestPages { .. } => return Err(VgpuError::Unsupported),
        };
        if !bytes.is_empty() {
            unsafe {
                core::ptr::copy_nonoverlapping(bytes.as_ptr(), virt.add(offset), bytes.len());
            }
            crate::intel::dma_flush(unsafe { virt.add(offset) }, bytes.len());
        }
    }
    device.copied_upload_bytes = device
        .copied_upload_bytes
        .saturating_add(bytes.len() as u64);
    Ok(bytes.len())
}

pub(crate) fn read_buffer(
    principal: Principal,
    device_handle: DeviceHandle,
    buffer_handle: BufferHandle,
    offset: usize,
    out: &mut [u8],
) -> Result<usize, VgpuError> {
    let broker = BROKER.lock();
    let device = lookup_device(&broker, device_handle, principal)?;
    ensure_live(device)?;
    let record = lookup_buffer(device, buffer_handle)?;
    if record.usage & BUFFER_USAGE_MAP_READ == 0 {
        return Err(VgpuError::PermissionDenied);
    }
    let end = offset
        .checked_add(out.len())
        .ok_or(VgpuError::Unsupported)?;
    if end > record.bytes {
        return Err(VgpuError::Unsupported);
    }
    let virt = match &record.backing {
        BufferBacking::Dma { virt, .. } => *virt,
        BufferBacking::GuestPages { .. } => return Err(VgpuError::Unsupported),
    };
    if !out.is_empty() {
        crate::intel::dma_flush(unsafe { virt.add(offset) }, out.len());
        unsafe { core::ptr::copy_nonoverlapping(virt.add(offset), out.as_mut_ptr(), out.len()) };
    }
    Ok(out.len())
}

pub(crate) fn destroy_buffer(
    principal: Principal,
    device_handle: DeviceHandle,
    buffer_handle: BufferHandle,
) -> Result<(), VgpuError> {
    let physical = require_physical()?;
    let mut broker = BROKER.lock();
    let device = lookup_device_mut(&mut broker, device_handle, principal)?;
    ensure_live(device)?;
    let (slot, generation) = decode_handle(buffer_handle.raw())?;
    let buffer_slot = device
        .buffers
        .get_mut(slot)
        .ok_or(VgpuError::InvalidHandle)?;
    if buffer_slot.generation != generation {
        return Err(VgpuError::InvalidHandle);
    }
    let record = buffer_slot
        .record
        .as_ref()
        .ok_or(VgpuError::InvalidHandle)?;
    if record.in_flight != 0 {
        return Err(VgpuError::Busy);
    }
    let vm = match device.gpuvm {
        GpuVmBinding::Owned(vm) => vm,
        GpuVmBinding::Borrowed { .. } => return Err(VgpuError::Unsupported),
    };
    physical.unmap_gpuvm(vm, record.gpu, record.bytes)?;
    let mut record = buffer_slot.record.take().expect("validated vgpu buffer");
    device.memory_used = device.memory_used.saturating_sub(record.bytes);
    release_buffer_backing(&mut record);
    Ok(())
}

pub(crate) fn create_queue(
    principal: Principal,
    device_handle: DeviceHandle,
    class: QueueClass,
) -> Result<QueueHandle, VgpuError> {
    let mut broker = BROKER.lock();
    let device = lookup_device_mut(&mut broker, device_handle, principal)?;
    ensure_live(device)?;
    if !device.capabilities.contains(Capabilities::QUEUE)
        || !device.capabilities.contains(class.capability())
    {
        return Err(VgpuError::PermissionDenied);
    }
    if device
        .queues
        .iter()
        .filter(|slot| slot.record.is_some())
        .count()
        >= device.quota.queues
    {
        return Err(VgpuError::QuotaExceeded);
    }
    Ok(insert_queue(
        device,
        QueueRecord {
            class,
            timeline: TimelineStatus::default(),
            failed_points: Vec::new(),
        },
    ))
}

pub(crate) fn destroy_queue(
    principal: Principal,
    device_handle: DeviceHandle,
    queue_handle: QueueHandle,
) -> Result<(), VgpuError> {
    let mut broker = BROKER.lock();
    let device = lookup_device_mut(&mut broker, device_handle, principal)?;
    ensure_live(device)?;
    if device
        .contexts
        .iter()
        .any(|binding| binding.queue == queue_handle)
    {
        return Err(VgpuError::Busy);
    }
    let (slot, generation) = decode_handle(queue_handle.raw())?;
    let queue_slot = device
        .queues
        .get_mut(slot)
        .ok_or(VgpuError::InvalidHandle)?;
    if queue_slot.generation != generation || queue_slot.record.is_none() {
        return Err(VgpuError::InvalidHandle);
    }
    queue_slot.record.take();
    Ok(())
}

/// Submit an ABI/control-path no-op. It validates device/queue ownership and
/// timeline behavior but deliberately does not claim GPU execution.
pub(crate) fn submit_control_nop(
    principal: Principal,
    device_handle: DeviceHandle,
    queue_handle: QueueHandle,
) -> Result<TimelinePoint, VgpuError> {
    let mut broker = BROKER.lock();
    let device = lookup_device_mut(&mut broker, device_handle, principal)?;
    ensure_live(device)?;
    let queue = lookup_queue_mut(device, queue_handle)?;
    queue.timeline.submitted = queue.timeline.submitted.wrapping_add(1).max(1);
    queue.timeline.completed = queue.timeline.submitted;
    Ok(TimelinePoint {
        queue: queue_handle,
        value: queue.timeline.submitted,
        physical_serial: 0,
        physical_publish_sequence: 0,
    })
}

/// Execute the fixed SceneDB AABB kernel in the tenant's own GPUVM. This is a
/// typed operation rather than an arbitrary batch/shader submission surface.
pub(crate) fn submit_scene_aabb(
    principal: Principal,
    device_handle: DeviceHandle,
    queue_handle: QueueHandle,
    dispatch: SceneAabbDispatch,
) -> Result<SceneAabbResult, VgpuError> {
    let physical = require_physical()?;
    let mut broker = BROKER.lock();
    let device = lookup_device_mut(&mut broker, device_handle, principal)?;
    ensure_live(device)?;
    if lookup_queue(device, queue_handle)?.class != QueueClass::Compute {
        return Err(VgpuError::PermissionDenied);
    }
    let row_bytes = (dispatch.rows as usize)
        .checked_mul(core::mem::size_of::<f32>())
        .ok_or(VgpuError::Unsupported)?;
    let live_bytes = (dispatch.rows as usize)
        .div_ceil(64)
        .checked_mul(core::mem::size_of::<u64>())
        .ok_or(VgpuError::Unsupported)?;
    let output_bytes = (dispatch.rows as usize)
        .checked_mul(core::mem::size_of::<u32>())
        .ok_or(VgpuError::Unsupported)?;
    let mut bounds = [PhysicalBufferSlice { gpu: 0, bytes: 0 }; 6];
    for (dst, src) in bounds.iter_mut().zip(dispatch.bounds) {
        *dst = validate_vvideo_slice(device, src, row_bytes, BUFFER_USAGE_STORAGE)?;
    }
    let liveness =
        validate_vvideo_slice(device, dispatch.liveness, live_bytes, BUFFER_USAGE_STORAGE)?;
    let output = validate_vvideo_slice(
        device,
        dispatch.output,
        output_bytes,
        BUFFER_USAGE_STORAGE | BUFFER_USAGE_MAP_READ,
    )?;
    let vm = match device.gpuvm {
        GpuVmBinding::Owned(vm) => vm,
        GpuVmBinding::Borrowed { .. } => return Err(VgpuError::Unsupported),
    };
    if dispatch.rows == 0 {
        let queue = lookup_queue_mut(device, queue_handle)?;
        queue.timeline.submitted = queue.timeline.submitted.wrapping_add(1).max(1);
        queue.timeline.completed = queue.timeline.submitted;
        return Ok(SceneAabbResult {
            point: TimelinePoint {
                queue: queue_handle,
                value: queue.timeline.submitted,
                physical_serial: 0,
                physical_publish_sequence: 0,
            },
            hits: 0,
        });
    }
    let request = PhysicalSceneAabbRequest {
        vm,
        bounds,
        liveness,
        output,
        rows: dispatch.rows,
        query_min: dispatch.query_min,
        query_max: dispatch.query_max,
    };
    pin_scene_aabb_buffers(device, &dispatch)?;
    let completion = match physical.submit_scene_aabb(request) {
        Ok(completion) => completion,
        Err(error) => {
            // A timeout means hardware ownership is unknown. Preserve the
            // pins permanently; freeing or remapping those pages could turn a
            // late GPU access into cross-tenant corruption.
            if error != PhysicalGpuError::CompletionTimeout {
                unpin_scene_aabb_buffers(device, &dispatch);
            }
            let queue = lookup_queue_mut(device, queue_handle)?;
            queue.timeline.failures = queue.timeline.failures.saturating_add(1);
            return Err(error.into());
        }
    };
    unpin_scene_aabb_buffers(device, &dispatch);
    let queue = lookup_queue_mut(device, queue_handle)?;
    queue.timeline.submitted = queue.timeline.submitted.wrapping_add(1).max(1);
    queue.timeline.completed = queue.timeline.submitted;
    queue.timeline.last_physical_serial = completion.serial;
    Ok(SceneAabbResult {
        point: TimelinePoint {
            queue: queue_handle,
            value: queue.timeline.submitted,
            physical_serial: completion.serial,
            physical_publish_sequence: 0,
        },
        hits: completion.hits,
    })
}

fn scene_aabb_buffer_handles(dispatch: &SceneAabbDispatch) -> [BufferHandle; 8] {
    [
        dispatch.bounds[0].buffer,
        dispatch.bounds[1].buffer,
        dispatch.bounds[2].buffer,
        dispatch.bounds[3].buffer,
        dispatch.bounds[4].buffer,
        dispatch.bounds[5].buffer,
        dispatch.liveness.buffer,
        dispatch.output.buffer,
    ]
}

fn pin_scene_aabb_buffers(
    device: &mut VirtualDevice,
    dispatch: &SceneAabbDispatch,
) -> Result<(), VgpuError> {
    let handles = scene_aabb_buffer_handles(dispatch);
    for index in 0..handles.len() {
        if handles[..index].contains(&handles[index]) {
            continue;
        }
        let result = lookup_buffer_mut(device, handles[index]).and_then(|record| {
            record.in_flight = record.in_flight.checked_add(1).ok_or(VgpuError::Busy)?;
            Ok(())
        });
        if let Err(error) = result {
            for rollback in 0..index {
                if handles[..rollback].contains(&handles[rollback]) {
                    continue;
                }
                if let Ok(record) = lookup_buffer_mut(device, handles[rollback]) {
                    record.in_flight = record.in_flight.saturating_sub(1);
                }
            }
            return Err(error);
        }
    }
    Ok(())
}

fn unpin_scene_aabb_buffers(device: &mut VirtualDevice, dispatch: &SceneAabbDispatch) {
    let handles = scene_aabb_buffer_handles(dispatch);
    for index in 0..handles.len() {
        if handles[..index].contains(&handles[index]) {
            continue;
        }
        if let Ok(record) = lookup_buffer_mut(device, handles[index]) {
            record.in_flight = record.in_flight.saturating_sub(1);
        }
    }
}

fn validate_vvideo_slice(
    device: &VirtualDevice,
    slice: BufferSlice,
    required_bytes: usize,
    required_usage: u32,
) -> Result<PhysicalBufferSlice, VgpuError> {
    if slice.bytes < required_bytes {
        return Err(VgpuError::Unsupported);
    }
    let record = lookup_buffer(device, slice.buffer)?;
    if record.epoch != device.epoch || record.in_flight != 0 {
        return Err(VgpuError::Busy);
    }
    if record.usage & required_usage != required_usage {
        return Err(VgpuError::PermissionDenied);
    }
    if !matches!(&record.backing, BufferBacking::GuestPages { .. }) {
        return Err(VgpuError::PermissionDenied);
    }
    let end = slice
        .offset
        .checked_add(slice.bytes)
        .ok_or(VgpuError::Unsupported)?;
    if end > record.bytes {
        return Err(VgpuError::Unsupported);
    }
    Ok(PhysicalBufferSlice {
        gpu: record.gpu + slice.offset as u64,
        bytes: slice.bytes,
    })
}

pub(crate) fn timeline_status(
    principal: Principal,
    device_handle: DeviceHandle,
    queue_handle: QueueHandle,
) -> Result<TimelineStatus, VgpuError> {
    let broker = BROKER.lock();
    let device = lookup_device(&broker, device_handle, principal)?;
    ensure_live(device)?;
    Ok(lookup_queue(device, queue_handle)?.timeline)
}

pub(crate) fn wait_timeline(
    principal: Principal,
    device_handle: DeviceHandle,
    queue_handle: QueueHandle,
    value: u64,
) -> Result<(), VgpuError> {
    let status = timeline_status(principal, device_handle, queue_handle)?;
    if status.completed >= value {
        Ok(())
    } else {
        Err(VgpuError::NotComplete)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum TimelinePointStatus {
    Pending,
    Complete,
    Failed,
}

/// Query one exact point on an adopted kernel queue.
pub(crate) fn kernel_point_status(
    client: KernelClient,
    point: TimelinePoint,
) -> Result<TimelinePointStatus, VgpuError> {
    let broker = BROKER.lock();
    let (_, device) =
        find_device_by_principal(&broker, client.principal()).ok_or(VgpuError::InvalidHandle)?;
    ensure_live(device)?;
    let queue_handle =
        find_queue_by_class(device, client.queue_class()).ok_or(VgpuError::InvalidHandle)?;
    if queue_handle != point.queue {
        return Err(VgpuError::InvalidHandle);
    }
    point_status(lookup_queue(device, queue_handle)?, point)
}

/// Admit one already-prepared kernel LRC through its privileged virtual
/// device, then through the physical GuC scheduler.
pub(crate) fn submit_kernel_context(
    client: KernelClient,
    descriptor: PhysicalContextDescriptor,
) -> Result<TimelinePoint, VgpuError> {
    if !client.accepts_engine(descriptor.engine) || descriptor.gpuvm_root_phys == 0 {
        return Err(VgpuError::PermissionDenied);
    }
    let physical = require_physical()?;
    let mut broker = BROKER.lock();

    // GuC's registration key includes the engine, but HWLRCA is still a global
    // graphics address naming physical context backing. Reject another broker
    // principal claiming that backing on any engine, or claiming the same
    // PPGTT root: different scheduler tokens must never alias storage.
    let context_identity_claimed_elsewhere = broker.devices.iter().any(|slot| {
        let Some(other) = slot.record.as_ref() else {
            return false;
        };
        other.principal != client.principal()
            && other.kernel_context_capability.is_some_and(|bound| {
                bound.gpuvm_root_phys == descriptor.gpuvm_root_phys
                    || (bound.hwlrca_lo == descriptor.hwlrca_lo
                        && bound.hwlrca_hi == descriptor.hwlrca_hi)
            })
    });
    if context_identity_claimed_elsewhere {
        return Err(VgpuError::PermissionDenied);
    }

    let device_handle = ensure_kernel_device(&mut broker, client, descriptor.gpuvm_root_phys)?;
    let device = lookup_device_mut(&mut broker, device_handle, client.principal())?;
    ensure_live(device)?;
    let queue_handle = ensure_kernel_queue(device, client.queue_class())?;

    let context = if let Some(bound) = device.kernel_context_capability {
        if bound != descriptor {
            // A kernel principal is not a bag of interchangeable contexts.
            // Rebinding any component would silently couple unrelated LRC
            // storage or address spaces through one software identity.
            return Err(VgpuError::PermissionDenied);
        }
        let Some(binding) = device.contexts.first() else {
            // A bound descriptor with no retained physical registration is a
            // quarantined identity, never permission to recycle its backing.
            return Err(VgpuError::DeviceLost);
        };
        if binding.queue != queue_handle || binding.descriptor != descriptor {
            return Err(VgpuError::DeviceLost);
        }
        binding.context
    } else {
        if !device.contexts.is_empty() {
            return Err(VgpuError::DeviceLost);
        }
        if device.contexts.len() >= device.quota.contexts {
            return Err(VgpuError::QuotaExceeded);
        }
        let context = physical.register_context(descriptor, client.physical_priority())?;
        device.contexts.push(ContextBinding {
            queue: queue_handle,
            descriptor,
            context,
        });
        device.kernel_context_capability = Some(descriptor);
        context
    };

    let submission = match physical.submit_context(context) {
        Ok(submission) => submission,
        Err(error) => {
            let queue = lookup_queue_mut(device, queue_handle)?;
            queue.timeline.failures = queue.timeline.failures.saturating_add(1);
            return Err(error.into());
        }
    };
    let queue = lookup_queue_mut(device, queue_handle)?;
    queue.timeline.submitted = queue.timeline.submitted.wrapping_add(1).max(1);
    queue.timeline.last_physical_serial = submission.serial;
    Ok(TimelinePoint {
        queue: queue_handle,
        value: queue.timeline.submitted,
        physical_serial: submission.serial,
        physical_publish_sequence: submission.scheduler_publish_sequence,
    })
}

/// Retire one exact serialized kernel submission after its hardware
/// marker/fence has been observed (or has definitively failed).
pub(crate) fn complete_kernel_submission(
    client: KernelClient,
    point: TimelinePoint,
    completed: bool,
) -> Option<TimelinePoint> {
    let mut broker = BROKER.lock();
    let principal = client.principal();
    let (_, device) = find_device_mut_by_principal(&mut broker, principal)?;
    let queue_handle = find_queue_by_class(device, client.queue_class())?;
    if queue_handle != point.queue || point.value == 0 {
        return None;
    }
    let queue = lookup_queue_mut(device, queue_handle).ok()?;
    if point.value > queue.timeline.submitted {
        return None;
    }
    if queue.timeline.completed < point.value {
        queue.timeline.completed = point.value;
        if !completed {
            queue.timeline.failures = queue.timeline.failures.saturating_add(1);
            if !queue.failed_points.contains(&point.value) {
                queue.failed_points.push(point.value);
            }
        }
    }
    Some(TimelinePoint {
        queue: queue_handle,
        value: point.value,
        physical_serial: point.physical_serial,
        physical_publish_sequence: point.physical_publish_sequence,
    })
}

pub(crate) fn kernel_timeline(client: KernelClient) -> Option<TimelineStatus> {
    let broker = BROKER.lock();
    let (_, device) = find_device_by_principal(&broker, client.principal())?;
    let queue = find_queue_by_class(device, client.queue_class())?;
    lookup_queue(device, queue).ok().map(|queue| queue.timeline)
}

/// Whether a backend may mutate this client's persistent submission storage.
/// An unseen client is eligible for first-time allocation; a quarantined or
/// partially destroyed capability is permanently non-reusable until reboot.
pub(crate) fn kernel_context_storage_reusable(client: KernelClient) -> bool {
    let broker = BROKER.lock();
    let Some((_, device)) = find_device_by_principal(&broker, client.principal()) else {
        return true;
    };
    !device.lost
        && match device.kernel_context_capability {
            None => device.contexts.is_empty(),
            Some(identity) => {
                device.contexts.len() == 1 && device.contexts[0].descriptor == identity
            }
        }
}

/// Result of containing one failed privileged kernel client.
///
/// The client's allocations and GPUVM deliberately remain owned by the
/// broker. A context that timed out may still have late writes in flight, so
/// none of its address space may be recycled until a full device reset.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct KernelClientIsolation {
    pub(crate) device_found: bool,
    pub(crate) contexts_disabled: usize,
    pub(crate) contexts_retained: usize,
}

/// Contain one failed kernel consumer without declaring the physical GPU, or
/// any other virtual device, lost.
///
/// Successful GuC context destruction disables scheduling before
/// deregistration. Failed destructions remain bound in the lost virtual device
/// so their IDs and backing storage cannot be reused unsafely.
pub(crate) fn isolate_kernel_context(client: KernelClient) -> KernelClientIsolation {
    let physical = physical_device();
    let mut broker = BROKER.lock();
    let Some((_, device)) = find_device_mut_by_principal(&mut broker, client.principal()) else {
        return KernelClientIsolation::default();
    };

    device.lost = true;
    let mut report = KernelClientIsolation {
        device_found: true,
        ..KernelClientIsolation::default()
    };
    let Some(physical) = physical else {
        report.contexts_retained = device.contexts.len();
        return report;
    };

    let mut index = 0usize;
    while index < device.contexts.len() {
        let context = device.contexts[index].context;
        match physical.destroy_context(context) {
            Ok(()) => {
                device.contexts.remove(index);
                report.contexts_disabled = report.contexts_disabled.saturating_add(1);
            }
            Err(_) => {
                // Keep the binding live: destroy_context deliberately leaves a
                // rejected GuC slot allocated, and the broker must mirror that
                // ownership rather than permit a late-write alias.
                report.contexts_retained = report.contexts_retained.saturating_add(1);
                index = index.saturating_add(1);
            }
        }
    }
    report
}

/// Compatibility name for callers whose lane and context are synonymous.
/// The immutable capability rule guarantees that one `KernelClient` owns at
/// most one physical context, so this never expands recovery to an engine or
/// another client.
pub(crate) fn isolate_kernel_client(client: KernelClient) -> KernelClientIsolation {
    isolate_kernel_context(client)
}

/// Reset/device-loss hook for the physical driver. All tenant handles remain
/// queryable for diagnosis but reject further allocation and submission.
pub(crate) fn notify_physical_device_lost() -> u64 {
    let mut broker = BROKER.lock();
    broker.epoch = broker.epoch.wrapping_add(1).max(1);
    let epoch = broker.epoch;
    for slot in &mut broker.devices {
        let Some(device) = slot.record.as_mut() else {
            continue;
        };
        device.epoch = epoch;
        device.lost = true;
        for queue in &mut device.queues {
            let Some(queue) = queue.record.as_mut() else {
                continue;
            };
            if queue.timeline.completed < queue.timeline.submitted {
                queue.timeline.failures = queue.timeline.failures.saturating_add(1);
            }
        }
    }
    epoch
}

/// Copy the broker's allocation counters without probing or validating the
/// physical GPU.
///
/// `mapped_bytes` is the broker's charged allocation size.  It describes bytes
/// mapped into mediated GPU address spaces, not physical dedicated-VRAM
/// residency.  The broker lock is held only while these counters are copied.
pub(crate) fn broker_memory_accounting() -> BrokerMemoryAccounting {
    let broker = BROKER.lock();
    let mut total_mapped_bytes = 0usize;
    let mut total_buffer_count = 0usize;
    let mut devices = Vec::new();

    for (slot, entry) in broker.devices.iter().enumerate() {
        let Some(device) = entry.record.as_ref() else {
            continue;
        };
        let buffer_count = device
            .buffers
            .iter()
            .filter(|slot| slot.record.is_some())
            .count();
        total_mapped_bytes = total_mapped_bytes.saturating_add(device.memory_used);
        total_buffer_count = total_buffer_count.saturating_add(buffer_count);
        devices.push(DeviceMemoryAccounting {
            handle: DeviceHandle(encode_handle(slot, entry.generation)),
            principal: device.principal,
            epoch: device.epoch,
            lost: device.lost,
            mapped_bytes: device.memory_used,
            memory_quota: device.quota.memory_bytes,
            buffer_count,
        });
    }

    BrokerMemoryAccounting {
        epoch: broker.epoch,
        total_mapped_bytes,
        total_buffer_count,
        devices,
    }
}

pub(crate) fn broker_status() -> BrokerStatus {
    let physical = physical_device();
    let info = physical.map(|device| device.adapter_info());
    let scheduler = physical
        .map(|device| device.scheduler_status())
        .unwrap_or_default();
    let broker = BROKER.lock();
    let kernel_context_boundaries = kernel_context_boundary_status(&broker);
    let mut devices = Vec::new();
    for (slot, entry) in broker.devices.iter().enumerate() {
        let Some(device) = entry.record.as_ref() else {
            continue;
        };
        let vm = match device.gpuvm {
            GpuVmBinding::Owned(vm) => Some(vm),
            GpuVmBinding::Borrowed { .. } => None,
        };
        let mut vvideo_buffers = 0usize;
        let mut vvideo_mapping_identity = true;
        let mut vvideo_mapping_digest = 0u64;
        for record in device
            .buffers
            .iter()
            .filter_map(|slot| slot.record.as_ref())
        {
            let BufferBacking::GuestPages { pages, .. } = &record.backing else {
                continue;
            };
            vvideo_buffers = vvideo_buffers.saturating_add(1);
            vvideo_mapping_digest ^= record
                .mapping_digest
                .rotate_left((vvideo_buffers & 63) as u32);
            vvideo_mapping_identity &= vm.is_some_and(|vm| {
                physical
                    .and_then(|gpu| gpu.verify_gpuvm_pages(vm, record.gpu, pages).ok())
                    .unwrap_or(false)
            });
        }
        devices.push(DeviceStatusSnapshot {
            handle: DeviceHandle(encode_handle(slot, entry.generation)),
            principal: device.principal,
            capabilities: device.capabilities,
            epoch: device.epoch,
            lost: device.lost,
            memory_used: device.memory_used,
            memory_quota: device.quota.memory_bytes,
            buffers: device
                .buffers
                .iter()
                .filter(|slot| slot.record.is_some())
                .count(),
            queues: device
                .queues
                .iter()
                .filter(|slot| slot.record.is_some())
                .count(),
            contexts: device.contexts.len(),
            kernel_context_capability: device.kernel_context_capability,
            vvideo_buffers,
            vvideo_mapping_identity,
            vvideo_mapping_digest,
            copied_upload_bytes: device.copied_upload_bytes,
            flushed_vvideo_bytes: device.flushed_vvideo_bytes,
        });
    }
    BrokerStatus {
        physical_ready: physical.is_some_and(|device| device.ready()),
        physical_name: info.map(|info| info.name).unwrap_or("none"),
        physical_device_id: info.map(|info| info.device_id).unwrap_or(0),
        physical_revision_id: info.map(|info| info.revision_id).unwrap_or(0),
        guc_submission: info.is_some_and(|info| info.guc_submission),
        epoch: broker.epoch,
        scheduler,
        kernel_context_boundaries,
        devices,
    }
}

fn kernel_context_boundary_status(broker: &Broker) -> KernelContextBoundaryStatus {
    let mut report = KernelContextBoundaryStatus {
        coherent: true,
        unique_hwlrcas: true,
        unique_ppgtt_roots: true,
        ..KernelContextBoundaryStatus::default()
    };
    let mut identities: Vec<PhysicalContextDescriptor> = Vec::new();

    for device in broker
        .devices
        .iter()
        .filter_map(|slot| slot.record.as_ref())
    {
        let Some(identity) = device.kernel_context_capability else {
            report.coherent &= device.contexts.is_empty();
            continue;
        };
        report.bound = report.bound.saturating_add(1);
        if device.lost {
            report.lost_bound = report.lost_bound.saturating_add(1);
        } else {
            report.active = report.active.saturating_add(device.contexts.len());
        }
        let root_matches = matches!(
            device.gpuvm,
            GpuVmBinding::Borrowed { root_phys } if root_phys == identity.gpuvm_root_phys
        );
        let binding_matches = device.contexts.len() <= 1
            && device
                .contexts
                .iter()
                .all(|binding| binding.descriptor == identity);
        let live_context_present = !device.lost && device.contexts.len() == 1;
        report.coherent &= root_matches && binding_matches && live_context_present;

        for previous in &identities {
            report.unique_hwlrcas &= previous.hwlrca_lo != identity.hwlrca_lo
                || previous.hwlrca_hi != identity.hwlrca_hi;
            report.unique_ppgtt_roots &= previous.gpuvm_root_phys != identity.gpuvm_root_phys;
        }
        identities.push(identity);
    }
    report
}

pub(crate) fn run_broker_self_test() -> BrokerSelfTestReport {
    let mut report = BrokerSelfTestReport::default();
    let a = Principal::RuntimeTest(0xA1);
    let b = Principal::RuntimeTest(0xB2);
    let requested = Capabilities::CLIENT_BASE;
    let Ok(dev_a) = open(a, requested) else {
        return report;
    };
    let Ok(dev_b) = open(b, requested) else {
        let _ = close(a, dev_a);
        return report;
    };
    report.opened = true;
    report.separate_gpuvms = device_gpuvm_root(a, dev_a)
        .zip(device_gpuvm_root(b, dev_b))
        .is_some_and(|(a_root, b_root)| a_root != 0 && b_root != 0 && a_root != b_root);

    if let Ok(buffer) =
        create_buffer(a, dev_a, PAGE_BYTES, BUFFER_USAGE_MAP_READ | BUFFER_USAGE_MAP_WRITE)
    {
        report.buffer_lifecycle = buffer_info(a, dev_a, buffer)
            .is_ok_and(|info| info.bytes == PAGE_BYTES)
            && write_buffer(a, dev_a, buffer, 7, b"vgpu").is_ok()
            && {
                let mut out = [0u8; 4];
                read_buffer(a, dev_a, buffer, 7, &mut out).is_ok() && out == *b"vgpu"
            };
        report.cross_principal_rejected =
            buffer_info(b, dev_a, buffer) == Err(VgpuError::PermissionDenied);
        let oversized = quota_for(a).memory_bytes.saturating_add(PAGE_BYTES);
        report.quota_rejected =
            create_buffer(a, dev_a, oversized, 0x1) == Err(VgpuError::QuotaExceeded);
        let _ = destroy_buffer(a, dev_a, buffer);
    }

    if let Ok(queue) = create_queue(a, dev_a, QueueClass::Compute) {
        let first = submit_control_nop(a, dev_a, queue);
        let second = submit_control_nop(a, dev_a, queue);
        report.timeline_monotonic = match (first, second) {
            (Ok(first), Ok(second)) => {
                first.value == 1
                    && second.value == 2
                    && wait_timeline(a, dev_a, queue, second.value).is_ok()
            }
            _ => false,
        };
        let _ = destroy_queue(a, dev_a, queue);
    }

    report.device_loss_propagated = mark_one_device_lost_for_test(a, dev_a)
        && create_queue(a, dev_a, QueueClass::Compute) == Err(VgpuError::DeviceLost)
        && device_info(a, dev_a).is_ok_and(|info| info.lost);

    let close_a = close(a, dev_a).is_ok();
    report.stale_handle_rejected = device_info(a, dev_a) == Err(VgpuError::InvalidHandle);
    let close_b = close(b, dev_b).is_ok();
    report.cleanup = close_a && close_b;
    report
}

fn mark_one_device_lost_for_test(principal: Principal, handle: DeviceHandle) -> bool {
    let mut broker = BROKER.lock();
    let epoch = broker.epoch.wrapping_add(1).max(1);
    broker.epoch = epoch;
    let Ok(device) = lookup_device_mut(&mut broker, handle, principal) else {
        return false;
    };
    device.epoch = epoch;
    device.lost = true;
    true
}

fn require_physical() -> Result<&'static dyn PhysicalGpuDevice, VgpuError> {
    let physical = physical_device().ok_or(VgpuError::NoPhysicalDevice)?;
    if !physical.ready() {
        return Err(VgpuError::DeviceNotReady);
    }
    Ok(physical)
}

fn allowed_capabilities(
    principal: Principal,
    physical: &'static dyn PhysicalGpuDevice,
) -> Capabilities {
    let info = physical.adapter_info();
    let mut caps = Capabilities::CLIENT_BASE;
    if info.copy {
        caps = caps.union(Capabilities::COPY);
    }
    match principal {
        Principal::KernelRender
        | Principal::KernelRender1
        | Principal::KernelRender2
        | Principal::KernelGpgpuSystem
        | Principal::KernelGpgpuExecution
        | Principal::KernelLfm25
        | Principal::KernelUi4Compositor
        | Principal::KernelUi4Blitter => caps
            .union(Capabilities::PRESENT)
            .union(Capabilities::KERNEL_CONTEXT),
        Principal::HostRuntime | Principal::HullGuest(_) | Principal::RuntimeTest(_) => caps,
    }
}

const fn quota_for(principal: Principal) -> Quota {
    match principal {
        Principal::KernelRender
        | Principal::KernelRender1
        | Principal::KernelRender2
        | Principal::KernelGpgpuSystem
        | Principal::KernelGpgpuExecution
        | Principal::KernelLfm25
        | Principal::KernelUi4Compositor
        | Principal::KernelUi4Blitter => Quota::KERNEL,
        Principal::HostRuntime => Quota::HOST,
        Principal::HullGuest(_) => Quota::GUEST,
        Principal::RuntimeTest(_) => Quota::TEST,
    }
}

fn ensure_kernel_device(
    broker: &mut Broker,
    client: KernelClient,
    root_phys: u64,
) -> Result<DeviceHandle, VgpuError> {
    let principal = client.principal();
    if let Some((handle, device)) = find_device_mut_by_principal(broker, principal) {
        let current_root = match device.gpuvm {
            GpuVmBinding::Owned(_) => return Err(VgpuError::InvalidHandle),
            GpuVmBinding::Borrowed { root_phys } => root_phys,
        };
        if current_root != root_phys {
            device.lost = true;
            return Err(VgpuError::DeviceLost);
        }
        return Ok(handle);
    }
    if root_phys == 0 {
        return Err(VgpuError::Physical(PhysicalGpuError::InvalidGpuVm));
    }
    let mut capabilities = Capabilities::CLIENT_BASE
        .union(Capabilities::PRESENT)
        .union(Capabilities::KERNEL_CONTEXT);
    if client == KernelClient::Ui4Blitter {
        capabilities = capabilities.union(Capabilities::COPY);
    }
    let record = VirtualDevice {
        principal,
        capabilities,
        quota: Quota::KERNEL,
        epoch: broker.epoch,
        lost: false,
        gpuvm: GpuVmBinding::Borrowed { root_phys },
        kernel_context_capability: None,
        next_gpu_va: CLIENT_GPU_VA_BASE,
        memory_used: 0,
        copied_upload_bytes: 0,
        flushed_vvideo_bytes: 0,
        buffers: Vec::new(),
        queues: Vec::new(),
        contexts: Vec::new(),
    };
    Ok(insert_device(broker, record))
}

fn ensure_kernel_queue(
    device: &mut VirtualDevice,
    class: QueueClass,
) -> Result<QueueHandle, VgpuError> {
    if let Some(handle) = find_queue_by_class(device, class) {
        return Ok(handle);
    }
    if device
        .queues
        .iter()
        .filter(|slot| slot.record.is_some())
        .count()
        >= device.quota.queues
    {
        return Err(VgpuError::QuotaExceeded);
    }
    Ok(insert_queue(
        device,
        QueueRecord {
            class,
            timeline: TimelineStatus::default(),
            failed_points: Vec::new(),
        },
    ))
}

fn point_status(
    queue: &QueueRecord,
    point: TimelinePoint,
) -> Result<TimelinePointStatus, VgpuError> {
    if point.value == 0 || point.value > queue.timeline.submitted {
        return Err(VgpuError::InvalidHandle);
    }
    if queue.failed_points.contains(&point.value) {
        return Ok(TimelinePointStatus::Failed);
    }
    if queue.timeline.completed >= point.value {
        Ok(TimelinePointStatus::Complete)
    } else {
        Ok(TimelinePointStatus::Pending)
    }
}

fn device_gpuvm_root(principal: Principal, handle: DeviceHandle) -> Option<u64> {
    let physical = physical_device()?;
    let broker = BROKER.lock();
    let device = lookup_device(&broker, handle, principal).ok()?;
    match device.gpuvm {
        GpuVmBinding::Owned(vm) => physical.gpuvm_root_phys(vm).ok(),
        GpuVmBinding::Borrowed { root_phys } => Some(root_phys),
    }
}

fn destroy_device_resources(
    physical: &'static dyn PhysicalGpuDevice,
    device: &mut VirtualDevice,
) -> Result<(), VgpuError> {
    if device
        .buffers
        .iter()
        .filter_map(|slot| slot.record.as_ref())
        .any(|record| record.in_flight != 0)
    {
        return Err(VgpuError::Busy);
    }
    while let Some(context) = device.contexts.last().map(|binding| binding.context) {
        // GuC destruction is complete only after DEREGISTER_CONTEXT_DONE. If
        // that event is still pending, retain the binding and all backing so a
        // late firmware access can never target recycled storage.
        physical.destroy_context(context)?;
        device.contexts.pop();
    }
    let vm = match device.gpuvm {
        GpuVmBinding::Owned(vm) => Some(vm),
        GpuVmBinding::Borrowed { .. } => None,
    };
    for slot in &mut device.buffers {
        if let Some(record) = slot.record.as_ref() {
            let vm = vm.ok_or(VgpuError::Unsupported)?;
            physical.unmap_gpuvm(vm, record.gpu, record.bytes)?;
        }
        if let Some(mut record) = slot.record.take() {
            release_buffer_backing(&mut record);
        }
    }
    device.memory_used = 0;
    if let Some(vm) = vm {
        physical.destroy_gpuvm(vm)?;
    }
    Ok(())
}

fn release_buffer_backing(record: &mut BufferRecord) {
    match &mut record.backing {
        BufferBacking::Dma { virt, .. } => crate::dma::dealloc(*virt, record.bytes),
        BufferBacking::GuestPages { pages, .. } => {
            for phys in pages.iter().copied() {
                let virt = crate::phys::phys_to_virt(phys as usize) as *mut u8;
                unsafe { core::ptr::write_bytes(virt, 0, PAGE_BYTES) };
                crate::intel::dma_flush(virt, PAGE_BYTES);
            }
        }
    }
}

fn insert_device(broker: &mut Broker, record: VirtualDevice) -> DeviceHandle {
    if let Some((slot, entry)) = broker
        .devices
        .iter_mut()
        .enumerate()
        .find(|(_, entry)| entry.record.is_none())
    {
        entry.generation = entry.generation.wrapping_add(1).max(1);
        entry.record = Some(record);
        return DeviceHandle(encode_handle(slot, entry.generation));
    }
    broker.devices.push(DeviceSlot {
        generation: 1,
        record: Some(record),
    });
    DeviceHandle(encode_handle(broker.devices.len() - 1, 1))
}

fn insert_buffer(device: &mut VirtualDevice, record: BufferRecord) -> BufferHandle {
    if let Some((slot, entry)) = device
        .buffers
        .iter_mut()
        .enumerate()
        .find(|(_, entry)| entry.record.is_none())
    {
        entry.generation = entry.generation.wrapping_add(1).max(1);
        entry.record = Some(record);
        return BufferHandle(encode_handle(slot, entry.generation));
    }
    device.buffers.push(BufferSlot {
        generation: 1,
        record: Some(record),
    });
    BufferHandle(encode_handle(device.buffers.len() - 1, 1))
}

fn insert_queue(device: &mut VirtualDevice, record: QueueRecord) -> QueueHandle {
    if let Some((slot, entry)) = device
        .queues
        .iter_mut()
        .enumerate()
        .find(|(_, entry)| entry.record.is_none())
    {
        entry.generation = entry.generation.wrapping_add(1).max(1);
        entry.record = Some(record);
        return QueueHandle(encode_handle(slot, entry.generation));
    }
    device.queues.push(QueueSlot {
        generation: 1,
        record: Some(record),
    });
    QueueHandle(encode_handle(device.queues.len() - 1, 1))
}

fn lookup_device<'a>(
    broker: &'a Broker,
    handle: DeviceHandle,
    principal: Principal,
) -> Result<&'a VirtualDevice, VgpuError> {
    let (slot, generation) = decode_handle(handle.raw())?;
    let entry = broker.devices.get(slot).ok_or(VgpuError::InvalidHandle)?;
    if entry.generation != generation {
        return Err(VgpuError::InvalidHandle);
    }
    let device = entry.record.as_ref().ok_or(VgpuError::InvalidHandle)?;
    if device.principal != principal {
        return Err(VgpuError::PermissionDenied);
    }
    Ok(device)
}

fn lookup_device_mut<'a>(
    broker: &'a mut Broker,
    handle: DeviceHandle,
    principal: Principal,
) -> Result<&'a mut VirtualDevice, VgpuError> {
    let (slot, generation) = decode_handle(handle.raw())?;
    let entry = broker
        .devices
        .get_mut(slot)
        .ok_or(VgpuError::InvalidHandle)?;
    if entry.generation != generation {
        return Err(VgpuError::InvalidHandle);
    }
    let device = entry.record.as_mut().ok_or(VgpuError::InvalidHandle)?;
    if device.principal != principal {
        return Err(VgpuError::PermissionDenied);
    }
    Ok(device)
}

fn find_device_by_principal(
    broker: &Broker,
    principal: Principal,
) -> Option<(DeviceHandle, &VirtualDevice)> {
    broker.devices.iter().enumerate().find_map(|(slot, entry)| {
        let device = entry.record.as_ref()?;
        (device.principal == principal)
            .then_some((DeviceHandle(encode_handle(slot, entry.generation)), device))
    })
}

fn find_device_mut_by_principal(
    broker: &mut Broker,
    principal: Principal,
) -> Option<(DeviceHandle, &mut VirtualDevice)> {
    broker
        .devices
        .iter_mut()
        .enumerate()
        .find_map(|(slot, entry)| {
            let device = entry.record.as_mut()?;
            (device.principal == principal)
                .then_some((DeviceHandle(encode_handle(slot, entry.generation)), device))
        })
}

fn lookup_buffer(device: &VirtualDevice, handle: BufferHandle) -> Result<&BufferRecord, VgpuError> {
    let (slot, generation) = decode_handle(handle.raw())?;
    let entry = device.buffers.get(slot).ok_or(VgpuError::InvalidHandle)?;
    if entry.generation != generation {
        return Err(VgpuError::InvalidHandle);
    }
    entry.record.as_ref().ok_or(VgpuError::InvalidHandle)
}

fn lookup_buffer_mut(
    device: &mut VirtualDevice,
    handle: BufferHandle,
) -> Result<&mut BufferRecord, VgpuError> {
    let (slot, generation) = decode_handle(handle.raw())?;
    let entry = device
        .buffers
        .get_mut(slot)
        .ok_or(VgpuError::InvalidHandle)?;
    if entry.generation != generation {
        return Err(VgpuError::InvalidHandle);
    }
    entry.record.as_mut().ok_or(VgpuError::InvalidHandle)
}

fn lookup_queue(device: &VirtualDevice, handle: QueueHandle) -> Result<&QueueRecord, VgpuError> {
    let (slot, generation) = decode_handle(handle.raw())?;
    let entry = device.queues.get(slot).ok_or(VgpuError::InvalidHandle)?;
    if entry.generation != generation {
        return Err(VgpuError::InvalidHandle);
    }
    entry.record.as_ref().ok_or(VgpuError::InvalidHandle)
}

fn lookup_queue_mut(
    device: &mut VirtualDevice,
    handle: QueueHandle,
) -> Result<&mut QueueRecord, VgpuError> {
    let (slot, generation) = decode_handle(handle.raw())?;
    let entry = device
        .queues
        .get_mut(slot)
        .ok_or(VgpuError::InvalidHandle)?;
    if entry.generation != generation {
        return Err(VgpuError::InvalidHandle);
    }
    entry.record.as_mut().ok_or(VgpuError::InvalidHandle)
}

fn find_queue_by_class(device: &VirtualDevice, class: QueueClass) -> Option<QueueHandle> {
    device.queues.iter().enumerate().find_map(|(slot, entry)| {
        let queue = entry.record.as_ref()?;
        (queue.class == class).then_some(QueueHandle(encode_handle(slot, entry.generation)))
    })
}

fn ensure_live(device: &VirtualDevice) -> Result<(), VgpuError> {
    if device.lost {
        Err(VgpuError::DeviceLost)
    } else {
        Ok(())
    }
}

const fn encode_handle(slot: usize, generation: u32) -> u64 {
    ((generation as u64) << 32) | slot as u64 + 1
}

const fn decode_handle(raw: u64) -> Result<(usize, u32), VgpuError> {
    let one_based = raw as u32;
    if one_based == 0 {
        return Err(VgpuError::InvalidHandle);
    }
    Ok(((one_based - 1) as usize, (raw >> 32) as u32))
}

fn align_up(value: usize, align: usize) -> Option<usize> {
    value
        .checked_add(align - 1)
        .map(|value| value & !(align - 1))
}

fn align_up_u64(value: u64, align: u64) -> Option<u64> {
    value
        .checked_add(align - 1)
        .map(|value| value & !(align - 1))
}

fn ranges_overlap(a_start: u64, a_bytes: usize, b_start: u64, b_bytes: usize) -> bool {
    let a_end = a_start.saturating_add(a_bytes as u64);
    let b_end = b_start.saturating_add(b_bytes as u64);
    a_start < b_end && b_start < a_end
}

fn vvideo_mapping_digest(epoch: u64, guest_va: u64, gpu: u64, pages: usize) -> u64 {
    // A non-cryptographic identifier for the opaque mapping configuration.
    // Exact PPGTT/HPA identity is reported separately after a page-by-page
    // broker verification; never mix an HPA into a guest-visible digest.
    let mut digest = 0xCBF2_9CE4_8422_2325u64 ^ epoch;
    digest = (digest ^ guest_va).wrapping_mul(0x0000_0100_0000_01B3);
    digest = (digest ^ gpu).wrapping_mul(0x0000_0100_0000_01B3);
    (digest ^ pages as u64).wrapping_mul(0x0000_0100_0000_01B3)
}
