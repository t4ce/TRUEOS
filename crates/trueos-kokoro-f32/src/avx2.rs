//! The crate's sole unsafe island.
//!
//! Every public-to-private entry is safe: it verifies AVX/OSXSAVE/AVX2 and
//! XCR0 XMM+YMM state, checks equal slice lengths, and only then calls a
//! target-feature function. All vector memory operations stay inside those
//! validated slices and use unaligned loads/stores.

use core::arch::x86_64::{
    __cpuid, __cpuid_count, __m256, _mm256_add_ps, _mm256_and_si256, _mm256_castps_si256,
    _mm256_castsi256_ps, _mm256_cmpeq_epi32, _mm256_div_ps, _mm256_loadu_ps, _mm256_movemask_ps,
    _mm256_mul_ps, _mm256_set1_epi32, _mm256_set1_ps, _mm256_storeu_ps, _mm256_sub_ps, _xgetbv,
};
use core::sync::atomic::{AtomicU8, Ordering};

use super::{BinaryOperation, Error};

const UNKNOWN: u8 = 0;
const UNAVAILABLE: u8 = 1;
const AVAILABLE: u8 = 2;
const LANES: usize = 8;

static AVX2_STATE: AtomicU8 = AtomicU8::new(UNKNOWN);

pub(super) fn is_available() -> bool {
    match AVX2_STATE.load(Ordering::Acquire) {
        AVAILABLE => true,
        UNAVAILABLE => false,
        _ => {
            let available = detect();
            AVX2_STATE.store(if available { AVAILABLE } else { UNAVAILABLE }, Ordering::Release);
            available
        }
    }
}

fn detect() -> bool {
    // SAFETY: CPUID is available on x86_64. XGETBV executes only after CPUID
    // reports OSXSAVE, exactly as required by Intel's AVX detection contract.
    unsafe {
        let leaf0 = __cpuid(0);
        if leaf0.eax < 7 {
            return false;
        }
        let leaf1 = __cpuid(1);
        const OSXSAVE: u32 = 1 << 27;
        const AVX: u32 = 1 << 28;
        if leaf1.ecx & (OSXSAVE | AVX) != OSXSAVE | AVX || _xgetbv(0) & 0b110 != 0b110 {
            return false;
        }
        __cpuid_count(7, 0).ebx & (1 << 5) != 0
    }
}

pub(super) fn binary_pair(
    operation: BinaryOperation,
    lhs: &[f32],
    rhs: &[f32],
    output: &mut [f32],
) -> Result<(), Error> {
    if lhs.len() != rhs.len() || lhs.len() != output.len() {
        return Err(Error::BufferTooSmall);
    }
    if !is_available() {
        return Err(Error::UnsupportedLane);
    }
    // SAFETY: availability and all slice bounds are proven above. The target
    // function uses only loadu/storeu within `0..len`.
    unsafe { binary_pair_avx2(operation, lhs, rhs, output) }
}

pub(super) fn binary_lhs_scalar(
    operation: BinaryOperation,
    lhs: f32,
    rhs: &[f32],
    output: &mut [f32],
) -> Result<(), Error> {
    if rhs.len() != output.len() {
        return Err(Error::BufferTooSmall);
    }
    if !is_available() {
        return Err(Error::UnsupportedLane);
    }
    // SAFETY: availability and all slice bounds are proven above.
    unsafe { binary_lhs_scalar_avx2(operation, lhs, rhs, output) }
}

pub(super) fn binary_rhs_scalar(
    operation: BinaryOperation,
    lhs: &[f32],
    rhs: f32,
    output: &mut [f32],
) -> Result<(), Error> {
    if lhs.len() != output.len() {
        return Err(Error::BufferTooSmall);
    }
    if !is_available() {
        return Err(Error::UnsupportedLane);
    }
    // SAFETY: availability and all slice bounds are proven above.
    unsafe { binary_rhs_scalar_avx2(operation, lhs, rhs, output) }
}

pub(super) fn square(input: &[f32], output: &mut [f32]) -> Result<(), Error> {
    if input.len() != output.len() {
        return Err(Error::BufferTooSmall);
    }
    if !is_available() {
        return Err(Error::UnsupportedLane);
    }
    // SAFETY: availability and all slice bounds are proven above.
    unsafe { square_avx2(input, output) }
}

#[target_feature(enable = "avx2")]
unsafe fn binary_pair_avx2(
    operation: BinaryOperation,
    lhs: &[f32],
    rhs: &[f32],
    output: &mut [f32],
) -> Result<(), Error> {
    let vector_end = lhs.len() / LANES * LANES;
    let mut index = 0usize;
    while index < vector_end {
        let lhs_vector = unsafe { _mm256_loadu_ps(lhs.as_ptr().add(index)) };
        let rhs_vector = unsafe { _mm256_loadu_ps(rhs.as_ptr().add(index)) };
        let result = unsafe { apply_vector(operation, lhs_vector, rhs_vector) };
        if unsafe {
            nonfinite_mask(lhs_vector) | nonfinite_mask(rhs_vector) | nonfinite_mask(result)
        } != 0
        {
            validate_pair_scalar(operation, lhs, rhs, index, index + LANES)?;
        }
        index += LANES;
    }
    validate_pair_scalar(operation, lhs, rhs, vector_end, lhs.len())?;

    index = 0;
    while index < vector_end {
        let lhs_vector = unsafe { _mm256_loadu_ps(lhs.as_ptr().add(index)) };
        let rhs_vector = unsafe { _mm256_loadu_ps(rhs.as_ptr().add(index)) };
        let result = unsafe { apply_vector(operation, lhs_vector, rhs_vector) };
        unsafe { _mm256_storeu_ps(output.as_mut_ptr().add(index), result) };
        index += LANES;
    }
    for index in vector_end..lhs.len() {
        output[index] = operation.apply(lhs[index], rhs[index]);
    }
    Ok(())
}

#[target_feature(enable = "avx2")]
unsafe fn binary_lhs_scalar_avx2(
    operation: BinaryOperation,
    lhs: f32,
    rhs: &[f32],
    output: &mut [f32],
) -> Result<(), Error> {
    if !lhs.is_finite() {
        return Err(Error::NonFiniteInput);
    }
    let lhs_vector = _mm256_set1_ps(lhs);
    let vector_end = rhs.len() / LANES * LANES;
    let mut index = 0usize;
    while index < vector_end {
        let rhs_vector = unsafe { _mm256_loadu_ps(rhs.as_ptr().add(index)) };
        let result = unsafe { apply_vector(operation, lhs_vector, rhs_vector) };
        if unsafe { nonfinite_mask(rhs_vector) | nonfinite_mask(result) } != 0 {
            validate_lhs_scalar(operation, lhs, rhs, index, index + LANES)?;
        }
        index += LANES;
    }
    validate_lhs_scalar(operation, lhs, rhs, vector_end, rhs.len())?;

    index = 0;
    while index < vector_end {
        let rhs_vector = unsafe { _mm256_loadu_ps(rhs.as_ptr().add(index)) };
        let result = unsafe { apply_vector(operation, lhs_vector, rhs_vector) };
        unsafe { _mm256_storeu_ps(output.as_mut_ptr().add(index), result) };
        index += LANES;
    }
    for index in vector_end..rhs.len() {
        output[index] = operation.apply(lhs, rhs[index]);
    }
    Ok(())
}

#[target_feature(enable = "avx2")]
unsafe fn binary_rhs_scalar_avx2(
    operation: BinaryOperation,
    lhs: &[f32],
    rhs: f32,
    output: &mut [f32],
) -> Result<(), Error> {
    if !rhs.is_finite() {
        return Err(Error::NonFiniteInput);
    }
    let rhs_vector = _mm256_set1_ps(rhs);
    let vector_end = lhs.len() / LANES * LANES;
    let mut index = 0usize;
    while index < vector_end {
        let lhs_vector = unsafe { _mm256_loadu_ps(lhs.as_ptr().add(index)) };
        let result = unsafe { apply_vector(operation, lhs_vector, rhs_vector) };
        if unsafe { nonfinite_mask(lhs_vector) | nonfinite_mask(result) } != 0 {
            validate_rhs_scalar(operation, lhs, rhs, index, index + LANES)?;
        }
        index += LANES;
    }
    validate_rhs_scalar(operation, lhs, rhs, vector_end, lhs.len())?;

    index = 0;
    while index < vector_end {
        let lhs_vector = unsafe { _mm256_loadu_ps(lhs.as_ptr().add(index)) };
        let result = unsafe { apply_vector(operation, lhs_vector, rhs_vector) };
        unsafe { _mm256_storeu_ps(output.as_mut_ptr().add(index), result) };
        index += LANES;
    }
    for index in vector_end..lhs.len() {
        output[index] = operation.apply(lhs[index], rhs);
    }
    Ok(())
}

#[target_feature(enable = "avx2")]
unsafe fn square_avx2(input: &[f32], output: &mut [f32]) -> Result<(), Error> {
    let vector_end = input.len() / LANES * LANES;
    let mut index = 0usize;
    while index < vector_end {
        let vector = unsafe { _mm256_loadu_ps(input.as_ptr().add(index)) };
        let result = _mm256_mul_ps(vector, vector);
        if unsafe { nonfinite_mask(vector) | nonfinite_mask(result) } != 0 {
            validate_square_scalar(input, index, index + LANES)?;
        }
        index += LANES;
    }
    validate_square_scalar(input, vector_end, input.len())?;

    index = 0;
    while index < vector_end {
        let vector = unsafe { _mm256_loadu_ps(input.as_ptr().add(index)) };
        unsafe { _mm256_storeu_ps(output.as_mut_ptr().add(index), _mm256_mul_ps(vector, vector)) };
        index += LANES;
    }
    for index in vector_end..input.len() {
        output[index] = input[index] * input[index];
    }
    Ok(())
}

#[inline]
#[target_feature(enable = "avx2")]
unsafe fn apply_vector(operation: BinaryOperation, lhs: __m256, rhs: __m256) -> __m256 {
    match operation {
        BinaryOperation::Add => _mm256_add_ps(lhs, rhs),
        BinaryOperation::Mul => _mm256_mul_ps(lhs, rhs),
        BinaryOperation::Div => _mm256_div_ps(lhs, rhs),
        BinaryOperation::Sub => _mm256_sub_ps(lhs, rhs),
    }
}

#[inline]
#[target_feature(enable = "avx2")]
unsafe fn nonfinite_mask(value: __m256) -> i32 {
    let bits = _mm256_castps_si256(value);
    let exponent = _mm256_and_si256(bits, _mm256_set1_epi32(0x7f80_0000));
    let nonfinite = _mm256_cmpeq_epi32(exponent, _mm256_set1_epi32(0x7f80_0000));
    _mm256_movemask_ps(_mm256_castsi256_ps(nonfinite))
}

fn validate_pair_scalar(
    operation: BinaryOperation,
    lhs: &[f32],
    rhs: &[f32],
    start: usize,
    end: usize,
) -> Result<(), Error> {
    for index in start..end {
        validate_values(operation, lhs[index], rhs[index])?;
    }
    Ok(())
}

fn validate_lhs_scalar(
    operation: BinaryOperation,
    lhs: f32,
    rhs: &[f32],
    start: usize,
    end: usize,
) -> Result<(), Error> {
    for &rhs in &rhs[start..end] {
        validate_values(operation, lhs, rhs)?;
    }
    Ok(())
}

fn validate_rhs_scalar(
    operation: BinaryOperation,
    lhs: &[f32],
    rhs: f32,
    start: usize,
    end: usize,
) -> Result<(), Error> {
    for &lhs in &lhs[start..end] {
        validate_values(operation, lhs, rhs)?;
    }
    Ok(())
}

fn validate_values(operation: BinaryOperation, lhs: f32, rhs: f32) -> Result<(), Error> {
    if !lhs.is_finite() || !rhs.is_finite() {
        return Err(Error::NonFiniteInput);
    }
    if !operation.apply(lhs, rhs).is_finite() {
        return Err(Error::NonFiniteOutput);
    }
    Ok(())
}

fn validate_square_scalar(input: &[f32], start: usize, end: usize) -> Result<(), Error> {
    for &value in &input[start..end] {
        if !value.is_finite() {
            return Err(Error::NonFiniteInput);
        }
        if !(value * value).is_finite() {
            return Err(Error::NonFiniteOutput);
        }
    }
    Ok(())
}
