use alloc::{string::String, vec::Vec};
use core::{
    fmt::Write,
    sync::atomic::{AtomicBool, Ordering},
};
use trueos_time::{Duration as EmbassyDuration, Instant as EmbassyInstant, Timer};

const H264_DECODE_TIMEOUT_MS: u64 = 5_000;
const H264_ONLINE_MEDIA_FETCH_TIMEOUT_MS: u64 = 120_000;
const H264_ONLINE_MEDIA_FETCH_MAX_BYTES: usize = 160 * 1024 * 1024;
const H264_TRUEOSFS_VIDEO_SOFT_CAP_BYTES: usize = 1024 * 1024 * 1024;
const H264_TRUEOSFS_READ_CHUNK_BYTES: usize = 64 * 1024;
const H264_FS_INDEX_READ_AHEAD_BYTES: usize = 64 * 1024;
const H264_FS_SAMPLE_READ_AHEAD_BYTES: usize = 1024 * 1024;
const H264_FS_METADATA_CAP_BYTES: usize = 32 * 1024 * 1024;
const H264_FS_PICTURE_CAP_BYTES: usize = 8 * 1024 * 1024;
const H264_FS_READ_ERROR: i32 = -35;
const H264_MEDIA_SESSION_WAIT_MS: u64 = 5_000;
const H264_MEDIA_SESSION_RETRY_MS: u64 = 1;
const H264_UI4_PRESENT_ERROR: i32 = -34;
pub(crate) const UI4_FRAMED_VIDEO_FS_DEFAULT_PATH: &str = "x31_head_movie.annexb.h264";
const UI4_FRAMED_VIDEO_FPS: u16 = 60;
const H264_ONLINE_MEDIA_URL: &str = "https://docs.evostream.com/sample_content/assets/bun33s.mp4";

static H264_UI4_HANDOFF_CHECKPOINT_LOGGED: AtomicBool = AtomicBool::new(false);

// Diagnostic switches are shared with still-picture probes. Only the first
// playback changes them and only the final playback restores their old values.
static PLAYBACK_DIAGNOSTICS: spin::Mutex<(usize, bool, bool)> = spin::Mutex::new((0, false, false));
struct H264PlaybackGuard;
impl H264PlaybackGuard {
    fn begin() -> Self {
        let mut state = PLAYBACK_DIAGNOSTICS.lock();
        if state.0 == 0 {
            state.1 = crate::intel::hw_pic::set_detailed_logging_enabled(false);
            state.2 = crate::intel::xelp_media2_ngin::set_output_surface_probes_enabled(false);
        }
        state.0 += 1;
        Self
    }
}
impl Drop for H264PlaybackGuard {
    fn drop(&mut self) {
        let mut state = PLAYBACK_DIAGNOSTICS.lock();
        state.0 -= 1;
        if state.0 == 0 {
            crate::intel::hw_pic::set_detailed_logging_enabled(state.1);
            crate::intel::xelp_media2_ngin::set_output_surface_probes_enabled(state.2);
        }
    }
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
    pub(crate) attempted: usize,
    pub(crate) retired: usize,
    pub(crate) presented: usize,
    pub(crate) first_failure_frame: usize,
    pub(crate) first_failure_error: i32,
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
    pub(crate) conversion_queued: usize,
    pub(crate) conversion_completed: usize,
    pub(crate) conversion_backpressure_events: usize,
    pub(crate) conversion_rgba_buffer_wait_events: usize,
    pub(crate) conversion_rcs_submit_wait_events: usize,
    pub(crate) conversion_max_outstanding: usize,
    pub(crate) avg_conversion_us: u64,
    pub(crate) max_conversion_us: u64,
    pub(crate) conversion_probe: crate::ui4::DecodedVideoConversionProbeReport,
    pub(crate) avg_poll_iters: u64,
    pub(crate) mode_transitions: usize,
    pub(crate) engine_resets: usize,
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
    mode_transitions: usize,
    engine_resets: usize,
}

impl H264PlaybackTiming {
    fn record_decode_ticks(&mut self, ticks: u64) {
        self.total_decode_ticks = self.total_decode_ticks.saturating_add(ticks);
        self.max_decode_ticks = self.max_decode_ticks.max(ticks);
    }

    fn record_hw_pic_timing(&mut self, timing: crate::intel::hw_pic::HwPicTiming) {
        if timing.backend_mode_transition {
            self.mode_transitions = self.mode_transitions.saturating_add(1);
        }
        if timing.backend_engine_reset {
            self.engine_resets = self.engine_resets.saturating_add(1);
        }
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

    fn avg_us(total_us: u64, attempted: usize) -> u64 {
        if attempted == 0 {
            0
        } else {
            total_us / attempted as u64
        }
    }

    fn report(
        self,
        mode: H264PlaybackOptions,
        attempted: usize,
        retired: usize,
        presented: usize,
        first_failure_frame: usize,
        first_failure_error: i32,
        skipped_unsupported: usize,
        playback_start: EmbassyInstant,
        conversion: crate::ui4::DecodedVideoConversionReport,
    ) -> H264PlaybackReport {
        let elapsed_ms = playback_start.elapsed().as_millis();
        let effective_fps_x100 = if elapsed_ms == 0 {
            0
        } else {
            (presented as u64).saturating_mul(100_000) / elapsed_ms
        };
        let avg_decode_us = if attempted == 0 {
            0
        } else {
            h264_ticks_to_micros(self.total_decode_ticks) / attempted as u64
        };
        H264PlaybackReport {
            target_fps: mode.fps(),
            target_frame_ms: mode.frame_ms(),
            attempted,
            retired,
            presented,
            first_failure_frame,
            first_failure_error,
            skipped_unsupported,
            elapsed_ms,
            effective_fps_x100,
            waited_frames: self.waited_frames,
            late_frames: self.late_frames,
            total_wait_ms: h264_ticks_to_millis(self.total_wait_ticks),
            avg_decode_us,
            max_decode_us: h264_ticks_to_micros(self.max_decode_ticks),
            max_late_ms: h264_ticks_to_millis(self.max_late_ticks),
            avg_queue_us: Self::avg_us(self.total_queue_us, attempted),
            avg_process_us: Self::avg_us(self.total_process_us, attempted),
            avg_reset_us: Self::avg_us(self.total_reset_us, attempted),
            avg_zero_clear_us: Self::avg_us(self.total_zero_clear_us, attempted),
            avg_zero_us: Self::avg_us(self.total_zero_us, attempted),
            avg_scratch_zero_us: Self::avg_us(self.total_scratch_zero_us, attempted),
            avg_output_clear_us: Self::avg_us(self.total_output_clear_us, attempted),
            avg_missing_clear_us: Self::avg_us(self.total_missing_clear_us, attempted),
            avg_scratch_flush_us: Self::avg_us(self.total_scratch_flush_us, attempted),
            avg_build_ctx_us: Self::avg_us(self.total_build_ctx_us, attempted),
            avg_poll_us: Self::avg_us(self.total_poll_us, attempted),
            max_poll_us: self.max_poll_us,
            avg_post_us: Self::avg_us(self.total_post_us, attempted),
            avg_present_us: if attempted == 0 {
                0
            } else {
                h264_ticks_to_micros(self.total_present_ticks) / attempted as u64
            },
            max_present_us: h264_ticks_to_micros(self.max_present_ticks),
            conversion_queued: conversion.queued,
            conversion_completed: conversion.completed,
            conversion_backpressure_events: conversion.backpressure_events,
            conversion_rgba_buffer_wait_events: conversion.rgba_buffer_wait_events,
            conversion_rcs_submit_wait_events: conversion.rcs_submit_wait_events,
            conversion_max_outstanding: conversion.max_outstanding,
            avg_conversion_us: conversion.avg_conversion_us(),
            max_conversion_us: conversion.max_conversion_us,
            conversion_probe: conversion.probe,
            avg_poll_iters: if attempted == 0 {
                0
            } else {
                self.total_poll_iters / attempted as u64
            },
            mode_transitions: self.mode_transitions,
            engine_resets: self.engine_resets,
        }
    }
}

async fn h264_reserve_decode_session(
    session: crate::ui4::VideoPlaybackSession,
) -> Result<crate::intel::xelp_media2_ngin::MediaSessionGuard, &'static str> {
    let started = EmbassyInstant::now();
    let mut attempts = 0usize;
    loop {
        if session.is_cancelled() {
            return Err("playback cancelled");
        }
        attempts = attempts.saturating_add(1);
        match crate::intel::xelp_media2_ngin::try_reserve_avc_decode_session() {
            Ok(session) => {
                crate::log_info!(target: "intel-media";
                    "intel/hw_vid: media-session reserved=1 engine={} generation={} codec_mode=avc-decode submission_owner=guc direct_execlist_submit=0 scope=playback waited_ms={} attempts={}\n",
                    session.engine_name(),
                    session.generation(),
                    started.elapsed().as_millis(),
                    attempts,
                );
                return Ok(session);
            }
            Err(crate::intel::xelp_media2_ngin::MediaLaneAcquireError::Busy)
                if started.elapsed().as_millis() < H264_MEDIA_SESSION_WAIT_MS =>
            {
                Timer::after_millis(H264_MEDIA_SESSION_RETRY_MS).await;
            }
            Err(crate::intel::xelp_media2_ngin::MediaLaneAcquireError::Busy) => {
                return Err("media playback session reservation timed out");
            }
            Err(crate::intel::xelp_media2_ngin::MediaLaneAcquireError::Quarantined) => {
                return Err("media playback session is quarantined");
            }
        }
    }
}

async fn h264_wait_for_fs_index(
    session: crate::ui4::VideoPlaybackSession,
    disk: crate::disc::block::DeviceHandle,
) -> Result<(), &'static str> {
    // Index replay is a root-service lifetime, not a video's lifetime. Waiting
    // inside file_read_open_async would prevent cancellation until a cold
    // root's entire log replay finished, even though this task owns no I/O.
    crate::r::fs::trueosfs::request_warm_index(disk.id());
    loop {
        if session.is_cancelled() {
            return Err("playback cancelled");
        }
        let roots = crate::r::fs::trueosfs::list_roots();
        let root = roots
            .iter()
            .find(|root| root.disk_id == disk.id())
            .ok_or("TRUEOSFS video root disappeared")?;
        if root.index_ready {
            return Ok(());
        }
        Timer::after_millis(5).await;
    }
}

pub(crate) struct PreparedTrueosFsVideo {
    reader: H264NalReader,
    stream_bytes: u64,
    sample_timing: Vec<H264SampleTiming>,
    decode_source: &'static str,
    extent: crate::intel::media::h264_cmd::AvcStreamExtent,
}

impl PreparedTrueosFsVideo {
    pub(crate) const fn visible_extent(&self) -> (u32, u32) {
        (self.extent.visible_width, self.extent.visible_height)
    }
}

/// Open an H.264 asset and inspect its initial SPS without loading its payload.
/// Annex-B, classic MP4, and indexed Matroska AVC use on-demand range reads; fragmented MP4 retains
/// the existing buffered compatibility path.
/// The returned visible extent comes from the same SPS parser used to build
/// VDBOX commands.
pub(crate) async fn prepare_trueosfs_ui4_video(
    session: crate::ui4::VideoPlaybackSession,
    path: &str,
) -> Result<PreparedTrueosFsVideo, &'static str> {
    crate::log_info!(target: "ui4";
        "shell2/vid: stage=trueosfs-entry source=trueosfs-h264-auto asset={} next=media-engine-check\n",
        path,
    );
    if !crate::intel::has_media_decode_engine() {
        return Err("media decode engine unavailable");
    }

    let heap_before_load = crate::allocators::host_heap_integrity_bounded();
    crate::log_info!(target: "ui4";
        "shell2/vid: stage=heap-before-trueosfs-load healthy={} reason={} nodes={} current=0x{:X} next=0x{:X}\n",
        heap_before_load.healthy,
        heap_before_load.reason,
        heap_before_load.nodes,
        heap_before_load.current,
        heap_before_load.next,
    );
    if !heap_before_load.healthy {
        return Err("host heap free list corrupt before TRUEOSFS video load");
    }

    let (disk, file_path) = if let Some(selector) = path.strip_prefix("trueosfs:disc") {
        let (raw, relative) = selector
            .split_once('/')
            .ok_or("invalid TRUEOSFS video path")?;
        let raw = raw
            .parse::<u32>()
            .map_err(|_| "invalid TRUEOSFS video disc")?;
        if relative.is_empty() {
            return Err("invalid TRUEOSFS video path");
        }
        let disk_id = crate::disc::block::DiscId::from_raw(raw);
        if !crate::r::fs::trueosfs::list_roots()
            .iter()
            .any(|root| root.disk_id == disk_id)
        {
            return Err("TRUEOSFS video disc unavailable");
        }
        (
            crate::disc::block::device_handle(disk_id).ok_or("TRUEOSFS video disc unavailable")?,
            relative,
        )
    } else {
        (
            crate::r::fs::trueosfs::primary_root_handle()
                .ok_or("TRUEOSFS primary root unavailable")?,
            path,
        )
    };
    h264_wait_for_fs_index(session, disk).await?;
    let file = crate::r::fs::trueosfs::file_read_open_async(disk, file_path)
        .await
        .map_err(|_| "TRUEOSFS video stream open failed")?
        .ok_or("video asset missing from TRUEOSFS root")?;
    if session.is_cancelled() {
        return Err("playback cancelled");
    }
    let file_bytes = usize::try_from(file.data_len()).map_err(|_| "TRUEOSFS video too large")?;
    if file_bytes == 0 || file_bytes > H264_TRUEOSFS_VIDEO_SOFT_CAP_BYTES {
        return Err("TRUEOSFS video size outside playback limit");
    }
    let (mut reader, sample_timing, decode_source) = h264_open_fs_reader(session, file).await?;
    // Inspect only the initial parameter sets, then replay those NALs to the
    // picture assembler. Payload reads continue on demand during playback.
    let mut prefix = Vec::new();
    let extent = loop {
        let nal = reader
            .next_nal()
            .await
            .ok_or("TRUEOSFS H.264 SPS dimensions unavailable")?;
        if prefix.len().saturating_add(nal.bytes.len()) > H264_FS_PICTURE_CAP_BYTES {
            return Err("TRUEOSFS H.264 initial headers exceed picture limit");
        }
        prefix.extend_from_slice(&nal.bytes);
        if let Ok(extent) = super::h264_cmd::parse_annexb_stream_extent(&prefix) {
            break extent;
        }
    };
    reader.replay(prefix);
    crate::log_info!(target: "ui4";
        "shell2/vid: stage=trueosfs-format-selected asset={} decode_source={} file_bytes={} visible={}x{} payload=on-demand next=ui4-frame-open\n",
        path, decode_source, file_bytes, extent.visible_width, extent.visible_height,
    );
    Ok(PreparedTrueosFsVideo {
        reader,
        stream_bytes: file.data_len(),
        sample_timing,
        decode_source,
        extent,
    })
}

/// Run a prepared TRUEOSFS stream through VDBOX and its already-sized UI4
/// Frame. Preparation is separate so the frame can use SPS-visible geometry.
pub(crate) async fn run_prepared_trueosfs_ui4_video(
    session: crate::ui4::VideoPlaybackSession,
    path: &str,
    prepared: PreparedTrueosFsVideo,
) -> Result<H264PlaybackReport, &'static str> {
    let options = H264PlaybackOptions::new(UI4_FRAMED_VIDEO_FPS, false, true);
    let media_session = h264_reserve_decode_session(session).await?;
    let media_session_generation = media_session.generation();
    let decode_engine_name = media_session.engine_name();
    let _diagnostics = H264PlaybackGuard::begin();
    let report = h264_i_p_playback_probe_with_reader(
        session,
        prepared.reader,
        prepared.stream_bytes,
        prepared.sample_timing,
        prepared.decode_source,
        path,
        options,
        media_session_generation,
    )
    .await;
    drop(media_session);
    crate::log_info!(target: "intel-media";
        "intel/hw_vid: media-session reserved=0 engine={} generation={} codec_mode=avc-decode submission_owner=guc direct_execlist_submit=0 scope=playback attempted={} retired={} presented={} release=stream-complete\n",
        decode_engine_name,
        media_session_generation,
        report.attempted,
        report.retired,
        report.presented,
    );
    if report.first_failure_error == H264_FS_READ_ERROR {
        Err("TRUEOSFS video streaming read failed")
    } else if session.is_cancelled() && report.presented == 0 {
        Err("playback cancelled")
    } else if report.presented == 0 {
        Err("TRUEOSFS video produced no decodable frames")
    } else {
        Ok(report)
    }
}

pub(crate) async fn run_online_ui4_framed_video_playback(
    session: crate::ui4::VideoPlaybackSession,
) -> Result<H264PlaybackReport, &'static str> {
    let options = H264PlaybackOptions::new(UI4_FRAMED_VIDEO_FPS, false, true);
    crate::log!(
        "intel/hw_vid: online-ui4-framed-video stage=download-begin url={} fps={} presentation=ui4-rgba-stream rgba_buffers={}\n",
        H264_ONLINE_MEDIA_URL,
        UI4_FRAMED_VIDEO_FPS,
        crate::ui4::VIDEO_RGBA_BUFFER_COUNT,
    );
    let report = run_media_url_playback(
        session,
        H264_ONLINE_MEDIA_URL,
        options,
        "online-ui4-framed-video",
        "online-ui4-framed-video",
        None,
    )
    .await?;
    if session.is_cancelled() && report.presented == 0 {
        Err("playback cancelled")
    } else if report.presented == 0 {
        Err("online video produced no decodable frames")
    } else {
        Ok(report)
    }
}

/// Browser-resolved HTTPS media uses the same bounded download/demux/decode path
/// as the fixed online demo. Resolution and page JavaScript stay in the browser.
pub(crate) async fn run_resolved_ui4_framed_video_playback(
    session: crate::ui4::VideoPlaybackSession,
    url: &str,
) -> Result<H264PlaybackReport, &'static str> {
    let report = run_media_url_playback(
        session,
        url,
        H264PlaybackOptions::new(UI4_FRAMED_VIDEO_FPS, false, true),
        "website-ui4-video",
        "website-ui4-video",
        // Signed sources can bind the User-Agent used to resolve the player.
        // Match the browser's kernel GET identity rather than the demo profile.
        Some(crate::r::net::https::DEFAULT_USER_AGENT),
    )
    .await?;
    if report.presented == 0 {
        Err("website video produced no decodable frames")
    } else {
        Ok(report)
    }
}

async fn run_media_url_playback(
    session: crate::ui4::VideoPlaybackSession,
    url: &str,
    options: H264PlaybackOptions,
    log_scope: &'static str,
    playback_path: &'static str,
    user_agent: Option<&str>,
) -> Result<H264PlaybackReport, &'static str> {
    if !crate::intel::has_media_decode_engine() {
        return Err("media decode engine unavailable");
    }
    let mp4_bytes = h264_fetch_media_url_bytes(session, url, log_scope, user_agent).await?;
    if session.is_cancelled() {
        return Err("playback cancelled");
    }
    let demuxed = mp4_avc1_to_annexb(mp4_bytes.as_slice())?;
    crate::log!(
        "intel/hw_vid: {} demux accepted=1 container=mp4 codec=avc1 mp4_bytes={} annexb_bytes={} url={}\n",
        log_scope,
        mp4_bytes.len(),
        demuxed.annexb.len(),
        url
    );

    let media_session = h264_reserve_decode_session(session).await?;
    let media_session_generation = media_session.generation();
    let decode_engine_name = media_session.engine_name();
    let _diagnostics = H264PlaybackGuard::begin();
    let report = h264_i_p_playback_probe_annexb_bytes(
        session,
        demuxed.annexb,
        demuxed.timing,
        "media-url-mp4-avc1",
        playback_path,
        options,
        media_session_generation,
    )
    .await;
    drop(media_session);
    crate::log_info!(target: "intel-media";
        "intel/hw_vid: media-session reserved=0 engine={} generation={} codec_mode=avc-decode submission_owner=guc direct_execlist_submit=0 scope=playback attempted={} retired={} presented={} release=stream-complete\n",
        decode_engine_name,
        media_session_generation,
        report.attempted,
        report.retired,
        report.presented,
    );
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
    session: crate::ui4::VideoPlaybackSession,
    url: &str,
    log_scope: &'static str,
    user_agent: Option<&str>,
) -> Result<Vec<u8>, &'static str> {
    let profiles = [
        "media-range",
        "plain-range",
        "media-norange",
        "plain-norange",
    ];
    for profile in profiles {
        if session.is_cancelled() {
            return Err("playback cancelled");
        }
        let started = EmbassyInstant::now();
        crate::log!(
            "intel/hw_vid: {} fetch begin profile={} timeout_ms={} max_bytes={} url={}\n",
            log_scope,
            profile,
            H264_ONLINE_MEDIA_FETCH_TIMEOUT_MS,
            H264_ONLINE_MEDIA_FETCH_MAX_BYTES,
            url
        );
        let cancellation = crate::r::net::https::MediaFetchCancellation::new();
        let fetch = crate::r::net::https::get_media_bytes_profile_shared(
            url,
            profile,
            H264_ONLINE_MEDIA_FETCH_TIMEOUT_MS as u32,
            H264_ONLINE_MEDIA_FETCH_MAX_BYTES,
            &cancellation,
            user_agent,
        );
        let watch = async {
            while !session.is_cancelled() {
                Timer::after_millis(5).await;
            }
            cancellation.cancel();
        };
        let mut fetch = core::pin::pin!(fetch);
        let mut watch = core::pin::pin!(watch);
        let mut cancellation_sent = false;
        let result = core::future::poll_fn(|cx| {
            use core::future::Future;
            if !cancellation_sent && watch.as_mut().poll(cx).is_ready() {
                cancellation_sent = true;
            }
            // Always await the fetch's TLS-owner fence after requesting stop.
            fetch.as_mut().poll(cx)
        })
        .await;
        if session.is_cancelled() {
            return Err("playback cancelled");
        }
        match result {
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
                crate::log_warn!(target: "intel-media";
                    "intel/hw_vid: {} fetch failed profile={} err={} waited_ms={}\n",
                    log_scope,
                    profile,
                    err,
                    started.elapsed().as_millis(),
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct H264ColourDescription {
    matrix_coefficients: u8,
    full_range: Option<bool>,
}

impl H264ColourDescription {
    fn resolve(self, full_range: bool, matrix: u8) -> (bool, u8) {
        (
            self.full_range.unwrap_or(full_range),
            if self.matrix_coefficients == 2 {
                matrix
            } else {
                self.matrix_coefficients
            },
        )
    }
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
    decode_time: u64,
    duration: u32,
    composition_offset: i64,
}

struct Mp4AvcTrackInfo {
    track_id: u32,
    timescale: u32,
    length_size: usize,
    colour: Option<H264ColourDescription>,
    sps: Vec<Vec<u8>>,
    pps: Vec<Vec<u8>>,
}

struct Mp4AvcTrack {
    track_id: u32,
    timescale: u32,
    length_size: usize,
    colour: Option<H264ColourDescription>,
    sps: Vec<Vec<u8>>,
    pps: Vec<Vec<u8>>,
    samples: Vec<Mp4SampleRef>,
}

struct Mp4Tfhd {
    track_id: u32,
    flags: u32,
    base_data_offset: Option<u64>,
    default_sample_duration: Option<u32>,
    default_sample_size: Option<usize>,
    default_sample_flags: Option<u32>,
}

#[derive(Clone, Copy)]
struct Mp4TrexDefaults {
    duration: u32,
    size: usize,
    flags: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct H264SampleTiming {
    dts: u64,
    pts: i64,
    duration: u32,
    timescale: u32,
    colour: Option<H264ColourDescription>,
}

struct H264DemuxedAvc {
    annexb: Vec<u8>,
    timing: Vec<H264SampleTiming>,
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

fn mp4_parse_colour(data: &[u8]) -> Result<Option<H264ColourDescription>, &'static str> {
    let kind = mp4_fourcc(data, 0).ok_or("mp4 colr missing type")?;
    if kind != *b"nclx" && kind != *b"nclc" {
        return Ok(None); // ICC profiles are outside the matrix conversion path.
    }
    let matrix = mp4_read_u16(data, 8).ok_or("mp4 colr truncated matrix")?;
    let full_range = if kind == *b"nclx" {
        Some(*data.get(10).ok_or("mp4 nclx missing range")? & 0x80 != 0)
    } else {
        None // QuickTime nclc has no range flag; retain the AVC VUI value.
    };
    Ok(Some(H264ColourDescription {
        matrix_coefficients: u8::try_from(matrix)
            .map_err(|_| "mp4 colour matrix outside H.264 range")?,
        full_range,
    }))
}
fn mp4_parse_stsd_avc1(
    data: &[u8],
    stsd: Mp4Box,
) -> Result<(usize, Vec<Vec<u8>>, Vec<Vec<u8>>, Option<H264ColourDescription>), &'static str> {
    let entry_count =
        mp4_read_u32(data, stsd.payload_start + 4).ok_or("mp4 stsd missing entry count")? as usize;
    let mut cursor = stsd.payload_start + 8;
    for _ in 0..entry_count {
        let entry =
            mp4_next_box(data, cursor, stsd.end).ok_or("mp4 stsd truncated sample entry")?;
        if entry.typ == *b"avc1" || entry.typ == *b"avc3" {
            let child_start = entry.payload_start.saturating_add(78);
            let avcc = mp4_find_child(data, child_start, entry.end, *b"avcC")
                .ok_or("mp4 AVC sample entry missing avcC")?;
            let (length_size, sps, pps) = mp4_parse_avcc(data, avcc.payload_start, avcc.end)?;
            let colour = mp4_find_child(data, child_start, entry.end, *b"colr")
                .map(|colr| mp4_parse_colour(&data[colr.payload_start..colr.end]))
                .transpose()?
                .flatten();
            return Ok((length_size, sps, pps, colour));
        }
        cursor = entry.end;
    }
    Err("mp4 stsd has no avc1/avc3 entry")
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
    let mdhd = mp4_find_child(data, mdia.payload_start, mdia.end, *b"mdhd")
        .ok_or("mp4 video track missing mdhd")?;
    let mdhd_version = *data
        .get(mdhd.payload_start)
        .ok_or("mp4 mdhd missing version")?;
    let timescale_offset = if mdhd_version == 1 {
        mdhd.payload_start + 20
    } else {
        mdhd.payload_start + 12
    };
    let timescale = mp4_read_u32(data, timescale_offset).ok_or("mp4 mdhd missing timescale")?;
    if timescale == 0 {
        return Err("mp4 mdhd has zero timescale");
    }
    let tkhd = mp4_find_child(data, trak.payload_start, trak.end, *b"tkhd")
        .ok_or("mp4 video track missing tkhd")?;
    let minf = mp4_find_child(data, mdia.payload_start, mdia.end, *b"minf")
        .ok_or("mp4 video track missing minf")?;
    let stbl = mp4_find_child(data, minf.payload_start, minf.end, *b"stbl")
        .ok_or("mp4 video track missing stbl")?;
    let stsd = mp4_find_child(data, stbl.payload_start, stbl.end, *b"stsd")
        .ok_or("mp4 video track missing stsd")?;
    let (length_size, sps, pps, colour) = mp4_parse_stsd_avc1(data, stsd)?;
    Ok(Some(Mp4AvcTrackInfo {
        track_id: mp4_parse_tkhd_track_id(data, tkhd)?,
        timescale,
        length_size,
        colour,
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

fn mp4_parse_stts(
    data: &[u8],
    stts: Mp4Box,
    sample_count: usize,
) -> Result<Vec<u32>, &'static str> {
    let entry_count =
        mp4_read_u32(data, stts.payload_start + 4).ok_or("mp4 stts missing count")? as usize;
    let mut durations = Vec::with_capacity(sample_count);
    let mut cursor = stts.payload_start + 8;
    for _ in 0..entry_count {
        let count = mp4_read_u32(data, cursor).ok_or("mp4 stts truncated count")? as usize;
        let delta = mp4_read_u32(data, cursor + 4).ok_or("mp4 stts truncated delta")?;
        if delta == 0 || durations.len().saturating_add(count) > sample_count {
            return Err("mp4 stts invalid run");
        }
        durations.resize(durations.len() + count, delta);
        cursor = cursor.saturating_add(8);
    }
    if durations.len() != sample_count {
        return Err("mp4 stts sample count mismatch");
    }
    Ok(durations)
}

fn mp4_parse_ctts(
    data: &[u8],
    ctts: Option<Mp4Box>,
    sample_count: usize,
) -> Result<Vec<i64>, &'static str> {
    let Some(ctts) = ctts else {
        return Ok(alloc::vec![0; sample_count]);
    };
    let version = *data.get(ctts.payload_start).ok_or("mp4 ctts too short")?;
    if version > 1 {
        return Err("mp4 unsupported ctts version");
    }
    let entry_count =
        mp4_read_u32(data, ctts.payload_start + 4).ok_or("mp4 ctts missing count")? as usize;
    let mut offsets = Vec::with_capacity(sample_count);
    let mut cursor = ctts.payload_start + 8;
    for _ in 0..entry_count {
        let count = mp4_read_u32(data, cursor).ok_or("mp4 ctts truncated count")? as usize;
        let raw = mp4_read_u32(data, cursor + 4).ok_or("mp4 ctts truncated offset")?;
        let offset = if version == 1 {
            i64::from(i32::from_be_bytes(raw.to_be_bytes()))
        } else {
            i64::from(raw)
        };
        if offsets.len().saturating_add(count) > sample_count {
            return Err("mp4 ctts invalid run");
        }
        offsets.resize(offsets.len() + count, offset);
        cursor = cursor.saturating_add(8);
    }
    if offsets.len() != sample_count {
        return Err("mp4 ctts sample count mismatch");
    }
    Ok(offsets)
}

fn mp4_build_samples(
    sample_sizes: &[usize],
    stsc: &[Mp4StscEntry],
    chunk_offsets: &[u64],
    keyframes: &[bool],
    durations: &[u32],
    composition_offsets: &[i64],
    data_len: usize,
) -> Result<Vec<Mp4SampleRef>, &'static str> {
    let mut samples = Vec::with_capacity(sample_sizes.len());
    let mut sample_index = 0usize;
    let mut decode_time = 0u64;
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
                decode_time,
                duration: *durations
                    .get(sample_index)
                    .ok_or("mp4 stts missing sample")?,
                composition_offset: *composition_offsets
                    .get(sample_index)
                    .ok_or("mp4 ctts missing sample")?,
            });
            decode_time = decode_time.saturating_add(u64::from(durations[sample_index]));
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
    mp4_parse_avc_track_with_len(data, trak, data.len())
}

fn mp4_parse_avc_track_with_len(
    data: &[u8],
    trak: Mp4Box,
    file_len: usize,
) -> Result<Option<Mp4AvcTrack>, &'static str> {
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
    let stts = mp4_find_child(data, stbl.payload_start, stbl.end, *b"stts")
        .ok_or("mp4 video track missing stts")?;
    let durations = mp4_parse_stts(data, stts, sample_sizes.len())?;
    let composition_offsets = mp4_parse_ctts(
        data,
        mp4_find_child(data, stbl.payload_start, stbl.end, *b"ctts"),
        sample_sizes.len(),
    )?;
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
        durations.as_slice(),
        composition_offsets.as_slice(),
        file_len,
    )?;
    Ok(Some(Mp4AvcTrack {
        track_id: info.track_id,
        timescale: info.timescale,
        length_size: info.length_size,
        colour: info.colour,
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
    let default_sample_duration = if flags & 0x000008 != 0 {
        let value = mp4_read_u32(data, cursor).ok_or("mp4 tfhd missing default sample duration")?;
        cursor = cursor.saturating_add(4);
        Some(value)
    } else {
        None
    };
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
        default_sample_duration,
        default_sample_size,
        default_sample_flags,
    })
}

fn mp4_parse_trex_defaults(data: &[u8], moov: Mp4Box, track_id: u32) -> Option<Mp4TrexDefaults> {
    let mvex = mp4_find_child(data, moov.payload_start, moov.end, *b"mvex")?;
    for trex in mp4_collect_children(data, mvex.payload_start, mvex.end, *b"trex") {
        if mp4_read_u32(data, trex.payload_start + 4) == Some(track_id) {
            return Some(Mp4TrexDefaults {
                duration: mp4_read_u32(data, trex.payload_start + 12)?,
                size: mp4_read_u32(data, trex.payload_start + 16)? as usize,
                flags: mp4_read_u32(data, trex.payload_start + 20)?,
            });
        }
    }
    None
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
    decode_time: &mut u64,
) -> Result<Vec<Mp4SampleRef>, &'static str> {
    let version_flags = mp4_read_u32(data, trun.payload_start).ok_or("mp4 trun missing flags")?;
    let version = (version_flags >> 24) as u8;
    let flags = version_flags & 0x00ff_ffff;
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
        let duration = if has_duration {
            let value = mp4_read_u32(data, cursor).ok_or("mp4 trun missing sample duration")?;
            cursor = cursor.saturating_add(4);
            value
        } else {
            tfhd.default_sample_duration
                .ok_or("mp4 trun missing default sample duration")?
        };
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
        let composition_offset = if has_composition_time {
            let raw = mp4_read_u32(data, cursor).ok_or("mp4 trun missing composition time")?;
            cursor = cursor.saturating_add(4);
            if version == 1 {
                i64::from(i32::from_be_bytes(raw.to_be_bytes()))
            } else {
                i64::from(raw)
            }
        } else {
            0
        };
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
            decode_time: *decode_time,
            duration,
            composition_offset,
        });
        *decode_time = (*decode_time).saturating_add(u64::from(duration));
        sample_offset = sample_end;
    }
    Ok(samples)
}

fn mp4_parse_tfdt(data: &[u8], tfdt: Mp4Box) -> Result<u64, &'static str> {
    let version = *data.get(tfdt.payload_start).ok_or("mp4 tfdt too short")?;
    if version == 1 {
        mp4_read_u64(data, tfdt.payload_start + 4).ok_or("mp4 tfdt missing decode time")
    } else {
        mp4_read_u32(data, tfdt.payload_start + 4)
            .map(u64::from)
            .ok_or("mp4 tfdt missing decode time")
    }
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
    trex: Option<Mp4TrexDefaults>,
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
            let mut tfhd = mp4_parse_tfhd(data, tfhd_box)?;
            if tfhd.track_id != track_id {
                continue;
            }
            if let Some(trex) = trex {
                tfhd.default_sample_duration = tfhd.default_sample_duration.or(Some(trex.duration));
                tfhd.default_sample_size = tfhd.default_sample_size.or(Some(trex.size));
                tfhd.default_sample_flags = tfhd.default_sample_flags.or(Some(trex.flags));
            }
            let mut decode_time = mp4_find_child(data, traf.payload_start, traf.end, *b"tfdt")
                .map(|tfdt| mp4_parse_tfdt(data, tfdt))
                .transpose()?
                .unwrap_or_else(|| {
                    samples
                        .last()
                        .map(|sample: &Mp4SampleRef| {
                            sample
                                .decode_time
                                .saturating_add(u64::from(sample.duration))
                        })
                        .unwrap_or(0)
                });
            let truns = mp4_collect_children(data, traf.payload_start, traf.end, *b"trun");
            for trun in truns {
                samples.extend(mp4_parse_trun_samples(
                    data,
                    moof,
                    trun,
                    &tfhd,
                    fallback_data_offset,
                    &mut decode_time,
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
    // primary_pic_type=7 keeps the marker generic for mixed I/P/B streams.
    out.extend_from_slice(&[0, 0, 0, 1, 0x09, 0xF0]);
}

fn mp4_emit_track_annexb(
    data: &[u8],
    track: &Mp4AvcTrack,
    mode: &str,
) -> Result<H264DemuxedAvc, &'static str> {
    let mut out = Vec::with_capacity(data.len().min(128 * 1024 * 1024));
    let mut timing = Vec::with_capacity(track.samples.len());
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
        timing.push(H264SampleTiming {
            dts: sample.decode_time,
            pts: i64::try_from(sample.decode_time)
                .unwrap_or(i64::MAX)
                .saturating_add(sample.composition_offset),
            duration: sample.duration,
            timescale: track.timescale,
            colour: track.colour,
        });
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
    Ok(H264DemuxedAvc {
        annexb: out,
        timing,
    })
}

fn mp4_avc1_to_annexb(data: &[u8]) -> Result<H264DemuxedAvc, &'static str> {
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
            let samples = mp4_parse_fragmented_samples(
                data,
                info.track_id,
                mp4_parse_trex_defaults(data, moov, info.track_id),
            )?;
            let track = Mp4AvcTrack {
                track_id: info.track_id,
                timescale: info.timescale,
                length_size: info.length_size,
                colour: info.colour,
                sps: info.sps,
                pps: info.pps,
                samples,
            };
            return mp4_emit_track_annexb(data, &track, "fragmented");
        }
    }

    Err(first_classic_err.unwrap_or("mp4 has no avc1/avc3 video track"))
}

fn h264_has_annexb_start_code(data: &[u8]) -> bool {
    let Some(one_offset) = data.iter().position(|byte| *byte != 0) else {
        return false;
    };
    if one_offset < 2 || data[one_offset] != 1 {
        return false;
    }
    let Some(nal_header) = data.get(one_offset + 1).copied() else {
        return false;
    };
    nal_header & 0x80 == 0 && nal_header & 0x1f != 0
}

fn h264_is_mp4_container(data: &[u8]) -> bool {
    data.get(4..8) == Some(b"ftyp".as_slice())
        || mp4_find_child(data, 0, data.len(), *b"moov").is_some()
}

fn h264_prepare_trueosfs_asset(
    asset: Vec<u8>,
) -> Result<(Vec<u8>, Vec<H264SampleTiming>, &'static str, &'static str), &'static str> {
    // Test ISO BMFF first: an MP4 mdat can naturally contain Annex-B-looking
    // byte sequences even though its AVC samples are length-prefixed.
    if h264_is_mp4_container(asset.as_slice()) {
        let demuxed = mp4_avc1_to_annexb(asset.as_slice())?;
        if demuxed.annexb.is_empty() || demuxed.annexb.len() > H264_TRUEOSFS_VIDEO_SOFT_CAP_BYTES {
            return Err("demuxed TRUEOSFS H.264 size outside playback limit");
        }
        return Ok((demuxed.annexb, demuxed.timing, "trueosfs-mp4-avc", "mp4"));
    }

    if h264_has_annexb_start_code(asset.as_slice()) {
        return Ok((asset, Vec::new(), "trueosfs-root-annexb", "annexb"));
    }

    Err("unsupported TRUEOSFS video format (expected H.264 Annex-B or AVC MP4)")
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
    timing: Option<H264SampleTiming>,
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
            timing: None,
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
    emitted_nals: usize,
    source: &'static str,
    buffer: Vec<u8>,
}

impl H264MemoryNalReader {
    fn new(buffer: Vec<u8>, source: &'static str) -> Self {
        Self {
            scan_offset: 0,
            emitted_nals: 0,
            source,
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
                let nal_ordinal = self.emitted_nals.saturating_add(1);
                let trace_nal = nal_ordinal <= 8;
                if trace_nal {
                    let heap = crate::allocators::host_heap_integrity_bounded();
                    crate::log_info!(target: "ui4";
                        "shell2/vid: stage=early-nal-span source={} ordinal={} offset=0x{:X} bytes={} heap_healthy={} heap_reason={} heap_nodes={} next=nal-allocation\n",
                        self.source,
                        nal_ordinal,
                        start,
                        end - start,
                        heap.healthy,
                        heap.reason,
                        heap.nodes,
                    );
                }
                let mut bytes = Vec::with_capacity(end - start);
                if trace_nal {
                    crate::log_info!(target: "ui4";
                        "shell2/vid: stage=early-nal-reserved source={} ordinal={} capacity={} next=nal-copy\n",
                        self.source,
                        nal_ordinal,
                        bytes.capacity(),
                    );
                }
                bytes.extend_from_slice(&self.buffer[start..end]);
                let nal_type = self.buffer[payload_start] & 0x1f;
                self.emitted_nals = self.emitted_nals.saturating_add(1);
                if trace_nal {
                    crate::log_info!(target: "ui4";
                        "shell2/vid: stage=early-nal-copied source={} ordinal={} bytes={} nal_type={} next=parse-nal\n",
                        self.source,
                        nal_ordinal,
                        bytes.len(),
                        nal_type,
                    );
                }
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

/// Fill a requested range completely; partial reads are legal, EOF is not.
async fn h264_fs_read_exact(
    session: crate::ui4::VideoPlaybackSession,
    file: crate::r::fs::trueosfs::FileReadHandle,
    offset: u64,
    out: &mut [u8],
) -> Result<(), &'static str> {
    let mut done = 0;
    while done < out.len() {
        if session.is_cancelled() {
            return Err("playback cancelled");
        }
        let end = (done + H264_TRUEOSFS_READ_CHUNK_BYTES).min(out.len());
        let n = crate::r::fs::trueosfs::file_read_handle_range_async(
            file,
            offset + done as u64,
            &mut out[done..end],
        )
        .await
        .map_err(|_| "TRUEOSFS video range read failed")?
        .ok_or("TRUEOSFS video disappeared during range read")?;
        if n == 0 || n > end - done {
            return Err("TRUEOSFS video range read was short");
        }
        done += n;
    }
    Ok(())
}

// Matroska AVC indexing uses only element/block headers. Compressed pictures
// stay in TRUEOSFS and use the same range reader as MP4 (no transcoding).
fn mkv_vint(data: &[u8], keep_marker: bool) -> Result<(u64, usize), &'static str> {
    let first = *data.first().ok_or("mkv truncated vint")?;
    let width = first.leading_zeros() as usize + 1;
    if width > 8 || data.len() < width {
        return Err("mkv invalid vint");
    }
    let mut value = if keep_marker {
        first as u64
    } else {
        (first as u64) & (0xffu64 >> width)
    };
    for byte in &data[1..width] {
        value = (value << 8) | *byte as u64;
    }
    Ok((value, width))
}

// Bounded read-ahead belongs to each immutable file handle. Coalesce nearby
// container headers/pictures instead of paying TRUEOSFS validation and disk
// submission latency for every tiny read on the decode task.
struct H264FsReadAhead {
    offset: u64,
    valid: usize,
    budget: usize,
    buffer: Vec<u8>,
}
impl H264FsReadAhead {
    fn new(budget: usize) -> Self {
        Self {
            offset: 0,
            valid: 0,
            budget,
            buffer: Vec::new(),
        }
    }
    async fn read(
        &mut self,
        session: crate::ui4::VideoPlaybackSession,
        file: crate::r::fs::trueosfs::FileReadHandle,
        offset: u64,
        out: &mut [u8],
    ) -> Result<(), &'static str> {
        if session.is_cancelled() {
            return Err("playback cancelled");
        }
        if !crate::r::fs::trueosfs::file_read_handle_is_current(file) {
            return Err("TRUEOSFS video read-ahead handle expired");
        }
        let end = offset
            .checked_add(out.len() as u64)
            .filter(|end| *end <= file.data_len())
            .ok_or("video read-ahead range outside file")?;
        if out.is_empty() {
            return Ok(());
        }
        if out.len() > self.budget {
            return h264_fs_read_exact(session, file, offset, out).await;
        }
        if offset < self.offset || end > self.offset + self.valid as u64 {
            self.valid = 0; // A cancelled/failed refill must never publish partial bytes.
            let count = (file.data_len() - offset).min(self.budget as u64) as usize;
            self.buffer.resize(count, 0);
            h264_fs_read_exact(session, file, offset, &mut self.buffer).await?;
            self.offset = offset;
            self.valid = count;
        }
        let start = (offset - self.offset) as usize;
        out.copy_from_slice(&self.buffer[start..start + out.len()]);
        Ok(())
    }
}

struct MkvIndexReader {
    session: crate::ui4::VideoPlaybackSession,
    file: crate::r::fs::trueosfs::FileReadHandle,
    read_ahead: H264FsReadAhead,
}
impl MkvIndexReader {
    async fn bytes(&mut self, start: usize, end: usize) -> Result<Vec<u8>, &'static str> {
        if end < start
            || end as u64 > self.file.data_len()
            || end - start > H264_FS_METADATA_CAP_BYTES
        {
            return Err("mkv metadata outside streaming limit");
        }
        let mut data = alloc::vec![0; end - start];
        self.read_ahead
            .read(self.session, self.file, start as u64, &mut data)
            .await?;
        Ok(data)
    }
    async fn element(
        &mut self,
        start: usize,
        limit: usize,
    ) -> Result<(u64, usize, usize), &'static str> {
        let data = self
            .bytes(start, start.saturating_add(12).min(limit))
            .await?;
        let (id, n) = mkv_vint(&data, true)?;
        if n > 4 {
            return Err("mkv invalid element id");
        }
        let (size, m) = mkv_vint(&data[n..], false)?;
        let payload = start + n + m;
        let end = if size == (1u64 << (7 * m)) - 1 {
            if id != 0x18538067 {
                return Err("mkv unknown size supported only for Segment");
            }
            limit
        } else {
            payload
                .checked_add(usize::try_from(size).map_err(|_| "mkv element too large")?)
                .filter(|end| *end <= limit)
                .ok_or("mkv element outside parent")?
        };
        Ok((id, payload, end))
    }
    async fn uint(&mut self, start: usize, end: usize) -> Result<u64, &'static str> {
        if end <= start || end - start > 8 {
            return Err("mkv invalid unsigned integer");
        }
        Ok(self
            .bytes(start, end)
            .await?
            .iter()
            .fold(0, |v, b| (v << 8) | *b as u64))
    }
}

async fn mkv_open_avc_index(
    session: crate::ui4::VideoPlaybackSession,
    file: crate::r::fs::trueosfs::FileReadHandle,
) -> Result<MkvAvcIndex, &'static str> {
    let mut r = MkvIndexReader {
        session,
        file,
        read_ahead: H264FsReadAhead::new(H264_FS_INDEX_READ_AHEAD_BYTES),
    };
    let len = usize::try_from(file.data_len()).map_err(|_| "mkv file too large")?;
    let mut pos = 0;
    let (segment_start, segment_end) = loop {
        if pos >= len {
            return Err("mkv missing Segment");
        }
        let (id, start, end) = r.element(pos, len).await?;
        if id == 0x18538067 {
            break (start, end);
        }
        pos = end;
    };
    let mut scale = 1_000_000u64;
    let mut selected = None;
    let mut duration = 0u32;
    pos = segment_start;
    while pos < segment_end {
        let (id, start, end) = r.element(pos, segment_end).await?;
        if id == 0x1549A966 {
            let mut p = start;
            while p < end {
                let (id, a, b) = r.element(p, end).await?;
                if id == 0x2AD7B1 {
                    scale = r.uint(a, b).await?;
                }
                p = b;
            }
        } else if id == 0x1654AE6B {
            let mut p = start;
            while p < end {
                let (id, a, b) = r.element(p, end).await?;
                if id == 0xAE && selected.is_none() {
                    let mut q = a;
                    let mut number = 0;
                    let mut kind = 0;
                    let mut codec = Vec::new();
                    let mut private = Vec::new();
                    let mut default_duration = 0;
                    let mut unsupported = false;
                    while q < b {
                        let (id, c, d) = r.element(q, b).await?;
                        match id {
                            0xD7 => number = r.uint(c, d).await?,
                            0x83 => kind = r.uint(c, d).await?,
                            0x86 => codec = r.bytes(c, d).await?,
                            0x63A2 => private = r.bytes(c, d).await?,
                            0x23E383 => default_duration = r.uint(c, d).await?,
                            // Reject transformations rather than silently corrupting samples/timing.
                            0x6D80 | 0x23314F | 0x56AA | 0x537F => unsupported = true,
                            _ => {}
                        }
                        q = d;
                    }
                    if kind == 1 && codec == b"V_MPEG4/ISO/AVC" {
                        if unsupported {
                            return Err(
                                "mkv AVC track has unsupported encoding or timing transform",
                            );
                        }
                        let track_id =
                            u32::try_from(number).map_err(|_| "mkv track number too large")?;
                        if track_id == 0 {
                            return Err("mkv invalid track number");
                        }
                        duration = u32::try_from(default_duration)
                            .map_err(|_| "mkv frame duration too large")?;
                        if duration == 0 {
                            return Err("mkv AVC track needs DefaultDuration");
                        }
                        let (length_size, sps, pps) = mp4_parse_avcc(&private, 0, private.len())?;
                        selected = Some(Mp4AvcTrack {
                            track_id,
                            timescale: 1_000_000_000,
                            length_size,
                            colour: None,
                            sps,
                            pps,
                            samples: Vec::new(),
                        });
                    }
                }
                p = b;
            }
        } else if id == 0x1F43B675 {
            // Stop at the first cluster: do not scan the episode before UI4 opens.
            break;
        }
        pos = end;
    }
    if scale == 0 {
        return Err("mkv zero TimestampScale");
    }
    let track = selected.ok_or("mkv AVC metadata must precede first Cluster")?;
    Ok(MkvAvcIndex {
        r,
        track,
        scale,
        duration,
        pos,
        segment_end,
        cluster_end: pos,
        timestamp: 0,
        pending: None,
        finished: false,
        last_group_max_pts: None,
    })
}

struct MkvAvcIndex {
    r: MkvIndexReader,
    track: Mp4AvcTrack,
    scale: u64,
    duration: u32,
    pos: usize,
    segment_end: usize,
    cluster_end: usize,
    timestamp: u64,
    pending: Option<Mp4SampleRef>,
    finished: bool,
    last_group_max_pts: Option<i64>,
}

impl MkvAvcIndex {
    // Index only the next closed group of pictures. This bounds startup disk
    // work and supplies a complete PTS order before any picture in that group
    // is presented. Payloads are still read only by the NAL reader.
    async fn next_group(&mut self) -> Result<bool, &'static str> {
        if self.finished {
            return Ok(false);
        }
        let first = self.track.samples.len();
        if let Some(sample) = self.pending.take() {
            self.track.samples.push(sample);
        }
        loop {
            let Some(sample) = self.next_sample().await? else {
                self.finished = true;
                break;
            };
            if sample.keyframe && self.track.samples.len() > first {
                self.pending = Some(sample);
                break;
            }
            self.track.samples.push(sample);
            if self.track.samples.len() - first > 4096 {
                return Err("mkv GOP exceeds streaming lookahead limit");
            }
        }
        let group = &self.track.samples[first..];
        if group.is_empty() {
            return Ok(false);
        }
        if !group[0].keyframe {
            return Err("mkv streaming requires a keyframe at each GOP boundary");
        }
        let pts = |sample: &Mp4SampleRef| sample.decode_time as i64 + sample.composition_offset;
        let min = group.iter().map(pts).min().unwrap();
        let max = group.iter().map(pts).max().unwrap();
        if self
            .last_group_max_pts
            .is_some_and(|previous| min < previous)
        {
            return Err("mkv open-GOP presentation overlap unsupported");
        }
        self.last_group_max_pts = Some(max);
        crate::log_info!(target: "intel-media";
            "intel/hw_vid: mkv-index stage=gop-ready samples={} total_samples={} file_offset={} file_bytes={} complete={} payload=on-demand\n",
            group.len(), self.track.samples.len(), self.pos,
            self.r.file.data_len(), self.finished as u8);
        Ok(true)
    }

    async fn next_sample(&mut self) -> Result<Option<Mp4SampleRef>, &'static str> {
        loop {
            if self.r.session.is_cancelled() {
                return Err("playback cancelled");
            }
            if self.pos >= self.cluster_end {
                if self.pos >= self.segment_end {
                    return Ok(None);
                }
                let (id, start, end) = self.r.element(self.pos, self.segment_end).await?;
                self.pos = end;
                if id != 0x1F43B675 {
                    continue;
                }
                self.cluster_end = end;
                self.pos = start;
                let mut p = start;
                let mut timestamp = None;
                while p < end {
                    let (id, a, b) = self.r.element(p, end).await?;
                    if id == 0xE7 {
                        timestamp = Some(self.r.uint(a, b).await?);
                        break;
                    }
                    p = b;
                }
                self.timestamp = timestamp.ok_or("mkv cluster missing Timestamp")?;
                Timer::after_millis(1).await;
            }
            let (id, a, b) = self.r.element(self.pos, self.cluster_end).await?;
            self.pos = b;
            let mut block = if id == 0xA3 { Some((a, b, true)) } else { None };
            if id == 0xA0 {
                let mut q = a;
                let mut reference = false;
                while q < b {
                    let (id, c, d) = self.r.element(q, b).await?;
                    if id == 0xA1 {
                        block = Some((c, d, true));
                    }
                    if id == 0xFB {
                        reference = true;
                    }
                    if id == 0xA4 {
                        return Err("mkv CodecState changes unsupported");
                    }
                    q = d;
                }
                if reference {
                    if let Some(block) = &mut block {
                        block.2 = false;
                    }
                }
            }
            let Some((a, b, keyframe_flag)) = block else {
                continue;
            };
            let header = self.r.bytes(a, (a + 11).min(b)).await?;
            let (number, n) = mkv_vint(&header, false)?;
            if number != self.track.track_id as u64 {
                continue;
            }
            if header.len() < n + 3 {
                return Err("mkv truncated block header");
            }
            let flags = header[n + 2];
            if flags & 0x0e != 0 {
                return Err("mkv laced or invisible AVC blocks unsupported");
            }
            let keyframe = if id == 0xA3 {
                flags & 0x80 != 0
            } else {
                keyframe_flag
            };
            let relative = i16::from_be_bytes([header[n], header[n + 1]]) as i64;
            let ticks = i64::try_from(self.timestamp)
                .map_err(|_| "mkv timestamp too large")?
                .checked_add(relative)
                .ok_or("mkv timestamp overflow")?;
            let pts = ticks
                .checked_mul(i64::try_from(self.scale).map_err(|_| "mkv scale too large")?)
                .ok_or("mkv timestamp overflow")?;
            let offset = a + n + 3;
            if b <= offset || b - offset > H264_FS_PICTURE_CAP_BYTES {
                return Err("mkv AVC picture outside decoder limit");
            }
            if self.track.samples.len()
                >= H264_FS_METADATA_CAP_BYTES / core::mem::size_of::<Mp4SampleRef>()
            {
                return Err("mkv sample index exceeds streaming limit");
            }
            let dts = (self.track.samples.len() as u64)
                .checked_mul(self.duration as u64)
                .ok_or("mkv decode time overflow")?;
            let composition_offset = pts
                .checked_sub(i64::try_from(dts).map_err(|_| "mkv decode time too large")?)
                .ok_or("mkv composition time overflow")?;
            return Ok(Some(Mp4SampleRef {
                offset,
                size: b - offset,
                keyframe,
                decode_time: dts,
                duration: self.duration,
                composition_offset,
            }));
        }
    }
}

fn h264_extend_mkv_timing(
    track: &Mp4AvcTrack,
    timing: &mut Vec<H264SampleTiming>,
    ranks: &mut Vec<usize>,
    base_pts: &mut i64,
) -> bool {
    let first = timing.len();
    if first >= track.samples.len() {
        return false;
    }
    let new_timing: Vec<_> = track.samples[first..]
        .iter()
        .map(|sample| H264SampleTiming {
            dts: sample.decode_time,
            pts: sample.decode_time as i64 + sample.composition_offset,
            duration: sample.duration,
            timescale: track.timescale,
            colour: track.colour,
        })
        .collect();
    if first == 0 {
        *base_pts = new_timing.iter().map(|t| t.pts).min().unwrap_or(0);
    }
    let mut order: Vec<_> = (0..new_timing.len()).collect();
    order.sort_by_key(|&i| (new_timing[i].pts, new_timing[i].dts, i));
    ranks.resize(first + new_timing.len(), 0);
    for (rank, index) in order.into_iter().enumerate() {
        ranks[first + index] = first + rank;
    }
    timing.extend(new_timing);
    true
}

async fn h264_open_fs_reader(
    session: crate::ui4::VideoPlaybackSession,
    file: crate::r::fs::trueosfs::FileReadHandle,
) -> Result<(H264NalReader, Vec<H264SampleTiming>, &'static str), &'static str> {
    let len = file.data_len() as usize;
    let mut header = [0u8; 16];
    h264_fs_read_exact(session, file, 0, &mut header[..len.min(16)]).await?;
    if h264_has_annexb_start_code(&header[..len.min(16)]) {
        return Ok((
            H264NalReader::File(H264FileNalReader::new(session, file, None)),
            Vec::new(),
            "trueosfs-stream-annexb",
        ));
    }
    if header[..len.min(16)].starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        let index = mkv_open_avc_index(session, file).await?;
        let mut prefix = Vec::new();
        for nal in index.track.sps.iter().chain(&index.track.pps) {
            mp4_emit_annexb_nal(&mut prefix, nal);
        }
        let mut reader = H264FileNalReader::new(session, file, None);
        reader.replay = Some(H264MemoryNalReader::new(prefix, "mkv-headers"));
        reader.mkv = Some(index);
        return Ok((H264NalReader::File(reader), Vec::new(), "trueosfs-stream-mkv-avc"));
    }
    // Box headers let us skip mdat without reading its payload, including when
    // moov sits at the end of the file. Offsets in sample tables remain absolute.
    let mut cursor = 0usize;
    let mut moov_data = None;
    let mut fragmented = false;
    while cursor < len {
        let available = (len - cursor).min(16);
        if available < 8 {
            return Err("mp4 truncated box header");
        }
        h264_fs_read_exact(session, file, cursor as u64, &mut header[..available]).await?;
        let size32 = mp4_read_u32(&header, 0).ok_or("mp4 invalid box size")?;
        let (size, header_len) = if size32 == 1 {
            if available < 16 {
                return Err("mp4 truncated extended box header");
            }
            (
                usize::try_from(mp4_read_u64(&header, 8).unwrap())
                    .map_err(|_| "mp4 box too large")?,
                16,
            )
        } else if size32 == 0 {
            (len - cursor, 8)
        } else {
            (size32 as usize, 8)
        };
        let end = cursor
            .checked_add(size)
            .filter(|&end| end <= len && size >= header_len)
            .ok_or("mp4 box outside file")?;
        match mp4_fourcc(&header, 4) {
            Some(typ) if typ == *b"moov" => {
                if size > H264_FS_METADATA_CAP_BYTES {
                    return Err("mp4 metadata exceeds streaming limit");
                }
                let mut bytes = alloc::vec![0; size];
                h264_fs_read_exact(session, file, cursor as u64, &mut bytes).await?;
                moov_data = Some(bytes);
            }
            Some(typ) if typ == *b"moof" => fragmented = true,
            _ => {}
        }
        cursor = end;
    }
    let data = moov_data.ok_or("mp4 missing moov")?;
    let moov = mp4_next_box(&data, 0, data.len()).ok_or("mp4 invalid moov")?;
    for trak in mp4_collect_children(&data, moov.payload_start, moov.end, *b"trak") {
        if let Ok(Some(track)) = mp4_parse_avc_track_with_len(&data, trak, len) {
            let timing = track
                .samples
                .iter()
                .map(|sample| H264SampleTiming {
                    dts: sample.decode_time,
                    pts: i64::try_from(sample.decode_time)
                        .unwrap_or(i64::MAX)
                        .saturating_add(sample.composition_offset),
                    duration: sample.duration,
                    timescale: track.timescale,
                    colour: track.colour,
                })
                .collect();
            return Ok((
                H264NalReader::File(H264FileNalReader::new(session, file, Some(track))),
                timing,
                "trueosfs-stream-mp4-avc",
            ));
        }
    }
    if fragmented {
        // Preserve existing fragmented MP4 support until its fragment metadata
        // can be loaded independently. Ordinary MP4 and Annex-B stream above.
        let mut asset = Vec::new();
        asset
            .try_reserve_exact(len)
            .map_err(|_| "TRUEOSFS video asset allocation failed")?;
        asset.resize(len, 0);
        h264_fs_read_exact(session, file, 0, &mut asset).await?;
        let demuxed = mp4_avc1_to_annexb(&asset)?;
        return Ok((
            H264NalReader::Memory(H264MemoryNalReader::new(
                demuxed.annexb,
                "trueosfs-fragmented-mp4",
            )),
            demuxed.timing,
            "trueosfs-fragmented-mp4-buffered",
        ));
    }
    Err("mp4 has no supported AVC sample table")
}

struct H264FileNalReader {
    session: crate::ui4::VideoPlaybackSession,
    file: crate::r::fs::trueosfs::FileReadHandle,
    track: Option<Mp4AvcTrack>,
    mkv: Option<MkvAvcIndex>,
    sample: usize,
    offset: u64,
    buffer_offset: u64,
    buffer: Vec<u8>,
    scan: usize,
    eof: bool,
    failed: bool,
    replay: Option<H264MemoryNalReader>,
    sample_reader: Option<H264MemoryNalReader>,
    sample_read_ahead: H264FsReadAhead,
}

impl H264FileNalReader {
    fn new(
        session: crate::ui4::VideoPlaybackSession,
        file: crate::r::fs::trueosfs::FileReadHandle,
        track: Option<Mp4AvcTrack>,
    ) -> Self {
        Self {
            session,
            file,
            track,
            mkv: None,
            sample: 0,
            offset: 0,
            buffer_offset: 0,
            buffer: Vec::new(),
            scan: 0,
            eof: false,
            failed: false,
            replay: None,
            sample_reader: None,
            sample_read_ahead: H264FsReadAhead::new(H264_FS_SAMPLE_READ_AHEAD_BYTES),
        }
    }
    async fn next_nal(&mut self) -> Option<H264BufferedNal> {
        match self.read_nal().await {
            Ok(nal) => nal,
            Err(reason) => {
                if !self.session.is_cancelled() {
                    crate::log_error!(target: "intel-media"; "intel/hw_vid: streaming-read failed reason={} offset={} sample={}\n", reason, self.offset, self.sample);
                    self.failed = true;
                }
                None
            }
        }
    }
    async fn read_nal(&mut self) -> Result<Option<H264BufferedNal>, &'static str> {
        if self.failed || self.session.is_cancelled() {
            return Ok(None);
        }
        if let Some(replay) = &mut self.replay {
            if let Some(nal) = replay.next_nal().await {
                return Ok(Some(nal));
            }
            self.replay = None;
        }
        if self.track.is_some() || self.mkv.is_some() {
            loop {
                if let Some(reader) = &mut self.sample_reader {
                    if let Some(mut nal) = reader.next_nal().await {
                        nal.meta.stream_offset += self.buffer_offset;
                        return Ok(Some(nal));
                    }
                    self.sample_reader = None;
                }
                if let Some(index) = &mut self.mkv {
                    if self.sample >= index.track.samples.len() && !index.next_group().await? {
                        return Ok(None);
                    }
                }
                let track = self
                    .mkv
                    .as_ref()
                    .map(|index| &index.track)
                    .or(self.track.as_ref())
                    .ok_or("missing AVC track")?;
                let Some(sample) = track.samples.get(self.sample) else {
                    return Ok(None);
                };
                if sample.size > H264_FS_PICTURE_CAP_BYTES {
                    return Err("mp4 sample exceeds decoder picture limit");
                }
                let mut payload = alloc::vec![0; sample.size];
                self.sample_read_ahead
                    .read(self.session, self.file, sample.offset as u64, &mut payload)
                    .await?;
                let mut annexb = Vec::new();
                mp4_emit_annexb_aud(&mut annexb);
                // Parameter sets are cheap; retain the same per-picture header
                // contract used by the existing decoder, including avc3 updates.
                if self.sample == 0 || sample.keyframe {
                    for nal in track.sps.iter().chain(&track.pps) {
                        mp4_emit_annexb_nal(&mut annexb, nal);
                    }
                }
                let mut cursor = 0;
                while cursor + track.length_size <= payload.len() {
                    let n = match track.length_size {
                        1 => payload[cursor] as usize,
                        2 => mp4_read_u16(&payload, cursor).unwrap() as usize,
                        4 => mp4_read_u32(&payload, cursor).unwrap() as usize,
                        _ => return Err("mp4 unsupported AVC length size"),
                    };
                    cursor += track.length_size;
                    let end = cursor
                        .checked_add(n)
                        .filter(|&end| end <= payload.len())
                        .ok_or("mp4 nal outside sample")?;
                    if n != 0 {
                        mp4_emit_annexb_nal(&mut annexb, &payload[cursor..end]);
                    }
                    cursor = end;
                }
                if cursor != payload.len() {
                    return Err("mp4 truncated NAL length");
                }
                self.buffer_offset = sample.offset as u64;
                self.sample += 1;
                self.sample_reader = Some(H264MemoryNalReader::new(annexb, "fs-mp4-sample"));
            }
        }
        loop {
            if let Some((start, code_len)) = h264_find_start_code(&self.buffer, 0) {
                // Resume scanning near the last chunk boundary; do not rescan
                // a large NAL from its start for each disk read.
                let next = h264_find_start_code(&self.buffer, self.scan.max(start + code_len));
                if let Some((end, _)) = next.or_else(|| self.eof.then_some((self.buffer.len(), 0)))
                {
                    if end - start > H264_FS_PICTURE_CAP_BYTES {
                        return Err("Annex-B NAL exceeds decoder picture limit");
                    }
                    if start + code_len < end {
                        let nal_type = self.buffer[start + code_len] & 0x1f;
                        let bytes = self.buffer[start..end].to_vec();
                        let offset = self.buffer_offset + start as u64;
                        self.buffer.drain(..end);
                        self.buffer_offset += end as u64;
                        self.scan = 0;
                        return Ok(Some(H264BufferedNal {
                            meta: H264StreamNal {
                                stream_offset: offset,
                                bytes: bytes.len(),
                                nal_type,
                            },
                            bytes,
                        }));
                    }
                    self.buffer.drain(..end);
                    self.buffer_offset += end as u64;
                    self.scan = 0;
                    continue;
                }
                self.scan = self.buffer.len().saturating_sub(3).max(start + code_len);
            } else if self.buffer.len() > 3 {
                let discard = self.buffer.len() - 3;
                self.buffer.drain(..discard);
                self.buffer_offset += discard as u64;
            }
            if self.eof {
                return Ok(None);
            }
            if self.buffer.len() > H264_FS_PICTURE_CAP_BYTES {
                return Err("Annex-B NAL exceeds decoder picture limit");
            }
            let n = (self.file.data_len() - self.offset).min(H264_TRUEOSFS_READ_CHUNK_BYTES as u64)
                as usize;
            if n == 0 {
                self.eof = true;
                continue;
            }
            let old_len = self.buffer.len();
            self.buffer.resize(old_len + n, 0);
            h264_fs_read_exact(self.session, self.file, self.offset, &mut self.buffer[old_len..])
                .await?;
            self.offset += n as u64;
            self.eof = self.offset == self.file.data_len();
        }
    }
}

async fn h264_next_stream_access_unit(
    session: crate::ui4::VideoPlaybackSession,
    reader: &mut H264NalReader,
    pending: &mut Option<H264AccessUnitBuilder>,
    sps: &mut Option<Vec<u8>>,
    pps: &mut Option<Vec<u8>>,
    missing: &mut usize,
    nal_count: &mut usize,
) -> Option<H264AccessUnit> {
    loop {
        if session.is_cancelled() {
            return None;
        }
        let Some(nal) = reader.next_nal().await else {
            if reader.failed() {
                return None;
            }
            return h264_finish_pending_access_unit(pending.take(), sps, pps, missing);
        };
        *nal_count += 1;
        if *nal_count % 64 == 0 {
            Timer::after_millis(1).await;
        }
        let boundary = nal.meta.nal_type == 9
            || (matches!(nal.meta.nal_type, 1 | 5)
                && pending.is_some()
                && h264_slice_first_mb_in_slice(&nal.bytes) == Some(0));
        let complete = if boundary {
            h264_finish_pending_access_unit(pending.take(), sps, pps, missing)
        } else {
            None
        };
        match nal.meta.nal_type {
            7 => *sps = Some(nal.bytes),
            8 => *pps = Some(nal.bytes),
            9 => {}
            1 | 5 if pending.is_none() => *pending = Some(H264AccessUnitBuilder::new(&nal)),
            _ => {
                if let Some(unit) = pending.as_mut() {
                    unit.push(nal);
                }
            }
        }
        if pending
            .as_ref()
            .is_some_and(|unit| unit.data.len() > H264_FS_PICTURE_CAP_BYTES)
        {
            if let H264NalReader::File(r) = reader {
                r.failed = true;
            }
            return None;
        }
        if complete.is_some() {
            return complete;
        }
    }
}

enum H264NalReader {
    Memory(H264MemoryNalReader),
    File(H264FileNalReader),
}

impl H264NalReader {
    fn mkv_track(&self) -> Option<&Mp4AvcTrack> {
        match self {
            Self::File(reader) => reader.mkv.as_ref().map(|index| &index.track),
            _ => None,
        }
    }
    fn streaming(&self) -> bool {
        matches!(self, Self::File(_))
    }
    fn failed(&self) -> bool {
        matches!(self, Self::File(r) if r.failed)
    }
    fn replay(&mut self, bytes: Vec<u8>) {
        match self {
            Self::File(r) => r.replay = Some(H264MemoryNalReader::new(bytes, "fs-prefix")),
            Self::Memory(r) => r.scan_offset = 0,
        }
    }
    async fn next_nal(&mut self) -> Option<H264BufferedNal> {
        match self {
            Self::Memory(reader) => reader.next_nal().await,
            Self::File(reader) => reader.next_nal().await,
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

async fn h264_wait_until_or_cancelled(
    session: crate::ui4::VideoPlaybackSession,
    deadline: EmbassyInstant,
) {
    while !session.is_cancelled() && EmbassyInstant::now() < deadline {
        Timer::at(deadline.min(EmbassyInstant::now() + EmbassyDuration::from_millis(5))).await;
    }
}

async fn h264_wait_until_next_frame(
    session: crate::ui4::VideoPlaybackSession,
    next_deadline: &mut EmbassyInstant,
    frame_period: EmbassyDuration,
    timing: &mut H264PlaybackTiming,
) {
    *next_deadline += frame_period;
    let now = EmbassyInstant::now();
    if now < *next_deadline {
        let wait_start = now.as_ticks();
        h264_wait_until_or_cancelled(session, *next_deadline).await;
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
    session: crate::ui4::VideoPlaybackSession,
    bytes: Vec<u8>,
    sample_timing: Vec<H264SampleTiming>,
    source: &'static str,
    path: &str,
    mode: H264PlaybackOptions,
    media_session_generation: u64,
) -> H264PlaybackReport {
    let stream_bytes = bytes.len() as u64;
    let reader = H264NalReader::Memory(H264MemoryNalReader::new(bytes, source));
    crate::log_info!(target: "ui4";
        "shell2/vid: stage=annexb-reader-ready source={} bytes={} next=parse-state-init\n",
        source,
        stream_bytes,
    );
    h264_i_p_playback_probe_with_reader(
        session,
        reader,
        stream_bytes,
        sample_timing,
        source,
        path,
        mode,
        media_session_generation,
    )
    .await
}

async fn h264_i_p_playback_probe_with_reader(
    session: crate::ui4::VideoPlaybackSession,
    mut reader: H264NalReader,
    stream_bytes: u64,
    mut sample_timing: Vec<H264SampleTiming>,
    source: &'static str,
    path: &str,
    mode: H264PlaybackOptions,
    media_session_generation: u64,
) -> H264PlaybackReport {
    let mut nal_count = 0usize;
    let mut idr_seen = 0usize;
    let mut p_seen = 0usize;
    let mut b_seen = 0usize;
    let mut attempted = 0usize;
    let mut retired = 0usize;
    let mut first_failure_frame = 0usize;
    let mut first_failure_error = 0i32;
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

    crate::log_info!(target: "ui4";
        "shell2/vid: stage=annexb-parse-loop-enter source={} bytes={} next=first-nal\n",
        source,
        stream_bytes,
    );

    crate::log!(
        "intel/hw_vid: h264-playback start bytes={} fps={} frame_ms={} frame_ticks={} subset=progressive-i-p-b source={} path={} mode=memory-annexb presentation=pts-ordered-ui4-rgba-stream rgba_buffers={} diagnostics={} noreset_lite={} stop=eos\n",
        stream_bytes,
        mode.fps(),
        mode.frame_ms(),
        frame_period.as_ticks(),
        source,
        path,
        crate::ui4::VIDEO_RGBA_BUFFER_COUNT,
        mode.diagnostics() as u8,
        mode.noreset_lite() as u8,
    );

    let streaming = reader.streaming();
    while !streaming {
        let Some(nal) = reader.next_nal().await else {
            break;
        };
        if nal_count % 64 == 0 {
            Timer::after_millis(1).await;
        }
        if session.is_cancelled() {
            break;
        }
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
    if !streaming && !sample_timing.is_empty() && sample_timing.len() == access_units.len() {
        for (unit, timing) in access_units.iter_mut().zip(sample_timing.iter().copied()) {
            unit.timing = Some(timing);
        }
    } else if !streaming && !sample_timing.is_empty() && !session.is_cancelled() {
        crate::log_error!(
            "intel/hw_vid: mp4-timing rejected=1 reason=sample-access-unit-count-mismatch samples={} access_units={} action=reject-stream\n",
            sample_timing.len(),
            access_units.len(),
        );
        skipped_unsupported_frames = skipped_unsupported_frames.saturating_add(access_units.len());
        access_units.clear();
    }

    crate::log!(
        "intel/hw_vid: h264-access-units nals={} vcl_nals={} access_units={} missing_headers={} stopped_at=0x{:X}\n",
        nal_count,
        vcl_nals_seen,
        access_units.len(),
        skipped_missing_headers,
        stopped_at
    );

    if !crate::ui4::begin_decoded_nv12_conversion_batch(session) {
        crate::log_error!(
            "intel/hw_vid: conversion-batch accepted=0 reason=prior-batch-not-idle action=ordered-wait-no-drop\n"
        );
    }
    let mut playback_start = EmbassyInstant::now();
    let mut next_frame_deadline = playback_start;
    let timing: Vec<_> = if streaming {
        sample_timing.iter().copied().map(Some).collect()
    } else {
        access_units.iter().map(|unit| unit.timing).collect()
    };
    let access_unit_count = timing.len();
    let incremental_mkv = reader.mkv_track().is_some();
    let presentation_reordering_required = incremental_mkv
        || timing.windows(2).any(|pair| {
            matches!((pair[0], pair[1]), (Some(previous), Some(next))
            if (next.pts, next.dts) < (previous.pts, previous.dts))
        });
    let mut presentation_rank = Vec::new();
    let mut base_pts = timing
        .first()
        .copied()
        .flatten()
        .map(|t| t.pts)
        .unwrap_or(0);
    if presentation_reordering_required && !incremental_mkv {
        let mut indices: Vec<usize> = (0..access_unit_count).collect();
        indices.sort_by_key(|&i| timing[i].map(|t| (t.pts, t.dts, i)));
        base_pts = timing[indices[0]].unwrap().pts;
        presentation_rank.resize(access_unit_count, 0);
        for (rank, index) in indices.into_iter().enumerate() {
            presentation_rank[index] = rank;
        }
    }
    let mut presentation_slots = if presentation_reordering_required {
        alloc::vec![H264PresentationSlot::Waiting; access_unit_count]
    } else {
        Vec::new()
    };
    let mut next_presentation_rank = 0usize;

    let mut buffered_units = access_units.into_iter();
    let mut decode_index = 0usize;
    loop {
        let next = if streaming {
            h264_next_stream_access_unit(
                session,
                &mut reader,
                &mut pending_au,
                &mut last_sps,
                &mut last_pps,
                &mut skipped_missing_headers,
                &mut nal_count,
            )
            .await
        } else {
            buffered_units.next()
        };
        let Some(mut unit) = next else {
            if reader.failed()
                || (streaming
                    && !session.is_cancelled()
                    && !sample_timing.is_empty()
                    && decode_index != sample_timing.len())
            {
                first_failure_frame = attempted.saturating_add(1);
                first_failure_error = H264_FS_READ_ERROR;
            }
            break;
        };
        if let Some(track) = reader.mkv_track() {
            if h264_extend_mkv_timing(
                track,
                &mut sample_timing,
                &mut presentation_rank,
                &mut base_pts,
            ) {
                presentation_slots.resize(sample_timing.len(), H264PresentationSlot::Waiting);
            }
        }
        if streaming && !sample_timing.is_empty() {
            let Some(t) = sample_timing.get(decode_index).copied() else {
                first_failure_frame = attempted.saturating_add(1);
                first_failure_error = H264_FS_READ_ERROR;
                break;
            };
            unit.timing = Some(t);
        }
        if incremental_mkv && decode_index == 0 {
            // Initial GOP lookahead is preparation time, not elapsed movie time.
            playback_start = EmbassyInstant::now();
            next_frame_deadline = playback_start;
        }
        stopped_at = unit.stream_offset.saturating_add(unit.bytes as u64);
        let current_decode_index = decode_index;
        decode_index += 1;
        if !session.wait_until_playing().await {
            break;
        }
        let rank = if presentation_reordering_required {
            presentation_rank[current_decode_index]
        } else {
            0
        };
        if unit.nal_type == 5 {
            idr_seen += 1;
        }
        let indexed_frame = indexed_frames.len();
        if unit.nal_type == 5 {
            last_idr_frame = Some(indexed_frame);
        }
        let mut frame = Vec::with_capacity(unit.sps.len() + unit.pps.len() + unit.data.len());
        frame.extend_from_slice(unit.sps.as_slice());
        frame.extend_from_slice(unit.pps.as_slice());
        frame.extend_from_slice(unit.data.as_slice());
        let detail = super::h264_cmd::parse_annexb_single_picture_debug(frame.as_slice())
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
        match detail.as_ref().map(|detail| detail.class) {
            Some(super::h264_cmd::AvcSliceClass::P) => p_seen += 1,
            Some(super::h264_cmd::AvcSliceClass::B) => b_seen += 1,
            _ => {}
        }
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

        let presentation_slot = if !decodable {
            skipped_unsupported_frames = skipped_unsupported_frames.saturating_add(1);
            H264PresentationSlot::Skipped(unit.timing)
        } else {
            attempted += 1;
            let decode_start = EmbassyInstant::now();
            let slot = match h264_decode_wait_frame(
                "dts",
                attempted,
                idr_seen,
                &frame,
                mode.diagnostics(),
                media_session_generation,
                Some(&mut playback_timing),
            )
            .await
            {
                Ok(mut output) => {
                    if let Some(colour) = unit.timing.and_then(|timing| timing.colour) {
                        (output.video_full_range, output.matrix_coefficients) =
                            colour.resolve(output.video_full_range, output.matrix_coefficients);
                    }
                    if retired == 0 {
                        crate::log_info!(target: "intel-media";
                            "intel/hw_vid: colour-selected matrix_coefficients={} full_range={} source={} conversion={}\n",
                            output.matrix_coefficients,
                            output.video_full_range as u8,
                            if unit.timing.and_then(|timing| timing.colour).is_some() { "mp4-colr+avc-vui" } else { "avc-vui" },
                            if output.matrix_coefficients == 1 { "bt709" } else { "bt601" },
                        );
                    }
                    retired = retired.saturating_add(1);
                    if crate::intel::hw_pic::hold_h264_output_surface(&output) {
                        H264PresentationSlot::Ready(H264PendingPresentation {
                            output,
                            playback_frame: attempted,
                            stream_idr_index: idr_seen,
                            timing: unit.timing,
                        })
                    } else {
                        if first_failure_frame == 0 {
                            first_failure_frame = attempted;
                            first_failure_error = H264_UI4_PRESENT_ERROR;
                        }
                        H264PresentationSlot::Skipped(unit.timing)
                    }
                }
                Err(error) => {
                    if first_failure_frame == 0 {
                        first_failure_frame = attempted;
                        first_failure_error = error;
                    }
                    H264PresentationSlot::Skipped(unit.timing)
                }
            };
            playback_timing.record_decode_ticks(
                EmbassyInstant::now()
                    .saturating_duration_since(decode_start)
                    .as_ticks(),
            );
            slot
        };

        if !presentation_reordering_required {
            if let Some((failed_frame, error)) = h264_present_slot(
                session,
                presentation_slot,
                playback_start,
                base_pts,
                &mut next_frame_deadline,
                frame_period,
                &mut playback_timing,
            )
            .await
            {
                if first_failure_frame == 0 {
                    first_failure_frame = failed_frame;
                    first_failure_error = error;
                }
            }
            continue;
        }

        presentation_slots[rank] = presentation_slot;
        while next_presentation_rank < presentation_slots.len() {
            let slot = presentation_slots[next_presentation_rank];
            if matches!(slot, H264PresentationSlot::Waiting) {
                break;
            }
            if let Some((failed_frame, error)) = h264_present_slot(
                session,
                slot,
                playback_start,
                base_pts,
                &mut next_frame_deadline,
                frame_period,
                &mut playback_timing,
            )
            .await
            {
                if first_failure_frame == 0 {
                    first_failure_frame = failed_frame;
                    first_failure_error = error;
                }
            }
            presentation_slots[next_presentation_rank] = H264PresentationSlot::Waiting;
            next_presentation_rank = next_presentation_rank.saturating_add(1);
        }
    }

    // B-frame reordering can retain future pictures which never reached RCS.
    // Return those pins before draining requests already owned by the worker.
    for slot in presentation_slots {
        if let H264PresentationSlot::Ready(pending) = slot {
            crate::intel::hw_pic::release_h264_output_surface(&pending.output);
        }
    }
    let conversion_report = crate::ui4::wait_decoded_nv12_conversion_idle(session).await;
    let presented = conversion_report.published;
    if first_failure_frame == 0 && conversion_report.first_failure_frame != 0 {
        first_failure_frame = conversion_report.first_failure_frame;
        first_failure_error = conversion_report.first_failure_error;
    }
    crate::log_info!(target: "intel-media";
        "intel/hw_vid: conversion-drain generation={} queued={} completed={} published={} first_failure_frame={} first_failure_error={} handoff_wait_events={} rgba_buffer_wait_events={} rcs_submit_wait_events={} max_outstanding={} avg_conversion_us={} max_conversion_us={} worker=independent-guc-rcs rgba_buffers={} rgba_ownership=producer-write+broker-pending+display-live ordering=preserved drop=0\n",
        conversion_report.generation,
        conversion_report.queued,
        conversion_report.completed,
        conversion_report.published,
        conversion_report.first_failure_frame,
        conversion_report.first_failure_error,
        conversion_report.backpressure_events,
        conversion_report.rgba_buffer_wait_events,
        conversion_report.rcs_submit_wait_events,
        conversion_report.max_outstanding,
        conversion_report.avg_conversion_us(),
        conversion_report.max_conversion_us,
        crate::ui4::VIDEO_RGBA_BUFFER_COUNT,
    );
    let conversion_probe = conversion_report.probe;
    crate::log_info!(target: "intel-media";
        "intel/hw_vid: conversion-probe generation={} samples={} rcs_samples={} worker_avg_us=queue_wait:{},bind_layout:{},rgba_acquire:{},surface_prepare:{},rcs_queue:{},rcs_completion:{},publish:{},end_to_end:{} worker_max_us=queue_wait:{},bind_layout:{},rgba_acquire:{},surface_prepare:{},rcs_queue:{},rcs_completion:{},publish:{},end_to_end:{} worker_percentile_us=end_to_end_p50:{},p95:{},p99:{} rcs_avg_us=queue_prepare:{},queue_total:{},forcewake:{},state_map:{},ppgtt_init:{},kernel_map:{},source_map:{},destination_map:{},batch_encode:{},admission:{},submit_to_marker:{} rcs_max_us=queue_prepare:{},submit_to_marker:{} rcs_percentile_us=queue_prepare_p50:{},p95:{},p99:{},submit_to_marker_p50:{},p95:{},p99:{} gpu_walker=samples:{},avg_us:{},max_us:{},p50_us:{},p95_us:{},p99_us:{},timestamp_hz:{} gpu_phase=samples:{},avg_us=pre_submit_to_batch:{},batch_to_walker:{},walker_to_release:{},release_to_observe:{},pre_submit_to_observe:{},max_us=pre_submit_to_batch:{},batch_to_walker:{},walker_to_release:{},release_to_observe:{},p50_us=pre_submit_to_batch:{},batch_to_walker:{},walker_to_release:{},release_to_observe:{},p95_us=pre_submit_to_batch:{},batch_to_walker:{},walker_to_release:{},release_to_observe:{} guc_h2g_split=samples:{},avg_us=pre_submit_to_consumed_observe:{},consumed_observe_to_batch:{},max_us=pre_submit_to_consumed_observe:{},consumed_observe_to_batch:{},p50_us=pre_submit_to_consumed_observe:{},consumed_observe_to_batch:{},p95_us=pre_submit_to_consumed_observe:{},consumed_observe_to_batch:{} completion_polls_avg={} completion_polls_max={} quantile_bucket_us={} clock=embassy-us+gpu-pipe-control marker_metric=h2g-published-to-host-observed-post-marker gpu_metric=ordered-pre-walker-to-post-media-state-flush gpu_phase_metric=host-pre-submit-to-h2g-consumed-observe+consumed-observe-to-batch-entry+batch-entry-to-pre-walker+post-walker-to-post-release+post-release-to-host-observe h2g_split_bounds=consume-upper+dispatch-lower scope=live-ordered-path\n",
        conversion_report.generation,
        conversion_probe.samples,
        conversion_probe.rcs_samples,
        conversion_probe.avg_queue_wait_us,
        conversion_probe.avg_bind_layout_us,
        conversion_probe.avg_rgba_acquire_us,
        conversion_probe.avg_surface_prepare_us,
        conversion_probe.avg_rcs_queue_us,
        conversion_probe.avg_rcs_completion_us,
        conversion_probe.avg_publish_us,
        conversion_probe.avg_end_to_end_us,
        conversion_probe.max_queue_wait_us,
        conversion_probe.max_bind_layout_us,
        conversion_probe.max_rgba_acquire_us,
        conversion_probe.max_surface_prepare_us,
        conversion_probe.max_rcs_queue_us,
        conversion_probe.max_rcs_completion_us,
        conversion_probe.max_publish_us,
        conversion_probe.max_end_to_end_us,
        conversion_probe.p50_end_to_end_us,
        conversion_probe.p95_end_to_end_us,
        conversion_probe.p99_end_to_end_us,
        conversion_probe.avg_rcs_queue_prepare_us,
        conversion_probe.avg_rcs_queue_total_us,
        conversion_probe.avg_rcs_forcewake_us,
        conversion_probe.avg_rcs_state_map_us,
        conversion_probe.avg_rcs_ppgtt_init_us,
        conversion_probe.avg_rcs_kernel_map_us,
        conversion_probe.avg_rcs_source_map_us,
        conversion_probe.avg_rcs_destination_map_us,
        conversion_probe.avg_rcs_batch_encode_us,
        conversion_probe.avg_rcs_admission_us,
        conversion_probe.avg_rcs_submit_to_marker_us,
        conversion_probe.max_rcs_queue_prepare_us,
        conversion_probe.max_rcs_submit_to_marker_us,
        conversion_probe.p50_rcs_queue_prepare_us,
        conversion_probe.p95_rcs_queue_prepare_us,
        conversion_probe.p99_rcs_queue_prepare_us,
        conversion_probe.p50_rcs_submit_to_marker_us,
        conversion_probe.p95_rcs_submit_to_marker_us,
        conversion_probe.p99_rcs_submit_to_marker_us,
        conversion_probe.gpu_timestamp_samples,
        conversion_probe.avg_gpu_walker_us,
        conversion_probe.max_gpu_walker_us,
        conversion_probe.p50_gpu_walker_us,
        conversion_probe.p95_gpu_walker_us,
        conversion_probe.p99_gpu_walker_us,
        conversion_probe.gpu_timestamp_frequency_hz,
        conversion_probe.gpu_phase_samples,
        conversion_probe.avg_gpu_pre_submit_to_batch_us,
        conversion_probe.avg_gpu_batch_to_walker_us,
        conversion_probe.avg_gpu_walker_to_release_us,
        conversion_probe.avg_gpu_release_to_observe_us,
        conversion_probe.avg_gpu_pre_submit_to_observe_us,
        conversion_probe.max_gpu_pre_submit_to_batch_us,
        conversion_probe.max_gpu_batch_to_walker_us,
        conversion_probe.max_gpu_walker_to_release_us,
        conversion_probe.max_gpu_release_to_observe_us,
        conversion_probe.p50_gpu_pre_submit_to_batch_us,
        conversion_probe.p50_gpu_batch_to_walker_us,
        conversion_probe.p50_gpu_walker_to_release_us,
        conversion_probe.p50_gpu_release_to_observe_us,
        conversion_probe.p95_gpu_pre_submit_to_batch_us,
        conversion_probe.p95_gpu_batch_to_walker_us,
        conversion_probe.p95_gpu_walker_to_release_us,
        conversion_probe.p95_gpu_release_to_observe_us,
        conversion_probe.gpu_h2g_split_samples,
        conversion_probe.avg_gpu_pre_submit_to_h2g_consumed_us,
        conversion_probe.avg_gpu_h2g_consumed_to_batch_us,
        conversion_probe.max_gpu_pre_submit_to_h2g_consumed_us,
        conversion_probe.max_gpu_h2g_consumed_to_batch_us,
        conversion_probe.p50_gpu_pre_submit_to_h2g_consumed_us,
        conversion_probe.p50_gpu_h2g_consumed_to_batch_us,
        conversion_probe.p95_gpu_pre_submit_to_h2g_consumed_us,
        conversion_probe.p95_gpu_h2g_consumed_to_batch_us,
        conversion_probe.avg_completion_polls,
        conversion_probe.max_completion_polls,
        conversion_probe.quantile_bucket_us,
    );
    h264_log_keyframe_summary(indexed_frames.as_slice(), stream_bytes);
    let playback_report = playback_timing.report(
        mode,
        attempted,
        retired,
        presented,
        first_failure_frame,
        first_failure_error,
        skipped_unsupported_frames,
        playback_start,
        conversion_report,
    );

    crate::log!(
        "intel/hw_vid: h264-playback done nals={} idr_seen={} p_seen={} b_seen={} attempted={} retired={} presented={} first_failure_frame={} first_failure_error={} skipped_unsupported={} indexed_frames={} missing_headers={} stopped_at=0x{:X} target_fps={} target_frame_ms={} elapsed_ms={} effective_fps_x100={} waited_frames={} late_frames={} total_wait_ms={} avg_decode_us={} max_decode_us={} max_late_ms={} avg_queue_us={} avg_process_us={} mode_transitions={} engine_resets={} avg_reset_us={} avg_zero_clear_us={} avg_zero_us={} avg_scratch_zero_us={} avg_output_clear_us={} avg_missing_clear_us={} avg_scratch_flush_us={} avg_build_ctx_us={} avg_poll_us={} max_poll_us={} avg_post_us={} avg_handoff_us={} max_handoff_us={} conversion_queued={} conversion_completed={} conversion_handoff_wait_events={} conversion_rgba_buffer_wait_events={} conversion_rcs_submit_wait_events={} conversion_max_outstanding={} avg_conversion_us={} max_conversion_us={} avg_poll_iters={} reason={}\n",
        nal_count,
        idr_seen,
        p_seen,
        b_seen,
        playback_report.attempted,
        playback_report.retired,
        playback_report.presented,
        playback_report.first_failure_frame,
        playback_report.first_failure_error,
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
        playback_report.mode_transitions,
        playback_report.engine_resets,
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
        playback_report.conversion_queued,
        playback_report.conversion_completed,
        playback_report.conversion_backpressure_events,
        playback_report.conversion_rgba_buffer_wait_events,
        playback_report.conversion_rcs_submit_wait_events,
        playback_report.conversion_max_outstanding,
        playback_report.avg_conversion_us,
        playback_report.max_conversion_us,
        playback_report.avg_poll_iters,
        if session.is_cancelled() {
            "cancelled"
        } else {
            "eos"
        }
    );
    playback_report
}

#[derive(Copy, Clone)]
struct H264PendingPresentation {
    output: super::hw_pic::HwPicOutput,
    playback_frame: usize,
    stream_idr_index: usize,
    timing: Option<H264SampleTiming>,
}

#[derive(Copy, Clone)]
enum H264PresentationSlot {
    Waiting,
    Skipped(Option<H264SampleTiming>),
    Ready(H264PendingPresentation),
}

async fn h264_present_slot(
    session: crate::ui4::VideoPlaybackSession,
    slot: H264PresentationSlot,
    playback_start: EmbassyInstant,
    base_pts: i64,
    next_fixed_deadline: &mut EmbassyInstant,
    frame_period: EmbassyDuration,
    playback_timing: &mut H264PlaybackTiming,
) -> Option<(usize, i32)> {
    if !session.wait_until_playing().await {
        if let H264PresentationSlot::Ready(pending) = slot {
            crate::intel::hw_pic::release_h264_output_surface(&pending.output);
        }
        return None;
    }
    let timing = match slot {
        H264PresentationSlot::Waiting => return None,
        H264PresentationSlot::Skipped(timing) => timing,
        H264PresentationSlot::Ready(pending) => pending.timing,
    };
    h264_wait_for_presentation_time(
        session,
        playback_start,
        timing,
        base_pts,
        next_fixed_deadline,
        frame_period,
        playback_timing,
    )
    .await;
    let H264PresentationSlot::Ready(pending) = slot else {
        return None;
    };
    if session.is_cancelled() {
        crate::intel::hw_pic::release_h264_output_surface(&pending.output);
        return None;
    }
    let present_start = EmbassyInstant::now();
    let queued = h264_queue_probe_output(
        session,
        "pts",
        pending.playback_frame,
        pending.stream_idr_index,
        &pending.output,
    )
    .await;
    playback_timing.record_present_ticks(
        EmbassyInstant::now()
            .saturating_duration_since(present_start)
            .as_ticks(),
    );
    if queued {
        // The conversion worker now owns the decoder-surface pin and releases
        // it only after its RCS source read has retired.
        None
    } else {
        crate::intel::hw_pic::release_h264_output_surface(&pending.output);
        if session.is_cancelled() {
            return None;
        }
        Some((
            pending.playback_frame,
            if pending.output.error_code != 0 {
                pending.output.error_code
            } else {
                H264_UI4_PRESENT_ERROR
            },
        ))
    }
}

async fn h264_wait_for_presentation_time(
    session: crate::ui4::VideoPlaybackSession,
    playback_start: EmbassyInstant,
    timing: Option<H264SampleTiming>,
    base_pts: i64,
    next_fixed_deadline: &mut EmbassyInstant,
    frame_period: EmbassyDuration,
    playback_timing: &mut H264PlaybackTiming,
) {
    let Some(timing) = timing else {
        h264_wait_until_next_frame(session, next_fixed_deadline, frame_period, playback_timing)
            .await;
        return;
    };
    let pts_from_start = timing.pts.saturating_sub(base_pts).max(0) as u64;
    let target_ticks = (u128::from(pts_from_start))
        .saturating_mul(u128::from(embassy_time_driver::TICK_HZ))
        / u128::from(timing.timescale.max(1));
    let deadline = playback_start + EmbassyDuration::from_ticks(target_ticks as u64);
    let now = EmbassyInstant::now();
    if now < deadline {
        let wait_start = now.as_ticks();
        h264_wait_until_or_cancelled(session, deadline).await;
        playback_timing.waited_frames = playback_timing.waited_frames.saturating_add(1);
        playback_timing.total_wait_ticks = playback_timing
            .total_wait_ticks
            .saturating_add(EmbassyInstant::now().as_ticks().saturating_sub(wait_start));
    } else {
        playback_timing.late_frames = playback_timing.late_frames.saturating_add(1);
        playback_timing.max_late_ticks = playback_timing
            .max_late_ticks
            .max(now.saturating_duration_since(deadline).as_ticks());
    }
}

async fn h264_decode_wait_frame(
    phase: &'static str,
    playback_frame: usize,
    stream_idr_index: usize,
    encoded: &[u8],
    diagnostics: bool,
    media_session_generation: u64,
    mut timing: Option<&mut H264PlaybackTiming>,
) -> Result<super::hw_pic::HwPicOutput, i32> {
    if diagnostics {
        let before = crate::intel::hw_pic_snapshot();
        crate::log!(
            "intel/hw_vid: h264-frame submit phase={} playback_frame={} stream_idr={} bytes={} destination=ui4-rgba-stream rgba_buffers={} pending={} outputs={} service_started={}\n",
            phase,
            playback_frame,
            stream_idr_index,
            encoded.len(),
            crate::ui4::VIDEO_RGBA_BUFFER_COUNT,
            before.pending,
            before.outputs,
            before.service_started as u8
        );
    }

    let id = match crate::intel::hw_pic_submit_h264_in_media_session(
        encoded,
        media_session_generation,
    ) {
        Ok(id) => id,
        Err(err) => {
            crate::log!(
                "intel/hw_vid: h264-probe submit-failed phase={} playback_frame={} stream_idr={} err={}\n",
                phase,
                playback_frame,
                stream_idr_index,
                err
            );
            return Err(err);
        }
    };

    // Once accepted, this job owns DMA/reference memory. Even cancellation
    // cannot release its reservation before the service has retired the job.
    let mut timeout_logged = false;
    let output = loop {
        if let Some(output) =
            crate::intel::hw_pic_wait_output_for_id(id, H264_DECODE_TIMEOUT_MS).await
        {
            break output;
        }
        if !timeout_logged {
            timeout_logged = true;
            crate::log_error!(target: "intel-media";
                "intel/hw_vid: decode delayed id={} generation={} action=retain-session-until-service-completion\n",
                id, media_session_generation);
        }
    };

    if let Some(timing) = timing.as_deref_mut() {
        timing.record_hw_pic_timing(output.timing);
    }
    if diagnostics {
        crate::log!(
            "intel/hw_vid: h264-frame output phase={} playback_frame={} stream_idr={} id={} codec={:?} status={:?} fmt={:?} decoded={}x{} visible={}x{} pitch=0x{:X} uv=0x{:X} bytes=0x{:X} gpu=0x{:X} phys=0x{:X} surface_slot={} stored=deferred-pts destination=ui4-rgba-stream rgba_buffers={} err={}\n",
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
            output.surface_slot,
            crate::ui4::VIDEO_RGBA_BUFFER_COUNT,
            output.error_code
        );
    }
    if matches!(
        output.status,
        super::hw_pic::HwPicStatus::Ready | super::hw_pic::HwPicStatus::Streamed
    ) && output.error_code == 0
    {
        Ok(output)
    } else if output.error_code != 0 {
        Err(output.error_code)
    } else {
        Err(H264_UI4_PRESENT_ERROR)
    }
}

async fn h264_queue_probe_output(
    session: crate::ui4::VideoPlaybackSession,
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
        let source = crate::ui4::DecodedNv12Source {
            decode_sequence: u64::from(output.id),
            decoder_surface_slot: output.surface_slot,
            decoder_session_slot: output.decoder_session_slot,
            gpu: output.gpu_addr,
            phys: output.phys_addr,
            virt: output.virt_addr,
            byte_len: output.byte_len,
            width: output.width,
            height: output.height,
            visible_width: output.visible_width,
            visible_height: output.visible_height,
            video_full_range: output.video_full_range,
            matrix_coefficients: output.matrix_coefficients,
            pitch_bytes: output.pitch_bytes,
            uv_offset: output.uv_offset,
        };
        if !H264_UI4_HANDOFF_CHECKPOINT_LOGGED.swap(true, Ordering::AcqRel) {
            crate::log!(
                "intel/hw_vid: checkpoint stage=decode-retired-ui4-handoff phase={} playback_frame={} id={} gpu=0x{:X} phys=0x{:X} bytes=0x{:X} decoded={}x{} visible={}x{} action=enqueue-independent-rcs-worker\n",
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
        let ui4_queued =
            crate::ui4::enqueue_decoded_nv12_stream_frame(session, source, playback_frame).await;
        if ui4_queued {
            return true;
        }
        crate::log!(
            "intel/hw_vid: h264-present ui4 failed phase={} playback_frame={} stream_idr={} id={} action=reject-handoff drop=0 fallback=none\n",
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

/// Encoded Blueprint ingress. Shares MP4/Annex-B parsing, PTS ordering, decoder
/// reservation, conversion workers and cancellation with shell playback.
pub(crate) async fn run_memory_texture_video_playback(
    session: crate::ui4::VideoPlaybackSession,
    asset: Vec<u8>,
) -> Result<H264PlaybackReport, &'static str> {
    if !crate::intel::has_media_decode_engine() {
        return Err("media decode engine unavailable");
    }
    let (annexb, timing, _, _) = h264_prepare_trueosfs_asset(asset)?;
    let media_session = h264_reserve_decode_session(session).await?;
    let report = h264_i_p_playback_probe_annexb_bytes(
        session,
        annexb,
        timing,
        "blueprint-mp4-avc",
        "retained-video",
        H264PlaybackOptions::new(UI4_FRAMED_VIDEO_FPS, false, true),
        media_session.generation(),
    )
    .await;
    if report.first_failure_frame != 0 || report.presented == 0 || report.skipped_unsupported != 0 {
        Err("video decode or texture publication failed")
    } else {
        Ok(report)
    }
}
