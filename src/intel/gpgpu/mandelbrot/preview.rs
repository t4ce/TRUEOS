pub(crate) fn submit_gpgpu_primary_scanout_line_pilot(
    mode: u32,
    line_index: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    submit_gpgpu_primary_scanout_line_pilot_rect(mode, line_index, 0, 0, u32::MAX, u32::MAX)
}

pub(crate) fn submit_gpgpu_primary_scanout_line_pilot_rect(
    mode: u32,
    line_index: u32,
    rect_x: u32,
    rect_y: u32,
    rect_width: u32,
    rect_height: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    let color_seed = if mode & 1 == 0 {
        0x0000_0000
    } else {
        0x00FF_FFFF
    };
    submit_gpgpu_primary_scanout_line_pilot_rect_color(
        color_seed,
        line_index,
        rect_x,
        rect_y,
        rect_width,
        rect_height,
    )
}

pub(crate) fn submit_gpgpu_primary_scanout_line_pilot_rect_color(
    color_seed: u32,
    line_index: u32,
    rect_x: u32,
    rect_y: u32,
    rect_width: u32,
    rect_height: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    const LANES_PER_PILOT: usize = trueos_eu::gfx12::PRIMARY_SCANOUT_LINE1280_LANE8ROWS_LANES;

    let program = gpgpu_primary_scanout_mandelbrot8_gpu_color_program();
    let Some(dev) = crate::intel::claimed_device() else {
        return gpgpu_one_tile_sentinel_failure("no-device", program, 0);
    };
    let Some(warm) = warm_state() else {
        return gpgpu_one_tile_sentinel_failure("no-warm-state", program, 0);
    };
    let Some(target) = crate::intel::display::primary_surface_gpgpu_marker_target() else {
        return gpgpu_one_tile_sentinel_failure("no-primary-scanout", program, 0);
    };
    if !forcewake_render_acquire(warm) {
        return gpgpu_one_tile_sentinel_failure("forcewake", program, target.gpu);
    }
    if !ensure_gpgpu_warm_buffers_mapped(dev, warm) {
        return gpgpu_one_tile_sentinel_failure("ggtt-map", program, target.gpu);
    }

    let target_width = target.width as usize;
    let target_height = target.height as usize;
    let rect_width = core::cmp::min(rect_width as usize, target_width);
    let rect_height = core::cmp::min(rect_height as usize, target_height);
    if rect_width < LANES_PER_PILOT || rect_height == 0 {
        return gpgpu_one_tile_sentinel_failure("line-pilot-rect-too-small", program, target.gpu);
    }
    let rect_x = core::cmp::min(rect_x as usize, target_width.saturating_sub(rect_width));
    let rect_y = core::cmp::min(rect_y as usize, target_height.saturating_sub(rect_height));
    let segments_per_row = rect_width.saturating_add(LANES_PER_PILOT - 1) / LANES_PER_PILOT;
    let segments_per_row = core::cmp::max(1, segments_per_row);
    let serial_index = line_index as usize;
    let segment = serial_index % segments_per_row;
    let y_in_rect = if rect_height == 0 {
        0
    } else {
        (serial_index / segments_per_row) % rect_height
    };
    let y = rect_y.saturating_add(y_in_rect);
    let x_in_rect = if segments_per_row <= 1 {
        0
    } else {
        core::cmp::min(
            segment.saturating_mul(LANES_PER_PILOT),
            rect_width.saturating_sub(LANES_PER_PILOT),
        )
    } as usize;
    let x_base = rect_x.saturating_add(x_in_rect);
    let row_offset = y
        .saturating_mul(target.pitch_bytes as usize)
        .saturating_add(x_base.saturating_mul(core::mem::size_of::<u32>()));
    let pilot_bytes = LANES_PER_PILOT.saturating_mul(core::mem::size_of::<u32>());
    if row_offset.saturating_add(pilot_bytes) > target.byte_len {
        return gpgpu_one_tile_sentinel_failure("line-pilot-outside-scanout", program, target.gpu);
    }
    let row_gpu = target.gpu + row_offset as u64;
    let row_virt = unsafe { target.virt.add(row_offset) };
    let pilot_groups = 1u32;
    let requested_mode = (color_seed != 0) as u32;

    submit_gpgpu_primary_scanout_mandelbrot_gpu_color_witness_strip(
        dev,
        warm,
        target.gpu,
        target.byte_len,
        row_gpu,
        row_virt,
        x_base,
        y,
        0,
        requested_mode,
        color_seed,
        pilot_groups,
        pilot_bytes,
    )
}

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot_preview(
    cursor: usize,
    target_phase: usize,
    pixel_budget: usize,
) -> (crate::intel::GpgpuOneTileSentinelProof, usize) {
    const STRIP_BURST_MAX: usize = 256;
    const STORES_PER_PROGRAM: usize =
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_STRIPS_PER_PROGRAM;
    const SIMD_LANES_PER_STORE: usize =
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_LANES;
    const PIXELS_PER_PROGRAM: usize =
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_PIXELS_PER_PROGRAM;

    let program = gpgpu_primary_scanout_mandelbrot8_program();
    let Some(dev) = crate::intel::claimed_device() else {
        return (gpgpu_one_tile_sentinel_failure("no-device", program, 0), cursor);
    };
    let Some(warm) = warm_state() else {
        return (gpgpu_one_tile_sentinel_failure("no-warm-state", program, 0), cursor);
    };
    let Some(target) = crate::intel::display::primary_surface_gpgpu_marker_target() else {
        return (gpgpu_one_tile_sentinel_failure("no-primary-scanout", program, 0), cursor);
    };
    if !forcewake_render_acquire(warm) {
        return (gpgpu_one_tile_sentinel_failure("forcewake", program, target.gpu), cursor);
    }
    if !ensure_gpgpu_warm_buffers_mapped(dev, warm) {
        return (gpgpu_one_tile_sentinel_failure("ggtt-map", program, target.gpu), cursor);
    }

    let scanout_w = target.width as usize;
    let scanout_h = target.height as usize;
    let strips_per_row = scanout_w / PIXELS_PER_PROGRAM;
    let total_strips = strips_per_row.saturating_mul(scanout_h);
    if total_strips == 0 || pixel_budget < PIXELS_PER_PROGRAM {
        return (
            gpgpu_one_tile_sentinel_failure("empty-preview-scanout", program, target.gpu),
            cursor,
        );
    }

    let start_cursor = cursor % total_strips;
    let mut last_proof =
        gpgpu_one_tile_sentinel_failure_quiet("no-preview-strips-submitted", program, target.gpu);
    let strip_budget =
        core::cmp::min(core::cmp::max(1, pixel_budget / PIXELS_PER_PROGRAM), STRIP_BURST_MAX);
    let mut submitted_strips = 0usize;
    let mut finished_strips = 0usize;
    let mut accepted_strips = 0usize;
    let mut advanced_strips = 0usize;
    let mut idx = start_cursor;
    while submitted_strips < strip_budget {
        let strip_x = idx % strips_per_row;
        let py = idx / strips_per_row;
        let px = strip_x * PIXELS_PER_PROGRAM;
        let byte_offset = py
            .saturating_mul(target.pitch_bytes as usize)
            .saturating_add(px.saturating_mul(core::mem::size_of::<u32>()));
        if byte_offset.saturating_add(PIXELS_PER_PROGRAM * core::mem::size_of::<u32>())
            > target.byte_len
        {
            last_proof = gpgpu_one_tile_sentinel_failure(
                "preview-strip-outside-scanout",
                program,
                target.gpu,
            );
            break;
        }
        let row_gpu = target.gpu + byte_offset as u64;
        let row_virt = unsafe { target.virt.add(byte_offset) };
        let proof = submit_gpgpu_primary_scanout_mandelbrot_strip(
            dev,
            warm,
            program,
            target.gpu,
            target.byte_len,
            row_gpu,
            row_virt,
            px,
            py,
            scanout_w,
            scanout_h,
            target_phase,
        );
        submitted_strips += proof.submitted as usize;
        let expected_mask = (1u64 << PIXELS_PER_PROGRAM) - 1;
        let strip_changed = proof.finished
            && proof.finish_marker == RCS_EXEC_RESULT_COMPUTE_WALKER_DONE
            && proof.output_hits_lo64 == expected_mask;
        let strip_finished =
            proof.finished && proof.finish_marker == RCS_EXEC_RESULT_COMPUTE_WALKER_DONE;
        if strip_finished {
            finished_strips += 1;
        }
        if strip_changed {
            accepted_strips += 1;
            advanced_strips += 1;
        } else {
            last_proof = proof;
            break;
        }
        last_proof = proof;
        idx += 1;
        if idx == total_strips {
            idx = 0;
        }
    }

    let flush_offset = 0usize;
    let flush_bytes = scanout_h
        .saturating_sub(1)
        .saturating_mul(target.pitch_bytes as usize)
        .saturating_add(scanout_w.saturating_mul(core::mem::size_of::<u32>()));
    let display_notified = accepted_strips != 0
        && crate::intel::display::notify_primary_surface_external_write(
            "gpgpu-primary-scanout-visual-preview",
            flush_offset,
            flush_bytes,
        );
    let next_cursor = (start_cursor + advanced_strips) % total_strips;
    let readback_ok =
        submitted_strips != 0 && submitted_strips == accepted_strips && last_proof.readback_ok;
    let first_failed_preview_log =
        !readback_ok && !MANDELBROT_PREVIEW_FAILURE_LOGGED.swap(true, Ordering::AcqRel);
    let should_log_preview = (accepted_strips != 0 && (start_cursor == 0 || next_cursor == 0))
        || first_failed_preview_log;
    if should_log_preview {
        crate::log!(
            "intel/gpgpu: primary-scanout-mandelbrot32-q12-preview scanout={}x{} submitted_programs={} finished_programs={} exact_programs={} advanced_programs={} hdc_sends_per_program={} simd_lanes_per_send={} pixels_per_program={} submitted_pixels={} exact_pixels={} strict_readback_ok={} reason={} program_source={} primary_gpu=0x{:X} primary_bytes=0x{:X} cursor_in={} cursor_out={} strip_budget={} burst_cap={} last_gpu=0x{:X} last_first_before=0x{:08X} last_first_after=0x{:08X} last_expected_mask=0x{:016X} display_notified={} finish_marker=0x{:08X} finish_expected=0x{:08X} lane_dispatch_delta={} scheduler=linear-scanout-32px-chunks cpu_runtime_patches=coords-and-address-bases eu_runtime_work=q12-iteration-color-and-hdc-message-payload action={} next={} deliverable=visible-q12-mandelbrot-pixels\n",
            scanout_w,
            scanout_h,
            submitted_strips,
            finished_strips,
            accepted_strips,
            advanced_strips,
            STORES_PER_PROGRAM,
            SIMD_LANES_PER_STORE,
            PIXELS_PER_PROGRAM,
            submitted_strips.saturating_mul(PIXELS_PER_PROGRAM),
            accepted_strips.saturating_mul(PIXELS_PER_PROGRAM),
            readback_ok as u8,
            last_proof.reason,
            program.name,
            target.gpu,
            target.byte_len,
            start_cursor,
            next_cursor,
            strip_budget,
            STRIP_BURST_MAX,
            last_proof.output_gpu,
            last_proof.output_first_before,
            last_proof.output_first_after,
            last_proof.output_hits_lo64,
            display_notified as u8,
            last_proof.finish_marker,
            last_proof.expected_finish_marker,
            last_proof.dispatch_delta,
            if readback_ok {
                "continue-gpgpu-visual-preview"
            } else {
                "hold-cursor-until-scanout-changes"
            },
            if next_cursor == 0 {
                "frame-covered"
            } else {
                "continue-visual-strips"
            },
        );
    }
    (last_proof, next_cursor)
}
