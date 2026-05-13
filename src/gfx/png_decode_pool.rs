use alloc::sync::Arc;
use alloc::vec::Vec;
use core::cmp;
use core::ops::Range;
use core::sync::atomic::{AtomicUsize, Ordering};

use spin::Mutex;

use super::png_codec::PngDecodeError;

const PNG_ROW_EXPAND_TASK_POOL_SIZE: usize = 16;
const PNG_ROW_EXPAND_MIN_ROWS_PER_TASK: usize = 8;
const PNG_ROW_EXPAND_SINGLE_AP_MAX_PIXELS: usize = 256 * 256;

enum RowExpandKind {
    Indexed {
        bit_depth: png::BitDepth,
        packed: Arc<Vec<u8>>,
        palette: Arc<Vec<u8>>,
        trns: Option<Arc<Vec<u8>>>,
    },
    Rgb {
        pixels: Arc<Vec<u8>>,
    },
    Gray {
        pixels: Arc<Vec<u8>>,
    },
    GrayAlpha {
        pixels: Arc<Vec<u8>>,
    },
}

struct RowExpandPlan {
    width: usize,
    height: usize,
    kind: RowExpandKind,
}

struct RowExpandJob {
    plan: RowExpandPlan,
    ranges: Vec<Range<usize>>,
    results: Mutex<Vec<Option<Result<Vec<u8>, PngDecodeError>>>>,
    remaining: AtomicUsize,
    wait: crate::wait::WaitQueue,
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

    fn process_part(&self, part_index: usize) -> Result<Vec<u8>, PngDecodeError> {
        let rows = self
            .ranges
            .get(part_index)
            .ok_or(PngDecodeError::DecodeFailed)?
            .clone();
        self.plan.process_rows(rows)
    }

    fn finish_part(&self, part_index: usize, result: Result<Vec<u8>, PngDecodeError>) {
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

    fn collect(self: Arc<Self>) -> Result<Vec<u8>, PngDecodeError> {
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
                None => return Err(PngDecodeError::DecodeFailed),
            };
            out.extend_from_slice(chunk.as_slice());
        }
        Ok(out)
    }
}

impl RowExpandPlan {
    fn process_rows(&self, rows: Range<usize>) -> Result<Vec<u8>, PngDecodeError> {
        match &self.kind {
            RowExpandKind::Indexed {
                bit_depth,
                packed,
                palette,
                trns,
            } => expand_indexed_rows(
                self.width,
                rows,
                *bit_depth,
                packed.as_slice(),
                palette.as_slice(),
                trns.as_deref().map(|bytes| bytes.as_slice()),
            ),
            RowExpandKind::Rgb { pixels } => expand_rgb_rows(self.width, rows, pixels.as_slice()),
            RowExpandKind::Gray { pixels } => expand_gray_rows(self.width, rows, pixels.as_slice()),
            RowExpandKind::GrayAlpha { pixels } => {
                expand_gray_alpha_rows(self.width, rows, pixels.as_slice())
            }
        }
    }
}

fn indexed_row_bytes(width: usize, bit_depth: png::BitDepth) -> Result<usize, PngDecodeError> {
    let bits_per_pixel = match bit_depth {
        png::BitDepth::One => 1usize,
        png::BitDepth::Two => 2usize,
        png::BitDepth::Four => 4usize,
        png::BitDepth::Eight => 8usize,
        png::BitDepth::Sixteen => return Err(PngDecodeError::Unsupported),
    };
    Ok(width.saturating_mul(bits_per_pixel).saturating_add(7) / 8)
}

fn indexed_sample(bits: &[u8], bit_depth: png::BitDepth, x: usize) -> Result<u8, PngDecodeError> {
    match bit_depth {
        png::BitDepth::One => {
            let byte = *bits.get(x / 8).ok_or(PngDecodeError::DecodeFailed)?;
            Ok((byte >> (7 - (x % 8))) & 0x01)
        }
        png::BitDepth::Two => {
            let byte = *bits.get(x / 4).ok_or(PngDecodeError::DecodeFailed)?;
            Ok((byte >> (6 - ((x % 4) * 2))) & 0x03)
        }
        png::BitDepth::Four => {
            let byte = *bits.get(x / 2).ok_or(PngDecodeError::DecodeFailed)?;
            Ok(if (x & 1) == 0 { byte >> 4 } else { byte & 0x0F })
        }
        png::BitDepth::Eight => bits.get(x).copied().ok_or(PngDecodeError::DecodeFailed),
        png::BitDepth::Sixteen => Err(PngDecodeError::Unsupported),
    }
}

fn expand_indexed_rows(
    width: usize,
    rows: Range<usize>,
    bit_depth: png::BitDepth,
    packed_indices: &[u8],
    palette: &[u8],
    trns: Option<&[u8]>,
) -> Result<Vec<u8>, PngDecodeError> {
    if palette.len() < 3 {
        return Err(PngDecodeError::Invalid);
    }

    let row_bytes = indexed_row_bytes(width, bit_depth)?;
    let expected = row_bytes.saturating_mul(rows.end);
    if packed_indices.len() < expected {
        return Err(PngDecodeError::DecodeFailed);
    }

    let mut rgba = Vec::with_capacity(rows.len().saturating_mul(width).saturating_mul(4));
    for y in rows {
        let row_start = y.saturating_mul(row_bytes);
        let row = &packed_indices[row_start..row_start + row_bytes];
        for x in 0..width {
            let sample = indexed_sample(row, bit_depth, x)? as usize;
            let palette_idx = sample.saturating_mul(3);
            let rgb = palette
                .get(palette_idx..palette_idx + 3)
                .ok_or(PngDecodeError::DecodeFailed)?;
            let alpha = trns
                .and_then(|table| table.get(sample).copied())
                .unwrap_or(0xFF);
            rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], alpha]);
        }
    }
    Ok(rgba)
}

fn expand_rgb_rows(
    width: usize,
    rows: Range<usize>,
    pixels: &[u8],
) -> Result<Vec<u8>, PngDecodeError> {
    let src_row_bytes = width.saturating_mul(3);
    let expected = src_row_bytes.saturating_mul(rows.end);
    if pixels.len() < expected {
        return Err(PngDecodeError::DecodeFailed);
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

fn expand_gray_rows(
    width: usize,
    rows: Range<usize>,
    pixels: &[u8],
) -> Result<Vec<u8>, PngDecodeError> {
    let src_row_bytes = width;
    let expected = src_row_bytes.saturating_mul(rows.end);
    if pixels.len() < expected {
        return Err(PngDecodeError::DecodeFailed);
    }

    let mut out = Vec::with_capacity(rows.len().saturating_mul(width).saturating_mul(4));
    for y in rows {
        let row_start = y.saturating_mul(src_row_bytes);
        let row = &pixels[row_start..row_start + src_row_bytes];
        for &value in row {
            out.extend_from_slice(&[value, value, value, 0xFF]);
        }
    }
    Ok(out)
}

fn expand_gray_alpha_rows(
    width: usize,
    rows: Range<usize>,
    pixels: &[u8],
) -> Result<Vec<u8>, PngDecodeError> {
    let src_row_bytes = width.saturating_mul(2);
    let expected = src_row_bytes.saturating_mul(rows.end);
    if pixels.len() < expected {
        return Err(PngDecodeError::DecodeFailed);
    }

    let mut out = Vec::with_capacity(rows.len().saturating_mul(width).saturating_mul(4));
    for y in rows {
        let row_start = y.saturating_mul(src_row_bytes);
        let row = &pixels[row_start..row_start + src_row_bytes];
        for chunk in row.chunks_exact(2) {
            out.extend_from_slice(&[chunk[0], chunk[0], chunk[0], chunk[1]]);
        }
    }
    Ok(out)
}

fn desired_part_count(width: usize, height: usize) -> usize {
    let pixel_count = width.saturating_mul(height);
    let pool_cap = if pixel_count <= PNG_ROW_EXPAND_SINGLE_AP_MAX_PIXELS {
        2
    } else {
        PNG_ROW_EXPAND_TASK_POOL_SIZE
    };
    let max_parts = cmp::min(pool_cap, height.max(1));
    if max_parts <= 1 {
        return 1;
    }
    let mut parts = max_parts;
    while parts > 1 && height / parts < PNG_ROW_EXPAND_MIN_ROWS_PER_TASK {
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

fn run_parallel(plan: RowExpandPlan) -> Result<Vec<u8>, PngDecodeError> {
    let part_count = desired_part_count(plan.width, plan.height);
    if part_count <= 1 {
        return plan.process_rows(0..plan.height);
    }

    let job = RowExpandJob::new(plan, partition_rows(job_height(&job_placeholder()), 1));
    drop(job);
    unreachable!()
}

fn job_height(job: &Arc<RowExpandJob>) -> usize {
    job.plan.height
}

fn job_placeholder() -> Arc<RowExpandJob> {
    unreachable!()
}

fn spawn_parallel(job: &Arc<RowExpandJob>) {
    let mut spawned = vec![false; job.ranges.len()];
    for part_index in 1..job.ranges.len() {
        let Some(spawner) = crate::workers::pick_background_spawner() else {
            break;
        };
        let Ok(token) = png_row_expand_task(job.clone(), part_index) else {
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

#[embassy_executor::task(pool_size = PNG_ROW_EXPAND_TASK_POOL_SIZE)]
async fn png_row_expand_task(job: Arc<RowExpandJob>, part_index: usize) {
    let result = job.process_part(part_index);
    job.finish_part(part_index, result);
}

pub(super) fn expand_indexed_png_to_rgba(
    width: u32,
    height: u32,
    bit_depth: png::BitDepth,
    packed_indices: Vec<u8>,
    palette: Vec<u8>,
    trns: Option<Vec<u8>>,
) -> Result<Vec<u8>, PngDecodeError> {
    let plan = RowExpandPlan {
        width: width as usize,
        height: height as usize,
        kind: RowExpandKind::Indexed {
            bit_depth,
            packed: Arc::new(packed_indices),
            palette: Arc::new(palette),
            trns: trns.map(Arc::new),
        },
    };
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

pub(super) fn expand_png_output_to_rgba(
    color_type: png::ColorType,
    bit_depth: png::BitDepth,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
) -> Result<Vec<u8>, PngDecodeError> {
    if bit_depth != png::BitDepth::Eight {
        return Err(PngDecodeError::Unsupported);
    }

    let plan_kind = match color_type {
        png::ColorType::Rgb => RowExpandKind::Rgb {
            pixels: Arc::new(pixels),
        },
        png::ColorType::Grayscale => RowExpandKind::Gray {
            pixels: Arc::new(pixels),
        },
        png::ColorType::GrayscaleAlpha => RowExpandKind::GrayAlpha {
            pixels: Arc::new(pixels),
        },
        png::ColorType::Rgba => return Ok(pixels),
        png::ColorType::Indexed => return Err(PngDecodeError::Unsupported),
    };

    let plan = RowExpandPlan {
        width: width as usize,
        height: height as usize,
        kind: plan_kind,
    };
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
