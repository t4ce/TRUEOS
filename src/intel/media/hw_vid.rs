use alloc::{format, string::String, vec::Vec};
use core::fmt::Write;
use embassy_time::{Duration as EmbassyDuration, Timer};

const H264_BOOT_PROBE_ENABLED: bool = true;
const H264_BOOT_PROBE_PLAYBACK_ENABLED: bool = true;
const H264_BOOT_PROBE_PLAYBACK_MODE: H264PlaybackMode = H264PlaybackMode {
    reverse_after_forward: true,
};
const H264_BOOT_PROBE_PLAYBACK_FRAME_MS: u64 = 33;
const H264_BOOT_PROBE_FORWARD_FULL_CACHE: bool = true;
const H264_BOOT_PROBE_REVERSE_SHOW_CACHE_FILL: bool = false;
const H264_BOOT_PROBE_STRIPE_STUDY_ENABLED: bool = true;
const H264_BOOT_PROBE_STRIPE_STUDY_FRAME_MS: u64 = 120;
const H264_BOOT_PROBE_STRIPE_STUDY_STORE_TOP: usize = 8;
const H264_BOOT_PROBE_STREAM_LOAD_TIMEOUT_MS: u64 = 20_000;
const H264_BOOT_PROBE_STREAM_LOAD_POLL_MS: u64 = 250;
const H264_BOOT_PROBE_STREAM_CHUNK_BYTES: usize = 64 * 1024;
const H264_BOOT_PROBE_TIMEOUT_MS: u64 = 5_000;
const H264_BOOT_PROBE_DELAY_MS: u64 = 2_000;
const H264_BOOT_PROBE_STREAM_PATH: &str = "x31_head_movie.annexb.h264";

#[derive(Copy, Clone, Debug)]
struct H264PlaybackMode {
    reverse_after_forward: bool,
}

impl H264PlaybackMode {
    const fn reverse_after_forward(self) -> bool {
        self.reverse_after_forward
    }

    const fn name(self) -> &'static str {
        if self.reverse_after_forward {
            "forward-then-reverse"
        } else {
            "forward"
        }
    }
}

pub(crate) const fn probe_enabled() -> bool {
    H264_BOOT_PROBE_ENABLED
}

#[embassy_executor::task]
pub(crate) async fn hw_vid_probe_task() {
    if !H264_BOOT_PROBE_ENABLED {
        crate::log!("intel/hw_vid: probe disabled reason=h264-boot-probe-disabled\n");
        return;
    }
    if !crate::intel::has_media_decode_engine() {
        crate::log!("intel/hw_vid: probe skipped reason=no-media-decode-engine\n");
        return;
    }

    Timer::after(EmbassyDuration::from_millis(H264_BOOT_PROBE_DELAY_MS)).await;
    if H264_BOOT_PROBE_PLAYBACK_ENABLED {
        if let Some(file) = h264_wait_for_playback_stream().await {
            h264_i_p_playback_probe(file, H264_BOOT_PROBE_PLAYBACK_MODE).await;
        } else {
            crate::log!(
                "intel/hw_vid: h264-playback-probe skipped reason=stream-file-unavailable path={} action=require-trueosfs-file\n",
                H264_BOOT_PROBE_STREAM_PATH
            );
        }
        return;
    }

    crate::log!(
        "intel/hw_vid: probe skipped reason=playback-disabled-and-no-embedded-first-frame path={}\n",
        H264_BOOT_PROBE_STREAM_PATH
    );
}

#[derive(Copy, Clone, Debug)]
struct H264StreamNal {
    stream_offset: u64,
    bytes: usize,
    nal_type: u8,
}

struct H264BufferedNal {
    meta: H264StreamNal,
    bytes: Vec<u8>,
}

struct H264IndexedFrame {
    stream_offset: u64,
    bytes: usize,
    nal_type: u8,
    stream_idr_index: usize,
    decode_start_frame: usize,
    detail: Option<super::h264_cmd::AvcFrameDebug>,
    sps: Vec<u8>,
    pps: Vec<u8>,
}

struct H264DecodedFrame {
    bytes: Vec<u8>,
    width: u32,
    height: u32,
    visible_width: u32,
    visible_height: u32,
    pitch_bytes: usize,
    uv_offset: usize,
}

struct H264ForwardTailCache {
    start_frame: usize,
    frames: Vec<H264DecodedFrame>,
}

enum H264ForwardCache {
    Full(Vec<H264DecodedFrame>),
    Tail(H264ForwardTailCache),
}

struct H264StripeStudyCandidate {
    metric: u64,
    source_index: usize,
    reverse_step: usize,
    snapshot: crate::intel::display::PrimarySurfaceBgra8Snapshot,
}

struct H264RangeNalReader {
    file: crate::r::fs::trueosfs::FileReadHandle,
    path: &'static str,
    file_size: u64,
    file_offset: u64,
    buffer_base: u64,
    scan_offset: usize,
    buffer: Vec<u8>,
    eof: bool,
}

impl H264RangeNalReader {
    fn new(
        file: crate::r::fs::trueosfs::FileReadHandle,
        path: &'static str,
        file_size: u64,
    ) -> Self {
        Self {
            file,
            path,
            file_size,
            file_offset: 0,
            buffer_base: 0,
            scan_offset: 0,
            buffer: Vec::with_capacity(H264_BOOT_PROBE_STREAM_CHUNK_BYTES * 2),
            eof: false,
        }
    }

    async fn next_nal(&mut self) -> Option<H264BufferedNal> {
        loop {
            if let Some(nal) = self.try_take_nal() {
                return Some(nal);
            }
            if self.eof {
                return None;
            }
            if !self.read_more().await {
                self.eof = true;
            }
        }
    }

    async fn read_more(&mut self) -> bool {
        if self.file_offset >= self.file_size {
            return false;
        }
        let remaining = self.file_size.saturating_sub(self.file_offset);
        let want = remaining.min(H264_BOOT_PROBE_STREAM_CHUNK_BYTES as u64) as usize;
        let old_len = self.buffer.len();
        self.buffer.resize(old_len + want, 0);
        let read = match crate::r::fs::trueosfs::file_read_handle_range_async(
            self.file,
            self.file_offset,
            &mut self.buffer[old_len..old_len + want],
        )
        .await
        {
            Ok(Some(read)) => read,
            Ok(None) => 0,
            Err(err) => {
                crate::log!(
                    "intel/hw_vid: h264-playback-probe stream-read failed path={} offset=0x{:X} want=0x{:X} err={:?}\n",
                    self.path,
                    self.file_offset,
                    want,
                    err
                );
                0
            }
        };
        self.buffer.truncate(old_len + read);
        if read == 0 {
            return false;
        }
        self.file_offset = self.file_offset.saturating_add(read as u64);
        true
    }

    fn try_take_nal(&mut self) -> Option<H264BufferedNal> {
        loop {
            let (start, start_code_len) = match h264_find_start_code(&self.buffer, self.scan_offset)
            {
                Some(found) => found,
                None => {
                    self.discard_start_code_search_prefix();
                    return None;
                }
            };
            let payload_start = start + start_code_len;
            let next = h264_find_start_code(&self.buffer, payload_start);
            let end = if let Some((next_start, _)) = next {
                next_start
            } else if self.eof {
                self.buffer.len()
            } else {
                self.scan_offset = start;
                return None;
            };

            self.scan_offset = end;
            if payload_start < end && payload_start < self.buffer.len() {
                let mut bytes = Vec::with_capacity(end - start);
                bytes.extend_from_slice(&self.buffer[start..end]);
                let nal_type = self.buffer[payload_start] & 0x1f;
                let stream_offset = self.buffer_base.saturating_add(start as u64);
                self.drain_before(end);
                return Some(H264BufferedNal {
                    meta: H264StreamNal {
                        stream_offset,
                        bytes: end - start,
                        nal_type,
                    },
                    bytes,
                });
            }

            if end > start {
                self.drain_before(end);
            } else {
                self.scan_offset = self.scan_offset.saturating_add(1);
            }
        }
    }

    fn discard_start_code_search_prefix(&mut self) {
        if self.buffer.len() > 3 {
            let keep = 3usize;
            let drain = self.buffer.len() - keep;
            self.drain_before(drain);
        }
    }

    fn drain_before(&mut self, end: usize) {
        let drain = end.min(self.buffer.len());
        if drain == 0 {
            return;
        }
        self.buffer.drain(0..drain);
        self.buffer_base = self.buffer_base.saturating_add(drain as u64);
        self.scan_offset = self.scan_offset.saturating_sub(drain);
    }
}

async fn h264_wait_for_playback_stream() -> Option<crate::r::fs::trueosfs::FileReadHandle> {
    let mut waited_ms = 0u64;
    let mut attempts = 0usize;
    let mut last_reason = "not-tried";

    loop {
        attempts += 1;
        if let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() {
            match crate::r::fs::trueosfs::file_read_open_async(disk, H264_BOOT_PROBE_STREAM_PATH)
                .await
            {
                Ok(Some(file)) => {
                    crate::log!(
                        "intel/hw_vid: h264-playback-probe stream-open accepted=1 path={} bytes={} data_lba={} source=trueosfs-root mode=open-handle-range-stream attempts={} waited_ms={}\n",
                        H264_BOOT_PROBE_STREAM_PATH,
                        file.data_len(),
                        file.data_lba(),
                        attempts,
                        waited_ms
                    );
                    return Some(file);
                }
                Ok(None) => last_reason = "file-missing",
                Err(err) => {
                    crate::log!(
                        "intel/hw_vid: h264-playback-probe stream-open retry path={} attempt={} waited_ms={} err={:?}\n",
                        H264_BOOT_PROBE_STREAM_PATH,
                        attempts,
                        waited_ms,
                        err
                    );
                    last_reason = "read-error";
                }
            }
        } else {
            last_reason = "no-trueosfs-root";
        }

        if waited_ms >= H264_BOOT_PROBE_STREAM_LOAD_TIMEOUT_MS {
            crate::log!(
                "intel/hw_vid: h264-playback-probe stream-open accepted=0 path={} reason={} attempts={} waited_ms={} timeout_ms={}\n",
                H264_BOOT_PROBE_STREAM_PATH,
                last_reason,
                attempts,
                waited_ms,
                H264_BOOT_PROBE_STREAM_LOAD_TIMEOUT_MS
            );
            return None;
        }

        Timer::after(EmbassyDuration::from_millis(H264_BOOT_PROBE_STREAM_LOAD_POLL_MS)).await;
        waited_ms = waited_ms.saturating_add(H264_BOOT_PROBE_STREAM_LOAD_POLL_MS);
    }
}

async fn h264_i_p_playback_probe(
    file: crate::r::fs::trueosfs::FileReadHandle,
    mode: H264PlaybackMode,
) {
    let stream_bytes = file.data_len();
    let mut reader = H264RangeNalReader::new(file, H264_BOOT_PROBE_STREAM_PATH, stream_bytes);
    let mut nal_count = 0usize;
    let mut idr_seen = 0usize;
    let mut p_seen = 0usize;
    let mut submitted = 0usize;
    let mut skipped_missing_headers = 0usize;
    let mut last_sps: Option<Vec<u8>> = None;
    let mut last_pps: Option<Vec<u8>> = None;
    let mut indexed_frames = Vec::new();
    let mut last_idr_frame: Option<usize> = None;
    let forward_full_cache_enabled =
        mode.reverse_after_forward() && H264_BOOT_PROBE_FORWARD_FULL_CACHE;
    let mut forward_full_cache = Vec::new();
    let mut forward_tail_start_frame = 0usize;
    let mut forward_tail_cache = Vec::new();
    let mut stopped_at = 0u64;

    crate::log!(
        "intel/hw_vid: h264-playback-probe start bytes={} frame_ms={} subset=idr-plus-p source=trueosfs-root path={} mode=range-stream chunk=0x{:X} playback_mode={} stop=eos\n",
        stream_bytes,
        H264_BOOT_PROBE_PLAYBACK_FRAME_MS,
        H264_BOOT_PROBE_STREAM_PATH,
        H264_BOOT_PROBE_STREAM_CHUNK_BYTES,
        mode.name()
    );

    while let Some(nal) = reader.next_nal().await {
        stopped_at = nal.meta.stream_offset.saturating_add(nal.meta.bytes as u64);
        nal_count += 1;
        match nal.meta.nal_type {
            7 => last_sps = Some(nal.bytes),
            8 => last_pps = Some(nal.bytes),
            1 | 5 => {
                if nal.meta.nal_type == 5 {
                    idr_seen += 1;
                } else {
                    p_seen += 1;
                }
                let (Some(sps), Some(pps)) = (&last_sps, &last_pps) else {
                    skipped_missing_headers += 1;
                    continue;
                };
                let indexed_frame = indexed_frames.len();
                if nal.meta.nal_type == 5 {
                    last_idr_frame = Some(indexed_frame);
                    if !forward_full_cache_enabled {
                        forward_tail_start_frame = indexed_frame;
                        forward_tail_cache.clear();
                    }
                }
                let mut frame = Vec::with_capacity(sps.len() + pps.len() + nal.bytes.len());
                frame.extend_from_slice(sps.as_slice());
                frame.extend_from_slice(pps.as_slice());
                frame.extend_from_slice(nal.bytes.as_slice());
                let detail = super::h264_cmd::parse_annexb_single_i_or_p_debug(frame.as_slice())
                    .map_err(|err| {
                        crate::log!(
                            "intel/hw_vid: h264-frame-index detail-parse-failed source_frame={} stream_idr={} nal={} offset=0x{:X} bytes=0x{:X} err={:?}\n",
                            indexed_frame + 1,
                            idr_seen,
                            nal.meta.nal_type,
                            nal.meta.stream_offset,
                            nal.meta.bytes,
                            err
                        );
                        err
                    })
                    .ok();
                indexed_frames.push(H264IndexedFrame {
                    stream_offset: nal.meta.stream_offset,
                    bytes: nal.meta.bytes,
                    nal_type: nal.meta.nal_type,
                    stream_idr_index: idr_seen,
                    decode_start_frame: last_idr_frame.unwrap_or(indexed_frame),
                    detail,
                    sps: sps.clone(),
                    pps: pps.clone(),
                });
                h264_log_frame_index(&indexed_frames[indexed_frame], indexed_frame);

                submitted += 1;
                let decoded = h264_submit_wait_probe_frame(
                    "forward",
                    submitted,
                    idr_seen,
                    &frame,
                    true,
                    mode.reverse_after_forward(),
                )
                .await;
                if let Some(decoded) = decoded {
                    if mode.reverse_after_forward() {
                        if forward_full_cache_enabled {
                            forward_full_cache.push(decoded);
                        } else {
                            forward_tail_cache.push(decoded);
                        }
                    }
                }
                Timer::after(EmbassyDuration::from_millis(H264_BOOT_PROBE_PLAYBACK_FRAME_MS)).await;
            }
            _ => {}
        }
    }

    let forward_full_cache_frames = forward_full_cache.len();
    let forward_full_cache_bytes = h264_decoded_frames_total_bytes(forward_full_cache.as_slice());
    let forward_tail_cache_frames = forward_tail_cache.len();
    let forward_tail_cache_bytes = h264_decoded_frames_total_bytes(forward_tail_cache.as_slice());

    crate::log!(
        "intel/hw_vid: h264-playback-probe done nals={} idr_seen={} p_seen={} submitted={} indexed_frames={} missing_headers={} stopped_at=0x{:X} forward_full_cache={} forward_full_cache_bytes=0x{:X} forward_tail_cache={} forward_tail_cache_bytes=0x{:X} reason={}\n",
        nal_count,
        idr_seen,
        p_seen,
        submitted,
        indexed_frames.len(),
        skipped_missing_headers,
        stopped_at,
        forward_full_cache_frames,
        forward_full_cache_bytes,
        forward_tail_cache_frames,
        forward_tail_cache_bytes,
        "eos"
    );

    if mode.reverse_after_forward() {
        let forward_cache = if forward_full_cache_enabled && !forward_full_cache.is_empty() {
            Some(H264ForwardCache::Full(forward_full_cache))
        } else if !forward_tail_cache.is_empty() {
            Some(H264ForwardCache::Tail(H264ForwardTailCache {
                start_frame: forward_tail_start_frame,
                frames: forward_tail_cache,
            }))
        } else {
            None
        };
        h264_reverse_playback_probe(file, indexed_frames.as_slice(), forward_cache).await;
    }
}

async fn h264_submit_wait_probe_frame(
    phase: &'static str,
    playback_frame: usize,
    stream_idr_index: usize,
    encoded: &[u8],
    present_output: bool,
    capture_output: bool,
) -> Option<H264DecodedFrame> {
    let before = crate::intel::hw_pic_snapshot();
    crate::log!(
        "intel/hw_vid: h264-probe submit phase={} playback_frame={} stream_idr={} bytes={} present={} capture={} pending={} outputs={} service_started={}\n",
        phase,
        playback_frame,
        stream_idr_index,
        encoded.len(),
        present_output as u8,
        capture_output as u8,
        before.pending,
        before.outputs,
        before.service_started as u8
    );

    let id = match crate::intel::hw_pic_submit_h264(encoded) {
        Ok(id) => id,
        Err(err) => {
            crate::log!(
                "intel/hw_vid: h264-probe submit-failed phase={} playback_frame={} stream_idr={} err={}\n",
                phase,
                playback_frame,
                stream_idr_index,
                err
            );
            return None;
        }
    };

    let Some(output) =
        crate::intel::hw_pic_wait_output_for_id(id, H264_BOOT_PROBE_TIMEOUT_MS).await
    else {
        let after = crate::intel::hw_pic_snapshot();
        crate::log!(
            "intel/hw_vid: h264-probe timeout phase={} playback_frame={} stream_idr={} id={} pending={} outputs={} service_started={}\n",
            phase,
            playback_frame,
            stream_idr_index,
            id,
            after.pending,
            after.outputs,
            after.service_started as u8
        );
        return None;
    };

    let stored = if present_output {
        h264_present_probe_output(&output)
    } else {
        false
    };

    crate::log!(
        "intel/hw_vid: h264-probe output phase={} playback_frame={} stream_idr={} id={} codec={:?} status={:?} fmt={:?} decoded={}x{} visible={}x{} pitch=0x{:X} uv=0x{:X} bytes=0x{:X} gpu=0x{:X} phys=0x{:X} stored={} present={} err={}\n",
        phase,
        playback_frame,
        stream_idr_index,
        output.id,
        output.codec,
        output.status,
        output.format,
        output.width,
        output.height,
        output.visible_width,
        output.visible_height,
        output.pitch_bytes,
        output.uv_offset,
        output.byte_len,
        output.gpu_addr,
        output.phys_addr,
        stored as u8,
        if present_output {
            "ytile-nv12-diagnostic"
        } else {
            "decode-only"
        },
        output.error_code
    );
    if capture_output {
        h264_capture_probe_output(&output)
    } else if stored {
        Some(H264DecodedFrame::empty_marker(&output))
    } else {
        None
    }
}

async fn h264_reverse_playback_probe(
    file: crate::r::fs::trueosfs::FileReadHandle,
    frames: &[H264IndexedFrame],
    forward_cache: Option<H264ForwardCache>,
) {
    if frames.is_empty() {
        crate::log!("intel/hw_vid: h264-reverse-probe skipped reason=no-indexed-frames\n");
        return;
    }

    let mut presented = 0usize;
    let mut submitted = 0usize;
    let mut read_failures = 0usize;
    let mut decode_failures = 0usize;
    let mut gops = 0usize;
    let mut cached_peak = 0usize;
    let mut reused_forward_tail = false;
    let mut forward_full_cache = None;
    let mut forward_tail = None;
    match forward_cache {
        Some(H264ForwardCache::Full(cache)) => forward_full_cache = Some(cache),
        Some(H264ForwardCache::Tail(tail)) => forward_tail = Some(tail),
        None => {}
    };
    let forward_full_cache_frames = forward_full_cache
        .as_ref()
        .map(|cache| cache.len())
        .unwrap_or(0);
    let forward_full_cache_bytes = forward_full_cache
        .as_ref()
        .map(|cache| h264_decoded_frames_total_bytes(cache.as_slice()))
        .unwrap_or(0);

    crate::log!(
        "intel/hw_vid: h264-reverse-probe start frames={} first_nal={} last_nal={} strategy=cache-gop-forward-present-gop-backward forward_full_cache={} forward_full_cache_bytes=0x{:X} forward_tail_cache={} visible_cache_fill={} source=trueosfs-root path={}\n",
        frames.len(),
        frames[0].nal_type,
        frames[frames.len() - 1].nal_type,
        forward_full_cache_frames,
        forward_full_cache_bytes,
        forward_tail
            .as_ref()
            .map(|tail| tail.frames.len())
            .unwrap_or(0),
        H264_BOOT_PROBE_REVERSE_SHOW_CACHE_FILL as u8,
        H264_BOOT_PROBE_STREAM_PATH
    );

    if let Some(cache) = forward_full_cache {
        if cache.len() == frames.len() {
            let cache_bytes = h264_decoded_frames_total_bytes(cache.as_slice());
            crate::log!(
                "intel/hw_vid: h264-reverse-probe full-cache begin frames={} bytes=0x{:X} action=present-backward-only\n",
                cache.len(),
                cache_bytes
            );

            for (source_index, decoded) in cache.iter().enumerate().rev() {
                let frame = &frames[source_index];
                presented += 1;
                let stored = h264_present_decoded_frame(decoded);
                crate::log!(
                    "intel/hw_vid: h264-reverse-probe present playback_frame={} source_frame={} gop=0 gop_frame={} stream_idr={} nal={} class={} frame_num={} poc={}/{} poc_type={} refs_l0={} offset=0x{:X} stored={} bytes=0x{:X} decoded={}x{} visible={}x{} pitch=0x{:X} uv=0x{:X}\n",
                    presented,
                    source_index + 1,
                    h264_gop_frame_number(frame, source_index),
                    frame.stream_idr_index,
                    frame.nal_type,
                    h264_frame_class_label(frame),
                    h264_frame_num_i32(frame),
                    h264_frame_poc_top(frame),
                    h264_frame_poc_bottom(frame),
                    h264_frame_poc_type(frame),
                    h264_frame_refs_l0(frame),
                    frame.stream_offset,
                    stored as u8,
                    decoded.bytes.len(),
                    decoded.width,
                    decoded.height,
                    decoded.visible_width,
                    decoded.visible_height,
                    decoded.pitch_bytes,
                    decoded.uv_offset
                );
                Timer::after(EmbassyDuration::from_millis(H264_BOOT_PROBE_PLAYBACK_FRAME_MS)).await;
            }

            h264_stripe_study_from_full_cache(frames, cache.as_slice()).await;

            crate::log!(
                "intel/hw_vid: h264-reverse-probe done frames={} gops=0 presented={} submitted=0 cached_peak={} read_failures=0 decode_failures=0 visible_cache_fill={} strategy=forward-full-cache reason=eos\n",
                frames.len(),
                presented,
                cache.len(),
                H264_BOOT_PROBE_REVERSE_SHOW_CACHE_FILL as u8
            );
            return;
        }

        crate::log!(
            "intel/hw_vid: h264-reverse-probe full-cache rejected frames={} indexed_frames={} action=fall-back-gop-cache\n",
            cache.len(),
            frames.len()
        );
    }

    let mut gop_end = frames.len();
    while gop_end > 0 {
        let gop_start = frames[gop_end - 1].decode_start_frame.min(gop_end - 1);
        let mut cached = Vec::new();
        gops += 1;

        let gop_span_end = frames[gop_end - 1]
            .stream_offset
            .saturating_add(frames[gop_end - 1].bytes as u64);
        let gop_span_bytes = gop_span_end.saturating_sub(frames[gop_start].stream_offset);
        crate::log!(
            "intel/hw_vid: h264-reverse-probe gop-cache begin gop={} start_frame={} end_frame={} frames={} idr={} span_offset=0x{:X} span_bytes=0x{:X}\n",
            gops,
            gop_start + 1,
            gop_end,
            gop_end.saturating_sub(gop_start),
            frames[gop_start].stream_idr_index,
            frames[gop_start].stream_offset,
            gop_span_bytes
        );

        if let Some(tail) = forward_tail.take() {
            let expected = gop_end.saturating_sub(gop_start);
            if tail.start_frame == gop_start && tail.frames.len() == expected {
                cached = tail.frames;
                reused_forward_tail = true;
                crate::log!(
                    "intel/hw_vid: h264-reverse-probe gop-cache reuse gop={} source=forward-tail start_frame={} frames={}\n",
                    gops,
                    gop_start + 1,
                    cached.len()
                );
            } else {
                crate::log!(
                    "intel/hw_vid: h264-reverse-probe gop-cache reuse-miss gop={} tail_start={} tail_frames={} want_start={} want_frames={}\n",
                    gops,
                    tail.start_frame + 1,
                    tail.frames.len(),
                    gop_start + 1,
                    expected
                );
            }
        }

        if cached.is_empty() {
            let Some(gop_bytes) =
                h264_read_indexed_frame_span(file, &frames[gop_start..gop_end]).await
            else {
                read_failures += 1;
                gop_end = gop_start;
                continue;
            };

            for decode_frame in gop_start..gop_end {
                let packet =
                    h264_build_indexed_frame_packet_from_span(&frames[decode_frame], &gop_bytes);
                submitted += 1;
                let Some(decoded) = h264_submit_wait_probe_frame(
                    if H264_BOOT_PROBE_REVERSE_SHOW_CACHE_FILL {
                        "reverse-cache-visible"
                    } else {
                        "reverse-cache"
                    },
                    submitted,
                    frames[decode_frame].stream_idr_index,
                    packet.as_slice(),
                    H264_BOOT_PROBE_REVERSE_SHOW_CACHE_FILL,
                    true,
                )
                .await
                else {
                    decode_failures += 1;
                    break;
                };
                cached.push(decoded);
            }
        }

        cached_peak = cached_peak.max(cached.len());
        crate::log!(
            "intel/hw_vid: h264-reverse-probe gop-cache end gop={} cached={} submitted={} read_failures={} decode_failures={} source={}\n",
            gops,
            cached.len(),
            submitted,
            read_failures,
            decode_failures,
            if reused_forward_tail {
                "forward-tail"
            } else {
                "decode"
            }
        );
        reused_forward_tail = false;

        for (cache_index, decoded) in cached.iter().enumerate().rev() {
            let source_index = gop_start + cache_index;
            let frame = &frames[source_index];
            presented += 1;
            let stored = h264_present_decoded_frame(decoded);
            crate::log!(
                "intel/hw_vid: h264-reverse-probe present playback_frame={} source_frame={} gop={} gop_frame={} stream_idr={} nal={} class={} frame_num={} poc={}/{} poc_type={} refs_l0={} offset=0x{:X} stored={} bytes=0x{:X} decoded={}x{} visible={}x{} pitch=0x{:X} uv=0x{:X}\n",
                presented,
                source_index + 1,
                gops,
                h264_gop_frame_number(frame, source_index),
                frame.stream_idr_index,
                frame.nal_type,
                h264_frame_class_label(frame),
                h264_frame_num_i32(frame),
                h264_frame_poc_top(frame),
                h264_frame_poc_bottom(frame),
                h264_frame_poc_type(frame),
                h264_frame_refs_l0(frame),
                frame.stream_offset,
                stored as u8,
                decoded.bytes.len(),
                decoded.width,
                decoded.height,
                decoded.visible_width,
                decoded.visible_height,
                decoded.pitch_bytes,
                decoded.uv_offset
            );
            Timer::after(EmbassyDuration::from_millis(H264_BOOT_PROBE_PLAYBACK_FRAME_MS)).await;
        }

        gop_end = gop_start;
    }

    crate::log!(
        "intel/hw_vid: h264-reverse-probe done frames={} gops={} presented={} submitted={} cached_peak={} read_failures={} decode_failures={} visible_cache_fill={} reason=eos\n",
        frames.len(),
        gops,
        presented,
        submitted,
        cached_peak,
        read_failures,
        decode_failures,
        H264_BOOT_PROBE_REVERSE_SHOW_CACHE_FILL as u8
    );
}

async fn h264_stripe_study_from_full_cache(
    frames: &[H264IndexedFrame],
    cache: &[H264DecodedFrame],
) {
    if !H264_BOOT_PROBE_STRIPE_STUDY_ENABLED {
        return;
    }
    if frames.len() != cache.len() || cache.is_empty() {
        crate::log!(
            "intel/hw_vid: h264-stripe-study skipped reason=cache-shape frames={} cache={}\n",
            frames.len(),
            cache.len()
        );
        return;
    }

    let mut candidates = Vec::new();
    let mut scanned = 0usize;
    let mut captured = 0usize;
    let mut max_metric = 0u64;
    crate::log!(
        "intel/hw_vid: h264-stripe-study begin frames={} direction=reverse frame_ms={} store_top={} artifact=primary-bgra8-raw metric=vertical-dark-discontinuity\n",
        cache.len(),
        H264_BOOT_PROBE_STRIPE_STUDY_FRAME_MS,
        H264_BOOT_PROBE_STRIPE_STUDY_STORE_TOP
    );

    for (reverse_offset, source_index) in (0..cache.len()).rev().enumerate() {
        let decoded = &cache[source_index];
        let frame = &frames[source_index];
        let reverse_step = reverse_offset + 1;
        let stored = h264_present_decoded_frame(decoded);
        let snapshot = crate::intel::capture_primary_surface_bgra8();
        let metric = snapshot
            .as_ref()
            .map(|snapshot| h264_vertical_black_stripe_metric(snapshot, decoded))
            .unwrap_or(0);
        scanned += 1;
        captured += usize::from(snapshot.is_some());
        max_metric = max_metric.max(metric);
        crate::log!(
            "intel/hw_vid: h264-stripe-study frame reverse_step={} source_frame={} gop_frame={} stream_idr={} class={} frame_num={} poc={}/{} metric={} stored={} captured={} decoded={}x{} visible={}x{}\n",
            reverse_step,
            source_index + 1,
            h264_gop_frame_number(frame, source_index),
            frame.stream_idr_index,
            h264_frame_class_label(frame),
            h264_frame_num_i32(frame),
            h264_frame_poc_top(frame),
            h264_frame_poc_bottom(frame),
            metric,
            stored as u8,
            snapshot.is_some() as u8,
            decoded.width,
            decoded.height,
            decoded.visible_width,
            decoded.visible_height
        );
        if let Some(snapshot) = snapshot {
            h264_stripe_study_insert_candidate(
                &mut candidates,
                H264StripeStudyCandidate {
                    metric,
                    source_index,
                    reverse_step,
                    snapshot,
                },
            );
        }
        Timer::after(EmbassyDuration::from_millis(H264_BOOT_PROBE_STRIPE_STUDY_FRAME_MS)).await;
    }

    h264_write_stripe_study_artifacts(frames, candidates.as_slice()).await;
    crate::log!(
        "intel/hw_vid: h264-stripe-study done scanned={} captured={} stored={} max_metric={}\n",
        scanned,
        captured,
        candidates.len(),
        max_metric
    );
}

fn h264_stripe_study_insert_candidate(
    candidates: &mut Vec<H264StripeStudyCandidate>,
    candidate: H264StripeStudyCandidate,
) {
    if H264_BOOT_PROBE_STRIPE_STUDY_STORE_TOP == 0 {
        return;
    }

    let insert_at = candidates
        .iter()
        .position(|existing| candidate.metric > existing.metric)
        .unwrap_or(candidates.len());
    if insert_at >= H264_BOOT_PROBE_STRIPE_STUDY_STORE_TOP {
        return;
    }
    candidates.insert(insert_at, candidate);
    if candidates.len() > H264_BOOT_PROBE_STRIPE_STUDY_STORE_TOP {
        candidates.pop();
    }
}

async fn h264_write_stripe_study_artifacts(
    frames: &[H264IndexedFrame],
    candidates: &[H264StripeStudyCandidate],
) {
    let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
        crate::log!("intel/hw_vid: h264-stripe-study write skipped reason=no-trueosfs-root\n");
        return;
    };

    let mut manifest = String::new();
    let _ = writeln!(
        manifest,
        "kind=h264-stripe-study format=primary-bgra8-raw candidates={}",
        candidates.len()
    );

    for (rank, candidate) in candidates.iter().enumerate() {
        let frame = &frames[candidate.source_index];
        let path = format!(
            "h264_stripe_study_rank{:02}_src{:05}_metric{}.bgra",
            rank + 1,
            candidate.source_index + 1,
            candidate.metric
        );
        let wrote = match crate::r::fs::trueosfs::file_in_async(
            disk,
            path.as_str(),
            candidate.snapshot.pixels.as_slice(),
        )
        .await
        {
            Ok(ok) => ok,
            Err(err) => {
                crate::log!(
                    "intel/hw_vid: h264-stripe-study write failed path={} err={:?}\n",
                    path.as_str(),
                    err
                );
                false
            }
        };
        crate::log!(
            "intel/hw_vid: h264-stripe-study artifact rank={} path={} wrote={} source_frame={} reverse_step={} metric={} width={} height={} bytes=0x{:X} class={} frame_num={} poc={}/{}\n",
            rank + 1,
            path.as_str(),
            wrote as u8,
            candidate.source_index + 1,
            candidate.reverse_step,
            candidate.metric,
            candidate.snapshot.width,
            candidate.snapshot.height,
            candidate.snapshot.pixels.len(),
            h264_frame_class_label(frame),
            h264_frame_num_i32(frame),
            h264_frame_poc_top(frame),
            h264_frame_poc_bottom(frame)
        );
        let _ = writeln!(
            manifest,
            "rank={} path={} wrote={} source_frame={} reverse_step={} metric={} width={} height={} bytes={} class={} frame_num={} poc_top={} poc_bottom={}",
            rank + 1,
            path.as_str(),
            wrote as u8,
            candidate.source_index + 1,
            candidate.reverse_step,
            candidate.metric,
            candidate.snapshot.width,
            candidate.snapshot.height,
            candidate.snapshot.pixels.len(),
            h264_frame_class_label(frame),
            h264_frame_num_i32(frame),
            h264_frame_poc_top(frame),
            h264_frame_poc_bottom(frame)
        );
    }

    let manifest_path = "h264_stripe_study_manifest.txt";
    let wrote_manifest =
        match crate::r::fs::trueosfs::file_in_async(disk, manifest_path, manifest.as_bytes()).await
        {
            Ok(ok) => ok,
            Err(err) => {
                crate::log!(
                    "intel/hw_vid: h264-stripe-study manifest failed path={} err={:?}\n",
                    manifest_path,
                    err
                );
                false
            }
        };
    crate::log!(
        "intel/hw_vid: h264-stripe-study manifest path={} wrote={} bytes={}\n",
        manifest_path,
        wrote_manifest as u8,
        manifest.len()
    );
}

fn h264_vertical_black_stripe_metric(
    snapshot: &crate::intel::display::PrimarySurfaceBgra8Snapshot,
    frame: &H264DecodedFrame,
) -> u64 {
    let screen_w = snapshot.width as usize;
    let screen_h = snapshot.height as usize;
    let video_w = (frame.visible_width as usize).min(screen_w);
    let video_h = (frame.visible_height as usize).min(screen_h);
    if screen_w < 4 || screen_h < 4 || video_w < 16 || video_h < 16 {
        return 0;
    }

    let x0 = screen_w.saturating_sub(video_w) / 2;
    let y0 = screen_h.saturating_sub(video_h) / 2;
    let x_start = x0 + video_w / 10;
    let x_end = x0 + video_w.saturating_mul(9) / 10;
    let y_start = y0 + video_h / 8;
    let y_end = y0 + video_h.saturating_mul(7) / 8;
    if x_end <= x_start + 2 || y_end <= y_start {
        return 0;
    }

    let mut max_deficit = 0u64;
    let mut total_deficit = 0u64;
    let mut x = x_start + 1;
    while x + 1 < x_end {
        let cur = h264_bgra_column_luma_average(snapshot, x, y_start, y_end);
        let left = h264_bgra_column_luma_average(snapshot, x - 1, y_start, y_end);
        let right = h264_bgra_column_luma_average(snapshot, x + 1, y_start, y_end);
        let neighbor = (left + right) / 2;
        if neighbor > cur && cur < 96 {
            let deficit = neighbor - cur;
            max_deficit = max_deficit.max(deficit);
            total_deficit = total_deficit.saturating_add(deficit);
        }
        x += 1;
    }

    max_deficit
        .saturating_mul(1_000_000)
        .saturating_add(total_deficit)
}

fn h264_bgra_column_luma_average(
    snapshot: &crate::intel::display::PrimarySurfaceBgra8Snapshot,
    x: usize,
    y_start: usize,
    y_end: usize,
) -> u64 {
    let width = snapshot.width as usize;
    if width == 0 || x >= width {
        return 0;
    }

    let mut sum = 0u64;
    let mut count = 0u64;
    let mut y = y_start;
    while y < y_end {
        let off = y.saturating_mul(width).saturating_add(x).saturating_mul(4);
        if off + 2 < snapshot.pixels.len() {
            let b = snapshot.pixels[off] as u64;
            let g = snapshot.pixels[off + 1] as u64;
            let r = snapshot.pixels[off + 2] as u64;
            sum = sum.saturating_add((r * 77 + g * 150 + b * 29) >> 8);
            count += 1;
        }
        y += 8;
    }

    if count == 0 { 0 } else { sum / count }
}

async fn h264_read_indexed_frame_span(
    file: crate::r::fs::trueosfs::FileReadHandle,
    frames: &[H264IndexedFrame],
) -> Option<H264IndexedSpan> {
    let first = frames.first()?;
    let last = frames.last()?;
    let span_end = last.stream_offset.saturating_add(last.bytes as u64);
    let span_len_u64 = span_end.checked_sub(first.stream_offset)?;
    let span_len = usize::try_from(span_len_u64).ok()?;
    let bytes = h264_read_stream_range(file, first.stream_offset, span_len).await?;
    Some(H264IndexedSpan {
        stream_offset: first.stream_offset,
        bytes,
    })
}

struct H264IndexedSpan {
    stream_offset: u64,
    bytes: Vec<u8>,
}

fn h264_build_indexed_frame_packet_from_span(
    frame: &H264IndexedFrame,
    span: &H264IndexedSpan,
) -> Vec<u8> {
    let rel = frame.stream_offset.saturating_sub(span.stream_offset);
    let rel = usize::try_from(rel).unwrap_or(usize::MAX);
    let end = rel.saturating_add(frame.bytes);
    let nal = if rel <= span.bytes.len() && end <= span.bytes.len() {
        &span.bytes[rel..end]
    } else {
        &[]
    };
    let mut packet = Vec::with_capacity(frame.sps.len() + frame.pps.len() + nal.len());
    packet.extend_from_slice(frame.sps.as_slice());
    packet.extend_from_slice(frame.pps.as_slice());
    packet.extend_from_slice(nal);
    packet
}

async fn h264_read_stream_range(
    file: crate::r::fs::trueosfs::FileReadHandle,
    offset: u64,
    bytes: usize,
) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    out.resize(bytes, 0);
    let mut done = 0usize;
    while done < bytes {
        let read = match crate::r::fs::trueosfs::file_read_handle_range_async(
            file,
            offset.saturating_add(done as u64),
            &mut out[done..bytes],
        )
        .await
        {
            Ok(Some(read)) => read,
            Ok(None) => 0,
            Err(err) => {
                crate::log!(
                    "intel/hw_vid: h264-reverse-probe stream-read failed path={} offset=0x{:X} want=0x{:X} err={:?}\n",
                    H264_BOOT_PROBE_STREAM_PATH,
                    offset.saturating_add(done as u64),
                    bytes.saturating_sub(done),
                    err
                );
                0
            }
        };
        if read == 0 {
            return None;
        }
        done = done.saturating_add(read);
    }
    Some(out)
}

fn h264_present_probe_output(output: &super::hw_pic::HwPicOutput) -> bool {
    if matches!(
        output.status,
        super::hw_pic::HwPicStatus::Ready | super::hw_pic::HwPicStatus::Streamed
    ) && output.format == super::hw_pic::HwPicPixelFormat::Nv12
        && output.width != 0
        && output.height != 0
        && output.pitch_bytes != 0
        && output.byte_len != 0
        && output.virt_addr != 0
    {
        let src =
            unsafe { core::slice::from_raw_parts(output.virt_addr as *const u8, output.byte_len) };
        crate::intel::display::present_ytile_nv12_surface_center(
            src,
            output.width,
            output.height,
            0,
            0,
            output.visible_width,
            output.visible_height,
            output.pitch_bytes,
            output.uv_offset,
        )
    } else {
        false
    }
}

impl H264DecodedFrame {
    fn empty_marker(output: &super::hw_pic::HwPicOutput) -> Self {
        Self {
            bytes: Vec::new(),
            width: output.width,
            height: output.height,
            visible_width: output.visible_width,
            visible_height: output.visible_height,
            pitch_bytes: output.pitch_bytes,
            uv_offset: output.uv_offset,
        }
    }
}

fn h264_capture_probe_output(output: &super::hw_pic::HwPicOutput) -> Option<H264DecodedFrame> {
    if !matches!(
        output.status,
        super::hw_pic::HwPicStatus::Ready | super::hw_pic::HwPicStatus::Streamed
    ) || output.format != super::hw_pic::HwPicPixelFormat::Nv12
        || output.width == 0
        || output.height == 0
        || output.pitch_bytes == 0
        || output.byte_len == 0
        || output.virt_addr == 0
    {
        return None;
    }

    let src =
        unsafe { core::slice::from_raw_parts(output.virt_addr as *const u8, output.byte_len) };
    let mut bytes = Vec::with_capacity(output.byte_len);
    bytes.extend_from_slice(src);
    Some(H264DecodedFrame {
        bytes,
        width: output.width,
        height: output.height,
        visible_width: output.visible_width,
        visible_height: output.visible_height,
        pitch_bytes: output.pitch_bytes,
        uv_offset: output.uv_offset,
    })
}

fn h264_present_decoded_frame(frame: &H264DecodedFrame) -> bool {
    if frame.bytes.is_empty() {
        return false;
    }
    crate::intel::display::present_ytile_nv12_surface_center(
        frame.bytes.as_slice(),
        frame.width,
        frame.height,
        0,
        0,
        frame.visible_width,
        frame.visible_height,
        frame.pitch_bytes,
        frame.uv_offset,
    )
}

fn h264_decoded_frames_total_bytes(frames: &[H264DecodedFrame]) -> usize {
    frames
        .iter()
        .fold(0usize, |total, frame| total.saturating_add(frame.bytes.len()))
}

fn h264_log_frame_index(frame: &H264IndexedFrame, index: usize) {
    crate::log!(
        "intel/hw_vid: h264-frame-index source_frame={} gop_frame={} stream_idr={} nal={} detail_nal={} class={} frame_num={} poc={}/{} poc_type={} log2_frame_minus4={} log2_poc_lsb_minus4={} refs_l0={} coded={}x{} visible={}x{} offset=0x{:X} bytes=0x{:X} decode_start_frame={}\n",
        index + 1,
        h264_gop_frame_number(frame, index),
        frame.stream_idr_index,
        frame.nal_type,
        h264_frame_detail_nal_i32(frame),
        h264_frame_class_label(frame),
        h264_frame_num_i32(frame),
        h264_frame_poc_top(frame),
        h264_frame_poc_bottom(frame),
        h264_frame_poc_type(frame),
        h264_frame_log2_frame_minus4(frame),
        h264_frame_log2_poc_lsb_minus4(frame),
        h264_frame_refs_l0(frame),
        h264_frame_coded_width(frame),
        h264_frame_coded_height(frame),
        h264_frame_visible_width(frame),
        h264_frame_visible_height(frame),
        frame.stream_offset,
        frame.bytes,
        frame.decode_start_frame + 1
    );
}

fn h264_gop_frame_number(frame: &H264IndexedFrame, index: usize) -> usize {
    index
        .saturating_sub(frame.decode_start_frame)
        .saturating_add(1)
}

fn h264_frame_class_label(frame: &H264IndexedFrame) -> &'static str {
    frame
        .detail
        .map(|detail| detail.class.label())
        .unwrap_or("unknown")
}

fn h264_frame_detail_nal_i32(frame: &H264IndexedFrame) -> i32 {
    frame
        .detail
        .map(|detail| i32::from(detail.nal_type))
        .unwrap_or(-1)
}

fn h264_frame_num_i32(frame: &H264IndexedFrame) -> i32 {
    frame
        .detail
        .map(|detail| i32::from(detail.frame_num))
        .unwrap_or(-1)
}

fn h264_frame_poc_top(frame: &H264IndexedFrame) -> i32 {
    frame
        .detail
        .map(|detail| detail.top_field_order_cnt)
        .unwrap_or(i32::MIN)
}

fn h264_frame_poc_bottom(frame: &H264IndexedFrame) -> i32 {
    frame
        .detail
        .map(|detail| detail.bottom_field_order_cnt)
        .unwrap_or(i32::MIN)
}

fn h264_frame_poc_type(frame: &H264IndexedFrame) -> i32 {
    frame
        .detail
        .map(|detail| i32::from(detail.pic_order_cnt_type))
        .unwrap_or(-1)
}

fn h264_frame_log2_frame_minus4(frame: &H264IndexedFrame) -> i32 {
    frame
        .detail
        .map(|detail| i32::from(detail.log2_max_frame_num_minus4))
        .unwrap_or(-1)
}

fn h264_frame_log2_poc_lsb_minus4(frame: &H264IndexedFrame) -> i32 {
    frame
        .detail
        .map(|detail| i32::from(detail.log2_max_pic_order_cnt_lsb_minus4))
        .unwrap_or(-1)
}

fn h264_frame_refs_l0(frame: &H264IndexedFrame) -> i32 {
    frame
        .detail
        .map(|detail| i32::from(detail.num_ref_idx_l0_active_minus1) + 1)
        .unwrap_or(-1)
}

fn h264_frame_coded_width(frame: &H264IndexedFrame) -> i32 {
    frame
        .detail
        .map(|detail| detail.coded_width as i32)
        .unwrap_or(-1)
}

fn h264_frame_coded_height(frame: &H264IndexedFrame) -> i32 {
    frame
        .detail
        .map(|detail| detail.coded_height as i32)
        .unwrap_or(-1)
}

fn h264_frame_visible_width(frame: &H264IndexedFrame) -> i32 {
    frame
        .detail
        .map(|detail| detail.visible_width as i32)
        .unwrap_or(-1)
}

fn h264_frame_visible_height(frame: &H264IndexedFrame) -> i32 {
    frame
        .detail
        .map(|detail| detail.visible_height as i32)
        .unwrap_or(-1)
}

fn h264_find_start_code(bytes: &[u8], offset: usize) -> Option<(usize, usize)> {
    let mut i = offset.min(bytes.len());
    while i + 3 <= bytes.len() {
        if bytes[i..].starts_with(&[0, 0, 1]) {
            return Some((i, 3));
        }
        if i + 4 <= bytes.len() && bytes[i..].starts_with(&[0, 0, 0, 1]) {
            return Some((i, 4));
        }
        i += 1;
    }
    None
}
