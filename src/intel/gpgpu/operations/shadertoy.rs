pub(crate) const SHADERTOY_SHADER_MANDELBROT: u32 = 1;
pub(crate) const SHADERTOY_SHADER_CUBE_FIELD: u32 = 2;
pub(crate) const SHADERTOY_SHADER_NGUYEN: u32 = 3;
pub(crate) const SHADERTOY_SHADER_PALETTE_GRID: u32 = 4;
pub(crate) const SHADERTOY_SHADER_COSMIC_STRANDS: u32 = 5;
pub(crate) const SHADERTOY_SHADER_PROTEAN_CLOUDS: u32 = 6;
pub(crate) const SHADERTOY_PARAMS_VERSION: u32 = 1;
pub(crate) const SHADERTOY_FLAG_NATIVE_RESOLUTION: u32 = 1;

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
            && (1..=15).contains(&self.shader_id)
            && (self.flags == 0
                || (self.shader_id == SHADERTOY_SHADER_PROTEAN_CLOUDS
                    && self.flags == SHADERTOY_FLAG_NATIVE_RESOLUTION)
                || (self.shader_id == 14 && self.flags == 2))
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

// Bound each non-preemptible walker independently of window size. This keeps
// a 1440p image from occupying RCS in one long batch, without dropping pixels
// or extending the retirement timeout. Global-ID offsets preserve coordinates.
const SHADERTOY_DISPATCH_MAX_PIXELS: u64 = 128 * 1024;
const SHADERTOY_LIGHT_DISPATCH_MAX_PIXELS: u64 = 1024 * 1024;

fn shadertoy_dispatch_rows(
    shader_id: u32,
    phase: u32,
    width: u32,
    height: u32,
    first_row: u32,
) -> Option<u32> {
    if width == 0 || first_row >= height {
        return None;
    }
    let launched_width = u64::from(width).div_ceil(16) * 16;
    // The cheap effects need fewer submission boundaries. Keep the two
    // expensive procedural kernels in small batches; neither policy scales
    // a single walker with the entire window's pixel count.
    let max_pixels = match (shader_id, phase) {
        (_, 2) => SHADERTOY_LIGHT_DISPATCH_MAX_PIXELS,
        (SHADERTOY_SHADER_NGUYEN | SHADERTOY_SHADER_PROTEAN_CLOUDS, _) => {
            SHADERTOY_DISPATCH_MAX_PIXELS
        }
        _ => SHADERTOY_LIGHT_DISPATCH_MAX_PIXELS,
    };
    let rows = max_pixels / launched_width;
    if rows == 0 {
        return None;
    }
    Some((rows.min(u64::from(height - first_row))) as u32)
}

/// Render one reviewed ShaderToy Image pass over a complete trusted UI4 RGBA8
/// allocation. The operation is synchronous through the producer-release
/// marker, matching the other UI4 compute producers.
pub(crate) fn shadertoy_rgba8_surface_full(
    dst: GpgpuRgba8Surface,
    params: ShaderToyFrameParams,
) -> GpgpuRgba8KernelResult {
    if !dst.is_valid() || !params.is_valid() || direct_rcs_context_is_quarantined() {
        return GpgpuRgba8KernelResult::default();
    }
    let Some(plan) = shadertoy_focus_plan(dst.width, dst.height, params) else {
        let mut result = shadertoy_render_pass(dst, params, ShaderToyPass::native(dst));
        if result.ok {
            result.release = Some(gpgpu_rgba8_release(dst));
        }
        return result;
    };
    // One shared scratch allocation; hold ownership across BOTH passes so two
    // Blueprint windows cannot interleave atlas writes and resolve reads.
    let mut cache = SHADERTOY_FOCUS_SCRATCH.lock();
    if cache.quarantined {
        return GpgpuRgba8KernelResult::default();
    }
    let Some(scratch) = cache.surface(plan.width, plan.height) else {
        return GpgpuRgba8KernelResult::default();
    };
    let result = shadertoy_render_focused(dst, scratch, params, plan.focus);
    if !result.ok && result.submitted {
        cache.quarantined = true;
    }
    result
}

fn shadertoy_render_focused(
    dst: GpgpuRgba8Surface,
    scratch: GpgpuRgba8Surface,
    params: ShaderToyFrameParams,
    focus: [f32; 4],
) -> GpgpuRgba8KernelResult {
    let start_tick = direct_rcs_now_tick();
    let pass = ShaderToyPass {
        phase: 1,
        width: dst.width,
        height: dst.height,
        source: scratch,
        focus,
    };
    let mut result = shadertoy_render_pass(scratch, params, pass);
    if result.ok {
        result = shadertoy_render_pass(dst, params, ShaderToyPass { phase: 2, ..pass });
        if result.ok {
            result.release = Some(gpgpu_rgba8_release(dst));
        }
    }
    result.submit_ms = direct_rcs_elapsed_ms_since(start_tick);
    result
}

fn shadertoy_render_pass(
    dst: GpgpuRgba8Surface,
    params: ShaderToyFrameParams,
    pass: ShaderToyPass,
) -> GpgpuRgba8KernelResult {
    let start_tick = direct_rcs_now_tick();
    let mut first_row = 0;
    while first_row < dst.height {
        let Some(rows) =
            shadertoy_dispatch_rows(params.shader_id, pass.phase, dst.width, dst.height, first_row)
        else {
            return GpgpuRgba8KernelResult::default();
        };
        // Each call releases the submit lock after proven retirement. Never
        // read the atlas or publish output while a previous batch is unretired.
        let outcome = submit_shadertoy_rgba8_rows(dst, params, pass, first_row, rows);
        if outcome.observed != SHADERTOY_POST_MARKER {
            return GpgpuRgba8KernelResult {
                ok: false,
                submitted: outcome.submitted,
                marker: outcome.observed,
                submit_ms: direct_rcs_elapsed_ms_since(start_tick),
                release: None,
            };
        }
        first_row += rows;
    }
    GpgpuRgba8KernelResult {
        ok: true,
        submitted: true,
        marker: SHADERTOY_POST_MARKER,
        submit_ms: direct_rcs_elapsed_ms_since(start_tick),
        release: None,
    }
}

struct ShaderToyFocusScratch {
    allocation: Option<GpgpuOwnedRgba8Surface>,
    quarantined: bool,
}

static SHADERTOY_FOCUS_SCRATCH: Mutex<ShaderToyFocusScratch> = Mutex::new(ShaderToyFocusScratch {
    allocation: None,
    quarantined: false,
});

impl ShaderToyFocusScratch {
    fn surface(&mut self, width: u32, height: u32) -> Option<GpgpuRgba8Surface> {
        let pitch = u32::try_from(align_up((width as usize).checked_mul(4)?, 64)?).ok()?;
        let bytes = (pitch as usize).checked_mul(height as usize)?;
        if self
            .allocation
            .as_ref()
            .is_none_or(|a| a.surface().bytes < bytes)
        {
            let mut allocation = allocate_font_instance_rgba8_surface(width, height)?;
            // Shares the unique resource VA allocator, but this resource is
            // mapped only by SystemService, whose retirement owns its teardown.
            allocation.system_service = true;
            self.allocation = Some(allocation);
        }
        let mut surface = self.allocation.as_ref()?.surface();
        surface.width = width;
        surface.height = height;
        surface.pitch_bytes = pitch;
        surface.is_valid().then_some(surface)
    }
}

fn submit_shadertoy_rgba8_rows(
    dst: GpgpuRgba8Surface,
    params: ShaderToyFrameParams,
    pass: ShaderToyPass,
    first_row: u32,
    rows: u32,
) -> DirectRcsDispatchOutcome {
    if !dst.is_valid()
        || !params.is_valid()
        || shadertoy_dispatch_rows(params.shader_id, pass.phase, dst.width, dst.height, first_row)
            != Some(rows)
    {
        return DirectRcsDispatchOutcome::default();
    }
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        return DirectRcsDispatchOutcome::default();
    };
    let Some(upload) = upload_shadertoy_kernel(params.shader_id) else {
        return DirectRcsDispatchOutcome::default();
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        return DirectRcsDispatchOutcome::default();
    };

    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ppgtt_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let dst_ppgtt_ok = kernel_ppgtt_ok
        && direct_rcs_map_ppgtt_destination(state, dst.gpu, dst.phys, dst.bytes, pass.phase != 1);
    let source_ppgtt_ok = dst_ppgtt_ok
        && (pass.phase != 2
            || direct_rcs_map_ppgtt_kernel(
                state,
                pass.source.gpu,
                pass.source.phys,
                pass.source.bytes,
            ));
    let batch_ok = source_ppgtt_ok
        && direct_rcs_encode_shadertoy_batch(state, upload, dst, params, pass, first_row, rows);
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result_slot_timeout_ms(
            state,
            SHADERTOY_POST_MARKER_SLOT,
            SHADERTOY_POST_MARKER,
            UI4_COMPUTE_PRODUCER_RETIRE_TIMEOUT_MS,
        )
    } else {
        0
    };
    if observed != SHADERTOY_POST_MARKER {
        if submitted {
            quarantine_direct_rcs_context("shadertoy-marker-timeout");
        }
        crate::log_error!(target: "gpgpu";
            "intel/gpgpu: shadertoy failed shader={} forcewake={} mapped={} ppgtt={} kernel={} dst={} source={} phase={} batch={} submitted={} observed=0x{:08X} want=0x{:08X} extent={}x{} first_row={} rows={} pitch={} artifact={} kernel_gpu=0x{:X} dst_gpu=0x{:X}\n",
            params.shader_id,
            forcewake_ok as u8,
            mapped_ok as u8,
            ppgtt_ok as u8,
            kernel_ppgtt_ok as u8,
            dst_ppgtt_ok as u8,
            source_ppgtt_ok as u8,
            pass.phase,
            batch_ok as u8,
            submitted as u8,
            observed,
            SHADERTOY_POST_MARKER,
            dst.width,
            dst.height,
            first_row,
            rows,
            dst.pitch_bytes,
            upload.name,
            upload.gpu,
            dst.gpu,
        );
    }
    DirectRcsDispatchOutcome {
        submitted,
        observed,
    }
}

/// SystemService owns every GPU reference to this atlas. Take its submit lock
/// through exact unmapping before freeing/recycling; a timeout leaks safely.
/// Unlike Font, this lane currently rebuilds its PPGTT at the next submission.
fn retire_shadertoy_scratch_range(surface: GpgpuRgba8Surface) -> bool {
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    if direct_rcs_context_is_quarantined() || DIRECT_RCS_SUBMIT_RUNTIME.lock().pending.is_some() {
        return false;
    }
    let Some(state) = *DIRECT_RCS_STATE.lock() else {
        return true;
    };
    if !direct_rcs_unmap_ppgtt_region_exact(state, surface.gpu, surface.phys, surface.bytes)
        || !direct_rcs_flush_ppgtt_pte_range(state, surface.gpu, surface.bytes)
    {
        return false;
    }
    core::sync::atomic::fence(Ordering::SeqCst);
    true
}
