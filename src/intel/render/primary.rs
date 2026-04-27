pub(crate) fn submit_primary_triangle_once() {
    if PRIMARY_TRIANGLE_SUBMITTED.swap(true, Ordering::AcqRel) {
        return;
    }

    let _ = submit_primary_probe_now("boot-once");
}

pub(crate) fn submit_primary_probe_periodic() {
    let _ = submit_primary_probe_now("periodic");
}

fn submit_primary_probe_now(reason: &'static str) -> bool {
    let probe_seq = PRIMARY_PROBE_SEQ.fetch_add(1, Ordering::AcqRel) + 1;
    if PRIMARY_PROBE_IN_FLIGHT
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        crate::log!("intel/render: primary-probe skipped reason=in-flight trigger={}\n", reason);
        return false;
    }

    if PRIMARY_DISABLE_RENDER_BRINGUP {
        crate::log!(
            "intel/render: primary-probe skipped reason=disabled trigger={} seq={}\n",
            reason,
            probe_seq
        );
        PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
        return false;
    }

    let Some(dev) = crate::intel::claimed_device() else {
        crate::log!("intel/render: primary-triangle skipped reason=no-device\n");
        PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
        return false;
    };
    let Some(surface_gpu) = crate::intel::display::primary_surface_gpu_addr() else {
        crate::log!("intel/render: primary-triangle skipped reason=no-surface\n");
        PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
        return false;
    };
    let Some((width, height)) = crate::intel::display::active_scanout_dimensions() else {
        crate::log!("intel/render: primary-triangle skipped reason=no-dimensions\n");
        PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
        return false;
    };
    let Some(pitch_bytes) = width
        .checked_mul(4)
        .and_then(|v| crate::intel::align_up(v as usize, 64))
    else {
        crate::log!("intel/render: primary-triangle skipped reason=bad-pitch width={}\n", width);
        PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
        return false;
    };

    let warm = warm_once(dev);
    if warm.ring_len == 0
        || warm.context_len == 0
        || warm.batch_len == 0
        || warm.draw_state_len == 0
        || warm.vertex_len == 0
        || warm.result_len == 0
        || warm.streamout_len == 0
    {
        crate::log!("intel/render: primary-triangle skipped reason=warm-buffers\n");
        PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
        return false;
    }
    if !forcewake_render_acquire(warm) {
        crate::log!("intel/render: primary-triangle skipped reason=forcewake\n");
        PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
        return false;
    }
    if !ensure_smoke_buffers_mapped(dev, warm) {
        crate::log!("intel/render: primary-triangle skipped reason=ggtt-map\n");
        PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
        return false;
    }
    if PRIMARY_USE_MI_SCANOUT_PROOF
        && reason == "boot-once"
        && !PRIMARY_MI_SCANOUT_PROOF_SUBMITTED.swap(true, Ordering::AcqRel)
    {
        let accepted = submit_mi_scanout_store_proof(
            dev,
            warm,
            surface_gpu,
            pitch_bytes,
            width as usize,
            height as usize,
        );
        if !accepted {
            intel_render_verbose_log!(
                "intel/render: primary-mi-scanout-store proof failed trigger={}\n",
                reason
            );
        }
    }
    let completed = if PRIMARY_USE_DRAW_PATH_BOOT_ONCE && reason == "boot-once" {
        let completed = submit_primary_triangle_with_retries(
            dev,
            warm,
            surface_gpu,
            pitch_bytes,
            width as usize,
            height as usize,
        );
        if !completed {
            intel_render_verbose_log!(
                "intel/render: primary-draw-path submit failed trigger={} mode=clean-boot-once\n",
                reason
            );
        }
        completed
    } else if PRIMARY_USE_MI_STRIPE_PROBE {
        let completed = submit_vertical_stripes_to_surface(
            dev,
            warm,
            surface_gpu,
            pitch_bytes,
            width as usize,
            height as usize,
        );
        if !completed {
            intel_render_verbose_log!(
                "intel/render: primary-mi-stripes submit failed trigger={}\n",
                reason
            );
        }
        completed
    } else if PRIMARY_USE_3D_NO_DRAW_PROBE {
        let completed = submit_3d_no_draw_probe(dev, warm);
        if !completed {
            intel_render_verbose_log!(
                "intel/render: primary-3d-no-draw submit failed trigger={}\n",
                reason
            );
        }
        completed
    } else if submit_primary_triangle_with_retries(
        dev,
        warm,
        surface_gpu,
        pitch_bytes,
        width as usize,
        height as usize,
    ) {
        true
    } else {
        let completed = submit_triangle_to_surface(
            dev,
            warm,
            surface_gpu,
            pitch_bytes,
            width as usize,
            height as usize,
        );
        if !completed {
            intel_render_verbose_log!(
                "intel/render: primary-triangle submit failed trigger={}\n",
                reason
            );
        }
        completed
    };
    if should_log_primary_probe(reason, probe_seq) {
        intel_render_verbose_log!(
            "intel/render: primary-probe seq={} trigger={} completed={} mode={}\n",
            probe_seq,
            reason,
            completed as u8,
            if PRIMARY_USE_MI_STRIPE_PROBE {
                "mi-stripes"
            } else if PRIMARY_USE_DRAW_PATH_BOOT_ONCE && reason == "boot-once" {
                "draw-path"
            } else if PRIMARY_USE_3D_NO_DRAW_PROBE {
                "3d-no-draw"
            } else {
                "3d"
            }
        );
    }
    PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
    completed
}

fn submit_primary_triangle_with_retries(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    surface_gpu: u64,
    pitch_bytes: usize,
    width: usize,
    height: usize,
) -> bool {
    let initial_streamout_experiment =
        select_streamout_proof_experiment(PRIMARY_PROBE_SEQ.load(Ordering::Acquire));
    let vf_streamout_precheck = submit_triangle_vf_streamout_proof(
        dev,
        warm,
        surface_gpu,
        pitch_bytes,
        width,
        height,
        initial_streamout_experiment,
    );
    intel_render_verbose_log!(
        "intel/render: primary-vf-streamout-precheck experiment={} accepted={}\n",
        initial_streamout_experiment.label(),
        vf_streamout_precheck as u8,
    );
    if !vf_streamout_precheck {
        return false;
    }

    let vf_draw_precheck = submit_triangle_vf_draw_to_surface(
        "vf-draw-path",
        dev,
        warm,
        surface_gpu,
        pitch_bytes,
        width,
        height,
        TriangleBlendProbeMode::ExplicitRt0,
        VfPrimitiveGeometry::Canonical,
        BackendProbeMode::MesaLike,
        PostDrawSyncVariant::HeavyAll,
    );
    intel_render_verbose_log!(
        "intel/render: primary-vf-draw-precheck completed={}\n",
        vf_draw_precheck as u8,
    );
    if vf_draw_precheck {
        return true;
    }

    reset_fragment_boundary_probe();
    let ps_launch_big_primitive = submit_triangle_vf_draw_to_surface(
        "ps-launch-big-primitive",
        dev,
        warm,
        surface_gpu,
        pitch_bytes,
        width,
        height,
        TriangleBlendProbeMode::ExplicitRt0,
        VfPrimitiveGeometry::Oversized,
        BackendProbeMode::MesaLike,
        PostDrawSyncVariant::HeavyAll,
    );
    intel_render_verbose_log!(
        "intel/render: primary-ps-launch-big-primitive completed={}\n",
        ps_launch_big_primitive as u8,
    );
    if ps_launch_big_primitive {
        return true;
    }

    run_postdraw_flush_spectrum(dev, warm, surface_gpu, pitch_bytes, width, height);

    let fragment_candidate_ready = fragment_candidate_ready();
    let fragment_boundary_observed = fragment_boundary_observed();
    intel_render_focus_log!(
        "intel/render: primary-fragment-boundary-gate candidate_ready={} fragment_observed={} action={} reason=shape_to_fragment_boundary_precedes_ps_spectrum\n",
        fragment_candidate_ready as u8,
        fragment_boundary_observed as u8,
        if fragment_boundary_observed {
            "continue-ps-spectrum"
        } else {
            "halt-ps-spectrum"
        },
    );
    if !fragment_boundary_observed {
        return false;
    }

    let ps_bt1_big_primitive = submit_triangle_vf_draw_to_surface(
        "ps-bt1-big-primitive",
        dev,
        warm,
        surface_gpu,
        pitch_bytes,
        width,
        height,
        TriangleBlendProbeMode::ExplicitRt0,
        VfPrimitiveGeometry::Oversized,
        BackendProbeMode::PsBindingTableCountOne,
        PostDrawSyncVariant::HeavyAll,
    );
    intel_render_verbose_log!(
        "intel/render: primary-ps-bt1-big-primitive completed={}\n",
        ps_bt1_big_primitive as u8,
    );
    if ps_bt1_big_primitive {
        return true;
    }

    let ps_wm_normal_big_primitive = submit_triangle_vf_draw_to_surface(
        "ps-wm-normal-big-primitive",
        dev,
        warm,
        surface_gpu,
        pitch_bytes,
        width,
        height,
        TriangleBlendProbeMode::ExplicitRt0,
        VfPrimitiveGeometry::Oversized,
        BackendProbeMode::WmNormalDispatch,
        PostDrawSyncVariant::HeavyAll,
    );
    intel_render_verbose_log!(
        "intel/render: primary-ps-wm-normal-big-primitive completed={}\n",
        ps_wm_normal_big_primitive as u8,
    );
    if ps_wm_normal_big_primitive {
        return true;
    }

    let ps_dispatch_slot0_big_primitive = submit_triangle_vf_draw_to_surface(
        "ps-dispatch-slot0-big-primitive",
        dev,
        warm,
        surface_gpu,
        pitch_bytes,
        width,
        height,
        TriangleBlendProbeMode::ExplicitRt0,
        VfPrimitiveGeometry::Oversized,
        BackendProbeMode::PsDispatchSlot0,
        PostDrawSyncVariant::HeavyAll,
    );
    intel_render_verbose_log!(
        "intel/render: primary-ps-dispatch-slot0-big-primitive completed={}\n",
        ps_dispatch_slot0_big_primitive as u8,
    );
    if ps_dispatch_slot0_big_primitive {
        return true;
    }

    let ps_dispatch_slot1_big_primitive = submit_triangle_vf_draw_to_surface(
        "ps-dispatch-slot1-big-primitive",
        dev,
        warm,
        surface_gpu,
        pitch_bytes,
        width,
        height,
        TriangleBlendProbeMode::ExplicitRt0,
        VfPrimitiveGeometry::Oversized,
        BackendProbeMode::PsDispatchSlot1,
        PostDrawSyncVariant::HeavyAll,
    );
    intel_render_verbose_log!(
        "intel/render: primary-ps-dispatch-slot1-big-primitive completed={}\n",
        ps_dispatch_slot1_big_primitive as u8,
    );
    if ps_dispatch_slot1_big_primitive {
        return true;
    }

    let ps_dispatch_slot2_big_primitive = submit_triangle_vf_draw_to_surface(
        "ps-dispatch-slot2-big-primitive",
        dev,
        warm,
        surface_gpu,
        pitch_bytes,
        width,
        height,
        TriangleBlendProbeMode::ExplicitRt0,
        VfPrimitiveGeometry::Oversized,
        BackendProbeMode::PsDispatchSlot2,
        PostDrawSyncVariant::HeavyAll,
    );
    intel_render_verbose_log!(
        "intel/render: primary-ps-dispatch-slot2-big-primitive completed={}\n",
        ps_dispatch_slot2_big_primitive as u8,
    );
    if ps_dispatch_slot2_big_primitive {
        return true;
    }

    let payload_variants = [
        ("ps-payload-push-big-primitive", BackendProbeMode::PsPayloadPushConstant),
        ("ps-payload-attr-big-primitive", BackendProbeMode::PsPayloadAttributeEnable),
        ("ps-payload-simple-big-primitive", BackendProbeMode::PsPayloadSimpleHint),
        ("ps-payload-source-depth-w-big-primitive", BackendProbeMode::PsPayloadSourceDepthW),
        ("ps-payload-bary-big-primitive", BackendProbeMode::PsPayloadBaryPlanes),
    ];
    for (payload_submit_name, payload_mode) in payload_variants {
        let completed = submit_triangle_vf_draw_to_surface(
            payload_submit_name,
            dev,
            warm,
            surface_gpu,
            pitch_bytes,
            width,
            height,
            TriangleBlendProbeMode::ExplicitRt0,
            VfPrimitiveGeometry::Oversized,
            payload_mode,
            PostDrawSyncVariant::HeavyAll,
        );
        intel_render_verbose_log!(
            "intel/render: primary-{} completed={}\n",
            payload_submit_name,
            completed as u8,
        );
        if completed {
            return true;
        }
    }

    let grf_variants = [
        ("ps-grf-start-r1-big-primitive", BackendProbeMode::PsGrfStartR1),
        ("ps-grf-start-r2-big-primitive", BackendProbeMode::PsGrfStartR2),
        ("ps-grf-start-r4-big-primitive", BackendProbeMode::PsGrfStartR4),
        ("ps-grf-maxthreads-31-big-primitive", BackendProbeMode::PsGrfMaxThreads31),
        ("ps-grf-maxthreads-15-big-primitive", BackendProbeMode::PsGrfMaxThreads15),
    ];
    for (grf_submit_name, grf_mode) in grf_variants {
        let completed = submit_triangle_vf_draw_to_surface(
            grf_submit_name,
            dev,
            warm,
            surface_gpu,
            pitch_bytes,
            width,
            height,
            TriangleBlendProbeMode::ExplicitRt0,
            VfPrimitiveGeometry::Oversized,
            grf_mode,
            PostDrawSyncVariant::HeavyAll,
        );
        intel_render_verbose_log!(
            "intel/render: primary-{} completed={}\n",
            grf_submit_name,
            completed as u8,
        );
        if completed {
            return true;
        }
    }

    let ps_dispatch_all_big_primitive = submit_triangle_vf_draw_to_surface(
        "ps-dispatch-all-big-primitive",
        dev,
        warm,
        surface_gpu,
        pitch_bytes,
        width,
        height,
        TriangleBlendProbeMode::ExplicitRt0,
        VfPrimitiveGeometry::Oversized,
        BackendProbeMode::PsDispatchAllKspSlots,
        PostDrawSyncVariant::HeavyAll,
    );
    intel_render_verbose_log!(
        "intel/render: primary-ps-dispatch-all-big-primitive completed={}\n",
        ps_dispatch_all_big_primitive as u8,
    );
    if ps_dispatch_all_big_primitive {
        return true;
    }

    let vs_draw_frontier_precheck = submit_triangle_vs_draw_frontier_to_surface(
        dev,
        warm,
        surface_gpu,
        pitch_bytes,
        width,
        height,
        TriangleBlendProbeMode::ExplicitRt0,
    );
    intel_render_verbose_log!(
        "intel/render: primary-vs-draw-frontier-precheck completed={}\n",
        vs_draw_frontier_precheck as u8,
    );
    if vs_draw_frontier_precheck {
        return true;
    }

    let mut vs_streamout_experiment = initial_streamout_experiment;
    let mut vs_streamout_precheck = false;
    for attempt in 1..=3 {
        let accepted = submit_triangle_vs_streamout_proof(
            dev,
            warm,
            surface_gpu,
            pitch_bytes,
            width,
            height,
            vs_streamout_experiment,
        );
        intel_render_verbose_log!(
            "intel/render: primary-vs-streamout-precheck experiment={} accepted={} attempt={}/3\n",
            vs_streamout_experiment.label(),
            accepted as u8,
            attempt
        );
        if accepted {
            vs_streamout_precheck = true;
            break;
        }
        vs_streamout_experiment = vs_streamout_experiment.alternate();
    }
    if !vs_streamout_precheck {
        return false;
    }

    let mut streamout_experiment = vs_streamout_experiment;
    let mut streamout_precheck = false;
    for attempt in 1..=3 {
        let accepted = submit_triangle_streamout_proof(
            dev,
            warm,
            surface_gpu,
            pitch_bytes,
            width,
            height,
            streamout_experiment,
        );
        intel_render_verbose_log!(
            "intel/render: primary-streamout-precheck experiment={} accepted={} attempt={}/3\n",
            streamout_experiment.label(),
            accepted as u8,
            attempt
        );
        if accepted {
            streamout_precheck = true;
            break;
        }
        streamout_experiment = streamout_experiment.alternate();
    }
    if !streamout_precheck {
        return false;
    }

    let mut completed_any = false;
    for attempt in 1..=PRIMARY_TRIANGLE_SUBMIT_ATTEMPTS {
        let blend_mode = TriangleBlendProbeMode::for_attempt(attempt);
        let completed = submit_triangle_draw_to_surface(
            dev,
            warm,
            surface_gpu,
            pitch_bytes,
            width,
            height,
            blend_mode,
        );
        intel_render_verbose_log!(
            "intel/render: primary-triangle attempt={}/{} target=0x{:X} blend_probe={} completed={}\n",
            attempt,
            PRIMARY_TRIANGLE_SUBMIT_ATTEMPTS,
            surface_gpu,
            blend_mode.label(),
            completed as u8
        );
        completed_any |= completed;
        if !completed {
            intel_render_verbose_log!(
                "intel/render: primary-streamout-proof skipped trigger=draw-fail attempt={} reason=post-hang-state-not-clean\n",
                attempt,
            );
            break;
        }
    }
    completed_any
}

fn run_postdraw_flush_spectrum(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    surface_gpu: u64,
    pitch_bytes: usize,
    width: usize,
    height: usize,
) {
    for variant in POST_DRAW_FLUSH_SPECTRUM {
        let submit_name = variant.submit_name();
        let completed = submit_triangle_vf_draw_to_surface(
            submit_name,
            dev,
            warm,
            surface_gpu,
            pitch_bytes,
            width,
            height,
            TriangleBlendProbeMode::ExplicitRt0,
            VfPrimitiveGeometry::Canonical,
            BackendProbeMode::MesaLike,
            variant,
        );
        intel_render_focus_log!(
            "intel/render: postdraw-flush-spectrum submit={} variant={} completed={} note=diagnostic_only\n",
            submit_name,
            variant.label(),
            completed as u8,
        );
    }
}

fn submit_triangle_vf_streamout_proof(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    experiment: StreamoutProofExperiment,
) -> bool {
    let Some(draw) = prepare_vf_streamout_proof_resources(
        warm,
        dst_gpu_addr,
        pitch,
        rect_w,
        rect_h,
        experiment,
        VfPrimitiveGeometry::Canonical,
    ) else {
        crate::log!(
            "intel/render: vf-streamout-proof skipped reason=resource-layout size={}x{} pitch=0x{:X}\n",
            rect_w,
            rect_h,
            pitch
        );
        return false;
    };
    let slice_hash_table_offset = match write_vf_streamout_probe_state(warm) {
        Ok(offset) => offset,
        Err(reason) => {
            crate::log!(
                "intel/render: vf-streamout-proof skipped reason=probe-state detail={}\n",
                reason
            );
            return false;
        }
    };

    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
        core::ptr::write_bytes(warm.streamout_virt, 0, warm.streamout_len);
    }
    seed_result_debug_slots(warm);
    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    crate::intel::dma_flush(warm.streamout_virt, warm.streamout_len);

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let batch_tail_bytes = match encode_vf_streamout_proof_batch(
        batch,
        warm,
        draw,
        GPU_VA_RESULT_BASE,
        RCS_EXEC_RESULT_DRAW_PRE3D,
        RCS_EXEC_RESULT_DRAW_POST3D,
        RCS_EXEC_RESULT_DONE,
        experiment,
        slice_hash_table_offset,
    ) {
        Ok(bytes) => bytes,
        Err(reason) => {
            crate::log!("intel/render: vf-streamout-proof batch build failed detail={}\n", reason);
            return false;
        }
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);
    intel_render_verbose_log!(
        "intel/render: vf-streamout-proof batch-ready experiment={} bytes=0x{:X} so_gpu=0x{:X} so_pitch={} vertices={}\n",
        experiment.label(),
        batch_tail_bytes,
        GPU_VA_STREAMOUT_BASE,
        experiment.vertex_bytes(),
        draw.vertex_count
    );

    let stats_before = capture_triangle_stage_stats(dev);
    let completed = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_DONE,
        RESULT_SLOT_FINAL_DWORD,
        "vf-streamout-proof",
    );
    let stats_after = capture_triangle_stage_stats(dev);
    let accepted = completed
        || maybe_soft_accept_streamout_submit(
            "vf-streamout-proof",
            warm,
            stats_before,
            stats_after,
            false,
            experiment.vertex_bytes() * draw.vertex_count as usize,
        );
    log_streamout_proof_result(
        "vf-streamout-proof",
        warm,
        completed,
        draw.vertex_count as usize,
        experiment,
    );
    if !completed {
        recover_render_engine_after_nonretired_submit(dev, warm, "vf-streamout-proof");
    }
    accepted
}

fn submit_triangle_vf_draw_to_surface(
    submit_name: &'static str,
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    blend_mode: TriangleBlendProbeMode,
    geometry: VfPrimitiveGeometry,
    backend_probe_mode: BackendProbeMode,
    post_draw_sync_variant: PostDrawSyncVariant,
) -> bool {
    let Some(draw) = prepare_vf_streamout_proof_resources(
        warm,
        dst_gpu_addr,
        pitch,
        rect_w,
        rect_h,
        StreamoutProofExperiment::PositionSlot1,
        geometry,
    ) else {
        crate::log!(
            "intel/render: {} staging skipped reason=resource-layout size={}x{} pitch=0x{:X} geometry={}\n",
            submit_name,
            rect_w,
            rect_h,
            pitch,
            geometry.label(),
        );
        return false;
    };

    let pipeline = crate::intel::shader::triangle_pipeline();
    log_render_buffer_layout(warm, Some(dst_gpu_addr));
    log_render_packet_encodings();
    if crate::intel::shader::triangle_pipeline_is_placeholder() {
        crate::log!(
            "intel/render: {} staged rt=0x{:X} vb=0x{:X} state=0x{:X} size={}x{} pitch=0x{:X} vertices={} stride={} geometry={} status=awaiting-baked-shaders vs_src={} ps_src={} note={}\n",
            submit_name,
            draw.rt_gpu_addr,
            draw.vertex_gpu_addr,
            draw.state_gpu_addr,
            draw.target_w,
            draw.target_h,
            draw.rt_pitch,
            draw.vertex_count,
            draw.vertex_stride,
            geometry.label(),
            crate::intel::shader::TRIANGLE_VERTEX_SOURCE_PATH,
            crate::intel::shader::TRIANGLE_FRAGMENT_SOURCE_PATH,
            crate::intel::shader::triangle_pipeline_note()
        );
        return false;
    }

    intel_render_verbose_log!(
        "intel/render: {} ps-meta dispatch={:?} grf_start={} grf_used={} ksp_off=0x{:X} size={} header_only={} geometry={} backend={} postdraw_sync={} note={}\n",
        submit_name,
        pipeline.ps.meta.kernel.dispatch_mode,
        pipeline.ps.meta.kernel.grf_start_register,
        pipeline.ps.meta.kernel.grf_used,
        pipeline.ps.meta.kernel.ksp_offset_bytes,
        pipeline.ps.meta.kernel.code_size_bytes,
        (pipeline.ps.meta.num_varying_inputs == 0
            && pipeline.ps.meta.kernel.push_constant_bytes == 0) as u8,
        geometry.label(),
        backend_probe_mode.label(),
        post_draw_sync_variant.label(),
        crate::intel::shader::triangle_pipeline_note()
    );
    if geometry.fullscreen_candidate() {
        intel_render_focus_log!(
            "intel/render: {} fragment-candidate-shape accepted=1 geometry={} ndc=v0[-1.000,-1.000] v1[3.000,-1.000] v2[-1.000,3.000] screen_bbox=[0,0..{},{}] sample_points=full-surface coverage_contract=oversized-triangle does_not_prove=raster_samples_or_ps\n",
            submit_name,
            geometry.label(),
            draw.target_w.saturating_sub(1),
            draw.target_h.saturating_sub(1),
        );
    }

    let shader_layout = match upload_triangle_shader_pipeline(warm, pipeline) {
        Ok(layout) => layout,
        Err(reason) => {
            crate::log!(
                "intel/render: {} staging skipped reason=shader-layout-error detail={} note={}\n",
                submit_name,
                reason,
                crate::intel::shader::triangle_pipeline_note()
            );
            return false;
        }
    };
    let ps_ksp_code_dword_index =
        (pipeline.ps.meta.kernel.ksp_offset_bytes / core::mem::size_of::<u32>() as u32) as usize;
    let ps_ksp_packet_offset = shader_layout
        .ps
        .code_offset_bytes
        .saturating_add(shader_layout.ps.ksp_offset_bytes);
    let ps_ksp_base = ps_ksp_packet_offset & !0x3F;
    let ps_ksp0 = if matches!(backend_probe_mode.ps_dispatch_slot(), Some(1 | 2)) {
        0
    } else {
        ps_ksp_base
    };
    let ps_ksp1 = if matches!(
        backend_probe_mode,
        BackendProbeMode::PsDispatchSlot1 | BackendProbeMode::PsDispatchAllKspSlots
    ) {
        ps_ksp_base
    } else {
        0
    };
    let ps_ksp2 = if matches!(
        backend_probe_mode,
        BackendProbeMode::PsDispatchSlot2 | BackendProbeMode::PsDispatchAllKspSlots
    ) {
        ps_ksp_base
    } else {
        0
    };
    let baked_ps_first = pipeline
        .ps
        .code
        .get(ps_ksp_code_dword_index)
        .copied()
        .unwrap_or(0);
    let uploaded_ps_first = unsafe {
        let ptr = (warm.draw_state_virt as *const u8).add(
            shader_layout.ps.code_offset_bytes as usize
                + shader_layout.ps.ksp_offset_bytes as usize,
        ) as *const u32;
        core::ptr::read_volatile(ptr)
    };
    let ps_ksp_contract_ok = baked_ps_first != 0 && baked_ps_first == uploaded_ps_first;
    intel_render_focus_log!(
        "intel/render: {} ps-ksp-proof accepted={} backend={} ksp0=0x{:X} ksp1=0x{:X} ksp2=0x{:X} ksp_off=0x{:X} first_dw=0x{:08X} baked_first=0x{:08X} dispatch={:?} does_not_prove=ps_thread_launch\n",
        submit_name,
        ps_ksp_contract_ok as u8,
        backend_probe_mode.label(),
        ps_ksp0,
        ps_ksp1,
        ps_ksp2,
        ps_ksp_packet_offset,
        uploaded_ps_first,
        baked_ps_first,
        pipeline.ps.meta.kernel.dispatch_mode,
    );

    intel_render_verbose_log!(
        "intel/render: {} staged rt=0x{:X} vb=0x{:X} state=0x{:X} used_end=0x{:X} state_off=0x{:X} state_region=0x{:X} free=0x{:X} size={}x{} pitch=0x{:X} vertices={} stride={} geometry={} backend={} status=pipeline-ready vs_bytes={} vs_off=0x{:X} vs_gpu=0x{:X} vs_ksp_off=0x{:X} vs_ksp=0x{:X} ps_bytes={} ps_off=0x{:X} ps_gpu=0x{:X} ps_ksp_off=0x{:X} ps_ksp=0x{:X} varyings={} ps_dispatch={:?}\n",
        submit_name,
        draw.rt_gpu_addr,
        draw.vertex_gpu_addr,
        draw.state_gpu_addr,
        shader_layout.used_bytes,
        shader_layout.state_region_offset_bytes,
        shader_layout.state_region_gpu_addr,
        warm.draw_state_len
            .saturating_sub(shader_layout.state_region_offset_bytes as usize),
        draw.target_w,
        draw.target_h,
        draw.rt_pitch,
        draw.vertex_count,
        draw.vertex_stride,
        geometry.label(),
        backend_probe_mode.label(),
        shader_layout.vs.code_size_bytes,
        shader_layout.vs.code_offset_bytes,
        shader_layout.vs.code_gpu_addr,
        shader_layout.vs.ksp_offset_bytes,
        shader_layout.vs.ksp_gpu_addr,
        shader_layout.ps.code_size_bytes,
        shader_layout.ps.code_offset_bytes,
        shader_layout.ps.code_gpu_addr,
        shader_layout.ps.ksp_offset_bytes,
        shader_layout.ps.ksp_gpu_addr,
        pipeline.ps.meta.num_varying_inputs,
        pipeline.ps.meta.kernel.dispatch_mode
    );

    let probe_state = match write_triangle_probe_state(warm, draw, shader_layout, blend_mode) {
        Ok(layout) => layout,
        Err(reason) => {
            crate::log!(
                "intel/render: {} staging skipped reason=probe-state-error detail={}\n",
                submit_name,
                reason
            );
            return false;
        }
    };

    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
    }
    seed_result_debug_slots(warm);
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let batch_tail_bytes = match encode_triangle_probe_batch(
        batch,
        warm,
        draw,
        blend_mode,
        pipeline,
        shader_layout,
        probe_state,
        GPU_VA_RESULT_BASE,
        RCS_EXEC_RESULT_DRAW_PRE3D,
        RCS_EXEC_RESULT_DRAW_POST3D,
        RCS_EXEC_RESULT_DONE,
        TriangleBatchMode::VfDraw,
        StreamoutProofExperiment::PositionSlot1,
        TRIANGLE_DEFAULT_FRONT_END_CONTRACT,
        backend_probe_mode,
        post_draw_sync_variant,
    ) {
        Ok(bytes) => bytes,
        Err(reason) => {
            crate::log!(
                "intel/render: {} staging skipped reason=probe-batch-error detail={}\n",
                submit_name,
                reason
            );
            return false;
        }
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);

    intel_render_verbose_log!(
        "intel/render: {} batch-ready bytes=0x{:X} bt_off=0x{:X} samp_off=0x{:X} blend_off=0x{:X} cc_state_off=0x{:X} cc_vp_off=0x{:X} sf_vp_off=0x{:X} geometry={}\n",
        submit_name,
        batch_tail_bytes,
        probe_state.binding_table_offset_bytes,
        probe_state.sampler_state_offset_bytes,
        probe_state.blend_state_offset_bytes,
        probe_state.color_calc_state_offset_bytes,
        probe_state.cc_viewport_offset_bytes,
        probe_state.sf_clip_viewport_offset_bytes,
        geometry.label(),
    );
    intel_render_verbose_log!(
        "intel/render: {} blend-probe={} geometry={}\n",
        submit_name,
        blend_mode.label(),
        geometry.label(),
    );
    log_triangle_probe_state(warm, shader_layout, probe_state);

    let completed = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_DONE,
        RESULT_SLOT_FINAL_DWORD,
        submit_name,
    );
    if !completed {
        recover_render_engine_after_nonretired_submit(dev, warm, submit_name);
    }
    completed
}

fn submit_triangle_vs_streamout_proof(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    experiment: StreamoutProofExperiment,
) -> bool {
    let Some(draw) = prepare_triangle_draw_resources(warm, dst_gpu_addr, pitch, rect_w, rect_h)
    else {
        crate::log!(
            "intel/render: vs-streamout-proof skipped reason=resource-layout size={}x{} pitch=0x{:X}\n",
            rect_w,
            rect_h,
            pitch
        );
        return false;
    };
    let pipeline = crate::intel::shader::triangle_pipeline();
    if crate::intel::shader::triangle_pipeline_is_placeholder() {
        crate::log!("intel/render: vs-streamout-proof skipped reason=placeholder-pipeline\n");
        return false;
    }
    let slice_hash_table_offset = match write_vf_streamout_probe_state(warm) {
        Ok(offset) => offset,
        Err(reason) => {
            crate::log!(
                "intel/render: vs-streamout-proof skipped reason=probe-state detail={}\n",
                reason
            );
            return false;
        }
    };
    let shader_layout = match upload_triangle_shader_pipeline(warm, pipeline) {
        Ok(layout) => layout,
        Err(reason) => {
            crate::log!(
                "intel/render: vs-streamout-proof skipped reason=shader-layout detail={}\n",
                reason
            );
            return false;
        }
    };
    if slice_hash_table_offset != 0
        && usize::try_from(shader_layout.used_bytes)
            .ok()
            .unwrap_or(usize::MAX)
            > slice_hash_table_offset as usize
    {
        crate::log!(
            "intel/render: vs-streamout-proof skipped reason=slice-hash-overlap used_end=0x{:X} slice_hash_off=0x{:X}\n",
            shader_layout.used_bytes,
            slice_hash_table_offset
        );
        return false;
    }

    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
        core::ptr::write_bytes(warm.streamout_virt, 0, warm.streamout_len);
    }
    seed_result_debug_slots(warm);
    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    crate::intel::dma_flush(warm.streamout_virt, warm.streamout_len);

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let batch_tail_bytes = match encode_vs_streamout_proof_batch(
        batch,
        warm,
        draw,
        GPU_VA_RESULT_BASE,
        RCS_EXEC_RESULT_DRAW_PRE3D,
        RCS_EXEC_RESULT_DRAW_POST3D,
        RCS_EXEC_RESULT_DONE,
        experiment,
        slice_hash_table_offset,
        VsStreamoutProofConfig {
            pipeline,
            shader_layout,
        },
    ) {
        Ok(bytes) => bytes,
        Err(reason) => {
            crate::log!("intel/render: vs-streamout-proof batch build failed detail={}\n", reason);
            return false;
        }
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);
    intel_render_verbose_log!(
        "intel/render: vs-streamout-proof batch-ready experiment={} bytes=0x{:X} so_gpu=0x{:X} so_pitch={} vertices={}\n",
        experiment.label(),
        batch_tail_bytes,
        GPU_VA_STREAMOUT_BASE,
        experiment.vertex_bytes(),
        draw.vertex_count
    );

    let stats_before = capture_triangle_stage_stats(dev);
    let completed = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_DONE,
        RESULT_SLOT_FINAL_DWORD,
        "vs-streamout-proof",
    );
    let stats_after = capture_triangle_stage_stats(dev);
    let accepted = completed
        || maybe_soft_accept_streamout_submit(
            "vs-streamout-proof",
            warm,
            stats_before,
            stats_after,
            true,
            experiment.vertex_bytes() * draw.vertex_count as usize,
        );
    log_streamout_proof_result(
        "vs-streamout-proof",
        warm,
        completed,
        draw.vertex_count as usize,
        experiment,
    );
    if !completed {
        recover_render_engine_after_nonretired_submit(dev, warm, "vs-streamout-proof");
    }
    accepted
}

fn submit_triangle_streamout_proof(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    experiment: StreamoutProofExperiment,
) -> bool {
    let Some(draw) = prepare_triangle_draw_resources(warm, dst_gpu_addr, pitch, rect_w, rect_h)
    else {
        crate::log!(
            "intel/render: streamout-proof skipped reason=resource-layout size={}x{} pitch=0x{:X}\n",
            rect_w,
            rect_h,
            pitch
        );
        return false;
    };
    let pipeline = crate::intel::shader::triangle_pipeline();
    if crate::intel::shader::triangle_pipeline_is_placeholder() {
        crate::log!("intel/render: streamout-proof skipped reason=placeholder-pipeline\n");
        return false;
    }
    let shader_layout = match upload_triangle_shader_pipeline(warm, pipeline) {
        Ok(layout) => layout,
        Err(reason) => {
            crate::log!(
                "intel/render: streamout-proof skipped reason=shader-layout detail={}\n",
                reason
            );
            return false;
        }
    };
    let probe_state = match write_triangle_probe_state(
        warm,
        draw,
        shader_layout,
        TriangleBlendProbeMode::ExplicitRt0,
    ) {
        Ok(layout) => layout,
        Err(reason) => {
            crate::log!(
                "intel/render: streamout-proof skipped reason=probe-state detail={}\n",
                reason
            );
            return false;
        }
    };

    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
        core::ptr::write_bytes(warm.streamout_virt, 0, warm.streamout_len);
    }
    seed_result_debug_slots(warm);
    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    crate::intel::dma_flush(warm.streamout_virt, warm.streamout_len);

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let batch_tail_bytes = match encode_triangle_probe_batch(
        batch,
        warm,
        draw,
        TriangleBlendProbeMode::ExplicitRt0,
        pipeline,
        shader_layout,
        probe_state,
        GPU_VA_RESULT_BASE,
        RCS_EXEC_RESULT_DRAW_PRE3D,
        RCS_EXEC_RESULT_DRAW_POST3D,
        RCS_EXEC_RESULT_DONE,
        TriangleBatchMode::StreamoutProof,
        experiment,
        TRIANGLE_DEFAULT_FRONT_END_CONTRACT,
        BackendProbeMode::MesaLike,
        PostDrawSyncVariant::HeavyAll,
    ) {
        Ok(bytes) => bytes,
        Err(reason) => {
            crate::log!("intel/render: streamout-proof batch build failed detail={}\n", reason);
            return false;
        }
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);
    intel_render_verbose_log!(
        "intel/render: streamout-proof batch-ready experiment={} bytes=0x{:X} so_gpu=0x{:X} so_pitch={} vertices={}\n",
        experiment.label(),
        batch_tail_bytes,
        GPU_VA_STREAMOUT_BASE,
        experiment.vertex_bytes(),
        draw.vertex_count
    );

    let stats_before = capture_triangle_stage_stats(dev);
    let completed = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_DONE,
        RESULT_SLOT_FINAL_DWORD,
        "streamout-proof",
    );
    let stats_after = capture_triangle_stage_stats(dev);
    let accepted = completed
        || maybe_soft_accept_streamout_submit(
            "streamout-proof",
            warm,
            stats_before,
            stats_after,
            true,
            experiment.vertex_bytes() * draw.vertex_count as usize,
        );
    log_streamout_proof_result(
        "streamout-proof",
        warm,
        completed,
        draw.vertex_count as usize,
        experiment,
    );
    if !completed {
        recover_render_engine_after_nonretired_submit(dev, warm, "streamout-proof");
    }
    accepted
}

fn submit_triangle_vs_draw_frontier_to_surface(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    blend_mode: TriangleBlendProbeMode,
) -> bool {
    for contract in VS_DRAW_FRONTIER_CONTRACTS {
        let completed = submit_triangle_real_vs_draw_probe_to_surface(
            dev,
            warm,
            dst_gpu_addr,
            pitch,
            rect_w,
            rect_h,
            blend_mode,
            "vs-draw-frontier",
            contract,
        );
        intel_render_verbose_log!(
            "intel/render: primary-vs-draw-frontier-contract variant={} completed={}\n",
            contract.label,
            completed as u8,
        );
        if completed {
            return true;
        }
    }
    false
}

fn wait_eq(dev: crate::intel::Dev, reg: usize, mask: u32, want: u32, n: usize) -> bool {
    for _ in 0..n {
        if (crate::intel::mmio_read(dev, reg) & mask) == want {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

fn map_smoke_buffers(dev: crate::intel::Dev, warm: RenderWarmState) -> bool {
    let ok_ring = super::map_ggtt(dev, warm.ring_phys, warm.ring_len, GPU_VA_RING_BASE);
    let ok_context = super::map_ggtt(dev, warm.context_phys, warm.context_len, GPU_VA_CONTEXT_BASE);
    let ok_batch = super::map_ggtt(dev, warm.batch_phys, warm.batch_len, GPU_VA_BATCH_BASE);
    let ok_draw_state =
        super::map_ggtt(dev, warm.draw_state_phys, warm.draw_state_len, GPU_VA_DRAW_STATE_BASE);
    let ok_vertex = super::map_ggtt(dev, warm.vertex_phys, warm.vertex_len, GPU_VA_VERTEX_BASE);
    let ok_result = super::map_ggtt(dev, warm.result_phys, warm.result_len, GPU_VA_RESULT_BASE);
    let ok_streamout =
        super::map_ggtt(dev, warm.streamout_phys, warm.streamout_len, GPU_VA_STREAMOUT_BASE);
    if ok_ring && ok_context && ok_batch && ok_draw_state && ok_vertex && ok_result && ok_streamout
    {
        super::ggtt_invalidate(dev);
        true
    } else {
        false
    }
}

fn ensure_smoke_buffers_mapped(dev: crate::intel::Dev, warm: RenderWarmState) -> bool {
    if WARM_BUFFERS_MAPPED.load(Ordering::Acquire) {
        return true;
    }
    if !map_smoke_buffers(dev, warm) {
        return false;
    }
    WARM_BUFFERS_MAPPED.store(true, Ordering::Release);
    true
}

fn should_log_primary_probe(reason: &str, seq: u32) -> bool {
    reason == "boot-once" || seq <= 3 || seq.is_multiple_of(PRIMARY_PERIODIC_LOG_EVERY)
}

fn should_log_primary_probe_detail() -> bool {
    if crate::logflag::INTEL_STAGE1_LOGS {
        return false;
    }
    let seq = PRIMARY_PROBE_SEQ.load(Ordering::Acquire);
    seq <= 3 || seq.is_multiple_of(PRIMARY_PERIODIC_LOG_EVERY)
}

fn submit_triangle_draw_to_surface(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    blend_mode: TriangleBlendProbeMode,
) -> bool {
    submit_triangle_real_vs_draw_probe_to_surface(
        dev,
        warm,
        dst_gpu_addr,
        pitch,
        rect_w,
        rect_h,
        blend_mode,
        "draw-path",
        TRIANGLE_DEFAULT_FRONT_END_CONTRACT,
    )
}

fn submit_triangle_real_vs_draw_probe_to_surface(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    blend_mode: TriangleBlendProbeMode,
    submit_name: &'static str,
    front_end_contract: TriangleFrontEndContract,
) -> bool {
    let Some(draw) = prepare_triangle_draw_resources(warm, dst_gpu_addr, pitch, rect_w, rect_h)
    else {
        crate::log!(
            "intel/render: {} staging skipped reason=resource-layout size={}x{} pitch=0x{:X}\n",
            submit_name,
            rect_w,
            rect_h,
            pitch
        );
        return false;
    };

    let pipeline = crate::intel::shader::triangle_pipeline();
    log_render_buffer_layout(warm, Some(dst_gpu_addr));
    log_render_packet_encodings();
    if crate::intel::shader::triangle_pipeline_is_placeholder() {
        crate::log!(
            "intel/render: {} staged rt=0x{:X} vb=0x{:X} state=0x{:X} size={}x{} pitch=0x{:X} vertices={} stride={} status=awaiting-baked-shaders vs_src={} ps_src={} note={}\n",
            submit_name,
            draw.rt_gpu_addr,
            draw.vertex_gpu_addr,
            draw.state_gpu_addr,
            draw.target_w,
            draw.target_h,
            draw.rt_pitch,
            draw.vertex_count,
            draw.vertex_stride,
            crate::intel::shader::TRIANGLE_VERTEX_SOURCE_PATH,
            crate::intel::shader::TRIANGLE_FRAGMENT_SOURCE_PATH,
            crate::intel::shader::triangle_pipeline_note()
        );
        return false;
    }

    intel_render_verbose_log!(
        "intel/render: {} ps-meta dispatch={:?} grf_start={} grf_used={} ksp_off=0x{:X} size={} header_only={} note={}\n",
        submit_name,
        pipeline.ps.meta.kernel.dispatch_mode,
        pipeline.ps.meta.kernel.grf_start_register,
        pipeline.ps.meta.kernel.grf_used,
        pipeline.ps.meta.kernel.ksp_offset_bytes,
        pipeline.ps.meta.kernel.code_size_bytes,
        (pipeline.ps.meta.num_varying_inputs == 0
            && pipeline.ps.meta.kernel.push_constant_bytes == 0) as u8,
        crate::intel::shader::triangle_pipeline_note()
    );
    let programmed_vs_urb_output_length = front_end_contract
        .vs_urb_output_length_override
        .or(TRIANGLE_VS_URB_OUTPUT_LENGTH_OVERRIDE)
        .unwrap_or(pipeline.vs.meta.urb_entry_output_length);
    if submit_name == "vs-draw-frontier" {
        intel_render_focus_log!(
            "intel/render: {} contract variant={} baked_vs_urb_out_len={} programmed_vs_urb_out_len={} sbe[read_offset={} read_length={} force_offset={} force_length={} num_sf_attrs={}]\n",
            submit_name,
            front_end_contract.label,
            pipeline.vs.meta.urb_entry_output_length,
            programmed_vs_urb_output_length,
            front_end_contract.sbe_read_offset,
            front_end_contract.sbe_read_length,
            front_end_contract.force_sbe_read_offset as u8,
            front_end_contract.force_sbe_read_length as u8,
            pipeline.ps.meta.num_varying_inputs,
        );
    } else {
        intel_render_verbose_log!(
            "intel/render: {} contract variant={} baked_vs_urb_out_len={} programmed_vs_urb_out_len={} sbe[read_offset={} read_length={} force_offset={} force_length={} num_sf_attrs={}]\n",
            submit_name,
            front_end_contract.label,
            pipeline.vs.meta.urb_entry_output_length,
            programmed_vs_urb_output_length,
            front_end_contract.sbe_read_offset,
            front_end_contract.sbe_read_length,
            front_end_contract.force_sbe_read_offset as u8,
            front_end_contract.force_sbe_read_length as u8,
            pipeline.ps.meta.num_varying_inputs,
        );
    }

    let shader_layout = match upload_triangle_shader_pipeline(warm, pipeline) {
        Ok(layout) => layout,
        Err(reason) => {
            crate::log!(
                "intel/render: {} staging skipped reason=shader-layout-error detail={} note={}\n",
                submit_name,
                reason,
                crate::intel::shader::triangle_pipeline_note()
            );
            return false;
        }
    };
    log_uploaded_triangle_shader_verification(warm, pipeline, shader_layout, submit_name);

    intel_render_verbose_log!(
        "intel/render: {} staged rt=0x{:X} vb=0x{:X} state=0x{:X} used_end=0x{:X} state_off=0x{:X} state_region=0x{:X} free=0x{:X} size={}x{} pitch=0x{:X} vertices={} stride={} status=pipeline-ready vs_bytes={} vs_off=0x{:X} vs_gpu=0x{:X} vs_ksp_off=0x{:X} vs_ksp=0x{:X} ps_bytes={} ps_off=0x{:X} ps_gpu=0x{:X} ps_ksp_off=0x{:X} ps_ksp=0x{:X} varyings={} ps_dispatch={:?}\n",
        submit_name,
        draw.rt_gpu_addr,
        draw.vertex_gpu_addr,
        draw.state_gpu_addr,
        shader_layout.used_bytes,
        shader_layout.state_region_offset_bytes,
        shader_layout.state_region_gpu_addr,
        warm.draw_state_len
            .saturating_sub(shader_layout.state_region_offset_bytes as usize),
        draw.target_w,
        draw.target_h,
        draw.rt_pitch,
        draw.vertex_count,
        draw.vertex_stride,
        shader_layout.vs.code_size_bytes,
        shader_layout.vs.code_offset_bytes,
        shader_layout.vs.code_gpu_addr,
        shader_layout.vs.ksp_offset_bytes,
        shader_layout.vs.ksp_gpu_addr,
        shader_layout.ps.code_size_bytes,
        shader_layout.ps.code_offset_bytes,
        shader_layout.ps.code_gpu_addr,
        shader_layout.ps.ksp_offset_bytes,
        shader_layout.ps.ksp_gpu_addr,
        pipeline.ps.meta.num_varying_inputs,
        pipeline.ps.meta.kernel.dispatch_mode
    );

    let probe_state = match write_triangle_probe_state(warm, draw, shader_layout, blend_mode) {
        Ok(layout) => layout,
        Err(reason) => {
            crate::log!(
                "intel/render: {} staging skipped reason=probe-state-error detail={}\n",
                submit_name,
                reason
            );
            return false;
        }
    };

    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
    }
    seed_result_debug_slots(warm);
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let batch_tail_bytes = match encode_triangle_probe_batch(
        batch,
        warm,
        draw,
        blend_mode,
        pipeline,
        shader_layout,
        probe_state,
        GPU_VA_RESULT_BASE,
        RCS_EXEC_RESULT_DRAW_PRE3D,
        RCS_EXEC_RESULT_DRAW_POST3D,
        RCS_EXEC_RESULT_DONE,
        TriangleBatchMode::Draw,
        StreamoutProofExperiment::PositionSlot1,
        front_end_contract,
        BackendProbeMode::MesaLike,
        PostDrawSyncVariant::HeavyAll,
    ) {
        Ok(bytes) => bytes,
        Err(reason) => {
            crate::log!(
                "intel/render: {} staging skipped reason=probe-batch-error detail={}\n",
                submit_name,
                reason
            );
            return false;
        }
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);

    intel_render_verbose_log!(
        "intel/render: {} batch-ready bytes=0x{:X} bt_off=0x{:X} samp_off=0x{:X} blend_off=0x{:X} cc_state_off=0x{:X} cc_vp_off=0x{:X} sf_vp_off=0x{:X}\n",
        submit_name,
        batch_tail_bytes,
        probe_state.binding_table_offset_bytes,
        probe_state.sampler_state_offset_bytes,
        probe_state.blend_state_offset_bytes,
        probe_state.color_calc_state_offset_bytes,
        probe_state.cc_viewport_offset_bytes,
        probe_state.sf_clip_viewport_offset_bytes
    );
    intel_render_verbose_log!("intel/render: {} blend-probe={}\n", submit_name, blend_mode.label());
    log_triangle_probe_state(warm, shader_layout, probe_state);

    let completed = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_DONE,
        RESULT_SLOT_FINAL_DWORD,
        submit_name,
    );
    if !completed {
        recover_render_engine_after_nonretired_submit(dev, warm, submit_name);
    }
    completed
}

fn submit_result_store_probe(dev: crate::intel::Dev, warm: RenderWarmState) -> bool {
    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
        core::ptr::write_volatile(warm.result_virt as *mut u32, 0xC0DE_7700);
    }
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let Ok(batch_tail_bytes) =
        encode_result_store_probe_batch(batch, GPU_VA_RESULT_BASE, RCS_EXEC_RESULT_MI_PROBE_DONE)
    else {
        crate::log!("intel/render: mi-store-probe batch build failed\n");
        return false;
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);
    submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_MI_PROBE_DONE,
        RESULT_SLOT_PRE3D_DWORD,
        "mi-store-probe",
    )
}

fn submit_3d_no_draw_probe(dev: crate::intel::Dev, warm: RenderWarmState) -> bool {
    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
        core::ptr::write_volatile(warm.result_virt as *mut u32, 0xC0DE_7700);
        core::ptr::write_volatile((warm.result_virt as *mut u32).add(1), 0xC0DE_7700);
        core::ptr::write_volatile((warm.result_virt as *mut u32).add(2), 0xC0DE_7700);
    }
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let Ok(batch_tail_bytes) = encode_3d_no_draw_probe_batch(
        batch,
        warm,
        GPU_VA_RESULT_BASE + (RESULT_SLOT_POST3D_DWORD as u64) * 4,
        RCS_EXEC_RESULT_3D_NO_DRAW_DONE,
    ) else {
        crate::log!("intel/render: 3d-no-draw-probe batch build failed\n");
        return false;
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);
    submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_3D_NO_DRAW_DONE,
        RESULT_SLOT_POST3D_DWORD,
        "3d-no-draw",
    )
}
