//! Software-mediated virtual GPU devices.
//!
//! This is TRUEOS's policy and ownership boundary.  Every caller receives a
//! principal-scoped, generation-tagged device with its own GPUVM, resource
//! handles, queues, quotas, and virtual timelines.  Existing kernel render and
//! GPGPU contexts are adopted as privileged devices while retaining their
//! validated LRC/ring/PPGTT layouts.

extern crate alloc;

use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;
use trueos_time::{Duration, Timer};

use super::physical::{
    PhysicalBufferSlice, PhysicalContextDescriptor, PhysicalContextFaultKind,
    PhysicalContextHandle, PhysicalContextPriority, PhysicalGpuDevice, PhysicalGpuError,
    PhysicalGpuFault, PhysicalGpuVmHandle, PhysicalSchedulerStatus, physical_device,
};

const PAGE_BYTES: usize = 4096;
const CLIENT_GPU_VA_BASE: u64 = 0x1_0000_0000;
const CLIENT_GPU_VA_LIMIT: u64 = 0x0000_7FFF_0000_0000;
pub(crate) const BUFFER_USAGE_MAP_READ: u32 = 1 << 0;
pub(crate) const BUFFER_USAGE_MAP_WRITE: u32 = 1 << 1;
pub(crate) const BUFFER_USAGE_STORAGE: u32 = 1 << 2;
pub(crate) const BUFFER_USAGE_COPY_SRC: u32 = 1 << 3;
pub(crate) const BUFFER_USAGE_COPY_DST: u32 = 1 << 4;
pub(crate) const BUFFER_USAGE_VERTEX: u32 = 1 << 5;
pub(crate) const BUFFER_USAGE_INDEX: u32 = 1 << 6;
pub(crate) const BUFFER_INFO_FLAG_VVIDEO_MEM: u32 = 1 << 0;
const BUFFER_USAGE_ALL: u32 = BUFFER_USAGE_MAP_READ
    | BUFFER_USAGE_MAP_WRITE
    | BUFFER_USAGE_STORAGE
    | BUFFER_USAGE_COPY_SRC
    | BUFFER_USAGE_COPY_DST
    | BUFFER_USAGE_VERTEX
    | BUFFER_USAGE_INDEX;
pub(crate) const SAMPLER_FLAGS_ALL: u32 = 0xF;
pub(crate) const SAMPLER_ADDRESS_U_REPEAT: u32 = 1 << 0;
pub(crate) const SAMPLER_ADDRESS_V_REPEAT: u32 = 1 << 1;

pub(crate) const SHADER_PACKAGE_CLIP_POSITION3_RGBA_FNV1A64: u64 = 0x1438_5963_136A_A36F;
pub(crate) const SHADER_PACKAGE_CLIP_POSITION3_IMMEDIATE_RGBA_FNV1A64: u64 = 0x4A7C_D238_6AA5_C232;
pub(crate) const SHADER_PACKAGE_CLIP_POSITION3_UV_TEXTURE_FNV1A64: u64 = 0xD2A3_B942_FA09_24B6;
pub(crate) const SHADER_PACKAGE_CLIP_POSITION3_UV_TEXEL_LOAD_FNV1A64: u64 = 0x0CFE_4DDB_C885_8871;
const SHADER_PACKAGE_CLIP_POSITION3_RGBA_COLOR: u32 = u32::from_le_bytes([118, 221, 153, 255]);
pub(crate) const MAX_INDEXED_BATCH_DRAWS: usize = 16;

/// The only native cloud graph currently understood by the broker.  This is
/// a profile selector, not a shader ID supplied by the tenant: the kernel maps
/// it to the two baked, full-digest-authenticated Helio stages.
pub(crate) const CLOUD_PROFILE_HELIO_ENGINE_V1: u32 = 1;
pub(crate) const CLOUD_FRAME_MAX_SIMULATION_STEPS: u32 = 2;
const CLOUD_WORK_GRAPH_LIMIT: usize = 4;
const CLOUD_VOLUME_BYTES: usize = 3_538_944;
const CLOUD_SIM_PARAMS_BYTES: usize = 112;
const CLOUD_RENDER_PARAMS_BYTES: usize = 272;
const CLOUD_VOLUME_REQUIRED_USAGE: u32 =
    BUFFER_USAGE_STORAGE | BUFFER_USAGE_COPY_SRC | BUFFER_USAGE_COPY_DST;
const CLOUD_PARAMS_REQUIRED_USAGE: u32 = BUFFER_USAGE_STORAGE | BUFFER_USAGE_COPY_DST;

/// SHA-256 of the authored WGSL accepted by HelioC.  The eventual native
/// payload must carry these identities in its sealed compiler metadata; a
/// truncated application digest is never sufficient for execution admission.
pub(crate) const CLOUD_SIMULATION_WGSL_SHA256: [u8; 32] = [
    0xf5, 0x83, 0xd3, 0xc6, 0x3e, 0x5f, 0x38, 0x7a, 0x59, 0x26, 0x28, 0x1d, 0xf2, 0x9b, 0x76, 0x88,
    0xeb, 0x09, 0xea, 0xa5, 0xf0, 0x61, 0x19, 0xd7, 0x4f, 0xff, 0xa7, 0x0d, 0x59, 0x20, 0x13, 0xf6,
];
pub(crate) const CLOUD_RENDER_WGSL_SHA256: [u8; 32] = [
    0x5d, 0x53, 0x6a, 0x46, 0x8f, 0xcb, 0x69, 0x8c, 0x3d, 0xca, 0x79, 0xfa, 0xac, 0x0e, 0x5a, 0x49,
    0x24, 0xfd, 0xc8, 0xce, 0x2c, 0x9c, 0xe3, 0xa9, 0xb5, 0xd2, 0x4c, 0x40, 0xa8, 0x4c, 0xc9, 0xff,
];

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
    /// Ordered compute lane for retained UI producers and general-purpose
    /// synchronous operations. Font Engine work has its own client below.
    GpgpuSystem,
    /// Font Engine lane with its own persistent HWLRCA, ring, PPGTT root, GuC
    /// registration, and exact timeline. Font work must never rewrite or
    /// quarantine the general system-service context.
    GpgpuFont,
    /// Independent compute lane for continuously executing GPU programs. Its
    /// context may remain in flight without blocking system-service compute.
    GpgpuExecution,
    /// Authored Helio Cloud Engine frame graph. This mixed compute/render lane
    /// owns one persistent RCS0 context and remains a normal-priority producer;
    /// it must never borrow Font Engine's high-priority context or storage.
    HelioCloud,
    /// Fixed-model compute lane with its own persistent PPGTT and GuC context.
    Lfm25,
    /// Persistent UI4 composition queue.  This is deliberately a separate
    /// virtual device/principal from general kernel GPGPU: UI4 may leave one
    /// frame in flight while video conversion and application compute continue
    /// through `GpgpuSystem`, and Font Engine work through `GpgpuFont`.
    Ui4Compositor,
    /// Persistent GuC-owned BCS0 lane for UI4 copies and composition staging.
    /// Keeping it separate from the RCS compositor lane gives copy work its
    /// own backpressure and completion timeline.
    Ui4Blitter,
}

impl KernelClient {
    pub(crate) const RENDER_CARRIERS: [Self; 3] = [Self::Render, Self::Render1, Self::Render2];

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) const fn render_carrier(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::Render),
            1 => Some(Self::Render1),
            2 => Some(Self::Render2),
            _ => None,
        }
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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
            Self::GpgpuFont => "kernel-gpgpu-font",
            Self::GpgpuExecution => "kernel-gpgpu-execution",
            Self::HelioCloud => "kernel-helio-cloud",
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
            Self::GpgpuFont => Principal::KernelGpgpuFont,
            Self::GpgpuExecution => Principal::KernelGpgpuExecution,
            Self::HelioCloud => Principal::KernelHelioCloud,
            Self::Lfm25 => Principal::KernelLfm25,
            Self::Ui4Compositor => Principal::KernelUi4Compositor,
            Self::Ui4Blitter => Principal::KernelUi4Blitter,
        }
    }

    const fn queue_class(self) -> QueueClass {
        match self {
            Self::Render | Self::Render1 | Self::Render2 | Self::HelioCloud => QueueClass::Render,
            Self::GpgpuSystem | Self::GpgpuFont | Self::GpgpuExecution | Self::Lfm25 => {
                QueueClass::Compute
            }
            Self::Ui4Compositor => QueueClass::Compute,
            Self::Ui4Blitter => QueueClass::Copy,
        }
    }

    const fn physical_priority(self) -> PhysicalContextPriority {
        match self {
            // UI4 is the finite downstream scanout combiner. Ordinary
            // composition admits one pending job, so it may preempt producers
            // without manufacturing an unbounded high-priority queue.
            //
            // LFM submissions are likewise bounded interactive batches. A
            // normal-priority context added one repeatable scheduler quantum
            // to every model projection on ADL-S, overwhelming the actual
            // kernel time. Each LFM batch remains bounded to at most three
            // projection walkers, so it cannot turn into a persistent
            // high-priority program.
            // GPGPU system and Font Engine submissions are likewise bounded
            // synchronous kernels. They produce visible retained UI surfaces,
            // so normal priority adds one complete GuC scheduler quantum to
            // every copy/coverage/release stage while UI4 motion remains crisp.
            Self::GpgpuSystem | Self::GpgpuFont | Self::Ui4Compositor | Self::Lfm25 => {
                PhysicalContextPriority::KernelHigh
            }
            // Retained Render carriers and the Spirit/Lab256 execution lane
            // are independent producers. Both can remain continuously
            // runnable even though each request is bounded. Keep them as
            // normal-priority peers so neither side consumer can invert the
            // other, while the downstream compositor can drain a completed
            // frame promptly. The copy-only blitter is not scanout-critical.
            Self::Render
            | Self::Render1
            | Self::Render2
            | Self::GpgpuExecution
            | Self::HelioCloud
            | Self::Ui4Blitter => PhysicalContextPriority::KernelNormal,
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
            | Self::GpgpuFont
            | Self::GpgpuExecution
            | Self::HelioCloud
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
        KernelClient::Render1.physical_priority(),
        PhysicalContextPriority::KernelNormal
    ));
    assert!(matches!(
        KernelClient::Render2.physical_priority(),
        PhysicalContextPriority::KernelNormal
    ));
    assert!(matches!(
        KernelClient::GpgpuSystem.physical_priority(),
        PhysicalContextPriority::KernelHigh
    ));
    assert!(matches!(
        KernelClient::GpgpuFont.physical_priority(),
        PhysicalContextPriority::KernelHigh
    ));
    assert!(matches!(
        KernelClient::GpgpuExecution.physical_priority(),
        PhysicalContextPriority::KernelNormal
    ));
    assert!(matches!(
        KernelClient::HelioCloud.physical_priority(),
        PhysicalContextPriority::KernelNormal
    ));
    assert!(matches!(KernelClient::Lfm25.physical_priority(), PhysicalContextPriority::KernelHigh));
    assert!(matches!(
        KernelClient::Ui4Blitter.physical_priority(),
        PhysicalContextPriority::KernelNormal
    ));
};

#[cfg(test)]
mod kernel_client_priority_tests {
    use super::{KernelClient, PhysicalContextPriority};

    #[test]
    fn retained_render_and_side_execution_are_peer_producers() {
        for client in KernelClient::RENDER_CARRIERS {
            assert_eq!(
                client.physical_priority(),
                PhysicalContextPriority::KernelNormal,
                "{} must remain a fair producer peer",
                client.name(),
            );
        }
        assert_eq!(
            KernelClient::GpgpuExecution.physical_priority(),
            PhysicalContextPriority::KernelNormal,
        );
        assert_eq!(
            KernelClient::HelioCloud.physical_priority(),
            PhysicalContextPriority::KernelNormal,
        );
    }

    #[test]
    fn ui4_compositor_is_downstream_priority() {
        assert_eq!(
            KernelClient::Ui4Compositor.physical_priority(),
            PhysicalContextPriority::KernelHigh,
        );
    }

    #[test]
    fn copy_only_ui4_blitter_remains_normal_priority() {
        assert_eq!(
            KernelClient::Ui4Blitter.physical_priority(),
            PhysicalContextPriority::KernelNormal,
        );
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Principal {
    KernelRender,
    KernelRender1,
    KernelRender2,
    KernelGpgpuSystem,
    KernelGpgpuFont,
    KernelGpgpuExecution,
    KernelHelioCloud,
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
            Self::KernelGpgpuFont => "kernel-gpgpu-font",
            Self::KernelGpgpuExecution => "kernel-gpgpu-execution",
            Self::KernelHelioCloud => "kernel-helio-cloud",
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
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) const fn from_raw(raw: u32) -> Option<Self> {
        match raw {
            1 => Some(Self::Render),
            2 => Some(Self::Compute),
            3 => Some(Self::Copy),
            _ => None,
        }
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) const fn raw(self) -> u32 {
        match self {
            Self::Render => 1,
            Self::Compute => 2,
            Self::Copy => 3,
        }
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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
pub(crate) struct SurfaceHandle(u64);

impl SurfaceHandle {
    pub(crate) const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    pub(crate) const fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub(crate) struct ShaderModuleHandle(u64);

impl ShaderModuleHandle {
    pub(crate) const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }
    pub(crate) const fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub(crate) struct RenderPipelineHandle(u64);

impl RenderPipelineHandle {
    pub(crate) const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }
    pub(crate) const fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub(crate) struct CloudWorkGraphHandle(u64);

impl CloudWorkGraphHandle {
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

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct CloudFrameTelemetry {
    pub(crate) point: TimelinePoint,
    pub(crate) gpu_active_ns: u64,
    pub(crate) budget_window_ns: u64,
    pub(crate) simulation_steps: u32,
    pub(crate) simd_width: u32,
    pub(crate) flags: u32,
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
pub(crate) struct Ui4SurfaceDescriptor {
    pub(crate) window_id: u32,
    pub(crate) phys: u64,
    /// Existing kernel producer address for the retained UI4 allocation. This
    /// is never exposed to the guest; the broker separately maps `phys` into
    /// the tenant GPUVM and uses this address only for mediated execution.
    pub(crate) producer_gpu: u64,
    pub(crate) bytes: usize,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pitch: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct SurfaceInfo {
    pub(crate) handle: SurfaceHandle,
    pub(crate) bytes: usize,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pitch: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) struct BufferSlice {
    pub(crate) buffer: BufferHandle,
    pub(crate) offset: usize,
    pub(crate) bytes: usize,
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
        // WGPU engines intentionally split persistent scene state across many
        // small buffers.  Keep the byte quota as the isolation boundary, but
        // allow enough opaque handles for a normal Helio scene graph.
        buffers: 256,
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
        #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
        phys: u64,
        virt: *mut u8,
    },
    GuestPages {
        #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
        vm_id: u8,
        guest_va: u64,
        pages: Vec<u64>,
        /// Allocator ownership remains pinned until a definitive PPGTT unmap.
        /// `None` is fail-closed quarantine, never permission to reuse pages.
        dma_pin: Option<crate::allocators::HvGuestDmaPin>,
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

struct SurfaceRecord {
    window_id: u32,
    phys: u64,
    bytes: usize,
    gpu: u64,
    producer_gpu: u64,
    width: u32,
    height: u32,
    pitch: u32,
    epoch: u64,
    in_flight: u32,
}

struct SurfaceSlot {
    generation: u32,
    record: Option<SurfaceRecord>,
}

struct ShaderModuleRecord {
    package_digest: u64,
    epoch: u64,
}

struct ShaderModuleSlot {
    generation: u32,
    record: Option<ShaderModuleRecord>,
}

struct RenderPipelineRecord {
    package_digest: u64,
    vertex_stride: u32,
    position_offset: u32,
    epoch: u64,
}

struct RenderPipelineSlot {
    generation: u32,
    record: Option<RenderPipelineRecord>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct CloudWorkGraphDescriptor {
    pub(crate) volume_a: BufferHandle,
    pub(crate) volume_b: BufferHandle,
    pub(crate) sim_params: BufferHandle,
    pub(crate) render_params: BufferHandle,
    pub(crate) profile: u32,
}

struct CloudWorkGraphRecord {
    resources: CloudWorkGraphDescriptor,
    /// Immutable physical backing authenticated once when the graph is
    /// created. Frame submission only clones four `Arc` handles; it never
    /// recopies the 1,730-page ping-pong map on the CPU hot path.
    buffer_metadata: [CloudBufferMetadata; 4],
    backing_admission: crate::intel::gpgpu::HelioCloudBackingAdmission,
    epoch: u64,
    /// The volume sampled by a zero-step render. Each completed simulation
    /// step flips this selector; the tenant never supplies a GPU address or a
    /// ping-pong target.
    current_volume: u8,
    in_flight: u32,
}

struct CloudSubmissionLease {
    device_epoch: u64,
    queue: QueueHandle,
    graph: CloudWorkGraphHandle,
    surface: SurfaceHandle,
    buffers: [BufferHandle; 4],
    buffer_metadata: [CloudBufferMetadata; 4],
    backing_admission: crate::intel::gpgpu::HelioCloudBackingAdmission,
    surface_metadata: CloudSurfaceMetadata,
    graph_resources: CloudWorkGraphDescriptor,
    current_volume: u8,
    simulation_steps: u32,
}

#[derive(Copy, Clone)]
struct CloudSurfaceMetadata {
    window_id: u32,
    gpu: u64,
    phys: u64,
    producer_gpu: u64,
    bytes: usize,
    width: u32,
    height: u32,
    pitch: u32,
}

#[derive(Clone)]
struct CloudBufferMetadata {
    gpu: u64,
    bytes: usize,
    usage: u32,
    epoch: u64,
    mapping_digest: u64,
    pages: Arc<[u64]>,
}

#[expect(
    dead_code,
    reason = "activated by the authenticated HelioC encoder rung"
)]
enum CloudNativeSubmitResult {
    /// A future authenticated encoder may return its real fence and telemetry.
    Submitted(CloudFrameTelemetry),
    /// Native ownership became uncertain; callers must quarantine the lease.
    Ambiguous,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum CloudResourceRole {
    Volume,
    SimulationParams,
    RenderParams,
}

struct CloudWorkGraphSlot {
    generation: u32,
    record: Option<CloudWorkGraphRecord>,
}

struct QueueRecord {
    class: QueueClass,
    timeline: TimelineStatus,
    failed_points: Vec<u64>,
    /// Broker-visible operation lease. Long synchronous physical calls release
    /// `BROKER`, so queue destruction must remain fenced independently.
    in_flight: u32,
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
    surfaces: Vec<SurfaceSlot>,
    shader_modules: Vec<ShaderModuleSlot>,
    render_pipelines: Vec<RenderPipelineSlot>,
    cloud_work_graphs: Vec<CloudWorkGraphSlot>,
    queues: Vec<QueueSlot>,
    contexts: Vec<ContextBinding>,
}

struct DeviceSlot {
    generation: u32,
    record: Option<VirtualDevice>,
}

struct Broker {
    epoch: u64,
    /// Sticky until a real device reset/reboot. This makes unattributed GuC
    /// faults idempotent and prevents new mediated devices entering a GT whose
    /// ownership can no longer be established.
    physical_lost: bool,
    devices: Vec<DeviceSlot>,
}

static BROKER: Mutex<Broker> = Mutex::new(Broker {
    epoch: 1,
    physical_lost: false,
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
    pub(crate) physical_lost: bool,
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
    pub(crate) helio_render_live: bool,
    pub(crate) spirit_execution_live: bool,
    pub(crate) font_engine_live: bool,
    pub(crate) helio_spirit_distinct_hwlrca: bool,
    pub(crate) helio_spirit_distinct_ppgtt_root: bool,
    pub(crate) font_helio_distinct_hwlrca: bool,
    pub(crate) font_helio_distinct_ppgtt_root: bool,
    pub(crate) font_spirit_distinct_hwlrca: bool,
    pub(crate) font_spirit_distinct_ppgtt_root: bool,
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

    pub(crate) const fn helio_spirit_valid(self) -> bool {
        self.helio_render_live
            && self.spirit_execution_live
            && self.helio_spirit_distinct_hwlrca
            && self.helio_spirit_distinct_ppgtt_root
    }

    pub(crate) const fn font_helio_spirit_valid(self) -> bool {
        self.font_engine_live
            && self.helio_spirit_valid()
            && self.font_helio_distinct_hwlrca
            && self.font_helio_distinct_ppgtt_root
            && self.font_spirit_distinct_hwlrca
            && self.font_spirit_distinct_ppgtt_root
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
    if broker.physical_lost {
        // No work was admitted through this VM. Reclaim its CPU-owned page
        // tables instead of publishing a new device after global GT loss.
        drop(broker);
        let _ = physical.destroy_gpuvm(gpuvm);
        return Err(VgpuError::DeviceLost);
    }
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
        surfaces: Vec::new(),
        shader_modules: Vec::new(),
        render_pipelines: Vec::new(),
        cloud_work_graphs: Vec::new(),
        queues: Vec::new(),
        contexts: Vec::new(),
    };
    Ok(insert_device(&mut broker, record))
}

pub(crate) fn close(principal: Principal, handle: DeviceHandle) -> Result<(), VgpuError> {
    let physical = require_physical()?;
    let mut broker = BROKER.lock();
    let device = lookup_device(&broker, handle, principal)?;
    if device_has_operation_leases(device) {
        // A known in-flight operation owns its queue and pages. An
        // administrative close attempt is not device loss and must not poison
        // the operation's eventual exact lease release.
        return Err(VgpuError::Busy);
    }
    let mut fault_service = PhysicalFaultServiceResult::default();
    let contexts = destroy_device_contexts_locked(
        &mut broker,
        physical,
        handle,
        principal,
        &mut fault_service,
    );
    let result = match contexts {
        Ok(_) => match decode_handle(handle.raw()) {
            Err(error) => Err(error),
            Ok((slot, generation)) => match broker.devices.get_mut(slot) {
                None => Err(VgpuError::InvalidHandle),
                Some(device_slot) if device_slot.generation != generation => {
                    Err(VgpuError::InvalidHandle)
                }
                Some(device_slot) => match device_slot.record.take() {
                    None => Err(VgpuError::InvalidHandle),
                    Some(mut device) => match destroy_device_resources(physical, &mut device) {
                        Ok(()) => Ok(()),
                        Err(error) => {
                            if error != VgpuError::Busy {
                                device.lost = true;
                            }
                            device_slot.record = Some(device);
                            Err(error)
                        }
                    },
                },
            },
        },
        Err(error) => {
            if let Ok(device) = lookup_device_mut(&mut broker, handle, principal) {
                device.lost = true;
            }
            Err(error)
        }
    };
    drop(broker);
    finish_physical_gpu_fault_service(fault_service);
    result
}

/// Tear down every vGPU device owned by a Hull VM at its VMX lifetime
/// boundary. A subsequent occupant of the same VM slot receives fresh handle
/// generations and a new broker epoch. Resources whose GPU ownership is
/// uncertain remain installed, lost, and pinned rather than being reused.
pub(crate) fn release_hull_guest(vm_id: u8) -> (usize, usize, u64) {
    let principal = Principal::HullGuest(vm_id as u16);
    let Some(physical) = physical_device().filter(|device| device.ready()) else {
        let broker = BROKER.lock();
        let quarantined = broker
            .devices
            .iter()
            .filter_map(|slot| slot.record.as_ref())
            .filter(|device| device.principal == principal)
            .count();
        return (0, quarantined, broker.epoch);
    };
    let mut broker = BROKER.lock();
    broker.epoch = broker.epoch.wrapping_add(1).max(1);
    let mut released = 0usize;
    let mut quarantined = 0usize;
    let handles: Vec<DeviceHandle> = broker
        .devices
        .iter()
        .enumerate()
        .filter_map(|(slot, entry)| {
            entry
                .record
                .as_ref()
                .filter(|device| device.principal == principal)
                .map(|_| DeviceHandle::from_raw(encode_handle(slot, entry.generation)))
        })
        .collect();
    let mut fault_service = PhysicalFaultServiceResult::default();
    for handle in handles {
        if lookup_device(&broker, handle, principal).is_ok_and(device_has_operation_leases) {
            // The VM CPU is gone, but a physical operation still has an exact
            // completion owner. Keep the device live long enough for that
            // completion to release its leases; the VM slot/heap gate below
            // prevents a new tenant from reusing the pages meanwhile.
            quarantined = quarantined.saturating_add(1);
            continue;
        }
        let contexts = destroy_device_contexts_locked(
            &mut broker,
            physical,
            handle,
            principal,
            &mut fault_service,
        );
        if contexts.is_err() {
            let current_epoch = broker.epoch;
            if let Ok(device) = lookup_device_mut(&mut broker, handle, principal) {
                device.epoch = current_epoch;
                device.lost = true;
            }
            quarantined = quarantined.saturating_add(1);
            continue;
        }
        let Ok((slot, generation)) = decode_handle(handle.raw()) else {
            quarantined = quarantined.saturating_add(1);
            continue;
        };
        let current_epoch = broker.epoch;
        let Some(device_slot) = broker.devices.get_mut(slot) else {
            quarantined = quarantined.saturating_add(1);
            continue;
        };
        if device_slot.generation != generation {
            quarantined = quarantined.saturating_add(1);
            continue;
        }
        let Some(mut device) = device_slot.record.take() else {
            quarantined = quarantined.saturating_add(1);
            continue;
        };
        match destroy_device_resources(physical, &mut device) {
            Ok(()) => released = released.saturating_add(1),
            Err(_) => {
                device.epoch = current_epoch;
                device.lost = true;
                device_slot.record = Some(device);
                quarantined = quarantined.saturating_add(1);
            }
        }
    }
    let epoch = broker.epoch;
    drop(broker);
    finish_physical_gpu_fault_service(fault_service);
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
    // Pin the allocator allocation before resolving or publishing any PPGTT
    // mapping. This closes the raw-ABI path where the guest could otherwise
    // return these pages to its free list while the GPU still named them.
    let dma_pin = crate::allocators::pin_hv_guest_dma_range(vm_id, guest_va, bytes)
        .ok_or(VgpuError::PermissionDenied)?;
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
            // `map_gpuvm` may fail during post-write verification, so include
            // the just-attempted page in rollback rather than assuming an
            // error proves that no PTE was installed.
            let rollback_bytes = mapped.saturating_add(1).saturating_mul(PAGE_BYTES);
            if physical.unmap_gpuvm(vm, gpu, rollback_bytes).is_err() {
                // Rollback did not prove that the partial PPGTT mapping is
                // gone. Keep the allocation pin and fence the whole vGPU/VM
                // lifecycle rather than hand late-write pages back to either
                // allocator or a future VM-slot occupant.
                device.lost = true;
                dma_pin.quarantine();
            }
            return Err(error.into());
        }
        mapped += 1;
    }
    match physical.verify_gpuvm_pages(vm, gpu, &pages) {
        Ok(true) => {}
        Ok(false) => {
            if physical.unmap_gpuvm(vm, gpu, bytes).is_err() {
                device.lost = true;
                dma_pin.quarantine();
            }
            return Err(VgpuError::Physical(PhysicalGpuError::MapFailed));
        }
        Err(error) => {
            if physical.unmap_gpuvm(vm, gpu, bytes).is_err() {
                device.lost = true;
                dma_pin.quarantine();
            }
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
                dma_pin: Some(dma_pin.retain_for_mapping()),
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
    if device
        .cloud_work_graphs
        .iter()
        .filter_map(|slot| slot.record.as_ref())
        .any(|graph| cloud_graph_references_buffer(graph, buffer_handle))
    {
        return Err(VgpuError::Busy);
    }
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

/// Seal the four tenant allocations used by the authored Helio Cloud Engine
/// into one bounded, opaque work graph. The tenant supplies resources, never
/// shader code, GPU addresses, state selectors, or an arbitrary dispatch.
pub(crate) fn create_cloud_work_graph(
    principal: Principal,
    device_handle: DeviceHandle,
    descriptor: CloudWorkGraphDescriptor,
) -> Result<CloudWorkGraphHandle, VgpuError> {
    if descriptor.profile != CLOUD_PROFILE_HELIO_ENGINE_V1 {
        return Err(VgpuError::Unsupported);
    }
    let handles = [
        descriptor.volume_a.raw(),
        descriptor.volume_b.raw(),
        descriptor.sim_params.raw(),
        descriptor.render_params.raw(),
    ];
    if handles.contains(&0)
        || handles
            .iter()
            .enumerate()
            .any(|(index, handle)| handles[..index].contains(handle))
    {
        return Err(VgpuError::InvalidHandle);
    }

    let mut broker = BROKER.lock();
    let device = lookup_device_mut(&mut broker, device_handle, principal)?;
    ensure_live(device)?;
    let required_capabilities = Capabilities::COMPUTE
        .union(Capabilities::RENDER)
        .union(Capabilities::PRESENT);
    if !device.capabilities.contains(required_capabilities) {
        return Err(VgpuError::PermissionDenied);
    }
    if device
        .cloud_work_graphs
        .iter()
        .filter(|slot| slot.record.is_some())
        .count()
        >= CLOUD_WORK_GRAPH_LIMIT
    {
        return Err(VgpuError::QuotaExceeded);
    }

    validate_cloud_buffer(device, descriptor.volume_a, CloudResourceRole::Volume)?;
    validate_cloud_buffer(device, descriptor.volume_b, CloudResourceRole::Volume)?;
    validate_cloud_buffer(device, descriptor.sim_params, CloudResourceRole::SimulationParams)?;
    validate_cloud_buffer(device, descriptor.render_params, CloudResourceRole::RenderParams)?;
    let buffer_metadata = [
        snapshot_cloud_buffer(device, descriptor.volume_a)?,
        snapshot_cloud_buffer(device, descriptor.volume_b)?,
        snapshot_cloud_buffer(device, descriptor.sim_params)?,
        snapshot_cloud_buffer(device, descriptor.render_params)?,
    ];
    let backing_admission = crate::intel::gpgpu::admit_helioc_backing(
        cloud_buffer_pages(device, descriptor.volume_a)?,
        cloud_buffer_pages(device, descriptor.volume_b)?,
        cloud_buffer_pages(device, descriptor.sim_params)?,
        cloud_buffer_pages(device, descriptor.render_params)?,
    )
    .ok_or(VgpuError::Unsupported)?;

    Ok(insert_cloud_work_graph(
        device,
        CloudWorkGraphRecord {
            resources: descriptor,
            buffer_metadata,
            backing_admission,
            epoch: device.epoch,
            current_volume: 0,
            in_flight: 0,
        },
    ))
}

pub(crate) fn destroy_cloud_work_graph(
    principal: Principal,
    device_handle: DeviceHandle,
    graph_handle: CloudWorkGraphHandle,
) -> Result<(), VgpuError> {
    let mut broker = BROKER.lock();
    let device = lookup_device_mut(&mut broker, device_handle, principal)?;
    ensure_live(device)?;
    let graph = lookup_cloud_work_graph(device, graph_handle)?;
    if graph.epoch != device.epoch {
        return Err(VgpuError::DeviceLost);
    }
    if graph.in_flight != 0 {
        return Err(VgpuError::Busy);
    }
    let (slot, generation) = decode_handle(graph_handle.raw())?;
    let graph_slot = device
        .cloud_work_graphs
        .get_mut(slot)
        .ok_or(VgpuError::InvalidHandle)?;
    if graph_slot.generation != generation || graph_slot.record.is_none() {
        return Err(VgpuError::InvalidHandle);
    }
    graph_slot.record.take();
    Ok(())
}

/// Submit one mediated Cloud frame. The broker owns the complete reservation
/// phase; the native seam is deliberately cold until an authenticated encoder
/// and release-fence path exists.
pub(crate) fn submit_cloud_frame(
    principal: Principal,
    device_handle: DeviceHandle,
    queue_handle: QueueHandle,
    graph_handle: CloudWorkGraphHandle,
    surface_handle: SurfaceHandle,
    simulation_steps: u32,
) -> Result<CloudFrameTelemetry, VgpuError> {
    if simulation_steps > CLOUD_FRAME_MAX_SIMULATION_STEPS {
        return Err(VgpuError::Unsupported);
    }
    let lease = {
        let mut broker = BROKER.lock();
        let device = lookup_device_mut(&mut broker, device_handle, principal)?;
        ensure_live(device)?;
        let required_capabilities = Capabilities::COMPUTE
            .union(Capabilities::RENDER)
            .union(Capabilities::PRESENT);
        if !device.capabilities.contains(required_capabilities) {
            return Err(VgpuError::PermissionDenied);
        }
        let queue = lookup_queue(device, queue_handle)?;
        if queue.class != QueueClass::Render {
            return Err(VgpuError::PermissionDenied);
        }
        if queue.in_flight != 0 {
            return Err(VgpuError::Busy);
        }
        let surface = lookup_surface(device, surface_handle)?;
        if surface.epoch != device.epoch || surface.in_flight != 1 {
            return Err(VgpuError::Busy);
        }
        let graph = lookup_cloud_work_graph(device, graph_handle)?;
        if graph.epoch != device.epoch
            || graph.resources.profile != CLOUD_PROFILE_HELIO_ENGINE_V1
            || graph.current_volume > 1
        {
            return Err(VgpuError::DeviceLost);
        }
        if graph.in_flight != 0 {
            return Err(VgpuError::Busy);
        }
        let buffers = [
            graph.resources.volume_a,
            graph.resources.volume_b,
            graph.resources.sim_params,
            graph.resources.render_params,
        ];
        let graph_resources = graph.resources;
        let buffer_metadata = graph.buffer_metadata.clone();
        let backing_admission = graph.backing_admission;
        let current_volume = graph.current_volume;
        let surface_metadata = CloudSurfaceMetadata {
            window_id: surface.window_id,
            gpu: surface.gpu,
            phys: surface.phys,
            producer_gpu: surface.producer_gpu,
            bytes: surface.bytes,
            width: surface.width,
            height: surface.height,
            pitch: surface.pitch,
        };
        validate_cloud_buffer(device, buffers[0], CloudResourceRole::Volume)?;
        validate_cloud_buffer(device, buffers[1], CloudResourceRole::Volume)?;
        validate_cloud_buffer(device, buffers[2], CloudResourceRole::SimulationParams)?;
        validate_cloud_buffer(device, buffers[3], CloudResourceRole::RenderParams)?;
        if buffers
            .iter()
            .zip(buffer_metadata.iter())
            .any(|(buffer, metadata)| {
                lookup_buffer(device, *buffer)
                    .map_or(true, |record| !cloud_buffer_matches_snapshot(record, metadata, 0))
            })
        {
            return Err(VgpuError::DeviceLost);
        }

        // All checks above are preflight. Only now publish the operation lease.
        lookup_queue_mut(device, queue_handle)?.in_flight = 1;
        lookup_surface_mut(device, surface_handle)?.in_flight = 2;
        lookup_cloud_work_graph_mut(device, graph_handle)?.in_flight = 1;
        for buffer in buffers {
            lookup_buffer_mut(device, buffer)?.in_flight = 1;
        }
        CloudSubmissionLease {
            device_epoch: device.epoch,
            queue: queue_handle,
            graph: graph_handle,
            surface: surface_handle,
            buffers,
            buffer_metadata,
            backing_admission,
            surface_metadata,
            graph_resources,
            current_volume,
            simulation_steps,
        }
    };

    match submit_cloud_frame_native(&lease) {
        Ok(CloudNativeSubmitResult::Submitted(telemetry)) => {
            // A real backend will retire these leases with its release fence.
            // This cold seam cannot produce this branch today.
            if !retire_cloud_submission_lease(principal, device_handle, &lease, &telemetry) {
                return Err(VgpuError::DeviceLost);
            }
            Ok(telemetry)
        }
        Ok(CloudNativeSubmitResult::Ambiguous) => {
            quarantine_cloud_submission_lease(principal, device_handle, &lease);
            Err(VgpuError::DeviceLost)
        }
        Err(error) => {
            // Unsupported is a definite pre-submit failure: no telemetry,
            // timeline point, or ping-pong transition may be fabricated.
            if rollback_cloud_submission_lease(principal, device_handle, &lease) {
                Err(error)
            } else {
                // A reservation mismatch means ownership became ambiguous;
                // never leave a live device with partially held leases.
                quarantine_cloud_submission_lease(principal, device_handle, &lease);
                Err(VgpuError::DeviceLost)
            }
        }
    }
}

fn submit_cloud_frame_native(
    lease: &CloudSubmissionLease,
) -> Result<CloudNativeSubmitResult, VgpuError> {
    let surface = crate::intel::gpgpu::HelioCloudSurfaceDesc {
        producer_gpu: lease.surface_metadata.producer_gpu,
        phys: lease.surface_metadata.phys,
        bytes: lease.surface_metadata.bytes,
        width: lease.surface_metadata.width,
        height: lease.surface_metadata.height,
        pitch_bytes: lease.surface_metadata.pitch,
    };
    let resources = lease.buffer_metadata.each_ref().map(|metadata| metadata.pages.as_ref());
    if crate::intel::gpgpu::helioc_surface_overlaps_backing(surface, resources) {
        return Err(VgpuError::Unsupported);
    }
    let _plan = crate::intel::gpgpu::plan_helioc_frame(
        lease.backing_admission,
        surface,
        lease.simulation_steps,
        lease.current_volume,
    )
    .ok_or(VgpuError::Unsupported)?;
    Err(VgpuError::Unsupported)
}

fn rollback_cloud_submission_lease(
    principal: Principal,
    device_handle: DeviceHandle,
    lease: &CloudSubmissionLease,
) -> bool {
    let mut broker = BROKER.lock();
    let Ok(device) = lookup_device_mut(&mut broker, device_handle, principal) else {
        return false;
    };
    if device.epoch != lease.device_epoch {
        return false;
    }
    let Ok(queue) = lookup_queue(device, lease.queue) else {
        return false;
    };
    let Ok(surface) = lookup_surface(device, lease.surface) else {
        return false;
    };
    let Ok(graph) = lookup_cloud_work_graph(device, lease.graph) else {
        return false;
    };
    if queue.in_flight != 1 || surface.in_flight != 2 || graph.in_flight != 1 {
        return false;
    }
    if lease
        .buffers
        .iter()
        .any(|buffer| lookup_buffer(device, *buffer).map_or(true, |record| record.in_flight != 1))
    {
        return false;
    }
    lookup_queue_mut(device, lease.queue)
        .expect("validated Cloud queue")
        .in_flight = 0;
    lookup_surface_mut(device, lease.surface)
        .expect("validated Cloud surface")
        .in_flight = 1;
    lookup_cloud_work_graph_mut(device, lease.graph)
        .expect("validated Cloud graph")
        .in_flight = 0;
    for buffer in lease.buffers {
        lookup_buffer_mut(device, buffer)
            .expect("validated Cloud buffer")
            .in_flight = 0;
    }
    true
}

fn retire_cloud_submission_lease(
    principal: Principal,
    device_handle: DeviceHandle,
    lease: &CloudSubmissionLease,
    telemetry: &CloudFrameTelemetry,
) -> bool {
    let mut broker = BROKER.lock();
    let Ok(device) = lookup_device_mut(&mut broker, device_handle, principal) else {
        return false;
    };
    if device.epoch != lease.device_epoch {
        return false;
    }
    let Ok(queue) = lookup_queue(device, lease.queue) else {
        return false;
    };
    let Ok(surface) = lookup_surface(device, lease.surface) else {
        return false;
    };
    let Ok(graph) = lookup_cloud_work_graph(device, lease.graph) else {
        return false;
    };
    if telemetry.point.queue != lease.queue
        || telemetry.point.value == 0
        || telemetry.point.value <= queue.timeline.submitted
        || queue.in_flight != 1
        || surface.in_flight != 2
        || graph.in_flight != 1
        || graph.epoch != lease.device_epoch
        || graph.resources != lease.graph_resources
        || graph.current_volume != lease.current_volume
        || surface.epoch != lease.device_epoch
        || surface.window_id != lease.surface_metadata.window_id
        || surface.gpu != lease.surface_metadata.gpu
        || surface.phys != lease.surface_metadata.phys
        || surface.producer_gpu != lease.surface_metadata.producer_gpu
        || surface.bytes != lease.surface_metadata.bytes
        || surface.width != lease.surface_metadata.width
        || surface.height != lease.surface_metadata.height
        || surface.pitch != lease.surface_metadata.pitch
    {
        return false;
    }
    if lease
        .buffers
        .iter()
        .zip(lease.buffer_metadata.iter())
        .any(|(buffer, metadata)| {
            lookup_buffer(device, *buffer)
                .map_or(true, |record| !cloud_buffer_matches_snapshot(record, metadata, 1))
        })
    {
        return false;
    }
    lookup_queue_mut(device, lease.queue)
        .expect("validated Cloud queue")
        .in_flight = 0;
    let queue = lookup_queue_mut(device, lease.queue).expect("validated Cloud queue");
    queue.timeline.submitted = telemetry.point.value;
    queue.timeline.completed = telemetry.point.value;
    queue.timeline.last_physical_serial = telemetry.point.physical_serial;
    lookup_surface_mut(device, lease.surface)
        .expect("validated Cloud surface")
        .in_flight = 3;
    lookup_cloud_work_graph_mut(device, lease.graph)
        .expect("validated Cloud graph")
        .in_flight = 0;
    lookup_cloud_work_graph_mut(device, lease.graph)
        .expect("validated Cloud graph")
        .current_volume = cloud_next_volume(lease.current_volume, lease.simulation_steps);
    for buffer in lease.buffers {
        lookup_buffer_mut(device, buffer)
            .expect("validated Cloud buffer")
            .in_flight = 0;
    }
    true
}

fn quarantine_cloud_submission_lease(
    principal: Principal,
    device_handle: DeviceHandle,
    lease: &CloudSubmissionLease,
) {
    let mut broker = BROKER.lock();
    if let Ok(device) = lookup_device_mut(&mut broker, device_handle, principal) {
        if device.epoch == lease.device_epoch {
            device.lost = true;
        }
    }
}

fn snapshot_cloud_buffer(
    device: &VirtualDevice,
    handle: BufferHandle,
) -> Result<CloudBufferMetadata, VgpuError> {
    let record = lookup_buffer(device, handle)?;
    let pages = match &record.backing {
        BufferBacking::GuestPages { pages, .. } => pages.clone(),
        BufferBacking::Dma { .. } => return Err(VgpuError::PermissionDenied),
    };
    Ok(CloudBufferMetadata {
        gpu: record.gpu,
        bytes: record.bytes,
        usage: record.usage,
        epoch: record.epoch,
        mapping_digest: record.mapping_digest,
        pages: Arc::from(pages.as_slice()),
    })
}

fn cloud_buffer_pages(device: &VirtualDevice, handle: BufferHandle) -> Result<&[u64], VgpuError> {
    let record = lookup_buffer(device, handle)?;
    match &record.backing {
        BufferBacking::GuestPages { pages, .. } => Ok(pages.as_slice()),
        BufferBacking::Dma { .. } => Err(VgpuError::PermissionDenied),
    }
}

const fn cloud_next_volume(current_volume: u8, simulation_steps: u32) -> u8 {
    current_volume ^ (simulation_steps as u8 & 1)
}

fn cloud_buffer_matches_snapshot(
    record: &BufferRecord,
    snapshot: &CloudBufferMetadata,
    expected_in_flight: u32,
) -> bool {
    record.in_flight == expected_in_flight
        && record.gpu == snapshot.gpu
        && record.bytes == snapshot.bytes
        && record.usage == snapshot.usage
        && record.epoch == snapshot.epoch
        && record.mapping_digest == snapshot.mapping_digest
        && matches!(&record.backing, BufferBacking::GuestPages { pages, .. } if pages.as_slice() == snapshot.pages.as_ref())
}

fn validate_cloud_buffer(
    device: &VirtualDevice,
    handle: BufferHandle,
    role: CloudResourceRole,
) -> Result<(), VgpuError> {
    let record = lookup_buffer(device, handle)?;
    if record.epoch != device.epoch || record.in_flight != 0 {
        return Err(VgpuError::Busy);
    }
    if !matches!(&record.backing, BufferBacking::GuestPages { .. }) {
        return Err(VgpuError::PermissionDenied);
    }
    validate_cloud_resource_shape(role, record.bytes, record.usage)
}

fn validate_cloud_resource_shape(
    role: CloudResourceRole,
    mapped_bytes: usize,
    usage: u32,
) -> Result<(), VgpuError> {
    let (logical_bytes, required_usage) = match role {
        CloudResourceRole::Volume => (CLOUD_VOLUME_BYTES, CLOUD_VOLUME_REQUIRED_USAGE),
        CloudResourceRole::SimulationParams => {
            (CLOUD_SIM_PARAMS_BYTES, CLOUD_PARAMS_REQUIRED_USAGE)
        }
        CloudResourceRole::RenderParams => (CLOUD_RENDER_PARAMS_BYTES, CLOUD_PARAMS_REQUIRED_USAGE),
    };
    let expected_mapped = align_up(logical_bytes, PAGE_BYTES).ok_or(VgpuError::OutOfMemory)?;
    if mapped_bytes != expected_mapped {
        return Err(VgpuError::Unsupported);
    }
    if usage & required_usage != required_usage {
        return Err(VgpuError::PermissionDenied);
    }
    Ok(())
}

fn cloud_graph_references_buffer(graph: &CloudWorkGraphRecord, buffer: BufferHandle) -> bool {
    graph.resources.volume_a == buffer
        || graph.resources.volume_b == buffer
        || graph.resources.sim_params == buffer
        || graph.resources.render_params == buffer
}

#[cfg(test)]
mod cloud_resource_contract_tests {
    use super::{
        BUFFER_USAGE_COPY_DST, BUFFER_USAGE_COPY_SRC, BUFFER_USAGE_STORAGE,
        CLOUD_PARAMS_REQUIRED_USAGE, CLOUD_RENDER_PARAMS_BYTES, CLOUD_SIM_PARAMS_BYTES,
        CLOUD_VOLUME_BYTES, CLOUD_VOLUME_REQUIRED_USAGE, CloudResourceRole, PAGE_BYTES, VgpuError,
        align_up, cloud_next_volume, validate_cloud_resource_shape,
    };

    #[test]
    fn exact_authored_resource_shapes_are_admitted() {
        assert_eq!(CLOUD_VOLUME_BYTES, 96 * 48 * 96 * 8);
        assert_eq!(CLOUD_VOLUME_BYTES % PAGE_BYTES, 0);
        assert_eq!(CLOUD_SIM_PARAMS_BYTES, 112);
        assert_eq!(CLOUD_RENDER_PARAMS_BYTES, 272);
        assert_eq!(align_up(CLOUD_SIM_PARAMS_BYTES, PAGE_BYTES), Some(PAGE_BYTES));
        assert_eq!(align_up(CLOUD_RENDER_PARAMS_BYTES, PAGE_BYTES), Some(PAGE_BYTES));
        assert_eq!(
            validate_cloud_resource_shape(
                CloudResourceRole::Volume,
                CLOUD_VOLUME_BYTES,
                CLOUD_VOLUME_REQUIRED_USAGE,
            ),
            Ok(())
        );
        assert_eq!(
            validate_cloud_resource_shape(
                CloudResourceRole::SimulationParams,
                PAGE_BYTES,
                CLOUD_PARAMS_REQUIRED_USAGE,
            ),
            Ok(())
        );
        assert_eq!(
            validate_cloud_resource_shape(
                CloudResourceRole::RenderParams,
                PAGE_BYTES,
                CLOUD_PARAMS_REQUIRED_USAGE,
            ),
            Ok(())
        );
    }

    #[test]
    fn shape_and_usage_mismatches_fail_closed() {
        assert_eq!(
            validate_cloud_resource_shape(
                CloudResourceRole::Volume,
                CLOUD_VOLUME_BYTES - PAGE_BYTES,
                CLOUD_VOLUME_REQUIRED_USAGE,
            ),
            Err(VgpuError::Unsupported)
        );
        assert_eq!(
            validate_cloud_resource_shape(
                CloudResourceRole::Volume,
                CLOUD_VOLUME_BYTES,
                BUFFER_USAGE_STORAGE | BUFFER_USAGE_COPY_DST,
            ),
            Err(VgpuError::PermissionDenied)
        );
        assert_eq!(
            validate_cloud_resource_shape(
                CloudResourceRole::SimulationParams,
                PAGE_BYTES * 2,
                CLOUD_PARAMS_REQUIRED_USAGE,
            ),
            Err(VgpuError::Unsupported)
        );
        assert_eq!(
            validate_cloud_resource_shape(
                CloudResourceRole::RenderParams,
                PAGE_BYTES,
                BUFFER_USAGE_COPY_SRC,
            ),
            Err(VgpuError::PermissionDenied)
        );
    }

    #[test]
    fn simulation_parity_controls_only_ping_pong_selector() {
        assert_eq!(cloud_next_volume(0, 0), 0);
        assert_eq!(cloud_next_volume(0, 1), 1);
        assert_eq!(cloud_next_volume(1, 1), 0);
        assert_eq!(cloud_next_volume(1, 2), 1);
        assert_eq!(cloud_next_volume(0, 2), 0);
        assert_eq!(cloud_next_volume(1, 3), 0);
    }
}

pub(crate) fn import_ui4_surface(
    principal: Principal,
    device_handle: DeviceHandle,
    descriptor: Ui4SurfaceDescriptor,
) -> Result<SurfaceInfo, VgpuError> {
    if descriptor.width == 0
        || descriptor.height == 0
        || descriptor.phys & (PAGE_BYTES as u64 - 1) != 0
        || descriptor.bytes == 0
        || descriptor.bytes & (PAGE_BYTES - 1) != 0
        || descriptor.pitch < descriptor.width.saturating_mul(4)
        || (descriptor.pitch as usize)
            .checked_mul(descriptor.height as usize)
            .is_none_or(|required| required > descriptor.bytes)
    {
        return Err(VgpuError::Unsupported);
    }
    let physical = require_physical()?;
    let mut broker = BROKER.lock();
    if broker
        .devices
        .iter()
        .filter_map(|slot| slot.record.as_ref())
        .flat_map(|device| device.surfaces.iter())
        .filter_map(|slot| slot.record.as_ref())
        .any(|surface| surface.phys == descriptor.phys)
    {
        return Err(VgpuError::Busy);
    }
    let device = lookup_device_mut(&mut broker, device_handle, principal)?;
    ensure_live(device)?;
    if !device.capabilities.contains(Capabilities::PRESENT) {
        return Err(VgpuError::PermissionDenied);
    }
    let resource_count = device
        .buffers
        .iter()
        .filter(|slot| slot.record.is_some())
        .count()
        .saturating_add(
            device
                .surfaces
                .iter()
                .filter(|slot| slot.record.is_some())
                .count(),
        );
    if resource_count >= device.quota.buffers
        || device.memory_used.saturating_add(descriptor.bytes) > device.quota.memory_bytes
    {
        return Err(VgpuError::QuotaExceeded);
    }
    let gpu = align_up_u64(device.next_gpu_va, PAGE_BYTES as u64).ok_or(VgpuError::OutOfMemory)?;
    let next = gpu
        .checked_add(descriptor.bytes as u64)
        .ok_or(VgpuError::OutOfMemory)?;
    if next > CLIENT_GPU_VA_LIMIT {
        return Err(VgpuError::QuotaExceeded);
    }
    let vm = match device.gpuvm {
        GpuVmBinding::Owned(vm) => vm,
        GpuVmBinding::Borrowed { .. } => return Err(VgpuError::Unsupported),
    };
    physical.map_gpuvm(vm, gpu, descriptor.phys, descriptor.bytes)?;
    device.next_gpu_va = next;
    device.memory_used = device.memory_used.saturating_add(descriptor.bytes);
    let handle = insert_surface(
        device,
        SurfaceRecord {
            window_id: descriptor.window_id,
            phys: descriptor.phys,
            bytes: descriptor.bytes,
            gpu,
            producer_gpu: descriptor.producer_gpu,
            width: descriptor.width,
            height: descriptor.height,
            pitch: descriptor.pitch,
            epoch: device.epoch,
            in_flight: 1,
        },
    );
    Ok(SurfaceInfo {
        handle,
        bytes: descriptor.bytes,
        width: descriptor.width,
        height: descriptor.height,
        pitch: descriptor.pitch,
    })
}

pub(crate) fn create_shader_module(
    principal: Principal,
    device_handle: DeviceHandle,
    package_digest: u64,
) -> Result<ShaderModuleHandle, VgpuError> {
    if package_digest != SHADER_PACKAGE_CLIP_POSITION3_RGBA_FNV1A64
        && package_digest != SHADER_PACKAGE_CLIP_POSITION3_IMMEDIATE_RGBA_FNV1A64
        && package_digest != SHADER_PACKAGE_CLIP_POSITION3_UV_TEXTURE_FNV1A64
        && package_digest != SHADER_PACKAGE_CLIP_POSITION3_UV_TEXEL_LOAD_FNV1A64
    {
        return Err(VgpuError::Unsupported);
    }
    let mut broker = BROKER.lock();
    let device = lookup_device_mut(&mut broker, device_handle, principal)?;
    ensure_live(device)?;
    if !device.capabilities.contains(Capabilities::RENDER) {
        return Err(VgpuError::PermissionDenied);
    }
    if device
        .shader_modules
        .iter()
        .filter(|slot| slot.record.is_some())
        .count()
        .saturating_add(
            device
                .render_pipelines
                .iter()
                .filter(|slot| slot.record.is_some())
                .count(),
        )
        >= 64
    {
        return Err(VgpuError::QuotaExceeded);
    }
    let epoch = device.epoch;
    Ok(insert_shader_module(
        device,
        ShaderModuleRecord {
            package_digest,
            epoch,
        },
    ))
}

pub(crate) fn destroy_shader_module(
    principal: Principal,
    device_handle: DeviceHandle,
    shader_handle: ShaderModuleHandle,
) -> Result<(), VgpuError> {
    let mut broker = BROKER.lock();
    let device = lookup_device_mut(&mut broker, device_handle, principal)?;
    ensure_live(device)?;
    let (slot, generation) = decode_handle(shader_handle.raw())?;
    let entry = device
        .shader_modules
        .get_mut(slot)
        .ok_or(VgpuError::InvalidHandle)?;
    if entry.generation != generation || entry.record.take().is_none() {
        return Err(VgpuError::InvalidHandle);
    }
    Ok(())
}

pub(crate) fn create_render_pipeline(
    principal: Principal,
    device_handle: DeviceHandle,
    shader_handle: ShaderModuleHandle,
    vertex_stride: u32,
    position_offset: u32,
) -> Result<RenderPipelineHandle, VgpuError> {
    if vertex_stride < 12
        || vertex_stride > 256
        || !vertex_stride.is_multiple_of(4)
        || !position_offset.is_multiple_of(4)
        || position_offset.saturating_add(12) > vertex_stride
    {
        return Err(VgpuError::Unsupported);
    }
    let mut broker = BROKER.lock();
    let device = lookup_device_mut(&mut broker, device_handle, principal)?;
    ensure_live(device)?;
    if device
        .render_pipelines
        .iter()
        .filter(|slot| slot.record.is_some())
        .count()
        >= 64
    {
        return Err(VgpuError::QuotaExceeded);
    }
    let shader = lookup_shader_module(device, shader_handle)?;
    if shader.epoch != device.epoch {
        return Err(VgpuError::InvalidHandle);
    }
    if matches!(
        shader.package_digest,
        SHADER_PACKAGE_CLIP_POSITION3_UV_TEXTURE_FNV1A64
            | SHADER_PACKAGE_CLIP_POSITION3_UV_TEXEL_LOAD_FNV1A64
    ) && (vertex_stride != 20 || position_offset != 0)
    {
        return Err(VgpuError::Unsupported);
    }
    let record = RenderPipelineRecord {
        package_digest: shader.package_digest,
        vertex_stride,
        position_offset,
        epoch: device.epoch,
    };
    Ok(insert_render_pipeline(device, record))
}

pub(crate) fn destroy_render_pipeline(
    principal: Principal,
    device_handle: DeviceHandle,
    pipeline_handle: RenderPipelineHandle,
) -> Result<(), VgpuError> {
    let mut broker = BROKER.lock();
    let device = lookup_device_mut(&mut broker, device_handle, principal)?;
    ensure_live(device)?;
    let (slot, generation) = decode_handle(pipeline_handle.raw())?;
    let entry = device
        .render_pipelines
        .get_mut(slot)
        .ok_or(VgpuError::InvalidHandle)?;
    if entry.generation != generation || entry.record.take().is_none() {
        return Err(VgpuError::InvalidHandle);
    }
    Ok(())
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct Ui4IndexedDrawDescriptor {
    pub(crate) surface: SurfaceHandle,
    pub(crate) pipeline: RenderPipelineHandle,
    pub(crate) vertex_buffer: BufferHandle,
    pub(crate) index_buffer: BufferHandle,
    pub(crate) vertex_offset: usize,
    pub(crate) index_offset: usize,
    pub(crate) index_count: u32,
    pub(crate) first_index: u32,
    pub(crate) base_vertex: i32,
    pub(crate) clear_rgba8_srgb: u32,
    pub(crate) sampled_texture: BufferHandle,
    pub(crate) texture_width: u32,
    pub(crate) texture_height: u32,
    pub(crate) texture_pitch: u32,
    pub(crate) sampler_flags: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct Ui4IndexedBatchDrawDescriptor {
    pub(crate) index_count: u32,
    pub(crate) first_index: u32,
    pub(crate) base_vertex: i32,
    pub(crate) rgba8_srgb: u32,
}

pub(crate) struct Ui4IndexedBatchDescriptor {
    pub(crate) surface: SurfaceHandle,
    pub(crate) pipeline: RenderPipelineHandle,
    pub(crate) vertex_buffer: BufferHandle,
    pub(crate) index_buffer: BufferHandle,
    pub(crate) vertex_offset: usize,
    pub(crate) index_offset: usize,
    pub(crate) clear_rgba8_srgb: u32,
    pub(crate) draws: Vec<Ui4IndexedBatchDrawDescriptor>,
}

pub(crate) struct Ui4SurfaceIndexedCompletion {
    pub(crate) window_id: u32,
    pub(crate) surface: SurfaceInfo,
    pub(crate) release: crate::intel::render::ResidentSceneReleaseFence,
    pub(crate) point: TimelinePoint,
}

enum IndexedVertexPayload {
    Position(Vec<[f32; 3]>),
    PositionUv(Vec<[f32; 5]>),
}

struct IndexedTexturePayload {
    bytes: Vec<u8>,
    width: u32,
    height: u32,
    pitch: u32,
    sampler_flags: u32,
}

/// Resolve one bounded WGPU indexed draw into the existing authenticated
/// Render frontier. The broker understands only byte layouts, opaque handles,
/// and the admitted shader-package interface; Helio meshes and voxel meaning
/// remain entirely above this boundary.
pub(crate) fn submit_ui4_indexed_draw(
    principal: Principal,
    device_handle: DeviceHandle,
    queue_handle: QueueHandle,
    draw: Ui4IndexedDrawDescriptor,
) -> Result<Ui4SurfaceIndexedCompletion, VgpuError> {
    if draw.index_count == 0 || draw.base_vertex != 0 {
        return Err(VgpuError::Unsupported);
    }
    let (
        window_id,
        phys,
        producer_gpu,
        bytes,
        width,
        height,
        pitch,
        package_digest,
        vertices,
        indices,
        texture,
    ) = {
        let mut broker = BROKER.lock();
        let device = lookup_device_mut(&mut broker, device_handle, principal)?;
        ensure_live(device)?;
        if !device.capabilities.contains(Capabilities::RENDER)
            || !device.capabilities.contains(Capabilities::PRESENT)
        {
            return Err(VgpuError::PermissionDenied);
        }
        let pipeline = lookup_render_pipeline(device, draw.pipeline)?;
        if pipeline.epoch != device.epoch
            || !matches!(
                pipeline.package_digest,
                SHADER_PACKAGE_CLIP_POSITION3_RGBA_FNV1A64
                    | SHADER_PACKAGE_CLIP_POSITION3_UV_TEXTURE_FNV1A64
                    | SHADER_PACKAGE_CLIP_POSITION3_UV_TEXEL_LOAD_FNV1A64
            )
        {
            return Err(VgpuError::InvalidHandle);
        }
        let package_digest = pipeline.package_digest;
        let textured = matches!(
            package_digest,
            SHADER_PACKAGE_CLIP_POSITION3_UV_TEXTURE_FNV1A64
                | SHADER_PACKAGE_CLIP_POSITION3_UV_TEXEL_LOAD_FNV1A64
        );
        if textured
            != (draw.sampled_texture.raw() != 0
                && draw.texture_width != 0
                && draw.texture_height != 0
                && draw.texture_pitch != 0)
        {
            return Err(VgpuError::Unsupported);
        }
        let vertex_stride = pipeline.vertex_stride as usize;
        let position_offset = pipeline.position_offset as usize;
        {
            let queue = lookup_queue_mut(device, queue_handle)?;
            if queue.class != QueueClass::Render || queue.in_flight != 0 {
                return Err(VgpuError::Busy);
            }
            queue.in_flight = 1;
        }
        let surface_epoch = device.epoch;
        let (window_id, phys, producer_gpu, bytes, width, height, pitch) = {
            let surface = lookup_surface_mut(device, draw.surface)?;
            if surface.epoch != surface_epoch || surface.in_flight != 1 {
                lookup_queue_mut(device, queue_handle)?.in_flight = 0;
                return Err(VgpuError::Busy);
            }
            surface.in_flight = 2;
            (
                surface.window_id,
                surface.phys,
                surface.producer_gpu,
                surface.bytes,
                surface.width,
                surface.height,
                surface.pitch,
            )
        };
        let copied = (|| {
            let index_record = lookup_buffer(device, draw.index_buffer)?;
            if index_record.usage & BUFFER_USAGE_INDEX == 0 {
                return Err(VgpuError::PermissionDenied);
            }
            let first_index_bytes = (draw.first_index as usize)
                .checked_mul(4)
                .ok_or(VgpuError::Unsupported)?;
            let index_start = draw
                .index_offset
                .checked_add(first_index_bytes)
                .ok_or(VgpuError::Unsupported)?;
            let index_bytes = (draw.index_count as usize)
                .checked_mul(4)
                .ok_or(VgpuError::Unsupported)?;
            let index_end = index_start
                .checked_add(index_bytes)
                .ok_or(VgpuError::Unsupported)?;
            if index_end > index_record.bytes {
                return Err(VgpuError::Unsupported);
            }
            let index_virt = match index_record.backing {
                BufferBacking::Dma { virt, .. } => virt,
                BufferBacking::GuestPages { .. } => return Err(VgpuError::Unsupported),
            };
            crate::intel::dma_flush(unsafe { index_virt.add(index_start) }, index_bytes);
            let raw_indices =
                unsafe { core::slice::from_raw_parts(index_virt.add(index_start), index_bytes) };
            let mut indices = Vec::with_capacity(draw.index_count as usize);
            for raw in raw_indices.chunks_exact(4) {
                indices.push(u32::from_le_bytes(raw.try_into().expect("four-byte index")));
            }
            let vertex_count = indices
                .iter()
                .copied()
                .max()
                .ok_or(VgpuError::Unsupported)? as usize
                + 1;
            let vertex_record = lookup_buffer(device, draw.vertex_buffer)?;
            if vertex_record.usage & BUFFER_USAGE_VERTEX == 0 {
                return Err(VgpuError::PermissionDenied);
            }
            let vertex_end = draw
                .vertex_offset
                .checked_add(
                    vertex_count
                        .checked_mul(vertex_stride)
                        .ok_or(VgpuError::Unsupported)?,
                )
                .ok_or(VgpuError::Unsupported)?;
            if vertex_end > vertex_record.bytes {
                return Err(VgpuError::Unsupported);
            }
            let vertex_virt = match vertex_record.backing {
                BufferBacking::Dma { virt, .. } => virt,
                BufferBacking::GuestPages { .. } => return Err(VgpuError::Unsupported),
            };
            crate::intel::dma_flush(
                unsafe { vertex_virt.add(draw.vertex_offset) },
                vertex_end - draw.vertex_offset,
            );
            let vertices = if textured {
                let mut vertices = Vec::with_capacity(vertex_count);
                for vertex in 0..vertex_count {
                    let start = draw.vertex_offset + vertex * vertex_stride + position_offset;
                    let raw = unsafe { core::slice::from_raw_parts(vertex_virt.add(start), 20) };
                    vertices.push([
                        f32::from_le_bytes(raw[0..4].try_into().unwrap()),
                        f32::from_le_bytes(raw[4..8].try_into().unwrap()),
                        f32::from_le_bytes(raw[8..12].try_into().unwrap()),
                        f32::from_le_bytes(raw[12..16].try_into().unwrap()),
                        f32::from_le_bytes(raw[16..20].try_into().unwrap()),
                    ]);
                }
                IndexedVertexPayload::PositionUv(vertices)
            } else {
                let mut vertices = Vec::with_capacity(vertex_count);
                for vertex in 0..vertex_count {
                    let start = draw.vertex_offset + vertex * vertex_stride + position_offset;
                    let raw = unsafe { core::slice::from_raw_parts(vertex_virt.add(start), 12) };
                    vertices.push([
                        f32::from_le_bytes(raw[0..4].try_into().unwrap()),
                        f32::from_le_bytes(raw[4..8].try_into().unwrap()),
                        f32::from_le_bytes(raw[8..12].try_into().unwrap()),
                    ]);
                }
                IndexedVertexPayload::Position(vertices)
            };
            let texture = if textured {
                let texture_bytes = usize::try_from(
                    u64::from(draw.texture_pitch)
                        .checked_mul(u64::from(draw.texture_height))
                        .ok_or(VgpuError::Unsupported)?,
                )
                .map_err(|_| VgpuError::Unsupported)?;
                if draw.texture_pitch < draw.texture_width.saturating_mul(4)
                    || !draw.texture_pitch.is_multiple_of(4)
                    || draw.sampler_flags != (SAMPLER_ADDRESS_U_REPEAT | SAMPLER_ADDRESS_V_REPEAT)
                {
                    return Err(VgpuError::Unsupported);
                }
                let texture_record = lookup_buffer(device, draw.sampled_texture)?;
                if texture_record.usage & BUFFER_USAGE_STORAGE == 0
                    || texture_bytes > texture_record.bytes
                {
                    return Err(VgpuError::PermissionDenied);
                }
                let texture_virt = match texture_record.backing {
                    BufferBacking::Dma { virt, .. } => virt,
                    BufferBacking::GuestPages { .. } => return Err(VgpuError::Unsupported),
                };
                crate::intel::dma_flush(texture_virt, texture_bytes);
                Some(IndexedTexturePayload {
                    bytes: unsafe {
                        core::slice::from_raw_parts(texture_virt, texture_bytes).to_vec()
                    },
                    width: draw.texture_width,
                    height: draw.texture_height,
                    pitch: draw.texture_pitch,
                    sampler_flags: draw.sampler_flags,
                })
            } else {
                None
            };
            Ok((vertices, indices, texture))
        })();
        let (vertices, mut indices, texture) = match copied {
            Ok(copied) => copied,
            Err(error) => {
                lookup_surface_mut(device, draw.surface)?.in_flight = 1;
                lookup_queue_mut(device, queue_handle)?.in_flight = 0;
                return Err(error);
            }
        };
        // The authenticated package exposes cull-none WebGPU semantics while
        // the current resident fixed-function packet accepts one canonical
        // winding. Canonicalize each projected triangle without changing its
        // topology; depth still resolves the visible voxel faces.
        for triangle in indices.chunks_exact_mut(3) {
            let position = |index: u32| match &vertices {
                IndexedVertexPayload::Position(vertices) => vertices[index as usize],
                IndexedVertexPayload::PositionUv(vertices) => {
                    let vertex = vertices[index as usize];
                    [vertex[0], vertex[1], vertex[2]]
                }
            };
            let a = position(triangle[0]);
            let b = position(triangle[1]);
            let c = position(triangle[2]);
            let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
            if area < 0.0 {
                triangle.swap(1, 2);
            }
        }
        (
            window_id,
            phys,
            producer_gpu,
            bytes,
            width,
            height,
            pitch,
            package_digest,
            vertices,
            indices,
            texture,
        )
    };

    let destination = crate::intel::gpgpu::GpgpuRgba8Surface::new(
        phys,
        producer_gpu,
        bytes,
        width,
        height,
        pitch,
    )
    .ok_or(VgpuError::Unsupported)?;
    let mesh = match &vertices {
        IndexedVertexPayload::Position(vertices) => {
            crate::intel::render::create_resident_triangle_mesh(vertices, &indices)
        }
        IndexedVertexPayload::PositionUv(vertices) => {
            crate::intel::render::create_resident_textured_triangle_mesh(vertices, &indices)
        }
    };
    let mesh = match mesh {
        Ok(mesh) => mesh,
        Err(_) => {
            rollback_indexed_submission_lease(principal, device_handle, queue_handle, draw.surface);
            return Err(VgpuError::OutOfMemory);
        }
    };
    let resident_texture = match texture {
        Some(texture) => match crate::intel::render::create_resident_sampled_rgba8_texture(
            texture.width,
            texture.height,
            texture.pitch,
            texture.sampler_flags,
            &texture.bytes,
        ) {
            Ok(texture) => Some(texture),
            Err(_) => {
                let _ = crate::intel::render::release_resident_triangle_mesh(&mesh);
                rollback_indexed_submission_lease(
                    principal,
                    device_handle,
                    queue_handle,
                    draw.surface,
                );
                return Err(VgpuError::OutOfMemory);
            }
        },
        None => None,
    };
    let scene_draw = crate::intel::render::ResidentSceneDraw {
        mesh: &mesh,
        rgba: SHADER_PACKAGE_CLIP_POSITION3_RGBA_COLOR.to_le_bytes(),
        sampled_texture: resident_texture.as_ref(),
        fragment_contract: match package_digest {
            SHADER_PACKAGE_CLIP_POSITION3_RGBA_FNV1A64 => {
                crate::intel::render::ResidentSceneFragmentContract::ConstantRgba
            }
            SHADER_PACKAGE_CLIP_POSITION3_UV_TEXTURE_FNV1A64 => {
                crate::intel::render::ResidentSceneFragmentContract::FilteredSample
            }
            SHADER_PACKAGE_CLIP_POSITION3_UV_TEXEL_LOAD_FNV1A64 => {
                crate::intel::render::ResidentSceneFragmentContract::FixedTexelLoadProbe
            }
            _ => unreachable!("shader package was validated before resident draw creation"),
        },
        viewport_translation_px: [0.0, 0.0],
    };
    let rendered = crate::intel::render::render_resident_triangle_scene_frame_premultiplied_with_opaque_depth_direct_to_surface(
        core::slice::from_ref(&scene_draw),
        Some(draw.clear_rgba8_srgb.to_le_bytes()),
        destination,
        false,
    );
    let released_resources = if rendered.is_ok() || matches!(rendered, Err("render-busy")) {
        let texture_released = resident_texture
            .as_ref()
            .is_none_or(crate::intel::render::release_resident_sampled_texture);
        crate::intel::render::release_resident_triangle_mesh(&mesh) && texture_released
    } else {
        // Physical completion is ambiguous. Keep the resident geometry pinned
        // with the lost device instead of recycling storage still reachable by
        // Render0.
        false
    };
    if matches!(rendered, Err("render-busy")) && released_resources {
        rollback_indexed_submission_lease(principal, device_handle, queue_handle, draw.surface);
        return Err(VgpuError::Busy);
    }
    let release = rendered
        .ok()
        .and_then(|result| {
            (result.completed_draws == 1
                && result.requested_draws == 1
                && !result.present_copy_performed)
                .then_some(result.release_fence)
                .flatten()
        })
        .filter(|release| release.matches(phys, bytes));
    let Some(release) = release.filter(|_| released_resources) else {
        let mut broker = BROKER.lock();
        if let Ok(device) = lookup_device_mut(&mut broker, device_handle, principal) {
            device.lost = true;
            if let Ok(queue) = lookup_queue_mut(device, queue_handle) {
                queue.in_flight = 0;
                queue.timeline.failures = queue.timeline.failures.saturating_add(1);
            }
            if let Ok(surface) = lookup_surface_mut(device, draw.surface) {
                surface.in_flight = 3;
            }
        }
        return Err(VgpuError::DeviceLost);
    };

    let physical = require_physical()?;
    let mut broker = BROKER.lock();
    let device = lookup_device_mut(&mut broker, device_handle, principal)?;
    ensure_live(device)?;
    let vm = match device.gpuvm {
        GpuVmBinding::Owned(vm) => vm,
        GpuVmBinding::Borrowed { .. } => return Err(VgpuError::Unsupported),
    };
    let (slot, generation) = decode_handle(draw.surface.raw())?;
    let surface_slot = device
        .surfaces
        .get_mut(slot)
        .ok_or(VgpuError::InvalidHandle)?;
    if surface_slot.generation != generation
        || surface_slot
            .record
            .as_ref()
            .is_none_or(|record| record.in_flight != 2)
    {
        return Err(VgpuError::DeviceLost);
    }
    let guest_gpu = surface_slot.record.as_ref().expect("validated surface").gpu;
    physical.unmap_gpuvm(vm, guest_gpu, bytes)?;
    let record = surface_slot.record.take().expect("validated surface");
    device.memory_used = device.memory_used.saturating_sub(record.bytes);
    let queue = lookup_queue_mut(device, queue_handle)?;
    queue.in_flight = 0;
    queue.timeline.submitted = queue.timeline.submitted.wrapping_add(1).max(1);
    queue.timeline.completed = queue.timeline.submitted;
    queue.timeline.last_physical_serial = release.sequence();
    let point = TimelinePoint {
        queue: queue_handle,
        value: queue.timeline.submitted,
        physical_serial: release.sequence(),
        physical_publish_sequence: release.sequence(),
    };
    crate::log_info!(target: "vgpu";
        "vgpu: indexed UI4 draw retired principal={:?} shader_package=fnv1a64:{:016X} pipeline={} vertex_buffer={} index_buffer={} indices={} target={}x{} timeline={} render_release={} path=opaque-wgpu-objects->resident-render0->ui4\n",
        principal,
        package_digest,
        draw.pipeline.raw(),
        draw.vertex_buffer.raw(),
        draw.index_buffer.raw(),
        draw.index_count,
        width,
        height,
        point.value,
        release.sequence(),
    );
    Ok(Ui4SurfaceIndexedCompletion {
        window_id,
        surface: SurfaceInfo {
            handle: draw.surface,
            bytes,
            width,
            height,
            pitch,
        },
        release,
        point,
    })
}

/// Resolve repeated WGPU `draw_indexed` calls from one render pass into the
/// resident renderer's already-batched scene path. Material meaning remains in
/// the client; the broker sees only authenticated immediate RGBA bytes and
/// bounded index ranges sharing one ordinary vertex/index binding.
pub(crate) fn submit_ui4_indexed_batch(
    principal: Principal,
    device_handle: DeviceHandle,
    queue_handle: QueueHandle,
    batch: Ui4IndexedBatchDescriptor,
) -> Result<Ui4SurfaceIndexedCompletion, VgpuError> {
    if batch.draws.is_empty()
        || batch.draws.len() > MAX_INDEXED_BATCH_DRAWS
        || batch.draws.iter().any(|draw| {
            draw.index_count == 0 || !draw.index_count.is_multiple_of(3) || draw.base_vertex < 0
        })
    {
        return Err(VgpuError::Unsupported);
    }
    let (window_id, phys, producer_gpu, bytes, width, height, pitch, vertices, mut indexed) = {
        let mut broker = BROKER.lock();
        let device = lookup_device_mut(&mut broker, device_handle, principal)?;
        ensure_live(device)?;
        if !device.capabilities.contains(Capabilities::RENDER)
            || !device.capabilities.contains(Capabilities::PRESENT)
        {
            return Err(VgpuError::PermissionDenied);
        }
        let pipeline = lookup_render_pipeline(device, batch.pipeline)?;
        if pipeline.epoch != device.epoch
            || pipeline.package_digest != SHADER_PACKAGE_CLIP_POSITION3_IMMEDIATE_RGBA_FNV1A64
        {
            return Err(VgpuError::InvalidHandle);
        }
        let vertex_stride = pipeline.vertex_stride as usize;
        let position_offset = pipeline.position_offset as usize;
        {
            let queue = lookup_queue_mut(device, queue_handle)?;
            if queue.class != QueueClass::Render || queue.in_flight != 0 {
                return Err(VgpuError::Busy);
            }
            queue.in_flight = 1;
        }
        let surface_epoch = device.epoch;
        let (window_id, phys, producer_gpu, bytes, width, height, pitch) = {
            let surface = lookup_surface_mut(device, batch.surface)?;
            if surface.epoch != surface_epoch || surface.in_flight != 1 {
                lookup_queue_mut(device, queue_handle)?.in_flight = 0;
                return Err(VgpuError::Busy);
            }
            surface.in_flight = 2;
            (
                surface.window_id,
                surface.phys,
                surface.producer_gpu,
                surface.bytes,
                surface.width,
                surface.height,
                surface.pitch,
            )
        };
        let copied = (|| {
            let index_record = lookup_buffer(device, batch.index_buffer)?;
            if index_record.usage & BUFFER_USAGE_INDEX == 0 {
                return Err(VgpuError::PermissionDenied);
            }
            let index_virt = match index_record.backing {
                BufferBacking::Dma { virt, .. } => virt,
                BufferBacking::GuestPages { .. } => return Err(VgpuError::Unsupported),
            };
            let mut indexed = Vec::with_capacity(batch.draws.len());
            let mut vertex_count = 0usize;
            for draw in &batch.draws {
                let first_index_bytes = (draw.first_index as usize)
                    .checked_mul(4)
                    .ok_or(VgpuError::Unsupported)?;
                let index_start = batch
                    .index_offset
                    .checked_add(first_index_bytes)
                    .ok_or(VgpuError::Unsupported)?;
                let index_bytes = (draw.index_count as usize)
                    .checked_mul(4)
                    .ok_or(VgpuError::Unsupported)?;
                let index_end = index_start
                    .checked_add(index_bytes)
                    .ok_or(VgpuError::Unsupported)?;
                if index_end > index_record.bytes {
                    return Err(VgpuError::Unsupported);
                }
                crate::intel::dma_flush(unsafe { index_virt.add(index_start) }, index_bytes);
                let raw_indices = unsafe {
                    core::slice::from_raw_parts(index_virt.add(index_start), index_bytes)
                };
                let base_vertex = draw.base_vertex as usize;
                let mut indices = Vec::with_capacity(draw.index_count as usize);
                for raw in raw_indices.chunks_exact(4) {
                    let index = u32::from_le_bytes(raw.try_into().expect("four-byte index"));
                    vertex_count = vertex_count.max(
                        base_vertex
                            .checked_add(index as usize)
                            .and_then(|index| index.checked_add(1))
                            .ok_or(VgpuError::Unsupported)?,
                    );
                    indices.push(index);
                }
                indexed.push((indices, draw.rgba8_srgb, base_vertex));
            }
            if vertex_count == 0 {
                return Err(VgpuError::Unsupported);
            }
            let vertex_record = lookup_buffer(device, batch.vertex_buffer)?;
            if vertex_record.usage & BUFFER_USAGE_VERTEX == 0 {
                return Err(VgpuError::PermissionDenied);
            }
            let vertex_end = batch
                .vertex_offset
                .checked_add(
                    vertex_count
                        .checked_mul(vertex_stride)
                        .ok_or(VgpuError::Unsupported)?,
                )
                .ok_or(VgpuError::Unsupported)?;
            if vertex_end > vertex_record.bytes {
                return Err(VgpuError::Unsupported);
            }
            let vertex_virt = match vertex_record.backing {
                BufferBacking::Dma { virt, .. } => virt,
                BufferBacking::GuestPages { .. } => return Err(VgpuError::Unsupported),
            };
            crate::intel::dma_flush(
                unsafe { vertex_virt.add(batch.vertex_offset) },
                vertex_end - batch.vertex_offset,
            );
            let mut vertices = Vec::with_capacity(vertex_count);
            for vertex in 0..vertex_count {
                let start = batch.vertex_offset + vertex * vertex_stride + position_offset;
                let raw = unsafe { core::slice::from_raw_parts(vertex_virt.add(start), 12) };
                vertices.push([
                    f32::from_le_bytes(raw[0..4].try_into().unwrap()),
                    f32::from_le_bytes(raw[4..8].try_into().unwrap()),
                    f32::from_le_bytes(raw[8..12].try_into().unwrap()),
                ]);
            }
            Ok((vertices, indexed))
        })();
        let (vertices, indexed) = match copied {
            Ok(copied) => copied,
            Err(error) => {
                lookup_surface_mut(device, batch.surface)?.in_flight = 1;
                lookup_queue_mut(device, queue_handle)?.in_flight = 0;
                return Err(error);
            }
        };
        (window_id, phys, producer_gpu, bytes, width, height, pitch, vertices, indexed)
    };

    for (indices, _, base_vertex) in &mut indexed {
        for triangle in indices.chunks_exact_mut(3) {
            let a = vertices[*base_vertex + triangle[0] as usize];
            let b = vertices[*base_vertex + triangle[1] as usize];
            let c = vertices[*base_vertex + triangle[2] as usize];
            let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
            if area < 0.0 {
                triangle.swap(1, 2);
            }
        }
    }
    let destination = match crate::intel::gpgpu::GpgpuRgba8Surface::new(
        phys,
        producer_gpu,
        bytes,
        width,
        height,
        pitch,
    ) {
        Some(destination) => destination,
        None => {
            rollback_indexed_submission_lease(
                principal,
                device_handle,
                queue_handle,
                batch.surface,
            );
            return Err(VgpuError::Unsupported);
        }
    };
    let mut meshes = Vec::with_capacity(indexed.len());
    for (indices, _, base_vertex) in &indexed {
        let local_vertex_count = indices
            .iter()
            .copied()
            .max()
            .map(|index| index as usize + 1)
            .expect("non-empty indexed batch section");
        let section_vertices = base_vertex
            .checked_add(local_vertex_count)
            .and_then(|section_end| vertices.get(*base_vertex..section_end));
        let Some(section_vertices) = section_vertices else {
            for mesh in &meshes {
                let _ = crate::intel::render::release_resident_triangle_mesh(mesh);
            }
            rollback_indexed_submission_lease(
                principal,
                device_handle,
                queue_handle,
                batch.surface,
            );
            return Err(VgpuError::Unsupported);
        };
        match crate::intel::render::create_resident_triangle_mesh(section_vertices, indices) {
            Ok(mesh) => meshes.push(mesh),
            Err(_) => {
                for mesh in &meshes {
                    let _ = crate::intel::render::release_resident_triangle_mesh(mesh);
                }
                rollback_indexed_submission_lease(
                    principal,
                    device_handle,
                    queue_handle,
                    batch.surface,
                );
                return Err(VgpuError::OutOfMemory);
            }
        }
    }
    let scene_draws: Vec<_> = meshes
        .iter()
        .zip(indexed.iter())
        .map(|(mesh, (_, rgba, _))| crate::intel::render::ResidentSceneDraw {
            mesh,
            rgba: rgba.to_le_bytes(),
            sampled_texture: None,
            fragment_contract: crate::intel::render::ResidentSceneFragmentContract::ConstantRgba,
            viewport_translation_px: [0.0, 0.0],
        })
        .collect();
    let rendered = crate::intel::render::render_resident_triangle_scene_frame_premultiplied_with_opaque_depth_direct_to_surface(
        &scene_draws,
        Some(batch.clear_rgba8_srgb.to_le_bytes()),
        destination,
        false,
    );
    let mut released_resources = true;
    if rendered.is_ok() || matches!(rendered, Err("render-busy")) {
        for mesh in &meshes {
            released_resources &= crate::intel::render::release_resident_triangle_mesh(mesh);
        }
    } else {
        released_resources = false;
    }
    if matches!(rendered, Err("render-busy")) && released_resources {
        rollback_indexed_submission_lease(principal, device_handle, queue_handle, batch.surface);
        return Err(VgpuError::Busy);
    }
    let expected_draws = batch.draws.len();
    let release = rendered
        .ok()
        .and_then(|result| {
            (result.completed_draws == expected_draws
                && result.requested_draws == expected_draws
                && !result.present_copy_performed)
                .then_some(result.release_fence)
                .flatten()
        })
        .filter(|release| release.matches(phys, bytes));
    let Some(release) = release.filter(|_| released_resources) else {
        let mut broker = BROKER.lock();
        if let Ok(device) = lookup_device_mut(&mut broker, device_handle, principal) {
            device.lost = true;
            if let Ok(queue) = lookup_queue_mut(device, queue_handle) {
                queue.in_flight = 0;
                queue.timeline.failures = queue.timeline.failures.saturating_add(1);
            }
            if let Ok(surface) = lookup_surface_mut(device, batch.surface) {
                surface.in_flight = 3;
            }
        }
        return Err(VgpuError::DeviceLost);
    };

    let physical = require_physical()?;
    let mut broker = BROKER.lock();
    let device = lookup_device_mut(&mut broker, device_handle, principal)?;
    ensure_live(device)?;
    let vm = match device.gpuvm {
        GpuVmBinding::Owned(vm) => vm,
        GpuVmBinding::Borrowed { .. } => return Err(VgpuError::Unsupported),
    };
    let (slot, generation) = decode_handle(batch.surface.raw())?;
    let surface_slot = device
        .surfaces
        .get_mut(slot)
        .ok_or(VgpuError::InvalidHandle)?;
    if surface_slot.generation != generation
        || surface_slot
            .record
            .as_ref()
            .is_none_or(|record| record.in_flight != 2)
    {
        return Err(VgpuError::DeviceLost);
    }
    let guest_gpu = surface_slot.record.as_ref().expect("validated surface").gpu;
    physical.unmap_gpuvm(vm, guest_gpu, bytes)?;
    let record = surface_slot.record.take().expect("validated surface");
    device.memory_used = device.memory_used.saturating_sub(record.bytes);
    let queue = lookup_queue_mut(device, queue_handle)?;
    queue.in_flight = 0;
    queue.timeline.submitted = queue.timeline.submitted.wrapping_add(1).max(1);
    queue.timeline.completed = queue.timeline.submitted;
    queue.timeline.last_physical_serial = release.sequence();
    let point = TimelinePoint {
        queue: queue_handle,
        value: queue.timeline.submitted,
        physical_serial: release.sequence(),
        physical_publish_sequence: release.sequence(),
    };
    crate::log_info!(target: "vgpu";
        "vgpu: indexed UI4 batch retired principal={:?} shader_package=fnv1a64:{:016X} pipeline={} vertex_buffer={} index_buffer={} draws={} indices={} target={}x{} timeline={} render_release={} path=wgpu-immediates->resident-scene-batch->ui4\n",
        principal,
        SHADER_PACKAGE_CLIP_POSITION3_IMMEDIATE_RGBA_FNV1A64,
        batch.pipeline.raw(),
        batch.vertex_buffer.raw(),
        batch.index_buffer.raw(),
        expected_draws,
        batch.draws.iter().map(|draw| draw.index_count as usize).sum::<usize>(),
        width,
        height,
        point.value,
        release.sequence(),
    );
    Ok(Ui4SurfaceIndexedCompletion {
        window_id,
        surface: SurfaceInfo {
            handle: batch.surface,
            bytes,
            width,
            height,
            pitch,
        },
        release,
        point,
    })
}

fn rollback_indexed_submission_lease(
    principal: Principal,
    device_handle: DeviceHandle,
    queue_handle: QueueHandle,
    surface_handle: SurfaceHandle,
) {
    let mut broker = BROKER.lock();
    if let Ok(device) = lookup_device_mut(&mut broker, device_handle, principal) {
        if let Ok(surface) = lookup_surface_mut(device, surface_handle)
            && surface.in_flight == 2
        {
            surface.in_flight = 1;
        }
        if let Ok(queue) = lookup_queue_mut(device, queue_handle) {
            queue.in_flight = 0;
        }
    }
}

pub(crate) struct Ui4SurfaceClearCompletion {
    pub(crate) window_id: u32,
    pub(crate) surface: SurfaceInfo,
    pub(crate) release: crate::intel::gpgpu::GpgpuRgba8ReleaseFence,
    pub(crate) point: TimelinePoint,
}

/// Execute WebGPU's full-target render-pass clear through the mediated GPU
/// queue and retire the exact UI4 allocation. This is command semantics, not
/// a demo shader: the AOT fill kernel is the Intel implementation of
/// `LoadOp::Clear`, while shader pipelines remain a separate object class.
pub(crate) fn submit_ui4_surface_clear(
    principal: Principal,
    device_handle: DeviceHandle,
    queue_handle: QueueHandle,
    surface_handle: SurfaceHandle,
    rgba8_srgb: u32,
) -> Result<Ui4SurfaceClearCompletion, VgpuError> {
    let (window_id, phys, producer_gpu, bytes, width, height, pitch) = {
        let mut broker = BROKER.lock();
        let device = lookup_device_mut(&mut broker, device_handle, principal)?;
        ensure_live(device)?;
        if !device.capabilities.contains(Capabilities::RENDER)
            || !device.capabilities.contains(Capabilities::PRESENT)
        {
            return Err(VgpuError::PermissionDenied);
        }
        {
            let queue = lookup_queue_mut(device, queue_handle)?;
            if queue.class != QueueClass::Render || queue.in_flight != 0 {
                return Err(VgpuError::Busy);
            }
            queue.in_flight = 1;
        }
        let device_epoch = device.epoch;
        let surface = match lookup_surface_mut(device, surface_handle) {
            Ok(surface) if surface.epoch == device_epoch && surface.in_flight == 1 => surface,
            Ok(_) => {
                lookup_queue_mut(device, queue_handle)?.in_flight = 0;
                return Err(VgpuError::Busy);
            }
            Err(error) => {
                lookup_queue_mut(device, queue_handle)?.in_flight = 0;
                return Err(error);
            }
        };
        surface.in_flight = 2;
        (
            surface.window_id,
            surface.phys,
            surface.producer_gpu,
            surface.bytes,
            surface.width,
            surface.height,
            surface.pitch,
        )
    };

    let surface = crate::intel::gpgpu::GpgpuRgba8Surface::new(
        phys,
        producer_gpu,
        bytes,
        width,
        height,
        pitch,
    )
    .ok_or(VgpuError::Unsupported)?;
    let fill = crate::intel::gpgpu::fill_rect_rgba8_stats(surface, surface.bounds(), rgba8_srgb);
    let release = (fill.submits == 1)
        .then(|| crate::intel::gpgpu::release_rgba8_surface_for_scanout(surface))
        .filter(|release| release.ok)
        .and_then(|release| release.release);
    let Some(release) = release else {
        // A failed retirement is ambiguous: hardware may have accepted work.
        // Preserve every mapping and make the device fail-closed rather than
        // recycling an allocation that the GPU could still reference.
        let mut broker = BROKER.lock();
        if let Ok(device) = lookup_device_mut(&mut broker, device_handle, principal) {
            device.lost = true;
            if let Ok(queue) = lookup_queue_mut(device, queue_handle) {
                queue.in_flight = 0;
                queue.timeline.failures = queue.timeline.failures.saturating_add(1);
            }
            if let Ok(surface) = lookup_surface_mut(device, surface_handle) {
                surface.in_flight = 3;
            }
        }
        return Err(VgpuError::DeviceLost);
    };

    let physical = require_physical()?;
    let mut broker = BROKER.lock();
    let device = lookup_device_mut(&mut broker, device_handle, principal)?;
    ensure_live(device)?;
    let vm = match device.gpuvm {
        GpuVmBinding::Owned(vm) => vm,
        GpuVmBinding::Borrowed { .. } => return Err(VgpuError::Unsupported),
    };
    let (slot, generation) = decode_handle(surface_handle.raw())?;
    let surface_slot = device
        .surfaces
        .get_mut(slot)
        .ok_or(VgpuError::InvalidHandle)?;
    if surface_slot.generation != generation
        || surface_slot
            .record
            .as_ref()
            .is_none_or(|record| record.in_flight != 2)
    {
        return Err(VgpuError::DeviceLost);
    }
    let guest_gpu = surface_slot.record.as_ref().expect("validated surface").gpu;
    physical.unmap_gpuvm(vm, guest_gpu, bytes)?;
    let record = surface_slot.record.take().expect("validated surface");
    device.memory_used = device.memory_used.saturating_sub(record.bytes);
    let queue = lookup_queue_mut(device, queue_handle)?;
    queue.in_flight = 0;
    queue.timeline.submitted = queue.timeline.submitted.wrapping_add(1).max(1);
    queue.timeline.completed = queue.timeline.submitted;
    queue.timeline.last_physical_serial = release.sequence();
    let point = TimelinePoint {
        queue: queue_handle,
        value: queue.timeline.submitted,
        physical_serial: release.sequence(),
        physical_publish_sequence: release.sequence(),
    };
    Ok(Ui4SurfaceClearCompletion {
        window_id,
        surface: SurfaceInfo {
            handle: surface_handle,
            bytes,
            width,
            height,
            pitch,
        },
        release,
        point,
    })
}

pub(crate) fn discard_ui4_surface(
    principal: Principal,
    device_handle: DeviceHandle,
    surface_handle: SurfaceHandle,
) -> Result<(u32, SurfaceInfo), VgpuError> {
    let physical = require_physical()?;
    let mut broker = BROKER.lock();
    let device = lookup_device_mut(&mut broker, device_handle, principal)?;
    ensure_live(device)?;
    let (slot, generation) = decode_handle(surface_handle.raw())?;
    let surface_slot = device
        .surfaces
        .get_mut(slot)
        .ok_or(VgpuError::InvalidHandle)?;
    if surface_slot.generation != generation {
        return Err(VgpuError::InvalidHandle);
    }
    let record = surface_slot
        .record
        .as_ref()
        .ok_or(VgpuError::InvalidHandle)?;
    if record.epoch != device.epoch || record.in_flight != 1 {
        return Err(VgpuError::Busy);
    }
    let vm = match device.gpuvm {
        GpuVmBinding::Owned(vm) => vm,
        GpuVmBinding::Borrowed { .. } => return Err(VgpuError::Unsupported),
    };
    physical.unmap_gpuvm(vm, record.gpu, record.bytes)?;
    let record = surface_slot
        .record
        .take()
        .expect("validated vgpu UI4 surface");
    device.memory_used = device.memory_used.saturating_sub(record.bytes);
    Ok((
        record.window_id,
        SurfaceInfo {
            handle: surface_handle,
            bytes: record.bytes,
            width: record.width,
            height: record.height,
            pitch: record.pitch,
        },
    ))
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
            in_flight: 0,
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
        || lookup_queue(device, queue_handle)?.in_flight != 0
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
    if queue.in_flight != 0 {
        return Err(VgpuError::Busy);
    }
    queue.timeline.submitted = queue.timeline.submitted.wrapping_add(1).max(1);
    queue.timeline.completed = queue.timeline.submitted;
    Ok(TimelinePoint {
        queue: queue_handle,
        value: queue.timeline.submitted,
        physical_serial: 0,
        physical_publish_sequence: 0,
    })
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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
    let (queue_handle, existing_context) = {
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
                // A bound descriptor with no retained physical registration is
                // a quarantined identity, never permission to recycle backing.
                return Err(VgpuError::DeviceLost);
            };
            if binding.queue != queue_handle || binding.descriptor != descriptor {
                return Err(VgpuError::DeviceLost);
            }
            Some(binding.context)
        } else {
            if !device.contexts.is_empty() {
                return Err(VgpuError::DeviceLost);
            }
            if device.contexts.len() >= device.quota.contexts {
                return Err(VgpuError::QuotaExceeded);
            }
            None
        };
        (queue_handle, context)
    };

    let mut fault_service = PhysicalFaultServiceResult::default();
    let context = if let Some(context) = existing_context {
        context
    } else {
        let context = match physical.register_context(descriptor, client.physical_priority()) {
            Ok(context) => context,
            Err(error) => {
                // A rejected return does not prove REGISTER was never
                // published; policy enqueue or a queued CAT can fail after
                // firmware accepted the ID. Bind the immutable descriptor and
                // quarantine its storage even without a returned token.
                if let Ok(device) =
                    lookup_device_mut(&mut broker, device_handle, client.principal())
                {
                    device.kernel_context_capability = Some(descriptor);
                    device.lost = true;
                }
                service_physical_gpu_faults_locked(&mut broker, physical, &mut fault_service);
                drop(broker);
                finish_physical_gpu_fault_service(fault_service);
                return Err(error.into());
            }
        };
        let binding_inserted =
            match lookup_device_mut(&mut broker, device_handle, client.principal()) {
                Ok(device) => {
                    device.contexts.push(ContextBinding {
                        queue: queue_handle,
                        descriptor,
                        context,
                    });
                    device.kernel_context_capability = Some(descriptor);
                    true
                }
                Err(_) => false,
            };

        // REGISTER may itself ingest a queued CAT event. Publish the returned
        // token into the broker map first, then classify that event before any
        // SUBMIT can cross the same ownership boundary.
        service_physical_gpu_faults_locked(&mut broker, physical, &mut fault_service);
        if !binding_inserted {
            // A successful physical registration without a durable owner map
            // cannot be recovered by guessing which tenant owns the token.
            record_physical_device_loss_locked(&mut broker, &mut fault_service, 0);
            drop(broker);
            finish_physical_gpu_fault_service(fault_service);
            return Err(VgpuError::DeviceLost);
        }
        let current_lost = broker.physical_lost
            || match lookup_device(&broker, device_handle, client.principal()) {
                Ok(device) => device.lost,
                Err(_) => true,
            };
        if current_lost {
            drop(broker);
            finish_physical_gpu_fault_service(fault_service);
            return Err(VgpuError::DeviceLost);
        }
        context
    };

    // Record accepted work before servicing G2H so an immediately reported
    // CAT marks this exact virtual point failed rather than losing its fence.
    let submitted = match physical.submit_context(context) {
        Ok(submission) => match lookup_device_mut(&mut broker, device_handle, client.principal())
            .and_then(|device| lookup_queue_mut(device, queue_handle))
        {
            Ok(queue) => {
                queue.timeline.submitted = queue.timeline.submitted.wrapping_add(1).max(1);
                queue.timeline.last_physical_serial = submission.serial;
                Ok(TimelinePoint {
                    queue: queue_handle,
                    value: queue.timeline.submitted,
                    physical_serial: submission.serial,
                    physical_publish_sequence: submission.scheduler_publish_sequence,
                })
            }
            Err(error) => Err(error),
        },
        Err(error) => {
            if let Ok(queue) = lookup_device_mut(&mut broker, device_handle, client.principal())
                .and_then(|device| lookup_queue_mut(device, queue_handle))
            {
                queue.timeline.failures = queue.timeline.failures.saturating_add(1);
            }
            Err(error.into())
        }
    };
    service_physical_gpu_faults_locked(&mut broker, physical, &mut fault_service);
    let current_lost = broker.physical_lost
        || match lookup_device(&broker, device_handle, client.principal()) {
            Ok(device) => device.lost,
            Err(_) => true,
        };
    let submitted = if current_lost {
        Err(VgpuError::DeviceLost)
    } else {
        submitted
    };
    drop(broker);
    finish_physical_gpu_fault_service(fault_service);
    submitted
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
    if broker.physical_lost {
        return false;
    }
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
    let Some((handle, _)) = find_device_by_principal(&broker, client.principal()) else {
        // Isolation can race the interval after executor reservation but before
        // the first broker device is published. Fence that client in the
        // executor even when there is no physical context to tear down yet.
        drop(broker);
        crate::gpu::executor::notify_kernel_client_lost(client);
        return KernelClientIsolation::default();
    };
    let device = lookup_device_mut(&mut broker, handle, client.principal())
        .expect("principal lookup returned an invalid vGPU handle");
    device.lost = true;
    let mut report = KernelClientIsolation {
        device_found: true,
        ..KernelClientIsolation::default()
    };
    let Some(physical) = physical else {
        report.contexts_retained = device.contexts.len();
        drop(broker);
        crate::gpu::executor::notify_kernel_client_lost(client);
        return report;
    };

    let mut fault_service = PhysicalFaultServiceResult::default();
    // Isolation is itself a sticky loss boundary, even when no CAT record was
    // involved. Wake every fence for this client after releasing BROKER.
    fault_service.push_client(client);
    match destroy_device_contexts_locked(
        &mut broker,
        physical,
        handle,
        client.principal(),
        &mut fault_service,
    ) {
        Ok(destroyed) => report.contexts_disabled = destroyed,
        Err(_) => {
            // The helper retains every context whose firmware lifecycle or
            // fault attribution is incomplete, keeping its ID and backing from
            // becoming a late-write alias.
            report.contexts_retained = lookup_device(&broker, handle, client.principal())
                .map(|device| device.contexts.len())
                .unwrap_or(0);
        }
    }
    drop(broker);
    finish_physical_gpu_fault_service(fault_service);
    report
}

/// Compatibility name for callers whose lane and context are synonymous.
/// The immutable capability rule guarantees that one `KernelClient` owns at
/// most one physical context, so this never expands recovery to an engine or
/// another client.
pub(crate) fn isolate_kernel_client(client: KernelClient) -> KernelClientIsolation {
    isolate_kernel_context(client)
}

const fn kernel_client_for_principal(principal: Principal) -> Option<KernelClient> {
    match principal {
        Principal::KernelRender => Some(KernelClient::Render),
        Principal::KernelRender1 => Some(KernelClient::Render1),
        Principal::KernelRender2 => Some(KernelClient::Render2),
        Principal::KernelGpgpuSystem => Some(KernelClient::GpgpuSystem),
        Principal::KernelGpgpuFont => Some(KernelClient::GpgpuFont),
        Principal::KernelGpgpuExecution => Some(KernelClient::GpgpuExecution),
        Principal::KernelHelioCloud => Some(KernelClient::HelioCloud),
        Principal::KernelLfm25 => Some(KernelClient::Lfm25),
        Principal::KernelUi4Compositor => Some(KernelClient::Ui4Compositor),
        Principal::KernelUi4Blitter => Some(KernelClient::Ui4Blitter),
        Principal::HostRuntime | Principal::HullGuest(_) | Principal::RuntimeTest(_) => None,
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct PhysicalContextFaultIsolation {
    bindings_found: usize,
    devices_lost: usize,
    timelines_failed: usize,
    engine_mismatches: usize,
    ownership_corrupt: bool,
    clients: Vec<KernelClient>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RecordedPhysicalContextFault {
    context: PhysicalContextHandle,
    engine: super::physical::PhysicalEngineId,
    mediated: bool,
    kind: PhysicalContextFaultKind,
    hw_type: Option<u32>,
    report: PhysicalContextFaultIsolation,
    acknowledged: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct PhysicalFaultServiceResult {
    clients: Vec<KernelClient>,
    exact_reports: Vec<RecordedPhysicalContextFault>,
    global_report: Option<(u64, u64)>,
}

impl PhysicalFaultServiceResult {
    fn push_client(&mut self, client: KernelClient) {
        if !self.clients.contains(&client) {
            self.clients.push(client);
        }
    }
}

fn record_physical_device_loss_locked(
    broker: &mut Broker,
    result: &mut PhysicalFaultServiceResult,
    unattributed_events: u64,
) {
    let (epoch, newly_lost, affected) = mark_physical_device_lost_locked(broker);
    for client in affected {
        result.push_client(client);
    }
    if newly_lost {
        result.global_report = Some((epoch, unattributed_events));
    }
}

const fn physical_fault_ownership_is_exact(
    mediated: bool,
    bindings_found: usize,
    engine_mismatches: usize,
) -> bool {
    if mediated {
        bindings_found == 1 && engine_mismatches == 0
    } else {
        bindings_found == 0
    }
}

const _: () = {
    assert!(physical_fault_ownership_is_exact(true, 1, 0));
    assert!(physical_fault_ownership_is_exact(false, 0, 0));
    assert!(!physical_fault_ownership_is_exact(true, 0, 0));
    assert!(!physical_fault_ownership_is_exact(true, 2, 0));
    assert!(!physical_fault_ownership_is_exact(true, 1, 1));
    assert!(!physical_fault_ownership_is_exact(false, 1, 0));
};

/// Mark only the mediated owner of one generation-tagged physical context as
/// lost. This helper owns no GuC/backend calls, so `BROKER` is never nested
/// with the physical context registry.
fn mark_physical_context_faulted(
    broker: &mut Broker,
    context: PhysicalContextHandle,
    engine: super::physical::PhysicalEngineId,
    mediated: bool,
) -> PhysicalContextFaultIsolation {
    let mut report = PhysicalContextFaultIsolation::default();

    // Classify the ownership map before changing any tenant. A physical token
    // registered through this boundary must have exactly one matching broker
    // binding and engine. A direct backend token must have none. Anything else
    // is ownership corruption and requires global loss, never a guessed owner.
    for slot in &broker.devices {
        let Some(device) = slot.record.as_ref() else {
            continue;
        };
        for binding in &device.contexts {
            if binding.context != context {
                continue;
            }
            report.bindings_found = report.bindings_found.saturating_add(1);
            if binding.descriptor.engine != engine {
                report.engine_mismatches = report.engine_mismatches.saturating_add(1);
            }
        }
    }
    if !physical_fault_ownership_is_exact(mediated, report.bindings_found, report.engine_mismatches)
    {
        report.ownership_corrupt = true;
        return report;
    }
    if !mediated {
        return report;
    }

    for slot in &mut broker.devices {
        let Some(device) = slot.record.as_mut() else {
            continue;
        };
        let matching_bindings: Vec<(QueueHandle, bool)> = device
            .contexts
            .iter()
            .filter(|binding| binding.context == context)
            .map(|binding| (binding.queue, binding.descriptor.engine == engine))
            .collect();
        if matching_bindings.is_empty() {
            continue;
        }
        if let Some(client) = kernel_client_for_principal(device.principal)
            && !report.clients.contains(&client)
        {
            report.clients.push(client);
        }
        if device.lost {
            continue;
        }
        device.lost = true;
        report.devices_lost = report.devices_lost.saturating_add(1);
        for (index, (queue_handle, _)) in matching_bindings.iter().copied().enumerate() {
            if matching_bindings[..index]
                .iter()
                .any(|(previous, _)| *previous == queue_handle)
            {
                continue;
            }
            let Ok(queue) = lookup_queue_mut(device, queue_handle) else {
                continue;
            };
            if queue.timeline.completed < queue.timeline.submitted {
                queue.timeline.failures = queue.timeline.failures.saturating_add(1);
                let first_failed = queue.timeline.completed.saturating_add(1);
                for failed_point in first_failed..=queue.timeline.submitted {
                    if !queue.failed_points.contains(&failed_point) {
                        queue.failed_points.push(failed_point);
                    }
                }
                report.timelines_failed = report.timelines_failed.saturating_add(1);
            }
        }
    }
    report
}

/// Drain and classify every pending physical fault while the broker ownership
/// map is stable. Callers must release `BROKER` before delivering executor
/// notifications collected in `result`.
fn service_physical_gpu_faults_locked(
    broker: &mut Broker,
    physical: &'static dyn PhysicalGpuDevice,
    result: &mut PhysicalFaultServiceResult,
) {
    for fault in physical.fault_snapshot() {
        match fault {
            PhysicalGpuFault::Context {
                context,
                engine,
                mediated,
                kind,
                hw_type,
            } => {
                let report = mark_physical_context_faulted(broker, context, engine, mediated);
                for client in report.clients.iter().copied() {
                    result.push_client(client);
                }
                if report.ownership_corrupt {
                    record_physical_device_loss_locked(broker, result, 0);
                }
                let acknowledged = physical.acknowledge_context_fault(context);
                result.exact_reports.push(RecordedPhysicalContextFault {
                    context,
                    engine,
                    mediated,
                    kind,
                    hw_type,
                    report,
                    acknowledged,
                });
            }
            PhysicalGpuFault::UnattributedFault { events } => {
                record_physical_device_loss_locked(broker, result, events);
            }
        }
    }
}

fn finish_physical_gpu_fault_service(result: PhysicalFaultServiceResult) {
    for client in result.clients {
        crate::gpu::executor::notify_kernel_client_lost(client);
    }
    if let Some((epoch, events)) = result.global_report {
        crate::log_error!(
            target: "gpgpu";
            "vgpu: physical-device-lost=1 epoch={} unattributed_fault_events={} action=reject-all-devices-until-reset\n",
            epoch,
            events,
        );
    }
    for recorded in result.exact_reports {
        let RecordedPhysicalContextFault {
            context,
            engine,
            mediated,
            kind,
            hw_type,
            report,
            acknowledged,
        } = recorded;
        crate::log_error!(
            target: "gpgpu";
            "vgpu: physical-context-fault={} token=0x{:X} engine={:?}:{} mediated={} hw_type=0x{:08X} bindings_found={} devices_lost={} timelines_failed={} engine_mismatches={} ownership_corrupt={} action=quarantine-owner-and-backing hardware_lifecycle=retain-until-reset\n",
            kind.name(),
            context.raw(),
            engine.class,
            engine.instance,
            mediated as u8,
            hw_type.unwrap_or(u32::MAX),
            report.bindings_found,
            report.devices_lost,
            report.timelines_failed,
            report.engine_mismatches,
            report.ownership_corrupt as u8,
        );
        if !acknowledged {
            crate::log_warn!(
                target: "gpgpu";
                "vgpu: physical-context-fault ack=0 token=0x{:X} action=retain-and-retry\n",
                context.raw(),
            );
        }
    }
}

fn service_physical_gpu_faults_once() {
    let Some(physical) = physical_device() else {
        return;
    };
    // Canonical ownership order is BROKER -> physical/GuC. Holding the broker
    // across snapshot, classification, and acknowledgement prevents any CPU
    // from beginning a new lease in the middle of fault attribution.
    let mut broker = BROKER.lock();
    let mut result = PhysicalFaultServiceResult::default();
    service_physical_gpu_faults_locked(&mut broker, physical, &mut result);
    drop(broker);
    finish_physical_gpu_fault_service(result);
}

/// Boot-owned GuC event/containment pump. It is deliberately independent of
/// Helio, Spirit, UI4, fonts, and every individual GPU consumer.
#[trueos_executor::task]
pub(crate) async fn gpu_fault_containment_task() {
    loop {
        service_physical_gpu_faults_once();
        Timer::after(Duration::from_millis(2)).await;
    }
}

/// Reset/device-loss hook for the physical driver. All tenant handles remain
/// queryable for diagnosis but reject further allocation and submission.
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn notify_physical_device_lost() -> u64 {
    let mut broker = BROKER.lock();
    let (epoch, newly_lost, clients) = mark_physical_device_lost_locked(&mut broker);
    drop(broker);
    for client in clients {
        crate::gpu::executor::notify_kernel_client_lost(client);
    }
    if newly_lost {
        crate::log_error!(
            target: "gpgpu";
            "vgpu: physical-device-lost=1 epoch={} reason=physical-driver-notification action=reject-all-devices-until-reset\n",
            epoch,
        );
    }
    epoch
}

fn mark_physical_device_lost_locked(broker: &mut Broker) -> (u64, bool, Vec<KernelClient>) {
    if broker.physical_lost {
        return (broker.epoch, false, Vec::new());
    }
    broker.physical_lost = true;
    broker.epoch = broker.epoch.wrapping_add(1).max(1);
    let epoch = broker.epoch;
    let mut clients = Vec::new();
    for slot in &mut broker.devices {
        let Some(device) = slot.record.as_mut() else {
            continue;
        };
        if let Some(client) = kernel_client_for_principal(device.principal)
            && !clients.contains(&client)
        {
            clients.push(client);
        }
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
    (epoch, true, clients)
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
        physical_lost: broker.physical_lost,
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
            report.unique_hwlrcas &=
                hwlrca_backing_identity(*previous) != hwlrca_backing_identity(identity);
            report.unique_ppgtt_roots &= previous.gpuvm_root_phys != identity.gpuvm_root_phys;
        }
        identities.push(identity);
    }
    let helio = live_kernel_context_descriptor(broker, Principal::KernelRender);
    let spirit = live_kernel_context_descriptor(broker, Principal::KernelGpgpuExecution);
    let font = live_kernel_context_descriptor(broker, Principal::KernelGpgpuFont);
    report.helio_render_live = helio.is_some();
    report.spirit_execution_live = spirit.is_some();
    report.font_engine_live = font.is_some();
    if let (Some(helio), Some(spirit)) = (helio, spirit) {
        report.helio_spirit_distinct_hwlrca =
            hwlrca_backing_identity(helio) != hwlrca_backing_identity(spirit);
        report.helio_spirit_distinct_ppgtt_root = helio.gpuvm_root_phys != 0
            && spirit.gpuvm_root_phys != 0
            && helio.gpuvm_root_phys != spirit.gpuvm_root_phys;
    }
    if let (Some(font), Some(helio)) = (font, helio) {
        report.font_helio_distinct_hwlrca =
            hwlrca_backing_identity(font) != hwlrca_backing_identity(helio);
        report.font_helio_distinct_ppgtt_root = font.gpuvm_root_phys != 0
            && helio.gpuvm_root_phys != 0
            && font.gpuvm_root_phys != helio.gpuvm_root_phys;
    }
    if let (Some(font), Some(spirit)) = (font, spirit) {
        report.font_spirit_distinct_hwlrca =
            hwlrca_backing_identity(font) != hwlrca_backing_identity(spirit);
        report.font_spirit_distinct_ppgtt_root = font.gpuvm_root_phys != 0
            && spirit.gpuvm_root_phys != 0
            && font.gpuvm_root_phys != spirit.gpuvm_root_phys;
    }
    report
}

fn live_kernel_context_descriptor(
    broker: &Broker,
    principal: Principal,
) -> Option<PhysicalContextDescriptor> {
    let mut matches = broker
        .devices
        .iter()
        .filter_map(|slot| slot.record.as_ref())
        .filter(|device| device.principal == principal);
    let device = matches.next()?;
    if matches.next().is_some() || device.lost || device.contexts.len() != 1 {
        return None;
    }
    let identity = device.kernel_context_capability?;
    let GpuVmBinding::Borrowed { root_phys } = device.gpuvm else {
        return None;
    };
    let binding = &device.contexts[0];
    (root_phys != 0 && root_phys == identity.gpuvm_root_phys && binding.descriptor == identity)
        .then_some(identity)
}

const fn hwlrca_backing_identity(descriptor: PhysicalContextDescriptor) -> (u32, u32) {
    (descriptor.hwlrca_hi, descriptor.hwlrca_lo & !0xFFF)
}

/// Guest heap and VM-slot storage may be reused only after every vGPU device
/// for this principal has been removed. A retained device may still name guest
/// physical pages even when the VM CPU has already stopped.
pub(crate) fn hull_guest_storage_reusable(vm_id: u8) -> bool {
    let bit = 1u64.checked_shl(vm_id as u32).unwrap_or(0);
    bit != 0 && hull_guest_reuse_fence_mask() & bit == 0
}

pub(crate) fn hull_guest_reuse_fence_mask() -> u64 {
    let mut mask = {
        let broker = BROKER.lock();
        broker
            .devices
            .iter()
            .filter_map(|slot| slot.record.as_ref())
            .filter_map(|device| match device.principal {
                Principal::HullGuest(vm_id) => Some(u32::from(vm_id)),
                _ => None,
            })
            .filter_map(|vm_id| 1u64.checked_shl(vm_id))
            .fold(0u64, |mask, bit| mask | bit)
    };
    for vm_id in 0..crate::allcaps::hv::VM_ID_LIMIT {
        if crate::allocators::hv_guest_dma_ranges_pinned(vm_id as u8) {
            mask |= 1u64.checked_shl(vm_id as u32).unwrap_or(0);
        }
    }
    mask
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
    if BROKER.lock().physical_lost {
        return Err(VgpuError::DeviceLost);
    }
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
        | Principal::KernelGpgpuFont
        | Principal::KernelGpgpuExecution
        | Principal::KernelHelioCloud
        | Principal::KernelLfm25
        | Principal::KernelUi4Compositor
        | Principal::KernelUi4Blitter => caps
            .union(Capabilities::PRESENT)
            .union(Capabilities::KERNEL_CONTEXT),
        Principal::HullGuest(_) => caps.union(Capabilities::PRESENT),
        Principal::HostRuntime | Principal::RuntimeTest(_) => caps,
    }
}

const fn quota_for(principal: Principal) -> Quota {
    match principal {
        Principal::KernelRender
        | Principal::KernelRender1
        | Principal::KernelRender2
        | Principal::KernelGpgpuSystem
        | Principal::KernelGpgpuFont
        | Principal::KernelGpgpuExecution
        | Principal::KernelHelioCloud
        | Principal::KernelLfm25
        | Principal::KernelUi4Compositor
        | Principal::KernelUi4Blitter => Quota::KERNEL,
        Principal::HostRuntime => Quota::HOST,
        Principal::HullGuest(_) => Quota::GUEST,
        Principal::RuntimeTest(_) => Quota::TEST,
    }
}

const _: () = {
    assert!(Quota::GUEST.memory_bytes == 32 * 1024 * 1024);
    assert!(Quota::GUEST.buffers == 256);
};

fn ensure_kernel_device(
    broker: &mut Broker,
    client: KernelClient,
    root_phys: u64,
) -> Result<DeviceHandle, VgpuError> {
    if broker.physical_lost {
        return Err(VgpuError::DeviceLost);
    }
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
        surfaces: Vec::new(),
        shader_modules: Vec::new(),
        render_pipelines: Vec::new(),
        cloud_work_graphs: Vec::new(),
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
            in_flight: 0,
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

fn destroy_device_contexts_locked(
    broker: &mut Broker,
    physical: &'static dyn PhysicalGpuDevice,
    handle: DeviceHandle,
    principal: Principal,
    fault_service: &mut PhysicalFaultServiceResult,
) -> Result<usize, VgpuError> {
    let mut destroyed = 0usize;
    loop {
        if broker.physical_lost {
            return Err(VgpuError::DeviceLost);
        }
        let Some(context) = lookup_device(broker, handle, principal)?
            .contexts
            .last()
            .map(|binding| binding.context)
        else {
            return Ok(destroyed);
        };

        let exact_before = fault_service.exact_reports.len();
        let result = physical.destroy_context(context);
        // Keep the binding visible until every G2H event consumed by DESTROY
        // has been classified. Removing it first would turn an exact owner into
        // a false global ownership failure.
        service_physical_gpu_faults_locked(broker, physical, fault_service);
        let boundary_faulted = broker.physical_lost
            || fault_service.exact_reports[exact_before..]
                .iter()
                .any(|fault| fault.context == context);
        if boundary_faulted {
            return Err(VgpuError::DeviceLost);
        }
        result?;

        let device = lookup_device_mut(broker, handle, principal)?;
        let Some(binding) = device.contexts.last() else {
            return Err(VgpuError::InvalidHandle);
        };
        if binding.context != context {
            device.lost = true;
            return Err(VgpuError::DeviceLost);
        }
        device.contexts.pop();
        destroyed = destroyed.saturating_add(1);
    }
}

fn destroy_device_resources(
    physical: &'static dyn PhysicalGpuDevice,
    device: &mut VirtualDevice,
) -> Result<(), VgpuError> {
    if device_has_operation_leases(device) {
        return Err(VgpuError::Busy);
    }
    if !device.contexts.is_empty() {
        // Physical context teardown must run through
        // `destroy_device_contexts_locked` while the broker mapping is visible.
        return Err(VgpuError::DeviceLost);
    }
    let vm = match device.gpuvm {
        GpuVmBinding::Owned(vm) => Some(vm),
        GpuVmBinding::Borrowed { .. } => None,
    };
    if device.surfaces.iter().any(|slot| slot.record.is_some()) {
        return Err(VgpuError::Busy);
    }
    // Work graphs own no physical allocation of their own. Once every lease
    // is idle they must be dropped before their referenced buffers are
    // unmapped, preserving the same dependency order as explicit teardown.
    device.cloud_work_graphs.clear();
    device.render_pipelines.clear();
    device.shader_modules.clear();
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

fn device_has_operation_leases(device: &VirtualDevice) -> bool {
    device
        .buffers
        .iter()
        .filter_map(|slot| slot.record.as_ref())
        .any(|record| record.in_flight != 0)
        || device
            .surfaces
            .iter()
            .filter_map(|slot| slot.record.as_ref())
            .any(|record| record.in_flight != 0)
        || device
            .queues
            .iter()
            .filter_map(|slot| slot.record.as_ref())
            .any(|record| record.in_flight != 0)
        || device
            .cloud_work_graphs
            .iter()
            .filter_map(|slot| slot.record.as_ref())
            .any(|record| record.in_flight != 0)
}

fn release_buffer_backing(record: &mut BufferRecord) {
    match &mut record.backing {
        BufferBacking::Dma { virt, .. } => crate::dma::dealloc(*virt, record.bytes),
        BufferBacking::GuestPages { dma_pin, .. } => {
            if let Some(pin) = dma_pin.take() {
                // Every caller reaches this helper only after a successful
                // physical unmap. If token validation unexpectedly fails, the
                // allocator's aggregate pin count remains sticky/fail-closed.
                let _ = crate::allocators::release_hv_guest_dma_pin(pin);
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

fn insert_surface(device: &mut VirtualDevice, record: SurfaceRecord) -> SurfaceHandle {
    if let Some((slot, entry)) = device
        .surfaces
        .iter_mut()
        .enumerate()
        .find(|(_, entry)| entry.record.is_none())
    {
        entry.generation = entry.generation.wrapping_add(1).max(1);
        entry.record = Some(record);
        return SurfaceHandle(encode_handle(slot, entry.generation));
    }
    device.surfaces.push(SurfaceSlot {
        generation: 1,
        record: Some(record),
    });
    SurfaceHandle(encode_handle(device.surfaces.len() - 1, 1))
}

fn insert_shader_module(
    device: &mut VirtualDevice,
    record: ShaderModuleRecord,
) -> ShaderModuleHandle {
    if let Some((slot, entry)) = device
        .shader_modules
        .iter_mut()
        .enumerate()
        .find(|(_, entry)| entry.record.is_none())
    {
        entry.generation = entry.generation.wrapping_add(1).max(1);
        entry.record = Some(record);
        return ShaderModuleHandle(encode_handle(slot, entry.generation));
    }
    device.shader_modules.push(ShaderModuleSlot {
        generation: 1,
        record: Some(record),
    });
    ShaderModuleHandle(encode_handle(device.shader_modules.len() - 1, 1))
}

fn insert_render_pipeline(
    device: &mut VirtualDevice,
    record: RenderPipelineRecord,
) -> RenderPipelineHandle {
    if let Some((slot, entry)) = device
        .render_pipelines
        .iter_mut()
        .enumerate()
        .find(|(_, entry)| entry.record.is_none())
    {
        entry.generation = entry.generation.wrapping_add(1).max(1);
        entry.record = Some(record);
        return RenderPipelineHandle(encode_handle(slot, entry.generation));
    }
    device.render_pipelines.push(RenderPipelineSlot {
        generation: 1,
        record: Some(record),
    });
    RenderPipelineHandle(encode_handle(device.render_pipelines.len() - 1, 1))
}

fn insert_cloud_work_graph(
    device: &mut VirtualDevice,
    record: CloudWorkGraphRecord,
) -> CloudWorkGraphHandle {
    if let Some((slot, entry)) = device
        .cloud_work_graphs
        .iter_mut()
        .enumerate()
        .find(|(_, entry)| entry.record.is_none())
    {
        entry.generation = entry.generation.wrapping_add(1).max(1);
        entry.record = Some(record);
        return CloudWorkGraphHandle(encode_handle(slot, entry.generation));
    }
    device.cloud_work_graphs.push(CloudWorkGraphSlot {
        generation: 1,
        record: Some(record),
    });
    CloudWorkGraphHandle(encode_handle(device.cloud_work_graphs.len() - 1, 1))
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

fn lookup_surface(
    device: &VirtualDevice,
    handle: SurfaceHandle,
) -> Result<&SurfaceRecord, VgpuError> {
    let (slot, generation) = decode_handle(handle.raw())?;
    let entry = device.surfaces.get(slot).ok_or(VgpuError::InvalidHandle)?;
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

fn lookup_surface_mut(
    device: &mut VirtualDevice,
    handle: SurfaceHandle,
) -> Result<&mut SurfaceRecord, VgpuError> {
    let (slot, generation) = decode_handle(handle.raw())?;
    let entry = device
        .surfaces
        .get_mut(slot)
        .ok_or(VgpuError::InvalidHandle)?;
    if entry.generation != generation {
        return Err(VgpuError::InvalidHandle);
    }
    entry.record.as_mut().ok_or(VgpuError::InvalidHandle)
}

fn lookup_shader_module(
    device: &VirtualDevice,
    handle: ShaderModuleHandle,
) -> Result<&ShaderModuleRecord, VgpuError> {
    let (slot, generation) = decode_handle(handle.raw())?;
    let entry = device
        .shader_modules
        .get(slot)
        .ok_or(VgpuError::InvalidHandle)?;
    if entry.generation != generation {
        return Err(VgpuError::InvalidHandle);
    }
    entry.record.as_ref().ok_or(VgpuError::InvalidHandle)
}

fn lookup_render_pipeline(
    device: &VirtualDevice,
    handle: RenderPipelineHandle,
) -> Result<&RenderPipelineRecord, VgpuError> {
    let (slot, generation) = decode_handle(handle.raw())?;
    let entry = device
        .render_pipelines
        .get(slot)
        .ok_or(VgpuError::InvalidHandle)?;
    if entry.generation != generation {
        return Err(VgpuError::InvalidHandle);
    }
    entry.record.as_ref().ok_or(VgpuError::InvalidHandle)
}

fn lookup_cloud_work_graph(
    device: &VirtualDevice,
    handle: CloudWorkGraphHandle,
) -> Result<&CloudWorkGraphRecord, VgpuError> {
    let (slot, generation) = decode_handle(handle.raw())?;
    let entry = device
        .cloud_work_graphs
        .get(slot)
        .ok_or(VgpuError::InvalidHandle)?;
    if entry.generation != generation {
        return Err(VgpuError::InvalidHandle);
    }
    entry.record.as_ref().ok_or(VgpuError::InvalidHandle)
}

fn lookup_cloud_work_graph_mut(
    device: &mut VirtualDevice,
    handle: CloudWorkGraphHandle,
) -> Result<&mut CloudWorkGraphRecord, VgpuError> {
    let (slot, generation) = decode_handle(handle.raw())?;
    let entry = device
        .cloud_work_graphs
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
