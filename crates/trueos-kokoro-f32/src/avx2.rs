//! The crate's sole unsafe island.
//!
//! Every public-to-private entry is safe: it verifies AVX/OSXSAVE/AVX2 and
//! XCR0 XMM+YMM state, checks equal slice lengths, and only then calls a
//! target-feature function. All vector memory operations stay inside those
//! validated slices and use unaligned loads/stores.

use core::arch::x86_64::{
    __cpuid, __cpuid_count, __m256, _mm_add_ps, _mm_div_ps, _mm_loadu_ps, _mm_mul_ps, _mm_set1_ps,
    _mm_storeu_ps, _mm_sub_ps, _mm256_add_ps, _mm256_and_si256, _mm256_castps_si256,
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
    // above. The target function deliberately evaluates four-float packets to
    // match the Eigen SSE2 implementation in the pinned ORT wheel.
    unsafe { sin_avx2(input, output) }
}

pub(super) fn atan(input: &[f32], output: &mut [f32], allow_infinite: bool) -> Result<(), Error> {
    if input.len() != output.len() {
        return Err(Error::BufferTooSmall);
    }
    if !is_available() {
        return Err(Error::UnsupportedLane);
    }
    if input
        .iter()
        .any(|value| value.is_nan() || !allow_infinite && value.is_infinite())
    {
        return Err(Error::NonFiniteInput);
    }
    // SAFETY: availability, complete slice bounds, and the caller-selected
    // finite/infinite input contract are proven above.
    unsafe { atan_avx2(input, output) }
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

#[target_feature(enable = "avx2")]
unsafe fn sin_avx2(input: &[f32], output: &mut [f32]) -> Result<(), Error> {
    // The official ORT wheel compiles Eigen's ArrayMap.sin() translation unit
    // for Packet4f/SSE2, even on an AVX2/FMA host. Packet width matters because
    // Eigen sends only the final `len % 4` elements through scalar sinf.
    const EIGEN_LANES: usize = 4;
    let vector_end = input.len() / EIGEN_LANES * EIGEN_LANES;
    let mut index = 0usize;
    while index < vector_end {
        let packet = unsafe { _mm_loadu_ps(input.as_ptr().add(index)) };
        let result = unsafe { eigen_packet4_sin(packet) };
        unsafe { _mm_storeu_ps(output.as_mut_ptr().add(index), result) };
        index += EIGEN_LANES;
    }
    for index in vector_end..input.len() {
        // Eigen's scalar_sin_op<float> resolves to the platform sinf. The
        // no-std libm implementation has the same pinned scalar-tail result.
        output[index] = libm::sinf(input[index]);
    }
    Ok(())
}

#[target_feature(enable = "avx2")]
unsafe fn atan_avx2(input: &[f32], output: &mut [f32]) -> Result<(), Error> {
    const EIGEN_LANES: usize = 4;
    let vector_end = input.len() / EIGEN_LANES * EIGEN_LANES;
    let mut index = 0usize;
    while index < vector_end {
        let packet = unsafe { _mm_loadu_ps(input.as_ptr().add(index)) };
        let result = unsafe { eigen_packet4_atan(packet) };
        unsafe { _mm_storeu_ps(output.as_mut_ptr().add(index), result) };
        index += EIGEN_LANES;
    }
    for index in vector_end..input.len() {
        output[index] = libm::atanf(input[index]);
    }
    Ok(())
}

#[inline]
#[target_feature(enable = "avx2")]
unsafe fn eigen_packet4_atan(input: core::arch::x86_64::__m128) -> core::arch::x86_64::__m128 {
    const PI_OVER_TWO: f32 = f32::from_bits(0x3FC9_0FDB);

    let mut original = [0.0f32; 4];
    unsafe { _mm_storeu_ps(original.as_mut_ptr(), input) };
    let absolute = original.map(|value| f32::from_bits(value.to_bits() & 0x7FFF_FFFF));
    let absolute_packet = unsafe { _mm_loadu_ps(absolute.as_ptr()) };

    // Eigen specializes Packet4f reciprocal to rcp+Newton only in an FMA
    // translation unit. The official wheel's baseline SSE2 unit falls through
    // to pdiv(1, x), so this exact division is observable in t3177.
    let reciprocal = _mm_div_ps(_mm_set1_ps(1.0), absolute_packet);
    let mut reciprocal_lanes = [0.0f32; 4];
    unsafe { _mm_storeu_ps(reciprocal_lanes.as_mut_ptr(), reciprocal) };
    let reduced_lanes: [f32; 4] = core::array::from_fn(|lane| {
        if absolute[lane] > 1.0 {
            reciprocal_lanes[lane]
        } else {
            absolute[lane]
        }
    });
    let reduced = unsafe { _mm_loadu_ps(reduced_lanes.as_ptr()) };
    let squared = _mm_mul_ps(reduced, reduced);

    let mut numerator = _mm_set1_ps(f32::from_bits(0x3DE5_6E67));
    numerator =
        _mm_add_ps(_mm_mul_ps(numerator, squared), _mm_set1_ps(f32::from_bits(0x3F3A_CBA0)));
    numerator =
        _mm_add_ps(_mm_mul_ps(numerator, squared), _mm_set1_ps(f32::from_bits(0x3F4F_9D60)));

    let mut denominator = _mm_set1_ps(f32::from_bits(0x3C25_57B4));
    denominator =
        _mm_add_ps(_mm_mul_ps(denominator, squared), _mm_set1_ps(f32::from_bits(0x3E90_FDB4)));
    denominator = _mm_add_ps(_mm_mul_ps(denominator, squared), _mm_set1_ps(1.0));
    denominator =
        _mm_add_ps(_mm_mul_ps(denominator, squared), _mm_set1_ps(f32::from_bits(0x3F4F_9D60)));

    let polynomial = _mm_mul_ps(reduced, _mm_div_ps(numerator, denominator));
    let mut polynomial_lanes = [0.0f32; 4];
    unsafe { _mm_storeu_ps(polynomial_lanes.as_mut_ptr(), polynomial) };
    let result: [f32; 4] = core::array::from_fn(|lane| {
        let magnitude = if absolute[lane] > 1.0 {
            PI_OVER_TWO - polynomial_lanes[lane]
        } else {
            polynomial_lanes[lane]
        };
        f32::from_bits(magnitude.to_bits() ^ (original[lane].to_bits() & 0x8000_0000))
    });
    unsafe { _mm_loadu_ps(result.as_ptr()) }
}

#[inline]
#[target_feature(enable = "avx2")]
unsafe fn eigen_packet4_sin(input: core::arch::x86_64::__m128) -> core::arch::x86_64::__m128 {
    const TWO_OVER_PI: f32 = f32::from_bits(0x3F22_F983);
    const ROUNDING_MAGIC: f32 = f32::from_bits(0x4B40_0000);
    const HUGE_THRESHOLD: f32 = f32::from_bits(0x46CA_DC00);

    let mut original = [0.0f32; 4];
    unsafe { _mm_storeu_ps(original.as_mut_ptr(), input) };
    let absolute = original.map(|value| f32::from_bits(value.to_bits() & 0x7FFF_FFFF));
    let mut reduced = unsafe { _mm_loadu_ps(absolute.as_ptr()) };

    let scaled = _mm_mul_ps(reduced, _mm_set1_ps(TWO_OVER_PI));
    let rounded_with_magic = _mm_add_ps(scaled, _mm_set1_ps(ROUNDING_MAGIC));
    let quadrant = _mm_sub_ps(rounded_with_magic, _mm_set1_ps(ROUNDING_MAGIC));
    let mut quadrant_floats = [0.0f32; 4];
    unsafe { _mm_storeu_ps(quadrant_floats.as_mut_ptr(), rounded_with_magic) };
    let mut quadrant_bits = quadrant_floats.map(f32::to_bits);

    // This ORT build has no EIGEN_VECTORIZE_FMA in this translation unit.
    // Keep every pmadd as an explicit, separately rounded multiply then add.
    reduced = _mm_add_ps(_mm_mul_ps(quadrant, _mm_set1_ps(f32::from_bits(0xBFC9_0000))), reduced);
    reduced = _mm_add_ps(_mm_mul_ps(quadrant, _mm_set1_ps(f32::from_bits(0xB9FD_C000))), reduced);
    reduced = _mm_add_ps(_mm_mul_ps(quadrant, _mm_set1_ps(f32::from_bits(0x342E_E000))), reduced);
    reduced = _mm_add_ps(_mm_mul_ps(quadrant, _mm_set1_ps(f32::from_bits(0x2E74_B9EE))), reduced);

    // Eigen switches individual large lanes to its scalar Payne-Hanek reducer
    // and then resumes the same packet polynomial. This path is outside the
    // Kokoro checkpoint range but keeps the public finite-input contract whole.
    if absolute.iter().any(|&value| value >= HUGE_THRESHOLD) {
        let mut reduced_lanes = [0.0f32; 4];
        unsafe { _mm_storeu_ps(reduced_lanes.as_mut_ptr(), reduced) };
        for lane in 0..4 {
            if absolute[lane] >= HUGE_THRESHOLD {
                let (lane_reduced, lane_quadrant) = eigen_trig_reduce_huge(absolute[lane]);
                reduced_lanes[lane] = lane_reduced;
                quadrant_bits[lane] = lane_quadrant;
            }
        }
        reduced = unsafe { _mm_loadu_ps(reduced_lanes.as_ptr()) };
    }

    let squared = _mm_mul_ps(reduced, reduced);

    let mut cosine = _mm_set1_ps(f32::from_bits(0x37CC_730B));
    cosine = _mm_add_ps(_mm_mul_ps(cosine, squared), _mm_set1_ps(f32::from_bits(0xBAB6_036E)));
    cosine = _mm_add_ps(_mm_mul_ps(cosine, squared), _mm_set1_ps(f32::from_bits(0x3D2A_AA9E)));
    cosine = _mm_add_ps(_mm_mul_ps(cosine, squared), _mm_set1_ps(-0.5));
    cosine = _mm_add_ps(_mm_mul_ps(cosine, squared), _mm_set1_ps(1.0));

    let mut sine = _mm_set1_ps(f32::from_bits(0xB94D_70CA));
    sine = _mm_add_ps(_mm_mul_ps(sine, squared), _mm_set1_ps(f32::from_bits(0x3C08_85D3)));
    sine = _mm_add_ps(_mm_mul_ps(sine, squared), _mm_set1_ps(f32::from_bits(0xBE2A_AAA8)));
    sine = _mm_mul_ps(sine, squared);
    sine = _mm_add_ps(_mm_mul_ps(sine, reduced), reduced);

    let mut sine_lanes = [0.0f32; 4];
    let mut cosine_lanes = [0.0f32; 4];
    unsafe {
        _mm_storeu_ps(sine_lanes.as_mut_ptr(), sine);
        _mm_storeu_ps(cosine_lanes.as_mut_ptr(), cosine);
    }
    let mut result = [0.0f32; 4];
    for lane in 0..4 {
        let magnitude = if quadrant_bits[lane] & 1 == 0 {
            sine_lanes[lane]
        } else {
            cosine_lanes[lane]
        };
        let sign = (original[lane].to_bits() ^ quadrant_bits[lane].wrapping_shl(30)) & 0x8000_0000;
        result[lane] = f32::from_bits(magnitude.to_bits() ^ sign);
    }
    unsafe { _mm_loadu_ps(result.as_ptr()) }
}

fn eigen_trig_reduce_huge(value: f32) -> (f32, u32) {
    const PIO2_62: f64 = 3.406_121_580_086_554_5e-19;
    const HALF_FIXED: u64 = 1_u64 << 61;
    const TWO_OVER_PI: [u32; 26] = [
        0x0000_0028,
        0x0000_28BE,
        0x0028_BE60,
        0x28BE_60DB,
        0xBE60_DB93,
        0x60DB_9391,
        0xDB93_9105,
        0x9391_054A,
        0x9105_4A7F,
        0x054A_7F09,
        0x4A7F_09D5,
        0x7F09_D5F4,
        0x09D5_F47D,
        0xD5F4_7D4D,
        0xF47D_4D37,
        0x7D4D_3770,
        0x4D37_7036,
        0x3770_36D8,
        0x7036_D8A5,
        0x36D8_A566,
        0xD8A5_664F,
        0xA566_4F10,
        0x664F_10E4,
        0x4F10_E410,
        0x10E4_1000,
        0xE410_0000,
    ];

    let mut significand = value.to_bits();
    let exponent = (significand >> 23).wrapping_sub(118);
    significand = ((significand & 0x007F_FFFF) | 0x0080_0000) << (exponent & 7);
    let table = (exponent >> 3) as usize;

    let mut product = u64::from(significand) * u64::from(TWO_OVER_PI[table + 7]);
    product = u64::from(significand) * u64::from(TWO_OVER_PI[table + 3]) + (product >> 32);
    product =
        (u64::from(significand.wrapping_mul(TWO_OVER_PI[table - 1])) << 32).wrapping_add(product);
    let quadrant = product.wrapping_add(HALF_FIXED) >> 62;
    product = product.wrapping_sub(quadrant << 62);
    let reduced = ((product as i64) as f64 * PIO2_62) as f32;
    (reduced, quadrant as u32)
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
