use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static SEEN_BF16_MATVECS: AtomicU64 = AtomicU64::new(0);
static LOGGED_SHADOW_PLAN: AtomicBool = AtomicBool::new(false);
static LOGGED_PROMPT_SHADOW_PLAN: AtomicBool = AtomicBool::new(false);
static LOGGED_STATIC_TILE_PROOF: AtomicBool = AtomicBool::new(false);
static LOGGED_T4_WAITING: AtomicBool = AtomicBool::new(false);
static LOGGED_T4_LIVE_ROW_PROBE: AtomicBool = AtomicBool::new(false);
static LOGGED_PROMPT_LIVE_ROW_PROBE: AtomicBool = AtomicBool::new(false);

// T6.2/T6.3 are 8-lane row-block artifacts. The cap is coordination-only:
// each block is restaged into the same tile-record prefix, so raising this
// increases proved row coverage without changing artifact math.
const T62_ROW_BLOCK_DISPATCH_BLOCK_CAP: usize = 8;

// The T4/T5/T6/T6.1 single-row ladder is already proven.  Keep it available
// for bring-up, but the accepted-prefix path should spend its live prompt
// budget on the row-block artifacts that actually feed CPU suffix ownership.
const CGP_RERUN_UPFRONT_PROVEN_STAGES: bool = false;

#[derive(Copy, Clone, Debug)]
pub(crate) struct LocalGpuProofPlan {
    pub(crate) source_label: &'static str,
    pub(crate) call_index: u64,
    pub(crate) candidate: bool,
    pub(crate) static_tile_proven: bool,
    pub(crate) lane_dispatch_count: u32,
    pub(crate) expected_store_value: u32,
    pub(crate) observed_store_value: u32,
    pub(crate) program_name: &'static str,
}

pub(crate) fn observe_bf16_matvec_call(
    source_label: &'static str,
    n_rows: usize,
    k_dim: usize,
    chunk_rows: usize,
    chunks: usize,
) -> LocalGpuProofPlan {
    let call_index = SEEN_BF16_MATVECS.fetch_add(1, Ordering::AcqRel) + 1;
    let gpu = crate::intel::gpgpu_preflight_status();
    let shape_candidate =
        n_rows >= gpu.min_burn_rows && k_dim >= gpu.min_burn_k_dim && chunk_rows != 0;
    let static_tile_proven =
        gpu.eu_walker_retired && gpu.result_c_changed_by_eu && gpu.eu_dispatch_delta != 0;
    let candidate = shape_candidate && static_tile_proven && gpu.enough_for_shape;
    let plan = LocalGpuProofPlan {
        source_label,
        call_index,
        candidate,
        static_tile_proven,
        lane_dispatch_count: gpu.eu_dispatch_delta,
        expected_store_value: gpu.eu_expected_store_value,
        observed_store_value: gpu.eu_c_store_value,
        program_name: gpu.eu_program_name,
    };

    let log_global = !LOGGED_SHADOW_PLAN.swap(true, Ordering::AcqRel);
    let log_prompt =
        source_label == "lumen-prompt" && !LOGGED_PROMPT_SHADOW_PLAN.swap(true, Ordering::AcqRel);
    if log_global || log_prompt {
        crate::log!(
            "lumen-gpu-proof: director-step step=2 backend=local-gpu source={} mode=proof-only call={} rows={} k_dim={} chunk_rows={} chunks={} min_rows={} min_k_dim={} arena_ready={} shape_candidate={} candidate={} static_tile_proven={} program={} lane_dispatch={} expected=0x{:08X} observed=0x{:08X} output_owner=cpu-ap action=no-output-ownership next=one-live-row-proof-compare\n",
            source_label,
            call_index,
            n_rows,
            k_dim,
            chunk_rows,
            chunks,
            gpu.min_burn_rows,
            gpu.min_burn_k_dim,
            gpu.enough_for_shape as u8,
            shape_candidate as u8,
            candidate as u8,
            static_tile_proven as u8,
            gpu.eu_program_name,
            gpu.eu_dispatch_delta,
            gpu.eu_expected_store_value,
            gpu.eu_c_store_value,
        );
    }

    if static_tile_proven && !LOGGED_STATIC_TILE_PROOF.swap(true, Ordering::AcqRel) {
        crate::log!(
            "lumen-gpu-proof: director-step step=3 backend=local-gpu proof=static-dp4a-hdc-store-eot program={} lane_dispatch={} store_expected=0x{:08X} store_observed=0x{:08X} eot_retired=1 action=promote-to-live-row-proof next=bind-manifest-row-and-x-buffer does_not_prove=model_matvec\n",
            gpu.eu_program_name,
            gpu.eu_dispatch_delta,
            gpu.eu_expected_store_value,
            gpu.eu_c_store_value,
        );
    }

    plan
}

pub(crate) fn observe_live_bf16_matvec_probe(
    x: &[f32],
    w_rowmajor_bf16: &[u8],
    n_rows: usize,
    k_dim: usize,
    chunk_rows: usize,
    chunks: usize,
    plan: LocalGpuProofPlan,
) -> crate::lumen::cgp::CgpBf16PrefixContribution {
    if !plan.static_tile_proven {
        if !LOGGED_T4_WAITING.swap(true, Ordering::AcqRel) {
            crate::log!(
                "lumen-gpu-proof: director-step step=4 backend=local-gpu source={} mode=t4-live-row-probe ready=0 reason=static-gpu-artifact-not-proven-yet call={} program={} next=wait-for-static-dp4a-hdc-store-eot\n",
                plan.source_label,
                plan.call_index,
                plan.program_name
            );
        }
        return crate::lumen::cgp::CgpBf16PrefixContribution::none();
    }

    let log_global = !LOGGED_T4_LIVE_ROW_PROBE.swap(true, Ordering::AcqRel);
    let log_prompt = plan.source_label == "lumen-prompt"
        && !LOGGED_PROMPT_LIVE_ROW_PROBE.swap(true, Ordering::AcqRel);
    if !log_global && !log_prompt {
        return crate::lumen::cgp::CgpBf16PrefixContribution::none();
    }

    let Some(expected_w_len) = n_rows
        .checked_mul(k_dim)
        .and_then(|values| values.checked_mul(2))
    else {
        crate::log!(
            "lumen-gpu-proof: director-step step=4 backend=local-gpu source={} mode=t4-live-row-probe ready=0 reason=shape-overflow rows={} k_dim={}\n",
            plan.source_label,
            n_rows,
            k_dim
        );
        return crate::lumen::cgp::CgpBf16PrefixContribution::none();
    };
    if n_rows == 0 || k_dim == 0 || x.len() < k_dim || w_rowmajor_bf16.len() < expected_w_len {
        crate::log!(
            "lumen-gpu-proof: director-step step=4 backend=local-gpu source={} mode=t4-live-row-probe ready=0 reason=bad-shape rows={} k_dim={} x_len={} w_len={} expected_w_len={}\n",
            plan.source_label,
            n_rows,
            k_dim,
            x.len(),
            w_rowmajor_bf16.len(),
            expected_w_len
        );
        return crate::lumen::cgp::CgpBf16PrefixContribution::none();
    }

    let manifest = crate::lumen::lumen_net::resolve_bf16_matrix_probe(
        w_rowmajor_bf16.as_ptr() as usize,
        expected_w_len,
        n_rows,
        k_dim,
    );
    let row0 = bf16_row_dot(x, w_rowmajor_bf16, k_dim);
    let static4 = live_x_static4_probe(x, k_dim);
    let x_checksum = checksum_f32_prefix(x, k_dim);
    let row_checksum = checksum_bytes(&w_rowmajor_bf16[..k_dim.saturating_mul(2)]);
    let matrix_id = manifest.map(|entry| entry.matrix_id).unwrap_or(0);
    let matrix_epoch = manifest.map(|entry| entry.epoch).unwrap_or(0);
    let matrix_name_hash = manifest.map(|entry| entry.name_hash).unwrap_or(0);
    let matrix_name_len = manifest.map(|entry| entry.name_len).unwrap_or(0);
    let matrix_rows = manifest.map(|entry| entry.rows).unwrap_or(0);
    let matrix_k_dim = manifest.map(|entry| entry.k_dim).unwrap_or(0);
    let matrix_ptr = manifest.map(|entry| entry.data_ptr).unwrap_or(0);
    let matrix_bytes = manifest.map(|entry| entry.byte_len).unwrap_or(0);

    crate::log!(
        "lumen-gpu-proof: director-step step=4 backend=local-gpu source={} mode=t4-live-row-probe ready=1 call={} rows={} k_dim={} chunk_rows={} chunks={} manifest={} matrix=0x{:016X} matrix_epoch={} matrix_name_hash=0x{:016X} matrix_name_len={} matrix_rows={} matrix_k_dim={} matrix_ptr=0x{:X} matrix_bytes={} matrix_access=resident-read-only row=0 row_ptr=0x{:X} x_ptr=0x{:X} x_bytes={} x_checksum=0x{:016X} row_checksum=0x{:016X} static4_weights=01020304 static4_expected_bits=0x{:08X} row0_cpu_expected_bits=0x{:08X} gpu_submission=0 output_owner=cpu-ap next=stage-manifest-row-to-gpgpu-arena does_not_prove=gpu_live_load_or_model_matvec\n",
        plan.source_label,
        plan.call_index,
        n_rows,
        k_dim,
        chunk_rows,
        chunks,
        manifest.is_some() as u8,
        matrix_id,
        matrix_epoch,
        matrix_name_hash,
        matrix_name_len,
        matrix_rows,
        matrix_k_dim,
        matrix_ptr,
        matrix_bytes,
        w_rowmajor_bf16.as_ptr() as usize,
        x.as_ptr() as usize,
        k_dim.saturating_mul(core::mem::size_of::<f32>()),
        x_checksum,
        row_checksum,
        static4.to_bits(),
        row0.to_bits()
    );

    let gpu = crate::intel::gpgpu_preflight_status();
    let proof_tile_rows = gpu.tile_rows.max(1);
    let armed_tiles = n_rows.div_ceil(proof_tile_rows).min(gpu.max_tiles);
    if armed_tiles == 0 {
        crate::log!(
            "lumen-gpu-proof: director-step step=5 backend=local-gpu mode=gpgpu-actual-work-tiles ready=0 reason=no-armed-tiles rows={} tile_rows={} arena_max_tiles={} action=hold-scale does_not_prove=full_model_matvec\n",
            n_rows,
            proof_tile_rows,
            gpu.max_tiles,
        );
        return crate::lumen::cgp::CgpBf16PrefixContribution::none();
    }

    let mut staged_tiles = 0usize;
    let mut t5_submitted_tiles = 0usize;
    let mut t5_finished_tiles = 0usize;
    let mut t5_compare_ok_tiles = 0usize;
    let mut t6_submitted_tiles = 0usize;
    let mut t6_finished_tiles = 0usize;
    let mut t6_compare_ok_tiles = 0usize;
    let mut t61_submitted_tiles = 0usize;
    let mut t61_finished_tiles = 0usize;
    let mut t61_compare_ok_tiles = 0usize;
    let mut t62_staged_blocks = 0usize;
    let mut t62_submitted_blocks = 0usize;
    let mut t62_finished_blocks = 0usize;
    let mut t62_compare_ok_blocks = 0usize;
    let mut t62_compared_rows = 0usize;
    let mut t63_submitted_blocks = 0usize;
    let mut t63_finished_blocks = 0usize;
    let mut t63_compare_ok_blocks = 0usize;
    let mut t63_compared_rows = 0usize;
    let mut t64_window_staged_blocks = 0usize;
    let mut t64_submitted_blocks = 0usize;
    let mut t64_finished_blocks = 0usize;
    let mut t64_compare_ok_blocks = 0usize;
    let mut t64_compared_rows = 0usize;
    let mut t65_window_staged_blocks = 0usize;
    let mut t65_submitted_blocks = 0usize;
    let mut t65_finished_blocks = 0usize;
    let mut t65_compare_ok_blocks = 0usize;
    let mut t65_compared_rows = 0usize;
    let mut last_row = 0usize;
    let mut last_gpu_value = 0u32;
    let mut last_cpu_expected_bits = 0u32;
    let mut last_dispatch = 0u64;
    let mut last_partial_rows = 0usize;
    let mut cgp_prefix = crate::lumen::cgp::CgpBf16PrefixContribution::accepted_prefix(
        k_dim.min(trueos_eu::gfx12::T65_WINDOWED_ACCUM16_LIVE64_LIVE_K),
    );

    for tile_index in 0..armed_tiles {
        let row = tile_index.saturating_mul(proof_tile_rows);
        if row >= n_rows {
            break;
        }
        last_row = row;
        let Some(row_offset) = row
            .checked_mul(k_dim)
            .and_then(|values| values.checked_mul(2))
        else {
            crate::log!(
                "lumen-gpu-proof: director-step step=5 backend=local-gpu mode=gpgpu-actual-work-tile ready=0 tile_index={} row={} reason=row-offset-overflow\n",
                tile_index,
                row,
            );
            break;
        };
        let row_bytes = k_dim.saturating_mul(2);
        let row_end = row_offset.saturating_add(row_bytes);
        if row_end > w_rowmajor_bf16.len() {
            crate::log!(
                "lumen-gpu-proof: director-step step=5 backend=local-gpu mode=gpgpu-actual-work-tile ready=0 tile_index={} row={} reason=row-outside-matrix row_end={} w_len={}\n",
                tile_index,
                row,
                row_end,
                w_rowmajor_bf16.len(),
            );
            break;
        }

        let row_bf16 = &w_rowmajor_bf16[row_offset..row_end];
        let row_checksum = checksum_bytes(row_bf16);
        let full_row_expected = bf16_row_dot(x, row_bf16, k_dim);
        let stage = crate::intel::stage_gpgpu_one_tile_record_probe(
            x,
            row_bf16,
            k_dim,
            row,
            x_checksum,
            row_checksum,
            full_row_expected.to_bits(),
        );
        staged_tiles += stage.readback_ok as usize;
        crate::log!(
            "lumen-gpu-proof: director-step step=5 backend=local-gpu mode=gpgpu-actual-work-tile-stage staged={} reason={} call={} manifest={} tile_index={} armed_tiles={} row={} tile_rows={} k_dim={} artifact_addressing=tile-record-output-slots x_gpu=0x{:X} row_gpu=0x{:X} output_gpu=0x{:X} x_bytes={} row_bytes={} output_bytes={} cpu_expected_bits=0x{:08X} gpu_submission=0 output_owner=cpu-ap next=t5-live4-then-t6-live8 does_not_prove=full_model_matvec\n",
            stage.staged as u8,
            stage.reason,
            plan.call_index,
            manifest.is_some() as u8,
            tile_index,
            armed_tiles,
            row,
            stage.tile_rows,
            stage.k_dim,
            stage.x_gpu,
            stage.row_gpu,
            stage.output_gpu,
            stage.x_bytes,
            stage.row_bytes,
            stage.output_bytes,
            full_row_expected.to_bits(),
        );
        crate::log!(
            "lumen-gpu-proof: director-step step=6 backend=local-gpu mode=gpgpu-actual-work-tile-readback readback_ok={} staged={} tile_index={} row={} output_zeroed={} output_first_bits=0x{:08X} output_nonzero_dwords={} output_expected_hits_lo64=0x{:016X} output_checksum=0x{:016X} cpu_expected_bits=0x{:08X} gpu_submission=0 output_owner=cpu-ap action=hold-scale next=t5-live4-then-t6-live8 does_not_prove=gpu_output_or_model_matvec\n",
            stage.readback_ok as u8,
            stage.staged as u8,
            tile_index,
            row,
            stage.output_zeroed as u8,
            stage.output_first_bits,
            stage.output_nonzero_dwords,
            stage.output_expected_hits_lo64,
            stage.output_checksum,
            full_row_expected.to_bits(),
        );
        if !stage.readback_ok {
            continue;
        }

        if CGP_RERUN_UPFRONT_PROVEN_STAGES && tile_index == 0 {
            let sentinel = crate::intel::submit_gpgpu_one_tile_output_sentinel_probe(
                stage.output_gpu,
                stage.output_bytes,
                full_row_expected.to_bits(),
            );
            crate::log!(
                "lumen-gpu-proof: director-step step=7 backend=local-gpu mode=one-worker-output-sentinel submitted={} finished={} readback_ok={} reason={} program={} output_gpu=0x{:X} sentinel=0x{:08X} output_first_before=0x{:08X} output_first_after=0x{:08X} output_nonzero_before={} output_nonzero_after={} output_hits_lo64=0x{:016X} lane_dispatch={} finish_marker=0x{:08X} finish_expected=0x{:08X} batch_bytes=0x{:X} cpu_expected_bits=0x{:08X} output_owner=cpu-ap action=hold-scale next=one-tile-output-compare does_not_prove=model_matvec\n",
                sentinel.submitted as u8,
                sentinel.finished as u8,
                sentinel.readback_ok as u8,
                sentinel.reason,
                sentinel.program_name,
                sentinel.output_gpu,
                sentinel.sentinel,
                sentinel.output_first_before,
                sentinel.output_first_after,
                sentinel.output_nonzero_before,
                sentinel.output_nonzero_after,
                sentinel.output_hits_lo64,
                sentinel.dispatch_delta,
                sentinel.finish_marker,
                sentinel.expected_finish_marker,
                sentinel.batch_bytes,
                full_row_expected.to_bits(),
            );
            if !sentinel.readback_ok {
                continue;
            }

            let compare = crate::intel::submit_gpgpu_one_tile_output_compare_probe(
                stage.output_gpu,
                stage.output_bytes,
                full_row_expected.to_bits(),
            );
            crate::log!(
                "lumen-gpu-proof: director-step step=8 backend=local-gpu mode=one-worker-output-compare submitted={} finished={} readback_ok={} compare_ok={} reason={} program={} output_gpu=0x{:X} gpu_value=0x{:08X} cpu_expected_bits=0x{:08X} output_first_before=0x{:08X} output_first_after=0x{:08X} output_hits_lo64=0x{:016X} lane_dispatch={} finish_marker=0x{:08X} finish_expected=0x{:08X} batch_bytes=0x{:X} output_owner=cpu-ap action=hold-scale next=t5-live4-packed-bf16-dot does_not_prove=model_matvec_or_gpu_live_load\n",
                compare.submitted as u8,
                compare.finished as u8,
                compare.readback_ok as u8,
                compare.compare_ok as u8,
                compare.reason,
                compare.program_name,
                compare.output_gpu,
                compare.gpu_value,
                compare.cpu_expected_bits,
                compare.output_first_before,
                compare.output_first_after,
                compare.output_hits_lo64,
                compare.dispatch_delta,
                compare.finish_marker,
                compare.expected_finish_marker,
                compare.batch_bytes,
            );
            if !compare.readback_ok {
                continue;
            }
        }

        if CGP_RERUN_UPFRONT_PROVEN_STAGES {
            let t5_live_k_dim = k_dim.min(trueos_eu::gfx12::T5_ONE_ROW_MATVEC_LIVE_K);
            let t5_expected = bf16_row_dot_prefix(x, row_bf16, t5_live_k_dim);
            let t5 = crate::intel::submit_gpgpu_t5_one_row_matvec_probe(
                stage.output_gpu,
                stage.output_bytes,
                t5_expected.to_bits(),
                t5_live_k_dim,
            );
            t5_submitted_tiles += t5.submitted as usize;
            t5_finished_tiles += t5.finished as usize;
            t5_compare_ok_tiles += t5.compare_ok as usize;
            last_gpu_value = t5.gpu_value;
            last_cpu_expected_bits = t5.cpu_expected_bits;
            last_dispatch = t5.dispatch_delta;
            crate::log!(
                "lumen-gpu-proof: director-step step=9 backend=local-gpu mode=t5-small-live4-bf16-dot submitted={} finished={} readback_ok={} compare_ok={} reason={} program={} tile_index={} armed_tiles={} row={} output_gpu=0x{:X} gpu_value=0x{:08X} cpu_expected_bits=0x{:08X} full_row_cpu_bits=0x{:08X} output_first_before=0x{:08X} output_first_after=0x{:08X} output_hits_lo64=0x{:016X} lane_dispatch={} live_k_dim={} requires_live_gpu_load={} finish_marker=0x{:08X} finish_expected=0x{:08X} batch_bytes=0x{:X} output_owner=cpu-ap action=hold-scale next=scale-live-k-or-row-count does_not_prove=full_model_matvec\n",
                t5.submitted as u8,
                t5.finished as u8,
                t5.readback_ok as u8,
                t5.compare_ok as u8,
                t5.reason,
                t5.program_name,
                tile_index,
                armed_tiles,
                row,
                t5.output_gpu,
                t5.gpu_value,
                t5.cpu_expected_bits,
                full_row_expected.to_bits(),
                t5.output_first_before,
                t5.output_first_after,
                t5.output_hits_lo64,
                t5.dispatch_delta,
                t5.live_k_dim,
                t5.requires_live_gpu_load as u8,
                t5.finish_marker,
                t5.expected_finish_marker,
                t5.batch_bytes,
            );
            if !t5.compare_ok {
                continue;
            }

            let t6_live_k_dim = k_dim.min(trueos_eu::gfx12::T6_ONE_ROW_MATVEC_LIVE_K);
            let t6_expected = bf16_row_dot_prefix(x, row_bf16, t6_live_k_dim);
            let t6 = crate::intel::submit_gpgpu_t6_one_row_matvec_probe(
                stage.output_gpu,
                stage.output_bytes,
                t6_expected.to_bits(),
                t6_live_k_dim,
            );
            t6_submitted_tiles += t6.submitted as usize;
            t6_finished_tiles += t6.finished as usize;
            t6_compare_ok_tiles += t6.compare_ok as usize;
            last_gpu_value = t6.gpu_value;
            last_cpu_expected_bits = t6.cpu_expected_bits;
            last_dispatch = t6.dispatch_delta;
            crate::log!(
                "lumen-gpu-proof: director-step step=10 backend=local-gpu mode=t6-small-live8-bf16-dot submitted={} finished={} readback_ok={} compare_ok={} reason={} program={} tile_index={} armed_tiles={} row={} output_gpu=0x{:X} gpu_value=0x{:08X} cpu_expected_bits=0x{:08X} full_row_cpu_bits=0x{:08X} t5_gpu_value=0x{:08X} t5_cpu_expected_bits=0x{:08X} output_first_before=0x{:08X} output_first_after=0x{:08X} output_hits_lo64=0x{:016X} lane_dispatch={} live_k_dim={} requires_live_gpu_load={} finish_marker=0x{:08X} finish_expected=0x{:08X} batch_bytes=0x{:X} output_owner=cpu-ap action=hold-scale next=t6-1-live16-bf16-dot does_not_prove=full_model_matvec\n",
                t6.submitted as u8,
                t6.finished as u8,
                t6.readback_ok as u8,
                t6.compare_ok as u8,
                t6.reason,
                t6.program_name,
                tile_index,
                armed_tiles,
                row,
                t6.output_gpu,
                t6.gpu_value,
                t6.cpu_expected_bits,
                full_row_expected.to_bits(),
                t5.gpu_value,
                t5.cpu_expected_bits,
                t6.output_first_before,
                t6.output_first_after,
                t6.output_hits_lo64,
                t6.dispatch_delta,
                t6.live_k_dim,
                t6.requires_live_gpu_load as u8,
                t6.finish_marker,
                t6.expected_finish_marker,
                t6.batch_bytes,
            );
            if !t6.compare_ok {
                continue;
            }

            let t61_live_k_dim = k_dim.min(trueos_eu::gfx12::T61_ONE_ROW_MATVEC_LIVE_K);
            let t61_expected = bf16_row_dot_prefix(x, row_bf16, t61_live_k_dim);
            let t61 = crate::intel::submit_gpgpu_t61_one_row_matvec_probe(
                stage.output_gpu,
                stage.output_bytes,
                t61_expected.to_bits(),
                t61_live_k_dim,
            );
            t61_submitted_tiles += t61.submitted as usize;
            t61_finished_tiles += t61.finished as usize;
            t61_compare_ok_tiles += t61.compare_ok as usize;
            last_gpu_value = t61.gpu_value;
            last_cpu_expected_bits = t61.cpu_expected_bits;
            last_dispatch = t61.dispatch_delta;
            crate::log!(
                "lumen-gpu-proof: director-step step=11 backend=local-gpu mode=t6-1-live16-bf16-dot submitted={} finished={} readback_ok={} compare_ok={} reason={} program={} tile_index={} armed_tiles={} row={} output_gpu=0x{:X} gpu_value=0x{:08X} cpu_expected_bits=0x{:08X} full_row_cpu_bits=0x{:08X} t6_gpu_value=0x{:08X} t6_cpu_expected_bits=0x{:08X} output_first_before=0x{:08X} output_first_after=0x{:08X} output_hits_lo64=0x{:016X} lane_dispatch={} live_k_dim={} requires_live_gpu_load={} finish_marker=0x{:08X} finish_expected=0x{:08X} batch_bytes=0x{:X} output_owner=cpu-ap action=hold-scale next=t6-2-lane-indexed-live16-partial does_not_prove=full_model_matvec\n",
                t61.submitted as u8,
                t61.finished as u8,
                t61.readback_ok as u8,
                t61.compare_ok as u8,
                t61.reason,
                t61.program_name,
                tile_index,
                armed_tiles,
                row,
                t61.output_gpu,
                t61.gpu_value,
                t61.cpu_expected_bits,
                full_row_expected.to_bits(),
                t6.gpu_value,
                t6.cpu_expected_bits,
                t61.output_first_before,
                t61.output_first_after,
                t61.output_hits_lo64,
                t61.dispatch_delta,
                t61.live_k_dim,
                t61.requires_live_gpu_load as u8,
                t61.finish_marker,
                t61.expected_finish_marker,
                t61.batch_bytes,
            );
            if !t61.compare_ok {
                continue;
            }
        }

        let t62_block_rows = trueos_eu::gfx12::T62_ROW_INDEXED_PARTIAL_ROWS
            .min(proof_tile_rows)
            .min(n_rows.saturating_sub(row));
        let t62_live_k_dim = k_dim.min(trueos_eu::gfx12::T62_ROW_INDEXED_LIVE_K);
        if t62_block_rows == 0 {
            continue;
        }

        let tile_remaining_rows = proof_tile_rows.min(n_rows.saturating_sub(row));
        let t62_target_rows = tile_remaining_rows
            .min(t62_block_rows.saturating_mul(T62_ROW_BLOCK_DISPATCH_BLOCK_CAP));
        let t62_block_count = t62_target_rows.div_ceil(t62_block_rows);
        crate::log!(
            "lumen-gpu-proof: director-step step=12 backend=local-gpu mode=t6-2-row-block-plan tile_index={} armed_tiles={} tile_row_start={} block_rows={} block_cap={} planned_blocks={} planned_rows={} live_k_dim={} scheme=restage-row-prefix-per-block artifact_uses=gl_LocalInvocationID.x avoids=gl_WorkGroupID.x output_owner=cpu-ap next=t6-2-row-block-dispatch does_not_prove=full_model_matvec\n",
            tile_index,
            armed_tiles,
            row,
            t62_block_rows,
            T62_ROW_BLOCK_DISPATCH_BLOCK_CAP,
            t62_block_count,
            t62_target_rows,
            t62_live_k_dim,
        );

        for row_block_index in 0..t62_block_count {
            let block_tile_row = row_block_index.saturating_mul(t62_block_rows);
            let global_row = row.saturating_add(block_tile_row);
            let block_row_count =
                t62_block_rows.min(t62_target_rows.saturating_sub(block_tile_row));
            let block_row_offset =
                row_offset.saturating_add(block_tile_row.saturating_mul(k_dim).saturating_mul(2));
            let block_rows_bytes = block_row_count.saturating_mul(k_dim).saturating_mul(2);
            let block_rows_end = block_row_offset.saturating_add(block_rows_bytes);
            if block_row_count == 0 || block_rows_end > w_rowmajor_bf16.len() {
                continue;
            }

            let t62_rows = &w_rowmajor_bf16[block_row_offset..block_rows_end];
            let t62_rows_checksum = checksum_bytes(t62_rows);
            let t62_stage = crate::intel::stage_gpgpu_tile_record_rows_probe(
                stage.output_gpu,
                t62_rows,
                block_row_count,
                k_dim,
                t62_rows_checksum,
            );
            t62_staged_blocks += t62_stage.readback_ok as usize;
            crate::log!(
                "lumen-gpu-proof: director-step step=13 backend=local-gpu mode=t6-2-row-block-stage staged={} reason={} tile_index={} armed_tiles={} row_block={} global_row_start={} tile_row_start={} row_count={} live_k_dim={} output_gpu=0x{:X} rows_checksum=0x{:016X} staged_rows_checksum=0x{:016X} row_bytes={} output_zeroed={} output_nonzero_dwords={} gpu_submission=0 output_owner=cpu-ap next=t6-2-lane-indexed-live16-partial does_not_prove=full_model_matvec\n",
                t62_stage.staged as u8,
                t62_stage.reason,
                tile_index,
                armed_tiles,
                row_block_index,
                global_row,
                block_tile_row,
                t62_stage.row_count,
                t62_live_k_dim,
                t62_stage.output_gpu,
                t62_stage.rows_checksum,
                t62_stage.staged_rows_checksum,
                t62_stage.row_bytes,
                t62_stage.output_zeroed as u8,
                t62_stage.output_nonzero_dwords,
            );
            if !t62_stage.readback_ok {
                continue;
            }

            let mut t62_expected_words = [0u32; 8];
            for local_row in 0..block_row_count {
                let row_start = local_row.saturating_mul(k_dim).saturating_mul(2);
                let row_end = row_start.saturating_add(k_dim.saturating_mul(2));
                t62_expected_words[local_row] =
                    bf16_row_dot_prefix(x, &t62_rows[row_start..row_end], t62_live_k_dim).to_bits();
            }
            let t62 = crate::intel::submit_gpgpu_t62_partial_matvec_probe(
                stage.output_gpu,
                stage.output_bytes,
                t62_expected_words,
                block_row_count,
                t62_live_k_dim,
            );
            t62_submitted_blocks += t62.submitted as usize;
            t62_finished_blocks += t62.finished as usize;
            t62_compare_ok_blocks += t62.compare_ok as usize;
            t62_compared_rows =
                t62_compared_rows.saturating_add(if t62.compare_ok { block_row_count } else { 0 });
            last_gpu_value = t62.output_words[0];
            last_cpu_expected_bits = t62.expected_words[0];
            last_dispatch = t62.dispatch_delta;
            last_partial_rows = t62_compared_rows;
            crate::log!(
                "lumen-gpu-proof: director-step step=14 backend=local-gpu mode=t6-2-row-block-live16-partial submitted={} finished={} readback_ok={} compare_ok={} reason={} program={} tile_index={} armed_tiles={} row_block={} global_row_start={} tile_row_start={} row_count={} output_gpu=0x{:X} compare_mask=0x{:08X} expected_mask=0x{:08X} first_gpu=0x{:08X} first_cpu_expected=0x{:08X} last_gpu=0x{:08X} last_cpu_expected=0x{:08X} lane_dispatch={} live_k_dim={} finish_marker=0x{:08X} finish_expected=0x{:08X} batch_bytes=0x{:X} output_owner=cpu-ap action=hold-scale next=t6-3-accum16-hi-live32-partial does_not_prove=full_model_matvec\n",
                t62.submitted as u8,
                t62.finished as u8,
                t62.readback_ok as u8,
                t62.compare_ok as u8,
                t62.reason,
                t62.program_name,
                tile_index,
                armed_tiles,
                row_block_index,
                global_row,
                block_tile_row,
                t62.row_count,
                t62.output_gpu,
                t62.compare_mask,
                t62.expected_mask,
                t62.output_words[0],
                t62.expected_words[0],
                t62.output_words[t62.row_count.saturating_sub(1)],
                t62.expected_words[t62.row_count.saturating_sub(1)],
                t62.dispatch_delta,
                t62.live_k_dim,
                t62.finish_marker,
                t62.expected_finish_marker,
                t62.batch_bytes,
            );
            if !t62.compare_ok {
                continue;
            }

            let t63_restore_live_k_dim =
                k_dim.min(trueos_eu::gfx12::T63_ACCUM16_HI_LIVE32_LIVE_K);
            let t63_restore = crate::intel::stage_gpgpu_tile_record_accum16_window_probe(
                stage.output_gpu,
                x,
                t62_rows,
                block_row_count,
                k_dim,
                trueos_eu::gfx12::T62_ROW_INDEXED_LIVE_K,
            );
            crate::log!(
                "lumen-gpu-proof: director-step step=14 backend=local-gpu mode=t6-3-accum16-hi-live32-window-restore staged={} reason={} program={} tile_index={} armed_tiles={} row_block={} global_row_start={} tile_row_start={} row_count={} output_gpu=0x{:X} source_window={}..{} artifact_window=16..32 live_k_dim={} output_nonzero_dwords={} output_owner=cpu-ap next=t6-3-accum16-hi-live32-partial does_not_prove=full_model_matvec\n",
                t63_restore.readback_ok as u8,
                t63_restore.reason,
                trueos_eu::gfx12::T63_ACCUM16_HI_LIVE32_PROGRAM_NAME,
                tile_index,
                armed_tiles,
                row_block_index,
                global_row,
                block_tile_row,
                t63_restore.row_count,
                t63_restore.output_gpu,
                trueos_eu::gfx12::T62_ROW_INDEXED_LIVE_K,
                t63_restore_live_k_dim,
                t63_restore_live_k_dim,
                t63_restore.output_nonzero_dwords,
            );
            if !t63_restore.readback_ok {
                continue;
            }

            let t63_live_k_dim = k_dim.min(trueos_eu::gfx12::T63_ACCUM16_HI_LIVE32_LIVE_K);
            let mut t63_expected_words = [0u32; 8];
            for local_row in 0..block_row_count {
                let row_start = local_row.saturating_mul(k_dim).saturating_mul(2);
                let row_end = row_start.saturating_add(k_dim.saturating_mul(2));
                t63_expected_words[local_row] =
                    bf16_row_dot_prefix(x, &t62_rows[row_start..row_end], t63_live_k_dim).to_bits();
            }
            let t63 = crate::intel::submit_gpgpu_t63_accum16_hi_live32_partial_matvec_probe(
                stage.output_gpu,
                stage.output_bytes,
                t63_expected_words,
                block_row_count,
                t63_live_k_dim,
            );
            t63_submitted_blocks += t63.submitted as usize;
            t63_finished_blocks += t63.finished as usize;
            t63_compare_ok_blocks += t63.compare_ok as usize;
            t63_compared_rows =
                t63_compared_rows.saturating_add(if t63.compare_ok { block_row_count } else { 0 });
            last_gpu_value = t63.output_words[0];
            last_cpu_expected_bits = t63.expected_words[0];
            last_dispatch = t63.dispatch_delta;
            last_partial_rows = t63_compared_rows;
            crate::log!(
                "lumen-gpu-proof: director-step step=15 backend=local-gpu mode=t6-3-accum16-hi-row-block-live32-partial submitted={} finished={} readback_ok={} compare_ok={} reason={} program={} tile_index={} armed_tiles={} row_block={} global_row_start={} tile_row_start={} row_count={} output_gpu=0x{:X} compare_mask=0x{:08X} expected_mask=0x{:08X} first_gpu=0x{:08X} first_cpu_expected=0x{:08X} last_gpu=0x{:08X} last_cpu_expected=0x{:08X} lane_dispatch={} live_k_dim={} t62_first_gpu=0x{:08X} t62_first_cpu_expected=0x{:08X} finish_marker=0x{:08X} finish_expected=0x{:08X} batch_bytes=0x{:X} output_owner=cpu-ap action=hold-scale next=promote-row-block-owner-or-scale-live-k does_not_prove=full_model_matvec\n",
                t63.submitted as u8,
                t63.finished as u8,
                t63.readback_ok as u8,
                t63.compare_ok as u8,
                t63.reason,
                t63.program_name,
                tile_index,
                armed_tiles,
                row_block_index,
                global_row,
                block_tile_row,
                t63.row_count,
                t63.output_gpu,
                t63.compare_mask,
                t63.expected_mask,
                t63.output_words[0],
                t63.expected_words[0],
                t63.output_words[t63.row_count.saturating_sub(1)],
                t63.expected_words[t63.row_count.saturating_sub(1)],
                t63.dispatch_delta,
                t63.live_k_dim,
                t62.output_words[0],
                t62.expected_words[0],
                t63.finish_marker,
                t63.expected_finish_marker,
                t63.batch_bytes,
            );
            if tile_index == 0 && row_block_index == 0 {
                crate::intel::log_gpgpu_t63_first_tile_output_detail_once(
                    stage.output_gpu,
                    stage.output_bytes,
                    t63_expected_words,
                    block_row_count,
                    t63_live_k_dim,
                );
            }
            if !t63.compare_ok {
                continue;
            }

            let t64_live_k_dim = k_dim.min(trueos_eu::gfx12::T64_WINDOWED_ACCUM16_LIVE48_LIVE_K);
            let t64_stage = crate::intel::stage_gpgpu_tile_record_accum16_window_probe(
                stage.output_gpu,
                x,
                t62_rows,
                block_row_count,
                k_dim,
                trueos_eu::gfx12::T64_WINDOWED_ACCUM16_LIVE48_WINDOW_START,
            );
            t64_window_staged_blocks += t64_stage.readback_ok as usize;
            crate::log!(
                "lumen-gpu-proof: director-step step=17 backend=local-gpu mode=t6-4-windowed-accum16-live48-stage staged={} reason={} program={} tile_index={} armed_tiles={} row_block={} global_row_start={} tile_row_start={} row_count={} output_gpu=0x{:X} source_window={}..{} artifact_window=16..32 live_k_dim={} output_nonzero_dwords={} output_owner=cpu-ap next=t6-4-windowed-accum16-live48-partial does_not_prove=full_model_matvec\n",
                t64_stage.readback_ok as u8,
                t64_stage.reason,
                trueos_eu::gfx12::T64_WINDOWED_ACCUM16_LIVE48_PROGRAM_NAME,
                tile_index,
                armed_tiles,
                row_block_index,
                global_row,
                block_tile_row,
                t64_stage.row_count,
                t64_stage.output_gpu,
                trueos_eu::gfx12::T64_WINDOWED_ACCUM16_LIVE48_WINDOW_START,
                t64_live_k_dim,
                t64_live_k_dim,
                t64_stage.output_nonzero_dwords,
            );
            if !t64_stage.readback_ok {
                continue;
            }

            let mut t64_expected_words = [0u32; 8];
            for local_row in 0..block_row_count {
                let row_start = local_row.saturating_mul(k_dim).saturating_mul(2);
                let row_end = row_start.saturating_add(k_dim.saturating_mul(2));
                t64_expected_words[local_row] =
                    bf16_row_dot_prefix(x, &t62_rows[row_start..row_end], t64_live_k_dim).to_bits();
            }
            let t64 = crate::intel::submit_gpgpu_t64_windowed_accum16_live48_partial_matvec_probe(
                stage.output_gpu,
                stage.output_bytes,
                t64_expected_words,
                block_row_count,
                t64_live_k_dim,
            );
            t64_submitted_blocks += t64.submitted as usize;
            t64_finished_blocks += t64.finished as usize;
            t64_compare_ok_blocks += t64.compare_ok as usize;
            t64_compared_rows =
                t64_compared_rows.saturating_add(if t64.compare_ok { block_row_count } else { 0 });
            last_gpu_value = t64.output_words[0];
            last_cpu_expected_bits = t64.expected_words[0];
            last_dispatch = t64.dispatch_delta;
            last_partial_rows = t64_compared_rows;
            crate::log!(
                "lumen-gpu-proof: director-step step=18 backend=local-gpu mode=t6-4-windowed-accum16-live48-partial submitted={} finished={} readback_ok={} compare_ok={} reason={} program={} tile_index={} armed_tiles={} row_block={} global_row_start={} tile_row_start={} row_count={} output_gpu=0x{:X} compare_mask=0x{:08X} expected_mask=0x{:08X} first_gpu=0x{:08X} first_cpu_expected=0x{:08X} last_gpu=0x{:08X} last_cpu_expected=0x{:08X} lane_dispatch={} live_k_dim={} t63_first_gpu=0x{:08X} t63_first_cpu_expected=0x{:08X} finish_marker=0x{:08X} finish_expected=0x{:08X} batch_bytes=0x{:X} output_owner=cpu-ap action=hold-scale next=t6-5-windowed-accum16-live64-partial does_not_prove=full_model_matvec\n",
                t64.submitted as u8,
                t64.finished as u8,
                t64.readback_ok as u8,
                t64.compare_ok as u8,
                t64.reason,
                t64.program_name,
                tile_index,
                armed_tiles,
                row_block_index,
                global_row,
                block_tile_row,
                t64.row_count,
                t64.output_gpu,
                t64.compare_mask,
                t64.expected_mask,
                t64.output_words[0],
                t64.expected_words[0],
                t64.output_words[t64.row_count.saturating_sub(1)],
                t64.expected_words[t64.row_count.saturating_sub(1)],
                t64.dispatch_delta,
                t64.live_k_dim,
                t63.output_words[0],
                t63.expected_words[0],
                t64.finish_marker,
                t64.expected_finish_marker,
                t64.batch_bytes,
            );
            if !t64.compare_ok {
                continue;
            }

            let t65_live_k_dim = k_dim.min(trueos_eu::gfx12::T65_WINDOWED_ACCUM16_LIVE64_LIVE_K);
            let t65_stage = crate::intel::stage_gpgpu_tile_record_accum16_window_probe(
                stage.output_gpu,
                x,
                t62_rows,
                block_row_count,
                k_dim,
                trueos_eu::gfx12::T65_WINDOWED_ACCUM16_LIVE64_WINDOW_START,
            );
            t65_window_staged_blocks += t65_stage.readback_ok as usize;
            crate::log!(
                "lumen-gpu-proof: director-step step=19 backend=local-gpu mode=t6-5-windowed-accum16-live64-stage staged={} reason={} program={} tile_index={} armed_tiles={} row_block={} global_row_start={} tile_row_start={} row_count={} output_gpu=0x{:X} source_window={}..{} artifact_window=16..32 live_k_dim={} output_nonzero_dwords={} output_owner=cpu-ap next=t6-5-windowed-accum16-live64-partial does_not_prove=full_model_matvec\n",
                t65_stage.readback_ok as u8,
                t65_stage.reason,
                trueos_eu::gfx12::T65_WINDOWED_ACCUM16_LIVE64_PROGRAM_NAME,
                tile_index,
                armed_tiles,
                row_block_index,
                global_row,
                block_tile_row,
                t65_stage.row_count,
                t65_stage.output_gpu,
                trueos_eu::gfx12::T65_WINDOWED_ACCUM16_LIVE64_WINDOW_START,
                t65_live_k_dim,
                t65_live_k_dim,
                t65_stage.output_nonzero_dwords,
            );
            if !t65_stage.readback_ok {
                continue;
            }

            let mut t65_expected_words = [0u32; 8];
            for local_row in 0..block_row_count {
                let row_start = local_row.saturating_mul(k_dim).saturating_mul(2);
                let row_end = row_start.saturating_add(k_dim.saturating_mul(2));
                t65_expected_words[local_row] =
                    bf16_row_dot_prefix(x, &t62_rows[row_start..row_end], t65_live_k_dim).to_bits();
            }
            let t65 = crate::intel::submit_gpgpu_t65_windowed_accum16_live64_partial_matvec_probe(
                stage.output_gpu,
                stage.output_bytes,
                t65_expected_words,
                block_row_count,
                t65_live_k_dim,
            );
            t65_submitted_blocks += t65.submitted as usize;
            t65_finished_blocks += t65.finished as usize;
            t65_compare_ok_blocks += t65.compare_ok as usize;
            t65_compared_rows =
                t65_compared_rows.saturating_add(if t65.compare_ok { block_row_count } else { 0 });
            last_gpu_value = t65.output_words[0];
            last_cpu_expected_bits = t65.expected_words[0];
            last_dispatch = t65.dispatch_delta;
            last_partial_rows = t65_compared_rows;
            crate::log!(
                "lumen-gpu-proof: director-step step=20 backend=local-gpu mode=t6-5-windowed-accum16-live64-partial submitted={} finished={} readback_ok={} compare_ok={} reason={} program={} tile_index={} armed_tiles={} row_block={} global_row_start={} tile_row_start={} row_count={} output_gpu=0x{:X} compare_mask=0x{:08X} expected_mask=0x{:08X} first_gpu=0x{:08X} first_cpu_expected=0x{:08X} last_gpu=0x{:08X} last_cpu_expected=0x{:08X} lane_dispatch={} live_k_dim={} t64_first_gpu=0x{:08X} t64_first_cpu_expected=0x{:08X} finish_marker=0x{:08X} finish_expected=0x{:08X} batch_bytes=0x{:X} output_owner=cpu-ap action=offer-accepted-prefix next=promote-row-block-owner-or-scale-live-k does_not_prove=full_model_matvec\n",
                t65.submitted as u8,
                t65.finished as u8,
                t65.readback_ok as u8,
                t65.compare_ok as u8,
                t65.reason,
                t65.program_name,
                tile_index,
                armed_tiles,
                row_block_index,
                global_row,
                block_tile_row,
                t65.row_count,
                t65.output_gpu,
                t65.compare_mask,
                t65.expected_mask,
                t65.output_words[0],
                t65.expected_words[0],
                t65.output_words[t65.row_count.saturating_sub(1)],
                t65.expected_words[t65.row_count.saturating_sub(1)],
                t65.dispatch_delta,
                t65.live_k_dim,
                t64.output_words[0],
                t64.expected_words[0],
                t65.finish_marker,
                t65.expected_finish_marker,
                t65.batch_bytes,
            );
            if t65.compare_ok && t65.live_k_dim == cgp_prefix.live_k_dim {
                for local_row in 0..block_row_count.min(t65.output_words.len()) {
                    cgp_prefix.push_row(
                        global_row.saturating_add(local_row),
                        t65.output_words[local_row],
                    );
                }
            }
        }
    }

    crate::log!(
        "lumen-gpu-proof: director-step step=16 backend=local-gpu source={} mode=t6-5-actual-work-row-blocks upfront_proven_stages={} armed_tiles={} staged_tiles={} t5_submitted_tiles={} t5_finished_tiles={} t5_compare_ok_tiles={} t6_submitted_tiles={} t6_finished_tiles={} t6_compare_ok_tiles={} t61_submitted_tiles={} t61_finished_tiles={} t61_compare_ok_tiles={} t62_staged_blocks={} t62_submitted_blocks={} t62_finished_blocks={} t62_compare_ok_blocks={} t62_compared_rows={} t63_submitted_blocks={} t63_finished_blocks={} t63_compare_ok_blocks={} t63_compared_rows={} t64_window_staged_blocks={} t64_submitted_blocks={} t64_finished_blocks={} t64_compare_ok_blocks={} t64_compared_rows={} t65_window_staged_blocks={} t65_submitted_blocks={} t65_finished_blocks={} t65_compare_ok_blocks={} t65_compared_rows={} first_row=0 last_row={} row_block_rows={} row_block_cap={} partial_rows={} tile_rows={} k_dim={} t5_artifact=gfx12-t5-small-live4-packed-bf16-dot t6_artifact=gfx12-t6-small-live8-packed-bf16-dot t61_artifact=gfx12-t6-1-live16-packed-bf16-dot t62_artifact=gfx12-t6-2-lane-indexed-live16-packed-bf16-dot t63_artifact=gfx12-t6-3-accum16-hi-live32-packed-bf16-dot t64_artifact=gfx12-t6-4-windowed-accum16-live48-packed-bf16-dot t65_artifact=gfx12-t6-5-windowed-accum16-live64-packed-bf16-dot artifact_addressing=row-block-restaged-tile-record-prefix proof_role=actual-work-row-block-frontier cgp_mode={} cgp_prefix_rows={} cgp_prefix_live_k={} last_gpu_value=0x{:08X} last_cpu_expected_bits=0x{:08X} last_lane_dispatch={} output_owner=cpu-ap action=offer-accepted-prefix next=cpu-suffix-finish-or-scale-live-k does_not_prove=full_model_matvec\n",
        plan.source_label,
        CGP_RERUN_UPFRONT_PROVEN_STAGES as u8,
        armed_tiles,
        staged_tiles,
        t5_submitted_tiles,
        t5_finished_tiles,
        t5_compare_ok_tiles,
        t6_submitted_tiles,
        t6_finished_tiles,
        t6_compare_ok_tiles,
        t61_submitted_tiles,
        t61_finished_tiles,
        t61_compare_ok_tiles,
        t62_staged_blocks,
        t62_submitted_blocks,
        t62_finished_blocks,
        t62_compare_ok_blocks,
        t62_compared_rows,
        t63_submitted_blocks,
        t63_finished_blocks,
        t63_compare_ok_blocks,
        t63_compared_rows,
        t64_window_staged_blocks,
        t64_submitted_blocks,
        t64_finished_blocks,
        t64_compare_ok_blocks,
        t64_compared_rows,
        t65_window_staged_blocks,
        t65_submitted_blocks,
        t65_finished_blocks,
        t65_compare_ok_blocks,
        t65_compared_rows,
        last_row,
        trueos_eu::gfx12::T62_ROW_INDEXED_PARTIAL_ROWS,
        T62_ROW_BLOCK_DISPATCH_BLOCK_CAP,
        last_partial_rows,
        proof_tile_rows,
        k_dim,
        cgp_prefix.mode.as_str(),
        cgp_prefix.rows.len(),
        cgp_prefix.live_k_dim,
        last_gpu_value,
        last_cpu_expected_bits,
        last_dispatch,
    );
    cgp_prefix
}

fn bf16_row_dot_prefix(x: &[f32], row_bf16: &[u8], count: usize) -> f32 {
    let mut acc = 0.0f32;
    for i in 0..count {
        let off = i.saturating_mul(2);
        let bits = u16::from_le_bytes([row_bf16[off], row_bf16[off + 1]]);
        acc += x[i] * bf16_to_f32(bits);
    }
    acc
}

fn bf16_row_dot(x: &[f32], row_bf16: &[u8], k_dim: usize) -> f32 {
    let mut acc = 0.0f32;
    for i in 0..k_dim {
        let off = i.saturating_mul(2);
        let bits = u16::from_le_bytes([row_bf16[off], row_bf16[off + 1]]);
        acc += x[i] * bf16_to_f32(bits);
    }
    acc
}

fn live_x_static4_probe(x: &[f32], k_dim: usize) -> f32 {
    let weights = trueos_eu::gfx12::T4_LIVE_X_STATIC_DP4A_WEIGHTS_U8;
    let mut acc = 0.0f32;
    let count = k_dim.min(weights.len()).min(x.len());
    for i in 0..count {
        acc += x[i] * weights[i] as f32;
    }
    acc
}

fn bf16_to_f32(bits: u16) -> f32 {
    f32::from_bits((bits as u32) << 16)
}

fn checksum_f32_prefix(values: &[f32], count: usize) -> u64 {
    let mut hash = 0xCBF2_9CE4_8422_2325u64;
    for value in values.iter().take(count) {
        hash = checksum_bytes_step(hash, &value.to_bits().to_le_bytes());
    }
    hash
}

fn checksum_bytes(bytes: &[u8]) -> u64 {
    let mut hash = 0xCBF2_9CE4_8422_2325u64;
    hash = checksum_bytes_step(hash, bytes);
    hash
}

fn checksum_bytes_step(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
    }
    hash
}
