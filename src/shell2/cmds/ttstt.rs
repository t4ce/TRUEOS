extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use spin::Mutex;

use super::super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_line,
    print_shell_line, set_matrix_target_active, switch_matrix_target_slot,
};
use crate::r::ttstt_service::{
    ServiceState, SpeechRequestError, SttRequest, TtsAudioChunk, TtsRequest, TtsStreamEvent,
};
use crate::shell2::shell2_cmd::ParseOutcome;

const DEFAULT_VOICE: &str = "af_heart";
const DEFAULT_SPEED: f32 = 1.0;
// This is a defensive shell allocation cap, not Kokoro's model limit. The
// backend must enforce KOKORO_MAX_PHONEMES after language-specific G2P.
const TTS_TEXT_MAX_BYTES: usize = 8 * 1024;
const TTS_SHELL_QUEUE_DEPTH: usize = crate::r::ttstt_service::TTS_QUEUE_DEPTH;
const TTS_PCM_BACKPRESSURE_TIMEOUT_MS: u64 = 30_000;
const STT_AUDIO_MAX_BYTES: u64 = 64 * 1024 * 1024;
const STT_AUDIO_MAX_SECONDS: usize = 5 * 60;
const STT_SAMPLE_RATE_HZ: u32 = 16_000;
const STT_CONVERT_YIELD_FRAMES: usize = 8 * 1024;

struct ShellTtsRequest {
    id: u64,
    target: Option<MatrixTarget>,
    text: String,
    voice: String,
    speed: f32,
    spirit_talk: bool,
    stop_generation: u32,
    queued_at: Instant,
    capture: Option<crate::r::ttstt_capture::CaptureSession>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
enum TtsShellPhase {
    Idle = 0,
    ServiceAdmission = 1,
    Inference = 2,
    PcmHandoff = 3,
}

impl TtsShellPhase {
    fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::ServiceAdmission,
            2 => Self::Inference,
            3 => Self::PcmHandoff,
            _ => Self::Idle,
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::ServiceAdmission => "service-admission",
            Self::Inference => "inference",
            Self::PcmHandoff => "pcm-handoff",
        }
    }
}

static TTS_SHELL_QUEUE: Mutex<VecDeque<ShellTtsRequest>> = Mutex::new(VecDeque::new());
static TTS_SHELL_QUEUE_WAIT: crate::wait::WaitQueue = crate::wait::WaitQueue::new();
static TTS_SHELL_WORKER_STARTED: AtomicBool = AtomicBool::new(false);
static NEXT_TTS_SHELL_REQUEST_ID: AtomicU64 = AtomicU64::new(1);
static TTS_SHELL_ACTIVE_REQUEST_ID: AtomicU64 = AtomicU64::new(0);
static TTS_SHELL_ACTIVE_SERVICE_JOB_ID: AtomicU64 = AtomicU64::new(0);
static TTS_SHELL_PHASE: AtomicU8 = AtomicU8::new(TtsShellPhase::Idle as u8);
static TTS_SHELL_PCM_CHUNKS_HANDED_OFF: AtomicU64 = AtomicU64::new(0);
static TTS_SHELL_PCM_FRAMES_HANDED_OFF: AtomicU64 = AtomicU64::new(0);
static TTS_SHELL_REQUESTS_COMPLETED: AtomicU64 = AtomicU64::new(0);
static TTS_SHELL_REQUESTS_FAILED: AtomicU64 = AtomicU64::new(0);
static TTS_SHELL_REQUESTS_CANCELLED: AtomicU64 = AtomicU64::new(0);

pub(crate) fn try_parse_tts(
    _spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> ParseOutcome {
    let input = rest.trim();
    if input.is_empty() || input.eq_ignore_ascii_case("help") {
        tts_usage(io);
        return ParseOutcome::Handled;
    }
    if input.eq_ignore_ascii_case("status") {
        print_status(io, "tts");
        return ParseOutcome::Handled;
    }
    if input.eq_ignore_ascii_case("stop") {
        let generation = crate::aud::pcm_lane::request_stop();
        let cancelled = cancel_waiting_tts();
        print_shell_line(
            io,
            alloc::format!(
                "tts: stop generation={generation} cancelled_waiting={cancelled} active_stream_cancel=cooperative pcm_lane=cleared"
            )
            .as_str(),
        );
        return ParseOutcome::Handled;
    }
    if let Some(command) = input.split_whitespace().next()
        && command.eq_ignore_ascii_case("capture")
    {
        let argument = input.get(command.len()..).unwrap_or_default().trim();
        handle_tts_capture_command(io, argument);
        return ParseOutcome::Handled;
    }

    let (text, voice, speed) = match parse_tts_request(input) {
        Ok(request) => request,
        Err(reason) => {
            print_shell_line(io, alloc::format!("tts: rejected reason={reason}").as_str());
            tts_usage(io);
            return ParseOutcome::Handled;
        }
    };
    if crate::r::ttstt_service::speech_backend_name().is_none() {
        print_shell_line(
            io,
            "tts: unavailable reason=native-kokoro-backend-unregistered; models/pool status follows",
        );
        print_status(io, "tts");
        return ParseOutcome::Handled;
    }
    if crate::r::ttstt_service::status().state != ServiceState::Ready {
        print_shell_line(io, "tts: unavailable reason=model-service-not-ready");
        print_status(io, "tts");
        return ParseOutcome::Handled;
    }
    if !crate::r::ttstt_service::tts_backend_ready() {
        if let Some(reason) = crate::r::ttstt_service::speech_backend_warm_failure_reason() {
            print_shell_line(
                io,
                alloc::format!(
                    "tts: unavailable reason=native-kokoro-backend-rejected detail={reason}"
                )
                .as_str(),
            );
        } else {
            print_shell_line(io, "tts: unavailable reason=native-kokoro-backend-warming");
        }
        print_status(io, "tts");
        return ParseOutcome::Handled;
    }
    if !crate::r::readiness::is_set(crate::r::readiness::INTEL_HDA_READY) {
        print_shell_line(io, "tts: unavailable reason=intel-hda-not-ready");
        return ParseOutcome::Handled;
    }

    if let Err(reason) = ensure_tts_shell_worker() {
        print_shell_line(io, alloc::format!("tts: unavailable reason={reason}").as_str());
        return ParseOutcome::Handled;
    }

    let active_target = matrix_target_for_backend(io);
    let target = switch_matrix_target_slot(&active_target, "tts");
    set_matrix_target_active(&target, true);
    let request_id = NEXT_TTS_SHELL_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
    let capture = crate::r::ttstt_capture::claim_next(request_id, &text, &voice, speed);
    let request = ShellTtsRequest {
        id: request_id,
        target: Some(target.clone()),
        text,
        voice: voice.clone(),
        speed,
        spirit_talk: false,
        stop_generation: crate::aud::pcm_lane::stop_generation(),
        queued_at: Instant::now(),
        capture: capture.clone(),
    };
    let depth = {
        let mut queue = TTS_SHELL_QUEUE.lock();
        if queue.len() >= TTS_SHELL_QUEUE_DEPTH {
            None
        } else {
            queue.push_back(request);
            Some(queue.len())
        }
    };
    let Some(depth) = depth else {
        // Matrix/UI output can acquire unrelated locks; never invoke it while
        // holding the queue's spin mutex.
        set_matrix_target_active(&target, false);
        if let Some(capture) = capture.as_ref() {
            capture.fail("shell-queue-full");
        }
        print_matrix_target_line(
            &target,
            alloc::format!(
                "tts: submit failed reason=shell-queue-full cap={TTS_SHELL_QUEUE_DEPTH}"
            )
            .as_str(),
        );
        return ParseOutcome::Handled;
    };
    TTS_SHELL_QUEUE_WAIT.notify_one();
    print_matrix_target_line(
        &target,
        alloc::format!(
            "tts: queued request={request_id} waiting={depth}/{TTS_SHELL_QUEUE_DEPTH} voice={voice} speed={speed:.2} model_chunk_max_phonemes={} pcm_chunk_max_frames={}",
            crate::r::ttstt_service::KOKORO_MAX_PHONEMES,
            crate::r::ttstt_service::TTS_PCM_CHUNK_MAX_FRAMES,
        )
        .as_str(),
    );

    ParseOutcome::Handled
}

/// Queue a Lumen response on the same serialized synthesis/playback path as
/// the shell command, without creating a second Matrix presentation target.
pub(crate) fn enqueue_lumen_tts(text: &str) -> Result<u64, &'static str> {
    let text = text.trim();
    if text.is_empty() {
        return Err("empty-text");
    }
    if text.len() > TTS_TEXT_MAX_BYTES {
        return Err("text-too-long");
    }
    if crate::r::ttstt_service::speech_backend_name().is_none() {
        return Err("native-kokoro-backend-unregistered");
    }
    if crate::r::ttstt_service::status().state != ServiceState::Ready {
        return Err("model-service-not-ready");
    }
    if !crate::r::ttstt_service::tts_backend_ready() {
        return Err("native-kokoro-backend-not-ready");
    }
    if !crate::r::readiness::is_set(crate::r::readiness::INTEL_HDA_READY) {
        return Err("intel-hda-not-ready");
    }
    ensure_tts_shell_worker()?;

    let request_id = NEXT_TTS_SHELL_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
    let request = ShellTtsRequest {
        id: request_id,
        target: None,
        text: String::from(text),
        voice: String::from(DEFAULT_VOICE),
        speed: DEFAULT_SPEED,
        spirit_talk: true,
        stop_generation: crate::aud::pcm_lane::stop_generation(),
        queued_at: Instant::now(),
        capture: None,
    };
    let depth = {
        let mut queue = TTS_SHELL_QUEUE.lock();
        if queue.len() >= TTS_SHELL_QUEUE_DEPTH {
            return Err("tts-queue-full");
        }
        queue.push_back(request);
        queue.len()
    };
    TTS_SHELL_QUEUE_WAIT.notify_one();
    crate::log_info!(
        target: "ttstt";
        "lumen-tts: queued request={} waiting={}/{} voice={} speed={:.2}\n",
        request_id,
        depth,
        TTS_SHELL_QUEUE_DEPTH,
        DEFAULT_VOICE,
        DEFAULT_SPEED,
    );
    Ok(request_id)
}

fn ensure_tts_shell_worker() -> Result<(), &'static str> {
    if TTS_SHELL_WORKER_STARTED.load(Ordering::Acquire) {
        return Ok(());
    }
    let Some(ap1) = crate::workers::ap1_ui_core_spawner() else {
        return Err("ap1-audio-service-core-unavailable");
    };
    if TTS_SHELL_WORKER_STARTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Ok(());
    }
    match tts_shell_worker_task() {
        Ok(token) => {
            ap1.spawn(token);
            Ok(())
        }
        Err(_) => {
            TTS_SHELL_WORKER_STARTED.store(false, Ordering::Release);
            Err("tts-queue-worker-spawn-failed")
        }
    }
}

fn tts_request_line(request: &ShellTtsRequest, message: &str) {
    if let Some(target) = request.target.as_ref() {
        print_matrix_target_line(target, message);
    } else {
        crate::log_info!(target: "ttstt"; "lumen-tts: {}\n", message);
    }
}

fn finish_tts_request(request: &ShellTtsRequest) {
    if let Some(target) = request.target.as_ref() {
        set_matrix_target_active(target, false);
    }
}

fn handle_tts_capture_command(io: &'static dyn ShellBackend2, argument: &str) {
    if argument.eq_ignore_ascii_case("next") {
        let armed = crate::r::ttstt_capture::arm_next();
        print_shell_line(
            io,
            if armed {
                "tts: capture armed mode=next max_seconds=30 outputs=mono24-f32+stereo48-s16+metadata"
            } else {
                "tts: capture already armed mode=next"
            },
        );
        return;
    }
    if argument.eq_ignore_ascii_case("off") {
        let was_armed = crate::r::ttstt_capture::disarm();
        print_shell_line(
            io,
            if was_armed {
                "tts: capture disarmed"
            } else {
                "tts: capture already disarmed"
            },
        );
        return;
    }
    if argument.is_empty() || argument.eq_ignore_ascii_case("status") {
        let status = crate::r::ttstt_capture::status();
        print_shell_line(
            io,
            alloc::format!(
                "tts: capture armed={} writer_online={} busy={} queued={} queued_total={} written={} failed={} dropped={} last_sequence={} mode=one-shot max_seconds=30",
                status.armed as u8,
                status.writer_online as u8,
                status.busy as u8,
                status.queued,
                status.captures_queued,
                status.captures_written,
                status.captures_failed,
                status.captures_dropped,
                status.last_sequence,
            )
            .as_str(),
        );
        return;
    }
    print_shell_line(io, "tts: capture usage `tts capture next|off|status`");
}

fn cancel_waiting_tts() -> usize {
    let waiting = {
        let mut queue = TTS_SHELL_QUEUE.lock();
        queue.drain(..).collect::<Vec<_>>()
    };
    let count = waiting.len();
    for request in waiting {
        if let Some(capture) = request.capture.as_ref() {
            capture.fail("cancelled-before-inference");
        }
        TTS_SHELL_REQUESTS_CANCELLED.fetch_add(1, Ordering::Relaxed);
        tts_request_line(
            &request,
            alloc::format!("tts: cancelled request={} before inference", request.id).as_str(),
        );
        finish_tts_request(&request);
    }
    count
}

struct TtsShellQueueStatus {
    waiting: usize,
    phase: TtsShellPhase,
    active_request_id: u64,
    active_service_job_id: u64,
}

fn tts_shell_queue_status() -> TtsShellQueueStatus {
    TtsShellQueueStatus {
        waiting: TTS_SHELL_QUEUE.lock().len(),
        phase: TtsShellPhase::from_u8(TTS_SHELL_PHASE.load(Ordering::Acquire)),
        active_request_id: TTS_SHELL_ACTIVE_REQUEST_ID.load(Ordering::Acquire),
        active_service_job_id: TTS_SHELL_ACTIVE_SERVICE_JOB_ID.load(Ordering::Acquire),
    }
}

#[embassy_executor::task]
async fn tts_shell_worker_task() {
    loop {
        let request = { TTS_SHELL_QUEUE.lock().pop_front() };
        let Some(request) = request else {
            TTS_SHELL_QUEUE_WAIT.wait_for_event_timeout(25).await;
            continue;
        };
        TTS_SHELL_ACTIVE_REQUEST_ID.store(request.id, Ordering::Release);
        TTS_SHELL_ACTIVE_SERVICE_JOB_ID.store(0, Ordering::Release);
        TTS_SHELL_PHASE.store(TtsShellPhase::ServiceAdmission as u8, Ordering::Release);
        process_tts_request(request).await;
        TTS_SHELL_PHASE.store(TtsShellPhase::Idle as u8, Ordering::Release);
        TTS_SHELL_ACTIVE_SERVICE_JOB_ID.store(0, Ordering::Release);
        TTS_SHELL_ACTIVE_REQUEST_ID.store(0, Ordering::Release);
    }
}

async fn process_tts_request(request: ShellTtsRequest) {
    let active_started = Instant::now();
    let queue_wait_ms = active_started
        .saturating_duration_since(request.queued_at)
        .as_millis();
    let capture = request.capture.clone();
    if crate::aud::pcm_lane::stop_generation() != request.stop_generation {
        if let Some(capture) = capture.as_ref() {
            capture.fail("stopped-before-inference");
        }
        TTS_SHELL_REQUESTS_CANCELLED.fetch_add(1, Ordering::Relaxed);
        tts_request_line(
            &request,
            alloc::format!("tts: cancelled request={} before inference", request.id).as_str(),
        );
        finish_tts_request(&request);
        return;
    }

    let voice = request.voice.clone();
    let submission = match crate::r::ttstt_service::submit_tts(TtsRequest {
        text: request.text.clone(),
        voice: request.voice.clone(),
        speed: request.speed,
        capture: capture.clone(),
    }) {
        Ok(submission) => submission,
        Err(error) => {
            if let Some(capture) = capture.as_ref() {
                capture.fail("service-submit-failed");
            }
            TTS_SHELL_REQUESTS_FAILED.fetch_add(1, Ordering::Relaxed);
            tts_request_line(
                &request,
                alloc::format!(
                    "tts: submit failed request={} reason={}",
                    request.id,
                    request_error(error)
                )
                .as_str(),
            );
            finish_tts_request(&request);
            return;
        }
    };
    if let Some(capture) = capture.as_ref() {
        capture.set_job_id(submission.id);
    }
    let service_submit_returned = Instant::now();
    TTS_SHELL_ACTIVE_SERVICE_JOB_ID.store(submission.id, Ordering::Release);
    TTS_SHELL_PHASE.store(TtsShellPhase::Inference as u8, Ordering::Release);
    tts_request_line(
        &request,
        alloc::format!(
            "tts: job submitted request={} job={} queue=ttstt-common voice={voice} speed={:.2} shell_queue_wait_ms={queue_wait_ms}",
            request.id,
            submission.id,
            request.speed
        )
        .as_str(),
    );

    let stream = submission.stream;
    let mut handed_model_chunks = 0u32;
    let mut handed_pcm_chunks = 0u32;
    let mut handed_pcm_frames = 0u64;
    let mut first_pcm_ms = None;
    let mut service_queue_wait_us = 0u64;
    let mut service_dispatch_to_first_pcm_us = 0u64;
    let mut service_submit_to_first_pcm_us = 0u64;
    let mut handoff_wait_ms = 0u64;
    loop {
        if crate::aud::pcm_lane::stop_generation() != request.stop_generation {
            stream.cancel();
            if let Some(capture) = capture.as_ref() {
                capture.fail("stopped-during-inference");
            }
            TTS_SHELL_REQUESTS_CANCELLED.fetch_add(1, Ordering::Relaxed);
            tts_request_line(
                &request,
                alloc::format!(
                    "tts: stopped request={} job={} handed_pcm_chunks={} handed_pcm_frames={}",
                    request.id,
                    submission.id,
                    handed_pcm_chunks,
                    handed_pcm_frames
                )
                .as_str(),
            );
            finish_tts_request(&request);
            return;
        }

        let Some(event) = stream.try_next() else {
            stream.wait_for_event_timeout(25).await;
            continue;
        };
        match event {
            TtsStreamEvent::Chunk(chunk) => {
                if first_pcm_ms.is_none() {
                    let observed_at = Instant::now();
                    first_pcm_ms = Some(
                        observed_at
                            .saturating_duration_since(service_submit_returned)
                            .as_millis(),
                    );
                    if let Some(timing) = stream.service_timing_at(observed_at) {
                        service_queue_wait_us = timing.initial_queue_us;
                        service_dispatch_to_first_pcm_us = timing.dispatch_elapsed_us;
                        service_submit_to_first_pcm_us = timing.submit_elapsed_us;
                    }
                }
                if let Some(capture) = capture.as_ref() {
                    capture.push_pcm(chunk.samples_i16_stereo_48k.as_slice());
                }
                let end_of_model_chunk = chunk.end_of_model_chunk;
                TTS_SHELL_PHASE.store(TtsShellPhase::PcmHandoff as u8, Ordering::Release);
                let handoff_started = Instant::now();
                match handoff_tts_audio_chunk(chunk, request.stop_generation, request.spirit_talk)
                    .await
                {
                    Ok(frames) => {
                        handoff_wait_ms = handoff_wait_ms.saturating_add(
                            Instant::now()
                                .saturating_duration_since(handoff_started)
                                .as_millis(),
                        );
                        if end_of_model_chunk {
                            handed_model_chunks = handed_model_chunks.saturating_add(1);
                        }
                        handed_pcm_chunks = handed_pcm_chunks.saturating_add(1);
                        handed_pcm_frames = handed_pcm_frames.saturating_add(frames as u64);
                        TTS_SHELL_PCM_CHUNKS_HANDED_OFF.fetch_add(1, Ordering::Relaxed);
                        TTS_SHELL_PCM_FRAMES_HANDED_OFF.fetch_add(frames as u64, Ordering::Relaxed);
                        TTS_SHELL_PHASE.store(TtsShellPhase::Inference as u8, Ordering::Release);
                    }
                    Err(reason) => {
                        stream.cancel();
                        if let Some(capture) = capture.as_ref() {
                            capture.fail("pcm-handoff-failed");
                        }
                        if crate::aud::pcm_lane::stop_generation() != request.stop_generation {
                            TTS_SHELL_REQUESTS_CANCELLED.fetch_add(1, Ordering::Relaxed);
                        } else {
                            TTS_SHELL_REQUESTS_FAILED.fetch_add(1, Ordering::Relaxed);
                        }
                        tts_request_line(
                            &request,
                            alloc::format!(
                                "tts: pcm handoff failed request={} job={} reason={reason}",
                                request.id,
                                submission.id
                            )
                            .as_str(),
                        );
                        finish_tts_request(&request);
                        return;
                    }
                }
            }
            TtsStreamEvent::Finished(Ok(summary)) => {
                if handed_model_chunks != summary.model_chunks
                    || handed_pcm_chunks != summary.pcm_chunks
                    || handed_pcm_frames != summary.pcm_frames
                {
                    if let Some(capture) = capture.as_ref() {
                        capture.fail("stream-accounting-failed");
                    }
                    TTS_SHELL_REQUESTS_FAILED.fetch_add(1, Ordering::Relaxed);
                    tts_request_line(
                        &request,
                        alloc::format!(
                            "tts: stream accounting failed request={} job={} model_chunks={}/{} pcm_chunks={}/{} frames={}/{}",
                            request.id,
                            submission.id,
                            handed_model_chunks,
                            summary.model_chunks,
                            handed_pcm_chunks,
                            summary.pcm_chunks,
                            handed_pcm_frames,
                            summary.pcm_frames,
                        )
                        .as_str(),
                    );
                    finish_tts_request(&request);
                    return;
                }
                let finished_at = Instant::now();
                let active_ms = finished_at
                    .saturating_duration_since(active_started)
                    .as_millis();
                let total_ms = finished_at
                    .saturating_duration_since(request.queued_at)
                    .as_millis();
                let first_pcm_ms = first_pcm_ms.unwrap_or(0);
                if let Some(capture) = capture.as_ref() {
                    let _ = capture.finish_success(
                        summary.model_chunks,
                        summary.pcm_chunks,
                        summary.pcm_frames,
                        crate::r::ttstt_capture::CaptureTiming {
                            queue_wait_ms,
                            service_queue_wait_us,
                            service_dispatch_to_first_pcm_us,
                            service_submit_to_first_pcm_us,
                            first_pcm_ms,
                            handoff_wait_ms,
                            active_ms,
                            total_ms,
                        },
                    );
                }
                TTS_SHELL_REQUESTS_COMPLETED.fetch_add(1, Ordering::Relaxed);
                tts_request_line(
                    &request,
                    alloc::format!(
                        "tts: pcm handoff complete request={} job={} model_chunks={} pcm_chunks={} frames={} audio_ms={} shell_queue_wait_ms={} service_queue_wait_us={} service_dispatch_to_first_pcm_us={} service_submit_to_first_pcm_us={} first_pcm_ms={} handoff_wait_ms={} active_ms={} total_ms={} format=s16le/stereo/48k playback_completion=untracked",
                        request.id,
                        submission.id,
                        summary.model_chunks,
                        summary.pcm_chunks,
                        summary.pcm_frames,
                        summary.pcm_frames.saturating_mul(1_000)
                            / crate::r::ttstt_service::TTS_PCM_SAMPLE_RATE_HZ as u64,
                        queue_wait_ms,
                        service_queue_wait_us,
                        service_dispatch_to_first_pcm_us,
                        service_submit_to_first_pcm_us,
                        first_pcm_ms,
                        handoff_wait_ms,
                        active_ms,
                        total_ms,
                    )
                    .as_str(),
                );
                finish_tts_request(&request);
                return;
            }
            TtsStreamEvent::Finished(Err(reason)) => {
                if let Some(capture) = capture.as_ref() {
                    capture.fail(reason);
                }
                TTS_SHELL_REQUESTS_FAILED.fetch_add(1, Ordering::Relaxed);
                tts_request_line(
                    &request,
                    alloc::format!(
                        "tts: inference failed request={} job={} reason={reason} partial_pcm_chunks={} partial_pcm_frames={}",
                        request.id,
                        submission.id,
                        handed_pcm_chunks,
                        handed_pcm_frames,
                    )
                    .as_str(),
                );
                finish_tts_request(&request);
                return;
            }
        }
    }
}

pub(crate) fn try_parse_stt(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> ParseOutcome {
    let input = rest.trim();
    if input.is_empty() || input.eq_ignore_ascii_case("help") {
        stt_usage(io);
        return ParseOutcome::Handled;
    }
    if input.eq_ignore_ascii_case("status") {
        print_status(io, "stt");
        print_capture_status(io);
        return ParseOutcome::Handled;
    }
    if input
        .split_whitespace()
        .next()
        .is_some_and(|word| word.eq_ignore_ascii_case("record"))
    {
        print_capture_status(io);
        print_shell_line(
            io,
            "stt: record is experimental and disabled until HDA input BDL/stream routing is implemented",
        );
        return ParseOutcome::Handled;
    }
    if crate::r::ttstt_service::speech_backend_name().is_none() {
        print_shell_line(
            io,
            "stt: unavailable reason=native-whisper-backend-unregistered; no audio file was read",
        );
        print_status(io, "stt");
        return ParseOutcome::Handled;
    }
    if crate::r::ttstt_service::status().state != ServiceState::Ready {
        print_shell_line(
            io,
            "stt: unavailable reason=model-service-not-ready; no audio file was read",
        );
        print_status(io, "stt");
        return ParseOutcome::Handled;
    }
    if !crate::r::ttstt_service::stt_backend_ready() {
        if let Some(reason) = crate::r::ttstt_service::speech_backend_warm_failure_reason() {
            print_shell_line(
                io,
                alloc::format!("stt: unavailable reason=speech-backend-rejected detail={reason}; no audio file was read")
                    .as_str(),
            );
        } else {
            print_shell_line(
                io,
                "stt: unavailable reason=native-whisper-backend-warming; no audio file was read",
            );
        }
        print_status(io, "stt");
        return ParseOutcome::Handled;
    }

    let command = match parse_stt_file_request(input) {
        Ok(command) => command,
        Err(reason) => {
            print_shell_line(io, alloc::format!("stt: rejected reason={reason}").as_str());
            stt_usage(io);
            return ParseOutcome::Handled;
        }
    };
    let active_target = matrix_target_for_backend(io);
    let target = switch_matrix_target_slot(&active_target, "stt");
    set_matrix_target_active(&target, true);
    match stt_file_task(target.clone(), command) {
        Ok(token) => {
            spawner.spawn(token);
            print_matrix_target_line(&target, "stt: audio load queued on BSP");
        }
        Err(error) => {
            print_matrix_target_line(
                &target,
                alloc::format!("stt: task spawn failed err={error:?}").as_str(),
            );
            set_matrix_target_active(&target, false);
        }
    }
    ParseOutcome::Handled
}

fn print_status(io: &'static dyn ShellBackend2, command: &str) {
    let status = crate::r::ttstt_service::status();
    let state = match status.state {
        ServiceState::WaitingForModels => "waiting-models",
        ServiceState::LoadingModels => "loading-models",
        ServiceState::ModelsResident => "models-resident",
        ServiceState::Ready => "ready",
    };
    let backend = crate::r::ttstt_service::speech_backend_name().unwrap_or("unregistered");
    let backend_failure = crate::r::ttstt_service::speech_backend_warm_failure_reason();
    let direction_ready = if command == "tts" {
        crate::r::ttstt_service::tts_backend_ready()
    } else {
        crate::r::ttstt_service::stt_backend_ready()
    };
    let backend_state = if backend == "unregistered" {
        "unregistered"
    } else if backend_failure.is_some() {
        "rejected"
    } else if direction_ready {
        "ready"
    } else {
        "warming"
    };
    if command == "tts" {
        let shell = tts_shell_queue_status();
        print_shell_line(
            io,
            alloc::format!(
                "tts: state={state} backend={backend} backend_state={backend_state} backend_reason={} resident_bytes={} workers={} outstanding={} tts_outstanding={} tts_queued={} tts_active_job={} output_buffered_chunks={}/{} output_buffered_frames={} shell_waiting={}/{} shell_phase={} shell_request={} shell_job={} pcm_lane_pending_frames={} pcm_lane_paused={} hda_ready={} model_chunk_max_phonemes={} pcm_chunk_max_frames={} emitted_chunks={} emitted_frames={} service_streams_ok={} service_streams_failed={} handed_chunks={} handed_frames={} shell_completed={} shell_failed={} shell_cancelled={} policy=AP2+-prefer-pcore playback_completion=untracked",
                backend_failure.unwrap_or("none"),
                status.resident_bytes,
                status.workers,
                status.outstanding_jobs,
                status.outstanding_tts_jobs,
                status.queued_tts_jobs,
                status.active_tts_job_id.unwrap_or(0),
                status.tts_pcm_chunks_buffered,
                crate::r::ttstt_service::TTS_OUTPUT_QUEUE_DEPTH,
                status.tts_pcm_frames_buffered,
                shell.waiting,
                TTS_SHELL_QUEUE_DEPTH,
                shell.phase.as_str(),
                shell.active_request_id,
                shell.active_service_job_id,
                crate::aud::pcm_lane::pending_frames(),
                crate::aud::pcm_lane::paused() as u8,
                crate::r::readiness::is_set(crate::r::readiness::INTEL_HDA_READY) as u8,
                crate::r::ttstt_service::KOKORO_MAX_PHONEMES,
                crate::r::ttstt_service::TTS_PCM_CHUNK_MAX_FRAMES,
                status.tts_pcm_chunks_emitted,
                status.tts_pcm_frames_emitted,
                status.tts_streams_completed,
                status.tts_streams_failed,
                TTS_SHELL_PCM_CHUNKS_HANDED_OFF.load(Ordering::Acquire),
                TTS_SHELL_PCM_FRAMES_HANDED_OFF.load(Ordering::Acquire),
                TTS_SHELL_REQUESTS_COMPLETED.load(Ordering::Acquire),
                TTS_SHELL_REQUESTS_FAILED.load(Ordering::Acquire),
                TTS_SHELL_REQUESTS_CANCELLED.load(Ordering::Acquire),
            )
            .as_str(),
        );
    } else {
        print_shell_line(
            io,
            alloc::format!(
                "{command}: state={state} backend={backend} backend_state={backend_state} backend_reason={} resident_bytes={} workers={} outstanding={} policy=AP2+-prefer-pcore",
                backend_failure.unwrap_or("none"),
                status.resident_bytes,
                status.workers,
                status.outstanding_jobs
            )
            .as_str(),
        );
    }
}

fn print_capture_status(io: &'static dyn ShellBackend2) {
    match crate::hda::pcm_capture_capabilities() {
        Some(caps) => print_shell_line(
            io,
            alloc::format!(
                "stt: hda_capture input_streams={} adc_widgets={} mic_pins={} line_input_pins={} dma_configured={}",
                caps.input_streams,
                caps.adc_widgets,
                caps.microphone_pins,
                caps.line_input_pins,
                caps.dma_configured as u8
            )
            .as_str(),
        ),
        None => print_shell_line(io, "stt: hda_capture unavailable reason=hda-not-initialized"),
    }
}

fn parse_tts_request(input: &str) -> Result<(String, String, f32), &'static str> {
    let mut remaining = input.trim_start();
    let mut voice = DEFAULT_VOICE.to_string();
    let mut speed = DEFAULT_SPEED;
    loop {
        let token_end = remaining
            .char_indices()
            .find_map(|(index, ch)| ch.is_whitespace().then_some(index))
            .unwrap_or(remaining.len());
        let token = &remaining[..token_end];
        if let Some(value) = token.strip_prefix("voice=") {
            if value.is_empty() {
                return Err("empty-voice");
            }
            voice = value.to_string();
        } else if let Some(value) = token.strip_prefix("speed=") {
            speed = value.parse::<f32>().map_err(|_| "invalid-speed")?;
            if !speed.is_finite() || !(0.5..=2.0).contains(&speed) {
                return Err("speed-out-of-range-0.5-to-2.0");
            }
        } else {
            break;
        }
        remaining = remaining[token_end..].trim_start();
        if remaining.is_empty() {
            return Err("empty-text");
        }
    }

    let text = if remaining.starts_with('"') {
        parse_quoted_text(remaining)?
    } else {
        remaining.to_string()
    };
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("empty-text");
    }
    if text.len() > TTS_TEXT_MAX_BYTES {
        return Err("text-too-large");
    }
    Ok((text, voice, speed))
}

fn parse_quoted_text(input: &str) -> Result<String, &'static str> {
    let quoted = input
        .strip_prefix('"')
        .ok_or("text-must-start-with-quote")?;
    let mut text = String::new();
    let mut escaped = false;
    for (offset, ch) in quoted.char_indices() {
        if escaped {
            match ch {
                '"' | '\\' => text.push(ch),
                'n' => text.push('\n'),
                'r' => text.push('\r'),
                't' => text.push('\t'),
                _ => return Err("unsupported-escape"),
            }
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => {
                let tail = &quoted[offset + ch.len_utf8()..];
                if !tail.trim().is_empty() {
                    return Err("unexpected-text-after-closing-quote");
                }
                return Ok(text);
            }
            _ => text.push(ch),
        }
    }
    Err("missing-closing-quote")
}

struct SttFileCommand {
    path: String,
    language: Option<String>,
    translate: bool,
}

fn parse_stt_file_request(input: &str) -> Result<SttFileCommand, &'static str> {
    let mut args = input.split_whitespace();
    let first = args.next().ok_or("missing-path")?;
    let path = if first.eq_ignore_ascii_case("file") {
        args.next().ok_or("missing-path")?
    } else {
        first
    };
    let path = path.trim_start_matches('/');
    if path.is_empty() {
        return Err("missing-path");
    }
    let mut language = Some(String::from("en"));
    let mut translate = false;
    for arg in args {
        if let Some(value) = arg.strip_prefix("language=") {
            if value.eq_ignore_ascii_case("auto") {
                language = None;
            } else if value.is_empty() {
                return Err("empty-language");
            } else {
                language = Some(value.to_string());
            }
        } else if arg.eq_ignore_ascii_case("translate") {
            translate = true;
        } else {
            return Err("unknown-option");
        }
    }
    Ok(SttFileCommand {
        path: path.to_string(),
        language,
        translate,
    })
}

#[embassy_executor::task(pool_size = 2)]
async fn stt_file_task(target: MatrixTarget, command: SttFileCommand) {
    let result = load_wav_mono_16k(command.path.as_str()).await;
    let pcm = match result {
        Ok(pcm) => pcm,
        Err(reason) => {
            print_matrix_target_line(&target, alloc::format!("stt: {reason}").as_str());
            set_matrix_target_active(&target, false);
            return;
        }
    };
    let samples = pcm.len();
    let completion_target = target.clone();
    let complete = Box::new(move |result: Result<String, &'static str>| {
        match result {
            Ok(text) => {
                let text = text.replace(['\r', '\n'], " ");
                print_matrix_target_line(
                    &completion_target,
                    alloc::format!("stt: text={text}").as_str(),
                );
            }
            Err(reason) => print_matrix_target_line(
                &completion_target,
                alloc::format!("stt: inference failed reason={reason}").as_str(),
            ),
        }
        set_matrix_target_active(&completion_target, false);
    });
    match crate::r::ttstt_service::submit_stt(SttRequest {
        pcm_f32_mono_16k: pcm,
        language: command.language,
        translate: command.translate,
        complete,
    }) {
        Ok(id) => print_matrix_target_line(
            &target,
            alloc::format!(
                "stt: queued id={id} path=trueosfs:/{} samples={} rate=16000 channels=1",
                command.path,
                samples
            )
            .as_str(),
        ),
        Err(error) => {
            print_matrix_target_line(
                &target,
                alloc::format!("stt: submit failed reason={}", request_error(error)).as_str(),
            );
            set_matrix_target_active(&target, false);
        }
    }
}

async fn load_wav_mono_16k(path: &str) -> Result<Vec<f32>, String> {
    let disk = crate::r::fs::trueosfs::primary_root_handle()
        .ok_or_else(|| String::from("no TRUEOSFS root mounted"))?;
    let info = crate::r::fs::trueosfs::file_info_async(disk, path)
        .await
        .map_err(|error| alloc::format!("file info failed path=trueosfs:/{path} err={error:?}"))?
        .ok_or_else(|| alloc::format!("file missing path=trueosfs:/{path}"))?;
    if info.data_len > STT_AUDIO_MAX_BYTES {
        return Err(alloc::format!(
            "audio too large bytes={} cap={STT_AUDIO_MAX_BYTES}",
            info.data_len
        ));
    }
    let bytes = crate::r::fs::trueosfs::file_out_async(disk, path)
        .await
        .map_err(|error| alloc::format!("file read failed path=trueosfs:/{path} err={error:?}"))?
        .ok_or_else(|| alloc::format!("file missing path=trueosfs:/{path}"))?;
    if bytes.len() as u64 > STT_AUDIO_MAX_BYTES {
        return Err(alloc::format!(
            "audio changed while loading bytes={} cap={STT_AUDIO_MAX_BYTES}",
            bytes.len()
        ));
    }
    let wav = crate::hda::parse_wav(bytes.as_slice()).map_err(String::from)?;
    if wav.bits_per_sample != 16 {
        return Err(String::from("WAV must contain signed 16-bit PCM"));
    }
    if wav.channels == 0 || wav.channels > 2 || wav.sample_rate == 0 {
        return Err(String::from("WAV must be mono/stereo with a nonzero sample rate"));
    }
    let channels = wav.channels as usize;
    let pcm = &bytes[wav.data_offset..wav.data_offset + wav.data_size];
    let source_frames = pcm.len() / (2 * channels);
    let max_source_frames = (wav.sample_rate as usize).saturating_mul(STT_AUDIO_MAX_SECONDS);
    if source_frames > max_source_frames {
        return Err(alloc::format!("audio duration exceeds {} seconds", STT_AUDIO_MAX_SECONDS));
    }
    let output_frames = (source_frames as u64)
        .saturating_mul(STT_SAMPLE_RATE_HZ as u64)
        .div_ceil(wav.sample_rate as u64) as usize;
    let mut out = Vec::new();
    out.try_reserve_exact(output_frames)
        .map_err(|_| String::from("cannot reserve STT PCM buffer"))?;

    for output_frame in 0..output_frames {
        let source_frame = (output_frame as u64)
            .saturating_mul(wav.sample_rate as u64)
            .checked_div(STT_SAMPLE_RATE_HZ as u64)
            .unwrap_or(0) as usize;
        if source_frame >= source_frames {
            break;
        }
        let sample_base = source_frame * channels;
        let left_offset = sample_base * 2;
        let left = i16::from_le_bytes([pcm[left_offset], pcm[left_offset + 1]]) as i32;
        let mono = if channels == 2 {
            let right_offset = left_offset + 2;
            let right = i16::from_le_bytes([pcm[right_offset], pcm[right_offset + 1]]) as i32;
            (left + right) / 2
        } else {
            left
        };
        out.push(mono as f32 / 32768.0);
        if output_frame != 0 && output_frame % STT_CONVERT_YIELD_FRAMES == 0 {
            Timer::after(EmbassyDuration::from_millis(1)).await;
        }
    }
    if out.is_empty() {
        return Err(String::from("WAV contains no PCM frames"));
    }
    Ok(out)
}

async fn handoff_tts_audio_chunk(
    chunk: TtsAudioChunk,
    stop_generation: u32,
    spirit_talk: bool,
) -> Result<usize, String> {
    let frames = chunk
        .validate()
        .map_err(|reason| alloc::format!("invalid-backend-chunk-{reason}"))?;
    let mut samples = chunk.samples_i16_stereo_48k;
    let wait_started = Instant::now();
    loop {
        if crate::aud::pcm_lane::stop_generation() != stop_generation {
            return Err(String::from("stopped"));
        }
        if crate::aud::pcm_lane::pending_frames().saturating_add(frames)
            > crate::r::ttstt_service::TTS_PCM_SAMPLE_RATE_HZ as usize
        {
            if Instant::now()
                .saturating_duration_since(wait_started)
                .as_millis()
                >= TTS_PCM_BACKPRESSURE_TIMEOUT_MS
            {
                return Err(String::from("pcm-lane-backpressure-timeout"));
            }
            Timer::after(EmbassyDuration::from_millis(5)).await;
            continue;
        }
        let submitted = if spirit_talk {
            crate::aud::pcm_lane::submit_spirit_talk_i16_stereo_48k_if_generation(
                "lumen-tts",
                samples,
                stop_generation,
            )
        } else {
            crate::aud::pcm_lane::submit_i16_stereo_48k_if_generation(
                "shell2-ttstt",
                samples,
                stop_generation,
            )
        };
        match submitted {
            Ok(accepted_frames) => return Ok(accepted_frames),
            Err(crate::aud::pcm_lane::GuardedPcmLaneError::GenerationChanged(_samples)) => {
                return Err(String::from("stopped"));
            }
            Err(crate::aud::pcm_lane::GuardedPcmLaneError::Lane(
                crate::aud::pcm_lane::PcmLaneError::QueueFull,
                returned_samples,
            )) => {
                samples = returned_samples;
                if Instant::now()
                    .saturating_duration_since(wait_started)
                    .as_millis()
                    >= TTS_PCM_BACKPRESSURE_TIMEOUT_MS
                {
                    return Err(String::from("pcm-lane-backpressure-timeout"));
                }
                Timer::after(EmbassyDuration::from_millis(5)).await;
            }
            Err(crate::aud::pcm_lane::GuardedPcmLaneError::Lane(error, _samples)) => {
                return Err(alloc::format!("pcm-lane-{error:?}"));
            }
        }
    }
}

fn request_error(error: SpeechRequestError) -> String {
    match error {
        SpeechRequestError::BackendUnavailable => String::from("backend-unavailable"),
        SpeechRequestError::BackendWarming => String::from("backend-warming"),
        SpeechRequestError::InvalidRequest(reason) => alloc::format!("invalid-{reason}"),
        SpeechRequestError::BackendRejected(reason) => alloc::format!("backend-{reason}"),
        SpeechRequestError::Service(error) => alloc::format!("service-{error:?}"),
    }
}

fn tts_usage(io: &'static dyn ShellBackend2) {
    print_shell_line(
        io,
        "tts: usage `tts [voice=NAME] [speed=0.5..2.0] <text|\"quoted text\">` | `tts status` | `tts stop` | `tts capture next|off|status`; waiting_queue=8 serialized/asynchronous, model_chunk=510 phonemes after G2P, pcm=s16le/stereo/48k via live pcm_lane",
    );
}

fn stt_usage(io: &'static dyn ShellBackend2) {
    print_shell_line(
        io,
        "stt: usage `stt [file] <audio.wav> [language=CODE|auto] [translate]` | `stt status` | `stt record` (experimental)",
    );
}
