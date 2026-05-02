const GPGPU_PREFLIGHT_LANES: usize = 4;
const GPGPU_BURN_MIN_ROWS: usize = 512;
const GPGPU_BURN_MIN_K_DIM: usize = 512;

static GPGPU_PREFLIGHT_SUBMITTED: AtomicBool = AtomicBool::new(false);
static GPGPU_PREFLIGHT_ACCEPTED: AtomicBool = AtomicBool::new(false);
static GPGPU_PREFLIGHT_COMPLETED: AtomicBool = AtomicBool::new(false);
static GPGPU_PREFLIGHT_MARKER: AtomicU32 = AtomicU32::new(0);
static GPGPU_PREFLIGHT_DOT: AtomicU32 = AtomicU32::new(0);
static GPGPU_PREFLIGHT_SUM_A: AtomicU32 = AtomicU32::new(0);
static GPGPU_PREFLIGHT_SUM_B: AtomicU32 = AtomicU32::new(0);
static GPGPU_PREFLIGHT_LANES_OBSERVED: AtomicU32 = AtomicU32::new(0);
static GPGPU_TILE_ARENA_MAPPED: AtomicBool = AtomicBool::new(false);
static GPGPU_TILE_ARENA_STATUS_LOGGED: AtomicBool = AtomicBool::new(false);
static GPGPU_EU_KERNEL_UPLOADED: AtomicBool = AtomicBool::new(false);
static GPGPU_EU_WALKER_ENCODED: AtomicBool = AtomicBool::new(false);
static GPGPU_EU_WALKER_SUBMITTED: AtomicBool = AtomicBool::new(false);
static GPGPU_EU_WALKER_RETIRED: AtomicBool = AtomicBool::new(false);
static GPGPU_EU_DISPATCH_DELTA: AtomicU32 = AtomicU32::new(0);

#[derive(Copy, Clone, Debug)]
pub(crate) struct GpgpuPreflightStatus {
    pub(crate) submitted: bool,
    pub(crate) accepted: bool,
    pub(crate) completed: bool,
    pub(crate) guc_ready: bool,
    pub(crate) marker: u32,
    pub(crate) dot: u32,
    pub(crate) sum_a: u32,
    pub(crate) sum_b: u32,
    pub(crate) lanes: u32,
    pub(crate) min_burn_rows: usize,
    pub(crate) min_burn_k_dim: usize,
    pub(crate) arena_gpu_base: u64,
    pub(crate) arena_bytes: usize,
    pub(crate) tile_rows: usize,
    pub(crate) max_tiles: usize,
    pub(crate) enough_for_shape: bool,
    pub(crate) eu_kernel_uploaded: bool,
    pub(crate) eu_walker_encoded: bool,
    pub(crate) eu_walker_submitted: bool,
    pub(crate) eu_walker_retired: bool,
    pub(crate) eu_dispatch_delta: u32,
}

pub(crate) fn gpgpu_preflight_status() -> GpgpuPreflightStatus {
    let warm = warm_state();
    let arena_bytes = warm.map_or(0, |warm| warm.gpgpu_arena_len);
    GpgpuPreflightStatus {
        submitted: GPGPU_PREFLIGHT_SUBMITTED.load(Ordering::Acquire),
        accepted: GPGPU_PREFLIGHT_ACCEPTED.load(Ordering::Acquire),
        completed: GPGPU_PREFLIGHT_COMPLETED.load(Ordering::Acquire),
        guc_ready: crate::intel::guc_ready(),
        marker: GPGPU_PREFLIGHT_MARKER.load(Ordering::Acquire),
        dot: GPGPU_PREFLIGHT_DOT.load(Ordering::Acquire),
        sum_a: GPGPU_PREFLIGHT_SUM_A.load(Ordering::Acquire),
        sum_b: GPGPU_PREFLIGHT_SUM_B.load(Ordering::Acquire),
        lanes: GPGPU_PREFLIGHT_LANES_OBSERVED.load(Ordering::Acquire),
        min_burn_rows: GPGPU_BURN_MIN_ROWS,
        min_burn_k_dim: GPGPU_BURN_MIN_K_DIM,
        arena_gpu_base: gpgpu_arena_gpu_base(arena_bytes),
        arena_bytes,
        tile_rows: GPGPU_TILE_ROWS,
        max_tiles: gpgpu_arena_max_tiles(arena_bytes),
        enough_for_shape: gpgpu_arena_enough_for_shape(arena_bytes),
        eu_kernel_uploaded: GPGPU_EU_KERNEL_UPLOADED.load(Ordering::Acquire),
        eu_walker_encoded: GPGPU_EU_WALKER_ENCODED.load(Ordering::Acquire),
        eu_walker_submitted: GPGPU_EU_WALKER_SUBMITTED.load(Ordering::Acquire),
        eu_walker_retired: GPGPU_EU_WALKER_RETIRED.load(Ordering::Acquire),
        eu_dispatch_delta: GPGPU_EU_DISPATCH_DELTA.load(Ordering::Acquire),
    }
}

pub(crate) fn submit_gpgpu_preflight_once() {
    if GPGPU_PREFLIGHT_SUBMITTED.swap(true, Ordering::AcqRel) {
        return;
    }

    let Some(dev) = crate::intel::claimed_device() else {
        crate::log!("intel/gpgpu: preflight skipped reason=no-device\n");
        return;
    };

    let warm = warm_once(dev);
    if warm.ring_len == 0
        || warm.context_len == 0
        || warm.batch_len == 0
        || warm.vertex_len < GPGPU_PREFLIGHT_LANES * core::mem::size_of::<u32>()
        || warm.streamout_len < GPGPU_PREFLIGHT_LANES * core::mem::size_of::<u32>()
        || warm.result_len
            < (RESULT_SLOT_GPGPU_PREFLIGHT_LANES_DWORD + 1) * core::mem::size_of::<u32>()
    {
        crate::log!("intel/gpgpu: preflight skipped reason=warm-buffers\n");
        return;
    }

    if !forcewake_render_acquire(warm) {
        crate::log!("intel/gpgpu: preflight skipped reason=forcewake\n");
        return;
    }

    let arena_mapped = ensure_gpgpu_tile_arena_mapped(dev, warm);
    log_gpgpu_tile_arena_status(warm, arena_mapped);
    crate::intel::log_guc_submission_contract(dev, "gpgpu-preflight");
    let accepted = submit_gpgpu_preflight(dev, warm);
    if !accepted {
        recover_render_engine_after_nonretired_submit(dev, warm, "gpgpu-preflight");
    }
    let eu_artifact = prepare_gpgpu_eu_artifact(warm, accepted);
    log_gpgpu_eu_artifact_status(eu_artifact);
    if eu_artifact.walker_encoded {
        let walker = submit_gpgpu_compute_walker_probe(dev, warm);
        if !walker.retired {
            recover_render_engine_after_nonretired_submit(dev, warm, "gpgpu-compute-walker");
        }
        log_gpgpu_compute_walker_status(walker);
    }
}

fn gpgpu_arena_gpu_base(arena_bytes: usize) -> u64 {
    if arena_bytes == 0 {
        0
    } else {
        GPU_VA_GPGPU_TILE_ARENA_BASE
    }
}

fn gpgpu_arena_max_tiles(arena_bytes: usize) -> usize {
    if arena_bytes <= GPGPU_X_VECTOR_BYTES {
        return 0;
    }
    (arena_bytes - GPGPU_X_VECTOR_BYTES) / (GPGPU_WEIGHT_TILE_BYTES + GPGPU_OUTPUT_TILE_BYTES)
}

fn gpgpu_arena_enough_for_shape(arena_bytes: usize) -> bool {
    gpgpu_arena_max_tiles(arena_bytes) >= GPGPU_TILE_TARGET_TILES
}

fn ensure_gpgpu_tile_arena_mapped(dev: crate::intel::Dev, warm: RenderWarmState) -> bool {
    if GPGPU_TILE_ARENA_MAPPED.load(Ordering::Acquire) {
        return true;
    }
    if warm.gpgpu_arena_len == 0 {
        return false;
    }

    let mapped = super::map_ggtt(
        dev,
        warm.gpgpu_arena_phys,
        warm.gpgpu_arena_len,
        GPU_VA_GPGPU_TILE_ARENA_BASE,
    );
    if mapped {
        super::ggtt_invalidate(dev);
        GPGPU_TILE_ARENA_MAPPED.store(true, Ordering::Release);
    }
    mapped
}

fn log_gpgpu_tile_arena_status(warm: RenderWarmState, mapped: bool) {
    if GPGPU_TILE_ARENA_STATUS_LOGGED.swap(true, Ordering::AcqRel) {
        return;
    }

    let arena_bytes = warm.gpgpu_arena_len;
    crate::log!(
        "intel/gpgpu: arena mapped={} arena_gpu_base=0x{:X} arena_bytes=0x{:X} tile_rows={} max_tiles={} enough_for_shape={} tile_k={} weight_tile_bytes=0x{:X} x_bytes=0x{:X} output_tile_bytes=0x{:X} target_tiles={} does_not_prove=eu_thread_execution_or_matvec\n",
        mapped as u8,
        gpgpu_arena_gpu_base(arena_bytes),
        arena_bytes,
        GPGPU_TILE_ROWS,
        gpgpu_arena_max_tiles(arena_bytes),
        gpgpu_arena_enough_for_shape(arena_bytes) as u8,
        GPGPU_TILE_K_DIM,
        GPGPU_WEIGHT_TILE_BYTES,
        GPGPU_X_VECTOR_BYTES,
        GPGPU_OUTPUT_TILE_BYTES,
        GPGPU_TILE_TARGET_TILES,
    );
}

fn submit_gpgpu_preflight(dev: crate::intel::Dev, warm: RenderWarmState) -> bool {
    let a = [1u32, 2, 3, 4];
    let b = [10u32, 20, 30, 40];
    let sum_a = a.iter().copied().fold(0u32, u32::wrapping_add);
    let sum_b = b.iter().copied().fold(0u32, u32::wrapping_add);
    let dot = a
        .iter()
        .copied()
        .zip(b.iter().copied())
        .fold(0u32, |acc, (lhs, rhs)| acc.wrapping_add(lhs.wrapping_mul(rhs)));

    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);

        let input_a = warm.vertex_virt as *mut u32;
        let input_b = warm.streamout_virt as *mut u32;
        for i in 0..GPGPU_PREFLIGHT_LANES {
            core::ptr::write_volatile(input_a.add(i), a[i]);
            core::ptr::write_volatile(input_b.add(i), b[i]);
        }
    }
    seed_result_debug_slots(warm);
    crate::intel::dma_flush(warm.vertex_virt, GPGPU_PREFLIGHT_LANES * core::mem::size_of::<u32>());
    crate::intel::dma_flush(
        warm.streamout_virt,
        GPGPU_PREFLIGHT_LANES * core::mem::size_of::<u32>(),
    );
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let batch_tail_bytes = match encode_gpgpu_preflight_batch(batch, dot, sum_a, sum_b) {
        Ok(bytes) => bytes,
        Err(reason) => {
            crate::log!("intel/gpgpu: preflight accepted=0 reason={}\n", reason);
            return false;
        }
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);

    let completed = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_GPGPU_PREFLIGHT_DONE,
        RESULT_SLOT_GPGPU_PREFLIGHT_MARKER_DWORD,
        "gpgpu-preflight",
    );
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let marker = read_result_dword(warm, RESULT_SLOT_GPGPU_PREFLIGHT_MARKER_DWORD);
    let gpu_dot = read_result_dword(warm, RESULT_SLOT_GPGPU_PREFLIGHT_DOT_DWORD);
    let gpu_sum_a = read_result_dword(warm, RESULT_SLOT_GPGPU_PREFLIGHT_SUM_A_DWORD);
    let gpu_sum_b = read_result_dword(warm, RESULT_SLOT_GPGPU_PREFLIGHT_SUM_B_DWORD);
    let gpu_lanes = read_result_dword(warm, RESULT_SLOT_GPGPU_PREFLIGHT_LANES_DWORD);
    let guc_status = crate::intel::guc_status(warm);
    let accepted = completed
        && marker == RCS_EXEC_RESULT_GPGPU_PREFLIGHT_DONE
        && gpu_dot == dot
        && gpu_sum_a == sum_a
        && gpu_sum_b == sum_b
        && gpu_lanes == GPGPU_PREFLIGHT_LANES as u32;
    GPGPU_PREFLIGHT_COMPLETED.store(completed, Ordering::Release);
    GPGPU_PREFLIGHT_ACCEPTED.store(accepted, Ordering::Release);
    GPGPU_PREFLIGHT_MARKER.store(marker, Ordering::Release);
    GPGPU_PREFLIGHT_DOT.store(gpu_dot, Ordering::Release);
    GPGPU_PREFLIGHT_SUM_A.store(gpu_sum_a, Ordering::Release);
    GPGPU_PREFLIGHT_SUM_B.store(gpu_sum_b, Ordering::Release);
    GPGPU_PREFLIGHT_LANES_OBSERVED.store(gpu_lanes, Ordering::Release);

    crate::log!(
        "intel/gpgpu: preflight-readback accepted={} completed={} result_gpu=0x{:X} marker_slot={} marker_expected=0x{:08X} marker_observed=0x{:08X} dot_slot={} dot_expected={} dot_observed={} sum_a_slot={} sum_a_expected={} sum_a_observed={} sum_b_slot={} sum_b_expected={} sum_b_observed={} lanes_slot={} lanes_expected={} lanes_observed={} batch_bytes=0x{:X} does_not_prove=eu_thread_execution_or_matmul_or_guc_scheduling\n",
        accepted as u8,
        completed as u8,
        GPU_VA_RESULT_BASE,
        RESULT_SLOT_GPGPU_PREFLIGHT_MARKER_DWORD,
        RCS_EXEC_RESULT_GPGPU_PREFLIGHT_DONE,
        marker,
        RESULT_SLOT_GPGPU_PREFLIGHT_DOT_DWORD,
        dot,
        gpu_dot,
        RESULT_SLOT_GPGPU_PREFLIGHT_SUM_A_DWORD,
        sum_a,
        gpu_sum_a,
        RESULT_SLOT_GPGPU_PREFLIGHT_SUM_B_DWORD,
        sum_b,
        gpu_sum_b,
        RESULT_SLOT_GPGPU_PREFLIGHT_LANES_DWORD,
        GPGPU_PREFLIGHT_LANES,
        gpu_lanes,
        batch_tail_bytes,
    );

    crate::log!(
        "intel/gpgpu: preflight accepted={} completed={} backend=rcs-mi-store-constants guc_ready={} guc_status=0x{:08X} lanes={} marker=0x{:08X} dot={} sum_a={} sum_b={} batch_bytes=0x{:X} input_a_gpu=0x{:X} input_b_gpu=0x{:X} result_gpu=0x{:X} next=eu-kernel-dispatch does_not_prove=eu_thread_execution_or_matmul_or_guc_scheduling\n",
        accepted as u8,
        completed as u8,
        crate::intel::guc_ready() as u8,
        guc_status,
        gpu_lanes,
        marker,
        gpu_dot,
        gpu_sum_a,
        gpu_sum_b,
        batch_tail_bytes,
        GPU_VA_VERTEX_BASE,
        GPU_VA_STREAMOUT_BASE,
        GPU_VA_RESULT_BASE,
    );

    accepted
}

#[derive(Copy, Clone)]
struct GpgpuEuArtifactProof {
    kernel_uploaded: bool,
    walker_encoded: bool,
    result_changed_by_current_backend: bool,
    kernel_gpu: u64,
    kernel_bytes: usize,
    kernel_sig: u64,
    walker_gpu: u64,
    walker_bytes: usize,
}

fn prepare_gpgpu_eu_artifact(
    warm: RenderWarmState,
    result_changed_by_current_backend: bool,
) -> GpgpuEuArtifactProof {
    let pipeline = crate::intel::shader::triangle_pipeline();
    let kernel = pipeline.ps.code;
    let kernel_bytes = kernel.len() * core::mem::size_of::<u32>();
    let kernel_gpu = GPU_VA_DRAW_STATE_BASE + GPGPU_EU_KERNEL_OFFSET_BYTES as u64;
    let walker_gpu = GPU_VA_BATCH_BASE + GPGPU_WALKER_SCRATCH_OFFSET_BYTES as u64;

    let kernel_uploaded = kernel_bytes != 0
        && GPGPU_EU_KERNEL_OFFSET_BYTES
            .checked_add(kernel_bytes)
            .is_some_and(|end| end <= warm.draw_state_len)
        && upload_and_verify_eu_kernel(warm, kernel);
    GPGPU_EU_KERNEL_UPLOADED.store(kernel_uploaded, Ordering::Release);

    let walker_bytes = core::mem::size_of::<GpgpuWalkerCandidate>();
    let walker_encoded = kernel_uploaded
        && GPGPU_WALKER_SCRATCH_OFFSET_BYTES
            .checked_add(walker_bytes)
            .is_some_and(|end| end <= warm.batch_len)
        && encode_gpgpu_walker_candidate(warm, kernel_gpu, kernel_bytes as u32);
    GPGPU_EU_WALKER_ENCODED.store(walker_encoded, Ordering::Release);

    GpgpuEuArtifactProof {
        kernel_uploaded,
        walker_encoded,
        result_changed_by_current_backend,
        kernel_gpu,
        kernel_bytes,
        kernel_sig: shader_word_signature(kernel),
        walker_gpu,
        walker_bytes,
    }
}

fn upload_and_verify_eu_kernel(warm: RenderWarmState, kernel: &'static [u32]) -> bool {
    unsafe {
        core::ptr::copy_nonoverlapping(
            kernel.as_ptr() as *const u8,
            warm.draw_state_virt.add(GPGPU_EU_KERNEL_OFFSET_BYTES),
            core::mem::size_of_val(kernel),
        );
    }
    crate::intel::dma_flush(
        unsafe { warm.draw_state_virt.add(GPGPU_EU_KERNEL_OFFSET_BYTES) },
        core::mem::size_of_val(kernel),
    );
    let uploaded = unsafe {
        core::slice::from_raw_parts(
            warm.draw_state_virt.add(GPGPU_EU_KERNEL_OFFSET_BYTES) as *const u32,
            kernel.len(),
        )
    };
    uploaded == kernel
}

#[repr(C)]
#[derive(Copy, Clone)]
struct GpgpuWalkerCandidate {
    magic: u32,
    version: u32,
    simd_lanes: u32,
    kernel_gpu_lo: u32,
    kernel_gpu_hi: u32,
    kernel_bytes: u32,
    input_a_gpu_lo: u32,
    input_a_gpu_hi: u32,
    input_b_gpu_lo: u32,
    input_b_gpu_hi: u32,
    result_c_gpu_lo: u32,
    result_c_gpu_hi: u32,
    lanes: u32,
    reserved: [u32; 3],
}

fn encode_gpgpu_walker_candidate(
    warm: RenderWarmState,
    kernel_gpu: u64,
    kernel_bytes: u32,
) -> bool {
    let candidate = GpgpuWalkerCandidate {
        magic: 0x4750_4757,
        version: 1,
        simd_lanes: 8,
        kernel_gpu_lo: kernel_gpu as u32,
        kernel_gpu_hi: (kernel_gpu >> 32) as u32,
        kernel_bytes,
        input_a_gpu_lo: GPU_VA_VERTEX_BASE as u32,
        input_a_gpu_hi: (GPU_VA_VERTEX_BASE >> 32) as u32,
        input_b_gpu_lo: GPU_VA_STREAMOUT_BASE as u32,
        input_b_gpu_hi: (GPU_VA_STREAMOUT_BASE >> 32) as u32,
        result_c_gpu_lo: GPU_VA_RESULT_BASE as u32,
        result_c_gpu_hi: (GPU_VA_RESULT_BASE >> 32) as u32,
        lanes: GPGPU_PREFLIGHT_LANES as u32,
        reserved: [0; 3],
    };
    unsafe {
        core::ptr::copy_nonoverlapping(
            core::ptr::addr_of!(candidate) as *const u8,
            warm.batch_virt.add(GPGPU_WALKER_SCRATCH_OFFSET_BYTES),
            core::mem::size_of::<GpgpuWalkerCandidate>(),
        );
    }
    crate::intel::dma_flush(
        unsafe { warm.batch_virt.add(GPGPU_WALKER_SCRATCH_OFFSET_BYTES) },
        core::mem::size_of::<GpgpuWalkerCandidate>(),
    );
    true
}

fn log_gpgpu_eu_artifact_status(proof: GpgpuEuArtifactProof) {
    crate::log!(
        "intel/gpgpu: eu-offload-proof-ladder input_buffer_a_in_ggtt=1 input_buffer_b_in_ggtt=1 input_a_gpu=0x{:X} input_b_gpu=0x{:X} kernel_shader_uploaded={} eu_kernel_uploaded={} eu_walker_encoded={} gpu_eu_execution_runs=0 result_buffer_c_gpu=0x{:X} result_buffer_c_changed_by_current_backend={} result_buffer_c_changed_by_eu=0 cpu_reads_c_back=1 current_backend=rcs-mi-store-constants walker_submitted=0 blocker=submit-gpgpu-walker next=submit-walker-and-compare-c does_not_prove=eu_thread_execution_or_matmul_or_guc_scheduling\n",
        GPU_VA_VERTEX_BASE,
        GPU_VA_STREAMOUT_BASE,
        proof.kernel_uploaded as u8,
        proof.kernel_uploaded as u8,
        proof.walker_encoded as u8,
        GPU_VA_RESULT_BASE,
        proof.result_changed_by_current_backend as u8,
    );

    crate::log!(
        "intel/gpgpu: eu-artifact-proof eu_kernel_uploaded={} eu_walker_encoded={} kernel_source=triangle-ps-simd8-eu-blob kernel_gpu=0x{:X} kernel_bytes=0x{:X} kernel_sig=0x{:016X} walker_gpu=0x{:X} walker_bytes=0x{:X} submitted=0 eu_execution_runs=0 result_c_changed_by_eu=0 next=submit-walker-and-compare-c does_not_prove=eu_thread_execution_or_matmul\n",
        proof.kernel_uploaded as u8,
        proof.walker_encoded as u8,
        proof.kernel_gpu,
        proof.kernel_bytes,
        proof.kernel_sig,
        proof.walker_gpu,
        proof.walker_bytes,
    );
}

#[derive(Copy, Clone)]
struct GpgpuComputeWalkerProof {
    submitted: bool,
    retired: bool,
    marker: u32,
    dispatch_before: u64,
    dispatch_after: u64,
    dispatch_delta: u64,
    batch_bytes: usize,
}

fn submit_gpgpu_compute_walker_probe(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
) -> GpgpuComputeWalkerProof {
    let dispatch_before = read_gpgpu_threads_dispatched(dev);
    let marker_slot = RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD;
    unsafe {
        let slot = warm
            .result_virt
            .add(marker_slot * core::mem::size_of::<u32>()) as *mut u32;
        core::ptr::write_volatile(slot, 0);
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
    }
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let batch_bytes = match encode_gfx125_compute_walker_probe_batch(batch) {
        Ok(bytes) => bytes,
        Err(reason) => {
            crate::log!("intel/gpgpu: compute-walker accepted=0 reason={}\n", reason);
            return GpgpuComputeWalkerProof {
                submitted: false,
                retired: false,
                marker: 0,
                dispatch_before,
                dispatch_after: dispatch_before,
                dispatch_delta: 0,
                batch_bytes: 0,
            };
        }
    };
    crate::intel::dma_flush(warm.batch_virt, batch_bytes);

    GPGPU_EU_WALKER_SUBMITTED.store(true, Ordering::Release);
    let retired = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD,
        "gpgpu-compute-walker",
    );
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let marker = read_result_dword(warm, RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD);
    let dispatch_after = read_gpgpu_threads_dispatched(dev);
    let dispatch_delta = dispatch_after.saturating_sub(dispatch_before);
    GPGPU_EU_WALKER_RETIRED.store(retired, Ordering::Release);
    GPGPU_EU_DISPATCH_DELTA.store(dispatch_delta.min(u32::MAX as u64) as u32, Ordering::Release);

    GpgpuComputeWalkerProof {
        submitted: true,
        retired,
        marker,
        dispatch_before,
        dispatch_after,
        dispatch_delta,
        batch_bytes,
    }
}

fn read_gpgpu_threads_dispatched(dev: crate::intel::Dev) -> u64 {
    let lo = crate::intel::mmio_read(dev, TS_GPGPU_THREADS_DISPATCHED_LO) as u64;
    let hi = crate::intel::mmio_read(dev, TS_GPGPU_THREADS_DISPATCHED_HI) as u64;
    (hi << 32) | lo
}

fn encode_gfx125_compute_walker_probe_batch(
    batch_dwords: &mut [u32],
) -> Result<usize, &'static str> {
    const STATE_COMPUTE_MODE_CMD: u32 = (3 << 29) | (1 << 24) | (5 << 16);
    const CFE_STATE_CMD: u32 = (3 << 29) | (2 << 27) | (2 << 24) | 4;
    const COMPUTE_WALKER_CMD: u32 = (3 << 29) | (2 << 27) | (2 << 24) | (2 << 18) | 37;
    const PIPELINE_SELECT_GPGPU: u32 =
        (3 << 29) | (1 << 27) | (1 << 24) | (4 << 16) | (0x03 << 8) | 2;
    const GFX125_L1CC_WB: u32 = 2;
    const COMPUTE_SBA_SPAN_BYTES: usize = 0x1000_0000;

    fn push(batch_dwords: &mut [u32], cursor: &mut usize, value: u32) -> Result<(), &'static str> {
        if *cursor >= batch_dwords.len() {
            return Err("compute-walker-batch-exhausted");
        }
        batch_dwords[*cursor] = value;
        *cursor += 1;
        Ok(())
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

    fn push_sba_address(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        enable: bool,
        mocs: u32,
        address: u64,
    ) -> Result<(), &'static str> {
        let low = ((address as u32) & 0xFFFF_F000) | (mocs << 4) | u32::from(enable);
        push(batch_dwords, cursor, low)?;
        push(batch_dwords, cursor, (address >> 32) as u32)
    }

    fn push_sba_size(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        enable: bool,
        size_bytes: usize,
    ) -> Result<(), &'static str> {
        let size_bytes =
            crate::intel::align_up(size_bytes, 4096).ok_or("compute-sba-size-align")?;
        let size_bytes = u32::try_from(size_bytes).map_err(|_| "compute-sba-size-convert")?;
        push(batch_dwords, cursor, (size_bytes & 0xFFFF_F000) | u32::from(enable))
    }

    batch_dwords.fill(0);
    let mut cursor = 0usize;

    push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_FLUSH_BITS)?;
    push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_INVALIDATE_BITS)?;
    push(batch_dwords, &mut cursor, PIPELINE_SELECT_GPGPU)?;
    push(batch_dwords, &mut cursor, STATE_BASE_ADDRESS_CMD)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, 0)?;
    push(
        batch_dwords,
        &mut cursor,
        (RENDER_MOCS << 16) | (GFX125_L1CC_WB << 23),
    )?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, 0)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, 0)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, 0)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, 0)?;
    push_sba_size(batch_dwords, &mut cursor, true, COMPUTE_SBA_SPAN_BYTES)?;
    push_sba_size(batch_dwords, &mut cursor, true, COMPUTE_SBA_SPAN_BYTES)?;
    push_sba_size(batch_dwords, &mut cursor, true, COMPUTE_SBA_SPAN_BYTES)?;
    push_sba_size(batch_dwords, &mut cursor, true, COMPUTE_SBA_SPAN_BYTES)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push_pipe_control(
        batch_dwords,
        &mut cursor,
        PIPE_CONTROL_FLUSH_BITS | PIPE_CONTROL_INVALIDATE_BITS,
    )?;
    push(batch_dwords, &mut cursor, STATE_COMPUTE_MODE_CMD)?;
    push(batch_dwords, &mut cursor, 0xFFFF_0000)?;
    push(batch_dwords, &mut cursor, CFE_STATE_CMD)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, (64 << 16) | (1 << 3))?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    let walker_start = cursor;
    for _ in 0..39 {
        push(batch_dwords, &mut cursor, 0)?;
    }
    batch_dwords[walker_start] = COMPUTE_WALKER_CMD;
    batch_dwords[walker_start + 5] = 0xFFFF_FFFF;
    batch_dwords[walker_start + 6] = 0;
    batch_dwords[walker_start + 7] = 1;
    batch_dwords[walker_start + 8] = 1;
    batch_dwords[walker_start + 9] = 1;

    let idd = walker_start + 18;
    batch_dwords[idd] = (GPU_VA_DRAW_STATE_BASE + GPGPU_EU_KERNEL_OFFSET_BYTES as u64) as u32;
    batch_dwords[idd + 5] = 1;

    let post = walker_start + 26;
    batch_dwords[post] = 1 | (1 << 2) | (RENDER_MOCS << 4);
    let dst = GPU_VA_RESULT_BASE
        + (RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD as u64) * core::mem::size_of::<u32>() as u64;
    batch_dwords[post + 1] = dst as u32;
    batch_dwords[post + 2] = (dst >> 32) as u32;
    batch_dwords[post + 3] = RCS_EXEC_RESULT_COMPUTE_WALKER_DONE;
    batch_dwords[post + 4] = 0;

    push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_FLUSH_BITS)?;
    push(batch_dwords, &mut cursor, MI_BATCH_BUFFER_END)?;
    push(batch_dwords, &mut cursor, MI_NOOP)?;

    Ok(cursor * core::mem::size_of::<u32>())
}

fn log_gpgpu_compute_walker_status(proof: GpgpuComputeWalkerProof) {
    let eu_dispatch_observed = proof.dispatch_delta != 0;
    crate::log!(
        "intel/gpgpu: compute-walker-proof submitted={} retired={} marker=0x{:08X} marker_expected=0x{:08X} dispatch_before={} dispatch_after={} dispatch_delta={} batch_bytes=0x{:X} eu_execution_runs={} result_c_changed_by_eu=0 cpu_reads_c_back=1 backend=gfx125-compute-walker next=replace-eot-only-kernel-with-c-store-kernel does_not_prove=eu_memory_store_or_matmul\n",
        proof.submitted as u8,
        proof.retired as u8,
        proof.marker,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        proof.dispatch_before,
        proof.dispatch_after,
        proof.dispatch_delta,
        proof.batch_bytes,
        eu_dispatch_observed as u8,
    );
}

fn encode_gpgpu_preflight_batch(
    batch_dwords: &mut [u32],
    dot: u32,
    sum_a: u32,
    sum_b: u32,
) -> Result<usize, &'static str> {
    const STORES: [(usize, fn(u32, u32, u32) -> u32); 5] = [
        (RESULT_SLOT_GPGPU_PREFLIGHT_MARKER_DWORD, |_, _, _| RCS_EXEC_RESULT_GPGPU_PREFLIGHT_DONE),
        (RESULT_SLOT_GPGPU_PREFLIGHT_DOT_DWORD, |dot, _, _| dot),
        (RESULT_SLOT_GPGPU_PREFLIGHT_SUM_A_DWORD, |_, sum_a, _| sum_a),
        (RESULT_SLOT_GPGPU_PREFLIGHT_SUM_B_DWORD, |_, _, sum_b| sum_b),
        (RESULT_SLOT_GPGPU_PREFLIGHT_LANES_DWORD, |_, _, _| GPGPU_PREFLIGHT_LANES as u32),
    ];
    const STORE_DWORDS: usize = 4;
    const END_DWORDS: usize = 2;

    if batch_dwords.len() < STORES.len() * STORE_DWORDS + END_DWORDS {
        return Err("batch-too-small");
    }

    let mut idx = 0;
    for (slot, value_fn) in STORES {
        let dst = GPU_VA_RESULT_BASE + (slot as u64) * core::mem::size_of::<u32>() as u64;
        batch_dwords[idx] = MI_STORE_DATA_IMM_GGTT_DW1;
        batch_dwords[idx + 1] = dst as u32;
        batch_dwords[idx + 2] = (dst >> 32) as u32;
        batch_dwords[idx + 3] = value_fn(dot, sum_a, sum_b);
        idx += STORE_DWORDS;
    }
    batch_dwords[idx] = MI_BATCH_BUFFER_END;
    batch_dwords[idx + 1] = MI_NOOP;
    idx += END_DWORDS;

    Ok(idx * core::mem::size_of::<u32>())
}
