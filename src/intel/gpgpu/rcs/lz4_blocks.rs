const LZ4_IDD: usize = 1024;
const LZ4_BINDING: usize = 1088;
const LZ4_SURFACES: usize = 1152;
const LZ4_PAYLOAD: usize = 1536;
const LZ4_POST_MARKER: u32 = 0x4C5A3401;
const _: () = {
    let c = LZ4_BLOCKS_ADLS_CPP_ABI_CONTRACT;
    assert!(matches!(c.validate(), Ok(())));
    assert!(c.simd_width == 16 && c.scratch_bytes == 0 && c.slm_bytes == 0);
    assert!(c.cross_thread_data_bytes == 96 && c.per_thread_data_bytes == 96);
    assert!(c.bindings.len() == 4 && c.payload_args.len() == 6);
    let mut i = 0;
    while i < 4 {
        assert!(c.bindings[i].arg_index as usize == i && c.bindings[i].bti as usize == i);
        assert!(c.payload_args[i].offset_bytes == 48 + i as u32 * 8);
        assert!(c.payload_args[i].size_bytes == 8);
        i += 1;
    }
    assert!(c.payload_args[4].offset_bytes == 80 && c.payload_args[5].offset_bytes == 84);
};

fn encode_lz4_batch(resources: Lz4Resources, count: u32, mode: u32) -> bool {
    let state = resources.state;
    let upload = resources.upload;
    if count == 0 || count > LZ4_GPU_BATCH_BLOCKS as u32 || mode > 1 {
        return false;
    }
    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }
    if !direct_rcs_write_interface_descriptor_at(
        state,
        LZ4_IDD,
        LZ4_BINDING,
        LZ4_BLOCKS_ADLS_CPP_ABI_CONTRACT.entry_offset as u64,
        4,
        3,
    ) {
        return false;
    }
    let regions = [
        (0, LZ4_REGION_BYTES),
        (LZ4_REGION_BYTES, LZ4_REGION_BYTES),
        (LZ4_DESC_OFFSET, 8192),
        (LZ4_HASH_OFFSET, 256 * 1024),
    ];
    for (index, (offset, bytes)) in regions.into_iter().enumerate() {
        let surface = LZ4_SURFACES + index * 64;
        if !direct_rcs_write_buffer_surface_state(
            state,
            surface,
            LZ4_ARENA_GPU + offset as u64,
            bytes,
        ) {
            return false;
        }
        unsafe {
            core::ptr::write_volatile(
                state.batch_virt.add(LZ4_BINDING + index * 4).cast::<u32>(),
                surface as u32,
            );
            core::ptr::write_unaligned(
                state
                    .batch_virt
                    .add(LZ4_PAYLOAD + 48 + index * 8)
                    .cast::<u64>(),
                LZ4_ARENA_GPU + offset as u64,
            );
        }
    }
    unsafe {
        let payload = state.batch_virt.add(LZ4_PAYLOAD).cast::<u32>();
        for (offset, value) in [
            (3, 16),
            (4, 1),
            (5, 1),
            (8, 16),
            (9, 1),
            (10, 1),
            (20, count),
            (21, mode),
        ] {
            core::ptr::write_volatile(payload.add(offset), value);
        }
        let ids = state.batch_virt.add(LZ4_PAYLOAD + 96).cast::<u16>();
        for lane in 0..16 {
            core::ptr::write_volatile(ids.add(lane), lane as u16);
        }
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
    ok &= direct_rcs_push(batch, &mut cursor, 32 as u32);
    ok &= direct_rcs_push(batch, &mut cursor, LZ4_IDD as u32);
    ok &= direct_rcs_push_store_marker_at(batch, &mut cursor, state.gpu_va.result, 0, 0x4C5A3400);
    ok &= direct_rcs_push_gpgpu_walker_2d(
        batch,
        &mut cursor,
        LZ4_PAYLOAD,
        192,
        count.div_ceil(16),
        1,
        GPGPU_WALKER_SIMD16_MASK,
    );
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push_gpgpu_dispatch_epilogue(
        batch,
        &mut cursor,
        state.gpu_va.result,
        1,
        LZ4_POST_MARKER,
    );
    if !ok || cursor * core::mem::size_of::<u32>() > LZ4_IDD {
        return false;
    }

    super::dma_flush(state.batch_virt, DIRECT_RCS_BATCH_BYTES);
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    true
}
