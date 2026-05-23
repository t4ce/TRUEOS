/*
 * Copyright (c) 2023.
 *
 * This software is free software;
 *
 * You can redistribute it or modify it under terms of the MIT, Apache License or Zlib license
 */

//! Routines for IDCT
//!
//! Essentially we provide 2 routines for IDCT, a scalar implementation and a not super optimized
//! AVX2 one, i'll talk about them here.
//!
//! There are 2 reasons why we have the avx one
//! 1. No one compiles with -C target-features=avx2 hence binaries won't probably take advantage(even
//! if it exists).
//! 2. AVX employs zero short circuit in a way the scalar code cannot employ it.
//!     - AVX does this by checking for MCU's whose 63 AC coefficients are zero and if true, it writes
//!        values directly, if false, it goes the long way of calculating.
//!     -   Although this can be trivially implemented in the scalar version, it  generates code
//!         I'm not happy width(scalar version that basically loops and that is too many branches for me)
//!         The avx one does a better job of using bitwise or's with (`_mm256_or_si256`) which is magnitudes of faster
//!         than anything I could come up with
//!
//! The AVX code also has some cool transpose_u16 instructions which look so complicated to be cool
//! (spoiler alert, i barely understand how it works, that's why I credited the owner).
//!
#![allow(
    clippy::excessive_precision,
    clippy::unreadable_literal,
    clippy::module_name_repetitions,
    unused_parens,
    clippy::wildcard_imports
)]

use zune_core::log::debug;
use zune_core::options::DecoderOptions;

use crate::decoder::IDCTPtr;
use crate::idct::scalar::{idct_int, idct_int_1x1};

#[cfg(feature = "x86")]
pub mod avx2;
#[cfg(feature = "neon")]
pub mod neon;

pub mod scalar;

/// Choose an appropriate IDCT function
#[allow(unused_variables)]
pub fn choose_idct_func(options: &DecoderOptions) -> IDCTPtr {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    #[cfg(feature = "x86")]
    {
        if options.use_avx2() {
            debug!("Using vector integer IDCT");
            return |a: &mut [i32; 64], b: &mut [i16], c: usize| {
                // SAFETY: `options.use_avx2()` only returns true if avx2 is supported.
                unsafe { avx2::idct_avx2(a,b,c) }
            };
        }
    }
    #[cfg(target_arch = "aarch64")]
    #[cfg(feature = "neon")]
    {
        if options.use_neon() {
            debug!("Using vector integer IDCT");
            return |a: &mut [i32; 64], b: &mut [i16], c: usize| {
                // SAFETY: `options.use_neon()` only returns true if neon is supported.
                unsafe { neon::idct_neon(a,b,c) }
            };
        }
    }
    debug!("Using scalar integer IDCT");
    // use generic one
    return idct_int;
}

/// Choose a function to implement 4x4 IDCT.
///
/// These functions get the same input but have an extra contract: Only the first 4x4 block of
/// coefficients are non-zero. All other entries are zeroed.
///
/// **The callee must uphold that contract on return**
pub fn choose_idct_4x4_func(_options: &DecoderOptions) -> IDCTPtr {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    #[cfg(feature = "x86")]
    {
        if _options.use_avx2() {
            debug!("Using vector integer IDCT");
            return |a: &mut [i32; 64], b: &mut [i16], c: usize| {
                // SAFETY: `options.use_avx2()` only returns true if avx2 is supported.
                unsafe { avx2::idct_avx2_4x4(a,b,c) }
            };
        }
    }

    scalar::idct4x4
}

pub fn choose_idct_1x1_func(_: &DecoderOptions) -> IDCTPtr {
    // These are simple stores, no alternative implementation for now
    idct_int_1x1
}
