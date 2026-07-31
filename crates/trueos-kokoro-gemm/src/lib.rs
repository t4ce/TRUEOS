#![cfg_attr(not(test), no_std)]
#![deny(unsafe_op_in_unsafe_fn)]

//! Sealed f32 MatMul kernels for the 27 remaining Kokoro graph nodes.
//!
//! Inputs and outputs are contiguous row-major matrices. Leading ONNX batch
//! dimensions are flattened into `batches`. The deterministic loop contract is
//! batch, row, eight-column tile, K in ascending order. Both lanes use fused
//! multiply-add in that K order; the AVX2+FMA lane only computes eight output
//! columns in parallel and never changes a dot product's reduction order.

pub const PINNED_NODE_COVERAGE: usize = 27;
pub const ATTENTION_HEADS: usize = 12;
pub const ATTENTION_HEAD_WIDTH: usize = 64;
pub const MAX_SEQUENCE_LENGTH: usize = 512;
pub const SOURCE_SAMPLES_PER_FRAME: usize = 600;
pub const N_TILE: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    EmptyDimension,
    SequenceTooLong,
    UnsupportedShape,
    ShapeOverflow,
    LhsTooSmall,
    RhsTooSmall,
    OutputTooSmall,
    Aliasing,
    NonFiniteInput,
    NonFiniteOutputRisk,
    UnsupportedLane,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DurationChannels {
    Prosody640,
    Text512,
}

impl DurationChannels {
    pub const fn count(self) -> usize {
        match self {
            Self::Prosody640 => 640,
            Self::Text512 => 512,
        }
    }
}

/// The five shape families admitted from the pinned Kokoro graph.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KokoroMatMul {
    /// Twelve `[S, 64] x [64, S]` attention-score products.
    AttentionScores { sequence: usize },
    /// Twelve `[S, S] x [S, 64]` attention-context products.
    AttentionContext { sequence: usize },
    /// One `[C, S] x [S, T]` duration expansion for each admitted C.
    DurationProjection {
        channels: DurationChannels,
        sequence: usize,
        frames: usize,
    },
    /// Generator source projection `[samples, 9] x [9, 1]`.
    SourceLinear { samples: usize },
}

impl KokoroMatMul {
    pub fn dimensions(self) -> Result<Dimensions, Error> {
        match self {
            Self::AttentionScores { sequence } => {
                validate_sequence(sequence)?;
                Ok(Dimensions {
                    batches: ATTENTION_HEADS,
                    m: sequence,
                    k: ATTENTION_HEAD_WIDTH,
                    n: sequence,
                })
            }
            Self::AttentionContext { sequence } => {
                validate_sequence(sequence)?;
                Ok(Dimensions {
                    batches: ATTENTION_HEADS,
                    m: sequence,
                    k: sequence,
                    n: ATTENTION_HEAD_WIDTH,
                })
            }
            Self::DurationProjection {
                channels,
                sequence,
                frames,
            } => {
                validate_sequence(sequence)?;
                if frames == 0 {
                    return Err(Error::EmptyDimension);
                }
                Ok(Dimensions {
                    batches: 1,
                    m: channels.count(),
                    k: sequence,
                    n: frames,
                })
            }
            Self::SourceLinear { samples } => {
                if samples == 0 {
                    return Err(Error::EmptyDimension);
                }
                if !samples.is_multiple_of(SOURCE_SAMPLES_PER_FRAME) {
                    return Err(Error::UnsupportedShape);
                }
                Ok(Dimensions {
                    batches: 1,
                    m: samples,
                    k: 9,
                    n: 1,
                })
            }
        }
    }

    pub const fn graph_nodes(self) -> usize {
        match self {
            Self::AttentionScores { .. } | Self::AttentionContext { .. } => 12,
            Self::DurationProjection { .. } | Self::SourceLinear { .. } => 1,
        }
    }
}

fn validate_sequence(sequence: usize) -> Result<(), Error> {
    if sequence == 0 {
        Err(Error::EmptyDimension)
    } else if sequence > MAX_SEQUENCE_LENGTH {
        Err(Error::SequenceTooLong)
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Dimensions {
    pub batches: usize,
    pub m: usize,
    pub k: usize,
    pub n: usize,
}

impl Dimensions {
    pub fn lhs_elements(self) -> Result<usize, Error> {
        self.batches
            .checked_mul(self.m)
            .and_then(|value| value.checked_mul(self.k))
            .ok_or(Error::ShapeOverflow)
    }

    pub fn rhs_elements(self) -> Result<usize, Error> {
        self.batches
            .checked_mul(self.k)
            .and_then(|value| value.checked_mul(self.n))
            .ok_or(Error::ShapeOverflow)
    }

    pub fn output_elements(self) -> Result<usize, Error> {
        self.batches
            .checked_mul(self.m)
            .and_then(|value| value.checked_mul(self.n))
            .ok_or(Error::ShapeOverflow)
    }

    pub fn scalar_operations(self) -> Result<usize, Error> {
        self.output_elements()?
            .checked_mul(self.k)
            .and_then(|value| value.checked_mul(2))
            .ok_or(Error::ShapeOverflow)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Lane {
    Scalar,
    Avx2Fma,
}

impl Lane {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Scalar => "scalar-fma",
            Self::Avx2Fma => "avx2-fma-256",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CpuCapabilities {
    ymm_state: bool,
    avx2: bool,
    fma: bool,
}

impl CpuCapabilities {
    pub const fn ymm_state(self) -> bool {
        self.ymm_state
    }

    pub const fn avx2(self) -> bool {
        self.avx2
    }

    pub const fn fma(self) -> bool {
        self.fma
    }

    pub const fn supports(self, lane: Lane) -> bool {
        match lane {
            Lane::Scalar => true,
            Lane::Avx2Fma => self.ymm_state && self.avx2 && self.fma,
        }
    }

    pub const fn best_lane(self) -> Lane {
        if self.supports(Lane::Avx2Fma) {
            Lane::Avx2Fma
        } else {
            Lane::Scalar
        }
    }
}

/// Per-current-CPU runtime dispatcher. Detection is intentionally not cached.
#[derive(Clone, Copy, Debug)]
pub struct Dispatcher {
    capabilities: CpuCapabilities,
}

impl Dispatcher {
    /// Probe AVX, OSXSAVE/XCR0, AVX2, and FMA for the current CPU.
    pub fn detect() -> Self {
        Self {
            capabilities: detect_cpu_capabilities(),
        }
    }

    pub const fn capabilities(self) -> CpuCapabilities {
        self.capabilities
    }

    pub const fn best_lane(self) -> Lane {
        self.capabilities.best_lane()
    }

    pub const fn supports(self, lane: Lane) -> bool {
        self.capabilities.supports(lane)
    }

    pub fn matmul(
        self,
        profile: KokoroMatMul,
        lhs: &[f32],
        rhs: &[f32],
        output: &mut [f32],
    ) -> Result<Lane, Error> {
        let lane = self.best_lane();
        self.matmul_with_lane(profile, lhs, rhs, output, lane)?;
        Ok(lane)
    }

    pub fn matmul_with_lane(
        self,
        profile: KokoroMatMul,
        lhs: &[f32],
        rhs: &[f32],
        output: &mut [f32],
        lane: Lane,
    ) -> Result<(), Error> {
        if !self.supports(lane) {
            return Err(Error::UnsupportedLane);
        }
        let dimensions = profile.dimensions()?;
        validate_buffers(dimensions, lhs, rhs, output)?;
        match lane {
            Lane::Scalar => matmul_scalar(dimensions, lhs, rhs, output),
            Lane::Avx2Fma => {
                #[cfg(target_arch = "x86_64")]
                unsafe {
                    matmul_avx2_fma(dimensions, lhs, rhs, output);
                }
                #[cfg(not(target_arch = "x86_64"))]
                return Err(Error::UnsupportedLane);
            }
        }
        Ok(())
    }
}

fn validate_buffers(
    dimensions: Dimensions,
    lhs: &[f32],
    rhs: &[f32],
    output: &[f32],
) -> Result<(), Error> {
    let lhs_elements = dimensions.lhs_elements()?;
    let rhs_elements = dimensions.rhs_elements()?;
    let output_elements = dimensions.output_elements()?;
    if lhs.len() < lhs_elements {
        return Err(Error::LhsTooSmall);
    }
    if rhs.len() < rhs_elements {
        return Err(Error::RhsTooSmall);
    }
    if output.len() < output_elements {
        return Err(Error::OutputTooSmall);
    }
    if memory_ranges_overlap(output.as_ptr(), output.len(), lhs.as_ptr(), lhs.len())
        || memory_ranges_overlap(output.as_ptr(), output.len(), rhs.as_ptr(), rhs.len())
    {
        return Err(Error::Aliasing);
    }

    let mut lhs_maximum = 0.0f32;
    for &value in &lhs[..lhs_elements] {
        if !value.is_finite() {
            return Err(Error::NonFiniteInput);
        }
        lhs_maximum = lhs_maximum.max(value.abs());
    }
    let mut rhs_maximum = 0.0f32;
    for &value in &rhs[..rhs_elements] {
        if !value.is_finite() {
            return Err(Error::NonFiniteInput);
        }
        rhs_maximum = rhs_maximum.max(value.abs());
    }
    let magnitude_bound = lhs_maximum as f64 * rhs_maximum as f64 * dimensions.k as f64;
    if !magnitude_bound.is_finite() || magnitude_bound > f32::MAX as f64 {
        return Err(Error::NonFiniteOutputRisk);
    }
    Ok(())
}

fn matmul_scalar(dimensions: Dimensions, lhs: &[f32], rhs: &[f32], output: &mut [f32]) {
    for batch in 0..dimensions.batches {
        let lhs_batch = batch * dimensions.m * dimensions.k;
        let rhs_batch = batch * dimensions.k * dimensions.n;
        let output_batch = batch * dimensions.m * dimensions.n;
        for row in 0..dimensions.m {
            let lhs_row = lhs_batch + row * dimensions.k;
            let output_row = output_batch + row * dimensions.n;
            for column in 0..dimensions.n {
                output[output_row + column] =
                    dot_fused(lhs, lhs_row, rhs, rhs_batch + column, dimensions.k, dimensions.n);
            }
        }
    }
}

#[inline]
fn dot_fused(
    lhs: &[f32],
    lhs_start: usize,
    rhs: &[f32],
    rhs_start: usize,
    k: usize,
    rhs_stride: usize,
) -> f32 {
    let mut accumulator = 0.0f32;
    for reduction in 0..k {
        accumulator = libm::fmaf(
            lhs[lhs_start + reduction],
            rhs[rhs_start + reduction * rhs_stride],
            accumulator,
        );
    }
    accumulator
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn matmul_avx2_fma(dimensions: Dimensions, lhs: &[f32], rhs: &[f32], output: &mut [f32]) {
    use core::arch::x86_64::{
        _mm256_fmadd_ps, _mm256_i32gather_ps, _mm256_loadu_ps, _mm256_set_epi32, _mm256_set1_ps,
        _mm256_setzero_ps, _mm256_storeu_ps, _mm256_zeroupper,
    };

    for batch in 0..dimensions.batches {
        let lhs_batch = batch * dimensions.m * dimensions.k;
        let rhs_batch = batch * dimensions.k * dimensions.n;
        let output_batch = batch * dimensions.m * dimensions.n;
        if dimensions.n == 1 {
            let k = dimensions.k as i32;
            let row_offsets = _mm256_set_epi32(7 * k, 6 * k, 5 * k, 4 * k, 3 * k, 2 * k, k, 0);
            let mut row = 0usize;
            while row + N_TILE <= dimensions.m {
                let mut accumulator = _mm256_setzero_ps();
                for reduction in 0..dimensions.k {
                    let lhs_values = unsafe {
                        _mm256_i32gather_ps::<4>(
                            lhs.as_ptr().add(lhs_batch + row * dimensions.k + reduction),
                            row_offsets,
                        )
                    };
                    let rhs_value = _mm256_set1_ps(rhs[rhs_batch + reduction]);
                    accumulator = _mm256_fmadd_ps(lhs_values, rhs_value, accumulator);
                }
                unsafe {
                    _mm256_storeu_ps(output.as_mut_ptr().add(output_batch + row), accumulator);
                }
                row += N_TILE;
            }
            while row < dimensions.m {
                output[output_batch + row] =
                    dot_fused(lhs, lhs_batch + row * dimensions.k, rhs, rhs_batch, dimensions.k, 1);
                row += 1;
            }
            continue;
        }
        for row in 0..dimensions.m {
            let lhs_row = lhs_batch + row * dimensions.k;
            let output_row = output_batch + row * dimensions.n;
            let mut column = 0usize;
            while column + N_TILE <= dimensions.n {
                let mut accumulator = _mm256_setzero_ps();
                for reduction in 0..dimensions.k {
                    let lhs_value = _mm256_set1_ps(lhs[lhs_row + reduction]);
                    let rhs_values = unsafe {
                        _mm256_loadu_ps(
                            rhs.as_ptr()
                                .add(rhs_batch + reduction * dimensions.n + column),
                        )
                    };
                    accumulator = _mm256_fmadd_ps(lhs_value, rhs_values, accumulator);
                }
                unsafe {
                    _mm256_storeu_ps(output.as_mut_ptr().add(output_row + column), accumulator);
                }
                column += N_TILE;
            }
            while column < dimensions.n {
                output[output_row + column] =
                    dot_fused(lhs, lhs_row, rhs, rhs_batch + column, dimensions.k, dimensions.n);
                column += 1;
            }
        }
    }
    _mm256_zeroupper();
}

#[cfg(target_arch = "x86_64")]
fn detect_cpu_capabilities() -> CpuCapabilities {
    use core::arch::x86_64::{__cpuid, __cpuid_count};

    const CPUID_1_ECX_FMA: u32 = 1 << 12;
    const CPUID_1_ECX_OSXSAVE: u32 = 1 << 27;
    const CPUID_1_ECX_AVX: u32 = 1 << 28;
    const CPUID_7_EBX_AVX2: u32 = 1 << 5;
    const XCR0_XMM_YMM: u64 = (1 << 1) | (1 << 2);

    let maximum_leaf = __cpuid(0).eax;
    if maximum_leaf < 1 {
        return CpuCapabilities::default();
    }
    let leaf_1 = __cpuid(1);
    let avx_state_contract = CPUID_1_ECX_OSXSAVE | CPUID_1_ECX_AVX;
    if leaf_1.ecx & avx_state_contract != avx_state_contract {
        return CpuCapabilities::default();
    }
    let xcr0 = unsafe { read_xcr0() };
    if xcr0 & XCR0_XMM_YMM != XCR0_XMM_YMM || maximum_leaf < 7 {
        return CpuCapabilities::default();
    }
    let leaf_7 = __cpuid_count(7, 0);
    CpuCapabilities {
        ymm_state: true,
        avx2: leaf_7.ebx & CPUID_7_EBX_AVX2 != 0,
        fma: leaf_1.ecx & CPUID_1_ECX_FMA != 0,
    }
}

#[cfg(not(target_arch = "x86_64"))]
const fn detect_cpu_capabilities() -> CpuCapabilities {
    CpuCapabilities {
        ymm_state: false,
        avx2: false,
        fma: false,
    }
}

#[cfg(target_arch = "x86_64")]
#[inline]
unsafe fn read_xcr0() -> u64 {
    let low: u32;
    let high: u32;
    unsafe {
        core::arch::asm!(
            "xgetbv",
            in("ecx") 0u32,
            out("eax") low,
            out("edx") high,
            options(nostack, preserves_flags),
        );
    }
    (u64::from(high) << 32) | u64::from(low)
}

fn memory_ranges_overlap(lhs: *const f32, lhs_len: usize, rhs: *const f32, rhs_len: usize) -> bool {
    if lhs_len == 0 || rhs_len == 0 {
        return false;
    }
    let bytes = core::mem::size_of::<f32>();
    let lhs_start = lhs as usize;
    let rhs_start = rhs as usize;
    let lhs_end = lhs_start.saturating_add(lhs_len.saturating_mul(bytes));
    let rhs_end = rhs_start.saturating_add(rhs_len.saturating_mul(bytes));
    lhs_start < rhs_end && rhs_start < lhs_end
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec;
    use std::vec::Vec;

    // Fixtures were generated by ONNX Runtime 1.28.0 with graph
    // optimizations disabled, sequential CPU execution, and ONNX opset 20.

    fn pattern(
        length: usize,
        multiplier: usize,
        modulus: usize,
        center: i32,
        divisor: f32,
    ) -> Vec<f32> {
        (0..length)
            .map(|index| {
                let integer = ((index * multiplier) % modulus) as i32 - center;
                integer as f32 / divisor
            })
            .collect()
    }

    fn run_ort_fixture(profile: KokoroMatMul, indices: &[usize], expected: &[u32]) {
        let dimensions = profile.dimensions().unwrap();
        let lhs = pattern(dimensions.lhs_elements().unwrap(), 17, 31, 15, 64.0);
        let rhs = pattern(dimensions.rhs_elements().unwrap(), 13, 29, 14, 32.0);
        let mut scalar = vec![0.0f32; dimensions.output_elements().unwrap()];
        let dispatcher = Dispatcher::detect();
        dispatcher
            .matmul_with_lane(profile, &lhs, &rhs, &mut scalar, Lane::Scalar)
            .unwrap();
        assert_eq!(indices.len(), expected.len());
        for (&index, &bits) in indices.iter().zip(expected) {
            assert_eq!(scalar[index].to_bits(), bits, "fixture index {index}");
        }

        if dispatcher.supports(Lane::Avx2Fma) {
            let mut vector = vec![0.0f32; scalar.len()];
            dispatcher
                .matmul_with_lane(profile, &lhs, &rhs, &mut vector, Lane::Avx2Fma)
                .unwrap();
            assert_eq!(
                vector
                    .iter()
                    .map(|value| value.to_bits())
                    .collect::<Vec<_>>(),
                scalar
                    .iter()
                    .map(|value| value.to_bits())
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn profile_dimensions_cover_exactly_twenty_seven_nodes() {
        let scores = KokoroMatMul::AttentionScores { sequence: 18 };
        let context = KokoroMatMul::AttentionContext { sequence: 18 };
        let prosody = KokoroMatMul::DurationProjection {
            channels: DurationChannels::Prosody640,
            sequence: 18,
            frames: 69,
        };
        let text = KokoroMatMul::DurationProjection {
            channels: DurationChannels::Text512,
            sequence: 18,
            frames: 69,
        };
        let source = KokoroMatMul::SourceLinear { samples: 41_400 };
        assert_eq!(
            scores.graph_nodes()
                + context.graph_nodes()
                + prosody.graph_nodes()
                + text.graph_nodes()
                + source.graph_nodes(),
            PINNED_NODE_COVERAGE
        );
        assert_eq!(
            scores.dimensions().unwrap(),
            Dimensions {
                batches: 12,
                m: 18,
                k: 64,
                n: 18,
            }
        );
        assert_eq!(
            context.dimensions().unwrap(),
            Dimensions {
                batches: 12,
                m: 18,
                k: 18,
                n: 64,
            }
        );
        assert_eq!(
            prosody.dimensions().unwrap(),
            Dimensions {
                batches: 1,
                m: 640,
                k: 18,
                n: 69,
            }
        );
        assert_eq!(
            text.dimensions().unwrap(),
            Dimensions {
                batches: 1,
                m: 512,
                k: 18,
                n: 69,
            }
        );
        assert_eq!(
            source.dimensions().unwrap(),
            Dimensions {
                batches: 1,
                m: 41_400,
                k: 9,
                n: 1,
            }
        );
    }

    #[test]
    fn representative_real_shapes_match_ort_exact_fixtures() {
        run_ort_fixture(
            KokoroMatMul::AttentionScores { sequence: 18 },
            &[0, 1, 17, 18, 323, 324, 1_944, 3_887],
            &[
                0xBA00_0000,
                0x3E30_0000,
                0xBEA0_4000,
                0x3E07_8000,
                0xBD30_0000,
                0xBE00_8000,
                0xBE11_8000,
                0x3E0C_8000,
            ],
        );
        run_ort_fixture(
            KokoroMatMul::AttentionContext { sequence: 18 },
            &[0, 1, 63, 64, 1_151, 1_152, 6_912, 13_823],
            &[
                0xBD83_0000,
                0x3D78_0000,
                0xBCB4_0000,
                0xBE88_4000,
                0xBD54_0000,
                0x3D20_0000,
                0xBC60_0000,
                0x3CE4_0000,
            ],
        );
        run_ort_fixture(
            KokoroMatMul::DurationProjection {
                channels: DurationChannels::Prosody640,
                sequence: 18,
                frames: 69,
            },
            &[0, 1, 68, 69, 22_079, 22_080, 44_159],
            &[
                0x3D8C_0000,
                0x3E0B_8000,
                0xBD02_0000,
                0xBE01_0000,
                0x3DC9_0000,
                0xBCE8_0000,
                0xBCE8_0000,
            ],
        );
        run_ort_fixture(
            KokoroMatMul::DurationProjection {
                channels: DurationChannels::Text512,
                sequence: 18,
                frames: 69,
            },
            &[0, 1, 68, 69, 17_663, 17_664, 35_327],
            &[
                0x3D8C_0000,
                0x3E0B_8000,
                0xBD02_0000,
                0xBE01_0000,
                0xBE63_0000,
                0xBE1E_0000,
                0xBE5B_0000,
            ],
        );
        run_ort_fixture(
            KokoroMatMul::SourceLinear { samples: 41_400 },
            &[0, 1, 20_699, 20_700, 41_399],
            &[
                0xBDF8_0000,
                0xBEA7_8000,
                0x3E23_0000,
                0x3E29_0000,
                0xBE05_0000,
            ],
        );
    }

    #[test]
    fn avx2_fma_preserves_scalar_fma_order_for_nonexact_values() {
        let dispatcher = Dispatcher::detect();
        if !dispatcher.supports(Lane::Avx2Fma) {
            return;
        }
        let profile = KokoroMatMul::AttentionScores { sequence: 19 };
        let dimensions = profile.dimensions().unwrap();
        let lhs = pattern(dimensions.lhs_elements().unwrap(), 7, 37, 18, 10.0);
        let rhs = pattern(dimensions.rhs_elements().unwrap(), 11, 41, 20, 10.0);
        let mut scalar = vec![0.0; dimensions.output_elements().unwrap()];
        let mut vector = vec![0.0; scalar.len()];
        dispatcher
            .matmul_with_lane(profile, &lhs, &rhs, &mut scalar, Lane::Scalar)
            .unwrap();
        dispatcher
            .matmul_with_lane(profile, &lhs, &rhs, &mut vector, Lane::Avx2Fma)
            .unwrap();
        assert_eq!(
            vector
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>(),
            scalar
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn validation_is_fail_closed_and_transactional() {
        assert_eq!(
            KokoroMatMul::AttentionScores { sequence: 0 }.dimensions(),
            Err(Error::EmptyDimension)
        );
        assert_eq!(
            KokoroMatMul::AttentionContext { sequence: 513 }.dimensions(),
            Err(Error::SequenceTooLong)
        );
        assert_eq!(
            KokoroMatMul::SourceLinear { samples: 599 }.dimensions(),
            Err(Error::UnsupportedShape)
        );

        let profile = KokoroMatMul::AttentionScores { sequence: 1 };
        let dimensions = profile.dimensions().unwrap();
        let mut lhs = vec![1.0; dimensions.lhs_elements().unwrap()];
        let rhs = vec![1.0; dimensions.rhs_elements().unwrap()];
        let mut output = vec![123.0; dimensions.output_elements().unwrap()];
        lhs[17] = f32::NAN;
        assert_eq!(
            Dispatcher::detect().matmul_with_lane(profile, &lhs, &rhs, &mut output, Lane::Scalar,),
            Err(Error::NonFiniteInput)
        );
        assert!(output.iter().all(|&value| value == 123.0));

        lhs.fill(f32::MAX);
        assert_eq!(
            Dispatcher::detect().matmul_with_lane(profile, &lhs, &rhs, &mut output, Lane::Scalar,),
            Err(Error::NonFiniteOutputRisk)
        );
        assert!(output.iter().all(|&value| value == 123.0));
        assert_eq!(
            Dispatcher::detect().matmul_with_lane(
                profile,
                &lhs[..lhs.len() - 1],
                &rhs,
                &mut output,
                Lane::Scalar,
            ),
            Err(Error::LhsTooSmall)
        );
    }

    #[test]
    fn runtime_capability_contract_never_admits_partial_avx_state() {
        let dispatcher = Dispatcher::detect();
        let capabilities = dispatcher.capabilities();
        if capabilities.supports(Lane::Avx2Fma) {
            assert!(capabilities.ymm_state());
            assert!(capabilities.avx2());
            assert!(capabilities.fma());
        }
        assert_eq!(dispatcher.best_lane(), capabilities.best_lane());

        let scalar_only = Dispatcher {
            capabilities: CpuCapabilities::default(),
        };
        let profile = KokoroMatMul::AttentionScores { sequence: 1 };
        let dimensions = profile.dimensions().unwrap();
        assert_eq!(
            scalar_only.matmul_with_lane(
                profile,
                &vec![0.0; dimensions.lhs_elements().unwrap()],
                &vec![0.0; dimensions.rhs_elements().unwrap()],
                &mut vec![0.0; dimensions.output_elements().unwrap()],
                Lane::Avx2Fma,
            ),
            Err(Error::UnsupportedLane)
        );
    }
}
