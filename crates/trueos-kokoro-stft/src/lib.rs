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
//! range without allocation. It reproduces ONNX Runtime's fixed `N=20`,
//! `M=64` Bluestein transform with two 64-element complex scratch buffers.
//! The Hann window, chirp, radix-2 Vandermonde, and cached `FFT(b)` tables are
//! stored as audited f32 bits, so inference never evaluates trigonometric
//! functions or initializes a transform plan.

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
///
/// These direct-DFT roots remain public for artifact audits. Runtime execution
/// uses the ORT Bluestein tables below so its arithmetic order is bit exact.
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

const BLUESTEIN_LENGTH: usize = 64;

#[derive(Clone, Copy)]
#[repr(C)]
struct Complex {
    real: f32,
    imaginary: f32,
}

impl Complex {
    const ZERO: Self = Self { real: 0.0, imaginary: 0.0 };
    const ONE: Self = Self { real: 1.0, imaginary: 0.0 };

    const fn from_bits(bits: [u32; 2]) -> Self {
        Self {
            real: f32::from_bits(bits[0]),
            imaginary: f32::from_bits(bits[1]),
        }
    }

    #[inline(always)]
    fn add(self, rhs: Self) -> Self {
        Self {
            real: self.real + rhs.real,
            imaginary: self.imaginary + rhs.imaginary,
        }
    }

    #[inline(always)]
    fn multiply(self, rhs: Self) -> Self {
        // Preserve libstdc++'s std::complex<float> operation tree. In
        // particular, none of these multiply/add pairs may contract to FMA.
        let real_left = self.real * rhs.real;
        let real_right = self.imaginary * rhs.imaginary;
        let imaginary_left = self.real * rhs.imaginary;
        let imaginary_right = self.imaginary * rhs.real;
        Self {
            real: real_left - real_right,
            imaginary: imaginary_left + imaginary_right,
        }
    }

    #[inline(always)]
    fn multiply_scalar(self, rhs: f32) -> Self {
        Self {
            real: self.real * rhs,
            imaginary: self.imaginary * rhs,
        }
    }

    #[inline(always)]
    fn divide_scalar(self, rhs: f32) -> Self {
        Self {
            real: self.real / rhs,
            imaginary: self.imaginary / rhs,
        }
    }
}

// The three tables below are the exact bit output of the pinned ORT CPU
// implementation at transform-plan creation. ORT computes its float chirps
// with the host C++ sin/cos overloads, caches FFT(b), then reuses the same
// forward Vandermonde table for the inverse pass. Baking the results removes
// both host-libm variability and all plan setup from the inference path.
const CHIRP_BITS: [[u32; 2]; FRAME_LENGTH] = [
    [0x3f80_0000, 0x8000_0000], [0x3f7c_d925, 0xbe20_305c],
    [0x3f4f_1bbd, 0xbf16_7918], [0x3e20_305d, 0xbf7c_d925],
    [0xbf4f_1bbe, 0xbf16_7917], [0xbf35_04f1, 0x3f35_04f5],
    [0x3f4f_1bbc, 0x3f16_7919], [0x3e20_3049, 0xbf7c_d925],
    [0xbf4f_1bba, 0x3f16_791c], [0x3f7c_d924, 0xbe20_3077],
    [0xbf80_0000, 0x3535_563d], [0x3f7c_d91f, 0xbe20_30e1],
    [0xbf4f_1bc0, 0x3f16_7914], [0x3e20_3007, 0xbf7c_d928],
    [0x3f4f_1bc8, 0x3f16_7909], [0xbf35_04e7, 0x3f35_0500],
    [0xbf4f_1bc8, 0xbf16_7909], [0x3e20_2f07, 0xbf7c_d932],
    [0x3f4f_1bad, 0xbf16_792f], [0x3f7c_d91f, 0xbe20_30e7],
];

const VANDERMONDE_BITS: [[u32; 2]; BLUESTEIN_LENGTH] = [
    [0x3f80_0000, 0x8000_0000], [0xbf80_0000, 0x33bb_bd2e],
    [0xb33b_bd2e, 0xbf80_0000], [0x324c_de2e, 0x3f80_0000],
    [0x3f35_04f3, 0xbf35_04f3], [0xbf35_04f1, 0x3f35_04f5],
    [0xbf35_04f3, 0xbf35_04f3], [0x3f35_04f7, 0x3f35_04ef],
    [0x3f6c_835e, 0xbec3_ef16], [0xbf6c_835e, 0x3ec3_ef15],
    [0xbec3_ef18, 0xbf6c_835e], [0x3ec3_ef1b, 0x3f6c_835d],
    [0x3ec3_ef15, 0xbf6c_835e], [0xbec3_ef0b, 0x3f6c_8361],
    [0xbf6c_8360, 0xbec3_ef10], [0x3f6c_835f, 0x3ec3_ef15],
    [0x3f7b_14be, 0xbe47_c5c2], [0xbf7b_14be, 0x3e47_c5cd],
    [0xbe47_c5c2, 0xbf7b_14be], [0x3e47_c5c8, 0x3f7b_14be],
    [0x3f0e_39d9, 0xbf54_db32], [0xbf0e_39d6, 0x3f54_db34],
    [0xbf54_db32, 0xbf0e_39d9], [0x3f54_db31, 0x3f0e_39db],
    [0x3f54_db31, 0xbf0e_39da], [0xbf54_db30, 0x3f0e_39db],
    [0xbf0e_39dc, 0xbf54_db30], [0x3f0e_39dd, 0x3f54_db2f],
    [0x3e47_c5bc, 0xbf7b_14bf], [0xbe47_c5c6, 0x3f7b_14be],
    [0xbf7b_14bf, 0xbe47_c5c1], [0x3f7b_14bf, 0x3e47_c5bc],
    [0x3f7e_c46d, 0xbdc8_bd36], [0xbf7e_c46d, 0x3dc8_bd47],
    [0xbdc8_bd41, 0xbf7e_c46d], [0x3dc8_bd5d, 0x3f7e_c46d],
    [0x3f22_6799, 0xbf45_e403], [0xbf22_679a, 0x3f45_e403],
    [0xbf45_e404, 0xbf22_6799], [0x3f45_e405, 0x3f22_6797],
    [0x3f61_c597, 0xbef1_5aea], [0xbf61_c597, 0x3ef1_5aeb],
    [0xbef1_5aed, 0xbf61_c597], [0x3ef1_5ae9, 0x3f61_c598],
    [0x3e94_a030, 0xbf74_fa0b], [0xbe94_a02d, 0x3f74_fa0b],
    [0xbf74_fa0b, 0xbe94_a033], [0x3f74_fa0c, 0x3e94_a028],
    [0x3f74_fa0b, 0xbe94_a032], [0xbf74_fa0a, 0x3e94_a038],
    [0xbe94_a033, 0xbf74_fa0a], [0x3e94_a03d, 0x3f74_fa09],
    [0x3ef1_5ae7, 0xbf61_c598], [0xbef1_5ae8, 0x3f61_c598],
    [0xbf61_c599, 0xbef1_5ae6], [0x3f61_c599, 0x3ef1_5ae3],
    [0x3f45_e403, 0xbf22_679a], [0xbf45_e402, 0x3f22_679b],
    [0xbf22_6799, 0xbf45_e404], [0x3f22_679a, 0x3f45_e403],
    [0x3dc8_bd35, 0xbf7e_c46d], [0xbdc8_bd1a, 0x3f7e_c46d],
    [0xbf7e_c46d, 0xbdc8_bd30], [0x3f7e_c46e, 0x3dc8_bd04],
];

const B_FFT_BITS: [[u32; 2]; BLUESTEIN_LENGTH] = [
    [0x40aa_62a2, 0x40ca_62d8], [0x4016_bd8d, 0x401f_f914],
    [0x3fd6_72fc, 0xbc80_9880], [0x40bc_8d31, 0x408f_2f30],
    [0x4085_a63b, 0x4098_5355], [0x3f77_cc50, 0xbf82_2536],
    [0x40bb_21f7, 0xbf2d_61aa], [0x40f2_d3c0, 0x406a_8d4f],
    [0x3f7f_ffba, 0xb708_6d78], [0x3fdb_364e, 0xc0bf_aba4],
    [0x4102_ed9f, 0xc04a_38fc], [0x404a_0bd3, 0x3ecb_222a],
    [0xc0a4_1b40, 0xc099_e493], [0xbf4b_e2b8, 0xc10d_a072],
    [0x404f_3fb6, 0xc045_ca48], [0xc09d_ba35, 0x4023_8fc5],
    [0xc11f_1bbe, 0xb692_e1a0], [0xc04e_b345, 0xc018_4868],
    [0x400d_fa97, 0x4032_aeee], [0x3f5a_8008, 0x4108_f5fc],
    [0xbe8d_9da8, 0x40c5_f70e], [0x3fc4_a5b2, 0xbfc1_1797],
    [0x409f_4122, 0xc0b2_d3a1], [0x40d7_07d8, 0xc054_9cda],
    [0x3f80_001e, 0x3632_5dea], [0xc102_4111, 0xbe64_5378],
    [0xc108_914a, 0xbff6_a6f0], [0xbf16_62dd, 0xbf30_b4d0],
    [0x40a7_4ed3, 0x4051_18a1], [0x40c4_27d9, 0x40a9_267e],
    [0x4055_c3d4, 0x4010_c48a], [0xc04e_ae60, 0xc05d_2e8a],
    [0xc0ea_62ad, 0xc0ca_62d2], [0xc04e_ae5d, 0xc05d_2e8e],
    [0x4055_c3cb, 0x4010_c48a], [0x40c4_27d7, 0x40a9_2680],
    [0x40a7_4ed5, 0x4051_18aa], [0xbf16_62c4, 0xbf30_b4e4],
    [0xc108_9148, 0xbff6_a705], [0xc102_4112, 0xbe64_5280],
    [0x3f80_001e, 0x361b_bd4a], [0x40d7_07d9, 0xc054_9cd4],
    [0x409f_4121, 0xc0b2_d3a1], [0x3fc4_a5ae, 0xbfc1_178e],
    [0xbe8d_9d70, 0x40c5_f70c], [0x3f5a_8034, 0x4108_f5fc],
    [0x400d_fa94, 0x4032_aef2], [0xc04e_b348, 0xc018_4861],
    [0xc11f_1bbe, 0xb6a2_e1a0], [0xc09d_ba34, 0x4023_8fc7],
    [0x404f_3fb4, 0xc045_ca48], [0xbf4b_e2d6, 0xc10d_a074],
    [0xc0a4_1b46, 0xc099_e490], [0x404a_0bcc, 0x3ecb_222a],
    [0x4102_ed9d, 0xc04a_3902], [0x3fdb_3646, 0xc0bf_aba7],
    [0x3f7f_ffae, 0xb70e_159e], [0x40f2_d3bc, 0x406a_8d50],
    [0x40bb_21f8, 0xbf2d_61a8], [0x3f77_cc34, 0xbf82_254a],
    [0x4085_a63c, 0x4098_5353], [0x40bc_8d32, 0x408f_2f2e],
    [0x3fd6_72fb, 0xbc80_9a40], [0x4016_bd92, 0x401f_f909],
];

#[inline]
fn reverse_bits(value: usize, significant_bits: u32) -> usize {
    value.reverse_bits() >> (usize::BITS - significant_bits)
}

fn fft_radix2(
    input: &[Complex; BLUESTEIN_LENGTH],
    output: &mut [Complex; BLUESTEIN_LENGTH],
    inverse: bool,
) {
    const SIGNIFICANT_BITS: u32 = BLUESTEIN_LENGTH.ilog2();

    for index in 0..BLUESTEIN_LENGTH {
        let reversed = reverse_bits(index, SIGNIFICANT_BITS);
        // ORT spells this as complex(1, 0) * x * window_element, with the
        // absent window represented by complex(1, 0). Retain both products;
        // simplifying them changes signed-zero behavior.
        output[index] = Complex::ONE
            .multiply(input[reversed])
            .multiply(Complex::ONE);
    }

    let mut width = 2usize;
    let mut current_bits = 0u32;
    while width <= BLUESTEIN_LENGTH {
        let midpoint = width >> 1;
        current_bits += 1;
        for k in 0..midpoint {
            let first_index = reverse_bits(k, current_bits);
            let second_index = reverse_bits(midpoint + k, current_bits);
            let first_root = Complex::from_bits(VANDERMONDE_BITS[first_index]);
            let second_root = Complex::from_bits(VANDERMONDE_BITS[second_index]);
            let mut j = 0usize;
            while j < BLUESTEIN_LENGTH {
                let even_index = k + j;
                let odd_index = even_index + midpoint;
                let even = output[even_index];
                let odd = output[odd_index];
                let first = even.add(first_root.multiply(odd));
                let second = even.add(second_root.multiply(odd));
                output[even_index] = first;
                output[odd_index] = second;
                j += width;
            }
        }
        width <<= 1;
    }

    if inverse {
        for value in output {
            *value = value.divide_scalar(BLUESTEIN_LENGTH as f32);
        }
    }
}

fn ort_bluestein_frame(input: &[f32], output: &mut [f32]) {
    debug_assert_eq!(input.len(), FRAME_LENGTH);
    debug_assert_eq!(output.len(), OUTPUT_ELEMENTS_PER_FRAME);

    let mut a = [Complex::ZERO; BLUESTEIN_LENGTH];
    let mut a_fft = [Complex::ZERO; BLUESTEIN_LENGTH];
    for sample in 0..FRAME_LENGTH {
        let signal = Complex {
            real: input[sample],
            imaginary: 0.0,
        };
        let windowed = signal.multiply_scalar(f32::from_bits(HANN_WINDOW_BITS[sample]));
        a[sample] = windowed.multiply(Complex::from_bits(CHIRP_BITS[sample]));
    }

    fft_radix2(&a, &mut a_fft, false);
    for index in 0..BLUESTEIN_LENGTH {
        a_fft[index] = a_fft[index].multiply(Complex::from_bits(B_FFT_BITS[index]));
    }
    // ORT deliberately reuses its cached forward Vandermonde for this inverse
    // pass, divides by 64, and compensates by reading convolution index 64-k.
    fft_radix2(&a_fft, &mut a, true);

    for bin in 0..OUTPUT_BINS {
        let convolution = if bin == 0 { a[0] } else { a[BLUESTEIN_LENGTH - bin] };
        let result = convolution
            .multiply(Complex::from_bits(CHIRP_BITS[bin]))
            .multiply_scalar(1.0);
        output[bin * OUTPUT_COMPONENTS] = result.real;
        output[bin * OUTPUT_COMPONENTS + 1] = result.imaginary;
    }
}

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
        let input = &self.problem.input[input_start..input_start + FRAME_LENGTH];
        let output =
            &mut self.output[output_start..output_start + OUTPUT_ELEMENTS_PER_FRAME];
        ort_bluestein_frame(input, output);
    }
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;
