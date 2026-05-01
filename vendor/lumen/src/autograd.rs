// src/autograd.rs
use crate::precision::{
    DType, ParameterQuantization, allow_parameter_dtype_copies, default_parameter_dtype,
    default_parameter_quantization,
};
use half::{bf16, f16, slice::HalfFloatSliceExt};
use ndarray::prelude::*;
pub use ndarray::{ArcArray, IxDyn};
use serde::{Deserialize, Serialize};
use std::cell::{Cell, Ref, RefCell, RefMut};
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
thread_local! {
    static INFERENCE_MODE: Cell<bool> = const { Cell::new(false) };
    static NO_GRAD_DEPTH: Cell<usize> = const { Cell::new(0) };
    static F32_COMPUTE_SCRATCH: RefCell<Vec<f32>> = const { RefCell::new(Vec::new()) };
}

pub struct NoGradGuard {
    _priv: (),
}

impl NoGradGuard {
    pub fn enter() -> Self {
        NO_GRAD_DEPTH.with(|depth| depth.set(depth.get() + 1));
        Self { _priv: () }
    }
}

impl Drop for NoGradGuard {
    fn drop(&mut self) {
        NO_GRAD_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

// 开/关 全局推理模式（eval_mode/train_mode 可调用它）
pub fn set_inference_mode(on: bool) {
    INFERENCE_MODE.with(|flag| flag.set(on));
}

#[inline]
pub fn is_inference_mode() -> bool {
    INFERENCE_MODE.with(|flag| flag.get())
}

// no_grad 的判定：
// - 在 NoGradGuard 作用域内为 true
// - 或者处于 inference_mode 为 true
#[inline]
pub fn is_no_grad() -> bool {
    NO_GRAD_DEPTH.with(|depth| depth.get() > 0) || is_inference_mode()
}

// 便利封装：no_grad(|| { ... })
pub fn no_grad<R>(f: impl FnOnce() -> R) -> R {
    let _g = NoGradGuard::enter();
    f()
}

fn env_strict_device_execution() -> bool {
    std::env::var("LUMEN_STRICT_DEVICE")
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn strict_device_execution_toggle() -> &'static AtomicBool {
    static STRICT_DEVICE_EXECUTION: OnceLock<AtomicBool> = OnceLock::new();
    STRICT_DEVICE_EXECUTION.get_or_init(|| AtomicBool::new(env_strict_device_execution()))
}

pub fn set_strict_device_execution(enabled: bool) {
    strict_device_execution_toggle().store(enabled, Ordering::Relaxed);
}

pub fn is_strict_device_execution() -> bool {
    strict_device_execution_toggle().load(Ordering::Relaxed)
}

pub struct StrictDeviceExecutionGuard {
    previous: bool,
}

pub fn set_strict_device_execution_scoped(enabled: bool) -> StrictDeviceExecutionGuard {
    let previous = is_strict_device_execution();
    set_strict_device_execution(enabled);
    StrictDeviceExecutionGuard { previous }
}

impl Drop for StrictDeviceExecutionGuard {
    fn drop(&mut self) {
        set_strict_device_execution(self.previous);
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub enum TensorRawData {
    F32(Vec<f32>),
    F16(Vec<u16>),
    BF16(Vec<u16>),
    I8 { values: Vec<i8>, scale: f32 },
}

pub enum TensorStorageView<'a> {
    F32(ArrayViewD<'a, f32>),
    F16(ArrayViewD<'a, f16>),
    BF16(ArrayViewD<'a, bf16>),
}

pub enum TensorStorageViewMut<'a> {
    F32(ArrayViewMutD<'a, f32>),
    F16(ArrayViewMutD<'a, f16>),
    BF16(ArrayViewMutD<'a, bf16>),
    I8(ArrayViewMutD<'a, i8>, f32),
}

pub(crate) enum TensorStorageOwned {
    F32(ArcArray<f32, IxDyn>),
    F16(ArcArray<f16, IxDyn>),
    BF16(ArcArray<bf16, IxDyn>),
    I8(ArcArray<i8, IxDyn>, f32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Device {
    Cpu,
    Cuda,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoragePreference {
    Auto,
    Native,
    F32Compute,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DTypeDispatch {
    PureF32,
    SameF16,
    SameBF16,
    SameI8,
    Mixed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KernelRouteClass {
    GenericMatmul,
    DecodeKernel,
    Argmax,
}

#[inline]
pub fn classify_dtype_dispatch(input_dtype: DType, tensor_dtype: DType) -> DTypeDispatch {
    match (input_dtype, tensor_dtype) {
        (DType::F32, DType::F32) => DTypeDispatch::PureF32,
        (DType::F16, DType::F16) => DTypeDispatch::SameF16,
        (DType::BF16, DType::BF16) => DTypeDispatch::SameBF16,
        (DType::I8, DType::I8) => DTypeDispatch::SameI8,
        _ => DTypeDispatch::Mixed,
    }
}

#[inline]
pub fn preferred_parameter_storage_for_input_dtype(
    input_dtype: DType,
    tensor_dtype: DType,
) -> StoragePreference {
    preferred_parameter_storage_for_route(input_dtype, tensor_dtype, KernelRouteClass::DecodeKernel)
}

#[inline]
pub fn preferred_parameter_storage_for_route(
    input_dtype: DType,
    tensor_dtype: DType,
    route: KernelRouteClass,
) -> StoragePreference {
    match classify_dtype_dispatch(input_dtype, tensor_dtype) {
        DTypeDispatch::PureF32 => StoragePreference::Auto,
        DTypeDispatch::SameF16 => StoragePreference::Auto,
        DTypeDispatch::SameBF16 if matches!(route, KernelRouteClass::GenericMatmul) => {
            StoragePreference::Auto
        }
        DTypeDispatch::SameBF16 | DTypeDispatch::SameI8 => StoragePreference::Native,
        DTypeDispatch::Mixed => {
            #[cfg(all(feature = "arm64-fp-kernels", target_arch = "aarch64"))]
            {
                if matches!(route, KernelRouteClass::GenericMatmul)
                    && input_dtype == DType::F32
                    && matches!(tensor_dtype, DType::BF16 | DType::F16)
                {
                    return StoragePreference::Native;
                }
            }

            let _ = route;
            StoragePreference::Auto
        }
    }
}

pub struct TensorData {
    pub data: ArcArray<f32, IxDyn>,
    pub f16_data: Option<ArcArray<f16, IxDyn>>,
    pub bf16_data: Option<ArcArray<bf16, IxDyn>>,
    pub i8_data: Option<ArcArray<i8, IxDyn>>,
    pub cuda_f32_data: Option<crate::ops::cuda::CudaBuffer>,
    pub i8_scale: Option<f32>,
    pub has_f32_data: bool,
    pub storage_dtype: DType,
    pub cache_dirty: bool,
    pub is_parameter: bool,
    // 梯度：使用 ArcArray 便于 optimizer 侧 clone 为零拷贝（仅增 refcount）
    pub grad: Option<ArcArray<f32, IxDyn>>,
    pub cuda_f32_grad: Option<crate::ops::cuda::CudaBuffer>,
    pub parents: Vec<Tensor>,
    // backward_op 接收 grad 的 view，避免在反传遍历时额外 to_owned
    pub backward_op: Option<Rc<dyn Fn(&ArrayViewD<f32>)>>,
    pub requires_grad: bool,
    pub device: Device,
}

#[derive(Clone)]
pub struct Tensor(pub(crate) Rc<RefCell<TensorData>>);

impl Tensor {
    #[inline]
    fn empty_f32_storage() -> ArcArray<f32, IxDyn> {
        ArrayD::<f32>::zeros(IxDyn(&[0])).into_shared()
    }

    #[inline]
    fn empty_tensor_data_for_shape(
        shape: &[usize],
        dtype: DType,
        requires_grad: bool,
        is_parameter: bool,
        i8_scale: Option<f32>,
    ) -> TensorData {
        let shape_dyn = IxDyn(shape);
        match dtype {
            DType::F32 => TensorData {
                data: ArrayD::<f32>::zeros(shape_dyn).into_shared(),
                f16_data: None,
                bf16_data: None,
                i8_data: None,
                cuda_f32_data: None,
                i8_scale: None,
                has_f32_data: true,
                storage_dtype: DType::F32,
                cache_dirty: false,
                is_parameter,
                grad: None,
                cuda_f32_grad: None,
                parents: Vec::new(),
                backward_op: None,
                requires_grad,
                device: Device::Cpu,
            },
            DType::F16 => TensorData {
                data: Self::empty_f32_storage(),
                f16_data: Some(
                    ArrayD::<f16>::from_elem(shape_dyn, f16::from_f32(0.0)).into_shared(),
                ),
                bf16_data: None,
                i8_data: None,
                cuda_f32_data: None,
                i8_scale: None,
                has_f32_data: false,
                storage_dtype: DType::F16,
                cache_dirty: false,
                is_parameter,
                grad: None,
                cuda_f32_grad: None,
                parents: Vec::new(),
                backward_op: None,
                requires_grad,
                device: Device::Cpu,
            },
            DType::BF16 => TensorData {
                data: Self::empty_f32_storage(),
                bf16_data: Some(
                    ArrayD::<bf16>::from_elem(shape_dyn, bf16::from_f32(0.0)).into_shared(),
                ),
                f16_data: None,
                i8_data: None,
                cuda_f32_data: None,
                i8_scale: None,
                has_f32_data: false,
                storage_dtype: DType::BF16,
                cache_dirty: false,
                is_parameter,
                grad: None,
                cuda_f32_grad: None,
                parents: Vec::new(),
                backward_op: None,
                requires_grad,
                device: Device::Cpu,
            },
            DType::I8 => TensorData {
                data: Self::empty_f32_storage(),
                f16_data: None,
                bf16_data: None,
                i8_data: Some(ArrayD::<i8>::zeros(shape_dyn).into_shared()),
                cuda_f32_data: None,
                i8_scale: Some(i8_scale.unwrap_or(1.0)),
                has_f32_data: false,
                storage_dtype: DType::I8,
                cache_dirty: false,
                is_parameter,
                grad: None,
                cuda_f32_grad: None,
                parents: Vec::new(),
                backward_op: None,
                requires_grad,
                device: Device::Cpu,
            },
        }
    }

    fn quantize_f32_to_i8(
        data: &ArrayD<f32>,
        scale_override: Option<f32>,
    ) -> (ArcArray<i8, IxDyn>, f32) {
        let shape = data.shape().to_vec();
        let scale = if let Some(scale) = scale_override {
            assert!(
                scale.is_finite() && scale > 0.0,
                "quantization scale must be finite and > 0, got {}",
                scale
            );
            scale
        } else {
            let max_abs = data.iter().copied().map(f32::abs).fold(0.0f32, f32::max);
            if max_abs > 0.0 { max_abs / 127.0 } else { 1.0 }
        };
        let inv_scale = 1.0 / scale;
        let raw = data
            .iter()
            .map(|&v| (v * inv_scale).round().clamp(-127.0, 127.0) as i8)
            .collect::<Vec<_>>();
        (
            Array::from_shape_vec(IxDyn(&shape), raw)
                .expect("Failed to convert f32 array to i8 storage")
                .into_shared(),
            scale,
        )
    }

    fn quantize_f32_values_into_i8_slice(
        data: &[f32],
        scale_override: Option<f32>,
        dst: &mut [i8],
    ) -> f32 {
        assert_eq!(data.len(), dst.len(), "quantized dst len mismatch");
        let scale = if let Some(scale) = scale_override {
            assert!(
                scale.is_finite() && scale > 0.0,
                "quantization scale must be finite and > 0, got {}",
                scale
            );
            scale
        } else {
            let max_abs = data.iter().copied().map(f32::abs).fold(0.0f32, f32::max);
            if max_abs > 0.0 { max_abs / 127.0 } else { 1.0 }
        };
        let inv_scale = 1.0 / scale;
        for (src, dst) in data.iter().zip(dst.iter_mut()) {
            *dst = (*src * inv_scale).round().clamp(-127.0, 127.0) as i8;
        }
        scale
    }

    fn quantize_f16_values_into_i8_slice(
        data: &[f16],
        scale_override: Option<f32>,
        dst: &mut [i8],
    ) -> f32 {
        assert_eq!(data.len(), dst.len(), "quantized dst len mismatch");
        let scale = if let Some(scale) = scale_override {
            assert!(
                scale.is_finite() && scale > 0.0,
                "quantization scale must be finite and > 0, got {}",
                scale
            );
            scale
        } else {
            let max_abs = data
                .iter()
                .map(|&v| v.to_f32().abs())
                .fold(0.0f32, f32::max);
            if max_abs > 0.0 { max_abs / 127.0 } else { 1.0 }
        };
        let inv_scale = 1.0 / scale;
        for (src, dst) in data.iter().zip(dst.iter_mut()) {
            *dst = (src.to_f32() * inv_scale).round().clamp(-127.0, 127.0) as i8;
        }
        scale
    }

    fn quantize_bf16_values_into_i8_slice(
        data: &[bf16],
        scale_override: Option<f32>,
        dst: &mut [i8],
    ) -> f32 {
        assert_eq!(data.len(), dst.len(), "quantized dst len mismatch");
        let scale = if let Some(scale) = scale_override {
            assert!(
                scale.is_finite() && scale > 0.0,
                "quantization scale must be finite and > 0, got {}",
                scale
            );
            scale
        } else {
            let max_abs = data
                .iter()
                .map(|&v| v.to_f32().abs())
                .fold(0.0f32, f32::max);
            if max_abs > 0.0 { max_abs / 127.0 } else { 1.0 }
        };
        let inv_scale = 1.0 / scale;
        for (src, dst) in data.iter().zip(dst.iter_mut()) {
            *dst = (src.to_f32() * inv_scale).round().clamp(-127.0, 127.0) as i8;
        }
        scale
    }

    fn quantize_f32_slice_to_i8(
        shape: &[usize],
        data: &[f32],
        scale_override: Option<f32>,
    ) -> (ArcArray<i8, IxDyn>, f32) {
        let mut raw = vec![0i8; data.len()];
        let scale = Self::quantize_f32_values_into_i8_slice(data, scale_override, &mut raw);
        (
            Array::from_shape_vec(IxDyn(shape), raw)
                .expect("Failed to convert f32 slice to i8 storage")
                .into_shared(),
            scale,
        )
    }

    fn quantize_f16_slice_to_i8(
        shape: &[usize],
        data: &[f16],
        scale_override: Option<f32>,
    ) -> (ArcArray<i8, IxDyn>, f32) {
        let mut raw = vec![0i8; data.len()];
        let scale = Self::quantize_f16_values_into_i8_slice(data, scale_override, &mut raw);
        (
            Array::from_shape_vec(IxDyn(shape), raw)
                .expect("Failed to convert f16 slice to i8 storage")
                .into_shared(),
            scale,
        )
    }

    fn quantize_bf16_slice_to_i8(
        shape: &[usize],
        data: &[bf16],
        scale_override: Option<f32>,
    ) -> (ArcArray<i8, IxDyn>, f32) {
        let mut raw = vec![0i8; data.len()];
        let scale = Self::quantize_bf16_values_into_i8_slice(data, scale_override, &mut raw);
        (
            Array::from_shape_vec(IxDyn(shape), raw)
                .expect("Failed to convert bf16 slice to i8 storage")
                .into_shared(),
            scale,
        )
    }

    fn i8_slice_to_shared(shape: &[usize], data: &[i8]) -> ArcArray<i8, IxDyn> {
        Array::from_shape_vec(IxDyn(shape), data.to_vec())
            .expect("Failed to convert i8 slice to i8 storage")
            .into_shared()
    }

    fn i8_slice_to_f32_shared(shape: &[usize], data: &[i8], scale: f32) -> ArcArray<f32, IxDyn> {
        let raw = data.iter().map(|&v| (v as f32) * scale).collect::<Vec<_>>();
        Array::from_shape_vec(IxDyn(shape), raw)
            .expect("Failed to convert i8 slice to f32 storage")
            .into_shared()
    }

    fn i8_slice_to_f16_shared(shape: &[usize], data: &[i8], scale: f32) -> ArcArray<f16, IxDyn> {
        let raw = data
            .iter()
            .map(|&v| f16::from_f32((v as f32) * scale))
            .collect::<Vec<_>>();
        Array::from_shape_vec(IxDyn(shape), raw)
            .expect("Failed to convert i8 slice to f16 storage")
            .into_shared()
    }

    fn i8_slice_to_bf16_shared(shape: &[usize], data: &[i8], scale: f32) -> ArcArray<bf16, IxDyn> {
        let raw = data
            .iter()
            .map(|&v| bf16::from_f32((v as f32) * scale))
            .collect::<Vec<_>>();
        Array::from_shape_vec(IxDyn(shape), raw)
            .expect("Failed to convert i8 slice to bf16 storage")
            .into_shared()
    }

    fn quantize_f32_to_dtype(
        data: &ArrayD<f32>,
        dtype: DType,
        scale_override: Option<f32>,
    ) -> (ArcArray<i8, IxDyn>, f32) {
        match dtype {
            DType::I8 => Self::quantize_f32_to_i8(data, scale_override),
            other => {
                panic!(
                    "quantized storage dtype {:?} is not implemented yet; currently only I8 is supported",
                    other
                )
            }
        }
    }

    fn f32_array_to_bf16(data: &ArrayD<f32>) -> ArcArray<bf16, IxDyn> {
        let shape = data.shape().to_vec();
        let raw = if let Some(slice) = data.as_slice() {
            let mut raw = vec![bf16::from_bits(0); slice.len()];
            raw.convert_from_f32_slice(slice);
            raw
        } else {
            data.iter().map(|&v| bf16::from_f32(v)).collect::<Vec<_>>()
        };
        Array::from_shape_vec(IxDyn(&shape), raw)
            .expect("Failed to convert f32 array to bf16 storage")
            .into_shared()
    }

    fn f32_array_to_f16(data: &ArrayD<f32>) -> ArcArray<f16, IxDyn> {
        let shape = data.shape().to_vec();
        let raw = if let Some(slice) = data.as_slice() {
            let mut raw = vec![f16::from_bits(0); slice.len()];
            raw.convert_from_f32_slice(slice);
            raw
        } else {
            data.iter().map(|&v| f16::from_f32(v)).collect::<Vec<_>>()
        };
        Array::from_shape_vec(IxDyn(&shape), raw)
            .expect("Failed to convert f32 array to f16 storage")
            .into_shared()
    }

    fn f32_arc_to_f16(data: &ArcArray<f32, IxDyn>) -> ArcArray<f16, IxDyn> {
        let shape = data.shape().to_vec();
        let raw = if let Some(slice) = data.as_slice() {
            let mut raw = vec![f16::from_bits(0); slice.len()];
            raw.convert_from_f32_slice(slice);
            raw
        } else {
            data.iter().map(|&v| f16::from_f32(v)).collect::<Vec<_>>()
        };
        Array::from_shape_vec(IxDyn(&shape), raw)
            .expect("Failed to convert f32 array to f16 storage")
            .into_shared()
    }

    fn f32_arc_to_bf16(data: &ArcArray<f32, IxDyn>) -> ArcArray<bf16, IxDyn> {
        let shape = data.shape().to_vec();
        let raw = if let Some(slice) = data.as_slice() {
            let mut raw = vec![bf16::from_bits(0); slice.len()];
            raw.convert_from_f32_slice(slice);
            raw
        } else {
            data.iter().map(|&v| bf16::from_f32(v)).collect::<Vec<_>>()
        };
        Array::from_shape_vec(IxDyn(&shape), raw)
            .expect("Failed to convert f32 array to bf16 storage")
            .into_shared()
    }

    fn bf16_arc_to_f32(data: &ArcArray<bf16, IxDyn>) -> ArcArray<f32, IxDyn> {
        let shape = data.shape().to_vec();
        let raw = if let Some(slice) = data.as_slice() {
            let mut raw = vec![0.0f32; slice.len()];
            slice.convert_to_f32_slice(&mut raw);
            raw
        } else {
            data.iter().map(|&v| v.to_f32()).collect::<Vec<_>>()
        };
        Array::from_shape_vec(IxDyn(&shape), raw)
            .expect("Failed to convert bf16 storage to f32")
            .into_shared()
    }

    fn f16_arc_to_f32(data: &ArcArray<f16, IxDyn>) -> ArcArray<f32, IxDyn> {
        let shape = data.shape().to_vec();
        let raw = if let Some(slice) = data.as_slice() {
            let mut raw = vec![0.0f32; slice.len()];
            slice.convert_to_f32_slice(&mut raw);
            raw
        } else {
            data.iter().map(|&v| v.to_f32()).collect::<Vec<_>>()
        };
        Array::from_shape_vec(IxDyn(&shape), raw)
            .expect("Failed to convert f16 storage to f32")
            .into_shared()
    }

    fn i8_arc_to_f32(data: &ArcArray<i8, IxDyn>, scale: f32) -> ArcArray<f32, IxDyn> {
        let shape = data.shape().to_vec();
        let raw = data.iter().map(|&v| (v as f32) * scale).collect::<Vec<_>>();
        Array::from_shape_vec(IxDyn(&shape), raw)
            .expect("Failed to convert i8 storage to f32")
            .into_shared()
    }

    fn with_bf16_compute_view<R>(
        data: &ArcArray<bf16, IxDyn>,
        f: impl FnOnce(ArrayViewD<'_, f32>) -> R,
    ) -> R {
        let shape = data.shape().to_vec();
        let len = data.len();
        F32_COMPUTE_SCRATCH.with(|scratch| {
            if let Ok(mut scratch) = scratch.try_borrow_mut() {
                if scratch.len() < len {
                    scratch.resize(len, 0.0);
                }
                if let Some(slice) = data.as_slice() {
                    slice.convert_to_f32_slice(&mut scratch[..len]);
                } else {
                    for (dst, src) in scratch[..len].iter_mut().zip(data.iter()) {
                        *dst = src.to_f32();
                    }
                }
                let view = ArrayViewD::from_shape(IxDyn(&shape), &scratch[..len])
                    .expect("Failed to build temporary f32 compute view");
                return f(view);
            }

            let mut fallback = vec![0.0f32; len];
            if let Some(slice) = data.as_slice() {
                slice.convert_to_f32_slice(&mut fallback);
            } else {
                for (dst, src) in fallback.iter_mut().zip(data.iter()) {
                    *dst = src.to_f32();
                }
            }
            let view = ArrayViewD::from_shape(IxDyn(&shape), &fallback)
                .expect("Failed to build temporary f32 compute view");
            f(view)
        })
    }

    fn with_f16_compute_view<R>(
        data: &ArcArray<f16, IxDyn>,
        f: impl FnOnce(ArrayViewD<'_, f32>) -> R,
    ) -> R {
        let shape = data.shape().to_vec();
        let len = data.len();
        F32_COMPUTE_SCRATCH.with(|scratch| {
            if let Ok(mut scratch) = scratch.try_borrow_mut() {
                if scratch.len() < len {
                    scratch.resize(len, 0.0);
                }
                if let Some(slice) = data.as_slice() {
                    slice.convert_to_f32_slice(&mut scratch[..len]);
                } else {
                    for (dst, src) in scratch[..len].iter_mut().zip(data.iter()) {
                        *dst = src.to_f32();
                    }
                }
                let view = ArrayViewD::from_shape(IxDyn(&shape), &scratch[..len])
                    .expect("Failed to build temporary f32 compute view");
                return f(view);
            }

            let mut fallback = vec![0.0f32; len];
            if let Some(slice) = data.as_slice() {
                slice.convert_to_f32_slice(&mut fallback);
            } else {
                for (dst, src) in fallback.iter_mut().zip(data.iter()) {
                    *dst = src.to_f32();
                }
            }
            let view = ArrayViewD::from_shape(IxDyn(&shape), &fallback)
                .expect("Failed to build temporary f32 compute view");
            f(view)
        })
    }

    fn with_i8_compute_view<R>(
        data: &ArcArray<i8, IxDyn>,
        scale: f32,
        f: impl FnOnce(ArrayViewD<'_, f32>) -> R,
    ) -> R {
        let shape = data.shape().to_vec();
        let len = data.len();
        F32_COMPUTE_SCRATCH.with(|scratch| {
            if let Ok(mut scratch) = scratch.try_borrow_mut() {
                if scratch.len() < len {
                    scratch.resize(len, 0.0);
                }
                for (dst, src) in scratch[..len].iter_mut().zip(data.iter()) {
                    *dst = (*src as f32) * scale;
                }
                let view = ArrayViewD::from_shape(IxDyn(&shape), &scratch[..len])
                    .expect("Failed to build temporary f32 compute view");
                return f(view);
            }

            let mut fallback = vec![0.0f32; len];
            for (dst, src) in fallback.iter_mut().zip(data.iter()) {
                *dst = (*src as f32) * scale;
            }
            let view = ArrayViewD::from_shape(IxDyn(&shape), &fallback)
                .expect("Failed to build temporary f32 compute view");
            f(view)
        })
    }

    fn clear_non_f32_storage(inner: &mut TensorData) {
        inner.f16_data = None;
        inner.bf16_data = None;
        inner.i8_data = None;
        inner.i8_scale = None;
    }

    fn clear_cuda_storage(inner: &mut TensorData) {
        inner.cuda_f32_data = None;
    }

    fn logical_shape(inner: &TensorData) -> &[usize] {
        if inner.storage_dtype == DType::F16 {
            if let Some(f16_data) = inner.f16_data.as_ref() {
                return f16_data.shape();
            }
        }
        if inner.storage_dtype == DType::BF16 {
            if let Some(bf16_data) = inner.bf16_data.as_ref() {
                return bf16_data.shape();
            }
        }
        if inner.storage_dtype == DType::I8 {
            if let Some(i8_data) = inner.i8_data.as_ref() {
                return i8_data.shape();
            }
        }

        if inner.has_f32_data {
            inner.data.shape()
        } else if let Some(bf16_data) = inner.bf16_data.as_ref() {
            bf16_data.shape()
        } else if let Some(i8_data) = inner.i8_data.as_ref() {
            i8_data.shape()
        } else {
            inner.data.shape()
        }
    }

    fn build_tensor_data(
        dtype: DType,
        f32_data: ArrayD<f32>,
        requires_grad: bool,
        is_parameter: bool,
    ) -> TensorData {
        match dtype {
            DType::F32 => TensorData {
                data: f32_data.into_shared(),
                f16_data: None,
                bf16_data: None,
                i8_data: None,
                cuda_f32_data: None,
                i8_scale: None,
                has_f32_data: true,
                storage_dtype: DType::F32,
                cache_dirty: false,
                is_parameter,
                grad: None,
                cuda_f32_grad: None,
                parents: Vec::new(),
                backward_op: None,
                requires_grad,
                device: Device::Cpu,
            },
            DType::F16 => TensorData {
                data: Self::empty_f32_storage(),
                f16_data: Some(Self::f32_array_to_f16(&f32_data)),
                bf16_data: None,
                i8_data: None,
                cuda_f32_data: None,
                i8_scale: None,
                has_f32_data: false,
                storage_dtype: DType::F16,
                cache_dirty: false,
                is_parameter,
                grad: None,
                cuda_f32_grad: None,
                parents: Vec::new(),
                backward_op: None,
                requires_grad,
                device: Device::Cpu,
            },
            DType::BF16 => TensorData {
                data: Self::empty_f32_storage(),
                f16_data: None,
                bf16_data: Some(Self::f32_array_to_bf16(&f32_data)),
                i8_data: None,
                cuda_f32_data: None,
                i8_scale: None,
                has_f32_data: false,
                storage_dtype: DType::BF16,
                cache_dirty: false,
                is_parameter,
                grad: None,
                cuda_f32_grad: None,
                parents: Vec::new(),
                backward_op: None,
                requires_grad,
                device: Device::Cpu,
            },
            DType::I8 => {
                let (i8_data, scale) = Self::quantize_f32_to_dtype(&f32_data, DType::I8, None);
                TensorData {
                    data: Self::empty_f32_storage(),
                    f16_data: None,
                    bf16_data: None,
                    i8_data: Some(i8_data),
                    cuda_f32_data: None,
                    i8_scale: Some(scale),
                    has_f32_data: false,
                    storage_dtype: DType::I8,
                    cache_dirty: false,
                    is_parameter,
                    grad: None,
                    cuda_f32_grad: None,
                    parents: Vec::new(),
                    backward_op: None,
                    requires_grad,
                    device: Device::Cpu,
                }
            }
        }
    }

    fn ensure_f32_data(&self, mutable: bool) {
        let mut inner = self.0.borrow_mut();

        if inner.has_f32_data {
            if mutable {
                Self::clear_cuda_storage(&mut inner);
            }
            if mutable && inner.storage_dtype != DType::F32 {
                inner.storage_dtype = DType::F32;
                if inner.is_parameter
                    && allow_parameter_dtype_copies()
                    && (inner.f16_data.is_some()
                        || inner.bf16_data.is_some()
                        || inner.i8_data.is_some())
                {
                    inner.cache_dirty = true;
                } else {
                    Self::clear_non_f32_storage(&mut inner);
                    inner.cache_dirty = false;
                }
            }
            return;
        }

        if let Some(buffer) = inner.cuda_f32_data.as_ref()
            && inner.storage_dtype != DType::I8
        {
            let host_data = crate::ops::cuda::download_f32(buffer)
                .unwrap_or_else(|err| panic!("Failed to download CUDA tensor to host: {}", err));
            let shape = Self::logical_shape(&inner).to_vec();
            inner.data = Array::from_shape_vec(IxDyn(&shape), host_data)
                .expect("Failed to materialize host f32 storage from CUDA buffer")
                .into_shared();
            inner.has_f32_data = true;
            if mutable {
                Self::clear_cuda_storage(&mut inner);
            }
            if mutable || !inner.is_parameter || !allow_parameter_dtype_copies() {
                inner.storage_dtype = DType::F32;
                Self::clear_non_f32_storage(&mut inner);
            }
            inner.cache_dirty = false;
            return;
        }

        if mutable {
            Self::clear_cuda_storage(&mut inner);
        }

        if let Some(f16_data) = inner.f16_data.clone() {
            inner.data = Self::f16_arc_to_f32(&f16_data);
        } else if let Some(bf16_data) = inner.bf16_data.clone() {
            inner.data = Self::bf16_arc_to_f32(&bf16_data);
        } else if let Some(i8_data) = inner.i8_data.clone() {
            let scale = inner
                .i8_scale
                .expect("I8 tensor missing quantization scale");
            inner.data = Self::i8_arc_to_f32(&i8_data, scale);
        } else {
            panic!("Tensor has no materializable storage");
        }
        inner.has_f32_data = true;

        if mutable || !inner.is_parameter || !allow_parameter_dtype_copies() {
            inner.storage_dtype = DType::F32;
            if inner.is_parameter && allow_parameter_dtype_copies() && mutable {
                inner.cache_dirty = true;
            } else {
                Self::clear_non_f32_storage(&mut inner);
                inner.cache_dirty = false;
            }
        }
    }

    fn storage_as_f32_vec(&self) -> Vec<f32> {
        let inner = self.0.borrow();
        if inner.has_f32_data {
            return inner.data.iter().copied().collect();
        }

        if let Some(f16_data) = inner.f16_data.as_ref() {
            if let Some(slice) = f16_data.as_slice() {
                let mut out = vec![0.0f32; slice.len()];
                slice.convert_to_f32_slice(&mut out);
                return out;
            }
            return f16_data.iter().map(|&v| v.to_f32()).collect();
        }

        if let Some(bf16_data) = inner.bf16_data.as_ref() {
            if let Some(slice) = bf16_data.as_slice() {
                let mut out = vec![0.0f32; slice.len()];
                slice.convert_to_f32_slice(&mut out);
                return out;
            }
            return bf16_data.iter().map(|&v| v.to_f32()).collect();
        }

        if let Some(i8_data) = inner.i8_data.as_ref() {
            let scale = inner
                .i8_scale
                .expect("I8 tensor missing quantization scale");
            return i8_data.iter().map(|&v| (v as f32) * scale).collect();
        }

        inner.data.iter().copied().collect()
    }

    fn ensure_cuda_f32_data(&self) {
        assert!(
            self.device() == Device::Cuda,
            "ensure_cuda_f32_data expects a CUDA tensor"
        );
        if self.0.borrow().cuda_f32_data.is_some() {
            return;
        }
        if self.len() == 0 {
            let mut inner = self.0.borrow_mut();
            inner.device = Device::Cuda;
            inner.cuda_f32_data = None;
            return;
        }

        let host_data = self.storage_as_f32_vec();
        let buffer = crate::ops::cuda::upload_f32(&host_data)
            .unwrap_or_else(|err| panic!("Failed to upload tensor to CUDA: {}", err));

        let mut inner = self.0.borrow_mut();
        inner.device = Device::Cuda;
        inner.cuda_f32_data = Some(buffer);
    }

    pub(crate) fn with_cuda_f32_buffer<R>(
        &self,
        f: impl FnOnce(&crate::ops::cuda::CudaBuffer) -> R,
    ) -> R {
        self.ensure_cuda_f32_data();
        let inner = self.0.borrow();
        let buffer = inner
            .cuda_f32_data
            .as_ref()
            .expect("CUDA tensor missing resident f32 buffer");
        f(buffer)
    }

    pub(crate) fn cloned_cuda_f32_buffer(&self) -> Option<crate::ops::cuda::CudaBuffer> {
        if self.device() != Device::Cuda {
            return None;
        }
        self.ensure_cuda_f32_data();
        self.0.borrow().cuda_f32_data.clone()
    }

    #[allow(dead_code)]
    pub(crate) fn has_host_f32_data(&self) -> bool {
        self.0.borrow().has_f32_data
    }

    pub(crate) fn set_cuda_f32_buffer_inplace(&self, buffer: crate::ops::cuda::CudaBuffer) {
        let mut inner = self.0.borrow_mut();
        inner.device = Device::Cuda;
        inner.cuda_f32_data = Some(buffer);
    }

    pub(crate) fn replace_cuda_f32_buffer_no_host_sync(
        &self,
        buffer: crate::ops::cuda::CudaBuffer,
    ) {
        let shape = {
            let inner = self.0.borrow();
            Self::logical_shape(&inner).to_vec()
        };
        let len = shape.iter().product::<usize>();
        assert_eq!(
            buffer.len(),
            len,
            "CUDA tensor buffer length mismatch: expected {}, got {}",
            len,
            buffer.len()
        );

        let mut inner = self.0.borrow_mut();
        Self::clear_non_f32_storage(&mut inner);
        inner.data = ArrayD::<f32>::zeros(IxDyn(&shape)).into_shared();
        inner.has_f32_data = false;
        inner.storage_dtype = DType::F32;
        inner.cache_dirty = false;
        inner.device = Device::Cuda;
        inner.cuda_f32_data = Some(buffer);
        inner.grad = None;
        inner.cuda_f32_grad = None;
    }

    fn write_f32_row_4d_impl(
        &self,
        bb: usize,
        hk: usize,
        pos: usize,
        src: &[f32],
        cuda_src: Option<(&crate::ops::cuda::CudaBuffer, usize)>,
    ) {
        let cuda_buffer = {
            let inner = self.0.borrow();
            if inner.device == Device::Cuda {
                inner.cuda_f32_data.clone()
            } else {
                None
            }
        };

        let (offset, can_use_cuda_src) = {
            let mut inner = self.0.borrow_mut();
            match inner.storage_dtype {
                DType::F32 => {
                    let shape = inner.data.shape().to_vec();
                    assert_eq!(shape.len(), 4, "write_f32_row_4d_inplace expects [B,H,S,D]");
                    let (b_dim, h_dim, s_dim, d_dim) = (shape[0], shape[1], shape[2], shape[3]);
                    assert!(
                        bb < b_dim && hk < h_dim && pos < s_dim,
                        "cache row index out of bounds"
                    );
                    assert_eq!(src.len(), d_dim, "cache row width mismatch");
                    let mut view4 = inner
                        .data
                        .view_mut()
                        .into_dimensionality::<ndarray::Ix4>()
                        .expect("KV cache must be [B,H,S,D]");
                    let mut row = view4.slice_mut(ndarray::s![bb, hk, pos, ..]);
                    row.as_slice_mut()
                        .expect("KV cache row must be contiguous")
                        .copy_from_slice(src);
                    ((((bb * h_dim) + hk) * s_dim + pos) * d_dim, true)
                }
                DType::F16 => {
                    let data = inner
                        .f16_data
                        .as_mut()
                        .expect("f16 storage missing for cache row write");
                    let shape = data.shape().to_vec();
                    assert_eq!(shape.len(), 4, "write_f32_row_4d_inplace expects [B,H,S,D]");
                    let (b_dim, h_dim, s_dim, d_dim) = (shape[0], shape[1], shape[2], shape[3]);
                    assert!(
                        bb < b_dim && hk < h_dim && pos < s_dim,
                        "cache row index out of bounds"
                    );
                    assert_eq!(src.len(), d_dim, "cache row width mismatch");
                    let mut view4 = data
                        .view_mut()
                        .into_dimensionality::<ndarray::Ix4>()
                        .expect("KV cache must be [B,H,S,D]");
                    let mut row = view4.slice_mut(ndarray::s![bb, hk, pos, ..]);
                    let row_slice = row.as_slice_mut().expect("KV cache row must be contiguous");
                    for (dst, &value) in row_slice.iter_mut().zip(src.iter()) {
                        *dst = f16::from_f32(value);
                    }
                    ((((bb * h_dim) + hk) * s_dim + pos) * d_dim, false)
                }
                DType::BF16 => {
                    let data = inner
                        .bf16_data
                        .as_mut()
                        .expect("bf16 storage missing for cache row write");
                    let shape = data.shape().to_vec();
                    assert_eq!(shape.len(), 4, "write_f32_row_4d_inplace expects [B,H,S,D]");
                    let (b_dim, h_dim, s_dim, d_dim) = (shape[0], shape[1], shape[2], shape[3]);
                    assert!(
                        bb < b_dim && hk < h_dim && pos < s_dim,
                        "cache row index out of bounds"
                    );
                    assert_eq!(src.len(), d_dim, "cache row width mismatch");
                    let mut view4 = data
                        .view_mut()
                        .into_dimensionality::<ndarray::Ix4>()
                        .expect("KV cache must be [B,H,S,D]");
                    let mut row = view4.slice_mut(ndarray::s![bb, hk, pos, ..]);
                    let row_slice = row.as_slice_mut().expect("KV cache row must be contiguous");
                    for (dst, &value) in row_slice.iter_mut().zip(src.iter()) {
                        *dst = bf16::from_f32(value);
                    }
                    ((((bb * h_dim) + hk) * s_dim + pos) * d_dim, false)
                }
                DType::I8 => {
                    panic!("write_f32_row_4d_inplace does not support i8 storage");
                }
            }
        };

        if let Some(buffer) = cuda_buffer {
            if can_use_cuda_src {
                if let Some((src_buffer, src_offset)) = cuda_src {
                    crate::ops::cuda::copy_f32_offset(
                        &buffer,
                        offset,
                        src_buffer,
                        src_offset,
                        src.len(),
                    )
                    .unwrap_or_else(|err| panic!("Failed to copy CUDA tensor row: {}", err));
                } else {
                    crate::ops::cuda::upload_f32_offset(&buffer, offset, src)
                        .unwrap_or_else(|err| panic!("Failed to update CUDA tensor row: {}", err));
                }
            } else {
                crate::ops::cuda::upload_f32_offset(&buffer, offset, src)
                    .unwrap_or_else(|err| panic!("Failed to update CUDA tensor row: {}", err));
            }
        }
    }

    pub(crate) fn write_f32_row_4d_inplace(&self, bb: usize, hk: usize, pos: usize, src: &[f32]) {
        self.write_f32_row_4d_impl(bb, hk, pos, src, None);
    }

    pub(crate) fn write_f32_row_4d_from_cuda_source_inplace(
        &self,
        bb: usize,
        hk: usize,
        pos: usize,
        src: &[f32],
        src_buffer: &crate::ops::cuda::CudaBuffer,
        src_offset: usize,
    ) {
        self.write_f32_row_4d_impl(bb, hk, pos, src, Some((src_buffer, src_offset)));
    }

    pub(crate) fn write_f32_row_4d_from_cuda_buffer_inplace(
        &self,
        bb: usize,
        hk: usize,
        pos: usize,
        src_buffer: &crate::ops::cuda::CudaBuffer,
        src_offset: usize,
        len: usize,
    ) {
        let mut host = crate::ops::cuda::download_f32_offset(src_buffer, src_offset, len)
            .unwrap_or_else(|err| panic!("Failed to download CUDA tensor row: {}", err));
        match self.dtype() {
            DType::F32 => {}
            DType::F16 => {
                for value in host.iter_mut() {
                    *value = f16::from_f32(*value).to_f32();
                }
            }
            DType::BF16 => {
                for value in host.iter_mut() {
                    *value = bf16::from_f32(*value).to_f32();
                }
            }
            DType::I8 => {
                panic!("write_f32_row_4d_from_cuda_buffer_inplace does not support i8 storage");
            }
        }
        self.write_f32_row_4d_impl(bb, hk, pos, &host, Some((src_buffer, src_offset)));
    }

    pub fn shape_vec(&self) -> Vec<usize> {
        let inner = self.0.borrow();
        Self::logical_shape(&inner).to_vec()
    }

    #[inline]
    pub fn len(&self) -> usize {
        let inner = self.0.borrow();
        if inner.storage_dtype == DType::F16 {
            if let Some(f16_data) = inner.f16_data.as_ref() {
                return f16_data.len();
            }
        }
        if inner.storage_dtype == DType::BF16 {
            if let Some(bf16_data) = inner.bf16_data.as_ref() {
                return bf16_data.len();
            }
        }
        if inner.storage_dtype == DType::I8 {
            if let Some(i8_data) = inner.i8_data.as_ref() {
                return i8_data.len();
            }
        }
        if inner.has_f32_data {
            inner.data.len()
        } else if let Some(f16_data) = inner.f16_data.as_ref() {
            f16_data.len()
        } else if let Some(bf16_data) = inner.bf16_data.as_ref() {
            bf16_data.len()
        } else if let Some(i8_data) = inner.i8_data.as_ref() {
            i8_data.len()
        } else {
            inner.data.len()
        }
    }

    #[inline]
    pub fn ndim(&self) -> usize {
        self.shape_vec().len()
    }

    #[inline]
    pub fn dtype(&self) -> DType {
        self.0.borrow().storage_dtype
    }

    #[inline]
    pub fn quantization_scale(&self) -> Option<f32> {
        let inner = self.0.borrow();
        if inner.storage_dtype == DType::I8 {
            inner.i8_scale
        } else {
            None
        }
    }

    pub fn cast_inplace(&self, dtype: DType) {
        match dtype {
            DType::F32 => {
                self.ensure_f32_data(false);
                let mut inner = self.0.borrow_mut();
                Self::clear_cuda_storage(&mut inner);
                inner.storage_dtype = DType::F32;
                Self::clear_non_f32_storage(&mut inner);
                inner.cache_dirty = false;
            }
            DType::F16 => {
                self.ensure_f32_data(false);
                let mut inner = self.0.borrow_mut();
                Self::clear_cuda_storage(&mut inner);
                inner.f16_data = Some(Self::f32_arc_to_f16(&inner.data));
                inner.bf16_data = None;
                inner.i8_data = None;
                inner.i8_scale = None;
                inner.storage_dtype = DType::F16;
                inner.data = Self::empty_f32_storage();
                inner.has_f32_data = false;
                inner.cache_dirty = false;
            }
            DType::BF16 => {
                self.ensure_f32_data(false);
                let mut inner = self.0.borrow_mut();
                Self::clear_cuda_storage(&mut inner);
                inner.f16_data = None;
                inner.bf16_data = Some(Self::f32_arc_to_bf16(&inner.data));
                inner.i8_data = None;
                inner.i8_scale = None;
                inner.storage_dtype = DType::BF16;
                inner.data = Self::empty_f32_storage();
                inner.has_f32_data = false;
                inner.cache_dirty = false;
            }
            DType::I8 => {
                self.ensure_f32_data(false);
                let mut inner = self.0.borrow_mut();
                Self::clear_cuda_storage(&mut inner);
                inner.f16_data = None;
                let (i8_data, scale) =
                    Self::quantize_f32_to_dtype(&inner.data.to_owned(), DType::I8, None);
                inner.bf16_data = None;
                inner.i8_data = Some(i8_data);
                inner.i8_scale = Some(scale);
                inner.storage_dtype = DType::I8;
                inner.data = Self::empty_f32_storage();
                inner.has_f32_data = false;
                inner.cache_dirty = false;
            }
        }
    }

    pub fn set_array_f32_with_dtype(&self, data: ArrayD<f32>, dtype: DType) {
        let mut inner = self.0.borrow_mut();
        Self::clear_cuda_storage(&mut inner);
        match dtype {
            DType::F32 => {
                inner.data = data.into_shared();
                Self::clear_non_f32_storage(&mut inner);
                inner.has_f32_data = true;
                inner.storage_dtype = DType::F32;
                inner.cache_dirty = false;
            }
            DType::F16 => {
                inner.data = Self::empty_f32_storage();
                inner.f16_data = Some(Self::f32_array_to_f16(&data));
                inner.bf16_data = None;
                inner.i8_data = None;
                inner.i8_scale = None;
                inner.has_f32_data = false;
                inner.storage_dtype = DType::F16;
                inner.cache_dirty = false;
            }
            DType::BF16 => {
                inner.data = Self::empty_f32_storage();
                inner.f16_data = None;
                inner.bf16_data = Some(Self::f32_array_to_bf16(&data));
                inner.i8_data = None;
                inner.i8_scale = None;
                inner.has_f32_data = false;
                inner.storage_dtype = DType::BF16;
                inner.cache_dirty = false;
            }
            DType::I8 => {
                let (i8_data, scale) = Self::quantize_f32_to_dtype(&data, DType::I8, None);
                inner.data = Self::empty_f32_storage();
                inner.f16_data = None;
                inner.bf16_data = None;
                inner.i8_data = Some(i8_data);
                inner.i8_scale = Some(scale);
                inner.has_f32_data = false;
                inner.storage_dtype = DType::I8;
                inner.cache_dirty = false;
            }
        }
        inner.grad = None;
        inner.cuda_f32_grad = None;
    }

    pub fn set_array_f32_with_quantization(
        &self,
        data: ArrayD<f32>,
        quantization: ParameterQuantization,
    ) {
        if let Some(dtype) = quantization.storage_dtype() {
            let mut inner = self.0.borrow_mut();
            Self::clear_cuda_storage(&mut inner);
            match dtype {
                DType::I8 => {
                    let (i8_data, scale) =
                        Self::quantize_f32_to_dtype(&data, dtype, quantization.scale_override());
                    inner.data = Self::empty_f32_storage();
                    inner.f16_data = None;
                    inner.bf16_data = None;
                    inner.i8_data = Some(i8_data);
                    inner.i8_scale = Some(scale);
                    inner.has_f32_data = false;
                    inner.storage_dtype = dtype;
                    inner.cache_dirty = false;
                    inner.grad = None;
                }
                other => {
                    panic!(
                        "quantized storage dtype {:?} is not implemented yet; currently only I8 is supported",
                        other
                    );
                }
            }
            return;
        }

        self.set_array_f32_with_dtype(data, DType::F32);
    }

    fn set_i8_storage(&self, i8_data: ArcArray<i8, IxDyn>, scale: f32) {
        let mut inner = self.0.borrow_mut();
        Self::clear_cuda_storage(&mut inner);
        inner.data = Self::empty_f32_storage();
        inner.f16_data = None;
        inner.bf16_data = None;
        inner.i8_data = Some(i8_data);
        inner.i8_scale = Some(scale);
        inner.has_f32_data = false;
        inner.storage_dtype = DType::I8;
        inner.cache_dirty = false;
        inner.grad = None;
    }

    fn try_quantize_into_existing_i8_storage_f32(
        &self,
        shape: &[usize],
        data: &[f32],
        quantization: ParameterQuantization,
    ) -> bool {
        let mut inner = self.0.borrow_mut();
        Self::clear_cuda_storage(&mut inner);
        if inner.storage_dtype != DType::I8 {
            return false;
        }
        let Some(existing) = inner.i8_data.as_mut() else {
            return false;
        };
        if existing.shape() != shape {
            return false;
        }
        let Some(dst) = existing.as_slice_memory_order_mut() else {
            return false;
        };
        let scale =
            Self::quantize_f32_values_into_i8_slice(data, quantization.scale_override(), dst);
        inner.i8_scale = Some(scale);
        inner.cache_dirty = false;
        inner.grad = None;
        true
    }

    fn try_quantize_into_existing_i8_storage_f16(
        &self,
        shape: &[usize],
        data: &[f16],
        quantization: ParameterQuantization,
    ) -> bool {
        let mut inner = self.0.borrow_mut();
        Self::clear_cuda_storage(&mut inner);
        if inner.storage_dtype != DType::I8 {
            return false;
        }
        let Some(existing) = inner.i8_data.as_mut() else {
            return false;
        };
        if existing.shape() != shape {
            return false;
        }
        let Some(dst) = existing.as_slice_memory_order_mut() else {
            return false;
        };
        let scale =
            Self::quantize_f16_values_into_i8_slice(data, quantization.scale_override(), dst);
        inner.i8_scale = Some(scale);
        inner.cache_dirty = false;
        inner.grad = None;
        true
    }

    fn try_quantize_into_existing_i8_storage_bf16(
        &self,
        shape: &[usize],
        data: &[bf16],
        quantization: ParameterQuantization,
    ) -> bool {
        let mut inner = self.0.borrow_mut();
        Self::clear_cuda_storage(&mut inner);
        if inner.storage_dtype != DType::I8 {
            return false;
        }
        let Some(existing) = inner.i8_data.as_mut() else {
            return false;
        };
        if existing.shape() != shape {
            return false;
        }
        let Some(dst) = existing.as_slice_memory_order_mut() else {
            return false;
        };
        let scale =
            Self::quantize_bf16_values_into_i8_slice(data, quantization.scale_override(), dst);
        inner.i8_scale = Some(scale);
        inner.cache_dirty = false;
        inner.grad = None;
        true
    }

    fn try_copy_into_existing_i8_storage_i8(
        &self,
        shape: &[usize],
        data: &[i8],
        scale: f32,
    ) -> bool {
        let mut inner = self.0.borrow_mut();
        Self::clear_cuda_storage(&mut inner);
        if inner.storage_dtype != DType::I8 {
            return false;
        }
        let Some(existing) = inner.i8_data.as_mut() else {
            return false;
        };
        if existing.shape() != shape {
            return false;
        }
        let Some(dst) = existing.as_slice_memory_order_mut() else {
            return false;
        };
        dst.copy_from_slice(data);
        inner.i8_scale = Some(scale);
        inner.cache_dirty = false;
        inner.grad = None;
        true
    }

    pub fn set_f32_slice_with_quantization(
        &self,
        shape: &[usize],
        data: &[f32],
        quantization: ParameterQuantization,
    ) {
        if self.try_quantize_into_existing_i8_storage_f32(shape, data, quantization) {
            return;
        }
        let dtype = quantization
            .storage_dtype()
            .expect("enabled quantization must provide storage dtype");
        match dtype {
            DType::I8 => {
                let (i8_data, scale) =
                    Self::quantize_f32_slice_to_i8(shape, data, quantization.scale_override());
                self.set_i8_storage(i8_data, scale);
            }
            other => panic!(
                "quantized storage dtype {:?} is not implemented yet; currently only I8 is supported",
                other
            ),
        }
    }

    pub fn set_f16_slice_with_quantization(
        &self,
        shape: &[usize],
        data: &[f16],
        quantization: ParameterQuantization,
    ) {
        if self.try_quantize_into_existing_i8_storage_f16(shape, data, quantization) {
            return;
        }
        let dtype = quantization
            .storage_dtype()
            .expect("enabled quantization must provide storage dtype");
        match dtype {
            DType::I8 => {
                let (i8_data, scale) =
                    Self::quantize_f16_slice_to_i8(shape, data, quantization.scale_override());
                self.set_i8_storage(i8_data, scale);
            }
            other => panic!(
                "quantized storage dtype {:?} is not implemented yet; currently only I8 is supported",
                other
            ),
        }
    }

    pub fn set_bf16_slice_with_quantization(
        &self,
        shape: &[usize],
        data: &[bf16],
        quantization: ParameterQuantization,
    ) {
        if self.try_quantize_into_existing_i8_storage_bf16(shape, data, quantization) {
            return;
        }
        let dtype = quantization
            .storage_dtype()
            .expect("enabled quantization must provide storage dtype");
        match dtype {
            DType::I8 => {
                let (i8_data, scale) =
                    Self::quantize_bf16_slice_to_i8(shape, data, quantization.scale_override());
                self.set_i8_storage(i8_data, scale);
            }
            other => panic!(
                "quantized storage dtype {:?} is not implemented yet; currently only I8 is supported",
                other
            ),
        }
    }

    pub fn set_i8_slice_with_dtype(&self, shape: &[usize], data: &[i8], scale: f32, dtype: DType) {
        match dtype {
            DType::F32 => {
                let mut inner = self.0.borrow_mut();
                Self::clear_cuda_storage(&mut inner);
                inner.data = Self::i8_slice_to_f32_shared(shape, data, scale);
                Self::clear_non_f32_storage(&mut inner);
                inner.has_f32_data = true;
                inner.storage_dtype = DType::F32;
                inner.cache_dirty = false;
                inner.grad = None;
            }
            DType::F16 => {
                let mut inner = self.0.borrow_mut();
                Self::clear_cuda_storage(&mut inner);
                inner.data = Self::empty_f32_storage();
                inner.f16_data = Some(Self::i8_slice_to_f16_shared(shape, data, scale));
                inner.bf16_data = None;
                inner.i8_data = None;
                inner.i8_scale = None;
                inner.has_f32_data = false;
                inner.storage_dtype = DType::F16;
                inner.cache_dirty = false;
                inner.grad = None;
            }
            DType::BF16 => {
                let mut inner = self.0.borrow_mut();
                Self::clear_cuda_storage(&mut inner);
                inner.data = Self::empty_f32_storage();
                inner.f16_data = None;
                inner.bf16_data = Some(Self::i8_slice_to_bf16_shared(shape, data, scale));
                inner.i8_data = None;
                inner.i8_scale = None;
                inner.has_f32_data = false;
                inner.storage_dtype = DType::BF16;
                inner.cache_dirty = false;
                inner.grad = None;
            }
            DType::I8 => {
                if self.try_copy_into_existing_i8_storage_i8(shape, data, scale) {
                    return;
                }
                self.set_i8_storage(Self::i8_slice_to_shared(shape, data), scale);
            }
        }
    }

    pub fn set_array_f16_with_dtype(&self, data: ArrayD<f16>, dtype: DType) {
        let mut inner = self.0.borrow_mut();
        Self::clear_cuda_storage(&mut inner);
        match dtype {
            DType::F32 => {
                inner.data = Self::f16_arc_to_f32(&data.into_shared());
                Self::clear_non_f32_storage(&mut inner);
                inner.has_f32_data = true;
                inner.storage_dtype = DType::F32;
                inner.cache_dirty = false;
            }
            DType::F16 => {
                inner.data = Self::empty_f32_storage();
                inner.f16_data = Some(data.into_shared());
                inner.bf16_data = None;
                inner.i8_data = None;
                inner.i8_scale = None;
                inner.has_f32_data = false;
                inner.storage_dtype = DType::F16;
                inner.cache_dirty = false;
            }
            DType::BF16 => {
                let shared = data.into_shared();
                let f32_data = Self::f16_arc_to_f32(&shared);
                inner.data = Self::empty_f32_storage();
                inner.f16_data = None;
                inner.bf16_data = Some(Self::f32_arc_to_bf16(&f32_data));
                inner.i8_data = None;
                inner.i8_scale = None;
                inner.has_f32_data = false;
                inner.storage_dtype = DType::BF16;
                inner.cache_dirty = false;
            }
            DType::I8 => {
                let shared = data.into_shared();
                let f32_data = Self::f16_arc_to_f32(&shared).to_owned();
                let (i8_data, scale) = Self::quantize_f32_to_dtype(&f32_data, DType::I8, None);
                inner.data = Self::empty_f32_storage();
                inner.f16_data = None;
                inner.bf16_data = None;
                inner.i8_data = Some(i8_data);
                inner.i8_scale = Some(scale);
                inner.has_f32_data = false;
                inner.storage_dtype = DType::I8;
                inner.cache_dirty = false;
            }
        }
        inner.grad = None;
    }

    pub fn set_array_bf16_with_dtype(&self, data: ArrayD<bf16>, dtype: DType) {
        let mut inner = self.0.borrow_mut();
        Self::clear_cuda_storage(&mut inner);
        match dtype {
            DType::F32 => {
                inner.data = Self::bf16_arc_to_f32(&data.into_shared());
                Self::clear_non_f32_storage(&mut inner);
                inner.has_f32_data = true;
                inner.storage_dtype = DType::F32;
                inner.cache_dirty = false;
            }
            DType::F16 => {
                let shared = data.into_shared();
                let f32_data = Self::bf16_arc_to_f32(&shared);
                inner.data = Self::empty_f32_storage();
                inner.f16_data = Some(Self::f32_arc_to_f16(&f32_data));
                inner.bf16_data = None;
                inner.i8_data = None;
                inner.i8_scale = None;
                inner.has_f32_data = false;
                inner.storage_dtype = DType::F16;
                inner.cache_dirty = false;
            }
            DType::BF16 => {
                inner.data = Self::empty_f32_storage();
                inner.f16_data = None;
                inner.bf16_data = Some(data.into_shared());
                inner.i8_data = None;
                inner.i8_scale = None;
                inner.has_f32_data = false;
                inner.storage_dtype = DType::BF16;
                inner.cache_dirty = false;
            }
            DType::I8 => {
                let shared = data.into_shared();
                let f32_data = Self::bf16_arc_to_f32(&shared).to_owned();
                let (i8_data, scale) = Self::quantize_f32_to_dtype(&f32_data, DType::I8, None);
                inner.data = Self::empty_f32_storage();
                inner.f16_data = None;
                inner.bf16_data = None;
                inner.i8_data = Some(i8_data);
                inner.i8_scale = Some(scale);
                inner.has_f32_data = false;
                inner.storage_dtype = DType::I8;
                inner.cache_dirty = false;
            }
        }
        inner.grad = None;
    }

    pub fn set_array_i8_with_dtype(&self, data: ArrayD<i8>, scale: f32, dtype: DType) {
        let mut inner = self.0.borrow_mut();
        Self::clear_cuda_storage(&mut inner);
        match dtype {
            DType::F32 => {
                inner.data = Self::i8_arc_to_f32(&data.into_shared(), scale);
                Self::clear_non_f32_storage(&mut inner);
                inner.has_f32_data = true;
                inner.storage_dtype = DType::F32;
                inner.cache_dirty = false;
            }
            DType::F16 => {
                let shared = data.into_shared();
                inner.data = Self::empty_f32_storage();
                inner.f16_data = Some(Self::f32_arc_to_f16(&Self::i8_arc_to_f32(&shared, scale)));
                inner.bf16_data = None;
                inner.i8_data = None;
                inner.i8_scale = None;
                inner.has_f32_data = false;
                inner.storage_dtype = DType::F16;
                inner.cache_dirty = false;
            }
            DType::BF16 => {
                let shared = data.into_shared();
                inner.data = Self::empty_f32_storage();
                inner.f16_data = None;
                inner.bf16_data = Some(Self::f32_arc_to_bf16(&Self::i8_arc_to_f32(&shared, scale)));
                inner.i8_data = None;
                inner.i8_scale = None;
                inner.has_f32_data = false;
                inner.storage_dtype = DType::BF16;
                inner.cache_dirty = false;
            }
            DType::I8 => {
                inner.data = Self::empty_f32_storage();
                inner.f16_data = None;
                inner.bf16_data = None;
                inner.i8_data = Some(data.into_shared());
                inner.i8_scale = Some(scale);
                inner.has_f32_data = false;
                inner.storage_dtype = DType::I8;
                inner.cache_dirty = false;
            }
        }
        inner.grad = None;
    }

    pub fn export_raw(&self) -> (Vec<usize>, DType, TensorRawData) {
        let inner = self.0.borrow();
        match inner.storage_dtype {
            DType::F32 => (
                inner.data.shape().to_vec(),
                DType::F32,
                TensorRawData::F32(inner.data.iter().copied().collect()),
            ),
            DType::F16 => {
                let f16_data = inner
                    .f16_data
                    .as_ref()
                    .expect("F16 tensor missing f16 storage");
                (
                    f16_data.shape().to_vec(),
                    DType::F16,
                    TensorRawData::F16(f16_data.iter().map(|v| v.to_bits()).collect()),
                )
            }
            DType::BF16 => {
                let bf16_data = inner
                    .bf16_data
                    .as_ref()
                    .expect("BF16 tensor missing bf16 storage");
                (
                    bf16_data.shape().to_vec(),
                    DType::BF16,
                    TensorRawData::BF16(bf16_data.iter().map(|v| v.to_bits()).collect()),
                )
            }
            DType::I8 => {
                let i8_data = inner
                    .i8_data
                    .as_ref()
                    .expect("I8 tensor missing i8 storage");
                (
                    i8_data.shape().to_vec(),
                    DType::I8,
                    TensorRawData::I8 {
                        values: i8_data.iter().copied().collect(),
                        scale: inner
                            .i8_scale
                            .expect("I8 tensor missing quantization scale"),
                    },
                )
            }
        }
    }

    pub fn import_raw(
        &self,
        shape: Vec<usize>,
        dtype: DType,
        raw: TensorRawData,
    ) -> Result<(), String> {
        match raw {
            TensorRawData::F32(values) => {
                if dtype != DType::F32 {
                    return Err(format!(
                        "Raw checkpoint payload mismatch: dtype {:?} with f32 data",
                        dtype
                    ));
                }
                let data = Array::from_shape_vec(IxDyn(&shape), values)
                    .map_err(|e| format!("Checkpoint shape mismatch for f32 payload: {}", e))?;
                self.set_array_f32_with_dtype(data, DType::F32);
            }
            TensorRawData::F16(values) => {
                if dtype != DType::F16 {
                    return Err(format!(
                        "Raw checkpoint payload mismatch: dtype {:?} with f16 data",
                        dtype
                    ));
                }
                let f16_values = values.into_iter().map(f16::from_bits).collect::<Vec<_>>();
                let data = Array::from_shape_vec(IxDyn(&shape), f16_values)
                    .map_err(|e| format!("Checkpoint shape mismatch for f16 payload: {}", e))?;
                self.set_array_f16_with_dtype(data, DType::F16);
            }
            TensorRawData::BF16(values) => {
                if dtype != DType::BF16 {
                    return Err(format!(
                        "Raw checkpoint payload mismatch: dtype {:?} with bf16 data",
                        dtype
                    ));
                }
                let bf16_values = values.into_iter().map(bf16::from_bits).collect::<Vec<_>>();
                let data = Array::from_shape_vec(IxDyn(&shape), bf16_values)
                    .map_err(|e| format!("Checkpoint shape mismatch for bf16 payload: {}", e))?;
                self.set_array_bf16_with_dtype(data, DType::BF16);
            }
            TensorRawData::I8 { values, scale } => {
                if dtype != DType::I8 {
                    return Err(format!(
                        "Raw checkpoint payload mismatch: dtype {:?} with i8 data",
                        dtype
                    ));
                }
                let data = Array::from_shape_vec(IxDyn(&shape), values)
                    .map_err(|e| format!("Checkpoint shape mismatch for i8 payload: {}", e))?;
                self.set_array_i8_with_dtype(data, scale, DType::I8);
            }
        }
        Ok(())
    }

    pub fn with_storage_view_preferring<R>(
        &self,
        preference: StoragePreference,
        f: impl FnOnce(TensorStorageView<'_>) -> R,
    ) -> R {
        if matches!(preference, StoragePreference::F32Compute) {
            {
                let inner = self.0.borrow();
                if inner.cuda_f32_data.is_some() && !inner.has_f32_data {
                    drop(inner);
                    self.ensure_f32_data(false);
                }
            }

            let should_materialize_parameter_cache = {
                let inner = self.0.borrow();
                inner.is_parameter
                    && allow_parameter_dtype_copies()
                    && inner.storage_dtype != DType::F32
            };

            if should_materialize_parameter_cache {
                self.ensure_f32_data(false);
            }

            let inner = self.0.borrow();
            if inner.has_f32_data {
                return f(TensorStorageView::F32(inner.data.view()));
            }
            if let Some(f16_data) = inner.f16_data.clone() {
                drop(inner);
                return Self::with_f16_compute_view(&f16_data, |view| {
                    f(TensorStorageView::F32(view))
                });
            }
            if let Some(bf16_data) = inner.bf16_data.clone() {
                drop(inner);
                return Self::with_bf16_compute_view(&bf16_data, |view| {
                    f(TensorStorageView::F32(view))
                });
            }
            if let Some(i8_data) = inner.i8_data.clone() {
                let scale = inner
                    .i8_scale
                    .expect("I8 tensor missing quantization scale");
                drop(inner);
                return Self::with_i8_compute_view(&i8_data, scale, |view| {
                    f(TensorStorageView::F32(view))
                });
            }
            return f(TensorStorageView::F32(inner.data.view()));
        }

        let should_ensure_f32 = {
            let inner = self.0.borrow();
            match preference {
                StoragePreference::Auto => {
                    (inner.is_parameter
                        && allow_parameter_dtype_copies()
                        && inner.storage_dtype != DType::F32)
                        || (inner.storage_dtype == DType::F32 && !inner.has_f32_data)
                        || (inner.cuda_f32_data.is_some()
                            && ((inner.storage_dtype == DType::F16 && inner.f16_data.is_none())
                                || (inner.storage_dtype == DType::BF16
                                    && inner.bf16_data.is_none())))
                }
                StoragePreference::Native => {
                    (inner.storage_dtype == DType::F32 && !inner.has_f32_data)
                        || (inner.cuda_f32_data.is_some()
                            && ((inner.storage_dtype == DType::F16 && inner.f16_data.is_none())
                                || (inner.storage_dtype == DType::BF16
                                    && inner.bf16_data.is_none())))
                }
                StoragePreference::F32Compute => unreachable!("handled above"),
            }
        };

        if should_ensure_f32 {
            self.ensure_f32_data(false);
        }

        let inner = self.0.borrow();
        if matches!(preference, StoragePreference::Auto)
            && inner.is_parameter
            && allow_parameter_dtype_copies()
            && inner.has_f32_data
        {
            return f(TensorStorageView::F32(inner.data.view()));
        }

        if inner.storage_dtype == DType::F32 {
            return f(TensorStorageView::F32(inner.data.view()));
        }

        if inner.cache_dirty {
            return f(TensorStorageView::F32(inner.data.view()));
        }

        if let Some(f16_data) = inner.f16_data.as_ref() {
            return f(TensorStorageView::F16(f16_data.view()));
        }

        if let Some(bf16_data) = inner.bf16_data.as_ref() {
            return f(TensorStorageView::BF16(bf16_data.view()));
        }

        if let Some(i8_data) = inner.i8_data.clone() {
            let scale = inner
                .i8_scale
                .expect("I8 tensor missing quantization scale");
            drop(inner);
            return Self::with_i8_compute_view(&i8_data, scale, |view| {
                f(TensorStorageView::F32(view))
            });
        } else {
            f(TensorStorageView::F32(inner.data.view()))
        }
    }

    pub fn with_storage_view<R>(&self, f: impl FnOnce(TensorStorageView<'_>) -> R) -> R {
        self.with_storage_view_preferring(StoragePreference::Auto, f)
    }

    pub fn with_storage_view_for_input_dtype<R>(
        &self,
        input_dtype: DType,
        f: impl FnOnce(TensorStorageView<'_>) -> R,
    ) -> R {
        self.with_storage_view_for_input_dtype_and_route(
            input_dtype,
            KernelRouteClass::DecodeKernel,
            f,
        )
    }

    pub fn with_storage_view_for_input_dtype_and_route<R>(
        &self,
        input_dtype: DType,
        route: KernelRouteClass,
        f: impl FnOnce(TensorStorageView<'_>) -> R,
    ) -> R {
        let preference = preferred_parameter_storage_for_route(input_dtype, self.dtype(), route);
        self.with_storage_view_preferring(preference, f)
    }

    pub fn with_native_storage_view_mut<R>(
        &self,
        f: impl FnOnce(TensorStorageViewMut<'_>) -> R,
    ) -> R {
        {
            let inner = self.0.borrow();
            match inner.storage_dtype {
                DType::F32 if !inner.has_f32_data => {
                    drop(inner);
                    self.ensure_f32_data(false);
                }
                DType::F16 if inner.f16_data.is_none() => {
                    drop(inner);
                    self.ensure_f32_data(false);
                    let mut inner = self.0.borrow_mut();
                    inner.f16_data = Some(Self::f32_arc_to_f16(&inner.data));
                    inner.data = Self::empty_f32_storage();
                    inner.has_f32_data = false;
                    inner.cache_dirty = false;
                }
                DType::BF16 if inner.bf16_data.is_none() => {
                    drop(inner);
                    self.ensure_f32_data(false);
                    let mut inner = self.0.borrow_mut();
                    inner.bf16_data = Some(Self::f32_arc_to_bf16(&inner.data));
                    inner.data = Self::empty_f32_storage();
                    inner.has_f32_data = false;
                    inner.cache_dirty = false;
                }
                DType::I8 if inner.i8_data.is_none() => {
                    drop(inner);
                    self.ensure_f32_data(false);
                    let mut inner = self.0.borrow_mut();
                    let (i8_data, scale) =
                        Self::quantize_f32_to_dtype(&inner.data.to_owned(), DType::I8, None);
                    inner.i8_data = Some(i8_data);
                    inner.i8_scale = Some(scale);
                    inner.data = Self::empty_f32_storage();
                    inner.has_f32_data = false;
                    inner.cache_dirty = false;
                }
                _ => {}
            }
        }

        let mut inner = self.0.borrow_mut();
        Self::clear_cuda_storage(&mut inner);
        match inner.storage_dtype {
            DType::F32 => f(TensorStorageViewMut::F32(inner.data.view_mut())),
            DType::F16 => {
                let view = inner
                    .f16_data
                    .as_mut()
                    .expect("F16 tensor missing f16 storage")
                    .view_mut();
                f(TensorStorageViewMut::F16(view))
            }
            DType::BF16 => {
                let view = inner
                    .bf16_data
                    .as_mut()
                    .expect("BF16 tensor missing bf16 storage")
                    .view_mut();
                f(TensorStorageViewMut::BF16(view))
            }
            DType::I8 => {
                let scale = inner
                    .i8_scale
                    .expect("I8 tensor missing quantization scale");
                let view = inner
                    .i8_data
                    .as_mut()
                    .expect("I8 tensor missing i8 storage")
                    .view_mut();
                f(TensorStorageViewMut::I8(view, scale))
            }
        }
    }

    pub fn op_data(
        data: ArcArray<f32, IxDyn>,
        parents: Vec<Tensor>,
        backward_op: Option<Rc<dyn Fn(&ArrayViewD<f32>)>>,
        requires_grad: bool,
    ) -> TensorData {
        TensorData {
            data,
            f16_data: None,
            bf16_data: None,
            i8_data: None,
            cuda_f32_data: None,
            i8_scale: None,
            has_f32_data: true,
            storage_dtype: DType::F32,
            cache_dirty: false,
            is_parameter: false,
            grad: None,
            cuda_f32_grad: None,
            parents,
            backward_op,
            requires_grad,
            device: Device::Cpu,
        }
    }

    // 默认构造叶子张量：
    // - 推理模式/no_grad 下：requires_grad=false
    // - 否则：requires_grad=true（更适合训练时手工造张量）
    pub fn new(data: ArrayD<f32>) -> Self {
        let req = !is_no_grad();
        Tensor(Rc::new(RefCell::new(Self::build_tensor_data(
            DType::F32,
            data,
            req,
            false,
        ))))
    }

    pub fn new_with_dtype(data: ArrayD<f32>, dtype: DType) -> Self {
        let req = !is_no_grad();
        Tensor(Rc::new(RefCell::new(Self::build_tensor_data(
            dtype, data, req, false,
        ))))
    }

    // 获取数据的只读引用（零拷贝）
    pub fn data_ref(&self) -> Ref<'_, ArcArray<f32, IxDyn>> {
        self.ensure_f32_data(false);
        let borrow = self.0.borrow();
        Ref::map(borrow, |t| &t.data)
    }

    // 获取梯度的只读引用（零拷贝）
    pub fn grad_ref(&self) -> Ref<'_, Option<ArcArray<f32, IxDyn>>> {
        let borrow = self.0.borrow();
        Ref::map(borrow, |t| &t.grad)
    }

    #[allow(dead_code)]
    pub(crate) fn has_host_grad(&self) -> bool {
        self.0.borrow().grad.is_some()
    }

    // 获取数据的可变引用
    pub fn data_mut(&self) -> RefMut<'_, ArcArray<f32, IxDyn>> {
        self.ensure_f32_data(true);
        let borrow = self.0.borrow_mut();
        RefMut::map(borrow, |t| &mut t.data)
    }

    // 获取梯度的可变引用
    pub fn grad_mut(&self) -> RefMut<'_, Option<ArcArray<f32, IxDyn>>> {
        let borrow = self.0.borrow_mut();
        RefMut::map(borrow, |t| &mut t.grad)
    }

    pub fn data(&self) -> ArrayD<f32> {
        self.data_ref().to_owned()
    }

    // 快路径：返回共享数据（clone 仅增加引用计数，不复制）
    pub fn data_arc(&self) -> ArcArray<f32, IxDyn> {
        self.data_ref().clone()
    }

    // 慢路径：返回 owned 的 grad（会拷贝）
    pub fn grad(&self) -> Option<ArrayD<f32>> {
        if self.0.borrow().grad.is_none() {
            self.materialize_cuda_grad_to_host();
        }
        self.0.borrow().grad.as_ref().map(|g| g.to_owned())
    }

    // 快路径：返回共享 grad（clone 仅增 refcount，不复制）
    pub fn grad_arc(&self) -> Option<ArcArray<f32, IxDyn>> {
        if self.0.borrow().grad.is_none() {
            self.materialize_cuda_grad_to_host();
        }
        self.0.borrow().grad.clone()
    }

    #[allow(dead_code)]
    pub(crate) fn cloned_cuda_f32_grad(&self) -> Option<crate::ops::cuda::CudaBuffer> {
        if self.device() != Device::Cuda {
            return None;
        }
        self.0.borrow().cuda_f32_grad.clone()
    }

    #[inline]
    pub fn device(&self) -> Device {
        self.0.borrow().device
    }

    #[inline]
    pub fn is_cuda(&self) -> bool {
        self.device() == Device::Cuda
    }

    #[inline]
    pub fn is_cpu(&self) -> bool {
        self.device() == Device::Cpu
    }

    pub(crate) fn set_device_inplace(&self, device: Device) {
        let mut inner = self.0.borrow_mut();
        inner.device = device;
        if device == Device::Cpu {
            Self::clear_cuda_storage(&mut inner);
        }
    }

    pub fn to_device_inplace(&self, device: Device) {
        match device {
            Device::Cpu => self.set_device_inplace(Device::Cpu),
            Device::Cuda => {
                assert!(
                    crate::ops::cuda::is_available(),
                    "CUDA is not available. Rebuild with `--features cuda` and ensure the NVIDIA runtime is installed."
                );
                if self.is_cuda() && self.0.borrow().cuda_f32_data.is_some() {
                    return;
                }
                if self.len() == 0 {
                    let mut inner = self.0.borrow_mut();
                    inner.device = Device::Cuda;
                    inner.cuda_f32_data = None;
                    return;
                }
                let host_data = self.storage_as_f32_vec();
                let buffer = crate::ops::cuda::upload_f32(&host_data)
                    .unwrap_or_else(|err| panic!("Failed to upload tensor to CUDA: {}", err));
                let mut inner = self.0.borrow_mut();
                inner.device = Device::Cuda;
                inner.cuda_f32_data = Some(buffer);
            }
        }
    }

    pub fn to_cpu_inplace(&self) {
        self.to_device_inplace(Device::Cpu);
    }

    pub fn to_cuda_inplace(&self) {
        self.to_device_inplace(Device::Cuda);
    }

    fn clone_storage_with_device(&self, device: Device, requires_grad: bool) -> Tensor {
        match self.dtype() {
            DType::F32 => {
                let inner = self.0.borrow();
                let tensor = Tensor(Rc::new(RefCell::new(TensorData {
                    data: inner.data.clone(),
                    f16_data: None,
                    bf16_data: None,
                    i8_data: None,
                    cuda_f32_data: None,
                    i8_scale: None,
                    has_f32_data: true,
                    storage_dtype: DType::F32,
                    cache_dirty: false,
                    is_parameter: inner.is_parameter,
                    grad: None,
                    cuda_f32_grad: None,
                    parents: vec![],
                    backward_op: None,
                    requires_grad,
                    device: Device::Cpu,
                })));
                drop(inner);
                if device == Device::Cuda {
                    tensor.to_cuda_inplace();
                }
                tensor
            }
            DType::F16 => {
                let inner = self.0.borrow();
                let tensor = Tensor(Rc::new(RefCell::new(TensorData {
                    data: Self::empty_f32_storage(),
                    f16_data: Some(
                        inner
                            .f16_data
                            .as_ref()
                            .expect("F16 tensor missing f16 storage")
                            .clone(),
                    ),
                    bf16_data: None,
                    i8_data: None,
                    cuda_f32_data: None,
                    i8_scale: None,
                    has_f32_data: false,
                    storage_dtype: DType::F16,
                    cache_dirty: false,
                    is_parameter: inner.is_parameter,
                    grad: None,
                    cuda_f32_grad: None,
                    parents: vec![],
                    backward_op: None,
                    requires_grad,
                    device: Device::Cpu,
                })));
                drop(inner);
                if device == Device::Cuda {
                    tensor.to_cuda_inplace();
                }
                tensor
            }
            DType::BF16 => {
                let inner = self.0.borrow();
                let tensor = Tensor(Rc::new(RefCell::new(TensorData {
                    data: Self::empty_f32_storage(),
                    f16_data: None,
                    bf16_data: Some(
                        inner
                            .bf16_data
                            .as_ref()
                            .expect("BF16 tensor missing bf16 storage")
                            .clone(),
                    ),
                    i8_data: None,
                    cuda_f32_data: None,
                    i8_scale: None,
                    has_f32_data: false,
                    storage_dtype: DType::BF16,
                    cache_dirty: false,
                    is_parameter: inner.is_parameter,
                    grad: None,
                    cuda_f32_grad: None,
                    parents: vec![],
                    backward_op: None,
                    requires_grad,
                    device: Device::Cpu,
                })));
                drop(inner);
                if device == Device::Cuda {
                    tensor.to_cuda_inplace();
                }
                tensor
            }
            DType::I8 => {
                let inner = self.0.borrow();
                let tensor = Tensor(Rc::new(RefCell::new(TensorData {
                    data: Self::empty_f32_storage(),
                    f16_data: None,
                    bf16_data: None,
                    i8_data: Some(
                        inner
                            .i8_data
                            .as_ref()
                            .expect("I8 tensor missing i8 storage")
                            .clone(),
                    ),
                    cuda_f32_data: None,
                    i8_scale: Some(
                        inner
                            .i8_scale
                            .expect("I8 tensor missing quantization scale"),
                    ),
                    has_f32_data: false,
                    storage_dtype: DType::I8,
                    cache_dirty: false,
                    is_parameter: inner.is_parameter,
                    grad: None,
                    cuda_f32_grad: None,
                    parents: vec![],
                    backward_op: None,
                    requires_grad,
                    device: Device::Cpu,
                })));
                drop(inner);
                if device == Device::Cuda {
                    tensor.to_cuda_inplace();
                }
                tensor
            }
        }
    }

    pub fn to_device(&self, device: Device) -> Tensor {
        if self.device() == device {
            return self.clone_storage_with_device(device, self.requires_grad());
        }
        self.clone_storage_with_device(device, self.requires_grad())
    }

    pub fn to_cpu(&self) -> Tensor {
        self.to_device(Device::Cpu)
    }

    pub fn to_cuda(&self) -> Tensor {
        self.to_device(Device::Cuda)
    }

    pub fn cpu(&self) -> Tensor {
        self.to_cpu()
    }

    pub fn cuda(&self) -> Tensor {
        self.to_cuda()
    }

    pub fn sum(&self) -> Tensor {
        crate::ops::arithmetic::sum(self)
    }

    // 创建叶子张量（显式指定 requires_grad）
    pub fn from_data_with_grad_flag(data: ArrayD<f32>, requires_grad: bool) -> Tensor {
        Tensor(Rc::new(RefCell::new(Self::build_tensor_data(
            DType::F32,
            data,
            requires_grad,
            false,
        ))))
    }

    // 创建叶子张量：根据 is_no_grad() 自动决定 requires_grad
    pub fn from_data(data: ArrayD<f32>) -> Tensor {
        let req = !is_no_grad();
        Tensor::from_data_with_grad_flag(data, req)
    }

    // 推理/常量：不需要梯度
    pub fn from_data_no_grad(data: ArcArray<f32, IxDyn>) -> Tensor {
        Tensor(Rc::new(RefCell::new(TensorData {
            data,
            f16_data: None,
            bf16_data: None,
            i8_data: None,
            cuda_f32_data: None,
            i8_scale: None,
            has_f32_data: true,
            storage_dtype: DType::F32,
            cache_dirty: false,
            is_parameter: false,
            grad: None,
            cuda_f32_grad: None,
            parents: vec![],
            backward_op: None,
            requires_grad: false,
            device: Device::Cpu,
        })))
    }

    pub(crate) fn from_f16_data_no_grad(data: ArcArray<f16, IxDyn>) -> Tensor {
        Tensor(Rc::new(RefCell::new(TensorData {
            data: Self::empty_f32_storage(),
            f16_data: Some(data),
            bf16_data: None,
            i8_data: None,
            cuda_f32_data: None,
            i8_scale: None,
            has_f32_data: false,
            storage_dtype: DType::F16,
            cache_dirty: false,
            is_parameter: false,
            grad: None,
            cuda_f32_grad: None,
            parents: vec![],
            backward_op: None,
            requires_grad: false,
            device: Device::Cpu,
        })))
    }

    pub(crate) fn from_bf16_data_no_grad(data: ArcArray<bf16, IxDyn>) -> Tensor {
        Tensor(Rc::new(RefCell::new(TensorData {
            data: Self::empty_f32_storage(),
            f16_data: None,
            bf16_data: Some(data),
            i8_data: None,
            cuda_f32_data: None,
            i8_scale: None,
            has_f32_data: false,
            storage_dtype: DType::BF16,
            cache_dirty: false,
            is_parameter: false,
            grad: None,
            cuda_f32_grad: None,
            parents: vec![],
            backward_op: None,
            requires_grad: false,
            device: Device::Cpu,
        })))
    }

    pub(crate) fn from_i8_data_no_grad(data: ArcArray<i8, IxDyn>, scale: f32) -> Tensor {
        Tensor(Rc::new(RefCell::new(TensorData {
            data: Self::empty_f32_storage(),
            f16_data: None,
            bf16_data: None,
            i8_data: Some(data),
            cuda_f32_data: None,
            i8_scale: Some(scale),
            has_f32_data: false,
            storage_dtype: DType::I8,
            cache_dirty: false,
            is_parameter: false,
            grad: None,
            cuda_f32_grad: None,
            parents: vec![],
            backward_op: None,
            requires_grad: false,
            device: Device::Cpu,
        })))
    }

    pub(crate) fn from_f32_data_no_grad_with_dtype(data: ArrayD<f32>, dtype: DType) -> Tensor {
        match dtype {
            DType::F32 => Tensor::from_array_no_grad(data),
            DType::F16 => Tensor::from_f16_data_no_grad(data.mapv(f16::from_f32).into_shared()),
            DType::BF16 => Tensor::from_bf16_data_no_grad(data.mapv(bf16::from_f32).into_shared()),
            DType::I8 => {
                let (i8_data, scale) = Self::quantize_f32_to_dtype(&data, DType::I8, None);
                Tensor::from_i8_data_no_grad(i8_data, scale)
            }
        }
    }

    pub(crate) fn from_f32_data_no_grad_with_device_dtype(
        data: ArrayD<f32>,
        dtype: DType,
        device: Device,
    ) -> Tensor {
        Self::from_f32_data_no_grad_with_device_dtype_and_cuda_buffer(data, dtype, device, None)
    }

    pub(crate) fn from_f32_data_no_grad_with_device_dtype_and_cuda_buffer(
        data: ArrayD<f32>,
        dtype: DType,
        device: Device,
        cuda_f32_data: Option<crate::ops::cuda::CudaBuffer>,
    ) -> Tensor {
        let tensor = Self::from_f32_data_no_grad_with_dtype(data, dtype);
        match (device, cuda_f32_data) {
            (Device::Cpu, _) => tensor.to_cpu_inplace(),
            (Device::Cuda, Some(buffer)) => tensor.set_cuda_f32_buffer_inplace(buffer),
            (Device::Cuda, None) => tensor.to_cuda_inplace(),
        }
        tensor
    }

    pub(crate) fn from_cuda_f32_buffer_no_host(
        shape: &[usize],
        buffer: crate::ops::cuda::CudaBuffer,
        device: Device,
    ) -> Tensor {
        assert_eq!(
            device,
            Device::Cuda,
            "from_cuda_f32_buffer_no_host currently expects CUDA device"
        );
        Tensor(Rc::new(RefCell::new(TensorData {
            data: ArrayD::<f32>::zeros(IxDyn(shape)).into_shared(),
            f16_data: None,
            bf16_data: None,
            i8_data: None,
            cuda_f32_data: Some(buffer),
            i8_scale: None,
            has_f32_data: false,
            storage_dtype: DType::F32,
            cache_dirty: false,
            is_parameter: false,
            grad: None,
            cuda_f32_grad: None,
            parents: vec![],
            backward_op: None,
            requires_grad: false,
            device,
        })))
    }

    pub(crate) fn from_cuda_f32_buffer_no_host_with_dtype(
        shape: &[usize],
        buffer: crate::ops::cuda::CudaBuffer,
        device: Device,
        dtype: DType,
    ) -> Tensor {
        if dtype == DType::F32 {
            return Self::from_cuda_f32_buffer_no_host(shape, buffer, device);
        }
        assert_eq!(
            device,
            Device::Cuda,
            "from_cuda_f32_buffer_no_host_with_dtype currently expects CUDA device"
        );
        Tensor(Rc::new(RefCell::new(TensorData {
            data: ArrayD::<f32>::zeros(IxDyn(shape)).into_shared(),
            f16_data: None,
            bf16_data: None,
            i8_data: None,
            cuda_f32_data: Some(buffer),
            i8_scale: None,
            has_f32_data: false,
            storage_dtype: dtype,
            cache_dirty: false,
            is_parameter: false,
            grad: None,
            cuda_f32_grad: None,
            parents: vec![],
            backward_op: None,
            requires_grad: false,
            device,
        })))
    }

    pub(crate) fn from_shared_f32_no_grad_with_device(
        data: ArcArray<f32, IxDyn>,
        device: Device,
    ) -> Tensor {
        let tensor = Self::from_data_no_grad(data);
        tensor.to_device_inplace(device);
        tensor
    }

    pub(crate) fn from_shared_f16_no_grad_with_device(
        data: ArcArray<f16, IxDyn>,
        device: Device,
    ) -> Tensor {
        let tensor = Self::from_f16_data_no_grad(data);
        tensor.to_device_inplace(device);
        tensor
    }

    pub(crate) fn from_shared_bf16_no_grad_with_device(
        data: ArcArray<bf16, IxDyn>,
        device: Device,
    ) -> Tensor {
        let tensor = Self::from_bf16_data_no_grad(data);
        tensor.to_device_inplace(device);
        tensor
    }

    pub(crate) fn from_shared_i8_no_grad_with_device(
        data: ArcArray<i8, IxDyn>,
        scale: f32,
        device: Device,
    ) -> Tensor {
        let tensor = Self::from_i8_data_no_grad(data, scale);
        tensor.to_device_inplace(device);
        tensor
    }

    pub(crate) fn native_storage_owned(&self) -> TensorStorageOwned {
        let inner = self.0.borrow();

        if inner.storage_dtype == DType::F16 {
            if let Some(f16_data) = inner.f16_data.as_ref() {
                return TensorStorageOwned::F16(f16_data.clone());
            }
        }
        if inner.storage_dtype == DType::BF16 {
            if let Some(bf16_data) = inner.bf16_data.as_ref() {
                return TensorStorageOwned::BF16(bf16_data.clone());
            }
        }
        if inner.storage_dtype == DType::I8 {
            if let Some(i8_data) = inner.i8_data.as_ref() {
                return TensorStorageOwned::I8(
                    i8_data.clone(),
                    inner
                        .i8_scale
                        .expect("I8 tensor missing quantization scale"),
                );
            }
        }

        if inner.has_f32_data {
            return TensorStorageOwned::F32(inner.data.clone());
        }

        if let Some(f16_data) = inner.f16_data.as_ref() {
            TensorStorageOwned::F16(f16_data.clone())
        } else if let Some(bf16_data) = inner.bf16_data.as_ref() {
            TensorStorageOwned::BF16(bf16_data.clone())
        } else if let Some(i8_data) = inner.i8_data.as_ref() {
            TensorStorageOwned::I8(
                i8_data.clone(),
                inner
                    .i8_scale
                    .expect("I8 tensor missing quantization scale"),
            )
        } else {
            TensorStorageOwned::F32(inner.data.clone())
        }
    }

    // 兼容旧接口：传入 ArrayD 作为常量
    pub fn from_array_no_grad(data: ArrayD<f32>) -> Tensor {
        Tensor::from_data_no_grad(data.into_shared())
    }

    // 训练参数：需要梯度（叶子）
    pub fn parameter(data: ArrayD<f32>) -> Tensor {
        let quantization = default_parameter_quantization();
        if quantization.is_enabled() {
            return Self::parameter_with_quantization(data, quantization);
        }

        let dtype = default_parameter_dtype();
        Self::parameter_with_dtype(data, dtype)
    }

    // 训练参数：显式指定 dtype，优先级高于全局默认参数 dtype
    pub fn parameter_with_dtype(data: ArrayD<f32>, dtype: DType) -> Tensor {
        Tensor(Rc::new(RefCell::new(Self::build_tensor_data(
            dtype, data, true, true,
        ))))
    }

    pub fn parameter_placeholder_with_dtype(shape: &[usize], dtype: DType) -> Tensor {
        Tensor(Rc::new(RefCell::new(Self::empty_tensor_data_for_shape(
            shape, dtype, true, true, None,
        ))))
    }

    pub fn parameter_with_quantization(
        data: ArrayD<f32>,
        quantization: ParameterQuantization,
    ) -> Tensor {
        if !quantization.is_enabled() {
            return Self::parameter_with_dtype(data, default_parameter_dtype());
        }

        let dtype = quantization
            .storage_dtype()
            .expect("enabled quantization must provide storage dtype");
        match dtype {
            DType::I8 => {
                let (i8_data, scale) =
                    Self::quantize_f32_to_dtype(&data, dtype, quantization.scale_override());
                Tensor(Rc::new(RefCell::new(TensorData {
                    data: Self::empty_f32_storage(),
                    f16_data: None,
                    bf16_data: None,
                    i8_data: Some(i8_data),
                    cuda_f32_data: None,
                    i8_scale: Some(scale),
                    has_f32_data: false,
                    storage_dtype: dtype,
                    cache_dirty: false,
                    is_parameter: true,
                    grad: None,
                    cuda_f32_grad: None,
                    parents: Vec::new(),
                    backward_op: None,
                    requires_grad: true,
                    device: Device::Cpu,
                })))
            }
            other => {
                panic!(
                    "quantized storage dtype {:?} is not implemented yet; currently only I8 is supported",
                    other
                );
            }
        }
    }

    pub fn parameter_placeholder_with_quantization(
        shape: &[usize],
        quantization: ParameterQuantization,
    ) -> Tensor {
        if !quantization.is_enabled() {
            return Self::parameter_placeholder_with_dtype(shape, default_parameter_dtype());
        }

        let dtype = quantization
            .storage_dtype()
            .expect("enabled quantization must provide storage dtype");
        Tensor(Rc::new(RefCell::new(Self::empty_tensor_data_for_shape(
            shape,
            dtype,
            true,
            true,
            quantization.scale_override(),
        ))))
    }

    pub fn quantize_inplace(&self, dtype: DType) {
        assert!(
            dtype.is_integer(),
            "quantize_inplace currently expects integer dtype, got {:?}",
            dtype
        );
        self.cast_inplace(dtype);
    }

    pub fn quantize_inplace_with_quantization(&self, quantization: ParameterQuantization) {
        if !quantization.is_enabled() {
            return;
        }
        let dtype = quantization
            .storage_dtype()
            .expect("enabled quantization must provide storage dtype");
        assert!(
            dtype.is_integer(),
            "quantize_inplace_with_quantization currently expects integer dtype, got {:?}",
            dtype
        );
        self.ensure_f32_data(false);
        let data = self.data_ref().to_owned();
        self.set_array_f32_with_quantization(data, quantization);
    }

    pub fn dequantize_inplace(&self, dtype: DType) {
        assert!(
            dtype.is_float(),
            "dequantize_inplace currently expects floating dtype, got {:?}",
            dtype
        );
        self.cast_inplace(dtype);
    }

    #[inline]
    pub fn requires_grad(&self) -> bool {
        self.0.borrow().requires_grad
    }

    pub fn zero_grad(&self) {
        let mut inner = self.0.borrow_mut();
        inner.grad = None;
        inner.cuda_f32_grad = None;
    }

    pub fn reshape(&self, shape: Vec<i32>) -> Tensor {
        crate::ops::shape::reshape(self, shape)
    }

    pub fn permute(&self, axes: Vec<usize>) -> Tensor {
        crate::ops::shape::permute(self, axes)
    }

    pub fn transpose(&self, dim0: usize, dim1: usize) -> Tensor {
        let ndim = self.ndim();
        let mut axes: Vec<usize> = (0..ndim).collect();
        axes.swap(dim0, dim1);
        self.permute(axes)
    }

    pub fn add_grad(&self, grad: ArrayD<f32>) {
        self.add_grad_with_cuda_buffer(grad, None);
    }

    pub(crate) fn add_grad_with_cuda_buffer(
        &self,
        grad: ArrayD<f32>,
        cuda_grad: Option<crate::ops::cuda::CudaBuffer>,
    ) {
        let mut inner = self.0.borrow_mut();

        if Self::logical_shape(&inner) != grad.shape() {
            panic!(
                "CRITICAL: Gradient shape mismatch!\nParameter Shape: {:?}\nGradient Shape: {:?}\nHint: Check ops/arithmetic.rs reduce_gradient logic.",
                Self::logical_shape(&inner),
                grad.shape()
            );
        }

        if let Some(existing) = &inner.grad {
            // existing 为共享 ArcArray；累加时会产生一个 owned ArrayD，然后再转回 shared。
            let summed = existing.to_owned() + &grad;
            inner.grad = Some(summed.into_shared());
        } else {
            inner.grad = Some(grad.into_shared());
        }

        if inner.device != Device::Cuda {
            inner.cuda_f32_grad = None;
            return;
        }

        let grad_len = Self::logical_shape(&inner).iter().product::<usize>();
        if let Some(new_buffer) = cuda_grad {
            if new_buffer.len() == grad_len {
                inner.cuda_f32_grad = match inner.cuda_f32_grad.as_ref() {
                    Some(existing_buffer) if existing_buffer.len() == grad_len => {
                        match crate::ops::cuda::binary_f32_buffer(
                            existing_buffer,
                            &new_buffer,
                            crate::ops::cuda::BinaryOp::Add,
                        ) {
                            Ok(buffer) => Some(buffer),
                            Err(_) => Some(new_buffer),
                        }
                    }
                    _ => Some(new_buffer),
                };
                return;
            }
        }

        let host_grad = inner
            .grad
            .as_ref()
            .expect("gradient host data should exist after add_grad")
            .iter()
            .copied()
            .collect::<Vec<_>>();
        inner.cuda_f32_grad = crate::ops::cuda::upload_f32(&host_grad).ok();
    }

    #[allow(dead_code)]
    pub(crate) fn add_cuda_grad_buffer_only(&self, cuda_grad: crate::ops::cuda::CudaBuffer) {
        let mut inner = self.0.borrow_mut();
        assert_eq!(
            inner.device,
            Device::Cuda,
            "add_cuda_grad_buffer_only expects a CUDA tensor"
        );
        let grad_len = Self::logical_shape(&inner).iter().product::<usize>();
        assert_eq!(
            cuda_grad.len(),
            grad_len,
            "CUDA grad length mismatch: expected {}, got {}",
            grad_len,
            cuda_grad.len()
        );

        let existing_cuda_grad = inner.cuda_f32_grad.clone().or_else(|| {
            inner.grad.as_ref().and_then(|grad| {
                let host_grad = grad.iter().copied().collect::<Vec<_>>();
                crate::ops::cuda::upload_f32(&host_grad).ok()
            })
        });

        inner.cuda_f32_grad = match existing_cuda_grad.as_ref() {
            Some(existing_buffer) if existing_buffer.len() == grad_len => {
                match crate::ops::cuda::binary_f32_buffer(
                    existing_buffer,
                    &cuda_grad,
                    crate::ops::cuda::BinaryOp::Add,
                ) {
                    Ok(buffer) => Some(buffer),
                    Err(_) => Some(cuda_grad),
                }
            }
            _ => Some(cuda_grad),
        };
        inner.grad = None;
    }

    fn materialize_cuda_grad_to_host(&self) {
        let (shape, buffer) = {
            let inner = self.0.borrow();
            let Some(buffer) = inner.cuda_f32_grad.clone() else {
                return;
            };
            (Self::logical_shape(&inner).to_vec(), buffer)
        };

        let host = crate::ops::cuda::download_f32(&buffer)
            .unwrap_or_else(|err| panic!("Failed to download CUDA grad to host: {}", err));
        let grad = Array::from_shape_vec(IxDyn(&shape), host)
            .expect("Failed to materialize host grad from CUDA buffer")
            .into_shared();
        self.0.borrow_mut().grad = Some(grad);
    }

    pub fn backward(&self) {
        let mut topo = Vec::new();
        let mut visited = HashSet::new();

        fn build_topo(
            node: &Tensor,
            topo: &mut Vec<Tensor>,
            visited: &mut HashSet<*const TensorData>,
        ) {
            let ptr = node.0.as_ptr() as *const TensorData;
            if visited.contains(&ptr) {
                return;
            }
            visited.insert(ptr);

            for parent in &node.0.borrow().parents {
                build_topo(parent, topo, visited);
            }
            topo.push(node.clone());
        }

        build_topo(self, &mut topo, &mut visited);

        let has_existing_grad = {
            let inner = self.0.borrow();
            inner.grad.is_some() || inner.cuda_f32_grad.is_some()
        };
        if !has_existing_grad {
            let (shape, seed_cuda_only) = {
                let inner = self.0.borrow();
                (
                    Self::logical_shape(&inner).to_vec(),
                    inner.device == Device::Cuda
                        && !inner.has_f32_data
                        && is_strict_device_execution(),
                )
            };
            if seed_cuda_only {
                let len = shape.iter().product::<usize>();
                if len == 0 {
                    self.add_grad(ArrayD::ones(shape));
                } else {
                    let buffer =
                        crate::ops::cuda::fill_scalar_f32_buffer(len, 1.0).unwrap_or_else(|err| {
                            panic!("Failed to create CUDA backward seed: {}", err)
                        });
                    self.add_cuda_grad_buffer_only(buffer);
                }
            } else {
                self.add_grad(ArrayD::ones(shape));
            }
        }

        for node in topo.iter().rev() {
            let (has_cuda_only_grad, op_rc, node_device, node_shape, has_host_f32_data) = {
                let inner = node.0.borrow();
                (
                    inner.grad.is_none() && inner.cuda_f32_grad.is_some(),
                    inner.backward_op.clone(),
                    inner.device,
                    Self::logical_shape(&inner).to_vec(),
                    inner.has_f32_data,
                )
            };

            let Some(op) = op_rc else {
                continue;
            };
            let strict_cuda_placeholder = has_cuda_only_grad
                && node_device == Device::Cuda
                && !has_host_f32_data
                && is_strict_device_execution();
            if has_cuda_only_grad && !strict_cuda_placeholder {
                node.materialize_cuda_grad_to_host();
            }

            let grad_arc = if strict_cuda_placeholder {
                Some(ArrayD::<f32>::zeros(IxDyn(&node_shape)).into_shared())
            } else {
                node.0.borrow().grad.clone()
            };
            if let Some(grad) = grad_arc {
                let grad_view = grad.view();
                op(&grad_view.into_dyn());
            }
        }
    }

    pub fn get_raw_data(&self) -> (Vec<usize>, Vec<f32>) {
        let data = self.data();
        (data.shape().to_vec(), data.iter().copied().collect())
    }

    pub fn take_raw_data(&self) -> (Vec<usize>, Vec<f32>) {
        let (shape, raw_data) = self.get_raw_data();
        let mut inner = self.0.borrow_mut();
        inner.data = Self::empty_f32_storage();
        Self::clear_non_f32_storage(&mut inner);
        Self::clear_cuda_storage(&mut inner);
        inner.has_f32_data = false;
        inner.storage_dtype = DType::F32;
        inner.cache_dirty = false;
        (shape, raw_data)
    }

    pub fn set_raw_data(&self, shape: Vec<usize>, raw_data: Vec<f32>) {
        let new_data = Array::from_shape_vec(shape, raw_data).unwrap().into_dyn();
        self.set_array_f32_with_dtype(new_data, DType::F32);
    }

    // detach：返回一个新 Tensor（数据拷贝），requires_grad=false，且无 parents/backward_op
    pub fn detach(&self) -> Tensor {
        self.clone_storage_with_device(self.device(), false)
    }
}

// 切断梯度流（等价于 t.detach()）
pub fn detach(t: &Tensor) -> Tensor {
    t.detach()
}

pub fn assert_same_device(lhs: &Tensor, rhs: &Tensor, op_name: &str) -> Device {
    let lhs_device = lhs.device();
    let rhs_device = rhs.device();
    assert!(
        lhs_device == rhs_device,
        "{op_name} expects tensors on the same device, got lhs={lhs_device:?}, rhs={rhs_device:?}. Move them with to_cuda()/to_cpu() before combining them."
    );
    lhs_device
}

pub fn assert_native_device_support(device: Device, op_name: &str, cuda_supported: bool) {
    if !is_strict_device_execution() {
        return;
    }

    match device {
        Device::Cpu => {}
        Device::Cuda => {
            assert!(
                cuda_supported,
                "{op_name} does not have a native CUDA implementation yet. This operation would fall back through CPU semantics, which is disallowed while strict device execution is enabled."
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::precision::{
        DType, ParameterQuantization, PrecisionConfig, with_parameter_quantization,
        with_precision_config,
    };

    #[test]
    fn parameter_creation_follows_precision_scope() {
        with_precision_config(
            PrecisionConfig {
                parameter_dtype: DType::BF16,
                runtime_dtype: DType::F32,
                allow_parameter_dtype_copies: false,
            },
            || {
                let param = Tensor::parameter(ArrayD::from_elem(IxDyn(&[2, 3]), 1.0));
                assert_eq!(param.dtype(), DType::BF16);
                param.with_storage_view(|view| match view {
                    TensorStorageView::BF16(view) => assert_eq!(view.shape(), &[2, 3]),
                    TensorStorageView::F16(_) => {
                        panic!("bf16 parameter unexpectedly exposed as f16")
                    }
                    TensorStorageView::F32(_) => {
                        panic!("bf16 parameter unexpectedly exposed as f32")
                    }
                });
            },
        );
    }

    #[test]
    fn no_grad_scope_is_thread_local() {
        set_inference_mode(false);

        let (tx, rx) = std::sync::mpsc::channel();
        let handle = std::thread::spawn(move || {
            let _guard = NoGradGuard::enter();
            tx.send(is_no_grad())
                .expect("send thread-local no_grad state");
            std::thread::sleep(std::time::Duration::from_millis(25));
            tx.send(is_no_grad())
                .expect("send thread-local no_grad state");
        });

        assert!(
            rx.recv().expect("receive spawned thread state"),
            "spawned thread should observe its own no_grad guard"
        );
        assert!(
            !is_no_grad(),
            "main thread should not inherit spawned thread's no_grad guard"
        );
        assert!(
            rx.recv().expect("receive spawned thread state"),
            "spawned thread should remain in no_grad until its guard drops"
        );
        assert!(
            !is_no_grad(),
            "main thread should still remain outside no_grad"
        );

        handle.join().expect("join no_grad thread");
        set_inference_mode(false);
    }

    #[test]
    fn inference_mode_is_thread_local() {
        set_inference_mode(false);

        let (tx, rx) = std::sync::mpsc::channel();
        let handle = std::thread::spawn(move || {
            set_inference_mode(true);
            tx.send(is_inference_mode())
                .expect("send thread-local inference state");
            std::thread::sleep(std::time::Duration::from_millis(25));
            tx.send(is_inference_mode())
                .expect("send thread-local inference state");
            set_inference_mode(false);
        });

        assert!(
            rx.recv().expect("receive spawned thread state"),
            "spawned thread should observe its own inference mode"
        );
        assert!(
            !is_inference_mode(),
            "main thread should not inherit spawned thread's inference mode"
        );
        assert!(
            rx.recv().expect("receive spawned thread state"),
            "spawned thread should remain in inference mode until it resets"
        );
        assert!(
            !is_inference_mode(),
            "main thread should still remain outside inference mode"
        );

        handle.join().expect("join inference thread");
        set_inference_mode(false);
    }

    #[test]
    fn parameter_storage_view_prefers_f32_cache_when_copies_allowed() {
        with_precision_config(
            PrecisionConfig {
                parameter_dtype: DType::BF16,
                runtime_dtype: DType::F32,
                allow_parameter_dtype_copies: true,
            },
            || {
                let param = Tensor::parameter(ArrayD::from_elem(IxDyn(&[4]), 2.0));
                assert_eq!(param.dtype(), DType::BF16);
                param.with_storage_view(|view| match view {
                    TensorStorageView::F32(view) => assert_eq!(view.len(), 4),
                    TensorStorageView::F16(_) => {
                        panic!("bf16 parameter should expose cached f32 view")
                    }
                    TensorStorageView::BF16(_) => {
                        panic!("bf16 parameter should expose cached f32 view")
                    }
                });
                assert_eq!(param.dtype(), DType::BF16, "storage dtype should stay bf16");
            },
        );
    }

    #[test]
    fn native_storage_view_preserves_bf16_parameter_storage() {
        with_precision_config(
            PrecisionConfig {
                parameter_dtype: DType::BF16,
                runtime_dtype: DType::F32,
                allow_parameter_dtype_copies: true,
            },
            || {
                let param = Tensor::parameter(ArrayD::from_elem(IxDyn(&[4]), 3.0));
                param.with_storage_view_preferring(StoragePreference::Native, |view| match view {
                    TensorStorageView::BF16(view) => assert_eq!(view.len(), 4),
                    TensorStorageView::F16(_) => {
                        panic!("native preference should keep bf16 storage")
                    }
                    TensorStorageView::F32(_) => {
                        panic!("native preference should keep bf16 storage")
                    }
                });
            },
        );
    }

    #[test]
    fn input_dtype_dispatch_prefers_native_same_dtype_parameter_view() {
        with_precision_config(
            PrecisionConfig {
                parameter_dtype: DType::BF16,
                runtime_dtype: DType::F32,
                allow_parameter_dtype_copies: true,
            },
            || {
                let param = Tensor::parameter(ArrayD::from_elem(IxDyn(&[4]), 3.0));

                param.with_storage_view_for_input_dtype(DType::BF16, |view| match view {
                    TensorStorageView::BF16(view) => assert_eq!(view.len(), 4),
                    TensorStorageView::F16(_) => {
                        panic!("same-dtype bf16 input should keep native bf16 parameter view")
                    }
                    TensorStorageView::F32(_) => {
                        panic!("same-dtype bf16 input should not be hijacked by cached f32 view")
                    }
                });
            },
        );
    }

    #[test]
    fn input_dtype_dispatch_keeps_cached_f32_view_for_mixed_parameter_use() {
        with_precision_config(
            PrecisionConfig {
                parameter_dtype: DType::BF16,
                runtime_dtype: DType::F32,
                allow_parameter_dtype_copies: true,
            },
            || {
                let param = Tensor::parameter(ArrayD::from_elem(IxDyn(&[4]), 3.0));

                param.with_storage_view_for_input_dtype(DType::F32, |view| {
                    #[cfg(all(feature = "arm64-fp-kernels", target_arch = "aarch64"))]
                    match view {
                        TensorStorageView::BF16(view) => assert_eq!(view.len(), 4),
                        TensorStorageView::F16(_) => {
                            panic!("mixed f32 generic-dispatch on arm should keep native bf16 parameter view")
                        }
                        TensorStorageView::F32(_) => {
                            panic!("mixed f32 generic-dispatch on arm should keep native bf16 parameter view")
                        }
                    }

                    #[cfg(not(all(feature = "arm64-fp-kernels", target_arch = "aarch64")))]
                    match view {
                        TensorStorageView::F32(view) => assert_eq!(view.len(), 4),
                        TensorStorageView::F16(_) => panic!(
                            "mixed f32 input should still be allowed to use cached f32 parameter view"
                        ),
                        TensorStorageView::BF16(_) => panic!(
                            "mixed f32 input should still be allowed to use cached f32 parameter view"
                        ),
                    }
                });
            },
        );
    }

    #[test]
    fn generic_matmul_route_prefers_native_low_precision_parameter_view_on_arm() {
        with_precision_config(
            PrecisionConfig {
                parameter_dtype: DType::BF16,
                runtime_dtype: DType::F32,
                allow_parameter_dtype_copies: true,
            },
            || {
                let param = Tensor::parameter(ArrayD::from_elem(IxDyn(&[4]), 3.0));

                param.with_storage_view_for_input_dtype_and_route(
                    DType::F32,
                    KernelRouteClass::GenericMatmul,
                    |view| {
                        #[cfg(all(feature = "arm64-fp-kernels", target_arch = "aarch64"))]
                        match view {
                            TensorStorageView::BF16(view) => assert_eq!(view.len(), 4),
                            TensorStorageView::F16(_) => {
                                panic!("generic matmul on arm should keep native bf16 parameter view")
                            }
                            TensorStorageView::F32(_) => {
                                panic!("generic matmul on arm should keep native bf16 parameter view")
                            }
                        }

                        #[cfg(not(all(feature = "arm64-fp-kernels", target_arch = "aarch64")))]
                        match view {
                            TensorStorageView::F32(view) => assert_eq!(view.len(), 4),
                            TensorStorageView::F16(_) => {
                                panic!("generic matmul should still be allowed to use cached f32 parameter view")
                            }
                            TensorStorageView::BF16(_) => {
                                panic!("generic matmul should still be allowed to use cached f32 parameter view")
                            }
                        }
                    },
                );
            },
        );
    }

    #[test]
    fn argmax_route_prefers_native_same_dtype_parameter_view() {
        with_precision_config(
            PrecisionConfig {
                parameter_dtype: DType::BF16,
                runtime_dtype: DType::F32,
                allow_parameter_dtype_copies: true,
            },
            || {
                let param = Tensor::parameter(ArrayD::from_elem(IxDyn(&[4]), 3.0));

                param.with_storage_view_for_input_dtype_and_route(
                    DType::BF16,
                    KernelRouteClass::Argmax,
                    |view| match view {
                        TensorStorageView::BF16(view) => assert_eq!(view.len(), 4),
                        TensorStorageView::F16(_) => {
                            panic!("same-dtype argmax should keep native bf16 parameter view")
                        }
                        TensorStorageView::F32(_) => {
                            panic!("same-dtype argmax should keep native bf16 parameter view")
                        }
                    },
                );
            },
        );
    }

    #[test]
    fn decode_route_prefers_cached_f32_view_for_same_f16_parameter_use() {
        with_precision_config(
            PrecisionConfig {
                parameter_dtype: DType::F16,
                runtime_dtype: DType::F32,
                allow_parameter_dtype_copies: true,
            },
            || {
                let param = Tensor::parameter(ArrayD::from_elem(IxDyn(&[4]), 3.0));

                param.with_storage_view_for_input_dtype(DType::F16, |view| match view {
                    TensorStorageView::F32(view) => assert_eq!(view.len(), 4),
                    TensorStorageView::F16(_) => {
                        panic!("same-dtype f16 decode should currently prefer cached f32 parameter view")
                    }
                    TensorStorageView::BF16(_) => {
                        panic!("same-dtype f16 decode should currently prefer cached f32 parameter view")
                    }
                });
            },
        );
    }

    #[test]
    fn argmax_route_prefers_cached_f32_view_for_same_f16_parameter_use() {
        with_precision_config(
            PrecisionConfig {
                parameter_dtype: DType::F16,
                runtime_dtype: DType::F32,
                allow_parameter_dtype_copies: true,
            },
            || {
                let param = Tensor::parameter(ArrayD::from_elem(IxDyn(&[4]), 3.0));

                param.with_storage_view_for_input_dtype_and_route(
                    DType::F16,
                    KernelRouteClass::Argmax,
                    |view| match view {
                        TensorStorageView::F32(view) => assert_eq!(view.len(), 4),
                        TensorStorageView::F16(_) => {
                            panic!("same-dtype f16 argmax should currently prefer cached f32 parameter view")
                        }
                        TensorStorageView::BF16(_) => {
                            panic!("same-dtype f16 argmax should currently prefer cached f32 parameter view")
                        }
                    },
                );
            },
        );
    }

    #[test]
    fn f32_compute_preference_materializes_compute_view() {
        with_precision_config(
            PrecisionConfig {
                parameter_dtype: DType::BF16,
                runtime_dtype: DType::F32,
                allow_parameter_dtype_copies: false,
            },
            || {
                let tensor =
                    Tensor::new_with_dtype(ArrayD::from_elem(IxDyn(&[3]), 1.5), DType::BF16);
                tensor.with_storage_view_preferring(
                    StoragePreference::F32Compute,
                    |view| match view {
                        TensorStorageView::F32(view) => assert_eq!(view.len(), 3),
                        TensorStorageView::F16(_) => {
                            panic!("f32 compute preference should expose f32 view")
                        }
                        TensorStorageView::BF16(_) => {
                            panic!("f32 compute preference should expose f32 view")
                        }
                    },
                );
                assert_eq!(
                    tensor.dtype(),
                    DType::BF16,
                    "compute view should not mutate bf16 storage"
                );
            },
        );
    }

    #[test]
    fn f32_compute_preference_is_reentrant() {
        with_precision_config(
            PrecisionConfig {
                parameter_dtype: DType::BF16,
                runtime_dtype: DType::F32,
                allow_parameter_dtype_copies: false,
            },
            || {
                let lhs = Tensor::new_with_dtype(ArrayD::from_elem(IxDyn(&[2]), 1.0), DType::BF16);
                let rhs = Tensor::new_with_dtype(ArrayD::from_elem(IxDyn(&[3]), 2.0), DType::BF16);
                lhs.with_storage_view_preferring(StoragePreference::F32Compute, |lhs_view| {
                    match lhs_view {
                        TensorStorageView::F32(lhs_view) => {
                            assert_eq!(lhs_view.len(), 2);
                            rhs.with_storage_view_preferring(
                                StoragePreference::F32Compute,
                                |rhs_view| match rhs_view {
                                    TensorStorageView::F32(rhs_view) => {
                                        assert_eq!(rhs_view.len(), 3)
                                    }
                                    TensorStorageView::F16(_) => panic!(
                                        "nested f32 compute preference should expose f32 view"
                                    ),
                                    TensorStorageView::BF16(_) => panic!(
                                        "nested f32 compute preference should expose f32 view"
                                    ),
                                },
                            );
                        }
                        TensorStorageView::F16(_) => {
                            panic!("f32 compute preference should expose f32 view")
                        }
                        TensorStorageView::BF16(_) => {
                            panic!("f32 compute preference should expose f32 view")
                        }
                    }
                });
            },
        );
    }

    #[test]
    fn parameter_creation_supports_i8_scope() {
        with_precision_config(
            PrecisionConfig {
                parameter_dtype: DType::I8,
                runtime_dtype: DType::F32,
                allow_parameter_dtype_copies: false,
            },
            || {
                let param = Tensor::parameter(
                    ArrayD::from_shape_vec(IxDyn(&[3]), vec![1.0, -0.5, 0.25]).unwrap(),
                );
                assert_eq!(param.dtype(), DType::I8);
                match param.native_storage_owned() {
                    TensorStorageOwned::I8(data, scale) => {
                        assert_eq!(data.len(), 3);
                        assert!(scale > 0.0);
                    }
                    TensorStorageOwned::F32(_)
                    | TensorStorageOwned::F16(_)
                    | TensorStorageOwned::BF16(_) => {
                        panic!("i8 parameter should keep native i8 storage")
                    }
                }
                param.with_storage_view_preferring(
                    StoragePreference::F32Compute,
                    |view| match view {
                        TensorStorageView::F32(view) => assert_eq!(view.len(), 3),
                        TensorStorageView::F16(_) => panic!("i8 compute view should expose f32"),
                        TensorStorageView::BF16(_) => panic!("i8 compute view should expose f32"),
                    },
                );
                assert_eq!(
                    param.dtype(),
                    DType::I8,
                    "compute view should not mutate i8 storage"
                );
            },
        );
    }

    #[test]
    fn parameter_creation_can_follow_global_quantization_setting() {
        with_precision_config(
            PrecisionConfig {
                parameter_dtype: DType::F32,
                runtime_dtype: DType::F32,
                allow_parameter_dtype_copies: false,
            },
            || {
                with_parameter_quantization(ParameterQuantization::Int8, || {
                    let param = Tensor::parameter(
                        ArrayD::from_shape_vec(IxDyn(&[3]), vec![1.0, -0.5, 0.25]).unwrap(),
                    );
                    assert_eq!(param.dtype(), DType::I8);
                });
            },
        );
    }

    #[test]
    fn parameter_creation_can_follow_manual_quantization_scale() {
        with_precision_config(
            PrecisionConfig {
                parameter_dtype: DType::F32,
                runtime_dtype: DType::F32,
                allow_parameter_dtype_copies: false,
            },
            || {
                with_parameter_quantization(ParameterQuantization::Int8.with_scale(0.5), || {
                    let param = Tensor::parameter(
                        ArrayD::from_shape_vec(IxDyn(&[4]), vec![0.9, -1.1, 1.6, -2.6]).unwrap(),
                    );
                    assert_eq!(param.dtype(), DType::I8);
                    assert_eq!(param.quantization_scale(), Some(0.5));
                    let loaded = param.data();
                    let expected = [1.0f32, -1.0, 1.5, -2.5];
                    for (got, want) in loaded.iter().zip(expected.iter()) {
                        assert!((got - want).abs() <= 1e-6, "got {got}, want {want}");
                    }
                });
            },
        );
    }

    #[test]
    fn set_i8_slice_with_dtype_reuses_existing_i8_placeholder_storage() {
        let tensor =
            Tensor::parameter_placeholder_with_quantization(&[2, 2], ParameterQuantization::Int8);

        let before_ptr = {
            let inner = tensor.0.borrow();
            inner
                .i8_data
                .as_ref()
                .expect("placeholder should allocate i8 storage")
                .as_slice_memory_order()
                .expect("placeholder i8 storage should be contiguous")
                .as_ptr()
        };

        tensor.set_i8_slice_with_dtype(&[2, 2], &[4, -8, 7, 9], 0.5, DType::I8);

        let inner = tensor.0.borrow();
        let after_slice = inner
            .i8_data
            .as_ref()
            .expect("tensor should retain i8 storage")
            .as_slice_memory_order()
            .expect("tensor i8 storage should remain contiguous");
        assert_eq!(before_ptr, after_slice.as_ptr());
        assert_eq!(after_slice, &[4, -8, 7, 9]);
        assert_eq!(inner.i8_scale, Some(0.5));
        assert!(!inner.has_f32_data);
    }

    #[test]
    fn parameter_explicit_dtype_overrides_global_default() {
        with_precision_config(
            PrecisionConfig {
                parameter_dtype: DType::BF16,
                runtime_dtype: DType::F32,
                allow_parameter_dtype_copies: false,
            },
            || {
                let param = Tensor::parameter_with_dtype(
                    ArrayD::from_shape_vec(IxDyn(&[3]), vec![1.0, -0.5, 0.25]).unwrap(),
                    DType::F32,
                );
                assert_eq!(param.dtype(), DType::F32);
                param.with_storage_view_preferring(StoragePreference::Native, |view| match view {
                    TensorStorageView::F32(view) => assert_eq!(view.len(), 3),
                    TensorStorageView::F16(_) => {
                        panic!("explicit parameter dtype should override global bf16 default")
                    }
                    TensorStorageView::BF16(_) => {
                        panic!("explicit parameter dtype should override global bf16 default")
                    }
                });
            },
        );
    }

    #[test]
    fn parameter_explicit_dtype_overrides_global_quantization() {
        with_parameter_quantization(ParameterQuantization::Int8, || {
            let param = Tensor::parameter_with_dtype(
                ArrayD::from_shape_vec(IxDyn(&[3]), vec![1.0, -0.5, 0.25]).unwrap(),
                DType::F32,
            );
            assert_eq!(param.dtype(), DType::F32);
        });
    }

    #[test]
    fn i8_export_import_preserves_dtype_and_values() {
        let src = Tensor::new_with_dtype(
            ArrayD::from_shape_vec(IxDyn(&[4]), vec![1.0, -2.0, 0.5, 3.25]).unwrap(),
            DType::I8,
        );
        let (shape, dtype, raw) = src.export_raw();
        let dst = Tensor::from_array_no_grad(ArrayD::zeros(IxDyn(&[4])));
        dst.import_raw(shape, dtype, raw)
            .expect("i8 export/import should succeed");

        assert_eq!(dst.dtype(), DType::I8);
        let src_vals = src.data();
        let dst_vals = dst.data();
        for (&lhs, &rhs) in src_vals.iter().zip(dst_vals.iter()) {
            assert!((lhs - rhs).abs() <= 1e-6, "lhs={lhs}, rhs={rhs}");
        }
    }

    #[test]
    fn import_raw_returns_error_on_dtype_mismatch() {
        let dst = Tensor::from_array_no_grad(ArrayD::zeros(IxDyn(&[2])));
        let err = dst
            .import_raw(vec![2], DType::BF16, TensorRawData::F32(vec![1.0, -2.0]))
            .expect_err("dtype mismatch should return an error");
        assert!(
            err.contains("dtype BF16 with f32 data"),
            "unexpected error: {err}"
        );
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn to_cuda_and_back_preserves_tensor_metadata() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let tensor = Tensor::new_with_dtype(
            ArrayD::from_shape_vec(IxDyn(&[2, 2]), vec![1.0, -2.0, 0.5, 3.25]).unwrap(),
            DType::BF16,
        );
        let cuda_tensor = tensor.to_cuda();
        assert_eq!(cuda_tensor.device(), Device::Cuda);
        assert_eq!(cuda_tensor.dtype(), DType::BF16);
        assert_eq!(cuda_tensor.shape_vec(), vec![2, 2]);

        let cpu_tensor = cuda_tensor.to_cpu();
        assert_eq!(cpu_tensor.device(), Device::Cpu);
        assert_eq!(cpu_tensor.dtype(), DType::BF16);
        assert_eq!(cpu_tensor.shape_vec(), vec![2, 2]);
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn to_cuda_materializes_resident_buffer_and_to_cpu_releases_it() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let tensor = Tensor::from_array_no_grad(
            ArrayD::from_shape_vec(IxDyn(&[4]), vec![1.0, -2.0, 0.5, 3.25]).unwrap(),
        );
        assert!(tensor.0.borrow().cuda_f32_data.is_none());

        tensor.to_cuda_inplace();
        {
            let inner = tensor.0.borrow();
            assert_eq!(inner.device, Device::Cuda);
            assert!(inner.cuda_f32_data.is_some());
        }

        tensor.to_cpu_inplace();
        {
            let inner = tensor.0.borrow();
            assert_eq!(inner.device, Device::Cpu);
            assert!(inner.cuda_f32_data.is_none());
        }
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn empty_tensor_to_cuda_preserves_metadata_without_resident_buffer() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let tensor = Tensor::new_with_dtype(
            ArrayD::from_shape_vec(IxDyn(&[2, 0, 3]), Vec::<f32>::new()).unwrap(),
            DType::BF16,
        );
        let cuda_tensor = tensor.to_cuda();

        assert_eq!(cuda_tensor.device(), Device::Cuda);
        assert_eq!(cuda_tensor.dtype(), DType::BF16);
        assert_eq!(cuda_tensor.shape_vec(), vec![2, 0, 3]);
        assert_eq!(cuda_tensor.len(), 0);
        assert!(cuda_tensor.cloned_cuda_f32_buffer().is_none());
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn host_mutation_invalidates_resident_cuda_buffer() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let tensor = Tensor::from_array_no_grad(
            ArrayD::from_shape_vec(IxDyn(&[2]), vec![1.0, 2.0]).unwrap(),
        );
        tensor.to_cuda_inplace();
        assert!(tensor.0.borrow().cuda_f32_data.is_some());

        tensor.set_raw_data(vec![2], vec![3.0, 4.0]);
        assert!(tensor.0.borrow().cuda_f32_data.is_none());

        tensor.to_cuda_inplace();
        assert!(tensor.0.borrow().cuda_f32_data.is_some());
    }
}
