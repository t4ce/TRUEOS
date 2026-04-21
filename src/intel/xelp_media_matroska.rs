use alloc::vec::Vec;

use super::xelp_media_mp4::{AvccSummary, parse_avcc_summary};
use super::xelp_media_source::MediaSource;

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

#[derive(Default)]
struct OwnedTrackCandidate {
    track_number: Option<u64>,
    codec_id: Option<Vec<u8>>,
    codec_private: Option<Vec<u8>>,
    width: Option<u16>,
    height: Option<u16>,
}

#[derive(Clone, Copy)]
pub(crate) struct SampleRange {
    pub offset: u64,
    pub len: u32,
}

pub(crate) struct MatroskaH264Summary {
    pub width: u16,
    pub height: u16,
    pub sample_count: u32,
    pub first_sample: Vec<u8>,
    pub avcc: AvccSummary,
    pub samples: Vec<SampleRange>,
    sample_count_known: bool,
    source_track_number: Option<u64>,
    source_segment_cursor: u64,
    source_segment_end: u64,
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
    base_offset: u64,
) -> Result<(), MatroskaParseError> {
    let mut off = 0usize;
    while let Some(child) = next_ebml_elem(bytes, &mut off) {
        let child = child?;
        match child.id {
            SIMPLE_BLOCK_ID => {
                if let Some(payload) = parse_block_payload(child.data, wanted_track)? {
                    let offset = base_offset
                        .checked_add(child.data_offset as u64)
                        .and_then(|o| {
                            o.checked_add(
                                (payload.as_ptr() as usize - child.data.as_ptr() as usize) as u64,
                            )
                        })
                        .ok_or(MatroskaParseError::BadBlock)?;
                    samples.push(SampleRange {
                        offset,
                        len: u32::try_from(payload.len())
                            .map_err(|_| MatroskaParseError::BadBlock)?,
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
                            .checked_add(child.data_offset as u64)
                            .and_then(|o| o.checked_add(group_child.data_offset as u64))
                            .and_then(|o| {
                                o.checked_add(
                                    (payload.as_ptr() as usize - group_child.data.as_ptr() as usize)
                                        as u64,
                                )
                            })
                            .ok_or(MatroskaParseError::BadBlock)?;
                        samples.push(SampleRange {
                            offset,
                            len: u32::try_from(payload.len())
                                .map_err(|_| MatroskaParseError::BadBlock)?,
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

pub(crate) fn parse_h264_matroska_summary(
    bytes: &[u8],
) -> Result<MatroskaH264Summary, MatroskaParseError> {
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
                        collect_cluster_samples(
                            child.data,
                            track_no,
                            &mut samples,
                            cluster_base as u64,
                        )?;
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
        .get(
            usize::try_from(first.offset).map_err(|_| MatroskaParseError::BadBlock)?
                ..usize::try_from(first.offset)
                    .map_err(|_| MatroskaParseError::BadBlock)?
                    .saturating_add(
                        usize::try_from(first.len).map_err(|_| MatroskaParseError::BadBlock)?,
                    ),
        )
        .ok_or(MatroskaParseError::BadBlock)?;

    Ok(MatroskaH264Summary {
        width,
        height,
        sample_count: u32::try_from(samples.len()).unwrap_or(u32::MAX),
        first_sample: first_sample.to_vec(),
        avcc,
        samples,
        sample_count_known: true,
        source_track_number: None,
        source_segment_cursor: 0,
        source_segment_end: 0,
    })
}

struct SourceEbmlHeader {
    id: u32,
    data_offset: u64,
    data_len: u64,
    end_offset: u64,
}

async fn read_source_range_exact(
    source: &MediaSource,
    offset: u64,
    len: usize,
) -> Result<Vec<u8>, MatroskaParseError> {
    let mut out = Vec::with_capacity(len);
    out.resize(len, 0);
    if !source
        .read_range_into(offset, out.as_mut_slice())
        .await
        .map_err(|_| MatroskaParseError::Truncated)?
    {
        return Err(MatroskaParseError::Truncated);
    }
    Ok(out)
}

async fn read_source_ebml_header(
    source: &MediaSource,
    off: u64,
    total_len: u64,
) -> Result<Option<SourceEbmlHeader>, MatroskaParseError> {
    if off >= total_len {
        return Ok(None);
    }
    let header_len =
        usize::try_from((total_len - off).min(12)).map_err(|_| MatroskaParseError::Truncated)?;
    let mut header = [0u8; 12];
    if !source
        .read_range_into(off, &mut header[..header_len])
        .await
        .map_err(|_| MatroskaParseError::Truncated)?
    {
        return Err(MatroskaParseError::Truncated);
    }

    let (id, id_len) =
        read_ebml_id(&header[..header_len], 0).ok_or(MatroskaParseError::Truncated)?;
    let (size, size_len) =
        read_ebml_size(&header[..header_len], id_len).ok_or(MatroskaParseError::Truncated)?;
    let data_offset = off
        .checked_add((id_len + size_len) as u64)
        .ok_or(MatroskaParseError::Truncated)?;
    let data_len = size.unwrap_or_else(|| total_len.saturating_sub(data_offset));
    let end_offset = data_offset
        .checked_add(data_len)
        .ok_or(MatroskaParseError::Truncated)?;
    if end_offset > total_len {
        return Err(MatroskaParseError::Truncated);
    }

    Ok(Some(SourceEbmlHeader {
        id,
        data_offset,
        data_len,
        end_offset,
    }))
}

async fn extend_source_sample_index_until(
    summary: &mut MatroskaH264Summary,
    source: &MediaSource,
    wanted_index: usize,
) -> Result<(), MatroskaParseError> {
    if summary.sample_count_known || summary.samples.len() > wanted_index {
        return Ok(());
    }

    let Some(track_no) = summary.source_track_number else {
        return Ok(());
    };

    while summary.samples.len() <= wanted_index
        && summary.source_segment_cursor < summary.source_segment_end
    {
        let Some(child) = read_source_ebml_header(
            source,
            summary.source_segment_cursor,
            summary.source_segment_end,
        )
        .await?
        else {
            break;
        };
        summary.source_segment_cursor = child.end_offset;
        if child.id != CLUSTER_ID {
            continue;
        }

        let cluster_len =
            usize::try_from(child.data_len).map_err(|_| MatroskaParseError::Truncated)?;
        let cluster = read_source_range_exact(source, child.data_offset, cluster_len).await?;
        collect_cluster_samples(
            cluster.as_slice(),
            track_no,
            &mut summary.samples,
            child.data_offset,
        )?;
    }

    if summary.source_segment_cursor >= summary.source_segment_end {
        summary.sample_count = u32::try_from(summary.samples.len()).unwrap_or(u32::MAX);
        summary.sample_count_known = true;
    }

    Ok(())
}

pub(crate) async fn parse_h264_matroska_summary_from_source(
    source: &MediaSource,
) -> Result<MatroskaH264Summary, MatroskaParseError> {
    if let Some(bytes) = source.body() {
        return parse_h264_matroska_summary(bytes);
    }

    let total_len = source.total_len();
    let mut off = 0u64;
    let mut saw_segment = false;
    let mut segment = None;
    while let Some(root) = read_source_ebml_header(source, off, total_len).await? {
        if root.id == SEGMENT_ID {
            saw_segment = true;
            segment = Some(root);
            break;
        }
        off = root.end_offset;
    }

    if !saw_segment {
        return Err(MatroskaParseError::NoSegment);
    }
    let segment = segment.ok_or(MatroskaParseError::NoSegment)?;

    let mut best_track = None;
    let mut track_scan_cursor = segment.data_offset;
    let mut seg_off = segment.data_offset;
    while seg_off < segment.end_offset {
        let Some(child) = read_source_ebml_header(source, seg_off, segment.end_offset).await?
        else {
            break;
        };
        track_scan_cursor = child.end_offset;
        if child.id == TRACKS_ID && best_track.is_none() {
            let tracks_len =
                usize::try_from(child.data_len).map_err(|_| MatroskaParseError::Truncated)?;
            let tracks = read_source_range_exact(source, child.data_offset, tracks_len).await?;
            best_track = parse_tracks(tracks.as_slice())?.map(|track| OwnedTrackCandidate {
                track_number: track.track_number,
                codec_id: track.codec_id.map(|value| value.as_bytes().to_vec()),
                codec_private: track.codec_private.map(|value| value.to_vec()),
                width: track.width,
                height: track.height,
            });
            break;
        }
        seg_off = child.end_offset;
    }

    let track = best_track.ok_or(MatroskaParseError::NoVideoTrack)?;
    if track.codec_id.as_deref() != Some(b"V_MPEG4/ISO/AVC".as_slice()) {
        return Err(MatroskaParseError::UnsupportedCodec);
    }
    let track_no = track.track_number.ok_or(MatroskaParseError::NoVideoTrack)?;

    let mut summary = MatroskaH264Summary {
        width: track.width.ok_or(MatroskaParseError::NoDimensions)?,
        height: track.height.ok_or(MatroskaParseError::NoDimensions)?,
        sample_count: 0,
        first_sample: Vec::new(),
        avcc: parse_avcc_summary(
            track
                .codec_private
                .as_deref()
                .ok_or(MatroskaParseError::NoCodecPrivate)?,
        )
        .ok_or(MatroskaParseError::BadCodecPrivate)?,
        samples: Vec::new(),
        sample_count_known: false,
        source_track_number: Some(track_no),
        source_segment_cursor: track_scan_cursor,
        source_segment_end: segment.end_offset,
    };

    extend_source_sample_index_until(&mut summary, source, 0).await?;

    let first = *summary
        .samples
        .first()
        .ok_or(MatroskaParseError::NoSamples)?;
    summary.first_sample = read_source_range_exact(
        source,
        first.offset,
        usize::try_from(first.len).map_err(|_| MatroskaParseError::BadBlock)?,
    )
    .await
    .map_err(|_| MatroskaParseError::BadBlock)?;

    Ok(summary)
}

pub(crate) fn get_sample_range(summary: &MatroskaH264Summary, index: u32) -> Option<SampleRange> {
    summary.samples.get(index as usize).copied()
}

pub(crate) fn sample_count_known(summary: &MatroskaH264Summary) -> bool {
    summary.sample_count_known
}

pub(crate) async fn ensure_sample_index(
    summary: &mut MatroskaH264Summary,
    source: &MediaSource,
    index: u32,
) -> Result<(), MatroskaParseError> {
    extend_source_sample_index_until(summary, source, index as usize).await
}
