use crate::{Distribution, InverseGaussian, Standard, StandardNormal};
use num_traits::Float;
use rand::Rng;
use core::fmt;

/// Error type returned from `NormalInverseGaussian::new`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// `alpha <= 0` or `nan`.
    AlphaNegativeOrNull,
    /// `|beta| >= alpha` or `nan`.
    AbsoluteBetaNotLessThanAlpha,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Error::AlphaNegativeOrNull => "alpha <= 0 or is NaN in normal inverse Gaussian distribution",
            Error::AbsoluteBetaNotLessThanAlpha => "|beta| >= alpha or is NaN in normal inverse Gaussian distribution",
        })
    }
}

#[cfg(feature = "std")]
#[cfg_attr(doc_cfg, doc(cfg(feature = "std")))]
impl core::error::Error for Error {}

/// The [normal-inverse Gaussian distribution](https://en.wikipedia.org/wiki/Normal-inverse_Gaussian_distribution)
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "serde1", derive(serde::Serialize, serde::Deserialize))]
pub struct NormalInverseGaussian<F>
where
    F: Float,
    StandardNormal: Distribution<F>,
    Standard: Distribution<F>,
{
    alpha: F,
    beta: F,
    inverse_gaussian: InverseGaussian<F>,
}

impl<F> NormalInverseGaussian<F>
where
    F: Float,
    StandardNormal: Distribution<F>,
    Standard: Distribution<F>,
{
    /// Construct a new `NormalInverseGaussian` distribution with the given alpha (tail heaviness) and
    /// beta (asymmetry) parameters.
    pub fn new(alpha: F, beta: F) -> Result<NormalInverseGaussian<F>, Error> {
        if !(alpha > F::zero()) {
            return Err(Error::AlphaNegativeOrNull);
        }

        if !(beta.abs() < alpha) {
            return Err(Error::AbsoluteBetaNotLessThanAlpha);
        }

        let gamma = (alpha * alpha - beta * beta).sqrt();

        let mu = F::one() / gamma;

        let inverse_gaussian = InverseGaussian::new(mu, F::one()).unwrap();

        Ok(Self {
            alpha,
            beta,
            inverse_gaussian,
        })
    }
}

impl<F> Distribution<F> for NormalInverseGaussian<F>
where
    F: Float,
    StandardNormal: Distribution<F>,
    Standard: Distribution<F>,
{
    fn sample<R>(&self, rng: &mut R) -> F
    where R: Rng + ?Sized {
        let inv_gauss = rng.sample(&self.inverse_gaussian);

        self.beta * inv_gauss + inv_gauss.sqrt() * rng.sample(StandardNormal)
    }
}
