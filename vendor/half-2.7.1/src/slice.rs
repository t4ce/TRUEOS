//! Contains utility functions and traits to convert between slices of [`u16`] bits and [`struct@f16`] or
//! [`struct@bf16`] numbers.
//!
//! The utility [`HalfBitsSliceExt`] sealed extension trait is implemented for `[u16]` slices,
//! while the utility [`HalfFloatSliceExt`] sealed extension trait is implemented for both `[f16]`
//! and `[bf16]` slices. These traits provide efficient conversions and reinterpret casting of
//! larger buffers of floating point values, and are automatically included in the
//! [`prelude`][crate::prelude] module.

use crate::{bf16, binary16::arch, f16};
#[cfg(feature = "alloc")]
#[allow(unused_imports)]
use alloc::{vec, vec::Vec};
use zerocopy::{transmute_mut, transmute_ref};

/// Extensions to `[f16]` and `[bf16]` slices to support conversion and reinterpret operations.
///
/// This trait is sealed and cannot be implemented outside of this crate.
pub trait HalfFloatSliceExt: private::SealedHalfFloatSlice {
    /// Reinterprets a slice of [`struct@f16`] or [`struct@bf16`] numbers as a slice of [`u16`] bits.
    ///
    /// This is a zero-copy operation. The reinterpreted slice has the same lifetime and memory
    /// location as `self`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use half::prelude::*;
    /// let float_buffer = [f16::from_f32(1.), f16::from_f32(2.), f16::from_f32(3.)];
    /// let int_buffer = float_buffer.reinterpret_cast();
    ///
    /// assert_eq!(int_buffer, [float_buffer[0].to_bits(), float_buffer[1].to_bits(), float_buffer[2].to_bits()]);
    /// ```
    #[must_use]
    fn reinterpret_cast(&self) -> &[u16];

    /// Reinterprets a mutable slice of [`struct@f16`] or [`struct@bf16`] numbers as a mutable slice of [`u16`].
    /// bits
    ///
    /// This is a zero-copy operation. The transmuted slice has the same lifetime as the original,
    /// which prevents mutating `self` as long as the returned `&mut [u16]` is borrowed.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use half::prelude::*;
    /// let mut float_buffer = [f16::from_f32(1.), f16::from_f32(2.), f16::from_f32(3.)];
    ///
    /// {
    ///     let int_buffer = float_buffer.reinterpret_cast_mut();
    ///
    ///     assert_eq!(int_buffer, [f16::from_f32(1.).to_bits(), f16::from_f32(2.).to_bits(), f16::from_f32(3.).to_bits()]);
    ///
    ///     // Mutating the u16 slice will mutating the original
    ///     int_buffer[0] = 0;
    /// }
    ///
    /// // Note that we need to drop int_buffer before using float_buffer again or we will get a borrow error.
    /// assert_eq!(float_buffer, [f16::from_f32(0.), f16::from_f32(2.), f16::from_f32(3.)]);
    /// ```
    #[must_use]
    fn reinterpret_cast_mut(&mut self) -> &mut [u16];

    /// Converts all of the elements of a `[f32]` slice into [`struct@f16`] or [`struct@bf16`] values in `self`.
    ///
    /// The length of `src` must be the same as `self`.
    ///
    /// The conversion operation is vectorized over the slice, meaning the conversion may be more
    /// efficient than converting individual elements on some hardware that supports SIMD
    /// conversions. See [crate documentation](crate) for more information on hardware conversion
    /// support.
    ///
    /// # Panics
    ///
    /// This function will panic if the two slices have different lengths.
    ///
    /// # Examples
    /// ```rust
    /// # use half::prelude::*;
    /// // Initialize an empty buffer
    /// let mut buffer = [0u16; 4];
    /// let buffer = buffer.reinterpret_cast_mut::<f16>();
    ///
    /// let float_values = [1., 2., 3., 4.];
    ///
    /// // Now convert
    /// buffer.convert_from_f32_slice(&float_values);
    ///
    /// assert_eq!(buffer, [f16::from_f32(1.), f16::from_f32(2.), f16::from_f32(3.), f16::from_f32(4.)]);
    /// ```
    fn convert_from_f32_slice(&mut self, src: &[f32]);

    /// Converts all of the elements of a `[f64]` slice into [`struct@f16`] or [`struct@bf16`] values in `self`.
    ///
    /// The length of `src` must be the same as `self`.
    ///
    /// The conversion operation is vectorized over the slice, meaning the conversion may be more
    /// efficient than converting individual elements on some hardware that supports SIMD
    /// conversions. See [crate documentation](crate) for more information on hardware conversion
    /// support.
    ///
    /// # Panics
    ///
    /// This function will panic if the two slices have different lengths.
    ///
    /// # Examples
    /// ```rust
    /// # use half::prelude::*;
    /// // Initialize an empty buffer
    /// let mut buffer = [0u16; 4];
    /// let buffer = buffer.reinterpret_cast_mut::<f16>();
    ///
    /// let float_values = [1., 2., 3., 4.];
    ///
    /// // Now convert
    /// buffer.convert_from_f64_slice(&float_values);
    ///
    /// assert_eq!(buffer, [f16::from_f64(1.), f16::from_f64(2.), f16::from_f64(3.), f16::from_f64(4.)]);
    /// ```
    fn convert_from_f64_slice(&mut self, src: &[f64]);

    /// Converts all of the [`struct@f16`] or [`struct@bf16`] elements of `self` into [`f32`] values in `dst`.
    ///
    /// The length of `src` must be the same as `self`.
    ///
    /// The conversion operation is vectorized over the slice, meaning the conversion may be more
    /// efficient than converting individual elements on some hardware that supports SIMD
    /// conversions. See [crate documentation](crate) for more information on hardware conversion
    /// support.
    ///
    /// # Panics
    ///
    /// This function will panic if the two slices have different lengths.
    ///
    /// # Examples
    /// ```rust
    /// # use half::prelude::*;
    /// // Initialize an empty buffer
    /// let mut buffer = [0f32; 4];
    ///
    /// let half_values = [f16::from_f32(1.), f16::from_f32(2.), f16::from_f32(3.), f16::from_f32(4.)];
    ///
    /// // Now convert
    /// half_values.convert_to_f32_slice(&mut buffer);
    ///
    /// assert_eq!(buffer, [1., 2., 3., 4.]);
    /// ```
    fn convert_to_f32_slice(&self, dst: &mut [f32]);

    /// Converts all of the [`struct@f16`] or [`struct@bf16`] elements of `self` into [`f64`] values in `dst`.
    ///
    /// The length of `src` must be the same as `self`.
    ///
    /// The conversion operation is vectorized over the slice, meaning the conversion may be more
    /// efficient than converting individual elements on some hardware that supports SIMD
    /// conversions. See [crate documentation](crate) for more information on hardware conversion
    /// support.
    ///
    /// # Panics
    ///
    /// This function will panic if the two slices have different lengths.
    ///
    /// # Examples
    /// ```rust
    /// # use half::prelude::*;
    /// // Initialize an empty buffer
    /// let mut buffer = [0f64; 4];
    ///
    /// let half_values = [f16::from_f64(1.), f16::from_f64(2.), f16::from_f64(3.), f16::from_f64(4.)];
    ///
    /// // Now convert
    /// half_values.convert_to_f64_slice(&mut buffer);
    ///
    /// assert_eq!(buffer, [1., 2., 3., 4.]);
    /// ```
    fn convert_to_f64_slice(&self, dst: &mut [f64]);

    // Because trait is sealed, we can get away with different interfaces between features.

    /// Converts all of the [`struct@f16`] or [`struct@bf16`] elements of `self` into [`f32`] values in a new
    /// vector
    ///
    /// The conversion operation is vectorized over the slice, meaning the conversion may be more
    /// efficient than converting individual elements on some hardware that supports SIMD
    /// conversions. See [crate documentation](crate) for more information on hardware conversion
    /// support.
    ///
    /// This method is only available with the `std` or `alloc` feature.
    ///
    /// # Examples
    /// ```rust
    /// # use half::prelude::*;
    /// let half_values = [f16::from_f32(1.), f16::from_f32(2.), f16::from_f32(3.), f16::from_f32(4.)];
    /// let vec = half_values.to_f32_vec();
    ///
    /// assert_eq!(vec, vec![1., 2., 3., 4.]);
    /// ```
    #[cfg(any(feature = "alloc", feature = "std"))]
    #[must_use]
    fn to_f32_vec(&self) -> Vec<f32>;

    /// Converts all of the [`struct@f16`] or [`struct@bf16`] elements of `self` into [`f64`] values in a new
    /// vector.
    ///
    /// The conversion operation is vectorized over the slice, meaning the conversion may be more
    /// efficient than converting individual elements on some hardware that supports SIMD
    /// conversions. See [crate documentation](crate) for more information on hardware conversion
    /// support.
    ///
    /// This method is only available with the `std` or `alloc` feature.
    ///
    /// # Examples
    /// ```rust
    /// # use half::prelude::*;
    /// let half_values = [f16::from_f64(1.), f16::from_f64(2.), f16::from_f64(3.), f16::from_f64(4.)];
    /// let vec = half_values.to_f64_vec();
    ///
    /// assert_eq!(vec, vec![1., 2., 3., 4.]);
    /// ```
    #[cfg(feature = "alloc")]
    #[must_use]
    fn to_f64_vec(&self) -> Vec<f64>;
}

/// Extensions to `[u16]` slices to support reinterpret operations.
///
/// This trait is sealed and cannot be implemented outside of this crate.
pub trait HalfBitsSliceExt: private::SealedHalfBitsSlice {
    /// Reinterprets a slice of [`u16`] bits as a slice of [`struct@f16`] or [`struct@bf16`] numbers.
    ///
    /// `H` is the type to cast to, and must be either the [`struct@f16`] or [`struct@bf16`] type.
    ///
    /// This is a zero-copy operation. The reinterpreted slice has the same lifetime and memory
    /// location as `self`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use half::prelude::*;
    /// let int_buffer = [f16::from_f32(1.).to_bits(), f16::from_f32(2.).to_bits(), f16::from_f32(3.).to_bits()];
    /// let float_buffer: &[f16] = int_buffer.reinterpret_cast();
    ///
    /// assert_eq!(float_buffer, [f16::from_f32(1.), f16::from_f32(2.), f16::from_f32(3.)]);
    ///
    /// // You may have to specify the cast type directly if the compiler can't infer the type.
    /// // The following is also valid in Rust.
    /// let typed_buffer = int_buffer.reinterpret_cast::<f16>();
    /// ```
    #[must_use]
    fn reinterpret_cast<H>(&self) -> &[H]
    where
        H: crate::private::SealedHalf;

    /// Reinterprets a mutable slice of [`u16`] bits as a mutable slice of [`struct@f16`] or [`struct@bf16`]
    /// numbers.
    ///
    /// `H` is the type to cast to, and must be either the [`struct@f16`] or [`struct@bf16`] type.
    ///
    /// This is a zero-copy operation. The transmuted slice has the same lifetime as the original,
    /// which prevents mutating `self` as long as the returned `&mut [f16]` is borrowed.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use half::prelude::*;
    /// let mut int_buffer = [f16::from_f32(1.).to_bits(), f16::from_f32(2.).to_bits(), f16::from_f32(3.).to_bits()];
    ///
    /// {
    ///     let float_buffer: &mut [f16] = int_buffer.reinterpret_cast_mut();
    ///
    ///     assert_eq!(float_buffer, [f16::from_f32(1.), f16::from_f32(2.), f16::from_f32(3.)]);
    ///
    ///     // Mutating the f16 slice will mutating the original
    ///     float_buffer[0] = f16::from_f32(0.);
    /// }
    ///
    /// // Note that we need to drop float_buffer before using int_buffer again or we will get a borrow error.
    /// assert_eq!(int_buffer, [f16::from_f32(0.).to_bits(), f16::from_f32(2.).to_bits(), f16::from_f32(3.).to_bits()]);
    ///
    /// // You may have to specify the cast type directly if the compiler can't infer the type.
    /// // The following is also valid in Rust.
    /// let typed_buffer = int_buffer.reinterpret_cast_mut::<f16>();
    /// ```
    #[must_use]
    fn reinterpret_cast_mut<H>(&mut self) -> &mut [H]
    where
        H: crate::private::SealedHalf;
}

mod private {
    use crate::{bf16, f16};

    pub trait SealedHalfFloatSlice {}
    impl SealedHalfFloatSlice for [f16] {}
    impl SealedHalfFloatSlice for [bf16] {}

    pub trait SealedHalfBitsSlice {}
    impl SealedHalfBitsSlice for [u16] {}
}

impl HalfFloatSliceExt for [f16] {
    #[inline]
    fn reinterpret_cast(&self) -> &[u16] {
        transmute_ref!(self)
    }

    #[inline]
    fn reinterpret_cast_mut(&mut self) -> &mut [u16] {
        transmute_mut!(self)
    }

    #[inline]
    fn convert_from_f32_slice(&mut self, src: &[f32]) {
        assert_eq!(
            self.len(),
            src.len(),
            "destination and source slices have different lengths"
        );

        arch::f32_to_f16_slice(src, self.reinterpret_cast_mut())
    }

    #[inline]
    fn convert_from_f64_slice(&mut self, src: &[f64]) {
        assert_eq!(
            self.len(),
            src.len(),
            "destination and source slices have different lengths"
        );

        arch::f64_to_f16_slice(src, self.reinterpret_cast_mut())
    }

    #[inline]
    fn convert_to_f32_slice(&self, dst: &mut [f32]) {
        assert_eq!(
            self.len(),
            dst.len(),
            "destination and source slices have different lengths"
        );

        arch::f16_to_f32_slice(self.reinterpret_cast(), dst)
    }

    #[inline]
    fn convert_to_f64_slice(&self, dst: &mut [f64]) {
        assert_eq!(
            self.len(),
            dst.len(),
            "destination and source slices have different lengths"
        );

        arch::f16_to_f64_slice(self.reinterpret_cast(), dst)
    }

    #[cfg(any(feature = "alloc", feature = "std"))]
    #[inline]
    #[allow(clippy::uninit_vec)]
    fn to_f32_vec(&self) -> Vec<f32> {
        let mut vec = vec![0f32; self.len()];
        self.convert_to_f32_slice(&mut vec);
        vec
    }

    #[cfg(any(feature = "alloc", feature = "std"))]
    #[inline]
    #[allow(clippy::uninit_vec)]
    fn to_f64_vec(&self) -> Vec<f64> {
        let mut vec = vec![0f64; self.len()];
        self.convert_to_f64_slice(&mut vec);
        vec
    }
}

impl HalfFloatSliceExt for [bf16] {
    #[inline]
    fn reinterpret_cast(&self) -> &[u16] {
        transmute_ref!(self)
    }

    #[inline]
    fn reinterpret_cast_mut(&mut self) -> &mut [u16] {
        transmute_mut!(self)
    }

    #[inline]
    fn convert_from_f32_slice(&mut self, src: &[f32]) {
        assert_eq!(
            self.len(),
            src.len(),
            "destination and source slices have different lengths"
        );

        // Just use regular loop here until there's any bf16 SIMD support.
        for (i, f) in src.iter().enumerate() {
            self[i] = bf16::from_f32(*f);
        }
    }

    #[inline]
    fn convert_from_f64_slice(&mut self, src: &[f64]) {
        assert_eq!(
            self.len(),
            src.len(),
            "destination and source slices have different lengths"
        );

        // Just use regular loop here until there's any bf16 SIMD support.
        for (i, f) in src.iter().enumerate() {
            self[i] = bf16::from_f64(*f);
        }
    }

    #[inline]
    fn convert_to_f32_slice(&self, dst: &mut [f32]) {
        assert_eq!(
            self.len(),
            dst.len(),
            "destination and source slices have different lengths"
        );

        // Just use regular loop here until there's any bf16 SIMD support.
        for (i, f) in self.iter().enumerate() {
            dst[i] = f.to_f32();
        }
    }

    #[inline]
    fn convert_to_f64_slice(&self, dst: &mut [f64]) {
        assert_eq!(
            self.len(),
            dst.len(),
            "destination and source slices have different lengths"
        );

        // Just use regular loop here until there's any bf16 SIMD support.
        for (i, f) in self.iter().enumerate() {
            dst[i] = f.to_f64();
        }
    }

    #[cfg(any(feature = "alloc", feature = "std"))]
    #[inline]
    #[allow(clippy::uninit_vec)]
    fn to_f32_vec(&self) -> Vec<f32> {
        let mut vec = vec![0f32; self.len()];
        self.convert_to_f32_slice(&mut vec);
        vec
    }

    #[cfg(any(feature = "alloc", feature = "std"))]
    #[inline]
    #[allow(clippy::uninit_vec)]
    fn to_f64_vec(&self) -> Vec<f64> {
        let mut vec = vec![0f64; self.len()];
        self.convert_to_f64_slice(&mut vec);
        vec
    }
}

impl HalfBitsSliceExt for [u16] {
    // Since we sealed all the traits involved, these are safe.
    #[inline]
    fn reinterpret_cast<H>(&self) -> &[H]
    where
        H: crate::private::SealedHalf,
    {
        transmute_ref!(self)
    }

    #[inline]
    fn reinterpret_cast_mut<H>(&mut self) -> &mut [H]
    where
        H: crate::private::SealedHalf,
    {
        transmute_mut!(self)
    }
}

