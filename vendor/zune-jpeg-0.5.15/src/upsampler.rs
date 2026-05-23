/*
 * Copyright (c) 2023.
 *
 * This software is free software;
 *
 * You can redistribute it or modify it under terms of the MIT, Apache License or Zlib license
 */

//! Up-sampling routines
//!
//! The main upsampling method is a bi-linear interpolation or a "triangle
//! filter " or libjpeg turbo `fancy_upsampling` which is a good compromise
//! between speed and visual quality
//!
//! # The filter
//! Each output pixel is made from `(3*A+B)/4` where A is the original
//! pixel closer to the output and B is the one further.
//!
//! ```text
//!+---+---+
//! | A | B |
//! +---+---+
//! +-+-+-+-+
//! | |P| | |
//! +-+-+-+-+
//! ```
//!
//! # Horizontal Bi-linear filter
//! ```text
//! |---+-----------+---+
//! |   |           |   |
//! | A | |p1 | p2| | B |
//! |   |           |   |
//! |---+-----------+---+
//!
//! ```
//! For a horizontal bi-linear it's trivial to implement,
//!
//! `A` becomes the input closest to the output.
//!
//! `B` varies depending on output.
//!  - For odd positions, input is the `next` pixel after A
//!  - For even positions, input is the `previous` value before A.
//!
//! We iterate in a classic 1-D sliding window with a window of 3.
//! For our sliding window approach, `A` is the 1st and `B` is either the 0th term or 2nd term
//! depending on position we are writing.(see scalar code).
//!
//! For vector code see module sse for explanation.
//!
//! # Vertical bi-linear.
//! Vertical up-sampling is a bit trickier.
//!
//! ```text
//! +----+----+
//! | A1 | A2 |
//! +----+----+
//! +----+----+
//! | p1 | p2 |
//! +----+-+--+
//! +----+-+--+
//! | p3 | p4 |
//! +----+-+--+
//! +----+----+
//! | B1 | B2 |
//! +----+----+
//! ```
//!
//! For `p1`
//! - `A1` is given a weight of `3` and `B1` is given a weight of 1.
//!
//! For `p3`
//! - `B1` is given a weight of `3` and `A1` is given a weight of 1
//!
//! # Horizontal vertical downsampling/chroma quartering.
//!
//! Carry out a vertical filter in the first pass, then a horizontal filter in the second pass.
#![allow(unreachable_code)]
use zune_core::options::DecoderOptions;

use crate::components::UpSampler;

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[cfg(feature = "x86")]
mod avx2;
#[cfg(target_arch = "aarch64")]
#[cfg(feature = "neon")]
mod neon;
#[cfg(feature = "portable_simd")]
mod portable_simd;
mod scalar;

// choose the best possible implementation for this platform
#[allow(unused_variables)]
pub fn choose_horizontal_samp_function(options: &DecoderOptions) -> UpSampler {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    #[cfg(feature = "x86")]
    if options.use_avx2() {
        return |a: &[i16], b: &[i16], c: &[i16], d: &mut [i16], e: &mut [i16]| {
            // SAFETY: `options.use_avx2()` only returns true if avx2 is supported.
            unsafe { avx2::upsample_horizontal_avx2(a, b, c, d, e) }
        };
    }
    #[cfg(target_arch = "aarch64")]
    #[cfg(feature = "neon")]
    if options.use_neon() {
        return |a: &[i16], b: &[i16], c: &[i16], d: &mut [i16], e: &mut [i16]| {
            // SAFETY: `options.use_neon()` only returns true if neon is supported.
            unsafe { neon::upsample_horizontal_neon(a, b, c, d, e) }
        };
    }
    #[cfg(feature = "portable_simd")]
    return portable_simd::upsample_horizontal_simd;
    return scalar::upsample_horizontal;
}

#[allow(unused_variables)]
pub fn choose_hv_samp_function(options: &DecoderOptions) -> UpSampler {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    #[cfg(feature = "x86")]
    if options.use_avx2() {
        return |a: &[i16], b: &[i16], c: &[i16], d: &mut [i16], e: &mut [i16]| {
            // SAFETY: `options.use_avx2()` only returns true if avx2 is supported.
            unsafe { avx2::upsample_hv_avx2(a, b, c, d, e) }
        };
    }
    #[cfg(target_arch = "aarch64")]
    #[cfg(feature = "neon")]
    if options.use_neon() {
        return |a: &[i16], b: &[i16], c: &[i16], d: &mut [i16], e: &mut [i16]| {
            // SAFETY: `options.use_neon()` only returns true if neon is supported.
            unsafe { neon::upsample_hv_neon(a, b, c, d, e) }
        };
    }
    #[cfg(feature = "portable_simd")]
    return portable_simd::upsample_hv_simd;
    return scalar::upsample_hv;
}

#[allow(unused_variables)]
pub fn choose_v_samp_function(options: &DecoderOptions) -> UpSampler {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    #[cfg(feature = "x86")]
    if options.use_avx2() {
        return |a: &[i16], b: &[i16], c: &[i16], d: &mut [i16], e: &mut [i16]| {
            // SAFETY: `options.use_avx2()` only returns true if avx2 is supported.
            unsafe { avx2::upsample_vertical_avx2(a, b, c, d, e) }
        };
    }
    #[cfg(target_arch = "aarch64")]
    #[cfg(feature = "neon")]
    if options.use_neon() {
        return |a: &[i16], b: &[i16], c: &[i16], d: &mut [i16], e: &mut [i16]| {
            // SAFETY: `options.use_neon()` only returns true if neon is supported.
            unsafe { neon::upsample_vertical_neon(a, b, c, d, e) }
        };
    }
    #[cfg(feature = "portable_simd")]
    return portable_simd::upsample_vertical_simd;
    return scalar::upsample_vertical;
}

/// Upsample nothing

pub fn upsample_no_op(
    _input: &[i16],
    _in_ref: &[i16],
    _in_near: &[i16],
    _scratch_space: &mut [i16],
    _output: &mut [i16],
) {
}

pub fn generic_sampler() -> UpSampler {
    scalar::upsample_generic
}
