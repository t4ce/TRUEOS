use crate::autograd::Tensor;
use crate::precision::{DType, default_parameter_dtype, default_parameter_quantization};
use ndarray::{Array, ArrayD, IxDyn};
use ndarray_rand::RandomExt;
use ndarray_rand::rand_distr::{Normal, Uniform};
use std::sync::atomic::{AtomicU8, Ordering};

pub enum InitType {
    XavierUniform, // For Tanh/Sigmoid (Glorot)
    KaimingNormal, // For ReLU/GELU (He)
    Zeros,         // For Bias
    Ones,          // For LayerNorm/RMSNorm weights
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParameterInitMode {
    Full = 0,
    Placeholder = 1,
}

impl ParameterInitMode {
    #[inline]
    fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Placeholder,
            _ => Self::Full,
        }
    }
}

static PARAMETER_INIT_MODE: AtomicU8 = AtomicU8::new(ParameterInitMode::Full as u8);

pub struct ParameterInitModeGuard {
    previous: ParameterInitMode,
}

#[inline]
pub fn parameter_init_mode() -> ParameterInitMode {
    ParameterInitMode::from_u8(PARAMETER_INIT_MODE.load(Ordering::Relaxed))
}

#[inline]
pub fn set_parameter_init_mode(mode: ParameterInitMode) {
    PARAMETER_INIT_MODE.store(mode as u8, Ordering::Relaxed);
}

#[inline]
pub fn parameter_init_guard(mode: ParameterInitMode) -> ParameterInitModeGuard {
    let previous = parameter_init_mode();
    set_parameter_init_mode(mode);
    ParameterInitModeGuard { previous }
}

#[inline]
pub fn with_parameter_init_mode<R>(mode: ParameterInitMode, f: impl FnOnce() -> R) -> R {
    let _guard = parameter_init_guard(mode);
    f()
}

impl Drop for ParameterInitModeGuard {
    fn drop(&mut self) {
        set_parameter_init_mode(self.previous);
    }
}

#[inline]
fn build_init_data(shape: &[usize], init_type: InitType) -> ArrayD<f32> {
    let shape_dyn = IxDyn(shape);
    if parameter_init_mode() == ParameterInitMode::Placeholder {
        return ArrayD::zeros(shape_dyn);
    }

    match init_type {
        InitType::Zeros => ArrayD::zeros(shape_dyn.clone()),

        InitType::Ones => ArrayD::ones(shape_dyn.clone()),

        InitType::XavierUniform => {
            let fan_in = shape[0] as f32;
            let fan_out = if shape.len() > 1 { shape[1] } else { shape[0] } as f32;
            let limit = (6.0 / (fan_in + fan_out)).sqrt();
            Array::random(shape_dyn.clone(), Uniform::new(-limit, limit)).into_dyn()
        }

        InitType::KaimingNormal => {
            let fan_in = shape[0] as f32;
            let std = (2.0 / fan_in).sqrt();
            Array::random(shape_dyn, Normal::new(0.0, std).unwrap()).into_dyn()
        }
    }
}

pub fn tensor_init(shape: Vec<usize>, init_type: InitType) -> Tensor {
    if parameter_init_mode() == ParameterInitMode::Placeholder {
        let quantization = default_parameter_quantization();
        if quantization.is_enabled() {
            return Tensor::parameter_placeholder_with_quantization(shape.as_slice(), quantization);
        }
        return Tensor::parameter_placeholder_with_dtype(
            shape.as_slice(),
            default_parameter_dtype(),
        );
    }
    Tensor::parameter(build_init_data(shape.as_slice(), init_type))
}

pub fn tensor_init_with_dtype(shape: Vec<usize>, init_type: InitType, dtype: DType) -> Tensor {
    if parameter_init_mode() == ParameterInitMode::Placeholder {
        let quantization = default_parameter_quantization();
        if quantization.is_enabled() && quantization.storage_dtype() == Some(dtype) {
            return Tensor::parameter_placeholder_with_quantization(shape.as_slice(), quantization);
        }
        return Tensor::parameter_placeholder_with_dtype(shape.as_slice(), dtype);
    }
    Tensor::parameter_with_dtype(build_init_data(shape.as_slice(), init_type), dtype)
}

#[cfg(test)]
mod tests {
    use super::{InitType, ParameterInitMode, tensor_init, with_parameter_init_mode};
    use crate::precision::{DType, PrecisionConfig, with_precision_config};

    #[test]
    fn placeholder_parameter_init_overrides_random_initializers() {
        let tensor = with_parameter_init_mode(ParameterInitMode::Placeholder, || {
            tensor_init(vec![2, 3], InitType::KaimingNormal)
        });
        assert!(tensor.data_ref().iter().all(|&v| v == 0.0));
    }

    #[test]
    fn placeholder_parameter_init_follows_global_parameter_dtype() {
        with_precision_config(
            PrecisionConfig {
                parameter_dtype: DType::I8,
                runtime_dtype: DType::F32,
                allow_parameter_dtype_copies: false,
            },
            || {
                let tensor = with_parameter_init_mode(ParameterInitMode::Placeholder, || {
                    tensor_init(vec![2, 3], InitType::KaimingNormal)
                });
                assert_eq!(tensor.dtype(), DType::I8);
            },
        );
    }
}
