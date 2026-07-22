fn direct_rcs_write_fill_rect_worklist_interface_descriptor(state: DirectRcsState) -> bool {
    direct_rcs_write_rect_worklist_interface_descriptor(
        state,
        FILL_RECT_WORKLIST_RGBA8_TEXT_OFFSET_BYTES,
        2,
        FILL_RECT_WORKLIST_CROSS_THREAD_GRFS,
    )
}

fn direct_rcs_write_mandel64_worklist_interface_descriptor(state: DirectRcsState) -> bool {
    direct_rcs_write_rect_worklist_interface_descriptor(
        state,
        MANDEL64_WORKLIST_RGBA8_TEXT_OFFSET_BYTES,
        2,
        RECT_WORKLIST_CROSS_THREAD_GRFS,
    )
}

fn direct_rcs_write_sprite_quad_worklist_interface_descriptor(state: DirectRcsState) -> bool {
    direct_rcs_write_rect_worklist_interface_descriptor(
        state,
        SPRITE_QUAD_WORKLIST_RGBA8_TEXT_OFFSET_BYTES,
        3,
        SPRITE_QUAD_WORKLIST_CROSS_THREAD_GRFS,
    )
}

fn direct_rcs_write_sprite_quad_worklist_interface_descriptor_at(
    state: DirectRcsState,
    idd_offset: usize,
    binding_table_offset: usize,
) -> bool {
    direct_rcs_write_rect_worklist_interface_descriptor_at(
        state,
        idd_offset,
        binding_table_offset,
        SPRITE_QUAD_WORKLIST_RGBA8_TEXT_OFFSET_BYTES,
        3,
        SPRITE_QUAD_WORKLIST_CROSS_THREAD_GRFS,
    )
}

fn direct_rcs_write_rect_worklist_interface_descriptor(
    state: DirectRcsState,
    text_offset_bytes: u64,
    binding_table_entries: u32,
    cross_thread_grfs: u32,
) -> bool {
    direct_rcs_write_rect_worklist_interface_descriptor_at(
        state,
        RECT_WORKLIST_IDD_OFFSET_BYTES,
        RECT_WORKLIST_BINDING_TABLE_OFFSET_BYTES,
        text_offset_bytes,
        binding_table_entries,
        cross_thread_grfs,
    )
}

fn direct_rcs_write_rect_worklist_interface_descriptor_at(
    state: DirectRcsState,
    idd_offset: usize,
    binding_table_offset: usize,
    text_offset_bytes: u64,
    binding_table_entries: u32,
    cross_thread_grfs: u32,
) -> bool {
    if idd_offset + RECT_WORKLIST_IDD_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    let idd = unsafe { state.batch_virt.add(idd_offset) as *mut u32 };
    unsafe {
        core::ptr::write_volatile(idd, text_offset_bytes as u32);
        core::ptr::write_volatile(idd.add(1), 0);
        core::ptr::write_volatile(idd.add(2), IDD_THREAD_PREEMPTION_DISABLE);
        core::ptr::write_volatile(idd.add(3), 0);
        core::ptr::write_volatile(
            idd.add(4),
            (binding_table_offset as u32) | binding_table_entries,
        );
        core::ptr::write_volatile(idd.add(5), 3 << 16);
        core::ptr::write_volatile(idd.add(6), GPGPU_WALKER_GROUP_THREADS);
        core::ptr::write_volatile(idd.add(7), cross_thread_grfs);
    }
    true
}

fn direct_rcs_write_fill_rect_worklist_surface_states(
    state: DirectRcsState,
    dst_gpu: u64,
    dst_bytes: usize,
    desc_gpu: u64,
    desc_bytes: usize,
) -> bool {
    let binding_end = RECT_WORKLIST_BINDING_TABLE_OFFSET_BYTES + 2 * core::mem::size_of::<u32>();
    let surface_bytes = COPY_RECT_SURFACE_STATE_DWORDS * core::mem::size_of::<u32>();
    let dst_surface_end = RECT_WORKLIST_DST_SURFACE_STATE_OFFSET_BYTES + surface_bytes;
    let desc_surface_end = RECT_WORKLIST_DESC_SURFACE_STATE_OFFSET_BYTES + surface_bytes;
    if binding_end > DIRECT_RCS_BATCH_BYTES
        || dst_surface_end > DIRECT_RCS_BATCH_BYTES
        || desc_surface_end > DIRECT_RCS_BATCH_BYTES
    {
        return false;
    }

    unsafe {
        let binding = state
            .batch_virt
            .add(RECT_WORKLIST_BINDING_TABLE_OFFSET_BYTES) as *mut u32;
        core::ptr::write_volatile(binding, RECT_WORKLIST_DST_SURFACE_STATE_OFFSET_BYTES as u32);
        core::ptr::write_volatile(
            binding.add(1),
            RECT_WORKLIST_DESC_SURFACE_STATE_OFFSET_BYTES as u32,
        );
    }

    direct_rcs_write_buffer_surface_state(
        state,
        RECT_WORKLIST_DST_SURFACE_STATE_OFFSET_BYTES,
        dst_gpu,
        dst_bytes,
    ) && direct_rcs_write_buffer_surface_state(
        state,
        RECT_WORKLIST_DESC_SURFACE_STATE_OFFSET_BYTES,
        desc_gpu,
        desc_bytes,
    )
}

fn direct_rcs_write_alpha_blend_worklist_surface_states(
    state: DirectRcsState,
    src_gpu: u64,
    src_bytes: usize,
    dst_gpu: u64,
    dst_bytes: usize,
    desc_gpu: u64,
    desc_bytes: usize,
) -> bool {
    direct_rcs_write_alpha_blend_worklist_surface_states_at(
        state,
        RECT_WORKLIST_BINDING_TABLE_OFFSET_BYTES,
        RECT_WORKLIST_SRC_SURFACE_STATE_OFFSET_BYTES,
        RECT_WORKLIST_DST_SURFACE_STATE_OFFSET_BYTES,
        RECT_WORKLIST_DESC_SURFACE_STATE_OFFSET_BYTES,
        src_gpu,
        src_bytes,
        dst_gpu,
        dst_bytes,
        desc_gpu,
        desc_bytes,
    )
}

fn direct_rcs_write_alpha_blend_worklist_surface_states_at(
    state: DirectRcsState,
    binding_table_offset: usize,
    src_surface_state_offset: usize,
    dst_surface_state_offset: usize,
    desc_surface_state_offset: usize,
    src_gpu: u64,
    src_bytes: usize,
    dst_gpu: u64,
    dst_bytes: usize,
    desc_gpu: u64,
    desc_bytes: usize,
) -> bool {
    let binding_end = binding_table_offset + 3 * core::mem::size_of::<u32>();
    let surface_bytes = COPY_RECT_SURFACE_STATE_DWORDS * core::mem::size_of::<u32>();
    let src_surface_end = src_surface_state_offset + surface_bytes;
    let dst_surface_end = dst_surface_state_offset + surface_bytes;
    let desc_surface_end = desc_surface_state_offset + surface_bytes;
    if binding_end > DIRECT_RCS_BATCH_BYTES
        || src_surface_end > DIRECT_RCS_BATCH_BYTES
        || dst_surface_end > DIRECT_RCS_BATCH_BYTES
        || desc_surface_end > DIRECT_RCS_BATCH_BYTES
    {
        return false;
    }

    unsafe {
        let binding = state.batch_virt.add(binding_table_offset) as *mut u32;
        core::ptr::write_volatile(binding, src_surface_state_offset as u32);
        core::ptr::write_volatile(binding.add(1), dst_surface_state_offset as u32);
        core::ptr::write_volatile(binding.add(2), desc_surface_state_offset as u32);
    }

    direct_rcs_write_buffer_surface_state(state, src_surface_state_offset, src_gpu, src_bytes)
        && direct_rcs_write_buffer_surface_state(
            state,
            dst_surface_state_offset,
            dst_gpu,
            dst_bytes,
        )
        && direct_rcs_write_buffer_surface_state(
            state,
            desc_surface_state_offset,
            desc_gpu,
            desc_bytes,
        )
}

fn direct_rcs_write_fill_rect_worklist_payload_at(
    state: DirectRcsState,
    payload_offset: usize,
    params: FillRectWorklistRgba8Params,
) -> bool {
    if payload_offset + RECT_WORKLIST_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        let payload = state.batch_virt.add(payload_offset);
        core::ptr::write_bytes(payload, 0, RECT_WORKLIST_INDIRECT_BYTES);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        core::ptr::write_volatile(dwords.add(8), params.dst_gpu as u32);
        core::ptr::write_volatile(dwords.add(9), (params.dst_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(10), params.desc_gpu as u32);
        core::ptr::write_volatile(dwords.add(11), (params.desc_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(12), params.dst_pitch_bytes);
        core::ptr::write_volatile(dwords.add(13), params.desc_base);
        core::ptr::write_volatile(dwords.add(14), params.desc_count);

        let local_ids = payload.add(FILL_RECT_WORKLIST_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_write_alpha_blend_worklist_payload_at(
    state: DirectRcsState,
    payload_offset: usize,
    params: AlphaBlendWorklistRgba8Params,
) -> bool {
    if payload_offset + ALPHA_BLEND_WORKLIST_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        let payload = state.batch_virt.add(payload_offset);
        core::ptr::write_bytes(payload, 0, ALPHA_BLEND_WORKLIST_INDIRECT_BYTES);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        // The baked artifact's .ze_info places its three 64-bit buffer
        // addresses at byte offsets 32/40/48 and the scalar arguments at
        // 56/60/64/68. Unlike the arbitrary-quad artifact, this 1D kernel has
        // no enqueued_local_size field in its cross-thread payload.
        core::ptr::write_volatile(dwords.add(8), params.src_gpu as u32);
        core::ptr::write_volatile(dwords.add(9), (params.src_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(10), params.dst_gpu as u32);
        core::ptr::write_volatile(dwords.add(11), (params.dst_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(12), params.desc_gpu as u32);
        core::ptr::write_volatile(dwords.add(13), (params.desc_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(14), params.src_pitch_bytes);
        core::ptr::write_volatile(dwords.add(15), params.dst_pitch_bytes);
        core::ptr::write_volatile(dwords.add(16), params.desc_base);
        core::ptr::write_volatile(dwords.add(17), params.desc_count);

        let local_ids = payload.add(ALPHA_BLEND_WORKLIST_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_write_mandel64_worklist_payload_at(
    state: DirectRcsState,
    payload_offset: usize,
    params: Mandel64WorklistRgba8Params,
) -> bool {
    if payload_offset + RECT_WORKLIST_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        let payload = state.batch_virt.add(payload_offset);
        core::ptr::write_bytes(payload, 0, RECT_WORKLIST_INDIRECT_BYTES);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        core::ptr::write_volatile(dwords.add(12), params.dst_gpu as u32);
        core::ptr::write_volatile(dwords.add(13), (params.dst_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(14), params.desc_gpu as u32);
        core::ptr::write_volatile(dwords.add(15), (params.desc_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(16), params.dst_pitch_bytes);
        core::ptr::write_volatile(dwords.add(17), params.desc_base);
        core::ptr::write_volatile(dwords.add(18), params.desc_count);

        let local_ids = payload.add(RECT_WORKLIST_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_write_sprite_quad_worklist_payload_at(
    state: DirectRcsState,
    payload_offset: usize,
    params: SpriteQuadWorklistRgba8Params,
    global_x: u32,
    global_tile_y: u32,
) -> bool {
    if payload_offset + SPRITE_QUAD_WORKLIST_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        let payload = state.batch_virt.add(payload_offset);
        core::ptr::write_bytes(payload, 0, SPRITE_QUAD_WORKLIST_INDIRECT_BYTES);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords, global_x);
        core::ptr::write_volatile(dwords.add(1), global_tile_y);
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        core::ptr::write_volatile(dwords.add(12), params.src_gpu as u32);
        core::ptr::write_volatile(dwords.add(13), (params.src_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(14), params.dst_gpu as u32);
        core::ptr::write_volatile(dwords.add(15), (params.dst_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(16), params.desc_gpu as u32);
        core::ptr::write_volatile(dwords.add(17), (params.desc_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(18), params.src_pitch_bytes);
        core::ptr::write_volatile(dwords.add(19), params.dst_pitch_bytes);
        core::ptr::write_volatile(dwords.add(20), params.src_width);
        core::ptr::write_volatile(dwords.add(21), params.src_height);
        core::ptr::write_volatile(dwords.add(22), params.dst_width);
        core::ptr::write_volatile(dwords.add(23), params.dst_height);
        core::ptr::write_volatile(dwords.add(24), params.desc_base);
        core::ptr::write_volatile(dwords.add(25), params.desc_count);

        let local_ids = payload.add(SPRITE_QUAD_WORKLIST_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_write_fill_rect_interface_descriptor(state: DirectRcsState) -> bool {
    if CLEAR_RECT_IDD_OFFSET_BYTES + CLEAR_RECT_IDD_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    let idd = unsafe { state.batch_virt.add(CLEAR_RECT_IDD_OFFSET_BYTES) as *mut u32 };
    unsafe {
        core::ptr::write_volatile(idd, FILL_RECT_RGBA8_TEXT_OFFSET_BYTES as u32);
        core::ptr::write_volatile(idd.add(1), 0);
        core::ptr::write_volatile(idd.add(2), IDD_THREAD_PREEMPTION_DISABLE);
        core::ptr::write_volatile(idd.add(3), 0);
        core::ptr::write_volatile(idd.add(4), (CLEAR_RECT_BINDING_TABLE_OFFSET_BYTES as u32) | 1);
        core::ptr::write_volatile(idd.add(5), 3 << 16);
        core::ptr::write_volatile(idd.add(6), GPGPU_WALKER_GROUP_THREADS);
        core::ptr::write_volatile(idd.add(7), 3);
    }
    true
}

fn direct_rcs_write_clear_rect_surface_state(
    state: DirectRcsState,
    dst_gpu: u64,
    dst_bytes: usize,
) -> bool {
    let binding_end = CLEAR_RECT_BINDING_TABLE_OFFSET_BYTES + core::mem::size_of::<u32>();
    let surface_bytes = CLEAR_RECT_SURFACE_STATE_DWORDS * core::mem::size_of::<u32>();
    let surface_end = CLEAR_RECT_SURFACE_STATE_OFFSET_BYTES + surface_bytes;
    if binding_end > DIRECT_RCS_BATCH_BYTES || surface_end > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    if dst_bytes == 0 {
        return false;
    }

    let extent = dst_bytes.saturating_sub(1);
    let surface_width_minus1 = (extent & 0x7F) as u32;
    let surface_height_minus1 = ((extent >> 7) & 0x3FFF) as u32;
    let surface_depth_minus1 = ((extent >> 21) & 0x7FF) as u32;
    let surface_dword0 = (SURFTYPE_BUFFER << 29) | (SURFACE_FORMAT_RAW << 18);
    let surface_dword2 = (surface_height_minus1 << 16) | surface_width_minus1;
    let surface_dword3 = surface_depth_minus1 << 21;

    unsafe {
        let binding = state.batch_virt.add(CLEAR_RECT_BINDING_TABLE_OFFSET_BYTES) as *mut u32;
        core::ptr::write_volatile(binding, CLEAR_RECT_SURFACE_STATE_OFFSET_BYTES as u32);

        let surface = state.batch_virt.add(CLEAR_RECT_SURFACE_STATE_OFFSET_BYTES) as *mut u32;
        for index in 0..CLEAR_RECT_SURFACE_STATE_DWORDS {
            core::ptr::write_volatile(surface.add(index), 0);
        }
        core::ptr::write_volatile(surface, surface_dword0);
        core::ptr::write_volatile(surface.add(1), RENDER_MOCS << 24);
        core::ptr::write_volatile(surface.add(2), surface_dword2);
        core::ptr::write_volatile(surface.add(3), surface_dword3);
        core::ptr::write_volatile(surface.add(8), dst_gpu as u32);
        core::ptr::write_volatile(surface.add(9), (dst_gpu >> 32) as u32);
    }
    true
}

fn direct_rcs_write_fill_rect_payload(state: DirectRcsState, params: FillRectRgba8Params) -> bool {
    if CLEAR_RECT_PAYLOAD_OFFSET_BYTES + CLEAR_RECT_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        let payload = state.batch_virt.add(CLEAR_RECT_PAYLOAD_OFFSET_BYTES);
        core::ptr::write_bytes(payload, 0, CLEAR_RECT_INDIRECT_BYTES);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        core::ptr::write_volatile(dwords.add(12), params.dst_gpu as u32);
        core::ptr::write_volatile(dwords.add(13), (params.dst_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(14), params.dst_pitch_bytes);
        core::ptr::write_volatile(dwords.add(15), params.dst_x);
        core::ptr::write_volatile(dwords.add(16), params.dst_y);
        core::ptr::write_volatile(dwords.add(17), params.width);
        core::ptr::write_volatile(dwords.add(18), params.height);
        core::ptr::write_volatile(dwords.add(19), params.color_rgba);

        let local_ids = payload.add(CLEAR_RECT_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn pack_i16_pair_u32(x: i16, y: i16) -> u32 {
    (u16::from_ne_bytes(x.to_ne_bytes()) as u32)
        | ((u16::from_ne_bytes(y.to_ne_bytes()) as u32) << 16)
}

fn pack_u16_pair_u32(x: u16, y: u16) -> u32 {
    (x as u32) | ((y as u32) << 16)
}

fn direct_rcs_read_worklist_probe_span(
    state: DirectRcsState,
    row_index: usize,
    start_pixel: usize,
) -> [u32; 4] {
    super::dma_flush(state.clear_test_virt, CLEAR_RECT_TEST_BYTES);
    let mut values = [0u32; 4];
    unsafe {
        let surface = state.clear_test_virt as *const u32;
        let row = surface.add(row_index * 64);
        for (index, value) in values.iter_mut().enumerate() {
            *value = core::ptr::read_volatile(row.add(start_pixel + index));
        }
    }
    values
}
