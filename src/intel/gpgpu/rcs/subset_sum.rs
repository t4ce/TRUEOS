fn subset_sum_probe_upload_valid(upload: UploadedKernelArtifact) -> bool {
    upload.name == SUBSET_SUM_COLLAPSE5_MERGE10_KERNEL_NAME
        && upload.bin_sha256 == SUBSET_SUM_COLLAPSE5_MERGE10_ADLS_BIN_SHA256
        && upload.gpu == SUBSET_SUM_COLLAPSE5_MERGE10_ADLS_GPU
        && upload.bytes == SUBSET_SUM_COLLAPSE5_MERGE10_ADLS_BIN.len()
        && upload.address_space == GpgpuArtifactAddressSpace::CallerPpgtt
}

fn direct_rcs_write_subset_sum_probe_payload(
    state: DirectRcsState,
    payload_offset: usize,
    arena_gpu: u64,
    stage: u32,
    leaf_width: u32,
    layout: SubsetSumArenaLayout,
) -> bool {
    if payload_offset + SUBSET_SUM_PROBE_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    let Some(known) =
        super::opencl::registry::known_aot_kernel(SUBSET_SUM_COLLAPSE5_MERGE10_KERNEL_NAME)
    else {
        crate::log_error!(
            target: "gpgpu";
            "intel/gpgpu: subset-sum payload rejected reason=missing-opencl-contract\n"
        );
        return false;
    };

    unsafe {
        let payload = state.batch_virt.add(payload_offset);
        core::ptr::write_bytes(payload, 0, SUBSET_SUM_PROBE_INDIRECT_BYTES);
        let dwords = payload as *mut u32;

        // The admitted walker uses one SIMD16 hardware thread per group.
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        core::ptr::write_volatile(dwords.add(12), arena_gpu as u32);
        core::ptr::write_volatile(dwords.add(13), (arena_gpu >> 32) as u32);

        let cross_thread =
            core::slice::from_raw_parts_mut(payload, SUBSET_SUM_PROBE_CROSS_THREAD_BYTES);
        let values = (|| {
            let mut writer = super::opencl::KernelValueWriter::new(known.contract, cross_thread)?;
            writer.set_u32(1, stage)?;
            writer.set_u32(2, leaf_width)?;
            writer.set_u32(3, layout.leaf_state_count as u32)?;
            writer.set_u32(4, layout.merge_state_count as u32)?;
            writer.set_u32(5, layout.weights_offset_words as u32)?;
            writer.set_u32(6, layout.left_leaf_offset_words as u32)?;
            writer.set_u32(7, layout.right_leaf_offset_words as u32)?;
            writer.set_u32(8, layout.output_offset_words as u32)?;
            writer.finish()?;
            Ok::<(), super::opencl::KernelValueError>(())
        })();
        if let Err(error) = values {
            crate::log_error!(
                target: "gpgpu";
                "intel/gpgpu: subset-sum payload rejected reason=value-contract error={:?}\n",
                error,
            );
            return false;
        }

        let local_ids = payload.add(SUBSET_SUM_PROBE_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_push_subset_sum_probe_dependency(
    batch: &mut [u32],
    cursor: &mut usize,
) -> bool {
    direct_rcs_push(batch, cursor, MEDIA_STATE_FLUSH_CMD)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push_pipe_control(batch, cursor, PIPE_CONTROL_INVALIDATE_BITS)
}

fn direct_rcs_encode_subset_sum_probe_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    arena_gpu: u64,
    arena_bytes: usize,
    leaf_width: usize,
    layout: SubsetSumArenaLayout,
) -> bool {
    let expected_layout = SubsetSumArenaLayout::two_equal_leaves(leaf_width);
    if !subset_sum_probe_upload_valid(upload)
        || arena_gpu != SUBSET_SUM_PROBE_ARENA_GPU
        || arena_bytes != SUBSET_SUM_PROBE_ARENA_ALLOC_BYTES
        || expected_layout != Some(layout)
        || layout.arena_bytes() > arena_bytes
    {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }

    let cross_thread_grfs = SUBSET_SUM_PROBE_CROSS_THREAD_BYTES.div_ceil(32) as u32;
    if !direct_rcs_write_interface_descriptor_at(
        state,
        SUBSET_SUM_PROBE_IDD_OFFSET_BYTES,
        SUBSET_SUM_PROBE_BINDING_TABLE_OFFSET_BYTES,
        SUBSET_SUM_COLLAPSE5_MERGE10_TEXT_OFFSET_BYTES,
        1,
        cross_thread_grfs,
    ) {
        return false;
    }
    unsafe {
        let binding = state
            .batch_virt
            .add(SUBSET_SUM_PROBE_BINDING_TABLE_OFFSET_BYTES) as *mut u32;
        core::ptr::write_volatile(binding, SUBSET_SUM_PROBE_SURFACE_OFFSET_BYTES as u32);
    }
    if !direct_rcs_write_buffer_surface_state(
        state,
        SUBSET_SUM_PROBE_SURFACE_OFFSET_BYTES,
        arena_gpu,
        arena_bytes,
    ) {
        return false;
    }
    for stage in 0..3u32 {
        let payload_offset = SUBSET_SUM_PROBE_PAYLOAD_BASE_OFFSET_BYTES
            + stage as usize * SUBSET_SUM_PROBE_PAYLOAD_STRIDE_BYTES;
        if !direct_rcs_write_subset_sum_probe_payload(
            state,
            payload_offset,
            arena_gpu,
            stage,
            leaf_width as u32,
            layout,
        ) {
            return false;
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
    ok &= direct_rcs_push(batch, &mut cursor, SUBSET_SUM_PROBE_IDD_BYTES as u32);
    ok &= direct_rcs_push(
        batch,
        &mut cursor,
        SUBSET_SUM_PROBE_IDD_OFFSET_BYTES as u32,
    );
    ok &= direct_rcs_push_store_marker_at(
        batch,
        &mut cursor,
        state.gpu_va.result,
        SUBSET_SUM_PROBE_PRE_MARKER_SLOT,
        SUBSET_SUM_PROBE_PRE_MARKER,
    );

    for stage in 0..3usize {
        let work_items = if stage < 2 {
            layout.leaf_state_count
        } else {
            layout.merge_state_count
        };
        ok &= direct_rcs_push_gpgpu_walker_2d(
            batch,
            &mut cursor,
            SUBSET_SUM_PROBE_PAYLOAD_BASE_OFFSET_BYTES
                + stage * SUBSET_SUM_PROBE_PAYLOAD_STRIDE_BYTES,
            SUBSET_SUM_PROBE_INDIRECT_BYTES,
            work_items.div_ceil(16) as u32,
            1,
            GPGPU_WALKER_SIMD16_MASK,
        );
        if stage < 2 {
            ok &= direct_rcs_push_subset_sum_probe_dependency(batch, &mut cursor);
        }
    }
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push_gpgpu_dispatch_epilogue(
        batch,
        &mut cursor,
        state.gpu_va.result,
        SUBSET_SUM_PROBE_POST_MARKER_SLOT,
        SUBSET_SUM_PROBE_POST_MARKER,
    );
    if !ok || cursor * core::mem::size_of::<u32>() > SUBSET_SUM_PROBE_IDD_OFFSET_BYTES {
        return false;
    }

    super::dma_flush(state.batch_virt, DIRECT_RCS_BATCH_BYTES);
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    true
}
