fn submit_sprite_quad_worklist(
    src: GpgpuRgba8Surface,
    dst: GpgpuRgba8Surface,
    desc: GpgpuRectWorklistDescBuffer,
    params: SpriteQuadWorklistRgba8Params,
) -> bool {
    submit_known_descriptor_worklist_sprite_quad(src, dst, desc, params)
}

fn submit_known_descriptor_worklist_sprite_quad(
    src: GpgpuRgba8Surface,
    dst: GpgpuRgba8Surface,
    desc: GpgpuRectWorklistDescBuffer,
    params: SpriteQuadWorklistRgba8Params,
) -> bool {
    if params.desc_count == 0 || params.desc_count as usize > SPRITE_QUAD_WORKLIST_MAX_DESCS {
        return false;
    }
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        return false;
    };
    let Some(upload) = upload_sprite_quad_worklist_rgba8_kernel() else {
        return false;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        return false;
    };

    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ppgtt_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let src_ppgtt_ok =
        kernel_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, src.gpu, src.phys, src.bytes);
    let dst_ppgtt_ok =
        src_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, dst.gpu, dst.phys, dst.bytes);
    let desc_ppgtt_ok =
        dst_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, desc.gpu, desc.phys, desc.bytes);
    let batch_ok = desc_ppgtt_ok
        && direct_rcs_encode_sprite_quad_worklist_batch(
            state, upload, params, src.bytes, dst.bytes, desc.bytes,
        );
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result_slot(
            state,
            SPRITE_QUAD_WORKLIST_POST_MARKER_SLOT,
            SPRITE_QUAD_WORKLIST_POST_MARKER,
        )
    } else {
        0
    };
    if observed != SPRITE_QUAD_WORKLIST_POST_MARKER {
        let fail_count = SPRITE_QUAD_WORKLIST_SUBMIT_FAIL_LOGS.fetch_add(1, Ordering::Relaxed) + 1;
        if fail_count <= 16 || fail_count.is_power_of_two() {
            crate::log!(
                "intel/gpgpu: sprite-quad-worklist submit failed count={} forcewake={} mapped={} ppgtt={} kernel={} src={} dst={} desc={} batch={} submitted={} observed=0x{:X} want=0x{:X} ppgtt_limit=0x{:X} upload_gpu=0x{:X} src_gpu=0x{:X} src_end=0x{:X} dst_gpu=0x{:X} dst_end=0x{:X} dst_bytes=0x{:X} desc_gpu=0x{:X} desc_end=0x{:X} desc_count={}\n",
                fail_count,
                forcewake_ok as u8,
                mapped_ok as u8,
                ppgtt_ok as u8,
                kernel_ppgtt_ok as u8,
                src_ppgtt_ok as u8,
                dst_ppgtt_ok as u8,
                desc_ppgtt_ok as u8,
                batch_ok as u8,
                submitted as u8,
                observed,
                SPRITE_QUAD_WORKLIST_POST_MARKER,
                direct_rcs_ppgtt_limit_bytes(),
                upload.gpu,
                src.gpu,
                src.gpu.saturating_add(src.bytes as u64),
                dst.gpu,
                dst.gpu.saturating_add(dst.bytes as u64),
                dst.bytes,
                desc.gpu,
                desc.gpu.saturating_add(desc.bytes as u64),
                params.desc_count
            );
        }
    }
    observed == SPRITE_QUAD_WORKLIST_POST_MARKER
}

fn submit_sprite_quad_worklist_runs(
    dst: GpgpuRgba8Surface,
    desc: GpgpuRectWorklistDescBuffer,
    runs: &[GpgpuSpriteQuadWorklistRun<'_>],
) -> GpgpuSubmissionOutcome {
    if runs.is_empty() {
        return GpgpuSubmissionOutcome::Unavailable;
    }
    let total_descs = runs
        .iter()
        .try_fold(0usize, |total, run| total.checked_add(run.descs.len()));
    let Some(total_descs) = total_descs else {
        return GpgpuSubmissionOutcome::Unavailable;
    };
    if total_descs == 0 || total_descs > SPRITE_QUAD_WORKLIST_MAX_DESCS {
        return GpgpuSubmissionOutcome::Unavailable;
    }
    if runs.iter().any(|run| run.descs.is_empty()) {
        return GpgpuSubmissionOutcome::Unavailable;
    }

    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        return GpgpuSubmissionOutcome::Unavailable;
    };
    let Some(upload) = upload_sprite_quad_worklist_rgba8_kernel() else {
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
    let dst_ppgtt_ok =
        kernel_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, dst.gpu, dst.phys, dst.bytes);
    let desc_ppgtt_ok =
        dst_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, desc.gpu, desc.phys, desc.bytes);
    let mut src_ppgtt_ok = desc_ppgtt_ok;
    if src_ppgtt_ok {
        for run in runs {
            if !direct_rcs_map_ppgtt_kernel(state, run.src.gpu, run.src.phys, run.src.bytes) {
                src_ppgtt_ok = false;
                break;
            }
        }
    }
    let batch_ok = src_ppgtt_ok
        && direct_rcs_encode_sprite_quad_worklist_runs_batch(state, upload, dst, desc, runs);
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result_slot(
            state,
            SPRITE_QUAD_WORKLIST_POST_MARKER_SLOT,
            SPRITE_QUAD_WORKLIST_POST_MARKER,
        )
    } else {
        0
    };
    if observed != SPRITE_QUAD_WORKLIST_POST_MARKER {
        let fail_count = SPRITE_QUAD_WORKLIST_SUBMIT_FAIL_LOGS.fetch_add(1, Ordering::Relaxed) + 1;
        if fail_count <= 16 || fail_count.is_power_of_two() {
            crate::log!(
                "intel/gpgpu: sprite-quad-worklist-runs submit failed count={} forcewake={} mapped={} ppgtt={} kernel={} dst={} desc={} src={} batch={} submitted={} observed=0x{:X} want=0x{:X} runs={} descs={} ppgtt_limit=0x{:X} upload_gpu=0x{:X} dst_gpu=0x{:X} dst_end=0x{:X} desc_gpu=0x{:X} desc_end=0x{:X}\n",
                fail_count,
                forcewake_ok as u8,
                mapped_ok as u8,
                ppgtt_ok as u8,
                kernel_ppgtt_ok as u8,
                dst_ppgtt_ok as u8,
                desc_ppgtt_ok as u8,
                src_ppgtt_ok as u8,
                batch_ok as u8,
                submitted as u8,
                observed,
                SPRITE_QUAD_WORKLIST_POST_MARKER,
                runs.len(),
                total_descs,
                direct_rcs_ppgtt_limit_bytes(),
                upload.gpu,
                dst.gpu,
                dst.gpu.saturating_add(dst.bytes as u64),
                desc.gpu,
                desc.gpu.saturating_add(desc.bytes as u64)
            );
        }
    }
    if observed == SPRITE_QUAD_WORKLIST_POST_MARKER {
        GpgpuSubmissionOutcome::Complete
    } else if submitted {
        GpgpuSubmissionOutcome::SubmittedIncomplete
    } else {
        GpgpuSubmissionOutcome::Unavailable
    }
}

fn submit_fill_rect_worklist(
    dst: GpgpuRgba8Surface,
    desc: GpgpuRectWorklistDescBuffer,
    params: FillRectWorklistRgba8Params,
    direct_scanout: bool,
) -> bool {
    if params.desc_count == 0 || params.desc_count as usize > RECT_WORKLIST_MAX_DESCS {
        return false;
    }
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        return false;
    };
    let Some(upload) = upload_fill_rect_worklist_rgba8_kernel() else {
        return false;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        return false;
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
        && direct_rcs_encode_fill_rect_worklist_batch(state, upload, params, dst.bytes, desc.bytes);
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result_slot(
            state,
            RECT_WORKLIST_POST_MARKER_SLOT,
            FILL_RECT_WORKLIST_POST_MARKER,
        )
    } else {
        0
    };
    observed == FILL_RECT_WORKLIST_POST_MARKER
}

fn submit_mandel64_worklist(
    dst: GpgpuRgba8Surface,
    desc: GpgpuRectWorklistDescBuffer,
    params: Mandel64WorklistRgba8Params,
    direct_scanout: bool,
) -> DirectRcsDispatchOutcome {
    if params.desc_count == 0 || params.desc_count as usize > MANDEL64_WORKLIST_MAX_DESCS {
        return DirectRcsDispatchOutcome::default();
    }
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        return DirectRcsDispatchOutcome::default();
    };
    let Some(upload) = upload_mandel64_worklist_rgba8_kernel() else {
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
        && if direct_scanout {
            direct_rcs_map_ppgtt_scanout(state, dst.gpu, dst.phys, dst.bytes)
        } else {
            direct_rcs_map_ppgtt_kernel(state, dst.gpu, dst.phys, dst.bytes)
        };
    let desc_ppgtt_ok =
        dst_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, desc.gpu, desc.phys, desc.bytes);
    let batch_ok = desc_ppgtt_ok
        && direct_rcs_encode_mandel64_worklist_batch(state, upload, params, dst.bytes, desc.bytes);
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted && direct_scanout {
        direct_rcs_poll_result_slot_timeout_ms(
            state,
            RECT_WORKLIST_POST_MARKER_SLOT,
            MANDEL64_WORKLIST_POST_MARKER,
            UI4_COMPUTE_PRODUCER_RETIRE_TIMEOUT_MS,
        )
    } else if submitted {
        direct_rcs_poll_result_slot(
            state,
            RECT_WORKLIST_POST_MARKER_SLOT,
            MANDEL64_WORKLIST_POST_MARKER,
        )
    } else {
        0
    };
    if observed != MANDEL64_WORKLIST_POST_MARKER {
        if submitted && direct_scanout {
            quarantine_direct_rcs_context("mandel64-worklist-marker-timeout");
        }
        crate::log_error!(
            target: "gpgpu";
            "intel/gpgpu: mandel64-worklist failed direct_scanout={} mapped={} ppgtt={} kernel={} dst={} desc={} batch={} submitted={} observed=0x{:08X} want=0x{:08X} descs={} dst_gpu=0x{:X}\n",
            direct_scanout as u8,
            mapped_ok as u8,
            ppgtt_ok as u8,
            kernel_ppgtt_ok as u8,
            dst_ppgtt_ok as u8,
            desc_ppgtt_ok as u8,
            batch_ok as u8,
            submitted as u8,
            observed,
            MANDEL64_WORKLIST_POST_MARKER,
            params.desc_count,
            dst.gpu,
        );
    }
    DirectRcsDispatchOutcome {
        submitted,
        observed,
    }
}

fn sprite_quad_worklist_walker_count(desc_count: usize) -> usize {
    desc_count
        .div_ceil(SPRITE_QUAD_WORKLIST_DESCS_PER_WALKER)
        .min(SPRITE_QUAD_WORKLIST_MAX_WALKERS)
}

fn rect_worklist_walker_count(desc_count: usize) -> usize {
    desc_count
        .div_ceil(RECT_WORKLIST_DESCS_PER_WALKER)
        .min(RECT_WORKLIST_MAX_WALKERS)
}

fn mandel64_worklist_walker_count(desc_count: usize) -> usize {
    desc_count
        .div_ceil(MANDEL64_WORKLIST_DESCS_PER_WALKER)
        .min(MANDEL64_WORKLIST_MAX_WALKERS)
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

