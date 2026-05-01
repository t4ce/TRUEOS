use crate::autograd::{
    KernelRouteClass, StoragePreference, Tensor, TensorData, TensorStorageOwned, TensorStorageView,
    assert_native_device_support, assert_same_device, is_no_grad, is_strict_device_execution,
};
use crate::ops::cuda;
use crate::ops::fp_kernels::{
    dot_f32_arch, dot_f32_bf16_arch, dot_f32_f16_arch, dot2_f32_arch, dot2_f32_bf16_arch,
    dot2_f32_f16_arch, dot3_f32_arch, dot3_f32_bf16_arch, dot3_f32_f16_arch,
};
use crate::ops::int8_kernels::{dot_f32_i8_arch, dot2_f32_i8_arch, dot3_f32_i8_arch};
use crate::precision::DType;
use half::{bf16, f16, slice::HalfFloatSliceExt};
use ndarray::linalg::general_mat_mul;
use ndarray::{Array2, Array4, Ix2, Ix4, Zip};
use rayon::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

#[cfg(all(feature = "arm64-int8-kernels", target_arch = "aarch64"))]
const MATVEC_BLOCK_ROWS: usize = 32;
#[cfg(not(all(feature = "arm64-int8-kernels", target_arch = "aarch64")))]
const MATVEC_BLOCK_ROWS: usize = 16;

#[cfg(all(feature = "arm64-int8-kernels", target_arch = "aarch64"))]
const SILU_I8_BLOCK_ROWS: usize = 64;
#[cfg(not(all(feature = "arm64-int8-kernels", target_arch = "aarch64")))]
const SILU_I8_BLOCK_ROWS: usize = 32;

const ARGMAX_BLOCK_ROWS: usize = 32;

#[cfg(all(feature = "arm64-int8-kernels", target_arch = "aarch64"))]
const MATVEC_I8_PAR_CHUNK_ROWS: usize = 128;
#[cfg(not(all(feature = "arm64-int8-kernels", target_arch = "aarch64")))]
const MATVEC_I8_PAR_CHUNK_ROWS: usize = 64;

#[cfg(all(feature = "arm64-int8-kernels", target_arch = "aarch64"))]
const QKV_I8_PAR_CHUNK_ROWS: usize = 64;
#[cfg(not(all(feature = "arm64-int8-kernels", target_arch = "aarch64")))]
const QKV_I8_PAR_CHUNK_ROWS: usize = 64;

#[cfg(all(feature = "arm64-fp-kernels", target_arch = "aarch64"))]
const MIXED_ROW_PAR_CHUNK_ROWS: usize = 32;
#[cfg(not(all(feature = "arm64-fp-kernels", target_arch = "aarch64")))]
const MIXED_ROW_PAR_CHUNK_ROWS: usize = 16;

const MATVEC_BLOCK_THRESHOLD: usize = 16384;
#[cfg(all(feature = "arm64-int8-kernels", target_arch = "aarch64"))]
const MATVEC_PAR_THRESHOLD: usize = 1024;
#[cfg(not(all(feature = "arm64-int8-kernels", target_arch = "aarch64")))]
const MATVEC_PAR_THRESHOLD: usize = 256;

#[inline]
fn should_use_mixed_matvec_block_kernel(n_rows: usize) -> bool {
    #[cfg(all(feature = "arm64-fp-kernels", target_arch = "aarch64"))]
    {
        let _ = n_rows;
        false
    }

    #[cfg(not(all(feature = "arm64-fp-kernels", target_arch = "aarch64")))]
    {
        let _ = n_rows;
        false
    }
}

fn should_use_mixed_dual_block_kernel(n_rows: usize) -> bool {
    #[cfg(all(feature = "arm64-fp-kernels", target_arch = "aarch64"))]
    {
        let _ = n_rows;
        false
    }

    #[cfg(not(all(feature = "arm64-fp-kernels", target_arch = "aarch64")))]
    {
        let _ = n_rows;
        false
    }
}

#[inline]
fn should_use_argmax_block_kernel(n_rows: usize) -> bool {
    #[cfg(all(feature = "arm64-fp-kernels", target_arch = "aarch64"))]
    {
        let _ = n_rows;
        false
    }

    #[cfg(not(all(feature = "arm64-fp-kernels", target_arch = "aarch64")))]
    {
        n_rows >= MATVEC_BLOCK_THRESHOLD
    }
}

thread_local! {
    static F16_TO_F32_SCRATCH: RefCell<Vec<f32>> = const { RefCell::new(Vec::new()) };
    static BF16_TO_F32_SCRATCH: RefCell<Vec<f32>> = const { RefCell::new(Vec::new()) };
    static I8_TO_F32_SCRATCH: RefCell<Vec<f32>> = const { RefCell::new(Vec::new()) };
}

pub trait DotElem: Copy + Send + Sync {
    fn to_f32(self) -> f32;
}

impl DotElem for f32 {
    #[inline]
    fn to_f32(self) -> f32 {
        self
    }
}

impl DotElem for bf16 {
    #[inline]
    fn to_f32(self) -> f32 {
        self.to_f32()
    }
}

impl DotElem for f16 {
    #[inline]
    fn to_f32(self) -> f32 {
        self.to_f32()
    }
}

#[derive(Clone, Copy)]
pub enum SliceRef<'a> {
    F32(&'a [f32]),
    F16(&'a [f16]),
    BF16(&'a [bf16]),
    I8(&'a [i8], f32),
}

#[inline]
fn should_use_i8_block_kernel(n_rows: usize, k_dim: usize) -> bool {
    #[cfg(all(feature = "arm64-int8-kernels", target_arch = "aarch64"))]
    {
        let _ = k_dim;
        n_rows >= MATVEC_BLOCK_THRESHOLD
    }

    #[cfg(not(all(feature = "arm64-int8-kernels", target_arch = "aarch64")))]
    {
        let _ = (n_rows, k_dim);
        false
    }
}

#[inline]
fn should_use_i8_silu_block_kernel(n_rows: usize, k_dim: usize) -> bool {
    #[cfg(all(feature = "arm64-int8-kernels", target_arch = "aarch64"))]
    {
        let _ = (n_rows, k_dim);
        false
    }

    #[cfg(not(all(feature = "arm64-int8-kernels", target_arch = "aarch64")))]
    {
        let _ = (n_rows, k_dim);
        false
    }
}

#[inline]
fn should_use_i8_matmul_block_kernel(n_rows: usize, k_dim: usize) -> bool {
    #[cfg(all(feature = "arm64-int8-kernels", target_arch = "aarch64"))]
    {
        let _ = (n_rows, k_dim);
        false
    }

    #[cfg(not(all(feature = "arm64-int8-kernels", target_arch = "aarch64")))]
    {
        let _ = (n_rows, k_dim);
        false
    }
}

#[inline]
fn should_use_i8_qkv_row4_kernel() -> bool {
    #[cfg(all(feature = "arm64-int8-kernels", target_arch = "aarch64"))]
    {
        false
    }

    #[cfg(not(all(feature = "arm64-int8-kernels", target_arch = "aarch64")))]
    {
        false
    }
}

#[inline]
pub(crate) fn with_f16_input_as_f32<R>(x: &[f16], f: impl FnOnce(&[f32]) -> R) -> R {
    F16_TO_F32_SCRATCH.with(|scratch| {
        if let Ok(mut scratch) = scratch.try_borrow_mut() {
            if scratch.len() < x.len() {
                scratch.resize(x.len(), 0.0);
            }
            x.convert_to_f32_slice(&mut scratch[..x.len()]);
            return f(&scratch[..x.len()]);
        }

        let mut fallback = vec![0.0f32; x.len()];
        x.convert_to_f32_slice(&mut fallback);
        f(&fallback)
    })
}

#[inline]
pub(crate) fn with_bf16_input_as_f32<R>(x: &[bf16], f: impl FnOnce(&[f32]) -> R) -> R {
    BF16_TO_F32_SCRATCH.with(|scratch| {
        if let Ok(mut scratch) = scratch.try_borrow_mut() {
            if scratch.len() < x.len() {
                scratch.resize(x.len(), 0.0);
            }
            x.convert_to_f32_slice(&mut scratch[..x.len()]);
            return f(&scratch[..x.len()]);
        }

        let mut fallback = vec![0.0f32; x.len()];
        x.convert_to_f32_slice(&mut fallback);
        f(&fallback)
    })
}

#[inline]
pub(crate) fn with_i8_input_as_f32<R>(x: &[i8], scale: f32, f: impl FnOnce(&[f32]) -> R) -> R {
    I8_TO_F32_SCRATCH.with(|scratch| {
        if let Ok(mut scratch) = scratch.try_borrow_mut() {
            if scratch.len() < x.len() {
                scratch.resize(x.len(), 0.0);
            }
            for (dst, &src) in scratch[..x.len()].iter_mut().zip(x.iter()) {
                *dst = src as f32 * scale;
            }
            return f(&scratch[..x.len()]);
        }

        let mut fallback = vec![0.0f32; x.len()];
        for (dst, &src) in fallback.iter_mut().zip(x.iter()) {
            *dst = src as f32 * scale;
        }
        f(&fallback)
    })
}

#[inline]
pub(crate) fn dot_unrolled(x: &[f32], row: &[f32]) -> f32 {
    if let Some(sum) = dot_f32_arch(x, row) {
        return sum;
    }

    let mut s0 = 0.0f32;
    let mut s1 = 0.0f32;
    let mut s2 = 0.0f32;
    let mut s3 = 0.0f32;
    let mut kk = 0usize;
    let k_dim = x.len();

    while kk + 8 <= k_dim {
        s0 += row[kk] * x[kk] + row[kk + 4] * x[kk + 4];
        s1 += row[kk + 1] * x[kk + 1] + row[kk + 5] * x[kk + 5];
        s2 += row[kk + 2] * x[kk + 2] + row[kk + 6] * x[kk + 6];
        s3 += row[kk + 3] * x[kk + 3] + row[kk + 7] * x[kk + 7];
        kk += 8;
    }

    while kk + 4 <= k_dim {
        s0 += row[kk] * x[kk];
        s1 += row[kk + 1] * x[kk + 1];
        s2 += row[kk + 2] * x[kk + 2];
        s3 += row[kk + 3] * x[kk + 3];
        kk += 4;
    }

    let mut sum = s0 + s1 + s2 + s3;
    while kk < k_dim {
        sum += row[kk] * x[kk];
        kk += 1;
    }
    sum
}

#[inline]
pub(crate) fn dot_unrolled_f32_bf16(x: &[f32], row: &[bf16]) -> f32 {
    if let Some(sum) = dot_f32_bf16_arch(x, row) {
        return sum;
    }

    let mut s0 = 0.0f32;
    let mut s1 = 0.0f32;
    let mut s2 = 0.0f32;
    let mut s3 = 0.0f32;
    let mut kk = 0usize;
    let k_dim = x.len();

    while kk + 8 <= k_dim {
        s0 += row[kk].to_f32() * x[kk] + row[kk + 4].to_f32() * x[kk + 4];
        s1 += row[kk + 1].to_f32() * x[kk + 1] + row[kk + 5].to_f32() * x[kk + 5];
        s2 += row[kk + 2].to_f32() * x[kk + 2] + row[kk + 6].to_f32() * x[kk + 6];
        s3 += row[kk + 3].to_f32() * x[kk + 3] + row[kk + 7].to_f32() * x[kk + 7];
        kk += 8;
    }

    while kk + 4 <= k_dim {
        s0 += row[kk].to_f32() * x[kk];
        s1 += row[kk + 1].to_f32() * x[kk + 1];
        s2 += row[kk + 2].to_f32() * x[kk + 2];
        s3 += row[kk + 3].to_f32() * x[kk + 3];
        kk += 4;
    }

    let mut sum = s0 + s1 + s2 + s3;
    while kk < k_dim {
        sum += row[kk].to_f32() * x[kk];
        kk += 1;
    }
    sum
}

#[inline]
pub(crate) fn dot_unrolled_f32_f16(x: &[f32], row: &[f16]) -> f32 {
    if let Some(sum) = dot_f32_f16_arch(x, row) {
        return sum;
    }

    let mut s0 = 0.0f32;
    let mut s1 = 0.0f32;
    let mut s2 = 0.0f32;
    let mut s3 = 0.0f32;
    let mut kk = 0usize;
    let k_dim = x.len();

    while kk + 8 <= k_dim {
        s0 += row[kk].to_f32() * x[kk] + row[kk + 4].to_f32() * x[kk + 4];
        s1 += row[kk + 1].to_f32() * x[kk + 1] + row[kk + 5].to_f32() * x[kk + 5];
        s2 += row[kk + 2].to_f32() * x[kk + 2] + row[kk + 6].to_f32() * x[kk + 6];
        s3 += row[kk + 3].to_f32() * x[kk + 3] + row[kk + 7].to_f32() * x[kk + 7];
        kk += 8;
    }

    while kk + 4 <= k_dim {
        s0 += row[kk].to_f32() * x[kk];
        s1 += row[kk + 1].to_f32() * x[kk + 1];
        s2 += row[kk + 2].to_f32() * x[kk + 2];
        s3 += row[kk + 3].to_f32() * x[kk + 3];
        kk += 4;
    }

    let mut sum = s0 + s1 + s2 + s3;
    while kk < k_dim {
        sum += row[kk].to_f32() * x[kk];
        kk += 1;
    }
    sum
}

#[inline]
fn dot_unrolled_f32_i8_portable(x: &[f32], row: &[i8], scale: f32) -> f32 {
    let mut s0 = 0.0f32;
    let mut s1 = 0.0f32;
    let mut s2 = 0.0f32;
    let mut s3 = 0.0f32;
    let mut s4 = 0.0f32;
    let mut s5 = 0.0f32;
    let mut s6 = 0.0f32;
    let mut s7 = 0.0f32;
    let mut kk = 0usize;
    let k_dim = x.len();

    while kk + 16 <= k_dim {
        s0 += row[kk] as f32 * x[kk] + row[kk + 8] as f32 * x[kk + 8];
        s1 += row[kk + 1] as f32 * x[kk + 1] + row[kk + 9] as f32 * x[kk + 9];
        s2 += row[kk + 2] as f32 * x[kk + 2] + row[kk + 10] as f32 * x[kk + 10];
        s3 += row[kk + 3] as f32 * x[kk + 3] + row[kk + 11] as f32 * x[kk + 11];
        s4 += row[kk + 4] as f32 * x[kk + 4] + row[kk + 12] as f32 * x[kk + 12];
        s5 += row[kk + 5] as f32 * x[kk + 5] + row[kk + 13] as f32 * x[kk + 13];
        s6 += row[kk + 6] as f32 * x[kk + 6] + row[kk + 14] as f32 * x[kk + 14];
        s7 += row[kk + 7] as f32 * x[kk + 7] + row[kk + 15] as f32 * x[kk + 15];
        kk += 16;
    }

    while kk + 8 <= k_dim {
        s0 += row[kk] as f32 * x[kk] + row[kk + 4] as f32 * x[kk + 4];
        s1 += row[kk + 1] as f32 * x[kk + 1] + row[kk + 5] as f32 * x[kk + 5];
        s2 += row[kk + 2] as f32 * x[kk + 2] + row[kk + 6] as f32 * x[kk + 6];
        s3 += row[kk + 3] as f32 * x[kk + 3] + row[kk + 7] as f32 * x[kk + 7];
        kk += 8;
    }

    while kk + 4 <= k_dim {
        s0 += row[kk] as f32 * x[kk];
        s1 += row[kk + 1] as f32 * x[kk + 1];
        s2 += row[kk + 2] as f32 * x[kk + 2];
        s3 += row[kk + 3] as f32 * x[kk + 3];
        kk += 4;
    }

    let mut sum = s0 + s1 + s2 + s3 + s4 + s5 + s6 + s7;
    while kk < k_dim {
        sum += row[kk] as f32 * x[kk];
        kk += 1;
    }
    sum * scale
}

#[inline]
fn dot_unrolled_i8_i8(x: &[i8], x_scale: f32, row: &[i8], w_scale: f32) -> f32 {
    let mut s0 = 0i32;
    let mut s1 = 0i32;
    let mut s2 = 0i32;
    let mut s3 = 0i32;
    let mut s4 = 0i32;
    let mut s5 = 0i32;
    let mut s6 = 0i32;
    let mut s7 = 0i32;
    let mut kk = 0usize;
    let k_dim = x.len();

    while kk + 16 <= k_dim {
        s0 += row[kk] as i32 * x[kk] as i32 + row[kk + 8] as i32 * x[kk + 8] as i32;
        s1 += row[kk + 1] as i32 * x[kk + 1] as i32 + row[kk + 9] as i32 * x[kk + 9] as i32;
        s2 += row[kk + 2] as i32 * x[kk + 2] as i32 + row[kk + 10] as i32 * x[kk + 10] as i32;
        s3 += row[kk + 3] as i32 * x[kk + 3] as i32 + row[kk + 11] as i32 * x[kk + 11] as i32;
        s4 += row[kk + 4] as i32 * x[kk + 4] as i32 + row[kk + 12] as i32 * x[kk + 12] as i32;
        s5 += row[kk + 5] as i32 * x[kk + 5] as i32 + row[kk + 13] as i32 * x[kk + 13] as i32;
        s6 += row[kk + 6] as i32 * x[kk + 6] as i32 + row[kk + 14] as i32 * x[kk + 14] as i32;
        s7 += row[kk + 7] as i32 * x[kk + 7] as i32 + row[kk + 15] as i32 * x[kk + 15] as i32;
        kk += 16;
    }

    while kk + 8 <= k_dim {
        s0 += row[kk] as i32 * x[kk] as i32 + row[kk + 4] as i32 * x[kk + 4] as i32;
        s1 += row[kk + 1] as i32 * x[kk + 1] as i32 + row[kk + 5] as i32 * x[kk + 5] as i32;
        s2 += row[kk + 2] as i32 * x[kk + 2] as i32 + row[kk + 6] as i32 * x[kk + 6] as i32;
        s3 += row[kk + 3] as i32 * x[kk + 3] as i32 + row[kk + 7] as i32 * x[kk + 7] as i32;
        kk += 8;
    }

    while kk + 4 <= k_dim {
        s0 += row[kk] as i32 * x[kk] as i32;
        s1 += row[kk + 1] as i32 * x[kk + 1] as i32;
        s2 += row[kk + 2] as i32 * x[kk + 2] as i32;
        s3 += row[kk + 3] as i32 * x[kk + 3] as i32;
        kk += 4;
    }

    let mut sum = s0 + s1 + s2 + s3 + s4 + s5 + s6 + s7;
    while kk < k_dim {
        sum += row[kk] as i32 * x[kk] as i32;
        kk += 1;
    }
    (sum as f32) * x_scale * w_scale
}

#[inline]
fn dot2_unrolled_i8_i8(
    x: &[i8],
    x_scale: f32,
    row0: &[i8],
    scale0: f32,
    row1: &[i8],
    scale1: f32,
) -> (f32, f32) {
    let mut a0 = 0i32;
    let mut a1 = 0i32;
    let mut b0 = 0i32;
    let mut b1 = 0i32;
    let mut c0 = 0i32;
    let mut c1 = 0i32;
    let mut d0 = 0i32;
    let mut d1 = 0i32;
    let mut e0 = 0i32;
    let mut e1 = 0i32;
    let mut f0 = 0i32;
    let mut f1 = 0i32;
    let mut g0 = 0i32;
    let mut g1 = 0i32;
    let mut h0 = 0i32;
    let mut h1 = 0i32;
    let mut kk = 0usize;
    let k_dim = x.len();

    while kk + 16 <= k_dim {
        let x0 = x[kk] as i32;
        let x1 = x[kk + 1] as i32;
        let x2 = x[kk + 2] as i32;
        let x3 = x[kk + 3] as i32;
        let x4 = x[kk + 4] as i32;
        let x5 = x[kk + 5] as i32;
        let x6 = x[kk + 6] as i32;
        let x7 = x[kk + 7] as i32;
        let x8 = x[kk + 8] as i32;
        let x9 = x[kk + 9] as i32;
        let x10 = x[kk + 10] as i32;
        let x11 = x[kk + 11] as i32;
        let x12 = x[kk + 12] as i32;
        let x13 = x[kk + 13] as i32;
        let x14 = x[kk + 14] as i32;
        let x15 = x[kk + 15] as i32;

        a0 += row0[kk] as i32 * x0;
        a1 += row1[kk] as i32 * x0;
        b0 += row0[kk + 1] as i32 * x1;
        b1 += row1[kk + 1] as i32 * x1;
        c0 += row0[kk + 2] as i32 * x2;
        c1 += row1[kk + 2] as i32 * x2;
        d0 += row0[kk + 3] as i32 * x3;
        d1 += row1[kk + 3] as i32 * x3;
        e0 += row0[kk + 4] as i32 * x4;
        e1 += row1[kk + 4] as i32 * x4;
        f0 += row0[kk + 5] as i32 * x5;
        f1 += row1[kk + 5] as i32 * x5;
        g0 += row0[kk + 6] as i32 * x6;
        g1 += row1[kk + 6] as i32 * x6;
        h0 += row0[kk + 7] as i32 * x7;
        h1 += row1[kk + 7] as i32 * x7;
        a0 += row0[kk + 8] as i32 * x8;
        a1 += row1[kk + 8] as i32 * x8;
        b0 += row0[kk + 9] as i32 * x9;
        b1 += row1[kk + 9] as i32 * x9;
        c0 += row0[kk + 10] as i32 * x10;
        c1 += row1[kk + 10] as i32 * x10;
        d0 += row0[kk + 11] as i32 * x11;
        d1 += row1[kk + 11] as i32 * x11;
        e0 += row0[kk + 12] as i32 * x12;
        e1 += row1[kk + 12] as i32 * x12;
        f0 += row0[kk + 13] as i32 * x13;
        f1 += row1[kk + 13] as i32 * x13;
        g0 += row0[kk + 14] as i32 * x14;
        g1 += row1[kk + 14] as i32 * x14;
        h0 += row0[kk + 15] as i32 * x15;
        h1 += row1[kk + 15] as i32 * x15;
        kk += 16;
    }

    while kk + 8 <= k_dim {
        let x0 = x[kk] as i32;
        let x1 = x[kk + 1] as i32;
        let x2 = x[kk + 2] as i32;
        let x3 = x[kk + 3] as i32;
        let x4 = x[kk + 4] as i32;
        let x5 = x[kk + 5] as i32;
        let x6 = x[kk + 6] as i32;
        let x7 = x[kk + 7] as i32;

        a0 += row0[kk] as i32 * x0 + row0[kk + 4] as i32 * x4;
        a1 += row1[kk] as i32 * x0 + row1[kk + 4] as i32 * x4;
        b0 += row0[kk + 1] as i32 * x1 + row0[kk + 5] as i32 * x5;
        b1 += row1[kk + 1] as i32 * x1 + row1[kk + 5] as i32 * x5;
        c0 += row0[kk + 2] as i32 * x2 + row0[kk + 6] as i32 * x6;
        c1 += row1[kk + 2] as i32 * x2 + row1[kk + 6] as i32 * x6;
        d0 += row0[kk + 3] as i32 * x3 + row0[kk + 7] as i32 * x7;
        d1 += row1[kk + 3] as i32 * x3 + row1[kk + 7] as i32 * x7;
        kk += 8;
    }

    while kk + 4 <= k_dim {
        let x0 = x[kk] as i32;
        let x1 = x[kk + 1] as i32;
        let x2 = x[kk + 2] as i32;
        let x3 = x[kk + 3] as i32;
        a0 += row0[kk] as i32 * x0;
        a1 += row1[kk] as i32 * x0;
        b0 += row0[kk + 1] as i32 * x1;
        b1 += row1[kk + 1] as i32 * x1;
        c0 += row0[kk + 2] as i32 * x2;
        c1 += row1[kk + 2] as i32 * x2;
        d0 += row0[kk + 3] as i32 * x3;
        d1 += row1[kk + 3] as i32 * x3;
        kk += 4;
    }

    let mut sum0 = a0 + b0 + c0 + d0 + e0 + f0 + g0 + h0;
    let mut sum1 = a1 + b1 + c1 + d1 + e1 + f1 + g1 + h1;
    while kk < k_dim {
        let xv = x[kk] as i32;
        sum0 += row0[kk] as i32 * xv;
        sum1 += row1[kk] as i32 * xv;
        kk += 1;
    }
    (
        (sum0 as f32) * x_scale * scale0,
        (sum1 as f32) * x_scale * scale1,
    )
}

#[inline]
fn dot3_unrolled_i8_i8(
    x: &[i8],
    x_scale: f32,
    row0: &[i8],
    scale0: f32,
    row1: &[i8],
    scale1: f32,
    row2: &[i8],
    scale2: f32,
) -> (f32, f32, f32) {
    let mut a0 = 0i32;
    let mut a1 = 0i32;
    let mut a2 = 0i32;
    let mut b0 = 0i32;
    let mut b1 = 0i32;
    let mut b2 = 0i32;
    let mut c0 = 0i32;
    let mut c1 = 0i32;
    let mut c2 = 0i32;
    let mut d0 = 0i32;
    let mut d1 = 0i32;
    let mut d2 = 0i32;
    let mut e0 = 0i32;
    let mut e1 = 0i32;
    let mut e2 = 0i32;
    let mut f0 = 0i32;
    let mut f1 = 0i32;
    let mut f2 = 0i32;
    let mut g0 = 0i32;
    let mut g1 = 0i32;
    let mut g2 = 0i32;
    let mut h0 = 0i32;
    let mut h1 = 0i32;
    let mut h2 = 0i32;
    let mut kk = 0usize;
    let k_dim = x.len();

    while kk + 16 <= k_dim {
        let x0 = x[kk] as i32;
        let x1 = x[kk + 1] as i32;
        let x2 = x[kk + 2] as i32;
        let x3 = x[kk + 3] as i32;
        let x4 = x[kk + 4] as i32;
        let x5 = x[kk + 5] as i32;
        let x6 = x[kk + 6] as i32;
        let x7 = x[kk + 7] as i32;
        let x8 = x[kk + 8] as i32;
        let x9 = x[kk + 9] as i32;
        let x10 = x[kk + 10] as i32;
        let x11 = x[kk + 11] as i32;
        let x12 = x[kk + 12] as i32;
        let x13 = x[kk + 13] as i32;
        let x14 = x[kk + 14] as i32;
        let x15 = x[kk + 15] as i32;

        a0 += row0[kk] as i32 * x0;
        a1 += row1[kk] as i32 * x0;
        a2 += row2[kk] as i32 * x0;
        b0 += row0[kk + 1] as i32 * x1;
        b1 += row1[kk + 1] as i32 * x1;
        b2 += row2[kk + 1] as i32 * x1;
        c0 += row0[kk + 2] as i32 * x2;
        c1 += row1[kk + 2] as i32 * x2;
        c2 += row2[kk + 2] as i32 * x2;
        d0 += row0[kk + 3] as i32 * x3;
        d1 += row1[kk + 3] as i32 * x3;
        d2 += row2[kk + 3] as i32 * x3;
        e0 += row0[kk + 4] as i32 * x4;
        e1 += row1[kk + 4] as i32 * x4;
        e2 += row2[kk + 4] as i32 * x4;
        f0 += row0[kk + 5] as i32 * x5;
        f1 += row1[kk + 5] as i32 * x5;
        f2 += row2[kk + 5] as i32 * x5;
        g0 += row0[kk + 6] as i32 * x6;
        g1 += row1[kk + 6] as i32 * x6;
        g2 += row2[kk + 6] as i32 * x6;
        h0 += row0[kk + 7] as i32 * x7;
        h1 += row1[kk + 7] as i32 * x7;
        h2 += row2[kk + 7] as i32 * x7;
        a0 += row0[kk + 8] as i32 * x8;
        a1 += row1[kk + 8] as i32 * x8;
        a2 += row2[kk + 8] as i32 * x8;
        b0 += row0[kk + 9] as i32 * x9;
        b1 += row1[kk + 9] as i32 * x9;
        b2 += row2[kk + 9] as i32 * x9;
        c0 += row0[kk + 10] as i32 * x10;
        c1 += row1[kk + 10] as i32 * x10;
        c2 += row2[kk + 10] as i32 * x10;
        d0 += row0[kk + 11] as i32 * x11;
        d1 += row1[kk + 11] as i32 * x11;
        d2 += row2[kk + 11] as i32 * x11;
        e0 += row0[kk + 12] as i32 * x12;
        e1 += row1[kk + 12] as i32 * x12;
        e2 += row2[kk + 12] as i32 * x12;
        f0 += row0[kk + 13] as i32 * x13;
        f1 += row1[kk + 13] as i32 * x13;
        f2 += row2[kk + 13] as i32 * x13;
        g0 += row0[kk + 14] as i32 * x14;
        g1 += row1[kk + 14] as i32 * x14;
        g2 += row2[kk + 14] as i32 * x14;
        h0 += row0[kk + 15] as i32 * x15;
        h1 += row1[kk + 15] as i32 * x15;
        h2 += row2[kk + 15] as i32 * x15;
        kk += 16;
    }

    while kk + 8 <= k_dim {
        let x0 = x[kk] as i32;
        let x1 = x[kk + 1] as i32;
        let x2 = x[kk + 2] as i32;
        let x3 = x[kk + 3] as i32;
        let x4 = x[kk + 4] as i32;
        let x5 = x[kk + 5] as i32;
        let x6 = x[kk + 6] as i32;
        let x7 = x[kk + 7] as i32;

        a0 += row0[kk] as i32 * x0 + row0[kk + 4] as i32 * x4;
        a1 += row1[kk] as i32 * x0 + row1[kk + 4] as i32 * x4;
        a2 += row2[kk] as i32 * x0 + row2[kk + 4] as i32 * x4;
        b0 += row0[kk + 1] as i32 * x1 + row0[kk + 5] as i32 * x5;
        b1 += row1[kk + 1] as i32 * x1 + row1[kk + 5] as i32 * x5;
        b2 += row2[kk + 1] as i32 * x1 + row2[kk + 5] as i32 * x5;
        c0 += row0[kk + 2] as i32 * x2 + row0[kk + 6] as i32 * x6;
        c1 += row1[kk + 2] as i32 * x2 + row1[kk + 6] as i32 * x6;
        c2 += row2[kk + 2] as i32 * x2 + row2[kk + 6] as i32 * x6;
        d0 += row0[kk + 3] as i32 * x3 + row0[kk + 7] as i32 * x7;
        d1 += row1[kk + 3] as i32 * x3 + row1[kk + 7] as i32 * x7;
        d2 += row2[kk + 3] as i32 * x3 + row2[kk + 7] as i32 * x7;
        kk += 8;
    }

    while kk + 4 <= k_dim {
        let x0 = x[kk] as i32;
        let x1 = x[kk + 1] as i32;
        let x2 = x[kk + 2] as i32;
        let x3 = x[kk + 3] as i32;
        a0 += row0[kk] as i32 * x0;
        a1 += row1[kk] as i32 * x0;
        a2 += row2[kk] as i32 * x0;
        b0 += row0[kk + 1] as i32 * x1;
        b1 += row1[kk + 1] as i32 * x1;
        b2 += row2[kk + 1] as i32 * x1;
        c0 += row0[kk + 2] as i32 * x2;
        c1 += row1[kk + 2] as i32 * x2;
        c2 += row2[kk + 2] as i32 * x2;
        d0 += row0[kk + 3] as i32 * x3;
        d1 += row1[kk + 3] as i32 * x3;
        d2 += row2[kk + 3] as i32 * x3;
        kk += 4;
    }

    let mut sum0 = a0 + b0 + c0 + d0 + e0 + f0 + g0 + h0;
    let mut sum1 = a1 + b1 + c1 + d1 + e1 + f1 + g1 + h1;
    let mut sum2 = a2 + b2 + c2 + d2 + e2 + f2 + g2 + h2;
    while kk < k_dim {
        let xv = x[kk] as i32;
        sum0 += row0[kk] as i32 * xv;
        sum1 += row1[kk] as i32 * xv;
        sum2 += row2[kk] as i32 * xv;
        kk += 1;
    }
    (
        (sum0 as f32) * x_scale * scale0,
        (sum1 as f32) * x_scale * scale1,
        (sum2 as f32) * x_scale * scale2,
    )
}

#[inline]
pub(crate) fn dot_unrolled_f32_i8(x: &[f32], row: &[i8], scale: f32) -> f32 {
    if let Some(sum) = dot_f32_i8_arch(x, row, scale) {
        sum
    } else {
        dot_unrolled_f32_i8_portable(x, row, scale)
    }
}

#[inline]
fn matvec_rowmajor_serial(
    x: &[f32],
    w_rowmajor: &[f32],
    n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
) {
    for i in 0..n_rows {
        let row = &w_rowmajor[i * k_dim..(i + 1) * k_dim];
        out[i] = dot_unrolled(x, row);
    }
}

#[inline]
fn matvec_rowmajor_serial_f32_bf16(
    x: &[f32],
    w_rowmajor: &[bf16],
    n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
) {
    for i in 0..n_rows {
        let row = &w_rowmajor[i * k_dim..(i + 1) * k_dim];
        out[i] = dot_unrolled_f32_bf16(x, row);
    }
}

#[inline]
fn matvec_rowmajor_serial_f32_f16(
    x: &[f32],
    w_rowmajor: &[f16],
    n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
) {
    for i in 0..n_rows {
        let row = &w_rowmajor[i * k_dim..(i + 1) * k_dim];
        out[i] = dot_unrolled_f32_f16(x, row);
    }
}

#[inline]
fn matvec_rowmajor_serial_f32_i8(
    x: &[f32],
    w_rowmajor: &[i8],
    scale: f32,
    n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
) {
    #[cfg(all(feature = "arm64-int8-kernels", target_arch = "aarch64"))]
    {
        let mut i = 0usize;
        while i + 1 < n_rows {
            let row0 = &w_rowmajor[i * k_dim..(i + 1) * k_dim];
            let row1 = &w_rowmajor[(i + 1) * k_dim..(i + 2) * k_dim];
            let (s0, s1) = dot2_unrolled_f32_i8(x, row0, scale, row1, scale);
            out[i] = s0;
            out[i + 1] = s1;
            i += 2;
        }
        if i < n_rows {
            let row = &w_rowmajor[i * k_dim..(i + 1) * k_dim];
            out[i] = dot_unrolled_f32_i8(x, row, scale);
        }
        return;
    }

    #[cfg(not(all(feature = "arm64-int8-kernels", target_arch = "aarch64")))]
    {
        for i in 0..n_rows {
            let row = &w_rowmajor[i * k_dim..(i + 1) * k_dim];
            out[i] = dot_unrolled_f32_i8(x, row, scale);
        }
    }
}

fn matvec_rowmajor_rowwise_parallel(
    x: &[f32],
    w_rowmajor: &[f32],
    _n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
) {
    out.par_iter_mut().enumerate().for_each(|(i, out_val)| {
        let row = &w_rowmajor[i * k_dim..(i + 1) * k_dim];
        *out_val = dot_unrolled(x, row);
    });
}

#[inline]
fn matvec_rowmajor_rowwise_parallel_f32_bf16(
    x: &[f32],
    w_rowmajor: &[bf16],
    _n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
) {
    out.par_chunks_mut(MIXED_ROW_PAR_CHUNK_ROWS)
        .enumerate()
        .for_each(|(chunk_idx, out_chunk)| {
            let row_start = chunk_idx * MIXED_ROW_PAR_CHUNK_ROWS;
            let mut offset = 0usize;
            while offset + 1 < out_chunk.len() {
                let row0_idx = row_start + offset;
                let row1_idx = row0_idx + 1;
                let row0 = &w_rowmajor[row0_idx * k_dim..(row0_idx + 1) * k_dim];
                let row1 = &w_rowmajor[row1_idx * k_dim..(row1_idx + 1) * k_dim];
                let (s0, s1) = dot2_unrolled_f32_bf16(x, row0, row1);
                out_chunk[offset] = s0;
                out_chunk[offset + 1] = s1;
                offset += 2;
            }
            if offset < out_chunk.len() {
                let row_idx = row_start + offset;
                let row = &w_rowmajor[row_idx * k_dim..(row_idx + 1) * k_dim];
                out_chunk[offset] = dot_unrolled_f32_bf16(x, row);
            }
        });
}

#[inline]
fn matvec_rowmajor_rowwise_parallel_f32_f16(
    x: &[f32],
    w_rowmajor: &[f16],
    _n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
) {
    out.par_chunks_mut(MIXED_ROW_PAR_CHUNK_ROWS)
        .enumerate()
        .for_each(|(chunk_idx, out_chunk)| {
            let row_start = chunk_idx * MIXED_ROW_PAR_CHUNK_ROWS;
            let mut offset = 0usize;
            while offset + 1 < out_chunk.len() {
                let row0_idx = row_start + offset;
                let row1_idx = row0_idx + 1;
                let row0 = &w_rowmajor[row0_idx * k_dim..(row0_idx + 1) * k_dim];
                let row1 = &w_rowmajor[row1_idx * k_dim..(row1_idx + 1) * k_dim];
                let (s0, s1) = dot2_unrolled_f32_f16(x, row0, row1);
                out_chunk[offset] = s0;
                out_chunk[offset + 1] = s1;
                offset += 2;
            }
            if offset < out_chunk.len() {
                let row_idx = row_start + offset;
                let row = &w_rowmajor[row_idx * k_dim..(row_idx + 1) * k_dim];
                out_chunk[offset] = dot_unrolled_f32_f16(x, row);
            }
        });
}

#[inline]
fn matvec_rowmajor_rowwise_parallel_f32_i8(
    x: &[f32],
    w_rowmajor: &[i8],
    scale: f32,
    _n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
) {
    out.par_chunks_mut(MATVEC_I8_PAR_CHUNK_ROWS)
        .enumerate()
        .for_each(|(chunk_idx, out_chunk)| {
            let row_start = chunk_idx * MATVEC_I8_PAR_CHUNK_ROWS;

            #[cfg(all(feature = "arm64-int8-kernels", target_arch = "aarch64"))]
            {
                let mut offset = 0usize;
                while offset + 1 < out_chunk.len() {
                    let row0_idx = row_start + offset;
                    let row1_idx = row0_idx + 1;
                    let row0 = &w_rowmajor[row0_idx * k_dim..(row0_idx + 1) * k_dim];
                    let row1 = &w_rowmajor[row1_idx * k_dim..(row1_idx + 1) * k_dim];
                    let (s0, s1) = dot2_unrolled_f32_i8(x, row0, scale, row1, scale);
                    out_chunk[offset] = s0;
                    out_chunk[offset + 1] = s1;
                    offset += 2;
                }
                if offset < out_chunk.len() {
                    let row_idx = row_start + offset;
                    let row = &w_rowmajor[row_idx * k_dim..(row_idx + 1) * k_dim];
                    out_chunk[offset] = dot_unrolled_f32_i8(x, row, scale);
                }
            }

            #[cfg(not(all(feature = "arm64-int8-kernels", target_arch = "aarch64")))]
            {
                for (offset, out_val) in out_chunk.iter_mut().enumerate() {
                    let row_idx = row_start + offset;
                    let row = &w_rowmajor[row_idx * k_dim..(row_idx + 1) * k_dim];
                    *out_val = dot_unrolled_f32_i8(x, row, scale);
                }
            }
        });
}

#[inline]
fn matvec_rowmajor_block_parallel(
    x: &[f32],
    w_rowmajor: &[f32],
    _n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
) {
    out.par_chunks_mut(MATVEC_BLOCK_ROWS)
        .enumerate()
        .for_each(|(block_idx, out_chunk)| {
            let row_start = block_idx * MATVEC_BLOCK_ROWS;
            let rows = out_chunk.len();
            let w_block = &w_rowmajor[row_start * k_dim..(row_start + rows) * k_dim];
            let mut acc = [0.0f32; MATVEC_BLOCK_ROWS];

            let mut kk = 0usize;
            while kk + 8 <= k_dim {
                let x0 = x[kk];
                let x1 = x[kk + 1];
                let x2 = x[kk + 2];
                let x3 = x[kk + 3];
                let x4 = x[kk + 4];
                let x5 = x[kk + 5];
                let x6 = x[kk + 6];
                let x7 = x[kk + 7];
                for r in 0..rows {
                    let base = r * k_dim + kk;
                    acc[r] += w_block[base] * x0
                        + w_block[base + 1] * x1
                        + w_block[base + 2] * x2
                        + w_block[base + 3] * x3
                        + w_block[base + 4] * x4
                        + w_block[base + 5] * x5
                        + w_block[base + 6] * x6
                        + w_block[base + 7] * x7;
                }
                kk += 8;
            }

            while kk < k_dim {
                let xv = x[kk];
                for r in 0..rows {
                    acc[r] += w_block[r * k_dim + kk] * xv;
                }
                kk += 1;
            }

            out_chunk.copy_from_slice(&acc[..rows]);
        });
}

#[inline]
fn matvec_rowmajor_block_parallel_f32_bf16(
    x: &[f32],
    w_rowmajor: &[bf16],
    _n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
) {
    out.par_chunks_mut(MATVEC_BLOCK_ROWS)
        .enumerate()
        .for_each(|(block_idx, out_chunk)| {
            let row_start = block_idx * MATVEC_BLOCK_ROWS;
            let rows = out_chunk.len();
            let w_block = &w_rowmajor[row_start * k_dim..(row_start + rows) * k_dim];
            let mut acc = [0.0f32; MATVEC_BLOCK_ROWS];

            let mut kk = 0usize;
            while kk + 8 <= k_dim {
                let x0 = x[kk];
                let x1 = x[kk + 1];
                let x2 = x[kk + 2];
                let x3 = x[kk + 3];
                let x4 = x[kk + 4];
                let x5 = x[kk + 5];
                let x6 = x[kk + 6];
                let x7 = x[kk + 7];
                for r in 0..rows {
                    let base = r * k_dim + kk;
                    acc[r] += w_block[base].to_f32() * x0
                        + w_block[base + 1].to_f32() * x1
                        + w_block[base + 2].to_f32() * x2
                        + w_block[base + 3].to_f32() * x3
                        + w_block[base + 4].to_f32() * x4
                        + w_block[base + 5].to_f32() * x5
                        + w_block[base + 6].to_f32() * x6
                        + w_block[base + 7].to_f32() * x7;
                }
                kk += 8;
            }

            while kk < k_dim {
                let xv = x[kk];
                for r in 0..rows {
                    acc[r] += w_block[r * k_dim + kk].to_f32() * xv;
                }
                kk += 1;
            }

            out_chunk.copy_from_slice(&acc[..rows]);
        });
}

fn matvec_rowmajor_block_parallel_f32_i8(
    x: &[f32],
    w_rowmajor: &[i8],
    scale: f32,
    _n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
) {
    out.par_chunks_mut(MATVEC_BLOCK_ROWS)
        .enumerate()
        .for_each(|(block_idx, out_chunk)| {
            let row_start = block_idx * MATVEC_BLOCK_ROWS;
            let rows = out_chunk.len();
            let w_block = &w_rowmajor[row_start * k_dim..(row_start + rows) * k_dim];
            let mut acc = [0.0f32; MATVEC_BLOCK_ROWS];

            let mut kk = 0usize;
            while kk + 16 <= k_dim {
                let x0 = x[kk];
                let x1 = x[kk + 1];
                let x2 = x[kk + 2];
                let x3 = x[kk + 3];
                let x4 = x[kk + 4];
                let x5 = x[kk + 5];
                let x6 = x[kk + 6];
                let x7 = x[kk + 7];
                let x8 = x[kk + 8];
                let x9 = x[kk + 9];
                let x10 = x[kk + 10];
                let x11 = x[kk + 11];
                let x12 = x[kk + 12];
                let x13 = x[kk + 13];
                let x14 = x[kk + 14];
                let x15 = x[kk + 15];
                for r in 0..rows {
                    let base = r * k_dim + kk;
                    acc[r] += w_block[base] as f32 * x0
                        + w_block[base + 1] as f32 * x1
                        + w_block[base + 2] as f32 * x2
                        + w_block[base + 3] as f32 * x3
                        + w_block[base + 4] as f32 * x4
                        + w_block[base + 5] as f32 * x5
                        + w_block[base + 6] as f32 * x6
                        + w_block[base + 7] as f32 * x7
                        + w_block[base + 8] as f32 * x8
                        + w_block[base + 9] as f32 * x9
                        + w_block[base + 10] as f32 * x10
                        + w_block[base + 11] as f32 * x11
                        + w_block[base + 12] as f32 * x12
                        + w_block[base + 13] as f32 * x13
                        + w_block[base + 14] as f32 * x14
                        + w_block[base + 15] as f32 * x15;
                }
                kk += 16;
            }

            while kk + 8 <= k_dim {
                let x0 = x[kk];
                let x1 = x[kk + 1];
                let x2 = x[kk + 2];
                let x3 = x[kk + 3];
                let x4 = x[kk + 4];
                let x5 = x[kk + 5];
                let x6 = x[kk + 6];
                let x7 = x[kk + 7];
                for r in 0..rows {
                    let base = r * k_dim + kk;
                    acc[r] += w_block[base] as f32 * x0
                        + w_block[base + 1] as f32 * x1
                        + w_block[base + 2] as f32 * x2
                        + w_block[base + 3] as f32 * x3
                        + w_block[base + 4] as f32 * x4
                        + w_block[base + 5] as f32 * x5
                        + w_block[base + 6] as f32 * x6
                        + w_block[base + 7] as f32 * x7;
                }
                kk += 8;
            }

            while kk < k_dim {
                let xv = x[kk];
                for r in 0..rows {
                    acc[r] += w_block[r * k_dim + kk] as f32 * xv;
                }
                kk += 1;
            }

            for r in 0..rows {
                out_chunk[r] = acc[r] * scale;
            }
        });
}

pub fn matvec_rowmajor_parallel(
    x: &[f32],
    w_rowmajor: &[f32],
    n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
) {
    assert_eq!(x.len(), k_dim, "x len / k_dim mismatch");
    assert_eq!(w_rowmajor.len(), n_rows * k_dim, "weight size mismatch");
    assert_eq!(out.len(), n_rows, "out size mismatch");

    if n_rows < MATVEC_PAR_THRESHOLD {
        matvec_rowmajor_serial(x, w_rowmajor, n_rows, k_dim, out);
    } else if n_rows >= MATVEC_BLOCK_THRESHOLD {
        matvec_rowmajor_block_parallel(x, w_rowmajor, n_rows, k_dim, out);
    } else {
        matvec_rowmajor_rowwise_parallel(x, w_rowmajor, n_rows, k_dim, out);
    }
}

#[inline]
pub fn matvec_rowmajor_parallel_f32_bf16(
    x: &[f32],
    w_rowmajor: &[bf16],
    n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
) {
    assert_eq!(x.len(), k_dim, "x len / k_dim mismatch");
    assert_eq!(w_rowmajor.len(), n_rows * k_dim, "weight size mismatch");
    assert_eq!(out.len(), n_rows, "out size mismatch");

    if n_rows < MATVEC_PAR_THRESHOLD {
        matvec_rowmajor_serial_f32_bf16(x, w_rowmajor, n_rows, k_dim, out);
    } else if should_use_mixed_matvec_block_kernel(n_rows) {
        matvec_rowmajor_block_parallel_f32_bf16(x, w_rowmajor, n_rows, k_dim, out);
    } else {
        matvec_rowmajor_rowwise_parallel_f32_bf16(x, w_rowmajor, n_rows, k_dim, out);
    }
}

#[inline]
pub fn matvec_rowmajor_parallel_f32_f16(
    x: &[f32],
    w_rowmajor: &[f16],
    n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
) {
    assert_eq!(x.len(), k_dim, "x len / k_dim mismatch");
    assert_eq!(w_rowmajor.len(), n_rows * k_dim, "weight size mismatch");
    assert_eq!(out.len(), n_rows, "out size mismatch");

    if n_rows < MATVEC_PAR_THRESHOLD {
        matvec_rowmajor_serial_f32_f16(x, w_rowmajor, n_rows, k_dim, out);
    } else {
        matvec_rowmajor_rowwise_parallel_f32_f16(x, w_rowmajor, n_rows, k_dim, out);
    }
}

#[inline]
pub fn matvec_rowmajor_parallel_f32_i8(
    x: &[f32],
    w_rowmajor: &[i8],
    scale: f32,
    n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
) {
    assert_eq!(x.len(), k_dim, "x len / k_dim mismatch");
    assert_eq!(w_rowmajor.len(), n_rows * k_dim, "weight size mismatch");
    assert_eq!(out.len(), n_rows, "out size mismatch");

    if n_rows < MATVEC_PAR_THRESHOLD {
        matvec_rowmajor_serial_f32_i8(x, w_rowmajor, scale, n_rows, k_dim, out);
    } else if should_use_i8_block_kernel(n_rows, k_dim) {
        matvec_rowmajor_block_parallel_f32_i8(x, w_rowmajor, scale, n_rows, k_dim, out);
    } else {
        matvec_rowmajor_rowwise_parallel_f32_i8(x, w_rowmajor, scale, n_rows, k_dim, out);
    }
}

#[inline]
pub fn matvec_rowmajor_parallel_f32_i8_matmul(
    x: &[f32],
    w_rowmajor: &[i8],
    scale: f32,
    n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
) {
    assert_eq!(x.len(), k_dim, "x len / k_dim mismatch");
    assert_eq!(w_rowmajor.len(), n_rows * k_dim, "weight size mismatch");
    assert_eq!(out.len(), n_rows, "out size mismatch");

    if n_rows < MATVEC_PAR_THRESHOLD {
        matvec_rowmajor_serial_f32_i8(x, w_rowmajor, scale, n_rows, k_dim, out);
    } else if should_use_i8_matmul_block_kernel(n_rows, k_dim) {
        matvec_rowmajor_block_parallel_f32_i8(x, w_rowmajor, scale, n_rows, k_dim, out);
    } else {
        matvec_rowmajor_rowwise_parallel_f32_i8(x, w_rowmajor, scale, n_rows, k_dim, out);
    }
}

#[inline]
pub fn matvec_rowmajor_parallel_bf16_f32(
    x: &[bf16],
    w_rowmajor: &[f32],
    n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
) {
    with_bf16_input_as_f32(x, |x_f32| {
        matvec_rowmajor_parallel(x_f32, w_rowmajor, n_rows, k_dim, out);
    });
}

#[inline]
pub fn matvec_rowmajor_parallel_bf16_bf16(
    x: &[bf16],
    w_rowmajor: &[bf16],
    n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
) {
    with_bf16_input_as_f32(x, |x_f32| {
        matvec_rowmajor_parallel_f32_bf16(x_f32, w_rowmajor, n_rows, k_dim, out);
    });
}

#[inline]
pub fn matvec_rowmajor_parallel_bf16_i8(
    x: &[bf16],
    w_rowmajor: &[i8],
    scale: f32,
    n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
) {
    with_bf16_input_as_f32(x, |x_f32| {
        matvec_rowmajor_parallel_f32_i8(x_f32, w_rowmajor, scale, n_rows, k_dim, out);
    });
}

#[inline]
pub fn matvec_rowmajor_parallel_i8_f32(
    x: &[i8],
    x_scale: f32,
    w_rowmajor: &[f32],
    n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
) {
    with_i8_input_as_f32(x, x_scale, |x_f32| {
        matvec_rowmajor_parallel(x_f32, w_rowmajor, n_rows, k_dim, out);
    });
}

#[inline]
pub fn matvec_rowmajor_parallel_i8_bf16(
    x: &[i8],
    x_scale: f32,
    w_rowmajor: &[bf16],
    n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
) {
    with_i8_input_as_f32(x, x_scale, |x_f32| {
        matvec_rowmajor_parallel_f32_bf16(x_f32, w_rowmajor, n_rows, k_dim, out);
    });
}

#[inline]
pub fn matvec_rowmajor_parallel_i8_i8(
    x: &[i8],
    x_scale: f32,
    w_rowmajor: &[i8],
    w_scale: f32,
    n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
) {
    assert_eq!(x.len(), k_dim, "x len / k_dim mismatch");
    assert_eq!(w_rowmajor.len(), n_rows * k_dim, "weight size mismatch");
    assert_eq!(out.len(), n_rows, "out size mismatch");

    if n_rows < MATVEC_PAR_THRESHOLD {
        #[cfg(all(feature = "arm64-int8-kernels", target_arch = "aarch64"))]
        {
            let mut i = 0usize;
            while i + 1 < n_rows {
                let row0 = &w_rowmajor[i * k_dim..(i + 1) * k_dim];
                let row1 = &w_rowmajor[(i + 1) * k_dim..(i + 2) * k_dim];
                let (s0, s1) = dot2_unrolled_i8_i8(x, x_scale, row0, w_scale, row1, w_scale);
                out[i] = s0;
                out[i + 1] = s1;
                i += 2;
            }
            if i < n_rows {
                let row = &w_rowmajor[i * k_dim..(i + 1) * k_dim];
                out[i] = dot_unrolled_i8_i8(x, x_scale, row, w_scale);
            }
        }

        #[cfg(not(all(feature = "arm64-int8-kernels", target_arch = "aarch64")))]
        {
            for i in 0..n_rows {
                let row = &w_rowmajor[i * k_dim..(i + 1) * k_dim];
                out[i] = dot_unrolled_i8_i8(x, x_scale, row, w_scale);
            }
        }
    } else {
        out.par_chunks_mut(MATVEC_I8_PAR_CHUNK_ROWS)
            .enumerate()
            .for_each(|(chunk_idx, out_chunk)| {
                let row_start = chunk_idx * MATVEC_I8_PAR_CHUNK_ROWS;

                #[cfg(all(feature = "arm64-int8-kernels", target_arch = "aarch64"))]
                {
                    let mut offset = 0usize;
                    while offset + 1 < out_chunk.len() {
                        let row0_idx = row_start + offset;
                        let row1_idx = row0_idx + 1;
                        let row0 = &w_rowmajor[row0_idx * k_dim..(row0_idx + 1) * k_dim];
                        let row1 = &w_rowmajor[row1_idx * k_dim..(row1_idx + 1) * k_dim];
                        let (s0, s1) =
                            dot2_unrolled_i8_i8(x, x_scale, row0, w_scale, row1, w_scale);
                        out_chunk[offset] = s0;
                        out_chunk[offset + 1] = s1;
                        offset += 2;
                    }
                    if offset < out_chunk.len() {
                        let row_idx = row_start + offset;
                        let row = &w_rowmajor[row_idx * k_dim..(row_idx + 1) * k_dim];
                        out_chunk[offset] = dot_unrolled_i8_i8(x, x_scale, row, w_scale);
                    }
                }

                #[cfg(not(all(feature = "arm64-int8-kernels", target_arch = "aarch64")))]
                {
                    for (offset, out_val) in out_chunk.iter_mut().enumerate() {
                        let row_idx = row_start + offset;
                        let row = &w_rowmajor[row_idx * k_dim..(row_idx + 1) * k_dim];
                        *out_val = dot_unrolled_i8_i8(x, x_scale, row, w_scale);
                    }
                }
            });
    }
}

#[inline]
pub fn matvec_rowmajor_parallel_mixed(
    x: SliceRef<'_>,
    w_rowmajor: SliceRef<'_>,
    n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
) {
    assert_eq!(out.len(), n_rows, "out size mismatch");

    match (x, w_rowmajor) {
        (SliceRef::F32(x), SliceRef::F32(w)) => matvec_rowmajor_parallel(x, w, n_rows, k_dim, out),
        (SliceRef::F32(x), SliceRef::F16(w)) => {
            matvec_rowmajor_parallel_f32_f16(x, w, n_rows, k_dim, out);
        }
        (SliceRef::F32(x), SliceRef::BF16(w)) => {
            matvec_rowmajor_parallel_f32_bf16(x, w, n_rows, k_dim, out);
        }
        (SliceRef::F32(x), SliceRef::I8(w, scale)) => {
            matvec_rowmajor_parallel_f32_i8(x, w, scale, n_rows, k_dim, out);
        }
        (SliceRef::F16(x), SliceRef::F32(w)) => {
            with_f16_input_as_f32(x, |x_f32| {
                matvec_rowmajor_parallel(x_f32, w, n_rows, k_dim, out);
            });
        }
        (SliceRef::F16(x), SliceRef::F16(w)) => {
            with_f16_input_as_f32(x, |x_f32| {
                matvec_rowmajor_parallel_f32_f16(x_f32, w, n_rows, k_dim, out);
            });
        }
        (SliceRef::F16(x), SliceRef::BF16(w)) => {
            with_f16_input_as_f32(x, |x_f32| {
                matvec_rowmajor_parallel_f32_bf16(x_f32, w, n_rows, k_dim, out);
            });
        }
        (SliceRef::F16(x), SliceRef::I8(w, scale)) => {
            with_f16_input_as_f32(x, |x_f32| {
                matvec_rowmajor_parallel_f32_i8(x_f32, w, scale, n_rows, k_dim, out);
            });
        }
        (SliceRef::BF16(x), SliceRef::F32(w)) => {
            matvec_rowmajor_parallel_bf16_f32(x, w, n_rows, k_dim, out);
        }
        (SliceRef::BF16(x), SliceRef::BF16(w)) => {
            matvec_rowmajor_parallel_bf16_bf16(x, w, n_rows, k_dim, out);
        }
        (SliceRef::BF16(x), SliceRef::I8(w, scale)) => {
            matvec_rowmajor_parallel_bf16_i8(x, w, scale, n_rows, k_dim, out);
        }
        (SliceRef::BF16(x), SliceRef::F16(w)) => {
            with_bf16_input_as_f32(x, |x_f32| {
                matvec_rowmajor_parallel_f32_f16(x_f32, w, n_rows, k_dim, out);
            });
        }
        (SliceRef::I8(x, scale), SliceRef::F32(w)) => {
            matvec_rowmajor_parallel_i8_f32(x, scale, w, n_rows, k_dim, out);
        }
        (SliceRef::I8(x, scale), SliceRef::F16(w)) => {
            with_i8_input_as_f32(x, scale, |x_f32| {
                matvec_rowmajor_parallel_f32_f16(x_f32, w, n_rows, k_dim, out)
            });
        }
        (SliceRef::I8(x, scale), SliceRef::BF16(w)) => {
            matvec_rowmajor_parallel_i8_bf16(x, scale, w, n_rows, k_dim, out);
        }
        (SliceRef::I8(x, x_scale), SliceRef::I8(w, w_scale)) => {
            matvec_rowmajor_parallel_i8_i8(x, x_scale, w, w_scale, n_rows, k_dim, out);
        }
    }
}

#[inline]
pub(crate) fn dot2_unrolled(x: &[f32], row0: &[f32], row1: &[f32]) -> (f32, f32) {
    if let Some(sum) = dot2_f32_arch(x, row0, row1) {
        return sum;
    }

    let mut a0 = 0.0f32;
    let mut a1 = 0.0f32;
    let mut b0 = 0.0f32;
    let mut b1 = 0.0f32;
    let mut c0 = 0.0f32;
    let mut c1 = 0.0f32;
    let mut d0 = 0.0f32;
    let mut d1 = 0.0f32;
    let mut kk = 0usize;
    let k_dim = x.len();

    while kk + 8 <= k_dim {
        let x0 = x[kk];
        let x1 = x[kk + 1];
        let x2 = x[kk + 2];
        let x3 = x[kk + 3];
        let x4 = x[kk + 4];
        let x5 = x[kk + 5];
        let x6 = x[kk + 6];
        let x7 = x[kk + 7];

        a0 += row0[kk] * x0 + row0[kk + 4] * x4;
        a1 += row1[kk] * x0 + row1[kk + 4] * x4;
        b0 += row0[kk + 1] * x1 + row0[kk + 5] * x5;
        b1 += row1[kk + 1] * x1 + row1[kk + 5] * x5;
        c0 += row0[kk + 2] * x2 + row0[kk + 6] * x6;
        c1 += row1[kk + 2] * x2 + row1[kk + 6] * x6;
        d0 += row0[kk + 3] * x3 + row0[kk + 7] * x7;
        d1 += row1[kk + 3] * x3 + row1[kk + 7] * x7;
        kk += 8;
    }

    while kk + 4 <= k_dim {
        let x0 = x[kk];
        let x1 = x[kk + 1];
        let x2 = x[kk + 2];
        let x3 = x[kk + 3];
        a0 += row0[kk] * x0;
        a1 += row1[kk] * x0;
        b0 += row0[kk + 1] * x1;
        b1 += row1[kk + 1] * x1;
        c0 += row0[kk + 2] * x2;
        c1 += row1[kk + 2] * x2;
        d0 += row0[kk + 3] * x3;
        d1 += row1[kk + 3] * x3;
        kk += 4;
    }

    let mut sum0 = a0 + b0 + c0 + d0;
    let mut sum1 = a1 + b1 + c1 + d1;
    while kk < k_dim {
        let xv = x[kk];
        sum0 += row0[kk] * xv;
        sum1 += row1[kk] * xv;
        kk += 1;
    }
    (sum0, sum1)
}

#[inline]
pub(crate) fn dot2_unrolled_f32_bf16(x: &[f32], row0: &[bf16], row1: &[bf16]) -> (f32, f32) {
    if let Some(sum) = dot2_f32_bf16_arch(x, row0, row1) {
        return sum;
    }

    let mut a0 = 0.0f32;
    let mut a1 = 0.0f32;
    let mut b0 = 0.0f32;
    let mut b1 = 0.0f32;
    let mut c0 = 0.0f32;
    let mut c1 = 0.0f32;
    let mut d0 = 0.0f32;
    let mut d1 = 0.0f32;
    let mut kk = 0usize;
    let k_dim = x.len();

    while kk + 8 <= k_dim {
        let x0 = x[kk];
        let x1 = x[kk + 1];
        let x2 = x[kk + 2];
        let x3 = x[kk + 3];
        let x4 = x[kk + 4];
        let x5 = x[kk + 5];
        let x6 = x[kk + 6];
        let x7 = x[kk + 7];

        a0 += row0[kk].to_f32() * x0 + row0[kk + 4].to_f32() * x4;
        a1 += row1[kk].to_f32() * x0 + row1[kk + 4].to_f32() * x4;
        b0 += row0[kk + 1].to_f32() * x1 + row0[kk + 5].to_f32() * x5;
        b1 += row1[kk + 1].to_f32() * x1 + row1[kk + 5].to_f32() * x5;
        c0 += row0[kk + 2].to_f32() * x2 + row0[kk + 6].to_f32() * x6;
        c1 += row1[kk + 2].to_f32() * x2 + row1[kk + 6].to_f32() * x6;
        d0 += row0[kk + 3].to_f32() * x3 + row0[kk + 7].to_f32() * x7;
        d1 += row1[kk + 3].to_f32() * x3 + row1[kk + 7].to_f32() * x7;
        kk += 8;
    }

    while kk + 4 <= k_dim {
        let x0 = x[kk];
        let x1 = x[kk + 1];
        let x2 = x[kk + 2];
        let x3 = x[kk + 3];
        a0 += row0[kk].to_f32() * x0;
        a1 += row1[kk].to_f32() * x0;
        b0 += row0[kk + 1].to_f32() * x1;
        b1 += row1[kk + 1].to_f32() * x1;
        c0 += row0[kk + 2].to_f32() * x2;
        c1 += row1[kk + 2].to_f32() * x2;
        d0 += row0[kk + 3].to_f32() * x3;
        d1 += row1[kk + 3].to_f32() * x3;
        kk += 4;
    }

    let mut sum0 = a0 + b0 + c0 + d0;
    let mut sum1 = a1 + b1 + c1 + d1;
    while kk < k_dim {
        let xv = x[kk];
        sum0 += row0[kk].to_f32() * xv;
        sum1 += row1[kk].to_f32() * xv;
        kk += 1;
    }
    (sum0, sum1)
}

#[inline]
pub(crate) fn dot2_unrolled_f32_f16(x: &[f32], row0: &[f16], row1: &[f16]) -> (f32, f32) {
    if let Some(sum) = dot2_f32_f16_arch(x, row0, row1) {
        return sum;
    }

    let mut a0 = 0.0f32;
    let mut a1 = 0.0f32;
    let mut b0 = 0.0f32;
    let mut b1 = 0.0f32;
    let mut c0 = 0.0f32;
    let mut c1 = 0.0f32;
    let mut d0 = 0.0f32;
    let mut d1 = 0.0f32;
    let mut kk = 0usize;
    let k_dim = x.len();

    while kk + 8 <= k_dim {
        let x0 = x[kk];
        let x1 = x[kk + 1];
        let x2 = x[kk + 2];
        let x3 = x[kk + 3];
        let x4 = x[kk + 4];
        let x5 = x[kk + 5];
        let x6 = x[kk + 6];
        let x7 = x[kk + 7];

        a0 += row0[kk].to_f32() * x0 + row0[kk + 4].to_f32() * x4;
        a1 += row1[kk].to_f32() * x0 + row1[kk + 4].to_f32() * x4;
        b0 += row0[kk + 1].to_f32() * x1 + row0[kk + 5].to_f32() * x5;
        b1 += row1[kk + 1].to_f32() * x1 + row1[kk + 5].to_f32() * x5;
        c0 += row0[kk + 2].to_f32() * x2 + row0[kk + 6].to_f32() * x6;
        c1 += row1[kk + 2].to_f32() * x2 + row1[kk + 6].to_f32() * x6;
        d0 += row0[kk + 3].to_f32() * x3 + row0[kk + 7].to_f32() * x7;
        d1 += row1[kk + 3].to_f32() * x3 + row1[kk + 7].to_f32() * x7;
        kk += 8;
    }

    while kk + 4 <= k_dim {
        let x0 = x[kk];
        let x1 = x[kk + 1];
        let x2 = x[kk + 2];
        let x3 = x[kk + 3];
        a0 += row0[kk].to_f32() * x0;
        a1 += row1[kk].to_f32() * x0;
        b0 += row0[kk + 1].to_f32() * x1;
        b1 += row1[kk + 1].to_f32() * x1;
        c0 += row0[kk + 2].to_f32() * x2;
        c1 += row1[kk + 2].to_f32() * x2;
        d0 += row0[kk + 3].to_f32() * x3;
        d1 += row1[kk + 3].to_f32() * x3;
        kk += 4;
    }

    let mut sum0 = a0 + b0 + c0 + d0;
    let mut sum1 = a1 + b1 + c1 + d1;
    while kk < k_dim {
        let xv = x[kk];
        sum0 += row0[kk].to_f32() * xv;
        sum1 += row1[kk].to_f32() * xv;
        kk += 1;
    }
    (sum0, sum1)
}

#[inline]
pub(crate) fn dot2_unrolled_f32_i8(
    x: &[f32],
    row0: &[i8],
    scale0: f32,
    row1: &[i8],
    scale1: f32,
) -> (f32, f32) {
    if let Some(sum) = dot2_f32_i8_arch(x, row0, scale0, row1, scale1) {
        return sum;
    }

    let mut a0 = 0.0f32;
    let mut a1 = 0.0f32;
    let mut b0 = 0.0f32;
    let mut b1 = 0.0f32;
    let mut c0 = 0.0f32;
    let mut c1 = 0.0f32;
    let mut d0 = 0.0f32;
    let mut d1 = 0.0f32;
    let mut e0 = 0.0f32;
    let mut e1 = 0.0f32;
    let mut f0 = 0.0f32;
    let mut f1 = 0.0f32;
    let mut g0 = 0.0f32;
    let mut g1 = 0.0f32;
    let mut h0 = 0.0f32;
    let mut h1 = 0.0f32;
    let mut kk = 0usize;
    let k_dim = x.len();

    while kk + 16 <= k_dim {
        let x0 = x[kk];
        let x1 = x[kk + 1];
        let x2 = x[kk + 2];
        let x3 = x[kk + 3];
        let x4 = x[kk + 4];
        let x5 = x[kk + 5];
        let x6 = x[kk + 6];
        let x7 = x[kk + 7];
        let x8 = x[kk + 8];
        let x9 = x[kk + 9];
        let x10 = x[kk + 10];
        let x11 = x[kk + 11];
        let x12 = x[kk + 12];
        let x13 = x[kk + 13];
        let x14 = x[kk + 14];
        let x15 = x[kk + 15];

        a0 += row0[kk] as f32 * x0;
        a1 += row1[kk] as f32 * x0;
        b0 += row0[kk + 1] as f32 * x1;
        b1 += row1[kk + 1] as f32 * x1;
        c0 += row0[kk + 2] as f32 * x2;
        c1 += row1[kk + 2] as f32 * x2;
        d0 += row0[kk + 3] as f32 * x3;
        d1 += row1[kk + 3] as f32 * x3;
        e0 += row0[kk + 4] as f32 * x4;
        e1 += row1[kk + 4] as f32 * x4;
        f0 += row0[kk + 5] as f32 * x5;
        f1 += row1[kk + 5] as f32 * x5;
        g0 += row0[kk + 6] as f32 * x6;
        g1 += row1[kk + 6] as f32 * x6;
        h0 += row0[kk + 7] as f32 * x7;
        h1 += row1[kk + 7] as f32 * x7;
        a0 += row0[kk + 8] as f32 * x8;
        a1 += row1[kk + 8] as f32 * x8;
        b0 += row0[kk + 9] as f32 * x9;
        b1 += row1[kk + 9] as f32 * x9;
        c0 += row0[kk + 10] as f32 * x10;
        c1 += row1[kk + 10] as f32 * x10;
        d0 += row0[kk + 11] as f32 * x11;
        d1 += row1[kk + 11] as f32 * x11;
        e0 += row0[kk + 12] as f32 * x12;
        e1 += row1[kk + 12] as f32 * x12;
        f0 += row0[kk + 13] as f32 * x13;
        f1 += row1[kk + 13] as f32 * x13;
        g0 += row0[kk + 14] as f32 * x14;
        g1 += row1[kk + 14] as f32 * x14;
        h0 += row0[kk + 15] as f32 * x15;
        h1 += row1[kk + 15] as f32 * x15;
        kk += 16;
    }

    while kk + 8 <= k_dim {
        let x0 = x[kk];
        let x1 = x[kk + 1];
        let x2 = x[kk + 2];
        let x3 = x[kk + 3];
        let x4 = x[kk + 4];
        let x5 = x[kk + 5];
        let x6 = x[kk + 6];
        let x7 = x[kk + 7];

        a0 += row0[kk] as f32 * x0 + row0[kk + 4] as f32 * x4;
        a1 += row1[kk] as f32 * x0 + row1[kk + 4] as f32 * x4;
        b0 += row0[kk + 1] as f32 * x1 + row0[kk + 5] as f32 * x5;
        b1 += row1[kk + 1] as f32 * x1 + row1[kk + 5] as f32 * x5;
        c0 += row0[kk + 2] as f32 * x2 + row0[kk + 6] as f32 * x6;
        c1 += row1[kk + 2] as f32 * x2 + row1[kk + 6] as f32 * x6;
        d0 += row0[kk + 3] as f32 * x3 + row0[kk + 7] as f32 * x7;
        d1 += row1[kk + 3] as f32 * x3 + row1[kk + 7] as f32 * x7;
        kk += 8;
    }

    while kk + 4 <= k_dim {
        let x0 = x[kk];
        let x1 = x[kk + 1];
        let x2 = x[kk + 2];
        let x3 = x[kk + 3];
        a0 += row0[kk] as f32 * x0;
        a1 += row1[kk] as f32 * x0;
        b0 += row0[kk + 1] as f32 * x1;
        b1 += row1[kk + 1] as f32 * x1;
        c0 += row0[kk + 2] as f32 * x2;
        c1 += row1[kk + 2] as f32 * x2;
        d0 += row0[kk + 3] as f32 * x3;
        d1 += row1[kk + 3] as f32 * x3;
        kk += 4;
    }

    let mut sum0 = a0 + b0 + c0 + d0 + e0 + f0 + g0 + h0;
    let mut sum1 = a1 + b1 + c1 + d1 + e1 + f1 + g1 + h1;
    while kk < k_dim {
        let xv = x[kk];
        sum0 += row0[kk] as f32 * xv;
        sum1 += row1[kk] as f32 * xv;
        kk += 1;
    }
    (sum0 * scale0, sum1 * scale1)
}

#[inline]
pub(crate) fn dot3_unrolled(
    x: &[f32],
    row0: &[f32],
    row1: &[f32],
    row2: &[f32],
) -> (f32, f32, f32) {
    if let Some(sum) = dot3_f32_arch(x, row0, row1, row2) {
        return sum;
    }

    let mut a0 = 0.0f32;
    let mut a1 = 0.0f32;
    let mut a2 = 0.0f32;
    let mut b0 = 0.0f32;
    let mut b1 = 0.0f32;
    let mut b2 = 0.0f32;
    let mut c0 = 0.0f32;
    let mut c1 = 0.0f32;
    let mut c2 = 0.0f32;
    let mut d0 = 0.0f32;
    let mut d1 = 0.0f32;
    let mut d2 = 0.0f32;
    let mut e0 = 0.0f32;
    let mut e1 = 0.0f32;
    let mut e2 = 0.0f32;
    let mut f0 = 0.0f32;
    let mut f1 = 0.0f32;
    let mut f2 = 0.0f32;
    let mut g0 = 0.0f32;
    let mut g1 = 0.0f32;
    let mut g2 = 0.0f32;
    let mut h0 = 0.0f32;
    let mut h1 = 0.0f32;
    let mut h2 = 0.0f32;
    let mut kk = 0usize;
    let k_dim = x.len();

    while kk + 16 <= k_dim {
        let x0 = x[kk];
        let x1 = x[kk + 1];
        let x2 = x[kk + 2];
        let x3 = x[kk + 3];
        let x4 = x[kk + 4];
        let x5 = x[kk + 5];
        let x6 = x[kk + 6];
        let x7 = x[kk + 7];
        let x8 = x[kk + 8];
        let x9 = x[kk + 9];
        let x10 = x[kk + 10];
        let x11 = x[kk + 11];
        let x12 = x[kk + 12];
        let x13 = x[kk + 13];
        let x14 = x[kk + 14];
        let x15 = x[kk + 15];

        a0 += row0[kk] * x0;
        a1 += row1[kk] * x0;
        a2 += row2[kk] * x0;
        b0 += row0[kk + 1] * x1;
        b1 += row1[kk + 1] * x1;
        b2 += row2[kk + 1] * x1;
        c0 += row0[kk + 2] * x2;
        c1 += row1[kk + 2] * x2;
        c2 += row2[kk + 2] * x2;
        d0 += row0[kk + 3] * x3;
        d1 += row1[kk + 3] * x3;
        d2 += row2[kk + 3] * x3;
        e0 += row0[kk + 4] * x4;
        e1 += row1[kk + 4] * x4;
        e2 += row2[kk + 4] * x4;
        f0 += row0[kk + 5] * x5;
        f1 += row1[kk + 5] * x5;
        f2 += row2[kk + 5] * x5;
        g0 += row0[kk + 6] * x6;
        g1 += row1[kk + 6] * x6;
        g2 += row2[kk + 6] * x6;
        h0 += row0[kk + 7] * x7;
        h1 += row1[kk + 7] * x7;
        h2 += row2[kk + 7] * x7;
        a0 += row0[kk + 8] * x8;
        a1 += row1[kk + 8] * x8;
        a2 += row2[kk + 8] * x8;
        b0 += row0[kk + 9] * x9;
        b1 += row1[kk + 9] * x9;
        b2 += row2[kk + 9] * x9;
        c0 += row0[kk + 10] * x10;
        c1 += row1[kk + 10] * x10;
        c2 += row2[kk + 10] * x10;
        d0 += row0[kk + 11] * x11;
        d1 += row1[kk + 11] * x11;
        d2 += row2[kk + 11] * x11;
        e0 += row0[kk + 12] * x12;
        e1 += row1[kk + 12] * x12;
        e2 += row2[kk + 12] * x12;
        f0 += row0[kk + 13] * x13;
        f1 += row1[kk + 13] * x13;
        f2 += row2[kk + 13] * x13;
        g0 += row0[kk + 14] * x14;
        g1 += row1[kk + 14] * x14;
        g2 += row2[kk + 14] * x14;
        h0 += row0[kk + 15] * x15;
        h1 += row1[kk + 15] * x15;
        h2 += row2[kk + 15] * x15;
        kk += 16;
    }

    while kk + 8 <= k_dim {
        let x0 = x[kk];
        let x1 = x[kk + 1];
        let x2 = x[kk + 2];
        let x3 = x[kk + 3];
        let x4 = x[kk + 4];
        let x5 = x[kk + 5];
        let x6 = x[kk + 6];
        let x7 = x[kk + 7];

        a0 += row0[kk] * x0 + row0[kk + 4] * x4;
        a1 += row1[kk] * x0 + row1[kk + 4] * x4;
        a2 += row2[kk] * x0 + row2[kk + 4] * x4;
        b0 += row0[kk + 1] * x1 + row0[kk + 5] * x5;
        b1 += row1[kk + 1] * x1 + row1[kk + 5] * x5;
        b2 += row2[kk + 1] * x1 + row2[kk + 5] * x5;
        c0 += row0[kk + 2] * x2 + row0[kk + 6] * x6;
        c1 += row1[kk + 2] * x2 + row1[kk + 6] * x6;
        c2 += row2[kk + 2] * x2 + row2[kk + 6] * x6;
        d0 += row0[kk + 3] * x3 + row0[kk + 7] * x7;
        d1 += row1[kk + 3] * x3 + row1[kk + 7] * x7;
        d2 += row2[kk + 3] * x3 + row2[kk + 7] * x7;
        kk += 8;
    }

    while kk + 4 <= k_dim {
        let x0 = x[kk];
        let x1 = x[kk + 1];
        let x2 = x[kk + 2];
        let x3 = x[kk + 3];
        a0 += row0[kk] * x0;
        a1 += row1[kk] * x0;
        a2 += row2[kk] * x0;
        b0 += row0[kk + 1] * x1;
        b1 += row1[kk + 1] * x1;
        b2 += row2[kk + 1] * x1;
        c0 += row0[kk + 2] * x2;
        c1 += row1[kk + 2] * x2;
        c2 += row2[kk + 2] * x2;
        d0 += row0[kk + 3] * x3;
        d1 += row1[kk + 3] * x3;
        d2 += row2[kk + 3] * x3;
        kk += 4;
    }

    let mut sum0 = a0 + b0 + c0 + d0 + e0 + f0 + g0 + h0;
    let mut sum1 = a1 + b1 + c1 + d1 + e1 + f1 + g1 + h1;
    let mut sum2 = a2 + b2 + c2 + d2 + e2 + f2 + g2 + h2;
    while kk < k_dim {
        let xv = x[kk];
        sum0 += row0[kk] * xv;
        sum1 += row1[kk] * xv;
        sum2 += row2[kk] * xv;
        kk += 1;
    }
    (sum0, sum1, sum2)
}

#[inline]
pub(crate) fn dot3_unrolled_f32_bf16(
    x: &[f32],
    row0: &[bf16],
    row1: &[bf16],
    row2: &[bf16],
) -> (f32, f32, f32) {
    if let Some(sum) = dot3_f32_bf16_arch(x, row0, row1, row2) {
        return sum;
    }

    let mut a0 = 0.0f32;
    let mut a1 = 0.0f32;
    let mut a2 = 0.0f32;
    let mut b0 = 0.0f32;
    let mut b1 = 0.0f32;
    let mut b2 = 0.0f32;
    let mut c0 = 0.0f32;
    let mut c1 = 0.0f32;
    let mut c2 = 0.0f32;
    let mut d0 = 0.0f32;
    let mut d1 = 0.0f32;
    let mut d2 = 0.0f32;
    let mut e0 = 0.0f32;
    let mut e1 = 0.0f32;
    let mut e2 = 0.0f32;
    let mut f0 = 0.0f32;
    let mut f1 = 0.0f32;
    let mut f2 = 0.0f32;
    let mut g0 = 0.0f32;
    let mut g1 = 0.0f32;
    let mut g2 = 0.0f32;
    let mut h0 = 0.0f32;
    let mut h1 = 0.0f32;
    let mut h2 = 0.0f32;
    let mut kk = 0usize;
    let k_dim = x.len();

    while kk + 16 <= k_dim {
        let x0 = x[kk];
        let x1 = x[kk + 1];
        let x2 = x[kk + 2];
        let x3 = x[kk + 3];
        let x4 = x[kk + 4];
        let x5 = x[kk + 5];
        let x6 = x[kk + 6];
        let x7 = x[kk + 7];
        let x8 = x[kk + 8];
        let x9 = x[kk + 9];
        let x10 = x[kk + 10];
        let x11 = x[kk + 11];
        let x12 = x[kk + 12];
        let x13 = x[kk + 13];
        let x14 = x[kk + 14];
        let x15 = x[kk + 15];

        a0 += row0[kk].to_f32() * x0;
        a1 += row1[kk].to_f32() * x0;
        a2 += row2[kk].to_f32() * x0;
        b0 += row0[kk + 1].to_f32() * x1;
        b1 += row1[kk + 1].to_f32() * x1;
        b2 += row2[kk + 1].to_f32() * x1;
        c0 += row0[kk + 2].to_f32() * x2;
        c1 += row1[kk + 2].to_f32() * x2;
        c2 += row2[kk + 2].to_f32() * x2;
        d0 += row0[kk + 3].to_f32() * x3;
        d1 += row1[kk + 3].to_f32() * x3;
        d2 += row2[kk + 3].to_f32() * x3;
        e0 += row0[kk + 4].to_f32() * x4;
        e1 += row1[kk + 4].to_f32() * x4;
        e2 += row2[kk + 4].to_f32() * x4;
        f0 += row0[kk + 5].to_f32() * x5;
        f1 += row1[kk + 5].to_f32() * x5;
        f2 += row2[kk + 5].to_f32() * x5;
        g0 += row0[kk + 6].to_f32() * x6;
        g1 += row1[kk + 6].to_f32() * x6;
        g2 += row2[kk + 6].to_f32() * x6;
        h0 += row0[kk + 7].to_f32() * x7;
        h1 += row1[kk + 7].to_f32() * x7;
        h2 += row2[kk + 7].to_f32() * x7;
        a0 += row0[kk + 8].to_f32() * x8;
        a1 += row1[kk + 8].to_f32() * x8;
        a2 += row2[kk + 8].to_f32() * x8;
        b0 += row0[kk + 9].to_f32() * x9;
        b1 += row1[kk + 9].to_f32() * x9;
        b2 += row2[kk + 9].to_f32() * x9;
        c0 += row0[kk + 10].to_f32() * x10;
        c1 += row1[kk + 10].to_f32() * x10;
        c2 += row2[kk + 10].to_f32() * x10;
        d0 += row0[kk + 11].to_f32() * x11;
        d1 += row1[kk + 11].to_f32() * x11;
        d2 += row2[kk + 11].to_f32() * x11;
        e0 += row0[kk + 12].to_f32() * x12;
        e1 += row1[kk + 12].to_f32() * x12;
        e2 += row2[kk + 12].to_f32() * x12;
        f0 += row0[kk + 13].to_f32() * x13;
        f1 += row1[kk + 13].to_f32() * x13;
        f2 += row2[kk + 13].to_f32() * x13;
        g0 += row0[kk + 14].to_f32() * x14;
        g1 += row1[kk + 14].to_f32() * x14;
        g2 += row2[kk + 14].to_f32() * x14;
        h0 += row0[kk + 15].to_f32() * x15;
        h1 += row1[kk + 15].to_f32() * x15;
        h2 += row2[kk + 15].to_f32() * x15;
        kk += 16;
    }

    while kk + 8 <= k_dim {
        let x0 = x[kk];
        let x1 = x[kk + 1];
        let x2 = x[kk + 2];
        let x3 = x[kk + 3];
        let x4 = x[kk + 4];
        let x5 = x[kk + 5];
        let x6 = x[kk + 6];
        let x7 = x[kk + 7];

        a0 += row0[kk].to_f32() * x0 + row0[kk + 4].to_f32() * x4;
        a1 += row1[kk].to_f32() * x0 + row1[kk + 4].to_f32() * x4;
        a2 += row2[kk].to_f32() * x0 + row2[kk + 4].to_f32() * x4;
        b0 += row0[kk + 1].to_f32() * x1 + row0[kk + 5].to_f32() * x5;
        b1 += row1[kk + 1].to_f32() * x1 + row1[kk + 5].to_f32() * x5;
        b2 += row2[kk + 1].to_f32() * x1 + row2[kk + 5].to_f32() * x5;
        c0 += row0[kk + 2].to_f32() * x2 + row0[kk + 6].to_f32() * x6;
        c1 += row1[kk + 2].to_f32() * x2 + row1[kk + 6].to_f32() * x6;
        c2 += row2[kk + 2].to_f32() * x2 + row2[kk + 6].to_f32() * x6;
        d0 += row0[kk + 3].to_f32() * x3 + row0[kk + 7].to_f32() * x7;
        d1 += row1[kk + 3].to_f32() * x3 + row1[kk + 7].to_f32() * x7;
        d2 += row2[kk + 3].to_f32() * x3 + row2[kk + 7].to_f32() * x7;
        kk += 8;
    }

    while kk + 4 <= k_dim {
        let x0 = x[kk];
        let x1 = x[kk + 1];
        let x2 = x[kk + 2];
        let x3 = x[kk + 3];
        a0 += row0[kk].to_f32() * x0;
        a1 += row1[kk].to_f32() * x0;
        a2 += row2[kk].to_f32() * x0;
        b0 += row0[kk + 1].to_f32() * x1;
        b1 += row1[kk + 1].to_f32() * x1;
        b2 += row2[kk + 1].to_f32() * x1;
        c0 += row0[kk + 2].to_f32() * x2;
        c1 += row1[kk + 2].to_f32() * x2;
        c2 += row2[kk + 2].to_f32() * x2;
        d0 += row0[kk + 3].to_f32() * x3;
        d1 += row1[kk + 3].to_f32() * x3;
        d2 += row2[kk + 3].to_f32() * x3;
        kk += 4;
    }

    let mut sum0 = a0 + b0 + c0 + d0 + e0 + f0 + g0 + h0;
    let mut sum1 = a1 + b1 + c1 + d1 + e1 + f1 + g1 + h1;
    let mut sum2 = a2 + b2 + c2 + d2 + e2 + f2 + g2 + h2;
    while kk < k_dim {
        let xv = x[kk];
        sum0 += row0[kk].to_f32() * xv;
        sum1 += row1[kk].to_f32() * xv;
        sum2 += row2[kk].to_f32() * xv;
        kk += 1;
    }
    (sum0, sum1, sum2)
}

#[inline]
pub(crate) fn dot3_unrolled_f32_f16(
    x: &[f32],
    row0: &[f16],
    row1: &[f16],
    row2: &[f16],
) -> (f32, f32, f32) {
    if let Some(sum) = dot3_f32_f16_arch(x, row0, row1, row2) {
        return sum;
    }

    let mut a0 = 0.0f32;
    let mut a1 = 0.0f32;
    let mut a2 = 0.0f32;
    let mut b0 = 0.0f32;
    let mut b1 = 0.0f32;
    let mut b2 = 0.0f32;
    let mut c0 = 0.0f32;
    let mut c1 = 0.0f32;
    let mut c2 = 0.0f32;
    let mut d0 = 0.0f32;
    let mut d1 = 0.0f32;
    let mut d2 = 0.0f32;
    let mut kk = 0usize;
    let k_dim = x.len();

    while kk + 8 <= k_dim {
        let x0 = x[kk];
        let x1 = x[kk + 1];
        let x2 = x[kk + 2];
        let x3 = x[kk + 3];
        let x4 = x[kk + 4];
        let x5 = x[kk + 5];
        let x6 = x[kk + 6];
        let x7 = x[kk + 7];

        a0 += row0[kk].to_f32() * x0 + row0[kk + 4].to_f32() * x4;
        a1 += row1[kk].to_f32() * x0 + row1[kk + 4].to_f32() * x4;
        a2 += row2[kk].to_f32() * x0 + row2[kk + 4].to_f32() * x4;
        b0 += row0[kk + 1].to_f32() * x1 + row0[kk + 5].to_f32() * x5;
        b1 += row1[kk + 1].to_f32() * x1 + row1[kk + 5].to_f32() * x5;
        b2 += row2[kk + 1].to_f32() * x1 + row2[kk + 5].to_f32() * x5;
        c0 += row0[kk + 2].to_f32() * x2 + row0[kk + 6].to_f32() * x6;
        c1 += row1[kk + 2].to_f32() * x2 + row1[kk + 6].to_f32() * x6;
        c2 += row2[kk + 2].to_f32() * x2 + row2[kk + 6].to_f32() * x6;
        d0 += row0[kk + 3].to_f32() * x3 + row0[kk + 7].to_f32() * x7;
        d1 += row1[kk + 3].to_f32() * x3 + row1[kk + 7].to_f32() * x7;
        d2 += row2[kk + 3].to_f32() * x3 + row2[kk + 7].to_f32() * x7;
        kk += 8;
    }

    while kk + 4 <= k_dim {
        let x0 = x[kk];
        let x1 = x[kk + 1];
        let x2 = x[kk + 2];
        let x3 = x[kk + 3];
        a0 += row0[kk].to_f32() * x0;
        a1 += row1[kk].to_f32() * x0;
        a2 += row2[kk].to_f32() * x0;
        b0 += row0[kk + 1].to_f32() * x1;
        b1 += row1[kk + 1].to_f32() * x1;
        b2 += row2[kk + 1].to_f32() * x1;
        c0 += row0[kk + 2].to_f32() * x2;
        c1 += row1[kk + 2].to_f32() * x2;
        c2 += row2[kk + 2].to_f32() * x2;
        d0 += row0[kk + 3].to_f32() * x3;
        d1 += row1[kk + 3].to_f32() * x3;
        d2 += row2[kk + 3].to_f32() * x3;
        kk += 4;
    }

    let mut sum0 = a0 + b0 + c0 + d0;
    let mut sum1 = a1 + b1 + c1 + d1;
    let mut sum2 = a2 + b2 + c2 + d2;
    while kk < k_dim {
        let xv = x[kk];
        sum0 += row0[kk].to_f32() * xv;
        sum1 += row1[kk].to_f32() * xv;
        sum2 += row2[kk].to_f32() * xv;
        kk += 1;
    }
    (sum0, sum1, sum2)
}

#[inline]
fn dot3_unrolled_f32_i8_portable(
    x: &[f32],
    row0: &[i8],
    scale0: f32,
    row1: &[i8],
    scale1: f32,
    row2: &[i8],
    scale2: f32,
) -> (f32, f32, f32) {
    let mut a0 = 0.0f32;
    let mut a1 = 0.0f32;
    let mut a2 = 0.0f32;
    let mut b0 = 0.0f32;
    let mut b1 = 0.0f32;
    let mut b2 = 0.0f32;
    let mut c0 = 0.0f32;
    let mut c1 = 0.0f32;
    let mut c2 = 0.0f32;
    let mut d0 = 0.0f32;
    let mut d1 = 0.0f32;
    let mut d2 = 0.0f32;
    let mut e0 = 0.0f32;
    let mut e1 = 0.0f32;
    let mut e2 = 0.0f32;
    let mut f0 = 0.0f32;
    let mut f1 = 0.0f32;
    let mut f2 = 0.0f32;
    let mut g0 = 0.0f32;
    let mut g1 = 0.0f32;
    let mut g2 = 0.0f32;
    let mut h0 = 0.0f32;
    let mut h1 = 0.0f32;
    let mut h2 = 0.0f32;
    let mut kk = 0usize;
    let k_dim = x.len();

    while kk + 16 <= k_dim {
        let x0 = x[kk];
        let x1 = x[kk + 1];
        let x2 = x[kk + 2];
        let x3 = x[kk + 3];
        let x4 = x[kk + 4];
        let x5 = x[kk + 5];
        let x6 = x[kk + 6];
        let x7 = x[kk + 7];
        let x8 = x[kk + 8];
        let x9 = x[kk + 9];
        let x10 = x[kk + 10];
        let x11 = x[kk + 11];
        let x12 = x[kk + 12];
        let x13 = x[kk + 13];
        let x14 = x[kk + 14];
        let x15 = x[kk + 15];

        a0 += row0[kk] as f32 * x0;
        a1 += row1[kk] as f32 * x0;
        a2 += row2[kk] as f32 * x0;
        b0 += row0[kk + 1] as f32 * x1;
        b1 += row1[kk + 1] as f32 * x1;
        b2 += row2[kk + 1] as f32 * x1;
        c0 += row0[kk + 2] as f32 * x2;
        c1 += row1[kk + 2] as f32 * x2;
        c2 += row2[kk + 2] as f32 * x2;
        d0 += row0[kk + 3] as f32 * x3;
        d1 += row1[kk + 3] as f32 * x3;
        d2 += row2[kk + 3] as f32 * x3;
        e0 += row0[kk + 4] as f32 * x4;
        e1 += row1[kk + 4] as f32 * x4;
        e2 += row2[kk + 4] as f32 * x4;
        f0 += row0[kk + 5] as f32 * x5;
        f1 += row1[kk + 5] as f32 * x5;
        f2 += row2[kk + 5] as f32 * x5;
        g0 += row0[kk + 6] as f32 * x6;
        g1 += row1[kk + 6] as f32 * x6;
        g2 += row2[kk + 6] as f32 * x6;
        h0 += row0[kk + 7] as f32 * x7;
        h1 += row1[kk + 7] as f32 * x7;
        h2 += row2[kk + 7] as f32 * x7;
        a0 += row0[kk + 8] as f32 * x8;
        a1 += row1[kk + 8] as f32 * x8;
        a2 += row2[kk + 8] as f32 * x8;
        b0 += row0[kk + 9] as f32 * x9;
        b1 += row1[kk + 9] as f32 * x9;
        b2 += row2[kk + 9] as f32 * x9;
        c0 += row0[kk + 10] as f32 * x10;
        c1 += row1[kk + 10] as f32 * x10;
        c2 += row2[kk + 10] as f32 * x10;
        d0 += row0[kk + 11] as f32 * x11;
        d1 += row1[kk + 11] as f32 * x11;
        d2 += row2[kk + 11] as f32 * x11;
        e0 += row0[kk + 12] as f32 * x12;
        e1 += row1[kk + 12] as f32 * x12;
        e2 += row2[kk + 12] as f32 * x12;
        f0 += row0[kk + 13] as f32 * x13;
        f1 += row1[kk + 13] as f32 * x13;
        f2 += row2[kk + 13] as f32 * x13;
        g0 += row0[kk + 14] as f32 * x14;
        g1 += row1[kk + 14] as f32 * x14;
        g2 += row2[kk + 14] as f32 * x14;
        h0 += row0[kk + 15] as f32 * x15;
        h1 += row1[kk + 15] as f32 * x15;
        h2 += row2[kk + 15] as f32 * x15;
        kk += 16;
    }

    while kk + 8 <= k_dim {
        let x0 = x[kk];
        let x1 = x[kk + 1];
        let x2 = x[kk + 2];
        let x3 = x[kk + 3];
        let x4 = x[kk + 4];
        let x5 = x[kk + 5];
        let x6 = x[kk + 6];
        let x7 = x[kk + 7];

        a0 += row0[kk] as f32 * x0 + row0[kk + 4] as f32 * x4;
        a1 += row1[kk] as f32 * x0 + row1[kk + 4] as f32 * x4;
        a2 += row2[kk] as f32 * x0 + row2[kk + 4] as f32 * x4;
        b0 += row0[kk + 1] as f32 * x1 + row0[kk + 5] as f32 * x5;
        b1 += row1[kk + 1] as f32 * x1 + row1[kk + 5] as f32 * x5;
        b2 += row2[kk + 1] as f32 * x1 + row2[kk + 5] as f32 * x5;
        c0 += row0[kk + 2] as f32 * x2 + row0[kk + 6] as f32 * x6;
        c1 += row1[kk + 2] as f32 * x2 + row1[kk + 6] as f32 * x6;
        c2 += row2[kk + 2] as f32 * x2 + row2[kk + 6] as f32 * x6;
        d0 += row0[kk + 3] as f32 * x3 + row0[kk + 7] as f32 * x7;
        d1 += row1[kk + 3] as f32 * x3 + row1[kk + 7] as f32 * x7;
        d2 += row2[kk + 3] as f32 * x3 + row2[kk + 7] as f32 * x7;
        kk += 8;
    }

    while kk + 4 <= k_dim {
        let x0 = x[kk];
        let x1 = x[kk + 1];
        let x2 = x[kk + 2];
        let x3 = x[kk + 3];
        a0 += row0[kk] as f32 * x0;
        a1 += row1[kk] as f32 * x0;
        a2 += row2[kk] as f32 * x0;
        b0 += row0[kk + 1] as f32 * x1;
        b1 += row1[kk + 1] as f32 * x1;
        b2 += row2[kk + 1] as f32 * x1;
        c0 += row0[kk + 2] as f32 * x2;
        c1 += row1[kk + 2] as f32 * x2;
        c2 += row2[kk + 2] as f32 * x2;
        d0 += row0[kk + 3] as f32 * x3;
        d1 += row1[kk + 3] as f32 * x3;
        d2 += row2[kk + 3] as f32 * x3;
        kk += 4;
    }

    let mut sum0 = a0 + b0 + c0 + d0 + e0 + f0 + g0 + h0;
    let mut sum1 = a1 + b1 + c1 + d1 + e1 + f1 + g1 + h1;
    let mut sum2 = a2 + b2 + c2 + d2 + e2 + f2 + g2 + h2;
    while kk < k_dim {
        let xv = x[kk];
        sum0 += row0[kk] as f32 * xv;
        sum1 += row1[kk] as f32 * xv;
        sum2 += row2[kk] as f32 * xv;
        kk += 1;
    }
    (sum0 * scale0, sum1 * scale1, sum2 * scale2)
}

#[inline]
pub(crate) fn dot3_unrolled_f32_i8(
    x: &[f32],
    row0: &[i8],
    scale0: f32,
    row1: &[i8],
    scale1: f32,
    row2: &[i8],
    scale2: f32,
) -> (f32, f32, f32) {
    if let Some(sum) = dot3_f32_i8_arch(x, row0, scale0, row1, scale1, row2, scale2) {
        sum
    } else {
        dot3_unrolled_f32_i8_portable(x, row0, scale0, row1, scale1, row2, scale2)
    }
}

#[inline]
fn dot3_rows_unrolled_f32_i8(
    x: &[f32],
    q_block: &[i8],
    q_scale: f32,
    k_block: &[i8],
    k_scale: f32,
    v_block: &[i8],
    v_scale: f32,
    k_dim: usize,
    rows: usize,
    q_out: &mut [f32],
    k_out: &mut [f32],
    v_out: &mut [f32],
) {
    debug_assert!(rows <= 4);
    debug_assert_eq!(q_out.len(), rows);
    debug_assert_eq!(k_out.len(), rows);
    debug_assert_eq!(v_out.len(), rows);

    let mut q_acc = [0.0f32; 4];
    let mut k_acc = [0.0f32; 4];
    let mut v_acc = [0.0f32; 4];
    let mut kk = 0usize;

    while kk + 16 <= k_dim {
        let x0 = x[kk];
        let x1 = x[kk + 1];
        let x2 = x[kk + 2];
        let x3 = x[kk + 3];
        let x4 = x[kk + 4];
        let x5 = x[kk + 5];
        let x6 = x[kk + 6];
        let x7 = x[kk + 7];
        let x8 = x[kk + 8];
        let x9 = x[kk + 9];
        let x10 = x[kk + 10];
        let x11 = x[kk + 11];
        let x12 = x[kk + 12];
        let x13 = x[kk + 13];
        let x14 = x[kk + 14];
        let x15 = x[kk + 15];
        for r in 0..rows {
            let base = r * k_dim + kk;
            q_acc[r] += q_block[base] as f32 * x0
                + q_block[base + 1] as f32 * x1
                + q_block[base + 2] as f32 * x2
                + q_block[base + 3] as f32 * x3
                + q_block[base + 4] as f32 * x4
                + q_block[base + 5] as f32 * x5
                + q_block[base + 6] as f32 * x6
                + q_block[base + 7] as f32 * x7
                + q_block[base + 8] as f32 * x8
                + q_block[base + 9] as f32 * x9
                + q_block[base + 10] as f32 * x10
                + q_block[base + 11] as f32 * x11
                + q_block[base + 12] as f32 * x12
                + q_block[base + 13] as f32 * x13
                + q_block[base + 14] as f32 * x14
                + q_block[base + 15] as f32 * x15;
            k_acc[r] += k_block[base] as f32 * x0
                + k_block[base + 1] as f32 * x1
                + k_block[base + 2] as f32 * x2
                + k_block[base + 3] as f32 * x3
                + k_block[base + 4] as f32 * x4
                + k_block[base + 5] as f32 * x5
                + k_block[base + 6] as f32 * x6
                + k_block[base + 7] as f32 * x7
                + k_block[base + 8] as f32 * x8
                + k_block[base + 9] as f32 * x9
                + k_block[base + 10] as f32 * x10
                + k_block[base + 11] as f32 * x11
                + k_block[base + 12] as f32 * x12
                + k_block[base + 13] as f32 * x13
                + k_block[base + 14] as f32 * x14
                + k_block[base + 15] as f32 * x15;
            v_acc[r] += v_block[base] as f32 * x0
                + v_block[base + 1] as f32 * x1
                + v_block[base + 2] as f32 * x2
                + v_block[base + 3] as f32 * x3
                + v_block[base + 4] as f32 * x4
                + v_block[base + 5] as f32 * x5
                + v_block[base + 6] as f32 * x6
                + v_block[base + 7] as f32 * x7
                + v_block[base + 8] as f32 * x8
                + v_block[base + 9] as f32 * x9
                + v_block[base + 10] as f32 * x10
                + v_block[base + 11] as f32 * x11
                + v_block[base + 12] as f32 * x12
                + v_block[base + 13] as f32 * x13
                + v_block[base + 14] as f32 * x14
                + v_block[base + 15] as f32 * x15;
        }
        kk += 16;
    }

    while kk + 8 <= k_dim {
        let x0 = x[kk];
        let x1 = x[kk + 1];
        let x2 = x[kk + 2];
        let x3 = x[kk + 3];
        let x4 = x[kk + 4];
        let x5 = x[kk + 5];
        let x6 = x[kk + 6];
        let x7 = x[kk + 7];
        for r in 0..rows {
            let base = r * k_dim + kk;
            q_acc[r] += q_block[base] as f32 * x0
                + q_block[base + 1] as f32 * x1
                + q_block[base + 2] as f32 * x2
                + q_block[base + 3] as f32 * x3
                + q_block[base + 4] as f32 * x4
                + q_block[base + 5] as f32 * x5
                + q_block[base + 6] as f32 * x6
                + q_block[base + 7] as f32 * x7;
            k_acc[r] += k_block[base] as f32 * x0
                + k_block[base + 1] as f32 * x1
                + k_block[base + 2] as f32 * x2
                + k_block[base + 3] as f32 * x3
                + k_block[base + 4] as f32 * x4
                + k_block[base + 5] as f32 * x5
                + k_block[base + 6] as f32 * x6
                + k_block[base + 7] as f32 * x7;
            v_acc[r] += v_block[base] as f32 * x0
                + v_block[base + 1] as f32 * x1
                + v_block[base + 2] as f32 * x2
                + v_block[base + 3] as f32 * x3
                + v_block[base + 4] as f32 * x4
                + v_block[base + 5] as f32 * x5
                + v_block[base + 6] as f32 * x6
                + v_block[base + 7] as f32 * x7;
        }
        kk += 8;
    }

    while kk < k_dim {
        let xv = x[kk];
        for r in 0..rows {
            let base = r * k_dim + kk;
            q_acc[r] += q_block[base] as f32 * xv;
            k_acc[r] += k_block[base] as f32 * xv;
            v_acc[r] += v_block[base] as f32 * xv;
        }
        kk += 1;
    }

    for r in 0..rows {
        q_out[r] = q_acc[r] * q_scale;
        k_out[r] = k_acc[r] * k_scale;
        v_out[r] = v_acc[r] * v_scale;
    }
}

#[inline]
fn row_slice(rowmajor: SliceRef<'_>, row_idx: usize, k_dim: usize) -> SliceRef<'_> {
    let start = row_idx * k_dim;
    let end = start + k_dim;
    match rowmajor {
        SliceRef::F32(w) => SliceRef::F32(&w[start..end]),
        SliceRef::F16(w) => SliceRef::F16(&w[start..end]),
        SliceRef::BF16(w) => SliceRef::BF16(&w[start..end]),
        SliceRef::I8(w, scale) => SliceRef::I8(&w[start..end], scale),
    }
}

#[inline]
fn dot_unrolled_from_slice(x: &[f32], row: SliceRef<'_>) -> f32 {
    match row {
        SliceRef::F32(row) => dot_unrolled(x, row),
        SliceRef::F16(row) => dot_unrolled_f32_f16(x, row),
        SliceRef::BF16(row) => dot_unrolled_f32_bf16(x, row),
        SliceRef::I8(row, scale) => dot_unrolled_f32_i8(x, row, scale),
    }
}

#[inline]
pub fn dual_matvec_rowmajor_parallel(
    x: &[f32],
    w0_rowmajor: &[f32],
    w1_rowmajor: &[f32],
    n_rows: usize,
    k_dim: usize,
    out0: &mut [f32],
    out1: &mut [f32],
) {
    assert_eq!(x.len(), k_dim, "x len / k_dim mismatch");
    assert_eq!(w0_rowmajor.len(), n_rows * k_dim, "weight0 size mismatch");
    assert_eq!(w1_rowmajor.len(), n_rows * k_dim, "weight1 size mismatch");
    assert_eq!(out0.len(), n_rows, "out0 size mismatch");
    assert_eq!(out1.len(), n_rows, "out1 size mismatch");

    if n_rows < MATVEC_PAR_THRESHOLD {
        for i in 0..n_rows {
            let row0 = &w0_rowmajor[i * k_dim..(i + 1) * k_dim];
            let row1 = &w1_rowmajor[i * k_dim..(i + 1) * k_dim];
            let (s0, s1) = dot2_unrolled(x, row0, row1);
            out0[i] = s0;
            out1[i] = s1;
        }
    } else {
        out0.par_iter_mut()
            .zip(out1.par_iter_mut())
            .enumerate()
            .for_each(|(i, (dst0, dst1))| {
                let row0 = &w0_rowmajor[i * k_dim..(i + 1) * k_dim];
                let row1 = &w1_rowmajor[i * k_dim..(i + 1) * k_dim];
                let (s0, s1) = dot2_unrolled(x, row0, row1);
                *dst0 = s0;
                *dst1 = s1;
            });
    }
}

#[inline]
pub(crate) fn dual_matvec_rowmajor_parallel_f32_bf16(
    x: &[f32],
    w0_rowmajor: &[bf16],
    w1_rowmajor: &[bf16],
    n_rows: usize,
    k_dim: usize,
    out0: &mut [f32],
    out1: &mut [f32],
) {
    assert_eq!(x.len(), k_dim, "x len / k_dim mismatch");
    assert_eq!(w0_rowmajor.len(), n_rows * k_dim, "weight0 size mismatch");
    assert_eq!(w1_rowmajor.len(), n_rows * k_dim, "weight1 size mismatch");
    assert_eq!(out0.len(), n_rows, "out0 size mismatch");
    assert_eq!(out1.len(), n_rows, "out1 size mismatch");

    if n_rows < MATVEC_PAR_THRESHOLD {
        for i in 0..n_rows {
            let row0 = &w0_rowmajor[i * k_dim..(i + 1) * k_dim];
            let row1 = &w1_rowmajor[i * k_dim..(i + 1) * k_dim];
            let (s0, s1) = dot2_unrolled_f32_bf16(x, row0, row1);
            out0[i] = s0;
            out1[i] = s1;
        }
    } else if should_use_mixed_dual_block_kernel(n_rows) {
        dual_matvec_rowmajor_block_parallel_f32_bf16(
            x,
            w0_rowmajor,
            w1_rowmajor,
            n_rows,
            k_dim,
            out0,
            out1,
        );
    } else {
        out0.par_chunks_mut(MIXED_ROW_PAR_CHUNK_ROWS)
            .zip(out1.par_chunks_mut(MIXED_ROW_PAR_CHUNK_ROWS))
            .enumerate()
            .for_each(|(chunk_idx, (out0_chunk, out1_chunk))| {
                let row_start = chunk_idx * MIXED_ROW_PAR_CHUNK_ROWS;
                for (offset, (dst0, dst1)) in
                    out0_chunk.iter_mut().zip(out1_chunk.iter_mut()).enumerate()
                {
                    let row_idx = row_start + offset;
                    let row0 = &w0_rowmajor[row_idx * k_dim..(row_idx + 1) * k_dim];
                    let row1 = &w1_rowmajor[row_idx * k_dim..(row_idx + 1) * k_dim];
                    let (s0, s1) = dot2_unrolled_f32_bf16(x, row0, row1);
                    *dst0 = s0;
                    *dst1 = s1;
                }
            });
    }
}

#[inline]
pub(crate) fn dual_matvec_rowmajor_parallel_f32_f16(
    x: &[f32],
    w0_rowmajor: &[f16],
    w1_rowmajor: &[f16],
    n_rows: usize,
    k_dim: usize,
    out0: &mut [f32],
    out1: &mut [f32],
) {
    assert_eq!(x.len(), k_dim, "x len / k_dim mismatch");
    assert_eq!(w0_rowmajor.len(), n_rows * k_dim, "weight0 size mismatch");
    assert_eq!(w1_rowmajor.len(), n_rows * k_dim, "weight1 size mismatch");
    assert_eq!(out0.len(), n_rows, "out0 size mismatch");
    assert_eq!(out1.len(), n_rows, "out1 size mismatch");

    if n_rows < MATVEC_PAR_THRESHOLD {
        for i in 0..n_rows {
            let row0 = &w0_rowmajor[i * k_dim..(i + 1) * k_dim];
            let row1 = &w1_rowmajor[i * k_dim..(i + 1) * k_dim];
            let (s0, s1) = dot2_unrolled_f32_f16(x, row0, row1);
            out0[i] = s0;
            out1[i] = s1;
        }
    } else {
        out0.par_chunks_mut(MIXED_ROW_PAR_CHUNK_ROWS)
            .zip(out1.par_chunks_mut(MIXED_ROW_PAR_CHUNK_ROWS))
            .enumerate()
            .for_each(|(chunk_idx, (out0_chunk, out1_chunk))| {
                let row_start = chunk_idx * MIXED_ROW_PAR_CHUNK_ROWS;
                for (offset, (dst0, dst1)) in
                    out0_chunk.iter_mut().zip(out1_chunk.iter_mut()).enumerate()
                {
                    let row_idx = row_start + offset;
                    let row0 = &w0_rowmajor[row_idx * k_dim..(row_idx + 1) * k_dim];
                    let row1 = &w1_rowmajor[row_idx * k_dim..(row_idx + 1) * k_dim];
                    let (s0, s1) = dot2_unrolled_f32_f16(x, row0, row1);
                    *dst0 = s0;
                    *dst1 = s1;
                }
            });
    }
}

#[inline]
pub(crate) fn dual_matvec_rowmajor_parallel_f32_i8(
    x: &[f32],
    w0_rowmajor: &[i8],
    w0_scale: f32,
    w1_rowmajor: &[i8],
    w1_scale: f32,
    n_rows: usize,
    k_dim: usize,
    out0: &mut [f32],
    out1: &mut [f32],
) {
    assert_eq!(x.len(), k_dim, "x len / k_dim mismatch");
    assert_eq!(w0_rowmajor.len(), n_rows * k_dim, "weight0 size mismatch");
    assert_eq!(w1_rowmajor.len(), n_rows * k_dim, "weight1 size mismatch");
    assert_eq!(out0.len(), n_rows, "out0 size mismatch");
    assert_eq!(out1.len(), n_rows, "out1 size mismatch");

    if n_rows < MATVEC_PAR_THRESHOLD {
        for i in 0..n_rows {
            let row0 = &w0_rowmajor[i * k_dim..(i + 1) * k_dim];
            let row1 = &w1_rowmajor[i * k_dim..(i + 1) * k_dim];
            let (s0, s1) = dot2_unrolled_f32_i8(x, row0, w0_scale, row1, w1_scale);
            out0[i] = s0;
            out1[i] = s1;
        }
    } else if should_use_i8_block_kernel(n_rows, k_dim) {
        dual_matvec_rowmajor_block_parallel_f32_i8(
            x,
            w0_rowmajor,
            w0_scale,
            w1_rowmajor,
            w1_scale,
            n_rows,
            k_dim,
            out0,
            out1,
        );
    } else {
        out0.par_chunks_mut(MATVEC_I8_PAR_CHUNK_ROWS)
            .zip(out1.par_chunks_mut(MATVEC_I8_PAR_CHUNK_ROWS))
            .enumerate()
            .for_each(|(chunk_idx, (out0_chunk, out1_chunk))| {
                let row_start = chunk_idx * MATVEC_I8_PAR_CHUNK_ROWS;
                for (offset, (dst0, dst1)) in
                    out0_chunk.iter_mut().zip(out1_chunk.iter_mut()).enumerate()
                {
                    let row_idx = row_start + offset;
                    let row0 = &w0_rowmajor[row_idx * k_dim..(row_idx + 1) * k_dim];
                    let row1 = &w1_rowmajor[row_idx * k_dim..(row_idx + 1) * k_dim];
                    let (s0, s1) = dot2_unrolled_f32_i8(x, row0, w0_scale, row1, w1_scale);
                    *dst0 = s0;
                    *dst1 = s1;
                }
            });
    }
}

#[inline]
fn dual_matvec_rowmajor_block_parallel_f32_bf16(
    x: &[f32],
    w0_rowmajor: &[bf16],
    w1_rowmajor: &[bf16],
    _n_rows: usize,
    k_dim: usize,
    out0: &mut [f32],
    out1: &mut [f32],
) {
    out0.par_chunks_mut(MATVEC_BLOCK_ROWS)
        .zip(out1.par_chunks_mut(MATVEC_BLOCK_ROWS))
        .enumerate()
        .for_each(|(block_idx, (out0_chunk, out1_chunk))| {
            let row_start = block_idx * MATVEC_BLOCK_ROWS;
            let rows = out0_chunk.len();
            let w0_block = &w0_rowmajor[row_start * k_dim..(row_start + rows) * k_dim];
            let w1_block = &w1_rowmajor[row_start * k_dim..(row_start + rows) * k_dim];
            let mut acc0 = [0.0f32; MATVEC_BLOCK_ROWS];
            let mut acc1 = [0.0f32; MATVEC_BLOCK_ROWS];

            let mut kk = 0usize;
            while kk + 8 <= k_dim {
                let x0 = x[kk];
                let x1 = x[kk + 1];
                let x2 = x[kk + 2];
                let x3 = x[kk + 3];
                let x4 = x[kk + 4];
                let x5 = x[kk + 5];
                let x6 = x[kk + 6];
                let x7 = x[kk + 7];
                for r in 0..rows {
                    let base = r * k_dim + kk;
                    acc0[r] += w0_block[base].to_f32() * x0
                        + w0_block[base + 1].to_f32() * x1
                        + w0_block[base + 2].to_f32() * x2
                        + w0_block[base + 3].to_f32() * x3
                        + w0_block[base + 4].to_f32() * x4
                        + w0_block[base + 5].to_f32() * x5
                        + w0_block[base + 6].to_f32() * x6
                        + w0_block[base + 7].to_f32() * x7;
                    acc1[r] += w1_block[base].to_f32() * x0
                        + w1_block[base + 1].to_f32() * x1
                        + w1_block[base + 2].to_f32() * x2
                        + w1_block[base + 3].to_f32() * x3
                        + w1_block[base + 4].to_f32() * x4
                        + w1_block[base + 5].to_f32() * x5
                        + w1_block[base + 6].to_f32() * x6
                        + w1_block[base + 7].to_f32() * x7;
                }
                kk += 8;
            }

            while kk < k_dim {
                let xv = x[kk];
                for r in 0..rows {
                    let base = r * k_dim + kk;
                    acc0[r] += w0_block[base].to_f32() * xv;
                    acc1[r] += w1_block[base].to_f32() * xv;
                }
                kk += 1;
            }

            out0_chunk.copy_from_slice(&acc0[..rows]);
            out1_chunk.copy_from_slice(&acc1[..rows]);
        });
}

fn dual_matvec_rowmajor_block_parallel_f32_i8(
    x: &[f32],
    w0_rowmajor: &[i8],
    w0_scale: f32,
    w1_rowmajor: &[i8],
    w1_scale: f32,
    _n_rows: usize,
    k_dim: usize,
    out0: &mut [f32],
    out1: &mut [f32],
) {
    out0.par_chunks_mut(MATVEC_BLOCK_ROWS)
        .zip(out1.par_chunks_mut(MATVEC_BLOCK_ROWS))
        .enumerate()
        .for_each(|(block_idx, (out0_chunk, out1_chunk))| {
            let row_start = block_idx * MATVEC_BLOCK_ROWS;
            let rows = out0_chunk.len();
            let w0_block = &w0_rowmajor[row_start * k_dim..(row_start + rows) * k_dim];
            let w1_block = &w1_rowmajor[row_start * k_dim..(row_start + rows) * k_dim];
            let mut acc0 = [0.0f32; MATVEC_BLOCK_ROWS];
            let mut acc1 = [0.0f32; MATVEC_BLOCK_ROWS];

            let mut kk = 0usize;
            while kk + 8 <= k_dim {
                let x0 = x[kk];
                let x1 = x[kk + 1];
                let x2 = x[kk + 2];
                let x3 = x[kk + 3];
                let x4 = x[kk + 4];
                let x5 = x[kk + 5];
                let x6 = x[kk + 6];
                let x7 = x[kk + 7];
                for r in 0..rows {
                    let base = r * k_dim + kk;
                    acc0[r] += w0_block[base] as f32 * x0
                        + w0_block[base + 1] as f32 * x1
                        + w0_block[base + 2] as f32 * x2
                        + w0_block[base + 3] as f32 * x3
                        + w0_block[base + 4] as f32 * x4
                        + w0_block[base + 5] as f32 * x5
                        + w0_block[base + 6] as f32 * x6
                        + w0_block[base + 7] as f32 * x7;
                    acc1[r] += w1_block[base] as f32 * x0
                        + w1_block[base + 1] as f32 * x1
                        + w1_block[base + 2] as f32 * x2
                        + w1_block[base + 3] as f32 * x3
                        + w1_block[base + 4] as f32 * x4
                        + w1_block[base + 5] as f32 * x5
                        + w1_block[base + 6] as f32 * x6
                        + w1_block[base + 7] as f32 * x7;
                }
                kk += 8;
            }

            while kk < k_dim {
                let xv = x[kk];
                for r in 0..rows {
                    let base = r * k_dim + kk;
                    acc0[r] += w0_block[base] as f32 * xv;
                    acc1[r] += w1_block[base] as f32 * xv;
                }
                kk += 1;
            }

            for r in 0..rows {
                out0_chunk[r] = acc0[r] * w0_scale;
                out1_chunk[r] = acc1[r] * w1_scale;
            }
        });
}

#[inline]
fn dual_matvec_rowmajor_parallel_bf16_f32(
    x: &[bf16],
    w0_rowmajor: &[f32],
    w1_rowmajor: &[f32],
    n_rows: usize,
    k_dim: usize,
    out0: &mut [f32],
    out1: &mut [f32],
) {
    with_bf16_input_as_f32(x, |x_f32| {
        dual_matvec_rowmajor_parallel(x_f32, w0_rowmajor, w1_rowmajor, n_rows, k_dim, out0, out1);
    });
}

#[inline]
fn dual_matvec_rowmajor_parallel_bf16_bf16(
    x: &[bf16],
    w0_rowmajor: &[bf16],
    w1_rowmajor: &[bf16],
    n_rows: usize,
    k_dim: usize,
    out0: &mut [f32],
    out1: &mut [f32],
) {
    with_bf16_input_as_f32(x, |x_f32| {
        dual_matvec_rowmajor_parallel_f32_bf16(
            x_f32,
            w0_rowmajor,
            w1_rowmajor,
            n_rows,
            k_dim,
            out0,
            out1,
        );
    });
}

#[inline]
fn dual_matvec_rowmajor_parallel_bf16_i8(
    x: &[bf16],
    w0_rowmajor: &[i8],
    w0_scale: f32,
    w1_rowmajor: &[i8],
    w1_scale: f32,
    n_rows: usize,
    k_dim: usize,
    out0: &mut [f32],
    out1: &mut [f32],
) {
    with_bf16_input_as_f32(x, |x_f32| {
        dual_matvec_rowmajor_parallel_f32_i8(
            x_f32,
            w0_rowmajor,
            w0_scale,
            w1_rowmajor,
            w1_scale,
            n_rows,
            k_dim,
            out0,
            out1,
        );
    });
}

#[inline]
fn dual_matvec_rowmajor_parallel_i8_f32(
    x: &[i8],
    x_scale: f32,
    w0_rowmajor: &[f32],
    w1_rowmajor: &[f32],
    n_rows: usize,
    k_dim: usize,
    out0: &mut [f32],
    out1: &mut [f32],
) {
    with_i8_input_as_f32(x, x_scale, |x_f32| {
        dual_matvec_rowmajor_parallel(x_f32, w0_rowmajor, w1_rowmajor, n_rows, k_dim, out0, out1);
    });
}

#[inline]
fn dual_matvec_rowmajor_parallel_i8_bf16(
    x: &[i8],
    x_scale: f32,
    w0_rowmajor: &[bf16],
    w1_rowmajor: &[bf16],
    n_rows: usize,
    k_dim: usize,
    out0: &mut [f32],
    out1: &mut [f32],
) {
    with_i8_input_as_f32(x, x_scale, |x_f32| {
        dual_matvec_rowmajor_parallel_f32_bf16(
            x_f32,
            w0_rowmajor,
            w1_rowmajor,
            n_rows,
            k_dim,
            out0,
            out1,
        );
    });
}

#[inline]
fn dual_matvec_rowmajor_parallel_i8_i8(
    x: &[i8],
    x_scale: f32,
    w0_rowmajor: &[i8],
    w0_scale: f32,
    w1_rowmajor: &[i8],
    w1_scale: f32,
    n_rows: usize,
    k_dim: usize,
    out0: &mut [f32],
    out1: &mut [f32],
) {
    assert_eq!(x.len(), k_dim, "x len / k_dim mismatch");
    assert_eq!(w0_rowmajor.len(), n_rows * k_dim, "weight0 size mismatch");
    assert_eq!(w1_rowmajor.len(), n_rows * k_dim, "weight1 size mismatch");
    assert_eq!(out0.len(), n_rows, "out0 size mismatch");
    assert_eq!(out1.len(), n_rows, "out1 size mismatch");

    if n_rows < MATVEC_PAR_THRESHOLD {
        for i in 0..n_rows {
            let row0 = &w0_rowmajor[i * k_dim..(i + 1) * k_dim];
            let row1 = &w1_rowmajor[i * k_dim..(i + 1) * k_dim];
            let (s0, s1) = dot2_unrolled_i8_i8(x, x_scale, row0, w0_scale, row1, w1_scale);
            out0[i] = s0;
            out1[i] = s1;
        }
    } else {
        out0.par_chunks_mut(MATVEC_I8_PAR_CHUNK_ROWS)
            .zip(out1.par_chunks_mut(MATVEC_I8_PAR_CHUNK_ROWS))
            .enumerate()
            .for_each(|(chunk_idx, (out0_chunk, out1_chunk))| {
                let row_start = chunk_idx * MATVEC_I8_PAR_CHUNK_ROWS;
                for offset in 0..out0_chunk.len() {
                    let row_idx = row_start + offset;
                    let row0 = &w0_rowmajor[row_idx * k_dim..(row_idx + 1) * k_dim];
                    let row1 = &w1_rowmajor[row_idx * k_dim..(row_idx + 1) * k_dim];
                    let (s0, s1) = dot2_unrolled_i8_i8(x, x_scale, row0, w0_scale, row1, w1_scale);
                    out0_chunk[offset] = s0;
                    out1_chunk[offset] = s1;
                }
            });
    }
}

fn dual_matvec_rowmajor_parallel_mixed_f32_input(
    x: &[f32],
    w0_rowmajor: SliceRef<'_>,
    w1_rowmajor: SliceRef<'_>,
    n_rows: usize,
    k_dim: usize,
    out0: &mut [f32],
    out1: &mut [f32],
) {
    if n_rows < MATVEC_PAR_THRESHOLD {
        for i in 0..n_rows {
            out0[i] = dot_unrolled_from_slice(x, row_slice(w0_rowmajor, i, k_dim));
            out1[i] = dot_unrolled_from_slice(x, row_slice(w1_rowmajor, i, k_dim));
        }
    } else {
        out0.par_iter_mut()
            .zip(out1.par_iter_mut())
            .enumerate()
            .for_each(|(i, (out0_val, out1_val))| {
                *out0_val = dot_unrolled_from_slice(x, row_slice(w0_rowmajor, i, k_dim));
                *out1_val = dot_unrolled_from_slice(x, row_slice(w1_rowmajor, i, k_dim));
            });
    }
}

#[inline]
pub fn dual_matvec_rowmajor_parallel_mixed(
    x: SliceRef<'_>,
    w0_rowmajor: SliceRef<'_>,
    w1_rowmajor: SliceRef<'_>,
    n_rows: usize,
    k_dim: usize,
    out0: &mut [f32],
    out1: &mut [f32],
) {
    match (x, w0_rowmajor, w1_rowmajor) {
        (SliceRef::F32(x), SliceRef::F32(w0), SliceRef::F32(w1)) => {
            dual_matvec_rowmajor_parallel(x, w0, w1, n_rows, k_dim, out0, out1)
        }
        (SliceRef::F32(x), SliceRef::F16(w0), SliceRef::F16(w1)) => {
            dual_matvec_rowmajor_parallel_f32_f16(x, w0, w1, n_rows, k_dim, out0, out1)
        }
        (SliceRef::F32(x), SliceRef::BF16(w0), SliceRef::BF16(w1)) => {
            dual_matvec_rowmajor_parallel_f32_bf16(x, w0, w1, n_rows, k_dim, out0, out1)
        }
        (SliceRef::F32(x), SliceRef::I8(w0, s0), SliceRef::I8(w1, s1)) => {
            dual_matvec_rowmajor_parallel_f32_i8(x, w0, s0, w1, s1, n_rows, k_dim, out0, out1)
        }
        (SliceRef::F16(x), SliceRef::F32(w0), SliceRef::F32(w1)) => {
            with_f16_input_as_f32(x, |x_f32| {
                dual_matvec_rowmajor_parallel(x_f32, w0, w1, n_rows, k_dim, out0, out1)
            })
        }
        (SliceRef::F16(x), SliceRef::F16(w0), SliceRef::F16(w1)) => {
            with_f16_input_as_f32(x, |x_f32| {
                dual_matvec_rowmajor_parallel_f32_f16(x_f32, w0, w1, n_rows, k_dim, out0, out1)
            })
        }
        (SliceRef::F16(x), SliceRef::BF16(w0), SliceRef::BF16(w1)) => {
            with_f16_input_as_f32(x, |x_f32| {
                dual_matvec_rowmajor_parallel_f32_bf16(x_f32, w0, w1, n_rows, k_dim, out0, out1)
            })
        }
        (SliceRef::F16(x), SliceRef::I8(w0, s0), SliceRef::I8(w1, s1)) => {
            with_f16_input_as_f32(x, |x_f32| {
                dual_matvec_rowmajor_parallel_f32_i8(
                    x_f32, w0, s0, w1, s1, n_rows, k_dim, out0, out1,
                )
            })
        }
        (SliceRef::BF16(x), SliceRef::F32(w0), SliceRef::F32(w1)) => {
            dual_matvec_rowmajor_parallel_bf16_f32(x, w0, w1, n_rows, k_dim, out0, out1)
        }
        (SliceRef::BF16(x), SliceRef::BF16(w0), SliceRef::BF16(w1)) => {
            dual_matvec_rowmajor_parallel_bf16_bf16(x, w0, w1, n_rows, k_dim, out0, out1)
        }
        (SliceRef::BF16(x), SliceRef::F16(w0), SliceRef::F16(w1)) => {
            with_bf16_input_as_f32(x, |x_f32| {
                dual_matvec_rowmajor_parallel_f32_f16(x_f32, w0, w1, n_rows, k_dim, out0, out1)
            })
        }
        (SliceRef::BF16(x), SliceRef::I8(w0, s0), SliceRef::I8(w1, s1)) => {
            dual_matvec_rowmajor_parallel_bf16_i8(x, w0, s0, w1, s1, n_rows, k_dim, out0, out1)
        }
        (SliceRef::I8(x, sx), SliceRef::F32(w0), SliceRef::F32(w1)) => {
            dual_matvec_rowmajor_parallel_i8_f32(x, sx, w0, w1, n_rows, k_dim, out0, out1)
        }
        (SliceRef::I8(x, sx), SliceRef::F16(w0), SliceRef::F16(w1)) => {
            with_i8_input_as_f32(x, sx, |x_f32| {
                dual_matvec_rowmajor_parallel_f32_f16(x_f32, w0, w1, n_rows, k_dim, out0, out1)
            })
        }
        (SliceRef::I8(x, sx), SliceRef::BF16(w0), SliceRef::BF16(w1)) => {
            dual_matvec_rowmajor_parallel_i8_bf16(x, sx, w0, w1, n_rows, k_dim, out0, out1)
        }
        (SliceRef::I8(x, sx), SliceRef::I8(w0, s0), SliceRef::I8(w1, s1)) => {
            dual_matvec_rowmajor_parallel_i8_i8(x, sx, w0, s0, w1, s1, n_rows, k_dim, out0, out1)
        }
        (SliceRef::F32(x), w0, w1) => {
            dual_matvec_rowmajor_parallel_mixed_f32_input(x, w0, w1, n_rows, k_dim, out0, out1)
        }
        (SliceRef::F16(x), w0, w1) => {
            with_f16_input_as_f32(x, |x_f32| {
                dual_matvec_rowmajor_parallel_mixed_f32_input(
                    x_f32, w0, w1, n_rows, k_dim, out0, out1,
                )
            });
        }
        (SliceRef::BF16(x), w0, w1) => {
            with_bf16_input_as_f32(x, |x_f32| {
                dual_matvec_rowmajor_parallel_mixed_f32_input(
                    x_f32, w0, w1, n_rows, k_dim, out0, out1,
                )
            });
        }
        (SliceRef::I8(x, scale), w0, w1) => {
            with_i8_input_as_f32(x, scale, |x_f32| {
                dual_matvec_rowmajor_parallel_mixed_f32_input(
                    x_f32, w0, w1, n_rows, k_dim, out0, out1,
                )
            });
        }
    }
}

#[inline]
pub(crate) fn qkv_matvec_rowmajor_parallel(
    x: &[f32],
    q_rowmajor: &[f32],
    k_rowmajor: &[f32],
    v_rowmajor: &[f32],
    q_rows: usize,
    kv_rows: usize,
    k_dim: usize,
    q_out: &mut [f32],
    k_out: &mut [f32],
    v_out: &mut [f32],
) {
    assert_eq!(x.len(), k_dim, "x len / k_dim mismatch");
    assert_eq!(q_rowmajor.len(), q_rows * k_dim, "Q weight size mismatch");
    assert_eq!(k_rowmajor.len(), kv_rows * k_dim, "K weight size mismatch");
    assert_eq!(v_rowmajor.len(), kv_rows * k_dim, "V weight size mismatch");
    assert_eq!(q_out.len(), q_rows, "Q output size mismatch");
    assert_eq!(k_out.len(), kv_rows, "K output size mismatch");
    assert_eq!(v_out.len(), kv_rows, "V output size mismatch");

    let shared_rows = q_rows.min(kv_rows);
    if shared_rows < MATVEC_PAR_THRESHOLD {
        for i in 0..shared_rows {
            let q_row = &q_rowmajor[i * k_dim..(i + 1) * k_dim];
            let k_row = &k_rowmajor[i * k_dim..(i + 1) * k_dim];
            let v_row = &v_rowmajor[i * k_dim..(i + 1) * k_dim];
            let (q, k, v) = dot3_unrolled(x, q_row, k_row, v_row);
            q_out[i] = q;
            k_out[i] = k;
            v_out[i] = v;
        }
    } else {
        q_out[..shared_rows]
            .par_iter_mut()
            .zip(k_out[..shared_rows].par_iter_mut())
            .zip(v_out[..shared_rows].par_iter_mut())
            .enumerate()
            .for_each(|(i, ((q_dst, k_dst), v_dst))| {
                let q_row = &q_rowmajor[i * k_dim..(i + 1) * k_dim];
                let k_row = &k_rowmajor[i * k_dim..(i + 1) * k_dim];
                let v_row = &v_rowmajor[i * k_dim..(i + 1) * k_dim];
                let (q, k, v) = dot3_unrolled(x, q_row, k_row, v_row);
                *q_dst = q;
                *k_dst = k;
                *v_dst = v;
            });
    }

    if q_rows > shared_rows {
        matvec_rowmajor_parallel(
            x,
            &q_rowmajor[shared_rows * k_dim..],
            q_rows - shared_rows,
            k_dim,
            &mut q_out[shared_rows..],
        );
    }
    if kv_rows > shared_rows {
        dual_matvec_rowmajor_parallel(
            x,
            &k_rowmajor[shared_rows * k_dim..],
            &v_rowmajor[shared_rows * k_dim..],
            kv_rows - shared_rows,
            k_dim,
            &mut k_out[shared_rows..],
            &mut v_out[shared_rows..],
        );
    }
}

#[inline]
pub(crate) fn qkv_matvec_rowmajor_parallel_f32_bf16(
    x: &[f32],
    q_rowmajor: &[bf16],
    k_rowmajor: &[bf16],
    v_rowmajor: &[bf16],
    q_rows: usize,
    kv_rows: usize,
    k_dim: usize,
    q_out: &mut [f32],
    k_out: &mut [f32],
    v_out: &mut [f32],
) {
    assert_eq!(x.len(), k_dim, "x len / k_dim mismatch");
    assert_eq!(q_rowmajor.len(), q_rows * k_dim, "Q weight size mismatch");
    assert_eq!(k_rowmajor.len(), kv_rows * k_dim, "K weight size mismatch");
    assert_eq!(v_rowmajor.len(), kv_rows * k_dim, "V weight size mismatch");
    assert_eq!(q_out.len(), q_rows, "Q output size mismatch");
    assert_eq!(k_out.len(), kv_rows, "K output size mismatch");
    assert_eq!(v_out.len(), kv_rows, "V output size mismatch");

    let shared_rows = q_rows.min(kv_rows);
    if shared_rows < MATVEC_PAR_THRESHOLD {
        for i in 0..shared_rows {
            let q_row = &q_rowmajor[i * k_dim..(i + 1) * k_dim];
            let k_row = &k_rowmajor[i * k_dim..(i + 1) * k_dim];
            let v_row = &v_rowmajor[i * k_dim..(i + 1) * k_dim];
            let (q, k, v) = dot3_unrolled_f32_bf16(x, q_row, k_row, v_row);
            q_out[i] = q;
            k_out[i] = k;
            v_out[i] = v;
        }
    } else {
        q_out[..shared_rows]
            .par_iter_mut()
            .zip(k_out[..shared_rows].par_iter_mut())
            .zip(v_out[..shared_rows].par_iter_mut())
            .enumerate()
            .for_each(|(i, ((q_dst, k_dst), v_dst))| {
                let q_row = &q_rowmajor[i * k_dim..(i + 1) * k_dim];
                let k_row = &k_rowmajor[i * k_dim..(i + 1) * k_dim];
                let v_row = &v_rowmajor[i * k_dim..(i + 1) * k_dim];
                let (q, k, v) = dot3_unrolled_f32_bf16(x, q_row, k_row, v_row);
                *q_dst = q;
                *k_dst = k;
                *v_dst = v;
            });
    }

    if q_rows > shared_rows {
        matvec_rowmajor_parallel_f32_bf16(
            x,
            &q_rowmajor[shared_rows * k_dim..],
            q_rows - shared_rows,
            k_dim,
            &mut q_out[shared_rows..],
        );
    }
    if kv_rows > shared_rows {
        dual_matvec_rowmajor_parallel_f32_bf16(
            x,
            &k_rowmajor[shared_rows * k_dim..],
            &v_rowmajor[shared_rows * k_dim..],
            kv_rows - shared_rows,
            k_dim,
            &mut k_out[shared_rows..],
            &mut v_out[shared_rows..],
        );
    }
}

#[inline]
pub(crate) fn qkv_matvec_rowmajor_parallel_f32_f16(
    x: &[f32],
    q_rowmajor: &[f16],
    k_rowmajor: &[f16],
    v_rowmajor: &[f16],
    q_rows: usize,
    kv_rows: usize,
    k_dim: usize,
    q_out: &mut [f32],
    k_out: &mut [f32],
    v_out: &mut [f32],
) {
    assert_eq!(x.len(), k_dim, "x len / k_dim mismatch");
    assert_eq!(q_rowmajor.len(), q_rows * k_dim, "Q weight size mismatch");
    assert_eq!(k_rowmajor.len(), kv_rows * k_dim, "K weight size mismatch");
    assert_eq!(v_rowmajor.len(), kv_rows * k_dim, "V weight size mismatch");
    assert_eq!(q_out.len(), q_rows, "Q output size mismatch");
    assert_eq!(k_out.len(), kv_rows, "K output size mismatch");
    assert_eq!(v_out.len(), kv_rows, "V output size mismatch");

    let shared_rows = q_rows.min(kv_rows);
    if shared_rows < MATVEC_PAR_THRESHOLD {
        for i in 0..shared_rows {
            let q_row = &q_rowmajor[i * k_dim..(i + 1) * k_dim];
            let k_row = &k_rowmajor[i * k_dim..(i + 1) * k_dim];
            let v_row = &v_rowmajor[i * k_dim..(i + 1) * k_dim];
            let (q, k, v) = dot3_unrolled_f32_f16(x, q_row, k_row, v_row);
            q_out[i] = q;
            k_out[i] = k;
            v_out[i] = v;
        }
    } else {
        q_out[..shared_rows]
            .par_iter_mut()
            .zip(k_out[..shared_rows].par_iter_mut())
            .zip(v_out[..shared_rows].par_iter_mut())
            .enumerate()
            .for_each(|(i, ((q_dst, k_dst), v_dst))| {
                let q_row = &q_rowmajor[i * k_dim..(i + 1) * k_dim];
                let k_row = &k_rowmajor[i * k_dim..(i + 1) * k_dim];
                let v_row = &v_rowmajor[i * k_dim..(i + 1) * k_dim];
                let (q, k, v) = dot3_unrolled_f32_f16(x, q_row, k_row, v_row);
                *q_dst = q;
                *k_dst = k;
                *v_dst = v;
            });
    }

    if q_rows > shared_rows {
        matvec_rowmajor_parallel_f32_f16(
            x,
            &q_rowmajor[shared_rows * k_dim..],
            q_rows - shared_rows,
            k_dim,
            &mut q_out[shared_rows..],
        );
    }
    if kv_rows > shared_rows {
        dual_matvec_rowmajor_parallel_f32_f16(
            x,
            &k_rowmajor[shared_rows * k_dim..],
            &v_rowmajor[shared_rows * k_dim..],
            kv_rows - shared_rows,
            k_dim,
            &mut k_out[shared_rows..],
            &mut v_out[shared_rows..],
        );
    }
}

#[inline]
pub(crate) fn qkv_matvec_rowmajor_parallel_f32_i8(
    x: &[f32],
    q_rowmajor: &[i8],
    q_scale: f32,
    k_rowmajor: &[i8],
    k_scale: f32,
    v_rowmajor: &[i8],
    v_scale: f32,
    q_rows: usize,
    kv_rows: usize,
    k_dim: usize,
    q_out: &mut [f32],
    k_out: &mut [f32],
    v_out: &mut [f32],
) {
    assert_eq!(x.len(), k_dim, "x len / k_dim mismatch");
    assert_eq!(q_rowmajor.len(), q_rows * k_dim, "Q weight size mismatch");
    assert_eq!(k_rowmajor.len(), kv_rows * k_dim, "K weight size mismatch");
    assert_eq!(v_rowmajor.len(), kv_rows * k_dim, "V weight size mismatch");
    assert_eq!(q_out.len(), q_rows, "Q output size mismatch");
    assert_eq!(k_out.len(), kv_rows, "K output size mismatch");
    assert_eq!(v_out.len(), kv_rows, "V output size mismatch");

    let shared_rows = q_rows.min(kv_rows);
    if shared_rows < MATVEC_PAR_THRESHOLD {
        for i in 0..shared_rows {
            let q_row = &q_rowmajor[i * k_dim..(i + 1) * k_dim];
            let k_row = &k_rowmajor[i * k_dim..(i + 1) * k_dim];
            let v_row = &v_rowmajor[i * k_dim..(i + 1) * k_dim];
            let (q, k, v) = dot3_unrolled_f32_i8(x, q_row, q_scale, k_row, k_scale, v_row, v_scale);
            q_out[i] = q;
            k_out[i] = k;
            v_out[i] = v;
        }
    } else if should_use_i8_block_kernel(shared_rows, k_dim) {
        qkv_matvec_rowmajor_block_parallel_f32_i8(
            x,
            q_rowmajor,
            q_scale,
            k_rowmajor,
            k_scale,
            v_rowmajor,
            v_scale,
            shared_rows,
            k_dim,
            &mut q_out[..shared_rows],
            &mut k_out[..shared_rows],
            &mut v_out[..shared_rows],
        );
    } else {
        q_out[..shared_rows]
            .par_chunks_mut(QKV_I8_PAR_CHUNK_ROWS)
            .zip(k_out[..shared_rows].par_chunks_mut(QKV_I8_PAR_CHUNK_ROWS))
            .zip(v_out[..shared_rows].par_chunks_mut(QKV_I8_PAR_CHUNK_ROWS))
            .enumerate()
            .for_each(|(chunk_idx, ((q_chunk, k_chunk), v_chunk))| {
                let row_start = chunk_idx * QKV_I8_PAR_CHUNK_ROWS;
                let mut offset = 0usize;
                if should_use_i8_qkv_row4_kernel() {
                    while offset + 4 <= q_chunk.len() {
                        let row_idx = row_start + offset;
                        dot3_rows_unrolled_f32_i8(
                            x,
                            &q_rowmajor[row_idx * k_dim..(row_idx + 4) * k_dim],
                            q_scale,
                            &k_rowmajor[row_idx * k_dim..(row_idx + 4) * k_dim],
                            k_scale,
                            &v_rowmajor[row_idx * k_dim..(row_idx + 4) * k_dim],
                            v_scale,
                            k_dim,
                            4,
                            &mut q_chunk[offset..offset + 4],
                            &mut k_chunk[offset..offset + 4],
                            &mut v_chunk[offset..offset + 4],
                        );
                        offset += 4;
                    }
                }
                while offset < q_chunk.len() {
                    let row_idx = row_start + offset;
                    let q_row = &q_rowmajor[row_idx * k_dim..(row_idx + 1) * k_dim];
                    let k_row = &k_rowmajor[row_idx * k_dim..(row_idx + 1) * k_dim];
                    let v_row = &v_rowmajor[row_idx * k_dim..(row_idx + 1) * k_dim];
                    let (q, k, v) =
                        dot3_unrolled_f32_i8(x, q_row, q_scale, k_row, k_scale, v_row, v_scale);
                    q_chunk[offset] = q;
                    k_chunk[offset] = k;
                    v_chunk[offset] = v;
                    offset += 1;
                }
            });
    }

    if q_rows > shared_rows {
        matvec_rowmajor_parallel_f32_i8(
            x,
            &q_rowmajor[shared_rows * k_dim..],
            q_scale,
            q_rows - shared_rows,
            k_dim,
            &mut q_out[shared_rows..],
        );
    }
    if kv_rows > shared_rows {
        dual_matvec_rowmajor_parallel_f32_i8(
            x,
            &k_rowmajor[shared_rows * k_dim..],
            k_scale,
            &v_rowmajor[shared_rows * k_dim..],
            v_scale,
            kv_rows - shared_rows,
            k_dim,
            &mut k_out[shared_rows..],
            &mut v_out[shared_rows..],
        );
    }
}

#[inline]
pub(crate) fn qkv_matvec_rowmajor_parallel_i8_i8(
    x: &[i8],
    x_scale: f32,
    q_rowmajor: &[i8],
    q_scale: f32,
    k_rowmajor: &[i8],
    k_scale: f32,
    v_rowmajor: &[i8],
    v_scale: f32,
    q_rows: usize,
    kv_rows: usize,
    k_dim: usize,
    q_out: &mut [f32],
    k_out: &mut [f32],
    v_out: &mut [f32],
) {
    assert_eq!(x.len(), k_dim, "x len / k_dim mismatch");
    assert_eq!(q_rowmajor.len(), q_rows * k_dim, "Q weight size mismatch");
    assert_eq!(k_rowmajor.len(), kv_rows * k_dim, "K weight size mismatch");
    assert_eq!(v_rowmajor.len(), kv_rows * k_dim, "V weight size mismatch");
    assert_eq!(q_out.len(), q_rows, "Q output size mismatch");
    assert_eq!(k_out.len(), kv_rows, "K output size mismatch");
    assert_eq!(v_out.len(), kv_rows, "V output size mismatch");

    let shared_rows = q_rows.min(kv_rows);
    if shared_rows < MATVEC_PAR_THRESHOLD {
        for i in 0..shared_rows {
            let q_row = &q_rowmajor[i * k_dim..(i + 1) * k_dim];
            let k_row = &k_rowmajor[i * k_dim..(i + 1) * k_dim];
            let v_row = &v_rowmajor[i * k_dim..(i + 1) * k_dim];
            let (q, k, v) =
                dot3_unrolled_i8_i8(x, x_scale, q_row, q_scale, k_row, k_scale, v_row, v_scale);
            q_out[i] = q;
            k_out[i] = k;
            v_out[i] = v;
        }
    } else {
        q_out[..shared_rows]
            .par_chunks_mut(QKV_I8_PAR_CHUNK_ROWS)
            .zip(k_out[..shared_rows].par_chunks_mut(QKV_I8_PAR_CHUNK_ROWS))
            .zip(v_out[..shared_rows].par_chunks_mut(QKV_I8_PAR_CHUNK_ROWS))
            .enumerate()
            .for_each(|(chunk_idx, ((q_chunk, k_chunk), v_chunk))| {
                let row_start = chunk_idx * QKV_I8_PAR_CHUNK_ROWS;
                for offset in 0..q_chunk.len() {
                    let row_idx = row_start + offset;
                    let q_row = &q_rowmajor[row_idx * k_dim..(row_idx + 1) * k_dim];
                    let k_row = &k_rowmajor[row_idx * k_dim..(row_idx + 1) * k_dim];
                    let v_row = &v_rowmajor[row_idx * k_dim..(row_idx + 1) * k_dim];
                    let (q, k, v) = dot3_unrolled_i8_i8(
                        x, x_scale, q_row, q_scale, k_row, k_scale, v_row, v_scale,
                    );
                    q_chunk[offset] = q;
                    k_chunk[offset] = k;
                    v_chunk[offset] = v;
                }
            });
    }

    if q_rows > shared_rows {
        matvec_rowmajor_parallel_i8_i8(
            x,
            x_scale,
            &q_rowmajor[shared_rows * k_dim..],
            q_scale,
            q_rows - shared_rows,
            k_dim,
            &mut q_out[shared_rows..],
        );
    }
    if kv_rows > shared_rows {
        dual_matvec_rowmajor_parallel_i8_i8(
            x,
            x_scale,
            &k_rowmajor[shared_rows * k_dim..],
            k_scale,
            &v_rowmajor[shared_rows * k_dim..],
            v_scale,
            kv_rows - shared_rows,
            k_dim,
            &mut k_out[shared_rows..],
            &mut v_out[shared_rows..],
        );
    }
}

fn qkv_matvec_rowmajor_block_parallel_f32_i8(
    x: &[f32],
    q_rowmajor: &[i8],
    q_scale: f32,
    k_rowmajor: &[i8],
    k_scale: f32,
    v_rowmajor: &[i8],
    v_scale: f32,
    _n_rows: usize,
    k_dim: usize,
    q_out: &mut [f32],
    k_out: &mut [f32],
    v_out: &mut [f32],
) {
    q_out
        .par_chunks_mut(MATVEC_BLOCK_ROWS)
        .zip(k_out.par_chunks_mut(MATVEC_BLOCK_ROWS))
        .zip(v_out.par_chunks_mut(MATVEC_BLOCK_ROWS))
        .enumerate()
        .for_each(|(block_idx, ((q_chunk, k_chunk), v_chunk))| {
            let row_start = block_idx * MATVEC_BLOCK_ROWS;
            let rows = q_chunk.len();
            let q_block = &q_rowmajor[row_start * k_dim..(row_start + rows) * k_dim];
            let k_block = &k_rowmajor[row_start * k_dim..(row_start + rows) * k_dim];
            let v_block = &v_rowmajor[row_start * k_dim..(row_start + rows) * k_dim];
            let mut q_acc = [0.0f32; MATVEC_BLOCK_ROWS];
            let mut k_acc = [0.0f32; MATVEC_BLOCK_ROWS];
            let mut v_acc = [0.0f32; MATVEC_BLOCK_ROWS];

            let mut kk = 0usize;
            while kk + 16 <= k_dim {
                let x0 = x[kk];
                let x1 = x[kk + 1];
                let x2 = x[kk + 2];
                let x3 = x[kk + 3];
                let x4 = x[kk + 4];
                let x5 = x[kk + 5];
                let x6 = x[kk + 6];
                let x7 = x[kk + 7];
                let x8 = x[kk + 8];
                let x9 = x[kk + 9];
                let x10 = x[kk + 10];
                let x11 = x[kk + 11];
                let x12 = x[kk + 12];
                let x13 = x[kk + 13];
                let x14 = x[kk + 14];
                let x15 = x[kk + 15];
                for r in 0..rows {
                    let base = r * k_dim + kk;
                    q_acc[r] += q_block[base] as f32 * x0
                        + q_block[base + 1] as f32 * x1
                        + q_block[base + 2] as f32 * x2
                        + q_block[base + 3] as f32 * x3
                        + q_block[base + 4] as f32 * x4
                        + q_block[base + 5] as f32 * x5
                        + q_block[base + 6] as f32 * x6
                        + q_block[base + 7] as f32 * x7
                        + q_block[base + 8] as f32 * x8
                        + q_block[base + 9] as f32 * x9
                        + q_block[base + 10] as f32 * x10
                        + q_block[base + 11] as f32 * x11
                        + q_block[base + 12] as f32 * x12
                        + q_block[base + 13] as f32 * x13
                        + q_block[base + 14] as f32 * x14
                        + q_block[base + 15] as f32 * x15;
                    k_acc[r] += k_block[base] as f32 * x0
                        + k_block[base + 1] as f32 * x1
                        + k_block[base + 2] as f32 * x2
                        + k_block[base + 3] as f32 * x3
                        + k_block[base + 4] as f32 * x4
                        + k_block[base + 5] as f32 * x5
                        + k_block[base + 6] as f32 * x6
                        + k_block[base + 7] as f32 * x7
                        + k_block[base + 8] as f32 * x8
                        + k_block[base + 9] as f32 * x9
                        + k_block[base + 10] as f32 * x10
                        + k_block[base + 11] as f32 * x11
                        + k_block[base + 12] as f32 * x12
                        + k_block[base + 13] as f32 * x13
                        + k_block[base + 14] as f32 * x14
                        + k_block[base + 15] as f32 * x15;
                    v_acc[r] += v_block[base] as f32 * x0
                        + v_block[base + 1] as f32 * x1
                        + v_block[base + 2] as f32 * x2
                        + v_block[base + 3] as f32 * x3
                        + v_block[base + 4] as f32 * x4
                        + v_block[base + 5] as f32 * x5
                        + v_block[base + 6] as f32 * x6
                        + v_block[base + 7] as f32 * x7
                        + v_block[base + 8] as f32 * x8
                        + v_block[base + 9] as f32 * x9
                        + v_block[base + 10] as f32 * x10
                        + v_block[base + 11] as f32 * x11
                        + v_block[base + 12] as f32 * x12
                        + v_block[base + 13] as f32 * x13
                        + v_block[base + 14] as f32 * x14
                        + v_block[base + 15] as f32 * x15;
                }
                kk += 16;
            }

            while kk + 8 <= k_dim {
                let x0 = x[kk];
                let x1 = x[kk + 1];
                let x2 = x[kk + 2];
                let x3 = x[kk + 3];
                let x4 = x[kk + 4];
                let x5 = x[kk + 5];
                let x6 = x[kk + 6];
                let x7 = x[kk + 7];
                for r in 0..rows {
                    let base = r * k_dim + kk;
                    q_acc[r] += q_block[base] as f32 * x0
                        + q_block[base + 1] as f32 * x1
                        + q_block[base + 2] as f32 * x2
                        + q_block[base + 3] as f32 * x3
                        + q_block[base + 4] as f32 * x4
                        + q_block[base + 5] as f32 * x5
                        + q_block[base + 6] as f32 * x6
                        + q_block[base + 7] as f32 * x7;
                    k_acc[r] += k_block[base] as f32 * x0
                        + k_block[base + 1] as f32 * x1
                        + k_block[base + 2] as f32 * x2
                        + k_block[base + 3] as f32 * x3
                        + k_block[base + 4] as f32 * x4
                        + k_block[base + 5] as f32 * x5
                        + k_block[base + 6] as f32 * x6
                        + k_block[base + 7] as f32 * x7;
                    v_acc[r] += v_block[base] as f32 * x0
                        + v_block[base + 1] as f32 * x1
                        + v_block[base + 2] as f32 * x2
                        + v_block[base + 3] as f32 * x3
                        + v_block[base + 4] as f32 * x4
                        + v_block[base + 5] as f32 * x5
                        + v_block[base + 6] as f32 * x6
                        + v_block[base + 7] as f32 * x7;
                }
                kk += 8;
            }

            while kk < k_dim {
                let xv = x[kk];
                for r in 0..rows {
                    let base = r * k_dim + kk;
                    q_acc[r] += q_block[base] as f32 * xv;
                    k_acc[r] += k_block[base] as f32 * xv;
                    v_acc[r] += v_block[base] as f32 * xv;
                }
                kk += 1;
            }

            for r in 0..rows {
                q_chunk[r] = q_acc[r] * q_scale;
                k_chunk[r] = k_acc[r] * k_scale;
                v_chunk[r] = v_acc[r] * v_scale;
            }
        });
}

#[inline]
pub fn dual_matvec_silu_mul_rowmajor_parallel(
    x: &[f32],
    gate_w_rowmajor: &[f32],
    up_w_rowmajor: &[f32],
    n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
) {
    assert_eq!(x.len(), k_dim, "x len / k_dim mismatch");
    assert_eq!(
        gate_w_rowmajor.len(),
        n_rows * k_dim,
        "gate weight size mismatch"
    );
    assert_eq!(
        up_w_rowmajor.len(),
        n_rows * k_dim,
        "up weight size mismatch"
    );
    assert_eq!(out.len(), n_rows, "out size mismatch");

    if n_rows < MATVEC_PAR_THRESHOLD {
        for i in 0..n_rows {
            let gate_row = &gate_w_rowmajor[i * k_dim..(i + 1) * k_dim];
            let up_row = &up_w_rowmajor[i * k_dim..(i + 1) * k_dim];
            let (g, u) = dot2_unrolled(x, gate_row, up_row);
            let sig = 1.0 / (1.0 + (-g).exp());
            out[i] = (g * sig) * u;
        }
    } else {
        out.par_iter_mut().enumerate().for_each(|(i, out_val)| {
            let gate_row = &gate_w_rowmajor[i * k_dim..(i + 1) * k_dim];
            let up_row = &up_w_rowmajor[i * k_dim..(i + 1) * k_dim];
            let (g, u) = dot2_unrolled(x, gate_row, up_row);
            let sig = 1.0 / (1.0 + (-g).exp());
            *out_val = (g * sig) * u;
        });
    }
}

#[inline]
pub(crate) fn dual_matvec_silu_mul_rowmajor_parallel_f32_bf16(
    x: &[f32],
    gate_w_rowmajor: &[bf16],
    up_w_rowmajor: &[bf16],
    n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
) {
    assert_eq!(x.len(), k_dim, "x len / k_dim mismatch");
    assert_eq!(
        gate_w_rowmajor.len(),
        n_rows * k_dim,
        "gate weight size mismatch"
    );
    assert_eq!(
        up_w_rowmajor.len(),
        n_rows * k_dim,
        "up weight size mismatch"
    );
    assert_eq!(out.len(), n_rows, "out size mismatch");

    if n_rows < MATVEC_PAR_THRESHOLD {
        for i in 0..n_rows {
            let gate_row = &gate_w_rowmajor[i * k_dim..(i + 1) * k_dim];
            let up_row = &up_w_rowmajor[i * k_dim..(i + 1) * k_dim];
            let (g, u) = dot2_unrolled_f32_bf16(x, gate_row, up_row);
            let sig = 1.0 / (1.0 + (-g).exp());
            out[i] = (g * sig) * u;
        }
    } else {
        out.par_chunks_mut(MIXED_ROW_PAR_CHUNK_ROWS)
            .enumerate()
            .for_each(|(chunk_idx, out_chunk)| {
                let row_start = chunk_idx * MIXED_ROW_PAR_CHUNK_ROWS;
                for (offset, out_val) in out_chunk.iter_mut().enumerate() {
                    let row_idx = row_start + offset;
                    let gate_row = &gate_w_rowmajor[row_idx * k_dim..(row_idx + 1) * k_dim];
                    let up_row = &up_w_rowmajor[row_idx * k_dim..(row_idx + 1) * k_dim];
                    let (g, u) = dot2_unrolled_f32_bf16(x, gate_row, up_row);
                    let sig = 1.0 / (1.0 + (-g).exp());
                    *out_val = (g * sig) * u;
                }
            });
    }
}

#[inline]
pub(crate) fn dual_matvec_silu_mul_rowmajor_parallel_f32_f16(
    x: &[f32],
    gate_w_rowmajor: &[f16],
    up_w_rowmajor: &[f16],
    n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
) {
    assert_eq!(x.len(), k_dim, "x len / k_dim mismatch");
    assert_eq!(
        gate_w_rowmajor.len(),
        n_rows * k_dim,
        "gate weight size mismatch"
    );
    assert_eq!(
        up_w_rowmajor.len(),
        n_rows * k_dim,
        "up weight size mismatch"
    );
    assert_eq!(out.len(), n_rows, "out size mismatch");

    if n_rows < MATVEC_PAR_THRESHOLD {
        for i in 0..n_rows {
            let gate_row = &gate_w_rowmajor[i * k_dim..(i + 1) * k_dim];
            let up_row = &up_w_rowmajor[i * k_dim..(i + 1) * k_dim];
            let (g, u) = dot2_unrolled_f32_f16(x, gate_row, up_row);
            let sig = 1.0 / (1.0 + (-g).exp());
            out[i] = (g * sig) * u;
        }
    } else {
        out.par_chunks_mut(MIXED_ROW_PAR_CHUNK_ROWS)
            .enumerate()
            .for_each(|(chunk_idx, out_chunk)| {
                let row_start = chunk_idx * MIXED_ROW_PAR_CHUNK_ROWS;
                for (offset, out_val) in out_chunk.iter_mut().enumerate() {
                    let row_idx = row_start + offset;
                    let gate_row = &gate_w_rowmajor[row_idx * k_dim..(row_idx + 1) * k_dim];
                    let up_row = &up_w_rowmajor[row_idx * k_dim..(row_idx + 1) * k_dim];
                    let (g, u) = dot2_unrolled_f32_f16(x, gate_row, up_row);
                    let sig = 1.0 / (1.0 + (-g).exp());
                    *out_val = (g * sig) * u;
                }
            });
    }
}

#[inline]
pub(crate) fn dual_matvec_silu_mul_rowmajor_parallel_f32_i8(
    x: &[f32],
    gate_w_rowmajor: &[i8],
    gate_scale: f32,
    up_w_rowmajor: &[i8],
    up_scale: f32,
    n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
) {
    assert_eq!(x.len(), k_dim, "x len / k_dim mismatch");
    assert_eq!(
        gate_w_rowmajor.len(),
        n_rows * k_dim,
        "gate weight size mismatch"
    );
    assert_eq!(
        up_w_rowmajor.len(),
        n_rows * k_dim,
        "up weight size mismatch"
    );
    assert_eq!(out.len(), n_rows, "out size mismatch");

    if n_rows < MATVEC_PAR_THRESHOLD {
        for i in 0..n_rows {
            let gate_row = &gate_w_rowmajor[i * k_dim..(i + 1) * k_dim];
            let up_row = &up_w_rowmajor[i * k_dim..(i + 1) * k_dim];
            let (g, u) = dot2_unrolled_f32_i8(x, gate_row, gate_scale, up_row, up_scale);
            let sig = 1.0 / (1.0 + (-g).exp());
            out[i] = (g * sig) * u;
        }
    } else if should_use_i8_silu_block_kernel(n_rows, k_dim) {
        dual_matvec_silu_mul_rowmajor_block_parallel_f32_i8(
            x,
            gate_w_rowmajor,
            gate_scale,
            up_w_rowmajor,
            up_scale,
            n_rows,
            k_dim,
            out,
        );
    } else {
        out.par_chunks_mut(MATVEC_I8_PAR_CHUNK_ROWS)
            .enumerate()
            .for_each(|(chunk_idx, out_chunk)| {
                let row_start = chunk_idx * MATVEC_I8_PAR_CHUNK_ROWS;
                for (offset, out_val) in out_chunk.iter_mut().enumerate() {
                    let row_idx = row_start + offset;
                    let gate_row = &gate_w_rowmajor[row_idx * k_dim..(row_idx + 1) * k_dim];
                    let up_row = &up_w_rowmajor[row_idx * k_dim..(row_idx + 1) * k_dim];
                    let (g, u) = dot2_unrolled_f32_i8(x, gate_row, gate_scale, up_row, up_scale);
                    let sig = 1.0 / (1.0 + (-g).exp());
                    *out_val = (g * sig) * u;
                }
            });
    }
}

#[inline]
pub(crate) fn dual_matvec_silu_mul_rowmajor_parallel_i8_i8(
    x: &[i8],
    x_scale: f32,
    gate_w_rowmajor: &[i8],
    gate_scale: f32,
    up_w_rowmajor: &[i8],
    up_scale: f32,
    n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
) {
    assert_eq!(x.len(), k_dim, "x len / k_dim mismatch");
    assert_eq!(
        gate_w_rowmajor.len(),
        n_rows * k_dim,
        "gate weight size mismatch"
    );
    assert_eq!(
        up_w_rowmajor.len(),
        n_rows * k_dim,
        "up weight size mismatch"
    );
    assert_eq!(out.len(), n_rows, "out size mismatch");

    if n_rows < MATVEC_PAR_THRESHOLD {
        for i in 0..n_rows {
            let gate_row = &gate_w_rowmajor[i * k_dim..(i + 1) * k_dim];
            let up_row = &up_w_rowmajor[i * k_dim..(i + 1) * k_dim];
            let (g, u) = dot2_unrolled_i8_i8(x, x_scale, gate_row, gate_scale, up_row, up_scale);
            let sig = 1.0 / (1.0 + (-g).exp());
            out[i] = (g * sig) * u;
        }
    } else {
        out.par_chunks_mut(MATVEC_I8_PAR_CHUNK_ROWS)
            .enumerate()
            .for_each(|(chunk_idx, out_chunk)| {
                let row_start = chunk_idx * MATVEC_I8_PAR_CHUNK_ROWS;
                for (offset, out_val) in out_chunk.iter_mut().enumerate() {
                    let row_idx = row_start + offset;
                    let gate_row = &gate_w_rowmajor[row_idx * k_dim..(row_idx + 1) * k_dim];
                    let up_row = &up_w_rowmajor[row_idx * k_dim..(row_idx + 1) * k_dim];
                    let (g, u) =
                        dot2_unrolled_i8_i8(x, x_scale, gate_row, gate_scale, up_row, up_scale);
                    let sig = 1.0 / (1.0 + (-g).exp());
                    *out_val = (g * sig) * u;
                }
            });
    }
}

fn dual_matvec_silu_mul_rowmajor_block_parallel_f32_i8(
    x: &[f32],
    gate_w_rowmajor: &[i8],
    gate_scale: f32,
    up_w_rowmajor: &[i8],
    up_scale: f32,
    _n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
) {
    out.par_chunks_mut(SILU_I8_BLOCK_ROWS)
        .enumerate()
        .for_each(|(block_idx, out_chunk)| {
            let row_start = block_idx * SILU_I8_BLOCK_ROWS;
            let rows = out_chunk.len();
            let gate_block = &gate_w_rowmajor[row_start * k_dim..(row_start + rows) * k_dim];
            let up_block = &up_w_rowmajor[row_start * k_dim..(row_start + rows) * k_dim];
            let mut gate_acc = [0.0f32; SILU_I8_BLOCK_ROWS];
            let mut up_acc = [0.0f32; SILU_I8_BLOCK_ROWS];

            let mut kk = 0usize;
            while kk + 16 <= k_dim {
                let x0 = x[kk];
                let x1 = x[kk + 1];
                let x2 = x[kk + 2];
                let x3 = x[kk + 3];
                let x4 = x[kk + 4];
                let x5 = x[kk + 5];
                let x6 = x[kk + 6];
                let x7 = x[kk + 7];
                let x8 = x[kk + 8];
                let x9 = x[kk + 9];
                let x10 = x[kk + 10];
                let x11 = x[kk + 11];
                let x12 = x[kk + 12];
                let x13 = x[kk + 13];
                let x14 = x[kk + 14];
                let x15 = x[kk + 15];
                for r in 0..rows {
                    let base = r * k_dim + kk;
                    gate_acc[r] += gate_block[base] as f32 * x0
                        + gate_block[base + 1] as f32 * x1
                        + gate_block[base + 2] as f32 * x2
                        + gate_block[base + 3] as f32 * x3
                        + gate_block[base + 4] as f32 * x4
                        + gate_block[base + 5] as f32 * x5
                        + gate_block[base + 6] as f32 * x6
                        + gate_block[base + 7] as f32 * x7
                        + gate_block[base + 8] as f32 * x8
                        + gate_block[base + 9] as f32 * x9
                        + gate_block[base + 10] as f32 * x10
                        + gate_block[base + 11] as f32 * x11
                        + gate_block[base + 12] as f32 * x12
                        + gate_block[base + 13] as f32 * x13
                        + gate_block[base + 14] as f32 * x14
                        + gate_block[base + 15] as f32 * x15;
                    up_acc[r] += up_block[base] as f32 * x0
                        + up_block[base + 1] as f32 * x1
                        + up_block[base + 2] as f32 * x2
                        + up_block[base + 3] as f32 * x3
                        + up_block[base + 4] as f32 * x4
                        + up_block[base + 5] as f32 * x5
                        + up_block[base + 6] as f32 * x6
                        + up_block[base + 7] as f32 * x7
                        + up_block[base + 8] as f32 * x8
                        + up_block[base + 9] as f32 * x9
                        + up_block[base + 10] as f32 * x10
                        + up_block[base + 11] as f32 * x11
                        + up_block[base + 12] as f32 * x12
                        + up_block[base + 13] as f32 * x13
                        + up_block[base + 14] as f32 * x14
                        + up_block[base + 15] as f32 * x15;
                }
                kk += 16;
            }

            while kk + 8 <= k_dim {
                let x0 = x[kk];
                let x1 = x[kk + 1];
                let x2 = x[kk + 2];
                let x3 = x[kk + 3];
                let x4 = x[kk + 4];
                let x5 = x[kk + 5];
                let x6 = x[kk + 6];
                let x7 = x[kk + 7];
                for r in 0..rows {
                    let base = r * k_dim + kk;
                    gate_acc[r] += gate_block[base] as f32 * x0
                        + gate_block[base + 1] as f32 * x1
                        + gate_block[base + 2] as f32 * x2
                        + gate_block[base + 3] as f32 * x3
                        + gate_block[base + 4] as f32 * x4
                        + gate_block[base + 5] as f32 * x5
                        + gate_block[base + 6] as f32 * x6
                        + gate_block[base + 7] as f32 * x7;
                    up_acc[r] += up_block[base] as f32 * x0
                        + up_block[base + 1] as f32 * x1
                        + up_block[base + 2] as f32 * x2
                        + up_block[base + 3] as f32 * x3
                        + up_block[base + 4] as f32 * x4
                        + up_block[base + 5] as f32 * x5
                        + up_block[base + 6] as f32 * x6
                        + up_block[base + 7] as f32 * x7;
                }
                kk += 8;
            }

            while kk < k_dim {
                let xv = x[kk];
                for r in 0..rows {
                    let base = r * k_dim + kk;
                    gate_acc[r] += gate_block[base] as f32 * xv;
                    up_acc[r] += up_block[base] as f32 * xv;
                }
                kk += 1;
            }

            for r in 0..rows {
                let g = gate_acc[r] * gate_scale;
                let sig = 1.0 / (1.0 + (-g).exp());
                out_chunk[r] = (g * sig) * (up_acc[r] * up_scale);
            }
        });
}

fn dual_matvec_silu_mul_rowmajor_parallel_bf16_f32(
    x: &[bf16],
    gate_w_rowmajor: &[f32],
    up_w_rowmajor: &[f32],
    n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
) {
    with_bf16_input_as_f32(x, |x_f32| {
        dual_matvec_silu_mul_rowmajor_parallel(
            x_f32,
            gate_w_rowmajor,
            up_w_rowmajor,
            n_rows,
            k_dim,
            out,
        );
    });
}

#[inline]
fn dual_matvec_silu_mul_rowmajor_parallel_bf16_bf16(
    x: &[bf16],
    gate_w_rowmajor: &[bf16],
    up_w_rowmajor: &[bf16],
    n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
) {
    with_bf16_input_as_f32(x, |x_f32| {
        dual_matvec_silu_mul_rowmajor_parallel_f32_bf16(
            x_f32,
            gate_w_rowmajor,
            up_w_rowmajor,
            n_rows,
            k_dim,
            out,
        );
    });
}

#[inline]
fn dual_matvec_silu_mul_rowmajor_parallel_bf16_i8(
    x: &[bf16],
    gate_w_rowmajor: &[i8],
    gate_scale: f32,
    up_w_rowmajor: &[i8],
    up_scale: f32,
    n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
) {
    with_bf16_input_as_f32(x, |x_f32| {
        dual_matvec_silu_mul_rowmajor_parallel_f32_i8(
            x_f32,
            gate_w_rowmajor,
            gate_scale,
            up_w_rowmajor,
            up_scale,
            n_rows,
            k_dim,
            out,
        );
    });
}

#[inline]
fn dual_matvec_silu_mul_rowmajor_parallel_i8_f32(
    x: &[i8],
    x_scale: f32,
    gate_w_rowmajor: &[f32],
    up_w_rowmajor: &[f32],
    n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
) {
    with_i8_input_as_f32(x, x_scale, |x_f32| {
        dual_matvec_silu_mul_rowmajor_parallel(
            x_f32,
            gate_w_rowmajor,
            up_w_rowmajor,
            n_rows,
            k_dim,
            out,
        );
    });
}

#[inline]
fn dual_matvec_silu_mul_rowmajor_parallel_i8_bf16(
    x: &[i8],
    x_scale: f32,
    gate_w_rowmajor: &[bf16],
    up_w_rowmajor: &[bf16],
    n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
) {
    with_i8_input_as_f32(x, x_scale, |x_f32| {
        dual_matvec_silu_mul_rowmajor_parallel_f32_bf16(
            x_f32,
            gate_w_rowmajor,
            up_w_rowmajor,
            n_rows,
            k_dim,
            out,
        );
    });
}

fn dual_matvec_silu_mul_rowmajor_parallel_mixed_f32_input(
    x: &[f32],
    gate_w_rowmajor: SliceRef<'_>,
    up_w_rowmajor: SliceRef<'_>,
    n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
) {
    if n_rows < MATVEC_PAR_THRESHOLD {
        for i in 0..n_rows {
            let g = dot_unrolled_from_slice(x, row_slice(gate_w_rowmajor, i, k_dim));
            let u = dot_unrolled_from_slice(x, row_slice(up_w_rowmajor, i, k_dim));
            let sig = 1.0 / (1.0 + (-g).exp());
            out[i] = (g * sig) * u;
        }
    } else {
        out.par_iter_mut().enumerate().for_each(|(i, out_val)| {
            let g = dot_unrolled_from_slice(x, row_slice(gate_w_rowmajor, i, k_dim));
            let u = dot_unrolled_from_slice(x, row_slice(up_w_rowmajor, i, k_dim));
            let sig = 1.0 / (1.0 + (-g).exp());
            *out_val = (g * sig) * u;
        });
    }
}

#[inline]
pub fn dual_matvec_silu_mul_rowmajor_parallel_mixed(
    x: SliceRef<'_>,
    gate_w_rowmajor: SliceRef<'_>,
    up_w_rowmajor: SliceRef<'_>,
    n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
) {
    match (x, gate_w_rowmajor, up_w_rowmajor) {
        (SliceRef::F32(x), SliceRef::F32(gate), SliceRef::F32(up)) => {
            dual_matvec_silu_mul_rowmajor_parallel(x, gate, up, n_rows, k_dim, out)
        }
        (SliceRef::F32(x), SliceRef::F16(gate), SliceRef::F16(up)) => {
            dual_matvec_silu_mul_rowmajor_parallel_f32_f16(x, gate, up, n_rows, k_dim, out)
        }
        (SliceRef::F32(x), SliceRef::BF16(gate), SliceRef::BF16(up)) => {
            dual_matvec_silu_mul_rowmajor_parallel_f32_bf16(x, gate, up, n_rows, k_dim, out)
        }
        (SliceRef::F32(x), SliceRef::I8(gate, gs), SliceRef::I8(up, us)) => {
            dual_matvec_silu_mul_rowmajor_parallel_f32_i8(x, gate, gs, up, us, n_rows, k_dim, out)
        }
        (SliceRef::F16(x), SliceRef::F32(gate), SliceRef::F32(up)) => {
            with_f16_input_as_f32(x, |x_f32| {
                dual_matvec_silu_mul_rowmajor_parallel(x_f32, gate, up, n_rows, k_dim, out)
            })
        }
        (SliceRef::F16(x), SliceRef::F16(gate), SliceRef::F16(up)) => {
            with_f16_input_as_f32(x, |x_f32| {
                dual_matvec_silu_mul_rowmajor_parallel_f32_f16(x_f32, gate, up, n_rows, k_dim, out)
            })
        }
        (SliceRef::F16(x), SliceRef::BF16(gate), SliceRef::BF16(up)) => {
            with_f16_input_as_f32(x, |x_f32| {
                dual_matvec_silu_mul_rowmajor_parallel_f32_bf16(x_f32, gate, up, n_rows, k_dim, out)
            })
        }
        (SliceRef::F16(x), SliceRef::I8(gate, gs), SliceRef::I8(up, us)) => {
            with_f16_input_as_f32(x, |x_f32| {
                dual_matvec_silu_mul_rowmajor_parallel_f32_i8(
                    x_f32, gate, gs, up, us, n_rows, k_dim, out,
                )
            })
        }
        (SliceRef::BF16(x), SliceRef::F32(gate), SliceRef::F32(up)) => {
            dual_matvec_silu_mul_rowmajor_parallel_bf16_f32(x, gate, up, n_rows, k_dim, out)
        }
        (SliceRef::BF16(x), SliceRef::BF16(gate), SliceRef::BF16(up)) => {
            dual_matvec_silu_mul_rowmajor_parallel_bf16_bf16(x, gate, up, n_rows, k_dim, out)
        }
        (SliceRef::BF16(x), SliceRef::F16(gate), SliceRef::F16(up)) => {
            with_bf16_input_as_f32(x, |x_f32| {
                dual_matvec_silu_mul_rowmajor_parallel_f32_f16(x_f32, gate, up, n_rows, k_dim, out)
            })
        }
        (SliceRef::BF16(x), SliceRef::I8(gate, gs), SliceRef::I8(up, us)) => {
            dual_matvec_silu_mul_rowmajor_parallel_bf16_i8(x, gate, gs, up, us, n_rows, k_dim, out)
        }
        (SliceRef::I8(x, xs), SliceRef::F32(gate), SliceRef::F32(up)) => {
            dual_matvec_silu_mul_rowmajor_parallel_i8_f32(x, xs, gate, up, n_rows, k_dim, out)
        }
        (SliceRef::I8(x, xs), SliceRef::F16(gate), SliceRef::F16(up)) => {
            with_i8_input_as_f32(x, xs, |x_f32| {
                dual_matvec_silu_mul_rowmajor_parallel_f32_f16(x_f32, gate, up, n_rows, k_dim, out)
            })
        }
        (SliceRef::I8(x, xs), SliceRef::BF16(gate), SliceRef::BF16(up)) => {
            dual_matvec_silu_mul_rowmajor_parallel_i8_bf16(x, xs, gate, up, n_rows, k_dim, out)
        }
        (SliceRef::I8(x, xs), SliceRef::I8(gate, gs), SliceRef::I8(up, us)) => {
            dual_matvec_silu_mul_rowmajor_parallel_i8_i8(
                x, xs, gate, gs, up, us, n_rows, k_dim, out,
            )
        }
        (SliceRef::F32(x), gate, up) => {
            dual_matvec_silu_mul_rowmajor_parallel_mixed_f32_input(x, gate, up, n_rows, k_dim, out)
        }
        (SliceRef::F16(x), gate, up) => {
            with_f16_input_as_f32(x, |x_f32| {
                dual_matvec_silu_mul_rowmajor_parallel_mixed_f32_input(
                    x_f32, gate, up, n_rows, k_dim, out,
                )
            });
        }
        (SliceRef::BF16(x), gate, up) => {
            with_bf16_input_as_f32(x, |x_f32| {
                dual_matvec_silu_mul_rowmajor_parallel_mixed_f32_input(
                    x_f32, gate, up, n_rows, k_dim, out,
                )
            });
        }
        (SliceRef::I8(x, scale), gate, up) => {
            with_i8_input_as_f32(x, scale, |x_f32| {
                dual_matvec_silu_mul_rowmajor_parallel_mixed_f32_input(
                    x_f32, gate, up, n_rows, k_dim, out,
                )
            });
        }
    }
}

#[inline]
pub fn matvec_argmax_rowmajor_parallel(
    x: &[f32],
    w_rowmajor: &[f32],
    n_rows: usize,
    k_dim: usize,
) -> usize {
    assert_eq!(x.len(), k_dim, "x len / k_dim mismatch");
    assert_eq!(w_rowmajor.len(), n_rows * k_dim, "weight size mismatch");

    if should_use_argmax_block_kernel(n_rows) {
        let n_blocks = (n_rows + ARGMAX_BLOCK_ROWS - 1) / ARGMAX_BLOCK_ROWS;
        return (0..n_blocks)
            .into_par_iter()
            .map(|block_idx| {
                let row_start = block_idx * ARGMAX_BLOCK_ROWS;
                let rows = (n_rows - row_start).min(ARGMAX_BLOCK_ROWS);
                let w_block = &w_rowmajor[row_start * k_dim..(row_start + rows) * k_dim];
                let mut acc = [0.0f32; ARGMAX_BLOCK_ROWS];

                let mut kk = 0usize;
                while kk + 8 <= k_dim {
                    let x0 = x[kk];
                    let x1 = x[kk + 1];
                    let x2 = x[kk + 2];
                    let x3 = x[kk + 3];
                    let x4 = x[kk + 4];
                    let x5 = x[kk + 5];
                    let x6 = x[kk + 6];
                    let x7 = x[kk + 7];
                    for r in 0..rows {
                        let base = r * k_dim + kk;
                        acc[r] += w_block[base] * x0
                            + w_block[base + 1] * x1
                            + w_block[base + 2] * x2
                            + w_block[base + 3] * x3
                            + w_block[base + 4] * x4
                            + w_block[base + 5] * x5
                            + w_block[base + 6] * x6
                            + w_block[base + 7] * x7;
                    }
                    kk += 8;
                }

                while kk < k_dim {
                    let xv = x[kk];
                    for r in 0..rows {
                        acc[r] += w_block[r * k_dim + kk] * xv;
                    }
                    kk += 1;
                }

                let mut best = (row_start, f32::NEG_INFINITY);
                for r in 0..rows {
                    let cand = (row_start + r, acc[r]);
                    if cand.1 > best.1 {
                        best = cand;
                    }
                }
                best
            })
            .reduce(
                || (0usize, f32::NEG_INFINITY),
                |a, b| if a.1 >= b.1 { a } else { b },
            )
            .0;
    }

    if n_rows < MATVEC_PAR_THRESHOLD {
        let mut best = (0usize, f32::NEG_INFINITY);
        for i in 0..n_rows {
            let row = &w_rowmajor[i * k_dim..(i + 1) * k_dim];
            let score = dot_unrolled(x, row);
            if score > best.1 {
                best = (i, score);
            }
        }
        best.0
    } else {
        (0..n_rows)
            .into_par_iter()
            .map(|i| {
                let row = &w_rowmajor[i * k_dim..(i + 1) * k_dim];
                (i, dot_unrolled(x, row))
            })
            .reduce(
                || (0usize, f32::NEG_INFINITY),
                |a, b| if a.1 >= b.1 { a } else { b },
            )
            .0
    }
}

#[inline]
fn matvec_argmax_rowmajor_parallel_f32_bf16(
    x: &[f32],
    w_rowmajor: &[bf16],
    n_rows: usize,
    k_dim: usize,
) -> usize {
    assert_eq!(x.len(), k_dim, "x len / k_dim mismatch");
    assert_eq!(w_rowmajor.len(), n_rows * k_dim, "weight size mismatch");

    if n_rows < MATVEC_PAR_THRESHOLD {
        let mut best = (0usize, f32::NEG_INFINITY);
        for i in 0..n_rows {
            let row = &w_rowmajor[i * k_dim..(i + 1) * k_dim];
            let score = dot_unrolled_f32_bf16(x, row);
            if score > best.1 {
                best = (i, score);
            }
        }
        best.0
    } else {
        (0..n_rows)
            .into_par_iter()
            .map(|i| {
                let row = &w_rowmajor[i * k_dim..(i + 1) * k_dim];
                (i, dot_unrolled_f32_bf16(x, row))
            })
            .reduce(
                || (0usize, f32::NEG_INFINITY),
                |a, b| if a.1 >= b.1 { a } else { b },
            )
            .0
    }
}

#[inline]
fn matvec_argmax_rowmajor_parallel_f32_f16(
    x: &[f32],
    w_rowmajor: &[f16],
    n_rows: usize,
    k_dim: usize,
) -> usize {
    assert_eq!(x.len(), k_dim, "x len / k_dim mismatch");
    assert_eq!(w_rowmajor.len(), n_rows * k_dim, "weight size mismatch");

    if n_rows < MATVEC_PAR_THRESHOLD {
        let mut best = (0usize, f32::NEG_INFINITY);
        for i in 0..n_rows {
            let row = &w_rowmajor[i * k_dim..(i + 1) * k_dim];
            let score = dot_unrolled_f32_f16(x, row);
            if score > best.1 {
                best = (i, score);
            }
        }
        best.0
    } else {
        (0..n_rows)
            .into_par_iter()
            .map(|i| {
                let row = &w_rowmajor[i * k_dim..(i + 1) * k_dim];
                (i, dot_unrolled_f32_f16(x, row))
            })
            .reduce(
                || (0usize, f32::NEG_INFINITY),
                |a, b| if a.1 >= b.1 { a } else { b },
            )
            .0
    }
}

#[inline]
fn matvec_argmax_rowmajor_parallel_f32_i8(
    x: &[f32],
    w_rowmajor: &[i8],
    scale: f32,
    n_rows: usize,
    k_dim: usize,
) -> usize {
    assert_eq!(x.len(), k_dim, "x len / k_dim mismatch");
    assert_eq!(w_rowmajor.len(), n_rows * k_dim, "weight size mismatch");

    if n_rows >= MATVEC_BLOCK_THRESHOLD {
        let n_blocks = n_rows.div_ceil(ARGMAX_BLOCK_ROWS);
        return (0..n_blocks)
            .into_par_iter()
            .map(|block_idx| {
                let row_start = block_idx * ARGMAX_BLOCK_ROWS;
                let rows = (n_rows - row_start).min(ARGMAX_BLOCK_ROWS);
                let w_block = &w_rowmajor[row_start * k_dim..(row_start + rows) * k_dim];
                let mut acc = [0.0f32; ARGMAX_BLOCK_ROWS];

                let mut kk = 0usize;
                while kk + 8 <= k_dim {
                    let x0 = x[kk];
                    let x1 = x[kk + 1];
                    let x2 = x[kk + 2];
                    let x3 = x[kk + 3];
                    let x4 = x[kk + 4];
                    let x5 = x[kk + 5];
                    let x6 = x[kk + 6];
                    let x7 = x[kk + 7];
                    for r in 0..rows {
                        let base = r * k_dim + kk;
                        acc[r] += w_block[base] as f32 * x0
                            + w_block[base + 1] as f32 * x1
                            + w_block[base + 2] as f32 * x2
                            + w_block[base + 3] as f32 * x3
                            + w_block[base + 4] as f32 * x4
                            + w_block[base + 5] as f32 * x5
                            + w_block[base + 6] as f32 * x6
                            + w_block[base + 7] as f32 * x7;
                    }
                    kk += 8;
                }

                while kk < k_dim {
                    let xv = x[kk];
                    for r in 0..rows {
                        acc[r] += w_block[r * k_dim + kk] as f32 * xv;
                    }
                    kk += 1;
                }

                let mut best = (row_start, f32::NEG_INFINITY);
                for (r, value) in acc[..rows].iter().enumerate() {
                    let cand = (row_start + r, *value * scale);
                    if cand.1 > best.1 {
                        best = cand;
                    }
                }
                best
            })
            .reduce(
                || (0usize, f32::NEG_INFINITY),
                |a, b| if a.1 >= b.1 { a } else { b },
            )
            .0;
    }

    if n_rows < MATVEC_PAR_THRESHOLD {
        let mut best = (0usize, f32::NEG_INFINITY);
        for i in 0..n_rows {
            let row = &w_rowmajor[i * k_dim..(i + 1) * k_dim];
            let score = dot_unrolled_f32_i8(x, row, scale);
            if score > best.1 {
                best = (i, score);
            }
        }
        best.0
    } else {
        (0..n_rows)
            .into_par_iter()
            .map(|i| {
                let row = &w_rowmajor[i * k_dim..(i + 1) * k_dim];
                (i, dot_unrolled_f32_i8(x, row, scale))
            })
            .reduce(
                || (0usize, f32::NEG_INFINITY),
                |a, b| if a.1 >= b.1 { a } else { b },
            )
            .0
    }
}

#[inline]
fn matvec_argmax_rowmajor_parallel_bf16_f32(
    x: &[bf16],
    w_rowmajor: &[f32],
    n_rows: usize,
    k_dim: usize,
) -> usize {
    with_bf16_input_as_f32(x, |x_f32| {
        matvec_argmax_rowmajor_parallel(x_f32, w_rowmajor, n_rows, k_dim)
    })
}

#[inline]
fn matvec_argmax_rowmajor_parallel_bf16_bf16(
    x: &[bf16],
    w_rowmajor: &[bf16],
    n_rows: usize,
    k_dim: usize,
) -> usize {
    with_bf16_input_as_f32(x, |x_f32| {
        matvec_argmax_rowmajor_parallel_f32_bf16(x_f32, w_rowmajor, n_rows, k_dim)
    })
}

#[inline]
fn matvec_argmax_rowmajor_parallel_bf16_i8(
    x: &[bf16],
    w_rowmajor: &[i8],
    scale: f32,
    n_rows: usize,
    k_dim: usize,
) -> usize {
    with_bf16_input_as_f32(x, |x_f32| {
        matvec_argmax_rowmajor_parallel_f32_i8(x_f32, w_rowmajor, scale, n_rows, k_dim)
    })
}

#[inline]
fn matvec_argmax_rowmajor_parallel_i8_f32(
    x: &[i8],
    x_scale: f32,
    w_rowmajor: &[f32],
    n_rows: usize,
    k_dim: usize,
) -> usize {
    with_i8_input_as_f32(x, x_scale, |x_f32| {
        matvec_argmax_rowmajor_parallel(x_f32, w_rowmajor, n_rows, k_dim)
    })
}

#[inline]
fn matvec_argmax_rowmajor_parallel_i8_bf16(
    x: &[i8],
    x_scale: f32,
    w_rowmajor: &[bf16],
    n_rows: usize,
    k_dim: usize,
) -> usize {
    with_i8_input_as_f32(x, x_scale, |x_f32| {
        matvec_argmax_rowmajor_parallel_f32_bf16(x_f32, w_rowmajor, n_rows, k_dim)
    })
}

#[inline]
fn matvec_argmax_rowmajor_parallel_i8_i8(
    x: &[i8],
    x_scale: f32,
    w_rowmajor: &[i8],
    w_scale: f32,
    n_rows: usize,
    k_dim: usize,
) -> usize {
    assert_eq!(x.len(), k_dim, "x len / k_dim mismatch");
    assert_eq!(w_rowmajor.len(), n_rows * k_dim, "weight size mismatch");

    let choose_best = |best: &mut (usize, f32), row_idx: usize, score: f32| {
        if score > best.1 {
            *best = (row_idx, score);
        }
    };

    if n_rows < MATVEC_PAR_THRESHOLD {
        let mut best = (0usize, f32::NEG_INFINITY);
        for i in 0..n_rows {
            let row = &w_rowmajor[i * k_dim..(i + 1) * k_dim];
            let score = dot_unrolled_i8_i8(x, x_scale, row, w_scale);
            choose_best(&mut best, i, score);
        }
        best.0
    } else {
        (0..n_rows)
            .into_par_iter()
            .map(|i| {
                let row = &w_rowmajor[i * k_dim..(i + 1) * k_dim];
                (i, dot_unrolled_i8_i8(x, x_scale, row, w_scale))
            })
            .reduce(
                || (0usize, f32::NEG_INFINITY),
                |a, b| if a.1 >= b.1 { a } else { b },
            )
            .0
    }
}

pub fn matvec_argmax_rowmajor_parallel_mixed(
    x: SliceRef<'_>,
    w_rowmajor: SliceRef<'_>,
    n_rows: usize,
    k_dim: usize,
) -> usize {
    match (x, w_rowmajor) {
        (SliceRef::F32(x), SliceRef::F32(w)) => {
            matvec_argmax_rowmajor_parallel(x, w, n_rows, k_dim)
        }
        (SliceRef::F32(x), SliceRef::F16(w)) => {
            matvec_argmax_rowmajor_parallel_f32_f16(x, w, n_rows, k_dim)
        }
        (SliceRef::F32(x), SliceRef::BF16(w)) => {
            matvec_argmax_rowmajor_parallel_f32_bf16(x, w, n_rows, k_dim)
        }
        (SliceRef::F32(x), SliceRef::I8(w, scale)) => {
            matvec_argmax_rowmajor_parallel_f32_i8(x, w, scale, n_rows, k_dim)
        }
        (SliceRef::F16(x), SliceRef::F32(w)) => with_f16_input_as_f32(x, |x_f32| {
            matvec_argmax_rowmajor_parallel(x_f32, w, n_rows, k_dim)
        }),
        (SliceRef::F16(x), SliceRef::F16(w)) => with_f16_input_as_f32(x, |x_f32| {
            matvec_argmax_rowmajor_parallel_f32_f16(x_f32, w, n_rows, k_dim)
        }),
        (SliceRef::F16(x), SliceRef::BF16(w)) => with_f16_input_as_f32(x, |x_f32| {
            matvec_argmax_rowmajor_parallel_f32_bf16(x_f32, w, n_rows, k_dim)
        }),
        (SliceRef::F16(x), SliceRef::I8(w, scale)) => with_f16_input_as_f32(x, |x_f32| {
            matvec_argmax_rowmajor_parallel_f32_i8(x_f32, w, scale, n_rows, k_dim)
        }),
        (SliceRef::BF16(x), SliceRef::F32(w)) => {
            matvec_argmax_rowmajor_parallel_bf16_f32(x, w, n_rows, k_dim)
        }
        (SliceRef::BF16(x), SliceRef::F16(w)) => with_bf16_input_as_f32(x, |x_f32| {
            matvec_argmax_rowmajor_parallel_f32_f16(x_f32, w, n_rows, k_dim)
        }),
        (SliceRef::BF16(x), SliceRef::BF16(w)) => {
            matvec_argmax_rowmajor_parallel_bf16_bf16(x, w, n_rows, k_dim)
        }
        (SliceRef::BF16(x), SliceRef::I8(w, scale)) => {
            matvec_argmax_rowmajor_parallel_bf16_i8(x, w, scale, n_rows, k_dim)
        }
        (SliceRef::I8(x, scale), SliceRef::F32(w)) => {
            matvec_argmax_rowmajor_parallel_i8_f32(x, scale, w, n_rows, k_dim)
        }
        (SliceRef::I8(x, scale), SliceRef::F16(w)) => with_i8_input_as_f32(x, scale, |x_f32| {
            matvec_argmax_rowmajor_parallel_f32_f16(x_f32, w, n_rows, k_dim)
        }),
        (SliceRef::I8(x, scale), SliceRef::BF16(w)) => {
            matvec_argmax_rowmajor_parallel_i8_bf16(x, scale, w, n_rows, k_dim)
        }
        (SliceRef::I8(x, x_scale), SliceRef::I8(w, w_scale)) => {
            matvec_argmax_rowmajor_parallel_i8_i8(x, x_scale, w, w_scale, n_rows, k_dim)
        }
    }
}

fn matmul_rows_f32_bf16(
    a_view: ndarray::ArrayViewD<'_, f32>,
    b_view: ndarray::ArrayViewD<'_, bf16>,
    m_dim: usize,
    k_dim: usize,
    n_dim: usize,
) -> Array2<f32> {
    let b_2d = b_view.into_dimensionality::<Ix2>().unwrap();
    let b_owned;
    let b_slice: &[bf16] = if let Some(s) = b_2d.as_slice() {
        s
    } else {
        b_owned = b_2d.as_standard_layout().to_owned();
        b_owned
            .as_slice()
            .expect("standard-layout matmul RHS should be contiguous")
    };

    #[cfg(all(feature = "arm64-fp-kernels", target_arch = "aarch64"))]
    if m_dim == 1 {
        let a_flat = a_view
            .clone()
            .into_shape(k_dim)
            .expect("single-row lhs reshape failed");
        let a_owned;
        let a_slice: &[f32] = if let Some(s) = a_flat.as_slice() {
            s
        } else {
            a_owned = a_flat.iter().copied().collect::<Vec<f32>>();
            a_owned.as_slice()
        };
        let mut out_vec = vec![0.0f32; n_dim];
        matvec_rowmajor_parallel_f32_bf16(a_slice, b_slice, n_dim, k_dim, &mut out_vec);
        return Array2::from_shape_vec((1, n_dim), out_vec)
            .expect("decode matvec shape build failed");
    }

    let mut res = Array2::<f32>::zeros((m_dim, n_dim));
    if let Ok(a_2d) = a_view.clone().into_shape((m_dim, k_dim)) {
        Zip::from(res.outer_iter_mut())
            .and(a_2d.outer_iter())
            .par_for_each(|mut out_row, a_row| {
                let a_owned;
                let a_slice: &[f32] = if let Some(s) = a_row.as_slice() {
                    s
                } else {
                    a_owned = a_row.iter().copied().collect::<Vec<f32>>();
                    a_owned.as_slice()
                };
                let out_slice = out_row
                    .as_slice_mut()
                    .expect("matmul output row should be contiguous");
                matvec_rowmajor_serial_f32_bf16(a_slice, b_slice, n_dim, k_dim, out_slice);
            });
    } else {
        let a_2d_owned = a_view
            .to_owned()
            .into_shape((m_dim, k_dim))
            .expect("Reshape A failed");
        Zip::from(res.outer_iter_mut())
            .and(a_2d_owned.outer_iter())
            .par_for_each(|mut out_row, a_row| {
                let a_owned;
                let a_slice: &[f32] = if let Some(s) = a_row.as_slice() {
                    s
                } else {
                    a_owned = a_row.iter().copied().collect::<Vec<f32>>();
                    a_owned.as_slice()
                };
                let out_slice = out_row
                    .as_slice_mut()
                    .expect("matmul output row should be contiguous");
                matvec_rowmajor_serial_f32_bf16(a_slice, b_slice, n_dim, k_dim, out_slice);
            });
    }
    res
}

fn matmul_rows_f32_f16(
    a_view: ndarray::ArrayViewD<'_, f32>,
    b_view: ndarray::ArrayViewD<'_, f16>,
    m_dim: usize,
    k_dim: usize,
    n_dim: usize,
) -> Array2<f32> {
    let b_2d = b_view.into_dimensionality::<Ix2>().unwrap();
    let b_owned;
    let b_slice: &[f16] = if let Some(s) = b_2d.as_slice() {
        s
    } else {
        b_owned = b_2d.as_standard_layout().to_owned();
        b_owned
            .as_slice()
            .expect("standard-layout matmul RHS should be contiguous")
    };

    #[cfg(all(feature = "arm64-fp-kernels", target_arch = "aarch64"))]
    if m_dim == 1 {
        let a_flat = a_view
            .clone()
            .into_shape(k_dim)
            .expect("single-row lhs reshape failed");
        let a_owned;
        let a_slice: &[f32] = if let Some(s) = a_flat.as_slice() {
            s
        } else {
            a_owned = a_flat.iter().copied().collect::<Vec<f32>>();
            a_owned.as_slice()
        };
        let mut out_vec = vec![0.0f32; n_dim];
        matvec_rowmajor_parallel_f32_f16(a_slice, b_slice, n_dim, k_dim, &mut out_vec);
        return Array2::from_shape_vec((1, n_dim), out_vec)
            .expect("decode matvec shape build failed");
    }

    let mut res = Array2::<f32>::zeros((m_dim, n_dim));
    if let Ok(a_2d) = a_view.clone().into_shape((m_dim, k_dim)) {
        Zip::from(res.outer_iter_mut())
            .and(a_2d.outer_iter())
            .par_for_each(|mut out_row, a_row| {
                let a_owned;
                let a_slice: &[f32] = if let Some(s) = a_row.as_slice() {
                    s
                } else {
                    a_owned = a_row.iter().copied().collect::<Vec<f32>>();
                    a_owned.as_slice()
                };
                let out_slice = out_row
                    .as_slice_mut()
                    .expect("matmul output row should be contiguous");
                matvec_rowmajor_serial_f32_f16(a_slice, b_slice, n_dim, k_dim, out_slice);
            });
    } else {
        let a_2d_owned = a_view
            .to_owned()
            .into_shape((m_dim, k_dim))
            .expect("Reshape A failed");
        Zip::from(res.outer_iter_mut())
            .and(a_2d_owned.outer_iter())
            .par_for_each(|mut out_row, a_row| {
                let a_owned;
                let a_slice: &[f32] = if let Some(s) = a_row.as_slice() {
                    s
                } else {
                    a_owned = a_row.iter().copied().collect::<Vec<f32>>();
                    a_owned.as_slice()
                };
                let out_slice = out_row
                    .as_slice_mut()
                    .expect("matmul output row should be contiguous");
                matvec_rowmajor_serial_f32_f16(a_slice, b_slice, n_dim, k_dim, out_slice);
            });
    }
    res
}

fn matmul_rows_f16_f32(
    a_view: ndarray::ArrayViewD<'_, f16>,
    b_view: ndarray::ArrayViewD<'_, f32>,
    m_dim: usize,
    k_dim: usize,
    n_dim: usize,
) -> Array2<f32> {
    let b_2d = b_view.into_dimensionality::<Ix2>().unwrap();
    let b_owned;
    let b_slice: &[f32] = if let Some(s) = b_2d.as_slice() {
        s
    } else {
        b_owned = b_2d.as_standard_layout().to_owned();
        b_owned
            .as_slice()
            .expect("standard-layout matmul RHS should be contiguous")
    };

    #[cfg(all(feature = "arm64-fp-kernels", target_arch = "aarch64"))]
    if m_dim == 1 {
        let a_flat = a_view
            .clone()
            .into_shape(k_dim)
            .expect("single-row lhs reshape failed");
        let a_owned;
        let a_slice: &[f16] = if let Some(s) = a_flat.as_slice() {
            s
        } else {
            a_owned = a_flat.iter().copied().collect::<Vec<f16>>();
            a_owned.as_slice()
        };
        let mut out_vec = vec![0.0f32; n_dim];
        with_f16_input_as_f32(a_slice, |a_f32| {
            matvec_rowmajor_parallel(a_f32, b_slice, n_dim, k_dim, &mut out_vec);
        });
        return Array2::from_shape_vec((1, n_dim), out_vec)
            .expect("decode matvec shape build failed");
    }

    let mut res = Array2::<f32>::zeros((m_dim, n_dim));
    if let Ok(a_2d) = a_view.clone().into_shape((m_dim, k_dim)) {
        Zip::from(res.outer_iter_mut())
            .and(a_2d.outer_iter())
            .par_for_each(|mut out_row, a_row| {
                let a_owned;
                let a_slice: &[f16] = if let Some(s) = a_row.as_slice() {
                    s
                } else {
                    a_owned = a_row.iter().copied().collect::<Vec<f16>>();
                    a_owned.as_slice()
                };
                let out_slice = out_row
                    .as_slice_mut()
                    .expect("matmul output row should be contiguous");
                with_f16_input_as_f32(a_slice, |a_f32| {
                    matvec_rowmajor_serial(a_f32, b_slice, n_dim, k_dim, out_slice);
                });
            });
    } else {
        let a_2d_owned = a_view
            .to_owned()
            .into_shape((m_dim, k_dim))
            .expect("Reshape A failed");
        Zip::from(res.outer_iter_mut())
            .and(a_2d_owned.outer_iter())
            .par_for_each(|mut out_row, a_row| {
                let a_slice = a_row.as_slice().expect("owned row must be contiguous");
                let out_slice = out_row
                    .as_slice_mut()
                    .expect("matmul output row should be contiguous");
                with_f16_input_as_f32(a_slice, |a_f32| {
                    matvec_rowmajor_serial(a_f32, b_slice, n_dim, k_dim, out_slice);
                });
            });
    }
    res
}

fn matmul_rows_f16_f16(
    a_view: ndarray::ArrayViewD<'_, f16>,
    b_view: ndarray::ArrayViewD<'_, f16>,
    m_dim: usize,
    k_dim: usize,
    n_dim: usize,
) -> Array2<f32> {
    let b_2d = b_view.into_dimensionality::<Ix2>().unwrap();
    let b_owned;
    let b_slice: &[f16] = if let Some(s) = b_2d.as_slice() {
        s
    } else {
        b_owned = b_2d.as_standard_layout().to_owned();
        b_owned
            .as_slice()
            .expect("standard-layout matmul RHS should be contiguous")
    };

    #[cfg(all(feature = "arm64-fp-kernels", target_arch = "aarch64"))]
    if m_dim == 1 {
        let a_flat = a_view
            .clone()
            .into_shape(k_dim)
            .expect("single-row lhs reshape failed");
        let a_owned;
        let a_slice: &[f16] = if let Some(s) = a_flat.as_slice() {
            s
        } else {
            a_owned = a_flat.iter().copied().collect::<Vec<f16>>();
            a_owned.as_slice()
        };
        let mut out_vec = vec![0.0f32; n_dim];
        with_f16_input_as_f32(a_slice, |a_f32| {
            matvec_rowmajor_parallel_f32_f16(a_f32, b_slice, n_dim, k_dim, &mut out_vec);
        });
        return Array2::from_shape_vec((1, n_dim), out_vec)
            .expect("decode matvec shape build failed");
    }

    let mut res = Array2::<f32>::zeros((m_dim, n_dim));
    if let Ok(a_2d) = a_view.clone().into_shape((m_dim, k_dim)) {
        Zip::from(res.outer_iter_mut())
            .and(a_2d.outer_iter())
            .par_for_each(|mut out_row, a_row| {
                let a_owned;
                let a_slice: &[f16] = if let Some(s) = a_row.as_slice() {
                    s
                } else {
                    a_owned = a_row.iter().copied().collect::<Vec<f16>>();
                    a_owned.as_slice()
                };
                let out_slice = out_row
                    .as_slice_mut()
                    .expect("matmul output row should be contiguous");
                with_f16_input_as_f32(a_slice, |a_f32| {
                    matvec_rowmajor_serial_f32_f16(a_f32, b_slice, n_dim, k_dim, out_slice);
                });
            });
    } else {
        let a_2d_owned = a_view
            .to_owned()
            .into_shape((m_dim, k_dim))
            .expect("Reshape A failed");
        Zip::from(res.outer_iter_mut())
            .and(a_2d_owned.outer_iter())
            .par_for_each(|mut out_row, a_row| {
                let a_slice = a_row.as_slice().expect("owned row must be contiguous");
                let out_slice = out_row
                    .as_slice_mut()
                    .expect("matmul output row should be contiguous");
                with_f16_input_as_f32(a_slice, |a_f32| {
                    matvec_rowmajor_serial_f32_f16(a_f32, b_slice, n_dim, k_dim, out_slice);
                });
            });
    }
    res
}

fn matmul_rows_f16_slice(
    a_view: ndarray::ArrayViewD<'_, f16>,
    b_rowmajor: SliceRef<'_>,
    m_dim: usize,
    k_dim: usize,
    n_dim: usize,
) -> Array2<f32> {
    #[cfg(all(feature = "arm64-fp-kernels", target_arch = "aarch64"))]
    if m_dim == 1 {
        let a_flat = a_view
            .clone()
            .into_shape(k_dim)
            .expect("single-row lhs reshape failed");
        let a_owned;
        let a_slice: &[f16] = if let Some(s) = a_flat.as_slice() {
            s
        } else {
            a_owned = a_flat.iter().copied().collect::<Vec<f16>>();
            a_owned.as_slice()
        };
        let mut out_vec = vec![0.0f32; n_dim];
        matvec_rowmajor_parallel_mixed(
            SliceRef::F16(a_slice),
            b_rowmajor,
            n_dim,
            k_dim,
            &mut out_vec,
        );
        return Array2::from_shape_vec((1, n_dim), out_vec)
            .expect("decode matvec shape build failed");
    }

    let mut res = Array2::<f32>::zeros((m_dim, n_dim));
    if let Ok(a_2d) = a_view.clone().into_shape((m_dim, k_dim)) {
        Zip::from(res.outer_iter_mut())
            .and(a_2d.outer_iter())
            .par_for_each(|mut out_row, a_row| {
                let a_owned;
                let a_slice: &[f16] = if let Some(s) = a_row.as_slice() {
                    s
                } else {
                    a_owned = a_row.iter().copied().collect::<Vec<f16>>();
                    a_owned.as_slice()
                };
                let out_slice = out_row
                    .as_slice_mut()
                    .expect("output row should be contiguous");
                matvec_rowmajor_parallel_mixed(
                    SliceRef::F16(a_slice),
                    b_rowmajor,
                    n_dim,
                    k_dim,
                    out_slice,
                );
            });
    } else {
        let a_2d = a_view
            .to_owned()
            .into_shape((m_dim, k_dim))
            .expect("Reshape A failed");
        Zip::from(res.outer_iter_mut())
            .and(a_2d.outer_iter())
            .par_for_each(|mut out_row, a_row| {
                let a_slice = a_row.as_slice().expect("owned row must be contiguous");
                let out_slice = out_row
                    .as_slice_mut()
                    .expect("output row should be contiguous");
                matvec_rowmajor_parallel_mixed(
                    SliceRef::F16(a_slice),
                    b_rowmajor,
                    n_dim,
                    k_dim,
                    out_slice,
                );
            });
    }
    res
}

fn matmul_rows_i8_slice(
    a_view: ndarray::ArrayViewD<'_, i8>,
    a_scale: f32,
    b_rowmajor: SliceRef<'_>,
    m_dim: usize,
    k_dim: usize,
    n_dim: usize,
) -> Array2<f32> {
    let mut res = Array2::<f32>::zeros((m_dim, n_dim));
    if let Ok(a_2d) = a_view.clone().into_shape((m_dim, k_dim)) {
        Zip::from(res.outer_iter_mut())
            .and(a_2d.outer_iter())
            .par_for_each(|mut out_row, a_row| {
                let a_owned;
                let a_slice: &[i8] = if let Some(s) = a_row.as_slice() {
                    s
                } else {
                    a_owned = a_row.iter().copied().collect::<Vec<i8>>();
                    a_owned.as_slice()
                };
                let out_slice = out_row
                    .as_slice_mut()
                    .expect("output row should be contiguous");
                matvec_rowmajor_parallel_mixed(
                    SliceRef::I8(a_slice, a_scale),
                    b_rowmajor,
                    n_dim,
                    k_dim,
                    out_slice,
                );
            });
    } else {
        let a_2d = a_view
            .to_owned()
            .into_shape((m_dim, k_dim))
            .expect("Reshape A failed");
        Zip::from(res.outer_iter_mut())
            .and(a_2d.outer_iter())
            .par_for_each(|mut out_row, a_row| {
                let a_slice = a_row.as_slice().expect("owned row must be contiguous");
                let out_slice = out_row
                    .as_slice_mut()
                    .expect("output row should be contiguous");
                matvec_rowmajor_parallel_mixed(
                    SliceRef::I8(a_slice, a_scale),
                    b_rowmajor,
                    n_dim,
                    k_dim,
                    out_slice,
                );
            });
    }
    res
}

fn matmul_rows_bf16_slice(
    a_view: ndarray::ArrayViewD<'_, bf16>,
    b_rowmajor: SliceRef<'_>,
    m_dim: usize,
    k_dim: usize,
    n_dim: usize,
) -> Array2<f32> {
    #[cfg(all(feature = "arm64-fp-kernels", target_arch = "aarch64"))]
    if m_dim == 1 {
        let a_flat = a_view
            .clone()
            .into_shape(k_dim)
            .expect("single-row lhs reshape failed");
        let a_owned;
        let a_slice: &[bf16] = if let Some(s) = a_flat.as_slice() {
            s
        } else {
            a_owned = a_flat.iter().copied().collect::<Vec<bf16>>();
            a_owned.as_slice()
        };
        let mut out_vec = vec![0.0f32; n_dim];
        matvec_rowmajor_parallel_mixed(
            SliceRef::BF16(a_slice),
            b_rowmajor,
            n_dim,
            k_dim,
            &mut out_vec,
        );
        return Array2::from_shape_vec((1, n_dim), out_vec)
            .expect("decode matvec shape build failed");
    }

    let mut res = Array2::<f32>::zeros((m_dim, n_dim));
    if let Ok(a_2d) = a_view.clone().into_shape((m_dim, k_dim)) {
        Zip::from(res.outer_iter_mut())
            .and(a_2d.outer_iter())
            .par_for_each(|mut out_row, a_row| {
                let a_owned;
                let a_slice: &[bf16] = if let Some(s) = a_row.as_slice() {
                    s
                } else {
                    a_owned = a_row.iter().copied().collect::<Vec<bf16>>();
                    a_owned.as_slice()
                };
                let out_slice = out_row
                    .as_slice_mut()
                    .expect("output row should be contiguous");
                matvec_rowmajor_parallel_mixed(
                    SliceRef::BF16(a_slice),
                    b_rowmajor,
                    n_dim,
                    k_dim,
                    out_slice,
                );
            });
    } else {
        let a_2d = a_view
            .to_owned()
            .into_shape((m_dim, k_dim))
            .expect("Reshape A failed");
        Zip::from(res.outer_iter_mut())
            .and(a_2d.outer_iter())
            .par_for_each(|mut out_row, a_row| {
                let a_slice = a_row.as_slice().expect("owned row must be contiguous");
                let out_slice = out_row
                    .as_slice_mut()
                    .expect("output row should be contiguous");
                matvec_rowmajor_parallel_mixed(
                    SliceRef::BF16(a_slice),
                    b_rowmajor,
                    n_dim,
                    k_dim,
                    out_slice,
                );
            });
    }
    res
}

fn matmul_rows_bf16_f32(
    a_view: ndarray::ArrayViewD<'_, bf16>,
    b_view: ndarray::ArrayViewD<'_, f32>,
    m_dim: usize,
    k_dim: usize,
    n_dim: usize,
) -> Array2<f32> {
    let b_2d = b_view.into_dimensionality::<Ix2>().unwrap();
    let b_owned;
    let b_slice: &[f32] = if let Some(s) = b_2d.as_slice() {
        s
    } else {
        b_owned = b_2d.as_standard_layout().to_owned();
        b_owned
            .as_slice()
            .expect("standard-layout matmul RHS should be contiguous")
    };

    #[cfg(all(feature = "arm64-fp-kernels", target_arch = "aarch64"))]
    if m_dim == 1 {
        let a_flat = a_view
            .clone()
            .into_shape(k_dim)
            .expect("single-row lhs reshape failed");
        let a_owned;
        let a_slice: &[bf16] = if let Some(s) = a_flat.as_slice() {
            s
        } else {
            a_owned = a_flat.iter().copied().collect::<Vec<bf16>>();
            a_owned.as_slice()
        };
        let mut out_vec = vec![0.0f32; n_dim];
        with_bf16_input_as_f32(a_slice, |a_f32| {
            matvec_rowmajor_parallel(a_f32, b_slice, n_dim, k_dim, &mut out_vec);
        });
        return Array2::from_shape_vec((1, n_dim), out_vec)
            .expect("decode matvec shape build failed");
    }

    let mut res = Array2::<f32>::zeros((m_dim, n_dim));
    if let Ok(a_2d) = a_view.clone().into_shape((m_dim, k_dim)) {
        Zip::from(res.outer_iter_mut())
            .and(a_2d.outer_iter())
            .par_for_each(|mut out_row, a_row| {
                let a_owned;
                let a_slice: &[bf16] = if let Some(s) = a_row.as_slice() {
                    s
                } else {
                    a_owned = a_row.iter().copied().collect::<Vec<bf16>>();
                    a_owned.as_slice()
                };
                let out_slice = out_row
                    .as_slice_mut()
                    .expect("matmul output row should be contiguous");
                with_bf16_input_as_f32(a_slice, |a_f32| {
                    matvec_rowmajor_serial(a_f32, b_slice, n_dim, k_dim, out_slice);
                });
            });
    } else {
        let a_2d_owned = a_view
            .to_owned()
            .into_shape((m_dim, k_dim))
            .expect("Reshape A failed");
        Zip::from(res.outer_iter_mut())
            .and(a_2d_owned.outer_iter())
            .par_for_each(|mut out_row, a_row| {
                let a_owned;
                let a_slice: &[bf16] = if let Some(s) = a_row.as_slice() {
                    s
                } else {
                    a_owned = a_row.iter().copied().collect::<Vec<bf16>>();
                    a_owned.as_slice()
                };
                let out_slice = out_row
                    .as_slice_mut()
                    .expect("matmul output row should be contiguous");
                with_bf16_input_as_f32(a_slice, |a_f32| {
                    matvec_rowmajor_serial(a_f32, b_slice, n_dim, k_dim, out_slice);
                });
            });
    }
    res
}

fn matmul_rows_bf16_bf16(
    a_view: ndarray::ArrayViewD<'_, bf16>,
    b_view: ndarray::ArrayViewD<'_, bf16>,
    m_dim: usize,
    k_dim: usize,
    n_dim: usize,
) -> Array2<f32> {
    let b_2d = b_view.into_dimensionality::<Ix2>().unwrap();
    let b_owned;
    let b_slice: &[bf16] = if let Some(s) = b_2d.as_slice() {
        s
    } else {
        b_owned = b_2d.as_standard_layout().to_owned();
        b_owned
            .as_slice()
            .expect("standard-layout matmul RHS should be contiguous")
    };

    #[cfg(all(feature = "arm64-fp-kernels", target_arch = "aarch64"))]
    if m_dim == 1 {
        let a_flat = a_view
            .clone()
            .into_shape(k_dim)
            .expect("single-row lhs reshape failed");
        let a_owned;
        let a_slice: &[bf16] = if let Some(s) = a_flat.as_slice() {
            s
        } else {
            a_owned = a_flat.iter().copied().collect::<Vec<bf16>>();
            a_owned.as_slice()
        };
        let mut out_vec = vec![0.0f32; n_dim];
        with_bf16_input_as_f32(a_slice, |a_f32| {
            matvec_rowmajor_parallel_f32_bf16(a_f32, b_slice, n_dim, k_dim, &mut out_vec);
        });
        return Array2::from_shape_vec((1, n_dim), out_vec)
            .expect("decode matvec shape build failed");
    }

    let mut res = Array2::<f32>::zeros((m_dim, n_dim));
    if let Ok(a_2d) = a_view.clone().into_shape((m_dim, k_dim)) {
        Zip::from(res.outer_iter_mut())
            .and(a_2d.outer_iter())
            .par_for_each(|mut out_row, a_row| {
                let a_owned;
                let a_slice: &[bf16] = if let Some(s) = a_row.as_slice() {
                    s
                } else {
                    a_owned = a_row.iter().copied().collect::<Vec<bf16>>();
                    a_owned.as_slice()
                };
                let out_slice = out_row
                    .as_slice_mut()
                    .expect("matmul output row should be contiguous");
                with_bf16_input_as_f32(a_slice, |a_f32| {
                    matvec_rowmajor_serial_f32_bf16(a_f32, b_slice, n_dim, k_dim, out_slice);
                });
            });
    } else {
        let a_2d_owned = a_view
            .to_owned()
            .into_shape((m_dim, k_dim))
            .expect("Reshape A failed");
        Zip::from(res.outer_iter_mut())
            .and(a_2d_owned.outer_iter())
            .par_for_each(|mut out_row, a_row| {
                let a_owned;
                let a_slice: &[bf16] = if let Some(s) = a_row.as_slice() {
                    s
                } else {
                    a_owned = a_row.iter().copied().collect::<Vec<bf16>>();
                    a_owned.as_slice()
                };
                let out_slice = out_row
                    .as_slice_mut()
                    .expect("matmul output row should be contiguous");
                with_bf16_input_as_f32(a_slice, |a_f32| {
                    matvec_rowmajor_serial_f32_bf16(a_f32, b_slice, n_dim, k_dim, out_slice);
                });
            });
    }
    res
}

fn matmul_rows_f32_i8(
    a_view: ndarray::ArrayViewD<'_, f32>,
    b_view: ndarray::ArrayViewD<'_, i8>,
    scale: f32,
    m_dim: usize,
    k_dim: usize,
    n_dim: usize,
) -> Array2<f32> {
    let b_2d = b_view.into_dimensionality::<Ix2>().unwrap();
    let b_owned;
    let b_slice: &[i8] = if let Some(s) = b_2d.as_slice() {
        s
    } else {
        b_owned = b_2d.as_standard_layout().to_owned();
        b_owned
            .as_slice()
            .expect("standard-layout matmul RHS should be contiguous")
    };

    let mut res = Array2::<f32>::zeros((m_dim, n_dim));
    if let Ok(a_2d) = a_view.clone().into_shape((m_dim, k_dim)) {
        Zip::from(res.outer_iter_mut())
            .and(a_2d.outer_iter())
            .par_for_each(|mut out_row, a_row| {
                let a_owned;
                let a_slice: &[f32] = if let Some(s) = a_row.as_slice() {
                    s
                } else {
                    a_owned = a_row.iter().copied().collect::<Vec<f32>>();
                    a_owned.as_slice()
                };
                let out_slice = out_row
                    .as_slice_mut()
                    .expect("matmul output row should be contiguous");
                matvec_rowmajor_serial_f32_i8(a_slice, b_slice, scale, n_dim, k_dim, out_slice);
            });
    } else {
        let a_2d_owned = a_view
            .to_owned()
            .into_shape((m_dim, k_dim))
            .expect("Reshape A failed");
        Zip::from(res.outer_iter_mut())
            .and(a_2d_owned.outer_iter())
            .par_for_each(|mut out_row, a_row| {
                let a_owned;
                let a_slice: &[f32] = if let Some(s) = a_row.as_slice() {
                    s
                } else {
                    a_owned = a_row.iter().copied().collect::<Vec<f32>>();
                    a_owned.as_slice()
                };
                let out_slice = out_row
                    .as_slice_mut()
                    .expect("matmul output row should be contiguous");
                matvec_rowmajor_serial_f32_i8(a_slice, b_slice, scale, n_dim, k_dim, out_slice);
            });
    }
    res
}

fn matmul_rows_bf16_i8(
    a_view: ndarray::ArrayViewD<'_, bf16>,
    b_view: ndarray::ArrayViewD<'_, i8>,
    scale: f32,
    m_dim: usize,
    k_dim: usize,
    n_dim: usize,
) -> Array2<f32> {
    let b_2d = b_view.into_dimensionality::<Ix2>().unwrap();
    let b_owned;
    let b_slice: &[i8] = if let Some(s) = b_2d.as_slice() {
        s
    } else {
        b_owned = b_2d.as_standard_layout().to_owned();
        b_owned
            .as_slice()
            .expect("standard-layout matmul RHS should be contiguous")
    };

    let mut res = Array2::<f32>::zeros((m_dim, n_dim));
    if let Ok(a_2d) = a_view.clone().into_shape((m_dim, k_dim)) {
        Zip::from(res.outer_iter_mut())
            .and(a_2d.outer_iter())
            .par_for_each(|mut out_row, a_row| {
                let a_owned;
                let a_slice: &[bf16] = if let Some(s) = a_row.as_slice() {
                    s
                } else {
                    a_owned = a_row.iter().copied().collect::<Vec<bf16>>();
                    a_owned.as_slice()
                };
                let out_slice = out_row
                    .as_slice_mut()
                    .expect("matmul output row should be contiguous");
                with_bf16_input_as_f32(a_slice, |a_f32| {
                    matvec_rowmajor_serial_f32_i8(a_f32, b_slice, scale, n_dim, k_dim, out_slice);
                });
            });
    } else {
        let a_2d_owned = a_view
            .to_owned()
            .into_shape((m_dim, k_dim))
            .expect("Reshape A failed");
        Zip::from(res.outer_iter_mut())
            .and(a_2d_owned.outer_iter())
            .par_for_each(|mut out_row, a_row| {
                let a_owned;
                let a_slice: &[bf16] = if let Some(s) = a_row.as_slice() {
                    s
                } else {
                    a_owned = a_row.iter().copied().collect::<Vec<bf16>>();
                    a_owned.as_slice()
                };
                let out_slice = out_row
                    .as_slice_mut()
                    .expect("matmul output row should be contiguous");
                with_bf16_input_as_f32(a_slice, |a_f32| {
                    matvec_rowmajor_serial_f32_i8(a_f32, b_slice, scale, n_dim, k_dim, out_slice);
                });
            });
    }
    res
}

fn try_cuda_matmul_buffer(
    a: &Tensor,
    b: &Tensor,
    m_dim: usize,
    k_dim: usize,
    n_dim: usize,
) -> Option<cuda::CudaBuffer> {
    if a.device() != crate::autograd::Device::Cuda || b.device() != crate::autograd::Device::Cuda {
        return None;
    }
    let force_cuda = is_strict_device_execution();
    if !force_cuda && !cuda::should_accelerate_matmul(m_dim, n_dim, k_dim) {
        return None;
    }
    let cuda_out = a.with_cuda_f32_buffer(|a_buf| {
        b.with_cuda_f32_buffer(|b_buf| cuda::matmul_f32_no_host(a_buf, b_buf, m_dim, n_dim, k_dim))
    });
    Some(match cuda_out {
        Ok(out) => out,
        Err(err) => {
            if force_cuda {
                panic!("CUDA matmul failed in strict device execution mode: {err}");
            }
            return None;
        }
    })
}

fn try_cuda_matmul(
    a: &Tensor,
    b: &Tensor,
    m_dim: usize,
    n_dim: usize,
    k_dim: usize,
    output_dtype: DType,
) -> Option<Tensor> {
    let buffer = try_cuda_matmul_buffer(a, b, m_dim, k_dim, n_dim)?;
    let mut out_shape = a.shape_vec();
    let last_idx = out_shape.len() - 1;
    out_shape[last_idx] = n_dim;

    if output_dtype == DType::F32 {
        return Some(Tensor::from_cuda_f32_buffer_no_host(
            &out_shape,
            buffer,
            a.device(),
        ));
    }

    if matches!(output_dtype, DType::F16 | DType::BF16) {
        return Some(Tensor::from_cuda_f32_buffer_no_host_with_dtype(
            &out_shape,
            buffer,
            a.device(),
            output_dtype,
        ));
    }

    let out = cuda::download_f32(&buffer).ok()?;
    Some(
        Tensor::from_f32_data_no_grad_with_device_dtype_and_cuda_buffer(
            Array2::from_shape_vec((m_dim, n_dim), out)
                .expect("CUDA matmul output shape build failed")
                .into_shape(out_shape)
                .expect("CUDA matmul output reshape failed")
                .into_dyn(),
            output_dtype,
            a.device(),
            Some(buffer),
        ),
    )
}

fn try_cuda_batch_matmul_buffer(
    lhs: &Tensor,
    rhs: &Tensor,
    b: usize,
    h: usize,
    m: usize,
    k: usize,
    n: usize,
) -> Option<cuda::CudaBuffer> {
    if lhs.device() != crate::autograd::Device::Cuda
        || rhs.device() != crate::autograd::Device::Cuda
    {
        return None;
    }
    let batch_count = b.checked_mul(h)?;
    let force_cuda = is_strict_device_execution();
    if !force_cuda && !cuda::should_accelerate_batch_matmul(batch_count, m, n, k) {
        return None;
    }
    let cuda_out = lhs.with_cuda_f32_buffer(|lhs_buf| {
        rhs.with_cuda_f32_buffer(|rhs_buf| {
            cuda::batch_matmul_f32_no_host(lhs_buf, rhs_buf, batch_count, m, n, k)
        })
    });
    Some(match cuda_out {
        Ok(out) => out,
        Err(err) => {
            if force_cuda {
                panic!("CUDA batch_matmul failed in strict device execution mode: {err}");
            }
            return None;
        }
    })
}

fn try_cuda_batch_matmul(
    lhs: &Tensor,
    rhs: &Tensor,
    b: usize,
    h: usize,
    m: usize,
    k: usize,
    n: usize,
    output_dtype: DType,
) -> Option<Tensor> {
    let buffer = try_cuda_batch_matmul_buffer(lhs, rhs, b, h, m, k, n)?;
    let out_shape = vec![b, h, m, n];
    if output_dtype == DType::F32 {
        return Some(Tensor::from_cuda_f32_buffer_no_host(
            &out_shape,
            buffer,
            lhs.device(),
        ));
    }

    if matches!(output_dtype, DType::F16 | DType::BF16) {
        return Some(Tensor::from_cuda_f32_buffer_no_host_with_dtype(
            &out_shape,
            buffer,
            lhs.device(),
            output_dtype,
        ));
    }

    let out = cuda::download_f32(&buffer).ok()?;
    Some(
        Tensor::from_f32_data_no_grad_with_device_dtype_and_cuda_buffer(
            Array4::from_shape_vec((b, h, m, n), out)
                .expect("CUDA batch_matmul output shape build failed")
                .into_dyn(),
            output_dtype,
            lhs.device(),
            Some(buffer),
        ),
    )
}

fn try_cuda_training_matmul_backward(
    grad: &ndarray::ArrayViewD<'_, f32>,
    cuda_grad: Option<cuda::CudaBuffer>,
    a: &Tensor,
    b: &Tensor,
    m_dim: usize,
    k_dim: usize,
    n_dim: usize,
) -> Result<
    (
        (ndarray::ArrayD<f32>, cuda::CudaBuffer),
        (ndarray::ArrayD<f32>, cuda::CudaBuffer),
    ),
    String,
> {
    let (da_buf, db_buf) =
        try_cuda_training_matmul_backward_buffers(grad, cuda_grad, a, b, m_dim, k_dim, n_dim)?;
    let da_host = cuda::download_f32(&da_buf)?;
    let db_host = cuda::download_f32(&db_buf)?;

    let da = Array2::from_shape_vec((m_dim, k_dim), da_host)
        .expect("CUDA matmul backward dA shape build failed")
        .into_shape(a.shape_vec())
        .expect("CUDA matmul backward dA reshape failed")
        .into_dyn();
    let db = Array2::from_shape_vec((n_dim, k_dim), db_host)
        .expect("CUDA matmul backward dB shape build failed")
        .into_dyn();
    Ok(((da, da_buf), (db, db_buf)))
}

fn try_cuda_training_matmul_backward_buffers(
    grad: &ndarray::ArrayViewD<'_, f32>,
    cuda_grad: Option<cuda::CudaBuffer>,
    a: &Tensor,
    b: &Tensor,
    m_dim: usize,
    k_dim: usize,
    n_dim: usize,
) -> Result<(cuda::CudaBuffer, cuda::CudaBuffer), String> {
    let grad_buf = match cuda_grad {
        Some(buffer) if buffer.len() == grad.len() => buffer,
        _ => cuda::upload_f32(&grad.iter().copied().collect::<Vec<_>>())?,
    };
    let a_buf = a
        .cloned_cuda_f32_buffer()
        .ok_or_else(|| "CUDA matmul backward expected lhs resident buffer".to_string())?;
    let b_buf = b
        .cloned_cuda_f32_buffer()
        .ok_or_else(|| "CUDA matmul backward expected rhs resident buffer".to_string())?;

    let grad_t = cuda::permute_f32_buffer(&grad_buf, &[n_dim, m_dim], &[1, 0])?;
    let b_t = cuda::permute_f32_buffer(&b_buf, &[k_dim, n_dim], &[1, 0])?;
    let a_t = cuda::permute_f32_buffer(&a_buf, &[k_dim, m_dim], &[1, 0])?;

    let da_buf = cuda::matmul_f32_no_host(&grad_buf, &b_t, m_dim, k_dim, n_dim)?;
    let db_buf = cuda::matmul_f32_no_host(&grad_t, &a_t, n_dim, k_dim, m_dim)?;
    Ok((da_buf, db_buf))
}

fn try_cuda_training_batch_matmul_backward(
    grad: &ndarray::ArrayViewD<'_, f32>,
    cuda_grad: Option<cuda::CudaBuffer>,
    lhs: &Tensor,
    rhs: &Tensor,
    b: usize,
    h: usize,
    m: usize,
    k: usize,
    n: usize,
) -> Result<
    (
        (ndarray::ArrayD<f32>, cuda::CudaBuffer),
        (ndarray::ArrayD<f32>, cuda::CudaBuffer),
    ),
    String,
> {
    let (d_lhs_buf, d_rhs_buf) =
        try_cuda_training_batch_matmul_backward_buffers(grad, cuda_grad, lhs, rhs, b, h, m, k, n)?;
    let d_lhs_host = cuda::download_f32(&d_lhs_buf)?;
    let d_rhs_host = cuda::download_f32(&d_rhs_buf)?;

    let d_lhs = Array4::from_shape_vec((b, h, m, k), d_lhs_host)
        .expect("CUDA batch_matmul backward dLHS shape build failed")
        .into_dyn();
    let d_rhs = Array4::from_shape_vec((b, h, k, n), d_rhs_host)
        .expect("CUDA batch_matmul backward dRHS shape build failed")
        .into_dyn();
    Ok(((d_lhs, d_lhs_buf), (d_rhs, d_rhs_buf)))
}

fn try_cuda_training_batch_matmul_backward_buffers(
    grad: &ndarray::ArrayViewD<'_, f32>,
    cuda_grad: Option<cuda::CudaBuffer>,
    lhs: &Tensor,
    rhs: &Tensor,
    b: usize,
    h: usize,
    m: usize,
    k: usize,
    n: usize,
) -> Result<(cuda::CudaBuffer, cuda::CudaBuffer), String> {
    let batch_count = b
        .checked_mul(h)
        .ok_or_else(|| "CUDA batch_matmul backward batch count overflow".to_string())?;
    let grad_buf = match cuda_grad {
        Some(buffer) if buffer.len() == grad.len() => buffer,
        _ => cuda::upload_f32(&grad.iter().copied().collect::<Vec<_>>())?,
    };
    let lhs_buf = lhs
        .cloned_cuda_f32_buffer()
        .ok_or_else(|| "CUDA batch_matmul backward expected lhs resident buffer".to_string())?;
    let rhs_buf = rhs
        .cloned_cuda_f32_buffer()
        .ok_or_else(|| "CUDA batch_matmul backward expected rhs resident buffer".to_string())?;

    let lhs_t = cuda::permute_f32_buffer(&lhs_buf, &[b, h, k, m], &[0, 1, 3, 2])?;
    let rhs_t = cuda::permute_f32_buffer(&rhs_buf, &[b, h, n, k], &[0, 1, 3, 2])?;

    let d_lhs_buf = cuda::batch_matmul_f32_no_host(&grad_buf, &rhs_t, batch_count, m, k, n)?;
    let d_rhs_buf = cuda::batch_matmul_f32_no_host(&lhs_t, &grad_buf, batch_count, k, n, m)?;
    Ok((d_lhs_buf, d_rhs_buf))
}

// A[..., K] @ B^T, where B is [N(out), K(in)]
// output: [..., N]
pub fn matmul(a: &Tensor, b: &Tensor) -> Tensor {
    let output_device = assert_same_device(a, b, "matmul");
    let build_graph = !is_no_grad() && (a.requires_grad() || b.requires_grad());
    let cuda_native_supported = output_device == crate::autograd::Device::Cuda;
    assert_native_device_support(output_device, "matmul", cuda_native_supported);

    let a_shape = a.shape_vec();
    let b_shape = b.shape_vec();
    let a_len = a.len();

    if b_shape.len() != 2 {
        panic!("MatMul RHS must be 2D, got {:?}", b_shape);
    }

    let k_dim_a = a_shape[a_shape.len() - 1];
    let n_dim = b_shape[0];
    let k_dim_b = b_shape[1];

    if k_dim_a != k_dim_b {
        panic!(
            "MatMul shape mismatch: a {:?} (K={}) vs b {:?} (K={})",
            a_shape, k_dim_a, b_shape, k_dim_b
        );
    }

    let m_dim = a_len / k_dim_a;

    if build_graph
        && output_device == crate::autograd::Device::Cuda
        && let Some(buffer) = try_cuda_matmul_buffer(a, b, m_dim, k_dim_a, n_dim)
    {
        let mut out_shape = a_shape.clone();
        let last_idx = out_shape.len() - 1;
        out_shape[last_idx] = n_dim;

        let a_clone = a.clone();
        let b_clone = b.clone();
        let output_self = Rc::new(RefCell::new(None::<Tensor>));
        let output_self_for_backward = output_self.clone();
        let tensor = Tensor(Rc::new(RefCell::new(TensorData {
            data: ndarray::ArrayD::<f32>::zeros(ndarray::IxDyn(&out_shape)).into_shared(),
            f16_data: None,
            bf16_data: None,
            i8_data: None,
            cuda_f32_data: Some(buffer),
            i8_scale: None,
            has_f32_data: false,
            storage_dtype: crate::precision::DType::F32,
            cache_dirty: false,
            is_parameter: false,
            grad: None,
            cuda_f32_grad: None,
            parents: vec![a_clone.clone(), b_clone.clone()],
            requires_grad: true,
            backward_op: Some(std::rc::Rc::new(move |grad: &ndarray::ArrayViewD<f32>| {
                let cuda_grad = output_self_for_backward
                    .borrow()
                    .as_ref()
                    .and_then(|output| output.cloned_cuda_f32_grad())
                    .filter(|buffer| buffer.len() == grad.len());
                if is_strict_device_execution() {
                    match try_cuda_training_matmul_backward_buffers(
                        grad,
                        cuda_grad.clone(),
                        &a_clone,
                        &b_clone,
                        m_dim,
                        k_dim_a,
                        n_dim,
                    ) {
                        Ok((da_buf, db_buf)) => {
                            a_clone.add_cuda_grad_buffer_only(da_buf);
                            b_clone.add_cuda_grad_buffer_only(db_buf);
                            return;
                        }
                        Err(err) => {
                            panic!(
                                "CUDA matmul backward failed in strict device execution mode: {err}"
                            );
                        }
                    }
                }
                let cuda_result = try_cuda_training_matmul_backward(
                    grad, cuda_grad, &a_clone, &b_clone, m_dim, k_dim_a, n_dim,
                );
                match cuda_result {
                    Ok(((da, da_buf), (db, db_buf))) => {
                        a_clone.add_grad_with_cuda_buffer(da, Some(da_buf));
                        b_clone.add_grad_with_cuda_buffer(db, Some(db_buf));
                    }
                    Err(err) => {
                        if is_strict_device_execution() {
                            panic!(
                                "CUDA matmul backward failed in strict device execution mode: {err}"
                            );
                        }
                        let g_len = grad.len();
                        let g_m = g_len / n_dim;
                        let grad_2d = grad
                            .view()
                            .into_shape((g_m, n_dim))
                            .expect("Grad reshape failed: non-contiguous gradient?");
                        let (a_data, b_data) = {
                            let ad = a_clone.0.borrow();
                            let bd = b_clone.0.borrow();
                            (ad.data.clone(), bd.data.clone())
                        };
                        let a_2d_view = a_data.view().into_shape((m_dim, k_dim_a));
                        let a_2d_owned;
                        let a_2d = match a_2d_view {
                            Ok(v) => v,
                            Err(_) => {
                                a_2d_owned =
                                    a_data.to_owned().into_shape((m_dim, k_dim_a)).unwrap();
                                a_2d_owned.view()
                            }
                        };
                        let b_2d = b_data.view().into_dimensionality::<Ix2>().unwrap();
                        let mut da_2d = Array2::<f32>::zeros((m_dim, k_dim_a));
                        general_mat_mul(1.0, &grad_2d, &b_2d, 0.0, &mut da_2d);
                        a_clone.add_grad(da_2d.into_shape(a_data.shape()).unwrap().into_dyn());
                        let mut db_2d = Array2::<f32>::zeros((n_dim, k_dim_a));
                        general_mat_mul(1.0, &grad_2d.t(), &a_2d, 0.0, &mut db_2d);
                        b_clone.add_grad(db_2d.into_dyn());
                    }
                }
            })),
            device: output_device,
        })));
        *output_self.borrow_mut() = Some(tensor.clone());
        return tensor;
    }

    if !build_graph {
        let input_dtype = a.dtype();
        let output_dtype = if a.dtype() == b.dtype() {
            a.dtype()
        } else {
            DType::F32
        };
        if let Some(cuda_out) = try_cuda_matmul(a, b, m_dim, n_dim, k_dim_a, output_dtype) {
            return cuda_out;
        }
        if b.dtype() == DType::I8 {
            let res_2d = match b.native_storage_owned() {
                TensorStorageOwned::I8(b_data, scale) => {
                    let b_2d = b_data
                        .view()
                        .into_dimensionality::<Ix2>()
                        .expect("matmul RHS must be 2D [N, K]");
                    let b_owned;
                    let b_slice: &[i8] = if let Some(s) = b_2d.as_slice() {
                        s
                    } else {
                        b_owned = b_2d.as_standard_layout().to_owned();
                        b_owned
                            .as_slice()
                            .expect("standard-layout matmul RHS should be contiguous")
                    };

                    if input_dtype == DType::I8 {
                        match a.native_storage_owned() {
                            TensorStorageOwned::I8(a_data, a_scale) => {
                                if m_dim == 1 {
                                    let a_owned;
                                    let a_vec: &[i8] = if let Some(s) = a_data.as_slice() {
                                        s
                                    } else {
                                        a_owned = a_data.iter().copied().collect::<Vec<i8>>();
                                        a_owned.as_slice()
                                    };

                                    let mut out_vec = vec![0.0f32; n_dim];
                                    matvec_rowmajor_parallel_i8_i8(
                                        a_vec,
                                        a_scale,
                                        b_slice,
                                        scale,
                                        n_dim,
                                        k_dim_a,
                                        &mut out_vec,
                                    );
                                    Array2::from_shape_vec((1, n_dim), out_vec)
                                        .expect("decode matvec shape build failed")
                                } else {
                                    matmul_rows_i8_slice(
                                        a_data.view().into_dyn(),
                                        a_scale,
                                        SliceRef::I8(b_slice, scale),
                                        m_dim,
                                        k_dim_a,
                                        n_dim,
                                    )
                                }
                            }
                            TensorStorageOwned::F32(_)
                            | TensorStorageOwned::F16(_)
                            | TensorStorageOwned::BF16(_) => {
                                unreachable!("checked i8 lhs above")
                            }
                        }
                    } else {
                        a.with_storage_view_preferring(StoragePreference::Native, |a_view| {
                            if m_dim == 1 {
                                match a_view {
                                    TensorStorageView::F32(a_view) => {
                                        let a_owned;
                                        let a_vec: &[f32] = if let Some(s) = a_view.as_slice() {
                                            s
                                        } else {
                                            a_owned = a_view.iter().copied().collect::<Vec<f32>>();
                                            a_owned.as_slice()
                                        };

                                        let mut out_vec = vec![0.0f32; n_dim];
                                        matvec_rowmajor_parallel_f32_i8_matmul(
                                            a_vec,
                                            b_slice,
                                            scale,
                                            n_dim,
                                            k_dim_a,
                                            &mut out_vec,
                                        );
                                        Array2::from_shape_vec((1, n_dim), out_vec)
                                            .expect("decode matvec shape build failed")
                                    }
                                    TensorStorageView::F16(a_view) => {
                                        let a_owned;
                                        let a_vec: &[f16] = if let Some(s) = a_view.as_slice() {
                                            s
                                        } else {
                                            a_owned = a_view.iter().copied().collect::<Vec<f16>>();
                                            a_owned.as_slice()
                                        };

                                        let mut out_vec = vec![0.0f32; n_dim];
                                        with_f16_input_as_f32(a_vec, |a_f32| {
                                            matvec_rowmajor_parallel_f32_i8_matmul(
                                                a_f32,
                                                b_slice,
                                                scale,
                                                n_dim,
                                                k_dim_a,
                                                &mut out_vec,
                                            );
                                        });
                                        Array2::from_shape_vec((1, n_dim), out_vec)
                                            .expect("decode matvec shape build failed")
                                    }
                                    TensorStorageView::BF16(a_view) => {
                                        let a_owned;
                                        let a_vec: &[bf16] = if let Some(s) = a_view.as_slice() {
                                            s
                                        } else {
                                            a_owned = a_view.iter().copied().collect::<Vec<bf16>>();
                                            a_owned.as_slice()
                                        };

                                        let mut out_vec = vec![0.0f32; n_dim];
                                        with_bf16_input_as_f32(a_vec, |a_f32| {
                                            matvec_rowmajor_parallel_f32_i8_matmul(
                                                a_f32,
                                                b_slice,
                                                scale,
                                                n_dim,
                                                k_dim_a,
                                                &mut out_vec,
                                            );
                                        });
                                        Array2::from_shape_vec((1, n_dim), out_vec)
                                            .expect("decode matvec shape build failed")
                                    }
                                }
                            } else {
                                let b_view = b_data.view().into_dyn();
                                match a_view {
                                    TensorStorageView::F32(a_view) => matmul_rows_f32_i8(
                                        a_view, b_view, scale, m_dim, k_dim_a, n_dim,
                                    ),
                                    TensorStorageView::F16(a_view) => matmul_rows_f16_slice(
                                        a_view,
                                        SliceRef::I8(b_slice, scale),
                                        m_dim,
                                        k_dim_a,
                                        n_dim,
                                    ),
                                    TensorStorageView::BF16(a_view) => matmul_rows_bf16_i8(
                                        a_view, b_view, scale, m_dim, k_dim_a, n_dim,
                                    ),
                                }
                            }
                        })
                    }
                }
                TensorStorageOwned::F32(_)
                | TensorStorageOwned::F16(_)
                | TensorStorageOwned::BF16(_) => unreachable!("checked i8 RHS above"),
            };

            let mut out_shape = a_shape.clone();
            let last_idx = out_shape.len() - 1;
            out_shape[last_idx] = n_dim;
            return Tensor::from_f32_data_no_grad_with_device_dtype(
                res_2d.into_shape(out_shape).unwrap().into_dyn(),
                output_dtype,
                output_device,
            );
        }
        let res_2d = a.with_storage_view_preferring(StoragePreference::Native, |a_view| {
            b.with_storage_view_for_input_dtype_and_route(
                input_dtype,
                KernelRouteClass::GenericMatmul,
                |b_view| match (a_view, b_view) {
                    (TensorStorageView::F32(a_view), TensorStorageView::F32(b_view)) => {
                        if m_dim == 1 {
                            let a_owned;
                            let a_vec: &[f32] = if let Some(s) = a_view.as_slice() {
                                s
                            } else {
                                a_owned = a_view.iter().copied().collect::<Vec<f32>>();
                                a_owned.as_slice()
                            };

                            let b_2d = b_view.into_dimensionality::<Ix2>().unwrap();
                            let b_owned;
                            let b_slice: &[f32] = if let Some(s) = b_2d.as_slice() {
                                s
                            } else {
                                b_owned = b_2d.as_standard_layout().to_owned();
                                b_owned
                                    .as_slice()
                                    .expect("standard-layout matmul RHS should be contiguous")
                            };

                            let mut out_vec = vec![0.0f32; n_dim];
                            matvec_rowmajor_parallel(a_vec, b_slice, n_dim, k_dim_a, &mut out_vec);
                            Array2::from_shape_vec((1, n_dim), out_vec)
                                .expect("decode matvec shape build failed")
                        } else {
                            let b_2d = b_view.into_dimensionality::<Ix2>().unwrap();
                            let mut res = Array2::<f32>::zeros((m_dim, n_dim));
                            if let Ok(a_2d_view) = a_view.clone().into_shape((m_dim, k_dim_a)) {
                                general_mat_mul(1.0, &a_2d_view, &b_2d.t(), 0.0, &mut res);
                            } else {
                                let a_2d_owned = a_view
                                    .to_owned()
                                    .into_shape((m_dim, k_dim_a))
                                    .expect("Reshape A failed");
                                general_mat_mul(1.0, &a_2d_owned, &b_2d.t(), 0.0, &mut res);
                            }
                            res
                        }
                    }
                    (TensorStorageView::F32(a_view), TensorStorageView::F16(b_view)) => {
                        matmul_rows_f32_f16(a_view, b_view, m_dim, k_dim_a, n_dim)
                    }
                    (TensorStorageView::F32(a_view), TensorStorageView::BF16(b_view)) => {
                        matmul_rows_f32_bf16(a_view, b_view, m_dim, k_dim_a, n_dim)
                    }
                    (TensorStorageView::F16(a_view), TensorStorageView::F32(b_view)) => {
                        matmul_rows_f16_f32(a_view, b_view, m_dim, k_dim_a, n_dim)
                    }
                    (TensorStorageView::F16(a_view), TensorStorageView::F16(b_view)) => {
                        matmul_rows_f16_f16(a_view, b_view, m_dim, k_dim_a, n_dim)
                    }
                    (TensorStorageView::F16(a_view), TensorStorageView::BF16(b_view)) => {
                        let b_2d = b_view.into_dimensionality::<Ix2>().unwrap();
                        matmul_rows_f16_slice(
                            a_view,
                            SliceRef::BF16(
                                b_2d.as_slice()
                                    .expect("standard-layout matmul RHS should be contiguous"),
                            ),
                            m_dim,
                            k_dim_a,
                            n_dim,
                        )
                    }
                    (TensorStorageView::BF16(a_view), TensorStorageView::F32(b_view)) => {
                        matmul_rows_bf16_f32(a_view, b_view, m_dim, k_dim_a, n_dim)
                    }
                    (TensorStorageView::BF16(a_view), TensorStorageView::F16(b_view)) => {
                        let b_2d = b_view.into_dimensionality::<Ix2>().unwrap();
                        matmul_rows_bf16_slice(
                            a_view,
                            SliceRef::F16(
                                b_2d.as_slice()
                                    .expect("standard-layout matmul RHS should be contiguous"),
                            ),
                            m_dim,
                            k_dim_a,
                            n_dim,
                        )
                    }
                    (TensorStorageView::BF16(a_view), TensorStorageView::BF16(b_view)) => {
                        matmul_rows_bf16_bf16(a_view, b_view, m_dim, k_dim_a, n_dim)
                    }
                },
            )
        });

        let mut out_shape = a_shape.clone();
        let last_idx = out_shape.len() - 1;
        out_shape[last_idx] = n_dim;
        return Tensor::from_f32_data_no_grad_with_device_dtype(
            res_2d.into_shape(out_shape).unwrap().into_dyn(),
            output_dtype,
            output_device,
        );
    }

    let res_2d = if m_dim == 1 {
        let ad = a.data_ref();
        let bd = b.data_ref();

        let a_owned;
        let a_vec: &[f32] = if let Some(s) = ad.as_slice() {
            s
        } else {
            a_owned = ad.iter().copied().collect::<Vec<f32>>();
            a_owned.as_slice()
        };

        let b_2d = bd.view().into_dimensionality::<Ix2>().unwrap();
        let mut out_vec = vec![0.0f32; n_dim];

        let b_owned;
        let b_slice: &[f32] = if let Some(s) = b_2d.as_slice() {
            s
        } else {
            b_owned = b_2d.as_standard_layout().to_owned();
            b_owned
                .as_slice()
                .expect("standard-layout matmul RHS should be contiguous")
        };
        matvec_rowmajor_parallel(a_vec, b_slice, n_dim, k_dim_a, &mut out_vec);

        Array2::from_shape_vec((1, n_dim), out_vec).expect("decode matvec shape build failed")
    } else {
        let ad = a.data_ref();
        let bd = b.data_ref();

        let b_2d = bd.view().into_dimensionality::<Ix2>().unwrap();
        let mut res = Array2::<f32>::zeros((m_dim, n_dim));

        if let Ok(a_2d_view) = ad.view().into_shape((m_dim, k_dim_a)) {
            general_mat_mul(1.0, &a_2d_view, &b_2d.t(), 0.0, &mut res);
        } else {
            let a_2d_owned = ad
                .to_owned()
                .into_shape((m_dim, k_dim_a))
                .expect("Reshape A failed");
            general_mat_mul(1.0, &a_2d_owned, &b_2d.t(), 0.0, &mut res);
        }

        res
    };

    let mut out_shape = a_shape.clone();
    let last_idx = out_shape.len() - 1;
    out_shape[last_idx] = n_dim;

    let result = res_2d.into_shape(out_shape).unwrap().into_dyn();

    let a_clone = a.clone();
    let b_clone = b.clone();

    Tensor(Rc::new(RefCell::new(TensorData {
        data: result.into_shared(),
        f16_data: None,
        bf16_data: None,
        i8_data: None,
        cuda_f32_data: None,
        i8_scale: None,
        has_f32_data: true,
        storage_dtype: crate::precision::DType::F32,
        cache_dirty: false,
        is_parameter: false,
        grad: None,
        cuda_f32_grad: None,
        parents: vec![a_clone.clone(), b_clone.clone()],
        requires_grad: true,
        backward_op: Some(std::rc::Rc::new(move |grad: &ndarray::ArrayViewD<f32>| {
            let g_len = grad.len();
            let g_m = g_len / n_dim;

            let grad_2d = grad
                .view()
                .into_shape((g_m, n_dim))
                .expect("Grad reshape failed: non-contiguous gradient?");

            let (a_data, b_data) = {
                let ad = a_clone.0.borrow();
                let bd = b_clone.0.borrow();
                (ad.data.clone(), bd.data.clone())
            };

            let a_2d_view = a_data.view().into_shape((m_dim, k_dim_a));
            let a_2d_owned;
            let a_2d = match a_2d_view {
                Ok(v) => v,
                Err(_) => {
                    a_2d_owned = a_data.to_owned().into_shape((m_dim, k_dim_a)).unwrap();
                    a_2d_owned.view()
                }
            };

            let b_2d = b_data.view().into_dimensionality::<Ix2>().unwrap();

            let mut da_2d = Array2::<f32>::zeros((m_dim, k_dim_a));
            general_mat_mul(1.0, &grad_2d, &b_2d, 0.0, &mut da_2d);
            a_clone.add_grad(da_2d.into_shape(a_data.shape()).unwrap().into_dyn());

            let mut db_2d = Array2::<f32>::zeros((n_dim, k_dim_a));
            general_mat_mul(1.0, &grad_2d.t(), &a_2d, 0.0, &mut db_2d);
            b_clone.add_grad(db_2d.into_dyn());
        })),
        device: output_device,
    })))
}

// lhs: [B, H, M, K]
// rhs: [B, H, K, N]
// out: [B, H, M, N]
pub fn batch_matmul(lhs: &Tensor, rhs: &Tensor) -> Tensor {
    let output_device = assert_same_device(lhs, rhs, "batch_matmul");
    let build_graph = !is_no_grad() && (lhs.requires_grad() || rhs.requires_grad());
    let cuda_native_supported = output_device == crate::autograd::Device::Cuda;
    assert_native_device_support(output_device, "batch_matmul", cuda_native_supported);

    let lhs_shape = lhs.shape_vec();
    let rhs_shape = rhs.shape_vec();
    assert_eq!(lhs_shape.len(), 4, "batch_matmul lhs must be [B,H,M,K]");
    assert_eq!(rhs_shape.len(), 4, "batch_matmul rhs must be [B,H,K,N]");

    let (b, h, m, k) = (lhs_shape[0], lhs_shape[1], lhs_shape[2], lhs_shape[3]);
    let (b2, h2, k2, n) = (rhs_shape[0], rhs_shape[1], rhs_shape[2], rhs_shape[3]);

    assert_eq!(b, b2, "batch dim mismatch");
    assert_eq!(h, h2, "head dim mismatch");
    assert_eq!(k, k2, "k dim mismatch");

    if build_graph
        && output_device == crate::autograd::Device::Cuda
        && let Some(buffer) = try_cuda_batch_matmul_buffer(lhs, rhs, b, h, m, k, n)
    {
        let lhs_clone = lhs.clone();
        let rhs_clone = rhs.clone();
        let out_shape = vec![b, h, m, n];
        let output_self = Rc::new(RefCell::new(None::<Tensor>));
        let output_self_for_backward = output_self.clone();
        let tensor = Tensor(Rc::new(RefCell::new(TensorData {
            data: ndarray::ArrayD::<f32>::zeros(ndarray::IxDyn(&out_shape)).into_shared(),
            f16_data: None,
            bf16_data: None,
            i8_data: None,
            cuda_f32_data: Some(buffer),
            i8_scale: None,
            has_f32_data: false,
            storage_dtype: crate::precision::DType::F32,
            cache_dirty: false,
            is_parameter: false,
            grad: None,
            cuda_f32_grad: None,
            parents: vec![lhs_clone.clone(), rhs_clone.clone()],
            backward_op: Some(std::rc::Rc::new(move |grad: &ndarray::ArrayViewD<f32>| {
                let cuda_grad = output_self_for_backward
                    .borrow()
                    .as_ref()
                    .and_then(|output| output.cloned_cuda_f32_grad())
                    .filter(|buffer| buffer.len() == grad.len());
                if is_strict_device_execution() {
                    match try_cuda_training_batch_matmul_backward_buffers(
                        grad,
                        cuda_grad.clone(),
                        &lhs_clone,
                        &rhs_clone,
                        b,
                        h,
                        m,
                        k,
                        n,
                    ) {
                        Ok((d_lhs_buf, d_rhs_buf)) => {
                            lhs_clone.add_cuda_grad_buffer_only(d_lhs_buf);
                            rhs_clone.add_cuda_grad_buffer_only(d_rhs_buf);
                            return;
                        }
                        Err(err) => {
                            panic!(
                                "CUDA batch_matmul backward failed in strict device execution mode: {err}"
                            );
                        }
                    }
                }
                let cuda_result = try_cuda_training_batch_matmul_backward(
                    grad, cuda_grad, &lhs_clone, &rhs_clone, b, h, m, k, n,
                );
                match cuda_result {
                    Ok(((d_lhs, d_lhs_buf), (d_rhs, d_rhs_buf))) => {
                        lhs_clone.add_grad_with_cuda_buffer(d_lhs, Some(d_lhs_buf));
                        rhs_clone.add_grad_with_cuda_buffer(d_rhs, Some(d_rhs_buf));
                    }
                    Err(err) => {
                        if is_strict_device_execution() {
                            panic!(
                                "CUDA batch_matmul backward failed in strict device execution mode: {err}"
                            );
                        }
                        let grad_view = grad.view().into_dimensionality::<Ix4>().unwrap();
                        let l_data = lhs_clone.0.borrow().data.clone();
                        let r_data = rhs_clone.0.borrow().data.clone();
                        let l_view_4d = l_data.view().into_dimensionality::<Ix4>().unwrap();
                        let r_view_4d = r_data.view().into_dimensionality::<Ix4>().unwrap();
                        let mut d_lhs = Array4::<f32>::zeros((b, h, m, k));
                        Zip::from(d_lhs.outer_iter_mut())
                            .and(grad_view.outer_iter())
                            .and(r_view_4d.outer_iter())
                            .for_each(|mut d_l_b, g_b, r_b| {
                                Zip::from(d_l_b.outer_iter_mut())
                                    .and(g_b.outer_iter())
                                    .and(r_b.outer_iter())
                                    .for_each(|mut d_l_mat, g_mat, r_mat| {
                                        general_mat_mul(1.0, &g_mat, &r_mat.t(), 0.0, &mut d_l_mat);
                                    });
                            });
                        lhs_clone.add_grad(d_lhs.into_dyn());
                        let mut d_rhs = Array4::<f32>::zeros((b, h, k, n));
                        Zip::from(d_rhs.outer_iter_mut())
                            .and(l_view_4d.outer_iter())
                            .and(grad_view.outer_iter())
                            .for_each(|mut d_r_b, l_b, g_b| {
                                Zip::from(d_r_b.outer_iter_mut())
                                    .and(l_b.outer_iter())
                                    .and(g_b.outer_iter())
                                    .for_each(|mut d_r_mat, l_mat, g_mat| {
                                        general_mat_mul(1.0, &l_mat.t(), &g_mat, 0.0, &mut d_r_mat);
                                    });
                            });
                        rhs_clone.add_grad(d_rhs.into_dyn());
                    }
                }
            })),
            requires_grad: true,
            device: output_device,
        })));
        *output_self.borrow_mut() = Some(tensor.clone());
        return tensor;
    }

    if !build_graph {
        let output_dtype = if lhs.dtype() == rhs.dtype() {
            lhs.dtype()
        } else {
            DType::F32
        };
        if let Some(cuda_out) = try_cuda_batch_matmul(lhs, rhs, b, h, m, k, n, output_dtype) {
            return cuda_out;
        }
        let output_dyn =
            lhs.with_storage_view_preferring(StoragePreference::F32Compute, |lhs_view| {
                rhs.with_storage_view_preferring(StoragePreference::F32Compute, |rhs_view| {
                    let lhs_view = match lhs_view {
                        TensorStorageView::F32(view) => view,
                        TensorStorageView::F16(_) => {
                            unreachable!("f32 compute preference should expose f32 view")
                        }
                        TensorStorageView::BF16(_) => {
                            unreachable!("f32 compute preference should expose f32 view")
                        }
                    };
                    let rhs_view = match rhs_view {
                        TensorStorageView::F32(view) => view,
                        TensorStorageView::F16(_) => {
                            unreachable!("f32 compute preference should expose f32 view")
                        }
                        TensorStorageView::BF16(_) => {
                            unreachable!("f32 compute preference should expose f32 view")
                        }
                    };

                    let lhs_view = lhs_view.into_dimensionality::<Ix4>().unwrap();
                    let rhs_view = rhs_view.into_dimensionality::<Ix4>().unwrap();
                    let mut output = Array4::<f32>::zeros((b, h, m, n));

                    Zip::from(output.outer_iter_mut())
                        .and(lhs_view.outer_iter())
                        .and(rhs_view.outer_iter())
                        .for_each(|mut out_batch, lhs_batch, rhs_batch| {
                            Zip::from(out_batch.outer_iter_mut())
                                .and(lhs_batch.outer_iter())
                                .and(rhs_batch.outer_iter())
                                .for_each(|mut out_mat, lhs_mat, rhs_mat| {
                                    general_mat_mul(1.0, &lhs_mat, &rhs_mat, 0.0, &mut out_mat);
                                });
                        });

                    output.into_dyn()
                })
            });

        return Tensor::from_f32_data_no_grad_with_device_dtype(
            output_dyn,
            output_dtype,
            output_device,
        );
    }

    let lhs_ref = lhs.data_ref();
    let rhs_ref = rhs.data_ref();

    let lhs_view = lhs_ref.view().into_dimensionality::<Ix4>().unwrap();
    let rhs_view = rhs_ref.view().into_dimensionality::<Ix4>().unwrap();

    let mut output = Array4::<f32>::zeros((b, h, m, n));

    Zip::from(output.outer_iter_mut())
        .and(lhs_view.outer_iter())
        .and(rhs_view.outer_iter())
        .for_each(|mut out_batch, lhs_batch, rhs_batch| {
            Zip::from(out_batch.outer_iter_mut())
                .and(lhs_batch.outer_iter())
                .and(rhs_batch.outer_iter())
                .for_each(|mut out_mat, lhs_mat, rhs_mat| {
                    general_mat_mul(1.0, &lhs_mat, &rhs_mat, 0.0, &mut out_mat);
                });
        });

    let output_dyn = output.into_dyn();

    let lhs_clone = lhs.clone();
    let rhs_clone = rhs.clone();

    Tensor(Rc::new(RefCell::new(TensorData {
        data: output_dyn.into_shared(),
        f16_data: None,
        bf16_data: None,
        i8_data: None,
        cuda_f32_data: None,
        i8_scale: None,
        has_f32_data: true,
        storage_dtype: crate::precision::DType::F32,
        cache_dirty: false,
        is_parameter: false,
        grad: None,
        cuda_f32_grad: None,
        parents: vec![lhs_clone.clone(), rhs_clone.clone()],
        backward_op: Some(std::rc::Rc::new(move |grad: &ndarray::ArrayViewD<f32>| {
            let grad_view = grad.view().into_dimensionality::<Ix4>().unwrap();
            let l_data = lhs_clone.0.borrow().data.clone();
            let r_data = rhs_clone.0.borrow().data.clone();

            let l_view_4d = l_data.view().into_dimensionality::<Ix4>().unwrap();
            let r_view_4d = r_data.view().into_dimensionality::<Ix4>().unwrap();

            let mut d_lhs = Array4::<f32>::zeros((b, h, m, k));
            Zip::from(d_lhs.outer_iter_mut())
                .and(grad_view.outer_iter())
                .and(r_view_4d.outer_iter())
                .for_each(|mut d_l_b, g_b, r_b| {
                    Zip::from(d_l_b.outer_iter_mut())
                        .and(g_b.outer_iter())
                        .and(r_b.outer_iter())
                        .for_each(|mut d_l_mat, g_mat, r_mat| {
                            general_mat_mul(1.0, &g_mat, &r_mat.t(), 0.0, &mut d_l_mat);
                        });
                });
            lhs_clone.add_grad(d_lhs.into_dyn());

            let mut d_rhs = Array4::<f32>::zeros((b, h, k, n));
            Zip::from(d_rhs.outer_iter_mut())
                .and(l_view_4d.outer_iter())
                .and(grad_view.outer_iter())
                .for_each(|mut d_r_b, l_b, g_b| {
                    Zip::from(d_r_b.outer_iter_mut())
                        .and(l_b.outer_iter())
                        .and(g_b.outer_iter())
                        .for_each(|mut d_r_mat, l_mat, g_mat| {
                            general_mat_mul(1.0, &l_mat.t(), &g_mat, 0.0, &mut d_r_mat);
                        });
                });
            rhs_clone.add_grad(d_rhs.into_dyn());
        })),
        requires_grad: true,
        device: output_device,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autograd::no_grad;
    #[cfg(feature = "cuda")]
    use crate::autograd::set_strict_device_execution;
    #[cfg(feature = "cuda")]
    use crate::ops::arithmetic::sum;
    use crate::precision::{PrecisionConfig, with_precision_config};
    #[cfg(feature = "cuda")]
    use ndarray::{Array, IxDyn};

    fn sample_f32(len: usize) -> Vec<f32> {
        (0..len)
            .map(|i| (((i * 17 + 11) % 29) as f32) / 13.0 - 1.0)
            .collect()
    }

    fn to_bf16(src: &[f32]) -> Vec<bf16> {
        src.iter().map(|&v| bf16::from_f32(v)).collect()
    }

    fn to_f16(src: &[f32]) -> Vec<f16> {
        src.iter().map(|&v| f16::from_f32(v)).collect()
    }

    fn bf16_to_f32(src: &[bf16]) -> Vec<f32> {
        src.iter().map(|&v| v.to_f32()).collect()
    }

    #[cfg(feature = "cuda")]
    fn make_grad_tensor(shape: &[usize], data: Vec<f32>) -> Tensor {
        Tensor::from_data_with_grad_flag(
            Array::from_shape_vec(IxDyn(shape), data)
                .expect("test tensor shape mismatch")
                .into_dyn(),
            true,
        )
    }

    fn f16_to_f32(src: &[f16]) -> Vec<f32> {
        src.iter().map(|&v| v.to_f32()).collect()
    }

    fn i8_storage(src: &[f32]) -> (Vec<f32>, Vec<i8>, f32) {
        let tensor = make_tensor(&[src.len()], src.to_vec(), DType::I8);
        match tensor.native_storage_owned() {
            TensorStorageOwned::I8(data, scale) => (
                tensor.data_ref().iter().copied().collect(),
                data.iter().copied().collect(),
                scale,
            ),
            TensorStorageOwned::F32(_)
            | TensorStorageOwned::F16(_)
            | TensorStorageOwned::BF16(_) => {
                panic!("expected i8 storage")
            }
        }
    }

    fn assert_close(lhs: &[f32], rhs: &[f32], tol: f32) {
        assert_eq!(lhs.len(), rhs.len());
        for (idx, (&a, &b)) in lhs.iter().zip(rhs.iter()).enumerate() {
            assert!(
                (a - b).abs() <= tol,
                "mismatch at {idx}: lhs={a}, rhs={b}, tol={tol}"
            );
        }
    }

    #[test]
    fn bf16_input_matvec_matches_quantized_reference() {
        let k_dim = 11usize;
        let n_rows = 7usize;
        let x = sample_f32(k_dim);
        let x_bf16 = to_bf16(&x);
        let w = sample_f32(n_rows * k_dim);
        let w_bf16 = to_bf16(&w);

        let mut ref_out = vec![0.0f32; n_rows];
        let mut out = vec![0.0f32; n_rows];

        let x_q = bf16_to_f32(&x_bf16);
        matvec_rowmajor_parallel(&x_q, &w, n_rows, k_dim, &mut ref_out);
        matvec_rowmajor_parallel_mixed(
            SliceRef::BF16(&x_bf16),
            SliceRef::F32(&w),
            n_rows,
            k_dim,
            &mut out,
        );
        assert_close(&ref_out, &out, 1e-5);

        let w_q = bf16_to_f32(&w_bf16);
        matvec_rowmajor_parallel(&x_q, &w_q, n_rows, k_dim, &mut ref_out);
        matvec_rowmajor_parallel_mixed(
            SliceRef::BF16(&x_bf16),
            SliceRef::BF16(&w_bf16),
            n_rows,
            k_dim,
            &mut out,
        );
        assert_close(&ref_out, &out, 1e-5);
    }

    #[test]
    fn bf16_input_dual_matvec_matches_quantized_reference() {
        let k_dim = 13usize;
        let n_rows = 5usize;
        let x = sample_f32(k_dim);
        let x_bf16 = to_bf16(&x);
        let w0 = sample_f32(n_rows * k_dim);
        let w1 = sample_f32(n_rows * k_dim)
            .into_iter()
            .map(|v| v * 0.7 - 0.1)
            .collect::<Vec<_>>();
        let w0_bf16 = to_bf16(&w0);
        let w1_bf16 = to_bf16(&w1);
        let x_q = bf16_to_f32(&x_bf16);

        let mut ref0 = vec![0.0f32; n_rows];
        let mut ref1 = vec![0.0f32; n_rows];
        let mut out0 = vec![0.0f32; n_rows];
        let mut out1 = vec![0.0f32; n_rows];

        dual_matvec_rowmajor_parallel(&x_q, &w0, &w1, n_rows, k_dim, &mut ref0, &mut ref1);
        dual_matvec_rowmajor_parallel_mixed(
            SliceRef::BF16(&x_bf16),
            SliceRef::F32(&w0),
            SliceRef::F32(&w1),
            n_rows,
            k_dim,
            &mut out0,
            &mut out1,
        );
        assert_close(&ref0, &out0, 1e-5);
        assert_close(&ref1, &out1, 1e-5);

        let w0_q = bf16_to_f32(&w0_bf16);
        let w1_q = bf16_to_f32(&w1_bf16);
        dual_matvec_rowmajor_parallel(&x_q, &w0_q, &w1_q, n_rows, k_dim, &mut ref0, &mut ref1);
        dual_matvec_rowmajor_parallel_mixed(
            SliceRef::BF16(&x_bf16),
            SliceRef::BF16(&w0_bf16),
            SliceRef::BF16(&w1_bf16),
            n_rows,
            k_dim,
            &mut out0,
            &mut out1,
        );
        assert_close(&ref0, &out0, 1e-5);
        assert_close(&ref1, &out1, 1e-5);
    }

    #[test]
    fn bf16_input_silu_matches_quantized_reference() {
        let k_dim = 9usize;
        let n_rows = 6usize;
        let x = sample_f32(k_dim);
        let x_bf16 = to_bf16(&x);
        let gate = sample_f32(n_rows * k_dim);
        let up = sample_f32(n_rows * k_dim)
            .into_iter()
            .map(|v| v * -0.5 + 0.2)
            .collect::<Vec<_>>();
        let gate_bf16 = to_bf16(&gate);
        let up_bf16 = to_bf16(&up);
        let x_q = bf16_to_f32(&x_bf16);

        let mut ref_out = vec![0.0f32; n_rows];
        let mut out = vec![0.0f32; n_rows];

        dual_matvec_silu_mul_rowmajor_parallel(&x_q, &gate, &up, n_rows, k_dim, &mut ref_out);
        dual_matvec_silu_mul_rowmajor_parallel_mixed(
            SliceRef::BF16(&x_bf16),
            SliceRef::F32(&gate),
            SliceRef::F32(&up),
            n_rows,
            k_dim,
            &mut out,
        );
        assert_close(&ref_out, &out, 1e-5);

        let gate_q = bf16_to_f32(&gate_bf16);
        let up_q = bf16_to_f32(&up_bf16);
        dual_matvec_silu_mul_rowmajor_parallel(&x_q, &gate_q, &up_q, n_rows, k_dim, &mut ref_out);
        dual_matvec_silu_mul_rowmajor_parallel_mixed(
            SliceRef::BF16(&x_bf16),
            SliceRef::BF16(&gate_bf16),
            SliceRef::BF16(&up_bf16),
            n_rows,
            k_dim,
            &mut out,
        );
        assert_close(&ref_out, &out, 1e-5);
    }

    #[test]
    fn bf16_input_argmax_matches_quantized_reference() {
        let k_dim = 10usize;
        let n_rows = 12usize;
        let x = sample_f32(k_dim);
        let x_bf16 = to_bf16(&x);
        let w = sample_f32(n_rows * k_dim);
        let w_bf16 = to_bf16(&w);
        let x_q = bf16_to_f32(&x_bf16);
        let w_q = bf16_to_f32(&w_bf16);

        let idx_f32 = matvec_argmax_rowmajor_parallel(&x_q, &w, n_rows, k_dim);
        let idx_bf16f32 = matvec_argmax_rowmajor_parallel_mixed(
            SliceRef::BF16(&x_bf16),
            SliceRef::F32(&w),
            n_rows,
            k_dim,
        );
        assert_eq!(idx_f32, idx_bf16f32);

        let idx_q = matvec_argmax_rowmajor_parallel(&x_q, &w_q, n_rows, k_dim);
        let idx_bf16bf16 = matvec_argmax_rowmajor_parallel_mixed(
            SliceRef::BF16(&x_bf16),
            SliceRef::BF16(&w_bf16),
            n_rows,
            k_dim,
        );
        assert_eq!(idx_q, idx_bf16bf16);
    }

    #[test]
    fn f16_input_matvec_matches_quantized_reference() {
        let k_dim = 11usize;
        let n_rows = 7usize;
        let x = sample_f32(k_dim);
        let x_f16 = to_f16(&x);
        let w = sample_f32(n_rows * k_dim);
        let w_f16 = to_f16(&w);

        let mut ref_out = vec![0.0f32; n_rows];
        let mut out = vec![0.0f32; n_rows];

        let x_q = f16_to_f32(&x_f16);
        matvec_rowmajor_parallel(&x_q, &w, n_rows, k_dim, &mut ref_out);
        matvec_rowmajor_parallel_mixed(
            SliceRef::F16(&x_f16),
            SliceRef::F32(&w),
            n_rows,
            k_dim,
            &mut out,
        );
        assert_close(&ref_out, &out, 1e-5);

        let w_q = f16_to_f32(&w_f16);
        matvec_rowmajor_parallel(&x_q, &w_q, n_rows, k_dim, &mut ref_out);
        matvec_rowmajor_parallel_mixed(
            SliceRef::F16(&x_f16),
            SliceRef::F16(&w_f16),
            n_rows,
            k_dim,
            &mut out,
        );
        assert_close(&ref_out, &out, 1e-5);
    }

    #[test]
    fn f16_input_dual_matvec_matches_quantized_reference() {
        let k_dim = 13usize;
        let n_rows = 5usize;
        let x = sample_f32(k_dim);
        let x_f16 = to_f16(&x);
        let w0 = sample_f32(n_rows * k_dim);
        let w1 = sample_f32(n_rows * k_dim)
            .into_iter()
            .map(|v| v * 0.7 - 0.1)
            .collect::<Vec<_>>();
        let w0_f16 = to_f16(&w0);
        let w1_f16 = to_f16(&w1);
        let x_q = f16_to_f32(&x_f16);

        let mut ref0 = vec![0.0f32; n_rows];
        let mut ref1 = vec![0.0f32; n_rows];
        let mut out0 = vec![0.0f32; n_rows];
        let mut out1 = vec![0.0f32; n_rows];

        dual_matvec_rowmajor_parallel(&x_q, &w0, &w1, n_rows, k_dim, &mut ref0, &mut ref1);
        dual_matvec_rowmajor_parallel_mixed(
            SliceRef::F16(&x_f16),
            SliceRef::F32(&w0),
            SliceRef::F32(&w1),
            n_rows,
            k_dim,
            &mut out0,
            &mut out1,
        );
        assert_close(&ref0, &out0, 1e-5);
        assert_close(&ref1, &out1, 1e-5);

        let w0_q = f16_to_f32(&w0_f16);
        let w1_q = f16_to_f32(&w1_f16);
        dual_matvec_rowmajor_parallel(&x_q, &w0_q, &w1_q, n_rows, k_dim, &mut ref0, &mut ref1);
        dual_matvec_rowmajor_parallel_mixed(
            SliceRef::F16(&x_f16),
            SliceRef::F16(&w0_f16),
            SliceRef::F16(&w1_f16),
            n_rows,
            k_dim,
            &mut out0,
            &mut out1,
        );
        assert_close(&ref0, &out0, 1e-5);
        assert_close(&ref1, &out1, 1e-5);
    }

    #[test]
    fn f16_input_silu_matches_quantized_reference() {
        let k_dim = 9usize;
        let n_rows = 6usize;
        let x = sample_f32(k_dim);
        let x_f16 = to_f16(&x);
        let gate = sample_f32(n_rows * k_dim);
        let up = sample_f32(n_rows * k_dim)
            .into_iter()
            .map(|v| v * 0.5 + 0.2)
            .collect::<Vec<_>>();
        let gate_f16 = to_f16(&gate);
        let up_f16 = to_f16(&up);
        let x_q = f16_to_f32(&x_f16);

        let mut ref_out = vec![0.0f32; n_rows];
        let mut out = vec![0.0f32; n_rows];

        dual_matvec_silu_mul_rowmajor_parallel(&x_q, &gate, &up, n_rows, k_dim, &mut ref_out);
        dual_matvec_silu_mul_rowmajor_parallel_mixed(
            SliceRef::F16(&x_f16),
            SliceRef::F32(&gate),
            SliceRef::F32(&up),
            n_rows,
            k_dim,
            &mut out,
        );
        assert_close(&ref_out, &out, 1e-5);

        let gate_q = f16_to_f32(&gate_f16);
        let up_q = f16_to_f32(&up_f16);
        dual_matvec_silu_mul_rowmajor_parallel(&x_q, &gate_q, &up_q, n_rows, k_dim, &mut ref_out);
        dual_matvec_silu_mul_rowmajor_parallel_mixed(
            SliceRef::F16(&x_f16),
            SliceRef::F16(&gate_f16),
            SliceRef::F16(&up_f16),
            n_rows,
            k_dim,
            &mut out,
        );
        assert_close(&ref_out, &out, 1e-5);
    }

    #[test]
    fn f16_input_argmax_matches_quantized_reference() {
        let k_dim = 11usize;
        let n_rows = 7usize;
        let x = sample_f32(k_dim);
        let x_f16 = to_f16(&x);
        let w = sample_f32(n_rows * k_dim);
        let w_f16 = to_f16(&w);
        let x_q = f16_to_f32(&x_f16);
        let w_q = f16_to_f32(&w_f16);

        let idx_f32 = matvec_argmax_rowmajor_parallel(&x_q, &w, n_rows, k_dim);
        let idx_f16f32 = matvec_argmax_rowmajor_parallel_mixed(
            SliceRef::F16(&x_f16),
            SliceRef::F32(&w),
            n_rows,
            k_dim,
        );
        assert_eq!(idx_f32, idx_f16f32);

        let idx_q = matvec_argmax_rowmajor_parallel(&x_q, &w_q, n_rows, k_dim);
        let idx_f16f16 = matvec_argmax_rowmajor_parallel_mixed(
            SliceRef::F16(&x_f16),
            SliceRef::F16(&w_f16),
            n_rows,
            k_dim,
        );
        assert_eq!(idx_q, idx_f16f16);
    }

    #[test]
    fn i8_weight_matvec_matches_quantized_reference() {
        let k_dim = 11usize;
        let n_rows = 7usize;
        let x = sample_f32(k_dim);
        let w = sample_f32(n_rows * k_dim);
        let (w_q, w_i8, w_scale) = i8_storage(&w);

        let mut ref_out = vec![0.0f32; n_rows];
        let mut out = vec![0.0f32; n_rows];

        matvec_rowmajor_parallel(&x, &w_q, n_rows, k_dim, &mut ref_out);
        matvec_rowmajor_parallel_mixed(
            SliceRef::F32(&x),
            SliceRef::I8(&w_i8, w_scale),
            n_rows,
            k_dim,
            &mut out,
        );
        assert_close(&ref_out, &out, 1e-5);
    }

    #[test]
    fn i8_weight_dual_matvec_matches_quantized_reference() {
        let k_dim = 13usize;
        let n_rows = 5usize;
        let x = sample_f32(k_dim);
        let w0 = sample_f32(n_rows * k_dim);
        let w1 = sample_f32(n_rows * k_dim)
            .into_iter()
            .map(|v| v * 0.7 - 0.1)
            .collect::<Vec<_>>();
        let (w0_q, w0_i8, w0_scale) = i8_storage(&w0);
        let (w1_q, w1_i8, w1_scale) = i8_storage(&w1);

        let mut ref0 = vec![0.0f32; n_rows];
        let mut ref1 = vec![0.0f32; n_rows];
        let mut out0 = vec![0.0f32; n_rows];
        let mut out1 = vec![0.0f32; n_rows];

        dual_matvec_rowmajor_parallel(&x, &w0_q, &w1_q, n_rows, k_dim, &mut ref0, &mut ref1);
        dual_matvec_rowmajor_parallel_mixed(
            SliceRef::F32(&x),
            SliceRef::I8(&w0_i8, w0_scale),
            SliceRef::I8(&w1_i8, w1_scale),
            n_rows,
            k_dim,
            &mut out0,
            &mut out1,
        );
        assert_close(&ref0, &out0, 1e-5);
        assert_close(&ref1, &out1, 1e-5);
    }

    #[test]
    fn i8_weight_silu_matches_quantized_reference() {
        let k_dim = 9usize;
        let n_rows = 6usize;
        let x = sample_f32(k_dim);
        let gate = sample_f32(n_rows * k_dim);
        let up = sample_f32(n_rows * k_dim)
            .into_iter()
            .map(|v| v * -0.5 + 0.2)
            .collect::<Vec<_>>();
        let (gate_q, gate_i8, gate_scale) = i8_storage(&gate);
        let (up_q, up_i8, up_scale) = i8_storage(&up);

        let mut ref_out = vec![0.0f32; n_rows];
        let mut out = vec![0.0f32; n_rows];

        dual_matvec_silu_mul_rowmajor_parallel(&x, &gate_q, &up_q, n_rows, k_dim, &mut ref_out);
        dual_matvec_silu_mul_rowmajor_parallel_mixed(
            SliceRef::F32(&x),
            SliceRef::I8(&gate_i8, gate_scale),
            SliceRef::I8(&up_i8, up_scale),
            n_rows,
            k_dim,
            &mut out,
        );
        assert_close(&ref_out, &out, 1e-5);
    }

    #[test]
    fn i8_weight_argmax_matches_quantized_reference() {
        let k_dim = 10usize;
        let n_rows = 12usize;
        let x = sample_f32(k_dim);
        let w = sample_f32(n_rows * k_dim);
        let (w_q, w_i8, w_scale) = i8_storage(&w);

        let idx_q = matvec_argmax_rowmajor_parallel(&x, &w_q, n_rows, k_dim);
        let idx_i8 = matvec_argmax_rowmajor_parallel_mixed(
            SliceRef::F32(&x),
            SliceRef::I8(&w_i8, w_scale),
            n_rows,
            k_dim,
        );
        assert_eq!(idx_q, idx_i8);
    }

    #[test]
    fn nested_bf16_input_conversion_is_reentrant() {
        let lhs = to_bf16(&sample_f32(5));
        let rhs = to_bf16(&sample_f32(3));
        let lhs_q = bf16_to_f32(&lhs);
        let rhs_q = bf16_to_f32(&rhs);

        with_bf16_input_as_f32(&lhs, |lhs_f32| {
            assert_close(lhs_f32, &lhs_q, 1e-6);
            with_bf16_input_as_f32(&rhs, |rhs_f32| {
                assert_close(rhs_f32, &rhs_q, 1e-6);
            });
        });
    }

    fn make_tensor(shape: &[usize], data: Vec<f32>, dtype: DType) -> Tensor {
        let t = Tensor::from_array_no_grad(
            ndarray::Array::from_shape_vec(ndarray::IxDyn(shape), data)
                .expect("test tensor shape mismatch")
                .into_dyn(),
        );
        t.cast_inplace(dtype);
        t
    }

    #[test]
    fn matmul_no_grad_preserves_bf16_output_dtype() {
        let a = make_tensor(&[1, 4], vec![1.0, -2.0, 0.5, 3.0], DType::BF16);
        let b = make_tensor(
            &[3, 4],
            vec![
                1.0, 0.0, -1.0, 2.0, 0.5, 1.5, -0.5, 0.25, -1.0, 2.0, 1.0, -0.5,
            ],
            DType::BF16,
        );

        let ref_out = no_grad(|| {
            matmul(
                &make_tensor(&[1, 4], vec![1.0, -2.0, 0.5, 3.0], DType::F32),
                &make_tensor(
                    &[3, 4],
                    vec![
                        1.0, 0.0, -1.0, 2.0, 0.5, 1.5, -0.5, 0.25, -1.0, 2.0, 1.0, -0.5,
                    ],
                    DType::F32,
                ),
            )
        });
        let out = no_grad(|| matmul(&a, &b));

        assert_eq!(a.dtype(), DType::BF16);
        assert_eq!(b.dtype(), DType::BF16);
        assert_eq!(out.dtype(), DType::BF16);

        let ref_vals = ref_out
            .data_ref()
            .iter()
            .map(|&v| bf16::from_f32(v).to_f32())
            .collect::<Vec<_>>();
        out.with_storage_view(|view| match view {
            TensorStorageView::BF16(view) => {
                let vals = view.iter().map(|v| v.to_f32()).collect::<Vec<_>>();
                assert_eq!(vals, ref_vals);
            }
            TensorStorageView::F16(_) => panic!("bf16 matmul output should stay bf16 in no-grad"),
            TensorStorageView::F32(_) => panic!("bf16 matmul output should stay bf16 in no-grad"),
        });
    }

    #[test]
    fn matmul_same_dtype_bf16_parameter_may_materialize_cached_f32_for_generic_gemm() {
        with_precision_config(
            PrecisionConfig {
                parameter_dtype: DType::BF16,
                runtime_dtype: DType::F32,
                allow_parameter_dtype_copies: true,
            },
            || {
                let a = make_tensor(&[1, 4], vec![1.0, -2.0, 0.5, 3.0], DType::BF16);
                let b = Tensor::parameter_with_dtype(
                    ndarray::Array::from_shape_vec(
                        ndarray::IxDyn(&[3, 4]),
                        vec![
                            1.0, 0.0, -1.0, 2.0, 0.5, 1.5, -0.5, 0.25, -1.0, 2.0, 1.0, -0.5,
                        ],
                    )
                    .expect("parameter shape mismatch")
                    .into_dyn(),
                    DType::BF16,
                );

                {
                    let inner = b.0.borrow();
                    assert!(
                        !inner.has_f32_data,
                        "bf16 parameter should start without cached f32 copy"
                    );
                }

                let out = no_grad(|| matmul(&a, &b));
                assert_eq!(out.dtype(), DType::BF16);

                let inner = b.0.borrow();
                assert!(
                    inner.has_f32_data,
                    "generic bf16 matmul is currently expected to materialize cached f32 parameter storage"
                );
            },
        );
    }

    #[test]
    fn matmul_mixed_f32_input_is_allowed_to_materialize_parameter_cache() {
        with_precision_config(
            PrecisionConfig {
                parameter_dtype: DType::BF16,
                runtime_dtype: DType::F32,
                allow_parameter_dtype_copies: true,
            },
            || {
                let a = make_tensor(&[1, 4], vec![1.0, -2.0, 0.5, 3.0], DType::F32);
                let b = Tensor::parameter_with_dtype(
                    ndarray::Array::from_shape_vec(
                        ndarray::IxDyn(&[3, 4]),
                        vec![
                            1.0, 0.0, -1.0, 2.0, 0.5, 1.5, -0.5, 0.25, -1.0, 2.0, 1.0, -0.5,
                        ],
                    )
                    .expect("parameter shape mismatch")
                    .into_dyn(),
                    DType::BF16,
                );

                {
                    let inner = b.0.borrow();
                    assert!(
                        !inner.has_f32_data,
                        "bf16 parameter should start without cached f32 copy"
                    );
                }

                let out = no_grad(|| matmul(&a, &b));
                assert_eq!(out.dtype(), DType::F32);

                let inner = b.0.borrow();
                assert!(
                    inner.has_f32_data,
                    "mixed f32 input should still be allowed to materialize cached f32 parameter storage"
                );
            },
        );
    }

    #[test]
    fn batch_matmul_no_grad_preserves_bf16_output_dtype() {
        let lhs = make_tensor(
            &[1, 1, 2, 3],
            vec![1.0, -2.0, 0.5, 3.0, -1.0, 2.0],
            DType::BF16,
        );
        let rhs = make_tensor(
            &[1, 1, 3, 2],
            vec![0.5, 1.0, -1.5, 2.0, 0.25, -0.75],
            DType::BF16,
        );

        let ref_out = no_grad(|| {
            batch_matmul(
                &make_tensor(
                    &[1, 1, 2, 3],
                    vec![1.0, -2.0, 0.5, 3.0, -1.0, 2.0],
                    DType::F32,
                ),
                &make_tensor(
                    &[1, 1, 3, 2],
                    vec![0.5, 1.0, -1.5, 2.0, 0.25, -0.75],
                    DType::F32,
                ),
            )
        });
        let out = no_grad(|| batch_matmul(&lhs, &rhs));

        assert_eq!(lhs.dtype(), DType::BF16);
        assert_eq!(rhs.dtype(), DType::BF16);
        assert_eq!(out.dtype(), DType::BF16);

        let ref_vals = ref_out
            .data_ref()
            .iter()
            .map(|&v| bf16::from_f32(v).to_f32())
            .collect::<Vec<_>>();
        out.with_storage_view(|view| match view {
            TensorStorageView::BF16(view) => {
                let vals = view.iter().map(|v| v.to_f32()).collect::<Vec<_>>();
                assert_eq!(vals, ref_vals);
            }
            TensorStorageView::F16(_) => {
                panic!("bf16 batch_matmul output should stay bf16 in no-grad")
            }
            TensorStorageView::F32(_) => {
                panic!("bf16 batch_matmul output should stay bf16 in no-grad")
            }
        });
    }

    #[test]
    fn matmul_no_grad_preserves_f16_output_dtype() {
        let a = make_tensor(&[1, 4], vec![1.0, -2.0, 0.5, 3.0], DType::F16);
        let b = make_tensor(
            &[3, 4],
            vec![
                1.0, 0.0, -1.0, 2.0, 0.5, 1.5, -0.5, 0.25, -1.0, 2.0, 1.0, -0.5,
            ],
            DType::F16,
        );

        let ref_out = no_grad(|| {
            matmul(
                &make_tensor(&[1, 4], vec![1.0, -2.0, 0.5, 3.0], DType::F32),
                &make_tensor(
                    &[3, 4],
                    vec![
                        1.0, 0.0, -1.0, 2.0, 0.5, 1.5, -0.5, 0.25, -1.0, 2.0, 1.0, -0.5,
                    ],
                    DType::F32,
                ),
            )
        });
        let out = no_grad(|| matmul(&a, &b));

        assert_eq!(a.dtype(), DType::F16);
        assert_eq!(b.dtype(), DType::F16);
        assert_eq!(out.dtype(), DType::F16);

        let ref_vals = ref_out
            .data_ref()
            .iter()
            .map(|&v| f16::from_f32(v).to_f32())
            .collect::<Vec<_>>();
        out.with_storage_view(|view| match view {
            TensorStorageView::F16(view) => {
                let vals = view.iter().map(|v| v.to_f32()).collect::<Vec<_>>();
                assert_eq!(vals, ref_vals);
            }
            TensorStorageView::BF16(_) => panic!("f16 matmul output should stay f16 in no-grad"),
            TensorStorageView::F32(_) => panic!("f16 matmul output should stay f16 in no-grad"),
        });
    }

    #[test]
    fn matmul_no_grad_preserves_i8_output_dtype() {
        let a = make_tensor(&[1, 4], vec![1.0, -2.0, 0.5, 3.0], DType::I8);
        let b = make_tensor(
            &[3, 4],
            vec![
                1.0, 0.0, -1.0, 2.0, 0.5, 1.5, -0.5, 0.25, -1.0, 2.0, 1.0, -0.5,
            ],
            DType::I8,
        );

        let ref_a_q = make_tensor(&[1, 4], vec![1.0, -2.0, 0.5, 3.0], DType::I8);
        let ref_b_q = make_tensor(
            &[3, 4],
            vec![
                1.0, 0.0, -1.0, 2.0, 0.5, 1.5, -0.5, 0.25, -1.0, 2.0, 1.0, -0.5,
            ],
            DType::I8,
        );
        let a_q = ref_a_q.data_ref().iter().copied().collect::<Vec<_>>();
        let b_q = ref_b_q.data_ref().iter().copied().collect::<Vec<_>>();
        let ref_out = no_grad(|| {
            matmul(
                &make_tensor(&[1, 4], a_q, DType::F32),
                &make_tensor(&[3, 4], b_q, DType::F32),
            )
        });
        let out = no_grad(|| matmul(&a, &b));

        assert_eq!(out.dtype(), DType::I8);

        let expected = make_tensor(
            &[1, 3],
            ref_out.data_ref().iter().copied().collect(),
            DType::I8,
        );
        let ref_vals = expected.data_ref().iter().copied().collect::<Vec<_>>();
        let out_vals = out.data_ref().iter().copied().collect::<Vec<_>>();
        assert_eq!(out_vals.len(), ref_vals.len());
        for (actual, expected) in out_vals.iter().zip(ref_vals.iter()) {
            assert!(
                (actual - expected).abs() < 1e-5,
                "i8 matmul output drifted: actual={actual}, expected={expected}"
            );
        }
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_matmul_matches_cpu_reference() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        crate::ops::cuda::set_enabled(true);
        let a = make_tensor(&[8, 32], sample_f32(8 * 32), DType::F32);
        let b = make_tensor(&[6, 32], sample_f32(6 * 32), DType::F32);

        let out = no_grad(|| matmul(&a, &b));
        crate::ops::cuda::set_enabled(false);
        let reference = no_grad(|| matmul(&a, &b));

        let out_vals = out.data_ref().iter().copied().collect::<Vec<_>>();
        let ref_vals = reference.data_ref().iter().copied().collect::<Vec<_>>();
        assert_eq!(out_vals.len(), ref_vals.len());
        for (got, expect) in out_vals.iter().zip(ref_vals.iter()) {
            assert!((got - expect).abs() < 1e-3, "got {got}, expect {expect}");
        }
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_batch_matmul_matches_cpu_reference() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        crate::ops::cuda::set_enabled(true);
        let lhs = make_tensor(&[2, 3, 4, 16], sample_f32(2 * 3 * 4 * 16), DType::F32);
        let rhs = make_tensor(&[2, 3, 16, 5], sample_f32(2 * 3 * 16 * 5), DType::F32);

        let out = no_grad(|| batch_matmul(&lhs, &rhs));
        crate::ops::cuda::set_enabled(false);
        let reference = no_grad(|| batch_matmul(&lhs, &rhs));

        let out_vals = out.data_ref().iter().copied().collect::<Vec<_>>();
        let ref_vals = reference.data_ref().iter().copied().collect::<Vec<_>>();
        assert_eq!(out_vals.len(), ref_vals.len());
        for (got, expect) in out_vals.iter().zip(ref_vals.iter()) {
            assert!((got - expect).abs() < 1e-3, "got {got}, expect {expect}");
        }
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_matmul_preserves_bf16_dtype_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let a = make_tensor(&[1, 4], vec![1.0, -2.0, 0.5, 3.0], DType::BF16).to_cuda();
        let b = make_tensor(
            &[3, 4],
            vec![
                1.0, 0.0, -1.0, 2.0, 0.5, 1.5, -0.5, 0.25, -1.0, 2.0, 1.0, -0.5,
            ],
            DType::BF16,
        )
        .to_cuda();

        crate::ops::cuda::set_enabled(true);
        crate::autograd::set_strict_device_execution(true);
        let out = no_grad(|| matmul(&a, &b));
        crate::autograd::set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);

        let reference = no_grad(|| matmul(&a.to_cpu(), &b.to_cpu()));
        assert!(out.is_cuda());
        assert_eq!(out.dtype(), DType::BF16);
        assert!(
            !out.has_host_f32_data(),
            "bf16 CUDA matmul should keep output resident until host data is requested"
        );
        for (got, expect) in out.data_ref().iter().zip(reference.data_ref().iter()) {
            assert!((got - expect).abs() < 2e-2, "got {got}, expect {expect}");
        }
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_bf16_matmul_native_view_materializes_cuda_values() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let a = make_tensor(&[1, 4], vec![1.0, -2.0, 0.5, 3.0], DType::BF16).to_cuda();
        let b = make_tensor(
            &[3, 4],
            vec![
                1.0, 0.0, -1.0, 2.0, 0.5, 1.5, -0.5, 0.25, -1.0, 2.0, 1.0, -0.5,
            ],
            DType::BF16,
        )
        .to_cuda();

        crate::ops::cuda::set_enabled(true);
        crate::autograd::set_strict_device_execution(true);
        let out = no_grad(|| matmul(&a, &b));
        crate::autograd::set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);

        assert!(out.is_cuda());
        assert_eq!(out.dtype(), DType::BF16);
        assert!(!out.has_host_f32_data());

        out.with_storage_view(|view| match view {
            TensorStorageView::F32(view) => {
                let vals = view.iter().copied().collect::<Vec<_>>();
                assert!(
                    vals.iter().any(|v| v.abs() > 1e-6),
                    "materialized CUDA values should not be an all-zero placeholder"
                );
            }
            TensorStorageView::F16(_) => panic!("bf16 matmul output should not expose f16 data"),
            TensorStorageView::BF16(_) => {
                panic!("bf16 CUDA-only output should materialize from its resident data")
            }
        });
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_matmul_preserves_i8_dtype_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let a = make_tensor(&[1, 4], vec![1.0, -2.0, 0.5, 3.0], DType::I8).to_cuda();
        let b = make_tensor(
            &[3, 4],
            vec![
                1.0, 0.0, -1.0, 2.0, 0.5, 1.5, -0.5, 0.25, -1.0, 2.0, 1.0, -0.5,
            ],
            DType::I8,
        )
        .to_cuda();

        crate::ops::cuda::set_enabled(true);
        crate::autograd::set_strict_device_execution(true);
        let out = no_grad(|| matmul(&a, &b));
        crate::autograd::set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);

        let reference = no_grad(|| matmul(&a.to_cpu(), &b.to_cpu()));
        assert!(out.is_cuda());
        assert_eq!(out.dtype(), DType::I8);
        for (got, expect) in out.data_ref().iter().zip(reference.data_ref().iter()) {
            assert!((got - expect).abs() < 1e-5, "got {got}, expect {expect}");
        }
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_batch_matmul_preserves_bf16_dtype_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let lhs = make_tensor(
            &[1, 1, 2, 3],
            vec![1.0, -2.0, 0.5, 3.0, -1.0, 2.0],
            DType::BF16,
        )
        .to_cuda();
        let rhs = make_tensor(
            &[1, 1, 3, 2],
            vec![0.5, 1.0, -1.5, 2.0, 0.25, -0.75],
            DType::BF16,
        )
        .to_cuda();

        crate::ops::cuda::set_enabled(true);
        crate::autograd::set_strict_device_execution(true);
        let out = no_grad(|| batch_matmul(&lhs, &rhs));
        crate::autograd::set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);

        let reference = no_grad(|| batch_matmul(&lhs.to_cpu(), &rhs.to_cpu()));
        assert!(out.is_cuda());
        assert_eq!(out.dtype(), DType::BF16);
        assert!(
            !out.has_host_f32_data(),
            "bf16 CUDA batch_matmul should keep output resident until host data is requested"
        );
        for (got, expect) in out.data_ref().iter().zip(reference.data_ref().iter()) {
            assert!((got - expect).abs() < 2e-2, "got {got}, expect {expect}");
        }
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_matmul_backward_matches_cpu_reference_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let a_data = vec![1.0, -2.0, 0.5, 3.0, -1.0, 2.0];
        let b_data = vec![0.5, 1.0, -1.5, 2.0, 0.25, -0.75, 1.25, -0.5, 0.75];
        let a_cpu = make_grad_tensor(&[2, 3], a_data.clone());
        let b_cpu = make_grad_tensor(&[3, 3], b_data.clone());
        let a_cuda = make_grad_tensor(&[2, 3], a_data).to_cuda();
        let b_cuda = make_grad_tensor(&[3, 3], b_data).to_cuda();

        crate::ops::cuda::set_enabled(true);
        set_strict_device_execution(true);
        let loss_cuda = sum(&matmul(&a_cuda, &b_cuda));
        loss_cuda.backward();
        set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);

        let loss_cpu = sum(&matmul(&a_cpu, &b_cpu));
        loss_cpu.backward();

        assert!(!a_cuda.has_host_grad());
        assert!(!b_cuda.has_host_grad());
        assert!(a_cuda.cloned_cuda_f32_grad().is_some());
        assert!(b_cuda.cloned_cuda_f32_grad().is_some());
        let a_cuda_grad = a_cuda.grad().expect("cuda lhs grad");
        let b_cuda_grad = b_cuda.grad().expect("cuda rhs grad");
        let a_cpu_grad = a_cpu.grad().expect("cpu lhs grad");
        let b_cpu_grad = b_cpu.grad().expect("cpu rhs grad");
        for (got, expect) in a_cuda_grad.iter().zip(a_cpu_grad.iter()) {
            assert!(
                (got - expect).abs() < 1e-4,
                "lhs grad got {got}, expect {expect}"
            );
        }
        for (got, expect) in b_cuda_grad.iter().zip(b_cpu_grad.iter()) {
            assert!(
                (got - expect).abs() < 1e-4,
                "rhs grad got {got}, expect {expect}"
            );
        }
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_batch_matmul_backward_matches_cpu_reference_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let lhs_data = vec![1.0, -2.0, 0.5, 3.0, -1.0, 2.0];
        let rhs_data = vec![0.5, 1.0, -1.5, 2.0, 0.25, -0.75];
        let lhs_cpu = make_grad_tensor(&[1, 1, 2, 3], lhs_data.clone());
        let rhs_cpu = make_grad_tensor(&[1, 1, 3, 2], rhs_data.clone());
        let lhs_cuda = make_grad_tensor(&[1, 1, 2, 3], lhs_data).to_cuda();
        let rhs_cuda = make_grad_tensor(&[1, 1, 3, 2], rhs_data).to_cuda();

        crate::ops::cuda::set_enabled(true);
        set_strict_device_execution(true);
        let loss_cuda = sum(&batch_matmul(&lhs_cuda, &rhs_cuda));
        loss_cuda.backward();
        set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);

        let loss_cpu = sum(&batch_matmul(&lhs_cpu, &rhs_cpu));
        loss_cpu.backward();

        assert!(!lhs_cuda.has_host_grad());
        assert!(!rhs_cuda.has_host_grad());
        assert!(lhs_cuda.cloned_cuda_f32_grad().is_some());
        assert!(rhs_cuda.cloned_cuda_f32_grad().is_some());
        let lhs_cuda_grad = lhs_cuda.grad().expect("cuda lhs grad");
        let rhs_cuda_grad = rhs_cuda.grad().expect("cuda rhs grad");
        let lhs_cpu_grad = lhs_cpu.grad().expect("cpu lhs grad");
        let rhs_cpu_grad = rhs_cpu.grad().expect("cpu rhs grad");
        for (got, expect) in lhs_cuda_grad.iter().zip(lhs_cpu_grad.iter()) {
            assert!(
                (got - expect).abs() < 1e-4,
                "lhs grad got {got}, expect {expect}"
            );
        }
        for (got, expect) in rhs_cuda_grad.iter().zip(rhs_cpu_grad.iter()) {
            assert!(
                (got - expect).abs() < 1e-4,
                "rhs grad got {got}, expect {expect}"
            );
        }
    }
}
