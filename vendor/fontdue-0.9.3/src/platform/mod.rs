#[cfg(not(all(any(target_arch = "x86", target_arch = "x86_64"), feature = "simd")))]
mod simd_core;
#[cfg(not(all(any(target_arch = "x86", target_arch = "x86_64"), feature = "simd")))]
pub use simd_core::*;

#[cfg(all(any(target_arch = "x86", target_arch = "x86_64"), feature = "simd"))]
mod simd_x86;
#[cfg(all(any(target_arch = "x86", target_arch = "x86_64"), feature = "simd"))]
pub use simd_x86::*;

mod float;
pub use float::*;
