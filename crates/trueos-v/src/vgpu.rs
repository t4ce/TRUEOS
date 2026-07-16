//! Opaque TRUEOS virtual GPU control facade.
//!
//! This is deliberately below WebGPU/OpenCL: it exposes tenant devices,
//! buffers, queues and timelines, but no Intel MMIO, physical addresses,
//! page-table entries, GuC context IDs, or shader-language semantics.

use crate::vcabi;

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
pub struct Queue(u64);

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

    pub fn create_buffer(self, bytes: usize, usage: u32) -> Result<Buffer, i32> {
        let mut handle = 0u64;
        rc_result(unsafe {
            vcabi::trueos_cabi_vgpu_buffer_create(self.0, bytes, usage, &mut handle)
        })?;
        Ok(Buffer(handle))
    }

    pub fn create_queue(self, class: QueueClass) -> Result<Queue, i32> {
        let mut handle = 0u64;
        rc_result(unsafe {
            vcabi::trueos_cabi_vgpu_queue_create(self.0, class as u32, &mut handle)
        })?;
        Ok(Queue(handle))
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
        let mut point = TimelinePoint::default();
        rc_result(unsafe {
            vcabi::trueos_cabi_vgpu_submit_control_nop(self.0, queue.0, &mut point)
        })?;
        Ok(point)
    }

    pub fn timeline(self, queue: Queue) -> Result<TimelineStatus, i32> {
        let mut status = TimelineStatus::default();
        rc_result(unsafe { vcabi::trueos_cabi_vgpu_timeline(self.0, queue.0, &mut status) })?;
        Ok(status)
    }

    pub fn wait(self, queue: Queue, value: u64) -> Result<(), i32> {
        rc_result(unsafe { vcabi::trueos_cabi_vgpu_wait(self.0, queue.0, value) })
    }

    pub fn destroy_queue(self, queue: Queue) -> Result<(), i32> {
        rc_result(unsafe { vcabi::trueos_cabi_vgpu_queue_destroy(self.0, queue.0) })
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

impl Queue {
    pub const fn raw(self) -> u64 {
        self.0
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
        assert_eq!(core::mem::size_of::<BufferInfo>(), 16);
        assert_eq!(core::mem::size_of::<TimelinePoint>(), 16);
        assert_eq!(core::mem::size_of::<TimelineStatus>(), 32);
    }
}
