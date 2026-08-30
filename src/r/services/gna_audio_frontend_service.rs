//! Low-power microphone precondition service for Intel GNA bring-up.
//!
//! The service deliberately owns policy rather than hardware. The future HDA
//! capture/GNA owner publishes compact observations through the lock-free
//! ingress functions below; this task samples them at a 100 ms soft cadence,
//! logs VAD level edges, and rate-limits wake-word evidence to one Important
//! record per 250 ms. Until a hardware owner publishes readiness or inference
//! results, the service remains visibly fail-closed in `AwaitingGna`.

use core::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, AtomicU8, Ordering};

use trueos_time::{Duration, Timer};

/// Service-side sampling cadence. The GNA owner may run its native frame clock
/// faster; only observation and logging policy is sampled at this interval.
pub const POLL_SOFTCAP_MS: u64 = 100;
/// Minimum distance between globally visible wake-word records.
pub const WAKE_LOG_SOFTCAP_MS: u64 = 250;
const CADENCE_VALIDATION_SAMPLES: u32 = 10;
const Q15_MAX: u16 = i16::MAX as u16;
const SNAPSHOT_ATTEMPTS: usize = 4;

const _: () = {
    assert!(POLL_SOFTCAP_MS != 0);
    assert!(WAKE_LOG_SOFTCAP_MS >= POLL_SOFTCAP_MS);
    assert!(CADENCE_VALIDATION_SAMPLES != 0);
};

/// Hardware/model lifecycle published by the eventual GNA owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum PipelineState {
    AwaitingGna = 0,
    AwaitingModel = 1,
    Ready = 2,
    Streaming = 3,
    Faulted = 4,
}

impl PipelineState {
    const fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::AwaitingModel,
            2 => Self::Ready,
            3 => Self::Streaming,
            4 => Self::Faulted,
            _ => Self::AwaitingGna,
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::AwaitingGna => "awaiting-gna",
            Self::AwaitingModel => "awaiting-model",
            Self::Ready => "ready",
            Self::Streaming => "streaming",
            Self::Faulted => "faulted",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrontEndStatus {
    pub pipeline: PipelineState,
    pub noise_reduction_active: bool,
    pub noise_reduction_observations: u64,
    pub vad_active: bool,
    pub vad_observations: u64,
    pub wake_events: u64,
    pub wake_logs: u64,
    pub wake_events_coalesced: u64,
}

#[derive(Clone, Copy)]
struct LevelSnapshot {
    sequence: u64,
    active: bool,
    confidence_q15: u16,
    source_timestamp_ms: u64,
}

#[derive(Clone, Copy)]
struct WakeSnapshot {
    sequence: u64,
    word_id: u32,
    confidence_q15: u16,
    source_timestamp_ms: u64,
}

#[derive(Clone, Copy)]
struct WakeLogRecord {
    wake: WakeSnapshot,
    coalesced: u64,
}

static PIPELINE_STATE: AtomicU8 = AtomicU8::new(PipelineState::AwaitingGna as u8);

// Even values are stable snapshots; odd values mean the single hardware owner
// is publishing fields. Readers never spin indefinitely if that owner is
// pre-empted: the service retries on its next 100 ms observation tick.
static NOISE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static NOISE_ACTIVE: AtomicBool = AtomicBool::new(false);
static NOISE_CONFIDENCE_Q15: AtomicU16 = AtomicU16::new(0);
static NOISE_SOURCE_TIMESTAMP_MS: AtomicU64 = AtomicU64::new(0);

static VAD_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static VAD_ACTIVE: AtomicBool = AtomicBool::new(false);
static VAD_CONFIDENCE_Q15: AtomicU16 = AtomicU16::new(0);
static VAD_SOURCE_TIMESTAMP_MS: AtomicU64 = AtomicU64::new(0);

static WAKE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static WAKE_WORD_ID: AtomicU32 = AtomicU32::new(0);
static WAKE_CONFIDENCE_Q15: AtomicU16 = AtomicU16::new(0);
static WAKE_SOURCE_TIMESTAMP_MS: AtomicU64 = AtomicU64::new(0);

static WAKE_LOGS: AtomicU64 = AtomicU64::new(0);
static WAKE_EVENTS_COALESCED: AtomicU64 = AtomicU64::new(0);

/// Publish lifecycle from the HDA/GNA hardware owner.
///
/// `AwaitingGna` is the boot default, so this service never claims that an
/// accelerator or model exists merely because its scheduler task is online.
#[allow(
    dead_code,
    reason = "GNA hardware owner integration follows this service milestone"
)]
pub(crate) fn publish_pipeline_state(state: PipelineState) {
    PIPELINE_STATE.store(state as u8, Ordering::Release);
}

/// Publish the latest noise-reduction level and confidence.
///
/// This ingress is lock-free so a future completion path can publish without
/// taking a service mutex. Concurrent publishers fail closed by returning
/// `false`; this boundary does not perform inference.
#[allow(
    dead_code,
    reason = "GNA hardware owner integration follows this service milestone"
)]
#[must_use]
pub(crate) fn publish_noise_reduction_observation(
    active: bool,
    confidence_q15: u16,
    source_timestamp_ms: u64,
) -> bool {
    publish_level(
        &NOISE_SEQUENCE,
        &NOISE_ACTIVE,
        &NOISE_CONFIDENCE_Q15,
        &NOISE_SOURCE_TIMESTAMP_MS,
        active,
        confidence_q15,
        source_timestamp_ms,
    )
}

/// Publish the latest voice-activity level and confidence.
#[allow(
    dead_code,
    reason = "GNA hardware owner integration follows this service milestone"
)]
#[must_use]
pub(crate) fn publish_vad_observation(
    active: bool,
    confidence_q15: u16,
    source_timestamp_ms: u64,
) -> bool {
    publish_level(
        &VAD_SEQUENCE,
        &VAD_ACTIVE,
        &VAD_CONFIDENCE_Q15,
        &VAD_SOURCE_TIMESTAMP_MS,
        active,
        confidence_q15,
        source_timestamp_ms,
    )
}

/// Publish one wake-word event from the admitted GNA model.
///
/// `word_id` is deliberately numeric at this boundary. Model-specific labels
/// belong to the authenticated model manifest, not an interrupt-adjacent path.
#[allow(
    dead_code,
    reason = "GNA hardware owner integration follows this service milestone"
)]
#[must_use]
pub(crate) fn publish_wake_word(
    word_id: u32,
    confidence_q15: u16,
    source_timestamp_ms: u64,
) -> bool {
    let Some(writing) = begin_publish(&WAKE_SEQUENCE) else {
        return false;
    };
    WAKE_WORD_ID.store(word_id, Ordering::Relaxed);
    WAKE_CONFIDENCE_Q15.store(confidence_q15.min(Q15_MAX), Ordering::Relaxed);
    WAKE_SOURCE_TIMESTAMP_MS.store(source_timestamp_ms, Ordering::Relaxed);
    finish_publish(&WAKE_SEQUENCE, writing);
    true
}

#[allow(
    dead_code,
    reason = "service status consumer follows this bring-up milestone"
)]
pub(crate) fn status() -> FrontEndStatus {
    let noise = load_level(
        &NOISE_SEQUENCE,
        &NOISE_ACTIVE,
        &NOISE_CONFIDENCE_Q15,
        &NOISE_SOURCE_TIMESTAMP_MS,
    );
    let vad = load_level(
        &VAD_SEQUENCE,
        &VAD_ACTIVE,
        &VAD_CONFIDENCE_Q15,
        &VAD_SOURCE_TIMESTAMP_MS,
    );
    FrontEndStatus {
        pipeline: PipelineState::from_u8(PIPELINE_STATE.load(Ordering::Acquire)),
        noise_reduction_active: noise.is_some_and(|snapshot| snapshot.active),
        noise_reduction_observations: noise.map_or(0, |snapshot| snapshot.sequence),
        vad_active: vad.is_some_and(|snapshot| snapshot.active),
        vad_observations: vad.map_or(0, |snapshot| snapshot.sequence),
        wake_events: WAKE_SEQUENCE.load(Ordering::Acquire) / 2,
        wake_logs: WAKE_LOGS.load(Ordering::Acquire),
        wake_events_coalesced: WAKE_EVENTS_COALESCED.load(Ordering::Acquire),
    }
}

fn publish_level(
    sequence: &AtomicU64,
    active_slot: &AtomicBool,
    confidence_slot: &AtomicU16,
    timestamp_slot: &AtomicU64,
    active: bool,
    confidence_q15: u16,
    source_timestamp_ms: u64,
) -> bool {
    let Some(writing) = begin_publish(sequence) else {
        return false;
    };
    active_slot.store(active, Ordering::Relaxed);
    confidence_slot.store(confidence_q15.min(Q15_MAX), Ordering::Relaxed);
    timestamp_slot.store(source_timestamp_ms, Ordering::Relaxed);
    finish_publish(sequence, writing);
    true
}

fn begin_publish(sequence: &AtomicU64) -> Option<u64> {
    for _ in 0..SNAPSHOT_ATTEMPTS {
        let stable = sequence.load(Ordering::Acquire);
        if stable & 1 != 0 {
            return None;
        }
        let writing = stable.wrapping_add(1);
        if sequence
            .compare_exchange_weak(stable, writing, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            return Some(writing);
        }
    }
    None
}

fn finish_publish(sequence: &AtomicU64, writing: u64) {
    sequence.store(writing.wrapping_add(1), Ordering::Release);
}

fn load_level(
    sequence: &AtomicU64,
    active_slot: &AtomicBool,
    confidence_slot: &AtomicU16,
    timestamp_slot: &AtomicU64,
) -> Option<LevelSnapshot> {
    for _ in 0..SNAPSHOT_ATTEMPTS {
        let before = sequence.load(Ordering::Acquire);
        if before == 0 || before & 1 != 0 {
            continue;
        }
        let snapshot = LevelSnapshot {
            sequence: before / 2,
            active: active_slot.load(Ordering::Relaxed),
            confidence_q15: confidence_slot.load(Ordering::Relaxed),
            source_timestamp_ms: timestamp_slot.load(Ordering::Relaxed),
        };
        core::sync::atomic::fence(Ordering::Acquire);
        if before == sequence.load(Ordering::Acquire) {
            return Some(snapshot);
        }
    }
    None
}

fn load_wake() -> Option<WakeSnapshot> {
    for _ in 0..SNAPSHOT_ATTEMPTS {
        let before = WAKE_SEQUENCE.load(Ordering::Acquire);
        if before == 0 || before & 1 != 0 {
            continue;
        }
        let snapshot = WakeSnapshot {
            sequence: before / 2,
            word_id: WAKE_WORD_ID.load(Ordering::Relaxed),
            confidence_q15: WAKE_CONFIDENCE_Q15.load(Ordering::Relaxed),
            source_timestamp_ms: WAKE_SOURCE_TIMESTAMP_MS.load(Ordering::Relaxed),
        };
        core::sync::atomic::fence(Ordering::Acquire);
        if before == WAKE_SEQUENCE.load(Ordering::Acquire) {
            return Some(snapshot);
        }
    }
    None
}

struct WakeLogGate {
    last_log_ms: Option<u64>,
    pending: Option<WakeSnapshot>,
    coalesced: u64,
}

impl WakeLogGate {
    const fn new() -> Self {
        Self {
            last_log_ms: None,
            pending: None,
            coalesced: 0,
        }
    }

    fn observe(
        &mut self,
        wake: WakeSnapshot,
        now_ms: u64,
        skipped_before_latest: u64,
    ) -> Option<WakeLogRecord> {
        self.record_coalesced(skipped_before_latest);
        if self.can_emit(now_ms) {
            if self.pending.take().is_some() {
                self.record_coalesced(1);
            }
            self.last_log_ms = Some(now_ms);
            let coalesced = core::mem::take(&mut self.coalesced);
            return Some(WakeLogRecord { wake, coalesced });
        }
        if self.pending.replace(wake).is_some() {
            self.record_coalesced(1);
        }
        None
    }

    fn flush(&mut self, now_ms: u64) -> Option<WakeLogRecord> {
        if !self.can_emit(now_ms) {
            return None;
        }
        let wake = self.pending.take()?;
        self.last_log_ms = Some(now_ms);
        let coalesced = core::mem::take(&mut self.coalesced);
        Some(WakeLogRecord { wake, coalesced })
    }

    fn can_emit(&self, now_ms: u64) -> bool {
        match self.last_log_ms {
            None => true,
            Some(last) => now_ms.saturating_sub(last) >= WAKE_LOG_SOFTCAP_MS,
        }
    }

    fn record_coalesced(&mut self, count: u64) {
        if count == 0 {
            return;
        }
        self.coalesced = self.coalesced.saturating_add(count);
        WAKE_EVENTS_COALESCED.fetch_add(count, Ordering::Relaxed);
    }
}

struct CadenceProbe {
    previous_ms: Option<u64>,
    samples: u32,
    min_ms: u64,
    max_ms: u64,
    total_ms: u64,
}

impl CadenceProbe {
    const fn new() -> Self {
        Self {
            previous_ms: None,
            samples: 0,
            min_ms: u64::MAX,
            max_ms: 0,
            total_ms: 0,
        }
    }

    fn observe(&mut self, now_ms: u64) -> Option<CadenceReport> {
        let Some(previous_ms) = self.previous_ms.replace(now_ms) else {
            return None;
        };
        if self.samples >= CADENCE_VALIDATION_SAMPLES {
            return None;
        }
        let delta_ms = now_ms.saturating_sub(previous_ms);
        self.samples = self.samples.saturating_add(1);
        self.min_ms = self.min_ms.min(delta_ms);
        self.max_ms = self.max_ms.max(delta_ms);
        self.total_ms = self.total_ms.saturating_add(delta_ms);
        (self.samples == CADENCE_VALIDATION_SAMPLES).then_some(CadenceReport {
            samples: self.samples,
            min_ms: self.min_ms,
            max_ms: self.max_ms,
            average_ms: self.total_ms / u64::from(self.samples),
        })
    }
}

struct CadenceReport {
    samples: u32,
    min_ms: u64,
    max_ms: u64,
    average_ms: u64,
}

fn log_wake(record: WakeLogRecord, observed_ms: u64) {
    WAKE_LOGS.fetch_add(1, Ordering::Relaxed);
    crate::log_os::service_important_line(format_args!(
        "gna-audio-front-end: event=wake-word word_id={} confidence_q15={} source_ms={} observed_ms={} sequence={} coalesced={} log_softcap_ms={}\n",
        record.wake.word_id,
        record.wake.confidence_q15,
        record.wake.source_timestamp_ms,
        observed_ms,
        record.wake.sequence,
        record.coalesced,
        WAKE_LOG_SOFTCAP_MS,
    ));
}

fn log_level(name: &str, snapshot: LevelSnapshot, observed_ms: u64) {
    crate::log_os::service_important_line(format_args!(
        "gna-audio-front-end: event={} state={} confidence_q15={} source_ms={} observed_ms={} sequence={}\n",
        name,
        if snapshot.active { "on" } else { "off" },
        snapshot.confidence_q15,
        snapshot.source_timestamp_ms,
        observed_ms,
        snapshot.sequence,
    ));
}

/// Policy/service milestone for the eventual HDA microphone → GNA 3.0 path.
///
/// No inference is synthesized here. Bare-metal logs prove scheduler cadence
/// and expose only observations explicitly published by a hardware/model owner.
#[trueos_executor::task]
pub(crate) async fn gna_audio_frontend_service_task() {
    crate::log_os::service_important_line(format_args!(
        "gna-audio-front-end: online path=hda-microphone->gna3(noise-reduction,vad,wake-word)->speech-detected sink=global-log poll_softcap_ms={} wake_log_softcap_ms={} inference=awaiting-hardware-owner fail_closed=1\n",
        POLL_SOFTCAP_MS,
        WAKE_LOG_SOFTCAP_MS,
    ));

    let mut last_pipeline = PipelineState::AwaitingGna;
    let mut last_noise_sequence = 0u64;
    let mut last_noise_active = None;
    let mut last_vad_sequence = 0u64;
    let mut last_vad_active = None;
    let mut last_wake_sequence = 0u64;
    let mut wake_log_gate = WakeLogGate::new();
    let mut cadence_probe = CadenceProbe::new();

    loop {
        let now_ms = uptime_ms();

        if let Some(report) = cadence_probe.observe(now_ms) {
            crate::log_os::service_important_line(format_args!(
                "gna-audio-front-end: baremetal=poll-cadence samples={} target_ms={} min_ms={} max_ms={} average_ms={} result=observed\n",
                report.samples,
                POLL_SOFTCAP_MS,
                report.min_ms,
                report.max_ms,
                report.average_ms,
            ));
        }

        let pipeline = PipelineState::from_u8(PIPELINE_STATE.load(Ordering::Acquire));
        if pipeline != last_pipeline {
            crate::log_os::service_important_line(format_args!(
                "gna-audio-front-end: event=pipeline-state previous={} current={} observed_ms={}\n",
                last_pipeline.as_str(),
                pipeline.as_str(),
                now_ms,
            ));
            last_pipeline = pipeline;
        }

        if let Some(snapshot) = load_level(
            &NOISE_SEQUENCE,
            &NOISE_ACTIVE,
            &NOISE_CONFIDENCE_Q15,
            &NOISE_SOURCE_TIMESTAMP_MS,
        ) && snapshot.sequence != last_noise_sequence
        {
            last_noise_sequence = snapshot.sequence;
            if last_noise_active != Some(snapshot.active) {
                log_level("noise-reduction", snapshot, now_ms);
                last_noise_active = Some(snapshot.active);
            }
        }

        if let Some(snapshot) = load_level(
            &VAD_SEQUENCE,
            &VAD_ACTIVE,
            &VAD_CONFIDENCE_Q15,
            &VAD_SOURCE_TIMESTAMP_MS,
        ) && snapshot.sequence != last_vad_sequence
        {
            last_vad_sequence = snapshot.sequence;
            if last_vad_active != Some(snapshot.active) {
                log_level("voice-activity", snapshot, now_ms);
                last_vad_active = Some(snapshot.active);
            }
        }

        if let Some(wake) = load_wake()
            && wake.sequence != last_wake_sequence
        {
            let skipped = wake
                .sequence
                .saturating_sub(last_wake_sequence)
                .saturating_sub(1);
            last_wake_sequence = wake.sequence;
            if let Some(record) = wake_log_gate.observe(wake, now_ms, skipped) {
                log_wake(record, now_ms);
            }
        }
        if let Some(record) = wake_log_gate.flush(now_ms) {
            log_wake(record, now_ms);
        }

        Timer::after(Duration::from_millis(POLL_SOFTCAP_MS)).await;
    }
}

fn uptime_ms() -> u64 {
    let hz = embassy_time_driver::TICK_HZ.max(1);
    embassy_time_driver::now().saturating_mul(1_000) / hz
}
