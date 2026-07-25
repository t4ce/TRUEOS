//! Subscriber-driven, bounded UDP egress for live UI4 H.264 access units.
//!
//! This is an encoded-access-unit stream. Intel display SURFLIVE is not part
//! of the payload or ownership contract; it remains only a scanout-latch
//! boundary elsewhere in UI4.

use alloc::vec::Vec;

use embassy_time::{Duration, Instant, Timer};

use crate::r::net::{
    VNet,
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
// One 32-fragment window occupies at most 38,400 bytes of the 64 KiB socket
// TX ring. Submit the window before awaiting its receipts so the network
// service can drain commands in one scheduler turn instead of one turn per
// fragment.
const UDP_RECEIPT_WINDOW_FRAGMENTS: usize = 32;

#[derive(Debug)]
struct EncodedAccessUnit {
    sequence: u32,
    keyframe: bool,
    bytes: Vec<u8>,
}

struct PendingDatagram {
    receipt: u32,
    packet: [u8; DATAGRAM_BYTES],
    packet_bytes: usize,
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

/// Wait for one receiver, start the bounded producer, then encode and send on
/// absolute `target_hz` deadlines. The first prepared frame is excluded from
/// cadence timing; subsequent preparation overlaps hardware encode and egress.
pub(super) async fn stream_generated_annex_b<B, R, F>(
    transport: &mut MediaUdpTransport,
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
    let mut report = MediaUdpStreamReport {
        session_id,
        network_waits: core::mem::take(&mut transport.pending_open_waits),
        ..MediaUdpStreamReport::default()
    };
    if access_unit_count == 0 || target_hz == 0 {
        return report;
    }

    crate::log_info!(target: "intel/media-encode";
        "intel/media-encode: udp-live staged=1 protocol=tme1 version={} session={} payload=annexb-access-units target_fps={} target_units={} live_high_water_cap=1 transport=udp-subscribe-unicast listen_port={} subscriber_token=TME1GET1 network=waiting\n",
        VERSION,
        session_id,
        target_hz,
        access_unit_count,
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
        "intel/media-encode: udp-live subscriber=accepted session={} peer={}.{}.{}.{}:{} wait_polls={} action=capture-encode-send\n",
        session_id,
        remote.addr[0],
        remote.addr[1],
        remote.addr[2],
        remote.addr[3],
        remote.port,
        report.subscriber_wait_polls,
    );

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
    let mut next_deadline = Instant::now();
    let mut datagram_sequence = 0u32;
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
                report.late_access_units = report.late_access_units.saturating_add(1);
                let late_us = now.saturating_duration_since(next_deadline).as_micros();
                report.max_late_us = report.max_late_us.max(late_us);
                // Rebase instead of emitting a catch-up burst.
                next_deadline = now;
            }
        }

        let Some(bytes) = generate(sequence) else {
            report.dropped_access_units = report.dropped_access_units.saturating_add(1);
            break;
        };
        report.queued_access_units = report.queued_access_units.saturating_add(1);
        report.high_water_access_units = report.high_water_access_units.max(1);
        report.high_water_bytes = report.high_water_bytes.max(bytes.len());
        if bytes.len() > crate::allcaps::media_encode::STREAM_MAX_ACCESS_UNIT_BYTES {
            report.dropped_access_units = report.dropped_access_units.saturating_add(1);
            report.dropped_bytes = report.dropped_bytes.saturating_add(bytes.len());
            break;
        }
        let Some(keyframe) = annex_b_access_unit_keyframe(bytes.as_slice()) else {
            report.dropped_access_units = report.dropped_access_units.saturating_add(1);
            report.dropped_bytes = report.dropped_bytes.saturating_add(bytes.len());
            break;
        };
        let access_unit = EncodedAccessUnit {
            sequence,
            keyframe,
            bytes,
        };
        let session_end = index + 1 == access_unit_count;
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
            break;
        }
        report.sent_access_units = report.sent_access_units.saturating_add(1);
    }
    report.elapsed_us = crate::chronos::monotonic_nanos().saturating_sub(started_ns) / 1_000;

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
            let mut packet = [0u8; DATAGRAM_BYTES];
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
                &mut packet[..HEADER_BYTES],
                flags,
                report.session_id,
                receipt,
                access_unit.sequence,
                fragment_index as u16,
                fragment_count as u16,
                payload,
            );
            packet[HEADER_BYTES..HEADER_BYTES + payload.len()].copy_from_slice(payload);
            pending.push(PendingDatagram {
                receipt,
                packet,
                packet_bytes: HEADER_BYTES + payload.len(),
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
            .send_v4_checked(remote, datagram.receipt, &datagram.packet[..datagram.packet_bytes])
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
