extern crate alloc;

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use spin::Mutex;

const TARGET_CHUNKS_PER_WORKER: usize = 4;
const MIN_CHUNK_ROWS: usize = 16;
const MAX_CHUNK_ROWS: usize = 256;
const MAX_COMPUTE_POLL_SLOTS: usize = 256;

static FALLBACK_QUEUE: Mutex<VecDeque<ComputeJob>> = Mutex::new(VecDeque::new());
static LOGGED_QUEUE_LANE: AtomicBool = AtomicBool::new(false);
static LOGGED_POLL_LANE: AtomicBool = AtomicBool::new(false);
#[cfg(target_arch = "x86_64")]
static LOGGED_BF16_SSE2_LANE: AtomicBool = AtomicBool::new(false);
static SUBMITTED_JOBS: AtomicU64 = AtomicU64::new(0);
static COMPLETED_JOBS: AtomicU64 = AtomicU64::new(0);
static POLLED_JOBS: AtomicU64 = AtomicU64::new(0);
static POLLED_JOBS_BY_SLOT: [AtomicU64; MAX_COMPUTE_POLL_SLOTS] =
    [const { AtomicU64::new(0) }; MAX_COMPUTE_POLL_SLOTS];

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ComputeError {
    BadShape,
    EmptyChunk,
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
enum ComputeJob {
    MatvecRowsF32(MatvecRowsF32),
    MatvecRowsBf16(MatvecRowsBf16),
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
    execute_job(job);
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
    online_background_worker_slots().len().max(1)
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
    let job = FALLBACK_QUEUE.lock().pop_front();
    let Some(job) = job else {
        return false;
    };

    let slot = crate::percpu::current_slot();
    if !LOGGED_POLL_LANE.swap(true, Ordering::AcqRel) {
        crate::log!("burn-baby: AP poll compute lane active slot={}\n", slot);
    }

    if let Some(counter) = POLLED_JOBS_BY_SLOT.get(slot as usize) {
        counter.fetch_add(1, Ordering::AcqRel);
    }
    POLLED_JOBS.fetch_add(1, Ordering::AcqRel);
    execute_job(job);
    true
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
        crate::runtime::poll_local_executor();
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
    if chunks <= 1 || !crate::workers::has_background_worker_slot() {
        matvec_rows_bf16(x, w_rowmajor_bf16, k_dim, out, 0, n_rows);
        return Ok(());
    }
    crate::burn_baba::share_matvec_rowmajor_bf16(n_rows, k_dim, chunk_rows);

    let done = AtomicUsize::new(0);
    let done_ptr = &done as *const AtomicUsize as usize;
    let x_ptr = x.as_ptr() as usize;
    let w_ptr = w_rowmajor_bf16.as_ptr() as usize;
    let out_ptr = out.as_mut_ptr() as usize;

    let mut submitted = 0usize;
    let mut row_start = 0usize;
    while row_start < n_rows {
        let row_end = row_start.saturating_add(chunk_rows).min(n_rows);
        let job = ComputeJob::MatvecRowsBf16(MatvecRowsBf16 {
            x: x_ptr,
            w_rowmajor_bf16: w_ptr,
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
        crate::runtime::poll_local_executor();
        crate::smp::poll();
        log_compute_wait_progress(&done, submitted, &mut last_wait_log, "bf16");
        if !poll_compute_lane() {
            core::hint::spin_loop();
        }
    }

    Ok(())
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

fn submit_job(job: ComputeJob) {
    SUBMITTED_JOBS.fetch_add(1, Ordering::AcqRel);

    if !LOGGED_QUEUE_LANE.swap(true, Ordering::AcqRel) {
        let slots = online_background_worker_slots();
        crate::log!(
            "burn-baby: queued compute jobs for AP poll lane workers={} slots={:?}\n",
            slots.len(),
            slots
        );
    }
    FALLBACK_QUEUE.lock().push_back(job);
}

fn execute_job(job: ComputeJob) {
    match job {
        ComputeJob::MatvecRowsF32(job) => execute_matvec_rows_f32(job),
        ComputeJob::MatvecRowsBf16(job) => execute_matvec_rows_bf16(job),
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
        if !LOGGED_BF16_SSE2_LANE.swap(true, Ordering::AcqRel) {
            crate::log!(
                "burn-baby: bf16 matvec SSE2 lane active rows={} k_dim={}\n",
                row_end.saturating_sub(row_start),
                k_dim
            );
        }
        unsafe {
            matvec_rows_bf16_sse2(x, w_rowmajor_bf16, k_dim, out, row_start, row_end);
        }
        return;
    }

    #[cfg(not(target_arch = "x86_64"))]
    for row in row_start..row_end {
        let base = row * k_dim * 2;
        let weights = &w_rowmajor_bf16[base..base + k_dim * 2];
        let mut acc = 0.0f32;
        for idx in 0..k_dim {
            let off = idx * 2;
            let bits = u16::from_le_bytes([weights[off], weights[off + 1]]);
            acc += x[idx] * bf16_to_f32(bits);
        }
        out[row] = acc;
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn matvec_rows_bf16_sse2(
    x: &[f32],
    w_rowmajor_bf16: &[u8],
    k_dim: usize,
    out: &mut [f32],
    row_start: usize,
    row_end: usize,
) {
    use core::arch::x86_64::{
        __m128, __m128i, _mm_add_ps, _mm_castsi128_ps, _mm_loadl_epi64, _mm_loadu_ps, _mm_mul_ps,
        _mm_setzero_ps, _mm_setzero_si128, _mm_slli_epi32, _mm_storeu_ps, _mm_unpacklo_epi16,
    };

    #[inline(always)]
    unsafe fn load_bf16x4_as_f32(ptr: *const u8) -> __m128 {
        let raw = unsafe { _mm_loadl_epi64(ptr.cast::<__m128i>()) };
        let widened = _mm_unpacklo_epi16(raw, _mm_setzero_si128());
        _mm_castsi128_ps(_mm_slli_epi32(widened, 16))
    }

    #[inline(always)]
    unsafe fn reduce_f32x4(v: __m128) -> f32 {
        let mut lanes = [0.0f32; 4];
        unsafe { _mm_storeu_ps(lanes.as_mut_ptr(), v) };
        lanes[0] + lanes[1] + lanes[2] + lanes[3]
    }

    for row in row_start..row_end {
        let base = row * k_dim * 2;
        let weights = unsafe { w_rowmajor_bf16.as_ptr().add(base) };
        let mut idx = 0usize;
        let mut acc0 = _mm_setzero_ps();
        let mut acc1 = _mm_setzero_ps();

        while idx + 8 <= k_dim {
            let x0 = unsafe { _mm_loadu_ps(x.as_ptr().add(idx)) };
            let x1 = unsafe { _mm_loadu_ps(x.as_ptr().add(idx + 4)) };
            let w0 = unsafe { load_bf16x4_as_f32(weights.add(idx * 2)) };
            let w1 = unsafe { load_bf16x4_as_f32(weights.add((idx + 4) * 2)) };
            acc0 = _mm_add_ps(acc0, _mm_mul_ps(x0, w0));
            acc1 = _mm_add_ps(acc1, _mm_mul_ps(x1, w1));
            idx += 8;
        }

        while idx + 4 <= k_dim {
            let x0 = unsafe { _mm_loadu_ps(x.as_ptr().add(idx)) };
            let w0 = unsafe { load_bf16x4_as_f32(weights.add(idx * 2)) };
            acc0 = _mm_add_ps(acc0, _mm_mul_ps(x0, w0));
            idx += 4;
        }

        let mut acc = unsafe { reduce_f32x4(acc0) + reduce_f32x4(acc1) };
        while idx < k_dim {
            let off = idx * 2;
            let lo = unsafe { *weights.add(off) };
            let hi = unsafe { *weights.add(off + 1) };
            let bits = u16::from_le_bytes([lo, hi]);
            acc += x[idx] * bf16_to_f32(bits);
            idx += 1;
        }
        out[row] = acc;
    }
}

fn bf16_to_f32(bits: u16) -> f32 {
    f32::from_bits((bits as u32) << 16)
}

fn mark_done(done: usize) {
    if done == 0 {
        return;
    }
    let done = unsafe { &*(done as *const AtomicUsize) };
    done.fetch_add(1, Ordering::AcqRel);
}
