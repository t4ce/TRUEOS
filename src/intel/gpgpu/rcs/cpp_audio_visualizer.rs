fn direct_rcs_write_cpp_audio_visualizer_payload(
    state: DirectRcsState,
    params: CppAudioVisualizerRgba8Params,
) -> bool {
    if CPP_AUDIO_VISUALIZER_PAYLOAD_OFFSET_BYTES + CPP_AUDIO_VISUALIZER_INDIRECT_BYTES
        > DIRECT_RCS_BATCH_BYTES
    {
        return false;
    }
    let Some(known) =
        super::opencl::registry::known_aot_kernel(CPP_AUDIO_VISUALIZER_RGBA8_KERNEL_NAME)
    else {
        crate::log_error!(
            target: "gpgpu";
            "intel/gpgpu: cpp-audio-visualizer payload rejected reason=missing-opencl-contract\n"
        );
        return false;
    };

    unsafe {
        let payload = state
            .batch_virt
            .add(CPP_AUDIO_VISUALIZER_PAYLOAD_OFFSET_BYTES);
        core::ptr::write_bytes(payload, 0, CPP_AUDIO_VISUALIZER_INDIRECT_BYTES);
        let dwords = payload as *mut u32;

        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        core::ptr::write_volatile(dwords.add(12), params.audio_gpu as u32);
        core::ptr::write_volatile(dwords.add(13), (params.audio_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(14), params.dst_gpu as u32);
        core::ptr::write_volatile(dwords.add(15), (params.dst_gpu >> 32) as u32);

        let cross_thread =
            core::slice::from_raw_parts_mut(payload, CPP_AUDIO_VISUALIZER_CROSS_THREAD_BYTES);
        let values = (|| {
            let mut writer = super::opencl::KernelValueWriter::new(known.contract, cross_thread)?;
            writer.set_u32(2, params.dst_pitch_bytes)?;
            writer.set_u32(3, params.dst_width)?;
            writer.set_u32(4, params.dst_height)?;
            writer.set_f32(5, params.time_seconds)?;
            writer.set_u32(6, params.frame)?;
            writer.set_u32(7, params.flags)?;
            writer.finish()?;
            Ok::<(), super::opencl::KernelValueError>(())
        })();
        if let Err(err) = values {
            crate::log_error!(
                target: "gpgpu";
                "intel/gpgpu: cpp-audio-visualizer payload rejected reason=value-contract error={:?}\n",
                err
            );
            return false;
        }

        let local_ids = payload.add(CPP_AUDIO_VISUALIZER_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_encode_cpp_audio_visualizer_rgba8_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: CppAudioVisualizerRgba8Params,
    audio_bytes: usize,
    dst_bytes: usize,
) -> bool {
    if params.dst_width == 0
        || params.dst_height == 0
        || audio_bytes != CPP_AUDIO_VISUALIZER_SNAPSHOT_BYTES
        || upload.bin_sha256 != CPP_AUDIO_VISUALIZER_RGBA8_ADLS_BIN_SHA256
        || upload.gpu != CPP_AUDIO_VISUALIZER_RGBA8_ADLS_GPU
        || upload.bytes != CPP_AUDIO_VISUALIZER_RGBA8_ADLS_BIN.len()
    {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }

    let cross_thread_grfs = CPP_AUDIO_VISUALIZER_CROSS_THREAD_BYTES.div_ceil(32) as u32;
    if !direct_rcs_write_interface_descriptor_at(
        state,
        CPP_AUDIO_VISUALIZER_IDD_OFFSET_BYTES,
        CPP_AUDIO_VISUALIZER_BINDING_TABLE_OFFSET_BYTES,
        CPP_AUDIO_VISUALIZER_RGBA8_TEXT_OFFSET_BYTES,
        2,
        cross_thread_grfs,
    ) {
        return false;
    }
    let binding_end =
        CPP_AUDIO_VISUALIZER_BINDING_TABLE_OFFSET_BYTES + 2 * core::mem::size_of::<u32>();
    if binding_end > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    unsafe {
        let binding = state
            .batch_virt
            .add(CPP_AUDIO_VISUALIZER_BINDING_TABLE_OFFSET_BYTES) as *mut u32;
        core::ptr::write_volatile(
            binding,
            CPP_AUDIO_VISUALIZER_AUDIO_SURFACE_STATE_OFFSET_BYTES as u32,
        );
        core::ptr::write_volatile(
            binding.add(1),
            CPP_AUDIO_VISUALIZER_DST_SURFACE_STATE_OFFSET_BYTES as u32,
        );
    }
    if !direct_rcs_write_buffer_surface_state(
        state,
        CPP_AUDIO_VISUALIZER_AUDIO_SURFACE_STATE_OFFSET_BYTES,
        params.audio_gpu,
        audio_bytes,
    ) || !direct_rcs_write_buffer_surface_state(
        state,
        CPP_AUDIO_VISUALIZER_DST_SURFACE_STATE_OFFSET_BYTES,
        params.dst_gpu,
        dst_bytes,
    ) || !direct_rcs_write_cpp_audio_visualizer_payload(state, params)
    {
        return false;
    }

    let pair_width = params.dst_width.div_ceil(2);
    let group_x = pair_width.div_ceil(16).max(1);
    let group_y = params.dst_height.max(1);
    let last_group_lanes = ((pair_width - 1) % 16) + 1;
    let right_mask = if last_group_lanes >= 16 {
        GPGPU_WALKER_SIMD16_MASK
    } else {
        (1u32 << last_group_lanes) - 1
    };

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
    ok &= direct_rcs_push(batch, &mut cursor, CPP_AUDIO_VISUALIZER_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, CPP_AUDIO_VISUALIZER_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        CPP_AUDIO_VISUALIZER_PRE_MARKER_SLOT,
        CPP_AUDIO_VISUALIZER_PRE_MARKER,
    );
    ok &= direct_rcs_push_gpgpu_walker_2d(
        batch,
        &mut cursor,
        CPP_AUDIO_VISUALIZER_PAYLOAD_OFFSET_BYTES,
        CPP_AUDIO_VISUALIZER_INDIRECT_BYTES,
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
        CPP_AUDIO_VISUALIZER_POST_MARKER_SLOT,
        CPP_AUDIO_VISUALIZER_POST_MARKER,
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
