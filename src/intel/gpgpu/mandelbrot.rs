use super::*;

fn gpgpu_primary_scanout_pixel_quiet_program() -> GpgpuEuProgram {
    let artifact = trueos_eu::gfx12::HDC1_STATELESS_STORE_THEN_TS_EOT;
    GpgpuEuProgram {
        name: "gfx12-t47-primary-scanout-pixel-quiet-hdc1-stateless-store-then-ts-eot",
        kind: artifact.kind,
        words: artifact.words,
        expects_store: artifact.expects_store,
        expected_store_value: GPGPU_ONE_TILE_OUTPUT_SENTINEL,
        store_send_dword: Some(trueos_eu::gfx12::HDC1_BTI34_STORE_SEND_DWORD),
        visible_seed_dword: Some(trueos_eu::gfx12::HDC1_BTI34_STORE_IMM_DWORD),
    }
}

fn gpgpu_primary_scanout_mandelbrot8_program() -> GpgpuEuProgram {
    let artifact =
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_ESCAPE_HDC1_BTI1_STORE_THEN_TS_EOT;
    GpgpuEuProgram {
        name: artifact.name,
        kind: artifact.kind,
        words: artifact.words,
        expects_store: artifact.expects_store,
        expected_store_value: 0,
        store_send_dword: Some(
            trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_STORE_EXDESC_DWORD,
        ),
        visible_seed_dword: None,
    }
}

pub(crate) fn submit_gpgpu_primary_scanout_marker_probe() -> crate::intel::GpgpuOneTileSentinelProof
{
    let program = gpgpu_one_tile_output_sentinel_program();
    let Some(dev) = crate::intel::claimed_device() else {
        return gpgpu_one_tile_sentinel_failure("no-device", program, 0);
    };
    let Some(warm) = warm_state() else {
        return gpgpu_one_tile_sentinel_failure("no-warm-state", program, 0);
    };
    let Some(target) = crate::intel::display::primary_surface_gpgpu_marker_target() else {
        return gpgpu_one_tile_sentinel_failure("no-primary-scanout", program, 0);
    };
    if target.marker_gpu >> 32 != 0 {
        return gpgpu_one_tile_sentinel_failure(
            "marker-gpu-high32-unsupported",
            program,
            target.marker_gpu,
        );
    }
    if !forcewake_render_acquire(warm) {
        return gpgpu_one_tile_sentinel_failure("forcewake", program, target.marker_gpu);
    }
    if !ensure_gpgpu_warm_buffers_mapped(dev, warm) {
        return gpgpu_one_tile_sentinel_failure("ggtt-map", program, target.marker_gpu);
    }

    unsafe {
        core::ptr::write_volatile(target.marker_virt as *mut u32, 0);
    }
    crate::intel::dma_flush(target.marker_virt, core::mem::size_of::<u32>());
    let output_first_before = unsafe { core::ptr::read_volatile(target.marker_virt as *const u32) };

    let mut sentinel_words = trueos_eu::gfx12::HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS;
    sentinel_words[trueos_eu::gfx12::HDC1_BTI34_STORE_IMM_DWORD] = GPGPU_ONE_TILE_OUTPUT_SENTINEL;
    sentinel_words[7] = target.marker_gpu as u32;
    if !upload_and_verify_gpu_program_at(warm, GPGPU_EU_KERNEL_OFFSET_BYTES, &sentinel_words) {
        return gpgpu_one_tile_sentinel_failure("program-upload", program, target.marker_gpu);
    }

    unsafe {
        core::ptr::write_volatile(
            warm.result_virt
                .add(RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD * core::mem::size_of::<u32>())
                as *mut u32,
            0,
        );
        for breadcrumb_slot in 23..=28 {
            core::ptr::write_volatile(
                warm.result_virt
                    .add(breadcrumb_slot * core::mem::size_of::<u32>()) as *mut u32,
                0,
            );
        }
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
    }
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let batch_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, batch_dwords) };
    let store_surface = prepare_gpgpu_store_surface_state_for_target(
        warm,
        target.marker_gpu,
        "bind-send-bti-to-primary-scanout-marker",
    );
    let batch_bytes =
        match encode_gfx12_gpgpu_walker_probe_batch(warm, batch, store_surface, program, 1) {
            Ok(bytes) => bytes,
            Err(reason) => {
                return gpgpu_one_tile_sentinel_failure(reason, program, target.marker_gpu);
            }
        };
    crate::intel::dma_flush(warm.batch_virt, batch_bytes);

    let dispatch_before = read_gpgpu_threads_dispatched(dev);
    let finished = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD,
        "gpgpu-primary-scanout-marker",
    );
    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    let display_notified = crate::intel::display::notify_primary_surface_external_write(
        "gpgpu-primary-scanout-marker",
        target.marker_offset,
        core::mem::size_of::<u32>(),
    );
    let output_first_after = unsafe { core::ptr::read_volatile(target.marker_virt as *const u32) };
    let dispatch_after = read_gpgpu_threads_dispatched(dev);
    let dispatch_delta = dispatch_after.saturating_sub(dispatch_before);
    let finish_marker = read_result_dword(warm, RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD);
    let output_hits_lo64 = if output_first_after == GPGPU_ONE_TILE_OUTPUT_SENTINEL {
        1
    } else {
        0
    };
    let readback_ok = output_first_before == 0
        && output_first_after == GPGPU_ONE_TILE_OUTPUT_SENTINEL
        && finished
        && finish_marker == RCS_EXEC_RESULT_COMPUTE_WALKER_DONE;
    let reason = if readback_ok && dispatch_delta == 0 {
        "scanout-sentinel-written-no-ts-delta"
    } else if readback_ok {
        "scanout-sentinel-written"
    } else if !finished {
        "submit-not-finished"
    } else if output_first_after != GPGPU_ONE_TILE_OUTPUT_SENTINEL {
        "scanout-sentinel-missing"
    } else {
        "scanout-sentinel-not-clean"
    };
    crate::log!(
        "intel/gpgpu: primary-scanout-marker submitted=1 finished={} readback_ok={} reason={} program_source={} primary_gpu=0x{:X} primary_phys=0x{:X} primary_bytes=0x{:X} marker_gpu=0x{:X} marker_off=0x{:X} xy={}x{} before=0x{:08X} after=0x{:08X} sentinel=0x{:08X} output_hits_lo64=0x{:016X} display_notified={} lane_dispatch={} finish_marker=0x{:08X} finish_expected=0x{:08X} batch_bytes=0x{:X} action={} next=expand-marker-to-visible-block-or-mandelbrot-pixels does_not_prove=fragment_shader_mandelbrot_pixels\n",
        finished as u8,
        readback_ok as u8,
        reason,
        program.name,
        target.gpu,
        target.phys,
        target.byte_len,
        target.marker_gpu,
        target.marker_offset,
        target.marker_x,
        target.marker_y,
        output_first_before,
        output_first_after,
        GPGPU_ONE_TILE_OUTPUT_SENTINEL,
        output_hits_lo64,
        display_notified as u8,
        dispatch_delta,
        finish_marker,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        batch_bytes,
        if readback_ok {
            "continue-framebuffer-target"
        } else {
            "fix-primary-scanout-target"
        },
    );
    if !finished {
        recover_render_engine_after_nonretired_submit(dev, warm, "gpgpu-primary-scanout-marker");
    }
    crate::intel::GpgpuOneTileSentinelProof {
        submitted: true,
        finished,
        readback_ok,
        reason,
        program_name: program.name,
        output_gpu: target.marker_gpu,
        sentinel: GPGPU_ONE_TILE_OUTPUT_SENTINEL,
        output_first_before,
        output_first_after,
        output_nonzero_before: (output_first_before != 0) as usize,
        output_nonzero_after: (output_first_after != 0) as usize,
        output_hits_lo64,
        dispatch_delta,
        finish_marker,
        expected_finish_marker: RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        batch_bytes,
    }
}

fn mandelbrot_q12_x_step(width: usize) -> i32 {
    let scale = 1i64 << trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_FRAC_BITS;
    ((3 * scale + (width.max(1) as i64 / 2)) / width.max(1) as i64) as i32
}

fn mandelbrot_q12_c_re_base(x_base: usize, width: usize) -> i32 {
    let scale = 1i64 << trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_FRAC_BITS;
    (-2 * scale + (x_base as i64 * 3 * scale) / width.max(1) as i64) as i32
}

fn mandelbrot_q12_c_im(y: usize, height: usize) -> i32 {
    let scale = 1i64 << trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_FRAC_BITS;
    (-scale + (y as i64 * 2 * scale) / height.max(1) as i64) as i32
}

fn submit_gpgpu_primary_scanout_pixel_quiet(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    program: GpgpuEuProgram,
    pixel_gpu: u64,
    pixel_virt: *mut u8,
    color: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    if pixel_gpu >> 32 != 0 {
        return gpgpu_one_tile_sentinel_failure("pixel-gpu-high32-unsupported", program, pixel_gpu);
    }

    let output_first_before = unsafe { core::ptr::read_volatile(pixel_virt as *const u32) };
    let mut pixel_words = trueos_eu::gfx12::HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS;
    pixel_words[trueos_eu::gfx12::HDC1_BTI34_STORE_IMM_DWORD] = color;
    pixel_words[7] = pixel_gpu as u32;
    if !upload_and_verify_gpu_program_at(warm, GPGPU_EU_KERNEL_OFFSET_BYTES, &pixel_words) {
        return gpgpu_one_tile_sentinel_failure("program-upload", program, pixel_gpu);
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
    let store_surface = prepare_gpgpu_store_surface_state_for_target(
        warm,
        pixel_gpu,
        "bind-send-bti-to-primary-scanout-pixel-quiet",
    );
    let batch_bytes =
        match encode_gfx12_gpgpu_walker_probe_batch(warm, batch, store_surface, program, 1) {
            Ok(bytes) => bytes,
            Err(reason) => return gpgpu_one_tile_sentinel_failure(reason, program, pixel_gpu),
        };
    crate::intel::dma_flush(warm.batch_virt, batch_bytes);

    let dispatch_before = read_gpgpu_threads_dispatched(dev);
    let finished = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD,
        "gpgpu-primary-scanout-pixel-quiet",
    );
    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    crate::intel::dma_flush(pixel_virt, core::mem::size_of::<u32>());
    let output_first_after = unsafe { core::ptr::read_volatile(pixel_virt as *const u32) };
    let dispatch_after = read_gpgpu_threads_dispatched(dev);
    let dispatch_delta = dispatch_after.saturating_sub(dispatch_before);
    let finish_marker = read_result_dword(warm, RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD);
    let output_hits_lo64 = if output_first_after == color { 1 } else { 0 };
    let readback_ok = output_first_after == color
        && finished
        && finish_marker == RCS_EXEC_RESULT_COMPUTE_WALKER_DONE;
    let reason = if readback_ok {
        "scanout-pixel-written"
    } else if !finished {
        "submit-not-finished"
    } else if output_first_after != color {
        "scanout-pixel-mismatch"
    } else {
        "scanout-pixel-not-clean"
    };
    if !finished {
        recover_render_engine_after_nonretired_submit(
            dev,
            warm,
            "gpgpu-primary-scanout-pixel-quiet",
        );
    }
    crate::intel::GpgpuOneTileSentinelProof {
        submitted: batch_bytes != 0,
        finished,
        readback_ok,
        reason,
        program_name: program.name,
        output_gpu: pixel_gpu,
        sentinel: color,
        output_first_before,
        output_first_after,
        output_nonzero_before: (output_first_before != 0) as usize,
        output_nonzero_after: (output_first_after != 0) as usize,
        output_hits_lo64,
        dispatch_delta,
        finish_marker,
        expected_finish_marker: RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        batch_bytes,
    }
}

fn submit_gpgpu_primary_scanout_mandelbrot_strip(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    program: GpgpuEuProgram,
    scanout_gpu: u64,
    scanout_bytes: usize,
    row_gpu: u64,
    row_virt: *mut u8,
    x_base: usize,
    y: usize,
    width: usize,
    height: usize,
    phase: usize,
) -> crate::intel::GpgpuOneTileSentinelProof {
    const LANES: usize = trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_LANES;
    const PIXELS_PER_PROGRAM: usize =
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_PIXELS_PER_PROGRAM;

    if row_gpu >> 32 != 0 {
        return gpgpu_one_tile_sentinel_failure("strip-gpu-high32-unsupported", program, row_gpu);
    }
    if row_gpu < scanout_gpu {
        return gpgpu_one_tile_sentinel_failure("strip-before-scanout", program, row_gpu);
    }
    let row_offset = row_gpu - scanout_gpu;
    if row_offset >> 32 != 0 {
        return gpgpu_one_tile_sentinel_failure(
            "strip-offset-high32-unsupported",
            program,
            row_gpu,
        );
    }

    let x_step_q12 = mandelbrot_q12_x_step(width);
    let c_re_base_q12 = mandelbrot_q12_c_re_base(x_base, width);
    let c_im_q12 = mandelbrot_q12_c_im(y, height);
    crate::intel::dma_flush(row_virt, PIXELS_PER_PROGRAM * core::mem::size_of::<u32>());
    let mut before_words = [0u32; PIXELS_PER_PROGRAM];
    let mut lane = 0usize;
    while lane < PIXELS_PER_PROGRAM {
        before_words[lane] = unsafe {
            core::ptr::read_volatile(row_virt.add(lane * core::mem::size_of::<u32>()) as *const u32)
        };
        lane += 1;
    }
    let output_first_before = before_words[0];

    let mut strip_words =
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_ESCAPE_HDC1_BTI1_STORE_THEN_TS_EOT_WORDS;
    let x_step_dwords = trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_X_STEP_DWORDS;
    let c_re_base_dwords = trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_C_RE_BASE_DWORDS;
    let address_offset_dwords =
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_ADDRESS_OFFSET_DWORDS;
    for strip in 0..trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_STRIPS_PER_PROGRAM {
        strip_words[x_step_dwords[strip]] = x_step_q12 as u32;
        strip_words[c_re_base_dwords[strip]] = c_re_base_q12 as u32;
        strip_words[address_offset_dwords[strip]] = row_offset as u32;
    }
    strip_words[trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_C_IM_DWORD] =
        c_im_q12 as u32;
    if x_base == 0 && y == 0 && !MANDELBROT_Q12_PATCH_LOGGED.swap(true, Ordering::AcqRel) {
        let send_dword = trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_STORE_EXDESC_DWORD;
        let artifact_bytes = strip_words.len() * core::mem::size_of::<u32>();
        let x_step_dword = x_step_dwords[0];
        let c_re_base_dword = c_re_base_dwords[0];
        let address_offset_dword = address_offset_dwords[0];
        crate::log!(
            "intel/gpgpu: primary-scanout-mandelbrot16-patch scanout_gpu=0x{:X} row_gpu=0x{:X} row_virt=0x{:X} row={} x_base={} width={} height={} phase={} addressing=simd8x2-lane-derived-bti1-surface-relative-g127 q12_frac_bits={} max_iter={} store_surface=0x{:02X} lanes_per_send={} sends_per_program={} pixels_per_program={} x_step_q12={} c_re_base_q12={} c_im_q12={} x_step_dword={} c_re_base_dword={} c_im_dword={} address_offset_dword={} address_offset=0x{:X} first_before=0x{:08X} send_desc=0x{:08X} send_exdesc=0x{:08X} kernel_off=0x{:X} artifact_bytes=0x{:X} artifact_end_off=0x{:X} dynamic_state_off=0x{:X} bt_off=0x{:X} surf_off=0x{:X} store_state_after_artifact={} note=uniform-setup-patched-eu-words-before-upload\n",
            scanout_gpu,
            row_gpu,
            row_virt as usize,
            y,
            x_base,
            width,
            height,
            phase,
            trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_FRAC_BITS,
            trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_MAX_ITER,
            trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_STORE_SURFACE,
            LANES,
            trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_STRIPS_PER_PROGRAM,
            PIXELS_PER_PROGRAM,
            x_step_q12,
            c_re_base_q12,
            c_im_q12,
            x_step_dword,
            c_re_base_dword,
            trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_C_IM_DWORD,
            address_offset_dword,
            strip_words[address_offset_dword],
            output_first_before,
            strip_words[send_dword - 1],
            strip_words[send_dword],
            GPGPU_EU_KERNEL_OFFSET_BYTES,
            artifact_bytes,
            GPGPU_EU_KERNEL_OFFSET_BYTES.saturating_add(artifact_bytes),
            GPGPU_WALKER_SCRATCH_OFFSET_BYTES,
            GPGPU_MANDELBROT_STORE_BINDING_TABLE_OFFSET_BYTES,
            GPGPU_MANDELBROT_STORE_SURFACE_STATE_OFFSET_BYTES,
            (GPGPU_MANDELBROT_STORE_BINDING_TABLE_OFFSET_BYTES
                >= GPGPU_EU_KERNEL_OFFSET_BYTES.saturating_add(artifact_bytes)) as u8,
        );
    }
    if !upload_and_verify_gpu_program_at(warm, GPGPU_EU_KERNEL_OFFSET_BYTES, &strip_words) {
        return gpgpu_one_tile_sentinel_failure("program-upload", program, row_gpu);
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
        scanout_gpu,
        scanout_bytes,
        "bind-stateless-hdc253-to-primary-scanout-full-surface-quiet",
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
        "gpgpu-primary-scanout-mandelbrot8-strip",
    );
    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    let mut hits = 0u64;
    let readback_poll_limit = if finished {
        MANDELBROT_STRIP_READBACK_POLLS
    } else {
        1
    };
    let mut readback_poll = 0usize;
    let mut output_first_after = output_first_before;
    while readback_poll < readback_poll_limit {
        crate::intel::dma_flush(row_virt, PIXELS_PER_PROGRAM * core::mem::size_of::<u32>());
        hits = 0;
        let mut lane = 0usize;
        while lane < PIXELS_PER_PROGRAM {
            let after = unsafe {
                core::ptr::read_volatile(
                    row_virt.add(lane * core::mem::size_of::<u32>()) as *const u32
                )
            };
            if after != before_words[lane] {
                hits |= 1u64 << lane;
            }
            if lane == 0 {
                output_first_after = after;
            }
            lane += 1;
        }
        if hits != 0 {
            break;
        }
        readback_poll += 1;
        core::hint::spin_loop();
    }

    let dispatch_after = read_gpgpu_threads_dispatched(dev);
    let dispatch_delta = dispatch_after.saturating_sub(dispatch_before);
    let finish_marker = read_result_dword(warm, RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD);
    let readback_ok = finished && finish_marker == RCS_EXEC_RESULT_COMPUTE_WALKER_DONE && hits != 0;
    let reason = if readback_ok {
        "mandelbrot16-program-changed"
    } else if !finished {
        "submit-not-finished"
    } else if dispatch_delta == 0 {
        "mandelbrot16-no-eu-dispatch"
    } else if hits == 0 {
        "mandelbrot16-program-unchanged"
    } else {
        "mandelbrot16-program-partial"
    };
    if !finished {
        recover_render_engine_after_nonretired_submit(
            dev,
            warm,
            "gpgpu-primary-scanout-mandelbrot8-strip",
        );
    }
    crate::intel::GpgpuOneTileSentinelProof {
        submitted: batch_bytes != 0,
        finished,
        readback_ok,
        reason,
        program_name: program.name,
        output_gpu: row_gpu,
        sentinel: output_first_before,
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

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot_preview(
    cursor: usize,
    target_phase: usize,
    pixel_budget: usize,
) -> (crate::intel::GpgpuOneTileSentinelProof, usize) {
    const ROW_INTERLACE: usize = 16;
    const STRIP_BURST_MAX: usize = 64;
    const QUADRANT_PREVIEW_W: u32 = 640;
    const QUADRANT_PREVIEW_H: u32 = 360;
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

    let quadrant = target_phase & 3;
    let half_w = target.width / 2;
    let half_h = target.height / 2;
    let block_x = if quadrant & 1 == 0 { 0 } else { half_w };
    let block_y = if quadrant & 2 == 0 { 0 } else { half_h };
    let block_w = core::cmp::min(QUADRANT_PREVIEW_W, target.width.saturating_sub(block_x)) as usize;
    let block_h =
        core::cmp::min(QUADRANT_PREVIEW_H, target.height.saturating_sub(block_y)) as usize;
    let strips_per_row = block_w / PIXELS_PER_PROGRAM;
    let total_strips = strips_per_row.saturating_mul(block_h);
    if total_strips == 0 || pixel_budget < PIXELS_PER_PROGRAM {
        return (
            gpgpu_one_tile_sentinel_failure("empty-preview-strip-block", program, target.gpu),
            cursor,
        );
    }

    let start_cursor = cursor % total_strips;
    let phase = cursor / total_strips;
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
        let logical_row = idx / strips_per_row;
        let rows_per_band = (block_h + ROW_INTERLACE - 1) / ROW_INTERLACE;
        let band = logical_row / rows_per_band;
        let band_row = logical_row % rows_per_band;
        let interlaced_row = band_row.saturating_mul(ROW_INTERLACE).saturating_add(band);
        let py = if interlaced_row < block_h {
            interlaced_row
        } else {
            logical_row
        };
        let px = strip_x * PIXELS_PER_PROGRAM;
        let byte_offset = ((block_y as usize + py) * target.pitch_bytes as usize)
            + ((block_x as usize + px).saturating_mul(core::mem::size_of::<u32>()));
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
            block_w,
            block_h,
            phase,
        );
        submitted_strips += proof.submitted as usize;
        let strip_changed = proof.finished
            && proof.finish_marker == RCS_EXEC_RESULT_COMPUTE_WALKER_DONE
            && proof.output_hits_lo64 != 0;
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

    let flush_offset = (block_y as usize * target.pitch_bytes as usize)
        + (block_x as usize * core::mem::size_of::<u32>());
    let flush_bytes = block_h
        .saturating_sub(1)
        .saturating_mul(target.pitch_bytes as usize)
        .saturating_add(block_w.saturating_mul(core::mem::size_of::<u32>()));
    let display_notified = accepted_strips != 0
        && crate::intel::display::notify_primary_surface_external_write(
            "gpgpu-primary-scanout-mandelbrot-preview",
            flush_offset,
            flush_bytes,
        );
    let next_cursor = (start_cursor + advanced_strips) % total_strips;
    let readback_ok =
        submitted_strips != 0 && submitted_strips == accepted_strips && last_proof.readback_ok;
    let first_failed_preview_log =
        !last_proof.finished && !MANDELBROT_PREVIEW_FAILURE_LOGGED.swap(true, Ordering::AcqRel);
    let should_log_preview = (accepted_strips != 0 && (start_cursor == 0 || next_cursor == 0))
        || first_failed_preview_log;
    if should_log_preview {
        crate::log!(
            "intel/gpgpu: primary-scanout-mandelbrot16-preview target_quadrant={} block={}x{}@{}x{} submitted_programs={} finished_programs={} changed_programs={} advanced_programs={} pixels_per_program={} submitted_pixels={} changed_pixels={} strict_readback_ok={} reason={} program_source={} primary_gpu=0x{:X} primary_bytes=0x{:X} cursor_in={} cursor_out={} strip_budget={} burst_cap={} last_gpu=0x{:X} last_first_before=0x{:08X} last_first_after=0x{:08X} last_change_mask=0x{:016X} display_notified={} finish_marker=0x{:08X} finish_expected=0x{:08X} lane_dispatch_delta={} action={} next={} deliverable=visible-mandelbrot-frame-progress\n",
            quadrant,
            block_w,
            block_h,
            block_x,
            block_y,
            submitted_strips,
            finished_strips,
            accepted_strips,
            advanced_strips,
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
                "continue-gpgpu-strip-preview"
            } else {
                "hold-cursor-until-scanout-changes"
            },
            if next_cursor == 0 {
                "frame-covered"
            } else {
                "continue-preview-strips"
            },
        );
    }
    (last_proof, next_cursor)
}
