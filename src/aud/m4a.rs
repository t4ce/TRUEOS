use alloc::vec::Vec;
use core::fmt;

pub const PCM_SAMPLE_RATE_HZ: u32 = 48_000;
pub const PCM_CHANNELS: usize = 2;
pub const PCM_SAMPLE_BITS: usize = 16;
pub const PCM_SAMPLE_BYTES: usize = PCM_SAMPLE_BITS / 8;
pub const PCM_FRAME_BYTES: usize = PCM_CHANNELS * PCM_SAMPLE_BYTES;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodedPcm48kStereo {
    pub samples: Vec<i16>,
    pub frames: usize,
}

impl DecodedPcm48kStereo {
    pub fn new(samples: Vec<i16>) -> Result<Self, M4aDecodeError> {
        if samples.len() % PCM_CHANNELS != 0 {
            return Err(M4aDecodeError::InvalidPcmSampleCount {
                samples: samples.len(),
            });
        }

        Ok(Self {
            frames: samples.len() / PCM_CHANNELS,
            samples,
        })
    }

    pub fn sample_rate_hz(&self) -> u32 {
        PCM_SAMPLE_RATE_HZ
    }

    pub fn channels(&self) -> usize {
        PCM_CHANNELS
    }

    pub fn sample_bits(&self) -> usize {
        PCM_SAMPLE_BITS
    }

    pub fn frame_bytes(&self) -> usize {
        PCM_FRAME_BYTES
    }

    pub fn duration_ms_ceil(&self) -> u32 {
        duration_ms_for_frames(self.frames)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct M4aContainerInfo {
    pub ftyp_offset: usize,
    pub ftyp_size: usize,
    pub major_brand: FourCc,
    pub minor_version: u32,
    pub compatible_brand_count: usize,
    pub has_m4a_brand: bool,
}

#[derive(Copy, Clone, Eq, PartialEq)]
pub struct FourCc(pub [u8; 4]);

impl FourCc {
    pub const fn from_bytes(bytes: [u8; 4]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(self) -> [u8; 4] {
        self.0
    }

    pub fn is_m4a(self) -> bool {
        self.0 == *b"M4A "
    }
}

impl fmt::Debug for FourCc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt_fourcc(f, self.0)
    }
}

impl fmt::Display for FourCc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt_fourcc(f, self.0)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum M4aDecodeError {
    NotM4aContainer,
    TruncatedBoxHeader,
    InvalidBoxSize {
        offset: usize,
        size: u64,
    },
    TruncatedBox {
        offset: usize,
        size: u64,
        input_len: usize,
    },
    InvalidFtypPayload {
        offset: usize,
        size: usize,
    },
    Demux(crate::aud::m4a_demux::M4aError),
    DecoderMissing {
        container: M4aContainerInfo,
        source_sample_rate: Option<u32>,
        source_channels: Option<u16>,
        packet_count: usize,
        asc_len: usize,
        object_type_indication: Option<u8>,
    },
    InvalidPcmSampleCount {
        samples: usize,
    },
    DecodeFailed,
}

impl M4aDecodeError {
    pub fn code(self) -> i32 {
        match self {
            Self::NotM4aContainer => -1,
            Self::TruncatedBoxHeader => -2,
            Self::InvalidBoxSize { .. } => -3,
            Self::TruncatedBox { .. } => -4,
            Self::InvalidFtypPayload { .. } => -5,
            Self::Demux(_) => -6,
            Self::DecoderMissing { .. } => -7,
            Self::InvalidPcmSampleCount { .. } => -8,
            Self::DecodeFailed => -9,
        }
    }

    pub fn is_decoder_missing(self) -> bool {
        matches!(self, Self::DecoderMissing { .. })
    }
}

pub fn decode_m4a_to_pcm_48k_stereo_s16(
    bytes: &[u8],
) -> Result<DecodedPcm48kStereo, M4aDecodeError> {
    let container = detect_m4a_container(bytes)?;
    let demuxed = crate::aud::m4a_demux::parse_m4a(bytes).map_err(M4aDecodeError::Demux)?;

    // Decoder internals land behind this boundary: demux AAC/ALAC samples,
    // decode, resample, and normalize to interleaved s16 stereo at 48 kHz.
    Err(M4aDecodeError::DecoderMissing {
        container,
        source_sample_rate: demuxed.sample_rate,
        source_channels: demuxed.channel_count,
        packet_count: demuxed.packets.len(),
        asc_len: demuxed.codec.audio_specific_config.len(),
        object_type_indication: demuxed.codec.object_type_indication,
    })
}

pub fn detect_m4a_container(bytes: &[u8]) -> Result<M4aContainerInfo, M4aDecodeError> {
    let mut offset = 0usize;

    while offset < bytes.len() {
        let header = read_box_header(bytes, offset)?;
        let payload_offset = offset + header.header_size;
        let payload_len = header.size - header.header_size;

        if header.kind == FourCc::from_bytes(*b"ftyp") {
            return parse_ftyp(bytes, offset, header.size, payload_offset, payload_len);
        }

        if !is_allowed_before_ftyp(header.kind) {
            return Err(M4aDecodeError::NotM4aContainer);
        }

        offset = offset
            .checked_add(header.size)
            .ok_or(M4aDecodeError::InvalidBoxSize {
                offset,
                size: header.size as u64,
            })?;
    }

    Err(M4aDecodeError::NotM4aContainer)
}

pub fn duration_ms_for_frames(frames: usize) -> u32 {
    let ms = (((frames as u128) * 1_000) + u128::from(PCM_SAMPLE_RATE_HZ) - 1)
        / u128::from(PCM_SAMPLE_RATE_HZ);
    ms.clamp(1, u128::from(u32::MAX)) as u32
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct BoxHeader {
    size: usize,
    header_size: usize,
    kind: FourCc,
}

fn parse_ftyp(
    bytes: &[u8],
    ftyp_offset: usize,
    ftyp_size: usize,
    payload_offset: usize,
    payload_len: usize,
) -> Result<M4aContainerInfo, M4aDecodeError> {
    if payload_len < 8 || payload_len % 4 != 0 {
        return Err(M4aDecodeError::InvalidFtypPayload {
            offset: ftyp_offset,
            size: ftyp_size,
        });
    }

    let major_brand = FourCc::from_bytes(read_array_4(bytes, payload_offset).ok_or(
        M4aDecodeError::InvalidFtypPayload {
            offset: ftyp_offset,
            size: ftyp_size,
        },
    )?);
    let minor_version =
        be_u32(bytes, payload_offset + 4).ok_or(M4aDecodeError::InvalidFtypPayload {
            offset: ftyp_offset,
            size: ftyp_size,
        })?;

    let compatible_brand_count = (payload_len - 8) / 4;
    let mut has_m4a_brand = major_brand.is_m4a();
    let mut brand_offset = payload_offset + 8;
    for _ in 0..compatible_brand_count {
        let brand = FourCc::from_bytes(read_array_4(bytes, brand_offset).ok_or(
            M4aDecodeError::InvalidFtypPayload {
                offset: ftyp_offset,
                size: ftyp_size,
            },
        )?);
        has_m4a_brand |= brand.is_m4a();
        brand_offset += 4;
    }

    Ok(M4aContainerInfo {
        ftyp_offset,
        ftyp_size,
        major_brand,
        minor_version,
        compatible_brand_count,
        has_m4a_brand,
    })
}

fn read_box_header(bytes: &[u8], offset: usize) -> Result<BoxHeader, M4aDecodeError> {
    if bytes.len().saturating_sub(offset) < 8 {
        return Err(M4aDecodeError::TruncatedBoxHeader);
    }

    let size32 = be_u32(bytes, offset).ok_or(M4aDecodeError::TruncatedBoxHeader)?;
    let kind = FourCc::from_bytes(
        read_array_4(bytes, offset + 4).ok_or(M4aDecodeError::TruncatedBoxHeader)?,
    );

    let (size, header_size) = match size32 {
        0 => (bytes.len() - offset, 8),
        1 => {
            if bytes.len().saturating_sub(offset) < 16 {
                return Err(M4aDecodeError::TruncatedBoxHeader);
            }
            let size64 = be_u64(bytes, offset + 8).ok_or(M4aDecodeError::TruncatedBoxHeader)?;
            let size = usize::try_from(size64).map_err(|_| M4aDecodeError::InvalidBoxSize {
                offset,
                size: size64,
            })?;
            (size, 16)
        }
        size => (
            usize::try_from(size).map_err(|_| M4aDecodeError::InvalidBoxSize {
                offset,
                size: u64::from(size),
            })?,
            8,
        ),
    };

    if size < header_size {
        return Err(M4aDecodeError::InvalidBoxSize {
            offset,
            size: size as u64,
        });
    }

    let end = offset
        .checked_add(size)
        .ok_or(M4aDecodeError::InvalidBoxSize {
            offset,
            size: size as u64,
        })?;
    if end > bytes.len() {
        return Err(M4aDecodeError::TruncatedBox {
            offset,
            size: size as u64,
            input_len: bytes.len(),
        });
    }

    Ok(BoxHeader {
        size,
        header_size,
        kind,
    })
}

fn is_allowed_before_ftyp(kind: FourCc) -> bool {
    matches!(kind.0, [b'f', b'r', b'e', b'e'] | [b's', b'k', b'i', b'p'] | [b'w', b'i', b'd', b'e'])
}

fn be_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let bytes = bytes.get(offset..offset + 4)?;
    Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn be_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    let bytes = bytes.get(offset..offset + 8)?;
    Some(u64::from_be_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]))
}

fn read_array_4(bytes: &[u8], offset: usize) -> Option<[u8; 4]> {
    let bytes = bytes.get(offset..offset + 4)?;
    Some([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn fmt_fourcc(f: &mut fmt::Formatter<'_>, bytes: [u8; 4]) -> fmt::Result {
    f.write_str("'")?;
    for byte in bytes {
        let c = if byte.is_ascii_graphic() || byte == b' ' {
            char::from(byte)
        } else {
            '.'
        };
        f.write_str(c.encode_utf8(&mut [0; 4]))?;
    }
    f.write_str("'")
}
