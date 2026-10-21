fn kokoro_conv1d_upload_valid(upload: UploadedKernelArtifact) -> bool {
    upload.name == KOKORO_CONV1D_U8_U8_KERNEL_NAME
        && upload.bin_sha256 == KOKORO_CONV1D_U8_U8_ADLS_BIN_SHA256
        && upload.gpu == KOKORO_CONV1D_U8_U8_ADLS_GPU
        && upload.bytes == KOKORO_CONV1D_U8_U8_ADLS_BIN.len()
}

fn direct_rcs_write_kokoro_conv1d_payload(
    state: DirectRcsState,
    params: KokoroConv1dParams,
) -> bool {
    if KOKORO_CONV1D_PAYLOAD_OFFSET_BYTES + KOKORO_CONV1D_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    let Some(known) = super::opencl::registry::known_aot_kernel(KOKORO_CONV1D_U8_U8_KERNEL_NAME)
    else {
        crate::log_error!(
            target: "gpgpu";
            "intel/gpgpu: kokoro-conv1d payload rejected reason=missing-opencl-contract\n"
        );
        return false;
    };

    unsafe {
        let payload = state.batch_virt.add(KOKORO_CONV1D_PAYLOAD_OFFSET_BYTES);
        core::ptr::write_bytes(payload, 0, KOKORO_CONV1D_INDIRECT_BYTES);
        let dwords = payload as *mut u32;

        // One SIMD16 output-channel tile by one temporal position.
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);

        let pointers = [
            params.packed_weights_gpu,
            params.weight_tap_sums_gpu,
            params.packed_activations_gpu,
            params.output_gpu,
        ];
        for (index, pointer) in pointers.into_iter().enumerate() {
            let offset = 12 + index * 2;
            core::ptr::write_volatile(dwords.add(offset), pointer as u32);
            core::ptr::write_volatile(dwords.add(offset + 1), (pointer >> 32) as u32);
        }

        let cross_thread =
            core::slice::from_raw_parts_mut(payload, KOKORO_CONV1D_CROSS_THREAD_BYTES);
        let values = (|| {
            let mut writer = super::opencl::KernelValueWriter::new(known.contract, cross_thread)?;
            writer.set_u32(4, params.input_length)?;
            writer.set_u32(5, params.output_base)?;
            writer.set_u32(6, params.tile_length)?;
            writer.set_u32(7, params.activation_origin)?;
            writer.set_u32(8, params.activation_rows)?;
            writer.set_u32(9, params.input_channels)?;
            writer.set_u32(10, params.output_channels)?;
            writer.set_u32(11, params.kernel_size)?;
            writer.set_u32(12, params.dilation)?;
            writer.set_u32(13, params.pad_left)?;
            writer.set_u32(14, params.activation_zero_point)?;
            writer.set_u32(15, params.weight_zero_point)?;
            writer.finish()?;
            Ok::<(), super::opencl::KernelValueError>(())
        })();
        if let Err(error) = values {
            crate::log_error!(
                target: "gpgpu";
                "intel/gpgpu: kokoro-conv1d payload rejected reason=value-contract error={:?}\n",
                error,
            );
            return false;
        }

        let local_ids = payload.add(KOKORO_CONV1D_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_write_kokoro_conv1d_state(state: DirectRcsState, params: KokoroConv1dParams) -> bool {
    let cross_thread_grfs = KOKORO_CONV1D_CROSS_THREAD_BYTES.div_ceil(32) as u32;
    if !direct_rcs_write_interface_descriptor_at(
        state,
        KOKORO_CONV1D_IDD_OFFSET_BYTES,
        KOKORO_CONV1D_BINDING_TABLE_OFFSET_BYTES,
        KOKORO_CONV1D_U8_U8_TEXT_OFFSET_BYTES,
        4,
        cross_thread_grfs,
    ) {
        return false;
    }

    let surface_offsets = [
        KOKORO_CONV1D_PACKED_WEIGHTS_SURFACE_OFFSET_BYTES,
        KOKORO_CONV1D_WEIGHT_TAP_SUMS_SURFACE_OFFSET_BYTES,
        KOKORO_CONV1D_ACTIVATIONS_SURFACE_OFFSET_BYTES,
        KOKORO_CONV1D_OUTPUT_SURFACE_OFFSET_BYTES,
    ];
    let surface_gpus = [
        params.packed_weights_gpu,
        params.weight_tap_sums_gpu,
        params.packed_activations_gpu,
        params.output_gpu,
    ];
    let surface_bytes = [
        params.packed_weights_bytes,
        params.weight_tap_sums_bytes,
        params.packed_activations_bytes,
        params.output_bytes,
    ];
    let binding_end = KOKORO_CONV1D_BINDING_TABLE_OFFSET_BYTES
        + surface_offsets.len() * core::mem::size_of::<u32>();
    if binding_end > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    unsafe {
        let binding = state
            .batch_virt
            .add(KOKORO_CONV1D_BINDING_TABLE_OFFSET_BYTES) as *mut u32;
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
    direct_rcs_write_kokoro_conv1d_payload(state, params)
}

fn direct_rcs_encode_kokoro_conv1d_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: KokoroConv1dParams,
) -> bool {
    if params.activation_zero_point > u8::MAX as u32 || params.weight_zero_point > u8::MAX as u32 {
        return false;
    }
    let spec = KokoroConv1dSpec {
        input_length: params.input_length,
        input_channels: params.input_channels,
        output_channels: params.output_channels,
        kernel_size: params.kernel_size,
        dilation: params.dilation,
        pad_left: params.pad_left,
        activation_zero_point: params.activation_zero_point as u8,
        weight_zero_point: params.weight_zero_point as u8,
    };
    let Some(tile) = kokoro_conv1d_tile(spec, params.output_base, params.tile_length) else {
        return false;
    };
    let expected_weight_words = params.output_channels as usize
        * params.input_channels as usize
        * params.kernel_size as usize
        / 4;
    let expected_sum_elements = params.output_channels as usize * params.kernel_size as usize;
    let expected_activation_words =
        tile.activation_rows as usize * params.input_channels as usize / 4;
    let expected_output_elements = tile.tile_length as usize * params.output_channels as usize;
    if !kokoro_conv1d_upload_valid(upload)
        || tile.activation_origin != params.activation_origin
        || tile.activation_rows != params.activation_rows
        || params.packed_weights_gpu
            != KOKORO_CONV1D_ARENA_GPU + KOKORO_CONV1D_PACKED_WEIGHTS_OFFSET_BYTES as u64
        || params.weight_tap_sums_gpu
            != KOKORO_CONV1D_ARENA_GPU + KOKORO_CONV1D_WEIGHT_TAP_SUMS_OFFSET_BYTES as u64
        || params.packed_activations_gpu
            != KOKORO_CONV1D_ARENA_GPU + KOKORO_CONV1D_ACTIVATIONS_OFFSET_BYTES as u64
        || params.output_gpu != KOKORO_CONV1D_ARENA_GPU + KOKORO_CONV1D_OUTPUT_OFFSET_BYTES as u64
        || params.packed_weights_bytes != expected_weight_words * core::mem::size_of::<u32>()
        || params.weight_tap_sums_bytes != expected_sum_elements * core::mem::size_of::<u32>()
        || params.packed_activations_bytes
            != expected_activation_words * core::mem::size_of::<u32>()
        || params.output_bytes != expected_output_elements * core::mem::size_of::<i32>()
        || params.packed_weights_bytes > KOKORO_CONV1D_PACKED_WEIGHTS_ALLOC_BYTES
        || params.weight_tap_sums_bytes > KOKORO_CONV1D_WEIGHT_TAP_SUMS_ALLOC_BYTES
        || params.packed_activations_bytes > KOKORO_CONV1D_ACTIVATIONS_ALLOC_BYTES
        || params.output_bytes > KOKORO_CONV1D_OUTPUT_ALLOC_BYTES
        || params.group_x != params.output_channels / 16
        || params.group_y != params.tile_length
        || params.right_mask != GPGPU_WALKER_SIMD16_MASK
    {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }
    if !direct_rcs_write_kokoro_conv1d_state(state, params) {
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
    ok &= direct_rcs_push(batch, &mut cursor, KOKORO_CONV1D_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, KOKORO_CONV1D_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_store_marker_at(
        batch,
        &mut cursor,
        state.gpu_va.result,
        KOKORO_CONV1D_PRE_MARKER_SLOT,
        KOKORO_CONV1D_PRE_MARKER,
    );
    ok &= direct_rcs_push_gpgpu_walker_2d(
        batch,
        &mut cursor,
        KOKORO_CONV1D_PAYLOAD_OFFSET_BYTES,
        KOKORO_CONV1D_INDIRECT_BYTES,
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
        KOKORO_CONV1D_POST_MARKER_SLOT,
        KOKORO_CONV1D_POST_MARKER,
    );
    if !ok || cursor * core::mem::size_of::<u32>() > KOKORO_CONV1D_IDD_OFFSET_BYTES {
        return false;
    }

    super::dma_flush(state.batch_virt, DIRECT_RCS_BATCH_BYTES);
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    true
}
