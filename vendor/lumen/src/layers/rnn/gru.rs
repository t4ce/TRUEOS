// src/layers/gru.rs
use crate::autograd::Tensor;
use crate::layers::Linear;
use crate::layers::activation::{Sigmoid, Tanh};
use crate::module::Module;
use crate::ops::shape::slice_last_dim;
use crate::precision::DType;

pub struct GRU {
    hidden_size: usize,

    // w_x_rz: 负责将 Input 映射到 (Reset Gate, Update Gate)
    // 输出维度是 2 * hidden_size
    w_x_rz: Linear,
    // w_h_rz: 负责将 Hidden 映射到 (Reset Gate, Update Gate)
    w_h_rz: Linear,

    // 候选状态部分 (Candidate) 依然保持独立，因为 n_t 需要 r_t 先算出来
    w_x_n: Linear,
    w_h_n: Linear,

    sigmoid: Sigmoid,
    tanh: Tanh,
}

impl GRU {
    pub fn new(input_size: usize, hidden_size: usize) -> Self {
        GRU {
            hidden_size,
            w_x_rz: Linear::new(input_size, 2 * hidden_size),
            w_h_rz: Linear::new(hidden_size, 2 * hidden_size),
            w_x_n: Linear::new(input_size, hidden_size),
            w_h_n: Linear::new(hidden_size, hidden_size),
            sigmoid: Sigmoid::new(),
            tanh: Tanh::new(),
        }
    }

    pub fn new_with_dtype(input_size: usize, hidden_size: usize, dtype: DType) -> Self {
        GRU {
            hidden_size,
            // 融合 r 和 z 门
            w_x_rz: Linear::new_with_dtype(input_size, 2 * hidden_size, dtype),
            w_h_rz: Linear::new_with_dtype(hidden_size, 2 * hidden_size, dtype),

            w_x_n: Linear::new_with_dtype(input_size, hidden_size, dtype),
            w_h_n: Linear::new_with_dtype(hidden_size, hidden_size, dtype),

            sigmoid: Sigmoid::new(),
            tanh: Tanh::new(),
        }
    }

    pub fn forward_step(&self, x: &Tensor, h_prev: &Tensor) -> Tensor {
        let h_size = self.hidden_size;

        //计算融合的 Gate 预激活值
        // gate_x = x @ W_x_rz
        // gate_h = h @ W_h_rz
        // gates = gate_x + gate_h
        let gate_x = self.w_x_rz.forward(x.clone());
        let gate_h = self.w_h_rz.forward(h_prev.clone());
        let gates = gate_x + gate_h;

        //切片 (Split)
        // 将 [Batch, 2*H] 切分为两个 [Batch, H]
        // r_t (Reset Gate), z_t (Update Gate)
        let r_gate_raw = slice_last_dim(&gates, 0, h_size);
        let z_gate_raw = slice_last_dim(&gates, h_size, 2 * h_size);

        // 激活
        let r_t = self.sigmoid.forward(r_gate_raw);
        let z_t = self.sigmoid.forward(z_gate_raw);

        // 计算候选状态 n_t
        // n_t = tanh( W_xn * x + r_t * (W_hn * h) )
        let n_hidden = self.w_h_n.forward(h_prev.clone());
        let h_reset = r_t * n_hidden; // 逐元素乘法

        let n_t = self.tanh.forward(self.w_x_n.forward(x.clone()) + h_reset);

        // 混合新旧状态
        // h_t = (1 - z) * n + z * h
        // 优化公式: h_t = n + z * (h - n)
        let diff = h_prev.clone() - n_t.clone();
        let update = z_t * diff;

        n_t + update
    }
}

impl Module for GRU {
    fn forward(&self, _input: Tensor) -> Tensor {
        panic!("Use forward_step() for RNNs");
    }

    fn parameters(&self) -> Vec<Tensor> {
        let mut params = Vec::new();
        params.extend(self.w_x_rz.parameters());
        params.extend(self.w_h_rz.parameters());
        params.extend(self.w_x_n.parameters());
        params.extend(self.w_h_n.parameters());
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
    fn cuda_gru_step_backward_runs_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let gru = GRU::new_with_dtype(3, 4, DType::F32);
        gru.to_cuda();
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
        let out = gru.forward_step(&input, &h_prev);
        assert!(out.is_cuda());
        assert_eq!(out.shape_vec(), vec![2, 4]);
        let loss = crate::ops::arithmetic::sum(&(&out * &coeff));
        loss.backward();
        assert!(input.cloned_cuda_f32_grad().is_some());
        assert!(h_prev.cloned_cuda_f32_grad().is_some());
        assert!(!input.has_host_grad());
        assert!(!h_prev.has_host_grad());
        for param in gru.parameters() {
            assert!(param.cloned_cuda_f32_grad().is_some());
            assert!(!param.has_host_grad());
        }
        set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);
    }
}
