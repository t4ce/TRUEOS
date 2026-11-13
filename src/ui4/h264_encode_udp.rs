//! Subscriber-driven, bounded SSH egress for live UI4 H.264 access units.
//!
//! This is an encoded-access-unit stream. Intel display SURFLIVE is not part
//! of the payload or ownership contract; it remains only a scanout-latch
//! boundary elsewhere in UI4.

use alloc::{collections::VecDeque, vec::Vec};
use core::sync::atomic::{AtomicU32, Ordering};

use spin::Mutex;
use trueos_time::{Duration, Instant, Timer};

#[path = "rdp_media_queue.rs"]
mod rdp_media_queue;
static SSH_MEDIA: Mutex<rdp_media_queue::Queue> = Mutex::new(rdp_media_queue::Queue::new());
static SSH_MEDIA_OWNER_SLOT: AtomicU32 = AtomicU32::new(u32::MAX);

/// Called only after the SSH subsystem has bound its authenticated account.
pub(crate) fn rdp_media_subscribe(session: u64) -> bool {
    let mut media = SSH_MEDIA.lock();
    let admitted = media.subscribe(session);
    if admitted {
        SSH_MEDIA_OWNER_SLOT.store(crate::percpu::current_slot() as u32, Ordering::Release);
    }
    drop(media);
    if admitted {
        EGRESS_WAKE.signal(());
        PRODUCER_WAKE.signal(());
    }
    admitted
}
pub(crate) fn rdp_media_unsubscribe(session: u64) {
    SSH_MEDIA.lock().unsubscribe(session);
    EGRESS_WAKE.signal(());
    PRODUCER_WAKE.signal(());
}
pub(crate) fn rdp_media_take_packet(session: u64) -> Option<Vec<u8>> {
    SSH_MEDIA.lock().pop(session)
}

pub(super) fn current_subscriber() -> Option<u64> {
    SSH_MEDIA.lock().owner()
}

pub(super) fn subscriber_is(subscriber: u64) -> bool {
    current_subscriber() == Some(subscriber)
}

pub(super) async fn wait_producer_work() {
    PRODUCER_WAKE.wait().await;
}

const MAGIC: &[u8; 4] = b"TME1";
const VERSION: u8 = 1;
const HEADER_BYTES: usize = 32;
const DATAGRAM_BYTES: usize = 1200;
const PAYLOAD_BYTES: usize = DATAGRAM_BYTES - HEADER_BYTES;
const MAX_FRAGMENT_COUNT: usize = 4096;
const FLAG_START: u8 = 1 << 0;
const FLAG_END: u8 = 1 << 1;
const FLAG_KEYFRAME: u8 = 1 << 2;
const FLAG_SESSION_END: u8 = 1 << 3;
const ENCODED_ACCESS_UNIT_QUEUE_CAP: usize = 4;

/// Preserve an exact average cadence when the timer frequency is not evenly
/// divisible by the requested frame rate (notably 60 Hz).
pub(super) struct FractionalCadenceTicks {
    whole: u64,
    remainder: u64,
    divisor: u64,
    phase: u64,
}

impl FractionalCadenceTicks {
    pub(super) fn new(timer_hz: u64, target_hz: usize) -> Self {
        let divisor = target_hz as u64;
        Self {
            whole: (timer_hz / divisor).max(1),
            remainder: if timer_hz >= divisor {
                timer_hz % divisor
            } else {
                0
            },
            divisor,
            phase: 0,
        }
    }

    pub(super) fn next(&mut self) -> u64 {
        let mut ticks = self.whole;
        self.phase += self.remainder;
        if self.phase >= self.divisor {
            self.phase -= self.divisor;
            ticks += 1;
        }
        ticks
    }
}

#[derive(Debug)]
struct EncodedAccessUnit {
    sequence: u32,
    keyframe: bool,
    bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EgressSessionPhase {
    Idle,
    Requested,
    WaitingSubscriber,
    Ready,
    ProducerDone,
    Aborted,
    Complete,
}

#[derive(Clone, Copy)]
struct EgressSessionRequest {
    session_id: u32,
    subscriber: u64,
    access_unit_count: usize,
    target_hz: usize,
}

struct EgressPipeline {
    phase: EgressSessionPhase,
    request: Option<EgressSessionRequest>,
    session_id: u32,
    queue: VecDeque<EncodedAccessUnit>,
    queued_bytes: usize,
    queued_access_units: usize,
    high_water_access_units: usize,
    high_water_bytes: usize,
    producer_queue_wait_events: usize,
    producer_queue_wait_us: u64,
    producer_dropped_access_units: usize,
    producer_dropped_bytes: usize,
    producer_finished: bool,
    cadence_started_ns: u64,
    report: Option<MediaUdpStreamReport>,
}

impl EgressPipeline {
    const fn new() -> Self {
        Self {
            phase: EgressSessionPhase::Idle,
            request: None,
            session_id: 0,
            queue: VecDeque::new(),
            queued_bytes: 0,
            queued_access_units: 0,
            high_water_access_units: 0,
            high_water_bytes: 0,
            producer_queue_wait_events: 0,
            producer_queue_wait_us: 0,
            producer_dropped_access_units: 0,
            producer_dropped_bytes: 0,
            producer_finished: false,
            cadence_started_ns: 0,
            report: None,
        }
    }

    fn reset_for_request(&mut self, request: EgressSessionRequest) {
        self.phase = EgressSessionPhase::Requested;
        self.request = Some(request);
        self.session_id = request.session_id;
        self.queue.clear();
        self.queued_bytes = 0;
        self.queued_access_units = 0;
        self.high_water_access_units = 0;
        self.high_water_bytes = 0;
        self.producer_queue_wait_events = 0;
        self.producer_queue_wait_us = 0;
        self.producer_dropped_access_units = 0;
        self.producer_dropped_bytes = 0;
        self.producer_finished = false;
        self.cadence_started_ns = 0;
        self.report = None;
    }
}

static EGRESS_PIPELINE: Mutex<EgressPipeline> = Mutex::new(EgressPipeline::new());
static EGRESS_WAKE: embassy_sync::signal::Signal<crate::wait::EmbassySpinRawMutex, ()> =
    embassy_sync::signal::Signal::new();
static PRODUCER_WAKE: embassy_sync::signal::Signal<crate::wait::EmbassySpinRawMutex, ()> =
    embassy_sync::signal::Signal::new();

pub(super) fn wake_producer() {
    PRODUCER_WAKE.signal(());
}
static EGRESS_WORKER_SLOT: AtomicU32 = AtomicU32::new(u32::MAX);

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct MediaUdpStreamReport {
    pub(super) session_id: u32,
    pub(super) queued_access_units: usize,
    pub(super) sent_access_units: usize,
    pub(super) sent_datagrams: usize,
    pub(super) sent_payload_bytes: usize,
    pub(super) dropped_access_units: usize,
    pub(super) dropped_bytes: usize,
    pub(super) high_water_access_units: usize,
    pub(super) high_water_bytes: usize,
    pub(super) submit_retries: usize,
    pub(super) adapter_backpressure_events: usize,
    pub(super) adapter_send_errors: usize,
    pub(super) network_waits: usize,
    pub(super) subscriber_wait_polls: usize,
    pub(super) peer_addr: [u8; 4],
    pub(super) peer_port: u16,
    pub(super) elapsed_us: u64,
    pub(super) late_access_units: usize,
    pub(super) max_late_us: u64,
    pub(super) producer_queue_wait_events: usize,
    pub(super) producer_queue_wait_us: u64,
}

pub(super) const fn encoded_access_unit_queue_cap() -> usize {
    ENCODED_ACCESS_UNIT_QUEUE_CAP
}

pub(super) fn egress_worker_slot() -> u32 {
    EGRESS_WORKER_SLOT.load(Ordering::Acquire)
}

fn request_egress_session(request: EgressSessionRequest) -> bool {
    let mut pipeline = EGRESS_PIPELINE.lock();
    if pipeline.phase != EgressSessionPhase::Idle {
        return false;
    }
    pipeline.reset_for_request(request);
    drop(pipeline);
    EGRESS_WAKE.signal(());
    true
}

fn take_egress_session_request() -> Option<EgressSessionRequest> {
    let mut pipeline = EGRESS_PIPELINE.lock();
    if pipeline.phase != EgressSessionPhase::Requested {
        return None;
    }
    let request = pipeline.request.take()?;
    pipeline.phase = EgressSessionPhase::WaitingSubscriber;
    Some(request)
}

fn mark_egress_session_ready(session_id: u32) -> bool {
    let mut pipeline = EGRESS_PIPELINE.lock();
    if pipeline.session_id != session_id || pipeline.phase != EgressSessionPhase::WaitingSubscriber
    {
        return false;
    }
    pipeline.phase = EgressSessionPhase::Ready;
    drop(pipeline);
    PRODUCER_WAKE.signal(());
    true
}

fn egress_session_ready(session_id: u32) -> bool {
    let pipeline = EGRESS_PIPELINE.lock();
    pipeline.session_id == session_id && pipeline.phase == EgressSessionPhase::Ready
}

/// The producer holds the view lease across bounded encoder sessions. This
/// stream may only prepare or publish for that exact authenticated subscriber.
fn producer_active(session_id: u32, subscriber: u64) -> bool {
    if !subscriber_is(subscriber) {
        abort_egress_session(session_id);
        return false;
    }
    egress_session_ready(session_id)
}

async fn wait_prepared<R: FnMut(u32) -> bool>(
    session_id: u32,
    subscriber: u64,
    sequence: u32,
    prepared: &mut R,
) -> bool {
    loop {
        if !producer_active(session_id, subscriber) { return false; }
        if prepared(sequence) {
            return producer_active(session_id, subscriber);
        }
        PRODUCER_WAKE.wait().await;
    }
}

/// Ready can be cancelled before the producer observes the readiness wake.
/// In that case no hardware work started, but egress still needs the producer
/// acknowledgement before it can publish the final report and release view.
fn finish_cancelled_start(session_id: u32) {
    let mut pipeline = EGRESS_PIPELINE.lock();
    if pipeline.session_id != session_id || pipeline.phase != EgressSessionPhase::Aborted {
        return;
    }
    pipeline.producer_finished = true;
    drop(pipeline);
    EGRESS_WAKE.signal(());
}

fn enqueue_access_unit(session_id: u32, access_unit: EncodedAccessUnit) -> bool {
    let mut pipeline = EGRESS_PIPELINE.lock();
    if pipeline.session_id != session_id {
        return false;
    }
    if pipeline.phase != EgressSessionPhase::Ready
        || pipeline.queue.len() >= ENCODED_ACCESS_UNIT_QUEUE_CAP
    {
        pipeline.producer_dropped_access_units =
            pipeline.producer_dropped_access_units.saturating_add(1);
        pipeline.producer_dropped_bytes = pipeline
            .producer_dropped_bytes
            .saturating_add(access_unit.bytes.len());
        let full = pipeline.phase == EgressSessionPhase::Ready;
        drop(pipeline);
        if full {
            // P pictures depend on preceding pictures. End this session on
            // overload; never drop an arbitrary AU then send its dependants.
            // The next subscription starts a fresh IDR. Network delay cannot
            // keep this producer waiting with a prepared hardware frame.
            abort_egress_session(session_id);
            crate::log_warn!(target: "intel/media-encode";
                "intel/media-encode: udp queue full session={} cap={} action=abort-session-next-idr producer_wait=0 ui4_wait=none\n",
                session_id, ENCODED_ACCESS_UNIT_QUEUE_CAP,
            );
        }
        return false;
    }
    let bytes = access_unit.bytes.len();
    pipeline.queue.push_back(access_unit);
    pipeline.queued_bytes = pipeline.queued_bytes.saturating_add(bytes);
    pipeline.queued_access_units = pipeline.queued_access_units.saturating_add(1);
    pipeline.high_water_access_units = pipeline.high_water_access_units.max(pipeline.queue.len());
    pipeline.high_water_bytes = pipeline.high_water_bytes.max(pipeline.queued_bytes);
    drop(pipeline);
    EGRESS_WAKE.signal(());
    true
}

fn finish_egress_producer(session_id: u32, dropped_access_units: usize, dropped_bytes: usize) {
    let mut pipeline = EGRESS_PIPELINE.lock();
    if pipeline.session_id != session_id {
        return;
    }
    pipeline.producer_dropped_access_units = pipeline
        .producer_dropped_access_units
        .saturating_add(dropped_access_units);
    pipeline.producer_dropped_bytes = pipeline
        .producer_dropped_bytes
        .saturating_add(dropped_bytes);
    pipeline.producer_finished = true;
    if pipeline.phase == EgressSessionPhase::Ready {
        pipeline.phase = EgressSessionPhase::ProducerDone;
    }
    drop(pipeline);
    EGRESS_WAKE.signal(());
}

fn mark_egress_cadence_started(session_id: u32, started_ns: u64) {
    let mut pipeline = EGRESS_PIPELINE.lock();
    if pipeline.session_id == session_id && pipeline.phase == EgressSessionPhase::Ready {
        pipeline.cadence_started_ns = started_ns;
    }
}

fn egress_producer_finished(session_id: u32) -> bool {
    let pipeline = EGRESS_PIPELINE.lock();
    pipeline.session_id == session_id && pipeline.producer_finished
}

fn egress_elapsed_us(session_id: u32) -> u64 {
    let pipeline = EGRESS_PIPELINE.lock();
    if pipeline.session_id != session_id || pipeline.cadence_started_ns == 0 {
        return 0;
    }
    crate::chronos::monotonic_nanos().saturating_sub(pipeline.cadence_started_ns) / 1_000
}

fn take_next_egress_access_unit(session_id: u32) -> Option<Option<EncodedAccessUnit>> {
    let mut pipeline = EGRESS_PIPELINE.lock();
    if pipeline.session_id != session_id {
        return Some(None);
    }
    if let Some(access_unit) = pipeline.queue.pop_front() {
        pipeline.queued_bytes = pipeline
            .queued_bytes
            .saturating_sub(access_unit.bytes.len());
        return Some(Some(access_unit));
    }
    if matches!(pipeline.phase, EgressSessionPhase::ProducerDone | EgressSessionPhase::Aborted) {
        return Some(None);
    }
    None
}

fn abort_egress_session(session_id: u32) {
    let mut pipeline = EGRESS_PIPELINE.lock();
    if pipeline.session_id != session_id {
        return;
    }
    while let Some(access_unit) = pipeline.queue.pop_front() {
        pipeline.producer_dropped_access_units =
            pipeline.producer_dropped_access_units.saturating_add(1);
        pipeline.producer_dropped_bytes = pipeline
            .producer_dropped_bytes
            .saturating_add(access_unit.bytes.len());
    }
    pipeline.queued_bytes = 0;
    pipeline.phase = EgressSessionPhase::Aborted;
    drop(pipeline);
    EGRESS_WAKE.signal(());
    PRODUCER_WAKE.signal(());
}

fn complete_egress_session(session_id: u32, mut report: MediaUdpStreamReport) {
    let mut pipeline = EGRESS_PIPELINE.lock();
    if pipeline.session_id != session_id {
        return;
    }
    report.queued_access_units = pipeline.queued_access_units;
    report.high_water_access_units = pipeline.high_water_access_units;
    report.high_water_bytes = pipeline.high_water_bytes;
    report.producer_queue_wait_events = pipeline.producer_queue_wait_events;
    report.producer_queue_wait_us = pipeline.producer_queue_wait_us;
    report.dropped_access_units = report
        .dropped_access_units
        .saturating_add(pipeline.producer_dropped_access_units);
    report.dropped_bytes = report
        .dropped_bytes
        .saturating_add(pipeline.producer_dropped_bytes);
    pipeline.queue.clear();
    pipeline.report = Some(report);
    pipeline.phase = EgressSessionPhase::Complete;
    drop(pipeline);
    PRODUCER_WAKE.signal(());
}

fn take_egress_report(session_id: u32) -> Option<MediaUdpStreamReport> {
    let mut pipeline = EGRESS_PIPELINE.lock();
    if pipeline.session_id != session_id || pipeline.phase != EgressSessionPhase::Complete {
        return None;
    }
    let report = pipeline.report.take()?;
    pipeline.phase = EgressSessionPhase::Idle;
    pipeline.session_id = 0;
    Some(report)
}

/// Encode on absolute `target_hz` deadlines and publish complete Annex-B
/// access units into the bounded queue owned by the independent UDP worker.
/// The first prepared frame is excluded from cadence timing.
/// Publish each following capture deadline before encoding can release the
/// sole capture target. A refused schedule aborts the bounded session.
pub(super) async fn stream_generated_annex_b<B, S, R, F>(
    session_id: u32,
    subscriber: u64,
    access_unit_count: usize,
    target_hz: usize,
    begin_preparation: B,
    mut schedule_next_capture: S,
    mut prepared: R,
    mut generate: F,
) -> MediaUdpStreamReport
where
    B: FnOnce() -> bool,
    S: FnMut(u32, Instant) -> bool,
    R: FnMut(u32) -> bool,
    F: AsyncFnMut(u32) -> Option<Vec<u8>>,
{
    if access_unit_count == 0 || target_hz == 0 {
        return MediaUdpStreamReport {
            session_id,
            ..MediaUdpStreamReport::default()
        };
    }

    let request = EgressSessionRequest {
        session_id,
        subscriber,
        access_unit_count,
        target_hz,
    };
    while !request_egress_session(request) {
        PRODUCER_WAKE.wait().await;
    }
    loop {
        if egress_session_ready(session_id) {
            break;
        }
        finish_cancelled_start(session_id);
        if let Some(report) = take_egress_report(session_id) {
            return report;
        }
        PRODUCER_WAKE.wait().await;
    }

    let mut started_ns = None;
    let mut late_access_units = 0usize;
    let mut max_late_us = 0u64;
    let mut producer_dropped_access_units = 0usize;
    let mut producer_dropped_bytes = 0usize;
    'production: {
        if !producer_active(session_id, subscriber) { break 'production; }
        let prefill_started_ns = crate::chronos::monotonic_nanos();
        if !begin_preparation() {
            abort_egress_session(session_id);
            producer_dropped_access_units += 1;
            break 'production;
        }
        if !wait_prepared(session_id, subscriber, 0, &mut prepared).await { break 'production; }
        let prefill_us = crate::chronos::monotonic_nanos().saturating_sub(prefill_started_ns) / 1_000;
        crate::log_info!(target: "intel/media-encode";
            "intel/media-encode: udp-live preparation=prefilled session={} sequence=0 prefill_us={} buffering=single action=start-cadence\n",
            session_id, prefill_us,
        );

        let mut cadence = FractionalCadenceTicks::new(trueos_time::TICK_HZ, target_hz);
        let cadence_started_ns = crate::chronos::monotonic_nanos();
        started_ns = Some(cadence_started_ns);
        mark_egress_cadence_started(session_id, cadence_started_ns);
        let mut next_deadline = Instant::now();
        for index in 0..access_unit_count {
            if !producer_active(session_id, subscriber) { break; }
            if index != 0 {
                let now = Instant::now();
                if now < next_deadline {
                    Timer::at(next_deadline).await;
                }
            }

            let sequence = index as u32;
            if !wait_prepared(session_id, subscriber, sequence, &mut prepared).await { break; }
            if index != 0 {
                let now = Instant::now();
                if now > next_deadline {
                    late_access_units = late_access_units.saturating_add(1);
                    let late_us = now.saturating_duration_since(next_deadline).as_micros();
                    max_late_us = max_late_us.max(late_us);
                    // Rebase instead of emitting a catch-up burst.
                    next_deadline = now;
                }
            }

            // Publish the next capture's deadline before this encode future
            // can retire and release the sole WD target to its prepare worker.
            // Advance the fractional cadence once, after any lateness rebase.
            if index + 1 < access_unit_count {
                next_deadline += Duration::from_ticks(cadence.next());
                if !schedule_next_capture(sequence + 1, next_deadline) {
                    abort_egress_session(session_id);
                    break;
                }
                if !producer_active(session_id, subscriber) { break; }
            }

            let Some(bytes) = generate(sequence).await else {
                producer_dropped_access_units = producer_dropped_access_units.saturating_add(1);
                break;
            };
            // A submitted encoder future must retire normally even on disconnect.
            // Once it returns, never publish its output for a replaced subscriber.
            if !producer_active(session_id, subscriber) {
                producer_dropped_access_units += 1;
                producer_dropped_bytes = producer_dropped_bytes.saturating_add(bytes.len());
                break;
            }
            if bytes.len() > crate::allcaps::media_encode::STREAM_MAX_ACCESS_UNIT_BYTES {
                producer_dropped_access_units = producer_dropped_access_units.saturating_add(1);
                producer_dropped_bytes = producer_dropped_bytes.saturating_add(bytes.len());
                break;
            }
            let Some(keyframe) = annex_b_access_unit_keyframe(bytes.as_slice()) else {
                producer_dropped_access_units = producer_dropped_access_units.saturating_add(1);
                producer_dropped_bytes = producer_dropped_bytes.saturating_add(bytes.len());
                break;
            };
            let access_unit = EncodedAccessUnit {
                sequence,
                keyframe,
                bytes,
            };
            if !enqueue_access_unit(session_id, access_unit) {
                break;
            }
        }
    }
    finish_egress_producer(session_id, producer_dropped_access_units, producer_dropped_bytes);
    let mut report = loop {
        if let Some(report) = take_egress_report(session_id) {
            break report;
        }
        PRODUCER_WAKE.wait().await;
    };
    if report.elapsed_us == 0 {
        if let Some(started_ns) = started_ns {
            report.elapsed_us = crate::chronos::monotonic_nanos().saturating_sub(started_ns) / 1_000;
        }
    }
    report.late_access_units = late_access_units;
    report.max_late_us = max_late_us;
    report
}

#[trueos_executor::task(pool_size = 1)]
pub(crate) async fn ui4_h264_encode_udp_egress_task() {
    let worker = crate::cpu::CpuProfile::current();
    let worker_slot = worker.map(|profile| profile.slot()).unwrap_or(u32::MAX);
    let worker_kind = worker
        .map(|profile| profile.core_kind_name())
        .unwrap_or("unknown");
    EGRESS_WORKER_SLOT.store(worker_slot, Ordering::Release);
    crate::log_info!(target: "intel/media-encode";
        "intel/media-encode: ssh-egress service online carrier=ordinary-cooperative worker_slot={} worker_kind={} queue_cap={} ingress=one-way-bounded-au-handoff ownership=authenticated-session+bounded-fragments ordering=session-sequence backpressure=abort-session-next-idr-no-producer-wait\n",
        worker_slot,
        worker_kind,
        ENCODED_ACCESS_UNIT_QUEUE_CAP,
    );

    loop {
        let request = loop {
            if let Some(request) = take_egress_session_request() {
                break request;
            }
            EGRESS_WAKE.wait().await;
        };
        let report = run_egress_session(request).await;
        complete_egress_session(request.session_id, report);
    }
}

async fn run_egress_session(request: EgressSessionRequest) -> MediaUdpStreamReport {
    let mut report = MediaUdpStreamReport { session_id: request.session_id, ..MediaUdpStreamReport::default() };
    // The producer owns the view lease continuously across bounded encoder
    // sessions. Egress only serves the exact subscriber bound to this request.
    let subscriber = request.subscriber;
    if !subscriber_is(subscriber) || !mark_egress_session_ready(request.session_id) {
        abort_egress_session(request.session_id);
        while !egress_producer_finished(request.session_id) { EGRESS_WAKE.wait().await; }
        return report;
    }
    crate::log_info!(target: "intel/media-encode";
        "intel/media-encode: ssh-live subscriber=accepted session={} ssh_session={} target_fps={} target_units={} protocol=tme1-in-ssh public_udp=disabled\n",
        request.session_id, subscriber, request.target_hz, request.access_unit_count);
    let mut datagram_sequence = 0u32;
    loop {
        if !subscriber_is(subscriber) {
            abort_egress_session(request.session_id);
            break;
        }
        let Some(next) = take_next_egress_access_unit(request.session_id) else {
            EGRESS_WAKE.wait().await;
            continue;
        };
        let Some(access_unit) = next else { break; };
        let session_end = access_unit.sequence as usize + 1 == request.access_unit_count;
        if !send_access_unit(subscriber, &access_unit, session_end, &mut datagram_sequence, &mut report) {
            report.dropped_access_units += 1;
            report.dropped_bytes += access_unit.bytes.len();
            // Do not retain a partial IDR or dependent P pictures on overload.
            // The producer starts the next bounded session with a fresh IDR.
            SSH_MEDIA.lock().clear(subscriber);
            abort_egress_session(request.session_id);
            break;
        }
        report.sent_access_units += 1;
    }
    while !egress_producer_finished(request.session_id) { EGRESS_WAKE.wait().await; }
    report.elapsed_us = egress_elapsed_us(request.session_id);
    report
}

fn send_access_unit(
    subscriber: u64,
    access_unit: &EncodedAccessUnit,
    session_end: bool,
    datagram_sequence: &mut u32,
    report: &mut MediaUdpStreamReport,
) -> bool {
    let fragment_count = access_unit.bytes.len().div_ceil(PAYLOAD_BYTES);
    if fragment_count == 0 || fragment_count > MAX_FRAGMENT_COUNT { return false; }
    for (fragment_index, payload) in access_unit.bytes.chunks(PAYLOAD_BYTES).enumerate() {
        let mut packet = alloc::vec![0; HEADER_BYTES + payload.len()];
        let mut flags = if access_unit.keyframe { FLAG_KEYFRAME } else { 0 };
        if fragment_index == 0 { flags |= FLAG_START; }
        if fragment_index + 1 == fragment_count {
            flags |= FLAG_END;
            if session_end { flags |= FLAG_SESSION_END; }
        }
        encode_header(&mut packet[..HEADER_BYTES], flags, report.session_id,
            *datagram_sequence, access_unit.sequence, fragment_index as u16,
            fragment_count as u16, payload);
        packet[HEADER_BYTES..].copy_from_slice(payload);
        if !SSH_MEDIA.lock().push(subscriber, packet) { return false; }
        *datagram_sequence = datagram_sequence.wrapping_add(1);
        report.sent_datagrams += 1;
        report.sent_payload_bytes += payload.len();
    }
    // One wake per completed AU. The queue and session ownership are already
    // published; the SSH owner can now drain without a terminal polling tick.
    crate::shell3::net::notify_owner(SSH_MEDIA_OWNER_SLOT.load(Ordering::Acquire));
    true
}

fn annex_b_access_unit_keyframe(annex_b: &[u8]) -> Option<bool> {
    let mut cursor = 0usize;
    while cursor + 4 <= annex_b.len() {
        let start_code_bytes =
            if cursor + 5 <= annex_b.len() && annex_b[cursor..cursor + 4] == [0, 0, 0, 1] {
                4
            } else if annex_b[cursor..cursor + 3] == [0, 0, 1] {
                3
            } else {
                cursor += 1;
                continue;
            };
        match annex_b[cursor + start_code_bytes] & 0x1f {
            5 => return Some(true),
            1 => return Some(false),
            _ => {}
        }
        cursor += start_code_bytes + 1;
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn encode_header(
    header: &mut [u8],
    flags: u8,
    session_id: u32,
    datagram_sequence: u32,
    access_unit_sequence: u32,
    fragment_index: u16,
    fragment_count: u16,
    payload: &[u8],
) {
    header.fill(0);
    header[0..4].copy_from_slice(MAGIC);
    header[4] = VERSION;
    header[5] = flags;
    header[6..8].copy_from_slice(&(HEADER_BYTES as u16).to_be_bytes());
    header[8..12].copy_from_slice(&session_id.to_be_bytes());
    header[12..16].copy_from_slice(&datagram_sequence.to_be_bytes());
    header[16..20].copy_from_slice(&access_unit_sequence.to_be_bytes());
    header[20..22].copy_from_slice(&fragment_index.to_be_bytes());
    header[22..24].copy_from_slice(&fragment_count.to_be_bytes());
    header[24..26].copy_from_slice(&(payload.len() as u16).to_be_bytes());
    header[28..32].copy_from_slice(&crc32fast::hash(payload).to_be_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sixty_hz_fractional_period_preserves_one_second() {
        let mut cadence = FractionalCadenceTicks::new(1_000, 60);
        let ticks = (0..60).map(|_| cadence.next()).sum::<u64>();
        assert_eq!(ticks, 1_000);
    }
}
