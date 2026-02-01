extern crate alloc;

use alloc::{string::String, vec::Vec};
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_time::{Duration as EmbassyDuration, Instant, Timer};

use trueos_v::vnet as vnet;

use super::VNet;

#[derive(Clone, Copy, Debug)]
pub enum DnsError {
    NoNic,
    BadName,
    Timeout,
    NoAnswer,
}

#[derive(Clone, Copy, Debug)]
pub struct DnsConfig {
    pub servers: &'static [[u8; 4]],
    pub timeout_ms: u64,
    pub resend_ms: u64,
    pub cname_depth: u8,
}

impl Default for DnsConfig {
    fn default() -> Self {
        Self {
            servers: &PUBLIC_DNS_SERVERS,
            timeout_ms: 1500,
            resend_ms: 350,
            cname_depth: 6,
        }
    }
}

pub const PUBLIC_DNS_SERVERS: [[u8; 4]; 3] = [
    // Cloudflare
    [1, 1, 1, 1],
    // Google
    [8, 8, 8, 8],
    // Quad9
    [9, 9, 9, 9],
];

const DNS_PORT: u16 = 53;

static DNS_SEQ: AtomicU32 = AtomicU32::new(1);

#[inline]
fn alloc_dns_id() -> u16 {
    // Avoid 0; keep reasonably unique.
    let s = DNS_SEQ.fetch_add(1, Ordering::Relaxed);
    let id = (s as u16) ^ ((s >> 16) as u16) ^ 0xBEEF;
    if id == 0 { 1 } else { id }
}

#[inline]
fn alloc_local_port() -> u16 {
    // Pick an ephemeral-ish port to avoid collisions between concurrent resolves.
    // Range: 53000..=62999
    let s = DNS_SEQ.fetch_add(1, Ordering::Relaxed);
    53000u16.wrapping_add((s % 10000) as u16)
}

fn dns_make_query(id: u16, host: &str, qtype: u16) -> Result<Vec<u8>, DnsError> {
    let host = host.trim().trim_end_matches('.');
    if host.is_empty() {
        return Err(DnsError::BadName);
    }

    // Minimal DNS query for <qtype>/IN <host>.
    let mut q: Vec<u8> = Vec::new();
    q.extend_from_slice(&id.to_be_bytes());
    q.extend_from_slice(&0x0100u16.to_be_bytes()); // flags: recursion desired
    q.extend_from_slice(&1u16.to_be_bytes()); // QDCOUNT
    q.extend_from_slice(&0u16.to_be_bytes()); // ANCOUNT
    q.extend_from_slice(&0u16.to_be_bytes()); // NSCOUNT
    q.extend_from_slice(&0u16.to_be_bytes()); // ARCOUNT

    for label in host.split('.') {
        if label.is_empty() {
            return Err(DnsError::BadName);
        }
        let len = label.len().min(63);
        q.push(len as u8);
        q.extend_from_slice(&label.as_bytes()[..len]);
    }
    q.push(0);

    // QTYPE, QCLASS=IN (1)
    q.extend_from_slice(&qtype.to_be_bytes());
    q.extend_from_slice(&1u16.to_be_bytes());
    Ok(q)
}

fn dns_skip_name(pkt: &[u8], idx: &mut usize) -> bool {
    let mut i = *idx;
    let mut guard = 0u8;
    loop {
        if i >= pkt.len() {
            return false;
        }
        let b = pkt[i];
        if b == 0 {
            i += 1;
            *idx = i;
            return true;
        }
        // compression pointer
        if (b & 0xC0) == 0xC0 {
            if i + 1 >= pkt.len() {
                return false;
            }
            i += 2;
            *idx = i;
            return true;
        }
        let len = b as usize;
        i += 1;
        if i + len > pkt.len() {
            return false;
        }
        i += len;

        guard = guard.wrapping_add(1);
        if guard > 64 {
            return false;
        }
    }
}

fn dns_read_name(pkt: &[u8], start: usize) -> Option<(String, usize)> {
    // Decode a domain name; supports compression pointers.
    // Returns (decoded_name, next_index_after_name_at_start).
    let mut out = String::new();

    let mut pos = start;
    let mut next = start;
    let mut jumped = false;

    let mut guard = 0u8;
    loop {
        if pos >= pkt.len() {
            return None;
        }
        let b = pkt[pos];
        if b == 0 {
            if !jumped {
                next = pos + 1;
            }
            break;
        }

        // compression pointer
        if (b & 0xC0) == 0xC0 {
            if pos + 1 >= pkt.len() {
                return None;
            }
            let ptr = (((b & 0x3F) as usize) << 8) | (pkt[pos + 1] as usize);
            if ptr >= pkt.len() {
                return None;
            }
            if !jumped {
                next = pos + 2;
                jumped = true;
            }
            pos = ptr;

            guard = guard.wrapping_add(1);
            if guard > 64 {
                return None;
            }
            continue;
        }

        let len = b as usize;
        pos += 1;
        if pos + len > pkt.len() {
            return None;
        }
        if !out.is_empty() {
            out.push('.');
        }
        for &ch in &pkt[pos..pos + len] {
            if !ch.is_ascii() {
                return None;
            }
            // Keep it permissive; callers may still enforce DNS-label rules.
            out.push(char::from(ch));
            if out.len() > 253 {
                return None;
            }
        }
        pos += len;

        guard = guard.wrapping_add(1);
        if guard > 64 {
            return None;
        }
    }

    Some((out, next))
}

enum DnsAnswer {
    A([u8; 4]),
    Cname(String),
    None,
}

fn dns_parse_a_or_cname(pkt: &[u8], want_id: u16) -> Option<DnsAnswer> {
    if pkt.len() < 12 {
        return None;
    }
    let id = u16::from_be_bytes([pkt[0], pkt[1]]);
    if id != want_id {
        return None;
    }
    let flags = u16::from_be_bytes([pkt[2], pkt[3]]);
    let rcode = (flags & 0x000F) as u8;
    if rcode != 0 {
        return None;
    }

    let qd = u16::from_be_bytes([pkt[4], pkt[5]]) as usize;
    let an = u16::from_be_bytes([pkt[6], pkt[7]]) as usize;
    let mut idx = 12usize;

    for _ in 0..qd {
        if !dns_skip_name(pkt, &mut idx) {
            return None;
        }
        if idx + 4 > pkt.len() {
            return None;
        }
        idx += 4;
    }

    let mut cname: Option<String> = None;

    for _ in 0..an {
        if !dns_skip_name(pkt, &mut idx) {
            return None;
        }
        if idx + 10 > pkt.len() {
            return None;
        }
        let typ = u16::from_be_bytes([pkt[idx], pkt[idx + 1]]);
        let class = u16::from_be_bytes([pkt[idx + 2], pkt[idx + 3]]);
        let rdlen = u16::from_be_bytes([pkt[idx + 8], pkt[idx + 9]]) as usize;
        idx += 10;

        if idx + rdlen > pkt.len() {
            return None;
        }

        if class == 1 {
            if typ == 1 && rdlen == 4 {
                return Some(DnsAnswer::A([pkt[idx], pkt[idx + 1], pkt[idx + 2], pkt[idx + 3]]));
            }
            if typ == 5 && cname.is_none() {
                if let Some((name, _next)) = dns_read_name(pkt, idx) {
                    if !name.is_empty() {
                        cname = Some(name);
                    }
                }
            }
        }

        idx += rdlen;
    }

    if let Some(c) = cname {
        Some(DnsAnswer::Cname(c))
    } else {
        Some(DnsAnswer::None)
    }
}

async fn open_udp(net: &VNet, local_port: u16, timeout_ms: u64) -> Option<vnet::NetHandle> {
    let _ = net.submit(vnet::Command::OpenUdp { port: local_port });

    let deadline = Instant::now() + EmbassyDuration::from_millis(timeout_ms);
    loop {
        for _ in 0..64 {
            let Some(ev) = net.pop_event() else {
                break;
            };
            if let vnet::Event::Opened { handle, kind } = ev {
                if kind == vnet::SocketKind::Udp {
                    return Some(handle);
                }
            }
        }
        if Instant::now() >= deadline {
            return None;
        }
        Timer::after(EmbassyDuration::from_millis(5)).await;
    }
}

/// Resolve a hostname to an IPv4 address (A record) using UDP DNS over vnet.
///
/// Supports limited CNAME chasing (bounded by `cfg.cname_depth`).
pub async fn resolve_ipv4_for_device(
    dev_idx: usize,
    host: &str,
    cfg: DnsConfig,
) -> Result<[u8; 4], DnsError> {
    let net = VNet::open(dev_idx).ok_or(DnsError::NoNic)?;

    let local_port = alloc_local_port();
    let Some(udp) = open_udp(&net, local_port, cfg.timeout_ms.min(500)).await else {
        return Err(DnsError::Timeout);
    };

    let mut current = host.trim().trim_end_matches('.').to_string();
    if current.is_empty() {
        let _ = net.submit(vnet::Command::Close { handle: udp });
        return Err(DnsError::BadName);
    }

    let total_deadline = Instant::now() + EmbassyDuration::from_millis(cfg.timeout_ms);

    let mut depth: u8 = 0;
    loop {
        if depth > cfg.cname_depth {
            let _ = net.submit(vnet::Command::Close { handle: udp });
            return Err(DnsError::NoAnswer);
        }

        let dns_id = alloc_dns_id();
        let q = dns_make_query(dns_id, current.as_str(), 1)?;

        let mut answered: Option<DnsAnswer> = None;

        for &server in cfg.servers.iter() {
            let mut attempt = 0u8;
            while attempt < 3 {
                let _ = net.submit(vnet::Command::SendUdp {
                    handle: udp,
                    remote: vnet::EndpointV4 {
                        addr: server,
                        port: DNS_PORT,
                    },
                    data: vnet::ByteBuf::from_slice_trunc(&q),
                });

                let per_try_deadline = Instant::now() + EmbassyDuration::from_millis(cfg.resend_ms);
                loop {
                    for _ in 0..64 {
                        let Some(ev) = net.pop_event() else {
                            break;
                        };
                        match ev {
                            vnet::Event::UdpPacket { handle, from, data } => {
                                if handle != udp {
                                    continue;
                                }
                                if from.port != DNS_PORT || from.addr != server {
                                    continue;
                                }
                                if let Some(ans) = dns_parse_a_or_cname(data.as_slice(), dns_id) {
                                    answered = Some(ans);
                                    break;
                                }
                            }
                            vnet::Event::Error { .. } => {}
                            _ => {}
                        }
                    }

                    if answered.is_some() {
                        break;
                    }
                    if Instant::now() >= per_try_deadline {
                        break;
                    }
                    if Instant::now() >= total_deadline {
                        let _ = net.submit(vnet::Command::Close { handle: udp });
                        return Err(DnsError::Timeout);
                    }
                    Timer::after(EmbassyDuration::from_millis(10)).await;
                }

                if answered.is_some() {
                    break;
                }

                attempt = attempt.wrapping_add(1);
            }

            if answered.is_some() {
                break;
            }
        }

        match answered.unwrap_or(DnsAnswer::None) {
            DnsAnswer::A(ip) => {
                let _ = net.submit(vnet::Command::Close { handle: udp });
                return Ok(ip);
            }
            DnsAnswer::Cname(next) => {
                current = next;
                depth = depth.wrapping_add(1);
                continue;
            }
            DnsAnswer::None => {
                let _ = net.submit(vnet::Command::Close { handle: udp });
                return Err(DnsError::NoAnswer);
            }
        }
    }
}

#[inline]
pub async fn resolve_ipv4_primary(host: &str, cfg: DnsConfig) -> Result<[u8; 4], DnsError> {
    resolve_ipv4_for_device(0, host, cfg).await
}
