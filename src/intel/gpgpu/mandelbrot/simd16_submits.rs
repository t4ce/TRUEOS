pub(crate) fn submit_gpgpu_primary_scanout_walkrow16(
    row_one_based: u32,
    x_base: u32,
    color: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    const PIXELS: usize =
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_PIXELS_PER_PROGRAM;
    const STORE_BYTES: usize = PIXELS * core::mem::size_of::<u32>();
    const MODE: u32 = MANDELBROT16_T17_MODE_IMMEDIATE_CONSTANT_STORE;

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
        .saturating_add(x.saturating_mul(core::mem::size_of::<u32>()));
    if row_offset.saturating_add(STORE_BYTES) > target.byte_len {
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
    let expected_hit_mask = mandelbrot16_active_lane_mask() as u64;
    let poison = expected ^ 0x00A5_A5A5;
    let mut lane = 0usize;
    while lane < PIXELS {
        unsafe {
            core::ptr::write_volatile(
                row_virt.add(lane * core::mem::size_of::<u32>()) as *mut u32,
                poison,
            );
        }
        lane += 1;
    }
    crate::intel::dma_flush(row_virt, STORE_BYTES);

    let mut before_words = [0u32; PIXELS];
    lane = 0;
    while lane < PIXELS {
        before_words[lane] = unsafe {
            core::ptr::read_volatile(row_virt.add(lane * core::mem::size_of::<u32>()) as *const u32)
        };
        lane += 1;
    }
    let output_first_before = before_words[0];

    if !upload_primary_scanout_mandelbrot16_simd16_bw_artifact(
        warm,
        row_offset as u32,
        0,
        MODE,
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
        "walkrow16-primary-scanout",
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
        "gpgpu-primary-scanout-walkrow16",
    );
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let mut poll = 0usize;
    let mut hits = 0u64;
    let mut changed = 0u64;
    let mut after_words = [0u32; PIXELS];
    let mut output_first_after = output_first_before;
    while poll < MANDELBROT_STRIP_READBACK_POLLS {
        crate::intel::dma_flush(row_virt, STORE_BYTES);
        hits = 0;
        changed = 0;
        lane = 0;
        while lane < PIXELS {
            let after = unsafe {
                core::ptr::read_volatile(
                    row_virt.add(lane * core::mem::size_of::<u32>()) as *const u32
                )
            };
            after_words[lane] = after;
            if lane == 0 {
                output_first_after = after;
            }
            if after == expected {
                hits |= 1u64 << lane;
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

    let dispatch_after = read_gpgpu_threads_dispatched(dev);
    let dispatch_delta = dispatch_after.saturating_sub(dispatch_before);
    let finish_marker = read_result_dword(warm, RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD);
    let expected_dispatch = trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_LANES as u64;
    let command_ok = finished
        && finish_marker == RCS_EXEC_RESULT_COMPUTE_WALKER_DONE
        && dispatch_delta >= expected_dispatch;
    let readback_ok = command_ok && changed != 0 && hits & expected_hit_mask == expected_hit_mask;
    let display_notified = command_ok
        && crate::intel::display::notify_primary_surface_external_write(
            "gpgpu-primary-scanout-walkrow16",
            row_offset,
            STORE_BYTES,
        );
    let reason = if readback_ok {
        "simd16-immediate-constant-store-visible"
    } else if !finished {
        "simd16-store-submit-not-finished"
    } else if dispatch_delta == 0 {
        "simd16-store-no-eu-dispatch"
    } else if hits == 0 {
        "simd16-store-no-lane-matched-expected-color"
    } else {
        "simd16-store-partial-lane-match"
    };

    crate::log!(
        "intel/gpgpu: walkrow16 row={} y={} x={} row_offset=0x{:X} row_gpu=0x{:X} target_gpu=0x{:X} target_phys=0x{:X} pitch_bytes={} color=0x{:08X} pixels={} expected_hit_mask=0x{:04X} hit_mask=0x{:04X} changed_mask=0x{:04X} readback_ok={} reason={} first_before=0x{:08X} first_after=0x{:08X} after0=0x{:08X} after1=0x{:08X} after2=0x{:08X} after3=0x{:08X} after4=0x{:08X} after5=0x{:08X} after6=0x{:08X} after7=0x{:08X} after8=0x{:08X} after9=0x{:08X} after10=0x{:08X} after11=0x{:08X} after12=0x{:08X} after13=0x{:08X} after14=0x{:08X} after15=0x{:08X} display_notified={} finish_marker=0x{:08X} lane_dispatch_delta={} expected_dispatch={} program_source={} batch_bytes=0x{:X}\n",
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
        program.name,
        batch_bytes,
    );

    if !finished {
        recover_render_engine_after_nonretired_submit(
            dev,
            warm,
            "gpgpu-primary-scanout-walkrow16",
        );
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
