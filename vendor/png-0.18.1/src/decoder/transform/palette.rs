//! Helpers for taking a slice of indices (indices into `PLTE` and/or `trNS`
//! entries) and transforming this into RGB or RGBA output.
//!
//! # Memoization
//!
//! To achieve higher throughput, `create_rgba_palette` combines entries from
//! `PLTE` and `trNS` chunks into a single lookup table.  This is based on the
//! ideas explored in <https://crbug.com/706134>.
//!
//! Memoization is a trade-off:
//! * On one hand, memoization requires spending X ns before starting to call
//!   `expand_paletted_...` functions.
//! * On the other hand, memoization improves the throughput of the
//!   `expand_paletted_...` functions - they take Y ns less to process each byte
//!
//! Based on X and Y, we can try to calculate the breakeven point.  It seems
//! that memoization is a net benefit for images bigger than around 13x13 pixels.

use alloc::boxed::Box;

use super::{unpack_bits, TransformFn};
use crate::{BitDepth, Info};

pub fn create_expansion_into_rgb8(info: &Info) -> TransformFn {
    let rgba_palette = create_rgba_palette(info);

    if info.bit_depth == BitDepth::Eight {
        Box::new(move |input, output, _info| expand_8bit_into_rgb8(input, output, &rgba_palette))
    } else {
        Box::new(move |input, output, info| expand_into_rgb8(input, output, info, &rgba_palette))
    }
}

pub fn create_expansion_into_rgba8(info: &Info) -> TransformFn {
    let rgba_palette = create_rgba_palette(info);
    Box::new(move |input, output, info| {
        expand_paletted_into_rgba8(input, output, info, &rgba_palette)
    })
}

fn create_rgba_palette(info: &Info) -> [[u8; 4]; 256] {
    let palette = info.palette.as_deref().expect("Caller should verify");
    let trns = info.trns.as_deref().unwrap_or(&[]);

    // > The tRNS chunk shall not contain more alpha values than there are palette
    // entries, but a tRNS chunk may contain fewer values than there are palette
    // entries. In this case, the alpha value for all remaining palette entries is
    // assumed to be 255.
    //
    // It seems, accepted reading is to fully *ignore* an invalid tRNS as if it were
    // completely empty / all pixels are non-transparent.
    let trns = if trns.len() <= palette.len() / 3 {
        trns
    } else {
        &[]
    };

    // Default to black, opaque entries.
    let mut rgba_palette = [[0, 0, 0, 0xFF]; 256];

    // Copy `palette` (RGB) entries into `rgba_palette`.  This may clobber alpha
    // values in `rgba_palette` - we need to fix this later.
    {
        let mut palette_iter = palette;
        let mut rgba_iter = &mut rgba_palette[..];
        while palette_iter.len() >= 4 {
            // Copying 4 bytes at a time is more efficient than copying 3.
            // OTOH, this clobbers the alpha value in `rgba_iter[0][3]` - we
            // need to fix this later.
            rgba_iter[0].copy_from_slice(&palette_iter[0..4]);

            palette_iter = &palette_iter[3..];
            rgba_iter = &mut rgba_iter[1..];
        }
        if !palette_iter.is_empty() {
            rgba_iter[0][0..3].copy_from_slice(&palette_iter[0..3]);
        }
    }

    // Copy `trns` (alpha) entries into `rgba_palette`.  `trns.len()` may be
    // smaller than `palette.len()` and therefore this is not sufficient to fix
    // all the clobbered alpha values.
    for (alpha, rgba) in trns.iter().copied().zip(rgba_palette.iter_mut()) {
        rgba[3] = alpha;
    }

    // Unclobber the remaining alpha values.
    for rgba in rgba_palette[trns.len()..(palette.len() / 3)].iter_mut() {
        rgba[3] = 0xFF;
    }

    rgba_palette
}

fn expand_8bit_into_rgb8(mut input: &[u8], mut output: &mut [u8], rgba_palette: &[[u8; 4]; 256]) {
    while output.len() >= 4 {
        // Copying 4 bytes at a time is more efficient than 3.
        let rgba = &rgba_palette[input[0] as usize];
        output[0..4].copy_from_slice(rgba);

        input = &input[1..];
        output = &mut output[3..];
    }
    if !output.is_empty() {
        let rgba = &rgba_palette[input[0] as usize];
        output[0..3].copy_from_slice(&rgba[0..3]);
    }
}

fn expand_into_rgb8(row: &[u8], buffer: &mut [u8], info: &Info, rgba_palette: &[[u8; 4]; 256]) {
    unpack_bits(row, buffer, 3, info.bit_depth as u8, |i, chunk| {
        let rgba = &rgba_palette[i as usize];
        chunk[0] = rgba[0];
        chunk[1] = rgba[1];
        chunk[2] = rgba[2];
    })
}

fn expand_paletted_into_rgba8(
    row: &[u8],
    buffer: &mut [u8],
    info: &Info,
    rgba_palette: &[[u8; 4]; 256],
) {
    unpack_bits(row, buffer, 4, info.bit_depth as u8, |i, chunk| {
        chunk.copy_from_slice(&rgba_palette[i as usize]);
    });
}
