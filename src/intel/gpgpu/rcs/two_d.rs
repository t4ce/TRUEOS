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
        UI4_COMPOSITOR_PRE_MARKER_SLOT,
        UI4_COMPOSITOR_PRE_MARKER,
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
        UI4_COMPOSITOR_POST_MARKER_SLOT,
        UI4_COMPOSITOR_POST_MARKER,
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
    entry_offset: u64,
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
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }

    if !direct_rcs_write_copy_rect_interface_descriptor_at(
        state,
        COPY_RECT_BATCH_IDD_OFFSET_BYTES,
        COPY_RECT_BATCH_BINDING_TABLE_OFFSET_BYTES,
        entry_offset,
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
    direct_rcs_finish_copy_rect_dispatch_batch(
        state,
        upload,
        COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES,
        COPY_RECT_INDIRECT_BYTES,
        dispatch,
    )
}

const BFLY_SMOKE_HISTORY_CROSS_THREAD_BYTES: usize = 128;
const BFLY_SMOKE_HISTORY_INDIRECT_BYTES: usize =
    BFLY_SMOKE_HISTORY_CROSS_THREAD_BYTES + COPY_RECT_PER_THREAD_BYTES;
const BFLY_SMOKE_HISTORY_R8_SURFACE_OFFSET_BYTES: usize = 0x1100;
const fn assert_bfly_textured_history_abi(abi: &GpgpuKernelAbiContract) {
    assert!(abi.simd_width == 16);
    assert!(abi.scratch_bytes == 0 && abi.slm_bytes == 0);
    assert!(abi.cross_thread_data_bytes as usize == BFLY_SMOKE_HISTORY_CROSS_THREAD_BYTES);
    assert!(abi.per_thread_data_bytes as usize == COPY_RECT_PER_THREAD_BYTES);
    assert!(abi.bindings.len() == 3 && abi.bindings[2].arg_index == 10 && abi.bindings[2].bti == 2);
    let args = abi.payload_args;
    assert!(args.len() == 11 && args[0].offset_bytes == 48 && args[1].offset_bytes == 56);
    assert!(args[10].arg_index == 10 && args[10].offset_bytes == 64 && args[10].size_bytes == 8);
    let mut index = 2;
    while index < 10 {
        assert!(args[index].arg_index as usize == index);
        assert!(args[index].offset_bytes as usize == 64 + 4 * index && args[index].size_bytes == 4);
        index += 1;
    }
    assert!(abi.implicit_payload_args.len() == 4 && abi.implicit_payload_args[3].offset_bytes == 104);
}
const _: () = {
    assert_bfly_textured_history_abi(&BFLY_SMOKE_HISTORY_RGBA8_ADLS_CPP_ABI_CONTRACT);
    assert_bfly_textured_history_abi(&BFLY_CLOUD_HISTORY_RGBA8_ADLS_CPP_ABI_CONTRACT);
    assert_bfly_textured_history_abi(&BFLY_CLOUD_TRAIL_RGBA8_ADLS_CPP_ABI_CONTRACT);
};

fn direct_rcs_encode_bfly_textured_history_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    entry_offset: u64,
    params: CopyRectRgba8Params,
    src_bytes: usize,
    dst_bytes: usize,
    mask: &BflyHistoryR8,
) -> bool {
    let Some(dispatch) = copy_rect_2d_dispatch(params.width, params.height) else {
        return false;
    };
    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }
    direct_rcs_write_interface_descriptor_at(
        state,
        COPY_RECT_BATCH_IDD_OFFSET_BYTES,
        COPY_RECT_BATCH_BINDING_TABLE_OFFSET_BYTES,
        entry_offset,
        3,
        (BFLY_SMOKE_HISTORY_CROSS_THREAD_BYTES / 32) as u32,
    ) && direct_rcs_write_alpha_blend_worklist_surface_states_at(
        state,
        COPY_RECT_BATCH_BINDING_TABLE_OFFSET_BYTES,
        COPY_RECT_BATCH_SRC_SURFACE_STATE_OFFSET_BYTES,
        COPY_RECT_BATCH_DST_SURFACE_STATE_OFFSET_BYTES,
        BFLY_SMOKE_HISTORY_R8_SURFACE_OFFSET_BYTES,
        params.src_gpu,
        src_bytes,
        params.dst_gpu,
        dst_bytes,
        mask.gpu,
        mask.bytes,
    ) && direct_rcs_write_copy_rect_payload_with_texture_at(
        state,
        COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES,
        params,
        Some(mask.gpu),
    ) && direct_rcs_finish_copy_rect_dispatch_batch(
        state,
        upload,
        COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES,
        BFLY_SMOKE_HISTORY_INDIRECT_BYTES,
        dispatch,
    )
}

fn direct_rcs_finish_copy_rect_dispatch_batch(
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
    ok &= direct_rcs_push_compute_3d_entry(
        batch,
        &mut cursor,
        (1 << 9) | (1 << 11),
        PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL | 1,
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
    ok &= direct_rcs_push(batch, &mut cursor, COPY_RECT_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, COPY_RECT_BATCH_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_store_marker_for_state(
        state,
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
