#![allow(dead_code)]

use embassy_time::{Duration as EmbassyDuration, Timer};

use crate::r::net::{
    NetProfile, VNet,
    udp::{VNetUdpEndpoint, VNetUdpEvent, VNetUdpPacket},
};

const SNTP_PORT: u16 = crate::allports::well_known::SNTP;
const SNTP_PACKET_LEN: usize = 48;
const SNTP_OPEN_TIMEOUT_MS: u64 = 4000;
const SNTP_IDLE_POLL_MS: u64 = 10;
const NTP_UNIX_EPOCH_OFFSET: u64 = 2_208_988_800;
const NTP_FRACTION_SCALE: u128 = 4_294_967_296;

struct SntpRequest {
    version: u8,
    poll: u8,
    tx_secs: u32,
    tx_frac: u32,
}

#[inline]
fn read_u32_be(packet: &[u8], off: usize) -> u32 {
    u32::from_be_bytes([
        packet[off],
        packet[off + 1],
        packet[off + 2],
        packet[off + 3],
    ])
}

#[inline]
fn write_u32_be(packet: &mut [u8; SNTP_PACKET_LEN], off: usize, value: u32) {
    packet[off..off + 4].copy_from_slice(&value.to_be_bytes());
}

#[inline]
fn parse_client_request(packet: &[u8]) -> Option<SntpRequest> {
    if packet.len() < SNTP_PACKET_LEN {
        return None;
    }

    let mode = packet[0] & 0x07;
    if mode != 3 {
        return None;
    }

    let version = (packet[0] >> 3) & 0x07;
    let poll = packet[2];
    Some(SntpRequest {
        version: if version == 0 { 4 } else { version },
        poll,
        tx_secs: read_u32_be(packet, 40),
        tx_frac: read_u32_be(packet, 44),
    })
}

#[inline]
fn current_ntp_timestamp() -> Option<(u32, u32)> {
    let unix =
        crate::r::net::ntp::current_unix_seconds().or_else(crate::r::time::unix_time_seconds)?;
    let ntp_secs = unix.saturating_add(NTP_UNIX_EPOCH_OFFSET);

    let hz = embassy_time_driver::TICK_HZ;
    let frac = if hz == 0 {
        0
    } else {
        let ticks = embassy_time_driver::now() % hz;
        (((ticks as u128) * NTP_FRACTION_SCALE) / (hz as u128)) as u32
    };

    Some((ntp_secs as u32, frac))
}

pub fn build_sntp_response(packet: &[u8]) -> Option<[u8; SNTP_PACKET_LEN]> {
    let req = parse_client_request(packet)?;
    let (now_secs, now_frac) = current_ntp_timestamp()?;

    let mut out = [0u8; SNTP_PACKET_LEN];

    // LI=0, VN=request version, Mode=4 (server)
    let version = req.version.min(4);
    out[0] = (version << 3) | 0x04;
    out[1] = 1; // Stratum 1 while synchronized to our in-kernel wall clock.
    out[2] = req.poll;
    out[3] = 0xEC; // Precision ~= 2^-20 seconds.

    // Reference ID: "TRUO".
    out[12] = b'T';
    out[13] = b'R';
    out[14] = b'U';
    out[15] = b'O';

    // Reference timestamp.
    write_u32_be(&mut out, 16, now_secs);
    write_u32_be(&mut out, 20, now_frac);

    // Originate timestamp = client's transmit timestamp.
    write_u32_be(&mut out, 24, req.tx_secs);
    write_u32_be(&mut out, 28, req.tx_frac);

    // Receive + transmit timestamps from local clock.
    write_u32_be(&mut out, 32, now_secs);
    write_u32_be(&mut out, 36, now_frac);
    write_u32_be(&mut out, 40, now_secs);
    write_u32_be(&mut out, 44, now_frac);

    Some(out)
}

#[embassy_executor::task]
pub async fn sntp_service_task() {
    crate::log_trace!("sntp: waiting for NET_V4_CONFIGURED\n");
    crate::r::readiness::wait_for(crate::r::readiness::NET_V4_CONFIGURED).await;
    crate::log_trace!("sntp: readiness reached, starting service loop\n");

    let mut no_dev_count: u32 = 0;
    let mut vnet_open_fail_count: u32 = 0;
    let mut udp_open_fail_count: u32 = 0;

    loop {
        let profile = NetProfile::default();
        let Some(dev_idx) = profile.resolve_device_index() else {
            no_dev_count = no_dev_count.saturating_add(1);
            if no_dev_count == 1 || no_dev_count % 20 == 0 {
                crate::log_trace!(
                    "sntp: no network device for default profile (retry_count={})\n",
                    no_dev_count
                );
            }
            Timer::after(EmbassyDuration::from_millis(250)).await;
            continue;
        };
        if no_dev_count != 0 {
            crate::log_trace!(
                "sntp: network device resolved after {} retries (dev_idx={})\n",
                no_dev_count,
                dev_idx
            );
            no_dev_count = 0;
        }

        let Some(net) = VNet::open(dev_idx) else {
            vnet_open_fail_count = vnet_open_fail_count.saturating_add(1);
            if vnet_open_fail_count == 1 || vnet_open_fail_count % 20 == 0 {
                crate::log_trace!(
                    "sntp: VNet::open failed (dev_idx={}, retry_count={})\n",
                    dev_idx,
                    vnet_open_fail_count
                );
            }
            Timer::after(EmbassyDuration::from_millis(250)).await;
            continue;
        };
        if vnet_open_fail_count != 0 {
            crate::log_trace!(
                "sntp: VNet::open recovered after {} retries (dev_idx={})\n",
                vnet_open_fail_count,
                dev_idx
            );
            vnet_open_fail_count = 0;
        }

        let Some(mut udp) = VNetUdpEndpoint::bind(
            &net,
            SNTP_PORT,
            EmbassyDuration::from_millis(SNTP_OPEN_TIMEOUT_MS),
        )
        .await
        else {
            crate::log_trace!("sntp: udp open timed out on port={}\n", SNTP_PORT);
            udp_open_fail_count = udp_open_fail_count.saturating_add(1);
            if udp_open_fail_count == 1 || udp_open_fail_count % 20 == 0 {
                crate::log_trace!(
                    "sntp: failed to open UDP socket on port {} (retry_count={})\n",
                    SNTP_PORT,
                    udp_open_fail_count
                );
            }
            Timer::after(EmbassyDuration::from_millis(250)).await;
            continue;
        };
        crate::log_trace!("sntp: udp opened on port={} handle={:?}\n", SNTP_PORT, udp.handle());
        if udp_open_fail_count != 0 {
            crate::log_trace!(
                "sntp: UDP socket open recovered after {} retries\n",
                udp_open_fail_count
            );
            udp_open_fail_count = 0;
        }

        let mut served_packets: u64 = 0;
        let mut rejected_packets: u64 = 0;

        loop {
            let mut had_event = false;
            let mut socket_closed = false;

            for _ in 0..64 {
                let Some(ev) = udp.poll_event() else {
                    break;
                };
                had_event = true;

                match ev {
                    VNetUdpEvent::Packet(VNetUdpPacket::V4 { from, data }) => {
                        if let Some(reply) = build_sntp_response(data.as_slice()) {
                            served_packets = served_packets.saturating_add(1);
                            if served_packets == 1 || served_packets % 64 == 0 {
                                crate::log_trace!(
                                    "sntp: replied to {} requests (last_from={:?})\n",
                                    served_packets,
                                    from
                                );
                            }
                            let _ = udp.send_v4(from, &reply);
                        } else {
                            rejected_packets = rejected_packets.saturating_add(1);
                            if rejected_packets == 1 || rejected_packets % 64 == 0 {
                                crate::log_trace!(
                                    "sntp: ignored non-client/invalid packet count={} (from={:?})\n",
                                    rejected_packets,
                                    from
                                );
                            }
                        }
                    }
                    VNetUdpEvent::Packet(VNetUdpPacket::V6 { .. }) => {}
                    VNetUdpEvent::Closed => {
                        crate::log_trace!(
                            "sntp: UDP socket closed (handle={:?}), reopening\n",
                            udp.handle()
                        );
                        socket_closed = true;
                        break;
                    }
                }
            }

            if socket_closed {
                break;
            }

            if !had_event {
                Timer::after(EmbassyDuration::from_millis(SNTP_IDLE_POLL_MS)).await;
            }
        }
    }
}
