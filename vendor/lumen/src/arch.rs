#[cfg(all(feature = "arm64-int8-kernels", not(target_arch = "aarch64")))]
compile_error!("feature `arm64-int8-kernels` only supports the `aarch64` target architecture");

#[cfg(all(feature = "arm64-fp-kernels", not(target_arch = "aarch64")))]
compile_error!("feature `arm64-fp-kernels` only supports the `aarch64` target architecture");

#[cfg(all(
    feature = "x86-fp-kernels",
    not(any(target_arch = "x86_64", target_arch = "x86"))
))]
compile_error!("feature `x86-fp-kernels` only supports the `x86_64` or `x86` target architectures");

#[cfg(all(
    feature = "x86-int8-kernels",
    not(any(target_arch = "x86_64", target_arch = "x86"))
))]
compile_error!(
    "feature `x86-int8-kernels` only supports the `x86_64` or `x86` target architectures"
);

#[inline]
pub const fn arm64_int8_kernels_compiled() -> bool {
    cfg!(all(feature = "arm64-int8-kernels", target_arch = "aarch64"))
}

#[inline]
pub const fn arm64_fp_kernels_compiled() -> bool {
    cfg!(all(feature = "arm64-fp-kernels", target_arch = "aarch64"))
}

#[inline]
pub const fn x86_int8_kernels_compiled() -> bool {
    cfg!(all(
        feature = "x86-int8-kernels",
        any(target_arch = "x86_64", target_arch = "x86")
    ))
}

#[inline]
pub const fn x86_fp_kernels_compiled() -> bool {
    cfg!(all(
        feature = "x86-fp-kernels",
        any(target_arch = "x86_64", target_arch = "x86")
    ))
}

#[inline]
pub const fn any_arch_int8_kernels_compiled() -> bool {
    arm64_int8_kernels_compiled() || x86_int8_kernels_compiled()
}

#[inline]
pub fn arm64_i8_kernel_runtime_available() -> bool {
    arm64_int8_kernels_compiled()
}

#[inline]
pub fn arm64_fp_kernel_runtime_available() -> bool {
    arm64_fp_kernels_compiled()
}

#[inline]
pub fn arm64_fp16_kernel_runtime_available() -> bool {
    #[cfg(all(feature = "arm64-fp-kernels", target_arch = "aarch64"))]
    {
        std::arch::is_aarch64_feature_detected!("fp16")
    }
    #[cfg(not(all(feature = "arm64-fp-kernels", target_arch = "aarch64")))]
    {
        false
    }
}

#[inline]
pub fn x86_avx512_fp_kernel_runtime_available() -> bool {
    #[cfg(all(
        feature = "x86-fp-kernels",
        any(target_arch = "x86_64", target_arch = "x86")
    ))]
    {
        std::arch::is_x86_feature_detected!("avx512f")
    }
    #[cfg(not(all(
        feature = "x86-fp-kernels",
        any(target_arch = "x86_64", target_arch = "x86")
    )))]
    {
        false
    }
}

#[inline]
pub fn x86_avx512_bf16_kernel_runtime_available() -> bool {
    #[cfg(all(
        feature = "x86-fp-kernels",
        any(target_arch = "x86_64", target_arch = "x86")
    ))]
    {
        x86_avx512_fp_kernel_runtime_available()
            && std::arch::is_x86_feature_detected!("avx512vl")
            && std::arch::is_x86_feature_detected!("avx512bf16")
    }
    #[cfg(not(all(
        feature = "x86-fp-kernels",
        any(target_arch = "x86_64", target_arch = "x86")
    )))]
    {
        false
    }
}

#[inline]
pub fn x86_fp_kernel_runtime_available() -> bool {
    #[cfg(all(
        feature = "x86-fp-kernels",
        any(target_arch = "x86_64", target_arch = "x86")
    ))]
    {
        std::arch::is_x86_feature_detected!("avx2") && std::arch::is_x86_feature_detected!("fma")
    }
    #[cfg(not(all(
        feature = "x86-fp-kernels",
        any(target_arch = "x86_64", target_arch = "x86")
    )))]
    {
        false
    }
}

#[inline]
pub fn x86_fp16_kernel_runtime_available() -> bool {
    #[cfg(all(
        feature = "x86-fp-kernels",
        any(target_arch = "x86_64", target_arch = "x86")
    ))]
    {
        x86_avx512_fp_kernel_runtime_available()
            || (x86_fp_kernel_runtime_available() && std::arch::is_x86_feature_detected!("f16c"))
    }
    #[cfg(not(all(
        feature = "x86-fp-kernels",
        any(target_arch = "x86_64", target_arch = "x86")
    )))]
    {
        false
    }
}

#[inline]
pub fn x86_i8_kernel_runtime_available() -> bool {
    #[cfg(all(
        feature = "x86-int8-kernels",
        any(target_arch = "x86_64", target_arch = "x86")
    ))]
    {
        std::arch::is_x86_feature_detected!("avx2") && std::arch::is_x86_feature_detected!("fma")
    }
    #[cfg(not(all(
        feature = "x86-int8-kernels",
        any(target_arch = "x86_64", target_arch = "x86")
    )))]
    {
        false
    }
}

#[inline]
pub fn preferred_i8_kernel_backend() -> &'static str {
    if arm64_i8_kernel_runtime_available() {
        "arm64-neon"
    } else if x86_i8_kernel_runtime_available() {
        "x86-avx2"
    } else {
        "portable"
    }
}

#[inline]
pub fn preferred_fp_kernel_backend() -> &'static str {
    if arm64_fp_kernel_runtime_available() {
        "arm64-neon"
    } else if x86_avx512_fp_kernel_runtime_available() {
        "x86-avx512"
    } else if x86_fp_kernel_runtime_available() {
        "x86-avx2"
    } else {
        "portable"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn architecture_specific_i8_kernels_are_disabled_by_default() {
        assert_eq!(
            any_arch_int8_kernels_compiled(),
            arm64_int8_kernels_compiled() || x86_int8_kernels_compiled()
        );
    }

    #[test]
    fn preferred_backend_is_consistent_with_runtime_probes() {
        let backend = preferred_i8_kernel_backend();
        if arm64_i8_kernel_runtime_available() {
            assert_eq!(backend, "arm64-neon");
        } else if x86_i8_kernel_runtime_available() {
            assert_eq!(backend, "x86-avx2");
        } else {
            assert_eq!(backend, "portable");
        }
    }

    #[test]
    fn preferred_fp_backend_is_consistent_with_runtime_probes() {
        let backend = preferred_fp_kernel_backend();
        if arm64_fp_kernel_runtime_available() {
            assert_eq!(backend, "arm64-neon");
        } else if x86_avx512_fp_kernel_runtime_available() {
            assert_eq!(backend, "x86-avx512");
        } else if x86_fp_kernel_runtime_available() {
            assert_eq!(backend, "x86-avx2");
        } else {
            assert_eq!(backend, "portable");
        }
    }

    #[test]
    fn arm64_fp16_probe_is_never_broader_than_fp_backend_probe() {
        assert!(!arm64_fp16_kernel_runtime_available() || arm64_fp_kernel_runtime_available());
    }

    #[test]
    fn x86_fp16_probe_is_never_broader_than_fp_backend_probe() {
        assert!(!x86_fp16_kernel_runtime_available() || x86_fp_kernel_runtime_available());
    }
}
