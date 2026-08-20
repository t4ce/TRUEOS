fn shadertoy_contract(shader_id: u32) -> Option<&'static GpgpuKernelAbiContract> {
    match shader_id {
        SHADERTOY_SHADER_MANDELBROT => Some(&SHADERTOY_MANDELBROT_ADLS_CPP_ABI_CONTRACT),
        SHADERTOY_SHADER_CUBE_FIELD => Some(&SHADERTOY_CUBE_FIELD_ADLS_CPP_ABI_CONTRACT),
        SHADERTOY_SHADER_NGUYEN => Some(&SHADERTOY_NGUYEN_ADLS_CPP_ABI_CONTRACT),
        _ => None,
    }
}

fn shadertoy_text_offset(shader_id: u32) -> Option<u64> {
    match shader_id {
        SHADERTOY_SHADER_MANDELBROT => Some(SHADERTOY_MANDELBROT_TEXT_OFFSET_BYTES),
        SHADERTOY_SHADER_CUBE_FIELD => Some(SHADERTOY_CUBE_FIELD_TEXT_OFFSET_BYTES),
        SHADERTOY_SHADER_NGUYEN => Some(SHADERTOY_NGUYEN_TEXT_OFFSET_BYTES),
        _ => None,
    }
}

fn direct_rcs_write_shadertoy_payload(
    state: DirectRcsState,
    dst: GpgpuRgba8Surface,
    params: ShaderToyFrameParams,
) -> bool {
    let Some(contract) = shadertoy_contract(params.shader_id) else {
        return false;
    };
    if contract.cross_thread_data_bytes as usize != SHADERTOY_CROSS_THREAD_BYTES
        || contract.per_thread_data_bytes as usize != SHADERTOY_PER_THREAD_BYTES
        || SHADERTOY_PAYLOAD_OFFSET_BYTES + SHADERTOY_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES
        || SHADERTOY_UNIFORMS_OFFSET_BYTES + SHADERTOY_UNIFORMS_BYTES > DIRECT_RCS_BATCH_BYTES
    {
        return false;
    }

    let uniforms_gpu = state.gpu_va.batch + SHADERTOY_UNIFORMS_OFFSET_BYTES as u64;
    unsafe {
        let payload = state.batch_virt.add(SHADERTOY_PAYLOAD_OFFSET_BYTES);
        core::ptr::write_bytes(payload, 0, SHADERTOY_INDIRECT_BYTES);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        core::ptr::write_volatile(dwords.add(12), dst.gpu as u32);
        core::ptr::write_volatile(dwords.add(13), (dst.gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(14), uniforms_gpu as u32);
        core::ptr::write_volatile(dwords.add(15), (uniforms_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(16), dst.width);
        core::ptr::write_volatile(dwords.add(17), dst.height);
        core::ptr::write_volatile(dwords.add(18), dst.pitch_bytes);

        let local_ids = payload.add(SHADERTOY_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }

        let values = [
            dst.width as f32,
            dst.height as f32,
            1.0,
            params.time_seconds,
            params.mouse_x,
            params.mouse_y,
            params.click_x,
            params.click_y,
            params.date_year,
            params.date_month,
            params.date_day,
            params.date_seconds,
            params.delta_seconds,
            params.frame_rate,
            params.sample_rate,
            params.frame as f32,
        ];
        let uniforms = state.batch_virt.add(SHADERTOY_UNIFORMS_OFFSET_BYTES) as *mut u32;
        for (index, value) in values.into_iter().enumerate() {
            core::ptr::write_volatile(uniforms.add(index), value.to_bits());
        }
    }
    true
}

fn direct_rcs_encode_shadertoy_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    dst: GpgpuRgba8Surface,
    params: ShaderToyFrameParams,
) -> bool {
    let Some(contract) = shadertoy_contract(params.shader_id) else {
        return false;
    };
    let Some(text_offset) = shadertoy_text_offset(params.shader_id) else {
        return false;
    };
    let expected_artifact = match params.shader_id {
        SHADERTOY_SHADER_MANDELBROT => SHADERTOY_MANDELBROT_ADLS_ARTIFACT,
        SHADERTOY_SHADER_CUBE_FIELD => SHADERTOY_CUBE_FIELD_ADLS_ARTIFACT,
        SHADERTOY_SHADER_NGUYEN => SHADERTOY_NGUYEN_ADLS_ARTIFACT,
        _ => return false,
    };
    if upload.bin_sha256 != expected_artifact.bin_sha256
        || upload.bytes != expected_artifact.bin.len()
        || contract.bindings.is_empty()
        || contract.bindings.len() > 2
        || contract.bindings[0].arg_index != 0
        || contract.bindings[0].bti != 0
    {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }

    let binding_count = contract.bindings.len() as u32;
    if !direct_rcs_write_interface_descriptor_at(
        state,
        SHADERTOY_IDD_OFFSET_BYTES,
        SHADERTOY_BINDING_TABLE_OFFSET_BYTES,
        text_offset,
        binding_count,
        SHADERTOY_CROSS_THREAD_BYTES.div_ceil(32) as u32,
    ) {
        return false;
    }
    unsafe {
        let binding = state.batch_virt.add(SHADERTOY_BINDING_TABLE_OFFSET_BYTES) as *mut u32;
        core::ptr::write_volatile(binding, SHADERTOY_DST_SURFACE_STATE_OFFSET_BYTES as u32);
        if binding_count == 2 {
            if contract.bindings[1].arg_index != 1 || contract.bindings[1].bti != 1 {
                return false;
            }
            core::ptr::write_volatile(
                binding.add(1),
                SHADERTOY_UNIFORMS_SURFACE_STATE_OFFSET_BYTES as u32,
            );
        }
    }
    let uniforms_gpu = state.gpu_va.batch + SHADERTOY_UNIFORMS_OFFSET_BYTES as u64;
    if !direct_rcs_write_buffer_surface_state(
        state,
        SHADERTOY_DST_SURFACE_STATE_OFFSET_BYTES,
        dst.gpu,
        dst.bytes,
    ) || (binding_count == 2
        && !direct_rcs_write_buffer_surface_state(
            state,
            SHADERTOY_UNIFORMS_SURFACE_STATE_OFFSET_BYTES,
            uniforms_gpu,
            SHADERTOY_UNIFORMS_BYTES,
        ))
        || !direct_rcs_write_shadertoy_payload(state, dst, params)
    {
        return false;
    }

    let group_x = dst.width.div_ceil(16).max(1);
    let group_y = dst.height.max(1);
    let last_group_pixels = ((dst.width - 1) % 16) + 1;
    let right_mask = if last_group_pixels == 16 {
        GPGPU_WALKER_SIMD16_MASK
    } else {
        (1u32 << last_group_pixels) - 1
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
    ok &= direct_rcs_push(batch, &mut cursor, SHADERTOY_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, SHADERTOY_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        SHADERTOY_PRE_MARKER_SLOT,
        SHADERTOY_PRE_MARKER,
    );
    ok &= direct_rcs_push_gpgpu_walker_2d(
        batch,
        &mut cursor,
        SHADERTOY_PAYLOAD_OFFSET_BYTES,
        SHADERTOY_INDIRECT_BYTES,
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
        SHADERTOY_POST_MARKER_SLOT,
        SHADERTOY_POST_MARKER,
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
