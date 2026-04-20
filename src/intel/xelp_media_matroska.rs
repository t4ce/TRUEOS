use alloc::vec::Vec;

use super::xelp_media_mp4::{AvccSummary, parse_avcc_summary};

const EBML_ID: u32 = 0x1A45_DFA3;
const SEGMENT_ID: u32 = 0x1853_8067;
const TRACKS_ID: u32 = 0x1654_AE6B;
const TRACK_ENTRY_ID: u32 = 0xAE;
const TRACK_NUMBER_ID: u32 = 0xD7;
const TRACK_TYPE_ID: u32 = 0x83;
const CODEC_ID_ID: u32 = 0x86;
const CODEC_PRIVATE_ID: u32 = 0x63A2;
const VIDEO_ID: u32 = 0xE0;
const PIXEL_WIDTH_ID: u32 = 0xB0;
const PIXEL_HEIGHT_ID: u32 = 0xBA;
const CLUSTER_ID: u32 = 0x1F43_B675;
const SIMPLE_BLOCK_ID: u32 = 0xA3;
const BLOCK_GROUP_ID: u32 = 0xA0;
const BLOCK_ID: u32 = 0xA1;

#[derive(Debug, Clone, Copy)]
pub(crate) enum MatroskaParseError {
    Truncated,
    NoSegment,
    NoVideoTrack,
    UnsupportedCodec,
    NoCodecPrivate,
    BadCodecPrivate,
    NoDimensions,
    NoSamples,
    BadBlock,
    UnsupportedLacing,
}

#[derive(Clone, Copy)]
struct EbmlElem<'a> {
    id: u32,
    data: &'a [u8],
    data_offset: usize,
}

#[derive(Default)]
struct TrackCandidate<'a> {
    track_number: Option<u64>,
    track_type: Option<u64>,
    codec_id: Option<&'a str>,
    codec_private: Option<&'a [u8]>,
    width: Option<u16>,
    height: Option<u16>,
}

#[derive(Clone, Copy)]
struct SampleRange {
    offset: usize,
    len: usize,
}

pub(crate) struct MatroskaH264Summary<'a> {
    pub width: u16,
    pub height: u16,
    pub sample_count: u32,
    pub first_sample: &'a [u8],
    pub avcc: AvccSummary<'a>,
    body: &'a [u8],
    samples: Vec<SampleRange>,
}

fn ebml_vint_len(first: u8, max_len: usize) -> Option<usize> {
    let mut mask = 0x80u8;
    let mut len = 1usize;
    while len <= max_len {
        if first & mask != 0 {
            return Some(len);
        }
        mask >>= 1;
        len += 1;
    }
    None
}

fn read_ebml_id(bytes: &[u8], off: usize) -> Option<(u32, usize)> {
    let first = *bytes.get(off)?;
    let len = ebml_vint_len(first, 4)?;
    let raw = bytes.get(off..off + len)?;
    let mut id = 0u32;
    for &b in raw {
        id = (id << 8) | u32::from(b);
    }
    Some((id, len))
}

fn read_ebml_size(bytes: &[u8], off: usize) -> Option<(Option<u64>, usize)> {
    let first = *bytes.get(off)?;
    let len = ebml_vint_len(first, 8)?;
    let raw = bytes.get(off..off + len)?;
    let mut value = u64::from(raw[0] & ((1u8 << (8 - len)) - 1));
    for &b in &raw[1..] {
        value = (value << 8) | u64::from(b);
    }

    let unknown = if len < 8 {
        value == ((1u64 << (7 * len)) - 1)
    } else {
        value == u64::MAX
    };
    Some((if unknown { None } else { Some(value) }, len))
}

fn next_ebml_elem<'a>(
    bytes: &'a [u8],
    off: &mut usize,
) -> Option<Result<EbmlElem<'a>, MatroskaParseError>> {
    if *off >= bytes.len() {
        return None;
    }
    let start = *off;
    let (id, id_len) = read_ebml_id(bytes, start)
        .ok_or(MatroskaParseError::Truncated)
        .ok()?;
    let (size, size_len) = read_ebml_size(bytes, start + id_len)
        .ok_or(MatroskaParseError::Truncated)
        .ok()?;
    let data_offset = start.checked_add(id_len)?.checked_add(size_len)?;
    let payload_len = match size {
        Some(v) => usize::try_from(v)
            .ok()
            .ok_or(MatroskaParseError::Truncated)
            .ok()?,
        None => bytes.len().saturating_sub(data_offset),
    };
    let end = data_offset.checked_add(payload_len)?;
    if end > bytes.len() {
        return Some(Err(MatroskaParseError::Truncated));
    }
    *off = end;
    Some(Ok(EbmlElem {
        id,
        data: &bytes[data_offset..end],
        data_offset,
    }))
}

fn parse_uint(bytes: &[u8]) -> Option<u64> {
    if bytes.is_empty() || bytes.len() > 8 {
        return None;
    }
    let mut out = 0u64;
    for &b in bytes {
        out = (out << 8) | u64::from(b);
    }
    Some(out)
}

fn parse_utf8(bytes: &[u8]) -> Option<&str> {
    core::str::from_utf8(bytes).ok()
}

fn parse_video_info(
    bytes: &[u8],
    track: &mut TrackCandidate<'_>,
) -> Result<(), MatroskaParseError> {
    let mut off = 0usize;
    while let Some(child) = next_ebml_elem(bytes, &mut off) {
        let child = child?;
        match child.id {
            PIXEL_WIDTH_ID => {
                track.width = parse_uint(child.data).and_then(|v| u16::try_from(v).ok());
            }
            PIXEL_HEIGHT_ID => {
                track.height = parse_uint(child.data).and_then(|v| u16::try_from(v).ok());
            }
            _ => {}
        }
    }
    Ok(())
}

fn parse_track_entry<'a>(bytes: &'a [u8]) -> Result<TrackCandidate<'a>, MatroskaParseError> {
    let mut track = TrackCandidate::default();
    let mut off = 0usize;
    while let Some(child) = next_ebml_elem(bytes, &mut off) {
        let child = child?;
        match child.id {
            TRACK_NUMBER_ID => track.track_number = parse_uint(child.data),
            TRACK_TYPE_ID => track.track_type = parse_uint(child.data),
            CODEC_ID_ID => track.codec_id = parse_utf8(child.data),
            CODEC_PRIVATE_ID => track.codec_private = Some(child.data),
            VIDEO_ID => parse_video_info(child.data, &mut track)?,
            _ => {}
        }
    }
    Ok(track)
}

fn parse_tracks<'a>(bytes: &'a [u8]) -> Result<Option<TrackCandidate<'a>>, MatroskaParseError> {
    let mut off = 0usize;
    let mut fallback_video = None;
    while let Some(child) = next_ebml_elem(bytes, &mut off) {
        let child = child?;
        if child.id != TRACK_ENTRY_ID {
            continue;
        }
        let track = parse_track_entry(child.data)?;
        if track.track_type != Some(1) {
            continue;
        }
        if track.codec_id == Some("V_MPEG4/ISO/AVC") {
            return Ok(Some(track));
        }
        if fallback_video.is_none() {
            fallback_video = Some(track);
        }
    }
    Ok(fallback_video)
}

fn parse_block_payload<'a>(
    block: &'a [u8],
    wanted_track: u64,
) -> Result<Option<&'a [u8]>, MatroskaParseError> {
    let Some(first) = block.first().copied() else {
        return Err(MatroskaParseError::BadBlock);
    };
    let Some(track_len) = ebml_vint_len(first, 8) else {
        return Err(MatroskaParseError::BadBlock);
    };
    if block.len() < track_len + 3 {
        return Err(MatroskaParseError::BadBlock);
    }

    let mut track_no = u64::from(first & ((1u8 << (8 - track_len)) - 1));
    for &b in &block[1..track_len] {
        track_no = (track_no << 8) | u64::from(b);
    }
    if track_no != wanted_track {
        return Ok(None);
    }

    let flags = block[track_len + 2];
    let lacing = (flags >> 1) & 0x03;
    if lacing != 0 {
        return Err(MatroskaParseError::UnsupportedLacing);
    }

    Ok(Some(&block[track_len + 3..]))
}

fn collect_cluster_samples(
    bytes: &[u8],
    wanted_track: u64,
    samples: &mut Vec<SampleRange>,
    base_offset: usize,
) -> Result<(), MatroskaParseError> {
    let mut off = 0usize;
    while let Some(child) = next_ebml_elem(bytes, &mut off) {
        let child = child?;
        match child.id {
            SIMPLE_BLOCK_ID => {
                if let Some(payload) = parse_block_payload(child.data, wanted_track)? {
                    let offset = base_offset
                        .checked_add(child.data_offset)
                        .and_then(|o| {
                            o.checked_add(payload.as_ptr() as usize - child.data.as_ptr() as usize)
                        })
                        .ok_or(MatroskaParseError::BadBlock)?;
                    samples.push(SampleRange {
                        offset,
                        len: payload.len(),
                    });
                }
            }
            BLOCK_GROUP_ID => {
                let mut group_off = 0usize;
                while let Some(group_child) = next_ebml_elem(child.data, &mut group_off) {
                    let group_child = group_child?;
                    if group_child.id != BLOCK_ID {
                        continue;
                    }
                    if let Some(payload) = parse_block_payload(group_child.data, wanted_track)? {
                        let offset = base_offset
                            .checked_add(child.data_offset)
                            .and_then(|o| o.checked_add(group_child.data_offset))
                            .and_then(|o| {
                                o.checked_add(
                                    payload.as_ptr() as usize - group_child.data.as_ptr() as usize,
                                )
                            })
                            .ok_or(MatroskaParseError::BadBlock)?;
                        samples.push(SampleRange {
                            offset,
                            len: payload.len(),
                        });
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

pub(crate) fn looks_like_matroska(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3])
}

pub(crate) fn parse_h264_matroska_summary<'a>(
    bytes: &'a [u8],
) -> Result<MatroskaH264Summary<'a>, MatroskaParseError> {
    let mut off = 0usize;
    let mut saw_segment = false;
    let mut best_track = None;
    let mut samples = Vec::new();

    while let Some(root) = next_ebml_elem(bytes, &mut off) {
        let root = root?;
        match root.id {
            EBML_ID => {}
            SEGMENT_ID => {
                saw_segment = true;
                let mut seg_off = 0usize;
                while let Some(child) = next_ebml_elem(root.data, &mut seg_off) {
                    let child = child?;
                    if child.id == TRACKS_ID && best_track.is_none() {
                        best_track = parse_tracks(child.data)?;
                    }
                }
                let track_no = best_track
                    .as_ref()
                    .and_then(|track| track.track_number)
                    .ok_or(MatroskaParseError::NoVideoTrack)?;
                seg_off = 0;
                while let Some(child) = next_ebml_elem(root.data, &mut seg_off) {
                    let child = child?;
                    if child.id == CLUSTER_ID {
                        let cluster_base = root
                            .data_offset
                            .checked_add(child.data_offset)
                            .ok_or(MatroskaParseError::Truncated)?;
                        collect_cluster_samples(child.data, track_no, &mut samples, cluster_base)?;
                    }
                }
            }
            _ => {}
        }
    }

    if !saw_segment {
        return Err(MatroskaParseError::NoSegment);
    }

    let track = best_track.ok_or(MatroskaParseError::NoVideoTrack)?;
    if track.codec_id != Some("V_MPEG4/ISO/AVC") {
        return Err(MatroskaParseError::UnsupportedCodec);
    }
    let codec_private = track
        .codec_private
        .ok_or(MatroskaParseError::NoCodecPrivate)?;
    let avcc = parse_avcc_summary(codec_private).ok_or(MatroskaParseError::BadCodecPrivate)?;
    let width = track.width.ok_or(MatroskaParseError::NoDimensions)?;
    let height = track.height.ok_or(MatroskaParseError::NoDimensions)?;
    let first = *samples.first().ok_or(MatroskaParseError::NoSamples)?;
    let first_sample = bytes
        .get(first.offset..first.offset.saturating_add(first.len))
        .ok_or(MatroskaParseError::BadBlock)?;

    Ok(MatroskaH264Summary {
        width,
        height,
        sample_count: u32::try_from(samples.len()).unwrap_or(u32::MAX),
        first_sample,
        avcc,
        body: bytes,
        samples,
    })
}

pub(crate) fn get_sample_data<'a>(
    summary: &'a MatroskaH264Summary<'a>,
    index: u32,
) -> Option<&'a [u8]> {
    let range = *summary.samples.get(index as usize)?;
    summary
        .body
        .get(range.offset..range.offset.checked_add(range.len)?)
}
