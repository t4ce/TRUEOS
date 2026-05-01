use crate::autograd::{
    StoragePreference, Tensor, TensorData, TensorStorageView, assert_native_device_support,
    is_no_grad, is_strict_device_execution,
};
use crate::ops::cuda;
use crate::precision::DType;
use ndarray::{Array2, Zip, arr0};
use rayon::prelude::*;
use std::cell::RefCell;
use std::rc::Rc; // 引入并行迭代

// --- MSE Loss ---
pub struct MSELoss;
impl MSELoss {
    pub fn apply(output: &Tensor, target: &Tensor) -> Tensor {
        let output_device = crate::autograd::assert_same_device(output, target, "mse_loss");
        assert_eq!(
            output.shape_vec(),
            target.shape_vec(),
            "mse_loss expects output and target to have the same shape"
        );
        assert!(output.len() > 0, "mse_loss expects at least one element");
        let build_graph = !is_no_grad() && (output.requires_grad() || target.requires_grad());
        let cuda_native_supported = output_device == crate::autograd::Device::Cuda;
        assert_native_device_support(output_device, "mse_loss", cuda_native_supported);

        if output_device == crate::autograd::Device::Cuda {
            let cuda_forward = output.with_cuda_f32_buffer(|out_buf| {
                target.with_cuda_f32_buffer(|tar_buf| {
                    let diff_buf = cuda::binary_f32_buffer(out_buf, tar_buf, cuda::BinaryOp::Sub)?;
                    let sq_buf =
                        cuda::binary_f32_buffer(&diff_buf, &diff_buf, cuda::BinaryOp::Mul)?;
                    let sq_host = cuda::download_f32(&sq_buf)?;
                    Ok::<_, String>((diff_buf, sq_host))
                })
            });
            if let Ok((diff_buf, sq_host)) = cuda_forward {
                let n = sq_host.len() as f32;
                let loss_val = sq_host.iter().sum::<f32>() / n;
                if !build_graph {
                    return Tensor::from_f32_data_no_grad_with_device_dtype(
                        arr0(loss_val).into_dyn(),
                        DType::F32,
                        output_device,
                    );
                }

                let output_clone = output.clone();
                let target_clone = target.clone();
                return Tensor(Rc::new(RefCell::new(TensorData {
                    data: arr0(loss_val).into_dyn().into_shared(),
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
                    parents: vec![output.clone(), target.clone()],
                    backward_op: Some(std::rc::Rc::new(move |grad_output| {
                        let grad_val = *grad_output.first().unwrap();
                        let factor = 2.0 / diff_buf.len() as f32 * grad_val;
                        if is_strict_device_execution() {
                            let (grad_output_buffer, grad_target_buffer) =
                                cuda::mse_backward_f32_buffers(&diff_buf, factor).unwrap_or_else(
                                    |err| panic!("CUDA mse_loss backward failed: {}", err),
                                );
                            output_clone.add_cuda_grad_buffer_only(grad_output_buffer);
                            target_clone.add_cuda_grad_buffer_only(grad_target_buffer);
                            return;
                        }
                        let (
                            (grad_output_buffer, grad_output_host),
                            (grad_target_buffer, grad_target_host),
                        ) = cuda::mse_backward_f32(&diff_buf, factor)
                            .unwrap_or_else(|err| panic!("CUDA mse_loss backward failed: {}", err));
                        let grad_out = ndarray::Array::from_shape_vec(
                            output_clone.shape_vec(),
                            grad_output_host,
                        )
                        .expect("CUDA mse_loss output grad shape build failed")
                        .into_dyn();
                        let grad_target = ndarray::Array::from_shape_vec(
                            target_clone.shape_vec(),
                            grad_target_host,
                        )
                        .expect("CUDA mse_loss target grad shape build failed")
                        .into_dyn();
                        output_clone.add_grad_with_cuda_buffer(grad_out, Some(grad_output_buffer));
                        target_clone
                            .add_grad_with_cuda_buffer(grad_target, Some(grad_target_buffer));
                    })),
                    requires_grad: true,
                    device: output_device,
                })));
            }
        }

        let loss_val =
            output.with_storage_view_preferring(StoragePreference::F32Compute, |out_view| {
                target.with_storage_view_preferring(StoragePreference::F32Compute, |tar_view| {
                    let out_ref = match out_view {
                        TensorStorageView::F32(view) => view,
                        TensorStorageView::F16(_) => {
                            unreachable!("f32 compute preference should expose f32 view")
                        }
                        TensorStorageView::BF16(_) => {
                            unreachable!("f32 compute preference should expose f32 view")
                        }
                    };
                    let tar_ref = match tar_view {
                        TensorStorageView::F32(view) => view,
                        TensorStorageView::F16(_) => {
                            unreachable!("f32 compute preference should expose f32 view")
                        }
                        TensorStorageView::BF16(_) => {
                            unreachable!("f32 compute preference should expose f32 view")
                        }
                    };
                    let n = out_ref.len() as f32;
                    let sum_sq: f32 = Zip::from(&out_ref)
                        .and(&tar_ref)
                        .par_map_collect(|&o, &t| (o - t).powi(2))
                        .sum();
                    sum_sq / n
                })
            });

        if !build_graph {
            return Tensor::from_f32_data_no_grad_with_device_dtype(
                arr0(loss_val).into_dyn(),
                DType::F32,
                output_device,
            );
        }

        let output_clone = output.clone();
        let target_clone = target.clone();

        Tensor(Rc::new(RefCell::new(TensorData {
            data: arr0(loss_val).into_dyn().into_shared(),
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
            parents: vec![output.clone(), target.clone()],
            backward_op: Some(std::rc::Rc::new(move |grad_output| {
                let grad_val = grad_output.first().unwrap();
                let (grad_out, grad_target) = {
                    let out_d = output_clone.data_ref();
                    let tar_d = target_clone.data_ref();
                    let n = out_d.len() as f32;
                    let factor = 2.0 / n * grad_val;

                    let grad = Zip::from(&*out_d)
                        .and(&*tar_d)
                        .par_map_collect(|&o, &t| (o - t) * factor);
                    (grad.clone(), grad.mapv(|x| -x))
                };

                output_clone.add_grad(grad_out);
                target_clone.add_grad(grad_target);
            })),
            requires_grad: true,
            device: output_device,
        })))
    }
}

// --- Cross Entropy Loss ---
// 针对 Batch 进行行级并行优化
pub struct CrossEntropyLoss;

impl CrossEntropyLoss {
    pub fn apply(input_logits: &Tensor, target_onehot: &Tensor) -> Tensor {
        let output_device =
            crate::autograd::assert_same_device(input_logits, target_onehot, "cross_entropy");
        let build_graph =
            !is_no_grad() && (input_logits.requires_grad() || target_onehot.requires_grad());
        let cuda_native_supported = output_device == crate::autograd::Device::Cuda;
        assert_native_device_support(output_device, "cross_entropy", cuda_native_supported);
        let shape = input_logits.shape_vec();
        assert_eq!(
            shape.len(),
            2,
            "cross_entropy currently expects [B, C] logits"
        );
        assert_eq!(
            target_onehot.shape_vec(),
            shape,
            "cross_entropy expects logits and target to have the same shape"
        );
        let batch_size = shape[0];
        let dim = shape[1];
        assert!(
            batch_size > 0,
            "cross_entropy batch size must be greater than zero"
        );
        assert!(
            dim > 0,
            "cross_entropy class dimension must be greater than zero"
        );
        if output_device == crate::autograd::Device::Cuda {
            let cuda_forward = input_logits.with_cuda_f32_buffer(|logits_buf| {
                let softmax_buf = cuda::softmax_lastdim_f32_no_host(logits_buf, batch_size, dim)?;
                let (loss_buf, loss_host) = target_onehot.with_cuda_f32_buffer(|target_buf| {
                    cuda::cross_entropy_loss_f32(&softmax_buf, target_buf, batch_size)
                })?;
                let loss_val = loss_host.first().copied().ok_or_else(|| {
                    "CUDA cross_entropy loss returned empty host scalar".to_string()
                })?;
                Ok::<_, String>((softmax_buf, loss_buf, loss_val))
            });
            if let Ok((softmax_buf, loss_buf, loss_val)) = cuda_forward {
                if !build_graph {
                    return Tensor::from_f32_data_no_grad_with_device_dtype_and_cuda_buffer(
                        arr0(loss_val).into_dyn(),
                        DType::F32,
                        output_device,
                        Some(loss_buf),
                    );
                }

                let input_clone = input_logits.clone();
                let target_clone = target_onehot.clone();
                let softmax_buffer = softmax_buf.clone();

                return Tensor(Rc::new(RefCell::new(TensorData {
                    data: arr0(loss_val).into_dyn().into_shared(),
                    f16_data: None,
                    bf16_data: None,
                    i8_data: None,
                    cuda_f32_data: Some(loss_buf),
                    i8_scale: None,
                    has_f32_data: true,
                    storage_dtype: crate::precision::DType::F32,
                    cache_dirty: false,
                    is_parameter: false,
                    grad: None,
                    cuda_f32_grad: None,
                    parents: vec![input_logits.clone(), target_onehot.clone()],
                    backward_op: Some(std::rc::Rc::new(move |grad_output| {
                        let grad_val = *grad_output.first().unwrap();
                        let factor = grad_val / batch_size as f32;
                        if is_strict_device_execution() {
                            let grad_buffer = target_clone
                                .with_cuda_f32_buffer(|target_buf| {
                                    cuda::cross_entropy_backward_f32_buffer(
                                        &softmax_buffer,
                                        target_buf,
                                        factor,
                                    )
                                })
                                .unwrap_or_else(|err| {
                                    panic!("CUDA cross_entropy backward failed: {}", err)
                                });
                            input_clone.add_cuda_grad_buffer_only(grad_buffer);
                            return;
                        }
                        let (grad_buffer, grad_host) = target_clone
                            .with_cuda_f32_buffer(|target_buf| {
                                cuda::cross_entropy_backward_f32(
                                    &softmax_buffer,
                                    target_buf,
                                    factor,
                                )
                            })
                            .unwrap_or_else(|err| {
                                panic!("CUDA cross_entropy backward failed: {}", err)
                            });
                        let grad =
                            ndarray::Array::from_shape_vec(input_clone.shape_vec(), grad_host)
                                .expect("CUDA cross_entropy grad shape build failed")
                                .into_dyn();
                        input_clone.add_grad_with_cuda_buffer(grad, Some(grad_buffer));
                    })),
                    requires_grad: true,
                    device: output_device,
                })));
            }
        }
        // Forward
        let (loss_val, softmax_output) = input_logits.with_storage_view_preferring(
            StoragePreference::F32Compute,
            |logits_view| {
                target_onehot.with_storage_view_preferring(
                    StoragePreference::F32Compute,
                    |targets_view| {
                        let logits_ref = match logits_view {
                            TensorStorageView::F32(view) => view,
                            TensorStorageView::F16(_) => {
                                unreachable!("f32 compute preference should expose f32 view")
                            }
                            TensorStorageView::BF16(_) => {
                                unreachable!("f32 compute preference should expose f32 view")
                            }
                        };
                        let targets_ref = match targets_view {
                            TensorStorageView::F32(view) => view,
                            TensorStorageView::F16(_) => {
                                unreachable!("f32 compute preference should expose f32 view")
                            }
                            TensorStorageView::BF16(_) => {
                                unreachable!("f32 compute preference should expose f32 view")
                            }
                        };

                        let batch_size = logits_ref.shape()[0];
                        let dim = logits_ref.shape()[1];

                        let logits_2d = logits_ref.view().into_shape((batch_size, dim)).unwrap();
                        let targets_2d = targets_ref.view().into_shape((batch_size, dim)).unwrap();

                        let mut softmax_out_flat = Array2::<f32>::zeros((batch_size, dim));
                        let total_loss: f32 = Zip::from(softmax_out_flat.outer_iter_mut())
                            .and(logits_2d.outer_iter())
                            .and(targets_2d.outer_iter())
                            .into_par_iter()
                            .map(|(mut sm_row, l_row, t_row)| {
                                let max_val = l_row.fold(f32::NEG_INFINITY, |a, &b| a.max(b));
                                let mut sum_exp = 0.0f32;

                                for (s_val, &l_val) in sm_row.iter_mut().zip(l_row.iter()) {
                                    let e = (l_val - max_val).exp();
                                    *s_val = e;
                                    sum_exp += e;
                                }

                                let inv_sum = 1.0 / sum_exp;
                                let epsilon = 1e-9;
                                let mut row_loss = 0.0;

                                for (s_val, &t_val) in sm_row.iter_mut().zip(t_row.iter()) {
                                    *s_val *= inv_sum;
                                    if t_val > 0.0 {
                                        row_loss -= t_val * (*s_val + epsilon).ln();
                                    }
                                }
                                row_loss
                            })
                            .sum();

                        (total_loss / batch_size as f32, softmax_out_flat.into_dyn())
                    },
                )
            },
        );

        if !build_graph {
            return Tensor::from_f32_data_no_grad_with_device_dtype(
                arr0(loss_val).into_dyn(),
                DType::F32,
                output_device,
            );
        }

        let input_clone = input_logits.clone();
        let target_clone = target_onehot.clone();
        let softmax_cache = softmax_output;

        Tensor(Rc::new(RefCell::new(TensorData {
            data: arr0(loss_val).into_dyn().into_shared(),
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
            parents: vec![input_logits.clone(), target_onehot.clone()],
            backward_op: Some(std::rc::Rc::new(move |grad_output| {
                let grad_val = grad_output.first().unwrap();
                let grad = {
                    let targets_ref = target_clone.data_ref();
                    let batch_size = targets_ref.shape()[0] as f32;
                    let factor = grad_val / batch_size;

                    Zip::from(&softmax_cache)
                        .and(&*targets_ref)
                        .par_map_collect(|&p, &t| (p - t) * factor)
                };

                input_clone.add_grad(grad);
            })),
            requires_grad: true,
            device: output_device,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autograd::no_grad;
    #[cfg(feature = "cuda")]
    use crate::autograd::set_strict_device_execution;
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
    fn mse_loss_no_grad_returns_f32_scalar_without_materializing_bf16_input() {
        let output = make_tensor(&[2], vec![1.0, 3.0], DType::BF16);
        let target = make_tensor(&[2], vec![2.0, 1.0], DType::BF16);

        let loss = no_grad(|| MSELoss::apply(&output, &target));

        assert_eq!(loss.dtype(), DType::F32);
        assert!(!loss.requires_grad());
        assert_eq!(output.dtype(), DType::BF16);
        assert_eq!(target.dtype(), DType::BF16);
        assert!((loss.data_ref().first().copied().unwrap_or_default() - 2.5).abs() <= 1e-6);
    }

    #[test]
    fn cross_entropy_no_grad_accepts_i8_logits_and_returns_f32_scalar() {
        let logits = make_tensor(&[2, 3], vec![2.0, 0.0, -1.0, -1.0, 0.0, 2.0], DType::I8);
        let target = make_tensor(&[2, 3], vec![1.0, 0.0, 0.0, 0.0, 0.0, 1.0], DType::F32);

        let loss = no_grad(|| CrossEntropyLoss::apply(&logits, &target));

        assert_eq!(loss.dtype(), DType::F32);
        assert!(!loss.requires_grad());
        assert_eq!(logits.dtype(), DType::I8);
        assert!(
            loss.data_ref()
                .first()
                .copied()
                .unwrap_or_default()
                .is_finite()
        );
    }

    #[test]
    #[should_panic(expected = "mse_loss expects at least one element")]
    fn mse_loss_rejects_empty_input() {
        let output = make_tensor(&[0, 4], vec![], DType::F32);
        let target = make_tensor(&[0, 4], vec![], DType::F32);
        let _ = no_grad(|| MSELoss::apply(&output, &target));
    }

    #[test]
    #[should_panic(expected = "batch size must be greater than zero")]
    fn cross_entropy_rejects_empty_batch() {
        let logits = make_tensor(&[0, 4], vec![], DType::F32);
        let target = make_tensor(&[0, 4], vec![], DType::F32);
        let _ = no_grad(|| CrossEntropyLoss::apply(&logits, &target));
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_mse_loss_backward_matches_cpu_reference_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let output_cpu = Tensor::from_data_with_grad_flag(
            Array::from_shape_vec(IxDyn(&[2, 3]), vec![1.0, -2.0, 0.5, 3.0, -1.0, 2.0])
                .unwrap()
                .into_dyn(),
            true,
        );
        let target_cpu = Tensor::from_data_with_grad_flag(
            Array::from_shape_vec(IxDyn(&[2, 3]), vec![0.5, -1.5, 0.25, 2.5, -0.5, 1.0])
                .unwrap()
                .into_dyn(),
            true,
        );
        let output_cuda = output_cpu.to_cuda();
        let target_cuda = target_cpu.to_cuda();

        crate::ops::cuda::set_enabled(true);
        set_strict_device_execution(true);
        let loss_cuda = MSELoss::apply(&output_cuda, &target_cuda);
        loss_cuda.backward();
        assert!(output_cuda.cloned_cuda_f32_grad().is_some());
        assert!(target_cuda.cloned_cuda_f32_grad().is_some());
        assert!(!output_cuda.has_host_grad());
        assert!(!target_cuda.has_host_grad());
        set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);

        let loss_cpu = MSELoss::apply(&output_cpu, &target_cpu);
        loss_cpu.backward();

        let out_cuda_grad = output_cuda.grad().expect("cuda output grad");
        let out_cpu_grad = output_cpu.grad().expect("cpu output grad");
        let tar_cuda_grad = target_cuda.grad().expect("cuda target grad");
        let tar_cpu_grad = target_cpu.grad().expect("cpu target grad");
        for (got, expect) in out_cuda_grad.iter().zip(out_cpu_grad.iter()) {
            assert!(
                (got - expect).abs() < 1e-5,
                "output grad got {got}, expect {expect}"
            );
        }
        for (got, expect) in tar_cuda_grad.iter().zip(tar_cpu_grad.iter()) {
            assert!(
                (got - expect).abs() < 1e-5,
                "target grad got {got}, expect {expect}"
            );
        }
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_cross_entropy_backward_matches_cpu_reference_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let logits_cpu = Tensor::from_data_with_grad_flag(
            Array::from_shape_vec(IxDyn(&[2, 3]), vec![2.0, 0.0, -1.0, -1.0, 0.0, 2.0])
                .unwrap()
                .into_dyn(),
            true,
        );
        let targets_cpu = Tensor::from_data_with_grad_flag(
            Array::from_shape_vec(IxDyn(&[2, 3]), vec![1.0, 0.0, 0.0, 0.0, 0.0, 1.0])
                .unwrap()
                .into_dyn(),
            true,
        );
        let logits_cuda = logits_cpu.to_cuda();
        let targets_cuda = targets_cpu.to_cuda();

        crate::ops::cuda::set_enabled(true);
        set_strict_device_execution(true);
        let loss_cuda = CrossEntropyLoss::apply(&logits_cuda, &targets_cuda);
        loss_cuda.backward();
        assert!(logits_cuda.cloned_cuda_f32_grad().is_some());
        assert!(!logits_cuda.has_host_grad());
        set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);

        let loss_cpu = CrossEntropyLoss::apply(&logits_cpu, &targets_cpu);
        loss_cpu.backward();

        let logits_cuda_grad = logits_cuda.grad().expect("cuda logits grad");
        let logits_cpu_grad = logits_cpu.grad().expect("cpu logits grad");
        for (got, expect) in logits_cuda_grad.iter().zip(logits_cpu_grad.iter()) {
            assert!(
                (got - expect).abs() < 1e-5,
                "logits grad got {got}, expect {expect}"
            );
        }
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_cross_entropy_forward_keeps_resident_scalar_loss_buffer() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let logits_cpu = Tensor::from_data_with_grad_flag(
            Array::from_shape_vec(IxDyn(&[2, 3]), vec![2.0, 0.0, -1.0, -1.0, 0.0, 2.0])
                .unwrap()
                .into_dyn(),
            false,
        );
        let targets_cpu = Tensor::from_data_with_grad_flag(
            Array::from_shape_vec(IxDyn(&[2, 3]), vec![1.0, 0.0, 0.0, 0.0, 0.0, 1.0])
                .unwrap()
                .into_dyn(),
            false,
        );
        let logits_cuda = logits_cpu.to_cuda();
        let targets_cuda = targets_cpu.to_cuda();

        crate::ops::cuda::set_enabled(true);
        set_strict_device_execution(true);
        let loss_cuda = no_grad(|| CrossEntropyLoss::apply(&logits_cuda, &targets_cuda));
        let loss_buffer = loss_cuda
            .cloned_cuda_f32_buffer()
            .expect("CUDA cross_entropy loss should keep a resident scalar buffer");
        assert_eq!(loss_buffer.len(), 1);
        set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);

        let loss_cpu = CrossEntropyLoss::apply(&logits_cpu, &targets_cpu);
        let got = loss_cuda.data_ref().first().copied().unwrap_or_default();
        let expect = loss_cpu.data_ref().first().copied().unwrap_or_default();
        assert!(
            (got - expect).abs() < 1e-5,
            "loss got {got}, expect {expect}"
        );
    }
}
