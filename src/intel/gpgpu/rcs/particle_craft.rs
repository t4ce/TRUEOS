fn direct_rcs_write_particle_payload(
    state: DirectRcsState,
    offset: usize,
    cross_thread_bytes: usize,
    indirect_bytes: usize,
    pointers: &[u64],
) -> bool {
    if offset.saturating_add(indirect_bytes) > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    unsafe {
        let payload = state.batch_virt.add(offset);
        core::ptr::write_bytes(payload, 0, indirect_bytes);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        for (index, pointer) in pointers.iter().copied().enumerate() {
            let pointer_dword = 12 + index * 2;
            core::ptr::write_volatile(dwords.add(pointer_dword), pointer as u32);
            core::ptr::write_volatile(dwords.add(pointer_dword + 1), (pointer >> 32) as u32);
        }
        let local_ids = payload.add(cross_thread_bytes) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_write_particle_binding_table(
    state: DirectRcsState,
    offset: usize,
    surfaces: &[usize],
) -> bool {
    if offset.saturating_add(surfaces.len() * core::mem::size_of::<u32>()) > DIRECT_RCS_BATCH_BYTES
    {
        return false;
    }
    unsafe {
        let binding = state.batch_virt.add(offset) as *mut u32;
        for (index, surface) in surfaces.iter().copied().enumerate() {
            core::ptr::write_volatile(binding.add(index), surface as u32);
        }
    }
    true
}

fn direct_rcs_encode_particle_craft_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    craft: &GpgpuOwnedParticleCraftState,
    dst: GpgpuRgba8Surface,
    active_count: u32,
) -> bool {
    if upload.bin_sha256 != PARTICLE_CRAFT_ADLS_BIN_SHA256
        || upload.gpu != PARTICLE_CRAFT_ADLS_GPU
        || upload.bytes != PARTICLE_CRAFT_ADLS_BIN.len()
        || active_count == 0
        || active_count > PARTICLE_CRAFT_MAX_PARTICLES
    {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }

    let step_grfs = PARTICLE_CRAFT_STEP_CROSS_THREAD_BYTES.div_ceil(32) as u32;
    let render_grfs = PARTICLE_CRAFT_RENDER_CROSS_THREAD_BYTES.div_ceil(32) as u32;
    if !direct_rcs_write_interface_descriptor_at(
        state,
        PARTICLE_CRAFT_STEP_IDD_OFFSET_BYTES,
        PARTICLE_CRAFT_STEP_BINDING_TABLE_OFFSET_BYTES,
        PARTICLE_CRAFT_STEP_TEXT_OFFSET_BYTES,
        2,
        step_grfs,
    ) || !direct_rcs_write_interface_descriptor_at(
        state,
        PARTICLE_CRAFT_RENDER_IDD_OFFSET_BYTES,
        PARTICLE_CRAFT_RENDER_BINDING_TABLE_OFFSET_BYTES,
        PARTICLE_CRAFT_RENDER_RGBA8_TEXT_OFFSET_BYTES,
        3,
        render_grfs,
    ) {
        return false;
    }
    if !direct_rcs_write_particle_binding_table(
        state,
        PARTICLE_CRAFT_STEP_BINDING_TABLE_OFFSET_BYTES,
        &[
            PARTICLE_CRAFT_STEP_STATE_SURFACE_OFFSET_BYTES,
            PARTICLE_CRAFT_STEP_PARAMS_SURFACE_OFFSET_BYTES,
        ],
    ) || !direct_rcs_write_particle_binding_table(
        state,
        PARTICLE_CRAFT_RENDER_BINDING_TABLE_OFFSET_BYTES,
        &[
            PARTICLE_CRAFT_RENDER_STATE_SURFACE_OFFSET_BYTES,
            PARTICLE_CRAFT_RENDER_PARAMS_SURFACE_OFFSET_BYTES,
            PARTICLE_CRAFT_RENDER_DST_SURFACE_OFFSET_BYTES,
        ],
    ) {
        return false;
    }
    if !direct_rcs_write_buffer_surface_state(
        state,
        PARTICLE_CRAFT_STEP_STATE_SURFACE_OFFSET_BYTES,
        craft.state_gpu(),
        PARTICLE_CRAFT_STATE_BYTES,
    ) || !direct_rcs_write_buffer_surface_state(
        state,
        PARTICLE_CRAFT_STEP_PARAMS_SURFACE_OFFSET_BYTES,
        craft.params_gpu(),
        PARTICLE_CRAFT_PARAMS_BYTES,
    ) || !direct_rcs_write_buffer_surface_state(
        state,
        PARTICLE_CRAFT_RENDER_STATE_SURFACE_OFFSET_BYTES,
        craft.state_gpu(),
        PARTICLE_CRAFT_STATE_BYTES,
    ) || !direct_rcs_write_buffer_surface_state(
        state,
        PARTICLE_CRAFT_RENDER_PARAMS_SURFACE_OFFSET_BYTES,
        craft.params_gpu(),
        PARTICLE_CRAFT_PARAMS_BYTES,
    ) || !direct_rcs_write_buffer_surface_state(
        state,
        PARTICLE_CRAFT_RENDER_DST_SURFACE_OFFSET_BYTES,
        dst.gpu,
        dst.bytes,
    ) || !direct_rcs_write_particle_payload(
        state,
        PARTICLE_CRAFT_STEP_PAYLOAD_OFFSET_BYTES,
        PARTICLE_CRAFT_STEP_CROSS_THREAD_BYTES,
        PARTICLE_CRAFT_STEP_INDIRECT_BYTES,
        &[craft.state_gpu(), craft.params_gpu()],
    ) || !direct_rcs_write_particle_payload(
        state,
        PARTICLE_CRAFT_RENDER_PAYLOAD_OFFSET_BYTES,
        PARTICLE_CRAFT_RENDER_CROSS_THREAD_BYTES,
        PARTICLE_CRAFT_RENDER_INDIRECT_BYTES,
        &[craft.state_gpu(), craft.params_gpu(), dst.gpu],
    ) {
        return false;
    }

    let step_groups_x = active_count.div_ceil(16).max(1);
    let step_lanes = ((active_count - 1) % 16) + 1;
    let step_right_mask = if step_lanes == 16 {
        GPGPU_WALKER_SIMD16_MASK
    } else {
        (1u32 << step_lanes) - 1
    };
    let (sample_width, sample_height) = particle_craft_sample_extent(dst.width, dst.height);
    let render_groups_x = sample_width.div_ceil(16);

    let batch_len = DIRECT_RCS_BATCH_BYTES / core::mem::size_of::<u32>();
    let batch = unsafe { core::slice::from_raw_parts_mut(state.batch_virt as *mut u32, batch_len) };
    let mut cursor = 0usize;
    let mut ok = true;
    ok &= direct_rcs_push_pipe_control_full(
        batch,
        &mut cursor,
        (1 << 9) | (1 << 11),
        PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL | 1,
    );
    ok &= direct_rcs_push(batch, &mut cursor, PIPELINE_SELECT_GPGPU);
    ok &= direct_rcs_push_pipe_control_full(batch, &mut cursor, 1 << 9, PIPE_CONTROL_CS_STALL);
    ok &= direct_rcs_push(batch, &mut cursor, PIPELINE_SELECT_3D);
    ok &= direct_rcs_push_pipe_control_full(
        batch,
        &mut cursor,
        (1 << 9) | (1 << 11),
        PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL,
    );
    ok &= direct_rcs_push_state_base_address(
        batch,
        &mut cursor,
        DIRECT_RCS_GPU_VA_BATCH_BASE,
        DIRECT_RCS_GPU_VA_BATCH_BASE,
        upload.gpu,
    );
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_INVALIDATE_BITS);
    ok &= direct_rcs_push(batch, &mut cursor, PIPELINE_SELECT_GPGPU);
    ok &= direct_rcs_push_pipe_control_full(batch, &mut cursor, 1 << 9, PIPE_CONTROL_CS_STALL);
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_VFE_STATE_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, GPGPU_VFE_DW3_UOS);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, GPGPU_VFE_DW5_UOS);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);

    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, PARTICLE_CRAFT_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, PARTICLE_CRAFT_STEP_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        PARTICLE_CRAFT_PRE_MARKER_SLOT,
        PARTICLE_CRAFT_PRE_MARKER,
    );
    ok &= direct_rcs_push_gpgpu_walker_2d(
        batch,
        &mut cursor,
        PARTICLE_CRAFT_STEP_PAYLOAD_OFFSET_BYTES,
        PARTICLE_CRAFT_STEP_INDIRECT_BYTES,
        step_groups_x,
        1,
        step_right_mask,
    );

    // The state walker writes particle records which the pixel-gather walker
    // immediately consumes. Flush and invalidate at this explicit phase edge.
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_INVALIDATE_BITS);
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, PARTICLE_CRAFT_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, PARTICLE_CRAFT_RENDER_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_gpgpu_walker_2d(
        batch,
        &mut cursor,
        PARTICLE_CRAFT_RENDER_PAYLOAD_OFFSET_BYTES,
        PARTICLE_CRAFT_RENDER_INDIRECT_BYTES,
        render_groups_x,
        sample_height,
        GPGPU_WALKER_SIMD16_MASK,
    );
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_FLUSH_BITS);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        PARTICLE_CRAFT_POST_MARKER_SLOT,
        PARTICLE_CRAFT_POST_MARKER,
    );
    ok &= direct_rcs_push(batch, &mut cursor, MI_BATCH_BUFFER_END);
    ok &= direct_rcs_push(batch, &mut cursor, MI_NOOP);
    if !ok || cursor * core::mem::size_of::<u32>() >= PARTICLE_CRAFT_STEP_IDD_OFFSET_BYTES {
        return false;
    }
    super::dma_flush(state.batch_virt, DIRECT_RCS_BATCH_BYTES);
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    true
}

fn submit_particle_craft_rgba8(
    craft: &GpgpuOwnedParticleCraftState,
    dst: GpgpuRgba8Surface,
    active_count: u32,
) -> DirectRcsDispatchOutcome {
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        return DirectRcsDispatchOutcome::default();
    };
    let Some(upload) = upload_particle_craft_kernel() else {
        return DirectRcsDispatchOutcome::default();
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        return DirectRcsDispatchOutcome::default();
    };

    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let craft_ok = kernel_ok
        && direct_rcs_map_ppgtt_kernel(state, craft.state_gpu(), craft.state_phys(), craft.bytes());
    let dst_ok = craft_ok && direct_rcs_map_ppgtt_scanout(state, dst.gpu, dst.phys, dst.bytes);
    let batch_ok =
        dst_ok && direct_rcs_encode_particle_craft_batch(state, upload, craft, dst, active_count);
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result_slot_timeout_ms(
            state,
            PARTICLE_CRAFT_POST_MARKER_SLOT,
            PARTICLE_CRAFT_POST_MARKER,
            PARTICLE_CRAFT_COMPLETION_TIMEOUT_MS,
        )
    } else {
        0
    };
    if submitted && observed != PARTICLE_CRAFT_POST_MARKER {
        quarantine_direct_rcs_context("particle-craft-marker-timeout");
    }
    if observed != PARTICLE_CRAFT_POST_MARKER {
        let (sample_width, sample_height) = particle_craft_sample_extent(dst.width, dst.height);
        crate::log_error!(
            target: "gpgpu";
            "intel/gpgpu: ParticleCraft failed forcewake={} mapped={} ppgtt={} kernel={} craft={} dst={} batch={} submitted={} observed=0x{:08X} want=0x{:08X} particles={} samples={}x{} render_divisor={} artifact={} state_gpu=0x{:X} dst_gpu=0x{:X}\n",
            forcewake_ok as u8,
            mapped_ok as u8,
            ppgtt_ok as u8,
            kernel_ok as u8,
            craft_ok as u8,
            dst_ok as u8,
            batch_ok as u8,
            submitted as u8,
            observed,
            PARTICLE_CRAFT_POST_MARKER,
            active_count,
            sample_width,
            sample_height,
            PARTICLE_CRAFT_RENDER_DIVISOR,
            PARTICLE_CRAFT_ADLS_ARTIFACT.name,
            craft.state_gpu(),
            dst.gpu,
        );
    }
    DirectRcsDispatchOutcome {
        submitted,
        observed,
    }
}
