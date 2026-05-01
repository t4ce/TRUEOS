use crate::autograd::{
    StoragePreference, Tensor, TensorData, TensorStorageOwned, TensorStorageView,
    assert_native_device_support, is_no_grad, is_strict_device_execution,
};
use crate::module::Module;
use crate::ops::cuda;
use crate::precision::DType;
use half::{bf16, f16};
use ndarray::{Array1, Array2, Zip};
use rayon::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

pub struct RMSNorm {
    pub weight: Tensor,
    pub eps: f32,
}

impl RMSNorm {
    pub fn new(dim: usize, eps: f32) -> Self {
        let data = Array1::ones(dim).into_dyn();
        Self {
            weight: Tensor::parameter(data),
            eps,
        }
    }

    pub fn new_with_dtype(dim: usize, eps: f32, dtype: DType) -> Self {
        let data = Array1::ones(dim).into_dyn();
        Self {
            weight: Tensor::parameter_with_dtype(data, dtype),
            eps,
        }
    }
}

impl Module for RMSNorm {
    fn forward(&self, input: Tensor) -> Tensor {
        let output_device = input.device();
        let build_graph = !is_no_grad() && (input.requires_grad() || self.weight.requires_grad());
        assert_native_device_support(
            output_device,
            "rms_norm",
            output_device == crate::autograd::Device::Cuda,
        );
        assert_eq!(
            self.weight.device(),
            output_device,
            "rms_norm expects input and weight on the same device"
        );
        let input_shape = input.shape_vec();
        assert!(!input_shape.is_empty(), "RMSNorm expects at least 1D input");
        let last_dim = *input_shape.last().expect("checked non-empty shape");
        assert!(
            last_dim > 0,
            "RMSNorm last dimension must be greater than zero"
        );
        assert_eq!(
            self.weight.len(),
            last_dim,
            "RMSNorm weight length must match input last dimension"
        );

        if !build_graph {
            if output_device == crate::autograd::Device::Cuda && input.len() > 0 {
                let shape = input.shape_vec();
                let dim = shape[shape.len() - 1];
                let rows = input.len() / dim;
                let cuda_out = input.with_cuda_f32_buffer(|input_buf| {
                    self.weight.with_cuda_f32_buffer(|weight_buf| {
                        cuda::rms_norm_f32(input_buf, weight_buf, rows, dim, self.eps)
                    })
                });
                if let Ok((buffer, out)) = cuda_out {
                    let out = ndarray::Array::from_shape_vec(ndarray::IxDyn(&shape), out)
                        .expect("CUDA RMSNorm output shape build failed")
                        .into_dyn();
                    return Tensor::from_f32_data_no_grad_with_device_dtype_and_cuda_buffer(
                        out,
                        input.dtype(),
                        output_device,
                        Some(buffer),
                    );
                }
            }

            if input.dtype() == DType::I8 {
                return match input.native_storage_owned() {
                    TensorStorageOwned::I8(input_data, input_scale) => {
                        self.weight.with_storage_view_preferring(
                            StoragePreference::F32Compute,
                            |weight_view| {
                                let shape = input_data.shape().to_vec();
                                let dim = shape[shape.len() - 1];
                                let rows = shape.iter().product::<usize>() / dim;
                                let eps = self.eps;
                                let x_cow = input_data.view().to_owned();
                                let x_2d = x_cow.view().into_shape((rows, dim)).unwrap();
                                let mut output_flat = Array2::<f32>::zeros((rows, dim));

                                match weight_view {
                                    TensorStorageView::F32(weight_view) => {
                                        let w_1d = weight_view
                                            .into_dimensionality::<ndarray::Ix1>()
                                            .expect("RMSNorm weight must be 1D");
                                        let w_slice = w_1d
                                            .as_slice()
                                            .expect("RMSNorm weight should be contiguous");

                                        Zip::from(output_flat.outer_iter_mut())
                                            .and(x_2d.outer_iter())
                                            .par_for_each(|mut out_row, x_row| {
                                                let sum_sq = x_row.fold(0.0f32, |acc, &val| {
                                                    let v = val as f32 * input_scale;
                                                    acc + v * v
                                                });
                                                let rms = (sum_sq / dim as f32 + eps).sqrt();
                                                let inv_rms = 1.0 / rms;

                                                for (o, (&xi, &wi)) in out_row
                                                    .iter_mut()
                                                    .zip(x_row.iter().zip(w_slice))
                                                {
                                                    *o = (xi as f32 * input_scale) * inv_rms * wi;
                                                }
                                            });
                                    }
                                    TensorStorageView::F16(weight_view) => {
                                        let w_1d = weight_view
                                            .into_dimensionality::<ndarray::Ix1>()
                                            .expect("RMSNorm weight must be 1D");
                                        Zip::from(output_flat.outer_iter_mut())
                                            .and(x_2d.outer_iter())
                                            .par_for_each(|mut out_row, x_row| {
                                                let sum_sq = x_row.fold(0.0f32, |acc, &val| {
                                                    let v = val as f32 * input_scale;
                                                    acc + v * v
                                                });
                                                let rms = (sum_sq / dim as f32 + eps).sqrt();
                                                let inv_rms = 1.0 / rms;

                                                for (o, (&xi, &wi)) in out_row
                                                    .iter_mut()
                                                    .zip(x_row.iter().zip(w_1d.iter()))
                                                {
                                                    *o = (xi as f32 * input_scale)
                                                        * inv_rms
                                                        * wi.to_f32();
                                                }
                                            });
                                    }
                                    TensorStorageView::BF16(weight_view) => {
                                        let w_1d = weight_view
                                            .into_dimensionality::<ndarray::Ix1>()
                                            .expect("RMSNorm weight must be 1D");
                                        Zip::from(output_flat.outer_iter_mut())
                                            .and(x_2d.outer_iter())
                                            .par_for_each(|mut out_row, x_row| {
                                                let sum_sq = x_row.fold(0.0f32, |acc, &val| {
                                                    let v = val as f32 * input_scale;
                                                    acc + v * v
                                                });
                                                let rms = (sum_sq / dim as f32 + eps).sqrt();
                                                let inv_rms = 1.0 / rms;

                                                for (o, (&xi, &wi)) in out_row
                                                    .iter_mut()
                                                    .zip(x_row.iter().zip(w_1d.iter()))
                                                {
                                                    *o = (xi as f32 * input_scale)
                                                        * inv_rms
                                                        * wi.to_f32();
                                                }
                                            });
                                    }
                                }

                                let out = Tensor::from_f32_data_no_grad_with_device_dtype(
                                    output_flat.into_shape(shape).unwrap().into_dyn(),
                                    DType::F32,
                                    output_device,
                                );
                                out.cast_inplace(DType::I8);
                                out
                            },
                        )
                    }
                    TensorStorageOwned::F32(_)
                    | TensorStorageOwned::F16(_)
                    | TensorStorageOwned::BF16(_) => unreachable!("checked i8 input above"),
                };
            }

            return input.with_storage_view_preferring(StoragePreference::Native, |input_view| {
                self.weight.with_storage_view(|weight_view| {
                    let shape = match &input_view {
                        TensorStorageView::F32(input_view) => input_view.shape().to_vec(),
                        TensorStorageView::F16(input_view) => input_view.shape().to_vec(),
                        TensorStorageView::BF16(input_view) => input_view.shape().to_vec(),
                    };
                    let dim = shape[shape.len() - 1];
                    let rows = shape.iter().product::<usize>() / dim;
                    let eps = self.eps;

                    match input_view {
                        TensorStorageView::F32(input_view) => {
                            let x_cow = input_view.as_standard_layout();
                            let x_2d = x_cow.view().into_shape((rows, dim)).unwrap();
                            let mut output_flat = Array2::<f32>::zeros((rows, dim));

                            match weight_view {
                                TensorStorageView::F32(weight_view) => {
                                    let w_1d = weight_view
                                        .into_dimensionality::<ndarray::Ix1>()
                                        .expect("RMSNorm weight must be 1D");
                                    let w_slice = w_1d
                                        .as_slice()
                                        .expect("RMSNorm weight should be contiguous");

                                    Zip::from(output_flat.outer_iter_mut())
                                        .and(x_2d.outer_iter())
                                        .par_for_each(|mut out_row, x_row| {
                                            let sum_sq =
                                                x_row.fold(0.0f32, |acc, &val| acc + val * val);
                                            let rms = (sum_sq / dim as f32 + eps).sqrt();
                                            let inv_rms = 1.0 / rms;

                                            for (o, (&xi, &wi)) in
                                                out_row.iter_mut().zip(x_row.iter().zip(w_slice))
                                            {
                                                *o = xi * inv_rms * wi;
                                            }
                                        });
                                }
                                TensorStorageView::F16(weight_view) => {
                                    let w_1d = weight_view
                                        .into_dimensionality::<ndarray::Ix1>()
                                        .expect("RMSNorm weight must be 1D");

                                    Zip::from(output_flat.outer_iter_mut())
                                        .and(x_2d.outer_iter())
                                        .par_for_each(|mut out_row, x_row| {
                                            let sum_sq =
                                                x_row.fold(0.0f32, |acc, &val| acc + val * val);
                                            let rms = (sum_sq / dim as f32 + eps).sqrt();
                                            let inv_rms = 1.0 / rms;

                                            for (o, (&xi, &wi)) in out_row
                                                .iter_mut()
                                                .zip(x_row.iter().zip(w_1d.iter()))
                                            {
                                                *o = xi * inv_rms * wi.to_f32();
                                            }
                                        });
                                }
                                TensorStorageView::BF16(weight_view) => {
                                    let w_1d = weight_view
                                        .into_dimensionality::<ndarray::Ix1>()
                                        .expect("RMSNorm weight must be 1D");

                                    Zip::from(output_flat.outer_iter_mut())
                                        .and(x_2d.outer_iter())
                                        .par_for_each(|mut out_row, x_row| {
                                            let sum_sq =
                                                x_row.fold(0.0f32, |acc, &val| acc + val * val);
                                            let rms = (sum_sq / dim as f32 + eps).sqrt();
                                            let inv_rms = 1.0 / rms;

                                            for (o, (&xi, &wi)) in out_row
                                                .iter_mut()
                                                .zip(x_row.iter().zip(w_1d.iter()))
                                            {
                                                *o = xi * inv_rms * wi.to_f32();
                                            }
                                        });
                                }
                            }

                            Tensor::from_f32_data_no_grad_with_device_dtype(
                                output_flat.into_shape(shape).unwrap().into_dyn(),
                                input.dtype(),
                                output_device,
                            )
                        }
                        TensorStorageView::F16(input_view) => {
                            let x_cow = input_view.as_standard_layout();
                            let x_2d = x_cow.view().into_shape((rows, dim)).unwrap();
                            let mut output_flat =
                                Array2::<f16>::from_elem((rows, dim), f16::from_bits(0));

                            match weight_view {
                                TensorStorageView::F32(weight_view) => {
                                    let w_1d = weight_view
                                        .into_dimensionality::<ndarray::Ix1>()
                                        .expect("RMSNorm weight must be 1D");
                                    let w_slice = w_1d
                                        .as_slice()
                                        .expect("RMSNorm weight should be contiguous");

                                    Zip::from(output_flat.outer_iter_mut())
                                        .and(x_2d.outer_iter())
                                        .par_for_each(|mut out_row, x_row| {
                                            let sum_sq = x_row.fold(0.0f32, |acc, &val| {
                                                let v = val.to_f32();
                                                acc + v * v
                                            });
                                            let rms = (sum_sq / dim as f32 + eps).sqrt();
                                            let inv_rms = 1.0 / rms;

                                            for (o, (&xi, &wi)) in
                                                out_row.iter_mut().zip(x_row.iter().zip(w_slice))
                                            {
                                                *o = f16::from_f32(xi.to_f32() * inv_rms * wi);
                                            }
                                        });
                                }
                                TensorStorageView::F16(weight_view) => {
                                    let w_1d = weight_view
                                        .into_dimensionality::<ndarray::Ix1>()
                                        .expect("RMSNorm weight must be 1D");

                                    Zip::from(output_flat.outer_iter_mut())
                                        .and(x_2d.outer_iter())
                                        .par_for_each(|mut out_row, x_row| {
                                            let sum_sq = x_row.fold(0.0f32, |acc, &val| {
                                                let v = val.to_f32();
                                                acc + v * v
                                            });
                                            let rms = (sum_sq / dim as f32 + eps).sqrt();
                                            let inv_rms = 1.0 / rms;

                                            for (o, (&xi, &wi)) in out_row
                                                .iter_mut()
                                                .zip(x_row.iter().zip(w_1d.iter()))
                                            {
                                                *o = f16::from_f32(
                                                    xi.to_f32() * inv_rms * wi.to_f32(),
                                                );
                                            }
                                        });
                                }
                                TensorStorageView::BF16(weight_view) => {
                                    let w_1d = weight_view
                                        .into_dimensionality::<ndarray::Ix1>()
                                        .expect("RMSNorm weight must be 1D");

                                    Zip::from(output_flat.outer_iter_mut())
                                        .and(x_2d.outer_iter())
                                        .par_for_each(|mut out_row, x_row| {
                                            let sum_sq = x_row.fold(0.0f32, |acc, &val| {
                                                let v = val.to_f32();
                                                acc + v * v
                                            });
                                            let rms = (sum_sq / dim as f32 + eps).sqrt();
                                            let inv_rms = 1.0 / rms;

                                            for (o, (&xi, &wi)) in out_row
                                                .iter_mut()
                                                .zip(x_row.iter().zip(w_1d.iter()))
                                            {
                                                *o = f16::from_f32(
                                                    xi.to_f32() * inv_rms * wi.to_f32(),
                                                );
                                            }
                                        });
                                }
                            }

                            Tensor::from_shared_f16_no_grad_with_device(
                                output_flat
                                    .into_shape(shape)
                                    .unwrap()
                                    .into_dyn()
                                    .into_shared(),
                                output_device,
                            )
                        }
                        TensorStorageView::BF16(input_view) => {
                            let x_cow = input_view.as_standard_layout();
                            let x_2d = x_cow.view().into_shape((rows, dim)).unwrap();
                            let mut output_flat =
                                Array2::<bf16>::from_elem((rows, dim), bf16::from_bits(0));

                            match weight_view {
                                TensorStorageView::F32(weight_view) => {
                                    let w_1d = weight_view
                                        .into_dimensionality::<ndarray::Ix1>()
                                        .expect("RMSNorm weight must be 1D");
                                    let w_slice = w_1d
                                        .as_slice()
                                        .expect("RMSNorm weight should be contiguous");

                                    Zip::from(output_flat.outer_iter_mut())
                                        .and(x_2d.outer_iter())
                                        .par_for_each(|mut out_row, x_row| {
                                            let sum_sq = x_row.fold(0.0f32, |acc, &val| {
                                                let v = val.to_f32();
                                                acc + v * v
                                            });
                                            let rms = (sum_sq / dim as f32 + eps).sqrt();
                                            let inv_rms = 1.0 / rms;

                                            for (o, (&xi, &wi)) in
                                                out_row.iter_mut().zip(x_row.iter().zip(w_slice))
                                            {
                                                *o = bf16::from_f32(xi.to_f32() * inv_rms * wi);
                                            }
                                        });
                                }
                                TensorStorageView::F16(weight_view) => {
                                    let w_1d = weight_view
                                        .into_dimensionality::<ndarray::Ix1>()
                                        .expect("RMSNorm weight must be 1D");

                                    Zip::from(output_flat.outer_iter_mut())
                                        .and(x_2d.outer_iter())
                                        .par_for_each(|mut out_row, x_row| {
                                            let sum_sq = x_row.fold(0.0f32, |acc, &val| {
                                                let v = val.to_f32();
                                                acc + v * v
                                            });
                                            let rms = (sum_sq / dim as f32 + eps).sqrt();
                                            let inv_rms = 1.0 / rms;

                                            for (o, (&xi, &wi)) in out_row
                                                .iter_mut()
                                                .zip(x_row.iter().zip(w_1d.iter()))
                                            {
                                                *o = bf16::from_f32(
                                                    xi.to_f32() * inv_rms * wi.to_f32(),
                                                );
                                            }
                                        });
                                }
                                TensorStorageView::BF16(weight_view) => {
                                    let w_1d = weight_view
                                        .into_dimensionality::<ndarray::Ix1>()
                                        .expect("RMSNorm weight must be 1D");

                                    Zip::from(output_flat.outer_iter_mut())
                                        .and(x_2d.outer_iter())
                                        .par_for_each(|mut out_row, x_row| {
                                            let sum_sq = x_row.fold(0.0f32, |acc, &val| {
                                                let v = val.to_f32();
                                                acc + v * v
                                            });
                                            let rms = (sum_sq / dim as f32 + eps).sqrt();
                                            let inv_rms = 1.0 / rms;

                                            for (o, (&xi, &wi)) in out_row
                                                .iter_mut()
                                                .zip(x_row.iter().zip(w_1d.iter()))
                                            {
                                                *o = bf16::from_f32(
                                                    xi.to_f32() * inv_rms * wi.to_f32(),
                                                );
                                            }
                                        });
                                }
                            }

                            Tensor::from_shared_bf16_no_grad_with_device(
                                output_flat
                                    .into_shape(shape)
                                    .unwrap()
                                    .into_dyn()
                                    .into_shared(),
                                output_device,
                            )
                        }
                    }
                })
            });
        }

        if output_device == crate::autograd::Device::Cuda && input.len() > 0 {
            let shape = input.shape_vec();
            let dim = shape[shape.len() - 1];
            let rows = input.len() / dim;
            let cuda_out = input.with_cuda_f32_buffer(|input_buf| {
                self.weight.with_cuda_f32_buffer(|weight_buf| {
                    cuda::rms_norm_f32(input_buf, weight_buf, rows, dim, self.eps)
                })
            });
            match cuda_out {
                Ok((buffer, output_host)) => {
                    let output_data =
                        ndarray::Array::from_shape_vec(ndarray::IxDyn(&shape), output_host)
                            .expect("CUDA RMSNorm training output shape build failed")
                            .into_dyn();
                    let input_clone = input.clone();
                    let weight_clone = self.weight.clone();
                    let eps = self.eps;
                    let output_self = Rc::new(RefCell::new(None::<Tensor>));
                    let output_self_for_backward = output_self.clone();
                    let tensor = Tensor(Rc::new(RefCell::new(TensorData {
                        data: output_data.into_shared(),
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
                        parents: vec![input.clone(), self.weight.clone()],
                        backward_op: Some(std::rc::Rc::new(move |grad_output| {
                            let upstream_cuda_grad = output_self_for_backward
                                .borrow()
                                .as_ref()
                                .and_then(|output| output.cloned_cuda_f32_grad())
                                .filter(|grad_buf| grad_buf.len() == grad_output.len());
                            let cuda_grad = if let Some(grad_buf) = upstream_cuda_grad {
                                if is_strict_device_execution() {
                                    match input_clone.with_cuda_f32_buffer(|input_buf| {
                                        weight_clone.with_cuda_f32_buffer(|weight_buf| {
                                            cuda::rms_norm_backward_f32_buffers(
                                                input_buf, weight_buf, &grad_buf, rows, dim, eps,
                                            )
                                        })
                                    }) {
                                        Ok((grad_input_buffer, grad_weight_buffer)) => {
                                            input_clone
                                                .add_cuda_grad_buffer_only(grad_input_buffer);
                                            weight_clone
                                                .add_cuda_grad_buffer_only(grad_weight_buffer);
                                            return;
                                        }
                                        Err(err) => {
                                            panic!("CUDA RMSNorm backward failed: {err}");
                                        }
                                    }
                                }
                                input_clone.with_cuda_f32_buffer(|input_buf| {
                                    weight_clone.with_cuda_f32_buffer(|weight_buf| {
                                        cuda::rms_norm_backward_f32(
                                            input_buf, weight_buf, &grad_buf, rows, dim, eps,
                                        )
                                    })
                                })
                            } else {
                                let grad_host = grad_output.iter().copied().collect::<Vec<_>>();
                                if is_strict_device_execution() {
                                    match cuda::upload_f32(&grad_host).and_then(|grad_buf| {
                                        input_clone.with_cuda_f32_buffer(|input_buf| {
                                            weight_clone.with_cuda_f32_buffer(|weight_buf| {
                                                cuda::rms_norm_backward_f32_buffers(
                                                    input_buf, weight_buf, &grad_buf, rows, dim,
                                                    eps,
                                                )
                                            })
                                        })
                                    }) {
                                        Ok((grad_input_buffer, grad_weight_buffer)) => {
                                            input_clone
                                                .add_cuda_grad_buffer_only(grad_input_buffer);
                                            weight_clone
                                                .add_cuda_grad_buffer_only(grad_weight_buffer);
                                            return;
                                        }
                                        Err(err) => {
                                            panic!("CUDA RMSNorm backward failed: {err}");
                                        }
                                    }
                                }
                                cuda::upload_f32(&grad_host).and_then(|grad_buf| {
                                    input_clone.with_cuda_f32_buffer(|input_buf| {
                                        weight_clone.with_cuda_f32_buffer(|weight_buf| {
                                            cuda::rms_norm_backward_f32(
                                                input_buf, weight_buf, &grad_buf, rows, dim, eps,
                                            )
                                        })
                                    })
                                })
                            };
                            match cuda_grad {
                                Ok((
                                    (grad_input_buffer, grad_input_host),
                                    (grad_weight_buffer, grad_weight_host),
                                )) => {
                                    let d_input = ndarray::Array::from_shape_vec(
                                        ndarray::IxDyn(&shape),
                                        grad_input_host,
                                    )
                                    .expect("CUDA RMSNorm input grad shape build failed")
                                    .into_dyn();
                                    let d_weight = Array1::from_vec(grad_weight_host).into_dyn();
                                    input_clone.add_grad_with_cuda_buffer(
                                        d_input,
                                        Some(grad_input_buffer),
                                    );
                                    weight_clone.add_grad_with_cuda_buffer(
                                        d_weight,
                                        Some(grad_weight_buffer),
                                    );
                                }
                                Err(err) => {
                                    panic!("CUDA RMSNorm backward failed: {err}");
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
                        "rms_norm CUDA forward failed while strict device execution is enabled: {err}"
                    );
                }
            }
        }

        let (output_data, rows, dim, shape) = self.weight.with_storage_view(|weight_view| {
            let input_ref = input.data_ref();
            let x = &*input_ref;

            let shape = x.shape().to_vec();
            let dim = shape[shape.len() - 1];
            let rows = x.len() / dim;

            let x_cow = x.as_standard_layout();
            let x_2d = x_cow.view().into_shape((rows, dim)).unwrap();
            let mut output_flat = Array2::<f32>::zeros((rows, dim));
            let eps = self.eps;

            match weight_view {
                TensorStorageView::F32(weight_view) => {
                    let w_1d = weight_view
                        .into_dimensionality::<ndarray::Ix1>()
                        .expect("RMSNorm weight must be 1D");
                    let w_slice = w_1d
                        .as_slice()
                        .expect("RMSNorm weight should be contiguous");

                    Zip::from(output_flat.outer_iter_mut())
                        .and(x_2d.outer_iter())
                        .par_for_each(|mut out_row, x_row| {
                            let sum_sq = x_row.fold(0.0f32, |acc, &val| acc + val * val);
                            let rms = (sum_sq / dim as f32 + eps).sqrt();
                            let inv_rms = 1.0 / rms;

                            for (o, (&xi, &wi)) in out_row.iter_mut().zip(x_row.iter().zip(w_slice))
                            {
                                *o = xi * inv_rms * wi;
                            }
                        });
                }
                TensorStorageView::F16(weight_view) => {
                    let w_1d = weight_view
                        .into_dimensionality::<ndarray::Ix1>()
                        .expect("RMSNorm weight must be 1D");

                    Zip::from(output_flat.outer_iter_mut())
                        .and(x_2d.outer_iter())
                        .par_for_each(|mut out_row, x_row| {
                            let sum_sq = x_row.fold(0.0f32, |acc, &val| acc + val * val);
                            let rms = (sum_sq / dim as f32 + eps).sqrt();
                            let inv_rms = 1.0 / rms;

                            for (o, (&xi, &wi)) in
                                out_row.iter_mut().zip(x_row.iter().zip(w_1d.iter()))
                            {
                                *o = xi * inv_rms * wi.to_f32();
                            }
                        });
                }
                TensorStorageView::BF16(weight_view) => {
                    let w_1d = weight_view
                        .into_dimensionality::<ndarray::Ix1>()
                        .expect("RMSNorm weight must be 1D");

                    Zip::from(output_flat.outer_iter_mut())
                        .and(x_2d.outer_iter())
                        .par_for_each(|mut out_row, x_row| {
                            let sum_sq = x_row.fold(0.0f32, |acc, &val| acc + val * val);
                            let rms = (sum_sq / dim as f32 + eps).sqrt();
                            let inv_rms = 1.0 / rms;

                            for (o, (&xi, &wi)) in
                                out_row.iter_mut().zip(x_row.iter().zip(w_1d.iter()))
                            {
                                *o = xi * inv_rms * wi.to_f32();
                            }
                        });
                }
            }

            let out_d = output_flat.into_shape(shape.clone()).unwrap().into_dyn();
            (out_d, rows, dim, shape)
        });

        let input_clone = input.clone();
        let weight_clone = self.weight.clone();
        let eps = self.eps;

        Tensor(Rc::new(RefCell::new(TensorData {
            data: output_data.into_shared(),
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
            parents: vec![input.clone(), self.weight.clone()],
            backward_op: Some(std::rc::Rc::new(move |grad_output| {
                let (d_input, d_weight) = {
                    let x_ref = input_clone.data_ref();
                    let x_cow = x_ref.as_standard_layout();
                    let x_2d = x_cow.view().into_shape((rows, dim)).unwrap();

                    let w_ref = weight_clone.data_ref();
                    let w_slice = w_ref.as_slice().unwrap();

                    let g_cow = grad_output.as_standard_layout();
                    let g_2d = g_cow.view().into_shape((rows, dim)).unwrap();

                    let mut d_input_flat = Array2::<f32>::zeros((rows, dim));
                    Zip::from(d_input_flat.outer_iter_mut())
                        .and(x_2d.outer_iter())
                        .and(g_2d.outer_iter())
                        .par_for_each(|mut dx_row, x_row, g_row| {
                            let sum_sq = x_row.fold(0.0f32, |acc, &val| acc + val * val);
                            let inv_rms = 1.0 / (sum_sq / dim as f32 + eps).sqrt();
                            let inv_dim = 1.0 / dim as f32;

                            let mut dot = 0.0f32;
                            for (&gi, (&wi, &xi)) in
                                g_row.iter().zip(w_slice.iter().zip(x_row.iter()))
                            {
                                dot += (gi * wi) * (xi * inv_rms);
                            }

                            let mean_dot = dot * inv_dim;

                            for (dxi, (&gi, (&wi, &xi))) in dx_row
                                .iter_mut()
                                .zip(g_row.iter().zip(w_slice.iter().zip(x_row.iter())))
                            {
                                let term1 = gi * wi;
                                let x_norm = xi * inv_rms;
                                *dxi = inv_rms * (term1 - x_norm * mean_dot);
                            }
                        });

                    let dw_accum = (0..rows)
                        .into_par_iter()
                        .fold(
                            || Array1::<f32>::zeros(dim),
                            |mut acc, r| {
                                let x_row = x_2d.row(r);
                                let g_row = g_2d.row(r);

                                let sum_sq = x_row.fold(0.0f32, |a, &v| a + v * v);
                                let inv_rms = 1.0 / (sum_sq / dim as f32 + eps).sqrt();

                                for (a, (&gi, &xi)) in
                                    acc.iter_mut().zip(g_row.iter().zip(x_row.iter()))
                                {
                                    *a += gi * xi * inv_rms;
                                }
                                acc
                            },
                        )
                        .reduce(
                            || Array1::<f32>::zeros(dim),
                            |mut a, b| {
                                a += &b;
                                a
                            },
                        );

                    (
                        d_input_flat.into_shape(shape.clone()).unwrap().into_dyn(),
                        dw_accum.into_dyn(),
                    )
                };

                input_clone.add_grad(d_input);
                weight_clone.add_grad(d_weight);
            })),
            requires_grad: true,
            device: output_device,
        })))
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
    fn rms_norm_no_grad_preserves_bf16_input_dtype() {
        let norm = RMSNorm::new(4, 1e-5);
        norm.weight.set_array_f32_with_dtype(
            Array::from_shape_vec(IxDyn(&[4]), vec![1.0, 0.5, 1.5, 2.0])
                .expect("weight shape mismatch")
                .into_dyn(),
            DType::BF16,
        );

        let input_f32 = make_tensor(&[1, 1, 4], vec![1.0, -2.0, 3.0, -4.0], DType::F32);
        let input_bf16 = make_tensor(&[1, 1, 4], vec![1.0, -2.0, 3.0, -4.0], DType::BF16);

        let ref_out = no_grad(|| norm.forward(input_f32));
        let bf16_out = no_grad(|| norm.forward(input_bf16));

        assert_eq!(bf16_out.dtype(), DType::BF16);
        let ref_vals = ref_out
            .data_ref()
            .iter()
            .map(|&v| bf16::from_f32(v).to_f32())
            .collect::<Vec<_>>();
        bf16_out.with_storage_view(|view| match view {
            TensorStorageView::BF16(view) => {
                let vals = view.iter().map(|v| v.to_f32()).collect::<Vec<_>>();
                assert_eq!(vals, ref_vals);
            }
            TensorStorageView::F16(_) => panic!("bf16 RMSNorm output should stay bf16 in no-grad"),
            TensorStorageView::F32(_) => panic!("bf16 RMSNorm output should stay bf16 in no-grad"),
        });
    }

    #[test]
    fn rms_norm_explicit_dtype_overrides_global_default() {
        with_precision_config(
            PrecisionConfig {
                parameter_dtype: DType::BF16,
                runtime_dtype: DType::F32,
                allow_parameter_dtype_copies: false,
            },
            || {
                let norm = RMSNorm::new_with_dtype(4, 1e-5, DType::F32);
                assert_eq!(norm.weight.dtype(), DType::F32);
            },
        );
    }

    #[test]
    fn rms_norm_no_grad_preserves_i8_input_dtype() {
        let norm = RMSNorm::new(4, 1e-5);
        norm.weight.set_array_f32_with_dtype(
            Array::from_shape_vec(IxDyn(&[4]), vec![1.0, 0.5, 1.5, 2.0])
                .expect("weight shape mismatch")
                .into_dyn(),
            DType::BF16,
        );

        let input_i8 = make_tensor(&[1, 1, 4], vec![1.0, -2.0, 3.0, -4.0], DType::I8);
        let out = no_grad(|| norm.forward(input_i8));
        assert_eq!(out.dtype(), DType::I8);
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_rms_norm_matches_cpu_reference_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let weight_values = vec![1.0, 0.5, 1.5, 2.0];
        let norm = RMSNorm::new(4, 1e-5);
        norm.weight.set_array_f32_with_dtype(
            Array::from_shape_vec(IxDyn(&[4]), weight_values.clone())
                .expect("weight shape mismatch")
                .into_dyn(),
            DType::BF16,
        );
        norm.weight.to_cuda_inplace();
        let norm_ref = RMSNorm::new(4, 1e-5);
        norm_ref.weight.set_array_f32_with_dtype(
            Array::from_shape_vec(IxDyn(&[4]), weight_values)
                .expect("weight shape mismatch")
                .into_dyn(),
            DType::BF16,
        );

        let input = make_tensor(
            &[1, 2, 4],
            vec![1.0, -2.0, 3.0, -4.0, 0.5, 1.5, -2.5, 4.0],
            DType::BF16,
        )
        .to_cuda();

        crate::ops::cuda::set_enabled(true);
        crate::autograd::set_strict_device_execution(true);
        let cuda_out = no_grad(|| norm.forward(input.clone()));
        crate::autograd::set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);

        let cpu_out = no_grad(|| norm_ref.forward(input.to_cpu()));
        assert!(cuda_out.is_cuda());
        assert_eq!(cuda_out.dtype(), DType::BF16);

        for (got, expect) in cuda_out.data_ref().iter().zip(cpu_out.data_ref().iter()) {
            assert!((got - expect).abs() < 2e-2, "got {got}, expect {expect}");
        }
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_empty_rms_norm_stays_on_cuda_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let norm = RMSNorm::new(4, 1e-5);
        norm.weight.set_array_f32_with_dtype(
            Array::from_shape_vec(IxDyn(&[4]), vec![1.0, 0.5, 1.5, 2.0])
                .expect("weight shape mismatch")
                .into_dyn(),
            DType::BF16,
        );
        norm.weight.to_cuda_inplace();
        let input = make_tensor(&[0, 4], vec![], DType::BF16).to_cuda();

        crate::ops::cuda::set_enabled(true);
        crate::autograd::set_strict_device_execution(true);
        let out = no_grad(|| norm.forward(input.clone()));
        crate::autograd::set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);

        assert!(out.is_cuda());
        assert_eq!(out.dtype(), DType::BF16);
        assert_eq!(out.shape_vec(), vec![0, 4]);
        assert_eq!(out.len(), 0);
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_rms_norm_backward_matches_cpu_reference_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let weight_values = vec![1.0, 0.5, 1.5, 2.0];
        let input_values = vec![1.0, -2.0, 3.0, -4.0, 0.5, 1.5, -2.5, 4.0];
        let coeff_values = vec![0.5, -1.0, 2.0, 0.25, -0.75, 1.5, -0.5, 0.75];

        let norm_cuda = RMSNorm::new(4, 1e-5);
        norm_cuda.weight.set_array_f32_with_dtype(
            Array::from_shape_vec(IxDyn(&[4]), weight_values.clone())
                .expect("weight shape mismatch")
                .into_dyn(),
            DType::F32,
        );
        norm_cuda.weight.to_cuda_inplace();
        let input_cpu = Tensor::from_data_with_grad_flag(
            Array::from_shape_vec(IxDyn(&[1, 2, 4]), input_values.clone())
                .expect("input shape mismatch")
                .into_dyn(),
            true,
        );
        let coeff_cpu = Tensor::from_data_with_grad_flag(
            Array::from_shape_vec(IxDyn(&[1, 2, 4]), coeff_values)
                .expect("coeff shape mismatch")
                .into_dyn(),
            false,
        );
        let input_cuda = input_cpu.to_cuda();
        let coeff_cuda = coeff_cpu.to_cuda();

        crate::ops::cuda::set_enabled(true);
        crate::autograd::set_strict_device_execution(true);
        let cuda_out = norm_cuda.forward(input_cuda.clone());
        let cuda_loss = crate::ops::arithmetic::sum(&(&cuda_out * &coeff_cuda));
        cuda_loss.backward();
        assert!(input_cuda.cloned_cuda_f32_grad().is_some());
        assert!(norm_cuda.weight.cloned_cuda_f32_grad().is_some());
        assert!(!input_cuda.has_host_grad());
        assert!(!norm_cuda.weight.has_host_grad());
        crate::autograd::set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);

        let norm_cpu = RMSNorm::new(4, 1e-5);
        norm_cpu.weight.set_array_f32_with_dtype(
            Array::from_shape_vec(IxDyn(&[4]), weight_values)
                .expect("weight shape mismatch")
                .into_dyn(),
            DType::F32,
        );
        let cpu_out = norm_cpu.forward(input_cpu.clone());
        let cpu_loss = crate::ops::arithmetic::sum(&(&cpu_out * &coeff_cpu));
        cpu_loss.backward();

        let input_cuda_grad = input_cuda.grad().expect("cuda RMSNorm input grad");
        let input_cpu_grad = input_cpu.grad().expect("cpu RMSNorm input grad");
        let weight_cuda_grad = norm_cuda.weight.grad().expect("cuda RMSNorm weight grad");
        let weight_cpu_grad = norm_cpu.weight.grad().expect("cpu RMSNorm weight grad");
        for (got, expect) in input_cuda_grad.iter().zip(input_cpu_grad.iter()) {
            assert!(
                (got - expect).abs() < 1e-5,
                "input got {got}, expect {expect}"
            );
        }
        for (got, expect) in weight_cuda_grad.iter().zip(weight_cpu_grad.iter()) {
            assert!(
                (got - expect).abs() < 1e-5,
                "weight got {got}, expect {expect}"
            );
        }
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_rms_norm_preserves_i8_dtype_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let weight_values = vec![1.0, 0.5, 1.5, 2.0];
        let norm = RMSNorm::new(4, 1e-5);
        norm.weight.set_array_f32_with_dtype(
            Array::from_shape_vec(IxDyn(&[4]), weight_values.clone())
                .expect("weight shape mismatch")
                .into_dyn(),
            DType::BF16,
        );
        norm.weight.to_cuda_inplace();
        let norm_ref = RMSNorm::new(4, 1e-5);
        norm_ref.weight.set_array_f32_with_dtype(
            Array::from_shape_vec(IxDyn(&[4]), weight_values)
                .expect("weight shape mismatch")
                .into_dyn(),
            DType::BF16,
        );

        let input = make_tensor(
            &[1, 2, 4],
            vec![1.0, -2.0, 3.0, -4.0, 0.5, 1.5, -2.5, 4.0],
            DType::I8,
        )
        .to_cuda();

        crate::ops::cuda::set_enabled(true);
        crate::autograd::set_strict_device_execution(true);
        let cuda_out = no_grad(|| norm.forward(input.clone()));
        crate::autograd::set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);

        let cpu_out = no_grad(|| norm_ref.forward(input.to_cpu()));
        assert!(cuda_out.is_cuda());
        assert_eq!(cuda_out.dtype(), DType::I8);
        for (got, expect) in cuda_out.data_ref().iter().zip(cpu_out.data_ref().iter()) {
            assert!((got - expect).abs() < 1e-5, "got {got}, expect {expect}");
        }
    }
}
