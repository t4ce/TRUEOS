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
/// HelioV adapter, not an application/demo selector.
pub const SHADER_PACKAGE_CLIP_POSITION3_RGBA_FNV1A64: u64 = 0x1438_5963_136A_A36F;
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

    pub const fn is_lost(self) -> bool {
        self.flags & Self::FLAG_LOST != 0
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
    pub reserved: u32,
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
        (handle != 0).then_some(RenderPipeline(handle)).ok_or(ERR_IO)
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
        draw.reserved = 0;
        let mut point = TimelinePoint::default();
        rc_result(unsafe {
            vcabi::trueos_cabi_vgpu_ui4_indexed_submit(
                self.0,
                queue.handle,
                &draw,
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
    fn abi_records_have_stable_sizes() {
        assert_eq!(core::mem::size_of::<DeviceInfo>(), 48);
        assert_eq!(core::mem::size_of::<DeviceDiagnostics>(), 32);
        assert_eq!(core::mem::size_of::<BufferInfo>(), 16);
        assert_eq!(core::mem::size_of::<BufferSlice>(), 24);
        assert_eq!(core::mem::size_of::<SurfaceInfo>(), 32);
        assert_eq!(core::mem::size_of::<IndexedDraw>(), 72);
        assert_eq!(core::mem::size_of::<TimelinePoint>(), 16);
        assert_eq!(core::mem::size_of::<TimelineStatus>(), 32);
    }

    #[test]
    fn vvideo_ownership_wrapper_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<VVideoMem>();
    }
}
