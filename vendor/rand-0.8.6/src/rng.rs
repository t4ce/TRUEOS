// Copyright 2018 Developers of the Rand project.
// Copyright 2013-2017 The Rust Project Developers.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! [`Rng`] trait

use rand_core::{Error, RngCore};
use crate::distributions::uniform::{SampleRange, SampleUniform};
use crate::distributions::{self, Distribution, Standard};
use core::num::Wrapping;
use core::{mem, slice};

/// An automatically-implemented extension trait on [`RngCore`] providing high-level
/// generic methods for sampling values and other convenience methods.
///
/// This is the primary trait to use when generating random values.
///
/// # Generic usage
///
/// The basic pattern is `fn foo<R: Rng + ?Sized>(rng: &mut R)`. Some
/// things are worth noting here:
///
/// - Since `Rng: RngCore` and every `RngCore` implements `Rng`, it makes no
///   difference whether we use `R: Rng` or `R: RngCore`.
/// - The `+ ?Sized` un-bounding allows functions to be called directly on
///   type-erased references; i.e. `foo(r)` where `r: &mut dyn RngCore`. Without
///   this it would be necessary to write `foo(&mut r)`.
///
/// An alternative pattern is possible: `fn foo<R: Rng>(rng: R)`. This has some
/// trade-offs. It allows the argument to be consumed directly without a `&mut`
/// (which is how `from_rng(thread_rng())` works); also it still works directly
/// on references (including type-erased references). Unfortunately within the
/// function `foo` it is not known whether `rng` is a reference type or not,
/// hence many uses of `rng` require an extra reference, either explicitly
/// (`distr.sample(&mut rng)`) or implicitly (`rng.gen()`); one may hope the
/// optimiser can remove redundant references later.
///
/// Example:
///
/// ```
/// # use rand::thread_rng;
/// use rand::Rng;
///
/// fn foo<R: Rng + ?Sized>(rng: &mut R) -> f32 {
///     rng.gen()
/// }
///
/// # let v = foo(&mut thread_rng());
/// ```
pub trait Rng: RngCore {
    /// Return a random value supporting the [`Standard`] distribution.
    ///
    /// # Example
    ///
    /// ```
    /// use rand::{thread_rng, Rng};
    ///
    /// let mut rng = thread_rng();
    /// let x: u32 = rng.gen();
    /// println!("{}", x);
    /// println!("{:?}", rng.gen::<(f64, bool)>());
    /// ```
    ///
    /// # Arrays and tuples
    ///
    /// The `rng.gen()` method is able to generate arrays (up to 32 elements)
    /// and tuples (up to 12 elements), so long as all element types can be
    /// generated.
    /// When using `rustc` ≥ 1.51, enable the `min_const_gen` feature to support
    /// arrays larger than 32 elements.
    ///
    /// For arrays of integers, especially for those with small element types
    /// (< 64 bit), it will likely be faster to instead use [`Rng::fill`].
    ///
    /// ```
    /// use rand::{thread_rng, Rng};
    ///
    /// let mut rng = thread_rng();
    /// let tuple: (u8, i32, char) = rng.gen(); // arbitrary tuple support
    ///
    /// let arr1: [f32; 32] = rng.gen();        // array construction
    /// let mut arr2 = [0u8; 128];
    /// rng.fill(&mut arr2);                    // array fill
    /// ```
    ///
    /// [`Standard`]: distributions::Standard
    #[inline]
    fn gen<T>(&mut self) -> T
    where Standard: Distribution<T> {
        Standard.sample(self)
    }

    /// Generate a random value in the given range.
    ///
    /// This function is optimised for the case that only a single sample is
    /// made from the given range. See also the [`Uniform`] distribution
    /// type which may be faster if sampling from the same range repeatedly.
    ///
    /// Only `gen_range(low..high)` and `gen_range(low..=high)` are supported.
    ///
    /// # Panics
    ///
    /// Panics if the range is empty.
    ///
    /// # Example
    ///
    /// ```
    /// use rand::{thread_rng, Rng};
    ///
    /// let mut rng = thread_rng();
    ///
    /// // Exclusive range
    /// let n: u32 = rng.gen_range(0..10);
    /// println!("{}", n);
    /// let m: f64 = rng.gen_range(-40.0..1.3e5);
    /// println!("{}", m);
    ///
    /// // Inclusive range
    /// let n: u32 = rng.gen_range(0..=10);
    /// println!("{}", n);
    /// ```
    ///
    /// [`Uniform`]: distributions::uniform::Uniform
    fn gen_range<T, R>(&mut self, range: R) -> T
    where
        T: SampleUniform,
        R: SampleRange<T>
    {
        assert!(!range.is_empty(), "cannot sample empty range");
        range.sample_single(self)
    }

    /// Sample a new value, using the given distribution.
    ///
    /// ### Example
    ///
    /// ```
    /// use rand::{thread_rng, Rng};
    /// use rand::distributions::Uniform;
    ///
    /// let mut rng = thread_rng();
    /// let x = rng.sample(Uniform::new(10u32, 15));
    /// // Type annotation requires two types, the type and distribution; the
    /// // distribution can be inferred.
    /// let y = rng.sample::<u16, _>(Uniform::new(10, 15));
    /// ```
    fn sample<T, D: Distribution<T>>(&mut self, distr: D) -> T {
        distr.sample(self)
    }

    /// Create an iterator that generates values using the given distribution.
    ///
    /// Note that this function takes its arguments by value. This works since
    /// `(&mut R): Rng where R: Rng` and
    /// `(&D): Distribution where D: Distribution`,
    /// however borrowing is not automatic hence `rng.sample_iter(...)` may
    /// need to be replaced with `(&mut rng).sample_iter(...)`.
    ///
    /// # Example
    ///
    /// ```
    /// use rand::{thread_rng, Rng};
    /// use rand::distributions::{Alphanumeric, Uniform, Standard};
    ///
    /// let mut rng = thread_rng();
    ///
    /// // Vec of 16 x f32:
    /// let v: Vec<f32> = (&mut rng).sample_iter(Standard).take(16).collect();
    ///
    /// // String:
    /// let s: String = (&mut rng).sample_iter(Alphanumeric)
    ///     .take(7)
    ///     .map(char::from)
    ///     .collect();
    ///
    /// // Combined values
    /// println!("{:?}", (&mut rng).sample_iter(Standard).take(5)
    ///                              .collect::<Vec<(f64, bool)>>());
    ///
    /// // Dice-rolling:
    /// let die_range = Uniform::new_inclusive(1, 6);
    /// let mut roll_die = (&mut rng).sample_iter(die_range);
    /// while roll_die.next().unwrap() != 6 {
    ///     println!("Not a 6; rolling again!");
    /// }
    /// ```
    fn sample_iter<T, D>(self, distr: D) -> distributions::DistIter<D, Self, T>
    where
        D: Distribution<T>,
        Self: Sized,
    {
        distr.sample_iter(self)
    }

    /// Fill any type implementing [`Fill`] with random data
    ///
    /// The distribution is expected to be uniform with portable results, but
    /// this cannot be guaranteed for third-party implementations.
    ///
    /// This is identical to [`try_fill`] except that it panics on error.
    ///
    /// # Example
    ///
    /// ```
    /// use rand::{thread_rng, Rng};
    ///
    /// let mut arr = [0i8; 20];
    /// thread_rng().fill(&mut arr[..]);
    /// ```
    ///
    /// [`fill_bytes`]: RngCore::fill_bytes
    /// [`try_fill`]: Rng::try_fill
    fn fill<T: Fill + ?Sized>(&mut self, dest: &mut T) {
        dest.try_fill(self).unwrap_or_else(|_| panic!("Rng::fill failed"))
    }

    /// Fill any type implementing [`Fill`] with random data
    ///
    /// The distribution is expected to be uniform with portable results, but
    /// this cannot be guaranteed for third-party implementations.
    ///
    /// This is identical to [`fill`] except that it forwards errors.
    ///
    /// # Example
    ///
    /// ```
    /// # use rand::Error;
    /// use rand::{thread_rng, Rng};
    ///
    /// # fn try_inner() -> Result<(), Error> {
    /// let mut arr = [0u64; 4];
    /// thread_rng().try_fill(&mut arr[..])?;
    /// # Ok(())
    /// # }
    ///
    /// # try_inner().unwrap()
    /// ```
    ///
    /// [`try_fill_bytes`]: RngCore::try_fill_bytes
    /// [`fill`]: Rng::fill
    fn try_fill<T: Fill + ?Sized>(&mut self, dest: &mut T) -> Result<(), Error> {
        dest.try_fill(self)
    }

    /// Return a bool with a probability `p` of being true.
    ///
    /// See also the [`Bernoulli`] distribution, which may be faster if
    /// sampling from the same probability repeatedly.
    ///
    /// # Example
    ///
    /// ```
    /// use rand::{thread_rng, Rng};
    ///
    /// let mut rng = thread_rng();
    /// println!("{}", rng.gen_bool(1.0 / 3.0));
    /// ```
    ///
    /// # Panics
    ///
    /// If `p < 0` or `p > 1`.
    ///
    /// [`Bernoulli`]: distributions::Bernoulli
    #[inline]
    fn gen_bool(&mut self, p: f64) -> bool {
        let d = distributions::Bernoulli::new(p).unwrap();
        self.sample(d)
    }

    /// Return a bool with a probability of `numerator/denominator` of being
    /// true. I.e. `gen_ratio(2, 3)` has chance of 2 in 3, or about 67%, of
    /// returning true. If `numerator == denominator`, then the returned value
    /// is guaranteed to be `true`. If `numerator == 0`, then the returned
    /// value is guaranteed to be `false`.
    ///
    /// See also the [`Bernoulli`] distribution, which may be faster if
    /// sampling from the same `numerator` and `denominator` repeatedly.
    ///
    /// # Panics
    ///
    /// If `denominator == 0` or `numerator > denominator`.
    ///
    /// # Example
    ///
    /// ```
    /// use rand::{thread_rng, Rng};
    ///
    /// let mut rng = thread_rng();
    /// println!("{}", rng.gen_ratio(2, 3));
    /// ```
    ///
    /// [`Bernoulli`]: distributions::Bernoulli
    #[inline]
    fn gen_ratio(&mut self, numerator: u32, denominator: u32) -> bool {
        let d = distributions::Bernoulli::from_ratio(numerator, denominator).unwrap();
        self.sample(d)
    }
}

impl<R: RngCore + ?Sized> Rng for R {}

/// Types which may be filled with random data
///
/// This trait allows arrays to be efficiently filled with random data.
///
/// Implementations are expected to be portable across machines unless
/// clearly documented otherwise (see the
/// [Chapter on Portability](https://rust-random.github.io/book/portability.html)).
pub trait Fill {
    /// Fill self with random data
    fn try_fill<R: Rng + ?Sized>(&mut self, rng: &mut R) -> Result<(), Error>;
}

macro_rules! impl_fill_each {
    () => {};
    ($t:ty) => {
        impl Fill for [$t] {
            fn try_fill<R: Rng + ?Sized>(&mut self, rng: &mut R) -> Result<(), Error> {
                for elt in self.iter_mut() {
                    *elt = rng.gen();
                }
                Ok(())
            }
        }
    };
    ($t:ty, $($tt:ty,)*) => {
        impl_fill_each!($t);
        impl_fill_each!($($tt,)*);
    };
}

impl_fill_each!(bool, char, f32, f64,);

impl Fill for [u8] {
    fn try_fill<R: Rng + ?Sized>(&mut self, rng: &mut R) -> Result<(), Error> {
        rng.try_fill_bytes(self)
    }
}

macro_rules! impl_fill {
    () => {};
    ($t:ty) => {
        impl Fill for [$t] {
            #[inline(never)] // in micro benchmarks, this improves performance
            fn try_fill<R: Rng + ?Sized>(&mut self, rng: &mut R) -> Result<(), Error> {
                if self.len() > 0 {
                    rng.try_fill_bytes(unsafe {
                        slice::from_raw_parts_mut(self.as_mut_ptr()
                            as *mut u8,
                            self.len() * mem::size_of::<$t>()
                        )
                    })?;
                    for x in self {
                        *x = x.to_le();
                    }
                }
                Ok(())
            }
        }

        impl Fill for [Wrapping<$t>] {
            #[inline(never)]
            fn try_fill<R: Rng + ?Sized>(&mut self, rng: &mut R) -> Result<(), Error> {
                if self.len() > 0 {
                    rng.try_fill_bytes(unsafe {
                        slice::from_raw_parts_mut(self.as_mut_ptr()
                            as *mut u8,
                            self.len() * mem::size_of::<$t>()
                        )
                    })?;
                    for x in self {
                    *x = Wrapping(x.0.to_le());
                    }
                }
                Ok(())
            }
        }
    };
    ($t:ty, $($tt:ty,)*) => {
        impl_fill!($t);
        // TODO: this could replace above impl once Rust #32463 is fixed
        // impl_fill!(Wrapping<$t>);
        impl_fill!($($tt,)*);
    }
}

impl_fill!(u16, u32, u64, usize, u128,);
impl_fill!(i8, i16, i32, i64, isize, i128,);

#[cfg_attr(docsrs, doc(cfg(feature = "min_const_gen")))]
#[cfg(feature = "min_const_gen")]
impl<T, const N: usize> Fill for [T; N]
where [T]: Fill
{
    fn try_fill<R: Rng + ?Sized>(&mut self, rng: &mut R) -> Result<(), Error> {
        self[..].try_fill(rng)
    }
}

#[cfg(not(feature = "min_const_gen"))]
macro_rules! impl_fill_arrays {
    ($n:expr,) => {};
    ($n:expr, $N:ident) => {
        impl<T> Fill for [T; $n] where [T]: Fill {
            fn try_fill<R: Rng + ?Sized>(&mut self, rng: &mut R) -> Result<(), Error> {
                self[..].try_fill(rng)
            }
        }
    };
    ($n:expr, $N:ident, $($NN:ident,)*) => {
        impl_fill_arrays!($n, $N);
        impl_fill_arrays!($n - 1, $($NN,)*);
    };
    (!div $n:expr,) => {};
    (!div $n:expr, $N:ident, $($NN:ident,)*) => {
        impl_fill_arrays!($n, $N);
        impl_fill_arrays!(!div $n / 2, $($NN,)*);
    };
}
#[cfg(not(feature = "min_const_gen"))]
