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
    PhysicalContextDescriptor, PhysicalContextHandle, PhysicalGpuDevice, PhysicalGpuError,
    PhysicalGpuVmHandle, PhysicalSchedulerStatus, physical_device,
};

const PAGE_BYTES: usize = 4096;
const CLIENT_GPU_VA_BASE: u64 = 0x1_0000_0000;
const CLIENT_GPU_VA_LIMIT: u64 = 0x0000_7FFF_0000_0000;
pub(crate) const BUFFER_USAGE_MAP_READ: u32 = 1 << 0;
pub(crate) const BUFFER_USAGE_MAP_WRITE: u32 = 1 << 1;

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
    Render,
    Gpgpu,
    /// Persistent UI4 composition queue.  This is deliberately a separate
    /// virtual device/principal from general kernel GPGPU: UI4 may leave one
    /// frame in flight while video conversion, fonts, and application compute
    /// continue to submit through `Gpgpu`.
    Ui4Compositor,
}

impl KernelClient {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Render => "kernel-render",
            Self::Gpgpu => "kernel-gpgpu",
            Self::Ui4Compositor => "kernel-ui4-compositor",
        }
    }

    const fn principal(self) -> Principal {
        match self {
            Self::Render => Principal::KernelRender,
            Self::Gpgpu => Principal::KernelGpgpu,
            Self::Ui4Compositor => Principal::KernelUi4Compositor,
        }
    }

    const fn queue_class(self) -> QueueClass {
        match self {
            Self::Render => QueueClass::Render,
            Self::Gpgpu => QueueClass::Compute,
            Self::Ui4Compositor => QueueClass::Compute,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Principal {
    KernelRender,
    KernelGpgpu,
    KernelUi4Compositor,
    HostRuntime,
    HullGuest(u16),
    RuntimeTest(u16),
}

impl Principal {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::KernelRender => "kernel-render",
            Self::KernelGpgpu => "kernel-gpgpu",
            Self::KernelUi4Compositor => "kernel-ui4-compositor",
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

struct BufferRecord {
    phys: u64,
    virt: *mut u8,
    bytes: usize,
    gpu: u64,
    usage: u32,
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
    next_gpu_va: u64,
    memory_used: usize,
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
    pub(crate) devices: Vec<DeviceStatusSnapshot>,
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
        next_gpu_va: CLIENT_GPU_VA_BASE,
        memory_used: 0,
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
            phys,
            virt,
            bytes: alloc_bytes,
            gpu,
            usage,
        },
    ))
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
    })
}

pub(crate) fn write_buffer(
    principal: Principal,
    device_handle: DeviceHandle,
    buffer_handle: BufferHandle,
    offset: usize,
    bytes: &[u8],
) -> Result<usize, VgpuError> {
    let broker = BROKER.lock();
    let device = lookup_device(&broker, device_handle, principal)?;
    ensure_live(device)?;
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
    if !bytes.is_empty() {
        unsafe {
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), record.virt.add(offset), bytes.len());
        }
        crate::intel::dma_flush(unsafe { record.virt.add(offset) }, bytes.len());
    }
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
    if !out.is_empty() {
        crate::intel::dma_flush(unsafe { record.virt.add(offset) }, out.len());
        unsafe {
            core::ptr::copy_nonoverlapping(record.virt.add(offset), out.as_mut_ptr(), out.len());
        }
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
    let vm = match device.gpuvm {
        GpuVmBinding::Owned(vm) => vm,
        GpuVmBinding::Borrowed { .. } => return Err(VgpuError::Unsupported),
    };
    physical.unmap_gpuvm(vm, record.gpu, record.bytes)?;
    let record = buffer_slot.record.take().expect("validated vgpu buffer");
    device.memory_used = device.memory_used.saturating_sub(record.bytes);
    crate::dma::dealloc(record.virt, record.bytes);
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
    let physical = require_physical()?;
    let mut broker = BROKER.lock();
    let device_handle = ensure_kernel_device(&mut broker, client, descriptor.gpuvm_root_phys)?;
    let device = lookup_device_mut(&mut broker, device_handle, client.principal())?;
    ensure_live(device)?;
    let queue_handle = ensure_kernel_queue(device, client.queue_class())?;

    let context = if let Some(binding) = device
        .contexts
        .iter()
        .find(|binding| binding.queue == queue_handle && binding.descriptor == descriptor)
    {
        binding.context
    } else {
        if device.contexts.len() >= device.quota.contexts {
            return Err(VgpuError::QuotaExceeded);
        }
        let context = physical.register_context(descriptor)?;
        device.contexts.push(ContextBinding {
            queue: queue_handle,
            descriptor,
            context,
        });
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
    })
}

pub(crate) fn kernel_timeline(client: KernelClient) -> Option<TimelineStatus> {
    let broker = BROKER.lock();
    let (_, device) = find_device_by_principal(&broker, client.principal())?;
    let queue = find_queue_by_class(device, client.queue_class())?;
    lookup_queue(device, queue).ok().map(|queue| queue.timeline)
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

pub(crate) fn broker_status() -> BrokerStatus {
    let physical = physical_device();
    let info = physical.map(|device| device.adapter_info());
    let scheduler = physical
        .map(|device| device.scheduler_status())
        .unwrap_or_default();
    let broker = BROKER.lock();
    let mut devices = Vec::new();
    for (slot, entry) in broker.devices.iter().enumerate() {
        let Some(device) = entry.record.as_ref() else {
            continue;
        };
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
        devices,
    }
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
        | Principal::KernelGpgpu
        | Principal::KernelUi4Compositor => caps
            .union(Capabilities::PRESENT)
            .union(Capabilities::KERNEL_CONTEXT),
        Principal::HostRuntime | Principal::HullGuest(_) | Principal::RuntimeTest(_) => caps,
    }
}

const fn quota_for(principal: Principal) -> Quota {
    match principal {
        Principal::KernelRender
        | Principal::KernelGpgpu
        | Principal::KernelUi4Compositor => Quota::KERNEL,
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
    let capabilities = Capabilities::CLIENT_BASE
        .union(Capabilities::PRESENT)
        .union(Capabilities::KERNEL_CONTEXT);
    let record = VirtualDevice {
        principal,
        capabilities,
        quota: Quota::KERNEL,
        epoch: broker.epoch,
        lost: false,
        gpuvm: GpuVmBinding::Borrowed { root_phys },
        next_gpu_va: CLIENT_GPU_VA_BASE,
        memory_used: 0,
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
    while let Some(binding) = device.contexts.pop() {
        physical.destroy_context(binding.context)?;
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
        if let Some(record) = slot.record.take() {
            crate::dma::dealloc(record.virt, record.bytes);
        }
    }
    device.memory_used = 0;
    if let Some(vm) = vm {
        physical.destroy_gpuvm(vm)?;
    }
    Ok(())
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
