use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

static SEEN_BF16_MATVECS: AtomicU64 = AtomicU64::new(0);
static LOGGED_SHADOW_PLAN: AtomicBool = AtomicBool::new(false);
static LOGGED_PROMPT_SHADOW_PLAN: AtomicBool = AtomicBool::new(false);
static LOGGED_STATIC_TILE_PROOF: AtomicBool = AtomicBool::new(false);
static LOGGED_T4_WAITING: AtomicBool = AtomicBool::new(false);
static LOGGED_T4_LIVE_ROW_PROBE: AtomicBool = AtomicBool::new(false);
static LOGGED_PROMPT_LIVE_ROW_PROBE: AtomicBool = AtomicBool::new(false);
static LOGGED_TRUSTED_WINDOW_FAST_PATH: AtomicBool = AtomicBool::new(false);
static LOGGED_T8_BATCH2_RETIRE_PROBE: AtomicBool = AtomicBool::new(false);
static T8_GROUPID_FRONTIER_ROWS: AtomicUsize = AtomicUsize::new(0);
static TRUSTED_WINDOW_FRONTIER_RUNG: AtomicUsize = AtomicUsize::new(0);
static TRUSTED_WINDOW_FRONTIER_LIVE_K: AtomicUsize = AtomicUsize::new(0);

const LOG_STRICT_WINDOW_LADDER_DETAILS: bool = false;

macro_rules! log_strict_window_ladder_detail {
    ($($arg:tt)*) => {
        if LOG_STRICT_WINDOW_LADDER_DETAILS {
            crate::log!($($arg)*);
        }
    };
}

// T6.2/T6.3 are 8-lane row-block artifacts. The cap is coordination-only:
// each block is restaged into the same tile-record prefix, so raising this
// increases proved row coverage without changing artifact math.
const T62_ROW_BLOCK_DISPATCH_BLOCK_CAP: usize = 32;
const T8_GROUPID_ROW_SCALE_RUNGS: &[usize] = &[2, 4, 8, 16, 32];

pub(crate) fn t8_groupid_frontier_rows() -> usize {
    T8_GROUPID_FRONTIER_ROWS.load(Ordering::Acquire)
}

#[derive(Copy, Clone)]
struct WindowedAccum16Rung {
    rung: usize,
    live_k_dim: usize,
    window_start: usize,
    program_name: &'static str,
}

const CGP_WINDOWED_ACCUM16_EXTRA_RUNGS: &[WindowedAccum16Rung] = &[
    WindowedAccum16Rung {
        rung: 12,
        live_k_dim: 176,
        window_start: 160,
        program_name: "gfx12-t6-12-windowed-accum16-live176-packed-bf16-dot-hdc1-stateless-store-then-ts-eot",
    },
    WindowedAccum16Rung {
        rung: 13,
        live_k_dim: 192,
        window_start: 176,
        program_name: "gfx12-t6-13-windowed-accum16-live192-packed-bf16-dot-hdc1-stateless-store-then-ts-eot",
    },
    WindowedAccum16Rung {
        rung: 14,
        live_k_dim: 208,
        window_start: 192,
        program_name: "gfx12-t6-14-windowed-accum16-live208-packed-bf16-dot-hdc1-stateless-store-then-ts-eot",
    },
    WindowedAccum16Rung {
        rung: 15,
        live_k_dim: 224,
        window_start: 208,
        program_name: "gfx12-t6-15-windowed-accum16-live224-packed-bf16-dot-hdc1-stateless-store-then-ts-eot",
    },
    WindowedAccum16Rung {
        rung: 16,
        live_k_dim: 240,
        window_start: 224,
        program_name: "gfx12-t6-16-windowed-accum16-live240-packed-bf16-dot-hdc1-stateless-store-then-ts-eot",
    },
    WindowedAccum16Rung {
        rung: 17,
        live_k_dim: 256,
        window_start: 240,
        program_name: "gfx12-t6-17-windowed-accum16-live256-packed-bf16-dot-hdc1-stateless-store-then-ts-eot",
    },
    WindowedAccum16Rung {
        rung: 18,
        live_k_dim: 272,
        window_start: 256,
        program_name: "gfx12-t6-18-windowed-accum16-live272-packed-bf16-dot-hdc1-stateless-store-then-ts-eot",
    },
    WindowedAccum16Rung {
        rung: 19,
        live_k_dim: 288,
        window_start: 272,
        program_name: "gfx12-t6-19-windowed-accum16-live288-packed-bf16-dot-hdc1-stateless-store-then-ts-eot",
    },
    WindowedAccum16Rung {
        rung: 20,
        live_k_dim: 304,
        window_start: 288,
        program_name: "gfx12-t6-20-windowed-accum16-live304-packed-bf16-dot-hdc1-stateless-store-then-ts-eot",
    },
    WindowedAccum16Rung {
        rung: 21,
        live_k_dim: 320,
        window_start: 304,
        program_name: "gfx12-t6-21-windowed-accum16-live320-packed-bf16-dot-hdc1-stateless-store-then-ts-eot",
    },
    WindowedAccum16Rung {
        rung: 22,
        live_k_dim: 336,
        window_start: 320,
        program_name: "gfx12-t6-22-windowed-accum16-live336-packed-bf16-dot-hdc1-stateless-store-then-ts-eot",
    },
    WindowedAccum16Rung {
        rung: 23,
        live_k_dim: 352,
        window_start: 336,
        program_name: "gfx12-t6-23-windowed-accum16-live352-packed-bf16-dot-hdc1-stateless-store-then-ts-eot",
    },
    WindowedAccum16Rung {
        rung: 24,
        live_k_dim: 368,
        window_start: 352,
        program_name: "gfx12-t6-24-windowed-accum16-live368-packed-bf16-dot-hdc1-stateless-store-then-ts-eot",
    },
    WindowedAccum16Rung {
        rung: 25,
        live_k_dim: 384,
        window_start: 368,
        program_name: "gfx12-t6-25-windowed-accum16-live384-packed-bf16-dot-hdc1-stateless-store-then-ts-eot",
    },
    WindowedAccum16Rung {
        rung: 26,
        live_k_dim: 400,
        window_start: 384,
        program_name: "gfx12-t6-26-windowed-accum16-live400-packed-bf16-dot-hdc1-stateless-store-then-ts-eot",
    },
    WindowedAccum16Rung {
        rung: 27,
        live_k_dim: 416,
        window_start: 400,
        program_name: "gfx12-t6-27-windowed-accum16-live416-packed-bf16-dot-hdc1-stateless-store-then-ts-eot",
    },
    WindowedAccum16Rung {
        rung: 28,
        live_k_dim: 432,
        window_start: 416,
        program_name: "gfx12-t6-28-windowed-accum16-live432-packed-bf16-dot-hdc1-stateless-store-then-ts-eot",
    },
    WindowedAccum16Rung {
        rung: 29,
        live_k_dim: 448,
        window_start: 432,
        program_name: "gfx12-t6-29-windowed-accum16-live448-packed-bf16-dot-hdc1-stateless-store-then-ts-eot",
    },
    WindowedAccum16Rung {
        rung: 30,
        live_k_dim: 464,
        window_start: 448,
        program_name: "gfx12-t6-30-windowed-accum16-live464-packed-bf16-dot-hdc1-stateless-store-then-ts-eot",
    },
    WindowedAccum16Rung {
        rung: 31,
        live_k_dim: 480,
        window_start: 464,
        program_name: "gfx12-t6-31-windowed-accum16-live480-packed-bf16-dot-hdc1-stateless-store-then-ts-eot",
    },
    WindowedAccum16Rung {
        rung: 32,
        live_k_dim: 496,
        window_start: 480,
        program_name: "gfx12-t6-32-windowed-accum16-live496-packed-bf16-dot-hdc1-stateless-store-then-ts-eot",
    },
    WindowedAccum16Rung {
        rung: 33,
        live_k_dim: 512,
        window_start: 496,
        program_name: "gfx12-t6-33-windowed-accum16-live512-packed-bf16-dot-hdc1-stateless-store-then-ts-eot",
    },
];

const CGP_WINDOWED_ACCUM16_BASE_RUNGS: &[WindowedAccum16Rung] = &[
    WindowedAccum16Rung {
        rung: 4,
        live_k_dim: trueos_eu::gfx12::T64_WINDOWED_ACCUM16_LIVE48_LIVE_K,
        window_start: trueos_eu::gfx12::T64_WINDOWED_ACCUM16_LIVE48_WINDOW_START,
        program_name: trueos_eu::gfx12::T64_WINDOWED_ACCUM16_LIVE48_PROGRAM_NAME,
    },
    WindowedAccum16Rung {
        rung: 5,
        live_k_dim: trueos_eu::gfx12::T65_WINDOWED_ACCUM16_LIVE64_LIVE_K,
        window_start: trueos_eu::gfx12::T65_WINDOWED_ACCUM16_LIVE64_WINDOW_START,
        program_name: trueos_eu::gfx12::T65_WINDOWED_ACCUM16_LIVE64_PROGRAM_NAME,
    },
    WindowedAccum16Rung {
        rung: 6,
        live_k_dim: trueos_eu::gfx12::T66_WINDOWED_ACCUM16_LIVE80_LIVE_K,
        window_start: trueos_eu::gfx12::T66_WINDOWED_ACCUM16_LIVE80_WINDOW_START,
        program_name: trueos_eu::gfx12::T66_WINDOWED_ACCUM16_LIVE80_PROGRAM_NAME,
    },
    WindowedAccum16Rung {
        rung: 7,
        live_k_dim: trueos_eu::gfx12::T67_WINDOWED_ACCUM16_LIVE96_LIVE_K,
        window_start: trueos_eu::gfx12::T67_WINDOWED_ACCUM16_LIVE96_WINDOW_START,
        program_name: trueos_eu::gfx12::T67_WINDOWED_ACCUM16_LIVE96_PROGRAM_NAME,
    },
    WindowedAccum16Rung {
        rung: 8,
        live_k_dim: trueos_eu::gfx12::T68_WINDOWED_ACCUM16_LIVE112_LIVE_K,
        window_start: trueos_eu::gfx12::T68_WINDOWED_ACCUM16_LIVE112_WINDOW_START,
        program_name: trueos_eu::gfx12::T68_WINDOWED_ACCUM16_LIVE112_PROGRAM_NAME,
    },
    WindowedAccum16Rung {
        rung: 9,
        live_k_dim: trueos_eu::gfx12::T69_WINDOWED_ACCUM16_LIVE128_LIVE_K,
        window_start: trueos_eu::gfx12::T69_WINDOWED_ACCUM16_LIVE128_WINDOW_START,
        program_name: trueos_eu::gfx12::T69_WINDOWED_ACCUM16_LIVE128_PROGRAM_NAME,
    },
    WindowedAccum16Rung {
        rung: 10,
        live_k_dim: trueos_eu::gfx12::T610_WINDOWED_ACCUM16_LIVE144_LIVE_K,
        window_start: trueos_eu::gfx12::T610_WINDOWED_ACCUM16_LIVE144_WINDOW_START,
        program_name: trueos_eu::gfx12::T610_WINDOWED_ACCUM16_LIVE144_PROGRAM_NAME,
    },
    WindowedAccum16Rung {
        rung: 11,
        live_k_dim: trueos_eu::gfx12::T611_WINDOWED_ACCUM16_LIVE160_LIVE_K,
        window_start: trueos_eu::gfx12::T611_WINDOWED_ACCUM16_LIVE160_WINDOW_START,
        program_name: trueos_eu::gfx12::T611_WINDOWED_ACCUM16_LIVE160_PROGRAM_NAME,
    },
];

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
    let trusted_live_k = TRUSTED_WINDOW_FRONTIER_LIVE_K.load(Ordering::Acquire);
    if !log_global && !log_prompt {
        if trusted_live_k != 0 {
            return observe_trusted_window_frontier(
                x,
                w_rowmajor_bf16,
                n_rows,
                k_dim,
                expected_w_len,
                plan,
                trusted_live_k.min(k_dim),
            );
        }
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
    let mut t66_window_staged_blocks = 0usize;
    let mut t66_submitted_blocks = 0usize;
    let mut t66_finished_blocks = 0usize;
    let mut t66_compare_ok_blocks = 0usize;
    let mut t66_compared_rows = 0usize;
    let mut t66_disabled_after_failure = false;
    let mut t67_window_staged_blocks = 0usize;
    let mut t67_submitted_blocks = 0usize;
    let mut t67_finished_blocks = 0usize;
    let mut t67_compare_ok_blocks = 0usize;
    let mut t67_compared_rows = 0usize;
    let mut t67_disabled_after_failure = false;
    let mut t68_window_staged_blocks = 0usize;
    let mut t68_submitted_blocks = 0usize;
    let mut t68_finished_blocks = 0usize;
    let mut t68_compare_ok_blocks = 0usize;
    let mut t68_compared_rows = 0usize;
    let mut t68_disabled_after_failure = false;
    let mut t69_window_staged_blocks = 0usize;
    let mut t69_submitted_blocks = 0usize;
    let mut t69_finished_blocks = 0usize;
    let mut t69_compare_ok_blocks = 0usize;
    let mut t69_compared_rows = 0usize;
    let mut t69_disabled_after_failure = false;
    let mut t610_window_staged_blocks = 0usize;
    let mut t610_submitted_blocks = 0usize;
    let mut t610_finished_blocks = 0usize;
    let mut t610_compare_ok_blocks = 0usize;
    let mut t610_compared_rows = 0usize;
    let mut t610_disabled_after_failure = false;
    let mut t611_window_staged_blocks = 0usize;
    let mut t611_submitted_blocks = 0usize;
    let mut t611_finished_blocks = 0usize;
    let mut t611_compare_ok_blocks = 0usize;
    let mut t611_compared_rows = 0usize;
    let mut t611_disabled_after_failure = false;
    let mut twindow_staged_blocks = 0usize;
    let mut twindow_submitted_blocks = 0usize;
    let mut twindow_finished_blocks = 0usize;
    let mut twindow_compare_ok_blocks = 0usize;
    let mut twindow_compared_rows = 0usize;
    let mut twindow_disabled_after_failure = false;
    let mut twindow_frontier_rung = 0usize;
    let mut twindow_frontier_live_k = 0usize;
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
        log_strict_window_ladder_detail!(
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
            log_strict_window_ladder_detail!(
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
            log_strict_window_ladder_detail!(
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

            let t63_restore_live_k_dim = k_dim.min(trueos_eu::gfx12::T63_ACCUM16_HI_LIVE32_LIVE_K);
            let t63_restore = crate::intel::stage_gpgpu_tile_record_accum16_window_probe(
                stage.output_gpu,
                x,
                t62_rows,
                block_row_count,
                k_dim,
                trueos_eu::gfx12::T62_ROW_INDEXED_LIVE_K,
            );
            log_strict_window_ladder_detail!(
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
            log_strict_window_ladder_detail!(
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
            log_strict_window_ladder_detail!(
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
            log_strict_window_ladder_detail!(
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
            log_strict_window_ladder_detail!(
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
            log_strict_window_ladder_detail!(
                "lumen-gpu-proof: director-step step=20 backend=local-gpu mode=t6-5-windowed-accum16-live64-partial submitted={} finished={} readback_ok={} compare_ok={} reason={} program={} tile_index={} armed_tiles={} row_block={} global_row_start={} tile_row_start={} row_count={} output_gpu=0x{:X} compare_mask=0x{:08X} expected_mask=0x{:08X} first_gpu=0x{:08X} first_cpu_expected=0x{:08X} last_gpu=0x{:08X} last_cpu_expected=0x{:08X} lane_dispatch={} live_k_dim={} t64_first_gpu=0x{:08X} t64_first_cpu_expected=0x{:08X} finish_marker=0x{:08X} finish_expected=0x{:08X} batch_bytes=0x{:X} output_owner=cpu-ap action=hold-scale next=t6-6-windowed-accum16-live80-partial does_not_prove=full_model_matvec\n",
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
            if !t65.compare_ok {
                continue;
            }

            if t66_disabled_after_failure {
                if cgp_prefix.live_k_dim == t65.live_k_dim {
                    for local_row in 0..block_row_count.min(t65.output_words.len()) {
                        cgp_prefix.push_row(
                            global_row.saturating_add(local_row),
                            t65.output_words[local_row],
                        );
                    }
                }
                continue;
            }

            let t66_live_k_dim = k_dim.min(trueos_eu::gfx12::T66_WINDOWED_ACCUM16_LIVE80_LIVE_K);
            let t66_stage = crate::intel::stage_gpgpu_tile_record_accum16_window_probe(
                stage.output_gpu,
                x,
                t62_rows,
                block_row_count,
                k_dim,
                trueos_eu::gfx12::T66_WINDOWED_ACCUM16_LIVE80_WINDOW_START,
            );
            t66_window_staged_blocks += t66_stage.readback_ok as usize;
            log_strict_window_ladder_detail!(
                "lumen-gpu-proof: director-step step=21 backend=local-gpu mode=t6-6-windowed-accum16-live80-stage staged={} reason={} program={} tile_index={} armed_tiles={} row_block={} global_row_start={} tile_row_start={} row_count={} output_gpu=0x{:X} source_window={}..{} artifact_window=16..32 live_k_dim={} output_nonzero_dwords={} output_owner=cpu-ap next=t6-6-windowed-accum16-live80-partial does_not_prove=full_model_matvec\n",
                t66_stage.readback_ok as u8,
                t66_stage.reason,
                trueos_eu::gfx12::T66_WINDOWED_ACCUM16_LIVE80_PROGRAM_NAME,
                tile_index,
                armed_tiles,
                row_block_index,
                global_row,
                block_tile_row,
                t66_stage.row_count,
                t66_stage.output_gpu,
                trueos_eu::gfx12::T66_WINDOWED_ACCUM16_LIVE80_WINDOW_START,
                t66_live_k_dim,
                t66_live_k_dim,
                t66_stage.output_nonzero_dwords,
            );
            if !t66_stage.readback_ok {
                if cgp_prefix.live_k_dim == t65.live_k_dim {
                    for local_row in 0..block_row_count.min(t65.output_words.len()) {
                        cgp_prefix.push_row(
                            global_row.saturating_add(local_row),
                            t65.output_words[local_row],
                        );
                    }
                }
                continue;
            }

            let mut t66_expected_words = [0u32; 8];
            for local_row in 0..block_row_count {
                let row_start = local_row.saturating_mul(k_dim).saturating_mul(2);
                let row_end = row_start.saturating_add(k_dim.saturating_mul(2));
                t66_expected_words[local_row] =
                    bf16_row_dot_prefix(x, &t62_rows[row_start..row_end], t66_live_k_dim).to_bits();
            }
            let t66 = crate::intel::submit_gpgpu_t66_windowed_accum16_live80_partial_matvec_probe(
                stage.output_gpu,
                stage.output_bytes,
                t66_expected_words,
                block_row_count,
                t66_live_k_dim,
            );
            t66_submitted_blocks += t66.submitted as usize;
            t66_finished_blocks += t66.finished as usize;
            t66_compare_ok_blocks += t66.compare_ok as usize;
            t66_compared_rows =
                t66_compared_rows.saturating_add(if t66.compare_ok { block_row_count } else { 0 });
            last_gpu_value = t66.output_words[0];
            last_cpu_expected_bits = t66.expected_words[0];
            last_dispatch = t66.dispatch_delta;
            last_partial_rows = if t66.compare_ok {
                t66_compared_rows
            } else {
                t65_compared_rows
            };
            log_strict_window_ladder_detail!(
                "lumen-gpu-proof: director-step step=22 backend=local-gpu mode=t6-6-windowed-accum16-live80-partial submitted={} finished={} readback_ok={} compare_ok={} reason={} program={} tile_index={} armed_tiles={} row_block={} global_row_start={} tile_row_start={} row_count={} output_gpu=0x{:X} compare_mask=0x{:08X} expected_mask=0x{:08X} first_gpu=0x{:08X} first_cpu_expected=0x{:08X} last_gpu=0x{:08X} last_cpu_expected=0x{:08X} lane_dispatch={} live_k_dim={} t65_first_gpu=0x{:08X} t65_first_cpu_expected=0x{:08X} finish_marker=0x{:08X} finish_expected=0x{:08X} batch_bytes=0x{:X} output_owner=cpu-ap action=hold-scale next=t6-7-windowed-accum16-live96-partial does_not_prove=full_model_matvec\n",
                t66.submitted as u8,
                t66.finished as u8,
                t66.readback_ok as u8,
                t66.compare_ok as u8,
                t66.reason,
                t66.program_name,
                tile_index,
                armed_tiles,
                row_block_index,
                global_row,
                block_tile_row,
                t66.row_count,
                t66.output_gpu,
                t66.compare_mask,
                t66.expected_mask,
                t66.output_words[0],
                t66.expected_words[0],
                t66.output_words[t66.row_count.saturating_sub(1)],
                t66.expected_words[t66.row_count.saturating_sub(1)],
                t66.dispatch_delta,
                t66.live_k_dim,
                t65.output_words[0],
                t65.expected_words[0],
                t66.finish_marker,
                t66.expected_finish_marker,
                t66.batch_bytes,
            );
            if !t66.finished || !t66.readback_ok {
                t66_disabled_after_failure = true;
            }
            if !t66.compare_ok {
                if cgp_prefix.live_k_dim == t65.live_k_dim {
                    for local_row in 0..block_row_count.min(t65.output_words.len()) {
                        cgp_prefix.push_row(
                            global_row.saturating_add(local_row),
                            t65.output_words[local_row],
                        );
                    }
                }
                continue;
            }
            if t67_disabled_after_failure && t66.live_k_dim == t66_live_k_dim {
                if cgp_prefix.live_k_dim != t66.live_k_dim {
                    cgp_prefix.rows.clear();
                    cgp_prefix.live_k_dim = t66.live_k_dim;
                }
                for local_row in 0..block_row_count.min(t66.output_words.len()) {
                    cgp_prefix.push_row(
                        global_row.saturating_add(local_row),
                        t66.output_words[local_row],
                    );
                }
                continue;
            }

            let t67_live_k_dim = k_dim.min(trueos_eu::gfx12::T67_WINDOWED_ACCUM16_LIVE96_LIVE_K);
            let t67_stage = crate::intel::stage_gpgpu_tile_record_accum16_window_probe(
                stage.output_gpu,
                x,
                t62_rows,
                block_row_count,
                k_dim,
                trueos_eu::gfx12::T67_WINDOWED_ACCUM16_LIVE96_WINDOW_START,
            );
            t67_window_staged_blocks += t67_stage.readback_ok as usize;
            log_strict_window_ladder_detail!(
                "lumen-gpu-proof: director-step step=23 backend=local-gpu mode=t6-7-windowed-accum16-live96-stage staged={} reason={} program={} tile_index={} armed_tiles={} row_block={} global_row_start={} tile_row_start={} row_count={} output_gpu=0x{:X} source_window={}..{} artifact_window=16..32 live_k_dim={} output_nonzero_dwords={} output_owner=cpu-ap next=t6-7-windowed-accum16-live96-partial does_not_prove=full_model_matvec\n",
                t67_stage.readback_ok as u8,
                t67_stage.reason,
                trueos_eu::gfx12::T67_WINDOWED_ACCUM16_LIVE96_PROGRAM_NAME,
                tile_index,
                armed_tiles,
                row_block_index,
                global_row,
                block_tile_row,
                t67_stage.row_count,
                t67_stage.output_gpu,
                trueos_eu::gfx12::T67_WINDOWED_ACCUM16_LIVE96_WINDOW_START,
                t67_live_k_dim,
                t67_live_k_dim,
                t67_stage.output_nonzero_dwords,
            );
            if !t67_stage.readback_ok {
                if t66.live_k_dim == t66_live_k_dim {
                    if cgp_prefix.live_k_dim != t66.live_k_dim {
                        cgp_prefix.rows.clear();
                        cgp_prefix.live_k_dim = t66.live_k_dim;
                    }
                    for local_row in 0..block_row_count.min(t66.output_words.len()) {
                        cgp_prefix.push_row(
                            global_row.saturating_add(local_row),
                            t66.output_words[local_row],
                        );
                    }
                }
                continue;
            }

            let mut t67_expected_words = [0u32; 8];
            for local_row in 0..block_row_count {
                let row_start = local_row.saturating_mul(k_dim).saturating_mul(2);
                let row_end = row_start.saturating_add(k_dim.saturating_mul(2));
                t67_expected_words[local_row] =
                    bf16_row_dot_prefix(x, &t62_rows[row_start..row_end], t67_live_k_dim).to_bits();
            }
            let t67 = crate::intel::submit_gpgpu_t67_windowed_accum16_live96_partial_matvec_probe(
                stage.output_gpu,
                stage.output_bytes,
                t67_expected_words,
                block_row_count,
                t67_live_k_dim,
            );
            t67_submitted_blocks += t67.submitted as usize;
            t67_finished_blocks += t67.finished as usize;
            t67_compare_ok_blocks += t67.compare_ok as usize;
            t67_compared_rows =
                t67_compared_rows.saturating_add(if t67.compare_ok { block_row_count } else { 0 });
            last_gpu_value = t67.output_words[0];
            last_cpu_expected_bits = t67.expected_words[0];
            last_dispatch = t67.dispatch_delta;
            last_partial_rows = if t67.compare_ok {
                t67_compared_rows
            } else {
                t66_compared_rows
            };
            log_strict_window_ladder_detail!(
                "lumen-gpu-proof: director-step step=24 backend=local-gpu mode=t6-7-windowed-accum16-live96-partial submitted={} finished={} readback_ok={} compare_ok={} reason={} program={} tile_index={} armed_tiles={} row_block={} global_row_start={} tile_row_start={} row_count={} output_gpu=0x{:X} compare_mask=0x{:08X} expected_mask=0x{:08X} first_gpu=0x{:08X} first_cpu_expected=0x{:08X} last_gpu=0x{:08X} last_cpu_expected=0x{:08X} lane_dispatch={} live_k_dim={} t66_first_gpu=0x{:08X} t66_first_cpu_expected=0x{:08X} finish_marker=0x{:08X} finish_expected=0x{:08X} batch_bytes=0x{:X} output_owner=cpu-ap action=hold-scale next=t6-8-windowed-accum16-live112-partial does_not_prove=full_model_matvec\n",
                t67.submitted as u8,
                t67.finished as u8,
                t67.readback_ok as u8,
                t67.compare_ok as u8,
                t67.reason,
                t67.program_name,
                tile_index,
                armed_tiles,
                row_block_index,
                global_row,
                block_tile_row,
                t67.row_count,
                t67.output_gpu,
                t67.compare_mask,
                t67.expected_mask,
                t67.output_words[0],
                t67.expected_words[0],
                t67.output_words[t67.row_count.saturating_sub(1)],
                t67.expected_words[t67.row_count.saturating_sub(1)],
                t67.dispatch_delta,
                t67.live_k_dim,
                t66.output_words[0],
                t66.expected_words[0],
                t67.finish_marker,
                t67.expected_finish_marker,
                t67.batch_bytes,
            );
            if !t67.finished || !t67.readback_ok {
                t67_disabled_after_failure = true;
            }
            if !t67.compare_ok {
                if t66.live_k_dim == t66_live_k_dim {
                    if cgp_prefix.live_k_dim != t66.live_k_dim {
                        cgp_prefix.rows.clear();
                        cgp_prefix.live_k_dim = t66.live_k_dim;
                    }
                    for local_row in 0..block_row_count.min(t66.output_words.len()) {
                        cgp_prefix.push_row(
                            global_row.saturating_add(local_row),
                            t66.output_words[local_row],
                        );
                    }
                }
                continue;
            }
            if t68_disabled_after_failure && t67.live_k_dim == t67_live_k_dim {
                if cgp_prefix.live_k_dim != t67.live_k_dim {
                    cgp_prefix.rows.clear();
                    cgp_prefix.live_k_dim = t67.live_k_dim;
                }
                for local_row in 0..block_row_count.min(t67.output_words.len()) {
                    cgp_prefix.push_row(
                        global_row.saturating_add(local_row),
                        t67.output_words[local_row],
                    );
                }
                continue;
            }

            let t68_live_k_dim = k_dim.min(trueos_eu::gfx12::T68_WINDOWED_ACCUM16_LIVE112_LIVE_K);
            let t68_stage = crate::intel::stage_gpgpu_tile_record_accum16_window_probe(
                stage.output_gpu,
                x,
                t62_rows,
                block_row_count,
                k_dim,
                trueos_eu::gfx12::T68_WINDOWED_ACCUM16_LIVE112_WINDOW_START,
            );
            t68_window_staged_blocks += t68_stage.readback_ok as usize;
            log_strict_window_ladder_detail!(
                "lumen-gpu-proof: director-step step=25 backend=local-gpu mode=t6-8-windowed-accum16-live112-stage staged={} reason={} program={} tile_index={} armed_tiles={} row_block={} global_row_start={} tile_row_start={} row_count={} output_gpu=0x{:X} source_window={}..{} artifact_window=16..32 live_k_dim={} output_nonzero_dwords={} output_owner=cpu-ap next=t6-8-windowed-accum16-live112-partial does_not_prove=full_model_matvec\n",
                t68_stage.readback_ok as u8,
                t68_stage.reason,
                trueos_eu::gfx12::T68_WINDOWED_ACCUM16_LIVE112_PROGRAM_NAME,
                tile_index,
                armed_tiles,
                row_block_index,
                global_row,
                block_tile_row,
                t68_stage.row_count,
                t68_stage.output_gpu,
                trueos_eu::gfx12::T68_WINDOWED_ACCUM16_LIVE112_WINDOW_START,
                t68_live_k_dim,
                t68_live_k_dim,
                t68_stage.output_nonzero_dwords,
            );
            if !t68_stage.readback_ok {
                if cgp_prefix.live_k_dim != t67.live_k_dim {
                    cgp_prefix.rows.clear();
                    cgp_prefix.live_k_dim = t67.live_k_dim;
                }
                for local_row in 0..block_row_count.min(t67.output_words.len()) {
                    cgp_prefix.push_row(
                        global_row.saturating_add(local_row),
                        t67.output_words[local_row],
                    );
                }
                continue;
            }

            let mut t68_expected_words = [0u32; 8];
            for local_row in 0..block_row_count {
                let row_start = local_row.saturating_mul(k_dim).saturating_mul(2);
                let row_end = row_start.saturating_add(k_dim.saturating_mul(2));
                t68_expected_words[local_row] =
                    bf16_row_dot_prefix(x, &t62_rows[row_start..row_end], t68_live_k_dim).to_bits();
            }
            let t68 = crate::intel::submit_gpgpu_t68_windowed_accum16_live112_partial_matvec_probe(
                stage.output_gpu,
                stage.output_bytes,
                t68_expected_words,
                block_row_count,
                t68_live_k_dim,
            );
            t68_submitted_blocks += t68.submitted as usize;
            t68_finished_blocks += t68.finished as usize;
            t68_compare_ok_blocks += t68.compare_ok as usize;
            t68_compared_rows =
                t68_compared_rows.saturating_add(if t68.compare_ok { block_row_count } else { 0 });
            last_gpu_value = t68.output_words[0];
            last_cpu_expected_bits = t68.expected_words[0];
            last_dispatch = t68.dispatch_delta;
            last_partial_rows = if t68.compare_ok {
                t68_compared_rows
            } else {
                t67_compared_rows
            };
            log_strict_window_ladder_detail!(
                "lumen-gpu-proof: director-step step=26 backend=local-gpu mode=t6-8-windowed-accum16-live112-partial submitted={} finished={} readback_ok={} compare_ok={} reason={} program={} tile_index={} armed_tiles={} row_block={} global_row_start={} tile_row_start={} row_count={} output_gpu=0x{:X} compare_mask=0x{:08X} expected_mask=0x{:08X} first_gpu=0x{:08X} first_cpu_expected=0x{:08X} last_gpu=0x{:08X} last_cpu_expected=0x{:08X} lane_dispatch={} live_k_dim={} t67_first_gpu=0x{:08X} t67_first_cpu_expected=0x{:08X} finish_marker=0x{:08X} finish_expected=0x{:08X} batch_bytes=0x{:X} output_owner=cpu-ap action=hold-scale next=t6-9-windowed-accum16-live128-partial does_not_prove=full_model_matvec\n",
                t68.submitted as u8,
                t68.finished as u8,
                t68.readback_ok as u8,
                t68.compare_ok as u8,
                t68.reason,
                t68.program_name,
                tile_index,
                armed_tiles,
                row_block_index,
                global_row,
                block_tile_row,
                t68.row_count,
                t68.output_gpu,
                t68.compare_mask,
                t68.expected_mask,
                t68.output_words[0],
                t68.expected_words[0],
                t68.output_words[t68.row_count.saturating_sub(1)],
                t68.expected_words[t68.row_count.saturating_sub(1)],
                t68.dispatch_delta,
                t68.live_k_dim,
                t67.output_words[0],
                t67.expected_words[0],
                t68.finish_marker,
                t68.expected_finish_marker,
                t68.batch_bytes,
            );
            if !t68.finished || !t68.readback_ok {
                t68_disabled_after_failure = true;
            }
            if !t68.compare_ok {
                if t67.live_k_dim == t67_live_k_dim {
                    if cgp_prefix.live_k_dim != t67.live_k_dim {
                        cgp_prefix.rows.clear();
                        cgp_prefix.live_k_dim = t67.live_k_dim;
                    }
                    for local_row in 0..block_row_count.min(t67.output_words.len()) {
                        cgp_prefix.push_row(
                            global_row.saturating_add(local_row),
                            t67.output_words[local_row],
                        );
                    }
                }
                continue;
            }
            if t69_disabled_after_failure && t68.live_k_dim == t68_live_k_dim {
                if cgp_prefix.live_k_dim != t68.live_k_dim {
                    cgp_prefix.rows.clear();
                    cgp_prefix.live_k_dim = t68.live_k_dim;
                }
                for local_row in 0..block_row_count.min(t68.output_words.len()) {
                    cgp_prefix.push_row(
                        global_row.saturating_add(local_row),
                        t68.output_words[local_row],
                    );
                }
                continue;
            }

            let t69_live_k_dim = k_dim.min(trueos_eu::gfx12::T69_WINDOWED_ACCUM16_LIVE128_LIVE_K);
            let t69_stage = crate::intel::stage_gpgpu_tile_record_accum16_window_probe(
                stage.output_gpu,
                x,
                t62_rows,
                block_row_count,
                k_dim,
                trueos_eu::gfx12::T69_WINDOWED_ACCUM16_LIVE128_WINDOW_START,
            );
            t69_window_staged_blocks += t69_stage.readback_ok as usize;
            log_strict_window_ladder_detail!(
                "lumen-gpu-proof: director-step step=27 backend=local-gpu mode=t6-9-windowed-accum16-live128-stage staged={} reason={} program={} tile_index={} armed_tiles={} row_block={} global_row_start={} tile_row_start={} row_count={} output_gpu=0x{:X} source_window={}..{} artifact_window=16..32 live_k_dim={} output_nonzero_dwords={} output_owner=cpu-ap next=t6-9-windowed-accum16-live128-partial does_not_prove=full_model_matvec\n",
                t69_stage.readback_ok as u8,
                t69_stage.reason,
                trueos_eu::gfx12::T69_WINDOWED_ACCUM16_LIVE128_PROGRAM_NAME,
                tile_index,
                armed_tiles,
                row_block_index,
                global_row,
                block_tile_row,
                t69_stage.row_count,
                t69_stage.output_gpu,
                trueos_eu::gfx12::T69_WINDOWED_ACCUM16_LIVE128_WINDOW_START,
                t69_live_k_dim,
                t69_live_k_dim,
                t69_stage.output_nonzero_dwords,
            );
            if !t69_stage.readback_ok {
                if cgp_prefix.live_k_dim != t68.live_k_dim {
                    cgp_prefix.rows.clear();
                    cgp_prefix.live_k_dim = t68.live_k_dim;
                }
                for local_row in 0..block_row_count.min(t68.output_words.len()) {
                    cgp_prefix.push_row(
                        global_row.saturating_add(local_row),
                        t68.output_words[local_row],
                    );
                }
                continue;
            }

            let mut t69_expected_words = [0u32; 8];
            for local_row in 0..block_row_count {
                let row_start = local_row.saturating_mul(k_dim).saturating_mul(2);
                let row_end = row_start.saturating_add(k_dim.saturating_mul(2));
                t69_expected_words[local_row] =
                    bf16_row_dot_prefix(x, &t62_rows[row_start..row_end], t69_live_k_dim).to_bits();
            }
            let t69 = crate::intel::submit_gpgpu_t69_windowed_accum16_live128_partial_matvec_probe(
                stage.output_gpu,
                stage.output_bytes,
                t69_expected_words,
                block_row_count,
                t69_live_k_dim,
            );
            t69_submitted_blocks += t69.submitted as usize;
            t69_finished_blocks += t69.finished as usize;
            t69_compare_ok_blocks += t69.compare_ok as usize;
            t69_compared_rows =
                t69_compared_rows.saturating_add(if t69.compare_ok { block_row_count } else { 0 });
            last_gpu_value = t69.output_words[0];
            last_cpu_expected_bits = t69.expected_words[0];
            last_dispatch = t69.dispatch_delta;
            last_partial_rows = if t69.compare_ok {
                t69_compared_rows
            } else {
                t68_compared_rows
            };
            log_strict_window_ladder_detail!(
                "lumen-gpu-proof: director-step step=28 backend=local-gpu mode=t6-9-windowed-accum16-live128-partial submitted={} finished={} readback_ok={} compare_ok={} reason={} program={} tile_index={} armed_tiles={} row_block={} global_row_start={} tile_row_start={} row_count={} output_gpu=0x{:X} compare_mask=0x{:08X} expected_mask=0x{:08X} first_gpu=0x{:08X} first_cpu_expected=0x{:08X} last_gpu=0x{:08X} last_cpu_expected=0x{:08X} lane_dispatch={} live_k_dim={} t68_first_gpu=0x{:08X} t68_first_cpu_expected=0x{:08X} finish_marker=0x{:08X} finish_expected=0x{:08X} batch_bytes=0x{:X} output_owner=cpu-ap action=hold-scale next=t6-10-windowed-accum16-live144-partial does_not_prove=full_model_matvec\n",
                t69.submitted as u8,
                t69.finished as u8,
                t69.readback_ok as u8,
                t69.compare_ok as u8,
                t69.reason,
                t69.program_name,
                tile_index,
                armed_tiles,
                row_block_index,
                global_row,
                block_tile_row,
                t69.row_count,
                t69.output_gpu,
                t69.compare_mask,
                t69.expected_mask,
                t69.output_words[0],
                t69.expected_words[0],
                t69.output_words[t69.row_count.saturating_sub(1)],
                t69.expected_words[t69.row_count.saturating_sub(1)],
                t69.dispatch_delta,
                t69.live_k_dim,
                t68.output_words[0],
                t68.expected_words[0],
                t69.finish_marker,
                t69.expected_finish_marker,
                t69.batch_bytes,
            );
            if !t69.finished || !t69.readback_ok {
                t69_disabled_after_failure = true;
            }
            if !t69.compare_ok {
                if t68.live_k_dim == t68_live_k_dim {
                    if cgp_prefix.live_k_dim != t68.live_k_dim {
                        cgp_prefix.rows.clear();
                        cgp_prefix.live_k_dim = t68.live_k_dim;
                    }
                    for local_row in 0..block_row_count.min(t68.output_words.len()) {
                        cgp_prefix.push_row(
                            global_row.saturating_add(local_row),
                            t68.output_words[local_row],
                        );
                    }
                }
                continue;
            }
            if t610_disabled_after_failure && t69.live_k_dim == t69_live_k_dim {
                if cgp_prefix.live_k_dim != t69.live_k_dim {
                    cgp_prefix.rows.clear();
                    cgp_prefix.live_k_dim = t69.live_k_dim;
                }
                for local_row in 0..block_row_count.min(t69.output_words.len()) {
                    cgp_prefix.push_row(
                        global_row.saturating_add(local_row),
                        t69.output_words[local_row],
                    );
                }
                continue;
            }

            let t610_live_k_dim = k_dim.min(trueos_eu::gfx12::T610_WINDOWED_ACCUM16_LIVE144_LIVE_K);
            let t610_stage = crate::intel::stage_gpgpu_tile_record_accum16_window_probe(
                stage.output_gpu,
                x,
                t62_rows,
                block_row_count,
                k_dim,
                trueos_eu::gfx12::T610_WINDOWED_ACCUM16_LIVE144_WINDOW_START,
            );
            t610_window_staged_blocks += t610_stage.readback_ok as usize;
            log_strict_window_ladder_detail!(
                "lumen-gpu-proof: director-step step=29 backend=local-gpu mode=t6-10-windowed-accum16-live144-stage staged={} reason={} program={} tile_index={} armed_tiles={} row_block={} global_row_start={} tile_row_start={} row_count={} output_gpu=0x{:X} source_window={}..{} artifact_window=16..32 live_k_dim={} output_nonzero_dwords={} output_owner=cpu-ap next=t6-10-windowed-accum16-live144-partial does_not_prove=full_model_matvec\n",
                t610_stage.readback_ok as u8,
                t610_stage.reason,
                trueos_eu::gfx12::T610_WINDOWED_ACCUM16_LIVE144_PROGRAM_NAME,
                tile_index,
                armed_tiles,
                row_block_index,
                global_row,
                block_tile_row,
                t610_stage.row_count,
                t610_stage.output_gpu,
                trueos_eu::gfx12::T610_WINDOWED_ACCUM16_LIVE144_WINDOW_START,
                t610_live_k_dim,
                t610_live_k_dim,
                t610_stage.output_nonzero_dwords,
            );
            if !t610_stage.readback_ok {
                if cgp_prefix.live_k_dim != t69.live_k_dim {
                    cgp_prefix.rows.clear();
                    cgp_prefix.live_k_dim = t69.live_k_dim;
                }
                for local_row in 0..block_row_count.min(t69.output_words.len()) {
                    cgp_prefix.push_row(
                        global_row.saturating_add(local_row),
                        t69.output_words[local_row],
                    );
                }
                continue;
            }

            let mut t610_expected_words = [0u32; 8];
            for local_row in 0..block_row_count {
                let row_start = local_row.saturating_mul(k_dim).saturating_mul(2);
                let row_end = row_start.saturating_add(k_dim.saturating_mul(2));
                t610_expected_words[local_row] =
                    bf16_row_dot_prefix(x, &t62_rows[row_start..row_end], t610_live_k_dim)
                        .to_bits();
            }
            let t610 =
                crate::intel::submit_gpgpu_t610_windowed_accum16_live144_partial_matvec_probe(
                    stage.output_gpu,
                    stage.output_bytes,
                    t610_expected_words,
                    block_row_count,
                    t610_live_k_dim,
                );
            t610_submitted_blocks += t610.submitted as usize;
            t610_finished_blocks += t610.finished as usize;
            t610_compare_ok_blocks += t610.compare_ok as usize;
            t610_compared_rows = t610_compared_rows.saturating_add(if t610.compare_ok {
                block_row_count
            } else {
                0
            });
            last_gpu_value = t610.output_words[0];
            last_cpu_expected_bits = t610.expected_words[0];
            last_dispatch = t610.dispatch_delta;
            last_partial_rows = if t610.compare_ok {
                t610_compared_rows
            } else {
                t69_compared_rows
            };
            log_strict_window_ladder_detail!(
                "lumen-gpu-proof: director-step step=30 backend=local-gpu mode=t6-10-windowed-accum16-live144-partial submitted={} finished={} readback_ok={} compare_ok={} reason={} program={} tile_index={} armed_tiles={} row_block={} global_row_start={} tile_row_start={} row_count={} output_gpu=0x{:X} compare_mask=0x{:08X} expected_mask=0x{:08X} first_gpu=0x{:08X} first_cpu_expected=0x{:08X} last_gpu=0x{:08X} last_cpu_expected=0x{:08X} lane_dispatch={} live_k_dim={} t69_first_gpu=0x{:08X} t69_first_cpu_expected=0x{:08X} finish_marker=0x{:08X} finish_expected=0x{:08X} batch_bytes=0x{:X} output_owner=cpu-ap action=hold-scale next=t6-11-windowed-accum16-live160-partial does_not_prove=full_model_matvec\n",
                t610.submitted as u8,
                t610.finished as u8,
                t610.readback_ok as u8,
                t610.compare_ok as u8,
                t610.reason,
                t610.program_name,
                tile_index,
                armed_tiles,
                row_block_index,
                global_row,
                block_tile_row,
                t610.row_count,
                t610.output_gpu,
                t610.compare_mask,
                t610.expected_mask,
                t610.output_words[0],
                t610.expected_words[0],
                t610.output_words[t610.row_count.saturating_sub(1)],
                t610.expected_words[t610.row_count.saturating_sub(1)],
                t610.dispatch_delta,
                t610.live_k_dim,
                t69.output_words[0],
                t69.expected_words[0],
                t610.finish_marker,
                t610.expected_finish_marker,
                t610.batch_bytes,
            );
            if !t610.finished || !t610.readback_ok {
                t610_disabled_after_failure = true;
            }
            if !t610.compare_ok {
                if t69.live_k_dim == t69_live_k_dim {
                    if cgp_prefix.live_k_dim != t69.live_k_dim {
                        cgp_prefix.rows.clear();
                        cgp_prefix.live_k_dim = t69.live_k_dim;
                    }
                    for local_row in 0..block_row_count.min(t69.output_words.len()) {
                        cgp_prefix.push_row(
                            global_row.saturating_add(local_row),
                            t69.output_words[local_row],
                        );
                    }
                }
                continue;
            }
            if t611_disabled_after_failure && t610.live_k_dim == t610_live_k_dim {
                if cgp_prefix.live_k_dim != t610.live_k_dim {
                    cgp_prefix.rows.clear();
                    cgp_prefix.live_k_dim = t610.live_k_dim;
                }
                for local_row in 0..block_row_count.min(t610.output_words.len()) {
                    cgp_prefix.push_row(
                        global_row.saturating_add(local_row),
                        t610.output_words[local_row],
                    );
                }
                continue;
            }

            let t611_live_k_dim = k_dim.min(trueos_eu::gfx12::T611_WINDOWED_ACCUM16_LIVE160_LIVE_K);
            let t611_stage = crate::intel::stage_gpgpu_tile_record_accum16_window_probe(
                stage.output_gpu,
                x,
                t62_rows,
                block_row_count,
                k_dim,
                trueos_eu::gfx12::T611_WINDOWED_ACCUM16_LIVE160_WINDOW_START,
            );
            t611_window_staged_blocks += t611_stage.readback_ok as usize;
            log_strict_window_ladder_detail!(
                "lumen-gpu-proof: director-step step=31 backend=local-gpu mode=t6-11-windowed-accum16-live160-stage staged={} reason={} program={} tile_index={} armed_tiles={} row_block={} global_row_start={} tile_row_start={} row_count={} output_gpu=0x{:X} source_window={}..{} artifact_window=16..32 live_k_dim={} output_nonzero_dwords={} output_owner=cpu-ap next=t6-11-windowed-accum16-live160-partial does_not_prove=full_model_matvec\n",
                t611_stage.readback_ok as u8,
                t611_stage.reason,
                trueos_eu::gfx12::T611_WINDOWED_ACCUM16_LIVE160_PROGRAM_NAME,
                tile_index,
                armed_tiles,
                row_block_index,
                global_row,
                block_tile_row,
                t611_stage.row_count,
                t611_stage.output_gpu,
                trueos_eu::gfx12::T611_WINDOWED_ACCUM16_LIVE160_WINDOW_START,
                t611_live_k_dim,
                t611_live_k_dim,
                t611_stage.output_nonzero_dwords,
            );
            if !t611_stage.readback_ok {
                if cgp_prefix.live_k_dim != t610.live_k_dim {
                    cgp_prefix.rows.clear();
                    cgp_prefix.live_k_dim = t610.live_k_dim;
                }
                for local_row in 0..block_row_count.min(t610.output_words.len()) {
                    cgp_prefix.push_row(
                        global_row.saturating_add(local_row),
                        t610.output_words[local_row],
                    );
                }
                continue;
            }

            let mut t611_expected_words = [0u32; 8];
            for local_row in 0..block_row_count {
                let row_start = local_row.saturating_mul(k_dim).saturating_mul(2);
                let row_end = row_start.saturating_add(k_dim.saturating_mul(2));
                t611_expected_words[local_row] =
                    bf16_row_dot_prefix(x, &t62_rows[row_start..row_end], t611_live_k_dim)
                        .to_bits();
            }
            let t611 =
                crate::intel::submit_gpgpu_t611_windowed_accum16_live160_partial_matvec_probe(
                    stage.output_gpu,
                    stage.output_bytes,
                    t611_expected_words,
                    block_row_count,
                    t611_live_k_dim,
                );
            t611_submitted_blocks += t611.submitted as usize;
            t611_finished_blocks += t611.finished as usize;
            t611_compare_ok_blocks += t611.compare_ok as usize;
            t611_compared_rows = t611_compared_rows.saturating_add(if t611.compare_ok {
                block_row_count
            } else {
                0
            });
            last_gpu_value = t611.output_words[0];
            last_cpu_expected_bits = t611.expected_words[0];
            last_dispatch = t611.dispatch_delta;
            last_partial_rows = if t611.compare_ok {
                t611_compared_rows
            } else {
                t610_compared_rows
            };
            log_strict_window_ladder_detail!(
                "lumen-gpu-proof: director-step step=32 backend=local-gpu mode=t6-11-windowed-accum16-live160-partial submitted={} finished={} readback_ok={} compare_ok={} reason={} program={} tile_index={} armed_tiles={} row_block={} global_row_start={} tile_row_start={} row_count={} output_gpu=0x{:X} compare_mask=0x{:08X} expected_mask=0x{:08X} first_gpu=0x{:08X} first_cpu_expected=0x{:08X} last_gpu=0x{:08X} last_cpu_expected=0x{:08X} lane_dispatch={} live_k_dim={} t610_first_gpu=0x{:08X} t610_first_cpu_expected=0x{:08X} finish_marker=0x{:08X} finish_expected=0x{:08X} batch_bytes=0x{:X} output_owner=cpu-ap action=offer-accepted-prefix next=cpu-suffix-finish-or-scale-live-k does_not_prove=full_model_matvec\n",
                t611.submitted as u8,
                t611.finished as u8,
                t611.readback_ok as u8,
                t611.compare_ok as u8,
                t611.reason,
                t611.program_name,
                tile_index,
                armed_tiles,
                row_block_index,
                global_row,
                block_tile_row,
                t611.row_count,
                t611.output_gpu,
                t611.compare_mask,
                t611.expected_mask,
                t611.output_words[0],
                t611.expected_words[0],
                t611.output_words[t611.row_count.saturating_sub(1)],
                t611.expected_words[t611.row_count.saturating_sub(1)],
                t611.dispatch_delta,
                t611.live_k_dim,
                t610.output_words[0],
                t610.expected_words[0],
                t611.finish_marker,
                t611.expected_finish_marker,
                t611.batch_bytes,
            );
            if !t611.finished || !t611.readback_ok {
                t611_disabled_after_failure = true;
            }
            if t611.compare_ok && t611.live_k_dim == t611_live_k_dim {
                let mut best_live_k_dim = t611.live_k_dim;
                let mut best_output_words = t611.output_words;
                let mut best_expected_words = t611.expected_words;
                let mut best_dispatch = t611.dispatch_delta;
                let mut best_rung = 11usize;

                for rung in CGP_WINDOWED_ACCUM16_EXTRA_RUNGS {
                    let rung_live_k_dim = k_dim.min(rung.live_k_dim);
                    if rung_live_k_dim != rung.live_k_dim || rung.window_start != best_live_k_dim {
                        break;
                    }

                    let rung_stage = crate::intel::stage_gpgpu_tile_record_accum16_window_probe(
                        stage.output_gpu,
                        x,
                        t62_rows,
                        block_row_count,
                        k_dim,
                        rung.window_start,
                    );
                    twindow_staged_blocks += rung_stage.readback_ok as usize;
                    log_strict_window_ladder_detail!(
                        "lumen-gpu-proof: director-step step=33 backend=local-gpu mode=t6-windowed-accum16-extra-stage rung={} staged={} reason={} program={} tile_index={} armed_tiles={} row_block={} global_row_start={} tile_row_start={} row_count={} output_gpu=0x{:X} source_window={}..{} artifact_window=16..32 previous_live_k_dim={} live_k_dim={} output_nonzero_dwords={} output_owner=cpu-ap next=t6-windowed-accum16-extra-partial does_not_prove=full_model_matvec\n",
                        rung.rung,
                        rung_stage.readback_ok as u8,
                        rung_stage.reason,
                        rung.program_name,
                        tile_index,
                        armed_tiles,
                        row_block_index,
                        global_row,
                        block_tile_row,
                        rung_stage.row_count,
                        rung_stage.output_gpu,
                        rung.window_start,
                        rung_live_k_dim,
                        best_live_k_dim,
                        rung_live_k_dim,
                        rung_stage.output_nonzero_dwords,
                    );
                    if !rung_stage.readback_ok {
                        twindow_disabled_after_failure = true;
                        break;
                    }

                    let mut rung_expected_words = [0u32; 8];
                    for local_row in 0..block_row_count {
                        let row_start = local_row.saturating_mul(k_dim).saturating_mul(2);
                        let row_end = row_start.saturating_add(k_dim.saturating_mul(2));
                        rung_expected_words[local_row] =
                            bf16_row_dot_prefix(x, &t62_rows[row_start..row_end], rung_live_k_dim)
                                .to_bits();
                    }
                    let rung_proof =
                        crate::intel::submit_gpgpu_windowed_accum16_partial_matvec_probe(
                            rung.program_name,
                            stage.output_gpu,
                            stage.output_bytes,
                            rung_expected_words,
                            block_row_count,
                            rung_live_k_dim,
                        );
                    twindow_submitted_blocks += rung_proof.submitted as usize;
                    twindow_finished_blocks += rung_proof.finished as usize;
                    twindow_compare_ok_blocks += rung_proof.compare_ok as usize;
                    twindow_compared_rows =
                        twindow_compared_rows.saturating_add(if rung_proof.compare_ok {
                            block_row_count
                        } else {
                            0
                        });
                    last_gpu_value = rung_proof.output_words[0];
                    last_cpu_expected_bits = rung_proof.expected_words[0];
                    last_dispatch = rung_proof.dispatch_delta;
                    last_partial_rows = if rung_proof.compare_ok {
                        twindow_compared_rows
                    } else {
                        t611_compared_rows
                    };
                    log_strict_window_ladder_detail!(
                        "lumen-gpu-proof: director-step step=34 backend=local-gpu mode=t6-windowed-accum16-extra-partial rung={} submitted={} finished={} readback_ok={} compare_ok={} reason={} program={} tile_index={} armed_tiles={} row_block={} global_row_start={} tile_row_start={} row_count={} output_gpu=0x{:X} compare_mask=0x{:08X} expected_mask=0x{:08X} first_gpu=0x{:08X} first_cpu_expected=0x{:08X} last_gpu=0x{:08X} last_cpu_expected=0x{:08X} lane_dispatch={} previous_live_k_dim={} live_k_dim={} previous_first_gpu=0x{:08X} previous_first_cpu_expected=0x{:08X} finish_marker=0x{:08X} finish_expected=0x{:08X} batch_bytes=0x{:X} output_owner=cpu-ap action={} next=cpu-suffix-finish-or-scale-live-k does_not_prove=full_model_matvec\n",
                        rung.rung,
                        rung_proof.submitted as u8,
                        rung_proof.finished as u8,
                        rung_proof.readback_ok as u8,
                        rung_proof.compare_ok as u8,
                        rung_proof.reason,
                        rung_proof.program_name,
                        tile_index,
                        armed_tiles,
                        row_block_index,
                        global_row,
                        block_tile_row,
                        rung_proof.row_count,
                        rung_proof.output_gpu,
                        rung_proof.compare_mask,
                        rung_proof.expected_mask,
                        rung_proof.output_words[0],
                        rung_proof.expected_words[0],
                        rung_proof.output_words[rung_proof.row_count.saturating_sub(1)],
                        rung_proof.expected_words[rung_proof.row_count.saturating_sub(1)],
                        rung_proof.dispatch_delta,
                        best_live_k_dim,
                        rung_proof.live_k_dim,
                        best_output_words[0],
                        best_expected_words[0],
                        rung_proof.finish_marker,
                        rung_proof.expected_finish_marker,
                        rung_proof.batch_bytes,
                        if rung_proof.compare_ok {
                            "advance-frontier"
                        } else {
                            "hold-last-frontier"
                        },
                    );
                    if !rung_proof.finished || !rung_proof.readback_ok {
                        twindow_disabled_after_failure = true;
                        break;
                    }
                    if rung_proof.compare_ok && rung_proof.live_k_dim == rung_live_k_dim {
                        best_live_k_dim = rung_proof.live_k_dim;
                        best_output_words = rung_proof.output_words;
                        best_expected_words = rung_proof.expected_words;
                        best_dispatch = rung_proof.dispatch_delta;
                        best_rung = rung.rung;
                        twindow_frontier_rung = rung.rung;
                        twindow_frontier_live_k = rung_proof.live_k_dim;
                    } else {
                        twindow_disabled_after_failure = true;
                        break;
                    }
                }

                if cgp_prefix.live_k_dim != best_live_k_dim {
                    cgp_prefix.rows.clear();
                    cgp_prefix.live_k_dim = best_live_k_dim;
                }
                for local_row in 0..block_row_count.min(best_output_words.len()) {
                    cgp_prefix.push_row(
                        global_row.saturating_add(local_row),
                        best_output_words[local_row],
                    );
                }
                last_gpu_value = best_output_words[0];
                last_cpu_expected_bits = best_expected_words[0];
                last_dispatch = best_dispatch;
                twindow_frontier_rung = best_rung;
                twindow_frontier_live_k = best_live_k_dim;
                if best_rung >= 33 && best_live_k_dim >= 512 {
                    TRUSTED_WINDOW_FRONTIER_RUNG.store(best_rung, Ordering::Release);
                    TRUSTED_WINDOW_FRONTIER_LIVE_K.store(best_live_k_dim, Ordering::Release);
                }
            } else if t610.live_k_dim == t610_live_k_dim {
                if cgp_prefix.live_k_dim != t610.live_k_dim {
                    cgp_prefix.rows.clear();
                    cgp_prefix.live_k_dim = t610.live_k_dim;
                }
                for local_row in 0..block_row_count.min(t610.output_words.len()) {
                    cgp_prefix.push_row(
                        global_row.saturating_add(local_row),
                        t610.output_words[local_row],
                    );
                }
            }
        }
    }

    crate::log!(
        "lumen-gpu-proof: director-step step=16 backend=local-gpu source={} mode=t6-windowed-live512-actual-work-row-blocks upfront_proven_stages={} armed_tiles={} staged_tiles={} t5_submitted_tiles={} t5_finished_tiles={} t5_compare_ok_tiles={} t6_submitted_tiles={} t6_finished_tiles={} t6_compare_ok_tiles={} t61_submitted_tiles={} t61_finished_tiles={} t61_compare_ok_tiles={} t62_staged_blocks={} t62_submitted_blocks={} t62_finished_blocks={} t62_compare_ok_blocks={} t62_compared_rows={} t63_submitted_blocks={} t63_finished_blocks={} t63_compare_ok_blocks={} t63_compared_rows={} t64_window_staged_blocks={} t64_submitted_blocks={} t64_finished_blocks={} t64_compare_ok_blocks={} t64_compared_rows={} t65_window_staged_blocks={} t65_submitted_blocks={} t65_finished_blocks={} t65_compare_ok_blocks={} t65_compared_rows={} t66_window_staged_blocks={} t66_submitted_blocks={} t66_finished_blocks={} t66_compare_ok_blocks={} t66_compared_rows={} t66_disabled_after_failure={} t67_window_staged_blocks={} t67_submitted_blocks={} t67_finished_blocks={} t67_compare_ok_blocks={} t67_compared_rows={} t67_disabled_after_failure={} t68_window_staged_blocks={} t68_submitted_blocks={} t68_finished_blocks={} t68_compare_ok_blocks={} t68_compared_rows={} t68_disabled_after_failure={} t69_window_staged_blocks={} t69_submitted_blocks={} t69_finished_blocks={} t69_compare_ok_blocks={} t69_compared_rows={} t69_disabled_after_failure={} t610_window_staged_blocks={} t610_submitted_blocks={} t610_finished_blocks={} t610_compare_ok_blocks={} t610_compared_rows={} t610_disabled_after_failure={} t611_window_staged_blocks={} t611_submitted_blocks={} t611_finished_blocks={} t611_compare_ok_blocks={} t611_compared_rows={} t611_disabled_after_failure={} twindow_staged_blocks={} twindow_submitted_blocks={} twindow_finished_blocks={} twindow_compare_ok_blocks={} twindow_compared_rows={} twindow_disabled_after_failure={} twindow_frontier_rung={} twindow_frontier_live_k={} first_row=0 last_row={} row_block_rows={} row_block_cap={} partial_rows={} tile_rows={} k_dim={} t5_artifact=gfx12-t5-small-live4-packed-bf16-dot t6_artifact=gfx12-t6-small-live8-packed-bf16-dot t61_artifact=gfx12-t6-1-live16-packed-bf16-dot t62_artifact=gfx12-t6-2-lane-indexed-live16-packed-bf16-dot t63_artifact=gfx12-t6-3-accum16-hi-live32-packed-bf16-dot t64_artifact=gfx12-t6-4-windowed-accum16-live48-packed-bf16-dot t65_artifact=gfx12-t6-5-windowed-accum16-live64-packed-bf16-dot t66_artifact=gfx12-t6-6-windowed-accum16-live80-packed-bf16-dot t67_artifact=gfx12-t6-7-windowed-accum16-live96-packed-bf16-dot t68_artifact=gfx12-t6-8-windowed-accum16-live112-packed-bf16-dot t69_artifact=gfx12-t6-9-windowed-accum16-live128-packed-bf16-dot t610_artifact=gfx12-t6-10-windowed-accum16-live144-packed-bf16-dot t611_artifact=gfx12-t6-11-windowed-accum16-live160-packed-bf16-dot extra_window_artifacts=t6-12..t6-33 artifact_addressing=row-block-restaged-tile-record-prefix proof_role=actual-work-row-block-frontier cgp_mode={} cgp_prefix_rows={} cgp_prefix_live_k={} last_gpu_value=0x{:08X} last_cpu_expected_bits=0x{:08X} last_lane_dispatch={} output_owner=cpu-ap action=offer-accepted-prefix next=cpu-suffix-finish-or-scale-live-k does_not_prove=full_model_matvec\n",
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
        t66_window_staged_blocks,
        t66_submitted_blocks,
        t66_finished_blocks,
        t66_compare_ok_blocks,
        t66_compared_rows,
        t66_disabled_after_failure as u8,
        t67_window_staged_blocks,
        t67_submitted_blocks,
        t67_finished_blocks,
        t67_compare_ok_blocks,
        t67_compared_rows,
        t67_disabled_after_failure as u8,
        t68_window_staged_blocks,
        t68_submitted_blocks,
        t68_finished_blocks,
        t68_compare_ok_blocks,
        t68_compared_rows,
        t68_disabled_after_failure as u8,
        t69_window_staged_blocks,
        t69_submitted_blocks,
        t69_finished_blocks,
        t69_compare_ok_blocks,
        t69_compared_rows,
        t69_disabled_after_failure as u8,
        t610_window_staged_blocks,
        t610_submitted_blocks,
        t610_finished_blocks,
        t610_compare_ok_blocks,
        t610_compared_rows,
        t610_disabled_after_failure as u8,
        t611_window_staged_blocks,
        t611_submitted_blocks,
        t611_finished_blocks,
        t611_compare_ok_blocks,
        t611_compared_rows,
        t611_disabled_after_failure as u8,
        twindow_staged_blocks,
        twindow_submitted_blocks,
        twindow_finished_blocks,
        twindow_compare_ok_blocks,
        twindow_compared_rows,
        twindow_disabled_after_failure as u8,
        twindow_frontier_rung,
        twindow_frontier_live_k,
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

fn observe_trusted_window_frontier(
    x: &[f32],
    w_rowmajor_bf16: &[u8],
    n_rows: usize,
    k_dim: usize,
    _expected_w_len: usize,
    plan: LocalGpuProofPlan,
    trusted_live_k: usize,
) -> crate::lumen::cgp::CgpBf16PrefixContribution {
    let gpu = crate::intel::gpgpu_preflight_status();
    let proof_tile_rows = gpu.tile_rows.max(1);
    let armed_tiles = n_rows.div_ceil(proof_tile_rows).min(gpu.max_tiles);
    if armed_tiles == 0 || trusted_live_k == 0 {
        return crate::lumen::cgp::CgpBf16PrefixContribution::none();
    }

    let mut prefix = crate::lumen::cgp::CgpBf16PrefixContribution::accepted_prefix(trusted_live_k);
    let x_checksum = checksum_f32_prefix(x, k_dim);
    let mut submitted_blocks = 0usize;
    let mut t8_live16_submitted_blocks = 0usize;
    let mut t8_live16_accepted_blocks = 0usize;
    let mut t8_live16_accepted_rows = 0usize;
    let mut residual_t62_submitted_blocks = 0usize;
    let mut accepted_blocks = 0usize;
    let mut accepted_rows = 0usize;
    let mut last_rung = 0usize;
    let mut last_live_k = 0usize;
    let mut skipped_output_readbacks = 0usize;
    let mut failed = false;
    let mut t8_live16_carry_row_end = 0usize;

    for tile_index in 0..armed_tiles {
        let tile_row = tile_index.saturating_mul(proof_tile_rows);
        if tile_row >= n_rows {
            break;
        }
        let Some(row_offset) = tile_row
            .checked_mul(k_dim)
            .and_then(|values| values.checked_mul(2))
        else {
            failed = true;
            break;
        };
        let row_bytes = k_dim.saturating_mul(2);
        let row_end = row_offset.saturating_add(row_bytes);
        if row_end > w_rowmajor_bf16.len() {
            failed = true;
            break;
        }

        let row_bf16 = &w_rowmajor_bf16[row_offset..row_end];
        let stage = crate::intel::stage_gpgpu_one_tile_record_probe(
            x,
            row_bf16,
            k_dim,
            tile_row,
            x_checksum,
            checksum_bytes(row_bf16),
            0,
        );
        if !stage.staged {
            failed = true;
            continue;
        }

        let t62_block_rows = trueos_eu::gfx12::T62_ROW_INDEXED_PARTIAL_ROWS
            .min(proof_tile_rows)
            .min(n_rows.saturating_sub(tile_row));
        if t62_block_rows == 0 {
            continue;
        }
        let tile_remaining_rows = proof_tile_rows.min(n_rows.saturating_sub(tile_row));
        let t62_target_rows = tile_remaining_rows
            .min(t62_block_rows.saturating_mul(T62_ROW_BLOCK_DISPATCH_BLOCK_CAP));
        let t62_block_count = t62_target_rows.div_ceil(t62_block_rows);

        for row_block_index in 0..t62_block_count {
            let block_tile_row = row_block_index.saturating_mul(t62_block_rows);
            let global_row = tile_row.saturating_add(block_tile_row);
            let block_row_count =
                t62_block_rows.min(t62_target_rows.saturating_sub(block_tile_row));
            let block_row_offset =
                row_offset.saturating_add(block_tile_row.saturating_mul(k_dim).saturating_mul(2));
            let block_rows_bytes = block_row_count.saturating_mul(k_dim).saturating_mul(2);
            let block_rows_end = block_row_offset.saturating_add(block_rows_bytes);
            if block_row_count == 0 || block_rows_end > w_rowmajor_bf16.len() {
                continue;
            }

            let t62_live_k = k_dim.min(trueos_eu::gfx12::T62_ROW_INDEXED_LIVE_K);
            let rows = &w_rowmajor_bf16[block_row_offset..block_rows_end];
            let t8_frontier_rows = T8_GROUPID_FRONTIER_ROWS.load(Ordering::Acquire);
            let t8_live16_rows = t8_frontier_rows
                .min(tile_remaining_rows.saturating_sub(block_tile_row))
                .min(32);
            let t8_live16_carried = global_row < t8_live16_carry_row_end;
            if t8_live16_carried {
                let t8_src_row = block_tile_row % t8_live16_rows.max(block_row_count);
                let output_stage = crate::intel::stage_gpgpu_tile_record_output_rows_trusted(
                    stage.output_gpu,
                    t8_src_row,
                    0,
                    block_row_count,
                );
                if !output_stage.readback_ok {
                    failed = true;
                    continue;
                }
            } else {
                let t62_stage = crate::intel::stage_gpgpu_tile_record_rows_trusted(
                    stage.output_gpu,
                    rows,
                    block_row_count,
                    k_dim,
                );
                if !t62_stage.readback_ok {
                    failed = true;
                    continue;
                }
            }
            if !LOGGED_T8_BATCH2_RETIRE_PROBE.swap(true, Ordering::AcqRel) {
                let max_t8_rung_rows = T8_GROUPID_ROW_SCALE_RUNGS
                    .last()
                    .copied()
                    .unwrap_or(block_row_count);
                let t8_probe_row_count = tile_remaining_rows
                    .saturating_sub(block_tile_row)
                    .min(max_t8_rung_rows)
                    .min(32);
                let t8_probe_rows_bytes =
                    t8_probe_row_count.saturating_mul(k_dim).saturating_mul(2);
                let t8_probe_rows_end = block_row_offset.saturating_add(t8_probe_rows_bytes);
                let t8_rows = if t8_probe_row_count > block_row_count
                    && t8_probe_rows_end <= w_rowmajor_bf16.len()
                {
                    let t8_rows = &w_rowmajor_bf16[block_row_offset..t8_probe_rows_end];
                    let t8_stage = crate::intel::stage_gpgpu_tile_record_rows_trusted(
                        stage.output_gpu,
                        t8_rows,
                        t8_probe_row_count,
                        k_dim,
                    );
                    if t8_stage.readback_ok { t8_rows } else { rows }
                } else {
                    rows
                };
                let t8_probe_row_count = t8_probe_row_count.min(t8_rows.len() / (k_dim * 2));
                let mut t8_expected_words = [0u32; 8];
                let mut t8_expected_words16 = [0u32; 16];
                let mut t8_expected_words32 = [0u32; 32];
                for local_row in 0..t8_probe_row_count.min(t8_expected_words32.len()) {
                    let row_start = local_row.saturating_mul(k_dim).saturating_mul(2);
                    let row_end = row_start.saturating_add(k_dim.saturating_mul(2));
                    t8_expected_words32[local_row] =
                        bf16_row_dot_prefix(x, &t8_rows[row_start..row_end], t62_live_k).to_bits();
                    if local_row < t8_expected_words16.len() {
                        t8_expected_words16[local_row] = t8_expected_words32[local_row];
                    }
                    if local_row < t8_expected_words.len() {
                        t8_expected_words[local_row] = t8_expected_words32[local_row];
                    }
                }
                let t8 = crate::intel::submit_gpgpu_t8_batch2_rowblock_retire_probe(
                    stage.output_gpu,
                    stage.output_bytes,
                    t8_expected_words,
                    block_row_count,
                    t62_live_k,
                );
                crate::log!(
                    "lumen-gpu-proof: director-step step=40 backend=local-gpu mode=t8-batch2-rowblock-retire submitted={} finished={} readback_ok={} compare_ok={} reason={} program={} groups=2 row_count={} live_k_dim={} expected_lane_dispatch=16 observed_lane_dispatch={} first_gpu=0x{:08X} first_cpu_expected=0x{:08X} finish_marker=0x{:08X} finish_expected=0x{:08X} batch_bytes=0x{:X} action={} next=t8-groupid-distinct-row-output does_not_prove=distinct_row_block_ownership_or_full_model_matvec\n",
                    t8.submitted as u8,
                    t8.finished as u8,
                    t8.readback_ok as u8,
                    t8.compare_ok as u8,
                    t8.reason,
                    t8.program_name,
                    t8.row_count,
                    t8.live_k_dim,
                    t8.dispatch_delta,
                    t8.output_words[0],
                    t8.expected_words[0],
                    t8.finish_marker,
                    t8.expected_finish_marker,
                    t8.batch_bytes,
                    if t8.readback_ok {
                        "advance-frontier"
                    } else {
                        "hold-frontier"
                    },
                );
                if t8.readback_ok {
                    let mut last_t8_row_count = 0usize;
                    let mut last_t8_dispatch = 0u64;
                    let mut t8_scale_failed = false;
                    for &t8_rung_rows in T8_GROUPID_ROW_SCALE_RUNGS {
                        let t8_distinct_row_count = t8_probe_row_count.min(t8_rung_rows);
                        if t8_distinct_row_count == 0 || t8_distinct_row_count == last_t8_row_count
                        {
                            continue;
                        }
                        let t8_distinct = if t8_distinct_row_count <= t8_expected_words.len() {
                            crate::intel::submit_gpgpu_t8_groupid_live16_distinct_row_probe(
                                stage.output_gpu,
                                stage.output_bytes,
                                t8_expected_words,
                                t8_distinct_row_count,
                                t62_live_k,
                            )
                        } else if t8_distinct_row_count <= t8_expected_words16.len() {
                            crate::intel::submit_gpgpu_t8_groupid_live16_distinct_row16_probe(
                                stage.output_gpu,
                                stage.output_bytes,
                                t8_expected_words16,
                                t8_distinct_row_count,
                                t62_live_k,
                            )
                        } else {
                            crate::intel::submit_gpgpu_t8_groupid_live16_distinct_row32_probe(
                                stage.output_gpu,
                                stage.output_bytes,
                                t8_expected_words32,
                                t8_distinct_row_count,
                                t62_live_k,
                            )
                        };
                        let t8_last_index = t8_distinct_row_count.saturating_sub(1);
                        let t8_last_gpu = if t8_last_index < t8_distinct.output_words.len() {
                            t8_distinct.output_words[t8_last_index]
                        } else if t8_last_index < t8_distinct.output_words16.len() {
                            t8_distinct.output_words16[t8_last_index]
                        } else {
                            t8_distinct.output_words32[t8_last_index]
                        };
                        let t8_last_expected = if t8_last_index < t8_distinct.expected_words.len() {
                            t8_distinct.expected_words[t8_last_index]
                        } else if t8_last_index < t8_distinct.expected_words16.len() {
                            t8_distinct.expected_words16[t8_last_index]
                        } else {
                            t8_distinct.expected_words32[t8_last_index]
                        };
                        let expected_lane_dispatch =
                            (t8_distinct_row_count as u64).saturating_mul(8);
                        crate::log!(
                            "lumen-gpu-proof: director-step step=42 backend=local-gpu mode=t8-groupid-live16-row-scale rung_rows={} submitted={} finished={} readback_ok={} compare_ok={} reason={} program={} groups={} row_count={} live_k_dim={} expected_lane_dispatch={} observed_lane_dispatch={} first_gpu=0x{:08X} first_cpu_expected=0x{:08X} second_gpu=0x{:08X} second_cpu_expected=0x{:08X} last_gpu=0x{:08X} last_cpu_expected=0x{:08X} finish_marker=0x{:08X} finish_expected=0x{:08X} batch_bytes=0x{:X} action={} next={} does_not_prove=multi_tile_or_window_coalescing_or_full_model_matvec\n",
                            t8_rung_rows,
                            t8_distinct.submitted as u8,
                            t8_distinct.finished as u8,
                            t8_distinct.readback_ok as u8,
                            t8_distinct.compare_ok as u8,
                            t8_distinct.reason,
                            t8_distinct.program_name,
                            t8_distinct_row_count,
                            t8_distinct.row_count,
                            t8_distinct.live_k_dim,
                            expected_lane_dispatch,
                            t8_distinct.dispatch_delta,
                            t8_distinct.output_words[0],
                            t8_distinct.expected_words[0],
                            t8_distinct.output_words[1],
                            t8_distinct.expected_words[1],
                            t8_last_gpu,
                            t8_last_expected,
                            t8_distinct.finish_marker,
                            t8_distinct.expected_finish_marker,
                            t8_distinct.batch_bytes,
                            if t8_distinct.readback_ok {
                                "advance-frontier"
                            } else {
                                "hold-frontier"
                            },
                            if t8_distinct.readback_ok {
                                "raise-t8-groupid-row-count-or-coalesce-submit"
                            } else {
                                "fix-t8-groupid-row-scale"
                            },
                        );
                        if !t8_distinct.readback_ok {
                            t8_scale_failed = true;
                            break;
                        }
                        last_t8_row_count = t8_distinct_row_count;
                        last_t8_dispatch = t8_distinct.dispatch_delta;
                    }
                    if !t8_scale_failed && last_t8_row_count >= 32 {
                        let t9_live_k =
                            k_dim.min(trueos_eu::gfx12::T63_ACCUM16_HI_LIVE32_LIVE_K);
                        let t9_stage = crate::intel::stage_gpgpu_tile_record_accum16_window_trusted(
                            stage.output_gpu,
                            x,
                            t8_rows,
                            last_t8_row_count,
                            k_dim,
                            trueos_eu::gfx12::T62_ROW_INDEXED_LIVE_K,
                        );
                        if t9_stage.readback_ok {
                            let mut t9_expected_words32 = [0u32; 32];
                            for local_row in 0..last_t8_row_count.min(t9_expected_words32.len()) {
                                let row_start =
                                    local_row.saturating_mul(k_dim).saturating_mul(2);
                                let row_end =
                                    row_start.saturating_add(k_dim.saturating_mul(2));
                                t9_expected_words32[local_row] = bf16_row_dot_prefix(
                                    x,
                                    &t8_rows[row_start..row_end],
                                    t9_live_k,
                                )
                                .to_bits();
                            }
                            let t9 = crate::intel::submit_gpgpu_t9_existing_t63_groupid_live32_negative_control_probe(
                                stage.output_gpu,
                                stage.output_bytes,
                                t9_expected_words32,
                                last_t8_row_count,
                                t9_live_k,
                            );
                            crate::log!(
                                "lumen-gpu-proof: director-step step=46 backend=local-gpu mode=existing-t63-live32-groupid-negative-control t8_frontier_rows={} submitted={} finished={} readback_ok={} compare_ok={} reason={} program={} groups={} row_count={} live_k_dim={} expected_lane_dispatch={} observed_lane_dispatch={} compare_mask=0x{:08X} expected_mask=0x{:08X} first_gpu=0x{:08X} first_cpu_expected=0x{:08X} second_gpu=0x{:08X} second_cpu_expected=0x{:08X} last_gpu=0x{:08X} last_cpu_expected=0x{:08X} finish_marker=0x{:08X} finish_expected=0x{:08X} batch_bytes=0x{:X} action={} next={} does_not_prove=trusted_live32_runtime_ownership\n",
                                last_t8_row_count,
                                t9.submitted as u8,
                                t9.finished as u8,
                                t9.readback_ok as u8,
                                t9.compare_ok as u8,
                                t9.reason,
                                t9.program_name,
                                last_t8_row_count,
                                t9.row_count,
                                t9.live_k_dim,
                                last_t8_row_count.saturating_mul(8),
                                t9.dispatch_delta,
                                t9.compare_mask,
                                t9.expected_mask,
                                t9.output_words[0],
                                t9.expected_words[0],
                                t9.output_words[1],
                                t9.expected_words[1],
                                t9.output_words32[last_t8_row_count.saturating_sub(1)],
                                t9.expected_words32[last_t8_row_count.saturating_sub(1)],
                                t9.finish_marker,
                                t9.expected_finish_marker,
                                t9.batch_bytes,
                                if t9.readback_ok {
                                    "candidate-promote-after-repeat-proof"
                                } else {
                                    "hold-runtime"
                                },
                                if t9.readback_ok {
                                    "repeat-live32-groupid-shaped-proof"
                                } else {
                                    "generate-dedicated-groupid-live32-artifact"
                                },
                            );
                            if !t9.readback_ok {
                                let accepted_rows = last_t8_row_count.saturating_mul(24);
                                let accepted_blocks = accepted_rows
                                    / trueos_eu::gfx12::T62_ROW_INDEXED_PARTIAL_ROWS.max(1);
                                let live_windows_to_trusted = trusted_live_k
                                    .div_ceil(trueos_eu::gfx12::T62_ROW_INDEXED_LIVE_K.max(1));
                                let groupid_live32_separate_projected_submits =
                                    24usize.saturating_mul(2).saturating_add(
                                        accepted_blocks.saturating_mul(
                                            live_windows_to_trusted.saturating_sub(2),
                                        ),
                                    );
                                crate::log!(
                                    "lumen-gpu-proof: director-step step=47 backend=local-gpu mode=groupid-live32-artifact-contract source=crates/trueos-shader/t9_groupid_accum16_hi_live32_trueos_arena_bf16_unpack.comp prior_probe=existing-t63-live32-groupid-negative-control prior_compare_mask=0x{:08X} prior_expected_mask=0x{:08X} required_selector=workgroup-id-x required_local_size_x=1 required_groups={} required_row_count={} required_live_k_dim={} required_expected_lane_dispatch={} target_compare_mask=0x{:08X} target_finish_marker=0x{:08X} projected_submits_after_separate_live32={} current_submitted_blocks=3000 action=await-native-eu-artifact next=bake-and-prove-t9-groupid-live32 does_not_prove=native_artifact_or_submit_reduction\n",
                                    t9.compare_mask,
                                    t9.expected_mask,
                                    last_t8_row_count,
                                    last_t8_row_count,
                                    t9_live_k,
                                    last_t8_row_count.saturating_mul(8),
                                    t9.expected_mask,
                                    t9.expected_finish_marker,
                                    groupid_live32_separate_projected_submits,
                                );
                            }
                        } else {
                            crate::log!(
                                "lumen-gpu-proof: director-step step=46 backend=local-gpu mode=existing-t63-live32-groupid-negative-control t8_frontier_rows={} submitted=0 finished=0 readback_ok=0 compare_ok=0 reason={} groups={} row_count={} live_k_dim={} action=hold-runtime next=fix-live32-window-stage does_not_prove=trusted_live32_runtime_ownership\n",
                                last_t8_row_count,
                                t9_stage.reason,
                                last_t8_row_count,
                                last_t8_row_count,
                                t9_live_k,
                            );
                        }
                    }
                    T8_GROUPID_FRONTIER_ROWS.store(last_t8_row_count, Ordering::Release);
                    crate::log!(
                        "lumen-gpu-proof: director-step step=43 backend=local-gpu mode=t8-groupid-live16-row-scale-summary attempted_rungs={} frontier_rows={} live_k_dim={} last_dispatch_delta={} failed={} action={} next={} does_not_prove=full_model_matvec\n",
                        T8_GROUPID_ROW_SCALE_RUNGS.len(),
                        last_t8_row_count,
                        t62_live_k,
                        last_t8_dispatch,
                        t8_scale_failed as u8,
                        if t8_scale_failed {
                            "hold-frontier"
                        } else {
                            "advance-frontier"
                        },
                        if t8_scale_failed {
                            "inspect-t8-scale-failure"
                        } else {
                            "coalesce-t8-rowblock-submit"
                        },
                    );
                }
            }
            let t8_frontier_rows = T8_GROUPID_FRONTIER_ROWS.load(Ordering::Acquire);
            let t8_live16_rows = t8_frontier_rows
                .min(tile_remaining_rows.saturating_sub(block_tile_row))
                .min(32);
            let t62_is_final = t62_live_k == trusted_live_k;
            let use_t8_live16 = !t62_is_final
                && t8_live16_rows >= block_row_count
                && t8_live16_rows > trueos_eu::gfx12::T62_ROW_INDEXED_PARTIAL_ROWS
                && block_tile_row % t8_live16_rows == 0;
            let t62 = if t8_live16_carried {
                crate::intel::GpgpuT62PartialMatvecProof {
                    submitted: false,
                    finished: true,
                    readback_ok: true,
                    compare_ok: true,
                    reason: "trusted-t8-live16-carried",
                    program_name: trueos_eu::gfx12::T8_GROUPID_LIVE16_PROGRAM_NAME,
                    output_gpu: stage.output_gpu,
                    output_words: [0; 8],
                    expected_words: [0; 8],
                    output_words16: [0; 16],
                    expected_words16: [0; 16],
                    output_words32: [0; 32],
                    expected_words32: [0; 32],
                    compare_mask: 0,
                    expected_mask: 0,
                    dispatch_delta: 0,
                    finish_marker: 0,
                    expected_finish_marker: 0,
                    batch_bytes: 0,
                    row_count: block_row_count,
                    live_k_dim: t62_live_k,
                }
            } else if use_t8_live16 {
                let t8_rows_bytes = t8_live16_rows.saturating_mul(k_dim).saturating_mul(2);
                let t8_rows_end = block_row_offset.saturating_add(t8_rows_bytes);
                let t8_stage_ok = if t8_rows_end <= w_rowmajor_bf16.len() {
                    let t8_rows = &w_rowmajor_bf16[block_row_offset..t8_rows_end];
                    crate::intel::stage_gpgpu_tile_record_rows_trusted(
                        stage.output_gpu,
                        t8_rows,
                        t8_live16_rows,
                        k_dim,
                    )
                    .readback_ok
                } else {
                    false
                };
                if !t8_stage_ok {
                    failed = true;
                    continue;
                }
                skipped_output_readbacks = skipped_output_readbacks.saturating_add(1);
                let proof = crate::intel::submit_gpgpu_t8_groupid_live16_trusted_no_readback(
                    stage.output_gpu,
                    stage.output_bytes,
                    t8_live16_rows,
                    t62_live_k,
                );
                t8_live16_submitted_blocks += proof.submitted as usize;
                if proof.readback_ok {
                    t8_live16_accepted_blocks += 1;
                    t8_live16_accepted_rows =
                        t8_live16_accepted_rows.saturating_add(t8_live16_rows);
                    t8_live16_carry_row_end = global_row.saturating_add(t8_live16_rows);
                }
                proof
            } else if t62_is_final {
                let proof = crate::intel::submit_gpgpu_t62_partial_matvec_trusted(
                    stage.output_gpu,
                    stage.output_bytes,
                    block_row_count,
                    t62_live_k,
                );
                residual_t62_submitted_blocks += proof.submitted as usize;
                proof
            } else {
                skipped_output_readbacks = skipped_output_readbacks.saturating_add(1);
                let proof = crate::intel::submit_gpgpu_t62_partial_matvec_trusted_no_readback(
                    stage.output_gpu,
                    stage.output_bytes,
                    block_row_count,
                    t62_live_k,
                );
                residual_t62_submitted_blocks += proof.submitted as usize;
                proof
            };
            submitted_blocks += t62.submitted as usize;
            if !t62.readback_ok {
                failed = true;
                continue;
            }
            if t62_is_final {
                last_rung = 2;
                last_live_k = t62.live_k_dim;
            }
            if t62_is_final {
                for local_row in 0..block_row_count.min(t62.output_words.len()) {
                    prefix.push_row(
                        global_row.saturating_add(local_row),
                        t62.output_words[local_row],
                    );
                }
                accepted_blocks += 1;
                accepted_rows = accepted_rows.saturating_add(block_row_count);
                continue;
            }

            let t63_live_k = k_dim.min(trueos_eu::gfx12::T63_ACCUM16_HI_LIVE32_LIVE_K);
            let t63_stage = crate::intel::stage_gpgpu_tile_record_accum16_window_trusted(
                stage.output_gpu,
                x,
                rows,
                block_row_count,
                k_dim,
                trueos_eu::gfx12::T62_ROW_INDEXED_LIVE_K,
            );
            if !t63_stage.readback_ok {
                failed = true;
                continue;
            }
            let t63_is_final = t63_live_k == trusted_live_k;
            let mut proof = if t63_is_final {
                crate::intel::submit_gpgpu_t63_accum16_hi_live32_partial_matvec_trusted(
                    stage.output_gpu,
                    stage.output_bytes,
                    block_row_count,
                    t63_live_k,
                )
            } else {
                skipped_output_readbacks = skipped_output_readbacks.saturating_add(1);
                crate::intel::submit_gpgpu_t63_accum16_hi_live32_partial_matvec_trusted_no_readback(
                    stage.output_gpu,
                    stage.output_bytes,
                    block_row_count,
                    t63_live_k,
                )
            };
            submitted_blocks += proof.submitted as usize;
            if !proof.readback_ok {
                failed = true;
                continue;
            }
            last_rung = 3;
            last_live_k = proof.live_k_dim;
            if t63_is_final {
                for local_row in 0..block_row_count.min(proof.output_words.len()) {
                    prefix.push_row(
                        global_row.saturating_add(local_row),
                        proof.output_words[local_row],
                    );
                }
                accepted_blocks += 1;
                accepted_rows = accepted_rows.saturating_add(block_row_count);
                continue;
            }

            for rung in CGP_WINDOWED_ACCUM16_BASE_RUNGS
                .iter()
                .chain(CGP_WINDOWED_ACCUM16_EXTRA_RUNGS.iter())
            {
                let rung_live_k = k_dim.min(rung.live_k_dim);
                if rung_live_k > trusted_live_k || rung.window_start != last_live_k {
                    break;
                }
                let rung_stage = crate::intel::stage_gpgpu_tile_record_accum16_window_trusted(
                    stage.output_gpu,
                    x,
                    rows,
                    block_row_count,
                    k_dim,
                    rung.window_start,
                );
                if !rung_stage.readback_ok {
                    failed = true;
                    break;
                }
                let rung_is_final = rung_live_k == trusted_live_k;
                proof = if rung_is_final {
                    crate::intel::submit_gpgpu_windowed_accum16_partial_matvec_trusted(
                        rung.program_name,
                        stage.output_gpu,
                        stage.output_bytes,
                        block_row_count,
                        rung_live_k,
                    )
                } else {
                    skipped_output_readbacks = skipped_output_readbacks.saturating_add(1);
                    crate::intel::submit_gpgpu_windowed_accum16_partial_matvec_trusted_no_readback(
                        rung.program_name,
                        stage.output_gpu,
                        stage.output_bytes,
                        block_row_count,
                        rung_live_k,
                    )
                };
                submitted_blocks += proof.submitted as usize;
                if !proof.readback_ok {
                    failed = true;
                    break;
                }
                last_rung = rung.rung;
                last_live_k = proof.live_k_dim;
                if last_live_k == trusted_live_k {
                    break;
                }
            }

            if proof.readback_ok && last_live_k == trusted_live_k {
                for local_row in 0..block_row_count.min(proof.output_words.len()) {
                    prefix.push_row(
                        global_row.saturating_add(local_row),
                        proof.output_words[local_row],
                    );
                }
                accepted_blocks += 1;
                accepted_rows = accepted_rows.saturating_add(block_row_count);
            }
        }
    }

    if !LOGGED_TRUSTED_WINDOW_FAST_PATH.swap(true, Ordering::AcqRel) {
        let row_block_rows = trueos_eu::gfx12::T62_ROW_INDEXED_PARTIAL_ROWS
            .min(proof_tile_rows)
            .max(1);
        let row_blocks_per_tile = proof_tile_rows.div_ceil(row_block_rows);
        let submit_windows_per_block = if accepted_blocks == 0 {
            0
        } else {
            submitted_blocks / accepted_blocks
        };
        let live_window = trueos_eu::gfx12::T62_ROW_INDEXED_LIVE_K.max(1);
        let live_windows_to_trusted = trusted_live_k.div_ceil(live_window);
        let t8_frontier_rows = T8_GROUPID_FRONTIER_ROWS.load(Ordering::Acquire);
        let row_blocks_per_t8_submit = t8_frontier_rows.div_ceil(row_block_rows);
        let projected_submits_at_current_t8 = if row_blocks_per_t8_submit == 0 {
            submitted_blocks
        } else {
            accepted_blocks
                .div_ceil(row_blocks_per_t8_submit)
                .saturating_mul(live_windows_to_trusted)
        };
        let ideal_tile_projected_submits = armed_tiles.saturating_mul(live_windows_to_trusted);
        let ideal_reduction_x = if ideal_tile_projected_submits == 0 {
            0
        } else {
            submitted_blocks / ideal_tile_projected_submits
        };
        crate::log!(
            "lumen-gpu-proof: director-step step=44 backend=local-gpu mode=t8-coalesce-submit-accounting current_model=trusted-t8-live16-plus-residual-t6-windows rows={} k_dim={} armed_tiles={} tile_rows={} row_block_rows={} row_blocks_per_tile={} accepted_blocks={} accepted_rows={} submitted_blocks={} t8_live16_submitted_blocks={} t8_live16_accepted_blocks={} t8_live16_accepted_rows={} residual_t62_submitted_blocks={} submit_windows_per_block={} live_window={} trusted_live_k={} live_windows_to_trusted={} t8_frontier_rows={} row_blocks_per_t8_submit={} projected_submits_at_current_t8={} ideal_tile_projected_submits={} ideal_reduction_x={} action=execute-t8-live16-frontier-and-account-residual-windows next=groupid-windowed-live32-through-live512 does_not_prove=fully_coalesced_window_execution\n",
            n_rows,
            k_dim,
            armed_tiles,
            proof_tile_rows,
            row_block_rows,
            row_blocks_per_tile,
            accepted_blocks,
            accepted_rows,
            submitted_blocks,
            t8_live16_submitted_blocks,
            t8_live16_accepted_blocks,
            t8_live16_accepted_rows,
            residual_t62_submitted_blocks,
            submit_windows_per_block,
            live_window,
            trusted_live_k,
            live_windows_to_trusted,
            t8_frontier_rows,
            row_blocks_per_t8_submit,
            projected_submits_at_current_t8,
            ideal_tile_projected_submits,
            ideal_reduction_x,
        );
        crate::log!(
            "lumen-gpu-proof: trusted-window-fast-path source={} call={} rows={} k_dim={} armed_tiles={} row_block_cap={} trusted_rung={} trusted_live_k={} submitted_blocks={} t8_live16_submitted_blocks={} residual_t62_submitted_blocks={} accepted_blocks={} accepted_rows={} t8_live16_accepted_rows={} skipped_output_readbacks={} failed={} validation=disabled-after-frontier-proof output_owner=hybrid-cgp-prefix-cpu-ap-suffix action=use-gpu-prefix-without-cpu-reference-replay does_not_prove=full_model_matvec\n",
            plan.source_label,
            plan.call_index,
            n_rows,
            k_dim,
            armed_tiles,
            T62_ROW_BLOCK_DISPATCH_BLOCK_CAP,
            last_rung,
            last_live_k,
            submitted_blocks,
            t8_live16_submitted_blocks,
            residual_t62_submitted_blocks,
            accepted_blocks,
            accepted_rows,
            t8_live16_accepted_rows,
            skipped_output_readbacks,
            failed as u8,
        );
        let t8_row_groups = if t8_frontier_rows > 0 {
            accepted_rows / t8_frontier_rows
        } else {
            0
        };
        let groupid_live32_separate_projected_submits =
            t8_row_groups.saturating_mul(2).saturating_add(
                accepted_blocks.saturating_mul(live_windows_to_trusted.saturating_sub(2)),
            );
        let fused_live16_live32_projected_submits = t8_row_groups.saturating_add(
            accepted_blocks.saturating_mul(live_windows_to_trusted.saturating_sub(2)),
        );
        crate::log!(
            "lumen-gpu-proof: director-step step=45 backend=local-gpu mode=next-window-rung-accounting current_model=trusted-t8-live16-plus-localid-window-chain rows={} k_dim={} accepted_blocks={} accepted_rows={} current_submitted_blocks={} t8_frontier_rows={} t8_row_groups={} row_blocks_per_t8_submit={} live_windows_to_trusted={} groupid_live32_separate_projected_submits={} fused_live16_live32_projected_submits={} all_groupid_window_projected_submits={} ideal_tile_projected_submits={} existing_t63_partial_rows={} existing_t63_live_k={} existing_t63_addressing=local-invocation action=hold-runtime next=prove-groupid-live32-or-fused-two-window-artifact does_not_prove=new_window_artifact_or_submit_reduction\n",
            n_rows,
            k_dim,
            accepted_blocks,
            accepted_rows,
            submitted_blocks,
            t8_frontier_rows,
            t8_row_groups,
            row_blocks_per_t8_submit,
            live_windows_to_trusted,
            groupid_live32_separate_projected_submits,
            fused_live16_live32_projected_submits,
            projected_submits_at_current_t8,
            ideal_tile_projected_submits,
            trueos_eu::gfx12::T63_ACCUM16_HI_LIVE32_PARTIAL_ROWS,
            trueos_eu::gfx12::T63_ACCUM16_HI_LIVE32_LIVE_K,
        );
    }

    if prefix.is_empty() {
        crate::lumen::cgp::CgpBf16PrefixContribution::none()
    } else {
        prefix
    }
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
