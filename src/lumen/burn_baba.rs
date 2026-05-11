use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static LOGGED_BF16_SHARE: AtomicBool = AtomicBool::new(false);
static SEEN_BF16_MATVECS: AtomicU64 = AtomicU64::new(0);
static GPU_SHAPE_CANDIDATES: AtomicU64 = AtomicU64::new(0);
static GPU_READY_CANDIDATES: AtomicU64 = AtomicU64::new(0);

const GPGPU_PILOT_MAX_TILES: usize = 3;
const LOCAL_GPGPU_PROOF_BACKEND_ENABLED: bool = true;
const LOCAL_GPU_BACKEND_BUDGET_PERCENT: usize = 20;
const GPGPU_VALIDATED_SIMD_LANES_PER_THREAD: usize = 8;
const MATVEC_PROTOCOL_SHAPE: &str = "matrix-id-row-range";
const MATVEC_DIRECTOR: &str = "single";

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum InferenceWorkload {
    Decode,
    BatchedDecode,
    Prefill,
    Training,
}

impl InferenceWorkload {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Decode => "decode",
            Self::BatchedDecode => "batched-decode",
            Self::Prefill => "prefill",
            Self::Training => "training",
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum KernelFamily {
    Matvec,
    BatchedGemm,
    FusedAttention,
    FusedDecodeBlock,
}

impl KernelFamily {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Matvec => "matvec",
            Self::BatchedGemm => "batched-gemm",
            Self::FusedAttention => "fused-attention",
            Self::FusedDecodeBlock => "fused-decode-block",
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum PrecisionLane {
    Bf16,
    Fp16,
    Fp8,
    Int8WeightOnly,
    Int4WeightOnly,
    MixedLowestQualitySafe,
}

impl PrecisionLane {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Bf16 => "bf16",
            Self::Fp16 => "fp16",
            Self::Fp8 => "fp8",
            Self::Int8WeightOnly => "int8-weight-only",
            Self::Int4WeightOnly => "int4-weight-only",
            Self::MixedLowestQualitySafe => "mixed-lowest-quality-safe",
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum MemoryLayoutPlan {
    RowMajorBf16,
    TiledGemm,
    PackedWeightOnly,
    KvCachePaged,
}

impl MemoryLayoutPlan {
    const fn as_str(self) -> &'static str {
        match self {
            Self::RowMajorBf16 => "rowmajor-bf16",
            Self::TiledGemm => "tiled-gemm",
            Self::PackedWeightOnly => "packed-weight-only",
            Self::KvCachePaged => "kv-cache-paged",
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum MatvecBackendRole {
    CpuAp,
    LocalNetCpu,
    LocalGpuProof,
    FutureNetGpu,
}

impl MatvecBackendRole {
    const fn as_str(self) -> &'static str {
        match self {
            Self::CpuAp => "cpu-ap",
            Self::LocalNetCpu => "local-net-cpu",
            Self::LocalGpuProof => "local-gpu-proof",
            Self::FutureNetGpu => "future-net-gpu",
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct InferenceKernelPlan {
    pub(crate) workload: InferenceWorkload,
    pub(crate) kernel: KernelFamily,
    pub(crate) precision: PrecisionLane,
    pub(crate) memory_layout: MemoryLayoutPlan,
    pub(crate) batch_rows: usize,
    pub(crate) gpu_ready: bool,
    pub(crate) next_kernel: KernelFamily,
    pub(crate) next_precision: PrecisionLane,
    pub(crate) next_memory_layout: MemoryLayoutPlan,
}

#[derive(Copy, Clone, Debug)]
struct MatvecDirectorPlan {
    director: &'static str,
    protocol: &'static str,
    execute_role: MatvecBackendRole,
    net_cpu_role: MatvecBackendRole,
    local_gpu_role: MatvecBackendRole,
    future_gpu_role: MatvecBackendRole,
    net_cpu_shadow_enabled: bool,
    net_cpu_route_enabled: bool,
    net_cpu_pending: u32,
    net_protocol_version: u16,
    net_caps: u32,
    net_min_remote_rows: u32,
    local_workers: u32,
    local_gpu_first: bool,
    future_net_gpu_deferred: bool,
}

impl MatvecDirectorPlan {
    fn bf16_decode(capacity_lanes: u32) -> Self {
        let telemetry = crate::lumen::lumen_net::backend_telemetry(capacity_lanes);
        Self {
            director: MATVEC_DIRECTOR,
            protocol: MATVEC_PROTOCOL_SHAPE,
            execute_role: MatvecBackendRole::CpuAp,
            net_cpu_role: MatvecBackendRole::LocalNetCpu,
            local_gpu_role: MatvecBackendRole::LocalGpuProof,
            future_gpu_role: MatvecBackendRole::FutureNetGpu,
            net_cpu_shadow_enabled: crate::lumen::lumen_net::shadow_bf16_matvec_to_net_backend(),
            net_cpu_route_enabled: crate::lumen::lumen_net::route_bf16_matvec_to_net_backend(),
            net_cpu_pending: telemetry.pending_bf16_matvecs,
            net_protocol_version: telemetry.protocol_version,
            net_caps: telemetry.caps,
            net_min_remote_rows: telemetry.min_remote_rows,
            local_workers: telemetry.local_workers,
            local_gpu_first: LOCAL_GPGPU_PROOF_BACKEND_ENABLED,
            future_net_gpu_deferred: true,
        }
    }
}

impl InferenceKernelPlan {
    const fn baseline_bf16_decode_matvec(
        batch_rows: usize,
        gpu_ready: bool,
    ) -> InferenceKernelPlan {
        InferenceKernelPlan {
            workload: InferenceWorkload::Decode,
            kernel: KernelFamily::Matvec,
            precision: PrecisionLane::Bf16,
            memory_layout: MemoryLayoutPlan::RowMajorBf16,
            batch_rows,
            gpu_ready,
            next_kernel: KernelFamily::BatchedGemm,
            next_precision: PrecisionLane::MixedLowestQualitySafe,
            next_memory_layout: MemoryLayoutPlan::TiledGemm,
        }
    }
}

// BF16 matvec is a solid practical baseline for LLM decode, and it gives
// Lumen a simple shared CPU/GPU contract today. The real pinnacle is
// hardware-aware fused kernels using the lowest precision that preserves
// model quality, with batching and memory layout optimized for the workload.
pub(crate) fn share_matvec_rowmajor_bf16(n_rows: usize, k_dim: usize, chunk_rows: usize) {
    SEEN_BF16_MATVECS.fetch_add(1, Ordering::AcqRel);

    let gpu = crate::intel::gpgpu_preflight_status();
    let shape_candidate =
        n_rows >= gpu.min_burn_rows && k_dim >= gpu.min_burn_k_dim && chunk_rows != 0;
    if shape_candidate {
        GPU_SHAPE_CANDIDATES.fetch_add(1, Ordering::AcqRel);
    }

    let gpu_ready =
        LOCAL_GPGPU_PROOF_BACKEND_ENABLED && shape_candidate && gpu.accepted && gpu.guc_ready;
    if gpu_ready {
        GPU_READY_CANDIDATES.fetch_add(1, Ordering::AcqRel);
    }
    let plan = plan_bf16_decode_matvec(n_rows, chunk_rows, gpu_ready);
    let pilot = plan_gpgpu_pilot(n_rows, k_dim, shape_candidate, gpu.tile_rows, gpu.max_tiles);
    let gpu_budget = plan_local_gpu_backend_budget(n_rows, gpu.eu_dispatch_delta);

    if LOGGED_BF16_SHARE.swap(true, Ordering::AcqRel) {
        return;
    }

    let director = MatvecDirectorPlan::bf16_decode(gpu.lanes);
    let gpu_burn_baby = crate::lumen::cgp::gpu_burn_baby_backend();

    crate::log!(
        "burn-baba: shared-inference-plan director={} protocol={} workload={} kernel={} precision={} layout={} batch_rows={} preflight_submitted={} accepted={} completed={} guc_ready={} lanes={} marker=0x{:08X} dot={} sum_a={} sum_b={} rows={} k_dim={} chunk_rows={} min_rows={} min_k_dim={} shape_candidate={} gpu_ready={} execute_role={} net_cpu_role={} net_cpu_route={} net_cpu_shadow={} net_cpu_pending={} net_protocol_v={} net_caps=0x{:X} net_min_rows={} local_workers={} local_gpu_role={} local_gpu_backend={} local_gpu_backend_label={} local_gpu_output_owner={} local_gpu_contract={} local_gpu_enabled={} local_gpu_first={} local_gpu_action={} local_gpu_budget_pct={} local_gpu_validated_lanes={} local_gpu_validated_threads={} local_gpu_budget_threads={} cpu_reserved_threads={} local_gpu_target_rows=0..{} cpu_rows={}..{} future_gpu_role={} future_net_gpu_deferred={} future_gpu_action=defer-until-net-gpu-protocol action=cpu-ap-director-keeps-local-results matrix_id_source=lumen-net-manifest cpu_ap_continues=1 next_kernel={} next_precision={} next_layout={} next=batched-gemm-attention-kv-fusion-mixed-precision does_not_prove=gpu_matmul\n",
        director.director,
        director.protocol,
        plan.workload.as_str(),
        plan.kernel.as_str(),
        plan.precision.as_str(),
        plan.memory_layout.as_str(),
        plan.batch_rows,
        gpu.submitted as u8,
        gpu.accepted as u8,
        gpu.completed as u8,
        gpu.guc_ready as u8,
        gpu.lanes,
        gpu.marker,
        gpu.dot,
        gpu.sum_a,
        gpu.sum_b,
        n_rows,
        k_dim,
        chunk_rows,
        gpu.min_burn_rows,
        gpu.min_burn_k_dim,
        shape_candidate as u8,
        plan.gpu_ready as u8,
        director.execute_role.as_str(),
        director.net_cpu_role.as_str(),
        director.net_cpu_route_enabled as u8,
        director.net_cpu_shadow_enabled as u8,
        director.net_cpu_pending,
        director.net_protocol_version,
        director.net_caps,
        director.net_min_remote_rows,
        director.local_workers,
        director.local_gpu_role.as_str(),
        gpu_burn_baby.name,
        gpu_burn_baby.label,
        gpu_burn_baby.output_owner,
        gpu_burn_baby.correctness_contract,
        LOCAL_GPGPU_PROOF_BACKEND_ENABLED as u8,
        director.local_gpu_first as u8,
        if LOCAL_GPGPU_PROOF_BACKEND_ENABLED {
            "one-tile-proof-budgeted-dispatch-disabled"
        } else {
            "disabled-by-selector"
        },
        gpu_budget.percent,
        gpu_budget.validated_lane_dispatch,
        gpu_budget.validated_hw_threads,
        gpu_budget.budget_hw_threads,
        gpu_budget.cpu_reserved_hw_threads,
        gpu_budget.target_rows,
        gpu_budget.target_rows,
        n_rows,
        director.future_gpu_role.as_str(),
        director.future_net_gpu_deferred as u8,
        plan.next_kernel.as_str(),
        plan.next_precision.as_str(),
        plan.next_memory_layout.as_str(),
    );
    let pilot_reason = if !LOCAL_GPGPU_PROOF_BACKEND_ENABLED {
        "local-gpu-proof-disabled-by-selector"
    } else if gpu.result_c_changed_by_eu {
        "eu-c-store-proven-pilot-still-guarded"
    } else if gpu.eu_walker_retired {
        "eu-walker-retired-awaiting-c-store"
    } else if gpu.eu_walker_submitted {
        "eu-walker-not-retired"
    } else if gpu.eu_walker_encoded {
        "eu-walker-encoded-awaiting-submit"
    } else {
        "eu-c-store-kernel-not-proven"
    };
    crate::log!(
        "burn-baba: gpgpu-pilot-plan director={} role={} protocol={} cgp_backend={} cgp_mode={} cgp_backend_role={} cgp_dispatch_contract={} enabled={} eligible={} gpu_ready={} arena_ready={} arena_gpu_base=0x{:X} arena_bytes=0x{:X} arena_max_tiles={} pilot_tiles={} pilot_tile_cap={} candidate_tiles={} tile_rows={} tile_k={} x_bytes={} weight_tile_bytes={} output_tile_bytes={} budget_pct={} validated_hw_threads={} budget_hw_threads={} target_rows={} cpu_rows={} compare={} dispatch=disabled reason={} cpu_ap_continues=1 net_gpu_role={} net_gpu_action=deferred does_not_prove=gpu_matmul\n",
        director.director,
        director.local_gpu_role.as_str(),
        director.protocol,
        gpu_burn_baby.name,
        pilot.mode.as_str(),
        gpu_burn_baby.role.as_str(),
        gpu_burn_baby.dispatch_contract,
        LOCAL_GPGPU_PROOF_BACKEND_ENABLED as u8,
        (LOCAL_GPGPU_PROOF_BACKEND_ENABLED && pilot.eligible) as u8,
        plan.gpu_ready as u8,
        gpu.enough_for_shape as u8,
        gpu.arena_gpu_base,
        gpu.arena_bytes,
        gpu.max_tiles,
        pilot.pilot_tiles,
        GPGPU_PILOT_MAX_TILES,
        pilot.candidate_tiles,
        pilot.tile_rows,
        pilot.tile_k,
        pilot.x_bytes,
        pilot.weight_tile_bytes,
        pilot.output_tile_bytes,
        gpu_budget.percent,
        gpu_budget.validated_hw_threads,
        gpu_budget.budget_hw_threads,
        gpu_budget.target_rows,
        gpu_budget.cpu_rows,
        gpu_burn_baby.correctness_contract,
        pilot_reason,
        director.future_gpu_role.as_str(),
    );
    let eu_execution_runs = gpu.eu_dispatch_delta != 0;
    let gate_blocker = if !LOCAL_GPGPU_PROOF_BACKEND_ENABLED {
        "local-gpu-proof-disabled-by-selector"
    } else if gpu.result_c_changed_by_eu {
        "pilot-scale-disabled-until-cpu-reference-compare"
    } else if gpu.eu_walker_retired {
        "eu-c-store-readback"
    } else if gpu.eu_walker_submitted {
        "eu-walker-retire"
    } else if gpu.eu_walker_encoded {
        "eu-walker-submit"
    } else {
        "eu-c-store-kernel"
    };
    let gate_next = if !LOCAL_GPGPU_PROOF_BACKEND_ENABLED {
        "enable-local-gpu-proof-selector"
    } else if gpu.result_c_changed_by_eu {
        "enable-one-tile-gpu-proof-compare"
    } else if gpu.eu_walker_retired {
        "fix-eu-c-store-message"
    } else if gpu.eu_walker_submitted {
        "fix-compute-walker-retire"
    } else if gpu.eu_walker_encoded {
        "submit-compute-walker"
    } else {
        "upload-gfx125-c-store-kernel"
    };
    crate::log!(
        "burn-baba: gpgpu-dispatch-gate director={} role={} cgp_backend={} output_owner={} enabled={} h2g_mmio={} input_buffers_ab_in_ggtt={} ctb_enabled=0 guc_context_registered=0 guc_sched_enabled=0 eu_kernel_uploaded={} eu_walker_encoded={} eu_walker_submitted={} eu_walker_retired={} eu_execution_runs={} eu_dispatch_delta={} result_c_slot={} result_c_value=0x{:08X} result_c_changed_by_eu={} cpu_reads_c_back={} arena_ready={} cpu_reference_compare=1 dispatch=disabled blocker={} next={} net_gpu_role={} net_gpu_action=deferred does_not_prove=gpu_matmul\n",
        director.director,
        director.local_gpu_role.as_str(),
        gpu_burn_baby.name,
        gpu_burn_baby.output_owner,
        LOCAL_GPGPU_PROOF_BACKEND_ENABLED as u8,
        crate::intel::guc_h2g_mmio_accepted() as u8,
        gpu.accepted as u8,
        gpu.eu_kernel_uploaded as u8,
        gpu.eu_walker_encoded as u8,
        gpu.eu_walker_submitted as u8,
        gpu.eu_walker_retired as u8,
        eu_execution_runs as u8,
        gpu.eu_dispatch_delta,
        22,
        gpu.eu_c_store_value,
        gpu.result_c_changed_by_eu as u8,
        gpu.accepted as u8,
        gpu.enough_for_shape as u8,
        gate_blocker,
        gate_next,
        director.future_gpu_role.as_str(),
    );
}

pub(crate) fn plan_bf16_decode_matvec(
    n_rows: usize,
    chunk_rows: usize,
    gpu_ready: bool,
) -> InferenceKernelPlan {
    let batch_rows = if chunk_rows == 0 {
        n_rows
    } else {
        chunk_rows.min(n_rows).max(1)
    };
    InferenceKernelPlan::baseline_bf16_decode_matvec(batch_rows, gpu_ready)
}

fn plan_gpgpu_pilot(
    n_rows: usize,
    k_dim: usize,
    eligible: bool,
    arena_tile_rows: usize,
    arena_max_tiles: usize,
) -> crate::lumen::cgp::CgpTilePlan {
    crate::lumen::cgp::plan_rowmajor_bf16_tile(
        n_rows,
        k_dim,
        eligible,
        arena_tile_rows,
        arena_max_tiles,
        GPGPU_PILOT_MAX_TILES,
        crate::lumen::cgp::CgpJobMode::ProofOnly,
    )
}

fn plan_local_gpu_backend_budget(
    n_rows: usize,
    validated_lane_dispatch: u32,
) -> crate::lumen::cgp::CgpBackendBudget {
    crate::lumen::cgp::plan_backend_budget(
        n_rows,
        validated_lane_dispatch,
        LOCAL_GPU_BACKEND_BUDGET_PERCENT,
        GPGPU_VALIDATED_SIMD_LANES_PER_THREAD,
    )
}
