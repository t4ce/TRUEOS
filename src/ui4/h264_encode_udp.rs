//! Bounded UDP egress for the deferred H.264 encode experiment.
//!
//! This is an encoded-access-unit stream. Intel display SURFLIVE is not part
//! of the payload or ownership contract; it remains only a scanout-latch
//! boundary elsewhere in UI4.

use alloc::{collections::VecDeque, vec::Vec};

use embassy_time::{Duration, Timer};

use crate::r::net::{VNet, udp::VNetUdpEndpoint};

const MAGIC: &[u8; 4] = b"TME1";
const VERSION: u8 = 1;
const HEADER_BYTES: usize = 32;
const DATAGRAM_BYTES: usize = 1200;
const PAYLOAD_BYTES: usize = DATAGRAM_BYTES - HEADER_BYTES;
const FLAG_START: u8 = 1 << 0;
const FLAG_END: u8 = 1 << 1;
const FLAG_KEYFRAME: u8 = 1 << 2;
const FLAG_SESSION_END: u8 = 1 << 3;
const UDP_OPEN_TIMEOUT_MS: u64 = 4_000;
const UDP_RETRY_MS: u64 = 250;
const UDP_PACKET_PACE_MS: u64 = 1;
const UDP_SUBMIT_RETRY_LIMIT: usize = 64;

#[derive(Debug)]
struct EncodedAccessUnit {
    sequence: u32,
    bytes: Vec<u8>,
}

struct AccessUnitRing {
    queue: VecDeque<EncodedAccessUnit>,
    queued_bytes: usize,
    high_water_units: usize,
    high_water_bytes: usize,
    dropped_units: usize,
    dropped_bytes: usize,
}

impl AccessUnitRing {
    fn new() -> Self {
        Self {
            queue: VecDeque::with_capacity(crate::allcaps::media_encode::STREAM_RING_ACCESS_UNITS),
            queued_bytes: 0,
            high_water_units: 0,
            high_water_bytes: 0,
            dropped_units: 0,
            dropped_bytes: 0,
        }
    }

    fn push(&mut self, access_unit: EncodedAccessUnit) {
        let bytes = access_unit.bytes.len();
        if bytes > crate::allcaps::media_encode::STREAM_MAX_ACCESS_UNIT_BYTES {
            self.dropped_units = self.dropped_units.saturating_add(1);
            self.dropped_bytes = self.dropped_bytes.saturating_add(bytes);
            return;
        }

        while self.queue.len() >= crate::allcaps::media_encode::STREAM_RING_ACCESS_UNITS
            || self.queued_bytes.saturating_add(bytes)
                > crate::allcaps::media_encode::STREAM_RING_BYTES
        {
            let Some(dropped) = self.pop() else {
                break;
            };
            self.dropped_units = self.dropped_units.saturating_add(1);
            self.dropped_bytes = self.dropped_bytes.saturating_add(dropped.bytes.len());
        }

        self.queued_bytes = self.queued_bytes.saturating_add(bytes);
        self.queue.push_back(access_unit);
        self.high_water_units = self.high_water_units.max(self.queue.len());
        self.high_water_bytes = self.high_water_bytes.max(self.queued_bytes);
    }

    fn pop(&mut self) -> Option<EncodedAccessUnit> {
        let access_unit = self.queue.pop_front()?;
        self.queued_bytes = self.queued_bytes.saturating_sub(access_unit.bytes.len());
        Some(access_unit)
    }

    fn len(&self) -> usize {
        self.queue.len()
    }
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
    pub(super) adapter_send_errors: usize,
    pub(super) network_waits: usize,
}

/// Broadcast one bounded test stream through the kernel VNet UDP path.
pub(super) async fn broadcast_annex_b(annex_b: &[u8], session_id: u32) -> MediaUdpStreamReport {
    let mut ring = AccessUnitRing::new();
    enqueue_annex_b_access_units(&mut ring, annex_b);
    let queued_access_units = ring.len();
    let mut report = MediaUdpStreamReport {
        session_id,
        queued_access_units,
        dropped_access_units: ring.dropped_units,
        dropped_bytes: ring.dropped_bytes,
        high_water_access_units: ring.high_water_units,
        high_water_bytes: ring.high_water_bytes,
        ..MediaUdpStreamReport::default()
    };

    if ring.len() == 0 {
        return report;
    }

    crate::log_info!(target: "intel/media-encode";
        "intel/media-encode: udp-stream staged=1 protocol=tme1 version={} session={} payload=annexb-access-units queued_units={} queued_bytes={} ring_seconds={} ring_units_cap={} ring_bytes_cap={} overflow=drop-oldest transport=udp-broadcast destination=255.255.255.255:{} network=waiting\n",
        VERSION,
        session_id,
        report.queued_access_units,
        report.high_water_bytes,
        crate::allcaps::media_encode::STREAM_RING_SECONDS,
        crate::allcaps::media_encode::STREAM_RING_ACCESS_UNITS,
        crate::allcaps::media_encode::STREAM_RING_BYTES,
        crate::allports::services::MEDIA_ENCODE_PROBE_UDP_PORT,
    );

    crate::r::readiness::wait_for(crate::r::readiness::NET_V4_CONFIGURED).await;
    let net = loop {
        let Some(device_index) = crate::r::net::NetProfile::default().resolve_device_index() else {
            report.network_waits = report.network_waits.saturating_add(1);
            Timer::after(Duration::from_millis(UDP_RETRY_MS)).await;
            continue;
        };
        let Some(net) = VNet::open_with_event_queue_depth(device_index, 64) else {
            report.network_waits = report.network_waits.saturating_add(1);
            Timer::after(Duration::from_millis(UDP_RETRY_MS)).await;
            continue;
        };
        break net;
    };
    let mut udp = loop {
        let Some(udp) = VNetUdpEndpoint::bind(
            &net,
            crate::allports::services::MEDIA_ENCODE_PROBE_UDP_PORT,
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

    let remote = v::vnet::EndpointV4::new(
        [255, 255, 255, 255],
        crate::allports::services::MEDIA_ENCODE_PROBE_UDP_PORT,
    );
    let final_sequence = ring.queue.back().map(|unit| unit.sequence);
    let mut datagram_sequence = 0u32;

    while let Some(access_unit) = ring.pop() {
        let fragment_count = access_unit.bytes.len().div_ceil(PAYLOAD_BYTES);
        if fragment_count == 0 || fragment_count > u16::MAX as usize {
            report.dropped_access_units = report.dropped_access_units.saturating_add(1);
            report.dropped_bytes = report.dropped_bytes.saturating_add(access_unit.bytes.len());
            continue;
        }

        let mut complete = true;
        for (fragment_index, payload) in access_unit.bytes.chunks(PAYLOAD_BYTES).enumerate() {
            let mut packet = [0u8; DATAGRAM_BYTES];
            let mut flags = FLAG_KEYFRAME;
            if fragment_index == 0 {
                flags |= FLAG_START;
            }
            if fragment_index + 1 == fragment_count {
                flags |= FLAG_END;
                if Some(access_unit.sequence) == final_sequence {
                    flags |= FLAG_SESSION_END;
                }
            }
            encode_header(
                &mut packet[..HEADER_BYTES],
                flags,
                session_id,
                datagram_sequence,
                access_unit.sequence,
                fragment_index as u16,
                fragment_count as u16,
                payload,
            );
            packet[HEADER_BYTES..HEADER_BYTES + payload.len()].copy_from_slice(payload);

            let mut retries = 0usize;
            loop {
                if udp
                    .send_v4(remote, &packet[..HEADER_BYTES + payload.len()])
                    .is_ok()
                {
                    Timer::after(Duration::from_millis(UDP_PACKET_PACE_MS)).await;
                    let mut adapter_failed = false;
                    while let Some(event) = net.pop_event() {
                        match event {
                            v::vnet::Event::Error { .. } => {
                                adapter_failed = true;
                                report.adapter_send_errors =
                                    report.adapter_send_errors.saturating_add(1);
                            }
                            v::vnet::Event::Closed { handle } if handle == udp.handle() => {
                                adapter_failed = true;
                                report.adapter_send_errors =
                                    report.adapter_send_errors.saturating_add(1);
                            }
                            _ => {}
                        }
                    }
                    if !adapter_failed {
                        break;
                    }
                }
                retries = retries.saturating_add(1);
                report.submit_retries = report.submit_retries.saturating_add(1);
                if retries >= UDP_SUBMIT_RETRY_LIMIT {
                    complete = false;
                    break;
                }
                Timer::after(Duration::from_millis(UDP_PACKET_PACE_MS)).await;
            }
            if !complete {
                break;
            }

            report.sent_datagrams = report.sent_datagrams.saturating_add(1);
            report.sent_payload_bytes = report.sent_payload_bytes.saturating_add(payload.len());
            datagram_sequence = datagram_sequence.wrapping_add(1);
        }

        if complete {
            report.sent_access_units = report.sent_access_units.saturating_add(1);
        } else {
            report.dropped_access_units = report.dropped_access_units.saturating_add(1);
            report.dropped_bytes = report.dropped_bytes.saturating_add(access_unit.bytes.len());
        }
    }

    udp.close();
    drop(udp);
    drop(net);
    report
}

fn enqueue_annex_b_access_units(ring: &mut AccessUnitRing, annex_b: &[u8]) {
    let mut idr_offsets = Vec::new();
    let mut cursor = 0usize;
    while cursor + 5 <= annex_b.len() {
        if annex_b[cursor..cursor + 4] == [0, 0, 0, 1] {
            if annex_b[cursor + 4] & 0x1f == 5 {
                idr_offsets.push(cursor);
            }
            cursor += 5;
        } else {
            cursor += 1;
        }
    }

    for index in 0..idr_offsets.len() {
        let start = if index == 0 { 0 } else { idr_offsets[index] };
        let end = idr_offsets.get(index + 1).copied().unwrap_or(annex_b.len());
        if start >= end {
            continue;
        }
        ring.push(EncodedAccessUnit {
            sequence: index as u32,
            bytes: annex_b[start..end].to_vec(),
        });
    }
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
