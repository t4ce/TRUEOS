pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe(
    mode: u32,
    row_index: u32,
    x_base: u32,
    lhs: u32,
    rhs: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe_impl(
        mode, row_index, x_base, 1, lhs, rhs, true,
    )
}

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_quiet_linear_band(
    row_index: u32,
    x_base: u32,
    row_groups: u32,
    x_blocks: u32,
    lhs: u32,
    rhs: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe_impl(
        MANDELBROT16_T15_MODE_LINEAR_GRADIENT_FULL_BAND,
        row_index,
        x_base,
        row_groups.max(1).saturating_mul(x_blocks.max(1)),
        lhs,
        rhs,
        false,
    )
}

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_linear_band_probe(
    row_index: u32,
    x_base: u32,
    row_groups: u32,
    x_blocks: u32,
    lhs: u32,
    rhs: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe_impl(
        MANDELBROT16_T15_MODE_LINEAR_GRADIENT_FULL_BAND,
        row_index,
        x_base,
        row_groups.max(1).saturating_mul(x_blocks.max(1)),
        lhs,
        rhs,
        true,
    )
}

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_immediate_gradient_probe(
    row_index: u32,
    x_base: u32,
    lhs: u32,
    rhs: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe_impl(
        MANDELBROT16_T18_MODE_IMMEDIATE_GRADIENT_STORE,
        row_index,
        x_base,
        1,
        lhs,
        rhs,
        true,
    )
}

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_quiet_immediate_raw_radius_rows(
    row_index: u32,
    x_base: u32,
    row_groups: u32,
    lhs: u32,
    rhs: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe_impl(
        MANDELBROT16_T19_MODE_IMMEDIATE_RAW_RADIUS_STORE,
        row_index,
        x_base,
        row_groups.max(1),
        lhs,
        rhs,
        false,
    )
}

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_immediate_raw_radius_probe(
    row_index: u32,
    x_base: u32,
    lhs: u32,
    rhs: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe_impl(
        MANDELBROT16_T19_MODE_IMMEDIATE_RAW_RADIUS_STORE,
        row_index,
        x_base,
        1,
        lhs,
        rhs,
        true,
    )
}

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_linear_constant_probe(
    row_index: u32,
    x_base: u32,
    color: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe_impl(
        MANDELBROT16_T16_MODE_LINEAR_CONSTANT_STORE,
        row_index,
        x_base,
        1,
        color,
        0,
        true,
    )
}

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_immediate_constant_probe(
    row_index: u32,
    x_base: u32,
    color: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe_impl(
        MANDELBROT16_T17_MODE_IMMEDIATE_CONSTANT_STORE,
        row_index,
        x_base,
        1,
        color,
        0,
        true,
    )
}

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_quiet_immediate_constant(
    row_index: u32,
    x_base: u32,
    color: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe_impl(
        MANDELBROT16_T17_MODE_IMMEDIATE_CONSTANT_STORE,
        row_index,
        x_base,
        1,
        color,
        0,
        false,
    )
}

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_immediate_constant_rows_probe(
    row_index: u32,
    x_base: u32,
    row_groups: u32,
    color: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe_impl(
        MANDELBROT16_T17_MODE_IMMEDIATE_CONSTANT_STORE,
        row_index,
        x_base,
        row_groups.max(1),
        color,
        0,
        true,
    )
}

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_t31_store_ladder_probe(
    heartbeat: u64,
    row_index: u32,
    x_base: u32,
    color: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    let proof = submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe_impl(
        MANDELBROT16_T17_MODE_IMMEDIATE_CONSTANT_STORE,
        row_index,
        x_base,
        1,
        color,
        0,
        true,
    );
    let hit_mask = proof.output_hits_lo64 as u16;
    crate::log!(
        "intel/gpgpu: t31-mandelbrot16-store-ladder heartbeat={} row={} x_base={} color=0x{:08X} submitted={} finished={} all_lane_readback_ok={} hit_mask=0x{:04X} target_lane0=0x0001 pass_lane0={} target_lane01=0x0003 pass_lane01={} target_lane03=0x000F pass_lane03={} target_lane07=0x00FF pass_lane07={} target_lane15=0xFFFF pass_lane15={} sample_gpu=0x{:X} sample_before=0x{:08X} sample_after=0x{:08X} sample_expected=0x{:08X} lane_dispatch_delta={} finish_marker=0x{:08X} artifact_body=t31-store-payload-ladder-over-t17-immediate-constant purpose=score-simd16-collapse-without-changing-t30-visual-bridge next=if-hit-mask-stays-0001-try-send-descriptor-and-grf-payload-variants\n",
        heartbeat,
        row_index,
        x_base,
        color,
        proof.submitted as u8,
        proof.finished as u8,
        proof.readback_ok as u8,
        hit_mask,
        ((hit_mask & 0x0001) == 0x0001) as u8,
        ((hit_mask & 0x0003) == 0x0003) as u8,
        ((hit_mask & 0x000F) == 0x000F) as u8,
        ((hit_mask & 0x00FF) == 0x00FF) as u8,
        ((hit_mask & 0xFFFF) == 0xFFFF) as u8,
        proof.output_gpu,
        proof.output_first_before,
        proof.output_first_after,
        proof.sentinel,
        proof.dispatch_delta,
        proof.finish_marker,
    );
    proof
}

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_t32_single_send_probe(
    heartbeat: u64,
    row_index: u32,
    x_base: u32,
    color: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    let proof = submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe_impl(
        MANDELBROT16_T32_MODE_IMMEDIATE_CONSTANT_SINGLE_SEND,
        row_index,
        x_base,
        1,
        color,
        0,
        true,
    );
    let hit_mask = proof.output_hits_lo64 as u16;
    crate::log!(
        "intel/gpgpu: t32-mandelbrot16-single-send heartbeat={} row={} x_base={} color=0x{:08X} submitted={} finished={} all_lane_readback_ok={} hit_mask=0x{:04X} pass_lane0={} pass_lane01={} pass_lane03={} pass_lane07={} pass_lane15={} sample_gpu=0x{:X} sample_before=0x{:08X} sample_after=0x{:08X} sample_expected=0x{:08X} lane_dispatch_delta={} finish_marker=0x{:08X} artifact_body=t32-original-single-simd16-send-g20-g22 purpose=test-original-send16-descriptor-against-t31-two-send-collapse next=if-t32-wide-then-promote-send16-into-t30-else-build-explicit-g21-g23-payload\n",
        heartbeat,
        row_index,
        x_base,
        color,
        proof.submitted as u8,
        proof.finished as u8,
        proof.readback_ok as u8,
        hit_mask,
        ((hit_mask & 0x0001) == 0x0001) as u8,
        ((hit_mask & 0x0003) == 0x0003) as u8,
        ((hit_mask & 0x000F) == 0x000F) as u8,
        ((hit_mask & 0x00FF) == 0x00FF) as u8,
        ((hit_mask & 0xFFFF) == 0xFFFF) as u8,
        proof.output_gpu,
        proof.output_first_before,
        proof.output_first_after,
        proof.sentinel,
        proof.dispatch_delta,
        proof.finish_marker,
    );
    proof
}

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_t33_bti1_untyped_probe(
    heartbeat: u64,
    row_index: u32,
    x_base: u32,
    color: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    let proof = submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe_impl(
        MANDELBROT16_T33_MODE_IMMEDIATE_CONSTANT_BTI1_UNTYPED,
        row_index,
        x_base,
        1,
        color,
        0,
        true,
    );
    let hit_mask = proof.output_hits_lo64 as u16;
    crate::log!(
        "intel/gpgpu: t33-mandelbrot16-bti1-untyped heartbeat={} row={} x_base={} color=0x{:08X} submitted={} finished={} all_lane_readback_ok={} hit_mask=0x{:04X} pass_lane0={} pass_lane01={} pass_lane03={} pass_lane07={} pass_lane15={} sample_gpu=0x{:X} sample_before=0x{:08X} sample_after=0x{:08X} sample_expected=0x{:08X} lane_dispatch_delta={} finish_marker=0x{:08X} artifact_body=t33-two-simd8-bti1-untyped-g20-g22-g21-g23 purpose=separate-address-payload-from-legacy-stateless-send-descriptor next=if-wide-promote-bti1-untyped-descriptor-to-t30-else-lane-address-payload-is-collapsing\n",
        heartbeat,
        row_index,
        x_base,
        color,
        proof.submitted as u8,
        proof.finished as u8,
        proof.readback_ok as u8,
        hit_mask,
        ((hit_mask & 0x0001) == 0x0001) as u8,
        ((hit_mask & 0x0003) == 0x0003) as u8,
        ((hit_mask & 0x000F) == 0x000F) as u8,
        ((hit_mask & 0x00FF) == 0x00FF) as u8,
        ((hit_mask & 0xFFFF) == 0xFFFF) as u8,
        proof.output_gpu,
        proof.output_first_before,
        proof.output_first_after,
        proof.sentinel,
        proof.dispatch_delta,
        proof.finish_marker,
    );
    proof
}

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_t34_address_data_witness_probe(
    heartbeat: u64,
    row_index: u32,
    x_base: u32,
    color_mask: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    let proof = submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe_impl(
        MANDELBROT16_T34_MODE_IMMEDIATE_ADDRESS_DATA_WITNESS,
        row_index,
        x_base,
        1,
        color_mask,
        0,
        true,
    );
    let hit_mask = proof.output_hits_lo64 as u16;
    let lane0_only = (hit_mask & 0x0001) == 0x0001
        && (hit_mask & 0xFFFE) == 0
        && proof.output_first_after == proof.sentinel;
    let alias_or_late_lane = proof.output_first_after != proof.output_first_before
        && proof.output_first_after != proof.sentinel;
    crate::log!(
        "intel/gpgpu: t34-mandelbrot16-address-data-witness heartbeat={} row={} x_base={} color_mask=0x{:08X} submitted={} finished={} all_lane_readback_ok={} hit_mask=0x{:04X} lane0_only={} alias_or_late_lane={} sample_gpu=0x{:X} sample_before=0x{:08X} sample_after=0x{:08X} lane0_expected=0x{:08X} lane_dispatch_delta={} finish_marker=0x{:08X} artifact_body=t34-address-derived-data-over-legacy-two-send purpose=distinguish-lane0-only-from-multilane-address-alias next=if-lane0-only-materialize-wide-address-data-payload else-fix-address-vector\n",
        heartbeat,
        row_index,
        x_base,
        color_mask,
        proof.submitted as u8,
        proof.finished as u8,
        proof.readback_ok as u8,
        hit_mask,
        lane0_only as u8,
        alias_or_late_lane as u8,
        proof.output_gpu,
        proof.output_first_before,
        proof.output_first_after,
        proof.sentinel,
        proof.dispatch_delta,
        proof.finish_marker,
    );
    proof
}

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_t35_explicit_wide_payload_probe(
    heartbeat: u64,
    row_index: u32,
    x_base: u32,
    color: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    let proof = submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe_impl(
        MANDELBROT16_T35_MODE_IMMEDIATE_EXPLICIT_WIDE_PAYLOAD,
        row_index,
        x_base,
        1,
        color,
        0,
        true,
    );
    let hit_mask = proof.output_hits_lo64 as u16;
    let low_half_ok = (hit_mask & 0x00FF) == 0x00FF;
    let high_half_ok = (hit_mask & 0xFF00) == 0xFF00;
    let high_lane0_ok = (hit_mask & 0x0100) == 0x0100;
    crate::log!(
        "intel/gpgpu: t35-mandelbrot16-explicit-wide-payload heartbeat={} row={} x_base={} color=0x{:08X} submitted={} finished={} all_lane_readback_ok={} hit_mask=0x{:04X} pass_lane0={} pass_low8={} pass_lane8={} pass_high8={} pass_lane15={} sample_gpu=0x{:X} sample_before=0x{:08X} sample_after=0x{:08X} sample_expected=0x{:08X} lane_dispatch_delta={} finish_marker=0x{:08X} artifact_body=t35-explicit-g21-g22-g23-payload-over-legacy-two-send purpose=separate-missing-high-payload-from-send-descriptor-collapse next=if-wide-promote-explicit-payload else-rebuild-message-descriptor-contract\n",
        heartbeat,
        row_index,
        x_base,
        color,
        proof.submitted as u8,
        proof.finished as u8,
        proof.readback_ok as u8,
        hit_mask,
        ((hit_mask & 0x0001) == 0x0001) as u8,
        low_half_ok as u8,
        high_lane0_ok as u8,
        high_half_ok as u8,
        ((hit_mask & 0x8000) == 0x8000) as u8,
        proof.output_gpu,
        proof.output_first_before,
        proof.output_first_after,
        proof.sentinel,
        proof.dispatch_delta,
        proof.finish_marker,
    );
    proof
}

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_t36_unrolled_scalar16_probe(
    heartbeat: u64,
    row_index: u32,
    x_base: u32,
    color: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    let proof = submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe_impl(
        MANDELBROT16_T36_MODE_IMMEDIATE_UNROLLED_SCALAR16,
        row_index,
        x_base,
        1,
        color,
        0,
        true,
    );
    let hit_mask = proof.output_hits_lo64 as u16;
    crate::log!(
        "intel/gpgpu: t36-mandelbrot16-unrolled-scalar16 heartbeat={} row={} x_base={} color=0x{:08X} submitted={} finished={} all_lane_readback_ok={} hit_mask=0x{:04X} pass_lane0={} pass_lane01={} pass_lane03={} pass_lane07={} pass_lane15={} sample_gpu=0x{:X} sample_before=0x{:08X} sample_after=0x{:08X} sample_expected=0x{:08X} lane_dispatch_delta={} finish_marker=0x{:08X} artifact_body=t36-sixteen-scalar-hdc-stores-from-one-eu-invocation purpose=prove-simd8-send-is-scalar-store-and-replace-cpu-lane-phasing next=if-ffff-promote-to-t30-block-fill-then-add-gpu-color-math\n",
        heartbeat,
        row_index,
        x_base,
        color,
        proof.submitted as u8,
        proof.finished as u8,
        proof.readback_ok as u8,
        hit_mask,
        ((hit_mask & 0x0001) == 0x0001) as u8,
        ((hit_mask & 0x0003) == 0x0003) as u8,
        ((hit_mask & 0x000F) == 0x000F) as u8,
        ((hit_mask & 0x00FF) == 0x00FF) as u8,
        ((hit_mask & 0xFFFF) == 0xFFFF) as u8,
        proof.output_gpu,
        proof.output_first_before,
        proof.output_first_after,
        proof.sentinel,
        proof.dispatch_delta,
        proof.finish_marker,
    );
    proof
}

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_t37_groupid_x_unrolled_scalar16_probe(
    heartbeat: u64,
    row_index: u32,
    x_base: u32,
    color: u32,
    row_groups: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    let row_groups = row_groups.max(2).min(8);
    let proof = submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe_impl(
        MANDELBROT16_T37_MODE_GROUPID_X_UNROLLED_SCALAR16,
        row_index,
        x_base,
        row_groups,
        color,
        0,
        true,
    );
    let hit_mask = proof.output_hits_lo64 as u16;
    crate::log!(
        "intel/gpgpu: t37-mandelbrot16-groupid-x-unrolled-scalar16 heartbeat={} row={} x_base={} row_groups={} color=0x{:08X} submitted={} finished={} groupid_x_readback_ok={} first_block_hit_mask=0x{:04X} first_block_all_lanes={} sample_gpu=0x{:X} sample_before=0x{:08X} sample_after=0x{:08X} sample_expected=0x{:08X} lane_dispatch_delta={} expected_dispatch={} finish_marker=0x{:08X} artifact_body=t37-groupid-x-linear64-t36-unrolled-scalar16 purpose=prove-gpu-side-groupid-x-selects-adjacent-16px-blocks next=if-readback-ok-promote-t30-from-cpu-xblock-submit-loop-to-row-group-walker else-fix-r0-groupid-source-prelude does_not_prove=groupid-y-or-mandelbrot-math\n",
        heartbeat,
        row_index,
        x_base,
        row_groups,
        color,
        proof.submitted as u8,
        proof.finished as u8,
        proof.readback_ok as u8,
        hit_mask,
        ((hit_mask & 0xFFFF) == 0xFFFF) as u8,
        proof.output_gpu,
        proof.output_first_before,
        proof.output_first_after,
        proof.sentinel,
        proof.dispatch_delta,
        (trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_LANES as u64)
            .saturating_mul(row_groups as u64),
        proof.finish_marker,
    );
    proof
}

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_t30_fullscreen_frame(
    frame_seed: u32,
    rows_per_submit: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    submit_gpgpu_primary_scanout_mandelbrot16_simd16_t30_fullscreen_bands(
        frame_seed,
        0,
        rows_per_submit,
        u32::MAX,
    )
}

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_t30_fullscreen_bands(
    frame_seed: u32,
    first_row: u32,
    rows_per_submit: u32,
    max_bands: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    let program = gpgpu_primary_scanout_mandelbrot16_simd16_bw_program();
    let Some(target) = crate::intel::display::primary_surface_gpgpu_marker_target() else {
        return gpgpu_one_tile_sentinel_failure("no-primary-scanout", program, 0);
    };
    let pixels_per_group =
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_PIXELS_PER_PROGRAM as u32;
    let pixels_per_stamp = pixels_per_group.saturating_mul(MANDELBROT16_T38_STAMP_REPEATS);
    let x_groups = target.width.saturating_add(pixels_per_stamp - 1) / pixels_per_stamp;
    let rows_per_submit = rows_per_submit.clamp(1, target.height.max(1));
    let total_bands = target
        .height
        .saturating_add(rows_per_submit.saturating_sub(1))
        / rows_per_submit;
    let requested_bands = max_bands.max(1).min(total_bands.max(1));
    let start_row = if target.height == 0 {
        0
    } else {
        first_row % target.height
    };
    let mut y = start_row;
    let mut submitted = 0u32;
    let mut finished = 0u32;
    let mut readback_ok = 0u32;
    let mut attempted_bands = 0u32;
    let mut total_dispatch_delta = 0u64;
    let mut total_batch_bytes = 0usize;
    let mut finish_marker = 0u32;
    let mut first_before = 0u32;
    let mut first_after = 0u32;
    let mut first_expected = 0u32;
    let mut first_gpu = target.gpu;
    let mut first_seen = false;
    let base_color = 0xFF00_0000 | (frame_seed & 0x00FF_FFFF);

    while attempted_bands < requested_bands {
        let rows = rows_per_submit.min(target.height.saturating_sub(y)).max(1);
        let groups = x_groups.saturating_mul(rows).max(1);
        let band_color = base_color ^ ((y & 0xFF) << 8) ^ ((rows & 0xFF) << 16);
        let proof = submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe_impl(
            MANDELBROT16_T30_MODE_FULLSCREEN_LINEAR_CONSTANT_STORE,
            y,
            0,
            groups,
            band_color,
            0,
            true,
        );
        submitted = submitted.saturating_add(proof.submitted as u32);
        finished = finished.saturating_add(proof.finished as u32);
        readback_ok = readback_ok.saturating_add(proof.readback_ok as u32);
        total_dispatch_delta = total_dispatch_delta.saturating_add(proof.dispatch_delta);
        total_batch_bytes = total_batch_bytes.saturating_add(proof.batch_bytes);
        finish_marker = proof.finish_marker;
        if !first_seen {
            first_seen = true;
            first_before = proof.output_first_before;
            first_after = proof.output_first_after;
            first_expected = band_color;
            first_gpu = proof.output_gpu;
        }
        attempted_bands = attempted_bands.saturating_add(1);
        if !proof.finished {
            break;
        }
        y = y.saturating_add(rows);
        if y >= target.height {
            y = 0;
        }
    }

    let all_requested_bands_finished = requested_bands != 0 && finished == requested_bands;
    let all_requested_bands_readback_ok = requested_bands != 0 && readback_ok == requested_bands;
    crate::log!(
        "intel/gpgpu: t30-mandelbrot16-fullscreen-bands first_row={} next_row={} submitted_bands={} finished_bands={} readback_ok_bands={} requested_bands={} total_frame_bands={} frame_seed=0x{:08X} target={}x{} pitch_bytes={} x_groups={} rows_per_submit={} groups_per_full_row={} requested_store_pixels={} full_frame_store_pixels={} lane_dispatch_delta={} finish_marker=0x{:08X} program_source={} artifact_body=t30-t36-linear-groupid-unrolled-scalar16-block-fill runtime_contract=redraw-same-frame-on-heartbeat address_path=linear-groupid-x-times-64 primary_gpu=0x{:X} sample_gpu=0x{:X} sample_before=0x{:08X} sample_after=0x{:08X} sample_expected=0x{:08X} sample_match={} validation=exact-first-row-chunk next=raise-rows-per-heartbeat-or-replace-constant-with-coordinate-mandelbrot-color\n",
        start_row,
        y,
        submitted,
        finished,
        readback_ok,
        requested_bands,
        total_bands,
        frame_seed,
        target.width,
        target.height,
        target.pitch_bytes,
        x_groups,
        rows_per_submit,
        x_groups,
        (target.width as u64)
            .saturating_mul(rows_per_submit as u64)
            .saturating_mul(requested_bands as u64),
        (target.width as u64).saturating_mul(target.height as u64),
        total_dispatch_delta,
        finish_marker,
        program.name,
        target.gpu,
        first_gpu,
        first_before,
        first_after,
        first_expected,
        (first_after == first_expected) as u8,
    );

    crate::intel::GpgpuOneTileSentinelProof {
        submitted: submitted != 0,
        finished: all_requested_bands_finished,
        readback_ok: all_requested_bands_readback_ok,
        reason: if all_requested_bands_finished {
            "t30-fullscreen-band-redraw-finished"
        } else {
            "t30-fullscreen-band-redraw-stopped"
        },
        program_name: program.name,
        output_gpu: first_gpu,
        sentinel: first_expected,
        output_first_before: first_before,
        output_first_after: first_after,
        output_nonzero_before: (first_before != 0) as usize,
        output_nonzero_after: (first_after != 0) as usize,
        output_hits_lo64: readback_ok as u64,
        dispatch_delta: total_dispatch_delta,
        finish_marker,
        expected_finish_marker: RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        batch_bytes: total_batch_bytes,
    }
}

fn mandelbrot16_t30_xy_gradient_color(
    frame_seed: u32,
    width: u32,
    height: u32,
    pixel_x: u32,
    pixel_y: u32,
) -> u32 {
    let width_m1 = width.saturating_sub(1).max(1);
    let height_m1 = height.saturating_sub(1).max(1);
    let r = pixel_x.min(width_m1).saturating_mul(255) / width_m1;
    let g = pixel_y.min(height_m1).saturating_mul(255) / height_m1;
    let b = ((pixel_x >> 4) ^ pixel_y ^ frame_seed) & 0xFF;
    0xFF00_0000 | (r << 16) | (g << 8) | b
}

fn mandelbrot16_t30_address_gradient_color(pitch_bytes: u32, pixel_x: u32, pixel_y: u32) -> u32 {
    let offset = pixel_y
        .saturating_mul(pitch_bytes)
        .saturating_add(pixel_x.saturating_mul(core::mem::size_of::<u32>() as u32));
    0xFF00_0000 | offset
}

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_t30_immediate_lane0_sweep_bands(
    frame_seed: u32,
    first_row: u32,
    first_x: u32,
    rows_per_submit: u32,
    max_bands: u32,
    max_x_blocks: u32,
    lane_phases_per_block: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    let program = gpgpu_primary_scanout_mandelbrot16_simd16_bw_program();
    let Some(target) = crate::intel::display::primary_surface_gpgpu_marker_target() else {
        return gpgpu_one_tile_sentinel_failure("no-primary-scanout", program, 0);
    };
    let pixels_per_group =
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_PIXELS_PER_PROGRAM as u32;
    let pixels_per_stamp = pixels_per_group.saturating_mul(MANDELBROT16_T38_STAMP_REPEATS);
    let x_groups = target.width.saturating_add(pixels_per_stamp - 1) / pixels_per_stamp;
    let rows_per_submit = rows_per_submit.clamp(1, target.height.max(1));
    let total_bands = target
        .height
        .saturating_add(rows_per_submit.saturating_sub(1))
        / rows_per_submit;
    let requested_bands = max_bands.max(1).min(total_bands.max(1));
    let requested_x_blocks = max_x_blocks.max(1).min(x_groups.max(1));
    let _requested_lane_phases = lane_phases_per_block.max(1).min(pixels_per_group.max(1));
    let start_row = if target.height == 0 {
        0
    } else {
        first_row % target.height
    };
    let start_x_group = if x_groups == 0 {
        0
    } else {
        (first_x / pixels_per_stamp).min(x_groups.saturating_sub(1))
    };
    let mut y = start_row;
    let mut submitted = 0u32;
    let mut finished = 0u32;
    let mut readback_ok = 0u32;
    let mut attempted_bands = 0u32;
    let mut total_dispatch_delta = 0u64;
    let mut total_batch_bytes = 0usize;
    let mut finish_marker = 0u32;
    let mut first_before = 0u32;
    let mut first_after = 0u32;
    let mut first_expected = 0u32;
    let mut first_gpu = target.gpu;
    let mut first_seen = false;

    while attempted_bands < requested_bands {
        let rows = rows_per_submit.min(target.height.saturating_sub(y)).max(1);
        let mut x_count = 0u32;
        while x_count < requested_x_blocks {
            let x_group = if x_groups == 0 {
                0
            } else {
                (start_x_group.saturating_add(x_count)) % x_groups
            };
            let group_base_x = if x_count == 0 {
                first_x
                    .min(target.width.saturating_sub(1))
                    .saturating_sub(first_x % pixels_per_stamp.max(1))
            } else {
                x_group.saturating_mul(pixels_per_stamp)
            };
            let mut row_delta = 0u32;
            while row_delta < rows {
                let row_y = y.saturating_add(row_delta);
                let x_base = group_base_x;
                let pixel_color =
                    mandelbrot16_t30_address_gradient_color(target.pitch_bytes, x_base, row_y);
                let proof = submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe_impl(
                    MANDELBROT16_T39_MODE_IMMEDIATE_WIDE_STAMP_ADDRESS_COLOR,
                    row_y,
                    x_base,
                    1,
                    pixel_color,
                    0,
                    !first_seen,
                );
                submitted = submitted.saturating_add(proof.submitted as u32);
                finished = finished.saturating_add(proof.finished as u32);
                readback_ok = readback_ok.saturating_add(proof.readback_ok as u32);
                total_dispatch_delta = total_dispatch_delta.saturating_add(proof.dispatch_delta);
                total_batch_bytes = total_batch_bytes.saturating_add(proof.batch_bytes);
                finish_marker = proof.finish_marker;
                if !first_seen {
                    first_seen = true;
                    first_before = proof.output_first_before;
                    first_after = proof.output_first_after;
                    first_expected = proof.sentinel;
                    first_gpu = proof.output_gpu;
                }
                if !proof.finished {
                    break;
                }
                row_delta = row_delta.saturating_add(1);
            }
            if row_delta < rows {
                break;
            }
            x_count = x_count.saturating_add(1);
        }
        attempted_bands = attempted_bands.saturating_add(1);
        if x_count < requested_x_blocks {
            break;
        }
        y = y.saturating_add(rows);
        if y >= target.height {
            y = 0;
        }
    }

    let requested_blocks = (requested_bands as u64)
        .saturating_mul(requested_x_blocks as u64)
        .saturating_mul(rows_per_submit as u64);
    let all_requested_finished = requested_blocks != 0 && finished as u64 == requested_blocks;
    let first_sample_match = first_seen && first_after == first_expected;
    let notified_rows = rows_per_submit
        .saturating_mul(requested_bands)
        .min(target.height.max(1));
    let display_notified = all_requested_finished
        && crate::intel::display::notify_primary_surface_external_write(
            "gpgpu-primary-scanout-mandelbrot16-t30-t36-block-fill-row",
            (start_row as usize).saturating_mul(target.pitch_bytes as usize),
            (notified_rows as usize).saturating_mul(target.pitch_bytes as usize),
        );
    crate::log!(
        "intel/gpgpu: t30-mandelbrot16-immediate-t39-wide-stamp-address-color-sweep first_row={} first_x={} start_x_group={} next_row={} submitted_blocks={} finished_blocks={} requested_blocks={} requested_x_blocks={} stamp_repeats={} pixels_per_stamp={} sample_readback_ok={} display_notified={} total_frame_bands={} frame_seed=0x{:08X} target={}x{} pitch_bytes={} x_groups={} rows_per_submit={} requested_store_pixels={} lane_dispatch_delta={} finish_marker=0x{:08X} program_source={} artifact_body=t30-t39-immediate-wide-stamp-address-derived-color-block-fill color_mode=eu-address-derived-block-constant runtime_contract=draw-same-frame-again address_path=immediate-base-cpu-wide-stamp-row-sweep-color-from-g20 primary_gpu=0x{:X} sample_gpu=0x{:X} sample_before=0x{:08X} sample_after=0x{:08X} sample_expected=0x{:08X} sample_match={} validation=first-16px-chunk-exact-rest-command-retire next=raise-rows-per-heartbeat-or-prove-gpu-side-groupid-xy\n",
        start_row,
        first_x,
        start_x_group,
        y,
        submitted,
        finished,
        requested_blocks,
        requested_x_blocks,
        MANDELBROT16_T38_STAMP_REPEATS,
        pixels_per_stamp,
        first_sample_match as u8,
        display_notified as u8,
        total_bands,
        frame_seed,
        target.width,
        target.height,
        target.pitch_bytes,
        x_groups,
        rows_per_submit,
        (target.width as u64)
            .saturating_mul(rows_per_submit as u64)
            .saturating_mul(requested_bands as u64),
        total_dispatch_delta,
        finish_marker,
        program.name,
        target.gpu,
        first_gpu,
        first_before,
        first_after,
        first_expected,
        first_sample_match as u8,
    );

    crate::intel::GpgpuOneTileSentinelProof {
        submitted: submitted != 0,
        finished: all_requested_finished,
        readback_ok: first_sample_match,
        reason: if first_sample_match {
            "t30-immediate-t39-address-color-block-sweep-first-block-visible"
        } else if all_requested_finished {
            "t30-immediate-t39-address-color-block-sweep-finished-no-first-match"
        } else {
            "t30-immediate-t39-address-color-block-sweep-stopped"
        },
        program_name: program.name,
        output_gpu: first_gpu,
        sentinel: first_expected,
        output_first_before: first_before,
        output_first_after: first_after,
        output_nonzero_before: (first_before != 0) as usize,
        output_nonzero_after: (first_after != 0) as usize,
        output_hits_lo64: readback_ok as u64,
        dispatch_delta: total_dispatch_delta,
        finish_marker,
        expected_finish_marker: RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        batch_bytes: total_batch_bytes,
    }
}

fn encode_gfx12_gpgpu_mandelbrot16_immediate_rows_batch(
    warm: RenderWarmState,
    batch_dwords: &mut [u32],
    store_surface: GpgpuStoreSurfaceState,
    program: GpgpuEuProgram,
    _scanout_gpu: u64,
    target_pitch_bytes: usize,
    target_byte_len: usize,
    first_row_offset: usize,
    row_count: usize,
    completion_marker: u32,
) -> Result<usize, &'static str> {
    const WALKER_AND_MSF_DWORDS: usize = 17;
    const POST_WALKER_FLUSH_DWORDS: usize = 6;
    const IMMEDIATE_ADDRESS_BASE_DWORD: usize = 19;

    fn push(batch_dwords: &mut [u32], cursor: &mut usize, value: u32) -> Result<(), &'static str> {
        if *cursor >= batch_dwords.len() {
            return Err("mandelbrot16-immediate-row-batch-exhausted");
        }
        batch_dwords[*cursor] = value;
        *cursor += 1;
        Ok(())
    }

    fn push_store_imm32(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        dst: u64,
        value: u32,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, MI_STORE_DATA_IMM_GGTT_DW1)?;
        push(batch_dwords, cursor, dst as u32)?;
        push(batch_dwords, cursor, (dst >> 32) as u32)?;
        push(batch_dwords, cursor, value)
    }

    fn push_pipe_control(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        flags: u32,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, PIPE_CONTROL_CMD)?;
        push(batch_dwords, cursor, flags)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)
    }

    if row_count == 0 {
        return Err("mandelbrot16-immediate-row-batch-empty");
    }
    if first_row_offset >= target_byte_len {
        return Err("mandelbrot16-immediate-row-batch-outside-scanout");
    }

    let template_bytes =
        encode_gfx12_gpgpu_walker_probe_batch(warm, batch_dwords, store_surface, program, 1)?;
    let template_dwords = template_bytes / core::mem::size_of::<u32>();
    let mut walker_start = None;
    let mut index = 0usize;
    while index < template_dwords {
        if batch_dwords[index] == GPGPU_WALKER_IPEHR_LEN13 {
            walker_start = Some(index);
            break;
        }
        index += 1;
    }
    let walker_start = walker_start.ok_or("mandelbrot16-immediate-row-batch-no-walker")?;
    let post_walker_flush_start = walker_start.saturating_add(WALKER_AND_MSF_DWORDS);
    let marker_start = post_walker_flush_start.saturating_add(POST_WALKER_FLUSH_DWORDS);
    let template_end = marker_start.saturating_add(4).saturating_add(2);
    if template_end > template_dwords {
        return Err("mandelbrot16-immediate-row-batch-template-short");
    }
    if batch_dwords[marker_start] != MI_STORE_DATA_IMM_GGTT_DW1 {
        return Err("mandelbrot16-immediate-row-batch-template-marker");
    }

    let mut walker_and_msf = [0u32; WALKER_AND_MSF_DWORDS];
    let mut post_walker_flush = [0u32; POST_WALKER_FLUSH_DWORDS];
    let mut copy = 0usize;
    while copy < WALKER_AND_MSF_DWORDS {
        walker_and_msf[copy] = batch_dwords[walker_start + copy];
        copy += 1;
    }
    copy = 0;
    while copy < POST_WALKER_FLUSH_DWORDS {
        post_walker_flush[copy] = batch_dwords[post_walker_flush_start + copy];
        copy += 1;
    }

    let address_gpu = GPU_VA_DRAW_STATE_BASE
        + (GPGPU_EU_KERNEL_OFFSET_BYTES
            + IMMEDIATE_ADDRESS_BASE_DWORD * core::mem::size_of::<u32>()) as u64;
    let mut cursor = walker_start;
    let mut row = 0usize;
    while row < row_count {
        let row_offset = first_row_offset.saturating_add(row.saturating_mul(target_pitch_bytes));
        if row_offset >= target_byte_len {
            return Err("mandelbrot16-immediate-row-batch-row-outside-scanout");
        }
        if row_offset >> 32 != 0 {
            return Err("mandelbrot16-immediate-row-batch-offset-high32");
        }
        push_store_imm32(batch_dwords, &mut cursor, address_gpu, row_offset as u32)?;
        push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_INVALIDATE_BITS)?;
        copy = 0;
        while copy < WALKER_AND_MSF_DWORDS {
            push(batch_dwords, &mut cursor, walker_and_msf[copy])?;
            copy += 1;
        }
        copy = 0;
        while copy < POST_WALKER_FLUSH_DWORDS {
            push(batch_dwords, &mut cursor, post_walker_flush[copy])?;
            copy += 1;
        }
        row += 1;
    }

    let marker_gpu = GPU_VA_RESULT_BASE
        + (RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD as u64) * core::mem::size_of::<u32>() as u64;
    push_store_imm32(batch_dwords, &mut cursor, marker_gpu, completion_marker)?;
    push(batch_dwords, &mut cursor, MI_BATCH_BUFFER_END)?;
    push(batch_dwords, &mut cursor, MI_NOOP)?;
    Ok(cursor * core::mem::size_of::<u32>())
}

fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_immediate_rows_batched_impl(
    mode: u32,
    row_index: u32,
    x_base: u32,
    row_groups: u32,
    lhs: u32,
    rhs: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    const PIXELS: usize =
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_PIXELS_PER_PROGRAM;
    const BYTES: usize = PIXELS * core::mem::size_of::<u32>();
    let row_groups = row_groups.max(1);
    let expected_hw_lane_dispatch =
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_LANES as u64 * row_groups as u64;
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
    if !forcewake_render_acquire(warm) {
        return gpgpu_one_tile_sentinel_failure("forcewake", program, target.gpu);
    }
    if !ensure_gpgpu_warm_buffers_mapped(dev, warm) {
        return gpgpu_one_tile_sentinel_failure("ggtt-map", program, target.gpu);
    }

    let y = if target.height == 0 {
        0
    } else {
        row_index % target.height
    } as usize;
    let x = core::cmp::min(x_base as usize, (target.width as usize).saturating_sub(PIXELS));
    let row_offset = y
        .saturating_mul(target.pitch_bytes as usize)
        .saturating_add(x.saturating_mul(core::mem::size_of::<u32>()));
    let submit_span_bytes = (row_groups as usize)
        .saturating_sub(1)
        .saturating_mul(target.pitch_bytes as usize)
        .saturating_add(BYTES);
    if row_offset.saturating_add(submit_span_bytes) > target.byte_len {
        return gpgpu_one_tile_sentinel_failure(
            "mandelbrot16-immediate-row-batch-outside-scanout",
            program,
            target.gpu,
        );
    }
    let row_gpu = target.gpu + row_offset as u64;
    if row_gpu >> 32 != 0 {
        return gpgpu_one_tile_sentinel_failure(
            "mandelbrot16-immediate-row-batch-gpu-high32",
            program,
            row_gpu,
        );
    }
    let row_virt = unsafe { target.virt.add(row_offset) };
    let sample_before = unsafe { core::ptr::read_volatile(row_virt as *const u32) };
    let expected_first = mandelbrot16_simd16_probe_expected_first(mode, lhs, rhs);

    if !upload_primary_scanout_mandelbrot16_simd16_bw_artifact(
        warm,
        row_offset as u32,
        0,
        mode,
        lhs,
        rhs,
        Mandelbrot16AddressMode::ImmediateBase,
    ) {
        return gpgpu_one_tile_sentinel_failure(
            "mandelbrot16-immediate-row-batch-program-upload",
            program,
            row_gpu,
        );
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
        "stateless-primary-scanout-mandelbrot16-simd16-immediate-row-batch",
    );
    let batch_bytes = match encode_gfx12_gpgpu_mandelbrot16_immediate_rows_batch(
        warm,
        batch,
        store_surface,
        program,
        target.gpu,
        target.pitch_bytes as usize,
        target.byte_len,
        row_offset,
        row_groups as usize,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
    ) {
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
        "gpgpu-primary-scanout-mandelbrot16-simd16-immediate-row-batch",
    );
    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    crate::intel::dma_flush(row_virt, submit_span_bytes);
    let dispatch_after = read_gpgpu_threads_dispatched(dev);
    let dispatch_delta = dispatch_after.saturating_sub(dispatch_before);
    let finish_marker = read_result_dword(warm, RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD);
    let sample_after = unsafe { core::ptr::read_volatile(row_virt as *const u32) };
    let command_ok = finished
        && finish_marker == RCS_EXEC_RESULT_COMPUTE_WALKER_DONE
        && dispatch_delta >= expected_hw_lane_dispatch;
    let display_notified = command_ok
        && crate::intel::display::notify_primary_surface_external_write(
            "gpgpu-primary-scanout-mandelbrot16-simd16-immediate-row-batch",
            row_offset,
            submit_span_bytes,
        );
    if !command_ok || x == 0 {
        crate::log!(
            "intel/gpgpu: t21-mandelbrot16-immediate-row-batch submitted=1 finished={} readback_ok={} row_index={} x_base={} row_groups={} row_gpu=0x{:X} span_bytes=0x{:X} sample_before=0x{:08X} sample_after=0x{:08X} sample_expected=0x{:08X} sample_match={} display_notified={} lane_dispatch_delta={} expected_hw_lane_dispatch={} finish_marker=0x{:08X} batch_bytes=0x{:X} program_source={} address_path=immediate-base-mi-patched-per-row log_policy=first-x-block-or-failure proves=simd16-immediate-store-body-plus-multiwalker-row-coverage does_not_prove=groupid-row-address-prelude-or-smooth-coloring\n",
            finished as u8,
            command_ok as u8,
            y,
            x,
            row_groups,
            row_gpu,
            submit_span_bytes,
            sample_before,
            sample_after,
            expected_first,
            (sample_after == expected_first) as u8,
            display_notified as u8,
            dispatch_delta,
            expected_hw_lane_dispatch,
            finish_marker,
            batch_bytes,
            program.name,
        );
    }

    crate::intel::GpgpuOneTileSentinelProof {
        submitted: true,
        finished,
        readback_ok: command_ok,
        reason: if command_ok {
            "mandelbrot16-immediate-row-batch-retired"
        } else {
            "mandelbrot16-immediate-row-batch-not-retired"
        },
        program_name: program.name,
        output_gpu: row_gpu,
        sentinel: expected_first,
        output_first_before: sample_before,
        output_first_after: sample_after,
        output_nonzero_before: (sample_before != 0) as usize,
        output_nonzero_after: (sample_after != 0) as usize,
        output_hits_lo64: (sample_after == expected_first) as u64,
        dispatch_delta,
        finish_marker,
        expected_finish_marker: RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        batch_bytes,
    }
}

fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe_impl(
    mode: u32,
    row_index: u32,
    x_base: u32,
    row_groups: u32,
    lhs: u32,
    rhs: u32,
    validate_readback: bool,
) -> crate::intel::GpgpuOneTileSentinelProof {
    submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe_impl_with_notify(
        mode,
        row_index,
        x_base,
        row_groups,
        lhs,
        rhs,
        validate_readback,
        true,
    )
}

fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe_impl_with_notify(
    mode: u32,
    row_index: u32,
    x_base: u32,
    row_groups: u32,
    lhs: u32,
    rhs: u32,
    validate_readback: bool,
    notify_display: bool,
) -> crate::intel::GpgpuOneTileSentinelProof {
    const PIXELS: usize =
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_PIXELS_PER_PROGRAM;
    const BYTES: usize = PIXELS * core::mem::size_of::<u32>();
    let row_groups = row_groups.max(1);
    let address_mode = if mode == MANDELBROT16_T11_MODE_LINEAR_FULL_BAND
        || mode == MANDELBROT16_T15_MODE_LINEAR_GRADIENT_FULL_BAND
        || mode == MANDELBROT16_T16_MODE_LINEAR_CONSTANT_STORE
        || mode == MANDELBROT16_T30_MODE_FULLSCREEN_LINEAR_CONSTANT_STORE
        || mode == MANDELBROT16_T37_MODE_GROUPID_X_UNROLLED_SCALAR16
    {
        Mandelbrot16AddressMode::GroupIdLinear64
    } else if row_groups > 1 {
        Mandelbrot16AddressMode::GroupIdRowPitch
    } else {
        Mandelbrot16AddressMode::ImmediateBase
    };
    let expected_hw_lane_dispatch =
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_LANES as u64 * row_groups as u64;

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
    if !forcewake_render_acquire(warm) {
        return gpgpu_one_tile_sentinel_failure("forcewake", program, target.gpu);
    }
    if !ensure_gpgpu_warm_buffers_mapped(dev, warm) {
        return gpgpu_one_tile_sentinel_failure("ggtt-map", program, target.gpu);
    }

    let y = if target.height == 0 {
        0
    } else {
        row_index % target.height
    } as usize;
    let store_pixels_per_invocation = if mode == MANDELBROT16_T38_MODE_IMMEDIATE_WIDE_STAMP
        || mode == MANDELBROT16_T39_MODE_IMMEDIATE_WIDE_STAMP_ADDRESS_COLOR
    {
        PIXELS.saturating_mul(MANDELBROT16_T38_STAMP_REPEATS as usize)
    } else {
        PIXELS
    };
    let store_bytes_per_invocation =
        store_pixels_per_invocation.saturating_mul(core::mem::size_of::<u32>());
    let x = if address_mode == Mandelbrot16AddressMode::GroupIdLinear64 {
        x_base as usize
    } else {
        core::cmp::min(
            x_base as usize,
            (target.width as usize).saturating_sub(store_pixels_per_invocation),
        )
    };
    let row_offset = y
        .saturating_mul(target.pitch_bytes as usize)
        .saturating_add(x.saturating_mul(core::mem::size_of::<u32>()));
    let submit_span_bytes = if address_mode == Mandelbrot16AddressMode::GroupIdLinear64 {
        (row_groups as usize).saturating_mul(store_bytes_per_invocation)
    } else {
        (row_groups as usize)
            .saturating_sub(1)
            .saturating_mul(target.pitch_bytes as usize)
            .saturating_add(store_bytes_per_invocation)
    };
    if row_offset.saturating_add(submit_span_bytes) > target.byte_len {
        return gpgpu_one_tile_sentinel_failure(
            "mandelbrot16-outside-scanout",
            program,
            target.gpu,
        );
    }
    if row_offset >> 32 != 0 {
        return gpgpu_one_tile_sentinel_failure("mandelbrot16-offset-high32", program, target.gpu);
    }
    let row_gpu = target.gpu + row_offset as u64;
    if row_gpu >> 32 != 0 {
        return gpgpu_one_tile_sentinel_failure("mandelbrot16-gpu-high32", program, row_gpu);
    }
    let row_virt = unsafe { target.virt.add(row_offset) };
    let expected_first = if mode == MANDELBROT16_T34_MODE_IMMEDIATE_ADDRESS_DATA_WITNESS {
        lhs | row_offset as u32
    } else {
        mandelbrot16_simd16_probe_expected_first(mode, lhs, rhs)
    };
    let validate_all_lanes = mode == MANDELBROT16_T16_MODE_LINEAR_CONSTANT_STORE
        || mode == MANDELBROT16_T17_MODE_IMMEDIATE_CONSTANT_STORE
        || mode == MANDELBROT16_T30_MODE_FULLSCREEN_LINEAR_CONSTANT_STORE
        || mode == MANDELBROT16_T32_MODE_IMMEDIATE_CONSTANT_SINGLE_SEND
        || mode == MANDELBROT16_T33_MODE_IMMEDIATE_CONSTANT_BTI1_UNTYPED
        || mode == MANDELBROT16_T34_MODE_IMMEDIATE_ADDRESS_DATA_WITNESS
        || mode == MANDELBROT16_T35_MODE_IMMEDIATE_EXPLICIT_WIDE_PAYLOAD
        || mode == MANDELBROT16_T36_MODE_IMMEDIATE_UNROLLED_SCALAR16
        || mode == MANDELBROT16_T37_MODE_GROUPID_X_UNROLLED_SCALAR16
        || mode == MANDELBROT16_T38_MODE_IMMEDIATE_WIDE_STAMP
        || mode == MANDELBROT16_T39_MODE_IMMEDIATE_WIDE_STAMP_ADDRESS_COLOR;
    let expected_hit_mask = if validate_all_lanes {
        mandelbrot16_active_lane_mask() as u64
    } else {
        1
    };
    let validation_scope = if validate_all_lanes {
        "simd16-all-lanes-constant-store"
    } else {
        "first-kickoff-lane0-only"
    };
    let validation_lanes = if validate_all_lanes {
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_LANES
    } else {
        1
    };
    let poison = expected_first ^ 0x00A5_A5A5;
    let row_group_stride_bytes = if address_mode == Mandelbrot16AddressMode::GroupIdLinear64 {
        store_bytes_per_invocation
    } else {
        target.pitch_bytes as usize
    };
    let is_t37_groupid_x = mode == MANDELBROT16_T37_MODE_GROUPID_X_UNROLLED_SCALAR16;
    let expected_row_group_mask = if is_t37_groupid_x && row_groups < 64 {
        (1u64 << row_groups) - 1
    } else if is_t37_groupid_x {
        u64::MAX
    } else {
        0
    };
    if validate_readback {
        let mut group = 0usize;
        while group < row_groups as usize {
            let group_row_virt =
                unsafe { row_virt.add(group.saturating_mul(row_group_stride_bytes)) };
            let mut lane = 0usize;
            while lane < PIXELS {
                unsafe {
                    core::ptr::write_volatile(
                        group_row_virt.add(lane * core::mem::size_of::<u32>()) as *mut u32,
                        poison,
                    );
                }
                lane += 1;
            }
            group += 1;
        }
        crate::intel::dma_flush(row_virt, submit_span_bytes);
    }
    let mut before_words = [0u32; PIXELS];
    if validate_readback {
        let mut lane = 0usize;
        while lane < PIXELS {
            before_words[lane] = unsafe {
                core::ptr::read_volatile(
                    row_virt.add(lane * core::mem::size_of::<u32>()) as *const u32
                )
            };
            lane += 1;
        }
    }
    let output_first_before = before_words[0];
    let patched_color = 0;

    if !upload_primary_scanout_mandelbrot16_simd16_bw_artifact(
        warm,
        row_offset as u32,
        patched_color,
        mode,
        lhs,
        rhs,
        address_mode,
    ) {
        return gpgpu_one_tile_sentinel_failure("mandelbrot16-program-upload", program, row_gpu);
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
    let surface_note = if validate_readback {
        "stateless-primary-scanout-mandelbrot16-simd16-q12-plane"
    } else {
        "stateless-primary-scanout-mandelbrot16-simd16-q12-plane-quiet"
    };
    let store_surface = prepare_gpgpu_mandelbrot_store_surface_state_for_target_span(
        warm,
        target.gpu,
        target.byte_len,
        surface_note,
    );
    let batch_bytes = match encode_gfx12_gpgpu_walker_probe_batch(
        warm,
        batch,
        store_surface,
        program,
        row_groups,
    ) {
        Ok(bytes) => bytes,
        Err(reason) => return gpgpu_one_tile_sentinel_failure(reason, program, row_gpu),
    };
    crate::intel::dma_flush(warm.batch_virt, batch_bytes);

    let dispatch_before = read_gpgpu_threads_dispatched(dev);
    let submit_name = if validate_readback {
        "gpgpu-primary-scanout-mandelbrot16-simd16-q12-plane"
    } else {
        "gpgpu-primary-scanout-mandelbrot16-simd16-q12-plane-quiet"
    };
    let finished = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD,
        submit_name,
    );
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let readback_poll_limit = if validate_readback && finished {
        MANDELBROT_STRIP_READBACK_POLLS
    } else if validate_readback {
        1
    } else {
        0
    };
    let mut readback_poll = 0usize;
    let mut hits = 0u64;
    let mut changed = 0u64;
    let mut after_words = [0u32; PIXELS];
    let mut output_first_after = output_first_before;
    let mut row_group_hit_mask = 0u64;
    let mut row_group_changed_mask = 0u64;
    let mut row_group_first_after = [0u32; 8];
    while readback_poll < readback_poll_limit {
        crate::intel::dma_flush(row_virt, submit_span_bytes);
        hits = 0;
        changed = 0;
        row_group_hit_mask = 0;
        row_group_changed_mask = 0;
        let mut lane = 0usize;
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
            let expected_lane = if mode == MANDELBROT16_T34_MODE_IMMEDIATE_ADDRESS_DATA_WITNESS {
                lhs | (row_offset as u32).wrapping_add((lane as u32).wrapping_mul(4))
            } else {
                expected_first
            };
            if (validate_all_lanes || lane == 0) && after == expected_lane {
                hits |= 1u64 << lane;
            }
            if after != before_words[lane] {
                changed |= 1u64 << lane;
            }
            lane += 1;
        }
        if row_groups > 1 {
            let mut group = 0usize;
            while group < row_groups as usize {
                let group_row_virt =
                    unsafe { row_virt.add(group.saturating_mul(row_group_stride_bytes)) };
                let after = unsafe { core::ptr::read_volatile(group_row_virt as *const u32) };
                if group < row_group_first_after.len() {
                    row_group_first_after[group] = after;
                }
                if group < 64 && after == expected_first {
                    row_group_hit_mask |= 1u64 << group;
                }
                if group < 64 && after != poison {
                    row_group_changed_mask |= 1u64 << group;
                }
                group += 1;
            }
        }
        if hits & expected_hit_mask == expected_hit_mask
            && (expected_row_group_mask == 0
                || row_group_hit_mask & expected_row_group_mask == expected_row_group_mask)
        {
            break;
        }
        readback_poll += 1;
        core::hint::spin_loop();
    }

    let dispatch_after = read_gpgpu_threads_dispatched(dev);
    let dispatch_delta = dispatch_after.saturating_sub(dispatch_before);
    let finish_marker = read_result_dword(warm, RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD);
    let expected_hit = hits & expected_hit_mask == expected_hit_mask;
    let any_changed = changed != 0;
    let command_ok = finished
        && finish_marker == RCS_EXEC_RESULT_COMPUTE_WALKER_DONE
        && dispatch_delta >= expected_hw_lane_dispatch;
    let expected_row_groups_hit = expected_row_group_mask == 0
        || row_group_hit_mask & expected_row_group_mask == expected_row_group_mask;
    let readback_ok = if validate_readback {
        command_ok && expected_hit && any_changed && expected_row_groups_hit
    } else {
        command_ok
    };
    let display_notified = notify_display
        && command_ok
        && crate::intel::display::notify_primary_surface_external_write(
            "gpgpu-primary-scanout-mandelbrot16-simd16-q12-plane",
            row_offset,
            submit_span_bytes,
        );
    let is_one_iter = mode == 42 || mode == 43;
    let is_one_iter_visible = mode == 43;
    let is_t11_linear_band = mode == MANDELBROT16_T11_MODE_LINEAR_FULL_BAND;
    let is_t15_linear_gradient = mode == MANDELBROT16_T15_MODE_LINEAR_GRADIENT_FULL_BAND;
    let is_t16_linear_constant = mode == MANDELBROT16_T16_MODE_LINEAR_CONSTANT_STORE;
    let is_t17_immediate_constant = mode == MANDELBROT16_T17_MODE_IMMEDIATE_CONSTANT_STORE;
    let is_t30_fullscreen_linear_constant =
        mode == MANDELBROT16_T30_MODE_FULLSCREEN_LINEAR_CONSTANT_STORE;
    let is_t32_single_send = mode == MANDELBROT16_T32_MODE_IMMEDIATE_CONSTANT_SINGLE_SEND;
    let is_t33_bti1_untyped = mode == MANDELBROT16_T33_MODE_IMMEDIATE_CONSTANT_BTI1_UNTYPED;
    let is_t34_address_data = mode == MANDELBROT16_T34_MODE_IMMEDIATE_ADDRESS_DATA_WITNESS;
    let is_t35_explicit_wide_payload =
        mode == MANDELBROT16_T35_MODE_IMMEDIATE_EXPLICIT_WIDE_PAYLOAD;
    let is_t36_unrolled_scalar16 = mode == MANDELBROT16_T36_MODE_IMMEDIATE_UNROLLED_SCALAR16;
    let is_t37_groupid_x_unrolled_scalar16 = is_t37_groupid_x;
    let is_t38_wide_stamp = mode == MANDELBROT16_T38_MODE_IMMEDIATE_WIDE_STAMP;
    let is_t39_wide_stamp_address_color =
        mode == MANDELBROT16_T39_MODE_IMMEDIATE_WIDE_STAMP_ADDRESS_COLOR;
    let is_t18_immediate_gradient = mode == MANDELBROT16_T18_MODE_IMMEDIATE_GRADIENT_STORE;
    let is_t19_immediate_raw_radius = mode == MANDELBROT16_T19_MODE_IMMEDIATE_RAW_RADIUS_STORE;
    let is_fixed10_visible = mode == 44 || is_t11_linear_band || is_t15_linear_gradient;
    let is_fixed10_gradient = is_t15_linear_gradient || is_t18_immediate_gradient;
    let is_fixed10_visible =
        is_fixed10_visible || is_t18_immediate_gradient || is_t19_immediate_raw_radius;
    let is_fixed1_visible = mode == 45;
    let is_fixed_iter_visible = is_fixed10_visible || is_fixed1_visible;
    let reason = if !validate_readback && command_ok {
        if is_fixed_iter_visible {
            if is_fixed1_visible {
                "mandelbrot16-simd16-q12-fixed1-feedback-quiet-submit-finished-no-readback"
            } else {
                if is_t19_immediate_raw_radius {
                    "mandelbrot16-t19-immediate-base-fixed1-raw-radius-quiet-submit-finished-no-readback"
                } else if is_t18_immediate_gradient {
                    "mandelbrot16-t18-immediate-base-fixed10-escape-gradient-quiet-submit-finished-no-readback"
                } else if is_t15_linear_gradient {
                    "mandelbrot16-t15-linear-groupid-fixed10-escape-gradient-quiet-submit-finished-no-readback"
                } else if is_t11_linear_band {
                    "mandelbrot16-t11-linear-groupid-fixed10-escape-bw-quiet-submit-finished-no-readback"
                } else {
                    "mandelbrot16-simd16-q12-fixed10-escape-bw-quiet-submit-finished-no-readback"
                }
            }
        } else if is_t30_fullscreen_linear_constant {
            "mandelbrot16-t30-fullscreen-linear-constant-quiet-submit-finished-no-readback"
        } else if is_t32_single_send {
            "mandelbrot16-t32-immediate-single-send-constant-quiet-submit-finished-no-readback"
        } else if is_t33_bti1_untyped {
            "mandelbrot16-t33-immediate-bti1-untyped-constant-quiet-submit-finished-no-readback"
        } else if is_t34_address_data {
            "mandelbrot16-t34-immediate-address-data-witness-quiet-submit-finished-no-readback"
        } else if is_t35_explicit_wide_payload {
            "mandelbrot16-t35-immediate-explicit-wide-payload-quiet-submit-finished-no-readback"
        } else if is_t36_unrolled_scalar16 {
            "mandelbrot16-t36-immediate-unrolled-scalar16-quiet-submit-finished-no-readback"
        } else if is_t37_groupid_x_unrolled_scalar16 {
            "mandelbrot16-t37-groupid-x-unrolled-scalar16-quiet-submit-finished-no-readback"
        } else if is_t38_wide_stamp {
            "mandelbrot16-t38-immediate-wide-stamp-quiet-submit-finished-no-readback"
        } else if is_t39_wide_stamp_address_color {
            "mandelbrot16-t39-immediate-wide-stamp-address-color-quiet-submit-finished-no-readback"
        } else {
            "mandelbrot16-simd16-q12-onevis-quiet-submit-finished-no-readback"
        }
    } else if readback_ok && is_fixed_iter_visible {
        if is_fixed1_visible {
            "mandelbrot16-simd16-q12-fixed1-feedback-color-store-visible"
        } else {
            if is_t19_immediate_raw_radius {
                "mandelbrot16-t19-immediate-base-fixed1-raw-radius-color-store-visible"
            } else if is_t18_immediate_gradient {
                "mandelbrot16-t18-immediate-base-fixed10-escape-gradient-color-store-visible"
            } else if is_t15_linear_gradient {
                "mandelbrot16-t15-linear-groupid-fixed10-escape-gradient-color-store-visible"
            } else if is_t11_linear_band {
                "mandelbrot16-t11-linear-groupid-fixed10-escape-bw-color-store-visible"
            } else {
                "mandelbrot16-simd16-q12-fixed10-escape-bw-color-store-visible"
            }
        }
    } else if readback_ok && is_one_iter {
        if is_one_iter_visible {
            "mandelbrot16-simd16-q12-one-iteration-visible-color-store-visible"
        } else {
            "mandelbrot16-simd16-q12-one-iteration-real-store-visible"
        }
    } else if readback_ok && is_t16_linear_constant {
        "mandelbrot16-t16-linear-groupid-constant-color-store-visible"
    } else if readback_ok && is_t17_immediate_constant {
        "mandelbrot16-t17-immediate-base-constant-color-store-visible"
    } else if readback_ok && is_t30_fullscreen_linear_constant {
        "mandelbrot16-t30-fullscreen-linear-constant-color-store-visible"
    } else if readback_ok && is_t32_single_send {
        "mandelbrot16-t32-immediate-single-send-constant-color-store-visible"
    } else if readback_ok && is_t33_bti1_untyped {
        "mandelbrot16-t33-immediate-bti1-untyped-constant-color-store-visible"
    } else if readback_ok && is_t34_address_data {
        "mandelbrot16-t34-immediate-address-data-witness-color-store-visible"
    } else if readback_ok && is_t35_explicit_wide_payload {
        "mandelbrot16-t35-immediate-explicit-wide-payload-color-store-visible"
    } else if readback_ok && is_t36_unrolled_scalar16 {
        "mandelbrot16-t36-immediate-unrolled-scalar16-color-store-visible"
    } else if readback_ok && is_t37_groupid_x_unrolled_scalar16 {
        "mandelbrot16-t37-groupid-x-unrolled-scalar16-color-store-visible"
    } else if readback_ok && is_t38_wide_stamp {
        "mandelbrot16-t38-immediate-wide-stamp-color-store-visible"
    } else if readback_ok && is_t39_wide_stamp_address_color {
        "mandelbrot16-t39-immediate-wide-stamp-address-color-store-visible"
    } else if readback_ok {
        "mandelbrot16-simd16-alu-store-witness-visible"
    } else if !finished {
        "mandelbrot16-simd16-alu-store-witness-submit-not-finished"
    } else if dispatch_delta == 0 {
        "mandelbrot16-simd16-alu-store-witness-no-eu-dispatch"
    } else if hits == 0 {
        "mandelbrot16-simd16-alu-store-witness-first-lane-no-expected-value"
    } else {
        "mandelbrot16-simd16-alu-store-witness-first-lane-ok-readback-side-observation"
    };
    let proves = if !validate_readback && command_ok {
        if is_fixed_iter_visible {
            if is_fixed1_visible {
                "simd16-q12-fixed1-feedback-submit-eot-no-readback-visual-exercise"
            } else {
                if is_t19_immediate_raw_radius {
                    "t19-simd16-immediate-base-raw-radius-submit-eot-no-readback-visual-exercise"
                } else if is_t18_immediate_gradient {
                    "t18-simd16-immediate-base-gradient-submit-eot-no-readback-visual-exercise"
                } else if is_t15_linear_gradient {
                    "t15-simd16-linear-groupid-full-band-gradient-submit-eot-no-readback-visual-exercise"
                } else if is_t11_linear_band {
                    "t11-simd16-linear-groupid-full-band-submit-eot-no-readback-visual-exercise"
                } else {
                    "simd16-q12-fixed10-escape-bw-submit-eot-no-readback-visual-exercise"
                }
            }
        } else if is_t30_fullscreen_linear_constant {
            "t30-simd16-fullscreen-linear-groupid-constant-submit-eot-no-readback"
        } else if is_t32_single_send {
            "t32-simd16-immediate-single-send-constant-submit-eot-no-readback"
        } else if is_t33_bti1_untyped {
            "t33-simd16-immediate-bti1-untyped-constant-submit-eot-no-readback"
        } else if is_t34_address_data {
            "t34-simd16-immediate-address-data-witness-submit-eot-no-readback"
        } else if is_t35_explicit_wide_payload {
            "t35-simd16-immediate-explicit-wide-payload-submit-eot-no-readback"
        } else if is_t36_unrolled_scalar16 {
            "t36-simd16-immediate-unrolled-scalar16-submit-eot-no-readback"
        } else if is_t37_groupid_x_unrolled_scalar16 {
            "t37-simd16-groupid-x-unrolled-scalar16-submit-eot-no-readback"
        } else if is_t38_wide_stamp {
            "t38-simd16-immediate-wide-stamp-submit-eot-no-readback"
        } else if is_t39_wide_stamp_address_color {
            "t39-simd16-immediate-wide-stamp-address-color-submit-eot-no-readback"
        } else {
            "simd16-q12-onevis-submit-eot-no-readback-visual-exercise"
        }
    } else if readback_ok && is_fixed_iter_visible {
        if is_fixed1_visible {
            "simd16-q12-fixed1-feedback-store-eot-first-lane-validation-once"
        } else {
            if is_t19_immediate_raw_radius {
                "t19-simd16-immediate-base-fixed1-raw-radius-store-eot-first-lane-validation-once"
            } else if is_t18_immediate_gradient {
                "t18-simd16-immediate-base-fixed10-escape-gradient-store-eot-first-lane-validation-once"
            } else if is_t15_linear_gradient {
                "t15-simd16-linear-groupid-fixed10-escape-gradient-store-eot-first-lane-validation-once"
            } else if is_t11_linear_band {
                "t11-simd16-linear-groupid-fixed10-escape-bw-store-eot-first-lane-validation-once"
            } else {
                "simd16-q12-fixed10-escape-bw-store-eot-first-lane-validation-once"
            }
        }
    } else if readback_ok && is_one_iter_visible {
        "simd16-q12-one-iteration-visible-color-store-eot-first-lane-validation-once"
    } else if readback_ok && is_one_iter {
        "simd16-q12-one-iteration-real-store-eot-first-lane-validation-once"
    } else if readback_ok && is_t16_linear_constant {
        "t16-simd16-linear-groupid-constant-store-eot-first-lane-validation-once"
    } else if readback_ok && is_t17_immediate_constant {
        "t17-simd16-immediate-base-constant-store-eot-first-lane-validation-once"
    } else if readback_ok && is_t30_fullscreen_linear_constant {
        "t30-simd16-fullscreen-linear-groupid-constant-store-eot-first-lane-validation-once"
    } else if readback_ok && is_t32_single_send {
        "t32-simd16-immediate-single-send-constant-store-eot-all-lane-validation-once"
    } else if readback_ok && is_t33_bti1_untyped {
        "t33-simd16-immediate-bti1-untyped-constant-store-eot-all-lane-validation-once"
    } else if readback_ok && is_t34_address_data {
        "t34-simd16-immediate-address-data-witness-store-eot-all-lane-validation-once"
    } else if readback_ok && is_t35_explicit_wide_payload {
        "t35-simd16-immediate-explicit-wide-payload-store-eot-all-lane-validation-once"
    } else if readback_ok && is_t36_unrolled_scalar16 {
        "t36-simd16-immediate-unrolled-scalar16-store-eot-all-lane-validation-once"
    } else if readback_ok && is_t37_groupid_x_unrolled_scalar16 {
        "t37-simd16-groupid-x-unrolled-scalar16-store-eot-all-lane-and-block-validation-once"
    } else if readback_ok && is_t38_wide_stamp {
        "t38-simd16-immediate-wide-stamp-store-eot-first-chunk-validation-once"
    } else if readback_ok && is_t39_wide_stamp_address_color {
        "t39-simd16-immediate-wide-stamp-address-color-store-eot-first-chunk-validation-once"
    } else if readback_ok {
        "simd16-q12-or-alu-store-eot-first-lane-validation-once"
    } else if dispatch_delta >= expected_hw_lane_dispatch {
        "simd16-q12-or-alu-dispatch-plus-store-mismatch"
    } else if dispatch_delta != 0 {
        "partial-eu-dispatch"
    } else {
        "no-eu-dispatch"
    };
    let artifact_body = if is_fixed1_visible {
        "simd16-q12-fixed1-feedback-visible-color-store"
    } else if is_t16_linear_constant {
        "t16-simd16-linear-groupid-constant-visible-color-store"
    } else if is_t17_immediate_constant {
        "t17-simd16-immediate-base-constant-visible-color-store"
    } else if is_t30_fullscreen_linear_constant {
        "t30-simd16-fullscreen-linear-groupid-constant-visible-color-store"
    } else if is_t32_single_send {
        "t32-simd16-immediate-single-send-constant-visible-color-store"
    } else if is_t33_bti1_untyped {
        "t33-simd16-immediate-bti1-untyped-constant-visible-color-store"
    } else if is_t34_address_data {
        "t34-simd16-immediate-address-data-witness-visible-color-store"
    } else if is_t35_explicit_wide_payload {
        "t35-simd16-immediate-explicit-wide-payload-visible-color-store"
    } else if is_t36_unrolled_scalar16 {
        "t36-simd16-immediate-unrolled-scalar16-visible-color-store"
    } else if is_t37_groupid_x_unrolled_scalar16 {
        "t37-simd16-groupid-x-unrolled-scalar16-visible-color-store"
    } else if is_t38_wide_stamp {
        "t38-simd16-immediate-wide-stamp-visible-color-store"
    } else if is_t39_wide_stamp_address_color {
        "t39-simd16-immediate-wide-stamp-address-derived-visible-color-store"
    } else if is_fixed10_visible {
        if is_t19_immediate_raw_radius {
            "t19-simd16-immediate-base-fixed1-raw-radius-visible-color-store"
        } else if is_t18_immediate_gradient {
            "t18-simd16-immediate-base-fixed10-escape-gradient-visible-color-store"
        } else if is_t15_linear_gradient {
            "t15-simd16-linear-groupid-fixed10-escape-gradient-visible-color-store"
        } else if is_t11_linear_band {
            "t11-simd16-linear-groupid-fixed10-escape-bw-visible-color-store"
        } else {
            "simd16-q12-fixed10-escape-bw-visible-color-store"
        }
    } else if is_one_iter_visible {
        "simd16-q12-one-iteration-visible-color-store"
    } else if is_one_iter {
        "simd16-q12-one-iteration-real-store"
    } else {
        "simd16-q12-wide-mul-or-alu-store"
    };
    let eu_work = if is_fixed1_visible {
        "q12-z0-cre-cim-one-feedback-iteration-visible-store"
    } else if is_t16_linear_constant {
        "groupid-linear64-address-constant-visible-store"
    } else if is_t17_immediate_constant {
        "immediate-base-address-constant-visible-store"
    } else if is_t30_fullscreen_linear_constant {
        "fullscreen-linear-groupid-address-constant-visible-store"
    } else if is_t32_single_send {
        "immediate-base-address-constant-single-send-visible-store"
    } else if is_t33_bti1_untyped {
        "immediate-base-address-constant-bti1-untyped-visible-store"
    } else if is_t34_address_data {
        "immediate-base-address-derived-data-visible-store"
    } else if is_t35_explicit_wide_payload {
        "immediate-base-explicit-g21-g22-g23-payload-visible-store"
    } else if is_t36_unrolled_scalar16 {
        "immediate-base-unrolled-scalar16-visible-store"
    } else if is_t37_groupid_x_unrolled_scalar16 {
        "groupid-x-linear64-unrolled-scalar16-visible-store"
    } else if is_t38_wide_stamp {
        "immediate-base-wide-stamp-unrolled-scalar16-visible-store"
    } else if is_t39_wide_stamp_address_color {
        "immediate-base-wide-stamp-address-derived-color-visible-store"
    } else if is_fixed10_visible {
        if is_t19_immediate_raw_radius {
            "immediate-base-address-q12-fixed1-raw-radius-visible-store"
        } else if is_t18_immediate_gradient {
            "immediate-base-address-q12-fixed10-escape-gradient-visible-store"
        } else if is_t15_linear_gradient {
            "groupid-linear64-address-q12-fixed10-escape-gradient-visible-store"
        } else if is_t11_linear_band {
            "groupid-linear64-address-q12-fixed10-escape-bw-visible-store"
        } else {
            "q12-z0-cre-cim-fixed10-escape-bw-visible-store"
        }
    } else if is_one_iter_visible {
        "q12-cre-cim-re2-minus-im2-plus-cre-or-visible-mask-store"
    } else if is_one_iter {
        "q12-cre-cim-re2-minus-im2-plus-cre-store"
    } else {
        mandelbrot16_simd16_probe_variant_name(mode)
    };
    let one_iter_dword = if is_one_iter {
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_BODY_DWORD
    } else {
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_ONE_ITER_DWORD
    };
    let store_send_dword = if is_fixed_iter_visible {
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_FIXED10_STORE_SEND_DWORD
    } else if is_t16_linear_constant
        || is_t17_immediate_constant
        || is_t30_fullscreen_linear_constant
        || is_t32_single_send
        || is_t33_bti1_untyped
        || is_t34_address_data
        || is_t35_explicit_wide_payload
        || is_t36_unrolled_scalar16
        || is_t37_groupid_x_unrolled_scalar16
        || is_t38_wide_stamp
        || is_t39_wide_stamp_address_color
    {
        if is_t36_unrolled_scalar16 {
            trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_BODY_DWORD + 8
        } else if is_t35_explicit_wide_payload {
            trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_BODY_DWORD + 20
        } else if is_t32_single_send {
            trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_BODY_DWORD + 8
        } else {
            trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_BODY_DWORD + 8
        }
    } else if is_one_iter {
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_BODY_DWORD + 36
    } else {
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_STORE_SEND_DWORD
    };
    let address_contract = match address_mode {
        Mandelbrot16AddressMode::GroupIdLinear64 => {
            "groupid-linear-tile-times-64-plus-base-plus-laneid-g20"
        }
        Mandelbrot16AddressMode::GroupIdRowPitch => {
            "groupid-row-times-pitch-plus-base-plus-laneid-g20"
        }
        Mandelbrot16AddressMode::ImmediateBase => {
            "legacy-immediate-base-plus-laneid-g20-validation"
        }
    };

    if validate_readback {
        crate::log!(
            "intel/gpgpu: primary-scanout-mandelbrot16-simd16-q12-plane y={} x_base={} row_groups={} row_offset=0x{:X} row_gpu=0x{:X} target_gpu=0x{:X} target_phys=0x{:X} target_virt=0x{:X} pitch_bytes={} byte_len=0x{:X} q12_lhs=0x{:08X} q12_rhs=0x{:08X} patched_color=0x{:08X} expected_plane_value=0x{:08X} artifact_body={} payload_contract=mesa-send16-address-g20-data-g22-bti1 dispatch_contract=simd16-t10-groupid-row-walker-v1 eu_math_lanes_mask=0x{:04X} eu_store_lanes_mask=0x{:04X} cpu_patched_lanes_mask=0x0000 eu_color_lanes=0 cpu_color_dwords_patched=0 eu_address_alu={} eu_alu_variant={} eu_store_value=g22 validation_scope={} validation_lanes={} logical_lanes={} hdc_store_sends={} expected_store_pixels={} expected_hit_mask=0x{:04X} hit_mask=0x{:04X} changed_mask=0x{:04X} row_group_hit_mask=0x{:016X} row_group_changed_mask=0x{:016X} row_first8=[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X}] readback_ok={} reason={} first_before=0x{:08X} first_after=0x{:08X} expected_first=0x{:08X} after0=0x{:08X} after1=0x{:08X} after2=0x{:08X} after3=0x{:08X} after4=0x{:08X} after5=0x{:08X} after6=0x{:08X} after7=0x{:08X} after8=0x{:08X} after9=0x{:08X} after10=0x{:08X} after11=0x{:08X} after12=0x{:08X} after13=0x{:08X} after14=0x{:08X} after15=0x{:08X} display_notified={} notify_bytes=0x{:X} finish_marker=0x{:08X} lane_dispatch_delta={} expected_hw_lane_dispatch={} program_source={} address_base_dword={} color_dword={} one_iter_dword={} color_from_depth_dword={} store_send_dword={} proves={} next={} does_not_prove=full-frame-mandelbrot\n",
            y,
            x,
            row_groups,
            row_offset,
            row_gpu,
            target.gpu,
            target.phys,
            target.virt as usize,
            target.pitch_bytes,
            target.byte_len,
            lhs,
            rhs,
            patched_color,
            expected_first,
            artifact_body,
            mandelbrot16_active_lane_mask(),
            mandelbrot16_active_lane_mask(),
            address_contract,
            eu_work,
            validation_scope,
            validation_lanes,
            trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_LANES,
            trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_STORE_SENDS,
            PIXELS,
            expected_hit_mask,
            hits,
            changed,
            row_group_hit_mask,
            row_group_changed_mask,
            row_group_first_after[0],
            row_group_first_after[1],
            row_group_first_after[2],
            row_group_first_after[3],
            row_group_first_after[4],
            row_group_first_after[5],
            row_group_first_after[6],
            row_group_first_after[7],
            readback_ok as u8,
            reason,
            output_first_before,
            output_first_after,
            expected_first,
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
            submit_span_bytes,
            finish_marker,
            dispatch_delta,
            expected_hw_lane_dispatch,
            program.name,
            trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_ADDRESS_BASE_DWORD,
            trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_COLOR_DWORD,
            one_iter_dword,
            trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_COLOR_FROM_DEPTH_DWORD,
            store_send_dword,
            proves,
            if readback_ok && is_fixed_iter_visible {
                if is_fixed1_visible {
                    "expand-feedback-loop-to-fixed10"
                } else if is_t19_immediate_raw_radius {
                    "fix-gradient-compare-accumulator-or-add-count-gradient"
                } else if is_fixed10_gradient {
                    "increase-iteration-budget-or-refine-gradient"
                } else {
                    "increase-iteration-budget-or-add-count-gradient"
                }
            } else if readback_ok && is_one_iter {
                "add-z-imaginary-and-iteration-count-color"
            } else if readback_ok {
                "replace-witness-with-coordinate-and-iteration-body"
            } else if finished {
                "fix-simd16-q12-readback-or-store"
            } else {
                "fix-simd16-q12-submit-or-eot"
            },
        );
    }
    if !finished {
        recover_render_engine_after_nonretired_submit(
            dev,
            warm,
            "gpgpu-primary-scanout-mandelbrot16-simd16-q12-plane",
        );
    }
    crate::intel::GpgpuOneTileSentinelProof {
        submitted: batch_bytes != 0,
        finished,
        readback_ok,
        reason,
        program_name: program.name,
        output_gpu: row_gpu,
        sentinel: expected_first,
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
