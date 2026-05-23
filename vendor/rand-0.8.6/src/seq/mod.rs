// Copyright 2018 Developers of the Rand project.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Sequence-related functionality
//!
//! This module provides:
//!
//! *   [`SliceRandom`] slice sampling and mutation
//! *   [`IteratorRandom`] iterator sampling
//! *   [`index::sample`] low-level API to choose multiple indices from
//!     `0..length`
//!
//! Also see:
//!
//! *   [`crate::distributions::WeightedIndex`] distribution which provides
//!     weighted index sampling.
//!
//! In order to make results reproducible across 32-64 bit architectures, all
//! `usize` indices are sampled as a `u32` where possible (also providing a
//! small performance boost in some cases).


#[cfg(feature = "alloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
pub mod index;

#[cfg(feature = "alloc")] use core::ops::Index;

#[cfg(feature = "alloc")] use alloc::vec::Vec;

#[cfg(feature = "alloc")]
use crate::distributions::uniform::{SampleBorrow, SampleUniform};
#[cfg(feature = "alloc")] use crate::distributions::WeightedError;
use crate::Rng;

/// Extension trait on slices, providing random mutation and sampling methods.
///
/// This trait is implemented on all `[T]` slice types, providing several
/// methods for choosing and shuffling elements. You must `use` this trait:
///
/// ```
/// use rand::seq::SliceRandom;
///
/// let mut rng = rand::thread_rng();
/// let mut bytes = "Hello, random!".to_string().into_bytes();
/// bytes.shuffle(&mut rng);
/// let str = String::from_utf8(bytes).unwrap();
/// println!("{}", str);
/// ```
/// Example output (non-deterministic):
/// ```none
/// l,nmroHado !le
/// ```
pub trait SliceRandom {
    /// The element type.
    type Item;

    /// Returns a reference to one random element of the slice, or `None` if the
    /// slice is empty.
    ///
    /// For slices, complexity is `O(1)`.
    ///
    /// # Example
    ///
    /// ```
    /// use rand::thread_rng;
    /// use rand::seq::SliceRandom;
    ///
    /// let choices = [1, 2, 4, 8, 16, 32];
    /// let mut rng = thread_rng();
    /// println!("{:?}", choices.choose(&mut rng));
    /// assert_eq!(choices[..0].choose(&mut rng), None);
    /// ```
    fn choose<R>(&self, rng: &mut R) -> Option<&Self::Item>
    where R: Rng + ?Sized;

    /// Returns a mutable reference to one random element of the slice, or
    /// `None` if the slice is empty.
    ///
    /// For slices, complexity is `O(1)`.
    fn choose_mut<R>(&mut self, rng: &mut R) -> Option<&mut Self::Item>
    where R: Rng + ?Sized;

    /// Chooses `amount` elements from the slice at random, without repetition,
    /// and in random order. The returned iterator is appropriate both for
    /// collection into a `Vec` and filling an existing buffer (see example).
    ///
    /// In case this API is not sufficiently flexible, use [`index::sample`].
    ///
    /// For slices, complexity is the same as [`index::sample`].
    ///
    /// # Example
    /// ```
    /// use rand::seq::SliceRandom;
    ///
    /// let mut rng = &mut rand::thread_rng();
    /// let sample = "Hello, audience!".as_bytes();
    ///
    /// // collect the results into a vector:
    /// let v: Vec<u8> = sample.choose_multiple(&mut rng, 3).cloned().collect();
    ///
    /// // store in a buffer:
    /// let mut buf = [0u8; 5];
    /// for (b, slot) in sample.choose_multiple(&mut rng, buf.len()).zip(buf.iter_mut()) {
    ///     *slot = *b;
    /// }
    /// ```
    #[cfg(feature = "alloc")]
    #[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
    fn choose_multiple<R>(&self, rng: &mut R, amount: usize) -> SliceChooseIter<'_, Self, Self::Item>
    where R: Rng + ?Sized;

    /// Similar to [`choose`], but where the likelihood of each outcome may be
    /// specified.
    ///
    /// The specified function `weight` maps each item `x` to a relative
    /// likelihood `weight(x)`. The probability of each item being selected is
    /// therefore `weight(x) / s`, where `s` is the sum of all `weight(x)`.
    ///
    /// For slices of length `n`, complexity is `O(n)`.
    /// See also [`choose_weighted_mut`], [`distributions::weighted`].
    ///
    /// # Example
    ///
    /// ```
    /// use rand::prelude::*;
    ///
    /// let choices = [('a', 2), ('b', 1), ('c', 1)];
    /// let mut rng = thread_rng();
    /// // 50% chance to print 'a', 25% chance to print 'b', 25% chance to print 'c'
    /// println!("{:?}", choices.choose_weighted(&mut rng, |item| item.1).unwrap().0);
    /// ```
    /// [`choose`]: SliceRandom::choose
    /// [`choose_weighted_mut`]: SliceRandom::choose_weighted_mut
    /// [`distributions::weighted`]: crate::distributions::weighted
    #[cfg(feature = "alloc")]
    #[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
    fn choose_weighted<R, F, B, X>(
        &self, rng: &mut R, weight: F,
    ) -> Result<&Self::Item, WeightedError>
    where
        R: Rng + ?Sized,
        F: Fn(&Self::Item) -> B,
        B: SampleBorrow<X>,
        X: SampleUniform
            + for<'a> ::core::ops::AddAssign<&'a X>
            + ::core::cmp::PartialOrd<X>
            + Clone
            + Default;

    /// Similar to [`choose_mut`], but where the likelihood of each outcome may
    /// be specified.
    ///
    /// The specified function `weight` maps each item `x` to a relative
    /// likelihood `weight(x)`. The probability of each item being selected is
    /// therefore `weight(x) / s`, where `s` is the sum of all `weight(x)`.
    ///
    /// For slices of length `n`, complexity is `O(n)`.
    /// See also [`choose_weighted`], [`distributions::weighted`].
    ///
    /// [`choose_mut`]: SliceRandom::choose_mut
    /// [`choose_weighted`]: SliceRandom::choose_weighted
    /// [`distributions::weighted`]: crate::distributions::weighted
    #[cfg(feature = "alloc")]
    #[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
    fn choose_weighted_mut<R, F, B, X>(
        &mut self, rng: &mut R, weight: F,
    ) -> Result<&mut Self::Item, WeightedError>
    where
        R: Rng + ?Sized,
        F: Fn(&Self::Item) -> B,
        B: SampleBorrow<X>,
        X: SampleUniform
            + for<'a> ::core::ops::AddAssign<&'a X>
            + ::core::cmp::PartialOrd<X>
            + Clone
            + Default;

    /// Similar to [`choose_multiple`], but where the likelihood of each element's
    /// inclusion in the output may be specified. The elements are returned in an
    /// arbitrary, unspecified order.
    ///
    /// The specified function `weight` maps each item `x` to a relative
    /// likelihood `weight(x)`. The probability of each item being selected is
    /// therefore `weight(x) / s`, where `s` is the sum of all `weight(x)`.
    ///
    /// If all of the weights are equal, even if they are all zero, each element has
    /// an equal likelihood of being selected.
    ///
    /// The complexity of this method depends on the feature `partition_at_index`.
    /// If the feature is enabled, then for slices of length `n`, the complexity
    /// is `O(n)` space and `O(n)` time. Otherwise, the complexity is `O(n)` space and
    /// `O(n * log amount)` time.
    ///
    /// # Example
    ///
    /// ```
    /// use rand::prelude::*;
    ///
    /// let choices = [('a', 2), ('b', 1), ('c', 1)];
    /// let mut rng = thread_rng();
    /// // First Draw * Second Draw = total odds
    /// // -----------------------
    /// // (50% * 50%) + (25% * 67%) = 41.7% chance that the output is `['a', 'b']` in some order.
    /// // (50% * 50%) + (25% * 67%) = 41.7% chance that the output is `['a', 'c']` in some order.
    /// // (25% * 33%) + (25% * 33%) = 16.6% chance that the output is `['b', 'c']` in some order.
    /// println!("{:?}", choices.choose_multiple_weighted(&mut rng, 2, |item| item.1).unwrap().collect::<Vec<_>>());
    /// ```
    /// [`choose_multiple`]: SliceRandom::choose_multiple
    //
    // Note: this is feature-gated on std due to usage of f64::powf.
    // If necessary, we may use alloc+libm as an alternative (see PR #1089).
    #[cfg(feature = "std")]
    #[cfg_attr(docsrs, doc(cfg(feature = "std")))]
    fn choose_multiple_weighted<R, F, X>(
        &self, rng: &mut R, amount: usize, weight: F,
    ) -> Result<SliceChooseIter<'_, Self, Self::Item>, WeightedError>
    where
        R: Rng + ?Sized,
        F: Fn(&Self::Item) -> X,
        X: Into<f64>;

    /// Shuffle a mutable slice in place.
    ///
    /// For slices of length `n`, complexity is `O(n)`.
    ///
    /// # Example
    ///
    /// ```
    /// use rand::seq::SliceRandom;
    /// use rand::thread_rng;
    ///
    /// let mut rng = thread_rng();
    /// let mut y = [1, 2, 3, 4, 5];
    /// println!("Unshuffled: {:?}", y);
    /// y.shuffle(&mut rng);
    /// println!("Shuffled:   {:?}", y);
    /// ```
    fn shuffle<R>(&mut self, rng: &mut R)
    where R: Rng + ?Sized;

    /// Shuffle a slice in place, but exit early.
    ///
    /// Returns two mutable slices from the source slice. The first contains
    /// `amount` elements randomly permuted. The second has the remaining
    /// elements that are not fully shuffled.
    ///
    /// This is an efficient method to select `amount` elements at random from
    /// the slice, provided the slice may be mutated.
    ///
    /// If you only need to choose elements randomly and `amount > self.len()/2`
    /// then you may improve performance by taking
    /// `amount = values.len() - amount` and using only the second slice.
    ///
    /// If `amount` is greater than the number of elements in the slice, this
    /// will perform a full shuffle.
    ///
    /// For slices, complexity is `O(m)` where `m = amount`.
    fn partial_shuffle<R>(
        &mut self, rng: &mut R, amount: usize,
    ) -> (&mut [Self::Item], &mut [Self::Item])
    where R: Rng + ?Sized;
}

/// Extension trait on iterators, providing random sampling methods.
///
/// This trait is implemented on all iterators `I` where `I: Iterator + Sized`
/// and provides methods for
/// choosing one or more elements. You must `use` this trait:
///
/// ```
/// use rand::seq::IteratorRandom;
///
/// let mut rng = rand::thread_rng();
///
/// let faces = "😀😎😐😕😠😢";
/// println!("I am {}!", faces.chars().choose(&mut rng).unwrap());
/// ```
/// Example output (non-deterministic):
/// ```none
/// I am 😀!
/// ```
pub trait IteratorRandom: Iterator + Sized {
    /// Choose one element at random from the iterator.
    ///
    /// Returns `None` if and only if the iterator is empty.
    ///
    /// This method uses [`Iterator::size_hint`] for optimisation. With an
    /// accurate hint and where [`Iterator::nth`] is a constant-time operation
    /// this method can offer `O(1)` performance. Where no size hint is
    /// available, complexity is `O(n)` where `n` is the iterator length.
    /// Partial hints (where `lower > 0`) also improve performance.
    ///
    /// Note that the output values and the number of RNG samples used
    /// depends on size hints. In particular, `Iterator` combinators that don't
    /// change the values yielded but change the size hints may result in
    /// `choose` returning different elements. If you want consistent results
    /// and RNG usage consider using [`IteratorRandom::choose_stable`].
    fn choose<R>(mut self, rng: &mut R) -> Option<Self::Item>
    where R: Rng + ?Sized {
        let (mut lower, mut upper) = self.size_hint();
        let mut consumed = 0;
        let mut result = None;

        // Handling for this condition outside the loop allows the optimizer to eliminate the loop
        // when the Iterator is an ExactSizeIterator. This has a large performance impact on e.g.
        // seq_iter_choose_from_1000.
        if upper == Some(lower) {
            return if lower == 0 {
                None
            } else {
                self.nth(gen_index(rng, lower))
            };
        }

        // Continue until the iterator is exhausted
        loop {
            if lower > 1 {
                let ix = gen_index(rng, lower + consumed);
                let skip = if ix < lower {
                    result = self.nth(ix);
                    lower - (ix + 1)
                } else {
                    lower
                };
                if upper == Some(lower) {
                    return result;
                }
                consumed += lower;
                if skip > 0 {
                    self.nth(skip - 1);
                }
            } else {
                let elem = self.next();
                if elem.is_none() {
                    return result;
                }
                consumed += 1;
                if gen_index(rng, consumed) == 0 {
                    result = elem;
                }
            }

            let hint = self.size_hint();
            lower = hint.0;
            upper = hint.1;
        }
    }

    /// Choose one element at random from the iterator.
    ///
    /// Returns `None` if and only if the iterator is empty.
    ///
    /// This method is very similar to [`choose`] except that the result
    /// only depends on the length of the iterator and the values produced by
    /// `rng`. Notably for any iterator of a given length this will make the
    /// same requests to `rng` and if the same sequence of values are produced
    /// the same index will be selected from `self`. This may be useful if you
    /// need consistent results no matter what type of iterator you are working
    /// with. If you do not need this stability prefer [`choose`].
    ///
    /// Note that this method still uses [`Iterator::size_hint`] to skip
    /// constructing elements where possible, however the selection and `rng`
    /// calls are the same in the face of this optimization. If you want to
    /// force every element to be created regardless call `.inspect(|e| ())`.
    ///
    /// [`choose`]: IteratorRandom::choose
    fn choose_stable<R>(mut self, rng: &mut R) -> Option<Self::Item>
    where R: Rng + ?Sized {
        let mut consumed = 0;
        let mut result = None;

        loop {
            // Currently the only way to skip elements is `nth()`. So we need to
            // store what index to access next here.
            // This should be replaced by `advance_by()` once it is stable:
            // https://github.com/rust-lang/rust/issues/77404
            let mut next = 0;

            let (lower, _) = self.size_hint();
            if lower >= 2 {
                let highest_selected = (0..lower)
                    .filter(|ix| gen_index(rng, consumed+ix+1) == 0)
                    .last();

                consumed += lower;
                next = lower;

                if let Some(ix) = highest_selected {
                    result = self.nth(ix);
                    next -= ix + 1;
                    debug_assert!(result.is_some(), "iterator shorter than size_hint().0");
                }
            }

            let elem = self.nth(next);
            if elem.is_none() {
                return result
            }

            if gen_index(rng, consumed+1) == 0 {
                result = elem;
            }
            consumed += 1;
        }
    }

    /// Collects values at random from the iterator into a supplied buffer
    /// until that buffer is filled.
    ///
    /// Although the elements are selected randomly, the order of elements in
    /// the buffer is neither stable nor fully random. If random ordering is
    /// desired, shuffle the result.
    ///
    /// Returns the number of elements added to the buffer. This equals the length
    /// of the buffer unless the iterator contains insufficient elements, in which
    /// case this equals the number of elements available.
    ///
    /// Complexity is `O(n)` where `n` is the length of the iterator.
    /// For slices, prefer [`SliceRandom::choose_multiple`].
    fn choose_multiple_fill<R>(mut self, rng: &mut R, buf: &mut [Self::Item]) -> usize
    where R: Rng + ?Sized {
        let amount = buf.len();
        let mut len = 0;
        while len < amount {
            if let Some(elem) = self.next() {
                buf[len] = elem;
                len += 1;
            } else {
                // Iterator exhausted; stop early
                return len;
            }
        }

        // Continue, since the iterator was not exhausted
        for (i, elem) in self.enumerate() {
            let k = gen_index(rng, i + 1 + amount);
            if let Some(slot) = buf.get_mut(k) {
                *slot = elem;
            }
        }
        len
    }

    /// Collects `amount` values at random from the iterator into a vector.
    ///
    /// This is equivalent to `choose_multiple_fill` except for the result type.
    ///
    /// Although the elements are selected randomly, the order of elements in
    /// the buffer is neither stable nor fully random. If random ordering is
    /// desired, shuffle the result.
    ///
    /// The length of the returned vector equals `amount` unless the iterator
    /// contains insufficient elements, in which case it equals the number of
    /// elements available.
    ///
    /// Complexity is `O(n)` where `n` is the length of the iterator.
    /// For slices, prefer [`SliceRandom::choose_multiple`].
    #[cfg(feature = "alloc")]
    #[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
    fn choose_multiple<R>(mut self, rng: &mut R, amount: usize) -> Vec<Self::Item>
    where R: Rng + ?Sized {
        let mut reservoir = Vec::with_capacity(amount);
        reservoir.extend(self.by_ref().take(amount));

        // Continue unless the iterator was exhausted
        //
        // note: this prevents iterators that "restart" from causing problems.
        // If the iterator stops once, then so do we.
        if reservoir.len() == amount {
            for (i, elem) in self.enumerate() {
                let k = gen_index(rng, i + 1 + amount);
                if let Some(slot) = reservoir.get_mut(k) {
                    *slot = elem;
                }
            }
        } else {
            // Don't hang onto extra memory. There is a corner case where
            // `amount` was much less than `self.len()`.
            reservoir.shrink_to_fit();
        }
        reservoir
    }
}


impl<T> SliceRandom for [T] {
    type Item = T;

    fn choose<R>(&self, rng: &mut R) -> Option<&Self::Item>
    where R: Rng + ?Sized {
        if self.is_empty() {
            None
        } else {
            Some(&self[gen_index(rng, self.len())])
        }
    }

    fn choose_mut<R>(&mut self, rng: &mut R) -> Option<&mut Self::Item>
    where R: Rng + ?Sized {
        if self.is_empty() {
            None
        } else {
            let len = self.len();
            Some(&mut self[gen_index(rng, len)])
        }
    }

    #[cfg(feature = "alloc")]
    fn choose_multiple<R>(&self, rng: &mut R, amount: usize) -> SliceChooseIter<'_, Self, Self::Item>
    where R: Rng + ?Sized {
        let amount = ::core::cmp::min(amount, self.len());
        SliceChooseIter {
            slice: self,
            _phantom: Default::default(),
            indices: index::sample(rng, self.len(), amount).into_iter(),
        }
    }

    #[cfg(feature = "alloc")]
    fn choose_weighted<R, F, B, X>(
        &self, rng: &mut R, weight: F,
    ) -> Result<&Self::Item, WeightedError>
    where
        R: Rng + ?Sized,
        F: Fn(&Self::Item) -> B,
        B: SampleBorrow<X>,
        X: SampleUniform
            + for<'a> ::core::ops::AddAssign<&'a X>
            + ::core::cmp::PartialOrd<X>
            + Clone
            + Default,
    {
        use crate::distributions::{Distribution, WeightedIndex};
        let distr = WeightedIndex::new(self.iter().map(weight))?;
        Ok(&self[distr.sample(rng)])
    }

    #[cfg(feature = "alloc")]
    fn choose_weighted_mut<R, F, B, X>(
        &mut self, rng: &mut R, weight: F,
    ) -> Result<&mut Self::Item, WeightedError>
    where
        R: Rng + ?Sized,
        F: Fn(&Self::Item) -> B,
        B: SampleBorrow<X>,
        X: SampleUniform
            + for<'a> ::core::ops::AddAssign<&'a X>
            + ::core::cmp::PartialOrd<X>
            + Clone
            + Default,
    {
        use crate::distributions::{Distribution, WeightedIndex};
        let distr = WeightedIndex::new(self.iter().map(weight))?;
        Ok(&mut self[distr.sample(rng)])
    }

    #[cfg(feature = "std")]
    fn choose_multiple_weighted<R, F, X>(
        &self, rng: &mut R, amount: usize, weight: F,
    ) -> Result<SliceChooseIter<'_, Self, Self::Item>, WeightedError>
    where
        R: Rng + ?Sized,
        F: Fn(&Self::Item) -> X,
        X: Into<f64>,
    {
        let amount = ::core::cmp::min(amount, self.len());
        Ok(SliceChooseIter {
            slice: self,
            _phantom: Default::default(),
            indices: index::sample_weighted(
                rng,
                self.len(),
                |idx| weight(&self[idx]).into(),
                amount,
            )?
            .into_iter(),
        })
    }

    fn shuffle<R>(&mut self, rng: &mut R)
    where R: Rng + ?Sized {
        for i in (1..self.len()).rev() {
            // invariant: elements with index > i have been locked in place.
            self.swap(i, gen_index(rng, i + 1));
        }
    }

    fn partial_shuffle<R>(
        &mut self, rng: &mut R, amount: usize,
    ) -> (&mut [Self::Item], &mut [Self::Item])
    where R: Rng + ?Sized {
        // This applies Durstenfeld's algorithm for the
        // [Fisher–Yates shuffle](https://en.wikipedia.org/wiki/Fisher%E2%80%93Yates_shuffle#The_modern_algorithm)
        // for an unbiased permutation, but exits early after choosing `amount`
        // elements.

        let len = self.len();
        let end = if amount >= len { 0 } else { len - amount };

        for i in (end..len).rev() {
            // invariant: elements with index > i have been locked in place.
            self.swap(i, gen_index(rng, i + 1));
        }
        let r = self.split_at_mut(end);
        (r.1, r.0)
    }
}

impl<I> IteratorRandom for I where I: Iterator + Sized {}


/// An iterator over multiple slice elements.
///
/// This struct is created by
/// [`SliceRandom::choose_multiple`](trait.SliceRandom.html#tymethod.choose_multiple).
#[cfg(feature = "alloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
#[derive(Debug)]
pub struct SliceChooseIter<'a, S: ?Sized + 'a, T: 'a> {
    slice: &'a S,
    _phantom: ::core::marker::PhantomData<T>,
    indices: index::IndexVecIntoIter,
}

#[cfg(feature = "alloc")]
impl<'a, S: Index<usize, Output = T> + ?Sized + 'a, T: 'a> Iterator for SliceChooseIter<'a, S, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        // TODO: investigate using SliceIndex::get_unchecked when stable
        self.indices.next().map(|i| &self.slice[i as usize])
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.indices.len(), Some(self.indices.len()))
    }
}

#[cfg(feature = "alloc")]
impl<'a, S: Index<usize, Output = T> + ?Sized + 'a, T: 'a> ExactSizeIterator
    for SliceChooseIter<'a, S, T>
{
    fn len(&self) -> usize {
        self.indices.len()
    }
}


// Sample a number uniformly between 0 and `ubound`. Uses 32-bit sampling where
// possible, primarily in order to produce the same output on 32-bit and 64-bit
// platforms.
#[inline]
fn gen_index<R: Rng + ?Sized>(rng: &mut R, ubound: usize) -> usize {
    if ubound <= (core::u32::MAX as usize) {
        rng.gen_range(0..ubound as u32) as usize
    } else {
        rng.gen_range(0..ubound)
    }
}

