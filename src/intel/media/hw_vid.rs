use alloc::{format, string::String, string::ToString, vec::Vec};
use core::fmt::Write;
use embassy_time::{Duration as EmbassyDuration, Instant as EmbassyInstant, Timer};
use serde_json::Value;

const H264_BOOT_PROBE_ENABLED: bool = true;
const H264_BOOT_PROBE_PLAYBACK_ENABLED: bool = false;
const H264_BOOT_PROBE_PLAYBACK_OPTIONS: H264PlaybackOptions = H264PlaybackOptions {
    fps: 30,
    reverse_after_forward: false,
    cache_mode: H264PlaybackCacheMode::Off,
    stripe_study: false,
    show_cache_fill: false,
    diagnostics: true,
    noreset_lite: false,
    loop_playback: false,
};
const H264_BOOT_PROBE_STRIPE_STUDY_FRAME_MS: u64 = 120;
const H264_BOOT_PROBE_STRIPE_STUDY_STORE_TOP: usize = 8;
const H264_BOOT_PROBE_STREAM_LOAD_TIMEOUT_MS: u64 = 20_000;
const H264_BOOT_PROBE_STREAM_LOAD_POLL_MS: u64 = 250;
const H264_BOOT_PROBE_STREAM_CHUNK_BYTES: usize = 64 * 1024;
const H264_BOOT_PROBE_TIMEOUT_MS: u64 = 5_000;
const H264_BOOT_PROBE_DELAY_MS: u64 = 2_000;
const H264_BROWSER_MEDIA_FETCH_TIMEOUT_MS: u64 = 120_000;
const H264_BROWSER_MEDIA_FETCH_MAX_BYTES: usize = 160 * 1024 * 1024;
const H264_BROWSER_MEDIA_CANDIDATE_WAIT_MS: u64 = 60_000;
const H264_BROWSER_INNERTUBE_TIMEOUT_MS: u64 = 45_000;
const H264_BROWSER_INNERTUBE_MAX_BYTES: usize = 4 * 1024 * 1024;
const H264_BROWSER_SABR_PROBE_TIMEOUT_MS: u64 = 30_000;
const H264_BROWSER_SABR_PROBE_BYTES: usize = 64 * 1024;
const H264_MEDIA_URL_BOOT_PROBE_ENABLED: bool = true;
const H264_MEDIA_URL_BOOT_PROBE_DELAY_MS: u64 = 6_000;
const H264_MEDIA_URL_BOOT_PROBE_URL: &str =
    "https://docs.evostream.com/sample_content/assets/bun33s.mp4";
const H264_MEDIA_URL_BOOT_PROBE_OPTIONS: H264PlaybackOptions = H264PlaybackOptions {
    fps: 25,
    reverse_after_forward: false,
    cache_mode: H264PlaybackCacheMode::Off,
    stripe_study: false,
    show_cache_fill: false,
    diagnostics: false,
    noreset_lite: true,
    loop_playback: false,
};
pub(crate) const H264_BOOT_PROBE_STREAM_PATH: &str = "x31_head_movie.annexb.h264";

#[derive(Copy, Clone, Debug)]
pub(crate) enum H264PlaybackCacheMode {
    Off,
    Tail,
    Full,
}

impl H264PlaybackCacheMode {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Tail => "tail",
            Self::Full => "full",
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct H264PlaybackOptions {
    fps: u16,
    reverse_after_forward: bool,
    cache_mode: H264PlaybackCacheMode,
    stripe_study: bool,
    show_cache_fill: bool,
    diagnostics: bool,
    noreset_lite: bool,
    loop_playback: bool,
}

impl H264PlaybackOptions {
    pub(crate) const fn new(
        fps: u16,
        reverse_after_forward: bool,
        cache_mode: H264PlaybackCacheMode,
        stripe_study: bool,
        show_cache_fill: bool,
        diagnostics: bool,
        noreset_lite: bool,
        loop_playback: bool,
    ) -> Self {
        Self {
            fps,
            reverse_after_forward,
            cache_mode,
            stripe_study,
            show_cache_fill,
            diagnostics,
            noreset_lite,
            loop_playback,
        }
    }

    pub(crate) const fn fps(self) -> u16 {
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

    pub(crate) const fn reverse_after_forward(self) -> bool {
        self.reverse_after_forward
    }

    pub(crate) const fn name(self) -> &'static str {
        if self.reverse_after_forward {
            "forward-then-reverse"
        } else {
            "forward"
        }
    }

    pub(crate) const fn cache_mode(self) -> H264PlaybackCacheMode {
        self.cache_mode
    }

    pub(crate) const fn stripe_study(self) -> bool {
        self.stripe_study
    }

    pub(crate) const fn show_cache_fill(self) -> bool {
        self.show_cache_fill
    }

    pub(crate) const fn diagnostics(self) -> bool {
        self.diagnostics
    }

    pub(crate) const fn noreset_lite(self) -> bool {
        self.noreset_lite
    }

    pub(crate) const fn loop_playback(self) -> bool {
        self.loop_playback
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct H264PlaybackReport {
    pub(crate) target_fps: u16,
    pub(crate) target_frame_ms: u64,
    pub(crate) submitted: usize,
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
            h264_i_p_playback_probe(file, H264_BOOT_PROBE_PLAYBACK_OPTIONS).await;
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

#[embassy_executor::task]
pub(crate) async fn hw_vid_media_url_probe_task() {
    if !H264_MEDIA_URL_BOOT_PROBE_ENABLED {
        crate::log!("intel/hw_vid: media-url-probe disabled\n");
        return;
    }
    if !crate::intel::has_media_decode_engine() {
        crate::log!("intel/hw_vid: media-url-probe skipped reason=no-media-decode-engine\n");
        return;
    }

    Timer::after(EmbassyDuration::from_millis(H264_MEDIA_URL_BOOT_PROBE_DELAY_MS)).await;
    crate::log!(
        "intel/hw_vid: media-url-probe begin fps={} url={}\n",
        H264_MEDIA_URL_BOOT_PROBE_OPTIONS.fps(),
        H264_MEDIA_URL_BOOT_PROBE_URL
    );
    match run_media_url_playback(
        H264_MEDIA_URL_BOOT_PROBE_URL,
        H264_MEDIA_URL_BOOT_PROBE_OPTIONS,
        "media-url-probe",
        "media-url-probe",
    )
    .await
    {
        Ok(report) => crate::log!(
            "intel/hw_vid: media-url-probe done submitted={} target_fps={} elapsed_ms={} effective_fps={}.{:02} waited={} late={} wait_ms={} avg_decode_us={} max_decode_us={} avg_process_us={} avg_present_us={} url={}\n",
            report.submitted,
            report.target_fps,
            report.elapsed_ms,
            report.effective_fps_x100 / 100,
            report.effective_fps_x100 % 100,
            report.waited_frames,
            report.late_frames,
            report.total_wait_ms,
            report.avg_decode_us,
            report.max_decode_us,
            report.avg_process_us,
            report.avg_present_us,
            H264_MEDIA_URL_BOOT_PROBE_URL
        ),
        Err(err) => crate::log!(
            "intel/hw_vid: media-url-probe failed err={} url={}\n",
            err,
            H264_MEDIA_URL_BOOT_PROBE_URL
        ),
    }
}

pub(crate) async fn run_shell_vid_playback(
    options: H264PlaybackOptions,
) -> Result<H264PlaybackReport, &'static str> {
    if !crate::intel::has_media_decode_engine() {
        return Err("media decode engine unavailable");
    }
    let Some(file) = h264_open_playback_stream_once().await else {
        return Err("video asset missing from TRUEOSFS root");
    };
    let old_hw_pic_logging =
        crate::intel::hw_pic::set_detailed_logging_enabled(options.diagnostics());
    let old_surface_probes =
        crate::intel::xelp_media2_ngin::set_output_surface_probes_enabled(options.diagnostics());
    let old_noreset_lite =
        crate::intel::xelp_media2_ngin_hw_pic::set_avc_noreset_lite_enabled(options.noreset_lite());
    let report = h264_i_p_playback_probe(file, options).await;
    crate::intel::xelp_media2_ngin_hw_pic::set_avc_noreset_lite_enabled(old_noreset_lite);
    crate::intel::hw_pic::set_detailed_logging_enabled(old_hw_pic_logging);
    crate::intel::xelp_media2_ngin::set_output_surface_probes_enabled(old_surface_probes);
    Ok(report)
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

pub(crate) async fn run_browser_media_playback(
    options: H264PlaybackOptions,
) -> Result<H264PlaybackReport, &'static str> {
    if !crate::intel::has_media_decode_engine() {
        return Err("media decode engine unavailable");
    }
    let queued_before = crate::surfer::media_stream::candidate_count();
    crate::log!(
        "intel/hw_vid: browser-media candidate-wait begin queued={} timeout_ms={}\n",
        queued_before,
        H264_BROWSER_MEDIA_CANDIDATE_WAIT_MS
    );
    let Some(candidate) =
        crate::surfer::media_stream::wait_latest_candidate(H264_BROWSER_MEDIA_CANDIDATE_WAIT_MS)
            .await
    else {
        crate::log!(
            "intel/hw_vid: browser-media candidate-wait timeout queued={} action=open-youtube-in-surf-before-vid\n",
            crate::surfer::media_stream::candidate_count()
        );
        return Err("no browser media candidate queued");
    };
    crate::log!(
        "intel/hw_vid: browser-media start browser={} generation={} tag={} kind={} fetch_timeout_ms={} url={}\n",
        candidate.browser_instance_id,
        candidate.generation,
        candidate.tag,
        candidate.kind,
        H264_BROWSER_MEDIA_FETCH_TIMEOUT_MS,
        candidate.url
    );
    let media_url = if h264_browser_media_is_innertube(
        candidate.kind.as_str(),
        candidate.url.as_str(),
    ) {
        match h264_resolve_youtube_innertube_candidate(candidate.url.as_str()).await {
            Ok(url) => url,
            Err(err) => {
                if let Some(sabr) = crate::surfer::media_stream::sabr_candidate() {
                    h264_probe_sabr_candidate(&sabr).await;
                }
                return Err(err);
            }
        }
    } else if h264_browser_media_is_sabr(candidate.kind.as_str(), candidate.url.as_str()) {
        h264_probe_sabr_candidate(&candidate).await;
        crate::log!(
            "intel/hw_vid: browser-media unsupported kind=sabr action=needs-sabr-demux-or-innertube-direct-format browser={} generation={} tag={} url={}\n",
            candidate.browser_instance_id,
            candidate.generation,
            candidate.tag,
            candidate.url
        );
        return Err("browser media SABR unsupported");
    } else {
        candidate.url.clone()
    };

    let mp4_bytes = match h264_fetch_browser_media_candidate(media_url.as_str()).await {
        Ok(bytes) => bytes,
        Err(err) => {
            if let Some(sabr) = crate::surfer::media_stream::sabr_candidate() {
                h264_probe_sabr_candidate(&sabr).await;
            }
            return Err(err);
        }
    };
    let annexb = mp4_avc1_to_annexb(mp4_bytes.as_slice())?;
    crate::log!(
        "intel/hw_vid: browser-media demux accepted=1 container=mp4 codec=avc1 mp4_bytes={} annexb_bytes={} source=browser url={}\n",
        mp4_bytes.len(),
        annexb.len(),
        media_url
    );

    let old_hw_pic_logging =
        crate::intel::hw_pic::set_detailed_logging_enabled(options.diagnostics());
    let old_surface_probes =
        crate::intel::xelp_media2_ngin::set_output_surface_probes_enabled(options.diagnostics());
    let old_noreset_lite =
        crate::intel::xelp_media2_ngin_hw_pic::set_avc_noreset_lite_enabled(options.noreset_lite());
    let report =
        h264_i_p_playback_probe_annexb_bytes(annexb, "browser-mp4-avc1", "browser-media", options)
            .await;
    crate::intel::xelp_media2_ngin_hw_pic::set_avc_noreset_lite_enabled(old_noreset_lite);
    crate::intel::hw_pic::set_detailed_logging_enabled(old_hw_pic_logging);
    crate::intel::xelp_media2_ngin::set_output_surface_probes_enabled(old_surface_probes);
    Ok(report)
}

fn h264_browser_media_is_sabr(kind: &str, url: &str) -> bool {
    let kind = kind.to_ascii_lowercase();
    let url = url.to_ascii_lowercase();
    kind.contains("sabr") || url.contains("sabr=1") || url.contains("sabr%3d1")
}

fn h264_browser_media_is_innertube(kind: &str, url: &str) -> bool {
    kind.to_ascii_lowercase().contains("youtube-innertube")
        || url.to_ascii_lowercase().starts_with("innertube://player?")
}

async fn h264_probe_sabr_candidate(candidate: &crate::surfer::media_stream::BrowserMediaCandidate) {
    let range_end = H264_BROWSER_SABR_PROBE_BYTES.saturating_sub(1);
    for profile in [
        "browser-range",
        "plain-range",
        "youtube-range",
        "sabr-range",
        "plain-norange",
        "youtube-norange",
        "sabr-norange",
    ] {
        let started = EmbassyInstant::now();
        crate::log!(
            "intel/hw_vid: browser-media sabr-probe begin profile={} browser={} generation={} tag={} timeout_ms={} range=0-{} url={}\n",
            profile,
            candidate.browser_instance_id,
            candidate.generation,
            candidate.tag,
            H264_BROWSER_SABR_PROBE_TIMEOUT_MS,
            range_end,
            candidate.url
        );
        match crate::r::net::https::get_browser_media_probe_bytes_shared(
            candidate.url.as_str(),
            range_end,
            profile,
            H264_BROWSER_SABR_PROBE_TIMEOUT_MS as u32,
            H264_BROWSER_SABR_PROBE_BYTES,
        )
        .await
        {
            Ok(bytes) => {
                let head = hex_prefix(bytes.as_slice(), 32);
                crate::log!(
                    "intel/hw_vid: browser-media sabr-probe done profile={} bytes={} waited_ms={} marker_ftyp={} marker_moof={} marker_mdat={} marker_avcc={} marker_start_code={} marker_sabr={} head_hex={}\n",
                    profile,
                    bytes.len(),
                    started.elapsed().as_millis(),
                    bytes_contains(bytes.as_slice(), b"ftyp") as u8,
                    bytes_contains(bytes.as_slice(), b"moof") as u8,
                    bytes_contains(bytes.as_slice(), b"mdat") as u8,
                    bytes_contains(bytes.as_slice(), b"avcC") as u8,
                    has_h264_start_code(bytes.as_slice()) as u8,
                    bytes_contains(bytes.as_slice(), b"sabr") as u8,
                    head
                );
                return;
            }
            Err(err) => {
                crate::log!(
                    "intel/hw_vid: browser-media sabr-probe failed profile={} err={} waited_ms={} url={}\n",
                    profile,
                    err,
                    started.elapsed().as_millis(),
                    candidate.url
                );
            }
        }
    }
    crate::log!(
        "intel/hw_vid: browser-media sabr-probe exhausted profiles=7 action=need-sabr-request-shape-or-demux\n"
    );
}

async fn h264_resolve_youtube_innertube_candidate(url: &str) -> Result<String, &'static str> {
    let probe = YoutubeInnertubeProbe::from_url(url)?;
    let request_url = format!(
        "https://www.youtube.com/youtubei/v1/player?key={}&prettyPrint=false",
        url_query_encode(probe.api_key.as_str())
    );

    let mut deferred_n_url: Option<String> = None;
    for profile in
        youtube_innertube_profiles(probe.client_name.as_str(), probe.client_version.as_str())
    {
        let started = EmbassyInstant::now();
        let visitor = if profile.use_visitor && !probe.visitor_data.is_empty() {
            Some(probe.visitor_data.as_str())
        } else {
            None
        };
        crate::log!(
            "intel/hw_vid: youtube-innertube fetch begin profile={} video_id={} client_name={} client_version={} visitor={} timeout_ms={} max_bytes={}\n",
            profile.label,
            probe.video_id,
            profile.client_name,
            profile.client_version,
            visitor.is_some() as u8,
            H264_BROWSER_INNERTUBE_TIMEOUT_MS,
            H264_BROWSER_INNERTUBE_MAX_BYTES
        );
        let body = youtube_innertube_player_body(&probe, &profile);
        let bytes = match crate::r::net::https::post_youtubei_player_bytes_shared(
            request_url.as_str(),
            body.as_str(),
            youtube_client_header_name(profile.client_name),
            profile.client_version,
            visitor,
            H264_BROWSER_INNERTUBE_TIMEOUT_MS as u32,
            H264_BROWSER_INNERTUBE_MAX_BYTES,
        )
        .await
        {
            Ok(bytes) => bytes,
            Err(err) => {
                crate::log!(
                    "intel/hw_vid: youtube-innertube fetch failed profile={} err={} waited_ms={} video_id={}\n",
                    profile.label,
                    err,
                    started.elapsed().as_millis(),
                    probe.video_id
                );
                continue;
            }
        };
        crate::log!(
            "intel/hw_vid: youtube-innertube fetch done profile={} bytes={} waited_ms={} video_id={}\n",
            profile.label,
            bytes.len(),
            started.elapsed().as_millis(),
            probe.video_id
        );
        match h264_pick_youtube_innertube_direct_h264(
            bytes.as_slice(),
            probe.video_id.as_str(),
            profile.label,
        ) {
            Ok(url) => {
                if youtube_url_has_query_field(url.as_str(), "n") {
                    crate::log!(
                        "intel/hw_vid: youtube-innertube direct-h264 deferred profile={} reason=n-param-needs-transform video_id={} url={}\n",
                        profile.label,
                        probe.video_id,
                        url
                    );
                    deferred_n_url.get_or_insert(url);
                    continue;
                }
                crate::log!(
                    "intel/hw_vid: youtube-innertube direct-h264 selected profile={} n_param=0 video_id={} url={}\n",
                    profile.label,
                    probe.video_id,
                    url
                );
                return Ok(url);
            }
            Err(err) => {
                crate::log!(
                    "intel/hw_vid: youtube-innertube profile miss profile={} err={} video_id={}\n",
                    profile.label,
                    err,
                    probe.video_id
                );
            }
        }
    }
    if let Some(url) = deferred_n_url {
        crate::log!(
            "intel/hw_vid: youtube-innertube direct-h264 selected profile=deferred-web n_param=1 action=fetch-to-confirm-or-need-n-transform video_id={} url={}\n",
            probe.video_id,
            url
        );
        return Ok(url);
    }
    Err("browser media innertube no direct h264")
}

#[derive(Debug)]
struct YoutubeInnertubeProbe {
    video_id: String,
    api_key: String,
    client_name: String,
    client_version: String,
    visitor_data: String,
    hl: String,
    gl: String,
    watch_url: String,
    signature_timestamp: String,
}

impl YoutubeInnertubeProbe {
    fn from_url(url: &str) -> Result<Self, &'static str> {
        let Some((_, query)) = url.split_once('?') else {
            return Err("browser media innertube bad probe");
        };
        let video_id =
            query_field(query, "video_id").ok_or("browser media innertube no video id")?;
        let api_key = query_field(query, "api_key").ok_or("browser media innertube no api key")?;
        let client_name = query_field(query, "client_name").unwrap_or_else(|| String::from("WEB"));
        let client_version = query_field(query, "client_version")
            .ok_or("browser media innertube no client version")?;
        let visitor_data = query_field(query, "visitor_data").unwrap_or_default();
        let hl = query_field(query, "hl").unwrap_or_else(|| String::from("en"));
        let gl = query_field(query, "gl").unwrap_or_else(|| String::from("US"));
        let watch_url = query_field(query, "watch_url")
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| format!("https://www.youtube.com/watch?v={}", video_id));
        let signature_timestamp = query_field(query, "sts").unwrap_or_default();
        if video_id.is_empty() || api_key.is_empty() || client_version.is_empty() {
            return Err("browser media innertube bad probe");
        }
        Ok(Self {
            video_id,
            api_key,
            client_name,
            client_version,
            visitor_data,
            hl,
            gl,
            watch_url,
            signature_timestamp,
        })
    }
}

#[derive(Clone, Copy, Debug)]
struct YoutubeInnertubeProfile<'a> {
    label: &'static str,
    client_name: &'a str,
    client_version: &'a str,
    use_visitor: bool,
    rich_web_context: bool,
    playback_context: bool,
    client_extra_json: &'static str,
    context_extra_json: &'static str,
}

fn youtube_innertube_profiles<'a>(
    page_client_name: &'a str,
    page_client_version: &'a str,
) -> [YoutubeInnertubeProfile<'a>; 7] {
    [
        YoutubeInnertubeProfile {
            label: "page-web",
            client_name: page_client_name,
            client_version: page_client_version,
            use_visitor: true,
            rich_web_context: false,
            playback_context: false,
            client_extra_json: "",
            context_extra_json: "",
        },
        YoutubeInnertubeProfile {
            label: "page-web-watch",
            client_name: page_client_name,
            client_version: page_client_version,
            use_visitor: true,
            rich_web_context: true,
            playback_context: true,
            client_extra_json: ",\"clientScreen\":\"WATCH\"",
            context_extra_json: "",
        },
        YoutubeInnertubeProfile {
            label: "page-web-novisitor",
            client_name: page_client_name,
            client_version: page_client_version,
            use_visitor: false,
            rich_web_context: false,
            playback_context: false,
            client_extra_json: "",
            context_extra_json: "",
        },
        YoutubeInnertubeProfile {
            label: "page-web-watch-novisitor",
            client_name: page_client_name,
            client_version: page_client_version,
            use_visitor: false,
            rich_web_context: true,
            playback_context: true,
            client_extra_json: ",\"clientScreen\":\"WATCH\"",
            context_extra_json: "",
        },
        YoutubeInnertubeProfile {
            label: "web-embedded",
            client_name: "WEB_EMBEDDED_PLAYER",
            client_version: page_client_version,
            use_visitor: false,
            rich_web_context: true,
            playback_context: true,
            client_extra_json: "",
            context_extra_json: ",\"thirdParty\":{\"embedUrl\":\"https://www.youtube.com/\"}",
        },
        YoutubeInnertubeProfile {
            label: "android",
            client_name: "ANDROID",
            client_version: "19.09.37",
            use_visitor: false,
            rich_web_context: false,
            playback_context: false,
            client_extra_json: ",\"androidSdkVersion\":30,\"osName\":\"Android\",\"osVersion\":\"11\"",
            context_extra_json: "",
        },
        YoutubeInnertubeProfile {
            label: "ios",
            client_name: "IOS",
            client_version: "19.09.3",
            use_visitor: false,
            rich_web_context: false,
            playback_context: false,
            client_extra_json: ",\"deviceMake\":\"Apple\",\"deviceModel\":\"iPhone16,2\",\"osName\":\"iOS\",\"osVersion\":\"17.5.1.21F90\"",
            context_extra_json: "",
        },
    ]
}

fn youtube_client_header_name(client_name: &str) -> &str {
    match client_name {
        "WEB" => "1",
        "ANDROID" => "3",
        "IOS" => "5",
        "WEB_EMBEDDED_PLAYER" => "56",
        _ => client_name,
    }
}

fn youtube_innertube_player_body(
    probe: &YoutubeInnertubeProbe,
    profile: &YoutubeInnertubeProfile<'_>,
) -> String {
    let rich_client = if profile.rich_web_context {
        format!(
            ",\"originalUrl\":\"{}\",\"platform\":\"DESKTOP\",\"userAgent\":\"{}\"",
            json_escape(probe.watch_url.as_str()),
            json_escape(YOUTUBE_WEB_USER_AGENT)
        )
    } else {
        String::new()
    };
    let request_context = if profile.rich_web_context {
        String::from(",\"request\":{\"useSsl\":true},\"user\":{\"lockedSafetyMode\":false}")
    } else {
        String::new()
    };
    let playback_context = if profile.playback_context {
        let current_url =
            youtube_watch_path_from_url(probe.watch_url.as_str(), probe.video_id.as_str());
        let sts = probe.signature_timestamp.trim();
        let sts_json = if sts.chars().all(|ch| ch.is_ascii_digit()) && !sts.is_empty() {
            format!(",\"signatureTimestamp\":{}", sts)
        } else {
            String::new()
        };
        format!(
            ",\"playbackContext\":{{\"contentPlaybackContext\":{{\"currentUrl\":\"{}\",\"html5Preference\":\"HTML5_PREF_WANTS\",\"lactMilliseconds\":\"-1\"{}}}}}",
            json_escape(current_url.as_str()),
            sts_json
        )
    } else {
        String::new()
    };
    let client = format!(
        "\"client\":{{\"clientName\":\"{}\",\"clientVersion\":\"{}\",\"hl\":\"{}\",\"gl\":\"{}\",\"visitorData\":\"{}\"{}{}}}",
        json_escape(profile.client_name),
        json_escape(profile.client_version),
        json_escape(probe.hl.as_str()),
        json_escape(probe.gl.as_str()),
        if profile.use_visitor {
            json_escape(probe.visitor_data.as_str())
        } else {
            String::new()
        },
        rich_client,
        profile.client_extra_json
    );
    format!(
        "{{\"context\":{{{}{}{}}},\"videoId\":\"{}\"{},\"contentCheckOk\":true,\"racyCheckOk\":true}}",
        client,
        request_context,
        profile.context_extra_json,
        json_escape(probe.video_id.as_str()),
        playback_context
    )
}

const YOUTUBE_WEB_USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

fn youtube_watch_path_from_url(url: &str, video_id: &str) -> String {
    if let Some(path_start) = url.find("youtube.com") {
        let after_host = &url[path_start + "youtube.com".len()..];
        if after_host.starts_with("/watch?") {
            return after_host.to_string();
        }
    }
    format!("/watch?v={}", video_id)
}

fn h264_pick_youtube_innertube_direct_h264(
    bytes: &[u8],
    video_id: &str,
    profile: &str,
) -> Result<String, &'static str> {
    let value = serde_json::from_slice::<Value>(bytes)
        .map_err(|_| "browser media innertube json failed")?;
    let playability_status = value
        .get("playabilityStatus")
        .and_then(|entry| entry.get("status"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let playability_reason = value
        .get("playabilityStatus")
        .and_then(|entry| entry.get("reason"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let streaming = value.get("streamingData");
    let regular = streaming
        .and_then(|entry| entry.get("formats"))
        .and_then(Value::as_array)
        .map(|formats| formats.len())
        .unwrap_or(0);
    let adaptive = streaming
        .and_then(|entry| entry.get("adaptiveFormats"))
        .and_then(Value::as_array)
        .map(|formats| formats.len())
        .unwrap_or(0);
    let server_abr = streaming
        .and_then(|entry| entry.get("serverAbrStreamingUrl"))
        .and_then(Value::as_str)
        .is_some();
    let mut total_video = 0usize;
    let mut direct_url = 0usize;
    let mut h264 = 0usize;
    let mut h264_direct = 0usize;
    let mut picked: Option<(String, String, String, u64, u64, u64)> = None;
    if let Some(streaming) = streaming {
        for group in ["formats", "adaptiveFormats"] {
            let Some(formats) = streaming.get(group).and_then(Value::as_array) else {
                continue;
            };
            for format in formats {
                let mime = format.get("mimeType").and_then(Value::as_str).unwrap_or("");
                if !mime.to_ascii_lowercase().contains("video/") {
                    continue;
                }
                total_video += 1;
                let url = format.get("url").and_then(Value::as_str).unwrap_or("");
                if !url.is_empty() {
                    direct_url += 1;
                }
                if !youtube_format_is_h264(format, mime) {
                    continue;
                }
                h264 += 1;
                if url.is_empty() {
                    continue;
                }
                h264_direct += 1;
                let quality = format
                    .get("qualityLabel")
                    .or_else(|| format.get("quality"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let itag = format.get("itag").and_then(Value::as_u64).unwrap_or(0);
                let width = format.get("width").and_then(Value::as_u64).unwrap_or(0);
                let height = format.get("height").and_then(Value::as_u64).unwrap_or(0);
                let bitrate = format.get("bitrate").and_then(Value::as_u64).unwrap_or(0);
                let score = youtube_direct_h264_score(group, itag, height);
                let replace = picked
                    .as_ref()
                    .map(|(_, _, _, old_score, _, _)| score > *old_score)
                    .unwrap_or(true);
                if replace {
                    picked = Some((
                        String::from(url),
                        String::from(mime),
                        String::from(quality),
                        score,
                        width,
                        bitrate,
                    ));
                }
            }
        }
    }
    crate::log!(
        "intel/hw_vid: youtube-innertube formats profile={} video_id={} playability={} reason={} regular={} adaptive={} total_video={} direct_url={} h264={} h264_direct={} server_abr={}\n",
        profile,
        video_id,
        playability_status,
        log_token(playability_reason).as_str(),
        regular,
        adaptive,
        total_video,
        direct_url,
        h264,
        h264_direct,
        server_abr as u8
    );
    if let Some((url, mime, quality, score, width, bitrate)) = picked {
        crate::log!(
            "intel/hw_vid: youtube-innertube direct-h264 candidate profile={} score={} quality={} width={} bitrate={} mime={} url={}\n",
            profile,
            score,
            quality,
            width,
            bitrate,
            mime,
            url
        );
        return Ok(url);
    }
    Err("browser media innertube no direct h264")
}

fn youtube_format_is_h264(format: &Value, mime: &str) -> bool {
    let lower = mime.to_ascii_lowercase();
    if lower.contains("video/mp4") && (lower.contains("avc1") || lower.contains("avc3")) {
        return true;
    }
    matches!(
        format.get("itag").and_then(Value::as_u64).unwrap_or(0),
        18 | 22 | 37 | 38 | 82 | 83 | 84 | 85
    )
}

fn youtube_direct_h264_score(group: &str, itag: u64, height: u64) -> u64 {
    if itag == 18 {
        return 10_000;
    }
    let progressive_bonus = if group == "formats" { 1_000 } else { 0 };
    progressive_bonus + height.min(4_000)
}

fn query_field(query: &str, key: &str) -> Option<String> {
    for part in query.split('&') {
        let (raw_key, raw_value) = part.split_once('=').unwrap_or((part, ""));
        if url_decode_component(raw_key).as_str() == key {
            return Some(url_decode_component(raw_value));
        }
    }
    None
}

fn youtube_url_has_query_field(url: &str, key: &str) -> bool {
    let Some((_, after_query)) = url.split_once('?') else {
        return false;
    };
    let query = after_query
        .split_once('#')
        .map(|(query, _)| query)
        .unwrap_or(after_query);
    query.split('&').any(|part| {
        let raw_key = part
            .split_once('=')
            .map(|(raw_key, _)| raw_key)
            .unwrap_or(part);
        url_decode_component(raw_key).as_str() == key
    })
}

fn url_decode_component(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
            {
                out.push((hi << 4) | lo);
                index += 3;
                continue;
            }
        }
        out.push(if bytes[index] == b'+' {
            b' '
        } else {
            bytes[index]
        });
        index += 1;
    }
    String::from_utf8(out).unwrap_or_default()
}

fn url_query_encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            let _ = write!(out, "%{:02X}", byte);
        }
    }
    out
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn json_escape(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04X}", ch as u32);
            }
            ch => out.push(ch),
        }
    }
    out
}

fn log_token(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars().take(96) {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':') {
            out.push(ch);
        } else if ch.is_whitespace() {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push('-');
    }
    out
}

fn bytes_contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

fn has_h264_start_code(bytes: &[u8]) -> bool {
    bytes_contains(bytes, &[0, 0, 1]) || bytes_contains(bytes, &[0, 0, 0, 1])
}

fn hex_prefix(bytes: &[u8], max_len: usize) -> String {
    let mut out = String::new();
    for (idx, byte) in bytes.iter().take(max_len).copied().enumerate() {
        if idx != 0 {
            out.push('_');
        }
        let _ = write!(out, "{:02X}", byte);
    }
    if out.is_empty() {
        out.push('-');
    }
    out
}

async fn h264_fetch_browser_media_candidate(url: &str) -> Result<Vec<u8>, &'static str> {
    h264_fetch_media_url_bytes(url, "browser-media").await
}

async fn h264_fetch_media_url_bytes(
    url: &str,
    log_scope: &'static str,
) -> Result<Vec<u8>, &'static str> {
    let youtube_n_param = youtube_url_has_query_field(url, "n");
    let profiles = [
        "browser-range",
        "plain-range",
        "youtube-range",
        "browser-norange",
        "plain-norange",
        "youtube-norange",
    ];
    for profile in profiles {
        let started = EmbassyInstant::now();
        crate::log!(
            "intel/hw_vid: {} fetch begin profile={} timeout_ms={} max_bytes={} youtube_n_param={} url={}\n",
            log_scope,
            profile,
            H264_BROWSER_MEDIA_FETCH_TIMEOUT_MS,
            H264_BROWSER_MEDIA_FETCH_MAX_BYTES,
            youtube_n_param as u8,
            url
        );
        match crate::r::net::https::get_browser_media_bytes_profile_shared(
            url,
            profile,
            H264_BROWSER_MEDIA_FETCH_TIMEOUT_MS as u32,
            H264_BROWSER_MEDIA_FETCH_MAX_BYTES,
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
        "intel/hw_vid: {} fetch exhausted profiles={} youtube_n_param={} action=check-url-signature-or-n-transform url={}\n",
        log_scope,
        profiles.len(),
        youtube_n_param as u8,
        url
    );
    Err("browser media fetch failed")
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
        "intel/hw_vid: browser-media mp4-demux track=video codec=avc1 mode={} track_id={} length_size={} samples={} sps={} pps={} annexb_bytes={} first_box={}\n",
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
    decoded_y_metric: u64,
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

struct H264MemoryNalReader {
    buffer_base: u64,
    scan_offset: usize,
    buffer: Vec<u8>,
    eof: bool,
}

impl H264MemoryNalReader {
    fn new(buffer: Vec<u8>) -> Self {
        Self {
            buffer_base: 0,
            scan_offset: 0,
            buffer,
            eof: true,
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

enum H264NalReader {
    Range(H264RangeNalReader),
    Memory(H264MemoryNalReader),
}

impl H264NalReader {
    async fn next_nal(&mut self) -> Option<H264BufferedNal> {
        match self {
            Self::Range(reader) => reader.next_nal().await,
            Self::Memory(reader) => reader.next_nal().await,
        }
    }
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

async fn h264_open_playback_stream_once() -> Option<crate::r::fs::trueosfs::FileReadHandle> {
    let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
        crate::log!(
            "intel/hw_vid: h264-playback-probe stream-open accepted=0 path={} reason=no-trueosfs-root mode=shell\n",
            H264_BOOT_PROBE_STREAM_PATH
        );
        return None;
    };
    match crate::r::fs::trueosfs::file_read_open_async(disk, H264_BOOT_PROBE_STREAM_PATH).await {
        Ok(Some(file)) => {
            crate::log!(
                "intel/hw_vid: h264-playback-probe stream-open accepted=1 path={} bytes={} data_lba={} source=trueosfs-root mode=shell-open-handle-range-stream\n",
                H264_BOOT_PROBE_STREAM_PATH,
                file.data_len(),
                file.data_lba()
            );
            Some(file)
        }
        Ok(None) => {
            crate::log!(
                "intel/hw_vid: h264-playback-probe stream-open accepted=0 path={} reason=file-missing mode=shell\n",
                H264_BOOT_PROBE_STREAM_PATH
            );
            None
        }
        Err(err) => {
            crate::log!(
                "intel/hw_vid: h264-playback-probe stream-open accepted=0 path={} reason=read-error err={:?} mode=shell\n",
                H264_BOOT_PROBE_STREAM_PATH,
                err
            );
            None
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

async fn h264_i_p_playback_probe(
    file: crate::r::fs::trueosfs::FileReadHandle,
    mode: H264PlaybackOptions,
) -> H264PlaybackReport {
    let stream_bytes = file.data_len();
    let reader = H264NalReader::Range(H264RangeNalReader::new(
        file,
        H264_BOOT_PROBE_STREAM_PATH,
        stream_bytes,
    ));
    h264_i_p_playback_probe_with_reader(
        reader,
        stream_bytes,
        "trueosfs-root",
        H264_BOOT_PROBE_STREAM_PATH,
        mode,
        Some(file),
    )
    .await
}

async fn h264_i_p_playback_probe_annexb_bytes(
    bytes: Vec<u8>,
    source: &'static str,
    path: &'static str,
    mode: H264PlaybackOptions,
) -> H264PlaybackReport {
    let stream_bytes = bytes.len() as u64;
    let reader = H264NalReader::Memory(H264MemoryNalReader::new(bytes));
    h264_i_p_playback_probe_with_reader(reader, stream_bytes, source, path, mode, None).await
}

async fn h264_i_p_playback_probe_with_reader(
    mut reader: H264NalReader,
    stream_bytes: u64,
    source: &'static str,
    path: &'static str,
    mode: H264PlaybackOptions,
    reverse_file: Option<crate::r::fs::trueosfs::FileReadHandle>,
) -> H264PlaybackReport {
    let mut nal_count = 0usize;
    let mut idr_seen = 0usize;
    let mut p_seen = 0usize;
    let mut submitted = 0usize;
    let mut skipped_missing_headers = 0usize;
    let mut last_sps: Option<Vec<u8>> = None;
    let mut last_pps: Option<Vec<u8>> = None;
    let mut access_units = Vec::new();
    let mut pending_au: Option<H264AccessUnitBuilder> = None;
    let mut vcl_nals_seen = 0usize;
    let mut indexed_frames = Vec::new();
    let mut last_idr_frame: Option<usize> = None;
    let forward_full_cache_enabled = matches!(mode.cache_mode(), H264PlaybackCacheMode::Full)
        && (mode.reverse_after_forward() || mode.stripe_study());
    let forward_tail_cache_enabled =
        matches!(mode.cache_mode(), H264PlaybackCacheMode::Tail) && mode.reverse_after_forward();
    let capture_forward_output = forward_full_cache_enabled || forward_tail_cache_enabled;
    let mut forward_full_cache = Vec::new();
    let mut forward_tail_start_frame = 0usize;
    let mut forward_tail_cache = Vec::new();
    let mut stopped_at = 0u64;
    let frame_period = mode.frame_period();
    let mut playback_timing = H264PlaybackTiming::default();

    crate::log!(
        "intel/hw_vid: h264-playback-probe start bytes={} fps={} frame_ms={} frame_ticks={} subset=idr-plus-p source={} path={} mode=range-stream chunk=0x{:X} playback_mode={} cache={} stripe_study={} fill={} diagnostics={} noreset_lite={} loop={} stop=eos\n",
        stream_bytes,
        mode.fps(),
        mode.frame_ms(),
        frame_period.as_ticks(),
        source,
        path,
        H264_BOOT_PROBE_STREAM_CHUNK_BYTES,
        mode.name(),
        mode.cache_mode().name(),
        mode.stripe_study() as u8,
        mode.show_cache_fill() as u8,
        mode.diagnostics() as u8,
        mode.noreset_lite() as u8,
        mode.loop_playback() as u8
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
            if forward_tail_cache_enabled {
                forward_tail_start_frame = indexed_frame;
                forward_tail_cache.clear();
            }
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
        indexed_frames.push(H264IndexedFrame {
            stream_offset: unit.stream_offset,
            bytes: unit.bytes,
            nal_type: unit.nal_type,
            stream_idr_index: idr_seen,
            decode_start_frame: last_idr_frame.unwrap_or(indexed_frame),
            detail,
            sps: unit.sps,
            pps: unit.pps,
        });
        if mode.diagnostics() {
            h264_log_frame_index(&indexed_frames[indexed_frame], indexed_frame);
        }

        submitted += 1;
        let decode_start = EmbassyInstant::now();
        let decoded = h264_submit_wait_probe_frame(
            "forward",
            submitted,
            idr_seen,
            &frame,
            true,
            capture_forward_output,
            mode.diagnostics(),
            Some(&mut playback_timing),
        )
        .await;
        playback_timing.record_decode_ticks(
            EmbassyInstant::now()
                .saturating_duration_since(decode_start)
                .as_ticks(),
        );
        if capture_forward_output {
            if let Some(decoded) = decoded {
                if forward_full_cache_enabled {
                    forward_full_cache.push(decoded);
                } else if forward_tail_cache_enabled {
                    forward_tail_cache.push(decoded);
                }
            }
        }
        h264_wait_until_next_frame(&mut next_frame_deadline, frame_period, &mut playback_timing)
            .await;
    }

    let forward_full_cache_frames = forward_full_cache.len();
    let forward_full_cache_bytes = h264_decoded_frames_total_bytes(forward_full_cache.as_slice());
    let forward_tail_cache_frames = forward_tail_cache.len();
    let forward_tail_cache_bytes = h264_decoded_frames_total_bytes(forward_tail_cache.as_slice());
    h264_log_keyframe_summary(indexed_frames.as_slice(), stream_bytes);
    let playback_report = playback_timing.report(mode, submitted, playback_start);

    crate::log!(
        "intel/hw_vid: h264-playback-probe done nals={} idr_seen={} p_seen={} submitted={} indexed_frames={} missing_headers={} stopped_at=0x{:X} target_fps={} target_frame_ms={} elapsed_ms={} effective_fps_x100={} waited_frames={} late_frames={} total_wait_ms={} avg_decode_us={} max_decode_us={} max_late_ms={} avg_queue_us={} avg_process_us={} avg_reset_us={} avg_zero_clear_us={} avg_zero_us={} avg_scratch_zero_us={} avg_output_clear_us={} avg_missing_clear_us={} avg_scratch_flush_us={} avg_build_ctx_us={} avg_poll_us={} max_poll_us={} avg_post_us={} avg_present_us={} max_present_us={} avg_poll_iters={} forward_full_cache={} forward_full_cache_bytes=0x{:X} forward_tail_cache={} forward_tail_cache_bytes=0x{:X} reason={}\n",
        nal_count,
        idr_seen,
        p_seen,
        submitted,
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
        forward_full_cache_frames,
        forward_full_cache_bytes,
        forward_tail_cache_frames,
        forward_tail_cache_bytes,
        "eos"
    );

    if mode.stripe_study() && forward_full_cache.len() == indexed_frames.len() {
        h264_stripe_study_from_full_cache(indexed_frames.as_slice(), forward_full_cache.as_slice())
            .await;
    }

    if mode.reverse_after_forward() {
        let forward_cache = if forward_full_cache_enabled && !forward_full_cache.is_empty() {
            Some(H264ForwardCache::Full(forward_full_cache))
        } else if forward_tail_cache_enabled && !forward_tail_cache.is_empty() {
            Some(H264ForwardCache::Tail(H264ForwardTailCache {
                start_frame: forward_tail_start_frame,
                frames: forward_tail_cache,
            }))
        } else {
            None
        };
        if let Some(file) = reverse_file {
            h264_reverse_playback_probe(file, indexed_frames.as_slice(), forward_cache, mode).await;
        } else {
            crate::log!(
                "intel/hw_vid: h264-reverse-probe skipped reason=source-not-seekable source={} path={}\n",
                source,
                path
            );
        }
    }
    playback_report
}

async fn h264_submit_wait_probe_frame(
    phase: &'static str,
    playback_frame: usize,
    stream_idr_index: usize,
    encoded: &[u8],
    present_output: bool,
    capture_output: bool,
    diagnostics: bool,
    mut timing: Option<&mut H264PlaybackTiming>,
) -> Option<H264DecodedFrame> {
    if diagnostics {
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

    if let Some(timing) = timing.as_deref_mut() {
        timing.record_hw_pic_timing(output.timing);
    }
    let present_start = EmbassyInstant::now();
    let stored = if present_output {
        h264_present_probe_output(phase, playback_frame, stream_idr_index, &output)
    } else {
        false
    };
    if let Some(timing) = timing.as_deref_mut() {
        timing.record_present_ticks(
            EmbassyInstant::now()
                .saturating_duration_since(present_start)
                .as_ticks(),
        );
    }

    if diagnostics {
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
    }
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
    mode: H264PlaybackOptions,
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
    let frame_period = mode.frame_period();
    let mut next_present_deadline = EmbassyInstant::now();
    let mut present_timing = H264PlaybackTiming::default();
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
        mode.show_cache_fill() as u8,
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
                h264_wait_until_next_frame(
                    &mut next_present_deadline,
                    frame_period,
                    &mut present_timing,
                )
                .await;
            }

            crate::log!(
                "intel/hw_vid: h264-reverse-probe done frames={} gops=0 presented={} submitted=0 cached_peak={} read_failures=0 decode_failures=0 visible_cache_fill={} waited_frames={} late_frames={} total_wait_ms={} max_late_ms={} strategy=forward-full-cache reason=eos\n",
                frames.len(),
                presented,
                cache.len(),
                mode.show_cache_fill() as u8,
                present_timing.waited_frames,
                present_timing.late_frames,
                h264_ticks_to_millis(present_timing.total_wait_ticks),
                h264_ticks_to_millis(present_timing.max_late_ticks)
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
                    if mode.show_cache_fill() {
                        "reverse-cache-visible"
                    } else {
                        "reverse-cache"
                    },
                    submitted,
                    frames[decode_frame].stream_idr_index,
                    packet.as_slice(),
                    mode.show_cache_fill(),
                    true,
                    mode.diagnostics(),
                    None,
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
            h264_wait_until_next_frame(
                &mut next_present_deadline,
                frame_period,
                &mut present_timing,
            )
            .await;
        }

        gop_end = gop_start;
    }

    crate::log!(
        "intel/hw_vid: h264-reverse-probe done frames={} gops={} presented={} submitted={} cached_peak={} read_failures={} decode_failures={} visible_cache_fill={} waited_frames={} late_frames={} total_wait_ms={} max_late_ms={} reason=eos\n",
        frames.len(),
        gops,
        presented,
        submitted,
        cached_peak,
        read_failures,
        decode_failures,
        mode.show_cache_fill() as u8,
        present_timing.waited_frames,
        present_timing.late_frames,
        h264_ticks_to_millis(present_timing.total_wait_ticks),
        h264_ticks_to_millis(present_timing.max_late_ticks)
    );
}

async fn h264_stripe_study_from_full_cache(
    frames: &[H264IndexedFrame],
    cache: &[H264DecodedFrame],
) {
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
        let primary_metric = snapshot
            .as_ref()
            .map(|snapshot| h264_vertical_black_stripe_metric(snapshot, decoded))
            .unwrap_or(0);
        let decoded_y_metric = h264_decoded_luma_stripe_metric(decoded);
        scanned += 1;
        captured += usize::from(snapshot.is_some());
        max_metric = max_metric.max(primary_metric);
        crate::log!(
            "intel/hw_vid: h264-stripe-study frame reverse_step={} source_frame={} gop_frame={} stream_idr={} class={} frame_num={} poc={}/{} primary_metric={} decoded_y_metric={} stored={} captured={} decoded={}x{} visible={}x{}\n",
            reverse_step,
            source_index + 1,
            h264_gop_frame_number(frame, source_index),
            frame.stream_idr_index,
            h264_frame_class_label(frame),
            h264_frame_num_i32(frame),
            h264_frame_poc_top(frame),
            h264_frame_poc_bottom(frame),
            primary_metric,
            decoded_y_metric,
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
                    metric: primary_metric,
                    decoded_y_metric,
                    source_index,
                    reverse_step,
                    snapshot,
                },
            );
        }
        Timer::after(EmbassyDuration::from_millis(H264_BOOT_PROBE_STRIPE_STUDY_FRAME_MS)).await;
    }

    h264_write_stripe_study_artifacts(frames, cache, candidates.as_slice()).await;
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
    cache: &[H264DecodedFrame],
    candidates: &[H264StripeStudyCandidate],
) {
    let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
        crate::log!("intel/hw_vid: h264-stripe-study write skipped reason=no-trueosfs-root\n");
        return;
    };

    let mut manifest = String::new();
    let _ = writeln!(
        manifest,
        "kind=h264-stripe-study format=primary-bgra8-raw+decoded-nv12-raw candidates={}",
        candidates.len()
    );

    for (rank, candidate) in candidates.iter().enumerate() {
        let frame = &frames[candidate.source_index];
        let primary_path = format!(
            "h264_stripe_study_rank{:02}_src{:05}_primary_metric{}.bgra",
            rank + 1,
            candidate.source_index + 1,
            candidate.metric
        );
        let primary_wrote = match crate::r::fs::trueosfs::file_in_async(
            disk,
            primary_path.as_str(),
            candidate.snapshot.pixels.as_slice(),
        )
        .await
        {
            Ok(ok) => ok,
            Err(err) => {
                crate::log!(
                    "intel/hw_vid: h264-stripe-study write failed path={} err={:?}\n",
                    primary_path.as_str(),
                    err
                );
                false
            }
        };
        let decoded_path = format!(
            "h264_stripe_study_rank{:02}_src{:05}_decoded_nv12.yuv",
            rank + 1,
            candidate.source_index + 1
        );
        let decoded = cache.get(candidate.source_index);
        let decoded_wrote = if let Some(decoded) = decoded {
            match crate::r::fs::trueosfs::file_in_async(
                disk,
                decoded_path.as_str(),
                decoded.bytes.as_slice(),
            )
            .await
            {
                Ok(ok) => ok,
                Err(err) => {
                    crate::log!(
                        "intel/hw_vid: h264-stripe-study write failed path={} err={:?}\n",
                        decoded_path.as_str(),
                        err
                    );
                    false
                }
            }
        } else {
            false
        };
        crate::log!(
            "intel/hw_vid: h264-stripe-study artifact rank={} primary_path={} primary_wrote={} decoded_path={} decoded_wrote={} source_frame={} reverse_step={} primary_metric={} decoded_y_metric={} primary_width={} primary_height={} primary_bytes=0x{:X} decoded_width={} decoded_height={} decoded_visible={}x{} decoded_pitch=0x{:X} decoded_uv=0x{:X} decoded_bytes=0x{:X} class={} frame_num={} poc={}/{}\n",
            rank + 1,
            primary_path.as_str(),
            primary_wrote as u8,
            decoded_path.as_str(),
            decoded_wrote as u8,
            candidate.source_index + 1,
            candidate.reverse_step,
            candidate.metric,
            candidate.decoded_y_metric,
            candidate.snapshot.width,
            candidate.snapshot.height,
            candidate.snapshot.pixels.len(),
            decoded.map(|decoded| decoded.width).unwrap_or(0),
            decoded.map(|decoded| decoded.height).unwrap_or(0),
            decoded.map(|decoded| decoded.visible_width).unwrap_or(0),
            decoded.map(|decoded| decoded.visible_height).unwrap_or(0),
            decoded.map(|decoded| decoded.pitch_bytes).unwrap_or(0),
            decoded.map(|decoded| decoded.uv_offset).unwrap_or(0),
            decoded.map(|decoded| decoded.bytes.len()).unwrap_or(0),
            h264_frame_class_label(frame),
            h264_frame_num_i32(frame),
            h264_frame_poc_top(frame),
            h264_frame_poc_bottom(frame)
        );
        let _ = writeln!(
            manifest,
            "rank={} primary_path={} primary_wrote={} decoded_path={} decoded_wrote={} source_frame={} reverse_step={} primary_metric={} decoded_y_metric={} primary_width={} primary_height={} primary_bytes={} decoded_width={} decoded_height={} decoded_visible_width={} decoded_visible_height={} decoded_pitch={} decoded_uv={} decoded_bytes={} class={} frame_num={} poc_top={} poc_bottom={}",
            rank + 1,
            primary_path.as_str(),
            primary_wrote as u8,
            decoded_path.as_str(),
            decoded_wrote as u8,
            candidate.source_index + 1,
            candidate.reverse_step,
            candidate.metric,
            candidate.decoded_y_metric,
            candidate.snapshot.width,
            candidate.snapshot.height,
            candidate.snapshot.pixels.len(),
            decoded.map(|decoded| decoded.width).unwrap_or(0),
            decoded.map(|decoded| decoded.height).unwrap_or(0),
            decoded.map(|decoded| decoded.visible_width).unwrap_or(0),
            decoded.map(|decoded| decoded.visible_height).unwrap_or(0),
            decoded.map(|decoded| decoded.pitch_bytes).unwrap_or(0),
            decoded.map(|decoded| decoded.uv_offset).unwrap_or(0),
            decoded.map(|decoded| decoded.bytes.len()).unwrap_or(0),
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

fn h264_decoded_luma_stripe_metric(frame: &H264DecodedFrame) -> u64 {
    let width = frame.visible_width as usize;
    let height = frame.visible_height as usize;
    let pitch = frame.pitch_bytes;
    const YTILE_W: usize = 128;
    if width < 16 || height < 16 || pitch < width || !pitch.is_multiple_of(YTILE_W) {
        return 0;
    }

    let tiles_per_row = pitch / YTILE_W;
    if tiles_per_row == 0 {
        return 0;
    }

    let x_start = width / 10;
    let x_end = width.saturating_mul(9) / 10;
    let y_start = height / 8;
    let y_end = height.saturating_mul(7) / 8;
    if x_end <= x_start + 2 || y_end <= y_start {
        return 0;
    }

    let mut max_deficit = 0u64;
    let mut total_deficit = 0u64;
    let mut x = x_start + 1;
    while x + 1 < x_end {
        let cur = h264_ytile_nv12_luma_column_average(frame, x, y_start, y_end, tiles_per_row);
        let left = h264_ytile_nv12_luma_column_average(frame, x - 1, y_start, y_end, tiles_per_row);
        let right =
            h264_ytile_nv12_luma_column_average(frame, x + 1, y_start, y_end, tiles_per_row);
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

fn h264_ytile_nv12_luma_column_average(
    frame: &H264DecodedFrame,
    x: usize,
    y_start: usize,
    y_end: usize,
    tiles_per_row: usize,
) -> u64 {
    let width = frame.visible_width as usize;
    if width == 0 || x >= width {
        return 0;
    }

    let mut sum = 0u64;
    let mut count = 0u64;
    let mut y = y_start;
    while y < y_end {
        let off = h264_ytile_8bpp_offset(x, y, tiles_per_row);
        if off < frame.bytes.len() && off < frame.uv_offset {
            sum = sum.saturating_add(frame.bytes[off] as u64);
            count += 1;
        }
        y += 8;
    }

    if count == 0 { 0 } else { sum / count }
}

#[inline(always)]
fn h264_ytile_8bpp_offset(byte_x: usize, row_y: usize, tiles_per_row: usize) -> usize {
    const YTILE_W: usize = 128;
    const YTILE_H: usize = 32;
    let tile_col = byte_x / YTILE_W;
    let tile_row = row_y / YTILE_H;
    let in_x = byte_x % YTILE_W;
    let in_y = row_y % YTILE_H;
    let oword_col = in_x / 16;
    let byte_in_oword = in_x % 16;
    let within_tile = oword_col * 512 + in_y * 16 + byte_in_oword;
    (tile_row * tiles_per_row + tile_col) * 4096 + within_tile
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

fn h264_present_probe_output(
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
        let src =
            unsafe { core::slice::from_raw_parts(output.virt_addr as *const u8, output.byte_len) };
        let reason = format!(
            "h264-decoded-nv12:{}:frame{}:idr{}:id{}",
            phase, playback_frame, stream_idr_index, output.id
        );
        let direct_presented = crate::intel::display::arm_decoded_nv12_overlay_plane_probe(
            reason.as_str(),
            output.gpu_addr,
            output.phys_addr,
            output.virt_addr,
            output.width,
            output.height,
            output.visible_width,
            output.visible_height,
            output.pitch_bytes,
            output.uv_offset,
            output.byte_len,
        );
        if direct_presented
            && crate::intel::display::decoded_nv12_overlay_plane_probe_replaces_cpu_present()
        {
            return true;
        }
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
