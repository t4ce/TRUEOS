//! VP9 parser errors.

use core::{array, error::Error, num};

/// Errors that can occur when parsing VP9 frames.
#[derive(Debug)]
pub enum Vp9ParserError {
    /// A `bitreader::BitReaderError`.
    BitReaderError(bitreader::BitReaderError),
    #[cfg(feature = "std")]
    /// A `std::io::Error`.
    IoError(std::io::Error),
    /// A `TryFromSliceError`.
    TryFromSliceError(array::TryFromSliceError),
    /// A `TryFromIntError`.
    TryFromIntError(num::TryFromIntError),
    /// Invalid frame marker.
    InvalidFrameMarker,
    /// Invalid padding.
    InvalidPadding,
    /// Invalid sync byte.
    InvalidSyncByte,
    /// Invalid reference frame index.
    InvalidRefFrameIndex,
    /// Invalid metadata.
    InvalidMetadata,
    /// Invalid frame_size byte size.
    InvalidFrameSizeByteSize(usize),
}

impl core::fmt::Display for Vp9ParserError {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            Vp9ParserError::BitReaderError(err) => {
                write!(f, "{:?}", err)
            }
            #[cfg(feature = "std")]
            Vp9ParserError::IoError(err) => {
                write!(f, "{:?}", err.source())
            }
            Vp9ParserError::TryFromSliceError(err) => {
                write!(f, "{:?}", err.source())
            }
            Vp9ParserError::TryFromIntError(err) => {
                write!(f, "{:?}", err.source())
            }
            Vp9ParserError::InvalidFrameMarker => {
                write!(f, "invalid frame marker")
            }
            Vp9ParserError::InvalidPadding => {
                write!(f, "invalid padding")
            }
            Vp9ParserError::InvalidSyncByte => {
                write!(f, "invalid sync byte")
            }
            Vp9ParserError::InvalidRefFrameIndex => {
                write!(f, "invalid reference frame index")
            }
            Vp9ParserError::InvalidMetadata => {
                write!(f, "invalid metadata")
            }
            Vp9ParserError::InvalidFrameSizeByteSize(size) => {
                write!(f, "invalid frame_size byte size: {}", size)
            }
        }
    }
}

#[cfg(feature = "std")]
impl From<std::io::Error> for Vp9ParserError {
    fn from(err: std::io::Error) -> Vp9ParserError {
        Vp9ParserError::IoError(err)
    }
}

impl From<array::TryFromSliceError> for Vp9ParserError {
    fn from(err: array::TryFromSliceError) -> Vp9ParserError {
        Vp9ParserError::TryFromSliceError(err)
    }
}

impl From<num::TryFromIntError> for Vp9ParserError {
    fn from(err: num::TryFromIntError) -> Vp9ParserError {
        Vp9ParserError::TryFromIntError(err)
    }
}

impl From<bitreader::BitReaderError> for Vp9ParserError {
    fn from(err: bitreader::BitReaderError) -> Vp9ParserError {
        Vp9ParserError::BitReaderError(err)
    }
}

impl core::error::Error for Vp9ParserError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match *self {
            #[cfg(feature = "std")]
            Vp9ParserError::IoError(ref e) => Some(e),
            Vp9ParserError::TryFromSliceError(ref e) => Some(e),
            Vp9ParserError::TryFromIntError(ref e) => Some(e),
            Vp9ParserError::BitReaderError(_) => None,
            _ => None,
        }
    }
}
