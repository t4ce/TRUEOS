use serde::{Deserialize, Serialize};
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DType {
    F32 = 0,
    F16 = 1,
    BF16 = 2,
    I8 = 3,
}

impl DType {
    #[inline]
    fn from_u8(value: u8) -> Self {
        match value {
            0 => DType::F32,
            1 => DType::F16,
            2 => DType::BF16,
            3 => DType::I8,
            _ => DType::F32,
        }
    }

    #[inline]
    pub fn is_float(self) -> bool {
        matches!(self, DType::F32 | DType::F16 | DType::BF16)
    }

    #[inline]
    pub fn is_integer(self) -> bool {
        matches!(self, DType::I8)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum QuantizationScale {
    Auto,
    Manual(f32),
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParameterQuantization {
    storage_dtype: Option<DType>,
    scale: QuantizationScale,
}

impl Default for ParameterQuantization {
    fn default() -> Self {
        Self::Disabled
    }
}

impl ParameterQuantization {
    #[allow(non_upper_case_globals)]
    pub const Disabled: Self = Self {
        storage_dtype: None,
        scale: QuantizationScale::Auto,
    };

    #[allow(non_upper_case_globals)]
    pub const Int8: Self = Self {
        storage_dtype: Some(DType::I8),
        scale: QuantizationScale::Auto,
    };

    #[inline]
    pub fn new(storage_dtype: DType) -> Self {
        assert!(
            storage_dtype.is_integer(),
            "parameter quantization storage dtype must be integer, got {:?}",
            storage_dtype
        );
        let quantization = Self {
            storage_dtype: Some(storage_dtype),
            scale: QuantizationScale::Auto,
        };
        quantization.validate();
        quantization
    }

    #[inline]
    pub fn storage_dtype(self) -> Option<DType> {
        self.storage_dtype
    }

    #[inline]
    pub fn is_enabled(self) -> bool {
        self.storage_dtype.is_some()
    }

    #[inline]
    pub fn scale(self) -> QuantizationScale {
        self.scale
    }

    #[inline]
    pub fn scale_override(self) -> Option<f32> {
        match self.scale {
            QuantizationScale::Auto => None,
            QuantizationScale::Manual(scale) => Some(scale),
        }
    }

    #[inline]
    pub fn with_scale(mut self, scale: f32) -> Self {
        validate_quantization_scale(scale);
        self.scale = QuantizationScale::Manual(scale);
        self
    }

    #[inline]
    pub fn with_optional_scale(mut self, scale: Option<f32>) -> Self {
        self.scale = match scale {
            Some(scale) => {
                validate_quantization_scale(scale);
                QuantizationScale::Manual(scale)
            }
            None => QuantizationScale::Auto,
        };
        self
    }

    #[inline]
    pub fn without_scale(mut self) -> Self {
        self.scale = QuantizationScale::Auto;
        self
    }

    #[inline]
    fn validate(self) {
        if let Some(dtype) = self.storage_dtype {
            assert!(
                dtype.is_integer(),
                "parameter quantization storage dtype must be integer, got {:?}",
                dtype
            );
            assert!(
                matches!(dtype, DType::I8),
                "parameter quantization dtype {:?} is not implemented yet; currently only I8 is supported",
                dtype
            );
        }
        if let Some(scale) = self.scale_override() {
            validate_quantization_scale(scale);
        }
    }
}

#[inline]
fn validate_quantization_scale(scale: f32) {
    assert!(
        scale.is_finite() && scale > 0.0,
        "quantization scale must be finite and > 0, got {}",
        scale
    );
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrecisionConfig {
    pub parameter_dtype: DType,
    pub runtime_dtype: DType,
    pub allow_parameter_dtype_copies: bool,
}

impl Default for PrecisionConfig {
    fn default() -> Self {
        Self {
            parameter_dtype: DType::F32,
            runtime_dtype: DType::F32,
            allow_parameter_dtype_copies: false,
        }
    }
}

pub struct PrecisionConfigGuard {
    previous: PrecisionConfig,
    _lock: Option<MutexGuard<'static, ()>>,
}

pub struct ParameterQuantizationGuard {
    previous: ParameterQuantization,
    _lock: Option<MutexGuard<'static, ()>>,
}

pub struct RuntimeComponentDTypesGuard {
    previous_activation_dtype: DType,
    previous_activation_follows_runtime: bool,
    previous_kv_cache_dtype: DType,
    previous_kv_cache_follows_runtime: bool,
    _lock: Option<MutexGuard<'static, ()>>,
}

static DEFAULT_PARAMETER_DTYPE: AtomicU8 = AtomicU8::new(DType::F32 as u8);
static DEFAULT_RUNTIME_DTYPE: AtomicU8 = AtomicU8::new(DType::F32 as u8);
static DEFAULT_ACTIVATION_DTYPE: AtomicU8 = AtomicU8::new(DType::F32 as u8);
static DEFAULT_KV_CACHE_DTYPE: AtomicU8 = AtomicU8::new(DType::F32 as u8);
static ACTIVATION_DTYPE_FOLLOWS_RUNTIME: AtomicBool = AtomicBool::new(true);
static KV_CACHE_DTYPE_FOLLOWS_RUNTIME: AtomicBool = AtomicBool::new(true);
static ALLOW_PARAMETER_DTYPE_COPIES: AtomicBool = AtomicBool::new(false);
static PRECISION_CONFIG_LOCK: Mutex<()> = Mutex::new(());
const DISABLED_PARAMETER_QUANTIZATION_DTYPE: u8 = u8::MAX;
static DEFAULT_PARAMETER_QUANTIZATION_DTYPE: AtomicU8 =
    AtomicU8::new(DISABLED_PARAMETER_QUANTIZATION_DTYPE);
static DEFAULT_PARAMETER_QUANTIZATION_SCALE_IS_MANUAL: AtomicBool = AtomicBool::new(false);
static DEFAULT_PARAMETER_QUANTIZATION_SCALE_BITS: AtomicU32 = AtomicU32::new(1.0f32.to_bits());

thread_local! {
    static GLOBAL_CONFIG_SCOPE_DEPTH: Cell<usize> = const { Cell::new(0) };
}

#[inline]
fn begin_global_config_scope(lock_message: &'static str) -> Option<MutexGuard<'static, ()>> {
    GLOBAL_CONFIG_SCOPE_DEPTH.with(|depth| {
        let current = depth.get();
        depth.set(current + 1);
        if current == 0 {
            Some(PRECISION_CONFIG_LOCK.lock().expect(lock_message))
        } else {
            None
        }
    })
}

#[inline]
fn end_global_config_scope(scope_name: &'static str) {
    GLOBAL_CONFIG_SCOPE_DEPTH.with(|depth| {
        let current = depth.get();
        debug_assert!(current > 0, "{scope_name} scope depth underflow");
        depth.set(current.saturating_sub(1));
    });
}

#[inline]
pub fn default_parameter_dtype() -> DType {
    DType::from_u8(DEFAULT_PARAMETER_DTYPE.load(Ordering::Relaxed))
}

#[inline]
pub fn set_default_parameter_dtype(dtype: DType) {
    DEFAULT_PARAMETER_DTYPE.store(dtype as u8, Ordering::Relaxed);
}

#[inline]
pub fn default_parameter_quantization() -> ParameterQuantization {
    let storage_dtype_raw = DEFAULT_PARAMETER_QUANTIZATION_DTYPE.load(Ordering::Relaxed);
    let storage_dtype = if storage_dtype_raw == DISABLED_PARAMETER_QUANTIZATION_DTYPE {
        None
    } else {
        Some(DType::from_u8(storage_dtype_raw))
    };
    let scale = if DEFAULT_PARAMETER_QUANTIZATION_SCALE_IS_MANUAL.load(Ordering::Relaxed) {
        QuantizationScale::Manual(f32::from_bits(
            DEFAULT_PARAMETER_QUANTIZATION_SCALE_BITS.load(Ordering::Relaxed),
        ))
    } else {
        QuantizationScale::Auto
    };
    let quantization = ParameterQuantization {
        storage_dtype,
        scale,
    };
    quantization.validate();
    quantization
}

#[inline]
pub fn set_default_parameter_quantization(quantization: ParameterQuantization) {
    quantization.validate();
    DEFAULT_PARAMETER_QUANTIZATION_DTYPE.store(
        quantization
            .storage_dtype()
            .map(|dtype| dtype as u8)
            .unwrap_or(DISABLED_PARAMETER_QUANTIZATION_DTYPE),
        Ordering::Relaxed,
    );
    match quantization.scale() {
        QuantizationScale::Auto => {
            DEFAULT_PARAMETER_QUANTIZATION_SCALE_IS_MANUAL.store(false, Ordering::Relaxed);
        }
        QuantizationScale::Manual(scale) => {
            DEFAULT_PARAMETER_QUANTIZATION_SCALE_BITS.store(scale.to_bits(), Ordering::Relaxed);
            DEFAULT_PARAMETER_QUANTIZATION_SCALE_IS_MANUAL.store(true, Ordering::Relaxed);
        }
    }
}

#[inline]
pub fn default_parameter_quantization_dtype() -> Option<DType> {
    default_parameter_quantization().storage_dtype()
}

#[inline]
pub fn set_default_parameter_quantization_dtype(storage_dtype: Option<DType>) {
    let current = default_parameter_quantization();
    let updated = match storage_dtype {
        Some(storage_dtype) => ParameterQuantization::new(storage_dtype),
        None => ParameterQuantization::Disabled,
    }
    .with_optional_scale(current.scale_override());
    set_default_parameter_quantization(updated);
}

#[inline]
pub fn default_parameter_quantization_scale() -> Option<f32> {
    default_parameter_quantization().scale_override()
}

#[inline]
pub fn set_default_parameter_quantization_scale(scale: Option<f32>) {
    let current = default_parameter_quantization();
    set_default_parameter_quantization(current.with_optional_scale(scale));
}

#[inline]
pub fn parameter_quantization_enabled() -> bool {
    default_parameter_quantization().is_enabled()
}

#[inline]
pub fn set_parameter_quantization_enabled(enabled: bool) {
    let current = default_parameter_quantization();
    set_default_parameter_quantization(if enabled {
        if current.is_enabled() {
            current
        } else {
            ParameterQuantization::Int8.with_optional_scale(current.scale_override())
        }
    } else {
        ParameterQuantization::Disabled.with_optional_scale(current.scale_override())
    });
}

#[inline]
pub fn default_parameter_storage_dtype() -> DType {
    default_parameter_quantization()
        .storage_dtype()
        .unwrap_or_else(default_parameter_dtype)
}

#[inline]
pub fn default_runtime_dtype() -> DType {
    DType::from_u8(DEFAULT_RUNTIME_DTYPE.load(Ordering::Relaxed))
}

#[inline]
pub fn set_default_runtime_dtype(dtype: DType) {
    DEFAULT_RUNTIME_DTYPE.store(dtype as u8, Ordering::Relaxed);
    if ACTIVATION_DTYPE_FOLLOWS_RUNTIME.load(Ordering::Relaxed) {
        DEFAULT_ACTIVATION_DTYPE.store(dtype as u8, Ordering::Relaxed);
    }
    if KV_CACHE_DTYPE_FOLLOWS_RUNTIME.load(Ordering::Relaxed) {
        DEFAULT_KV_CACHE_DTYPE.store(dtype as u8, Ordering::Relaxed);
    }
}

#[inline]
pub fn default_activation_dtype() -> DType {
    DType::from_u8(DEFAULT_ACTIVATION_DTYPE.load(Ordering::Relaxed))
}

#[inline]
pub fn set_default_activation_dtype(dtype: DType) {
    DEFAULT_ACTIVATION_DTYPE.store(dtype as u8, Ordering::Relaxed);
    ACTIVATION_DTYPE_FOLLOWS_RUNTIME.store(false, Ordering::Relaxed);
}

#[inline]
pub fn reset_default_activation_dtype_to_runtime() {
    DEFAULT_ACTIVATION_DTYPE.store(default_runtime_dtype() as u8, Ordering::Relaxed);
    ACTIVATION_DTYPE_FOLLOWS_RUNTIME.store(true, Ordering::Relaxed);
}

#[inline]
pub fn activation_dtype_follows_runtime() -> bool {
    ACTIVATION_DTYPE_FOLLOWS_RUNTIME.load(Ordering::Relaxed)
}

#[inline]
pub fn default_kv_cache_dtype() -> DType {
    DType::from_u8(DEFAULT_KV_CACHE_DTYPE.load(Ordering::Relaxed))
}

#[inline]
pub fn set_default_kv_cache_dtype(dtype: DType) {
    DEFAULT_KV_CACHE_DTYPE.store(dtype as u8, Ordering::Relaxed);
    KV_CACHE_DTYPE_FOLLOWS_RUNTIME.store(false, Ordering::Relaxed);
}

#[inline]
pub fn reset_default_kv_cache_dtype_to_runtime() {
    DEFAULT_KV_CACHE_DTYPE.store(default_runtime_dtype() as u8, Ordering::Relaxed);
    KV_CACHE_DTYPE_FOLLOWS_RUNTIME.store(true, Ordering::Relaxed);
}

#[inline]
pub fn kv_cache_dtype_follows_runtime() -> bool {
    KV_CACHE_DTYPE_FOLLOWS_RUNTIME.load(Ordering::Relaxed)
}

#[inline]
fn restore_activation_dtype_state(dtype: DType, follows_runtime: bool) {
    if follows_runtime {
        reset_default_activation_dtype_to_runtime();
    } else {
        set_default_activation_dtype(dtype);
    }
}

#[inline]
fn restore_kv_cache_dtype_state(dtype: DType, follows_runtime: bool) {
    if follows_runtime {
        reset_default_kv_cache_dtype_to_runtime();
    } else {
        set_default_kv_cache_dtype(dtype);
    }
}

#[inline]
pub fn default_dtype() -> DType {
    default_runtime_dtype()
}

#[inline]
pub fn set_default_dtype(dtype: DType) {
    set_default_runtime_dtype(dtype);
}

#[inline]
pub fn allow_parameter_dtype_copies() -> bool {
    ALLOW_PARAMETER_DTYPE_COPIES.load(Ordering::Relaxed)
}

#[inline]
pub fn set_allow_parameter_dtype_copies(allow: bool) {
    ALLOW_PARAMETER_DTYPE_COPIES.store(allow, Ordering::Relaxed);
}

#[inline]
pub fn set_precision_config(config: PrecisionConfig) {
    set_default_parameter_dtype(config.parameter_dtype);
    set_default_runtime_dtype(config.runtime_dtype);
    set_allow_parameter_dtype_copies(config.allow_parameter_dtype_copies);
}

#[inline]
pub fn precision_guard(config: PrecisionConfig) -> PrecisionConfigGuard {
    let lock = begin_global_config_scope("precision config lock poisoned");
    let previous = precision_config();
    set_precision_config(config);
    PrecisionConfigGuard {
        previous,
        _lock: lock,
    }
}

#[inline]
pub fn with_precision_config<R>(config: PrecisionConfig, f: impl FnOnce() -> R) -> R {
    let _guard = precision_guard(config);
    f()
}

#[inline]
pub fn parameter_quantization_guard(
    quantization: ParameterQuantization,
) -> ParameterQuantizationGuard {
    let lock = begin_global_config_scope("global config lock poisoned");
    let previous = default_parameter_quantization();
    set_default_parameter_quantization(quantization);
    ParameterQuantizationGuard {
        previous,
        _lock: lock,
    }
}

#[inline]
pub fn with_parameter_quantization<R>(
    quantization: ParameterQuantization,
    f: impl FnOnce() -> R,
) -> R {
    let _guard = parameter_quantization_guard(quantization);
    f()
}

#[inline]
pub fn runtime_component_dtypes_guard(
    activation_dtype: Option<DType>,
    kv_cache_dtype: Option<DType>,
) -> RuntimeComponentDTypesGuard {
    let lock = begin_global_config_scope("runtime component dtype lock poisoned");
    let previous_activation_dtype = default_activation_dtype();
    let previous_activation_follows_runtime = activation_dtype_follows_runtime();
    let previous_kv_cache_dtype = default_kv_cache_dtype();
    let previous_kv_cache_follows_runtime = kv_cache_dtype_follows_runtime();

    if let Some(dtype) = activation_dtype {
        set_default_activation_dtype(dtype);
    }
    if let Some(dtype) = kv_cache_dtype {
        set_default_kv_cache_dtype(dtype);
    }

    RuntimeComponentDTypesGuard {
        previous_activation_dtype,
        previous_activation_follows_runtime,
        previous_kv_cache_dtype,
        previous_kv_cache_follows_runtime,
        _lock: lock,
    }
}

#[inline]
pub fn with_runtime_component_dtypes<R>(
    activation_dtype: Option<DType>,
    kv_cache_dtype: Option<DType>,
    f: impl FnOnce() -> R,
) -> R {
    let _guard = runtime_component_dtypes_guard(activation_dtype, kv_cache_dtype);
    f()
}

#[inline]
pub fn allow_parameter_copies() -> bool {
    allow_parameter_dtype_copies()
}

#[inline]
pub fn set_allow_parameter_copies(allow: bool) {
    set_allow_parameter_dtype_copies(allow);
}

#[inline]
pub fn precision_config() -> PrecisionConfig {
    PrecisionConfig {
        parameter_dtype: default_parameter_dtype(),
        runtime_dtype: default_runtime_dtype(),
        allow_parameter_dtype_copies: allow_parameter_dtype_copies(),
    }
}

impl Drop for PrecisionConfigGuard {
    fn drop(&mut self) {
        set_precision_config(self.previous);
        end_global_config_scope("precision");
    }
}

impl Drop for ParameterQuantizationGuard {
    fn drop(&mut self) {
        set_default_parameter_quantization(self.previous);
        end_global_config_scope("parameter quantization");
    }
}

impl Drop for RuntimeComponentDTypesGuard {
    fn drop(&mut self) {
        restore_activation_dtype_state(
            self.previous_activation_dtype,
            self.previous_activation_follows_runtime,
        );
        restore_kv_cache_dtype_state(
            self.previous_kv_cache_dtype,
            self.previous_kv_cache_follows_runtime,
        );
        end_global_config_scope("runtime component dtypes");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precision_scope_restores_previous_config() {
        let _guard = precision_guard(PrecisionConfig {
            parameter_dtype: DType::F32,
            runtime_dtype: DType::F32,
            allow_parameter_dtype_copies: false,
        });

        with_precision_config(
            PrecisionConfig {
                parameter_dtype: DType::BF16,
                runtime_dtype: DType::BF16,
                allow_parameter_dtype_copies: true,
            },
            || {
                assert_eq!(precision_config().parameter_dtype, DType::BF16);
                assert_eq!(precision_config().runtime_dtype, DType::BF16);
                assert!(precision_config().allow_parameter_dtype_copies);
            },
        );

        assert_eq!(precision_config().parameter_dtype, DType::F32);
        assert_eq!(precision_config().runtime_dtype, DType::F32);
        assert!(!precision_config().allow_parameter_dtype_copies);
    }

    #[test]
    fn nested_precision_scopes_restore_in_stack_order() {
        let _guard = precision_guard(PrecisionConfig::default());

        let outer = PrecisionConfig {
            parameter_dtype: DType::BF16,
            runtime_dtype: DType::BF16,
            allow_parameter_dtype_copies: false,
        };
        let inner = PrecisionConfig {
            parameter_dtype: DType::F32,
            runtime_dtype: DType::F32,
            allow_parameter_dtype_copies: true,
        };

        with_precision_config(outer, || {
            assert_eq!(precision_config(), outer);
            with_precision_config(inner, || {
                assert_eq!(precision_config(), inner);
            });
            assert_eq!(precision_config(), outer);
        });

        assert_eq!(precision_config(), PrecisionConfig::default());
    }

    #[test]
    fn parameter_quantization_scope_restores_previous_setting() {
        let _precision_guard = precision_guard(PrecisionConfig {
            parameter_dtype: DType::F32,
            runtime_dtype: DType::F32,
            allow_parameter_dtype_copies: false,
        });
        let _quantization_guard = parameter_quantization_guard(ParameterQuantization::Disabled);

        with_parameter_quantization(ParameterQuantization::Int8, || {
            assert_eq!(
                default_parameter_quantization(),
                ParameterQuantization::Int8
            );
            assert!(parameter_quantization_enabled());
            assert_eq!(default_parameter_storage_dtype(), DType::I8);
        });

        assert_eq!(
            default_parameter_quantization(),
            ParameterQuantization::Disabled
        );
        assert_eq!(default_parameter_storage_dtype(), DType::F32);
    }

    #[test]
    fn enabling_parameter_quantization_defaults_parameter_storage_to_i8() {
        let _precision_guard = precision_guard(PrecisionConfig {
            parameter_dtype: DType::F32,
            runtime_dtype: DType::F32,
            allow_parameter_dtype_copies: false,
        });
        let _quantization_guard = parameter_quantization_guard(ParameterQuantization::Disabled);

        set_parameter_quantization_enabled(true);
        assert!(parameter_quantization_enabled());
        assert_eq!(default_parameter_storage_dtype(), DType::I8);

        set_parameter_quantization_enabled(false);
        assert!(!parameter_quantization_enabled());
        assert_eq!(default_parameter_storage_dtype(), DType::F32);
    }

    #[test]
    fn parameter_quantization_can_store_manual_scale() {
        let _precision_guard = precision_guard(PrecisionConfig {
            parameter_dtype: DType::F32,
            runtime_dtype: DType::F32,
            allow_parameter_dtype_copies: false,
        });
        let _quantization_guard = parameter_quantization_guard(ParameterQuantization::Disabled);
        let quantization = ParameterQuantization::Int8.with_scale(0.25);
        set_default_parameter_quantization(quantization);

        assert_eq!(default_parameter_quantization(), quantization);
        assert_eq!(default_parameter_quantization_dtype(), Some(DType::I8));
        assert_eq!(default_parameter_quantization_scale(), Some(0.25));
    }

    #[test]
    fn activation_and_kv_cache_defaults_follow_runtime_until_overridden() {
        let _precision_guard = precision_guard(PrecisionConfig {
            parameter_dtype: DType::F32,
            runtime_dtype: DType::F32,
            allow_parameter_dtype_copies: false,
        });
        reset_default_activation_dtype_to_runtime();
        reset_default_kv_cache_dtype_to_runtime();

        assert_eq!(default_activation_dtype(), DType::F32);
        assert_eq!(default_kv_cache_dtype(), DType::F32);
        assert!(activation_dtype_follows_runtime());
        assert!(kv_cache_dtype_follows_runtime());

        set_default_runtime_dtype(DType::BF16);
        assert_eq!(default_runtime_dtype(), DType::BF16);
        assert_eq!(default_activation_dtype(), DType::BF16);
        assert_eq!(default_kv_cache_dtype(), DType::BF16);
    }

    #[test]
    fn activation_and_kv_cache_defaults_can_override_runtime_independently() {
        let _precision_guard = precision_guard(PrecisionConfig {
            parameter_dtype: DType::F32,
            runtime_dtype: DType::F32,
            allow_parameter_dtype_copies: false,
        });
        reset_default_activation_dtype_to_runtime();
        reset_default_kv_cache_dtype_to_runtime();

        set_default_activation_dtype(DType::F16);
        set_default_kv_cache_dtype(DType::BF16);
        set_default_runtime_dtype(DType::F32);

        assert_eq!(default_runtime_dtype(), DType::F32);
        assert_eq!(default_activation_dtype(), DType::F16);
        assert_eq!(default_kv_cache_dtype(), DType::BF16);
        assert!(!activation_dtype_follows_runtime());
        assert!(!kv_cache_dtype_follows_runtime());

        reset_default_activation_dtype_to_runtime();
        reset_default_kv_cache_dtype_to_runtime();
        assert_eq!(default_activation_dtype(), DType::F32);
        assert_eq!(default_kv_cache_dtype(), DType::F32);
        assert!(activation_dtype_follows_runtime());
        assert!(kv_cache_dtype_follows_runtime());
    }

    #[test]
    fn runtime_component_dtype_scope_restores_previous_defaults_and_follow_flags() {
        let _precision_guard = precision_guard(PrecisionConfig {
            parameter_dtype: DType::F32,
            runtime_dtype: DType::BF16,
            allow_parameter_dtype_copies: false,
        });
        reset_default_activation_dtype_to_runtime();
        set_default_kv_cache_dtype(DType::F16);

        with_runtime_component_dtypes(Some(DType::F32), Some(DType::BF16), || {
            assert_eq!(default_runtime_dtype(), DType::BF16);
            assert_eq!(default_activation_dtype(), DType::F32);
            assert_eq!(default_kv_cache_dtype(), DType::BF16);
            assert!(!activation_dtype_follows_runtime());
            assert!(!kv_cache_dtype_follows_runtime());
        });

        assert_eq!(default_runtime_dtype(), DType::BF16);
        assert_eq!(default_activation_dtype(), DType::BF16);
        assert_eq!(default_kv_cache_dtype(), DType::F16);
        assert!(activation_dtype_follows_runtime());
        assert!(!kv_cache_dtype_follows_runtime());
    }
}
