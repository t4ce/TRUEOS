pub(crate) fn submit_gpgpu_primary_scanout_walkrow16(
    row_one_based: u32,
    x_base: u32,
    color: u32,
    verify_readback: bool,
) -> crate::intel::GpgpuOneTileSentinelProof {
    submit_gpgpu_primary_scanout_stamp::<
        { trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_PIXELS_PER_PROGRAM },
    >(
        row_one_based,
        x_base,
        color,
        verify_readback,
        MANDELBROT16_T36_MODE_IMMEDIATE_UNROLLED_SCALAR16,
        "gpgpu-primary-scanout-walkrow16",
    )
}

pub(crate) fn submit_gpgpu_primary_scanout_chunkstamp704(
    row_one_based: u32,
    x_base: u32,
    color: u32,
    verify_readback: bool,
) -> crate::intel::GpgpuOneTileSentinelProof {
    submit_gpgpu_primary_scanout_stamp::<
        {
            trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_PIXELS_PER_PROGRAM
                * MANDELBROT16_T38_STAMP_REPEATS as usize
        },
    >(
        row_one_based,
        x_base,
        color,
        verify_readback,
        MANDELBROT16_T38_MODE_IMMEDIATE_WIDE_STAMP,
        "gpgpu-primary-scanout-chunkstamp704",
    )
}

pub(crate) fn submit_gpgpu_primary_scanout_chunkstamp704_unrolled(
    row_one_based: u32,
    x_base: u32,
    color: u32,
    verify_readback: bool,
) -> crate::intel::GpgpuOneTileSentinelProof {
    submit_gpgpu_primary_scanout_stamp::<
        {
            trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_PIXELS_PER_PROGRAM
                * MANDELBROT16_T38_STAMP_REPEATS as usize
        },
    >(
        row_one_based,
        x_base,
        color,
        verify_readback,
        MANDELBROT16_T39_MODE_IMMEDIATE_WIDE_STAMP_ADDRESS_COLOR,
        "gpgpu-primary-scanout-chunkstamp704-unrolled",
    )
}

pub(crate) fn submit_gpgpu_primary_scanout_fillrow_linear16(
    row_one_based: u32,
    color: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    const LANES: usize = trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_LANES;
    const STORE_BYTES_PER_PIXEL: usize = core::mem::size_of::<u32>();
    let program = gpgpu_primary_scanout_mandelbrot16_simd16_bw_program();
    let Some(dev) = crate::intel::claimed_device() else {
        return gpgpu_one_tile_sentinel_failure("no-device", program, 0);
    };
    let Some(warm) = warm_state() else {
        return gpgpu_one_tile_sentinel_failure("no-warm-state", program, 0);
    };
    let Some(target) = crate::intel::display::primary_surface_gpgpu_marker_target() else {
        return gpgpu_one_tile_sentinel_failure("no-primary-scanout", program, 0);
    };
    if row_one_based == 0 || row_one_based > 1440 || row_one_based > target.height {
        return gpgpu_one_tile_sentinel_failure("fillrow-row-out-of-range", program, target.gpu);
    }
    if target.width == 0 || target.width as usize % LANES != 0 {
        return gpgpu_one_tile_sentinel_failure("fillrow-width-not-simd16-multiple", program, target.gpu);
    }
    if !forcewake_render_acquire(warm) {
        return gpgpu_one_tile_sentinel_failure("forcewake", program, target.gpu);
    }
    if !ensure_gpgpu_warm_buffers_mapped(dev, warm) {
        return gpgpu_one_tile_sentinel_failure("ggtt-map", program, target.gpu);
    }

    let y = row_one_based.saturating_sub(1) as usize;
    let row_offset = y.saturating_mul(target.pitch_bytes as usize);
    let groups = (target.width as usize / LANES) as u32;
    let store_bytes = target.width as usize * STORE_BYTES_PER_PIXEL;
    if row_offset.saturating_add(store_bytes) > target.byte_len {
        return gpgpu_one_tile_sentinel_failure("fillrow-outside-scanout", program, target.gpu);
    }
    if row_offset >> 32 != 0 {
        return gpgpu_one_tile_sentinel_failure("fillrow-offset-high32", program, target.gpu);
    }
    let row_gpu = target.gpu + row_offset as u64;
    if row_gpu >> 32 != 0 {
        return gpgpu_one_tile_sentinel_failure("fillrow-gpu-high32", program, row_gpu);
    }
    let row_virt = unsafe { target.virt.add(row_offset) };
    let output_first_before = unsafe { core::ptr::read_volatile(row_virt as *const u32) };

    if !upload_primary_scanout_mandelbrot16_simd16_bw_artifact(
        warm,
        row_offset as u32,
        0,
        MANDELBROT16_T37_MODE_GROUPID_X_UNROLLED_SCALAR16,
        color,
        0,
        Mandelbrot16AddressMode::GroupIdLinear64,
    ) {
        return gpgpu_one_tile_sentinel_failure("fillrow-program-upload", program, row_gpu);
    }

    unsafe {
        core::ptr::write_volatile(
            warm.result_virt
                .add(RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD * core::mem::size_of::<u32>())
                as *mut u32,
            0,
        );
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
    }
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let batch_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, batch_dwords) };
    let store_surface = prepare_gpgpu_mandelbrot_store_surface_state_for_target_span(
        warm,
        target.gpu,
        target.byte_len,
        "gpgpu-primary-scanout-fillrow-linear16",
    );
    let batch_bytes =
        match encode_gfx12_gpgpu_walker_probe_batch(warm, batch, store_surface, program, groups) {
            Ok(bytes) => bytes,
            Err(reason) => return gpgpu_one_tile_sentinel_failure(reason, program, row_gpu),
        };
    crate::intel::dma_flush(warm.batch_virt, batch_bytes);

    let dispatch_before = read_gpgpu_threads_dispatched(dev);
    let finished = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD,
        "gpgpu-primary-scanout-fillrow-linear16",
    );
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let dispatch_after = read_gpgpu_threads_dispatched(dev);
    let dispatch_delta = dispatch_after.saturating_sub(dispatch_before);
    let finish_marker = read_result_dword(warm, RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD);
    let command_ok = finished && finish_marker == RCS_EXEC_RESULT_COMPUTE_WALKER_DONE;
    crate::intel::dma_flush(row_virt, store_bytes);
    let mut sample_hits = 0u64;
    let mut sample_values = [0u32; 8];
    let max_x = target.width.saturating_sub(1) as usize;
    let mut sample = 0usize;
    while sample < sample_values.len() {
        let x = match sample {
            0 => 0,
            1 => 1,
            2 => 15,
            3 => 16,
            4 => target.width as usize / 4,
            5 => target.width as usize / 2,
            6 => target.width.saturating_sub(16) as usize,
            _ => max_x,
        }
        .min(max_x);
        let value = unsafe {
            core::ptr::read_volatile(row_virt.add(x * STORE_BYTES_PER_PIXEL) as *const u32)
        };
        sample_values[sample] = value;
        if value & 0x00FF_FFFF == color & 0x00FF_FFFF {
            sample_hits |= 1u64 << sample;
        }
        sample += 1;
    }
    let output_first_after = sample_values[0];
    let memory_ok = sample_hits == 0xFF;
    let display_notified = command_ok
        && crate::intel::display::notify_primary_surface_external_write(
            "gpgpu-primary-scanout-fillrow-linear16",
            row_offset,
            store_bytes,
        );
    let reason = if command_ok && memory_ok {
        "fillrow-linear16-memory-visible"
    } else if command_ok && sample_hits != 0 {
        "fillrow-linear16-partial-memory-visible"
    } else if command_ok {
        "fillrow-linear16-submitted-no-readback"
    } else if !finished {
        "fillrow-linear16-submit-not-finished"
    } else {
        "fillrow-linear16-finish-marker-mismatch"
    };

    crate::log!(
        "intel/gpgpu: fillrow-linear16 row={} y={} width={} groups={} color=0x{:08X} row_offset=0x{:X} row_gpu=0x{:X} pitch_bytes={} display_notified={} finished={} memory_ok={} reason={} sample_hits=0x{:02X} first_before=0x{:08X} sample=[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X}] finish_marker=0x{:08X} dispatch_delta={} expected_dispatch={} program_source={} batch_bytes=0x{:X}\n",
        row_one_based,
        y,
        target.width,
        groups,
        color,
        row_offset,
        row_gpu,
        target.pitch_bytes,
        display_notified as u8,
        finished as u8,
        memory_ok as u8,
        reason,
        sample_hits,
        output_first_before,
        sample_values[0],
        sample_values[1],
        sample_values[2],
        sample_values[3],
        sample_values[4],
        sample_values[5],
        sample_values[6],
        sample_values[7],
        finish_marker,
        dispatch_delta,
        u64::from(groups) * LANES as u64,
        program.name,
        batch_bytes,
    );

    if !finished {
        recover_render_engine_after_nonretired_submit(
            dev,
            warm,
            "gpgpu-primary-scanout-fillrow-linear16",
        );
    }

    crate::intel::GpgpuOneTileSentinelProof {
        submitted: batch_bytes != 0,
        finished,
        readback_ok: command_ok && memory_ok,
        reason,
        program_name: program.name,
        output_gpu: row_gpu,
        sentinel: color,
        output_first_before,
        output_first_after,
        output_nonzero_before: (output_first_before != 0) as usize,
        output_nonzero_after: (output_first_after != 0) as usize,
        output_hits_lo64: sample_hits,
        dispatch_delta,
        finish_marker,
        expected_finish_marker: RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        batch_bytes,
    }
}

pub(crate) fn submit_gpgpu_primary_scanout_row2560_simd16(
    row_one_based: u32,
    color: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    submit_gpgpu_primary_scanout_row2560_simd16_variant(row_one_based, color, 0)
}

pub(crate) fn submit_gpgpu_primary_scanout_row2560_simd16_variant(
    row_one_based: u32,
    color: u32,
    variant: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    const PIXELS: usize = trueos_eu::gfx12::PRIMARY_SCANOUT_ROW2560_SIMD16_BW_PIXELS;
    const STORE_BYTES_PER_PIXEL: usize = core::mem::size_of::<u32>();
    let store_bytes = PIXELS * STORE_BYTES_PER_PIXEL;
    let program = gpgpu_primary_scanout_row2560_simd16_program();
    let Some(dev) = crate::intel::claimed_device() else {
        return gpgpu_one_tile_sentinel_failure("no-device", program, 0);
    };
    let Some(warm) = warm_state() else {
        return gpgpu_one_tile_sentinel_failure("no-warm-state", program, 0);
    };
    let Some(target) = crate::intel::display::primary_surface_gpgpu_marker_target() else {
        return gpgpu_one_tile_sentinel_failure("no-primary-scanout", program, 0);
    };
    if row_one_based == 0 || row_one_based > 1440 || row_one_based > target.height {
        return gpgpu_one_tile_sentinel_failure("row2560-simd16-row-out-of-range", program, target.gpu);
    }
    if target.width as usize != PIXELS {
        return gpgpu_one_tile_sentinel_failure("row2560-simd16-width-mismatch", program, target.gpu);
    }
    if !forcewake_render_acquire(warm) {
        return gpgpu_one_tile_sentinel_failure("forcewake", program, target.gpu);
    }
    if !ensure_gpgpu_warm_buffers_mapped(dev, warm) {
        return gpgpu_one_tile_sentinel_failure("ggtt-map", program, target.gpu);
    }

    let y = row_one_based.saturating_sub(1) as usize;
    let row_offset = y.saturating_mul(target.pitch_bytes as usize);
    if row_offset.saturating_add(store_bytes) > target.byte_len {
        return gpgpu_one_tile_sentinel_failure("row2560-simd16-outside-scanout", program, target.gpu);
    }
    if row_offset >> 32 != 0 {
        return gpgpu_one_tile_sentinel_failure("row2560-simd16-offset-high32", program, target.gpu);
    }
    let row_gpu = target.gpu + row_offset as u64;
    if row_gpu >> 32 != 0 {
        return gpgpu_one_tile_sentinel_failure("row2560-simd16-gpu-high32", program, row_gpu);
    }

    if !upload_primary_scanout_row2560_simd16_artifact(
        warm,
        row_offset as u32,
        color,
        variant,
    ) {
        return gpgpu_one_tile_sentinel_failure("row2560-simd16-program-upload", program, row_gpu);
    }

    unsafe {
        core::ptr::write_volatile(
            warm.result_virt
                .add(RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD * core::mem::size_of::<u32>())
                as *mut u32,
            0,
        );
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
    }
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let batch_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, batch_dwords) };
    let store_surface = prepare_gpgpu_mandelbrot_store_surface_state_for_target_span(
        warm,
        target.gpu,
        target.byte_len,
        "gpgpu-primary-scanout-row2560-simd16",
    );
    let batch_bytes =
        match encode_gfx12_gpgpu_walker_probe_batch(warm, batch, store_surface, program, 1) {
            Ok(bytes) => bytes,
            Err(reason) => return gpgpu_one_tile_sentinel_failure(reason, program, row_gpu),
        };
    crate::intel::dma_flush(warm.batch_virt, batch_bytes);

    let dispatch_before = read_gpgpu_threads_dispatched(dev);
    let finished = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD,
        "gpgpu-primary-scanout-row2560-simd16",
    );
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let dispatch_after = read_gpgpu_threads_dispatched(dev);
    let dispatch_delta = dispatch_after.saturating_sub(dispatch_before);
    let finish_marker = read_result_dword(warm, RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD);
    let command_ok = finished && finish_marker == RCS_EXEC_RESULT_COMPUTE_WALKER_DONE;
    let display_notified = command_ok
        && crate::intel::display::notify_primary_surface_external_write(
            "gpgpu-primary-scanout-row2560-simd16",
            row_offset,
            store_bytes,
        );
    let reason = if command_ok {
        "row2560-simd16-submitted-no-readback"
    } else if !finished {
        "row2560-simd16-submit-not-finished"
    } else {
        "row2560-simd16-finish-marker-mismatch"
    };

    crate::log!(
        "intel/gpgpu: row2560-simd16 row={} y={} width={} row_offset=0x{:X} row_gpu=0x{:X} target_gpu=0x{:X} pitch_bytes={} color=0x{:08X} variant={} store_bytes={} display_notified={} finished={} reason={} finish_marker=0x{:08X} dispatch_delta={} expected_dispatch=16 program_source={} batch_bytes=0x{:X}\n",
        row_one_based,
        y,
        target.width,
        row_offset,
        row_gpu,
        target.gpu,
        target.pitch_bytes,
        color,
        variant % 5,
        store_bytes,
        display_notified as u8,
        finished as u8,
        reason,
        finish_marker,
        dispatch_delta,
        program.name,
        batch_bytes,
    );

    if !finished {
        recover_render_engine_after_nonretired_submit(
            dev,
            warm,
            "gpgpu-primary-scanout-row2560-simd16",
        );
    }

    crate::intel::GpgpuOneTileSentinelProof {
        submitted: batch_bytes != 0,
        finished,
        readback_ok: command_ok,
        reason,
        program_name: program.name,
        output_gpu: row_gpu,
        sentinel: color,
        output_first_before: 0,
        output_first_after: 0,
        output_nonzero_before: 0,
        output_nonzero_after: 0,
        output_hits_lo64: 0,
        dispatch_delta,
        finish_marker,
        expected_finish_marker: RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        batch_bytes,
    }
}

pub(crate) fn submit_gpgpu_primary_scanout_rowburst1280(
    row_one_based: u32,
    rows_requested: u32,
    x_base: u32,
    color: u32,
    allow_no_eot: bool,
) -> crate::intel::GpgpuOneTileSentinelProof {
    const PIXELS: usize = trueos_eu::gfx12::PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_LANES;
    const STORE_BYTES_PER_PIXEL: usize = core::mem::size_of::<u32>();
    let store_bytes = PIXELS * STORE_BYTES_PER_PIXEL;
    let program = gpgpu_primary_scanout_groupid_line1280_rows_program();
    let Some(dev) = crate::intel::claimed_device() else {
        return gpgpu_one_tile_sentinel_failure("no-device", program, 0);
    };
    let Some(warm) = warm_state() else {
        return gpgpu_one_tile_sentinel_failure("no-warm-state", program, 0);
    };
    let Some(target) = crate::intel::display::primary_surface_gpgpu_marker_target() else {
        return gpgpu_one_tile_sentinel_failure("no-primary-scanout", program, 0);
    };
    if row_one_based == 0 || row_one_based > 1440 || row_one_based > target.height {
        return gpgpu_one_tile_sentinel_failure("rowburst-row-out-of-range", program, target.gpu);
    }
    if !forcewake_render_acquire(warm) {
        return gpgpu_one_tile_sentinel_failure("forcewake", program, target.gpu);
    }
    if !ensure_gpgpu_warm_buffers_mapped(dev, warm) {
        return gpgpu_one_tile_sentinel_failure("ggtt-map", program, target.gpu);
    }

    let rows = core::cmp::min(
        rows_requested.max(1),
        target.height
            .saturating_sub(row_one_based)
            .saturating_add(1),
    );
    let x = x_base as usize;
    if x.saturating_add(PIXELS) > target.width as usize {
        return gpgpu_one_tile_sentinel_failure("rowburst-width-out-of-range", program, target.gpu);
    }
    let y = row_one_based.saturating_sub(1) as usize;
    let row_offset = y
        .saturating_mul(target.pitch_bytes as usize)
        .saturating_add(x.saturating_mul(STORE_BYTES_PER_PIXEL));
    let span_bytes = (rows as usize)
        .saturating_sub(1)
        .saturating_mul(target.pitch_bytes as usize)
        .saturating_add(store_bytes);
    if row_offset.saturating_add(span_bytes) > target.byte_len {
        return gpgpu_one_tile_sentinel_failure("rowburst-outside-scanout", program, target.gpu);
    }
    if row_offset >> 32 != 0 {
        return gpgpu_one_tile_sentinel_failure("rowburst-offset-high32", program, target.gpu);
    }
    let row_gpu = target.gpu + row_offset as u64;
    if row_gpu >> 32 != 0 {
        return gpgpu_one_tile_sentinel_failure("rowburst-gpu-high32", program, row_gpu);
    }

    if !upload_primary_scanout_groupid_line1280_rows_artifact(warm, row_offset as u32, color) {
        return gpgpu_one_tile_sentinel_failure("rowburst-program-upload", program, row_gpu);
    }

    unsafe {
        core::ptr::write_volatile(
            warm.result_virt
                .add(RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD * core::mem::size_of::<u32>())
                as *mut u32,
            0,
        );
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
    }
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let batch_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, batch_dwords) };
    let store_surface = prepare_gpgpu_mandelbrot_store_surface_state_for_target_span(
        warm,
        target.gpu,
        target.byte_len,
        "gpgpu-primary-scanout-rowburst1280",
    );
    let batch_bytes =
        match encode_gfx12_gpgpu_walker_probe_batch(warm, batch, store_surface, program, rows) {
            Ok(bytes) => bytes,
            Err(reason) => return gpgpu_one_tile_sentinel_failure(reason, program, row_gpu),
        };
    crate::intel::dma_flush(warm.batch_virt, batch_bytes);

    let dispatch_before = read_gpgpu_threads_dispatched(dev);
    let finished = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD,
        "gpgpu-primary-scanout-rowburst1280",
    );
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let dispatch_after = read_gpgpu_threads_dispatched(dev);
    let dispatch_delta = dispatch_after.saturating_sub(dispatch_before);
    let finish_marker = read_result_dword(warm, RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD);
    let expected_dispatch = u64::from(rows.saturating_mul(8));
    let command_ok = finished && finish_marker == RCS_EXEC_RESULT_COMPUTE_WALKER_DONE;
    let loose_dispatch_ok = allow_no_eot && batch_bytes != 0 && dispatch_delta >= expected_dispatch;
    let accepted_ok = command_ok || loose_dispatch_ok;
    let display_notified = accepted_ok
        && crate::intel::display::notify_primary_surface_external_write(
            "gpgpu-primary-scanout-rowburst1280",
            row_offset,
            span_bytes,
        );
    let reason = if command_ok {
        "rowburst1280-submitted-no-readback"
    } else if loose_dispatch_ok {
        "rowburst1280-dispatch-observed-no-eot"
    } else if !finished {
        "rowburst1280-submit-not-finished"
    } else {
        "rowburst1280-finish-marker-mismatch"
    };

    crate::log!(
        "intel/gpgpu: rowburst1280 start_row={} rows={} x={} pixels_per_row={} total_pixels={} color=0x{:08X} allow_no_eot={} submitted={} finished={} readback_ok={} reason={} row_offset=0x{:X} output_gpu=0x{:X} target_gpu=0x{:X} pitch_bytes={} display_notified={} finish_marker=0x{:08X} dispatch_delta={} expected_dispatch={} program_source={} batch_bytes=0x{:X}\n",
        row_one_based,
        rows,
        x_base,
        PIXELS,
        rows.saturating_mul(PIXELS as u32),
        color,
        allow_no_eot as u8,
        (batch_bytes != 0) as u8,
        finished as u8,
        accepted_ok as u8,
        reason,
        row_offset,
        row_gpu,
        target.gpu,
        target.pitch_bytes,
        display_notified as u8,
        finish_marker,
        dispatch_delta,
        expected_dispatch,
        program.name,
        batch_bytes,
    );

    if !finished && !allow_no_eot {
        recover_render_engine_after_nonretired_submit(
            dev,
            warm,
            "gpgpu-primary-scanout-rowburst1280",
        );
    }

    crate::intel::GpgpuOneTileSentinelProof {
        submitted: batch_bytes != 0,
        finished,
        readback_ok: accepted_ok,
        reason,
        program_name: program.name,
        output_gpu: row_gpu,
        sentinel: color,
        output_first_before: 0,
        output_first_after: 0,
        output_nonzero_before: 0,
        output_nonzero_after: 0,
        output_hits_lo64: 0,
        dispatch_delta,
        finish_marker,
        expected_finish_marker: RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        batch_bytes,
    }
}

fn submit_gpgpu_primary_scanout_stamp<const PIXELS: usize>(
    row_one_based: u32,
    x_base: u32,
    color: u32,
    verify_readback: bool,
    mode: u32,
    submit_name: &'static str,
) -> crate::intel::GpgpuOneTileSentinelProof {
    const STORE_BYTES_PER_PIXEL: usize = core::mem::size_of::<u32>();
    let store_bytes = PIXELS * STORE_BYTES_PER_PIXEL;
    let program = gpgpu_primary_scanout_mandelbrot16_simd16_bw_program();
    let Some(dev) = crate::intel::claimed_device() else {
        return gpgpu_one_tile_sentinel_failure("no-device", program, 0);
    };
    let Some(warm) = warm_state() else {
        return gpgpu_one_tile_sentinel_failure("no-warm-state", program, 0);
    };
    let Some(target) = crate::intel::display::primary_surface_gpgpu_marker_target() else {
        return gpgpu_one_tile_sentinel_failure("no-primary-scanout", program, 0);
    };
    if row_one_based == 0 || row_one_based > 1440 || row_one_based > target.height {
        return gpgpu_one_tile_sentinel_failure("walkrow-row-out-of-range", program, target.gpu);
    }
    if !forcewake_render_acquire(warm) {
        return gpgpu_one_tile_sentinel_failure("forcewake", program, target.gpu);
    }
    if !ensure_gpgpu_warm_buffers_mapped(dev, warm) {
        return gpgpu_one_tile_sentinel_failure("ggtt-map", program, target.gpu);
    }

    let y = row_one_based.saturating_sub(1) as usize;
    let x = core::cmp::min(x_base as usize, (target.width as usize).saturating_sub(PIXELS));
    let row_offset = y
        .saturating_mul(target.pitch_bytes as usize)
        .saturating_add(x.saturating_mul(STORE_BYTES_PER_PIXEL));
    if row_offset.saturating_add(store_bytes) > target.byte_len {
        return gpgpu_one_tile_sentinel_failure(
            "simd16-store-outside-scanout",
            program,
            target.gpu,
        );
    }
    if row_offset >> 32 != 0 {
        return gpgpu_one_tile_sentinel_failure("simd16-store-offset-high32", program, target.gpu);
    }
    let row_gpu = target.gpu + row_offset as u64;
    if row_gpu >> 32 != 0 {
        return gpgpu_one_tile_sentinel_failure("simd16-store-gpu-high32", program, row_gpu);
    }
    let row_virt = unsafe { target.virt.add(row_offset) };

    let expected = color;
    let sample_lanes = core::cmp::min(PIXELS, 64);
    let expected_hit_mask = if sample_lanes >= 64 {
        u64::MAX
    } else {
        (1u64 << sample_lanes) - 1
    };
    let poison = expected ^ 0x00A5_A5A5;
    let mut lane = 0usize;
    if verify_readback {
        while lane < PIXELS {
            unsafe {
                core::ptr::write_volatile(
                    row_virt.add(lane * STORE_BYTES_PER_PIXEL) as *mut u32,
                    poison,
                );
            }
            lane += 1;
        }
        crate::intel::dma_flush(row_virt, store_bytes);
    }

    let mut before_words = [0u32; 64];
    if verify_readback {
        lane = 0;
        while lane < sample_lanes {
            before_words[lane] = unsafe {
                core::ptr::read_volatile(row_virt.add(lane * STORE_BYTES_PER_PIXEL) as *const u32)
            };
            lane += 1;
        }
    }
    let output_first_before = before_words[0];

    if !upload_primary_scanout_mandelbrot16_simd16_bw_artifact(
        warm,
        row_offset as u32,
        0,
        mode,
        color,
        0,
        Mandelbrot16AddressMode::ImmediateBase,
    ) {
        return gpgpu_one_tile_sentinel_failure("simd16-store-program-upload", program, row_gpu);
    }

    unsafe {
        core::ptr::write_volatile(
            warm.result_virt
                .add(RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD * core::mem::size_of::<u32>())
                as *mut u32,
            0,
        );
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
    }
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let batch_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, batch_dwords) };
    let store_surface = prepare_gpgpu_mandelbrot_store_surface_state_for_target_span(
        warm,
        target.gpu,
        target.byte_len,
        submit_name,
    );
    let batch_bytes =
        match encode_gfx12_gpgpu_walker_probe_batch(warm, batch, store_surface, program, 1) {
            Ok(bytes) => bytes,
            Err(reason) => return gpgpu_one_tile_sentinel_failure(reason, program, row_gpu),
        };
    crate::intel::dma_flush(warm.batch_virt, batch_bytes);

    let dispatch_before = read_gpgpu_threads_dispatched(dev);
    let finished = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD,
        submit_name,
    );
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let mut poll = 0usize;
    let mut hits = 0u64;
    let mut changed = 0u64;
    let mut after_words = [0u32; 64];
    let mut output_first_after = output_first_before;
    if verify_readback {
        while poll < MANDELBROT_STRIP_READBACK_POLLS {
            crate::intel::dma_flush(row_virt, store_bytes);
            hits = 0;
            changed = 0;
            lane = 0;
            while lane < sample_lanes {
                let after = unsafe {
                    core::ptr::read_volatile(
                        row_virt.add(lane * STORE_BYTES_PER_PIXEL) as *const u32
                    )
                };
                after_words[lane] = after;
                if lane == 0 {
                    output_first_after = after;
                }
                if after == expected {
                    if lane < 64 {
                        hits |= 1u64 << lane;
                    }
                }
                if after != before_words[lane] {
                    changed |= 1u64 << lane;
                }
                lane += 1;
            }
            if hits & expected_hit_mask == expected_hit_mask {
                break;
            }
            poll += 1;
            core::hint::spin_loop();
        }
    }

    let dispatch_after = read_gpgpu_threads_dispatched(dev);
    let dispatch_delta = dispatch_after.saturating_sub(dispatch_before);
    let finish_marker = read_result_dword(warm, RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD);
    let expected_dispatch = trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_LANES as u64;
    let command_ok = finished && finish_marker == RCS_EXEC_RESULT_COMPUTE_WALKER_DONE;
    let readback_ok = if verify_readback {
        command_ok && changed != 0 && hits & expected_hit_mask == expected_hit_mask
    } else {
        command_ok
    };
    let display_notified = command_ok
        && crate::intel::display::notify_primary_surface_external_write(
            submit_name,
            row_offset,
            store_bytes,
        );
    let reason = if !verify_readback && command_ok {
        "simd16-store-submitted-no-readback"
    } else if readback_ok {
        "simd16-immediate-constant-store-visible"
    } else if !finished {
        "simd16-store-submit-not-finished"
    } else if finish_marker != RCS_EXEC_RESULT_COMPUTE_WALKER_DONE {
        "simd16-store-finish-marker-mismatch"
    } else if hits == 0 {
        "simd16-store-no-lane-matched-expected-color"
    } else {
        "simd16-store-partial-lane-match"
    };

    crate::log!(
        "intel/gpgpu: walkrow16 row={} y={} x={} row_offset=0x{:X} row_gpu=0x{:X} target_gpu=0x{:X} target_phys=0x{:X} pitch_bytes={} color=0x{:08X} pixels={} verify_readback={} expected_hit_mask=0x{:04X} hit_mask=0x{:04X} changed_mask=0x{:04X} readback_ok={} reason={} first_before=0x{:08X} first_after=0x{:08X} after0=0x{:08X} after1=0x{:08X} after2=0x{:08X} after3=0x{:08X} after4=0x{:08X} after5=0x{:08X} after6=0x{:08X} after7=0x{:08X} after8=0x{:08X} after9=0x{:08X} after10=0x{:08X} after11=0x{:08X} after12=0x{:08X} after13=0x{:08X} after14=0x{:08X} after15=0x{:08X} display_notified={} finish_marker=0x{:08X} lane_dispatch_delta={} expected_dispatch={} program_source={} batch_bytes=0x{:X}\n",
        row_one_based,
        y,
        x,
        row_offset,
        row_gpu,
        target.gpu,
        target.phys,
        target.pitch_bytes,
        color,
        PIXELS,
        verify_readback as u8,
        expected_hit_mask,
        hits,
        changed,
        readback_ok as u8,
        reason,
        output_first_before,
        output_first_after,
        after_words[0],
        after_words[1],
        after_words[2],
        after_words[3],
        after_words[4],
        after_words[5],
        after_words[6],
        after_words[7],
        after_words[8],
        after_words[9],
        after_words[10],
        after_words[11],
        after_words[12],
        after_words[13],
        after_words[14],
        after_words[15],
        display_notified as u8,
        finish_marker,
        dispatch_delta,
        expected_dispatch,
        submit_name,
        batch_bytes,
    );

    if !finished {
        recover_render_engine_after_nonretired_submit(dev, warm, submit_name);
    }
    crate::intel::GpgpuOneTileSentinelProof {
        submitted: batch_bytes != 0,
        finished,
        readback_ok,
        reason,
        program_name: program.name,
        output_gpu: row_gpu,
        sentinel: expected,
        output_first_before,
        output_first_after,
        output_nonzero_before: (output_first_before != 0) as usize,
        output_nonzero_after: (output_first_after != 0) as usize,
        output_hits_lo64: hits,
        dispatch_delta,
        finish_marker,
        expected_finish_marker: RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        batch_bytes,
    }
}
