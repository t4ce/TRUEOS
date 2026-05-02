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

fn encode_gpgpu_preflight_batch(
    batch_dwords: &mut [u32],
    dot: u32,
    sum_a: u32,
    sum_b: u32,
) -> Result<usize, &'static str> {
    const STORES: [(usize, fn(u32, u32, u32) -> u32); 5] = [
        (
            RESULT_SLOT_GPGPU_PREFLIGHT_MARKER_DWORD,
            |_, _, _| RCS_EXEC_RESULT_GPGPU_PREFLIGHT_DONE,
        ),
        (RESULT_SLOT_GPGPU_PREFLIGHT_DOT_DWORD, |dot, _, _| dot),
        (RESULT_SLOT_GPGPU_PREFLIGHT_SUM_A_DWORD, |_, sum_a, _| sum_a),
        (RESULT_SLOT_GPGPU_PREFLIGHT_SUM_B_DWORD, |_, _, sum_b| sum_b),
        (
            RESULT_SLOT_GPGPU_PREFLIGHT_LANES_DWORD,
            |_, _, _| GPGPU_PREFLIGHT_LANES as u32,
        ),
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
