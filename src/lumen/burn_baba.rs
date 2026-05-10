use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static LOGGED_BF16_SHARE: AtomicBool = AtomicBool::new(false);
static SEEN_BF16_MATVECS: AtomicU64 = AtomicU64::new(0);
static GPU_SHAPE_CANDIDATES: AtomicU64 = AtomicU64::new(0);
static GPU_READY_CANDIDATES: AtomicU64 = AtomicU64::new(0);

const GPGPU_PILOT_MAX_TILES: usize = 1;
const LOCAL_GPGPU_SHADOW_BACKEND_ENABLED: bool = false;
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
    LocalGpuShadow,
    FutureNetGpu,
}

impl MatvecBackendRole {
    const fn as_str(self) -> &'static str {
        match self {
            Self::CpuAp => "cpu-ap",
            Self::LocalNetCpu => "local-net-cpu",
            Self::LocalGpuShadow => "local-gpu-shadow",
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
            local_gpu_role: MatvecBackendRole::LocalGpuShadow,
            future_gpu_role: MatvecBackendRole::FutureNetGpu,
            net_cpu_shadow_enabled: crate::lumen::lumen_net::shadow_bf16_matvec_to_net_backend(),
            net_cpu_route_enabled: crate::lumen::lumen_net::route_bf16_matvec_to_net_backend(),
            net_cpu_pending: telemetry.pending_bf16_matvecs,
            net_protocol_version: telemetry.protocol_version,
            net_caps: telemetry.caps,
            net_min_remote_rows: telemetry.min_remote_rows,
            local_workers: telemetry.local_workers,
            local_gpu_first: LOCAL_GPGPU_SHADOW_BACKEND_ENABLED,
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
        LOCAL_GPGPU_SHADOW_BACKEND_ENABLED && shape_candidate && gpu.accepted && gpu.guc_ready;
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

    crate::log!(
        "burn-baba: shared-inference-plan director={} protocol={} workload={} kernel={} precision={} layout={} batch_rows={} preflight_submitted={} accepted={} completed={} guc_ready={} lanes={} marker=0x{:08X} dot={} sum_a={} sum_b={} rows={} k_dim={} chunk_rows={} min_rows={} min_k_dim={} shape_candidate={} gpu_ready={} execute_role={} net_cpu_role={} net_cpu_route={} net_cpu_shadow={} net_cpu_pending={} net_protocol_v={} net_caps=0x{:X} net_min_rows={} local_workers={} local_gpu_role={} local_gpu_enabled={} local_gpu_first={} local_gpu_action={} local_gpu_budget_pct={} local_gpu_validated_lanes={} local_gpu_validated_threads={} local_gpu_budget_threads={} cpu_reserved_threads={} local_gpu_target_rows=0..{} cpu_rows={}..{} future_gpu_role={} future_net_gpu_deferred={} future_gpu_action=defer-until-net-gpu-protocol action=cpu-ap-director-keeps-local-results matrix_id_source=lumen-net-manifest cpu_ap_continues=1 next_kernel={} next_precision={} next_layout={} next=batched-gemm-attention-kv-fusion-mixed-precision does_not_prove=gpu_matmul\n",
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
        LOCAL_GPGPU_SHADOW_BACKEND_ENABLED as u8,
        director.local_gpu_first as u8,
        if LOCAL_GPGPU_SHADOW_BACKEND_ENABLED {
            "one-tile-shadow-budgeted-dispatch-disabled"
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
    let pilot_reason = if !LOCAL_GPGPU_SHADOW_BACKEND_ENABLED {
        "local-gpu-disabled-by-selector"
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
        "burn-baba: gpgpu-pilot-plan director={} role={} protocol={} enabled={} eligible={} gpu_ready={} arena_ready={} arena_gpu_base=0x{:X} arena_bytes=0x{:X} arena_max_tiles={} pilot_tiles={} pilot_tile_cap={} candidate_tiles={} tile_rows={} tile_k={} x_bytes={} weight_tile_bytes={} output_tile_bytes={} budget_pct={} validated_hw_threads={} budget_hw_threads={} target_rows={} cpu_rows={} compare=cpu-reference-first dispatch=disabled reason={} cpu_ap_continues=1 net_gpu_role={} net_gpu_action=deferred does_not_prove=gpu_matmul\n",
        director.director,
        director.local_gpu_role.as_str(),
        director.protocol,
        LOCAL_GPGPU_SHADOW_BACKEND_ENABLED as u8,
        (LOCAL_GPGPU_SHADOW_BACKEND_ENABLED && pilot.eligible) as u8,
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
        pilot_reason,
        director.future_gpu_role.as_str(),
    );
    let eu_execution_runs = gpu.eu_dispatch_delta != 0;
    let gate_blocker = if !LOCAL_GPGPU_SHADOW_BACKEND_ENABLED {
        "local-gpu-disabled-by-selector"
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
    let gate_next = if !LOCAL_GPGPU_SHADOW_BACKEND_ENABLED {
        "enable-local-gpu-shadow-selector"
    } else if gpu.result_c_changed_by_eu {
        "enable-one-tile-gpu-shadow-compare"
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
        "burn-baba: gpgpu-dispatch-gate director={} role={} enabled={} h2g_mmio={} input_buffers_ab_in_ggtt={} ctb_enabled=0 guc_context_registered=0 guc_sched_enabled=0 eu_kernel_uploaded={} eu_walker_encoded={} eu_walker_submitted={} eu_walker_retired={} eu_execution_runs={} eu_dispatch_delta={} result_c_slot={} result_c_value=0x{:08X} result_c_changed_by_eu={} cpu_reads_c_back={} arena_ready={} cpu_reference_compare=1 dispatch=disabled blocker={} next={} net_gpu_role={} net_gpu_action=deferred does_not_prove=gpu_matmul\n",
        director.director,
        director.local_gpu_role.as_str(),
        LOCAL_GPGPU_SHADOW_BACKEND_ENABLED as u8,
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

#[derive(Copy, Clone, Debug)]
struct GpgpuPilotPlan {
    eligible: bool,
    pilot_tiles: usize,
    candidate_tiles: usize,
    tile_rows: usize,
    tile_k: usize,
    x_bytes: usize,
    weight_tile_bytes: usize,
    output_tile_bytes: usize,
}

#[derive(Copy, Clone, Debug)]
struct LocalGpuBackendBudget {
    percent: usize,
    validated_lane_dispatch: usize,
    validated_hw_threads: usize,
    budget_hw_threads: usize,
    cpu_reserved_hw_threads: usize,
    target_rows: usize,
    cpu_rows: usize,
}

fn plan_gpgpu_pilot(
    n_rows: usize,
    k_dim: usize,
    eligible: bool,
    arena_tile_rows: usize,
    arena_max_tiles: usize,
) -> GpgpuPilotPlan {
    let tile_rows = arena_tile_rows.max(1).min(n_rows).max(1);
    let candidate_tiles = if eligible {
        n_rows.div_ceil(tile_rows)
    } else {
        0
    };
    let pilot_tiles = candidate_tiles
        .min(GPGPU_PILOT_MAX_TILES)
        .min(arena_max_tiles);
    let x_bytes = k_dim.saturating_mul(core::mem::size_of::<f32>());
    let weight_tile_bytes = tile_rows.saturating_mul(k_dim).saturating_mul(2);
    let output_tile_bytes = tile_rows.saturating_mul(core::mem::size_of::<f32>());

    GpgpuPilotPlan {
        eligible,
        pilot_tiles,
        candidate_tiles,
        tile_rows,
        tile_k: k_dim,
        x_bytes,
        weight_tile_bytes,
        output_tile_bytes,
    }
}

fn plan_local_gpu_backend_budget(
    n_rows: usize,
    validated_lane_dispatch: u32,
) -> LocalGpuBackendBudget {
    let validated_lane_dispatch = validated_lane_dispatch as usize;
    let validated_hw_threads =
        validated_lane_dispatch / GPGPU_VALIDATED_SIMD_LANES_PER_THREAD.max(1);
    let budget_hw_threads = if validated_hw_threads == 0 {
        0
    } else {
        validated_hw_threads
            .saturating_mul(LOCAL_GPU_BACKEND_BUDGET_PERCENT)
            .saturating_div(100)
            .max(1)
    };
    let target_rows = if n_rows == 0 || budget_hw_threads == 0 {
        0
    } else {
        n_rows
            .saturating_mul(LOCAL_GPU_BACKEND_BUDGET_PERCENT)
            .saturating_div(100)
            .max(1)
            .min(n_rows)
    };

    LocalGpuBackendBudget {
        percent: LOCAL_GPU_BACKEND_BUDGET_PERCENT,
        validated_lane_dispatch,
        validated_hw_threads,
        budget_hw_threads,
        cpu_reserved_hw_threads: validated_hw_threads.saturating_sub(budget_hw_threads),
        target_rows,
        cpu_rows: n_rows.saturating_sub(target_rows),
    }
}
