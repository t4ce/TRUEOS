#[inline]
pub(crate) fn bf16_to_f32(bits: u16) -> f32 {
    f32::from_bits((bits as u32) << 16)
}

#[allow(dead_code)]
pub(crate) fn matvec_rows_bf16_scalar(
    x: &[f32],
    w_rowmajor_bf16: &[u8],
    k_dim: usize,
    out: &mut [f32],
    row_start: usize,
    row_end: usize,
) {
    for row in row_start..row_end {
        let base = row * k_dim * 2;
        let weights = &w_rowmajor_bf16[base..base + k_dim * 2];
        let mut acc = 0.0f32;
        for idx in 0..k_dim {
            let off = idx * 2;
            let bits = u16::from_le_bytes([weights[off], weights[off + 1]]);
            acc += x[idx] * bf16_to_f32(bits);
        }
        out[row] = acc;
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
pub(crate) unsafe fn matvec_rows_bf16_avx2_fma(
    x: &[f32],
    w_rowmajor_bf16: &[u8],
    k_dim: usize,
    out: &mut [f32],
    row_start: usize,
    row_end: usize,
) {
    use core::arch::x86_64::{
        __m256, __m256i, _mm_loadu_si128, _mm256_castsi256_ps, _mm256_cvtepu16_epi32,
        _mm256_fmadd_ps, _mm256_loadu_ps, _mm256_setzero_ps, _mm256_slli_epi32, _mm256_storeu_ps,
    };

    #[inline(always)]
    unsafe fn load_bf16x8_as_f32(ptr: *const u8) -> __m256 {
        let raw = unsafe { _mm_loadu_si128(ptr.cast::<core::arch::x86_64::__m128i>()) };
        let widened: __m256i = _mm256_cvtepu16_epi32(raw);
        _mm256_castsi256_ps(_mm256_slli_epi32(widened, 16))
    }

    #[inline(always)]
    unsafe fn reduce_f32x8(v: __m256) -> f32 {
        let mut lanes = [0.0f32; 8];
        unsafe { _mm256_storeu_ps(lanes.as_mut_ptr(), v) };
        lanes[0] + lanes[1] + lanes[2] + lanes[3] + lanes[4] + lanes[5] + lanes[6] + lanes[7]
    }

    #[inline(always)]
    unsafe fn compute_row(
        x: &[f32],
        weights: *const u8,
        k_dim: usize,
        out: &mut [f32],
        row: usize,
    ) {
        let mut idx = 0usize;
        let mut acc0 = _mm256_setzero_ps();
        let mut acc1 = _mm256_setzero_ps();

        while idx + 16 <= k_dim {
            let x0 = unsafe { _mm256_loadu_ps(x.as_ptr().add(idx)) };
            let x1 = unsafe { _mm256_loadu_ps(x.as_ptr().add(idx + 8)) };
            let w0 = unsafe { load_bf16x8_as_f32(weights.add(idx * 2)) };
            let w1 = unsafe { load_bf16x8_as_f32(weights.add((idx + 8) * 2)) };
            acc0 = _mm256_fmadd_ps(x0, w0, acc0);
            acc1 = _mm256_fmadd_ps(x1, w1, acc1);
            idx += 16;
        }

        while idx + 8 <= k_dim {
            let x0 = unsafe { _mm256_loadu_ps(x.as_ptr().add(idx)) };
            let w0 = unsafe { load_bf16x8_as_f32(weights.add(idx * 2)) };
            acc0 = _mm256_fmadd_ps(x0, w0, acc0);
            idx += 8;
        }

        let mut acc = unsafe { reduce_f32x8(_mm256_add_ps_inline(acc0, acc1)) };
        while idx < k_dim {
            let off = idx * 2;
            let lo = unsafe { *weights.add(off) };
            let hi = unsafe { *weights.add(off + 1) };
            let bits = u16::from_le_bytes([lo, hi]);
            acc += x[idx] * bf16_to_f32(bits);
            idx += 1;
        }
        out[row] = acc;
    }

    #[inline(always)]
    unsafe fn compute_4rows(
        x: &[f32],
        w_rowmajor_bf16: &[u8],
        k_dim: usize,
        out: &mut [f32],
        row: usize,
    ) {
        let row_stride = k_dim * 2;
        let base = row * row_stride;
        let w0 = unsafe { w_rowmajor_bf16.as_ptr().add(base) };
        let w1 = unsafe { w0.add(row_stride) };
        let w2 = unsafe { w1.add(row_stride) };
        let w3 = unsafe { w2.add(row_stride) };
        let mut idx = 0usize;
        let mut acc0 = _mm256_setzero_ps();
        let mut acc1 = _mm256_setzero_ps();
        let mut acc2 = _mm256_setzero_ps();
        let mut acc3 = _mm256_setzero_ps();

        while idx + 8 <= k_dim {
            let xv = unsafe { _mm256_loadu_ps(x.as_ptr().add(idx)) };
            let r0 = unsafe { load_bf16x8_as_f32(w0.add(idx * 2)) };
            let r1 = unsafe { load_bf16x8_as_f32(w1.add(idx * 2)) };
            let r2 = unsafe { load_bf16x8_as_f32(w2.add(idx * 2)) };
            let r3 = unsafe { load_bf16x8_as_f32(w3.add(idx * 2)) };
            acc0 = _mm256_fmadd_ps(xv, r0, acc0);
            acc1 = _mm256_fmadd_ps(xv, r1, acc1);
            acc2 = _mm256_fmadd_ps(xv, r2, acc2);
            acc3 = _mm256_fmadd_ps(xv, r3, acc3);
            idx += 8;
        }

        let mut sum0 = unsafe { reduce_f32x8(acc0) };
        let mut sum1 = unsafe { reduce_f32x8(acc1) };
        let mut sum2 = unsafe { reduce_f32x8(acc2) };
        let mut sum3 = unsafe { reduce_f32x8(acc3) };
        while idx < k_dim {
            let off = idx * 2;
            let bits0 = u16::from_le_bytes([unsafe { *w0.add(off) }, unsafe { *w0.add(off + 1) }]);
            let bits1 = u16::from_le_bytes([unsafe { *w1.add(off) }, unsafe { *w1.add(off + 1) }]);
            let bits2 = u16::from_le_bytes([unsafe { *w2.add(off) }, unsafe { *w2.add(off + 1) }]);
            let bits3 = u16::from_le_bytes([unsafe { *w3.add(off) }, unsafe { *w3.add(off + 1) }]);
            let xv = x[idx];
            sum0 += xv * bf16_to_f32(bits0);
            sum1 += xv * bf16_to_f32(bits1);
            sum2 += xv * bf16_to_f32(bits2);
            sum3 += xv * bf16_to_f32(bits3);
            idx += 1;
        }
        out[row] = sum0;
        out[row + 1] = sum1;
        out[row + 2] = sum2;
        out[row + 3] = sum3;
    }

    #[inline(always)]
    unsafe fn _mm256_add_ps_inline(a: __m256, b: __m256) -> __m256 {
        core::arch::x86_64::_mm256_add_ps(a, b)
    }

    let mut row = row_start;
    while row + 4 <= row_end {
        unsafe { compute_4rows(x, w_rowmajor_bf16, k_dim, out, row) };
        row += 4;
    }
    while row < row_end {
        let base = row * k_dim * 2;
        let weights = unsafe { w_rowmajor_bf16.as_ptr().add(base) };
        unsafe { compute_row(x, weights, k_dim, out, row) };
        row += 1;
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
pub(crate) unsafe fn matvec_rows_bf16_sse2(
    x: &[f32],
    w_rowmajor_bf16: &[u8],
    k_dim: usize,
    out: &mut [f32],
    row_start: usize,
    row_end: usize,
) {
    use core::arch::x86_64::{
        __m128, __m128i, _mm_add_ps, _mm_castsi128_ps, _mm_loadl_epi64, _mm_loadu_ps, _mm_mul_ps,
        _mm_setzero_ps, _mm_setzero_si128, _mm_slli_epi32, _mm_storeu_ps, _mm_unpacklo_epi16,
    };

    #[inline(always)]
    unsafe fn load_bf16x4_as_f32(ptr: *const u8) -> __m128 {
        let raw = unsafe { _mm_loadl_epi64(ptr.cast::<__m128i>()) };
        let widened = _mm_unpacklo_epi16(raw, _mm_setzero_si128());
        _mm_castsi128_ps(_mm_slli_epi32(widened, 16))
    }

    #[inline(always)]
    unsafe fn reduce_f32x4(v: __m128) -> f32 {
        let mut lanes = [0.0f32; 4];
        unsafe { _mm_storeu_ps(lanes.as_mut_ptr(), v) };
        lanes[0] + lanes[1] + lanes[2] + lanes[3]
    }

    for row in row_start..row_end {
        let base = row * k_dim * 2;
        let weights = unsafe { w_rowmajor_bf16.as_ptr().add(base) };
        let mut idx = 0usize;
        let mut acc0 = _mm_setzero_ps();
        let mut acc1 = _mm_setzero_ps();

        while idx + 8 <= k_dim {
            let x0 = unsafe { _mm_loadu_ps(x.as_ptr().add(idx)) };
            let x1 = unsafe { _mm_loadu_ps(x.as_ptr().add(idx + 4)) };
            let w0 = unsafe { load_bf16x4_as_f32(weights.add(idx * 2)) };
            let w1 = unsafe { load_bf16x4_as_f32(weights.add((idx + 4) * 2)) };
            acc0 = _mm_add_ps(acc0, _mm_mul_ps(x0, w0));
            acc1 = _mm_add_ps(acc1, _mm_mul_ps(x1, w1));
            idx += 8;
        }

        while idx + 4 <= k_dim {
            let x0 = unsafe { _mm_loadu_ps(x.as_ptr().add(idx)) };
            let w0 = unsafe { load_bf16x4_as_f32(weights.add(idx * 2)) };
            acc0 = _mm_add_ps(acc0, _mm_mul_ps(x0, w0));
            idx += 4;
        }

        let mut acc = unsafe { reduce_f32x4(acc0) + reduce_f32x4(acc1) };
        while idx < k_dim {
            let off = idx * 2;
            let lo = unsafe { *weights.add(off) };
            let hi = unsafe { *weights.add(off + 1) };
            let bits = u16::from_le_bytes([lo, hi]);
            acc += x[idx] * bf16_to_f32(bits);
            idx += 1;
        }
        out[row] = acc;
    }
}
