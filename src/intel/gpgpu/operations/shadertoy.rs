pub(crate) const SHADERTOY_SHADER_MANDELBROT: u32 = 1;
pub(crate) const SHADERTOY_SHADER_CUBE_FIELD: u32 = 2;
pub(crate) const SHADERTOY_SHADER_NGUYEN: u32 = 3;
pub(crate) const SHADERTOY_SHADER_PALETTE_GRID: u32 = 4;
pub(crate) const SHADERTOY_SHADER_COSMIC_STRANDS: u32 = 5;
pub(crate) const SHADERTOY_SHADER_PROTEAN_CLOUDS: u32 = 6;
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
                SHADERTOY_SHADER_MANDELBROT
                    | SHADERTOY_SHADER_CUBE_FIELD
                    | SHADERTOY_SHADER_NGUYEN
                    | SHADERTOY_SHADER_PALETTE_GRID
                    | SHADERTOY_SHADER_COSMIC_STRANDS
                    | SHADERTOY_SHADER_PROTEAN_CLOUDS
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

// Bound each non-preemptible walker independently of window size. This keeps
// a 1440p image from occupying RCS in one long batch, without dropping pixels
// or extending the retirement timeout. Global-ID offsets preserve coordinates.
const SHADERTOY_DISPATCH_MAX_PIXELS: u64 = 128 * 1024;
const SHADERTOY_LIGHT_DISPATCH_MAX_PIXELS: u64 = 1024 * 1024;

fn shadertoy_dispatch_rows(
    shader_id: u32, width: u32, height: u32, first_row: u32,
) -> Option<u32> {
    if width == 0 || first_row >= height {
        return None;
    }
    let launched_width = u64::from(width).div_ceil(16) * 16;
    // The cheap effects need fewer submission boundaries. Keep the two
    // expensive procedural kernels in small batches; neither policy scales
    // a single walker with the entire window's pixel count.
    let max_pixels = match shader_id {
        SHADERTOY_SHADER_NGUYEN | SHADERTOY_SHADER_PROTEAN_CLOUDS => SHADERTOY_DISPATCH_MAX_PIXELS,
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
    let start_tick = direct_rcs_now_tick();
    if !dst.is_valid() || !params.is_valid() {
        return GpgpuRgba8KernelResult::default();
    }
    let mut first_row = 0;
    while first_row < dst.height {
        let Some(rows) = shadertoy_dispatch_rows(params.shader_id, dst.width, dst.height, first_row) else {
            return GpgpuRgba8KernelResult::default();
        };
        // Each call releases the submit lock after proven retirement. No
        // subsequent batch or scanout release may reuse an unretired buffer.
        let outcome = submit_shadertoy_rgba8_rows(dst, params, first_row, rows);
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
        release: Some(gpgpu_rgba8_release(dst)),
    }
}

fn submit_shadertoy_rgba8_rows(
    dst: GpgpuRgba8Surface,
    params: ShaderToyFrameParams,
    first_row: u32,
    rows: u32,
) -> DirectRcsDispatchOutcome {
    if !dst.is_valid() || !params.is_valid()
        || shadertoy_dispatch_rows(params.shader_id, dst.width, dst.height, first_row) != Some(rows)
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
    let dst_ppgtt_ok =
        kernel_ppgtt_ok && direct_rcs_map_ppgtt_scanout(state, dst.gpu, dst.phys, dst.bytes);
    let batch_ok = dst_ppgtt_ok
        && direct_rcs_encode_shadertoy_batch(state, upload, dst, params, first_row, rows);
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
            "intel/gpgpu: shadertoy failed shader={} forcewake={} mapped={} ppgtt={} kernel={} dst={} batch={} submitted={} observed=0x{:08X} want=0x{:08X} extent={}x{} first_row={} rows={} pitch={} artifact={} kernel_gpu=0x{:X} dst_gpu=0x{:X}\n",
            params.shader_id,
            forcewake_ok as u8,
            mapped_ok as u8,
            ppgtt_ok as u8,
            kernel_ppgtt_ok as u8,
            dst_ppgtt_ok as u8,
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
