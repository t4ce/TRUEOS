use crate::autograd::Tensor;
use crate::ops::cuda;
use crate::precision::{DType, default_runtime_dtype};
use ndarray::Zip;
use ndarray::prelude::*;

pub trait Optimizer {
    fn step(&mut self);
    fn zero_grad(&self) {
        for param in self.params() {
            param.zero_grad();
        }
    }
    fn params(&self) -> &Vec<Tensor>;
}

pub struct SGD {
    params: Vec<Tensor>,
    lr: f32,
    momentum: f32,
    state_dtype: DType,
    velocities: Vec<Option<Tensor>>,
}

impl SGD {
    pub fn new(params: Vec<Tensor>, lr: f32) -> Self {
        Self::new_with_dtype(params, lr, default_runtime_dtype())
    }

    pub fn new_with_dtype(params: Vec<Tensor>, lr: f32, state_dtype: DType) -> Self {
        assert!(
            state_dtype.is_float(),
            "Optimizer state currently only supports floating dtypes, got {:?}",
            state_dtype
        );
        let len = params.len();
        SGD {
            params,
            lr,
            momentum: 0.0, // 默认无动量
            state_dtype,
            velocities: vec![None; len],
        }
    }

    #[inline]
    pub fn state_dtype(&self) -> DType {
        self.state_dtype
    }

    pub fn with_momentum(mut self, momentum: f32) -> Self {
        self.momentum = momentum;
        self
    }
}

impl Optimizer for SGD {
    fn params(&self) -> &Vec<Tensor> {
        &self.params
    }

    fn step(&mut self) {
        for (i, param) in self.params.iter().enumerate() {
            if self.momentum == 0.0 && try_cuda_sgd_step(param, self.lr) {
                continue;
            }

            let grad = match param.grad_arc() {
                Some(g) => g,
                None => continue,
            };

            if self.momentum == 0.0 {
                let lr = self.lr;
                let mut data = param.data_mut();
                Zip::from(data.view_mut())
                    .and(grad.view())
                    .for_each(|w, g| {
                        *w -= lr * *g;
                    });
            } else {
                if self.velocities[i].is_none() {
                    let mut state =
                        Tensor::from_array_no_grad(ArrayD::zeros(IxDyn(&param.shape_vec())));
                    state.cast_inplace(self.state_dtype);
                    if param.is_cuda() && self.state_dtype == DType::F32 {
                        state = state.to_cuda();
                    }
                    self.velocities[i] = Some(state);
                }

                let m = self.momentum;
                let lr = self.lr;
                let v_buf = self.velocities[i].as_ref().unwrap();
                if self.state_dtype == DType::F32 && try_cuda_sgd_momentum_step(param, v_buf, lr, m)
                {
                    continue;
                }
                let mut next_v = v_buf.data();

                Zip::from(next_v.view_mut())
                    .and(grad.view())
                    .for_each(|v, g| {
                        *v = m * (*v) + *g;
                    });

                let mut data = param.data_mut();
                Zip::from(data.view_mut())
                    .and(next_v.view())
                    .for_each(|w, vv| {
                        *w -= lr * *vv;
                    });
                v_buf.set_array_f32_with_dtype(next_v, self.state_dtype);
            }
        }
    }
}

fn try_cuda_sgd_step(param: &Tensor, lr: f32) -> bool {
    if !param.is_cuda() || !param.dtype().is_float() {
        return false;
    }
    let Some(param_buf) = param.cloned_cuda_f32_buffer() else {
        return false;
    };
    let Some(grad_buf) = param.cloned_cuda_f32_grad() else {
        return false;
    };
    let Ok(()) = cuda::sgd_update_f32_no_host(&param_buf, &grad_buf, lr) else {
        return false;
    };
    param.replace_cuda_f32_buffer_no_host_sync(param_buf);
    true
}

fn try_cuda_sgd_momentum_step(param: &Tensor, velocity: &Tensor, lr: f32, momentum: f32) -> bool {
    if !param.is_cuda() || !param.dtype().is_float() || !velocity.is_cuda() {
        return false;
    }
    let Some(param_buf) = param.cloned_cuda_f32_buffer() else {
        return false;
    };
    let Some(grad_buf) = param.cloned_cuda_f32_grad() else {
        return false;
    };
    let Some(velocity_buf) = velocity.cloned_cuda_f32_buffer() else {
        return false;
    };
    let Ok(()) =
        cuda::sgd_momentum_update_f32_no_host(&param_buf, &grad_buf, &velocity_buf, lr, momentum)
    else {
        return false;
    };
    param.replace_cuda_f32_buffer_no_host_sync(param_buf);
    velocity.replace_cuda_f32_buffer_no_host_sync(velocity_buf);
    true
}

pub struct Adam {
    params: Vec<Tensor>,
    lr: f32,
    betas: (f32, f32),
    eps: f32,

    // 状态
    step_count: usize,
    state_dtype: DType,
    exp_avg: Vec<Option<Tensor>>,    // m (一阶矩)
    exp_avg_sq: Vec<Option<Tensor>>, // v (二阶矩)
}

impl Adam {
    pub fn new(params: Vec<Tensor>, lr: f32) -> Self {
        Self::new_with_dtype(params, lr, default_runtime_dtype())
    }

    pub fn new_with_dtype(params: Vec<Tensor>, lr: f32, state_dtype: DType) -> Self {
        assert!(
            state_dtype.is_float(),
            "Optimizer state currently only supports floating dtypes, got {:?}",
            state_dtype
        );
        let len = params.len();
        Adam {
            params,
            lr,
            betas: (0.9, 0.999),
            eps: 1e-8,
            step_count: 0,
            state_dtype,
            exp_avg: vec![None; len],
            exp_avg_sq: vec![None; len],
        }
    }

    #[inline]
    pub fn state_dtype(&self) -> DType {
        self.state_dtype
    }
}

impl Optimizer for Adam {
    fn params(&self) -> &Vec<Tensor> {
        &self.params
    }

    fn step(&mut self) {
        self.step_count += 1;
        let (beta1, beta2) = self.betas;

        // 预计算 Bias Correction
        let bias_correction1 = 1.0 - beta1.powi(self.step_count as i32);
        let bias_correction2 = 1.0 - beta2.powi(self.step_count as i32);

        for (i, param) in self.params.iter().enumerate() {
            let has_grad = param.cloned_cuda_f32_grad().is_some() || param.grad_ref().is_some();
            if !has_grad {
                continue;
            }

            if self.exp_avg[i].is_none() {
                let mut exp_avg =
                    Tensor::from_array_no_grad(ArrayD::zeros(IxDyn(&param.shape_vec())));
                exp_avg.cast_inplace(self.state_dtype);
                let exp_avg_sq =
                    Tensor::from_array_no_grad(ArrayD::zeros(IxDyn(&param.shape_vec())));
                let mut exp_avg_sq = exp_avg_sq;
                exp_avg_sq.cast_inplace(self.state_dtype);
                if param.is_cuda() && self.state_dtype == DType::F32 {
                    exp_avg = exp_avg.to_cuda();
                    exp_avg_sq = exp_avg_sq.to_cuda();
                }
                self.exp_avg[i] = Some(exp_avg);
                self.exp_avg_sq[i] = Some(exp_avg_sq);
            }

            let lr = self.lr;
            let eps = self.eps;
            let m_buf = self.exp_avg[i].as_ref().unwrap();
            let v_buf = self.exp_avg_sq[i].as_ref().unwrap();

            if self.state_dtype == DType::F32
                && try_cuda_adam_step(
                    param,
                    m_buf,
                    v_buf,
                    lr,
                    beta1,
                    beta2,
                    bias_correction1,
                    bias_correction2,
                    eps,
                )
            {
                continue;
            }

            let grad = match param.grad_arc() {
                Some(g) => g,
                None => continue,
            };

            let mut m_next = m_buf.data();
            let mut v_next = v_buf.data();
            let mut data = param.data_mut();

            Zip::from(data.view_mut())
                .and(m_next.view_mut())
                .and(v_next.view_mut())
                .and(grad.view())
                .for_each(|w, m, v, g| {
                    *m = beta1 * (*m) + (1.0 - beta1) * g;
                    *v = beta2 * (*v) + (1.0 - beta2) * g * g;
                    let m_hat = *m / bias_correction1;
                    let v_hat = *v / bias_correction2;
                    *w -= lr * (m_hat / (v_hat.sqrt() + eps));
                });
            m_buf.set_array_f32_with_dtype(m_next, self.state_dtype);
            v_buf.set_array_f32_with_dtype(v_next, self.state_dtype);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn try_cuda_adam_step(
    param: &Tensor,
    exp_avg: &Tensor,
    exp_avg_sq: &Tensor,
    lr: f32,
    beta1: f32,
    beta2: f32,
    bias_correction1: f32,
    bias_correction2: f32,
    eps: f32,
) -> bool {
    if !param.is_cuda() || !param.dtype().is_float() || !exp_avg.is_cuda() || !exp_avg_sq.is_cuda()
    {
        return false;
    }
    let Some(param_buf) = param.cloned_cuda_f32_buffer() else {
        return false;
    };
    let Some(grad_buf) = param.cloned_cuda_f32_grad() else {
        return false;
    };
    let Some(exp_avg_buf) = exp_avg.cloned_cuda_f32_buffer() else {
        return false;
    };
    let Some(exp_avg_sq_buf) = exp_avg_sq.cloned_cuda_f32_buffer() else {
        return false;
    };

    let Ok(()) = cuda::adam_update_f32_no_host(
        &param_buf,
        &grad_buf,
        &exp_avg_buf,
        &exp_avg_sq_buf,
        lr,
        beta1,
        beta2,
        bias_correction1,
        bias_correction2,
        eps,
    ) else {
        return false;
    };

    param.replace_cuda_f32_buffer_no_host_sync(param_buf);
    exp_avg.replace_cuda_f32_buffer_no_host_sync(exp_avg_buf);
    exp_avg_sq.replace_cuda_f32_buffer_no_host_sync(exp_avg_sq_buf);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::precision::{PrecisionConfig, set_default_runtime_dtype, with_precision_config};

    #[test]
    fn sgd_default_construction_captures_runtime_dtype_for_future_state() {
        with_precision_config(
            PrecisionConfig {
                parameter_dtype: DType::F32,
                runtime_dtype: DType::BF16,
                allow_parameter_dtype_copies: false,
            },
            || {
                let param = Tensor::parameter(ArrayD::from_elem(IxDyn(&[2]), 1.0));
                let mut opt = SGD::new(vec![param.clone()], 0.1).with_momentum(0.9);
                set_default_runtime_dtype(DType::F32);
                param.add_grad(ArrayD::from_elem(IxDyn(&[2]), 0.5));
                opt.step();

                assert_eq!(opt.state_dtype(), DType::BF16);
                assert_eq!(
                    opt.velocities[0].as_ref().expect("velocity state").dtype(),
                    DType::BF16
                );
            },
        );
    }

    #[test]
    fn sgd_explicit_state_dtype_overrides_global_default() {
        with_precision_config(
            PrecisionConfig {
                parameter_dtype: DType::F32,
                runtime_dtype: DType::BF16,
                allow_parameter_dtype_copies: false,
            },
            || {
                let param = Tensor::parameter(ArrayD::from_elem(IxDyn(&[2]), 1.0));
                let mut opt =
                    SGD::new_with_dtype(vec![param.clone()], 0.1, DType::F32).with_momentum(0.9);
                param.add_grad(ArrayD::from_elem(IxDyn(&[2]), 0.5));
                opt.step();

                assert_eq!(opt.state_dtype(), DType::F32);
                assert_eq!(
                    opt.velocities[0].as_ref().expect("velocity state").dtype(),
                    DType::F32
                );
            },
        );
    }

    #[test]
    fn adam_default_construction_captures_runtime_dtype_for_future_state() {
        with_precision_config(
            PrecisionConfig {
                parameter_dtype: DType::F32,
                runtime_dtype: DType::BF16,
                allow_parameter_dtype_copies: false,
            },
            || {
                let param = Tensor::parameter(ArrayD::from_elem(IxDyn(&[2]), 1.0));
                let mut opt = Adam::new(vec![param.clone()], 0.1);
                set_default_runtime_dtype(DType::F32);
                param.add_grad(ArrayD::from_elem(IxDyn(&[2]), 0.25));
                opt.step();

                assert_eq!(opt.state_dtype(), DType::BF16);
                assert_eq!(
                    opt.exp_avg[0].as_ref().expect("exp_avg").dtype(),
                    DType::BF16
                );
                assert_eq!(
                    opt.exp_avg_sq[0].as_ref().expect("exp_avg_sq").dtype(),
                    DType::BF16
                );
            },
        );
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn sgd_step_uses_cuda_grad_and_keeps_parameter_resident() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        crate::ops::cuda::set_enabled(true);
        let param = Tensor::parameter(
            ArrayD::from_shape_vec(IxDyn(&[4]), vec![1.0, -2.0, 0.5, 3.0])
                .expect("parameter shape"),
        )
        .to_cuda();
        let grad =
            ArrayD::from_shape_vec(IxDyn(&[4]), vec![0.5, -1.0, 2.0, 0.25]).expect("grad shape");
        let grad_buffer =
            crate::ops::cuda::upload_f32(grad.as_slice().expect("contiguous grad")).unwrap();
        param.add_grad_with_cuda_buffer(grad, Some(grad_buffer));

        let mut opt = SGD::new(vec![param.clone()], 0.1);
        opt.step();
        crate::ops::cuda::set_enabled(false);

        assert!(param.is_cuda());
        assert!(param.cloned_cuda_f32_buffer().is_some());
        let values = param.data_ref().iter().copied().collect::<Vec<_>>();
        let expected = [0.95, -1.9, 0.3, 2.975];
        for (got, expect) in values.iter().zip(expected.iter()) {
            assert!(
                (got - expect).abs() <= 1e-6,
                "SGD CUDA update got {got}, expected {expect}"
            );
        }
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn sgd_step_accepts_bf16_parameter_on_cuda_fast_path() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let initial = vec![1.0, -2.0, 0.5, 3.0];
        let grad_values = vec![0.5, -1.0, 2.0, 0.25];

        let cpu_param = Tensor::parameter_with_dtype(
            ArrayD::from_shape_vec(IxDyn(&[4]), initial.clone()).unwrap(),
            DType::BF16,
        );
        cpu_param.add_grad(ArrayD::from_shape_vec(IxDyn(&[4]), grad_values.clone()).unwrap());
        let mut cpu_opt = SGD::new(vec![cpu_param.clone()], 0.1);
        cpu_opt.step();
        let cpu_values = cpu_param.data_ref().iter().copied().collect::<Vec<_>>();

        crate::ops::cuda::set_enabled(true);
        let cuda_param = Tensor::parameter_with_dtype(
            ArrayD::from_shape_vec(IxDyn(&[4]), initial).unwrap(),
            DType::BF16,
        )
        .to_cuda();
        let grad = ArrayD::from_shape_vec(IxDyn(&[4]), grad_values).unwrap();
        let grad_buffer =
            crate::ops::cuda::upload_f32(grad.as_slice().expect("contiguous grad")).unwrap();
        cuda_param.add_grad_with_cuda_buffer(grad, Some(grad_buffer));

        let mut cuda_opt = SGD::new(vec![cuda_param.clone()], 0.1);
        cuda_opt.step();
        crate::ops::cuda::set_enabled(false);

        assert!(cuda_param.is_cuda());
        assert!(cuda_param.cloned_cuda_f32_buffer().is_some());
        assert_eq!(cuda_param.dtype(), DType::F32);

        let cuda_values = cuda_param.data_ref().iter().copied().collect::<Vec<_>>();
        for (got, expect) in cuda_values.iter().zip(cpu_values.iter()) {
            assert!(
                (got - expect).abs() <= 1e-6,
                "BF16 SGD CUDA update got {got}, expected {expect}"
            );
        }
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_sgd_step_clears_consumed_grad() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        crate::ops::cuda::set_enabled(true);
        let param = Tensor::parameter(
            ArrayD::from_shape_vec(IxDyn(&[2]), vec![1.0, 2.0]).expect("parameter shape"),
        )
        .to_cuda();
        let grad = ArrayD::from_shape_vec(IxDyn(&[2]), vec![0.5, -1.0]).expect("grad shape");
        let grad_buffer =
            crate::ops::cuda::upload_f32(grad.as_slice().expect("contiguous grad")).unwrap();
        param.add_grad_with_cuda_buffer(grad, Some(grad_buffer));

        let mut opt = SGD::new(vec![param.clone()], 0.1);
        opt.step();
        let after_first = param.data_ref().iter().copied().collect::<Vec<_>>();
        opt.step();
        let after_second = param.data_ref().iter().copied().collect::<Vec<_>>();
        crate::ops::cuda::set_enabled(false);

        assert_eq!(after_first.len(), after_second.len());
        for (got, expect) in after_second.iter().zip(after_first.iter()) {
            assert!(
                (got - expect).abs() <= 1e-6,
                "CUDA SGD reused a consumed grad: got {got}, expected {expect}"
            );
        }
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn adam_step_uses_cuda_grad_and_keeps_state_resident() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let initial = vec![1.0, -2.0, 0.5, 3.0];
        let grad_values = vec![0.25, -0.5, 1.0, -0.125];

        let cpu_param =
            Tensor::parameter(ArrayD::from_shape_vec(IxDyn(&[4]), initial.clone()).unwrap());
        cpu_param.add_grad(ArrayD::from_shape_vec(IxDyn(&[4]), grad_values.clone()).unwrap());
        let mut cpu_opt = Adam::new_with_dtype(vec![cpu_param.clone()], 0.1, DType::F32);
        cpu_opt.step();
        let cpu_values = cpu_param.data_ref().iter().copied().collect::<Vec<_>>();

        crate::ops::cuda::set_enabled(true);
        let cuda_param =
            Tensor::parameter(ArrayD::from_shape_vec(IxDyn(&[4]), initial).unwrap()).to_cuda();
        let grad = ArrayD::from_shape_vec(IxDyn(&[4]), grad_values).unwrap();
        let grad_buffer =
            crate::ops::cuda::upload_f32(grad.as_slice().expect("contiguous grad")).unwrap();
        cuda_param.add_grad_with_cuda_buffer(grad, Some(grad_buffer));

        let mut cuda_opt = Adam::new_with_dtype(vec![cuda_param.clone()], 0.1, DType::F32);
        cuda_opt.step();
        crate::ops::cuda::set_enabled(false);

        assert!(cuda_param.is_cuda());
        assert!(cuda_param.cloned_cuda_f32_buffer().is_some());
        assert!(
            cuda_opt.exp_avg[0]
                .as_ref()
                .expect("exp_avg")
                .cloned_cuda_f32_buffer()
                .is_some()
        );
        assert!(
            cuda_opt.exp_avg_sq[0]
                .as_ref()
                .expect("exp_avg_sq")
                .cloned_cuda_f32_buffer()
                .is_some()
        );

        let cuda_values = cuda_param.data_ref().iter().copied().collect::<Vec<_>>();
        for (got, expect) in cuda_values.iter().zip(cpu_values.iter()) {
            assert!(
                (got - expect).abs() <= 1e-6,
                "Adam CUDA update got {got}, expected {expect}"
            );
        }
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn adam_step_accepts_bf16_parameter_with_f32_state_on_cuda_fast_path() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let initial = vec![1.0, -2.0, 0.5, 3.0];
        let grad_values = vec![0.25, -0.5, 1.0, -0.125];

        let cpu_param = Tensor::parameter_with_dtype(
            ArrayD::from_shape_vec(IxDyn(&[4]), initial.clone()).unwrap(),
            DType::BF16,
        );
        cpu_param.add_grad(ArrayD::from_shape_vec(IxDyn(&[4]), grad_values.clone()).unwrap());
        let mut cpu_opt = Adam::new_with_dtype(vec![cpu_param.clone()], 0.1, DType::F32);
        cpu_opt.step();
        let cpu_values = cpu_param.data_ref().iter().copied().collect::<Vec<_>>();
        let cpu_exp_avg = cpu_opt.exp_avg[0]
            .as_ref()
            .expect("cpu exp_avg")
            .data_ref()
            .iter()
            .copied()
            .collect::<Vec<_>>();
        let cpu_exp_avg_sq = cpu_opt.exp_avg_sq[0]
            .as_ref()
            .expect("cpu exp_avg_sq")
            .data_ref()
            .iter()
            .copied()
            .collect::<Vec<_>>();

        crate::ops::cuda::set_enabled(true);
        let cuda_param = Tensor::parameter_with_dtype(
            ArrayD::from_shape_vec(IxDyn(&[4]), initial).unwrap(),
            DType::BF16,
        )
        .to_cuda();
        let grad = ArrayD::from_shape_vec(IxDyn(&[4]), grad_values).unwrap();
        let grad_buffer =
            crate::ops::cuda::upload_f32(grad.as_slice().expect("contiguous grad")).unwrap();
        cuda_param.add_grad_with_cuda_buffer(grad, Some(grad_buffer));

        let mut cuda_opt = Adam::new_with_dtype(vec![cuda_param.clone()], 0.1, DType::F32);
        cuda_opt.step();
        crate::ops::cuda::set_enabled(false);

        assert!(cuda_param.is_cuda());
        assert!(cuda_param.cloned_cuda_f32_buffer().is_some());
        assert_eq!(cuda_param.dtype(), DType::F32);
        let cuda_exp_avg_tensor = cuda_opt.exp_avg[0].as_ref().expect("cuda exp_avg");
        let cuda_exp_avg_sq_tensor = cuda_opt.exp_avg_sq[0].as_ref().expect("cuda exp_avg_sq");
        assert!(cuda_exp_avg_tensor.is_cuda());
        assert!(cuda_exp_avg_sq_tensor.is_cuda());
        assert!(cuda_exp_avg_tensor.cloned_cuda_f32_buffer().is_some());
        assert!(cuda_exp_avg_sq_tensor.cloned_cuda_f32_buffer().is_some());

        let cuda_values = cuda_param.data_ref().iter().copied().collect::<Vec<_>>();
        let cuda_exp_avg = cuda_exp_avg_tensor
            .data_ref()
            .iter()
            .copied()
            .collect::<Vec<_>>();
        let cuda_exp_avg_sq = cuda_exp_avg_sq_tensor
            .data_ref()
            .iter()
            .copied()
            .collect::<Vec<_>>();
        for (got, expect) in cuda_values.iter().zip(cpu_values.iter()) {
            assert!(
                (got - expect).abs() <= 1e-6,
                "BF16 Adam CUDA update got {got}, expected {expect}"
            );
        }
        for (got, expect) in cuda_exp_avg.iter().zip(cpu_exp_avg.iter()) {
            assert!(
                (got - expect).abs() <= 1e-6,
                "BF16 Adam CUDA exp_avg got {got}, expected {expect}"
            );
        }
        for (got, expect) in cuda_exp_avg_sq.iter().zip(cpu_exp_avg_sq.iter()) {
            assert!(
                (got - expect).abs() <= 1e-6,
                "BF16 Adam CUDA exp_avg_sq got {got}, expected {expect}"
            );
        }
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn sgd_momentum_step_uses_cuda_grad_and_keeps_velocity_resident() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let initial = vec![1.0, -2.0, 0.5, 3.0];
        let grad_values = vec![0.5, -1.0, 2.0, 0.25];

        let cpu_param =
            Tensor::parameter(ArrayD::from_shape_vec(IxDyn(&[4]), initial.clone()).unwrap());
        cpu_param.add_grad(ArrayD::from_shape_vec(IxDyn(&[4]), grad_values.clone()).unwrap());
        let mut cpu_opt =
            SGD::new_with_dtype(vec![cpu_param.clone()], 0.1, DType::F32).with_momentum(0.9);
        cpu_opt.step();
        let cpu_values = cpu_param.data_ref().iter().copied().collect::<Vec<_>>();
        let cpu_velocity = cpu_opt.velocities[0]
            .as_ref()
            .expect("cpu velocity")
            .data_ref()
            .iter()
            .copied()
            .collect::<Vec<_>>();

        crate::ops::cuda::set_enabled(true);
        let cuda_param =
            Tensor::parameter(ArrayD::from_shape_vec(IxDyn(&[4]), initial).unwrap()).to_cuda();
        let grad = ArrayD::from_shape_vec(IxDyn(&[4]), grad_values).unwrap();
        let grad_buffer =
            crate::ops::cuda::upload_f32(grad.as_slice().expect("contiguous grad")).unwrap();
        cuda_param.add_grad_with_cuda_buffer(grad, Some(grad_buffer));

        let mut cuda_opt =
            SGD::new_with_dtype(vec![cuda_param.clone()], 0.1, DType::F32).with_momentum(0.9);
        cuda_opt.step();
        crate::ops::cuda::set_enabled(false);

        assert!(cuda_param.is_cuda());
        assert!(cuda_param.cloned_cuda_f32_buffer().is_some());
        let cuda_velocity_tensor = cuda_opt.velocities[0].as_ref().expect("cuda velocity");
        assert!(cuda_velocity_tensor.is_cuda());
        assert!(cuda_velocity_tensor.cloned_cuda_f32_buffer().is_some());

        let cuda_values = cuda_param.data_ref().iter().copied().collect::<Vec<_>>();
        let cuda_velocity = cuda_velocity_tensor
            .data_ref()
            .iter()
            .copied()
            .collect::<Vec<_>>();
        for (got, expect) in cuda_values.iter().zip(cpu_values.iter()) {
            assert!(
                (got - expect).abs() <= 1e-6,
                "SGD momentum CUDA update got {got}, expected {expect}"
            );
        }
        for (got, expect) in cuda_velocity.iter().zip(cpu_velocity.iter()) {
            assert!(
                (got - expect).abs() <= 1e-6,
                "SGD momentum CUDA velocity got {got}, expected {expect}"
            );
        }
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn sgd_momentum_step_accepts_bf16_parameter_on_cuda_fast_path() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let initial = vec![1.0, -2.0, 0.5, 3.0];
        let grad_values = vec![0.5, -1.0, 2.0, 0.25];

        let cpu_param = Tensor::parameter_with_dtype(
            ArrayD::from_shape_vec(IxDyn(&[4]), initial.clone()).unwrap(),
            DType::BF16,
        );
        cpu_param.add_grad(ArrayD::from_shape_vec(IxDyn(&[4]), grad_values.clone()).unwrap());
        let mut cpu_opt =
            SGD::new_with_dtype(vec![cpu_param.clone()], 0.1, DType::F32).with_momentum(0.9);
        cpu_opt.step();
        let cpu_values = cpu_param.data_ref().iter().copied().collect::<Vec<_>>();
        let cpu_velocity = cpu_opt.velocities[0]
            .as_ref()
            .expect("cpu velocity")
            .data_ref()
            .iter()
            .copied()
            .collect::<Vec<_>>();

        crate::ops::cuda::set_enabled(true);
        let cuda_param = Tensor::parameter_with_dtype(
            ArrayD::from_shape_vec(IxDyn(&[4]), initial).unwrap(),
            DType::BF16,
        )
        .to_cuda();
        let grad = ArrayD::from_shape_vec(IxDyn(&[4]), grad_values).unwrap();
        let grad_buffer =
            crate::ops::cuda::upload_f32(grad.as_slice().expect("contiguous grad")).unwrap();
        cuda_param.add_grad_with_cuda_buffer(grad, Some(grad_buffer));

        let mut cuda_opt =
            SGD::new_with_dtype(vec![cuda_param.clone()], 0.1, DType::F32).with_momentum(0.9);
        cuda_opt.step();
        crate::ops::cuda::set_enabled(false);

        assert!(cuda_param.is_cuda());
        assert!(cuda_param.cloned_cuda_f32_buffer().is_some());
        assert_eq!(cuda_param.dtype(), DType::F32);
        let cuda_velocity_tensor = cuda_opt.velocities[0].as_ref().expect("cuda velocity");
        assert!(cuda_velocity_tensor.is_cuda());
        assert!(cuda_velocity_tensor.cloned_cuda_f32_buffer().is_some());

        let cuda_values = cuda_param.data_ref().iter().copied().collect::<Vec<_>>();
        let cuda_velocity = cuda_velocity_tensor
            .data_ref()
            .iter()
            .copied()
            .collect::<Vec<_>>();
        for (got, expect) in cuda_values.iter().zip(cpu_values.iter()) {
            assert!(
                (got - expect).abs() <= 1e-6,
                "BF16 SGD momentum CUDA update got {got}, expected {expect}"
            );
        }
        for (got, expect) in cuda_velocity.iter().zip(cpu_velocity.iter()) {
            assert!(
                (got - expect).abs() <= 1e-6,
                "BF16 SGD momentum CUDA velocity got {got}, expected {expect}"
            );
        }
    }

    #[test]
    #[should_panic(expected = "Optimizer state currently only supports floating dtypes")]
    fn optimizer_state_rejects_integer_dtype() {
        let _ = SGD::new_with_dtype(Vec::new(), 0.1, DType::I8);
    }
}
