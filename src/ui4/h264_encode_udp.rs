//! Subscriber-driven, bounded UDP egress for live UI4 H.264 access units.
//!
//! This is an encoded-access-unit stream. Intel display SURFLIVE is not part
//! of the payload or ownership contract; it remains only a scanout-latch
//! boundary elsewhere in UI4.

use alloc::{collections::VecDeque, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_time::{Duration, Instant, Timer};
use spin::Mutex;

use crate::r::net::{
    SharedNetPayload, VNet,
    udp::{VNetUdpEndpoint, VNetUdpEvent, VNetUdpPacket},
};

const MAGIC: &[u8; 4] = b"TME1";
const SUBSCRIBE: &[u8; 8] = b"TME1GET1";
const VERSION: u8 = 1;
const HEADER_BYTES: usize = 32;
const DATAGRAM_BYTES: usize = 1200;
const PAYLOAD_BYTES: usize = DATAGRAM_BYTES - HEADER_BYTES;
const MAX_FRAGMENT_COUNT: usize = 4096;
const FLAG_START: u8 = 1 << 0;
const FLAG_END: u8 = 1 << 1;
const FLAG_KEYFRAME: u8 = 1 << 2;
const FLAG_SESSION_END: u8 = 1 << 3;
const UDP_OPEN_TIMEOUT_MS: u64 = 4_000;
const UDP_RETRY_MS: u64 = 250;
// Keep enough socket-local payload headroom for several MTU-sized fragments.
// Correctness still comes from checked adapter receipts below; this capacity
// only absorbs short service-task scheduling jitter.
const UDP_TX_BUFFER_BYTES: usize = 64 * 1024;
const UDP_RETRY_DELAY_MS: u64 = 1;
const UDP_SEND_RECEIPT_TIMEOUT_MS: u64 = 250;
// VNet submission is asynchronous. Keep the endpoint alive across at least
// one adapter service interval so a one-datagram probe is not cancelled by
// closing its socket immediately after queueing the packet.
const UDP_CLOSE_LINGER_MS: u64 = 100;
const UDP_SUBSCRIBER_POLL_MS: u64 = 10;
const PREPARED_FRAME_POLL_MS: u64 = 1;
const UDP_SUBMIT_RETRY_LIMIT: usize = 64;
const ENCODED_ACCESS_UNIT_QUEUE_CAP: usize = 4;
const EGRESS_QUEUE_POLL_MS: u64 = 1;
// The adapter's UDP socket allocates eight TX packet-metadata entries. Match
// that exact capacity: one eight-fragment window occupies at most 9,600 bytes
// of the 64 KiB byte ring and can be admitted in one network-service turn.
const UDP_RECEIPT_WINDOW_FRAGMENTS: usize = 8;

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
static EGRESS_WORKER_SLOT: AtomicU32 = AtomicU32::new(u32::MAX);

struct PendingDatagram {
    receipt: u32,
    packet: SharedNetPayload,
    payload_bytes: usize,
    retries: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CheckedSendReceipt {
    Accepted,
    Backpressure,
    Failed,
}

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

/// One boot-lifetime VNet registration shared by every bounded media session.
///
/// VNet application queues are intentionally registered for the lifetime of
/// the kernel, so reopening a VNet for every receiver would permanently grow
/// the adapter's queue registry. UDP endpoints remain per-session: closing and
/// rebinding them still requires every new session to send a fresh subscription.
pub(super) struct MediaUdpTransport {
    net: VNet,
    pending_open_waits: usize,
}

impl MediaUdpTransport {
    pub(super) async fn open() -> Self {
        crate::r::readiness::wait_for(crate::r::readiness::NET_V4_CONFIGURED).await;
        let mut pending_open_waits = 0usize;
        let net = loop {
            let Some(device_index) = crate::r::net::NetProfile::default().resolve_device_index()
            else {
                pending_open_waits = pending_open_waits.saturating_add(1);
                Timer::after(Duration::from_millis(UDP_RETRY_MS)).await;
                continue;
            };
            let Some(net) = VNet::open_with_event_queue_depth(device_index, 64) else {
                pending_open_waits = pending_open_waits.saturating_add(1);
                Timer::after(Duration::from_millis(UDP_RETRY_MS)).await;
                continue;
            };
            break net;
        };
        Self {
            net,
            pending_open_waits,
        }
    }
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
    true
}

fn egress_session_ready(session_id: u32) -> bool {
    let pipeline = EGRESS_PIPELINE.lock();
    pipeline.session_id == session_id && pipeline.phase == EgressSessionPhase::Ready
}

async fn enqueue_access_unit(session_id: u32, access_unit: EncodedAccessUnit) -> bool {
    let mut access_unit = Some(access_unit);
    let wait_started_ns = crate::chronos::monotonic_nanos();
    let mut waited = false;
    loop {
        {
            let mut pipeline = EGRESS_PIPELINE.lock();
            if pipeline.session_id != session_id || pipeline.phase != EgressSessionPhase::Ready {
                let dropped_bytes = access_unit
                    .as_ref()
                    .map(|unit| unit.bytes.len())
                    .unwrap_or(0);
                pipeline.producer_dropped_access_units =
                    pipeline.producer_dropped_access_units.saturating_add(1);
                pipeline.producer_dropped_bytes = pipeline
                    .producer_dropped_bytes
                    .saturating_add(dropped_bytes);
                return false;
            }
            if pipeline.phase == EgressSessionPhase::Ready
                && pipeline.queue.len() < ENCODED_ACCESS_UNIT_QUEUE_CAP
            {
                let access_unit = access_unit.take().expect("egress access unit retained");
                let access_unit_bytes = access_unit.bytes.len();
                pipeline.queue.push_back(access_unit);
                pipeline.queued_bytes = pipeline.queued_bytes.saturating_add(access_unit_bytes);
                pipeline.queued_access_units = pipeline.queued_access_units.saturating_add(1);
                pipeline.high_water_access_units =
                    pipeline.high_water_access_units.max(pipeline.queue.len());
                pipeline.high_water_bytes = pipeline.high_water_bytes.max(pipeline.queued_bytes);
                if waited {
                    pipeline.producer_queue_wait_events =
                        pipeline.producer_queue_wait_events.saturating_add(1);
                    pipeline.producer_queue_wait_us =
                        pipeline.producer_queue_wait_us.saturating_add(
                            crate::chronos::monotonic_nanos().saturating_sub(wait_started_ns)
                                / 1_000,
                        );
                }
                return true;
            }
        }
        waited = true;
        Timer::after(Duration::from_millis(EGRESS_QUEUE_POLL_MS)).await;
    }
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
pub(super) async fn stream_generated_annex_b<B, R, F>(
    session_id: u32,
    access_unit_count: usize,
    target_hz: usize,
    begin_preparation: B,
    mut prepared: R,
    mut generate: F,
) -> MediaUdpStreamReport
where
    B: FnOnce(),
    R: FnMut(u32) -> bool,
    F: FnMut(u32) -> Option<Vec<u8>>,
{
    if access_unit_count == 0 || target_hz == 0 {
        return MediaUdpStreamReport {
            session_id,
            ..MediaUdpStreamReport::default()
        };
    }

    let request = EgressSessionRequest {
        session_id,
        access_unit_count,
        target_hz,
    };
    while !request_egress_session(request) {
        Timer::after(Duration::from_millis(EGRESS_QUEUE_POLL_MS)).await;
    }
    loop {
        if egress_session_ready(session_id) {
            break;
        }
        if let Some(report) = take_egress_report(session_id) {
            return report;
        }
        Timer::after(Duration::from_millis(EGRESS_QUEUE_POLL_MS)).await;
    }

    let prefill_started_ns = crate::chronos::monotonic_nanos();
    begin_preparation();
    while !prepared(0) {
        Timer::after(Duration::from_millis(PREPARED_FRAME_POLL_MS)).await;
    }
    let prefill_us = crate::chronos::monotonic_nanos().saturating_sub(prefill_started_ns) / 1_000;
    crate::log_info!(target: "intel/media-encode";
        "intel/media-encode: udp-live preparation=prefilled session={} sequence=0 prefill_us={} buffering=double action=start-cadence\n",
        session_id,
        prefill_us,
    );

    let period_ticks = (embassy_time::TICK_HZ / target_hz as u64).max(1);
    let started_ns = crate::chronos::monotonic_nanos();
    mark_egress_cadence_started(session_id, started_ns);
    let mut next_deadline = Instant::now();
    let mut late_access_units = 0usize;
    let mut max_late_us = 0u64;
    let mut producer_dropped_access_units = 0usize;
    let mut producer_dropped_bytes = 0usize;
    for index in 0..access_unit_count {
        if index != 0 {
            next_deadline += Duration::from_ticks(period_ticks);
            let now = Instant::now();
            if now < next_deadline {
                Timer::at(next_deadline).await;
            }
        }

        let sequence = index as u32;
        while !prepared(sequence) {
            Timer::after(Duration::from_millis(PREPARED_FRAME_POLL_MS)).await;
        }
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

        let Some(bytes) = generate(sequence) else {
            producer_dropped_access_units = producer_dropped_access_units.saturating_add(1);
            break;
        };
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
        if !enqueue_access_unit(session_id, access_unit).await {
            break;
        }
    }
    finish_egress_producer(session_id, producer_dropped_access_units, producer_dropped_bytes);
    let mut report = loop {
        if let Some(report) = take_egress_report(session_id) {
            break report;
        }
        Timer::after(Duration::from_millis(EGRESS_QUEUE_POLL_MS)).await;
    };
    if report.elapsed_us == 0 {
        report.elapsed_us = crate::chronos::monotonic_nanos().saturating_sub(started_ns) / 1_000;
    }
    report.late_access_units = late_access_units;
    report.max_late_us = max_late_us;
    report
}

#[embassy_executor::task(pool_size = 1)]
pub(crate) async fn ui4_h264_encode_udp_egress_task(assigned_slot: u32) {
    let worker = crate::cpu::CpuProfile::current();
    let worker_slot = worker.map(|profile| profile.slot()).unwrap_or(u32::MAX);
    let worker_kind = worker
        .map(|profile| profile.core_kind_name())
        .unwrap_or("unknown");
    EGRESS_WORKER_SLOT.store(worker_slot, Ordering::Release);
    crate::log_info!(target: "intel/media-encode";
        "intel/media-encode: udp-egress service online carrier=lastap assigned_slot={} worker_slot={} worker_kind={} queue_cap={} ownership=fragment+checked-send-receipts ordering=session-sequence backpressure=bounded-wait-no-drop\n",
        assigned_slot,
        worker_slot,
        worker_kind,
        ENCODED_ACCESS_UNIT_QUEUE_CAP,
    );
    if worker_slot != assigned_slot {
        crate::log_error!(target: "intel/media-encode";
            "intel/media-encode: udp-egress service rejected assigned_slot={} actual_slot={} reason=executor-residency-mismatch action=park\n",
            assigned_slot,
            worker_slot,
        );
        loop {
            Timer::after(Duration::from_secs(3_600)).await;
        }
    }

    let mut transport = MediaUdpTransport::open().await;
    loop {
        let request = loop {
            if let Some(request) = take_egress_session_request() {
                break request;
            }
            Timer::after(Duration::from_millis(EGRESS_QUEUE_POLL_MS)).await;
        };
        let report = run_egress_session(&mut transport, request).await;
        complete_egress_session(request.session_id, report);
    }
}

async fn run_egress_session(
    transport: &mut MediaUdpTransport,
    request: EgressSessionRequest,
) -> MediaUdpStreamReport {
    let mut report = MediaUdpStreamReport {
        session_id: request.session_id,
        network_waits: core::mem::take(&mut transport.pending_open_waits),
        ..MediaUdpStreamReport::default()
    };
    crate::log_info!(target: "intel/media-encode";
        "intel/media-encode: udp-live staged=1 protocol=tme1 version={} session={} payload=annexb-access-units target_fps={} target_units={} live_high_water_cap={} pipeline=encode-producer+independent-egress-consumer transport=udp-subscribe-unicast listen_port={} subscriber_token=TME1GET1 network=waiting\n",
        VERSION,
        request.session_id,
        request.target_hz,
        request.access_unit_count,
        ENCODED_ACCESS_UNIT_QUEUE_CAP,
        crate::allports::services::MEDIA_ENCODE_UDP_PORT,
    );

    let net = &transport.net;
    let mut udp = loop {
        let Some(udp) = VNetUdpEndpoint::bind_with_tx_buffer(
            net,
            crate::allports::services::MEDIA_ENCODE_UDP_PORT,
            UDP_TX_BUFFER_BYTES,
            Duration::from_millis(UDP_OPEN_TIMEOUT_MS),
        )
        .await
        else {
            report.network_waits = report.network_waits.saturating_add(1);
            Timer::after(Duration::from_millis(UDP_RETRY_MS)).await;
            continue;
        };
        break udp;
    };
    let remote = loop {
        match udp.poll_event() {
            Some(VNetUdpEvent::Packet(VNetUdpPacket::V4 { from, data }))
                if data.as_slice() == SUBSCRIBE =>
            {
                break from;
            }
            Some(VNetUdpEvent::Closed) => {
                report.network_waits = report.network_waits.saturating_add(1);
                udp = loop {
                    let Some(reopened) = VNetUdpEndpoint::bind_with_tx_buffer(
                        net,
                        crate::allports::services::MEDIA_ENCODE_UDP_PORT,
                        UDP_TX_BUFFER_BYTES,
                        Duration::from_millis(UDP_OPEN_TIMEOUT_MS),
                    )
                    .await
                    else {
                        report.network_waits = report.network_waits.saturating_add(1);
                        Timer::after(Duration::from_millis(UDP_RETRY_MS)).await;
                        continue;
                    };
                    break reopened;
                };
            }
            Some(_) | None => {
                report.subscriber_wait_polls = report.subscriber_wait_polls.saturating_add(1);
                Timer::after(Duration::from_millis(UDP_SUBSCRIBER_POLL_MS)).await;
            }
        }
    };
    report.peer_addr = remote.addr;
    report.peer_port = remote.port;
    crate::log_info!(target: "intel/media-encode";
        "intel/media-encode: udp-live subscriber=accepted session={} peer={}.{}.{}.{}:{} wait_polls={} action=signal-encoder-and-drain-bounded-au-queue\n",
        request.session_id,
        remote.addr[0],
        remote.addr[1],
        remote.addr[2],
        remote.addr[3],
        remote.port,
        report.subscriber_wait_polls,
    );
    if !mark_egress_session_ready(request.session_id) {
        abort_egress_session(request.session_id);
        udp.close();
        return report;
    }

    let mut datagram_sequence = 0u32;
    loop {
        let Some(next) = take_next_egress_access_unit(request.session_id) else {
            Timer::after(Duration::from_millis(EGRESS_QUEUE_POLL_MS)).await;
            continue;
        };
        let Some(access_unit) = next else {
            break;
        };
        let session_end = access_unit.sequence as usize + 1 == request.access_unit_count;
        if !send_access_unit(
            &udp,
            remote,
            &access_unit,
            session_end,
            &mut datagram_sequence,
            &mut report,
        )
        .await
        {
            report.dropped_access_units = report.dropped_access_units.saturating_add(1);
            report.dropped_bytes = report.dropped_bytes.saturating_add(access_unit.bytes.len());
            abort_egress_session(request.session_id);
            break;
        }
        report.sent_access_units = report.sent_access_units.saturating_add(1);
    }

    while !egress_producer_finished(request.session_id) {
        Timer::after(Duration::from_millis(EGRESS_QUEUE_POLL_MS)).await;
    }
    report.elapsed_us = egress_elapsed_us(request.session_id);
    Timer::after(Duration::from_millis(UDP_CLOSE_LINGER_MS)).await;
    udp.close();
    drop(udp);
    report
}

async fn send_access_unit(
    udp: &VNetUdpEndpoint<'_>,
    remote: v::vnet::EndpointV4,
    access_unit: &EncodedAccessUnit,
    session_end: bool,
    datagram_sequence: &mut u32,
    report: &mut MediaUdpStreamReport,
) -> bool {
    let fragment_count = access_unit.bytes.len().div_ceil(PAYLOAD_BYTES);
    if fragment_count == 0 || fragment_count > MAX_FRAGMENT_COUNT {
        return false;
    }

    for window_start in (0..fragment_count).step_by(UDP_RECEIPT_WINDOW_FRAGMENTS) {
        let window_end =
            fragment_count.min(window_start.saturating_add(UDP_RECEIPT_WINDOW_FRAGMENTS));
        let mut pending = Vec::with_capacity(window_end.saturating_sub(window_start));
        for fragment_index in window_start..window_end {
            let payload_start = fragment_index.saturating_mul(PAYLOAD_BYTES);
            let payload_end = access_unit
                .bytes
                .len()
                .min(payload_start.saturating_add(PAYLOAD_BYTES));
            let payload = &access_unit.bytes[payload_start..payload_end];
            let mut packet = Arc::new([0u8; DATAGRAM_BYTES]);
            let Some(packet_bytes) = Arc::get_mut(&mut packet) else {
                return false;
            };
            let mut flags = if access_unit.keyframe {
                FLAG_KEYFRAME
            } else {
                0
            };
            if fragment_index == 0 {
                flags |= FLAG_START;
            }
            if fragment_index + 1 == fragment_count {
                flags |= FLAG_END;
                if session_end {
                    flags |= FLAG_SESSION_END;
                }
            }
            let receipt =
                datagram_sequence.wrapping_add(fragment_index as u32 - window_start as u32);
            encode_header(
                &mut packet_bytes[..HEADER_BYTES],
                flags,
                report.session_id,
                receipt,
                access_unit.sequence,
                fragment_index as u16,
                fragment_count as u16,
                payload,
            );
            packet_bytes[HEADER_BYTES..HEADER_BYTES + payload.len()].copy_from_slice(payload);
            let packet_len = HEADER_BYTES + payload.len();
            let Some(packet) = SharedNetPayload::from_arc_prefix(packet, packet_len) else {
                return false;
            };
            pending.push(PendingDatagram {
                receipt,
                packet,
                payload_bytes: payload.len(),
                retries: 0,
            });
        }

        for datagram in &mut pending {
            if !submit_checked_datagram(udp, remote, datagram, report).await {
                return false;
            }
        }
        for datagram in &mut pending {
            loop {
                match wait_for_checked_send_receipt(udp, datagram.receipt).await {
                    Some(CheckedSendReceipt::Accepted) => {
                        report.sent_datagrams = report.sent_datagrams.saturating_add(1);
                        report.sent_payload_bytes = report
                            .sent_payload_bytes
                            .saturating_add(datagram.payload_bytes);
                        *datagram_sequence = datagram_sequence.wrapping_add(1);
                        break;
                    }
                    Some(CheckedSendReceipt::Backpressure) => {
                        report.adapter_backpressure_events =
                            report.adapter_backpressure_events.saturating_add(1);
                        datagram.retries = datagram.retries.saturating_add(1);
                        report.submit_retries = report.submit_retries.saturating_add(1);
                        if datagram.retries >= UDP_SUBMIT_RETRY_LIMIT {
                            return false;
                        }
                        Timer::after(Duration::from_millis(UDP_RETRY_DELAY_MS)).await;
                        if !submit_checked_datagram(udp, remote, datagram, report).await {
                            return false;
                        }
                    }
                    Some(CheckedSendReceipt::Failed) | None => {
                        // A missing receipt has an unknown disposition. Abort
                        // instead of risking a duplicate by resubmitting it.
                        report.adapter_send_errors = report.adapter_send_errors.saturating_add(1);
                        return false;
                    }
                }
            }
        }
    }
    true
}

async fn submit_checked_datagram(
    udp: &VNetUdpEndpoint<'_>,
    remote: v::vnet::EndpointV4,
    datagram: &mut PendingDatagram,
    report: &mut MediaUdpStreamReport,
) -> bool {
    loop {
        if udp
            .send_v4_checked(remote, datagram.receipt, &datagram.packet)
            .is_ok()
        {
            return true;
        }
        datagram.retries = datagram.retries.saturating_add(1);
        report.submit_retries = report.submit_retries.saturating_add(1);
        if datagram.retries >= UDP_SUBMIT_RETRY_LIMIT {
            return false;
        }
        Timer::after(Duration::from_millis(UDP_RETRY_DELAY_MS)).await;
    }
}

async fn wait_for_checked_send_receipt(
    udp: &VNetUdpEndpoint<'_>,
    receipt: u32,
) -> Option<CheckedSendReceipt> {
    let deadline = Instant::now() + Duration::from_millis(UDP_SEND_RECEIPT_TIMEOUT_MS);
    loop {
        if let Some(result) = udp.poll_checked_send_result(receipt) {
            return Some(classify_checked_send_result(result));
        }
        if Instant::now() >= deadline {
            return None;
        }
        // A zero-duration timer is a cooperative executor yield. It lets the
        // adapter service consume the checked command without imposing the
        // kernel's one-millisecond clock tick on every fragment.
        Timer::after(Duration::from_micros(0)).await;
    }
}

fn classify_checked_send_result(result: Result<(), &'static str>) -> CheckedSendReceipt {
    match result {
        Ok(()) => CheckedSendReceipt::Accepted,
        Err("udp send fail") => CheckedSendReceipt::Backpressure,
        Err(_) => CheckedSendReceipt::Failed,
    }
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
    fn checked_receipt_separates_acceptance_backpressure_and_failure() {
        assert_eq!(classify_checked_send_result(Ok(())), CheckedSendReceipt::Accepted);
        assert_eq!(
            classify_checked_send_result(Err("udp send fail")),
            CheckedSendReceipt::Backpressure
        );
        assert_eq!(classify_checked_send_result(Err("link down")), CheckedSendReceipt::Failed);
    }
}
