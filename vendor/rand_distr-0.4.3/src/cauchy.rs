// Copyright 2018 Developers of the Rand project.
// Copyright 2016-2017 The Rust Project Developers.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! The Cauchy distribution.

use num_traits::{Float, FloatConst};
use crate::{Distribution, Standard};
use rand::Rng;
use ::core::fmt;

/// The Cauchy distribution `Cauchy(median, scale)`.
///
/// This distribution has a density function:
/// `f(x) = 1 / (pi * scale * (1 + ((x - median) / scale)^2))`
///
/// Note that at least for `f32`, results are not fully portable due to minor
/// differences in the target system's *tan* implementation, `tanf`.
///
/// # Example
///
/// ```
/// use rand_distr::{Cauchy, Distribution};
///
/// let cau = Cauchy::new(2.0, 5.0).unwrap();
/// let v = cau.sample(&mut rand::thread_rng());
/// println!("{} is from a Cauchy(2, 5) distribution", v);
/// ```
#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "serde1", derive(serde::Serialize, serde::Deserialize))]
pub struct Cauchy<F>
where F: Float + FloatConst, Standard: Distribution<F>
{
    median: F,
    scale: F,
}

/// Error type returned from `Cauchy::new`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// `scale <= 0` or `nan`.
    ScaleTooSmall,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Error::ScaleTooSmall => "scale is not positive in Cauchy distribution",
        })
    }
}

#[cfg(feature = "std")]
#[cfg_attr(doc_cfg, doc(cfg(feature = "std")))]
impl core::error::Error for Error {}

impl<F> Cauchy<F>
where F: Float + FloatConst, Standard: Distribution<F>
{
    /// Construct a new `Cauchy` with the given shape parameters
    /// `median` the peak location and `scale` the scale factor.
    pub fn new(median: F, scale: F) -> Result<Cauchy<F>, Error> {
        if !(scale > F::zero()) {
            return Err(Error::ScaleTooSmall);
        }
        Ok(Cauchy { median, scale })
    }
}

impl<F> Distribution<F> for Cauchy<F>
where F: Float + FloatConst, Standard: Distribution<F>
{
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> F {
        // sample from [0, 1)
        let x = Standard.sample(rng);
        // get standard cauchy random number
        // note that π/2 is not exactly representable, even if x=0.5 the result is finite
        let comp_dev = (F::PI() * x).tan();
        // shift and scale according to parameters
        self.median + self.scale * comp_dev
    }
}
