// One WC3 frame: CPU-prepared geometry, native GPU raster, one UI4 release.
// This is deliberately a fixed shader contract, not a general GL API.

struct PreparedRasterDecoded {
    vertices: Vec<[f32; 16]>,
    indices: Vec<u32>,
    state: [f32; 384],
    texture: Arc<crate::intel::render::ResidentSampledTexture>,
    flags: u32,
    clear_rgba8: u32,
}

const _: () = assert!(
    v::vgpu::MAX_PREPARED_RASTER_SUBMIT_DRAWS
        <= crate::intel::render::RESIDENT_SCENE_MAX_DRAWS
);

fn prepared_raster_texture(
    device: &mut VirtualDevice,
    desc: &v::vgpu::PreparedRasterDrawV1,
) -> Result<Arc<crate::intel::render::ResidentSampledTexture>, VgpuError> {
    let handle = BufferHandle::from_raw(desc.texture);
    let shape = [desc.texture_width, desc.texture_height, desc.texture_pitch, desc.sampler_flags];
    if let Some(resident) = lookup_buffer(device, handle)?.sampled.as_ref()
        .filter(|cache| cache.shape == shape)
        .map(|cache| Arc::clone(&cache.resident)) {
        return Ok(resident);
    }
    let old_bytes = release_sampled_buffer(lookup_buffer_mut(device, handle)?)?;
    device.memory_used = device.memory_used.saturating_sub(old_bytes);
    let byte_count = usize::try_from(u64::from(desc.texture_pitch) * u64::from(desc.texture_height))
        .map_err(|_| VgpuError::Unsupported)?;
    let record = lookup_buffer(device, handle)?;
    if record.usage & BUFFER_USAGE_MAP_WRITE == 0
        || desc.texture_width == 0 || desc.texture_height == 0
        || desc.texture_pitch < desc.texture_width.saturating_mul(4)
        || desc.texture_pitch % 4 != 0 || byte_count > record.bytes
        || desc.sampler_flags != SAMPLER_ADDRESS_U_REPEAT | SAMPLER_ADDRESS_V_REPEAT
        || device.memory_used.saturating_add(byte_count) > device.quota.memory_bytes {
        return Err(VgpuError::Unsupported);
    }
    let virt = match record.backing {
        BufferBacking::Dma { virt, .. } => virt,
        BufferBacking::GuestPages { .. } => return Err(VgpuError::Unsupported),
    };
    crate::intel::dma_flush(virt, byte_count);
    let bytes = unsafe { core::slice::from_raw_parts(virt, byte_count) };
    let resident = Arc::new(crate::intel::render::create_resident_sampled_rgba8_texture(
        desc.texture_width, desc.texture_height, desc.texture_pitch,
        desc.sampler_flags, bytes,
    ).map_err(|_| VgpuError::OutOfMemory)?);
    lookup_buffer_mut(device, handle)?.sampled = Some(SampledBufferCache {
        shape, bytes: byte_count, resident: Arc::clone(&resident),
    });
    device.memory_used += byte_count;
    Ok(resident)
}

fn prepared_raster_decode(
    device: &mut VirtualDevice,
    batch: &v::vgpu::PreparedRasterBatchV1,
    desc: &v::vgpu::PreparedRasterDrawV1,
    width: u32,
    height: u32,
) -> Result<PreparedRasterDecoded, VgpuError> {
    if desc.reserved != 0 || desc.index_count == 0 || desc.index_count % 3 != 0
        || desc.texture == 0 || !v::vgpu::indexed_draw_flags_valid(desc.flags) {
        return Err(VgpuError::Unsupported);
    }
    let index_count = desc.index_count as usize;
    let index_offset = usize::try_from(desc.index_offset).map_err(|_| VgpuError::Unsupported)?;
    let index_end = index_offset.checked_add(index_count.checked_mul(4).ok_or(VgpuError::Unsupported)?)
        .ok_or(VgpuError::Unsupported)?;
    let index_record = lookup_buffer(device, BufferHandle::from_raw(batch.index_buffer))?;
    if index_record.usage & BUFFER_USAGE_INDEX == 0 || index_end > index_record.bytes {
        return Err(VgpuError::Unsupported);
    }
    let index_virt = match index_record.backing {
        BufferBacking::Dma { virt, .. } => virt,
        BufferBacking::GuestPages { .. } => return Err(VgpuError::Unsupported),
    };
    crate::intel::dma_flush(unsafe { index_virt.add(index_offset) }, index_end-index_offset);
    let mut indices = Vec::with_capacity(index_count);
    for n in 0..index_count {
        let raw = unsafe { core::slice::from_raw_parts(index_virt.add(index_offset+n*4), 4) };
        indices.push(u32::from_le_bytes(raw.try_into().unwrap()));
    }
    let vertex_count = indices.iter().copied().max().ok_or(VgpuError::Unsupported)? as usize + 1;
    let vertex_offset = usize::try_from(desc.vertex_offset).map_err(|_| VgpuError::Unsupported)?;
    let state_offset = usize::try_from(desc.state_offset).map_err(|_| VgpuError::Unsupported)?;
    let vertex_end = vertex_offset.checked_add(vertex_count.checked_mul(64).ok_or(VgpuError::Unsupported)?)
        .ok_or(VgpuError::Unsupported)?;
    let state_end = state_offset.checked_add(v::vgpu::WC3_FIXED_STATE_BYTES)
        .ok_or(VgpuError::Unsupported)?;
    let vertex_record = lookup_buffer(device, BufferHandle::from_raw(batch.vertex_buffer))?;
    if vertex_record.usage & BUFFER_USAGE_VERTEX == 0
        || vertex_end > vertex_record.bytes || state_end > vertex_record.bytes {
        return Err(VgpuError::Unsupported);
    }
    let vertex_virt = match vertex_record.backing {
        BufferBacking::Dma { virt, .. } => virt,
        BufferBacking::GuestPages { .. } => return Err(VgpuError::Unsupported),
    };
    crate::intel::dma_flush(unsafe { vertex_virt.add(vertex_offset) }, vertex_end-vertex_offset);
    crate::intel::dma_flush(unsafe { vertex_virt.add(state_offset) }, state_end-state_offset);
    let mut state = [0f32; 384];
    for (i, value) in state.iter_mut().enumerate() {
        let raw = unsafe { core::slice::from_raw_parts(vertex_virt.add(state_offset+i*4), 4) };
        *value = f32::from_le_bytes(raw.try_into().unwrap());
    }
    if state[360] != 1.0 || !fixed_gl_state_valid(&state, width, height)
        || !fixed_gl_texture_state_valid(&state, desc.texture_width, desc.texture_height) {
        return Err(VgpuError::Unsupported);
    }
    let mut vertices = Vec::with_capacity(vertex_count);
    for n in 0..vertex_count {
        let mut vertex = [0f32; 16];
        for (i, value) in vertex.iter_mut().enumerate() {
            let raw = unsafe { core::slice::from_raw_parts(vertex_virt.add(vertex_offset+n*64+i*4), 4) };
            *value = f32::from_le_bytes(raw.try_into().unwrap());
        }
        vertices.push(vertex);
    }
    if vertices.iter().flatten().any(|v| !v.is_finite()) { return Err(VgpuError::Unsupported); }
    let texture = prepared_raster_texture(device, desc)?;
    Ok(PreparedRasterDecoded {
        vertices, indices, state, texture,
        flags: desc.flags, clear_rgba8: desc.clear_rgba8,
    })
}

pub(crate) fn submit_ui4_prepared_raster_batch_v1(
    principal: Principal,
    device_handle: DeviceHandle,
    queue_handle: QueueHandle,
    batch: v::vgpu::PreparedRasterBatchV1,
) -> Result<Ui4SurfaceIndexedCompletion, VgpuError> {
    let timing_start = crate::chronos::monotonic_nanos();
    let count = batch.draw_count as usize;
    if count == 0 || count > v::vgpu::MAX_PREPARED_RASTER_SUBMIT_DRAWS || batch.reserved != 0
        || batch.draws[count..].iter().any(|draw| *draw != v::vgpu::PreparedRasterDrawV1::default()) {
        return Err(VgpuError::Unsupported);
    }
    let surface_handle = SurfaceHandle::from_raw(batch.surface);
    let (window_id, phys, producer_gpu, bytes, width, height, pitch, decoded, depth) = {
        let mut broker = BROKER.lock();
        let device = lookup_device_mut(&mut broker, device_handle, principal)?;
        ensure_live(device)?;
        if device.picasso_setup_in_flight || !device.capabilities.contains(Capabilities::RENDER)
            || !device.capabilities.contains(Capabilities::PRESENT) { return Err(VgpuError::PermissionDenied); }
        let pipeline = lookup_render_pipeline(device, RenderPipelineHandle::from_raw(batch.pipeline))?;
        if pipeline.epoch != device.epoch
            || pipeline.package_digest != v::vgpu::SHADER_PACKAGE_WC3_FIXED_FNV1A64
            || pipeline.vertex_stride != 64 || pipeline.position_offset != 0 {
            return Err(VgpuError::InvalidHandle);
        }
        let queue = lookup_queue_mut(device, queue_handle)?;
        if queue.class != QueueClass::Render || queue.in_flight != 0 { return Err(VgpuError::Busy); }
        let epoch = device.epoch;
        let surface = lookup_surface_mut(device, surface_handle)?;
        if surface.epoch != epoch || surface.in_flight != 1 { return Err(VgpuError::Busy); }
        let (window_id, phys, producer_gpu, bytes, width, height, pitch) =
            (surface.window_id, surface.phys, surface.producer_gpu, surface.bytes,
             surface.width, surface.height, surface.pitch);
        // Do all descriptor bounds checks and snapshots before taking a GPU lease.
        let mut decoded = Vec::with_capacity(count);
        for desc in &batch.draws[..count] {
            decoded.push(prepared_raster_decode(device, &batch, desc, width, height)?);
        }
        let existing = device.drawable_depths.iter().position(|(id, _)| *id == window_id);
        if let Some(index) = existing {
            if !device.drawable_depths[index].1.matches(width, height) {
                if Arc::strong_count(&device.drawable_depths[index].1) != 1 { return Err(VgpuError::Busy); }
                if !crate::intel::render::release_drawable_depth(&device.drawable_depths[index].1) {
                    device.lost = true;
                    return Err(VgpuError::DeviceLost);
                }
                device.memory_used = device.memory_used.saturating_sub(device.drawable_depths[index].1.bytes());
                device.drawable_depths.swap_remove(index);
            }
        }
        let depth = if let Some((_, depth)) = device.drawable_depths.iter().find(|(id, _)| *id == window_id) {
            Arc::clone(depth)
        } else {
            let depth_bytes = crate::intel::render::drawable_depth_bytes(width, height).ok_or(VgpuError::Unsupported)?;
            if device.drawable_depths.len() >= 16 || device.memory_used.saturating_add(depth_bytes) > device.quota.memory_bytes {
                return Err(VgpuError::QuotaExceeded);
            }
            let depth = Arc::new(crate::intel::render::create_drawable_depth(width, height)
                .map_err(|_| VgpuError::OutOfMemory)?);
            device.memory_used += depth.bytes();
            device.drawable_depths.push((window_id, Arc::clone(&depth)));
            depth
        };
        lookup_queue_mut(device, queue_handle)?.in_flight = 1;
        lookup_surface_mut(device, surface_handle)?.in_flight = 2;
        (window_id, phys, producer_gpu, bytes, width, height, pitch, decoded, depth)
    };
    let timing_decoded = crate::chronos::monotonic_nanos();
    let staged_bytes = decoded.iter().map(|draw|
        draw.vertices.len() * 64 + draw.indices.len() * 4 + v::vgpu::WC3_FIXED_STATE_BYTES
    ).sum::<usize>();
    let destination = crate::intel::gpgpu::GpgpuRgba8Surface::new(
        phys, producer_gpu, bytes, width, height, pitch,
    ).ok_or(VgpuError::Unsupported)?;
    let mut meshes = Vec::with_capacity(decoded.len());
    for draw in &decoded {
        match crate::intel::render::create_resident_fixed_gl_mesh(&draw.vertices, &draw.indices, &draw.state) {
            Ok(mesh) => meshes.push(mesh),
            Err(_) => {
                for mesh in &meshes { let _ = crate::intel::render::release_resident_triangle_mesh(mesh); }
                rollback_indexed_submission_lease(principal, device_handle, queue_handle, surface_handle);
                return Err(VgpuError::OutOfMemory);
            }
        }
    }
    let draws = decoded.iter().zip(&meshes).map(|(draw, mesh)| {
        let state = &draw.state;
        crate::intel::render::ResidentSceneDraw {
            mesh,
            depth_flags: Some(draw.flags),
            rgba: if draw.flags & v::vgpu::INDEXED_DRAW_GEOMETRY_CLEAR != 0 {
                if draw.flags & v::vgpu::INDEXED_DRAW_LOAD_COLOR != 0 { [0; 4] }
                else { draw.clear_rgba8.to_le_bytes() }
            } else { SHADER_PACKAGE_CLIP_POSITION3_RGBA_COLOR.to_le_bytes() },
            sampled_texture: Some(draw.texture.as_ref()),
            fragment_contract: crate::intel::render::ResidentSceneFragmentContract::FixedGl([
                state[116] as u32, state[117] as u32, state[118] as u32, state[119] as u32,
                state[114] as u32, state[352] as u32, state[353] as u32, state[354] as u32,
                state[356] as u32, state[357] as u32,
            ]),
            viewport_translation_px: [0.0, 0.0],
            topology: crate::intel::render::ResidentScenePrimitiveTopology::TriangleList,
            point_width_px: 0,
        }
    }).collect::<Vec<_>>();
    let timing_mesh = crate::chronos::monotonic_nanos();
    let rendered = crate::intel::render::render_drawable_depth_scene(
        &draws, None, destination, Some(depth.as_ref()),
        v::vgpu::INDEXED_DRAW_DRAWABLE_DEPTH | v::vgpu::INDEXED_DRAW_LOAD_COLOR,
        false,
    );
    let timing_rendered = crate::chronos::monotonic_nanos();
    let transient_busy = matches!(rendered.as_ref().err(), Some(&"render-busy" | &"render-storage-busy"));
    let proven_release = rendered.as_ref().ok().and_then(|result| result.release_fence
        .filter(|release| result.frame_complete && result.completed_draws == count
            && result.requested_draws == count && !result.present_copy_performed
            && release.matches(phys, bytes)));
    if transient_busy {
        if meshes.iter().all(crate::intel::render::release_resident_triangle_mesh) {
            rollback_indexed_submission_lease(principal, device_handle, queue_handle, surface_handle);
            return Err(VgpuError::Busy);
        }
    }
    if proven_release.is_none() {
        // GPU completion was not established. Keep all referenced allocations
        // pinned and quarantine the owner rather than recycle them.
        core::mem::forget(meshes);
        core::mem::forget(decoded);
        let mut broker = BROKER.lock();
        if let Ok(device) = lookup_device_mut(&mut broker, device_handle, principal) { device.lost = true; }
        return Err(VgpuError::DeviceLost);
    }
    let release = proven_release.unwrap();
    if !meshes.iter().all(crate::intel::render::release_resident_triangle_mesh) {
        core::mem::forget(meshes);
        core::mem::forget(decoded);
        let mut broker = BROKER.lock();
        if let Ok(device) = lookup_device_mut(&mut broker, device_handle, principal) { device.lost = true; }
        return Err(VgpuError::DeviceLost);
    }
    let rendered = rendered.unwrap();
    depth.mark_initialized();
    let physical = require_physical()?;
    let mut broker = BROKER.lock();
    let device = lookup_device_mut(&mut broker, device_handle, principal)?;
    ensure_live(device)?;
    let vm = match device.gpuvm { GpuVmBinding::Owned(vm) => vm, _ => return Err(VgpuError::DeviceLost) };
    let (slot, generation) = decode_handle(surface_handle.raw())?;
    let surface_slot = device.surfaces.get_mut(slot).ok_or(VgpuError::InvalidHandle)?;
    if surface_slot.generation != generation || surface_slot.record.as_ref().is_none_or(|r| r.in_flight != 2) {
        return Err(VgpuError::DeviceLost);
    }
    let guest_gpu = surface_slot.record.as_ref().unwrap().gpu;
    physical.unmap_gpuvm(vm, guest_gpu, bytes)?;
    surface_slot.record.take();
    device.memory_used = device.memory_used.saturating_sub(bytes);
    let queue = lookup_queue_mut(device, queue_handle)?;
    queue.in_flight = 0;
    queue.timeline.submitted = queue.timeline.submitted.wrapping_add(1).max(1);
    queue.timeline.completed = queue.timeline.submitted;
    queue.timeline.last_physical_serial = release.sequence();
    let point = TimelinePoint { queue: queue_handle, value: queue.timeline.submitted,
        physical_serial: release.sequence(), physical_publish_sequence: release.sequence() };
    drop(broker);
    let timing_done = crate::chronos::monotonic_nanos();
    static REPORTS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
    let report = REPORTS.fetch_add(1, core::sync::atomic::Ordering::Relaxed) + 1;
    if report <= 8 || report.is_multiple_of(16) {
        crate::log_important!(target: "vgpu";
            "XPAPP GPU RASTER BATCH seq={} draws={} staged_bytes={} decode_texture_us={} mesh_us={} render_us={} gpu_poll_us={} retire_us={} total_us={} target={}x{} release={}\n",
            report, count, staged_bytes,
            timing_decoded.saturating_sub(timing_start)/1000,
            timing_mesh.saturating_sub(timing_decoded)/1000,
            timing_rendered.saturating_sub(timing_mesh)/1000,
            rendered.gpu_poll_us,
            timing_done.saturating_sub(timing_rendered)/1000,
            timing_done.saturating_sub(timing_start)/1000,
            width, height, release.sequence());
    }
    Ok(Ui4SurfaceIndexedCompletion {
        window_id,
        surface: SurfaceInfo { handle: surface_handle, bytes, width, height, pitch },
        release,
        point,
    })
}
