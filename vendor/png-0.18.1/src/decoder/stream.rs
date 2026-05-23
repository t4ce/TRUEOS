use alloc::vec::Vec;
use core::convert::TryInto;
use core::error;
use core::fmt;
use crate::io;
use std::{borrow::Cow, cmp::min};

use crc32fast::Hasher as Crc32;

use super::zlib::UnfilterBuf;
use super::zlib::ZlibStream;
use crate::chunk::is_critical;
use crate::chunk::{self, ChunkType, IDAT, IEND, IHDR};
use crate::common::{
    AnimationControl, BitDepth, BlendOp, ColorType, ContentLightLevelInfo, DisposeOp, FrameControl,
    Info, MasteringDisplayColorVolume, ParameterError, ParameterErrorKind, PixelDimensions,
    ScaledFloat, SourceChromaticities, Unit,
};
use crate::text_metadata::{ITXtChunk, TEXtChunk, TextDecodingError, ZTXtChunk};
use crate::traits::ReadBytesExt;
use crate::{CodingIndependentCodePoints, Limits};

pub const CHUNK_BUFFER_SIZE: usize = 128;

/// Determines if checksum checks should be disabled globally.
///
/// This is used only in fuzzing. `afl` automatically adds `--cfg fuzzing` to RUSTFLAGS which can
/// be used to detect that build.
#[allow(unexpected_cfgs)]
const CHECKSUM_DISABLED: bool = cfg!(fuzzing);

/// Kind of `u32` value that is being read via `State::U32`.
#[derive(Debug)]
enum U32ValueKind {
    /// First 4 bytes of the PNG signature - see
    /// http://www.libpng.org/pub/png/spec/1.2/PNG-Structure.html#PNG-file-signature
    Signature1stU32,
    /// Second 4 bytes of the PNG signature - see
    /// http://www.libpng.org/pub/png/spec/1.2/PNG-Structure.html#PNG-file-signature
    Signature2ndU32,
    /// Chunk length - see
    /// http://www.libpng.org/pub/png/spec/1.2/PNG-Structure.html#Chunk-layout
    Length,
    /// Chunk type - see
    /// http://www.libpng.org/pub/png/spec/1.2/PNG-Structure.html#Chunk-layout
    Type { length: u32 },
    /// Chunk checksum - see
    /// http://www.libpng.org/pub/png/spec/1.2/PNG-Structure.html#Chunk-layout
    Crc(ChunkType),
    /// Sequence number from an `fdAT` chunk - see
    /// https://wiki.mozilla.org/APNG_Specification#.60fdAT.60:_The_Frame_Data_Chunk
    ApngSequenceNumber,
}

#[derive(Debug)]
enum State {
    /// In this state we are reading a u32 value from external input.  We start with
    /// `accumulated_count` set to `0`. After reading or accumulating the required 4 bytes we will
    /// call `parse_32` which will then move onto the next state.
    U32 {
        kind: U32ValueKind,
        bytes: [u8; 4],
        accumulated_count: usize,
    },
    /// In this state we are reading chunk data from external input, and appending it to
    /// `ChunkState::raw_bytes`. Then if all data has been read, we parse the chunk.
    ReadChunkData(ChunkType),
    /// In this state we are reading image data from external input and feeding it directly into
    /// `StreamingDecoder::inflater`.
    ImageData(ChunkType),
}

impl State {
    fn new_u32(kind: U32ValueKind) -> Self {
        Self::U32 {
            kind,
            bytes: [0; 4],
            accumulated_count: 0,
        }
    }
}

#[derive(Debug)]
/// Result of the decoding process
pub enum Decoded {
    /// Nothing decoded yet
    Nothing,

    /// A chunk header (length and type fields) has been read.
    ChunkBegin(u32, ChunkType),

    /// Chunk has been read successfully.
    ChunkComplete(ChunkType),

    /// An ancillary chunk has been read but it was in the wrong place, had corrupt contents, or had
    /// an invalid CRC.
    BadAncillaryChunk(ChunkType),

    /// Skipped an ancillary chunk because it was unrecognized or the decoder was configured to skip
    /// this type of chunk.
    SkippedAncillaryChunk(ChunkType),

    /// Decoded raw image data.
    ImageData,

    /// The last of a consecutive chunk of IDAT was done.
    /// This is distinct from ChunkComplete which only marks that some IDAT chunk was completed but
    /// not that no additional IDAT chunk follows.
    ImageDataFlushed,
}

/// Any kind of error during PNG decoding.
///
/// This enumeration provides a very rough analysis on the origin of the failure. That is, each
/// variant corresponds to one kind of actor causing the error. It should not be understood as a
/// direct blame but can inform the search for a root cause or if such a search is required.
#[derive(Debug)]
pub enum DecodingError {
    /// An error in IO of the underlying reader.
    ///
    /// Note that some IO errors may be recoverable - decoding may be retried after the
    /// error is resolved.  For example, decoding from a slow stream of data (e.g. decoding from a
    /// network stream) may occasionally result in [crate::io::ErrorKind::UnexpectedEof] kind of
    /// error, but decoding can resume when more data becomes available.
    IoError(io::Error),
    /// The input image was not a valid PNG.
    ///
    /// There isn't a lot that can be done here, except if the program itself was responsible for
    /// creating this image then investigate the generator. This is internally implemented with a
    /// large Enum. If You are interested in accessing some of the more exact information on the
    /// variant then we can discuss in an issue.
    Format(FormatError),
    /// An interface was used incorrectly.
    ///
    /// This is used in cases where it's expected that the programmer might trip up and stability
    /// could be affected. For example when:
    ///
    /// * The decoder is polled for more animation frames despite being done (or not being animated
    ///   in the first place).
    /// * The output buffer does not have the required size.
    ///
    /// As a rough guideline for introducing new variants parts of the requirements are dynamically
    /// derived from the (untrusted) input data while the other half is from the caller. In the
    /// above cases the number of frames respectively the size is determined by the file while the
    /// number of calls
    ///
    /// If you're an application you might want to signal that a bug report is appreciated.
    Parameter(ParameterError),
    /// The image would have required exceeding the limits configured with the decoder.
    ///
    /// Note that Your allocations, e.g. when reading into a pre-allocated buffer, is __NOT__
    /// considered part of the limits. Nevertheless, required intermediate buffers such as for
    /// singular lines is checked against the limit.
    ///
    /// Note that this is a best-effort basis.
    LimitsExceeded,
}

#[derive(Debug)]
pub struct FormatError {
    inner: FormatErrorInner,
}

#[derive(Debug)]
pub(crate) enum FormatErrorInner {
    /// Bad framing.
    CrcMismatch {
        /// Stored CRC32 value
        crc_val: u32,
        /// Calculated CRC32 sum
        crc_sum: u32,
        /// The chunk type that has the CRC mismatch.
        chunk: ChunkType,
    },
    /// Not a PNG, the magic signature is missing.
    InvalidSignature,
    // Errors of chunk level ordering, missing etc.
    /// Fctl must occur if an animated chunk occurs.
    MissingFctl,
    /// Image data that was indicated in IHDR or acTL is missing.
    MissingImageData,
    /// 4.3., Must be first.
    ChunkBeforeIhdr {
        kind: ChunkType,
    },
    /// 4.3., some chunks must be before IDAT.
    AfterIdat {
        kind: ChunkType,
    },
    // 4.3., Some chunks must be after PLTE.
    BeforePlte {
        kind: ChunkType,
    },
    /// 4.3., some chunks must be before PLTE.
    AfterPlte {
        kind: ChunkType,
    },
    /// 4.3., some chunks must be between PLTE and IDAT.
    OutsidePlteIdat {
        kind: ChunkType,
    },
    /// 4.3., some chunks must be unique.
    DuplicateChunk {
        kind: ChunkType,
    },
    /// Specifically for fdat there is an embedded sequence number for chunks.
    ApngOrder {
        /// The sequence number in the chunk.
        present: u32,
        /// The one that should have been present.
        expected: u32,
    },
    // Errors specific to particular chunk data to be validated.
    /// The palette did not even contain a single pixel data.
    ShortPalette {
        expected: usize,
        len: usize,
    },
    /// sBIT chunk size based on color type.
    InvalidSbitChunkSize {
        color_type: ColorType,
        expected: usize,
        len: usize,
    },
    InvalidSbit {
        sample_depth: BitDepth,
        sbit: u8,
    },
    /// A palletized image did not have a palette.
    PaletteRequired,
    /// The color-depth combination is not valid according to Table 11.1.
    InvalidColorBitDepth {
        color_type: ColorType,
        bit_depth: BitDepth,
    },
    ColorWithBadTrns(ColorType),
    /// The image width or height is zero.
    InvalidDimensions,
    InvalidBitDepth(u8),
    InvalidColorType(u8),
    InvalidDisposeOp(u8),
    InvalidBlendOp(u8),
    InvalidUnit(u8),
    /// The rendering intent of the sRGB chunk is invalid.
    InvalidSrgbRenderingIntent(u8),
    UnknownCompressionMethod(u8),
    UnknownFilterMethod(u8),
    UnknownInterlaceMethod(u8),
    /// The subframe is not in bounds of the image.
    /// TODO: fields with relevant data.
    BadSubFrameBounds {},
    // Errors specific to the IDAT/fdAT chunks.
    /// The compression of the data stream was faulty.
    CorruptFlateStream {
        err: fdeflate::DecompressionError,
    },
    /// The image data chunk was too short for the expected pixel count.
    NoMoreImageData,
    /// Bad text encoding
    BadTextEncoding(TextDecodingError),
    /// fdAT shorter than 4 bytes
    FdatShorterThanFourBytes,
    /// "11.2.4 IDAT Image data" section of the PNG spec says: There may be multiple IDAT chunks;
    /// if so, they shall appear consecutively with no other intervening chunks.
    /// `UnexpectedRestartOfDataChunkSequence{kind: IDAT}` indicates that there were "intervening
    /// chunks".
    ///
    /// The APNG spec doesn't directly describe an error similar to `CantInterleaveIdatChunks`,
    /// but we require that a new sequence of consecutive `fdAT` chunks cannot appear unless we've
    /// seen an `fcTL` chunk.
    UnexpectedRestartOfDataChunkSequence {
        kind: ChunkType,
    },
    /// Failure to parse a chunk, because the chunk had the wrong number of bytes.
    ChunkLengthWrong {
        kind: ChunkType,
    },
    UnrecognizedCriticalChunk {
        /// The type of the unrecognized critical chunk.
        type_str: ChunkType,
    },
    BadGammaValue,
}

impl error::Error for DecodingError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        None
    }
}

impl fmt::Display for DecodingError {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        use self::DecodingError::*;
        match self {
            IoError(err) => write!(fmt, "{}", err),
            Parameter(desc) => write!(fmt, "{}", &desc),
            Format(desc) => write!(fmt, "{}", desc),
            LimitsExceeded => write!(fmt, "limits are exceeded"),
        }
    }
}

impl fmt::Display for FormatError {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        use FormatErrorInner::*;
        match &self.inner {
            CrcMismatch {
                crc_val,
                crc_sum,
                chunk,
                ..
            } => write!(
                fmt,
                "CRC error: expected 0x{:x} have 0x{:x} while decoding {:?} chunk.",
                crc_val, crc_sum, chunk
            ),
            MissingFctl => write!(fmt, "fcTL chunk missing before fdAT chunk."),
            MissingImageData => write!(fmt, "IDAT or fdAT chunk is missing."),
            ChunkBeforeIhdr { kind } => write!(fmt, "{:?} chunk appeared before IHDR chunk", kind),
            AfterIdat { kind } => write!(fmt, "Chunk {:?} is invalid after IDAT chunk.", kind),
            BeforePlte { kind } => write!(fmt, "Chunk {:?} is invalid before PLTE chunk.", kind),
            AfterPlte { kind } => write!(fmt, "Chunk {:?} is invalid after PLTE chunk.", kind),
            OutsidePlteIdat { kind } => write!(
                fmt,
                "Chunk {:?} must appear between PLTE and IDAT chunks.",
                kind
            ),
            DuplicateChunk { kind } => write!(fmt, "Chunk {:?} must appear at most once.", kind),
            ApngOrder { present, expected } => write!(
                fmt,
                "Sequence is not in order, expected #{} got #{}.",
                expected, present,
            ),
            ShortPalette { expected, len } => write!(
                fmt,
                "Not enough palette entries, expect {} got {}.",
                expected, len
            ),
            InvalidSbitChunkSize {color_type, expected, len} => write!(
                fmt,
                "The size of the sBIT chunk should be {} byte(s), but {} byte(s) were provided for the {:?} color type.",
                expected, len, color_type
            ),
            InvalidSbit {sample_depth, sbit} => write!(
                fmt,
                "Invalid sBIT value {}. It must be greater than zero and less than the sample depth {:?}.",
                sbit, sample_depth
            ),
            PaletteRequired => write!(fmt, "Missing palette of indexed image."),
            InvalidDimensions => write!(fmt, "Invalid image dimensions"),
            InvalidColorBitDepth {
                color_type,
                bit_depth,
            } => write!(
                fmt,
                "Invalid color/depth combination in header: {:?}/{:?}",
                color_type, bit_depth,
            ),
            ColorWithBadTrns(color_type) => write!(
                fmt,
                "Transparency chunk found for color type {:?}.",
                color_type
            ),
            InvalidBitDepth(nr) => write!(fmt, "Invalid bit depth {}.", nr),
            InvalidColorType(nr) => write!(fmt, "Invalid color type {}.", nr),
            InvalidDisposeOp(nr) => write!(fmt, "Invalid dispose op {}.", nr),
            InvalidBlendOp(nr) => write!(fmt, "Invalid blend op {}.", nr),
            InvalidUnit(nr) => write!(fmt, "Invalid physical pixel size unit {}.", nr),
            InvalidSrgbRenderingIntent(nr) => write!(fmt, "Invalid sRGB rendering intent {}.", nr),
            UnknownCompressionMethod(nr) => write!(fmt, "Unknown compression method {}.", nr),
            UnknownFilterMethod(nr) => write!(fmt, "Unknown filter method {}.", nr),
            UnknownInterlaceMethod(nr) => write!(fmt, "Unknown interlace method {}.", nr),
            BadSubFrameBounds {} => write!(fmt, "Sub frame is out-of-bounds."),
            InvalidSignature => write!(fmt, "Invalid PNG signature."),
            NoMoreImageData => write!(
                fmt,
                "IDAT or fDAT chunk does not have enough data for image."
            ),
            CorruptFlateStream { err } => {
                write!(fmt, "Corrupt deflate stream. ")?;
                write!(fmt, "{:?}", err)
            }
            // TODO: Wrap more info in the enum variant
            BadTextEncoding(tde) => {
                match tde {
                    TextDecodingError::Unrepresentable => {
                        write!(fmt, "Unrepresentable data in tEXt chunk.")
                    }
                    TextDecodingError::InvalidKeywordSize => {
                        write!(fmt, "Keyword empty or longer than 79 bytes.")
                    }
                    TextDecodingError::MissingNullSeparator => {
                        write!(fmt, "No null separator in tEXt chunk.")
                    }
                    TextDecodingError::InflationError => {
                        write!(fmt, "Invalid compressed text data.")
                    }
                    TextDecodingError::OutOfDecompressionSpace => {
                        write!(fmt, "Out of decompression space. Try with a larger limit.")
                    }
                    TextDecodingError::InvalidCompressionMethod => {
                        write!(fmt, "Using an unrecognized byte as compression method.")
                    }
                    TextDecodingError::InvalidCompressionFlag => {
                        write!(fmt, "Using a flag that is not 0 or 255 as a compression flag for iTXt chunk.")
                    }
                    TextDecodingError::MissingCompressionFlag => {
                        write!(fmt, "No compression flag in the iTXt chunk.")
                    }
                }
            }
            FdatShorterThanFourBytes => write!(fmt, "fdAT chunk shorter than 4 bytes"),
            UnexpectedRestartOfDataChunkSequence { kind } => {
                write!(fmt, "Unexpected restart of {:?} chunk sequence", kind)
            }
            ChunkLengthWrong { kind } => {
                write!(fmt, "Chunk length wrong: {:?}", kind)
            }
            UnrecognizedCriticalChunk { type_str } => {
                write!(fmt, "Unrecognized critical chunk: {:?}", type_str)
            }
            BadGammaValue => write!(fmt, "Bad gamma value."),
        }
    }
}

impl From<io::Error> for DecodingError {
    fn from(err: io::Error) -> DecodingError {
        DecodingError::IoError(err)
    }
}

impl From<FormatError> for DecodingError {
    fn from(err: FormatError) -> DecodingError {
        DecodingError::Format(err)
    }
}

impl From<FormatErrorInner> for FormatError {
    fn from(inner: FormatErrorInner) -> Self {
        FormatError { inner }
    }
}

impl From<DecodingError> for io::Error {
    fn from(err: DecodingError) -> io::Error {
        match err {
            DecodingError::IoError(err) => err,
            err => {
                let _ = err;
                io::Error::new(io::ErrorKind::Other, "png decoding error")
            }
        }
    }
}

impl From<TextDecodingError> for DecodingError {
    fn from(tbe: TextDecodingError) -> Self {
        DecodingError::Format(FormatError {
            inner: FormatErrorInner::BadTextEncoding(tbe),
        })
    }
}

/// Decoder configuration options
#[derive(Clone)]
pub struct DecodeOptions {
    ignore_adler32: bool,
    ignore_crc: bool,
    ignore_text_chunk: bool,
    ignore_iccp_chunk: bool,
    skip_ancillary_crc_failures: bool,
}

impl Default for DecodeOptions {
    fn default() -> Self {
        Self {
            ignore_adler32: true,
            ignore_crc: false,
            ignore_text_chunk: false,
            ignore_iccp_chunk: false,
            skip_ancillary_crc_failures: true,
        }
    }
}

impl DecodeOptions {
    /// When set, the decoder will not compute and verify the Adler-32 checksum.
    ///
    /// Defaults to `true`.
    pub fn set_ignore_adler32(&mut self, ignore_adler32: bool) {
        self.ignore_adler32 = ignore_adler32;
    }

    /// When set, the decoder will not compute and verify the CRC code.
    ///
    /// Defaults to `false`.
    pub fn set_ignore_crc(&mut self, ignore_crc: bool) {
        self.ignore_crc = ignore_crc;
    }

    /// Flag to ignore computing and verifying the Adler-32 checksum and CRC
    /// code.
    pub fn set_ignore_checksums(&mut self, ignore_checksums: bool) {
        self.ignore_adler32 = ignore_checksums;
        self.ignore_crc = ignore_checksums;
    }

    /// Ignore text chunks while decoding.
    ///
    /// Defaults to `false`.
    pub fn set_ignore_text_chunk(&mut self, ignore_text_chunk: bool) {
        self.ignore_text_chunk = ignore_text_chunk;
    }

    /// Ignore ICCP chunks while decoding.
    ///
    /// Defaults to `false`.
    pub fn set_ignore_iccp_chunk(&mut self, ignore_iccp_chunk: bool) {
        self.ignore_iccp_chunk = ignore_iccp_chunk;
    }

    /// Ignore ancillary chunks if CRC fails
    ///
    /// Defaults to `true`
    pub fn set_skip_ancillary_crc_failures(&mut self, skip_ancillary_crc_failures: bool) {
        self.skip_ancillary_crc_failures = skip_ancillary_crc_failures;
    }
}

/// PNG StreamingDecoder (low-level interface)
///
/// By default, the decoder does not verify Adler-32 checksum computation. To
/// enable checksum verification, set it with [`StreamingDecoder::set_ignore_adler32`]
/// before starting decompression.
pub struct StreamingDecoder {
    state: Option<State>,
    current_chunk: ChunkState,
    /// The inflater state handling consecutive `IDAT` and `fdAT` chunks.
    inflater: ZlibStream,
    /// The complete image info read from all prior chunks.
    pub(crate) info: Option<Info<'static>>,
    /// The animation chunk sequence number.
    current_seq_no: Option<u32>,
    /// Whether we have already seen a start of an IDAT chunk.  (Used to validate chunk ordering -
    /// some chunk types can only appear before or after an IDAT chunk.)
    have_idat: bool,
    /// Whether we are ready for a start of an `IDAT` chunk sequence.  Initially `true` and set to
    /// `false` when the first sequence of consecutive `IDAT` chunks ends.
    ready_for_idat_chunks: bool,
    /// Whether we are ready for a start of an `fdAT` chunk sequence.  Initially `false`.  Set to
    /// `true` after encountering an `fcTL` chunk. Set to `false` when a sequence of consecutive
    /// `fdAT` chunks ends.
    ready_for_fdat_chunks: bool,
    /// Whether we have already seen an iCCP chunk. Used to prevent parsing of duplicate iCCP chunks.
    have_iccp: bool,
    decode_options: DecodeOptions,
    pub(crate) limits: Limits,
}

struct ChunkState {
    /// The type of the current chunk.
    /// Relevant for `IDAT` and `fdAT` which aggregate consecutive chunks of their own type.
    type_: ChunkType,

    /// Partial crc until now.
    crc: Crc32,

    /// Remaining bytes to be read.
    remaining: u32,

    /// Non-decoded bytes in the chunk.
    raw_bytes: Vec<u8>,

    /// Whether this chunk should be skipped or decoded.
    action: ChunkAction,
}

#[derive(Debug, PartialEq)]
enum ChunkAction {
    Process,
    Skip,
    Reject,
}

impl StreamingDecoder {
    /// Creates a new StreamingDecoder
    ///
    /// Allocates the internal buffers.
    pub fn new() -> StreamingDecoder {
        StreamingDecoder::new_with_options(DecodeOptions::default())
    }

    pub fn new_with_options(decode_options: DecodeOptions) -> StreamingDecoder {
        let mut inflater = ZlibStream::new();
        inflater.set_ignore_adler32(decode_options.ignore_adler32);

        StreamingDecoder {
            state: Some(State::new_u32(U32ValueKind::Signature1stU32)),
            current_chunk: ChunkState {
                type_: ChunkType([0; 4]),
                crc: Crc32::new(),
                remaining: 0,
                raw_bytes: Vec::with_capacity(CHUNK_BUFFER_SIZE),
                action: ChunkAction::Process,
            },
            inflater,
            info: None,
            current_seq_no: None,
            have_idat: false,
            have_iccp: false,
            ready_for_idat_chunks: true,
            ready_for_fdat_chunks: false,
            decode_options,
            limits: Limits { bytes: usize::MAX },
        }
    }

    /// Resets the StreamingDecoder
    pub fn reset(&mut self) {
        self.state = Some(State::new_u32(U32ValueKind::Signature1stU32));
        self.current_chunk.crc = Crc32::new();
        self.current_chunk.remaining = 0;
        self.current_chunk.raw_bytes.clear();
        self.inflater.reset();
        self.info = None;
        self.current_seq_no = None;
        self.have_idat = false;
    }

    /// Provides access to the inner `info` field
    pub fn info(&self) -> Option<&Info<'static>> {
        self.info.as_ref()
    }

    pub fn set_ignore_text_chunk(&mut self, ignore_text_chunk: bool) {
        self.decode_options.set_ignore_text_chunk(ignore_text_chunk);
    }

    pub fn set_ignore_iccp_chunk(&mut self, ignore_iccp_chunk: bool) {
        self.decode_options.set_ignore_iccp_chunk(ignore_iccp_chunk);
    }

    /// Return whether the decoder is set to ignore the Adler-32 checksum.
    pub fn ignore_adler32(&self) -> bool {
        self.inflater.ignore_adler32()
    }

    /// Set whether to compute and verify the Adler-32 checksum during
    /// decompression. Return `true` if the flag was successfully set.
    ///
    /// The decoder defaults to `true`.
    ///
    /// This flag cannot be modified after decompression has started until the
    /// [`StreamingDecoder`] is reset.
    pub fn set_ignore_adler32(&mut self, ignore_adler32: bool) -> bool {
        self.inflater.set_ignore_adler32(ignore_adler32)
    }

    /// Set whether to compute and verify the Adler-32 checksum during
    /// decompression.
    ///
    /// The decoder defaults to `false`.
    pub fn set_ignore_crc(&mut self, ignore_crc: bool) {
        self.decode_options.set_ignore_crc(ignore_crc)
    }

    /// Ignore ancillary chunks if CRC fails
    ///
    /// Defaults to `true`
    pub fn set_skip_ancillary_crc_failures(&mut self, skip_ancillary_crc_failures: bool) {
        self.decode_options
            .set_skip_ancillary_crc_failures(skip_ancillary_crc_failures)
    }

    /// Low level StreamingDecoder interface.
    ///
    /// Allows to stream partial data to the encoder. Returns a tuple containing the bytes that have
    /// been consumed from the input buffer and the current decoding result. If the decoded chunk
    /// was an image data chunk, it also appends the read data to `image_data`.
    pub fn update(
        &mut self,
        mut buf: &[u8],
        mut image_data: Option<&mut UnfilterBuf<'_>>,
    ) -> Result<(usize, Decoded), DecodingError> {
        if self.state.is_none() {
            return Err(DecodingError::Parameter(
                ParameterErrorKind::PolledAfterFatalError.into(),
            ));
        }

        let len = buf.len();
        while !buf.is_empty() {
            let image_data = image_data.as_deref_mut();

            match self.next_state(buf, image_data) {
                Ok((bytes, Decoded::Nothing)) => buf = &buf[bytes..],
                Ok((bytes, result)) => {
                    buf = &buf[bytes..];
                    return Ok((len - buf.len(), result));
                }
                Err(err) => {
                    debug_assert!(self.state.is_none());
                    return Err(err);
                }
            }
        }
        Ok((len - buf.len(), Decoded::Nothing))
    }

    fn next_state(
        &mut self,
        buf: &[u8],
        image_data: Option<&mut UnfilterBuf<'_>>,
    ) -> Result<(usize, Decoded), DecodingError> {
        use self::State::*;

        // Driver should ensure that state is never None
        let state = self.state.take().unwrap();

        match state {
            U32 {
                kind,
                mut bytes,
                mut accumulated_count,
            } => {
                debug_assert!(accumulated_count <= 4);
                if accumulated_count == 0 && buf.len() >= 4 {
                    // Handling these `accumulated_count` and `buf.len()` values in a separate `if`
                    // branch is not strictly necessary - the `else` statement below is already
                    // capable of handling these values.  The main reason for special-casing these
                    // values is that they occur fairly frequently and special-casing them results
                    // in performance gains.
                    const CONSUMED_BYTES: usize = 4;
                    self.parse_u32(kind, &buf[0..4], image_data, CONSUMED_BYTES)
                } else {
                    let remaining_count = 4 - accumulated_count;
                    let consumed_bytes = {
                        let available_count = min(remaining_count, buf.len());
                        bytes[accumulated_count..accumulated_count + available_count]
                            .copy_from_slice(&buf[0..available_count]);
                        accumulated_count += available_count;
                        available_count
                    };

                    if accumulated_count < 4 {
                        self.state = Some(U32 {
                            kind,
                            bytes,
                            accumulated_count,
                        });
                        Ok((consumed_bytes, Decoded::Nothing))
                    } else {
                        debug_assert_eq!(accumulated_count, 4);
                        self.parse_u32(kind, &bytes, image_data, consumed_bytes)
                    }
                }
            }
            ReadChunkData(type_str) => {
                debug_assert!(type_str != IDAT && type_str != chunk::fdAT);
                if self.current_chunk.remaining == 0 {
                    self.state = Some(State::new_u32(U32ValueKind::Crc(type_str)));
                    Ok((0, Decoded::Nothing))
                } else {
                    let ChunkState {
                        crc,
                        remaining,
                        raw_bytes,
                        type_: _,
                        action,
                    } = &mut self.current_chunk;

                    let buf_avail = raw_bytes.capacity() - raw_bytes.len();
                    let bytes_avail = min(buf.len(), buf_avail);
                    let n = min(*remaining, bytes_avail as u32);
                    let buf = &buf[..n as usize];

                    if !self.decode_options.ignore_crc {
                        crc.update(buf);
                    }

                    if *action == ChunkAction::Process {
                        if raw_bytes.len() == raw_bytes.capacity() {
                            if self.limits.bytes == 0 {
                                return Err(DecodingError::LimitsExceeded);
                            }

                            // Double the size of the Vec, but not beyond the allocation limit.
                            debug_assert!(raw_bytes.capacity() > 0);
                            let reserve_size = raw_bytes.capacity().min(self.limits.bytes);

                            self.limits.reserve_bytes(reserve_size)?;
                            raw_bytes.reserve_exact(reserve_size);
                        }
                        raw_bytes.extend_from_slice(buf);
                    }

                    *remaining -= n;
                    if *remaining == 0 {
                        debug_assert!(type_str != IDAT && type_str != chunk::fdAT);
                        self.state = Some(State::new_u32(U32ValueKind::Crc(type_str)));
                    } else {
                        self.state = Some(ReadChunkData(type_str));
                    }
                    Ok((n as usize, Decoded::Nothing))
                }
            }
            ImageData(type_str) => {
                debug_assert!(type_str == IDAT || type_str == chunk::fdAT);
                let len = core::cmp::min(buf.len(), self.current_chunk.remaining as usize);
                let buf = &buf[..len];

                let consumed = if let Some(image_data) = image_data {
                    self.inflater.decompress(buf, image_data)?
                } else {
                    len
                };

                if !self.decode_options.ignore_crc {
                    self.current_chunk.crc.update(&buf[..consumed]);
                }

                self.current_chunk.remaining -= consumed as u32;
                if self.current_chunk.remaining == 0 {
                    self.state = Some(State::new_u32(U32ValueKind::Crc(type_str)));
                } else {
                    self.state = Some(ImageData(type_str));
                }
                Ok((consumed, Decoded::ImageData))
            }
        }
    }

    fn parse_u32(
        &mut self,
        kind: U32ValueKind,
        u32_be_bytes: &[u8],
        image_data: Option<&mut UnfilterBuf<'_>>,
        consumed_bytes: usize,
    ) -> Result<(usize, Decoded), DecodingError> {
        debug_assert_eq!(u32_be_bytes.len(), 4);
        let bytes = u32_be_bytes.try_into().unwrap();
        let val = u32::from_be_bytes(bytes);

        match kind {
            U32ValueKind::Signature1stU32 => {
                if bytes == [137, 80, 78, 71] {
                    self.state = Some(State::new_u32(U32ValueKind::Signature2ndU32));
                    Ok((consumed_bytes, Decoded::Nothing))
                } else {
                    Err(DecodingError::Format(
                        FormatErrorInner::InvalidSignature.into(),
                    ))
                }
            }
            U32ValueKind::Signature2ndU32 => {
                if bytes == [13, 10, 26, 10] {
                    self.state = Some(State::new_u32(U32ValueKind::Length));
                    Ok((consumed_bytes, Decoded::Nothing))
                } else {
                    Err(DecodingError::Format(
                        FormatErrorInner::InvalidSignature.into(),
                    ))
                }
            }
            U32ValueKind::Length => {
                self.state = Some(State::new_u32(U32ValueKind::Type { length: val }));
                Ok((consumed_bytes, Decoded::Nothing))
            }
            U32ValueKind::Type { length } => {
                let type_str = ChunkType(bytes);
                if self.info.is_none() && type_str != IHDR {
                    return Err(DecodingError::Format(
                        FormatErrorInner::ChunkBeforeIhdr { kind: type_str }.into(),
                    ));
                }
                if type_str != self.current_chunk.type_
                    && (self.current_chunk.type_ == IDAT || self.current_chunk.type_ == chunk::fdAT)
                {
                    let finished = match image_data {
                        Some(image_data) => self.inflater.finish(image_data)?,
                        None => true,
                    };

                    // We ended up handling IDAT/fdAT data rather than the chunk
                    // type header atually received. Thus rewind `self.state` to
                    // what it was before this function was called.
                    self.state = Some(State::U32 {
                        kind,
                        bytes,
                        accumulated_count: 4 - consumed_bytes,
                    });

                    if finished {
                        // We've processed all the image data necessary. Update
                        // `current_chunk.type_`so this codepath isn't taken
                        // again next time.
                        self.current_chunk.type_ = type_str;
                        self.ready_for_idat_chunks = false;
                        self.ready_for_fdat_chunks = false;
                        return Ok((0, Decoded::ImageDataFlushed));
                    } else {
                        // Report that we processed some image data without
                        // consuming any input. This gives the caller a chance
                        // to grow the output buffer and call us again.
                        return Ok((0, Decoded::ImageData));
                    }
                }

                self.current_chunk.type_ = type_str;
                if !self.decode_options.ignore_crc {
                    self.current_chunk.crc.reset();
                    self.current_chunk.crc.update(&type_str.0);
                }
                self.current_chunk.remaining = length;
                self.current_chunk.raw_bytes.clear();

                self.state = match type_str {
                    chunk::fdAT => {
                        if !self.ready_for_fdat_chunks {
                            return Err(DecodingError::Format(
                                FormatErrorInner::UnexpectedRestartOfDataChunkSequence {
                                    kind: chunk::fdAT,
                                }
                                .into(),
                            ));
                        }
                        if length < 4 {
                            return Err(DecodingError::Format(
                                FormatErrorInner::FdatShorterThanFourBytes.into(),
                            ));
                        }
                        self.current_chunk.action = ChunkAction::Process;
                        Some(State::new_u32(U32ValueKind::ApngSequenceNumber))
                    }
                    IDAT => {
                        if !self.ready_for_idat_chunks {
                            return Err(DecodingError::Format(
                                FormatErrorInner::UnexpectedRestartOfDataChunkSequence {
                                    kind: IDAT,
                                }
                                .into(),
                            ));
                        }
                        self.have_idat = true;
                        self.current_chunk.action = ChunkAction::Process;
                        Some(State::ImageData(type_str))
                    }
                    _ => Some(self.start_chunk(type_str, length)?),
                };
                Ok((consumed_bytes, Decoded::ChunkBegin(length, type_str)))
            }
            U32ValueKind::Crc(type_str) => {
                // If ignore_crc is set, do not calculate CRC. We set
                // sum=val so that it short-circuits to true in the next
                // if-statement block
                let sum = if self.decode_options.ignore_crc {
                    val
                } else {
                    self.current_chunk.crc.clone().finalize()
                };

                if val == sum || CHECKSUM_DISABLED {
                    match self.current_chunk.action {
                        ChunkAction::Process => {
                            // A fatal error in chunk parsing leaves the decoder in state 'None' to enforce
                            // that parsing can't continue after an error.
                            debug_assert!(self.state.is_none());
                            let decoded = self.parse_chunk(type_str)?;

                            if type_str != IEND {
                                self.state = Some(State::new_u32(U32ValueKind::Length));
                            }
                            Ok((consumed_bytes, decoded))
                        }
                        ChunkAction::Skip => {
                            self.state = Some(State::new_u32(U32ValueKind::Length));
                            Ok((
                                consumed_bytes,
                                Decoded::SkippedAncillaryChunk(self.current_chunk.type_),
                            ))
                        }
                        ChunkAction::Reject => {
                            self.state = Some(State::new_u32(U32ValueKind::Length));
                            Ok((consumed_bytes, Decoded::BadAncillaryChunk(type_str)))
                        }
                    }
                } else if self.decode_options.skip_ancillary_crc_failures
                    && !chunk::is_critical(type_str)
                {
                    // Ignore ancillary chunk with invalid CRC
                    self.state = Some(State::new_u32(U32ValueKind::Length));
                    Ok((consumed_bytes, Decoded::BadAncillaryChunk(type_str)))
                } else {
                    Err(DecodingError::Format(
                        FormatErrorInner::CrcMismatch {
                            crc_val: val,
                            crc_sum: sum,
                            chunk: type_str,
                        }
                        .into(),
                    ))
                }
            }
            U32ValueKind::ApngSequenceNumber => {
                debug_assert_eq!(self.current_chunk.type_, chunk::fdAT);
                let next_seq_no = val;

                // Should be verified by the FdatShorterThanFourBytes check earlier.
                debug_assert!(self.current_chunk.remaining >= 4);
                self.current_chunk.remaining -= 4;

                if let Some(seq_no) = self.current_seq_no {
                    if next_seq_no != seq_no + 1 {
                        return Err(DecodingError::Format(
                            FormatErrorInner::ApngOrder {
                                present: next_seq_no,
                                expected: seq_no + 1,
                            }
                            .into(),
                        ));
                    }
                    self.current_seq_no = Some(next_seq_no);
                } else {
                    return Err(DecodingError::Format(FormatErrorInner::MissingFctl.into()));
                }

                if !self.decode_options.ignore_crc {
                    let data = next_seq_no.to_be_bytes();
                    self.current_chunk.crc.update(&data);
                }

                self.state = Some(State::ImageData(chunk::fdAT));
                Ok((consumed_bytes, Decoded::Nothing))
            }
        }
    }

    fn start_chunk(&mut self, type_str: ChunkType, length: u32) -> Result<State, DecodingError> {
        let target_length = match type_str {
            IHDR => 13..=13,
            chunk::PLTE => 3..=768,
            chunk::IEND => 0..=0,
            chunk::sBIT => 1..=4,
            chunk::tRNS => 1..=256,
            chunk::pHYs => 9..=9,
            chunk::gAMA => 4..=4,
            chunk::acTL => 8..=8,
            chunk::fcTL => 26..=26,
            chunk::cHRM => 32..=32,
            chunk::sRGB => 1..=1,
            chunk::cICP => 4..=4,
            chunk::mDCV => 24..=24,
            chunk::cLLI => 8..=8,
            chunk::bKGD => 1..=6,

            // Unbounded size chunks
            chunk::eXIf => 0..=u32::MAX >> 1, // TODO: allow skipping.
            chunk::iCCP if !self.decode_options.ignore_iccp_chunk => 0..=u32::MAX >> 1,
            chunk::tEXt if !self.decode_options.ignore_text_chunk => 0..=u32::MAX >> 1,
            chunk::zTXt if !self.decode_options.ignore_text_chunk => 0..=u32::MAX >> 1,
            chunk::iTXt if !self.decode_options.ignore_text_chunk => 0..=u32::MAX >> 1,

            chunk::IDAT | chunk::fdAT => unreachable!(),

            _ if is_critical(type_str) => {
                return Err(DecodingError::Format(
                    FormatErrorInner::UnrecognizedCriticalChunk { type_str }.into(),
                ));
            }
            _ => {
                self.current_chunk.action = ChunkAction::Skip;
                return Ok(State::ReadChunkData(type_str));
            }
        };

        if !target_length.contains(&length) {
            // Uncomment to detect unexpected chunk lengths during testing.
            // panic!("chunk type_str={type_str:?} has length={length}, target_length={target_length:?}");
            match type_str {
                IHDR | chunk::PLTE | chunk::IEND | chunk::fcTL => {
                    return Err(DecodingError::Format(
                        FormatErrorInner::ChunkLengthWrong { kind: type_str }.into(),
                    ));
                }
                _ => {
                    self.current_chunk.action = ChunkAction::Reject;
                }
            }
        } else {
            self.current_chunk.action = ChunkAction::Process;
        }

        Ok(State::ReadChunkData(type_str))
    }

    fn parse_chunk(&mut self, type_str: ChunkType) -> Result<Decoded, DecodingError> {
        let mut parse_result = match type_str {
            // Critical non-data chunks.
            IHDR => self.parse_ihdr(),
            chunk::PLTE => self.parse_plte(),
            chunk::IEND => Ok(()), // TODO: Check chunk size.

            // Data chunks handled separately.
            chunk::IDAT => Ok(()),
            chunk::fdAT => Ok(()),

            // Recognized bounded-size ancillary chunks.
            chunk::sBIT => self.parse_sbit(),
            chunk::tRNS => self.parse_trns(),
            chunk::pHYs => self.parse_phys(),
            chunk::gAMA => self.parse_gama(),
            chunk::acTL => self.parse_actl(),
            chunk::fcTL => self.parse_fctl(),
            chunk::cHRM => self.parse_chrm(),
            chunk::sRGB => self.parse_srgb(),
            chunk::cICP => self.parse_cicp(),
            chunk::mDCV => self.parse_mdcv(),
            chunk::cLLI => self.parse_clli(),
            chunk::bKGD => self.parse_bkgd(),

            // Ancillary chunks with unbounded size.
            chunk::eXIf => self.parse_exif(),
            chunk::iCCP => self.parse_iccp(),
            chunk::tEXt => self.parse_text(),
            chunk::zTXt => self.parse_ztxt(),
            chunk::iTXt => self.parse_itxt(),

            // Unrecognized chunks.
            _ => unreachable!(
                "Unrecognized chunk {type_str:?} should have been caught in start_chunk"
            ),
        };

        parse_result = parse_result.map_err(|e| {
            match e {
                // `parse_chunk` is invoked after gathering **all** bytes of a chunk, so
                // `UnexpectedEof` from something like `read_be` is permanent and indicates an
                // invalid PNG that should be represented as a `FormatError`, rather than as a
                // (potentially recoverable) `IoError` / `UnexpectedEof`.
                DecodingError::IoError(e) if e.kind() == crate::io::ErrorKind::UnexpectedEof => {
                    let fmt_err: FormatError =
                        FormatErrorInner::ChunkLengthWrong { kind: type_str }.into();
                    fmt_err.into()
                }
                e => e,
            }
        });

        match parse_result {
            Ok(()) => Ok(Decoded::ChunkComplete(type_str)),
            Err(DecodingError::Format(_))
                if type_str != chunk::fcTL && !chunk::is_critical(type_str) =>
            {
                // Ignore benign errors in most auxiliary chunks. `LimitsExceeded`, `Parameter` and
                // other error kinds are *not* treated as benign. We don't ignore errors in `fcTL`
                // chunks because the fallback to the static/non-animated image has to be
                // implemented *on top* of the `StreamingDecoder` API.
                //
                // TODO: Consider supporting a strict mode where even benign errors are reported up.
                // See https://github.com/image-rs/image-png/pull/569#issuecomment-2642062285
                Ok(Decoded::BadAncillaryChunk(type_str))
            }
            Err(e) => Err(e),
        }
    }

    fn parse_fctl(&mut self) -> Result<(), DecodingError> {
        let mut buf = &self.current_chunk.raw_bytes[..];
        let next_seq_no = buf.read_be()?;

        // Assuming that fcTL is required before *every* fdAT-sequence
        self.current_seq_no = Some(if let Some(seq_no) = self.current_seq_no {
            if next_seq_no != seq_no + 1 {
                return Err(DecodingError::Format(
                    FormatErrorInner::ApngOrder {
                        expected: seq_no + 1,
                        present: next_seq_no,
                    }
                    .into(),
                ));
            }
            next_seq_no
        } else {
            if next_seq_no != 0 {
                return Err(DecodingError::Format(
                    FormatErrorInner::ApngOrder {
                        expected: 0,
                        present: next_seq_no,
                    }
                    .into(),
                ));
            }
            0
        });
        self.inflater.reset();
        self.ready_for_fdat_chunks = self.have_idat;
        let fc = FrameControl {
            sequence_number: next_seq_no,
            width: buf.read_be()?,
            height: buf.read_be()?,
            x_offset: buf.read_be()?,
            y_offset: buf.read_be()?,
            delay_num: buf.read_be()?,
            delay_den: buf.read_be()?,
            dispose_op: {
                let dispose_op = buf.read_be()?;
                match DisposeOp::from_u8(dispose_op) {
                    Some(dispose_op) => dispose_op,
                    None => {
                        return Err(DecodingError::Format(
                            FormatErrorInner::InvalidDisposeOp(dispose_op).into(),
                        ))
                    }
                }
            },
            blend_op: {
                let blend_op = buf.read_be()?;
                match BlendOp::from_u8(blend_op) {
                    Some(blend_op) => blend_op,
                    None => {
                        return Err(DecodingError::Format(
                            FormatErrorInner::InvalidBlendOp(blend_op).into(),
                        ))
                    }
                }
            },
        };
        self.info.as_ref().unwrap().validate(&fc)?;
        if !self.have_idat {
            self.info.as_ref().unwrap().validate_default_image(&fc)?;
        }
        self.info.as_mut().unwrap().frame_control = Some(fc);
        Ok(())
    }

    fn parse_actl(&mut self) -> Result<(), DecodingError> {
        let info = self.info.as_mut().unwrap();
        if self.have_idat {
            Err(DecodingError::Format(
                FormatErrorInner::AfterIdat { kind: chunk::acTL }.into(),
            ))
        } else if info.animation_control.is_some() {
            Err(DecodingError::Format(
                FormatErrorInner::DuplicateChunk { kind: chunk::acTL }.into(),
            ))
        } else {
            let mut buf = &self.current_chunk.raw_bytes[..];
            let actl = AnimationControl {
                num_frames: buf.read_be()?,
                num_plays: buf.read_be()?,
            };
            // The spec says that "0 is not a valid value" for `num_frames`.
            // So let's ignore such malformed `acTL` chunks.
            if actl.num_frames == 0 {
                return Ok(());
            }

            // The spec also says that the number of frames and number of plays should be limited
            // to (2^31)-1. Same as the other condition we enforce it by ignoring the chunk.
            // Another option may be saturation which would lose us some frames but encourage
            // rather dubious handling.
            if actl.num_frames > 0x7FFFFFFF || actl.num_plays > 0x7FFFFFFF {
                return Ok(());
            }

            info.animation_control = Some(actl);
            Ok(())
        }
    }

    fn parse_plte(&mut self) -> Result<(), DecodingError> {
        let info = self.info.as_mut().unwrap();
        if info.palette.is_some() {
            // Only one palette is allowed
            Err(DecodingError::Format(
                FormatErrorInner::DuplicateChunk { kind: chunk::PLTE }.into(),
            ))
        } else {
            info.palette = Some(Cow::Owned(self.current_chunk.raw_bytes.clone()));
            Ok(())
        }
    }

    fn parse_sbit(&mut self) -> Result<(), DecodingError> {
        let info = self.info.as_mut().unwrap();
        if info.palette.is_some() {
            return Err(DecodingError::Format(
                FormatErrorInner::AfterPlte { kind: chunk::sBIT }.into(),
            ));
        }

        if self.have_idat {
            return Err(DecodingError::Format(
                FormatErrorInner::AfterIdat { kind: chunk::sBIT }.into(),
            ));
        }

        if info.sbit.is_some() {
            return Err(DecodingError::Format(
                FormatErrorInner::DuplicateChunk { kind: chunk::sBIT }.into(),
            ));
        }

        let (color_type, bit_depth) = { (info.color_type, info.bit_depth) };
        // The sample depth for color type 3 is fixed at eight bits.
        let sample_depth = if color_type == ColorType::Indexed {
            BitDepth::Eight
        } else {
            bit_depth
        };
        let vec = self.current_chunk.raw_bytes.clone();
        let len = vec.len();

        // expected lenth of the chunk
        let expected = match color_type {
            ColorType::Grayscale => 1,
            ColorType::Rgb | ColorType::Indexed => 3,
            ColorType::GrayscaleAlpha => 2,
            ColorType::Rgba => 4,
        };

        // Check if the sbit chunk size is valid.
        if expected != len {
            return Err(DecodingError::Format(
                FormatErrorInner::InvalidSbitChunkSize {
                    color_type,
                    expected,
                    len,
                }
                .into(),
            ));
        }

        for sbit in &vec {
            if *sbit < 1 || *sbit > sample_depth as u8 {
                return Err(DecodingError::Format(
                    FormatErrorInner::InvalidSbit {
                        sample_depth,
                        sbit: *sbit,
                    }
                    .into(),
                ));
            }
        }
        info.sbit = Some(Cow::Owned(vec));
        Ok(())
    }

    fn parse_trns(&mut self) -> Result<(), DecodingError> {
        let info = self.info.as_mut().unwrap();
        if info.trns.is_some() {
            return Err(DecodingError::Format(
                FormatErrorInner::DuplicateChunk { kind: chunk::PLTE }.into(),
            ));
        }
        let (color_type, bit_depth) = { (info.color_type, info.bit_depth as u8) };
        let mut vec = self.current_chunk.raw_bytes.clone();
        let len = vec.len();
        match color_type {
            ColorType::Grayscale => {
                if len < 2 {
                    return Err(DecodingError::Format(
                        FormatErrorInner::ShortPalette { expected: 2, len }.into(),
                    ));
                }
                if bit_depth < 16 {
                    vec[0] = vec[1];
                    vec.truncate(1);
                }
                info.trns = Some(Cow::Owned(vec));
                Ok(())
            }
            ColorType::Rgb => {
                if len < 6 {
                    return Err(DecodingError::Format(
                        FormatErrorInner::ShortPalette { expected: 6, len }.into(),
                    ));
                }
                if bit_depth < 16 {
                    vec[0] = vec[1];
                    vec[1] = vec[3];
                    vec[2] = vec[5];
                    vec.truncate(3);
                }
                info.trns = Some(Cow::Owned(vec));
                Ok(())
            }
            ColorType::Indexed => {
                // The transparency chunk must be after the palette chunk and
                // before the data chunk.
                if info.palette.is_none() {
                    return Err(DecodingError::Format(
                        FormatErrorInner::BeforePlte { kind: chunk::tRNS }.into(),
                    ));
                } else if self.have_idat {
                    return Err(DecodingError::Format(
                        FormatErrorInner::OutsidePlteIdat { kind: chunk::tRNS }.into(),
                    ));
                }

                info.trns = Some(Cow::Owned(vec));
                Ok(())
            }
            c => Err(DecodingError::Format(
                FormatErrorInner::ColorWithBadTrns(c).into(),
            )),
        }
    }

    fn parse_phys(&mut self) -> Result<(), DecodingError> {
        let info = self.info.as_mut().unwrap();
        if self.have_idat {
            Err(DecodingError::Format(
                FormatErrorInner::AfterIdat { kind: chunk::pHYs }.into(),
            ))
        } else if info.pixel_dims.is_some() {
            Err(DecodingError::Format(
                FormatErrorInner::DuplicateChunk { kind: chunk::pHYs }.into(),
            ))
        } else {
            let mut buf = &self.current_chunk.raw_bytes[..];
            let xppu = buf.read_be()?;
            let yppu = buf.read_be()?;
            let unit = buf.read_be()?;
            let unit = match Unit::from_u8(unit) {
                Some(unit) => unit,
                None => {
                    return Err(DecodingError::Format(
                        FormatErrorInner::InvalidUnit(unit).into(),
                    ))
                }
            };
            let pixel_dims = PixelDimensions { xppu, yppu, unit };
            info.pixel_dims = Some(pixel_dims);
            Ok(())
        }
    }

    fn parse_chrm(&mut self) -> Result<(), DecodingError> {
        let info = self.info.as_mut().unwrap();
        if self.have_idat {
            Err(DecodingError::Format(
                FormatErrorInner::AfterIdat { kind: chunk::cHRM }.into(),
            ))
        } else if info.chrm_chunk.is_some() {
            Err(DecodingError::Format(
                FormatErrorInner::DuplicateChunk { kind: chunk::cHRM }.into(),
            ))
        } else {
            let mut buf = &self.current_chunk.raw_bytes[..];
            let white_x: u32 = buf.read_be()?;
            let white_y: u32 = buf.read_be()?;
            let red_x: u32 = buf.read_be()?;
            let red_y: u32 = buf.read_be()?;
            let green_x: u32 = buf.read_be()?;
            let green_y: u32 = buf.read_be()?;
            let blue_x: u32 = buf.read_be()?;
            let blue_y: u32 = buf.read_be()?;

            let source_chromaticities = SourceChromaticities {
                white: (
                    ScaledFloat::from_scaled(white_x),
                    ScaledFloat::from_scaled(white_y),
                ),
                red: (
                    ScaledFloat::from_scaled(red_x),
                    ScaledFloat::from_scaled(red_y),
                ),
                green: (
                    ScaledFloat::from_scaled(green_x),
                    ScaledFloat::from_scaled(green_y),
                ),
                blue: (
                    ScaledFloat::from_scaled(blue_x),
                    ScaledFloat::from_scaled(blue_y),
                ),
            };

            info.chrm_chunk = Some(source_chromaticities);
            Ok(())
        }
    }

    fn parse_gama(&mut self) -> Result<(), DecodingError> {
        let info = self.info.as_mut().unwrap();
        if self.have_idat {
            Err(DecodingError::Format(
                FormatErrorInner::AfterIdat { kind: chunk::gAMA }.into(),
            ))
        } else if info.gama_chunk.is_some() {
            Err(DecodingError::Format(
                FormatErrorInner::DuplicateChunk { kind: chunk::gAMA }.into(),
            ))
        } else {
            let mut buf = &self.current_chunk.raw_bytes[..];
            let source_gamma: u32 = buf.read_be()?;
            if source_gamma == 0 {
                return Err(DecodingError::Format(
                    FormatErrorInner::BadGammaValue.into(),
                ));
            }

            let source_gamma = ScaledFloat::from_scaled(source_gamma);
            info.gama_chunk = Some(source_gamma);
            Ok(())
        }
    }

    fn parse_srgb(&mut self) -> Result<(), DecodingError> {
        let info = self.info.as_mut().unwrap();
        if self.have_idat {
            Err(DecodingError::Format(
                FormatErrorInner::AfterIdat { kind: chunk::sRGB }.into(),
            ))
        } else if info.srgb.is_some() {
            Err(DecodingError::Format(
                FormatErrorInner::DuplicateChunk { kind: chunk::sRGB }.into(),
            ))
        } else {
            let mut buf = &self.current_chunk.raw_bytes[..];
            let raw: u8 = buf.read_be()?; // BE is is nonsense for single bytes, but this way the size is checked.
            let rendering_intent = crate::SrgbRenderingIntent::from_raw(raw).ok_or_else(|| {
                FormatError::from(FormatErrorInner::InvalidSrgbRenderingIntent(raw))
            })?;

            // Set srgb and override source gamma and chromaticities.
            info.srgb = Some(rendering_intent);
            Ok(())
        }
    }

    fn parse_cicp(&mut self) -> Result<(), DecodingError> {
        let info = self.info.as_mut().unwrap();

        // The spec requires that the cICP chunk MUST come before the PLTE and IDAT chunks.
        if info.coding_independent_code_points.is_some() {
            return Err(DecodingError::Format(
                FormatErrorInner::DuplicateChunk { kind: chunk::cICP }.into(),
            ));
        } else if info.palette.is_some() {
            return Err(DecodingError::Format(
                FormatErrorInner::AfterPlte { kind: chunk::cICP }.into(),
            ));
        } else if self.have_idat {
            return Err(DecodingError::Format(
                FormatErrorInner::AfterIdat { kind: chunk::cICP }.into(),
            ));
        }

        let mut buf = &*self.current_chunk.raw_bytes;
        let color_primaries: u8 = buf.read_be()?;
        let transfer_function: u8 = buf.read_be()?;
        let matrix_coefficients: u8 = buf.read_be()?;
        let is_video_full_range_image = {
            let flag: u8 = buf.read_be()?;
            match flag {
                0 => false,
                1 => true,
                _ => {
                    return Err(DecodingError::IoError(
                        crate::io::ErrorKind::InvalidData.into(),
                    ));
                }
            }
        };

        // RGB is currently the only supported color model in PNG, and as
        // such Matrix Coefficients shall be set to 0.
        if matrix_coefficients != 0 {
            return Err(DecodingError::IoError(
                crate::io::ErrorKind::InvalidData.into(),
            ));
        }

        if !buf.is_empty() {
            return Err(DecodingError::IoError(
                crate::io::ErrorKind::InvalidData.into(),
            ));
        }

        info.coding_independent_code_points = Some(CodingIndependentCodePoints {
            color_primaries,
            transfer_function,
            matrix_coefficients,
            is_video_full_range_image,
        });

        Ok(())
    }

    fn parse_mdcv(&mut self) -> Result<(), DecodingError> {
        let info = self.info.as_mut().unwrap();

        // The spec requires that the mDCV chunk MUST come before the PLTE and IDAT chunks.
        if info.mastering_display_color_volume.is_some() {
            return Err(DecodingError::Format(
                FormatErrorInner::DuplicateChunk { kind: chunk::mDCV }.into(),
            ));
        } else if info.palette.is_some() {
            return Err(DecodingError::Format(
                FormatErrorInner::AfterPlte { kind: chunk::mDCV }.into(),
            ));
        } else if self.have_idat {
            return Err(DecodingError::Format(
                FormatErrorInner::AfterIdat { kind: chunk::mDCV }.into(),
            ));
        }

        let mut buf = &*self.current_chunk.raw_bytes;
        let red_x: u16 = buf.read_be()?;
        let red_y: u16 = buf.read_be()?;
        let green_x: u16 = buf.read_be()?;
        let green_y: u16 = buf.read_be()?;
        let blue_x: u16 = buf.read_be()?;
        let blue_y: u16 = buf.read_be()?;
        let white_x: u16 = buf.read_be()?;
        let white_y: u16 = buf.read_be()?;
        fn scale(chunk: u16) -> ScaledFloat {
            // `ScaledFloat::SCALING` is hardcoded to 100_000, which works
            // well for the `cHRM` chunk where the spec says that "a value
            // of 0.3127 would be stored as the integer 31270".  In the
            // `mDCV` chunk the spec says that "0.708, 0.292)" is stored as
            // "{ 35400, 14600 }", using a scaling factor of 50_000, so we
            // multiply by 2 before converting.
            ScaledFloat::from_scaled((chunk as u32) * 2)
        }
        let chromaticities = SourceChromaticities {
            white: (scale(white_x), scale(white_y)),
            red: (scale(red_x), scale(red_y)),
            green: (scale(green_x), scale(green_y)),
            blue: (scale(blue_x), scale(blue_y)),
        };
        let max_luminance: u32 = buf.read_be()?;
        let min_luminance: u32 = buf.read_be()?;
        if !buf.is_empty() {
            return Err(DecodingError::IoError(
                crate::io::ErrorKind::InvalidData.into(),
            ));
        }
        info.mastering_display_color_volume = Some(MasteringDisplayColorVolume {
            chromaticities,
            max_luminance,
            min_luminance,
        });

        Ok(())
    }

    fn parse_clli(&mut self) -> Result<(), DecodingError> {
        let info = self.info.as_mut().unwrap();
        if info.content_light_level.is_some() {
            return Err(DecodingError::Format(
                FormatErrorInner::DuplicateChunk { kind: chunk::cLLI }.into(),
            ));
        }

        let mut buf = &*self.current_chunk.raw_bytes;
        let max_content_light_level: u32 = buf.read_be()?;
        let max_frame_average_light_level: u32 = buf.read_be()?;
        if !buf.is_empty() {
            return Err(DecodingError::IoError(
                crate::io::ErrorKind::InvalidData.into(),
            ));
        }
        info.content_light_level = Some(ContentLightLevelInfo {
            max_content_light_level,
            max_frame_average_light_level,
        });

        Ok(())
    }

    fn parse_exif(&mut self) -> Result<(), DecodingError> {
        let info = self.info.as_mut().unwrap();
        if info.exif_metadata.is_some() {
            return Err(DecodingError::Format(
                FormatErrorInner::DuplicateChunk { kind: chunk::eXIf }.into(),
            ));
        }

        info.exif_metadata = Some(self.current_chunk.raw_bytes.clone().into());
        Ok(())
    }

    fn parse_iccp(&mut self) -> Result<(), DecodingError> {
        if self.have_idat {
            Err(DecodingError::Format(
                FormatErrorInner::AfterIdat { kind: chunk::iCCP }.into(),
            ))
        } else if self.have_iccp {
            Err(DecodingError::Format(
                FormatErrorInner::DuplicateChunk { kind: chunk::iCCP }.into(),
            ))
        } else {
            self.have_iccp = true;
            let _ = self.parse_iccp_raw();
            Ok(())
        }
    }

    fn parse_iccp_raw(&mut self) -> Result<(), DecodingError> {
        let info = self.info.as_mut().unwrap();
        let mut buf = &self.current_chunk.raw_bytes[..];

        // read profile name
        for len in 0..=80 {
            let raw: u8 = buf.read_be()?;
            if (raw == 0 && len == 0) || (raw != 0 && len == 80) {
                return Err(DecodingError::from(TextDecodingError::InvalidKeywordSize));
            }
            if raw == 0 {
                break;
            }
        }

        match buf.read_be()? {
            // compression method
            0u8 => (),
            n => {
                return Err(DecodingError::Format(
                    FormatErrorInner::UnknownCompressionMethod(n).into(),
                ))
            }
        }

        match fdeflate::decompress_to_vec_bounded(buf, self.limits.bytes) {
            Ok(profile) => {
                self.limits.reserve_bytes(profile.len())?;
                info.icc_profile = Some(Cow::Owned(profile));
            }
            Err(fdeflate::BoundedDecompressionError::DecompressionError { inner: err }) => {
                return Err(DecodingError::Format(
                    FormatErrorInner::CorruptFlateStream { err }.into(),
                ))
            }
            Err(fdeflate::BoundedDecompressionError::OutputTooLarge { .. }) => {
                return Err(DecodingError::LimitsExceeded);
            }
        }

        Ok(())
    }

    fn parse_ihdr(&mut self) -> Result<(), DecodingError> {
        if self.info.is_some() {
            return Err(DecodingError::Format(
                FormatErrorInner::DuplicateChunk { kind: IHDR }.into(),
            ));
        }
        let mut buf = &self.current_chunk.raw_bytes[..];
        let width = buf.read_be()?;
        let height = buf.read_be()?;
        if width == 0 || height == 0 {
            return Err(DecodingError::Format(
                FormatErrorInner::InvalidDimensions.into(),
            ));
        }
        let bit_depth = buf.read_be()?;
        let bit_depth = match BitDepth::from_u8(bit_depth) {
            Some(bits) => bits,
            None => {
                return Err(DecodingError::Format(
                    FormatErrorInner::InvalidBitDepth(bit_depth).into(),
                ))
            }
        };
        let color_type = buf.read_be()?;
        let color_type = match ColorType::from_u8(color_type) {
            Some(color_type) => {
                if color_type.is_combination_invalid(bit_depth) {
                    return Err(DecodingError::Format(
                        FormatErrorInner::InvalidColorBitDepth {
                            color_type,
                            bit_depth,
                        }
                        .into(),
                    ));
                } else {
                    color_type
                }
            }
            None => {
                return Err(DecodingError::Format(
                    FormatErrorInner::InvalidColorType(color_type).into(),
                ))
            }
        };
        match buf.read_be()? {
            // compression method
            0u8 => (),
            n => {
                return Err(DecodingError::Format(
                    FormatErrorInner::UnknownCompressionMethod(n).into(),
                ))
            }
        }
        match buf.read_be()? {
            // filter method
            0u8 => (),
            n => {
                return Err(DecodingError::Format(
                    FormatErrorInner::UnknownFilterMethod(n).into(),
                ))
            }
        }
        let interlaced = match buf.read_be()? {
            0u8 => false,
            1 => true,
            n => {
                return Err(DecodingError::Format(
                    FormatErrorInner::UnknownInterlaceMethod(n).into(),
                ))
            }
        };

        self.info = Some(Info {
            width,
            height,
            bit_depth,
            color_type,
            interlaced,
            ..Default::default()
        });

        Ok(())
    }

    fn split_keyword(buf: &[u8]) -> Result<(&[u8], &[u8]), DecodingError> {
        let null_byte_index = buf
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| DecodingError::from(TextDecodingError::MissingNullSeparator))?;

        if null_byte_index == 0 || null_byte_index > 79 {
            return Err(DecodingError::from(TextDecodingError::InvalidKeywordSize));
        }

        Ok((&buf[..null_byte_index], &buf[null_byte_index + 1..]))
    }

    fn parse_text(&mut self) -> Result<(), DecodingError> {
        let buf = &self.current_chunk.raw_bytes[..];
        self.limits.reserve_bytes(buf.len())?;

        let (keyword_slice, value_slice) = Self::split_keyword(buf)?;

        self.info
            .as_mut()
            .unwrap()
            .uncompressed_latin1_text
            .push(TEXtChunk::decode(keyword_slice, value_slice).map_err(DecodingError::from)?);

        Ok(())
    }

    fn parse_ztxt(&mut self) -> Result<(), DecodingError> {
        let buf = &self.current_chunk.raw_bytes[..];
        self.limits.reserve_bytes(buf.len())?;

        let (keyword_slice, value_slice) = Self::split_keyword(buf)?;

        let compression_method = *value_slice
            .first()
            .ok_or_else(|| DecodingError::from(TextDecodingError::InvalidCompressionMethod))?;

        let text_slice = &value_slice[1..];

        self.info.as_mut().unwrap().compressed_latin1_text.push(
            ZTXtChunk::decode(keyword_slice, compression_method, text_slice)
                .map_err(DecodingError::from)?,
        );

        Ok(())
    }

    fn parse_itxt(&mut self) -> Result<(), DecodingError> {
        let buf = &self.current_chunk.raw_bytes[..];
        self.limits.reserve_bytes(buf.len())?;

        let (keyword_slice, value_slice) = Self::split_keyword(buf)?;

        let compression_flag = *value_slice
            .first()
            .ok_or_else(|| DecodingError::from(TextDecodingError::MissingCompressionFlag))?;

        let compression_method = *value_slice
            .get(1)
            .ok_or_else(|| DecodingError::from(TextDecodingError::InvalidCompressionMethod))?;

        let second_null_byte_index = value_slice[2..]
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| DecodingError::from(TextDecodingError::MissingNullSeparator))?
            + 2;

        let language_tag_slice = &value_slice[2..second_null_byte_index];

        let third_null_byte_index = value_slice[second_null_byte_index + 1..]
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| DecodingError::from(TextDecodingError::MissingNullSeparator))?
            + (second_null_byte_index + 1);

        let translated_keyword_slice =
            &value_slice[second_null_byte_index + 1..third_null_byte_index];

        let text_slice = &value_slice[third_null_byte_index + 1..];

        self.info.as_mut().unwrap().utf8_text.push(
            ITXtChunk::decode(
                keyword_slice,
                compression_flag,
                compression_method,
                language_tag_slice,
                translated_keyword_slice,
                text_slice,
            )
            .map_err(DecodingError::from)?,
        );

        Ok(())
    }

    fn parse_bkgd(&mut self) -> Result<(), DecodingError> {
        let info = self.info.as_mut().unwrap();
        if info.bkgd.is_some() {
            // Only one bKGD chunk is allowed
            return Err(DecodingError::Format(
                FormatErrorInner::DuplicateChunk { kind: chunk::bKGD }.into(),
            ));
        } else if self.have_idat {
            return Err(DecodingError::Format(
                FormatErrorInner::AfterIdat { kind: chunk::bKGD }.into(),
            ));
        }

        let expected = match info.color_type {
            ColorType::Indexed => {
                if info.palette.is_none() {
                    return Err(DecodingError::IoError(
                        crate::io::ErrorKind::InvalidData.into(),
                    ));
                };
                1
            }
            ColorType::Grayscale | ColorType::GrayscaleAlpha => 2,
            ColorType::Rgb | ColorType::Rgba => 6,
        };
        let vec = self.current_chunk.raw_bytes.clone();
        if vec.len() != expected {
            return Err(DecodingError::Format(
                FormatErrorInner::ChunkLengthWrong { kind: chunk::bKGD }.into(),
            ));
        }

        info.bkgd = Some(Cow::Owned(vec));
        Ok(())
    }
}

impl Info<'_> {
    fn validate_default_image(&self, fc: &FrameControl) -> Result<(), DecodingError> {
        // https://www.w3.org/TR/png-3/#fcTL-chunk says that:
        //
        // > The fcTL chunk corresponding to the default image, if it exists, has these
        // > restrictions:
        // >
        // > * The x_offset and y_offset fields must be 0.
        // > * The width and height fields must equal
        // >   the corresponding fields from the IHDR chunk.
        if fc.x_offset != 0
            || fc.y_offset != 0
            || fc.width != self.width
            || fc.height != self.height
        {
            return Err(DecodingError::Format(
                FormatErrorInner::BadSubFrameBounds {}.into(),
            ));
        }
        Ok(())
    }

    fn validate(&self, fc: &FrameControl) -> Result<(), DecodingError> {
        if fc.width == 0 || fc.height == 0 {
            return Err(DecodingError::Format(
                FormatErrorInner::InvalidDimensions.into(),
            ));
        }

        // Validate mathematically: fc.width + fc.x_offset <= self.width
        let in_x_bounds = Some(fc.width) <= self.width.checked_sub(fc.x_offset);
        // Validate mathematically: fc.height + fc.y_offset <= self.height
        let in_y_bounds = Some(fc.height) <= self.height.checked_sub(fc.y_offset);

        if !in_x_bounds || !in_y_bounds {
            return Err(DecodingError::Format(
                // TODO: do we want to display the bad bounds?
                FormatErrorInner::BadSubFrameBounds {}.into(),
            ));
        }

        Ok(())
    }
}

impl Default for StreamingDecoder {
    fn default() -> Self {
        Self::new()
    }
}
