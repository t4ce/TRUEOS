fn direct_rcs_encode_alpha_blend_worklist_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: AlphaBlendWorklistRgba8Params,
    src_bytes: usize,
    dst_bytes: usize,
    desc_bytes: usize,
) -> bool {
    let desc_count = params.desc_count as usize;
    let walker_count = rect_worklist_walker_count(desc_count);
    if desc_count == 0 || desc_count > ALPHA_BLEND_WORKLIST_MAX_DESCS || walker_count == 0 {
        return false;
    }
    let payload_end =
        RECT_WORKLIST_PAYLOAD_OFFSET_BYTES + walker_count * ALPHA_BLEND_WORKLIST_INDIRECT_BYTES;
    if payload_end > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }
    if !direct_rcs_write_rect_worklist_interface_descriptor_at(
        state,
        RECT_WORKLIST_IDD_OFFSET_BYTES,
        RECT_WORKLIST_BINDING_TABLE_OFFSET_BYTES,
        ALPHA_BLEND_WORKLIST_RGBA8_TEXT_OFFSET_BYTES,
        3,
        ALPHA_BLEND_WORKLIST_CROSS_THREAD_GRFS,
    ) || !direct_rcs_write_alpha_blend_worklist_surface_states(
        state,
        params.src_gpu,
        src_bytes,
        params.dst_gpu,
        dst_bytes,
        params.desc_gpu,
        desc_bytes,
    ) {
        return false;
    }
    for walker in 0..walker_count {
        let desc_base = walker.saturating_mul(RECT_WORKLIST_DESCS_PER_WALKER);
        let local_count = desc_count
            .saturating_sub(desc_base)
            .min(RECT_WORKLIST_DESCS_PER_WALKER);
        let payload_offset =
            RECT_WORKLIST_PAYLOAD_OFFSET_BYTES + walker * ALPHA_BLEND_WORKLIST_INDIRECT_BYTES;
        let payload_params = AlphaBlendWorklistRgba8Params {
            desc_base: params.desc_base.saturating_add(desc_base as u32),
            desc_count: local_count as u32,
            ..params
        };
        if !direct_rcs_write_alpha_blend_worklist_payload_at(state, payload_offset, payload_params)
        {
            return false;
        }
    }

    direct_rcs_encode_rect_worklist_command_stream(
        state,
        upload,
        walker_count,
        desc_count,
        ALPHA_BLEND_WORKLIST_PRE_MARKER,
        ALPHA_BLEND_WORKLIST_POST_MARKER,
        true,
    )
}




fn direct_rcs_encode_rect_worklist_command_stream(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    walker_count: usize,
    desc_count: usize,
    pre_marker: u32,
    post_marker: u32,
    one_group_per_descriptor: bool,
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
    ok &= direct_rcs_push(batch, &mut cursor, RECT_WORKLIST_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, RECT_WORKLIST_IDD_OFFSET_BYTES as u32);
    ok &=
        direct_rcs_push_store_marker(batch, &mut cursor, RECT_WORKLIST_PRE_MARKER_SLOT, pre_marker);
    for walker in 0..walker_count {
        let desc_base = walker.saturating_mul(RECT_WORKLIST_DESCS_PER_WALKER);
        let local_count = desc_count
            .saturating_sub(desc_base)
            .min(RECT_WORKLIST_DESCS_PER_WALKER);
        let payload_offset =
            RECT_WORKLIST_PAYLOAD_OFFSET_BYTES + walker * RECT_WORKLIST_INDIRECT_BYTES;
        ok &= direct_rcs_push_rect_worklist_walker(
            batch,
            &mut cursor,
            payload_offset,
            if one_group_per_descriptor {
                local_count as u32
            } else {
                1
            },
            if one_group_per_descriptor {
                GPGPU_WALKER_SIMD16_MASK
            } else {
                simd16_right_mask(local_count as u32)
            },
        );
    }
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_FLUSH_BITS);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        RECT_WORKLIST_POST_MARKER_SLOT,
        post_marker,
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

fn direct_rcs_push_gpgpu_dispatch_prologue(
    batch: &mut [u32],
    cursor: &mut usize,
    upload: UploadedKernelArtifact,
    batch_gpu: u64,
) -> bool {
    direct_rcs_push_gpgpu_dispatch_prologue_with_vfe_dw5(
        batch,
        cursor,
        upload,
        batch_gpu,
        GPGPU_VFE_DW5_UOS,
    )
}

fn direct_rcs_push_gpgpu_dispatch_prologue_with_vfe_dw5(
    batch: &mut [u32],
    cursor: &mut usize,
    upload: UploadedKernelArtifact,
    batch_gpu: u64,
    vfe_dw5: u32,
) -> bool {
    direct_rcs_push_gpgpu_prologue_with_instruction_base(batch, cursor, batch_gpu, upload.gpu, vfe_dw5)
}

fn direct_rcs_push_gpgpu_prologue_with_instruction_base(
    batch: &mut [u32],
    cursor: &mut usize,
    batch_gpu: u64,
    instruction_gpu: u64,
    vfe_dw5: u32,
) -> bool {
    direct_rcs_push_pipe_control_full(
        batch,
        cursor,
        PIPE_CONTROL_HDC_PIPELINE_FLUSH,
        PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH
            | PIPE_CONTROL_DEPTH_CACHE_FLUSH
            | PIPE_CONTROL_DEPTH_STALL
            | PIPE_CONTROL_CS_STALL,
    ) && direct_rcs_push(batch, cursor, PIPELINE_SELECT_GPGPU)
        // Mesa deliberately omits Generic Media State Clear because it hangs
        // gfx12. Gen12.0 also has no DW0 bit11 untyped-flush field, so the
        // supported transition is HDC+CS only.
        && direct_rcs_push_pipe_control_full(
            batch,
            cursor,
            PIPE_CONTROL_HDC_PIPELINE_FLUSH,
            PIPE_CONTROL_CS_STALL,
        )
        && direct_rcs_push(batch, cursor, PIPELINE_SELECT_3D)
        && direct_rcs_push_pipe_control_full(
            batch,
            cursor,
            PIPE_CONTROL_HDC_PIPELINE_FLUSH,
            PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH
                | PIPE_CONTROL_DEPTH_CACHE_FLUSH
                | PIPE_CONTROL_DEPTH_STALL
                | PIPE_CONTROL_CS_STALL,
        )
        && direct_rcs_push_state_base_address(batch, cursor, batch_gpu, batch_gpu, instruction_gpu)
        && direct_rcs_push_pipe_control(batch, cursor, PIPE_CONTROL_INVALIDATE_BITS)
        && direct_rcs_push(batch, cursor, PIPELINE_SELECT_GPGPU)
        && direct_rcs_push_pipe_control_full(batch, cursor, 1 << 9, PIPE_CONTROL_CS_STALL)
        && direct_rcs_push(batch, cursor, MEDIA_VFE_STATE_CMD)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, GPGPU_VFE_DW3_UOS)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, vfe_dw5)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
}

fn direct_rcs_push_gpgpu_dispatch_epilogue(
    batch: &mut [u32],
    cursor: &mut usize,
    result_gpu: u64,
    post_marker_slot: usize,
    post_marker: u32,
) -> bool {
    // The CPU and display must not infer dispatch completion from a later
    // MI_STORE_DATA_IMM.  That store can become observable independently of
    // the dataport/cache release which makes the destination usable.  Keep the
    // full Gen12 HDC/L3 drain as a separate producer release, then make its
    // retirement cookie the post-sync write of an ordered PIPE_CONTROL.  The
    // result allocation is addressed through this context's PPGTT, so
    // PIPE_CONTROL_DEST_GGTT deliberately remains clear.
    direct_rcs_push_pipe_control(batch, cursor, PIPE_CONTROL_FLUSH_BITS)
        && direct_rcs_push_pipe_control_post_sync_marker_at(
            batch,
            cursor,
            result_gpu,
            post_marker_slot,
            post_marker,
        )
        && direct_rcs_push(batch, cursor, MI_BATCH_BUFFER_END)
        && direct_rcs_push(batch, cursor, MI_NOOP)
}

fn direct_rcs_push_gpgpu_dispatch_timestamped_epilogue(
    batch: &mut [u32],
    cursor: &mut usize,
    result_gpu: u64,
    release_timestamp_slot: usize,
    post_marker_slot: usize,
    post_marker: u32,
) -> bool {
    // Preserve the production release fence exactly, then timestamp the point
    // at which its HDC/L3 drain has retired before issuing the completion
    // marker observed by the host.
    direct_rcs_push_pipe_control(batch, cursor, PIPE_CONTROL_FLUSH_BITS)
        && direct_rcs_push_pipe_control_timestamp_at(
            batch,
            cursor,
            result_gpu,
            release_timestamp_slot,
        )
        && direct_rcs_push_pipe_control_post_sync_marker_at(
            batch,
            cursor,
            result_gpu,
            post_marker_slot,
            post_marker,
        )
        && direct_rcs_push(batch, cursor, MI_BATCH_BUFFER_END)
        && direct_rcs_push(batch, cursor, MI_NOOP)
}

fn direct_rcs_encode_rgba8_scanout_release_batch(state: DirectRcsState) -> bool {
    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }
    let batch_len = DIRECT_RCS_BATCH_BYTES / core::mem::size_of::<u32>();
    let batch = unsafe { core::slice::from_raw_parts_mut(state.batch_virt as *mut u32, batch_len) };
    let mut cursor = 0usize;
    let ok = direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_FLUSH_BITS)
        && direct_rcs_push_pipe_control_post_sync_marker_at(
            batch,
            &mut cursor,
            state.gpu_va.result,
            RGBA8_SCANOUT_RELEASE_MARKER_SLOT,
            RGBA8_SCANOUT_RELEASE_MARKER,
        )
        && direct_rcs_push(batch, &mut cursor, MI_BATCH_BUFFER_END)
        && direct_rcs_push(batch, &mut cursor, MI_NOOP);
    if !ok {
        return false;
    }
    super::dma_flush(state.batch_virt, DIRECT_RCS_BATCH_BYTES);
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    true
}

