//! Bounded TTS diagnostic capture and BSP-owned TRUEOSFS writer.
//!
//! Automatic recent capture starts armed for exactly three claimed sessions
//! per boot (or explicit rearm) and retains at most three committed bundles in
//! stable rolling slots. This physical-write budget matters because TRUEOSFS is
//! append-only. The explicit one-shot arm remains available, takes precedence,
//! and does not consume the automatic budget. The audio path only moves a
//! completed model waveform into the session and copies bounded PCM into it;
//! filesystem work and WAV encoding happen later on the BSP executor. Capture
//! exhaustion, truncation, and persistence failures never become TTS failures.

extern crate alloc;

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::fmt::Write as _;
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

use spin::Mutex;

pub(crate) const CAPTURE_MAX_SECONDS: usize = 30;
pub(crate) const CAPTURE_RAW_SAMPLE_RATE_HZ: usize = 24_000;
pub(crate) const CAPTURE_PCM_SAMPLE_RATE_HZ: usize = 48_000;
pub(crate) const CAPTURE_PCM_CHANNELS: usize = 2;

const RAW_SAMPLE_CAP: usize = CAPTURE_RAW_SAMPLE_RATE_HZ * CAPTURE_MAX_SECONDS;
const PCM_FRAME_CAP: usize = CAPTURE_PCM_SAMPLE_RATE_HZ * CAPTURE_MAX_SECONDS;
const PCM_SAMPLE_CAP: usize = PCM_FRAME_CAP * CAPTURE_PCM_CHANNELS;
const RAW_MODEL_CHUNK_CAP: usize = 128;
pub(crate) const RECENT_SLOT_COUNT: usize = 3;
pub(crate) const RECENT_CLAIM_BUDGET: u8 = RECENT_SLOT_COUNT as u8;

const SESSION_ACTIVE: u8 = 0;
const SESSION_FINISHING: u8 = 1;
const SESSION_FINISHED: u8 = 2;

static ARMED: AtomicBool = AtomicBool::new(false);
static RECENT_CLAIMS_REMAINING: AtomicU8 = AtomicU8::new(RECENT_CLAIM_BUDGET);
static ACTIVE_SESSION: AtomicBool = AtomicBool::new(false);
static WRITER_ONLINE: AtomicBool = AtomicBool::new(false);
static NEXT_SEQUENCE: AtomicU64 = AtomicU64::new(1);
static LAST_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static CAPTURES_QUEUED: AtomicU64 = AtomicU64::new(0);
static CAPTURES_WRITTEN: AtomicU64 = AtomicU64::new(0);
static CAPTURES_FAILED: AtomicU64 = AtomicU64::new(0);
static CAPTURES_DROPPED: AtomicU64 = AtomicU64::new(0);
static CAPTURES_SKIPPED: AtomicU64 = AtomicU64::new(0);
static WRITER_WAIT: crate::wait::WaitQueue = crate::wait::WaitQueue::new();

static WRITER_STATE: Mutex<WriterState> = Mutex::new(WriterState {
    queued: None,
    busy: false,
});

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct CaptureTiming {
    /// Wait in shell2's serialized request FIFO before service admission.
    pub queue_wait_ms: u64,
    /// Shared TTSTT service admission to the first actual backend slice.
    pub service_queue_wait_us: u64,
    /// First actual backend slice to first PCM observed by shell2.
    pub service_dispatch_to_first_pcm_us: u64,
    /// Shared TTSTT service admission to first PCM observed by shell2.
    pub service_submit_to_first_pcm_us: u64,
    pub first_pcm_ms: u64,
    pub handoff_wait_ms: u64,
    pub active_ms: u64,
    pub total_ms: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CaptureDisposition {
    Queued,
    DroppedWriterBusy,
    AlreadyFinished,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CaptureFinish {
    pub sequence: u64,
    pub request_id: u64,
    pub job_id: u64,
    pub disposition: CaptureDisposition,
    pub raw_truncated: bool,
    pub pcm_truncated: bool,
    pub raw_samples_retained: u64,
    pub pcm_frames_retained: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct CaptureStatus {
    pub armed: bool,
    pub recent_enabled: bool,
    pub recent_claims_remaining: u8,
    pub active: bool,
    pub writer_online: bool,
    pub busy: bool,
    pub queued: bool,
    pub captures_queued: u64,
    pub captures_written: u64,
    pub captures_failed: u64,
    pub captures_dropped: u64,
    pub captures_skipped: u64,
    pub last_sequence: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CaptureMode {
    OneShot,
    Recent,
}

impl CaptureMode {
    const fn as_str(self) -> &'static str {
        match self {
            Self::OneShot => "one-shot",
            Self::Recent => "recent",
        }
    }
}

#[derive(Clone)]
pub(crate) struct CaptureSession {
    inner: Arc<CaptureInner>,
}

struct CaptureInner {
    sequence: u64,
    slot: u8,
    mode: CaptureMode,
    recent_claims_remaining: Option<u8>,
    request_id: u64,
    job_id: AtomicU64,
    terminal: AtomicU8,
    payload: Mutex<Option<CapturePayload>>,
    finish: Mutex<Option<CaptureFinish>>,
}

struct CapturePayload {
    claimed_at_ms: u64,
    text: String,
    voice: String,
    speed: f32,
    raw_chunks: Vec<RawModelChunk>,
    raw_chunks_seen: u64,
    raw_samples_seen: u64,
    raw_samples_retained: usize,
    raw_non_finite_samples: u64,
    raw_truncated: bool,
    raw_capture_closed: bool,
    pcm_samples: Vec<i16>,
    pcm_pushes: u64,
    pcm_samples_seen: u64,
    pcm_bad_shape_pushes: u64,
    pcm_truncated: bool,
    pcm_capture_closed: bool,
}

struct RawModelChunk {
    index: u32,
    phonemes: u16,
    original_samples: usize,
    samples: Vec<f32>,
}

struct DeclaredSummary {
    model_chunks: u32,
    pcm_chunks: u32,
    pcm_frames: u64,
}

enum CaptureTerminal {
    Success,
    Failed(String),
}

struct CompletedCapture {
    sequence: u64,
    slot: u8,
    mode: CaptureMode,
    recent_claims_remaining: Option<u8>,
    request_id: u64,
    job_id: u64,
    terminal: CaptureTerminal,
    declared: Option<DeclaredSummary>,
    timing: CaptureTiming,
    payload: CapturePayload,
}

struct WriterState {
    queued: Option<CompletedCapture>,
    busy: bool,
}

/// Arm exactly the next eligible shell TTS request.
///
/// Returns `false` when capture is already armed, a session is active, or the
/// sole completed-artifact slot is queued/writing.
pub(crate) fn arm_next() -> bool {
    if ARMED.load(Ordering::Acquire) || ACTIVE_SESSION.load(Ordering::Acquire) {
        return false;
    }
    {
        let writer = WRITER_STATE.lock();
        if writer.busy || writer.queued.is_some() {
            return false;
        }
    }
    if ARMED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return false;
    }
    crate::log_info!(target: "ttstt"; "ttstt-capture: armed mode=next-request max_seconds={}\n", CAPTURE_MAX_SECONDS);
    true
}

/// Disarm a pending one-shot capture. Active sessions are not affected.
pub(crate) fn disarm() -> bool {
    let was_armed = ARMED.swap(false, Ordering::AcqRel);
    if was_armed {
        crate::log_info!(target: "ttstt"; "ttstt-capture: disarmed\n");
    }
    was_armed
}

/// Rearm bounded automatic capture for exactly three claimed sessions. This
/// resets the budget even when it was already enabled. Active and queued work
/// is not affected.
pub(crate) fn arm_recent() -> u8 {
    let previous = RECENT_CLAIMS_REMAINING.swap(RECENT_CLAIM_BUDGET, Ordering::AcqRel);
    crate::log_info!(target: "ttstt";
        "ttstt-capture: mode=recent enabled=1 rearmed=1 previous_remaining={} remaining={} physical_budget={}-per-arm slots={} max_seconds={}\n",
        previous,
        RECENT_CLAIM_BUDGET,
        RECENT_CLAIM_BUDGET,
        RECENT_SLOT_COUNT,
        CAPTURE_MAX_SECONDS,
    );
    previous
}

/// Disable automatic recent capture without disturbing active or queued work.
pub(crate) fn disable_recent() -> u8 {
    let previous = RECENT_CLAIMS_REMAINING.swap(0, Ordering::AcqRel);
    crate::log_info!(target: "ttstt";
        "ttstt-capture: mode=recent enabled=0 previous_remaining={} remaining=0 physical_budget={}-per-arm active_capture=unaffected\n",
        previous,
        RECENT_CLAIM_BUDGET,
    );
    previous
}

/// Consume one automatic claim atomically. The returned value is the budget
/// remaining after this claim; zero also represents automatic disablement.
fn claim_recent_budget() -> Option<u8> {
    let mut remaining = RECENT_CLAIMS_REMAINING.load(Ordering::Acquire);
    loop {
        if remaining == 0 {
            return None;
        }
        let next = remaining - 1;
        match RECENT_CLAIMS_REMAINING.compare_exchange_weak(
            remaining,
            next,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return Some(next),
            Err(actual) => remaining = actual,
        }
    }
}

fn skip_capture(mode: CaptureMode, request_id: u64, reason: &'static str) {
    let skipped = CAPTURES_SKIPPED
        .fetch_add(1, Ordering::Relaxed)
        .saturating_add(1);
    crate::log_info!(target: "ttstt";
        "ttstt-capture: skipped mode={} request={} reason={} skipped_total={} speech_affected=0\n",
        mode.as_str(),
        request_id,
        reason,
        skipped,
    );
}

/// Claim an explicit one-shot arm, or otherwise the automatic recent slot, for
/// a shell request. This is best-effort and never waits for capture capacity.
pub(crate) fn claim_next(
    request_id: u64,
    text: &str,
    voice: &str,
    speed: f32,
) -> Option<CaptureSession> {
    let mut mode = if ARMED.load(Ordering::Acquire) {
        CaptureMode::OneShot
    } else if RECENT_CLAIMS_REMAINING.load(Ordering::Acquire) != 0 {
        CaptureMode::Recent
    } else {
        return None;
    };

    {
        let writer = WRITER_STATE.lock();
        if writer.busy || writer.queued.is_some() {
            skip_capture(mode, request_id, "writer-busy-or-queued");
            return None;
        }
    }
    if ACTIVE_SESSION
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        skip_capture(mode, request_id, "active-session");
        return None;
    }

    // Close the arm/claim race: an explicit one-shot arm established before
    // this request acquired ACTIVE_SESSION still takes precedence.
    let one_shot_claimed = ARMED
        .compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire)
        .is_ok();
    if one_shot_claimed {
        mode = CaptureMode::OneShot;
    } else if mode == CaptureMode::OneShot {
        if RECENT_CLAIMS_REMAINING.load(Ordering::Acquire) != 0 {
            mode = CaptureMode::Recent;
        } else {
            ACTIVE_SESSION.store(false, Ordering::Release);
            return None;
        }
    }

    let recent_claims_remaining = if mode == CaptureMode::Recent {
        let Some(remaining) = claim_recent_budget() else {
            ACTIVE_SESSION.store(false, Ordering::Release);
            return None;
        };
        if remaining == 0 {
            crate::log_info!(target: "ttstt";
                "ttstt-capture: mode=recent auto-disabled=1 reason=physical-budget-exhausted remaining=0 physical_budget={}-per-arm rearm_command=tts-capture-recent-on\n",
                RECENT_CLAIM_BUDGET,
            );
        }
        Some(remaining)
    } else {
        None
    };

    let sequence = NEXT_SEQUENCE.fetch_add(1, Ordering::AcqRel).max(1);
    let slot = ((sequence - 1) % RECENT_SLOT_COUNT as u64) as u8;
    LAST_SEQUENCE.store(sequence, Ordering::Release);
    let inner = CaptureInner {
        sequence,
        slot,
        mode,
        recent_claims_remaining,
        request_id,
        job_id: AtomicU64::new(0),
        terminal: AtomicU8::new(SESSION_ACTIVE),
        payload: Mutex::new(Some(CapturePayload {
            claimed_at_ms: embassy_time::Instant::now().as_millis(),
            text: String::from(text),
            voice: String::from(voice),
            speed,
            raw_chunks: Vec::new(),
            raw_chunks_seen: 0,
            raw_samples_seen: 0,
            raw_samples_retained: 0,
            raw_non_finite_samples: 0,
            raw_truncated: false,
            raw_capture_closed: false,
            pcm_samples: Vec::new(),
            pcm_pushes: 0,
            pcm_samples_seen: 0,
            pcm_bad_shape_pushes: 0,
            pcm_truncated: false,
            pcm_capture_closed: false,
        })),
        finish: Mutex::new(None),
    };
    let base = capture_base_path_for_slot(slot);
    crate::log_info!(target: "ttstt";
        "ttstt-capture: claimed mode={} sequence={} slot={} request={} recent_remaining={} physical_budget={}-per-arm one_shot_consumes_recent_budget=0 base=trueosfs:/{} metadata=trueosfs:/{}-metadata.txt max_raw_samples={} max_pcm_frames={}\n",
        mode.as_str(),
        sequence,
        slot,
        request_id,
        recent_claims_remaining.unwrap_or_else(|| RECENT_CLAIMS_REMAINING.load(Ordering::Acquire)),
        RECENT_CLAIM_BUDGET,
        base,
        base,
        RAW_SAMPLE_CAP,
        PCM_FRAME_CAP,
    );
    Some(CaptureSession {
        inner: Arc::new(inner),
    })
}

pub(crate) fn status() -> CaptureStatus {
    let writer = WRITER_STATE.lock();
    let recent_claims_remaining = RECENT_CLAIMS_REMAINING.load(Ordering::Acquire);
    CaptureStatus {
        armed: ARMED.load(Ordering::Acquire),
        recent_enabled: recent_claims_remaining != 0,
        recent_claims_remaining,
        active: ACTIVE_SESSION.load(Ordering::Acquire),
        writer_online: WRITER_ONLINE.load(Ordering::Acquire),
        busy: writer.busy,
        queued: writer.queued.is_some(),
        captures_queued: CAPTURES_QUEUED.load(Ordering::Acquire),
        captures_written: CAPTURES_WRITTEN.load(Ordering::Acquire),
        captures_failed: CAPTURES_FAILED.load(Ordering::Acquire),
        captures_dropped: CAPTURES_DROPPED.load(Ordering::Acquire),
        captures_skipped: CAPTURES_SKIPPED.load(Ordering::Acquire),
        last_sequence: LAST_SEQUENCE.load(Ordering::Acquire),
    }
}

impl CaptureSession {
    pub(crate) fn sequence(&self) -> u64 {
        self.inner.sequence
    }

    pub(crate) fn slot(&self) -> u8 {
        self.inner.slot
    }

    pub(crate) fn mode_name(&self) -> &'static str {
        self.inner.mode.as_str()
    }

    pub(crate) fn recent_claims_remaining(&self) -> Option<u8> {
        self.inner.recent_claims_remaining
    }

    pub(crate) fn metadata_path(&self) -> String {
        alloc::format!("trueosfs:/{}-metadata.txt", capture_base_path_for_slot(self.inner.slot))
    }

    pub(crate) fn set_job_id(&self, job_id: u64) {
        if self.inner.terminal.load(Ordering::Acquire) == SESSION_ACTIVE {
            self.inner.job_id.store(job_id, Ordering::Release);
        }
    }

    /// Move one native mono/24-kHz model result into this bounded capture.
    pub(crate) fn push_raw_model_chunk(&self, index: u32, phonemes: u16, mut waveform: Vec<f32>) {
        if self.inner.terminal.load(Ordering::Acquire) != SESSION_ACTIVE {
            return;
        }
        let mut guard = self.inner.payload.lock();
        if self.inner.terminal.load(Ordering::Acquire) != SESSION_ACTIVE {
            return;
        }
        let Some(payload) = guard.as_mut() else {
            return;
        };

        payload.raw_chunks_seen = payload.raw_chunks_seen.saturating_add(1);
        payload.raw_samples_seen = payload
            .raw_samples_seen
            .saturating_add(waveform.len() as u64);
        payload.raw_non_finite_samples = payload
            .raw_non_finite_samples
            .saturating_add(waveform.iter().filter(|sample| !sample.is_finite()).count() as u64);
        if payload.raw_capture_closed {
            return;
        }
        let original_samples = waveform.len();
        let remaining = RAW_SAMPLE_CAP.saturating_sub(payload.raw_samples_retained);
        let retained = waveform.len().min(remaining);
        if retained != waveform.len() || payload.raw_chunks.len() >= RAW_MODEL_CHUNK_CAP {
            payload.raw_truncated = true;
            payload.raw_capture_closed = true;
        }
        if retained == 0 || payload.raw_chunks.len() >= RAW_MODEL_CHUNK_CAP {
            return;
        }
        waveform.truncate(retained);
        if payload.raw_chunks.try_reserve(1).is_err() {
            payload.raw_truncated = true;
            payload.raw_capture_closed = true;
            return;
        }
        payload.raw_samples_retained = payload.raw_samples_retained.saturating_add(retained);
        payload.raw_chunks.push(RawModelChunk {
            index,
            phonemes,
            original_samples,
            samples: waveform,
        });
    }

    /// Copy post-conversion PCM into the request-level capture without ever
    /// backpressuring its caller.
    pub(crate) fn push_pcm(&self, samples: &[i16]) {
        if self.inner.terminal.load(Ordering::Acquire) != SESSION_ACTIVE {
            return;
        }
        let mut guard = self.inner.payload.lock();
        if self.inner.terminal.load(Ordering::Acquire) != SESSION_ACTIVE {
            return;
        }
        let Some(payload) = guard.as_mut() else {
            return;
        };

        payload.pcm_pushes = payload.pcm_pushes.saturating_add(1);
        payload.pcm_samples_seen = payload
            .pcm_samples_seen
            .saturating_add(samples.len() as u64);
        let valid_samples = samples.len() - samples.len() % CAPTURE_PCM_CHANNELS;
        if valid_samples != samples.len() {
            payload.pcm_bad_shape_pushes = payload.pcm_bad_shape_pushes.saturating_add(1);
            payload.pcm_truncated = true;
            payload.pcm_capture_closed = true;
        }
        if payload.pcm_capture_closed {
            return;
        }
        let remaining = PCM_SAMPLE_CAP.saturating_sub(payload.pcm_samples.len());
        let retained = valid_samples.min(remaining);
        if retained != valid_samples {
            payload.pcm_truncated = true;
            payload.pcm_capture_closed = true;
        }
        if retained == 0 {
            return;
        }
        if payload.pcm_samples.try_reserve(retained).is_err() {
            payload.pcm_truncated = true;
            payload.pcm_capture_closed = true;
            return;
        }
        payload.pcm_samples.extend_from_slice(&samples[..retained]);
    }

    pub(crate) fn fail(&self, reason: &str) -> CaptureFinish {
        self.finish_once(
            CaptureTerminal::Failed(String::from(reason)),
            None,
            CaptureTiming::default(),
        )
    }

    pub(crate) fn finish_success(
        &self,
        model_chunks: u32,
        pcm_chunks: u32,
        pcm_frames: u64,
        timing: CaptureTiming,
    ) -> CaptureFinish {
        self.finish_once(
            CaptureTerminal::Success,
            Some(DeclaredSummary {
                model_chunks,
                pcm_chunks,
                pcm_frames,
            }),
            timing,
        )
    }

    fn finish_once(
        &self,
        terminal: CaptureTerminal,
        declared: Option<DeclaredSummary>,
        timing: CaptureTiming,
    ) -> CaptureFinish {
        if self
            .inner
            .terminal
            .compare_exchange(
                SESSION_ACTIVE,
                SESSION_FINISHING,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_err()
        {
            return self.inner.finish.lock().unwrap_or(CaptureFinish {
                sequence: self.inner.sequence,
                request_id: self.inner.request_id,
                job_id: self.inner.job_id.load(Ordering::Acquire),
                disposition: CaptureDisposition::AlreadyFinished,
                raw_truncated: false,
                pcm_truncated: false,
                raw_samples_retained: 0,
                pcm_frames_retained: 0,
            });
        }

        let job_id = self.inner.job_id.load(Ordering::Acquire);
        let Some(payload) = self.inner.payload.lock().take() else {
            let finish = CaptureFinish {
                sequence: self.inner.sequence,
                request_id: self.inner.request_id,
                job_id,
                disposition: CaptureDisposition::AlreadyFinished,
                raw_truncated: false,
                pcm_truncated: false,
                raw_samples_retained: 0,
                pcm_frames_retained: 0,
            };
            *self.inner.finish.lock() = Some(finish);
            self.inner
                .terminal
                .store(SESSION_FINISHED, Ordering::Release);
            ACTIVE_SESSION.store(false, Ordering::Release);
            return finish;
        };

        let finish_fields = (
            payload.raw_truncated,
            payload.pcm_truncated,
            payload.raw_samples_retained as u64,
            (payload.pcm_samples.len() / CAPTURE_PCM_CHANNELS) as u64,
        );
        let mut completed = Some(CompletedCapture {
            sequence: self.inner.sequence,
            slot: self.inner.slot,
            mode: self.inner.mode,
            recent_claims_remaining: self.inner.recent_claims_remaining,
            request_id: self.inner.request_id,
            job_id,
            terminal,
            declared,
            timing,
            payload,
        });
        let queued = {
            let mut writer = WRITER_STATE.lock();
            if writer.busy || writer.queued.is_some() {
                false
            } else {
                writer.queued = completed.take();
                true
            }
        };
        ACTIVE_SESSION.store(false, Ordering::Release);

        let disposition = if queued {
            CAPTURES_QUEUED.fetch_add(1, Ordering::Relaxed);
            WRITER_WAIT.notify_one();
            CaptureDisposition::Queued
        } else {
            CAPTURES_DROPPED.fetch_add(1, Ordering::Relaxed);
            crate::log_warn!(target: "ttstt";
                "ttstt-capture: dropped mode={} sequence={} slot={} request={} job={} reason=writer-busy-or-queued\n",
                self.inner.mode.as_str(),
                self.inner.sequence,
                self.inner.slot,
                self.inner.request_id,
                job_id,
            );
            CaptureDisposition::DroppedWriterBusy
        };
        drop(completed);

        let finish = CaptureFinish {
            sequence: self.inner.sequence,
            request_id: self.inner.request_id,
            job_id,
            disposition,
            raw_truncated: finish_fields.0,
            pcm_truncated: finish_fields.1,
            raw_samples_retained: finish_fields.2,
            pcm_frames_retained: finish_fields.3,
        };
        *self.inner.finish.lock() = Some(finish);
        self.inner
            .terminal
            .store(SESSION_FINISHED, Ordering::Release);
        finish
    }
}

impl Drop for CaptureInner {
    fn drop(&mut self) {
        if self.terminal.load(Ordering::Acquire) == SESSION_ACTIVE {
            ACTIVE_SESSION.store(false, Ordering::Release);
            CAPTURES_DROPPED.fetch_add(1, Ordering::Relaxed);
            crate::log_warn!(target: "ttstt";
                "ttstt-capture: dropped mode={} sequence={} slot={} request={} job={} reason=session-abandoned\n",
                self.mode.as_str(),
                self.sequence,
                self.slot,
                self.request_id,
                self.job_id.load(Ordering::Acquire),
            );
        }
    }
}

#[embassy_executor::task]
pub(crate) async fn writer_task() {
    if WRITER_ONLINE.swap(true, Ordering::AcqRel) {
        crate::log_warn!(target: "ttstt"; "ttstt-capture: duplicate writer rejected\n");
        return;
    }
    crate::log_info!(target: "ttstt";
        "ttstt-capture: writer online realm=bsp queue_cap=1 max_seconds={} recent_default=armed recent_remaining={} physical_budget={}-per-arm recent_slots={} paths=trueosfs:/audio/tts-recent-s{{0,1,2}}-*\n",
        CAPTURE_MAX_SECONDS,
        RECENT_CLAIM_BUDGET,
        RECENT_CLAIM_BUDGET,
        RECENT_SLOT_COUNT,
    );

    loop {
        let capture = {
            let mut writer = WRITER_STATE.lock();
            match writer.queued.take() {
                Some(capture) => {
                    writer.busy = true;
                    Some(capture)
                }
                None => None,
            }
        };
        let Some(capture) = capture else {
            WRITER_WAIT.wait_for_event_timeout(250).await;
            continue;
        };

        let sequence = capture.sequence;
        let request_id = capture.request_id;
        let job_id = capture.job_id;
        let ok = write_completed_capture(capture).await;
        if ok {
            CAPTURES_WRITTEN.fetch_add(1, Ordering::Relaxed);
        } else {
            CAPTURES_FAILED.fetch_add(1, Ordering::Relaxed);
        }
        WRITER_STATE.lock().busy = false;
        crate::log_info!(target: "ttstt";
            "ttstt-capture: writer complete sequence={} request={} job={} ok={}\n",
            sequence,
            request_id,
            job_id,
            usize::from(ok),
        );
    }
}

async fn write_completed_capture(mut capture: CompletedCapture) -> bool {
    let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
        crate::log_warn!(target: "ttstt";
            "ttstt-capture: write failed sequence={} request={} job={} reason=no-root\n",
            capture.sequence,
            capture.request_id,
            capture.job_id,
        );
        return false;
    };

    let base = capture_base_path(&capture);
    let metadata_path = alloc::format!("{}-metadata.txt", base);
    let metadata = encode_metadata(&capture, &base);
    let mut written_paths = Vec::new();
    if written_paths
        .try_reserve(capture.payload.raw_chunks.len().saturating_add(2))
        .is_err()
    {
        crate::log_warn!(target: "ttstt";
            "ttstt-capture: write failed sequence={} reason=path-table-allocation\n",
            capture.sequence,
        );
        return false;
    }

    let pcm_path = alloc::format!("{}-pcm-s16-stereo-48k.wav", base);
    let pcm_samples = core::mem::take(&mut capture.payload.pcm_samples);
    let pcm_frames = pcm_samples.len() / CAPTURE_PCM_CHANNELS;
    let Some(pcm_wav) = encode_pcm_wav(pcm_samples.as_slice()) else {
        crate::log_warn!(target: "ttstt";
            "ttstt-capture: encode failed sequence={} artifact=pcm reason=allocation-or-size\n",
            capture.sequence,
        );
        return false;
    };
    if !prepare_capture_slot(disk, capture.slot, metadata_path.as_str()).await {
        return false;
    }
    if !write_artifact(disk, pcm_path.as_str(), pcm_wav.as_slice()).await {
        return false;
    }
    drop(pcm_wav);
    drop(pcm_samples);
    written_paths.push(pcm_path);

    let raw_chunks = core::mem::take(&mut capture.payload.raw_chunks);
    let raw_file_count = raw_chunks.len();
    for (ordinal, chunk) in raw_chunks.into_iter().enumerate() {
        let raw_path = raw_chunk_path(&base, ordinal);
        let Some(raw_wav) = encode_f32_wav(chunk.samples.as_slice()) else {
            crate::log_warn!(target: "ttstt";
                "ttstt-capture: encode failed sequence={} artifact=raw ordinal={} model_chunk={} reason=allocation-or-size\n",
                capture.sequence,
                ordinal,
                chunk.index,
            );
            cleanup_artifacts(disk, written_paths.as_slice()).await;
            return false;
        };
        if !write_artifact(disk, raw_path.as_str(), raw_wav.as_slice()).await {
            cleanup_artifacts(disk, written_paths.as_slice()).await;
            return false;
        }
        written_paths.push(raw_path);
    }

    // Metadata is the bundle's commit marker. A visible metadata file therefore
    // means every referenced WAV was already published successfully.
    if !write_artifact(disk, metadata_path.as_str(), metadata.as_bytes()).await {
        cleanup_artifacts(disk, written_paths.as_slice()).await;
        return false;
    }

    crate::log_info!(target: "ttstt";
        "ttstt-capture: wrote mode={} sequence={} slot={}/{} request={} job={} recent_remaining_after_claim={} physical_budget={}-per-arm base=trueosfs:/{} metadata=trueosfs:/{} raw_files={} pcm_frames={} raw_truncated={} pcm_truncated={}\n",
        capture.mode.as_str(),
        capture.sequence,
        capture.slot,
        RECENT_SLOT_COUNT,
        capture.request_id,
        capture.job_id,
        capture.recent_claims_remaining.map_or_else(
            || String::from("unchanged"),
            |remaining| alloc::format!("{}", remaining),
        ),
        RECENT_CLAIM_BUDGET,
        base,
        metadata_path,
        raw_file_count,
        pcm_frames,
        usize::from(capture.payload.raw_truncated),
        usize::from(capture.payload.pcm_truncated),
    );
    true
}

/// Invalidate and clean one rolling slot before publishing its replacement.
/// Metadata is deleted first because it is the only commit marker consumers
/// may use to recognize a complete bundle.
async fn prepare_capture_slot(
    disk: crate::disc::block::DeviceHandle,
    slot: u8,
    metadata_path: &str,
) -> bool {
    let listing = match crate::r::fs::trueosfs::list_dir_async(disk, "audio").await {
        Ok(Some(listing)) => listing,
        Ok(None) => {
            crate::log_warn!(target: "ttstt";
                "ttstt-capture: slot prepare failed slot={} reason=no-trueosfs-root\n",
                slot,
            );
            return false;
        }
        Err(error) => {
            crate::log_warn!(target: "ttstt";
                "ttstt-capture: slot prepare failed slot={} reason=list-audio error={:?}\n",
                slot,
                error,
            );
            return false;
        }
    };

    if !delete_artifact(disk, metadata_path, "invalidate-commit").await {
        return false;
    }

    let child_prefix = alloc::format!("tts-recent-s{}-", slot);
    let mut listing_truncated = false;
    for child in listing.lines() {
        if child == "..." {
            listing_truncated = true;
            continue;
        }
        if !child.starts_with(child_prefix.as_str()) {
            continue;
        }
        let path = alloc::format!("audio/{}", child);
        if path == metadata_path {
            continue;
        }
        if !delete_artifact(disk, path.as_str(), "replace-slot").await {
            return false;
        }
    }

    // A truncated directory listing cannot prove cleanliness. Sweep every
    // filename admitted by this bounded format before reusing the slot.
    if listing_truncated {
        let base = capture_base_path_for_slot(slot);
        let pcm_path = alloc::format!("{}-pcm-s16-stereo-48k.wav", base);
        if !delete_artifact(disk, pcm_path.as_str(), "replace-slot-fallback").await {
            return false;
        }
        for ordinal in 0..RAW_MODEL_CHUNK_CAP {
            let raw_path = raw_chunk_path(base.as_str(), ordinal);
            if !delete_artifact(disk, raw_path.as_str(), "replace-slot-fallback").await {
                return false;
            }
        }
    }
    true
}

async fn delete_artifact(
    disk: crate::disc::block::DeviceHandle,
    path: &str,
    operation: &'static str,
) -> bool {
    match crate::r::fs::trueosfs::file_delete_async(disk, path).await {
        Ok(_) => true,
        Err(error) => {
            crate::log_warn!(target: "ttstt";
                "ttstt-capture: artifact delete failed operation={} path=trueosfs:/{} error={:?}\n",
                operation,
                path,
                error,
            );
            false
        }
    }
}

async fn cleanup_artifacts(disk: crate::disc::block::DeviceHandle, paths: &[String]) {
    for path in paths.iter().rev() {
        match crate::r::fs::trueosfs::file_delete_async(disk, path.as_str()).await {
            Ok(true) => crate::log_info!(target: "ttstt";
                "ttstt-capture: cleanup removed path=trueosfs:/{}\n",
                path,
            ),
            Ok(false) => {}
            Err(error) => crate::log_warn!(target: "ttstt";
                "ttstt-capture: cleanup failed path=trueosfs:/{} error={:?}\n",
                path,
                error,
            ),
        }
    }
}

async fn write_artifact(disk: crate::disc::block::DeviceHandle, path: &str, bytes: &[u8]) -> bool {
    match crate::r::fs::trueosfs::file_write_all_async(disk, path, bytes).await {
        Ok(true) => true,
        Ok(false) => {
            crate::log_warn!(target: "ttstt";
                "ttstt-capture: artifact write failed path=trueosfs:/{} bytes={} reason=no-space-or-placement\n",
                path,
                bytes.len(),
            );
            false
        }
        Err(error) => {
            crate::log_warn!(target: "ttstt";
                "ttstt-capture: artifact write failed path=trueosfs:/{} bytes={} error={:?}\n",
                path,
                bytes.len(),
                error,
            );
            false
        }
    }
}

fn capture_base_path(capture: &CompletedCapture) -> String {
    capture_base_path_for_slot(capture.slot)
}

fn capture_base_path_for_slot(slot: u8) -> String {
    alloc::format!("audio/tts-recent-s{}", slot)
}

fn raw_chunk_path(base: &str, ordinal: usize) -> String {
    alloc::format!("{}-raw-o{:03}-f32-mono-24k.wav", base, ordinal)
}

fn encode_metadata(capture: &CompletedCapture, base: &str) -> String {
    let payload = &capture.payload;
    let mut out = String::with_capacity(2_048 + payload.raw_chunks.len() * 128);
    let terminal = match &capture.terminal {
        CaptureTerminal::Success => "success",
        CaptureTerminal::Failed(_) => "failed",
    };
    let _ = writeln!(out, "format=trueos-tts-capture-v2");
    let _ = writeln!(out, "base_path=trueosfs:/{}", base);
    let _ = writeln!(out, "capture_mode={}", capture.mode.as_str());
    let _ = writeln!(out, "physical_budget={}-per-arm", RECENT_CLAIM_BUDGET);
    let _ = writeln!(out, "one_shot_consumes_recent_budget=0");
    if let Some(remaining) = capture.recent_claims_remaining {
        let _ = writeln!(out, "recent_claims_remaining_after_claim={}", remaining);
    }
    let _ = writeln!(out, "rolling_slot={}", capture.slot);
    let _ = writeln!(out, "rolling_slot_count={}", RECENT_SLOT_COUNT);
    let _ = writeln!(out, "retention=latest-{}-committed-bundles", RECENT_SLOT_COUNT);
    let _ = writeln!(out, "sequence={}", capture.sequence);
    let _ = writeln!(out, "request_id={}", capture.request_id);
    let _ = writeln!(out, "job_id={}", capture.job_id);
    let _ = writeln!(out, "terminal={}", terminal);
    if let CaptureTerminal::Failed(reason) = &capture.terminal {
        let _ = writeln!(out, "failure_reason={:?}", reason);
    }
    let _ = writeln!(out, "text={:?}", payload.text);
    let _ = writeln!(out, "voice={:?}", payload.voice);
    let _ = writeln!(out, "speed={:.6}", payload.speed);
    let _ = writeln!(out, "speed_bits=0x{:08x}", payload.speed.to_bits());
    let _ = writeln!(out, "claimed_at_ms={}", payload.claimed_at_ms);
    // Retain queue_wait_ms for v1 readers, while making its shell-only scope
    // explicit and reporting the separate shared service-queue timing.
    let _ = writeln!(out, "queue_wait_ms={}", capture.timing.queue_wait_ms);
    let _ = writeln!(out, "queue_wait_scope=shell-serialized");
    let _ = writeln!(out, "shell_queue_wait_ms={}", capture.timing.queue_wait_ms);
    let _ = writeln!(out, "service_queue_wait_us={}", capture.timing.service_queue_wait_us);
    let _ = writeln!(
        out,
        "service_dispatch_to_first_pcm_us={}",
        capture.timing.service_dispatch_to_first_pcm_us
    );
    let _ = writeln!(
        out,
        "service_submit_to_first_pcm_us={}",
        capture.timing.service_submit_to_first_pcm_us
    );
    let _ = writeln!(out, "first_pcm_ms={}", capture.timing.first_pcm_ms);
    let _ = writeln!(out, "handoff_wait_ms={}", capture.timing.handoff_wait_ms);
    let _ = writeln!(out, "active_ms={}", capture.timing.active_ms);
    let _ = writeln!(out, "total_ms={}", capture.timing.total_ms);
    if let Some(declared) = capture.declared.as_ref() {
        let _ = writeln!(out, "declared_model_chunks={}", declared.model_chunks);
        let _ = writeln!(out, "declared_pcm_chunks={}", declared.pcm_chunks);
        let _ = writeln!(out, "declared_pcm_frames={}", declared.pcm_frames);
    }
    let _ = writeln!(out, "raw_format=wave-ieee-f32-mono-24000");
    let _ = writeln!(out, "raw_cap_seconds={}", CAPTURE_MAX_SECONDS);
    let _ = writeln!(out, "raw_cap_samples={}", RAW_SAMPLE_CAP);
    let _ = writeln!(out, "raw_chunks_seen={}", payload.raw_chunks_seen);
    let _ = writeln!(out, "raw_chunks_retained={}", payload.raw_chunks.len());
    let _ = writeln!(out, "raw_samples_seen={}", payload.raw_samples_seen);
    let _ = writeln!(out, "raw_samples_retained={}", payload.raw_samples_retained);
    let _ = writeln!(out, "raw_non_finite_samples={}", payload.raw_non_finite_samples);
    let _ = writeln!(out, "raw_truncated={}", usize::from(payload.raw_truncated));
    let _ = writeln!(out, "pcm_format=wave-pcm-s16le-stereo-48000");
    let _ = writeln!(out, "pcm_file={:?}", alloc::format!("{}-pcm-s16-stereo-48k.wav", base));
    let _ = writeln!(out, "pcm_cap_seconds={}", CAPTURE_MAX_SECONDS);
    let _ = writeln!(out, "pcm_cap_frames={}", PCM_FRAME_CAP);
    let _ = writeln!(out, "pcm_pushes={}", payload.pcm_pushes);
    let _ = writeln!(out, "pcm_samples_seen={}", payload.pcm_samples_seen);
    let _ =
        writeln!(out, "pcm_frames_retained={}", payload.pcm_samples.len() / CAPTURE_PCM_CHANNELS);
    let _ = writeln!(out, "pcm_bad_shape_pushes={}", payload.pcm_bad_shape_pushes);
    let _ = writeln!(out, "pcm_truncated={}", usize::from(payload.pcm_truncated));
    for (ordinal, chunk) in payload.raw_chunks.iter().enumerate() {
        let _ = writeln!(
            out,
            "raw_chunk ordinal={} model_index={} phonemes={} original_samples={} retained_samples={} file={:?}",
            ordinal,
            chunk.index,
            chunk.phonemes,
            chunk.original_samples,
            chunk.samples.len(),
            raw_chunk_path(base, ordinal),
        );
    }
    out
}

fn encode_pcm_wav(samples: &[i16]) -> Option<Vec<u8>> {
    let data_bytes = samples.len().checked_mul(core::mem::size_of::<i16>())?;
    let data_bytes_u32 = u32::try_from(data_bytes).ok()?;
    let riff_size = 36u32.checked_add(data_bytes_u32)?;
    let total = 44usize.checked_add(data_bytes)?;
    let mut out = Vec::new();
    out.try_reserve_exact(total).ok()?;
    out.extend_from_slice(b"RIFF");
    push_u32(&mut out, riff_size);
    out.extend_from_slice(b"WAVEfmt ");
    push_u32(&mut out, 16);
    push_u16(&mut out, 1);
    push_u16(&mut out, CAPTURE_PCM_CHANNELS as u16);
    push_u32(&mut out, CAPTURE_PCM_SAMPLE_RATE_HZ as u32);
    push_u32(
        &mut out,
        (CAPTURE_PCM_SAMPLE_RATE_HZ * CAPTURE_PCM_CHANNELS * core::mem::size_of::<i16>()) as u32,
    );
    push_u16(&mut out, (CAPTURE_PCM_CHANNELS * core::mem::size_of::<i16>()) as u16);
    push_u16(&mut out, 16);
    out.extend_from_slice(b"data");
    push_u32(&mut out, data_bytes_u32);
    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    Some(out)
}

/// Strict WAVE_FORMAT_IEEE_FLOAT file with a WAVEFORMATEX extension and fact
/// chunk. This is intentionally one file per native model chunk.
fn encode_f32_wav(samples: &[f32]) -> Option<Vec<u8>> {
    const HEADER_BYTES: usize = 58;
    let data_bytes = samples.len().checked_mul(core::mem::size_of::<f32>())?;
    let data_bytes_u32 = u32::try_from(data_bytes).ok()?;
    let sample_count = u32::try_from(samples.len()).ok()?;
    let riff_size = 50u32.checked_add(data_bytes_u32)?;
    let total = HEADER_BYTES.checked_add(data_bytes)?;
    let mut out = Vec::new();
    out.try_reserve_exact(total).ok()?;
    out.extend_from_slice(b"RIFF");
    push_u32(&mut out, riff_size);
    out.extend_from_slice(b"WAVEfmt ");
    push_u32(&mut out, 18);
    push_u16(&mut out, 3);
    push_u16(&mut out, 1);
    push_u32(&mut out, CAPTURE_RAW_SAMPLE_RATE_HZ as u32);
    push_u32(&mut out, (CAPTURE_RAW_SAMPLE_RATE_HZ * core::mem::size_of::<f32>()) as u32);
    push_u16(&mut out, core::mem::size_of::<f32>() as u16);
    push_u16(&mut out, 32);
    push_u16(&mut out, 0);
    out.extend_from_slice(b"fact");
    push_u32(&mut out, 4);
    push_u32(&mut out, sample_count);
    out.extend_from_slice(b"data");
    push_u32(&mut out, data_bytes_u32);
    for sample in samples {
        out.extend_from_slice(&sample.to_bits().to_le_bytes());
    }
    Some(out)
}

fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}
