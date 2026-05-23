// Copyright 2018 Developers of the Rand project.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! The Weibull distribution.

use num_traits::Float;
use crate::{Distribution, OpenClosed01};
use rand::Rng;
use ::core::fmt;

/// Samples floating-point numbers according to the Weibull distribution
///
/// # Example
/// ```
/// use rand::prelude::*;
/// use rand_distr::Weibull;
///
/// let val: f64 = thread_rng().sample(Weibull::new(1., 10.).unwrap());
/// println!("{}", val);
/// ```
#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "serde1", derive(serde::Serialize, serde::Deserialize))]
pub struct Weibull<F>
where F: Float, OpenClosed01: Distribution<F>
{
    inv_shape: F,
    scale: F,
}

/// Error type returned from `Weibull::new`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// `scale <= 0` or `nan`.
    ScaleTooSmall,
    /// `shape <= 0` or `nan`.
    ShapeTooSmall,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Error::ScaleTooSmall => "scale is not positive in Weibull distribution",
            Error::ShapeTooSmall => "shape is not positive in Weibull distribution",
        })
    }
}

#[cfg(feature = "std")]
#[cfg_attr(doc_cfg, doc(cfg(feature = "std")))]
impl core::error::Error for Error {}

impl<F> Weibull<F>
where F: Float, OpenClosed01: Distribution<F>
{
    /// Construct a new `Weibull` distribution with given `scale` and `shape`.
    pub fn new(scale: F, shape: F) -> Result<Weibull<F>, Error> {
        if !(scale > F::zero()) {
            return Err(Error::ScaleTooSmall);
        }
        if !(shape > F::zero()) {
            return Err(Error::ShapeTooSmall);
        }
        Ok(Weibull {
            inv_shape: F::from(1.).unwrap() / shape,
            scale,
        })
    }
}

impl<F> Distribution<F> for Weibull<F>
where F: Float, OpenClosed01: Distribution<F>
{
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> F {
        let x: F = rng.sample(OpenClosed01);
        self.scale * (-x.ln()).powf(self.inv_shape)
    }
}
