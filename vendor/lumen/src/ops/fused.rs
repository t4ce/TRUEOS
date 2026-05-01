use crate::autograd::{
    StoragePreference, Tensor, TensorData, TensorStorageOwned, TensorStorageView,
    assert_native_device_support, is_no_grad, is_strict_device_execution,
};
use crate::ops::cuda;
use crate::ops::matmul::{
    SliceRef, dual_matvec_rowmajor_parallel_mixed, dual_matvec_silu_mul_rowmajor_parallel_f32_bf16,
    dual_matvec_silu_mul_rowmajor_parallel_f32_f16, dual_matvec_silu_mul_rowmajor_parallel_f32_i8,
    dual_matvec_silu_mul_rowmajor_parallel_i8_i8, dual_matvec_silu_mul_rowmajor_parallel_mixed,
    matvec_rowmajor_parallel_mixed, qkv_matvec_rowmajor_parallel,
    qkv_matvec_rowmajor_parallel_f32_bf16, qkv_matvec_rowmajor_parallel_f32_f16,
    qkv_matvec_rowmajor_parallel_f32_i8, qkv_matvec_rowmajor_parallel_i8_i8,
    with_bf16_input_as_f32, with_f16_input_as_f32, with_i8_input_as_f32,
};
use crate::precision::DType;
use half::{bf16, f16};
use ndarray::{Array, Array2, Array3, Axis, Ix2, Zip};
use std::cell::RefCell;
use std::rc::Rc;

#[inline]
fn causal_masked_with_past(
    is_causal: bool,
    q_len: usize,
    past_len: usize,
    q_idx: usize,
    k_idx: usize,
) -> bool {
    is_causal && q_len > 1 && k_idx > past_len + q_idx
}

#[inline]
fn slice_ref_dtype(slice: SliceRef<'_>) -> DType {
    match slice {
        SliceRef::F32(_) => DType::F32,
        SliceRef::F16(_) => DType::F16,
        SliceRef::BF16(_) => DType::BF16,
        SliceRef::I8(_, _) => DType::I8,
    }
}

fn with_decode_input_as_slice_ref<R>(input: &Tensor, f: impl FnOnce(SliceRef<'_>) -> R) -> R {
    if input.dtype() == DType::I8 {
        return match input.native_storage_owned() {
            TensorStorageOwned::I8(data, scale) => {
                if let Some(slice) = data.as_slice() {
                    f(SliceRef::I8(slice, scale))
                } else {
                    let owned = data.iter().copied().collect::<Vec<_>>();
                    f(SliceRef::I8(owned.as_slice(), scale))
                }
            }
            TensorStorageOwned::F32(_)
            | TensorStorageOwned::F16(_)
            | TensorStorageOwned::BF16(_) => {
                unreachable!("checked i8 decode input above")
            }
        };
    }

    input.with_storage_view_preferring(StoragePreference::Native, |x_view| match x_view {
        TensorStorageView::F32(x_view) => {
            if let Some(x_slice) = x_view.as_slice() {
                f(SliceRef::F32(x_slice))
            } else {
                let x_owned = x_view.iter().copied().collect::<Vec<f32>>();
                f(SliceRef::F32(x_owned.as_slice()))
            }
        }
        TensorStorageView::F16(x_view) => {
            if let Some(x_slice) = x_view.as_slice() {
                f(SliceRef::F16(x_slice))
            } else {
                let x_owned = x_view.iter().copied().collect::<Vec<_>>();
                f(SliceRef::F16(x_owned.as_slice()))
            }
        }
        TensorStorageView::BF16(x_view) => {
            if let Some(x_slice) = x_view.as_slice() {
                f(SliceRef::BF16(x_slice))
            } else {
                let x_owned = x_view.iter().copied().collect::<Vec<_>>();
                f(SliceRef::BF16(x_owned.as_slice()))
            }
        }
    })
}

fn for_each_decode_input_as_slice_ref(
    input: &Tensor,
    rows: usize,
    k_dim: usize,
    mut f: impl FnMut(usize, SliceRef<'_>),
) {
    if input.dtype() == DType::I8 {
        return match input.native_storage_owned() {
            TensorStorageOwned::I8(data, scale) => {
                let run_rows = |slice: &[i8], f: &mut dyn FnMut(usize, SliceRef<'_>)| {
                    assert_eq!(slice.len(), rows * k_dim, "decode input size mismatch");
                    for row_idx in 0..rows {
                        let start = row_idx * k_dim;
                        f(row_idx, SliceRef::I8(&slice[start..start + k_dim], scale));
                    }
                };
                if let Some(slice) = data.as_slice() {
                    run_rows(slice, &mut f);
                } else {
                    let owned = data.iter().copied().collect::<Vec<_>>();
                    run_rows(owned.as_slice(), &mut f);
                }
            }
            TensorStorageOwned::F32(_)
            | TensorStorageOwned::F16(_)
            | TensorStorageOwned::BF16(_) => {
                unreachable!("checked i8 decode input above")
            }
        };
    }

    input.with_storage_view_preferring(StoragePreference::Native, |x_view| {
        for_each_decode_input_row(x_view, rows, k_dim, f)
    });
}

fn validate_gate_up_shapes(k_dim: usize, gate_weight: &Tensor, up_weight: &Tensor) -> usize {
    assert!(k_dim > 0, "input hidden dim must be > 0");
    let gate_shape = gate_weight.shape_vec();
    let up_shape = up_weight.shape_vec();
    assert_eq!(gate_shape.len(), 2, "gate weight must be 2D [N, K]");
    assert_eq!(up_shape.len(), 2, "up weight must be 2D [N, K]");
    let n_dim = gate_shape[0];
    assert_eq!(gate_shape[1], k_dim, "gate weight K mismatch");
    assert_eq!(up_shape[0], n_dim, "gate/up out dim mismatch");
    assert_eq!(up_shape[1], k_dim, "up weight K mismatch");
    n_dim
}

fn validate_qkv_shapes(
    k_dim: usize,
    q_weight: &Tensor,
    k_weight: &Tensor,
    v_weight: &Tensor,
) -> (usize, usize, usize) {
    assert!(k_dim > 0, "input hidden dim must be > 0");
    let q_shape = q_weight.shape_vec();
    let k_shape = k_weight.shape_vec();
    let v_shape = v_weight.shape_vec();
    assert_eq!(q_shape.len(), 2, "Q weight must be 2D [Nq, K]");
    assert_eq!(k_shape.len(), 2, "K weight must be 2D [Nk, K]");
    assert_eq!(v_shape.len(), 2, "V weight must be 2D [Nv, K]");
    let q_n = q_shape[0];
    let k_n = k_shape[0];
    let v_n = v_shape[0];
    assert_eq!(q_shape[1], k_dim, "Q weight K mismatch");
    assert_eq!(k_shape[1], k_dim, "K weight K mismatch");
    assert_eq!(v_shape[1], k_dim, "V weight K mismatch");
    assert_eq!(v_n, k_n, "K/V dim mismatch");
    (q_n, k_n, v_n)
}

enum SliceRef2D<'a> {
    F32Borrowed(ndarray::ArrayViewD<'a, f32>),
    F32Owned(Vec<f32>, usize, usize),
    F16Borrowed(ndarray::ArrayViewD<'a, f16>),
    F16Owned(Vec<f16>, usize, usize),
    BF16Borrowed(ndarray::ArrayViewD<'a, bf16>),
    BF16Owned(Vec<bf16>, usize, usize),
}

impl<'a> SliceRef2D<'a> {
    fn as_slice_ref(&self) -> SliceRef<'_> {
        match self {
            Self::F32Borrowed(view) => SliceRef::F32(
                view.as_slice()
                    .expect("borrowed f32 view should remain contiguous"),
            ),
            Self::F32Owned(slice, _, _) => SliceRef::F32(slice.as_slice()),
            Self::F16Borrowed(view) => SliceRef::F16(
                view.as_slice()
                    .expect("borrowed f16 view should remain contiguous"),
            ),
            Self::F16Owned(slice, _, _) => SliceRef::F16(slice.as_slice()),
            Self::BF16Borrowed(view) => SliceRef::BF16(
                view.as_slice()
                    .expect("borrowed bf16 view should remain contiguous"),
            ),
            Self::BF16Owned(slice, _, _) => SliceRef::BF16(slice.as_slice()),
        }
    }

    fn rows(&self) -> usize {
        match self {
            Self::F32Borrowed(view) => view.shape()[0],
            Self::F32Owned(_, rows, _)
            | Self::F16Owned(_, rows, _)
            | Self::BF16Owned(_, rows, _) => *rows,
            Self::F16Borrowed(view) => view.shape()[0],
            Self::BF16Borrowed(view) => view.shape()[0],
        }
    }

    fn cols(&self) -> usize {
        match self {
            Self::F32Borrowed(view) => view.shape()[1],
            Self::F32Owned(_, _, cols)
            | Self::F16Owned(_, _, cols)
            | Self::BF16Owned(_, _, cols) => *cols,
            Self::F16Borrowed(view) => view.shape()[1],
            Self::BF16Borrowed(view) => view.shape()[1],
        }
    }
}

fn storage_view_2d_as_slice_ref<'a>(view: TensorStorageView<'a>, label: &str) -> SliceRef2D<'a> {
    match view {
        TensorStorageView::F32(view) => {
            let shape = view.shape();
            assert_eq!(shape.len(), 2, "{label} weight must be 2D [N, K]");
            let (rows, cols) = (shape[0], shape[1]);
            if view.as_slice().is_some() {
                SliceRef2D::F32Borrowed(view)
            } else {
                SliceRef2D::F32Owned(view.iter().copied().collect::<Vec<_>>(), rows, cols)
            }
        }
        TensorStorageView::F16(view) => {
            let shape = view.shape();
            assert_eq!(shape.len(), 2, "{label} weight must be 2D [N, K]");
            let (rows, cols) = (shape[0], shape[1]);
            if view.as_slice().is_some() {
                SliceRef2D::F16Borrowed(view)
            } else {
                SliceRef2D::F16Owned(view.iter().copied().collect::<Vec<_>>(), rows, cols)
            }
        }
        TensorStorageView::BF16(view) => {
            let shape = view.shape();
            assert_eq!(shape.len(), 2, "{label} weight must be 2D [N, K]");
            let (rows, cols) = (shape[0], shape[1]);
            if view.as_slice().is_some() {
                SliceRef2D::BF16Borrowed(view)
            } else {
                SliceRef2D::BF16Owned(view.iter().copied().collect::<Vec<_>>(), rows, cols)
            }
        }
    }
}

fn for_each_decode_input_row<'a>(
    x_view: TensorStorageView<'a>,
    rows: usize,
    k_dim: usize,
    mut f: impl FnMut(usize, SliceRef<'_>),
) {
    match x_view {
        TensorStorageView::F32(view) => {
            if let Ok(x_2d) = view.clone().into_shape((rows, k_dim)) {
                for row_idx in 0..rows {
                    let x_row = x_2d.row(row_idx);
                    let x_owned;
                    let x_slice = if let Some(s) = x_row.as_slice() {
                        SliceRef::F32(s)
                    } else {
                        x_owned = x_row.iter().copied().collect::<Vec<_>>();
                        SliceRef::F32(x_owned.as_slice())
                    };
                    f(row_idx, x_slice);
                }
            } else {
                let x_2d = view
                    .to_owned()
                    .into_shape((rows, k_dim))
                    .expect("decode input reshape failed");
                for row_idx in 0..rows {
                    let x_row = x_2d.row(row_idx);
                    let x_slice = SliceRef::F32(
                        x_row
                            .as_slice()
                            .expect("owned decode input row must be contiguous"),
                    );
                    f(row_idx, x_slice);
                }
            }
        }
        TensorStorageView::F16(view) => {
            if let Ok(x_2d) = view.clone().into_shape((rows, k_dim)) {
                for row_idx in 0..rows {
                    let x_row = x_2d.row(row_idx);
                    let x_owned;
                    let x_slice = if let Some(s) = x_row.as_slice() {
                        SliceRef::F16(s)
                    } else {
                        x_owned = x_row.iter().copied().collect::<Vec<_>>();
                        SliceRef::F16(x_owned.as_slice())
                    };
                    f(row_idx, x_slice);
                }
            } else {
                let x_2d = view
                    .to_owned()
                    .into_shape((rows, k_dim))
                    .expect("decode input reshape failed");
                for row_idx in 0..rows {
                    let x_row = x_2d.row(row_idx);
                    let x_slice = SliceRef::F16(
                        x_row
                            .as_slice()
                            .expect("owned decode input row must be contiguous"),
                    );
                    f(row_idx, x_slice);
                }
            }
        }
        TensorStorageView::BF16(view) => {
            if let Ok(x_2d) = view.clone().into_shape((rows, k_dim)) {
                for row_idx in 0..rows {
                    let x_row = x_2d.row(row_idx);
                    let x_owned;
                    let x_slice = if let Some(s) = x_row.as_slice() {
                        SliceRef::BF16(s)
                    } else {
                        x_owned = x_row.iter().copied().collect::<Vec<_>>();
                        SliceRef::BF16(x_owned.as_slice())
                    };
                    f(row_idx, x_slice);
                }
            } else {
                let x_2d = view
                    .to_owned()
                    .into_shape((rows, k_dim))
                    .expect("decode input reshape failed");
                for row_idx in 0..rows {
                    let x_row = x_2d.row(row_idx);
                    let x_slice = SliceRef::BF16(
                        x_row
                            .as_slice()
                            .expect("owned decode input row must be contiguous"),
                    );
                    f(row_idx, x_slice);
                }
            }
        }
    }
}

fn run_qkv_slices(
    x_slice: SliceRef<'_>,
    q_slice: SliceRef<'_>,
    k_slice: SliceRef<'_>,
    v_slice: SliceRef<'_>,
    q_n: usize,
    k_n: usize,
    k_dim: usize,
    q_out: &mut [f32],
    k_out: &mut [f32],
    v_out: &mut [f32],
) {
    match (x_slice, q_slice, k_slice, v_slice) {
        (
            SliceRef::F32(x_f32),
            SliceRef::F32(q_slice),
            SliceRef::F32(k_slice),
            SliceRef::F32(v_slice),
        ) => {
            qkv_matvec_rowmajor_parallel(
                x_f32, q_slice, k_slice, v_slice, q_n, k_n, k_dim, q_out, k_out, v_out,
            );
        }
        (
            SliceRef::F32(x_f32),
            SliceRef::F16(q_slice),
            SliceRef::F16(k_slice),
            SliceRef::F16(v_slice),
        ) => {
            qkv_matvec_rowmajor_parallel_f32_f16(
                x_f32, q_slice, k_slice, v_slice, q_n, k_n, k_dim, q_out, k_out, v_out,
            );
        }
        (
            SliceRef::F16(x_f16),
            SliceRef::F32(q_slice),
            SliceRef::F32(k_slice),
            SliceRef::F32(v_slice),
        ) => {
            with_f16_input_as_f32(x_f16, |x_f32| {
                qkv_matvec_rowmajor_parallel(
                    x_f32, q_slice, k_slice, v_slice, q_n, k_n, k_dim, q_out, k_out, v_out,
                );
            });
        }
        (
            SliceRef::F16(x_f16),
            SliceRef::F16(q_slice),
            SliceRef::F16(k_slice),
            SliceRef::F16(v_slice),
        ) => {
            with_f16_input_as_f32(x_f16, |x_f32| {
                qkv_matvec_rowmajor_parallel_f32_f16(
                    x_f32, q_slice, k_slice, v_slice, q_n, k_n, k_dim, q_out, k_out, v_out,
                );
            });
        }
        (
            SliceRef::BF16(x_bf16),
            SliceRef::F32(q_slice),
            SliceRef::F32(k_slice),
            SliceRef::F32(v_slice),
        ) => {
            with_bf16_input_as_f32(x_bf16, |x_f32| {
                qkv_matvec_rowmajor_parallel(
                    x_f32, q_slice, k_slice, v_slice, q_n, k_n, k_dim, q_out, k_out, v_out,
                );
            });
        }
        (
            SliceRef::I8(x_i8, x_scale),
            SliceRef::F32(q_slice),
            SliceRef::F32(k_slice),
            SliceRef::F32(v_slice),
        ) => {
            with_i8_input_as_f32(x_i8, x_scale, |x_f32| {
                qkv_matvec_rowmajor_parallel(
                    x_f32, q_slice, k_slice, v_slice, q_n, k_n, k_dim, q_out, k_out, v_out,
                );
            });
        }
        (
            SliceRef::F32(x_f32),
            SliceRef::BF16(q_slice),
            SliceRef::BF16(k_slice),
            SliceRef::BF16(v_slice),
        ) => {
            qkv_matvec_rowmajor_parallel_f32_bf16(
                x_f32, q_slice, k_slice, v_slice, q_n, k_n, k_dim, q_out, k_out, v_out,
            );
        }
        (
            SliceRef::BF16(x_bf16),
            SliceRef::BF16(q_slice),
            SliceRef::BF16(k_slice),
            SliceRef::BF16(v_slice),
        ) => {
            with_bf16_input_as_f32(x_bf16, |x_f32| {
                qkv_matvec_rowmajor_parallel_f32_bf16(
                    x_f32, q_slice, k_slice, v_slice, q_n, k_n, k_dim, q_out, k_out, v_out,
                );
            });
        }
        (
            SliceRef::I8(x_i8, x_scale),
            SliceRef::BF16(q_slice),
            SliceRef::BF16(k_slice),
            SliceRef::BF16(v_slice),
        ) => {
            with_i8_input_as_f32(x_i8, x_scale, |x_f32| {
                qkv_matvec_rowmajor_parallel_f32_bf16(
                    x_f32, q_slice, k_slice, v_slice, q_n, k_n, k_dim, q_out, k_out, v_out,
                );
            });
        }
        (
            SliceRef::F32(x_f32),
            SliceRef::I8(q_slice, q_scale),
            SliceRef::I8(k_slice, k_scale),
            SliceRef::I8(v_slice, v_scale),
        ) => {
            qkv_matvec_rowmajor_parallel_f32_i8(
                x_f32, q_slice, q_scale, k_slice, k_scale, v_slice, v_scale, q_n, k_n, k_dim,
                q_out, k_out, v_out,
            );
        }
        (
            SliceRef::BF16(x_bf16),
            SliceRef::I8(q_slice, q_scale),
            SliceRef::I8(k_slice, k_scale),
            SliceRef::I8(v_slice, v_scale),
        ) => {
            with_bf16_input_as_f32(x_bf16, |x_f32| {
                qkv_matvec_rowmajor_parallel_f32_i8(
                    x_f32, q_slice, q_scale, k_slice, k_scale, v_slice, v_scale, q_n, k_n, k_dim,
                    q_out, k_out, v_out,
                );
            });
        }
        (
            SliceRef::F16(x_f16),
            SliceRef::I8(q_slice, q_scale),
            SliceRef::I8(k_slice, k_scale),
            SliceRef::I8(v_slice, v_scale),
        ) => {
            with_f16_input_as_f32(x_f16, |x_f32| {
                qkv_matvec_rowmajor_parallel_f32_i8(
                    x_f32, q_slice, q_scale, k_slice, k_scale, v_slice, v_scale, q_n, k_n, k_dim,
                    q_out, k_out, v_out,
                );
            });
        }
        (
            SliceRef::I8(x_i8, x_scale),
            SliceRef::I8(q_slice, q_scale),
            SliceRef::I8(k_slice, k_scale),
            SliceRef::I8(v_slice, v_scale),
        ) => {
            qkv_matvec_rowmajor_parallel_i8_i8(
                x_i8, x_scale, q_slice, q_scale, k_slice, k_scale, v_slice, v_scale, q_n, k_n,
                k_dim, q_out, k_out, v_out,
            );
        }
        (x_slice, q_slice, k_slice, v_slice) => {
            matvec_rowmajor_parallel_mixed(x_slice, q_slice, q_n, k_dim, q_out);
            dual_matvec_rowmajor_parallel_mixed(
                x_slice, k_slice, v_slice, k_n, k_dim, k_out, v_out,
            );
        }
    }
}

fn run_gate_up_slice(
    x_slice: SliceRef<'_>,
    gate_slice: SliceRef<'_>,
    up_slice: SliceRef<'_>,
    n_dim: usize,
    k_dim: usize,
    out: &mut [f32],
) {
    match (x_slice, gate_slice, up_slice) {
        (SliceRef::F32(x_f32), SliceRef::F16(gate_slice), SliceRef::F16(up_slice)) => {
            dual_matvec_silu_mul_rowmajor_parallel_f32_f16(
                x_f32, gate_slice, up_slice, n_dim, k_dim, out,
            );
        }
        (SliceRef::F16(x_f16), SliceRef::F16(gate_slice), SliceRef::F16(up_slice)) => {
            with_f16_input_as_f32(x_f16, |x_f32| {
                dual_matvec_silu_mul_rowmajor_parallel_f32_f16(
                    x_f32, gate_slice, up_slice, n_dim, k_dim, out,
                );
            });
        }
        (SliceRef::F32(x_f32), SliceRef::BF16(gate_slice), SliceRef::BF16(up_slice)) => {
            dual_matvec_silu_mul_rowmajor_parallel_f32_bf16(
                x_f32, gate_slice, up_slice, n_dim, k_dim, out,
            );
        }
        (SliceRef::BF16(x_bf16), SliceRef::BF16(gate_slice), SliceRef::BF16(up_slice)) => {
            with_bf16_input_as_f32(x_bf16, |x_f32| {
                dual_matvec_silu_mul_rowmajor_parallel_f32_bf16(
                    x_f32, gate_slice, up_slice, n_dim, k_dim, out,
                );
            });
        }
        (SliceRef::I8(x_i8, x_scale), SliceRef::BF16(gate_slice), SliceRef::BF16(up_slice)) => {
            with_i8_input_as_f32(x_i8, x_scale, |x_f32| {
                dual_matvec_silu_mul_rowmajor_parallel_f32_bf16(
                    x_f32, gate_slice, up_slice, n_dim, k_dim, out,
                );
            });
        }
        (
            SliceRef::F32(x_f32),
            SliceRef::I8(gate_slice, gate_scale),
            SliceRef::I8(up_slice, up_scale),
        ) => {
            dual_matvec_silu_mul_rowmajor_parallel_f32_i8(
                x_f32, gate_slice, gate_scale, up_slice, up_scale, n_dim, k_dim, out,
            );
        }
        (
            SliceRef::BF16(x_bf16),
            SliceRef::I8(gate_slice, gate_scale),
            SliceRef::I8(up_slice, up_scale),
        ) => {
            with_bf16_input_as_f32(x_bf16, |x_f32| {
                dual_matvec_silu_mul_rowmajor_parallel_f32_i8(
                    x_f32, gate_slice, gate_scale, up_slice, up_scale, n_dim, k_dim, out,
                );
            });
        }
        (
            SliceRef::F16(x_f16),
            SliceRef::I8(gate_slice, gate_scale),
            SliceRef::I8(up_slice, up_scale),
        ) => {
            with_f16_input_as_f32(x_f16, |x_f32| {
                dual_matvec_silu_mul_rowmajor_parallel_f32_i8(
                    x_f32, gate_slice, gate_scale, up_slice, up_scale, n_dim, k_dim, out,
                );
            });
        }
        (
            SliceRef::I8(x_i8, x_scale),
            SliceRef::I8(gate_slice, gate_scale),
            SliceRef::I8(up_slice, up_scale),
        ) => {
            dual_matvec_silu_mul_rowmajor_parallel_i8_i8(
                x_i8, x_scale, gate_slice, gate_scale, up_slice, up_scale, n_dim, k_dim, out,
            );
        }
        (x_slice, gate_slice, up_slice) => {
            dual_matvec_silu_mul_rowmajor_parallel_mixed(
                x_slice, gate_slice, up_slice, n_dim, k_dim, out,
            );
        }
    }
}

pub fn fused_softmax(input: &Tensor, scale: f32, is_causal: bool) -> Tensor {
    let output_device = input.device();
    let build_graph = !is_no_grad() && input.requires_grad();
    let cuda_native_supported = output_device == crate::autograd::Device::Cuda;
    assert_native_device_support(output_device, "fused_softmax", cuda_native_supported);

    if !build_graph {
        let shape = input.shape_vec();
        if shape.len() != 4 {
            panic!("Fused Softmax expects 4D input [B, H, Q, K]");
        }
        let output_dtype = match input.dtype() {
            DType::F16 => DType::F16,
            DType::BF16 => DType::BF16,
            DType::F32 | DType::I8 => DType::F32,
        };
        assert!(
            shape[3] > 0,
            "Fused Softmax key dimension must be greater than zero"
        );
        if output_device == crate::autograd::Device::Cuda && input.len() > 0 {
            let batch_heads = shape[0]
                .checked_mul(shape[1])
                .expect("fused_softmax batch_heads overflow");
            let q_len = shape[2];
            let k_len = shape[3];
            let cuda_out = input.with_cuda_f32_buffer(|input_buf| {
                cuda::fused_softmax_f32(input_buf, batch_heads, q_len, k_len, scale, is_causal)
            });
            if let Ok((buffer, out)) = cuda_out {
                let out = Array::from_shape_vec(ndarray::IxDyn(&shape), out)
                    .expect("CUDA fused_softmax output shape build failed")
                    .into_dyn();
                return Tensor::from_f32_data_no_grad_with_device_dtype_and_cuda_buffer(
                    out,
                    output_dtype,
                    output_device,
                    Some(buffer),
                );
            }
        }
        return input.with_storage_view_preferring(
            StoragePreference::Native,
            |x_view| match x_view {
                TensorStorageView::F32(x_view) => {
                    let shape = x_view.shape().to_vec();
                    if shape.len() != 4 {
                        panic!("Fused Softmax expects 4D input [B, H, Q, K]");
                    }

                    let q_len = shape[2];
                    let k_len = shape[3];

                    let mut out = Array::zeros(x_view.raw_dim());
                    let x_view = x_view.into_dimensionality::<ndarray::Ix4>().unwrap();
                    let mut out_view = out
                        .view_mut()
                        .into_dimensionality::<ndarray::Ix4>()
                        .unwrap();

                    Zip::from(out_view.outer_iter_mut())
                        .and(x_view.outer_iter())
                        .par_for_each(|mut out_b, x_b| {
                            Zip::from(out_b.outer_iter_mut())
                                .and(x_b.outer_iter())
                                .for_each(|mut out_h, x_h| {
                                    for i in 0..q_len {
                                        let row_in = x_h.slice(ndarray::s![i, ..]);
                                        let mut row_out = out_h.slice_mut(ndarray::s![i, ..]);

                                        let mut max_val = f32::NEG_INFINITY;
                                        for j in 0..k_len {
                                            let is_masked =
                                                if is_causal && q_len > 1 { j > i } else { false };
                                            if !is_masked {
                                                let val = row_in[j] * scale;
                                                if val > max_val {
                                                    max_val = val;
                                                }
                                            }
                                        }

                                        let mut sum_exp = 0.0;
                                        for j in 0..k_len {
                                            let is_masked =
                                                if is_causal && q_len > 1 { j > i } else { false };
                                            if is_masked {
                                                row_out[j] = 0.0;
                                            } else {
                                                let val = (row_in[j] * scale - max_val).exp();
                                                row_out[j] = val;
                                                sum_exp += val;
                                            }
                                        }

                                        let inv_sum = 1.0 / (sum_exp + 1e-10);
                                        for j in 0..k_len {
                                            row_out[j] *= inv_sum;
                                        }
                                    }
                                });
                        });

                    Tensor::from_f32_data_no_grad_with_device_dtype(
                        out.into_dyn(),
                        crate::precision::DType::F32,
                        output_device,
                    )
                }
                TensorStorageView::F16(x_view) => {
                    let shape = x_view.shape().to_vec();
                    if shape.len() != 4 {
                        panic!("Fused Softmax expects 4D input [B, H, Q, K]");
                    }

                    let q_len = shape[2];
                    let k_len = shape[3];

                    let mut out = ndarray::ArrayD::<f16>::from_elem(
                        ndarray::IxDyn(&shape),
                        f16::from_bits(0),
                    );
                    let x_view = x_view.into_dimensionality::<ndarray::Ix4>().unwrap();
                    let mut out_view = out
                        .view_mut()
                        .into_dimensionality::<ndarray::Ix4>()
                        .unwrap();

                    Zip::from(out_view.outer_iter_mut())
                        .and(x_view.outer_iter())
                        .par_for_each(|mut out_b, x_b| {
                            Zip::from(out_b.outer_iter_mut())
                                .and(x_b.outer_iter())
                                .for_each(|mut out_h, x_h| {
                                    for i in 0..q_len {
                                        let row_in = x_h.slice(ndarray::s![i, ..]);
                                        let mut row_out = out_h.slice_mut(ndarray::s![i, ..]);
                                        let mut row_exp = vec![0.0f32; k_len];

                                        let mut max_val = f32::NEG_INFINITY;
                                        for j in 0..k_len {
                                            let is_masked =
                                                if is_causal && q_len > 1 { j > i } else { false };
                                            if !is_masked {
                                                let val = row_in[j].to_f32() * scale;
                                                if val > max_val {
                                                    max_val = val;
                                                }
                                            }
                                        }

                                        let mut sum_exp = 0.0;
                                        for j in 0..k_len {
                                            let is_masked =
                                                if is_causal && q_len > 1 { j > i } else { false };
                                            if is_masked {
                                                row_out[j] = f16::from_bits(0);
                                                row_exp[j] = 0.0;
                                            } else {
                                                let val =
                                                    (row_in[j].to_f32() * scale - max_val).exp();
                                                row_exp[j] = val;
                                                sum_exp += val;
                                            }
                                        }

                                        let inv_sum = 1.0 / (sum_exp + 1e-10);
                                        for j in 0..k_len {
                                            row_out[j] = f16::from_f32(row_exp[j] * inv_sum);
                                        }
                                    }
                                });
                        });

                    Tensor::from_shared_f16_no_grad_with_device(out.into_shared(), output_device)
                }
                TensorStorageView::BF16(x_view) => {
                    let shape = x_view.shape().to_vec();
                    if shape.len() != 4 {
                        panic!("Fused Softmax expects 4D input [B, H, Q, K]");
                    }

                    let q_len = shape[2];
                    let k_len = shape[3];

                    let mut out = ndarray::ArrayD::<bf16>::from_elem(
                        ndarray::IxDyn(&shape),
                        bf16::from_bits(0),
                    );
                    let x_view = x_view.into_dimensionality::<ndarray::Ix4>().unwrap();
                    let mut out_view = out
                        .view_mut()
                        .into_dimensionality::<ndarray::Ix4>()
                        .unwrap();

                    Zip::from(out_view.outer_iter_mut())
                        .and(x_view.outer_iter())
                        .par_for_each(|mut out_b, x_b| {
                            Zip::from(out_b.outer_iter_mut())
                                .and(x_b.outer_iter())
                                .for_each(|mut out_h, x_h| {
                                    for i in 0..q_len {
                                        let row_in = x_h.slice(ndarray::s![i, ..]);
                                        let mut row_out = out_h.slice_mut(ndarray::s![i, ..]);
                                        let mut row_exp = vec![0.0f32; k_len];

                                        let mut max_val = f32::NEG_INFINITY;
                                        for j in 0..k_len {
                                            let is_masked =
                                                if is_causal && q_len > 1 { j > i } else { false };
                                            if !is_masked {
                                                let val = row_in[j].to_f32() * scale;
                                                if val > max_val {
                                                    max_val = val;
                                                }
                                            }
                                        }

                                        let mut sum_exp = 0.0;
                                        for j in 0..k_len {
                                            let is_masked =
                                                if is_causal && q_len > 1 { j > i } else { false };
                                            if is_masked {
                                                row_out[j] = bf16::from_bits(0);
                                                row_exp[j] = 0.0;
                                            } else {
                                                let val =
                                                    (row_in[j].to_f32() * scale - max_val).exp();
                                                row_exp[j] = val;
                                                sum_exp += val;
                                            }
                                        }

                                        let inv_sum = 1.0 / (sum_exp + 1e-10);
                                        for j in 0..k_len {
                                            row_out[j] = bf16::from_f32(row_exp[j] * inv_sum);
                                        }
                                    }
                                });
                        });

                    Tensor::from_shared_bf16_no_grad_with_device(out.into_shared(), output_device)
                }
            },
        );
    }

    if output_device == crate::autograd::Device::Cuda {
        let shape = input.shape_vec();
        if shape.len() != 4 {
            panic!("Fused Softmax expects 4D input [B, H, Q, K]");
        }
        assert!(
            shape[3] > 0,
            "Fused Softmax key dimension must be greater than zero"
        );
        let batch_heads = shape[0]
            .checked_mul(shape[1])
            .expect("fused_softmax batch_heads overflow");
        let q_len = shape[2];
        let k_len = shape[3];
        let cuda_out = if input.len() > 0 {
            input.with_cuda_f32_buffer(|input_buf| {
                cuda::fused_softmax_f32_no_host(
                    input_buf,
                    batch_heads,
                    q_len,
                    k_len,
                    scale,
                    is_causal,
                )
            })
        } else {
            Err("empty fused_softmax input skips CUDA kernel".to_string())
        };
        if let Ok(output_buffer) = cuda_out {
            let output = Array::zeros(ndarray::IxDyn(&shape)).into_dyn();
            let input_clone = input.clone();
            let backward_output_buffer = output_buffer.clone();
            let output_self = Rc::new(RefCell::new(None::<Tensor>));
            let output_self_for_backward = output_self.clone();
            let tensor = Tensor(Rc::new(RefCell::new(TensorData {
                data: output.into_shared(),
                f16_data: None,
                bf16_data: None,
                i8_data: None,
                cuda_f32_data: Some(output_buffer),
                i8_scale: None,
                has_f32_data: false,
                storage_dtype: crate::precision::DType::F32,
                cache_dirty: false,
                is_parameter: false,
                grad: None,
                cuda_f32_grad: None,
                parents: vec![input.clone()],
                backward_op: Some(std::rc::Rc::new(move |grad| {
                    let grad_buffer = output_self_for_backward
                        .borrow()
                        .as_ref()
                        .and_then(|output| output.cloned_cuda_f32_grad())
                        .filter(|grad_buffer| grad_buffer.len() == grad.len())
                        .unwrap_or_else(|| {
                            let grad_host = grad.iter().copied().collect::<Vec<_>>();
                            cuda::upload_f32(&grad_host)
                                .expect("CUDA fused_softmax grad upload failed")
                        });
                    if is_strict_device_execution() {
                        let dx_buffer = cuda::fused_softmax_backward_f32_buffer(
                            &backward_output_buffer,
                            &grad_buffer,
                            batch_heads,
                            q_len,
                            k_len,
                            scale,
                        )
                        .expect("CUDA fused_softmax backward failed");
                        input_clone.add_cuda_grad_buffer_only(dx_buffer);
                        return;
                    }
                    let (dx_buffer, dx) = cuda::fused_softmax_backward_f32(
                        &backward_output_buffer,
                        &grad_buffer,
                        batch_heads,
                        q_len,
                        k_len,
                        scale,
                    )
                    .expect("CUDA fused_softmax backward failed");
                    let dx = Array::from_shape_vec(ndarray::IxDyn(&shape), dx)
                        .expect("CUDA fused_softmax backward shape build failed")
                        .into_dyn();
                    input_clone.add_grad_with_cuda_buffer(dx, Some(dx_buffer));
                })),
                requires_grad: true,
                device: output_device,
            })));
            *output_self.borrow_mut() = Some(tensor.clone());
            return tensor;
        }
    }

    let (output, output_data) = {
        let x = input.data_ref();
        let shape = x.shape();
        if shape.len() != 4 {
            panic!("Fused Softmax expects 4D input [B, H, Q, K]");
        }
        assert!(
            shape[3] > 0,
            "Fused Softmax key dimension must be greater than zero"
        );

        let q_len = shape[2];
        let k_len = shape[3];

        let mut out = Array::zeros(x.dim());
        let x_view = x.view().into_dimensionality::<ndarray::Ix4>().unwrap();
        let mut out_view = out
            .view_mut()
            .into_dimensionality::<ndarray::Ix4>()
            .unwrap();

        Zip::from(out_view.outer_iter_mut())
            .and(x_view.outer_iter())
            .par_for_each(|mut out_b, x_b| {
                Zip::from(out_b.outer_iter_mut())
                    .and(x_b.outer_iter())
                    .for_each(|mut out_h, x_h| {
                        for i in 0..q_len {
                            let row_in = x_h.slice(ndarray::s![i, ..]);
                            let mut row_out = out_h.slice_mut(ndarray::s![i, ..]);

                            let mut max_val = f32::NEG_INFINITY;
                            for j in 0..k_len {
                                let is_masked = if is_causal && q_len > 1 { j > i } else { false };
                                if !is_masked {
                                    let val = row_in[j] * scale;
                                    if val > max_val {
                                        max_val = val;
                                    }
                                }
                            }

                            let mut sum_exp = 0.0;
                            for j in 0..k_len {
                                let is_masked = if is_causal && q_len > 1 { j > i } else { false };
                                if is_masked {
                                    row_out[j] = 0.0;
                                } else {
                                    let val = (row_in[j] * scale - max_val).exp();
                                    row_out[j] = val;
                                    sum_exp += val;
                                }
                            }

                            let inv_sum = 1.0 / (sum_exp + 1e-10);
                            for j in 0..k_len {
                                row_out[j] *= inv_sum;
                            }
                        }
                    });
            });

        (out.clone().into_dyn(), out)
    };
    let input_clone = input.clone();
    Tensor(Rc::new(RefCell::new(TensorData {
        data: output.into_shared(),
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
        parents: vec![input.clone()],
        backward_op: Some(std::rc::Rc::new(move |grad| {
            let y = &output_data;
            let y_grad = y * grad;
            let sum_y_grad = y_grad.sum_axis(Axis(3)).insert_axis(Axis(3));
            let dx = y * (grad - &sum_y_grad) * scale;
            input_clone.add_grad(dx);
        })),
        requires_grad: true,
        device: output_device,
    })))
}

pub(crate) fn fused_softmax_with_past_infer(
    input: &Tensor,
    scale: f32,
    is_causal: bool,
    past_len: usize,
) -> Tensor {
    assert!(
        is_no_grad(),
        "fused_softmax_with_past_infer is inference-only"
    );
    if past_len == 0 {
        return fused_softmax(input, scale, is_causal);
    }

    let output_device = input.device();
    let shape = input.shape_vec();
    if shape.len() != 4 {
        panic!("Fused Softmax expects 4D input [B, H, Q, K]");
    }
    let batch_heads = shape[0]
        .checked_mul(shape[1])
        .expect("fused_softmax_with_past batch_heads overflow");
    let q_len = shape[2];
    let k_len = shape[3];
    assert!(
        k_len > 0,
        "Fused Softmax key dimension must be greater than zero"
    );
    let output_dtype = match input.dtype() {
        DType::F16 => DType::F16,
        DType::BF16 => DType::BF16,
        DType::F32 | DType::I8 => DType::F32,
    };

    if output_device == crate::autograd::Device::Cuda && input.len() > 0 {
        let cuda_out = input.with_cuda_f32_buffer(|input_buf| {
            cuda::fused_softmax_f32_with_past(
                input_buf,
                batch_heads,
                q_len,
                k_len,
                scale,
                is_causal,
                past_len,
            )
        });
        if let Ok((buffer, out)) = cuda_out {
            let out = Array::from_shape_vec(ndarray::IxDyn(&shape), out)
                .expect("CUDA fused_softmax_with_past output shape build failed")
                .into_dyn();
            return Tensor::from_f32_data_no_grad_with_device_dtype_and_cuda_buffer(
                out,
                output_dtype,
                output_device,
                Some(buffer),
            );
        }
    }

    input.with_storage_view_preferring(StoragePreference::Native, |x_view| match x_view {
        TensorStorageView::F32(x_view) => {
            let mut out = Array::zeros(x_view.raw_dim());
            let x_view = x_view.into_dimensionality::<ndarray::Ix4>().unwrap();
            let mut out_view = out
                .view_mut()
                .into_dimensionality::<ndarray::Ix4>()
                .unwrap();

            Zip::from(out_view.outer_iter_mut())
                .and(x_view.outer_iter())
                .par_for_each(|mut out_b, x_b| {
                    Zip::from(out_b.outer_iter_mut())
                        .and(x_b.outer_iter())
                        .for_each(|mut out_h, x_h| {
                            for i in 0..q_len {
                                let row_in = x_h.slice(ndarray::s![i, ..]);
                                let mut row_out = out_h.slice_mut(ndarray::s![i, ..]);

                                let mut max_val = f32::NEG_INFINITY;
                                for j in 0..k_len {
                                    if causal_masked_with_past(is_causal, q_len, past_len, i, j) {
                                        continue;
                                    }
                                    let val = row_in[j] * scale;
                                    if val > max_val {
                                        max_val = val;
                                    }
                                }

                                let mut sum_exp = 0.0;
                                for j in 0..k_len {
                                    if causal_masked_with_past(is_causal, q_len, past_len, i, j) {
                                        row_out[j] = 0.0;
                                    } else {
                                        let val = (row_in[j] * scale - max_val).exp();
                                        row_out[j] = val;
                                        sum_exp += val;
                                    }
                                }

                                let inv_sum = 1.0 / (sum_exp + 1e-10);
                                for j in 0..k_len {
                                    row_out[j] *= inv_sum;
                                }
                            }
                        });
                });

            Tensor::from_f32_data_no_grad_with_device_dtype(
                out.into_dyn(),
                DType::F32,
                output_device,
            )
        }
        TensorStorageView::F16(x_view) => {
            let mut out =
                ndarray::ArrayD::<f16>::from_elem(ndarray::IxDyn(&shape), f16::from_bits(0));
            let x_view = x_view.into_dimensionality::<ndarray::Ix4>().unwrap();
            let mut out_view = out
                .view_mut()
                .into_dimensionality::<ndarray::Ix4>()
                .unwrap();

            Zip::from(out_view.outer_iter_mut())
                .and(x_view.outer_iter())
                .par_for_each(|mut out_b, x_b| {
                    Zip::from(out_b.outer_iter_mut())
                        .and(x_b.outer_iter())
                        .for_each(|mut out_h, x_h| {
                            for i in 0..q_len {
                                let row_in = x_h.slice(ndarray::s![i, ..]);
                                let mut row_out = out_h.slice_mut(ndarray::s![i, ..]);
                                let mut row_exp = vec![0.0f32; k_len];

                                let mut max_val = f32::NEG_INFINITY;
                                for j in 0..k_len {
                                    if causal_masked_with_past(is_causal, q_len, past_len, i, j) {
                                        continue;
                                    }
                                    let val = row_in[j].to_f32() * scale;
                                    if val > max_val {
                                        max_val = val;
                                    }
                                }

                                let mut sum_exp = 0.0;
                                for j in 0..k_len {
                                    if causal_masked_with_past(is_causal, q_len, past_len, i, j) {
                                        row_out[j] = f16::from_bits(0);
                                        row_exp[j] = 0.0;
                                    } else {
                                        let val = (row_in[j].to_f32() * scale - max_val).exp();
                                        row_exp[j] = val;
                                        sum_exp += val;
                                    }
                                }

                                let inv_sum = 1.0 / (sum_exp + 1e-10);
                                for j in 0..k_len {
                                    row_out[j] = f16::from_f32(row_exp[j] * inv_sum);
                                }
                            }
                        });
                });

            Tensor::from_shared_f16_no_grad_with_device(out.into_shared(), output_device)
        }
        TensorStorageView::BF16(x_view) => {
            let mut out =
                ndarray::ArrayD::<bf16>::from_elem(ndarray::IxDyn(&shape), bf16::from_bits(0));
            let x_view = x_view.into_dimensionality::<ndarray::Ix4>().unwrap();
            let mut out_view = out
                .view_mut()
                .into_dimensionality::<ndarray::Ix4>()
                .unwrap();

            Zip::from(out_view.outer_iter_mut())
                .and(x_view.outer_iter())
                .par_for_each(|mut out_b, x_b| {
                    Zip::from(out_b.outer_iter_mut())
                        .and(x_b.outer_iter())
                        .for_each(|mut out_h, x_h| {
                            for i in 0..q_len {
                                let row_in = x_h.slice(ndarray::s![i, ..]);
                                let mut row_out = out_h.slice_mut(ndarray::s![i, ..]);
                                let mut row_exp = vec![0.0f32; k_len];

                                let mut max_val = f32::NEG_INFINITY;
                                for j in 0..k_len {
                                    if causal_masked_with_past(is_causal, q_len, past_len, i, j) {
                                        continue;
                                    }
                                    let val = row_in[j].to_f32() * scale;
                                    if val > max_val {
                                        max_val = val;
                                    }
                                }

                                let mut sum_exp = 0.0;
                                for j in 0..k_len {
                                    if causal_masked_with_past(is_causal, q_len, past_len, i, j) {
                                        row_out[j] = bf16::from_bits(0);
                                        row_exp[j] = 0.0;
                                    } else {
                                        let val = (row_in[j].to_f32() * scale - max_val).exp();
                                        row_exp[j] = val;
                                        sum_exp += val;
                                    }
                                }

                                let inv_sum = 1.0 / (sum_exp + 1e-10);
                                for j in 0..k_len {
                                    row_out[j] = bf16::from_f32(row_exp[j] * inv_sum);
                                }
                            }
                        });
                });

            Tensor::from_shared_bf16_no_grad_with_device(out.into_shared(), output_device)
        }
    })
}

pub fn fused_gate_up_silu_infer(
    input: &Tensor,
    gate_weight: &Tensor,
    up_weight: &Tensor,
) -> Tensor {
    assert!(is_no_grad(), "fused_gate_up_silu_infer is inference-only");
    let output_device = input.device();
    assert_native_device_support(
        output_device,
        "fused_gate_up_silu_infer",
        output_device == crate::autograd::Device::Cuda,
    );
    assert_eq!(
        gate_weight.device(),
        output_device,
        "fused_gate_up_silu_infer expects input and gate_weight on the same device"
    );
    assert_eq!(
        up_weight.device(),
        output_device,
        "fused_gate_up_silu_infer expects input and up_weight on the same device"
    );

    let x_shape = input.shape_vec();
    let k_dim = *x_shape.last().expect("input must have last dim");
    let n_dim = validate_gate_up_shapes(k_dim, gate_weight, up_weight);
    let m_dim = input.len() / k_dim;
    let output_dtype = if input.dtype() == gate_weight.dtype()
        && input.dtype() == up_weight.dtype()
        && matches!(input.dtype(), DType::F16 | DType::BF16)
    {
        input.dtype()
    } else {
        DType::F32
    };
    if output_device == crate::autograd::Device::Cuda && input.len() > 0 {
        let cuda_out = input.with_cuda_f32_buffer(|input_buf| {
            gate_weight.with_cuda_f32_buffer(|gate_buf| {
                up_weight.with_cuda_f32_buffer(|up_buf| {
                    cuda::fused_gate_up_silu_f32(input_buf, gate_buf, up_buf, m_dim, n_dim, k_dim)
                })
            })
        });
        if let Ok((buffer, out)) = cuda_out {
            let mut out_shape = x_shape.clone();
            let last = out_shape.len() - 1;
            out_shape[last] = n_dim;
            let out = Array::from_shape_vec(ndarray::IxDyn(&out_shape), out)
                .expect("CUDA fused gate/up output shape build failed")
                .into_dyn();
            return Tensor::from_f32_data_no_grad_with_device_dtype_and_cuda_buffer(
                out,
                output_dtype,
                output_device,
                Some(buffer),
            );
        }
    }
    if m_dim == 1 {
        let mut out = vec![0.0f32; n_dim];
        fused_gate_up_silu_infer_into(input, gate_weight, up_weight, &mut out);
        let mut out_shape = x_shape;
        let last = out_shape.len() - 1;
        out_shape[last] = n_dim;
        return Tensor::from_f32_data_no_grad_with_device_dtype(
            Array2::from_shape_vec((1, n_dim), out)
                .expect("decode gate/up output shape build failed")
                .into_shape(out_shape)
                .expect("decode gate/up reshape failed")
                .into_dyn(),
            output_dtype,
            output_device,
        );
    }

    let (out, n_dim) = input.with_storage_view_preferring(StoragePreference::Native, |x_view| {
        let input_dtype = match &x_view {
            TensorStorageView::F32(_) => DType::F32,
            TensorStorageView::F16(_) => DType::F16,
            TensorStorageView::BF16(_) => DType::BF16,
        };
        gate_weight.with_storage_view_for_input_dtype(input_dtype, |gate_view| {
            up_weight.with_storage_view_for_input_dtype(input_dtype, |up_view| {
                macro_rules! run_gate_up {
                    ($gate_slice:expr, $up_slice:expr, $n_dim:expr) => {{
                        let n_dim = $n_dim;
                        let gate_slice = $gate_slice;
                        let up_slice = $up_slice;
                        let mut out = Array2::<f32>::zeros((m_dim, n_dim));
                        for_each_decode_input_row(x_view, m_dim, k_dim, |row_idx, x_slice| {
                            let mut out_row = out.slice_mut(ndarray::s![row_idx, ..]);
                            let out_slice = out_row
                                .as_slice_mut()
                                .expect("output row should be contiguous");
                            dual_matvec_silu_mul_rowmajor_parallel_mixed(
                                x_slice, gate_slice, up_slice, n_dim, k_dim, out_slice,
                            );
                        });
                        (out, n_dim)
                    }};
                }

                match (gate_view, up_view) {
                    (TensorStorageView::F32(gate_view), TensorStorageView::F32(up_view)) => {
                        let gate_2d = gate_view
                            .into_dimensionality::<Ix2>()
                            .expect("gate weight must be 2D [N, K]");
                        let up_2d = up_view
                            .into_dimensionality::<Ix2>()
                            .expect("up weight must be 2D [N, K]");
                        let n_dim = gate_2d.nrows();
                        assert_eq!(gate_2d.ncols(), k_dim, "gate weight K mismatch");
                        assert_eq!(up_2d.nrows(), n_dim, "gate/up out dim mismatch");
                        assert_eq!(up_2d.ncols(), k_dim, "up weight K mismatch");
                        run_gate_up!(
                            SliceRef::F32(
                                gate_2d.as_slice().expect("gate weight must be contiguous")
                            ),
                            SliceRef::F32(up_2d.as_slice().expect("up weight must be contiguous")),
                            n_dim
                        )
                    }
                    (TensorStorageView::F32(gate_view), TensorStorageView::BF16(up_view)) => {
                        let gate_2d = gate_view
                            .into_dimensionality::<Ix2>()
                            .expect("gate weight must be 2D [N, K]");
                        let up_2d = up_view
                            .into_dimensionality::<Ix2>()
                            .expect("up weight must be 2D [N, K]");
                        let n_dim = gate_2d.nrows();
                        assert_eq!(gate_2d.ncols(), k_dim, "gate weight K mismatch");
                        assert_eq!(up_2d.nrows(), n_dim, "gate/up out dim mismatch");
                        assert_eq!(up_2d.ncols(), k_dim, "up weight K mismatch");
                        run_gate_up!(
                            SliceRef::F32(
                                gate_2d.as_slice().expect("gate weight must be contiguous")
                            ),
                            SliceRef::BF16(up_2d.as_slice().expect("up weight must be contiguous")),
                            n_dim
                        )
                    }
                    (TensorStorageView::BF16(gate_view), TensorStorageView::F32(up_view)) => {
                        let gate_2d = gate_view
                            .into_dimensionality::<Ix2>()
                            .expect("gate weight must be 2D [N, K]");
                        let up_2d = up_view
                            .into_dimensionality::<Ix2>()
                            .expect("up weight must be 2D [N, K]");
                        let n_dim = gate_2d.nrows();
                        assert_eq!(gate_2d.ncols(), k_dim, "gate weight K mismatch");
                        assert_eq!(up_2d.nrows(), n_dim, "gate/up out dim mismatch");
                        assert_eq!(up_2d.ncols(), k_dim, "up weight K mismatch");
                        run_gate_up!(
                            SliceRef::BF16(
                                gate_2d.as_slice().expect("gate weight must be contiguous")
                            ),
                            SliceRef::F32(up_2d.as_slice().expect("up weight must be contiguous")),
                            n_dim
                        )
                    }
                    (TensorStorageView::BF16(gate_view), TensorStorageView::BF16(up_view)) => {
                        let gate_2d = gate_view
                            .into_dimensionality::<Ix2>()
                            .expect("gate weight must be 2D [N, K]");
                        let up_2d = up_view
                            .into_dimensionality::<Ix2>()
                            .expect("up weight must be 2D [N, K]");
                        let n_dim = gate_2d.nrows();
                        assert_eq!(gate_2d.ncols(), k_dim, "gate weight K mismatch");
                        assert_eq!(up_2d.nrows(), n_dim, "gate/up out dim mismatch");
                        assert_eq!(up_2d.ncols(), k_dim, "up weight K mismatch");
                        run_gate_up!(
                            SliceRef::BF16(
                                gate_2d.as_slice().expect("gate weight must be contiguous")
                            ),
                            SliceRef::BF16(up_2d.as_slice().expect("up weight must be contiguous")),
                            n_dim
                        )
                    }
                    (gate_view, up_view) => {
                        let gate_slice = storage_view_2d_as_slice_ref(gate_view, "gate");
                        let up_slice = storage_view_2d_as_slice_ref(up_view, "up");
                        assert_eq!(gate_slice.cols(), k_dim, "gate weight K mismatch");
                        assert_eq!(
                            up_slice.rows(),
                            gate_slice.rows(),
                            "gate/up out dim mismatch"
                        );
                        assert_eq!(up_slice.cols(), k_dim, "up weight K mismatch");
                        run_gate_up!(
                            gate_slice.as_slice_ref(),
                            up_slice.as_slice_ref(),
                            gate_slice.rows()
                        )
                    }
                }
            })
        })
    });

    let mut out_shape = x_shape;
    let last = out_shape.len() - 1;
    out_shape[last] = n_dim;
    Tensor::from_f32_data_no_grad_with_device_dtype(
        out.into_shape(out_shape).unwrap().into_dyn(),
        output_dtype,
        output_device,
    )
}

pub fn fused_gate_up_silu_infer_into(
    input: &Tensor,
    gate_weight: &Tensor,
    up_weight: &Tensor,
    out: &mut [f32],
) {
    assert!(
        is_no_grad(),
        "fused_gate_up_silu_infer_into is inference-only"
    );
    let compute_device = input.device();
    assert_native_device_support(
        compute_device,
        "fused_gate_up_silu_infer_into",
        compute_device == crate::autograd::Device::Cuda,
    );
    assert_eq!(
        gate_weight.device(),
        compute_device,
        "fused_gate_up_silu_infer_into expects input and gate_weight on the same device"
    );
    assert_eq!(
        up_weight.device(),
        compute_device,
        "fused_gate_up_silu_infer_into expects input and up_weight on the same device"
    );

    let x_shape = input.shape_vec();
    let k_dim = *x_shape.last().expect("input must have last dim");
    let n_dim = validate_gate_up_shapes(k_dim, gate_weight, up_weight);
    let m_dim = input.len() / k_dim;
    assert_eq!(
        m_dim, 1,
        "fused_gate_up_silu_infer_into currently expects single-token decode input"
    );
    assert_eq!(out.len(), n_dim, "output size mismatch");
    if compute_device == crate::autograd::Device::Cuda {
        let cuda_out = input.with_cuda_f32_buffer(|input_buf| {
            gate_weight.with_cuda_f32_buffer(|gate_buf| {
                up_weight.with_cuda_f32_buffer(|up_buf| {
                    cuda::fused_gate_up_silu_f32(input_buf, gate_buf, up_buf, m_dim, n_dim, k_dim)
                })
            })
        });
        if let Ok((_, host)) = cuda_out {
            out.copy_from_slice(&host);
            return;
        }
    }
    if gate_weight.dtype() == crate::precision::DType::I8
        && up_weight.dtype() == crate::precision::DType::I8
    {
        let gate_owned = gate_weight.native_storage_owned();
        let up_owned = up_weight.native_storage_owned();
        return with_decode_input_as_slice_ref(input, |x_slice| match (gate_owned, up_owned) {
            (
                TensorStorageOwned::I8(gate_data, gate_scale),
                TensorStorageOwned::I8(up_data, up_scale),
            ) => {
                let gate_2d = gate_data
                    .view()
                    .into_dimensionality::<Ix2>()
                    .expect("gate weight must be 2D [N, K]");
                let up_2d = up_data
                    .view()
                    .into_dimensionality::<Ix2>()
                    .expect("up weight must be 2D [N, K]");
                run_gate_up_slice(
                    x_slice,
                    SliceRef::I8(
                        gate_2d.as_slice().expect("gate weight must be contiguous"),
                        gate_scale,
                    ),
                    SliceRef::I8(
                        up_2d.as_slice().expect("up weight must be contiguous"),
                        up_scale,
                    ),
                    n_dim,
                    k_dim,
                    out,
                );
            }
            _ => unreachable!("checked I8 weights above"),
        });
    }
    with_decode_input_as_slice_ref(input, |x_slice| {
        let input_dtype = slice_ref_dtype(x_slice);
        gate_weight.with_storage_view_for_input_dtype(input_dtype, |gate_view| {
            up_weight.with_storage_view_for_input_dtype(input_dtype, |up_view| {
                macro_rules! run_gate_up_into {
                    ($gate_slice:expr, $up_slice:expr, $n_dim:expr) => {{
                        let n_dim = $n_dim;
                        let gate_slice = $gate_slice;
                        let up_slice = $up_slice;
                        assert_eq!(out.len(), n_dim, "output size mismatch");
                        run_gate_up_slice(x_slice, gate_slice, up_slice, n_dim, k_dim, out);
                    }};
                }

                match (gate_view, up_view) {
                    (TensorStorageView::F32(gate_view), TensorStorageView::F32(up_view)) => {
                        let gate_2d = gate_view
                            .into_dimensionality::<Ix2>()
                            .expect("gate weight must be 2D [N, K]");
                        let up_2d = up_view
                            .into_dimensionality::<Ix2>()
                            .expect("up weight must be 2D [N, K]");
                        run_gate_up_into!(
                            SliceRef::F32(
                                gate_2d.as_slice().expect("gate weight must be contiguous")
                            ),
                            SliceRef::F32(up_2d.as_slice().expect("up weight must be contiguous")),
                            n_dim
                        );
                    }
                    (TensorStorageView::F32(gate_view), TensorStorageView::BF16(up_view)) => {
                        let gate_2d = gate_view
                            .into_dimensionality::<Ix2>()
                            .expect("gate weight must be 2D [N, K]");
                        let up_2d = up_view
                            .into_dimensionality::<Ix2>()
                            .expect("up weight must be 2D [N, K]");
                        run_gate_up_into!(
                            SliceRef::F32(
                                gate_2d.as_slice().expect("gate weight must be contiguous")
                            ),
                            SliceRef::BF16(up_2d.as_slice().expect("up weight must be contiguous")),
                            n_dim
                        );
                    }
                    (TensorStorageView::BF16(gate_view), TensorStorageView::F32(up_view)) => {
                        let gate_2d = gate_view
                            .into_dimensionality::<Ix2>()
                            .expect("gate weight must be 2D [N, K]");
                        let up_2d = up_view
                            .into_dimensionality::<Ix2>()
                            .expect("up weight must be 2D [N, K]");
                        run_gate_up_into!(
                            SliceRef::BF16(
                                gate_2d.as_slice().expect("gate weight must be contiguous")
                            ),
                            SliceRef::F32(up_2d.as_slice().expect("up weight must be contiguous")),
                            n_dim
                        );
                    }
                    (TensorStorageView::BF16(gate_view), TensorStorageView::BF16(up_view)) => {
                        let gate_2d = gate_view
                            .into_dimensionality::<Ix2>()
                            .expect("gate weight must be 2D [N, K]");
                        let up_2d = up_view
                            .into_dimensionality::<Ix2>()
                            .expect("up weight must be 2D [N, K]");
                        run_gate_up_into!(
                            SliceRef::BF16(
                                gate_2d.as_slice().expect("gate weight must be contiguous")
                            ),
                            SliceRef::BF16(up_2d.as_slice().expect("up weight must be contiguous")),
                            n_dim
                        );
                    }
                    (gate_view, up_view) => {
                        let gate_slice = storage_view_2d_as_slice_ref(gate_view, "gate");
                        let up_slice = storage_view_2d_as_slice_ref(up_view, "up");
                        assert_eq!(gate_slice.cols(), k_dim, "gate weight K mismatch");
                        assert_eq!(up_slice.cols(), k_dim, "up weight K mismatch");
                        run_gate_up_into!(
                            gate_slice.as_slice_ref(),
                            up_slice.as_slice_ref(),
                            n_dim
                        );
                    }
                }
            })
        })
    });
}

pub fn fused_qkv_decode_infer_into(
    input: &Tensor,
    q_weight: &Tensor,
    k_weight: &Tensor,
    v_weight: &Tensor,
    q_out: &mut [f32],
    k_out: &mut [f32],
    v_out: &mut [f32],
) {
    assert!(
        is_no_grad(),
        "fused_qkv_decode_infer_into is inference-only"
    );
    let compute_device = input.device();
    assert_native_device_support(
        compute_device,
        "fused_qkv_decode_infer_into",
        compute_device == crate::autograd::Device::Cuda,
    );
    assert_eq!(
        q_weight.device(),
        compute_device,
        "fused_qkv_decode_infer_into expects input and q_weight on the same device"
    );
    assert_eq!(
        k_weight.device(),
        compute_device,
        "fused_qkv_decode_infer_into expects input and k_weight on the same device"
    );
    assert_eq!(
        v_weight.device(),
        compute_device,
        "fused_qkv_decode_infer_into expects input and v_weight on the same device"
    );

    let x_shape = input.shape_vec();
    assert_eq!(x_shape.len(), 3, "decode input must be [B, S, K]");
    let (b, s, k_dim) = (x_shape[0], x_shape[1], x_shape[2]);
    let (q_n, k_n, v_n) = validate_qkv_shapes(k_dim, q_weight, k_weight, v_weight);
    assert_eq!(s, 1, "fused_qkv_decode_infer_into only supports S=1 decode");
    assert_eq!(q_out.len(), b * q_n, "Q output size mismatch");
    assert_eq!(k_out.len(), b * k_n, "K output size mismatch");
    assert_eq!(v_out.len(), b * v_n, "V output size mismatch");
    if compute_device == crate::autograd::Device::Cuda && input.len() > 0 {
        let cuda_out = input.with_cuda_f32_buffer(|input_buf| {
            q_weight.with_cuda_f32_buffer(|q_buf| {
                k_weight.with_cuda_f32_buffer(|k_buf| {
                    v_weight.with_cuda_f32_buffer(|v_buf| {
                        cuda::fused_qkv_f32(input_buf, q_buf, k_buf, v_buf, b, q_n, k_n, k_dim)
                    })
                })
            })
        });
        if let Ok(((.., q_host), (.., k_host), (.., v_host))) = cuda_out {
            q_out.copy_from_slice(&q_host);
            k_out.copy_from_slice(&k_host);
            v_out.copy_from_slice(&v_host);
            return;
        }
    }
    if q_weight.dtype() == crate::precision::DType::I8
        && k_weight.dtype() == crate::precision::DType::I8
        && v_weight.dtype() == crate::precision::DType::I8
    {
        let q_owned = q_weight.native_storage_owned();
        let k_owned = k_weight.native_storage_owned();
        let v_owned = v_weight.native_storage_owned();
        return with_decode_input_as_slice_ref(input, |x_slice| {
            match (q_owned, k_owned, v_owned) {
                (
                    TensorStorageOwned::I8(q_data, q_scale),
                    TensorStorageOwned::I8(k_data, k_scale),
                    TensorStorageOwned::I8(v_data, v_scale),
                ) => {
                    let q_2d = q_data
                        .view()
                        .into_dimensionality::<Ix2>()
                        .expect("Q weight must be 2D [Nq, K]");
                    let k_2d = k_data
                        .view()
                        .into_dimensionality::<Ix2>()
                        .expect("K weight must be 2D [Nk, K]");
                    let v_2d = v_data
                        .view()
                        .into_dimensionality::<Ix2>()
                        .expect("V weight must be 2D [Nv, K]");
                    let q_slice = SliceRef::I8(
                        q_2d.as_slice().expect("Q weight must be contiguous"),
                        q_scale,
                    );
                    let k_slice = SliceRef::I8(
                        k_2d.as_slice().expect("K weight must be contiguous"),
                        k_scale,
                    );
                    let v_slice = SliceRef::I8(
                        v_2d.as_slice().expect("V weight must be contiguous"),
                        v_scale,
                    );
                    for row_idx in 0..b {
                        let input_start = row_idx * k_dim;
                        let q_start = row_idx * q_n;
                        let kv_start = row_idx * k_n;
                        run_qkv_slices(
                            match x_slice {
                                SliceRef::F32(slice) => {
                                    SliceRef::F32(&slice[input_start..input_start + k_dim])
                                }
                                SliceRef::F16(slice) => {
                                    SliceRef::F16(&slice[input_start..input_start + k_dim])
                                }
                                SliceRef::BF16(slice) => {
                                    SliceRef::BF16(&slice[input_start..input_start + k_dim])
                                }
                                SliceRef::I8(slice, scale) => {
                                    SliceRef::I8(&slice[input_start..input_start + k_dim], scale)
                                }
                            },
                            q_slice,
                            k_slice,
                            v_slice,
                            q_n,
                            k_n,
                            k_dim,
                            &mut q_out[q_start..q_start + q_n],
                            &mut k_out[kv_start..kv_start + k_n],
                            &mut v_out[kv_start..kv_start + v_n],
                        );
                    }
                }
                _ => unreachable!("checked I8 weights above"),
            }
        });
    }
    let input_dtype = input.dtype();
    q_weight.with_storage_view_for_input_dtype(input_dtype, |q_view| {
        k_weight.with_storage_view_for_input_dtype(input_dtype, |k_view| {
            v_weight.with_storage_view_for_input_dtype(input_dtype, |v_view| {
                macro_rules! run_qkv {
                    ($q_slice:expr, $k_slice:expr, $v_slice:expr, $q_n:expr, $k_n:expr, $v_n:expr) => {{
                        let q_n = $q_n;
                        let k_n = $k_n;
                        let v_n = $v_n;
                        let q_slice = $q_slice;
                        let k_slice = $k_slice;
                        let v_slice = $v_slice;
                        assert_eq!(q_out.len(), b * q_n, "Q output size mismatch");
                        assert_eq!(k_out.len(), b * k_n, "K output size mismatch");
                        assert_eq!(v_out.len(), b * v_n, "V output size mismatch");
                        assert_eq!(v_n, k_n, "K/V dim mismatch");
                        for_each_decode_input_as_slice_ref(input, b, k_dim, |row_idx, x_slice| {
                            let q_start = row_idx * q_n;
                            let kv_start = row_idx * k_n;
                            run_qkv_slices(
                                x_slice,
                                q_slice,
                                k_slice,
                                v_slice,
                                q_n,
                                k_n,
                                k_dim,
                                &mut q_out[q_start..q_start + q_n],
                                &mut k_out[kv_start..kv_start + k_n],
                                &mut v_out[kv_start..kv_start + v_n],
                            );
                        });
                    }};
                }

                let q_slice = storage_view_2d_as_slice_ref(q_view, "Q");
                let k_slice = storage_view_2d_as_slice_ref(k_view, "K");
                let v_slice = storage_view_2d_as_slice_ref(v_view, "V");
                assert_eq!(q_slice.cols(), k_dim, "Q weight K mismatch");
                assert_eq!(k_slice.cols(), k_dim, "K weight K mismatch");
                assert_eq!(v_slice.cols(), k_dim, "V weight K mismatch");
                run_qkv!(
                    q_slice.as_slice_ref(),
                    k_slice.as_slice_ref(),
                    v_slice.as_slice_ref(),
                    q_slice.rows(),
                    k_slice.rows(),
                    v_slice.rows()
                );
            })
        })
    });
}
pub fn fused_qkv_decode_infer(
    input: &Tensor,
    q_weight: &Tensor,
    k_weight: &Tensor,
    v_weight: &Tensor,
    n_head: usize,
    n_kv_head: usize,
) -> (Array3<f32>, Array3<f32>, Array3<f32>) {
    assert!(is_no_grad(), "fused_qkv_decode_infer is inference-only");
    let compute_device = input.device();
    assert_native_device_support(
        compute_device,
        "fused_qkv_decode_infer",
        compute_device == crate::autograd::Device::Cuda,
    );
    assert_eq!(
        q_weight.device(),
        compute_device,
        "fused_qkv_decode_infer expects input and q_weight on the same device"
    );
    assert_eq!(
        k_weight.device(),
        compute_device,
        "fused_qkv_decode_infer expects input and k_weight on the same device"
    );
    assert_eq!(
        v_weight.device(),
        compute_device,
        "fused_qkv_decode_infer expects input and v_weight on the same device"
    );

    let x_shape = input.shape_vec();
    assert_eq!(x_shape.len(), 3, "decode input must be [B, S, K]");
    let (b, s, k_dim) = (x_shape[0], x_shape[1], x_shape[2]);
    assert_eq!(s, 1, "fused_qkv_decode_infer only supports S=1 decode");
    assert!(n_head > 0, "n_head must be > 0");
    assert!(n_kv_head > 0, "n_kv_head must be > 0");
    let (q_n, k_n, v_n) = validate_qkv_shapes(k_dim, q_weight, k_weight, v_weight);
    assert_eq!(q_n % n_head, 0, "Q dim must be divisible by n_head");
    assert_eq!(k_n % n_kv_head, 0, "K dim must be divisible by n_kv_head");

    let d = q_n / n_head;
    assert_eq!(k_n / n_kv_head, d, "Q/K head dim mismatch");

    if compute_device == crate::autograd::Device::Cuda && input.len() > 0 {
        let cuda_out = input.with_cuda_f32_buffer(|input_buf| {
            q_weight.with_cuda_f32_buffer(|q_buf| {
                k_weight.with_cuda_f32_buffer(|k_buf| {
                    v_weight.with_cuda_f32_buffer(|v_buf| {
                        cuda::fused_qkv_f32(input_buf, q_buf, k_buf, v_buf, b, q_n, k_n, k_dim)
                    })
                })
            })
        });
        if let Ok(((.., q_host), (.., k_host), (.., v_host))) = cuda_out {
            return (
                Array3::from_shape_vec((b, n_head, d), q_host)
                    .expect("decode Q shape build failed"),
                Array3::from_shape_vec((b, n_kv_head, d), k_host)
                    .expect("decode K shape build failed"),
                Array3::from_shape_vec((b, n_kv_head, d), v_host)
                    .expect("decode V shape build failed"),
            );
        }
    }

    if b == 1 {
        let mut q_out = vec![0.0f32; q_n];
        let mut k_out = vec![0.0f32; k_n];
        let mut v_out = vec![0.0f32; v_n];
        fused_qkv_decode_infer_into(
            input, q_weight, k_weight, v_weight, &mut q_out, &mut k_out, &mut v_out,
        );
        let q = Array3::from_shape_vec((1, n_head, d), q_out).expect("decode Q shape build failed");
        let k =
            Array3::from_shape_vec((1, n_kv_head, d), k_out).expect("decode K shape build failed");
        let v =
            Array3::from_shape_vec((1, n_kv_head, d), v_out).expect("decode V shape build failed");
        return (q, k, v);
    }

    let mut q_out = Array2::<f32>::zeros((b, q_n));
    let mut k_out = Array2::<f32>::zeros((b, k_n));
    let mut v_out = Array2::<f32>::zeros((b, v_n));
    if q_weight.dtype() == crate::precision::DType::I8
        && k_weight.dtype() == crate::precision::DType::I8
        && v_weight.dtype() == crate::precision::DType::I8
    {
        let q_owned = q_weight.native_storage_owned();
        let k_owned = k_weight.native_storage_owned();
        let v_owned = v_weight.native_storage_owned();
        input.with_storage_view_preferring(StoragePreference::Native, |x_view| {
            match (q_owned, k_owned, v_owned, x_view) {
                (
                    TensorStorageOwned::I8(q_data, q_scale),
                    TensorStorageOwned::I8(k_data, k_scale),
                    TensorStorageOwned::I8(v_data, v_scale),
                    TensorStorageView::F32(x_view),
                ) => {
                    let q_2d = q_data
                        .view()
                        .into_dimensionality::<Ix2>()
                        .expect("Q weight must be 2D [Nq, K]");
                    let k_2d = k_data
                        .view()
                        .into_dimensionality::<Ix2>()
                        .expect("K weight must be 2D [Nk, K]");
                    let v_2d = v_data
                        .view()
                        .into_dimensionality::<Ix2>()
                        .expect("V weight must be 2D [Nv, K]");
                    for_each_decode_input_row(
                        TensorStorageView::F32(x_view),
                        b,
                        k_dim,
                        |bb, x_slice| {
                            let mut q_row_view = q_out.slice_mut(ndarray::s![bb, ..]);
                            let mut k_row_view = k_out.slice_mut(ndarray::s![bb, ..]);
                            let mut v_row_view = v_out.slice_mut(ndarray::s![bb, ..]);
                            run_qkv_slices(
                                x_slice,
                                SliceRef::I8(
                                    q_2d.as_slice().expect("Q weight must be contiguous"),
                                    q_scale,
                                ),
                                SliceRef::I8(
                                    k_2d.as_slice().expect("K weight must be contiguous"),
                                    k_scale,
                                ),
                                SliceRef::I8(
                                    v_2d.as_slice().expect("V weight must be contiguous"),
                                    v_scale,
                                ),
                                q_n,
                                k_n,
                                k_dim,
                                q_row_view
                                    .as_slice_mut()
                                    .expect("Q output row not contiguous"),
                                k_row_view
                                    .as_slice_mut()
                                    .expect("K output row not contiguous"),
                                v_row_view
                                    .as_slice_mut()
                                    .expect("V output row not contiguous"),
                            );
                        },
                    );
                }
                (
                    TensorStorageOwned::I8(q_data, q_scale),
                    TensorStorageOwned::I8(k_data, k_scale),
                    TensorStorageOwned::I8(v_data, v_scale),
                    TensorStorageView::F16(x_view),
                ) => {
                    let q_2d = q_data
                        .view()
                        .into_dimensionality::<Ix2>()
                        .expect("Q weight must be 2D [Nq, K]");
                    let k_2d = k_data
                        .view()
                        .into_dimensionality::<Ix2>()
                        .expect("K weight must be 2D [Nk, K]");
                    let v_2d = v_data
                        .view()
                        .into_dimensionality::<Ix2>()
                        .expect("V weight must be 2D [Nv, K]");
                    for_each_decode_input_row(
                        TensorStorageView::F16(x_view),
                        b,
                        k_dim,
                        |bb, x_slice| {
                            let mut q_row_view = q_out.slice_mut(ndarray::s![bb, ..]);
                            let mut k_row_view = k_out.slice_mut(ndarray::s![bb, ..]);
                            let mut v_row_view = v_out.slice_mut(ndarray::s![bb, ..]);
                            run_qkv_slices(
                                x_slice,
                                SliceRef::I8(
                                    q_2d.as_slice().expect("Q weight must be contiguous"),
                                    q_scale,
                                ),
                                SliceRef::I8(
                                    k_2d.as_slice().expect("K weight must be contiguous"),
                                    k_scale,
                                ),
                                SliceRef::I8(
                                    v_2d.as_slice().expect("V weight must be contiguous"),
                                    v_scale,
                                ),
                                q_n,
                                k_n,
                                k_dim,
                                q_row_view
                                    .as_slice_mut()
                                    .expect("Q output row not contiguous"),
                                k_row_view
                                    .as_slice_mut()
                                    .expect("K output row not contiguous"),
                                v_row_view
                                    .as_slice_mut()
                                    .expect("V output row not contiguous"),
                            );
                        },
                    );
                }
                (
                    TensorStorageOwned::I8(q_data, q_scale),
                    TensorStorageOwned::I8(k_data, k_scale),
                    TensorStorageOwned::I8(v_data, v_scale),
                    TensorStorageView::BF16(x_view),
                ) => {
                    let q_2d = q_data
                        .view()
                        .into_dimensionality::<Ix2>()
                        .expect("Q weight must be 2D [Nq, K]");
                    let k_2d = k_data
                        .view()
                        .into_dimensionality::<Ix2>()
                        .expect("K weight must be 2D [Nk, K]");
                    let v_2d = v_data
                        .view()
                        .into_dimensionality::<Ix2>()
                        .expect("V weight must be 2D [Nv, K]");
                    for_each_decode_input_row(
                        TensorStorageView::BF16(x_view),
                        b,
                        k_dim,
                        |bb, x_slice| {
                            let mut q_row_view = q_out.slice_mut(ndarray::s![bb, ..]);
                            let mut k_row_view = k_out.slice_mut(ndarray::s![bb, ..]);
                            let mut v_row_view = v_out.slice_mut(ndarray::s![bb, ..]);
                            run_qkv_slices(
                                x_slice,
                                SliceRef::I8(
                                    q_2d.as_slice().expect("Q weight must be contiguous"),
                                    q_scale,
                                ),
                                SliceRef::I8(
                                    k_2d.as_slice().expect("K weight must be contiguous"),
                                    k_scale,
                                ),
                                SliceRef::I8(
                                    v_2d.as_slice().expect("V weight must be contiguous"),
                                    v_scale,
                                ),
                                q_n,
                                k_n,
                                k_dim,
                                q_row_view
                                    .as_slice_mut()
                                    .expect("Q output row not contiguous"),
                                k_row_view
                                    .as_slice_mut()
                                    .expect("K output row not contiguous"),
                                v_row_view
                                    .as_slice_mut()
                                    .expect("V output row not contiguous"),
                            );
                        },
                    );
                }
                _ => unreachable!("checked I8 weights above"),
            }
        });

        return (
            q_out
                .into_shape((b, n_head, d))
                .expect("Q output reshape failed"),
            k_out
                .into_shape((b, n_kv_head, d))
                .expect("K output reshape failed"),
            v_out
                .into_shape((b, n_kv_head, d))
                .expect("V output reshape failed"),
        );
    }
    input.with_storage_view_preferring(StoragePreference::Native, |x_view| {
        let input_dtype = match &x_view {
            TensorStorageView::F32(_) => DType::F32,
            TensorStorageView::F16(_) => DType::F16,
            TensorStorageView::BF16(_) => DType::BF16,
        };
        q_weight.with_storage_view_for_input_dtype(input_dtype, |q_view| {
            k_weight.with_storage_view_for_input_dtype(input_dtype, |k_view| {
                v_weight.with_storage_view_for_input_dtype(input_dtype, |v_view| {
                    macro_rules! run_qkv_rows {
                        ($q_slice:expr, $k_slice:expr, $v_slice:expr) => {{
                            let q_slice = $q_slice;
                            let k_slice = $k_slice;
                            let v_slice = $v_slice;
                            for_each_decode_input_row(x_view, b, k_dim, |bb, x_slice| {
                                let mut q_row_view = q_out.slice_mut(ndarray::s![bb, ..]);
                                let q_out_slice = q_row_view
                                    .as_slice_mut()
                                    .expect("Q output row not contiguous");
                                let mut k_row_view = k_out.slice_mut(ndarray::s![bb, ..]);
                                let k_out_slice = k_row_view
                                    .as_slice_mut()
                                    .expect("K output row not contiguous");
                                let mut v_row_view = v_out.slice_mut(ndarray::s![bb, ..]);
                                let v_out_slice = v_row_view
                                    .as_slice_mut()
                                    .expect("V output row not contiguous");
                                run_qkv_slices(
                                    x_slice,
                                    q_slice,
                                    k_slice,
                                    v_slice,
                                    q_n,
                                    k_n,
                                    k_dim,
                                    q_out_slice,
                                    k_out_slice,
                                    v_out_slice,
                                );
                            });
                        }};
                    }

                    let q_slice = storage_view_2d_as_slice_ref(q_view, "Q");
                    let k_slice = storage_view_2d_as_slice_ref(k_view, "K");
                    let v_slice = storage_view_2d_as_slice_ref(v_view, "V");
                    assert_eq!(q_slice.cols(), k_dim, "Q weight K mismatch");
                    assert_eq!(k_slice.cols(), k_dim, "K weight K mismatch");
                    assert_eq!(v_slice.cols(), k_dim, "V weight K mismatch");
                    run_qkv_rows!(
                        q_slice.as_slice_ref(),
                        k_slice.as_slice_ref(),
                        v_slice.as_slice_ref()
                    );
                })
            })
        })
    });

    (
        q_out
            .into_shape((b, n_head, d))
            .expect("Q output reshape failed"),
        k_out
            .into_shape((b, n_kv_head, d))
            .expect("K output reshape failed"),
        v_out
            .into_shape((b, n_kv_head, d))
            .expect("V output reshape failed"),
    )
}

fn fused_qkv_tensor_output_dtype(
    input: &Tensor,
    q_weight: &Tensor,
    k_weight: &Tensor,
    v_weight: &Tensor,
) -> DType {
    if input.dtype() == q_weight.dtype()
        && input.dtype() == k_weight.dtype()
        && input.dtype() == v_weight.dtype()
        && matches!(input.dtype(), DType::F16 | DType::BF16)
    {
        input.dtype()
    } else {
        DType::F32
    }
}

pub fn fused_qkv_decode_infer_tensors(
    input: &Tensor,
    q_weight: &Tensor,
    k_weight: &Tensor,
    v_weight: &Tensor,
    n_head: usize,
    n_kv_head: usize,
) -> (Tensor, Tensor, Tensor) {
    assert!(
        is_no_grad(),
        "fused_qkv_decode_infer_tensors is inference-only"
    );
    let compute_device = input.device();
    assert_eq!(
        q_weight.device(),
        compute_device,
        "fused_qkv_decode_infer_tensors expects input and q_weight on the same device"
    );
    assert_eq!(
        k_weight.device(),
        compute_device,
        "fused_qkv_decode_infer_tensors expects input and k_weight on the same device"
    );
    assert_eq!(
        v_weight.device(),
        compute_device,
        "fused_qkv_decode_infer_tensors expects input and v_weight on the same device"
    );

    let x_shape = input.shape_vec();
    assert_eq!(x_shape.len(), 3, "decode input must be [B, S, K]");
    let (b, s, k_dim) = (x_shape[0], x_shape[1], x_shape[2]);
    assert_eq!(
        s, 1,
        "fused_qkv_decode_infer_tensors only supports S=1 decode"
    );
    let (q_n, k_n, v_n) = validate_qkv_shapes(k_dim, q_weight, k_weight, v_weight);
    assert_eq!(q_n % n_head, 0, "Q dim must be divisible by n_head");
    assert_eq!(k_n % n_kv_head, 0, "K dim must be divisible by n_kv_head");
    let d = q_n / n_head;
    assert_eq!(k_n / n_kv_head, d, "Q/K head dim mismatch");
    assert_eq!(v_n / n_kv_head, d, "V/K head dim mismatch");
    let output_dtype = fused_qkv_tensor_output_dtype(input, q_weight, k_weight, v_weight);

    if compute_device == crate::autograd::Device::Cuda && input.len() > 0 {
        let cuda_out = input.with_cuda_f32_buffer(|input_buf| {
            q_weight.with_cuda_f32_buffer(|q_buf| {
                k_weight.with_cuda_f32_buffer(|k_buf| {
                    v_weight.with_cuda_f32_buffer(|v_buf| {
                        cuda::fused_qkv_f32_buffer(
                            input_buf, q_buf, k_buf, v_buf, b, q_n, k_n, k_dim,
                        )
                    })
                })
            })
        });
        if let Ok((q_buf, k_buf, v_buf)) = cuda_out {
            let q = Tensor::from_cuda_f32_buffer_no_host_with_dtype(
                &[b, n_head, 1, d],
                q_buf,
                compute_device,
                output_dtype,
            );
            let k = Tensor::from_cuda_f32_buffer_no_host_with_dtype(
                &[b, n_kv_head, 1, d],
                k_buf,
                compute_device,
                output_dtype,
            );
            let v = Tensor::from_cuda_f32_buffer_no_host_with_dtype(
                &[b, n_kv_head, 1, d],
                v_buf,
                compute_device,
                output_dtype,
            );
            return (q, k, v);
        }
    }

    let (q, k, v) = fused_qkv_decode_infer(input, q_weight, k_weight, v_weight, n_head, n_kv_head);
    (
        Tensor::from_f32_data_no_grad_with_device_dtype(
            q.into_shape((b, n_head, 1, d))
                .expect("decode Q tensor reshape failed")
                .into_dyn(),
            output_dtype,
            compute_device,
        ),
        Tensor::from_f32_data_no_grad_with_device_dtype(
            k.into_shape((b, n_kv_head, 1, d))
                .expect("decode K tensor reshape failed")
                .into_dyn(),
            output_dtype,
            compute_device,
        ),
        Tensor::from_f32_data_no_grad_with_device_dtype(
            v.into_shape((b, n_kv_head, 1, d))
                .expect("decode V tensor reshape failed")
                .into_dyn(),
            output_dtype,
            compute_device,
        ),
    )
}

pub fn fused_qkv_prefill_infer_tensors(
    input: &Tensor,
    q_weight: &Tensor,
    k_weight: &Tensor,
    v_weight: &Tensor,
    n_head: usize,
    n_kv_head: usize,
) -> Option<(Tensor, Tensor, Tensor)> {
    assert!(
        is_no_grad(),
        "fused_qkv_prefill_infer_tensors is inference-only"
    );
    let compute_device = input.device();
    if compute_device != crate::autograd::Device::Cuda {
        return None;
    }
    if q_weight.device() != compute_device
        || k_weight.device() != compute_device
        || v_weight.device() != compute_device
    {
        return None;
    }
    let x_shape = input.shape_vec();
    if x_shape.len() != 3 {
        return None;
    }
    let (b, s, k_dim) = (x_shape[0], x_shape[1], x_shape[2]);
    let (q_n, k_n, v_n) = validate_qkv_shapes(k_dim, q_weight, k_weight, v_weight);
    if q_n % n_head != 0 || k_n % n_kv_head != 0 || v_n != k_n {
        return None;
    }
    let d = q_n / n_head;
    if k_n / n_kv_head != d {
        return None;
    }
    let output_dtype = fused_qkv_tensor_output_dtype(input, q_weight, k_weight, v_weight);
    if b == 0 || s == 0 {
        let q = Tensor::from_f32_data_no_grad_with_device_dtype(
            Array::zeros(ndarray::IxDyn(&[b, n_head, s, d])).into_dyn(),
            output_dtype,
            compute_device,
        );
        let k = Tensor::from_f32_data_no_grad_with_device_dtype(
            Array::zeros(ndarray::IxDyn(&[b, n_kv_head, s, d])).into_dyn(),
            output_dtype,
            compute_device,
        );
        let v = Tensor::from_f32_data_no_grad_with_device_dtype(
            Array::zeros(ndarray::IxDyn(&[b, n_kv_head, s, d])).into_dyn(),
            output_dtype,
            compute_device,
        );
        return Some((q, k, v));
    }

    let rows = b.checked_mul(s)?;
    let cuda_out = input.with_cuda_f32_buffer(|input_buf| {
        q_weight.with_cuda_f32_buffer(|q_buf| {
            k_weight.with_cuda_f32_buffer(|k_buf| {
                v_weight.with_cuda_f32_buffer(|v_buf| {
                    cuda::fused_qkv_f32_buffer(
                        input_buf, q_buf, k_buf, v_buf, rows, q_n, k_n, k_dim,
                    )
                })
            })
        })
    });
    let (q_bshd, k_bshd, v_bshd) = cuda_out.ok()?;
    let q_buf = cuda::permute_f32_buffer(&q_bshd, &[b, n_head, s, d], &[0, 2, 1, 3]).ok()?;
    let k_buf = cuda::permute_f32_buffer(&k_bshd, &[b, n_kv_head, s, d], &[0, 2, 1, 3]).ok()?;
    let v_buf = cuda::permute_f32_buffer(&v_bshd, &[b, n_kv_head, s, d], &[0, 2, 1, 3]).ok()?;

    Some((
        Tensor::from_cuda_f32_buffer_no_host_with_dtype(
            &[b, n_head, s, d],
            q_buf,
            compute_device,
            output_dtype,
        ),
        Tensor::from_cuda_f32_buffer_no_host_with_dtype(
            &[b, n_kv_head, s, d],
            k_buf,
            compute_device,
            output_dtype,
        ),
        Tensor::from_cuda_f32_buffer_no_host_with_dtype(
            &[b, n_kv_head, s, d],
            v_buf,
            compute_device,
            output_dtype,
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autograd::no_grad;
    use crate::precision::DType;
    use ndarray::IxDyn;

    fn sample_f32(len: usize) -> Vec<f32> {
        (0..len)
            .map(|i| (((i * 19 + 7) % 31) as f32) / 15.0 - 1.0)
            .collect()
    }

    fn quantize_bf16(src: &[f32]) -> Vec<f32> {
        src.iter()
            .map(|&v| half::bf16::from_f32(v).to_f32())
            .collect()
    }

    fn quantize_f16(src: &[f32]) -> Vec<f32> {
        src.iter()
            .map(|&v| half::f16::from_f32(v).to_f32())
            .collect()
    }

    fn quantize_i8(shape: &[usize], src: &[f32]) -> Vec<f32> {
        let t = make_tensor(shape, src.to_vec(), DType::I8);
        t.data_ref().iter().copied().collect()
    }

    fn make_tensor(shape: &[usize], data: Vec<f32>, dtype: DType) -> Tensor {
        let t = Tensor::from_array_no_grad(
            Array::from_shape_vec(IxDyn(shape), data)
                .unwrap()
                .into_dyn(),
        );
        t.cast_inplace(dtype);
        t
    }

    fn assert_close(lhs: &[f32], rhs: &[f32], tol: f32) {
        assert_eq!(lhs.len(), rhs.len());
        for (idx, (&a, &b)) in lhs.iter().zip(rhs.iter()).enumerate() {
            assert!(
                (a - b).abs() <= tol,
                "idx={idx}, lhs={a}, rhs={b}, tol={tol}"
            );
        }
    }

    #[test]
    fn fused_qkv_bf16_matches_quantized_reference() {
        let hidden = 8usize;
        let x = sample_f32(hidden);
        let q = sample_f32(hidden * hidden);
        let k = sample_f32(hidden * hidden)
            .into_iter()
            .map(|v| v * 0.7 - 0.1)
            .collect::<Vec<_>>();
        let v = sample_f32(hidden * hidden)
            .into_iter()
            .map(|v| v * -0.4 + 0.05)
            .collect::<Vec<_>>();

        let x_q = quantize_bf16(&x);
        let q_q = quantize_bf16(&q);
        let k_q = quantize_bf16(&k);
        let v_q = quantize_bf16(&v);

        let input_f32 = make_tensor(&[1, 1, hidden], x_q.clone(), DType::F32);
        let input_bf16 = make_tensor(&[1, 1, hidden], x.clone(), DType::BF16);
        let q_f32 = make_tensor(&[hidden, hidden], q_q.clone(), DType::F32);
        let k_f32 = make_tensor(&[hidden, hidden], k_q.clone(), DType::F32);
        let v_f32 = make_tensor(&[hidden, hidden], v_q.clone(), DType::F32);
        let q_bf16 = make_tensor(&[hidden, hidden], q.clone(), DType::BF16);
        let k_bf16 = make_tensor(&[hidden, hidden], k.clone(), DType::BF16);
        let v_bf16 = make_tensor(&[hidden, hidden], v.clone(), DType::BF16);

        let mut q_ref = vec![0.0f32; hidden];
        let mut k_ref = vec![0.0f32; hidden];
        let mut v_ref = vec![0.0f32; hidden];
        let mut q_out = vec![0.0f32; hidden];
        let mut k_out = vec![0.0f32; hidden];
        let mut v_out = vec![0.0f32; hidden];

        no_grad(|| {
            fused_qkv_decode_infer_into(
                &input_f32, &q_f32, &k_f32, &v_f32, &mut q_ref, &mut k_ref, &mut v_ref,
            );
            fused_qkv_decode_infer_into(
                &input_bf16,
                &q_bf16,
                &k_bf16,
                &v_bf16,
                &mut q_out,
                &mut k_out,
                &mut v_out,
            );
        });

        assert_close(&q_ref, &q_out, 1e-4);
        assert_close(&k_ref, &k_out, 1e-4);
        assert_close(&v_ref, &v_out, 1e-4);
    }

    #[test]
    fn fused_qkv_decode_infer_bf16_matches_quantized_reference() {
        let hidden = 8usize;
        let x = sample_f32(hidden);
        let q = sample_f32(hidden * hidden);
        let k = sample_f32(hidden * hidden)
            .into_iter()
            .map(|v| v * 0.7 - 0.1)
            .collect::<Vec<_>>();
        let v = sample_f32(hidden * hidden)
            .into_iter()
            .map(|v| v * -0.4 + 0.05)
            .collect::<Vec<_>>();

        let x_q = quantize_bf16(&x);
        let q_q = quantize_bf16(&q);
        let k_q = quantize_bf16(&k);
        let v_q = quantize_bf16(&v);

        let input_f32 = make_tensor(&[1, 1, hidden], x_q.clone(), DType::F32);
        let input_bf16 = make_tensor(&[1, 1, hidden], x.clone(), DType::BF16);
        let q_f32 = make_tensor(&[hidden, hidden], q_q.clone(), DType::F32);
        let k_f32 = make_tensor(&[hidden, hidden], k_q.clone(), DType::F32);
        let v_f32 = make_tensor(&[hidden, hidden], v_q.clone(), DType::F32);
        let q_bf16 = make_tensor(&[hidden, hidden], q.clone(), DType::BF16);
        let k_bf16 = make_tensor(&[hidden, hidden], k.clone(), DType::BF16);
        let v_bf16 = make_tensor(&[hidden, hidden], v.clone(), DType::BF16);

        let (q_ref, k_ref, v_ref) =
            no_grad(|| fused_qkv_decode_infer(&input_f32, &q_f32, &k_f32, &v_f32, 1, 1));
        let (q_out, k_out, v_out) =
            no_grad(|| fused_qkv_decode_infer(&input_bf16, &q_bf16, &k_bf16, &v_bf16, 1, 1));

        assert_close(q_ref.as_slice().unwrap(), q_out.as_slice().unwrap(), 1e-4);
        assert_close(k_ref.as_slice().unwrap(), k_out.as_slice().unwrap(), 1e-4);
        assert_close(v_ref.as_slice().unwrap(), v_out.as_slice().unwrap(), 1e-4);
    }

    #[test]
    fn fused_qkv_decode_infer_batch_bf16_matches_quantized_reference() {
        let batch = 2usize;
        let hidden = 8usize;
        let x = sample_f32(batch * hidden)
            .into_iter()
            .enumerate()
            .map(|(i, v)| v + (i / hidden) as f32 * 0.1)
            .collect::<Vec<_>>();
        let q = sample_f32(hidden * hidden);
        let k = sample_f32(hidden * hidden)
            .into_iter()
            .map(|v| v * 0.7 - 0.1)
            .collect::<Vec<_>>();
        let v = sample_f32(hidden * hidden)
            .into_iter()
            .map(|v| v * -0.4 + 0.05)
            .collect::<Vec<_>>();

        let x_q = quantize_bf16(&x);
        let q_q = quantize_bf16(&q);
        let k_q = quantize_bf16(&k);
        let v_q = quantize_bf16(&v);

        let input_f32 = make_tensor(&[batch, 1, hidden], x_q.clone(), DType::F32);
        let input_bf16 = make_tensor(&[batch, 1, hidden], x.clone(), DType::BF16);
        let q_f32 = make_tensor(&[hidden, hidden], q_q.clone(), DType::F32);
        let k_f32 = make_tensor(&[hidden, hidden], k_q.clone(), DType::F32);
        let v_f32 = make_tensor(&[hidden, hidden], v_q.clone(), DType::F32);
        let q_bf16 = make_tensor(&[hidden, hidden], q.clone(), DType::BF16);
        let k_bf16 = make_tensor(&[hidden, hidden], k.clone(), DType::BF16);
        let v_bf16 = make_tensor(&[hidden, hidden], v.clone(), DType::BF16);

        let (q_ref, k_ref, v_ref) =
            no_grad(|| fused_qkv_decode_infer(&input_f32, &q_f32, &k_f32, &v_f32, 1, 1));
        let (q_out, k_out, v_out) =
            no_grad(|| fused_qkv_decode_infer(&input_bf16, &q_bf16, &k_bf16, &v_bf16, 1, 1));

        assert_close(q_ref.as_slice().unwrap(), q_out.as_slice().unwrap(), 1e-4);
        assert_close(k_ref.as_slice().unwrap(), k_out.as_slice().unwrap(), 1e-4);
        assert_close(v_ref.as_slice().unwrap(), v_out.as_slice().unwrap(), 1e-4);
    }

    #[test]
    fn fused_qkv_decode_infer_into_accepts_batch_input() {
        let batch = 2usize;
        let hidden = 8usize;
        let x = sample_f32(batch * hidden)
            .into_iter()
            .enumerate()
            .map(|(i, v)| v + (i / hidden) as f32 * 0.15)
            .collect::<Vec<_>>();
        let q = sample_f32(hidden * hidden);
        let k = sample_f32(hidden * hidden)
            .into_iter()
            .map(|v| v * 0.7 - 0.1)
            .collect::<Vec<_>>();
        let v = sample_f32(hidden * hidden)
            .into_iter()
            .map(|v| v * -0.4 + 0.05)
            .collect::<Vec<_>>();

        let input = make_tensor(&[batch, 1, hidden], x, DType::BF16);
        let q_w = make_tensor(&[hidden, hidden], q, DType::BF16);
        let k_w = make_tensor(&[hidden, hidden], k, DType::BF16);
        let v_w = make_tensor(&[hidden, hidden], v, DType::BF16);

        let (q_ref, k_ref, v_ref) =
            no_grad(|| fused_qkv_decode_infer(&input, &q_w, &k_w, &v_w, 1, 1));
        let mut q_out = vec![0.0f32; batch * hidden];
        let mut k_out = vec![0.0f32; batch * hidden];
        let mut v_out = vec![0.0f32; batch * hidden];
        no_grad(|| {
            fused_qkv_decode_infer_into(
                &input, &q_w, &k_w, &v_w, &mut q_out, &mut k_out, &mut v_out,
            )
        });

        assert_close(q_ref.as_slice().unwrap(), &q_out, 1e-4);
        assert_close(k_ref.as_slice().unwrap(), &k_out, 1e-4);
        assert_close(v_ref.as_slice().unwrap(), &v_out, 1e-4);
    }

    #[test]
    fn fused_qkv_decode_infer_batch_accepts_non_contiguous_input() {
        let batch = 2usize;
        let hidden = 8usize;
        let x = sample_f32(batch * hidden)
            .into_iter()
            .enumerate()
            .map(|(i, v)| v + (i / hidden) as f32 * 0.2)
            .collect::<Vec<_>>();
        let q = sample_f32(hidden * hidden);
        let k = sample_f32(hidden * hidden)
            .into_iter()
            .map(|v| v * 0.7 - 0.1)
            .collect::<Vec<_>>();
        let v = sample_f32(hidden * hidden)
            .into_iter()
            .map(|v| v * -0.4 + 0.05)
            .collect::<Vec<_>>();

        let input_contig = make_tensor(&[batch, 1, hidden], x.clone(), DType::F32);
        let input_non_contig = Tensor::from_array_no_grad(
            Array3::from_shape_vec((1, batch, hidden), x)
                .expect("input shape")
                .permuted_axes([1, 0, 2])
                .into_dyn(),
        );
        let q_f32 = make_tensor(&[hidden, hidden], q, DType::F32);
        let k_f32 = make_tensor(&[hidden, hidden], k, DType::F32);
        let v_f32 = make_tensor(&[hidden, hidden], v, DType::F32);

        let (q_ref, k_ref, v_ref) =
            no_grad(|| fused_qkv_decode_infer(&input_contig, &q_f32, &k_f32, &v_f32, 1, 1));
        let (q_out, k_out, v_out) =
            no_grad(|| fused_qkv_decode_infer(&input_non_contig, &q_f32, &k_f32, &v_f32, 1, 1));

        assert_close(q_ref.as_slice().unwrap(), q_out.as_slice().unwrap(), 1e-5);
        assert_close(k_ref.as_slice().unwrap(), k_out.as_slice().unwrap(), 1e-5);
        assert_close(v_ref.as_slice().unwrap(), v_out.as_slice().unwrap(), 1e-5);
    }

    #[test]
    fn fused_qkv_f16_matches_quantized_reference() {
        let hidden = 8usize;
        let x = sample_f32(hidden);
        let q = sample_f32(hidden * hidden);
        let k = sample_f32(hidden * hidden)
            .into_iter()
            .map(|v| v * 0.7 - 0.1)
            .collect::<Vec<_>>();
        let v = sample_f32(hidden * hidden)
            .into_iter()
            .map(|v| v * -0.4 + 0.05)
            .collect::<Vec<_>>();

        let x_q = quantize_f16(&x);
        let q_q = quantize_f16(&q);
        let k_q = quantize_f16(&k);
        let v_q = quantize_f16(&v);

        let input_f32 = make_tensor(&[1, 1, hidden], x_q.clone(), DType::F32);
        let input_f16 = make_tensor(&[1, 1, hidden], x.clone(), DType::F16);
        let q_f32 = make_tensor(&[hidden, hidden], q_q.clone(), DType::F32);
        let k_f32 = make_tensor(&[hidden, hidden], k_q.clone(), DType::F32);
        let v_f32 = make_tensor(&[hidden, hidden], v_q.clone(), DType::F32);
        let q_f16 = make_tensor(&[hidden, hidden], q.clone(), DType::F16);
        let k_f16 = make_tensor(&[hidden, hidden], k.clone(), DType::F16);
        let v_f16 = make_tensor(&[hidden, hidden], v.clone(), DType::F16);

        let mut q_ref = vec![0.0f32; hidden];
        let mut k_ref = vec![0.0f32; hidden];
        let mut v_ref = vec![0.0f32; hidden];
        let mut q_out = vec![0.0f32; hidden];
        let mut k_out = vec![0.0f32; hidden];
        let mut v_out = vec![0.0f32; hidden];

        no_grad(|| {
            fused_qkv_decode_infer_into(
                &input_f32, &q_f32, &k_f32, &v_f32, &mut q_ref, &mut k_ref, &mut v_ref,
            );
            fused_qkv_decode_infer_into(
                &input_f16, &q_f16, &k_f16, &v_f16, &mut q_out, &mut k_out, &mut v_out,
            );
        });

        assert_close(&q_ref, &q_out, 1e-4);
        assert_close(&k_ref, &k_out, 1e-4);
        assert_close(&v_ref, &v_out, 1e-4);
    }

    #[test]
    fn fused_gateup_bf16_matches_quantized_reference() {
        let hidden = 8usize;
        let inter = 12usize;
        let x = sample_f32(hidden);
        let gate = sample_f32(inter * hidden);
        let up = sample_f32(inter * hidden)
            .into_iter()
            .map(|v| v * 0.5 - 0.2)
            .collect::<Vec<_>>();

        let x_q = quantize_bf16(&x);
        let gate_q = quantize_bf16(&gate);
        let up_q = quantize_bf16(&up);

        let input_f32 = make_tensor(&[1, 1, hidden], x_q.clone(), DType::F32);
        let input_bf16 = make_tensor(&[1, 1, hidden], x.clone(), DType::BF16);
        let gate_f32 = make_tensor(&[inter, hidden], gate_q.clone(), DType::F32);
        let up_f32 = make_tensor(&[inter, hidden], up_q.clone(), DType::F32);
        let gate_bf16 = make_tensor(&[inter, hidden], gate.clone(), DType::BF16);
        let up_bf16 = make_tensor(&[inter, hidden], up.clone(), DType::BF16);

        let mut ref_out = vec![0.0f32; inter];
        let mut out = vec![0.0f32; inter];

        no_grad(|| {
            fused_gate_up_silu_infer_into(&input_f32, &gate_f32, &up_f32, &mut ref_out);
            fused_gate_up_silu_infer_into(&input_bf16, &gate_bf16, &up_bf16, &mut out);
        });

        assert_close(&ref_out, &out, 1e-3);
    }

    #[test]
    fn fused_gateup_f16_matches_quantized_reference() {
        let hidden = 8usize;
        let inter = 12usize;
        let x = sample_f32(hidden);
        let gate = sample_f32(inter * hidden);
        let up = sample_f32(inter * hidden)
            .into_iter()
            .map(|v| v * 0.5 - 0.2)
            .collect::<Vec<_>>();

        let x_q = quantize_f16(&x);
        let gate_q = quantize_f16(&gate);
        let up_q = quantize_f16(&up);

        let input_f32 = make_tensor(&[1, 1, hidden], x_q.clone(), DType::F32);
        let input_f16 = make_tensor(&[1, 1, hidden], x.clone(), DType::F16);
        let gate_f32 = make_tensor(&[inter, hidden], gate_q.clone(), DType::F32);
        let up_f32 = make_tensor(&[inter, hidden], up_q.clone(), DType::F32);
        let gate_f16 = make_tensor(&[inter, hidden], gate.clone(), DType::F16);
        let up_f16 = make_tensor(&[inter, hidden], up.clone(), DType::F16);

        let mut ref_out = vec![0.0f32; inter];
        let mut out = vec![0.0f32; inter];

        no_grad(|| {
            fused_gate_up_silu_infer_into(&input_f32, &gate_f32, &up_f32, &mut ref_out);
            fused_gate_up_silu_infer_into(&input_f16, &gate_f16, &up_f16, &mut out);
        });

        assert_close(&ref_out, &out, 1e-3);
    }

    #[test]
    fn fused_gateup_batch_accepts_non_contiguous_input() {
        let batch = 2usize;
        let hidden = 8usize;
        let inter = 12usize;
        let x = sample_f32(batch * hidden)
            .into_iter()
            .enumerate()
            .map(|(i, v)| v + (i / hidden) as f32 * 0.15)
            .collect::<Vec<_>>();
        let gate = sample_f32(inter * hidden);
        let up = sample_f32(inter * hidden)
            .into_iter()
            .map(|v| v * 0.5 - 0.2)
            .collect::<Vec<_>>();

        let input_contig = make_tensor(&[batch, 1, hidden], x.clone(), DType::F32);
        let input_non_contig = Tensor::from_array_no_grad(
            Array3::from_shape_vec((1, batch, hidden), x)
                .expect("input shape")
                .permuted_axes([1, 0, 2])
                .into_dyn(),
        );
        let gate_f32 = make_tensor(&[inter, hidden], gate, DType::F32);
        let up_f32 = make_tensor(&[inter, hidden], up, DType::F32);

        let ref_out = no_grad(|| fused_gate_up_silu_infer(&input_contig, &gate_f32, &up_f32));
        let out = no_grad(|| fused_gate_up_silu_infer(&input_non_contig, &gate_f32, &up_f32));

        let ref_vals = ref_out.data_ref().iter().copied().collect::<Vec<_>>();
        let out_vals = out.data_ref().iter().copied().collect::<Vec<_>>();
        assert_close(&ref_vals, &out_vals, 1e-5);
    }

    #[test]
    fn fused_qkv_i8_matches_quantized_reference() {
        let hidden = 8usize;
        let x = sample_f32(hidden);
        let q = sample_f32(hidden * hidden);
        let k = sample_f32(hidden * hidden)
            .into_iter()
            .map(|v| v * 0.7 - 0.1)
            .collect::<Vec<_>>();
        let v = sample_f32(hidden * hidden)
            .into_iter()
            .map(|v| v * -0.4 + 0.05)
            .collect::<Vec<_>>();

        let q_q = quantize_i8(&[hidden, hidden], &q);
        let k_q = quantize_i8(&[hidden, hidden], &k);
        let v_q = quantize_i8(&[hidden, hidden], &v);

        let input = make_tensor(&[1, 1, hidden], x.clone(), DType::F32);
        let q_ref = make_tensor(&[hidden, hidden], q_q, DType::F32);
        let k_ref = make_tensor(&[hidden, hidden], k_q, DType::F32);
        let v_ref = make_tensor(&[hidden, hidden], v_q, DType::F32);
        let q_i8 = make_tensor(&[hidden, hidden], q, DType::I8);
        let k_i8 = make_tensor(&[hidden, hidden], k, DType::I8);
        let v_i8 = make_tensor(&[hidden, hidden], v, DType::I8);

        let mut q_expected = vec![0.0f32; hidden];
        let mut k_expected = vec![0.0f32; hidden];
        let mut v_expected = vec![0.0f32; hidden];
        let mut q_out = vec![0.0f32; hidden];
        let mut k_out = vec![0.0f32; hidden];
        let mut v_out = vec![0.0f32; hidden];

        no_grad(|| {
            fused_qkv_decode_infer_into(
                &input,
                &q_ref,
                &k_ref,
                &v_ref,
                &mut q_expected,
                &mut k_expected,
                &mut v_expected,
            );
            fused_qkv_decode_infer_into(
                &input, &q_i8, &k_i8, &v_i8, &mut q_out, &mut k_out, &mut v_out,
            );
        });

        assert_close(&q_expected, &q_out, 1e-5);
        assert_close(&k_expected, &k_out, 1e-5);
        assert_close(&v_expected, &v_out, 1e-5);
    }

    #[test]
    fn fused_gateup_i8_matches_quantized_reference() {
        let hidden = 8usize;
        let inter = 12usize;
        let x = sample_f32(hidden);
        let gate = sample_f32(inter * hidden);
        let up = sample_f32(inter * hidden)
            .into_iter()
            .map(|v| v * 0.5 - 0.2)
            .collect::<Vec<_>>();

        let gate_q = quantize_i8(&[inter, hidden], &gate);
        let up_q = quantize_i8(&[inter, hidden], &up);

        let input = make_tensor(&[1, 1, hidden], x.clone(), DType::F32);
        let gate_ref = make_tensor(&[inter, hidden], gate_q, DType::F32);
        let up_ref = make_tensor(&[inter, hidden], up_q, DType::F32);
        let gate_i8 = make_tensor(&[inter, hidden], gate, DType::I8);
        let up_i8 = make_tensor(&[inter, hidden], up, DType::I8);

        let mut ref_out = vec![0.0f32; inter];
        let mut out = vec![0.0f32; inter];

        no_grad(|| {
            fused_gate_up_silu_infer_into(&input, &gate_ref, &up_ref, &mut ref_out);
            fused_gate_up_silu_infer_into(&input, &gate_i8, &up_i8, &mut out);
        });

        assert_close(&ref_out, &out, 1e-5);
    }

    #[test]
    fn fused_qkv_bf16_input_i8_matches_quantized_reference() {
        let hidden = 8usize;
        let x = sample_f32(hidden);
        let q = sample_f32(hidden * hidden);
        let k = sample_f32(hidden * hidden)
            .into_iter()
            .map(|v| v * 0.7 - 0.1)
            .collect::<Vec<_>>();
        let v = sample_f32(hidden * hidden)
            .into_iter()
            .map(|v| v * -0.4 + 0.05)
            .collect::<Vec<_>>();

        let q_q = quantize_i8(&[hidden, hidden], &q);
        let k_q = quantize_i8(&[hidden, hidden], &k);
        let v_q = quantize_i8(&[hidden, hidden], &v);

        let input = make_tensor(&[1, 1, hidden], x.clone(), DType::BF16);
        let q_ref = make_tensor(&[hidden, hidden], q_q, DType::F32);
        let k_ref = make_tensor(&[hidden, hidden], k_q, DType::F32);
        let v_ref = make_tensor(&[hidden, hidden], v_q, DType::F32);
        let q_i8 = make_tensor(&[hidden, hidden], q, DType::I8);
        let k_i8 = make_tensor(&[hidden, hidden], k, DType::I8);
        let v_i8 = make_tensor(&[hidden, hidden], v, DType::I8);

        let mut q_expected = vec![0.0f32; hidden];
        let mut k_expected = vec![0.0f32; hidden];
        let mut v_expected = vec![0.0f32; hidden];
        let mut q_out = vec![0.0f32; hidden];
        let mut k_out = vec![0.0f32; hidden];
        let mut v_out = vec![0.0f32; hidden];

        no_grad(|| {
            fused_qkv_decode_infer_into(
                &input,
                &q_ref,
                &k_ref,
                &v_ref,
                &mut q_expected,
                &mut k_expected,
                &mut v_expected,
            );
            fused_qkv_decode_infer_into(
                &input, &q_i8, &k_i8, &v_i8, &mut q_out, &mut k_out, &mut v_out,
            );
        });

        assert_close(&q_expected, &q_out, 1e-5);
        assert_close(&k_expected, &k_out, 1e-5);
        assert_close(&v_expected, &v_out, 1e-5);
    }

    #[test]
    fn fused_qkv_f16_input_i8_matches_quantized_reference() {
        let hidden = 8usize;
        let x = sample_f32(hidden);
        let q = sample_f32(hidden * hidden);
        let k = sample_f32(hidden * hidden)
            .into_iter()
            .map(|v| v * 0.7 - 0.1)
            .collect::<Vec<_>>();
        let v = sample_f32(hidden * hidden)
            .into_iter()
            .map(|v| v * -0.4 + 0.05)
            .collect::<Vec<_>>();

        let q_q = quantize_i8(&[hidden, hidden], &q);
        let k_q = quantize_i8(&[hidden, hidden], &k);
        let v_q = quantize_i8(&[hidden, hidden], &v);

        let input = make_tensor(&[1, 1, hidden], x.clone(), DType::F16);
        let q_ref = make_tensor(&[hidden, hidden], q_q, DType::F32);
        let k_ref = make_tensor(&[hidden, hidden], k_q, DType::F32);
        let v_ref = make_tensor(&[hidden, hidden], v_q, DType::F32);
        let q_i8 = make_tensor(&[hidden, hidden], q, DType::I8);
        let k_i8 = make_tensor(&[hidden, hidden], k, DType::I8);
        let v_i8 = make_tensor(&[hidden, hidden], v, DType::I8);

        let mut q_expected = vec![0.0f32; hidden];
        let mut k_expected = vec![0.0f32; hidden];
        let mut v_expected = vec![0.0f32; hidden];
        let mut q_out = vec![0.0f32; hidden];
        let mut k_out = vec![0.0f32; hidden];
        let mut v_out = vec![0.0f32; hidden];

        no_grad(|| {
            fused_qkv_decode_infer_into(
                &input,
                &q_ref,
                &k_ref,
                &v_ref,
                &mut q_expected,
                &mut k_expected,
                &mut v_expected,
            );
            fused_qkv_decode_infer_into(
                &input, &q_i8, &k_i8, &v_i8, &mut q_out, &mut k_out, &mut v_out,
            );
        });

        assert_close(&q_expected, &q_out, 1e-5);
        assert_close(&k_expected, &k_out, 1e-5);
        assert_close(&v_expected, &v_out, 1e-5);
    }

    #[test]
    fn fused_gateup_bf16_input_i8_matches_quantized_reference() {
        let hidden = 8usize;
        let inter = 12usize;
        let x = sample_f32(hidden);
        let gate = sample_f32(inter * hidden);
        let up = sample_f32(inter * hidden)
            .into_iter()
            .map(|v| v * 0.5 - 0.2)
            .collect::<Vec<_>>();

        let gate_q = quantize_i8(&[inter, hidden], &gate);
        let up_q = quantize_i8(&[inter, hidden], &up);

        let input = make_tensor(&[1, 1, hidden], x.clone(), DType::BF16);
        let gate_ref = make_tensor(&[inter, hidden], gate_q, DType::F32);
        let up_ref = make_tensor(&[inter, hidden], up_q, DType::F32);
        let gate_i8 = make_tensor(&[inter, hidden], gate, DType::I8);
        let up_i8 = make_tensor(&[inter, hidden], up, DType::I8);

        let mut ref_out = vec![0.0f32; inter];
        let mut out = vec![0.0f32; inter];

        no_grad(|| {
            fused_gate_up_silu_infer_into(&input, &gate_ref, &up_ref, &mut ref_out);
            fused_gate_up_silu_infer_into(&input, &gate_i8, &up_i8, &mut out);
        });

        assert_close(&ref_out, &out, 1e-5);
    }

    #[test]
    fn fused_gateup_f16_input_i8_matches_quantized_reference() {
        let hidden = 8usize;
        let inter = 12usize;
        let x = sample_f32(hidden);
        let gate = sample_f32(inter * hidden);
        let up = sample_f32(inter * hidden)
            .into_iter()
            .map(|v| v * 0.5 - 0.2)
            .collect::<Vec<_>>();

        let gate_q = quantize_i8(&[inter, hidden], &gate);
        let up_q = quantize_i8(&[inter, hidden], &up);

        let input = make_tensor(&[1, 1, hidden], x.clone(), DType::F16);
        let gate_ref = make_tensor(&[inter, hidden], gate_q, DType::F32);
        let up_ref = make_tensor(&[inter, hidden], up_q, DType::F32);
        let gate_i8 = make_tensor(&[inter, hidden], gate, DType::I8);
        let up_i8 = make_tensor(&[inter, hidden], up, DType::I8);

        let mut ref_out = vec![0.0f32; inter];
        let mut out = vec![0.0f32; inter];

        no_grad(|| {
            fused_gate_up_silu_infer_into(&input, &gate_ref, &up_ref, &mut ref_out);
            fused_gate_up_silu_infer_into(&input, &gate_i8, &up_i8, &mut out);
        });

        assert_close(&ref_out, &out, 1e-5);
    }

    #[test]
    fn fused_qkv_i8_input_i8_matches_quantized_reference() {
        let hidden = 8usize;
        let x = sample_f32(hidden)
            .into_iter()
            .map(|v| v * 0.8 - 0.1)
            .collect::<Vec<_>>();
        let q = sample_f32(hidden * hidden);
        let k = sample_f32(hidden * hidden)
            .into_iter()
            .map(|v| v * 0.7 - 0.1)
            .collect::<Vec<_>>();
        let v = sample_f32(hidden * hidden)
            .into_iter()
            .map(|v| v * -0.4 + 0.05)
            .collect::<Vec<_>>();

        let x_q = quantize_i8(&[1, 1, hidden], &x);
        let q_q = quantize_i8(&[hidden, hidden], &q);
        let k_q = quantize_i8(&[hidden, hidden], &k);
        let v_q = quantize_i8(&[hidden, hidden], &v);

        let input_ref = make_tensor(&[1, 1, hidden], x_q, DType::F32);
        let input = make_tensor(&[1, 1, hidden], x, DType::I8);
        let q_ref = make_tensor(&[hidden, hidden], q_q, DType::F32);
        let k_ref = make_tensor(&[hidden, hidden], k_q, DType::F32);
        let v_ref = make_tensor(&[hidden, hidden], v_q, DType::F32);
        let q_i8 = make_tensor(&[hidden, hidden], q, DType::I8);
        let k_i8 = make_tensor(&[hidden, hidden], k, DType::I8);
        let v_i8 = make_tensor(&[hidden, hidden], v, DType::I8);

        let mut q_expected = vec![0.0f32; hidden];
        let mut k_expected = vec![0.0f32; hidden];
        let mut v_expected = vec![0.0f32; hidden];
        let mut q_out = vec![0.0f32; hidden];
        let mut k_out = vec![0.0f32; hidden];
        let mut v_out = vec![0.0f32; hidden];

        no_grad(|| {
            fused_qkv_decode_infer_into(
                &input_ref,
                &q_ref,
                &k_ref,
                &v_ref,
                &mut q_expected,
                &mut k_expected,
                &mut v_expected,
            );
            fused_qkv_decode_infer_into(
                &input, &q_i8, &k_i8, &v_i8, &mut q_out, &mut k_out, &mut v_out,
            );
        });

        assert_close(&q_expected, &q_out, 1e-5);
        assert_close(&k_expected, &k_out, 1e-5);
        assert_close(&v_expected, &v_out, 1e-5);
    }

    #[test]
    fn fused_gateup_i8_input_i8_matches_quantized_reference() {
        let hidden = 8usize;
        let inter = 12usize;
        let x = sample_f32(hidden)
            .into_iter()
            .map(|v| v * 0.75 - 0.05)
            .collect::<Vec<_>>();
        let gate = sample_f32(inter * hidden);
        let up = sample_f32(inter * hidden)
            .into_iter()
            .map(|v| v * 0.5 - 0.2)
            .collect::<Vec<_>>();

        let x_q = quantize_i8(&[1, 1, hidden], &x);
        let gate_q = quantize_i8(&[inter, hidden], &gate);
        let up_q = quantize_i8(&[inter, hidden], &up);

        let input_ref = make_tensor(&[1, 1, hidden], x_q, DType::F32);
        let input = make_tensor(&[1, 1, hidden], x, DType::I8);
        let gate_ref = make_tensor(&[inter, hidden], gate_q, DType::F32);
        let up_ref = make_tensor(&[inter, hidden], up_q, DType::F32);
        let gate_i8 = make_tensor(&[inter, hidden], gate, DType::I8);
        let up_i8 = make_tensor(&[inter, hidden], up, DType::I8);

        let mut ref_out = vec![0.0f32; inter];
        let mut out = vec![0.0f32; inter];

        no_grad(|| {
            fused_gate_up_silu_infer_into(&input_ref, &gate_ref, &up_ref, &mut ref_out);
            fused_gate_up_silu_infer_into(&input, &gate_i8, &up_i8, &mut out);
        });

        assert_close(&ref_out, &out, 1e-5);
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_fused_gateup_matches_cpu_reference_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let hidden = 8usize;
        let inter = 12usize;
        let input = make_tensor(
            &[2, 1, hidden],
            sample_f32(2 * hidden)
                .into_iter()
                .enumerate()
                .map(|(i, v)| v + (i / hidden) as f32 * 0.1)
                .collect(),
            DType::BF16,
        )
        .to_cuda();
        let gate = make_tensor(&[inter, hidden], sample_f32(inter * hidden), DType::BF16).to_cuda();
        let up = make_tensor(
            &[inter, hidden],
            sample_f32(inter * hidden)
                .into_iter()
                .map(|v| v * 0.5 - 0.2)
                .collect(),
            DType::BF16,
        )
        .to_cuda();

        crate::autograd::set_strict_device_execution(true);
        let cuda_out = no_grad(|| fused_gate_up_silu_infer(&input, &gate, &up));
        crate::autograd::set_strict_device_execution(false);

        let cpu_out =
            no_grad(|| fused_gate_up_silu_infer(&input.to_cpu(), &gate.to_cpu(), &up.to_cpu()));
        assert!(cuda_out.is_cuda());
        assert_eq!(cuda_out.dtype(), DType::BF16);
        assert_eq!(cpu_out.dtype(), DType::BF16);
        assert_eq!(cuda_out.shape_vec(), cpu_out.shape_vec());
        let got = cuda_out.data_ref().iter().copied().collect::<Vec<_>>();
        let expect = cpu_out.data_ref().iter().copied().collect::<Vec<_>>();
        assert_close(&got, &expect, 2e-2);
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_fused_gateup_accepts_i8_inputs_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let hidden = 8usize;
        let inter = 12usize;
        let input = make_tensor(
            &[1, 1, hidden],
            sample_f32(hidden)
                .into_iter()
                .map(|v| v * 0.75 - 0.05)
                .collect(),
            DType::I8,
        )
        .to_cuda();
        let gate = make_tensor(&[inter, hidden], sample_f32(inter * hidden), DType::I8).to_cuda();
        let up = make_tensor(
            &[inter, hidden],
            sample_f32(inter * hidden)
                .into_iter()
                .map(|v| v * 0.5 - 0.2)
                .collect(),
            DType::I8,
        )
        .to_cuda();

        crate::autograd::set_strict_device_execution(true);
        let out = no_grad(|| fused_gate_up_silu_infer(&input, &gate, &up));
        crate::autograd::set_strict_device_execution(false);

        assert!(out.is_cuda());
        assert_eq!(out.dtype(), DType::F32);
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_fused_gateup_mixed_float_dtypes_promote_to_f32_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let hidden = 8usize;
        let inter = 12usize;
        let input = make_tensor(&[1, 1, hidden], sample_f32(hidden), DType::BF16).to_cuda();
        let gate = make_tensor(&[inter, hidden], sample_f32(inter * hidden), DType::F32).to_cuda();
        let up = make_tensor(
            &[inter, hidden],
            sample_f32(inter * hidden)
                .into_iter()
                .map(|v| v * 0.5 - 0.2)
                .collect(),
            DType::BF16,
        )
        .to_cuda();

        crate::autograd::set_strict_device_execution(true);
        let out = no_grad(|| fused_gate_up_silu_infer(&input, &gate, &up));
        crate::autograd::set_strict_device_execution(false);

        assert!(out.is_cuda());
        assert_eq!(out.dtype(), DType::F32);
        assert!(out.cloned_cuda_f32_buffer().is_some());
    }

    #[test]
    fn fused_gateup_i8_or_mixed_dtype_outputs_f32_on_cpu() {
        let hidden = 8usize;
        let inter = 12usize;
        let input = make_tensor(&[1, 1, hidden], sample_f32(hidden), DType::F16);
        let gate = make_tensor(&[inter, hidden], sample_f32(inter * hidden), DType::I8);
        let up = make_tensor(
            &[inter, hidden],
            sample_f32(inter * hidden)
                .into_iter()
                .map(|v| v * 0.5 - 0.2)
                .collect(),
            DType::I8,
        );

        let out = no_grad(|| fused_gate_up_silu_infer(&input, &gate, &up));

        assert_eq!(out.dtype(), DType::F32);
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_fused_qkv_matches_cpu_reference_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let batch = 2usize;
        let hidden = 8usize;
        let x = sample_f32(batch * hidden)
            .into_iter()
            .enumerate()
            .map(|(i, v)| v + (i / hidden) as f32 * 0.15)
            .collect::<Vec<_>>();
        let q = sample_f32(hidden * hidden);
        let k = sample_f32(hidden * hidden)
            .into_iter()
            .map(|v| v * 0.7 - 0.1)
            .collect::<Vec<_>>();
        let v = sample_f32(hidden * hidden)
            .into_iter()
            .map(|v| v * -0.4 + 0.05)
            .collect::<Vec<_>>();

        let input = make_tensor(&[batch, 1, hidden], x.clone(), DType::BF16).to_cuda();
        let q_w = make_tensor(&[hidden, hidden], q.clone(), DType::BF16).to_cuda();
        let k_w = make_tensor(&[hidden, hidden], k.clone(), DType::BF16).to_cuda();
        let v_w = make_tensor(&[hidden, hidden], v.clone(), DType::BF16).to_cuda();

        crate::autograd::set_strict_device_execution(true);
        let (q_cuda, k_cuda, v_cuda) =
            no_grad(|| fused_qkv_decode_infer(&input, &q_w, &k_w, &v_w, 1, 1));
        crate::autograd::set_strict_device_execution(false);

        let (q_cpu, k_cpu, v_cpu) = no_grad(|| {
            fused_qkv_decode_infer(
                &input.to_cpu(),
                &q_w.to_cpu(),
                &k_w.to_cpu(),
                &v_w.to_cpu(),
                1,
                1,
            )
        });
        assert_close(q_cuda.as_slice().unwrap(), q_cpu.as_slice().unwrap(), 2e-2);
        assert_close(k_cuda.as_slice().unwrap(), k_cpu.as_slice().unwrap(), 2e-2);
        assert_close(v_cuda.as_slice().unwrap(), v_cpu.as_slice().unwrap(), 2e-2);
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_fused_qkv_accepts_i8_weights_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let hidden = 8usize;
        let x = sample_f32(hidden);
        let q = sample_f32(hidden * hidden);
        let k = sample_f32(hidden * hidden)
            .into_iter()
            .map(|v| v * 0.7 - 0.1)
            .collect::<Vec<_>>();
        let v = sample_f32(hidden * hidden)
            .into_iter()
            .map(|v| v * -0.4 + 0.05)
            .collect::<Vec<_>>();

        let input = make_tensor(&[1, 1, hidden], x, DType::I8).to_cuda();
        let q_w = make_tensor(&[hidden, hidden], q, DType::I8).to_cuda();
        let k_w = make_tensor(&[hidden, hidden], k, DType::I8).to_cuda();
        let v_w = make_tensor(&[hidden, hidden], v, DType::I8).to_cuda();

        crate::autograd::set_strict_device_execution(true);
        let (q_out, k_out, v_out) =
            no_grad(|| fused_qkv_decode_infer(&input, &q_w, &k_w, &v_w, 1, 1));
        crate::autograd::set_strict_device_execution(false);

        assert_eq!(q_out.shape(), &[1, 1, hidden]);
        assert_eq!(k_out.shape(), &[1, 1, hidden]);
        assert_eq!(v_out.shape(), &[1, 1, hidden]);
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_fused_qkv_decode_tensors_preserve_bf16_dtype_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let hidden = 8usize;
        let input = make_tensor(&[2, 1, hidden], sample_f32(2 * hidden), DType::BF16).to_cuda();
        let q_w =
            make_tensor(&[hidden, hidden], sample_f32(hidden * hidden), DType::BF16).to_cuda();
        let k_w = make_tensor(
            &[hidden, hidden],
            sample_f32(hidden * hidden)
                .into_iter()
                .map(|v| v * 0.7 - 0.1)
                .collect(),
            DType::BF16,
        )
        .to_cuda();
        let v_w = make_tensor(
            &[hidden, hidden],
            sample_f32(hidden * hidden)
                .into_iter()
                .map(|v| v * -0.4 + 0.05)
                .collect(),
            DType::BF16,
        )
        .to_cuda();

        crate::autograd::set_strict_device_execution(true);
        let (q_out, k_out, v_out) =
            no_grad(|| fused_qkv_decode_infer_tensors(&input, &q_w, &k_w, &v_w, 1, 1));
        crate::autograd::set_strict_device_execution(false);

        for out in [&q_out, &k_out, &v_out] {
            assert!(out.is_cuda());
            assert_eq!(out.dtype(), DType::BF16);
            assert!(out.cloned_cuda_f32_buffer().is_some());
        }
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_fused_qkv_decode_tensors_mixed_dtype_promotes_to_f32_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let hidden = 8usize;
        let input = make_tensor(&[1, 1, hidden], sample_f32(hidden), DType::F16).to_cuda();
        let q_w = make_tensor(&[hidden, hidden], sample_f32(hidden * hidden), DType::F16).to_cuda();
        let k_w =
            make_tensor(&[hidden, hidden], sample_f32(hidden * hidden), DType::BF16).to_cuda();
        let v_w = make_tensor(&[hidden, hidden], sample_f32(hidden * hidden), DType::F16).to_cuda();

        crate::autograd::set_strict_device_execution(true);
        let (q_out, k_out, v_out) =
            no_grad(|| fused_qkv_decode_infer_tensors(&input, &q_w, &k_w, &v_w, 1, 1));
        crate::autograd::set_strict_device_execution(false);

        for out in [&q_out, &k_out, &v_out] {
            assert!(out.is_cuda());
            assert_eq!(out.dtype(), DType::F32);
            assert!(out.cloned_cuda_f32_buffer().is_some());
        }
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_fused_qkv_prefill_tensors_preserve_f16_dtype_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let batch = 2usize;
        let seq = 3usize;
        let hidden = 8usize;
        let input = make_tensor(
            &[batch, seq, hidden],
            sample_f32(batch * seq * hidden),
            DType::F16,
        )
        .to_cuda();
        let q_w = make_tensor(&[hidden, hidden], sample_f32(hidden * hidden), DType::F16).to_cuda();
        let k_w = make_tensor(
            &[hidden, hidden],
            sample_f32(hidden * hidden)
                .into_iter()
                .map(|v| v * 0.7 - 0.1)
                .collect(),
            DType::F16,
        )
        .to_cuda();
        let v_w = make_tensor(
            &[hidden, hidden],
            sample_f32(hidden * hidden)
                .into_iter()
                .map(|v| v * -0.4 + 0.05)
                .collect(),
            DType::F16,
        )
        .to_cuda();

        crate::autograd::set_strict_device_execution(true);
        let (q_out, k_out, v_out) = no_grad(|| {
            fused_qkv_prefill_infer_tensors(&input, &q_w, &k_w, &v_w, 1, 1)
                .expect("CUDA prefill fused QKV should run")
        });
        crate::autograd::set_strict_device_execution(false);

        for out in [&q_out, &k_out, &v_out] {
            assert!(out.is_cuda());
            assert_eq!(out.dtype(), DType::F16);
            assert!(out.cloned_cuda_f32_buffer().is_some());
        }
        assert_eq!(q_out.shape_vec(), vec![batch, 1, seq, hidden]);
        assert_eq!(k_out.shape_vec(), vec![batch, 1, seq, hidden]);
        assert_eq!(v_out.shape_vec(), vec![batch, 1, seq, hidden]);
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_fused_qkv_prefill_tensors_gqa_matches_cpu_reference_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let batch = 1usize;
        let seq = 3usize;
        let hidden = 8usize;
        let kv_hidden = 4usize;
        let input_data = (0..batch * seq * hidden)
            .map(|i| (i as f32 * 0.0625) - 0.4)
            .collect::<Vec<_>>();
        let q_data = (0..hidden * hidden)
            .map(|i| (i as f32 * 0.03125) - 0.25)
            .collect::<Vec<_>>();
        let k_data = (0..kv_hidden * hidden)
            .map(|i| (i as f32 * 0.0275) - 0.25)
            .collect::<Vec<_>>();
        let v_data = (0..kv_hidden * hidden)
            .map(|i| (i as f32 * -0.021) - 0.25)
            .collect::<Vec<_>>();

        let input = make_tensor(&[batch, seq, hidden], input_data, DType::BF16);
        let q_w = make_tensor(&[hidden, hidden], q_data, DType::BF16);
        let k_w = make_tensor(&[kv_hidden, hidden], k_data, DType::BF16);
        let v_w = make_tensor(&[kv_hidden, hidden], v_data, DType::BF16);

        crate::autograd::set_strict_device_execution(true);
        let (q_cuda, k_cuda, v_cuda) = no_grad(|| {
            fused_qkv_prefill_infer_tensors(
                &input.to_cuda(),
                &q_w.to_cuda(),
                &k_w.to_cuda(),
                &v_w.to_cuda(),
                2,
                1,
            )
            .expect("CUDA prefill fused QKV should run")
        });
        crate::autograd::set_strict_device_execution(false);

        let expected_projection = |weight: &Tensor, out_heads: usize| {
            let x_vals = input.data_ref().iter().copied().collect::<Vec<_>>();
            let w_vals = weight.data_ref().iter().copied().collect::<Vec<_>>();
            let out_dim = weight.shape_vec()[0];
            let head_dim = out_dim / out_heads;
            let mut out = vec![0.0f32; batch * out_heads * seq * head_dim];
            for bb in 0..batch {
                for ss in 0..seq {
                    for oo in 0..out_dim {
                        let mut sum = 0.0f32;
                        for kk in 0..hidden {
                            sum += x_vals[(bb * seq + ss) * hidden + kk] * w_vals[oo * hidden + kk];
                        }
                        let head = oo / head_dim;
                        let dim = oo % head_dim;
                        out[((bb * out_heads + head) * seq + ss) * head_dim + dim] = sum;
                    }
                }
            }
            out
        };
        let q_cpu = expected_projection(&q_w, 2);
        let k_cpu = expected_projection(&k_w, 1);
        let v_cpu = expected_projection(&v_w, 1);

        assert_eq!(q_cuda.shape_vec(), vec![batch, 2, seq, 4]);
        assert_eq!(k_cuda.shape_vec(), vec![batch, 1, seq, 4]);
        assert_eq!(v_cuda.shape_vec(), vec![batch, 1, seq, 4]);
        assert_close(
            &q_cuda.data_ref().iter().copied().collect::<Vec<_>>(),
            &q_cpu,
            2e-2,
        );
        assert_close(
            &k_cuda.data_ref().iter().copied().collect::<Vec<_>>(),
            &k_cpu,
            2e-2,
        );
        assert_close(
            &v_cuda.data_ref().iter().copied().collect::<Vec<_>>(),
            &v_cpu,
            2e-2,
        );
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_fused_gateup_into_matches_cpu_reference_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let hidden = 8usize;
        let inter = 12usize;
        let input = make_tensor(
            &[1, 1, hidden],
            sample_f32(hidden)
                .into_iter()
                .map(|v| v * 0.9 - 0.1)
                .collect(),
            DType::BF16,
        )
        .to_cuda();
        let gate = make_tensor(&[inter, hidden], sample_f32(inter * hidden), DType::BF16).to_cuda();
        let up = make_tensor(
            &[inter, hidden],
            sample_f32(inter * hidden)
                .into_iter()
                .map(|v| v * 0.5 - 0.2)
                .collect(),
            DType::BF16,
        )
        .to_cuda();

        let mut cuda_out = vec![0.0f32; inter];
        crate::autograd::set_strict_device_execution(true);
        no_grad(|| fused_gate_up_silu_infer_into(&input, &gate, &up, &mut cuda_out));
        crate::autograd::set_strict_device_execution(false);

        let mut cpu_out = vec![0.0f32; inter];
        no_grad(|| {
            fused_gate_up_silu_infer_into(
                &input.to_cpu(),
                &gate.to_cpu(),
                &up.to_cpu(),
                &mut cpu_out,
            )
        });
        assert_close(&cuda_out, &cpu_out, 2e-2);
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_fused_qkv_into_matches_cpu_reference_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let hidden = 8usize;
        let x = sample_f32(hidden)
            .into_iter()
            .map(|v| v * 0.85 - 0.05)
            .collect::<Vec<_>>();
        let q = sample_f32(hidden * hidden);
        let k = sample_f32(hidden * hidden)
            .into_iter()
            .map(|v| v * 0.7 - 0.1)
            .collect::<Vec<_>>();
        let v = sample_f32(hidden * hidden)
            .into_iter()
            .map(|v| v * -0.4 + 0.05)
            .collect::<Vec<_>>();

        let input = make_tensor(&[1, 1, hidden], x, DType::BF16).to_cuda();
        let q_w = make_tensor(&[hidden, hidden], q, DType::BF16).to_cuda();
        let k_w = make_tensor(&[hidden, hidden], k, DType::BF16).to_cuda();
        let v_w = make_tensor(&[hidden, hidden], v, DType::BF16).to_cuda();

        let mut q_cuda = vec![0.0f32; hidden];
        let mut k_cuda = vec![0.0f32; hidden];
        let mut v_cuda = vec![0.0f32; hidden];
        crate::autograd::set_strict_device_execution(true);
        no_grad(|| {
            fused_qkv_decode_infer_into(
                &input,
                &q_w,
                &k_w,
                &v_w,
                &mut q_cuda,
                &mut k_cuda,
                &mut v_cuda,
            )
        });
        crate::autograd::set_strict_device_execution(false);

        let mut q_cpu = vec![0.0f32; hidden];
        let mut k_cpu = vec![0.0f32; hidden];
        let mut v_cpu = vec![0.0f32; hidden];
        no_grad(|| {
            fused_qkv_decode_infer_into(
                &input.to_cpu(),
                &q_w.to_cpu(),
                &k_w.to_cpu(),
                &v_w.to_cpu(),
                &mut q_cpu,
                &mut k_cpu,
                &mut v_cpu,
            )
        });
        assert_close(&q_cuda, &q_cpu, 2e-2);
        assert_close(&k_cuda, &k_cpu, 2e-2);
        assert_close(&v_cuda, &v_cpu, 2e-2);
    }

    #[test]
    fn fused_softmax_no_grad_preserves_bf16_dtype() {
        let input_f32 = make_tensor(
            &[1, 1, 2, 4],
            vec![1.0, 2.0, 3.0, 4.0, -1.0, 0.5, 2.5, -3.0],
            DType::F32,
        );
        let input_bf16 = make_tensor(
            &[1, 1, 2, 4],
            vec![1.0, 2.0, 3.0, 4.0, -1.0, 0.5, 2.5, -3.0],
            DType::BF16,
        );

        let ref_out = no_grad(|| fused_softmax(&input_f32, 1.0, false));
        let out = no_grad(|| fused_softmax(&input_bf16, 1.0, false));

        assert_eq!(input_bf16.dtype(), DType::BF16);
        assert_eq!(out.dtype(), DType::BF16);

        let ref_vals = ref_out
            .data_ref()
            .iter()
            .map(|&v| half::bf16::from_f32(v).to_f32())
            .collect::<Vec<_>>();
        out.with_storage_view(|view| match view {
            TensorStorageView::BF16(view) => {
                let vals = view.iter().map(|v| v.to_f32()).collect::<Vec<_>>();
                assert_eq!(vals, ref_vals);
            }
            TensorStorageView::F16(_) => {
                panic!("bf16 fused_softmax output should stay bf16 in no-grad")
            }
            TensorStorageView::F32(_) => {
                panic!("bf16 fused_softmax output should stay bf16 in no-grad")
            }
        });
    }

    #[test]
    fn fused_softmax_bf16_matches_quantized_reference() {
        let input_f32 = make_tensor(
            &[1, 1, 2, 4],
            vec![1.0, 2.0, 3.0, 4.0, -1.0, 0.5, 2.5, -3.0],
            DType::F32,
        );
        let input_bf16 = make_tensor(
            &[1, 1, 2, 4],
            vec![1.0, 2.0, 3.0, 4.0, -1.0, 0.5, 2.5, -3.0],
            DType::BF16,
        );

        let ref_out = no_grad(|| fused_softmax(&input_f32, 0.75, true));
        let out = no_grad(|| fused_softmax(&input_bf16, 0.75, true));

        let ref_vals = ref_out
            .data_ref()
            .iter()
            .map(|&v| half::bf16::from_f32(v).to_f32())
            .collect::<Vec<_>>();
        out.with_storage_view(|view| match view {
            TensorStorageView::BF16(view) => {
                let vals = view.iter().map(|v| v.to_f32()).collect::<Vec<_>>();
                assert_eq!(vals, ref_vals);
            }
            TensorStorageView::F16(_) => {
                panic!("bf16 fused_softmax output should stay bf16 in no-grad")
            }
            TensorStorageView::F32(_) => {
                panic!("bf16 fused_softmax output should stay bf16 in no-grad")
            }
        });
    }

    #[test]
    fn fused_softmax_with_past_no_grad_preserves_bf16_dtype() {
        let input_f32 = make_tensor(
            &[1, 1, 2, 5],
            vec![1.0, -0.5, 2.0, 0.25, -1.25, 0.5, 1.5, -1.0, 0.75, 2.0],
            DType::F32,
        );
        let input_bf16 = make_tensor(
            &[1, 1, 2, 5],
            vec![1.0, -0.5, 2.0, 0.25, -1.25, 0.5, 1.5, -1.0, 0.75, 2.0],
            DType::BF16,
        );

        let ref_out = no_grad(|| fused_softmax_with_past_infer(&input_f32, 0.75, true, 2));
        let out = no_grad(|| fused_softmax_with_past_infer(&input_bf16, 0.75, true, 2));

        assert_eq!(out.dtype(), DType::BF16);
        let ref_vals = ref_out
            .data_ref()
            .iter()
            .map(|&v| half::bf16::from_f32(v).to_f32())
            .collect::<Vec<_>>();
        out.with_storage_view(|view| match view {
            TensorStorageView::BF16(view) => {
                let vals = view.iter().map(|v| v.to_f32()).collect::<Vec<_>>();
                assert_eq!(vals, ref_vals);
            }
            TensorStorageView::F16(_) => {
                panic!("bf16 fused_softmax_with_past output should stay bf16 in no-grad")
            }
            TensorStorageView::F32(_) => {
                panic!("bf16 fused_softmax_with_past output should stay bf16 in no-grad")
            }
        });
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_fused_softmax_preserves_bf16_dtype_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let input = make_tensor(
            &[1, 2, 3, 4],
            vec![
                1.0, -0.5, 2.0, 0.25, 0.5, 1.5, -1.0, 0.75, -0.25, 0.0, 1.25, -1.5, 2.0, -0.75,
                0.5, 1.0, -1.0, 1.25, 0.25, -0.5, 0.75, 0.5, -0.25, 2.5,
            ],
            DType::BF16,
        )
        .to_cuda();

        crate::ops::cuda::set_enabled(true);
        crate::autograd::set_strict_device_execution(true);
        let cuda_out = no_grad(|| fused_softmax(&input, 0.75, true));
        crate::autograd::set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);

        let cpu_out = no_grad(|| fused_softmax(&input.to_cpu(), 0.75, true));
        assert!(cuda_out.is_cuda());
        assert_eq!(cuda_out.dtype(), DType::BF16);
        for (got, expect) in cuda_out.data_ref().iter().zip(cpu_out.data_ref().iter()) {
            assert!((got - expect).abs() < 2e-2, "got {got}, expect {expect}");
        }
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_fused_softmax_promotes_i8_to_f32_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let input = make_tensor(
            &[1, 1, 3, 4],
            vec![
                1.0, -2.0, 0.5, 3.0, -1.0, 0.25, 1.5, -0.5, 2.0, -1.5, 0.75, 0.0,
            ],
            DType::I8,
        )
        .to_cuda();

        crate::ops::cuda::set_enabled(true);
        crate::autograd::set_strict_device_execution(true);
        let cuda_out = no_grad(|| fused_softmax(&input, 1.0, false));
        crate::autograd::set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);

        let cpu_out = no_grad(|| fused_softmax(&input.to_cpu(), 1.0, false));
        assert!(cuda_out.is_cuda());
        assert_eq!(cuda_out.dtype(), DType::F32);
        for (got, expect) in cuda_out.data_ref().iter().zip(cpu_out.data_ref().iter()) {
            assert!((got - expect).abs() < 1e-5, "got {got}, expect {expect}");
        }
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_fused_softmax_backward_matches_cpu_reference_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let shape = [1, 2, 3, 4];
        let input_cpu = Tensor::from_data_with_grad_flag(
            Array::from_shape_vec(
                IxDyn(&shape),
                vec![
                    1.0, -0.5, 2.0, 0.25, 0.5, 1.5, -1.0, 0.75, -0.25, 0.0, 1.25, -1.5, 2.0, -0.75,
                    0.5, 1.0, -1.0, 1.25, 0.25, -0.5, 0.75, 0.5, -0.25, 2.5,
                ],
            )
            .unwrap()
            .into_dyn(),
            true,
        );
        let coeff_cpu = Tensor::from_data_with_grad_flag(
            Array::from_shape_vec(
                IxDyn(&shape),
                vec![
                    -0.3, 0.2, 0.7, -0.1, 0.4, -0.6, 0.8, 0.1, -0.2, 0.5, -0.7, 0.9, 0.25, -0.35,
                    0.45, -0.55, 0.65, -0.75, 0.85, -0.95, 0.15, 0.05, -0.45, 0.55,
                ],
            )
            .unwrap()
            .into_dyn(),
            false,
        );
        let input_cuda = input_cpu.to_cuda();
        let coeff_cuda = coeff_cpu.to_cuda();

        crate::ops::cuda::set_enabled(true);
        crate::autograd::set_strict_device_execution(true);
        let cuda_out = fused_softmax(&input_cuda, 0.75, true);
        assert!(cuda_out.is_cuda());
        let loss_cuda = crate::ops::arithmetic::sum(&(&cuda_out * &coeff_cuda));
        loss_cuda.backward();
        assert!(input_cuda.cloned_cuda_f32_grad().is_some());
        assert!(!input_cuda.has_host_grad());
        crate::autograd::set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);

        let cpu_out = fused_softmax(&input_cpu, 0.75, true);
        let loss_cpu = crate::ops::arithmetic::sum(&(&cpu_out * &coeff_cpu));
        loss_cpu.backward();

        let cuda_grad = input_cuda.grad().expect("cuda fused_softmax grad");
        let cpu_grad = input_cpu.grad().expect("cpu fused_softmax grad");
        for (got, expect) in cuda_grad.iter().zip(cpu_grad.iter()) {
            assert!((got - expect).abs() < 1e-5, "got {got}, expect {expect}");
        }
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_fused_softmax_with_past_matches_cpu_reference_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let input = make_tensor(
            &[1, 2, 2, 5],
            vec![
                1.0, -0.5, 2.0, 0.25, -1.25, 0.5, 1.5, -1.0, 0.75, 2.0, -0.25, 0.0, 1.25, -1.5,
                0.5, 2.0, -0.75, 0.5, 1.0, -1.0,
            ],
            DType::BF16,
        )
        .to_cuda();

        crate::ops::cuda::set_enabled(true);
        crate::autograd::set_strict_device_execution(true);
        let cuda_out = no_grad(|| fused_softmax_with_past_infer(&input, 0.75, true, 2));
        crate::autograd::set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);

        let cpu_out = no_grad(|| fused_softmax_with_past_infer(&input.to_cpu(), 0.75, true, 2));
        assert!(cuda_out.is_cuda());
        assert_eq!(cuda_out.dtype(), DType::BF16);
        for (got, expect) in cuda_out.data_ref().iter().zip(cpu_out.data_ref().iter()) {
            assert!((got - expect).abs() < 2e-2, "got {got}, expect {expect}");
        }
    }

    #[test]
    #[should_panic(expected = "Fused Softmax key dimension must be greater than zero")]
    fn fused_softmax_rejects_zero_key_dim() {
        let input = make_tensor(&[1, 2, 3, 0], Vec::new(), DType::F32);
        let _ = no_grad(|| fused_softmax(&input, 1.0, false));
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_empty_fused_softmax_no_grad_stays_on_cuda_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let input = make_tensor(&[1, 2, 0, 4], Vec::new(), DType::BF16).to_cuda();

        crate::ops::cuda::set_enabled(true);
        crate::autograd::set_strict_device_execution(true);
        let out = no_grad(|| fused_softmax(&input, 1.0, false));
        crate::autograd::set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);

        assert!(out.is_cuda());
        assert_eq!(out.dtype(), DType::BF16);
        assert_eq!(out.shape_vec(), vec![1, 2, 0, 4]);
        assert_eq!(out.len(), 0);
        assert!(out.cloned_cuda_f32_buffer().is_none());
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_empty_fused_gateup_no_grad_stays_on_cuda_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let hidden = 8usize;
        let inter = 12usize;
        let input = make_tensor(&[0, 1, hidden], Vec::new(), DType::BF16).to_cuda();
        let gate = make_tensor(&[inter, hidden], sample_f32(inter * hidden), DType::BF16).to_cuda();
        let up = make_tensor(
            &[inter, hidden],
            sample_f32(inter * hidden)
                .into_iter()
                .map(|v| v * 0.5 - 0.2)
                .collect(),
            DType::BF16,
        )
        .to_cuda();

        crate::ops::cuda::set_enabled(true);
        crate::autograd::set_strict_device_execution(true);
        let out = no_grad(|| fused_gate_up_silu_infer(&input, &gate, &up));
        crate::autograd::set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);

        assert!(out.is_cuda());
        assert_eq!(out.dtype(), DType::BF16);
        assert_eq!(out.shape_vec(), vec![0, 1, inter]);
        assert_eq!(out.len(), 0);
        assert!(out.cloned_cuda_f32_buffer().is_none());
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_empty_fused_qkv_decode_tensors_stay_on_cuda_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let hidden = 8usize;
        let input = make_tensor(&[0, 1, hidden], Vec::new(), DType::BF16).to_cuda();
        let q_w =
            make_tensor(&[hidden, hidden], sample_f32(hidden * hidden), DType::BF16).to_cuda();
        let k_w = make_tensor(
            &[hidden, hidden],
            sample_f32(hidden * hidden)
                .into_iter()
                .map(|v| v * 0.7 - 0.1)
                .collect(),
            DType::BF16,
        )
        .to_cuda();
        let v_w = make_tensor(
            &[hidden, hidden],
            sample_f32(hidden * hidden)
                .into_iter()
                .map(|v| v * -0.4 + 0.05)
                .collect(),
            DType::BF16,
        )
        .to_cuda();

        crate::ops::cuda::set_enabled(true);
        crate::autograd::set_strict_device_execution(true);
        let (q_out, k_out, v_out) =
            no_grad(|| fused_qkv_decode_infer_tensors(&input, &q_w, &k_w, &v_w, 1, 1));
        crate::autograd::set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);

        for out in [&q_out, &k_out, &v_out] {
            assert!(out.is_cuda());
            assert_eq!(out.dtype(), DType::BF16);
            assert_eq!(out.len(), 0);
            assert!(out.cloned_cuda_f32_buffer().is_none());
        }
        assert_eq!(q_out.shape_vec(), vec![0, 1, 1, hidden]);
        assert_eq!(k_out.shape_vec(), vec![0, 1, 1, hidden]);
        assert_eq!(v_out.shape_vec(), vec![0, 1, 1, hidden]);
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_empty_fused_qkv_prefill_tensors_stay_on_cuda_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let batch = 2usize;
        let seq = 0usize;
        let hidden = 8usize;
        let input = make_tensor(&[batch, seq, hidden], Vec::new(), DType::BF16).to_cuda();
        let q_w =
            make_tensor(&[hidden, hidden], sample_f32(hidden * hidden), DType::BF16).to_cuda();
        let k_w = make_tensor(
            &[hidden, hidden],
            sample_f32(hidden * hidden)
                .into_iter()
                .map(|v| v * 0.7 - 0.1)
                .collect(),
            DType::BF16,
        )
        .to_cuda();
        let v_w = make_tensor(
            &[hidden, hidden],
            sample_f32(hidden * hidden)
                .into_iter()
                .map(|v| v * -0.4 + 0.05)
                .collect(),
            DType::BF16,
        )
        .to_cuda();

        crate::ops::cuda::set_enabled(true);
        crate::autograd::set_strict_device_execution(true);
        let (q_out, k_out, v_out) = no_grad(|| {
            fused_qkv_prefill_infer_tensors(&input, &q_w, &k_w, &v_w, 1, 1)
                .expect("empty CUDA prefill QKV should return empty tensors")
        });
        crate::autograd::set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);

        for out in [&q_out, &k_out, &v_out] {
            assert!(out.is_cuda());
            assert_eq!(out.dtype(), DType::BF16);
            assert_eq!(out.len(), 0);
            assert!(out.cloned_cuda_f32_buffer().is_none());
        }
        assert_eq!(q_out.shape_vec(), vec![batch, 1, seq, hidden]);
        assert_eq!(k_out.shape_vec(), vec![batch, 1, seq, hidden]);
        assert_eq!(v_out.shape_vec(), vec![batch, 1, seq, hidden]);
    }
}
