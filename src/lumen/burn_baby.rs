extern crate alloc;

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering};

use crate::turbo::avx2_fma_sse2_help::bf16_to_f32;
use spin::Mutex;

const TARGET_CHUNKS_PER_WORKER: usize = 4;
const MIN_CHUNK_ROWS: usize = 16;
const MAX_CHUNK_ROWS: usize = 256;
const MAX_COMPUTE_POLL_SLOTS: usize = 256;

static FALLBACK_QUEUE: Mutex<VecDeque<ComputeJob>> = Mutex::new(VecDeque::new());
static LOGGED_QUEUE_LANE: AtomicBool = AtomicBool::new(false);
static LOGGED_POLL_LANE: AtomicBool = AtomicBool::new(false);
static LOGGED_SERVICE_PROTECTED_LANE: AtomicBool = AtomicBool::new(false);
static LOGGED_BF16_SLOT_SPREAD: AtomicBool = AtomicBool::new(false);
static LOGGED_BF16_ARGMAX_BRIDGE: AtomicBool = AtomicBool::new(false);
static LOGGED_BF16_DUAL_SILU_BRIDGE: AtomicBool = AtomicBool::new(false);
static LOGGED_BF16_CGP_PROMPT_PATH: AtomicBool = AtomicBool::new(false);
static LUMEN_PROMPT_BF16_DEPTH: AtomicUsize = AtomicUsize::new(0);
static SERVICE_PROTECTED_SLOTS: AtomicU64 = AtomicU64::new(0);
#[cfg(target_arch = "x86_64")]
static LOGGED_BF16_SIMD_PROBE: AtomicBool = AtomicBool::new(false);
#[cfg(target_arch = "x86_64")]
static LOGGED_BF16_DISPATCH_PLAN: AtomicBool = AtomicBool::new(false);
#[cfg(target_arch = "x86_64")]
static LOGGED_BF16_AVX2_LANE: AtomicBool = AtomicBool::new(false);
#[cfg(target_arch = "x86_64")]
static LOGGED_BF16_SSE2_LANE: AtomicBool = AtomicBool::new(false);
#[cfg(target_arch = "x86_64")]
static BF16_SIMD_LANE: AtomicU8 = AtomicU8::new(BF16_SIMD_LANE_UNKNOWN);
static SUBMITTED_JOBS: AtomicU64 = AtomicU64::new(0);
static COMPLETED_JOBS: AtomicU64 = AtomicU64::new(0);
static POLLED_JOBS: AtomicU64 = AtomicU64::new(0);
static POLLED_JOBS_BY_SLOT: [AtomicU64; MAX_COMPUTE_POLL_SLOTS] =
    [const { AtomicU64::new(0) }; MAX_COMPUTE_POLL_SLOTS];

#[cfg(target_arch = "x86_64")]
const BF16_SIMD_LANE_UNKNOWN: u8 = 0;
#[cfg(target_arch = "x86_64")]
const BF16_SIMD_LANE_AVX2_FMA: u8 = 1;
#[cfg(target_arch = "x86_64")]
const BF16_SIMD_LANE_SSE2: u8 = 2;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ComputeError {
    BadShape,
    EmptyChunk,
}

pub(crate) struct LumenPromptBf16Context {
    active: bool,
}

pub(crate) fn enter_lumen_prompt_bf16_context() -> LumenPromptBf16Context {
    LUMEN_PROMPT_BF16_DEPTH.fetch_add(1, Ordering::AcqRel);
    LumenPromptBf16Context { active: true }
}

impl Drop for LumenPromptBf16Context {
    fn drop(&mut self) {
        if self.active {
            LUMEN_PROMPT_BF16_DEPTH.fetch_sub(1, Ordering::AcqRel);
            self.active = false;
        }
    }
}

fn bf16_matvec_source_label() -> &'static str {
    if LUMEN_PROMPT_BF16_DEPTH.load(Ordering::Acquire) != 0 {
        "lumen-prompt"
    } else {
        "runtime"
    }
}

#[derive(Copy, Clone)]
struct MatvecRowsF32 {
    x: usize,
    w_rowmajor: usize,
    out: usize,
    n_rows: usize,
    k_dim: usize,
    row_start: usize,
    row_end: usize,
    done: usize,
}

#[derive(Copy, Clone)]
struct MatvecRowsBf16 {
    x: usize,
    w_rowmajor_bf16: usize,
    out: usize,
    n_rows: usize,
    k_dim: usize,
    row_start: usize,
    row_end: usize,
    done: usize,
}

#[derive(Copy, Clone)]
struct MatvecRowsBf16Suffix {
    x: usize,
    w_rowmajor_bf16: usize,
    out: usize,
    n_rows: usize,
    k_dim: usize,
    live_k_dim: usize,
    prefix_rows: usize,
    prefix_count: usize,
    prefix_index_start: usize,
    prefix_index_end: usize,
    done: usize,
}

#[derive(Copy, Clone)]
enum ComputeJob {
    MatvecRowsF32(MatvecRowsF32),
    MatvecRowsBf16(MatvecRowsBf16),
    MatvecRowsBf16Suffix(MatvecRowsBf16Suffix),
}

#[derive(Copy, Clone, Debug, Default)]
struct CgpPrefixSuffixSubmit {
    accepted_rows: usize,
    submitted_jobs: usize,
    first_row: usize,
    last_row: usize,
}

#[derive(Copy, Clone, Debug, Default)]
pub struct ComputeStats {
    pub submitted_jobs: u64,
    pub completed_jobs: u64,
    pub polled_jobs: u64,
    pub queued_jobs: usize,
}

#[embassy_executor::task(pool_size = 128)]
async fn compute_job_task(job: ComputeJob) {
    crate::t::kernel_task_domain::with(
        crate::t::kernel_task_domain::KernelTaskDomain::ComputeWorker,
        None,
        || execute_job(job),
    );
}

pub fn stats() -> ComputeStats {
    ComputeStats {
        submitted_jobs: SUBMITTED_JOBS.load(Ordering::Acquire),
        completed_jobs: COMPLETED_JOBS.load(Ordering::Acquire),
        polled_jobs: POLLED_JOBS.load(Ordering::Acquire),
        queued_jobs: FALLBACK_QUEUE.lock().len(),
    }
}

pub fn poll_counts_for_slots(slots: &[u32]) -> Vec<(u32, u64)> {
    slots
        .iter()
        .copied()
        .filter_map(|slot| {
            let idx = slot as usize;
            POLLED_JOBS_BY_SLOT
                .get(idx)
                .map(|counter| (slot, counter.load(Ordering::Acquire)))
        })
        .collect()
}

pub fn online_worker_count() -> usize {
    online_compute_worker_slots().len().max(1)
}

pub fn recommended_chunk_rows(n_rows: usize) -> usize {
    if n_rows == 0 {
        return 0;
    }

    let target_chunks = online_worker_count()
        .saturating_mul(TARGET_CHUNKS_PER_WORKER)
        .max(1);
    let chunk_rows = n_rows.div_ceil(target_chunks);
    chunk_rows
        .clamp(MIN_CHUNK_ROWS.min(n_rows), MAX_CHUNK_ROWS.min(n_rows))
        .max(1)
}

pub fn poll_compute_lane() -> bool {
    let slot = crate::percpu::current_slot() as u32;
    if !crate::workers::is_background_worker_slot(slot) {
        return false;
    }
    if should_skip_compute_slot(slot) {
        return false;
    }

    let job = FALLBACK_QUEUE.lock().pop_front();
    let Some(job) = job else {
        return false;
    };

    if !LOGGED_POLL_LANE.swap(true, Ordering::AcqRel) {
        crate::log!("burn-baby: AP poll compute lane active slot={}\n", slot);
    }

    if let Some(counter) = POLLED_JOBS_BY_SLOT.get(slot as usize) {
        counter.fetch_add(1, Ordering::AcqRel);
    }
    POLLED_JOBS.fetch_add(1, Ordering::AcqRel);
    crate::t::kernel_task_domain::with(
        crate::t::kernel_task_domain::KernelTaskDomain::ComputeWorker,
        None,
        || execute_job(job),
    );
    true
}

pub fn protect_service_compute_slot(cpu_slot: u32, purpose: &'static str) {
    if !crate::workers::is_background_worker_slot(cpu_slot) || cpu_slot >= 64 {
        return;
    }
    let bit = 1u64 << cpu_slot;
    let previous = SERVICE_PROTECTED_SLOTS.fetch_or(bit, Ordering::AcqRel);
    if previous & bit == 0 {
        crate::log!(
            "burn-baby: service-protected compute slot={} purpose={} action=skip-ap-poll-chunks\n",
            cpu_slot,
            purpose
        );
    }
}

pub fn matvec_rowmajor_f32(
    x: &[f32],
    w_rowmajor: &[f32],
    n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
    chunk_rows: usize,
) -> Result<(), ComputeError> {
    if n_rows == 0 || k_dim == 0 {
        return Ok(());
    }
    let Some(w_len) = n_rows.checked_mul(k_dim) else {
        return Err(ComputeError::BadShape);
    };
    if x.len() < k_dim || w_rowmajor.len() < w_len || out.len() < n_rows {
        return Err(ComputeError::BadShape);
    }

    let chunk_rows = if chunk_rows == 0 {
        recommended_chunk_rows(n_rows)
    } else {
        chunk_rows
    };
    if chunk_rows == 0 {
        return Err(ComputeError::EmptyChunk);
    }

    let chunks = n_rows.div_ceil(chunk_rows);
    if chunks <= 1 || !crate::workers::has_background_worker_slot() {
        matvec_rows_f32(x, w_rowmajor, k_dim, out, 0, n_rows);
        return Ok(());
    }

    let done = AtomicUsize::new(0);
    let done_ptr = &done as *const AtomicUsize as usize;
    let x_ptr = x.as_ptr() as usize;
    let w_ptr = w_rowmajor.as_ptr() as usize;
    let out_ptr = out.as_mut_ptr() as usize;

    let mut submitted = 0usize;
    let mut row_start = 0usize;
    while row_start < n_rows {
        let row_end = row_start.saturating_add(chunk_rows).min(n_rows);
        let job = ComputeJob::MatvecRowsF32(MatvecRowsF32 {
            x: x_ptr,
            w_rowmajor: w_ptr,
            out: out_ptr,
            n_rows,
            k_dim,
            row_start,
            row_end,
            done: done_ptr,
        });
        submit_job(job);
        submitted += 1;
        row_start = row_end;
    }

    let wait_start = embassy_time_driver::now();
    let mut last_wait_log = wait_start;
    while done.load(Ordering::Acquire) != submitted {
        crate::time::poll();
        crate::smp::poll();
        log_compute_wait_progress(&done, submitted, &mut last_wait_log, "f32");
        if !poll_compute_lane() {
            core::hint::spin_loop();
        }
    }

    Ok(())
}

pub fn matvec_rowmajor_bf16(
    x: &[f32],
    w_rowmajor_bf16: &[u8],
    n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
    chunk_rows: usize,
) -> Result<(), ComputeError> {
    if n_rows == 0 || k_dim == 0 {
        return Ok(());
    }
    let Some(w_len) = n_rows
        .checked_mul(k_dim)
        .and_then(|values| values.checked_mul(2))
    else {
        return Err(ComputeError::BadShape);
    };
    if x.len() < k_dim || w_rowmajor_bf16.len() < w_len || out.len() < n_rows {
        return Err(ComputeError::BadShape);
    }

    let chunk_rows = if chunk_rows == 0 {
        recommended_chunk_rows(n_rows)
    } else {
        chunk_rows
    };
    if chunk_rows == 0 {
        return Err(ComputeError::EmptyChunk);
    }

    let chunks = n_rows.div_ceil(chunk_rows);
    log_bf16_dispatch_plan(n_rows, k_dim, chunk_rows, chunks);
    if chunks <= 1 || !crate::workers::has_background_worker_slot() {
        matvec_rows_bf16(x, w_rowmajor_bf16, k_dim, out, 0, n_rows);
        return Ok(());
    }
    let source_label = bf16_matvec_source_label();
    if source_label == "lumen-prompt" && !LOGGED_BF16_CGP_PROMPT_PATH.swap(true, Ordering::AcqRel) {
        let backend = crate::lumen::cgp::gpu_burn_baby_backend();
        crate::log!(
            "burn-baby: cgp prompt path source={} backend={} label={} role={} output_owner={} contract={} dispatch_contract={} action=guarded-proof-before-prefix-ownership\n",
            source_label,
            backend.name,
            backend.label,
            backend.role.as_str(),
            backend.output_owner,
            backend.correctness_contract,
            backend.dispatch_contract,
        );
    }
    let local_gpu_proof = crate::lumen::gpu_shadow::observe_bf16_matvec_call(
        source_label,
        n_rows,
        k_dim,
        chunk_rows,
        chunks,
    );
    crate::lumen::burn_baba::share_matvec_rowmajor_bf16(n_rows, k_dim, chunk_rows);
    let cgp_prefix = crate::lumen::gpu_shadow::observe_live_bf16_matvec_probe(
        x,
        w_rowmajor_bf16,
        n_rows,
        k_dim,
        chunk_rows,
        chunks,
        local_gpu_proof,
    );
    let remote = crate::lumen::lumen_net::enqueue_remote_bf16_matvec_suffix(
        x,
        w_rowmajor_bf16,
        n_rows,
        k_dim,
        out,
        chunk_rows,
    );
    let local_row_end = remote.map(|ticket| ticket.row_start).unwrap_or(n_rows);
    if let Some(ticket) = remote {
        crate::log!(
            "burn-baby: bf16 matvec split local_rows=0..{} remote_rows={}..{} remote_job={} remote_pending={} completion=tcp-result\n",
            local_row_end,
            ticket.row_start,
            ticket.row_end,
            ticket.job_id,
            crate::lumen::lumen_net::pending_remote_bf16_matvecs()
        );
    }

    let done = AtomicUsize::new(0);
    let done_ptr = &done as *const AtomicUsize as usize;
    let x_ptr = x.as_ptr() as usize;
    let w_ptr = w_rowmajor_bf16.as_ptr() as usize;
    let out_ptr = out.as_mut_ptr() as usize;
    let spread_slots = if !LOGGED_BF16_SLOT_SPREAD.load(Ordering::Acquire) {
        online_compute_worker_slots()
    } else {
        Vec::new()
    };
    let spread_counts_before = if spread_slots.is_empty() {
        Vec::new()
    } else {
        poll_counts_for_slots(&spread_slots)
    };

    let cgp_prefix_submit = submit_cgp_bf16_prefix_suffix_jobs(
        source_label,
        x_ptr,
        w_ptr,
        out_ptr,
        n_rows,
        k_dim,
        local_row_end,
        done_ptr,
        &cgp_prefix,
    );
    let submitted = cgp_prefix_submit.submitted_jobs
        + submit_bf16_jobs_skipping_cgp_prefix(
        x_ptr,
        w_ptr,
        out_ptr,
        n_rows,
        k_dim,
        chunk_rows,
        local_row_end,
        done_ptr,
        &cgp_prefix,
    );

    let wait_start = embassy_time_driver::now();
    let mut last_wait_log = wait_start;
    while done.load(Ordering::Acquire) != submitted {
        crate::time::poll();
        crate::smp::poll();
        log_compute_wait_progress(&done, submitted, &mut last_wait_log, "bf16");
        if !poll_compute_lane() {
            core::hint::spin_loop();
        }
    }
    if !spread_counts_before.is_empty() && !LOGGED_BF16_SLOT_SPREAD.swap(true, Ordering::AcqRel) {
        let spread_counts_after = poll_counts_for_slots(&spread_slots);
        let spread = slot_poll_deltas(&spread_counts_before, &spread_counts_after);
        crate::log!(
            "burn-baby: bf16 slot-spread rows={} k_dim={} chunk_rows={} submitted={} slots={:?} deltas={:?} proof=ap-queue-distribution\n",
            local_row_end,
            k_dim,
            chunk_rows,
            submitted,
            spread_slots,
            spread
        );
    }
    if cgp_prefix_submit.accepted_rows != 0 {
        let first_output_bits = out
            .get(cgp_prefix_submit.first_row)
            .copied()
            .unwrap_or(0.0)
            .to_bits();
        let last_output_bits = out
            .get(cgp_prefix_submit.last_row)
            .copied()
            .unwrap_or(0.0)
            .to_bits();
        crate::log!(
            "burn-baby: cgp accepted-prefix complete source={} rows={} first_row={} last_row={} live_k_dim={} k_dim={} suffix_jobs={} total_cpu_jobs={} first_output_bits=0x{:08X} last_output_bits=0x{:08X} output_owner=hybrid-cgp-prefix-cpu-ap-suffix action=cpu-ap-skipped-prefix-owned-rows\n",
            source_label,
            cgp_prefix_submit.accepted_rows,
            cgp_prefix_submit.first_row,
            cgp_prefix_submit.last_row,
            cgp_prefix.live_k_dim,
            k_dim,
            cgp_prefix_submit.submitted_jobs,
            submitted,
            first_output_bits,
            last_output_bits,
        );
    }

    if let Some(ticket) = remote
        && !crate::lumen::lumen_net::wait_remote_bf16_matvec(ticket)
    {
        let _ = crate::lumen::lumen_net::cancel_remote_bf16_matvec(ticket.job_id);
        crate::log!(
            "burn-baby: bf16 remote result timeout job={} rows={}..{} action=local-suffix-fallback\n",
            ticket.job_id,
            ticket.row_start,
            ticket.row_end
        );
        matvec_rows_bf16(x, w_rowmajor_bf16, k_dim, out, ticket.row_start, ticket.row_end);
    }

    Ok(())
}

fn submit_bf16_range_jobs(
    x_ptr: usize,
    w_ptr: usize,
    out_ptr: usize,
    n_rows: usize,
    k_dim: usize,
    chunk_rows: usize,
    row_start: usize,
    row_end: usize,
    done_ptr: usize,
) -> usize {
    let mut submitted = 0usize;
    let mut cursor = row_start;
    while cursor < row_end {
        let end = cursor.saturating_add(chunk_rows).min(row_end);
        submit_job(ComputeJob::MatvecRowsBf16(MatvecRowsBf16 {
            x: x_ptr,
            w_rowmajor_bf16: w_ptr,
            out: out_ptr,
            n_rows,
            k_dim,
            row_start: cursor,
            row_end: end,
            done: done_ptr,
        }));
        submitted += 1;
        cursor = end;
    }
    submitted
}

fn submit_cgp_bf16_prefix_suffix_jobs(
    source_label: &'static str,
    x_ptr: usize,
    w_ptr: usize,
    out_ptr: usize,
    n_rows: usize,
    k_dim: usize,
    local_row_end: usize,
    done_ptr: usize,
    cgp_prefix: &crate::lumen::cgp::CgpBf16PrefixContribution,
) -> CgpPrefixSuffixSubmit {
    if cgp_prefix.is_empty() || cgp_prefix.live_k_dim == 0 || cgp_prefix.live_k_dim > k_dim {
        return CgpPrefixSuffixSubmit::default();
    }

    let mut accepted_indices = 0usize;
    let mut first_row = usize::MAX;
    let mut last_row = 0usize;
    for contribution in cgp_prefix.rows.iter() {
        let row = contribution.row;
        if row >= local_row_end || row >= n_rows {
            continue;
        }
        let base = row.saturating_mul(k_dim).saturating_mul(2);
        let row_end = base.saturating_add(k_dim.saturating_mul(2));
        let w_len = n_rows.saturating_mul(k_dim).saturating_mul(2);
        if row_end > w_len {
            continue;
        }

        if accepted_indices == 0 {
            first_row = row;
        }
        last_row = row;
        accepted_indices += 1;
    }

    if accepted_indices == 0 {
        return CgpPrefixSuffixSubmit::default();
    }

    let chunk_rows = recommended_chunk_rows(accepted_indices).max(1);
    let mut submitted_jobs = 0usize;
    let mut prefix_index_start = 0usize;
    while prefix_index_start < cgp_prefix.rows.len() {
        let prefix_index_end = prefix_index_start
            .saturating_add(chunk_rows)
            .min(cgp_prefix.rows.len());
        submit_job(ComputeJob::MatvecRowsBf16Suffix(MatvecRowsBf16Suffix {
            x: x_ptr,
            w_rowmajor_bf16: w_ptr,
            out: out_ptr,
            n_rows,
            k_dim,
            live_k_dim: cgp_prefix.live_k_dim,
            prefix_rows: cgp_prefix.rows.as_ptr() as usize,
            prefix_count: cgp_prefix.rows.len(),
            prefix_index_start,
            prefix_index_end,
            done: done_ptr,
        }));
        submitted_jobs += 1;
        prefix_index_start = prefix_index_end;
    }

    crate::log!(
        "burn-baby: cgp accepted-prefix suffix-submit source={} mode={} rows={} first_row={} last_row={} live_k_dim={} k_dim={} suffix_jobs={} output_owner=hybrid-cgp-prefix-cpu-ap-suffix contract=scalar-bf16-reference action=parallel-cpu-suffix-skip-ap-full-row-recompute\n",
        source_label,
        cgp_prefix.mode.as_str(),
        accepted_indices,
        first_row,
        last_row,
        cgp_prefix.live_k_dim,
        k_dim,
        submitted_jobs,
    );

    CgpPrefixSuffixSubmit {
        accepted_rows: accepted_indices,
        submitted_jobs,
        first_row,
        last_row,
    }
}

fn submit_bf16_jobs_skipping_cgp_prefix(
    x_ptr: usize,
    w_ptr: usize,
    out_ptr: usize,
    n_rows: usize,
    k_dim: usize,
    chunk_rows: usize,
    local_row_end: usize,
    done_ptr: usize,
    cgp_prefix: &crate::lumen::cgp::CgpBf16PrefixContribution,
) -> usize {
    if cgp_prefix.is_empty() {
        return submit_bf16_range_jobs(
            x_ptr,
            w_ptr,
            out_ptr,
            n_rows,
            k_dim,
            chunk_rows,
            0,
            local_row_end,
            done_ptr,
        );
    }

    let mut submitted = 0usize;
    let mut cursor = 0usize;
    let mut row_index = 0usize;
    while cursor < local_row_end {
        while row_index < cgp_prefix.rows.len() && cgp_prefix.rows[row_index].row < cursor {
            row_index += 1;
        }
        let next_cgp_row = cgp_prefix
            .rows
            .get(row_index)
            .map(|row| row.row.min(local_row_end))
            .unwrap_or(local_row_end);
        if cursor < next_cgp_row {
            submitted += submit_bf16_range_jobs(
                x_ptr,
                w_ptr,
                out_ptr,
                n_rows,
                k_dim,
                chunk_rows,
                cursor,
                next_cgp_row,
                done_ptr,
            );
            cursor = next_cgp_row;
        }
        if row_index >= cgp_prefix.rows.len() || cursor >= local_row_end {
            break;
        }

        let mut skip_end = cgp_prefix.rows[row_index].row;
        while row_index < cgp_prefix.rows.len()
            && cgp_prefix.rows[row_index].row == skip_end
            && skip_end < local_row_end
        {
            skip_end += 1;
            row_index += 1;
        }
        cursor = skip_end.max(cursor.saturating_add(1)).min(local_row_end);
    }
    submitted
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lumen_trueos_matvec_rowmajor_f32_bf16(
    x: *const f32,
    x_len: usize,
    w_rowmajor_bf16: *const u8,
    w_len: usize,
    n_rows: usize,
    k_dim: usize,
    out: *mut f32,
    out_len: usize,
) -> i32 {
    if x.is_null() || w_rowmajor_bf16.is_null() || out.is_null() {
        return -1;
    }

    let Some(expected_w_len) = n_rows
        .checked_mul(k_dim)
        .and_then(|values| values.checked_mul(2))
    else {
        return -1;
    };
    if x_len < k_dim || w_len < expected_w_len || out_len < n_rows {
        return -1;
    }

    let x = unsafe { core::slice::from_raw_parts(x, x_len) };
    let w = unsafe { core::slice::from_raw_parts(w_rowmajor_bf16, w_len) };
    let out = unsafe { core::slice::from_raw_parts_mut(out, out_len) };

    match matvec_rowmajor_bf16(x, w, n_rows, k_dim, out, 0) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lumen_trueos_matvec_argmax_rowmajor_f32_bf16(
    x: *const f32,
    x_len: usize,
    w_rowmajor_bf16: *const u8,
    w_len: usize,
    n_rows: usize,
    k_dim: usize,
) -> isize {
    if x.is_null() || w_rowmajor_bf16.is_null() || n_rows == 0 {
        return -1;
    }

    let Some(expected_w_len) = expected_bf16_bytes(n_rows, k_dim) else {
        return -1;
    };
    if x_len < k_dim || w_len < expected_w_len {
        return -1;
    }

    let x = unsafe { core::slice::from_raw_parts(x, x_len) };
    let w = unsafe { core::slice::from_raw_parts(w_rowmajor_bf16, w_len) };
    let mut scores = Vec::new();
    scores.resize(n_rows, 0.0f32);

    if matvec_rowmajor_bf16_local_ap(x, w, n_rows, k_dim, &mut scores, 0).is_err() {
        return -1;
    }
    if !LOGGED_BF16_ARGMAX_BRIDGE.swap(true, Ordering::AcqRel) {
        crate::log!(
            "burn-baby: bf16 bridge argmax local-ap rows={} k_dim={} chunk_rows={} proof=lumen-trueos-extern\n",
            n_rows,
            k_dim,
            recommended_chunk_rows(n_rows)
        );
    }

    let mut best_index = 0usize;
    let mut best_score = f32::NEG_INFINITY;
    for (index, score) in scores.iter().copied().enumerate() {
        if score > best_score {
            best_index = index;
            best_score = score;
        }
    }
    best_index as isize
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lumen_trueos_dual_matvec_silu_mul_rowmajor_f32_bf16(
    x: *const f32,
    x_len: usize,
    gate_w_rowmajor_bf16: *const u8,
    gate_w_len: usize,
    up_w_rowmajor_bf16: *const u8,
    up_w_len: usize,
    n_rows: usize,
    k_dim: usize,
    out: *mut f32,
    out_len: usize,
) -> i32 {
    if x.is_null()
        || gate_w_rowmajor_bf16.is_null()
        || up_w_rowmajor_bf16.is_null()
        || out.is_null()
    {
        return -1;
    }

    let Some(expected_w_len) = expected_bf16_bytes(n_rows, k_dim) else {
        return -1;
    };
    if x_len < k_dim || gate_w_len < expected_w_len || up_w_len < expected_w_len || out_len < n_rows
    {
        return -1;
    }

    if n_rows == 0 || k_dim == 0 {
        return 0;
    }

    let x = unsafe { core::slice::from_raw_parts(x, x_len) };
    let gate_w = unsafe { core::slice::from_raw_parts(gate_w_rowmajor_bf16, gate_w_len) };
    let up_w = unsafe { core::slice::from_raw_parts(up_w_rowmajor_bf16, up_w_len) };
    let out = unsafe { core::slice::from_raw_parts_mut(out, out_len) };
    let mut gate = Vec::new();
    let mut up = Vec::new();
    gate.resize(n_rows, 0.0f32);
    up.resize(n_rows, 0.0f32);

    if matvec_rowmajor_bf16_local_ap(x, gate_w, n_rows, k_dim, &mut gate, 0).is_err() {
        return -1;
    }
    if matvec_rowmajor_bf16_local_ap(x, up_w, n_rows, k_dim, &mut up, 0).is_err() {
        return -1;
    }
    if !LOGGED_BF16_DUAL_SILU_BRIDGE.swap(true, Ordering::AcqRel) {
        crate::log!(
            "burn-baby: bf16 bridge dual-silu local-ap rows={} k_dim={} chunk_rows={} proof=lumen-trueos-extern\n",
            n_rows,
            k_dim,
            recommended_chunk_rows(n_rows)
        );
    }

    for row in 0..n_rows {
        let g = gate[row];
        let sig = 1.0 / (1.0 + libm::expf(-g));
        out[row] = (g * sig) * up[row];
    }

    0
}

fn expected_bf16_bytes(n_rows: usize, k_dim: usize) -> Option<usize> {
    n_rows
        .checked_mul(k_dim)
        .and_then(|values| values.checked_mul(2))
}

fn matvec_rowmajor_bf16_local_ap(
    x: &[f32],
    w_rowmajor_bf16: &[u8],
    n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
    chunk_rows: usize,
) -> Result<(), ComputeError> {
    if n_rows == 0 || k_dim == 0 {
        return Ok(());
    }
    let Some(w_len) = expected_bf16_bytes(n_rows, k_dim) else {
        return Err(ComputeError::BadShape);
    };
    if x.len() < k_dim || w_rowmajor_bf16.len() < w_len || out.len() < n_rows {
        return Err(ComputeError::BadShape);
    }

    let chunk_rows = if chunk_rows == 0 {
        recommended_chunk_rows(n_rows)
    } else {
        chunk_rows
    };
    if chunk_rows == 0 {
        return Err(ComputeError::EmptyChunk);
    }

    let chunks = n_rows.div_ceil(chunk_rows);
    if chunks <= 1 || !crate::workers::has_background_worker_slot() {
        matvec_rows_bf16(x, w_rowmajor_bf16, k_dim, out, 0, n_rows);
        return Ok(());
    }

    let done = AtomicUsize::new(0);
    let done_ptr = &done as *const AtomicUsize as usize;
    let x_ptr = x.as_ptr() as usize;
    let w_ptr = w_rowmajor_bf16.as_ptr() as usize;
    let out_ptr = out.as_mut_ptr() as usize;

    let mut submitted = 0usize;
    let mut row_start = 0usize;
    while row_start < n_rows {
        let row_end = row_start.saturating_add(chunk_rows).min(n_rows);
        submit_job(ComputeJob::MatvecRowsBf16(MatvecRowsBf16 {
            x: x_ptr,
            w_rowmajor_bf16: w_ptr,
            out: out_ptr,
            n_rows,
            k_dim,
            row_start,
            row_end,
            done: done_ptr,
        }));
        submitted += 1;
        row_start = row_end;
    }

    while done.load(Ordering::Acquire) != submitted {
        crate::time::poll();
        crate::smp::poll();
        if !poll_compute_lane() {
            core::hint::spin_loop();
        }
    }

    Ok(())
}

fn log_compute_wait_progress(
    done: &AtomicUsize,
    submitted: usize,
    last_wait_log: &mut u64,
    dtype: &'static str,
) {
    let now = embassy_time_driver::now();
    let hz = embassy_time_driver::TICK_HZ;
    if hz == 0 || now.saturating_sub(*last_wait_log) < hz {
        return;
    }
    *last_wait_log = now;
    let stats = stats();
    crate::log!(
        "burn-baby: wait dtype={} done={}/{} submitted={} completed={} polled={} queued={}\n",
        dtype,
        done.load(Ordering::Acquire),
        submitted,
        stats.submitted_jobs,
        stats.completed_jobs,
        stats.polled_jobs,
        stats.queued_jobs
    );
}

fn slot_poll_deltas(before: &[(u32, u64)], after: &[(u32, u64)]) -> Vec<(u32, u64)> {
    after
        .iter()
        .copied()
        .filter_map(|(slot, count_after)| {
            let count_before = before
                .iter()
                .find(|(before_slot, _)| *before_slot == slot)
                .map(|(_, count)| *count)
                .unwrap_or(0);
            let delta = count_after.saturating_sub(count_before);
            (delta != 0).then_some((slot, delta))
        })
        .collect()
}

fn online_background_worker_slots() -> Vec<u32> {
    crate::workers::background_worker_slots()
        .into_iter()
        .filter(|slot| {
            crate::smp::read(*slot as usize)
                .map(|state| state.online)
                .unwrap_or(false)
        })
        .collect()
}

fn online_compute_worker_slots() -> Vec<u32> {
    let slots = online_background_worker_slots();
    let mut compute_slots: Vec<u32> = slots
        .iter()
        .copied()
        .filter(|slot| !is_service_protected_slot(*slot))
        .collect();
    if compute_slots.is_empty() {
        compute_slots = slots;
    }
    compute_slots
}

fn is_service_protected_slot(cpu_slot: u32) -> bool {
    if cpu_slot >= 64 {
        return false;
    }
    (SERVICE_PROTECTED_SLOTS.load(Ordering::Acquire) & (1u64 << cpu_slot)) != 0
}

fn should_skip_compute_slot(cpu_slot: u32) -> bool {
    if !is_service_protected_slot(cpu_slot) {
        return false;
    }
    let has_other_compute_slot = online_background_worker_slots()
        .into_iter()
        .any(|slot| slot != cpu_slot && !is_service_protected_slot(slot));
    if has_other_compute_slot && !LOGGED_SERVICE_PROTECTED_LANE.swap(true, Ordering::AcqRel) {
        crate::log!(
            "burn-baby: AP poll compute lane protected slot={} action=leave-for-service\n",
            cpu_slot
        );
    }
    has_other_compute_slot
}

fn submit_job(job: ComputeJob) {
    SUBMITTED_JOBS.fetch_add(1, Ordering::AcqRel);

    if !LOGGED_QUEUE_LANE.swap(true, Ordering::AcqRel) {
        let slots = online_compute_worker_slots();
        crate::log!(
            "burn-baby: queued compute jobs for AP poll lane workers={} slots={:?} protected_mask=0x{:016X}\n",
            slots.len(),
            slots,
            SERVICE_PROTECTED_SLOTS.load(Ordering::Acquire)
        );
    }
    FALLBACK_QUEUE.lock().push_back(job);
}

fn execute_job(job: ComputeJob) {
    match job {
        ComputeJob::MatvecRowsF32(job) => execute_matvec_rows_f32(job),
        ComputeJob::MatvecRowsBf16(job) => execute_matvec_rows_bf16(job),
        ComputeJob::MatvecRowsBf16Suffix(job) => execute_matvec_rows_bf16_suffix(job),
    }
    COMPLETED_JOBS.fetch_add(1, Ordering::AcqRel);
}

fn execute_matvec_rows_f32(job: MatvecRowsF32) {
    if job.row_start >= job.row_end || job.row_end > job.n_rows {
        mark_done(job.done);
        return;
    }

    let x = unsafe { core::slice::from_raw_parts(job.x as *const f32, job.k_dim) };
    let w_len = job.n_rows.saturating_mul(job.k_dim);
    let w = unsafe { core::slice::from_raw_parts(job.w_rowmajor as *const f32, w_len) };
    let out = unsafe { core::slice::from_raw_parts_mut(job.out as *mut f32, job.n_rows) };

    matvec_rows_f32(x, w, job.k_dim, out, job.row_start, job.row_end);
    mark_done(job.done);
}

fn execute_matvec_rows_bf16(job: MatvecRowsBf16) {
    if job.row_start >= job.row_end || job.row_end > job.n_rows {
        mark_done(job.done);
        return;
    }

    let x = unsafe { core::slice::from_raw_parts(job.x as *const f32, job.k_dim) };
    let w_len = job.n_rows.saturating_mul(job.k_dim).saturating_mul(2);
    let w = unsafe { core::slice::from_raw_parts(job.w_rowmajor_bf16 as *const u8, w_len) };
    let out = unsafe { core::slice::from_raw_parts_mut(job.out as *mut f32, job.n_rows) };

    matvec_rows_bf16(x, w, job.k_dim, out, job.row_start, job.row_end);
    mark_done(job.done);
}

fn execute_matvec_rows_bf16_suffix(job: MatvecRowsBf16Suffix) {
    if job.live_k_dim >= job.k_dim
        || job.prefix_index_start >= job.prefix_index_end
        || job.prefix_index_end > job.prefix_count
    {
        mark_done(job.done);
        return;
    }

    let x = unsafe { core::slice::from_raw_parts(job.x as *const f32, job.k_dim) };
    let w_len = job.n_rows.saturating_mul(job.k_dim).saturating_mul(2);
    let w = unsafe { core::slice::from_raw_parts(job.w_rowmajor_bf16 as *const u8, w_len) };
    let out = unsafe { core::slice::from_raw_parts_mut(job.out as *mut f32, job.n_rows) };
    let prefix_rows = unsafe {
        core::slice::from_raw_parts(
            job.prefix_rows as *const crate::lumen::cgp::CgpBf16PrefixRow,
            job.prefix_count,
        )
    };

    for contribution in &prefix_rows[job.prefix_index_start..job.prefix_index_end] {
        let row = contribution.row;
        if row >= job.n_rows || row >= out.len() {
            continue;
        }
        let base = row.saturating_mul(job.k_dim).saturating_mul(2);
        let row_end = base.saturating_add(job.k_dim.saturating_mul(2));
        if row_end > w.len() {
            continue;
        }
        let weights = &w[base..row_end];
        let mut acc = f32::from_bits(contribution.prefix_bits);
        for idx in job.live_k_dim..job.k_dim {
            let off = idx.saturating_mul(2);
            let bits = u16::from_le_bytes([weights[off], weights[off + 1]]);
            acc += x[idx] * bf16_to_f32(bits);
        }
        out[row] = acc;
    }
    mark_done(job.done);
}

fn matvec_rows_f32(
    x: &[f32],
    w_rowmajor: &[f32],
    k_dim: usize,
    out: &mut [f32],
    row_start: usize,
    row_end: usize,
) {
    for row in row_start..row_end {
        let base = row * k_dim;
        let weights = &w_rowmajor[base..base + k_dim];
        let mut acc = 0.0f32;
        for idx in 0..k_dim {
            acc += x[idx] * weights[idx];
        }
        out[row] = acc;
    }
}

fn matvec_rows_bf16(
    x: &[f32],
    w_rowmajor_bf16: &[u8],
    k_dim: usize,
    out: &mut [f32],
    row_start: usize,
    row_end: usize,
) {
    #[cfg(target_arch = "x86_64")]
    {
        if bf16_simd_lane() == BF16_SIMD_LANE_AVX2_FMA {
            if !LOGGED_BF16_AVX2_LANE.swap(true, Ordering::AcqRel) {
                crate::log!(
                    "burn-baby: bf16 matvec AVX2/FMA lane active rows={} k_dim={}\n",
                    row_end.saturating_sub(row_start),
                    k_dim
                );
            }
            unsafe {
                crate::turbo::avx2_fma_sse2_help::matvec_rows_bf16_avx2_fma(
                    x,
                    w_rowmajor_bf16,
                    k_dim,
                    out,
                    row_start,
                    row_end,
                );
            }
            return;
        }

        if !LOGGED_BF16_SSE2_LANE.swap(true, Ordering::AcqRel) {
            crate::log!(
                "burn-baby: bf16 matvec SSE2 lane active rows={} k_dim={}\n",
                row_end.saturating_sub(row_start),
                k_dim
            );
        }
        unsafe {
            crate::turbo::avx2_fma_sse2_help::matvec_rows_bf16_sse2(
                x,
                w_rowmajor_bf16,
                k_dim,
                out,
                row_start,
                row_end,
            );
        }
        return;
    }

    #[cfg(not(target_arch = "x86_64"))]
    crate::turbo::avx2_fma_sse2_help::matvec_rows_bf16_scalar(
        x,
        w_rowmajor_bf16,
        k_dim,
        out,
        row_start,
        row_end,
    );
}

#[cfg(target_arch = "x86_64")]
fn log_bf16_dispatch_plan(n_rows: usize, k_dim: usize, chunk_rows: usize, chunks: usize) {
    if LOGGED_BF16_DISPATCH_PLAN.swap(true, Ordering::AcqRel) {
        return;
    }

    let lane = match bf16_simd_lane() {
        BF16_SIMD_LANE_AVX2_FMA => "avx2-fma",
        BF16_SIMD_LANE_SSE2 => "sse2",
        _ => "unknown",
    };
    crate::log!(
        "burn-baby: bf16 dispatch plan rows={} k_dim={} chunk_rows={} chunks={} workers={} lane={}\n",
        n_rows,
        k_dim,
        chunk_rows,
        chunks,
        online_worker_count(),
        lane
    );
}

#[cfg(not(target_arch = "x86_64"))]
#[inline]
fn log_bf16_dispatch_plan(_n_rows: usize, _k_dim: usize, _chunk_rows: usize, _chunks: usize) {}

#[cfg(target_arch = "x86_64")]
fn bf16_simd_lane() -> u8 {
    let cached = BF16_SIMD_LANE.load(Ordering::Acquire);
    if cached != BF16_SIMD_LANE_UNKNOWN {
        return cached;
    }

    let status = crate::cpu::simd_status();
    let selected = if status.avx2_fma_ready {
        BF16_SIMD_LANE_AVX2_FMA
    } else {
        BF16_SIMD_LANE_SSE2
    };

    let _ = BF16_SIMD_LANE.compare_exchange(
        BF16_SIMD_LANE_UNKNOWN,
        selected,
        Ordering::AcqRel,
        Ordering::Acquire,
    );

    if !LOGGED_BF16_SIMD_PROBE.swap(true, Ordering::AcqRel) {
        let lane = if selected == BF16_SIMD_LANE_AVX2_FMA {
            "avx2-fma"
        } else {
            "sse2"
        };
        crate::log!(
            "burn-baby: bf16 simd probe avx_state={} reason={} avx2_fma={} reason={} selected={}\n",
            status.avx_state_enabled,
            status.avx_state_reason.as_str(),
            status.avx2_fma_ready,
            status.avx2_fma_reason.as_str(),
            lane
        );
    }

    BF16_SIMD_LANE.load(Ordering::Acquire)
}

fn mark_done(done: usize) {
    if done == 0 {
        return;
    }
    let done = unsafe { &*(done as *const AtomicUsize) };
    done.fetch_add(1, Ordering::AcqRel);
}
