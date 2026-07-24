fn direct_rcs_write_lfm25_q8_payload(
    state: DirectRcsState,
    params: Lfm25Q8ProjectParams,
) -> bool {
    if LFM25_Q8_PAYLOAD_OFFSET_BYTES + LFM25_Q8_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    let Some(known) = super::opencl::registry::known_aot_kernel(LFM25_Q8_PROJECT_KERNEL_NAME)
    else {
        return false;
    };

    unsafe {
        let payload = state.batch_virt.add(LFM25_Q8_PAYLOAD_OFFSET_BYTES);
        core::ptr::write_bytes(payload, 0, LFM25_Q8_INDIRECT_BYTES);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        let pointers = [
            params.weights_gpu,
            params.activation_gpu,
            params.output_gpu,
        ];
        for (index, pointer) in pointers.into_iter().enumerate() {
            let offset = 12 + index * 2;
            core::ptr::write_volatile(dwords.add(offset), pointer as u32);
            core::ptr::write_volatile(dwords.add(offset + 1), (pointer >> 32) as u32);
        }

        let cross_thread =
            core::slice::from_raw_parts_mut(payload, LFM25_Q8_CROSS_THREAD_BYTES);
        let values = (|| {
            let mut writer = super::opencl::KernelValueWriter::new(known.contract, cross_thread)?;
            writer.set_u32(3, params.weight_offset)?;
            writer.set_u32(4, params.columns)?;
            writer.set_u32(5, params.rows)?;
            writer.finish()?;
            Ok::<(), super::opencl::KernelValueError>(())
        })();
        if values.is_err() {
            return false;
        }

        let local_ids = payload.add(LFM25_Q8_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_encode_lfm25_q8_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: Lfm25Q8ProjectParams,
) -> bool {
    if !lfm25_q8_admitted_shape(params.columns, params.rows)
        || params.activation_bytes == 0
        || params.output_bytes != params.rows as usize * core::mem::size_of::<f32>()
        || upload.bin_sha256 != LFM25_Q8_PROJECT_ADLS_BIN_SHA256
        || upload.gpu != LFM25_Q8_PROJECT_ADLS_GPU
        || upload.bytes != LFM25_Q8_PROJECT_ADLS_BIN.len()
    {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }

    let cross_thread_grfs = LFM25_Q8_CROSS_THREAD_BYTES.div_ceil(32) as u32;
    if !direct_rcs_write_interface_descriptor_at(
        state,
        LFM25_Q8_IDD_OFFSET_BYTES,
        LFM25_Q8_BINDING_TABLE_OFFSET_BYTES,
        LFM25_Q8_PROJECT_TEXT_OFFSET_BYTES,
        3,
        cross_thread_grfs,
    ) {
        return false;
    }
    unsafe {
        let binding = state
            .batch_virt
            .add(LFM25_Q8_BINDING_TABLE_OFFSET_BYTES) as *mut u32;
        core::ptr::write_volatile(binding, LFM25_Q8_WEIGHTS_SURFACE_STATE_OFFSET_BYTES as u32);
        core::ptr::write_volatile(
            binding.add(1),
            LFM25_Q8_ACTIVATION_SURFACE_STATE_OFFSET_BYTES as u32,
        );
        core::ptr::write_volatile(
            binding.add(2),
            LFM25_Q8_OUTPUT_SURFACE_STATE_OFFSET_BYTES as u32,
        );
    }
    if !direct_rcs_write_buffer_surface_state(
        state,
        LFM25_Q8_WEIGHTS_SURFACE_STATE_OFFSET_BYTES,
        params.weights_gpu,
        params.model_bytes,
    ) || !direct_rcs_write_buffer_surface_state(
        state,
        LFM25_Q8_ACTIVATION_SURFACE_STATE_OFFSET_BYTES,
        params.activation_gpu,
        params.activation_bytes,
    ) || !direct_rcs_write_buffer_surface_state(
        state,
        LFM25_Q8_OUTPUT_SURFACE_STATE_OFFSET_BYTES,
        params.output_gpu,
        params.output_bytes,
    ) || !direct_rcs_write_lfm25_q8_payload(state, params)
    {
        return false;
    }

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
        state.gpu_va.batch,
        state.gpu_va.batch,
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
    ok &= direct_rcs_push(batch, &mut cursor, LFM25_Q8_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, LFM25_Q8_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_store_marker_at(
        batch,
        &mut cursor,
        state.gpu_va.result,
        LFM25_Q8_PRE_MARKER_SLOT,
        LFM25_Q8_PRE_MARKER,
    );
    ok &= direct_rcs_push_gpgpu_walker_2d(
        batch,
        &mut cursor,
        LFM25_Q8_PAYLOAD_OFFSET_BYTES,
        LFM25_Q8_INDIRECT_BYTES,
        params.rows / 16,
        1,
        GPGPU_WALKER_SIMD16_MASK,
    );
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_FLUSH_BITS);
    ok &= direct_rcs_push_pipe_control_post_sync_marker_at(
        batch,
        &mut cursor,
        state.gpu_va.result,
        LFM25_Q8_POST_MARKER_SLOT,
        LFM25_Q8_POST_MARKER,
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
