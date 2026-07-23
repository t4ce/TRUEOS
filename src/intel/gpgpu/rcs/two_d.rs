fn direct_rcs_encode_ui4_nv12_tile64_to_rgba8_frame_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: Ui4Nv12Tile64ToRgba8FrameParams,
    source_bytes: usize,
    dst_bytes: usize,
) -> bool {
    if params.output_width == 0
        || params.output_height == 0
        || UI4_NV12_PRIMARY_PAYLOAD_OFFSET_BYTES + UI4_NV12_PRIMARY_INDIRECT_BYTES
            > DIRECT_RCS_BATCH_BYTES
    {
        return false;
    }
    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }
    if !direct_rcs_write_interface_descriptor_at(
        state,
        UI4_NV12_PRIMARY_IDD_OFFSET_BYTES,
        UI4_NV12_PRIMARY_BINDING_TABLE_OFFSET_BYTES,
        UI4_NV12_TILE64_TO_RGBA8_FRAME_TEXT_OFFSET_BYTES,
        3,
        UI4_NV12_PRIMARY_CROSS_THREAD_GRFS,
    ) || !direct_rcs_write_alpha_blend_worklist_surface_states_at(
        state,
        UI4_NV12_PRIMARY_BINDING_TABLE_OFFSET_BYTES,
        UI4_NV12_PRIMARY_SRC_SURFACE_STATE_OFFSET_BYTES,
        UI4_NV12_PRIMARY_BASE_SURFACE_STATE_OFFSET_BYTES,
        UI4_NV12_PRIMARY_DST_SURFACE_STATE_OFFSET_BYTES,
        params.nv12_gpu,
        source_bytes,
        params.base_gpu,
        dst_bytes,
        params.dst_gpu,
        dst_bytes,
    ) || !direct_rcs_write_ui4_nv12_frame_payload_at(
        state,
        UI4_NV12_PRIMARY_PAYLOAD_OFFSET_BYTES,
        params,
    ) {
        return false;
    }

    let batch_len = DIRECT_RCS_BATCH_BYTES / core::mem::size_of::<u32>();
    let batch = unsafe { core::slice::from_raw_parts_mut(state.batch_virt as *mut u32, batch_len) };
    let mut cursor = 0usize;
    let mut ok = direct_rcs_push_pipe_control_timestamp_at(
        batch,
        &mut cursor,
        state.gpu_va.result,
        UI4_VIDEO_FRAME_GPU_BATCH_ENTER_TIMESTAMP_SLOT,
    );
    ok &= direct_rcs_push_gpgpu_dispatch_prologue(batch, &mut cursor, upload, state.gpu_va.batch);
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, COPY_RECT_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, UI4_NV12_PRIMARY_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_store_marker_at(
        batch,
        &mut cursor,
        state.gpu_va.result,
        SPRITE_QUAD_WORKLIST_PRE_MARKER_SLOT,
        SPRITE_QUAD_WORKLIST_PRE_MARKER,
    );
    ok &= direct_rcs_push_pipe_control_timestamp_at(
        batch,
        &mut cursor,
        state.gpu_va.result,
        UI4_VIDEO_FRAME_GPU_PRE_WALKER_TIMESTAMP_SLOT,
    );
    ok &= direct_rcs_push_gpgpu_walker_2d(
        batch,
        &mut cursor,
        UI4_NV12_PRIMARY_PAYLOAD_OFFSET_BYTES,
        UI4_NV12_PRIMARY_INDIRECT_BYTES,
        params.output_width.div_ceil(16),
        params.output_height,
        GPGPU_WALKER_SIMD16_MASK,
    );
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push_pipe_control_timestamp_at(
        batch,
        &mut cursor,
        state.gpu_va.result,
        UI4_VIDEO_FRAME_GPU_POST_WALKER_TIMESTAMP_SLOT,
    );
    ok &= direct_rcs_push_gpgpu_dispatch_timestamped_epilogue(
        batch,
        &mut cursor,
        state.gpu_va.result,
        UI4_VIDEO_FRAME_GPU_POST_RELEASE_TIMESTAMP_SLOT,
        SPRITE_QUAD_WORKLIST_POST_MARKER_SLOT,
        SPRITE_QUAD_WORKLIST_POST_MARKER,
    );
    if !ok {
        return false;
    }
    super::dma_flush(state.batch_virt, DIRECT_RCS_BATCH_BYTES);
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    true
}

fn direct_rcs_encode_ui4_nv12_tile64_to_primary_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: Ui4Nv12Tile64ToRgba8FrameParams,
    source_bytes: usize,
    base_bytes: usize,
    dst_bytes: usize,
) -> bool {
    if params.output_width == 0
        || params.output_height == 0
        || UI4_NV12_PRIMARY_PAYLOAD_OFFSET_BYTES + UI4_NV12_PRIMARY_INDIRECT_BYTES
            > DIRECT_RCS_BATCH_BYTES
    {
        return false;
    }
    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }
    if !direct_rcs_write_interface_descriptor_at(
        state,
        UI4_NV12_PRIMARY_IDD_OFFSET_BYTES,
        UI4_NV12_PRIMARY_BINDING_TABLE_OFFSET_BYTES,
        UI4_NV12_YTILE_TO_PRIMARY_XRGB_TEXT_OFFSET_BYTES,
        3,
        UI4_NV12_PRIMARY_CROSS_THREAD_GRFS,
    ) || !direct_rcs_write_alpha_blend_worklist_surface_states_at(
        state,
        UI4_NV12_PRIMARY_BINDING_TABLE_OFFSET_BYTES,
        UI4_NV12_PRIMARY_SRC_SURFACE_STATE_OFFSET_BYTES,
        UI4_NV12_PRIMARY_BASE_SURFACE_STATE_OFFSET_BYTES,
        UI4_NV12_PRIMARY_DST_SURFACE_STATE_OFFSET_BYTES,
        params.nv12_gpu,
        source_bytes,
        params.base_gpu,
        base_bytes,
        params.dst_gpu,
        dst_bytes,
    ) || !direct_rcs_write_ui4_nv12_frame_payload_at(
        state,
        UI4_NV12_PRIMARY_PAYLOAD_OFFSET_BYTES,
        params,
    ) {
        return false;
    }

    let batch_len = DIRECT_RCS_BATCH_BYTES / core::mem::size_of::<u32>();
    let batch = unsafe { core::slice::from_raw_parts_mut(state.batch_virt as *mut u32, batch_len) };
    let mut cursor = 0usize;
    let mut ok =
        direct_rcs_push_gpgpu_dispatch_prologue(batch, &mut cursor, upload, state.gpu_va.batch);
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, COPY_RECT_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, UI4_NV12_PRIMARY_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_store_marker_at(
        batch,
        &mut cursor,
        state.gpu_va.result,
        SPRITE_QUAD_WORKLIST_PRE_MARKER_SLOT,
        SPRITE_QUAD_WORKLIST_PRE_MARKER,
    );
    ok &= direct_rcs_push_gpgpu_walker_2d(
        batch,
        &mut cursor,
        UI4_NV12_PRIMARY_PAYLOAD_OFFSET_BYTES,
        UI4_NV12_PRIMARY_INDIRECT_BYTES,
        params.output_width.div_ceil(16),
        params.output_height,
        GPGPU_WALKER_SIMD16_MASK,
    );
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push_gpgpu_dispatch_epilogue(
        batch,
        &mut cursor,
        state.gpu_va.result,
        SPRITE_QUAD_WORKLIST_POST_MARKER_SLOT,
        SPRITE_QUAD_WORKLIST_POST_MARKER,
    );
    if !ok {
        return false;
    }
    super::dma_flush(state.batch_virt, DIRECT_RCS_BATCH_BYTES);
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    true
}

fn direct_rcs_encode_copy_rect_2d_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: CopyRectRgba8Params,
    src_bytes: usize,
    dst_bytes: usize,
) -> bool {
    if params.width == 0
        || params.height == 0
        || COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES + COPY_RECT_INDIRECT_BYTES
            > DIRECT_RCS_BATCH_BYTES
    {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }

    if !direct_rcs_write_copy_rect_interface_descriptor_at(
        state,
        COPY_RECT_BATCH_IDD_OFFSET_BYTES,
        COPY_RECT_BATCH_BINDING_TABLE_OFFSET_BYTES,
        COPY_RECT_RGBA8_TEXT_OFFSET_BYTES,
    ) || !direct_rcs_write_copy_rect_surface_states_at(
        state,
        COPY_RECT_BATCH_BINDING_TABLE_OFFSET_BYTES,
        COPY_RECT_BATCH_SRC_SURFACE_STATE_OFFSET_BYTES,
        COPY_RECT_BATCH_DST_SURFACE_STATE_OFFSET_BYTES,
        params.src_gpu,
        src_bytes,
        params.dst_gpu,
        dst_bytes,
    ) || !direct_rcs_write_copy_rect_payload_at(
        state,
        COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES,
        params,
    ) {
        return false;
    }

    let Some(dispatch) = copy_rect_2d_dispatch(params.width, params.height) else {
        return false;
    };
    direct_rcs_finish_two_buffer_dispatch_batch(
        state,
        upload,
        COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES,
        COPY_RECT_INDIRECT_BYTES,
        dispatch,
    )
}

fn direct_rcs_encode_resolve_tile64_msaa4_2d_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: CopyRectRgba8Params,
    src_bytes: usize,
    dst_bytes: usize,
) -> bool {
    if params.width == 0 || params.height == 0 {
        return false;
    }
    if COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES + COPY_RECT_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES
    {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }

    if !direct_rcs_write_copy_rect_interface_descriptor_at_with_cross_thread_grfs(
        state,
        COPY_RECT_BATCH_IDD_OFFSET_BYTES,
        COPY_RECT_BATCH_BINDING_TABLE_OFFSET_BYTES,
        RESOLVE_TILE64_MSAA4_RGBA8_TEXT_OFFSET_BYTES,
        3,
    ) {
        return false;
    }
    if !direct_rcs_write_copy_rect_surface_states_at(
        state,
        COPY_RECT_BATCH_BINDING_TABLE_OFFSET_BYTES,
        COPY_RECT_BATCH_SRC_SURFACE_STATE_OFFSET_BYTES,
        COPY_RECT_BATCH_DST_SURFACE_STATE_OFFSET_BYTES,
        params.src_gpu,
        src_bytes,
        params.dst_gpu,
        dst_bytes,
    ) {
        return false;
    }
    if !direct_rcs_write_copy_rect_payload_at(
        state,
        COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES,
        params,
    ) {
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
    ok &= direct_rcs_push(batch, &mut cursor, COPY_RECT_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, COPY_RECT_BATCH_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        COPY_RECT_PRE_MARKER_SLOT,
        COPY_RECT_PRE_MARKER,
    );
    ok &= direct_rcs_push_gpgpu_walker_2d(
        batch,
        &mut cursor,
        COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES,
        COPY_RECT_INDIRECT_BYTES,
        dispatch.group_x,
        dispatch.group_y,
        dispatch.right_mask,
    );
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push_gpgpu_dispatch_epilogue(
        batch,
        &mut cursor,
        state.gpu_va.result,
        COPY_RECT_POST_MARKER_SLOT,
        COPY_RECT_POST_MARKER,
    );

    if !ok {
        return false;
    }

    super::dma_flush(state.batch_virt, DIRECT_RCS_BATCH_BYTES);
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    true
}

fn direct_rcs_encode_font_outline_coverage_r8_2d_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: FontOutlineCoverageR8Params,
    ops_bytes: usize,
    mask_bytes: usize,
) -> bool {
    if params.rect_width == 0
        || params.rect_height == 0
        || COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES + FONT_OUTLINE_COVERAGE_R8_INDIRECT_BYTES
            > DIRECT_RCS_BATCH_BYTES
    {
        return false;
    }
    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }
    if !direct_rcs_write_copy_rect_interface_descriptor_at_with_cross_thread_grfs(
        state,
        COPY_RECT_BATCH_IDD_OFFSET_BYTES,
        COPY_RECT_BATCH_BINDING_TABLE_OFFSET_BYTES,
        FONT_OUTLINE_COVERAGE_R8_TEXT_OFFSET_BYTES,
        4,
    ) || !direct_rcs_write_copy_rect_surface_states_at(
        state,
        COPY_RECT_BATCH_BINDING_TABLE_OFFSET_BYTES,
        COPY_RECT_BATCH_SRC_SURFACE_STATE_OFFSET_BYTES,
        COPY_RECT_BATCH_DST_SURFACE_STATE_OFFSET_BYTES,
        params.ops_gpu,
        ops_bytes,
        params.mask_gpu,
        mask_bytes,
    ) || !direct_rcs_write_font_outline_coverage_r8_payload_at(
        state,
        COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES,
        params,
    ) {
        return false;
    }
    direct_rcs_finish_two_buffer_2d_batch(
        state,
        upload,
        COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES,
        FONT_OUTLINE_COVERAGE_R8_INDIRECT_BYTES,
        params.rect_width,
        params.rect_height,
    )
}

fn direct_rcs_encode_glyph_mask_2d_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: CopyRectRgba8Params,
    color_rgba: u32,
    mask_bytes: usize,
    dst_bytes: usize,
) -> bool {
    if params.width == 0
        || params.height == 0
        || COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES + GLYPH_MASK_INDIRECT_BYTES
            > DIRECT_RCS_BATCH_BYTES
    {
        return false;
    }
    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }
    if !direct_rcs_write_copy_rect_interface_descriptor_at_with_cross_thread_grfs(
        state,
        COPY_RECT_BATCH_IDD_OFFSET_BYTES,
        COPY_RECT_BATCH_BINDING_TABLE_OFFSET_BYTES,
        GLYPH_MASK_RGBA8_TEXT_OFFSET_BYTES,
        4,
    ) || !direct_rcs_write_copy_rect_surface_states_at(
        state,
        COPY_RECT_BATCH_BINDING_TABLE_OFFSET_BYTES,
        COPY_RECT_BATCH_SRC_SURFACE_STATE_OFFSET_BYTES,
        COPY_RECT_BATCH_DST_SURFACE_STATE_OFFSET_BYTES,
        params.src_gpu,
        mask_bytes,
        params.dst_gpu,
        dst_bytes,
    ) || !direct_rcs_write_glyph_mask_payload_at(
        state,
        COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES,
        params,
        color_rgba,
    ) {
        return false;
    }
    direct_rcs_finish_two_buffer_2d_batch(
        state,
        upload,
        COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES,
        GLYPH_MASK_INDIRECT_BYTES,
        params.width,
        params.height,
    )
}

fn direct_rcs_encode_glyph_mask_layers_2d_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    layers: &[GpgpuGlyphMaskLayer],
    dst: GpgpuRgba8Surface,
) -> bool {
    let mut active_walkers = 0usize;
    for layer in layers {
        let blit = GpgpuGlyphMaskBlit {
            mask: layer.mask,
            mask_rect: layer.mask_rect,
            dst,
            dst_xy: layer.dst_xy,
            color_rgba: layer.color_rgba,
        };
        if lower_glyph_mask_blit(blit).is_some() {
            active_walkers += 1;
        }
    }
    if active_walkers == 0 {
        return false;
    }
    if active_walkers > GLYPH_MASK_BATCH_MAX_LAYERS {
        return false;
    }
    let payload_end = GLYPH_MASK_BATCH_PAYLOAD_BASE_OFFSET_BYTES
        .saturating_add(active_walkers.saturating_mul(GLYPH_MASK_INDIRECT_BYTES));
    if payload_end > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }
    let mut walker_index = 0usize;
    for layer in layers {
        let blit = GpgpuGlyphMaskBlit {
            mask: layer.mask,
            mask_rect: layer.mask_rect,
            dst,
            dst_xy: layer.dst_xy,
            color_rgba: layer.color_rgba,
        };
        let Some(params) = lower_glyph_mask_blit(blit) else {
            continue;
        };
        let state_block = GLYPH_MASK_BATCH_STATE_BASE_OFFSET_BYTES
            + walker_index * GLYPH_MASK_BATCH_STATE_BLOCK_BYTES;
        let idd_offset = state_block + GLYPH_MASK_BATCH_IDD_OFFSET_IN_BLOCK_BYTES;
        let binding_table_offset =
            state_block + GLYPH_MASK_BATCH_BINDING_TABLE_OFFSET_IN_BLOCK_BYTES;
        let src_surface_offset = state_block + GLYPH_MASK_BATCH_SRC_SURFACE_OFFSET_IN_BLOCK_BYTES;
        let dst_surface_offset = state_block + GLYPH_MASK_BATCH_DST_SURFACE_OFFSET_IN_BLOCK_BYTES;
        if !direct_rcs_write_copy_rect_interface_descriptor_at_with_cross_thread_grfs(
            state,
            idd_offset,
            binding_table_offset,
            GLYPH_MASK_RGBA8_TEXT_OFFSET_BYTES,
            4,
        ) || !direct_rcs_write_copy_rect_surface_states_at(
            state,
            binding_table_offset,
            src_surface_offset,
            dst_surface_offset,
            params.src_gpu,
            layer.mask.bytes,
            params.dst_gpu,
            dst.bytes,
        ) {
            return false;
        }
        let payload_offset =
            GLYPH_MASK_BATCH_PAYLOAD_BASE_OFFSET_BYTES + walker_index * GLYPH_MASK_INDIRECT_BYTES;
        if !direct_rcs_write_glyph_mask_payload_at(state, payload_offset, params, layer.color_rgba)
        {
            return false;
        }
        walker_index += 1;
    }

    let batch_len = DIRECT_RCS_BATCH_BYTES / core::mem::size_of::<u32>();
    let batch = unsafe { core::slice::from_raw_parts_mut(state.batch_virt as *mut u32, batch_len) };
    let mut cursor = 0usize;
    let mut ok = true;
    ok &= direct_rcs_push_gpgpu_dispatch_prologue(batch, &mut cursor, upload, state.gpu_va.batch);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        COPY_RECT_PRE_MARKER_SLOT,
        COPY_RECT_PRE_MARKER,
    );

    walker_index = 0;
    for layer in layers {
        let blit = GpgpuGlyphMaskBlit {
            mask: layer.mask,
            mask_rect: layer.mask_rect,
            dst,
            dst_xy: layer.dst_xy,
            color_rgba: layer.color_rgba,
        };
        let Some(params) = lower_glyph_mask_blit(blit) else {
            continue;
        };
        let Some(dispatch) = fill_rect_2d_dispatch(params.width, params.height) else {
            return false;
        };
        let state_block = GLYPH_MASK_BATCH_STATE_BASE_OFFSET_BYTES
            + walker_index * GLYPH_MASK_BATCH_STATE_BLOCK_BYTES;
        let idd_offset = state_block + GLYPH_MASK_BATCH_IDD_OFFSET_IN_BLOCK_BYTES;
        let payload_offset =
            GLYPH_MASK_BATCH_PAYLOAD_BASE_OFFSET_BYTES + walker_index * GLYPH_MASK_INDIRECT_BYTES;
        ok &= direct_rcs_push(batch, &mut cursor, MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD);
        ok &= direct_rcs_push(batch, &mut cursor, 0);
        ok &= direct_rcs_push(batch, &mut cursor, COPY_RECT_IDD_BYTES as u32);
        ok &= direct_rcs_push(batch, &mut cursor, idd_offset as u32);
        ok &= direct_rcs_push_gpgpu_walker_2d(
            batch,
            &mut cursor,
            payload_offset,
            GLYPH_MASK_INDIRECT_BYTES,
            dispatch.group_x,
            dispatch.group_y,
            dispatch.right_mask,
        );
        ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
        ok &= direct_rcs_push(batch, &mut cursor, 0);
        walker_index += 1;
    }
    ok &= direct_rcs_push_gpgpu_dispatch_epilogue(
        batch,
        &mut cursor,
        state.gpu_va.result,
        COPY_RECT_POST_MARKER_SLOT,
        COPY_RECT_POST_MARKER,
    );
    if !ok
        || cursor.saturating_mul(core::mem::size_of::<u32>())
            > GLYPH_MASK_BATCH_STATE_BASE_OFFSET_BYTES
    {
        return false;
    }
    super::dma_flush(state.batch_virt, DIRECT_RCS_BATCH_BYTES);
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    true
}

fn direct_rcs_finish_two_buffer_2d_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    payload_offset: usize,
    indirect_bytes: usize,
    width: u32,
    height: u32,
) -> bool {
    let Some(dispatch) = fill_rect_2d_dispatch(width, height) else {
        return false;
    };
    direct_rcs_finish_two_buffer_dispatch_batch(
        state,
        upload,
        payload_offset,
        indirect_bytes,
        dispatch,
    )
}

fn direct_rcs_finish_two_buffer_dispatch_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    payload_offset: usize,
    indirect_bytes: usize,
    dispatch: FillRect2dDispatch,
) -> bool {
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
    ok &= direct_rcs_push(batch, &mut cursor, COPY_RECT_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, COPY_RECT_BATCH_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        COPY_RECT_PRE_MARKER_SLOT,
        COPY_RECT_PRE_MARKER,
    );
    ok &= direct_rcs_push_gpgpu_walker_2d(
        batch,
        &mut cursor,
        payload_offset,
        indirect_bytes,
        dispatch.group_x,
        dispatch.group_y,
        dispatch.right_mask,
    );
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push_gpgpu_dispatch_epilogue(
        batch,
        &mut cursor,
        state.gpu_va.result,
        COPY_RECT_POST_MARKER_SLOT,
        COPY_RECT_POST_MARKER,
    );
    if !ok {
        return false;
    }
    super::dma_flush(state.batch_virt, DIRECT_RCS_BATCH_BYTES);
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    true
}
