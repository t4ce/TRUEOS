extern crate alloc;

use alloc::vec::Vec;

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
    EmptyAccessUnit,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct AvccSummary<'a> {
    pub nal_length_size: usize,
    pub profile_idc: u8,
    pub level_idc: u8,
    pub sps: &'a [u8],
    pub pps: &'a [u8],
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Mp4H264Summary<'a> {
    pub width: u16,
    pub height: u16,
    pub timescale: u32,
    pub duration: u64,
    pub sample_count: u32,
    pub first_chunk_offset: u64,
    pub first_sample_size: u32,
    pub first_sample: &'a [u8],
    pub avcc: AvccSummary<'a>,
}

pub(crate) struct AnnexBAccessUnit {
    pub bytes: Vec<u8>,
    pub sample_nal_count: usize,
    pub has_idr: bool,
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
    avcc: Option<AvccSummary<'a>>,
    sample_count: Option<u32>,
    first_sample_size: Option<u32>,
    first_chunk_offset: Option<u64>,
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

fn parse_avcc<'a>(bytes: &'a [u8]) -> Option<AvccSummary<'a>> {
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
        sps: first_sps?,
        pps: first_pps?,
    })
}

fn parse_stsd<'a>(bytes: &'a [u8]) -> Option<(u16, u16, AvccSummary<'a>)> {
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
            }
            b"stco" => {
                track.first_chunk_offset = parse_stco(child.data);
            }
            b"co64" => {
                track.first_chunk_offset = parse_co64(child.data);
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

pub(crate) fn parse_h264_mp4_summary<'a>(
    bytes: &'a [u8],
) -> Result<Mp4H264Summary<'a>, Mp4ParseError> {
    let mut off = 0usize;
    let mut saw_moov = false;
    let mut best_track: Option<TrackCandidate<'a>> = None;

    while let Some(root) = next_box(bytes, &mut off) {
        let root = root?;
        if root.kind != b"moov" {
            continue;
        }
        saw_moov = true;
        let mut moov_off = 0usize;
        while let Some(child) = next_box(root.data, &mut moov_off) {
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
    }

    if !saw_moov {
        return Err(Mp4ParseError::NoMoov);
    }
    let track = best_track.ok_or(Mp4ParseError::NoVideoTrack)?;
    let avcc = track.avcc.ok_or(Mp4ParseError::NoAvcc)?;
    let sample_count = track.sample_count.ok_or(Mp4ParseError::NoSamples)?;
    let first_sample_size = track.first_sample_size.ok_or(Mp4ParseError::NoSamples)?;
    let first_chunk_offset = track
        .first_chunk_offset
        .ok_or(Mp4ParseError::NoChunkOffsets)?;
    let first_sample_off =
        usize::try_from(first_chunk_offset).map_err(|_| Mp4ParseError::BadSampleRange)?;
    let first_sample_len =
        usize::try_from(first_sample_size).map_err(|_| Mp4ParseError::BadSampleRange)?;
    let first_sample = bytes
        .get(first_sample_off..first_sample_off.saturating_add(first_sample_len))
        .ok_or(Mp4ParseError::BadSampleRange)?;

    Ok(Mp4H264Summary {
        width: track.width.ok_or(Mp4ParseError::NoAvc1)?,
        height: track.height.ok_or(Mp4ParseError::NoAvc1)?,
        timescale: track.timescale.unwrap_or(0),
        duration: track.duration.unwrap_or(0),
        sample_count,
        first_chunk_offset,
        first_sample_size,
        first_sample,
        avcc,
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

fn push_annex_b_nal(out: &mut Vec<u8>, nal: &[u8]) {
    out.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
    out.extend_from_slice(nal);
}

pub(crate) fn build_annex_b_access_unit(
    summary: &Mp4H264Summary<'_>,
) -> Result<AnnexBAccessUnit, Mp4ParseError> {
    let nal_length_size = summary.avcc.nal_length_size;
    if !(1..=4).contains(&nal_length_size) {
        return Err(Mp4ParseError::BadNalLengthSize);
    }

    let mut bytes = Vec::new();
    push_annex_b_nal(&mut bytes, summary.avcc.sps);
    push_annex_b_nal(&mut bytes, summary.avcc.pps);

    let mut off = 0usize;
    let mut sample_nal_count = 0usize;
    let mut has_idr = false;
    while off.saturating_add(nal_length_size) <= summary.first_sample.len() {
        let mut nal_len = 0usize;
        let mut idx = 0usize;
        while idx < nal_length_size {
            nal_len = (nal_len << 8) | summary.first_sample[off + idx] as usize;
            idx += 1;
        }
        off = off.saturating_add(nal_length_size);
        if nal_len == 0 {
            return Err(Mp4ParseError::BadNalRange);
        }
        let nal = summary
            .first_sample
            .get(off..off.saturating_add(nal_len))
            .ok_or(Mp4ParseError::BadNalRange)?;
        has_idr |= (nal[0] & 0x1F) == 5;
        push_annex_b_nal(&mut bytes, nal);
        off = off.saturating_add(nal_len);
        sample_nal_count += 1;
    }

    if sample_nal_count == 0 {
        return Err(Mp4ParseError::EmptyAccessUnit);
    }

    Ok(AnnexBAccessUnit {
        bytes,
        sample_nal_count,
        has_idr,
    })
}
