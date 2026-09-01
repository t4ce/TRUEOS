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

fn direct_rcs_encode_mandel64_worklist_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: Mandel64WorklistRgba8Params,
    dst_bytes: usize,
    desc_bytes: usize,
) -> bool {
    let desc_count = params.desc_count as usize;
    let walker_count = mandel64_worklist_walker_count(desc_count);
    if desc_count == 0 || walker_count == 0 {
        return false;
    }
    let payload_end =
        RECT_WORKLIST_PAYLOAD_OFFSET_BYTES + walker_count * RECT_WORKLIST_INDIRECT_BYTES;
    if payload_end > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }

    if !direct_rcs_write_mandel64_worklist_interface_descriptor(state) {
        return false;
    }
    if !direct_rcs_write_fill_rect_worklist_surface_states(
        state,
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
            RECT_WORKLIST_PAYLOAD_OFFSET_BYTES + walker * RECT_WORKLIST_INDIRECT_BYTES;
        let payload_params = Mandel64WorklistRgba8Params {
            desc_base: params.desc_base.saturating_add(desc_base as u32),
            desc_count: local_count as u32,
            ..params
        };
        if !direct_rcs_write_mandel64_worklist_payload_at(state, payload_offset, payload_params) {
            return false;
        }
    }

    direct_rcs_encode_rect_worklist_command_stream(
        state,
        upload,
        walker_count,
        desc_count,
        MANDEL64_WORKLIST_PRE_MARKER,
        MANDEL64_WORKLIST_POST_MARKER,
        false,
    )
}

fn direct_rcs_encode_ui4_compose_layers_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: Ui4ComposeLayersParams,
    base_bytes: usize,
    dst_bytes: usize,
    desc_bytes: usize,
) -> bool {
    if params.damage_width == 0
        || params.damage_height == 0
        || params.layer_count as usize > UI4_COMPOSE_LAYERS_MAX_LAYERS
        || RECT_WORKLIST_PAYLOAD_OFFSET_BYTES + UI4_COMPOSE_LAYERS_INDIRECT_BYTES
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
        RECT_WORKLIST_IDD_OFFSET_BYTES,
        RECT_WORKLIST_BINDING_TABLE_OFFSET_BYTES,
        UI4_COMPOSE_LAYERS_RGBA8_TEXT_OFFSET_BYTES,
        3,
        UI4_COMPOSE_LAYERS_CROSS_THREAD_GRFS,
    ) || !direct_rcs_write_alpha_blend_worklist_surface_states_at(
        state,
        RECT_WORKLIST_BINDING_TABLE_OFFSET_BYTES,
        RECT_WORKLIST_SRC_SURFACE_STATE_OFFSET_BYTES,
        RECT_WORKLIST_DST_SURFACE_STATE_OFFSET_BYTES,
        RECT_WORKLIST_DESC_SURFACE_STATE_OFFSET_BYTES,
        params.base_gpu,
        base_bytes,
        params.dst_gpu,
        dst_bytes,
        params.layers_gpu,
        desc_bytes,
    ) || !direct_rcs_write_ui4_compose_layers_payload_at(
        state,
        RECT_WORKLIST_PAYLOAD_OFFSET_BYTES,
        params,
    ) {
        return false;
    }

    let batch_len = DIRECT_RCS_BATCH_BYTES / core::mem::size_of::<u32>();
    let batch = unsafe { core::slice::from_raw_parts_mut(state.batch_virt as *mut u32, batch_len) };
    let mut cursor = 0usize;
    let group_x = params.damage_width.div_ceil(16).max(1);
    let group_y = params.damage_height.max(1);
    let mut ok =
        direct_rcs_push_gpgpu_dispatch_prologue(batch, &mut cursor, upload, state.gpu_va.batch);
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, RECT_WORKLIST_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, RECT_WORKLIST_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_store_marker_at(
        batch,
        &mut cursor,
        state.gpu_va.result,
        SPRITE_QUAD_WORKLIST_PRE_MARKER_SLOT,
        UI4_COMPOSE_LAYERS_PRE_MARKER,
    );
    ok &= direct_rcs_push_gpgpu_walker_2d(
        batch,
        &mut cursor,
        RECT_WORKLIST_PAYLOAD_OFFSET_BYTES,
        UI4_COMPOSE_LAYERS_INDIRECT_BYTES,
        group_x,
        group_y,
        GPGPU_WALKER_SIMD16_MASK,
    );
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push_gpgpu_dispatch_epilogue(
        batch,
        &mut cursor,
        state.gpu_va.result,
        SPRITE_QUAD_WORKLIST_POST_MARKER_SLOT,
        UI4_COMPOSE_LAYERS_POST_MARKER,
    );
    if !ok {
        return false;
    }
    super::dma_flush(state.batch_virt, DIRECT_RCS_BATCH_BYTES);
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    true
}

fn direct_rcs_write_ui4_compose_layers_payload_at(
    state: DirectRcsState,
    payload_offset: usize,
    params: Ui4ComposeLayersParams,
) -> bool {
    if payload_offset + UI4_COMPOSE_LAYERS_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    let Some(known) =
        super::opencl::registry::known_aot_kernel(UI4_COMPOSE_LAYERS_RGBA8_KERNEL_NAME)
    else {
        return false;
    };
    unsafe {
        let payload = state.batch_virt.add(payload_offset);
        core::ptr::write_bytes(payload, 0, UI4_COMPOSE_LAYERS_INDIRECT_BYTES);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        core::ptr::write_volatile(dwords.add(12), params.base_gpu as u32);
        core::ptr::write_volatile(dwords.add(13), (params.base_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(14), params.dst_gpu as u32);
        core::ptr::write_volatile(dwords.add(15), (params.dst_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(16), params.layers_gpu as u32);
        core::ptr::write_volatile(dwords.add(17), (params.layers_gpu >> 32) as u32);

        let cross_thread =
            core::slice::from_raw_parts_mut(payload, UI4_COMPOSE_LAYERS_CROSS_THREAD_BYTES);
        let values = (|| {
            let mut writer = super::opencl::KernelValueWriter::new(known.contract, cross_thread)?;
            writer.set_u32(3, params.base_pitch_bytes)?;
            writer.set_u32(4, params.dst_pitch_bytes)?;
            writer.set_u32(5, params.dst_width)?;
            writer.set_u32(6, params.dst_height)?;
            writer.set_u32(7, params.damage_x)?;
            writer.set_u32(8, params.damage_y)?;
            writer.set_u32(9, params.damage_width)?;
            writer.set_u32(10, params.damage_height)?;
            writer.set_u32(11, params.layer_count)?;
            writer.set_u32(12, params.flags)?;
            writer.finish()?;
            Ok::<(), super::opencl::KernelValueError>(())
        })();
        if values.is_err() {
            return false;
        }

        let local_ids = payload.add(UI4_COMPOSE_LAYERS_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn sprite_quad_worklist_payload_offset(payload_base: usize, descriptor: usize) -> Option<usize> {
    let offset = descriptor
        .checked_mul(SPRITE_QUAD_WORKLIST_PAYLOAD_STRIDE_BYTES)?
        .checked_add(payload_base)?;
    offset
        .is_multiple_of(GPGPU_WALKER_INDIRECT_ALIGNMENT_BYTES)
        .then_some(offset)
}

fn sprite_quad_worklist_payload_end(payload_base: usize, descriptor_count: usize) -> Option<usize> {
    descriptor_count
        .checked_mul(SPRITE_QUAD_WORKLIST_PAYLOAD_STRIDE_BYTES)?
        .checked_add(payload_base)
}

fn direct_rcs_encode_sprite_quad_worklist_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: SpriteQuadWorklistRgba8Params,
    src_bytes: usize,
    dst_bytes: usize,
    desc_bytes: usize,
) -> bool {
    let desc_count = params.desc_count as usize;
    if desc_count == 0 || sprite_quad_worklist_walker_count(desc_count) != desc_count {
        return false;
    }
    let Some(payload_end) = sprite_quad_worklist_payload_end(
        SPRITE_QUAD_WORKLIST_SINGLE_PAYLOAD_BASE_OFFSET_BYTES,
        desc_count,
    ) else {
        return false;
    };
    if payload_end > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }

    if !direct_rcs_write_sprite_quad_worklist_interface_descriptor_at(
        state,
        SPRITE_QUAD_WORKLIST_STATE_BASE_OFFSET_BYTES,
        SPRITE_QUAD_WORKLIST_STATE_BASE_OFFSET_BYTES + SPRITE_QUAD_WORKLIST_RUN_BINDING_REL,
    ) {
        return false;
    }
    if !direct_rcs_write_alpha_blend_worklist_surface_states_at(
        state,
        SPRITE_QUAD_WORKLIST_STATE_BASE_OFFSET_BYTES + SPRITE_QUAD_WORKLIST_RUN_BINDING_REL,
        SPRITE_QUAD_WORKLIST_STATE_BASE_OFFSET_BYTES + SPRITE_QUAD_WORKLIST_RUN_SRC_SURFACE_REL,
        SPRITE_QUAD_WORKLIST_STATE_BASE_OFFSET_BYTES + SPRITE_QUAD_WORKLIST_RUN_DST_SURFACE_REL,
        SPRITE_QUAD_WORKLIST_STATE_BASE_OFFSET_BYTES + SPRITE_QUAD_WORKLIST_RUN_DESC_SURFACE_REL,
        params.src_gpu,
        src_bytes,
        params.dst_gpu,
        dst_bytes,
        params.desc_gpu,
        desc_bytes,
    ) {
        return false;
    }
    for descriptor in 0..desc_count {
        let Some(payload_offset) = sprite_quad_worklist_payload_offset(
            SPRITE_QUAD_WORKLIST_SINGLE_PAYLOAD_BASE_OFFSET_BYTES,
            descriptor,
        ) else {
            return false;
        };
        let payload_params = SpriteQuadWorklistRgba8Params {
            desc_base: params.desc_base.saturating_add(descriptor as u32),
            desc_count: 1,
            ..params
        };
        if !direct_rcs_write_sprite_quad_worklist_payload_at(
            state,
            payload_offset,
            payload_params,
            0,
            0,
        ) {
            return false;
        }
    }

    direct_rcs_encode_sprite_quad_worklist_command_stream(
        state,
        upload,
        params.dst_width,
        params.dst_height,
        desc_count,
    )
}

fn direct_rcs_encode_sprite_quad_worklist_runs_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    dst: GpgpuRgba8Surface,
    desc: GpgpuRectWorklistDescBuffer,
    runs: &[GpgpuSpriteQuadWorklistRun<'_>],
) -> bool {
    if runs.is_empty() || runs.len() > SPRITE_QUAD_WORKLIST_MAX_DESCS {
        return false;
    }
    let total_descs = runs
        .iter()
        .try_fold(0usize, |total, run| total.checked_add(run.descs.len()));
    let Some(total_descs) = total_descs else {
        return false;
    };
    if total_descs == 0 || total_descs > SPRITE_QUAD_WORKLIST_MAX_DESCS {
        return false;
    }
    if runs.iter().any(|run| run.descs.is_empty()) {
        return false;
    }

    let state_bytes = runs
        .len()
        .checked_mul(SPRITE_QUAD_WORKLIST_RUN_STATE_BLOCK_BYTES);
    let Some(state_bytes) = state_bytes else {
        return false;
    };
    let Some(payload_base) =
        align_up(SPRITE_QUAD_WORKLIST_STATE_BASE_OFFSET_BYTES.saturating_add(state_bytes), 0x40)
    else {
        return false;
    };
    let Some(payload_end) = sprite_quad_worklist_payload_end(payload_base, total_descs) else {
        return false;
    };
    if payload_end > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }

    let mut desc_base = 0usize;
    for (run_index, run) in runs.iter().enumerate() {
        let run_base = SPRITE_QUAD_WORKLIST_STATE_BASE_OFFSET_BYTES
            + run_index * SPRITE_QUAD_WORKLIST_RUN_STATE_BLOCK_BYTES;
        let idd_offset = run_base + SPRITE_QUAD_WORKLIST_RUN_IDD_REL;
        let binding_offset = run_base + SPRITE_QUAD_WORKLIST_RUN_BINDING_REL;
        let src_surface_offset = run_base + SPRITE_QUAD_WORKLIST_RUN_SRC_SURFACE_REL;
        let dst_surface_offset = run_base + SPRITE_QUAD_WORKLIST_RUN_DST_SURFACE_REL;
        let desc_surface_offset = run_base + SPRITE_QUAD_WORKLIST_RUN_DESC_SURFACE_REL;
        if !direct_rcs_write_sprite_quad_worklist_interface_descriptor_at(
            state,
            idd_offset,
            binding_offset,
        ) {
            return false;
        }
        if !direct_rcs_write_alpha_blend_worklist_surface_states_at(
            state,
            binding_offset,
            src_surface_offset,
            dst_surface_offset,
            desc_surface_offset,
            run.src.gpu,
            run.src.bytes,
            dst.gpu,
            dst.bytes,
            desc.gpu,
            desc.bytes,
        ) {
            return false;
        }
        for descriptor in 0..run.descs.len() {
            let Some(payload_offset) = sprite_quad_worklist_payload_offset(
                payload_base,
                desc_base.saturating_add(descriptor),
            ) else {
                return false;
            };
            let params = SpriteQuadWorklistRgba8Params {
                src_gpu: run.src.gpu,
                dst_gpu: dst.gpu,
                desc_gpu: desc.gpu,
                src_pitch_bytes: run.src.pitch_bytes,
                dst_pitch_bytes: dst.pitch_bytes,
                src_width: run.src.width,
                src_height: run.src.height,
                dst_width: dst.width,
                dst_height: dst.height,
                desc_base: desc_base.saturating_add(descriptor) as u32,
                desc_count: 1,
            };
            let Some(dispatch) =
                sprite_quad_descriptor_dispatch(run.descs[descriptor], dst.width, dst.height)
            else {
                return false;
            };
            if !direct_rcs_write_sprite_quad_worklist_payload_at(
                state,
                payload_offset,
                params,
                dispatch.global_x,
                dispatch.global_tile_y,
            ) {
                return false;
            }
        }
        desc_base = desc_base.saturating_add(run.descs.len());
    }

    direct_rcs_encode_sprite_quad_worklist_runs_command_stream(
        state,
        upload,
        dst,
        runs,
        payload_base,
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
        && direct_rcs_push_state_base_address(batch, cursor, batch_gpu, batch_gpu, upload.gpu)
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

fn direct_rcs_encode_sprite_quad_worklist_runs_command_stream(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    dst: GpgpuRgba8Surface,
    runs: &[GpgpuSpriteQuadWorklistRun<'_>],
    payload_base: usize,
) -> bool {
    let batch_len = DIRECT_RCS_BATCH_BYTES / core::mem::size_of::<u32>();
    let batch = unsafe { core::slice::from_raw_parts_mut(state.batch_virt as *mut u32, batch_len) };
    let mut cursor = 0usize;
    let mut ok = true;

    ok &= direct_rcs_push_gpgpu_dispatch_prologue(batch, &mut cursor, upload, state.gpu_va.batch);
    ok &= direct_rcs_push_store_marker_at(
        batch,
        &mut cursor,
        state.gpu_va.result,
        SPRITE_QUAD_WORKLIST_PRE_MARKER_SLOT,
        SPRITE_QUAD_WORKLIST_PRE_MARKER,
    );
    let mut descriptor_base = 0usize;
    let total_descriptors = runs
        .iter()
        .fold(0usize, |total, run| total.saturating_add(run.descs.len()));
    let mut submitted_descriptors = 0usize;
    for (run_index, run) in runs.iter().enumerate() {
        let idd_offset = SPRITE_QUAD_WORKLIST_STATE_BASE_OFFSET_BYTES
            + run_index * SPRITE_QUAD_WORKLIST_RUN_STATE_BLOCK_BYTES;
        ok &= direct_rcs_push(batch, &mut cursor, MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD);
        ok &= direct_rcs_push(batch, &mut cursor, 0);
        ok &= direct_rcs_push(batch, &mut cursor, RECT_WORKLIST_IDD_BYTES as u32);
        ok &= direct_rcs_push(batch, &mut cursor, idd_offset as u32);
        for descriptor in 0..run.descs.len() {
            let Some(dispatch) =
                sprite_quad_descriptor_dispatch(run.descs[descriptor], dst.width, dst.height)
            else {
                return false;
            };
            let Some(payload_offset) = sprite_quad_worklist_payload_offset(
                payload_base,
                descriptor_base.saturating_add(descriptor),
            ) else {
                return false;
            };
            ok &= direct_rcs_push_sprite_quad_worklist_walker(
                batch,
                &mut cursor,
                payload_offset,
                dispatch.walker.group_x,
                dispatch.walker.group_y,
                dispatch.walker.right_mask,
            );
            ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
            ok &= direct_rcs_push(batch, &mut cursor, 0);
            submitted_descriptors = submitted_descriptors.saturating_add(1);
            if submitted_descriptors < total_descriptors {
                ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_FLUSH_BITS);
            }
        }
        descriptor_base = descriptor_base.saturating_add(run.descs.len());
    }
    ok &= direct_rcs_push_gpgpu_dispatch_epilogue(
        batch,
        &mut cursor,
        state.gpu_va.result,
        SPRITE_QUAD_WORKLIST_POST_MARKER_SLOT,
        SPRITE_QUAD_WORKLIST_POST_MARKER,
    );

    if !ok
        || cursor.saturating_mul(core::mem::size_of::<u32>())
            > SPRITE_QUAD_WORKLIST_STATE_BASE_OFFSET_BYTES
    {
        return false;
    }

    super::dma_flush(state.batch_virt, DIRECT_RCS_BATCH_BYTES);
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    true
}

fn direct_rcs_encode_sprite_quad_worklist_command_stream(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    dst_width: u32,
    dst_height: u32,
    desc_count: usize,
) -> bool {
    let batch_len = DIRECT_RCS_BATCH_BYTES / core::mem::size_of::<u32>();
    let batch = unsafe { core::slice::from_raw_parts_mut(state.batch_virt as *mut u32, batch_len) };
    let mut cursor = 0usize;
    let mut ok = true;

    ok &= direct_rcs_push_gpgpu_dispatch_prologue(batch, &mut cursor, upload, state.gpu_va.batch);
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, RECT_WORKLIST_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, SPRITE_QUAD_WORKLIST_STATE_BASE_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_store_marker_at(
        batch,
        &mut cursor,
        state.gpu_va.result,
        SPRITE_QUAD_WORKLIST_PRE_MARKER_SLOT,
        SPRITE_QUAD_WORKLIST_PRE_MARKER,
    );
    let Some(dispatch) = sprite_quad_2d_dispatch(dst_width, dst_height) else {
        return false;
    };
    for descriptor in 0..desc_count {
        let Some(payload_offset) = sprite_quad_worklist_payload_offset(
            SPRITE_QUAD_WORKLIST_SINGLE_PAYLOAD_BASE_OFFSET_BYTES,
            descriptor,
        ) else {
            return false;
        };
        ok &= direct_rcs_push_sprite_quad_worklist_walker(
            batch,
            &mut cursor,
            payload_offset,
            dispatch.group_x,
            dispatch.group_y,
            dispatch.right_mask,
        );
        ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
        ok &= direct_rcs_push(batch, &mut cursor, 0);
        if descriptor + 1 < desc_count {
            ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_FLUSH_BITS);
        }
    }
    ok &= direct_rcs_push_gpgpu_dispatch_epilogue(
        batch,
        &mut cursor,
        state.gpu_va.result,
        SPRITE_QUAD_WORKLIST_POST_MARKER_SLOT,
        SPRITE_QUAD_WORKLIST_POST_MARKER,
    );

    if !ok
        || cursor.saturating_mul(core::mem::size_of::<u32>())
            > SPRITE_QUAD_WORKLIST_STATE_BASE_OFFSET_BYTES
    {
        return false;
    }

    super::dma_flush(state.batch_virt, DIRECT_RCS_BATCH_BYTES);
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    true
}
