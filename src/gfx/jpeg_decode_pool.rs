use alloc::sync::Arc;
use alloc::vec::Vec;
use core::cmp;
use core::ops::Range;
use core::sync::atomic::{AtomicUsize, Ordering};

use spin::Mutex;

use super::jpeg_codec::JpegDecodeError;

const JPEG_ROW_EXPAND_TASK_POOL_SIZE: usize = 8;
const JPEG_ROW_EXPAND_MIN_ROWS_PER_TASK: usize = 16;
const JPEG_ROW_EXPAND_SINGLE_AP_MAX_PIXELS: usize = 512 * 512;
const JPEG_ROW_EXPAND_POOL_ENABLED: bool = true;

struct RowExpandPlan {
    width: usize,
    height: usize,
    rgb: Arc<Vec<u8>>,
}

struct RowExpandJob {
    plan: RowExpandPlan,
    ranges: Vec<Range<usize>>,
    results: Mutex<Vec<Option<Result<Vec<u8>, JpegDecodeError>>>>,
    remaining: AtomicUsize,
    wait: crate::wait::WaitQueue,
}

impl RowExpandPlan {
    fn process_rows(&self, rows: Range<usize>) -> Result<Vec<u8>, JpegDecodeError> {
        expand_rgb_rows(self.width, rows, self.rgb.as_slice())
    }
}

impl RowExpandJob {
    fn new(plan: RowExpandPlan, ranges: Vec<Range<usize>>) -> Arc<Self> {
        let part_count = ranges.len();
        let mut results = Vec::with_capacity(part_count);
        results.resize_with(part_count, || None);
        Arc::new(Self {
            plan,
            ranges,
            results: Mutex::new(results),
            remaining: AtomicUsize::new(part_count),
            wait: crate::wait::WaitQueue::new(),
        })
    }

    fn process_part(&self, part_index: usize) -> Result<Vec<u8>, JpegDecodeError> {
        let rows = self
            .ranges
            .get(part_index)
            .ok_or(JpegDecodeError::DecodeFailed)?
            .clone();
        self.plan.process_rows(rows)
    }

    fn finish_part(&self, part_index: usize, result: Result<Vec<u8>, JpegDecodeError>) {
        if let Some(slot) = self.results.lock().get_mut(part_index) {
            *slot = Some(result);
        }
        self.remaining.fetch_sub(1, Ordering::AcqRel);
        self.wait.notify_all();
    }

    fn wait_for_completion(&self) {
        while self.remaining.load(Ordering::Acquire) != 0 {
            self.wait.wait_for_event_blocking(0);
        }
    }

    fn collect(self: Arc<Self>) -> Result<Vec<u8>, JpegDecodeError> {
        let mut out = Vec::with_capacity(
            self.plan
                .width
                .saturating_mul(self.plan.height)
                .saturating_mul(4),
        );
        let mut results = self.results.lock();
        for slot in results.iter_mut() {
            let chunk = match slot.take() {
                Some(Ok(chunk)) => chunk,
                Some(Err(err)) => return Err(err),
                None => return Err(JpegDecodeError::DecodeFailed),
            };
            out.extend_from_slice(chunk.as_slice());
        }
        Ok(out)
    }
}

fn desired_part_count(width: usize, height: usize) -> usize {
    let pixel_count = width.saturating_mul(height);
    let pool_cap = if pixel_count <= JPEG_ROW_EXPAND_SINGLE_AP_MAX_PIXELS {
        2
    } else {
        JPEG_ROW_EXPAND_TASK_POOL_SIZE
    };
    let max_parts = cmp::min(pool_cap, height.max(1));
    if max_parts <= 1 {
        return 1;
    }
    let mut parts = max_parts;
    while parts > 1 && height / parts < JPEG_ROW_EXPAND_MIN_ROWS_PER_TASK {
        parts -= 1;
    }
    parts.max(1)
}

fn partition_rows(height: usize, part_count: usize) -> Vec<Range<usize>> {
    let parts = cmp::min(part_count.max(1), height.max(1));
    let mut ranges = Vec::with_capacity(parts);
    let mut start = 0usize;
    for idx in 0..parts {
        let remaining_parts = parts - idx;
        let remaining_rows = height.saturating_sub(start);
        let rows = remaining_rows.div_ceil(remaining_parts);
        let end = start.saturating_add(rows);
        ranges.push(start..end);
        start = end;
    }
    ranges
}

fn spawn_parallel(job: &Arc<RowExpandJob>) {
    let mut spawned = vec![false; job.ranges.len()];
    for part_index in 1..job.ranges.len() {
        let Some(spawner) = crate::workers::pick_background_spawner() else {
            break;
        };
        let Ok(token) = jpeg_row_expand_task(job.clone(), part_index) else {
            break;
        };
        spawner.spawn(token);
        spawned[part_index] = true;
    }

    for (part_index, did_spawn) in spawned.into_iter().enumerate() {
        if did_spawn {
            continue;
        }
        let result = job.process_part(part_index);
        job.finish_part(part_index, result);
    }
}

#[embassy_executor::task(pool_size = JPEG_ROW_EXPAND_TASK_POOL_SIZE)]
async fn jpeg_row_expand_task(job: Arc<RowExpandJob>, part_index: usize) {
    let result = job.process_part(part_index);
    job.finish_part(part_index, result);
}

fn expand_rgb_rows(
    width: usize,
    rows: Range<usize>,
    pixels: &[u8],
) -> Result<Vec<u8>, JpegDecodeError> {
    let src_row_bytes = width.saturating_mul(3);
    let expected = src_row_bytes.saturating_mul(rows.end);
    if pixels.len() < expected {
        return Err(JpegDecodeError::DecodeFailed);
    }

    let mut out = Vec::with_capacity(rows.len().saturating_mul(width).saturating_mul(4));
    for y in rows {
        let row_start = y.saturating_mul(src_row_bytes);
        let row = &pixels[row_start..row_start + src_row_bytes];
        for chunk in row.chunks_exact(3) {
            out.extend_from_slice(&[chunk[0], chunk[1], chunk[2], 0xFF]);
        }
    }
    Ok(out)
}

pub(super) fn expand_jpeg_rgb_to_rgba(
    width: u32,
    height: u32,
    rgb: Vec<u8>,
) -> Result<Vec<u8>, JpegDecodeError> {
    let plan = RowExpandPlan {
        width: width as usize,
        height: height as usize,
        rgb: Arc::new(rgb),
    };
    if !JPEG_ROW_EXPAND_POOL_ENABLED {
        return plan.process_rows(0..plan.height);
    }
    if !crate::workers::has_background_worker_slot() {
        return plan.process_rows(0..plan.height);
    }
    let ranges = partition_rows(plan.height, desired_part_count(plan.width, plan.height));
    if ranges.len() <= 1 {
        return plan.process_rows(0..plan.height);
    }
    let job = RowExpandJob::new(plan, ranges);
    spawn_parallel(&job);
    job.wait_for_completion();
    job.collect()
}