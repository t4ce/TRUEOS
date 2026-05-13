#[inline]
pub(crate) fn bf16_to_f32(bits: u16) -> f32 {
    f32::from_bits((bits as u32) << 16)
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Bf16HelperSmoke {
    pub rows: usize,
    pub k_dim: usize,
    pub scalar_checksum: u32,
    pub sse2_checksum: u32,
    pub avx2_checksum: u32,
    pub sse2_ran: bool,
    pub avx2_ran: bool,
    pub max_abs_delta: f32,
}

pub(crate) fn exercise_bf16_helpers_once() -> Bf16HelperSmoke {
    const ROWS: usize = 5;
    const K_DIM: usize = 17;

    let mut x = [0.0f32; K_DIM];
    let mut w = [0u8; ROWS * K_DIM * 2];
    let mut scalar = [0.0f32; ROWS];
    let mut sse2 = [0.0f32; ROWS];
    let mut avx2 = [0.0f32; ROWS];

    for (idx, value) in x.iter_mut().enumerate() {
        *value = ((idx as f32) + 1.0) * 0.0625;
    }
    for row in 0..ROWS {
        for col in 0..K_DIM {
            let pattern = 0x3F80u16.wrapping_add(((row * 23 + col * 7) & 0x7f) as u16);
            let off = (row * K_DIM + col) * 2;
            let bytes = pattern.to_le_bytes();
            w[off] = bytes[0];
            w[off + 1] = bytes[1];
        }
    }

    let _bf16_probe = bf16_to_f32(u16::from_le_bytes([w[0], w[1]]));
    matvec_rows_bf16_scalar(&x, &w, K_DIM, &mut scalar, 0, ROWS);

    let mut smoke = Bf16HelperSmoke {
        rows: ROWS,
        k_dim: K_DIM,
        scalar_checksum: checksum_f32_bits(&scalar),
        ..Bf16HelperSmoke::default()
    };

    #[cfg(target_arch = "x86_64")]
    {
        unsafe {
            matvec_rows_bf16_sse2(&x, &w, K_DIM, &mut sse2, 0, ROWS);
        }
        smoke.sse2_ran = true;
        smoke.sse2_checksum = checksum_f32_bits(&sse2);
        smoke.max_abs_delta = smoke.max_abs_delta.max(max_abs_delta(&scalar, &sse2));

        if crate::cpu::simd_status().avx2_fma_ready {
            unsafe {
                matvec_rows_bf16_avx2_fma(&x, &w, K_DIM, &mut avx2, 0, ROWS);
            }
            smoke.avx2_ran = true;
            smoke.avx2_checksum = checksum_f32_bits(&avx2);
            smoke.max_abs_delta = smoke.max_abs_delta.max(max_abs_delta(&scalar, &avx2));
        }
    }

    smoke
}

#[embassy_executor::task]
pub(crate) async fn bf16_helper_boot_exercise_task() {
    embassy_time::Timer::after(embassy_time::Duration::from_millis(250)).await;
    let start = embassy_time_driver::now();
    let mut last = Bf16HelperSmoke::default();
    for _ in 0..8 {
        last = exercise_bf16_helpers_once();
        crate::time::poll();
    }
    let elapsed_ticks = embassy_time_driver::now().saturating_sub(start);
    crate::log!(
        "lumen-simd-help: bf16 helper boot exercise rows={} k_dim={} scalar=0x{:08X} sse2=0x{:08X} avx2=0x{:08X} sse2_ran={} avx2_ran={} max_abs_delta={:.8} ticks={}\n",
        last.rows,
        last.k_dim,
        last.scalar_checksum,
        last.sse2_checksum,
        last.avx2_checksum,
        last.sse2_ran,
        last.avx2_ran,
        last.max_abs_delta,
        elapsed_ticks
    );
}

fn checksum_f32_bits(values: &[f32]) -> u32 {
    let mut acc = 0xA5A5_5A5Au32;
    for (idx, value) in values.iter().enumerate() {
        acc ^= value.to_bits().rotate_left(((idx as u32) & 15) + 1);
        acc = acc.rotate_left(5).wrapping_add(0x9E37_79B9);
    }
    acc
}

fn max_abs_delta(a: &[f32], b: &[f32]) -> f32 {
    let mut max = 0.0f32;
    let len = a.len().min(b.len());
    for idx in 0..len {
        let delta = (a[idx] - b[idx]).abs();
        if delta > max {
            max = delta;
        }
    }
    max
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
        __m256, __m256i, _mm_loadu_si128, _mm256_add_ps, _mm256_castsi256_ps,
        _mm256_cvtepu16_epi32, _mm256_fmadd_ps, _mm256_loadu_ps, _mm256_setzero_ps,
        _mm256_slli_epi32, _mm256_storeu_ps,
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

        let mut acc = unsafe { reduce_f32x8(_mm256_add_ps(acc0, acc1)) };
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
        let mut acc4 = _mm256_setzero_ps();
        let mut acc5 = _mm256_setzero_ps();
        let mut acc6 = _mm256_setzero_ps();
        let mut acc7 = _mm256_setzero_ps();

        while idx + 16 <= k_dim {
            let x0 = unsafe { _mm256_loadu_ps(x.as_ptr().add(idx)) };
            let x1 = unsafe { _mm256_loadu_ps(x.as_ptr().add(idx + 8)) };
            let r00 = unsafe { load_bf16x8_as_f32(w0.add(idx * 2)) };
            let r01 = unsafe { load_bf16x8_as_f32(w0.add((idx + 8) * 2)) };
            let r10 = unsafe { load_bf16x8_as_f32(w1.add(idx * 2)) };
            let r11 = unsafe { load_bf16x8_as_f32(w1.add((idx + 8) * 2)) };
            let r20 = unsafe { load_bf16x8_as_f32(w2.add(idx * 2)) };
            let r21 = unsafe { load_bf16x8_as_f32(w2.add((idx + 8) * 2)) };
            let r30 = unsafe { load_bf16x8_as_f32(w3.add(idx * 2)) };
            let r31 = unsafe { load_bf16x8_as_f32(w3.add((idx + 8) * 2)) };
            acc0 = _mm256_fmadd_ps(x0, r00, acc0);
            acc1 = _mm256_fmadd_ps(x0, r10, acc1);
            acc2 = _mm256_fmadd_ps(x0, r20, acc2);
            acc3 = _mm256_fmadd_ps(x0, r30, acc3);
            acc4 = _mm256_fmadd_ps(x1, r01, acc4);
            acc5 = _mm256_fmadd_ps(x1, r11, acc5);
            acc6 = _mm256_fmadd_ps(x1, r21, acc6);
            acc7 = _mm256_fmadd_ps(x1, r31, acc7);
            idx += 16;
        }

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

        let mut sum0 = unsafe { reduce_f32x8(_mm256_add_ps(acc0, acc4)) };
        let mut sum1 = unsafe { reduce_f32x8(_mm256_add_ps(acc1, acc5)) };
        let mut sum2 = unsafe { reduce_f32x8(_mm256_add_ps(acc2, acc6)) };
        let mut sum3 = unsafe { reduce_f32x8(_mm256_add_ps(acc3, acc7)) };
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
