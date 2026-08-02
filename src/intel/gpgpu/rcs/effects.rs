fn direct_rcs_encode_fill_rect_2d_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: FillRectRgba8Params,
    dst_bytes: usize,
) -> bool {
    if params.width == 0 || params.height == 0 {
        return false;
    }
    if CLEAR_RECT_PAYLOAD_OFFSET_BYTES + CLEAR_RECT_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }

    if !direct_rcs_write_fill_rect_interface_descriptor(state) {
        return false;
    }
    if !direct_rcs_write_clear_rect_surface_state(state, params.dst_gpu, dst_bytes) {
        return false;
    }
    if !direct_rcs_write_fill_rect_payload(state, params) {
        return false;
    }

    let batch_len = DIRECT_RCS_BATCH_BYTES / core::mem::size_of::<u32>();
    let batch = unsafe { core::slice::from_raw_parts_mut(state.batch_virt as *mut u32, batch_len) };
    let mut cursor = 0usize;
    let mut ok = true;
    let Some(dispatch) = fill_rect_2d_dispatch(params.width, params.height) else {
        return false;
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
    ok &= direct_rcs_push(batch, &mut cursor, CLEAR_RECT_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, CLEAR_RECT_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_store_marker_for_state(
        state,
        batch,
        &mut cursor,
        CLEAR_RECT_PRE_MARKER_SLOT,
        CLEAR_RECT_PRE_MARKER,
    );
    ok &= direct_rcs_push_gpgpu_walker_2d(
        batch,
        &mut cursor,
        CLEAR_RECT_PAYLOAD_OFFSET_BYTES,
        CLEAR_RECT_INDIRECT_BYTES,
        dispatch.group_x,
        dispatch.group_y,
        dispatch.right_mask,
    );
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_FLUSH_BITS);
    ok &= direct_rcs_push_store_marker_for_state(
        state,
        batch,
        &mut cursor,
        CLEAR_RECT_POST_MARKER_SLOT,
        CLEAR_RECT_POST_MARKER,
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

fn direct_rcs_encode_skybox_sample_rgb565_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: SkyboxSampleRgb565Params,
    skybox_bytes: usize,
    dst_bytes: usize,
) -> bool {
    if params.rect_width == 0 || params.rect_height == 0 {
        return false;
    }
    if SKYBOX_SAMPLE_PAYLOAD_OFFSET_BYTES + SKYBOX_SAMPLE_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }

    if !direct_rcs_write_copy_rect_interface_descriptor_at_with_cross_thread_grfs(
        state,
        SKYBOX_SAMPLE_IDD_OFFSET_BYTES,
        SKYBOX_SAMPLE_BINDING_TABLE_OFFSET_BYTES,
        SKYBOX_SAMPLE_RGB565_TEXT_OFFSET_BYTES,
        5,
    ) {
        return false;
    }
    if !direct_rcs_write_copy_rect_surface_states_at(
        state,
        SKYBOX_SAMPLE_BINDING_TABLE_OFFSET_BYTES,
        SKYBOX_SAMPLE_SRC_SURFACE_STATE_OFFSET_BYTES,
        SKYBOX_SAMPLE_DST_SURFACE_STATE_OFFSET_BYTES,
        params.sky_gpu,
        skybox_bytes,
        params.dst_gpu,
        dst_bytes,
    ) {
        return false;
    }
    if !direct_rcs_write_skybox_sample_rgb565_payload_at(
        state,
        SKYBOX_SAMPLE_PAYLOAD_OFFSET_BYTES,
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
    ok &= direct_rcs_push(batch, &mut cursor, SKYBOX_SAMPLE_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, SKYBOX_SAMPLE_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        SKYBOX_SAMPLE_PRE_MARKER_SLOT,
        SKYBOX_SAMPLE_PRE_MARKER,
    );
    ok &= direct_rcs_push_gpgpu_walker_2d(
        batch,
        &mut cursor,
        SKYBOX_SAMPLE_PAYLOAD_OFFSET_BYTES,
        SKYBOX_SAMPLE_INDIRECT_BYTES,
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
        SKYBOX_SAMPLE_POST_MARKER_SLOT,
        SKYBOX_SAMPLE_POST_MARKER,
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

fn direct_rcs_encode_chart_sine_rgba8_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: ChartSineRgba8Params,
    dst_bytes: usize,
) -> bool {
    if params.rect_width == 0 || params.rect_height == 0 {
        return false;
    }
    if CHART_SINE_PAYLOAD_OFFSET_BYTES + CHART_SINE_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }

    if !direct_rcs_write_interface_descriptor_at(
        state,
        CHART_SINE_IDD_OFFSET_BYTES,
        CHART_SINE_BINDING_TABLE_OFFSET_BYTES,
        CHART_SINE_RGBA8_TEXT_OFFSET_BYTES,
        1,
        4,
    ) {
        return false;
    }
    let binding_end = CHART_SINE_BINDING_TABLE_OFFSET_BYTES + core::mem::size_of::<u32>();
    if binding_end > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    unsafe {
        let binding = state.batch_virt.add(CHART_SINE_BINDING_TABLE_OFFSET_BYTES) as *mut u32;
        core::ptr::write_volatile(binding, CHART_SINE_DST_SURFACE_STATE_OFFSET_BYTES as u32);
    }
    if !direct_rcs_write_buffer_surface_state(
        state,
        CHART_SINE_DST_SURFACE_STATE_OFFSET_BYTES,
        params.dst_gpu,
        dst_bytes,
    ) || !direct_rcs_write_chart_sine_rgba8_payload_at(
        state,
        CHART_SINE_PAYLOAD_OFFSET_BYTES,
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
    // RightExecutionMask describes the SIMD lanes in every hardware thread,
    // not merely the final X workgroup. Each group here is one full SIMD16
    // thread; the shader's x >= rect_width guard safely rejects padded lanes
    // in the final group.
    let right_mask = GPGPU_WALKER_SIMD16_MASK;

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
    ok &= direct_rcs_push(batch, &mut cursor, CHART_SINE_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, CHART_SINE_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        CHART_SINE_PRE_MARKER_SLOT,
        CHART_SINE_PRE_MARKER,
    );
    ok &= direct_rcs_push_gpgpu_walker_2d(
        batch,
        &mut cursor,
        CHART_SINE_PAYLOAD_OFFSET_BYTES,
        CHART_SINE_INDIRECT_BYTES,
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
        CHART_SINE_POST_MARKER_SLOT,
        CHART_SINE_POST_MARKER,
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

fn direct_rcs_encode_pixel_plasma_rgba8_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: PixelPlasmaRgba8Params,
    dst_bytes: usize,
) -> bool {
    if params.rect_width == 0 || params.rect_height == 0 {
        return false;
    }
    if PIXEL_PLASMA_PAYLOAD_OFFSET_BYTES + PIXEL_PLASMA_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }

    if !direct_rcs_write_interface_descriptor_at(
        state,
        PIXEL_PLASMA_IDD_OFFSET_BYTES,
        PIXEL_PLASMA_BINDING_TABLE_OFFSET_BYTES,
        PIXEL_PLASMA_RGBA8_TEXT_OFFSET_BYTES,
        1,
        4,
    ) {
        return false;
    }
    let binding_end = PIXEL_PLASMA_BINDING_TABLE_OFFSET_BYTES + core::mem::size_of::<u32>();
    if binding_end > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    unsafe {
        let binding = state
            .batch_virt
            .add(PIXEL_PLASMA_BINDING_TABLE_OFFSET_BYTES) as *mut u32;
        core::ptr::write_volatile(binding, PIXEL_PLASMA_DST_SURFACE_STATE_OFFSET_BYTES as u32);
    }
    if !direct_rcs_write_buffer_surface_state(
        state,
        PIXEL_PLASMA_DST_SURFACE_STATE_OFFSET_BYTES,
        params.dst_gpu,
        dst_bytes,
    ) || !direct_rcs_write_pixel_plasma_rgba8_payload_at(
        state,
        PIXEL_PLASMA_PAYLOAD_OFFSET_BYTES,
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
    ok &= direct_rcs_push(batch, &mut cursor, PIXEL_PLASMA_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, PIXEL_PLASMA_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        PIXEL_PLASMA_PRE_MARKER_SLOT,
        PIXEL_PLASMA_PRE_MARKER,
    );
    ok &= direct_rcs_push_gpgpu_walker_2d(
        batch,
        &mut cursor,
        PIXEL_PLASMA_PAYLOAD_OFFSET_BYTES,
        PIXEL_PLASMA_INDIRECT_BYTES,
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
        PIXEL_PLASMA_POST_MARKER_SLOT,
        PIXEL_PLASMA_POST_MARKER,
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
