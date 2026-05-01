// src/layers/linear.rs
use crate::autograd::{Tensor, TensorStorageOwned, TensorStorageView, is_no_grad};
use crate::init::{InitType, tensor_init, tensor_init_with_dtype}; // 引入 Init
use crate::module::Module;
use crate::ops::matmul::{SliceRef, matmul, matvec_rowmajor_parallel_mixed};
use crate::precision::{DType, allow_parameter_dtype_copies};
use half::f16;
use ndarray::{ArrayD, Axis, Ix1, IxDyn};

pub struct Linear {
    pub weight: Tensor,       // shape: [out_features, in_features]
    pub bias: Option<Tensor>, // shape: [out_features]
    pub in_features: usize,
    pub out_features: usize,
}

impl Linear {
    pub fn new(in_features: usize, out_features: usize) -> Self {
        let weight = tensor_init(vec![out_features, in_features], InitType::KaimingNormal);
        let bias = tensor_init(vec![out_features], InitType::Zeros);
        Linear {
            weight,
            bias: Some(bias),
            in_features,
            out_features,
        }
    }

    pub fn new_with_dtype(in_features: usize, out_features: usize, dtype: DType) -> Self {
        // 注意：为了对齐 PyTorch/HF nn.Linear.weight 的布局，weight 存成 [out, in]
        let weight = tensor_init_with_dtype(
            vec![out_features, in_features],
            InitType::KaimingNormal,
            dtype,
        );

        let bias = tensor_init_with_dtype(vec![out_features], InitType::Zeros, dtype);

        Linear {
            weight,
            bias: Some(bias),
            in_features,
            out_features,
        }
    }

    pub fn new_no_bias(in_features: usize, out_features: usize) -> Self {
        let weight = tensor_init(vec![out_features, in_features], InitType::KaimingNormal);

        Linear {
            weight,
            bias: None,
            in_features,
            out_features,
        }
    }

    pub fn new_no_bias_with_dtype(in_features: usize, out_features: usize, dtype: DType) -> Self {
        let weight = tensor_init_with_dtype(
            vec![out_features, in_features],
            InitType::KaimingNormal,
            dtype,
        );

        Linear {
            weight,
            bias: None,
            in_features,
            out_features,
        }
    }

    #[inline]
    pub fn forward_decode_slice_no_bias_into(&self, input: &[f32], out: &mut [f32]) {
        assert!(
            is_no_grad(),
            "forward_decode_slice_no_bias_into is inference-only"
        );
        assert!(
            self.bias.is_none(),
            "forward_decode_slice_no_bias_into currently expects no bias"
        );
        assert_eq!(input.len(), self.in_features, "input width mismatch");
        assert_eq!(out.len(), self.out_features, "output width mismatch");

        if self.weight.dtype() == DType::I8 && !allow_parameter_dtype_copies() {
            let weight_owned = self.weight.native_storage_owned();
            match weight_owned {
                TensorStorageOwned::I8(weight_data, scale) => {
                    let weight2 = weight_data
                        .view()
                        .into_dimensionality::<ndarray::Ix2>()
                        .expect("Linear weight must be 2D [out,in]");
                    assert_eq!(
                        weight2.dim(),
                        (self.out_features, self.in_features),
                        "Linear weight shape mismatch: expected [{}, {}], got {:?}",
                        self.out_features,
                        self.in_features,
                        weight2.dim()
                    );
                    let weight_slice = SliceRef::I8(
                        weight2
                            .as_slice()
                            .expect("Linear i8 weight must be contiguous"),
                        scale,
                    );
                    matvec_rowmajor_parallel_mixed(
                        SliceRef::F32(input),
                        weight_slice,
                        self.out_features,
                        self.in_features,
                        out,
                    );
                    return;
                }
                _ => unreachable!("i8 linear weight unexpectedly exposed non-i8 storage"),
            }
        }

        self.weight
            .with_storage_view(|weight_view| match weight_view {
                TensorStorageView::F32(weight_view) => {
                    let weight2 = weight_view
                        .into_dimensionality::<ndarray::Ix2>()
                        .expect("Linear weight must be 2D [out,in]");
                    assert_eq!(
                        weight2.dim(),
                        (self.out_features, self.in_features),
                        "Linear weight shape mismatch: expected [{}, {}], got {:?}",
                        self.out_features,
                        self.in_features,
                        weight2.dim()
                    );
                    let weight_owned;
                    let weight_slice = if let Some(s) = weight2.as_slice() {
                        SliceRef::F32(s)
                    } else {
                        weight_owned = weight2.iter().copied().collect::<Vec<f32>>();
                        SliceRef::F32(weight_owned.as_slice())
                    };
                    matvec_rowmajor_parallel_mixed(
                        SliceRef::F32(input),
                        weight_slice,
                        self.out_features,
                        self.in_features,
                        out,
                    );
                }
                TensorStorageView::F16(weight_view) => {
                    let weight2 = weight_view
                        .into_dimensionality::<ndarray::Ix2>()
                        .expect("Linear weight must be 2D [out,in]");
                    assert_eq!(
                        weight2.dim(),
                        (self.out_features, self.in_features),
                        "Linear weight shape mismatch: expected [{}, {}], got {:?}",
                        self.out_features,
                        self.in_features,
                        weight2.dim()
                    );
                    let weight_owned;
                    let weight_slice = if let Some(s) = weight2.as_slice() {
                        SliceRef::F16(s)
                    } else {
                        weight_owned = weight2.iter().copied().collect::<Vec<f16>>();
                        SliceRef::F16(weight_owned.as_slice())
                    };
                    matvec_rowmajor_parallel_mixed(
                        SliceRef::F32(input),
                        weight_slice,
                        self.out_features,
                        self.in_features,
                        out,
                    );
                }
                TensorStorageView::BF16(weight_view) => {
                    let weight2 = weight_view
                        .into_dimensionality::<ndarray::Ix2>()
                        .expect("Linear weight must be 2D [out,in]");
                    assert_eq!(
                        weight2.dim(),
                        (self.out_features, self.in_features),
                        "Linear weight shape mismatch: expected [{}, {}], got {:?}",
                        self.out_features,
                        self.in_features,
                        weight2.dim()
                    );
                    let weight_owned;
                    let weight_slice = if let Some(s) = weight2.as_slice() {
                        SliceRef::BF16(s)
                    } else {
                        weight_owned = weight2.iter().copied().collect::<Vec<_>>();
                        SliceRef::BF16(weight_owned.as_slice())
                    };
                    matvec_rowmajor_parallel_mixed(
                        SliceRef::F32(input),
                        weight_slice,
                        self.out_features,
                        self.in_features,
                        out,
                    );
                }
            });
    }

    pub fn forward_decode_slice_no_bias(&self, input: &[f32]) -> Tensor {
        assert!(
            is_no_grad(),
            "forward_decode_slice_no_bias is inference-only"
        );
        assert!(
            self.bias.is_none(),
            "forward_decode_slice_no_bias currently expects no bias"
        );
        assert_eq!(input.len(), self.in_features, "input width mismatch");

        let mut data = ArrayD::<f32>::zeros(IxDyn(&[1, 1, self.out_features])).into_shared();
        let out_slice = data
            .as_slice_mut()
            .expect("decode linear output should be contiguous");
        self.forward_decode_slice_no_bias_into(input, out_slice);
        Tensor::from_data_no_grad(data)
    }

    pub fn forward_decode_rows_no_bias(&self, input: &[f32], rows: usize) -> Tensor {
        assert!(
            is_no_grad(),
            "forward_decode_rows_no_bias is inference-only"
        );
        assert!(
            self.bias.is_none(),
            "forward_decode_rows_no_bias currently expects no bias"
        );
        assert_eq!(input.len(), rows * self.in_features, "input width mismatch");

        let mut data = ArrayD::<f32>::zeros(IxDyn(&[rows, 1, self.out_features])).into_shared();
        let out_slice = data
            .as_slice_mut()
            .expect("decode linear output should be contiguous");
        for row in 0..rows {
            let in_start = row * self.in_features;
            let out_start = row * self.out_features;
            self.forward_decode_slice_no_bias_into(
                &input[in_start..in_start + self.in_features],
                &mut out_slice[out_start..out_start + self.out_features],
            );
        }
        Tensor::from_data_no_grad(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autograd::no_grad;
    use crate::precision::{
        ParameterQuantization, PrecisionConfig, with_parameter_quantization, with_precision_config,
    };
    use ndarray::{ArrayD, IxDyn};

    #[test]
    fn linear_explicit_dtype_overrides_global_default() {
        with_precision_config(
            PrecisionConfig {
                parameter_dtype: DType::BF16,
                runtime_dtype: DType::F32,
                allow_parameter_dtype_copies: false,
            },
            || {
                let linear = Linear::new_with_dtype(4, 3, DType::F32);
                assert_eq!(linear.weight.dtype(), DType::F32);
                assert_eq!(
                    linear.bias.as_ref().expect("linear bias").dtype(),
                    DType::F32
                );
            },
        );
    }

    #[test]
    fn linear_decode_i8_no_copy_uses_native_weight_without_materializing_f32_cache() {
        with_precision_config(
            PrecisionConfig {
                parameter_dtype: DType::I8,
                runtime_dtype: DType::F32,
                allow_parameter_dtype_copies: false,
            },
            || {
                let linear = Linear::new_no_bias_with_dtype(4, 3, DType::I8);
                linear.weight.set_array_i8_with_dtype(
                    ArrayD::from_shape_vec(
                        IxDyn(&[3, 4]),
                        vec![2i8, -1, 0, 4, -3, 1, 2, -2, 1, 1, -4, 3],
                    )
                    .expect("weight shape"),
                    0.25,
                    DType::I8,
                );
                {
                    let inner = linear.weight.0.borrow();
                    assert_eq!(inner.storage_dtype, DType::I8);
                    assert!(
                        !inner.has_f32_data,
                        "i8 weight should start without f32 cache"
                    );
                }

                let input = [1.0f32, -2.0, 0.5, 3.0];
                let mut out = [0.0f32; 3];
                no_grad(|| {
                    linear.forward_decode_slice_no_bias_into(&input, &mut out);
                });

                let expected = [
                    (2.0 * 1.0 + -1.0 * -2.0 + 0.0 * 0.5 + 4.0 * 3.0) * 0.25,
                    (-3.0 * 1.0 + 1.0 * -2.0 + 2.0 * 0.5 + -2.0 * 3.0) * 0.25,
                    (1.0 * 1.0 + 1.0 * -2.0 + -4.0 * 0.5 + 3.0 * 3.0) * 0.25,
                ];
                for (got, want) in out.iter().zip(expected.iter()) {
                    assert!((got - want).abs() < 1e-5, "got {got}, expected {want}");
                }

                let inner = linear.weight.0.borrow();
                assert!(
                    !inner.has_f32_data,
                    "native i8 decode path should not materialize f32 cache"
                );
            },
        );
    }

    #[test]
    fn linear_decode_rows_no_bias_matches_single_row_decode() {
        let linear = Linear::new_no_bias_with_dtype(4, 3, DType::BF16);
        linear.weight.set_array_f32_with_dtype(
            ArrayD::from_shape_vec(
                IxDyn(&[3, 4]),
                vec![
                    1.0, 0.0, -1.0, 2.0, 0.5, 1.5, -0.5, 0.25, -1.0, 2.0, 1.0, -0.5,
                ],
            )
            .expect("weight shape"),
            DType::BF16,
        );
        let input = vec![1.0f32, -2.0, 0.5, 3.0, -0.25, 0.75, 2.0, -1.5];

        let out = no_grad(|| linear.forward_decode_rows_no_bias(&input, 2));
        let first = no_grad(|| linear.forward_decode_slice_no_bias(&input[..4]));
        let second = no_grad(|| linear.forward_decode_slice_no_bias(&input[4..]));
        let out_vals = out.data_ref().iter().copied().collect::<Vec<_>>();
        let first_vals = first.data_ref().iter().copied().collect::<Vec<_>>();
        let second_vals = second.data_ref().iter().copied().collect::<Vec<_>>();

        assert_eq!(out.shape_vec(), vec![2, 1, 3]);
        assert_eq!(&out_vals[..3], first_vals.as_slice());
        assert_eq!(&out_vals[3..], second_vals.as_slice());
    }

    #[test]
    fn linear_default_construction_can_follow_global_quantization() {
        with_precision_config(
            PrecisionConfig {
                parameter_dtype: DType::F32,
                runtime_dtype: DType::F32,
                allow_parameter_dtype_copies: false,
            },
            || {
                with_parameter_quantization(ParameterQuantization::Int8, || {
                    let linear = Linear::new(4, 3);
                    assert_eq!(linear.weight.dtype(), DType::I8);
                    assert_eq!(
                        linear.bias.as_ref().expect("linear bias").dtype(),
                        DType::I8
                    );
                });
            },
        );
    }
}

impl Module for Linear {
    fn forward(&self, input: Tensor) -> Tensor {
        let y = matmul(&input, &self.weight);

        if let Some(bias) = &self.bias {
            if is_no_grad() {
                let bias_vals = bias.with_storage_view(|bias_view| match bias_view {
                    TensorStorageView::F32(bias_view) => {
                        let bias_1d = bias_view
                            .into_dimensionality::<Ix1>()
                            .expect("Linear bias must be 1D [out]");
                        bias_1d.iter().copied().collect::<Vec<f32>>()
                    }
                    TensorStorageView::F16(bias_view) => {
                        let bias_1d = bias_view
                            .into_dimensionality::<Ix1>()
                            .expect("Linear bias must be 1D [out]");
                        bias_1d.iter().map(|v| v.to_f32()).collect::<Vec<f32>>()
                    }
                    TensorStorageView::BF16(bias_view) => {
                        let bias_1d = bias_view
                            .into_dimensionality::<Ix1>()
                            .expect("Linear bias must be 1D [out]");
                        bias_1d.iter().map(|v| v.to_f32()).collect::<Vec<f32>>()
                    }
                });

                {
                    let mut y_data = y.data_mut();
                    let last_axis = Axis(y_data.ndim() - 1);
                    for mut lane in y_data.lanes_mut(last_axis) {
                        for (dst, &b) in lane.iter_mut().zip(bias_vals.iter()) {
                            *dst += b;
                        }
                    }
                }
                y
            } else {
                y + bias.clone()
            }
        } else {
            y
        }
    }

    fn parameters(&self) -> Vec<Tensor> {
        let mut params = vec![self.weight.clone()];
        if let Some(b) = &self.bias {
            params.push(b.clone());
        }
        params
    }
}
