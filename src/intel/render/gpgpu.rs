const GPGPU_PREFLIGHT_LANES: usize = 4;
const GPGPU_BURN_MIN_ROWS: usize = 512;
const GPGPU_BURN_MIN_K_DIM: usize = 512;
const GPU_PROGRAM_SHARED_RAM_WRITE_EXPECTED: u32 = RCS_EXEC_RESULT_GPGPU_EU_C_STORE_DONE;

#[derive(Copy, Clone)]
struct GpgpuEuProgram {
    name: &'static str,
    words: &'static [u32],
    expects_store: bool,
}

// First active proof target for this machine: copy the thread payload header
// from R0 into a high GRF payload slot, then use Mesa's GFX12/TGL Thread
// Spawner EOT send.  8086:4680 is ADL-S GT1/UHD 770, which Mesa maps to
// GFX12.0; do not use the GFX12.5 Gateway/COMPUTE_WALKER path here.
// Assembled locally with Mesa brw_asm from .codex_tmp/gfx12_eot_g127.asm.
static GPU_PROGRAM_EOT_ONLY_CODE: [u32; 8] = [
    0x80030061,
    0x7F050220,
    0x00460005,
    0x00000000,
    0x80030131,
    0x00000004,
    0x70007F0C,
    0x00000000,
];

// Legacy diagnostic dataport probe.  This hand-written EU blob is not the final
// Burn/matmul kernel path; it is only a bounded oscilloscope for the current
// phase: if the dispatched EU thread can write shared RAM, then we know it
// decoded enough instructions to issue a dataport side effect before/around EOT.
static GPU_PROGRAM_SHARED_RAM_WRITE_CODE: [u32; 12] = [
    0xA07E0061,
    0x00010000,
    0xA0780061,
    GPU_PROGRAM_SHARED_RAM_WRITE_EXPECTED,
    0xA07A0061,
    0x3F810000,
    0xA07C0061,
    0x3F810000,
    0x00040132,
    0x00000004,
    0x50007E14,
    0x00C47834,
];

// Assembled with Mesa brw_asm from:
// .codex_tmp/gfx12_hdc1_bti34_store_eot.asm
//
// This follows Mesa executor's GFX12.0 write shape: hdc1 untyped surface write,
// SIMD8, Mask = 0xe.  This variant targets BTI 0x34, which the command stream
// binds to the CPU-readback dword surface, then attempts normal TS EOT.
static GPU_PROGRAM_HDC1_BTI34_STORE_EOT_CODE: [u32; 20] = [
    0x80030061,
    0x04054660,
    0x00000000,
    0xC0DE7733,
    0x80030061,
    0x7F054220,
    0x00000000,
    0x00000000,
    0x00030131,
    0x00000000,
    0xCC687F0C,
    0x009A040C,
    0x80030061,
    0x7F050220,
    0x00460005,
    0x00000000,
    0x80030131,
    0x00000004,
    0x70007F0C,
    0x00000000,
];

fn selected_gpgpu_eu_program() -> GpgpuEuProgram {
    GpgpuEuProgram {
        name: "gfx12-hdc1-bti34-store-then-ts-eot",
        words: &GPU_PROGRAM_HDC1_BTI34_STORE_EOT_CODE,
        expects_store: true,
    }
}

fn gpgpu_store_eot_program() -> GpgpuEuProgram {
    GpgpuEuProgram {
        name: "diagnostic-legacy-dataport-store-before-eot",
        words: &GPU_PROGRAM_SHARED_RAM_WRITE_CODE,
        expects_store: true,
    }
}

const GPGPU_C_STORE_KERNEL_SEND_DWORD: usize = 11;
const GPGPU_C_STORE_KERNEL_IMM_DWORD: usize = 3;
const GPGPU_STORE_BINDING_TABLE_OFFSET_BYTES: usize = 0x3400;
const GPGPU_STORE_SURFACE_STATE_OFFSET_BYTES: usize = 0x3500;
const GPGPU_STORE_BINDING_TABLE_INDEX: usize = 0x34;
const GPGPU_STORE_BINDING_TABLE_ENTRIES: usize = GPGPU_STORE_BINDING_TABLE_INDEX + 1;
const GPGPU_STORE_SURFACE_DWORDS: usize = 16;
const SURFTYPE_BUFFER: u32 = 4;
const SURFACE_FORMAT_RAW: u32 = 0x1FF;

static GPGPU_PREFLIGHT_SUBMITTED: AtomicBool = AtomicBool::new(false);
static GPGPU_PREFLIGHT_ACCEPTED: AtomicBool = AtomicBool::new(false);
static GPGPU_PREFLIGHT_COMPLETED: AtomicBool = AtomicBool::new(false);
static GPGPU_PREFLIGHT_MARKER: AtomicU32 = AtomicU32::new(0);
static GPGPU_PREFLIGHT_DOT: AtomicU32 = AtomicU32::new(0);
static GPGPU_PREFLIGHT_SUM_A: AtomicU32 = AtomicU32::new(0);
static GPGPU_PREFLIGHT_SUM_B: AtomicU32 = AtomicU32::new(0);
static GPGPU_PREFLIGHT_LANES_OBSERVED: AtomicU32 = AtomicU32::new(0);
static GPGPU_WARM_BUFFERS_MAPPED: AtomicBool = AtomicBool::new(false);
static GPGPU_TILE_ARENA_MAPPED: AtomicBool = AtomicBool::new(false);
static GPGPU_TILE_ARENA_STATUS_LOGGED: AtomicBool = AtomicBool::new(false);
static GPGPU_EU_KERNEL_UPLOADED: AtomicBool = AtomicBool::new(false);
static GPGPU_EU_WALKER_ENCODED: AtomicBool = AtomicBool::new(false);
static GPGPU_EU_WALKER_SUBMITTED: AtomicBool = AtomicBool::new(false);
static GPGPU_EU_WALKER_RETIRED: AtomicBool = AtomicBool::new(false);
static GPGPU_EU_DISPATCH_DELTA: AtomicU32 = AtomicU32::new(0);
static GPGPU_EU_C_STORE_VALUE: AtomicU32 = AtomicU32::new(0);

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
    pub(crate) eu_c_store_value: u32,
    pub(crate) result_c_changed_by_eu: bool,
}

pub(crate) fn gpgpu_preflight_status() -> GpgpuPreflightStatus {
    let warm = warm_state();
    let arena_bytes = warm.map_or(0, |warm| warm.gpgpu_arena_len);
    let eu_dispatch_delta = GPGPU_EU_DISPATCH_DELTA.load(Ordering::Acquire);
    let eu_c_store_value = GPGPU_EU_C_STORE_VALUE.load(Ordering::Acquire);
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
        eu_dispatch_delta,
        eu_c_store_value,
        result_c_changed_by_eu: eu_dispatch_delta != 0
            && eu_c_store_value == GPU_PROGRAM_SHARED_RAM_WRITE_EXPECTED,
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
            < (RESULT_SLOT_GPGPU_EU_C_STORE_DWORD + 1) * core::mem::size_of::<u32>()
    {
        crate::log!("intel/gpgpu: preflight skipped reason=warm-buffers\n");
        return;
    }

    if PRIMARY_DISABLE_RENDER_BRINGUP && !GPGPU_SUBMIT_WHEN_PRIMARY_RENDER_DISABLED {
        let arena_mapped = ensure_gpgpu_tile_arena_mapped(dev, warm);
        log_gpgpu_tile_arena_status(warm, arena_mapped);
        let eu_artifact = prepare_gpgpu_program_artifact(warm, false);
        log_gpgpu_program_artifact_status(eu_artifact);
        crate::log!(
            "intel/gpgpu: preflight skipped reason=render-bringup-disabled artifact_only=1 gpu_program_uploaded={} start_command_encoded={}\n",
            eu_artifact.program_uploaded as u8,
            eu_artifact.walker_encoded as u8,
        );
        return;
    }
    if PRIMARY_DISABLE_RENDER_BRINGUP {
        crate::log!(
            "intel/gpgpu: primary-render-disabled-but-gpgpu-submit-enabled artifact_only=0\n"
        );
    }

    if !forcewake_render_acquire(warm) {
        crate::log!("intel/gpgpu: preflight skipped reason=forcewake\n");
        return;
    }

    if !ensure_gpgpu_warm_buffers_mapped(dev, warm) {
        crate::log!("intel/gpgpu: preflight skipped reason=warm-buffer-ggtt-map\n");
        return;
    }
    let arena_mapped = ensure_gpgpu_tile_arena_mapped(dev, warm);
    log_gpgpu_tile_arena_status(warm, arena_mapped);
    crate::intel::log_guc_submission_contract(dev, "gpgpu-preflight");
    let accepted = submit_gpgpu_preflight(dev, warm);
    if !accepted {
        recover_render_engine_after_nonretired_submit(dev, warm, "gpgpu-preflight");
    }
    let eu_artifact = prepare_gpgpu_program_artifact(warm, accepted);
    log_gpgpu_program_artifact_status(eu_artifact);
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

fn ensure_gpgpu_warm_buffers_mapped(dev: crate::intel::Dev, warm: RenderWarmState) -> bool {
    if GPGPU_WARM_BUFFERS_MAPPED.load(Ordering::Acquire) {
        return true;
    }

    let mapped = super::map_ggtt(dev, warm.ring_phys, warm.ring_len, GPU_VA_RING_BASE)
        && super::map_ggtt(dev, warm.context_phys, warm.context_len, GPU_VA_CONTEXT_BASE)
        && super::map_ggtt(dev, warm.batch_phys, warm.batch_len, GPU_VA_BATCH_BASE)
        && super::map_ggtt(dev, warm.draw_state_phys, warm.draw_state_len, GPU_VA_DRAW_STATE_BASE)
        && super::map_ggtt(dev, warm.vertex_phys, warm.vertex_len, GPU_VA_VERTEX_BASE)
        && super::map_ggtt(dev, warm.result_phys, warm.result_len, GPU_VA_RESULT_BASE)
        && super::map_ggtt(dev, warm.streamout_phys, warm.streamout_len, GPU_VA_STREAMOUT_BASE);
    if mapped {
        super::ggtt_invalidate(dev);
        GPGPU_WARM_BUFFERS_MAPPED.store(true, Ordering::Release);
    }
    crate::log!(
        "intel/gpgpu: warm-buffers mapped={} ring=0x{:X} context=0x{:X} batch=0x{:X} result=0x{:X}\n",
        mapped as u8,
        GPU_VA_RING_BASE,
        GPU_VA_CONTEXT_BASE,
        GPU_VA_BATCH_BASE,
        GPU_VA_RESULT_BASE,
    );
    mapped
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
struct GpgpuProgramArtifactProof {
    program_name: &'static str,
    expects_store: bool,
    program_uploaded: bool,
    walker_encoded: bool,
    result_changed_by_current_backend: bool,
    program_gpu: u64,
    program_bytes: usize,
    program_sig: u64,
    walker_gpu: u64,
    walker_bytes: usize,
}

#[derive(Copy, Clone)]
struct GpgpuStoreSurfaceState {
    ready: bool,
    binding_table_offset: usize,
    surface_state_offset: usize,
    binding_table_index: usize,
    surface_gpu: u64,
    target_gpu: u64,
    surface_dword0: u32,
    binding_entry: u32,
}

fn prepare_gpgpu_program_artifact(
    warm: RenderWarmState,
    result_changed_by_current_backend: bool,
) -> GpgpuProgramArtifactProof {
    let program = selected_gpgpu_eu_program();
    let program_bytes = program.words.len() * core::mem::size_of::<u32>();
    let program_gpu = GPU_VA_DRAW_STATE_BASE + GPGPU_EU_KERNEL_OFFSET_BYTES as u64;
    let walker_gpu = GPU_VA_BATCH_BASE + GPGPU_WALKER_SCRATCH_OFFSET_BYTES as u64;

    let program_uploaded = program_bytes != 0
        && GPGPU_EU_KERNEL_OFFSET_BYTES
            .checked_add(program_bytes)
            .is_some_and(|end| end <= warm.draw_state_len)
        && upload_and_verify_gpu_program(warm, program.words);
    GPGPU_EU_KERNEL_UPLOADED.store(program_uploaded, Ordering::Release);

    let walker_bytes = core::mem::size_of::<GpgpuWalkerCandidate>();
    let walker_encoded = program_uploaded
        && GPGPU_WALKER_SCRATCH_OFFSET_BYTES
            .checked_add(walker_bytes)
            .is_some_and(|end| end <= warm.batch_len)
        && encode_gpgpu_walker_candidate(warm, program_gpu, program_bytes as u32);
    GPGPU_EU_WALKER_ENCODED.store(walker_encoded, Ordering::Release);

    GpgpuProgramArtifactProof {
        program_name: program.name,
        expects_store: program.expects_store,
        program_uploaded,
        walker_encoded,
        result_changed_by_current_backend,
        program_gpu,
        program_bytes,
        program_sig: shader_word_signature(program.words),
        walker_gpu,
        walker_bytes,
    }
}

fn upload_and_verify_gpu_program(warm: RenderWarmState, program: &'static [u32]) -> bool {
    unsafe {
        core::ptr::copy_nonoverlapping(
            program.as_ptr() as *const u8,
            warm.draw_state_virt.add(GPGPU_EU_KERNEL_OFFSET_BYTES),
            core::mem::size_of_val(program),
        );
    }
    crate::intel::dma_flush(
        unsafe { warm.draw_state_virt.add(GPGPU_EU_KERNEL_OFFSET_BYTES) },
        core::mem::size_of_val(program),
    );
    let uploaded = unsafe {
        core::slice::from_raw_parts(
            warm.draw_state_virt.add(GPGPU_EU_KERNEL_OFFSET_BYTES) as *const u32,
            program.len(),
        )
    };
    uploaded == program
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

fn log_gpgpu_program_artifact_status(proof: GpgpuProgramArtifactProof) {
    crate::log!(
        "intel/gpgpu: gpu-shared-ram-ladder input_buffer_a_in_ggtt=1 input_buffer_b_in_ggtt=1 input_a_gpu=0x{:X} input_b_gpu=0x{:X} gpu_program_uploaded={} gpu_start_command_encoded={} gpu_program_started=0 shared_ram_c_gpu=0x{:X} shared_ram_c_changed_by_current_backend={} shared_ram_c_changed_by_program=0 cpu_reads_c_back=1 current_backend=rcs-command-store-constants start_submitted=0 blocker=start-gpu-program next=start-program-and-compare-shared-ram does_not_prove=program_body_or_matmul\n",
        GPU_VA_VERTEX_BASE,
        GPU_VA_STREAMOUT_BASE,
        proof.program_uploaded as u8,
        proof.walker_encoded as u8,
        GPU_VA_RESULT_BASE,
        proof.result_changed_by_current_backend as u8,
    );

    crate::log!(
        "intel/gpgpu: gpu-program-artifact gpu_program_uploaded={} gpu_start_command_encoded={} program_source={} expects_store={} program_gpu=0x{:X} program_bytes=0x{:X} program_sig=0x{:016X} start_command_gpu=0x{:X} start_command_bytes=0x{:X} shared_ram_slot={} shared_ram_expected=0x{:08X} submitted=0 started=0 wrote_shared_ram=0 next=start-program-and-compare-shared-ram does_not_prove=program_body_or_matmul\n",
        proof.program_uploaded as u8,
        proof.walker_encoded as u8,
        proof.program_name,
        proof.expects_store as u8,
        proof.program_gpu,
        proof.program_bytes,
        proof.program_sig,
        proof.walker_gpu,
        proof.walker_bytes,
        RESULT_SLOT_GPGPU_EU_C_STORE_DWORD,
        GPU_PROGRAM_SHARED_RAM_WRITE_EXPECTED,
    );
    log_gpgpu_program_contract(proof);
}

fn log_gpgpu_program_contract(proof: GpgpuProgramArtifactProof) {
    let active_program = selected_gpgpu_eu_program();
    let program = active_program.words;
    let immediate = program
        .get(GPGPU_C_STORE_KERNEL_IMM_DWORD)
        .copied()
        .unwrap_or(0);
    crate::log!(
        "intel/gpgpu: gpu-program-contract source={} uploaded={} expects_store={} program_gpu=0x{:X} words={} w0=0x{:08X} w1=0x{:08X} w2=0x{:08X} w3=0x{:08X} w4=0x{:08X} w5=0x{:08X} w6=0x{:08X} w7=0x{:08X} active_send_w8=0x{:08X} active_send_w9=0x{:08X} active_send_desc_w10=0x{:08X} active_send_exdesc_w11=0x{:08X} immediate_expected=0x{:08X} shared_ram_c_gpu=0x{:X} shared_ram_slot={} binding_table_present={} surface_state_present={} curbe_present=0 expected_failure_if_send_needs_surface={} microscope=program-store-contract does_not_prove=shared_ram_store_or_matmul\n",
        proof.program_name,
        proof.program_uploaded as u8,
        proof.expects_store as u8,
        proof.program_gpu,
        program.len(),
        program.first().copied().unwrap_or(0),
        program.get(1).copied().unwrap_or(0),
        program.get(2).copied().unwrap_or(0),
        immediate,
        program.get(4).copied().unwrap_or(0),
        program.get(5).copied().unwrap_or(0),
        program.get(6).copied().unwrap_or(0),
        program.get(7).copied().unwrap_or(0),
        program.get(8).copied().unwrap_or(0),
        program.get(9).copied().unwrap_or(0),
        program.get(10).copied().unwrap_or(0),
        program.get(11).copied().unwrap_or(0),
        GPU_PROGRAM_SHARED_RAM_WRITE_EXPECTED,
        GPU_VA_RESULT_BASE + (RESULT_SLOT_GPGPU_EU_C_STORE_DWORD as u64) * core::mem::size_of::<u32>() as u64,
        RESULT_SLOT_GPGPU_EU_C_STORE_DWORD,
        proof.expects_store as u8,
        proof.expects_store as u8,
        proof.expects_store as u8,
    );
}

fn prepare_gpgpu_store_surface_state(warm: RenderWarmState) -> GpgpuStoreSurfaceState {
    let target_gpu = GPU_VA_RESULT_BASE
        + (RESULT_SLOT_GPGPU_EU_C_STORE_DWORD as u64) * core::mem::size_of::<u32>() as u64;
    let binding_table_bytes = GPGPU_STORE_BINDING_TABLE_ENTRIES * core::mem::size_of::<u32>();
    let surface_bytes = GPGPU_STORE_SURFACE_DWORDS * core::mem::size_of::<u32>();
    let binding_end = GPGPU_STORE_BINDING_TABLE_OFFSET_BYTES.saturating_add(binding_table_bytes);
    let surface_end = GPGPU_STORE_SURFACE_STATE_OFFSET_BYTES.saturating_add(surface_bytes);
    let binding_table_aligned = GPGPU_STORE_BINDING_TABLE_OFFSET_BYTES & 0x3F == 0;
    let surface_aligned = GPGPU_STORE_SURFACE_STATE_OFFSET_BYTES & 0x3F == 0;
    let ready = binding_table_aligned
        && surface_aligned
        && binding_end <= warm.draw_state_len
        && surface_end <= warm.draw_state_len;
    if !ready {
        crate::log!(
            "intel/gpgpu: gpu-program-surface-state ready=0 reason=draw-state-bounds bt_off=0x{:X} bt_bytes=0x{:X} surf_off=0x{:X} surf_bytes=0x{:X} draw_state_len=0x{:X}\n",
            GPGPU_STORE_BINDING_TABLE_OFFSET_BYTES,
            binding_table_bytes,
            GPGPU_STORE_SURFACE_STATE_OFFSET_BYTES,
            surface_bytes,
            warm.draw_state_len,
        );
        return GpgpuStoreSurfaceState {
            ready: false,
            binding_table_offset: GPGPU_STORE_BINDING_TABLE_OFFSET_BYTES,
            surface_state_offset: GPGPU_STORE_SURFACE_STATE_OFFSET_BYTES,
            binding_table_index: GPGPU_STORE_BINDING_TABLE_INDEX,
            surface_gpu: GPU_VA_DRAW_STATE_BASE + GPGPU_STORE_SURFACE_STATE_OFFSET_BYTES as u64,
            target_gpu,
            surface_dword0: 0,
            binding_entry: 0,
        };
    }

    let binding_entry = GPGPU_STORE_SURFACE_STATE_OFFSET_BYTES as u32;
    let surface_dword0 = (SURFTYPE_BUFFER << 29) | (SURFACE_FORMAT_RAW << 18);
    unsafe {
        let binding_table =
            warm.draw_state_virt.add(GPGPU_STORE_BINDING_TABLE_OFFSET_BYTES) as *mut u32;
        for index in 0..GPGPU_STORE_BINDING_TABLE_ENTRIES {
            core::ptr::write_volatile(binding_table.add(index), binding_entry);
        }

        let surface = warm
            .draw_state_virt
            .add(GPGPU_STORE_SURFACE_STATE_OFFSET_BYTES) as *mut u32;
        for index in 0..GPGPU_STORE_SURFACE_DWORDS {
            core::ptr::write_volatile(surface.add(index), 0);
        }
        core::ptr::write_volatile(surface.add(0), surface_dword0);
        core::ptr::write_volatile(surface.add(1), RENDER_MOCS << 24);
        core::ptr::write_volatile(surface.add(2), 3);
        core::ptr::write_volatile(surface.add(3), 0);
        core::ptr::write_volatile(surface.add(8), target_gpu as u32);
        core::ptr::write_volatile(surface.add(9), (target_gpu >> 32) as u32);
    }
    crate::intel::dma_flush(
        unsafe { warm.draw_state_virt.add(GPGPU_STORE_BINDING_TABLE_OFFSET_BYTES) },
        binding_table_bytes,
    );
    crate::intel::dma_flush(
        unsafe { warm.draw_state_virt.add(GPGPU_STORE_SURFACE_STATE_OFFSET_BYTES) },
        surface_bytes,
    );
    crate::log!(
        "intel/gpgpu: gpu-program-surface-state ready=1 bti=0x{:02X} bt_off=0x{:X} bt_entries={} bt_entry=0x{:08X} surf_off=0x{:X} surf_gpu=0x{:X} target_gpu=0x{:X} surf0=0x{:08X} surf1=0x{:08X} surf2=0x{:08X} surf3=0x{:08X} note=bind-send-bti-to-result-raw-buffer\n",
        GPGPU_STORE_BINDING_TABLE_INDEX,
        GPGPU_STORE_BINDING_TABLE_OFFSET_BYTES,
        GPGPU_STORE_BINDING_TABLE_ENTRIES,
        binding_entry,
        GPGPU_STORE_SURFACE_STATE_OFFSET_BYTES,
        GPU_VA_DRAW_STATE_BASE + GPGPU_STORE_SURFACE_STATE_OFFSET_BYTES as u64,
        target_gpu,
        surface_dword0,
        RENDER_MOCS << 24,
        3,
        0,
    );

    GpgpuStoreSurfaceState {
        ready: true,
        binding_table_offset: GPGPU_STORE_BINDING_TABLE_OFFSET_BYTES,
        surface_state_offset: GPGPU_STORE_SURFACE_STATE_OFFSET_BYTES,
        binding_table_index: GPGPU_STORE_BINDING_TABLE_INDEX,
        surface_gpu: GPU_VA_DRAW_STATE_BASE + GPGPU_STORE_SURFACE_STATE_OFFSET_BYTES as u64,
        target_gpu,
        surface_dword0,
        binding_entry,
    }
}

fn disabled_gpgpu_store_surface_state() -> GpgpuStoreSurfaceState {
    GpgpuStoreSurfaceState {
        ready: false,
        binding_table_offset: GPGPU_STORE_BINDING_TABLE_OFFSET_BYTES,
        surface_state_offset: GPGPU_STORE_SURFACE_STATE_OFFSET_BYTES,
        binding_table_index: GPGPU_STORE_BINDING_TABLE_INDEX,
        surface_gpu: GPU_VA_DRAW_STATE_BASE + GPGPU_STORE_SURFACE_STATE_OFFSET_BYTES as u64,
        target_gpu: GPU_VA_RESULT_BASE
            + (RESULT_SLOT_GPGPU_EU_C_STORE_DWORD as u64) * core::mem::size_of::<u32>() as u64,
        surface_dword0: 0,
        binding_entry: 0,
    }
}

#[derive(Copy, Clone)]
struct GpgpuComputeWalkerProof {
    program_name: &'static str,
    expects_store: bool,
    submitted: bool,
    retired: bool,
    marker: u32,
    dispatch_before: u64,
    dispatch_after: u64,
    dispatch_delta: u64,
    c_value: u32,
    result_c_changed_by_eu: bool,
    batch_bytes: usize,
}

fn submit_gpgpu_compute_walker_probe(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
) -> GpgpuComputeWalkerProof {
    let program = selected_gpgpu_eu_program();
    let dispatch_before = read_gpgpu_threads_dispatched(dev);
    let marker_slot = RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD;
    unsafe {
        let slot = warm
            .result_virt
            .add(marker_slot * core::mem::size_of::<u32>()) as *mut u32;
        core::ptr::write_volatile(slot, 0);
        let c_slot = warm
            .result_virt
            .add(RESULT_SLOT_GPGPU_EU_C_STORE_DWORD * core::mem::size_of::<u32>())
            as *mut u32;
        core::ptr::write_volatile(c_slot, 0);
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
    }
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let store_surface = if program.expects_store {
        prepare_gpgpu_store_surface_state(warm)
    } else {
        disabled_gpgpu_store_surface_state()
    };
    let batch_bytes = match encode_gfx12_gpgpu_walker_probe_batch(batch, store_surface, program) {
        Ok(bytes) => bytes,
        Err(reason) => {
            crate::log!("intel/gpgpu: compute-walker accepted=0 reason={}\n", reason);
            return GpgpuComputeWalkerProof {
                program_name: program.name,
                expects_store: program.expects_store,
                submitted: false,
                retired: false,
                marker: 0,
                dispatch_before,
                dispatch_after: dispatch_before,
                dispatch_delta: 0,
                c_value: 0,
                result_c_changed_by_eu: false,
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
    let c_value = read_result_dword(warm, RESULT_SLOT_GPGPU_EU_C_STORE_DWORD);
    let post_pipeline = read_result_dword(warm, 23);
    let post_sba = read_result_dword(warm, 24);
    let post_scm = read_result_dword(warm, 25);
    let post_cfe = read_result_dword(warm, 26);
    let mut expected_hits_mask = 0u64;
    for slot in 0..64 {
        if read_result_dword(warm, slot) == GPU_PROGRAM_SHARED_RAM_WRITE_EXPECTED {
            expected_hits_mask |= 1u64 << slot;
        }
    }
    let dispatch_after = read_gpgpu_threads_dispatched(dev);
    let dispatch_delta = dispatch_after.saturating_sub(dispatch_before);
    let result_c_changed_by_eu = program.expects_store
        && c_value == GPU_PROGRAM_SHARED_RAM_WRITE_EXPECTED
        && dispatch_delta != 0;
    GPGPU_EU_WALKER_RETIRED.store(retired, Ordering::Release);
    GPGPU_EU_DISPATCH_DELTA.store(dispatch_delta.min(u32::MAX as u64) as u32, Ordering::Release);
    GPGPU_EU_C_STORE_VALUE.store(c_value, Ordering::Release);
    crate::log!(
        "intel/gpgpu: compute-walker-breadcrumbs post_pipeline=0x{:08X} post_sba=0x{:08X} post_scm=0x{:08X} post_cfe=0x{:08X}\n",
        post_pipeline,
        post_sba,
        post_scm,
        post_cfe,
    );
    crate::log!(
        "intel/gpgpu: result-window-scan expected=0x{:08X} expected_hits_mask_lo64=0x{:016X} target_slot={} target_gpu=0x{:X} s16=0x{:08X} s17=0x{:08X} s18=0x{:08X} s19=0x{:08X} s20=0x{:08X} s21=0x{:08X} s22=0x{:08X} s23=0x{:08X} s24=0x{:08X} s25=0x{:08X} s26=0x{:08X} s27=0x{:08X} s28=0x{:08X} s29=0x{:08X} s30=0x{:08X} s31=0x{:08X} note=checks-nearby-result-buffer-for-misplaced-eu-store\n",
        GPU_PROGRAM_SHARED_RAM_WRITE_EXPECTED,
        expected_hits_mask,
        RESULT_SLOT_GPGPU_EU_C_STORE_DWORD,
        GPU_VA_RESULT_BASE
            + (RESULT_SLOT_GPGPU_EU_C_STORE_DWORD as u64)
                * core::mem::size_of::<u32>() as u64,
        read_result_dword(warm, 16),
        read_result_dword(warm, 17),
        read_result_dword(warm, 18),
        read_result_dword(warm, 19),
        read_result_dword(warm, 20),
        read_result_dword(warm, 21),
        read_result_dword(warm, 22),
        read_result_dword(warm, 23),
        read_result_dword(warm, 24),
        read_result_dword(warm, 25),
        read_result_dword(warm, 26),
        read_result_dword(warm, 27),
        read_result_dword(warm, 28),
        read_result_dword(warm, 29),
        read_result_dword(warm, 30),
        read_result_dword(warm, 31),
    );

    GpgpuComputeWalkerProof {
        program_name: program.name,
        expects_store: program.expects_store,
        submitted: true,
        retired,
        marker,
        dispatch_before,
        dispatch_after,
        dispatch_delta,
        c_value,
        result_c_changed_by_eu,
        batch_bytes,
    }
}

fn read_gpgpu_threads_dispatched(dev: crate::intel::Dev) -> u64 {
    let lo = crate::intel::mmio_read(dev, TS_GPGPU_THREADS_DISPATCHED_LO) as u64;
    let hi = crate::intel::mmio_read(dev, TS_GPGPU_THREADS_DISPATCHED_HI) as u64;
    (hi << 32) | lo
}

// Disabled reference only.  COMPUTE_WALKER/CFE_STATE is for GFX12.5+ (for
// example DG2); the current baremetal target is 8086:4680 ADL-S GT1/UHD 770,
// a GFX12.0 part.  Submitting this path on that device pins at CFE_STATE before
// any EU thread starts, so runtime dispatch intentionally never calls it.
#[allow(dead_code)]
fn encode_gfx125_compute_walker_probe_batch(
    batch_dwords: &mut [u32],
    store_surface: GpgpuStoreSurfaceState,
    program: GpgpuEuProgram,
) -> Result<usize, &'static str> {
    const STATE_COMPUTE_MODE_CMD: u32 = (3 << 29) | (1 << 24) | (5 << 16);
    const CFE_STATE_CMD: u32 = (3 << 29) | (2 << 27) | (2 << 24) | 4;
    const COMPUTE_WALKER_CMD: u32 = (3 << 29) | (2 << 27) | (2 << 24) | (2 << 18) | 37;
    const PIPELINE_SELECT_BASE: u32 = (3 << 29) | (1 << 27) | (1 << 24) | (4 << 16);
    const PIPELINE_SELECT_GFX125_MASK: u32 = 0x93 << 8;
    const PIPELINE_SELECT_MEDIA_SAMPLER_DOP_CG_ENABLE: u32 = 1 << 4;
    const PIPELINE_SELECT_GPGPU: u32 = PIPELINE_SELECT_BASE
        | PIPELINE_SELECT_GFX125_MASK
        | PIPELINE_SELECT_MEDIA_SAMPLER_DOP_CG_ENABLE
        | 2;
    const COMPUTE_SBA_SPAN_BYTES: usize = 0x1000_0000;
    const CS_GPR_STAMP_HI: u32 = 0x0000_0001;
    const CS_GPR0_STAMP_LO: u32 = 0xC5A0_2650;
    const CS_GPR1_STAMP_LO: u32 = 0xC5A0_2658;
    const COMPUTE_WALKER_BODY_DWORDS: usize = 38;
    const COMPUTE_WALKER_DWORDS: usize = 1 + COMPUTE_WALKER_BODY_DWORDS;
    const BODY_INTERFACE_DESCRIPTOR_DWORD: usize = 17;
    const BODY_POSTSYNC_DWORD: usize = 25;

    fn push(batch_dwords: &mut [u32], cursor: &mut usize, value: u32) -> Result<(), &'static str> {
        if *cursor >= batch_dwords.len() {
            return Err("compute-walker-batch-exhausted");
        }
        batch_dwords[*cursor] = value;
        *cursor += 1;
        Ok(())
    }

    fn push_pipe_control_full(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        header_flags: u32,
        dw1_flags: u32,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, PIPE_CONTROL_CMD | header_flags)?;
        push(batch_dwords, cursor, dw1_flags)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)
    }

    fn push_pipe_control(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        flags: u32,
    ) -> Result<(), &'static str> {
        push_pipe_control_full(batch_dwords, cursor, 0, flags)
    }

    fn push_store_marker(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        slot: usize,
        value: u32,
    ) -> Result<(), &'static str> {
        let dst = GPU_VA_RESULT_BASE + (slot as u64) * core::mem::size_of::<u32>() as u64;
        push(batch_dwords, cursor, MI_STORE_DATA_IMM_GGTT_DW1)?;
        push(batch_dwords, cursor, dst as u32)?;
        push(batch_dwords, cursor, (dst >> 32) as u32)?;
        push(batch_dwords, cursor, value)
    }

    fn push_cs_gpr_stamp(batch_dwords: &mut [u32], cursor: &mut usize) -> Result<(), &'static str> {
        push(batch_dwords, cursor, mi_lri_cmd(4, MI_LRI_FORCE_POSTED))?;
        push(batch_dwords, cursor, RCS_CS_GPR_REL_BASE as u32)?;
        push(batch_dwords, cursor, CS_GPR0_STAMP_LO)?;
        push(batch_dwords, cursor, (RCS_CS_GPR_REL_BASE + 4) as u32)?;
        push(batch_dwords, cursor, CS_GPR_STAMP_HI)?;
        push(batch_dwords, cursor, (RCS_CS_GPR_REL_BASE + 8) as u32)?;
        push(batch_dwords, cursor, CS_GPR1_STAMP_LO)?;
        push(batch_dwords, cursor, (RCS_CS_GPR_REL_BASE + 12) as u32)?;
        push(batch_dwords, cursor, CS_GPR_STAMP_HI)
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

    const PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER: u32 = 1 << 9;
    const PIPE_CONTROL_UNTYPED_DATAPORT_FLUSH_HEADER: u32 = 1 << 11;
    const PIPE_CONTROL_GPGPU_SELECT_DW1: u32 =
        (1 << 0) | PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL;

    push_pipe_control_full(
        batch_dwords,
        &mut cursor,
        PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER | PIPE_CONTROL_UNTYPED_DATAPORT_FLUSH_HEADER,
        PIPE_CONTROL_GPGPU_SELECT_DW1,
    )?;
    push(batch_dwords, &mut cursor, PIPELINE_SELECT_GPGPU)?;
    push_store_marker(batch_dwords, &mut cursor, 23, 0xC0DE_7901)?;
    push_pipe_control_full(
        batch_dwords,
        &mut cursor,
        PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER | PIPE_CONTROL_UNTYPED_DATAPORT_FLUSH_HEADER,
        PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL,
    )?;

    push(batch_dwords, &mut cursor, STATE_BASE_ADDRESS_CMD)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, 0)?;
    push(batch_dwords, &mut cursor, RENDER_MOCS << 16)?;
    push_sba_address(
        batch_dwords,
        &mut cursor,
        true,
        RENDER_MOCS,
        GPU_VA_DRAW_STATE_BASE,
    )?;
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
    push_store_marker(batch_dwords, &mut cursor, 24, 0xC0DE_7902)?;
    push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_INVALIDATE_BITS)?;

    push(batch_dwords, &mut cursor, STATE_COMPUTE_MODE_CMD)?;
    push(batch_dwords, &mut cursor, 0xFFFF_0000)?;
    push_store_marker(batch_dwords, &mut cursor, 25, 0xC0DE_7903)?;
    push_pipe_control_full(
        batch_dwords,
        &mut cursor,
        PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER | PIPE_CONTROL_UNTYPED_DATAPORT_FLUSH_HEADER,
        PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL,
    )?;

    let cfe_start = cursor;
    push(batch_dwords, &mut cursor, CFE_STATE_CMD)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, (63 << 16) | (1 << 3))?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push_pipe_control_full(
        batch_dwords,
        &mut cursor,
        PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER,
        PIPE_CONTROL_CS_STALL,
    )?;
    push_cs_gpr_stamp(batch_dwords, &mut cursor)?;

    let walker_start = cursor;
    push(batch_dwords, &mut cursor, COMPUTE_WALKER_CMD)?;
    let body_start = cursor;
    for _ in 0..COMPUTE_WALKER_BODY_DWORDS {
        push(batch_dwords, &mut cursor, 0)?;
    }

    let kernel_gpu = GPU_VA_DRAW_STATE_BASE + GPGPU_EU_KERNEL_OFFSET_BYTES as u64;
    batch_dwords[body_start + 4] = 0xFFFF_FFFF;
    batch_dwords[body_start + 6] = 1;
    batch_dwords[body_start + 7] = 1;
    batch_dwords[body_start + 8] = 1;

    let idd = body_start + BODY_INTERFACE_DESCRIPTOR_DWORD;
    batch_dwords[idd] = (kernel_gpu as u32) & 0xFFFF_FFC0;
    batch_dwords[idd + 4] = if program.expects_store && store_surface.ready {
        ((store_surface.binding_table_offset as u32) & 0x001F_FFE0) | 31
    } else {
        0
    };
    batch_dwords[idd + 5] = 1 | (3 << 26);

    let post_sync = body_start + BODY_POSTSYNC_DWORD;
    batch_dwords[post_sync] = RENDER_MOCS << 4;

    push_store_marker(
        batch_dwords,
        &mut cursor,
        RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
    )?;

    push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_FLUSH_BITS)?;
    push(batch_dwords, &mut cursor, MI_BATCH_BUFFER_END)?;
    push(batch_dwords, &mut cursor, MI_NOOP)?;

    let command_bytes = cursor * core::mem::size_of::<u32>();
    crate::log!(
        "intel/gpgpu: compute-walker-layout program_source={} expects_store={} cfe_off=0x{:X} cfe_cmd=0x{:08X} cfe_dw3=0x{:08X} walker_off=0x{:X} walker_cmd=0x{:08X} body0=0x{:08X} exec_mask=0x{:08X} tg_dims={}x{}x{} idd0=0x{:08X} idd4=0x{:08X} idd5=0x{:08X} post_sync0=0x{:08X} surface_base=0x{:X} tail_off=0x{:X} cs_marker=0x{:08X} note=gen125-cfe-compute-walker-embedded-idd-no-post-cfe-mi-store\n",
        program.name,
        program.expects_store as u8,
        cfe_start * core::mem::size_of::<u32>(),
        batch_dwords[cfe_start],
        batch_dwords[cfe_start + 3],
        walker_start * core::mem::size_of::<u32>(),
        batch_dwords[walker_start],
        batch_dwords[body_start],
        batch_dwords[body_start + 4],
        batch_dwords[body_start + 6],
        batch_dwords[body_start + 7],
        batch_dwords[body_start + 8],
        batch_dwords[idd],
        batch_dwords[idd + 4],
        batch_dwords[idd + 5],
        batch_dwords[post_sync],
        GPU_VA_DRAW_STATE_BASE,
        command_bytes,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
    );
    crate::log!(
        "intel/gpgpu: compute-walker-store-contract program_source={} expects_store={} send_bti=0x{:02X} expected_bti=0x{:02X} binding_ready={} bt_off=0x{:X} bt_entry=0x{:08X} surf_off=0x{:X} surf_gpu=0x{:X} target_gpu=0x{:X} surf0=0x{:08X} note=kernel-send-resource-contract\n",
        program.name,
        program.expects_store as u8,
        gpgpu_store_eot_program().words[GPGPU_C_STORE_KERNEL_SEND_DWORD] & 0xFF,
        store_surface.binding_table_index,
        store_surface.ready as u8,
        store_surface.binding_table_offset,
        store_surface.binding_entry,
        store_surface.surface_state_offset,
        store_surface.surface_gpu,
        store_surface.target_gpu,
        store_surface.surface_dword0,
    );
    crate::log!(
        "intel/gpgpu: compute-walker-dwords w0=0x{:08X} w1=0x{:08X} w2=0x{:08X} w3=0x{:08X} w4=0x{:08X} w5=0x{:08X} w6=0x{:08X} w7=0x{:08X} w8=0x{:08X} w9=0x{:08X} w10=0x{:08X} w11=0x{:08X} w12=0x{:08X} w13=0x{:08X} w14=0x{:08X} w15=0x{:08X} w16=0x{:08X} w17=0x{:08X} w18=0x{:08X} idd0=0x{:08X} idd1=0x{:08X} idd2=0x{:08X} idd3=0x{:08X} idd4=0x{:08X} idd5=0x{:08X} idd6=0x{:08X} idd7=0x{:08X}\n",
        batch_dwords[walker_start],
        batch_dwords[walker_start + 1],
        batch_dwords[walker_start + 2],
        batch_dwords[walker_start + 3],
        batch_dwords[walker_start + 4],
        batch_dwords[walker_start + 5],
        batch_dwords[walker_start + 6],
        batch_dwords[walker_start + 7],
        batch_dwords[walker_start + 8],
        batch_dwords[walker_start + 9],
        batch_dwords[walker_start + 10],
        batch_dwords[walker_start + 11],
        batch_dwords[walker_start + 12],
        batch_dwords[walker_start + 13],
        batch_dwords[walker_start + 14],
        batch_dwords[walker_start + 15],
        batch_dwords[walker_start + 16],
        batch_dwords[walker_start + 17],
        batch_dwords[walker_start + 18],
        batch_dwords[idd],
        batch_dwords[idd + 1],
        batch_dwords[idd + 2],
        batch_dwords[idd + 3],
        batch_dwords[idd + 4],
        batch_dwords[idd + 5],
        batch_dwords[idd + 6],
        batch_dwords[idd + 7],
    );

    debug_assert_eq!(cursor - walker_start, COMPUTE_WALKER_DWORDS + 4 + 6 + 2);
    Ok(command_bytes)
}

#[allow(dead_code)]
fn encode_gfx12_gpgpu_walker_probe_batch(
    batch_dwords: &mut [u32],
    store_surface: GpgpuStoreSurfaceState,
    program: GpgpuEuProgram,
) -> Result<usize, &'static str> {
    const MEDIA_VFE_STATE_CMD: u32 = (3 << 29) | (2 << 27) | 7;
    const MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD: u32 = (3 << 29) | (2 << 27) | (2 << 16) | 2;
    const GPGPU_WALKER_CMD: u32 = (3 << 29) | (2 << 27) | (1 << 24) | (5 << 16) | 13;
    const MEDIA_STATE_FLUSH_CMD: u32 = (3 << 29) | (2 << 27) | (4 << 16);
    const PIPELINE_SELECT_BASE: u32 = (3 << 29) | (1 << 27) | (1 << 24) | (4 << 16);
    const PIPELINE_SELECT_GFX12_MASK: u32 = 0x13 << 8;
    const PIPELINE_SELECT_MEDIA_SAMPLER_DOP_CG_ENABLE: u32 = 1 << 4;
    const PIPELINE_SELECT_3D: u32 = PIPELINE_SELECT_BASE
        | PIPELINE_SELECT_GFX12_MASK
        | PIPELINE_SELECT_MEDIA_SAMPLER_DOP_CG_ENABLE;
    const PIPELINE_SELECT_GPGPU: u32 = PIPELINE_SELECT_3D | 2;
    const COMPUTE_SBA_SPAN_BYTES: usize = 0x1000_0000;
    const CS_GPR_STAMP_HI: u32 = 0x0000_0001;
    const CS_GPR0_STAMP_LO: u32 = 0xC5A0_2600;
    const CS_GPR1_STAMP_LO: u32 = 0xC5A0_2608;
    const IDD_OFFSET_BYTES: usize = GPGPU_WALKER_SCRATCH_OFFSET_BYTES;
    const IDD_DWORDS: usize = 8;

    fn push(batch_dwords: &mut [u32], cursor: &mut usize, value: u32) -> Result<(), &'static str> {
        if *cursor >= batch_dwords.len() {
            return Err("compute-walker-batch-exhausted");
        }
        batch_dwords[*cursor] = value;
        *cursor += 1;
        Ok(())
    }

    fn push_pipe_control_full(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        header_flags: u32,
        dw1_flags: u32,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, PIPE_CONTROL_CMD | header_flags)?;
        push(batch_dwords, cursor, dw1_flags)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)
    }

    fn push_pipe_control(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        flags: u32,
    ) -> Result<(), &'static str> {
        push_pipe_control_full(batch_dwords, cursor, 0, flags)
    }

    fn push_store_marker(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        slot: usize,
        value: u32,
    ) -> Result<(), &'static str> {
        let dst = GPU_VA_RESULT_BASE + (slot as u64) * core::mem::size_of::<u32>() as u64;
        push(batch_dwords, cursor, MI_STORE_DATA_IMM_GGTT_DW1)?;
        push(batch_dwords, cursor, dst as u32)?;
        push(batch_dwords, cursor, (dst >> 32) as u32)?;
        push(batch_dwords, cursor, value)
    }

    fn push_cs_gpr_stamp(batch_dwords: &mut [u32], cursor: &mut usize) -> Result<(), &'static str> {
        push(batch_dwords, cursor, mi_lri_cmd(4, MI_LRI_FORCE_POSTED))?;
        push(batch_dwords, cursor, RCS_CS_GPR_REL_BASE as u32)?;
        push(batch_dwords, cursor, CS_GPR0_STAMP_LO)?;
        push(batch_dwords, cursor, (RCS_CS_GPR_REL_BASE + 4) as u32)?;
        push(batch_dwords, cursor, CS_GPR_STAMP_HI)?;
        push(batch_dwords, cursor, (RCS_CS_GPR_REL_BASE + 8) as u32)?;
        push(batch_dwords, cursor, CS_GPR1_STAMP_LO)?;
        push(batch_dwords, cursor, (RCS_CS_GPR_REL_BASE + 12) as u32)?;
        push(batch_dwords, cursor, CS_GPR_STAMP_HI)
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

    const PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER: u32 = 1 << 9;
    const PIPE_CONTROL_UNTYPED_DATAPORT_FLUSH_HEADER: u32 = 1 << 11;
    const PIPE_CONTROL_GPGPU_SELECT_DW1: u32 =
        (1 << 0) | PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL;

    push_pipe_control_full(
        batch_dwords,
        &mut cursor,
        PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER | PIPE_CONTROL_UNTYPED_DATAPORT_FLUSH_HEADER,
        PIPE_CONTROL_GPGPU_SELECT_DW1,
    )?;
    push(batch_dwords, &mut cursor, PIPELINE_SELECT_GPGPU)?;
    push_store_marker(batch_dwords, &mut cursor, 23, 0xC0DE_7801)?;
    push_pipe_control_full(
        batch_dwords,
        &mut cursor,
        PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER,
        PIPE_CONTROL_CS_STALL,
    )?;

    // Wa_1607854226/TGL: non-pipelined state may not latch when emitted under
    // GPGPU pipeline select.  Program SBA/SCM while temporarily in 3D, then
    // switch back before MEDIA_VFE_STATE and GPGPU_WALKER.
    push(batch_dwords, &mut cursor, PIPELINE_SELECT_3D)?;
    push_pipe_control_full(
        batch_dwords,
        &mut cursor,
        PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER | PIPE_CONTROL_UNTYPED_DATAPORT_FLUSH_HEADER,
        PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL,
    )?;
    let idd_index = IDD_OFFSET_BYTES / core::mem::size_of::<u32>();
    if idd_index
        .checked_add(IDD_DWORDS)
        .is_none_or(|end| end > batch_dwords.len())
    {
        return Err("gpgpu-idd-scratch-exhausted");
    }
    let kernel_gpu = GPU_VA_DRAW_STATE_BASE + GPGPU_EU_KERNEL_OFFSET_BYTES as u64;
    batch_dwords[idd_index] = kernel_gpu as u32;
    batch_dwords[idd_index + 1] = (kernel_gpu >> 32) as u32;
    batch_dwords[idd_index + 2] = 0;
    batch_dwords[idd_index + 3] = 0;
    batch_dwords[idd_index + 4] = if program.expects_store && store_surface.ready {
        (store_surface.binding_table_offset as u32) | 31
    } else {
        0
    };
    batch_dwords[idd_index + 5] = 0;
    batch_dwords[idd_index + 6] = 1;
    batch_dwords[idd_index + 7] = 0;
    crate::log!(
        "intel/gpgpu: idd-debug-policy program_source={} idd_dw2=0x{:08X} software_exception_enable={} illegal_opcode_exception_enable={} mask_stack_exception_enable={} sip_programmed=0 note=prm-idd-dw2-loads-eu-cr0-exception-enables-state-sip-not-yet-installed\n",
        program.name,
        batch_dwords[idd_index + 2],
        (batch_dwords[idd_index + 2] >> 7) & 1,
        (batch_dwords[idd_index + 2] >> 13) & 1,
        (batch_dwords[idd_index + 2] >> 11) & 1,
    );

    push(batch_dwords, &mut cursor, STATE_BASE_ADDRESS_CMD)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, 0)?;
    push(batch_dwords, &mut cursor, RENDER_MOCS << 16)?;
    push_sba_address(
        batch_dwords,
        &mut cursor,
        true,
        RENDER_MOCS,
        GPU_VA_DRAW_STATE_BASE,
    )?;
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
    push_store_marker(batch_dwords, &mut cursor, 24, 0xC0DE_7802)?;
    push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_INVALIDATE_BITS)?;
    push_store_marker(batch_dwords, &mut cursor, 25, 0xC0DE_7803)?;
    push_pipe_control_full(
        batch_dwords,
        &mut cursor,
        PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER | PIPE_CONTROL_UNTYPED_DATAPORT_FLUSH_HEADER,
        PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL,
    )?;
    push(batch_dwords, &mut cursor, PIPELINE_SELECT_GPGPU)?;
    push_pipe_control_full(
        batch_dwords,
        &mut cursor,
        PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER,
        PIPE_CONTROL_CS_STALL,
    )?;
    push_cs_gpr_stamp(batch_dwords, &mut cursor)?;
    let vfe_start = cursor;
    push(batch_dwords, &mut cursor, MEDIA_VFE_STATE_CMD)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, (223 << 16) | (2 << 8))?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 2 << 16)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push_pipe_control_full(
        batch_dwords,
        &mut cursor,
        PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER,
        PIPE_CONTROL_CS_STALL,
    )?;
    push_store_marker(batch_dwords, &mut cursor, 26, 0xC0DE_7804)?;
    let id_load_start = cursor;
    push(batch_dwords, &mut cursor, MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, (IDD_DWORDS * core::mem::size_of::<u32>()) as u32)?;
    push(
        batch_dwords,
        &mut cursor,
        (GPU_VA_BATCH_BASE + IDD_OFFSET_BYTES as u64) as u32,
    )?;
    let walker_start = cursor;
    push(batch_dwords, &mut cursor, GPGPU_WALKER_CMD)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 1)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 1)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 1)?;
    push(batch_dwords, &mut cursor, 0xFFFF_FFFF)?;
    push(batch_dwords, &mut cursor, 0xFFFF_FFFF)?;
    push(batch_dwords, &mut cursor, MEDIA_STATE_FLUSH_CMD)?;
    push(batch_dwords, &mut cursor, 0)?;
    push_store_marker(
        batch_dwords,
        &mut cursor,
        RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
    )?;

    push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_FLUSH_BITS)?;
    push(batch_dwords, &mut cursor, MI_BATCH_BUFFER_END)?;
    push(batch_dwords, &mut cursor, MI_NOOP)?;
    let command_bytes = cursor * core::mem::size_of::<u32>();
    let batch_bytes = command_bytes.max(IDD_OFFSET_BYTES + IDD_DWORDS * core::mem::size_of::<u32>());

    crate::log!(
        "intel/gpgpu: compute-walker-layout program_source={} expects_store={} vfe_off=0x{:X} vfe_dw3=0x{:08X} vfe_dw5=0x{:08X} id_load_off=0x{:X} walker_off=0x{:X} walker_cmd=0x{:08X} exec_mask=0x{:08X} idd_gpu=0x{:X} idd_dw2=0x{:08X} idd_dw4=0x{:08X} idd_dw6=0x{:08X} surface_base=0x{:X} tail_off=0x{:X} cs_marker=0x{:08X} note=gen12-media-vfe-midl-gpgpu-walker\n",
        program.name,
        program.expects_store as u8,
        vfe_start * core::mem::size_of::<u32>(),
        batch_dwords[vfe_start + 3],
        batch_dwords[vfe_start + 5],
        id_load_start * core::mem::size_of::<u32>(),
        walker_start * core::mem::size_of::<u32>(),
        batch_dwords[walker_start],
        batch_dwords[walker_start + 13],
        GPU_VA_BATCH_BASE + IDD_OFFSET_BYTES as u64,
        batch_dwords[idd_index + 2],
        batch_dwords[idd_index + 4],
        batch_dwords[idd_index + 6],
        GPU_VA_DRAW_STATE_BASE,
        command_bytes,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
    );
    crate::log!(
        "intel/gpgpu: compute-walker-store-contract program_source={} expects_store={} send_bti=0x{:02X} expected_bti=0x{:02X} binding_ready={} bt_off=0x{:X} bt_entry=0x{:08X} surf_off=0x{:X} surf_gpu=0x{:X} target_gpu=0x{:X} surf0=0x{:08X} note=kernel-send-resource-contract\n",
        program.name,
        program.expects_store as u8,
        gpgpu_store_eot_program().words[GPGPU_C_STORE_KERNEL_SEND_DWORD] & 0xFF,
        store_surface.binding_table_index,
        store_surface.ready as u8,
        store_surface.binding_table_offset,
        store_surface.binding_entry,
        store_surface.surface_state_offset,
        store_surface.surface_gpu,
        store_surface.target_gpu,
        store_surface.surface_dword0,
    );
    crate::log!(
        "intel/gpgpu: compute-walker-dwords w0=0x{:08X} w1=0x{:08X} w2=0x{:08X} w3=0x{:08X} w4=0x{:08X} w5=0x{:08X} w6=0x{:08X} w7=0x{:08X} w8=0x{:08X} w9=0x{:08X} w10=0x{:08X} w11=0x{:08X} w12=0x{:08X} w13=0x{:08X} w14=0x{:08X} idd0=0x{:08X} idd1=0x{:08X} idd2=0x{:08X} idd3=0x{:08X} idd4=0x{:08X} idd5=0x{:08X} idd6=0x{:08X} idd7=0x{:08X} midl0=0x{:08X} midl2=0x{:08X} midl3=0x{:08X}\n",
        batch_dwords[walker_start],
        batch_dwords[walker_start + 1],
        batch_dwords[walker_start + 2],
        batch_dwords[walker_start + 3],
        batch_dwords[walker_start + 4],
        batch_dwords[walker_start + 5],
        batch_dwords[walker_start + 6],
        batch_dwords[walker_start + 7],
        batch_dwords[walker_start + 8],
        batch_dwords[walker_start + 9],
        batch_dwords[walker_start + 10],
        batch_dwords[walker_start + 11],
        batch_dwords[walker_start + 12],
        batch_dwords[walker_start + 13],
        batch_dwords[walker_start + 14],
        batch_dwords[idd_index],
        batch_dwords[idd_index + 1],
        batch_dwords[idd_index + 2],
        batch_dwords[idd_index + 3],
        batch_dwords[idd_index + 4],
        batch_dwords[idd_index + 5],
        batch_dwords[idd_index + 6],
        batch_dwords[idd_index + 7],
        batch_dwords[id_load_start],
        batch_dwords[id_load_start + 2],
        batch_dwords[id_load_start + 3],
    );

    Ok(batch_bytes)
}

fn log_gpgpu_compute_walker_status(proof: GpgpuComputeWalkerProof) {
    let gpu_program_started = proof.dispatch_delta != 0;
    let eot_only_retired = !proof.expects_store && gpu_program_started && proof.retired;
    let failure_class = if eot_only_retired {
        "thread-eot-retired-proven"
    } else if proof.result_c_changed_by_eu {
        "shared-ram-write-proven"
    } else if gpu_program_started && !proof.retired && proof.c_value == 0 {
        "program-started-did-not-finish-or-store"
    } else if gpu_program_started && proof.retired && proof.c_value == 0 {
        "program-started-no-shared-ram-write"
    } else if !gpu_program_started {
        "program-not-started"
    } else {
        "unexpected-shared-ram-value"
    };
    crate::log!(
        "intel/gpgpu: gpu-program-proof program_source={} expects_store={} start_submitted={} finished={} finish_marker=0x{:08X} finish_expected=0x{:08X} starts_before={} starts_after={} starts_delta={} start_command_bytes=0x{:X} gpu_program_started={} shared_ram_slot={} shared_ram_value=0x{:08X} shared_ram_expected=0x{:08X} wrote_shared_ram={} eot_retired={} failure_class={} cpu_reads_c_back=1 backend=gfx12-gpgpu-start-command next=fix-eot-then-dataport-store-then-scale-tiles does_not_prove=matmul\n",
        proof.program_name,
        proof.expects_store as u8,
        proof.submitted as u8,
        proof.retired as u8,
        proof.marker,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        proof.dispatch_before,
        proof.dispatch_after,
        proof.dispatch_delta,
        proof.batch_bytes,
        gpu_program_started as u8,
        RESULT_SLOT_GPGPU_EU_C_STORE_DWORD,
        proof.c_value,
        GPU_PROGRAM_SHARED_RAM_WRITE_EXPECTED,
        proof.result_c_changed_by_eu as u8,
        eot_only_retired as u8,
        failure_class,
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
