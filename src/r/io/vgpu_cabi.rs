//! Opaque vGPU C ABI and guest transport selection.

use crate::gpu::vgpu::{
    self, BufferHandle, Capabilities, DeviceHandle, Principal, QueueClass, QueueHandle,
};

fn queue_class(raw: u32) -> Result<QueueClass, i32> {
    match raw {
        1 => Ok(QueueClass::Render),
        2 => Ok(QueueClass::Compute),
        3 => Ok(QueueClass::Copy),
        _ => Err(-95),
    }
}

fn direct_principal() -> Principal {
    Principal::HostRuntime
}

pub(crate) fn broker_open(principal: Principal, requested: u64) -> Result<u64, i32> {
    vgpu::open(principal, Capabilities::from_bits(requested))
        .map(DeviceHandle::raw)
        .map_err(|error| error.errno())
}

pub(crate) fn broker_close(principal: Principal, device: u64) -> i32 {
    vgpu::close(principal, DeviceHandle::from_raw(device))
        .map(|()| 0)
        .unwrap_or_else(|error| error.errno())
}

pub(crate) fn broker_device_info(
    principal: Principal,
    device: u64,
) -> Result<v::vgpu::DeviceInfo, i32> {
    vgpu::device_info(principal, DeviceHandle::from_raw(device))
        .map(|info| v::vgpu::DeviceInfo {
            capabilities: info.capabilities.bits(),
            epoch: info.epoch,
            memory_used: info.memory_used as u64,
            memory_quota: info.memory_quota as u64,
            buffer_count: info.buffer_count as u32,
            queue_count: info.queue_count as u32,
            flags: if info.lost {
                v::vgpu::DeviceInfo::FLAG_LOST
            } else {
                0
            },
            reserved: 0,
        })
        .map_err(|error| error.errno())
}

pub(crate) fn broker_buffer_create(
    principal: Principal,
    device: u64,
    bytes: usize,
    usage: u32,
) -> Result<u64, i32> {
    vgpu::create_buffer(principal, DeviceHandle::from_raw(device), bytes, usage)
        .map(BufferHandle::raw)
        .map_err(|error| error.errno())
}

pub(crate) fn broker_buffer_destroy(principal: Principal, device: u64, buffer: u64) -> i32 {
    vgpu::destroy_buffer(principal, DeviceHandle::from_raw(device), BufferHandle::from_raw(buffer))
        .map(|()| 0)
        .unwrap_or_else(|error| error.errno())
}

pub(crate) fn broker_buffer_write(
    principal: Principal,
    device: u64,
    buffer: u64,
    offset: usize,
    bytes: &[u8],
) -> Result<usize, i32> {
    vgpu::write_buffer(
        principal,
        DeviceHandle::from_raw(device),
        BufferHandle::from_raw(buffer),
        offset,
        bytes,
    )
    .map_err(|error| error.errno())
}

pub(crate) fn broker_buffer_read(
    principal: Principal,
    device: u64,
    buffer: u64,
    offset: usize,
    out: &mut [u8],
) -> Result<usize, i32> {
    vgpu::read_buffer(
        principal,
        DeviceHandle::from_raw(device),
        BufferHandle::from_raw(buffer),
        offset,
        out,
    )
    .map_err(|error| error.errno())
}

pub(crate) fn broker_buffer_info(
    principal: Principal,
    device: u64,
    buffer: u64,
) -> Result<v::vgpu::BufferInfo, i32> {
    vgpu::buffer_info(principal, DeviceHandle::from_raw(device), BufferHandle::from_raw(buffer))
        .map(|info| v::vgpu::BufferInfo {
            bytes: info.bytes as u64,
            usage: info.usage,
            flags: info.flags,
        })
        .map_err(|error| error.errno())
}

pub(crate) fn broker_vvideo_create(
    principal: Principal,
    device: u64,
    guest_va: u64,
    bytes: usize,
    usage: u32,
) -> Result<u64, i32> {
    vgpu::create_vvideo_mem(
        principal,
        DeviceHandle::from_raw(device),
        guest_va,
        bytes,
        usage,
    )
    .map(BufferHandle::raw)
    .map_err(|error| error.errno())
}

pub(crate) fn broker_vvideo_flush(
    principal: Principal,
    device: u64,
    buffer: u64,
    offset: usize,
    bytes: usize,
) -> i32 {
    vgpu::flush_vvideo_mem(
        principal,
        DeviceHandle::from_raw(device),
        BufferHandle::from_raw(buffer),
        offset,
        bytes,
    )
    .map(|_| 0)
    .unwrap_or_else(|error| error.errno())
}

pub(crate) fn broker_vvideo_invalidate(
    principal: Principal,
    device: u64,
    buffer: u64,
    offset: usize,
    bytes: usize,
) -> i32 {
    vgpu::invalidate_vvideo_mem(
        principal,
        DeviceHandle::from_raw(device),
        BufferHandle::from_raw(buffer),
        offset,
        bytes,
    )
    .map(|_| 0)
    .unwrap_or_else(|error| error.errno())
}

pub(crate) fn broker_submit_scene_aabb(
    principal: Principal,
    device: u64,
    queue: u64,
    dispatch: v::vgpu::SceneAabbDispatch,
) -> Result<v::vgpu::SceneAabbResult, i32> {
    let convert = |slice: v::vgpu::BufferSlice| -> Result<vgpu::BufferSlice, i32> {
        Ok(vgpu::BufferSlice {
            buffer: BufferHandle::from_raw(slice.buffer),
            offset: usize::try_from(slice.offset).map_err(|_| -95)?,
            bytes: usize::try_from(slice.bytes).map_err(|_| -95)?,
        })
    };
    let mut bounds = [vgpu::BufferSlice {
        buffer: BufferHandle::from_raw(0),
        offset: 0,
        bytes: 0,
    }; 6];
    for (dst, src) in bounds.iter_mut().zip(dispatch.bounds) {
        *dst = convert(src)?;
    }
    let result = vgpu::submit_scene_aabb(
        principal,
        DeviceHandle::from_raw(device),
        QueueHandle::from_raw(queue),
        vgpu::SceneAabbDispatch {
            bounds,
            liveness: convert(dispatch.liveness)?,
            output: convert(dispatch.output)?,
            rows: dispatch.rows,
            query_min: [dispatch.query_min[0], dispatch.query_min[1], dispatch.query_min[2]],
            query_max: [dispatch.query_max[0], dispatch.query_max[1], dispatch.query_max[2]],
        },
    )
    .map_err(|error| error.errno())?;
    Ok(v::vgpu::SceneAabbResult {
        point: v::vgpu::TimelinePoint {
            value: result.point.value,
            physical_serial: result.point.physical_serial,
        },
        hits: result.hits,
        reserved: 0,
    })
}

pub(crate) fn broker_queue_create(
    principal: Principal,
    device: u64,
    class: u32,
) -> Result<u64, i32> {
    let class = queue_class(class)?;
    vgpu::create_queue(principal, DeviceHandle::from_raw(device), class)
        .map(QueueHandle::raw)
        .map_err(|error| error.errno())
}

pub(crate) fn broker_queue_destroy(principal: Principal, device: u64, queue: u64) -> i32 {
    vgpu::destroy_queue(principal, DeviceHandle::from_raw(device), QueueHandle::from_raw(queue))
        .map(|()| 0)
        .unwrap_or_else(|error| error.errno())
}

pub(crate) fn broker_submit_control_nop(
    principal: Principal,
    device: u64,
    queue: u64,
) -> Result<v::vgpu::TimelinePoint, i32> {
    vgpu::submit_control_nop(
        principal,
        DeviceHandle::from_raw(device),
        QueueHandle::from_raw(queue),
    )
    .map(|point| v::vgpu::TimelinePoint {
        value: point.value,
        physical_serial: point.physical_serial,
    })
    .map_err(|error| error.errno())
}

pub(crate) fn broker_timeline(
    principal: Principal,
    device: u64,
    queue: u64,
) -> Result<v::vgpu::TimelineStatus, i32> {
    vgpu::timeline_status(principal, DeviceHandle::from_raw(device), QueueHandle::from_raw(queue))
        .map(|status| v::vgpu::TimelineStatus {
            submitted: status.submitted,
            completed: status.completed,
            failures: status.failures,
            last_physical_serial: status.last_physical_serial,
        })
        .map_err(|error| error.errno())
}

pub(crate) fn broker_wait(principal: Principal, device: u64, queue: u64, value: u64) -> i32 {
    vgpu::wait_timeline(
        principal,
        DeviceHandle::from_raw(device),
        QueueHandle::from_raw(queue),
        value,
    )
    .map(|()| 0)
    .unwrap_or_else(|error| error.errno())
}

fn guest_rc(op: u32, arg0: u64, arg1: u64, request: &[u8]) -> i32 {
    let (status, data) = trueos_vm::vmcall::call_with_payload(op, arg0, arg1, request, &mut []);
    if status == trueos_vm::vmcall::STATUS_OK {
        data as i64 as i32
    } else {
        -5
    }
}

fn guest_handle(op: u32, arg0: u64, arg1: u64, request: &[u8]) -> Result<u64, i32> {
    let (status, data) = trueos_vm::vmcall::call_with_payload(op, arg0, arg1, request, &mut []);
    if status != trueos_vm::vmcall::STATUS_OK {
        return Err(-5);
    }
    if (data as i64) < 0 {
        Err(data as i64 as i32)
    } else {
        Ok(data)
    }
}

fn guest_record<T: Copy + Default>(
    op: u32,
    arg0: u64,
    arg1: u64,
    request: &[u8],
) -> Result<T, i32> {
    let mut value = T::default();
    let out = unsafe {
        core::slice::from_raw_parts_mut(
            (&mut value as *mut T).cast::<u8>(),
            core::mem::size_of::<T>(),
        )
    };
    let (status, rc) = trueos_vm::vmcall::call_with_payload(op, arg0, arg1, request, out);
    if status != trueos_vm::vmcall::STATUS_OK {
        return Err(-5);
    }
    if (rc as i64) < 0 {
        Err(rc as i64 as i32)
    } else {
        Ok(value)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_vgpu_open(requested_caps: u64, out_device: *mut u64) -> i32 {
    if out_device.is_null() {
        return -14;
    }
    let result = if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_handle(trueos_vm::vmcall::OP_BP_VGPU_OPEN, requested_caps, 0, &[])
    } else {
        broker_open(direct_principal(), requested_caps)
    };
    match result {
        Ok(handle) => {
            unsafe { out_device.write(handle) };
            0
        }
        Err(rc) => rc,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_vgpu_close(device: u64) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_rc(trueos_vm::vmcall::OP_BP_VGPU_CLOSE, device, 0, &[])
    } else {
        broker_close(direct_principal(), device)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_vgpu_device_info(
    device: u64,
    out_info: *mut v::vgpu::DeviceInfo,
) -> i32 {
    if out_info.is_null() {
        return -14;
    }
    let result = if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_record(trueos_vm::vmcall::OP_BP_VGPU_DEVICE_INFO, device, 0, &[])
    } else {
        broker_device_info(direct_principal(), device)
    };
    match result {
        Ok(info) => {
            unsafe { out_info.write(info) };
            0
        }
        Err(rc) => rc,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_vgpu_buffer_create(
    device: u64,
    bytes: usize,
    usage: u32,
    out_buffer: *mut u64,
) -> i32 {
    if out_buffer.is_null() {
        return -14;
    }
    let result = if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_handle(
            trueos_vm::vmcall::OP_BP_VGPU_BUFFER_CREATE,
            device,
            bytes as u64,
            &usage.to_le_bytes(),
        )
    } else {
        broker_buffer_create(direct_principal(), device, bytes, usage)
    };
    match result {
        Ok(handle) => {
            unsafe { out_buffer.write(handle) };
            0
        }
        Err(rc) => rc,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_vgpu_buffer_destroy(device: u64, buffer: u64) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_rc(trueos_vm::vmcall::OP_BP_VGPU_BUFFER_DESTROY, device, buffer, &[])
    } else {
        broker_buffer_destroy(direct_principal(), device, buffer)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_vgpu_buffer_write(
    device: u64,
    buffer: u64,
    offset: usize,
    data: *const u8,
    data_len: usize,
) -> isize {
    if data_len != 0 && data.is_null() {
        return -14;
    }
    let bytes = if data_len == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(data, data_len) }
    };
    if crate::hv::current_hull_guest_context_vm_id().is_none() {
        return broker_buffer_write(direct_principal(), device, buffer, offset, bytes)
            .map(|count| count as isize)
            .unwrap_or_else(|rc| rc as isize);
    }
    let chunk_cap = trueos_vm::vmcall::PAYLOAD_CAP.saturating_sub(8);
    let mut written = 0usize;
    while written < bytes.len() {
        let count = core::cmp::min(chunk_cap, bytes.len() - written);
        let mut request = alloc::vec::Vec::with_capacity(8 + count);
        request.extend_from_slice(&(offset + written).to_le_bytes());
        request.extend_from_slice(&bytes[written..written + count]);
        let rc = guest_rc(trueos_vm::vmcall::OP_BP_VGPU_BUFFER_WRITE, device, buffer, &request);
        if rc < 0 {
            return if written == 0 {
                rc as isize
            } else {
                written as isize
            };
        }
        if rc as usize != count {
            return if written == 0 { -5 } else { written as isize };
        }
        written += count;
    }
    written as isize
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_vgpu_buffer_read(
    device: u64,
    buffer: u64,
    offset: usize,
    out: *mut u8,
    out_len: usize,
) -> isize {
    if out_len != 0 && out.is_null() {
        return -14;
    }
    let out = if out_len == 0 {
        &mut []
    } else {
        unsafe { core::slice::from_raw_parts_mut(out, out_len) }
    };
    if crate::hv::current_hull_guest_context_vm_id().is_none() {
        return broker_buffer_read(direct_principal(), device, buffer, offset, out)
            .map(|count| count as isize)
            .unwrap_or_else(|rc| rc as isize);
    }
    let mut read = 0usize;
    while read < out.len() {
        let count = core::cmp::min(trueos_vm::vmcall::PAYLOAD_CAP, out.len() - read);
        let mut request = [0u8; 16];
        request[..8].copy_from_slice(&(offset + read).to_le_bytes());
        request[8..].copy_from_slice(&(count as u64).to_le_bytes());
        let (status, got) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_VGPU_BUFFER_READ,
            device,
            buffer,
            &request,
            &mut out[read..read + count],
        );
        if status != trueos_vm::vmcall::STATUS_OK {
            return if read == 0 { -5 } else { read as isize };
        }
        if (got as i64) < 0 {
            return if read == 0 {
                got as i64 as isize
            } else {
                read as isize
            };
        }
        let got = got as usize;
        if got == 0 || got > count {
            break;
        }
        read += got;
    }
    read as isize
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_vgpu_buffer_info(
    device: u64,
    buffer: u64,
    out_info: *mut v::vgpu::BufferInfo,
) -> i32 {
    if out_info.is_null() {
        return -14;
    }
    let result = if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_record(trueos_vm::vmcall::OP_BP_VGPU_BUFFER_INFO, device, buffer, &[])
    } else {
        broker_buffer_info(direct_principal(), device, buffer)
    };
    match result {
        Ok(info) => {
            unsafe { out_info.write(info) };
            0
        }
        Err(rc) => rc,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_vgpu_vvideo_create(
    device: u64,
    guest_va: u64,
    bytes: usize,
    usage: u32,
    out_buffer: *mut u64,
) -> i32 {
    if out_buffer.is_null() {
        return -14;
    }
    let Some(vm_id) = crate::hv::current_hull_guest_context_vm_id() else {
        return -95;
    };
    let mut request = [0u8; 12];
    request[..8].copy_from_slice(&(bytes as u64).to_le_bytes());
    request[8..].copy_from_slice(&usage.to_le_bytes());
    let result = guest_handle(
        trueos_vm::vmcall::OP_BP_VGPU_VVIDEO_CREATE,
        device,
        guest_va,
        &request,
    );
    let _ = vm_id;
    match result {
        Ok(handle) => {
            unsafe { out_buffer.write(handle) };
            0
        }
        Err(rc) => rc,
    }
}

fn guest_vvideo_range(op: u32, device: u64, buffer: u64, offset: usize, bytes: usize) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_none() {
        return -95;
    }
    let mut request = [0u8; 16];
    request[..8].copy_from_slice(&(offset as u64).to_le_bytes());
    request[8..].copy_from_slice(&(bytes as u64).to_le_bytes());
    guest_rc(op, device, buffer, &request)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_vgpu_vvideo_flush(
    device: u64,
    buffer: u64,
    offset: usize,
    bytes: usize,
) -> i32 {
    guest_vvideo_range(
        trueos_vm::vmcall::OP_BP_VGPU_VVIDEO_FLUSH,
        device,
        buffer,
        offset,
        bytes,
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_vgpu_vvideo_invalidate(
    device: u64,
    buffer: u64,
    offset: usize,
    bytes: usize,
) -> i32 {
    guest_vvideo_range(
        trueos_vm::vmcall::OP_BP_VGPU_VVIDEO_INVALIDATE,
        device,
        buffer,
        offset,
        bytes,
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_vgpu_queue_create(
    device: u64,
    class: u32,
    out_queue: *mut u64,
) -> i32 {
    if out_queue.is_null() {
        return -14;
    }
    let result = if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_handle(trueos_vm::vmcall::OP_BP_VGPU_QUEUE_CREATE, device, class as u64, &[])
    } else {
        broker_queue_create(direct_principal(), device, class)
    };
    match result {
        Ok(handle) => {
            unsafe { out_queue.write(handle) };
            0
        }
        Err(rc) => rc,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_vgpu_queue_destroy(device: u64, queue: u64) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_rc(trueos_vm::vmcall::OP_BP_VGPU_QUEUE_DESTROY, device, queue, &[])
    } else {
        broker_queue_destroy(direct_principal(), device, queue)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_vgpu_submit_control_nop(
    device: u64,
    queue: u64,
    out_point: *mut v::vgpu::TimelinePoint,
) -> i32 {
    if out_point.is_null() {
        return -14;
    }
    let result = if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_record(trueos_vm::vmcall::OP_BP_VGPU_SUBMIT_CONTROL_NOP, device, queue, &[])
    } else {
        broker_submit_control_nop(direct_principal(), device, queue)
    };
    match result {
        Ok(point) => {
            unsafe { out_point.write(point) };
            0
        }
        Err(rc) => rc,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_vgpu_submit_scene_aabb(
    device: u64,
    queue: u64,
    dispatch: *const v::vgpu::SceneAabbDispatch,
    out_result: *mut v::vgpu::SceneAabbResult,
) -> i32 {
    if dispatch.is_null() || out_result.is_null() {
        return -14;
    }
    let dispatch = unsafe { dispatch.read() };
    let request = unsafe {
        core::slice::from_raw_parts(
            (&dispatch as *const v::vgpu::SceneAabbDispatch).cast::<u8>(),
            core::mem::size_of::<v::vgpu::SceneAabbDispatch>(),
        )
    };
    let result = if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_record(
            trueos_vm::vmcall::OP_BP_VGPU_SCENE_AABB,
            device,
            queue,
            request,
        )
    } else {
        broker_submit_scene_aabb(direct_principal(), device, queue, dispatch)
    };
    match result {
        Ok(result) => {
            unsafe { out_result.write(result) };
            0
        }
        Err(rc) => rc,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_vgpu_timeline(
    device: u64,
    queue: u64,
    out_status: *mut v::vgpu::TimelineStatus,
) -> i32 {
    if out_status.is_null() {
        return -14;
    }
    let result = if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_record(trueos_vm::vmcall::OP_BP_VGPU_TIMELINE, device, queue, &[])
    } else {
        broker_timeline(direct_principal(), device, queue)
    };
    match result {
        Ok(status) => {
            unsafe { out_status.write(status) };
            0
        }
        Err(rc) => rc,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_vgpu_wait(device: u64, queue: u64, value: u64) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_rc(trueos_vm::vmcall::OP_BP_VGPU_WAIT, device, queue, &value.to_le_bytes())
    } else {
        broker_wait(direct_principal(), device, queue, value)
    }
}
