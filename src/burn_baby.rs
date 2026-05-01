extern crate alloc;

use alloc::collections::VecDeque;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use spin::Mutex;

const DEFAULT_CHUNK_ROWS: usize = 16;

static FALLBACK_QUEUE: Mutex<VecDeque<ComputeJob>> = Mutex::new(VecDeque::new());
static LOGGED_EMBASSY_LANE: AtomicBool = AtomicBool::new(false);
static LOGGED_POLL_LANE: AtomicBool = AtomicBool::new(false);
static SUBMITTED_JOBS: AtomicU64 = AtomicU64::new(0);
static COMPLETED_JOBS: AtomicU64 = AtomicU64::new(0);
static POLLED_JOBS: AtomicU64 = AtomicU64::new(0);

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
enum ComputeJob {
    MatvecRowsF32(MatvecRowsF32),
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

pub fn poll_compute_lane() -> bool {
    let job = FALLBACK_QUEUE.lock().pop_front();
    let Some(job) = job else {
        return false;
    };

    if !LOGGED_POLL_LANE.swap(true, Ordering::AcqRel) {
        crate::log!(
            "burn-baby: AP poll compute lane active slot={}\n",
            crate::percpu::current_slot()
        );
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
        DEFAULT_CHUNK_ROWS
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
        submit_job(job, current_cpu_can_block_on_background_tasks());
        submitted += 1;
        row_start = row_end;
    }

    while done.load(Ordering::Acquire) != submitted {
        crate::time::poll();
        crate::runtime::poll_local_executor();
        while poll_compute_lane() {}
        crate::smp::poll();
        core::hint::spin_loop();
    }

    Ok(())
}

fn submit_job(job: ComputeJob, allow_embassy_task: bool) {
    SUBMITTED_JOBS.fetch_add(1, Ordering::AcqRel);

    if allow_embassy_task {
        if let Some((_slot, _kind, spawner)) = crate::workers::pick_background_spawner_with_slot() {
            if let Ok(token) = compute_job_task(job) {
                if !LOGGED_EMBASSY_LANE.swap(true, Ordering::AcqRel) {
                    crate::log!("burn-baby: using Embassy AP compute task pool\n");
                }
                spawner.spawn(token);
                return;
            }
        }
    }

    FALLBACK_QUEUE.lock().push_back(job);
}

fn current_cpu_can_block_on_background_tasks() -> bool {
    crate::percpu::current_slot() < 2
}

fn execute_job(job: ComputeJob) {
    match job {
        ComputeJob::MatvecRowsF32(job) => execute_matvec_rows_f32(job),
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

fn mark_done(done: usize) {
    if done == 0 {
        return;
    }
    let done = unsafe { &*(done as *const AtomicUsize) };
    done.fetch_add(1, Ordering::AcqRel);
}
