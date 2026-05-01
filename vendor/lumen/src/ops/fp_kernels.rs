use crate::arch;
use half::{bf16, f16};

#[cfg(all(feature = "x86-fp-kernels", target_arch = "x86"))]
use std::arch::x86::{__m256, __m256bh, __m256i, __m512};
#[cfg(all(feature = "x86-fp-kernels", target_arch = "x86_64"))]
use std::arch::x86_64::{__m256, __m256bh, __m256i, __m512};

#[inline]
fn dot_len_matches<T>(x: &[f32], row: &[T]) -> bool {
    x.len() == row.len()
}

#[inline]
fn dot2_len_matches<T>(x: &[f32], row0: &[T], row1: &[T]) -> bool {
    x.len() == row0.len() && x.len() == row1.len()
}

#[inline]
fn dot3_len_matches<T>(x: &[f32], row0: &[T], row1: &[T], row2: &[T]) -> bool {
    x.len() == row0.len() && x.len() == row1.len() && x.len() == row2.len()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FloatKernelBackend {
    Portable,
    Arm64Neon,
    X86Avx512,
    X86Avx2,
}

#[inline]
pub fn active_float_backend() -> FloatKernelBackend {
    if arch::arm64_fp_kernel_runtime_available() {
        FloatKernelBackend::Arm64Neon
    } else if arch::x86_avx512_fp_kernel_runtime_available() {
        FloatKernelBackend::X86Avx512
    } else if arch::x86_fp_kernel_runtime_available() {
        FloatKernelBackend::X86Avx2
    } else {
        FloatKernelBackend::Portable
    }
}

#[inline]
pub fn active_float_backend_name() -> &'static str {
    match active_float_backend() {
        FloatKernelBackend::Portable => "portable",
        FloatKernelBackend::Arm64Neon => "arm64-neon",
        FloatKernelBackend::X86Avx512 => "x86-avx512",
        FloatKernelBackend::X86Avx2 => "x86-avx2",
    }
}

#[inline]
pub fn dot_f32_arch(_x: &[f32], _row: &[f32]) -> Option<f32> {
    if !dot_len_matches(_x, _row) {
        return None;
    }

    match active_float_backend() {
        FloatKernelBackend::Portable => None,
        #[cfg(all(feature = "arm64-fp-kernels", target_arch = "aarch64"))]
        FloatKernelBackend::Arm64Neon => Some(unsafe { dot_f32_arm64_neon(_x, _row) }),
        #[cfg(all(
            feature = "x86-fp-kernels",
            any(target_arch = "x86_64", target_arch = "x86")
        ))]
        FloatKernelBackend::X86Avx512 => Some(unsafe { dot_f32_x86_avx512(_x, _row) }),
        #[cfg(all(
            feature = "x86-fp-kernels",
            any(target_arch = "x86_64", target_arch = "x86")
        ))]
        FloatKernelBackend::X86Avx2 => Some(unsafe { dot_f32_x86_avx2(_x, _row) }),
        #[allow(unreachable_patterns)]
        _ => None,
    }
}

#[inline]
pub fn dot2_f32_arch(_x: &[f32], _row0: &[f32], _row1: &[f32]) -> Option<(f32, f32)> {
    if !dot2_len_matches(_x, _row0, _row1) {
        return None;
    }

    match active_float_backend() {
        FloatKernelBackend::Portable => None,
        #[cfg(all(feature = "arm64-fp-kernels", target_arch = "aarch64"))]
        FloatKernelBackend::Arm64Neon => Some(unsafe { dot2_f32_arm64_neon(_x, _row0, _row1) }),
        #[cfg(all(
            feature = "x86-fp-kernels",
            any(target_arch = "x86_64", target_arch = "x86")
        ))]
        FloatKernelBackend::X86Avx512 => Some(unsafe { dot2_f32_x86_avx512(_x, _row0, _row1) }),
        #[cfg(all(
            feature = "x86-fp-kernels",
            any(target_arch = "x86_64", target_arch = "x86")
        ))]
        FloatKernelBackend::X86Avx2 => Some(unsafe { dot2_f32_x86_avx2(_x, _row0, _row1) }),
        #[allow(unreachable_patterns)]
        _ => None,
    }
}

#[inline]
pub fn dot3_f32_arch(
    _x: &[f32],
    _row0: &[f32],
    _row1: &[f32],
    _row2: &[f32],
) -> Option<(f32, f32, f32)> {
    if !dot3_len_matches(_x, _row0, _row1, _row2) {
        return None;
    }

    match active_float_backend() {
        FloatKernelBackend::Portable => None,
        #[cfg(all(feature = "arm64-fp-kernels", target_arch = "aarch64"))]
        FloatKernelBackend::Arm64Neon => {
            Some(unsafe { dot3_f32_arm64_neon(_x, _row0, _row1, _row2) })
        }
        #[cfg(all(
            feature = "x86-fp-kernels",
            any(target_arch = "x86_64", target_arch = "x86")
        ))]
        FloatKernelBackend::X86Avx512 => {
            Some(unsafe { dot3_f32_x86_avx512(_x, _row0, _row1, _row2) })
        }
        #[cfg(all(
            feature = "x86-fp-kernels",
            any(target_arch = "x86_64", target_arch = "x86")
        ))]
        FloatKernelBackend::X86Avx2 => Some(unsafe { dot3_f32_x86_avx2(_x, _row0, _row1, _row2) }),
        #[allow(unreachable_patterns)]
        _ => None,
    }
}

#[inline]
pub fn dot_f32_f16_arch(_x: &[f32], _row: &[f16]) -> Option<f32> {
    if !dot_len_matches(_x, _row) {
        return None;
    }

    match active_float_backend() {
        FloatKernelBackend::Portable => None,
        #[cfg(all(feature = "arm64-fp-kernels", target_arch = "aarch64"))]
        FloatKernelBackend::Arm64Neon => arch::arm64_fp16_kernel_runtime_available()
            .then(|| unsafe { dot_f32_f16_arm64_neon(_x, _row) }),
        #[cfg(all(
            feature = "x86-fp-kernels",
            any(target_arch = "x86_64", target_arch = "x86")
        ))]
        FloatKernelBackend::X86Avx512 => Some(unsafe { dot_f32_f16_x86_avx512(_x, _row) }),
        #[cfg(all(
            feature = "x86-fp-kernels",
            any(target_arch = "x86_64", target_arch = "x86")
        ))]
        FloatKernelBackend::X86Avx2 => arch::x86_fp16_kernel_runtime_available()
            .then(|| unsafe { dot_f32_f16_x86_avx2(_x, _row) }),
        #[allow(unreachable_patterns)]
        _ => None,
    }
}

#[inline]
pub fn dot2_f32_f16_arch(_x: &[f32], _row0: &[f16], _row1: &[f16]) -> Option<(f32, f32)> {
    if !dot2_len_matches(_x, _row0, _row1) {
        return None;
    }

    match active_float_backend() {
        FloatKernelBackend::Portable => None,
        #[cfg(all(feature = "arm64-fp-kernels", target_arch = "aarch64"))]
        FloatKernelBackend::Arm64Neon => arch::arm64_fp16_kernel_runtime_available()
            .then(|| unsafe { dot2_f32_f16_arm64_neon(_x, _row0, _row1) }),
        #[cfg(all(
            feature = "x86-fp-kernels",
            any(target_arch = "x86_64", target_arch = "x86")
        ))]
        FloatKernelBackend::X86Avx512 => Some(unsafe { dot2_f32_f16_x86_avx512(_x, _row0, _row1) }),
        #[cfg(all(
            feature = "x86-fp-kernels",
            any(target_arch = "x86_64", target_arch = "x86")
        ))]
        FloatKernelBackend::X86Avx2 => arch::x86_fp16_kernel_runtime_available()
            .then(|| unsafe { dot2_f32_f16_x86_avx2(_x, _row0, _row1) }),
        #[allow(unreachable_patterns)]
        _ => None,
    }
}

#[inline]
pub fn dot3_f32_f16_arch(
    _x: &[f32],
    _row0: &[f16],
    _row1: &[f16],
    _row2: &[f16],
) -> Option<(f32, f32, f32)> {
    if !dot3_len_matches(_x, _row0, _row1, _row2) {
        return None;
    }

    match active_float_backend() {
        FloatKernelBackend::Portable => None,
        #[cfg(all(feature = "arm64-fp-kernels", target_arch = "aarch64"))]
        FloatKernelBackend::Arm64Neon => arch::arm64_fp16_kernel_runtime_available()
            .then(|| unsafe { dot3_f32_f16_arm64_neon(_x, _row0, _row1, _row2) }),
        #[cfg(all(
            feature = "x86-fp-kernels",
            any(target_arch = "x86_64", target_arch = "x86")
        ))]
        FloatKernelBackend::X86Avx512 => {
            Some(unsafe { dot3_f32_f16_x86_avx512(_x, _row0, _row1, _row2) })
        }
        #[cfg(all(
            feature = "x86-fp-kernels",
            any(target_arch = "x86_64", target_arch = "x86")
        ))]
        FloatKernelBackend::X86Avx2 => arch::x86_fp16_kernel_runtime_available()
            .then(|| unsafe { dot3_f32_f16_x86_avx2(_x, _row0, _row1, _row2) }),
        #[allow(unreachable_patterns)]
        _ => None,
    }
}

#[inline]
pub fn dot_f32_bf16_arch(_x: &[f32], _row: &[bf16]) -> Option<f32> {
    if !dot_len_matches(_x, _row) {
        return None;
    }

    match active_float_backend() {
        FloatKernelBackend::Portable => None,
        #[cfg(all(feature = "arm64-fp-kernels", target_arch = "aarch64"))]
        FloatKernelBackend::Arm64Neon => Some(unsafe { dot_f32_bf16_arm64_neon(_x, _row) }),
        #[cfg(all(
            feature = "x86-fp-kernels",
            any(target_arch = "x86_64", target_arch = "x86")
        ))]
        FloatKernelBackend::X86Avx512 => {
            Some(if arch::x86_avx512_bf16_kernel_runtime_available() {
                unsafe { dot_f32_bf16_x86_avx512(_x, _row) }
            } else {
                unsafe { dot_f32_bf16_x86_avx2(_x, _row) }
            })
        }
        #[cfg(all(
            feature = "x86-fp-kernels",
            any(target_arch = "x86_64", target_arch = "x86")
        ))]
        FloatKernelBackend::X86Avx2 => Some(unsafe { dot_f32_bf16_x86_avx2(_x, _row) }),
        #[allow(unreachable_patterns)]
        _ => None,
    }
}

#[inline]
pub fn dot2_f32_bf16_arch(_x: &[f32], _row0: &[bf16], _row1: &[bf16]) -> Option<(f32, f32)> {
    if !dot2_len_matches(_x, _row0, _row1) {
        return None;
    }

    match active_float_backend() {
        FloatKernelBackend::Portable => None,
        #[cfg(all(feature = "arm64-fp-kernels", target_arch = "aarch64"))]
        FloatKernelBackend::Arm64Neon => {
            Some(unsafe { dot2_f32_bf16_arm64_neon(_x, _row0, _row1) })
        }
        #[cfg(all(
            feature = "x86-fp-kernels",
            any(target_arch = "x86_64", target_arch = "x86")
        ))]
        FloatKernelBackend::X86Avx512 => {
            Some(if arch::x86_avx512_bf16_kernel_runtime_available() {
                unsafe { dot2_f32_bf16_x86_avx512(_x, _row0, _row1) }
            } else {
                unsafe { dot2_f32_bf16_x86_avx2(_x, _row0, _row1) }
            })
        }
        #[cfg(all(
            feature = "x86-fp-kernels",
            any(target_arch = "x86_64", target_arch = "x86")
        ))]
        FloatKernelBackend::X86Avx2 => Some(unsafe { dot2_f32_bf16_x86_avx2(_x, _row0, _row1) }),
        #[allow(unreachable_patterns)]
        _ => None,
    }
}

#[inline]
pub fn dot3_f32_bf16_arch(
    _x: &[f32],
    _row0: &[bf16],
    _row1: &[bf16],
    _row2: &[bf16],
) -> Option<(f32, f32, f32)> {
    if !dot3_len_matches(_x, _row0, _row1, _row2) {
        return None;
    }

    match active_float_backend() {
        FloatKernelBackend::Portable => None,
        #[cfg(all(feature = "arm64-fp-kernels", target_arch = "aarch64"))]
        FloatKernelBackend::Arm64Neon => {
            Some(unsafe { dot3_f32_bf16_arm64_neon(_x, _row0, _row1, _row2) })
        }
        #[cfg(all(
            feature = "x86-fp-kernels",
            any(target_arch = "x86_64", target_arch = "x86")
        ))]
        FloatKernelBackend::X86Avx512 => {
            Some(if arch::x86_avx512_bf16_kernel_runtime_available() {
                unsafe { dot3_f32_bf16_x86_avx512(_x, _row0, _row1, _row2) }
            } else {
                unsafe { dot3_f32_bf16_x86_avx2(_x, _row0, _row1, _row2) }
            })
        }
        #[cfg(all(
            feature = "x86-fp-kernels",
            any(target_arch = "x86_64", target_arch = "x86")
        ))]
        FloatKernelBackend::X86Avx2 => {
            Some(unsafe { dot3_f32_bf16_x86_avx2(_x, _row0, _row1, _row2) })
        }
        #[allow(unreachable_patterns)]
        _ => None,
    }
}

#[cfg(all(feature = "arm64-fp-kernels", target_arch = "aarch64"))]
#[target_feature(enable = "neon")]
unsafe fn dot_f32_arm64_neon(x: &[f32], row: &[f32]) -> f32 {
    use std::arch::aarch64::*;

    let k_dim = x.len();
    let mut kk = 0usize;
    let mut acc0 = vdupq_n_f32(0.0);
    let mut acc1 = vdupq_n_f32(0.0);

    while kk + 8 <= k_dim {
        let row_lo = unsafe { vld1q_f32(row.as_ptr().add(kk)) };
        let row_hi = unsafe { vld1q_f32(row.as_ptr().add(kk + 4)) };
        let x_lo = unsafe { vld1q_f32(x.as_ptr().add(kk)) };
        let x_hi = unsafe { vld1q_f32(x.as_ptr().add(kk + 4)) };
        acc0 = vfmaq_f32(acc0, row_lo, x_lo);
        acc1 = vfmaq_f32(acc1, row_hi, x_hi);
        kk += 8;
    }

    let mut sum = vaddvq_f32(acc0) + vaddvq_f32(acc1);
    while kk < k_dim {
        sum += row[kk] * x[kk];
        kk += 1;
    }
    sum
}

#[cfg(all(feature = "arm64-fp-kernels", target_arch = "aarch64"))]
#[target_feature(enable = "neon")]
unsafe fn dot2_f32_arm64_neon(x: &[f32], row0: &[f32], row1: &[f32]) -> (f32, f32) {
    use std::arch::aarch64::*;

    let k_dim = x.len();
    let mut kk = 0usize;
    let mut acc00 = vdupq_n_f32(0.0);
    let mut acc01 = vdupq_n_f32(0.0);
    let mut acc10 = vdupq_n_f32(0.0);
    let mut acc11 = vdupq_n_f32(0.0);

    while kk + 8 <= k_dim {
        let x_lo = unsafe { vld1q_f32(x.as_ptr().add(kk)) };
        let x_hi = unsafe { vld1q_f32(x.as_ptr().add(kk + 4)) };
        let row0_lo = unsafe { vld1q_f32(row0.as_ptr().add(kk)) };
        let row0_hi = unsafe { vld1q_f32(row0.as_ptr().add(kk + 4)) };
        let row1_lo = unsafe { vld1q_f32(row1.as_ptr().add(kk)) };
        let row1_hi = unsafe { vld1q_f32(row1.as_ptr().add(kk + 4)) };
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
        sum0 += row0[kk] * xv;
        sum1 += row1[kk] * xv;
        kk += 1;
    }
    (sum0, sum1)
}

#[cfg(all(feature = "arm64-fp-kernels", target_arch = "aarch64"))]
#[target_feature(enable = "neon")]
unsafe fn dot3_f32_arm64_neon(
    x: &[f32],
    row0: &[f32],
    row1: &[f32],
    row2: &[f32],
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
        let x_lo = unsafe { vld1q_f32(x.as_ptr().add(kk)) };
        let x_hi = unsafe { vld1q_f32(x.as_ptr().add(kk + 4)) };
        let row0_lo = unsafe { vld1q_f32(row0.as_ptr().add(kk)) };
        let row0_hi = unsafe { vld1q_f32(row0.as_ptr().add(kk + 4)) };
        let row1_lo = unsafe { vld1q_f32(row1.as_ptr().add(kk)) };
        let row1_hi = unsafe { vld1q_f32(row1.as_ptr().add(kk + 4)) };
        let row2_lo = unsafe { vld1q_f32(row2.as_ptr().add(kk)) };
        let row2_hi = unsafe { vld1q_f32(row2.as_ptr().add(kk + 4)) };
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
        sum0 += row0[kk] * xv;
        sum1 += row1[kk] * xv;
        sum2 += row2[kk] * xv;
        kk += 1;
    }
    (sum0, sum1, sum2)
}

#[cfg(all(feature = "arm64-fp-kernels", target_arch = "aarch64"))]
#[target_feature(enable = "neon,fp16")]
unsafe fn dot_f32_f16_arm64_neon(x: &[f32], row: &[f16]) -> f32 {
    use std::arch::aarch64::*;

    let k_dim = x.len();
    let mut kk = 0usize;
    let mut acc0 = vdupq_n_f32(0.0);
    let mut acc1 = vdupq_n_f32(0.0);

    while kk + 8 <= k_dim {
        let row_u16 = unsafe { vld1q_u16(row.as_ptr().add(kk) as *const u16) };
        let row_f16 = vreinterpretq_f16_u16(row_u16);
        let row_lo = vcvt_f32_f16(vget_low_f16(row_f16));
        let row_hi = vcvt_f32_f16(vget_high_f16(row_f16));
        let x_lo = unsafe { vld1q_f32(x.as_ptr().add(kk)) };
        let x_hi = unsafe { vld1q_f32(x.as_ptr().add(kk + 4)) };
        acc0 = vfmaq_f32(acc0, row_lo, x_lo);
        acc1 = vfmaq_f32(acc1, row_hi, x_hi);
        kk += 8;
    }

    let mut sum = vaddvq_f32(acc0) + vaddvq_f32(acc1);
    while kk < k_dim {
        sum += row[kk].to_f32() * x[kk];
        kk += 1;
    }
    sum
}

#[cfg(all(feature = "arm64-fp-kernels", target_arch = "aarch64"))]
#[target_feature(enable = "neon,fp16")]
unsafe fn dot2_f32_f16_arm64_neon(x: &[f32], row0: &[f16], row1: &[f16]) -> (f32, f32) {
    use std::arch::aarch64::*;

    let k_dim = x.len();
    let mut kk = 0usize;
    let mut acc00 = vdupq_n_f32(0.0);
    let mut acc01 = vdupq_n_f32(0.0);
    let mut acc10 = vdupq_n_f32(0.0);
    let mut acc11 = vdupq_n_f32(0.0);

    while kk + 8 <= k_dim {
        let x_lo = unsafe { vld1q_f32(x.as_ptr().add(kk)) };
        let x_hi = unsafe { vld1q_f32(x.as_ptr().add(kk + 4)) };
        let row0_u16 = unsafe { vld1q_u16(row0.as_ptr().add(kk) as *const u16) };
        let row1_u16 = unsafe { vld1q_u16(row1.as_ptr().add(kk) as *const u16) };
        let row0_f16 = vreinterpretq_f16_u16(row0_u16);
        let row1_f16 = vreinterpretq_f16_u16(row1_u16);
        let row0_lo = vcvt_f32_f16(vget_low_f16(row0_f16));
        let row0_hi = vcvt_f32_f16(vget_high_f16(row0_f16));
        let row1_lo = vcvt_f32_f16(vget_low_f16(row1_f16));
        let row1_hi = vcvt_f32_f16(vget_high_f16(row1_f16));
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
        sum0 += row0[kk].to_f32() * xv;
        sum1 += row1[kk].to_f32() * xv;
        kk += 1;
    }
    (sum0, sum1)
}

#[cfg(all(feature = "arm64-fp-kernels", target_arch = "aarch64"))]
#[target_feature(enable = "neon,fp16")]
unsafe fn dot3_f32_f16_arm64_neon(
    x: &[f32],
    row0: &[f16],
    row1: &[f16],
    row2: &[f16],
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
        let x_lo = unsafe { vld1q_f32(x.as_ptr().add(kk)) };
        let x_hi = unsafe { vld1q_f32(x.as_ptr().add(kk + 4)) };
        let row0_u16 = unsafe { vld1q_u16(row0.as_ptr().add(kk) as *const u16) };
        let row1_u16 = unsafe { vld1q_u16(row1.as_ptr().add(kk) as *const u16) };
        let row2_u16 = unsafe { vld1q_u16(row2.as_ptr().add(kk) as *const u16) };
        let row0_f16 = vreinterpretq_f16_u16(row0_u16);
        let row1_f16 = vreinterpretq_f16_u16(row1_u16);
        let row2_f16 = vreinterpretq_f16_u16(row2_u16);
        let row0_lo = vcvt_f32_f16(vget_low_f16(row0_f16));
        let row0_hi = vcvt_f32_f16(vget_high_f16(row0_f16));
        let row1_lo = vcvt_f32_f16(vget_low_f16(row1_f16));
        let row1_hi = vcvt_f32_f16(vget_high_f16(row1_f16));
        let row2_lo = vcvt_f32_f16(vget_low_f16(row2_f16));
        let row2_hi = vcvt_f32_f16(vget_high_f16(row2_f16));
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
        sum0 += row0[kk].to_f32() * xv;
        sum1 += row1[kk].to_f32() * xv;
        sum2 += row2[kk].to_f32() * xv;
        kk += 1;
    }
    (sum0, sum1, sum2)
}

#[cfg(all(feature = "arm64-fp-kernels", target_arch = "aarch64"))]
#[target_feature(enable = "neon")]
unsafe fn dot_f32_bf16_arm64_neon(x: &[f32], row: &[bf16]) -> f32 {
    use std::arch::aarch64::*;

    let k_dim = x.len();
    let mut kk = 0usize;
    let mut acc0 = vdupq_n_f32(0.0);
    let mut acc1 = vdupq_n_f32(0.0);

    while kk + 8 <= k_dim {
        let row_lo_u16 = unsafe { vld1_u16(row.as_ptr().add(kk) as *const u16) };
        let row_hi_u16 = unsafe { vld1_u16(row.as_ptr().add(kk + 4) as *const u16) };
        let row_lo_u32 = vshlq_n_u32(vmovl_u16(row_lo_u16), 16);
        let row_hi_u32 = vshlq_n_u32(vmovl_u16(row_hi_u16), 16);
        let row_lo = vreinterpretq_f32_u32(row_lo_u32);
        let row_hi = vreinterpretq_f32_u32(row_hi_u32);
        let x_lo = unsafe { vld1q_f32(x.as_ptr().add(kk)) };
        let x_hi = unsafe { vld1q_f32(x.as_ptr().add(kk + 4)) };
        acc0 = vfmaq_f32(acc0, row_lo, x_lo);
        acc1 = vfmaq_f32(acc1, row_hi, x_hi);
        kk += 8;
    }

    let mut sum = vaddvq_f32(acc0) + vaddvq_f32(acc1);
    while kk < k_dim {
        sum += row[kk].to_f32() * x[kk];
        kk += 1;
    }
    sum
}

#[cfg(all(feature = "arm64-fp-kernels", target_arch = "aarch64"))]
#[target_feature(enable = "neon")]
unsafe fn dot2_f32_bf16_arm64_neon(x: &[f32], row0: &[bf16], row1: &[bf16]) -> (f32, f32) {
    use std::arch::aarch64::*;

    let k_dim = x.len();
    let mut kk = 0usize;
    let mut acc00 = vdupq_n_f32(0.0);
    let mut acc01 = vdupq_n_f32(0.0);
    let mut acc10 = vdupq_n_f32(0.0);
    let mut acc11 = vdupq_n_f32(0.0);

    while kk + 8 <= k_dim {
        let x_lo = unsafe { vld1q_f32(x.as_ptr().add(kk)) };
        let x_hi = unsafe { vld1q_f32(x.as_ptr().add(kk + 4)) };
        let row0_lo_u16 = unsafe { vld1_u16(row0.as_ptr().add(kk) as *const u16) };
        let row0_hi_u16 = unsafe { vld1_u16(row0.as_ptr().add(kk + 4) as *const u16) };
        let row1_lo_u16 = unsafe { vld1_u16(row1.as_ptr().add(kk) as *const u16) };
        let row1_hi_u16 = unsafe { vld1_u16(row1.as_ptr().add(kk + 4) as *const u16) };
        let row0_lo = vreinterpretq_f32_u32(vshlq_n_u32(vmovl_u16(row0_lo_u16), 16));
        let row0_hi = vreinterpretq_f32_u32(vshlq_n_u32(vmovl_u16(row0_hi_u16), 16));
        let row1_lo = vreinterpretq_f32_u32(vshlq_n_u32(vmovl_u16(row1_lo_u16), 16));
        let row1_hi = vreinterpretq_f32_u32(vshlq_n_u32(vmovl_u16(row1_hi_u16), 16));
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
        sum0 += row0[kk].to_f32() * xv;
        sum1 += row1[kk].to_f32() * xv;
        kk += 1;
    }
    (sum0, sum1)
}

#[cfg(all(feature = "arm64-fp-kernels", target_arch = "aarch64"))]
#[target_feature(enable = "neon")]
unsafe fn dot3_f32_bf16_arm64_neon(
    x: &[f32],
    row0: &[bf16],
    row1: &[bf16],
    row2: &[bf16],
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
        let x_lo = unsafe { vld1q_f32(x.as_ptr().add(kk)) };
        let x_hi = unsafe { vld1q_f32(x.as_ptr().add(kk + 4)) };
        let row0_lo_u16 = unsafe { vld1_u16(row0.as_ptr().add(kk) as *const u16) };
        let row0_hi_u16 = unsafe { vld1_u16(row0.as_ptr().add(kk + 4) as *const u16) };
        let row1_lo_u16 = unsafe { vld1_u16(row1.as_ptr().add(kk) as *const u16) };
        let row1_hi_u16 = unsafe { vld1_u16(row1.as_ptr().add(kk + 4) as *const u16) };
        let row2_lo_u16 = unsafe { vld1_u16(row2.as_ptr().add(kk) as *const u16) };
        let row2_hi_u16 = unsafe { vld1_u16(row2.as_ptr().add(kk + 4) as *const u16) };
        let row0_lo = vreinterpretq_f32_u32(vshlq_n_u32(vmovl_u16(row0_lo_u16), 16));
        let row0_hi = vreinterpretq_f32_u32(vshlq_n_u32(vmovl_u16(row0_hi_u16), 16));
        let row1_lo = vreinterpretq_f32_u32(vshlq_n_u32(vmovl_u16(row1_lo_u16), 16));
        let row1_hi = vreinterpretq_f32_u32(vshlq_n_u32(vmovl_u16(row1_hi_u16), 16));
        let row2_lo = vreinterpretq_f32_u32(vshlq_n_u32(vmovl_u16(row2_lo_u16), 16));
        let row2_hi = vreinterpretq_f32_u32(vshlq_n_u32(vmovl_u16(row2_hi_u16), 16));
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
        sum0 += row0[kk].to_f32() * xv;
        sum1 += row1[kk].to_f32() * xv;
        sum2 += row2[kk].to_f32() * xv;
        kk += 1;
    }
    (sum0, sum1, sum2)
}

#[cfg(all(
    feature = "x86-fp-kernels",
    any(target_arch = "x86_64", target_arch = "x86")
))]
#[inline]
#[target_feature(enable = "avx512f")]
unsafe fn reduce_f32x16_x86(v: __m512) -> f32 {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::*;
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;

    let mut buf = [0.0f32; 16];
    unsafe {
        _mm512_storeu_ps(buf.as_mut_ptr(), v);
    }
    buf.iter().sum()
}

#[cfg(all(
    feature = "x86-fp-kernels",
    any(target_arch = "x86_64", target_arch = "x86")
))]
#[inline]
#[target_feature(enable = "avx512f")]
unsafe fn load_f16_as_f32x16_x86(ptr: *const f16) -> __m512 {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::*;
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;

    let raw = unsafe { std::ptr::read_unaligned(ptr as *const __m256i) };
    _mm512_cvtph_ps(raw)
}

#[cfg(all(
    feature = "x86-fp-kernels",
    any(target_arch = "x86_64", target_arch = "x86")
))]
#[inline]
#[target_feature(enable = "avx512f,avx512bf16,avx512vl")]
unsafe fn load_bf16_x16_x86(ptr: *const bf16) -> __m256bh {
    let raw = unsafe { std::ptr::read_unaligned(ptr as *const __m256i) };
    unsafe { std::mem::transmute::<__m256i, __m256bh>(raw) }
}

#[cfg(all(
    feature = "x86-fp-kernels",
    any(target_arch = "x86_64", target_arch = "x86")
))]
#[inline]
#[target_feature(enable = "avx512f,avx512bf16")]
unsafe fn convert_f32_to_bf16x16_x86(v: __m512) -> __m256bh {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::*;
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;

    _mm512_cvtneps_pbh(v)
}

#[cfg(all(
    feature = "x86-fp-kernels",
    any(target_arch = "x86_64", target_arch = "x86")
))]
#[target_feature(enable = "avx512f")]
unsafe fn dot_f32_x86_avx512(x: &[f32], row: &[f32]) -> f32 {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::*;
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;

    let k_dim = x.len();
    let mut kk = 0usize;
    let mut acc0 = _mm512_setzero_ps();
    let mut acc1 = _mm512_setzero_ps();

    while kk + 32 <= k_dim {
        let row_lo = unsafe { _mm512_loadu_ps(row.as_ptr().add(kk)) };
        let row_hi = unsafe { _mm512_loadu_ps(row.as_ptr().add(kk + 16)) };
        let x_lo = unsafe { _mm512_loadu_ps(x.as_ptr().add(kk)) };
        let x_hi = unsafe { _mm512_loadu_ps(x.as_ptr().add(kk + 16)) };
        acc0 = _mm512_fmadd_ps(row_lo, x_lo, acc0);
        acc1 = _mm512_fmadd_ps(row_hi, x_hi, acc1);
        kk += 32;
    }

    while kk + 16 <= k_dim {
        let row_chunk = unsafe { _mm512_loadu_ps(row.as_ptr().add(kk)) };
        let x_chunk = unsafe { _mm512_loadu_ps(x.as_ptr().add(kk)) };
        acc0 = _mm512_fmadd_ps(row_chunk, x_chunk, acc0);
        kk += 16;
    }

    let mut sum = unsafe { reduce_f32x16_x86(acc0) + reduce_f32x16_x86(acc1) };
    while kk < k_dim {
        sum += row[kk] * x[kk];
        kk += 1;
    }
    sum
}

#[cfg(all(
    feature = "x86-fp-kernels",
    any(target_arch = "x86_64", target_arch = "x86")
))]
#[target_feature(enable = "avx512f")]
unsafe fn dot2_f32_x86_avx512(x: &[f32], row0: &[f32], row1: &[f32]) -> (f32, f32) {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::*;
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;

    let k_dim = x.len();
    let mut kk = 0usize;
    let mut acc00 = _mm512_setzero_ps();
    let mut acc01 = _mm512_setzero_ps();
    let mut acc10 = _mm512_setzero_ps();
    let mut acc11 = _mm512_setzero_ps();

    while kk + 32 <= k_dim {
        let x_lo = unsafe { _mm512_loadu_ps(x.as_ptr().add(kk)) };
        let x_hi = unsafe { _mm512_loadu_ps(x.as_ptr().add(kk + 16)) };
        let row0_lo = unsafe { _mm512_loadu_ps(row0.as_ptr().add(kk)) };
        let row0_hi = unsafe { _mm512_loadu_ps(row0.as_ptr().add(kk + 16)) };
        let row1_lo = unsafe { _mm512_loadu_ps(row1.as_ptr().add(kk)) };
        let row1_hi = unsafe { _mm512_loadu_ps(row1.as_ptr().add(kk + 16)) };
        acc00 = _mm512_fmadd_ps(row0_lo, x_lo, acc00);
        acc01 = _mm512_fmadd_ps(row0_hi, x_hi, acc01);
        acc10 = _mm512_fmadd_ps(row1_lo, x_lo, acc10);
        acc11 = _mm512_fmadd_ps(row1_hi, x_hi, acc11);
        kk += 32;
    }

    while kk + 16 <= k_dim {
        let x_chunk = unsafe { _mm512_loadu_ps(x.as_ptr().add(kk)) };
        let row0_chunk = unsafe { _mm512_loadu_ps(row0.as_ptr().add(kk)) };
        let row1_chunk = unsafe { _mm512_loadu_ps(row1.as_ptr().add(kk)) };
        acc00 = _mm512_fmadd_ps(row0_chunk, x_chunk, acc00);
        acc10 = _mm512_fmadd_ps(row1_chunk, x_chunk, acc10);
        kk += 16;
    }

    let mut sum0 = unsafe { reduce_f32x16_x86(acc00) + reduce_f32x16_x86(acc01) };
    let mut sum1 = unsafe { reduce_f32x16_x86(acc10) + reduce_f32x16_x86(acc11) };
    while kk < k_dim {
        let xv = x[kk];
        sum0 += row0[kk] * xv;
        sum1 += row1[kk] * xv;
        kk += 1;
    }
    (sum0, sum1)
}

#[cfg(all(
    feature = "x86-fp-kernels",
    any(target_arch = "x86_64", target_arch = "x86")
))]
#[target_feature(enable = "avx512f")]
unsafe fn dot3_f32_x86_avx512(
    x: &[f32],
    row0: &[f32],
    row1: &[f32],
    row2: &[f32],
) -> (f32, f32, f32) {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::*;
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;

    let k_dim = x.len();
    let mut kk = 0usize;
    let mut acc00 = _mm512_setzero_ps();
    let mut acc01 = _mm512_setzero_ps();
    let mut acc10 = _mm512_setzero_ps();
    let mut acc11 = _mm512_setzero_ps();
    let mut acc20 = _mm512_setzero_ps();
    let mut acc21 = _mm512_setzero_ps();

    while kk + 32 <= k_dim {
        let x_lo = unsafe { _mm512_loadu_ps(x.as_ptr().add(kk)) };
        let x_hi = unsafe { _mm512_loadu_ps(x.as_ptr().add(kk + 16)) };
        let row0_lo = unsafe { _mm512_loadu_ps(row0.as_ptr().add(kk)) };
        let row0_hi = unsafe { _mm512_loadu_ps(row0.as_ptr().add(kk + 16)) };
        let row1_lo = unsafe { _mm512_loadu_ps(row1.as_ptr().add(kk)) };
        let row1_hi = unsafe { _mm512_loadu_ps(row1.as_ptr().add(kk + 16)) };
        let row2_lo = unsafe { _mm512_loadu_ps(row2.as_ptr().add(kk)) };
        let row2_hi = unsafe { _mm512_loadu_ps(row2.as_ptr().add(kk + 16)) };
        acc00 = _mm512_fmadd_ps(row0_lo, x_lo, acc00);
        acc01 = _mm512_fmadd_ps(row0_hi, x_hi, acc01);
        acc10 = _mm512_fmadd_ps(row1_lo, x_lo, acc10);
        acc11 = _mm512_fmadd_ps(row1_hi, x_hi, acc11);
        acc20 = _mm512_fmadd_ps(row2_lo, x_lo, acc20);
        acc21 = _mm512_fmadd_ps(row2_hi, x_hi, acc21);
        kk += 32;
    }

    while kk + 16 <= k_dim {
        let x_chunk = unsafe { _mm512_loadu_ps(x.as_ptr().add(kk)) };
        let row0_chunk = unsafe { _mm512_loadu_ps(row0.as_ptr().add(kk)) };
        let row1_chunk = unsafe { _mm512_loadu_ps(row1.as_ptr().add(kk)) };
        let row2_chunk = unsafe { _mm512_loadu_ps(row2.as_ptr().add(kk)) };
        acc00 = _mm512_fmadd_ps(row0_chunk, x_chunk, acc00);
        acc10 = _mm512_fmadd_ps(row1_chunk, x_chunk, acc10);
        acc20 = _mm512_fmadd_ps(row2_chunk, x_chunk, acc20);
        kk += 16;
    }

    let mut sum0 = unsafe { reduce_f32x16_x86(acc00) + reduce_f32x16_x86(acc01) };
    let mut sum1 = unsafe { reduce_f32x16_x86(acc10) + reduce_f32x16_x86(acc11) };
    let mut sum2 = unsafe { reduce_f32x16_x86(acc20) + reduce_f32x16_x86(acc21) };
    while kk < k_dim {
        let xv = x[kk];
        sum0 += row0[kk] * xv;
        sum1 += row1[kk] * xv;
        sum2 += row2[kk] * xv;
        kk += 1;
    }
    (sum0, sum1, sum2)
}

#[cfg(all(
    feature = "x86-fp-kernels",
    any(target_arch = "x86_64", target_arch = "x86")
))]
#[target_feature(enable = "avx512f")]
unsafe fn dot_f32_f16_x86_avx512(x: &[f32], row: &[f16]) -> f32 {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::*;
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;

    let k_dim = x.len();
    let mut kk = 0usize;
    let mut acc0 = _mm512_setzero_ps();
    let mut acc1 = _mm512_setzero_ps();

    while kk + 32 <= k_dim {
        let row_lo = unsafe { load_f16_as_f32x16_x86(row.as_ptr().add(kk)) };
        let row_hi = unsafe { load_f16_as_f32x16_x86(row.as_ptr().add(kk + 16)) };
        let x_lo = unsafe { _mm512_loadu_ps(x.as_ptr().add(kk)) };
        let x_hi = unsafe { _mm512_loadu_ps(x.as_ptr().add(kk + 16)) };
        acc0 = _mm512_fmadd_ps(row_lo, x_lo, acc0);
        acc1 = _mm512_fmadd_ps(row_hi, x_hi, acc1);
        kk += 32;
    }

    while kk + 16 <= k_dim {
        let row_chunk = unsafe { load_f16_as_f32x16_x86(row.as_ptr().add(kk)) };
        let x_chunk = unsafe { _mm512_loadu_ps(x.as_ptr().add(kk)) };
        acc0 = _mm512_fmadd_ps(row_chunk, x_chunk, acc0);
        kk += 16;
    }

    let mut sum = unsafe { reduce_f32x16_x86(acc0) + reduce_f32x16_x86(acc1) };
    while kk < k_dim {
        sum += row[kk].to_f32() * x[kk];
        kk += 1;
    }
    sum
}

#[cfg(all(
    feature = "x86-fp-kernels",
    any(target_arch = "x86_64", target_arch = "x86")
))]
#[target_feature(enable = "avx512f")]
unsafe fn dot2_f32_f16_x86_avx512(x: &[f32], row0: &[f16], row1: &[f16]) -> (f32, f32) {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::*;
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;

    let k_dim = x.len();
    let mut kk = 0usize;
    let mut acc00 = _mm512_setzero_ps();
    let mut acc01 = _mm512_setzero_ps();
    let mut acc10 = _mm512_setzero_ps();
    let mut acc11 = _mm512_setzero_ps();

    while kk + 32 <= k_dim {
        let x_lo = unsafe { _mm512_loadu_ps(x.as_ptr().add(kk)) };
        let x_hi = unsafe { _mm512_loadu_ps(x.as_ptr().add(kk + 16)) };
        let row0_lo = unsafe { load_f16_as_f32x16_x86(row0.as_ptr().add(kk)) };
        let row0_hi = unsafe { load_f16_as_f32x16_x86(row0.as_ptr().add(kk + 16)) };
        let row1_lo = unsafe { load_f16_as_f32x16_x86(row1.as_ptr().add(kk)) };
        let row1_hi = unsafe { load_f16_as_f32x16_x86(row1.as_ptr().add(kk + 16)) };
        acc00 = _mm512_fmadd_ps(row0_lo, x_lo, acc00);
        acc01 = _mm512_fmadd_ps(row0_hi, x_hi, acc01);
        acc10 = _mm512_fmadd_ps(row1_lo, x_lo, acc10);
        acc11 = _mm512_fmadd_ps(row1_hi, x_hi, acc11);
        kk += 32;
    }

    while kk + 16 <= k_dim {
        let x_chunk = unsafe { _mm512_loadu_ps(x.as_ptr().add(kk)) };
        let row0_chunk = unsafe { load_f16_as_f32x16_x86(row0.as_ptr().add(kk)) };
        let row1_chunk = unsafe { load_f16_as_f32x16_x86(row1.as_ptr().add(kk)) };
        acc00 = _mm512_fmadd_ps(row0_chunk, x_chunk, acc00);
        acc10 = _mm512_fmadd_ps(row1_chunk, x_chunk, acc10);
        kk += 16;
    }

    let mut sum0 = unsafe { reduce_f32x16_x86(acc00) + reduce_f32x16_x86(acc01) };
    let mut sum1 = unsafe { reduce_f32x16_x86(acc10) + reduce_f32x16_x86(acc11) };
    while kk < k_dim {
        let xv = x[kk];
        sum0 += row0[kk].to_f32() * xv;
        sum1 += row1[kk].to_f32() * xv;
        kk += 1;
    }
    (sum0, sum1)
}

#[cfg(all(
    feature = "x86-fp-kernels",
    any(target_arch = "x86_64", target_arch = "x86")
))]
#[target_feature(enable = "avx512f")]
unsafe fn dot3_f32_f16_x86_avx512(
    x: &[f32],
    row0: &[f16],
    row1: &[f16],
    row2: &[f16],
) -> (f32, f32, f32) {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::*;
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;

    let k_dim = x.len();
    let mut kk = 0usize;
    let mut acc00 = _mm512_setzero_ps();
    let mut acc01 = _mm512_setzero_ps();
    let mut acc10 = _mm512_setzero_ps();
    let mut acc11 = _mm512_setzero_ps();
    let mut acc20 = _mm512_setzero_ps();
    let mut acc21 = _mm512_setzero_ps();

    while kk + 32 <= k_dim {
        let x_lo = unsafe { _mm512_loadu_ps(x.as_ptr().add(kk)) };
        let x_hi = unsafe { _mm512_loadu_ps(x.as_ptr().add(kk + 16)) };
        let row0_lo = unsafe { load_f16_as_f32x16_x86(row0.as_ptr().add(kk)) };
        let row0_hi = unsafe { load_f16_as_f32x16_x86(row0.as_ptr().add(kk + 16)) };
        let row1_lo = unsafe { load_f16_as_f32x16_x86(row1.as_ptr().add(kk)) };
        let row1_hi = unsafe { load_f16_as_f32x16_x86(row1.as_ptr().add(kk + 16)) };
        let row2_lo = unsafe { load_f16_as_f32x16_x86(row2.as_ptr().add(kk)) };
        let row2_hi = unsafe { load_f16_as_f32x16_x86(row2.as_ptr().add(kk + 16)) };
        acc00 = _mm512_fmadd_ps(row0_lo, x_lo, acc00);
        acc01 = _mm512_fmadd_ps(row0_hi, x_hi, acc01);
        acc10 = _mm512_fmadd_ps(row1_lo, x_lo, acc10);
        acc11 = _mm512_fmadd_ps(row1_hi, x_hi, acc11);
        acc20 = _mm512_fmadd_ps(row2_lo, x_lo, acc20);
        acc21 = _mm512_fmadd_ps(row2_hi, x_hi, acc21);
        kk += 32;
    }

    while kk + 16 <= k_dim {
        let x_chunk = unsafe { _mm512_loadu_ps(x.as_ptr().add(kk)) };
        let row0_chunk = unsafe { load_f16_as_f32x16_x86(row0.as_ptr().add(kk)) };
        let row1_chunk = unsafe { load_f16_as_f32x16_x86(row1.as_ptr().add(kk)) };
        let row2_chunk = unsafe { load_f16_as_f32x16_x86(row2.as_ptr().add(kk)) };
        acc00 = _mm512_fmadd_ps(row0_chunk, x_chunk, acc00);
        acc10 = _mm512_fmadd_ps(row1_chunk, x_chunk, acc10);
        acc20 = _mm512_fmadd_ps(row2_chunk, x_chunk, acc20);
        kk += 16;
    }

    let mut sum0 = unsafe { reduce_f32x16_x86(acc00) + reduce_f32x16_x86(acc01) };
    let mut sum1 = unsafe { reduce_f32x16_x86(acc10) + reduce_f32x16_x86(acc11) };
    let mut sum2 = unsafe { reduce_f32x16_x86(acc20) + reduce_f32x16_x86(acc21) };
    while kk < k_dim {
        let xv = x[kk];
        sum0 += row0[kk].to_f32() * xv;
        sum1 += row1[kk].to_f32() * xv;
        sum2 += row2[kk].to_f32() * xv;
        kk += 1;
    }
    (sum0, sum1, sum2)
}

#[cfg(all(
    feature = "x86-fp-kernels",
    any(target_arch = "x86_64", target_arch = "x86")
))]
#[target_feature(enable = "avx512f,avx512vl,avx512bf16")]
unsafe fn dot_f32_bf16_x86_avx512(x: &[f32], row: &[bf16]) -> f32 {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::*;
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;

    let k_dim = x.len();
    let mut kk = 0usize;
    let mut acc0 = _mm256_setzero_ps();
    let mut acc1 = _mm256_setzero_ps();

    while kk + 32 <= k_dim {
        let x_lo = unsafe { _mm512_loadu_ps(x.as_ptr().add(kk)) };
        let x_hi = unsafe { _mm512_loadu_ps(x.as_ptr().add(kk + 16)) };
        let row_lo = unsafe { load_bf16_x16_x86(row.as_ptr().add(kk)) };
        let row_hi = unsafe { load_bf16_x16_x86(row.as_ptr().add(kk + 16)) };
        let x_lo_bf16 = unsafe { convert_f32_to_bf16x16_x86(x_lo) };
        let x_hi_bf16 = unsafe { convert_f32_to_bf16x16_x86(x_hi) };
        acc0 = _mm256_dpbf16_ps(acc0, row_lo, x_lo_bf16);
        acc1 = _mm256_dpbf16_ps(acc1, row_hi, x_hi_bf16);
        kk += 32;
    }

    while kk + 16 <= k_dim {
        let x_chunk = unsafe { _mm512_loadu_ps(x.as_ptr().add(kk)) };
        let row_chunk = unsafe { load_bf16_x16_x86(row.as_ptr().add(kk)) };
        let x_chunk_bf16 = unsafe { convert_f32_to_bf16x16_x86(x_chunk) };
        acc0 = _mm256_dpbf16_ps(acc0, row_chunk, x_chunk_bf16);
        kk += 16;
    }

    let mut sum = unsafe { reduce_f32x8_x86(acc0) + reduce_f32x8_x86(acc1) };
    while kk < k_dim {
        sum += row[kk].to_f32() * x[kk];
        kk += 1;
    }
    sum
}

#[cfg(all(
    feature = "x86-fp-kernels",
    any(target_arch = "x86_64", target_arch = "x86")
))]
#[target_feature(enable = "avx512f,avx512vl,avx512bf16")]
unsafe fn dot2_f32_bf16_x86_avx512(x: &[f32], row0: &[bf16], row1: &[bf16]) -> (f32, f32) {
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

    while kk + 32 <= k_dim {
        let x_lo = unsafe { _mm512_loadu_ps(x.as_ptr().add(kk)) };
        let x_hi = unsafe { _mm512_loadu_ps(x.as_ptr().add(kk + 16)) };
        let row0_lo = unsafe { load_bf16_x16_x86(row0.as_ptr().add(kk)) };
        let row0_hi = unsafe { load_bf16_x16_x86(row0.as_ptr().add(kk + 16)) };
        let row1_lo = unsafe { load_bf16_x16_x86(row1.as_ptr().add(kk)) };
        let row1_hi = unsafe { load_bf16_x16_x86(row1.as_ptr().add(kk + 16)) };
        let x_lo_bf16 = unsafe { convert_f32_to_bf16x16_x86(x_lo) };
        let x_hi_bf16 = unsafe { convert_f32_to_bf16x16_x86(x_hi) };
        acc00 = _mm256_dpbf16_ps(acc00, row0_lo, x_lo_bf16);
        acc01 = _mm256_dpbf16_ps(acc01, row0_hi, x_hi_bf16);
        acc10 = _mm256_dpbf16_ps(acc10, row1_lo, x_lo_bf16);
        acc11 = _mm256_dpbf16_ps(acc11, row1_hi, x_hi_bf16);
        kk += 32;
    }

    while kk + 16 <= k_dim {
        let x_chunk = unsafe { _mm512_loadu_ps(x.as_ptr().add(kk)) };
        let row0_chunk = unsafe { load_bf16_x16_x86(row0.as_ptr().add(kk)) };
        let row1_chunk = unsafe { load_bf16_x16_x86(row1.as_ptr().add(kk)) };
        let x_chunk_bf16 = unsafe { convert_f32_to_bf16x16_x86(x_chunk) };
        acc00 = _mm256_dpbf16_ps(acc00, row0_chunk, x_chunk_bf16);
        acc10 = _mm256_dpbf16_ps(acc10, row1_chunk, x_chunk_bf16);
        kk += 16;
    }

    let mut sum0 = unsafe { reduce_f32x8_x86(acc00) + reduce_f32x8_x86(acc01) };
    let mut sum1 = unsafe { reduce_f32x8_x86(acc10) + reduce_f32x8_x86(acc11) };
    while kk < k_dim {
        let xv = x[kk];
        sum0 += row0[kk].to_f32() * xv;
        sum1 += row1[kk].to_f32() * xv;
        kk += 1;
    }
    (sum0, sum1)
}

#[cfg(all(
    feature = "x86-fp-kernels",
    any(target_arch = "x86_64", target_arch = "x86")
))]
#[target_feature(enable = "avx512f,avx512vl,avx512bf16")]
unsafe fn dot3_f32_bf16_x86_avx512(
    x: &[f32],
    row0: &[bf16],
    row1: &[bf16],
    row2: &[bf16],
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

    while kk + 32 <= k_dim {
        let x_lo = unsafe { _mm512_loadu_ps(x.as_ptr().add(kk)) };
        let x_hi = unsafe { _mm512_loadu_ps(x.as_ptr().add(kk + 16)) };
        let row0_lo = unsafe { load_bf16_x16_x86(row0.as_ptr().add(kk)) };
        let row0_hi = unsafe { load_bf16_x16_x86(row0.as_ptr().add(kk + 16)) };
        let row1_lo = unsafe { load_bf16_x16_x86(row1.as_ptr().add(kk)) };
        let row1_hi = unsafe { load_bf16_x16_x86(row1.as_ptr().add(kk + 16)) };
        let row2_lo = unsafe { load_bf16_x16_x86(row2.as_ptr().add(kk)) };
        let row2_hi = unsafe { load_bf16_x16_x86(row2.as_ptr().add(kk + 16)) };
        let x_lo_bf16 = unsafe { convert_f32_to_bf16x16_x86(x_lo) };
        let x_hi_bf16 = unsafe { convert_f32_to_bf16x16_x86(x_hi) };
        acc00 = _mm256_dpbf16_ps(acc00, row0_lo, x_lo_bf16);
        acc01 = _mm256_dpbf16_ps(acc01, row0_hi, x_hi_bf16);
        acc10 = _mm256_dpbf16_ps(acc10, row1_lo, x_lo_bf16);
        acc11 = _mm256_dpbf16_ps(acc11, row1_hi, x_hi_bf16);
        acc20 = _mm256_dpbf16_ps(acc20, row2_lo, x_lo_bf16);
        acc21 = _mm256_dpbf16_ps(acc21, row2_hi, x_hi_bf16);
        kk += 32;
    }

    while kk + 16 <= k_dim {
        let x_chunk = unsafe { _mm512_loadu_ps(x.as_ptr().add(kk)) };
        let row0_chunk = unsafe { load_bf16_x16_x86(row0.as_ptr().add(kk)) };
        let row1_chunk = unsafe { load_bf16_x16_x86(row1.as_ptr().add(kk)) };
        let row2_chunk = unsafe { load_bf16_x16_x86(row2.as_ptr().add(kk)) };
        let x_chunk_bf16 = unsafe { convert_f32_to_bf16x16_x86(x_chunk) };
        acc00 = _mm256_dpbf16_ps(acc00, row0_chunk, x_chunk_bf16);
        acc10 = _mm256_dpbf16_ps(acc10, row1_chunk, x_chunk_bf16);
        acc20 = _mm256_dpbf16_ps(acc20, row2_chunk, x_chunk_bf16);
        kk += 16;
    }

    let mut sum0 = unsafe { reduce_f32x8_x86(acc00) + reduce_f32x8_x86(acc01) };
    let mut sum1 = unsafe { reduce_f32x8_x86(acc10) + reduce_f32x8_x86(acc11) };
    let mut sum2 = unsafe { reduce_f32x8_x86(acc20) + reduce_f32x8_x86(acc21) };
    while kk < k_dim {
        let xv = x[kk];
        sum0 += row0[kk].to_f32() * xv;
        sum1 += row1[kk].to_f32() * xv;
        sum2 += row2[kk].to_f32() * xv;
        kk += 1;
    }
    (sum0, sum1, sum2)
}

#[cfg(all(
    feature = "x86-fp-kernels",
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
    feature = "x86-fp-kernels",
    any(target_arch = "x86_64", target_arch = "x86")
))]
#[inline]
#[target_feature(enable = "avx2,f16c")]
unsafe fn load_f16_as_f32x8_x86(ptr: *const f16) -> __m256 {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::*;
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;

    let raw = unsafe { _mm_loadu_si128(ptr as *const __m128i) };
    _mm256_cvtph_ps(raw)
}

#[cfg(all(
    feature = "x86-fp-kernels",
    any(target_arch = "x86_64", target_arch = "x86")
))]
#[inline]
#[target_feature(enable = "avx2")]
unsafe fn load_bf16_as_f32x8_x86(ptr: *const bf16) -> __m256 {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::*;
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;

    let raw = unsafe { _mm_loadu_si128(ptr as *const __m128i) };
    let widened = _mm256_cvtepu16_epi32(raw);
    let bits = _mm256_slli_epi32(widened, 16);
    _mm256_castsi256_ps(bits)
}

#[cfg(all(
    feature = "x86-fp-kernels",
    any(target_arch = "x86_64", target_arch = "x86")
))]
#[target_feature(enable = "avx2,fma,f16c")]
unsafe fn dot_f32_f16_x86_avx2(x: &[f32], row: &[f16]) -> f32 {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::*;
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;

    let k_dim = x.len();
    let mut kk = 0usize;
    let mut acc0 = _mm256_setzero_ps();
    let mut acc1 = _mm256_setzero_ps();

    while kk + 16 <= k_dim {
        let row_lo = unsafe { load_f16_as_f32x8_x86(row.as_ptr().add(kk)) };
        let row_hi = unsafe { load_f16_as_f32x8_x86(row.as_ptr().add(kk + 8)) };
        let x_lo = unsafe { _mm256_loadu_ps(x.as_ptr().add(kk)) };
        let x_hi = unsafe { _mm256_loadu_ps(x.as_ptr().add(kk + 8)) };
        acc0 = _mm256_fmadd_ps(row_lo, x_lo, acc0);
        acc1 = _mm256_fmadd_ps(row_hi, x_hi, acc1);
        kk += 16;
    }

    while kk + 8 <= k_dim {
        let row_chunk = unsafe { load_f16_as_f32x8_x86(row.as_ptr().add(kk)) };
        let x_chunk = unsafe { _mm256_loadu_ps(x.as_ptr().add(kk)) };
        acc0 = _mm256_fmadd_ps(row_chunk, x_chunk, acc0);
        kk += 8;
    }

    let mut sum = unsafe { reduce_f32x8_x86(acc0) + reduce_f32x8_x86(acc1) };
    while kk < k_dim {
        sum += row[kk].to_f32() * x[kk];
        kk += 1;
    }
    sum
}

#[cfg(all(
    feature = "x86-fp-kernels",
    any(target_arch = "x86_64", target_arch = "x86")
))]
#[target_feature(enable = "avx2,fma,f16c")]
unsafe fn dot2_f32_f16_x86_avx2(x: &[f32], row0: &[f16], row1: &[f16]) -> (f32, f32) {
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
        let row0_lo = unsafe { load_f16_as_f32x8_x86(row0.as_ptr().add(kk)) };
        let row0_hi = unsafe { load_f16_as_f32x8_x86(row0.as_ptr().add(kk + 8)) };
        let row1_lo = unsafe { load_f16_as_f32x8_x86(row1.as_ptr().add(kk)) };
        let row1_hi = unsafe { load_f16_as_f32x8_x86(row1.as_ptr().add(kk + 8)) };
        acc00 = _mm256_fmadd_ps(row0_lo, x_lo, acc00);
        acc01 = _mm256_fmadd_ps(row0_hi, x_hi, acc01);
        acc10 = _mm256_fmadd_ps(row1_lo, x_lo, acc10);
        acc11 = _mm256_fmadd_ps(row1_hi, x_hi, acc11);
        kk += 16;
    }

    while kk + 8 <= k_dim {
        let x_chunk = unsafe { _mm256_loadu_ps(x.as_ptr().add(kk)) };
        let row0_chunk = unsafe { load_f16_as_f32x8_x86(row0.as_ptr().add(kk)) };
        let row1_chunk = unsafe { load_f16_as_f32x8_x86(row1.as_ptr().add(kk)) };
        acc00 = _mm256_fmadd_ps(row0_chunk, x_chunk, acc00);
        acc10 = _mm256_fmadd_ps(row1_chunk, x_chunk, acc10);
        kk += 8;
    }

    let mut sum0 = unsafe { reduce_f32x8_x86(acc00) + reduce_f32x8_x86(acc01) };
    let mut sum1 = unsafe { reduce_f32x8_x86(acc10) + reduce_f32x8_x86(acc11) };
    while kk < k_dim {
        let xv = x[kk];
        sum0 += row0[kk].to_f32() * xv;
        sum1 += row1[kk].to_f32() * xv;
        kk += 1;
    }
    (sum0, sum1)
}

#[cfg(all(
    feature = "x86-fp-kernels",
    any(target_arch = "x86_64", target_arch = "x86")
))]
#[target_feature(enable = "avx2,fma,f16c")]
unsafe fn dot3_f32_f16_x86_avx2(
    x: &[f32],
    row0: &[f16],
    row1: &[f16],
    row2: &[f16],
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
        let row0_lo = unsafe { load_f16_as_f32x8_x86(row0.as_ptr().add(kk)) };
        let row0_hi = unsafe { load_f16_as_f32x8_x86(row0.as_ptr().add(kk + 8)) };
        let row1_lo = unsafe { load_f16_as_f32x8_x86(row1.as_ptr().add(kk)) };
        let row1_hi = unsafe { load_f16_as_f32x8_x86(row1.as_ptr().add(kk + 8)) };
        let row2_lo = unsafe { load_f16_as_f32x8_x86(row2.as_ptr().add(kk)) };
        let row2_hi = unsafe { load_f16_as_f32x8_x86(row2.as_ptr().add(kk + 8)) };
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
        let row0_chunk = unsafe { load_f16_as_f32x8_x86(row0.as_ptr().add(kk)) };
        let row1_chunk = unsafe { load_f16_as_f32x8_x86(row1.as_ptr().add(kk)) };
        let row2_chunk = unsafe { load_f16_as_f32x8_x86(row2.as_ptr().add(kk)) };
        acc00 = _mm256_fmadd_ps(row0_chunk, x_chunk, acc00);
        acc10 = _mm256_fmadd_ps(row1_chunk, x_chunk, acc10);
        acc20 = _mm256_fmadd_ps(row2_chunk, x_chunk, acc20);
        kk += 8;
    }

    let mut sum0 = unsafe { reduce_f32x8_x86(acc00) + reduce_f32x8_x86(acc01) };
    let mut sum1 = unsafe { reduce_f32x8_x86(acc10) + reduce_f32x8_x86(acc11) };
    let mut sum2 = unsafe { reduce_f32x8_x86(acc20) + reduce_f32x8_x86(acc21) };
    while kk < k_dim {
        let xv = x[kk];
        sum0 += row0[kk].to_f32() * xv;
        sum1 += row1[kk].to_f32() * xv;
        sum2 += row2[kk].to_f32() * xv;
        kk += 1;
    }
    (sum0, sum1, sum2)
}

#[cfg(all(
    feature = "x86-fp-kernels",
    any(target_arch = "x86_64", target_arch = "x86")
))]
#[target_feature(enable = "avx2,fma")]
unsafe fn dot_f32_bf16_x86_avx2(x: &[f32], row: &[bf16]) -> f32 {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::*;
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;

    let k_dim = x.len();
    let mut kk = 0usize;
    let mut acc0 = _mm256_setzero_ps();
    let mut acc1 = _mm256_setzero_ps();

    while kk + 16 <= k_dim {
        let row_lo = unsafe { load_bf16_as_f32x8_x86(row.as_ptr().add(kk)) };
        let row_hi = unsafe { load_bf16_as_f32x8_x86(row.as_ptr().add(kk + 8)) };
        let x_lo = unsafe { _mm256_loadu_ps(x.as_ptr().add(kk)) };
        let x_hi = unsafe { _mm256_loadu_ps(x.as_ptr().add(kk + 8)) };
        acc0 = _mm256_fmadd_ps(row_lo, x_lo, acc0);
        acc1 = _mm256_fmadd_ps(row_hi, x_hi, acc1);
        kk += 16;
    }

    while kk + 8 <= k_dim {
        let row_chunk = unsafe { load_bf16_as_f32x8_x86(row.as_ptr().add(kk)) };
        let x_chunk = unsafe { _mm256_loadu_ps(x.as_ptr().add(kk)) };
        acc0 = _mm256_fmadd_ps(row_chunk, x_chunk, acc0);
        kk += 8;
    }

    let mut sum = unsafe { reduce_f32x8_x86(acc0) + reduce_f32x8_x86(acc1) };
    while kk < k_dim {
        sum += row[kk].to_f32() * x[kk];
        kk += 1;
    }
    sum
}

#[cfg(all(
    feature = "x86-fp-kernels",
    any(target_arch = "x86_64", target_arch = "x86")
))]
#[target_feature(enable = "avx2,fma")]
unsafe fn dot2_f32_bf16_x86_avx2(x: &[f32], row0: &[bf16], row1: &[bf16]) -> (f32, f32) {
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
        let row0_lo = unsafe { load_bf16_as_f32x8_x86(row0.as_ptr().add(kk)) };
        let row0_hi = unsafe { load_bf16_as_f32x8_x86(row0.as_ptr().add(kk + 8)) };
        let row1_lo = unsafe { load_bf16_as_f32x8_x86(row1.as_ptr().add(kk)) };
        let row1_hi = unsafe { load_bf16_as_f32x8_x86(row1.as_ptr().add(kk + 8)) };
        acc00 = _mm256_fmadd_ps(row0_lo, x_lo, acc00);
        acc01 = _mm256_fmadd_ps(row0_hi, x_hi, acc01);
        acc10 = _mm256_fmadd_ps(row1_lo, x_lo, acc10);
        acc11 = _mm256_fmadd_ps(row1_hi, x_hi, acc11);
        kk += 16;
    }

    while kk + 8 <= k_dim {
        let x_chunk = unsafe { _mm256_loadu_ps(x.as_ptr().add(kk)) };
        let row0_chunk = unsafe { load_bf16_as_f32x8_x86(row0.as_ptr().add(kk)) };
        let row1_chunk = unsafe { load_bf16_as_f32x8_x86(row1.as_ptr().add(kk)) };
        acc00 = _mm256_fmadd_ps(row0_chunk, x_chunk, acc00);
        acc10 = _mm256_fmadd_ps(row1_chunk, x_chunk, acc10);
        kk += 8;
    }

    let mut sum0 = unsafe { reduce_f32x8_x86(acc00) + reduce_f32x8_x86(acc01) };
    let mut sum1 = unsafe { reduce_f32x8_x86(acc10) + reduce_f32x8_x86(acc11) };
    while kk < k_dim {
        let xv = x[kk];
        sum0 += row0[kk].to_f32() * xv;
        sum1 += row1[kk].to_f32() * xv;
        kk += 1;
    }
    (sum0, sum1)
}

#[cfg(all(
    feature = "x86-fp-kernels",
    any(target_arch = "x86_64", target_arch = "x86")
))]
#[target_feature(enable = "avx2,fma")]
unsafe fn dot3_f32_bf16_x86_avx2(
    x: &[f32],
    row0: &[bf16],
    row1: &[bf16],
    row2: &[bf16],
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
        let row0_lo = unsafe { load_bf16_as_f32x8_x86(row0.as_ptr().add(kk)) };
        let row0_hi = unsafe { load_bf16_as_f32x8_x86(row0.as_ptr().add(kk + 8)) };
        let row1_lo = unsafe { load_bf16_as_f32x8_x86(row1.as_ptr().add(kk)) };
        let row1_hi = unsafe { load_bf16_as_f32x8_x86(row1.as_ptr().add(kk + 8)) };
        let row2_lo = unsafe { load_bf16_as_f32x8_x86(row2.as_ptr().add(kk)) };
        let row2_hi = unsafe { load_bf16_as_f32x8_x86(row2.as_ptr().add(kk + 8)) };
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
        let row0_chunk = unsafe { load_bf16_as_f32x8_x86(row0.as_ptr().add(kk)) };
        let row1_chunk = unsafe { load_bf16_as_f32x8_x86(row1.as_ptr().add(kk)) };
        let row2_chunk = unsafe { load_bf16_as_f32x8_x86(row2.as_ptr().add(kk)) };
        acc00 = _mm256_fmadd_ps(row0_chunk, x_chunk, acc00);
        acc10 = _mm256_fmadd_ps(row1_chunk, x_chunk, acc10);
        acc20 = _mm256_fmadd_ps(row2_chunk, x_chunk, acc20);
        kk += 8;
    }

    let mut sum0 = unsafe { reduce_f32x8_x86(acc00) + reduce_f32x8_x86(acc01) };
    let mut sum1 = unsafe { reduce_f32x8_x86(acc10) + reduce_f32x8_x86(acc11) };
    let mut sum2 = unsafe { reduce_f32x8_x86(acc20) + reduce_f32x8_x86(acc21) };
    while kk < k_dim {
        let xv = x[kk];
        sum0 += row0[kk].to_f32() * xv;
        sum1 += row1[kk].to_f32() * xv;
        sum2 += row2[kk].to_f32() * xv;
        kk += 1;
    }
    (sum0, sum1, sum2)
}

#[cfg(all(
    feature = "x86-fp-kernels",
    any(target_arch = "x86_64", target_arch = "x86")
))]
#[target_feature(enable = "avx2,fma")]
unsafe fn dot_f32_x86_avx2(x: &[f32], row: &[f32]) -> f32 {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::*;
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;

    let k_dim = x.len();
    let mut kk = 0usize;
    let mut acc0 = _mm256_setzero_ps();
    let mut acc1 = _mm256_setzero_ps();

    while kk + 16 <= k_dim {
        let row_lo = unsafe { _mm256_loadu_ps(row.as_ptr().add(kk)) };
        let row_hi = unsafe { _mm256_loadu_ps(row.as_ptr().add(kk + 8)) };
        let x_lo = unsafe { _mm256_loadu_ps(x.as_ptr().add(kk)) };
        let x_hi = unsafe { _mm256_loadu_ps(x.as_ptr().add(kk + 8)) };
        acc0 = _mm256_fmadd_ps(row_lo, x_lo, acc0);
        acc1 = _mm256_fmadd_ps(row_hi, x_hi, acc1);
        kk += 16;
    }

    while kk + 8 <= k_dim {
        let row_chunk = unsafe { _mm256_loadu_ps(row.as_ptr().add(kk)) };
        let x_chunk = unsafe { _mm256_loadu_ps(x.as_ptr().add(kk)) };
        acc0 = _mm256_fmadd_ps(row_chunk, x_chunk, acc0);
        kk += 8;
    }

    let mut buf0 = [0.0f32; 8];
    let mut buf1 = [0.0f32; 8];
    unsafe {
        _mm256_storeu_ps(buf0.as_mut_ptr(), acc0);
        _mm256_storeu_ps(buf1.as_mut_ptr(), acc1);
    }

    let mut sum: f32 = buf0.iter().sum::<f32>() + buf1.iter().sum::<f32>();
    while kk < k_dim {
        sum += row[kk] * x[kk];
        kk += 1;
    }
    sum
}

#[cfg(all(
    feature = "x86-fp-kernels",
    any(target_arch = "x86_64", target_arch = "x86")
))]
#[target_feature(enable = "avx2,fma")]
unsafe fn dot2_f32_x86_avx2(x: &[f32], row0: &[f32], row1: &[f32]) -> (f32, f32) {
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
        let row0_lo = unsafe { _mm256_loadu_ps(row0.as_ptr().add(kk)) };
        let row0_hi = unsafe { _mm256_loadu_ps(row0.as_ptr().add(kk + 8)) };
        let row1_lo = unsafe { _mm256_loadu_ps(row1.as_ptr().add(kk)) };
        let row1_hi = unsafe { _mm256_loadu_ps(row1.as_ptr().add(kk + 8)) };
        acc00 = _mm256_fmadd_ps(row0_lo, x_lo, acc00);
        acc01 = _mm256_fmadd_ps(row0_hi, x_hi, acc01);
        acc10 = _mm256_fmadd_ps(row1_lo, x_lo, acc10);
        acc11 = _mm256_fmadd_ps(row1_hi, x_hi, acc11);
        kk += 16;
    }

    while kk + 8 <= k_dim {
        let x_chunk = unsafe { _mm256_loadu_ps(x.as_ptr().add(kk)) };
        let row0_chunk = unsafe { _mm256_loadu_ps(row0.as_ptr().add(kk)) };
        let row1_chunk = unsafe { _mm256_loadu_ps(row1.as_ptr().add(kk)) };
        acc00 = _mm256_fmadd_ps(row0_chunk, x_chunk, acc00);
        acc10 = _mm256_fmadd_ps(row1_chunk, x_chunk, acc10);
        kk += 8;
    }

    let mut buf00 = [0.0f32; 8];
    let mut buf01 = [0.0f32; 8];
    let mut buf10 = [0.0f32; 8];
    let mut buf11 = [0.0f32; 8];
    unsafe {
        _mm256_storeu_ps(buf00.as_mut_ptr(), acc00);
        _mm256_storeu_ps(buf01.as_mut_ptr(), acc01);
        _mm256_storeu_ps(buf10.as_mut_ptr(), acc10);
        _mm256_storeu_ps(buf11.as_mut_ptr(), acc11);
    }

    let mut sum0 = buf00.iter().sum::<f32>() + buf01.iter().sum::<f32>();
    let mut sum1 = buf10.iter().sum::<f32>() + buf11.iter().sum::<f32>();
    while kk < k_dim {
        let xv = x[kk];
        sum0 += row0[kk] * xv;
        sum1 += row1[kk] * xv;
        kk += 1;
    }
    (sum0, sum1)
}

#[cfg(all(
    feature = "x86-fp-kernels",
    any(target_arch = "x86_64", target_arch = "x86")
))]
#[target_feature(enable = "avx2,fma")]
unsafe fn dot3_f32_x86_avx2(
    x: &[f32],
    row0: &[f32],
    row1: &[f32],
    row2: &[f32],
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
        let row0_lo = unsafe { _mm256_loadu_ps(row0.as_ptr().add(kk)) };
        let row0_hi = unsafe { _mm256_loadu_ps(row0.as_ptr().add(kk + 8)) };
        let row1_lo = unsafe { _mm256_loadu_ps(row1.as_ptr().add(kk)) };
        let row1_hi = unsafe { _mm256_loadu_ps(row1.as_ptr().add(kk + 8)) };
        let row2_lo = unsafe { _mm256_loadu_ps(row2.as_ptr().add(kk)) };
        let row2_hi = unsafe { _mm256_loadu_ps(row2.as_ptr().add(kk + 8)) };
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
        let row0_chunk = unsafe { _mm256_loadu_ps(row0.as_ptr().add(kk)) };
        let row1_chunk = unsafe { _mm256_loadu_ps(row1.as_ptr().add(kk)) };
        let row2_chunk = unsafe { _mm256_loadu_ps(row2.as_ptr().add(kk)) };
        acc00 = _mm256_fmadd_ps(row0_chunk, x_chunk, acc00);
        acc10 = _mm256_fmadd_ps(row1_chunk, x_chunk, acc10);
        acc20 = _mm256_fmadd_ps(row2_chunk, x_chunk, acc20);
        kk += 8;
    }

    let mut buf00 = [0.0f32; 8];
    let mut buf01 = [0.0f32; 8];
    let mut buf10 = [0.0f32; 8];
    let mut buf11 = [0.0f32; 8];
    let mut buf20 = [0.0f32; 8];
    let mut buf21 = [0.0f32; 8];
    unsafe {
        _mm256_storeu_ps(buf00.as_mut_ptr(), acc00);
        _mm256_storeu_ps(buf01.as_mut_ptr(), acc01);
        _mm256_storeu_ps(buf10.as_mut_ptr(), acc10);
        _mm256_storeu_ps(buf11.as_mut_ptr(), acc11);
        _mm256_storeu_ps(buf20.as_mut_ptr(), acc20);
        _mm256_storeu_ps(buf21.as_mut_ptr(), acc21);
    }

    let mut sum0 = buf00.iter().sum::<f32>() + buf01.iter().sum::<f32>();
    let mut sum1 = buf10.iter().sum::<f32>() + buf11.iter().sum::<f32>();
    let mut sum2 = buf20.iter().sum::<f32>() + buf21.iter().sum::<f32>();
    while kk < k_dim {
        let xv = x[kk];
        sum0 += row0[kk] * xv;
        sum1 += row1[kk] * xv;
        sum2 += row2[kk] * xv;
        kk += 1;
    }
    (sum0, sum1, sum2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_name_matches_backend_enum() {
        let name = active_float_backend_name();
        match active_float_backend() {
            FloatKernelBackend::Portable => assert_eq!(name, "portable"),
            FloatKernelBackend::Arm64Neon => assert_eq!(name, "arm64-neon"),
            FloatKernelBackend::X86Avx512 => assert_eq!(name, "x86-avx512"),
            FloatKernelBackend::X86Avx2 => assert_eq!(name, "x86-avx2"),
        }
    }

    #[test]
    fn architecture_dispatch_consistency() {
        let x = [0.5f32, -1.0, 2.0, 0.25, -0.75, 1.5, -0.5, 3.0];
        let row = [3.0f32, -2.0, 5.0, 1.0, -4.0, 6.0, -1.0, 2.0];
        let single = dot_f32_arch(&x, &row);

        if active_float_backend() == FloatKernelBackend::Portable {
            assert!(single.is_none());
        } else {
            assert!(single.is_some());
        }
    }

    #[test]
    fn f16_and_bf16_dispatch_consistency() {
        let x = [0.5f32, -1.0, 2.0, 0.25, -0.75, 1.5, -0.5, 3.0];
        let row_f16 = x.map(f16::from_f32);
        let row_bf16 = x.map(bf16::from_f32);
        let f16_sum = dot_f32_f16_arch(&x, &row_f16);
        let bf16_sum = dot_f32_bf16_arch(&x, &row_bf16);

        match active_float_backend() {
            FloatKernelBackend::Portable => {
                assert!(f16_sum.is_none());
                assert!(bf16_sum.is_none());
            }
            FloatKernelBackend::Arm64Neon => {
                if arch::arm64_fp16_kernel_runtime_available() {
                    assert!(f16_sum.is_some());
                } else {
                    assert!(f16_sum.is_none());
                }
                assert!(bf16_sum.is_some());
            }
            FloatKernelBackend::X86Avx512 => {
                assert!(f16_sum.is_some());
                assert!(bf16_sum.is_some());
            }
            FloatKernelBackend::X86Avx2 => {
                if arch::x86_fp16_kernel_runtime_available() {
                    assert!(f16_sum.is_some());
                } else {
                    assert!(f16_sum.is_none());
                }
                assert!(bf16_sum.is_some());
            }
        }
    }

    #[test]
    fn mixed_precision_fast_paths_match_scalar_reference() {
        let x = [
            -1.25f32, 0.5, 2.0, -0.75, 1.5, -2.25, 3.0, 0.125, 1.75, -1.0, 0.625, 2.5, -3.5, 4.0,
            -0.875, 1.125, 0.333, -0.666, 1.999,
        ];
        let row0_f16 = x.map(|v| f16::from_f32(v * 0.75 + 0.125));
        let row1_f16 = x.map(|v| f16::from_f32(v * -0.5 + 0.25));
        let row2_f16 = x.map(|v| f16::from_f32(v * 1.25 - 0.75));
        let row0_bf16 = x.map(|v| bf16::from_f32(v * 0.75 + 0.125));
        let row1_bf16 = x.map(|v| bf16::from_f32(v * -0.5 + 0.25));
        let row2_bf16 = x.map(|v| bf16::from_f32(v * 1.25 - 0.75));

        let scalar_f16 = |row: &[f16]| -> f32 {
            x.iter()
                .zip(row.iter())
                .map(|(&xv, &rv)| xv * rv.to_f32())
                .sum()
        };
        let scalar_bf16 = |row: &[bf16]| -> f32 {
            x.iter()
                .zip(row.iter())
                .map(|(&xv, &rv)| xv * rv.to_f32())
                .sum()
        };

        if let Some(sum) = dot_f32_f16_arch(&x, &row0_f16) {
            assert!((sum - scalar_f16(&row0_f16)).abs() <= 1e-3);
        }
        if let Some((sum0, sum1)) = dot2_f32_f16_arch(&x, &row0_f16, &row1_f16) {
            assert!((sum0 - scalar_f16(&row0_f16)).abs() <= 1e-3);
            assert!((sum1 - scalar_f16(&row1_f16)).abs() <= 1e-3);
        }
        if let Some((sum0, sum1, sum2)) = dot3_f32_f16_arch(&x, &row0_f16, &row1_f16, &row2_f16) {
            assert!((sum0 - scalar_f16(&row0_f16)).abs() <= 1e-3);
            assert!((sum1 - scalar_f16(&row1_f16)).abs() <= 1e-3);
            assert!((sum2 - scalar_f16(&row2_f16)).abs() <= 1e-3);
        }

        if let Some(sum) = dot_f32_bf16_arch(&x, &row0_bf16) {
            assert!((sum - scalar_bf16(&row0_bf16)).abs() <= 1e-2);
        }
        if let Some((sum0, sum1)) = dot2_f32_bf16_arch(&x, &row0_bf16, &row1_bf16) {
            assert!((sum0 - scalar_bf16(&row0_bf16)).abs() <= 1e-2);
            assert!((sum1 - scalar_bf16(&row1_bf16)).abs() <= 1e-2);
        }
        if let Some((sum0, sum1, sum2)) = dot3_f32_bf16_arch(&x, &row0_bf16, &row1_bf16, &row2_bf16)
        {
            assert!((sum0 - scalar_bf16(&row0_bf16)).abs() <= 1e-2);
            assert!((sum1 - scalar_bf16(&row1_bf16)).abs() <= 1e-2);
            assert!((sum2 - scalar_bf16(&row2_bf16)).abs() <= 1e-2);
        }
    }

    #[test]
    fn length_mismatch_disables_arch_fast_path() {
        let x = [1.0f32, 2.0, 3.0, 4.0];
        let short_f32 = [1.0f32, 2.0];
        let short_f16 = [f16::from_f32(1.0), f16::from_f32(2.0)];
        let short_bf16 = [bf16::from_f32(1.0), bf16::from_f32(2.0)];

        assert!(dot_f32_arch(&x, &short_f32).is_none());
        assert!(dot2_f32_arch(&x, &short_f32, &short_f32).is_none());
        assert!(dot3_f32_arch(&x, &short_f32, &short_f32, &short_f32).is_none());
        assert!(dot_f32_f16_arch(&x, &short_f16).is_none());
        assert!(dot2_f32_f16_arch(&x, &short_f16, &short_f16).is_none());
        assert!(dot3_f32_f16_arch(&x, &short_f16, &short_f16, &short_f16).is_none());
        assert!(dot_f32_bf16_arch(&x, &short_bf16).is_none());
        assert!(dot2_f32_bf16_arch(&x, &short_bf16, &short_bf16).is_none());
        assert!(dot3_f32_bf16_arch(&x, &short_bf16, &short_bf16, &short_bf16).is_none());
    }
}
