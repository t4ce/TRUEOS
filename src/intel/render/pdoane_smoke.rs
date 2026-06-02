const PDOANE_SMOKE_ENABLED: bool = true;
const PDOANE_SMOKE_SUBMIT_NAME: &str = "pdoane-smoke";
static PDOANE_SMOKE_SUBMITTED: AtomicBool = AtomicBool::new(false);

fn submit_pdoane_smoke_once(reason: &'static str) -> bool {
    if !PDOANE_SMOKE_ENABLED {
        return false;
    }
    if PDOANE_SMOKE_SUBMITTED.swap(true, Ordering::AcqRel) {
        intel_render_focus_log!(
            "intel/render: pdoane-smoke skipped reason=already-submitted trigger={}\n",
            reason
        );
        return false;
    }

    let Some(dev) = crate::intel::claimed_device() else {
        crate::log!("intel/render: pdoane-smoke skipped reason=no-device trigger={}\n", reason);
        return false;
    };
    let Some(surface_gpu) = crate::intel::display::primary_surface_gpu_addr() else {
        crate::log!("intel/render: pdoane-smoke skipped reason=no-surface trigger={}\n", reason);
        return false;
    };
    let Some((width, height)) = crate::intel::display::active_scanout_dimensions() else {
        crate::log!("intel/render: pdoane-smoke skipped reason=no-dimensions trigger={}\n", reason);
        return false;
    };
    let Some(pitch_bytes) = width
        .checked_mul(4)
        .and_then(|v| crate::intel::align_up(v as usize, 64))
    else {
        crate::log!(
            "intel/render: pdoane-smoke skipped reason=bad-pitch trigger={} width={}\n",
            reason,
            width
        );
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
        crate::log!("intel/render: pdoane-smoke skipped reason=warm-buffers trigger={}\n", reason);
        return false;
    }
    if !forcewake_render_acquire(warm) {
        crate::log!("intel/render: pdoane-smoke skipped reason=forcewake trigger={}\n", reason);
        return false;
    }
    if !ensure_smoke_buffers_mapped(dev, warm) {
        crate::log!("intel/render: pdoane-smoke skipped reason=ggtt-map trigger={}\n", reason);
        return false;
    }

    let Some(draw) = prepare_triangle_draw_resources(
        warm,
        surface_gpu,
        pitch_bytes,
        width as usize,
        height as usize,
    ) else {
        crate::log!(
            "intel/render: pdoane-smoke skipped reason=draw-resources trigger={} size={}x{} pitch=0x{:X}\n",
            reason,
            width,
            height,
            pitch_bytes
        );
        return false;
    };

    let pipeline = crate::intel::shader::triangle_pipeline();
    if crate::intel::shader::triangle_pipeline_is_placeholder() {
        crate::log!(
            "intel/render: pdoane-smoke skipped reason=placeholder-pipeline vs_src={} ps_src={} note={}\n",
            crate::intel::shader::TRIANGLE_VERTEX_SOURCE_PATH,
            crate::intel::shader::TRIANGLE_FRAGMENT_SOURCE_PATH,
            crate::intel::shader::triangle_pipeline_note()
        );
        return false;
    }

    let shader_layout = match upload_triangle_shader_pipeline(warm, pipeline) {
        Ok(layout) => layout,
        Err(reason) => {
            crate::log!(
                "intel/render: pdoane-smoke skipped reason=shader-layout detail={}\n",
                reason
            );
            return false;
        }
    };
    let blend_mode = TriangleBlendProbeMode::MesaZeroedState;
    let backend_mode = BackendProbeMode::MesaLike;
    let probe_state =
        match write_triangle_probe_state(warm, draw, shader_layout, blend_mode, backend_mode) {
            Ok(layout) => layout,
            Err(reason) => {
                crate::log!(
                    "intel/render: pdoane-smoke skipped reason=probe-state detail={}\n",
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
        TRIANGLE_DEFAULT_FRONT_END_CONTRACT,
        backend_mode,
        PostDrawSyncVariant::LightOnlyRetire,
    ) {
        Ok(bytes) => bytes,
        Err(reason) => {
            crate::log!(
                "intel/render: pdoane-smoke skipped reason=batch-encode detail={}\n",
                reason
            );
            return false;
        }
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);

    intel_render_focus_log!(
        "intel/render: pdoane-smoke batch-ready trigger={} bytes=0x{:X} rt=0x{:X} vb=0x{:X} state=0x{:X} size={}x{} pitch=0x{:X} backend={} shape=single-path state_sequence=pipeline_select-sba-pointers-urb-vb-ve-vs-disable_hs_te_ds_gs-clip-sf-sbe-wm-ps-null_depth-dummy_3dprimitive-real_3dprimitive source=pdoane-osdev-gfx-gfx_c analogy=not-gen7-packets\n",
        reason,
        batch_tail_bytes,
        draw.rt_gpu_addr,
        draw.vertex_gpu_addr,
        draw.state_gpu_addr,
        draw.target_w,
        draw.target_h,
        draw.rt_pitch,
        backend_mode.label(),
    );

    let completed = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_DONE,
        RESULT_SLOT_FINAL_DWORD,
        PDOANE_SMOKE_SUBMIT_NAME,
    );
    intel_render_focus_log!(
        "intel/render: pdoane-smoke result completed={} trigger={} note=single-end-to-end-render-smoke-kept-outside-old-probe-ladder\n",
        completed as u8,
        reason
    );
    completed
}
