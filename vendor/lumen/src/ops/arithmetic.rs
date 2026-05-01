// src/ops/arithmetic.rs
use crate::autograd::{
    Device, StoragePreference, Tensor, TensorData, TensorStorageView, assert_native_device_support,
    assert_same_device, is_no_grad, is_strict_device_execution,
};
use crate::ops::cuda;
use crate::precision::DType;
use ndarray::{ArrayD, ArrayViewD, IxDyn, Zip};
use std::cell::RefCell;
use std::ops::{Add, Mul, Sub};
use std::rc::Rc;

#[derive(Clone, Copy, Debug)]
enum BinaryOp {
    Add,
    Sub,
    Mul,
}

fn apply_binary_views(
    lhs: ArrayViewD<'_, f32>,
    rhs: ArrayViewD<'_, f32>,
    op: BinaryOp,
) -> ArrayD<f32> {
    match op {
        BinaryOp::Add => (&lhs + &rhs).into_dyn(),
        BinaryOp::Sub => (&lhs - &rhs).into_dyn(),
        BinaryOp::Mul => (&lhs * &rhs).into_dyn(),
    }
}

fn binary_no_grad(lhs: &Tensor, rhs: &Tensor, op: BinaryOp) -> Tensor {
    let output_device = assert_same_device(lhs, rhs, "binary op");
    let output_dtype = if lhs.dtype() == rhs.dtype() {
        lhs.dtype()
    } else {
        DType::F32
    };

    lhs.with_storage_view_preferring(StoragePreference::F32Compute, |lhs_view| {
        rhs.with_storage_view_preferring(StoragePreference::F32Compute, |rhs_view| {
            let lhs_f32 = match lhs_view {
                TensorStorageView::F32(view) => view,
                TensorStorageView::F16(_) => {
                    unreachable!("f32 compute preference should expose f32 view")
                }
                TensorStorageView::BF16(_) => {
                    unreachable!("f32 compute preference should expose f32 view")
                }
            };
            let rhs_f32 = match rhs_view {
                TensorStorageView::F32(view) => view,
                TensorStorageView::F16(_) => {
                    unreachable!("f32 compute preference should expose f32 view")
                }
                TensorStorageView::BF16(_) => {
                    unreachable!("f32 compute preference should expose f32 view")
                }
            };

            let out_shape = broadcast_shape(lhs_f32.shape(), rhs_f32.shape());
            let out_len = out_shape
                .as_ref()
                .map(|shape| shape.iter().product::<usize>())
                .unwrap_or(0);
            if output_device == crate::autograd::Device::Cuda
                && out_shape.is_some()
                && out_len > 0
                && (cuda::should_accelerate_elementwise(
                    out_len,
                ) || is_strict_device_execution())
            {
                let out_shape = out_shape.expect("checked");
                let cuda_op = cuda_binary_op(op);
                if output_dtype == DType::F32 {
                    let cuda_out = if lhs_f32.shape() == rhs_f32.shape() {
                        lhs.with_cuda_f32_buffer(|lhs_buf| {
                            rhs.with_cuda_f32_buffer(|rhs_buf| {
                                cuda::binary_f32_buffer(lhs_buf, rhs_buf, cuda_op)
                            })
                        })
                    } else {
                        lhs.with_cuda_f32_buffer(|lhs_buf| {
                            rhs.with_cuda_f32_buffer(|rhs_buf| {
                                cuda::binary_broadcast_f32_buffer(
                                    lhs_buf,
                                    rhs_buf,
                                    lhs_f32.shape(),
                                    rhs_f32.shape(),
                                    &out_shape,
                                    cuda_op,
                                )
                            })
                        })
                    };
                    match cuda_out {
                        Ok(buffer) => {
                            return Tensor::from_cuda_f32_buffer_no_host(
                                &out_shape,
                                buffer,
                                output_device,
                            );
                        }
                        Err(err) => {
                            assert!(
                                !is_strict_device_execution(),
                                "binary op CUDA forward failed while strict device execution is enabled: {err}"
                            );
                        }
                    }
                } else {
                    let cuda_out = if lhs_f32.shape() == rhs_f32.shape() {
                        lhs.with_cuda_f32_buffer(|lhs_buf| {
                            rhs.with_cuda_f32_buffer(|rhs_buf| {
                                cuda::binary_f32(lhs_buf, rhs_buf, cuda_op)
                            })
                        })
                    } else {
                        lhs.with_cuda_f32_buffer(|lhs_buf| {
                            rhs.with_cuda_f32_buffer(|rhs_buf| {
                                cuda::binary_broadcast_f32(
                                    lhs_buf,
                                    rhs_buf,
                                    lhs_f32.shape(),
                                    rhs_f32.shape(),
                                    &out_shape,
                                    cuda_op,
                                )
                            })
                        })
                    };
                    match cuda_out {
                        Ok((buffer, out)) => {
                            let out = ndarray::Array::from_shape_vec(IxDyn(&out_shape), out)
                                .expect("CUDA binary op output shape build failed")
                                .into_dyn();
                            return Tensor::from_f32_data_no_grad_with_device_dtype_and_cuda_buffer(
                                out,
                                output_dtype,
                                output_device,
                                Some(buffer),
                            );
                        }
                        Err(err) => {
                            assert!(
                                !is_strict_device_execution(),
                                "binary op CUDA forward failed while strict device execution is enabled: {err}"
                            );
                        }
                    }
                }
            }

            Tensor::from_f32_data_no_grad_with_device_dtype(
                apply_binary_views(lhs_f32, rhs_f32, op),
                output_dtype,
                output_device,
            )
        })
    })
}

fn reduce_gradient(grad: ArrayViewD<'_, f32>, target_shape: &[usize]) -> ArrayD<f32> {
    if grad.shape() == target_shape {
        return grad.to_owned().into_dyn();
    }

    let mut res = grad.to_owned().into_dyn();
    let g_ndim = res.ndim();
    let t_ndim = target_shape.len();

    if g_ndim > t_ndim {
        for _ in 0..(g_ndim - t_ndim) {
            res = res.sum_axis(ndarray::Axis(0));
        }
    }

    for i in 0..res.ndim() {
        if target_shape[i] == 1 && res.shape()[i] > 1 {
            let summed = res.sum_axis(ndarray::Axis(i));
            res = summed.insert_axis(ndarray::Axis(i));
        } else if target_shape[i] != res.shape()[i] {
            panic!(
                "Gradient shape mismatch. Grad: {:?}, Target: {:?}",
                grad.shape(),
                target_shape
            );
        }
    }

    if res.shape() != target_shape {
        if res.len() == target_shape.iter().product::<usize>() {
            return res.into_shape(target_shape).unwrap();
        }
        panic!("Reduction failed.");
    }

    res
}

fn cuda_binary_op(op: BinaryOp) -> cuda::BinaryOp {
    match op {
        BinaryOp::Add => cuda::BinaryOp::Add,
        BinaryOp::Sub => cuda::BinaryOp::Sub,
        BinaryOp::Mul => cuda::BinaryOp::Mul,
    }
}

fn broadcast_shape(lhs_shape: &[usize], rhs_shape: &[usize]) -> Option<Vec<usize>> {
    let ndim = lhs_shape.len().max(rhs_shape.len());
    let mut out = vec![1usize; ndim];
    for i in 0..ndim {
        let lhs_idx = lhs_shape.len() as isize - 1 - i as isize;
        let rhs_idx = rhs_shape.len() as isize - 1 - i as isize;
        let lhs_dim = if lhs_idx >= 0 {
            lhs_shape[lhs_idx as usize]
        } else {
            1
        };
        let rhs_dim = if rhs_idx >= 0 {
            rhs_shape[rhs_idx as usize]
        } else {
            1
        };
        if lhs_dim != rhs_dim && lhs_dim != 1 && rhs_dim != 1 {
            return None;
        }
        out[ndim - 1 - i] = lhs_dim.max(rhs_dim);
    }
    Some(out)
}

fn add_cpu_binary_grads(lhs: &Tensor, rhs: &Tensor, grad: ArrayViewD<'_, f32>, op: BinaryOp) {
    let l_shape = lhs.shape_vec();
    let r_shape = rhs.shape_vec();
    match op {
        BinaryOp::Add => {
            lhs.add_grad(reduce_gradient(grad.view(), &l_shape));
            rhs.add_grad(reduce_gradient(grad.view(), &r_shape));
        }
        BinaryOp::Sub => {
            lhs.add_grad(reduce_gradient(grad.view(), &l_shape));
            let grad_neg = Zip::from(&grad).par_map_collect(|&x| -x);
            rhs.add_grad(reduce_gradient(grad_neg.view(), &r_shape));
        }
        BinaryOp::Mul => {
            let (g_lhs, g_rhs, lhs_shape, rhs_shape) = {
                let lhs_data = lhs.data_ref();
                let rhs_data = rhs.data_ref();

                let (g_lhs, g_rhs) =
                    if grad.shape() == lhs_data.shape() && grad.shape() == rhs_data.shape() {
                        let gl = Zip::from(&grad)
                            .and(&*rhs_data)
                            .par_map_collect(|&g, &b| g * b);
                        let gr = Zip::from(&grad)
                            .and(&*lhs_data)
                            .par_map_collect(|&g, &a| g * a);
                        (gl, gr)
                    } else {
                        (grad.to_owned() * &*rhs_data, grad.to_owned() * &*lhs_data)
                    };

                (
                    g_lhs,
                    g_rhs,
                    lhs_data.shape().to_vec(),
                    rhs_data.shape().to_vec(),
                )
            };
            lhs.add_grad(reduce_gradient(g_lhs.view(), &lhs_shape));
            rhs.add_grad(reduce_gradient(g_rhs.view(), &rhs_shape));
        }
    }
}

fn try_cuda_binary_training(
    lhs: &Tensor,
    rhs: &Tensor,
    op: BinaryOp,
    op_name: &'static str,
) -> Option<Tensor> {
    if lhs.device() != Device::Cuda {
        return None;
    }

    let lhs_shape = lhs.shape_vec();
    let rhs_shape = rhs.shape_vec();
    let out_shape = broadcast_shape(&lhs_shape, &rhs_shape)?;
    if out_shape.iter().product::<usize>() == 0 {
        return None;
    }
    let cuda_op = cuda_binary_op(op);
    let cuda_out = if lhs_shape == rhs_shape {
        lhs.with_cuda_f32_buffer(|lhs_buf| {
            rhs.with_cuda_f32_buffer(|rhs_buf| cuda::binary_f32_buffer(lhs_buf, rhs_buf, cuda_op))
        })
    } else {
        lhs.with_cuda_f32_buffer(|lhs_buf| {
            rhs.with_cuda_f32_buffer(|rhs_buf| {
                cuda::binary_broadcast_f32_buffer(
                    lhs_buf, rhs_buf, &lhs_shape, &rhs_shape, &out_shape, cuda_op,
                )
            })
        })
    };

    match cuda_out {
        Ok(buffer) => {
            let lhs_clone = lhs.clone();
            let rhs_clone = rhs.clone();
            let lhs_shape_for_backward = lhs_shape.clone();
            let rhs_shape_for_backward = rhs_shape.clone();
            let out_shape_for_backward = out_shape.clone();
            let output_self = Rc::new(RefCell::new(None::<Tensor>));
            let output_self_for_backward = output_self.clone();

            let tensor = Tensor(Rc::new(RefCell::new(TensorData {
                data: ndarray::ArrayD::<f32>::zeros(IxDyn(&out_shape)).into_shared(),
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
                parents: vec![lhs.clone(), rhs.clone()],
                backward_op: Some(std::rc::Rc::new(move |grad| {
                    let upstream_cuda_grad = output_self_for_backward
                        .borrow()
                        .as_ref()
                        .and_then(|output| output.cloned_cuda_f32_grad())
                        .filter(|grad_buf| grad_buf.len() == grad.len());
                    if is_strict_device_execution() {
                        let grad_buf = match upstream_cuda_grad.clone() {
                            Some(buffer) => Ok(buffer),
                            None => {
                                let grad_host = grad.iter().copied().collect::<Vec<_>>();
                                cuda::upload_f32(&grad_host)
                            }
                        };
                        let cuda_grad_buffers = grad_buf.and_then(|grad_buf| {
                            lhs_clone.with_cuda_f32_buffer(|lhs_buf| {
                                rhs_clone.with_cuda_f32_buffer(|rhs_buf| {
                                    if lhs_shape_for_backward == rhs_shape_for_backward {
                                        cuda::binary_backward_f32_buffers(
                                            lhs_buf, rhs_buf, &grad_buf, cuda_op,
                                        )
                                    } else {
                                        cuda::binary_broadcast_backward_f32_buffers(
                                            lhs_buf,
                                            rhs_buf,
                                            &grad_buf,
                                            &lhs_shape_for_backward,
                                            &rhs_shape_for_backward,
                                            &out_shape_for_backward,
                                            cuda_op,
                                        )
                                    }
                                })
                            })
                        });

                        match cuda_grad_buffers {
                            Ok((lhs_buffer, rhs_buffer)) => {
                                lhs_clone.add_cuda_grad_buffer_only(lhs_buffer);
                                rhs_clone.add_cuda_grad_buffer_only(rhs_buffer);
                                return;
                            }
                            Err(err) => {
                                panic!(
                                    "{op_name} CUDA backward failed while strict device execution is enabled: {err}"
                                );
                            }
                        }
                    }
                    let cuda_grad = if let Some(grad_buf) = upstream_cuda_grad {
                        lhs_clone.with_cuda_f32_buffer(|lhs_buf| {
                            rhs_clone.with_cuda_f32_buffer(|rhs_buf| {
                                if lhs_shape_for_backward == rhs_shape_for_backward {
                                    cuda::binary_backward_f32(lhs_buf, rhs_buf, &grad_buf, cuda_op)
                                } else {
                                    cuda::binary_broadcast_backward_f32(
                                        lhs_buf,
                                        rhs_buf,
                                        &grad_buf,
                                        &lhs_shape_for_backward,
                                        &rhs_shape_for_backward,
                                        &out_shape_for_backward,
                                        cuda_op,
                                    )
                                }
                            })
                        })
                    } else {
                        let grad_host = grad.iter().copied().collect::<Vec<_>>();
                        cuda::upload_f32(&grad_host).and_then(|grad_buf| {
                            lhs_clone.with_cuda_f32_buffer(|lhs_buf| {
                                rhs_clone.with_cuda_f32_buffer(|rhs_buf| {
                                    if lhs_shape_for_backward == rhs_shape_for_backward {
                                        cuda::binary_backward_f32(
                                            lhs_buf, rhs_buf, &grad_buf, cuda_op,
                                        )
                                    } else {
                                        cuda::binary_broadcast_backward_f32(
                                            lhs_buf,
                                            rhs_buf,
                                            &grad_buf,
                                            &lhs_shape_for_backward,
                                            &rhs_shape_for_backward,
                                            &out_shape_for_backward,
                                            cuda_op,
                                        )
                                    }
                                })
                            })
                        })
                    };

                    match cuda_grad {
                        Ok(((lhs_buffer, lhs_host), (rhs_buffer, rhs_host))) => {
                            let grad_lhs = ndarray::Array::from_shape_vec(
                                IxDyn(&lhs_shape_for_backward),
                                lhs_host,
                            )
                            .expect("CUDA binary lhs grad shape build failed")
                            .into_dyn();
                            let grad_rhs = ndarray::Array::from_shape_vec(
                                IxDyn(&rhs_shape_for_backward),
                                rhs_host,
                            )
                            .expect("CUDA binary rhs grad shape build failed")
                            .into_dyn();
                            lhs_clone.add_grad_with_cuda_buffer(grad_lhs, Some(lhs_buffer));
                            rhs_clone.add_grad_with_cuda_buffer(grad_rhs, Some(rhs_buffer));
                        }
                        Err(err) => {
                            assert!(
                                !is_strict_device_execution(),
                                "{op_name} CUDA backward failed while strict device execution is enabled: {err}"
                            );
                            add_cpu_binary_grads(&lhs_clone, &rhs_clone, grad.view(), op);
                        }
                    }
                })),
                requires_grad: true,
                device: Device::Cuda,
            })));
            *output_self.borrow_mut() = Some(tensor.clone());
            Some(tensor)
        }
        Err(err) => {
            assert!(
                !is_strict_device_execution(),
                "{op_name} CUDA forward failed while strict device execution is enabled: {err}"
            );
            None
        }
    }
}

impl Add for Tensor {
    type Output = Tensor;
    fn add(self, rhs: Tensor) -> Tensor {
        let output_device = assert_same_device(&self, &rhs, "add");
        let build_graph = !is_no_grad() && (self.requires_grad() || rhs.requires_grad());
        let lhs_shape = self.shape_vec();
        let rhs_shape = rhs.shape_vec();
        let cuda_native_supported =
            output_device == Device::Cuda && broadcast_shape(&lhs_shape, &rhs_shape).is_some();
        assert_native_device_support(output_device, "add", cuda_native_supported);

        if !build_graph {
            return binary_no_grad(&self, &rhs, BinaryOp::Add);
        }

        if output_device == Device::Cuda {
            if let Some(output) = try_cuda_binary_training(&self, &rhs, BinaryOp::Add, "add") {
                return output;
            }
        }

        let data = (&*self.data_ref() + &*rhs.data_ref()).into_dyn();

        let lhs = self.clone();
        let rhs = rhs.clone();

        Tensor(Rc::new(RefCell::new(TensorData {
            data: data.into_shared(),
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
            parents: vec![self.clone(), rhs.clone()],
            backward_op: Some(std::rc::Rc::new(move |grad| {
                add_cpu_binary_grads(&lhs, &rhs, grad.view(), BinaryOp::Add);
            })),
            requires_grad: true,
            device: output_device,
        })))
    }
}
impl<'a, 'b> Add<&'b Tensor> for &'a Tensor {
    type Output = Tensor;
    fn add(self, rhs: &'b Tensor) -> Tensor {
        self.clone() + rhs.clone()
    }
}

impl Sub for Tensor {
    type Output = Tensor;
    fn sub(self, rhs: Tensor) -> Tensor {
        let output_device = assert_same_device(&self, &rhs, "sub");
        let build_graph = !is_no_grad() && (self.requires_grad() || rhs.requires_grad());
        let lhs_shape = self.shape_vec();
        let rhs_shape = rhs.shape_vec();
        let cuda_native_supported =
            output_device == Device::Cuda && broadcast_shape(&lhs_shape, &rhs_shape).is_some();
        assert_native_device_support(output_device, "sub", cuda_native_supported);

        if !build_graph {
            return binary_no_grad(&self, &rhs, BinaryOp::Sub);
        }

        if output_device == Device::Cuda {
            if let Some(output) = try_cuda_binary_training(&self, &rhs, BinaryOp::Sub, "sub") {
                return output;
            }
        }

        let data = (&*self.data_ref() - &*rhs.data_ref()).into_dyn();

        let lhs = self.clone();
        let rhs = rhs.clone();

        Tensor(Rc::new(RefCell::new(TensorData {
            data: data.into_shared(),
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
            parents: vec![self.clone(), rhs.clone()],
            backward_op: Some(std::rc::Rc::new(move |grad| {
                add_cpu_binary_grads(&lhs, &rhs, grad.view(), BinaryOp::Sub);
            })),
            requires_grad: true,
            device: output_device,
        })))
    }
}
impl<'a, 'b> Sub<&'b Tensor> for &'a Tensor {
    type Output = Tensor;
    fn sub(self, rhs: &'b Tensor) -> Tensor {
        self.clone() - rhs.clone()
    }
}

impl Mul for Tensor {
    type Output = Tensor;
    fn mul(self, rhs: Tensor) -> Tensor {
        let output_device = assert_same_device(&self, &rhs, "mul");
        let build_graph = !is_no_grad() && (self.requires_grad() || rhs.requires_grad());
        let lhs_shape = self.shape_vec();
        let rhs_shape = rhs.shape_vec();
        let cuda_native_supported =
            output_device == Device::Cuda && broadcast_shape(&lhs_shape, &rhs_shape).is_some();
        assert_native_device_support(output_device, "mul", cuda_native_supported);

        if !build_graph {
            return binary_no_grad(&self, &rhs, BinaryOp::Mul);
        }

        if output_device == Device::Cuda {
            if let Some(output) = try_cuda_binary_training(&self, &rhs, BinaryOp::Mul, "mul") {
                return output;
            }
        }

        let data = (&*self.data_ref() * &*rhs.data_ref()).into_dyn();

        let lhs = self.clone();
        let rhs = rhs.clone();

        Tensor(Rc::new(RefCell::new(TensorData {
            data: data.into_shared(),
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
            parents: vec![self.clone(), rhs.clone()],
            backward_op: Some(std::rc::Rc::new(move |grad| {
                add_cpu_binary_grads(&lhs, &rhs, grad.view(), BinaryOp::Mul);
            })),
            requires_grad: true,
            device: output_device,
        })))
    }
}
impl<'a, 'b> Mul<&'b Tensor> for &'a Tensor {
    type Output = Tensor;
    fn mul(self, rhs: &'b Tensor) -> Tensor {
        self.clone() * rhs.clone()
    }
}

pub fn sum(input: &Tensor) -> Tensor {
    let output_device = input.device();
    let build_graph = !is_no_grad() && input.requires_grad();
    let cuda_native_supported = output_device == Device::Cuda;
    assert_native_device_support(output_device, "sum", cuda_native_supported);

    if !build_graph {
        if output_device == Device::Cuda && input.len() > 0 {
            let cuda_out = input.with_cuda_f32_buffer(cuda::sum_f32);
            match cuda_out {
                Ok((buffer, out)) => {
                    let result = ndarray::arr0(out[0]).into_dyn();
                    return Tensor::from_f32_data_no_grad_with_device_dtype_and_cuda_buffer(
                        result,
                        DType::F32,
                        output_device,
                        Some(buffer),
                    );
                }
                Err(err) => {
                    assert!(
                        !is_strict_device_execution(),
                        "sum CUDA forward failed while strict device execution is enabled: {err}"
                    );
                }
            }
        }

        let sum_val =
            input.with_storage_view_preferring(StoragePreference::F32Compute, |view| match view {
                TensorStorageView::F32(view) => view.sum(),
                TensorStorageView::F16(_) => {
                    unreachable!("f32 compute preference should expose f32 view")
                }
                TensorStorageView::BF16(_) => {
                    unreachable!("f32 compute preference should expose f32 view")
                }
            });
        let result = ndarray::arr0(sum_val).into_dyn();
        return Tensor::from_f32_data_no_grad_with_device_dtype(result, DType::F32, output_device);
    }

    if output_device == Device::Cuda && input.len() > 0 {
        let input_shape = input.shape_vec();
        let input_len = input.len();
        let cuda_out = input.with_cuda_f32_buffer(cuda::sum_f32);
        match cuda_out {
            Ok((buffer, out)) => {
                let result = ndarray::arr0(out[0]).into_dyn();
                let input_clone = input.clone();
                return Tensor(Rc::new(RefCell::new(TensorData {
                    data: result.into_shared(),
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
                    parents: vec![input.clone()],
                    backward_op: Some(std::rc::Rc::new(move |grad| {
                        let g = grad.first().copied().unwrap_or(0.0);
                        match cuda::fill_scalar_f32(input_len, g) {
                            Ok((grad_buffer, grad_host)) => {
                                let grad_input =
                                    ndarray::Array::from_shape_vec(input_shape.clone(), grad_host)
                                        .expect("CUDA sum backward grad shape build failed")
                                        .into_dyn();
                                input_clone
                                    .add_grad_with_cuda_buffer(grad_input, Some(grad_buffer));
                            }
                            Err(err) => {
                                assert!(
                                    !is_strict_device_execution(),
                                    "sum CUDA backward failed while strict device execution is enabled: {err}"
                                );
                                let grad_input = ndarray::ArrayD::from_elem(input_shape.clone(), g);
                                input_clone.add_grad(grad_input);
                            }
                        }
                    })),
                    requires_grad: true,
                    device: output_device,
                })));
            }
            Err(err) => {
                assert!(
                    !is_strict_device_execution(),
                    "sum CUDA forward failed while strict device execution is enabled: {err}"
                );
            }
        }
    }

    let sum_val = input.data_ref().sum();
    let result = ndarray::arr0(sum_val).into_dyn();

    let input_clone = input.clone();

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
        parents: vec![input.clone()],
        backward_op: Some(std::rc::Rc::new(move |grad| {
            let g = grad.first().copied().unwrap_or(0.0);
            let input_shape = input_clone.shape_vec();
            let grad_input = ndarray::ArrayD::from_elem(input_shape, g);
            input_clone.add_grad(grad_input);
        })),
        requires_grad: true,
        device: output_device,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autograd::no_grad;
    use crate::precision::DType;
    use half::bf16;
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

    #[cfg(feature = "cuda")]
    fn make_training_tensor(shape: &[usize], data: Vec<f32>) -> Tensor {
        Tensor::from_data_with_grad_flag(
            Array::from_shape_vec(IxDyn(shape), data)
                .expect("test tensor shape mismatch")
                .into_dyn(),
            true,
        )
    }

    #[test]
    fn bf16_add_no_grad_preserves_dtype_and_inputs() {
        let lhs = make_tensor(&[2], vec![1.0, -2.0], DType::BF16);
        let rhs = make_tensor(&[2], vec![0.5, 3.0], DType::BF16);

        let out = no_grad(|| lhs.clone() + rhs.clone());

        assert_eq!(lhs.dtype(), DType::BF16);
        assert_eq!(rhs.dtype(), DType::BF16);
        assert_eq!(out.dtype(), DType::BF16);
        out.with_storage_view(|view| match view {
            TensorStorageView::BF16(view) => {
                let vals = view.iter().map(|v| v.to_f32()).collect::<Vec<_>>();
                let expected = vec![bf16::from_f32(1.5).to_f32(), bf16::from_f32(1.0).to_f32()];
                assert_eq!(vals, expected);
            }
            TensorStorageView::F16(_) => panic!("bf16 add output should stay bf16 in no-grad"),
            TensorStorageView::F32(_) => panic!("bf16 add output should stay bf16 in no-grad"),
        });
    }

    #[test]
    fn mixed_add_no_grad_promotes_to_f32_without_mutating_bf16_input() {
        let lhs = make_tensor(&[2], vec![1.0, -2.0], DType::BF16);
        let rhs = make_tensor(&[2], vec![0.5, 3.0], DType::F32);

        let out = no_grad(|| lhs.clone() + rhs.clone());

        assert_eq!(lhs.dtype(), DType::BF16);
        assert_eq!(rhs.dtype(), DType::F32);
        assert_eq!(out.dtype(), DType::F32);
        let vals = out.data_ref().iter().copied().collect::<Vec<_>>();
        assert_eq!(vals, vec![1.5, 1.0]);
    }

    #[test]
    fn bf16_mul_no_grad_preserves_dtype() {
        let lhs = make_tensor(&[2], vec![2.0, -1.5], DType::BF16);
        let rhs = make_tensor(&[2], vec![0.25, 2.0], DType::BF16);

        let out = no_grad(|| lhs * rhs);
        assert_eq!(out.dtype(), DType::BF16);
    }

    #[test]
    fn bf16_sum_no_grad_keeps_input_dtype() {
        let input = make_tensor(&[2, 2], vec![1.0, 2.0, 3.0, 4.0], DType::BF16);
        let out = no_grad(|| sum(&input));
        assert_eq!(input.dtype(), DType::BF16);
        assert_eq!(out.dtype(), DType::F32);
        assert_eq!(out.data_ref().first().copied(), Some(10.0));
    }

    #[test]
    fn i8_add_no_grad_preserves_dtype() {
        let lhs = make_tensor(&[2], vec![1.0, -2.0], DType::I8);
        let rhs = make_tensor(&[2], vec![0.5, 3.0], DType::I8);

        let out = no_grad(|| lhs.clone() + rhs.clone());

        assert_eq!(lhs.dtype(), DType::I8);
        assert_eq!(rhs.dtype(), DType::I8);
        assert_eq!(out.dtype(), DType::I8);
        let vals = out.data_ref().iter().copied().collect::<Vec<_>>();
        assert!((vals[0] - 1.5).abs() <= 0.02);
        assert!((vals[1] - 1.0).abs() <= 0.02);
    }

    #[test]
    fn i8_mul_no_grad_preserves_dtype() {
        let lhs = make_tensor(&[2], vec![2.0, -1.5], DType::I8);
        let rhs = make_tensor(&[2], vec![0.25, 2.0], DType::I8);

        let out = no_grad(|| lhs * rhs);
        assert_eq!(out.dtype(), DType::I8);
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_add_matches_cpu_reference() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        crate::ops::cuda::set_enabled(true);
        let lhs = make_tensor(
            &[16384],
            (0..16384).map(|i| i as f32 * 0.25).collect(),
            DType::F32,
        )
        .to_cuda();
        let rhs = make_tensor(
            &[16384],
            (0..16384).map(|i| -(i as f32) * 0.5).collect(),
            DType::F32,
        )
        .to_cuda();

        let out = no_grad(|| lhs.clone() + rhs.clone());
        assert!(out.is_cuda());

        crate::ops::cuda::set_enabled(false);
        let reference = no_grad(|| lhs.to_cpu() + rhs.to_cpu());

        let out_vals = out.data_ref().iter().copied().collect::<Vec<_>>();
        let ref_vals = reference.data_ref().iter().copied().collect::<Vec<_>>();
        for (got, expect) in out_vals.iter().zip(ref_vals.iter()) {
            assert!((got - expect).abs() < 1e-5, "got {got}, expect {expect}");
        }
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_empty_binary_and_sum_stay_stable_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        crate::ops::cuda::set_enabled(true);
        crate::autograd::set_strict_device_execution(true);

        let lhs = make_tensor(&[0, 4], vec![], DType::BF16).to_cuda();
        let rhs = make_tensor(&[0, 4], vec![], DType::BF16).to_cuda();
        let add_out = no_grad(|| lhs.clone() + rhs.clone());
        assert!(add_out.is_cuda());
        assert_eq!(add_out.dtype(), DType::BF16);
        assert_eq!(add_out.shape_vec(), vec![0, 4]);
        assert_eq!(add_out.len(), 0);

        let sum_out = no_grad(|| sum(&add_out));
        assert!(sum_out.is_cuda());
        assert_eq!(sum_out.data_ref().first().copied(), Some(0.0));

        let lhs_train = make_training_tensor(&[0, 4], vec![]).to_cuda();
        let rhs_train = make_training_tensor(&[0, 4], vec![]).to_cuda();
        let train_out = lhs_train.clone() + rhs_train.clone();
        assert!(train_out.is_cuda());
        assert_eq!(train_out.len(), 0);
        let loss = sum(&train_out);
        assert!(loss.is_cuda());
        loss.backward();

        let lhs_grad = lhs_train
            .grad()
            .expect("empty CUDA lhs grad should be recorded");
        let rhs_grad = rhs_train
            .grad()
            .expect("empty CUDA rhs grad should be recorded");
        assert_eq!(lhs_grad.len(), 0);
        assert_eq!(rhs_grad.len(), 0);

        crate::autograd::set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_sum_backward_matches_cpu_reference_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let shape = [128, 128];
        let values = (0..(shape[0] * shape[1]))
            .map(|i| (i as f32 % 67.0) / 23.0 - 1.25)
            .collect::<Vec<_>>();

        crate::ops::cuda::set_enabled(false);
        crate::autograd::set_strict_device_execution(false);
        let cpu_input = make_training_tensor(&shape, values.clone());
        let cpu_out = sum(&cpu_input);
        cpu_out.backward();
        let cpu_grad = cpu_input
            .grad()
            .expect("CPU sum backward should populate input grad");
        let cpu_sum = cpu_out.data_ref().first().copied().unwrap();

        crate::ops::cuda::set_enabled(true);
        crate::autograd::set_strict_device_execution(true);
        let cuda_input = make_training_tensor(&shape, values).to_cuda();
        let cuda_out = sum(&cuda_input);
        assert!(cuda_out.is_cuda());
        cuda_out.backward();
        let cuda_grad = cuda_input
            .grad()
            .expect("CUDA sum backward should populate input grad");
        assert!(cuda_input.cloned_cuda_f32_grad().is_some());
        let cuda_sum = cuda_out.data_ref().first().copied().unwrap();

        crate::autograd::set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);

        assert!(
            (cuda_sum - cpu_sum).abs() <= 1e-2,
            "CUDA sum got {cuda_sum}, CPU expected {cpu_sum}"
        );
        for (idx, (got, expect)) in cuda_grad.iter().zip(cpu_grad.iter()).enumerate() {
            assert!(
                (got - expect).abs() <= 1e-6,
                "sum grad mismatch at {idx}: got {got}, expect {expect}"
            );
        }
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_binary_backward_matches_cpu_reference_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let shape = [64, 256];
        let lhs_values = (0..(shape[0] * shape[1]))
            .map(|i| (i as f32 % 37.0) / 17.0 - 0.9)
            .collect::<Vec<_>>();
        let rhs_values = (0..(shape[0] * shape[1]))
            .map(|i| ((i * 11) as f32 % 43.0) / 19.0 - 0.7)
            .collect::<Vec<_>>();

        for op in [BinaryOp::Add, BinaryOp::Sub, BinaryOp::Mul] {
            crate::ops::cuda::set_enabled(false);
            crate::autograd::set_strict_device_execution(false);
            let cpu_lhs = make_training_tensor(&shape, lhs_values.clone());
            let cpu_rhs = make_training_tensor(&shape, rhs_values.clone());
            let cpu_out = match op {
                BinaryOp::Add => sum(&(cpu_lhs.clone() + cpu_rhs.clone())),
                BinaryOp::Sub => sum(&(cpu_lhs.clone() - cpu_rhs.clone())),
                BinaryOp::Mul => sum(&(cpu_lhs.clone() * cpu_rhs.clone())),
            };
            cpu_out.backward();
            let cpu_lhs_grad = cpu_lhs
                .grad()
                .expect("CPU binary backward should populate lhs grad");
            let cpu_rhs_grad = cpu_rhs
                .grad()
                .expect("CPU binary backward should populate rhs grad");

            crate::ops::cuda::set_enabled(true);
            crate::autograd::set_strict_device_execution(true);
            let cuda_lhs = make_training_tensor(&shape, lhs_values.clone()).to_cuda();
            let cuda_rhs = make_training_tensor(&shape, rhs_values.clone()).to_cuda();
            let binary_out = match op {
                BinaryOp::Add => cuda_lhs.clone() + cuda_rhs.clone(),
                BinaryOp::Sub => cuda_lhs.clone() - cuda_rhs.clone(),
                BinaryOp::Mul => cuda_lhs.clone() * cuda_rhs.clone(),
            };
            assert!(binary_out.is_cuda());
            assert!(!binary_out.has_host_f32_data());
            let cuda_out = match op {
                BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul => sum(&binary_out),
            };
            assert!(cuda_out.is_cuda());
            cuda_out.backward();
            assert!(!cuda_lhs.has_host_grad());
            assert!(!cuda_rhs.has_host_grad());
            assert!(cuda_lhs.cloned_cuda_f32_grad().is_some());
            assert!(cuda_rhs.cloned_cuda_f32_grad().is_some());
            let cuda_lhs_grad = cuda_lhs
                .grad()
                .expect("CUDA binary backward should populate lhs grad");
            let cuda_rhs_grad = cuda_rhs
                .grad()
                .expect("CUDA binary backward should populate rhs grad");

            for (idx, (got, expect)) in cuda_lhs_grad.iter().zip(cpu_lhs_grad.iter()).enumerate() {
                assert!(
                    (got - expect).abs() <= 1e-5,
                    "{op:?} lhs grad mismatch at {idx}: got {got}, expect {expect}"
                );
            }
            for (idx, (got, expect)) in cuda_rhs_grad.iter().zip(cpu_rhs_grad.iter()).enumerate() {
                assert!(
                    (got - expect).abs() <= 1e-5,
                    "{op:?} rhs grad mismatch at {idx}: got {got}, expect {expect}"
                );
            }
        }

        crate::autograd::set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn backward_traverses_node_with_cuda_only_grad() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        crate::ops::cuda::set_enabled(true);
        crate::autograd::set_strict_device_execution(true);

        let shape = [64, 256];
        let values = (0..(shape[0] * shape[1]))
            .map(|i| (i as f32 % 29.0) / 11.0 - 0.75)
            .collect::<Vec<_>>();
        let input = make_training_tensor(&shape, values).to_cuda();
        let out = input.clone() + input.clone();
        assert!(out.is_cuda());
        assert!(!out.has_host_f32_data());

        let grad = vec![1.0f32; out.len()];
        let grad_buffer =
            crate::ops::cuda::upload_f32(&grad).expect("test should upload CUDA-only grad");
        out.add_cuda_grad_buffer_only(grad_buffer);
        out.backward();
        assert!(
            !out.has_host_f32_data(),
            "strict CUDA-only backward should not materialize output data"
        );

        let input_grad = input
            .grad()
            .expect("CUDA-only grad should still propagate to input");
        assert!(input.cloned_cuda_f32_grad().is_some());
        for (idx, got) in input_grad.iter().enumerate() {
            assert!(
                (*got - 2.0).abs() <= 1e-6,
                "CUDA-only backward grad mismatch at {idx}: got {got}"
            );
        }

        crate::autograd::set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn backward_seeds_no_host_cuda_output_without_host_grad() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        crate::ops::cuda::set_enabled(true);
        crate::autograd::set_strict_device_execution(true);

        let shape = [64, 256];
        let values = (0..(shape[0] * shape[1]))
            .map(|i| (i as f32 % 23.0) / 9.0 - 0.6)
            .collect::<Vec<_>>();
        let input = make_training_tensor(&shape, values).to_cuda();
        let out = input.clone() + input.clone();
        assert!(!out.has_host_f32_data());

        out.backward();
        assert!(
            !out.has_host_f32_data(),
            "strict CUDA backward seed should stay CUDA-only for no-host outputs"
        );

        let input_grad = input.grad().expect("input grad");
        assert!(input.cloned_cuda_f32_grad().is_some());
        for (idx, got) in input_grad.iter().enumerate() {
            assert!(
                (*got - 2.0).abs() <= 1e-6,
                "seeded CUDA backward grad mismatch at {idx}: got {got}"
            );
        }

        crate::autograd::set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn backward_materializes_cuda_only_grad_for_host_backed_cuda_node() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        crate::ops::cuda::set_enabled(true);
        crate::autograd::set_strict_device_execution(true);

        let shape = [128];
        let values = (0..shape[0])
            .map(|i| i as f32 / 17.0 - 0.5)
            .collect::<Vec<_>>();
        let input = make_training_tensor(&shape, values).to_cuda();
        let out = sum(&input);
        assert!(out.is_cuda());
        assert!(
            out.has_host_f32_data(),
            "sum currently keeps a host scalar result"
        );

        let grad_buffer =
            crate::ops::cuda::upload_f32(&[3.0]).expect("test should upload CUDA-only grad");
        out.add_cuda_grad_buffer_only(grad_buffer);
        out.backward();

        let input_grad = input
            .grad()
            .expect("CUDA-only sum grad should still propagate to input");
        assert!(input.cloned_cuda_f32_grad().is_some());
        for (idx, got) in input_grad.iter().enumerate() {
            assert!(
                (*got - 3.0).abs() <= 1e-6,
                "CUDA-only sum backward grad mismatch at {idx}: got {got}"
            );
        }

        crate::autograd::set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_only_grad_accumulates_after_host_grad() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        crate::ops::cuda::set_enabled(true);
        crate::autograd::set_strict_device_execution(true);

        let input = make_training_tensor(&[3], vec![1.0, 2.0, 3.0]).to_cuda();
        let out = input.clone() + input.clone();
        out.add_grad(ArrayD::from_elem(IxDyn(&[3]), 1.0));
        let cuda_grad = crate::ops::cuda::upload_f32(&[2.0, 2.0, 2.0]).expect("upload grad");
        out.add_cuda_grad_buffer_only(cuda_grad);

        out.backward();

        let grad = input.grad().expect("input grad");
        assert_eq!(
            grad.iter().copied().collect::<Vec<_>>(),
            vec![6.0, 6.0, 6.0]
        );
        assert!(input.cloned_cuda_f32_grad().is_some());
        crate::autograd::set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_broadcast_binary_backward_matches_cpu_reference_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let lhs_shape = [32, 128];
        let rhs_shape = [128];
        let lhs_values = (0..(lhs_shape[0] * lhs_shape[1]))
            .map(|i| (i as f32 % 31.0) / 13.0 - 0.8)
            .collect::<Vec<_>>();
        let rhs_values = (0..rhs_shape[0])
            .map(|i| (i as f32 % 17.0) / 7.0 - 1.1)
            .collect::<Vec<_>>();

        for op in [BinaryOp::Add, BinaryOp::Sub, BinaryOp::Mul] {
            crate::ops::cuda::set_enabled(false);
            crate::autograd::set_strict_device_execution(false);
            let cpu_lhs = make_training_tensor(&lhs_shape, lhs_values.clone());
            let cpu_rhs = make_training_tensor(&rhs_shape, rhs_values.clone());
            let cpu_out = match op {
                BinaryOp::Add => sum(&(cpu_lhs.clone() + cpu_rhs.clone())),
                BinaryOp::Sub => sum(&(cpu_lhs.clone() - cpu_rhs.clone())),
                BinaryOp::Mul => sum(&(cpu_lhs.clone() * cpu_rhs.clone())),
            };
            cpu_out.backward();
            let cpu_lhs_grad = cpu_lhs
                .grad()
                .expect("CPU broadcast binary backward should populate lhs grad");
            let cpu_rhs_grad = cpu_rhs
                .grad()
                .expect("CPU broadcast binary backward should populate rhs grad");

            crate::ops::cuda::set_enabled(true);
            crate::autograd::set_strict_device_execution(true);
            let cuda_lhs = make_training_tensor(&lhs_shape, lhs_values.clone()).to_cuda();
            let cuda_rhs = make_training_tensor(&rhs_shape, rhs_values.clone()).to_cuda();
            let cuda_out = match op {
                BinaryOp::Add => sum(&(cuda_lhs.clone() + cuda_rhs.clone())),
                BinaryOp::Sub => sum(&(cuda_lhs.clone() - cuda_rhs.clone())),
                BinaryOp::Mul => sum(&(cuda_lhs.clone() * cuda_rhs.clone())),
            };
            assert!(cuda_out.is_cuda());
            cuda_out.backward();
            assert!(!cuda_lhs.has_host_grad());
            assert!(!cuda_rhs.has_host_grad());
            assert!(cuda_lhs.cloned_cuda_f32_grad().is_some());
            assert!(cuda_rhs.cloned_cuda_f32_grad().is_some());
            let cuda_lhs_grad = cuda_lhs
                .grad()
                .expect("CUDA broadcast binary backward should populate lhs grad");
            let cuda_rhs_grad = cuda_rhs
                .grad()
                .expect("CUDA broadcast binary backward should populate rhs grad");

            for (idx, (got, expect)) in cuda_lhs_grad.iter().zip(cpu_lhs_grad.iter()).enumerate() {
                assert!(
                    (got - expect).abs() <= 1e-5,
                    "{op:?} broadcast lhs grad mismatch at {idx}: got {got}, expect {expect}"
                );
            }
            for (idx, (got, expect)) in cuda_rhs_grad.iter().zip(cpu_rhs_grad.iter()).enumerate() {
                assert!(
                    (got - expect).abs() <= 1e-4,
                    "{op:?} broadcast rhs grad mismatch at {idx}: got {got}, expect {expect}"
                );
            }
        }

        crate::autograd::set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);
    }

    #[cfg(feature = "cuda")]
    #[test]
    #[should_panic(expected = "same device")]
    fn add_panics_on_mixed_devices() {
        if !crate::ops::cuda::is_available() {
            panic!("same device");
        }

        let lhs = make_tensor(&[2], vec![1.0, 2.0], DType::F32).to_cuda();
        let rhs = make_tensor(&[2], vec![3.0, 4.0], DType::F32);
        let _ = no_grad(|| lhs + rhs);
    }
}
