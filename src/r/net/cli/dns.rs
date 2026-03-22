#![allow(dead_code)]

extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use heapless::Vec as HVec;
use spin::Mutex;

use v::vnet;

use crate::r::net::{NetProfile, VNet};

#[derive(Clone, Copy, Debug)]
pub enum DnsError {
    NoNic,
    BadName,
    Timeout,
    NoAnswer,
}

#[derive(Clone, Copy, Debug)]
pub struct DnsConfig {
    pub servers: [DnsServer; 8],
    pub server_count: u8,
    pub timeout_ms: u64,
    pub resend_ms: u64,
    pub cname_depth: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DnsServer {
    V4([u8; 4]),
    V6([u8; 16]),
}

impl Default for DnsConfig {
    fn default() -> Self {
        Self::for_profile(NetProfile::default())
    }
}

impl DnsConfig {
    pub fn for_profile(profile: NetProfile) -> Self {
        if let Some(dev_idx) = profile.resolve_device_index() {
            Self::for_device(dev_idx)
        } else {
            Self::for_device(0)
        }
    }

    pub fn for_device(dev_idx: usize) -> Self {
        let (ra6, ra6_count) = crate::net::adapter::ra_dns6_snapshot_at(dev_idx)
            .unwrap_or_else(crate::net::adapter::primary_ra_dns6_snapshot);
        let (dhcp6, dhcp6_count) = crate::net::adapter::dhcp6_dns6_snapshot_at(dev_idx)
            .unwrap_or_else(crate::net::adapter::primary_dhcp6_dns6_snapshot);
        let (dhcp4, dhcp4_count) = crate::net::adapter::dhcp_dns_snapshot_at(dev_idx)
            .unwrap_or_else(crate::net::adapter::primary_dhcp_dns_snapshot);

        // Prefer network-provided DNS (RA RDNSS for v6, then DHCPv6, then DHCPv4), then fall back
        // to public resolvers. Keep both v6 and v4 fallbacks so we work on:
        // - v6-only networks (need v6 resolvers)
        // - v4-only networks (need v4 resolvers)
        // - dual-stack (either)
        let mut servers = [DnsServer::V4([0u8; 4]); 8];
        let mut n: u8 = 0;

        for i in 0..(ra6_count as usize).min(ra6.len()) {
            if (n as usize) >= servers.len() {
                break;
            }
            servers[n as usize] = DnsServer::V6(ra6[i]);
            n = n.saturating_add(1);
        }
        for i in 0..(dhcp6_count as usize).min(dhcp6.len()) {
            if (n as usize) >= servers.len() {
                break;
            }
            servers[n as usize] = DnsServer::V6(dhcp6[i]);
            n = n.saturating_add(1);
        }
        for i in 0..(dhcp4_count as usize).min(dhcp4.len()) {
            if (n as usize) >= servers.len() {
                break;
            }
            servers[n as usize] = DnsServer::V4(dhcp4[i]);
            n = n.saturating_add(1);
        }
        for s in PUBLIC_DNS_SERVERS_V6.iter() {
            if (n as usize) >= servers.len() {
                break;
            }
            servers[n as usize] = DnsServer::V6(*s);
            n = n.saturating_add(1);
        }
        for s in PUBLIC_DNS_SERVERS_V4.iter() {
            if (n as usize) >= servers.len() {
                break;
            }
            servers[n as usize] = DnsServer::V4(*s);
            n = n.saturating_add(1);
        }

        Self {
            servers,
            server_count: n,
            // Loader/CDN imports are sensitive to resolver jitter; use a less aggressive
            // default than 1.5s to avoid spurious NET_ERR_TIMEOUT_DNS.
            timeout_ms: 4000,
            resend_ms: 500,
            cname_depth: 6,
        }
    }

    /// DNS config that uses IPv4 resolvers only.
    ///
    /// This is useful for IPv4-only workflows (like the boot netbench URL),
    /// where trying IPv6 resolvers first can introduce unnecessary delays or
    /// confusing logs on networks with partial/broken IPv6.
    pub fn for_device_v4_only(dev_idx: usize) -> Self {
        let (dhcp4, dhcp4_count) = crate::net::adapter::dhcp_dns_snapshot_at(dev_idx)
            .unwrap_or_else(crate::net::adapter::primary_dhcp_dns_snapshot);

        let mut servers = [DnsServer::V4([0u8; 4]); 8];
        let mut n: u8 = 0;

        for i in 0..(dhcp4_count as usize).min(dhcp4.len()) {
            if (n as usize) >= servers.len() {
                break;
            }
            servers[n as usize] = DnsServer::V4(dhcp4[i]);
            n = n.saturating_add(1);
        }
        for s in PUBLIC_DNS_SERVERS_V4.iter() {
            if (n as usize) >= servers.len() {
                break;
            }
            servers[n as usize] = DnsServer::V4(*s);
            n = n.saturating_add(1);
        }

        Self {
            servers,
            server_count: n,
            timeout_ms: 4000,
            resend_ms: 500,
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

pub const PUBLIC_DNS_SERVERS_V4: [[u8; 4]; 3] = PUBLIC_DNS_SERVERS;

pub const PUBLIC_DNS_SERVERS_V6: [[u8; 16]; 3] = [
    // Cloudflare: 2606:4700:4700::1111
    [
        0x26, 0x06, 0x47, 0x00, 0x47, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x11,
        0x11,
    ],
    // Google: 2001:4860:4860::8888
    [
        0x20, 0x01, 0x48, 0x60, 0x48, 0x60, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x88,
        0x88,
    ],
    // Quad9: 2620:fe::fe
    [
        0x26, 0x20, 0x00, 0xfe, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0xfe,
    ],
];

const DNS_PORT: u16 = 53;

const DNS_CACHE_CAP: usize = 8;
// Keep this short; it's mainly to avoid repeated lookups during module loading.
const DNS_CACHE_TTL_TICKS: u64 = 15 * embassy_time_driver::TICK_HZ;
const DNS_FS_CACHE_PATH: &str = "net_dns_v4.cache";
const DNS_FS_CACHE_MAX_ENTRIES: usize = 32;

#[derive(Clone, Debug)]
struct DnsCacheEntry {
    dev_idx: u8,
    host: heapless::String<96>,
    ip: [u8; 4],
    expires_at: u64,
}

static DNS_CACHE: Mutex<HVec<DnsCacheEntry, DNS_CACHE_CAP>> = Mutex::new(HVec::new());

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

fn log_dns_ip(prefix: &str, host: &str, dev_idx: usize, ip: [u8; 4]) {
    crate::log!(
        "{} host={} dev={} ip={}.{}.{}.{}\n",
        prefix,
        host,
        dev_idx,
        ip[0],
        ip[1],
        ip[2],
        ip[3]
    );
}

fn dns_cache_insert(dev_idx: usize, host_trimmed: &str, ip: [u8; 4]) {
    let Ok(hs) = heapless::String::<96>::try_from(host_trimmed) else {
        return;
    };
    let mut cache = DNS_CACHE.lock();
    cache.retain(|e| e.expires_at > embassy_time_driver::now());
    if let Some(e) = cache
        .iter_mut()
        .find(|e| e.dev_idx as usize == dev_idx && e.host.as_str() == hs.as_str())
    {
        e.ip = ip;
        e.expires_at = embassy_time_driver::now().saturating_add(DNS_CACHE_TTL_TICKS);
        return;
    }
    let dev = dev_idx.min(255) as u8;
    let _ = cache.push(DnsCacheEntry {
        dev_idx: dev,
        host: {
            let mut s = heapless::String::<96>::new();
            let _ = s.push_str(hs.as_str());
            s
        },
        ip,
        expires_at: embassy_time_driver::now().saturating_add(DNS_CACHE_TTL_TICKS),
    });
}

fn parse_ipv4_text(text: &str) -> Option<[u8; 4]> {
    let mut out = [0u8; 4];
    let mut parts = text.split('.');
    for octet in &mut out {
        let value = parts.next()?.trim().parse::<u8>().ok()?;
        *octet = value;
    }
    if parts.next().is_some() {
        return None;
    }
    Some(out)
}

fn parse_dns_fs_cache_line(line: &str) -> Option<(usize, &str, [u8; 4])> {
    let mut parts = line.splitn(3, '|');
    let dev_idx = parts.next()?.trim().parse::<usize>().ok()?;
    let host = parts.next()?.trim();
    let ip = parse_ipv4_text(parts.next()?.trim())?;
    if host.is_empty() {
        return None;
    }
    Some((dev_idx, host, ip))
}

async fn dns_fs_cache_lookup(dev_idx: usize, host_trimmed: &str) -> Option<[u8; 4]> {
    let disk = crate::r::fs::trueosfs::primary_root_handle()?;
    let bytes = crate::r::fs::trueosfs::file_out_async(disk, DNS_FS_CACHE_PATH)
        .await
        .ok()
        .flatten()?;
    let text = core::str::from_utf8(bytes.as_slice()).ok()?;
    let mut found: Option<[u8; 4]> = None;
    for line in text.lines() {
        let Some((line_dev_idx, line_host, line_ip)) = parse_dns_fs_cache_line(line) else {
            continue;
        };
        if line_dev_idx == dev_idx && line_host == host_trimmed {
            found = Some(line_ip);
        }
    }
    found
}

async fn dns_fs_cache_update(dev_idx: usize, host_trimmed: &str, ip: [u8; 4]) {
    let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
        return;
    };

    let existing = crate::r::fs::trueosfs::file_out_async(disk, DNS_FS_CACHE_PATH)
        .await
        .ok()
        .flatten();

    let mut lines: Vec<String> = Vec::new();
    if let Some(bytes) = existing
        && let Ok(text) = core::str::from_utf8(bytes.as_slice())
    {
        for line in text.lines() {
            let Some((line_dev_idx, line_host, line_ip)) = parse_dns_fs_cache_line(line) else {
                continue;
            };
            if line_dev_idx == dev_idx && line_host == host_trimmed {
                continue;
            }
            lines.push(format!(
                "{}|{}|{}.{}.{}.{}",
                line_dev_idx, line_host, line_ip[0], line_ip[1], line_ip[2], line_ip[3]
            ));
        }
    }

    lines.push(format!(
        "{}|{}|{}.{}.{}.{}",
        dev_idx, host_trimmed, ip[0], ip[1], ip[2], ip[3]
    ));
    if lines.len() > DNS_FS_CACHE_MAX_ENTRIES {
        let drop_n = lines.len().saturating_sub(DNS_FS_CACHE_MAX_ENTRIES);
        lines.drain(0..drop_n);
    }

    let mut body = String::new();
    for (idx, line) in lines.iter().enumerate() {
        if idx > 0 {
            body.push('\n');
        }
        body.push_str(line.as_str());
    }
    let _ = crate::r::fs::trueosfs::file_in_async(disk, DNS_FS_CACHE_PATH, body.as_bytes()).await;
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

enum DnsAnswer6 {
    Aaaa([u8; 16]),
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
                return Some(DnsAnswer::A([
                    pkt[idx],
                    pkt[idx + 1],
                    pkt[idx + 2],
                    pkt[idx + 3],
                ]));
            }
            if typ == 5
                && cname.is_none()
                && let Some((name, _next)) = dns_read_name(pkt, idx)
                && !name.is_empty()
            {
                cname = Some(name);
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

fn dns_parse_aaaa_or_cname(pkt: &[u8], want_id: u16) -> Option<DnsAnswer6> {
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
            if typ == 28 && rdlen == 16 {
                let mut ip = [0u8; 16];
                ip.copy_from_slice(&pkt[idx..idx + 16]);
                return Some(DnsAnswer6::Aaaa(ip));
            }
            if typ == 5
                && cname.is_none()
                && let Some((name, _next)) = dns_read_name(pkt, idx)
                && !name.is_empty()
            {
                cname = Some(name);
            }
        }

        idx += rdlen;
    }

    if let Some(c) = cname {
        Some(DnsAnswer6::Cname(c))
    } else {
        Some(DnsAnswer6::None)
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
            if let vnet::Event::Opened { handle, kind } = ev
                && kind == vnet::SocketKind::Udp
            {
                return Some(handle);
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
    let host_trimmed = host.trim().trim_end_matches('.');
    if host_trimmed.is_empty() {
        return Err(DnsError::BadName);
    }

    // Fast path: small in-kernel DNS cache to avoid repeated lookups with short timeouts.
    // This is especially helpful for QJS module loading (many imports hit the same host).
    {
        let now = embassy_time_driver::now();
        let mut cache = DNS_CACHE.lock();
        // Drop expired entries opportunistically.
        cache.retain(|e| e.expires_at > now);
        if let Some(e) = cache
            .iter()
            .find(|e| e.dev_idx as usize == dev_idx && e.host.as_str() == host_trimmed)
        {
            log_dns_ip("dns: cache hit", host_trimmed, dev_idx, e.ip);
            return Ok(e.ip);
        }
    }

    if let Some(ip) = dns_fs_cache_lookup(dev_idx, host_trimmed).await {
        dns_cache_insert(dev_idx, host_trimmed, ip);
        log_dns_ip("dns: fs-cache hit", host_trimmed, dev_idx, ip);
        return Ok(ip);
    }

    let net = VNet::open(dev_idx).ok_or(DnsError::NoNic)?;

    let local_port = alloc_local_port();
    let Some(udp) = open_udp(&net, local_port, cfg.timeout_ms).await else {
        return Err(DnsError::Timeout);
    };

    let mut current: String = host_trimmed.into();

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

        for idx in 0..(cfg.server_count as usize).min(cfg.servers.len()) {
            let server = cfg.servers[idx];
            let mut attempt = 0u8;
            while attempt < 3 {
                match server {
                    DnsServer::V4(addr) => {
                        let _ = net.submit(vnet::Command::SendUdp {
                            handle: udp,
                            remote: vnet::EndpointV4 {
                                addr,
                                port: DNS_PORT,
                            },
                            data: vnet::ByteBuf::from_slice_trunc(&q),
                        });
                    }
                    DnsServer::V6(addr) => {
                        let _ = net.submit(vnet::Command::SendUdpV6 {
                            handle: udp,
                            remote: vnet::EndpointV6 {
                                addr,
                                port: DNS_PORT,
                            },
                            data: vnet::ByteBuf::from_slice_trunc(&q),
                        });
                    }
                }

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
                                let DnsServer::V4(server) = server else {
                                    continue;
                                };
                                if from.port != DNS_PORT || from.addr != server {
                                    continue;
                                }
                                if let Some(ans) = dns_parse_a_or_cname(data.as_slice(), dns_id) {
                                    answered = Some(ans);
                                    break;
                                }
                            }
                            vnet::Event::UdpPacketV6 { handle, from, data } => {
                                if handle != udp {
                                    continue;
                                }
                                let DnsServer::V6(server) = server else {
                                    continue;
                                };
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
                log_dns_ip("dns: resolved", host_trimmed, dev_idx, ip);
                let _ = net.submit(vnet::Command::Close { handle: udp });
                dns_cache_insert(dev_idx, host_trimmed, ip);
                dns_fs_cache_update(dev_idx, host_trimmed, ip).await;
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

/// Resolve a hostname to an IPv6 address (AAAA record) using UDP DNS over vnet.
///
/// Uses the same resolver flow as IPv4 but queries qtype AAAA (28).
pub async fn resolve_ipv6_for_device(
    dev_idx: usize,
    host: &str,
    cfg: DnsConfig,
) -> Result<[u8; 16], DnsError> {
    let host_trimmed = host.trim().trim_end_matches('.');
    if host_trimmed.is_empty() {
        return Err(DnsError::BadName);
    }

    let net = VNet::open(dev_idx).ok_or(DnsError::NoNic)?;

    let local_port = alloc_local_port();
    let Some(udp) = open_udp(&net, local_port, cfg.timeout_ms).await else {
        return Err(DnsError::Timeout);
    };

    let mut current: String = host_trimmed.into();
    let total_deadline = Instant::now() + EmbassyDuration::from_millis(cfg.timeout_ms);

    let mut depth: u8 = 0;
    loop {
        if depth > cfg.cname_depth {
            let _ = net.submit(vnet::Command::Close { handle: udp });
            return Err(DnsError::NoAnswer);
        }

        let dns_id = alloc_dns_id();
        let q = dns_make_query(dns_id, current.as_str(), 28)?; // AAAA

        let mut answered: Option<DnsAnswer6> = None;

        for idx in 0..(cfg.server_count as usize).min(cfg.servers.len()) {
            let server = cfg.servers[idx];
            let mut attempt = 0u8;
            while attempt < 3 {
                match server {
                    DnsServer::V4(addr) => {
                        let _ = net.submit(vnet::Command::SendUdp {
                            handle: udp,
                            remote: vnet::EndpointV4 {
                                addr,
                                port: DNS_PORT,
                            },
                            data: vnet::ByteBuf::from_slice_trunc(&q),
                        });
                    }
                    DnsServer::V6(addr) => {
                        let _ = net.submit(vnet::Command::SendUdpV6 {
                            handle: udp,
                            remote: vnet::EndpointV6 {
                                addr,
                                port: DNS_PORT,
                            },
                            data: vnet::ByteBuf::from_slice_trunc(&q),
                        });
                    }
                }

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
                                let DnsServer::V4(server) = server else {
                                    continue;
                                };
                                if from.port != DNS_PORT || from.addr != server {
                                    continue;
                                }
                                if let Some(ans) = dns_parse_aaaa_or_cname(data.as_slice(), dns_id)
                                {
                                    answered = Some(ans);
                                    break;
                                }
                            }
                            vnet::Event::UdpPacketV6 { handle, from, data } => {
                                if handle != udp {
                                    continue;
                                }
                                let DnsServer::V6(server) = server else {
                                    continue;
                                };
                                if from.port != DNS_PORT || from.addr != server {
                                    continue;
                                }
                                if let Some(ans) = dns_parse_aaaa_or_cname(data.as_slice(), dns_id)
                                {
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

        match answered.unwrap_or(DnsAnswer6::None) {
            DnsAnswer6::Aaaa(ip) => {
                crate::log!(
                    "dns: resolved6 host={} dev={} ip={:02x}{:02x}:{:02x}{:02x}:...\n",
                    host_trimmed,
                    dev_idx,
                    ip[0],
                    ip[1],
                    ip[2],
                    ip[3]
                );
                let _ = net.submit(vnet::Command::Close { handle: udp });
                return Ok(ip);
            }
            DnsAnswer6::Cname(next) => {
                current = next;
                depth = depth.wrapping_add(1);
                continue;
            }
            DnsAnswer6::None => {
                let _ = net.submit(vnet::Command::Close { handle: udp });
                return Err(DnsError::NoAnswer);
            }
        }
    }
}

#[inline]
pub async fn resolve_ipv4_primary(host: &str, cfg: DnsConfig) -> Result<[u8; 4], DnsError> {
    resolve_ipv4_with_profile(host, NetProfile::default(), cfg).await
}

#[inline]
pub async fn resolve_ipv4_with_profile(
    host: &str,
    profile: NetProfile,
    cfg: DnsConfig,
) -> Result<[u8; 4], DnsError> {
    let dev_idx = profile.resolve_device_index().ok_or(DnsError::NoNic)?;
    resolve_ipv4_for_device(dev_idx, host, cfg).await
}
