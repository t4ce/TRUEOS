//! Cooperative TTSTT model residency and CPU inference service.
//!
//! The host `ttstt` program uses ONNX Runtime and whisper.cpp. Those `std`
//! runtimes are not linked into the kernel. This module owns the kernel-side
//! boundary instead: the BSP loads the same model assets from TRUEOSFS once,
//! and AP2+ Embassy workers execute small, explicitly cooperative inference
//! slices against the resident images. A decoder/backend can implement
//! [`InferenceJob`] without putting filesystem I/O on an inference path.

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, AtomicUsize, Ordering};

use spin::Mutex;
use trueos_executor::{SpawnError, Spawner};
use trueos_time::{Duration as EmbassyDuration, Instant, Timer};

const MODEL_ROOT: &str = "models";
const WHISPER_MODEL_PATH: &str = "models/whisper/ggml-base.bin";
const KOKORO_DIR: &str = "models/kokoro";
const KOKORO_AOT_PATH: &str = "models/kokoro/kokoro.kkaot";
const KOKORO_ONNX_PREFERRED_PATH: &str = "models/kokoro/kokoro-quant-convinteger.onnx";
const KOKORO_VOICES_PATH: &str = "models/kokoro/voices-v1.0.bin";
const KOKORO_G2P_PATH: &str = "models/kokoro/en.g2p";
const KOKORO_LEXICON_PATH: &str = "models/kokoro/misaki-us.klex";

// Range reads keep both the device and the BSP executor moving. Model files
// are deliberately read once; inference jobs never reopen them.
const MODEL_READ_CHUNK_BYTES: usize = 256 * 1024;
const MODEL_READ_YIELD_MS: u64 = 1;
const MODEL_RETRY_MS: u64 = 2_000;
const MODEL_MISSING_RETRY_MS: u64 = 60_000;
const WORKER_IDLE_POLL_MS: u64 = 25;
const WORKER_SLICE_YIELD_MS: u64 = 1;
const MODEL_FILE_MAX_BYTES: u64 = 512 * 1024 * 1024;
const MODEL_SET_MAX_BYTES: usize = 768 * 1024 * 1024;
const JOB_QUEUE_CAP: usize = 64;
/// Match the proven host service: one Kokoro owner with eight requests waiting.
pub const TTS_QUEUE_DEPTH: usize = 8;
const TTS_MAX_OUTSTANDING: usize = TTS_QUEUE_DEPTH + 1;
/// Kokoro's 512-position input reserves the first and last positions for padding.
pub const KOKORO_MAX_PHONEMES: usize = 510;
/// PCM accepted by the existing live kernel playback lane.
pub const TTS_PCM_SAMPLE_RATE_HZ: u32 = 48_000;
pub const TTS_PCM_CHANNELS: usize = 2;
/// Bound every backend-to-consumer transfer to 250 ms of PCM.
pub const TTS_PCM_CHUNK_MAX_FRAMES: usize = 12_000;
/// Let synthesis run ahead by at most one second of finalized PCM.
pub const TTS_OUTPUT_QUEUE_DEPTH: usize = 4;
const WORKER_TASK_POOL: usize = crate::allcaps::hv::VM_CPU_SLOT_LIMIT;

static SERVICE_STATE: AtomicU8 = AtomicU8::new(ServiceState::WaitingForModels as u8);
static RESIDENT_BYTES: AtomicU64 = AtomicU64::new(0);
static WORKER_COUNT: AtomicU32 = AtomicU32::new(0);
static NEXT_JOB_ID: AtomicU64 = AtomicU64::new(1);
static OUTSTANDING_JOBS: AtomicUsize = AtomicUsize::new(0);
static OUTSTANDING_TTS_JOBS: AtomicUsize = AtomicUsize::new(0);
static ACTIVE_TTS_JOB_ID: AtomicU64 = AtomicU64::new(0);
static BACKEND_WARM_STARTED: AtomicBool = AtomicBool::new(false);
static SERVICE_TASK_STARTED: AtomicBool = AtomicBool::new(false);
static TTS_PCM_CHUNKS_EMITTED: AtomicU64 = AtomicU64::new(0);
static TTS_PCM_FRAMES_EMITTED: AtomicU64 = AtomicU64::new(0);
static TTS_PCM_CHUNKS_BUFFERED: AtomicUsize = AtomicUsize::new(0);
static TTS_PCM_FRAMES_BUFFERED: AtomicU64 = AtomicU64::new(0);
static TTS_STREAMS_COMPLETED: AtomicU64 = AtomicU64::new(0);
static TTS_STREAMS_FAILED: AtomicU64 = AtomicU64::new(0);

// Model images are loaded once and deliberately remain resident for the life
// of the kernel. Exposing that lifetime explicitly lets zero-copy AOT, voice,
// and G2P parsers retain validated views without self-referential ownership or
// unsafe lifetime extension. Worker-spawn retries reuse this same allocation.
static MODELS: Mutex<Option<&'static ModelSet>> = Mutex::new(None);
static JOBS: Mutex<VecDeque<QueuedJob>> = Mutex::new(VecDeque::new());
static JOB_WAIT: crate::wait::WaitQueue = crate::wait::WaitQueue::new();
static SPEECH_BACKEND: Mutex<Option<&'static dyn SpeechBackend>> = Mutex::new(None);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ServiceState {
    WaitingForModels = 0,
    LoadingModels = 1,
    ModelsResident = 2,
    Ready = 3,
}

impl ServiceState {
    fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::LoadingModels,
            2 => Self::ModelsResident,
            3 => Self::Ready,
            _ => Self::WaitingForModels,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServiceStatus {
    pub state: ServiceState,
    pub resident_bytes: u64,
    pub workers: u32,
    pub outstanding_jobs: usize,
    pub outstanding_tts_jobs: usize,
    pub queued_tts_jobs: usize,
    pub active_tts_job_id: Option<u64>,
    pub tts_pcm_chunks_emitted: u64,
    pub tts_pcm_frames_emitted: u64,
    pub tts_pcm_chunks_buffered: usize,
    pub tts_pcm_frames_buffered: u64,
    pub tts_streams_completed: u64,
    pub tts_streams_failed: u64,
}

pub fn status() -> ServiceStatus {
    let active_tts_job_id = ACTIVE_TTS_JOB_ID.load(Ordering::Acquire);
    let outstanding_tts_jobs = OUTSTANDING_TTS_JOBS.load(Ordering::Acquire);
    ServiceStatus {
        state: ServiceState::from_u8(SERVICE_STATE.load(Ordering::Acquire)),
        resident_bytes: RESIDENT_BYTES.load(Ordering::Acquire),
        workers: WORKER_COUNT.load(Ordering::Acquire),
        outstanding_jobs: OUTSTANDING_JOBS.load(Ordering::Acquire),
        outstanding_tts_jobs,
        queued_tts_jobs: outstanding_tts_jobs.saturating_sub(usize::from(active_tts_job_id != 0)),
        active_tts_job_id: (active_tts_job_id != 0).then_some(active_tts_job_id),
        tts_pcm_chunks_emitted: TTS_PCM_CHUNKS_EMITTED.load(Ordering::Acquire),
        tts_pcm_frames_emitted: TTS_PCM_FRAMES_EMITTED.load(Ordering::Acquire),
        tts_pcm_chunks_buffered: TTS_PCM_CHUNKS_BUFFERED.load(Ordering::Acquire),
        tts_pcm_frames_buffered: TTS_PCM_FRAMES_BUFFERED.load(Ordering::Acquire),
        tts_streams_completed: TTS_STREAMS_COMPLETED.load(Ordering::Acquire),
        tts_streams_failed: TTS_STREAMS_FAILED.load(Ordering::Acquire),
    }
}

/// Start the BSP-local model residency controller exactly once. Boot policy
/// and command-driven lazy startup share this claim so they cannot duplicate
/// the permanently resident model set.
pub fn ensure_service_started(spawner: Spawner) -> Result<bool, SpawnError> {
    if SERVICE_TASK_STARTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Ok(false);
    }

    match service_task() {
        Ok(token) => {
            spawner.spawn(token);
            Ok(true)
        }
        Err(error) => {
            SERVICE_TASK_STARTED.store(false, Ordering::Release);
            Err(error)
        }
    }
}

// The sealed Kokoro graph contains constant tensors whose proven alignment is
// as large as 64 bytes.  A `Vec<u8>` allocation only promises byte alignment,
// even when every on-disk section offset is correctly aligned.  Keep resident
// model images in the same aligned representation used by the native host
// oracle so zero-copy tensor views preserve the artifact's alignment proof.
#[repr(C, align(64))]
#[derive(Clone, Copy, Debug)]
struct ModelImageLine([u8; 64]);

const _: () = assert!(core::mem::size_of::<ModelImageLine>() == 64);

#[derive(Debug)]
struct ModelImageBytes {
    lines: Vec<ModelImageLine>,
    len: usize,
}

impl ModelImageBytes {
    fn try_zeroed(len: usize) -> Result<Self, ()> {
        let line_count = len.checked_add(63).ok_or(())? / 64;
        let mut lines = Vec::new();
        lines.try_reserve_exact(line_count).map_err(|_| ())?;
        lines.resize(line_count, ModelImageLine([0; 64]));
        Ok(Self { lines, len })
    }

    fn len(&self) -> usize {
        self.len
    }

    fn as_slice(&self) -> &[u8] {
        // SAFETY: ModelImageLine is a repr(C), 64-byte-aligned wrapper with
        // exactly 64 initialized bytes. `len` is bounded by the Vec storage.
        unsafe { core::slice::from_raw_parts(self.lines.as_ptr().cast::<u8>(), self.len) }
    }

    fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: the same representation proof as `as_slice` applies and the
        // exclusive borrow keeps the returned byte slice unique.
        unsafe { core::slice::from_raw_parts_mut(self.lines.as_mut_ptr().cast::<u8>(), self.len) }
    }
}

#[derive(Debug)]
pub struct ModelImage {
    path: String,
    bytes: ModelImageBytes,
}

impl ModelImage {
    pub fn path(&self) -> &str {
        self.path.as_str()
    }

    pub fn bytes(&self) -> &[u8] {
        self.bytes.as_slice()
    }
}

#[derive(Debug)]
pub struct ModelSet {
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    whisper: ModelImage,
    kokoro: ModelImage,
    kokoro_voices: ModelImage,
    kokoro_g2p: Option<ModelImage>,
    kokoro_lexicon: Option<ModelImage>,
    total_bytes: usize,
}

impl ModelSet {
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub fn whisper(&self) -> &ModelImage {
        &self.whisper
    }

    pub fn kokoro(&self) -> &ModelImage {
        &self.kokoro
    }

    pub fn kokoro_voices(&self) -> &ModelImage {
        &self.kokoro_voices
    }

    /// Optional compact English fallback frontend. Its absence keeps model
    /// residency/STT available but leaves the native Kokoro backend unready.
    pub fn kokoro_g2p(&self) -> Option<&ModelImage> {
        self.kokoro_g2p.as_ref()
    }

    /// Optional Kokoro/Misaki pronunciation overlay used ahead of G2P2.
    pub fn kokoro_lexicon(&self) -> Option<&ModelImage> {
        self.kokoro_lexicon.as_ref()
    }

    pub const fn total_bytes(&self) -> usize {
        self.total_bytes
    }
}

/// Borrow the immutable, permanently resident model set without filesystem I/O.
pub fn resident_models() -> Option<&'static ModelSet> {
    *MODELS.lock()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    Warmup,
    SpeechToText,
    TextToSpeech,
}

impl Direction {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Warmup => "warmup",
            Self::SpeechToText => "stt",
            Self::TextToSpeech => "tts",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JobProgress {
    /// The slice consumed its bounded budget and should be scheduled again.
    Pending,
    Complete,
    Failed(&'static str),
}

/// A decoder/inference request split into bounded CPU slices.
///
/// `run_slice` executes synchronously on an AP2+ worker. Implementations must
/// return regularly; the service yields and round-robins pending jobs between
/// calls. Results can be published through state owned by the job.
pub trait InferenceJob: Send {
    fn direction(&self) -> Direction;

    fn run_slice(&mut self, models: &'static ModelSet, worker: WorkerContext) -> JobProgress;
}

/// One finalized, bounded handoff from a native Kokoro job.
///
/// A model inference chunk contains at most [`KOKORO_MAX_PHONEMES`] phonemes.
/// Its waveform may be divided into several of these PCM chunks. Backends must
/// set `end_of_model_chunk` on the last PCM chunk produced by that inference.
#[derive(Debug)]
pub struct TtsAudioChunk {
    pub samples_i16_stereo_48k: Vec<i16>,
    pub model_chunk_index: u32,
    pub model_chunk_phonemes: u16,
    pub end_of_model_chunk: bool,
}

impl TtsAudioChunk {
    pub fn validate(&self) -> Result<usize, &'static str> {
        if self.samples_i16_stereo_48k.is_empty() {
            return Err("empty-pcm-chunk");
        }
        if !self
            .samples_i16_stereo_48k
            .len()
            .is_multiple_of(TTS_PCM_CHANNELS)
        {
            return Err("pcm-chunk-not-stereo");
        }
        let frames = self.samples_i16_stereo_48k.len() / TTS_PCM_CHANNELS;
        if frames > TTS_PCM_CHUNK_MAX_FRAMES {
            return Err("pcm-chunk-too-large");
        }
        if self.model_chunk_phonemes == 0
            || usize::from(self.model_chunk_phonemes) > KOKORO_MAX_PHONEMES
        {
            return Err("model-chunk-phonemes-out-of-range");
        }
        Ok(frames)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TtsSynthesisSummary {
    pub model_chunks: u32,
    pub pcm_chunks: u32,
    pub pcm_frames: u64,
}

#[derive(Debug)]
pub enum TtsOutputError {
    WouldBlock(TtsAudioChunk),
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    Closed(TtsAudioChunk),
    Invalid {
        reason: &'static str,
        #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
        chunk: TtsAudioChunk,
    },
}

struct TtsOutputState {
    chunks: Mutex<VecDeque<TtsAudioChunk>>,
    progress: Mutex<TtsOutputProgress>,
    completion: Mutex<Option<Result<TtsSynthesisSummary, &'static str>>>,
    service_timing: Mutex<Option<TtsServiceTimingState>>,
    closed: AtomicBool,
    cancelled: AtomicBool,
    wait: crate::wait::WaitQueue,
}

#[derive(Clone, Copy, Debug)]
struct TtsServiceTimingState {
    submitted_at: Instant,
    first_run_at: Option<Instant>,
}

#[derive(Default)]
struct TtsOutputProgress {
    summary: TtsSynthesisSummary,
    next_model_chunk_index: u32,
    open_model_chunk_phonemes: Option<u16>,
}

impl TtsOutputState {
    fn new() -> Self {
        Self {
            chunks: Mutex::new(VecDeque::new()),
            progress: Mutex::new(TtsOutputProgress::default()),
            completion: Mutex::new(None),
            service_timing: Mutex::new(None),
            closed: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            wait: crate::wait::WaitQueue::new(),
        }
    }
}

impl Drop for TtsOutputState {
    fn drop(&mut self) {
        // Normally the sole TtsStream drains this queue. Also account for a
        // backend factory that fails after emitting prematurely, so global
        // observability cannot retain phantom buffered audio.
        let chunks = self.chunks.lock();
        let chunk_count = chunks.len();
        let frames = chunks
            .iter()
            .map(|chunk| chunk.samples_i16_stereo_48k.len() / TTS_PCM_CHANNELS)
            .sum::<usize>();
        if chunk_count != 0 {
            TTS_PCM_CHUNKS_BUFFERED.fetch_sub(chunk_count, Ordering::AcqRel);
            TTS_PCM_FRAMES_BUFFERED.fetch_sub(frames as u64, Ordering::AcqRel);
        }
    }
}

/// Nonblocking output side held by one native Kokoro inference job.
///
/// A backend retains a chunk when [`Self::try_push`] returns `WouldBlock`,
/// returns `Pending` from `InferenceJob::run_slice`, and retries that same
/// chunk later. It must not silently discard or reorder output.
#[derive(Clone)]
pub struct TtsOutput {
    state: Arc<TtsOutputState>,
}

impl TtsOutput {
    fn mark_service_submitted(&self, submitted_at: Instant) {
        let mut timing = self.state.service_timing.lock();
        if timing.is_none() {
            *timing = Some(TtsServiceTimingState {
                submitted_at,
                first_run_at: None,
            });
        }
    }

    fn mark_service_first_run(&self, first_run_at: Instant) {
        let mut timing = self.state.service_timing.lock();
        if let Some(timing) = timing.as_mut()
            && timing.first_run_at.is_none()
        {
            timing.first_run_at = Some(first_run_at);
        }
    }

    pub fn try_push(&self, chunk: TtsAudioChunk) -> Result<(), TtsOutputError> {
        let frames = match chunk.validate() {
            Ok(frames) => frames,
            Err(reason) => return Err(TtsOutputError::Invalid { reason, chunk }),
        };
        if self.state.closed.load(Ordering::Acquire) || self.state.cancelled.load(Ordering::Acquire)
        {
            return Err(TtsOutputError::Closed(chunk));
        }
        let mut chunks = self.state.chunks.lock();
        if chunks.len() >= TTS_OUTPUT_QUEUE_DEPTH {
            return Err(TtsOutputError::WouldBlock(chunk));
        }
        // Recheck under the queue lock so finish/cancel cannot accept data
        // after publishing a terminal state.
        if self.state.closed.load(Ordering::Acquire) || self.state.cancelled.load(Ordering::Acquire)
        {
            return Err(TtsOutputError::Closed(chunk));
        }
        let mut progress = self.state.progress.lock();
        if chunk.model_chunk_index != progress.next_model_chunk_index {
            return Err(TtsOutputError::Invalid {
                reason: "model-chunk-index-out-of-order",
                chunk,
            });
        }
        match progress.open_model_chunk_phonemes {
            Some(phonemes) if phonemes != chunk.model_chunk_phonemes => {
                return Err(TtsOutputError::Invalid {
                    reason: "model-chunk-phoneme-count-changed",
                    chunk,
                });
            }
            None => progress.open_model_chunk_phonemes = Some(chunk.model_chunk_phonemes),
            Some(_) => {}
        }
        progress.summary.pcm_chunks = progress.summary.pcm_chunks.saturating_add(1);
        progress.summary.pcm_frames = progress.summary.pcm_frames.saturating_add(frames as u64);
        if chunk.end_of_model_chunk {
            progress.summary.model_chunks = progress.summary.model_chunks.saturating_add(1);
            progress.next_model_chunk_index = progress.next_model_chunk_index.saturating_add(1);
            progress.open_model_chunk_phonemes = None;
        }
        chunks.push_back(chunk);
        drop(progress);
        TTS_PCM_CHUNKS_EMITTED.fetch_add(1, Ordering::Relaxed);
        TTS_PCM_FRAMES_EMITTED.fetch_add(frames as u64, Ordering::Relaxed);
        // Publish buffered counters before releasing the queue lock. A fast
        // consumer must never decrement them before the producer increments.
        TTS_PCM_CHUNKS_BUFFERED.fetch_add(1, Ordering::AcqRel);
        TTS_PCM_FRAMES_BUFFERED.fetch_add(frames as u64, Ordering::AcqRel);
        drop(chunks);
        self.state.wait.notify_one();
        Ok(())
    }

    /// Publish the sole terminal result after all PCM chunks were accepted.
    fn finish(&self, result: Result<TtsSynthesisSummary, &'static str>) -> bool {
        // Serialize the terminal transition with the final queue push. This
        // makes the closed recheck in `try_push` a real admission boundary.
        let chunks = self.state.chunks.lock();
        if self
            .state
            .closed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return false;
        }
        let result = match (self.state.cancelled.load(Ordering::Acquire), result) {
            (true, Ok(_)) => Err("tts-stream-cancelled"),
            (_, Ok(summary)) => {
                let progress = self.state.progress.lock();
                if progress.open_model_chunk_phonemes.is_some() {
                    Err("backend-finished-mid-model-chunk")
                } else if progress.summary.pcm_chunks == 0 {
                    Err("backend-finished-without-pcm")
                } else if summary != progress.summary {
                    Err("backend-tts-summary-mismatch")
                } else {
                    Ok(summary)
                }
            }
            (_, Err(reason)) => Err(reason),
        };
        if result.is_ok() {
            TTS_STREAMS_COMPLETED.fetch_add(1, Ordering::Relaxed);
        } else {
            TTS_STREAMS_FAILED.fetch_add(1, Ordering::Relaxed);
        }
        *self.state.completion.lock() = Some(result);
        drop(chunks);
        self.state.wait.notify_all();
        true
    }

    /// Close a well-formed stream and publish the service-verified counters.
    pub fn finish_success(&self) -> bool {
        let summary = self.state.progress.lock().summary;
        self.finish(Ok(summary))
    }

    pub fn finish_error(&self, reason: &'static str) -> bool {
        self.finish(Err(reason))
    }

    pub fn cancelled(&self) -> bool {
        self.state.cancelled.load(Ordering::Acquire)
    }

    fn finished(&self) -> bool {
        self.state.closed.load(Ordering::Acquire)
    }

    fn ensure_finished(&self, result: Result<TtsSynthesisSummary, &'static str>) {
        let _ = self.finish(result);
    }
}

/// Consumer side returned to shell2 immediately after a TTS job is admitted.
pub struct TtsStream {
    state: Arc<TtsOutputState>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TtsServiceDispatchTiming {
    /// Admission into the shared service queue to the first actual `run_slice`.
    pub initial_queue_us: u64,
    /// First actual `run_slice` to the caller's observation timestamp.
    pub dispatch_elapsed_us: u64,
    /// Shared service queue admission to the caller's observation timestamp.
    pub submit_elapsed_us: u64,
}

pub enum TtsStreamEvent {
    Chunk(TtsAudioChunk),
    Finished(Result<TtsSynthesisSummary, &'static str>),
}

impl TtsStream {
    /// Snapshot timing relative to an event observed by the stream consumer.
    ///
    /// This becomes available immediately before the job's first real
    /// `run_slice`; merely selecting, cancelling, or closing a queued job does
    /// not count as dispatch.
    pub fn service_timing_at(&self, observed_at: Instant) -> Option<TtsServiceDispatchTiming> {
        let timing = (*self.state.service_timing.lock())?;
        let first_run_at = timing.first_run_at?;
        Some(TtsServiceDispatchTiming {
            initial_queue_us: first_run_at
                .saturating_duration_since(timing.submitted_at)
                .as_micros(),
            dispatch_elapsed_us: observed_at
                .saturating_duration_since(first_run_at)
                .as_micros(),
            submit_elapsed_us: observed_at
                .saturating_duration_since(timing.submitted_at)
                .as_micros(),
        })
    }

    pub fn try_next(&self) -> Option<TtsStreamEvent> {
        let chunk = { self.state.chunks.lock().pop_front() };
        if let Some(chunk) = chunk {
            let frames = chunk.samples_i16_stereo_48k.len() / TTS_PCM_CHANNELS;
            TTS_PCM_CHUNKS_BUFFERED.fetch_sub(1, Ordering::AcqRel);
            TTS_PCM_FRAMES_BUFFERED.fetch_sub(frames as u64, Ordering::AcqRel);
            // Wake a producer that retained a chunk after WouldBlock.
            self.state.wait.notify_one();
            return Some(TtsStreamEvent::Chunk(chunk));
        }
        self.state
            .completion
            .lock()
            .take()
            .map(TtsStreamEvent::Finished)
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub async fn next(&self) -> TtsStreamEvent {
        loop {
            if let Some(event) = self.try_next() {
                return event;
            }
            self.state.wait.wait_for_event_timeout(25).await;
        }
    }

    pub async fn wait_for_event_timeout(&self, timeout_ms: u64) -> bool {
        self.state.wait.wait_for_event_timeout(timeout_ms).await
    }

    /// Consumer cancellation is cooperative. It closes the chunk sink, but a
    /// backend must observe `TtsOutput::cancelled` and finish its job promptly.
    pub fn cancel(&self) {
        // Share the producer's queue lock so cancel and finish have one
        // deterministic ordering. If finish linearized first, its terminal
        // result is immutable; otherwise finish observes cancellation.
        let chunks = self.state.chunks.lock();
        if self.state.closed.load(Ordering::Acquire) {
            return;
        }
        self.state.cancelled.store(true, Ordering::Release);
        drop(chunks);
        self.state.wait.notify_all();
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub fn pending_chunks(&self) -> usize {
        self.state.chunks.lock().len()
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub fn pending_frames(&self) -> usize {
        self.state
            .chunks
            .lock()
            .iter()
            .map(|chunk| chunk.samples_i16_stereo_48k.len() / TTS_PCM_CHANNELS)
            .sum()
    }
}

impl Drop for TtsStream {
    fn drop(&mut self) {
        self.cancel();
        let mut chunks = self.state.chunks.lock();
        let chunk_count = chunks.len();
        let frames = chunks
            .iter()
            .map(|chunk| chunk.samples_i16_stereo_48k.len() / TTS_PCM_CHANNELS)
            .sum::<usize>();
        chunks.clear();
        drop(chunks);
        if chunk_count != 0 {
            TTS_PCM_CHUNKS_BUFFERED.fetch_sub(chunk_count, Ordering::AcqRel);
            TTS_PCM_FRAMES_BUFFERED.fetch_sub(frames as u64, Ordering::AcqRel);
        }
    }
}

pub struct TtsSubmission {
    pub id: u64,
    pub stream: TtsStream,
}

pub type SttCompletion = Box<dyn FnOnce(Result<String, &'static str>) + Send + 'static>;

pub struct TtsRequest {
    pub text: String,
    pub voice: String,
    pub speed: f32,
    pub(crate) capture: Option<crate::ai::ttstt_capture::CaptureSession>,
}

/// Request delivered to the native backend after service admission.
pub struct BackendTtsRequest {
    pub request: TtsRequest,
    pub output: TtsOutput,
}

pub struct SttRequest {
    pub pcm_f32_mono_16k: Vec<f32>,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub language: Option<String>,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub translate: bool,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub complete: SttCompletion,
}

/// Native ONNX/GGML adapters register here once. Factory methods must only
/// construct cooperative jobs; model bytes come from the `ModelSet` supplied
/// to each `run_slice`, never from a filesystem read in the request path.
pub trait SpeechBackend: Send + Sync {
    fn name(&self) -> &'static str;

    /// True only after model parsing and backend state construction completed.
    fn ready(&self) -> bool;

    /// Directional readiness lets the TTS-first native port become truthful
    /// without claiming that its later Whisper adapter is already available.
    fn tts_ready(&self) -> bool {
        self.ready()
    }

    fn stt_ready(&self) -> bool {
        self.ready()
    }

    /// A terminal warm failure for the permanently resident model set.
    ///
    /// `None` means that warming may still start or retry. Returning a reason
    /// stops automatic retries until reboot/model replacement and lets shell2
    /// distinguish a rejected asset from a backend that is merely warming.
    fn warm_failure_reason(&self) -> Option<&'static str> {
        None
    }

    /// Construct a bounded-slice warm job for both resident model images.
    fn create_warm_job(&self) -> Result<Box<dyn InferenceJob>, &'static str>;

    fn create_tts_job(
        &self,
        request: BackendTtsRequest,
    ) -> Result<Box<dyn InferenceJob>, &'static str>;

    fn create_stt_job(&self, request: SttRequest) -> Result<Box<dyn InferenceJob>, &'static str>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpeechRequestError {
    BackendUnavailable,
    BackendWarming,
    InvalidRequest(&'static str),
    BackendRejected(&'static str),
    Service(SubmitError),
}

pub fn install_speech_backend(backend: &'static dyn SpeechBackend) -> bool {
    let mut installed = SPEECH_BACKEND.lock();
    if installed.is_some() {
        return false;
    }
    crate::log_info!(target: "ttstt"; "ttstt: speech backend installed name={}\n", backend.name());
    *installed = Some(backend);
    drop(installed);
    let _ = ensure_backend_warm_started();
    true
}

pub fn speech_backend_name() -> Option<&'static str> {
    (*SPEECH_BACKEND.lock()).map(SpeechBackend::name)
}

pub fn speech_backend_ready() -> bool {
    let backend = *SPEECH_BACKEND.lock();
    backend.is_some_and(SpeechBackend::ready)
}

pub fn speech_backend_warm_failure_reason() -> Option<&'static str> {
    let backend = *SPEECH_BACKEND.lock();
    backend.and_then(SpeechBackend::warm_failure_reason)
}

pub fn tts_backend_ready() -> bool {
    let backend = *SPEECH_BACKEND.lock();
    backend.is_some_and(SpeechBackend::tts_ready)
}

pub fn stt_backend_ready() -> bool {
    let backend = *SPEECH_BACKEND.lock();
    backend.is_some_and(SpeechBackend::stt_ready)
}

fn ensure_backend_warm_started() -> bool {
    let Some(backend) = *SPEECH_BACKEND.lock() else {
        return false;
    };
    if backend.ready() {
        return true;
    }
    if backend.warm_failure_reason().is_some() {
        return false;
    }
    if ServiceState::from_u8(SERVICE_STATE.load(Ordering::Acquire)) != ServiceState::Ready {
        return false;
    }
    if BACKEND_WARM_STARTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return false;
    }
    let job = match backend.create_warm_job() {
        Ok(job) => job,
        Err(reason) => {
            BACKEND_WARM_STARTED.store(false, Ordering::Release);
            crate::log_warn!(
                target: "ttstt";
                "ttstt: backend warm factory deferred name={} reason={}\n",
                backend.name(),
                reason
            );
            return false;
        }
    };
    if job.direction() != Direction::Warmup {
        BACKEND_WARM_STARTED.store(false, Ordering::Release);
        crate::log_warn!(
            target: "ttstt";
            "ttstt: backend warm factory returned non-warm job name={}\n",
            backend.name()
        );
        return false;
    }
    match submit_with_completion(job, JobCompletion::None, Direction::Warmup) {
        Ok(id) => {
            crate::log_info!(
                target: "ttstt";
                "ttstt: backend warm queued name={} id={}\n",
                backend.name(),
                id
            );
            true
        }
        Err(error) => {
            BACKEND_WARM_STARTED.store(false, Ordering::Release);
            crate::log_warn!(
                target: "ttstt";
                "ttstt: backend warm queue deferred name={} error={:?}\n",
                backend.name(),
                error
            );
            false
        }
    }
}

pub fn submit_tts(request: TtsRequest) -> Result<TtsSubmission, SpeechRequestError> {
    if ServiceState::from_u8(SERVICE_STATE.load(Ordering::Acquire)) != ServiceState::Ready {
        return Err(SpeechRequestError::Service(SubmitError::NotReady));
    }
    if request.text.trim().is_empty() {
        return Err(SpeechRequestError::InvalidRequest("empty-text"));
    }
    if request.voice.trim().is_empty() {
        return Err(SpeechRequestError::InvalidRequest("empty-voice"));
    }
    if !request.speed.is_finite() || !(0.5..=2.0).contains(&request.speed) {
        return Err(SpeechRequestError::InvalidRequest("speed-out-of-range"));
    }
    let backend = (*SPEECH_BACKEND.lock()).ok_or(SpeechRequestError::BackendUnavailable)?;
    if !backend.tts_ready() {
        if let Some(reason) = backend.warm_failure_reason() {
            return Err(SpeechRequestError::BackendRejected(reason));
        }
        let _ = ensure_backend_warm_started();
        return Err(SpeechRequestError::BackendWarming);
    }
    let state = Arc::new(TtsOutputState::new());
    let output = TtsOutput {
        state: state.clone(),
    };
    let job = backend
        .create_tts_job(BackendTtsRequest {
            request,
            output: output.clone(),
        })
        .map_err(SpeechRequestError::BackendRejected)?;
    let direction = job.direction();
    if direction != Direction::TextToSpeech {
        return Err(SpeechRequestError::BackendRejected("tts-factory-returned-wrong-direction"));
    }
    let id = submit_with_completion(job, JobCompletion::Tts(output), direction)
        .map_err(SpeechRequestError::Service)?;
    Ok(TtsSubmission {
        id,
        stream: TtsStream { state },
    })
}

pub fn submit_stt(request: SttRequest) -> Result<u64, SpeechRequestError> {
    if ServiceState::from_u8(SERVICE_STATE.load(Ordering::Acquire)) != ServiceState::Ready {
        return Err(SpeechRequestError::Service(SubmitError::NotReady));
    }
    if request.pcm_f32_mono_16k.is_empty() {
        return Err(SpeechRequestError::InvalidRequest("empty-pcm"));
    }
    let backend = (*SPEECH_BACKEND.lock()).ok_or(SpeechRequestError::BackendUnavailable)?;
    if !backend.stt_ready() {
        if let Some(reason) = backend.warm_failure_reason() {
            return Err(SpeechRequestError::BackendRejected(reason));
        }
        let _ = ensure_backend_warm_started();
        return Err(SpeechRequestError::BackendWarming);
    }
    let job = backend
        .create_stt_job(request)
        .map_err(SpeechRequestError::BackendRejected)?;
    let direction = job.direction();
    if direction != Direction::SpeechToText {
        return Err(SpeechRequestError::BackendRejected("stt-factory-returned-wrong-direction"));
    }
    submit_with_completion(job, JobCompletion::None, direction).map_err(SpeechRequestError::Service)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KernelLane {
    Scalar,
    Avx2,
    AvxVnni,
}

impl KernelLane {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Scalar => "scalar",
            Self::Avx2 => "avx2",
            Self::AvxVnni => "avx-vnni",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Bf16KernelLane {
    Scalar,
    Sse2,
    Avx2Fma,
}

impl Bf16KernelLane {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Scalar => "scalar",
            Self::Sse2 => "sse2",
            Self::Avx2Fma => "avx2-fma",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub enum KernelError {
    InvalidShape,
}

#[derive(Clone, Copy, Debug)]
pub struct WorkerContext {
    pub slot: u32,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub core_kind: u8,
    /// Best `u8 x i8` Kokoro lane detected on this worker's current CPU.
    pub q8_lane: KernelLane,
    /// Per-current-CPU dispatcher; safe entry points recheck lane support.
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub q8_dispatcher: trueos_ttstt_cpu::Dispatcher,
    pub bf16_lane: Bf16KernelLane,
}

impl WorkerContext {
    /// Execute the existing TRUEOS BF16 matvec dispatch. On x86 this selects
    /// AVX2/FMA when enabled, with the established SSE2 fallback.
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub fn matvec_bf16(
        self,
        x: &[f32],
        weights_rowmajor_bf16: &[u8],
        rows: usize,
        k_dim: usize,
        out: &mut [f32],
        row_start: usize,
        row_end: usize,
    ) -> Result<Bf16KernelLane, KernelError> {
        crate::turbo::avx2_fma_sse2_help::matvec_rowmajor_bf16_dispatch(
            x,
            weights_rowmajor_bf16,
            rows,
            k_dim,
            out,
            row_start,
            row_end,
        )
        .map(map_bf16_kernel_lane)
        .map_err(|_| KernelError::InvalidShape)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubmitError {
    NotReady,
    QueueFull,
    TtsOutputRequired,
}

struct QueuedJob {
    id: u64,
    /// Cache the admitted direction. `InferenceJob::direction` is not an
    /// immutable associated value, so consulting an implementation again
    /// would let a stateful job evade TTS serialization and counter cleanup.
    direction: Direction,
    job: Box<dyn InferenceJob>,
    completion: JobCompletion,
    submitted_at: Instant,
    first_run_at: Option<Instant>,
    run_slices: u64,
    run_us: u64,
}

enum JobCompletion {
    None,
    Tts(TtsOutput),
}

/// Queue a cooperative warmup or STT job on the resident AP2+ worker pool.
/// TTS must enter through [`submit_tts`] so its bounded stream guard is present.
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub fn submit(job: Box<dyn InferenceJob>) -> Result<u64, SubmitError> {
    let direction = job.direction();
    submit_with_completion(job, JobCompletion::None, direction)
}

fn submit_with_completion(
    job: Box<dyn InferenceJob>,
    completion: JobCompletion,
    direction: Direction,
) -> Result<u64, SubmitError> {
    if ServiceState::from_u8(SERVICE_STATE.load(Ordering::Acquire)) != ServiceState::Ready {
        return Err(SubmitError::NotReady);
    }

    if direction == Direction::TextToSpeech && !matches!(&completion, JobCompletion::Tts(_)) {
        return Err(SubmitError::TtsOutputRequired);
    }
    if direction == Direction::TextToSpeech {
        OUTSTANDING_TTS_JOBS
            .try_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                (current < TTS_MAX_OUTSTANDING).then_some(current + 1)
            })
            .map_err(|_| SubmitError::QueueFull)?;
    }

    if OUTSTANDING_JOBS
        .try_update(Ordering::AcqRel, Ordering::Acquire, |current| {
            (current < JOB_QUEUE_CAP).then_some(current + 1)
        })
        .is_err()
    {
        if direction == Direction::TextToSpeech {
            OUTSTANDING_TTS_JOBS.fetch_sub(1, Ordering::AcqRel);
        }
        return Err(SubmitError::QueueFull);
    }

    let id = NEXT_JOB_ID.fetch_add(1, Ordering::Relaxed);
    let mut jobs = JOBS.lock();
    // Holding the queue lock makes this timestamp the admission
    // linearization point: a worker cannot select the job before its timing is
    // visible to the TTS stream.
    let submitted_at = Instant::now();
    if let JobCompletion::Tts(output) = &completion {
        output.mark_service_submitted(submitted_at);
    }
    jobs.push_back(QueuedJob {
        id,
        direction,
        job,
        completion,
        submitted_at,
        first_run_at: None,
        run_slices: 0,
        run_us: 0,
    });
    drop(jobs);
    JOB_WAIT.notify_one();
    Ok(id)
}

#[derive(Debug)]
enum ModelLoadError {
    Missing(String),
    Empty(String),
    TooLarge {
        path: String,
        bytes: u64,
    },
    SetTooLarge(usize),
    Allocation(String),
    ShortRead {
        path: String,
        offset: usize,
    },
    Changed {
        path: String,
        expected: u64,
        actual: u64,
    },
    AmbiguousKokoroModel(String),
    FileSystem {
        path: String,
        error: crate::disc::block::Error,
    },
}

impl ModelLoadError {
    fn retry_ms(&self) -> u64 {
        match self {
            Self::FileSystem {
                error: crate::disc::block::Error::NotReady,
                ..
            } => 250,
            Self::Missing(_) => MODEL_MISSING_RETRY_MS,
            _ => MODEL_RETRY_MS,
        }
    }

    fn describe(&self) -> String {
        match self {
            Self::Missing(path) => alloc::format!("missing trueosfs:/{path}"),
            Self::Empty(path) => alloc::format!("empty trueosfs:/{path}"),
            Self::TooLarge { path, bytes } => alloc::format!(
                "trueosfs:/{path} is {bytes} bytes (per-file cap {MODEL_FILE_MAX_BYTES})"
            ),
            Self::SetTooLarge(bytes) => {
                alloc::format!("model set is {bytes} bytes (resident cap {MODEL_SET_MAX_BYTES})")
            }
            Self::Allocation(path) => {
                alloc::format!("cannot reserve resident memory for trueosfs:/{path}")
            }
            Self::ShortRead { path, offset } => {
                alloc::format!("short read at trueosfs:/{path}+{offset}")
            }
            Self::Changed {
                path,
                expected,
                actual,
            } => alloc::format!(
                "trueosfs:/{path} changed while warming (expected {expected} bytes, found {actual})"
            ),
            Self::AmbiguousKokoroModel(listing) => alloc::format!(
                "expected one .onnx file below trueosfs:/{KOKORO_DIR}; found {listing}"
            ),
            Self::FileSystem { path, error } => {
                alloc::format!("trueosfs:/{path}: {error:?}")
            }
        }
    }
}

fn map_bf16_kernel_lane(lane: crate::turbo::avx2_fma_sse2_help::Bf16MatvecLane) -> Bf16KernelLane {
    use crate::turbo::avx2_fma_sse2_help::Bf16MatvecLane;
    match lane {
        Bf16MatvecLane::Scalar => Bf16KernelLane::Scalar,
        Bf16MatvecLane::Sse2 => Bf16KernelLane::Sse2,
        Bf16MatvecLane::Avx2Fma => Bf16KernelLane::Avx2Fma,
    }
}

fn map_q8_kernel_lane(lane: trueos_ttstt_cpu::Lane) -> KernelLane {
    match lane {
        trueos_ttstt_cpu::Lane::Scalar => KernelLane::Scalar,
        trueos_ttstt_cpu::Lane::Avx2 => KernelLane::Avx2,
        trueos_ttstt_cpu::Lane::AvxVnni => KernelLane::AvxVnni,
    }
}

async fn find_kokoro_model_path(
    disk: crate::disc::block::DeviceHandle,
) -> Result<String, ModelLoadError> {
    // The sealed native artifact owns its prepacked constants and is the only
    // inference image the kernel backend needs. Retain ONNX discovery as a
    // migration fallback while existing TRUEOSFS installations are updated;
    // an ONNX image can be resident but cannot make the native backend ready.
    match crate::r::fs::trueosfs::file_info_async(disk, KOKORO_AOT_PATH).await {
        Ok(Some(_)) => return Ok(KOKORO_AOT_PATH.to_string()),
        Ok(None) => {}
        Err(error) => {
            return Err(ModelLoadError::FileSystem {
                path: KOKORO_AOT_PATH.to_string(),
                error,
            });
        }
    }

    match crate::r::fs::trueosfs::file_info_async(disk, KOKORO_ONNX_PREFERRED_PATH).await {
        Ok(Some(_)) => return Ok(KOKORO_ONNX_PREFERRED_PATH.to_string()),
        Ok(None) => {}
        Err(error) => {
            return Err(ModelLoadError::FileSystem {
                path: KOKORO_ONNX_PREFERRED_PATH.to_string(),
                error,
            });
        }
    }

    let listing = crate::r::fs::trueosfs::list_dir_async(disk, KOKORO_DIR)
        .await
        .map_err(|error| ModelLoadError::FileSystem {
            path: KOKORO_DIR.to_string(),
            error,
        })?
        .ok_or_else(|| ModelLoadError::Missing(KOKORO_DIR.to_string()))?;
    let found: Vec<&str> = listing
        .entries
        .iter()
        .filter(|entry| {
            entry.kind == crate::r::fs::trueosfs::NodeKind::File && entry.name.ends_with(".onnx")
        })
        .map(|entry| entry.name.as_str())
        .collect();
    let Some(name) = found.first().copied() else {
        return Err(ModelLoadError::Missing(alloc::format!("{KOKORO_DIR}/*.onnx")));
    };
    if found.len() > 1 || listing.truncated {
        return Err(ModelLoadError::AmbiguousKokoroModel(found.join("\n")));
    }
    Ok(alloc::format!("{KOKORO_DIR}/{name}"))
}

async fn load_model_image(
    disk: crate::disc::block::DeviceHandle,
    path: String,
    expected_len: u64,
) -> Result<ModelImage, ModelLoadError> {
    let handle = crate::r::fs::trueosfs::file_read_open_async(disk, path.as_str())
        .await
        .map_err(|error| ModelLoadError::FileSystem {
            path: path.clone(),
            error,
        })?
        .ok_or_else(|| ModelLoadError::Missing(path.clone()))?;
    let data_len = handle.data_len();
    if data_len != expected_len {
        return Err(ModelLoadError::Changed {
            path,
            expected: expected_len,
            actual: data_len,
        });
    }
    if data_len == 0 {
        return Err(ModelLoadError::Empty(path));
    }
    if data_len > MODEL_FILE_MAX_BYTES {
        return Err(ModelLoadError::TooLarge {
            path,
            bytes: data_len,
        });
    }
    let len = usize::try_from(data_len).map_err(|_| ModelLoadError::TooLarge {
        path: path.clone(),
        bytes: data_len,
    })?;

    let mut bytes =
        ModelImageBytes::try_zeroed(len).map_err(|_| ModelLoadError::Allocation(path.clone()))?;
    let mut scratch = vec![0u8; MODEL_READ_CHUNK_BYTES.min(len)];
    let mut offset = 0usize;
    while offset < len {
        let want = (len - offset).min(scratch.len());
        let got = crate::r::fs::trueosfs::file_read_handle_range_async(
            handle,
            offset as u64,
            &mut scratch[..want],
        )
        .await
        .map_err(|error| ModelLoadError::FileSystem {
            path: path.clone(),
            error,
        })?
        .ok_or_else(|| ModelLoadError::ShortRead {
            path: path.clone(),
            offset,
        })?;
        if got != want {
            return Err(ModelLoadError::ShortRead { path, offset });
        }
        bytes.as_mut_slice()[offset..offset + got].copy_from_slice(&scratch[..got]);
        offset += got;
        Timer::after(EmbassyDuration::from_millis(MODEL_READ_YIELD_MS)).await;
    }

    crate::log_info!(
        target: "ttstt";
        "ttstt: model resident path=trueosfs:/{} bytes={}\n",
        path,
        bytes.len()
    );
    Ok(ModelImage { path, bytes })
}

async fn preflight_model_image(
    disk: crate::disc::block::DeviceHandle,
    path: &str,
) -> Result<u64, ModelLoadError> {
    let info = crate::r::fs::trueosfs::file_info_async(disk, path)
        .await
        .map_err(|error| ModelLoadError::FileSystem {
            path: path.to_string(),
            error,
        })?
        .ok_or_else(|| ModelLoadError::Missing(path.to_string()))?;
    if info.data_len == 0 {
        return Err(ModelLoadError::Empty(path.to_string()));
    }
    if info.data_len > MODEL_FILE_MAX_BYTES {
        return Err(ModelLoadError::TooLarge {
            path: path.to_string(),
            bytes: info.data_len,
        });
    }
    Ok(info.data_len)
}

async fn preflight_optional_model_image(
    disk: crate::disc::block::DeviceHandle,
    path: &str,
) -> Result<Option<u64>, ModelLoadError> {
    let Some(info) = crate::r::fs::trueosfs::file_info_async(disk, path)
        .await
        .map_err(|error| ModelLoadError::FileSystem {
            path: path.to_string(),
            error,
        })?
    else {
        return Ok(None);
    };
    if info.data_len == 0 {
        return Err(ModelLoadError::Empty(path.to_string()));
    }
    if info.data_len > MODEL_FILE_MAX_BYTES {
        return Err(ModelLoadError::TooLarge {
            path: path.to_string(),
            bytes: info.data_len,
        });
    }
    Ok(Some(info.data_len))
}

async fn load_model_set() -> Result<Box<ModelSet>, ModelLoadError> {
    let disk = crate::r::fs::trueosfs::primary_root_handle()
        .ok_or_else(|| ModelLoadError::Missing(MODEL_ROOT.to_string()))?;
    let kokoro_path = find_kokoro_model_path(disk).await?;

    // Validate the complete set before the first bulk read. A missing last
    // asset must not turn retries into repeated reads of the earlier models.
    let whisper_len = preflight_model_image(disk, WHISPER_MODEL_PATH).await?;
    let kokoro_len = preflight_model_image(disk, kokoro_path.as_str()).await?;
    let voices_len = preflight_model_image(disk, KOKORO_VOICES_PATH).await?;
    let g2p_len = preflight_optional_model_image(disk, KOKORO_G2P_PATH).await?;
    let lexicon_len = preflight_optional_model_image(disk, KOKORO_LEXICON_PATH).await?;
    let total_bytes_u64 = whisper_len
        .checked_add(kokoro_len)
        .and_then(|bytes| bytes.checked_add(voices_len))
        .and_then(|bytes| bytes.checked_add(g2p_len.unwrap_or(0)))
        .and_then(|bytes| bytes.checked_add(lexicon_len.unwrap_or(0)))
        .ok_or(ModelLoadError::SetTooLarge(usize::MAX))?;
    let total_bytes =
        usize::try_from(total_bytes_u64).map_err(|_| ModelLoadError::SetTooLarge(usize::MAX))?;
    if total_bytes > MODEL_SET_MAX_BYTES {
        return Err(ModelLoadError::SetTooLarge(total_bytes));
    }

    // Sequential reads intentionally limit boot-time device pressure and peak
    // scratch memory. Every range read is async and every chunk yields.
    let whisper = load_model_image(disk, WHISPER_MODEL_PATH.to_string(), whisper_len).await?;
    let kokoro = load_model_image(disk, kokoro_path, kokoro_len).await?;
    let kokoro_voices = load_model_image(disk, KOKORO_VOICES_PATH.to_string(), voices_len).await?;
    let kokoro_g2p = match g2p_len {
        Some(len) => Some(load_model_image(disk, KOKORO_G2P_PATH.to_string(), len).await?),
        None => None,
    };
    let kokoro_lexicon = match lexicon_len {
        Some(len) => Some(load_model_image(disk, KOKORO_LEXICON_PATH.to_string(), len).await?),
        None => None,
    };

    Ok(Box::new(ModelSet {
        whisper,
        kokoro,
        kokoro_voices,
        kokoro_g2p,
        kokoro_lexicon,
        total_bytes,
    }))
}

async fn eligible_worker_slots() -> Vec<u32> {
    loop {
        // Wait for the topology to settle so the pool gets every eligible
        // P-core rather than whichever P-core happened to register first.
        // This also prevents an early fallback to an E/unknown lane.
        if crate::workers::all_topology_spawners_registered() {
            let background = crate::workers::background_worker_slots();
            let perf: Vec<u32> = background
                .iter()
                .copied()
                .filter(|slot| {
                    crate::workers::core_kind_for_slot(*slot) == crate::workers::CORE_KIND_PERF
                })
                .collect();
            if !perf.is_empty() {
                return perf;
            }
            if !background.is_empty() {
                return background;
            }
        }
        Timer::after(EmbassyDuration::from_millis(25)).await;
    }
}

fn pop_job() -> Option<QueuedJob> {
    let mut jobs = JOBS.lock();
    let active_tts_id = ACTIVE_TTS_JOB_ID.load(Ordering::Acquire);
    let index = jobs.iter().position(|queued| {
        queued.direction != Direction::TextToSpeech
            || active_tts_id == 0
            || queued.id == active_tts_id
    })?;
    let queued = jobs.remove(index)?;
    if queued.direction == Direction::TextToSpeech && active_tts_id == 0 {
        // The queue lock makes selection and ownership assignment one operation.
        ACTIVE_TTS_JOB_ID.store(queued.id, Ordering::Release);
    }
    Some(queued)
}

fn requeue_job(job: QueuedJob) {
    JOBS.lock().push_back(job);
    JOB_WAIT.notify_one();
}

fn finish_job(queued: QueuedJob, direction: Direction, result: JobProgress) {
    let id = queued.id;
    let finished_at = Instant::now();
    let total_us = finished_at
        .saturating_duration_since(queued.submitted_at)
        .as_micros();
    let initial_queue_us = queued
        .first_run_at
        .map(|first_run_at| {
            first_run_at
                .saturating_duration_since(queued.submitted_at)
                .as_micros()
        })
        .unwrap_or(total_us);
    let active_wall_us = queued
        .first_run_at
        .map(|first_run_at| {
            finished_at
                .saturating_duration_since(first_run_at)
                .as_micros()
        })
        .unwrap_or(0);
    let run_slices = queued.run_slices;
    let run_us = queued.run_us;
    if let JobCompletion::Tts(output) = queued.completion {
        match result {
            JobProgress::Complete => {
                output.ensure_finished(Err("backend-completed-without-finishing-tts-stream"))
            }
            JobProgress::Failed(reason) => output.ensure_finished(Err(reason)),
            JobProgress::Pending => {}
        }
    }
    OUTSTANDING_JOBS.fetch_sub(1, Ordering::AcqRel);
    if direction == Direction::TextToSpeech {
        OUTSTANDING_TTS_JOBS.fetch_sub(1, Ordering::AcqRel);
        let _ = ACTIVE_TTS_JOB_ID.compare_exchange(id, 0, Ordering::AcqRel, Ordering::Acquire);
        // Workers may be asleep because they observed only blocked TTS jobs.
        JOB_WAIT.notify_one();
    }
    match result {
        JobProgress::Complete => crate::log_info!(
            target: "ttstt";
            "ttstt: job complete id={} direction={} initial_queue_us={} active_wall_us={} run_us={} slices={} total_us={}\n",
            id,
            direction.as_str(),
            initial_queue_us,
            active_wall_us,
            run_us,
            run_slices,
            total_us
        ),
        JobProgress::Failed(reason) => crate::log_warn!(
            target: "ttstt";
            "ttstt: job failed id={} direction={} reason={} initial_queue_us={} active_wall_us={} run_us={} slices={} total_us={}\n",
            id,
            direction.as_str(),
            reason,
            initial_queue_us,
            active_wall_us,
            run_us,
            run_slices,
            total_us
        ),
        JobProgress::Pending => {}
    }
    if direction == Direction::Warmup {
        let ready = speech_backend_ready();
        let terminal_reason = speech_backend_warm_failure_reason();
        if terminal_reason.is_none() && (!matches!(result, JobProgress::Complete) || !ready) {
            BACKEND_WARM_STARTED.store(false, Ordering::Release);
        }
        crate::log_info!(
            target: "ttstt";
            "ttstt: backend warm finished id={} ready={} terminal={} reason={}\n",
            id,
            ready,
            terminal_reason.is_some() as u8,
            terminal_reason.unwrap_or("none")
        );
    }
}

#[trueos_executor::task(pool_size = WORKER_TASK_POOL)]
async fn worker_task(slot: u32, core_kind: u8, models: &'static ModelSet) {
    let q8_dispatcher = trueos_ttstt_cpu::Dispatcher::detect();
    let context = WorkerContext {
        slot,
        core_kind,
        q8_lane: map_q8_kernel_lane(q8_dispatcher.best_lane()),
        q8_dispatcher,
        bf16_lane: map_bf16_kernel_lane(
            crate::turbo::avx2_fma_sse2_help::selected_bf16_matvec_lane(),
        ),
    };
    WORKER_COUNT.fetch_add(1, Ordering::AcqRel);
    crate::log_info!(
        target: "ttstt";
        "ttstt: worker online slot={} core_kind={} q8_lane={} bf16_lane={} policy=ap2+-prefer-pcore\n",
        slot,
        core_kind,
        context.q8_lane.as_str(),
        context.bf16_lane.as_str()
    );

    loop {
        let Some(mut queued) = pop_job() else {
            JOB_WAIT.wait_for_event_timeout(WORKER_IDLE_POLL_MS).await;
            continue;
        };
        let direction = queued.direction;
        if matches!(&queued.completion, JobCompletion::Tts(output) if output.cancelled()) {
            finish_job(queued, direction, JobProgress::Failed("tts-stream-cancelled"));
            Timer::after(EmbassyDuration::from_millis(WORKER_SLICE_YIELD_MS)).await;
            continue;
        }
        // A stream terminal is the output-side commit point. Do not retain the
        // serialized owner forever if a backend closes it but mistakenly
        // reports `Pending`, including when another output clone closes it
        // between worker slices.
        if matches!(&queued.completion, JobCompletion::Tts(output) if output.finished()) {
            finish_job(queued, direction, JobProgress::Complete);
            Timer::after(EmbassyDuration::from_millis(WORKER_SLICE_YIELD_MS)).await;
            continue;
        }
        let first_dispatch = queued.first_run_at.is_none();
        let slice_started = Instant::now();
        if first_dispatch {
            queued.first_run_at = Some(slice_started);
            if let JobCompletion::Tts(output) = &queued.completion {
                output.mark_service_first_run(slice_started);
            }
        }
        // Keep the high-performance request strictly inside this synchronous
        // Kokoro slice. The AP executor is slot-pinned until `run_slice`
        // returns, so the core-local HWP state is restored before any yield.
        // Unsupported or firmware-disabled HWP takes the unchanged fallback
        // without executing WRMSR.
        let hwp_boost = if direction == Direction::TextToSpeech {
            crate::power::hwp::ScopedHwpPerformance::try_begin().ok()
        } else {
            None
        };
        let result = queued.job.run_slice(models, context);
        drop(hwp_boost);
        let slice_finished = Instant::now();
        queued.run_us = queued.run_us.saturating_add(
            slice_finished
                .saturating_duration_since(slice_started)
                .as_micros(),
        );
        queued.run_slices = queued.run_slices.saturating_add(1);
        if first_dispatch {
            crate::log_info!(
                target: "ttstt";
                "ttstt: job dispatched id={} direction={} initial_queue_us={} slot={}\n",
                queued.id,
                direction.as_str(),
                slice_started
                    .saturating_duration_since(queued.submitted_at)
                    .as_micros(),
                context.slot
            );
        }
        match result {
            JobProgress::Pending if matches!(&queued.completion, JobCompletion::Tts(output) if output.finished()) => {
                finish_job(queued, direction, JobProgress::Complete)
            }
            JobProgress::Pending => requeue_job(queued),
            result => finish_job(queued, direction, result),
        }
        Timer::after(EmbassyDuration::from_millis(WORKER_SLICE_YIELD_MS)).await;
    }
}

async fn start_worker_pool(models: &'static ModelSet) -> Result<usize, SpawnError> {
    let slots = eligible_worker_slots().await;
    let perf_affine = slots
        .iter()
        .all(|slot| crate::workers::core_kind_for_slot(*slot) == crate::workers::CORE_KIND_PERF);
    let mut spawned = 0usize;
    for slot in slots {
        let Some(spawner) = crate::workers::spawner_for_slot(slot) else {
            continue;
        };
        let core_kind = crate::workers::core_kind_for_slot(slot);
        match worker_task(slot, core_kind, models) {
            Ok(token) => {
                spawner.spawn(token);
                spawned += 1;
            }
            Err(error) if spawned == 0 => return Err(error),
            Err(error) => crate::log_warn!(
                target: "ttstt";
                "ttstt: worker spawn failed slot={} err={:?}\n",
                slot,
                error
            ),
        }
    }
    crate::log_info!(
        target: "ttstt";
        "ttstt: worker pool spawned={} pcore_affine={} placement=AP2+\n",
        spawned,
        perf_affine
    );
    Ok(spawned)
}

/// BSP-resident service controller. The central service registry spawns this
/// task locally; only the inference workers are placed on AP executors.
#[trueos_executor::task]
pub async fn service_task() {
    crate::ai::ttstt_kokoro::install();
    loop {
        let models = if let Some(models) = resident_models() {
            // A previous worker-pool attempt failed before spawning anything.
            // Model residency is permanent, so retry placement without doing
            // filesystem I/O or allocating a second resident copy.
            SERVICE_STATE.store(ServiceState::ModelsResident as u8, Ordering::Release);
            models
        } else {
            SERVICE_STATE.store(ServiceState::LoadingModels as u8, Ordering::Release);
            crate::log_info!(
                target: "ttstt";
                "ttstt: cooperative model warm begin root=trueosfs:/{} chunk_bytes={}\n",
                MODEL_ROOT,
                MODEL_READ_CHUNK_BYTES
            );
            match load_model_set().await {
                Ok(models) => {
                    // Model inference is a boot-lifetime service: seal the one
                    // successful load into permanent residency. This is the
                    // ownership proof for all zero-copy backend views.
                    let models: &'static ModelSet = Box::leak(models);
                    RESIDENT_BYTES.store(models.total_bytes() as u64, Ordering::Release);
                    *MODELS.lock() = Some(models);
                    SERVICE_STATE.store(ServiceState::ModelsResident as u8, Ordering::Release);
                    models
                }
                Err(error) => {
                    let delay_ms = error.retry_ms();
                    crate::log_warn!(
                        target: "ttstt";
                        "ttstt: model warm deferred reason={} retry_ms={}\n",
                        error.describe(),
                        delay_ms
                    );
                    SERVICE_STATE.store(ServiceState::WaitingForModels as u8, Ordering::Release);
                    Timer::after(EmbassyDuration::from_millis(delay_ms)).await;
                    continue;
                }
            }
        };

        match start_worker_pool(models).await {
            Ok(0) => {
                crate::log_warn!(target: "ttstt"; "ttstt: no AP2+ worker could start\n");
                SERVICE_STATE.store(ServiceState::ModelsResident as u8, Ordering::Release);
            }
            Ok(workers) => {
                SERVICE_STATE.store(ServiceState::Ready as u8, Ordering::Release);
                crate::log_info!(
                    target: "ttstt";
                    "ttstt: ready resident_bytes={} workers={} filesystem_on_inference_path=no speech_backend={} speech_ready={}\n",
                    RESIDENT_BYTES.load(Ordering::Acquire),
                    workers,
                    speech_backend_name().unwrap_or("unregistered"),
                    speech_backend_ready() as u8
                );
                loop {
                    let _ = ensure_backend_warm_started();
                    Timer::after(EmbassyDuration::from_secs(1)).await;
                }
            }
            Err(error) => {
                crate::log_warn!(
                    target: "ttstt";
                    "ttstt: worker pool start failed err={:?}\n",
                    error
                );
                SERVICE_STATE.store(ServiceState::ModelsResident as u8, Ordering::Release);
            }
        }

        Timer::after(EmbassyDuration::from_millis(250)).await;
    }
}
