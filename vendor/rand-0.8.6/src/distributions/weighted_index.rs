// Copyright 2018 Developers of the Rand project.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Weighted index sampling

use crate::distributions::uniform::{SampleBorrow, SampleUniform, UniformSampler};
use crate::distributions::Distribution;
use crate::Rng;
use core::cmp::PartialOrd;
use ::core::fmt;

// Note that this whole module is only imported if feature="alloc" is enabled.
use alloc::vec::Vec;

#[cfg(feature = "serde1")]
use serde::{Serialize, Deserialize};

/// A distribution using weighted sampling of discrete items
///
/// Sampling a `WeightedIndex` distribution returns the index of a randomly
/// selected element from the iterator used when the `WeightedIndex` was
/// created. The chance of a given element being picked is proportional to the
/// value of the element. The weights can use any type `X` for which an
/// implementation of [`Uniform<X>`] exists.
///
/// # Performance
///
/// Time complexity of sampling from `WeightedIndex` is `O(log N)` where
/// `N` is the number of weights. As an alternative,
/// [`rand_distr::weighted_alias`](https://docs.rs/rand_distr/*/rand_distr/weighted_alias/index.html)
/// supports `O(1)` sampling, but with much higher initialisation cost.
///
/// A `WeightedIndex<X>` contains a `Vec<X>` and a [`Uniform<X>`] and so its
/// size is the sum of the size of those objects, possibly plus some alignment.
///
/// Creating a `WeightedIndex<X>` will allocate enough space to hold `N - 1`
/// weights of type `X`, where `N` is the number of weights. However, since
/// `Vec` doesn't guarantee a particular growth strategy, additional memory
/// might be allocated but not used. Since the `WeightedIndex` object also
/// contains, this might cause additional allocations, though for primitive
/// types, [`Uniform<X>`] doesn't allocate any memory.
///
/// Sampling from `WeightedIndex` will result in a single call to
/// `Uniform<X>::sample` (method of the [`Distribution`] trait), which typically
/// will request a single value from the underlying [`RngCore`], though the
/// exact number depends on the implementation of `Uniform<X>::sample`.
///
/// # Example
///
/// ```
/// use rand::prelude::*;
/// use rand::distributions::WeightedIndex;
///
/// let choices = ['a', 'b', 'c'];
/// let weights = [2,   1,   1];
/// let dist = WeightedIndex::new(&weights).unwrap();
/// let mut rng = thread_rng();
/// for _ in 0..100 {
///     // 50% chance to print 'a', 25% chance to print 'b', 25% chance to print 'c'
///     println!("{}", choices[dist.sample(&mut rng)]);
/// }
///
/// let items = [('a', 0), ('b', 3), ('c', 7)];
/// let dist2 = WeightedIndex::new(items.iter().map(|item| item.1)).unwrap();
/// for _ in 0..100 {
///     // 0% chance to print 'a', 30% chance to print 'b', 70% chance to print 'c'
///     println!("{}", items[dist2.sample(&mut rng)].0);
/// }
/// ```
///
/// [`Uniform<X>`]: crate::distributions::Uniform
/// [`RngCore`]: crate::RngCore
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde1", derive(Serialize, Deserialize))]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
pub struct WeightedIndex<X: SampleUniform + PartialOrd> {
    cumulative_weights: Vec<X>,
    total_weight: X,
    weight_distribution: X::Sampler,
}

impl<X: SampleUniform + PartialOrd> WeightedIndex<X> {
    /// Creates a new a `WeightedIndex` [`Distribution`] using the values
    /// in `weights`. The weights can use any type `X` for which an
    /// implementation of [`Uniform<X>`] exists.
    ///
    /// Returns an error if the iterator is empty, if any weight is `< 0`, or
    /// if its total value is 0.
    ///
    /// [`Uniform<X>`]: crate::distributions::uniform::Uniform
    pub fn new<I>(weights: I) -> Result<WeightedIndex<X>, WeightedError>
    where
        I: IntoIterator,
        I::Item: SampleBorrow<X>,
        X: for<'a> ::core::ops::AddAssign<&'a X> + Clone + Default,
    {
        let mut iter = weights.into_iter();
        let mut total_weight: X = iter.next().ok_or(WeightedError::NoItem)?.borrow().clone();

        let zero = <X as Default>::default();
        if !(total_weight >= zero) {
            return Err(WeightedError::InvalidWeight);
        }

        let mut weights = Vec::<X>::with_capacity(iter.size_hint().0);
        for w in iter {
            // Note that `!(w >= x)` is not equivalent to `w < x` for partially
            // ordered types due to NaNs which are equal to nothing.
            if !(w.borrow() >= &zero) {
                return Err(WeightedError::InvalidWeight);
            }
            weights.push(total_weight.clone());
            total_weight += w.borrow();
        }

        if total_weight == zero {
            return Err(WeightedError::AllWeightsZero);
        }
        let distr = X::Sampler::new(zero, total_weight.clone());

        Ok(WeightedIndex {
            cumulative_weights: weights,
            total_weight,
            weight_distribution: distr,
        })
    }

    /// Update a subset of weights, without changing the number of weights.
    ///
    /// `new_weights` must be sorted by the index.
    ///
    /// Using this method instead of `new` might be more efficient if only a small number of
    /// weights is modified. No allocations are performed, unless the weight type `X` uses
    /// allocation internally.
    ///
    /// In case of error, `self` is not modified.
    pub fn update_weights(&mut self, new_weights: &[(usize, &X)]) -> Result<(), WeightedError>
    where X: for<'a> ::core::ops::AddAssign<&'a X>
            + for<'a> ::core::ops::SubAssign<&'a X>
            + Clone
            + Default {
        if new_weights.is_empty() {
            return Ok(());
        }

        let zero = <X as Default>::default();

        let mut total_weight = self.total_weight.clone();

        // Check for errors first, so we don't modify `self` in case something
        // goes wrong.
        let mut prev_i = None;
        for &(i, w) in new_weights {
            if let Some(old_i) = prev_i {
                if old_i >= i {
                    return Err(WeightedError::InvalidWeight);
                }
            }
            if !(*w >= zero) {
                return Err(WeightedError::InvalidWeight);
            }
            if i > self.cumulative_weights.len() {
                return Err(WeightedError::TooMany);
            }

            let mut old_w = if i < self.cumulative_weights.len() {
                self.cumulative_weights[i].clone()
            } else {
                self.total_weight.clone()
            };
            if i > 0 {
                old_w -= &self.cumulative_weights[i - 1];
            }

            total_weight -= &old_w;
            total_weight += w;
            prev_i = Some(i);
        }
        if total_weight <= zero {
            return Err(WeightedError::AllWeightsZero);
        }

        // Update the weights. Because we checked all the preconditions in the
        // previous loop, this should never panic.
        let mut iter = new_weights.iter();

        let mut prev_weight = zero.clone();
        let mut next_new_weight = iter.next();
        let &(first_new_index, _) = next_new_weight.unwrap();
        let mut cumulative_weight = if first_new_index > 0 {
            self.cumulative_weights[first_new_index - 1].clone()
        } else {
            zero.clone()
        };
        for i in first_new_index..self.cumulative_weights.len() {
            match next_new_weight {
                Some(&(j, w)) if i == j => {
                    cumulative_weight += w;
                    next_new_weight = iter.next();
                }
                _ => {
                    let mut tmp = self.cumulative_weights[i].clone();
                    tmp -= &prev_weight; // We know this is positive.
                    cumulative_weight += &tmp;
                }
            }
            prev_weight = cumulative_weight.clone();
            core::mem::swap(&mut prev_weight, &mut self.cumulative_weights[i]);
        }

        self.total_weight = total_weight;
        self.weight_distribution = X::Sampler::new(zero, self.total_weight.clone());

        Ok(())
    }
}

impl<X> Distribution<usize> for WeightedIndex<X>
where X: SampleUniform + PartialOrd
{
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> usize {
        use ::core::cmp::Ordering;
        let chosen_weight = self.weight_distribution.sample(rng);
        // Find the first item which has a weight *higher* than the chosen weight.
        self.cumulative_weights
            .binary_search_by(|w| {
                if *w <= chosen_weight {
                    Ordering::Less
                } else {
                    Ordering::Greater
                }
            })
            .unwrap_err()
    }
}

/// Error type returned from `WeightedIndex::new`.
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeightedError {
    /// The provided weight collection contains no items.
    NoItem,

    /// A weight is either less than zero, greater than the supported maximum,
    /// NaN, or otherwise invalid.
    InvalidWeight,

    /// All items in the provided weight collection are zero.
    AllWeightsZero,

    /// Too many weights are provided (length greater than `u32::MAX`)
    TooMany,
}

#[cfg(feature = "std")]
impl core::error::Error for WeightedError {}

impl fmt::Display for WeightedError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(match *self {
            WeightedError::NoItem => "No weights provided in distribution",
            WeightedError::InvalidWeight => "A weight is invalid in distribution",
            WeightedError::AllWeightsZero => "All weights are zero in distribution",
            WeightedError::TooMany => "Too many weights (hit u32::MAX) in distribution",
        })
    }
}
