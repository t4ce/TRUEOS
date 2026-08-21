pub(crate) fn skybox_sample_rgb565_to_rgba8(
    skybox: GpgpuRgb565Surface,
    dst: GpgpuRgba8Surface,
    mut params: SkyboxSampleRgb565Params,
) -> GpgpuRgba8KernelResult {
    let started = direct_rcs_now_tick();
    if !skybox.is_valid() || !dst.is_valid() || params.rect_width == 0 || params.rect_height == 0 {
        return GpgpuRgba8KernelResult::default();
    }
    if params.rect_x >= dst.width || params.rect_y >= dst.height {
        return GpgpuRgba8KernelResult::default();
    }
    params.sky_gpu = skybox.gpu;
    params.dst_gpu = dst.gpu;
    params.sky_pitch_bytes = skybox.pitch_bytes;
    params.sky_width = skybox.width;
    params.sky_height = skybox.height;
    params.dst_pitch_bytes = dst.pitch_bytes;
    params.dst_width = dst.width;
    params.dst_height = dst.height;
    params.rect_width = params.rect_width.min(dst.width - params.rect_x);
    params.rect_height = params.rect_height.min(dst.height - params.rect_y);

    let seq = SKYBOX_SAMPLE_RGB565_LOG_SEQ.fetch_add(1, Ordering::Relaxed) + 1;
    let trace = seq <= 8 || seq % 120 == 0;
    if trace {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: skybox-sample-rgb565 begin seq={} rect={}x{} dst={}x{} sky={}x{} sky_gpu=0x{:X} dst_gpu=0x{:X}\n",
            seq,
            params.rect_width,
            params.rect_height,
            dst.width,
            dst.height,
            skybox.width,
            skybox.height,
            skybox.gpu,
            dst.gpu
        );
    }

    // The skybox owns one UI4 write lease. Queue behind the shared RCS lane
    // instead of converting transient engine contention into a permanent CPU
    // fallback for the Blueprint.
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        if trace {
            crate::log_info!(target: "gpgpu"; "intel/gpgpu: skybox-sample-rgb565 no claimed device seq={}\n", seq);
        }
        return GpgpuRgba8KernelResult::default();
    };
    let Some(upload) = upload_skybox_sample_rgb565_kernel() else {
        if trace {
            crate::log_info!(target: "gpgpu"; "intel/gpgpu: skybox-sample-rgb565 kernel upload unavailable seq={}\n", seq);
        }
        return GpgpuRgba8KernelResult::default();
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        if trace {
            crate::log_info!(target: "gpgpu"; "intel/gpgpu: skybox-sample-rgb565 direct state unavailable seq={}\n", seq);
        }
        return GpgpuRgba8KernelResult::default();
    };

    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ppgtt_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let sky_ppgtt_ok = kernel_ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, skybox.gpu, skybox.phys, skybox.bytes);
    let dst_ppgtt_ok =
        sky_ppgtt_ok && direct_rcs_map_ppgtt_scanout(state, dst.gpu, dst.phys, dst.bytes);
    let batch_ok = dst_ppgtt_ok
        && direct_rcs_encode_skybox_sample_rgb565_batch(
            state,
            upload,
            params,
            skybox.bytes,
            dst.bytes,
        );
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result_slot_timeout_ms(
            state,
            SKYBOX_SAMPLE_POST_MARKER_SLOT,
            SKYBOX_SAMPLE_POST_MARKER,
            UI4_COMPUTE_PRODUCER_RETIRE_TIMEOUT_MS,
        )
    } else {
        0
    };
    let ok = observed == SKYBOX_SAMPLE_POST_MARKER;
    if ok {
        if trace {
            crate::log_info!(
                target: "gpgpu";
                "intel/gpgpu: skybox-sample-rgb565 submitted=1 seq={} size={}x{} dst={}x{} marker=0x{:X}\n",
                seq,
                params.rect_width,
                params.rect_height,
                dst.width,
                dst.height,
                observed
            );
        }
    } else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: skybox-sample-rgb565 failed seq={} forcewake={} mapped={} ppgtt={} kernel={} sky={} dst={} batch={} submitted={} observed=0x{:X} want=0x{:X} upload_gpu=0x{:X} sky_gpu=0x{:X} dst_gpu=0x{:X} sky_bytes=0x{:X} dst_bytes=0x{:X}\n",
            seq,
            forcewake_ok as u8,
            mapped_ok as u8,
            ppgtt_ok as u8,
            kernel_ppgtt_ok as u8,
            sky_ppgtt_ok as u8,
            dst_ppgtt_ok as u8,
            batch_ok as u8,
            submitted as u8,
            observed,
            SKYBOX_SAMPLE_POST_MARKER,
            upload.gpu,
            skybox.gpu,
            dst.gpu,
            skybox.bytes,
            dst.bytes
        );
    }
    GpgpuRgba8KernelResult {
        ok,
        submitted,
        marker: observed,
        submit_ms: direct_rcs_elapsed_ms_since(started),
        release: ok.then(|| gpgpu_rgba8_release(dst)),
    }
}

fn submit_chart_sine_rgba8(
    dst: GpgpuRgba8Surface,
    mut params: ChartSineRgba8Params,
) -> DirectRcsDispatchOutcome {
    if !dst.is_valid()
        || params.rect_width == 0
        || params.rect_height == 0
        || !params.phase.is_finite()
        || !params.cycles.is_finite()
        || !params.amplitude.is_finite()
        || !params.line_width_px.is_finite()
    {
        return DirectRcsDispatchOutcome::default();
    }
    if params.rect_x >= dst.width || params.rect_y >= dst.height {
        return DirectRcsDispatchOutcome::default();
    }
    params.dst_gpu = dst.gpu;
    params.dst_pitch_bytes = dst.pitch_bytes;
    params.dst_width = dst.width;
    params.dst_height = dst.height;
    params.rect_width = params.rect_width.min(dst.width - params.rect_x);
    params.rect_height = params.rect_height.min(dst.height - params.rect_y);
    params.cycles = params.cycles.clamp(0.25, 32.0);
    params.amplitude = params.amplitude.clamp(0.0, 0.48);
    params.line_width_px = params.line_width_px.clamp(0.75, 8.0);

    // Chart work shares RCS0. Back-pressure behind an in-flight dispatch.
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        crate::log_warn!(
            target: "gpgpu";
            "intel/gpgpu: chart-sine-rgba8 submit rejected reason=no-claimed-device\n"
        );
        return DirectRcsDispatchOutcome::default();
    };
    let Some(upload) = upload_chart_sine_rgba8_kernel() else {
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
    let batch_ok =
        dst_ppgtt_ok && direct_rcs_encode_chart_sine_rgba8_batch(state, upload, params, dst.bytes);
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result_slot_timeout_ms(
            state,
            CHART_SINE_POST_MARKER_SLOT,
            CHART_SINE_POST_MARKER,
            UI4_COMPUTE_PRODUCER_RETIRE_TIMEOUT_MS,
        )
    } else {
        0
    };
    if observed != CHART_SINE_POST_MARKER {
        if submitted {
            quarantine_direct_rcs_context("chart-sine-marker-timeout");
        }
        crate::log_error!(
            target: "gpgpu";
            "intel/gpgpu: chart-sine-rgba8 failed forcewake={} mapped={} ppgtt={} kernel={} dst={} batch={} submitted={} observed=0x{:08X} want=0x{:08X} size={}x{} kernel_gpu=0x{:X} dst_gpu=0x{:X}\n",
            forcewake_ok as u8,
            mapped_ok as u8,
            ppgtt_ok as u8,
            kernel_ppgtt_ok as u8,
            dst_ppgtt_ok as u8,
            batch_ok as u8,
            submitted as u8,
            observed,
            CHART_SINE_POST_MARKER,
            params.rect_width,
            params.rect_height,
            upload.gpu,
            dst.gpu
        );
        return DirectRcsDispatchOutcome {
            submitted,
            observed,
        };
    }
    DirectRcsDispatchOutcome {
        submitted,
        observed,
    }
}

fn submit_pixel_plasma_rgba8(
    dst: GpgpuRgba8Surface,
    mut params: PixelPlasmaRgba8Params,
) -> DirectRcsDispatchOutcome {
    if !dst.is_valid()
        || params.rect_width == 0
        || params.rect_height == 0
        || !params.time.is_finite()
        || !params.spatial_scale.is_finite()
        || !params.intensity.is_finite()
    {
        return DirectRcsDispatchOutcome::default();
    }
    if params.rect_x >= dst.width || params.rect_y >= dst.height {
        return DirectRcsDispatchOutcome::default();
    }
    params.dst_gpu = dst.gpu;
    params.dst_pitch_bytes = dst.pitch_bytes;
    params.dst_width = dst.width;
    params.dst_height = dst.height;
    params.rect_width = params.rect_width.min(dst.width - params.rect_x);
    params.rect_height = params.rect_height.min(dst.height - params.rect_y);
    params.spatial_scale = params.spatial_scale.clamp(0.25, 8.0);
    params.intensity = params.intensity.clamp(0.25, 2.0);

    let Some(_guard) = DIRECT_RCS_SUBMIT_LOCK.try_lock() else {
        crate::log_warn!(
            target: "gpgpu";
            "intel/gpgpu: pixel-plasma-rgba8 submit rejected reason=direct-submit-busy\n"
        );
        return DirectRcsDispatchOutcome::default();
    };
    let Some(dev) = super::claimed_device() else {
        crate::log_warn!(
            target: "gpgpu";
            "intel/gpgpu: pixel-plasma-rgba8 submit rejected reason=no-claimed-device\n"
        );
        return DirectRcsDispatchOutcome::default();
    };
    let Some(upload) = upload_pixel_plasma_rgba8_kernel() else {
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
        && direct_rcs_encode_pixel_plasma_rgba8_batch(state, upload, params, dst.bytes);
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result_slot_timeout_ms(
            state,
            PIXEL_PLASMA_POST_MARKER_SLOT,
            PIXEL_PLASMA_POST_MARKER,
            UI4_COMPUTE_PRODUCER_RETIRE_TIMEOUT_MS,
        )
    } else {
        0
    };
    if observed != PIXEL_PLASMA_POST_MARKER {
        if submitted {
            quarantine_direct_rcs_context("pixel-plasma-marker-timeout");
        }
        crate::log_error!(
            target: "gpgpu";
            "intel/gpgpu: pixel-plasma-rgba8 failed forcewake={} mapped={} ppgtt={} kernel={} dst={} batch={} submitted={} observed=0x{:08X} want=0x{:08X} size={}x{} kernel_gpu=0x{:X} dst_gpu=0x{:X}\n",
            forcewake_ok as u8,
            mapped_ok as u8,
            ppgtt_ok as u8,
            kernel_ppgtt_ok as u8,
            dst_ppgtt_ok as u8,
            batch_ok as u8,
            submitted as u8,
            observed,
            PIXEL_PLASMA_POST_MARKER,
            params.rect_width,
            params.rect_height,
            upload.gpu,
            dst.gpu
        );
        return DirectRcsDispatchOutcome {
            submitted,
            observed,
        };
    }
    DirectRcsDispatchOutcome {
        submitted,
        observed,
    }
}
