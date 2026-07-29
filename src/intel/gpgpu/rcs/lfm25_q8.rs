const LFM25_Q8_LEGACY_BASE_COMMAND_DWORDS: usize = 94;
const LFM25_Q8_COMMAND_DWORDS_PER_PROJECTION: usize = 21;
const LFM25_Q8_PHASE_PROBE_EXTRA_COMMAND_DWORDS: usize = 12;

const fn lfm25_q8_result_end_slot(phase_probe_sampled: bool) -> usize {
    if phase_probe_sampled {
        LFM25_Q8_GPU_POST_RELEASE_TIMESTAMP_SLOT + 2
    } else {
        LFM25_Q8_GPU_END_TIMESTAMP_SLOT + 2
    }
}

const fn lfm25_q8_encoded_command_dwords(
    projection_count: usize,
    phase_probe_sampled: bool,
) -> usize {
    LFM25_Q8_LEGACY_BASE_COMMAND_DWORDS
        + projection_count * LFM25_Q8_COMMAND_DWORDS_PER_PROJECTION
        + if phase_probe_sampled {
            LFM25_Q8_PHASE_PROBE_EXTRA_COMMAND_DWORDS
        } else {
            0
        }
}

const _: () = {
    // Keep the unsampled batch byte-for-byte on the legacy command-length and
    // result-flush extent. The sampled variant adds two six-DWord timestamps.
    assert!(lfm25_q8_result_end_slot(false) == LFM25_Q8_GPU_END_TIMESTAMP_SLOT + 2);
    assert!(
        (lfm25_q8_result_end_slot(false) - LFM25_Q8_POST_MARKER_SLOT) * core::mem::size_of::<u32>()
            == 24
    );
    assert!(
        (lfm25_q8_result_end_slot(true) - LFM25_Q8_POST_MARKER_SLOT) * core::mem::size_of::<u32>()
            == 40
    );
    assert!(lfm25_q8_encoded_command_dwords(LFM25_Q8_MAX_BATCH_PROJECTIONS, false) == 157);
    assert!(lfm25_q8_encoded_command_dwords(LFM25_Q8_MAX_BATCH_PROJECTIONS, true) == 169);
    assert!(
        lfm25_q8_encoded_command_dwords(LFM25_Q8_MAX_BATCH_PROJECTIONS, true)
            * core::mem::size_of::<u32>()
            <= LFM25_Q8_COMMAND_BYTES
    );
};

fn direct_rcs_write_lfm25_q8_payload(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: Lfm25Q8ProjectParams,
    payload_offset: usize,
) -> bool {
    if payload_offset + LFM25_Q8_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    let Some(known) = super::opencl::registry::known_aot_kernel(upload.name) else {
        return false;
    };

    unsafe {
        let payload = state.batch_virt.add(payload_offset);
        core::ptr::write_bytes(payload, 0, LFM25_Q8_INDIRECT_BYTES);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        let pointers = [params.weights_gpu, params.activation_gpu, params.output_gpu];
        for (index, pointer) in pointers.into_iter().enumerate() {
            let offset = 12 + index * 2;
            core::ptr::write_volatile(dwords.add(offset), pointer as u32);
            core::ptr::write_volatile(dwords.add(offset + 1), (pointer >> 32) as u32);
        }

        let cross_thread = core::slice::from_raw_parts_mut(payload, LFM25_Q8_CROSS_THREAD_BYTES);
        let values = (|| {
            let mut writer = super::opencl::KernelValueWriter::new(known.contract, cross_thread)?;
            writer.set_u32(3, params.weight_offset)?;
            writer.set_u32(4, params.columns)?;
            writer.set_u32(5, params.rows)?;
            writer.finish()?;
            Ok::<(), super::opencl::KernelValueError>(())
        })();
        if values.is_err() {
            return false;
        }

        let local_ids = payload.add(LFM25_Q8_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

const fn lfm25_q8_state_offset(dispatch: usize, relative: usize) -> usize {
    LFM25_Q8_STATE_BASE_OFFSET_BYTES + dispatch * LFM25_Q8_STATE_STRIDE_BYTES + relative
}

fn direct_rcs_write_lfm25_q8_dispatch_state(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: Lfm25Q8ProjectParams,
    dispatch: usize,
) -> bool {
    let idd_offset = lfm25_q8_state_offset(dispatch, LFM25_Q8_IDD_RELATIVE_OFFSET_BYTES);
    let binding_offset =
        lfm25_q8_state_offset(dispatch, LFM25_Q8_BINDING_TABLE_RELATIVE_OFFSET_BYTES);
    let weights_surface_offset =
        lfm25_q8_state_offset(dispatch, LFM25_Q8_WEIGHTS_SURFACE_STATE_RELATIVE_OFFSET_BYTES);
    let activation_surface_offset =
        lfm25_q8_state_offset(dispatch, LFM25_Q8_ACTIVATION_SURFACE_STATE_RELATIVE_OFFSET_BYTES);
    let output_surface_offset =
        lfm25_q8_state_offset(dispatch, LFM25_Q8_OUTPUT_SURFACE_STATE_RELATIVE_OFFSET_BYTES);
    let payload_offset = lfm25_q8_state_offset(dispatch, LFM25_Q8_PAYLOAD_RELATIVE_OFFSET_BYTES);
    let cross_thread_grfs = LFM25_Q8_CROSS_THREAD_BYTES.div_ceil(32) as u32;
    let text_offset = match upload.name {
        LFM25_Q8_PROJECT_KERNEL_NAME => LFM25_Q8_PROJECT_TEXT_OFFSET_BYTES,
        LFM25_Q8_PROJECT_PACKED_KERNEL_NAME => LFM25_Q8_PROJECT_PACKED_TEXT_OFFSET_BYTES,
        _ => return false,
    };

    if !direct_rcs_write_interface_descriptor_at(
        state,
        idd_offset,
        binding_offset,
        text_offset,
        3,
        cross_thread_grfs,
    ) {
        return false;
    }
    unsafe {
        let binding = state.batch_virt.add(binding_offset) as *mut u32;
        core::ptr::write_volatile(binding, weights_surface_offset as u32);
        core::ptr::write_volatile(binding.add(1), activation_surface_offset as u32);
        core::ptr::write_volatile(binding.add(2), output_surface_offset as u32);
    }
    direct_rcs_write_buffer_surface_state_with_mocs_index(
        state,
        weights_surface_offset,
        params.weights_gpu,
        params.model_bytes,
        LFM25_RENDER_MOCS_INDEX,
    ) && direct_rcs_write_buffer_surface_state_with_mocs_index(
        state,
        activation_surface_offset,
        params.activation_gpu,
        params.activation_bytes,
        LFM25_RENDER_MOCS_INDEX,
    ) && direct_rcs_write_buffer_surface_state_with_mocs_index(
        state,
        output_surface_offset,
        params.output_gpu,
        params.output_bytes,
        LFM25_RENDER_MOCS_INDEX,
    ) && direct_rcs_write_lfm25_q8_payload(state, upload, params, payload_offset)
}

fn lfm25_q8_project_upload_valid(upload: UploadedKernelArtifact) -> bool {
    match upload.name {
        LFM25_Q8_PROJECT_KERNEL_NAME => {
            upload.bin_sha256 == LFM25_Q8_PROJECT_ADLS_BIN_SHA256
                && upload.gpu == LFM25_Q8_PROJECT_ADLS_GPU
                && upload.bytes == LFM25_Q8_PROJECT_ADLS_BIN.len()
        }
        LFM25_Q8_PROJECT_PACKED_KERNEL_NAME => {
            upload.bin_sha256 == LFM25_Q8_PROJECT_PACKED_ADLS_BIN_SHA256
                && upload.gpu == LFM25_Q8_PROJECT_PACKED_ADLS_GPU
                && upload.bytes == LFM25_Q8_PROJECT_PACKED_ADLS_BIN.len()
        }
        _ => false,
    }
}

fn direct_rcs_encode_lfm25_q8_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    params: &[Lfm25Q8ProjectParams],
    phase_probe_sampled: bool,
) -> bool {
    if params.is_empty()
        || params.len() > LFM25_Q8_MAX_BATCH_PROJECTIONS
        || !lfm25_q8_project_upload_valid(upload)
        || params.iter().any(|params| {
            let blocks = params.columns as usize / trueos_lfm25_cpu::Q8_BLOCK_VALUES;
            let activation_bytes = if upload.name == LFM25_Q8_PROJECT_PACKED_KERNEL_NAME {
                blocks * (core::mem::size_of::<u32>() * 9)
            } else {
                blocks * trueos_lfm25_cpu::Q8_BLOCK_BYTES
            };
            !lfm25_q8_admitted_shape(params.columns, params.rows)
                || params.activation_bytes != activation_bytes
                || params.output_bytes != params.rows as usize * core::mem::size_of::<f32>()
        })
    {
        return false;
    }

    let state_bytes = params.len() * LFM25_Q8_STATE_STRIDE_BYTES;
    unsafe {
        // The LFM command stream occupies the first page and its dispatch
        // state lives in a compact private region. Do not dirty or CLFLUSH the
        // remaining 220+ KiB of the generic RCS batch allocation.
        core::ptr::write_bytes(state.batch_virt, 0, LFM25_Q8_COMMAND_BYTES);
        core::ptr::write_bytes(
            state.batch_virt.add(LFM25_Q8_STATE_BASE_OFFSET_BYTES),
            0,
            state_bytes,
        );

        // The post-sync command writes a QWord at slot 44. Slot 45 is also the
        // pre-marker. Clear that pair and the ordered timestamp samples. The
        // two diagnostic samples are touched only for a selected submission.
        let result_end_slot = lfm25_q8_result_end_slot(phase_probe_sampled);
        let marker = state
            .result_virt
            .add(LFM25_Q8_POST_MARKER_SLOT * core::mem::size_of::<u32>());
        core::ptr::write_bytes(
            marker,
            0,
            (result_end_slot - LFM25_Q8_POST_MARKER_SLOT) * core::mem::size_of::<u32>(),
        );
    }

    for (dispatch, params) in params.iter().copied().enumerate() {
        if !direct_rcs_write_lfm25_q8_dispatch_state(state, upload, params, dispatch) {
            return false;
        }
    }

    let batch_len = LFM25_Q8_COMMAND_BYTES / core::mem::size_of::<u32>();
    let batch = unsafe { core::slice::from_raw_parts_mut(state.batch_virt as *mut u32, batch_len) };
    let mut cursor = 0usize;
    let mut ok = true;
    if phase_probe_sampled {
        // First command in the sampled batch. Host pre-submit and this sample
        // share the render timestamp domain, so their interval includes GuC
        // queue/admission plus dispatch latency without a clock conversion.
        ok &= direct_rcs_push_pipe_control_timestamp_at(
            batch,
            &mut cursor,
            state.gpu_va.result,
            LFM25_Q8_GPU_BATCH_ENTER_TIMESTAMP_SLOT,
        );
    }
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
    ok &= direct_rcs_push_state_base_address_with_mocs_index(
        batch,
        &mut cursor,
        state.gpu_va.batch,
        state.gpu_va.batch,
        upload.gpu,
        LFM25_RENDER_MOCS_INDEX,
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
        LFM25_Q8_PRE_MARKER_SLOT,
        LFM25_Q8_PRE_MARKER,
    );
    ok &= direct_rcs_push_pipe_control_timestamp_at(
        batch,
        &mut cursor,
        state.gpu_va.result,
        LFM25_Q8_GPU_START_TIMESTAMP_SLOT,
    );
    for (dispatch, params) in params.iter().copied().enumerate() {
        let idd_offset = lfm25_q8_state_offset(dispatch, LFM25_Q8_IDD_RELATIVE_OFFSET_BYTES);
        let payload_offset =
            lfm25_q8_state_offset(dispatch, LFM25_Q8_PAYLOAD_RELATIVE_OFFSET_BYTES);
        ok &= direct_rcs_push(batch, &mut cursor, MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD);
        ok &= direct_rcs_push(batch, &mut cursor, 0);
        ok &= direct_rcs_push(batch, &mut cursor, LFM25_Q8_IDD_BYTES as u32);
        ok &= direct_rcs_push(batch, &mut cursor, idd_offset as u32);
        ok &= direct_rcs_push_gpgpu_walker_2d(
            batch,
            &mut cursor,
            payload_offset,
            LFM25_Q8_INDIRECT_BYTES,
            params.rows / 16,
            1,
            GPGPU_WALKER_SIMD16_MASK,
        );
        ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
        ok &= direct_rcs_push(batch, &mut cursor, 0);
    }
    ok &= direct_rcs_push_pipe_control_timestamp_at(
        batch,
        &mut cursor,
        state.gpu_va.result,
        LFM25_Q8_GPU_END_TIMESTAMP_SLOT,
    );
    // Preserve the production release fence exactly. A sampled submission
    // timestamps its retirement before the existing completion marker.
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_FLUSH_BITS);
    if phase_probe_sampled {
        ok &= direct_rcs_push_pipe_control_timestamp_at(
            batch,
            &mut cursor,
            state.gpu_va.result,
            LFM25_Q8_GPU_POST_RELEASE_TIMESTAMP_SLOT,
        );
    }
    ok &= direct_rcs_push_pipe_control_post_sync_marker_at(
        batch,
        &mut cursor,
        state.gpu_va.result,
        LFM25_Q8_POST_MARKER_SLOT,
        LFM25_Q8_POST_MARKER,
    );
    ok &= direct_rcs_push(batch, &mut cursor, MI_BATCH_BUFFER_END);
    ok &= direct_rcs_push(batch, &mut cursor, MI_NOOP);
    if !ok {
        return false;
    }
    debug_assert_eq!(cursor, lfm25_q8_encoded_command_dwords(params.len(), phase_probe_sampled));

    super::dma_flush(state.batch_virt, cursor * core::mem::size_of::<u32>());
    unsafe {
        super::dma_flush(state.batch_virt.add(LFM25_Q8_STATE_BASE_OFFSET_BYTES), state_bytes);
        let result_end_slot = lfm25_q8_result_end_slot(phase_probe_sampled);
        super::dma_flush(
            state
                .result_virt
                .add(LFM25_Q8_POST_MARKER_SLOT * core::mem::size_of::<u32>()),
            (result_end_slot - LFM25_Q8_POST_MARKER_SLOT) * core::mem::size_of::<u32>(),
        );
    }
    true
}
