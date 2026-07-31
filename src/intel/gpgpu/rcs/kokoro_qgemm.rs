fn kokoro_qgemm_upload_valid(upload: UploadedKernelArtifact) -> bool {
    upload.name == KOKORO_QGEMM_U8_I8_KERNEL_NAME
        && upload.bin_sha256 == KOKORO_QGEMM_U8_I8_ADLS_BIN_SHA256
        && upload.gpu == KOKORO_QGEMM_U8_I8_ADLS_GPU
        && upload.bytes == KOKORO_QGEMM_U8_I8_ADLS_BIN.len()
}

fn direct_rcs_write_kokoro_qgemm_payload(state: DirectRcsState, params: KokoroQgemmParams) -> bool {
    if KOKORO_QGEMM_PAYLOAD_OFFSET_BYTES + KOKORO_QGEMM_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    let Some(known) = super::opencl::registry::known_aot_kernel(KOKORO_QGEMM_U8_I8_KERNEL_NAME)
    else {
        crate::log_error!(
            target: "gpgpu";
            "intel/gpgpu: kokoro-qgemm payload rejected reason=missing-opencl-contract\n"
        );
        return false;
    };

    unsafe {
        let payload = state.batch_virt.add(KOKORO_QGEMM_PAYLOAD_OFFSET_BYTES);
        core::ptr::write_bytes(payload, 0, KOKORO_QGEMM_INDIRECT_BYTES);
        let dwords = payload as *mut u32;

        // One 16-lane row per workgroup. The second NDRange dimension is the
        // matrix row and therefore has local/enqueued-local size one.
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);

        let pointers = [
            params.packed_weights_gpu,
            params.weight_sums_gpu,
            params.weight_scales_gpu,
            params.activations_gpu,
            params.bias_gpu,
            params.output_gpu,
        ];
        for (index, pointer) in pointers.into_iter().enumerate() {
            let offset = 12 + index * 2;
            core::ptr::write_volatile(dwords.add(offset), pointer as u32);
            core::ptr::write_volatile(dwords.add(offset + 1), (pointer >> 32) as u32);
        }

        let cross_thread =
            core::slice::from_raw_parts_mut(payload, KOKORO_QGEMM_CROSS_THREAD_BYTES);
        let values = (|| {
            let mut writer = super::opencl::KernelValueWriter::new(known.contract, cross_thread)?;
            writer.set_u32(6, params.matrix_rows)?;
            writer.set_u32(7, params.output_columns)?;
            writer.set_u32(8, params.reduction_words)?;
            writer.set_u32(9, params.activation_stride_words)?;
            writer.set_u32(10, params.output_stride)?;
            writer.set_u32(11, params.activation_zero_point)?;
            writer.set_f32(12, params.activation_scale)?;
            writer.set_u32(13, params.has_bias)?;
            writer.finish()?;
            Ok::<(), super::opencl::KernelValueError>(())
        })();
        if let Err(error) = values {
            crate::log_error!(
                target: "gpgpu";
                "intel/gpgpu: kokoro-qgemm payload rejected reason=value-contract error={:?}\n",
                error,
            );
            return false;
        }

        let local_ids = payload.add(KOKORO_QGEMM_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_write_kokoro_qgemm_state(state: DirectRcsState, params: KokoroQgemmParams) -> bool {
    let cross_thread_grfs = KOKORO_QGEMM_CROSS_THREAD_BYTES.div_ceil(32) as u32;
    if !direct_rcs_write_interface_descriptor_at(
        state,
        KOKORO_QGEMM_IDD_OFFSET_BYTES,
        KOKORO_QGEMM_BINDING_TABLE_OFFSET_BYTES,
        KOKORO_QGEMM_U8_I8_TEXT_OFFSET_BYTES,
        6,
        cross_thread_grfs,
    ) {
        return false;
    }

    let surface_offsets = [
        KOKORO_QGEMM_PACKED_WEIGHTS_SURFACE_OFFSET_BYTES,
        KOKORO_QGEMM_WEIGHT_SUMS_SURFACE_OFFSET_BYTES,
        KOKORO_QGEMM_WEIGHT_SCALES_SURFACE_OFFSET_BYTES,
        KOKORO_QGEMM_ACTIVATIONS_SURFACE_OFFSET_BYTES,
        KOKORO_QGEMM_BIAS_SURFACE_OFFSET_BYTES,
        KOKORO_QGEMM_OUTPUT_SURFACE_OFFSET_BYTES,
    ];
    let surface_gpus = [
        params.packed_weights_gpu,
        params.weight_sums_gpu,
        params.weight_scales_gpu,
        params.activations_gpu,
        params.bias_gpu,
        params.output_gpu,
    ];
    let surface_bytes = [
        params.packed_weights_bytes,
        params.weight_sums_bytes,
        params.weight_scales_bytes,
        params.activations_bytes,
        params.bias_bytes,
        params.output_bytes,
    ];
    let binding_end = KOKORO_QGEMM_BINDING_TABLE_OFFSET_BYTES
        + surface_offsets.len() * core::mem::size_of::<u32>();
    if binding_end > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    unsafe {
        let binding = state
            .batch_virt
            .add(KOKORO_QGEMM_BINDING_TABLE_OFFSET_BYTES) as *mut u32;
        for (index, surface_offset) in surface_offsets.into_iter().enumerate() {
            core::ptr::write_volatile(binding.add(index), surface_offset as u32);
        }
    }
    for index in 0..surface_offsets.len() {
        if !direct_rcs_write_buffer_surface_state(
            state,
            surface_offsets[index],
            surface_gpus[index],
            surface_bytes[index],
        ) {
            return false;
        }
    }
    direct_rcs_write_kokoro_qgemm_payload(state, params)
}

fn direct_rcs_encode_kokoro_qgemm_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: KokoroQgemmParams,
) -> bool {
    let output_tiles = params.output_columns.div_ceil(16);
    let last_group_lanes = ((params.output_columns.saturating_sub(1)) % 16) + 1;
    let expected_right_mask = if last_group_lanes == 16 {
        GPGPU_WALKER_SIMD16_MASK
    } else {
        (1u32 << last_group_lanes) - 1
    };
    let expected_weight_words = output_tiles as usize * params.reduction_words as usize * 16;
    let expected_activation_words =
        params.matrix_rows as usize * params.activation_stride_words as usize;
    let expected_output_elements = params.matrix_rows as usize * params.output_stride as usize;
    if !kokoro_qgemm_upload_valid(upload)
        || params.matrix_rows == 0
        || params.matrix_rows as usize > KOKORO_QGEMM_MAX_MATRIX_ROWS
        || params.output_columns == 0
        || params.output_columns as usize > KOKORO_QGEMM_MAX_OUTPUT_COLUMNS
        || params.reduction_words == 0
        || params.reduction_words as usize > KOKORO_QGEMM_MAX_REDUCTION_WORDS
        || !kokoro_qgemm_admitted_shape(params.reduction_words * 4, params.output_columns)
        || params.activation_stride_words < params.reduction_words
        || params.output_stride < params.output_columns
        || params.activation_zero_point > u8::MAX as u32
        || !params.activation_scale.is_finite()
        || params.activation_scale <= 0.0
        || params.has_bias > 1
        || params.packed_weights_gpu
            != KOKORO_QGEMM_ARENA_GPU + KOKORO_QGEMM_PACKED_WEIGHTS_OFFSET_BYTES as u64
        || params.weight_sums_gpu
            != KOKORO_QGEMM_ARENA_GPU + KOKORO_QGEMM_WEIGHT_SUMS_OFFSET_BYTES as u64
        || params.weight_scales_gpu
            != KOKORO_QGEMM_ARENA_GPU + KOKORO_QGEMM_WEIGHT_SCALES_OFFSET_BYTES as u64
        || params.activations_gpu
            != KOKORO_QGEMM_ARENA_GPU + KOKORO_QGEMM_ACTIVATIONS_OFFSET_BYTES as u64
        || params.bias_gpu != KOKORO_QGEMM_ARENA_GPU + KOKORO_QGEMM_BIAS_OFFSET_BYTES as u64
        || params.output_gpu != KOKORO_QGEMM_ARENA_GPU + KOKORO_QGEMM_OUTPUT_OFFSET_BYTES as u64
        || params.packed_weights_bytes != expected_weight_words * core::mem::size_of::<u32>()
        || params.weight_sums_bytes != params.output_columns as usize * core::mem::size_of::<i32>()
        || params.weight_scales_bytes
            != params.output_columns as usize * core::mem::size_of::<f32>()
        || params.activations_bytes != expected_activation_words * core::mem::size_of::<u32>()
        || params.bias_bytes != params.output_columns as usize * core::mem::size_of::<f32>()
        || params.output_bytes != expected_output_elements * core::mem::size_of::<f32>()
        || params.group_x != output_tiles
        || params.group_y != params.matrix_rows
        || params.right_mask != expected_right_mask
    {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }
    if !direct_rcs_write_kokoro_qgemm_state(state, params) {
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
    ok &= direct_rcs_push(batch, &mut cursor, KOKORO_QGEMM_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, KOKORO_QGEMM_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_store_marker_at(
        batch,
        &mut cursor,
        state.gpu_va.result,
        KOKORO_QGEMM_PRE_MARKER_SLOT,
        KOKORO_QGEMM_PRE_MARKER,
    );
    ok &= direct_rcs_push_gpgpu_walker_2d(
        batch,
        &mut cursor,
        KOKORO_QGEMM_PAYLOAD_OFFSET_BYTES,
        KOKORO_QGEMM_INDIRECT_BYTES,
        params.group_x,
        params.group_y,
        params.right_mask,
    );
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push_gpgpu_dispatch_epilogue(
        batch,
        &mut cursor,
        state.gpu_va.result,
        KOKORO_QGEMM_POST_MARKER_SLOT,
        KOKORO_QGEMM_POST_MARKER,
    );
    if !ok || cursor * core::mem::size_of::<u32>() > KOKORO_QGEMM_IDD_OFFSET_BYTES {
        return false;
    }

    super::dma_flush(state.batch_virt, DIRECT_RCS_BATCH_BYTES);
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    true
}
