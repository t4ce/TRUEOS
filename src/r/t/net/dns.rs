#![allow(dead_code)]

extern crate alloc;
extern crate std;

use alloc::{format, string::String, vec::Vec};
use core::sync::atomic::{AtomicU32, Ordering};

use heapless::Vec as HVec;
use spin::Mutex;

use crate::r::net::NetProfile;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecureDnsTransport {
    Doh,
    Dot,
    Doh3,
}

impl SecureDnsTransport {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Doh => "doh",
            Self::Dot => "dot",
            Self::Doh3 => "doh3",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SecureDnsPolicy {
    pub enabled: bool,
    pub warn_on_disagreement: bool,
    pub transports: [Option<SecureDnsTransport>; 3],
}

impl Default for SecureDnsPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            warn_on_disagreement: true,
            transports: [
                Some(SecureDnsTransport::Doh),
                Some(SecureDnsTransport::Dot),
                None,
            ],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DnsIpv4Candidate {
    transport: SecureDnsTransport,
    ip: [u8; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DnsIpv6Candidate {
    transport: SecureDnsTransport,
    ip: [u8; 16],
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
        let router4 = crate::net::adapter::ipv4_router_snapshot_at(dev_idx)
            .flatten()
            .or_else(crate::net::adapter::primary_ipv4_router_snapshot);

        let mut servers = [DnsServer::V4([0u8; 4]); 8];
        let mut n: u8 = 0;

        for i in 0..(dhcp4_count as usize).min(dhcp4.len()) {
            if (n as usize) >= servers.len() {
                break;
            }
            servers[n as usize] = DnsServer::V4(dhcp4[i]);
            n = n.saturating_add(1);
        }
        if let Some(router) = router4 {
            let duplicate = servers[..(n as usize)]
                .iter()
                .any(|server| matches!(server, DnsServer::V4(addr) if *addr == router));
            if !duplicate && (n as usize) < servers.len() {
                servers[n as usize] = DnsServer::V4(router);
                n = n.saturating_add(1);
            }
        }
        for s in PUBLIC_DNS_SERVERS_V4.iter() {
            if (n as usize) >= servers.len() {
                break;
            }
            let duplicate = servers[..(n as usize)]
                .iter()
                .any(|server| matches!(server, DnsServer::V4(addr) if addr == s));
            if duplicate {
                continue;
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

const DOT_PORT: u16 = 853;

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

static SECURE_DNS_STUB_LOGGED: AtomicU32 = AtomicU32::new(0);
static CLASSIC_DNS_DISABLED_LOGGED: AtomicU32 = AtomicU32::new(0);

#[derive(Clone, Copy, Debug)]
enum SecureEndpointAddr {
    V4([u8; 4]),
    V6([u8; 16]),
}

#[derive(Clone, Copy, Debug)]
struct DotEndpoint {
    addr: SecureEndpointAddr,
    tls_name: &'static str,
}

const PUBLIC_DOT_ENDPOINTS: [DotEndpoint; 6] = [
    DotEndpoint {
        addr: SecureEndpointAddr::V4([1, 1, 1, 1]),
        tls_name: "cloudflare-dns.com",
    },
    DotEndpoint {
        addr: SecureEndpointAddr::V4([8, 8, 8, 8]),
        tls_name: "dns.google",
    },
    DotEndpoint {
        addr: SecureEndpointAddr::V4([9, 9, 9, 9]),
        tls_name: "dns.quad9.net",
    },
    DotEndpoint {
        addr: SecureEndpointAddr::V6([
            0x26, 0x06, 0x47, 0x00, 0x47, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x11, 0x11,
        ]),
        tls_name: "cloudflare-dns.com",
    },
    DotEndpoint {
        addr: SecureEndpointAddr::V6([
            0x20, 0x01, 0x48, 0x60, 0x48, 0x60, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x88, 0x88,
        ]),
        tls_name: "dns.google",
    },
    DotEndpoint {
        addr: SecureEndpointAddr::V6([
            0x26, 0x20, 0x00, 0xfe, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0xfe,
        ]),
        tls_name: "dns.quad9.net",
    },
];

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

fn log_dns_transport_ip(
    prefix: &str,
    transport: SecureDnsTransport,
    host: &str,
    dev_idx: usize,
    ip: [u8; 4],
) {
    crate::log!(
        "{} transport={} host={} dev={} ip={}.{}.{}.{}\n",
        prefix,
        transport.label(),
        host,
        dev_idx,
        ip[0],
        ip[1],
        ip[2],
        ip[3]
    );
}

fn log_dns_transport_ip6(
    prefix: &str,
    transport: SecureDnsTransport,
    host: &str,
    dev_idx: usize,
    ip: [u8; 16],
) {
    crate::log!(
        "{} transport={} host={} dev={} ip6={:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}\n",
        prefix,
        transport.label(),
        host,
        dev_idx,
        ip[0],
        ip[1],
        ip[2],
        ip[3],
        ip[4],
        ip[5],
        ip[6],
        ip[7],
        ip[8],
        ip[9],
        ip[10],
        ip[11],
        ip[12],
        ip[13],
        ip[14],
        ip[15]
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

    lines.push(format!("{}|{}|{}.{}.{}.{}", dev_idx, host_trimmed, ip[0], ip[1], ip[2], ip[3]));
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

async fn resolve_ipv4_secure_transport(
    transport: SecureDnsTransport,
    dev_idx: usize,
    host_trimmed: &str,
    cfg: DnsConfig,
) -> Result<[u8; 4], DnsError> {
    match transport {
        SecureDnsTransport::Doh => resolve_ipv4_doh_for_device(dev_idx, host_trimmed, cfg).await,
        SecureDnsTransport::Dot => resolve_ipv4_dot_for_device(dev_idx, host_trimmed, cfg).await,
        SecureDnsTransport::Doh3 => Err(DnsError::NoAnswer),
    }
}

fn dot_runtime_ready() -> bool {
    tokio::runtime::Handle::try_current().is_ok()
}

pub(crate) fn dot_resolver_config() -> hickory_resolver::config::ResolverConfig {
    use hickory_resolver::config::{NameServerConfig, ResolverConfig};
    use hickory_resolver::proto::xfer::Protocol as DnsProtocol;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    let mut servers = Vec::new();
    for endpoint in PUBLIC_DOT_ENDPOINTS.iter() {
        let socket_addr = match endpoint.addr {
            SecureEndpointAddr::V4(addr) => {
                SocketAddr::from((IpAddr::V4(Ipv4Addr::from(addr)), DOT_PORT))
            }
            SecureEndpointAddr::V6(addr) => {
                SocketAddr::from((IpAddr::V6(std::net::Ipv6Addr::from(addr)), DOT_PORT))
            }
        };
        let mut server = NameServerConfig::new(
            socket_addr,
            DnsProtocol::Tls,
        );
        server.tls_dns_name = Some(String::from(endpoint.tls_name));
        server.trust_negative_responses = false;
        servers.push(server);
    }

    ResolverConfig::from_parts(None, Vec::new(), servers)
}

async fn resolve_ipv4_dot_for_device(
    dev_idx: usize,
    host_trimmed: &str,
    cfg: DnsConfig,
) -> Result<[u8; 4], DnsError> {
    if !dot_runtime_ready() {
        crate::log!(
            "dns: dot skipped; tokio runtime unavailable host={} dev={} qtype=A\n",
            host_trimmed,
            dev_idx
        );
        return Err(DnsError::NoAnswer);
    }

    crate::log!(
        "dns: dot enter host={} dev={} qtype=A timeout_ms={}\n",
        host_trimmed,
        dev_idx,
        cfg.timeout_ms.max(cfg.resend_ms).max(100)
    );

    use hickory_resolver::config::{LookupIpStrategy, ResolveHosts, ResolverOpts};
    use hickory_resolver::name_server::TokioConnectionProvider;

    let mut opts = ResolverOpts::default();
    let query_timeout_ms = cfg.timeout_ms.max(cfg.resend_ms).max(100);
    opts.timeout = core::time::Duration::from_millis(query_timeout_ms);
    opts.attempts = 1;
    opts.ip_strategy = LookupIpStrategy::Ipv4Only;
    opts.num_concurrent_reqs = 1;
    opts.use_hosts_file = ResolveHosts::Never;
    opts.try_tcp_on_error = false;
    opts.os_port_selection = true;

    let resolver = hickory_resolver::Resolver::builder_with_config(
        dot_resolver_config(),
        TokioConnectionProvider::default(),
    )
    .with_options(opts)
    .build();
    crate::log!("dns: dot resolver built host={} dev={} qtype=A\n", host_trimmed, dev_idx);

    match tokio::time::timeout(
        core::time::Duration::from_millis(query_timeout_ms),
        resolver.ipv4_lookup(host_trimmed),
    )
    .await
    {
        Ok(Ok(lookup)) => {
            if let Some(ip) = lookup.iter().next() {
                return Ok(ip.octets());
            }
            crate::log!(
                "dns: dot lookup empty host={} dev={} qtype=A\n",
                host_trimmed,
                dev_idx
            );
            Err(DnsError::NoAnswer)
        }
        Ok(Err(err)) => {
            crate::log!(
                "dns: dot lookup failed host={} dev={} err={}\n",
                host_trimmed,
                dev_idx,
                err
            );
            Err(DnsError::NoAnswer)
        }
        Err(_) => {
            crate::log!(
                "dns: dot lookup timeout host={} dev={} qtype=A timeout_ms={}\n",
                host_trimmed,
                dev_idx,
                query_timeout_ms
            );
            Err(DnsError::Timeout)
        }
    }
}

async fn resolve_ipv6_dot_for_device(
    dev_idx: usize,
    host_trimmed: &str,
    cfg: DnsConfig,
) -> Result<[u8; 16], DnsError> {
    if !dot_runtime_ready() {
        crate::log!(
            "dns: dot skipped; tokio runtime unavailable host={} dev={} qtype=AAAA\n",
            host_trimmed,
            dev_idx
        );
        return Err(DnsError::NoAnswer);
    }

    crate::log!(
        "dns: dot enter host={} dev={} qtype=AAAA timeout_ms={}\n",
        host_trimmed,
        dev_idx,
        cfg.timeout_ms.max(cfg.resend_ms).max(100)
    );

    use hickory_resolver::config::{LookupIpStrategy, ResolveHosts, ResolverOpts};
    use hickory_resolver::name_server::TokioConnectionProvider;

    let mut opts = ResolverOpts::default();
    let query_timeout_ms = cfg.timeout_ms.max(cfg.resend_ms).max(100);
    opts.timeout = core::time::Duration::from_millis(query_timeout_ms);
    opts.attempts = 1;
    opts.ip_strategy = LookupIpStrategy::Ipv6Only;
    opts.num_concurrent_reqs = 1;
    opts.use_hosts_file = ResolveHosts::Never;
    opts.try_tcp_on_error = false;
    opts.os_port_selection = true;

    let resolver = hickory_resolver::Resolver::builder_with_config(
        dot_resolver_config(),
        TokioConnectionProvider::default(),
    )
    .with_options(opts)
    .build();
    crate::log!(
        "dns: dot resolver built host={} dev={} qtype=AAAA\n",
        host_trimmed,
        dev_idx
    );

    match tokio::time::timeout(
        core::time::Duration::from_millis(query_timeout_ms),
        resolver.ipv6_lookup(host_trimmed),
    )
    .await
    {
        Ok(Ok(lookup)) => {
            if let Some(ip) = lookup.iter().next() {
                return Ok(ip.octets());
            }
            crate::log!(
                "dns: dot lookup empty host={} dev={} qtype=AAAA\n",
                host_trimmed,
                dev_idx
            );
            Err(DnsError::NoAnswer)
        }
        Ok(Err(err)) => {
            crate::log!(
                "dns: dot ipv6 lookup failed host={} dev={} err={}\n",
                host_trimmed,
                dev_idx,
                err
            );
            Err(DnsError::NoAnswer)
        }
        Err(_) => {
            crate::log!(
                "dns: dot lookup timeout host={} dev={} qtype=AAAA timeout_ms={}\n",
                host_trimmed,
                dev_idx,
                query_timeout_ms
            );
            Err(DnsError::Timeout)
        }
    }
}

struct DohEndpoint {
    addr: SecureEndpointAddr,
    tls_name: &'static str,
    path: &'static str,
}

const PUBLIC_DOH_ENDPOINTS: [DohEndpoint; 6] = [
    DohEndpoint {
        addr: SecureEndpointAddr::V4([1, 1, 1, 1]),
        tls_name: "cloudflare-dns.com",
        path: "/dns-query",
    },
    DohEndpoint {
        addr: SecureEndpointAddr::V4([8, 8, 8, 8]),
        tls_name: "dns.google",
        path: "/dns-query",
    },
    DohEndpoint {
        addr: SecureEndpointAddr::V4([9, 9, 9, 9]),
        tls_name: "dns.quad9.net",
        path: "/dns-query",
    },
    DohEndpoint {
        addr: SecureEndpointAddr::V6([
            0x26, 0x06, 0x47, 0x00, 0x47, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x11, 0x11,
        ]),
        tls_name: "cloudflare-dns.com",
        path: "/dns-query",
    },
    DohEndpoint {
        addr: SecureEndpointAddr::V6([
            0x20, 0x01, 0x48, 0x60, 0x48, 0x60, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x88, 0x88,
        ]),
        tls_name: "dns.google",
        path: "/dns-query",
    },
    DohEndpoint {
        addr: SecureEndpointAddr::V6([
            0x26, 0x20, 0x00, 0xfe, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0xfe,
        ]),
        tls_name: "dns.quad9.net",
        path: "/dns-query",
    },
];

fn doh_runtime_ready() -> bool {
    tokio::runtime::Handle::try_current().is_ok()
}

pub(crate) fn doh_resolver_config() -> hickory_resolver::config::ResolverConfig {
    use hickory_resolver::config::{NameServerConfig, ResolverConfig};
    use hickory_resolver::proto::xfer::Protocol as DnsProtocol;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    let mut servers = Vec::new();
    for endpoint in PUBLIC_DOH_ENDPOINTS.iter() {
        let socket_addr = match endpoint.addr {
            SecureEndpointAddr::V4(addr) => {
                SocketAddr::from((IpAddr::V4(Ipv4Addr::from(addr)), 443))
            }
            SecureEndpointAddr::V6(addr) => {
                SocketAddr::from((IpAddr::V6(std::net::Ipv6Addr::from(addr)), 443))
            }
        };
        let mut server = NameServerConfig::new(
            socket_addr,
            DnsProtocol::Https,
        );
        server.tls_dns_name = Some(String::from(endpoint.tls_name));
        server.http_endpoint = Some(String::from(endpoint.path));
        server.trust_negative_responses = false;
        servers.push(server);
    }

    ResolverConfig::from_parts(None, Vec::new(), servers)
}

async fn resolve_ipv4_doh_for_device(
    dev_idx: usize,
    host_trimmed: &str,
    cfg: DnsConfig,
) -> Result<[u8; 4], DnsError> {
    if !doh_runtime_ready() {
        crate::log!(
            "dns: doh skipped; tokio runtime unavailable host={} dev={} qtype=A\n",
            host_trimmed,
            dev_idx
        );
        return Err(DnsError::NoAnswer);
    }

    crate::log!(
        "dns: doh enter host={} dev={} qtype=A timeout_ms={}\n",
        host_trimmed,
        dev_idx,
        cfg.timeout_ms.max(cfg.resend_ms).max(100)
    );

    use hickory_resolver::config::{LookupIpStrategy, ResolveHosts, ResolverOpts};
    use hickory_resolver::name_server::TokioConnectionProvider;

    let mut opts = ResolverOpts::default();
    let query_timeout_ms = cfg.timeout_ms.max(cfg.resend_ms).max(100);
    opts.timeout = core::time::Duration::from_millis(query_timeout_ms);
    opts.attempts = 1;
    opts.ip_strategy = LookupIpStrategy::Ipv4Only;
    opts.num_concurrent_reqs = 1;
    opts.use_hosts_file = ResolveHosts::Never;
    opts.try_tcp_on_error = false;
    opts.os_port_selection = true;

    let resolver = hickory_resolver::Resolver::builder_with_config(
        doh_resolver_config(),
        TokioConnectionProvider::default(),
    )
    .with_options(opts)
    .build();
    crate::log!("dns: doh resolver built host={} dev={} qtype=A\n", host_trimmed, dev_idx);

    match tokio::time::timeout(
        core::time::Duration::from_millis(query_timeout_ms),
        resolver.ipv4_lookup(host_trimmed),
    )
    .await
    {
        Ok(Ok(lookup)) => {
            if let Some(ip) = lookup.iter().next() {
                return Ok(ip.octets());
            }
            crate::log!(
                "dns: doh lookup empty host={} dev={} qtype=A\n",
                host_trimmed,
                dev_idx
            );
            Err(DnsError::NoAnswer)
        }
        Ok(Err(err)) => {
            crate::log!(
                "dns: doh lookup failed host={} dev={} err={}\n",
                host_trimmed,
                dev_idx,
                err
            );
            Err(DnsError::NoAnswer)
        }
        Err(_) => {
            crate::log!(
                "dns: doh lookup timeout host={} dev={} qtype=A timeout_ms={}\n",
                host_trimmed,
                dev_idx,
                query_timeout_ms
            );
            Err(DnsError::Timeout)
        }
    }
}

async fn resolve_ipv6_doh_for_device(
    dev_idx: usize,
    host_trimmed: &str,
    cfg: DnsConfig,
) -> Result<[u8; 16], DnsError> {
    if !doh_runtime_ready() {
        crate::log!(
            "dns: doh skipped; tokio runtime unavailable host={} dev={} qtype=AAAA\n",
            host_trimmed,
            dev_idx
        );
        return Err(DnsError::NoAnswer);
    }

    crate::log!(
        "dns: doh enter host={} dev={} qtype=AAAA timeout_ms={}\n",
        host_trimmed,
        dev_idx,
        cfg.timeout_ms.max(cfg.resend_ms).max(100)
    );

    use hickory_resolver::config::{LookupIpStrategy, ResolveHosts, ResolverOpts};
    use hickory_resolver::name_server::TokioConnectionProvider;

    let mut opts = ResolverOpts::default();
    let query_timeout_ms = cfg.timeout_ms.max(cfg.resend_ms).max(100);
    opts.timeout = core::time::Duration::from_millis(query_timeout_ms);
    opts.attempts = 1;
    opts.ip_strategy = LookupIpStrategy::Ipv6Only;
    opts.num_concurrent_reqs = 1;
    opts.use_hosts_file = ResolveHosts::Never;
    opts.try_tcp_on_error = false;
    opts.os_port_selection = true;

    let resolver = hickory_resolver::Resolver::builder_with_config(
        doh_resolver_config(),
        TokioConnectionProvider::default(),
    )
    .with_options(opts)
    .build();
    crate::log!(
        "dns: doh resolver built host={} dev={} qtype=AAAA\n",
        host_trimmed,
        dev_idx
    );

    match tokio::time::timeout(
        core::time::Duration::from_millis(query_timeout_ms),
        resolver.ipv6_lookup(host_trimmed),
    )
    .await
    {
        Ok(Ok(lookup)) => {
            if let Some(ip) = lookup.iter().next() {
                return Ok(ip.octets());
            }
            crate::log!(
                "dns: doh lookup empty host={} dev={} qtype=AAAA\n",
                host_trimmed,
                dev_idx
            );
            Err(DnsError::NoAnswer)
        }
        Ok(Err(err)) => {
            crate::log!(
                "dns: doh ipv6 lookup failed host={} dev={} err={}\n",
                host_trimmed,
                dev_idx,
                err
            );
            Err(DnsError::NoAnswer)
        }
        Err(_) => {
            crate::log!(
                "dns: doh lookup timeout host={} dev={} qtype=AAAA timeout_ms={}\n",
                host_trimmed,
                dev_idx,
                query_timeout_ms
            );
            Err(DnsError::Timeout)
        }
    }
}

fn warn_secure_dns_disagreement(
    host_trimmed: &str,
    dev_idx: usize,
    first: DnsIpv4Candidate,
    other: DnsIpv4Candidate,
) {
    if first.ip == other.ip {
        return;
    }

    crate::log!(
        "dns: warning secure disagreement host={} dev={} first={} {}.{}.{}.{} other={} {}.{}.{}.{}\n",
        host_trimmed,
        dev_idx,
        first.transport.label(),
        first.ip[0],
        first.ip[1],
        first.ip[2],
        first.ip[3],
        other.transport.label(),
        other.ip[0],
        other.ip[1],
        other.ip[2],
        other.ip[3]
    );
}

fn record_secure_dns_result(
    first_healthy: &mut Option<DnsIpv4Candidate>,
    policy: SecureDnsPolicy,
    transport: SecureDnsTransport,
    result: Result<[u8; 4], DnsError>,
    host_trimmed: &str,
    dev_idx: usize,
) {
    match result {
        Ok(ip) => {
            let candidate = DnsIpv4Candidate { transport, ip };
            log_dns_transport_ip("dns: secure resolved", transport, host_trimmed, dev_idx, ip);

            if let Some(first) = *first_healthy {
                if policy.warn_on_disagreement {
                    warn_secure_dns_disagreement(host_trimmed, dev_idx, first, candidate);
                }
            } else {
                *first_healthy = Some(candidate);
            }
        }
        Err(DnsError::NoAnswer) => {
            crate::log!(
                "dns: secure transport no-answer transport={} host={} dev={} qtype=A\n",
                transport.label(),
                host_trimmed,
                dev_idx
            );
        }
        Err(err) => {
            crate::log!(
                "dns: secure transport failed transport={} host={} dev={} qtype=A err={:?}\n",
                transport.label(),
                host_trimmed,
                dev_idx,
                err
            );
        }
    }
}

async fn resolve_ipv4_secure_policy(
    dev_idx: usize,
    host_trimmed: &str,
    cfg: DnsConfig,
    policy: SecureDnsPolicy,
) -> Result<[u8; 4], DnsError> {
    if !policy.enabled {
        return Err(DnsError::NoAnswer);
    }

    if SECURE_DNS_STUB_LOGGED
        .compare_exchange(0, 1, Ordering::Relaxed, Ordering::Relaxed)
        .is_ok()
    {
        crate::log!("dns: secure transport policy wired; classic udp dns disabled\n");
    }

    let doh_enabled = policy
        .transports
        .iter()
        .flatten()
        .any(|transport| *transport == SecureDnsTransport::Doh);
    let dot_enabled = policy
        .transports
        .iter()
        .flatten()
        .any(|transport| *transport == SecureDnsTransport::Dot);

    let mut first_healthy: Option<DnsIpv4Candidate> = None;
    if doh_enabled && dot_enabled {
        let (doh_result, dot_result) = tokio::join!(
            resolve_ipv4_doh_for_device(dev_idx, host_trimmed, cfg),
            resolve_ipv4_dot_for_device(dev_idx, host_trimmed, cfg)
        );
        record_secure_dns_result(
            &mut first_healthy,
            policy,
            SecureDnsTransport::Doh,
            doh_result,
            host_trimmed,
            dev_idx,
        );
        record_secure_dns_result(
            &mut first_healthy,
            policy,
            SecureDnsTransport::Dot,
            dot_result,
            host_trimmed,
            dev_idx,
        );
    }

    for transport in policy.transports.iter().flatten().copied() {
        if (transport == SecureDnsTransport::Doh && doh_enabled && dot_enabled)
            || (transport == SecureDnsTransport::Dot && doh_enabled && dot_enabled)
        {
            continue;
        }
        let result = resolve_ipv4_secure_transport(transport, dev_idx, host_trimmed, cfg).await;
        record_secure_dns_result(
            &mut first_healthy,
            policy,
            transport,
            result,
            host_trimmed,
            dev_idx,
        );
    }

    first_healthy
        .map(|candidate| candidate.ip)
        .ok_or(DnsError::NoAnswer)
}

fn warn_secure_dns6_disagreement(
    host_trimmed: &str,
    dev_idx: usize,
    first: DnsIpv6Candidate,
    other: DnsIpv6Candidate,
) {
    if first.ip == other.ip {
        return;
    }

    crate::log!(
        "dns: warning secure ipv6 disagreement host={} dev={} first={} other={}\n",
        host_trimmed,
        dev_idx,
        first.transport.label(),
        other.transport.label()
    );
}

fn record_secure_dns6_result(
    first_healthy: &mut Option<DnsIpv6Candidate>,
    policy: SecureDnsPolicy,
    transport: SecureDnsTransport,
    result: Result<[u8; 16], DnsError>,
    host_trimmed: &str,
    dev_idx: usize,
) {
    match result {
        Ok(ip) => {
            let candidate = DnsIpv6Candidate { transport, ip };
            log_dns_transport_ip6("dns: secure resolved", transport, host_trimmed, dev_idx, ip);

            if let Some(first) = *first_healthy {
                if policy.warn_on_disagreement {
                    warn_secure_dns6_disagreement(host_trimmed, dev_idx, first, candidate);
                }
            } else {
                *first_healthy = Some(candidate);
            }
        }
        Err(DnsError::NoAnswer) => {
            crate::log!(
                "dns: secure transport no-answer transport={} host={} dev={} qtype=AAAA\n",
                transport.label(),
                host_trimmed,
                dev_idx
            );
        }
        Err(err) => {
            crate::log!(
                "dns: secure transport failed transport={} host={} dev={} qtype=AAAA err={:?}\n",
                transport.label(),
                host_trimmed,
                dev_idx,
                err
            );
        }
    }
}

async fn resolve_ipv6_secure_transport(
    transport: SecureDnsTransport,
    dev_idx: usize,
    host_trimmed: &str,
    cfg: DnsConfig,
) -> Result<[u8; 16], DnsError> {
    match transport {
        SecureDnsTransport::Doh => resolve_ipv6_doh_for_device(dev_idx, host_trimmed, cfg).await,
        SecureDnsTransport::Dot => resolve_ipv6_dot_for_device(dev_idx, host_trimmed, cfg).await,
        SecureDnsTransport::Doh3 => Err(DnsError::NoAnswer),
    }
}

async fn resolve_ipv6_secure_policy(
    dev_idx: usize,
    host_trimmed: &str,
    cfg: DnsConfig,
    policy: SecureDnsPolicy,
) -> Result<[u8; 16], DnsError> {
    if !policy.enabled {
        return Err(DnsError::NoAnswer);
    }

    let doh_enabled = policy
        .transports
        .iter()
        .flatten()
        .any(|transport| *transport == SecureDnsTransport::Doh);
    let dot_enabled = policy
        .transports
        .iter()
        .flatten()
        .any(|transport| *transport == SecureDnsTransport::Dot);

    let mut first_healthy: Option<DnsIpv6Candidate> = None;
    if doh_enabled && dot_enabled {
        let (doh_result, dot_result) = tokio::join!(
            resolve_ipv6_doh_for_device(dev_idx, host_trimmed, cfg),
            resolve_ipv6_dot_for_device(dev_idx, host_trimmed, cfg)
        );
        record_secure_dns6_result(
            &mut first_healthy,
            policy,
            SecureDnsTransport::Doh,
            doh_result,
            host_trimmed,
            dev_idx,
        );
        record_secure_dns6_result(
            &mut first_healthy,
            policy,
            SecureDnsTransport::Dot,
            dot_result,
            host_trimmed,
            dev_idx,
        );
    }

    for transport in policy.transports.iter().flatten().copied() {
        if (transport == SecureDnsTransport::Doh && doh_enabled && dot_enabled)
            || (transport == SecureDnsTransport::Dot && doh_enabled && dot_enabled)
        {
            continue;
        }
        let result = resolve_ipv6_secure_transport(transport, dev_idx, host_trimmed, cfg).await;
        record_secure_dns6_result(
            &mut first_healthy,
            policy,
            transport,
            result,
            host_trimmed,
            dev_idx,
        );
    }

    first_healthy
        .map(|candidate| candidate.ip)
        .ok_or(DnsError::NoAnswer)
}

pub async fn resolve_ipv4_for_device(
    dev_idx: usize,
    host: &str,
    cfg: DnsConfig,
) -> Result<[u8; 4], DnsError> {
    let host_trimmed = host.trim().trim_end_matches('.');
    if host_trimmed.is_empty() {
        return Err(DnsError::BadName);
    }

    {
        let now = embassy_time_driver::now();
        let mut cache = DNS_CACHE.lock();
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

    if let Ok(ip) =
        resolve_ipv4_secure_policy(dev_idx, host_trimmed, cfg, SecureDnsPolicy::default()).await
    {
        dns_cache_insert(dev_idx, host_trimmed, ip);
        dns_fs_cache_update(dev_idx, host_trimmed, ip).await;
        return Ok(ip);
    }

    if CLASSIC_DNS_DISABLED_LOGGED
        .compare_exchange(0, 1, Ordering::Relaxed, Ordering::Relaxed)
        .is_ok()
    {
        crate::log!(
            "dns: no secure dns possible host={} dev={} qtype=A; network lookup discarded\n",
            host_trimmed,
            dev_idx
        );
    }
    Err(DnsError::NoAnswer)
}

pub async fn resolve_ipv6_for_device(
    dev_idx: usize,
    host: &str,
    cfg: DnsConfig,
) -> Result<[u8; 16], DnsError> {
    let host_trimmed = host.trim().trim_end_matches('.');
    if host_trimmed.is_empty() {
        return Err(DnsError::BadName);
    }

    if let Ok(ip) =
        resolve_ipv6_secure_policy(dev_idx, host_trimmed, cfg, SecureDnsPolicy::default()).await
    {
        return Ok(ip);
    }

    if CLASSIC_DNS_DISABLED_LOGGED
        .compare_exchange(0, 1, Ordering::Relaxed, Ordering::Relaxed)
        .is_ok()
    {
        crate::log!(
            "dns: no secure dns possible host={} dev={} qtype=AAAA; network lookup discarded\n",
            host_trimmed,
            dev_idx
        );
    }
    Err(DnsError::NoAnswer)
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
