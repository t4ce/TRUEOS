//! Opaque vGPU C ABI and guest transport selection.

use crate::gpu::vgpu::{
    self, BufferHandle, Capabilities, DeviceHandle, Principal, QueueClass, QueueHandle,
    RenderPipelineHandle, ShaderModuleHandle, SurfaceHandle,
};

fn queue_class(raw: u32) -> Result<QueueClass, i32> {
    match raw {
        1 => Ok(QueueClass::Render),
        2 => Ok(QueueClass::Compute),
        3 => Ok(QueueClass::Copy),
        _ => Err(-95),
    }
}

/// Principal for a call that is already executing inside the kernel.
///
/// A Hull Blueprint normally reaches this file while running on its VMX
/// guest stack and therefore takes the vmcall transport below. `pthread`
/// jobs are different: the guest closure runs on a background service-lane
/// stack, while `kernel_task_domain` retains the owning VM. Those calls must
/// address the same tenant broker records, not fall through to HostRuntime.
fn direct_principal() -> Principal {
    crate::hv::current_guest_execution_context_vm_id()
        .map(|vm_id| Principal::HullGuest(vm_id as u16))
        .unwrap_or(Principal::HostRuntime)
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

pub(crate) fn broker_device_diagnostics(
    principal: Principal,
    device: u64,
) -> Result<v::vgpu::DeviceDiagnostics, i32> {
    vgpu::device_diagnostics(principal, DeviceHandle::from_raw(device))
        .map(|diagnostics| v::vgpu::DeviceDiagnostics {
            copied_upload_bytes: diagnostics.copied_upload_bytes,
            flushed_vvideo_bytes: diagnostics.flushed_vvideo_bytes,
            mapping_digest: diagnostics.mapping_digest,
            vvideo_buffers: diagnostics.vvideo_buffers as u32,
            flags: if diagnostics.mapping_identity {
                v::vgpu::DeviceDiagnostics::FLAG_MAPPING_IDENTITY
            } else {
                0
            },
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

fn ui4_owner(principal: Principal) -> Result<crate::ui4::WindowOwner, i32> {
    match principal {
        Principal::HullGuest(vm_id) => u8::try_from(vm_id)
            .map(crate::ui4::WindowOwner::Vm)
            .map_err(|_| -13),
        _ => Err(-13),
    }
}

pub(crate) fn broker_ui4_surface_acquire(
    principal: Principal,
    device: u64,
    window_id: u32,
) -> Result<v::vgpu::SurfaceInfo, i32> {
    let owner = ui4_owner(principal)?;
    let descriptor = match crate::ui4::blueprint_text::begin_vgpu_surface_import(owner, window_id) {
        Ok(descriptor) => descriptor,
        Err(error) => {
            crate::log_rate_limited!(
                target: "vgpu";
                level: crate::log_os::LogLevel::Warn;
                first: 3;
                every: 1_000;
                "vgpu ui4 surface import rejected stage=descriptor owner={:?} window={} errno={} action=retry\n",
                owner,
                window_id,
                error,
            );
            return Err(error);
        }
    };
    let imported = match vgpu::import_ui4_surface(
        principal,
        DeviceHandle::from_raw(device),
        descriptor,
    ) {
        Ok(imported) => imported,
        Err(error) => {
            crate::log_rate_limited!(
                target: "vgpu";
                level: crate::log_os::LogLevel::Error;
                first: 3;
                every: 1_000;
                "vgpu ui4 surface import rejected stage=map error={} errno={} owner={:?} window={} device=0x{:X} phys=0x{:X} bytes=0x{:X} size={}x{} pitch={} action=abort-import-and-retry\n",
                error.name(),
                error.errno(),
                owner,
                window_id,
                device,
                descriptor.phys,
                descriptor.bytes,
                descriptor.width,
                descriptor.height,
                descriptor.pitch,
            );
            crate::ui4::blueprint_text::abort_vgpu_surface_import(owner, window_id);
            return Err(error.errno());
        }
    };
    if let Err(error) = crate::ui4::blueprint_text::commit_vgpu_surface_import(
        owner,
        window_id,
        imported.handle.raw(),
    ) {
        let _ =
            vgpu::discard_ui4_surface(principal, DeviceHandle::from_raw(device), imported.handle);
        crate::ui4::blueprint_text::abort_vgpu_surface_import(owner, window_id);
        return Err(error);
    }
    Ok(v::vgpu::SurfaceInfo {
        surface: imported.handle.raw(),
        bytes: imported.bytes as u64,
        width: imported.width,
        height: imported.height,
        pitch: imported.pitch,
        format: v::vgpu::SURFACE_FORMAT_RGBA8_UNORM_SRGB,
    })
}

pub(crate) fn broker_ui4_surface_discard(principal: Principal, device: u64, surface: u64) -> i32 {
    let owner = match ui4_owner(principal) {
        Ok(owner) => owner,
        Err(error) => return error,
    };
    let (window_id, released) = match vgpu::discard_ui4_surface(
        principal,
        DeviceHandle::from_raw(device),
        SurfaceHandle::from_raw(surface),
    ) {
        Ok(released) => released,
        Err(error) => return error.errno(),
    };
    crate::ui4::blueprint_text::complete_vgpu_surface_discard(
        owner,
        window_id,
        released.handle.raw(),
    )
    .map(|()| 0)
    .unwrap_or_else(|error| error)
}

pub(crate) fn broker_ui4_surface_clear_submit(
    principal: Principal,
    device: u64,
    queue: u64,
    surface: u64,
    rgba8_srgb: u32,
) -> Result<v::vgpu::TimelinePoint, i32> {
    let owner = ui4_owner(principal)?;
    let completed = vgpu::submit_ui4_surface_clear(
        principal,
        DeviceHandle::from_raw(device),
        QueueHandle::from_raw(queue),
        SurfaceHandle::from_raw(surface),
        rgba8_srgb,
    )
    .map_err(|error| error.errno())?;
    crate::ui4::blueprint_text::complete_vgpu_surface_submission(
        owner,
        completed.window_id,
        completed.surface.handle.raw(),
        completed.release,
    )?;
    Ok(v::vgpu::TimelinePoint {
        value: completed.point.value,
        physical_serial: completed.point.physical_serial,
    })
}

pub(crate) fn broker_shader_module_create(
    principal: Principal,
    device: u64,
    package_digest: u64,
) -> Result<u64, i32> {
    vgpu::create_shader_module(principal, DeviceHandle::from_raw(device), package_digest)
        .map(ShaderModuleHandle::raw)
        .map_err(|error| error.errno())
}

pub(crate) fn broker_shader_module_destroy(principal: Principal, device: u64, shader: u64) -> i32 {
    vgpu::destroy_shader_module(
        principal,
        DeviceHandle::from_raw(device),
        ShaderModuleHandle::from_raw(shader),
    )
    .map(|()| 0)
    .unwrap_or_else(|error| error.errno())
}

pub(crate) fn broker_render_pipeline_create(
    principal: Principal,
    device: u64,
    shader: u64,
    vertex_stride: u32,
    position_offset: u32,
) -> Result<u64, i32> {
    vgpu::create_render_pipeline(
        principal,
        DeviceHandle::from_raw(device),
        ShaderModuleHandle::from_raw(shader),
        vertex_stride,
        position_offset,
    )
    .map(RenderPipelineHandle::raw)
    .map_err(|error| error.errno())
}

pub(crate) fn broker_render_pipeline_destroy(
    principal: Principal,
    device: u64,
    pipeline: u64,
) -> i32 {
    vgpu::destroy_render_pipeline(
        principal,
        DeviceHandle::from_raw(device),
        RenderPipelineHandle::from_raw(pipeline),
    )
    .map(|()| 0)
    .unwrap_or_else(|error| error.errno())
}

pub(crate) fn broker_ui4_indexed_submit(
    principal: Principal,
    device: u64,
    queue: u64,
    draw: v::vgpu::IndexedDraw,
) -> Result<v::vgpu::TimelinePoint, i32> {
    if draw.reserved != 0
        || draw.texture_reserved != 0
        || draw.sampler_flags & !v::vgpu::SAMPLER_FLAGS_ALL != 0
    {
        return Err(-22);
    }
    let owner = ui4_owner(principal)?;
    let completed = vgpu::submit_ui4_indexed_draw(
        principal,
        DeviceHandle::from_raw(device),
        QueueHandle::from_raw(queue),
        vgpu::Ui4IndexedDrawDescriptor {
            surface: SurfaceHandle::from_raw(draw.surface),
            pipeline: RenderPipelineHandle::from_raw(draw.pipeline),
            vertex_buffer: BufferHandle::from_raw(draw.vertex_buffer),
            index_buffer: BufferHandle::from_raw(draw.index_buffer),
            vertex_offset: usize::try_from(draw.vertex_offset).map_err(|_| -22)?,
            index_offset: usize::try_from(draw.index_offset).map_err(|_| -22)?,
            index_count: draw.index_count,
            first_index: draw.first_index,
            base_vertex: draw.base_vertex,
            clear_rgba8_srgb: draw.clear_rgba8_srgb,
            sampled_texture: BufferHandle::from_raw(draw.sampled_texture),
            texture_width: draw.texture_width,
            texture_height: draw.texture_height,
            texture_pitch: draw.texture_pitch,
            sampler_flags: draw.sampler_flags,
        },
    )
    .map_err(|error| error.errno())?;
    crate::ui4::blueprint_text::complete_vgpu_resident_surface_submission(
        owner,
        completed.window_id,
        completed.surface.handle.raw(),
        completed.release,
    )?;
    Ok(v::vgpu::TimelinePoint {
        value: completed.point.value,
        physical_serial: completed.point.physical_serial,
    })
}

pub(crate) fn broker_ui4_indexed_batch_submit(
    principal: Principal,
    device: u64,
    queue: u64,
    batch: v::vgpu::IndexedDrawBatch,
) -> Result<v::vgpu::TimelinePoint, i32> {
    let draw_count = usize::try_from(batch.draw_count).map_err(|_| -22)?;
    if draw_count == 0
        || draw_count > v::vgpu::MAX_INDEXED_BATCH_DRAWS
        || batch.draws[draw_count..]
            .iter()
            .any(|draw| *draw != v::vgpu::IndexedBatchDraw::default())
    {
        return Err(-22);
    }
    let owner = ui4_owner(principal)?;
    let draws = batch.draws[..draw_count]
        .iter()
        .map(|draw| vgpu::Ui4IndexedBatchDrawDescriptor {
            index_count: draw.index_count,
            first_index: draw.first_index,
            base_vertex: draw.base_vertex,
            rgba8_srgb: draw.rgba8_srgb,
            topology: crate::intel::render::ResidentScenePrimitiveTopology::TriangleList,
        })
        .collect();
    let completed = vgpu::submit_ui4_indexed_batch(
        principal,
        DeviceHandle::from_raw(device),
        QueueHandle::from_raw(queue),
        vgpu::Ui4IndexedBatchDescriptor {
            surface: SurfaceHandle::from_raw(batch.surface),
            pipeline: RenderPipelineHandle::from_raw(batch.pipeline),
            vertex_buffer: BufferHandle::from_raw(batch.vertex_buffer),
            index_buffer: BufferHandle::from_raw(batch.index_buffer),
            vertex_offset: usize::try_from(batch.vertex_offset).map_err(|_| -22)?,
            index_offset: usize::try_from(batch.index_offset).map_err(|_| -22)?,
            clear_rgba8_srgb: batch.clear_rgba8_srgb,
            draws,
        },
    )
    .map_err(|error| error.errno())?;
    crate::ui4::blueprint_text::complete_vgpu_resident_surface_submission(
        owner,
        completed.window_id,
        completed.surface.handle.raw(),
        completed.release,
    )?;
    Ok(v::vgpu::TimelinePoint {
        value: completed.point.value,
        physical_serial: completed.point.physical_serial,
    })
}

fn broker_primitive_topology(
    topology: u32,
) -> Result<crate::intel::render::ResidentScenePrimitiveTopology, i32> {
    match topology {
        v::vgpu::PRIMITIVE_TOPOLOGY_POINT_LIST => {
            Ok(crate::intel::render::ResidentScenePrimitiveTopology::PointList)
        }
        v::vgpu::PRIMITIVE_TOPOLOGY_LINE_LIST => {
            Ok(crate::intel::render::ResidentScenePrimitiveTopology::LineList)
        }
        v::vgpu::PRIMITIVE_TOPOLOGY_LINE_LIST_ADJ => {
            Ok(crate::intel::render::ResidentScenePrimitiveTopology::LineListAdj)
        }
        v::vgpu::PRIMITIVE_TOPOLOGY_LINE_STRIP => {
            Ok(crate::intel::render::ResidentScenePrimitiveTopology::LineStrip)
        }
        v::vgpu::PRIMITIVE_TOPOLOGY_LINE_STRIP_ADJ => {
            Ok(crate::intel::render::ResidentScenePrimitiveTopology::LineStripAdj)
        }
        v::vgpu::PRIMITIVE_TOPOLOGY_TRIANGLE_LIST => {
            Ok(crate::intel::render::ResidentScenePrimitiveTopology::TriangleList)
        }
        v::vgpu::PRIMITIVE_TOPOLOGY_TRIANGLE_LIST_ADJ => {
            Ok(crate::intel::render::ResidentScenePrimitiveTopology::TriangleListAdj)
        }
        v::vgpu::PRIMITIVE_TOPOLOGY_TRIANGLE_STRIP => {
            Ok(crate::intel::render::ResidentScenePrimitiveTopology::TriangleStrip)
        }
        v::vgpu::PRIMITIVE_TOPOLOGY_TRIANGLE_STRIP_ADJ => {
            Ok(crate::intel::render::ResidentScenePrimitiveTopology::TriangleStripAdj)
        }
        v::vgpu::PRIMITIVE_TOPOLOGY_TRIANGLE_FAN => {
            Ok(crate::intel::render::ResidentScenePrimitiveTopology::TriangleFan)
        }
        v::vgpu::PRIMITIVE_TOPOLOGY_QUAD_LIST => {
            Ok(crate::intel::render::ResidentScenePrimitiveTopology::QuadList)
        }
        v::vgpu::PRIMITIVE_TOPOLOGY_QUAD_STRIP => {
            Ok(crate::intel::render::ResidentScenePrimitiveTopology::QuadStrip)
        }
        v::vgpu::PRIMITIVE_TOPOLOGY_RECT_LIST => {
            Ok(crate::intel::render::ResidentScenePrimitiveTopology::RectList)
        }
        _ => Err(-95),
    }
}

#[cfg(test)]
mod primitive_topology_tests {
    use super::*;
    use crate::intel::render::ResidentScenePrimitiveTopology as Topology;

    #[test]
    fn v2_wire_values_reach_every_native_resident_topology() {
        let cases = [
            (v::vgpu::PRIMITIVE_TOPOLOGY_POINT_LIST, Topology::PointList),
            (v::vgpu::PRIMITIVE_TOPOLOGY_LINE_LIST, Topology::LineList),
            (v::vgpu::PRIMITIVE_TOPOLOGY_LINE_LIST_ADJ, Topology::LineListAdj),
            (v::vgpu::PRIMITIVE_TOPOLOGY_LINE_STRIP, Topology::LineStrip),
            (v::vgpu::PRIMITIVE_TOPOLOGY_LINE_STRIP_ADJ, Topology::LineStripAdj),
            (v::vgpu::PRIMITIVE_TOPOLOGY_TRIANGLE_LIST, Topology::TriangleList),
            (v::vgpu::PRIMITIVE_TOPOLOGY_TRIANGLE_LIST_ADJ, Topology::TriangleListAdj),
            (v::vgpu::PRIMITIVE_TOPOLOGY_TRIANGLE_STRIP, Topology::TriangleStrip),
            (v::vgpu::PRIMITIVE_TOPOLOGY_TRIANGLE_STRIP_ADJ, Topology::TriangleStripAdj),
            (v::vgpu::PRIMITIVE_TOPOLOGY_TRIANGLE_FAN, Topology::TriangleFan),
            (v::vgpu::PRIMITIVE_TOPOLOGY_QUAD_LIST, Topology::QuadList),
            (v::vgpu::PRIMITIVE_TOPOLOGY_QUAD_STRIP, Topology::QuadStrip),
            (v::vgpu::PRIMITIVE_TOPOLOGY_RECT_LIST, Topology::RectList),
        ];

        for (wire, expected) in cases {
            assert_eq!(broker_primitive_topology(wire), Ok(expected));
        }
        assert_eq!(broker_primitive_topology(0), Err(-95));
        assert_eq!(broker_primitive_topology(u32::MAX), Err(-95));
    }
}

pub(crate) fn broker_ui4_indexed_batch_submit_v2(
    principal: Principal,
    device: u64,
    queue: u64,
    batch: v::vgpu::IndexedDrawBatchV2,
) -> Result<v::vgpu::TimelinePoint, i32> {
    let draw_count = usize::try_from(batch.draw_count).map_err(|_| -22)?;
    if draw_count == 0
        || draw_count > v::vgpu::MAX_INDEXED_BATCH_V2_DRAWS
        || batch.draws[..draw_count]
            .iter()
            .any(|draw| draw.reserved != 0)
        || batch.draws[draw_count..]
            .iter()
            .any(|draw| *draw != v::vgpu::IndexedBatchDrawV2::default())
    {
        return Err(-22);
    }
    let owner = ui4_owner(principal)?;
    let draws = batch.draws[..draw_count]
        .iter()
        .map(|draw| {
            Ok(vgpu::Ui4IndexedBatchDrawDescriptor {
                index_count: draw.index_count,
                first_index: draw.first_index,
                base_vertex: draw.base_vertex,
                rgba8_srgb: draw.rgba8_srgb,
                topology: broker_primitive_topology(draw.topology)?,
            })
        })
        .collect::<Result<_, i32>>()?;
    let completed = vgpu::submit_ui4_indexed_batch(
        principal,
        DeviceHandle::from_raw(device),
        QueueHandle::from_raw(queue),
        vgpu::Ui4IndexedBatchDescriptor {
            surface: SurfaceHandle::from_raw(batch.surface),
            pipeline: RenderPipelineHandle::from_raw(batch.pipeline),
            vertex_buffer: BufferHandle::from_raw(batch.vertex_buffer),
            index_buffer: BufferHandle::from_raw(batch.index_buffer),
            vertex_offset: usize::try_from(batch.vertex_offset).map_err(|_| -22)?,
            index_offset: usize::try_from(batch.index_offset).map_err(|_| -22)?,
            clear_rgba8_srgb: batch.clear_rgba8_srgb,
            draws,
        },
    )
    .map_err(|error| error.errno())?;
    crate::ui4::blueprint_text::complete_vgpu_resident_surface_submission(
        owner,
        completed.window_id,
        completed.surface.handle.raw(),
        completed.release,
    )?;
    Ok(v::vgpu::TimelinePoint {
        value: completed.point.value,
        physical_serial: completed.point.physical_serial,
    })
}

pub(crate) fn broker_retained_mesh_create(
    principal: Principal,
    device: u64,
    descriptor: v::vgpu::RetainedMeshDescriptor,
) -> Result<u64, i32> {
    vgpu::create_retained_mesh(principal, DeviceHandle::from_raw(device), descriptor)
        .map(vgpu::RetainedMeshHandle::raw)
        .map_err(|error| error.errno())
}

pub(crate) fn broker_retained_mesh_destroy(principal: Principal, device: u64, mesh: u64) -> i32 {
    vgpu::destroy_retained_mesh(
        principal,
        DeviceHandle::from_raw(device),
        vgpu::RetainedMeshHandle::from_raw(mesh),
    )
    .map(|()| 0)
    .unwrap_or_else(|error| error.errno())
}

pub(crate) fn broker_retained_frame_submit(
    principal: Principal,
    device: u64,
    queue: u64,
    submit: v::vgpu::RetainedFrameSubmit,
) -> Result<v::vgpu::TimelinePoint, i32> {
    let owner = ui4_owner(principal)?;
    let completed = vgpu::submit_ui4_retained_frame(
        principal,
        DeviceHandle::from_raw(device),
        QueueHandle::from_raw(queue),
        submit,
    )
    .map_err(|error| error.errno())?;
    crate::ui4::blueprint_text::complete_vgpu_resident_surface_submission(
        owner,
        completed.window_id,
        completed.surface.handle.raw(),
        completed.release,
    )?;
    Ok(v::vgpu::TimelinePoint {
        value: completed.point.value,
        physical_serial: completed.point.physical_serial,
    })
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
    vgpu::create_vvideo_mem(principal, DeviceHandle::from_raw(device), guest_va, bytes, usage)
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

pub(crate) fn broker_cloud_work_graph_create(
    principal: Principal,
    device: u64,
    descriptor: v::vgpu::CloudWorkGraphDescriptor,
) -> Result<u64, i32> {
    let d = vgpu::CloudWorkGraphDescriptor {
        volume_a: BufferHandle::from_raw(descriptor.volume_a),
        volume_b: BufferHandle::from_raw(descriptor.volume_b),
        sim_params: BufferHandle::from_raw(descriptor.sim_params),
        render_params: BufferHandle::from_raw(descriptor.render_params),
        profile: descriptor.profile,
    };
    vgpu::create_cloud_work_graph(principal, DeviceHandle::from_raw(device), d)
        .map(|h| h.raw())
        .map_err(|e| e.errno())
}

pub(crate) fn broker_cloud_work_graph_destroy(
    principal: Principal,
    device: u64,
    graph: u64,
) -> i32 {
    vgpu::destroy_cloud_work_graph(
        principal,
        DeviceHandle::from_raw(device),
        vgpu::CloudWorkGraphHandle::from_raw(graph),
    )
    .map(|_| 0)
    .unwrap_or_else(|e| e.errno())
}

pub(crate) fn broker_cloud_frame_submit(
    principal: Principal,
    device: u64,
    queue: u64,
    graph: u64,
    surface: u64,
    steps: u32,
) -> Result<v::vgpu::CloudFrameTelemetry, i32> {
    let owner = ui4_owner(principal)?;
    let progress = vgpu::submit_cloud_frame(
        principal,
        DeviceHandle::from_raw(device),
        QueueHandle::from_raw(queue),
        vgpu::CloudWorkGraphHandle::from_raw(graph),
        SurfaceHandle::from_raw(surface),
        steps,
    )
    .map_err(|e| e.errno())?;
    let vgpu::CloudFrameSubmissionProgress::Complete(completed) = progress else {
        return Err(-16);
    };
    complete_cloud_frame_submission(owner, completed)
}

pub(crate) enum BrokerCloudFrameSubmissionProgress {
    Pending,
    Complete(v::vgpu::CloudFrameTelemetry),
}

/// Advance one matching VMCALL-owned Cloud ticket. The caller decides when to
/// retry; this function never spins while the HelioC context is in flight.
pub(crate) fn broker_cloud_frame_submit_retry(
    principal: Principal,
    device: u64,
    queue: u64,
    graph: u64,
    surface: u64,
    steps: u32,
) -> Result<BrokerCloudFrameSubmissionProgress, i32> {
    let owner = ui4_owner(principal)?;
    match vgpu::retry_cloud_frame_submission(
        principal,
        DeviceHandle::from_raw(device),
        QueueHandle::from_raw(queue),
        vgpu::CloudWorkGraphHandle::from_raw(graph),
        SurfaceHandle::from_raw(surface),
        steps,
    )
    .map_err(|e| e.errno())?
    {
        vgpu::CloudFrameSubmissionProgress::Pending => {
            Ok(BrokerCloudFrameSubmissionProgress::Pending)
        }
        vgpu::CloudFrameSubmissionProgress::Complete(completed) => {
            complete_cloud_frame_submission(owner, completed)
                .map(BrokerCloudFrameSubmissionProgress::Complete)
        }
    }
}

fn complete_cloud_frame_submission(
    owner: crate::ui4::WindowOwner,
    completed: vgpu::CloudFrameCompletion,
) -> Result<v::vgpu::CloudFrameTelemetry, i32> {
    // `submit_cloud_frame` reaches this boundary only after an authenticated
    // native completion has retired the producer release and removed the
    // tenant GPUVM alias. UI4 now owns the exact physical allocation until
    // its display SURFLIVE release; the guest sees only the virtual timeline.
    crate::ui4::blueprint_text::complete_vgpu_surface_submission(
        owner,
        completed.window_id,
        completed.surface.handle.raw(),
        completed.release,
    )?;
    Ok(v::vgpu::CloudFrameTelemetry {
        point: v::vgpu::TimelinePoint {
            value: completed.telemetry.point.value,
            physical_serial: completed.telemetry.point.physical_serial,
        },
        gpu_active_ns: completed.telemetry.gpu_active_ns,
        budget_window_ns: completed.telemetry.budget_window_ns,
        simulation_steps: completed.telemetry.simulation_steps,
        simd_width: completed.telemetry.simd_width,
        flags: completed.telemetry.flags,
        reserved: 0,
    })
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
pub unsafe extern "C" fn trueos_cabi_vgpu_device_diagnostics(
    device: u64,
    out: *mut v::vgpu::DeviceDiagnostics,
) -> i32 {
    if out.is_null() {
        return -14;
    }
    let result = if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_record(trueos_vm::vmcall::OP_BP_VGPU_DEVICE_DIAGNOSTICS, device, 0, &[])
    } else {
        broker_device_diagnostics(direct_principal(), device)
    };
    match result {
        Ok(diagnostics) => {
            unsafe { out.write(diagnostics) };
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
pub unsafe extern "C" fn trueos_cabi_vgpu_ui4_surface_acquire(
    device: u64,
    window_id: u32,
    out: *mut v::vgpu::SurfaceInfo,
) -> i32 {
    if out.is_null() {
        return -14;
    }
    let result = if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_record(
            trueos_vm::vmcall::OP_BP_VGPU_UI4_SURFACE_ACQUIRE,
            device,
            window_id as u64,
            &[],
        )
    } else {
        broker_ui4_surface_acquire(direct_principal(), device, window_id)
    };
    match result {
        Ok(info) => {
            unsafe { out.write(info) };
            0
        }
        Err(rc) => rc,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_vgpu_ui4_surface_discard(device: u64, surface: u64) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_rc(trueos_vm::vmcall::OP_BP_VGPU_UI4_SURFACE_DISCARD, device, surface, &[])
    } else {
        broker_ui4_surface_discard(direct_principal(), device, surface)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_vgpu_ui4_surface_clear_submit(
    device: u64,
    queue: u64,
    surface: u64,
    rgba8_srgb: u32,
    out_point: *mut v::vgpu::TimelinePoint,
) -> i32 {
    if out_point.is_null() {
        return -14;
    }
    let mut payload = [0u8; 12];
    payload[..8].copy_from_slice(&surface.to_le_bytes());
    payload[8..].copy_from_slice(&rgba8_srgb.to_le_bytes());
    let result = if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_record(
            trueos_vm::vmcall::OP_BP_VGPU_UI4_SURFACE_CLEAR_SUBMIT,
            device,
            queue,
            &payload,
        )
    } else {
        broker_ui4_surface_clear_submit(direct_principal(), device, queue, surface, rgba8_srgb)
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
pub unsafe extern "C" fn trueos_cabi_vgpu_cloud_work_graph_create(
    device: u64,
    descriptor: *const v::vgpu::CloudWorkGraphDescriptor,
    out_graph: *mut u64,
) -> i32 {
    if descriptor.is_null() || out_graph.is_null() {
        return -14;
    }
    let d = unsafe { descriptor.read() };
    if d.profile != v::vgpu::CLOUD_PROFILE_HELIO_ENGINE_V1 || d.flags != 0 || d.reserved != [0; 2] {
        return -95;
    }
    let result = if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let payload = unsafe {
            core::slice::from_raw_parts(
                core::ptr::from_ref(&d).cast::<u8>(),
                core::mem::size_of_val(&d),
            )
        };
        guest_handle(trueos_vm::vmcall::OP_BP_VGPU_CLOUD_WORK_GRAPH_CREATE, device, 0, payload)
    } else {
        broker_cloud_work_graph_create(direct_principal(), device, d)
    };
    match result {
        Ok(h) => {
            unsafe { out_graph.write(h) };
            0
        }
        Err(rc) => rc,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_vgpu_cloud_work_graph_destroy(device: u64, graph: u64) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_rc(trueos_vm::vmcall::OP_BP_VGPU_CLOUD_WORK_GRAPH_DESTROY, device, graph, &[])
    } else {
        broker_cloud_work_graph_destroy(direct_principal(), device, graph)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_vgpu_cloud_frame_submit(
    device: u64,
    queue: u64,
    submit: *const v::vgpu::CloudFrameSubmit,
    out: *mut v::vgpu::CloudFrameTelemetry,
) -> i32 {
    if submit.is_null() || out.is_null() {
        return -14;
    }
    let s = unsafe { submit.read() };
    if s.flags != 0
        || s.reserved != [0; 2]
        || s.simulation_steps > v::vgpu::CLOUD_FRAME_MAX_SIMULATION_STEPS
        || s.graph == 0
        || s.surface == 0
    {
        return -95;
    }
    let payload = unsafe {
        core::slice::from_raw_parts(
            core::ptr::from_ref(&s).cast::<u8>(),
            core::mem::size_of_val(&s),
        )
    };
    let result = if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_record(trueos_vm::vmcall::OP_BP_VGPU_CLOUD_FRAME_SUBMIT, device, queue, payload)
    } else {
        broker_cloud_frame_submit(
            direct_principal(),
            device,
            queue,
            s.graph,
            s.surface,
            s.simulation_steps,
        )
    };
    match result {
        Ok(t) => {
            unsafe { out.write(t) };
            0
        }
        Err(rc) => rc,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_vgpu_shader_module_create(
    device: u64,
    package_digest: u64,
    out_shader: *mut u64,
) -> i32 {
    if out_shader.is_null() {
        return -14;
    }
    let result = if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_handle(
            trueos_vm::vmcall::OP_BP_VGPU_SHADER_MODULE_CREATE,
            device,
            package_digest,
            &[],
        )
    } else {
        broker_shader_module_create(direct_principal(), device, package_digest)
    };
    match result {
        Ok(handle) => {
            unsafe { out_shader.write(handle) };
            0
        }
        Err(rc) => rc,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_vgpu_shader_module_destroy(device: u64, shader: u64) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_rc(trueos_vm::vmcall::OP_BP_VGPU_SHADER_MODULE_DESTROY, device, shader, &[])
    } else {
        broker_shader_module_destroy(direct_principal(), device, shader)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_vgpu_render_pipeline_create(
    device: u64,
    shader: u64,
    vertex_stride: u32,
    position_offset: u32,
    out_pipeline: *mut u64,
) -> i32 {
    if out_pipeline.is_null() {
        return -14;
    }
    let mut payload = [0u8; 8];
    payload[..4].copy_from_slice(&vertex_stride.to_le_bytes());
    payload[4..].copy_from_slice(&position_offset.to_le_bytes());
    let result = if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_handle(trueos_vm::vmcall::OP_BP_VGPU_RENDER_PIPELINE_CREATE, device, shader, &payload)
    } else {
        broker_render_pipeline_create(
            direct_principal(),
            device,
            shader,
            vertex_stride,
            position_offset,
        )
    };
    match result {
        Ok(handle) => {
            unsafe { out_pipeline.write(handle) };
            0
        }
        Err(rc) => rc,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_vgpu_render_pipeline_destroy(device: u64, pipeline: u64) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_rc(trueos_vm::vmcall::OP_BP_VGPU_RENDER_PIPELINE_DESTROY, device, pipeline, &[])
    } else {
        broker_render_pipeline_destroy(direct_principal(), device, pipeline)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_vgpu_ui4_indexed_submit(
    device: u64,
    queue: u64,
    draw: *const v::vgpu::IndexedDraw,
    out_point: *mut v::vgpu::TimelinePoint,
) -> i32 {
    if draw.is_null() || out_point.is_null() {
        return -14;
    }
    let draw = unsafe { draw.read() };
    let payload = unsafe {
        core::slice::from_raw_parts(
            (&draw as *const v::vgpu::IndexedDraw).cast::<u8>(),
            core::mem::size_of::<v::vgpu::IndexedDraw>(),
        )
    };
    let result = if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_record(trueos_vm::vmcall::OP_BP_VGPU_UI4_INDEXED_SUBMIT, device, queue, payload)
    } else {
        broker_ui4_indexed_submit(direct_principal(), device, queue, draw)
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
pub unsafe extern "C" fn trueos_cabi_vgpu_ui4_indexed_batch_submit(
    device: u64,
    queue: u64,
    batch: *const v::vgpu::IndexedDrawBatch,
    out_point: *mut v::vgpu::TimelinePoint,
) -> i32 {
    if batch.is_null() || out_point.is_null() {
        return -14;
    }
    let batch = unsafe { batch.read() };
    let payload = unsafe {
        core::slice::from_raw_parts(
            (&batch as *const v::vgpu::IndexedDrawBatch).cast::<u8>(),
            core::mem::size_of::<v::vgpu::IndexedDrawBatch>(),
        )
    };
    let result = if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_record(trueos_vm::vmcall::OP_BP_VGPU_UI4_INDEXED_BATCH_SUBMIT, device, queue, payload)
    } else {
        broker_ui4_indexed_batch_submit(direct_principal(), device, queue, batch)
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
pub unsafe extern "C" fn trueos_cabi_vgpu_ui4_indexed_batch_submit_v2(
    device: u64,
    queue: u64,
    batch: *const v::vgpu::IndexedDrawBatchV2,
    out_point: *mut v::vgpu::TimelinePoint,
) -> i32 {
    if batch.is_null() || out_point.is_null() {
        return -14;
    }
    let batch = unsafe { batch.read() };
    let payload = unsafe {
        core::slice::from_raw_parts(
            (&batch as *const v::vgpu::IndexedDrawBatchV2).cast::<u8>(),
            core::mem::size_of::<v::vgpu::IndexedDrawBatchV2>(),
        )
    };
    let result = if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_record(
            trueos_vm::vmcall::OP_BP_VGPU_UI4_INDEXED_BATCH_SUBMIT_V2,
            device,
            queue,
            payload,
        )
    } else {
        broker_ui4_indexed_batch_submit_v2(direct_principal(), device, queue, batch)
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
pub unsafe extern "C" fn trueos_cabi_vgpu_retained_mesh_create(
    device: u64,
    descriptor: *const v::vgpu::RetainedMeshDescriptor,
    out_mesh: *mut u64,
) -> i32 {
    if descriptor.is_null() || out_mesh.is_null() {
        return -14;
    }
    let descriptor = unsafe { descriptor.read() };
    let payload = unsafe {
        core::slice::from_raw_parts(
            (&descriptor as *const v::vgpu::RetainedMeshDescriptor).cast::<u8>(),
            core::mem::size_of::<v::vgpu::RetainedMeshDescriptor>(),
        )
    };
    let result = if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_handle(trueos_vm::vmcall::OP_BP_VGPU_RETAINED_MESH_CREATE, device, 0, payload)
    } else {
        broker_retained_mesh_create(direct_principal(), device, descriptor)
    };
    match result {
        Ok(mesh) => {
            unsafe { out_mesh.write(mesh) };
            0
        }
        Err(rc) => rc,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_vgpu_retained_mesh_destroy(device: u64, mesh: u64) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_rc(trueos_vm::vmcall::OP_BP_VGPU_RETAINED_MESH_DESTROY, device, mesh, &[])
    } else {
        broker_retained_mesh_destroy(direct_principal(), device, mesh)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_vgpu_retained_frame_submit(
    device: u64,
    queue: u64,
    submit: *const v::vgpu::RetainedFrameSubmit,
    out_point: *mut v::vgpu::TimelinePoint,
) -> i32 {
    if submit.is_null() || out_point.is_null() {
        return -14;
    }
    let submit = unsafe { submit.read() };
    let payload = unsafe {
        core::slice::from_raw_parts(
            (&submit as *const v::vgpu::RetainedFrameSubmit).cast::<u8>(),
            core::mem::size_of::<v::vgpu::RetainedFrameSubmit>(),
        )
    };
    let result = if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_record(trueos_vm::vmcall::OP_BP_VGPU_RETAINED_FRAME_SUBMIT, device, queue, payload)
    } else {
        broker_retained_frame_submit(direct_principal(), device, queue, submit)
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
    let result = if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let mut request = [0u8; 12];
        request[..8].copy_from_slice(&(bytes as u64).to_le_bytes());
        request[8..].copy_from_slice(&usage.to_le_bytes());
        guest_handle(trueos_vm::vmcall::OP_BP_VGPU_VVIDEO_CREATE, device, guest_va, &request)
    } else {
        let principal = direct_principal();
        if !matches!(principal, Principal::HullGuest(_)) {
            return -95;
        }
        broker_vvideo_create(principal, device, guest_va, bytes, usage)
    };
    match result {
        Ok(handle) => {
            unsafe { out_buffer.write(handle) };
            0
        }
        Err(rc) => rc,
    }
}

fn guest_vvideo_range(op: u32, device: u64, buffer: u64, offset: usize, bytes: usize) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let mut request = [0u8; 16];
        request[..8].copy_from_slice(&(offset as u64).to_le_bytes());
        request[8..].copy_from_slice(&(bytes as u64).to_le_bytes());
        return guest_rc(op, device, buffer, &request);
    }

    let principal = direct_principal();
    if !matches!(principal, Principal::HullGuest(_)) {
        return -95;
    }
    if op == trueos_vm::vmcall::OP_BP_VGPU_VVIDEO_FLUSH {
        broker_vvideo_flush(principal, device, buffer, offset, bytes)
    } else if op == trueos_vm::vmcall::OP_BP_VGPU_VVIDEO_INVALIDATE {
        broker_vvideo_invalidate(principal, device, buffer, offset, bytes)
    } else {
        -95
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_vgpu_vvideo_flush(
    device: u64,
    buffer: u64,
    offset: usize,
    bytes: usize,
) -> i32 {
    guest_vvideo_range(trueos_vm::vmcall::OP_BP_VGPU_VVIDEO_FLUSH, device, buffer, offset, bytes)
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
