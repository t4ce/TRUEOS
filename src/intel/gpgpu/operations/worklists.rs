fn submit_solid_rect_worklist(
    dst: GpgpuRgba8Surface,
    desc: GpgpuRectWorklistDescBuffer,
    params: AlphaBlendWorklistRgba8Params,
    direct_scanout: bool,
) -> GpgpuSubmissionOutcome {
    if params.desc_count == 0 || params.desc_count as usize > ALPHA_BLEND_WORKLIST_MAX_DESCS {
        return GpgpuSubmissionOutcome::Unavailable;
    }
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        return GpgpuSubmissionOutcome::Unavailable;
    };
    let Some(upload) = upload_alpha_blend_worklist_rgba8_kernel() else {
        return GpgpuSubmissionOutcome::Unavailable;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        return GpgpuSubmissionOutcome::Unavailable;
    };

    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ppgtt_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let dst_ppgtt_ok = kernel_ppgtt_ok
        && direct_rcs_map_ppgtt_destination(state, dst.gpu, dst.phys, dst.bytes, direct_scanout);
    let desc_ppgtt_ok =
        dst_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, desc.gpu, desc.phys, desc.bytes);
    let batch_ok = desc_ppgtt_ok
        && direct_rcs_encode_alpha_blend_worklist_batch(
            state, upload, params, dst.bytes, dst.bytes, desc.bytes,
        );
    let submission = if batch_ok {
        direct_rcs_submit_batch_state(dev, state)
    } else {
        DirectRcsSubmissionState::Rejected
    };
    // Mapping policy and retirement budget are independent. Retained UI
    // surfaces deliberately use PAT0/WB, but their scene-sized worklists need
    // the same bounded 1 s service budget as a direct scanout destination.
    // Falling back to the smoke-test spin count here can quarantine the shared
    // producer merely because a larger Gridpaper page takes longer to retire.
    let observed = if submission.can_poll() {
        direct_rcs_poll_result_slot_timeout_ms(
            state,
            RECT_WORKLIST_POST_MARKER_SLOT,
            ALPHA_BLEND_WORKLIST_POST_MARKER,
            UI4_COMPUTE_PRODUCER_RETIRE_TIMEOUT_MS,
        )
    } else {
        0
    };
    if observed == ALPHA_BLEND_WORKLIST_POST_MARKER {
        GpgpuSubmissionOutcome::Complete
    } else if submission.may_have_submitted() {
        GpgpuSubmissionOutcome::SubmittedIncomplete
    } else {
        GpgpuSubmissionOutcome::Unavailable
    }
}


fn rect_worklist_walker_count(desc_count: usize) -> usize {
    desc_count
        .div_ceil(RECT_WORKLIST_DESCS_PER_WALKER)
        .min(RECT_WORKLIST_MAX_WALKERS)
}
fn simd16_right_mask(lanes: u32) -> u32 {
    if lanes >= 16 {
        GPGPU_WALKER_SIMD16_MASK
    } else if lanes == 0 {
        0
    } else {
        (1u32 << lanes) - 1
    }
}
