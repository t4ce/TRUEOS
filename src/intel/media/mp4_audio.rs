//! The optional AAC-LC track in a classic AVC MP4.
use super::*;
use symphonia_codec_aac::AacDecoder;
use symphonia_core::{
    audio::{AudioBufferRef, Signal},
    codecs::{CODEC_TYPE_AAC, CodecParameters, Decoder, DecoderOptions},
    formats::Packet,
};

const OUTPUT_RATE: i128 = 48_000;
const FRAME_BYTES: usize = 4;
const MAX_SAMPLE_BYTES: usize = 32 * 1024;
const MAX_SAMPLE_COUNT: usize = 512_000;
const AAC_FRAME_SAMPLES: usize = 1024;

#[derive(Default)]
struct TrackEdit {
    leading_frames: i64,
    start: i64,
    length_frames: Option<i64>,
}

impl TrackEdit {
    fn frame_at(&self, time: i128, timescale: u32) -> Result<i64, &'static str> {
        let offset = frames(time - i128::from(self.start), timescale)?;
        self.leading_frames
            .checked_add(offset)
            .ok_or("MP4 audio timestamp overflow")
    }
}

pub(in super::super) struct Mp4Audio {
    samples: Vec<Mp4SampleRef>,
    index: usize,
    timescale: u32,
    decode_rate: u32,
    edit: TrackEdit,
    video_zero: i64,
    decoder: AacDecoder,
}

pub(in super::super) struct DecodedAudio {
    start_frame: u64,
    pcm: Vec<u8>,
}

#[derive(Default)]
pub(in super::super) struct PcmWriter {
    next_frame: u64,
}

impl PcmWriter {
    /// Submit one part of a packet, including any gap before it. The sink
    /// returns a byte count and can take a prefix or apply backpressure.
    pub(in super::super) fn write(
        &mut self,
        packet: &DecodedAudio,
        enqueue: impl FnOnce(&[u8]) -> Result<usize, i32>,
    ) -> Result<bool, i32> {
        if self.next_frame < packet.start_frame {
            const SILENCE: [u8; 4 * 1024] = [0; 4 * 1024];
            let gap =
                (packet.start_frame - self.next_frame).min((SILENCE.len() / FRAME_BYTES) as u64);
            self.next_frame +=
                (enqueue(&SILENCE[..gap as usize * FRAME_BYTES])? / FRAME_BYTES) as u64;
            return Ok(false);
        }
        let consumed = self.next_frame - packet.start_frame;
        let total = (packet.pcm.len() / FRAME_BYTES) as u64;
        if consumed < total {
            let data = &packet.pcm[consumed as usize * FRAME_BYTES..];
            self.next_frame += (enqueue(data)? / FRAME_BYTES) as u64;
        }
        Ok(self.next_frame - packet.start_frame >= total)
    }
}

impl Mp4Audio {
    pub(in super::super) fn open(
        data: &[u8],
        moov: Mp4Box,
        file_len: usize,
        video: &Mp4AvcTrack,
    ) -> Result<Option<Self>, &'static str> {
        let tracks = mp4_collect_children(data, moov.payload_start, moov.end, *b"trak");
        let timescale = child(data, moov, *b"mvhd")
            .ok()
            .and_then(|header| header_timescale(data, header).ok());
        let mut audio = None;
        for track in &tracks {
            let Ok(mdia) = child(data, *track, *b"mdia") else {
                continue;
            };
            let Ok(hdlr) = child(data, mdia, *b"hdlr") else {
                continue;
            };
            if body(data, hdlr).get(8..12) == Some(b"soun") {
                match parse_audio_track(data, *track, mdia, timescale, file_len) {
                    Ok(candidate) => {
                        audio = Some(candidate);
                        break;
                    }
                    Err(reason) => {
                        crate::log_warn!(target: "intel-media";
                            "video/mp4: audio track skipped reason={}\n", reason);
                    }
                }
            }
        }
        let Some(mut audio) = audio else {
            return Ok(None);
        };
        let video_track = tracks
            .into_iter()
            .find(|track| {
                child(data, *track, *b"tkhd").and_then(|tkhd| mp4_parse_tkhd_track_id(data, tkhd))
                    == Ok(video.track_id)
            })
            .ok_or("MP4 video track missing")?;
        let video_edit = track_edit(data, video_track, timescale)?;
        let first_pts = video
            .samples
            .iter()
            .map(|sample| i128::from(sample.decode_time) + i128::from(sample.composition_offset))
            .min()
            .ok_or("MP4 video timing missing")?;
        audio.video_zero = video_edit.frame_at(first_pts, video.timescale)?;
        Ok(Some(audio))
    }

    pub(in super::super) fn next_sample(&self) -> Option<Mp4SampleRef> {
        self.samples.get(self.index).copied()
    }

    pub(in super::super) fn decode_sample(
        &mut self,
        compressed: Vec<u8>,
    ) -> Result<Option<DecodedAudio>, &'static str> {
        let sample = self.next_sample().ok_or("MP4 soundtrack exhausted")?;
        if compressed.len() != sample.size {
            return Err("MP4 AAC packet length mismatch");
        }
        self.index += 1;
        let start = self
            .edit
            .frame_at(i128::from(sample.decode_time), self.timescale)?;
        let end = self.edit.frame_at(
            i128::from(sample.decode_time) + i128::from(sample.duration),
            self.timescale,
        )?;
        let trim_start = start.max(self.edit.leading_frames).max(self.video_zero);
        let trim_end = match self.edit.length_frames {
            Some(duration) => end.min(self.edit.leading_frames.saturating_add(duration)),
            None => end,
        };
        // Decode AAC delay packets before their edit point to warm up the
        // decoder's overlap state, even though those samples are discarded.
        let packet = Packet::new_from_boxed_slice(
            0,
            sample.decode_time,
            u64::from(sample.duration),
            compressed.into_boxed_slice(),
        );
        let AudioBufferRef::F32(decoded) = self
            .decoder
            .decode(&packet)
            .map_err(|_| "MP4 AAC packet decode failed")?
        else {
            return Err("MP4 AAC decoder produced unsupported PCM");
        };
        if trim_end <= trim_start {
            return Ok(None);
        }
        let sample_rate = i128::from(self.decode_rate);
        let channel_count = decoded.spec().channels.count();
        let frames = decoded.frames();
        if !(1..=2).contains(&channel_count)
            || frames != AAC_FRAME_SAMPLES
            || decoded.spec().rate != self.decode_rate
            || (i128::from(end) - i128::from(start)) * sample_rate
                > (frames as i128 + 1) * OUTPUT_RATE
        {
            return Err("MP4 AAC packet timing or format unsupported");
        }
        let left = decoded.chan(0);
        let right = if channel_count == 2 {
            decoded.chan(1)
        } else {
            left
        };
        let mut pcm = Vec::with_capacity((trim_end - trim_start) as usize * FRAME_BYTES);
        for frame in trim_start..trim_end {
            let phase = (i128::from(frame) - i128::from(start)) * sample_rate;
            let first = ((phase / OUTPUT_RATE) as usize).min(frames - 1);
            let next = (first + 1).min(frames - 1);
            let fraction = (phase % OUTPUT_RATE) as f32 / OUTPUT_RATE as f32;
            for channel in [left, right] {
                let sample = channel[first] + fraction * (channel[next] - channel[first]);
                let s16 = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
                pcm.extend_from_slice(&s16.to_le_bytes());
            }
        }
        let start_frame = u64::try_from(i128::from(trim_start) - i128::from(self.video_zero))
            .map_err(|_| "MP4 AAC output timestamp overflow")?;
        Ok(Some(DecodedAudio { start_frame, pcm }))
    }
}

fn parse_audio_track(
    data: &[u8],
    track: Mp4Box,
    mdia: Mp4Box,
    movie_timescale: Option<u32>,
    file_len: usize,
) -> Result<Mp4Audio, &'static str> {
    let timescale = header_timescale(data, child(data, mdia, *b"mdhd")?)?;
    let edit = track_edit(data, track, movie_timescale)?;
    let stbl = child(data, child(data, mdia, *b"minf")?, *b"stbl")?;
    let (config, decode_rate) = decoder_config(data, child(data, stbl, *b"stsd")?)?;
    let params = CodecParameters::new()
        .for_codec(CODEC_TYPE_AAC)
        .with_extra_data(config.into())
        .clone();
    let decoder = AacDecoder::try_new(&params, &DecoderOptions::default())
        .map_err(|_| "MP4 AAC-LC decoder initialization failed")?;

    let stsz = child(data, stbl, *b"stsz")?;
    let count = mp4_read_u32(body(data, stsz), 8).ok_or("MP4 AAC stsz count missing")? as usize;
    if !(1..=MAX_SAMPLE_COUNT).contains(&count) {
        return Err("MP4 AAC sample count outside limit");
    }
    let sizes = mp4_parse_stsz(data, stsz)?;
    if sizes
        .iter()
        .any(|&size| !(1..=MAX_SAMPLE_BYTES).contains(&size))
    {
        return Err("MP4 AAC sample size outside limit");
    }
    let stts = child(data, stbl, *b"stts")?;
    let runs = mp4_read_u32(body(data, stts), 4).ok_or("MP4 AAC stts count missing")? as usize;
    if runs > stts.end.saturating_sub(stts.payload_start + 8) / 8 {
        return Err("MP4 AAC stts truncated");
    }
    let durations = mp4_parse_stts(data, stts, count)?;
    let stsc = child(data, stbl, *b"stsc")?;
    let mapping = mp4_parse_stsc(data, stsc)?;
    if mapping[0].first_chunk != 1
        || mapping
            .windows(2)
            .any(|pair| pair[0].first_chunk >= pair[1].first_chunk)
        || mapping.iter().any(|entry| entry.samples_per_chunk == 0)
        || body(data, stsc)[8..]
            .chunks_exact(12)
            .take(mapping.len())
            .any(|run| run[8..12] != [0, 0, 0, 1])
    {
        return Err("MP4 AAC chunk mapping unsupported");
    }
    let offsets = mp4_parse_chunk_offsets(
        data,
        child(data, stbl, *b"stco").or_else(|_| child(data, stbl, *b"co64"))?,
    )?;
    let samples = mp4_build_samples(
        &sizes,
        &mapping,
        &offsets,
        &[],
        &durations,
        &alloc::vec![0; count],
        file_len,
    )?;
    if samples.len() != count {
        return Err("MP4 AAC incomplete chunk mapping");
    }
    Ok(Mp4Audio {
        samples,
        index: 0,
        timescale,
        decode_rate,
        edit,
        video_zero: 0,
        decoder,
    })
}

fn frames(time: i128, timescale: u32) -> Result<i64, &'static str> {
    i64::try_from((time * OUTPUT_RATE).div_euclid(i128::from(timescale)))
        .map_err(|_| "MP4 track timestamp overflow")
}

fn body(data: &[u8], box_: Mp4Box) -> &[u8] {
    &data[box_.payload_start..box_.end]
}

fn child(data: &[u8], parent: Mp4Box, typ: [u8; 4]) -> Result<Mp4Box, &'static str> {
    mp4_find_child(data, parent.payload_start, parent.end, typ).ok_or("MP4 soundtrack box missing")
}

fn header_timescale(data: &[u8], head: Mp4Box) -> Result<u32, &'static str> {
    let data = body(data, head);
    let offset = match data.first() {
        Some(0) => 12,
        Some(1) => 20,
        _ => return Err("MP4 track timescale header version unsupported"),
    };
    mp4_read_u32(data, offset)
        .filter(|&value| value != 0)
        .ok_or("MP4 track timescale missing")
}

fn track_edit(
    data: &[u8],
    track: Mp4Box,
    movie_timescale: Option<u32>,
) -> Result<TrackEdit, &'static str> {
    let Ok(edts) = child(data, track, *b"edts") else {
        return Ok(TrackEdit::default());
    };
    let elst = body(data, child(data, edts, *b"elst")?);
    let movie_timescale = movie_timescale.ok_or("MP4 movie timescale missing for track edit")?;
    let width = match elst.first() {
        Some(0) => 4,
        Some(1) => 8,
        _ => return Err("MP4 track edit version unsupported"),
    };
    let count = mp4_read_u32(elst, 4).ok_or("MP4 track edit count missing")? as usize;
    if !(1..=2).contains(&count) || elst.len() != 8 + (width * 2 + 4) * count {
        return Err("MP4 soundtrack requires one media edit and optional initial gap");
    }
    let mut edit = TrackEdit::default();
    for (index, row) in elst[8..].chunks_exact(width * 2 + 4).enumerate() {
        let (duration, start) = if width == 8 {
            (
                mp4_read_u64(row, 0).unwrap(),
                mp4_read_u64(row, width).unwrap() as i64,
            )
        } else {
            (
                u64::from(mp4_read_u32(row, 0).unwrap()),
                i64::from(mp4_read_u32(row, width).unwrap() as i32),
            )
        };
        if row[width * 2..] != [0, 1, 0, 0] {
            return Err("MP4 soundtrack track edit rate unsupported");
        }
        let duration = frames(i128::from(duration), movie_timescale)?;
        if count == 2 && index == 0 && start == -1 {
            edit.leading_frames = duration;
        } else if index == count - 1 && start >= 0 {
            edit.start = start;
            edit.length_frames = Some(duration);
        } else {
            return Err("MP4 soundtrack track edit unsupported");
        }
    }
    Ok(edit)
}

fn decoder_config(data: &[u8], stsd: Mp4Box) -> Result<(&[u8], u32), &'static str> {
    if mp4_read_u32(body(data, stsd), 4) != Some(1) {
        return Err("MP4 AAC requires one sample description");
    }
    let entry = mp4_next_box(data, stsd.payload_start + 8, stsd.end)
        .filter(|entry| entry.typ == *b"mp4a" && entry.end - entry.payload_start >= 28)
        .ok_or("MP4 soundtrack is not a supported mp4a entry")?;
    let fields = body(data, entry);
    if mp4_read_u16(fields, 6) != Some(1) || mp4_read_u16(fields, 8) != Some(0) {
        return Err("MP4 AAC sample entry version or reference unsupported");
    }
    let esds = mp4_find_child(data, entry.payload_start + 28, entry.end, *b"esds")
        .ok_or("MP4 AAC esds missing")?;
    let es = descriptor(body(data, esds).get(4..).unwrap_or(&[]), 3)?;
    let flags = *es.get(2).ok_or("MP4 AAC ES descriptor truncated")?;
    let mut child_start = 3 + if flags & 0x80 != 0 { 2 } else { 0 };
    if flags & 0x40 != 0 {
        let length = *es
            .get(child_start)
            .ok_or("MP4 AAC ES descriptor truncated")?;
        child_start += 1 + usize::from(length);
    }
    if flags & 0x20 != 0 {
        child_start += 2;
    }
    let decoder = descriptor(es.get(child_start..).unwrap_or(&[]), 4)?;
    if decoder.first() != Some(&0x40) {
        return Err("MP4 AAC decoder object type unsupported");
    }
    let config = descriptor(decoder.get(13..).unwrap_or(&[]), 5)?;
    let head = mp4_read_u16(config, 0).ok_or("MP4 AAC AudioSpecificConfig too short")?;
    if head >> 11 != 2
        || !(1..=2).contains(&((head >> 3) & 15))
        || (head & 7) != 0
        || config.len() > 64
    {
        return Err("MP4 soundtrack requires ordinary mono or stereo AAC-LC");
    }
    const RATES: [u32; 12] = [
        96_000, 88_200, 64_000, 48_000, 44_100, 32_000, 24_000, 22_050, 16_000, 12_000, 11_025,
        8_000,
    ];
    let rate = *RATES
        .get(((head >> 7) & 15) as usize)
        .ok_or("MP4 AAC sample rate unsupported")?;
    Ok((config, rate))
}

fn descriptor(mut input: &[u8], expected: u8) -> Result<&[u8], &'static str> {
    while let Some((&tag, rest)) = input.split_first() {
        input = rest;
        let mut size = 0;
        let mut length_finished = false;
        for _ in 0..4 {
            let (&length, rest) = input
                .split_first()
                .ok_or("MP4 AAC descriptor size missing")?;
            input = rest;
            size = (size << 7) | usize::from(length & 0x7f);
            if length & 0x80 == 0 {
                length_finished = true;
                break;
            }
        }
        if !length_finished {
            return Err("MP4 AAC descriptor size invalid");
        }
        let (payload, rest) = input
            .split_at_checked(size)
            .ok_or("MP4 AAC descriptor truncated")?;
        if tag == expected {
            return Ok(payload);
        }
        input = rest;
    }
    Err("MP4 AAC descriptor missing")
}
