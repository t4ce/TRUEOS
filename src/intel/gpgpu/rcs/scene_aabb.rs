fn submit_tenant_scene_aabb_rcs(
    tenant_root_phys: u64,
    request: crate::gpu::physical::PhysicalSceneAabbRequest,
) -> Result<crate::gpu::physical::PhysicalSceneAabbCompletion, crate::gpu::physical::PhysicalGpuError>
{
    use crate::gpu::physical::PhysicalGpuError;

    if SCENE_AABB_QUARANTINED.load(Ordering::Acquire) {
        return Err(PhysicalGpuError::SubmitFailed);
    }
    let _submit = SCENE_AABB_SUBMIT_LOCK.lock();
    let dev = super::claimed_device().ok_or(PhysicalGpuError::NotReady)?;
    let physical = crate::gpu::physical::physical_device().ok_or(PhysicalGpuError::NotReady)?;
    let upload = upload_scene_aabb_kernel().ok_or(PhysicalGpuError::SubmitFailed)?;
    if upload.address_space != GpgpuArtifactAddressSpace::CallerPpgtt {
        return Err(PhysicalGpuError::MapFailed);
    }
    let state = scene_aabb_rcs_state_once(dev).ok_or(PhysicalGpuError::OutOfMemory)?;

    if !direct_rcs_map_state(dev, state) {
        return Err(PhysicalGpuError::MapFailed);
    }

    let mappings = [
        (state.gpu_va.ring, state.ring_phys, DIRECT_RCS_RING_BYTES),
        (state.gpu_va.batch, state.batch_phys, DIRECT_RCS_BATCH_BYTES),
        (state.gpu_va.result, state.result_phys, DIRECT_RCS_RESULT_BYTES),
        (upload.gpu, upload.phys, upload.mapped_bytes),
    ];
    let mut mapped = 0usize;
    for &(gpu, phys, bytes) in &mappings {
        if let Err(error) = physical.map_gpuvm(request.vm, gpu, phys, bytes) {
            unmap_scene_aabb_ranges(physical, request.vm, &mappings[..mapped]);
            return Err(error);
        }
        mapped += 1;
    }

    if !encode_scene_aabb_batch(state, upload, request) {
        unmap_scene_aabb_ranges(physical, request.vm, &mappings);
        return Err(PhysicalGpuError::SubmitFailed);
    }
    let ring_tail = direct_rcs_append_ring_batch_start(state, 0, state.gpu_va.batch);
    let Some(ring_ctl) = direct_rcs_ring_ctl_value(DIRECT_RCS_RING_BYTES) else {
        unmap_scene_aabb_ranges(physical, request.vm, &mappings);
        return Err(PhysicalGpuError::SubmitFailed);
    };
    if !direct_rcs_init_lrc_context_image_with_root(
        state,
        state.gpu_va.ring as u32,
        ring_tail as u32,
        ring_ctl,
        tenant_root_phys,
    ) {
        unmap_scene_aabb_ranges(physical, request.vm, &mappings);
        return Err(PhysicalGpuError::SubmitFailed);
    }

    let (hwlrca_lo, hwlrca_hi) = guc_rcs_context_descriptor(state.gpu_va.context);
    // Boot installed and invalidated this immutable control window exactly
    // once. Tenant PPGTT mappings are context-private and do not authorize
    // another global GGTT invalidation here.
    core::sync::atomic::fence(Ordering::SeqCst);
    let token = match crate::intel::guc_submission::INTEL_GUC_SCHEDULER.register(
        dev,
        crate::gpu::physical::PhysicalEngineId::RCS0,
        hwlrca_lo,
        hwlrca_hi,
        crate::gpu::physical::PhysicalContextPriority::KernelNormal,
    ) {
        Ok(token) => token,
        Err(error) => {
            if guc_register_may_have_published(error) {
                SCENE_AABB_QUARANTINED.store(true, Ordering::Release);
                crate::log_error!(
                    target: "gpgpu";
                    "intel/gpgpu: scene-aabb register ownership_uncertain=1 error={} mappings=pinned context=quarantined action=retain-until-reset\n",
                    error.name(),
                );
                return Err(PhysicalGpuError::CompletionTimeout);
            }
            unmap_scene_aabb_ranges(physical, request.vm, &mappings);
            return Err(PhysicalGpuError::RegisterFailed);
        }
    };
    let submission = match crate::intel::guc_submission::INTEL_GUC_SCHEDULER.submit(dev, token) {
        Ok(submission) => submission,
        Err(error) => {
            let destroyed = crate::intel::guc_submission::INTEL_GUC_SCHEDULER.destroy(dev, token);
            if matches!(error, crate::intel::guc_submission::GucSubmissionError::DeviceFaulted)
                || destroyed.is_err()
            {
                SCENE_AABB_QUARANTINED.store(true, Ordering::Release);
                crate::log_error!(
                    target: "gpgpu";
                    "intel/gpgpu: scene-aabb submit ownership_uncertain=1 error={} destroy_confirmed={} mappings=pinned context=quarantined action=retain-until-reset\n",
                    error.name(),
                    destroyed.is_ok() as u8,
                );
                return Err(PhysicalGpuError::CompletionTimeout);
            }
            unmap_scene_aabb_ranges(physical, request.vm, &mappings);
            return Err(PhysicalGpuError::SubmitFailed);
        }
    };

    let started = direct_rcs_now_tick();
    let deadline =
        started.saturating_add(direct_rcs_ticks_from_ms(SCENE_AABB_COMPLETION_TIMEOUT_MS));
    let observed = loop {
        let value = direct_rcs_read_result_slot(state, SCENE_AABB_POST_MARKER_SLOT);
        if value == SCENE_AABB_POST_MARKER || direct_rcs_now_tick() >= deadline {
            break value;
        }
        core::hint::spin_loop();
    };
    if observed != SCENE_AABB_POST_MARKER {
        SCENE_AABB_QUARANTINED.store(true, Ordering::Release);
        crate::log_error!(
            target: "gpgpu";
            "intel/gpgpu: scene-aabb completion timeout serial={} root=0x{:X} observed=0x{:08X} expected=0x{:08X} controls=pinned context=quarantined\n",
            submission.serial,
            tenant_root_phys,
            observed,
            SCENE_AABB_POST_MARKER,
        );
        return Err(PhysicalGpuError::CompletionTimeout);
    }

    let hits = direct_rcs_read_result_slot(state, SCENE_AABB_HIT_COUNT_SLOT);
    if let Err(error) = crate::intel::guc_submission::INTEL_GUC_SCHEDULER.destroy(dev, token) {
        SCENE_AABB_QUARANTINED.store(true, Ordering::Release);
        crate::log_error!(
            target: "gpgpu";
            "intel/gpgpu: scene-aabb completed serial={} but context teardown failed error={} mappings=pinned context=quarantined action=retain-until-reset\n",
            submission.serial,
            error.name(),
        );
        return Err(PhysicalGpuError::CompletionTimeout);
    }
    unmap_scene_aabb_ranges(physical, request.vm, &mappings);

    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: scene-aabb completed serial={} rows={} hits={} tenant_ppgtt=0x{:X} exact_root=1 zero_upload=1\n",
        submission.serial,
        request.rows,
        hits,
        tenant_root_phys,
    );
    Ok(crate::gpu::physical::PhysicalSceneAabbCompletion {
        serial: submission.serial,
        hits,
    })
}

const fn guc_register_may_have_published(
    error: crate::intel::guc_submission::GucSubmissionError,
) -> bool {
    use crate::intel::guc_submission::GucSubmissionError;

    matches!(
        error,
        GucSubmissionError::DeviceFaulted
            | GucSubmissionError::InvalidContext
            | GucSubmissionError::OwnershipConflict
            | GucSubmissionError::PriorityConflict
            | GucSubmissionError::PolicyEnqueueRejected
    )
}

fn unmap_scene_aabb_ranges(
    physical: &dyn crate::gpu::physical::PhysicalGpuDevice,
    vm: crate::gpu::physical::PhysicalGpuVmHandle,
    mappings: &[(u64, u64, usize)],
) {
    for &(gpu, _, bytes) in mappings.iter().rev() {
        let _ = physical.unmap_gpuvm(vm, gpu, bytes);
    }
}

fn encode_scene_aabb_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    request: crate::gpu::physical::PhysicalSceneAabbRequest,
) -> bool {
    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }

    if !direct_rcs_write_interface_descriptor_at(
        state,
        SCENE_AABB_IDD_OFFSET_BYTES,
        SCENE_AABB_BINDING_TABLE_OFFSET_BYTES,
        SCENE_AABB_TEXT_OFFSET_BYTES,
        SCENE_AABB_BINDINGS as u32,
        (SCENE_AABB_CROSS_THREAD_BYTES / 32) as u32,
    ) || !write_scene_aabb_surface_states(state, request)
        || !write_scene_aabb_payload(state, request)
    {
        return false;
    }

    let batch_len = DIRECT_RCS_BATCH_BYTES / core::mem::size_of::<u32>();
    let batch = unsafe { core::slice::from_raw_parts_mut(state.batch_virt as *mut u32, batch_len) };
    let mut cursor = 0usize;
    let groups = request.rows.div_ceil(16).max(1);
    let remainder = request.rows & 15;
    let right_mask = if remainder == 0 {
        GPGPU_WALKER_SIMD16_MASK
    } else {
        (1u32 << remainder) - 1
    };
    let mut ok =
        direct_rcs_push_gpgpu_dispatch_prologue(batch, &mut cursor, upload, state.gpu_va.batch);
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, SCENE_AABB_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, SCENE_AABB_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_gpgpu_walker_2d(
        batch,
        &mut cursor,
        SCENE_AABB_PAYLOAD_OFFSET_BYTES,
        SCENE_AABB_INDIRECT_BYTES,
        groups,
        1,
        right_mask,
    );
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push_gpgpu_dispatch_epilogue(
        batch,
        &mut cursor,
        state.gpu_va.result,
        SCENE_AABB_POST_MARKER_SLOT,
        SCENE_AABB_POST_MARKER,
    );
    if !ok {
        return false;
    }
    super::dma_flush(state.batch_virt, DIRECT_RCS_BATCH_BYTES);
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    true
}

fn write_scene_aabb_surface_states(
    state: DirectRcsState,
    request: crate::gpu::physical::PhysicalSceneAabbRequest,
) -> bool {
    let slices = [
        request.bounds[0],
        request.bounds[1],
        request.bounds[2],
        request.bounds[3],
        request.bounds[4],
        request.bounds[5],
        request.liveness,
        request.output,
    ];
    let binding =
        unsafe { state.batch_virt.add(SCENE_AABB_BINDING_TABLE_OFFSET_BYTES) as *mut u32 };
    for (index, slice) in slices.into_iter().enumerate() {
        let surface_offset = SCENE_AABB_SURFACE_STATE_OFFSET_BYTES
            + index * COPY_RECT_SURFACE_STATE_DWORDS * core::mem::size_of::<u32>();
        unsafe {
            core::ptr::write_volatile(binding.add(index), surface_offset as u32);
        }
        if !direct_rcs_write_buffer_surface_state(state, surface_offset, slice.gpu, slice.bytes) {
            return false;
        }
    }
    true
}

fn write_scene_aabb_payload(
    state: DirectRcsState,
    request: crate::gpu::physical::PhysicalSceneAabbRequest,
) -> bool {
    if SCENE_AABB_PAYLOAD_OFFSET_BYTES + SCENE_AABB_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    let pointers = [
        request.bounds[0].gpu,
        request.bounds[1].gpu,
        request.bounds[2].gpu,
        request.bounds[3].gpu,
        request.bounds[4].gpu,
        request.bounds[5].gpu,
        request.liveness.gpu,
        request.output.gpu,
        state.gpu_va.result + (SCENE_AABB_HIT_COUNT_SLOT * 4) as u64,
    ];
    unsafe {
        let payload = state.batch_virt.add(SCENE_AABB_PAYLOAD_OFFSET_BYTES);
        core::ptr::write_bytes(payload, 0, SCENE_AABB_INDIRECT_BYTES);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        for (index, pointer) in pointers.into_iter().enumerate() {
            let dword = 12 + index * 2;
            core::ptr::write_volatile(dwords.add(dword), pointer as u32);
            core::ptr::write_volatile(dwords.add(dword + 1), (pointer >> 32) as u32);
        }
        core::ptr::write_volatile(dwords.add(30), request.rows);
        let values = [
            request.query_min[0],
            request.query_min[1],
            request.query_min[2],
            request.query_max[0],
            request.query_max[1],
            request.query_max[2],
        ];
        for (index, value) in values.into_iter().enumerate() {
            core::ptr::write_volatile(dwords.add(31 + index), value.to_bits());
        }
        let local_ids = payload.add(SCENE_AABB_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}
