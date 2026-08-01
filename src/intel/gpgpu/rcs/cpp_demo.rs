fn direct_rcs_write_cpp_demo_rgba8_payload_at(
    state: DirectRcsState,
    payload_offset: usize,
    params: CppDemoRgba8Params,
) -> bool {
    if payload_offset + CPP_DEMO_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    let Some(known) = super::opencl::registry::known_aot_kernel(CPP_DEMO_RGBA8_KERNEL_NAME) else {
        crate::log_error!(
            target: "gpgpu";
            "intel/gpgpu: cpp-demo-rgba8 payload rejected reason=missing-opencl-contract\n"
        );
        return false;
    };

    unsafe {
        let payload = state.batch_virt.add(payload_offset);
        core::ptr::write_bytes(payload, 0, CPP_DEMO_INDIRECT_BYTES);
        let dwords = payload as *mut u32;

        // Generated implicit cross-thread payload: local size and enqueued
        // local size are both one SIMD16 row.
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        core::ptr::write_volatile(dwords.add(12), params.dst_gpu as u32);
        core::ptr::write_volatile(dwords.add(13), (params.dst_gpu >> 32) as u32);

        let cross_thread = core::slice::from_raw_parts_mut(payload, CPP_DEMO_CROSS_THREAD_BYTES);
        let values = (|| {
            let mut writer = super::opencl::KernelValueWriter::new(known.contract, cross_thread)?;
            writer.set_u32(1, params.dst_pitch_bytes)?;
            writer.set_u32(2, params.dst_width)?;
            writer.set_u32(3, params.dst_height)?;
            writer.set_u32(4, params.rect_x)?;
            writer.set_u32(5, params.rect_y)?;
            writer.set_u32(6, params.rect_width)?;
            writer.set_u32(7, params.rect_height)?;
            writer.set_f32(8, params.time_seconds)?;
            writer.set_u32(9, params.demo_mode)?;
            writer.set_u32(10, params.seed)?;
            writer.set_u32(11, params.flags)?;
            writer.finish()?;
            Ok::<(), super::opencl::KernelValueError>(())
        })();
        if let Err(err) = values {
            crate::log_error!(
                target: "gpgpu";
                "intel/gpgpu: cpp-demo-rgba8 payload rejected reason=value-contract error={:?}\n",
                err
            );
            return false;
        }

        let local_ids = payload.add(CPP_DEMO_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_encode_cpp_demo_rgba8_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: CppDemoRgba8Params,
    dst_bytes: usize,
) -> bool {
    if params.rect_width == 0
        || params.rect_height == 0
        || CPP_DEMO_PAYLOAD_OFFSET_BYTES + CPP_DEMO_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES
    {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }

    let cross_thread_grfs = CPP_DEMO_CROSS_THREAD_BYTES.div_ceil(32) as u32;
    if !direct_rcs_write_interface_descriptor_at(
        state,
        CPP_DEMO_IDD_OFFSET_BYTES,
        CPP_DEMO_BINDING_TABLE_OFFSET_BYTES,
        CPP_DEMO_RGBA8_TEXT_OFFSET_BYTES,
        1,
        cross_thread_grfs,
    ) {
        return false;
    }
    let binding_end = CPP_DEMO_BINDING_TABLE_OFFSET_BYTES + core::mem::size_of::<u32>();
    if binding_end > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    unsafe {
        let binding = state.batch_virt.add(CPP_DEMO_BINDING_TABLE_OFFSET_BYTES) as *mut u32;
        core::ptr::write_volatile(binding, CPP_DEMO_DST_SURFACE_STATE_OFFSET_BYTES as u32);
    }
    if !direct_rcs_write_buffer_surface_state(
        state,
        CPP_DEMO_DST_SURFACE_STATE_OFFSET_BYTES,
        params.dst_gpu,
        dst_bytes,
    ) || !direct_rcs_write_cpp_demo_rgba8_payload_at(
        state,
        CPP_DEMO_PAYLOAD_OFFSET_BYTES,
        params,
    ) {
        return false;
    }

    let batch_len = DIRECT_RCS_BATCH_BYTES / core::mem::size_of::<u32>();
    let batch = unsafe { core::slice::from_raw_parts_mut(state.batch_virt as *mut u32, batch_len) };
    let mut cursor = 0usize;
    let mut ok = true;
    let group_x = params.rect_width.div_ceil(16).max(1);
    let group_y = params.rect_height.max(1);
    let last_group_pixels = ((params.rect_width - 1) % 16) + 1;
    let right_mask = if last_group_pixels >= 16 {
        GPGPU_WALKER_SIMD16_MASK
    } else {
        (1u32 << last_group_pixels) - 1
    };

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
    ok &= direct_rcs_push(batch, &mut cursor, CPP_DEMO_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, CPP_DEMO_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        CPP_DEMO_PRE_MARKER_SLOT,
        CPP_DEMO_PRE_MARKER,
    );
    ok &= direct_rcs_push_gpgpu_walker_2d(
        batch,
        &mut cursor,
        CPP_DEMO_PAYLOAD_OFFSET_BYTES,
        CPP_DEMO_INDIRECT_BYTES,
        group_x,
        group_y,
        right_mask,
    );
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_FLUSH_BITS);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        CPP_DEMO_POST_MARKER_SLOT,
        CPP_DEMO_POST_MARKER,
    );
    ok &= direct_rcs_push(batch, &mut cursor, MI_BATCH_BUFFER_END);
    ok &= direct_rcs_push(batch, &mut cursor, MI_NOOP);
    if !ok {
        return false;
    }

    super::dma_flush(state.batch_virt, DIRECT_RCS_BATCH_BYTES);
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    true
}
