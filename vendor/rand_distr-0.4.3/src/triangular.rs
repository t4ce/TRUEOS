// Copyright 2018 Developers of the Rand project.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.
//! The triangular distribution.

use num_traits::Float;
use crate::{Distribution, Standard};
use rand::Rng;
use ::core::fmt;

/// The triangular distribution.
///
/// A continuous probability distribution parameterised by a range, and a mode
/// (most likely value) within that range.
///
/// The probability density function is triangular. For a similar distribution
/// with a smooth PDF, see the [`Pert`] distribution.
///
/// # Example
///
/// ```rust
/// use rand_distr::{Triangular, Distribution};
///
/// let d = Triangular::new(0., 5., 2.5).unwrap();
/// let v = d.sample(&mut rand::thread_rng());
/// println!("{} is from a triangular distribution", v);
/// ```
///
/// [`Pert`]: crate::Pert
#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "serde1", derive(serde::Serialize, serde::Deserialize))]
pub struct Triangular<F>
where F: Float, Standard: Distribution<F>
{
    min: F,
    max: F,
    mode: F,
}

/// Error type returned from [`Triangular::new`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriangularError {
    /// `max < min` or `min` or `max` is NaN.
    RangeTooSmall,
    /// `mode < min` or `mode > max` or `mode` is NaN.
    ModeRange,
}

impl fmt::Display for TriangularError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            TriangularError::RangeTooSmall => {
                "requirement min <= max is not met in triangular distribution"
            }
            TriangularError::ModeRange => "mode is outside [min, max] in triangular distribution",
        })
    }
}

#[cfg(feature = "std")]
#[cfg_attr(doc_cfg, doc(cfg(feature = "std")))]
impl core::error::Error for TriangularError {}

impl<F> Triangular<F>
where F: Float, Standard: Distribution<F>
{
    /// Set up the Triangular distribution with defined `min`, `max` and `mode`.
    #[inline]
    pub fn new(min: F, max: F, mode: F) -> Result<Triangular<F>, TriangularError> {
        if !(max >= min) {
            return Err(TriangularError::RangeTooSmall);
        }
        if !(mode >= min && max >= mode) {
            return Err(TriangularError::ModeRange);
        }
        Ok(Triangular { min, max, mode })
    }
}

impl<F> Distribution<F> for Triangular<F>
where F: Float, Standard: Distribution<F>
{
    #[inline]
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> F {
        let f: F = rng.sample(Standard);
        let diff_mode_min = self.mode - self.min;
        let range = self.max - self.min;
        let f_range = f * range;
        if f_range < diff_mode_min {
            self.min + (f_range * diff_mode_min).sqrt()
        } else {
            self.max - ((range - f_range) * (self.max - self.mode)).sqrt()
        }
    }
}
