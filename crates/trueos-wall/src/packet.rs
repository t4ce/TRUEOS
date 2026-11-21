//! Ethernet ingress and egress policy, including return traffic for allowed flows.

use alloc::collections::BTreeMap;

use crate::{Scope, Transport, allows_inbound, allows_outbound};

const FLOWS_PER_DIRECTION: usize = 2048;
const TCP_OPENING_SECS: u64 = 30;
const TCP_IDLE_SECS: u64 = 2 * 60 * 60;
const TCP_CLOSING_SECS: u64 = 2 * 60;
const UDP_IDLE_SECS: u64 = 120;
const SWEEP_INTERVAL_SECS: u64 = 5;
const TCP_FIN: u8 = 0x01;
const TCP_SYN: u8 = 0x02;
const TCP_RST: u8 = 0x04;
const TCP_ACK: u8 = 0x10;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    Inbound,
    Outbound,
}

impl Direction {
    fn reverse(self) -> Self {
        match self {
            Self::Inbound => Self::Outbound,
            Self::Outbound => Self::Inbound,
        }
    }
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
enum Address {
    V4([u8; 4]),
    V6([u8; 16]),
}

impl Address {
    fn valid_on_wire(self) -> bool {
        match self {
            Self::V4([first, ..]) => first != 127,
            Self::V6(address) => {
                !core::net::Ipv6Addr::from(address).is_loopback()
                    && address[..12] != [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff]
            }
        }
    }

    fn multicast(self) -> bool {
        match self {
            Self::V4([first, ..]) => (224..=239).contains(&first),
            Self::V6([first, ..]) => first == 0xff,
        }
    }
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
struct Endpoint {
    address: Address,
    port: u16,
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
struct Flow {
    link: usize,
    protocol: u8,
    local: Endpoint,
    peer: Endpoint,
}

impl Flow {
    fn new(link: usize, direction: Direction, ip: &IpPacket<'_>, ports: Ports) -> Self {
        let source = Endpoint {
            address: ip.source,
            port: ports.source,
        };
        let destination = Endpoint {
            address: ip.destination,
            port: ports.destination,
        };
        let (local, peer) = match direction {
            Direction::Inbound => (destination, source),
            Direction::Outbound => (source, destination),
        };
        Self {
            link,
            protocol: ip.protocol,
            local,
            peer,
        }
    }
}

enum TcpFlow {
    Opening { syn_ack: bool },
    Established { fin: u8 },
}

struct FlowState {
    expires_at: u64,
    tcp: Option<TcpFlow>,
}

impl FlowState {
    fn new(transport: Transport, now_secs: u64) -> Self {
        match transport {
            Transport::Tcp => Self {
                expires_at: now_secs.saturating_add(TCP_OPENING_SECS),
                tcp: Some(TcpFlow::Opening { syn_ack: false }),
            },
            Transport::Udp => Self {
                expires_at: now_secs.saturating_add(UDP_IDLE_SECS),
                tcp: None,
            },
        }
    }

    fn observe(&mut self, initiator: Direction, direction: Direction, flags: Option<u8>, now: u64) {
        let Some(flags) = flags else {
            self.expires_at = now.saturating_add(UDP_IDLE_SECS);
            return;
        };
        if flags & TCP_RST != 0 {
            // A local reset removes the flow. Allow recovery from a peer reset
            // the TCP stack rejects after validating its sequence number.
            self.expires_at = self.expires_at.min(now.saturating_add(TCP_CLOSING_SECS));
            return;
        }
        if let Some(TcpFlow::Opening { syn_ack }) = &mut self.tcp {
            if direction != initiator && flags & (TCP_SYN | TCP_ACK) == (TCP_SYN | TCP_ACK) {
                *syn_ack = true;
            }
            if *syn_ack && direction == initiator && flags & (TCP_SYN | TCP_ACK) == TCP_ACK {
                self.tcp = Some(TcpFlow::Established { fin: 0 });
            }
        }
        let idle_secs = match &mut self.tcp {
            Some(TcpFlow::Established { fin }) => {
                if flags & TCP_FIN != 0 {
                    *fin |= match direction {
                        Direction::Inbound => 1,
                        Direction::Outbound => 2,
                    };
                }
                if *fin == 3 {
                    TCP_CLOSING_SECS
                } else {
                    TCP_IDLE_SECS
                }
            }
            _ => TCP_OPENING_SECS,
        };
        self.expires_at = now.saturating_add(idle_secs);
    }
}

/// Apply the same static policy to every link. Return flows track their link
/// so that two interfaces with overlapping addresses cannot reuse a flow.
pub struct PacketWall {
    inbound: BTreeMap<Flow, FlowState>,
    outbound: BTreeMap<Flow, FlowState>,
    sweep_at: u64,
}

impl PacketWall {
    pub const fn new() -> Self {
        Self {
            inbound: BTreeMap::new(),
            outbound: BTreeMap::new(),
            sweep_at: 0,
        }
    }

    /// `now_secs` comes from the kernel's monotonic clock.
    pub fn allows_frame(
        &mut self,
        link: usize,
        direction: Direction,
        frame: &[u8],
        now_secs: u64,
    ) -> bool {
        let Some((ethertype, payload)) = ethernet_payload(frame) else {
            return false;
        };
        if ethertype == 0x0806 {
            return arp(payload);
        }
        let ip = match ethertype {
            0x0800 => IpPacket::v4(payload, false),
            0x86dd => IpPacket::v6(payload, false),
            _ => None,
        };
        let Some(ip) = ip else {
            return false;
        };
        if !ip.source.valid_on_wire() || !ip.destination.valid_on_wire() {
            return false;
        }
        match (ip.source, ip.protocol) {
            (_, 6 | 17) => self.allows_transport(link, direction, &ip, now_secs),
            (Address::V4(_), 1) | (Address::V6(_), 58) => {
                self.allows_icmp(link, direction, &ip, now_secs)
            }
            (Address::V4(_), 2) => {
                ip.hop_limit == 1
                    && ip.destination.multicast()
                    && ip.payload.len() >= 8
                    && matches!(ip.payload[0], 0x11 | 0x12 | 0x16 | 0x17 | 0x22)
            }
            _ => false,
        }
    }

    fn allows_transport(
        &mut self,
        link: usize,
        direction: Direction,
        ip: &IpPacket<'_>,
        now_secs: u64,
    ) -> bool {
        let Some(ports) = Ports::parse(ip.payload) else {
            return false;
        };
        let (transport, tcp_flags) = if ip.protocol == 6 {
            if ip.payload.len() < 20 {
                return false;
            }
            let header_len = usize::from(ip.payload[12] >> 4) * 4;
            if header_len < 20 || header_len > ip.payload.len() {
                return false;
            }
            (Transport::Tcp, Some(ip.payload[13]))
        } else {
            let Some(udp_len) = word_at(ip.payload, 4).map(usize::from) else {
                return false;
            };
            if udp_len < 8 || udp_len > ip.payload.len() {
                return false;
            }
            if [ports.source, ports.destination]
                .iter()
                .any(|port| matches!(port, 67 | 68 | 546 | 547))
            {
                return dhcp(ip.source, direction, ports);
            }
            (Transport::Udp, None)
        };

        let flow = Flow::new(link, direction, ip, ports);
        // A new TCP flow must start with SYN and may negotiate ECN.
        let opening_syn = tcp_flags
            .is_some_and(|flags| flags & (TCP_FIN | TCP_SYN | TCP_RST | TCP_ACK) == TCP_SYN);
        for (initiator, table) in [
            (Direction::Inbound, &mut self.inbound),
            (Direction::Outbound, &mut self.outbound),
        ] {
            if let Some(state) = table.get_mut(&flow) {
                if state.expires_at <= now_secs {
                    continue;
                }
                if direction == Direction::Outbound
                    && tcp_flags.is_some_and(|flags| flags & TCP_RST != 0)
                {
                    table.remove(&flow);
                } else {
                    state.observe(initiator, direction, tcp_flags, now_secs);
                }
                return true;
            }
        }

        if tcp_flags.is_some() && !opening_syn {
            return false;
        }
        let allowed = match direction {
            Direction::Inbound => allows_inbound(transport, Scope::Network, ports.destination),
            Direction::Outbound => allows_outbound(transport, Scope::Network, ports.destination),
        };
        if !allowed {
            return false;
        }
        if now_secs >= self.sweep_at {
            self.inbound.retain(|_, state| state.expires_at > now_secs);
            self.outbound.retain(|_, state| state.expires_at > now_secs);
            self.sweep_at = now_secs.saturating_add(SWEEP_INTERVAL_SECS);
        }
        let table = match direction {
            Direction::Inbound => &mut self.inbound,
            Direction::Outbound => &mut self.outbound,
        };
        if table.len() >= FLOWS_PER_DIRECTION && !table.contains_key(&flow) {
            return false;
        }
        table.insert(flow, FlowState::new(transport, now_secs));
        true
    }

    fn allows_icmp(
        &self,
        link: usize,
        direction: Direction,
        ip: &IpPacket<'_>,
        now_secs: u64,
    ) -> bool {
        if ip.payload.len() < 8 {
            return false;
        }
        let kind = ip.payload[0];
        let code = ip.payload[1];
        let quoted = match ip.source {
            Address::V4(_) => match (kind, code) {
                (0 | 8, 0) => return true,
                (3 | 11 | 12, _) => IpPacket::v4(&ip.payload[8..], true),
                _ => None,
            },
            Address::V6(source) => match (kind, code) {
                (128 | 129, 0) => return true,
                (133, 0) => return direction == Direction::Outbound && ip.hop_limit == 255,
                (134, 0) => {
                    return direction == Direction::Inbound
                        && ip.hop_limit == 255
                        && ip.payload.len() >= 16
                        && source[0] == 0xfe
                        && source[1] & 0xc0 == 0x80;
                }
                (135 | 136, 0) => return ip.hop_limit == 255 && ip.payload.len() >= 24,
                (130..=132 | 143, 0) => {
                    return ip.hop_limit == 1
                        && ip.destination.multicast()
                        && (kind == 143 || ip.payload.len() >= 24);
                }
                (1..=4, _) => IpPacket::v6(&ip.payload[8..], true),
                _ => None,
            },
        };
        let Some(quoted) = quoted else {
            return false;
        };
        if !matches!(quoted.protocol, 6 | 17) || quoted.source != ip.destination {
            return false;
        }
        let Some(ports) = Ports::parse(quoted.payload) else {
            return false;
        };
        let flow = Flow::new(link, direction.reverse(), &quoted, ports);
        [&self.inbound, &self.outbound].iter().any(|table| {
            table
                .get(&flow)
                .is_some_and(|state| state.expires_at > now_secs)
        })
    }
}

impl Default for PacketWall {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy)]
struct Ports {
    source: u16,
    destination: u16,
}

impl Ports {
    fn parse(payload: &[u8]) -> Option<Self> {
        let source = word_at(payload, 0)?;
        let destination = word_at(payload, 2)?;
        (source != 0 && destination != 0).then_some(Self {
            source,
            destination,
        })
    }
}

fn dhcp(address: Address, direction: Direction, ports: Ports) -> bool {
    let (client, server) = match address {
        Address::V4(_) => (68, 67),
        Address::V6(_) => (546, 547),
    };
    let (source, destination) = match direction {
        Direction::Inbound => (server, client),
        Direction::Outbound => (client, server),
    };
    ports.source == source && ports.destination == destination
}

fn word_at(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn ethernet_payload(frame: &[u8]) -> Option<(u16, &[u8])> {
    let mut ethertype = word_at(frame, 12)?;
    let mut payload = &frame[14..];
    for _ in 0..2 {
        if !matches!(ethertype, 0x8100 | 0x88a8) {
            break;
        }
        ethertype = word_at(payload, 2)?;
        payload = &payload[4..];
    }
    Some((ethertype, payload))
}

fn arp(payload: &[u8]) -> bool {
    payload.len() >= 28
        && payload[..6] == [0, 1, 0x08, 0, 6, 4]
        && matches!(word_at(payload, 6), Some(1 | 2))
}

struct IpPacket<'a> {
    source: Address,
    destination: Address,
    hop_limit: u8,
    protocol: u8,
    payload: &'a [u8],
}

impl<'a> IpPacket<'a> {
    fn v4(packet: &'a [u8], quoted: bool) -> Option<Self> {
        if packet.len() < 20 || packet[0] >> 4 != 4 {
            return None;
        }
        let header_len = usize::from(packet[0] & 0x0f) * 4;
        let total_len = usize::from(word_at(packet, 2)?);
        // Reject fragments whose complete transport header and payload cannot
        // be inspected. The stack has no IP reassembly enabled.
        if header_len < 20 || word_at(packet, 6)? & 0xbfff != 0 {
            return None;
        }
        let end = if quoted {
            total_len.min(packet.len())
        } else {
            total_len
        };
        Some(Self {
            source: Address::V4(packet[12..16].try_into().ok()?),
            destination: Address::V4(packet[16..20].try_into().ok()?),
            hop_limit: packet[8],
            protocol: packet[9],
            payload: packet.get(header_len..end)?,
        })
    }

    fn v6(packet: &'a [u8], quoted: bool) -> Option<Self> {
        if packet.len() < 40 || packet[0] >> 4 != 6 {
            return None;
        }
        let total_len = 40 + usize::from(word_at(packet, 4)?);
        let end = if quoted {
            total_len.min(packet.len())
        } else {
            total_len
        };
        let mut payload = packet.get(40..end)?;
        let mut protocol = packet[6];
        if protocol == 0 {
            protocol = *payload.first()?;
            let header_len = (usize::from(*payload.get(1)?) + 1) * 8;
            payload = payload.get(header_len..)?;
        }
        Some(Self {
            source: Address::V6(packet[8..24].try_into().ok()?),
            destination: Address::V6(packet[24..40].try_into().ok()?),
            hop_limit: packet[7],
            protocol,
            payload,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Direction::*, PacketWall, TCP_CLOSING_SECS, TCP_OPENING_SECS, UDP_IDLE_SECS};
    use alloc::{vec, vec::Vec};
    use core::net::Ipv6Addr;

    const LOCAL: [u8; 4] = [192, 0, 2, 1];
    const PEER: [u8; 4] = [192, 0, 2, 2];

    fn ipv4(protocol: u8, source: [u8; 4], destination: [u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut frame = vec![0; 14 + 20 + payload.len()];
        frame[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
        frame[14] = 0x45;
        frame[16..18].copy_from_slice(&((20 + payload.len()) as u16).to_be_bytes());
        frame[22] = 64;
        frame[23] = protocol;
        frame[26..30].copy_from_slice(&source);
        frame[30..34].copy_from_slice(&destination);
        frame[34..].copy_from_slice(payload);
        frame
    }

    fn ipv6(protocol: u8, source: Ipv6Addr, destination: Ipv6Addr, payload: &[u8]) -> Vec<u8> {
        let mut frame = vec![0; 14 + 40 + payload.len()];
        frame[12..14].copy_from_slice(&0x86ddu16.to_be_bytes());
        frame[14] = 0x60;
        frame[18..20].copy_from_slice(&(payload.len() as u16).to_be_bytes());
        frame[20] = protocol;
        frame[21] = 64;
        frame[22..38].copy_from_slice(&source.octets());
        frame[38..54].copy_from_slice(&destination.octets());
        frame[54..].copy_from_slice(payload);
        frame
    }

    fn tcp(source: u16, destination: u16, flags: u8) -> [u8; 20] {
        let mut payload = [0; 20];
        payload[..2].copy_from_slice(&source.to_be_bytes());
        payload[2..4].copy_from_slice(&destination.to_be_bytes());
        payload[12] = 0x50;
        payload[13] = flags;
        payload
    }

    fn udp(source: u16, destination: u16) -> [u8; 8] {
        let mut payload = [0; 8];
        payload[..2].copy_from_slice(&source.to_be_bytes());
        payload[2..4].copy_from_slice(&destination.to_be_bytes());
        payload[4..6].copy_from_slice(&8u16.to_be_bytes());
        payload
    }

    #[test]
    fn tcp_admits_only_initial_syn_on_approved_ports_and_matching_returns() {
        let mut wall = PacketWall::new();
        let https_syn = ipv4(6, LOCAL, PEER, &tcp(50_031, 443, 0x02));
        let https_syn_ack = ipv4(6, PEER, LOCAL, &tcp(443, 50_031, 0x12));
        let public_ssh_syn = ipv4(6, PEER, LOCAL, &tcp(50_095, 22, 0x02));
        let public_ssh_syn_ack = ipv4(6, LOCAL, PEER, &tcp(22, 50_095, 0x12));
        assert!(!wall.allows_frame(0, Inbound, &https_syn_ack, 0));
        assert!(!wall.allows_frame(0, Outbound, &public_ssh_syn_ack, 0));
        assert!(wall.allows_frame(0, Outbound, &https_syn, 0));
        assert!(!wall.allows_frame(1, Inbound, &https_syn_ack, 1));
        assert!(wall.allows_frame(0, Inbound, &https_syn_ack, 1));
        assert!(wall.allows_frame(1, Inbound, &public_ssh_syn, 1));
        assert!(wall.allows_frame(1, Outbound, &public_ssh_syn_ack, 1));
        let closed_port = ipv4(6, PEER, LOCAL, &tcp(50_095, 65_001, 0x02));
        assert!(!wall.allows_frame(1, Inbound, &closed_port, 1));
    }

    #[test]
    fn tcp_retains_idle_and_half_closed_flows_and_retires_closed_tuples() {
        let mut wall = PacketWall::new();
        let local = |flags| ipv4(6, LOCAL, PEER, &tcp(50_031, 443, flags));
        let peer = |flags| ipv4(6, PEER, LOCAL, &tcp(443, 50_031, flags));

        assert!(wall.allows_frame(0, Outbound, &local(0x02), 0));
        assert!(!wall.allows_frame(0, Inbound, &peer(0x12), TCP_OPENING_SECS));
        assert!(wall.allows_frame(0, Outbound, &local(0x02), 40));
        assert!(wall.allows_frame(0, Inbound, &peer(0x12), 41));
        assert!(wall.allows_frame(0, Outbound, &local(0x10), 42));
        assert!(wall.allows_frame(0, Inbound, &peer(0x18), 600));

        assert!(wall.allows_frame(0, Outbound, &local(0x11), 601));
        assert!(wall.allows_frame(0, Inbound, &peer(0x18), 1200));
        assert!(wall.allows_frame(0, Inbound, &peer(0x11), 1201));
        assert!(wall.allows_frame(0, Outbound, &local(0x10), 1202));
        assert!(!wall.allows_frame(0, Inbound, &peer(0x10), 1202 + TCP_CLOSING_SECS));

        assert!(wall.allows_frame(0, Outbound, &local(0x02), 1500));
        assert!(wall.allows_frame(0, Inbound, &peer(0x12), 1501));
        assert!(wall.allows_frame(0, Outbound, &local(0x10), 1502));
        assert!(wall.allows_frame(0, Inbound, &peer(0x14), 1503));
        assert!(wall.allows_frame(0, Outbound, &local(0x10), 1504));
        assert!(wall.allows_frame(0, Inbound, &peer(0x18), 1800));
        assert!(wall.allows_frame(0, Outbound, &local(0x14), 1801));
        assert!(!wall.allows_frame(0, Inbound, &peer(0x18), 1802));
    }

    #[test]
    fn udp_replies_match_both_addresses_and_ports_until_idle_expiry() {
        let mut wall = PacketWall::new();
        let request = ipv4(17, LOCAL, PEER, &udp(50_031, 123));
        let reply = ipv4(17, PEER, LOCAL, &udp(123, 50_031));
        let other_peer = ipv4(17, [192, 0, 2, 3], LOCAL, &udp(123, 50_031));
        let other_port = ipv4(17, PEER, LOCAL, &udp(124, 50_031));
        assert!(!wall.allows_frame(0, Inbound, &reply, 0));
        assert!(wall.allows_frame(0, Outbound, &request, 0));
        assert!(!wall.allows_frame(0, Inbound, &other_peer, 1));
        assert!(!wall.allows_frame(0, Inbound, &other_port, 1));
        assert!(wall.allows_frame(0, Inbound, &reply, 1));
        assert!(!wall.allows_frame(0, Inbound, &reply, 1 + UDP_IDLE_SECS));
    }

    #[test]
    fn permits_only_dhcp_port_pairs_and_rejects_fragments_and_wire_loopback() {
        let mut wall = PacketWall::new();
        let dhcp_request = ipv4(17, [0; 4], [255; 4], &udp(68, 67));
        let dhcp_reply = ipv4(17, PEER, [255; 4], &udp(67, 68));
        let unrelated = ipv4(17, PEER, LOCAL, &udp(55_031, 68));
        let loopback = ipv4(6, PEER, [127, 0, 0, 1], &tcp(50_095, 22, 0x02));
        assert!(wall.allows_frame(0, Outbound, &dhcp_request, 0));
        assert!(wall.allows_frame(0, Inbound, &dhcp_reply, 0));
        assert!(!wall.allows_frame(0, Inbound, &unrelated, 0));
        assert!(!wall.allows_frame(0, Inbound, &loopback, 0));
        let mut fragment = dhcp_reply;
        fragment[20] = 0x20;
        assert!(!wall.allows_frame(0, Inbound, &fragment, 0));
        assert!(!wall.allows_frame(0, Inbound, &fragment[..31], 0));
    }

    #[test]
    fn multicast_discovery_uses_named_inbound_and_outbound_ports() {
        let mut wall = PacketWall::new();
        let mdns_query = ipv4(17, LOCAL, [224, 0, 0, 251], &udp(5353, 5353));
        let mdns_response = ipv4(17, PEER, LOCAL, &udp(5353, 5353));
        let peer_announcement = ipv4(17, PEER, [255; 4], &udp(32_343, 32_343));
        assert!(wall.allows_frame(0, Outbound, &mdns_query, 0));
        assert!(wall.allows_frame(0, Inbound, &mdns_response, 1));
        assert!(wall.allows_frame(1, Inbound, &peer_announcement, 1));
        assert!(wall.allows_frame(1, Outbound, &peer_announcement, 1));
    }

    #[test]
    fn ipv6_applies_transport_rules_after_hop_by_hop_and_preserves_router_discovery() {
        let mut wall = PacketWall::new();
        let local = "2001:db8::1".parse().unwrap();
        let peer = "2001:db8::2".parse().unwrap();
        let mut hbh_tcp = [0; 28];
        hbh_tcp[0] = 6;
        hbh_tcp[8..].copy_from_slice(&tcp(50_031, 22, 0x02));
        let open = ipv6(0, peer, local, &hbh_tcp);
        assert!(wall.allows_frame(0, Inbound, &open, 0));
        assert!(wall.allows_frame(
            0,
            Outbound,
            &ipv6(6, local, peer, &tcp(22, 50_031, 0x12)),
            1
        ));

        hbh_tcp[8..].copy_from_slice(&tcp(50_095, 65_001, 0x02));
        assert!(!wall.allows_frame(0, Inbound, &ipv6(0, peer, local, &hbh_tcp), 1));
        assert!(!wall.allows_frame(0, Inbound, &ipv6(44, peer, local, &hbh_tcp), 1));

        let mut advertisement = [0; 16];
        advertisement[0] = 134;
        let mut ra = ipv6(58, "fe80::1".parse().unwrap(), local, &advertisement);
        assert!(!wall.allows_frame(0, Inbound, &ra, 0));
        ra[21] = 255;
        assert!(wall.allows_frame(0, Inbound, &ra, 0));
        assert!(!wall.allows_frame(0, Outbound, &ra, 0));

        let dhcp_reply = ipv6(17, peer, local, &udp(547, 546));
        assert!(wall.allows_frame(0, Inbound, &dhcp_reply, 0));
        assert!(!wall.allows_frame(0, Inbound, &ipv6(17, peer, local, &udp(67, 68)), 0));
    }

    #[test]
    fn icmp_errors_require_a_related_transport_tuple() {
        let mut wall = PacketWall::new();
        let outgoing = ipv4(6, LOCAL, PEER, &tcp(50_031, 443, 0x02));
        let mut error = vec![0; 8];
        error[0] = 3;
        error[1] = 4;
        error.extend_from_slice(&outgoing[14..42]);
        let error = ipv4(1, [192, 0, 2, 3], LOCAL, &error);
        assert!(!wall.allows_frame(0, Inbound, &error, 0));
        assert!(wall.allows_frame(0, Outbound, &outgoing, 0));
        assert!(wall.allows_frame(0, Inbound, &error, 1));
    }
}
