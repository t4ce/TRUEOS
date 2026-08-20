pub(crate) const SHADERTOY_SHADER_MANDELBROT: u32 = 1;
pub(crate) const SHADERTOY_SHADER_CUBE_FIELD: u32 = 2;
pub(crate) const SHADERTOY_SHADER_NGUYEN: u32 = 3;
pub(crate) const SHADERTOY_PARAMS_VERSION: u32 = 1;

/// Pointer-free, host-owned launch state for one reviewed ShaderToy artifact.
/// The Blueprint chooses only a catalog id; executable bytes, surfaces, and
/// addresses remain kernel-owned.
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct ShaderToyFrameParams {
    pub(crate) version: u32,
    pub(crate) shader_id: u32,
    pub(crate) frame: u32,
    pub(crate) flags: u32,
    pub(crate) time_seconds: f32,
    pub(crate) delta_seconds: f32,
    pub(crate) frame_rate: f32,
    pub(crate) sample_rate: f32,
    pub(crate) mouse_x: f32,
    pub(crate) mouse_y: f32,
    pub(crate) click_x: f32,
    pub(crate) click_y: f32,
    pub(crate) date_year: f32,
    pub(crate) date_month: f32,
    pub(crate) date_day: f32,
    pub(crate) date_seconds: f32,
}

impl ShaderToyFrameParams {
    pub(crate) fn is_valid(self) -> bool {
        self.version == SHADERTOY_PARAMS_VERSION
            && matches!(
                self.shader_id,
                SHADERTOY_SHADER_MANDELBROT | SHADERTOY_SHADER_CUBE_FIELD | SHADERTOY_SHADER_NGUYEN
            )
            && self.flags == 0
            && self.time_seconds.is_finite()
            && self.time_seconds >= 0.0
            && self.delta_seconds.is_finite()
            && self.delta_seconds >= 0.0
            && self.frame_rate.is_finite()
            && self.frame_rate > 0.0
            && self.sample_rate.is_finite()
            && self.sample_rate >= 0.0
            && self.mouse_x.is_finite()
            && self.mouse_y.is_finite()
            && self.click_x.is_finite()
            && self.click_y.is_finite()
            && self.date_year.is_finite()
            && self.date_month.is_finite()
            && self.date_day.is_finite()
            && self.date_seconds.is_finite()
    }
}

/// One accepted ShaderToy dispatch.  Its backing belongs exclusively to the
/// ShaderToy RCS lane until `poll_shadertoy_rgba8_submission` proves both the
/// post marker and GuC context retirement.
#[derive(Copy, Clone, Debug)]
pub(crate) struct ShaderToyRgba8Submission {
    state: DirectRcsState,
    dst: GpgpuRgba8Surface,
    started_tick: u64,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum ShaderToyRgba8SubmissionPoll {
    Pending,
    Complete(GpgpuRgba8ReleaseFence),
    Failed,
}

/// Submit a reviewed ShaderToy Image pass without waiting for producer
/// retirement.  UI4 owns the later release poll and publication.
pub(crate) fn submit_shadertoy_rgba8_surface_full(
    dst: GpgpuRgba8Surface,
    params: ShaderToyFrameParams,
) -> Option<ShaderToyRgba8Submission> {
    if !dst.is_valid() || !params.is_valid() {
        return None;
    }
    let _guard = SHADERTOY_RCS_SUBMIT_LOCK.lock();
    if SHADERTOY_RCS_SUBMIT_RUNTIME.lock().pending.is_some() {
        return None;
    }
    let Some(dev) = super::claimed_device() else {
        return None;
    };
    let Some(upload) = upload_shadertoy_kernel(params.shader_id) else {
        return None;
    };
    let Some(state) = shadertoy_rcs_state_once(dev) else {
        return None;
    };

    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ppgtt_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let dst_ppgtt_ok =
        kernel_ppgtt_ok && direct_rcs_map_ppgtt_scanout(state, dst.gpu, dst.phys, dst.bytes);
    let batch_ok = dst_ppgtt_ok && direct_rcs_encode_shadertoy_batch(state, upload, dst, params);
    let submitted = batch_ok && shadertoy_rcs_submit_batch(dev, state);
    if !submitted {
        crate::log_error!(target: "gpgpu";
            "intel/gpgpu: shadertoy submit rejected shader={} forcewake={} mapped={} ppgtt={} kernel={} dst={} batch={} extent={}x{} pitch={} artifact={} kernel_gpu=0x{:X} dst_gpu=0x{:X}\n",
            params.shader_id,
            forcewake_ok as u8,
            mapped_ok as u8,
            ppgtt_ok as u8,
            kernel_ppgtt_ok as u8,
            dst_ppgtt_ok as u8,
            batch_ok as u8,
            dst.width,
            dst.height,
            dst.pitch_bytes,
            upload.name,
            upload.gpu,
            dst.gpu,
        );
        return None;
    }
    Some(ShaderToyRgba8Submission {
        state,
        dst,
        started_tick: direct_rcs_now_tick(),
    })
}

/// Make one nonblocking completion observation.  This replaces the old
/// 1-second busy wait on the Blueprint producer path.
pub(crate) fn poll_shadertoy_rgba8_submission(
    submission: ShaderToyRgba8Submission,
) -> ShaderToyRgba8SubmissionPoll {
    let _guard = SHADERTOY_RCS_SUBMIT_LOCK.lock();
    let observed = direct_rcs_read_result_slot(submission.state, SHADERTOY_POST_MARKER_SLOT);
    let proof = direct_rcs_retirement_proof_on_lane(
        submission.state,
        DirectRcsLane::ShaderToy,
        observed == SHADERTOY_POST_MARKER,
    );
    if proof.complete() {
        complete_shadertoy_rcs_submission();
        return ShaderToyRgba8SubmissionPoll::Complete(gpgpu_rgba8_release(submission.dst));
    }
    if direct_rcs_elapsed_ms_since(submission.started_tick)
        < UI4_COMPUTE_PRODUCER_RETIRE_TIMEOUT_MS
    {
        return ShaderToyRgba8SubmissionPoll::Pending;
    }
    let reason = if observed == SHADERTOY_POST_MARKER {
        "completion-marker-observed-context-save-timeout"
    } else {
        "completion-marker-timeout"
    };
    quarantine_shadertoy_rcs_context(reason);
    crate::log_error!(target: "gpgpu";
        "intel/gpgpu: shadertoy release timeout marker=0x{:08X} want=0x{:08X} saved_head={} published_tail={} action=quarantine-shadertoy-lane\n",
        observed,
        SHADERTOY_POST_MARKER,
        proof.saved_head_bytes,
        proof.published_tail_bytes,
    );
    ShaderToyRgba8SubmissionPoll::Failed
}
