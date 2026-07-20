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

pub(crate) fn shell_font_outline_probe(
    ops: &[[u32; 8]],
    expected_checksum: u32,
    stage: u32,
    units_per_em: u16,
) -> GpgpuFontOutlineProbeResult {
    let mut result = GpgpuFontOutlineProbeResult {
        op_count: ops.len().min(u32::MAX as usize) as u32,
        expected_checksum,
        ..GpgpuFontOutlineProbeResult::default()
    };
    if ops.is_empty()
        || ops.len() > FONT_OUTLINE_MESH_MAX_OPS
        || !(FONT_OUTLINE_STAGE_AUDIT..=FONT_OUTLINE_STAGE_STROKE_MESH).contains(&stage)
    {
        crate::log_error!(
            target: "gpgpu";
            "intel/gpgpu: font-outline rejected stage={} ops={} max_ops={} reason=invalid-request\n",
            stage,
            ops.len(),
            FONT_OUTLINE_MESH_MAX_OPS,
        );
        return result;
    }
    let Some(_guard) = DIRECT_RCS_SUBMIT_LOCK.try_lock() else {
        crate::log_warn!(
            target: "gpgpu";
            "intel/gpgpu: font-outline-mesh rejected stage={} reason=direct-submit-busy\n",
            stage
        );
        return result;
    };
    let Some(dev) = super::claimed_device() else {
        return result;
    };
    result.available = true;
    let Some(upload) = upload_font_outline_mesh_kernel() else {
        return result;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        return result;
    };

    let input_bytes = ops.len() * core::mem::size_of::<[u32; 8]>();
    unsafe {
        core::ptr::write_bytes(state.clear_test_virt, 0, CLEAR_RECT_TEST_BYTES);
        core::ptr::copy_nonoverlapping(
            ops.as_ptr().cast::<u8>(),
            state.clear_test_virt,
            input_bytes,
        );
        core::ptr::write_bytes(
            state.font_outline_mesh_out_virt,
            0,
            FONT_OUTLINE_MESH_OUT_ALLOC_BYTES,
        );
    }
    super::dma_flush(state.clear_test_virt, input_bytes);
    super::dma_flush(
        state.font_outline_mesh_out_virt,
        FONT_OUTLINE_MESH_OUT_ALLOC_BYTES,
    );

    let params = FontOutlineMeshParams {
        src_gpu: DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE,
        dst_gpu: DIRECT_RCS_GPU_VA_FONT_OUTLINE_MESH_BASE,
        op_count: ops.len() as u32,
        stage,
        subdivisions: 8,
        max_vertices: FONT_OUTLINE_MESH_MAX_VERTICES,
        max_indices: FONT_OUTLINE_MESH_MAX_INDICES,
        // Fit the complete sample string into clip space. The kernel keeps
        // font Y-up orientation; the render viewport performs the screen flip.
        scale: 0.32 / f32::from(units_per_em.max(1)),
        origin_x: -0.85,
        origin_y: -0.25,
        stroke_half_width: 0.008,
    };
    result.forcewake_ok = direct_rcs_forcewake(dev);
    result.mapped_ok = result.forcewake_ok && direct_rcs_map_state(dev, state);
    result.ppgtt_ok = result.mapped_ok && direct_rcs_init_ppgtt(state);
    result.kernel_ppgtt_ok = result.ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    result.src_ppgtt_ok = result.kernel_ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(
            state,
            params.src_gpu,
            state.clear_test_phys,
            CLEAR_RECT_TEST_BYTES,
        );
    result.dst_ppgtt_ok = result.src_ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(
            state,
            params.dst_gpu,
            state.font_outline_mesh_out_phys,
            FONT_OUTLINE_MESH_OUT_ALLOC_BYTES,
        );
    result.batch_ok = result.dst_ppgtt_ok
        && direct_rcs_encode_font_outline_mesh_batch(
            state,
            upload,
            params,
            input_bytes,
            FONT_OUTLINE_MESH_OUT_ALLOC_BYTES,
        );
    let submit_start_tick = direct_rcs_now_tick();
    result.submitted = result.batch_ok && direct_rcs_submit_batch(dev, state);
    let (observed, retire_ms) = if result.submitted {
        direct_rcs_poll_result_slot_elapsed(
            state,
            FONT_OUTLINE_MESH_POST_MARKER_SLOT,
            FONT_OUTLINE_MESH_POST_MARKER,
            submit_start_tick,
        )
    } else {
        (0, 0)
    };
    result.retire_ms = retire_ms;
    result.pre_marker = direct_rcs_read_result_slot(state, FONT_OUTLINE_MESH_PRE_MARKER_SLOT);
    result.post_marker = observed;
    result.retired = observed == FONT_OUTLINE_MESH_POST_MARKER;

    super::dma_flush(
        state.font_outline_mesh_out_virt,
        FONT_OUTLINE_MESH_OUT_ALLOC_BYTES,
    );
    let report = unsafe {
        core::slice::from_raw_parts(state.font_outline_mesh_out_virt as *const u32, 25)
    };
    result.report_marker = report[0];
    result.done_marker = report[24];
    result.kernel_done = report[24] == FONT_OUTLINE_MESH_RESULT_DONE;
    result.op_count = report[3];
    result.move_count = report[4];
    result.line_count = report[5];
    result.quad_count = report[6];
    result.cubic_count = report[7];
    result.close_count = report[8];
    result.vertices = report[9];
    result.segments = report[10];
    result.indices = report[12];
    result.checksum = report[13];
    result.invalid = report[14];
    result.truncated = report[15] != 0;
    result.min_x = f32::from_bits(report[16]);
    result.min_y = f32::from_bits(report[17]);
    result.max_x = f32::from_bits(report[18]);
    result.max_y = f32::from_bits(report[19]);
    let layout_ok = report[21] == FONT_OUTLINE_MESH_LAYOUT_VERSION
        && report[22] == FONT_OUTLINE_MESH_VERTEX_DWORD_OFFSET
        && report[23] == FONT_OUTLINE_MESH_INDEX_DWORD_OFFSET;
    result.indices_in_range = if stage == FONT_OUTLINE_STAGE_STROKE_MESH
        && result.indices <= FONT_OUTLINE_MESH_MAX_INDICES
    {
        let indices = unsafe {
            core::slice::from_raw_parts(
                (state.font_outline_mesh_out_virt as *const u32)
                    .add(FONT_OUTLINE_MESH_INDEX_DWORD_OFFSET as usize),
                result.indices as usize,
            )
        };
        indices.iter().all(|index| *index < result.vertices)
    } else {
        result.indices == 0
    };
    result.ok = result.retired
        && result.pre_marker == FONT_OUTLINE_MESH_PRE_MARKER
        && result.report_marker == (FONT_OUTLINE_MESH_RESULT_MAGIC_BASE | stage)
        && result.kernel_done
        && layout_ok
        && report[1] & 1 != 0
        && result.op_count == ops.len() as u32
        && result.checksum == expected_checksum
        && result.invalid == 0
        && !result.truncated
        && result.indices_in_range;
    if result.ok && stage == FONT_OUTLINE_STAGE_STROKE_MESH {
        result.generated_mesh = Some(GpgpuFontOutlineMesh {
            storage_phys: state.font_outline_mesh_out_phys,
            storage_bytes: FONT_OUTLINE_MESH_OUT_ALLOC_BYTES,
            vertex_offset_bytes: FONT_OUTLINE_MESH_VERTEX_DWORD_OFFSET * 4,
            vertex_count: result.vertices,
            vertex_stride: 2 * core::mem::size_of::<f32>() as u32,
            index_offset_bytes: FONT_OUTLINE_MESH_INDEX_DWORD_OFFSET * 4,
            index_count: result.indices,
            min_x: result.min_x,
            min_y: result.min_y,
            max_x: result.max_x,
            max_y: result.max_y,
        });
    }

    let level_ok = result.ok;
    let message = alloc::format!(
        "intel/gpgpu: font-outline stage={} ok={} retired={} kernel_done={} ops={} counts=[{},{},{},{},{}] vertices={} segments={} indices={} checksum=0x{:08X}/0x{:08X} invalid={} truncated={} index_range={} markers=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] bounds=[{:.2},{:.2}..{:.2},{:.2}] retire_ms={} residency=probe-scratch fill_tessellation=0",
        stage,
        result.ok as u8,
        result.retired as u8,
        result.kernel_done as u8,
        result.op_count,
        result.move_count,
        result.line_count,
        result.quad_count,
        result.cubic_count,
        result.close_count,
        result.vertices,
        result.segments,
        result.indices,
        result.checksum,
        result.expected_checksum,
        result.invalid,
        result.truncated as u8,
        result.indices_in_range as u8,
        result.pre_marker,
        result.post_marker,
        result.report_marker,
        result.done_marker,
        result.min_x,
        result.min_y,
        result.max_x,
        result.max_y,
        result.retire_ms,
    );
    if level_ok {
        crate::log_info!(target: "gpgpu"; "{}\n", message.as_str());
    } else {
        crate::log_error!(target: "gpgpu"; "{}\n", message.as_str());
    }
    result
}
