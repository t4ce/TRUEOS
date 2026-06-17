extern crate alloc;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_time::Duration as EmbassyDuration;
use v::vnet;

use crate::r::net::{
    NetProfile, VNet,
    udp::{VNetUdpEndpoint, VNetUdpEvent, VNetUdpPacket},
};

const DNS_PORT: u16 = 53;
const DNS_TIMEOUT_MS: u64 = 5_000;
const DNS_IDLE_POLL_MS: u64 = 5;
const DNS_SERVER_MAX: usize = crate::allcaps::net::DNS_SERVER_MAX;
static DNS_QUERY_SEQ: AtomicU32 = AtomicU32::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DnsError {
    BadName,
    NoAnswer,
    NoNic,
    Runtime,
    Timeout,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DnsConfig {
    timeout_ms: u64,
}

impl Default for DnsConfig {
    fn default() -> Self {
        Self {
            timeout_ms: DNS_TIMEOUT_MS,
        }
    }
}

impl DnsConfig {
    pub const fn for_device(_device_index: usize) -> Self {
        Self {
            timeout_ms: DNS_TIMEOUT_MS,
        }
    }

    pub const fn for_profile(_profile: NetProfile) -> Self {
        Self {
            timeout_ms: DNS_TIMEOUT_MS,
        }
    }

    pub const fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }
}

pub async fn resolve_ipv4_primary(host: &str, cfg: DnsConfig) -> Result<[u8; 4], DnsError> {
    let device_index = NetProfile::default()
        .resolve_device_index()
        .ok_or(DnsError::NoNic)?;
    resolve_ipv4_for_device(device_index, host, cfg).await
}

pub async fn resolve_ipv4_for_profile(
    profile: NetProfile,
    host: &str,
    cfg: DnsConfig,
) -> Result<[u8; 4], DnsError> {
    let device_index = profile.resolve_device_index().ok_or(DnsError::NoNic)?;
    resolve_ipv4_for_device(device_index, host, cfg).await
}

pub async fn resolve_ipv4_for_device(
    device_index: usize,
    host: &str,
    cfg: DnsConfig,
) -> Result<[u8; 4], DnsError> {
    let host = host.trim();
    if let Some(ip) = parse_ipv4_literal(host) {
        return Ok(ip);
    }

    let query = build_dns_a_query(host, next_query_id())?;
    let (servers, count) = dns_servers_for_device(device_index).ok_or(DnsError::NoNic)?;
    if count == 0 {
        return Err(DnsError::NoAnswer);
    }

    let net = VNet::open(device_index).ok_or(DnsError::NoNic)?;
    let mut udp = VNetUdpEndpoint::bind(&net, 0, EmbassyDuration::from_millis(cfg.timeout_ms))
        .await
        .ok_or(DnsError::Timeout)?;

    for server in servers.iter().take(count as usize).copied() {
        if server == [0, 0, 0, 0] {
            continue;
        }

        udp.send_v4(vnet::EndpointV4::new(server, DNS_PORT), query.as_slice())
            .map_err(|_| DnsError::Runtime)?;

        match wait_for_a_response(&mut udp, query_id(query.as_slice()), cfg.timeout_ms).await {
            Ok(ip) => return Ok(ip),
            Err(DnsError::Timeout) | Err(DnsError::NoAnswer) => continue,
            Err(err) => return Err(err),
        }
    }

    Err(DnsError::NoAnswer)
}

fn dns_servers_for_device(device_index: usize) -> Option<([[u8; 4]; DNS_SERVER_MAX], u8)> {
    let (servers, count) = crate::net::adapter::dhcp_dns_snapshot_at(device_index)
        .or_else(|| Some(crate::net::adapter::primary_dhcp_dns_snapshot()))?;
    Some((servers, count))
}

async fn wait_for_a_response(
    udp: &mut VNetUdpEndpoint<'_>,
    id: u16,
    timeout_ms: u64,
) -> Result<[u8; 4], DnsError> {
    let timeout = EmbassyDuration::from_millis(timeout_ms);
    let idle = EmbassyDuration::from_millis(DNS_IDLE_POLL_MS);
    loop {
        let Some(event) = udp.next_event(timeout, idle).await else {
            return Err(DnsError::Timeout);
        };
        match event {
            VNetUdpEvent::Packet(VNetUdpPacket::V4 { data, .. }) => {
                if let Some(ip) = parse_dns_a_response(data.as_slice(), id) {
                    return Ok(ip);
                }
            }
            VNetUdpEvent::Packet(VNetUdpPacket::V6 { .. }) => {}
            VNetUdpEvent::Closed => return Err(DnsError::Runtime),
        }
    }
}

fn next_query_id() -> u16 {
    let seq = DNS_QUERY_SEQ.fetch_add(1, Ordering::AcqRel) as u16;
    seq ^ ((embassy_time::Instant::now().as_millis() as u16).rotate_left(5))
}

fn query_id(query: &[u8]) -> u16 {
    u16::from_be_bytes([query[0], query[1]])
}

fn parse_ipv4_literal(host: &str) -> Option<[u8; 4]> {
    let mut out = [0u8; 4];
    let mut count = 0usize;
    for part in host.split('.') {
        if count >= out.len() || part.is_empty() {
            return None;
        }
        out[count] = part.parse::<u8>().ok()?;
        count += 1;
    }
    if count == out.len() { Some(out) } else { None }
}

fn build_dns_a_query(host: &str, id: u16) -> Result<Vec<u8>, DnsError> {
    let mut out = Vec::new();
    out.extend_from_slice(&id.to_be_bytes());
    out.extend_from_slice(&0x0100u16.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());

    for label in host.trim_end_matches('.').split('.') {
        if label.is_empty() || label.len() > 63 || !label.is_ascii() {
            return Err(DnsError::BadName);
        }
        out.push(label.len() as u8);
        out.extend_from_slice(label.as_bytes());
    }
    out.push(0);
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    Ok(out)
}

fn dns_skip_name(buf: &[u8], mut offset: usize) -> Option<usize> {
    loop {
        let len = *buf.get(offset)?;
        offset += 1;
        if len == 0 {
            return Some(offset);
        }
        if len & 0xC0 == 0xC0 {
            let _ = *buf.get(offset)?;
            return Some(offset + 1);
        }
        if len & 0xC0 != 0 {
            return None;
        }
        offset = offset.checked_add(len as usize)?;
        if offset > buf.len() {
            return None;
        }
    }
}

fn parse_dns_a_response(buf: &[u8], id: u16) -> Option<[u8; 4]> {
    if buf.len() < 12 || u16::from_be_bytes([buf[0], buf[1]]) != id {
        return None;
    }
    let qd = u16::from_be_bytes([buf[4], buf[5]]) as usize;
    let an = u16::from_be_bytes([buf[6], buf[7]]) as usize;
    let mut offset = 12usize;
    for _ in 0..qd {
        offset = dns_skip_name(buf, offset)?.checked_add(4)?;
        if offset > buf.len() {
            return None;
        }
    }
    for _ in 0..an {
        offset = dns_skip_name(buf, offset)?;
        if offset + 10 > buf.len() {
            return None;
        }
        let ty = u16::from_be_bytes([buf[offset], buf[offset + 1]]);
        let class = u16::from_be_bytes([buf[offset + 2], buf[offset + 3]]);
        let rdlen = u16::from_be_bytes([buf[offset + 8], buf[offset + 9]]) as usize;
        offset += 10;
        if offset + rdlen > buf.len() {
            return None;
        }
        if ty == 1 && class == 1 && rdlen == 4 {
            return Some([
                buf[offset],
                buf[offset + 1],
                buf[offset + 2],
                buf[offset + 3],
            ]);
        }
        offset += rdlen;
    }
    None
}
