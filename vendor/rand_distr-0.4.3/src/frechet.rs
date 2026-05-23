// Copyright 2021 Developers of the Rand project.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! The Fréchet distribution.

use crate::{Distribution, OpenClosed01};
use ::core::fmt;
use num_traits::Float;
use rand::Rng;

/// Samples floating-point numbers according to the Fréchet distribution
///
/// This distribution has density function:
/// `f(x) = [(x - μ) / σ]^(-1 - α) exp[-(x - μ) / σ]^(-α) α / σ`,
/// where `μ` is the location parameter, `σ` the scale parameter, and `α` the shape parameter.
///
/// # Example
/// ```
/// use rand::prelude::*;
/// use rand_distr::Frechet;
///
/// let val: f64 = thread_rng().sample(Frechet::new(0.0, 1.0, 1.0).unwrap());
/// println!("{}", val);
/// ```
#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "serde1", derive(serde::Serialize, serde::Deserialize))]
pub struct Frechet<F>
where
    F: Float,
    OpenClosed01: Distribution<F>,
{
    location: F,
    scale: F,
    shape: F,
}

/// Error type returned from `Frechet::new`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// location is infinite or NaN
    LocationNotFinite,
    /// scale is not finite positive number
    ScaleNotPositive,
    /// shape is not finite positive number
    ShapeNotPositive,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Error::LocationNotFinite => "location is not finite in Frechet distribution",
            Error::ScaleNotPositive => "scale is not positive and finite in Frechet distribution",
            Error::ShapeNotPositive => "shape is not positive and finite in Frechet distribution",
        })
    }
}

#[cfg(feature = "std")]
#[cfg_attr(doc_cfg, doc(cfg(feature = "std")))]
impl core::error::Error for Error {}

impl<F> Frechet<F>
where
    F: Float,
    OpenClosed01: Distribution<F>,
{
    /// Construct a new `Frechet` distribution with given `location`, `scale`, and `shape`.
    pub fn new(location: F, scale: F, shape: F) -> Result<Frechet<F>, Error> {
        if scale <= F::zero() || scale.is_infinite() || scale.is_nan() {
            return Err(Error::ScaleNotPositive);
        }
        if shape <= F::zero() || shape.is_infinite() || shape.is_nan() {
            return Err(Error::ShapeNotPositive);
        }
        if location.is_infinite() || location.is_nan() {
            return Err(Error::LocationNotFinite);
        }
        Ok(Frechet {
            location,
            scale,
            shape,
        })
    }
}

impl<F> Distribution<F> for Frechet<F>
where
    F: Float,
    OpenClosed01: Distribution<F>,
{
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> F {
        let x: F = rng.sample(OpenClosed01);
        self.location + self.scale * (-x.ln()).powf(-self.shape.recip())
    }
}
