use alloc::{format, string::String, vec::Vec};
use core::{
    fmt::Write,
    sync::atomic::{AtomicBool, Ordering},
};
use embassy_time::{Duration as EmbassyDuration, Instant as EmbassyInstant, Timer};

const H264_DECODE_TIMEOUT_MS: u64 = 5_000;
const H264_ONLINE_MEDIA_FETCH_TIMEOUT_MS: u64 = 120_000;
const H264_ONLINE_MEDIA_FETCH_MAX_BYTES: usize = 160 * 1024 * 1024;
pub(crate) const UI4_FRAMED_VIDEO_ASSET: &str = "x31_head_movie.annexb.h264";
const UI4_FRAMED_VIDEO_ANNEXB: &[u8] =
    include_bytes!("../../../tools/vid/x31_head_movie.annexb.h264");
const UI4_FRAMED_VIDEO_FPS: u16 = 60;
const H264_ONLINE_MEDIA_URL: &str = "https://docs.evostream.com/sample_content/assets/bun33s.mp4";

static H264_PLAYBACK_ACTIVE: AtomicBool = AtomicBool::new(false);
static H264_UI4_HANDOFF_CHECKPOINT_LOGGED: AtomicBool = AtomicBool::new(false);

struct H264PlaybackGuard;

impl Drop for H264PlaybackGuard {
    fn drop(&mut self) {
        H264_PLAYBACK_ACTIVE.store(false, Ordering::Release);
    }
}

fn h264_try_begin_playback(scope: &str) -> Result<H264PlaybackGuard, &'static str> {
    if H264_PLAYBACK_ACTIVE
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        crate::log!("intel/hw_vid: playback rejected scope={} reason=already-active\n", scope);
        return Err("video playback already active");
    }
    Ok(H264PlaybackGuard)
}

#[derive(Copy, Clone, Debug)]
struct H264PlaybackOptions {
    fps: u16,
    diagnostics: bool,
    noreset_lite: bool,
}

impl H264PlaybackOptions {
    const fn new(fps: u16, diagnostics: bool, noreset_lite: bool) -> Self {
        Self {
            fps,
            diagnostics,
            noreset_lite,
        }
    }

    const fn fps(self) -> u16 {
        self.fps
    }

    const fn frame_ms(self) -> u64 {
        let fps = self.fps as u64;
        let ms = (1000 + fps / 2) / fps;
        if ms == 0 { 1 } else { ms }
    }

    const fn frame_period(self) -> EmbassyDuration {
        EmbassyDuration::from_hz(self.fps as u64)
    }

    const fn diagnostics(self) -> bool {
        self.diagnostics
    }

    const fn noreset_lite(self) -> bool {
        self.noreset_lite
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct H264PlaybackReport {
    pub(crate) target_fps: u16,
    pub(crate) target_frame_ms: u64,
    pub(crate) submitted: usize,
    pub(crate) skipped_unsupported: usize,
    pub(crate) elapsed_ms: u64,
    pub(crate) effective_fps_x100: u64,
    pub(crate) waited_frames: usize,
    pub(crate) late_frames: usize,
    pub(crate) total_wait_ms: u64,
    pub(crate) avg_decode_us: u64,
    pub(crate) max_decode_us: u64,
    pub(crate) max_late_ms: u64,
    pub(crate) avg_queue_us: u64,
    pub(crate) avg_process_us: u64,
    pub(crate) avg_reset_us: u64,
    pub(crate) avg_zero_clear_us: u64,
    pub(crate) avg_zero_us: u64,
    pub(crate) avg_scratch_zero_us: u64,
    pub(crate) avg_output_clear_us: u64,
    pub(crate) avg_missing_clear_us: u64,
    pub(crate) avg_scratch_flush_us: u64,
    pub(crate) avg_build_ctx_us: u64,
    pub(crate) avg_poll_us: u64,
    pub(crate) max_poll_us: u64,
    pub(crate) avg_post_us: u64,
    pub(crate) avg_present_us: u64,
    pub(crate) max_present_us: u64,
    pub(crate) avg_poll_iters: u64,
}

#[derive(Copy, Clone, Debug, Default)]
struct H264PlaybackTiming {
    waited_frames: usize,
    late_frames: usize,
    total_wait_ticks: u64,
    total_decode_ticks: u64,
    max_decode_ticks: u64,
    max_late_ticks: u64,
    total_queue_us: u64,
    total_process_us: u64,
    total_reset_us: u64,
    total_zero_clear_us: u64,
    total_zero_us: u64,
    total_scratch_zero_us: u64,
    total_output_clear_us: u64,
    total_missing_clear_us: u64,
    total_scratch_flush_us: u64,
    total_build_ctx_us: u64,
    total_poll_us: u64,
    max_poll_us: u64,
    total_post_us: u64,
    total_present_ticks: u64,
    max_present_ticks: u64,
    total_poll_iters: u64,
}

impl H264PlaybackTiming {
    fn record_decode_ticks(&mut self, ticks: u64) {
        self.total_decode_ticks = self.total_decode_ticks.saturating_add(ticks);
        self.max_decode_ticks = self.max_decode_ticks.max(ticks);
    }

    fn record_hw_pic_timing(&mut self, timing: crate::intel::hw_pic::HwPicTiming) {
        self.total_queue_us = self.total_queue_us.saturating_add(timing.queue_wait_us);
        self.total_process_us = self.total_process_us.saturating_add(timing.process_us);
        self.total_reset_us = self.total_reset_us.saturating_add(timing.backend_reset_us);
        self.total_zero_clear_us = self
            .total_zero_clear_us
            .saturating_add(timing.backend_zero_clear_us);
        self.total_zero_us = self.total_zero_us.saturating_add(timing.backend_zero_us);
        self.total_scratch_zero_us = self
            .total_scratch_zero_us
            .saturating_add(timing.backend_scratch_zero_us);
        self.total_output_clear_us = self
            .total_output_clear_us
            .saturating_add(timing.backend_output_clear_us);
        self.total_missing_clear_us = self
            .total_missing_clear_us
            .saturating_add(timing.backend_missing_clear_us);
        self.total_scratch_flush_us = self
            .total_scratch_flush_us
            .saturating_add(timing.backend_scratch_flush_us);
        self.total_build_ctx_us = self
            .total_build_ctx_us
            .saturating_add(timing.backend_build_ctx_us);
        self.total_poll_us = self.total_poll_us.saturating_add(timing.backend_poll_us);
        self.max_poll_us = self.max_poll_us.max(timing.backend_poll_us);
        self.total_post_us = self.total_post_us.saturating_add(timing.backend_post_us);
        self.total_poll_iters = self
            .total_poll_iters
            .saturating_add(timing.backend_poll_iters as u64);
    }

    fn record_present_ticks(&mut self, ticks: u64) {
        self.total_present_ticks = self.total_present_ticks.saturating_add(ticks);
        self.max_present_ticks = self.max_present_ticks.max(ticks);
    }

    fn avg_us(total_us: u64, submitted: usize) -> u64 {
        if submitted == 0 {
            0
        } else {
            total_us / submitted as u64
        }
    }

    fn report(
        self,
        mode: H264PlaybackOptions,
        submitted: usize,
        skipped_unsupported: usize,
        playback_start: EmbassyInstant,
    ) -> H264PlaybackReport {
        let elapsed_ms = playback_start.elapsed().as_millis();
        let effective_fps_x100 = if elapsed_ms == 0 {
            0
        } else {
            (submitted as u64).saturating_mul(100_000) / elapsed_ms
        };
        let avg_decode_us = if submitted == 0 {
            0
        } else {
            h264_ticks_to_micros(self.total_decode_ticks) / submitted as u64
        };
        H264PlaybackReport {
            target_fps: mode.fps(),
            target_frame_ms: mode.frame_ms(),
            submitted,
            skipped_unsupported,
            elapsed_ms,
            effective_fps_x100,
            waited_frames: self.waited_frames,
            late_frames: self.late_frames,
            total_wait_ms: h264_ticks_to_millis(self.total_wait_ticks),
            avg_decode_us,
            max_decode_us: h264_ticks_to_micros(self.max_decode_ticks),
            max_late_ms: h264_ticks_to_millis(self.max_late_ticks),
            avg_queue_us: Self::avg_us(self.total_queue_us, submitted),
            avg_process_us: Self::avg_us(self.total_process_us, submitted),
            avg_reset_us: Self::avg_us(self.total_reset_us, submitted),
            avg_zero_clear_us: Self::avg_us(self.total_zero_clear_us, submitted),
            avg_zero_us: Self::avg_us(self.total_zero_us, submitted),
            avg_scratch_zero_us: Self::avg_us(self.total_scratch_zero_us, submitted),
            avg_output_clear_us: Self::avg_us(self.total_output_clear_us, submitted),
            avg_missing_clear_us: Self::avg_us(self.total_missing_clear_us, submitted),
            avg_scratch_flush_us: Self::avg_us(self.total_scratch_flush_us, submitted),
            avg_build_ctx_us: Self::avg_us(self.total_build_ctx_us, submitted),
            avg_poll_us: Self::avg_us(self.total_poll_us, submitted),
            max_poll_us: self.max_poll_us,
            avg_post_us: Self::avg_us(self.total_post_us, submitted),
            avg_present_us: if submitted == 0 {
                0
            } else {
                h264_ticks_to_micros(self.total_present_ticks) / submitted as u64
            },
            max_present_us: h264_ticks_to_micros(self.max_present_ticks),
            avg_poll_iters: if submitted == 0 {
                0
            } else {
                self.total_poll_iters / submitted as u64
            },
        }
    }
}

/// Decode the fixed, hardware-validated Annex-B asset and publish every picture
/// through the native UI4 double-Frame path. Shell2 owns the surrounding UI4
/// lifetime; this function owns one VDBOX playback lap and its engine lease.
pub(crate) async fn run_ui4_framed_video_playback() -> Result<H264PlaybackReport, &'static str> {
    if !crate::intel::has_media_decode_engine() {
        return Err("media decode engine unavailable");
    }
    let _playback_guard = h264_try_begin_playback("shell-ui4-framed-video")?;
    let options = H264PlaybackOptions::new(UI4_FRAMED_VIDEO_FPS, false, true);
    crate::log!(
        "intel/hw_vid: ui4-framed-video stage=decode-loop-begin asset={} bytes={} source=kernel-embedded-annexb fps={} presentation=ui4-double-frame\n",
        UI4_FRAMED_VIDEO_ASSET,
        UI4_FRAMED_VIDEO_ANNEXB.len(),
        UI4_FRAMED_VIDEO_FPS,
    );
    let old_hw_pic_logging =
        crate::intel::hw_pic::set_detailed_logging_enabled(options.diagnostics());
    let old_surface_probes =
        crate::intel::xelp_media2_ngin::set_output_surface_probes_enabled(options.diagnostics());
    let old_noreset_lite =
        crate::intel::xelp_media2_ngin_hw_pic::set_avc_noreset_lite_enabled(options.noreset_lite());
    let mut annexb = Vec::with_capacity(UI4_FRAMED_VIDEO_ANNEXB.len());
    annexb.extend_from_slice(UI4_FRAMED_VIDEO_ANNEXB);
    let report = h264_i_p_playback_probe_annexb_bytes(
        annexb,
        "kernel-embedded-annexb",
        UI4_FRAMED_VIDEO_ASSET,
        options,
    )
    .await;
    crate::intel::xelp_media2_ngin_hw_pic::set_avc_noreset_lite_enabled(old_noreset_lite);
    crate::intel::hw_pic::set_detailed_logging_enabled(old_hw_pic_logging);
    crate::intel::xelp_media2_ngin::set_output_surface_probes_enabled(old_surface_probes);
    if report.submitted == 0 {
        Err("embedded video produced no decodable frames")
    } else {
        Ok(report)
    }
}

pub(crate) async fn run_online_ui4_framed_video_playback()
-> Result<H264PlaybackReport, &'static str> {
    let _playback_guard = h264_try_begin_playback("shell-online-ui4-framed-video")?;
    let options = H264PlaybackOptions::new(UI4_FRAMED_VIDEO_FPS, false, true);
    crate::log!(
        "intel/hw_vid: online-ui4-framed-video stage=download-begin url={} fps={} presentation=ui4-double-frame\n",
        H264_ONLINE_MEDIA_URL,
        UI4_FRAMED_VIDEO_FPS,
    );
    let report = run_media_url_playback(
        H264_ONLINE_MEDIA_URL,
        options,
        "online-ui4-framed-video",
        "online-ui4-framed-video",
    )
    .await?;
    if report.submitted == 0 {
        Err("online video produced no decodable frames")
    } else {
        Ok(report)
    }
}

async fn run_media_url_playback(
    url: &str,
    options: H264PlaybackOptions,
    log_scope: &'static str,
    playback_path: &'static str,
) -> Result<H264PlaybackReport, &'static str> {
    if !crate::intel::has_media_decode_engine() {
        return Err("media decode engine unavailable");
    }
    let mp4_bytes = h264_fetch_media_url_bytes(url, log_scope).await?;
    let annexb = mp4_avc1_to_annexb(mp4_bytes.as_slice())?;
    crate::log!(
        "intel/hw_vid: {} demux accepted=1 container=mp4 codec=avc1 mp4_bytes={} annexb_bytes={} url={}\n",
        log_scope,
        mp4_bytes.len(),
        annexb.len(),
        url
    );

    let old_hw_pic_logging =
        crate::intel::hw_pic::set_detailed_logging_enabled(options.diagnostics());
    let old_surface_probes =
        crate::intel::xelp_media2_ngin::set_output_surface_probes_enabled(options.diagnostics());
    let old_noreset_lite =
        crate::intel::xelp_media2_ngin_hw_pic::set_avc_noreset_lite_enabled(options.noreset_lite());
    let report =
        h264_i_p_playback_probe_annexb_bytes(annexb, "media-url-mp4-avc1", playback_path, options)
            .await;
    crate::intel::xelp_media2_ngin_hw_pic::set_avc_noreset_lite_enabled(old_noreset_lite);
    crate::intel::hw_pic::set_detailed_logging_enabled(old_hw_pic_logging);
    crate::intel::xelp_media2_ngin::set_output_surface_probes_enabled(old_surface_probes);
    Ok(report)
}

fn bytes_contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

fn hex_prefix(bytes: &[u8], max_len: usize) -> String {
    let mut out = String::new();
    for (index, byte) in bytes.iter().take(max_len).copied().enumerate() {
        if index != 0 {
            out.push('_');
        }
        let _ = write!(out, "{:02X}", byte);
    }
    if out.is_empty() {
        out.push('-');
    }
    out
}

async fn h264_fetch_media_url_bytes(
    url: &str,
    log_scope: &'static str,
) -> Result<Vec<u8>, &'static str> {
    let profiles = [
        "media-range",
        "plain-range",
        "media-norange",
        "plain-norange",
    ];
    for profile in profiles {
        let started = EmbassyInstant::now();
        crate::log!(
            "intel/hw_vid: {} fetch begin profile={} timeout_ms={} max_bytes={} url={}\n",
            log_scope,
            profile,
            H264_ONLINE_MEDIA_FETCH_TIMEOUT_MS,
            H264_ONLINE_MEDIA_FETCH_MAX_BYTES,
            url
        );
        match crate::r::net::https::get_media_bytes_profile_shared(
            url,
            profile,
            H264_ONLINE_MEDIA_FETCH_TIMEOUT_MS as u32,
            H264_ONLINE_MEDIA_FETCH_MAX_BYTES,
        )
        .await
        {
            Ok(bytes) => {
                crate::log!(
                    "intel/hw_vid: {} fetch done profile={} bytes={} waited_ms={} marker_ftyp={} marker_moov={} marker_mdat={} marker_avcc={} head_hex={} url={}\n",
                    log_scope,
                    profile,
                    bytes.len(),
                    started.elapsed().as_millis(),
                    bytes_contains(bytes.as_slice(), b"ftyp") as u8,
                    bytes_contains(bytes.as_slice(), b"moov") as u8,
                    bytes_contains(bytes.as_slice(), b"mdat") as u8,
                    bytes_contains(bytes.as_slice(), b"avcC") as u8,
                    hex_prefix(bytes.as_slice(), 24),
                    url
                );
                return Ok(bytes);
            }
            Err(err) => {
                crate::log!(
                    "intel/hw_vid: {} fetch failed profile={} err={} waited_ms={} url={}\n",
                    log_scope,
                    profile,
                    err,
                    started.elapsed().as_millis(),
                    url
                );
            }
        }
    }
    crate::log!(
        "intel/hw_vid: {} fetch exhausted profiles={} action=check-server-or-asset url={}\n",
        log_scope,
        profiles.len(),
        url
    );
    Err("online media fetch failed")
}

#[derive(Clone, Copy, Debug)]
struct Mp4Box {
    typ: [u8; 4],
    start: usize,
    payload_start: usize,
    end: usize,
}

#[derive(Clone, Copy, Debug)]
struct Mp4StscEntry {
    first_chunk: u32,
    samples_per_chunk: u32,
}

#[derive(Clone, Copy, Debug)]
struct Mp4SampleRef {
    offset: usize,
    size: usize,
    keyframe: bool,
}

struct Mp4AvcTrackInfo {
    track_id: u32,
    length_size: usize,
    sps: Vec<Vec<u8>>,
    pps: Vec<Vec<u8>>,
}

struct Mp4AvcTrack {
    track_id: u32,
    length_size: usize,
    sps: Vec<Vec<u8>>,
    pps: Vec<Vec<u8>>,
    samples: Vec<Mp4SampleRef>,
}

struct Mp4Tfhd {
    track_id: u32,
    flags: u32,
    base_data_offset: Option<u64>,
    default_sample_size: Option<usize>,
    default_sample_flags: Option<u32>,
}

fn mp4_read_u16(data: &[u8], offset: usize) -> Option<u16> {
    let bytes = data.get(offset..offset + 2)?;
    Some(u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn mp4_read_u32(data: &[u8], offset: usize) -> Option<u32> {
    let bytes = data.get(offset..offset + 4)?;
    Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn mp4_read_u64(data: &[u8], offset: usize) -> Option<u64> {
    let bytes = data.get(offset..offset + 8)?;
    Some(u64::from_be_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]))
}

fn mp4_fourcc(data: &[u8], offset: usize) -> Option<[u8; 4]> {
    let bytes = data.get(offset..offset + 4)?;
    Some([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn mp4_fourcc_name(fourcc: [u8; 4]) -> String {
    let mut text = String::new();
    for byte in fourcc {
        let ch = if byte.is_ascii_graphic() {
            byte as char
        } else {
            '?'
        };
        let _ = text.write_char(ch);
    }
    text
}

fn mp4_next_box(data: &[u8], cursor: usize, limit: usize) -> Option<Mp4Box> {
    if cursor.checked_add(8)? > limit || limit > data.len() {
        return None;
    }
    let size32 = mp4_read_u32(data, cursor)? as u64;
    let typ = mp4_fourcc(data, cursor + 4)?;
    let (payload_start, size) = if size32 == 1 {
        let size64 = mp4_read_u64(data, cursor + 8)?;
        (cursor.checked_add(16)?, size64)
    } else if size32 == 0 {
        (cursor.checked_add(8)?, (limit - cursor) as u64)
    } else {
        (cursor.checked_add(8)?, size32)
    };
    if size < (payload_start - cursor) as u64 {
        return None;
    }
    let end = cursor.checked_add(size as usize)?;
    if end > limit || end < payload_start {
        return None;
    }
    Some(Mp4Box {
        typ,
        start: cursor,
        payload_start,
        end,
    })
}

fn mp4_find_child(data: &[u8], start: usize, end: usize, typ: [u8; 4]) -> Option<Mp4Box> {
    let mut cursor = start;
    while cursor + 8 <= end {
        let Some(b) = mp4_next_box(data, cursor, end) else {
            break;
        };
        if b.typ == typ {
            return Some(b);
        }
        cursor = b.end;
    }
    None
}

fn mp4_collect_children(data: &[u8], start: usize, end: usize, typ: [u8; 4]) -> Vec<Mp4Box> {
    let mut out = Vec::new();
    let mut cursor = start;
    while cursor + 8 <= end {
        let Some(b) = mp4_next_box(data, cursor, end) else {
            break;
        };
        if b.typ == typ {
            out.push(b);
        }
        cursor = b.end;
    }
    out
}

fn mp4_parse_avcc(
    data: &[u8],
    start: usize,
    end: usize,
) -> Result<(usize, Vec<Vec<u8>>, Vec<Vec<u8>>), &'static str> {
    if end.saturating_sub(start) < 7 {
        return Err("mp4 avcC too short");
    }
    let length_size = ((data[start + 4] & 0x03) + 1) as usize;
    let mut cursor = start + 6;
    let sps_count = (data[start + 5] & 0x1f) as usize;
    let mut sps = Vec::new();
    for _ in 0..sps_count {
        let len = mp4_read_u16(data, cursor).ok_or("mp4 avcC truncated sps length")? as usize;
        cursor = cursor.saturating_add(2);
        let nal = data
            .get(cursor..cursor + len)
            .ok_or("mp4 avcC truncated sps")?;
        sps.push(nal.to_vec());
        cursor = cursor.saturating_add(len);
    }
    let pps_count = *data.get(cursor).ok_or("mp4 avcC missing pps count")? as usize;
    cursor = cursor.saturating_add(1);
    let mut pps = Vec::new();
    for _ in 0..pps_count {
        let len = mp4_read_u16(data, cursor).ok_or("mp4 avcC truncated pps length")? as usize;
        cursor = cursor.saturating_add(2);
        let nal = data
            .get(cursor..cursor + len)
            .ok_or("mp4 avcC truncated pps")?;
        pps.push(nal.to_vec());
        cursor = cursor.saturating_add(len);
    }
    if sps.is_empty() || pps.is_empty() {
        return Err("mp4 avcC missing sps or pps");
    }
    Ok((length_size, sps, pps))
}

fn mp4_parse_stsd_avc1(
    data: &[u8],
    stsd: Mp4Box,
) -> Result<(usize, Vec<Vec<u8>>, Vec<Vec<u8>>), &'static str> {
    let entry_count =
        mp4_read_u32(data, stsd.payload_start + 4).ok_or("mp4 stsd missing entry count")? as usize;
    let mut cursor = stsd.payload_start + 8;
    for _ in 0..entry_count {
        let entry =
            mp4_next_box(data, cursor, stsd.end).ok_or("mp4 stsd truncated sample entry")?;
        if entry.typ == *b"avc1" || entry.typ == *b"avc3" {
            let child_start = entry.payload_start.saturating_add(78);
            let avcc = mp4_find_child(data, child_start, entry.end, *b"avcC")
                .ok_or("mp4 avc1 missing avcC")?;
            return mp4_parse_avcc(data, avcc.payload_start, avcc.end);
        }
        cursor = entry.end;
    }
    Err("mp4 stsd has no avc1 entry")
}

fn mp4_parse_tkhd_track_id(data: &[u8], tkhd: Mp4Box) -> Result<u32, &'static str> {
    let version = *data.get(tkhd.payload_start).ok_or("mp4 tkhd too short")?;
    let track_id_offset = if version == 1 {
        tkhd.payload_start + 20
    } else {
        tkhd.payload_start + 12
    };
    mp4_read_u32(data, track_id_offset).ok_or("mp4 tkhd missing track id")
}

fn mp4_parse_avc_track_info(
    data: &[u8],
    trak: Mp4Box,
) -> Result<Option<Mp4AvcTrackInfo>, &'static str> {
    let Some(mdia) = mp4_find_child(data, trak.payload_start, trak.end, *b"mdia") else {
        return Ok(None);
    };
    let Some(hdlr) = mp4_find_child(data, mdia.payload_start, mdia.end, *b"hdlr") else {
        return Ok(None);
    };
    if mp4_fourcc(data, hdlr.payload_start + 8) != Some(*b"vide") {
        return Ok(None);
    }
    let tkhd = mp4_find_child(data, trak.payload_start, trak.end, *b"tkhd")
        .ok_or("mp4 video track missing tkhd")?;
    let minf = mp4_find_child(data, mdia.payload_start, mdia.end, *b"minf")
        .ok_or("mp4 video track missing minf")?;
    let stbl = mp4_find_child(data, minf.payload_start, minf.end, *b"stbl")
        .ok_or("mp4 video track missing stbl")?;
    let stsd = mp4_find_child(data, stbl.payload_start, stbl.end, *b"stsd")
        .ok_or("mp4 video track missing stsd")?;
    let (length_size, sps, pps) = mp4_parse_stsd_avc1(data, stsd)?;
    Ok(Some(Mp4AvcTrackInfo {
        track_id: mp4_parse_tkhd_track_id(data, tkhd)?,
        length_size,
        sps,
        pps,
    }))
}

fn mp4_parse_stsz(data: &[u8], stsz: Mp4Box) -> Result<Vec<usize>, &'static str> {
    let sample_size =
        mp4_read_u32(data, stsz.payload_start + 4).ok_or("mp4 stsz missing sample size")? as usize;
    let sample_count =
        mp4_read_u32(data, stsz.payload_start + 8).ok_or("mp4 stsz missing sample count")? as usize;
    if sample_size != 0 {
        return Ok(alloc::vec![sample_size; sample_count]);
    }
    let mut sizes = Vec::with_capacity(sample_count);
    let mut cursor = stsz.payload_start + 12;
    for _ in 0..sample_count {
        sizes.push(mp4_read_u32(data, cursor).ok_or("mp4 stsz truncated table")? as usize);
        cursor = cursor.saturating_add(4);
    }
    Ok(sizes)
}

fn mp4_parse_stsc(data: &[u8], stsc: Mp4Box) -> Result<Vec<Mp4StscEntry>, &'static str> {
    let entry_count =
        mp4_read_u32(data, stsc.payload_start + 4).ok_or("mp4 stsc missing entry count")? as usize;
    let mut entries = Vec::with_capacity(entry_count);
    let mut cursor = stsc.payload_start + 8;
    for _ in 0..entry_count {
        entries.push(Mp4StscEntry {
            first_chunk: mp4_read_u32(data, cursor).ok_or("mp4 stsc truncated first_chunk")?,
            samples_per_chunk: mp4_read_u32(data, cursor + 4)
                .ok_or("mp4 stsc truncated samples_per_chunk")?,
        });
        cursor = cursor.saturating_add(12);
    }
    if entries.is_empty() {
        return Err("mp4 stsc empty");
    }
    Ok(entries)
}

fn mp4_parse_chunk_offsets(data: &[u8], box_: Mp4Box) -> Result<Vec<u64>, &'static str> {
    let entry_count = mp4_read_u32(data, box_.payload_start + 4)
        .ok_or("mp4 chunk offset missing count")? as usize;
    let mut offsets = Vec::with_capacity(entry_count);
    let mut cursor = box_.payload_start + 8;
    if box_.typ == *b"co64" {
        for _ in 0..entry_count {
            offsets.push(mp4_read_u64(data, cursor).ok_or("mp4 co64 truncated table")?);
            cursor = cursor.saturating_add(8);
        }
    } else {
        for _ in 0..entry_count {
            offsets.push(mp4_read_u32(data, cursor).ok_or("mp4 stco truncated table")? as u64);
            cursor = cursor.saturating_add(4);
        }
    }
    Ok(offsets)
}

fn mp4_parse_stss(
    data: &[u8],
    stss: Option<Mp4Box>,
    sample_count: usize,
) -> Result<Vec<bool>, &'static str> {
    let mut keyframes = alloc::vec![stss.is_none(); sample_count];
    let Some(stss) = stss else {
        return Ok(keyframes);
    };
    keyframes.fill(false);
    let entry_count =
        mp4_read_u32(data, stss.payload_start + 4).ok_or("mp4 stss missing count")? as usize;
    let mut cursor = stss.payload_start + 8;
    for _ in 0..entry_count {
        let sample_number = mp4_read_u32(data, cursor).ok_or("mp4 stss truncated table")? as usize;
        if sample_number != 0 && sample_number <= sample_count {
            keyframes[sample_number - 1] = true;
        }
        cursor = cursor.saturating_add(4);
    }
    Ok(keyframes)
}

fn mp4_build_samples(
    sample_sizes: &[usize],
    stsc: &[Mp4StscEntry],
    chunk_offsets: &[u64],
    keyframes: &[bool],
    data_len: usize,
) -> Result<Vec<Mp4SampleRef>, &'static str> {
    let mut samples = Vec::with_capacity(sample_sizes.len());
    let mut sample_index = 0usize;
    let mut stsc_index = 0usize;
    for chunk_index0 in 0..chunk_offsets.len() {
        let chunk_number = (chunk_index0 + 1) as u32;
        if stsc_index + 1 < stsc.len() && chunk_number >= stsc[stsc_index + 1].first_chunk {
            stsc_index += 1;
        }
        let samples_per_chunk = stsc[stsc_index].samples_per_chunk as usize;
        let mut offset = chunk_offsets[chunk_index0] as usize;
        for _ in 0..samples_per_chunk {
            if sample_index >= sample_sizes.len() {
                return Ok(samples);
            }
            let size = sample_sizes[sample_index];
            let end = offset
                .checked_add(size)
                .ok_or("mp4 sample offset overflow")?;
            if end > data_len {
                return Err("mp4 sample outside file");
            }
            samples.push(Mp4SampleRef {
                offset,
                size,
                keyframe: keyframes.get(sample_index).copied().unwrap_or(false),
            });
            offset = end;
            sample_index += 1;
        }
    }
    if sample_index == 0 {
        return Err("mp4 no samples mapped");
    }
    Ok(samples)
}

fn mp4_parse_avc_track(data: &[u8], trak: Mp4Box) -> Result<Option<Mp4AvcTrack>, &'static str> {
    let Some(info) = mp4_parse_avc_track_info(data, trak)? else {
        return Ok(None);
    };
    let mdia = mp4_find_child(data, trak.payload_start, trak.end, *b"mdia")
        .ok_or("mp4 video track missing mdia")?;
    let minf = mp4_find_child(data, mdia.payload_start, mdia.end, *b"minf")
        .ok_or("mp4 video track missing minf")?;
    let stbl = mp4_find_child(data, minf.payload_start, minf.end, *b"stbl")
        .ok_or("mp4 video track missing stbl")?;
    let stsz = mp4_find_child(data, stbl.payload_start, stbl.end, *b"stsz")
        .ok_or("mp4 video track missing stsz")?;
    let stsc_box = mp4_find_child(data, stbl.payload_start, stbl.end, *b"stsc")
        .ok_or("mp4 video track missing stsc")?;
    let offset_box = mp4_find_child(data, stbl.payload_start, stbl.end, *b"stco")
        .or_else(|| mp4_find_child(data, stbl.payload_start, stbl.end, *b"co64"))
        .ok_or("mp4 video track missing chunk offsets")?;
    let sample_sizes = mp4_parse_stsz(data, stsz)?;
    let stsc = mp4_parse_stsc(data, stsc_box)?;
    let chunk_offsets = mp4_parse_chunk_offsets(data, offset_box)?;
    let keyframes = mp4_parse_stss(
        data,
        mp4_find_child(data, stbl.payload_start, stbl.end, *b"stss"),
        sample_sizes.len(),
    )?;
    let samples = mp4_build_samples(
        sample_sizes.as_slice(),
        stsc.as_slice(),
        chunk_offsets.as_slice(),
        keyframes.as_slice(),
        data.len(),
    )?;
    Ok(Some(Mp4AvcTrack {
        track_id: info.track_id,
        length_size: info.length_size,
        sps: info.sps,
        pps: info.pps,
        samples,
    }))
}

fn mp4_parse_tfhd(data: &[u8], tfhd: Mp4Box) -> Result<Mp4Tfhd, &'static str> {
    let flags =
        mp4_read_u32(data, tfhd.payload_start).ok_or("mp4 tfhd missing flags")? & 0x00ff_ffff;
    let track_id = mp4_read_u32(data, tfhd.payload_start + 4).ok_or("mp4 tfhd missing track id")?;
    let mut cursor = tfhd.payload_start + 8;
    let base_data_offset = if flags & 0x000001 != 0 {
        let value = mp4_read_u64(data, cursor).ok_or("mp4 tfhd missing base data offset")?;
        cursor = cursor.saturating_add(8);
        Some(value)
    } else {
        None
    };
    if flags & 0x000002 != 0 {
        cursor = cursor.saturating_add(4);
    }
    if flags & 0x000008 != 0 {
        cursor = cursor.saturating_add(4);
    }
    let default_sample_size = if flags & 0x000010 != 0 {
        let value =
            mp4_read_u32(data, cursor).ok_or("mp4 tfhd missing default sample size")? as usize;
        cursor = cursor.saturating_add(4);
        Some(value)
    } else {
        None
    };
    let default_sample_flags = if flags & 0x000020 != 0 {
        Some(mp4_read_u32(data, cursor).ok_or("mp4 tfhd missing default sample flags")?)
    } else {
        None
    };
    Ok(Mp4Tfhd {
        track_id,
        flags,
        base_data_offset,
        default_sample_size,
        default_sample_flags,
    })
}

fn mp4_sample_flags_keyframe(flags: u32) -> bool {
    flags == 0 || (flags & 0x0001_0000) == 0
}

fn mp4_parse_trun_samples(
    data: &[u8],
    moof: Mp4Box,
    trun: Mp4Box,
    tfhd: &Mp4Tfhd,
    fallback_data_offset: usize,
) -> Result<Vec<Mp4SampleRef>, &'static str> {
    let flags =
        mp4_read_u32(data, trun.payload_start).ok_or("mp4 trun missing flags")? & 0x00ff_ffff;
    let sample_count =
        mp4_read_u32(data, trun.payload_start + 4).ok_or("mp4 trun missing sample count")? as usize;
    let mut cursor = trun.payload_start + 8;
    let mut data_offset = fallback_data_offset as i64;
    if flags & 0x000001 != 0 {
        let raw = mp4_read_u32(data, cursor).ok_or("mp4 trun missing data offset")?;
        cursor = cursor.saturating_add(4);
        let signed = i32::from_be_bytes(raw.to_be_bytes()) as i64;
        let base = tfhd
            .base_data_offset
            .unwrap_or(if tfhd.flags & 0x020000 != 0 {
                moof.start as u64
            } else {
                moof.start as u64
            }) as i64;
        data_offset = base.saturating_add(signed);
    }
    let first_sample_flags = if flags & 0x000004 != 0 {
        let value = mp4_read_u32(data, cursor).ok_or("mp4 trun missing first sample flags")?;
        cursor = cursor.saturating_add(4);
        Some(value)
    } else {
        None
    };

    let has_duration = flags & 0x000100 != 0;
    let has_size = flags & 0x000200 != 0;
    let has_flags = flags & 0x000400 != 0;
    let has_composition_time = flags & 0x000800 != 0;
    let mut sample_offset =
        usize::try_from(data_offset).map_err(|_| "mp4 trun negative data offset")?;
    let mut samples = Vec::with_capacity(sample_count);
    for index in 0..sample_count {
        if has_duration {
            cursor = cursor.saturating_add(4);
        }
        let size = if has_size {
            let value = mp4_read_u32(data, cursor).ok_or("mp4 trun missing sample size")? as usize;
            cursor = cursor.saturating_add(4);
            value
        } else {
            tfhd.default_sample_size
                .ok_or("mp4 trun missing default sample size")?
        };
        let sample_flags = if has_flags {
            let value = mp4_read_u32(data, cursor).ok_or("mp4 trun missing sample flags")?;
            cursor = cursor.saturating_add(4);
            value
        } else if index == 0 {
            first_sample_flags
                .or(tfhd.default_sample_flags)
                .unwrap_or(0)
        } else {
            tfhd.default_sample_flags.unwrap_or(0)
        };
        if has_composition_time {
            cursor = cursor.saturating_add(4);
        }
        let sample_end = sample_offset
            .checked_add(size)
            .ok_or("mp4 trun sample offset overflow")?;
        if sample_end > data.len() {
            return Err("mp4 trun sample outside file");
        }
        samples.push(Mp4SampleRef {
            offset: sample_offset,
            size,
            keyframe: mp4_sample_flags_keyframe(sample_flags),
        });
        sample_offset = sample_end;
    }
    Ok(samples)
}

fn mp4_find_following_mdat(data: &[u8], cursor: usize) -> Option<Mp4Box> {
    let mut cursor = cursor;
    while cursor + 8 <= data.len() {
        let b = mp4_next_box(data, cursor, data.len())?;
        if b.typ == *b"mdat" {
            return Some(b);
        }
        cursor = b.end;
    }
    None
}

fn mp4_parse_fragmented_samples(
    data: &[u8],
    track_id: u32,
) -> Result<Vec<Mp4SampleRef>, &'static str> {
    let mut samples = Vec::new();
    let mut cursor = 0usize;
    while cursor + 8 <= data.len() {
        let Some(moof) = mp4_next_box(data, cursor, data.len()) else {
            break;
        };
        if moof.typ != *b"moof" {
            cursor = moof.end;
            continue;
        }
        let fallback_data_offset = mp4_find_following_mdat(data, moof.end)
            .map(|mdat| mdat.payload_start)
            .ok_or("mp4 fragment missing following mdat")?;
        let trafs = mp4_collect_children(data, moof.payload_start, moof.end, *b"traf");
        for traf in trafs {
            let Some(tfhd_box) = mp4_find_child(data, traf.payload_start, traf.end, *b"tfhd")
            else {
                continue;
            };
            let tfhd = mp4_parse_tfhd(data, tfhd_box)?;
            if tfhd.track_id != track_id {
                continue;
            }
            let truns = mp4_collect_children(data, traf.payload_start, traf.end, *b"trun");
            for trun in truns {
                samples.extend(mp4_parse_trun_samples(
                    data,
                    moof,
                    trun,
                    &tfhd,
                    fallback_data_offset,
                )?);
            }
        }
        cursor = moof.end;
    }
    if samples.is_empty() {
        return Err("mp4 fragmented video track has no samples");
    }
    Ok(samples)
}

fn mp4_emit_annexb_nal(out: &mut Vec<u8>, nal: &[u8]) {
    out.extend_from_slice(&[0, 0, 0, 1]);
    out.extend_from_slice(nal);
}

fn mp4_emit_annexb_aud(out: &mut Vec<u8>) {
    // primary_pic_type=7 keeps the marker generic for mixed I/P streams.
    out.extend_from_slice(&[0, 0, 0, 1, 0x09, 0xF0]);
}

fn mp4_emit_track_annexb(
    data: &[u8],
    track: &Mp4AvcTrack,
    mode: &str,
) -> Result<Vec<u8>, &'static str> {
    let mut out = Vec::with_capacity(data.len().min(128 * 1024 * 1024));
    for sps in track.sps.as_slice() {
        mp4_emit_annexb_nal(&mut out, sps.as_slice());
    }
    for pps in track.pps.as_slice() {
        mp4_emit_annexb_nal(&mut out, pps.as_slice());
    }
    let mut samples_emitted = 0usize;
    for sample in track.samples.as_slice() {
        if sample.keyframe {
            for sps in track.sps.as_slice() {
                mp4_emit_annexb_nal(&mut out, sps.as_slice());
            }
            for pps in track.pps.as_slice() {
                mp4_emit_annexb_nal(&mut out, pps.as_slice());
            }
        }
        mp4_emit_annexb_aud(&mut out);
        let sample_end = sample.offset + sample.size;
        let mut cursor = sample.offset;
        while cursor + track.length_size <= sample_end {
            let nal_len = match track.length_size {
                1 => data[cursor] as usize,
                2 => mp4_read_u16(data, cursor).ok_or("mp4 sample truncated nal length")? as usize,
                4 => mp4_read_u32(data, cursor).ok_or("mp4 sample truncated nal length")? as usize,
                _ => return Err("mp4 unsupported avc nal length size"),
            };
            cursor = cursor.saturating_add(track.length_size);
            if nal_len == 0 {
                continue;
            }
            let nal_end = cursor
                .checked_add(nal_len)
                .ok_or("mp4 nal length overflow")?;
            if nal_end > sample_end {
                return Err("mp4 sample nal outside sample");
            }
            mp4_emit_annexb_nal(&mut out, &data[cursor..nal_end]);
            cursor = nal_end;
        }
        samples_emitted += 1;
    }
    if out.is_empty() || samples_emitted == 0 {
        return Err("mp4 avc track produced no annexb");
    }
    crate::log!(
        "intel/hw_vid: online-media mp4-demux track=video codec=avc1 mode={} track_id={} length_size={} samples={} sps={} pps={} annexb_bytes={} first_box={}\n",
        mode,
        track.track_id,
        track.length_size,
        samples_emitted,
        track.sps.len(),
        track.pps.len(),
        out.len(),
        mp4_fourcc_name(mp4_fourcc(data, 4).unwrap_or(*b"????")).as_str()
    );
    Ok(out)
}

fn mp4_avc1_to_annexb(data: &[u8]) -> Result<Vec<u8>, &'static str> {
    let moov = mp4_find_child(data, 0, data.len(), *b"moov").ok_or("mp4 missing moov")?;
    let traks = mp4_collect_children(data, moov.payload_start, moov.end, *b"trak");
    let mut first_classic_err = None;
    for trak in traks.as_slice() {
        match mp4_parse_avc_track(data, *trak) {
            Ok(Some(track)) => return mp4_emit_track_annexb(data, &track, "classic"),
            Ok(None) => {}
            Err(err) => {
                let _ = first_classic_err.get_or_insert(err);
            }
        };
    }

    if mp4_find_child(data, 0, data.len(), *b"moof").is_some() {
        for trak in traks {
            let Some(info) = mp4_parse_avc_track_info(data, trak)? else {
                continue;
            };
            let samples = mp4_parse_fragmented_samples(data, info.track_id)?;
            let track = Mp4AvcTrack {
                track_id: info.track_id,
                length_size: info.length_size,
                sps: info.sps,
                pps: info.pps,
                samples,
            };
            return mp4_emit_track_annexb(data, &track, "fragmented");
        }
    }

    Err(first_classic_err.unwrap_or("mp4 has no avc1 video track"))
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

struct H264AccessUnit {
    stream_offset: u64,
    bytes: usize,
    nal_type: u8,
    vcl_nals: usize,
    nals: usize,
    data: Vec<u8>,
    sps: Vec<u8>,
    pps: Vec<u8>,
}

struct H264AccessUnitBuilder {
    stream_offset: u64,
    bytes: usize,
    nal_type: u8,
    vcl_nals: usize,
    nals: usize,
    data: Vec<u8>,
}

impl H264AccessUnitBuilder {
    fn new(nal: &H264BufferedNal) -> Self {
        Self {
            stream_offset: nal.meta.stream_offset,
            bytes: nal.meta.bytes,
            nal_type: nal.meta.nal_type,
            vcl_nals: 1,
            nals: 1,
            data: nal.bytes.clone(),
        }
    }

    fn push(&mut self, nal: H264BufferedNal) {
        self.bytes = nal
            .meta
            .stream_offset
            .saturating_add(nal.meta.bytes as u64)
            .saturating_sub(self.stream_offset) as usize;
        if nal.meta.nal_type == 5 {
            self.nal_type = 5;
        }
        if matches!(nal.meta.nal_type, 1 | 5) {
            self.vcl_nals += 1;
        }
        self.nals += 1;
        self.data.extend_from_slice(nal.bytes.as_slice());
    }

    fn finish(self, sps: &[u8], pps: &[u8]) -> H264AccessUnit {
        H264AccessUnit {
            stream_offset: self.stream_offset,
            bytes: self.bytes,
            nal_type: self.nal_type,
            vcl_nals: self.vcl_nals,
            nals: self.nals,
            data: self.data,
            sps: sps.to_vec(),
            pps: pps.to_vec(),
        }
    }
}

fn h264_finish_pending_access_unit(
    pending: Option<H264AccessUnitBuilder>,
    last_sps: &Option<Vec<u8>>,
    last_pps: &Option<Vec<u8>>,
    skipped_missing_headers: &mut usize,
) -> Option<H264AccessUnit> {
    let pending = pending?;
    let (Some(sps), Some(pps)) = (last_sps, last_pps) else {
        *skipped_missing_headers = skipped_missing_headers.saturating_add(1);
        return None;
    };
    Some(pending.finish(sps.as_slice(), pps.as_slice()))
}

struct H264IndexedFrame {
    stream_offset: u64,
    bytes: usize,
    nal_type: u8,
    stream_idr_index: usize,
    decode_start_frame: usize,
    detail: Option<super::h264_cmd::AvcFrameDebug>,
}

struct H264MemoryNalReader {
    scan_offset: usize,
    buffer: Vec<u8>,
}

impl H264MemoryNalReader {
    fn new(buffer: Vec<u8>) -> Self {
        Self {
            scan_offset: 0,
            buffer,
        }
    }

    async fn next_nal(&mut self) -> Option<H264BufferedNal> {
        self.try_take_nal()
    }

    fn try_take_nal(&mut self) -> Option<H264BufferedNal> {
        loop {
            let (start, start_code_len) = match h264_find_start_code(&self.buffer, self.scan_offset)
            {
                Some(found) => found,
                None => {
                    self.scan_offset = self.buffer.len();
                    return None;
                }
            };
            let payload_start = start + start_code_len;
            let next = h264_find_start_code(&self.buffer, payload_start);
            let end = if let Some((next_start, _)) = next {
                next_start
            } else {
                self.buffer.len()
            };

            self.scan_offset = end;
            if payload_start < end && payload_start < self.buffer.len() {
                let mut bytes = Vec::with_capacity(end - start);
                bytes.extend_from_slice(&self.buffer[start..end]);
                let nal_type = self.buffer[payload_start] & 0x1f;
                return Some(H264BufferedNal {
                    meta: H264StreamNal {
                        stream_offset: start as u64,
                        bytes: end - start,
                        nal_type,
                    },
                    bytes,
                });
            }

            if end > start {
                self.scan_offset = end;
            } else {
                self.scan_offset = self.scan_offset.saturating_add(1);
            }
        }
    }
}

enum H264NalReader {
    Memory(H264MemoryNalReader),
}

impl H264NalReader {
    async fn next_nal(&mut self) -> Option<H264BufferedNal> {
        match self {
            Self::Memory(reader) => reader.next_nal().await,
        }
    }
}

fn h264_ticks_to_millis(ticks: u64) -> u64 {
    let hz = embassy_time_driver::TICK_HZ.max(1);
    ((ticks as u128).saturating_mul(1_000) / hz as u128) as u64
}

fn h264_ticks_to_micros(ticks: u64) -> u64 {
    let hz = embassy_time_driver::TICK_HZ.max(1);
    ((ticks as u128).saturating_mul(1_000_000) / hz as u128) as u64
}

async fn h264_wait_until_next_frame(
    next_deadline: &mut EmbassyInstant,
    frame_period: EmbassyDuration,
    timing: &mut H264PlaybackTiming,
) {
    *next_deadline += frame_period;
    let now = EmbassyInstant::now();
    if now < *next_deadline {
        let wait_start = now.as_ticks();
        Timer::at(*next_deadline).await;
        timing.waited_frames += 1;
        timing.total_wait_ticks = timing
            .total_wait_ticks
            .saturating_add(EmbassyInstant::now().as_ticks().saturating_sub(wait_start));
    } else {
        timing.late_frames += 1;
        let late_ticks = now.saturating_duration_since(*next_deadline).as_ticks();
        timing.max_late_ticks = timing.max_late_ticks.max(late_ticks);
    }
}

async fn h264_i_p_playback_probe_annexb_bytes(
    bytes: Vec<u8>,
    source: &'static str,
    path: &str,
    mode: H264PlaybackOptions,
) -> H264PlaybackReport {
    let stream_bytes = bytes.len() as u64;
    let reader = H264NalReader::Memory(H264MemoryNalReader::new(bytes));
    h264_i_p_playback_probe_with_reader(reader, stream_bytes, source, path, mode).await
}

async fn h264_i_p_playback_probe_with_reader(
    mut reader: H264NalReader,
    stream_bytes: u64,
    source: &'static str,
    path: &str,
    mode: H264PlaybackOptions,
) -> H264PlaybackReport {
    let mut nal_count = 0usize;
    let mut idr_seen = 0usize;
    let mut p_seen = 0usize;
    let mut submitted = 0usize;
    let mut skipped_unsupported_frames = 0usize;
    let mut skipped_missing_headers = 0usize;
    let mut last_sps: Option<Vec<u8>> = None;
    let mut last_pps: Option<Vec<u8>> = None;
    let mut access_units = Vec::new();
    let mut pending_au: Option<H264AccessUnitBuilder> = None;
    let mut vcl_nals_seen = 0usize;
    let mut indexed_frames = Vec::new();
    let mut last_idr_frame: Option<usize> = None;
    let mut stopped_at = 0u64;
    let frame_period = mode.frame_period();
    let mut playback_timing = H264PlaybackTiming::default();

    crate::log!(
        "intel/hw_vid: h264-playback start bytes={} fps={} frame_ms={} frame_ticks={} subset=idr-plus-p source={} path={} mode=memory-annexb presentation=ui4-double-frame diagnostics={} noreset_lite={} stop=eos\n",
        stream_bytes,
        mode.fps(),
        mode.frame_ms(),
        frame_period.as_ticks(),
        source,
        path,
        mode.diagnostics() as u8,
        mode.noreset_lite() as u8,
    );

    while let Some(nal) = reader.next_nal().await {
        stopped_at = nal.meta.stream_offset.saturating_add(nal.meta.bytes as u64);
        nal_count += 1;
        match nal.meta.nal_type {
            7 => last_sps = Some(nal.bytes),
            8 => last_pps = Some(nal.bytes),
            9 => {
                if let Some(unit) = h264_finish_pending_access_unit(
                    pending_au.take(),
                    &last_sps,
                    &last_pps,
                    &mut skipped_missing_headers,
                ) {
                    access_units.push(unit);
                }
            }
            1 | 5 => {
                vcl_nals_seen = vcl_nals_seen.saturating_add(1);
                let begins_new_picture = pending_au.is_some()
                    && h264_slice_first_mb_in_slice(nal.bytes.as_slice()) == Some(0);
                if begins_new_picture {
                    if let Some(unit) = h264_finish_pending_access_unit(
                        pending_au.take(),
                        &last_sps,
                        &last_pps,
                        &mut skipped_missing_headers,
                    ) {
                        access_units.push(unit);
                    }
                }
                if let Some(pending) = pending_au.as_mut() {
                    pending.push(nal);
                } else {
                    pending_au = Some(H264AccessUnitBuilder::new(&nal));
                }
            }
            _ => {
                if let Some(pending) = pending_au.as_mut() {
                    pending.push(nal);
                }
            }
        }
    }
    if let Some(unit) = h264_finish_pending_access_unit(
        pending_au.take(),
        &last_sps,
        &last_pps,
        &mut skipped_missing_headers,
    ) {
        access_units.push(unit);
    }

    crate::log!(
        "intel/hw_vid: h264-access-units nals={} vcl_nals={} access_units={} missing_headers={} stopped_at=0x{:X}\n",
        nal_count,
        vcl_nals_seen,
        access_units.len(),
        skipped_missing_headers,
        stopped_at
    );

    let playback_start = EmbassyInstant::now();
    let mut next_frame_deadline = playback_start;
    for unit in access_units {
        if unit.nal_type == 5 {
            idr_seen += 1;
        } else {
            p_seen += 1;
        }
        let indexed_frame = indexed_frames.len();
        if unit.nal_type == 5 {
            last_idr_frame = Some(indexed_frame);
        }
        let mut frame = Vec::with_capacity(unit.sps.len() + unit.pps.len() + unit.data.len());
        frame.extend_from_slice(unit.sps.as_slice());
        frame.extend_from_slice(unit.pps.as_slice());
        frame.extend_from_slice(unit.data.as_slice());
        let detail = super::h264_cmd::parse_annexb_single_i_or_p_debug(frame.as_slice())
            .map_err(|err| {
                crate::log!(
                    "intel/hw_vid: h264-frame-index detail-parse-failed source_frame={} stream_idr={} nal={} offset=0x{:X} bytes=0x{:X} slices={} nals={} err={:?}\n",
                    indexed_frame + 1,
                    idr_seen,
                    unit.nal_type,
                    unit.stream_offset,
                    unit.bytes,
                    unit.vcl_nals,
                    unit.nals,
                    err
                );
                err
            })
            .ok();
        let decodable = detail.is_some();
        indexed_frames.push(H264IndexedFrame {
            stream_offset: unit.stream_offset,
            bytes: unit.bytes,
            nal_type: unit.nal_type,
            stream_idr_index: idr_seen,
            decode_start_frame: last_idr_frame.unwrap_or(indexed_frame),
            detail,
        });
        if mode.diagnostics() {
            h264_log_frame_index(&indexed_frames[indexed_frame], indexed_frame);
        }

        if !decodable {
            skipped_unsupported_frames = skipped_unsupported_frames.saturating_add(1);
            h264_wait_until_next_frame(
                &mut next_frame_deadline,
                frame_period,
                &mut playback_timing,
            )
            .await;
            continue;
        }

        submitted += 1;
        let decode_start = EmbassyInstant::now();
        let _presented = h264_submit_wait_ui4_frame(
            "forward",
            submitted,
            idr_seen,
            &frame,
            mode.diagnostics(),
            Some(&mut playback_timing),
        )
        .await;
        playback_timing.record_decode_ticks(
            EmbassyInstant::now()
                .saturating_duration_since(decode_start)
                .as_ticks(),
        );
        h264_wait_until_next_frame(&mut next_frame_deadline, frame_period, &mut playback_timing)
            .await;
    }

    h264_log_keyframe_summary(indexed_frames.as_slice(), stream_bytes);
    let playback_report =
        playback_timing.report(mode, submitted, skipped_unsupported_frames, playback_start);

    crate::log!(
        "intel/hw_vid: h264-playback done nals={} idr_seen={} p_seen={} submitted={} skipped_unsupported={} indexed_frames={} missing_headers={} stopped_at=0x{:X} target_fps={} target_frame_ms={} elapsed_ms={} effective_fps_x100={} waited_frames={} late_frames={} total_wait_ms={} avg_decode_us={} max_decode_us={} max_late_ms={} avg_queue_us={} avg_process_us={} avg_reset_us={} avg_zero_clear_us={} avg_zero_us={} avg_scratch_zero_us={} avg_output_clear_us={} avg_missing_clear_us={} avg_scratch_flush_us={} avg_build_ctx_us={} avg_poll_us={} max_poll_us={} avg_post_us={} avg_present_us={} max_present_us={} avg_poll_iters={} reason={}\n",
        nal_count,
        idr_seen,
        p_seen,
        submitted,
        skipped_unsupported_frames,
        indexed_frames.len(),
        skipped_missing_headers,
        stopped_at,
        playback_report.target_fps,
        playback_report.target_frame_ms,
        playback_report.elapsed_ms,
        playback_report.effective_fps_x100,
        playback_report.waited_frames,
        playback_report.late_frames,
        playback_report.total_wait_ms,
        playback_report.avg_decode_us,
        playback_report.max_decode_us,
        playback_report.max_late_ms,
        playback_report.avg_queue_us,
        playback_report.avg_process_us,
        playback_report.avg_reset_us,
        playback_report.avg_zero_clear_us,
        playback_report.avg_zero_us,
        playback_report.avg_scratch_zero_us,
        playback_report.avg_output_clear_us,
        playback_report.avg_missing_clear_us,
        playback_report.avg_scratch_flush_us,
        playback_report.avg_build_ctx_us,
        playback_report.avg_poll_us,
        playback_report.max_poll_us,
        playback_report.avg_post_us,
        playback_report.avg_present_us,
        playback_report.max_present_us,
        playback_report.avg_poll_iters,
        "eos"
    );
    playback_report
}

async fn h264_submit_wait_ui4_frame(
    phase: &'static str,
    playback_frame: usize,
    stream_idr_index: usize,
    encoded: &[u8],
    diagnostics: bool,
    mut timing: Option<&mut H264PlaybackTiming>,
) -> bool {
    if diagnostics {
        let before = crate::intel::hw_pic_snapshot();
        crate::log!(
            "intel/hw_vid: h264-frame submit phase={} playback_frame={} stream_idr={} bytes={} destination=ui4-double-frame pending={} outputs={} service_started={}\n",
            phase,
            playback_frame,
            stream_idr_index,
            encoded.len(),
            before.pending,
            before.outputs,
            before.service_started as u8
        );
    }

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
            return false;
        }
    };

    let Some(output) = crate::intel::hw_pic_wait_output_for_id(id, H264_DECODE_TIMEOUT_MS).await
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
        return false;
    };

    if let Some(timing) = timing.as_deref_mut() {
        timing.record_hw_pic_timing(output.timing);
    }
    let present_start = EmbassyInstant::now();
    let stored = h264_present_probe_output(phase, playback_frame, stream_idr_index, &output).await;
    if let Some(timing) = timing.as_deref_mut() {
        timing.record_present_ticks(
            EmbassyInstant::now()
                .saturating_duration_since(present_start)
                .as_ticks(),
        );
    }

    if diagnostics {
        crate::log!(
            "intel/hw_vid: h264-frame output phase={} playback_frame={} stream_idr={} id={} codec={:?} status={:?} fmt={:?} decoded={}x{} visible={}x{} pitch=0x{:X} uv=0x{:X} bytes=0x{:X} gpu=0x{:X} phys=0x{:X} stored={} destination=ui4-double-frame err={}\n",
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
            output.error_code
        );
    }
    stored
}

async fn h264_present_probe_output(
    phase: &str,
    playback_frame: usize,
    stream_idr_index: usize,
    output: &super::hw_pic::HwPicOutput,
) -> bool {
    if output.error_code != 0 {
        crate::log!(
            "intel/hw_vid: h264-present skipped reason=decode-error phase={} playback_frame={} stream_idr={} id={} err={} status={:?} fmt={:?} decoded={}x{} visible={}x{} pitch=0x{:X} uv=0x{:X} gpu=0x{:X}\n",
            phase,
            playback_frame,
            stream_idr_index,
            output.id,
            output.error_code,
            output.status,
            output.format,
            output.width,
            output.height,
            output.visible_width,
            output.visible_height,
            output.pitch_bytes,
            output.uv_offset,
            output.gpu_addr
        );
        return false;
    }
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
        let reason = format!(
            "h264-decoded-nv12:{}:frame{}:idr{}:id{}",
            phase, playback_frame, stream_idr_index, output.id
        );
        let source = crate::ui4::DecodedNv12Source {
            decode_sequence: u64::from(output.id),
            gpu: output.gpu_addr,
            phys: output.phys_addr,
            virt: output.virt_addr,
            byte_len: output.byte_len,
            width: output.width,
            height: output.height,
            visible_width: output.visible_width,
            visible_height: output.visible_height,
            pitch_bytes: output.pitch_bytes,
            uv_offset: output.uv_offset,
        };
        if !H264_UI4_HANDOFF_CHECKPOINT_LOGGED.swap(true, Ordering::AcqRel) {
            crate::log!(
                "intel/hw_vid: checkpoint stage=decode-retired-ui4-handoff phase={} playback_frame={} id={} gpu=0x{:X} phys=0x{:X} bytes=0x{:X} decoded={}x{} visible={}x{} action=acquire-ui4-frame\n",
                phase,
                playback_frame,
                output.id,
                output.gpu_addr,
                output.phys_addr,
                output.byte_len,
                output.width,
                output.height,
                output.visible_width,
                output.visible_height,
            );
        }
        let ui4_presented =
            crate::ui4::present_decoded_nv12_stream_frame(source, reason.as_str()).await;
        if ui4_presented {
            return true;
        }
        crate::log!(
            "intel/hw_vid: h264-present ui4 failed phase={} playback_frame={} stream_idr={} id={} action=drop-frame fallback=none\n",
            phase,
            playback_frame,
            stream_idr_index,
            output.id
        );
        false
    } else {
        false
    }
}

fn h264_log_keyframe_summary(frames: &[H264IndexedFrame], stream_bytes: u64) {
    let mut idrs = 0usize;
    let mut list = String::new();
    for (index, frame) in frames.iter().enumerate() {
        if frame.nal_type != 5 {
            continue;
        }
        idrs += 1;
        if !list.is_empty() {
            let _ = write!(list, ",");
        }
        let _ = write!(list, "{}@0x{:X}+0x{:X}", index + 1, frame.stream_offset, frame.bytes);
    }
    crate::log!(
        "intel/hw_vid: h264-keyframe-summary frames={} idr={} stream_bytes=0x{:X} keyframes=[{}]\n",
        frames.len(),
        idrs,
        stream_bytes,
        list.as_str()
    );
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

fn h264_slice_first_mb_in_slice(nal: &[u8]) -> Option<u32> {
    let (start, start_code_len) = h264_find_start_code(nal, 0)?;
    let payload_start = start.checked_add(start_code_len)?;
    let header = *nal.get(payload_start)?;
    if !matches!(header & 0x1f, 1 | 5) {
        return None;
    }
    let payload = nal.get(payload_start + 1..)?;
    h264_read_first_ue_from_ebsp(payload)
}

fn h264_read_first_ue_from_ebsp(payload: &[u8]) -> Option<u32> {
    let mut leading_zero_bits = 0usize;
    let mut bit_index = 0usize;
    loop {
        let bit = h264_ebsp_bit(payload, bit_index)?;
        bit_index += 1;
        if bit == 0 {
            leading_zero_bits += 1;
            if leading_zero_bits > 31 {
                return None;
            }
        } else {
            break;
        }
    }

    let mut suffix = 0u32;
    for _ in 0..leading_zero_bits {
        let bit = h264_ebsp_bit(payload, bit_index)? as u32;
        bit_index += 1;
        suffix = (suffix << 1) | bit;
    }
    Some(((1u32 << leading_zero_bits) - 1).saturating_add(suffix))
}

fn h264_ebsp_bit(payload: &[u8], bit_index: usize) -> Option<u8> {
    let mut zero_run = 0usize;
    let mut rbsp_bit = 0usize;
    for byte in payload.iter().copied() {
        if zero_run >= 2 && byte == 0x03 {
            zero_run = 0;
            continue;
        }
        let next_zero_run = if byte == 0 {
            zero_run.saturating_add(1)
        } else {
            0
        };
        for bit in (0..8).rev() {
            if rbsp_bit == bit_index {
                return Some((byte >> bit) & 1);
            }
            rbsp_bit += 1;
        }
        zero_run = next_zero_run;
    }
    None
}
