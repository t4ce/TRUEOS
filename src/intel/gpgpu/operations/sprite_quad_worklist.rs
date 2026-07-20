pub(crate) fn sprite_quad_worklist_rgba8_runs_over_result(
    dst: GpgpuRgba8Surface,
    runs: &[GpgpuSpriteQuadWorklistRun<'_>],
) -> GpgpuWorklistSubmitResult {
    if !sprite_quad_worklist_ready() {
        return GpgpuWorklistSubmitResult::default();
    }
    let Some(desc_buffer) = sprite_quad_worklist_desc_buffer_once() else {
        return GpgpuWorklistSubmitResult::default();
    };
    let total_descs = runs
        .iter()
        .try_fold(0usize, |total, run| total.checked_add(run.descs.len()));
    let Some(total_descs) = total_descs else {
        return GpgpuWorklistSubmitResult::default();
    };
    if total_descs == 0 || total_descs > SPRITE_QUAD_WORKLIST_MAX_DESCS {
        return GpgpuWorklistSubmitResult::default();
    }
    if runs.iter().any(|run| run.descs.is_empty()) {
        return GpgpuWorklistSubmitResult::default();
    }

    let mut stats = GpgpuWorklistSubmitStats::default();
    let _desc_guard = RECT_WORKLIST_DESC_SUBMIT_LOCK.lock();
    unsafe {
        core::ptr::write_bytes(desc_buffer.virt, 0, desc_buffer.bytes);
        let out = desc_buffer.virt as *mut GpgpuSpriteQuadWorklistDesc;
        let mut index = 0usize;
        for run in runs {
            for desc in run.descs.iter().copied() {
                core::ptr::write_volatile(out.add(index), desc);
                index = index.saturating_add(1);
            }
        }
    }
    super::dma_flush(desc_buffer.virt, desc_buffer.bytes);

    let submit_start_tick = direct_rcs_now_tick();
    let outcome = submit_sprite_quad_worklist_runs(dst, desc_buffer, runs);
    if outcome != GpgpuSubmissionOutcome::Complete {
        return GpgpuWorklistSubmitResult { stats, outcome };
    }
    stats.submit_ms = stats
        .submit_ms
        .saturating_add(direct_rcs_elapsed_ms_since(submit_start_tick));
    stats.descs = total_descs;
    stats.walkers = runs.iter().fold(0usize, |total, run| {
        total.saturating_add(sprite_quad_worklist_walker_count(run.descs.len()))
    });
    stats.submits = 1;
    GpgpuWorklistSubmitResult { stats, outcome }
}
