//! The crate's sole unsafe island.
//!
//! Every public-to-private entry is safe: it verifies AVX/OSXSAVE/AVX2 and
//! XCR0 XMM+YMM state, checks equal slice lengths, and only then calls a
//! target-feature function. All vector memory operations stay inside those
//! validated slices and use unaligned loads/stores.

use core::arch::x86_64::{
    __cpuid, __cpuid_count, __m256, __m256d, _mm_storeu_ps, _mm256_add_pd, _mm256_add_ps,
    _mm256_and_si256, _mm256_castps_si256, _mm256_castsi256_ps, _mm256_cmpeq_epi32,
    _mm256_cvtpd_ps, _mm256_div_ps, _mm256_loadu_pd, _mm256_loadu_ps, _mm256_movemask_ps,
    _mm256_mul_pd, _mm256_mul_ps, _mm256_set1_epi32, _mm256_set1_pd, _mm256_set1_ps,
    _mm256_storeu_ps, _mm256_sub_ps, _xgetbv,
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

pub(super) fn binary_lhs_row_scalar(
    operation: BinaryOperation,
    lhs: &[f32],
    rhs: &[f32],
    row_elements: usize,
    output: &mut [f32],
) -> Result<(), Error> {
    if row_elements == 0
        || lhs.len().checked_mul(row_elements) != Some(rhs.len())
        || rhs.len() != output.len()
    {
        return Err(Error::BufferTooSmall);
    }
    if !is_available() {
        return Err(Error::UnsupportedLane);
    }
    // SAFETY: availability and the complete row partition are proven above.
    unsafe { binary_lhs_row_scalar_avx2(operation, lhs, rhs, row_elements, output) }
}

pub(super) fn binary_rhs_row_scalar(
    operation: BinaryOperation,
    lhs: &[f32],
    rhs: &[f32],
    row_elements: usize,
    output: &mut [f32],
) -> Result<(), Error> {
    if row_elements == 0
        || rhs.len().checked_mul(row_elements) != Some(lhs.len())
        || lhs.len() != output.len()
    {
        return Err(Error::BufferTooSmall);
    }
    if !is_available() {
        return Err(Error::UnsupportedLane);
    }
    // SAFETY: availability and the complete row partition are proven above.
    unsafe { binary_rhs_row_scalar_avx2(operation, lhs, rhs, row_elements, output) }
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

pub(super) fn sin(input: &[f32], output: &mut [f32]) -> Result<(), Error> {
    if input.len() != output.len() {
        return Err(Error::BufferTooSmall);
    }
    if !is_available() {
        return Err(Error::UnsupportedLane);
    }
    if input.iter().any(|value| !value.is_finite()) {
        return Err(Error::NonFiniteInput);
    }
    // SAFETY: availability, complete slice bounds, and finite input are proven
    // above. Large arguments retain the scalar libm fallback lane by lane.
    unsafe { sin_avx2(input, output) }
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
unsafe fn binary_lhs_row_scalar_avx2(
    operation: BinaryOperation,
    lhs: &[f32],
    rhs: &[f32],
    row_elements: usize,
    output: &mut [f32],
) -> Result<(), Error> {
    let vector_elements = row_elements / LANES * LANES;
    for (row, &lhs) in lhs.iter().enumerate() {
        if !lhs.is_finite() {
            return Err(Error::NonFiniteInput);
        }
        let start = row * row_elements;
        let vector_end = start + vector_elements;
        let end = start + row_elements;
        let lhs_vector = _mm256_set1_ps(lhs);
        let mut index = start;
        while index < vector_end {
            let rhs_vector = unsafe { _mm256_loadu_ps(rhs.as_ptr().add(index)) };
            let result = unsafe { apply_vector(operation, lhs_vector, rhs_vector) };
            if unsafe { nonfinite_mask(rhs_vector) | nonfinite_mask(result) } != 0 {
                validate_lhs_scalar(operation, lhs, rhs, index, index + LANES)?;
            }
            index += LANES;
        }
        validate_lhs_scalar(operation, lhs, rhs, vector_end, end)?;
    }

    for (row, &lhs) in lhs.iter().enumerate() {
        let start = row * row_elements;
        let vector_end = start + vector_elements;
        let end = start + row_elements;
        let lhs_vector = _mm256_set1_ps(lhs);
        let mut index = start;
        while index < vector_end {
            let rhs_vector = unsafe { _mm256_loadu_ps(rhs.as_ptr().add(index)) };
            let result = unsafe { apply_vector(operation, lhs_vector, rhs_vector) };
            unsafe { _mm256_storeu_ps(output.as_mut_ptr().add(index), result) };
            index += LANES;
        }
        for index in vector_end..end {
            output[index] = operation.apply(lhs, rhs[index]);
        }
    }
    Ok(())
}

#[target_feature(enable = "avx2")]
unsafe fn binary_rhs_row_scalar_avx2(
    operation: BinaryOperation,
    lhs: &[f32],
    rhs: &[f32],
    row_elements: usize,
    output: &mut [f32],
) -> Result<(), Error> {
    let vector_elements = row_elements / LANES * LANES;
    for (row, &rhs) in rhs.iter().enumerate() {
        if !rhs.is_finite() {
            return Err(Error::NonFiniteInput);
        }
        let start = row * row_elements;
        let vector_end = start + vector_elements;
        let end = start + row_elements;
        let rhs_vector = _mm256_set1_ps(rhs);
        let mut index = start;
        while index < vector_end {
            let lhs_vector = unsafe { _mm256_loadu_ps(lhs.as_ptr().add(index)) };
            let result = unsafe { apply_vector(operation, lhs_vector, rhs_vector) };
            if unsafe { nonfinite_mask(lhs_vector) | nonfinite_mask(result) } != 0 {
                validate_rhs_scalar(operation, lhs, rhs, index, index + LANES)?;
            }
            index += LANES;
        }
        validate_rhs_scalar(operation, lhs, rhs, vector_end, end)?;
    }

    for (row, &rhs) in rhs.iter().enumerate() {
        let start = row * row_elements;
        let vector_end = start + vector_elements;
        let end = start + row_elements;
        let rhs_vector = _mm256_set1_ps(rhs);
        let mut index = start;
        while index < vector_end {
            let lhs_vector = unsafe { _mm256_loadu_ps(lhs.as_ptr().add(index)) };
            let result = unsafe { apply_vector(operation, lhs_vector, rhs_vector) };
            unsafe { _mm256_storeu_ps(output.as_mut_ptr().add(index), result) };
            index += LANES;
        }
        for index in vector_end..end {
            output[index] = operation.apply(lhs[index], rhs);
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct ReducedSin {
    argument: f64,
    cosine: bool,
    negate: bool,
    direct: Option<f32>,
}

fn reduce_sin(value: f32) -> ReducedSin {
    use core::f64::consts::FRAC_PI_2;

    const TOINT: f64 = 1.5 / f64::EPSILON;
    const INV_PIO2: f64 = core::f64::consts::FRAC_2_PI;
    const PIO2_1: f64 = 1.570_796_310_901_641_8;
    const PIO2_1T: f64 = 1.589_325_477_352_819_6e-8;

    let value64 = value as f64;
    let bits = value.to_bits();
    let sign = bits >> 31 != 0;
    let magnitude = bits & 0x7fff_ffff;
    if magnitude <= 0x3f49_0fda {
        if magnitude < 0x3980_0000 {
            return ReducedSin {
                argument: 0.0,
                cosine: false,
                negate: false,
                direct: Some(value),
            };
        }
        return ReducedSin {
            argument: value64,
            cosine: false,
            negate: false,
            direct: None,
        };
    }
    if magnitude <= 0x407b_53d1 {
        if magnitude <= 0x4016_cbe3 {
            return ReducedSin {
                argument: if sign {
                    value64 + FRAC_PI_2
                } else {
                    value64 - FRAC_PI_2
                },
                cosine: true,
                negate: sign,
                direct: None,
            };
        }
        return ReducedSin {
            argument: if sign {
                -(value64 + 2.0 * FRAC_PI_2)
            } else {
                -(value64 - 2.0 * FRAC_PI_2)
            },
            cosine: false,
            negate: false,
            direct: None,
        };
    }
    if magnitude <= 0x40e2_31d5 {
        if magnitude <= 0x40af_eddf {
            return ReducedSin {
                argument: if sign {
                    value64 + 3.0 * FRAC_PI_2
                } else {
                    value64 - 3.0 * FRAC_PI_2
                },
                cosine: true,
                negate: !sign,
                direct: None,
            };
        }
        return ReducedSin {
            argument: if sign {
                value64 + 4.0 * FRAC_PI_2
            } else {
                value64 - 4.0 * FRAC_PI_2
            },
            cosine: false,
            negate: false,
            direct: None,
        };
    }

    // This is the complete medium-size reduction used by libm::sinf. Values
    // beyond it require the scalar Payne-Hanek reducer and are uncommon in the
    // pinned graph, so retain libm exactly for those individual lanes.
    if magnitude >= 0x4dc9_0fdb {
        return ReducedSin {
            argument: 0.0,
            cosine: false,
            negate: false,
            direct: Some(libm::sinf(value)),
        };
    }
    let temporary = value64 * INV_PIO2 + TOINT;
    let quadrant_value = temporary - TOINT;
    let quadrant = quadrant_value as i32;
    let remainder = value64 - quadrant_value * PIO2_1 - quadrant_value * PIO2_1T;
    match quadrant & 3 {
        0 => ReducedSin {
            argument: remainder,
            cosine: false,
            negate: false,
            direct: None,
        },
        1 => ReducedSin {
            argument: remainder,
            cosine: true,
            negate: false,
            direct: None,
        },
        2 => ReducedSin {
            argument: -remainder,
            cosine: false,
            negate: false,
            direct: None,
        },
        _ => ReducedSin {
            argument: remainder,
            cosine: true,
            negate: true,
            direct: None,
        },
    }
}

#[target_feature(enable = "avx2")]
unsafe fn sin_avx2(input: &[f32], output: &mut [f32]) -> Result<(), Error> {
    const DOUBLE_LANES: usize = 4;
    let vector_end = input.len() / DOUBLE_LANES * DOUBLE_LANES;
    let mut index = 0usize;
    while index < vector_end {
        let reduced = [
            reduce_sin(input[index]),
            reduce_sin(input[index + 1]),
            reduce_sin(input[index + 2]),
            reduce_sin(input[index + 3]),
        ];
        let arguments = [
            reduced[0].argument,
            reduced[1].argument,
            reduced[2].argument,
            reduced[3].argument,
        ];
        let values = unsafe { _mm256_loadu_pd(arguments.as_ptr()) };
        let sine = unsafe { kernel_sin_f64(values) };
        let cosine = unsafe { kernel_cos_f64(values) };
        let mut sine_f32 = [0.0_f32; DOUBLE_LANES];
        let mut cosine_f32 = [0.0_f32; DOUBLE_LANES];
        unsafe {
            _mm_storeu_ps(sine_f32.as_mut_ptr(), _mm256_cvtpd_ps(sine));
            _mm_storeu_ps(cosine_f32.as_mut_ptr(), _mm256_cvtpd_ps(cosine));
        }
        for lane in 0..DOUBLE_LANES {
            let mut result = if let Some(direct) = reduced[lane].direct {
                direct
            } else if reduced[lane].cosine {
                cosine_f32[lane]
            } else {
                sine_f32[lane]
            };
            if reduced[lane].negate {
                result = -result;
            }
            output[index + lane] = result;
        }
        index += DOUBLE_LANES;
    }
    for index in vector_end..input.len() {
        output[index] = libm::sinf(input[index]);
    }
    Ok(())
}

#[inline]
#[target_feature(enable = "avx2")]
unsafe fn kernel_sin_f64(value: __m256d) -> __m256d {
    const S1: f64 = -0.166_666_666_416_265_24;
    const S2: f64 = 0.008_333_329_385_889_463;
    const S3: f64 = -0.000_198_393_348_360_966_32;
    const S4: f64 = 0.000_002_718_311_493_989_822;

    let squared = _mm256_mul_pd(value, value);
    let fourth = _mm256_mul_pd(squared, squared);
    let remainder = _mm256_add_pd(_mm256_set1_pd(S3), _mm256_mul_pd(squared, _mm256_set1_pd(S4)));
    let cubic = _mm256_mul_pd(squared, value);
    let leading = _mm256_add_pd(
        value,
        _mm256_mul_pd(
            cubic,
            _mm256_add_pd(_mm256_set1_pd(S1), _mm256_mul_pd(squared, _mm256_set1_pd(S2))),
        ),
    );
    _mm256_add_pd(leading, _mm256_mul_pd(_mm256_mul_pd(cubic, fourth), remainder))
}

#[inline]
#[target_feature(enable = "avx2")]
unsafe fn kernel_cos_f64(value: __m256d) -> __m256d {
    const C0: f64 = -0.499_999_997_251_031;
    const C1: f64 = 0.041_666_623_323_739_06;
    const C2: f64 = -0.001_388_676_377_460_993;
    const C3: f64 = 0.000_024_390_448_796_277_41;

    let squared = _mm256_mul_pd(value, value);
    let fourth = _mm256_mul_pd(squared, squared);
    let remainder = _mm256_add_pd(_mm256_set1_pd(C2), _mm256_mul_pd(squared, _mm256_set1_pd(C3)));
    let leading = _mm256_add_pd(
        _mm256_add_pd(_mm256_set1_pd(1.0), _mm256_mul_pd(squared, _mm256_set1_pd(C0))),
        _mm256_mul_pd(fourth, _mm256_set1_pd(C1)),
    );
    _mm256_add_pd(leading, _mm256_mul_pd(_mm256_mul_pd(fourth, squared), remainder))
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
        BinaryOperation::Div | BinaryOperation::DivIeee => _mm256_div_ps(lhs, rhs),
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
    if !operation.valid_output(operation.apply(lhs, rhs)) {
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
