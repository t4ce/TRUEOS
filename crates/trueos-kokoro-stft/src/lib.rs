#![no_std]
#![deny(unsafe_code)]

//! Cooperative STFT execution for the one pinned Kokoro graph node.
//!
//! The admitted node consumes a finite FLOAT signal `[B, L]`, uses frame step
//! 5, frame length/DFT length 20, the model's exact periodic Hann window, and
//! `onesided=1`. It produces `[B, frames, 11, 2]` in real/imaginary order,
//! where `frames = 1 + (L - 20) / 5`. There is no centering or implicit
//! padding; a final incomplete frame is ignored.
//!
//! [`CooperativeStft::advance`] processes one caller-budgeted contiguous frame
//! range without allocation. The Hann and forward-DFT root tables are stored
//! as audited f32 bits, so the kernel never evaluates trigonometric functions.

/// STFT nodes admitted from the pinned Kokoro graph.
pub const PINNED_NODE_COVERAGE: usize = 1;
/// Stable node number in the prepared-model audit.
pub const PINNED_NODE_ID: u16 = 2159;
/// Offset, in input samples, between consecutive frames.
pub const FRAME_STEP: usize = 5;
/// Samples per frame and the fixed DFT size.
pub const FRAME_LENGTH: usize = 20;
/// Unique bins for a one-sided real DFT of length 20.
pub const OUTPUT_BINS: usize = FRAME_LENGTH / 2 + 1;
/// Real then imaginary components in the final output dimension.
pub const OUTPUT_COMPONENTS: usize = 2;
/// Output elements contributed by one batch/frame pair.
pub const OUTPUT_ELEMENTS_PER_FRAME: usize = OUTPUT_BINS * OUTPUT_COMPONENTS;

/// Exact f32 bits of the model initializer
/// `/decoder/decoder/generator/Constant_17_output_0`.
///
/// This is a periodic Hann window (`periodic=true`), so the final element is
/// nonzero. The values are exposed as bits to make artifact audits exact.
pub const HANN_WINDOW_BITS: [u32; FRAME_LENGTH] = [
    0x0000_0000,
    0x3cc8_78f6,
    0x3dc3_910d,
    0x3e53_0dd0,
    0x3eb0_e443,
    0x3f00_0000,
    0x3f27_8dde,
    0x3f4b_3c8c,
    0x3f67_8dde,
    0x3f79_bc38,
    0x3f80_0000,
    0x3f79_bc38,
    0x3f67_8dde,
    0x3f4b_3c8c,
    0x3f27_8dde,
    0x3f00_0000,
    0x3eb0_e443,
    0x3e53_0dd0,
    0x3dc3_910d,
    0x3cc8_78f6,
];

/// Exact f32 bits of `cos(2*pi*m/20)` for `m=0..20`.
pub const DFT_COS_BITS: [u32; FRAME_LENGTH] = [
    0x3f80_0000,
    0x3f73_7871,
    0x3f4f_1bbd,
    0x3f16_7918,
    0x3e9e_377a,
    0x0000_0000,
    0xbe9e_377a,
    0xbf16_7918,
    0xbf4f_1bbd,
    0xbf73_7871,
    0xbf80_0000,
    0xbf73_7871,
    0xbf4f_1bbd,
    0xbf16_7918,
    0xbe9e_377a,
    0x0000_0000,
    0x3e9e_377a,
    0x3f16_7918,
    0x3f4f_1bbd,
    0x3f73_7871,
];

/// Exact f32 bits of `-sin(2*pi*m/20)` for the forward DFT.
pub const DFT_NEG_SIN_BITS: [u32; FRAME_LENGTH] = [
    0x0000_0000,
    0xbe9e_377a,
    0xbf16_7918,
    0xbf4f_1bbd,
    0xbf73_7871,
    0xbf80_0000,
    0xbf73_7871,
    0xbf4f_1bbd,
    0xbf16_7918,
    0xbe9e_377a,
    0x0000_0000,
    0x3e9e_377a,
    0x3f16_7918,
    0x3f4f_1bbd,
    0x3f73_7871,
    0x3f80_0000,
    0x3f73_7871,
    0x3f4f_1bbd,
    0x3f16_7918,
    0x3e9e_377a,
];

/// A rejected fixed-node or buffer contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContractError {
    EmptyBatch,
    SignalTooShort,
    ShapeOverflow,
    InputLengthMismatch,
    NonFiniteInput,
    OutputLengthMismatch,
}

/// A rejected cooperative advance. Rejection never changes output or state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdvanceError {
    ZeroFrameBudget,
}

/// Checked shape of the fixed STFT output.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputShape {
    batch: usize,
    frames: usize,
    elements: usize,
}

impl OutputShape {
    pub const fn batch(self) -> usize {
        self.batch
    }

    pub const fn frames(self) -> usize {
        self.frames
    }

    pub const fn bins(self) -> usize {
        OUTPUT_BINS
    }

    pub const fn components(self) -> usize {
        OUTPUT_COMPONENTS
    }

    pub const fn elements(self) -> usize {
        self.elements
    }
}

/// Immutable finite signal and its prevalidated output shape.
#[derive(Clone, Copy, Debug)]
pub struct Problem<'a> {
    batch: usize,
    signal_length: usize,
    input: &'a [f32],
    output_shape: OutputShape,
}

impl<'a> Problem<'a> {
    /// Validate and seal a `[batch, signal_length]` FLOAT signal.
    ///
    /// Shape arithmetic and the complete finite-input scan happen here, before
    /// any output buffer is accepted or modified.
    pub fn new(
        batch: usize,
        signal_length: usize,
        input: &'a [f32],
    ) -> Result<Self, ContractError> {
        if batch == 0 {
            return Err(ContractError::EmptyBatch);
        }
        if signal_length < FRAME_LENGTH {
            return Err(ContractError::SignalTooShort);
        }

        let input_elements = batch
            .checked_mul(signal_length)
            .ok_or(ContractError::ShapeOverflow)?;
        let frames = signal_length
            .checked_sub(FRAME_LENGTH)
            .and_then(|tail| tail.checked_div(FRAME_STEP))
            .and_then(|frames| frames.checked_add(1))
            .ok_or(ContractError::ShapeOverflow)?;
        let output_elements = batch
            .checked_mul(frames)
            .and_then(|value| value.checked_mul(OUTPUT_ELEMENTS_PER_FRAME))
            .ok_or(ContractError::ShapeOverflow)?;

        if input.len() != input_elements {
            return Err(ContractError::InputLengthMismatch);
        }
        if input.iter().any(|value| !value.is_finite()) {
            return Err(ContractError::NonFiniteInput);
        }

        Ok(Self {
            batch,
            signal_length,
            input,
            output_shape: OutputShape {
                batch,
                frames,
                elements: output_elements,
            },
        })
    }

    pub const fn batch(&self) -> usize {
        self.batch
    }

    pub const fn signal_length(&self) -> usize {
        self.signal_length
    }

    pub const fn output_shape(&self) -> OutputShape {
        self.output_shape
    }
}

/// A contiguous frame range completed by one cooperative call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompletedRange {
    /// Batch index containing the entire range.
    pub batch: usize,
    /// First frame processed, inclusive.
    pub start_frame: usize,
    /// Frame after the last processed frame, exclusive.
    pub end_frame: usize,
    /// Number of batch/frame pairs complete after this call.
    pub completed_frames: usize,
    /// Total batch/frame pairs in the invocation.
    pub total_frames: usize,
}

/// Result of a cooperative frame-range advance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Advance {
    /// At least one frame was evaluated.
    Advanced(CompletedRange),
    /// The invocation was already complete; output was not touched.
    Complete,
}

/// Validated cooperative execution state borrowing caller-owned output.
pub struct CooperativeStft<'input, 'output> {
    problem: Problem<'input>,
    output: &'output mut [f32],
    next_batch: usize,
    next_frame: usize,
    completed_frames: usize,
}

impl<'input, 'output> CooperativeStft<'input, 'output> {
    /// Start an invocation after validating the exact output length.
    ///
    /// Neither successful construction nor rejection initializes output. Each
    /// frame range is committed when it is processed.
    pub fn start(
        problem: Problem<'input>,
        output: &'output mut [f32],
    ) -> Result<Self, ContractError> {
        if output.len() != problem.output_shape.elements {
            return Err(ContractError::OutputLengthMismatch);
        }
        Ok(Self {
            problem,
            output,
            next_batch: 0,
            next_frame: 0,
            completed_frames: 0,
        })
    }

    pub const fn output_shape(&self) -> OutputShape {
        self.problem.output_shape
    }

    pub fn output(&self) -> &[f32] {
        self.output
    }

    pub const fn completed_frames(&self) -> usize {
        self.completed_frames
    }

    pub const fn total_frames(&self) -> usize {
        self.problem.output_shape.batch * self.problem.output_shape.frames
    }

    pub const fn is_complete(&self) -> bool {
        self.next_batch == self.problem.batch
    }

    /// Inspect the next batch/frame pair without changing state.
    pub const fn next_frame(&self) -> Option<(usize, usize)> {
        if self.is_complete() {
            None
        } else {
            Some((self.next_batch, self.next_frame))
        }
    }

    /// Process up to `frame_budget` contiguous frames in the current batch.
    ///
    /// A range never crosses a batch boundary, which keeps the returned range
    /// directly usable as `[batch, start_frame..end_frame, 11, 2]`. A zero
    /// budget is rejected before output or the cooperative cursor changes.
    pub fn advance(&mut self, frame_budget: usize) -> Result<Advance, AdvanceError> {
        if frame_budget == 0 {
            return Err(AdvanceError::ZeroFrameBudget);
        }
        if self.is_complete() {
            return Ok(Advance::Complete);
        }

        let batch = self.next_batch;
        let start_frame = self.next_frame;
        let frames = self.problem.output_shape.frames;
        let end_frame = start_frame.saturating_add(frame_budget).min(frames);

        for frame in start_frame..end_frame {
            self.compute_frame(batch, frame);
        }

        let advanced = end_frame - start_frame;
        self.completed_frames += advanced;
        self.next_frame = end_frame;
        if self.next_frame == frames {
            self.next_batch += 1;
            self.next_frame = 0;
        }

        Ok(Advance::Advanced(CompletedRange {
            batch,
            start_frame,
            end_frame,
            completed_frames: self.completed_frames,
            total_frames: self.total_frames(),
        }))
    }

    /// Return the borrowed output after dropping cooperative state.
    pub fn into_output(self) -> &'output mut [f32] {
        self.output
    }

    fn compute_frame(&mut self, batch: usize, frame: usize) {
        let input_start = batch * self.problem.signal_length + frame * FRAME_STEP;
        let output_start =
            (batch * self.problem.output_shape.frames + frame) * OUTPUT_ELEMENTS_PER_FRAME;

        for bin in 0..OUTPUT_BINS {
            let mut real = 0.0_f32;
            let mut imaginary = 0.0_f32;
            for (sample, &window_bits) in HANN_WINDOW_BITS.iter().enumerate() {
                let windowed =
                    self.problem.input[input_start + sample] * f32::from_bits(window_bits);
                let root = (bin * sample) % FRAME_LENGTH;
                real = libm::fmaf(windowed, f32::from_bits(DFT_COS_BITS[root]), real);
                imaginary = libm::fmaf(windowed, f32::from_bits(DFT_NEG_SIN_BITS[root]), imaginary);
            }
            let bin_start = output_start + bin * OUTPUT_COMPONENTS;
            self.output[bin_start] = real;
            self.output[bin_start + 1] = imaginary;
        }
    }
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;
