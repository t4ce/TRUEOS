use crate::autograd::{
    Device, StoragePreference, Tensor, TensorData, TensorStorageView, assert_native_device_support,
    is_no_grad, is_strict_device_execution,
};
use crate::module::Module;
use crate::ops::cuda;
use crate::precision::DType;
use ndarray::{Array2, Zip};
use std::cell::RefCell;
use std::rc::Rc;

fn unary_no_grad(
    input: &Tensor,
    cuda_op: cuda::UnaryOp,
    op: impl Fn(f32) -> f32 + Copy + Send + Sync,
) -> Tensor {
    let output_dtype = input.dtype();
    let output_device = input.device();
    input.with_storage_view_preferring(StoragePreference::F32Compute, |input_view| {
        let input_f32 = match input_view {
            TensorStorageView::F32(view) => view,
            TensorStorageView::F16(_) => {
                unreachable!("f32 compute preference should expose f32 view")
            }
            TensorStorageView::BF16(_) => {
                unreachable!("f32 compute preference should expose f32 view")
            }
        };
        if output_device == crate::autograd::Device::Cuda
            && cuda::should_accelerate_elementwise(input_f32.len())
        {
            if output_dtype == DType::F32 {
                let cuda_out = input
                    .with_cuda_f32_buffer(|input_buf| cuda::unary_f32_buffer(input_buf, cuda_op));
                if let Ok(buffer) = cuda_out {
                    return Tensor::from_cuda_f32_buffer_no_host(
                        input_f32.shape(),
                        buffer,
                        output_device,
                    );
                }
            } else {
                let cuda_out =
                    input.with_cuda_f32_buffer(|input_buf| cuda::unary_f32(input_buf, cuda_op));
                if let Ok((buffer, out)) = cuda_out {
                    let data = ndarray::Array::from_shape_vec(input_f32.raw_dim(), out)
                        .expect("CUDA unary output shape build failed")
                        .into_dyn();
                    return Tensor::from_f32_data_no_grad_with_device_dtype_and_cuda_buffer(
                        data,
                        output_dtype,
                        output_device,
                        Some(buffer),
                    );
                }
            }
        }
        let data = Zip::from(&input_f32).par_map_collect(|&x| op(x)).into_dyn();
        Tensor::from_f32_data_no_grad_with_device_dtype(data, output_dtype, output_device)
    })
}

fn softmax_no_grad(input: &Tensor, axis: usize) -> Tensor {
    let output_dtype = if input.dtype().is_float() {
        input.dtype()
    } else {
        DType::F32
    };
    let output_device = input.device();

    input.with_storage_view_preferring(StoragePreference::F32Compute, |input_view| {
        let input_view = match input_view {
            TensorStorageView::F32(view) => view,
            TensorStorageView::F16(_) => {
                unreachable!("f32 compute preference should expose f32 view")
            }
            TensorStorageView::BF16(_) => {
                unreachable!("f32 compute preference should expose f32 view")
            }
        };
        let shape = input_view.shape().to_vec();
        assert!(!shape.is_empty(), "Softmax expects at least 1D input");
        assert_eq!(
            axis,
            shape.len() - 1,
            "Softmax currently only supports the last dimension in this implementation"
        );
        let last_dim = shape[axis];
        assert!(
            last_dim > 0,
            "Softmax last dimension must be greater than zero"
        );
        let outer_dim = shape.iter().product::<usize>() / last_dim;

        if output_device == crate::autograd::Device::Cuda
            && cuda::should_accelerate_softmax(outer_dim, last_dim)
        {
            if output_dtype == DType::F32 {
                let cuda_out = input.with_cuda_f32_buffer(|input_buf| {
                    cuda::softmax_lastdim_f32_no_host(input_buf, outer_dim, last_dim)
                });
                if let Ok(buffer) = cuda_out {
                    return Tensor::from_cuda_f32_buffer_no_host(&shape, buffer, output_device);
                }
            } else {
                let cuda_out = input.with_cuda_f32_buffer(|input_buf| {
                    cuda::softmax_lastdim_f32(input_buf, outer_dim, last_dim)
                });
                if let Ok((buffer, out)) = cuda_out {
                    let out = ndarray::Array::from_shape_vec(ndarray::IxDyn(&shape), out)
                        .expect("CUDA softmax output shape build failed")
                        .into_dyn();
                    return Tensor::from_f32_data_no_grad_with_device_dtype_and_cuda_buffer(
                        out,
                        output_dtype,
                        output_device,
                        Some(buffer),
                    );
                }
            }
        }

        let x_cow = input_view.as_standard_layout();
        let x_2d = x_cow.view().into_shape((outer_dim, last_dim)).unwrap();
        let mut y_flat = Array2::<f32>::zeros((outer_dim, last_dim));
        Zip::from(y_flat.outer_iter_mut())
            .and(x_2d.outer_iter())
            .par_for_each(|mut y_row, x_row| {
                let max_val = x_row.fold(f32::NEG_INFINITY, |a, &b| a.max(b));
                let mut sum = 0.0f32;

                for (y_val, &x_val) in y_row.iter_mut().zip(x_row.iter()) {
                    let e = (x_val - max_val).exp();
                    *y_val = e;
                    sum += e;
                }

                let inv_sum = 1.0 / sum;
                for y_val in y_row.iter_mut() {
                    *y_val *= inv_sum;
                }
            });

        Tensor::from_f32_data_no_grad_with_device_dtype(
            y_flat.into_shape(shape).unwrap().into_dyn(),
            output_dtype,
            output_device,
        )
    })
}

fn relu_f32(x: f32) -> f32 {
    x.max(0.0)
}

fn relu_backward_f32(x: f32, _y: f32, grad: f32) -> f32 {
    if x > 0.0 { grad } else { 0.0 }
}

fn sigmoid_f32(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

fn sigmoid_backward_f32(_x: f32, y: f32, grad: f32) -> f32 {
    grad * y * (1.0 - y)
}

fn tanh_f32(x: f32) -> f32 {
    x.tanh()
}

fn tanh_backward_f32(_x: f32, y: f32, grad: f32) -> f32 {
    grad * (1.0 - y * y)
}

fn silu_f32(x: f32) -> f32 {
    x * sigmoid_f32(x)
}

fn silu_backward_f32(x: f32, _y: f32, grad: f32) -> f32 {
    let sig = sigmoid_f32(x);
    grad * (sig + x * sig * (1.0 - sig))
}

fn gelu_f32(x: f32) -> f32 {
    const C: f32 = 0.7978845608;
    const K: f32 = 0.044715;
    let x3 = x * x * x;
    0.5 * x * (1.0 + (C * (x + K * x3)).tanh())
}

fn gelu_backward_f32(x: f32, _y: f32, grad: f32) -> f32 {
    const C: f32 = 0.7978845608;
    const K: f32 = 0.044715;
    let x3 = x * x * x;
    let inner = C * (x + K * x3);
    let tanh_i = inner.tanh();
    let sech2 = 1.0 - tanh_i * tanh_i;
    grad * (0.5 * (1.0 + tanh_i) + 0.5 * x * sech2 * C * (1.0 + 3.0 * K * x * x))
}

fn cpu_unary_activation_data(input: &Tensor, forward: fn(f32) -> f32) -> ndarray::ArrayD<f32> {
    let input_ref = input.data_ref();
    Zip::from(&*input_ref)
        .par_map_collect(|&x| forward(x))
        .into_dyn()
}

fn cpu_unary_activation_grad(
    input: &Tensor,
    output_data: &ndarray::ArrayD<f32>,
    grad: &ndarray::ArrayViewD<'_, f32>,
    backward: fn(f32, f32, f32) -> f32,
) -> ndarray::ArrayD<f32> {
    let input_ref = input.data_ref();
    let mut grad_input = grad.to_owned().into_dyn();
    Zip::from(grad_input.view_mut())
        .and(&*input_ref)
        .and(output_data)
        .par_for_each(|g, &x, &y| {
            *g = backward(x, y, *g);
        });
    grad_input
}

fn unary_activation_with_backward(
    input: Tensor,
    op_name: &'static str,
    cuda_op: cuda::UnaryOp,
    forward: fn(f32) -> f32,
    backward: fn(f32, f32, f32) -> f32,
) -> Tensor {
    let build_graph = !is_no_grad() && input.requires_grad();
    let output_device = input.device();

    if !build_graph {
        return unary_no_grad(&input, cuda_op, forward);
    }

    let cuda_native_supported = output_device == Device::Cuda;
    assert_native_device_support(output_device, op_name, cuda_native_supported);

    if output_device == Device::Cuda && input.len() > 0 {
        let shape = input.shape_vec();
        let cuda_out =
            input.with_cuda_f32_buffer(|input_buf| cuda::unary_f32_buffer(input_buf, cuda_op));
        match cuda_out {
            Ok(buffer) => {
                let output_buffer = buffer.clone();
                let input_clone = input.clone();
                let output_self = Rc::new(RefCell::new(None::<Tensor>));
                let output_self_for_backward = output_self.clone();

                let tensor = Tensor(Rc::new(RefCell::new(TensorData {
                    data: ndarray::ArrayD::<f32>::zeros(ndarray::IxDyn(&shape)).into_shared(),
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
                    parents: vec![input.clone()],
                    backward_op: Some(std::rc::Rc::new(move |grad| {
                        let grad_shape = grad.shape().to_vec();
                        let upstream_cuda_grad = output_self_for_backward
                            .borrow()
                            .as_ref()
                            .and_then(|output| output.cloned_cuda_f32_grad())
                            .filter(|grad_buf| grad_buf.len() == grad.len());
                        let cuda_grad = if let Some(grad_buf) = upstream_cuda_grad {
                            if is_strict_device_execution() {
                                match input_clone.with_cuda_f32_buffer(|input_buf| {
                                    cuda::unary_backward_f32_buffer(
                                        input_buf,
                                        &output_buffer,
                                        &grad_buf,
                                        cuda_op,
                                    )
                                }) {
                                    Ok(grad_buffer) => {
                                        input_clone.add_cuda_grad_buffer_only(grad_buffer);
                                        return;
                                    }
                                    Err(err) => {
                                        panic!(
                                            "{op_name} CUDA backward failed while strict device execution is enabled: {err}"
                                        );
                                    }
                                }
                            }
                            input_clone.with_cuda_f32_buffer(|input_buf| {
                                cuda::unary_backward_f32(
                                    input_buf,
                                    &output_buffer,
                                    &grad_buf,
                                    cuda_op,
                                )
                            })
                        } else {
                            let grad_host = grad.iter().copied().collect::<Vec<_>>();
                            cuda::upload_f32(&grad_host).and_then(|grad_buf| {
                                input_clone.with_cuda_f32_buffer(|input_buf| {
                                    cuda::unary_backward_f32(
                                        input_buf,
                                        &output_buffer,
                                        &grad_buf,
                                        cuda_op,
                                    )
                                })
                            })
                        };

                        match cuda_grad {
                            Ok((grad_buffer, grad_input_host)) => {
                                let grad_input = ndarray::Array::from_shape_vec(
                                    ndarray::IxDyn(&grad_shape),
                                    grad_input_host,
                                )
                                .expect("CUDA unary activation grad shape build failed")
                                .into_dyn();
                                input_clone
                                    .add_grad_with_cuda_buffer(grad_input, Some(grad_buffer));
                            }
                            Err(err) => {
                                assert!(
                                    !is_strict_device_execution(),
                                    "{op_name} CUDA backward failed while strict device execution is enabled: {err}"
                                );
                                if let Ok(output_host) = cuda::download_f32(&output_buffer) {
                                    let output_data = ndarray::Array::from_shape_vec(
                                        ndarray::IxDyn(&grad_shape),
                                        output_host,
                                    )
                                    .expect("CUDA unary fallback output shape build failed")
                                    .into_dyn();
                                    let grad_input = cpu_unary_activation_grad(
                                        &input_clone,
                                        &output_data,
                                        grad,
                                        backward,
                                    );
                                    input_clone.add_grad(grad_input);
                                }
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
                    "{op_name} CUDA forward failed while strict device execution is enabled: {err}"
                );
            }
        }
    }

    let data = cpu_unary_activation_data(&input, forward);
    let output_data = data.clone();
    let input_clone = input.clone();
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
        parents: vec![input.clone()],
        backward_op: Some(std::rc::Rc::new(move |grad| {
            let grad_input = cpu_unary_activation_grad(&input_clone, &output_data, grad, backward);
            input_clone.add_grad(grad_input);
        })),
        requires_grad: true,
        device: output_device,
    })))
}

pub struct ReLU;
impl ReLU {
    pub fn new() -> Self {
        ReLU
    }
}

impl Module for ReLU {
    fn forward(&self, input: Tensor) -> Tensor {
        unary_activation_with_backward(
            input,
            "relu",
            cuda::UnaryOp::Relu,
            relu_f32,
            relu_backward_f32,
        )
    }
    fn parameters(&self) -> Vec<Tensor> {
        vec![]
    }
}

pub struct Sigmoid;
impl Sigmoid {
    pub fn new() -> Self {
        Sigmoid
    }
}

impl Module for Sigmoid {
    fn forward(&self, input: Tensor) -> Tensor {
        unary_activation_with_backward(
            input,
            "sigmoid",
            cuda::UnaryOp::Sigmoid,
            sigmoid_f32,
            sigmoid_backward_f32,
        )
    }
    fn parameters(&self) -> Vec<Tensor> {
        vec![]
    }
}

pub struct Tanh;
impl Tanh {
    pub fn new() -> Self {
        Tanh
    }
}

impl Module for Tanh {
    fn forward(&self, input: Tensor) -> Tensor {
        unary_activation_with_backward(
            input,
            "tanh",
            cuda::UnaryOp::Tanh,
            tanh_f32,
            tanh_backward_f32,
        )
    }
    fn parameters(&self) -> Vec<Tensor> {
        vec![]
    }
}

pub struct SiLU;
impl SiLU {
    pub fn new() -> Self {
        SiLU
    }
}

impl Module for SiLU {
    fn forward(&self, input: Tensor) -> Tensor {
        unary_activation_with_backward(
            input,
            "silu",
            cuda::UnaryOp::Silu,
            silu_f32,
            silu_backward_f32,
        )
    }
    fn parameters(&self) -> Vec<Tensor> {
        vec![]
    }
}

pub struct Softmax {
    axis: usize,
}

impl Softmax {
    pub fn new(axis: usize) -> Self {
        Softmax { axis }
    }
}

impl Module for Softmax {
    fn forward(&self, input: Tensor) -> Tensor {
        let build_graph = !is_no_grad() && input.requires_grad();
        let output_device = input.device();

        if !build_graph {
            return softmax_no_grad(&input, self.axis);
        }

        let shape = input.shape_vec();
        assert!(!shape.is_empty(), "Softmax expects at least 1D input");
        assert_eq!(
            self.axis,
            shape.len() - 1,
            "Softmax currently only supports the last dimension in this implementation"
        );
        let axis_idx = self.axis;
        let last_dim = shape[axis_idx];
        assert!(
            last_dim > 0,
            "Softmax last dimension must be greater than zero"
        );
        let outer_dim = input.len() / last_dim;
        let cuda_native_supported = output_device == Device::Cuda;
        assert_native_device_support(output_device, "softmax", cuda_native_supported);

        if output_device == Device::Cuda && input.len() > 0 {
            let cuda_out = input.with_cuda_f32_buffer(|input_buf| {
                cuda::softmax_lastdim_f32_no_host(input_buf, outer_dim, last_dim)
            });
            match cuda_out {
                Ok(buffer) => {
                    let output_buffer = buffer.clone();
                    let input_clone = input.clone();
                    let output_self = Rc::new(RefCell::new(None::<Tensor>));
                    let output_self_for_backward = output_self.clone();

                    let tensor = Tensor(Rc::new(RefCell::new(TensorData {
                        data: ndarray::ArrayD::<f32>::zeros(ndarray::IxDyn(&shape)).into_shared(),
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
                        parents: vec![input.clone()],
                        backward_op: Some(std::rc::Rc::new(move |grad_output| {
                            let grad_shape = grad_output.shape().to_vec();
                            let dim = grad_shape[axis_idx];
                            let outer = grad_output.len() / dim;
                            let upstream_cuda_grad = output_self_for_backward
                                .borrow()
                                .as_ref()
                                .and_then(|output| output.cloned_cuda_f32_grad())
                                .filter(|grad_buf| grad_buf.len() == grad_output.len());
                            let cuda_grad = if let Some(grad_buf) = upstream_cuda_grad {
                                if is_strict_device_execution() {
                                    match cuda::softmax_lastdim_backward_f32_buffer(
                                        &output_buffer,
                                        &grad_buf,
                                        outer,
                                        dim,
                                    ) {
                                        Ok(grad_buffer) => {
                                            input_clone.add_cuda_grad_buffer_only(grad_buffer);
                                            return;
                                        }
                                        Err(err) => {
                                            panic!(
                                                "softmax CUDA backward failed while strict device execution is enabled: {err}"
                                            );
                                        }
                                    }
                                }
                                cuda::softmax_lastdim_backward_f32(
                                    &output_buffer,
                                    &grad_buf,
                                    outer,
                                    dim,
                                )
                            } else {
                                let grad_host = grad_output.iter().copied().collect::<Vec<_>>();
                                cuda::upload_f32(&grad_host).and_then(|grad_buf| {
                                    cuda::softmax_lastdim_backward_f32(
                                        &output_buffer,
                                        &grad_buf,
                                        outer,
                                        dim,
                                    )
                                })
                            };

                            match cuda_grad {
                                Ok((grad_buffer, grad_input_host)) => {
                                    let grad_input = ndarray::Array::from_shape_vec(
                                        ndarray::IxDyn(&grad_shape),
                                        grad_input_host,
                                    )
                                    .expect("CUDA softmax grad shape build failed")
                                    .into_dyn();
                                    input_clone
                                        .add_grad_with_cuda_buffer(grad_input, Some(grad_buffer));
                                }
                                Err(err) => {
                                    assert!(
                                        !is_strict_device_execution(),
                                        "softmax CUDA backward failed while strict device execution is enabled: {err}"
                                    );
                                    if let Ok(output_host) = cuda::download_f32(&output_buffer) {
                                        let output_data = ndarray::Array::from_shape_vec(
                                            ndarray::IxDyn(&grad_shape),
                                            output_host,
                                        )
                                        .expect("CUDA softmax fallback output shape build failed")
                                        .into_dyn();
                                        let g_cow = grad_output.as_standard_layout();
                                        let g_2d = g_cow.view().into_shape((outer, dim)).unwrap();
                                        let y_cow = output_data.as_standard_layout();
                                        let y_2d = y_cow.view().into_shape((outer, dim)).unwrap();
                                        let mut d_input_flat = Array2::<f32>::zeros((outer, dim));

                                        Zip::from(d_input_flat.outer_iter_mut())
                                            .and(y_2d.outer_iter())
                                            .and(g_2d.outer_iter())
                                            .par_for_each(|mut di_row, y_row, g_row| {
                                                let dot: f32 = y_row
                                                    .iter()
                                                    .zip(g_row.iter())
                                                    .map(|(&y, &g)| y * g)
                                                    .sum();

                                                for (di, (&y, &g)) in di_row
                                                    .iter_mut()
                                                    .zip(y_row.iter().zip(g_row.iter()))
                                                {
                                                    *di = y * (g - dot);
                                                }
                                            });

                                        let d_input =
                                            d_input_flat.into_shape(grad_shape).unwrap().into_dyn();
                                        input_clone.add_grad(d_input);
                                    }
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
                        "softmax CUDA forward failed while strict device execution is enabled: {err}"
                    );
                }
            }
        }

        let (y, output_data) = {
            let input_ref = input.data_ref();
            let x = &*input_ref;
            let shape = x.shape();
            assert!(!shape.is_empty(), "Softmax expects at least 1D input");
            assert_eq!(
                self.axis,
                shape.len() - 1,
                "Softmax currently only supports the last dimension in this implementation"
            );

            let axis = self.axis;

            let last_dim = shape[axis];
            let outer_dim = x.len() / last_dim;
            let x_cow = x.as_standard_layout();
            let x_2d = x_cow.view().into_shape((outer_dim, last_dim)).unwrap();

            let mut y_flat = Array2::<f32>::zeros((outer_dim, last_dim));
            Zip::from(y_flat.outer_iter_mut())
                .and(x_2d.outer_iter())
                .par_for_each(|mut y_row, x_row| {
                    let max_val = x_row.fold(f32::NEG_INFINITY, |a, &b| a.max(b));
                    let mut sum = 0.0f32;

                    for (y_val, &x_val) in y_row.iter_mut().zip(x_row.iter()) {
                        let e = (x_val - max_val).exp();
                        *y_val = e;
                        sum += e;
                    }

                    let inv_sum = 1.0 / sum;
                    for y_val in y_row.iter_mut() {
                        *y_val *= inv_sum;
                    }
                });

            let y = y_flat.into_shape(shape).unwrap();
            (y.clone(), y)
        };

        let input_clone = input.clone();
        let axis_idx = self.axis;

        Tensor(Rc::new(RefCell::new(TensorData {
            data: y.into_shared(),
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
            backward_op: Some(std::rc::Rc::new(move |grad_output| {
                let grad_shape = grad_output.shape();
                let dim = grad_shape[axis_idx];
                let outer = grad_output.len() / dim;
                let g_cow = grad_output.as_standard_layout();
                let g_2d = g_cow.view().into_shape((outer, dim)).unwrap();

                let y_cow = output_data.as_standard_layout();
                let y_2d = y_cow.view().into_shape((outer, dim)).unwrap();

                let mut d_input_flat = Array2::<f32>::zeros((outer, dim));

                Zip::from(d_input_flat.outer_iter_mut())
                    .and(y_2d.outer_iter())
                    .and(g_2d.outer_iter())
                    .par_for_each(|mut di_row, y_row, g_row| {
                        let dot: f32 = y_row.iter().zip(g_row.iter()).map(|(&y, &g)| y * g).sum();

                        for (di, (&y, &g)) in di_row.iter_mut().zip(y_row.iter().zip(g_row.iter()))
                        {
                            *di = y * (g - dot);
                        }
                    });

                let d_input = d_input_flat.into_shape(grad_shape).unwrap();
                input_clone.add_grad(d_input);
            })),
            requires_grad: true,
            device: output_device,
        })))
    }
    fn parameters(&self) -> Vec<Tensor> {
        vec![]
    }
}

pub struct Gelu;
impl Gelu {
    pub fn new() -> Self {
        Gelu
    }
}

impl Module for Gelu {
    fn forward(&self, input: Tensor) -> Tensor {
        unary_activation_with_backward(
            input,
            "gelu",
            cuda::UnaryOp::Gelu,
            gelu_f32,
            gelu_backward_f32,
        )
    }
    fn parameters(&self) -> Vec<Tensor> {
        vec![]
    }
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

    #[cfg(feature = "cuda")]
    fn assert_cuda_activation_backward_matches_cpu<M: Module>(
        make_module: impl Fn() -> M,
        input_values: &[f32],
        tolerance: f32,
    ) {
        crate::ops::cuda::set_enabled(false);
        crate::autograd::set_strict_device_execution(false);
        let cpu_input = make_training_tensor(&[64, 256], input_values.to_vec());
        let cpu_out = make_module().forward(cpu_input.clone());
        cpu_out.backward();
        let cpu_grad = cpu_input
            .grad()
            .expect("CPU activation backward should populate input grad");

        crate::ops::cuda::set_enabled(true);
        crate::autograd::set_strict_device_execution(true);
        let cuda_input = make_training_tensor(&[64, 256], input_values.to_vec()).to_cuda();
        let cuda_out = make_module().forward(cuda_input.clone());
        assert!(cuda_out.is_cuda());
        cuda_out.backward();
        assert!(cuda_input.cloned_cuda_f32_grad().is_some());
        assert!(!cuda_input.has_host_grad());
        let cuda_grad = cuda_input
            .grad()
            .expect("CUDA activation backward should populate input grad");

        crate::autograd::set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);

        for (idx, (got, expect)) in cuda_grad.iter().zip(cpu_grad.iter()).enumerate() {
            assert!(
                (got - expect).abs() <= tolerance,
                "activation grad mismatch at {idx}: got {got}, expect {expect}"
            );
        }
    }

    #[test]
    fn relu_no_grad_preserves_bf16_dtype() {
        let input = make_tensor(&[3], vec![-1.0, 0.5, 2.0], DType::BF16);
        let out = no_grad(|| ReLU::new().forward(input.clone()));

        assert_eq!(input.dtype(), DType::BF16);
        assert_eq!(out.dtype(), DType::BF16);
    }

    #[test]
    fn softmax_no_grad_preserves_bf16_dtype() {
        let input = make_tensor(&[1, 4], vec![1.0, 2.0, 3.0, 4.0], DType::BF16);
        let ref_out = no_grad(|| {
            Softmax::new(1).forward(make_tensor(&[1, 4], vec![1.0, 2.0, 3.0, 4.0], DType::F32))
        });
        let out = no_grad(|| Softmax::new(1).forward(input.clone()));

        assert_eq!(input.dtype(), DType::BF16);
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
            TensorStorageView::F16(_) => panic!("bf16 softmax output should stay bf16 in no-grad"),
            TensorStorageView::F32(_) => panic!("bf16 softmax output should stay bf16 in no-grad"),
        });
    }

    #[test]
    fn relu_no_grad_preserves_i8_dtype() {
        let input = make_tensor(&[3], vec![-1.0, 0.5, 2.0], DType::I8);
        let out = no_grad(|| ReLU::new().forward(input.clone()));

        assert_eq!(input.dtype(), DType::I8);
        assert_eq!(out.dtype(), DType::I8);
        let vals = out.data_ref().iter().copied().collect::<Vec<_>>();
        assert!(vals[0].abs() <= 0.02);
        assert!((vals[1] - 0.5).abs() <= 0.02);
        assert!((vals[2] - 2.0).abs() <= 0.02);
    }

    #[test]
    fn softmax_no_grad_promotes_i8_to_f32() {
        let input = make_tensor(&[1, 4], vec![1.0, 2.0, 3.0, 4.0], DType::I8);
        let out = no_grad(|| Softmax::new(1).forward(input.clone()));

        assert_eq!(input.dtype(), DType::I8);
        assert_eq!(out.dtype(), DType::F32);
        let vals = out.data_ref().iter().copied().collect::<Vec<_>>();
        let sum: f32 = vals.iter().sum();
        assert!((sum - 1.0).abs() <= 1e-5);
    }

    #[test]
    #[should_panic(expected = "only supports the last dimension")]
    fn softmax_rejects_non_last_axis() {
        let input = make_tensor(&[2, 3], (0..6).map(|v| v as f32).collect(), DType::F32);
        no_grad(|| {
            let _ = Softmax::new(0).forward(input);
        });
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_relu_and_softmax_match_cpu_reference() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        crate::ops::cuda::set_enabled(true);
        let input = make_tensor(
            &[64, 256],
            (0..(64 * 256))
                .map(|i| (i as f32 % 37.0) / 9.0 - 2.0)
                .collect(),
            DType::F32,
        )
        .to_cuda();

        let relu_out = no_grad(|| ReLU::new().forward(input.clone()));
        let softmax_out = no_grad(|| Softmax::new(1).forward(input.clone()));
        assert!(relu_out.is_cuda());
        assert!(softmax_out.is_cuda());

        crate::ops::cuda::set_enabled(false);
        let relu_ref = no_grad(|| ReLU::new().forward(input.to_cpu()));
        let softmax_ref = no_grad(|| Softmax::new(1).forward(input.to_cpu()));

        for (got, expect) in relu_out.data_ref().iter().zip(relu_ref.data_ref().iter()) {
            assert!(
                (got - expect).abs() < 1e-5,
                "relu got {got}, expect {expect}"
            );
        }
        for (got, expect) in softmax_out
            .data_ref()
            .iter()
            .zip(softmax_ref.data_ref().iter())
        {
            assert!(
                (got - expect).abs() < 1e-4,
                "softmax got {got}, expect {expect}"
            );
        }
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_empty_activation_forward_backward_stays_stable_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        crate::ops::cuda::set_enabled(true);
        crate::autograd::set_strict_device_execution(true);

        let relu_input = make_tensor(&[0, 4], vec![], DType::BF16).to_cuda();
        let relu_out = no_grad(|| ReLU::new().forward(relu_input.clone()));
        assert!(relu_out.is_cuda());
        assert_eq!(relu_out.dtype(), DType::BF16);
        assert_eq!(relu_out.shape_vec(), vec![0, 4]);

        let softmax_input = make_tensor(&[0, 4], vec![], DType::F32).to_cuda();
        let softmax_out = no_grad(|| Softmax::new(1).forward(softmax_input.clone()));
        assert!(softmax_out.is_cuda());
        assert_eq!(softmax_out.shape_vec(), vec![0, 4]);

        let relu_train = make_training_tensor(&[0, 4], vec![]).to_cuda();
        let relu_train_out = ReLU::new().forward(relu_train.clone());
        assert!(relu_train_out.is_cuda());
        relu_train_out.backward();
        let relu_grad = relu_train
            .grad()
            .expect("empty CUDA relu grad should be recorded");
        assert_eq!(relu_grad.len(), 0);

        let softmax_train = make_training_tensor(&[0, 4], vec![]).to_cuda();
        let softmax_train_out = Softmax::new(1).forward(softmax_train.clone());
        assert!(softmax_train_out.is_cuda());
        softmax_train_out.backward();
        let softmax_grad = softmax_train
            .grad()
            .expect("empty CUDA softmax grad should be recorded");
        assert_eq!(softmax_grad.len(), 0);

        crate::autograd::set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_activation_backward_matches_cpu_reference_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let values = (0..(64 * 256))
            .map(|i| {
                let phase = (i as f32 % 53.0) / 13.0 - 2.0;
                phase + ((i / 53) as f32 % 7.0) * 0.03125
            })
            .collect::<Vec<_>>();

        assert_cuda_activation_backward_matches_cpu(ReLU::new, &values, 1e-5);
        assert_cuda_activation_backward_matches_cpu(Sigmoid::new, &values, 1e-4);
        assert_cuda_activation_backward_matches_cpu(Tanh::new, &values, 1e-4);
        assert_cuda_activation_backward_matches_cpu(SiLU::new, &values, 2e-4);
        assert_cuda_activation_backward_matches_cpu(Gelu::new, &values, 2e-4);
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_softmax_backward_matches_cpu_reference_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let shape = [32, 128];
        let values = (0..(shape[0] * shape[1]))
            .map(|i| (i as f32 % 41.0) / 11.0 - 1.7)
            .collect::<Vec<_>>();
        let upstream = (0..(shape[0] * shape[1]))
            .map(|i| ((i * 17) as f32 % 29.0) / 19.0 - 0.6)
            .collect::<Vec<_>>();
        let upstream_grad = Array::from_shape_vec(IxDyn(&shape), upstream)
            .expect("test grad shape mismatch")
            .into_dyn();

        crate::ops::cuda::set_enabled(false);
        crate::autograd::set_strict_device_execution(false);
        let cpu_input = make_training_tensor(&shape, values.clone());
        let cpu_out = Softmax::new(1).forward(cpu_input.clone());
        cpu_out.add_grad(upstream_grad.clone());
        cpu_out.backward();
        let cpu_grad = cpu_input
            .grad()
            .expect("CPU softmax backward should populate input grad");

        crate::ops::cuda::set_enabled(true);
        crate::autograd::set_strict_device_execution(true);
        let cuda_input = make_training_tensor(&shape, values).to_cuda();
        let cuda_out = Softmax::new(1).forward(cuda_input.clone());
        assert!(cuda_out.is_cuda());
        cuda_out.add_grad(upstream_grad);
        cuda_out.backward();
        assert!(cuda_input.cloned_cuda_f32_grad().is_some());
        assert!(!cuda_input.has_host_grad());
        let cuda_grad = cuda_input
            .grad()
            .expect("CUDA softmax backward should populate input grad");

        crate::autograd::set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);

        for (idx, (got, expect)) in cuda_grad.iter().zip(cpu_grad.iter()).enumerate() {
            assert!(
                (got - expect).abs() <= 2e-4,
                "softmax grad mismatch at {idx}: got {got}, expect {expect}"
            );
        }
    }
}
