#![cfg_attr(not(test), no_std)]
#![deny(unsafe_op_in_unsafe_fn)]

//! Integer CPU primitives for the Kokoro inference path.
//!
//! The model's dynamically quantized projections multiply unsigned activations
//! by signed weights. This crate keeps that contract explicit and implements
//! three bit-identical lanes:
//!
//! - scalar, available everywhere;
//! - 256-bit AVX2, using widening multiplies rather than saturating
//!   `vpmaddubsw`;
//! - 256-bit AVX-VNNI, using `vpdpbusd`.
//!
//! [`Dispatcher::detect`] probes the current CPU and the OS-owned XMM/YMM state.
//! It intentionally does not cache the result globally, so a bare-metal caller
//! can construct one after enabling XCR0 on each worker CPU. AVX-512 state is
//! neither required nor used.

/// A CPU implementation of the unsigned-by-signed integer dot product.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Lane {
    Scalar,
    Avx2,
    AvxVnni,
}

impl Lane {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Scalar => "scalar",
            Self::Avx2 => "avx2",
            Self::AvxVnni => "avx-vnni",
        }
    }
}

/// SIMD state and instruction support observed on the current CPU.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CpuCapabilities {
    ymm_state: bool,
    avx2: bool,
    avx_vnni: bool,
}

impl CpuCapabilities {
    pub const fn ymm_state(self) -> bool {
        self.ymm_state
    }

    pub const fn avx2(self) -> bool {
        self.avx2
    }

    pub const fn avx_vnni(self) -> bool {
        self.avx_vnni
    }

    pub const fn supports(self, lane: Lane) -> bool {
        match lane {
            Lane::Scalar => true,
            Lane::Avx2 => self.ymm_state && self.avx2,
            Lane::AvxVnni => self.ymm_state && self.avx2 && self.avx_vnni,
        }
    }

    pub const fn best_lane(self) -> Lane {
        if self.supports(Lane::AvxVnni) {
            Lane::AvxVnni
        } else if self.supports(Lane::Avx2) {
            Lane::Avx2
        } else {
            Lane::Scalar
        }
    }
}

/// Errors returned before a kernel touches its input or output buffers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    EmptyReduction,
    EmptyMatrix,
    LengthMismatch,
    ShapeOverflow,
    LhsTooSmall,
    RhsTooSmall,
    OutputTooSmall,
    ZeroPointsTooSmall,
    RowSumsTooSmall,
    ScalesTooSmall,
    BiasTooSmall,
    UnsupportedLane,
}

/// A scalar or per-output-channel signed weight zero point.
#[derive(Clone, Copy, Debug)]
pub enum RhsZeroPoints<'a> {
    Scalar(i8),
    PerOutput(&'a [i8]),
}

impl RhsZeroPoints<'_> {
    #[inline]
    fn validate(self, outputs: usize) -> Result<(), Error> {
        match self {
            Self::Scalar(_) => Ok(()),
            Self::PerOutput(values) if values.len() >= outputs => Ok(()),
            Self::PerOutput(_) => Err(Error::ZeroPointsTooSmall),
        }
    }

    #[inline]
    fn get(self, output: usize) -> i8 {
        match self {
            Self::Scalar(value) => value,
            Self::PerOutput(values) => values[output],
        }
    }
}

/// Row-major quantized matrix multiplication parameters.
///
/// `lhs` has shape `[m, k]`. `rhs_transposed` has the CPU-native shape
/// `[n, k]`, so each output channel is contiguous. An ONNX `[k, n]` weight
/// should be transposed during the model's offline packing step. `output` has
/// shape `[m, n]` and contains the exact `i32` MatMulInteger accumulators.
///
/// `rhs_row_sums`, when present, holds the raw signed sum of each `[k]` weight
/// row. Supplying these offline-computed sums removes zero-point bookkeeping
/// from the hot dot-product loop.
#[derive(Clone, Copy, Debug)]
pub struct QGemmParams<'a> {
    pub m: usize,
    pub n: usize,
    pub k: usize,
    pub lhs_zero_point: u8,
    pub rhs_zero_points: RhsZeroPoints<'a>,
    pub rhs_row_sums: Option<&'a [i32]>,
}

impl QGemmParams<'_> {
    fn validate(self, lhs: &[u8], rhs: &[i8], output: &[i32]) -> Result<(), Error> {
        if self.m == 0 || self.n == 0 {
            return Err(Error::EmptyMatrix);
        }
        if self.k == 0 {
            return Err(Error::EmptyReduction);
        }
        let lhs_len = self.m.checked_mul(self.k).ok_or(Error::ShapeOverflow)?;
        let rhs_len = self.n.checked_mul(self.k).ok_or(Error::ShapeOverflow)?;
        let output_len = self.m.checked_mul(self.n).ok_or(Error::ShapeOverflow)?;
        if lhs.len() < lhs_len {
            return Err(Error::LhsTooSmall);
        }
        if rhs.len() < rhs_len {
            return Err(Error::RhsTooSmall);
        }
        if output.len() < output_len {
            return Err(Error::OutputTooSmall);
        }
        self.rhs_zero_points.validate(self.n)?;
        if self
            .rhs_row_sums
            .is_some_and(|row_sums| row_sums.len() < self.n)
        {
            return Err(Error::RowSumsTooSmall);
        }
        Ok(())
    }
}

/// A per-current-CPU runtime dispatcher.
#[derive(Clone, Copy, Debug)]
pub struct Dispatcher {
    capabilities: CpuCapabilities,
}

impl Dispatcher {
    /// Probe CPUID and XCR0 on the current CPU.
    ///
    /// AVX2 or AVX-VNNI is admitted only when the processor advertises AVX and
    /// OSXSAVE and XCR0 currently enables both XMM and YMM state. This prevents
    /// safe entry points from executing a VEX instruction before TRUEOS has
    /// established its extended-state contract.
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

    /// Compute one `(u8 - lhs_zero_point) * (i8 - rhs_zero_point)` dot product.
    pub fn dot(
        self,
        lhs: &[u8],
        rhs: &[i8],
        lhs_zero_point: u8,
        rhs_zero_point: i8,
    ) -> Result<(i32, Lane), Error> {
        let lane = self.best_lane();
        let value = self.dot_with_lane(lhs, rhs, lhs_zero_point, rhs_zero_point, lane)?;
        Ok((value, lane))
    }

    /// Compute one dot product on an explicitly selected, runtime-checked lane.
    pub fn dot_with_lane(
        self,
        lhs: &[u8],
        rhs: &[i8],
        lhs_zero_point: u8,
        rhs_zero_point: i8,
        lane: Lane,
    ) -> Result<i32, Error> {
        validate_dot(lhs, rhs)?;
        if !self.supports(lane) {
            return Err(Error::UnsupportedLane);
        }
        let (raw, lhs_sum, rhs_sum) = raw_dot_and_sums(lhs, rhs, lane);
        Ok(apply_zero_points(raw, lhs_sum, rhs_sum, lhs.len(), lhs_zero_point, rhs_zero_point))
    }

    /// Run row-major `u8 x i8 -> i32` QGEMM on the best available lane.
    pub fn qgemm(
        self,
        lhs: &[u8],
        rhs_transposed: &[i8],
        output: &mut [i32],
        params: QGemmParams<'_>,
    ) -> Result<Lane, Error> {
        let lane = self.best_lane();
        self.qgemm_with_lane(lhs, rhs_transposed, output, params, lane)?;
        Ok(lane)
    }

    /// Run QGEMM on an explicitly selected, runtime-checked lane.
    pub fn qgemm_with_lane(
        self,
        lhs: &[u8],
        rhs_transposed: &[i8],
        output: &mut [i32],
        params: QGemmParams<'_>,
        lane: Lane,
    ) -> Result<(), Error> {
        params.validate(lhs, rhs_transposed, output)?;
        if !self.supports(lane) {
            return Err(Error::UnsupportedLane);
        }

        for m in 0..params.m {
            let lhs_start = m * params.k;
            let lhs_row = &lhs[lhs_start..lhs_start + params.k];
            let prepared_lhs_sum = params.rhs_row_sums.map(|_| sum_u8(lhs_row));

            for n in 0..params.n {
                let rhs_start = n * params.k;
                let rhs_row = &rhs_transposed[rhs_start..rhs_start + params.k];
                let rhs_zero_point = params.rhs_zero_points.get(n);
                let value = if let (Some(lhs_sum), Some(rhs_sums)) =
                    (prepared_lhs_sum, params.rhs_row_sums)
                {
                    let raw = raw_dot(lhs_row, rhs_row, lane);
                    apply_zero_points(
                        raw,
                        lhs_sum,
                        rhs_sums[n],
                        params.k,
                        params.lhs_zero_point,
                        rhs_zero_point,
                    )
                } else {
                    let (raw, lhs_sum, rhs_sum) = raw_dot_and_sums(lhs_row, rhs_row, lane);
                    apply_zero_points(
                        raw,
                        lhs_sum,
                        rhs_sum,
                        params.k,
                        params.lhs_zero_point,
                        rhs_zero_point,
                    )
                };
                output[m * params.n + n] = value;
            }
        }
        Ok(())
    }
}

/// Compute raw signed row sums for an offline/native weight layout.
pub fn prepare_rhs_row_sums(
    rhs_transposed: &[i8],
    n: usize,
    k: usize,
    output: &mut [i32],
) -> Result<(), Error> {
    if n == 0 {
        return Err(Error::EmptyMatrix);
    }
    if k == 0 {
        return Err(Error::EmptyReduction);
    }
    let rhs_len = n.checked_mul(k).ok_or(Error::ShapeOverflow)?;
    if rhs_transposed.len() < rhs_len {
        return Err(Error::RhsTooSmall);
    }
    if output.len() < n {
        return Err(Error::OutputTooSmall);
    }
    for row in 0..n {
        output[row] = sum_i8(&rhs_transposed[row * k..(row + 1) * k]);
    }
    Ok(())
}

/// Apply ONNX-style per-output scales and optional bias to QGEMM accumulators.
pub fn dequantize_per_output(
    input: &[i32],
    m: usize,
    n: usize,
    lhs_scale: f32,
    rhs_scales: &[f32],
    bias: Option<&[f32]>,
    output: &mut [f32],
) -> Result<(), Error> {
    if m == 0 || n == 0 {
        return Err(Error::EmptyMatrix);
    }
    let elements = m.checked_mul(n).ok_or(Error::ShapeOverflow)?;
    if input.len() < elements {
        return Err(Error::LhsTooSmall);
    }
    if output.len() < elements {
        return Err(Error::OutputTooSmall);
    }
    if rhs_scales.len() < n {
        return Err(Error::ScalesTooSmall);
    }
    if bias.is_some_and(|values| values.len() < n) {
        return Err(Error::BiasTooSmall);
    }
    for row in 0..m {
        for column in 0..n {
            let index = row * n + column;
            output[index] = input[index] as f32 * (lhs_scale * rhs_scales[column])
                + bias.map_or(0.0, |values| values[column]);
        }
    }
    Ok(())
}

#[inline]
fn validate_dot(lhs: &[u8], rhs: &[i8]) -> Result<(), Error> {
    if lhs.is_empty() {
        Err(Error::EmptyReduction)
    } else if lhs.len() != rhs.len() {
        Err(Error::LengthMismatch)
    } else {
        Ok(())
    }
}

#[inline]
fn apply_zero_points(
    raw: i32,
    lhs_sum: i32,
    rhs_sum: i32,
    k: usize,
    lhs_zero_point: u8,
    rhs_zero_point: i8,
) -> i32 {
    let lhs_zero_point = i32::from(lhs_zero_point);
    let rhs_zero_point = i32::from(rhs_zero_point);
    raw.wrapping_sub(rhs_zero_point.wrapping_mul(lhs_sum))
        .wrapping_sub(lhs_zero_point.wrapping_mul(rhs_sum))
        .wrapping_add(
            (k as i32)
                .wrapping_mul(lhs_zero_point)
                .wrapping_mul(rhs_zero_point),
        )
}

#[inline]
fn raw_dot(lhs: &[u8], rhs: &[i8], lane: Lane) -> i32 {
    match lane {
        Lane::Scalar => raw_dot_scalar(lhs, rhs),
        #[cfg(target_arch = "x86_64")]
        Lane::Avx2 => unsafe { raw_dot_avx2(lhs, rhs) },
        #[cfg(target_arch = "x86_64")]
        Lane::AvxVnni => unsafe { raw_dot_avx_vnni(lhs, rhs) },
        #[cfg(not(target_arch = "x86_64"))]
        Lane::Avx2 | Lane::AvxVnni => unreachable!(),
    }
}

#[inline]
fn raw_dot_and_sums(lhs: &[u8], rhs: &[i8], lane: Lane) -> (i32, i32, i32) {
    match lane {
        Lane::Scalar => raw_dot_and_sums_scalar(lhs, rhs),
        #[cfg(target_arch = "x86_64")]
        Lane::Avx2 => unsafe { raw_dot_and_sums_avx2(lhs, rhs) },
        #[cfg(target_arch = "x86_64")]
        Lane::AvxVnni => unsafe { raw_dot_and_sums_avx_vnni(lhs, rhs) },
        #[cfg(not(target_arch = "x86_64"))]
        Lane::Avx2 | Lane::AvxVnni => unreachable!(),
    }
}

fn raw_dot_scalar(lhs: &[u8], rhs: &[i8]) -> i32 {
    lhs.iter()
        .copied()
        .zip(rhs.iter().copied())
        .fold(0i32, |sum, (lhs, rhs)| sum.wrapping_add(i32::from(lhs).wrapping_mul(i32::from(rhs))))
}

fn raw_dot_and_sums_scalar(lhs: &[u8], rhs: &[i8]) -> (i32, i32, i32) {
    lhs.iter().copied().zip(rhs.iter().copied()).fold(
        (0i32, 0i32, 0i32),
        |(dot, lhs_sum, rhs_sum), (lhs, rhs)| {
            (
                dot.wrapping_add(i32::from(lhs).wrapping_mul(i32::from(rhs))),
                lhs_sum.wrapping_add(i32::from(lhs)),
                rhs_sum.wrapping_add(i32::from(rhs)),
            )
        },
    )
}

fn sum_u8(values: &[u8]) -> i32 {
    values
        .iter()
        .copied()
        .fold(0i32, |sum, value| sum.wrapping_add(i32::from(value)))
}

fn sum_i8(values: &[i8]) -> i32 {
    values
        .iter()
        .copied()
        .fold(0i32, |sum, value| sum.wrapping_add(i32::from(value)))
}

#[cfg(target_arch = "x86_64")]
fn detect_cpu_capabilities() -> CpuCapabilities {
    use core::arch::x86_64::{__cpuid, __cpuid_count};

    const CPUID_1_ECX_OSXSAVE: u32 = 1 << 27;
    const CPUID_1_ECX_AVX: u32 = 1 << 28;
    const CPUID_7_0_EBX_AVX2: u32 = 1 << 5;
    const CPUID_7_1_EAX_AVX_VNNI: u32 = 1 << 4;
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

    let leaf_7_0 = __cpuid_count(7, 0);
    let avx2 = leaf_7_0.ebx & CPUID_7_0_EBX_AVX2 != 0;
    let avx_vnni =
        avx2 && leaf_7_0.eax >= 1 && __cpuid_count(7, 1).eax & CPUID_7_1_EAX_AVX_VNNI != 0;
    CpuCapabilities {
        ymm_state: true,
        avx2,
        avx_vnni,
    }
}

#[cfg(not(target_arch = "x86_64"))]
const fn detect_cpu_capabilities() -> CpuCapabilities {
    CpuCapabilities {
        ymm_state: false,
        avx2: false,
        avx_vnni: false,
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

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn raw_dot_avx2(lhs: &[u8], rhs: &[i8]) -> i32 {
    use core::arch::x86_64::{
        __m256i, _mm256_add_epi32, _mm256_castsi256_si128, _mm256_cvtepi8_epi16,
        _mm256_cvtepu8_epi16, _mm256_extracti128_si256, _mm256_loadu_si256, _mm256_madd_epi16,
        _mm256_setzero_si256, _mm256_storeu_si256,
    };

    let mut index = 0usize;
    let mut accumulator = _mm256_setzero_si256();
    while index + 32 <= lhs.len() {
        let lhs_bytes = unsafe { _mm256_loadu_si256(lhs.as_ptr().add(index).cast::<__m256i>()) };
        let rhs_bytes = unsafe { _mm256_loadu_si256(rhs.as_ptr().add(index).cast::<__m256i>()) };

        let lhs_low = _mm256_cvtepu8_epi16(_mm256_castsi256_si128(lhs_bytes));
        let lhs_high = _mm256_cvtepu8_epi16(_mm256_extracti128_si256::<1>(lhs_bytes));
        let rhs_low = _mm256_cvtepi8_epi16(_mm256_castsi256_si128(rhs_bytes));
        let rhs_high = _mm256_cvtepi8_epi16(_mm256_extracti128_si256::<1>(rhs_bytes));
        accumulator = _mm256_add_epi32(
            accumulator,
            _mm256_add_epi32(
                _mm256_madd_epi16(lhs_low, rhs_low),
                _mm256_madd_epi16(lhs_high, rhs_high),
            ),
        );
        index += 32;
    }

    let mut lanes = [0i32; 8];
    unsafe { _mm256_storeu_si256(lanes.as_mut_ptr().cast::<__m256i>(), accumulator) };
    let mut sum = lanes.into_iter().fold(0i32, i32::wrapping_add);
    while index < lhs.len() {
        sum = sum.wrapping_add(i32::from(lhs[index]).wrapping_mul(i32::from(rhs[index])));
        index += 1;
    }
    sum
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn raw_dot_and_sums_avx2(lhs: &[u8], rhs: &[i8]) -> (i32, i32, i32) {
    use core::arch::x86_64::{
        __m256i, _mm256_add_epi32, _mm256_castsi256_si128, _mm256_cvtepi8_epi16,
        _mm256_cvtepu8_epi16, _mm256_extracti128_si256, _mm256_loadu_si256, _mm256_madd_epi16,
        _mm256_set1_epi16, _mm256_setzero_si256, _mm256_storeu_si256,
    };

    let ones = _mm256_set1_epi16(1);
    let mut index = 0usize;
    let mut dot_accumulator = _mm256_setzero_si256();
    let mut lhs_accumulator = _mm256_setzero_si256();
    let mut rhs_accumulator = _mm256_setzero_si256();
    while index + 32 <= lhs.len() {
        let lhs_bytes = unsafe { _mm256_loadu_si256(lhs.as_ptr().add(index).cast::<__m256i>()) };
        let rhs_bytes = unsafe { _mm256_loadu_si256(rhs.as_ptr().add(index).cast::<__m256i>()) };
        let lhs_low = _mm256_cvtepu8_epi16(_mm256_castsi256_si128(lhs_bytes));
        let lhs_high = _mm256_cvtepu8_epi16(_mm256_extracti128_si256::<1>(lhs_bytes));
        let rhs_low = _mm256_cvtepi8_epi16(_mm256_castsi256_si128(rhs_bytes));
        let rhs_high = _mm256_cvtepi8_epi16(_mm256_extracti128_si256::<1>(rhs_bytes));

        dot_accumulator = _mm256_add_epi32(
            dot_accumulator,
            _mm256_add_epi32(
                _mm256_madd_epi16(lhs_low, rhs_low),
                _mm256_madd_epi16(lhs_high, rhs_high),
            ),
        );
        lhs_accumulator = _mm256_add_epi32(
            lhs_accumulator,
            _mm256_add_epi32(_mm256_madd_epi16(lhs_low, ones), _mm256_madd_epi16(lhs_high, ones)),
        );
        rhs_accumulator = _mm256_add_epi32(
            rhs_accumulator,
            _mm256_add_epi32(_mm256_madd_epi16(rhs_low, ones), _mm256_madd_epi16(rhs_high, ones)),
        );
        index += 32;
    }

    let mut dot_lanes = [0i32; 8];
    let mut lhs_lanes = [0i32; 8];
    let mut rhs_lanes = [0i32; 8];
    unsafe {
        _mm256_storeu_si256(dot_lanes.as_mut_ptr().cast::<__m256i>(), dot_accumulator);
        _mm256_storeu_si256(lhs_lanes.as_mut_ptr().cast::<__m256i>(), lhs_accumulator);
        _mm256_storeu_si256(rhs_lanes.as_mut_ptr().cast::<__m256i>(), rhs_accumulator);
    }
    let mut dot = dot_lanes.into_iter().fold(0i32, i32::wrapping_add);
    let mut lhs_sum = lhs_lanes.into_iter().fold(0i32, i32::wrapping_add);
    let mut rhs_sum = rhs_lanes.into_iter().fold(0i32, i32::wrapping_add);
    while index < lhs.len() {
        dot = dot.wrapping_add(i32::from(lhs[index]).wrapping_mul(i32::from(rhs[index])));
        lhs_sum = lhs_sum.wrapping_add(i32::from(lhs[index]));
        rhs_sum = rhs_sum.wrapping_add(i32::from(rhs[index]));
        index += 1;
    }
    (dot, lhs_sum, rhs_sum)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,avxvnni")]
unsafe fn raw_dot_avx_vnni(lhs: &[u8], rhs: &[i8]) -> i32 {
    use core::arch::x86_64::{
        __m256i, _mm256_dpbusd_avx_epi32, _mm256_loadu_si256, _mm256_setzero_si256,
        _mm256_storeu_si256,
    };

    let mut index = 0usize;
    let mut accumulator = _mm256_setzero_si256();
    while index + 32 <= lhs.len() {
        let lhs_bytes = unsafe { _mm256_loadu_si256(lhs.as_ptr().add(index).cast::<__m256i>()) };
        let rhs_bytes = unsafe { _mm256_loadu_si256(rhs.as_ptr().add(index).cast::<__m256i>()) };
        accumulator = _mm256_dpbusd_avx_epi32(accumulator, lhs_bytes, rhs_bytes);
        index += 32;
    }

    let mut lanes = [0i32; 8];
    unsafe { _mm256_storeu_si256(lanes.as_mut_ptr().cast::<__m256i>(), accumulator) };
    let mut sum = lanes.into_iter().fold(0i32, i32::wrapping_add);
    while index < lhs.len() {
        sum = sum.wrapping_add(i32::from(lhs[index]).wrapping_mul(i32::from(rhs[index])));
        index += 1;
    }
    sum
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,avxvnni")]
unsafe fn raw_dot_and_sums_avx_vnni(lhs: &[u8], rhs: &[i8]) -> (i32, i32, i32) {
    use core::arch::x86_64::{
        __m256i, _mm256_add_epi32, _mm256_castsi256_si128, _mm256_cvtepi8_epi16,
        _mm256_cvtepu8_epi16, _mm256_dpbusd_avx_epi32, _mm256_extracti128_si256,
        _mm256_loadu_si256, _mm256_madd_epi16, _mm256_set1_epi16, _mm256_setzero_si256,
        _mm256_storeu_si256,
    };

    let ones = _mm256_set1_epi16(1);
    let mut index = 0usize;
    let mut dot_accumulator = _mm256_setzero_si256();
    let mut lhs_accumulator = _mm256_setzero_si256();
    let mut rhs_accumulator = _mm256_setzero_si256();
    while index + 32 <= lhs.len() {
        let lhs_bytes = unsafe { _mm256_loadu_si256(lhs.as_ptr().add(index).cast::<__m256i>()) };
        let rhs_bytes = unsafe { _mm256_loadu_si256(rhs.as_ptr().add(index).cast::<__m256i>()) };
        dot_accumulator = _mm256_dpbusd_avx_epi32(dot_accumulator, lhs_bytes, rhs_bytes);

        let lhs_low = _mm256_cvtepu8_epi16(_mm256_castsi256_si128(lhs_bytes));
        let lhs_high = _mm256_cvtepu8_epi16(_mm256_extracti128_si256::<1>(lhs_bytes));
        let rhs_low = _mm256_cvtepi8_epi16(_mm256_castsi256_si128(rhs_bytes));
        let rhs_high = _mm256_cvtepi8_epi16(_mm256_extracti128_si256::<1>(rhs_bytes));
        lhs_accumulator = _mm256_add_epi32(
            lhs_accumulator,
            _mm256_add_epi32(_mm256_madd_epi16(lhs_low, ones), _mm256_madd_epi16(lhs_high, ones)),
        );
        rhs_accumulator = _mm256_add_epi32(
            rhs_accumulator,
            _mm256_add_epi32(_mm256_madd_epi16(rhs_low, ones), _mm256_madd_epi16(rhs_high, ones)),
        );
        index += 32;
    }

    let mut dot_lanes = [0i32; 8];
    let mut lhs_lanes = [0i32; 8];
    let mut rhs_lanes = [0i32; 8];
    unsafe {
        _mm256_storeu_si256(dot_lanes.as_mut_ptr().cast::<__m256i>(), dot_accumulator);
        _mm256_storeu_si256(lhs_lanes.as_mut_ptr().cast::<__m256i>(), lhs_accumulator);
        _mm256_storeu_si256(rhs_lanes.as_mut_ptr().cast::<__m256i>(), rhs_accumulator);
    }
    let mut dot = dot_lanes.into_iter().fold(0i32, i32::wrapping_add);
    let mut lhs_sum = lhs_lanes.into_iter().fold(0i32, i32::wrapping_add);
    let mut rhs_sum = rhs_lanes.into_iter().fold(0i32, i32::wrapping_add);
    while index < lhs.len() {
        dot = dot.wrapping_add(i32::from(lhs[index]).wrapping_mul(i32::from(rhs[index])));
        lhs_sum = lhs_sum.wrapping_add(i32::from(lhs[index]));
        rhs_sum = rhs_sum.wrapping_add(i32::from(rhs[index]));
        index += 1;
    }
    (dot, lhs_sum, rhs_sum)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec;
    use std::vec::Vec;

    fn vectors(len: usize) -> (Vec<u8>, Vec<i8>) {
        let mut state = 0x6A09_E667_F3BC_C909u64;
        let mut lhs = Vec::with_capacity(len);
        let mut rhs = Vec::with_capacity(len);
        for index in 0..len {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            lhs.push((state as u8).wrapping_add(index as u8));
            rhs.push((state.rotate_left(19) as u8).wrapping_add((index * 29) as u8) as i8);
        }
        (lhs, rhs)
    }

    fn available_lanes(dispatcher: Dispatcher) -> impl Iterator<Item = Lane> {
        [Lane::Scalar, Lane::Avx2, Lane::AvxVnni]
            .into_iter()
            .filter(move |lane| dispatcher.supports(*lane))
    }

    #[test]
    fn every_available_lane_matches_scalar_for_tails_and_zero_points() {
        let dispatcher = Dispatcher::detect();
        for len in [
            1, 2, 3, 4, 15, 16, 31, 32, 33, 63, 64, 65, 127, 257, 768, 1024, 11264,
        ] {
            let (lhs, rhs) = vectors(len);
            for (lhs_zero_point, rhs_zero_point) in [(0, 0), (128, 0), (247, -13), (255, 127)] {
                let expected = dispatcher
                    .dot_with_lane(&lhs, &rhs, lhs_zero_point, rhs_zero_point, Lane::Scalar)
                    .unwrap();
                for lane in available_lanes(dispatcher) {
                    assert_eq!(
                        dispatcher
                            .dot_with_lane(&lhs, &rhs, lhs_zero_point, rhs_zero_point, lane)
                            .unwrap(),
                        expected,
                        "lane={lane:?} len={len} lhs_zp={lhs_zero_point} rhs_zp={rhs_zero_point}"
                    );
                }
            }
        }
    }

    #[test]
    fn avx2_full_range_input_does_not_saturate_intermediate_pairs() {
        let dispatcher = Dispatcher::detect();
        if !dispatcher.supports(Lane::Avx2) {
            return;
        }
        let lhs = [255u8; 64];
        let rhs = [127i8; 64];
        let expected = 255 * 127 * 64;
        assert_eq!(
            dispatcher
                .dot_with_lane(&lhs, &rhs, 0, 0, Lane::Avx2)
                .unwrap(),
            expected
        );
    }

    #[test]
    fn qgemm_prepared_and_inline_sums_match_every_lane() {
        const M: usize = 3;
        const N: usize = 5;
        const K: usize = 65;
        let dispatcher = Dispatcher::detect();
        let (lhs, _) = vectors(M * K);
        let (_, rhs) = vectors(N * K);
        let rhs_zero_points = [-17, 0, 9, 127, -128];
        let mut rhs_sums = [0i32; N];
        prepare_rhs_row_sums(&rhs, N, K, &mut rhs_sums).unwrap();

        let mut expected = [0i32; M * N];
        let scalar_params = QGemmParams {
            m: M,
            n: N,
            k: K,
            lhs_zero_point: 193,
            rhs_zero_points: RhsZeroPoints::PerOutput(&rhs_zero_points),
            rhs_row_sums: Some(&rhs_sums),
        };
        dispatcher
            .qgemm_with_lane(&lhs, &rhs, &mut expected, scalar_params, Lane::Scalar)
            .unwrap();

        for lane in available_lanes(dispatcher) {
            let mut prepared = [0i32; M * N];
            dispatcher
                .qgemm_with_lane(&lhs, &rhs, &mut prepared, scalar_params, lane)
                .unwrap();
            assert_eq!(prepared, expected, "prepared lane={lane:?}");

            let mut inline = [0i32; M * N];
            let inline_params = QGemmParams {
                rhs_row_sums: None,
                ..scalar_params
            };
            dispatcher
                .qgemm_with_lane(&lhs, &rhs, &mut inline, inline_params, lane)
                .unwrap();
            assert_eq!(inline, expected, "inline lane={lane:?}");
        }
    }

    #[test]
    fn automatic_dispatch_is_supported_and_exact() {
        let dispatcher = Dispatcher::detect();
        let (lhs, rhs) = vectors(769);
        let expected = dispatcher
            .dot_with_lane(&lhs, &rhs, 131, -7, Lane::Scalar)
            .unwrap();
        let (observed, lane) = dispatcher.dot(&lhs, &rhs, 131, -7).unwrap();
        assert!(dispatcher.supports(lane));
        assert_eq!(lane, dispatcher.best_lane());
        assert_eq!(observed, expected);
    }

    #[test]
    fn capability_contract_never_admits_vnni_without_avx2_and_ymm() {
        let capabilities = Dispatcher::detect().capabilities();
        if capabilities.avx_vnni() {
            assert!(capabilities.avx2());
            assert!(capabilities.ymm_state());
        }
        if capabilities.avx2() {
            assert!(capabilities.ymm_state());
        }
    }

    #[test]
    fn dequantization_applies_per_output_scale_and_bias() {
        let input = [100i32, -200, 300, -400];
        let mut output = [0.0f32; 4];
        dequantize_per_output(&input, 2, 2, 0.25, &[0.5, 2.0], Some(&[1.0, -3.0]), &mut output)
            .unwrap();
        assert_eq!(output, [13.5, -103.0, 38.5, -203.0]);
    }

    #[test]
    fn validation_rejects_bad_shapes_and_unavailable_lanes() {
        let dispatcher = Dispatcher::detect();
        assert_eq!(dispatcher.dot(&[], &[], 0, 0), Err(Error::EmptyReduction));
        assert_eq!(dispatcher.dot(&[1, 2], &[1], 0, 0), Err(Error::LengthMismatch));

        let params = QGemmParams {
            m: 1,
            n: 2,
            k: 3,
            lhs_zero_point: 0,
            rhs_zero_points: RhsZeroPoints::PerOutput(&[0]),
            rhs_row_sums: None,
        };
        assert_eq!(
            dispatcher.qgemm(&[0; 3], &[0; 6], &mut [0; 2], params),
            Err(Error::ZeroPointsTooSmall)
        );

        let unavailable = [Lane::AvxVnni, Lane::Avx2]
            .into_iter()
            .find(|lane| !dispatcher.supports(*lane));
        if let Some(lane) = unavailable {
            assert_eq!(
                dispatcher.dot_with_lane(&[1], &[1], 0, 0, lane),
                Err(Error::UnsupportedLane)
            );
        }
    }

    #[test]
    fn scalar_reference_matches_direct_centered_arithmetic() {
        let dispatcher = Dispatcher::detect();
        let lhs = [0u8, 1, 127, 128, 254, 255];
        let rhs = [-128i8, -127, -1, 0, 126, 127];
        let lhs_zero_point = 123u8;
        let rhs_zero_point = -19i8;
        let direct = lhs.iter().zip(rhs.iter()).fold(0i32, |sum, (&lhs, &rhs)| {
            sum.wrapping_add(
                (i32::from(lhs) - i32::from(lhs_zero_point))
                    .wrapping_mul(i32::from(rhs) - i32::from(rhs_zero_point)),
            )
        });
        assert_eq!(
            dispatcher
                .dot_with_lane(&lhs, &rhs, lhs_zero_point, rhs_zero_point, Lane::Scalar,)
                .unwrap(),
            direct
        );
    }

    #[test]
    fn prepared_row_sum_validation_is_explicit() {
        let rhs = vec![1i8; 12];
        let mut too_short = [0i32; 2];
        assert_eq!(prepare_rhs_row_sums(&rhs, 3, 4, &mut too_short), Err(Error::OutputTooSmall));
    }
}
