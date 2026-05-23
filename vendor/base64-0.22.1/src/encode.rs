#[cfg(any(feature = "alloc", test))]
use alloc::string::String;
use ::core::fmt;
#[cfg(any(feature = "std", test))]
use core::error;

#[cfg(any(feature = "alloc", test))]
use crate::engine::general_purpose::STANDARD;
use crate::engine::{Config, Engine};
use crate::PAD_BYTE;

/// Encode arbitrary octets as base64 using the [`STANDARD` engine](STANDARD).
///
/// See [Engine::encode].
#[allow(unused)]
#[deprecated(since = "0.21.0", note = "Use Engine::encode")]
#[cfg(any(feature = "alloc", test))]
pub fn encode<T: AsRef<[u8]>>(input: T) -> String {
    STANDARD.encode(input)
}

///Encode arbitrary octets as base64 using the provided `Engine` into a new `String`.
///
/// See [Engine::encode].
#[allow(unused)]
#[deprecated(since = "0.21.0", note = "Use Engine::encode")]
#[cfg(any(feature = "alloc", test))]
pub fn encode_engine<E: Engine, T: AsRef<[u8]>>(input: T, engine: &E) -> String {
    engine.encode(input)
}

///Encode arbitrary octets as base64 into a supplied `String`.
///
/// See [Engine::encode_string].
#[allow(unused)]
#[deprecated(since = "0.21.0", note = "Use Engine::encode_string")]
#[cfg(any(feature = "alloc", test))]
pub fn encode_engine_string<E: Engine, T: AsRef<[u8]>>(
    input: T,
    output_buf: &mut String,
    engine: &E,
) {
    engine.encode_string(input, output_buf)
}

/// Encode arbitrary octets as base64 into a supplied slice.
///
/// See [Engine::encode_slice].
#[allow(unused)]
#[deprecated(since = "0.21.0", note = "Use Engine::encode_slice")]
pub fn encode_engine_slice<E: Engine, T: AsRef<[u8]>>(
    input: T,
    output_buf: &mut [u8],
    engine: &E,
) -> Result<usize, EncodeSliceError> {
    engine.encode_slice(input, output_buf)
}

/// B64-encode and pad (if configured).
///
/// This helper exists to avoid recalculating encoded_size, which is relatively expensive on short
/// inputs.
///
/// `encoded_size` is the encoded size calculated for `input`.
///
/// `output` must be of size `encoded_size`.
///
/// All bytes in `output` will be written to since it is exactly the size of the output.
pub(crate) fn encode_with_padding<E: Engine + ?Sized>(
    input: &[u8],
    output: &mut [u8],
    engine: &E,
    expected_encoded_size: usize,
) {
    debug_assert_eq!(expected_encoded_size, output.len());

    let b64_bytes_written = engine.internal_encode(input, output);

    let padding_bytes = if engine.config().encode_padding() {
        add_padding(b64_bytes_written, &mut output[b64_bytes_written..])
    } else {
        0
    };

    let encoded_bytes = b64_bytes_written
        .checked_add(padding_bytes)
        .expect("usize overflow when calculating b64 length");

    debug_assert_eq!(expected_encoded_size, encoded_bytes);
}

/// Calculate the base64 encoded length for a given input length, optionally including any
/// appropriate padding bytes.
///
/// Returns `None` if the encoded length can't be represented in `usize`. This will happen for
/// input lengths in approximately the top quarter of the range of `usize`.
pub const fn encoded_len(bytes_len: usize, padding: bool) -> Option<usize> {
    let rem = bytes_len % 3;

    let complete_input_chunks = bytes_len / 3;
    // `?` is disallowed in const, and `let Some(_) = _ else` requires 1.65.0, whereas this
    // messier syntax works on 1.48
    let complete_chunk_output =
        if let Some(complete_chunk_output) = complete_input_chunks.checked_mul(4) {
            complete_chunk_output
        } else {
            return None;
        };

    if rem > 0 {
        if padding {
            complete_chunk_output.checked_add(4)
        } else {
            let encoded_rem = match rem {
                1 => 2,
                // only other possible remainder is 2
                // can't use a separate _ => unreachable!() in const fns in ancient rust versions
                _ => 3,
            };
            complete_chunk_output.checked_add(encoded_rem)
        }
    } else {
        Some(complete_chunk_output)
    }
}

/// Write padding characters.
/// `unpadded_output_len` is the size of the unpadded but base64 encoded data.
/// `output` is the slice where padding should be written, of length at least 2.
///
/// Returns the number of padding bytes written.
pub(crate) fn add_padding(unpadded_output_len: usize, output: &mut [u8]) -> usize {
    let pad_bytes = (4 - (unpadded_output_len % 4)) % 4;
    // for just a couple bytes, this has better performance than using
    // .fill(), or iterating over mutable refs, which call memset()
    #[allow(clippy::needless_range_loop)]
    for i in 0..pad_bytes {
        output[i] = PAD_BYTE;
    }

    pad_bytes
}

/// Errors that can occur while encoding into a slice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EncodeSliceError {
    /// The provided slice is too small.
    OutputSliceTooSmall,
}

impl fmt::Display for EncodeSliceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutputSliceTooSmall => write!(f, "Output slice too small"),
        }
    }
}

