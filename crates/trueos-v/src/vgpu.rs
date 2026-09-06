//! Opaque TRUEOS virtual GPU control facade.
//!
//! This is deliberately below WebGPU/OpenCL: it exposes tenant devices,
//! buffers, queues and timelines, but no Intel MMIO, physical addresses,
//! page-table entries, GuC context IDs, or shader-language semantics.

extern crate alloc;

use alloc::alloc::{Layout, alloc_zeroed, dealloc};
use core::ptr::NonNull;

use crate::vcabi;

unsafe extern "C" {
    fn trueos_vlayer_vram_snapshot_read(offset: usize, out_ptr: *mut u8, out_cap: usize) -> isize;
}

pub const ERR_IO: i32 = -5;
pub const ERR_BAD_HANDLE: i32 = -9;
pub const ERR_OUT_OF_MEMORY: i32 = -12;
pub const ERR_PERMISSION: i32 = -13;
pub const ERR_BUSY: i32 = -16;
pub const ERR_NO_DEVICE: i32 = -19;
pub const ERR_DEVICE_LOST: i32 = -32;
pub const ERR_UNSUPPORTED: i32 = -95;
pub const BUFFER_USAGE_MAP_READ: u32 = 1 << 0;
pub const BUFFER_USAGE_MAP_WRITE: u32 = 1 << 1;
pub const BUFFER_USAGE_STORAGE: u32 = 1 << 2;
pub const BUFFER_USAGE_COPY_SRC: u32 = 1 << 3;
pub const BUFFER_USAGE_COPY_DST: u32 = 1 << 4;
pub const BUFFER_USAGE_VERTEX: u32 = 1 << 5;
pub const BUFFER_USAGE_INDEX: u32 = 1 << 6;
pub const BUFFER_INFO_FLAG_VVIDEO_MEM: u32 = 1 << 0;
pub const SURFACE_FORMAT_RGBA8_UNORM_SRGB: u32 = 1;
/// Authenticated AOT package backing the first WGPU graphics-module contract.
/// The value is the stable FNV-1a digest of the exact WGSL accepted by the
/// Picasso renderer adapter, not an application/demo selector.
pub const SHADER_PACKAGE_CLIP_POSITION3_RGBA_FNV1A64: u64 = 0x1438_5963_136A_A36F;
/// Authenticated position-only package whose fragment color is supplied as
/// one `vec4<f32>` block of WGPU immediate data for each indexed draw.
pub const SHADER_PACKAGE_CLIP_POSITION3_IMMEDIATE_RGBA_FNV1A64: u64 = 0x4A7C_D238_6AA5_C232;
const VVIDEO_PAGE_BYTES: usize = 4096;

/// Return the byte length of the latest cached, best-effort vGPU memory snapshot.
pub fn memory_snapshot_len() -> Result<usize, i32> {
    count_result(unsafe { trueos_vlayer_vram_snapshot_read(0, core::ptr::null_mut(), 0) })
}

/// Read bytes from the latest cached, best-effort vGPU memory snapshot.
pub fn read_memory_snapshot(offset: usize, out: &mut [u8]) -> Result<usize, i32> {
    if out.is_empty() {
        return Ok(0);
    }
    count_result(unsafe { trueos_vlayer_vram_snapshot_read(offset, out.as_mut_ptr(), out.len()) })
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
#[repr(transparent)]
pub struct Capabilities(u64);

impl Capabilities {
    pub const BUFFER: Self = Self(1 << 0);
    pub const QUEUE: Self = Self(1 << 1);
    pub const TIMELINE: Self = Self(1 << 2);
    pub const COMPUTE: Self = Self(1 << 3);
    pub const RENDER: Self = Self(1 << 4);
    pub const COPY: Self = Self(1 << 5);
    pub const PRESENT: Self = Self(1 << 6);
    pub const DEFAULT: Self =
        Self(Self::BUFFER.0 | Self::QUEUE.0 | Self::TIMELINE.0 | Self::COMPUTE.0 | Self::RENDER.0);

    pub const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

    pub const fn bits(self) -> u64 {
        self.0
    }

    pub const fn contains(self, required: Self) -> bool {
        self.0 & required.0 == required.0
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum QueueClass {
    Render = 1,
    Compute = 2,
    Copy = 3,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
#[repr(C)]
pub struct DeviceInfo {
    pub capabilities: u64,
    pub epoch: u64,
    pub memory_used: u64,
    pub memory_quota: u64,
    pub buffer_count: u32,
    pub queue_count: u32,
    pub flags: u32,
    pub reserved: u32,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
#[repr(C)]
pub struct DeviceDiagnostics {
    pub copied_upload_bytes: u64,
    pub flushed_vvideo_bytes: u64,
    pub mapping_digest: u64,
    pub vvideo_buffers: u32,
    pub flags: u32,
}

impl DeviceDiagnostics {
    pub const FLAG_MAPPING_IDENTITY: u32 = 1 << 0;

    pub const fn mapping_identity(self) -> bool {
        self.flags & Self::FLAG_MAPPING_IDENTITY != 0
    }
}

impl DeviceInfo {
    pub const FLAG_LOST: u32 = 1 << 0;
    /// The resident renderer can execute the GS-backed `_ADJ` primitive
    /// topologies on the current physical adapter.
    pub const FLAG_ADJACENCY_TOPOLOGY_RENDERING: u32 = 1 << 1;

    pub const fn is_lost(self) -> bool {
        self.flags & Self::FLAG_LOST != 0
    }

    pub const fn supports_adjacency_topology_rendering(self) -> bool {
        self.flags & Self::FLAG_ADJACENCY_TOPOLOGY_RENDERING != 0
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
#[repr(C)]
pub struct BufferInfo {
    pub bytes: u64,
    pub usage: u32,
    pub flags: u32,
}

impl BufferInfo {
    pub const fn is_vvideo_mem(self) -> bool {
        self.flags & BUFFER_INFO_FLAG_VVIDEO_MEM != 0
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
#[repr(C)]
pub struct BufferSlice {
    pub buffer: u64,
    pub offset: u64,
    pub bytes: u64,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
#[repr(C)]
pub struct SurfaceInfo {
    pub surface: u64,
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub format: u32,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
#[repr(C)]
pub struct IndexedDraw {
    pub surface: u64,
    pub pipeline: u64,
    pub vertex_buffer: u64,
    pub index_buffer: u64,
    pub vertex_offset: u64,
    pub index_offset: u64,
    pub index_count: u32,
    pub first_index: u32,
    pub base_vertex: i32,
    pub clear_rgba8_srgb: u32,
    /// Zero preserves the legacy triangle list. Explicit triangle lists and
    /// native quad lists are supported; all other values are rejected.
    /// This occupies the former zero-reserved word without changing layout.
    pub topology: u32,
    /// Optional buffer-backed tightly packed RGBA8 sampled texture.
    pub sampled_texture: u64,
    pub texture_width: u32,
    pub texture_height: u32,
    pub texture_pitch: u32,
    pub sampler_flags: u32,
    /// INDEXED_DRAW_* flags. Zero keeps the original clear + depth-tested draw.
    pub texture_reserved: u32,
}

/// Continue an existing UI4 frame in painter order, without clearing or depth.
/// Sampled pixels are cached until write_buffer or destruction; the pixel
/// buffer must have MAP_WRITE-only usage.
pub const INDEXED_DRAW_LOAD_COLOR: u32 = 1;

pub const MAX_INDEXED_BATCH_DRAWS: usize = 16;
/// The mixed-topology V2 batch maps directly to the resident renderer's
/// 600-draw scene capacity. V1 remains at 16 for ABI compatibility.
pub const MAX_INDEXED_BATCH_V2_DRAWS: usize = 600;
pub const PRIMITIVE_TOPOLOGY_POINT_LIST: u32 = 1;
pub const PRIMITIVE_TOPOLOGY_LINE_LIST: u32 = 2;
pub const PRIMITIVE_TOPOLOGY_LINE_STRIP: u32 = 3;
pub const PRIMITIVE_TOPOLOGY_TRIANGLE_LIST: u32 = 4;
pub const PRIMITIVE_TOPOLOGY_TRIANGLE_STRIP: u32 = 5;
pub const PRIMITIVE_TOPOLOGY_TRIANGLE_FAN: u32 = 6;
/// Intel `3DPRIM_QUADLIST` / `3DSTATE_VF_TOPOLOGY` value 0x07. Every four
/// consecutive vertices form one independent quad.
pub const PRIMITIVE_TOPOLOGY_QUAD_LIST: u32 = 7;
/// Intel `3DPRIM_QUADSTRIP` / `3DSTATE_VF_TOPOLOGY` value 0x08. Each pair of
/// vertices after the first pair completes one connected four-vertex quad.
pub const PRIMITIVE_TOPOLOGY_QUAD_STRIP: u32 = 8;
/// Intel `3DPRIM_LINELIST_ADJ` / `3DSTATE_VF_TOPOLOGY` value 0x09. Four
/// vertices describe one line plus its two adjacent-only neighbours.
pub const PRIMITIVE_TOPOLOGY_LINE_LIST_ADJ: u32 = 0x09;
/// Intel `3DPRIM_LINESTRIP_ADJ` / `3DSTATE_VF_TOPOLOGY` value 0x0A. The
/// first and last input vertices are adjacent-only strip endpoints.
pub const PRIMITIVE_TOPOLOGY_LINE_STRIP_ADJ: u32 = 0x0a;
/// Intel `3DPRIM_TRILIST_ADJ` / `3DSTATE_VF_TOPOLOGY` value 0x0B. Every six
/// vertices describe one triangle and its three edge neighbours.
pub const PRIMITIVE_TOPOLOGY_TRIANGLE_LIST_ADJ: u32 = 0x0b;
/// Intel `3DPRIM_TRISTRIP_ADJ` / `3DSTATE_VF_TOPOLOGY` value 0x0C. Even
/// input vertices form the strip; odd vertices are adjacency-only data.
pub const PRIMITIVE_TOPOLOGY_TRIANGLE_STRIP_ADJ: u32 = 0x0c;
/// Intel `3DPRIM_RECTLIST` / `3DSTATE_VF_TOPOLOGY` value 0x0F. Every three
/// vertices specify one screen-aligned rectangle; hardware derives its fourth
/// corner.
pub const PRIMITIVE_TOPOLOGY_RECT_LIST: u32 = 0x0f;
/// Intel `3DPRIM_LINELOOP` value 0x10. Retained submission materializes its
/// final-to-first edge and emits it as a hardware line strip.
pub const PRIMITIVE_TOPOLOGY_LINE_LOOP: u32 = 0x10;

/// One indexed range in a single-target, single-pipeline render-pass batch.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
#[repr(C)]
pub struct IndexedBatchDraw {
    pub index_count: u32,
    pub first_index: u32,
    pub base_vertex: i32,
    /// Linear fragment color encoded to the target's RGBA8 sRGB bytes.
    pub rgba8_srgb: u32,
}

/// Bounded render-pass batch used by custom WGPU backends. All ranges share
/// one surface, pipeline and vertex/index binding, matching ordinary repeated
/// `draw_indexed` calls inside one WebGPU render pass.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
#[repr(C)]
pub struct IndexedDrawBatch {
    pub surface: u64,
    pub pipeline: u64,
    pub vertex_buffer: u64,
    pub index_buffer: u64,
    pub vertex_offset: u64,
    pub index_offset: u64,
    pub clear_rgba8_srgb: u32,
    pub draw_count: u32,
    pub draws: [IndexedBatchDraw; MAX_INDEXED_BATCH_DRAWS],
}

/// Versioned mixed-topology draw. The original indexed-batch ABI remains
/// triangle-list-only and unchanged.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
#[repr(C)]
pub struct IndexedBatchDrawV2 {
    pub index_count: u32,
    pub first_index: u32,
    pub base_vertex: i32,
    pub rgba8_srgb: u32,
    pub topology: u32,
    pub reserved: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct IndexedDrawBatchV2 {
    pub surface: u64,
    pub pipeline: u64,
    pub vertex_buffer: u64,
    pub index_buffer: u64,
    pub vertex_offset: u64,
    pub index_offset: u64,
    pub clear_rgba8_srgb: u32,
    pub draw_count: u32,
    pub draws: [IndexedBatchDrawV2; MAX_INDEXED_BATCH_V2_DRAWS],
}

impl Default for IndexedDrawBatchV2 {
    fn default() -> Self {
        Self {
            surface: 0,
            pipeline: 0,
            vertex_buffer: 0,
            index_buffer: 0,
            vertex_offset: 0,
            index_offset: 0,
            clear_rgba8_srgb: 0,
            draw_count: 0,
            draws: [IndexedBatchDrawV2::default(); MAX_INDEXED_BATCH_V2_DRAWS],
        }
    }
}

pub const MAX_RETAINED_TRANSFORM_SEEDS: usize = 4;
/// Buffer-backed V3 scenes keep the four-inline-seed V1/V2 ABI unchanged.
pub const MAX_RETAINED_SCENE_INSTANCES: usize = 512;
pub const MAX_RETAINED_SCENE_DRAWS: usize = 4;
pub const MAX_RETAINED_STATIC_DRAWS: usize = 3;
/// Fixed role order in the retained material descriptor. A zero texture ID is
/// an absent optional glTF map; every nonzero ID is validated as part of the
/// same owner-scoped submission.
pub const RETAINED_MATERIAL_TEXTURE_COUNT: usize = 5;
pub const RETAINED_MATERIAL_BASE_COLOR: usize = 0;
pub const RETAINED_MATERIAL_METALLIC_ROUGHNESS: usize = 1;
pub const RETAINED_MATERIAL_EMISSIVE: usize = 2;
pub const RETAINED_MATERIAL_OCCLUSION: usize = 3;
pub const RETAINED_MATERIAL_NORMAL: usize = 4;
pub const RETAINED_VERTEX_LAYOUT_POS_NORMAL: u32 = 0;
pub const RETAINED_VERTEX_LAYOUT_POS_NORMAL_UV: u32 = 1;
/// Position3, normal3, UV2, and tangent4 (including handedness), 48 bytes.
pub const RETAINED_VERTEX_LAYOUT_POS_NORMAL_UV_TANGENT: u32 = 2;
pub const RETAINED_MATERIAL_FLAG_DOUBLE_SIDED: u32 = 1 << 2;
/// Retained mesh topology field flag: honor glTF material `doubleSided` by
/// disabling fixed-function face culling for this mesh. The topology remains
/// in the low bits, keeping this cross-process descriptor ABI at 48 bytes.
pub const RETAINED_MESH_FLAG_DOUBLE_SIDED: u32 = 1 << 31;

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
#[repr(transparent)]
pub struct RetainedMesh(u64);

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
#[repr(C)]
pub struct RetainedMeshDescriptor {
    pub vertex_buffer: u64,
    pub index_buffer: u64,
    pub vertex_offset: u64,
    pub index_offset: u64,
    pub vertex_count: u32,
    pub index_count: u32,
    pub vertex_layout: u32,
    /// One of the `PRIMITIVE_TOPOLOGY_*` constants. A zero value is accepted
    /// as the pre-topology ABI's triangle-list default.
    pub topology: u32,
}

/// Exact compact input consumed by the authenticated retained-transform
/// artifact. Matrices, compaction, and indirect records remain GPU-authored.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
#[repr(C)]
pub struct RetainedTransformSeed {
    pub translation: [f32; 3],
    pub scale: [f32; 3],
    pub rotation: [f32; 4],
    pub local_radius: f32,
    pub previous_translation: [f32; 3],
    pub draw_group: u32,
    pub flags: u32,
}

/// Camera block consumed verbatim by the retained native vertex shader.
/// Matrices are column-major `mat4x4<f32>` values, matching WGSL and the
/// Helio shader artifact.  Keeping it in the frame request lets retained
/// model transforms remain object/world-space TRS instead of folding a
/// camera projection into each object.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
#[repr(C)]
pub struct RetainedCamera {
    pub view: [f32; 16],
    pub projection: [f32; 16],
    pub view_projection: [f32; 16],
    pub inverse_view_projection: [f32; 16],
    pub position_near: [f32; 4],
    pub forward_far: [f32; 4],
    pub jitter_frame: [f32; 4],
    pub previous_view_projection: [f32; 16],
}

/// One atomically validated retained material submission.
///
/// Images remain individual owner-scoped vmedia resources so their lifetime
/// can be reclaimed independently by the application. This descriptor is the
/// render-time bundle: it keeps their roles and scalar emissive factor in one
/// ABI value, and never negotiates map residency one texture at a time.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
#[repr(C)]
pub struct RetainedMaterial {
    pub textures: [u64; RETAINED_MATERIAL_TEXTURE_COUNT],
    pub emissive_factor: [f32; 3],
    pub reserved: u32,
}

#[derive(Copy, Clone, Debug, Default, PartialEq)]
#[repr(C)]
pub struct RetainedFrameSubmit {
    pub surface: u64,
    pub mesh: u64,
    /// Owner-scoped generation-checked vmedia material bundle. A POS_NORMAL_UV
    /// retained mesh requires the base-color slot; remaining glTF slots are
    /// optional but, when present, are validated together.
    pub material: RetainedMaterial,
    pub static_vertex_buffer: u64,
    pub static_index_buffer: u64,
    pub static_vertex_offset: u64,
    pub static_index_offset: u64,
    pub clear_rgba8_srgb: u32,
    pub seed_count: u32,
    pub static_draw_count: u32,
    /// Caller-owned content token for `static_vertex_buffer`. Advance it
    /// after rewriting any static vertex payload so a retained mesh refreshes
    /// its resident copy in place. Indices and draw identity remain immutable.
    pub static_vertex_revision: u32,
    /// Live camera data for the native retained vertex shader.
    pub camera: RetainedCamera,
    pub seeds: [RetainedTransformSeed; MAX_RETAINED_TRANSFORM_SEEDS],
    pub static_draws: [IndexedBatchDrawV2; MAX_RETAINED_STATIC_DRAWS],
}

/// Scalar inputs for the retained opaque metallic-roughness material shader.
/// Texture IDs remain in the nested frame's owner-scoped material bundle.
/// Only DOUBLE_SIDED is currently accepted in flags; reserved must be zero.
#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(C)]
pub struct RetainedMaterialParameters {
    pub base_color_factor: [f32; 4],
    pub emissive_factor: [f32; 3],
    pub normal_scale: f32,
    pub metallic_factor: f32,
    pub roughness_factor: f32,
    pub occlusion_strength: f32,
    pub alpha_cutoff: f32,
    pub flags: u32,
    pub reserved: [u32; 3],
}

impl Default for RetainedMaterialParameters {
    fn default() -> Self {
        Self {
            base_color_factor: [1.0; 4],
            emissive_factor: [0.0; 3],
            normal_scale: 1.0,
            metallic_factor: 1.0,
            roughness_factor: 1.0,
            occlusion_strength: 1.0,
            alpha_cutoff: 0.5,
            flags: 0,
            reserved: [0; 3],
        }
    }
}

/// Versioned extension of the stable V1 retained frame. V2 requires a
/// POS_NORMAL_UV_TANGENT mesh and supplies all factors through this extension;
/// the legacy frame.material.emissive_factor must remain zero.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
#[repr(C)]
pub struct RetainedFrameSubmitV2 {
    pub frame: RetainedFrameSubmit,
    pub material_parameters: RetainedMaterialParameters,
}

/// Index range within one resident mesh. All ranges share its PBR material.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
#[repr(C)]
pub struct RetainedDrawRange {
    pub first_index: u32,
    pub index_count: u32,
}
/// PBR scene with up to 512 buffer-backed TRS seeds and four index ranges.
/// Inline frame seeds/count must be zero. The seed buffer contains tightly
/// packed 64-byte RetainedTransformSeed rows and requires MAP_READ usage.
/// Each seed's draw_group selects a range; flags[31:16] selects its contiguous
/// group-local slot. Remaining range entries and reserved must be zero.
/// Floats must be finite, scale/radius nonnegative, and the quaternion must
/// have finite squared length greater than 1e-12 (the GPU normalizes it).
#[derive(Copy, Clone, Debug, Default, PartialEq)]
#[repr(C)]
pub struct RetainedFrameSubmitV3 {
    pub frame: RetainedFrameSubmitV2,
    pub seed_buffer: u64,
    pub seed_offset: u64,
    pub seed_count: u32,
    pub draw_count: u32,
    pub draws: [RetainedDrawRange; MAX_RETAINED_SCENE_DRAWS],
    pub reserved: [u32; 2],
}
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
#[repr(C)]
pub struct TimelinePoint {
    pub value: u64,
    pub physical_serial: u64,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
#[repr(C)]
pub struct TimelineStatus {
    pub submitted: u64,
    pub completed: u64,
    pub failures: u64,
    pub last_physical_serial: u64,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct Device(u64);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct Buffer(u64);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct ShaderModule(u64);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct RenderPipeline(u64);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct Surface(u64);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Queue {
    device: Device,
    handle: u64,
}

/// Page-granular VM memory that is simultaneously CPU mapped through VMX and
/// GPU mapped through the owning device's PPGTT. The GPU address remains
/// opaque; callers can only form bounds-checked slices.
pub struct VVideoMem {
    device: Device,
    buffer: Buffer,
    ptr: NonNull<u8>,
    requested_bytes: usize,
    mapped_bytes: usize,
    layout: Layout,
}

// SAFETY: VVideoMem uniquely owns a page-aligned allocation until Drop, and
// the broker keeps that allocation pinned while GPU work is in flight. Shared
// methods expose only opaque slices/cache operations; forming a CPU reference
// requires an exclusive `&mut VVideoMem` borrow.
unsafe impl Send for VVideoMem {}
unsafe impl Sync for VVideoMem {}

/// Types that may be viewed directly in zeroed vVideoMem storage.
///
/// # Safety
///
/// Every bit pattern must be valid and the type must have no drop glue.
pub unsafe trait VVideoPod: Copy {}

macro_rules! impl_vvideo_pod {
    ($($ty:ty),* $(,)?) => { $(unsafe impl VVideoPod for $ty {})* };
}
impl_vvideo_pod!(u8, u16, u32, u64, i8, i16, i32, i64, f32, f64);

impl Device {
    pub fn open(requested: Capabilities) -> Result<Self, i32> {
        let mut handle = 0u64;
        let rc = unsafe { vcabi::trueos_cabi_vgpu_open(requested.bits(), &mut handle) };
        rc_result(rc)?;
        if handle == 0 {
            return Err(ERR_BAD_HANDLE);
        }
        Ok(Self(handle))
    }

    pub const fn raw(self) -> u64 {
        self.0
    }

    pub fn info(self) -> Result<DeviceInfo, i32> {
        let mut info = DeviceInfo::default();
        rc_result(unsafe { vcabi::trueos_cabi_vgpu_device_info(self.0, &mut info) })?;
        Ok(info)
    }

    pub fn diagnostics(self) -> Result<DeviceDiagnostics, i32> {
        let mut diagnostics = DeviceDiagnostics::default();
        rc_result(unsafe { vcabi::trueos_cabi_vgpu_device_diagnostics(self.0, &mut diagnostics) })?;
        Ok(diagnostics)
    }

    pub fn create_buffer(self, bytes: usize, usage: u32) -> Result<Buffer, i32> {
        let mut handle = 0u64;
        rc_result(unsafe {
            vcabi::trueos_cabi_vgpu_buffer_create(self.0, bytes, usage, &mut handle)
        })?;
        Ok(Buffer(handle))
    }

    pub fn allocate_vvideo_mem(self, bytes: usize, usage: u32) -> Result<VVideoMem, i32> {
        if bytes == 0 {
            return Err(ERR_UNSUPPORTED);
        }
        let mapped_bytes = bytes
            .checked_add(VVIDEO_PAGE_BYTES - 1)
            .map(|value| value & !(VVIDEO_PAGE_BYTES - 1))
            .ok_or(ERR_OUT_OF_MEMORY)?;
        let layout = Layout::from_size_align(mapped_bytes, VVIDEO_PAGE_BYTES)
            .map_err(|_| ERR_OUT_OF_MEMORY)?;
        let ptr = NonNull::new(unsafe { alloc_zeroed(layout) }).ok_or(ERR_OUT_OF_MEMORY)?;
        let mut handle = 0u64;
        let rc = unsafe {
            vcabi::trueos_cabi_vgpu_vvideo_create(
                self.0,
                ptr.as_ptr() as u64,
                mapped_bytes,
                usage,
                &mut handle,
            )
        };
        if let Err(error) = rc_result(rc) {
            unsafe { dealloc(ptr.as_ptr(), layout) };
            return Err(error);
        }
        Ok(VVideoMem {
            device: self,
            buffer: Buffer(handle),
            ptr,
            requested_bytes: bytes,
            mapped_bytes,
            layout,
        })
    }

    pub fn create_queue(self, class: QueueClass) -> Result<Queue, i32> {
        let mut handle = 0u64;
        rc_result(unsafe {
            vcabi::trueos_cabi_vgpu_queue_create(self.0, class as u32, &mut handle)
        })?;
        Ok(Queue {
            device: self,
            handle,
        })
    }

    pub fn acquire_ui4_surface(self, window_id: u32) -> Result<Ui4Surface, i32> {
        let mut info = SurfaceInfo::default();
        rc_result(unsafe {
            vcabi::trueos_cabi_vgpu_ui4_surface_acquire(self.0, window_id, &mut info)
        })?;
        if info.surface == 0
            || info.width == 0
            || info.height == 0
            || info.pitch < info.width.saturating_mul(4)
            || info.format != SURFACE_FORMAT_RGBA8_UNORM_SRGB
        {
            return Err(ERR_IO);
        }
        Ok(Ui4Surface {
            device: self,
            surface: Surface(info.surface),
            info,
            live: true,
        })
    }

    pub fn create_shader_module(self, package_digest: u64) -> Result<ShaderModule, i32> {
        let mut handle = 0;
        rc_result(unsafe {
            vcabi::trueos_cabi_vgpu_shader_module_create(self.0, package_digest, &mut handle)
        })?;
        (handle != 0).then_some(ShaderModule(handle)).ok_or(ERR_IO)
    }

    pub fn destroy_shader_module(self, shader: ShaderModule) -> Result<(), i32> {
        rc_result(unsafe { vcabi::trueos_cabi_vgpu_shader_module_destroy(self.0, shader.0) })
    }

    pub fn create_render_pipeline(
        self,
        shader: ShaderModule,
        vertex_stride: u32,
        position_offset: u32,
    ) -> Result<RenderPipeline, i32> {
        let mut handle = 0;
        rc_result(unsafe {
            vcabi::trueos_cabi_vgpu_render_pipeline_create(
                self.0,
                shader.0,
                vertex_stride,
                position_offset,
                &mut handle,
            )
        })?;
        (handle != 0)
            .then_some(RenderPipeline(handle))
            .ok_or(ERR_IO)
    }

    pub fn destroy_render_pipeline(self, pipeline: RenderPipeline) -> Result<(), i32> {
        rc_result(unsafe { vcabi::trueos_cabi_vgpu_render_pipeline_destroy(self.0, pipeline.0) })
    }

    pub fn buffer_info(self, buffer: Buffer) -> Result<BufferInfo, i32> {
        let mut info = BufferInfo::default();
        rc_result(unsafe { vcabi::trueos_cabi_vgpu_buffer_info(self.0, buffer.0, &mut info) })?;
        Ok(info)
    }

    pub fn write_buffer(self, buffer: Buffer, offset: usize, bytes: &[u8]) -> Result<usize, i32> {
        count_result(unsafe {
            vcabi::trueos_cabi_vgpu_buffer_write(
                self.0,
                buffer.0,
                offset,
                bytes.as_ptr(),
                bytes.len(),
            )
        })
    }

    pub fn read_buffer(self, buffer: Buffer, offset: usize, out: &mut [u8]) -> Result<usize, i32> {
        count_result(unsafe {
            vcabi::trueos_cabi_vgpu_buffer_read(
                self.0,
                buffer.0,
                offset,
                out.as_mut_ptr(),
                out.len(),
            )
        })
    }

    pub fn destroy_buffer(self, buffer: Buffer) -> Result<(), i32> {
        rc_result(unsafe { vcabi::trueos_cabi_vgpu_buffer_destroy(self.0, buffer.0) })
    }

    pub fn submit_control_nop(self, queue: Queue) -> Result<TimelinePoint, i32> {
        if queue.device != self {
            return Err(ERR_BAD_HANDLE);
        }
        let mut point = TimelinePoint::default();
        rc_result(unsafe {
            vcabi::trueos_cabi_vgpu_submit_control_nop(self.0, queue.handle, &mut point)
        })?;
        Ok(point)
    }

    /// Submit one complete WebGPU render-pass clear to an imported UI4
    /// surface. The surface is consumed: successful retirement transfers its
    /// exact producer release back to UI4, while failure remains fail-closed.
    pub fn submit_ui4_clear(
        self,
        queue: Queue,
        surface: Ui4Surface,
        rgba8_srgb: u32,
    ) -> Result<TimelinePoint, i32> {
        if queue.device != self || surface.device != self {
            return Err(ERR_BAD_HANDLE);
        }
        let mut surface = surface;
        let mut point = TimelinePoint::default();
        rc_result(unsafe {
            vcabi::trueos_cabi_vgpu_ui4_surface_clear_submit(
                self.0,
                queue.handle,
                surface.surface.0,
                rgba8_srgb,
                &mut point,
            )
        })?;
        surface.live = false;
        Ok(point)
    }

    pub fn submit_ui4_indexed(
        self,
        queue: Queue,
        surface: Ui4Surface,
        pipeline: RenderPipeline,
        vertex_buffer: Buffer,
        index_buffer: Buffer,
        mut draw: IndexedDraw,
    ) -> Result<TimelinePoint, i32> {
        if queue.device != self || surface.device != self {
            return Err(ERR_BAD_HANDLE);
        }
        let mut surface = surface;
        draw.surface = surface.surface.0;
        draw.pipeline = pipeline.0;
        draw.vertex_buffer = vertex_buffer.0;
        draw.index_buffer = index_buffer.0;
        let mut point = TimelinePoint::default();
        rc_result(unsafe {
            vcabi::trueos_cabi_vgpu_ui4_indexed_submit(self.0, queue.handle, &draw, &mut point)
        })?;
        surface.live = false;
        Ok(point)
    }

    pub fn submit_ui4_indexed_batch(
        self,
        queue: Queue,
        surface: Ui4Surface,
        pipeline: RenderPipeline,
        vertex_buffer: Buffer,
        index_buffer: Buffer,
        mut batch: IndexedDrawBatch,
    ) -> Result<TimelinePoint, i32> {
        if queue.device != self
            || surface.device != self
            || batch.draw_count == 0
            || batch.draw_count as usize > MAX_INDEXED_BATCH_DRAWS
        {
            return Err(ERR_BAD_HANDLE);
        }
        let mut surface = surface;
        batch.surface = surface.surface.0;
        batch.pipeline = pipeline.0;
        batch.vertex_buffer = vertex_buffer.0;
        batch.index_buffer = index_buffer.0;
        let mut point = TimelinePoint::default();
        rc_result(unsafe {
            vcabi::trueos_cabi_vgpu_ui4_indexed_batch_submit(
                self.0,
                queue.handle,
                &batch,
                &mut point,
            )
        })?;
        surface.live = false;
        Ok(point)
    }

    pub fn submit_ui4_indexed_batch_v2(
        self,
        queue: Queue,
        surface: Ui4Surface,
        pipeline: RenderPipeline,
        vertex_buffer: Buffer,
        index_buffer: Buffer,
        mut batch: IndexedDrawBatchV2,
    ) -> Result<TimelinePoint, i32> {
        if queue.device != self
            || surface.device != self
            || batch.draw_count == 0
            || batch.draw_count as usize > MAX_INDEXED_BATCH_V2_DRAWS
        {
            return Err(ERR_BAD_HANDLE);
        }
        let mut surface = surface;
        batch.surface = surface.surface.0;
        batch.pipeline = pipeline.0;
        batch.vertex_buffer = vertex_buffer.0;
        batch.index_buffer = index_buffer.0;
        let mut point = TimelinePoint::default();
        rc_result(unsafe {
            vcabi::trueos_cabi_vgpu_ui4_indexed_batch_submit_v2(
                self.0,
                queue.handle,
                &batch,
                &mut point,
            )
        })?;
        surface.live = false;
        Ok(point)
    }

    pub fn create_retained_mesh(
        self,
        vertex_buffer: Buffer,
        index_buffer: Buffer,
        mut descriptor: RetainedMeshDescriptor,
    ) -> Result<RetainedMesh, i32> {
        descriptor.vertex_buffer = vertex_buffer.0;
        descriptor.index_buffer = index_buffer.0;
        let mut mesh = 0;
        rc_result(unsafe {
            vcabi::trueos_cabi_vgpu_retained_mesh_create(self.0, &descriptor, &mut mesh)
        })?;
        Ok(RetainedMesh(mesh))
    }

    pub fn destroy_retained_mesh(self, mesh: RetainedMesh) -> Result<(), i32> {
        rc_result(unsafe { vcabi::trueos_cabi_vgpu_retained_mesh_destroy(self.0, mesh.0) })
    }

    pub fn submit_retained_frame(
        self,
        queue: Queue,
        surface: Ui4Surface,
        mesh: RetainedMesh,
        static_vertex_buffer: Buffer,
        static_index_buffer: Buffer,
        mut submit: RetainedFrameSubmit,
    ) -> Result<TimelinePoint, i32> {
        if queue.device != self || surface.device != self {
            return Err(ERR_BAD_HANDLE);
        }
        let mut surface = surface;
        submit.surface = surface.surface.0;
        submit.mesh = mesh.0;
        submit.static_vertex_buffer = static_vertex_buffer.0;
        submit.static_index_buffer = static_index_buffer.0;
        let mut point = TimelinePoint::default();
        rc_result(unsafe {
            vcabi::trueos_cabi_vgpu_retained_frame_submit(self.0, queue.handle, &submit, &mut point)
        })?;
        surface.live = false;
        Ok(point)
    }

    pub fn submit_retained_frame_v2(
        self,
        queue: Queue,
        surface: Ui4Surface,
        mesh: RetainedMesh,
        static_vertex_buffer: Buffer,
        static_index_buffer: Buffer,
        mut submit: RetainedFrameSubmitV2,
    ) -> Result<TimelinePoint, i32> {
        if queue.device != self || surface.device != self {
            return Err(ERR_BAD_HANDLE);
        }
        let mut surface = surface;
        submit.frame.surface = surface.surface.0;
        submit.frame.mesh = mesh.0;
        submit.frame.static_vertex_buffer = static_vertex_buffer.0;
        submit.frame.static_index_buffer = static_index_buffer.0;
        let mut point = TimelinePoint::default();
        rc_result(unsafe {
            vcabi::trueos_cabi_vgpu_retained_frame_submit_v2(
                self.0,
                queue.handle,
                &submit,
                &mut point,
            )
        })?;
        surface.live = false;
        Ok(point)
    }

    pub fn submit_retained_frame_v3(
        self,
        queue: Queue,
        surface: Ui4Surface,
        mesh: RetainedMesh,
        static_vertex_buffer: Buffer,
        static_index_buffer: Buffer,
        mut submit: RetainedFrameSubmitV3,
    ) -> Result<TimelinePoint, i32> {
        if queue.device != self || surface.device != self {
            return Err(ERR_BAD_HANDLE);
        }
        let mut surface = surface;
        submit.frame.frame.surface = surface.surface.0;
        submit.frame.frame.mesh = mesh.0;
        submit.frame.frame.static_vertex_buffer = static_vertex_buffer.0;
        submit.frame.frame.static_index_buffer = static_index_buffer.0;
        let mut point = TimelinePoint::default();
        rc_result(unsafe {
            vcabi::trueos_cabi_vgpu_retained_frame_submit_v3(
                self.0,
                queue.handle,
                &submit,
                &mut point,
            )
        })?;
        surface.live = false;
        Ok(point)
    }

    pub fn timeline(self, queue: Queue) -> Result<TimelineStatus, i32> {
        if queue.device != self {
            return Err(ERR_BAD_HANDLE);
        }
        let mut status = TimelineStatus::default();
        rc_result(unsafe { vcabi::trueos_cabi_vgpu_timeline(self.0, queue.handle, &mut status) })?;
        Ok(status)
    }

    pub fn wait(self, queue: Queue, value: u64) -> Result<(), i32> {
        if queue.device != self {
            return Err(ERR_BAD_HANDLE);
        }
        rc_result(unsafe { vcabi::trueos_cabi_vgpu_wait(self.0, queue.handle, value) })
    }

    pub fn destroy_queue(self, queue: Queue) -> Result<(), i32> {
        if queue.device != self {
            return Err(ERR_BAD_HANDLE);
        }
        rc_result(unsafe { vcabi::trueos_cabi_vgpu_queue_destroy(self.0, queue.handle) })
    }

    pub fn close(self) -> Result<(), i32> {
        rc_result(unsafe { vcabi::trueos_cabi_vgpu_close(self.0) })
    }
}

impl Buffer {
    pub const fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Debug)]
pub struct Ui4Surface {
    device: Device,
    surface: Surface,
    info: SurfaceInfo,
    live: bool,
}

impl Ui4Surface {
    pub const fn info(&self) -> SurfaceInfo {
        self.info
    }

    pub const fn surface(&self) -> Surface {
        self.surface
    }

    pub fn discard(mut self) -> Result<(), i32> {
        let result = rc_result(unsafe {
            vcabi::trueos_cabi_vgpu_ui4_surface_discard(self.device.0, self.surface.0)
        });
        if result.is_ok() {
            self.live = false;
        }
        result
    }
}

impl Drop for Ui4Surface {
    fn drop(&mut self) {
        if self.live {
            let rc = unsafe {
                vcabi::trueos_cabi_vgpu_ui4_surface_discard(self.device.0, self.surface.0)
            };
            if rc == 0 {
                self.live = false;
            }
        }
    }
}

impl VVideoMem {
    pub const fn len(&self) -> usize {
        self.requested_bytes
    }

    /// Page-rounded extent registered in the VM's PPGTT.
    pub const fn mapped_len(&self) -> usize {
        self.mapped_bytes
    }

    pub const fn is_empty(&self) -> bool {
        self.requested_bytes == 0
    }

    pub const fn buffer(&self) -> Buffer {
        self.buffer
    }

    pub const fn as_ptr(&self) -> *const u8 {
        self.ptr.as_ptr()
    }

    pub fn as_bytes(&mut self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.ptr.as_ptr(), self.requested_bytes) }
    }

    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.requested_bytes) }
    }

    pub fn as_slice<T: VVideoPod>(&mut self) -> Result<&[T], i32> {
        if core::mem::size_of::<T>() == 0
            || self.requested_bytes % core::mem::size_of::<T>() != 0
            || !(self.ptr.as_ptr() as usize).is_multiple_of(core::mem::align_of::<T>())
        {
            return Err(ERR_UNSUPPORTED);
        }
        Ok(unsafe {
            core::slice::from_raw_parts(
                self.ptr.as_ptr() as *const T,
                self.requested_bytes / core::mem::size_of::<T>(),
            )
        })
    }

    pub fn as_slice_mut<T: VVideoPod>(&mut self) -> Result<&mut [T], i32> {
        if core::mem::size_of::<T>() == 0
            || self.requested_bytes % core::mem::size_of::<T>() != 0
            || !(self.ptr.as_ptr() as usize).is_multiple_of(core::mem::align_of::<T>())
        {
            return Err(ERR_UNSUPPORTED);
        }
        Ok(unsafe {
            core::slice::from_raw_parts_mut(
                self.ptr.as_ptr() as *mut T,
                self.requested_bytes / core::mem::size_of::<T>(),
            )
        })
    }

    pub fn slice(&self, offset: usize, bytes: usize) -> Result<BufferSlice, i32> {
        let end = offset.checked_add(bytes).ok_or(ERR_UNSUPPORTED)?;
        if end > self.requested_bytes {
            return Err(ERR_UNSUPPORTED);
        }
        Ok(BufferSlice {
            buffer: self.buffer.0,
            offset: offset as u64,
            bytes: bytes as u64,
        })
    }

    pub fn flush(&self, offset: usize, bytes: usize) -> Result<(), i32> {
        rc_result(unsafe {
            vcabi::trueos_cabi_vgpu_vvideo_flush(self.device.0, self.buffer.0, offset, bytes)
        })
    }

    pub fn invalidate(&self, offset: usize, bytes: usize) -> Result<(), i32> {
        rc_result(unsafe {
            vcabi::trueos_cabi_vgpu_vvideo_invalidate(self.device.0, self.buffer.0, offset, bytes)
        })
    }
}

impl Drop for VVideoMem {
    fn drop(&mut self) {
        let rc = unsafe { vcabi::trueos_cabi_vgpu_buffer_destroy(self.device.0, self.buffer.0) };
        if rc == 0 {
            unsafe { dealloc(self.ptr.as_ptr(), self.layout) };
        }
        // On an unexpected busy/device failure, intentionally leak the guest
        // allocation rather than let its physical pages be reused while a GPU
        // mapping might still exist.
    }
}

impl Queue {
    pub const fn raw(self) -> u64 {
        self.handle
    }
}

fn rc_result(rc: i32) -> Result<(), i32> {
    if rc == 0 { Ok(()) } else { Err(rc) }
}

fn count_result(count: isize) -> Result<usize, i32> {
    if count < 0 {
        Err(count as i32)
    } else {
        Ok(count as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_capabilities_form_the_stable_control_waist() {
        assert!(Capabilities::DEFAULT.contains(Capabilities::BUFFER));
        assert!(Capabilities::DEFAULT.contains(Capabilities::QUEUE));
        assert!(Capabilities::DEFAULT.contains(Capabilities::TIMELINE));
        assert!(!Capabilities::DEFAULT.contains(Capabilities::PRESENT));
    }

    #[test]
    fn device_info_reports_adjacency_rendering_as_an_explicit_feature() {
        assert!(!DeviceInfo::default().supports_adjacency_topology_rendering());
        let info = DeviceInfo {
            flags: DeviceInfo::FLAG_ADJACENCY_TOPOLOGY_RENDERING,
            ..DeviceInfo::default()
        };
        assert!(info.supports_adjacency_topology_rendering());
        assert!(!info.is_lost());
    }

    #[test]
    fn abi_records_have_stable_sizes() {
        assert_eq!(core::mem::size_of::<DeviceInfo>(), 48);
        assert_eq!(core::mem::size_of::<DeviceDiagnostics>(), 32);
        assert_eq!(core::mem::size_of::<BufferInfo>(), 16);
        assert_eq!(core::mem::size_of::<BufferSlice>(), 24);
        assert_eq!(core::mem::size_of::<SurfaceInfo>(), 32);
        assert_eq!(core::mem::size_of::<IndexedDraw>(), 104);
        assert_eq!(core::mem::size_of::<IndexedBatchDraw>(), 16);
        assert_eq!(core::mem::size_of::<IndexedDrawBatch>(), 312);
        assert_eq!(core::mem::size_of::<IndexedBatchDrawV2>(), 24);
        assert_eq!(core::mem::size_of::<IndexedDrawBatchV2>(), 14456);
        assert_eq!(core::mem::size_of::<RetainedMeshDescriptor>(), 48);
        assert_eq!(core::mem::offset_of!(RetainedMeshDescriptor, topology), 44);
        assert_eq!(core::mem::size_of::<RetainedTransformSeed>(), 64);
        assert_eq!(core::mem::size_of::<RetainedCamera>(), 368);
        assert_eq!(core::mem::size_of::<RetainedMaterial>(), 56);
        assert_eq!(core::mem::size_of::<RetainedFrameSubmit>(), 816);
        assert_eq!(core::mem::align_of::<RetainedFrameSubmit>(), 8);
        assert_eq!(core::mem::offset_of!(RetainedFrameSubmit, material), 16);
        assert_eq!(core::mem::offset_of!(RetainedFrameSubmit, static_vertex_revision), 116);
        assert_eq!(core::mem::offset_of!(RetainedFrameSubmit, camera), 120);
        assert_eq!(core::mem::offset_of!(RetainedFrameSubmit, seeds), 488);
        assert_eq!(core::mem::size_of::<RetainedMaterialParameters>(), 64);
        assert_eq!(core::mem::offset_of!(RetainedMaterialParameters, normal_scale), 28);
        assert_eq!(core::mem::offset_of!(RetainedMaterialParameters, flags), 48);
        assert_eq!(core::mem::size_of::<RetainedFrameSubmitV2>(), 880);
        assert_eq!(core::mem::offset_of!(RetainedFrameSubmitV2, frame), 0);
        assert_eq!(core::mem::offset_of!(RetainedFrameSubmitV2, material_parameters), 816);
        assert_eq!(core::mem::size_of::<TimelinePoint>(), 16);
        assert_eq!(core::mem::size_of::<TimelineStatus>(), 32);
    }

    #[test]
    fn vvideo_ownership_wrapper_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<VVideoMem>();
    }
}
