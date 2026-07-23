fn submit_cpp_demo_rgba8(
    dst: GpgpuRgba8Surface,
    mut params: CppDemoRgba8Params,
) -> DirectRcsDispatchOutcome {
    if !dst.is_valid()
        || params.rect_width == 0
        || params.rect_height == 0
        || !params.time_seconds.is_finite()
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
    if params.demo_mode >= CPP_DEMO_MODE_COUNT {
        params.demo_mode = CPP_DEMO_MODE_GALLERY;
    }

    // This is an interactive producer. Queue behind other system-service RCS
    // work so transient compositor pressure becomes backpressure, not a lost
    // frame or a CPU-authored fallback.
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        crate::log_warn!(
            target: "gpgpu";
            "intel/gpgpu: cpp-demo-rgba8 submit rejected reason=no-claimed-device\n"
        );
        return DirectRcsDispatchOutcome::default();
    };
    let Some(upload) = upload_cpp_demo_rgba8_kernel() else {
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
        dst_ppgtt_ok && direct_rcs_encode_cpp_demo_rgba8_batch(state, upload, params, dst.bytes);
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result_slot_timeout_ms(
            state,
            CPP_DEMO_POST_MARKER_SLOT,
            CPP_DEMO_POST_MARKER,
            UI4_COMPUTE_PRODUCER_RETIRE_TIMEOUT_MS,
        )
    } else {
        0
    };

    if observed != CPP_DEMO_POST_MARKER {
        if submitted {
            quarantine_direct_rcs_context("cpp-demo-marker-timeout");
        }
        crate::log_error!(
            target: "gpgpu";
            "intel/gpgpu: cpp-demo-rgba8 failed mode={} forcewake={} mapped={} ppgtt={} kernel={} dst={} batch={} submitted={} observed=0x{:08X} want=0x{:08X} size={}x{} artifact={} kernel_gpu=0x{:X} dst_gpu=0x{:X}\n",
            params.demo_mode,
            forcewake_ok as u8,
            mapped_ok as u8,
            ppgtt_ok as u8,
            kernel_ppgtt_ok as u8,
            dst_ppgtt_ok as u8,
            batch_ok as u8,
            submitted as u8,
            observed,
            CPP_DEMO_POST_MARKER,
            params.rect_width,
            params.rect_height,
            CPP_DEMO_RGBA8_ADLS_ARTIFACT.name,
            upload.gpu,
            dst.gpu,
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

/// Render one C++ for OpenCL demo mode into a trusted UI4 RGBA8 surface.
///
/// The baked C++/IGC program is resident and exact-target admitted; only this
/// small scalar launch packet changes between frames and modes.
pub(crate) fn cpp_demo_rgba8_surface_full(
    dst: GpgpuRgba8Surface,
    time_seconds: f32,
    demo_mode: u32,
    seed: u32,
) -> GpgpuRgba8KernelResult {
    let start_tick = direct_rcs_now_tick();
    let mut params = CppDemoRgba8Params::new(time_seconds, demo_mode, seed);
    params.rect_width = dst.width;
    params.rect_height = dst.height;
    let outcome = submit_cpp_demo_rgba8(dst, params);
    let ok = outcome.observed == CPP_DEMO_POST_MARKER;
    GpgpuRgba8KernelResult {
        ok,
        submitted: outcome.submitted,
        marker: outcome.observed,
        submit_ms: direct_rcs_elapsed_ms_since(start_tick),
        release: ok.then(|| gpgpu_rgba8_release(dst)),
    }
}
