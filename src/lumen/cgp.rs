// CGP is the Lumen-local compute graphics processor boundary.  It names the
// queue/planning layer between burn_baba policy and the register-level Intel
// GPGPU implementation without letting hardware details leak upward.

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum CgpJobMode {
    ProofOnly,
    ShadowCompare,
    AcceptedOutput,
}

impl CgpJobMode {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::ProofOnly => "proof-only",
            Self::ShadowCompare => "shadow-compare",
            Self::AcceptedOutput => "accepted-output",
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct CgpBackendDescriptor {
    pub(crate) name: &'static str,
    pub(crate) label: &'static str,
    pub(crate) role: CgpJobMode,
    pub(crate) output_owner: &'static str,
    pub(crate) correctness_contract: &'static str,
    pub(crate) dispatch_contract: &'static str,
}

pub(crate) const fn gpu_burn_baby_backend() -> CgpBackendDescriptor {
    CgpBackendDescriptor {
        name: "gpu-burn-baby",
        label: "local-gpgpu-burn-baby",
        role: CgpJobMode::ProofOnly,
        output_owner: "cpu-ap",
        correctness_contract: "cpu-reference-first",
        dispatch_contract: "guarded-proof-before-ownership",
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum CgpJobState {
    Queued,
    Staged,
    Submitted,
    Completed,
    Compared,
    Rejected,
    TimedOut,
}

#[derive(Copy, Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct CgpMatvecTileJob {
    pub(crate) mode: CgpJobMode,
    pub(crate) row_start: usize,
    pub(crate) row_end: usize,
    pub(crate) tile_rows: usize,
    pub(crate) k_dim: usize,
    pub(crate) x_bytes: usize,
    pub(crate) weight_tile_bytes: usize,
    pub(crate) output_tile_bytes: usize,
}

impl CgpMatvecTileJob {
    pub(crate) fn rowmajor_bf16(
        mode: CgpJobMode,
        row_start: usize,
        row_end: usize,
        k_dim: usize,
    ) -> Self {
        let tile_rows = row_end.saturating_sub(row_start);
        Self {
            mode,
            row_start,
            row_end,
            tile_rows,
            k_dim,
            x_bytes: k_dim.saturating_mul(core::mem::size_of::<f32>()),
            weight_tile_bytes: tile_rows.saturating_mul(k_dim).saturating_mul(2),
            output_tile_bytes: tile_rows.saturating_mul(core::mem::size_of::<f32>()),
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct CgpTilePlan {
    pub(crate) eligible: bool,
    pub(crate) pilot_tiles: usize,
    pub(crate) candidate_tiles: usize,
    pub(crate) tile_rows: usize,
    pub(crate) tile_k: usize,
    pub(crate) x_bytes: usize,
    pub(crate) weight_tile_bytes: usize,
    pub(crate) output_tile_bytes: usize,
    pub(crate) mode: CgpJobMode,
}

pub(crate) fn plan_rowmajor_bf16_tile(
    n_rows: usize,
    k_dim: usize,
    eligible: bool,
    arena_tile_rows: usize,
    arena_max_tiles: usize,
    pilot_tile_cap: usize,
    mode: CgpJobMode,
) -> CgpTilePlan {
    let tile_rows = arena_tile_rows.max(1).min(n_rows).max(1);
    let candidate_tiles = if eligible {
        n_rows.div_ceil(tile_rows)
    } else {
        0
    };
    let pilot_tiles = candidate_tiles.min(pilot_tile_cap).min(arena_max_tiles);
    let tile_job = CgpMatvecTileJob::rowmajor_bf16(mode, 0, tile_rows, k_dim);

    CgpTilePlan {
        eligible,
        pilot_tiles,
        candidate_tiles,
        tile_rows,
        tile_k: k_dim,
        x_bytes: tile_job.x_bytes,
        weight_tile_bytes: tile_job.weight_tile_bytes,
        output_tile_bytes: tile_job.output_tile_bytes,
        mode,
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct CgpBackendBudget {
    pub(crate) percent: usize,
    pub(crate) validated_lane_dispatch: usize,
    pub(crate) validated_hw_threads: usize,
    pub(crate) budget_hw_threads: usize,
    pub(crate) cpu_reserved_hw_threads: usize,
    pub(crate) target_rows: usize,
    pub(crate) cpu_rows: usize,
}

pub(crate) fn plan_backend_budget(
    n_rows: usize,
    validated_lane_dispatch: u32,
    budget_percent: usize,
    simd_lanes_per_thread: usize,
) -> CgpBackendBudget {
    let validated_lane_dispatch = validated_lane_dispatch as usize;
    let validated_hw_threads = validated_lane_dispatch / simd_lanes_per_thread.max(1);
    let budget_hw_threads = if validated_hw_threads == 0 {
        0
    } else {
        validated_hw_threads
            .saturating_mul(budget_percent)
            .saturating_div(100)
            .max(1)
    };
    let target_rows = if n_rows == 0 || budget_hw_threads == 0 {
        0
    } else {
        n_rows
            .saturating_mul(budget_percent)
            .saturating_div(100)
            .max(1)
            .min(n_rows)
    };

    CgpBackendBudget {
        percent: budget_percent,
        validated_lane_dispatch,
        validated_hw_threads,
        budget_hw_threads,
        cpu_reserved_hw_threads: validated_hw_threads.saturating_sub(budget_hw_threads),
        target_rows,
        cpu_rows: n_rows.saturating_sub(target_rows),
    }
}
