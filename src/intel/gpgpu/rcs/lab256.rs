fn direct_rcs_write_lab256_bindings(
    state: DirectRcsState,
    binding_table_offset: usize,
    bindings: &[(usize, Lab256Buffer)],
) -> bool {
    let binding_bytes = bindings.len() * core::mem::size_of::<u32>();
    if binding_table_offset + binding_bytes > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    unsafe {
        let table = state.batch_virt.add(binding_table_offset) as *mut u32;
        for (index, (surface_offset, buffer)) in bindings.iter().enumerate() {
            core::ptr::write_volatile(table.add(index), *surface_offset as u32);
            if !direct_rcs_write_buffer_surface_state(
                state,
                *surface_offset,
                buffer.gpu,
                buffer.bytes,
            ) {
                return false;
            }
        }
    }
    true
}

fn direct_rcs_write_lab256_payload(
    state: DirectRcsState,
    payload_offset: usize,
    pointers: &[u64],
    scalar: Option<u32>,
) -> bool {
    if payload_offset + LAB256_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES || pointers.len() > 4 {
        return false;
    }
    unsafe {
        let payload = state.batch_virt.add(payload_offset);
        core::ptr::write_bytes(payload, 0, LAB256_INDIRECT_BYTES);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        for (index, pointer) in pointers.iter().enumerate() {
            let base = 12 + index * 2;
            core::ptr::write_volatile(dwords.add(base), *pointer as u32);
            core::ptr::write_volatile(dwords.add(base + 1), (*pointer >> 32) as u32);
        }
        if let Some(value) = scalar {
            core::ptr::write_volatile(dwords.add(20), value);
        }

        let local_ids = payload.add(LAB256_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_push_lab256_idd_load(
    batch: &mut [u32],
    cursor: &mut usize,
    idd_offset: usize,
) -> bool {
    direct_rcs_push(batch, cursor, MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, LAB256_IDD_BYTES as u32)
        && direct_rcs_push(batch, cursor, idd_offset as u32)
}

fn direct_rcs_push_lab256_dependency(batch: &mut [u32], cursor: &mut usize) -> bool {
    direct_rcs_push(batch, cursor, MEDIA_STATE_FLUSH_CMD)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push_pipe_control(batch, cursor, PIPE_CONTROL_INVALIDATE_BITS)
}

fn direct_rcs_encode_lab256_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    state_in: Lab256Buffer,
    state_out: Lab256Buffer,
    control: Lab256Buffer,
    report: Lab256Buffer,
    dst: GpgpuRgba8Surface,
) -> bool {
    if upload.bin_sha256 != LAB256_MULTIPHASE_ADLS_BIN_SHA256
        || upload.bytes < (LAB256_COMPOSITE_TEXT_OFFSET_BYTES as usize + 0x18C0)
        || dst.width != LAB256_SIZE
        || dst.height != LAB256_SIZE
        || dst.pitch_bytes < LAB256_SIZE * 4
    {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }

    let dst_buffer = Lab256Buffer {
        phys: dst.phys,
        gpu: dst.gpu,
        virt: core::ptr::null_mut(),
        bytes: dst.bytes,
    };
    let descriptors_ok = direct_rcs_write_interface_descriptor_at(
        state,
        LAB256_STEP_IDD_OFFSET_BYTES,
        LAB256_STEP_BINDING_TABLE_OFFSET_BYTES,
        LAB256_STEP_TEXT_OFFSET_BYTES,
        3,
        3,
    ) && direct_rcs_write_interface_descriptor_at(
        state,
        LAB256_REDUCE_IDD_OFFSET_BYTES,
        LAB256_REDUCE_BINDING_TABLE_OFFSET_BYTES,
        LAB256_REDUCE_TEXT_OFFSET_BYTES,
        3,
        3,
    ) && direct_rcs_write_interface_descriptor_at(
        state,
        LAB256_COMPOSITE_IDD_OFFSET_BYTES,
        LAB256_COMPOSITE_BINDING_TABLE_OFFSET_BYTES,
        LAB256_COMPOSITE_TEXT_OFFSET_BYTES,
        4,
        3,
    );
    if !descriptors_ok {
        return false;
    }

    let step_bindings = [
        (LAB256_STEP_STATE_IN_SURFACE_OFFSET_BYTES, state_in),
        (LAB256_STEP_STATE_OUT_SURFACE_OFFSET_BYTES, state_out),
        (LAB256_STEP_CONTROL_SURFACE_OFFSET_BYTES, control),
    ];
    let reduce_bindings = [
        (LAB256_REDUCE_STATE_SURFACE_OFFSET_BYTES, state_out),
        (LAB256_REDUCE_REPORT_SURFACE_OFFSET_BYTES, report),
        (LAB256_REDUCE_CONTROL_SURFACE_OFFSET_BYTES, control),
    ];
    let composite_bindings = [
        (LAB256_COMPOSITE_STATE_SURFACE_OFFSET_BYTES, state_out),
        (LAB256_COMPOSITE_REPORT_SURFACE_OFFSET_BYTES, report),
        (LAB256_COMPOSITE_CONTROL_SURFACE_OFFSET_BYTES, control),
        (LAB256_COMPOSITE_DST_SURFACE_OFFSET_BYTES, dst_buffer),
    ];
    if !direct_rcs_write_lab256_bindings(
        state,
        LAB256_STEP_BINDING_TABLE_OFFSET_BYTES,
        &step_bindings,
    ) || !direct_rcs_write_lab256_bindings(
        state,
        LAB256_REDUCE_BINDING_TABLE_OFFSET_BYTES,
        &reduce_bindings,
    ) || !direct_rcs_write_lab256_bindings(
        state,
        LAB256_COMPOSITE_BINDING_TABLE_OFFSET_BYTES,
        &composite_bindings,
    ) || !direct_rcs_write_lab256_payload(
        state,
        LAB256_STEP_PAYLOAD_OFFSET_BYTES,
        &[state_in.gpu, state_out.gpu, control.gpu],
        None,
    ) || !direct_rcs_write_lab256_payload(
        state,
        LAB256_REDUCE_PAYLOAD_OFFSET_BYTES,
        &[state_out.gpu, report.gpu, control.gpu],
        None,
    ) || !direct_rcs_write_lab256_payload(
        state,
        LAB256_COMPOSITE_PAYLOAD_OFFSET_BYTES,
        &[state_out.gpu, report.gpu, control.gpu, dst.gpu],
        Some(dst.pitch_bytes),
    ) {
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
    ok &= direct_rcs_push_store_marker_at(
        batch,
        &mut cursor,
        state.gpu_va.result,
        LAB256_PRE_MARKER_SLOT,
        LAB256_PRE_MARKER,
    );

    ok &= direct_rcs_push_lab256_idd_load(batch, &mut cursor, LAB256_STEP_IDD_OFFSET_BYTES);
    ok &= direct_rcs_push_gpgpu_walker_2d(
        batch,
        &mut cursor,
        LAB256_STEP_PAYLOAD_OFFSET_BYTES,
        LAB256_INDIRECT_BYTES,
        16,
        LAB256_SIZE,
        GPGPU_WALKER_SIMD16_MASK,
    );
    ok &= direct_rcs_push_lab256_dependency(batch, &mut cursor);

    ok &= direct_rcs_push_lab256_idd_load(batch, &mut cursor, LAB256_REDUCE_IDD_OFFSET_BYTES);
    ok &= direct_rcs_push_gpgpu_walker_2d(
        batch,
        &mut cursor,
        LAB256_REDUCE_PAYLOAD_OFFSET_BYTES,
        LAB256_INDIRECT_BYTES,
        1,
        1,
        GPGPU_WALKER_SIMD16_MASK,
    );
    ok &= direct_rcs_push_lab256_dependency(batch, &mut cursor);

    ok &= direct_rcs_push_lab256_idd_load(batch, &mut cursor, LAB256_COMPOSITE_IDD_OFFSET_BYTES);
    ok &= direct_rcs_push_gpgpu_walker_2d(
        batch,
        &mut cursor,
        LAB256_COMPOSITE_PAYLOAD_OFFSET_BYTES,
        LAB256_INDIRECT_BYTES,
        16,
        LAB256_SIZE,
        GPGPU_WALKER_SIMD16_MASK,
    );
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_FLUSH_BITS);
    ok &= direct_rcs_push_pipe_control_post_sync_marker_at(
        batch,
        &mut cursor,
        state.gpu_va.result,
        LAB256_POST_MARKER_SLOT,
        LAB256_POST_MARKER,
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
