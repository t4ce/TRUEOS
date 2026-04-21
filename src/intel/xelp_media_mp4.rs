extern crate alloc;

use alloc::vec::Vec;

use super::xelp_media_source::MediaSource;

#[derive(Debug, Clone, Copy)]
pub(crate) enum Mp4ParseError {
    Truncated,
    NoMoov,
    NoVideoTrack,
    NoAvc1,
    NoAvcc,
    NoSamples,
    NoChunkOffsets,
    BadSampleRange,
    BadNalLengthSize,
    BadNalRange,
    OutputTooSmall,
    EmptyAccessUnit,
}

#[derive(Debug, Clone)]
pub(crate) struct AvccSummary {
    pub nal_length_size: usize,
    pub profile_idc: u8,
    pub level_idc: u8,
    pub sps: Vec<u8>,
    pub pps: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SampleRange {
    pub offset: u64,
    pub len: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct Mp4H264Summary {
    pub width: u16,
    pub height: u16,
    pub timescale: u32,
    pub duration: u64,
    pub sample_count: u32,
    pub first_chunk_offset: u64,
    pub first_sample_size: u32,
    pub first_sample: Vec<u8>,
    pub avcc: AvccSummary,
    pub sample_ranges: Vec<SampleRange>,
}

pub(crate) struct AnnexBAccessUnit {
    pub bytes_written: usize,
    pub sample_nal_count: usize,
    pub has_idr: bool,
    pub idr_nal_offset: usize,
    pub idr_nal_length: usize,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct H264VclInfo {
    pub nal_type: u8,
    pub nal_ref_idc: u8,
    pub slice_type: u32,
    pub frame_num: u32,
    pub first_mb_in_slice: u32,
    pub pic_order_cnt_lsb: u32,
    pub cabac_init_idc: u32,
    pub slice_qp_delta: i32,
    pub disable_deblocking_filter_idc: u32,
    pub slice_alpha_c0_offset_div2: i32,
    pub slice_beta_offset_div2: i32,
    pub num_ref_idx_l0_active_minus1: u32,
    pub num_ref_idx_l1_active_minus1: u32,
    pub slice_header_bit_offset: u32,
}

#[derive(Clone, Copy)]
struct BoxHeader<'a> {
    kind: &'a [u8; 4],
    data: &'a [u8],
}

#[derive(Default)]
struct TrackCandidate<'a> {
    is_video: bool,
    width: Option<u16>,
    height: Option<u16>,
    timescale: Option<u32>,
    duration: Option<u64>,
    avcc: Option<AvccSummary>,
    sample_count: Option<u32>,
    first_sample_size: Option<u32>,
    first_chunk_offset: Option<u64>,
    stsz_data: Option<&'a [u8]>,
    stsc_data: Option<&'a [u8]>,
    stco_data: Option<&'a [u8]>,
    co64_data: Option<&'a [u8]>,
}

fn be_u16(bytes: &[u8], off: usize) -> Option<u16> {
    let chunk = bytes.get(off..off + 2)?;
    Some(u16::from_be_bytes([chunk[0], chunk[1]]))
}

fn be_u32(bytes: &[u8], off: usize) -> Option<u32> {
    let chunk = bytes.get(off..off + 4)?;
    Some(u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
}

fn be_u64(bytes: &[u8], off: usize) -> Option<u64> {
    let chunk = bytes.get(off..off + 8)?;
    Some(u64::from_be_bytes([
        chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
    ]))
}

fn next_box<'a>(bytes: &'a [u8], off: &mut usize) -> Option<Result<BoxHeader<'a>, Mp4ParseError>> {
    if *off >= bytes.len() {
        return None;
    }
    if bytes.len().saturating_sub(*off) < 8 {
        return Some(Err(Mp4ParseError::Truncated));
    }

    let start = *off;
    let size32 = be_u32(bytes, start)? as u64;
    let kind = bytes.get(start + 4..start + 8)?;
    let kind = <&[u8; 4]>::try_from(kind).ok()?;

    let (header_len, box_size) = if size32 == 1 {
        let size64 = be_u64(bytes, start + 8).ok_or(Mp4ParseError::Truncated);
        match size64 {
            Ok(size64) => (16usize, size64),
            Err(err) => return Some(Err(err)),
        }
    } else if size32 == 0 {
        (8usize, (bytes.len().saturating_sub(start)) as u64)
    } else {
        (8usize, size32)
    };

    if box_size < header_len as u64 {
        return Some(Err(Mp4ParseError::Truncated));
    }
    let end = start.saturating_add(box_size as usize);
    if end > bytes.len() {
        return Some(Err(Mp4ParseError::Truncated));
    }

    let data = &bytes[start + header_len..end];
    *off = end;
    Some(Ok(BoxHeader { kind, data }))
}

fn parse_hdlr(bytes: &[u8]) -> Option<bool> {
    let handler = bytes.get(8..12)?;
    Some(handler == b"vide")
}

fn parse_mdhd(bytes: &[u8]) -> Option<(u32, u64)> {
    let version = *bytes.first()?;
    if version == 1 {
        Some((be_u32(bytes, 20)?, be_u64(bytes, 24)?))
    } else {
        Some((be_u32(bytes, 12)?, be_u32(bytes, 16)? as u64))
    }
}

fn parse_stsz(bytes: &[u8]) -> Option<(u32, u32)> {
    let uniform = be_u32(bytes, 4)?;
    let sample_count = be_u32(bytes, 8)?;
    let first_sample_size = if uniform != 0 {
        uniform
    } else {
        be_u32(bytes, 12)?
    };
    Some((sample_count, first_sample_size))
}

fn parse_stco(bytes: &[u8]) -> Option<u64> {
    let entry_count = be_u32(bytes, 4)?;
    if entry_count == 0 {
        return None;
    }
    Some(be_u32(bytes, 8)? as u64)
}

fn parse_co64(bytes: &[u8]) -> Option<u64> {
    let entry_count = be_u32(bytes, 4)?;
    if entry_count == 0 {
        return None;
    }
    be_u64(bytes, 8)
}

fn table_entry_count(bytes: &[u8]) -> Option<u32> {
    be_u32(bytes, 4)
}

fn parse_stsc_entry(bytes: &[u8], index: u32) -> Option<(u32, u32, u32)> {
    let entry_count = table_entry_count(bytes)?;
    if index >= entry_count {
        return None;
    }
    let off = 8usize.checked_add(index as usize * 12)?;
    Some((be_u32(bytes, off)?, be_u32(bytes, off + 4)?, be_u32(bytes, off + 8)?))
}

fn sample_size_at(stsz: &[u8], index: u32) -> Option<usize> {
    let sample_count = be_u32(stsz, 8)?;
    if index >= sample_count {
        return None;
    }
    let uniform_size = be_u32(stsz, 4)?;
    if uniform_size != 0 {
        Some(uniform_size as usize)
    } else {
        Some(be_u32(stsz, 12 + index as usize * 4)? as usize)
    }
}

fn chunk_offset_at(
    stco_data: Option<&[u8]>,
    co64_data: Option<&[u8]>,
    chunk_index: u32,
) -> Option<u64> {
    if let Some(stco) = stco_data {
        let entry_count = table_entry_count(stco)?;
        if chunk_index >= entry_count {
            return None;
        }
        return Some(be_u32(stco, 8 + chunk_index as usize * 4)? as u64);
    }
    if let Some(co64) = co64_data {
        let entry_count = table_entry_count(co64)?;
        if chunk_index >= entry_count {
            return None;
        }
        return be_u64(co64, 8 + chunk_index as usize * 8);
    }
    None
}

fn sample_file_range(track: &TrackCandidate<'_>, index: u32) -> Option<SampleRange> {
    let sample_count = track.sample_count?;
    if index >= sample_count {
        return None;
    }
    let stsz_data = track.stsz_data?;

    let Some(stsc) = track.stsc_data else {
        let mut file_off = track.first_chunk_offset?;
        let mut i = 0u32;
        while i < index {
            file_off = file_off.checked_add(sample_size_at(stsz_data, i)? as u64)?;
            i += 1;
        }
        let sample_size = sample_size_at(stsz_data, index)?;
        return Some(SampleRange {
            offset: file_off,
            len: u32::try_from(sample_size).ok()?,
        });
    };

    let entry_count = table_entry_count(stsc)?;
    if entry_count == 0 {
        return None;
    }

    let chunk_count = if let Some(stco) = track.stco_data {
        table_entry_count(stco)?
    } else if let Some(co64) = track.co64_data {
        table_entry_count(co64)?
    } else {
        return None;
    };

    let mut sample_base = 0u32;
    let mut entry_index = 0u32;
    while entry_index < entry_count {
        let (first_chunk, samples_per_chunk, _sample_desc) = parse_stsc_entry(stsc, entry_index)?;
        if first_chunk == 0 || samples_per_chunk == 0 {
            return None;
        }
        let next_first_chunk = if entry_index + 1 < entry_count {
            parse_stsc_entry(stsc, entry_index + 1)?.0
        } else {
            chunk_count.checked_add(1)?
        };
        if next_first_chunk <= first_chunk {
            return None;
        }

        let chunk_span = next_first_chunk.checked_sub(first_chunk)?;
        let sample_span = chunk_span.checked_mul(samples_per_chunk)?;
        if index < sample_base.checked_add(sample_span)? {
            let rel_sample = index.checked_sub(sample_base)?;
            let chunk_in_run = rel_sample / samples_per_chunk;
            let sample_in_chunk = rel_sample % samples_per_chunk;
            let chunk_index = first_chunk.checked_sub(1)?.checked_add(chunk_in_run)?;
            let mut file_off = chunk_offset_at(track.stco_data, track.co64_data, chunk_index)?;
            let chunk_sample_start =
                sample_base.checked_add(chunk_in_run.checked_mul(samples_per_chunk)?)?;
            let mut in_chunk = 0u32;
            while in_chunk < sample_in_chunk {
                file_off =
                    file_off.checked_add(sample_size_at(stsz_data, chunk_sample_start + in_chunk)? as u64)?;
                in_chunk += 1;
            }
            let sample_size = sample_size_at(stsz_data, index)?;
            return Some(SampleRange {
                offset: file_off,
                len: u32::try_from(sample_size).ok()?,
            });
        }

        sample_base = sample_base.checked_add(sample_span)?;
        entry_index += 1;
    }

    None
}

fn parse_avcc(bytes: &[u8]) -> Option<AvccSummary> {
    if bytes.len() < 7 {
        return None;
    }
    let profile_idc = bytes[1];
    let level_idc = bytes[3];
    let nal_length_size = ((bytes[4] & 0x03) + 1) as usize;
    let sps_count = (bytes[5] & 0x1F) as usize;
    let mut off = 6usize;
    let mut first_sps = None;
    let mut idx = 0usize;
    while idx < sps_count {
        let len = be_u16(bytes, off)? as usize;
        off = off.saturating_add(2);
        let sps = bytes.get(off..off + len)?;
        if first_sps.is_none() {
            first_sps = Some(sps);
        }
        off = off.saturating_add(len);
        idx += 1;
    }
    let pps_count = *bytes.get(off)? as usize;
    off = off.saturating_add(1);
    let mut first_pps = None;
    idx = 0;
    while idx < pps_count {
        let len = be_u16(bytes, off)? as usize;
        off = off.saturating_add(2);
        let pps = bytes.get(off..off + len)?;
        if first_pps.is_none() {
            first_pps = Some(pps);
        }
        off = off.saturating_add(len);
        idx += 1;
    }

    Some(AvccSummary {
        nal_length_size,
        profile_idc,
        level_idc,
        sps: first_sps?.to_vec(),
        pps: first_pps?.to_vec(),
    })
}

pub(crate) fn parse_avcc_summary(bytes: &[u8]) -> Option<AvccSummary> {
    parse_avcc(bytes)
}

fn parse_stsd(bytes: &[u8]) -> Option<(u16, u16, AvccSummary)> {
    let entry_count = be_u32(bytes, 4)?;
    if entry_count == 0 {
        return None;
    }

    let sample = bytes.get(8..)?;
    let sample_size = be_u32(sample, 0)? as usize;
    if sample_size < 16 || sample_size > sample.len() {
        return None;
    }
    let sample_kind = sample.get(4..8)?;
    if sample_kind != b"avc1" {
        return None;
    }
    let sample_body = sample.get(8..sample_size)?;
    let width = be_u16(sample_body, 24)?;
    let height = be_u16(sample_body, 26)?;
    let mut child_off = 78usize;
    while let Some(child) = next_box(sample_body, &mut child_off) {
        let child = child.ok()?;
        if child.kind == b"avcC" {
            return Some((width, height, parse_avcc(child.data)?));
        }
    }
    None
}

fn parse_stbl<'a>(bytes: &'a [u8], track: &mut TrackCandidate<'a>) -> Result<(), Mp4ParseError> {
    let mut off = 0usize;
    while let Some(child) = next_box(bytes, &mut off) {
        let child = child?;
        match child.kind {
            b"stsd" => {
                if let Some((width, height, avcc)) = parse_stsd(child.data) {
                    track.width = Some(width);
                    track.height = Some(height);
                    track.avcc = Some(avcc);
                }
            }
            b"stsz" => {
                if let Some((sample_count, first_sample_size)) = parse_stsz(child.data) {
                    track.sample_count = Some(sample_count);
                    track.first_sample_size = Some(first_sample_size);
                }
                track.stsz_data = Some(child.data);
            }
            b"stsc" => {
                track.stsc_data = Some(child.data);
            }
            b"stco" => {
                track.first_chunk_offset = parse_stco(child.data);
                track.stco_data = Some(child.data);
            }
            b"co64" => {
                track.first_chunk_offset = parse_co64(child.data);
                track.co64_data = Some(child.data);
            }
            _ => {}
        }
    }
    Ok(())
}

fn parse_mdia<'a>(bytes: &'a [u8], track: &mut TrackCandidate<'a>) -> Result<(), Mp4ParseError> {
    let mut off = 0usize;
    while let Some(child) = next_box(bytes, &mut off) {
        let child = child?;
        match child.kind {
            b"hdlr" => {
                track.is_video = parse_hdlr(child.data).unwrap_or(false);
            }
            b"mdhd" => {
                if let Some((timescale, duration)) = parse_mdhd(child.data) {
                    track.timescale = Some(timescale);
                    track.duration = Some(duration);
                }
            }
            b"minf" => {
                let mut minf_off = 0usize;
                while let Some(minf_child) = next_box(child.data, &mut minf_off) {
                    let minf_child = minf_child?;
                    if minf_child.kind == b"stbl" {
                        parse_stbl(minf_child.data, track)?;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn parse_trak<'a>(bytes: &'a [u8]) -> Result<TrackCandidate<'a>, Mp4ParseError> {
    let mut track = TrackCandidate::default();
    let mut off = 0usize;
    while let Some(child) = next_box(bytes, &mut off) {
        let child = child?;
        if child.kind == b"mdia" {
            parse_mdia(child.data, &mut track)?;
        }
    }
    Ok(track)
}

fn build_sample_ranges(track: &TrackCandidate<'_>) -> Result<Vec<SampleRange>, Mp4ParseError> {
    let sample_count = track.sample_count.ok_or(Mp4ParseError::NoSamples)?;
    let mut ranges = Vec::with_capacity(sample_count as usize);
    let mut index = 0u32;
    while index < sample_count {
        ranges.push(sample_file_range(track, index).ok_or(Mp4ParseError::BadSampleRange)?);
        index += 1;
    }
    Ok(ranges)
}

#[derive(Debug, Clone)]
struct ParsedMp4Track {
    width: u16,
    height: u16,
    timescale: u32,
    duration: u64,
    sample_count: u32,
    first_chunk_offset: u64,
    first_sample_size: u32,
    avcc: AvccSummary,
    sample_ranges: Vec<SampleRange>,
}

fn parse_h264_mp4_track_from_moov(bytes: &[u8]) -> Result<ParsedMp4Track, Mp4ParseError> {
    let mut moov_off = 0usize;
    let mut best_track = None;
    while let Some(child) = next_box(bytes, &mut moov_off) {
        let child = child?;
        if child.kind != b"trak" {
            continue;
        }
        let track = parse_trak(child.data)?;
        if track.is_video && track.avcc.is_some() {
            best_track = Some(track);
            break;
        }
    }

    let track = best_track.ok_or(Mp4ParseError::NoVideoTrack)?;
    let avcc = track.avcc.clone().ok_or(Mp4ParseError::NoAvcc)?;
    let sample_count = track.sample_count.ok_or(Mp4ParseError::NoSamples)?;
    let first_sample_size = track.first_sample_size.ok_or(Mp4ParseError::NoSamples)?;
    let first_chunk_offset = track
        .first_chunk_offset
        .ok_or(Mp4ParseError::NoChunkOffsets)?;
    let sample_ranges = build_sample_ranges(&track)?;

    Ok(ParsedMp4Track {
        width: track.width.ok_or(Mp4ParseError::NoAvc1)?,
        height: track.height.ok_or(Mp4ParseError::NoAvc1)?,
        timescale: track.timescale.unwrap_or(0),
        duration: track.duration.unwrap_or(0),
        sample_count,
        first_chunk_offset,
        first_sample_size,
        avcc,
        sample_ranges,
    })
}

fn build_mp4_summary(parsed: ParsedMp4Track, first_sample: Vec<u8>) -> Mp4H264Summary {
    Mp4H264Summary {
        width: parsed.width,
        height: parsed.height,
        timescale: parsed.timescale,
        duration: parsed.duration,
        sample_count: parsed.sample_count,
        first_chunk_offset: parsed.first_chunk_offset,
        first_sample_size: parsed.first_sample_size,
        first_sample,
        avcc: parsed.avcc,
        sample_ranges: parsed.sample_ranges,
    }
}

fn parse_h264_mp4_summary_from_bytes(bytes: &[u8]) -> Result<Mp4H264Summary, Mp4ParseError> {
    let mut off = 0usize;
    let mut saw_moov = false;

    while let Some(root) = next_box(bytes, &mut off) {
        let root = root?;
        if root.kind != b"moov" {
            continue;
        }
        saw_moov = true;
        let parsed = parse_h264_mp4_track_from_moov(root.data)?;
        let first_range = *parsed.sample_ranges.first().ok_or(Mp4ParseError::NoSamples)?;
        let first_sample_off =
            usize::try_from(first_range.offset).map_err(|_| Mp4ParseError::BadSampleRange)?;
        let first_sample_len =
            usize::try_from(first_range.len).map_err(|_| Mp4ParseError::BadSampleRange)?;
        let first_sample = bytes
            .get(first_sample_off..first_sample_off.saturating_add(first_sample_len))
            .ok_or(Mp4ParseError::BadSampleRange)?
            .to_vec();
        return Ok(build_mp4_summary(parsed, first_sample));
    }

    if !saw_moov {
        return Err(Mp4ParseError::NoMoov);
    }

    Err(Mp4ParseError::NoVideoTrack)
}

pub(crate) fn parse_h264_mp4_summary(bytes: &[u8]) -> Result<Mp4H264Summary, Mp4ParseError> {
    parse_h264_mp4_summary_from_bytes(bytes)
}

async fn read_source_range_exact(
    source: &MediaSource,
    offset: u64,
    len: usize,
) -> Result<Vec<u8>, Mp4ParseError> {
    let mut out = Vec::with_capacity(len);
    out.resize(len, 0);
    if !source
        .read_range_into(offset, out.as_mut_slice())
        .await
        .map_err(|_| Mp4ParseError::Truncated)?
    {
        return Err(Mp4ParseError::Truncated);
    }
    Ok(out)
}

pub(crate) async fn parse_h264_mp4_summary_from_source(
    source: &MediaSource,
) -> Result<Mp4H264Summary, Mp4ParseError> {
    if let Some(bytes) = source.body() {
        return parse_h264_mp4_summary_from_bytes(bytes);
    }

    let total_len = source.total_len();
    let mut off = 0u64;
    while off < total_len {
        let mut header = [0u8; 16];
        if !source
            .read_range_into(off, &mut header)
            .await
            .map_err(|_| Mp4ParseError::Truncated)?
        {
            return Err(Mp4ParseError::Truncated);
        }

        let size32 = be_u32(&header, 0).ok_or(Mp4ParseError::Truncated)? as u64;
        let kind = header.get(4..8).ok_or(Mp4ParseError::Truncated)?;
        let (header_len, box_size) = if size32 == 1 {
            (16u64, be_u64(&header, 8).ok_or(Mp4ParseError::Truncated)?)
        } else if size32 == 0 {
            (8u64, total_len.saturating_sub(off))
        } else {
            (8u64, size32)
        };

        if box_size < header_len {
            return Err(Mp4ParseError::Truncated);
        }

        if kind == b"moov" {
            let payload_len = usize::try_from(box_size - header_len)
                .map_err(|_| Mp4ParseError::Truncated)?;
            let payload_off = off
                .checked_add(header_len)
                .ok_or(Mp4ParseError::Truncated)?;
            let payload = read_source_range_exact(source, payload_off, payload_len).await?;
            let parsed = parse_h264_mp4_track_from_moov(payload.as_slice())?;
            let first_range = *parsed.sample_ranges.first().ok_or(Mp4ParseError::NoSamples)?;
            let first_sample = read_source_range_exact(
                source,
                first_range.offset,
                usize::try_from(first_range.len).map_err(|_| Mp4ParseError::BadSampleRange)?,
            )
            .await
            .map_err(|_| Mp4ParseError::BadSampleRange)?;
            return Ok(build_mp4_summary(parsed, first_sample));
        }

        off = off.checked_add(box_size).ok_or(Mp4ParseError::Truncated)?;
    }

    Err(Mp4ParseError::NoMoov)
}

pub(crate) fn get_sample_range(summary: &Mp4H264Summary, index: u32) -> Option<SampleRange> {
    summary.sample_ranges.get(index as usize).copied()
}

fn push_annex_b_nal(
    out: &mut [u8],
    bytes_written: &mut usize,
    nal: &[u8],
) -> Result<(), Mp4ParseError> {
    let end = bytes_written
        .checked_add(4)
        .and_then(|value| value.checked_add(nal.len()))
        .ok_or(Mp4ParseError::OutputTooSmall)?;
    if end > out.len() {
        return Err(Mp4ParseError::OutputTooSmall);
    }
    out[*bytes_written..*bytes_written + 4].copy_from_slice(&[0x00, 0x00, 0x00, 0x01]);
    *bytes_written += 4;
    out[*bytes_written..*bytes_written + nal.len()].copy_from_slice(nal);
    *bytes_written += nal.len();
    Ok(())
}

/// Write an Annex-B access unit for an arbitrary sample (not just the first).
pub(crate) fn write_annex_b_for_sample(
    sample: &[u8],
    avcc: &AvccSummary,
    out: &mut [u8],
) -> Result<AnnexBAccessUnit, Mp4ParseError> {
    let nal_length_size = avcc.nal_length_size;
    if !(1..=4).contains(&nal_length_size) {
        return Err(Mp4ParseError::BadNalLengthSize);
    }
    let mut bytes_written = 0usize;
    push_annex_b_nal(out, &mut bytes_written, &avcc.sps)?;
    push_annex_b_nal(out, &mut bytes_written, &avcc.pps)?;

    let mut off = 0usize;
    let mut sample_nal_count = 0usize;
    let mut has_idr = false;
    let mut idr_nal_offset = 0usize;
    let mut idr_nal_length = 0usize;
    while off.saturating_add(nal_length_size) <= sample.len() {
        let mut nal_len = 0usize;
        let mut idx = 0usize;
        while idx < nal_length_size {
            nal_len = (nal_len << 8) | sample[off + idx] as usize;
            idx += 1;
        }
        off = off.saturating_add(nal_length_size);
        if nal_len == 0 {
            return Err(Mp4ParseError::BadNalRange);
        }
        let nal = sample
            .get(off..off.saturating_add(nal_len))
            .ok_or(Mp4ParseError::BadNalRange)?;
        let nal_type = nal[0] & 0x1F;
        if nal_type == 5 {
            idr_nal_offset = bytes_written + 4;
            idr_nal_length = nal_len;
        }
        // Also track non-IDR slices (type 1) for P/B frames
        if nal_type == 1 && !has_idr {
            idr_nal_offset = bytes_written + 4;
            idr_nal_length = nal_len;
        }
        has_idr |= nal_type == 5;
        push_annex_b_nal(out, &mut bytes_written, nal)?;
        off = off.saturating_add(nal_len);
        sample_nal_count += 1;
    }
    if sample_nal_count == 0 {
        return Err(Mp4ParseError::EmptyAccessUnit);
    }
    Ok(AnnexBAccessUnit {
        bytes_written,
        sample_nal_count,
        has_idr,
        idr_nal_offset,
        idr_nal_length,
    })
}

pub(crate) fn first_sample_nal_types(
    sample: &[u8],
    nal_length_size: usize,
    max_nals: usize,
) -> Vec<u8> {
    let mut out = Vec::new();
    if !(1..=4).contains(&nal_length_size) {
        return out;
    }

    let mut off = 0usize;
    while off.saturating_add(nal_length_size) <= sample.len() && out.len() < max_nals {
        let mut nal_len = 0usize;
        let mut idx = 0usize;
        while idx < nal_length_size {
            nal_len = (nal_len << 8) | sample[off + idx] as usize;
            idx += 1;
        }
        off = off.saturating_add(nal_length_size);
        if nal_len == 0 || off.saturating_add(nal_len) > sample.len() {
            break;
        }
        out.push(sample[off] & 0x1F);
        off = off.saturating_add(nal_len);
    }
    out
}

// --- H.264 SPS/PPS Exp-Golomb parsing ---

struct BitReader<'a> {
    data: &'a [u8],
    byte_off: usize,
    bit_off: u8,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            byte_off: 0,
            bit_off: 0,
        }
    }

    fn read_bit(&mut self) -> Option<u8> {
        let byte = *self.data.get(self.byte_off)?;
        let bit = (byte >> (7 - self.bit_off)) & 1;
        self.bit_off += 1;
        if self.bit_off >= 8 {
            self.bit_off = 0;
            self.byte_off += 1;
        }
        Some(bit)
    }

    fn read_bits(&mut self, n: u8) -> Option<u32> {
        let mut val = 0u32;
        for _ in 0..n {
            val = (val << 1) | u32::from(self.read_bit()?);
        }
        Some(val)
    }

    fn read_ue(&mut self) -> Option<u32> {
        let mut leading = 0u32;
        while self.read_bit()? == 0 {
            leading += 1;
            if leading > 31 {
                return None;
            }
        }
        if leading == 0 {
            return Some(0);
        }
        let suffix = self.read_bits(leading as u8)?;
        Some((1u32 << leading).wrapping_sub(1).wrapping_add(suffix))
    }

    fn read_se(&mut self) -> Option<i32> {
        let ue = self.read_ue()?;
        let val = ((ue + 1) / 2) as i32;
        if ue & 1 == 0 { Some(-val) } else { Some(val) }
    }
}

fn remove_emulation_prevention(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0usize;
    while i < data.len() {
        if i + 2 < data.len() && data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 3 {
            out.push(0);
            out.push(0);
            i += 3;
        } else {
            out.push(data[i]);
            i += 1;
        }
    }
    out
}

pub(crate) fn parse_sample_vcl_info(
    sample: &[u8],
    nal_length_size: usize,
    sps: &ParsedSps,
    pps: &ParsedPps,
) -> Option<H264VclInfo> {
    if !(1..=4).contains(&nal_length_size) {
        return None;
    }

    let mut off = 0usize;
    while off.saturating_add(nal_length_size) <= sample.len() {
        let mut nal_len = 0usize;
        let mut idx = 0usize;
        while idx < nal_length_size {
            nal_len = (nal_len << 8) | sample[off + idx] as usize;
            idx += 1;
        }
        off = off.saturating_add(nal_length_size);
        if nal_len == 0 || off.saturating_add(nal_len) > sample.len() {
            return None;
        }
        let nal = sample.get(off..off.saturating_add(nal_len))?;
        let nal_type = nal[0] & 0x1F;
        let nal_ref_idc = (nal[0] >> 5) & 0x03;
        if nal_type == 1 || nal_type == 5 {
            let clean = remove_emulation_prevention(nal);
            let mut br = BitReader::new(clean.get(1..)?);
            let first_mb_in_slice = br.read_ue()?;
            let slice_type = br.read_ue()?;
            let _pic_parameter_set_id = br.read_ue()?;
            let frame_num_bits = (sps.log2_max_frame_num_minus4 + 4).min(16) as u8;
            let frame_num = br.read_bits(frame_num_bits)?;
            if !sps.frame_mbs_only_flag {
                let _field_pic_flag = br.read_bit()?;
            }
            if nal_type == 5 {
                let _idr_pic_id = br.read_ue()?;
            }
            let mut pic_order_cnt_lsb = 0u32;
            if sps.pic_order_cnt_type == 0 {
                let poc_bits = (sps.log2_max_pic_order_cnt_lsb_minus4 + 4).min(16) as u8;
                pic_order_cnt_lsb = br.read_bits(poc_bits)?;
                if pps.bottom_field_pic_order_in_frame_present_flag {
                    let _delta = br.read_se()?;
                }
            }
            if sps.pic_order_cnt_type == 1 && !sps.delta_pic_order_always_zero_flag {
                let _d0 = br.read_se()?;
                if pps.bottom_field_pic_order_in_frame_present_flag {
                    let _d1 = br.read_se()?;
                }
            }
            if pps.redundant_pic_cnt_present_flag {
                let _redundant = br.read_ue()?;
            }
            let canonical = if slice_type >= 5 {
                slice_type - 5
            } else {
                slice_type
            };
            if canonical == 1 {
                let _direct_spatial = br.read_bit()?;
            }
            let mut num_ref_idx_l0_active_minus1 = pps.num_ref_idx_l0_default_active_minus1;
            let mut num_ref_idx_l1_active_minus1 = pps.num_ref_idx_l1_default_active_minus1;
            if canonical == 0 || canonical == 1 || canonical == 3 {
                if br.read_bit()? != 0 {
                    num_ref_idx_l0_active_minus1 = br.read_ue()?;
                    if canonical == 1 {
                        num_ref_idx_l1_active_minus1 = br.read_ue()?;
                    }
                }
            }
            // ref_pic_list_modification
            if canonical != 2 && canonical != 4 {
                if br.read_bit()? != 0 {
                    loop {
                        let op = br.read_ue()?;
                        if op == 3 {
                            break;
                        }
                        let _ = br.read_ue()?;
                    }
                }
            }
            if canonical == 1 {
                if br.read_bit()? != 0 {
                    loop {
                        let op = br.read_ue()?;
                        if op == 3 {
                            break;
                        }
                        let _ = br.read_ue()?;
                    }
                }
            }
            // pred_weight_table
            let need_wt = (pps.weighted_pred_flag && (canonical == 0 || canonical == 3))
                || (pps.weighted_bipred_idc == 1 && canonical == 1);
            if need_wt {
                let _ = br.read_ue()?; // luma_log2_weight_denom
                if sps.chroma_format_idc != 0 {
                    let _ = br.read_ue()?;
                }
                for _ in 0..=num_ref_idx_l0_active_minus1 {
                    if br.read_bit()? != 0 {
                        let _ = br.read_se()?;
                        let _ = br.read_se()?;
                    }
                    if sps.chroma_format_idc != 0 && br.read_bit()? != 0 {
                        for _ in 0..2 {
                            let _ = br.read_se()?;
                            let _ = br.read_se()?;
                        }
                    }
                }
                if canonical == 1 {
                    for _ in 0..=num_ref_idx_l1_active_minus1 {
                        if br.read_bit()? != 0 {
                            let _ = br.read_se()?;
                            let _ = br.read_se()?;
                        }
                        if sps.chroma_format_idc != 0 && br.read_bit()? != 0 {
                            for _ in 0..2 {
                                let _ = br.read_se()?;
                                let _ = br.read_se()?;
                            }
                        }
                    }
                }
            }
            // dec_ref_pic_marking
            if nal_ref_idc != 0 {
                if nal_type == 5 {
                    let _ = br.read_bit()?;
                    let _ = br.read_bit()?;
                } else {
                    if br.read_bit()? != 0 {
                        loop {
                            let op = br.read_ue()?;
                            if op == 0 {
                                break;
                            }
                            match op {
                                1 | 2 | 4 | 6 => {
                                    let _ = br.read_ue()?;
                                }
                                3 => {
                                    let _ = br.read_ue()?;
                                    let _ = br.read_ue()?;
                                }
                                5 => {}
                                _ => break,
                            }
                        }
                    }
                }
            }
            let cabac_init_idc = if pps.entropy_coding_mode_flag && canonical != 2 && canonical != 4
            {
                br.read_ue().unwrap_or(0)
            } else {
                0
            };
            let slice_qp_delta = br.read_se().unwrap_or(0);
            let mut disable_deblocking_filter_idc = 0u32;
            let mut slice_alpha_c0_offset_div2 = 0i32;
            let mut slice_beta_offset_div2 = 0i32;
            if pps.deblocking_filter_control_present_flag {
                disable_deblocking_filter_idc = br.read_ue().unwrap_or(0);
                if disable_deblocking_filter_idc != 1 {
                    slice_alpha_c0_offset_div2 = br.read_se().unwrap_or(0);
                    slice_beta_offset_div2 = br.read_se().unwrap_or(0);
                }
            }
            let slice_header_bit_offset = (br.byte_off as u32) * 8 + (br.bit_off as u32) + 8;
            return Some(H264VclInfo {
                nal_type,
                nal_ref_idc,
                slice_type,
                frame_num,
                first_mb_in_slice,
                pic_order_cnt_lsb,
                cabac_init_idc,
                slice_qp_delta,
                disable_deblocking_filter_idc,
                slice_alpha_c0_offset_div2,
                slice_beta_offset_div2,
                num_ref_idx_l0_active_minus1,
                num_ref_idx_l1_active_minus1,
                slice_header_bit_offset,
            });
        }
        off = off.saturating_add(nal_len);
    }

    None
}

fn skip_scaling_list(br: &mut BitReader, size: usize) -> Option<()> {
    let mut last_scale = 8i32;
    let mut next_scale = 8i32;
    for _j in 0..size {
        if next_scale != 0 {
            let delta = br.read_se()?;
            next_scale = (last_scale + delta + 256) % 256;
        }
        if next_scale != 0 {
            last_scale = next_scale;
        }
    }
    Some(())
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ParsedSps {
    pub profile_idc: u8,
    pub level_idc: u8,
    pub chroma_format_idc: u32,
    pub bit_depth_luma_minus8: u32,
    pub bit_depth_chroma_minus8: u32,
    pub log2_max_frame_num_minus4: u32,
    pub pic_order_cnt_type: u32,
    pub log2_max_pic_order_cnt_lsb_minus4: u32,
    pub delta_pic_order_always_zero_flag: bool,
    pub max_num_ref_frames: u32,
    pub pic_width_in_mbs_minus1: u32,
    pub pic_height_in_map_units_minus1: u32,
    pub frame_mbs_only_flag: bool,
    pub mb_adaptive_frame_field_flag: bool,
    pub direct_8x8_inference_flag: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ParsedPps {
    pub entropy_coding_mode_flag: bool,
    pub bottom_field_pic_order_in_frame_present_flag: bool,
    pub num_ref_idx_l0_default_active_minus1: u32,
    pub num_ref_idx_l1_default_active_minus1: u32,
    pub weighted_pred_flag: bool,
    pub weighted_bipred_idc: u32,
    pub pic_init_qp_minus26: i32,
    pub chroma_qp_index_offset: i32,
    pub deblocking_filter_control_present_flag: bool,
    pub constrained_intra_pred_flag: bool,
    pub redundant_pic_cnt_present_flag: bool,
    pub transform_8x8_mode_flag: bool,
    pub second_chroma_qp_index_offset: i32,
}

pub(crate) fn parse_sps(sps_nal: &[u8]) -> Option<ParsedSps> {
    if sps_nal.len() < 4 {
        return None;
    }
    let clean = remove_emulation_prevention(sps_nal);
    let profile_idc = *clean.get(1)?;
    let level_idc = *clean.get(3)?;
    let mut br = BitReader::new(clean.get(4..)?);
    let _seq_parameter_set_id = br.read_ue()?;

    let mut chroma_format_idc = 1u32;
    let mut bit_depth_luma_minus8 = 0u32;
    let mut bit_depth_chroma_minus8 = 0u32;
    let high_profiles: &[u8] = &[100, 110, 122, 244, 44, 83, 86, 118, 128, 138, 139, 134];
    if high_profiles.contains(&profile_idc) {
        chroma_format_idc = br.read_ue()?;
        if chroma_format_idc == 3 {
            let _separate_colour_plane = br.read_bit()?;
        }
        bit_depth_luma_minus8 = br.read_ue()?;
        bit_depth_chroma_minus8 = br.read_ue()?;
        let _qpprime_y_zero_transform_bypass = br.read_bit()?;
        let seq_scaling_matrix_present = br.read_bit()?;
        if seq_scaling_matrix_present == 1 {
            let num_lists = if chroma_format_idc != 3 { 8 } else { 12 };
            for i in 0..num_lists {
                let present = br.read_bit()?;
                if present == 1 {
                    let size = if i < 6 { 16 } else { 64 };
                    skip_scaling_list(&mut br, size)?;
                }
            }
        }
    }

    let log2_max_frame_num_minus4 = br.read_ue()?;
    let pic_order_cnt_type = br.read_ue()?;
    let mut log2_max_pic_order_cnt_lsb_minus4 = 0u32;
    let mut delta_pic_order_always_zero_flag = false;
    if pic_order_cnt_type == 0 {
        log2_max_pic_order_cnt_lsb_minus4 = br.read_ue()?;
    } else if pic_order_cnt_type == 1 {
        delta_pic_order_always_zero_flag = br.read_bit()? != 0;
        let _offset_for_non_ref_pic = br.read_se()?;
        let _offset_for_top_to_bottom_field = br.read_se()?;
        let num_ref_in_poc_cycle = br.read_ue()?;
        for _ in 0..num_ref_in_poc_cycle {
            let _offset = br.read_se()?;
        }
    }

    let max_num_ref_frames = br.read_ue()?;
    let _gaps_in_frame_num = br.read_bit()?;
    let pic_width_in_mbs_minus1 = br.read_ue()?;
    let pic_height_in_map_units_minus1 = br.read_ue()?;
    let frame_mbs_only_flag = br.read_bit()? != 0;
    let mut mb_adaptive_frame_field_flag = false;
    if !frame_mbs_only_flag {
        mb_adaptive_frame_field_flag = br.read_bit()? != 0;
    }
    let direct_8x8_inference_flag = br.read_bit()? != 0;

    Some(ParsedSps {
        profile_idc,
        level_idc,
        chroma_format_idc,
        bit_depth_luma_minus8,
        bit_depth_chroma_minus8,
        log2_max_frame_num_minus4,
        pic_order_cnt_type,
        log2_max_pic_order_cnt_lsb_minus4,
        delta_pic_order_always_zero_flag,
        max_num_ref_frames,
        pic_width_in_mbs_minus1,
        pic_height_in_map_units_minus1,
        frame_mbs_only_flag,
        mb_adaptive_frame_field_flag,
        direct_8x8_inference_flag,
    })
}

pub(crate) fn parse_pps(pps_nal: &[u8], sps: &ParsedSps) -> Option<ParsedPps> {
    if pps_nal.len() < 2 {
        return None;
    }
    let clean = remove_emulation_prevention(pps_nal);
    let mut br = BitReader::new(clean.get(1..)?);
    let _pic_parameter_set_id = br.read_ue()?;
    let _seq_parameter_set_id = br.read_ue()?;
    let entropy_coding_mode_flag = br.read_bit()? != 0;
    let bottom_field_pic_order_in_frame_present_flag = br.read_bit()? != 0;
    let num_slice_groups_minus1 = br.read_ue()?;
    if num_slice_groups_minus1 > 0 {
        return None;
    }
    let num_ref_idx_l0_default_active_minus1 = br.read_ue()?;
    let num_ref_idx_l1_default_active_minus1 = br.read_ue()?;
    let weighted_pred_flag = br.read_bit()? != 0;
    let weighted_bipred_idc = br.read_bits(2)?;
    let pic_init_qp_minus26 = br.read_se()?;
    let _pic_init_qs_minus26 = br.read_se()?;
    let chroma_qp_index_offset = br.read_se()?;
    let deblocking_filter_control_present_flag = br.read_bit()? != 0;
    let constrained_intra_pred_flag = br.read_bit()? != 0;
    let redundant_pic_cnt_present_flag = br.read_bit()? != 0;

    let mut transform_8x8_mode_flag = false;
    let mut second_chroma_qp_index_offset = chroma_qp_index_offset;
    if let Some(t8x8) = br.read_bit() {
        transform_8x8_mode_flag = t8x8 != 0;
        if let Some(scaling_present) = br.read_bit() {
            if scaling_present == 1 {
                let num_lists = if transform_8x8_mode_flag {
                    if sps.chroma_format_idc != 3 { 8 } else { 12 }
                } else {
                    6
                };
                for i in 0..num_lists {
                    if br.read_bit()? == 1 {
                        let size = if i < 6 { 16 } else { 64 };
                        skip_scaling_list(&mut br, size)?;
                    }
                }
            }
            if let Some(offset) = br.read_se() {
                second_chroma_qp_index_offset = offset;
            }
        }
    }

    Some(ParsedPps {
        entropy_coding_mode_flag,
        bottom_field_pic_order_in_frame_present_flag,
        num_ref_idx_l0_default_active_minus1,
        num_ref_idx_l1_default_active_minus1,
        weighted_pred_flag,
        weighted_bipred_idc,
        pic_init_qp_minus26,
        chroma_qp_index_offset,
        deblocking_filter_control_present_flag,
        constrained_intra_pred_flag,
        redundant_pic_cnt_present_flag,
        transform_8x8_mode_flag,
        second_chroma_qp_index_offset,
    })
}
