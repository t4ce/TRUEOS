//! Matroska AVC + little-endian PCM. RFC 9559 and Matroska codec mappings:
//! https://www.rfc-editor.org/rfc/rfc9559.html
//! https://www.matroska.org/technical/codec_specs.html
//! Two bounded passes over pinned source records; no whole recording in RAM.
use super::Recording;
use crate::r::fs::trueosfs as fs;
use alloc::{format, string::String, vec, vec::Vec};

const AUDIO_PACKET_FRAMES: u64 = 960; // 20 ms at 48 kHz
const MAX_FRAME: usize = 4 * 1024 * 1024;

fn id(out: &mut Vec<u8>, value: u32) {
    let bytes = value.to_be_bytes();
    out.extend_from_slice(&bytes[bytes.iter().position(|&b| b != 0).unwrap_or(3)..]);
}
fn size(out: &mut Vec<u8>, value: u64) {
    let width = (1..=8)
        .find(|width| value < (1u64 << (7 * width)) - 1)
        .expect("bounded EBML size");
    let marked = value | (1u64 << (7 * width));
    out.extend_from_slice(&marked.to_be_bytes()[8 - width..]);
}
fn element(out: &mut Vec<u8>, tag: u32, bytes: &[u8]) {
    id(out, tag);
    size(out, bytes.len() as u64);
    out.extend_from_slice(bytes);
}
fn uint(out: &mut Vec<u8>, tag: u32, value: u64) {
    let bytes = value.to_be_bytes();
    element(out, tag, &bytes[bytes.iter().position(|&b| b != 0).unwrap_or(7)..]);
}
fn float(out: &mut Vec<u8>, tag: u32, value: f64) {
    element(out, tag, &value.to_be_bytes());
}
fn string(out: &mut Vec<u8>, tag: u32, value: &str) {
    element(out, tag, value.as_bytes());
}

fn nals(bytes: &[u8]) -> Result<Vec<&[u8]>, &'static str> {
    let mut ranges = Vec::new();
    let mut index = 0;
    while index + 3 <= bytes.len() {
        let length = if bytes[index..].starts_with(&[0, 0, 0, 1]) {
            4
        } else if bytes[index..].starts_with(&[0, 0, 1]) {
            3
        } else {
            index += 1;
            continue;
        };
        ranges.push((index, index + length));
        index += length;
    }
    if ranges.is_empty() || bytes[..ranges[0].0].iter().any(|&b| b != 0) {
        return Err("Invalid Annex-B video");
    }
    let mut result = Vec::new();
    for (i, &(_, start)) in ranges.iter().enumerate() {
        let mut end = ranges.get(i + 1).map_or(bytes.len(), |range| range.0);
        while end > start && bytes[end - 1] == 0 {
            end -= 1;
        }
        if end == start || bytes[start] & 0x80 != 0 {
            return Err("Invalid video NAL unit");
        }
        result.push(&bytes[start..end]);
    }
    Ok(result)
}
fn codec_private(bytes: &[u8]) -> Result<Vec<u8>, &'static str> {
    let nals = nals(bytes)?;
    let sps = nals
        .iter()
        .find(|nal| nal[0] & 31 == 7)
        .ok_or("Video has no SPS")?;
    let pps = nals
        .iter()
        .find(|nal| nal[0] & 31 == 8)
        .ok_or("Video has no PPS")?;
    // The proven encoder emits baseline AVC with no B frames. Refuse another
    // profile rather than inventing high-profile avcC extension fields.
    if sps.len() < 4
        || sps[1] != 66
        || sps.len() > u16::MAX as usize
        || pps.len() > u16::MAX as usize
    {
        return Err("Unsupported AVC configuration");
    }
    let mut out = vec![1, sps[1], sps[2], sps[3], 0xff, 0xe1];
    out.extend_from_slice(&(sps.len() as u16).to_be_bytes());
    out.extend_from_slice(sps);
    out.push(1);
    out.extend_from_slice(&(pps.len() as u16).to_be_bytes());
    out.extend_from_slice(pps);
    Ok(out)
}
fn packet(bytes: &[u8]) -> Result<(Vec<u8>, bool), &'static str> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut key = false;
    let mut picture = false;
    for nal in nals(bytes)? {
        key |= nal[0] & 31 == 5;
        picture |= matches!(nal[0] & 31, 1 | 5);
        out.extend_from_slice(&(nal.len() as u32).to_be_bytes());
        out.extend_from_slice(nal);
    }
    if !picture {
        return Err("Video access unit has no picture");
    }
    Ok((out, key))
}

fn header(
    private: &[u8],
    width: u32,
    height: u32,
    channels: u8,
    duration_ms: f64,
) -> (Vec<u8>, usize) {
    let mut ebml = Vec::new();
    uint(&mut ebml, 0x4286, 1);
    uint(&mut ebml, 0x42f7, 1);
    uint(&mut ebml, 0x42f2, 4);
    uint(&mut ebml, 0x42f3, 8);
    string(&mut ebml, 0x4282, "matroska");
    uint(&mut ebml, 0x4287, 4);
    uint(&mut ebml, 0x4285, 2);
    let mut out = Vec::new();
    element(&mut out, 0x1a45dfa3, &ebml);
    id(&mut out, 0x18538067);
    out.extend_from_slice(&[1, 255, 255, 255, 255, 255, 255, 255]);
    let segment_start = out.len();
    let mut info = Vec::new();
    uint(&mut info, 0x2ad7b1, 1_000_000);
    string(&mut info, 0x4d80, "TRUEOS capture");
    string(&mut info, 0x5741, "TRUEOS capture");
    float(&mut info, 0x4489, duration_ms);
    element(&mut out, 0x1549a966, &info);
    let mut video = Vec::new();
    uint(&mut video, 0xd7, 1);
    uint(&mut video, 0x73c5, 1);
    uint(&mut video, 0x83, 1);
    uint(&mut video, 0x9c, 0);
    string(&mut video, 0x86, "V_MPEG4/ISO/AVC");
    element(&mut video, 0x63a2, private);
    let mut dimensions = Vec::new();
    uint(&mut dimensions, 0xb0, u64::from(width));
    uint(&mut dimensions, 0xba, u64::from(height));
    element(&mut video, 0xe0, &dimensions);
    let mut audio = Vec::new();
    uint(&mut audio, 0xd7, 2);
    uint(&mut audio, 0x73c5, 2);
    uint(&mut audio, 0x83, 2);
    uint(&mut audio, 0x9c, 0);
    string(&mut audio, 0x86, "A_PCM/INT/LIT");
    let mut format = Vec::new();
    float(&mut format, 0xb5, 48_000.0);
    uint(&mut format, 0x9f, u64::from(channels));
    uint(&mut format, 0x6264, 16);
    element(&mut audio, 0xe1, &format);
    let mut tracks = Vec::new();
    element(&mut tracks, 0xae, &video);
    element(&mut tracks, 0xae, &audio);
    element(&mut out, 0x1654ae6b, &tracks);
    (out, segment_start)
}
fn cluster_prefix(track: u8, timestamp_ms: u64, key: bool, payload_len: usize) -> Vec<u8> {
    // Each packet has its own finite cluster and zero relative timestamp.
    // This avoids the signed 16-bit Block timestamp limit at 15 minutes.
    let mut time = Vec::new();
    uint(&mut time, 0xe7, timestamp_ms);
    let mut block = Vec::new();
    id(&mut block, 0xa3);
    size(&mut block, payload_len as u64 + 4);
    block.extend_from_slice(&[0x80 | track, 0, 0, if key { 0x80 } else { 0 }]);
    let mut out = Vec::new();
    id(&mut out, 0x1f43b675);
    size(&mut out, (time.len() + block.len() + payload_len) as u64);
    out.extend(time);
    out.extend(block);
    out
}
fn cues(entries: &[(u64, u64)]) -> Vec<u8> {
    let mut contents = Vec::new();
    for &(time, position) in entries {
        let mut point = Vec::new();
        uint(&mut point, 0xb3, time);
        let mut track = Vec::new();
        uint(&mut track, 0xf7, 1);
        uint(&mut track, 0xf1, position);
        element(&mut point, 0xb7, &track);
        element(&mut contents, 0xbb, &point);
    }
    let mut out = Vec::new();
    element(&mut out, 0x1c53bb6b, &contents);
    out
}

async fn read(file: fs::FileReadHandle, offset: u64, out: &mut [u8]) -> Result<(), &'static str> {
    if fs::file_read_handle_range_async(file, offset, out)
        .await
        .map_err(|_| "Cannot read mux source")?
        != Some(out.len())
    {
        return Err("Incomplete mux source");
    }
    Ok(())
}
async fn write(handle: u32, bytes: &[u8]) -> Result<(), &'static str> {
    // Keep each filesystem copy bounded even for a large encoded video AU.
    for chunk in bytes.chunks(1024 * 1024) {
        fs::file_write_chunk_async(handle, chunk)
            .await
            .map_err(|_| "Cannot write combined recording")?;
    }
    Ok(())
}
struct Frame {
    offset: u64,
    bytes: usize,
    timestamp_ms: u64,
    packet_bytes: usize,
    key: bool,
}
struct Plan {
    frames: Vec<Frame>,
    audio_frames: u64,
    audio_offset_ms: u64,
    channels: u8,
    header: Vec<u8>,
    cues: Vec<u8>,
    total: u64,
}
impl Plan {
    fn audio_packet(&self, frame: u64) -> (u64, usize) {
        (
            self.audio_offset_ms + frame * 1000 / 48_000,
            self.audio_frames
                .saturating_sub(frame)
                .min(AUDIO_PACKET_FRAMES) as usize
                * usize::from(self.channels)
                * 2,
        )
    }
    fn lengths(&mut self, segment_start: usize) {
        let mut position = self.header.len() as u64 - segment_start as u64;
        let mut cue_entries = Vec::new();
        let mut video = 0;
        let mut audio = 0;
        while video < self.frames.len() || audio < self.audio_frames {
            let (audio_ms, audio_bytes) = self.audio_packet(audio);
            if video < self.frames.len()
                && (audio >= self.audio_frames || self.frames[video].timestamp_ms <= audio_ms)
            {
                let frame = &self.frames[video];
                if frame.key {
                    cue_entries.push((frame.timestamp_ms, position));
                }
                position += (cluster_prefix(1, frame.timestamp_ms, frame.key, frame.packet_bytes)
                    .len()
                    + frame.packet_bytes) as u64;
                video += 1;
            } else {
                position +=
                    (cluster_prefix(2, audio_ms, true, audio_bytes).len() + audio_bytes) as u64;
                audio += AUDIO_PACKET_FRAMES;
            }
        }
        self.cues = cues(&cue_entries);
        self.total = segment_start as u64 + position + self.cues.len() as u64;
    }
}
async fn plan(
    video: &Recording,
    audio: &Recording,
    vfile: fs::FileReadHandle,
    afile: fs::FileReadHandle,
) -> Result<Plan, &'static str> {
    let frames = core::mem::take(&mut video.status.lock().frames);
    if frames.is_empty() {
        return Err("Video has no frames to combine");
    }
    let audio_started = audio.status.lock().started_ns;
    let epoch = frames[0].1.min(audio_started);
    let mut wav = [0; 44];
    read(afile, 0, &mut wav).await?;
    let channels = u16::from_le_bytes(wav[22..24].try_into().unwrap());
    if &wav[..4] != b"RIFF"
        || &wav[8..16] != b"WAVEfmt "
        || &wav[36..40] != b"data"
        || u32::from_le_bytes(wav[16..20].try_into().unwrap()) != 16
        || u16::from_le_bytes(wav[20..22].try_into().unwrap()) != 1
        || !matches!(channels, 1 | 2)
        || u32::from_le_bytes(wav[24..28].try_into().unwrap()) != 48_000
        || u16::from_le_bytes(wav[34..36].try_into().unwrap()) != 16
    {
        return Err("Unsupported microphone WAV format");
    }
    let pcm_bytes = u64::from(u32::from_le_bytes(wav[40..44].try_into().unwrap()));
    if pcm_bytes == 0
        || afile.data_len() != pcm_bytes + 44
        || pcm_bytes % (u64::from(channels) * 2) != 0
    {
        return Err("Incomplete microphone WAV");
    }
    let audio_frames = pcm_bytes / (u64::from(channels) * 2);
    let audio_offset_ms = audio_started.saturating_sub(epoch) / 1_000_000;
    let mut parsed = Vec::with_capacity(frames.len());
    let mut offset = 0;
    let mut configuration = Vec::new();
    let mut previous_ns = 0;
    let mut buffer = Vec::new();
    for (index, &(bytes, ns)) in frames.iter().enumerate() {
        if bytes == 0
            || bytes > MAX_FRAME
            || offset + bytes as u64 > vfile.data_len()
            || ns < previous_ns
        {
            return Err("Invalid video frame index");
        }
        previous_ns = ns;
        buffer.resize(bytes, 0);
        read(vfile, offset, &mut buffer).await?;
        let (packet, key) = packet(&buffer)?;
        if index == 0 {
            if !key {
                return Err("Video does not start with a keyframe");
            }
            configuration = codec_private(&buffer)?;
        } else if nals(&buffer)?.iter().any(|nal| nal[0] & 31 == 7)
            && codec_private(&buffer)? != configuration
        {
            return Err("AVC configuration changed during capture");
        }
        parsed.push(Frame {
            offset,
            bytes,
            timestamp_ms: ns.saturating_sub(epoch) / 1_000_000,
            packet_bytes: packet.len(),
            key,
        });
        offset += bytes as u64;
    }
    if offset != vfile.data_len() {
        return Err("Video frame index does not cover recording");
    }
    let duration_ms = (audio_offset_ms as f64 + audio_frames as f64 / 48.0).max(
        parsed.last().unwrap().timestamp_ms as f64
            + 1000.0 / crate::allcaps::media_encode::REALTIME_HZ as f64,
    );
    let (header, segment_start) = header(
        &configuration,
        crate::intel::media::avc_encode_probe::FRAME_WIDTH as u32,
        crate::intel::media::avc_encode_probe::FRAME_HEIGHT as u32,
        channels as u8,
        duration_ms,
    );
    let mut plan = Plan {
        frames: parsed,
        audio_frames,
        audio_offset_ms,
        channels: channels as u8,
        header,
        cues: Vec::new(),
        total: 0,
    };
    plan.lengths(segment_start);
    Ok(plan)
}
async fn emit(
    plan: &Plan,
    vfile: fs::FileReadHandle,
    afile: fs::FileReadHandle,
    handle: u32,
) -> Result<(), &'static str> {
    write(handle, &plan.header).await?;
    let mut video = 0;
    let mut audio = 0;
    let mut buffer = Vec::new();
    while video < plan.frames.len() || audio < plan.audio_frames {
        let (audio_ms, audio_bytes) = plan.audio_packet(audio);
        if video < plan.frames.len()
            && (audio >= plan.audio_frames || plan.frames[video].timestamp_ms <= audio_ms)
        {
            let frame = &plan.frames[video];
            buffer.resize(frame.bytes, 0);
            read(vfile, frame.offset, &mut buffer).await?;
            let (packet, key) = packet(&buffer)?;
            write(handle, &cluster_prefix(1, frame.timestamp_ms, key, packet.len())).await?;
            write(handle, &packet).await?;
            video += 1;
        } else {
            buffer.resize(audio_bytes, 0);
            read(afile, 44 + audio * u64::from(plan.channels) * 2, &mut buffer).await?;
            write(handle, &cluster_prefix(2, audio_ms, true, audio_bytes)).await?;
            write(handle, &buffer).await?;
            audio += AUDIO_PACKET_FRAMES;
        }
    }
    write(handle, &plan.cues).await
}
pub(super) async fn save(video: &Recording, audio: &Recording) -> Result<String, &'static str> {
    let vfile = fs::file_read_open_async(video.disk, &video.path)
        .await
        .map_err(|_| "Cannot open video")?
        .ok_or("Video is missing")?;
    let afile = fs::file_read_open_async(audio.disk, &audio.path)
        .await
        .map_err(|_| "Cannot open audio")?
        .ok_or("Audio is missing")?;
    let plan = plan(video, audio, vfile, afile).await?;
    let name = video
        .path
        .rsplit('/')
        .next()
        .unwrap_or("capture.h264")
        .trim_end_matches(".h264");
    let path = format!("captures/{name}.mkv");
    if !matches!(fs::dir_create_all_async(video.disk, "captures").await, Ok(true)) {
        return Err("Cannot create captures directory");
    }
    let handle = fs::file_write_begin_async(video.disk, &path, plan.total)
        .await
        .map_err(|_| "Cannot open combined recording")?
        .ok_or("No space for combined recording")?;
    if let Err(error) = emit(&plan, vfile, afile, handle).await {
        let _ = fs::file_write_abort_async(handle).await;
        return Err(error);
    }
    if fs::file_write_finish_async(handle).await.is_err() {
        let _ = fs::file_write_abort_async(handle).await;
        return Err("Combined recording commit failed");
    }
    let verified = fs::file_read_open_async(video.disk, &path)
        .await
        .map_err(|_| "Cannot verify combined recording")?
        .ok_or("Combined recording is missing")?;
    if verified.data_len() != plan.total {
        return Err("Combined recording length verification failed");
    }
    // The only user-facing artifact is the verified two-track MKV. On any
    // earlier failure, proven source outputs remain available for recovery.
    let _ = fs::file_delete_async(video.disk, &video.path).await;
    let _ = fs::file_delete_async(audio.disk, &audio.path).await;
    Ok(path)
}
