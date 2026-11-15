//! Asynchronous character/VFX rendering behind the display-rate Codex tile.
//!
//! The GPU owns a separate persistent target. CPU cursor producers only read
//! the last proven, pristine BGRA snapshot, so a pending character render does
//! not hold the moving tile's next cursor presentation.

extern crate alloc;

use alloc::vec::Vec;
use spin::Mutex;

use super::{SpiritSubmitError, SpiritSurfaceLayout, lilly::LillyResidentFrame};
use crate::intel::gpgpu::{self, SpiritVfxCompletion, SpiritVfxControl, SpiritVfxSubmission};

const SIZE: u32 = 256;
const ROW_BYTES: usize = SIZE as usize * 4;
const BYTES: usize = ROW_BYTES * SIZE as usize;
// The resident asset loader already excludes this final 256 KiB interval.
const STAGING_GPU: u64 = 0x3ffc_0000;
const UPDATE_MS: u64 = 1000 / 30;

struct Job {
    submission: SpiritVfxSubmission,
    source: LillyResidentFrame,
    generation: u64,
}

struct State {
    staging: Option<crate::gpu::resident::ResidentDmaBuffer>,
    pending: Option<Job>,
    clean: Vec<u8>,
    source: Option<LillyResidentFrame>,
    last_submit: Option<u64>,
    poisoned: bool,
    generation: u64,
}

impl State {
    const fn new() -> Self {
        Self {
            staging: None,
            pending: None,
            clean: Vec::new(),
            source: None,
            last_submit: None,
            poisoned: false,
            generation: 0,
        }
    }

    fn poll(&mut self) -> Result<(), SpiritSubmitError> {
        if self.poisoned {
            return Err(SpiritSubmitError::GpuSubmissionFailed);
        }
        let Some(job) = self.pending.as_ref() else {
            return Ok(());
        };
        match gpgpu::poll_spirit_vfx_submission(job.submission) {
            SpiritVfxCompletion::Pending => Ok(()),
            SpiritVfxCompletion::Complete(release) => {
                let dma = self
                    .staging
                    .as_ref()
                    .ok_or(SpiritSubmitError::HardwareNotReady)?;
                if !release.matches(dma.phys(), BYTES) {
                    self.poisoned = true;
                    return Err(SpiritSubmitError::InvalidGpuRelease);
                }
                if job.generation != self.generation {
                    self.pending = None;
                    return Ok(());
                }
                // Completion proves that the GPU has stopped writing this
                // allocation. Invalidate CPU cache lines before the readback.
                dma.flush();
                self.clean.resize(BYTES, 0);
                if !dma.read(0, &mut self.clean) {
                    self.poisoned = true;
                    return Err(SpiritSubmitError::InvalidCommand);
                }
                self.source = Some(job.source);
                self.pending = None;
                Ok(())
            }
            SpiritVfxCompletion::Failed | SpiritVfxCompletion::InvalidSubmission => {
                // Retain both the job and allocation. Failed retirement is
                // never permission to reuse or free GPU-owned storage.
                self.poisoned = true;
                Err(SpiritSubmitError::GpuSubmissionFailed)
            }
        }
    }
}

// This allocation intentionally outlives cancellations and worker failure:
// neither event proves that an outstanding GPU read/write has retired.
static STATE: Mutex<State> = Mutex::new(State::new());

fn valid_layout(pixels: &[u8], layout: SpiritSurfaceLayout) -> bool {
    layout.width == SIZE
        && layout.height == SIZE
        && layout.pitch_bytes as usize >= ROW_BYTES
        && (layout.pitch_bytes as usize)
            .checked_mul(SIZE as usize)
            .is_some_and(|bytes| bytes <= pixels.len() && bytes <= layout.byte_len)
}

pub(super) fn ready() -> bool {
    STATE.lock().source.is_some()
}

pub(super) fn current_source() -> Option<LillyResidentFrame> {
    STATE.lock().source
}

pub(super) fn busy() -> bool {
    STATE.lock().pending.is_some()
}

/// Start another handoff without letting an older render repopulate its cache.
/// In-flight storage stays pinned until its exact retirement proof arrives.
pub(super) fn invalidate() {
    let mut state = STATE.lock();
    state.generation = state.generation.wrapping_add(1);
    state.source = None;
    state.clean.clear();
}

/// Seed from a completed direct GPU cursor frame before applying any tile.
/// The caller owns the CPU producer bit and has proven GPU retirement.
pub(super) fn capture(
    source: LillyResidentFrame,
    pixels: &[u8],
    layout: SpiritSurfaceLayout,
) -> bool {
    if !valid_layout(pixels, layout) {
        return false;
    }
    let mut state = STATE.lock();
    state.clean.resize(BYTES, 0);
    for (row, dst) in state.clean.chunks_exact_mut(ROW_BYTES).enumerate() {
        let start = row * layout.pitch_bytes as usize;
        dst.copy_from_slice(&pixels[start..start + ROW_BYTES]);
    }
    state.source = Some(source);
    true
}

/// Restore a pristine character snapshot into a newly acquired CPU lease.
/// Padding belongs to the cursor allocation and is left untouched.
pub(super) fn copy_into(
    pixels: &mut [u8],
    layout: SpiritSurfaceLayout,
) -> Option<LillyResidentFrame> {
    if !valid_layout(pixels, layout) {
        return None;
    }
    let state = STATE.lock();
    let source = state.source?;
    for (row, src) in state.clean.chunks_exact(ROW_BYTES).enumerate() {
        let start = row * layout.pitch_bytes as usize;
        pixels[start..start + ROW_BYTES].copy_from_slice(src);
    }
    Some(source)
}

/// Poll even after the handoff is canceled or GPU Logger takes ownership.
pub(super) fn poll() -> Result<(), SpiritSubmitError> {
    STATE.lock().poll()
}

/// Advance the ordinary character/VFX producer without waiting for its GPU.
pub(super) fn update(
    source: LillyResidentFrame,
    control: SpiritVfxControl,
    present_fps: u32,
) -> Result<(), SpiritSubmitError> {
    let mut state = STATE.lock();
    state.poll()?;
    let now = trueos_time::Instant::now().as_millis();
    if state.pending.is_some()
        || state
            .last_submit
            .is_some_and(|t| now.saturating_sub(t) < UPDATE_MS)
    {
        return Ok(());
    }
    if state.staging.is_none() {
        state.staging = Some(
            crate::gpu::resident::ResidentDmaBuffer::allocate_zeroed(BYTES, 4096)
                .ok_or(SpiritSubmitError::HardwareNotReady)?,
        );
    }
    let dma = state
        .staging
        .as_ref()
        .ok_or(SpiritSubmitError::HardwareNotReady)?;
    let target = gpgpu::GpgpuRgba8Surface::new_bgra(
        dma.phys(),
        STAGING_GPU,
        BYTES,
        SIZE,
        SIZE,
        ROW_BYTES as u32,
    )
    .ok_or(SpiritSubmitError::InvalidCommand)?;
    let sprite = gpgpu::GpgpuRgba8Surface::new(
        source.phys,
        source.gpu,
        source.bytes,
        source.width,
        source.height,
        source.pitch_bytes,
    )
    .ok_or(SpiritSubmitError::InvalidCommand)?;
    if let Some(submission) = gpgpu::submit_spirit_vfx_frame(target, sprite, control, present_fps) {
        state.pending = Some(Job {
            submission,
            source,
            generation: state.generation,
        });
        state.last_submit = Some(now);
    }
    Ok(())
}
