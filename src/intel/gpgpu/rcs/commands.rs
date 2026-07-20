fn direct_rcs_push(batch: &mut [u32], cursor: &mut usize, value: u32) -> bool {
    if *cursor >= batch.len() {
        return false;
    }
    batch[*cursor] = value;
    *cursor += 1;
    true
}

fn direct_rcs_push_pipe_control_full(
    batch: &mut [u32],
    cursor: &mut usize,
    header_flags: u32,
    dw1_flags: u32,
) -> bool {
    direct_rcs_push(batch, cursor, PIPE_CONTROL_CMD | header_flags)
        && direct_rcs_push(batch, cursor, dw1_flags)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
}

fn direct_rcs_push_pipe_control(batch: &mut [u32], cursor: &mut usize, flags: u32) -> bool {
    // Every caller is a GPGPU cache flush/invalidate boundary.  Drain HDC/LSC
    // in DW0 as required before the DW1 cache operation can be considered a
    // producer/consumer fence across GuC contexts.
    direct_rcs_push_pipe_control_full(batch, cursor, PIPE_CONTROL_HDC_PIPELINE_FLUSH, flags)
}

fn direct_rcs_push_pipe_control_post_sync_marker_at(
    batch: &mut [u32],
    cursor: &mut usize,
    result_gpu: u64,
    slot: usize,
    value: u32,
) -> bool {
    // PIPE_CONTROL post-sync writes a QWord. Keep the destination naturally
    // aligned and reserve the following result slot for its high DWORD.
    if slot & 1 != 0 {
        return false;
    }
    let dst = result_gpu + (slot as u64) * core::mem::size_of::<u32>() as u64;
    direct_rcs_push(batch, cursor, PIPE_CONTROL_CMD)
        && direct_rcs_push(
            batch,
            cursor,
            PIPE_CONTROL_FLUSH_ENABLE
                | PIPE_CONTROL_CS_STALL
                | PIPE_CONTROL_POST_SYNC_WRITE_IMMEDIATE,
        )
        && direct_rcs_push(batch, cursor, dst as u32)
        && direct_rcs_push(batch, cursor, (dst >> 32) as u32)
        && direct_rcs_push(batch, cursor, value)
        && direct_rcs_push(batch, cursor, 0)
}

fn direct_rcs_push_store_marker(
    batch: &mut [u32],
    cursor: &mut usize,
    slot: usize,
    value: u32,
) -> bool {
    direct_rcs_push_store_marker_at(batch, cursor, DIRECT_RCS_GPU_VA_RESULT_BASE, slot, value)
}

fn direct_rcs_push_store_marker_at(
    batch: &mut [u32],
    cursor: &mut usize,
    result_gpu: u64,
    slot: usize,
    value: u32,
) -> bool {
    let dst = result_gpu + (slot as u64) * core::mem::size_of::<u32>() as u64;
    direct_rcs_push(batch, cursor, MI_STORE_DATA_IMM_GGTT_DW1)
        && direct_rcs_push(batch, cursor, dst as u32)
        && direct_rcs_push(batch, cursor, (dst >> 32) as u32)
        && direct_rcs_push(batch, cursor, value)
}

fn direct_rcs_push_gpgpu_walker_2d(
    batch: &mut [u32],
    cursor: &mut usize,
    payload_offset: usize,
    indirect_bytes: usize,
    group_x: u32,
    group_y: u32,
    right_mask: u32,
) -> bool {
    if group_x == 0 || group_y == 0 || right_mask == 0 {
        return false;
    }
    direct_rcs_push(batch, cursor, GPGPU_WALKER_CMD)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, indirect_bytes as u32)
        && direct_rcs_push(batch, cursor, payload_offset as u32)
        && direct_rcs_push(
            batch,
            cursor,
            (GPGPU_WALKER_SIMD16_SELECT << 30) | (GPGPU_WALKER_GROUP_THREADS - 1),
        )
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, group_x)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, group_y)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_GROUP_Z_DIM)
        && direct_rcs_push(batch, cursor, right_mask)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_BOTTOM_MASK)
}

fn direct_rcs_push_rect_worklist_walker(
    batch: &mut [u32],
    cursor: &mut usize,
    payload_offset: usize,
    group_x: u32,
    right_mask: u32,
) -> bool {
    if group_x == 0 || group_x as usize > RECT_WORKLIST_DESCS_PER_WALKER {
        return false;
    }
    direct_rcs_push(batch, cursor, GPGPU_WALKER_CMD)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, RECT_WORKLIST_INDIRECT_BYTES as u32)
        && direct_rcs_push(batch, cursor, payload_offset as u32)
        && direct_rcs_push(
            batch,
            cursor,
            (GPGPU_WALKER_SIMD16_SELECT << 30) | (GPGPU_WALKER_GROUP_THREADS - 1),
        )
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, group_x)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 1)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_GROUP_Z_DIM)
        && direct_rcs_push(batch, cursor, right_mask)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_BOTTOM_MASK)
}

fn direct_rcs_push_sprite_quad_worklist_walker(
    batch: &mut [u32],
    cursor: &mut usize,
    payload_offset: usize,
    group_x: u32,
    group_y: u32,
    right_mask: u32,
) -> bool {
    if group_x == 0 || group_y == 0 || group_x as usize > SPRITE_QUAD_WORKLIST_MAX_GROUPS_PER_WALKER
    {
        return false;
    }
    direct_rcs_push(batch, cursor, GPGPU_WALKER_CMD)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, SPRITE_QUAD_WORKLIST_INDIRECT_BYTES as u32)
        && direct_rcs_push(batch, cursor, payload_offset as u32)
        && direct_rcs_push(
            batch,
            cursor,
            (GPGPU_WALKER_SIMD16_SELECT << 30) | (GPGPU_WALKER_GROUP_THREADS - 1),
        )
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, group_x)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, group_y)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_GROUP_Z_DIM)
        && direct_rcs_push(batch, cursor, right_mask)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_BOTTOM_MASK)
}

fn direct_rcs_push_state_base_address(
    batch: &mut [u32],
    cursor: &mut usize,
    indirect_object_base: u64,
    dynamic_state_base: u64,
    instruction_base: u64,
) -> bool {
    direct_rcs_push(batch, cursor, STATE_BASE_ADDRESS_CMD)
        && direct_rcs_push_sba_address(batch, cursor, true, RENDER_MOCS, indirect_object_base)
        && direct_rcs_push(batch, cursor, RENDER_MOCS << 16)
        && direct_rcs_push_sba_address(batch, cursor, true, RENDER_MOCS, dynamic_state_base)
        && direct_rcs_push_sba_address(batch, cursor, true, RENDER_MOCS, dynamic_state_base)
        && direct_rcs_push_sba_address(batch, cursor, true, RENDER_MOCS, indirect_object_base)
        && direct_rcs_push_sba_address(batch, cursor, true, RENDER_MOCS, instruction_base)
        && direct_rcs_push_sba_size(batch, cursor, true, 0xFFFF_F000)
        && direct_rcs_push_sba_size(batch, cursor, true, 0xFFFF_F000)
        && direct_rcs_push_sba_size(batch, cursor, true, 0xFFFF_F000)
        && direct_rcs_push_sba_size(batch, cursor, true, 0xFFFF_F000)
        && direct_rcs_push_sba_address(batch, cursor, true, RENDER_MOCS, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push_sba_address(batch, cursor, true, RENDER_MOCS, 0)
        && direct_rcs_push(batch, cursor, 0)
}

fn direct_rcs_push_sba_address(
    batch: &mut [u32],
    cursor: &mut usize,
    enable: bool,
    mocs: u32,
    address: u64,
) -> bool {
    let low = ((address as u32) & 0xFFFF_F000) | (mocs << 4) | u32::from(enable);
    direct_rcs_push(batch, cursor, low) && direct_rcs_push(batch, cursor, (address >> 32) as u32)
}

fn direct_rcs_push_sba_size(
    batch: &mut [u32],
    cursor: &mut usize,
    enable: bool,
    size_bytes: usize,
) -> bool {
    let Some(size_bytes) = align_up(size_bytes, 4096) else {
        return false;
    };
    let Ok(size_bytes) = u32::try_from(size_bytes) else {
        return false;
    };
    direct_rcs_push(batch, cursor, (size_bytes & 0xFFFF_F000) | u32::from(enable))
}

fn direct_rcs_submit_batch(dev: super::Dev, state: DirectRcsState) -> bool {
    if DIRECT_RCS_CONTEXT_QUARANTINED.load(Ordering::Acquire) {
        return false;
    }
    let mut runtime = DIRECT_RCS_SUBMIT_RUNTIME.lock();
    direct_rcs_submit_batch_for(dev, state, &mut runtime, crate::gpu::vgpu::KernelClient::Gpgpu)
}

fn quarantine_direct_rcs_context(reason: &'static str) {
    if DIRECT_RCS_CONTEXT_QUARANTINED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        crate::log_error!(
            target: "gpgpu";
            "intel/gpgpu: direct-rcs context quarantined reason={} action=reject-future-direct-submits-until-reboot late-batch-reuse=forbidden\n",
            reason,
        );
    }
}

fn direct_rcs_submit_batch_for(
    dev: super::Dev,
    state: DirectRcsState,
    runtime: &mut DirectRcsSubmitRuntime,
    client: crate::gpu::vgpu::KernelClient,
) -> bool {
    if runtime.pending.is_some() {
        return false;
    }
    // The GuC owns one persistent logical context for the direct-RCS client.
    // Its ring must therefore be persistent as well: publishing the same tail
    // for every request does not describe new work once the first request has
    // advanced the saved ring head. Append one BBS entry and advance the tail
    // instead of rebuilding the registered context at offset zero.
    let old_tail_bytes = runtime.ring_tail_bytes;
    let ring_tail_bytes =
        direct_rcs_append_ring_batch_start(state, old_tail_bytes, state.gpu_va.batch);
    let Some(ring_ctl) = direct_rcs_ring_ctl_value(DIRECT_RCS_RING_BYTES) else {
        return false;
    };
    if !runtime.context_initialized {
        if !direct_rcs_init_lrc_context_image(
            state,
            state.gpu_va.ring as u32,
            ring_tail_bytes as u32,
            ring_ctl,
        ) {
            return false;
        }
        runtime.context_initialized = true;
    } else {
        direct_rcs_write_lrc_ring_tail(state, ring_tail_bytes as u32);
    }
    let (context_desc_lo, context_desc_hi) = guc_rcs_context_descriptor(state.gpu_va.context);
    super::ggtt_invalidate(dev);
    core::sync::atomic::fence(Ordering::SeqCst);
    let descriptor = crate::gpu::physical::PhysicalContextDescriptor {
        engine: crate::gpu::physical::EngineClass::RenderCompute,
        hwlrca_lo: context_desc_lo,
        hwlrca_hi: context_desc_hi,
        gpuvm_root_phys: state.ppgtt_phys,
    };
    match crate::gpu::executor::submit_kernel_context(client, descriptor) {
        Ok(submission) => {
            runtime.ring_tail_bytes = ring_tail_bytes;
            runtime.pending = Some(submission);
            true
        }
        Err(error) => {
            // The entry was not admitted. Keep the software tail at the last
            // accepted position so a retry cannot silently skip ring space.
            direct_rcs_write_lrc_ring_tail(state, old_tail_bytes as u32);
            crate::log!(
                "gpgpu/vgpu: submit failed error={:?} submission_owner=gpu-executor/vgpu/guc direct_elsp=0\n",
                error
            );
            false
        }
    }
}

fn complete_direct_rcs_submission(completed: bool) {
    let submission = DIRECT_RCS_SUBMIT_RUNTIME.lock().pending.take();
    if let Some(submission) = submission {
        let _ = crate::gpu::executor::complete_kernel_submission(submission, completed);
    }
}

fn direct_rcs_append_ring_batch_start(
    state: DirectRcsState,
    ring_tail_bytes: usize,
    batch_gpu_addr: u64,
) -> usize {
    debug_assert_eq!(ring_tail_bytes % (DIRECT_RCS_BATCH_START_DWORDS * 4), 0);
    debug_assert!(ring_tail_bytes < DIRECT_RCS_RING_BYTES);
    let start = ring_tail_bytes / core::mem::size_of::<u32>();
    unsafe {
        let dwords = state.ring_virt as *mut u32;
        core::ptr::write_volatile(dwords.add(start), MI_BATCH_BUFFER_START_GEN8 | MI_BATCH_GTT);
        core::ptr::write_volatile(dwords.add(start + 1), batch_gpu_addr as u32);
        core::ptr::write_volatile(dwords.add(start + 2), (batch_gpu_addr >> 32) as u32);
        core::ptr::write_volatile(dwords.add(start + 3), MI_NOOP);
    }
    let tail_bytes = (ring_tail_bytes
        + DIRECT_RCS_BATCH_START_DWORDS * core::mem::size_of::<u32>())
        % DIRECT_RCS_RING_BYTES;
    unsafe {
        super::dma_flush(
            state.ring_virt.add(ring_tail_bytes),
            DIRECT_RCS_BATCH_START_DWORDS * core::mem::size_of::<u32>(),
        );
    }
    tail_bytes
}

fn direct_rcs_poll_result_slot(state: DirectRcsState, slot: usize, expected: u32) -> u32 {
    let mut observed = 0;
    for _ in 0..DIRECT_RCS_SMOKE_POLL_ITERS {
        observed = direct_rcs_read_result_slot(state, slot);
        if observed == expected {
            break;
        }
        core::hint::spin_loop();
    }
    complete_direct_rcs_submission(observed == expected);
    observed
}

fn direct_rcs_poll_result_slot_timeout_ms(
    state: DirectRcsState,
    slot: usize,
    expected: u32,
    timeout_ms: u64,
) -> u32 {
    let started = direct_rcs_now_tick();
    let deadline = started.saturating_add(direct_rcs_ticks_from_ms(timeout_ms));
    let log_probe = DIRECT_RCS_TIMEOUT_POLL_PROBE_LOGGED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok();
    if log_probe {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: marker-poll begin slot={} expected=0x{:08X} timeout_ms={} completion_limit=deadline cache_flush_bytes=4 pause_iters={} worker_slot={}\n",
            slot,
            expected,
            timeout_ms,
            DIRECT_RCS_TIMEOUT_POLL_PAUSE_ITERS,
            crate::percpu::current_slot(),
        );
    }
    let mut iterations = 0usize;
    let observed = loop {
        iterations = iterations.saturating_add(1);
        let observed = direct_rcs_read_result_slot(state, slot);
        if observed == expected {
            break observed;
        }
        if direct_rcs_now_tick() >= deadline {
            break observed;
        }
        for _ in 0..DIRECT_RCS_TIMEOUT_POLL_PAUSE_ITERS {
            core::hint::spin_loop();
        }
    };
    if log_probe {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: marker-poll end slot={} observed=0x{:08X} expected=0x{:08X} matched={} iterations={} elapsed_ms={}\n",
            slot,
            observed,
            expected,
            (observed == expected) as u8,
            iterations,
            direct_rcs_elapsed_ms_since(started),
        );
    }
    complete_direct_rcs_submission(observed == expected);
    observed
}

fn direct_rcs_poll_result_slot_elapsed(
    state: DirectRcsState,
    slot: usize,
    expected: u32,
    start_tick: u64,
) -> (u32, u64) {
    let observed = direct_rcs_poll_result_slot(state, slot, expected);
    (observed, direct_rcs_elapsed_ms_since(start_tick))
}

fn direct_rcs_read_result_slot(state: DirectRcsState, slot: usize) -> u32 {
    let offset = slot.saturating_mul(core::mem::size_of::<u32>());
    if offset + core::mem::size_of::<u32>() > DIRECT_RCS_RESULT_BYTES {
        return 0;
    }
    let marker = unsafe { state.result_virt.add(offset) };
    // CLFLUSH rounds to a cache-line boundary. Invalidating this one marker is
    // sufficient; flushing the full 4 KiB result page on every poll multiplied
    // each check into 64 CLFLUSH operations plus an MFENCE and starved sibling
    // tasks on the same executor core.
    super::dma_flush(marker, core::mem::size_of::<u32>());
    unsafe { core::ptr::read_volatile(marker as *const u32) }
}

fn direct_rcs_now_tick() -> u64 {
    embassy_time_driver::now()
}

fn direct_rcs_ticks_from_ms(ms: u64) -> u64 {
    let hz = embassy_time_driver::TICK_HZ;
    if hz == 0 {
        return ms.max(1);
    }
    let ticks = ((ms as u128).saturating_mul(hz as u128).saturating_add(999) / 1000) as u64;
    if ms == 0 { 0 } else { ticks.max(1) }
}

fn direct_rcs_elapsed_ms_since(start_tick: u64) -> u64 {
    let elapsed = direct_rcs_now_tick().saturating_sub(start_tick);
    let hz = embassy_time_driver::TICK_HZ;
    if hz == 0 {
        0
    } else {
        elapsed.saturating_mul(1000) / hz
    }
}

