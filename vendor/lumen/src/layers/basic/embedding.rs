use crate::autograd::{
    StoragePreference, Tensor, TensorData, TensorStorageOwned, TensorStorageView,
    assert_native_device_support, is_no_grad, is_strict_device_execution,
};
use crate::init::{InitType, tensor_init, tensor_init_with_dtype};
use crate::module::Module;
use crate::ops::cuda;
use crate::precision::DType;
use half::{bf16, f16};
use ndarray::{Array, IxDyn, Zip};
use std::cell::RefCell;
use std::ops::AddAssign;
use std::rc::Rc;

pub struct Embedding {
    pub weight: Tensor,
    pub vocab_size: usize,
    pub embed_dim: usize,
}

#[inline]
fn parse_embedding_index(value: f32, position: usize, vocab_size: usize) -> usize {
    assert!(
        value.is_finite(),
        "Embedding index at position {} must be finite, got {}",
        position,
        value
    );
    assert!(
        value >= 0.0,
        "Embedding index at position {} must be >= 0, got {}",
        position,
        value
    );
    assert!(
        value.fract() == 0.0,
        "Embedding index at position {} must be an integer, got {}",
        position,
        value
    );
    let idx = value as usize;
    assert!(
        idx < vocab_size,
        "Embedding index out of bounds at position {}: {} >= {}",
        position,
        idx,
        vocab_size
    );
    idx
}

impl Embedding {
    pub fn new(vocab_size: usize, embed_dim: usize) -> Self {
        let weight = tensor_init(vec![vocab_size, embed_dim], InitType::KaimingNormal);
        Self {
            weight,
            vocab_size,
            embed_dim,
        }
    }

    pub fn new_with_dtype(vocab_size: usize, embed_dim: usize, dtype: DType) -> Self {
        let weight =
            tensor_init_with_dtype(vec![vocab_size, embed_dim], InitType::KaimingNormal, dtype);
        Self {
            weight,
            vocab_size,
            embed_dim,
        }
    }

    pub fn forward(&self, indices: &Tensor) -> Tensor {
        let output_device = self.weight.device();
        let build_graph = !is_no_grad() && self.weight.requires_grad();
        assert_native_device_support(
            output_device,
            "embedding",
            output_device == crate::autograd::Device::Cuda,
        );
        assert_eq!(
            indices.device(),
            output_device,
            "embedding expects indices and weight on the same device"
        );
        let e_dim = self.embed_dim;
        let v_size = self.vocab_size;

        let mut out_shape = indices.shape_vec();
        out_shape.push(e_dim);

        let num_elements = indices.len();
        if output_device == crate::autograd::Device::Cuda && num_elements > 0 {
            let cuda_out = indices.with_cuda_f32_buffer(|indices_buf| {
                self.weight.with_cuda_f32_buffer(|weight_buf| {
                    cuda::embedding_f32(indices_buf, weight_buf, num_elements, v_size, e_dim)
                })
            });
            match cuda_out {
                Ok((buffer, out)) => {
                    let out = Array::from_shape_vec(IxDyn(&out_shape), out)
                        .expect("CUDA embedding output shape build failed")
                        .into_dyn();
                    if !build_graph {
                        return Tensor::from_f32_data_no_grad_with_device_dtype_and_cuda_buffer(
                            out,
                            self.weight.dtype(),
                            output_device,
                            Some(buffer),
                        );
                    }

                    let indices_clone = indices.clone();
                    let w_clone = self.weight.clone();
                    let output_self = Rc::new(RefCell::new(None::<Tensor>));
                    let output_self_for_backward = output_self.clone();
                    let tensor = Tensor(Rc::new(RefCell::new(TensorData {
                        data: out.into_shared(),
                        f16_data: None,
                        bf16_data: None,
                        i8_data: None,
                        cuda_f32_data: Some(buffer),
                        i8_scale: None,
                        has_f32_data: true,
                        storage_dtype: crate::precision::DType::F32,
                        cache_dirty: false,
                        is_parameter: false,
                        grad: None,
                        cuda_f32_grad: None,
                        parents: vec![indices.clone(), self.weight.clone()],
                        backward_op: Some(std::rc::Rc::new(move |grad| {
                            let upstream_cuda_grad = output_self_for_backward
                                .borrow()
                                .as_ref()
                                .and_then(|output| output.cloned_cuda_f32_grad())
                                .filter(|grad_buf| grad_buf.len() == grad.len());
                            let cuda_grad = if let Some(grad_buf) = upstream_cuda_grad {
                                if is_strict_device_execution() {
                                    match indices_clone.with_cuda_f32_buffer(|indices_buf| {
                                        cuda::embedding_backward_f32_buffer(
                                            indices_buf,
                                            &grad_buf,
                                            num_elements,
                                            v_size,
                                            e_dim,
                                        )
                                    }) {
                                        Ok(grad_buffer) => {
                                            w_clone.add_cuda_grad_buffer_only(grad_buffer);
                                            return;
                                        }
                                        Err(err) => {
                                            panic!(
                                                "embedding CUDA backward failed while strict device execution is enabled: {err}"
                                            );
                                        }
                                    }
                                }
                                indices_clone.with_cuda_f32_buffer(|indices_buf| {
                                    cuda::embedding_backward_f32(
                                        indices_buf,
                                        &grad_buf,
                                        num_elements,
                                        v_size,
                                        e_dim,
                                    )
                                })
                            } else {
                                let grad_host = grad.iter().copied().collect::<Vec<_>>();
                                if is_strict_device_execution() {
                                    match cuda::upload_f32(&grad_host).and_then(|grad_buf| {
                                        indices_clone.with_cuda_f32_buffer(|indices_buf| {
                                            cuda::embedding_backward_f32_buffer(
                                                indices_buf,
                                                &grad_buf,
                                                num_elements,
                                                v_size,
                                                e_dim,
                                            )
                                        })
                                    }) {
                                        Ok(grad_buffer) => {
                                            w_clone.add_cuda_grad_buffer_only(grad_buffer);
                                            return;
                                        }
                                        Err(err) => {
                                            panic!(
                                                "embedding CUDA backward failed while strict device execution is enabled: {err}"
                                            );
                                        }
                                    }
                                }
                                cuda::upload_f32(&grad_host).and_then(|grad_buf| {
                                    indices_clone.with_cuda_f32_buffer(|indices_buf| {
                                        cuda::embedding_backward_f32(
                                            indices_buf,
                                            &grad_buf,
                                            num_elements,
                                            v_size,
                                            e_dim,
                                        )
                                    })
                                })
                            };
                            match cuda_grad {
                                Ok((grad_buffer, grad_weight_host)) => {
                                    let grad_weight = Array::from_shape_vec(
                                        IxDyn(&[v_size, e_dim]),
                                        grad_weight_host,
                                    )
                                    .expect("CUDA embedding weight grad shape build failed")
                                    .into_dyn();
                                    w_clone
                                        .add_grad_with_cuda_buffer(grad_weight, Some(grad_buffer));
                                }
                                Err(err) => {
                                    assert!(
                                        !is_strict_device_execution(),
                                        "embedding CUDA backward failed while strict device execution is enabled: {err}"
                                    );
                                    let binding = indices_clone.data_ref();
                                    let idx_flat = binding.view().into_shape(num_elements).unwrap();
                                    let grad_2d =
                                        grad.view().into_shape((num_elements, e_dim)).unwrap();

                                    let mut d_w = Array::zeros((v_size, e_dim));
                                    for (i, &idx_f32) in idx_flat.iter().enumerate() {
                                        let idx = parse_embedding_index(idx_f32, i, v_size);
                                        d_w.slice_mut(ndarray::s![idx, ..])
                                            .add_assign(&grad_2d.slice(ndarray::s![i, ..]));
                                    }
                                    w_clone.add_grad(d_w.into_dyn());
                                }
                            }
                        })),
                        requires_grad: true,
                        device: output_device,
                    })));
                    *output_self.borrow_mut() = Some(tensor.clone());
                    return tensor;
                }
                Err(err) => {
                    assert!(
                        !is_strict_device_execution(),
                        "embedding CUDA forward failed while strict device execution is enabled: {err}"
                    );
                }
            }
        }

        let idx_values = indices.with_storage_view(|idx_view| match idx_view {
            TensorStorageView::F32(idx_view) => idx_view
                .iter()
                .enumerate()
                .map(|(pos, &v)| parse_embedding_index(v, pos, v_size))
                .collect::<Vec<_>>(),
            TensorStorageView::F16(idx_view) => idx_view
                .iter()
                .enumerate()
                .map(|(pos, v)| parse_embedding_index(v.to_f32(), pos, v_size))
                .collect::<Vec<_>>(),
            TensorStorageView::BF16(idx_view) => idx_view
                .iter()
                .enumerate()
                .map(|(pos, v)| parse_embedding_index(v.to_f32(), pos, v_size))
                .collect::<Vec<_>>(),
        });

        if !build_graph {
            if self.weight.dtype() == DType::I8 {
                return match self.weight.native_storage_owned() {
                    TensorStorageOwned::I8(w_data, scale) => {
                        let mut out = ndarray::ArrayD::<i8>::zeros(ndarray::IxDyn(&out_shape));
                        let mut out_flat = out
                            .view_mut()
                            .into_shape((num_elements, e_dim))
                            .expect("Flatten output failed");
                        let w_2d = w_data
                            .view()
                            .into_dimensionality::<ndarray::Ix2>()
                            .expect("Embedding weight must be 2D");
                        Zip::from(out_flat.outer_iter_mut())
                            .and(&idx_values)
                            .par_for_each(|mut out_row, &idx| {
                                let w_row = w_2d.slice(ndarray::s![idx, ..]);
                                out_row.assign(&w_row);
                            });
                        Tensor::from_shared_i8_no_grad_with_device(
                            out.into_shared(),
                            scale,
                            output_device,
                        )
                    }
                    TensorStorageOwned::F32(_)
                    | TensorStorageOwned::F16(_)
                    | TensorStorageOwned::BF16(_) => unreachable!("checked i8 weight above"),
                };
            }

            return self
                .weight
                .with_storage_view_preferring(StoragePreference::Native, |w_view| match w_view {
                    TensorStorageView::F32(w_view) => {
                        let mut out = Array::zeros(out_shape.clone());
                        let mut out_flat = out
                            .view_mut()
                            .into_shape((num_elements, e_dim))
                            .expect("Flatten output failed");
                        let w_2d = w_view
                            .into_dimensionality::<ndarray::Ix2>()
                            .expect("Embedding weight must be 2D");
                        Zip::from(out_flat.outer_iter_mut())
                            .and(&idx_values)
                            .par_for_each(|mut out_row, &idx| {
                                let w_row = w_2d.slice(ndarray::s![idx, ..]);
                                out_row.assign(&w_row);
                            });
                        Tensor::from_f32_data_no_grad_with_device_dtype(
                            out.into_dyn(),
                            DType::F32,
                            output_device,
                        )
                    }
                    TensorStorageView::F16(w_view) => {
                        let mut out = ndarray::ArrayD::<f16>::from_elem(
                            ndarray::IxDyn(&out_shape),
                            f16::from_bits(0),
                        );
                        let mut out_flat = out
                            .view_mut()
                            .into_shape((num_elements, e_dim))
                            .expect("Flatten output failed");
                        let w_2d = w_view
                            .into_dimensionality::<ndarray::Ix2>()
                            .expect("Embedding weight must be 2D");
                        Zip::from(out_flat.outer_iter_mut())
                            .and(&idx_values)
                            .par_for_each(|mut out_row, &idx| {
                                let w_row = w_2d.slice(ndarray::s![idx, ..]);
                                out_row.assign(&w_row);
                            });
                        Tensor::from_shared_f16_no_grad_with_device(
                            out.into_shared(),
                            output_device,
                        )
                    }
                    TensorStorageView::BF16(w_view) => {
                        let mut out = ndarray::ArrayD::<bf16>::from_elem(
                            ndarray::IxDyn(&out_shape),
                            bf16::from_bits(0),
                        );
                        let mut out_flat = out
                            .view_mut()
                            .into_shape((num_elements, e_dim))
                            .expect("Flatten output failed");
                        let w_2d = w_view
                            .into_dimensionality::<ndarray::Ix2>()
                            .expect("Embedding weight must be 2D");
                        Zip::from(out_flat.outer_iter_mut())
                            .and(&idx_values)
                            .par_for_each(|mut out_row, &idx| {
                                let w_row = w_2d.slice(ndarray::s![idx, ..]);
                                out_row.assign(&w_row);
                            });
                        Tensor::from_shared_bf16_no_grad_with_device(
                            out.into_shared(),
                            output_device,
                        )
                    }
                });
        }

        let mut out = Array::zeros(out_shape);
        let mut out_flat = out
            .view_mut()
            .into_shape((num_elements, e_dim))
            .expect("Flatten output failed");

        self.weight.with_storage_view(|w_view| match w_view {
            TensorStorageView::F32(w_view) => {
                let w_2d = w_view
                    .into_dimensionality::<ndarray::Ix2>()
                    .expect("Embedding weight must be 2D");
                Zip::from(out_flat.outer_iter_mut())
                    .and(&idx_values)
                    .par_for_each(|mut out_row, &idx| {
                        let w_row = w_2d.slice(ndarray::s![idx, ..]);
                        out_row.assign(&w_row);
                    });
            }
            TensorStorageView::F16(w_view) => {
                let w_2d = w_view
                    .into_dimensionality::<ndarray::Ix2>()
                    .expect("Embedding weight must be 2D");
                Zip::from(out_flat.outer_iter_mut())
                    .and(&idx_values)
                    .par_for_each(|mut out_row, &idx| {
                        let w_row = w_2d.slice(ndarray::s![idx, ..]);
                        for (dst, &src) in out_row.iter_mut().zip(w_row.iter()) {
                            *dst = src.to_f32();
                        }
                    });
            }
            TensorStorageView::BF16(w_view) => {
                let w_2d = w_view
                    .into_dimensionality::<ndarray::Ix2>()
                    .expect("Embedding weight must be 2D");
                Zip::from(out_flat.outer_iter_mut())
                    .and(&idx_values)
                    .par_for_each(|mut out_row, &idx| {
                        let w_row = w_2d.slice(ndarray::s![idx, ..]);
                        for (dst, &src) in out_row.iter_mut().zip(w_row.iter()) {
                            *dst = src.to_f32();
                        }
                    });
            }
        });

        let out_dyn = out.into_dyn();

        let indices_clone = indices.clone();
        let w_clone = self.weight.clone();
        let v_snap = v_size;
        let e_snap = e_dim;

        Tensor(Rc::new(RefCell::new(TensorData {
            data: out_dyn.into_shared(),
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
            parents: vec![indices.clone(), self.weight.clone()],
            backward_op: Some(std::rc::Rc::new(move |grad| {
                let binding = indices_clone.data_ref();
                let idx_flat = binding.view().into_shape(num_elements).unwrap();
                let grad_2d = grad.view().into_shape((num_elements, e_snap)).unwrap();

                let mut d_w = Array::zeros((v_snap, e_snap));
                for (i, &idx_f32) in idx_flat.iter().enumerate() {
                    let idx = parse_embedding_index(idx_f32, i, v_snap);
                    d_w.slice_mut(ndarray::s![idx, ..])
                        .add_assign(&grad_2d.slice(ndarray::s![i, ..]));
                }
                w_clone.add_grad(d_w.into_dyn());
            })),
            requires_grad: true,
            device: output_device,
        })))
    }
}

impl Module for Embedding {
    fn forward(&self, x: Tensor) -> Tensor {
        self.forward(&x)
    }
    fn parameters(&self) -> Vec<Tensor> {
        vec![self.weight.clone()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autograd::no_grad;
    use crate::precision::{PrecisionConfig, with_precision_config};
    use ndarray::{Array, IxDyn};

    fn make_tensor(shape: &[usize], data: Vec<f32>, dtype: DType) -> Tensor {
        let t = Tensor::from_array_no_grad(
            Array::from_shape_vec(IxDyn(shape), data)
                .expect("test tensor shape mismatch")
                .into_dyn(),
        );
        t.cast_inplace(dtype);
        t
    }

    #[test]
    fn embedding_accepts_integer_indices() {
        let emb = Embedding::new(8, 4);
        let indices = make_tensor(&[1, 3], vec![0.0, 2.0, 7.0], DType::F32);
        let out = no_grad(|| emb.forward(&indices));
        assert_eq!(out.shape_vec(), vec![1, 3, 4]);
    }

    #[test]
    #[should_panic(expected = "must be an integer")]
    fn embedding_rejects_fractional_indices() {
        let emb = Embedding::new(8, 4);
        let indices = make_tensor(&[1, 1], vec![1.5], DType::F32);
        no_grad(|| {
            let _ = emb.forward(&indices);
        });
    }

    #[test]
    #[should_panic(expected = "must be >= 0")]
    fn embedding_rejects_negative_indices() {
        let emb = Embedding::new(8, 4);
        let indices = make_tensor(&[1, 1], vec![-1.0], DType::BF16);
        no_grad(|| {
            let _ = emb.forward(&indices);
        });
    }

    #[test]
    #[should_panic(expected = "must be finite")]
    fn embedding_rejects_nan_indices() {
        let emb = Embedding::new(8, 4);
        let indices = make_tensor(&[1, 1], vec![f32::NAN], DType::F32);
        no_grad(|| {
            let _ = emb.forward(&indices);
        });
    }

    #[test]
    fn embedding_no_grad_preserves_bf16_weight_dtype() {
        let emb = Embedding::new(4, 2);
        emb.weight.set_array_f32_with_dtype(
            Array::from_shape_vec(IxDyn(&[4, 2]), vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0])
                .expect("weight shape mismatch")
                .into_dyn(),
            DType::BF16,
        );
        let indices = make_tensor(&[1, 2], vec![1.0, 3.0], DType::F32);

        let out = no_grad(|| emb.forward(&indices));
        assert_eq!(out.dtype(), DType::BF16);
        out.with_storage_view(|view| match view {
            TensorStorageView::BF16(view) => {
                let vals = view.iter().map(|v| v.to_f32()).collect::<Vec<_>>();
                assert_eq!(vals, vec![2.0, 3.0, 6.0, 7.0]);
            }
            TensorStorageView::F16(_) => {
                panic!("bf16 embedding output should stay bf16 in no-grad")
            }
            TensorStorageView::F32(_) => {
                panic!("bf16 embedding output should stay bf16 in no-grad")
            }
        });
    }

    #[test]
    fn embedding_no_grad_preserves_i8_weight_dtype() {
        let emb = Embedding::new(4, 2);
        emb.weight.set_array_f32_with_dtype(
            Array::from_shape_vec(IxDyn(&[4, 2]), vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0])
                .expect("weight shape mismatch")
                .into_dyn(),
            DType::I8,
        );
        let indices = make_tensor(&[1, 2], vec![1.0, 3.0], DType::F32);

        let out = no_grad(|| emb.forward(&indices));
        assert_eq!(out.dtype(), DType::I8);
        let ref_vals = no_grad(|| {
            let ref_emb = Embedding::new(4, 2);
            ref_emb.weight.set_array_f32_with_dtype(
                Array::from_shape_vec(IxDyn(&[4, 2]), vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0])
                    .expect("weight shape mismatch")
                    .into_dyn(),
                DType::I8,
            );
            ref_emb
                .forward(&indices)
                .data_ref()
                .iter()
                .copied()
                .collect::<Vec<_>>()
        });
        let out_vals = out.data_ref().iter().copied().collect::<Vec<_>>();
        assert_eq!(out_vals, ref_vals);
    }

    #[test]
    fn embedding_explicit_dtype_overrides_global_default() {
        with_precision_config(
            PrecisionConfig {
                parameter_dtype: DType::BF16,
                runtime_dtype: DType::F32,
                allow_parameter_dtype_copies: false,
            },
            || {
                let emb = Embedding::new_with_dtype(8, 4, DType::F32);
                assert_eq!(emb.weight.dtype(), DType::F32);
            },
        );
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_embedding_matches_cpu_reference_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let weight_values = vec![
            0.0, 1.0, 2.0, 3.0, 4.0, 5.0, -1.0, -2.0, -3.0, 1.5, 2.5, 3.5, 7.0, 8.0, 9.0, -4.0,
            -5.0, -6.0,
        ];
        let emb = Embedding::new(6, 3);
        emb.weight.set_array_f32_with_dtype(
            Array::from_shape_vec(IxDyn(&[6, 3]), weight_values.clone())
                .expect("weight shape mismatch")
                .into_dyn(),
            DType::BF16,
        );
        emb.weight.to_cuda_inplace();
        let emb_ref = Embedding::new(6, 3);
        emb_ref.weight.set_array_f32_with_dtype(
            Array::from_shape_vec(IxDyn(&[6, 3]), weight_values)
                .expect("weight shape mismatch")
                .into_dyn(),
            DType::BF16,
        );

        let indices = make_tensor(&[2, 2], vec![0.0, 2.0, 5.0, 1.0], DType::F32).to_cuda();

        crate::ops::cuda::set_enabled(true);
        crate::autograd::set_strict_device_execution(true);
        let cuda_out = no_grad(|| emb.forward(&indices));
        crate::autograd::set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);

        let cpu_out = no_grad(|| emb_ref.forward(&indices.to_cpu()));
        assert!(cuda_out.is_cuda());
        assert_eq!(cuda_out.dtype(), DType::BF16);

        for (got, expect) in cuda_out.data_ref().iter().zip(cpu_out.data_ref().iter()) {
            assert!((got - expect).abs() < 2e-2, "got {got}, expect {expect}");
        }
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_empty_embedding_no_grad_stays_on_cuda_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let emb = Embedding::new(8, 4);
        emb.weight.set_array_f32_with_dtype(
            Array::from_shape_vec(IxDyn(&[8, 4]), (0..32).map(|v| v as f32 / 8.0).collect())
                .expect("weight shape mismatch")
                .into_dyn(),
            DType::BF16,
        );
        emb.weight.to_cuda_inplace();
        let indices = make_tensor(&[0], vec![], DType::F32).to_cuda();

        crate::ops::cuda::set_enabled(true);
        crate::autograd::set_strict_device_execution(true);
        let out = no_grad(|| emb.forward(&indices));
        crate::autograd::set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);

        assert!(out.is_cuda());
        assert_eq!(out.dtype(), DType::BF16);
        assert_eq!(out.shape_vec(), vec![0, 4]);
        assert_eq!(out.len(), 0);
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_embedding_backward_matches_cpu_reference_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let weight_values = vec![
            0.0, 1.0, 2.0, 3.0, 4.0, 5.0, -1.0, -2.0, -3.0, 1.5, 2.5, 3.5, 7.0, 8.0, 9.0, -4.0,
            -5.0, -6.0,
        ];
        let indices_cpu = make_tensor(&[2, 3], vec![0.0, 2.0, 2.0, 5.0, 1.0, 0.0], DType::F32);
        let coeff_cpu = make_tensor(
            &[2, 3, 3],
            vec![
                0.5, -1.0, 2.0, 0.25, -0.75, 1.5, 1.25, 0.5, -0.25, -1.5, 0.75, 0.0, 0.1, -0.2,
                0.3, 2.0, -1.0, 0.5,
            ],
            DType::F32,
        );

        let emb_cuda = Embedding::new(6, 3);
        emb_cuda.weight.set_array_f32_with_dtype(
            Array::from_shape_vec(IxDyn(&[6, 3]), weight_values.clone())
                .expect("weight shape mismatch")
                .into_dyn(),
            DType::F32,
        );
        emb_cuda.weight.to_cuda_inplace();
        let indices_cuda = indices_cpu.to_cuda();
        let coeff_cuda = coeff_cpu.to_cuda();

        crate::ops::cuda::set_enabled(true);
        crate::autograd::set_strict_device_execution(true);
        let cuda_out = emb_cuda.forward(&indices_cuda);
        let cuda_loss = crate::ops::arithmetic::sum(&(&cuda_out * &coeff_cuda));
        cuda_loss.backward();
        assert!(emb_cuda.weight.cloned_cuda_f32_grad().is_some());
        assert!(!emb_cuda.weight.has_host_grad());
        crate::autograd::set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);

        let emb_cpu = Embedding::new(6, 3);
        emb_cpu.weight.set_array_f32_with_dtype(
            Array::from_shape_vec(IxDyn(&[6, 3]), weight_values)
                .expect("weight shape mismatch")
                .into_dyn(),
            DType::F32,
        );
        let cpu_out = emb_cpu.forward(&indices_cpu);
        let cpu_loss = crate::ops::arithmetic::sum(&(&cpu_out * &coeff_cpu));
        cpu_loss.backward();

        let cuda_grad = emb_cuda.weight.grad().expect("cuda embedding weight grad");
        let cpu_grad = emb_cpu.weight.grad().expect("cpu embedding weight grad");
        for (got, expect) in cuda_grad.iter().zip(cpu_grad.iter()) {
            assert!((got - expect).abs() < 1e-5, "got {got}, expect {expect}");
        }
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_embedding_preserves_i8_dtype_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let weight_values = vec![
            0.0, 1.0, 2.0, 3.0, 4.0, 5.0, -1.0, -2.0, -3.0, 1.5, 2.5, 3.5,
        ];
        let emb = Embedding::new(4, 3);
        emb.weight.set_array_f32_with_dtype(
            Array::from_shape_vec(IxDyn(&[4, 3]), weight_values.clone())
                .expect("weight shape mismatch")
                .into_dyn(),
            DType::I8,
        );
        emb.weight.to_cuda_inplace();
        let emb_ref = Embedding::new(4, 3);
        emb_ref.weight.set_array_f32_with_dtype(
            Array::from_shape_vec(IxDyn(&[4, 3]), weight_values)
                .expect("weight shape mismatch")
                .into_dyn(),
            DType::I8,
        );

        let indices = make_tensor(&[2, 2], vec![0.0, 2.0, 3.0, 1.0], DType::F32).to_cuda();

        crate::ops::cuda::set_enabled(true);
        crate::autograd::set_strict_device_execution(true);
        let cuda_out = no_grad(|| emb.forward(&indices));
        crate::autograd::set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);

        let cpu_out = no_grad(|| emb_ref.forward(&indices.to_cpu()));
        assert!(cuda_out.is_cuda());
        assert_eq!(cuda_out.dtype(), DType::I8);
        for (got, expect) in cuda_out.data_ref().iter().zip(cpu_out.data_ref().iter()) {
            assert!((got - expect).abs() < 1e-5, "got {got}, expect {expect}");
        }
    }
}
