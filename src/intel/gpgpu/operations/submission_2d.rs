fn clip_gpgpu_rect_to_surface(rect: GpgpuRect, width: u32, height: u32) -> Option<GpgpuRect> {
    if rect.is_empty() || width == 0 || height == 0 {
        return None;
    }
    let x0 = (rect.x as i64).max(0);
    let y0 = (rect.y as i64).max(0);
    let x1 = (rect.x as i64 + rect.width as i64).min(width as i64);
    let y1 = (rect.y as i64 + rect.height as i64).min(height as i64);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some(GpgpuRect::new(x0 as i32, y0 as i32, (x1 - x0) as u32, (y1 - y0) as u32))
}

fn lower_fill_rect(
    dst: GpgpuRgba8Surface,
    rect: GpgpuRect,
    color_rgba: u32,
) -> Option<FillRectRgba8Params> {
    if !dst.is_valid() || rect.is_empty() {
        return None;
    }
    let clipped = clip_rect_to_surface(rect, dst)?;
    Some(FillRectRgba8Params {
        dst_gpu: dst.gpu,
        dst_pitch_bytes: dst.pitch_bytes,
        dst_x: clipped.x as u32,
        dst_y: clipped.y as u32,
        width: clipped.width,
        height: clipped.height,
        color_rgba,
    })
}

fn lower_copy_rect(
    src: GpgpuRgba8Surface,
    src_rect: GpgpuRect,
    dst: GpgpuRgba8Surface,
    dst_xy: GpgpuPoint,
) -> Option<CopyRectRgba8Params> {
    if !src.is_valid() || !dst.is_valid() || src_rect.is_empty() {
        return None;
    }

    let mut sx = src_rect.x as i64;
    let mut sy = src_rect.y as i64;
    let mut dx = dst_xy.x as i64;
    let mut dy = dst_xy.y as i64;
    let mut width = src_rect.width as i64;
    let mut height = src_rect.height as i64;

    clip_copy_axis(&mut sx, &mut dx, &mut width, src.width as i64, dst.width as i64)?;
    clip_copy_axis(&mut sy, &mut dy, &mut height, src.height as i64, dst.height as i64)?;

    Some(CopyRectRgba8Params {
        src_gpu: src.gpu,
        dst_gpu: dst.gpu,
        src_pitch_bytes: src.pitch_bytes,
        dst_pitch_bytes: dst.pitch_bytes,
        src_x: sx as u32,
        src_y: sy as u32,
        dst_x: dx as u32,
        dst_y: dy as u32,
        width: width as u32,
        height: height as u32,
    })
}

fn lower_glyph_mask_blit(blit: GpgpuGlyphMaskBlit) -> Option<CopyRectRgba8Params> {
    if !blit.mask.is_valid() || !blit.dst.is_valid() || blit.mask_rect.is_empty() {
        return None;
    }

    let mut sx = blit.mask_rect.x as i64;
    let mut sy = blit.mask_rect.y as i64;
    let mut dx = blit.dst_xy.x as i64;
    let mut dy = blit.dst_xy.y as i64;
    let mut width = blit.mask_rect.width as i64;
    let mut height = blit.mask_rect.height as i64;

    clip_copy_axis(&mut sx, &mut dx, &mut width, blit.mask.width as i64, blit.dst.width as i64)?;
    clip_copy_axis(&mut sy, &mut dy, &mut height, blit.mask.height as i64, blit.dst.height as i64)?;

    Some(CopyRectRgba8Params {
        src_gpu: blit.mask.gpu,
        dst_gpu: blit.dst.gpu,
        src_pitch_bytes: blit.mask.pitch_bytes,
        dst_pitch_bytes: blit.dst.pitch_bytes,
        src_x: sx as u32,
        src_y: sy as u32,
        dst_x: dx as u32,
        dst_y: dy as u32,
        width: width as u32,
        height: height as u32,
    })
}

fn clip_rect_to_surface(rect: GpgpuRect, surface: GpgpuRgba8Surface) -> Option<GpgpuRect> {
    let mut x = rect.x as i64;
    let mut y = rect.y as i64;
    let mut width = rect.width as i64;
    let mut height = rect.height as i64;

    if x < 0 {
        width += x;
        x = 0;
    }
    if y < 0 {
        height += y;
        y = 0;
    }
    width = width.min(surface.width as i64 - x);
    height = height.min(surface.height as i64 - y);
    if width <= 0 || height <= 0 {
        return None;
    }
    Some(GpgpuRect::new(x as i32, y as i32, width as u32, height as u32))
}

fn clip_copy_axis(
    src_pos: &mut i64,
    dst_pos: &mut i64,
    len: &mut i64,
    src_limit: i64,
    dst_limit: i64,
) -> Option<()> {
    if *src_pos < 0 {
        let delta = -*src_pos;
        *src_pos = 0;
        *dst_pos += delta;
        *len -= delta;
    }
    if *dst_pos < 0 {
        let delta = -*dst_pos;
        *dst_pos = 0;
        *src_pos += delta;
        *len -= delta;
    }
    *len = (*len).min(src_limit - *src_pos).min(dst_limit - *dst_pos);
    if *len <= 0 { None } else { Some(()) }
}

fn submit_fill_rect_2d_with_stats(
    dst: GpgpuRgba8Surface,
    params: FillRectRgba8Params,
) -> GpgpuSubmitStats {
    let total_start_tick = direct_rcs_now_tick();
    let Some(dispatch) = fill_rect_2d_dispatch(params.width, params.height) else {
        return GpgpuSubmitStats::default();
    };
    let Some(total_spans) = (dispatch.group_x as usize).checked_mul(dispatch.group_y as usize)
    else {
        return GpgpuSubmitStats::default();
    };
    let submit_start_tick = direct_rcs_now_tick();
    if !submit_fill_rect_2d(dst, params) {
        return GpgpuSubmitStats {
            total_ms: direct_rcs_elapsed_ms_since(total_start_tick),
            ..GpgpuSubmitStats::default()
        };
    }
    GpgpuSubmitStats {
        spans: total_spans,
        submits: 1,
        submit_ms: direct_rcs_elapsed_ms_since(submit_start_tick),
        total_ms: direct_rcs_elapsed_ms_since(total_start_tick),
        ..GpgpuSubmitStats::default()
    }
}

fn submit_copy_rect_2d(
    src: GpgpuRgba8Surface,
    dst: GpgpuRgba8Surface,
    params: CopyRectRgba8Params,
    direct_scanout: bool,
) -> bool {
    if params.width == 0 || params.height == 0 {
        return false;
    }
    let Some(dispatch) = copy_rect_2d_dispatch(params.width, params.height) else {
        return false;
    };
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        return false;
    };
    let Some(upload) = upload_copy_rect_rgba8_kernel() else {
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
        kernel_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, params.src_gpu, src.phys, src.bytes);
    let dst_ppgtt_ok = src_ppgtt_ok
        && direct_rcs_map_ppgtt_destination(
            state,
            params.dst_gpu,
            dst.phys,
            dst.bytes,
            direct_scanout,
        );
    let batch_ok = dst_ppgtt_ok
        && direct_rcs_encode_copy_rect_2d_batch(state, upload, params, src.bytes, dst.bytes);
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result_slot_timeout_ms(
            state,
            COPY_RECT_POST_MARKER_SLOT,
            COPY_RECT_POST_MARKER,
            COPY_RECT_2D_COMPLETION_TIMEOUT_MS,
        )
    } else {
        0
    };
    let completed = observed == COPY_RECT_POST_MARKER;
    if !completed {
        let occurrence = COPY_RECT_2D_INCOMPLETE_SEQ.fetch_add(1, Ordering::Relaxed) + 1;
        if occurrence <= 8 || occurrence.is_multiple_of(20) {
            let pre_marker = direct_rcs_read_result_slot(state, COPY_RECT_PRE_MARKER_SLOT);
            let potential_reason = if !batch_ok {
                "batch-prepare"
            } else if !submitted {
                "guc-submit"
            } else if pre_marker != COPY_RECT_PRE_MARKER {
                "batch-not-started"
            } else {
                "walker-not-retired-before-timeout"
            };
            crate::log_warn!(
                target: "intel-gpgpu";
                "copy_rect_rgba8 2d incomplete occurrence={} rect={}x{} groups={}x{} pre=0x{:08X} post=0x{:08X} timeout_ms={} potential_reason={} action=fail-closed\n",
                occurrence,
                params.width,
                params.height,
                dispatch.group_x,
                dispatch.group_y,
                pre_marker,
                observed,
                COPY_RECT_2D_COMPLETION_TIMEOUT_MS,
                potential_reason,
            );
        }
    }
    completed
}

fn submit_fill_rect_2d(dst: GpgpuRgba8Surface, params: FillRectRgba8Params) -> bool {
    if params.width == 0 || params.height == 0 {
        return false;
    }
    let Some(dispatch) = fill_rect_2d_dispatch(params.width, params.height) else {
        return false;
    };
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        return false;
    };
    let Some(upload) = upload_fill_rect_rgba8_kernel() else {
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
    let dst_ppgtt_ok =
        kernel_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, dst.gpu, dst.phys, dst.bytes);
    let batch_ok =
        dst_ppgtt_ok && direct_rcs_encode_fill_rect_2d_batch(state, upload, params, dst.bytes);
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result_slot_timeout_ms(
            state,
            CLEAR_RECT_POST_MARKER_SLOT,
            CLEAR_RECT_POST_MARKER,
            FILL_RECT_2D_COMPLETION_TIMEOUT_MS,
        )
    } else {
        0
    };
    let completed = observed == CLEAR_RECT_POST_MARKER;
    if !completed {
        let occurrence = FILL_RECT_2D_INCOMPLETE_SEQ.fetch_add(1, Ordering::Relaxed) + 1;
        if occurrence <= 8 || occurrence.is_multiple_of(20) {
            let pre_marker = direct_rcs_read_result_slot(state, CLEAR_RECT_PRE_MARKER_SLOT);
            let potential_reason = if !batch_ok {
                "batch-prepare"
            } else if !submitted {
                "guc-submit"
            } else if pre_marker != CLEAR_RECT_PRE_MARKER {
                "batch-not-started"
            } else {
                "walker-not-retired-before-timeout"
            };
            crate::log_warn!(
                target: "intel-gpgpu";
                "fill_rect_rgba8 2d incomplete occurrence={} rect={}x{} groups={}x{} pre=0x{:08X} post=0x{:08X} timeout_ms={} potential_reason={} action=fail-closed\n",
                occurrence,
                params.width,
                params.height,
                dispatch.group_x,
                dispatch.group_y,
                pre_marker,
                observed,
                FILL_RECT_2D_COMPLETION_TIMEOUT_MS,
                potential_reason,
            );
        }
    }
    completed
}

fn submit_resolve_tile64_msaa4_2d(
    src: GpgpuRgba8Surface,
    dst: GpgpuRgba8Surface,
    params: CopyRectRgba8Params,
    direct_scanout: bool,
) -> bool {
    if params.width == 0 || params.height == 0 {
        return false;
    }
    let Some(dispatch) = fill_rect_2d_dispatch(params.width, params.height) else {
        return false;
    };
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        return false;
    };
    let Some(upload) = upload_resolve_tile64_msaa4_rgba8_kernel() else {
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
        kernel_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, params.src_gpu, src.phys, src.bytes);
    let dst_ppgtt_ok = src_ppgtt_ok
        && direct_rcs_map_ppgtt_destination(
            state,
            params.dst_gpu,
            dst.phys,
            dst.bytes,
            direct_scanout,
        );
    let batch_ok = dst_ppgtt_ok
        && direct_rcs_encode_resolve_tile64_msaa4_2d_batch(
            state, upload, params, src.bytes, dst.bytes,
        );
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result_slot_timeout_ms(
            state,
            COPY_RECT_POST_MARKER_SLOT,
            COPY_RECT_POST_MARKER,
            RESOLVE_TILE64_MSAA4_COMPLETION_TIMEOUT_MS,
        )
    } else {
        0
    };
    let completed = observed == COPY_RECT_POST_MARKER;
    if !completed {
        let occurrence = RESOLVE_TILE64_MSAA4_INCOMPLETE_SEQ.fetch_add(1, Ordering::Relaxed) + 1;
        if occurrence <= 8 || occurrence.is_multiple_of(20) {
            let pre_marker = direct_rcs_read_result_slot(state, COPY_RECT_PRE_MARKER_SLOT);
            let potential_reason = if !batch_ok {
                "batch-prepare"
            } else if !submitted {
                "guc-submit"
            } else if pre_marker != COPY_RECT_PRE_MARKER {
                "batch-not-started"
            } else {
                "walker-not-retired-before-timeout"
            };
            crate::log_warn!(
                target: "intel-gpgpu";
                "resolve_tile64_msaa4_rgba8 2d incomplete occurrence={} rect={}x{} groups={}x{} pre=0x{:08X} post=0x{:08X} timeout_ms={} potential_reason={} action=fail-closed\n",
                occurrence,
                params.width,
                params.height,
                dispatch.group_x,
                dispatch.group_y,
                pre_marker,
                observed,
                RESOLVE_TILE64_MSAA4_COMPLETION_TIMEOUT_MS,
                potential_reason,
            );
        }
    }
    completed
}

fn submit_font_outline_coverage_r8_2d(
    ops_phys: u64,
    ops_bytes: usize,
    mask: GpgpuMask8Surface,
    params: FontOutlineCoverageR8Params,
) -> GpgpuDispatchRetirement {
    let Some(dispatch) = fill_rect_2d_dispatch(params.rect_width, params.rect_height) else {
        return GpgpuDispatchRetirement::NotSubmitted;
    };
    // Font coverage is requested by asynchronous kernel/UI services. Never
    // spin a cooperative executor while another direct-RCS producer owns the
    // lane; the caller can preserve its resident fallback and retry later.
    let Some(_guard) = DIRECT_RCS_SUBMIT_LOCK.try_lock() else {
        return GpgpuDispatchRetirement::NotSubmitted;
    };
    let Some(dev) = super::claimed_device() else {
        return GpgpuDispatchRetirement::NotSubmitted;
    };
    let Some(upload) = upload_font_outline_coverage_r8_kernel() else {
        return GpgpuDispatchRetirement::NotSubmitted;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        return GpgpuDispatchRetirement::NotSubmitted;
    };
    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ppgtt_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let ops_ppgtt_ok =
        kernel_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, params.ops_gpu, ops_phys, ops_bytes);
    let mask_ppgtt_ok =
        ops_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, params.mask_gpu, mask.phys, mask.bytes);
    let batch_ok = mask_ppgtt_ok
        && direct_rcs_encode_font_outline_coverage_r8_2d_batch(
            state, upload, params, ops_bytes, mask.bytes,
        );
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result_slot_timeout_ms(
            state,
            COPY_RECT_POST_MARKER_SLOT,
            COPY_RECT_POST_MARKER,
            FONT_OUTLINE_COVERAGE_R8_COMPLETION_TIMEOUT_MS,
        )
    } else {
        0
    };
    let completed = observed == COPY_RECT_POST_MARKER;
    if !completed {
        let occurrence =
            FONT_OUTLINE_COVERAGE_R8_INCOMPLETE_SEQ.fetch_add(1, Ordering::Relaxed) + 1;
        if occurrence <= 8 || occurrence.is_multiple_of(20) {
            crate::log_warn!(
                target: "intel-gpgpu";
                "font_outline_coverage_r8 incomplete occurrence={} ops={} rect={}x{} groups={}x{} submitted={} post=0x{:08X} timeout_ms={} action={}\n",
                occurrence,
                params.op_count,
                params.rect_width,
                params.rect_height,
                dispatch.group_x,
                dispatch.group_y,
                submitted as u8,
                observed,
                FONT_OUTLINE_COVERAGE_R8_COMPLETION_TIMEOUT_MS,
                if submitted {
                    "quarantine-context+resources-no-fallback"
                } else {
                    "not-submitted"
                },
            );
        }
    }
    if completed {
        GpgpuDispatchRetirement::Complete
    } else if submitted {
        quarantine_direct_rcs_context("font-outline-coverage-marker-timeout");
        GpgpuDispatchRetirement::SubmittedIncomplete
    } else {
        GpgpuDispatchRetirement::NotSubmitted
    }
}

fn submit_glyph_mask_2d(
    mask: GpgpuMask8Surface,
    dst: GpgpuRgba8Surface,
    params: CopyRectRgba8Params,
    color_rgba: u32,
    direct_scanout: bool,
) -> bool {
    if fill_rect_2d_dispatch(params.width, params.height).is_none() {
        return false;
    }
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        return false;
    };
    let Some(upload) = upload_glyph_mask_rgba8_kernel() else {
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
    let mask_ppgtt_ok = kernel_ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, params.src_gpu, mask.phys, mask.bytes);
    let dst_ppgtt_ok = mask_ppgtt_ok
        && direct_rcs_map_ppgtt_destination(
            state,
            params.dst_gpu,
            dst.phys,
            dst.bytes,
            direct_scanout,
        );
    let batch_ok = dst_ppgtt_ok
        && direct_rcs_encode_glyph_mask_2d_batch(
            state, upload, params, color_rgba, mask.bytes, dst.bytes,
        );
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result_slot_timeout_ms(
            state,
            COPY_RECT_POST_MARKER_SLOT,
            COPY_RECT_POST_MARKER,
            RESOLVE_TILE64_MSAA4_COMPLETION_TIMEOUT_MS,
        )
    } else {
        0
    };
    observed == COPY_RECT_POST_MARKER
}

fn submit_glyph_mask_layers_2d(
    layers: &[GpgpuGlyphMaskLayer],
    dst: GpgpuRgba8Surface,
    direct_scanout: bool,
) -> (bool, bool) {
    if layers.is_empty() || layers.len() > GLYPH_MASK_BATCH_MAX_LAYERS {
        return (false, false);
    }
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        return (false, false);
    };
    let Some(upload) = upload_glyph_mask_rgba8_kernel() else {
        return (false, false);
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        return (false, false);
    };
    if !direct_rcs_forcewake(dev)
        || !direct_rcs_map_state(dev, state)
        || !direct_rcs_init_ppgtt(state)
        || !direct_rcs_map_ppgtt_region(
            state,
            upload.gpu,
            upload.phys,
            upload.mapped_bytes,
            direct_rcs_ppgtt_pte_flags(),
        )
        || !direct_rcs_map_ppgtt_destination(state, dst.gpu, dst.phys, dst.bytes, direct_scanout)
    {
        return (false, false);
    }
    for layer in layers {
        let blit = GpgpuGlyphMaskBlit {
            mask: layer.mask,
            mask_rect: layer.mask_rect,
            dst,
            dst_xy: layer.dst_xy,
            color_rgba: layer.color_rgba,
        };
        if lower_glyph_mask_blit(blit).is_none() {
            continue;
        }
        if !direct_rcs_map_ppgtt_region(
            state,
            layer.mask.gpu,
            layer.mask.phys,
            layer.mask.bytes,
            direct_rcs_ppgtt_pte_flags(),
        ) {
            return (false, false);
        }
    }
    // All scene masks share one address space and one submission. Publishing
    // their PTEs together avoids flushing the complete 2 MiB page table once
    // per layer.
    super::dma_flush(state.ppgtt_virt, DIRECT_RCS_PPGTT_BYTES);
    if !direct_rcs_encode_glyph_mask_layers_2d_batch(state, upload, layers, dst) {
        return (false, false);
    }
    let submitted = direct_rcs_submit_batch(dev, state);
    let completion_timeout_ms = if direct_scanout {
        UI4_COMPUTE_PRODUCER_RETIRE_TIMEOUT_MS
    } else {
        RESOLVE_TILE64_MSAA4_COMPLETION_TIMEOUT_MS
    };
    let observed = if submitted {
        direct_rcs_poll_result_slot_timeout_ms(
            state,
            COPY_RECT_POST_MARKER_SLOT,
            COPY_RECT_POST_MARKER,
            completion_timeout_ms,
        )
    } else {
        0
    };
    let completed = observed == COPY_RECT_POST_MARKER;
    if !completed {
        let occurrence = GLYPH_MASK_BATCH_INCOMPLETE_SEQ.fetch_add(1, Ordering::Relaxed) + 1;
        if occurrence <= 8 || occurrence.is_multiple_of(60) {
            crate::log_warn!(
                target: "intel-gpgpu";
                "glyph_mask_rgba8 batch incomplete occurrence={} layers={} submitted={} pre=0x{:08X} post=0x{:08X} timeout_ms={} action=fail-closed-and-rerender-scene\n",
                occurrence,
                layers.len(),
                submitted as u8,
                direct_rcs_read_result_slot(state, COPY_RECT_PRE_MARKER_SLOT),
                observed,
                completion_timeout_ms,
            );
        }
    }
    (submitted, completed)
}

fn submit_font_instance_layers_2d(
    layers: &[GpgpuFontInstanceLayer],
    descriptor_state: &GpgpuOwnedFontInstanceState,
    dst: GpgpuRgba8Surface,
    direct_scanout: bool,
    time_seconds: f32,
) -> (bool, bool) {
    if layers.is_empty()
        || layers.len() > FONT_INSTANCE_BATCH_MAX_LAYERS
        || !time_seconds.is_finite()
    {
        return (false, false);
    }
    // Retained font restamps are opportunistic producers. Contention is an
    // admission failure, not permission to monopolize a cooperative worker.
    let Some(_guard) = DIRECT_RCS_SUBMIT_LOCK.try_lock() else {
        return (false, false);
    };
    let Some(dev) = super::claimed_device() else {
        return (false, false);
    };
    let Some(upload) = upload_font_instance_rgba8_kernel() else {
        return (false, false);
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        return (false, false);
    };
    if !direct_rcs_forcewake(dev)
        || !direct_rcs_map_state(dev, state)
        || !direct_rcs_init_ppgtt(state)
        || !direct_rcs_map_ppgtt_region(
            state,
            upload.gpu,
            upload.phys,
            upload.mapped_bytes,
            direct_rcs_ppgtt_pte_flags(),
        )
        || !direct_rcs_map_ppgtt_destination(state, dst.gpu, dst.phys, dst.bytes, direct_scanout)
        || !direct_rcs_map_ppgtt_region(
            state,
            descriptor_state.gpu(),
            descriptor_state.phys(),
            descriptor_state.bytes(),
            direct_rcs_ppgtt_pte_flags(),
        )
    {
        return (false, false);
    }
    for &layer in layers {
        let Some(dispatch) = lower_font_instance_layer(layer, descriptor_state, dst) else {
            return (false, false);
        };
        if dispatch.is_empty() {
            continue;
        }
        if !direct_rcs_map_ppgtt_region(
            state,
            layer.mask.gpu,
            layer.mask.phys,
            layer.mask.bytes,
            direct_rcs_ppgtt_pte_flags(),
        ) {
            return (false, false);
        }
    }
    // Publish the complete retained scene address space once before encoding
    // its ordered walkers.
    super::dma_flush(state.ppgtt_virt, DIRECT_RCS_PPGTT_BYTES);
    if !direct_rcs_encode_font_instance_layers_2d_batch(
        state,
        upload,
        layers,
        descriptor_state,
        dst,
        time_seconds,
    ) {
        return (false, false);
    }
    let submitted = direct_rcs_submit_batch(dev, state);
    let completion_timeout_ms = if direct_scanout {
        UI4_COMPUTE_PRODUCER_RETIRE_TIMEOUT_MS
    } else {
        RESOLVE_TILE64_MSAA4_COMPLETION_TIMEOUT_MS
    };
    let observed = if submitted {
        direct_rcs_poll_result_slot_timeout_ms(
            state,
            COPY_RECT_POST_MARKER_SLOT,
            COPY_RECT_POST_MARKER,
            completion_timeout_ms,
        )
    } else {
        0
    };
    let completed = observed == COPY_RECT_POST_MARKER;
    if !completed {
        let occurrence = FONT_INSTANCE_BATCH_INCOMPLETE_SEQ.fetch_add(1, Ordering::Relaxed) + 1;
        if occurrence <= 8 || occurrence.is_multiple_of(60) {
            crate::log_warn!(
                target: "intel-gpgpu";
                "font_instance_rgba8 batch incomplete occurrence={} layers={} submitted={} pre=0x{:08X} post=0x{:08X} timeout_ms={} action=fail-closed-and-rerender-scene\n",
                occurrence,
                layers.len(),
                submitted as u8,
                direct_rcs_read_result_slot(state, COPY_RECT_PRE_MARKER_SLOT),
                observed,
                completion_timeout_ms,
            );
        }
    }
    (submitted, completed)
}
