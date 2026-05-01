use crate::arch;

#[cfg(all(feature = "x86-int8-kernels", target_arch = "x86"))]
use std::arch::x86::__m256;
#[cfg(all(feature = "x86-int8-kernels", target_arch = "x86_64"))]
use std::arch::x86_64::__m256;

#[inline]
fn dot_len_matches(x: &[f32], row: &[i8]) -> bool {
    x.len() == row.len()
}

#[inline]
fn dot2_len_matches(x: &[f32], row0: &[i8], row1: &[i8]) -> bool {
    x.len() == row0.len() && x.len() == row1.len()
}

#[inline]
fn dot3_len_matches(x: &[f32], row0: &[i8], row1: &[i8], row2: &[i8]) -> bool {
    x.len() == row0.len() && x.len() == row1.len() && x.len() == row2.len()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Int8KernelBackend {
    Portable,
    Arm64Neon,
    X86Avx2,
}

#[inline]
pub fn active_int8_backend() -> Int8KernelBackend {
    if arch::arm64_i8_kernel_runtime_available() {
        Int8KernelBackend::Arm64Neon
    } else if arch::x86_i8_kernel_runtime_available() {
        Int8KernelBackend::X86Avx2
    } else {
        Int8KernelBackend::Portable
    }
}

#[inline]
pub fn active_int8_backend_name() -> &'static str {
    match active_int8_backend() {
        Int8KernelBackend::Portable => "portable",
        Int8KernelBackend::Arm64Neon => "arm64-neon",
        Int8KernelBackend::X86Avx2 => "x86-avx2",
    }
}

#[inline]
pub fn dot_f32_i8_arch(_x: &[f32], _row: &[i8], _scale: f32) -> Option<f32> {
    if !dot_len_matches(_x, _row) {
        return None;
    }

    match active_int8_backend() {
        Int8KernelBackend::Portable => None,
        #[cfg(all(feature = "arm64-int8-kernels", target_arch = "aarch64"))]
        Int8KernelBackend::Arm64Neon => Some(unsafe { dot_f32_i8_arm64_neon(_x, _row, _scale) }),
        #[cfg(all(
            feature = "x86-int8-kernels",
            any(target_arch = "x86_64", target_arch = "x86")
        ))]
        Int8KernelBackend::X86Avx2 => Some(unsafe { dot_f32_i8_x86_avx2(_x, _row, _scale) }),
        #[allow(unreachable_patterns)]
        _ => None,
    }
}

#[inline]
pub fn dot2_f32_i8_arch(
    _x: &[f32],
    _row0: &[i8],
    _scale0: f32,
    _row1: &[i8],
    _scale1: f32,
) -> Option<(f32, f32)> {
    if !dot2_len_matches(_x, _row0, _row1) {
        return None;
    }

    match active_int8_backend() {
        Int8KernelBackend::Portable => None,
        #[cfg(all(feature = "arm64-int8-kernels", target_arch = "aarch64"))]
        Int8KernelBackend::Arm64Neon => {
            Some(unsafe { dot2_f32_i8_arm64_neon(_x, _row0, _scale0, _row1, _scale1) })
        }
        #[cfg(all(
            feature = "x86-int8-kernels",
            any(target_arch = "x86_64", target_arch = "x86")
        ))]
        Int8KernelBackend::X86Avx2 => {
            Some(unsafe { dot2_f32_i8_x86_avx2(_x, _row0, _scale0, _row1, _scale1) })
        }
        #[allow(unreachable_patterns)]
        _ => None,
    }
}

pub fn dot3_f32_i8_arch(
    _x: &[f32],
    _row0: &[i8],
    _scale0: f32,
    _row1: &[i8],
    _scale1: f32,
    _row2: &[i8],
    _scale2: f32,
) -> Option<(f32, f32, f32)> {
    if !dot3_len_matches(_x, _row0, _row1, _row2) {
        return None;
    }

    match active_int8_backend() {
        Int8KernelBackend::Portable => None,
        #[cfg(all(feature = "arm64-int8-kernels", target_arch = "aarch64"))]
        Int8KernelBackend::Arm64Neon => Some(unsafe {
            dot3_f32_i8_arm64_neon(_x, _row0, _scale0, _row1, _scale1, _row2, _scale2)
        }),
        #[cfg(all(
            feature = "x86-int8-kernels",
            any(target_arch = "x86_64", target_arch = "x86")
        ))]
        Int8KernelBackend::X86Avx2 => Some(unsafe {
            dot3_f32_i8_x86_avx2(_x, _row0, _scale0, _row1, _scale1, _row2, _scale2)
        }),
        #[allow(unreachable_patterns)]
        _ => None,
    }
}

#[cfg(all(feature = "arm64-int8-kernels", target_arch = "aarch64"))]
#[target_feature(enable = "neon")]
unsafe fn dot2_f32_i8_arm64_neon(
    x: &[f32],
    row0: &[i8],
    scale0: f32,
    row1: &[i8],
    scale1: f32,
) -> (f32, f32) {
    use std::arch::aarch64::*;

    let k_dim = x.len();
    let mut kk = 0usize;
    let mut acc00 = vdupq_n_f32(0.0);
    let mut acc01 = vdupq_n_f32(0.0);
    let mut acc10 = vdupq_n_f32(0.0);
    let mut acc11 = vdupq_n_f32(0.0);

    while kk + 8 <= k_dim {
        let row0_8 = unsafe { vld1_s8(row0.as_ptr().add(kk)) };
        let row1_8 = unsafe { vld1_s8(row1.as_ptr().add(kk)) };
        let row0_16 = vmovl_s8(row0_8);
        let row1_16 = vmovl_s8(row1_8);
        let row0_lo = vcvtq_f32_s32(vmovl_s16(vget_low_s16(row0_16)));
        let row0_hi = vcvtq_f32_s32(vmovl_s16(vget_high_s16(row0_16)));
        let row1_lo = vcvtq_f32_s32(vmovl_s16(vget_low_s16(row1_16)));
        let row1_hi = vcvtq_f32_s32(vmovl_s16(vget_high_s16(row1_16)));
        let x_lo = unsafe { vld1q_f32(x.as_ptr().add(kk)) };
        let x_hi = unsafe { vld1q_f32(x.as_ptr().add(kk + 4)) };
        acc00 = vfmaq_f32(acc00, row0_lo, x_lo);
        acc01 = vfmaq_f32(acc01, row0_hi, x_hi);
        acc10 = vfmaq_f32(acc10, row1_lo, x_lo);
        acc11 = vfmaq_f32(acc11, row1_hi, x_hi);
        kk += 8;
    }

    let mut sum0 = vaddvq_f32(acc00) + vaddvq_f32(acc01);
    let mut sum1 = vaddvq_f32(acc10) + vaddvq_f32(acc11);
    while kk < k_dim {
        let xv = x[kk];
        sum0 += row0[kk] as f32 * xv;
        sum1 += row1[kk] as f32 * xv;
        kk += 1;
    }
    (sum0 * scale0, sum1 * scale1)
}

#[cfg(all(feature = "arm64-int8-kernels", target_arch = "aarch64"))]
#[target_feature(enable = "neon")]
unsafe fn dot3_f32_i8_arm64_neon(
    x: &[f32],
    row0: &[i8],
    scale0: f32,
    row1: &[i8],
    scale1: f32,
    row2: &[i8],
    scale2: f32,
) -> (f32, f32, f32) {
    use std::arch::aarch64::*;

    let k_dim = x.len();
    let mut kk = 0usize;
    let mut acc00 = vdupq_n_f32(0.0);
    let mut acc01 = vdupq_n_f32(0.0);
    let mut acc10 = vdupq_n_f32(0.0);
    let mut acc11 = vdupq_n_f32(0.0);
    let mut acc20 = vdupq_n_f32(0.0);
    let mut acc21 = vdupq_n_f32(0.0);

    while kk + 8 <= k_dim {
        let row0_8 = unsafe { vld1_s8(row0.as_ptr().add(kk)) };
        let row1_8 = unsafe { vld1_s8(row1.as_ptr().add(kk)) };
        let row2_8 = unsafe { vld1_s8(row2.as_ptr().add(kk)) };
        let row0_16 = vmovl_s8(row0_8);
        let row1_16 = vmovl_s8(row1_8);
        let row2_16 = vmovl_s8(row2_8);
        let row0_lo = vcvtq_f32_s32(vmovl_s16(vget_low_s16(row0_16)));
        let row0_hi = vcvtq_f32_s32(vmovl_s16(vget_high_s16(row0_16)));
        let row1_lo = vcvtq_f32_s32(vmovl_s16(vget_low_s16(row1_16)));
        let row1_hi = vcvtq_f32_s32(vmovl_s16(vget_high_s16(row1_16)));
        let row2_lo = vcvtq_f32_s32(vmovl_s16(vget_low_s16(row2_16)));
        let row2_hi = vcvtq_f32_s32(vmovl_s16(vget_high_s16(row2_16)));
        let x_lo = unsafe { vld1q_f32(x.as_ptr().add(kk)) };
        let x_hi = unsafe { vld1q_f32(x.as_ptr().add(kk + 4)) };
        acc00 = vfmaq_f32(acc00, row0_lo, x_lo);
        acc01 = vfmaq_f32(acc01, row0_hi, x_hi);
        acc10 = vfmaq_f32(acc10, row1_lo, x_lo);
        acc11 = vfmaq_f32(acc11, row1_hi, x_hi);
        acc20 = vfmaq_f32(acc20, row2_lo, x_lo);
        acc21 = vfmaq_f32(acc21, row2_hi, x_hi);
        kk += 8;
    }

    let mut sum0 = vaddvq_f32(acc00) + vaddvq_f32(acc01);
    let mut sum1 = vaddvq_f32(acc10) + vaddvq_f32(acc11);
    let mut sum2 = vaddvq_f32(acc20) + vaddvq_f32(acc21);
    while kk < k_dim {
        let xv = x[kk];
        sum0 += row0[kk] as f32 * xv;
        sum1 += row1[kk] as f32 * xv;
        sum2 += row2[kk] as f32 * xv;
        kk += 1;
    }
    (sum0 * scale0, sum1 * scale1, sum2 * scale2)
}

#[cfg(all(feature = "arm64-int8-kernels", target_arch = "aarch64"))]
#[target_feature(enable = "neon")]
unsafe fn dot_f32_i8_arm64_neon(x: &[f32], row: &[i8], scale: f32) -> f32 {
    use std::arch::aarch64::*;

    let k_dim = x.len();
    let mut kk = 0usize;
    let mut acc0 = vdupq_n_f32(0.0);
    let mut acc1 = vdupq_n_f32(0.0);

    while kk + 8 <= k_dim {
        let row8 = unsafe { vld1_s8(row.as_ptr().add(kk)) };
        let row16 = vmovl_s8(row8);
        let row_lo_i32 = vmovl_s16(vget_low_s16(row16));
        let row_hi_i32 = vmovl_s16(vget_high_s16(row16));
        let row_lo = vcvtq_f32_s32(row_lo_i32);
        let row_hi = vcvtq_f32_s32(row_hi_i32);
        let x_lo = unsafe { vld1q_f32(x.as_ptr().add(kk)) };
        let x_hi = unsafe { vld1q_f32(x.as_ptr().add(kk + 4)) };
        acc0 = vfmaq_f32(acc0, row_lo, x_lo);
        acc1 = vfmaq_f32(acc1, row_hi, x_hi);
        kk += 8;
    }

    let mut sum = vaddvq_f32(acc0) + vaddvq_f32(acc1);
    while kk < k_dim {
        sum += row[kk] as f32 * x[kk];
        kk += 1;
    }
    sum * scale
}

#[cfg(all(
    feature = "x86-int8-kernels",
    any(target_arch = "x86_64", target_arch = "x86")
))]
#[inline]
#[target_feature(enable = "avx2")]
unsafe fn reduce_f32x8_x86(v: __m256) -> f32 {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::*;
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;

    let mut buf = [0.0f32; 8];
    unsafe {
        _mm256_storeu_ps(buf.as_mut_ptr(), v);
    }
    buf.iter().sum()
}

#[cfg(all(
    feature = "x86-int8-kernels",
    any(target_arch = "x86_64", target_arch = "x86")
))]
#[inline]
#[target_feature(enable = "avx2")]
unsafe fn load_i8_as_f32x8_x86(ptr: *const i8) -> __m256 {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::*;
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;

    let raw = unsafe { _mm_loadl_epi64(ptr as *const __m128i) };
    _mm256_cvtepi32_ps(_mm256_cvtepi8_epi32(raw))
}

#[cfg(all(
    feature = "x86-int8-kernels",
    any(target_arch = "x86_64", target_arch = "x86")
))]
#[target_feature(enable = "avx2,fma")]
unsafe fn dot_f32_i8_x86_avx2(x: &[f32], row: &[i8], scale: f32) -> f32 {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::*;
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;

    let k_dim = x.len();
    let mut kk = 0usize;
    let mut acc0 = _mm256_setzero_ps();
    let mut acc1 = _mm256_setzero_ps();

    while kk + 16 <= k_dim {
        let row_lo = unsafe { load_i8_as_f32x8_x86(row.as_ptr().add(kk)) };
        let row_hi = unsafe { load_i8_as_f32x8_x86(row.as_ptr().add(kk + 8)) };
        let x_lo = unsafe { _mm256_loadu_ps(x.as_ptr().add(kk)) };
        let x_hi = unsafe { _mm256_loadu_ps(x.as_ptr().add(kk + 8)) };
        acc0 = _mm256_fmadd_ps(row_lo, x_lo, acc0);
        acc1 = _mm256_fmadd_ps(row_hi, x_hi, acc1);
        kk += 16;
    }

    while kk + 8 <= k_dim {
        let row_f32 = unsafe { load_i8_as_f32x8_x86(row.as_ptr().add(kk)) };
        let x_chunk = unsafe { _mm256_loadu_ps(x.as_ptr().add(kk)) };
        acc0 = _mm256_fmadd_ps(row_f32, x_chunk, acc0);
        kk += 8;
    }

    let mut sum: f32 = unsafe { reduce_f32x8_x86(acc0) + reduce_f32x8_x86(acc1) };
    while kk < k_dim {
        sum += row[kk] as f32 * x[kk];
        kk += 1;
    }
    sum * scale
}

#[cfg(all(
    feature = "x86-int8-kernels",
    any(target_arch = "x86_64", target_arch = "x86")
))]
#[target_feature(enable = "avx2,fma")]
unsafe fn dot2_f32_i8_x86_avx2(
    x: &[f32],
    row0: &[i8],
    scale0: f32,
    row1: &[i8],
    scale1: f32,
) -> (f32, f32) {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::*;
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;

    let k_dim = x.len();
    let mut kk = 0usize;
    let mut acc00 = _mm256_setzero_ps();
    let mut acc01 = _mm256_setzero_ps();
    let mut acc10 = _mm256_setzero_ps();
    let mut acc11 = _mm256_setzero_ps();

    while kk + 16 <= k_dim {
        let x_lo = unsafe { _mm256_loadu_ps(x.as_ptr().add(kk)) };
        let x_hi = unsafe { _mm256_loadu_ps(x.as_ptr().add(kk + 8)) };
        let row0_lo = unsafe { load_i8_as_f32x8_x86(row0.as_ptr().add(kk)) };
        let row0_hi = unsafe { load_i8_as_f32x8_x86(row0.as_ptr().add(kk + 8)) };
        let row1_lo = unsafe { load_i8_as_f32x8_x86(row1.as_ptr().add(kk)) };
        let row1_hi = unsafe { load_i8_as_f32x8_x86(row1.as_ptr().add(kk + 8)) };
        acc00 = _mm256_fmadd_ps(row0_lo, x_lo, acc00);
        acc01 = _mm256_fmadd_ps(row0_hi, x_hi, acc01);
        acc10 = _mm256_fmadd_ps(row1_lo, x_lo, acc10);
        acc11 = _mm256_fmadd_ps(row1_hi, x_hi, acc11);
        kk += 16;
    }

    while kk + 8 <= k_dim {
        let x_chunk = unsafe { _mm256_loadu_ps(x.as_ptr().add(kk)) };
        let row0_f32 = unsafe { load_i8_as_f32x8_x86(row0.as_ptr().add(kk)) };
        let row1_f32 = unsafe { load_i8_as_f32x8_x86(row1.as_ptr().add(kk)) };
        acc00 = _mm256_fmadd_ps(row0_f32, x_chunk, acc00);
        acc10 = _mm256_fmadd_ps(row1_f32, x_chunk, acc10);
        kk += 8;
    }

    let mut sum0 = unsafe { reduce_f32x8_x86(acc00) + reduce_f32x8_x86(acc01) };
    let mut sum1 = unsafe { reduce_f32x8_x86(acc10) + reduce_f32x8_x86(acc11) };
    while kk < k_dim {
        let xv = x[kk];
        sum0 += row0[kk] as f32 * xv;
        sum1 += row1[kk] as f32 * xv;
        kk += 1;
    }
    (sum0 * scale0, sum1 * scale1)
}

#[cfg(all(
    feature = "x86-int8-kernels",
    any(target_arch = "x86_64", target_arch = "x86")
))]
#[target_feature(enable = "avx2,fma")]
unsafe fn dot3_f32_i8_x86_avx2(
    x: &[f32],
    row0: &[i8],
    scale0: f32,
    row1: &[i8],
    scale1: f32,
    row2: &[i8],
    scale2: f32,
) -> (f32, f32, f32) {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::*;
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;

    let k_dim = x.len();
    let mut kk = 0usize;
    let mut acc00 = _mm256_setzero_ps();
    let mut acc01 = _mm256_setzero_ps();
    let mut acc10 = _mm256_setzero_ps();
    let mut acc11 = _mm256_setzero_ps();
    let mut acc20 = _mm256_setzero_ps();
    let mut acc21 = _mm256_setzero_ps();

    while kk + 16 <= k_dim {
        let x_lo = unsafe { _mm256_loadu_ps(x.as_ptr().add(kk)) };
        let x_hi = unsafe { _mm256_loadu_ps(x.as_ptr().add(kk + 8)) };
        let row0_lo = unsafe { load_i8_as_f32x8_x86(row0.as_ptr().add(kk)) };
        let row0_hi = unsafe { load_i8_as_f32x8_x86(row0.as_ptr().add(kk + 8)) };
        let row1_lo = unsafe { load_i8_as_f32x8_x86(row1.as_ptr().add(kk)) };
        let row1_hi = unsafe { load_i8_as_f32x8_x86(row1.as_ptr().add(kk + 8)) };
        let row2_lo = unsafe { load_i8_as_f32x8_x86(row2.as_ptr().add(kk)) };
        let row2_hi = unsafe { load_i8_as_f32x8_x86(row2.as_ptr().add(kk + 8)) };
        acc00 = _mm256_fmadd_ps(row0_lo, x_lo, acc00);
        acc01 = _mm256_fmadd_ps(row0_hi, x_hi, acc01);
        acc10 = _mm256_fmadd_ps(row1_lo, x_lo, acc10);
        acc11 = _mm256_fmadd_ps(row1_hi, x_hi, acc11);
        acc20 = _mm256_fmadd_ps(row2_lo, x_lo, acc20);
        acc21 = _mm256_fmadd_ps(row2_hi, x_hi, acc21);
        kk += 16;
    }

    while kk + 8 <= k_dim {
        let x_chunk = unsafe { _mm256_loadu_ps(x.as_ptr().add(kk)) };
        let row0_f32 = unsafe { load_i8_as_f32x8_x86(row0.as_ptr().add(kk)) };
        let row1_f32 = unsafe { load_i8_as_f32x8_x86(row1.as_ptr().add(kk)) };
        let row2_f32 = unsafe { load_i8_as_f32x8_x86(row2.as_ptr().add(kk)) };
        acc00 = _mm256_fmadd_ps(row0_f32, x_chunk, acc00);
        acc10 = _mm256_fmadd_ps(row1_f32, x_chunk, acc10);
        acc20 = _mm256_fmadd_ps(row2_f32, x_chunk, acc20);
        kk += 8;
    }

    let mut sum0 = unsafe { reduce_f32x8_x86(acc00) + reduce_f32x8_x86(acc01) };
    let mut sum1 = unsafe { reduce_f32x8_x86(acc10) + reduce_f32x8_x86(acc11) };
    let mut sum2 = unsafe { reduce_f32x8_x86(acc20) + reduce_f32x8_x86(acc21) };
    while kk < k_dim {
        let xv = x[kk];
        sum0 += row0[kk] as f32 * xv;
        sum1 += row1[kk] as f32 * xv;
        sum2 += row2[kk] as f32 * xv;
        kk += 1;
    }
    (sum0 * scale0, sum1 * scale1, sum2 * scale2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_name_matches_backend_enum() {
        let name = active_int8_backend_name();
        match active_int8_backend() {
            Int8KernelBackend::Portable => assert_eq!(name, "portable"),
            Int8KernelBackend::Arm64Neon => assert_eq!(name, "arm64-neon"),
            Int8KernelBackend::X86Avx2 => assert_eq!(name, "x86-avx2"),
        }
    }

    #[test]
    fn architecture_dispatch_consistency() {
        let x = [0.5f32, -1.0, 2.0, 0.25, -0.75, 1.5, -0.5, 3.0];
        let row0 = [3i8, -2, 5, 1, -4, 6, -1, 2];
        let row1 = [-1i8, 4, -3, 2, 5, -2, 7, -6];
        let row2 = [2i8, 1, -2, 3, -5, 4, 1, -3];
        let single = dot_f32_i8_arch(&x, &row0, 0.125);
        let triple = dot3_f32_i8_arch(&x, &row0, 0.125, &row1, 0.25, &row2, 0.5);

        if active_int8_backend() == Int8KernelBackend::Portable {
            assert!(single.is_none());
            assert!(triple.is_none());
        } else {
            assert!(single.is_some());
            assert!(triple.is_some());
        }
    }

    #[test]
    fn x86_int8_fast_paths_match_scalar_reference() {
        let x = [
            -1.25f32, 0.5, 2.0, -0.75, 1.5, -2.25, 3.0, 0.125, 1.75, -1.0, 0.625, 2.5, -3.5, 4.0,
            -0.875, 1.125, 0.333, -0.666, 1.999,
        ];
        let row0 = [
            -7i8, 3, 12, -5, 9, -11, 15, 1, 8, -4, 6, 10, -13, 14, -2, 5, 2, -3, 7,
        ];
        let row1 = [
            4i8, -9, 6, 2, -8, 7, -5, 3, -1, 10, -6, 11, -12, 13, -4, 8, -2, 1, 5,
        ];
        let row2 = [
            -3i8, 2, -7, 9, -10, 6, 4, -1, 12, -8, 5, -6, 11, -13, 7, -2, 3, -4, 10,
        ];
        let scale0 = 0.125f32;
        let scale1 = 0.0625f32;
        let scale2 = 0.25f32;

        let scalar = |row: &[i8], scale: f32| -> f32 {
            x.iter()
                .zip(row.iter())
                .map(|(&xv, &rv)| xv * rv as f32)
                .sum::<f32>()
                * scale
        };

        if let Some(sum) = dot_f32_i8_arch(&x, &row0, scale0) {
            assert!((sum - scalar(&row0, scale0)).abs() <= 1e-5);
        }
        if let Some((sum0, sum1)) = dot2_f32_i8_arch(&x, &row0, scale0, &row1, scale1) {
            assert!((sum0 - scalar(&row0, scale0)).abs() <= 1e-5);
            assert!((sum1 - scalar(&row1, scale1)).abs() <= 1e-5);
        }
        if let Some((sum0, sum1, sum2)) =
            dot3_f32_i8_arch(&x, &row0, scale0, &row1, scale1, &row2, scale2)
        {
            assert!((sum0 - scalar(&row0, scale0)).abs() <= 1e-5);
            assert!((sum1 - scalar(&row1, scale1)).abs() <= 1e-5);
            assert!((sum2 - scalar(&row2, scale2)).abs() <= 1e-5);
        }
    }

    #[test]
    fn length_mismatch_disables_arch_fast_path() {
        let x = [1.0f32, 2.0, 3.0, 4.0];
        let short = [1i8, 2];

        assert!(dot_f32_i8_arch(&x, &short, 0.5).is_none());
        assert!(dot2_f32_i8_arch(&x, &short, 0.5, &short, 0.25).is_none());
        assert!(dot3_f32_i8_arch(&x, &short, 0.5, &short, 0.25, &short, 0.125).is_none());
    }
}
