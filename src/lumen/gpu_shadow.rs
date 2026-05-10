use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static SEEN_BF16_MATVECS: AtomicU64 = AtomicU64::new(0);
static LOGGED_SHADOW_PLAN: AtomicBool = AtomicBool::new(false);
static LOGGED_STATIC_TILE_PROOF: AtomicBool = AtomicBool::new(false);

#[derive(Copy, Clone, Debug)]
pub(crate) struct LocalGpuShadowPlan {
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
) -> LocalGpuShadowPlan {
    let call_index = SEEN_BF16_MATVECS.fetch_add(1, Ordering::AcqRel) + 1;
    let gpu = crate::intel::gpgpu_preflight_status();
    let shape_candidate =
        n_rows >= gpu.min_burn_rows && k_dim >= gpu.min_burn_k_dim && chunk_rows != 0;
    let static_tile_proven =
        gpu.eu_walker_retired && gpu.result_c_changed_by_eu && gpu.eu_dispatch_delta != 0;
    let candidate = shape_candidate && static_tile_proven && gpu.enough_for_shape;
    let plan = LocalGpuShadowPlan {
        candidate,
        static_tile_proven,
        lane_dispatch_count: gpu.eu_dispatch_delta,
        expected_store_value: gpu.eu_expected_store_value,
        observed_store_value: gpu.eu_c_store_value,
        program_name: gpu.eu_program_name,
    };

    if !LOGGED_SHADOW_PLAN.swap(true, Ordering::AcqRel) {
        crate::log!(
            "lumen-gpu-shadow: director-step step=2 backend=local-gpu mode=shadow-only call={} rows={} k_dim={} chunk_rows={} chunks={} min_rows={} min_k_dim={} arena_ready={} shape_candidate={} candidate={} static_tile_proven={} program={} lane_dispatch={} expected=0x{:08X} observed=0x{:08X} output_owner=cpu-ap action=no-output-ownership next=one-live-row-shadow-compare\n",
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
            "lumen-gpu-shadow: director-step step=3 backend=local-gpu proof=static-dp4a-hdc-store-eot program={} lane_dispatch={} store_expected=0x{:08X} store_observed=0x{:08X} eot_retired=1 action=promote-to-live-row-shadow next=bind-manifest-row-and-x-buffer does_not_prove=model_matvec\n",
            gpu.eu_program_name,
            gpu.eu_dispatch_delta,
            gpu.eu_expected_store_value,
            gpu.eu_c_store_value,
        );
    }

    plan
}
