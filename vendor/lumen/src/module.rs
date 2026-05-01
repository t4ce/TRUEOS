// src/module.rs
use crate::autograd::{Device, Tensor, TensorRawData, set_inference_mode};
use crate::precision::{DType, ParameterQuantization, default_parameter_quantization};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufReader, BufWriter};

#[derive(Serialize, Deserialize)]
pub struct CheckpointTensor {
    pub shape: Vec<usize>,
    pub dtype: DType,
    pub raw: TensorRawData,
}

#[derive(Serialize, Deserialize)]
pub struct ModelCheckpoint {
    pub params: Vec<CheckpointTensor>,
}

pub trait Module {
    fn forward(&self, input: Tensor) -> Tensor;
    fn parameters(&self) -> Vec<Tensor>;

    // 训练模式：允许构图
    fn train_mode(&mut self) {
        set_inference_mode(false);
    }

    // 推理模式：禁止构图（等价 no_grad）
    fn eval_mode(&mut self) {
        set_inference_mode(true);
    }

    fn save(&self, path: &str) -> std::io::Result<()> {
        let params = self.parameters();
        let mut data_list = Vec::new();
        for p in params {
            let (shape, dtype, raw) = p.export_raw();
            data_list.push(CheckpointTensor { shape, dtype, raw });
        }
        let checkpoint = ModelCheckpoint { params: data_list };
        let file = File::create(path)?;
        let writer = BufWriter::new(file);
        bincode::serialize_into(writer, &checkpoint)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        println!("Model saved to {} (Binary format)", path);
        Ok(())
    }

    fn load(&self, path: &str) -> std::io::Result<()> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let checkpoint: ModelCheckpoint = bincode::deserialize_from(reader)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

        let my_params = self.parameters();
        if checkpoint.params.len() != my_params.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "Load failed: parameter count mismatch (checkpoint={}, module={})",
                    checkpoint.params.len(),
                    my_params.len()
                ),
            ));
        }

        for (param, tensor_blob) in my_params.iter().zip(checkpoint.params.into_iter()) {
            param
                .import_raw(tensor_blob.shape, tensor_blob.dtype, tensor_blob.raw)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        }

        println!("Model loaded from {} (Binary format)", path);
        Ok(())
    }

    fn cast_parameters(&self, dtype: DType) {
        for param in self.parameters() {
            param.cast_inplace(dtype);
        }
    }

    fn quantize_parameters(&self, dtype: DType) {
        assert!(
            dtype.is_integer(),
            "quantize_parameters currently expects integer dtype, got {:?}",
            dtype
        );
        for param in self.parameters() {
            param.quantize_inplace(dtype);
        }
    }

    fn dequantize_parameters(&self, dtype: DType) {
        assert!(
            dtype.is_float(),
            "dequantize_parameters currently expects floating dtype, got {:?}",
            dtype
        );
        for param in self.parameters() {
            param.dequantize_inplace(dtype);
        }
    }

    fn apply_parameter_quantization(&self, quantization: ParameterQuantization) {
        if !quantization.is_enabled() {
            return;
        }
        for param in self.parameters() {
            param.quantize_inplace_with_quantization(quantization);
        }
    }

    fn apply_default_parameter_quantization(&self) {
        self.apply_parameter_quantization(default_parameter_quantization());
    }

    fn to_device(&self, device: Device) {
        for param in self.parameters() {
            param.to_device_inplace(device);
        }
    }

    fn to_cpu(&self) {
        self.to_device(Device::Cpu);
    }

    fn to_cuda(&self) {
        self.to_device(Device::Cuda);
    }
}

pub struct Sequential {
    layers: Vec<Box<dyn Module>>,
}

impl Sequential {
    pub fn new(layers: Vec<Box<dyn Module>>) -> Self {
        Sequential { layers }
    }
}

impl Module for Sequential {
    fn forward(&self, mut input: Tensor) -> Tensor {
        for layer in &self.layers {
            input = layer.forward(input);
        }
        input
    }

    fn parameters(&self) -> Vec<Tensor> {
        self.layers.iter().flat_map(|l| l.parameters()).collect()
    }

    fn train_mode(&mut self) {
        // 先设置全局模式，再递归
        set_inference_mode(false);
        for l in &mut self.layers {
            l.train_mode();
        }
    }

    fn eval_mode(&mut self) {
        // 先设置全局模式，再递归
        set_inference_mode(true);
        for l in &mut self.layers {
            l.eval_mode();
        }
    }
}

#[macro_export]
macro_rules! sequential {
    ($($layer:expr),* $(,)?) => {
        $crate::module::Sequential::new(vec![
            $(Box::new($layer)),*
        ])
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "cuda")]
    use crate::autograd::set_strict_device_execution;
    use crate::autograd::{Tensor, no_grad};
    use crate::layers::basic::linear::Linear;
    use crate::loss::MSELoss;
    #[cfg(feature = "cuda")]
    use crate::ops::arithmetic::sum;
    use crate::precision::{
        DType, ParameterQuantization, PrecisionConfig, with_parameter_quantization,
        with_precision_config,
    };
    use ndarray::{Array, ArrayD, IxDyn};
    use ndarray_rand::{
        RandomExt,
        rand::{SeedableRng, rngs::StdRng},
        rand_distr::Uniform,
    };
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct DummyModule {
        params: Vec<Tensor>,
    }

    impl Module for DummyModule {
        fn forward(&self, input: Tensor) -> Tensor {
            input
        }

        fn parameters(&self) -> Vec<Tensor> {
            self.params.clone()
        }
    }

    fn temp_checkpoint_path(prefix: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}_{stamp}.ckpt"))
    }

    fn random_array(shape: &[usize], rng: &mut StdRng, low: f32, high: f32) -> ArrayD<f32> {
        Array::random_using(IxDyn(shape), Uniform::new(low, high), rng).into_dyn()
    }

    fn assign_random_parameter_values(param: &Tensor, dtype: DType, rng: &mut StdRng) {
        let shape = param.shape_vec();
        let values = random_array(&shape, rng, -0.75, 0.75);
        param.set_array_f32_with_dtype(values, dtype);
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn module_to_cuda_moves_all_parameters() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let module = DummyModule {
            params: vec![
                Tensor::parameter(ArrayD::from_shape_vec(IxDyn(&[2]), vec![1.0, 2.0]).unwrap()),
                Tensor::parameter(ArrayD::from_shape_vec(IxDyn(&[2]), vec![3.0, 4.0]).unwrap()),
            ],
        };

        module.to_cuda();
        for param in module.parameters() {
            assert_eq!(param.device(), crate::autograd::Device::Cuda);
        }

        module.to_cpu();
        for param in module.parameters() {
            assert_eq!(param.device(), crate::autograd::Device::Cpu);
        }
    }

    fn scalar_mse_for_linear(linear: &Linear, input: &Tensor, target: &Tensor) -> f32 {
        no_grad(|| {
            let output = linear.forward(input.clone());
            let loss = MSELoss::apply(&output, target);
            loss.data_ref()
                .first()
                .copied()
                .expect("mse loss should be scalar")
        })
    }

    fn assert_close(name: &str, lhs: f32, rhs: f32, abs_tol: f32, rel_tol: f32) {
        let abs = (lhs - rhs).abs();
        let scale = lhs.abs().max(rhs.abs()).max(1.0);
        assert!(
            abs <= abs_tol || abs / scale <= rel_tol,
            "{name} mismatch: lhs={lhs}, rhs={rhs}, abs_diff={abs}, abs_tol={abs_tol}, rel_tol={rel_tol}",
        );
    }

    #[test]
    fn checkpoint_preserves_parameter_dtype_and_values() {
        with_precision_config(
            PrecisionConfig {
                parameter_dtype: DType::BF16,
                runtime_dtype: DType::F32,
                allow_parameter_dtype_copies: false,
            },
            || {
                let param = Tensor::parameter(
                    ArrayD::from_shape_vec(IxDyn(&[2, 2]), vec![1.0, -2.0, 3.5, 4.25]).unwrap(),
                );
                let module = DummyModule {
                    params: vec![param.clone()],
                };

                let restore = Tensor::parameter(ArrayD::zeros(IxDyn(&[2, 2])));
                restore.cast_inplace(DType::F32);
                let restore_module = DummyModule {
                    params: vec![restore.clone()],
                };

                let stamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos();
                let path = std::env::temp_dir().join(format!("lumen_precision_{stamp}.ckpt"));
                let path_str = path.to_string_lossy().to_string();

                module.save(&path_str).unwrap();
                restore_module.load(&path_str).unwrap();

                assert_eq!(restore.dtype(), DType::BF16);
                let loaded = restore.data();
                let expected = param.data();
                assert_eq!(loaded.shape(), expected.shape());
                for (&lhs, &rhs) in loaded.iter().zip(expected.iter()) {
                    assert!((lhs - rhs).abs() <= 0.02, "lhs={lhs}, rhs={rhs}");
                }

                let _ = fs::remove_file(path);
            },
        );
    }

    #[test]
    fn checkpoint_preserves_i8_parameter_dtype_and_values() {
        with_precision_config(
            PrecisionConfig {
                parameter_dtype: DType::I8,
                runtime_dtype: DType::F32,
                allow_parameter_dtype_copies: false,
            },
            || {
                let param = Tensor::parameter(
                    ArrayD::from_shape_vec(IxDyn(&[2, 2]), vec![1.0, -2.0, 3.5, 4.25]).unwrap(),
                );
                let module = DummyModule {
                    params: vec![param.clone()],
                };

                let restore = Tensor::parameter(ArrayD::zeros(IxDyn(&[2, 2])));
                restore.cast_inplace(DType::F32);
                let restore_module = DummyModule {
                    params: vec![restore.clone()],
                };

                let stamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos();
                let path = std::env::temp_dir().join(format!("lumen_precision_i8_{stamp}.ckpt"));
                let path_str = path.to_string_lossy().to_string();

                module.save(&path_str).unwrap();
                restore_module.load(&path_str).unwrap();

                assert_eq!(restore.dtype(), DType::I8);
                let loaded = restore.data();
                let expected = param.data();
                assert_eq!(loaded.shape(), expected.shape());
                for (&lhs, &rhs) in loaded.iter().zip(expected.iter()) {
                    assert!((lhs - rhs).abs() <= 1e-6, "lhs={lhs}, rhs={rhs}");
                }

                let _ = fs::remove_file(path);
            },
        );
    }

    #[test]
    fn module_can_apply_default_parameter_quantization() {
        let param = Tensor::parameter_with_dtype(
            ArrayD::from_shape_vec(IxDyn(&[2]), vec![1.0, -2.0]).unwrap(),
            DType::F32,
        );
        let module = DummyModule {
            params: vec![param.clone()],
        };

        with_parameter_quantization(ParameterQuantization::Int8, || {
            module.apply_default_parameter_quantization();
        });

        assert_eq!(param.dtype(), DType::I8);
    }

    #[test]
    fn module_can_apply_explicit_parameter_quantization() {
        let param = Tensor::parameter_with_dtype(
            ArrayD::from_shape_vec(IxDyn(&[2]), vec![1.0, -2.0]).unwrap(),
            DType::F32,
        );
        let module = DummyModule {
            params: vec![param.clone()],
        };

        module.apply_parameter_quantization(ParameterQuantization::Int8);

        assert_eq!(param.dtype(), DType::I8);
    }

    #[test]
    fn module_quantize_and_dequantize_parameters_are_explicit() {
        let param = Tensor::parameter_with_dtype(
            ArrayD::from_shape_vec(IxDyn(&[2]), vec![1.0, -2.0]).unwrap(),
            DType::F32,
        );
        let module = DummyModule {
            params: vec![param.clone()],
        };

        module.quantize_parameters(DType::I8);
        assert_eq!(param.dtype(), DType::I8);

        module.dequantize_parameters(DType::F32);
        assert_eq!(param.dtype(), DType::F32);
    }

    #[test]
    fn randomized_linear_backward_matches_finite_difference() {
        for seed in [7u64, 19, 42] {
            let linear = Linear::new_with_dtype(4, 2, DType::F32);
            let mut rng = StdRng::seed_from_u64(seed);

            linear
                .weight
                .set_array_f32_with_dtype(random_array(&[2, 4], &mut rng, -0.5, 0.5), DType::F32);
            linear
                .bias
                .as_ref()
                .expect("linear bias")
                .set_array_f32_with_dtype(random_array(&[2], &mut rng, -0.25, 0.25), DType::F32);

            let input = Tensor::from_array_no_grad(random_array(&[3, 4], &mut rng, -1.0, 1.0));
            let target = Tensor::from_array_no_grad(random_array(&[3, 2], &mut rng, -1.0, 1.0));

            linear.weight.zero_grad();
            linear.bias.as_ref().expect("linear bias").zero_grad();

            let loss = MSELoss::apply(&linear.forward(input.clone()), &target);
            let loss_value = loss
                .data_ref()
                .first()
                .copied()
                .expect("loss should be scalar");
            assert!(
                loss_value.is_finite(),
                "loss should be finite for seed {seed}"
            );
            loss.backward();

            let analytic_w = linear
                .weight
                .grad()
                .expect("weight grad should exist after backward");
            let analytic_b = linear
                .bias
                .as_ref()
                .expect("linear bias")
                .grad()
                .expect("bias grad should exist after backward");

            let eps = 1e-3f32;
            for out_idx in 0..2 {
                for in_idx in 0..4 {
                    let original = {
                        let weight = linear.weight.data_ref();
                        weight[[out_idx, in_idx]]
                    };

                    {
                        let mut weight = linear.weight.data_mut();
                        weight[[out_idx, in_idx]] = original + eps;
                    }
                    let loss_plus = scalar_mse_for_linear(&linear, &input, &target);

                    {
                        let mut weight = linear.weight.data_mut();
                        weight[[out_idx, in_idx]] = original - eps;
                    }
                    let loss_minus = scalar_mse_for_linear(&linear, &input, &target);

                    {
                        let mut weight = linear.weight.data_mut();
                        weight[[out_idx, in_idx]] = original;
                    }

                    let numeric = (loss_plus - loss_minus) / (2.0 * eps);
                    let analytic = analytic_w[[out_idx, in_idx]];
                    assert_close(
                        &format!("weight_grad[{}, {}] seed {}", out_idx, in_idx, seed),
                        numeric,
                        analytic,
                        2e-3,
                        1e-2,
                    );
                }
            }

            for out_idx in 0..2 {
                let bias = linear.bias.as_ref().expect("linear bias");
                let original = {
                    let bias_data = bias.data_ref();
                    bias_data[[out_idx]]
                };

                {
                    let mut bias_data = bias.data_mut();
                    bias_data[[out_idx]] = original + eps;
                }
                let loss_plus = scalar_mse_for_linear(&linear, &input, &target);

                {
                    let mut bias_data = bias.data_mut();
                    bias_data[[out_idx]] = original - eps;
                }
                let loss_minus = scalar_mse_for_linear(&linear, &input, &target);

                {
                    let mut bias_data = bias.data_mut();
                    bias_data[[out_idx]] = original;
                }

                let numeric = (loss_plus - loss_minus) / (2.0 * eps);
                let analytic = analytic_b[[out_idx]];
                assert_close(
                    &format!("bias_grad[{}] seed {}", out_idx, seed),
                    numeric,
                    analytic,
                    2e-3,
                    1e-2,
                );
            }
        }
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn linear_sum_backward_matches_cpu_reference_in_strict_cuda_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let linear_cpu = Linear::new_with_dtype(4, 2, DType::F32);
        linear_cpu.weight.set_array_f32_with_dtype(
            ArrayD::from_shape_vec(
                IxDyn(&[2, 4]),
                vec![0.5, -0.25, 1.0, 0.75, -1.0, 0.5, 0.25, -0.5],
            )
            .unwrap(),
            DType::F32,
        );
        linear_cpu
            .bias
            .as_ref()
            .expect("linear bias")
            .set_array_f32_with_dtype(
                ArrayD::from_shape_vec(IxDyn(&[2]), vec![0.1, -0.2]).unwrap(),
                DType::F32,
            );

        let linear_cuda = Linear::new_with_dtype(4, 2, DType::F32);
        linear_cuda
            .weight
            .set_array_f32_with_dtype(linear_cpu.weight.data_ref().to_owned(), DType::F32);
        linear_cuda
            .bias
            .as_ref()
            .expect("linear bias")
            .set_array_f32_with_dtype(
                linear_cpu
                    .bias
                    .as_ref()
                    .expect("linear bias")
                    .data_ref()
                    .to_owned(),
                DType::F32,
            );
        linear_cuda.to_cuda();

        let input_data = ArrayD::from_shape_vec(
            IxDyn(&[3, 4]),
            vec![
                1.0, -2.0, 0.5, 3.0, -1.0, 2.0, 0.0, 0.5, 0.25, -0.75, 1.5, -1.25,
            ],
        )
        .unwrap();
        let input_cpu = Tensor::from_data_with_grad_flag(input_data.clone(), true);
        let input_cuda = Tensor::from_data_with_grad_flag(input_data, true).to_cuda();

        crate::ops::cuda::set_enabled(true);
        set_strict_device_execution(true);
        let loss_cuda = sum(&linear_cuda.forward(input_cuda.clone()));
        loss_cuda.backward();
        set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);

        let loss_cpu = sum(&linear_cpu.forward(input_cpu.clone()));
        loss_cpu.backward();

        for (got, expect) in input_cuda
            .grad()
            .expect("cuda input grad")
            .iter()
            .zip(input_cpu.grad().expect("cpu input grad").iter())
        {
            assert!(
                (got - expect).abs() < 1e-4,
                "input grad got {got}, expect {expect}"
            );
        }
        for (got, expect) in linear_cuda
            .weight
            .grad()
            .expect("cuda weight grad")
            .iter()
            .zip(linear_cpu.weight.grad().expect("cpu weight grad").iter())
        {
            assert!(
                (got - expect).abs() < 1e-4,
                "weight grad got {got}, expect {expect}"
            );
        }
        for (got, expect) in linear_cuda
            .bias
            .as_ref()
            .expect("cuda bias")
            .grad()
            .expect("cuda bias grad")
            .iter()
            .zip(
                linear_cpu
                    .bias
                    .as_ref()
                    .expect("cpu bias")
                    .grad()
                    .expect("cpu bias grad")
                    .iter(),
            )
        {
            assert!(
                (got - expect).abs() < 1e-4,
                "bias grad got {got}, expect {expect}"
            );
        }
    }

    #[test]
    fn randomized_linear_checkpoint_roundtrip_restores_parameters_and_outputs() {
        for (dtype, tol, seed) in [
            (DType::F32, 1e-6f32, 101u64),
            (DType::BF16, 2e-2f32, 202u64),
            (DType::I8, 1e-6f32, 303u64),
        ] {
            let mut rng = StdRng::seed_from_u64(seed);
            let source = Linear::new_with_dtype(5, 3, dtype);
            let restore = Linear::new_with_dtype(5, 3, DType::F32);

            assign_random_parameter_values(&source.weight, dtype, &mut rng);
            assign_random_parameter_values(
                source.bias.as_ref().expect("source bias"),
                dtype,
                &mut rng,
            );
            assign_random_parameter_values(&restore.weight, DType::F32, &mut rng);
            assign_random_parameter_values(
                restore.bias.as_ref().expect("restore bias"),
                DType::F32,
                &mut rng,
            );

            let input = Tensor::from_array_no_grad(random_array(&[4, 5], &mut rng, -1.0, 1.0));
            let src_out = no_grad(|| source.forward(input.clone())).data();

            let path = temp_checkpoint_path(&format!("lumen_random_linear_{seed}"));
            let path_str = path.to_string_lossy().to_string();

            source.save(&path_str).unwrap();
            restore.load(&path_str).unwrap();

            let src_params = source.parameters();
            let restored_params = restore.parameters();
            assert_eq!(src_params.len(), restored_params.len());
            for (index, (src_param, restored_param)) in
                src_params.iter().zip(restored_params.iter()).enumerate()
            {
                assert_eq!(
                    restored_param.dtype(),
                    src_param.dtype(),
                    "parameter {} dtype should roundtrip",
                    index
                );
                let src_vals = src_param.data();
                let restored_vals = restored_param.data();
                assert_eq!(src_vals.shape(), restored_vals.shape());
                for (elem_idx, (&lhs, &rhs)) in
                    src_vals.iter().zip(restored_vals.iter()).enumerate()
                {
                    assert!(
                        (lhs - rhs).abs() <= tol,
                        "dtype {:?} parameter {} element {} mismatch: lhs={}, rhs={}, tol={}",
                        dtype,
                        index,
                        elem_idx,
                        lhs,
                        rhs,
                        tol
                    );
                }
            }

            let restored_out = no_grad(|| restore.forward(input.clone())).data();
            for (elem_idx, (&lhs, &rhs)) in src_out.iter().zip(restored_out.iter()).enumerate() {
                assert!(
                    (lhs - rhs).abs() <= tol * 2.0 + 1e-6,
                    "dtype {:?} output element {} mismatch: lhs={}, rhs={}, tol={}",
                    dtype,
                    elem_idx,
                    lhs,
                    rhs,
                    tol * 2.0 + 1e-6
                );
            }

            let _ = fs::remove_file(path);
        }
    }

    #[test]
    fn load_returns_error_on_parameter_count_mismatch() {
        let source = DummyModule {
            params: vec![
                Tensor::parameter_with_dtype(ArrayD::zeros(IxDyn(&[2])), DType::F32),
                Tensor::parameter_with_dtype(ArrayD::zeros(IxDyn(&[2])), DType::F32),
            ],
        };
        let restore = DummyModule {
            params: vec![Tensor::parameter_with_dtype(
                ArrayD::zeros(IxDyn(&[2])),
                DType::F32,
            )],
        };

        let path = temp_checkpoint_path("lumen_bad_count");
        let path_str = path.to_string_lossy().to_string();
        source.save(&path_str).unwrap();

        let err = restore
            .load(&path_str)
            .expect_err("parameter count mismatch should return an error");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert!(
            err.to_string().contains("parameter count mismatch"),
            "unexpected error: {err}"
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn load_returns_error_on_checkpoint_dtype_payload_mismatch() {
        let restore = DummyModule {
            params: vec![Tensor::parameter_with_dtype(
                ArrayD::zeros(IxDyn(&[2])),
                DType::F32,
            )],
        };

        let checkpoint = ModelCheckpoint {
            params: vec![CheckpointTensor {
                shape: vec![2],
                dtype: DType::BF16,
                raw: TensorRawData::F32(vec![1.0, -2.0]),
            }],
        };
        let path = temp_checkpoint_path("lumen_bad_dtype");
        let path_str = path.to_string_lossy().to_string();
        let file = File::create(&path).unwrap();
        let writer = BufWriter::new(file);
        bincode::serialize_into(writer, &checkpoint).unwrap();

        let err = restore
            .load(&path_str)
            .expect_err("dtype payload mismatch should return an error");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert!(
            err.to_string().contains("dtype BF16 with f32 data"),
            "unexpected error: {err}"
        );

        let _ = fs::remove_file(path);
    }
}
