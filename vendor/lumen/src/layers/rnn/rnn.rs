use crate::autograd::Tensor;
use crate::layers::Linear;
use crate::layers::activation::Tanh;
use crate::module::Module;
use crate::precision::DType;

pub struct RNN {
    w_ih: Linear, // Input to Hidden
    w_hh: Linear, // Hidden to Hidden
    activation: Tanh,
}

impl RNN {
    pub fn new(input_size: usize, hidden_size: usize) -> Self {
        RNN {
            w_ih: Linear::new(input_size, hidden_size),
            w_hh: Linear::new(hidden_size, hidden_size),
            activation: Tanh::new(),
        }
    }

    pub fn new_with_dtype(input_size: usize, hidden_size: usize, dtype: DType) -> Self {
        RNN {
            w_ih: Linear::new_with_dtype(input_size, hidden_size, dtype),
            w_hh: Linear::new_with_dtype(hidden_size, hidden_size, dtype),
            activation: Tanh::new(),
        }
    }

    // RNN 的前向传播需要两个输入：当前输入 x 和 上一时刻隐含状态 h_prev
    pub fn forward_step(&self, input: &Tensor, h_prev: &Tensor) -> Tensor {
        // h_t = Tanh( W_ih * x + W_hh * h_{t-1} )

        let i_part = self.w_ih.forward(input.clone());
        let h_part = self.w_hh.forward(h_prev.clone());
        let combined = i_part + h_part;

        self.activation.forward(combined)
    }
}

impl Module for RNN {
    fn forward(&self, _input: Tensor) -> Tensor {
        panic!("Use forward_step for RNN!");
    }

    fn parameters(&self) -> Vec<Tensor> {
        let mut params = self.w_ih.parameters();
        params.extend(self.w_hh.parameters());
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
    fn cuda_rnn_step_backward_runs_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let rnn = RNN::new_with_dtype(3, 4, DType::F32);
        rnn.to_cuda();
        let input = grad_tensor(&[2, 3], (0..6).map(|i| i as f32 * 0.1 - 0.2).collect()).to_cuda();
        let h_prev =
            grad_tensor(&[2, 4], (0..8).map(|i| i as f32 * 0.05 - 0.1).collect()).to_cuda();
        let coeff = Tensor::from_data_with_grad_flag(
            Array::from_shape_vec(
                IxDyn(&[2, 4]),
                (0..8).map(|i| i as f32 * 0.03 - 0.2).collect(),
            )
            .expect("coeff shape mismatch")
            .into_dyn(),
            false,
        )
        .to_cuda();

        crate::ops::cuda::set_enabled(true);
        set_strict_device_execution(true);
        let out = rnn.forward_step(&input, &h_prev);
        assert!(out.is_cuda());
        assert_eq!(out.shape_vec(), vec![2, 4]);
        let loss = crate::ops::arithmetic::sum(&(&out * &coeff));
        loss.backward();
        assert!(input.cloned_cuda_f32_grad().is_some());
        assert!(h_prev.cloned_cuda_f32_grad().is_some());
        assert!(!input.has_host_grad());
        assert!(!h_prev.has_host_grad());
        for param in rnn.parameters() {
            assert!(param.cloned_cuda_f32_grad().is_some());
            assert!(!param.has_host_grad());
        }
        set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);
    }
}
