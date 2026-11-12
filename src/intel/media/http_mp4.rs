//! Demand-driven HTTP MP4 reads. Encoded payload stays on the server until the
//! decoder needs it; the existing AVC decoder and surface lifetimes are shared.
use super::*;
use crate::r::net::https::{MediaFetchCancellation, MediaRangeIdentity};

const INDEX_WINDOW: usize = 64 * 1024;
const SAMPLE_WINDOW: usize = 1024 * 1024;
const BOX_LIMIT: usize = 65_536;

#[derive(Default)]
struct Window {
    offset: usize,
    bytes: Vec<u8>,
}
impl Window {
    fn get(&self, offset: usize, count: usize) -> Option<Vec<u8>> {
        let start = offset.checked_sub(self.offset)?;
        self.bytes
            .get(start..start.checked_add(count)?)
            .map(<[u8]>::to_vec)
    }
}

struct HttpSource {
    session: crate::ui4::VideoPlaybackSession,
    url: String,
    user_agent: Option<String>,
    referrer: Option<String>,
    identity: Option<MediaRangeIdentity>,
    len: usize,
    index: Window,
    samples: Window,
    bytes_fetched: usize,
}
impl HttpSource {
    async fn fetch(&mut self, offset: usize, count: usize) -> Result<Vec<u8>, &'static str> {
        if self.session.is_cancelled() {
            return Err("playback cancelled");
        }
        let cancellation = MediaFetchCancellation::new();
        let session = self.session;
        let response = {
            let fetch = crate::r::net::https::get_media_range_bytes_shared(
                &self.url,
                offset as u64,
                count,
                H264_ONLINE_MEDIA_FETCH_TIMEOUT_MS as u32,
                &cancellation,
                self.user_agent.as_deref(),
                self.referrer.as_deref(),
                self.identity.as_ref(),
            );
            let watch = async {
                while !session.is_cancelled() {
                    Timer::after_millis(5).await;
                }
                cancellation.cancel();
            };
            let mut fetch = core::pin::pin!(fetch);
            let mut watch = core::pin::pin!(watch);
            let mut cancelled = false;
            core::future::poll_fn(|cx| {
                use core::future::Future;
                if !cancelled && watch.as_mut().poll(cx).is_ready() {
                    cancelled = true;
                }
                // Cancellation must complete the HTTPS transport's TLS owner fence.
                fetch.as_mut().poll(cx)
            })
            .await
        };
        if session.is_cancelled() {
            return Err("playback cancelled");
        }
        let response = response.map_err(|error| {
            crate::log_warn!(target: "intel-media"; "video/http: range failed offset={} count={} reason={}\n", offset, count, error);
            "video HTTP range fetch failed"
        })?;
        if self.identity.is_none() {
            self.len = usize::try_from(response.identity.total_len)
                .map_err(|_| "video HTTP object too large")?;
            self.url = response.identity.final_url.clone();
            self.identity = Some(response.identity);
        }
        self.bytes_fetched = self.bytes_fetched.saturating_add(response.bytes.len());
        Ok(response.bytes)
    }
    async fn read(
        &mut self,
        offset: usize,
        count: usize,
        sample: bool,
    ) -> Result<Vec<u8>, &'static str> {
        let end = offset
            .checked_add(count)
            .filter(|&end| end <= self.len)
            .ok_or("video HTTP range outside file")?;
        if count == 0 {
            return Ok(Vec::new());
        }
        // An initial small 200 response may already contain the whole object;
        // either cache can also cover metadata adjoining a media read-ahead.
        if let Some(bytes) = self
            .samples
            .get(offset, count)
            .or_else(|| self.index.get(offset, count))
        {
            return Ok(bytes);
        }
        let budget = if sample { SAMPLE_WINDOW } else { INDEX_WINDOW };
        let n = count.max(budget).min(self.len - offset);
        let bytes = self.fetch(offset, n).await?;
        if bytes.len() != n {
            return Err("video HTTP range was short");
        }
        let window = Window { offset, bytes };
        let result = window
            .get(offset, end - offset)
            .ok_or("video HTTP range was short")?;
        if sample {
            self.samples = window;
        } else {
            self.index = window;
        }
        Ok(result)
    }
    async fn box_at(&mut self, offset: usize) -> Result<Mp4Box, &'static str> {
        let available = self
            .len
            .checked_sub(offset)
            .ok_or("mp4 box outside file")?
            .min(16);
        if available < 8 {
            return Err("mp4 truncated box header");
        }
        let header = self.read(offset, available, false).await?;
        let size32 = mp4_read_u32(&header, 0).ok_or("mp4 invalid box header")?;
        let (size, header_len) = if size32 == 1 {
            (
                usize::try_from(mp4_read_u64(&header, 8).ok_or("mp4 truncated extended header")?)
                    .map_err(|_| "mp4 box too large")?,
                16,
            )
        } else if size32 == 0 {
            (self.len - offset, 8)
        } else {
            (size32 as usize, 8)
        };
        let end = offset
            .checked_add(size)
            .filter(|&end| size >= header_len && end <= self.len)
            .ok_or("mp4 box outside file")?;
        Ok(Mp4Box {
            start: offset,
            payload_start: offset + header_len,
            end,
            typ: mp4_fourcc(&header, 4).ok_or("mp4 missing box type")?,
        })
    }
}

pub(super) struct HttpMp4Reader {
    source: HttpSource,
    pub(super) track: Mp4AvcTrack,
    pub(super) timing: Vec<H264SampleTiming>,
    fragmented: bool,
    trex: Option<Mp4TrexDefaults>,
    cursor: usize,
    sample: usize,
    decode_time: u64,
    boxes: usize,
}
impl HttpMp4Reader {
    pub(super) async fn open(
        session: crate::ui4::VideoPlaybackSession,
        url: &str,
        indexed_length: Option<usize>,
        user_agent: Option<&str>,
        referrer: Option<&str>,
    ) -> Result<Self, &'static str> {
        let started = EmbassyInstant::now();
        let mut source = HttpSource {
            session,
            url: url.into(),
            user_agent: user_agent.map(String::from),
            referrer: referrer.map(String::from),
            identity: None,
            len: 0,
            index: Window::default(),
            samples: Window::default(),
            bytes_fetched: 0,
        };
        let initial_count = indexed_length.unwrap_or(INDEX_WINDOW).min(INDEX_WINDOW);
        if initial_count == 0 {
            return Err("HLS MP4 empty indexed prefix");
        }
        let bytes = source.fetch(0, initial_count).await?;
        source.index = Window { offset: 0, bytes };
        if let Some(length) = indexed_length {
            if length > source.len {
                return Err("HLS MP4 resource shorter than indexed byte ranges");
            }
            source.len = length;
        }
        let mut cursor = 0;
        let mut moov_data = None;
        for _ in 0..BOX_LIMIT {
            if cursor == source.len {
                break;
            }
            let b = source.box_at(cursor).await?;
            if b.typ == *b"moov" {
                if b.end - b.start > H264_FS_METADATA_CAP_BYTES {
                    return Err("mp4 metadata exceeds streaming limit");
                }
                moov_data = Some(source.read(b.start, b.end - b.start, false).await?);
                break;
            }
            cursor = b.end; // Skip mdat by its header, including a tail moov.
        }
        let data = moov_data.ok_or("mp4 missing moov within box limit")?;
        let moov = mp4_next_box(&data, 0, data.len()).ok_or("mp4 invalid moov")?;
        let fragmented = mp4_find_child(&data, moov.payload_start, moov.end, *b"mvex").is_some();
        let mut track = None;
        for trak in mp4_collect_children(&data, moov.payload_start, moov.end, *b"trak") {
            if fragmented {
                if let Some(info) = mp4_parse_avc_track_info(&data, trak)? {
                    track = Some(Mp4AvcTrack {
                        track_id: info.track_id,
                        timescale: info.timescale,
                        length_size: info.length_size,
                        colour: info.colour,
                        sps: info.sps,
                        pps: info.pps,
                        samples: Vec::new(),
                    });
                    break;
                }
            } else if let Some(found) = mp4_parse_avc_track_with_len(&data, trak, source.len)? {
                track = Some(found);
                break;
            }
        }
        let track = track.ok_or("mp4 has no supported AVC track")?;
        let trex = mp4_parse_trex_defaults(&data, moov, track.track_id);
        let mut reader = Self {
            source,
            track,
            timing: Vec::new(),
            fragmented,
            trex,
            cursor: 0,
            sample: 0,
            decode_time: 0,
            boxes: 0,
        };
        if fragmented {
            if !reader.next_fragment().await? {
                return Err("mp4 fragmented video track has no samples");
            }
        } else {
            reader.append_timing()?;
        }
        crate::log_info!(target: "intel-media";
            "video/http: prepared incremental=1 fragmented={} file_bytes={} fetched_bytes={} indexed_pictures={} elapsed_ms={}\n",
            fragmented as u8, reader.source.len, reader.source.bytes_fetched, reader.timing.len(), started.elapsed().as_millis());
        Ok(reader)
    }
    fn append_timing(&mut self) -> Result<(), &'static str> {
        if self.timing.len().saturating_add(self.track.samples.len()) > 1_000_000 {
            return Err("mp4 streaming picture count limit");
        }
        let added: Vec<_> = self
            .track
            .samples
            .iter()
            .map(|s| H264SampleTiming {
                dts: s.decode_time,
                pts: i64::try_from(s.decode_time)
                    .unwrap_or(i64::MAX)
                    .saturating_add(s.composition_offset),
                duration: s.duration,
                timescale: self.track.timescale,
                colour: self.track.colour,
            })
            .collect();
        // The existing streaming presenter ranks a complete batch. Do not
        // silently misorder a fragment that crosses the prior batch's PTS.
        if let (Some(previous), Some(next)) = (
            self.timing.iter().map(|t| t.pts).max(),
            added.iter().map(|t| t.pts).min(),
        ) {
            if next <= previous {
                return Err("mp4 cross-fragment PTS inversion unsupported");
            }
        }
        self.timing.extend(added);
        Ok(())
    }
    async fn next_fragment(&mut self) -> Result<bool, &'static str> {
        while self.cursor < self.source.len {
            self.boxes += 1;
            if self.boxes > BOX_LIMIT {
                return Err("mp4 streaming box count limit");
            }
            let b = self.source.box_at(self.cursor).await?;
            self.cursor = b.end;
            if b.typ != *b"moof" {
                continue;
            }
            if b.end - b.start > H264_FS_METADATA_CAP_BYTES {
                return Err("mp4 fragment metadata exceeds streaming limit");
            }
            let data = self.source.read(b.start, b.end - b.start, false).await?;
            let local = mp4_next_box(&data, 0, data.len()).ok_or("mp4 invalid moof")?;
            let mut mdat = None;
            while self.cursor < self.source.len {
                self.boxes += 1;
                if self.boxes > BOX_LIMIT {
                    return Err("mp4 streaming box count limit");
                }
                let following = self.source.box_at(self.cursor).await?;
                if following.typ == *b"moof" {
                    break;
                }
                self.cursor = following.end;
                if following.typ == *b"mdat" {
                    mdat = Some(following);
                    break;
                }
            }
            let mdat = mdat.ok_or("mp4 fragment missing following mdat")?;
            let mut samples = Vec::new();
            for (traf_index, traf) in
                mp4_collect_children(&data, local.payload_start, local.end, *b"traf")
                    .into_iter()
                    .enumerate()
            {
                let tfhd_box = mp4_find_child(&data, traf.payload_start, traf.end, *b"tfhd")
                    .ok_or("mp4 fragment missing tfhd")?;
                let mut tfhd = mp4_parse_tfhd(&data, tfhd_box)?;
                if tfhd.track_id != self.track.track_id {
                    continue;
                }
                if tfhd.base_data_offset.is_none() && tfhd.flags & 0x020000 == 0 && traf_index != 0
                {
                    return Err("mp4 implicit cross-track fragment base unsupported");
                }
                if let Some(trex) = self.trex {
                    tfhd.default_sample_duration =
                        tfhd.default_sample_duration.or(Some(trex.duration));
                    tfhd.default_sample_size = tfhd.default_sample_size.or(Some(trex.size));
                    tfhd.default_sample_flags = tfhd.default_sample_flags.or(Some(trex.flags));
                }
                if let Some(tfdt) = mp4_find_child(&data, traf.payload_start, traf.end, *b"tfdt") {
                    self.decode_time = mp4_parse_tfdt(&data, tfdt)?;
                }
                // With no trun data_offset, the first run starts at an
                // explicit tfhd base. Later runs continue after the prior run.
                let mut fallback = usize::try_from(tfhd.base_data_offset.unwrap_or(mdat.payload_start as u64))
                    .map_err(|_| "mp4 fragment base outside address space")?;
                for trun in mp4_collect_children(&data, traf.payload_start, traf.end, *b"trun") {
                    let batch = mp4_parse_trun_samples_with_len(
                        &data,
                        Mp4Box {
                            start: b.start,
                            ..local
                        },
                        trun,
                        &tfhd,
                        fallback,
                        &mut self.decode_time,
                        self.source.len,
                    )?;
                    if batch.iter().any(|s| {
                        s.offset < mdat.payload_start
                            || s.offset
                                .checked_add(s.size)
                                .is_none_or(|end| end > mdat.end)
                    }) {
                        return Err("mp4 fragment sample outside mdat");
                    }
                    if let Some(last) = batch.last() {
                        fallback = last.offset + last.size;
                    }
                    if samples.len().saturating_add(batch.len()) > 1_000_000 {
                        return Err("mp4 fragment sample count limit");
                    }
                    samples.extend(batch);
                }
            }
            if samples.is_empty() {
                continue;
            }
            self.track.samples = samples;
            self.sample = 0;
            self.append_timing()?;
            return Ok(true);
        }
        Ok(false)
    }
    pub(super) async fn next_sample(
        &mut self,
    ) -> Result<Option<(Vec<u8>, H264SampleTiming)>, &'static str> {
        if self.source.session.is_cancelled() {
            return Err("playback cancelled");
        }
        if self.sample == self.track.samples.len()
            && (!self.fragmented || !self.next_fragment().await?)
        {
            return Ok(None);
        }
        let sample = self.track.samples[self.sample];
        if sample.size == 0 || sample.size > H264_FS_PICTURE_CAP_BYTES {
            return Err("mp4 sample exceeds decoder picture limit");
        }
        let payload = self.source.read(sample.offset, sample.size, true).await?;
        let mut bytes = Vec::new();
        mp4_emit_annexb_aud(&mut bytes);
        if self.sample == 0 || sample.keyframe {
            for nal in self.track.sps.iter().chain(&self.track.pps) {
                mp4_emit_annexb_nal(&mut bytes, nal);
            }
        }
        let mut cursor = 0;
        while cursor < payload.len() {
            let n = match self.track.length_size {
                1 => payload.get(cursor).map(|&v| v as usize),
                2 => mp4_read_u16(&payload, cursor).map(usize::from),
                4 => mp4_read_u32(&payload, cursor).map(|v| v as usize),
                _ => return Err("mp4 unsupported AVC length size"),
            }
            .ok_or("mp4 truncated NAL length")?;
            cursor += self.track.length_size;
            let end = cursor
                .checked_add(n)
                .filter(|&end| end <= payload.len())
                .ok_or("mp4 NAL outside sample")?;
            if n != 0 {
                mp4_emit_annexb_nal(&mut bytes, &payload[cursor..end]);
            }
            cursor = end;
        }
        // Emit this sample before requesting the next network sample.
        mp4_emit_annexb_aud(&mut bytes);
        self.sample += 1;
        Ok(Some((
            bytes,
            H264SampleTiming {
                dts: sample.decode_time,
                pts: i64::try_from(sample.decode_time)
                    .unwrap_or(i64::MAX)
                    .saturating_add(sample.composition_offset),
                duration: sample.duration,
                timescale: self.track.timescale,
                colour: self.track.colour,
            },
        )))
    }
}
