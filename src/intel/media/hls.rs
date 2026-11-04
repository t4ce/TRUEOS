//! Bounded, unencrypted AVC HLS VOD admission and MPEG-TS demux. No transcoding.
use alloc::{
    string::{String, ToString},
    vec::Vec,
};

pub(super) const PLAYLIST_CAP: usize = 1024 * 1024;
pub(super) const SEGMENT_CAP: usize = 16 * 1024 * 1024;
const PICTURE_CAP: usize = 8 * 1024 * 1024;
const SAMPLE_CAP: usize = 4096;

pub(super) enum Playlist {
    Variant(String),
    Vod(Vec<String>),
    /// A finite playlist indexes a contiguous prefix of one fMP4 resource.
    /// The existing complete-file AVC demux can consume that resource directly.
    SingleFileMp4 {
        reference: String,
        length: usize,
    },
}

fn reference(value: &str) -> Result<(), &'static str> {
    if value.is_empty()
        || value.len() > 3072
        || value.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return Err("HLS invalid reference");
    }
    Ok(())
}

fn byte_range(value: &str) -> Result<(usize, Option<usize>), &'static str> {
    let (length, offset) = value
        .split_once('@')
        .map_or((value, None), |(n, o)| (n, Some(o)));
    let number = |text: &str| {
        if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
            return Err("HLS invalid byte range");
        }
        text.parse::<usize>().map_err(|_| "HLS byte range overflow")
    };
    let length = number(length)?;
    if length == 0 || length > SEGMENT_CAP {
        return Err("HLS byte range length outside limit");
    }
    Ok((length, offset.map(number).transpose()?))
}

fn initialization_map(mut attributes: &str) -> Result<(String, usize), &'static str> {
    let mut uri = None;
    let mut range = None;
    let mut names = Vec::new();
    while !attributes.is_empty() {
        let (name, tail) = attributes
            .split_once('=')
            .ok_or("HLS invalid map attributes")?;
        if name.is_empty()
            || !name
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'-')
            || names.contains(&name)
        {
            return Err("HLS invalid map attribute name");
        }
        names.push(name);
        let (value, rest) = if let Some(tail) = tail.strip_prefix('"') {
            let (value, rest) = tail
                .split_once('"')
                .ok_or("HLS unterminated map attribute")?;
            (value, rest)
        } else {
            let (value, rest) = tail.split_once(',').map_or((tail, ""), |(v, r)| (v, r));
            if matches!(name, "URI" | "BYTERANGE") {
                return Err("HLS map URI and range must be quoted");
            }
            attributes = rest;
            if value.is_empty() {
                return Err("HLS empty map attribute");
            }
            continue;
        };
        match name {
            "URI" => {
                reference(value)?;
                uri = Some(value.to_string());
            }
            "BYTERANGE" => {
                range = Some(byte_range(value)?);
            }
            _ => {}
        }
        attributes = if rest.is_empty() {
            ""
        } else {
            rest.strip_prefix(',').ok_or("HLS invalid map delimiter")?
        };
    }
    let (length, offset) = range.ok_or("HLS single-file map requires a byte range")?;
    if offset != Some(0) {
        return Err("HLS single-file initialization must begin at zero");
    }
    Ok((uri.ok_or("HLS map missing URI")?, length))
}

/// Return references, resolving them with the shared HTTP URL parser at fetch time.
pub(super) fn playlist(text: &str) -> Result<Playlist, &'static str> {
    if text.len() > PLAYLIST_CAP || !text.trim_start().starts_with("#EXTM3U") {
        return Err("HLS invalid playlist");
    }
    let mut variant = false;
    let mut segment = false;
    let mut urls = Vec::new();
    let mut ranges = Vec::new();
    let mut pending_range = None;
    let mut initialization = None;
    let mut ended = false;
    for line in text.lines().map(str::trim).filter(|s| !s.is_empty()) {
        if line.starts_with("#EXT-X-KEY:") && line != "#EXT-X-KEY:METHOD=NONE"
            || line == "#EXT-X-DISCONTINUITY"
        {
            return Err("HLS encrypted or discontinuous stream unsupported");
        }
        if let Some(attributes) = line.strip_prefix("#EXT-X-MAP:") {
            if initialization.is_some() || !urls.is_empty() {
                return Err("HLS changing initialization map unsupported");
            }
            initialization = Some(initialization_map(attributes)?);
        } else if let Some(value) = line.strip_prefix("#EXT-X-BYTERANGE:") {
            if pending_range.replace(byte_range(value)?).is_some() {
                return Err("HLS repeated byte range");
            }
        } else if line.starts_with("#EXT-X-STREAM-INF:") {
            variant = true;
        } else if line.starts_with("#EXTINF:") {
            if segment || ended {
                return Err("HLS invalid segment list");
            }
            segment = true;
        } else if line == "#EXT-X-ENDLIST" {
            ended = true;
        } else if !line.starts_with('#') {
            reference(line)?;
            if variant {
                if initialization.is_some() || pending_range.is_some() {
                    return Err("HLS invalid variant list");
                }
                return Ok(Playlist::Variant(line.to_string()));
            }
            if !segment || urls.len() >= 4096 {
                return Err("HLS invalid segment list");
            }
            urls.push(line.to_string());
            ranges.push(pending_range.take());
            segment = false;
        }
    }
    if !ended || urls.is_empty() || segment || pending_range.is_some() {
        return Err("HLS requires finite VOD playlist");
    }
    if let Some((reference, mut length)) = initialization {
        for (index, (url, range)) in urls.iter().zip(ranges).enumerate() {
            let (count, offset) = range.ok_or("HLS single-file segment missing byte range")?;
            if url != &reference || index == 0 && offset.is_none() {
                return Err("HLS requires one byte-ranged MP4 resource");
            }
            if offset.unwrap_or(length) != length {
                return Err("HLS noncontiguous MP4 byte ranges unsupported");
            }
            length = length.checked_add(count).ok_or("HLS byte range overflow")?;
        }
        return Ok(Playlist::SingleFileMp4 { reference, length });
    }
    if ranges.iter().any(Option::is_some) {
        return Err("HLS byte-ranged MPEG-TS unsupported");
    }
    Ok(Playlist::Vod(urls))
}

pub(super) struct Sample {
    pub(super) data: Vec<u8>,
    pub(super) pts: u64,
    pub(super) dts: u64,
}

fn timestamp(b: &[u8]) -> Result<u64, &'static str> {
    if b.len() < 5 || b[0] & 1 == 0 || b[2] & 1 == 0 || b[4] & 1 == 0 {
        return Err("TS invalid PES timestamp");
    }
    Ok(((b[0] as u64 & 14) << 29)
        | ((b[1] as u64) << 22)
        | ((b[2] as u64 & 254) << 14)
        | ((b[3] as u64) << 7)
        | ((b[4] as u64) >> 1))
}

fn pes(bytes: &[u8]) -> Result<Sample, &'static str> {
    if bytes.len() < 14
        || bytes[..3] != [0, 0, 1]
        || bytes[3] & 0xf0 != 0xe0
        || bytes[6] & 0xc0 != 0x80
    {
        return Err("TS invalid video PES");
    }
    let length = u16::from_be_bytes([bytes[4], bytes[5]]) as usize;
    if length != 0 && length + 6 != bytes.len() {
        return Err("TS truncated video PES");
    }
    let flags = bytes[7] >> 6;
    let header = bytes[8] as usize;
    let start = 9 + header;
    if flags < 2 || header < if flags == 3 { 10 } else { 5 } || start >= bytes.len() {
        return Err("TS missing video timing");
    }
    let pts = timestamp(&bytes[9..14])?;
    let dts = if flags == 3 {
        timestamp(&bytes[14..19])?
    } else {
        pts
    };
    let data = bytes[start..].to_vec();
    Ok(Sample { data, pts, dts })
}

struct Psi {
    bytes: Vec<u8>,
    length: usize,
}
impl Psi {
    fn new() -> Self {
        Self {
            bytes: Vec::new(),
            length: 0,
        }
    }
    fn push(&mut self, payload: &[u8], start: bool) -> Result<Option<Vec<u8>>, &'static str> {
        let payload = if start {
            let skip = *payload.first().ok_or("TS empty PSI")? as usize + 1;
            if !self.bytes.is_empty() {
                return Err("TS incomplete PSI section");
            }
            payload.get(skip..).ok_or("TS invalid PSI pointer")?
        } else {
            payload
        };
        if payload.is_empty() || payload[0] == 0xff && self.bytes.is_empty() {
            return Ok(None);
        }
        self.bytes.extend_from_slice(payload);
        if self.bytes.len() >= 3 && self.length == 0 {
            self.length = 3 + (((self.bytes[1] as usize & 15) << 8) | self.bytes[2] as usize);
            if self.length > 1024 || self.length < 12 {
                return Err("TS invalid PSI length");
            }
        }
        if self.length != 0 && self.bytes.len() >= self.length {
            self.bytes.truncate(self.length);
            let bytes = core::mem::take(&mut self.bytes);
            self.length = 0;
            let mut crc = 0xffff_ffffu32;
            for b in &bytes {
                crc ^= (*b as u32) << 24;
                for _ in 0..8 {
                    crc = (crc << 1)
                        ^ if crc & 0x8000_0000 != 0 {
                            0x04c1_1db7
                        } else {
                            0
                        };
                }
            }
            if crc != 0 {
                return Err("TS invalid PSI CRC");
            }
            return Ok(Some(bytes));
        }
        Ok(None)
    }
}

fn packets(bytes: &[u8]) -> Result<Vec<Sample>, &'static str> {
    if bytes.is_empty() || bytes.len() > SEGMENT_CAP || bytes.len() % 188 != 0 {
        return Err("TS segment size invalid");
    }
    let mut pat = Psi::new();
    let mut pmt = Psi::new();
    let mut pmt_pid = None;
    let mut video_pid = None;
    let mut cc = None;
    let mut pending = Vec::new();
    let mut samples = Vec::new();
    for packet in bytes.chunks_exact(188) {
        if packet[0] != 0x47 || packet[1] & 0x80 != 0 || packet[3] & 0xc0 != 0 {
            return Err("TS corrupt or scrambled packet");
        }
        let pid = (((packet[1] & 31) as u16) << 8) | packet[2] as u16;
        let start = packet[1] & 0x40 != 0;
        let control = (packet[3] >> 4) & 3;
        if control == 0 {
            return Err("TS invalid adaptation control");
        }
        let offset = if control & 2 != 0 {
            5 + packet[4] as usize
        } else {
            4
        };
        if offset > 188 {
            return Err("TS invalid adaptation length");
        }
        if control & 1 == 0 {
            continue;
        }
        let payload = &packet[offset..];
        if pid == 0 {
            if let Some(s) = pat.push(payload, start)? {
                if s[0] != 0 || s[5] & 1 == 0 || s[6] != 0 || s[7] != 0 {
                    return Err("TS unsupported PAT");
                }
                let mut programs = s[8..s.len() - 4]
                    .chunks_exact(4)
                    .filter(|p| p[0] != 0 || p[1] != 0);
                let p = programs.next().ok_or("TS no program")?;
                let found = (((p[2] & 31) as u16) << 8) | p[3] as u16;
                if programs.next().is_some() || pmt_pid.is_some_and(|old| old != found) {
                    return Err("TS multiple programs");
                }
                pmt_pid = Some(found);
            }
        } else if Some(pid) == pmt_pid {
            if let Some(s) = pmt.push(payload, start)? {
                if s[0] != 2 || s[5] & 1 == 0 || s[6] != 0 || s[7] != 0 {
                    return Err("TS unsupported PMT");
                }
                let end = s.len() - 4;
                let mut i = 12 + (((s[10] as usize & 15) << 8) | s[11] as usize);
                let mut found = None;
                while i < end {
                    if i + 5 > end {
                        return Err("TS truncated PMT");
                    }
                    if s[i] == 0x1b {
                        let pid = (((s[i + 1] & 31) as u16) << 8) | s[i + 2] as u16;
                        if found.replace(pid).is_some() {
                            return Err("TS multiple AVC tracks");
                        }
                    }
                    i += 5 + (((s[i + 3] as usize & 15) << 8) | s[i + 4] as usize);
                }
                if i != end || found.is_none() || video_pid.is_some_and(|p| Some(p) != found) {
                    return Err("TS no stable AVC track");
                }
                video_pid = found;
            }
        } else if Some(pid) == video_pid {
            let count = packet[3] & 15;
            if cc.is_some_and(|old| count != (old + 1) & 15) {
                return Err("TS video continuity lost");
            }
            cc = Some(count);
            if start && !pending.is_empty() {
                if samples.len() >= SAMPLE_CAP {
                    return Err("TS sample limit reached");
                }
                samples.push(pes(&pending)?);
                pending.clear();
            }
            if !start && pending.is_empty() {
                return Err("TS starts inside video PES");
            }
            if pending.len() + payload.len() > PICTURE_CAP {
                return Err("TS picture too large");
            }
            pending.extend_from_slice(payload);
        }
    }
    if !pending.is_empty() {
        samples.push(pes(&pending)?);
    }
    if samples.is_empty() {
        return Err("TS has no AVC pictures");
    }
    Ok(samples)
}

/// PES boundaries can split the tail of the preceding AVC picture. Preserve
/// that tail across segment boundaries; publish only complete AUD-delimited AUs.
pub(super) struct Demux {
    pending: Option<Sample>,
    queued: Vec<Sample>,
}
impl Demux {
    pub(super) fn new() -> Self {
        Self {
            pending: None,
            queued: Vec::new(),
        }
    }
    pub(super) fn segment(
        &mut self,
        bytes: &[u8],
        last: bool,
    ) -> Result<Vec<Sample>, &'static str> {
        for mut sample in packets(bytes)? {
            let mut boundary = None;
            for i in 0..sample.data.len().saturating_sub(3) {
                if sample.data[i..i + 3] == [0, 0, 1] && sample.data[i + 3] & 31 == 9 {
                    if boundary.is_some() {
                        return Err("TS requires one AVC access unit per PES");
                    }
                    boundary = Some(if i > 0 && sample.data[i - 1] == 0 {
                        i - 1
                    } else {
                        i
                    });
                }
            }
            let start = boundary.ok_or("TS missing AVC access unit delimiter")?;
            if let Some(mut previous) = self.pending.take() {
                if previous.data.len() + start > PICTURE_CAP {
                    return Err("TS picture too large");
                }
                previous.data.extend_from_slice(&sample.data[..start]);
                self.queued.push(previous);
            } else if start != 0 {
                return Err("TS begins inside AVC picture");
            }
            sample.data.drain(..start);
            self.pending = Some(sample);
        }
        if last && let Some(sample) = self.pending.take() {
            self.queued.push(sample);
        }
        if self.queued.len() > SAMPLE_CAP
            || self.queued.iter().map(|s| s.data.len()).sum::<usize>() > 2 * SEGMENT_CAP
        {
            return Err("HLS GOP lookahead limit");
        }
        // Retain the final GOP until the next IDR or EOS, so every released
        // timing batch is closed under B-frame presentation reordering.
        let release = if last {
            self.queued.len()
        } else {
            self.queued
                .iter()
                .rposition(|s| {
                    s.data
                        .windows(4)
                        .any(|w| w[..3] == [0, 0, 1] && w[3] & 31 == 5)
                })
                .unwrap_or(0)
        };
        Ok(self.queued.drain(..release).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn crc(bytes: &mut Vec<u8>) {
        let mut crc = 0xffff_ffffu32;
        for b in bytes.iter() {
            crc ^= (*b as u32) << 24;
            for _ in 0..8 {
                crc = (crc << 1)
                    ^ if crc & 0x8000_0000 != 0 {
                        0x04c1_1db7
                    } else {
                        0
                    };
            }
        }
        bytes.extend_from_slice(&crc.to_be_bytes());
    }
    fn packet(pid: u16, cc: u8, payload: &[u8]) -> Vec<u8> {
        assert!(payload.len() < 183);
        let mut p = alloc::vec![0xff; 188];
        p[..5].copy_from_slice(&[
            0x47,
            0x40 | ((pid >> 8) as u8),
            pid as u8,
            0x30 | cc,
            (183 - payload.len()) as u8,
        ]);
        p[5] = 0;
        p[188 - payload.len()..].copy_from_slice(payload);
        p
    }
    fn segment(data: &[u8], pts: u64) -> Vec<u8> {
        let mut pat = alloc::vec![0, 0xb0, 13, 0, 1, 0xc1, 0, 0, 0, 1, 0xe0, 0x20];
        crc(&mut pat);
        pat.insert(0, 0);
        let mut pmt = alloc::vec![
            2, 0xb0, 18, 0, 1, 0xc1, 0, 0, 0xe1, 0, 0xf0, 0, 0x1b, 0xe1, 0, 0xf0, 0
        ];
        crc(&mut pmt);
        pmt.insert(0, 0);
        let mut pes = alloc::vec![
            0,
            0,
            1,
            0xe0,
            0,
            0,
            0x80,
            0x80,
            5,
            0x21 | (((pts >> 30) as u8 & 7) << 1),
            (pts >> 22) as u8,
            (((pts >> 15) as u8 & 127) << 1) | 1,
            (pts >> 7) as u8,
            ((pts as u8 & 127) << 1) | 1
        ];
        pes.extend_from_slice(data);
        [
            packet(0, 0, &pat),
            packet(32, 0, &pmt),
            packet(256, 0, &pes),
        ]
        .concat()
    }
    #[test]
    fn playlist_limits_and_unsupported_features_fail_explicitly() {
        assert!(matches!(
            playlist("#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\nchild.m3u8\n").unwrap(),
            Playlist::Variant(_)
        ));
        assert!(matches!(
            playlist("#EXTM3U\n#EXTINF:1,\nclip.ts?token=abc\n#EXT-X-ENDLIST").unwrap(),
            Playlist::Vod(_)
        ));
        for tag in [
            "#EXT-X-KEY:METHOD=AES-128,URI=\"key\"",
            "#EXT-X-MAP:URI=\"init.mp4\"",
            "#EXT-X-BYTERANGE:100",
            "#EXT-X-DISCONTINUITY",
        ] {
            assert!(
                playlist(&alloc::format!(
                    "#EXTM3U\n{tag}\n#EXTINF:1,\nclip.ts\n#EXT-X-ENDLIST"
                ))
                .is_err()
            );
        }
        assert!(playlist("#EXTM3U\n#EXTINF:1,\nclip.ts").is_err());
        assert!(playlist("#EXTM3U\nstray.ts\n#EXT-X-ENDLIST").is_err());
    }
    #[test]
    fn single_mp4_ranges_cover_initialization_and_finite_media() {
        let text = "#EXTM3U\n#EXT-X-MAP:URI=\"clip.mp4?token=a,b\",BYTERANGE=\"100@0\"\n#EXTINF:1,\n#EXT-X-BYTERANGE:50@100\nclip.mp4?token=a,b\n#EXTINF:1,\n#EXT-X-BYTERANGE:75\nclip.mp4?token=a,b\n#EXT-X-ENDLIST";
        match playlist(text).unwrap() {
            Playlist::SingleFileMp4 { reference, length } => {
                assert_eq!(reference, "clip.mp4?token=a,b");
                assert_eq!(length, 225);
            }
            _ => panic!("expected one MP4 resource"),
        }
    }
    #[test]
    fn ambiguous_or_noncontiguous_mp4_ranges_are_rejected() {
        let valid = "#EXTM3U\n#EXT-X-MAP:URI=\"clip.mp4\",BYTERANGE=\"100@0\"\n#EXTINF:1,\n#EXT-X-BYTERANGE:50@100\nclip.mp4\n#EXT-X-ENDLIST";
        for invalid in [
            valid.replace("100@0", "100@1"),
            valid.replace("50@100", "50@99"),
            valid.replace("50@100", "50@101"),
            valid.replace("50@100", "50"),
            valid.replace("50@100", "0@100"),
            valid.replace("50@100", "50@18446744073709551616"),
            valid.replace("\nclip.mp4\n", "\nother.mp4\n"),
            valid.replace(",BYTERANGE=\"100@0\"", ""),
            valid.replace("URI=\"clip.mp4\"", "URI=\"clip.mp4\",URI=\"other.mp4\""),
            valid.replace("URI=\"clip.mp4\"", "URI=clip.mp4"),
            valid.replace("BYTERANGE=\"100@0\"", "BYTERANGE=\"100@0"),
            valid.replace("#EXT-X-ENDLIST", ""),
        ] {
            assert!(playlist(&invalid).is_err(), "accepted {invalid}");
        }
    }
    #[test]
    fn pes_tail_survives_segment_boundary_and_gop_closes_at_next_idr() {
        let first = [0, 0, 0, 1, 9, 0xf0, 0, 0, 1, 0x65, 0x80, 0xaa, 0xbb];
        let mut demux = Demux::new();
        assert!(
            demux
                .segment(&segment(&first[..11], 9000), false)
                .unwrap()
                .is_empty()
        );
        let next = [0xaa, 0xbb, 0, 0, 0, 1, 9, 0xf0, 0, 0, 1, 0x65, 0x80];
        let out = demux.segment(&segment(&next, 18000), true).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].data, first);
        assert_eq!(out[0].pts, 9000);
        assert_eq!(out[1].data, &next[2..]);
        assert_eq!(out[1].pts, 18000);
    }
    #[test]
    fn truncated_corrupt_and_lost_video_packets_are_rejected() {
        let data = [0, 0, 1, 9, 0xf0, 0, 0, 1, 0x65, 0x80];
        let bytes = segment(&data, 9000);
        assert!(
            Demux::new()
                .segment(&bytes[..bytes.len() - 1], true)
                .is_err()
        );
        let mut corrupt = bytes.clone();
        corrupt[187] ^= 1;
        assert!(Demux::new().segment(&corrupt, true).is_err());
        let mut lost = bytes.clone();
        let mut p = bytes[376..].to_vec();
        p[3] = (p[3] & 0xf0) | 2;
        lost.extend(p);
        assert!(Demux::new().segment(&lost, true).is_err());
        let mut scrambled = bytes;
        scrambled[379] |= 0x80;
        assert!(Demux::new().segment(&scrambled, true).is_err());
    }
}
