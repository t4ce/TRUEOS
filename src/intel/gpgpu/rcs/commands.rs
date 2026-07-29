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

fn direct_rcs_push_pipe_control_timestamp_at(
    batch: &mut [u32],
    cursor: &mut usize,
    result_gpu: u64,
    slot: usize,
) -> bool {
    // PIPE_CONTROL writes a 64-bit command-stream timestamp. CS_STALL makes
    // the two samples ordered around the walker instead of merely describing
    // when their memory transactions became visible.
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
                | PIPE_CONTROL_POST_SYNC_WRITE_TIMESTAMP,
        )
        && direct_rcs_push(batch, cursor, dst as u32)
        && direct_rcs_push(batch, cursor, (dst >> 32) as u32)
        && direct_rcs_push(batch, cursor, 0)
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

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum DirectRcsLane {
    SystemService,
    Execution,
    Lfm25,
}

impl DirectRcsLane {
    const fn name(self) -> &'static str {
        match self {
            Self::SystemService => "system-service",
            Self::Execution => "execution",
            Self::Lfm25 => "lfm25",
        }
    }
}

fn direct_rcs_submit_batch(dev: super::Dev, state: DirectRcsState) -> bool {
    direct_rcs_submit_batch_on_lane(dev, state, DirectRcsLane::SystemService)
}

fn execution_rcs_submit_batch(dev: super::Dev, state: DirectRcsState) -> bool {
    direct_rcs_submit_batch_on_lane(dev, state, DirectRcsLane::Execution)
}

fn lfm25_rcs_submit_batch(dev: super::Dev, state: DirectRcsState) -> bool {
    direct_rcs_submit_batch_on_lane(dev, state, DirectRcsLane::Lfm25)
}

fn direct_rcs_submit_batch_on_lane(
    dev: super::Dev,
    state: DirectRcsState,
    lane: DirectRcsLane,
) -> bool {
    let (quarantined, runtime, client) = match lane {
        DirectRcsLane::SystemService => (
            &DIRECT_RCS_CONTEXT_QUARANTINED,
            &DIRECT_RCS_SUBMIT_RUNTIME,
            crate::gpu::vgpu::KernelClient::GpgpuSystem,
        ),
        DirectRcsLane::Execution => (
            &EXECUTION_RCS_CONTEXT_QUARANTINED,
            &EXECUTION_RCS_SUBMIT_RUNTIME,
            crate::gpu::vgpu::KernelClient::GpgpuExecution,
        ),
        DirectRcsLane::Lfm25 => (
            &LFM25_RCS_CONTEXT_QUARANTINED,
            &LFM25_RCS_SUBMIT_RUNTIME,
            crate::gpu::vgpu::KernelClient::Lfm25,
        ),
    };
    if quarantined.load(Ordering::Acquire) {
        return false;
    }
    let mut runtime = runtime.lock();
    direct_rcs_submit_batch_with_runtime(dev, state, &mut runtime, client, false).is_some()
}

fn quarantine_direct_rcs_context(reason: &'static str) {
    quarantine_direct_rcs_lane(DirectRcsLane::SystemService, reason);
}

fn direct_rcs_state_reuse_permitted(quarantined: &AtomicBool) -> bool {
    !quarantined.load(Ordering::Acquire)
}

pub(crate) fn direct_rcs_context_is_quarantined() -> bool {
    !direct_rcs_state_reuse_permitted(&DIRECT_RCS_CONTEXT_QUARANTINED)
}

fn execution_rcs_context_is_quarantined() -> bool {
    !direct_rcs_state_reuse_permitted(&EXECUTION_RCS_CONTEXT_QUARANTINED)
}

fn lfm25_rcs_context_is_quarantined() -> bool {
    !direct_rcs_state_reuse_permitted(&LFM25_RCS_CONTEXT_QUARANTINED)
}

fn quarantine_execution_rcs_context(reason: &'static str) {
    quarantine_direct_rcs_lane(DirectRcsLane::Execution, reason);
}

fn quarantine_lfm25_rcs_context(reason: &'static str) {
    quarantine_direct_rcs_lane(DirectRcsLane::Lfm25, reason);
}

fn quarantine_direct_rcs_lane(lane: DirectRcsLane, reason: &'static str) {
    let quarantined = match lane {
        DirectRcsLane::SystemService => &DIRECT_RCS_CONTEXT_QUARANTINED,
        DirectRcsLane::Execution => &EXECUTION_RCS_CONTEXT_QUARANTINED,
        DirectRcsLane::Lfm25 => &LFM25_RCS_CONTEXT_QUARANTINED,
    };
    if quarantined
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        crate::log_error!(
            target: "gpgpu";
            "intel/gpgpu: direct-rcs context quarantined lane={} reason={} action=reject-future-direct-submits-until-reboot late-batch-reuse=forbidden\n",
            lane.name(),
            reason,
        );
    }
}

fn direct_rcs_submit_batch_with_runtime(
    dev: super::Dev,
    state: DirectRcsState,
    runtime: &mut DirectRcsSubmitRuntime,
    client: crate::gpu::vgpu::KernelClient,
    allow_queued: bool,
) -> Option<crate::gpu::executor::KernelSubmission> {
    if !allow_queued && runtime.pending.is_some() {
        return None;
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
        return None;
    };
    if !runtime.context_initialized {
        if !direct_rcs_init_lrc_context_image(
            state,
            state.gpu_va.ring as u32,
            ring_tail_bytes as u32,
            ring_ctl,
        ) {
            return None;
        }
        runtime.context_initialized = true;
    } else {
        direct_rcs_write_lrc_ring_tail(state, ring_tail_bytes as u32);
    }
    let (context_desc_lo, context_desc_hi) = guc_rcs_context_descriptor(state.gpu_va.context);
    super::ggtt_invalidate(dev);
    core::sync::atomic::fence(Ordering::SeqCst);
    let descriptor = crate::gpu::physical::PhysicalContextDescriptor {
        engine: crate::gpu::physical::PhysicalEngineId::RCS0,
        hwlrca_lo: context_desc_lo,
        hwlrca_hi: context_desc_hi,
        gpuvm_root_phys: state.ppgtt_phys,
    };
    match crate::gpu::executor::submit_kernel_context(client, descriptor) {
        Ok(submission) => {
            runtime.ring_tail_bytes = ring_tail_bytes;
            if !allow_queued {
                runtime.pending = Some(submission);
            }
            Some(submission)
        }
        Err(error) => {
            // The entry was not admitted. Keep the software tail at the last
            // accepted position so a retry cannot silently skip ring space.
            direct_rcs_write_lrc_ring_tail(state, old_tail_bytes as u32);
            crate::log!(
                "gpgpu/vgpu: submit failed error={:?} submission_owner=gpu-executor/vgpu/guc direct_elsp=0\n",
                error
            );
            None
        }
    }
}

fn complete_direct_rcs_submission(completed: bool) {
    complete_direct_rcs_submission_on_lane(DirectRcsLane::SystemService, completed);
}

fn complete_execution_rcs_submission(completed: bool) {
    complete_direct_rcs_submission_on_lane(DirectRcsLane::Execution, completed);
}

fn complete_lfm25_rcs_submission(completed: bool) {
    complete_direct_rcs_submission_on_lane(DirectRcsLane::Lfm25, completed);
}

fn complete_direct_rcs_submission_on_lane(lane: DirectRcsLane, completed: bool) {
    let runtime = match lane {
        DirectRcsLane::SystemService => &DIRECT_RCS_SUBMIT_RUNTIME,
        DirectRcsLane::Execution => &EXECUTION_RCS_SUBMIT_RUNTIME,
        DirectRcsLane::Lfm25 => &LFM25_RCS_SUBMIT_RUNTIME,
    };
    let submission = runtime.lock().pending.take();
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
    let completed = observed == expected;
    if !completed {
        // The physical request is not cancelled by failing its software
        // timeline. Poison the shared context before the submit lock can be
        // released so no caller rewrites memory a late batch may still fetch.
        quarantine_direct_rcs_context("completion-marker-unobserved-reboot-required");
    }
    complete_direct_rcs_submission(completed);
    observed
}

fn direct_rcs_poll_result_slot_timeout_ms(
    state: DirectRcsState,
    slot: usize,
    expected: u32,
    timeout_ms: u64,
) -> u32 {
    direct_rcs_poll_result_slot_timeout_ms_on_lane(
        state,
        slot,
        expected,
        timeout_ms,
        DirectRcsLane::SystemService,
    )
}

fn execution_rcs_poll_result_slot_timeout_ms(
    state: DirectRcsState,
    slot: usize,
    expected: u32,
    timeout_ms: u64,
) -> u32 {
    direct_rcs_poll_result_slot_timeout_ms_on_lane(
        state,
        slot,
        expected,
        timeout_ms,
        DirectRcsLane::Execution,
    )
}

fn lfm25_rcs_poll_result_slot_timeout_ms(
    state: DirectRcsState,
    slot: usize,
    expected: u32,
    timeout_ms: u64,
) -> u32 {
    direct_rcs_poll_result_slot_timeout_ms_on_lane(
        state,
        slot,
        expected,
        timeout_ms,
        DirectRcsLane::Lfm25,
    )
}

fn direct_rcs_poll_result_slot_timeout_ms_on_lane(
    state: DirectRcsState,
    slot: usize,
    expected: u32,
    timeout_ms: u64,
    lane: DirectRcsLane,
) -> u32 {
    let started = direct_rcs_now_tick();
    let deadline = started.saturating_add(direct_rcs_ticks_from_ms(timeout_ms));
    let probe_logged = match lane {
        DirectRcsLane::SystemService => &DIRECT_RCS_TIMEOUT_POLL_PROBE_LOGGED,
        DirectRcsLane::Execution => &EXECUTION_RCS_TIMEOUT_POLL_PROBE_LOGGED,
        DirectRcsLane::Lfm25 => &LFM25_RCS_TIMEOUT_POLL_PROBE_LOGGED,
    };
    let log_probe = probe_logged
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok();
    if log_probe {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: marker-poll begin lane={} slot={} expected=0x{:08X} timeout_ms={} completion_limit=deadline cache_flush_bytes=4 pause_iters={} worker_slot={}\n",
            lane.name(),
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
            "intel/gpgpu: marker-poll end lane={} slot={} observed=0x{:08X} expected=0x{:08X} matched={} iterations={} elapsed_ms={}\n",
            lane.name(),
            slot,
            observed,
            expected,
            (observed == expected) as u8,
            iterations,
            direct_rcs_elapsed_ms_since(started),
        );
    }
    let completed = observed == expected;
    if !completed {
        quarantine_direct_rcs_lane(lane, "completion-marker-timeout-reboot-required");
    }
    complete_direct_rcs_submission_on_lane(lane, completed);
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

fn direct_rcs_read_result_qword(state: DirectRcsState, slot: usize) -> u64 {
    let offset = slot.saturating_mul(core::mem::size_of::<u32>());
    if slot & 1 != 0 || offset + core::mem::size_of::<u64>() > DIRECT_RCS_RESULT_BYTES {
        return 0;
    }
    let value = unsafe { state.result_virt.add(offset) };
    super::dma_flush(value, core::mem::size_of::<u64>());
    let low = unsafe { core::ptr::read_volatile(value as *const u32) };
    let high =
        unsafe { core::ptr::read_volatile(value.add(core::mem::size_of::<u32>()) as *const u32) };
    (u64::from(low) | (u64::from(high) << 32)) & DIRECT_RCS_TIMESTAMP_MASK
}

fn direct_rcs_read_render_timestamp(dev: super::Dev) -> u64 {
    const RCS_TIMESTAMP_LOW_MMIO: usize = 0x2358;
    const RCS_TIMESTAMP_HIGH_MMIO: usize = 0x235C;

    // The Gen12 engine timestamp is 36 bits. Read upper-lower-upper so a
    // rollover of the low DWORD cannot splice two different counter epochs.
    let mut upper = super::mmio_read(dev, RCS_TIMESTAMP_HIGH_MMIO);
    for _ in 0..3 {
        let lower = super::mmio_read(dev, RCS_TIMESTAMP_LOW_MMIO);
        let next_upper = super::mmio_read(dev, RCS_TIMESTAMP_HIGH_MMIO);
        if next_upper == upper {
            return (u64::from(upper) << 32 | u64::from(lower)) & DIRECT_RCS_TIMESTAMP_MASK;
        }
        upper = next_upper;
    }
    let lower = super::mmio_read(dev, RCS_TIMESTAMP_LOW_MMIO);
    (u64::from(upper) << 32 | u64::from(lower)) & DIRECT_RCS_TIMESTAMP_MASK
}

fn direct_rcs_timestamp_delta_ticks(start: u64, end: u64) -> Option<u64> {
    if start == 0 || end == 0 {
        return None;
    }
    let delta = end.wrapping_sub(start) & DIRECT_RCS_TIMESTAMP_MASK;
    // Any phase in this probe is bounded to one second in normal operation
    // and to the one-second compositor timeout on failure. Half a 36-bit
    // epoch is roughly thirty minutes at 19.2 MHz, so a larger modular delta
    // proves that these were not an ordered pair from the same frame.
    (delta != 0 && delta < (1u64 << (DIRECT_RCS_TIMESTAMP_BITS - 1))).then_some(delta)
}

fn direct_rcs_timestamp_interval_us(start: u64, end: u64, frequency_hz: u64) -> Option<(u64, u64)> {
    if frequency_hz == 0 {
        return None;
    }
    let ticks = direct_rcs_timestamp_delta_ticks(start, end)?;
    Some((ticks, direct_rcs_timestamp_ticks_to_us(ticks, frequency_hz)))
}

fn direct_rcs_timestamp_frequency_hz(dev: super::Dev) -> u32 {
    const CTC_MODE_MMIO: usize = 0x0A26C;
    const CTC_SOURCE_DIVIDE_LOGIC: u32 = 1;
    const RPM_CONFIG0_MMIO: usize = 0x00D00;
    const TIMESTAMP_OVERRIDE_MMIO: usize = 0x44074;

    static CACHED_HZ: AtomicU32 = AtomicU32::new(0);
    let cached = CACHED_HZ.load(Ordering::Acquire);
    if cached != 0 {
        return cached;
    }

    // Gen11+ command-stream clock selection, matching the hardware-owned
    // CTC_MODE/RPM_CONFIG0 contract. TRUEOS owns this GT and never reprograms
    // the clock source after initialization, so the resolved frequency can be
    // cached for the lifetime of the boot.
    let ctc_mode = super::mmio_read(dev, CTC_MODE_MMIO);
    let frequency = if ctc_mode & CTC_SOURCE_DIVIDE_LOGIC != 0 {
        let timestamp_override = super::mmio_read(dev, TIMESTAMP_OVERRIDE_MMIO);
        let divider = (timestamp_override & 0x3FF).saturating_add(1);
        let denominator = ((timestamp_override >> 12) & 0xF).saturating_add(1);
        divider
            .saturating_mul(1_000_000)
            .saturating_add(1_000_000 / denominator)
    } else {
        let rpm_config = super::mmio_read(dev, RPM_CONFIG0_MMIO);
        let crystal_hz = match (rpm_config >> 3) & 0x7 {
            0 => 24_000_000,
            1 => 19_200_000,
            2 => 38_400_000,
            3 => 25_000_000,
            _ => 0,
        };
        let shift = (rpm_config >> 1) & 0x3;
        crystal_hz >> (3 - shift)
    };
    if frequency != 0 {
        CACHED_HZ.store(frequency, Ordering::Release);
    }
    frequency
}

fn direct_rcs_timestamp_ticks_to_us(ticks: u64, frequency_hz: u64) -> u64 {
    if frequency_hz == 0 {
        return 0;
    }
    ((ticks as u128)
        .saturating_mul(1_000_000)
        .saturating_add(u128::from(frequency_hz / 2))
        / u128::from(frequency_hz))
    .min(u128::from(u64::MAX)) as u64
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

fn direct_rcs_elapsed_us_since(start_tick: u64) -> u64 {
    let elapsed = direct_rcs_now_tick().saturating_sub(start_tick);
    let hz = embassy_time_driver::TICK_HZ;
    if hz == 0 {
        0
    } else {
        ((elapsed as u128).saturating_mul(1_000_000) / hz as u128) as u64
    }
}

#[cfg(test)]
mod direct_rcs_fail_closed_tests {
    use super::*;

    #[test]
    fn quarantine_irreversibly_denies_shared_state_reuse() {
        let quarantined = AtomicBool::new(false);
        assert!(direct_rcs_state_reuse_permitted(&quarantined));

        quarantined.store(true, Ordering::Release);
        assert!(!direct_rcs_state_reuse_permitted(&quarantined));
    }

    #[test]
    fn copy_rect_completion_is_an_ordered_non_overlapping_post_sync_qword() {
        assert!(COPY_RECT_POST_MARKER_SLOT.is_multiple_of(2));
        assert_ne!(COPY_RECT_PRE_MARKER_SLOT, COPY_RECT_POST_MARKER_SLOT);
        assert_ne!(COPY_RECT_PRE_MARKER_SLOT, COPY_RECT_POST_MARKER_SLOT + 1);

        let mut batch = [0u32; 16];
        let mut cursor = 0usize;
        assert!(direct_rcs_push_gpgpu_dispatch_epilogue(
            &mut batch,
            &mut cursor,
            DIRECT_RCS_GPU_VA_RESULT_BASE,
            COPY_RECT_POST_MARKER_SLOT,
            COPY_RECT_POST_MARKER,
        ));
        assert_eq!(cursor, 14);

        // Producer release: HDC pipeline drain plus the full cache-flush set.
        assert_eq!(batch[0], PIPE_CONTROL_CMD | PIPE_CONTROL_HDC_PIPELINE_FLUSH);
        assert_eq!(batch[1], PIPE_CONTROL_FLUSH_BITS);

        // Ordered PIPE_CONTROL post-sync QWord, followed by batch retirement.
        assert_eq!(batch[6], PIPE_CONTROL_CMD);
        assert_eq!(
            batch[7],
            PIPE_CONTROL_FLUSH_ENABLE
                | PIPE_CONTROL_CS_STALL
                | PIPE_CONTROL_POST_SYNC_WRITE_IMMEDIATE
        );
        let marker_gpu = DIRECT_RCS_GPU_VA_RESULT_BASE
            + (COPY_RECT_POST_MARKER_SLOT * core::mem::size_of::<u32>()) as u64;
        assert_eq!(batch[8], marker_gpu as u32);
        assert_eq!(batch[9], (marker_gpu >> 32) as u32);
        assert_eq!(batch[10], COPY_RECT_POST_MARKER);
        assert_eq!(batch[11], 0);
        assert_eq!(batch[12], MI_BATCH_BUFFER_END);
        assert_eq!(batch[13], MI_NOOP);
    }
}
