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


