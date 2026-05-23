use crate::engine::{general_purpose::STANDARD, DecodeEstimate, Engine};
#[cfg(any(feature = "alloc", test))]
use alloc::vec::Vec;
use core::fmt;
#[cfg(any(feature = "std", test))]
use core::error;

/// Errors that can occur while decoding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// An invalid byte was found in the input. The offset and offending byte are provided.
    ///
    /// Padding characters (`=`) interspersed in the encoded form are invalid, as they may only
    /// be present as the last 0-2 bytes of input.
    ///
    /// This error may also indicate that extraneous trailing input bytes are present, causing
    /// otherwise valid padding to no longer be the last bytes of input.
    InvalidByte(usize, u8),
    /// The length of the input, as measured in valid base64 symbols, is invalid.
    /// There must be 2-4 symbols in the last input quad.
    InvalidLength(usize),
    /// The last non-padding input symbol's encoded 6 bits have nonzero bits that will be discarded.
    /// This is indicative of corrupted or truncated Base64.
    /// Unlike [DecodeError::InvalidByte], which reports symbols that aren't in the alphabet,
    /// this error is for symbols that are in the alphabet but represent nonsensical encodings.
    InvalidLastSymbol(usize, u8),
    /// The nature of the padding was not as configured: absent or incorrect when it must be
    /// canonical, or present when it must be absent, etc.
    InvalidPadding,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Self::InvalidByte(index, byte) => {
                write!(f, "Invalid symbol {}, offset {}.", byte, index)
            }
            Self::InvalidLength(len) => write!(f, "Invalid input length: {}", len),
            Self::InvalidLastSymbol(index, byte) => {
                write!(f, "Invalid last symbol {}, offset {}.", byte, index)
            }
            Self::InvalidPadding => write!(f, "Invalid padding"),
        }
    }
}

#[cfg(any(feature = "std", test))]
impl error::Error for DecodeError {}

/// Errors that can occur while decoding into a slice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeSliceError {
    /// A [DecodeError] occurred
    DecodeError(DecodeError),
    /// The provided slice is too small.
    OutputSliceTooSmall,
}

impl fmt::Display for DecodeSliceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DecodeError(e) => write!(f, "DecodeError: {}", e),
            Self::OutputSliceTooSmall => write!(f, "Output slice too small"),
        }
    }
}

#[cfg(any(feature = "std", test))]
impl error::Error for DecodeSliceError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            DecodeSliceError::DecodeError(e) => Some(e),
            DecodeSliceError::OutputSliceTooSmall => None,
        }
    }
}

impl From<DecodeError> for DecodeSliceError {
    fn from(e: DecodeError) -> Self {
        DecodeSliceError::DecodeError(e)
    }
}

/// Decode base64 using the [`STANDARD` engine](STANDARD).
///
/// See [Engine::decode].
#[deprecated(since = "0.21.0", note = "Use Engine::decode")]
#[cfg(any(feature = "alloc", test))]
pub fn decode<T: AsRef<[u8]>>(input: T) -> Result<Vec<u8>, DecodeError> {
    STANDARD.decode(input)
}

/// Decode from string reference as octets using the specified [Engine].
///
/// See [Engine::decode].
///Returns a `Result` containing a `Vec<u8>`.
#[deprecated(since = "0.21.0", note = "Use Engine::decode")]
#[cfg(any(feature = "alloc", test))]
pub fn decode_engine<E: Engine, T: AsRef<[u8]>>(
    input: T,
    engine: &E,
) -> Result<Vec<u8>, DecodeError> {
    engine.decode(input)
}

/// Decode from string reference as octets.
///
/// See [Engine::decode_vec].
#[cfg(any(feature = "alloc", test))]
#[deprecated(since = "0.21.0", note = "Use Engine::decode_vec")]
pub fn decode_engine_vec<E: Engine, T: AsRef<[u8]>>(
    input: T,
    buffer: &mut Vec<u8>,
    engine: &E,
) -> Result<(), DecodeError> {
    engine.decode_vec(input, buffer)
}

/// Decode the input into the provided output slice.
///
/// See [Engine::decode_slice].
#[deprecated(since = "0.21.0", note = "Use Engine::decode_slice")]
pub fn decode_engine_slice<E: Engine, T: AsRef<[u8]>>(
    input: T,
    output: &mut [u8],
    engine: &E,
) -> Result<usize, DecodeSliceError> {
    engine.decode_slice(input, output)
}

/// Returns a conservative estimate of the decoded size of `encoded_len` base64 symbols (rounded up
/// to the next group of 3 decoded bytes).
///
/// The resulting length will be a safe choice for the size of a decode buffer, but may have up to
/// 2 trailing bytes that won't end up being needed.
///
/// # Examples
///
/// ```
/// use base64::decoded_len_estimate;
///
/// assert_eq!(3, decoded_len_estimate(1));
/// assert_eq!(3, decoded_len_estimate(2));
/// assert_eq!(3, decoded_len_estimate(3));
/// assert_eq!(3, decoded_len_estimate(4));
/// // start of the next quad of encoded symbols
/// assert_eq!(6, decoded_len_estimate(5));
/// ```
pub fn decoded_len_estimate(encoded_len: usize) -> usize {
    STANDARD
        .internal_decoded_len_estimate(encoded_len)
        .decoded_len_estimate()
}
