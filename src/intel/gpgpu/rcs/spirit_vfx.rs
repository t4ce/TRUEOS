fn direct_rcs_write_spirit_vfx_binding(
    state: DirectRcsState,
    binding_table_offset: usize,
    binding_index: usize,
    surface_state_offset: usize,
    gpu: u64,
    bytes: usize,
) -> bool {
    let binding_offset = binding_table_offset + binding_index * core::mem::size_of::<u32>();
    if binding_offset + core::mem::size_of::<u32>() > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    unsafe {
        core::ptr::write_volatile(
            state.batch_virt.add(binding_offset) as *mut u32,
            surface_state_offset as u32,
        );
    }
    direct_rcs_write_buffer_surface_state(state, surface_state_offset, gpu, bytes)
}

fn direct_rcs_write_spirit_vfx_payload(
    state: DirectRcsState,
    payload_offset: usize,
    cross_thread_bytes: usize,
    pointers: &[u64],
) -> bool {
    let indirect_bytes = cross_thread_bytes + SPIRIT_VFX_PER_THREAD_BYTES;
    if payload_offset + indirect_bytes > DIRECT_RCS_BATCH_BYTES || pointers.len() > 3 {
        return false;
    }
    unsafe {
        let payload = state.batch_virt.add(payload_offset);
        core::ptr::write_bytes(payload, 0, indirect_bytes);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        for (index, pointer) in pointers.iter().copied().enumerate() {
            let base = 12 + index * 2;
            core::ptr::write_volatile(dwords.add(base), pointer as u32);
            core::ptr::write_volatile(dwords.add(base + 1), (pointer >> 32) as u32);
        }

        let local_ids = payload.add(cross_thread_bytes) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_push_spirit_vfx_idd_load(
    batch: &mut [u32],
    cursor: &mut usize,
    idd_offset: usize,
) -> bool {
    direct_rcs_push(batch, cursor, MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, SPIRIT_VFX_IDD_BYTES as u32)
        && direct_rcs_push(batch, cursor, idd_offset as u32)
}

fn direct_rcs_push_spirit_vfx_dependency(batch: &mut [u32], cursor: &mut usize) -> bool {
    direct_rcs_push(batch, cursor, MEDIA_STATE_FLUSH_CMD)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push_pipe_control(batch, cursor, PIPE_CONTROL_INVALIDATE_BITS)
}

fn direct_rcs_encode_spirit_vfx_batch(
    state: DirectRcsState,
    background_upload: Option<UploadedKernelArtifact>,
    sprite_upload: UploadedKernelArtifact,
    control: SpiritVfxBuffer,
    source: GpgpuRgba8Surface,
    dst: GpgpuRgba8Surface,
) -> bool {
    let background_valid = match background_upload {
        Some(upload) => {
            upload.bin_sha256 == SPIRIT_VFX_BACKGROUND_RGBA8_ADLS_BIN_SHA256
                && upload.gpu == SPIRIT_VFX_BACKGROUND_RGBA8_ADLS_GPU
                && upload.bytes >= 0x6780
        }
        None => true,
    };
    if !background_valid
        || sprite_upload.bin_sha256 != SPIRIT_VFX_SPRITE_RGBA8_ADLS_BIN_SHA256
        || sprite_upload.gpu != SPIRIT_VFX_SPRITE_RGBA8_ADLS_GPU
        || sprite_upload.bytes < 0xB880
        || dst.width != SPIRIT_VFX_SIZE
        || dst.height != SPIRIT_VFX_SIZE
        || dst.pitch_bytes < SPIRIT_VFX_SIZE * 4
    {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }

    let background_descriptor_ok = background_upload.is_none()
        || direct_rcs_write_interface_descriptor_at(
            state,
            SPIRIT_VFX_BACKGROUND_IDD_OFFSET_BYTES,
            SPIRIT_VFX_BACKGROUND_BINDING_TABLE_OFFSET_BYTES,
            SPIRIT_VFX_BACKGROUND_TEXT_OFFSET_BYTES,
            2,
            2,
        );
    let sprite_text_offset = if background_upload.is_some() {
        SPIRIT_VFX_SPRITE_TEXT_OFFSET_BYTES
    } else {
        SPIRIT_VFX_BACKGROUND_TEXT_OFFSET_BYTES
    };
    let descriptors_ok = background_descriptor_ok
        && direct_rcs_write_interface_descriptor_at(
            state,
            SPIRIT_VFX_SPRITE_IDD_OFFSET_BYTES,
            SPIRIT_VFX_SPRITE_BINDING_TABLE_OFFSET_BYTES,
            sprite_text_offset,
            3,
            3,
        );
    if !descriptors_ok {
        return false;
    }

    let background_bindings_ok = background_upload.is_none()
        || (direct_rcs_write_spirit_vfx_binding(
            state,
            SPIRIT_VFX_BACKGROUND_BINDING_TABLE_OFFSET_BYTES,
            0,
            SPIRIT_VFX_BACKGROUND_CONTROL_SURFACE_OFFSET_BYTES,
            control.gpu,
            control.bytes,
        ) && direct_rcs_write_spirit_vfx_binding(
            state,
            SPIRIT_VFX_BACKGROUND_BINDING_TABLE_OFFSET_BYTES,
            1,
            SPIRIT_VFX_BACKGROUND_DST_SURFACE_OFFSET_BYTES,
            dst.gpu,
            dst.bytes,
        ));
    let bindings_ok = background_bindings_ok
        && direct_rcs_write_spirit_vfx_binding(
            state,
            SPIRIT_VFX_SPRITE_BINDING_TABLE_OFFSET_BYTES,
            0,
            SPIRIT_VFX_SPRITE_SRC_SURFACE_OFFSET_BYTES,
            source.gpu,
            source.bytes,
        )
        && direct_rcs_write_spirit_vfx_binding(
            state,
            SPIRIT_VFX_SPRITE_BINDING_TABLE_OFFSET_BYTES,
            1,
            SPIRIT_VFX_SPRITE_CONTROL_SURFACE_OFFSET_BYTES,
            control.gpu,
            control.bytes,
        )
        && direct_rcs_write_spirit_vfx_binding(
            state,
            SPIRIT_VFX_SPRITE_BINDING_TABLE_OFFSET_BYTES,
            2,
            SPIRIT_VFX_SPRITE_DST_SURFACE_OFFSET_BYTES,
            dst.gpu,
            dst.bytes,
        );
    let background_payload_ok = background_upload.is_none()
        || direct_rcs_write_spirit_vfx_payload(
            state,
            SPIRIT_VFX_BACKGROUND_PAYLOAD_OFFSET_BYTES,
            SPIRIT_VFX_BACKGROUND_CROSS_THREAD_BYTES,
            &[control.gpu, dst.gpu],
        );
    let payloads_ok = background_payload_ok
        && direct_rcs_write_spirit_vfx_payload(
            state,
            SPIRIT_VFX_SPRITE_PAYLOAD_OFFSET_BYTES,
            SPIRIT_VFX_SPRITE_CROSS_THREAD_BYTES,
            &[source.gpu, control.gpu, dst.gpu],
        );
    if !bindings_ok || !payloads_ok {
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
    let instruction_base = background_upload
        .map(|upload| upload.gpu)
        .unwrap_or(sprite_upload.gpu);
    ok &= direct_rcs_push_state_base_address(
        batch,
        &mut cursor,
        state.gpu_va.batch,
        state.gpu_va.batch,
        instruction_base,
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
    ok &= direct_rcs_push_store_marker_at(
        batch,
        &mut cursor,
        state.gpu_va.result,
        SPIRIT_VFX_PRE_MARKER_SLOT,
        SPIRIT_VFX_PRE_MARKER,
    );

    if background_upload.is_some() {
        ok &= direct_rcs_push_spirit_vfx_idd_load(
            batch,
            &mut cursor,
            SPIRIT_VFX_BACKGROUND_IDD_OFFSET_BYTES,
        );
        ok &= direct_rcs_push_gpgpu_walker_2d(
            batch,
            &mut cursor,
            SPIRIT_VFX_BACKGROUND_PAYLOAD_OFFSET_BYTES,
            SPIRIT_VFX_BACKGROUND_INDIRECT_BYTES,
            16,
            SPIRIT_VFX_SIZE,
            GPGPU_WALKER_SIMD16_MASK,
        );
        ok &= direct_rcs_push_spirit_vfx_dependency(batch, &mut cursor);
    }

    ok &=
        direct_rcs_push_spirit_vfx_idd_load(batch, &mut cursor, SPIRIT_VFX_SPRITE_IDD_OFFSET_BYTES);
    ok &= direct_rcs_push_gpgpu_walker_2d(
        batch,
        &mut cursor,
        SPIRIT_VFX_SPRITE_PAYLOAD_OFFSET_BYTES,
        SPIRIT_VFX_SPRITE_INDIRECT_BYTES,
        16,
        SPIRIT_VFX_SIZE,
        GPGPU_WALKER_SIMD16_MASK,
    );
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_FLUSH_BITS);
    ok &= direct_rcs_push_pipe_control_post_sync_marker_at(
        batch,
        &mut cursor,
        state.gpu_va.result,
        SPIRIT_VFX_POST_MARKER_SLOT,
        SPIRIT_VFX_POST_MARKER,
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
