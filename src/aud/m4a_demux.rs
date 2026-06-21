//! Minimal no_std M4A/MP4 audio demux helper.
//!
//! This module parses enough ISO-BMFF structure to find an AAC `mp4a` sample
//! description, its `esds` codec config, and packet byte ranges in the original
//! byte slice. It does not decode AAC frames.

extern crate alloc;

use alloc::vec::Vec;
use core::convert::TryFrom;

pub const MAX_STSD_ENTRIES: usize = 16;
pub const MAX_STTS_ENTRIES: usize = 4096;
pub const MAX_STSC_ENTRIES: usize = 4096;
pub const MAX_CHUNK_OFFSETS: usize = 262_144;
pub const MAX_SAMPLE_COUNT: usize = 262_144;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct M4aFile {
    pub ftyp: FtypBox,
    pub codec: CodecConfig,
    pub sample_rate: Option<u32>,
    pub channel_count: Option<u16>,
    pub packets: Vec<PacketRange>,
    pub mdat_ranges: Vec<ByteRange>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FtypBox {
    pub major_brand: [u8; 4],
    pub minor_version: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodecConfig {
    pub sample_entry_format: [u8; 4],
    pub object_type_indication: Option<u8>,
    pub audio_specific_config: Vec<u8>,
    pub aac: Option<AacAudioSpecificConfig>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AacAudioSpecificConfig {
    pub audio_object_type: u8,
    pub sampling_frequency: Option<u32>,
    pub channel_configuration: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PacketRange {
    pub offset: usize,
    pub len: usize,
    pub timestamp: Option<u64>,
    pub duration: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ByteRange {
    pub offset: usize,
    pub len: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum M4aError {
    Truncated,
    InvalidBox,
    InvalidDescriptor,
    MissingFtyp,
    MissingMoov,
    MissingAudioTrack,
    MissingSampleDescription,
    MissingSampleSizes,
    MissingSampleToChunk,
    MissingChunkOffsets,
    TableTooLarge(&'static str),
    RangeOverflow,
    UnsupportedVersion(&'static str),
    InvalidData(&'static str),
}

#[derive(Clone, Copy)]
struct BoxHeader {
    typ: [u8; 4],
    data_start: usize,
    end: usize,
}

#[derive(Clone, Debug, Default)]
struct TrackTables {
    handler: Option<[u8; 4]>,
    sample_description: Option<SampleDescription>,
    stts: Vec<TimeToSample>,
    stsc: Vec<SampleToChunk>,
    sample_sizes: Option<SampleSizes>,
    chunk_offsets: Vec<u64>,
}

#[derive(Clone, Debug)]
struct SampleDescription {
    format: [u8; 4],
    object_type_indication: Option<u8>,
    audio_specific_config: Vec<u8>,
    sample_rate: Option<u32>,
    channel_count: Option<u16>,
}

#[derive(Clone, Copy, Debug)]
struct TimeToSample {
    sample_count: u32,
    sample_delta: u32,
}

#[derive(Clone, Copy, Debug)]
struct SampleToChunk {
    first_chunk: u32,
    samples_per_chunk: u32,
    sample_description_index: u32,
}

#[derive(Clone, Debug)]
struct SampleSizes {
    default_size: u32,
    sample_count: usize,
    sizes: Vec<u32>,
}

/// Parse an M4A/MP4 byte slice and return AAC codec config plus packet ranges.
pub fn parse_m4a(bytes: &[u8]) -> Result<M4aFile, M4aError> {
    let mut ftyp = None;
    let mut moov = None;
    let mut mdat_ranges = Vec::new();

    let mut off = 0usize;
    while off < bytes.len() {
        let header = read_box(bytes, off, bytes.len())?;
        match &header.typ {
            b"ftyp" => ftyp = Some(parse_ftyp(bytes, header)?),
            b"moov" => moov = Some((header.data_start, header.end)),
            b"mdat" => mdat_ranges.push(ByteRange {
                offset: header.data_start,
                len: header.end - header.data_start,
            }),
            _ => {}
        }
        off = header.end;
    }

    let ftyp = ftyp.ok_or(M4aError::MissingFtyp)?;
    let (moov_start, moov_end) = moov.ok_or(M4aError::MissingMoov)?;
    let track = parse_moov(bytes, moov_start, moov_end)?;
    let sample_description = track
        .sample_description
        .as_ref()
        .ok_or(M4aError::MissingSampleDescription)?;
    let packets = build_packets(bytes, &track)?;
    let aac = parse_aac_audio_specific_config(&sample_description.audio_specific_config);
    let sample_rate = sample_description
        .sample_rate
        .or_else(|| aac.and_then(|cfg| cfg.sampling_frequency));
    let channel_count = sample_description
        .channel_count
        .or_else(|| aac.and_then(|cfg| aac_channel_count(cfg.channel_configuration)));

    Ok(M4aFile {
        ftyp,
        codec: CodecConfig {
            sample_entry_format: sample_description.format,
            object_type_indication: sample_description.object_type_indication,
            audio_specific_config: sample_description.audio_specific_config.clone(),
            aac,
        },
        sample_rate,
        channel_count,
        packets,
        mdat_ranges,
    })
}

impl M4aFile {
    pub fn packet_data<'a>(&self, source: &'a [u8], packet: PacketRange) -> Option<&'a [u8]> {
        source.get(packet.offset..packet.offset.checked_add(packet.len)?)
    }
}

fn parse_ftyp(bytes: &[u8], header: BoxHeader) -> Result<FtypBox, M4aError> {
    if header.end - header.data_start < 8 {
        return Err(M4aError::Truncated);
    }
    Ok(FtypBox {
        major_brand: fourcc(bytes, header.data_start)?,
        minor_version: be_u32(bytes, header.data_start + 4)?,
    })
}

fn parse_moov(bytes: &[u8], start: usize, end: usize) -> Result<TrackTables, M4aError> {
    let mut first_track = None;
    let mut first_audio_track = None;
    let mut off = start;

    while off < end {
        let header = read_box(bytes, off, end)?;
        if &header.typ == b"trak" {
            let track = parse_trak(bytes, header.data_start, header.end)?;
            if first_track.is_none() {
                first_track = Some(track.clone());
            }
            if track.handler == Some(*b"soun") {
                first_audio_track = Some(track);
                break;
            }
        }
        off = header.end;
    }

    first_audio_track
        .or(first_track)
        .ok_or(M4aError::MissingAudioTrack)
}

fn parse_trak(bytes: &[u8], start: usize, end: usize) -> Result<TrackTables, M4aError> {
    let mut tables = TrackTables::default();
    let mut off = start;

    while off < end {
        let header = read_box(bytes, off, end)?;
        if &header.typ == b"mdia" {
            parse_mdia(bytes, header.data_start, header.end, &mut tables)?;
        }
        off = header.end;
    }

    Ok(tables)
}

fn parse_mdia(
    bytes: &[u8],
    start: usize,
    end: usize,
    tables: &mut TrackTables,
) -> Result<(), M4aError> {
    let mut off = start;

    while off < end {
        let header = read_box(bytes, off, end)?;
        match &header.typ {
            b"hdlr" => tables.handler = parse_hdlr(bytes, header.data_start, header.end)?,
            b"minf" => parse_minf(bytes, header.data_start, header.end, tables)?,
            _ => {}
        }
        off = header.end;
    }

    Ok(())
}

fn parse_minf(
    bytes: &[u8],
    start: usize,
    end: usize,
    tables: &mut TrackTables,
) -> Result<(), M4aError> {
    let mut off = start;

    while off < end {
        let header = read_box(bytes, off, end)?;
        if &header.typ == b"stbl" {
            parse_stbl(bytes, header.data_start, header.end, tables)?;
        }
        off = header.end;
    }

    Ok(())
}

fn parse_stbl(
    bytes: &[u8],
    start: usize,
    end: usize,
    tables: &mut TrackTables,
) -> Result<(), M4aError> {
    let mut off = start;

    while off < end {
        let header = read_box(bytes, off, end)?;
        match &header.typ {
            b"stsd" => {
                tables.sample_description = parse_stsd(bytes, header.data_start, header.end)?
            }
            b"stts" => tables.stts = parse_stts(bytes, header.data_start, header.end)?,
            b"stsc" => tables.stsc = parse_stsc(bytes, header.data_start, header.end)?,
            b"stsz" => {
                tables.sample_sizes = Some(parse_stsz(bytes, header.data_start, header.end)?)
            }
            b"stco" => tables.chunk_offsets = parse_stco(bytes, header.data_start, header.end)?,
            b"co64" => tables.chunk_offsets = parse_co64(bytes, header.data_start, header.end)?,
            _ => {}
        }
        off = header.end;
    }

    Ok(())
}

fn parse_hdlr(bytes: &[u8], start: usize, end: usize) -> Result<Option<[u8; 4]>, M4aError> {
    let payload = full_box_payload(bytes, start, end, "hdlr")?;
    if payload + 8 > end {
        return Ok(None);
    }
    Ok(Some(fourcc(bytes, payload + 4)?))
}

fn parse_stsd(
    bytes: &[u8],
    start: usize,
    end: usize,
) -> Result<Option<SampleDescription>, M4aError> {
    let payload = full_box_payload(bytes, start, end, "stsd")?;
    let entry_count = usize_from_u32(be_u32(bytes, payload)?)?;
    if entry_count > MAX_STSD_ENTRIES {
        return Err(M4aError::TableTooLarge("stsd"));
    }

    let mut off = payload + 4;
    let mut selected = None;
    for _ in 0..entry_count {
        let size = usize_from_u32(be_u32(bytes, off)?)?;
        if size < 8 {
            return Err(M4aError::InvalidBox);
        }
        let entry_end = off.checked_add(size).ok_or(M4aError::RangeOverflow)?;
        if entry_end > end {
            return Err(M4aError::Truncated);
        }

        let format = fourcc(bytes, off + 4)?;
        if selected.is_none() && &format == b"mp4a" {
            selected = Some(parse_audio_sample_entry(bytes, off + 8, entry_end, format)?);
        }
        off = entry_end;
    }

    Ok(selected)
}

fn parse_audio_sample_entry(
    bytes: &[u8],
    start: usize,
    end: usize,
    format: [u8; 4],
) -> Result<SampleDescription, M4aError> {
    if start + 28 > end {
        return Err(M4aError::Truncated);
    }

    let entry_channel_count = be_u16(bytes, start + 16)?;
    let entry_sample_rate = be_u32(bytes, start + 24)? >> 16;
    let mut description = SampleDescription {
        format,
        object_type_indication: None,
        audio_specific_config: Vec::new(),
        sample_rate: nonzero_u32(entry_sample_rate),
        channel_count: nonzero_u16(entry_channel_count),
    };

    let mut off = start + 28;
    while off < end {
        let header = read_box(bytes, off, end)?;
        if &header.typ == b"esds" {
            let esds = parse_esds(bytes, header.data_start, header.end)?;
            description.object_type_indication = esds.object_type_indication;
            description.audio_specific_config = esds.audio_specific_config;
        }
        off = header.end;
    }

    Ok(description)
}

#[derive(Default)]
struct EsdsConfig {
    object_type_indication: Option<u8>,
    audio_specific_config: Vec<u8>,
}

fn parse_esds(bytes: &[u8], start: usize, end: usize) -> Result<EsdsConfig, M4aError> {
    let payload = full_box_payload(bytes, start, end, "esds")?;
    let mut config = EsdsConfig::default();
    parse_descriptors(bytes, payload, end, 0, &mut config)?;
    Ok(config)
}

fn parse_descriptors(
    bytes: &[u8],
    mut off: usize,
    end: usize,
    depth: u8,
    config: &mut EsdsConfig,
) -> Result<(), M4aError> {
    if depth > 8 {
        return Err(M4aError::InvalidDescriptor);
    }

    while off < end {
        let desc = read_descriptor(bytes, off, end)?;
        match desc.tag {
            0x03 => {
                let child_start =
                    es_descriptor_child_start(bytes, desc.payload_start, desc.payload_end)?;
                parse_descriptors(bytes, child_start, desc.payload_end, depth + 1, config)?;
            }
            0x04 => {
                if desc.payload_start + 13 <= desc.payload_end {
                    config.object_type_indication = Some(bytes[desc.payload_start]);
                    parse_descriptors(
                        bytes,
                        desc.payload_start + 13,
                        desc.payload_end,
                        depth + 1,
                        config,
                    )?;
                }
            }
            0x05 => {
                config.audio_specific_config.clear();
                config
                    .audio_specific_config
                    .extend_from_slice(&bytes[desc.payload_start..desc.payload_end]);
            }
            _ => {}
        }
        off = desc.next;
    }

    Ok(())
}

fn es_descriptor_child_start(
    bytes: &[u8],
    payload_start: usize,
    payload_end: usize,
) -> Result<usize, M4aError> {
    if payload_start + 3 > payload_end {
        return Err(M4aError::InvalidDescriptor);
    }

    let flags = bytes[payload_start + 2];
    let mut off = payload_start + 3;
    if flags & 0x80 != 0 {
        off = off.checked_add(2).ok_or(M4aError::RangeOverflow)?;
    }
    if flags & 0x40 != 0 {
        let url_len = *bytes.get(off).ok_or(M4aError::InvalidDescriptor)? as usize;
        off = off
            .checked_add(1)
            .and_then(|value| value.checked_add(url_len))
            .ok_or(M4aError::RangeOverflow)?;
    }
    if flags & 0x20 != 0 {
        off = off.checked_add(2).ok_or(M4aError::RangeOverflow)?;
    }
    if off > payload_end {
        return Err(M4aError::InvalidDescriptor);
    }
    Ok(off)
}

struct Descriptor {
    tag: u8,
    payload_start: usize,
    payload_end: usize,
    next: usize,
}

fn read_descriptor(bytes: &[u8], off: usize, end: usize) -> Result<Descriptor, M4aError> {
    let tag = *bytes.get(off).ok_or(M4aError::Truncated)?;
    let mut len = 0usize;
    let mut cursor = off + 1;

    for _ in 0..4 {
        let byte = *bytes.get(cursor).ok_or(M4aError::Truncated)?;
        cursor += 1;
        len = (len << 7) | usize::from(byte & 0x7f);
        if byte & 0x80 == 0 {
            let payload_end = cursor.checked_add(len).ok_or(M4aError::RangeOverflow)?;
            if payload_end > end {
                return Err(M4aError::InvalidDescriptor);
            }
            return Ok(Descriptor {
                tag,
                payload_start: cursor,
                payload_end,
                next: payload_end,
            });
        }
    }

    Err(M4aError::InvalidDescriptor)
}

fn parse_stts(bytes: &[u8], start: usize, end: usize) -> Result<Vec<TimeToSample>, M4aError> {
    let payload = full_box_payload(bytes, start, end, "stts")?;
    let count = usize_from_u32(be_u32(bytes, payload)?)?;
    if count > MAX_STTS_ENTRIES {
        return Err(M4aError::TableTooLarge("stts"));
    }

    let mut entries = Vec::with_capacity(count);
    let mut off = payload + 4;
    for _ in 0..count {
        let sample_count = be_u32(bytes, off)?;
        let sample_delta = be_u32(bytes, off + 4)?;
        entries.push(TimeToSample {
            sample_count,
            sample_delta,
        });
        off += 8;
    }
    if off > end {
        return Err(M4aError::Truncated);
    }
    Ok(entries)
}

fn parse_stsc(bytes: &[u8], start: usize, end: usize) -> Result<Vec<SampleToChunk>, M4aError> {
    let payload = full_box_payload(bytes, start, end, "stsc")?;
    let count = usize_from_u32(be_u32(bytes, payload)?)?;
    if count > MAX_STSC_ENTRIES {
        return Err(M4aError::TableTooLarge("stsc"));
    }

    let mut entries = Vec::with_capacity(count);
    let mut off = payload + 4;
    for _ in 0..count {
        entries.push(SampleToChunk {
            first_chunk: be_u32(bytes, off)?,
            samples_per_chunk: be_u32(bytes, off + 4)?,
            sample_description_index: be_u32(bytes, off + 8)?,
        });
        off += 12;
    }
    if off > end {
        return Err(M4aError::Truncated);
    }
    Ok(entries)
}

fn parse_stsz(bytes: &[u8], start: usize, end: usize) -> Result<SampleSizes, M4aError> {
    let payload = full_box_payload(bytes, start, end, "stsz")?;
    let default_size = be_u32(bytes, payload)?;
    let sample_count = usize_from_u32(be_u32(bytes, payload + 4)?)?;
    if sample_count > MAX_SAMPLE_COUNT {
        return Err(M4aError::TableTooLarge("stsz"));
    }

    let mut sizes = Vec::new();
    if default_size == 0 {
        sizes = Vec::with_capacity(sample_count);
        let mut off = payload + 8;
        for _ in 0..sample_count {
            sizes.push(be_u32(bytes, off)?);
            off += 4;
        }
        if off > end {
            return Err(M4aError::Truncated);
        }
    }

    Ok(SampleSizes {
        default_size,
        sample_count,
        sizes,
    })
}

fn parse_stco(bytes: &[u8], start: usize, end: usize) -> Result<Vec<u64>, M4aError> {
    let payload = full_box_payload(bytes, start, end, "stco")?;
    let count = usize_from_u32(be_u32(bytes, payload)?)?;
    if count > MAX_CHUNK_OFFSETS {
        return Err(M4aError::TableTooLarge("stco"));
    }

    let mut offsets = Vec::with_capacity(count);
    let mut off = payload + 4;
    for _ in 0..count {
        offsets.push(u64::from(be_u32(bytes, off)?));
        off += 4;
    }
    if off > end {
        return Err(M4aError::Truncated);
    }
    Ok(offsets)
}

fn parse_co64(bytes: &[u8], start: usize, end: usize) -> Result<Vec<u64>, M4aError> {
    let payload = full_box_payload(bytes, start, end, "co64")?;
    let count = usize_from_u32(be_u32(bytes, payload)?)?;
    if count > MAX_CHUNK_OFFSETS {
        return Err(M4aError::TableTooLarge("co64"));
    }

    let mut offsets = Vec::with_capacity(count);
    let mut off = payload + 4;
    for _ in 0..count {
        offsets.push(be_u64(bytes, off)?);
        off += 8;
    }
    if off > end {
        return Err(M4aError::Truncated);
    }
    Ok(offsets)
}

fn build_packets(bytes: &[u8], tables: &TrackTables) -> Result<Vec<PacketRange>, M4aError> {
    let sizes = tables
        .sample_sizes
        .as_ref()
        .ok_or(M4aError::MissingSampleSizes)?;
    if tables.stsc.is_empty() {
        return Err(M4aError::MissingSampleToChunk);
    }
    if tables.chunk_offsets.is_empty() && sizes.sample_count != 0 {
        return Err(M4aError::MissingChunkOffsets);
    }

    let mut packets = Vec::with_capacity(sizes.sample_count);
    let mut sample_index = 0usize;
    let mut stsc_index = 0usize;
    let mut time_state = TimeState::new(&tables.stts);

    for (chunk_idx, chunk_offset) in tables.chunk_offsets.iter().enumerate() {
        let chunk_number = u32::try_from(chunk_idx + 1).map_err(|_| M4aError::RangeOverflow)?;
        while stsc_index + 1 < tables.stsc.len()
            && tables.stsc[stsc_index + 1].first_chunk <= chunk_number
        {
            stsc_index += 1;
        }

        let entry = tables.stsc[stsc_index];
        if entry.first_chunk == 0
            || entry.samples_per_chunk == 0
            || entry.sample_description_index == 0
        {
            return Err(M4aError::InvalidData("invalid stsc entry"));
        }

        let mut packet_offset = usize_from_u64(*chunk_offset)?;
        for _ in 0..entry.samples_per_chunk {
            if sample_index >= sizes.sample_count {
                break;
            }

            let len = usize_from_u32(sizes.size_at(sample_index)?)?;
            let packet_end = packet_offset
                .checked_add(len)
                .ok_or(M4aError::RangeOverflow)?;
            if packet_end > bytes.len() {
                return Err(M4aError::InvalidData("packet outside source slice"));
            }

            let (timestamp, duration) = time_state.next();
            packets.push(PacketRange {
                offset: packet_offset,
                len,
                timestamp,
                duration,
            });
            packet_offset = packet_end;
            sample_index += 1;
        }
    }

    if sample_index != sizes.sample_count {
        return Err(M4aError::InvalidData("sample tables do not cover all samples"));
    }

    Ok(packets)
}

impl SampleSizes {
    fn size_at(&self, index: usize) -> Result<u32, M4aError> {
        if index >= self.sample_count {
            return Err(M4aError::InvalidData("sample index out of range"));
        }
        if self.default_size != 0 {
            Ok(self.default_size)
        } else {
            self.sizes
                .get(index)
                .copied()
                .ok_or(M4aError::InvalidData("missing sample size"))
        }
    }
}

struct TimeState<'a> {
    entries: &'a [TimeToSample],
    index: usize,
    remaining: u32,
    timestamp: u64,
}

impl<'a> TimeState<'a> {
    fn new(entries: &'a [TimeToSample]) -> Self {
        let remaining = entries.first().map(|entry| entry.sample_count).unwrap_or(0);
        Self {
            entries,
            index: 0,
            remaining,
            timestamp: 0,
        }
    }

    fn next(&mut self) -> (Option<u64>, Option<u32>) {
        if self.entries.is_empty() {
            return (None, None);
        }

        while self.index < self.entries.len() && self.remaining == 0 {
            self.index += 1;
            self.remaining = self
                .entries
                .get(self.index)
                .map(|entry| entry.sample_count)
                .unwrap_or(0);
        }

        let entry = match self.entries.get(self.index) {
            Some(entry) => *entry,
            None => return (Some(self.timestamp), None),
        };
        let timestamp = self.timestamp;
        self.timestamp = self.timestamp.saturating_add(u64::from(entry.sample_delta));
        self.remaining = self.remaining.saturating_sub(1);
        (Some(timestamp), Some(entry.sample_delta))
    }
}

fn parse_aac_audio_specific_config(bytes: &[u8]) -> Option<AacAudioSpecificConfig> {
    let mut bits = BitReader::new(bytes);
    let mut audio_object_type = bits.read(5)? as u8;
    if audio_object_type == 31 {
        audio_object_type = 32 + bits.read(6)? as u8;
    }

    let frequency_index = bits.read(4)? as u8;
    let sampling_frequency = if frequency_index == 15 {
        Some(bits.read(24)?)
    } else {
        aac_sampling_frequency(frequency_index)
    };
    let channel_configuration = bits.read(4)? as u8;

    Some(AacAudioSpecificConfig {
        audio_object_type,
        sampling_frequency,
        channel_configuration,
    })
}

fn aac_sampling_frequency(index: u8) -> Option<u32> {
    match index {
        0 => Some(96_000),
        1 => Some(88_200),
        2 => Some(64_000),
        3 => Some(48_000),
        4 => Some(44_100),
        5 => Some(32_000),
        6 => Some(24_000),
        7 => Some(22_050),
        8 => Some(16_000),
        9 => Some(12_000),
        10 => Some(11_025),
        11 => Some(8_000),
        12 => Some(7_350),
        _ => None,
    }
}

fn aac_channel_count(channel_config: u8) -> Option<u16> {
    match channel_config {
        1 => Some(1),
        2 => Some(2),
        3 => Some(3),
        4 => Some(4),
        5 => Some(5),
        6 => Some(6),
        7 => Some(8),
        _ => None,
    }
}

fn nonzero_u16(value: u16) -> Option<u16> {
    if value == 0 { None } else { Some(value) }
}

fn nonzero_u32(value: u32) -> Option<u32> {
    if value == 0 { None } else { Some(value) }
}

struct BitReader<'a> {
    bytes: &'a [u8],
    bit_pos: usize,
}

impl<'a> BitReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, bit_pos: 0 }
    }

    fn read(&mut self, count: usize) -> Option<u32> {
        if count > 32 || self.bit_pos.checked_add(count)? > self.bytes.len() * 8 {
            return None;
        }

        let mut value = 0u32;
        for _ in 0..count {
            let byte = self.bytes[self.bit_pos / 8];
            let shift = 7 - (self.bit_pos % 8);
            value = (value << 1) | u32::from((byte >> shift) & 1);
            self.bit_pos += 1;
        }
        Some(value)
    }
}

fn full_box_payload(
    bytes: &[u8],
    start: usize,
    end: usize,
    name: &'static str,
) -> Result<usize, M4aError> {
    if start + 4 > end {
        return Err(M4aError::Truncated);
    }
    let version = bytes[start];
    if version != 0 {
        return Err(M4aError::UnsupportedVersion(name));
    }
    Ok(start + 4)
}

fn read_box(bytes: &[u8], start: usize, parent_end: usize) -> Result<BoxHeader, M4aError> {
    if start + 8 > parent_end || parent_end > bytes.len() {
        return Err(M4aError::Truncated);
    }

    let size32 = be_u32(bytes, start)?;
    let typ = fourcc(bytes, start + 4)?;
    let mut header_len = 8usize;
    let end = match size32 {
        0 => parent_end,
        1 => {
            header_len = 16;
            let large = be_u64(bytes, start + 8)?;
            start
                .checked_add(usize_from_u64(large)?)
                .ok_or(M4aError::RangeOverflow)?
        }
        size => start
            .checked_add(usize_from_u32(size)?)
            .ok_or(M4aError::RangeOverflow)?,
    };

    if &typ == b"uuid" {
        header_len = header_len.checked_add(16).ok_or(M4aError::RangeOverflow)?;
    }
    let data_start = start
        .checked_add(header_len)
        .ok_or(M4aError::RangeOverflow)?;
    if data_start > end || end > parent_end {
        return Err(M4aError::InvalidBox);
    }

    Ok(BoxHeader {
        typ,
        data_start,
        end,
    })
}

fn fourcc(bytes: &[u8], off: usize) -> Result<[u8; 4], M4aError> {
    let slice = bytes.get(off..off + 4).ok_or(M4aError::Truncated)?;
    Ok([slice[0], slice[1], slice[2], slice[3]])
}

fn be_u16(bytes: &[u8], off: usize) -> Result<u16, M4aError> {
    let slice = bytes.get(off..off + 2).ok_or(M4aError::Truncated)?;
    Ok(u16::from_be_bytes([slice[0], slice[1]]))
}

fn be_u32(bytes: &[u8], off: usize) -> Result<u32, M4aError> {
    let slice = bytes.get(off..off + 4).ok_or(M4aError::Truncated)?;
    Ok(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn be_u64(bytes: &[u8], off: usize) -> Result<u64, M4aError> {
    let slice = bytes.get(off..off + 8).ok_or(M4aError::Truncated)?;
    Ok(u64::from_be_bytes([
        slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6], slice[7],
    ]))
}

fn usize_from_u32(value: u32) -> Result<usize, M4aError> {
    usize::try_from(value).map_err(|_| M4aError::RangeOverflow)
}

fn usize_from_u64(value: u64) -> Result<usize, M4aError> {
    usize::try_from(value).map_err(|_| M4aError::RangeOverflow)
}
