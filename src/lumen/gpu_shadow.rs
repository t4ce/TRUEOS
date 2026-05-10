use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static SEEN_BF16_MATVECS: AtomicU64 = AtomicU64::new(0);
static LOGGED_SHADOW_PLAN: AtomicBool = AtomicBool::new(false);
static LOGGED_STATIC_TILE_PROOF: AtomicBool = AtomicBool::new(false);
static LOGGED_T4_WAITING: AtomicBool = AtomicBool::new(false);
static LOGGED_T4_LIVE_ROW_PROBE: AtomicBool = AtomicBool::new(false);

#[derive(Copy, Clone, Debug)]
pub(crate) struct LocalGpuProofPlan {
    pub(crate) call_index: u64,
    pub(crate) candidate: bool,
    pub(crate) static_tile_proven: bool,
    pub(crate) lane_dispatch_count: u32,
    pub(crate) expected_store_value: u32,
    pub(crate) observed_store_value: u32,
    pub(crate) program_name: &'static str,
}

pub(crate) fn observe_bf16_matvec_call(
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
        call_index,
        candidate,
        static_tile_proven,
        lane_dispatch_count: gpu.eu_dispatch_delta,
        expected_store_value: gpu.eu_expected_store_value,
        observed_store_value: gpu.eu_c_store_value,
        program_name: gpu.eu_program_name,
    };

    if !LOGGED_SHADOW_PLAN.swap(true, Ordering::AcqRel) {
        crate::log!(
            "lumen-gpu-proof: director-step step=2 backend=local-gpu mode=proof-only call={} rows={} k_dim={} chunk_rows={} chunks={} min_rows={} min_k_dim={} arena_ready={} shape_candidate={} candidate={} static_tile_proven={} program={} lane_dispatch={} expected=0x{:08X} observed=0x{:08X} output_owner=cpu-ap action=no-output-ownership next=one-live-row-proof-compare\n",
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
) {
    if !plan.static_tile_proven {
        if !LOGGED_T4_WAITING.swap(true, Ordering::AcqRel) {
            crate::log!(
                "lumen-gpu-proof: director-step step=4 backend=local-gpu mode=t4-live-row-probe ready=0 reason=static-gpu-artifact-not-proven-yet call={} program={} next=wait-for-static-dp4a-hdc-store-eot\n",
                plan.call_index,
                plan.program_name
            );
        }
        return;
    }

    if LOGGED_T4_LIVE_ROW_PROBE.swap(true, Ordering::AcqRel) {
        return;
    }

    let Some(expected_w_len) = n_rows
        .checked_mul(k_dim)
        .and_then(|values| values.checked_mul(2))
    else {
        crate::log!(
            "lumen-gpu-proof: director-step step=4 backend=local-gpu mode=t4-live-row-probe ready=0 reason=shape-overflow rows={} k_dim={}\n",
            n_rows,
            k_dim
        );
        return;
    };
    if n_rows == 0 || k_dim == 0 || x.len() < k_dim || w_rowmajor_bf16.len() < expected_w_len {
        crate::log!(
            "lumen-gpu-proof: director-step step=4 backend=local-gpu mode=t4-live-row-probe ready=0 reason=bad-shape rows={} k_dim={} x_len={} w_len={} expected_w_len={}\n",
            n_rows,
            k_dim,
            x.len(),
            w_rowmajor_bf16.len(),
            expected_w_len
        );
        return;
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
        "lumen-gpu-proof: director-step step=4 backend=local-gpu mode=t4-live-row-probe ready=1 call={} rows={} k_dim={} chunk_rows={} chunks={} manifest={} matrix=0x{:016X} matrix_epoch={} matrix_name_hash=0x{:016X} matrix_name_len={} matrix_rows={} matrix_k_dim={} matrix_ptr=0x{:X} matrix_bytes={} matrix_access=resident-read-only row=0 row_ptr=0x{:X} x_ptr=0x{:X} x_bytes={} x_checksum=0x{:016X} row_checksum=0x{:016X} static4_weights=01020304 static4_expected_bits=0x{:08X} row0_cpu_expected_bits=0x{:08X} gpu_submission=0 output_owner=cpu-ap next=stage-manifest-row-to-gpgpu-arena does_not_prove=gpu_live_load_or_model_matvec\n",
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

    let stage = crate::intel::stage_gpgpu_one_tile_shadow_probe(
        x,
        w_rowmajor_bf16,
        k_dim,
        0,
        x_checksum,
        row_checksum,
        row0.to_bits(),
    );
    crate::log!(
        "lumen-gpu-proof: director-step step=5 backend=local-gpu mode=one-tile-arena-stage staged={} reason={} call={} manifest={} row=0 tile_rows={} k_dim={} x_gpu=0x{:X} row_gpu=0x{:X} output_gpu=0x{:X} x_bytes={} row_bytes={} output_bytes={} cpu_expected_bits=0x{:08X} gpu_submission=0 output_owner=cpu-ap next=one-tile-gpu-proof-compare-artifact does_not_prove=gpu_live_load_or_model_matvec\n",
        stage.staged as u8,
        stage.reason,
        plan.call_index,
        manifest.is_some() as u8,
        stage.tile_rows,
        stage.k_dim,
        stage.x_gpu,
        stage.row_gpu,
        stage.output_gpu,
        stage.x_bytes,
        stage.row_bytes,
        stage.output_bytes,
        row0.to_bits(),
    );
    crate::log!(
        "lumen-gpu-proof: director-step step=6 backend=local-gpu mode=one-worker-tile-readback readback_ok={} staged={} output_zeroed={} output_first_bits=0x{:08X} output_nonzero_dwords={} output_expected_hits_lo64=0x{:016X} output_checksum=0x{:016X} cpu_expected_bits=0x{:08X} gpu_submission=0 output_owner=cpu-ap action=hold-scale next=submit-one-worker-read-alu-write-kernel does_not_prove=gpu_output_or_model_matvec\n",
        stage.readback_ok as u8,
        stage.staged as u8,
        stage.output_zeroed as u8,
        stage.output_first_bits,
        stage.output_nonzero_dwords,
        stage.output_expected_hits_lo64,
        stage.output_checksum,
        row0.to_bits(),
    );
    if stage.readback_ok {
        let sentinel = crate::intel::submit_gpgpu_one_tile_output_sentinel_probe(
            stage.output_gpu,
            stage.output_bytes,
            row0.to_bits(),
        );
        crate::log!(
            "lumen-gpu-proof: director-step step=7 backend=local-gpu mode=one-worker-output-sentinel submitted={} finished={} readback_ok={} reason={} program={} output_gpu=0x{:X} sentinel=0x{:08X} output_first_before=0x{:08X} output_first_after=0x{:08X} output_nonzero_before={} output_nonzero_after={} output_hits_lo64=0x{:016X} lane_dispatch={} finish_marker=0x{:08X} finish_expected=0x{:08X} batch_bytes=0x{:X} cpu_expected_bits=0x{:08X} output_owner=cpu-ap action=hold-scale next=replace-sentinel-with-one-row-dot does_not_prove=model_matvec\n",
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
            row0.to_bits(),
        );
        if sentinel.readback_ok {
            let compare = crate::intel::submit_gpgpu_one_tile_output_compare_probe(
                stage.output_gpu,
                stage.output_bytes,
                row0.to_bits(),
            );
            crate::log!(
                "lumen-gpu-proof: director-step step=8 backend=local-gpu mode=one-worker-output-compare submitted={} finished={} readback_ok={} compare_ok={} reason={} program={} output_gpu=0x{:X} gpu_value=0x{:08X} cpu_expected_bits=0x{:08X} output_first_before=0x{:08X} output_first_after=0x{:08X} output_hits_lo64=0x{:016X} lane_dispatch={} finish_marker=0x{:08X} finish_expected=0x{:08X} batch_bytes=0x{:X} output_owner=cpu-ap action=hold-scale next=replace-dp4a-echo-with-live-read does_not_prove=model_matvec_or_gpu_live_load\n",
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
            if compare.readback_ok {
                let live_k_dim = k_dim.min(trueos_eu::gfx12::T5_ONE_ROW_MATVEC_LIVE_K);
                let t5 = crate::intel::submit_gpgpu_t5_one_row_matvec_probe(
                    stage.output_gpu,
                    stage.output_bytes,
                    row0.to_bits(),
                    live_k_dim,
                );
                crate::log!(
                    "lumen-gpu-proof: director-step step=9 backend=local-gpu mode=t5-one-row-live-matvec submitted={} finished={} readback_ok={} compare_ok={} reason={} program={} output_gpu=0x{:X} gpu_value=0x{:08X} cpu_expected_bits=0x{:08X} output_first_before=0x{:08X} output_first_after=0x{:08X} output_hits_lo64=0x{:016X} lane_dispatch={} live_k_dim={} requires_live_gpu_load={} finish_marker=0x{:08X} finish_expected=0x{:08X} batch_bytes=0x{:X} output_owner=cpu-ap action=hold-scale next=generate-t5-live-load-bf16-dot-eu-artifact does_not_prove=model_matvec_or_gpu_live_load\n",
                    t5.submitted as u8,
                    t5.finished as u8,
                    t5.readback_ok as u8,
                    t5.compare_ok as u8,
                    t5.reason,
                    t5.program_name,
                    t5.output_gpu,
                    t5.gpu_value,
                    t5.cpu_expected_bits,
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
            }
        }
    }
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
