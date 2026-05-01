use crate::autograd::Tensor;
use crate::layers::Linear;
use crate::layers::activation::{Sigmoid, Tanh};
use crate::module::Module;
use crate::ops::shape::slice_last_dim;
use crate::precision::{DType, default_parameter_quantization};
use ndarray::s;

pub struct LSTM {
    hidden_size: usize,
    w_x: Linear,
    w_h: Linear,
    sigmoid: Sigmoid,
    tanh: Tanh,
}

impl LSTM {
    pub fn new(input_size: usize, hidden_size: usize) -> Self {
        let w_x = Linear::new(input_size, 4 * hidden_size);
        let w_h = Linear::new(hidden_size, 4 * hidden_size);

        if let Some(bias_tensor) = &w_x.bias {
            {
                let mut bias_view = bias_tensor.data_mut();
                bias_view
                    .slice_mut(s![hidden_size..2 * hidden_size])
                    .mapv_inplace(|_| 1.0);
            }
            let dtype = bias_tensor.dtype();
            if dtype.is_integer() {
                let quantization = default_parameter_quantization();
                if quantization.is_enabled() && quantization.storage_dtype() == Some(dtype) {
                    bias_tensor.quantize_inplace_with_quantization(quantization);
                } else {
                    bias_tensor.cast_inplace(dtype);
                }
            } else {
                bias_tensor.cast_inplace(dtype);
            }
        }

        LSTM {
            hidden_size,
            w_x,
            w_h,
            sigmoid: Sigmoid::new(),
            tanh: Tanh::new(),
        }
    }

    pub fn new_with_dtype(input_size: usize, hidden_size: usize, dtype: DType) -> Self {
        let w_x = Linear::new_with_dtype(input_size, 4 * hidden_size, dtype);
        let w_h = Linear::new_with_dtype(hidden_size, 4 * hidden_size, dtype);
        // 我们的切分顺序是: [Input, Forget, Cell, Output]
        // 所以 Forget Gate 在索引 [hidden_size .. 2*hidden_size]

        if let Some(bias_tensor) = &w_x.bias {
            {
                let mut bias_view = bias_tensor.data_mut();
                bias_view
                    .slice_mut(s![hidden_size..2 * hidden_size])
                    .mapv_inplace(|_| 1.0);
            }
            bias_tensor.cast_inplace(dtype);
        }

        LSTM {
            hidden_size,
            w_x,
            w_h,
            sigmoid: Sigmoid::new(),
            tanh: Tanh::new(),
        }
    }

    pub fn forward_step(&self, x: &Tensor, h_prev: &Tensor, c_prev: &Tensor) -> (Tensor, Tensor) {
        let h_size = self.hidden_size;

        //融合计算
        let gates = self.w_x.forward(x.clone()) + self.w_h.forward(h_prev.clone());

        //切分
        // chunk 0: Input (i)
        // chunk 1: Forget (f) <-- Bias 已经被初始化为 1.0
        // chunk 2: Cell (g)
        // chunk 3: Output (o)
        let i_raw = slice_last_dim(&gates, 0 * h_size, h_size);
        let f_raw = slice_last_dim(&gates, 1 * h_size, 2 * h_size);
        let g_raw = slice_last_dim(&gates, 2 * h_size, 3 * h_size);
        let o_raw = slice_last_dim(&gates, 3 * h_size, 4 * h_size);

        //激活
        let i = self.sigmoid.forward(i_raw);
        let f = self.sigmoid.forward(f_raw);
        let g = self.tanh.forward(g_raw);
        let o = self.sigmoid.forward(o_raw);

        //Update
        // c_t = f * c_{t-1} + i * g
        // 因为 f 初始值较大，c_{t-1} 容易被保留
        let c_t = (f * c_prev.clone()) + (i * g);

        //Output
        let h_t = o * self.tanh.forward(c_t.clone());

        (h_t, c_t)
    }
}

impl Module for LSTM {
    fn forward(&self, _input: Tensor) -> Tensor {
        panic!("Use forward_step() for LSTM. It returns (h, c).");
    }

    fn parameters(&self) -> Vec<Tensor> {
        let mut params = Vec::new();
        params.extend(self.w_x.parameters());
        params.extend(self.w_h.parameters());
        params
    }
}

#[cfg(all(test, feature = "cuda"))]
mod tests {
    use super::*;
    use crate::autograd::{Tensor, set_strict_device_execution};
    use ndarray::{Array, IxDyn};

    fn grad_tensor(shape: &[usize], data: Vec<f32>) -> Tensor {
        Tensor::from_data_with_grad_flag(
            Array::from_shape_vec(IxDyn(shape), data)
                .expect("tensor shape mismatch")
                .into_dyn(),
            true,
        )
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_lstm_step_backward_runs_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let lstm = LSTM::new_with_dtype(3, 4, DType::F32);
        lstm.to_cuda();
        let input = grad_tensor(&[2, 3], (0..6).map(|i| i as f32 * 0.1 - 0.2).collect()).to_cuda();
        let h_prev =
            grad_tensor(&[2, 4], (0..8).map(|i| i as f32 * 0.05 - 0.1).collect()).to_cuda();
        let c_prev =
            grad_tensor(&[2, 4], (0..8).map(|i| i as f32 * -0.04 + 0.15).collect()).to_cuda();
        let h_coeff = Tensor::from_data_with_grad_flag(
            Array::from_shape_vec(
                IxDyn(&[2, 4]),
                (0..8).map(|i| i as f32 * 0.03 - 0.2).collect(),
            )
            .expect("h coeff shape mismatch")
            .into_dyn(),
            false,
        )
        .to_cuda();
        let c_coeff = Tensor::from_data_with_grad_flag(
            Array::from_shape_vec(
                IxDyn(&[2, 4]),
                (0..8).map(|i| i as f32 * -0.02 + 0.1).collect(),
            )
            .expect("c coeff shape mismatch")
            .into_dyn(),
            false,
        )
        .to_cuda();

        crate::ops::cuda::set_enabled(true);
        set_strict_device_execution(true);
        let (h_t, c_t) = lstm.forward_step(&input, &h_prev, &c_prev);
        assert!(h_t.is_cuda());
        assert!(c_t.is_cuda());
        assert_eq!(h_t.shape_vec(), vec![2, 4]);
        assert_eq!(c_t.shape_vec(), vec![2, 4]);
        let h_loss = crate::ops::arithmetic::sum(&(&h_t * &h_coeff));
        let c_loss = crate::ops::arithmetic::sum(&(&c_t * &c_coeff));
        let loss = h_loss + c_loss;
        loss.backward();
        assert!(input.cloned_cuda_f32_grad().is_some());
        assert!(h_prev.cloned_cuda_f32_grad().is_some());
        assert!(c_prev.cloned_cuda_f32_grad().is_some());
        assert!(!input.has_host_grad());
        assert!(!h_prev.has_host_grad());
        assert!(!c_prev.has_host_grad());
        for param in lstm.parameters() {
            assert!(param.cloned_cuda_f32_grad().is_some());
            assert!(!param.has_host_grad());
        }
        set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);
    }
}
