use alloc::{boxed::Box, collections::VecDeque, vec, vec::Vec};
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use embassy_executor::task;
use embassy_time::{Duration as EmbassyDuration, Timer};
use smoltcp::iface::{Config as IfaceConfig, Interface, PollResult, SocketHandle, SocketSet};
use smoltcp::phy::ChecksumCapabilities;
use smoltcp::phy::{Device, DeviceCapabilities, Medium, PacketMeta, RxToken, TxToken};
use smoltcp::socket::{dhcpv4, icmp, raw, tcp, udp};
use smoltcp::time::{Duration as SmolDuration, Instant};
use smoltcp::wire::{
    EthernetAddress, HardwareAddress, Icmpv4Packet, Icmpv4Repr, Icmpv6Packet, Icmpv6Repr,
    IpAddress, IpCidr, IpEndpoint, IpProtocol, IpVersion, Ipv4Address, Ipv6Address, Ipv6Packet,
    Ipv6Repr, NdiscPrefixInfoFlags, NdiscRepr, RawHardwareAddress,
};

// Internal netbench (kernel-side) ------------------------------------------------
//
// `bench.net` via vnet is convenient but it copies payload bytes multiple times:
// smoltcp -> Vec -> vnet ByteBuf. That can cap throughput well below what the
// NIC/driver can do.
//
// This internal runner lives inside `NetService` and counts bytes directly from
// `tcp::Socket::recv_slice` without allocating per chunk.

#[derive(Clone)]
struct InternalNetbenchRequest {
    id: u32,
    device_index: usize,
    remote: InternalNetbenchRemote,
    remote_port: u16,
    request: Vec<u8>,
}

#[derive(Clone, Copy)]
enum InternalNetbenchRemote {
    V4([u8; 4]),
    V6([u8; 16]),
}

// Internal netbench is useful for performance tuning; allow a small amount of
// concurrency to avoid per-flow caps on some networks.
const INTERNAL_NETBENCH_MAX_PENDING: usize = 8;
const INTERNAL_NETBENCH_MAX_CONCURRENT_PER_NIC: usize = 4;

static INTERNAL_NETBENCH_NEXT_ID: AtomicU32 = AtomicU32::new(1);
static INTERNAL_NETBENCH_REQS: spin::Mutex<Vec<InternalNetbenchRequest>> =
    spin::Mutex::new(Vec::new());
static NET_TX_TAP_TCP_LAST_LOG_NS: AtomicU64 = AtomicU64::new(0);
static LOGTOTCP_TX4_SAMPLE_COUNT: AtomicU32 = AtomicU32::new(0);
static LOGTOTCP_SEND_FLUSH_LAST_LOG_NS: AtomicU64 = AtomicU64::new(0);
static LOGTOTCP_SEND_FLUSH_SUPPRESSED: AtomicU64 = AtomicU64::new(0);
static LOGTOTCP_SEND_FLUSH_BYTES: AtomicU64 = AtomicU64::new(0);

fn net_log_once_per_second(last: &AtomicU64) -> bool {
    const ONE_SECOND_NS: u64 = 1_000_000_000;
    let now = crate::chronos::monotonic_nanos();
    let prev = last.load(Ordering::Relaxed);
    if prev != 0 && now.saturating_sub(prev) < ONE_SECOND_NS {
        return false;
    }
    last.compare_exchange(prev, now, Ordering::Relaxed, Ordering::Relaxed)
        .is_ok()
}

fn should_log_tx4_summary(proto: u8, buf: &[u8], l2_off: usize, ihl: usize) -> bool {
    const LOGTOTCP_TX4_SAMPLE_EVERY: u32 = 10;

    if proto != 6 || buf.len() < l2_off + ihl + 20 {
        return true;
    }

    let tcp_off = l2_off + ihl;
    let sport = u16::from_be_bytes([buf[tcp_off], buf[tcp_off + 1]]);
    if sport != crate::r::net::ports::LOGTOTCP_TCP_PORT {
        return true;
    }

    LOGTOTCP_TX4_SAMPLE_COUNT.fetch_add(1, Ordering::Relaxed) % LOGTOTCP_TX4_SAMPLE_EVERY == 0
}

fn internal_netbench_format_speed(bps: u64) -> alloc::string::String {
    use alloc::format;
    if bps < 100 {
        return format!("{} B/s", bps);
    }
    let kb = bps as f64 / 1024.0;
    if kb < 100.0 {
        return format!("{:.1} KB/s", kb);
    }
    let mb = kb / 1024.0;
    if mb < 100.0 {
        return format!("{:.1} MB/s", mb);
    }
    let gb = mb / 1024.0;
    if gb < 100.0 {
        return format!("{:.1} GB/s", gb);
    }
    let tb = gb / 1024.0;
    format!("{:.1} TB/s", tb)
}

/// Submit a one-shot internal netbench run.
///
/// Returns `false` if the internal netbench queue is full.
pub fn internal_netbench_submit(
    device_index: usize,
    remote_ip: [u8; 4],
    remote_port: u16,
    request: &[u8],
) -> bool {
    let mut guard = INTERNAL_NETBENCH_REQS.lock();
    if guard.len() >= INTERNAL_NETBENCH_MAX_PENDING {
        return false;
    }
    guard.push(InternalNetbenchRequest {
        id: INTERNAL_NETBENCH_NEXT_ID.fetch_add(1, Ordering::Relaxed),
        device_index,
        remote: InternalNetbenchRemote::V4(remote_ip),
        remote_port,
        request: request.to_vec(),
    });
    true
}

pub fn internal_netbench_submit_v6(
    device_index: usize,
    remote_ip: [u8; 16],
    remote_port: u16,
    request: &[u8],
) -> bool {
    let mut guard = INTERNAL_NETBENCH_REQS.lock();
    if guard.len() >= INTERNAL_NETBENCH_MAX_PENDING {
        return false;
    }
    guard.push(InternalNetbenchRequest {
        id: INTERNAL_NETBENCH_NEXT_ID.fetch_add(1, Ordering::Relaxed),
        device_index,
        remote: InternalNetbenchRemote::V6(remote_ip),
        remote_port,
        request: request.to_vec(),
    });
    true
}

struct InternalNetbenchState {
    id: u32,
    socket: SocketHandle,
    request: Vec<u8>,
    request_sent: bool,

    header: [u8; 16 * 1024],
    header_len: usize,
    header_done: bool,
    expected_len: Option<usize>,

    received: u64,
    start_tick: u64,
    last_log_tick: u64,
    last_log_received: u64,

    last_tcp_state: Option<tcp::State>,
}

struct InternalNetbenchCombinedLog {
    start_tick: u64,
    last_log_tick: u64,
    last_log_received: u64,
}

// Boot can involve several concurrent TCP/TLS connections (DoH/DoT, fetches,
// net-shell, etc.). 8 was too tight and caused transient "no sockets available"
// failures under load.
const MAX_SOCKETS: usize = crate::allcaps::net::MAX_SOCKETS;
const MAX_DRAIN_PER_LOOP: usize = crate::allcaps::net::MAX_DRAIN_PER_LOOP;
const TCP_RX_BUF_BYTES: usize = crate::allcaps::net::TCP_RX_BUF_BYTES;
const TCP_TX_BUF_BYTES: usize = crate::allcaps::net::TCP_TX_BUF_BYTES;
const ICMP_IDENT: u16 = 0x1234;
const ICMP_VNET_MAX_INFLIGHT: usize = crate::allcaps::net::ICMP_VNET_MAX_INFLIGHT;
const ICMP_VNET_TIMEOUT_MS: i64 = crate::allcaps::net::ICMP_VNET_TIMEOUT_MS;
const NET_POLL_SLEEP_US: u64 = crate::allcaps::net::NET_POLL_SLEEP_US;
const NET_SERVICE_SLEEP_US: u64 = crate::allcaps::net::NET_SERVICE_SLEEP_US;
// DHCPv6 bring-up is easy to misdiagnose because failures often look like
// "nothing happens". Keep a tiny amount of always-on logging on state changes
// and a small RX sample to make it obvious whether we transmit/receive.
// Some networks provide DHCPv6 DNS without setting RA O=1, and some RAs omit
// RDNSS. When enabled, we will send an Information-Request if we don't have
// any IPv6 DNS yet (from either RA or DHCPv6).
const DHCP6_EAGER_DNS: bool = true;

const DHCP_DNS_MAX: usize = crate::allcaps::net::DNS_SERVER_MAX;
const RA_DNS6_MAX: usize = crate::allcaps::net::DNS_SERVER_MAX;
const DHCP6_DNS6_MAX: usize = crate::allcaps::net::DNS_SERVER_MAX;
pub const MAX_NET_DEVICES: usize = crate::allcaps::net::MAX_NET_DEVICES;
const STATIC_FALLBACK_PREFIX_LEN: u8 = 24;
const STATIC_FALLBACK_BASE_IPV4: [u8; 4] = [192, 168, 178, 111];
const STATIC_FALLBACK_GATEWAY: [u8; 4] = [192, 168, 178, 1];

const IPV6_LINK_LOCAL_PREFIX: [u8; 8] = [0xfe, 0x80, 0, 0, 0, 0, 0, 0];
const IPV6_LINK_LOCAL_PREFIX_LEN: u8 = 64;
const IPV6_RS_RETRY_MS: i64 = crate::allcaps::net::IPV6_RS_RETRY_MS;

// Best-effort primary DNS server snapshot (used by v-layer defaults).
// Kept intentionally simple: we record what DHCP reports for the active primary NIC.
static PRIMARY_DHCP_DNS: spin::Mutex<([[u8; 4]; DHCP_DNS_MAX], u8)> =
    spin::Mutex::new(([[0u8; 4]; DHCP_DNS_MAX], 0));

// Best-effort primary IPv6 DNS server snapshot from Router Advertisements (RDNSS).
// Used by v-layer defaults to make DNS work on IPv6-only networks.
static PRIMARY_RA_DNS6: spin::Mutex<([[u8; 16]; RA_DNS6_MAX], u8)> =
    spin::Mutex::new(([[0u8; 16]; RA_DNS6_MAX], 0));

// Best-effort primary IPv6 DNS server snapshot from DHCPv6.
static PRIMARY_DHCP6_DNS6: spin::Mutex<([[u8; 16]; DHCP6_DNS6_MAX], u8)> =
    spin::Mutex::new(([[0u8; 16]; DHCP6_DNS6_MAX], 0));

pub fn primary_dhcp_dns_snapshot() -> ([[u8; 4]; DHCP_DNS_MAX], u8) {
    *PRIMARY_DHCP_DNS.lock()
}

pub fn primary_ra_dns6_snapshot() -> ([[u8; 16]; RA_DNS6_MAX], u8) {
    *PRIMARY_RA_DNS6.lock()
}

pub fn primary_dhcp6_dns6_snapshot() -> ([[u8; 16]; DHCP6_DNS6_MAX], u8) {
    *PRIMARY_DHCP6_DNS6.lock()
}

pub fn dhcp_dns_snapshot_at(index: usize) -> Option<([[u8; 4]; DHCP_DNS_MAX], u8)> {
    let guard = NET_SERVICES.lock();
    let services = guard.as_ref()?;
    let svc = services.get(index)?.lock();
    Some((svc.dhcp_dns, svc.dhcp_dns_count))
}

pub fn ipv4_router_snapshot_at(index: usize) -> Option<Option<[u8; 4]>> {
    let guard = NET_SERVICES.lock();
    let services = guard.as_ref()?;
    let svc = services.get(index)?.lock();
    Some(svc.router_ipv4.map(|router| router.octets()))
}

pub fn primary_ipv4_router_snapshot() -> Option<[u8; 4]> {
    ipv4_router_snapshot_at(crate::net::primary_device_index()).flatten()
}

pub fn ra_dns6_snapshot_at(index: usize) -> Option<([[u8; 16]; RA_DNS6_MAX], u8)> {
    let guard = NET_SERVICES.lock();
    let services = guard.as_ref()?;
    let svc = services.get(index)?.lock();
    Some((svc.ra_dns6, svc.ra_dns6_count))
}

pub fn dhcp6_dns6_snapshot_at(index: usize) -> Option<([[u8; 16]; DHCP6_DNS6_MAX], u8)> {
    let guard = NET_SERVICES.lock();
    let services = guard.as_ref()?;
    let svc = services.get(index)?.lock();
    Some((svc.dhcp6_dns6, svc.dhcp6_dns6_count))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dhcp6Stage {
    Idle,
    Solicit,
    Request,
    Info,
    Bound,
}

pub fn ipv4_at(index: usize) -> Option<[u8; 4]> {
    if !crate::net::link_state_at(index)
        .map(|ls| ls.up)
        .unwrap_or(false)
    {
        return None;
    }

    let guard = NET_SERVICES.lock();
    let services = guard.as_ref()?;
    services
        .get(index)
        .and_then(|s| s.lock().local_ipv4.map(|ip| ip.octets()))
}

pub fn ipv6_link_local_at(index: usize) -> Option<[u8; 16]> {
    let guard = NET_SERVICES.lock();
    let services = guard.as_ref()?;
    services.get(index).map(|s| s.lock().local_ipv6_ll.octets())
}

pub fn ipv6_global_at(index: usize) -> Option<[u8; 16]> {
    let guard = NET_SERVICES.lock();
    let services = guard.as_ref()?;
    services
        .get(index)
        .and_then(|s| s.lock().local_ipv6_global.map(|ip| ip.octets()))
}

pub fn dhcp_has_lease_at(index: usize) -> Option<bool> {
    let guard = NET_SERVICES.lock();
    let services = guard.as_ref()?;
    let svc = services.get(index)?.lock();
    Some(svc.dhcp_has_lease)
}

static HOSTNAME: spin::Mutex<Option<alloc::string::String>> = spin::Mutex::new(None);

pub fn get_hostname() -> alloc::string::String {
    HOSTNAME
        .lock()
        .clone()
        .unwrap_or_else(|| alloc::string::String::from("TRUEOS"))
}

pub fn set_hostname(name: &str) {
    *HOSTNAME.lock() = Some(alloc::string::String::from(name));
}

static NET_RX_FRAMES: AtomicU64 = AtomicU64::new(0);
static NET_TX_FRAMES: AtomicU64 = AtomicU64::new(0);
static NET_TX_DROPPED: AtomicU64 = AtomicU64::new(0);

static NET_RX_FRAMES_AT: [AtomicU64; MAX_NET_DEVICES] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];
static NET_TX_FRAMES_AT: [AtomicU64; MAX_NET_DEVICES] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];
static NET_DHCP_OFFERS_RX_AT: [AtomicU64; MAX_NET_DEVICES] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];

static NET_ARP_RX_AT: [AtomicU64; MAX_NET_DEVICES] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];

// DHCP bring-up interoperability toggles.
//
// Some DHCP servers/routers only reply properly when the client requests
// broadcast replies (BOOTP flags bit15). For bring-up, we force this bit.
const DHCP_FORCE_BROADCAST_FLAG: bool = true;
// As a compatibility experiment, force UDP checksum to 0 (IPv4 allows this).
// Useful if we're accidentally producing an invalid checksum due to offload or
// descriptor behavior.
const DHCP_FORCE_UDP_CHECKSUM_ZERO: bool = true;

fn dhcp_fixup_broadcast_and_udp_checksum(buf: &mut [u8]) {
    if buf.len() < 14 {
        return;
    }

    let mut et = u16::from_be_bytes([buf[12], buf[13]]);
    let mut l2_off = 14usize;
    if et == 0x8100 {
        if buf.len() < 18 {
            return;
        }
        et = u16::from_be_bytes([buf[16], buf[17]]);
        l2_off = 18;
    }
    if et != 0x0800 || buf.len() < l2_off + 20 {
        return;
    }

    let ver_ihl = buf[l2_off];
    if (ver_ihl >> 4) != 4 {
        return;
    }
    let ihl = ((ver_ihl & 0x0f) as usize) * 4;
    if ihl < 20 || buf.len() < l2_off + ihl + 8 {
        return;
    }
    if buf[l2_off + 9] != 17 {
        return;
    }

    let udp_off = l2_off + ihl;
    let sport = u16::from_be_bytes([buf[udp_off], buf[udp_off + 1]]);
    let dport = u16::from_be_bytes([buf[udp_off + 2], buf[udp_off + 3]]);
    if !(sport == 68 && dport == 67) {
        return;
    }

    let udp_len = u16::from_be_bytes([buf[udp_off + 4], buf[udp_off + 5]]) as usize;
    let dhcp_off = udp_off + 8;
    if udp_len < 8 + 240 || buf.len() < udp_off + udp_len {
        return;
    }

    let flags_off = dhcp_off + 10;
    let old_flags = u16::from_be_bytes([buf[flags_off], buf[flags_off + 1]]);
    let mut mutated = false;
    if DHCP_FORCE_BROADCAST_FLAG && (old_flags & 0x8000) == 0 {
        buf[flags_off] = 0x80;
        buf[flags_off + 1] = 0x00;
        mutated = true;
    }

    // If we changed DHCP payload bytes, recompute UDP checksum (unless forcing it to 0).
    if mutated && !DHCP_FORCE_UDP_CHECKSUM_ZERO {
        let ip_src = [
            buf[l2_off + 12],
            buf[l2_off + 13],
            buf[l2_off + 14],
            buf[l2_off + 15],
        ];
        let ip_dst = [
            buf[l2_off + 16],
            buf[l2_off + 17],
            buf[l2_off + 18],
            buf[l2_off + 19],
        ];

        // Zero checksum field while computing.
        buf[udp_off + 6] = 0;
        buf[udp_off + 7] = 0;

        let mut sum: u32 = 0;
        let add16 = |sum: &mut u32, w: u16| {
            *sum = sum.wrapping_add(w as u32);
        };

        add16(&mut sum, u16::from_be_bytes([ip_src[0], ip_src[1]]));
        add16(&mut sum, u16::from_be_bytes([ip_src[2], ip_src[3]]));
        add16(&mut sum, u16::from_be_bytes([ip_dst[0], ip_dst[1]]));
        add16(&mut sum, u16::from_be_bytes([ip_dst[2], ip_dst[3]]));
        add16(&mut sum, 0x0011); // protocol UDP
        add16(&mut sum, udp_len as u16);

        let udp_bytes = &buf[udp_off..udp_off + udp_len];
        let mut i = 0usize;
        while i + 1 < udp_bytes.len() {
            add16(&mut sum, u16::from_be_bytes([udp_bytes[i], udp_bytes[i + 1]]));
            i += 2;
        }
        if (udp_bytes.len() & 1) != 0 {
            add16(&mut sum, u16::from_be_bytes([udp_bytes[udp_bytes.len() - 1], 0]));
        }

        while (sum >> 16) != 0 {
            sum = (sum & 0xFFFF) + (sum >> 16);
        }
        let mut udp_csum = !(sum as u16);
        if udp_csum == 0 {
            udp_csum = 0xFFFF;
        }
        let c = udp_csum.to_be_bytes();
        buf[udp_off + 6] = c[0];
        buf[udp_off + 7] = c[1];
    }

    if DHCP_FORCE_UDP_CHECKSUM_ZERO {
        buf[udp_off + 6] = 0;
        buf[udp_off + 7] = 0;
    }
}

// Console logging these counters too frequently can flood stdout and make it
// look like the system is "stuck". Keep the default cadence very low.
// (Mask form: log when `count & MASK == 0`.)
#[cfg(debug_assertions)]
const NET_FRAME_LOG_MASK: u64 = 0x0FFF; // 4096 frames

#[cfg(not(debug_assertions))]
const NET_FRAME_LOG_MASK: u64 = 0xFFFF; // 65536 frames

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NetHandle(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetEndpoint {
    pub addr: [u8; 4],
    pub port: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetEndpointV6 {
    pub addr: [u8; 16],
    pub port: u16,
}

#[derive(Clone, Debug)]
pub enum NetCommand {
    OpenUdp {
        port: u16,
    },
    OpenTcpListen {
        port: u16,
    },
    OpenTcpConnect {
        remote: NetEndpoint,
    },
    OpenTcpConnectV6 {
        remote: NetEndpointV6,
    },
    SendUdp {
        handle: NetHandle,
        remote: NetEndpoint,
        data: Vec<u8>,
    },
    SendUdpV6 {
        handle: NetHandle,
        remote: NetEndpointV6,
        data: Vec<u8>,
    },
    SendTcp {
        handle: NetHandle,
        data: Vec<u8>,
    },
    IcmpEcho {
        target: [u8; 4],
        seq: u16,
        data: Vec<u8>,
    },
    IcmpEchoV6 {
        target: [u8; 16],
        seq: u16,
        data: Vec<u8>,
    },
    Close {
        handle: NetHandle,
    },
}

#[derive(Clone, Debug)]
pub enum NetEvent {
    Opened {
        handle: NetHandle,
        kind: SocketKind,
    },
    Closed {
        handle: NetHandle,
    },
    Error {
        msg: &'static str,
    },
    UdpPacket {
        handle: NetHandle,
        from: NetEndpoint,
        data: Vec<u8>,
    },
    UdpPacketV6 {
        handle: NetHandle,
        from: NetEndpointV6,
        data: Vec<u8>,
    },
    TcpEstablished {
        handle: NetHandle,
        peer: Option<NetEndpoint>,
        peer6: Option<NetEndpointV6>,
    },
    TcpData {
        handle: NetHandle,
        data: Vec<u8>,
    },
    TcpSent {
        handle: NetHandle,
        len: usize,
    },
    IcmpReply {
        from: [u8; 4],
        seq: u16,
        rtt_ms: u32,
        data: Vec<u8>,
    },
    IcmpReplyV6 {
        from: [u8; 16],
        seq: u16,
        rtt_ms: u32,
        data: Vec<u8>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocketKind {
    Udp,
    Tcp,
}

pub struct NetQueue<T> {
    _name: &'static str,
    capacity: usize,
    inner: spin::Mutex<VecDeque<T>>,
    dropped: AtomicU32,
}

impl<T> NetQueue<T> {
    pub fn new_leaked(name: &'static str, capacity: usize) -> &'static Self {
        let q = Self {
            _name: name,
            capacity: capacity.max(1),
            inner: spin::Mutex::new(VecDeque::with_capacity(capacity)),
            dropped: AtomicU32::new(0),
        };
        Box::leak(Box::new(q))
    }

    pub fn push(&self, item: T) -> Result<(), ()> {
        let mut guard = self.inner.lock();
        if guard.len() >= self.capacity {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return Err(());
        }
        guard.push_back(item);
        Ok(())
    }

    #[inline]
    pub fn pop(&self) -> Option<T> {
        self.inner.lock().pop_front()
    }

    pub fn drain(&self, max: usize) -> Vec<T> {
        let mut guard = self.inner.lock();
        let mut out = Vec::with_capacity(max.min(guard.len()));
        for _ in 0..max {
            if let Some(item) = guard.pop_front() {
                out.push(item);
            } else {
                break;
            }
        }
        out
    }
}

struct AppQueues {
    name: &'static str,
    cmds: &'static NetQueue<NetCommand>,
    events: &'static NetQueue<NetEvent>,
}

static APP_QUEUES: spin::Mutex<Vec<AppQueues>> = spin::Mutex::new(Vec::new());

pub fn register_app_queues(
    name: &'static str,
    cmds: &'static NetQueue<NetCommand>,
    events: &'static NetQueue<NetEvent>,
) {
    let mut guard = APP_QUEUES.lock();
    if guard.iter().any(|entry| entry.name == name) {
        return;
    }
    guard.push(AppQueues { name, cmds, events });
}

fn pop_command_for_device(device_index: usize) -> Option<(&'static str, NetCommand)> {
    let guard = APP_QUEUES.lock();
    for entry in guard.iter() {
        let idx = owner_device_index(entry.name).unwrap_or_else(crate::net::primary_device_index);
        if idx != device_index {
            continue;
        }
        if let Some(cmd) = entry.cmds.pop() {
            return Some((entry.name, cmd));
        }
    }
    None
}

fn push_event(target: &'static str, event: NetEvent) -> bool {
    static DROP_COUNT: AtomicU64 = AtomicU64::new(0);

    let wakes_mio = matches!(
        event,
        NetEvent::TcpData { .. }
            | NetEvent::TcpEstablished { .. }
            | NetEvent::Closed { .. }
            | NetEvent::UdpPacket { .. }
            | NetEvent::UdpPacketV6 { .. }
    );

    let guard = APP_QUEUES.lock();
    if let Some(entry) = guard.iter().find(|e| e.name == target) {
        let ok = entry.events.push(event).is_ok();
        if ok && wakes_mio {
            crate::mio_compat::notify_net_event();
        }
        if !ok && wakes_mio {
            let n = DROP_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
            if n <= 2 || n.is_power_of_two() {
                crate::log_info!(target: "net"; "net: event drop owner={} (mio-signal) count={}\n", target, n);
            }
        }
        ok
    } else {
        false
    }
}

struct AdapterDeviceAt<'a> {
    index: usize,
    rx_buffer: &'a mut VecDeque<Vec<u8>>,
    tx_buffer: &'a mut VecDeque<Vec<u8>>,
}

impl<'a> Device for AdapterDeviceAt<'a> {
    type RxToken<'b>
        = AdapterRxToken
    where
        Self: 'b;
    type TxToken<'b>
        = AdapterTxTokenAt<'b>
    where
        Self: 'b;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let packet = if let Some(p) = self.rx_buffer.pop_front() {
            p
        } else {
            let drained = crate::net::drain_rx_packets_each_at(self.index, 128, &mut |packet| {
                self.rx_buffer.push_back(packet);
            });
            if drained == 0 {
                return None;
            }
            self.rx_buffer.pop_front()?
        };

        Some(packet).map(|packet| {
            let new_total = NET_RX_FRAMES.fetch_add(1, Ordering::Relaxed) + 1;
            let new_dev_total = NET_RX_FRAMES_AT
                .get(self.index)
                .map(|c| c.fetch_add(1, Ordering::Relaxed) + 1)
                .unwrap_or(new_total);
            if (new_total & NET_FRAME_LOG_MASK) == 0 {
                crate::log_info!(target: "net"; "net: rx frames={}\n", new_total);
            }

            // DHCP offer/ack detector (UDP 67 -> 68). Rate-limited so we can leave
            // it enabled while debugging without flooding logs.
            //
            // Important: replies are often broadcast, so multiple NICs on the same
            // LAN will all see the same Offer/Ack. To avoid confusing logs, we
            // only log packets whose BOOTP `chaddr` matches this NIC (and log a
            // single mismatch sample otherwise).
            if packet.len() >= 14 {
                let mut et = u16::from_be_bytes([packet[12], packet[13]]);
                let mut l2_off = 14usize;
                let mut vlan_tci: Option<u16> = None;
                if et == 0x8100 && packet.len() >= 18 {
                    vlan_tci = Some(u16::from_be_bytes([packet[14], packet[15]]));
                    et = u16::from_be_bytes([packet[16], packet[17]]);
                    l2_off = 18;
                }

                // ARP tap: log a few ARP frames so we can confirm whether we ever
                // receive a reply for the gateway (required for IPv4 egress).
                if et == 0x0806 && packet.len() >= l2_off + 28 {
                    let arp_count = NET_ARP_RX_AT
                        .get(self.index)
                        .map(|c| c.fetch_add(1, Ordering::Relaxed) + 1)
                        .unwrap_or(1);
                    let opcode =
                        u16::from_be_bytes([packet[l2_off + 6], packet[l2_off + 7]]);
                    // Always log replies; sample requests.
                    if crate::logflag::NET_LOG_ARP_RX && (opcode == 2 || arp_count <= 32) {
                        let sha = &packet[l2_off + 8..l2_off + 14];
                        let spa = [
                            packet[l2_off + 14],
                            packet[l2_off + 15],
                            packet[l2_off + 16],
                            packet[l2_off + 17],
                        ];
                        let tha = &packet[l2_off + 18..l2_off + 24];
                        let tpa = [
                            packet[l2_off + 24],
                            packet[l2_off + 25],
                            packet[l2_off + 26],
                            packet[l2_off + 27],
                        ];
                        if let Some(tci) = vlan_tci {
                            crate::log_info!(target: "net"; 
                                "net: arp-rx dev={} vlan=0x{:04x} n={} op={} sha={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} spa={}.{}.{}.{} tha={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} tpa={}.{}.{}.{}\n",
                                self.index,
                                tci,
                                arp_count,
                                opcode,
                                sha[0],
                                sha[1],
                                sha[2],
                                sha[3],
                                sha[4],
                                sha[5],
                                spa[0],
                                spa[1],
                                spa[2],
                                spa[3],
                                tha[0],
                                tha[1],
                                tha[2],
                                tha[3],
                                tha[4],
                                tha[5],
                                tpa[0],
                                tpa[1],
                                tpa[2],
                                tpa[3]
                            );
                        } else {
                            crate::log_info!(target: "net"; 
                                "net: arp-rx dev={} n={} op={} sha={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} spa={}.{}.{}.{} tha={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} tpa={}.{}.{}.{}\n",
                                self.index,
                                arp_count,
                                opcode,
                                sha[0],
                                sha[1],
                                sha[2],
                                sha[3],
                                sha[4],
                                sha[5],
                                spa[0],
                                spa[1],
                                spa[2],
                                spa[3],
                                tha[0],
                                tha[1],
                                tha[2],
                                tha[3],
                                tha[4],
                                tha[5],
                                tpa[0],
                                tpa[1],
                                tpa[2],
                                tpa[3]
                            );
                        }
                    }
                }

                if et == 0x0800 && packet.len() >= l2_off + 20 {
                    let ver_ihl = packet[l2_off];
                    let ihl = ((ver_ihl & 0x0f) as usize) * 4;
                    if (ver_ihl >> 4) == 4 && packet.len() >= l2_off + ihl + 8 {
                        let proto = packet[l2_off + 9];
                        if proto == 17 {
                            let udp_off = l2_off + ihl;
                            let sport = u16::from_be_bytes([packet[udp_off], packet[udp_off + 1]]);
                            let dport = u16::from_be_bytes([packet[udp_off + 2], packet[udp_off + 3]]);
                            if sport == 67 && dport == 68 {
                                let offer_count = NET_DHCP_OFFERS_RX_AT
                                    .get(self.index)
                                    .map(|c| c.fetch_add(1, Ordering::Relaxed) + 1)
                                    .unwrap_or(1);

                                if offer_count <= 4 || (offer_count & 0x3F) == 0 {
                                    let self_mac = crate::net::mac_address_at(self.index);
                                    let ip_src = [
                                        packet[l2_off + 12],
                                        packet[l2_off + 13],
                                        packet[l2_off + 14],
                                        packet[l2_off + 15],
                                    ];
                                    let ip_dst = [
                                        packet[l2_off + 16],
                                        packet[l2_off + 17],
                                        packet[l2_off + 18],
                                        packet[l2_off + 19],
                                    ];

                                    // Minimal BOOTP/DHCP parse. Payload begins after UDP header.
                                    let udp_len = u16::from_be_bytes([
                                        packet[udp_off + 4],
                                        packet[udp_off + 5],
                                    ]) as usize;
                                    let dhcp_off = udp_off + 8;

                                    let mut op: u8 = 0;
                                    let mut xid: u32 = 0;
                                    let mut yiaddr = [0u8; 4];
                                    let mut cookie_ok: u8 = 0;
                                    let mut msg_type: u8 = 0;
                                    let mut chaddr = [0u8; 6];
                                    let mut chaddr_match: u8 = 0;
                                    if udp_len >= 8 + 240 && packet.len() >= udp_off + udp_len {
                                        op = packet[dhcp_off];
                                        let hlen = packet[dhcp_off + 2];
                                        xid = u32::from_be_bytes([
                                            packet[dhcp_off + 4],
                                            packet[dhcp_off + 5],
                                            packet[dhcp_off + 6],
                                            packet[dhcp_off + 7],
                                        ]);
                                        yiaddr = [
                                            packet[dhcp_off + 16],
                                            packet[dhcp_off + 17],
                                            packet[dhcp_off + 18],
                                            packet[dhcp_off + 19],
                                        ];

                                        if hlen == 6 {
                                            chaddr.copy_from_slice(
                                                &packet[dhcp_off + 28..dhcp_off + 34],
                                            );
                                            if let Some(m) = self_mac {
                                                chaddr_match = (m == chaddr) as u8;
                                            }
                                        }

                                        cookie_ok = (packet[dhcp_off + 236..dhcp_off + 240]
                                            == [99, 130, 83, 99]) as u8;

                                        // Options: scan for message type (53)
                                        let mut opt_i = dhcp_off + 240;
                                        let end = udp_off + udp_len;
                                        while opt_i < end {
                                            let code = packet[opt_i];
                                            opt_i += 1;
                                            if code == 0 {
                                                continue;
                                            }
                                            if code == 255 {
                                                break;
                                            }
                                            if opt_i >= end {
                                                break;
                                            }
                                            let olen = packet[opt_i] as usize;
                                            opt_i += 1;
                                            if opt_i + olen > end {
                                                break;
                                            }
                                            if code == 53 && olen >= 1 {
                                                msg_type = packet[opt_i];
                                                break;
                                            }
                                            opt_i += olen;
                                        }
                                    }
                                    if crate::logflag::NET_LOG_DHCP_VERBOSE
                                        && (chaddr_match != 0 || offer_count == 1)
                                    {
                                        crate::log_info!(target: "net"; 
                                            "net: dhcp-offer-rx dev={} count={} ip_src={}.{}.{}.{} ip_dst={}.{}.{}.{} len={} op={} xid=0x{:08x} yiaddr={}.{}.{}.{} chaddr={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} chaddr_match={} cookie_ok={} msg_type={}\n",
                                            self.index,
                                            offer_count,
                                            ip_src[0],
                                            ip_src[1],
                                            ip_src[2],
                                            ip_src[3],
                                            ip_dst[0],
                                            ip_dst[1],
                                            ip_dst[2],
                                            ip_dst[3],
                                            packet.len(),
                                            op,
                                            xid,
                                            yiaddr[0],
                                            yiaddr[1],
                                            yiaddr[2],
                                            yiaddr[3],
                                            chaddr[0],
                                            chaddr[1],
                                            chaddr[2],
                                            chaddr[3],
                                            chaddr[4],
                                            chaddr[5],
                                            chaddr_match,
                                            cookie_ok,
                                            msg_type
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Cheap RX tap for bring-up: log the first few frames per NIC.
            if crate::logflag::NET_LOG_RX_TAP && new_dev_total <= 8 {
                if packet.len() >= 14 {
                    let dst = &packet[0..6];
                    let src = &packet[6..12];
                    let mut et = u16::from_be_bytes([packet[12], packet[13]]);
                    if et == 0x8100 && packet.len() >= 18 {
                        et = u16::from_be_bytes([packet[16], packet[17]]);
                    }

                    let self_mac = crate::net::mac_address_at(self.index);
                    let src_is_self = self_mac
                        .map(|m| m == [src[0], src[1], src[2], src[3], src[4], src[5]])
                        .unwrap_or(false);

                    crate::log_info!(target: "net"; 
                        "net: rx-tap dev={} len={} et=0x{:04x} dst={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} src={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} self_src={}\n",
                        self.index,
                        packet.len(),
                        et,
                        dst[0],
                        dst[1],
                        dst[2],
                        dst[3],
                        dst[4],
                        dst[5],
                        src[0],
                        src[1],
                        src[2],
                        src[3],
                        src[4],
                        src[5],
                        src_is_self as u8
                    );
                } else {
                    crate::log_info!(target: "net"; "net: rx-tap dev={} len={} (short)\n", self.index, packet.len());
                }
            }
            (
                AdapterRxToken { buffer: packet },
                AdapterTxTokenAt { index: self.index, tx_buffer: self.tx_buffer },
            )
        })
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        Some(AdapterTxTokenAt {
            index: self.index,
            tx_buffer: self.tx_buffer,
        })
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = 1500;
        // Let smoltcp decide burst handling without an artificial small cap.
        caps.max_burst_size = None;
        caps.medium = Medium::Ethernet;
        caps
    }
}

struct AdapterRxToken {
    buffer: Vec<u8>,
}

impl Drop for AdapterRxToken {
    fn drop(&mut self) {
        let buf = core::mem::take(&mut self.buffer);
        if !buf.is_empty() {
            crate::net::ring::recycle_rx_buf(buf);
        }
    }
}

impl RxToken for AdapterRxToken {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        f(&self.buffer[..])
    }

    fn meta(&self) -> PacketMeta {
        PacketMeta::default()
    }
}

struct AdapterTxTokenAt<'a> {
    index: usize,
    tx_buffer: &'a mut VecDeque<Vec<u8>>,
}

impl<'a> TxToken for AdapterTxTokenAt<'a> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        // Avoid per-packet heap allocation: reuse the shared packet pool.
        // Safety: `f` is expected to fully initialize the provided buffer.
        let mut buf = crate::net::ring::alloc_packet_buf(len);
        let result = f(&mut buf[..]);

        // DHCP fixup must run for every outgoing DHCP client packet, not only
        // the initial tx-tap frames.
        dhcp_fixup_broadcast_and_udp_checksum(&mut buf[..]);

        let new_total = NET_TX_FRAMES.fetch_add(1, Ordering::Relaxed) + 1;
        let new_dev_total = NET_TX_FRAMES_AT
            .get(self.index)
            .map(|c| c.fetch_add(1, Ordering::Relaxed) + 1)
            .unwrap_or(new_total);

        // TX tap: only log IPv4/TCP frames for bring-up. This avoids drowning in
        // NDP/ARP chatter while still letting us confirm SYN emission.
        if (crate::logflag::NET_LOG_TX_TAP || crate::logflag::NET_LOG_TCP_CONNECT_WIRE)
            && new_dev_total <= 8192
        {
            if buf.len() >= 14 {
                let mut et = u16::from_be_bytes([buf[12], buf[13]]);
                let mut l2_off = 14usize;
                if et == 0x8100 && buf.len() >= 18 {
                    et = u16::from_be_bytes([buf[16], buf[17]]);
                    l2_off = 18;
                }

                // Minimal ARP tap (helps diagnose missing gateway MAC resolution).
                if et == 0x0806 && buf.len() >= l2_off + 28 {
                    let opcode = u16::from_be_bytes([buf[l2_off + 6], buf[l2_off + 7]]);
                    let spa = [
                        buf[l2_off + 14],
                        buf[l2_off + 15],
                        buf[l2_off + 16],
                        buf[l2_off + 17],
                    ];
                    let tpa = [
                        buf[l2_off + 24],
                        buf[l2_off + 25],
                        buf[l2_off + 26],
                        buf[l2_off + 27],
                    ];
                    crate::log_trace!(target: "net";
                        "net: tx-tap dev={} arp op={} spa={}.{}.{}.{} tpa={}.{}.{}.{}\n",
                        self.index,
                        opcode,
                        spa[0],
                        spa[1],
                        spa[2],
                        spa[3],
                        tpa[0],
                        tpa[1],
                        tpa[2],
                        tpa[3]
                    );
                }

                if et == 0x0800 && buf.len() >= l2_off + 20 {
                    let ver_ihl = buf[l2_off];
                    if (ver_ihl >> 4) == 4 {
                        let ihl = ((ver_ihl & 0x0f) as usize) * 4;
                        if ihl >= 20 && buf.len() >= l2_off + ihl {
                            let proto = buf[l2_off + 9];

                            // Lightweight IPv4 TX summary (rate-limited): lets us see
                            // the actual source IP used after DHCP reconfiguration.
                            if new_dev_total <= 256
                                && should_log_tx4_summary(proto, &buf, l2_off, ihl)
                            {
                                let src_ip = [
                                    buf[l2_off + 12],
                                    buf[l2_off + 13],
                                    buf[l2_off + 14],
                                    buf[l2_off + 15],
                                ];
                                let dst_ip = [
                                    buf[l2_off + 16],
                                    buf[l2_off + 17],
                                    buf[l2_off + 18],
                                    buf[l2_off + 19],
                                ];
                                crate::log_trace!(target: "net";
                                    "net: tx4 dev={} {}.{}.{}.{} -> {}.{}.{}.{} proto={} len={}\n",
                                    self.index,
                                    src_ip[0],
                                    src_ip[1],
                                    src_ip[2],
                                    src_ip[3],
                                    dst_ip[0],
                                    dst_ip[1],
                                    dst_ip[2],
                                    dst_ip[3],
                                    proto,
                                    buf.len()
                                );
                            }

                            if proto == 1 {
                                // ICMP header is 8 bytes; don't require TCP-sized payload.
                                if buf.len() >= l2_off + ihl + 8 {
                                    let src_ip = [
                                        buf[l2_off + 12],
                                        buf[l2_off + 13],
                                        buf[l2_off + 14],
                                        buf[l2_off + 15],
                                    ];
                                    let dst_ip = [
                                        buf[l2_off + 16],
                                        buf[l2_off + 17],
                                        buf[l2_off + 18],
                                        buf[l2_off + 19],
                                    ];
                                    let icmp_off = l2_off + ihl;
                                    let icmp_type = buf.get(icmp_off).copied().unwrap_or(0);
                                    let icmp_code = buf.get(icmp_off + 1).copied().unwrap_or(0);
                                    crate::log_trace!(target: "net";
                                        "net: tx-tap dev={} icmp4 {}.{}.{}.{} -> {}.{}.{}.{} type={} code={}\n",
                                        self.index,
                                        src_ip[0],
                                        src_ip[1],
                                        src_ip[2],
                                        src_ip[3],
                                        dst_ip[0],
                                        dst_ip[1],
                                        dst_ip[2],
                                        dst_ip[3],
                                        icmp_type,
                                        icmp_code
                                    );
                                }
                            }
                            if proto == 6 {
                                if buf.len() >= l2_off + ihl + 20 {
                                    let src_ip = [
                                        buf[l2_off + 12],
                                        buf[l2_off + 13],
                                        buf[l2_off + 14],
                                        buf[l2_off + 15],
                                    ];
                                    let dst_ip = [
                                        buf[l2_off + 16],
                                        buf[l2_off + 17],
                                        buf[l2_off + 18],
                                        buf[l2_off + 19],
                                    ];
                                    let tcp_off = l2_off + ihl;
                                    let sport =
                                        u16::from_be_bytes([buf[tcp_off], buf[tcp_off + 1]]);
                                    let dport =
                                        u16::from_be_bytes([buf[tcp_off + 2], buf[tcp_off + 3]]);
                                    let flags = buf[tcp_off + 13];
                                    let control = (flags & 0x07) != 0;
                                    let sampled_data = crate::logflag::NET_LOG_TX_TAP
                                        && net_log_once_per_second(&NET_TX_TAP_TCP_LAST_LOG_NS);
                                    if control || sampled_data {
                                        crate::log_trace!(target: "net";
                                            "net: tx-tap dev={} tcp {}.{}.{}.{}:{} -> {}.{}.{}.{}:{} flags=0x{:02x}\n",
                                            self.index,
                                            src_ip[0],
                                            src_ip[1],
                                            src_ip[2],
                                            src_ip[3],
                                            sport,
                                            dst_ip[0],
                                            dst_ip[1],
                                            dst_ip[2],
                                            dst_ip[3],
                                            dport,
                                            flags
                                        );
                                    }
                                }
                            }
                        }
                    }
                }

                if et == 0x0800 && buf.len() >= l2_off + 20 {
                    let ver_ihl = buf[l2_off];
                    let ihl = ((ver_ihl & 0x0f) as usize) * 4;
                    if (ver_ihl >> 4) == 4 && buf.len() >= l2_off + ihl + 8 {
                        let proto = buf[l2_off + 9];
                        if proto == 17 {
                            let udp_off = l2_off + ihl;
                            let sport = u16::from_be_bytes([buf[udp_off], buf[udp_off + 1]]);
                            let dport = u16::from_be_bytes([buf[udp_off + 2], buf[udp_off + 3]]);
                            if crate::logflag::NET_LOG_DHCP_VERBOSE && sport == 68 && dport == 67 {
                                crate::log_trace!(target: "net";
                                    "net: tx-tap dev={} saw dhcp client (udp 68->67)\n",
                                    self.index
                                );

                                // Extra DHCP sanity: log header fields and a few DHCP options.
                                // This is intentionally minimal and only runs for the first few
                                // frames per NIC.
                                let ip_src = [
                                    buf[l2_off + 12],
                                    buf[l2_off + 13],
                                    buf[l2_off + 14],
                                    buf[l2_off + 15],
                                ];
                                let ip_dst = [
                                    buf[l2_off + 16],
                                    buf[l2_off + 17],
                                    buf[l2_off + 18],
                                    buf[l2_off + 19],
                                ];
                                let ip_tot_len =
                                    u16::from_be_bytes([buf[l2_off + 2], buf[l2_off + 3]]);
                                let ip_hdr_csum =
                                    u16::from_be_bytes([buf[l2_off + 10], buf[l2_off + 11]]);
                                let udp_len =
                                    u16::from_be_bytes([buf[udp_off + 4], buf[udp_off + 5]]);
                                let udp_csum =
                                    u16::from_be_bytes([buf[udp_off + 6], buf[udp_off + 7]]);

                                // Verify IPv4 header checksum.
                                let mut sum: u32 = 0;
                                let mut i = 0usize;
                                while i + 1 < ihl {
                                    if i == 10 {
                                        i += 2;
                                        continue;
                                    }
                                    let w =
                                        u16::from_be_bytes([buf[l2_off + i], buf[l2_off + i + 1]])
                                            as u32;
                                    sum = sum.wrapping_add(w);
                                    i += 2;
                                }
                                while (sum >> 16) != 0 {
                                    sum = (sum & 0xFFFF) + (sum >> 16);
                                }
                                let ip_hdr_csum_calc = !(sum as u16);

                                crate::log_info!(target: "net";
                                    "net: dhcp-tx dev={} ip_src={}.{}.{}.{} ip_dst={}.{}.{}.{} ip_len={} ip_csum=0x{:04x} calc=0x{:04x} udp_len={} udp_csum=0x{:04x}\n",
                                    self.index,
                                    ip_src[0],
                                    ip_src[1],
                                    ip_src[2],
                                    ip_src[3],
                                    ip_dst[0],
                                    ip_dst[1],
                                    ip_dst[2],
                                    ip_dst[3],
                                    ip_tot_len,
                                    ip_hdr_csum,
                                    ip_hdr_csum_calc,
                                    udp_len,
                                    udp_csum
                                );

                                // DHCP/BOOTP minimal parse (RFC2131): UDP payload starts after 8 bytes.
                                let dhcp_off = udp_off + 8;
                                if buf.len() >= dhcp_off + 240 {
                                    let op = buf[dhcp_off];
                                    let htype = buf[dhcp_off + 1];
                                    let hlen = buf[dhcp_off + 2];
                                    let flags = u16::from_be_bytes([
                                        buf[dhcp_off + 10],
                                        buf[dhcp_off + 11],
                                    ]);
                                    let xid = u32::from_be_bytes([
                                        buf[dhcp_off + 4],
                                        buf[dhcp_off + 5],
                                        buf[dhcp_off + 6],
                                        buf[dhcp_off + 7],
                                    ]);
                                    let chaddr = &buf[dhcp_off + 28..dhcp_off + 44];
                                    let cookie = &buf[dhcp_off + 236..dhcp_off + 240];
                                    let cookie_ok = cookie == [99, 130, 83, 99];

                                    // Scan options for message type (53).
                                    let mut msg_type: u8 = 0;
                                    let mut opt_i = dhcp_off + 240;
                                    while opt_i < buf.len() {
                                        let code = buf[opt_i];
                                        opt_i += 1;
                                        if code == 0 {
                                            continue;
                                        }
                                        if code == 255 {
                                            break;
                                        }
                                        if opt_i >= buf.len() {
                                            break;
                                        }
                                        let olen = buf[opt_i] as usize;
                                        opt_i += 1;
                                        if opt_i + olen > buf.len() {
                                            break;
                                        }
                                        if code == 53 && olen >= 1 {
                                            msg_type = buf[opt_i];
                                            break;
                                        }
                                        opt_i += olen;
                                    }

                                    crate::log_info!(target: "net";
                                        "net: dhcp-tx dev={} op={} htype={} hlen={} flags=0x{:04x} xid=0x{:08x} chaddr={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} cookie_ok={} msg_type={}\n",
                                        self.index,
                                        op,
                                        htype,
                                        hlen,
                                        flags,
                                        xid,
                                        chaddr[0],
                                        chaddr[1],
                                        chaddr[2],
                                        chaddr[3],
                                        chaddr[4],
                                        chaddr[5],
                                        cookie_ok as u8,
                                        msg_type
                                    );
                                } else {
                                    crate::log_info!(target: "net";
                                        "net: dhcp-tx dev={} dhcp payload too short (len={})\n",
                                        self.index,
                                        buf.len().saturating_sub(dhcp_off)
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        self.tx_buffer.push_back(buf);

        if (new_total & NET_FRAME_LOG_MASK) == 0 {
            crate::log_info!(target: "net";
                "net: tx frames={} dropped={}\n",
                new_total,
                NET_TX_DROPPED.load(Ordering::Relaxed)
            );
        }
        result
    }

    fn set_meta(&mut self, _meta: PacketMeta) {}
}

struct SocketRecord {
    owner: &'static str,
    handle: NetHandle,
    kind: SocketKind,
    socket: SocketHandle,
    tcp_tx: VecDeque<u8>,
    tcp_loopback_peer: Option<NetHandle>,
    tcp_connect: bool,
    tcp_local_port: Option<u16>,
    tcp_remote_v4: Option<NetEndpoint>,
    tcp_remote_v6: Option<NetEndpointV6>,
    established: bool,
    last_tcp_state: Option<tcp::State>,
}

fn log_tcp_connect_record_state(prefix: &str, rec: &SocketRecord, state: tcp::State) {
    if let Some(remote) = rec.tcp_remote_v4 {
        let local_port = rec.tcp_local_port.unwrap_or(0);
        crate::log_info!(target: "net";
            "{} owner={} handle={} local_port={} remote={}.{}.{}.{}:{} state={:?}\n",
            prefix,
            rec.owner,
            rec.handle.0,
            local_port,
            remote.addr[0],
            remote.addr[1],
            remote.addr[2],
            remote.addr[3],
            remote.port,
            state
        );
    } else if let Some(remote) = rec.tcp_remote_v6 {
        let local_port = rec.tcp_local_port.unwrap_or(0);
        crate::log_info!(target: "net";
            "{} owner={} handle={} local_port={} remote6={:02x}{:02x}:{:02x}{:02x}:...:{} state={:?}\n",
            prefix,
            rec.owner,
            rec.handle.0,
            local_port,
            remote.addr[0],
            remote.addr[1],
            remote.addr[2],
            remote.addr[3],
            remote.port,
            state
        );
    } else {
        crate::log_info!(target: "net"; "{} owner={} handle={} state={:?}\n", prefix, rec.owner, rec.handle.0, state);
    }
}

#[inline]
fn is_ipv4_loopback(addr: [u8; 4]) -> bool {
    addr[0] == 127
}

struct IcmpInflight {
    owner: &'static str,
    seq: u16,
    sent_at: Instant,
}

struct NetService {
    device_index: usize,
    iface: Interface,
    sockets: SocketSet<'static>,
    rx_buffer: VecDeque<Vec<u8>>,
    tx_buffer: VecDeque<Vec<u8>>,
    records: Vec<SocketRecord>,
    next_handle: AtomicU32,
    icmp: SocketHandle,

    raw_icmpv6: SocketHandle,

    dhcp: SocketHandle,
    dhcp_has_lease: bool,
    local_ipv4: Option<Ipv4Address>,
    router_ipv4: Option<Ipv4Address>,
    local_ipv6_ll: Ipv6Address,
    local_ipv6_global: Option<Ipv6Address>,
    router_ipv6: Option<Ipv6Address>,
    rs_last_sent: Option<Instant>,
    dhcp_dns: [[u8; 4]; DHCP_DNS_MAX],
    dhcp_dns_count: u8,

    ra_dns6: [[u8; 16]; RA_DNS6_MAX],
    ra_dns6_count: u8,

    // DHCPv6 (stateful for address when M=1; stateless for DNS when O=1).
    dhcp6_udp: SocketHandle,
    dhcp6_stage: Dhcp6Stage,
    dhcp6_last_sent: Option<Instant>,
    dhcp6_cooldown_until: Option<Instant>,
    dhcp6_xid: [u8; 3],
    dhcp6_retries: u8,
    dhcp6_server_id: Option<Vec<u8>>,
    dhcp6_server_addr: Option<Ipv6Address>,
    dhcp6_candidate_addr: Option<Ipv6Address>,
    dhcp6_duid: [u8; 10],
    dhcp6_iaid: u32,
    dhcp6_dns6: [[u8; 16]; DHCP6_DNS6_MAX],
    dhcp6_dns6_count: u8,

    // First few DHCPv6 RX packets are logged to help diagnose interop issues.
    dhcp6_rx_samples_left: u8,

    ra_seen: bool,
    ra_managed: bool,
    ra_other: bool,
    ra_logged_flags: bool,
    ipv6_global_is_dhcp: bool,

    // Minimal ICMP reachability probe (ping gateway).
    icmp_ping_seq: u16,
    icmp_ping_inflight: Option<(u16, Instant)>,
    icmp_ping_last_sent: Option<Instant>,
    icmp_ping_pongs: u8,

    // IPv6 reachability probe (ping router).
    icmp6_ping_seq: u16,
    icmp6_ping_inflight: Option<(u16, Instant)>,
    icmp6_ping_last_sent: Option<Instant>,
    icmp6_ping_pongs: u8,
    icmp_vnet_inflight: Vec<IcmpInflight>,

    tcp_next_ephemeral: u16,

    internal_netbench: Vec<InternalNetbenchState>,
    internal_netbench_combined: Option<InternalNetbenchCombinedLog>,
}

impl NetService {
    fn new(device_index: usize) -> Self {
        let mac = crate::net::mac_address_at(device_index).unwrap_or([0, 0, 0, 0, 0, 1]);
        crate::log_info!(target: "net";
            "net: dev={} mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}\n",
            device_index,
            mac[0],
            mac[1],
            mac[2],
            mac[3],
            mac[4],
            mac[5]
        );
        let hw_addr = HardwareAddress::Ethernet(EthernetAddress(mac));

        let mut cfg = IfaceConfig::new(hw_addr);
        cfg.random_seed = crate::tyche::rdrand_u64().unwrap_or(0x9E37_79B9);

        let mut rx_buffer = VecDeque::new();
        let mut tx_buffer = VecDeque::new();
        let mut device = AdapterDeviceAt {
            index: device_index,
            rx_buffer: &mut rx_buffer,
            tx_buffer: &mut tx_buffer,
        };
        let mut iface = Interface::new(cfg, &mut device, now());

        // mDNS/DNS-SD uses IPv4 multicast.
        let _ = iface.join_multicast_group(IpAddress::Ipv4(Ipv4Address::new(224, 0, 0, 251)));
        // ESP32 Wi-Fi RTP microphone stream.
        let _ = iface.join_multicast_group(IpAddress::Ipv4(Ipv4Address::new(239, 255, 77, 77)));
        // Ensure the stack accepts multicast IPv6 control traffic.
        // Router Advertisements are commonly sent to ff02::1 (all-nodes).
        let _ = iface.join_multicast_group(IpAddress::Ipv6(Ipv6Address::new(
            0xff02, 0, 0, 0, 0, 0, 0, 0x0001,
        )));
        // DHCPv6 servers/relays listen on ff02::1:2.
        let _ = iface.join_multicast_group(IpAddress::Ipv6(Ipv6Address::new(
            0xff02, 0, 0, 0, 0, 0, 0x0001, 0x0002,
        )));
        // Always install an IPv6 link-local address derived from the MAC.
        let ipv6_ll = ipv6_link_local_from_mac(mac);

        // Bring-up default: install static IPv4 before DHCP starts.
        let (fallback_ip, fallback_gw) =
            apply_static_fallback_ipv4(&mut iface, device_index, ipv6_ll);

        // Note: Some networks reply to RS with multicast RAs (ff02::1). Joining
        // multicast groups requires smoltcp's multicast interface support; we
        // currently rely on routers replying unicast to our RS.
        let ip_o = fallback_ip.octets();
        let gw_o = fallback_gw.octets();
        crate::log_info!(target: "net";
            "net: static fallback dev={} ipv4={}.{}.{}.{} mask=255.255.255.0 gw={}.{}.{}.{}\n",
            device_index,
            ip_o[0],
            ip_o[1],
            ip_o[2],
            ip_o[3],
            gw_o[0],
            gw_o[1],
            gw_o[2],
            gw_o[3]
        );

        let rx_meta = vec![icmp::PacketMetadata::EMPTY; 8];
        let rx_buf = vec![0u8; 2048];
        let tx_meta = vec![icmp::PacketMetadata::EMPTY; 8];
        let tx_buf = vec![0u8; 2048];
        let rx = icmp::PacketBuffer::new(rx_meta, rx_buf);
        let tx = icmp::PacketBuffer::new(tx_meta, tx_buf);
        let mut icmp_socket = icmp::Socket::new(rx, tx);
        let _ = icmp_socket.bind(icmp::Endpoint::Ident(ICMP_IDENT));

        // Raw ICMPv6 socket (for Router Advertisements / NDISC). ICMP sockets
        // only accept echo or UDP-related errors.
        let raw_rx_meta = vec![raw::PacketMetadata::EMPTY; 8];
        let raw_rx_buf = vec![0u8; 2048];
        let raw_tx_meta = vec![raw::PacketMetadata::EMPTY; 4];
        let raw_tx_buf = vec![0u8; 256];
        let raw_rx = raw::PacketBuffer::new(raw_rx_meta, raw_rx_buf);
        let raw_tx = raw::PacketBuffer::new(raw_tx_meta, raw_tx_buf);
        let raw_icmpv6_socket =
            raw::Socket::new(Some(IpVersion::Ipv6), Some(IpProtocol::Icmpv6), raw_rx, raw_tx);

        // DHCPv6 client socket (UDP 546).
        let dhcp6_rx_meta = vec![udp::PacketMetadata::EMPTY; 8];
        let dhcp6_rx_buf = vec![0u8; 2048];
        let dhcp6_tx_meta = vec![udp::PacketMetadata::EMPTY; 4];
        let dhcp6_tx_buf = vec![0u8; 512];
        let dhcp6_rx = udp::PacketBuffer::new(dhcp6_rx_meta, dhcp6_rx_buf);
        let dhcp6_tx = udp::PacketBuffer::new(dhcp6_tx_meta, dhcp6_tx_buf);
        let mut dhcp6_udp_socket = udp::Socket::new(dhcp6_rx, dhcp6_tx);
        let _ = dhcp6_udp_socket.bind(crate::net::dhcpv6::CLIENT_PORT);
        // DHCPv6 packets are link-local and should not be forwarded.
        // Some servers enforce IPv6 Hop Limit = 1.
        dhcp6_udp_socket.set_hop_limit(Some(1));

        let mut sockets = SocketSet::new(Vec::new());
        let icmp = sockets.add(icmp_socket);
        let raw_icmpv6 = sockets.add(raw_icmpv6_socket);

        let dhcp6_udp = sockets.add(dhcp6_udp_socket);

        let dhcp_socket = dhcpv4::Socket::new();
        // let current_hostname = get_hostname();
        // dhcp_socket.set_outgoing_options(&[DhcpOption {
        //     kind: 12,
        //     data: current_hostname.as_bytes(),
        // }]);
        let dhcp = sockets.add(dhcp_socket);
        crate::log_info!(target: "net"; "net: dhcp start dev={} mode=static-fallback\n", device_index);

        let dhcp6_duid = crate::net::dhcpv6::duid_ll_from_mac(mac);
        let dhcp6_iaid =
            u32::from_be_bytes([mac[2], mac[3], mac[4], mac[5]]) ^ ((device_index as u32) << 24);

        Self {
            device_index,
            iface,
            sockets,
            rx_buffer,
            tx_buffer,
            records: Vec::new(),
            next_handle: AtomicU32::new(1),
            icmp,

            raw_icmpv6,

            dhcp,
            dhcp_has_lease: false,
            local_ipv4: Some(fallback_ip),
            router_ipv4: Some(fallback_gw),
            local_ipv6_ll: ipv6_ll,
            local_ipv6_global: None,
            router_ipv6: None,
            rs_last_sent: None,
            dhcp_dns: [[0u8; 4]; DHCP_DNS_MAX],
            dhcp_dns_count: 0,

            ra_dns6: [[0u8; 16]; RA_DNS6_MAX],
            ra_dns6_count: 0,

            dhcp6_udp,
            dhcp6_stage: Dhcp6Stage::Idle,
            dhcp6_last_sent: None,
            dhcp6_cooldown_until: None,
            dhcp6_xid: [0, 0, 0],
            dhcp6_retries: 0,
            dhcp6_server_id: None,
            dhcp6_server_addr: None,
            dhcp6_candidate_addr: None,
            dhcp6_duid,
            dhcp6_iaid,
            dhcp6_dns6: [[0u8; 16]; DHCP6_DNS6_MAX],
            dhcp6_dns6_count: 0,
            dhcp6_rx_samples_left: crate::logflag::NET_LOG_DHCP6_SAMPLES as u8,

            ra_seen: false,
            ra_managed: false,
            ra_other: false,
            ra_logged_flags: false,
            ipv6_global_is_dhcp: false,

            icmp_ping_seq: 0,
            icmp_ping_inflight: None,
            icmp_ping_last_sent: None,
            icmp_ping_pongs: 0,

            icmp6_ping_seq: 0,
            icmp6_ping_inflight: None,
            icmp6_ping_last_sent: None,
            icmp6_ping_pongs: 0,
            icmp_vnet_inflight: Vec::new(),

            tcp_next_ephemeral: 49152,

            internal_netbench: Vec::new(),
            internal_netbench_combined: None,
        }
    }

    fn internal_netbench_find_header_end(buf: &[u8]) -> Option<usize> {
        buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
    }

    fn internal_netbench_parse_content_length(headers: &[u8]) -> Option<usize> {
        // Extremely small parser: find case-insensitive "content-length:" at line start.
        let mut i = 0usize;
        while i < headers.len() {
            let line_start = i;
            while i < headers.len() && headers[i] != b'\n' {
                i += 1;
            }
            let mut line = &headers[line_start..i];
            if i < headers.len() && headers[i] == b'\n' {
                i += 1;
            }
            if let Some((&b'\r', rest)) = line.split_last() {
                line = rest;
            }
            if line.is_empty() {
                continue;
            }
            let Some(colon) = line.iter().position(|b| *b == b':') else {
                continue;
            };
            let (k, mut v) = line.split_at(colon);
            v = v.get(1..).unwrap_or(&[]);
            if k.len() != b"content-length".len() {
                continue;
            }
            if !k
                .iter()
                .zip(b"content-length".iter())
                .all(|(a, b)| a.eq_ignore_ascii_case(b))
            {
                continue;
            }
            while !v.is_empty() && (v[0] == b' ' || v[0] == b'\t') {
                v = &v[1..];
            }
            let s = core::str::from_utf8(v).ok()?;
            return s.trim().parse::<usize>().ok();
        }
        None
    }

    fn internal_netbench_tick(&mut self, timestamp: Instant) -> bool {
        let mut did_work = false;

        // Start runs if we have pending requests targeted at this NIC.
        while self.internal_netbench.len() < INTERNAL_NETBENCH_MAX_CONCURRENT_PER_NIC {
            let maybe_req = {
                let mut g = INTERNAL_NETBENCH_REQS.lock();
                let pos = g.iter().position(|r| r.device_index == self.device_index);
                pos.map(|p| g.remove(p))
            };
            let Some(req) = maybe_req else {
                break;
            };

            let rx = tcp::SocketBuffer::new(vec![0; TCP_RX_BUF_BYTES]);
            let tx = tcp::SocketBuffer::new(vec![0; TCP_TX_BUF_BYTES]);
            let mut sock = tcp::Socket::new(rx, tx);
            sock.set_keep_alive(Some(SmolDuration::from_secs(30)));

            let local_port = self.tcp_next_ephemeral;
            self.tcp_next_ephemeral = self.tcp_next_ephemeral.wrapping_add(1).max(49152);

            let (local, remote, remote_log_v4, remote_log_v6) = match req.remote {
                InternalNetbenchRemote::V4(ip) => {
                    let Some(local_ip) = self.local_ipv4 else {
                        crate::log_info!(target: "net"; "netbench-internal: no ipv4 configured\n");
                        return did_work;
                    };
                    (
                        IpEndpoint::new(IpAddress::Ipv4(local_ip), local_port),
                        IpEndpoint::new(
                            IpAddress::Ipv4(Ipv4Address::from_octets(ip)),
                            req.remote_port,
                        ),
                        Some(ip),
                        None,
                    )
                }
                InternalNetbenchRemote::V6(ip) => {
                    let Some(local_ip) = self.local_ipv6_global else {
                        crate::log_info!(target: "net";
                            "netbench-internal: no global ipv6 yet (dev={})\n",
                            self.device_index
                        );
                        return did_work;
                    };
                    (
                        IpEndpoint::new(IpAddress::Ipv6(local_ip), local_port),
                        IpEndpoint::new(
                            IpAddress::Ipv6(Ipv6Address::from_octets(ip)),
                            req.remote_port,
                        ),
                        None,
                        Some(ip),
                    )
                }
            };

            if sock.connect(self.iface.context(), remote, local).is_err() {
                crate::log_info!(target: "net"; "netbench-internal: connect failed id={}\n", req.id);
                return did_work;
            }

            let sh = self.sockets.add(sock);
            if let Some(ip) = remote_log_v4 {
                crate::log_info!(target: "net";
                    "netbench-internal: started id={} dev={} remote={}.{}.{}.{}:{}\n",
                    req.id,
                    self.device_index,
                    ip[0],
                    ip[1],
                    ip[2],
                    ip[3],
                    req.remote_port
                );
            } else if let Some(ip) = remote_log_v6 {
                crate::log_info!(target: "net";
                    "netbench-internal: started id={} dev={} remote6={:02x}{:02x}:{:02x}{:02x}:...:{}\n",
                    req.id,
                    self.device_index,
                    ip[0],
                    ip[1],
                    ip[2],
                    ip[3],
                    req.remote_port
                );
            }

            let now_tick = embassy_time_driver::now();
            if self.internal_netbench.is_empty() {
                // First active run: initialize combined logging window.
                self.internal_netbench_combined = Some(InternalNetbenchCombinedLog {
                    start_tick: now_tick,
                    last_log_tick: now_tick,
                    last_log_received: 0,
                });
            }

            self.internal_netbench.push(InternalNetbenchState {
                id: req.id,
                socket: sh,
                request: req.request,
                request_sent: false,
                header: [0u8; 16 * 1024],
                header_len: 0,
                header_done: false,
                expected_len: None,
                received: 0,
                start_tick: now_tick,
                last_log_tick: now_tick,
                last_log_received: 0,
                last_tcp_state: None,
            });

            did_work = true;
        }

        if self.internal_netbench.is_empty() {
            return did_work;
        }

        // Drive each active run.
        let mut i = 0usize;
        while i < self.internal_netbench.len() {
            let mut remove = false;

            let (cur_state, elapsed_ms, received, id, header_done, expected_len, sock_open) = {
                let st = &mut self.internal_netbench[i];
                let sock = self.sockets.get_mut::<tcp::Socket>(st.socket);

                // Log TCP state transitions for bring-up debugging.
                let cur_state = sock.state();
                if st.last_tcp_state != Some(cur_state) {
                    st.last_tcp_state = Some(cur_state);
                    crate::log_info!(target: "net";
                        "netbench-internal: tcp id={} state={:?} open={} active={} can_send={} may_send={} can_recv={} may_recv={}\n",
                        st.id,
                        cur_state,
                        sock.is_open() as u8,
                        sock.is_active() as u8,
                        sock.can_send() as u8,
                        sock.may_send() as u8,
                        sock.can_recv() as u8,
                        sock.may_recv() as u8
                    );
                }

                // Send request once we can send.
                if !st.request_sent
                    && sock.can_send()
                    && sock.may_send()
                    && sock.send_slice(&st.request[..]).is_ok()
                {
                    st.request_sent = true;
                    crate::log_info!(target: "net"; "netbench-internal: request sent id={}\n", st.id);
                    did_work = true;
                }

                // Drain receive data without allocating/copying into events.
                if sock.can_recv() && sock.may_recv() {
                    let mut scratch = POLL_SCRATCH_BUF.lock();
                    let buf = &mut *scratch;

                    for _ in 0..64 {
                        let Ok(len) = sock.recv_slice(buf) else {
                            break;
                        };
                        if len == 0 {
                            break;
                        }

                        did_work = true;
                        let bytes = &buf[..len];

                        if !st.header_done {
                            let space = st.header.len().saturating_sub(st.header_len);
                            let take = space.min(bytes.len());
                            st.header[st.header_len..st.header_len + take]
                                .copy_from_slice(&bytes[..take]);
                            st.header_len += take;

                            if let Some(hend) =
                                Self::internal_netbench_find_header_end(&st.header[..st.header_len])
                            {
                                st.header_done = true;
                                st.expected_len = Self::internal_netbench_parse_content_length(
                                    &st.header[..hend],
                                );
                                if let Some(cl) = st.expected_len {
                                    crate::log_info!(target: "net";
                                        "netbench-internal: content-length id={} len={}\n",
                                        st.id,
                                        cl
                                    );
                                }

                                // Any remaining bytes after the header count as body.
                                let rem = st.header_len.saturating_sub(hend);
                                st.received = st.received.saturating_add(rem as u64);

                                // Also count any bytes in this recv chunk that didn't fit into header buffer.
                                let extra = bytes.len().saturating_sub(take);
                                st.received = st.received.saturating_add(extra as u64);
                            }
                        } else {
                            st.received = st.received.saturating_add(bytes.len() as u64);
                        }
                    }
                }

                // Periodic log (average since start). Keep it low-frequency.
                let now_tick = embassy_time_driver::now();
                let hz = embassy_time_driver::TICK_HZ;
                let elapsed_ticks = now_tick.saturating_sub(st.start_tick);
                let elapsed_ms = if hz == 0 {
                    0
                } else {
                    elapsed_ticks.saturating_mul(1000) / hz
                };
                if hz != 0 {
                    let log_every_ticks = hz; // ~1s
                    if now_tick.saturating_sub(st.last_log_tick) >= log_every_ticks {
                        let window_ticks = now_tick.saturating_sub(st.last_log_tick);
                        let window_ms = window_ticks.saturating_mul(1000) / hz;
                        let delta = st.received.saturating_sub(st.last_log_received);
                        st.last_log_tick = now_tick;
                        st.last_log_received = st.received;

                        let bps = if elapsed_ms == 0 {
                            0
                        } else {
                            st.received.saturating_mul(1000) / elapsed_ms
                        };

                        let inst_bps = if window_ms == 0 {
                            0
                        } else {
                            delta.saturating_mul(1000) / window_ms
                        };

                        crate::log_info!(target: "net";
                            "netbench-internal: rx id={} bytes={} inst={} avg={} state={:?}\n",
                            st.id,
                            st.received,
                            internal_netbench_format_speed(inst_bps),
                            internal_netbench_format_speed(bps),
                            cur_state
                        );
                    }
                }

                (
                    cur_state,
                    elapsed_ms,
                    st.received,
                    st.id,
                    st.header_done,
                    st.expected_len,
                    sock.is_open(),
                )
            };

            // Completion checks.
            let done_by_len = expected_len
                .map(|cl| received >= cl as u64)
                .unwrap_or(false);
            let done_by_close = !sock_open;

            if done_by_len || done_by_close {
                let bps = if elapsed_ms == 0 {
                    0
                } else {
                    received.saturating_mul(1000) / elapsed_ms
                };
                crate::log_info!(target: "net";
                    "netbench-internal: done id={} dev={} bytes={} elapsed_ms={} avg={} header_done={} state={:?}\n",
                    id,
                    self.device_index,
                    received,
                    elapsed_ms,
                    internal_netbench_format_speed(bps),
                    header_done as u8,
                    cur_state
                );

                // Close and remove socket.
                let sh = self.internal_netbench[i].socket;
                {
                    let sock = self.sockets.get_mut::<tcp::Socket>(sh);
                    sock.close();
                }
                let _ = self.sockets.remove(sh);
                remove = true;
                did_work = true;
            }

            if remove {
                self.internal_netbench.remove(i);
                continue;
            }

            i += 1;
        }

        // Combined throughput log: useful when multiple connections are active.
        if !self.internal_netbench.is_empty() {
            let total_received: u64 = self.internal_netbench.iter().map(|s| s.received).sum();
            let now_tick = embassy_time_driver::now();
            let hz = embassy_time_driver::TICK_HZ;
            if hz != 0 {
                let log_every_ticks = hz; // ~1s
                if let Some(c) = self.internal_netbench_combined.as_mut() {
                    if now_tick.saturating_sub(c.last_log_tick) >= log_every_ticks {
                        let elapsed_ticks = now_tick.saturating_sub(c.start_tick);
                        let elapsed_ms = elapsed_ticks.saturating_mul(1000) / hz;

                        let window_ticks = now_tick.saturating_sub(c.last_log_tick);
                        let window_ms = window_ticks.saturating_mul(1000) / hz;
                        let delta = total_received.saturating_sub(c.last_log_received);
                        c.last_log_tick = now_tick;
                        c.last_log_received = total_received;

                        let bps = if elapsed_ms == 0 {
                            0
                        } else {
                            total_received.saturating_mul(1000) / elapsed_ms
                        };
                        let inst_bps = if window_ms == 0 {
                            0
                        } else {
                            delta.saturating_mul(1000) / window_ms
                        };

                        crate::log_info!(target: "net";
                            "netbench-internal: combined dev={} active={} bytes={} inst={} avg={}\n",
                            self.device_index,
                            self.internal_netbench.len(),
                            total_received,
                            internal_netbench_format_speed(inst_bps),
                            internal_netbench_format_speed(bps)
                        );
                    }
                }
            }
        } else {
            self.internal_netbench_combined = None;
        }

        let _ = timestamp; // reserved for future timeout logic
        did_work
    }

    fn alloc_handle(&self) -> NetHandle {
        NetHandle(self.next_handle.fetch_add(1, Ordering::Relaxed))
    }

    fn find_record(&mut self, handle: NetHandle) -> Option<&mut SocketRecord> {
        self.records.iter_mut().find(|rec| rec.handle == handle)
    }

    fn remove_record(&mut self, handle: NetHandle) {
        if let Some(idx) = self.records.iter().position(|rec| rec.handle == handle) {
            let rec = self.records.remove(idx);
            let _ = self.sockets.remove(rec.socket);
        }
    }

    fn link_up(&self) -> bool {
        crate::net::link_state_at(self.device_index)
            .map(|state| state.up)
            .unwrap_or(false)
    }

    fn require_link_up(&self) -> Result<(), &'static str> {
        if self.link_up() {
            Ok(())
        } else {
            Err("link down")
        }
    }

    fn open_udp(&mut self, owner: &'static str, port: u16) -> Result<NetHandle, &'static str> {
        self.require_link_up()?;
        if self.records.len() >= MAX_SOCKETS {
            return Err("no sockets available");
        }

        let meta_rx = vec![udp::PacketMetadata::EMPTY; 8];
        let buf_rx = vec![0u8; 2048];
        let meta_tx = vec![udp::PacketMetadata::EMPTY; 8];
        let buf_tx = vec![0u8; 2048];
        let rx = udp::PacketBuffer::new(meta_rx, buf_rx);
        let tx = udp::PacketBuffer::new(meta_tx, buf_tx);
        let mut socket = udp::Socket::new(rx, tx);
        socket.bind(port).map_err(|_| "bind failed")?;

        let handle = self.alloc_handle();
        let sh = self.sockets.add(socket);
        self.records.push(SocketRecord {
            owner,
            handle,
            kind: SocketKind::Udp,
            socket: sh,
            tcp_tx: VecDeque::new(),
            tcp_loopback_peer: None,
            tcp_connect: false,
            tcp_local_port: None,
            tcp_remote_v4: None,
            tcp_remote_v6: None,
            established: false,
            last_tcp_state: None,
        });
        Ok(handle)
    }

    fn open_tcp(&mut self, owner: &'static str, port: u16) -> Result<NetHandle, &'static str> {
        self.require_link_up()?;
        if self.records.len() >= MAX_SOCKETS {
            return Err("no sockets available");
        }

        let rx = tcp::SocketBuffer::new(vec![0; TCP_RX_BUF_BYTES]);
        let tx = tcp::SocketBuffer::new(vec![0; TCP_TX_BUF_BYTES]);
        let mut socket = tcp::Socket::new(rx, tx);
        socket.listen(port).map_err(|_| "listen failed")?;
        socket.set_keep_alive(Some(SmolDuration::from_secs(30)));
        let initial_state = socket.state();

        let handle = self.alloc_handle();
        let sh = self.sockets.add(socket);
        self.records.push(SocketRecord {
            owner,
            handle,
            kind: SocketKind::Tcp,
            socket: sh,
            tcp_tx: VecDeque::new(),
            tcp_loopback_peer: None,
            tcp_connect: false,
            tcp_local_port: Some(port),
            tcp_remote_v4: None,
            tcp_remote_v6: None,
            established: false,
            last_tcp_state: Some(initial_state),
        });
        Ok(handle)
    }

    fn open_loopback_tcp_connect(
        &mut self,
        owner: &'static str,
        remote: NetEndpoint,
    ) -> Result<(NetHandle, NetHandle, &'static str), &'static str> {
        if self.records.len().saturating_add(2) > MAX_SOCKETS {
            return Err("no sockets available");
        }

        let listener_owner = self
            .records
            .iter()
            .find(|rec| {
                rec.kind == SocketKind::Tcp
                    && rec.tcp_loopback_peer.is_none()
                    && !rec.tcp_connect
                    && rec.tcp_local_port == Some(remote.port)
                    && rec.tcp_remote_v4.is_none()
                    && rec.tcp_remote_v6.is_none()
            })
            .map(|rec| rec.owner)
            .ok_or("connect failed")?;

        let local_port = self.tcp_next_ephemeral;
        self.tcp_next_ephemeral = self.tcp_next_ephemeral.wrapping_add(1).max(49152);

        let client_handle = self.alloc_handle();
        let server_handle = self.alloc_handle();

        let client_rx = tcp::SocketBuffer::new(vec![0; TCP_RX_BUF_BYTES]);
        let client_tx = tcp::SocketBuffer::new(vec![0; TCP_TX_BUF_BYTES]);
        let mut client_socket = tcp::Socket::new(client_rx, client_tx);
        client_socket.set_keep_alive(Some(SmolDuration::from_secs(30)));
        let client_socket = self.sockets.add(client_socket);

        let server_rx = tcp::SocketBuffer::new(vec![0; TCP_RX_BUF_BYTES]);
        let server_tx = tcp::SocketBuffer::new(vec![0; TCP_TX_BUF_BYTES]);
        let mut server_socket = tcp::Socket::new(server_rx, server_tx);
        server_socket.set_keep_alive(Some(SmolDuration::from_secs(30)));
        let server_socket = self.sockets.add(server_socket);

        self.records.push(SocketRecord {
            owner,
            handle: client_handle,
            kind: SocketKind::Tcp,
            socket: client_socket,
            tcp_tx: VecDeque::new(),
            tcp_loopback_peer: Some(server_handle),
            tcp_connect: true,
            tcp_local_port: Some(local_port),
            tcp_remote_v4: Some(remote),
            tcp_remote_v6: None,
            established: true,
            last_tcp_state: None,
        });

        self.records.push(SocketRecord {
            owner: listener_owner,
            handle: server_handle,
            kind: SocketKind::Tcp,
            socket: server_socket,
            tcp_tx: VecDeque::new(),
            tcp_loopback_peer: Some(client_handle),
            tcp_connect: false,
            tcp_local_port: Some(remote.port),
            tcp_remote_v4: Some(NetEndpoint {
                addr: [127, 0, 0, 1],
                port: local_port,
            }),
            tcp_remote_v6: None,
            established: true,
            last_tcp_state: None,
        });

        if crate::logflag::NET_LOG_TCP_FLOW {
            crate::log_info!(target: "net";
                "net: loopback tcp connect port={} client={} server={}\n",
                remote.port,
                client_handle.0,
                server_handle.0
            );
        }

        Ok((client_handle, server_handle, listener_owner))
    }

    fn open_tcp_connect(
        &mut self,
        owner: &'static str,
        remote: NetEndpoint,
    ) -> Result<NetHandle, &'static str> {
        self.require_link_up()?;
        if self.records.len() >= MAX_SOCKETS {
            return Err("no sockets available");
        }

        let rx = tcp::SocketBuffer::new(vec![0; TCP_RX_BUF_BYTES]);
        let tx = tcp::SocketBuffer::new(vec![0; TCP_TX_BUF_BYTES]);
        let mut socket = tcp::Socket::new(rx, tx);
        socket.set_keep_alive(Some(SmolDuration::from_secs(30)));

        let local_port = self.tcp_next_ephemeral;
        self.tcp_next_ephemeral = self.tcp_next_ephemeral.wrapping_add(1).max(49152);

        let local_ip = self.local_ipv4.ok_or("no ipv4 configured")?;

        let local = IpEndpoint::new(IpAddress::Ipv4(local_ip), local_port);
        let remote_addr = remote.addr;
        let remote_port = remote.port;
        let record_remote = remote;
        let remote =
            IpEndpoint::new(IpAddress::Ipv4(Ipv4Address::from_octets(remote_addr)), remote_port);

        let local_octets = local_ip.octets();
        if crate::logflag::NET_LOG_TCP_FLOW {
            crate::log_info!(target: "net";
                "net: tcp connect owner={} local={}.{}.{}.{}:{} remote={}.{}.{}.{}:{}\n",
                owner,
                local_octets[0],
                local_octets[1],
                local_octets[2],
                local_octets[3],
                local_port,
                remote_addr[0],
                remote_addr[1],
                remote_addr[2],
                remote_addr[3],
                remote_port
            );
        }

        socket
            .connect(self.iface.context(), remote, local)
            .map_err(|_| "connect failed")?;

        let initial_state = socket.state();

        let handle = self.alloc_handle();
        let sh = self.sockets.add(socket);
        if crate::logflag::NET_LOG_TCP_CONNECT_STATES {
            crate::log_info!(target: "net";
                "net: tcp connect-open owner={} handle={} local={}.{}.{}.{}:{} remote={}.{}.{}.{}:{} state={:?}\n",
                owner,
                handle.0,
                local_octets[0],
                local_octets[1],
                local_octets[2],
                local_octets[3],
                local_port,
                remote_addr[0],
                remote_addr[1],
                remote_addr[2],
                remote_addr[3],
                remote_port,
                initial_state
            );
        }
        self.records.push(SocketRecord {
            owner,
            handle,
            kind: SocketKind::Tcp,
            socket: sh,
            tcp_tx: VecDeque::new(),
            tcp_loopback_peer: None,
            tcp_connect: true,
            tcp_local_port: Some(local_port),
            tcp_remote_v4: Some(record_remote),
            tcp_remote_v6: None,
            established: false,
            last_tcp_state: Some(initial_state),
        });
        Ok(handle)
    }

    fn open_tcp_connect_v6(
        &mut self,
        owner: &'static str,
        remote: NetEndpointV6,
    ) -> Result<NetHandle, &'static str> {
        self.require_link_up()?;
        if self.records.len() >= MAX_SOCKETS {
            return Err("no sockets available");
        }

        let rx = tcp::SocketBuffer::new(vec![0; TCP_RX_BUF_BYTES]);
        let tx = tcp::SocketBuffer::new(vec![0; TCP_TX_BUF_BYTES]);
        let mut socket = tcp::Socket::new(rx, tx);
        socket.set_keep_alive(Some(SmolDuration::from_secs(30)));

        let local_port = self.tcp_next_ephemeral;
        self.tcp_next_ephemeral = self.tcp_next_ephemeral.wrapping_add(1).max(49152);

        let local_ip = self.local_ipv6_global.unwrap_or(self.local_ipv6_ll);
        let local = IpEndpoint::new(IpAddress::Ipv6(local_ip), local_port);

        let remote_ip = Ipv6Address::from_octets(remote.addr);
        let record_remote = remote;
        let remote = IpEndpoint::new(IpAddress::Ipv6(remote_ip), remote.port);

        crate::log_info!(target: "net";
            "net: tcp6 connect owner={} local=[{}]:{} remote=[{}]:{}\n",
            owner,
            local_ip,
            local_port,
            remote_ip,
            remote.port
        );

        socket
            .connect(self.iface.context(), remote, local)
            .map_err(|_| "connect failed")?;

        let initial_state = socket.state();

        let handle = self.alloc_handle();
        let sh = self.sockets.add(socket);
        if crate::logflag::NET_LOG_TCP_CONNECT_STATES {
            crate::log_info!(target: "net";
                "net: tcp6 connect-open owner={} handle={} local_port={} remote6={:02x}{:02x}:{:02x}{:02x}:...:{} state={:?}\n",
                owner,
                handle.0,
                local_port,
                record_remote.addr[0],
                record_remote.addr[1],
                record_remote.addr[2],
                record_remote.addr[3],
                record_remote.port,
                initial_state
            );
        }
        self.records.push(SocketRecord {
            owner,
            handle,
            kind: SocketKind::Tcp,
            socket: sh,
            tcp_tx: VecDeque::new(),
            tcp_loopback_peer: None,
            tcp_connect: true,
            tcp_local_port: Some(local_port),
            tcp_remote_v4: None,
            tcp_remote_v6: Some(record_remote),
            established: false,
            last_tcp_state: Some(initial_state),
        });
        Ok(handle)
    }

    fn send_loopback_tcp(&mut self, handle: NetHandle, data: Vec<u8>) -> Option<Result<(), ()>> {
        let idx = self.records.iter().position(|r| r.handle == handle)?;
        if self.records[idx].kind != SocketKind::Tcp {
            return Some(Err(()));
        }

        let peer = self.records[idx].tcp_loopback_peer?;
        let owner = self.records[idx].owner;
        let peer_owner = match self.records.iter().find(|r| r.handle == peer) {
            Some(rec) => rec.owner,
            None => {
                let _ = push_event(owner, NetEvent::Closed { handle });
                self.remove_record(handle);
                return Some(Ok(()));
            }
        };

        let len = data.len();
        let _ = push_event(peer_owner, NetEvent::TcpData { handle: peer, data });
        let _ = push_event(owner, NetEvent::TcpSent { handle, len });
        Some(Ok(()))
    }

    fn close_loopback_tcp(&mut self, handle: NetHandle) -> bool {
        let Some(idx) = self.records.iter().position(|r| r.handle == handle) else {
            return false;
        };
        let Some(peer) = self.records[idx].tcp_loopback_peer else {
            return false;
        };

        let owner = self.records[idx].owner;
        let peer_info = self
            .records
            .iter()
            .find(|r| r.handle == peer)
            .map(|r| (r.owner, r.handle));

        if let Some(peer_idx) = self.records.iter().position(|r| r.handle == peer) {
            if peer_idx > idx {
                self.remove_record(peer);
                self.remove_record(handle);
            } else {
                self.remove_record(handle);
                self.remove_record(peer);
            }
        } else {
            self.remove_record(handle);
        }

        let _ = push_event(owner, NetEvent::Closed { handle });
        if let Some((peer_owner, peer_handle)) = peer_info {
            let _ = push_event(
                peer_owner,
                NetEvent::Closed {
                    handle: peer_handle,
                },
            );
        }

        true
    }

    fn flush_tcp_tx(&mut self, idx: usize) {
        if self.records.get(idx).map(|r| r.kind) != Some(SocketKind::Tcp) {
            return;
        }

        let owner = self.records[idx].owner;
        let handle = self.records[idx].handle;

        if self.records[idx].tcp_tx.is_empty() {
            return;
        }

        let socket_handle = self.records[idx].socket;
        let socket = self.sockets.get_mut::<tcp::Socket>(socket_handle);
        if !(socket.can_send() && socket.may_send()) {
            return;
        }

        // Bound work per tick to keep other sockets responsive.
        let mut total_sent = 0usize;
        for _ in 0..64 {
            let (a, b) = self.records[idx].tcp_tx.as_slices();
            let chunk = if !a.is_empty() { a } else { b };
            if chunk.is_empty() {
                break;
            }

            match socket.send_slice(chunk) {
                Ok(sent) => {
                    if sent == 0 {
                        break;
                    }
                    total_sent = total_sent.saturating_add(sent);
                    for _ in 0..sent {
                        let _ = self.records[idx].tcp_tx.pop_front();
                    }
                }
                Err(_) => {
                    let _ = push_event(
                        owner,
                        NetEvent::Error {
                            msg: "tcp send fail",
                        },
                    );
                    break;
                }
            }

            if !(socket.can_send() && socket.may_send()) {
                break;
            }
        }

        if total_sent != 0 {
            if crate::logflag::NET_LOG_TCP_SEND_FLUSH {
                if owner == "logtotcp" {
                    LOGTOTCP_SEND_FLUSH_SUPPRESSED.fetch_add(1, Ordering::Relaxed);
                    LOGTOTCP_SEND_FLUSH_BYTES.fetch_add(total_sent as u64, Ordering::Relaxed);
                    if net_log_once_per_second(&LOGTOTCP_SEND_FLUSH_LAST_LOG_NS) {
                        let flushes = LOGTOTCP_SEND_FLUSH_SUPPRESSED.swap(0, Ordering::Relaxed);
                        let bytes = LOGTOTCP_SEND_FLUSH_BYTES.swap(0, Ordering::Relaxed);
                        crate::log_info!(target: "net";
                            "net: sendtcp flush owner={} handle={} flushes={} bytes={} last_sent={} queued_left={}\n",
                            owner,
                            handle.0,
                            flushes,
                            bytes,
                            total_sent,
                            self.records[idx].tcp_tx.len()
                        );
                    }
                } else {
                    crate::log_info!(target: "net";
                        "net: sendtcp flush owner={} handle={} sent={} queued_left={}\n",
                        owner,
                        handle.0,
                        total_sent,
                        self.records[idx].tcp_tx.len()
                    );
                }
            }
            let _ = push_event(
                owner,
                NetEvent::TcpSent {
                    handle,
                    len: total_sent,
                },
            );
        }
    }

    fn send_icmp_echo(&mut self, owner: &'static str, target: [u8; 4], seq: u16, data: Vec<u8>) {
        let socket = self.sockets.get_mut::<icmp::Socket>(self.icmp);
        if !socket.can_send() {
            let _ = push_event(
                owner,
                NetEvent::Error {
                    msg: "icmp send blocked",
                },
            );
            return;
        }

        let req = Icmpv4Repr::EchoRequest {
            ident: ICMP_IDENT,
            seq_no: seq,
            data: &data,
        };
        let mut out = vec![0u8; req.buffer_len()];
        req.emit(&mut Icmpv4Packet::new_unchecked(&mut out), &ChecksumCapabilities::default());

        let [a, b, c, d] = target;
        let target = Ipv4Address::new(a, b, c, d);
        if socket.send_slice(&out, IpAddress::Ipv4(target)).is_ok() {
            if self.icmp_vnet_inflight.len() >= ICMP_VNET_MAX_INFLIGHT {
                self.icmp_vnet_inflight.remove(0);
            }
            self.icmp_vnet_inflight.push(IcmpInflight {
                owner,
                seq,
                sent_at: now(),
            });
        } else {
            let _ = push_event(
                owner,
                NetEvent::Error {
                    msg: "icmp send fail",
                },
            );
        }
    }

    fn send_icmp_echo_v6(
        &mut self,
        owner: &'static str,
        target: [u8; 16],
        seq: u16,
        data: Vec<u8>,
    ) {
        let socket = self.sockets.get_mut::<icmp::Socket>(self.icmp);
        if !socket.can_send() {
            let _ = push_event(
                owner,
                NetEvent::Error {
                    msg: "icmp6 send blocked",
                },
            );
            return;
        }

        let dst = Ipv6Address::from_octets(target);
        let req = Icmpv6Repr::EchoRequest {
            ident: ICMP_IDENT,
            seq_no: seq,
            data: &data,
        };
        let mut out = vec![0u8; req.buffer_len()];
        req.emit(
            &self.local_ipv6_ll,
            &dst,
            &mut Icmpv6Packet::new_unchecked(&mut out),
            &ChecksumCapabilities::default(),
        );

        if socket.send_slice(&out, IpAddress::Ipv6(dst)).is_ok() {
            if self.icmp_vnet_inflight.len() >= ICMP_VNET_MAX_INFLIGHT {
                self.icmp_vnet_inflight.remove(0);
            }
            self.icmp_vnet_inflight.push(IcmpInflight {
                owner,
                seq,
                sent_at: now(),
            });
        } else {
            let _ = push_event(
                owner,
                NetEvent::Error {
                    msg: "icmp6 send fail",
                },
            );
        }
    }

    fn prune_icmp_inflight(&mut self, timestamp: Instant) {
        let timeout = SmolDuration::from_millis(ICMP_VNET_TIMEOUT_MS as u64);
        self.icmp_vnet_inflight
            .retain(|p| timestamp < p.sent_at + timeout);
    }

    fn maybe_send_router_solicit(&mut self, timestamp: Instant) {
        if self.router_ipv6.is_some() || self.local_ipv6_global.is_some() {
            return;
        }

        if let Some(last) = self.rs_last_sent
            && timestamp < last + SmolDuration::from_millis(IPV6_RS_RETRY_MS as u64)
        {
            return;
        }

        // NDP requires IPv6 Hop Limit = 255. If we send RS via the ICMP socket,
        // the interface default hop limit (typically 64) may be used, and many
        // routers will drop the solicitation. Craft a full IPv6 packet and send
        // it via a raw ICMPv6 socket instead.
        let socket = self.sockets.get_mut::<raw::Socket>(self.raw_icmpv6);
        if !socket.can_send() {
            return;
        }

        let mac = crate::net::mac_address_at(self.device_index).unwrap_or([0, 0, 0, 0, 0, 1]);
        let dst = Ipv6Address::new(0xff02, 0, 0, 0, 0, 0, 0, 0x0002); // ff02::2 all-routers
        let rs = Icmpv6Repr::Ndisc(NdiscRepr::RouterSolicit {
            lladdr: Some(RawHardwareAddress::from_bytes(&mac)),
        });

        let ipv6_repr = Ipv6Repr {
            src_addr: self.local_ipv6_ll,
            dst_addr: dst,
            next_header: IpProtocol::Icmpv6,
            payload_len: rs.buffer_len(),
            hop_limit: 255,
        };
        let mut out = vec![0u8; ipv6_repr.buffer_len() + rs.buffer_len()];
        {
            let mut ip_pkt = Ipv6Packet::new_unchecked(&mut out);
            ipv6_repr.emit(&mut ip_pkt);
            rs.emit(
                &self.local_ipv6_ll,
                &dst,
                &mut Icmpv6Packet::new_unchecked(ip_pkt.payload_mut()),
                &ChecksumCapabilities::default(),
            );
        }

        if crate::logflag::NET_LOG_IPV6_RA {
            crate::log_info!(target: "net";
                "net: ipv6 rs dev={} src_ll={:02x}{:02x}:{:02x}{:02x}:... -> ff02::2\n",
                self.device_index,
                self.local_ipv6_ll.octets()[0],
                self.local_ipv6_ll.octets()[1],
                self.local_ipv6_ll.octets()[2],
                self.local_ipv6_ll.octets()[3],
            );
        }

        if socket.send_slice(&out).is_ok() {
            self.rs_last_sent = Some(timestamp);
        }
    }

    fn poll_router_advertisements(&mut self) {
        let mut scratch = POLL_SCRATCH_BUF.lock();

        loop {
            // Keep the raw socket borrow scoped to just the recv.
            let len = {
                let socket = self.sockets.get_mut::<raw::Socket>(self.raw_icmpv6);
                if !socket.can_recv() {
                    break;
                }
                match socket.recv_slice(&mut scratch[..]) {
                    Ok(len) => len,
                    Err(_) => break,
                }
            };

            let ipv6 = match Ipv6Packet::new_checked(&scratch[..len]) {
                Ok(p) => p,
                Err(_) => continue,
            };
            let ip_repr = match Ipv6Repr::parse(&ipv6) {
                Ok(r) => r,
                Err(_) => continue,
            };
            if ip_repr.next_header != IpProtocol::Icmpv6 {
                continue;
            }

            let icmp = match Icmpv6Packet::new_checked(ipv6.payload()) {
                Ok(p) => p,
                Err(_) => continue,
            };
            let icmp_repr = match Icmpv6Repr::parse(
                &ip_repr.src_addr,
                &ip_repr.dst_addr,
                &icmp,
                // We don't have access to Interface's internal checksum caps.
                // For RA bring-up, accept packets even if checksum offload/caps
                // are not modeled perfectly.
                &ChecksumCapabilities::ignored(),
            ) {
                Ok(r) => r,
                Err(_) => continue,
            };

            let Icmpv6Repr::Ndisc(NdiscRepr::RouterAdvert {
                router_lifetime,
                prefix_info,
                ..
            }) = icmp_repr
            else {
                continue;
            };

            self.ra_seen = true;
            self.maybe_update_ra_flags_from_icmpv6(ipv6.payload());

            // RDNSS (RFC 8106) is not surfaced by smoltcp's RA representation.
            // Parse the raw ICMPv6 bytes to extract IPv6 DNS servers so we can
            // resolve names on IPv6-only networks.
            self.maybe_update_ra_dns6_from_icmpv6(ipv6.payload());

            // smoltcp's RA representation only exposes a single Prefix Information option.
            // Many routers advertise multiple prefixes (e.g. a ULA and a global-unicast).
            // Prefer global-unicast for Internet connectivity.
            let raw_slaac_prefix = Self::pick_slaac_prefix_from_ra_icmpv6(ipv6.payload());

            if crate::logflag::NET_LOG_IPV6_RA {
                crate::log_info!(target: "net";
                    "net: ipv6 ra dev={} from={:02x}{:02x}:{:02x}{:02x}:... lifetime_ms={} prefix_present={}\n",
                    self.device_index,
                    ip_repr.src_addr.octets()[0],
                    ip_repr.src_addr.octets()[1],
                    ip_repr.src_addr.octets()[2],
                    ip_repr.src_addr.octets()[3],
                    router_lifetime.total_millis(),
                    prefix_info.is_some() as u8,
                );
            }

            if router_lifetime.total_millis() != 0 {
                let routes = self.iface.routes_mut();
                routes.remove_default_ipv6_route();
                let _ = routes.add_default_ipv6_route(ip_repr.src_addr);
                self.router_ipv6 = Some(ip_repr.src_addr);
            }

            let mut prefix_octets: Option<[u8; 16]> = raw_slaac_prefix;
            if prefix_octets.is_none() {
                let Some(prefix) = prefix_info else {
                    continue;
                };
                if prefix.prefix_len != 64 {
                    continue;
                }
                if !prefix.flags.contains(NdiscPrefixInfoFlags::ADDRCONF) {
                    continue;
                }
                prefix_octets = Some(prefix.prefix.octets());
            }

            let Some(prefix_octets) = prefix_octets else {
                continue;
            };

            let mac = crate::net::mac_address_at(self.device_index).unwrap_or([0, 0, 0, 0, 0, 1]);
            let iid = eui64_interface_id(mac);
            let mut addr = prefix_octets;
            addr[8..16].copy_from_slice(&iid);
            let candidate = Ipv6Address::from_octets(addr);

            // Routers may advertise multiple SLAAC prefixes (e.g. one global-unicast
            // and one ULA). Prefer global-unicast for Internet connectivity.
            let mut chosen = candidate;

            // Don't clobber a DHCPv6-leased address; treat SLAAC as best-effort.
            if !self.ipv6_global_is_dhcp {
                if let Some(existing) = self.local_ipv6_global {
                    if ipv6_is_global_unicast(existing) && ipv6_is_ula(candidate) {
                        chosen = existing;
                    } else if ipv6_is_global_unicast(candidate)
                        && (!ipv6_is_global_unicast(existing) || ipv6_is_ula(existing))
                    {
                        self.local_ipv6_global = Some(candidate);
                        chosen = candidate;
                    } else {
                        chosen = existing;
                    }
                } else {
                    self.local_ipv6_global = Some(candidate);
                    chosen = candidate;
                }

                // SLAAC produced (or maintained) a usable global address.
                crate::r::readiness::set(
                    crate::r::readiness::NET_ANY_CONFIGURED
                        | crate::r::readiness::NET_V6_CONFIGURED,
                );
            }

            // Preserve current IPv4 CIDR(s) while updating IPv6.
            let mut v4_addrs = Vec::new();
            for cidr in self.iface.ip_addrs().iter().copied() {
                if let IpAddress::Ipv4(_) = cidr.address() {
                    v4_addrs.push(cidr);
                }
            }

            let v6_ll =
                IpCidr::new(IpAddress::Ipv6(self.local_ipv6_ll), IPV6_LINK_LOCAL_PREFIX_LEN);
            let v6_global = IpCidr::new(IpAddress::Ipv6(chosen), 64);
            self.iface.update_ip_addrs(|addrs| {
                addrs.clear();
                let _ = addrs.push(v6_ll);
                if !self.ipv6_global_is_dhcp {
                    let _ = addrs.push(v6_global);
                }
                for c in v4_addrs.iter().copied() {
                    let _ = addrs.push(c);
                }
            });
        }
    }

    fn pick_slaac_prefix_from_ra_icmpv6(icmpv6_bytes: &[u8]) -> Option<[u8; 16]> {
        // ICMPv6 Router Advertisement layout:
        // - 4 bytes ICMPv6 header
        // - 12 bytes RA fixed header
        // - options...
        // Options are: type (1), len (1, units of 8 bytes), data...
        const ICMPV6_HDR: usize = 4;
        const RA_FIXED: usize = 12;
        const RA_OPT_PREFIX_INFO: u8 = 3;

        if icmpv6_bytes.len() < ICMPV6_HDR + RA_FIXED {
            return None;
        }
        // Only proceed if this is an RA packet.
        if icmpv6_bytes[0] != 134 {
            return None;
        }

        let mut idx = ICMPV6_HDR + RA_FIXED;
        let mut fallback: Option<[u8; 16]> = None;

        while idx + 2 <= icmpv6_bytes.len() {
            let opt_type = icmpv6_bytes[idx];
            let opt_len_units = icmpv6_bytes[idx + 1];
            if opt_len_units == 0 {
                break;
            }
            let opt_len = (opt_len_units as usize) * 8;
            if idx + opt_len > icmpv6_bytes.len() {
                break;
            }

            if opt_type == RA_OPT_PREFIX_INFO && opt_len >= 32 {
                // Prefix Information option (RFC 4861):
                // 0 type=3
                // 1 len=4 (32 bytes)
                // 2 prefix length
                // 3 flags (A=0x40 autonomous)
                // 16..32 prefix (16 bytes)
                let prefix_len = icmpv6_bytes[idx + 2];
                let flags = icmpv6_bytes[idx + 3];
                let autonomous = (flags & 0x40) != 0;
                if prefix_len == 64 && autonomous {
                    let mut p = [0u8; 16];
                    p.copy_from_slice(&icmpv6_bytes[idx + 16..idx + 32]);

                    // Prefer global-unicast (2000::/3) over ULA (fc00::/7).
                    if (p[0] & 0xE0) == 0x20 {
                        return Some(p);
                    }
                    if fallback.is_none() {
                        fallback = Some(p);
                    }
                }
            }

            idx += opt_len;
        }

        fallback
    }

    fn maybe_update_ra_flags_from_icmpv6(&mut self, icmpv6_bytes: &[u8]) {
        // Router Advertisement flags byte is at offset:
        // 4 bytes ICMPv6 header + 1 byte cur hop limit + 1 byte flags.
        const ICMPV6_HDR: usize = 4;
        if icmpv6_bytes.len() < ICMPV6_HDR + 2 {
            return;
        }
        if icmpv6_bytes[0] != 134 {
            return;
        }
        let flags = icmpv6_bytes[ICMPV6_HDR + 1];
        self.ra_managed = (flags & 0x80) != 0;
        self.ra_other = (flags & 0x40) != 0;

        if !self.ra_logged_flags {
            self.ra_logged_flags = true;
            crate::log_info!(target: "net";
                "net: ipv6 ra-flags dev={} M={} O={}\n",
                self.device_index,
                self.ra_managed as u8,
                self.ra_other as u8
            );
        }
    }

    fn maybe_update_ra_dns6_from_icmpv6(&mut self, icmpv6_bytes: &[u8]) {
        // ICMPv6 Router Advertisement layout:
        // - 4 bytes ICMPv6 header
        // - 12 bytes RA fixed header
        // - options...
        // Options are: type (1), len (1, units of 8 bytes), data...
        const ICMPV6_HDR: usize = 4;
        const RA_FIXED: usize = 12;
        const RA_OPT_RDNSS: u8 = 25;

        if icmpv6_bytes.len() < ICMPV6_HDR + RA_FIXED {
            return;
        }

        // Only proceed if this is an RA packet.
        if icmpv6_bytes[0] != 134 {
            return;
        }

        let mut idx = ICMPV6_HDR + RA_FIXED;
        let mut found = false;
        let mut out = [[0u8; 16]; RA_DNS6_MAX];
        let mut count: u8 = 0;

        while idx + 2 <= icmpv6_bytes.len() {
            let opt_type = icmpv6_bytes[idx];
            let opt_len_units = icmpv6_bytes[idx + 1];
            if opt_len_units == 0 {
                break;
            }
            let opt_len = (opt_len_units as usize) * 8;
            if idx + opt_len > icmpv6_bytes.len() {
                break;
            }

            if opt_type == RA_OPT_RDNSS {
                // RDNSS option:
                // - 0: type=25
                // - 1: length (>= 3)
                // - 2..4: reserved
                // - 4..8: lifetime (u32 seconds)
                // - 8..:  one or more 16-byte IPv6 addresses
                if opt_len >= 24 {
                    found = true;
                    let lifetime = u32::from_be_bytes([
                        icmpv6_bytes[idx + 4],
                        icmpv6_bytes[idx + 5],
                        icmpv6_bytes[idx + 6],
                        icmpv6_bytes[idx + 7],
                    ]);
                    if lifetime == 0 {
                        // Explicit expiration.
                        self.ra_dns6 = [[0u8; 16]; RA_DNS6_MAX];
                        self.ra_dns6_count = 0;
                        if self.device_index == crate::net::primary_device_index() {
                            *PRIMARY_RA_DNS6.lock() = ([[0u8; 16]; RA_DNS6_MAX], 0);
                        }
                        return;
                    }

                    let mut a = idx + 8;
                    while a + 16 <= idx + opt_len && (count as usize) < RA_DNS6_MAX {
                        out[count as usize].copy_from_slice(&icmpv6_bytes[a..a + 16]);
                        count = count.saturating_add(1);
                        a += 16;
                    }

                    // Keep parsing in case there are multiple RDNSS options; we
                    // cap the output anyway.
                }
            }

            idx += opt_len;
        }

        if found {
            self.ra_dns6 = out;
            self.ra_dns6_count = count;
            if crate::logflag::NET_LOG_IPV6_RA {
                crate::log_info!(target: "net";
                    "net: ipv6 rdnss dev={} count={}\n",
                    self.device_index,
                    self.ra_dns6_count
                );
                for i in 0..(self.ra_dns6_count as usize) {
                    let a = self.ra_dns6[i];
                    crate::log_info!(target: "net";
                        "net: ipv6 rdnss dev={} idx={} server={:02x}{:02x}:{:02x}{:02x}:...\n",
                        self.device_index,
                        i,
                        a[0],
                        a[1],
                        a[2],
                        a[3]
                    );
                }
            }
            if self.device_index == crate::net::primary_device_index() {
                *PRIMARY_RA_DNS6.lock() = (self.ra_dns6, self.ra_dns6_count);
            }
        }
    }

    fn dhcp6_tick(&mut self, timestamp: Instant) {
        self.dhcp6_poll_rx();

        // If the network indicates M=1 (managed), prefer a DHCPv6-leased address.
        // Don't let a best-effort SLAAC address suppress DHCPv6.
        let want_stateful_addr = self.ra_managed && !self.ipv6_global_is_dhcp;
        let have_any_v6_dns = self.ra_dns6_count != 0 || self.dhcp6_dns6_count != 0;
        let want_dns = if DHCP6_EAGER_DNS {
            !have_any_v6_dns
        } else {
            self.ra_other && self.dhcp6_dns6_count == 0
        };

        // If we only did an info-request earlier and later learn we need a lease,
        // allow the state machine to start again.
        if self.dhcp6_stage == Dhcp6Stage::Bound && want_stateful_addr {
            self.dhcp6_stage = Dhcp6Stage::Idle;
        }

        if self.dhcp6_stage == Dhcp6Stage::Idle {
            if let Some(until) = self.dhcp6_cooldown_until {
                if timestamp < until {
                    return;
                }
                self.dhcp6_cooldown_until = None;
            }
            if want_stateful_addr {
                crate::log_info!(target: "net"; "net: dhcp6 start dev={} reason=ra-managed (M=1)\n", self.device_index);
                self.dhcp6_start_solicit(timestamp);
            } else if want_dns {
                crate::log_info!(target: "net";
                    "net: dhcp6 start dev={} reason=dns-needed ra_other={} ra_dns6={} dhcp6_dns6={}\n",
                    self.device_index,
                    self.ra_other as u8,
                    self.ra_dns6_count,
                    self.dhcp6_dns6_count
                );
                self.dhcp6_start_info(timestamp);
            }
            return;
        }

        // Retry logic for in-flight exchanges.
        let Some(last) = self.dhcp6_last_sent else {
            return;
        };

        let base_ms: i64 = 1500;
        let backoff = (self.dhcp6_retries as i64 + 1).clamp(1, 8);
        let retry_ms = base_ms.saturating_mul(backoff);

        if timestamp < last + SmolDuration::from_millis(retry_ms as u64) {
            return;
        }

        if self.dhcp6_retries >= 6 {
            // If the network sets O/M but doesn't actually provide DHCPv6,
            // don't spam forever.
            crate::log_info!(target: "net";
                "net: dhcp6 cooldown dev={} stage={:?} ra_managed={} ra_other={}\n",
                self.device_index,
                self.dhcp6_stage,
                self.ra_managed as u8,
                self.ra_other as u8
            );
            self.dhcp6_cooldown_until = Some(timestamp + SmolDuration::from_secs(60));
            self.dhcp6_stage = Dhcp6Stage::Idle;
            self.dhcp6_last_sent = None;
            self.dhcp6_retries = 0;
            return;
        }

        match self.dhcp6_stage {
            Dhcp6Stage::Solicit => {
                self.dhcp6_send_solicit(timestamp);
            }
            Dhcp6Stage::Request => {
                self.dhcp6_send_request(timestamp);
            }
            Dhcp6Stage::Info => {
                self.dhcp6_send_info(timestamp);
            }
            _ => {}
        }
    }

    fn dhcp6_new_xid(&self) -> [u8; 3] {
        let r = crate::tyche::rdrand_u64().unwrap_or(0x6a09e667_f3bcc909);
        let mut xid = [
            (r & 0xFF) as u8,
            ((r >> 8) & 0xFF) as u8,
            ((r >> 16) & 0xFF) as u8,
        ];
        if xid == [0, 0, 0] {
            xid = [0x12, 0x34, 0x56];
        }
        xid
    }

    fn dhcp6_start_solicit(&mut self, timestamp: Instant) {
        self.dhcp6_stage = Dhcp6Stage::Solicit;
        self.dhcp6_retries = 0;
        self.dhcp6_server_id = None;
        self.dhcp6_server_addr = None;
        self.dhcp6_candidate_addr = None;
        self.dhcp6_xid = self.dhcp6_new_xid();
        self.dhcp6_send_solicit(timestamp);
    }

    fn dhcp6_send_solicit(&mut self, timestamp: Instant) {
        let duid = &self.dhcp6_duid;
        let msg = crate::net::dhcpv6::build_solicit(self.dhcp6_xid, duid, self.dhcp6_iaid, true);
        if self.dhcp6_send_udp(&msg) {
            if self.dhcp6_retries == 0 {
                crate::log_info!(target: "net";
                    "net: dhcp6 solicit dev={} xid={:02x}{:02x}{:02x}\n",
                    self.device_index,
                    self.dhcp6_xid[0],
                    self.dhcp6_xid[1],
                    self.dhcp6_xid[2]
                );
            }
            self.dhcp6_last_sent = Some(timestamp);
            self.dhcp6_retries = self.dhcp6_retries.saturating_add(1);
        }
    }

    fn dhcp6_start_info(&mut self, timestamp: Instant) {
        self.dhcp6_stage = Dhcp6Stage::Info;
        self.dhcp6_retries = 0;
        self.dhcp6_xid = self.dhcp6_new_xid();
        self.dhcp6_send_info(timestamp);
    }

    fn dhcp6_send_info(&mut self, timestamp: Instant) {
        let duid = &self.dhcp6_duid;
        let msg = crate::net::dhcpv6::build_info_request(self.dhcp6_xid, duid);
        if self.dhcp6_send_udp(&msg) {
            if self.dhcp6_retries == 0 {
                crate::log_info!(target: "net";
                    "net: dhcp6 info-request dev={} xid={:02x}{:02x}{:02x}\n",
                    self.device_index,
                    self.dhcp6_xid[0],
                    self.dhcp6_xid[1],
                    self.dhcp6_xid[2]
                );
            }
            self.dhcp6_last_sent = Some(timestamp);
            self.dhcp6_retries = self.dhcp6_retries.saturating_add(1);
        }
    }

    fn dhcp6_send_request(&mut self, timestamp: Instant) {
        let Some(server_id) = self.dhcp6_server_id.as_deref() else {
            self.dhcp6_stage = Dhcp6Stage::Idle;
            return;
        };

        if self.dhcp6_retries == 0 {
            self.dhcp6_xid = self.dhcp6_new_xid();
        }
        let duid = &self.dhcp6_duid;
        let requested = self.dhcp6_candidate_addr.map(|a| a.octets());
        let msg = crate::net::dhcpv6::build_request(
            self.dhcp6_xid,
            duid,
            server_id,
            self.dhcp6_iaid,
            requested,
            true,
        );
        if self.dhcp6_send_udp(&msg) {
            if self.dhcp6_retries == 0 {
                crate::log_info!(target: "net";
                    "net: dhcp6 request dev={} xid={:02x}{:02x}{:02x} unicast={}\n",
                    self.device_index,
                    self.dhcp6_xid[0],
                    self.dhcp6_xid[1],
                    self.dhcp6_xid[2],
                    self.dhcp6_server_addr.is_some() as u8
                );
            }
            self.dhcp6_last_sent = Some(timestamp);
            self.dhcp6_retries = self.dhcp6_retries.saturating_add(1);
        }
    }

    fn dhcp6_send_udp(&mut self, payload: &[u8]) -> bool {
        let socket = self.sockets.get_mut::<udp::Socket>(self.dhcp6_udp);
        if !socket.can_send() {
            return false;
        }

        // Solicit is multicast. Request is often accepted via multicast too, but
        // some servers are stricter and expect unicast to the server that sent
        // Advertise/Reply.
        let dst = if self.dhcp6_stage == Dhcp6Stage::Request {
            self.dhcp6_server_addr
                .unwrap_or_else(|| Ipv6Address::from_octets(crate::net::dhcpv6::ALL_SERVERS_MCAST))
        } else {
            Ipv6Address::from_octets(crate::net::dhcpv6::ALL_SERVERS_MCAST)
        };

        let ep = IpEndpoint::new(IpAddress::Ipv6(dst), crate::net::dhcpv6::SERVER_PORT);
        socket.send_slice(payload, ep).is_ok()
    }

    fn dhcp6_poll_rx(&mut self) {
        let mut bounce = [0u8; 1500];

        loop {
            // Keep the UDP socket borrow scoped to just the recv.
            let (len, meta) = {
                let socket = self.sockets.get_mut::<udp::Socket>(self.dhcp6_udp);
                match socket.recv_slice(&mut bounce) {
                    Ok(v) => v,
                    Err(_) => break,
                }
            };

            if len < 4 {
                continue;
            }
            if meta.endpoint.port != crate::net::dhcpv6::SERVER_PORT {
                continue;
            }
            let IpAddress::Ipv6(src) = meta.endpoint.addr else {
                continue;
            };

            let mut dns = [[0u8; 16]; DHCP6_DNS6_MAX];
            let Some(p) = crate::net::dhcpv6::parse(&bounce[..len], &mut dns) else {
                continue;
            };

            if self.dhcp6_rx_samples_left != 0 {
                self.dhcp6_rx_samples_left = self.dhcp6_rx_samples_left.saturating_sub(1);
                crate::log_info!(target: "net";
                    "net: dhcp6 rx dev={} stage={:?} src=[{}]:{} kind={:?} xid={:02x}{:02x}{:02x} sid_len={} cid_len={} ia={} dns={}\n",
                    self.device_index,
                    self.dhcp6_stage,
                    src,
                    meta.endpoint.port,
                    p.kind,
                    p.xid[0],
                    p.xid[1],
                    p.xid[2],
                    p.server_id.map(|s| s.len()).unwrap_or(0),
                    p.client_id.map(|s| s.len()).unwrap_or(0),
                    p.ia_addr.is_some() as u8,
                    p.dns_count
                );
            }

            // When we have an active exchange, only accept matching XID.
            if self.dhcp6_stage != Dhcp6Stage::Idle && p.xid != self.dhcp6_xid {
                if self.dhcp6_rx_samples_left != 0 {
                    crate::log_info!(target: "net";
                        "net: dhcp6 rx dev={} ignored reason=xid-mismatch expected={:02x}{:02x}{:02x}\n",
                        self.device_index,
                        self.dhcp6_xid[0],
                        self.dhcp6_xid[1],
                        self.dhcp6_xid[2]
                    );
                }
                continue;
            }
            // If a ClientID is present, require it matches our DUID.
            if let Some(cid) = p.client_id
                && cid != &self.dhcp6_duid[..]
            {
                if self.dhcp6_rx_samples_left != 0 {
                    crate::log_info!(target: "net";
                        "net: dhcp6 rx dev={} ignored reason=clientid-mismatch\n",
                        self.device_index
                    );
                }
                continue;
            }

            if p.dns_count != 0 {
                self.dhcp6_dns6 = dns;
                self.dhcp6_dns6_count = p.dns_count;
                if self.device_index == crate::net::primary_device_index() {
                    *PRIMARY_DHCP6_DNS6.lock() = (self.dhcp6_dns6, self.dhcp6_dns6_count);
                }

                crate::log_info!(target: "net";
                    "net: dhcp6 dns6 dev={} count={}\n",
                    self.device_index,
                    self.dhcp6_dns6_count
                );
            }

            match (self.dhcp6_stage, p.kind) {
                (Dhcp6Stage::Solicit, crate::net::dhcpv6::ParsedKind::Advertise) => {
                    self.dhcp6_server_addr = Some(src);
                    if let Some(sid) = p.server_id {
                        self.dhcp6_server_id = Some(sid.to_vec());
                    }
                    if let Some(a) = p.ia_addr {
                        self.dhcp6_candidate_addr = Some(Ipv6Address::from_octets(a));
                    }
                    if self.dhcp6_server_id.is_some() {
                        self.dhcp6_stage = Dhcp6Stage::Request;
                        self.dhcp6_retries = 0;
                        let ts = now();
                        self.dhcp6_send_request(ts);
                    } else {
                        crate::log_info!(target: "net";
                            "net: dhcp6 advertise dev={} missing server-id (ignoring)\n",
                            self.device_index
                        );
                    }
                }

                (Dhcp6Stage::Solicit, crate::net::dhcpv6::ParsedKind::Reply)
                | (Dhcp6Stage::Request, crate::net::dhcpv6::ParsedKind::Reply) => {
                    self.dhcp6_server_addr = Some(src);
                    if let Some(sid) = p.server_id {
                        self.dhcp6_server_id = Some(sid.to_vec());
                    }
                    if let Some(a) = p.ia_addr {
                        let addr = Ipv6Address::from_octets(a);
                        self.dhcp6_install_global_addr(addr);
                        self.dhcp6_stage = Dhcp6Stage::Bound;
                        self.dhcp6_last_sent = None;
                        self.dhcp6_retries = 0;
                    }
                }

                (Dhcp6Stage::Info, crate::net::dhcpv6::ParsedKind::Reply) => {
                    self.dhcp6_server_addr = Some(src);
                    self.dhcp6_stage = Dhcp6Stage::Idle;
                    self.dhcp6_last_sent = None;
                    self.dhcp6_retries = 0;
                }

                _ => {}
            }
        }
    }

    fn dhcp6_install_global_addr(&mut self, addr: Ipv6Address) {
        self.local_ipv6_global = Some(addr);
        self.ipv6_global_is_dhcp = true;

        // Mark IPv6 as configured. This is distinct from NET_V6_GATEWAY_REACHABLE.
        crate::r::readiness::set(
            crate::r::readiness::NET_ANY_CONFIGURED | crate::r::readiness::NET_V6_CONFIGURED,
        );

        // Preserve current IPv4 CIDR(s) while updating IPv6.
        let mut v4_addrs = Vec::new();
        for cidr in self.iface.ip_addrs().iter().copied() {
            if let IpAddress::Ipv4(_) = cidr.address() {
                v4_addrs.push(cidr);
            }
        }

        let v6_ll = IpCidr::new(IpAddress::Ipv6(self.local_ipv6_ll), IPV6_LINK_LOCAL_PREFIX_LEN);
        let v6_global = IpCidr::new(IpAddress::Ipv6(addr), 128);
        self.iface.update_ip_addrs(|addrs| {
            addrs.clear();
            let _ = addrs.push(v6_ll);
            let _ = addrs.push(v6_global);
            for c in v4_addrs.iter().copied() {
                let _ = addrs.push(c);
            }
        });

        crate::log_info!(target: "net"; "net: dhcp6 acquired dev={} addr6={}\n", self.device_index, addr);
    }

    fn handle_command(&mut self, owner: &'static str, cmd: NetCommand) {
        match cmd {
            NetCommand::OpenUdp { port } => match self.open_udp(owner, port) {
                Ok(handle) => {
                    let _ = push_event(
                        owner,
                        NetEvent::Opened {
                            handle,
                            kind: SocketKind::Udp,
                        },
                    );
                }
                Err(msg) => {
                    let _ = push_event(owner, NetEvent::Error { msg });
                }
            },
            NetCommand::OpenTcpListen { port } => match self.open_tcp(owner, port) {
                Ok(handle) => {
                    let _ = push_event(
                        owner,
                        NetEvent::Opened {
                            handle,
                            kind: SocketKind::Tcp,
                        },
                    );
                }
                Err(msg) => {
                    let _ = push_event(owner, NetEvent::Error { msg });
                }
            },
            NetCommand::OpenTcpConnect { remote } => {
                if is_ipv4_loopback(remote.addr) {
                    match self.open_loopback_tcp_connect(owner, remote) {
                        Ok((client_handle, server_handle, server_owner)) => {
                            let _ = push_event(
                                owner,
                                NetEvent::Opened {
                                    handle: client_handle,
                                    kind: SocketKind::Tcp,
                                },
                            );
                            let _ = push_event(
                                owner,
                                NetEvent::TcpEstablished {
                                    handle: client_handle,
                                    peer: Some(remote),
                                    peer6: None,
                                },
                            );
                            let _ = push_event(
                                server_owner,
                                NetEvent::TcpEstablished {
                                    handle: server_handle,
                                    peer: Some(NetEndpoint {
                                        addr: [127, 0, 0, 1],
                                        port: self.tcp_next_ephemeral.wrapping_sub(1),
                                    }),
                                    peer6: None,
                                },
                            );
                        }
                        Err(msg) => {
                            let _ = push_event(owner, NetEvent::Error { msg });
                        }
                    }
                } else {
                    match self.open_tcp_connect(owner, remote) {
                        Ok(handle) => {
                            if crate::logflag::NET_LOG_TCP_FLOW {
                                crate::log_info!(target: "net";
                                    "net: open-tcp cmd owner={} remote={}.{}.{}.{}:{} handle={}\n",
                                    owner,
                                    remote.addr[0],
                                    remote.addr[1],
                                    remote.addr[2],
                                    remote.addr[3],
                                    remote.port,
                                    handle.0
                                );
                            }
                            let _ = push_event(
                                owner,
                                NetEvent::Opened {
                                    handle,
                                    kind: SocketKind::Tcp,
                                },
                            );
                        }
                        Err(msg) => {
                            let _ = push_event(owner, NetEvent::Error { msg });
                        }
                    }
                }
            }
            NetCommand::OpenTcpConnectV6 { remote } => {
                match self.open_tcp_connect_v6(owner, remote) {
                    Ok(handle) => {
                        let _ = push_event(
                            owner,
                            NetEvent::Opened {
                                handle,
                                kind: SocketKind::Tcp,
                            },
                        );
                    }
                    Err(msg) => {
                        let _ = push_event(owner, NetEvent::Error { msg });
                    }
                }
            }
            NetCommand::SendUdp {
                handle,
                remote,
                data,
            } => {
                if !self.link_up() {
                    let _ = push_event(owner, NetEvent::Error { msg: "link down" });
                    return;
                }
                if let Some(rec) = self.find_record(handle) {
                    if rec.kind != SocketKind::Udp {
                        let _ = push_event(owner, NetEvent::Error { msg: "not udp" });
                        return;
                    }
                    let socket_handle = rec.socket;
                    let endpoint = IpEndpoint::new(
                        IpAddress::Ipv4(Ipv4Address::from_octets(remote.addr)),
                        remote.port,
                    );
                    let socket = self.sockets.get_mut::<udp::Socket>(socket_handle);
                    if socket.send_slice(&data, endpoint).is_err() {
                        let _ = push_event(
                            owner,
                            NetEvent::Error {
                                msg: "udp send fail",
                            },
                        );
                    }
                } else {
                    let _ = push_event(owner, NetEvent::Error { msg: "bad handle" });
                }
            }
            NetCommand::SendUdpV6 {
                handle,
                remote,
                data,
            } => {
                if !self.link_up() {
                    let _ = push_event(owner, NetEvent::Error { msg: "link down" });
                    return;
                }
                if let Some(rec) = self.find_record(handle) {
                    if rec.kind != SocketKind::Udp {
                        let _ = push_event(owner, NetEvent::Error { msg: "not udp" });
                        return;
                    }
                    let socket_handle = rec.socket;
                    let endpoint = IpEndpoint::new(
                        IpAddress::Ipv6(Ipv6Address::from_octets(remote.addr)),
                        remote.port,
                    );
                    let socket = self.sockets.get_mut::<udp::Socket>(socket_handle);
                    if socket.send_slice(&data, endpoint).is_err() {
                        let _ = push_event(
                            owner,
                            NetEvent::Error {
                                msg: "udp6 send fail",
                            },
                        );
                    }
                } else {
                    let _ = push_event(owner, NetEvent::Error { msg: "bad handle" });
                }
            }
            NetCommand::SendTcp { handle, data } => {
                if let Some(result) = self.send_loopback_tcp(handle, data.clone()) {
                    if result.is_err() {
                        let _ = push_event(owner, NetEvent::Error { msg: "not tcp" });
                    }
                    return;
                }

                if !self.link_up() {
                    let _ = push_event(owner, NetEvent::Error { msg: "link down" });
                    return;
                }

                if let Some(idx) = self.records.iter().position(|r| r.handle == handle) {
                    if self.records[idx].kind != SocketKind::Tcp {
                        let _ = push_event(owner, NetEvent::Error { msg: "not tcp" });
                        return;
                    }
                    if crate::logflag::NET_LOG_TCP_FLOW && data.starts_with(b"GET ") {
                        crate::log_info!(target: "net";
                            "net: sendtcp cmd owner={} handle={} bytes={}\n",
                            owner,
                            handle.0,
                            data.len()
                        );
                    }
                    // Don't drop on backpressure; queue and flush when the socket becomes writable.
                    // This is especially important for TLS handshakes (ClientHello) right after connect.
                    self.records[idx].tcp_tx.extend(data);
                    self.flush_tcp_tx(idx);
                } else {
                    let _ = push_event(owner, NetEvent::Closed { handle });
                }
            }
            NetCommand::IcmpEcho { target, seq, data } => {
                self.send_icmp_echo(owner, target, seq, data);
            }
            NetCommand::IcmpEchoV6 { target, seq, data } => {
                self.send_icmp_echo_v6(owner, target, seq, data);
            }
            NetCommand::Close { handle } => {
                if !self.close_loopback_tcp(handle) {
                    self.remove_record(handle);
                    let _ = push_event(owner, NetEvent::Closed { handle });
                }
            }
        }
    }

    fn poll_udp(&mut self, idx: usize) {
        if self.records.get(idx).map(|r| r.kind) != Some(SocketKind::Udp) {
            return;
        }
        let owner = self.records[idx].owner;
        let handle = self.records[idx].handle;
        let socket = self
            .sockets
            .get_mut::<udp::Socket>(self.records[idx].socket);
        let mut bounce = [0u8; 1500];
        while let Ok((len, meta)) = socket.recv_slice(&mut bounce) {
            let endpoint = meta.endpoint;
            let data = bounce[..len].to_vec();

            match endpoint.addr {
                IpAddress::Ipv4(addr) => {
                    let addr = addr.octets();
                    let ep = NetEndpoint {
                        addr,
                        port: endpoint.port,
                    };
                    let _ = push_event(
                        owner,
                        NetEvent::UdpPacket {
                            handle,
                            from: ep,
                            data,
                        },
                    );
                }
                IpAddress::Ipv6(addr) => {
                    let ep = NetEndpointV6 {
                        addr: addr.octets(),
                        port: endpoint.port,
                    };
                    let _ = push_event(
                        owner,
                        NetEvent::UdpPacketV6 {
                            handle,
                            from: ep,
                            data,
                        },
                    );
                }
            }
        }
    }

    fn poll_tcp(&mut self, idx: usize) -> bool {
        if self.records.get(idx).map(|r| r.kind) != Some(SocketKind::Tcp) {
            return false;
        }
        if self.records[idx].tcp_loopback_peer.is_some() {
            return false;
        }

        let (owner, handle, socket_handle) = {
            let rec = &self.records[idx];
            (rec.owner, rec.handle, rec.socket)
        };

        let mut should_remove = false;
        let mut state: tcp::State;
        let mut rx_bytes_this_poll = 0usize;
        let mut rx_events_this_poll = 0usize;
        let mut rx_event_drops_this_poll = 0usize;

        {
            let socket = self.sockets.get_mut::<tcp::Socket>(socket_handle);
            state = socket.state();

            if state == tcp::State::Established {
                if let Some(endpoint) = socket.local_endpoint() {
                    self.records[idx].tcp_local_port = Some(endpoint.port);
                }
                match socket.remote_endpoint().map(|endpoint| endpoint.addr) {
                    Some(IpAddress::Ipv4(addr)) => {
                        let port = socket
                            .remote_endpoint()
                            .map(|endpoint| endpoint.port)
                            .unwrap_or(0);
                        self.records[idx].tcp_remote_v4 = Some(NetEndpoint {
                            addr: addr.octets(),
                            port,
                        });
                        self.records[idx].tcp_remote_v6 = None;
                    }
                    Some(IpAddress::Ipv6(addr)) => {
                        let port = socket
                            .remote_endpoint()
                            .map(|endpoint| endpoint.port)
                            .unwrap_or(0);
                        self.records[idx].tcp_remote_v4 = None;
                        self.records[idx].tcp_remote_v6 = Some(NetEndpointV6 {
                            addr: addr.octets(),
                            port,
                        });
                    }
                    None => {}
                }
            }

            let last = self.records[idx].last_tcp_state;
            if last != Some(state) {
                self.records[idx].last_tcp_state = Some(state);
                if crate::logflag::NET_LOG_TCP_FLOW
                    || (crate::logflag::NET_LOG_TCP_CONNECT_STATES && self.records[idx].tcp_connect)
                {
                    log_tcp_connect_record_state("net: tcp state", &self.records[idx], state);
                }
            }

            let mut rx_backpressured = false;

            if socket.is_active() && socket.may_recv() {
                while socket.can_recv() {
                    let result = socket.recv(|buf| {
                        let len = core::cmp::min(buf.len(), v::vnet::MAX_MSG);
                        if len == 0 {
                            return (0, Some(0usize));
                        }

                        let data = buf[..len].to_vec();
                        if push_event(owner, NetEvent::TcpData { handle, data }) {
                            (len, Some(len))
                        } else {
                            (0, None)
                        }
                    });

                    match result {
                        Ok(Some(len)) if len > 0 => {
                            rx_bytes_this_poll = rx_bytes_this_poll.saturating_add(len);
                            rx_events_this_poll = rx_events_this_poll.saturating_add(1);
                        }
                        Ok(Some(_)) => break,
                        Ok(None) => {
                            rx_event_drops_this_poll = rx_event_drops_this_poll.saturating_add(1);
                            rx_backpressured = true;
                            break;
                        }
                        Err(_) => break,
                    }
                }
            }

            // If the peer has closed its send side, smoltcp enters CLOSE-WAIT and will
            // remain there until the local side closes too. Many of our higher-level
            // protocols (HTTP demos, TLS demo) use "Connection: close" and expect to
            // observe a close event without needing an explicit `Close` command.
            //
            // Convert CLOSE-WAIT into an orderly local close so we eventually emit
            // `NetEvent::Closed`.
            if socket.state() == tcp::State::CloseWait
                && !rx_backpressured
                && !socket.can_recv()
                && self.records[idx].tcp_tx.is_empty()
            {
                if crate::logflag::NET_LOG_TCP_FLOW {
                    crate::log_info!(target: "net";
                        "net: tcp closewait owner={} handle={} rx_bytes_this_poll={} rx_events={} rx_drops={}\n",
                        owner,
                        handle.0,
                        rx_bytes_this_poll,
                        rx_events_this_poll,
                        rx_event_drops_this_poll
                    );
                }
                socket.close();
                state = socket.state();
            }

            if !socket.is_open() {
                should_remove = true;
            }
        }

        // Flush any queued outbound bytes opportunistically (after dropping the previous socket borrow).
        self.flush_tcp_tx(idx);

        if state == tcp::State::Established && !self.records[idx].established {
            if crate::logflag::NET_LOG_TCP_FLOW {
                crate::log_info!(target: "net"; "net: tcp established branch owner={} handle={}\n", owner, handle.0);
            }
            self.records[idx].established = true;
            let ok = push_event(
                owner,
                NetEvent::TcpEstablished {
                    handle,
                    peer: self.records[idx].tcp_remote_v4,
                    peer6: self.records[idx].tcp_remote_v6,
                },
            );
            if crate::logflag::NET_LOG_TCP_FLOW
                || (crate::logflag::NET_LOG_TCP_CONNECT_STATES && self.records[idx].tcp_connect)
            {
                log_tcp_connect_record_state(
                    "net: tcp established event",
                    &self.records[idx],
                    state,
                );
                crate::log_info!(target: "net";
                    "net: tcp established queued owner={} handle={} queued={}\n",
                    owner,
                    handle.0,
                    ok
                );
            }
        }

        if should_remove {
            let _ = push_event(owner, NetEvent::Closed { handle });
            self.remove_record(handle);
            return true;
        }

        false
    }

    fn poll_sockets(&mut self) {
        let mut idx = 0;
        while idx < self.records.len() {
            let removed = match self.records[idx].kind {
                SocketKind::Udp => {
                    self.poll_udp(idx);
                    false
                }
                SocketKind::Tcp => self.poll_tcp(idx),
            };
            if !removed {
                idx += 1;
            }
        }
    }

    fn tick(&mut self) -> bool {
        let timestamp = now();
        let mut work_done = false;

        {
            let device_index = self.device_index;
            let NetService {
                iface,
                rx_buffer,
                tx_buffer,
                sockets,
                ..
            } = self;

            let mut device = AdapterDeviceAt {
                index: device_index,
                rx_buffer,
                tx_buffer,
            };

            // Always poll at least once
            let _ = iface.poll(timestamp, &mut device, sockets);

            // Drain packets with a higher limit.
            // If we hit the limit, we return true to hint "call me again immediately".
            let limit = 128;
            let mut count = 0;
            loop {
                if count >= limit {
                    work_done = true;
                    break;
                }
                if device.rx_buffer.is_empty() && crate::net::rx_pending_at(device_index) == 0 {
                    break;
                }
                match iface.poll(timestamp, &mut device, sockets) {
                    PollResult::None => {}
                    _ => {
                        work_done = true;
                        count += 1;
                    }
                }
            }

            // Flush buffered TX packets
            if !self.tx_buffer.is_empty() {
                crate::net::transmit_batch_at(device_index, self.tx_buffer.drain(..));
            }
        }

        // IPv6 bring-up (RA-based): process any Router Advertisements first,
        // then periodically solicit one while we have no router/global address.
        self.poll_router_advertisements();
        self.maybe_send_router_solicit(timestamp);

        // DHCPv6 (stateful address and/or stateless DNS). Runs alongside RA.
        self.dhcp6_tick(timestamp);

        // Also drain the TX queue for sockets (esp. TCP)
        if let Some(idx) = self.records.iter().position(|r| r.kind == SocketKind::Tcp) {
            self.flush_tcp_tx(idx);
        }

        // Internal netbench (no vnet/event copies).
        work_done |= self.internal_netbench_tick(timestamp);

        let dhcp_event = self.sockets.get_mut::<dhcpv4::Socket>(self.dhcp).poll();
        match dhcp_event {
            None => {}
            Some(dhcpv4::Event::Configured(config)) => {
                let ip = config.address.address();
                let ip_o = ip.octets();
                let prefix_len = config.address.prefix_len();
                let had_lease = self.dhcp_has_lease;
                self.dhcp_has_lease = true;

                let ipv6_tag = if self.local_ipv6_global.is_some() {
                    "ra"
                } else {
                    "none"
                };

                self.local_ipv4 = Some(config.address.address());
                self.router_ipv4 = config.router;

                if let Some(router) = config.router {
                    let r = router.octets();
                    crate::log_info!(target: "net";
                        "net: dhcp {} dev={} ipv4={}.{}.{}.{} gw={}.{}.{}.{} ipv6={}\n",
                        if had_lease { "renewed" } else { "acquired" },
                        self.device_index,
                        ip_o[0],
                        ip_o[1],
                        ip_o[2],
                        ip_o[3],
                        r[0],
                        r[1],
                        r[2],
                        r[3],
                        ipv6_tag
                    );
                } else {
                    crate::log_info!(target: "net";
                        "net: dhcp {} dev={} ipv4={}.{}.{}.{} gw=none ipv6={}\n",
                        if had_lease { "renewed" } else { "acquired" },
                        self.device_index,
                        ip_o[0],
                        ip_o[1],
                        ip_o[2],
                        ip_o[3],
                        ipv6_tag
                    );
                }

                // Keep IPv6 CIDRs while updating IPv4.
                let mut v6_addrs = Vec::new();
                for cidr in self.iface.ip_addrs().iter().copied() {
                    if let IpAddress::Ipv6(_) = cidr.address() {
                        v6_addrs.push(cidr);
                    }
                }
                self.iface.update_ip_addrs(|addrs| {
                    addrs.clear();
                    for c in v6_addrs.iter().copied() {
                        let _ = addrs.push(c);
                    }
                    let _ = addrs.push(IpCidr::Ipv4(config.address));
                });

                if crate::logflag::NET_LOG_DHCP_VERBOSE {
                    crate::log_info!(target: "net";
                        "net: dhcp apply dev={} ipv4_cidr={}.{}.{}.{} /{} iface_addrs={}\n",
                        self.device_index,
                        ip_o[0],
                        ip_o[1],
                        ip_o[2],
                        ip_o[3],
                        prefix_len,
                        self.iface.ip_addrs().len()
                    );
                    for (i, cidr) in self.iface.ip_addrs().iter().copied().enumerate() {
                        match cidr.address() {
                            IpAddress::Ipv4(v4) => {
                                let o = v4.octets();
                                crate::log_info!(target: "net";
                                    "net: dhcp iface_addr dev={} idx={} v4={}.{}.{}.{} /{}\n",
                                    self.device_index,
                                    i,
                                    o[0],
                                    o[1],
                                    o[2],
                                    o[3],
                                    cidr.prefix_len()
                                );
                            }
                            IpAddress::Ipv6(v6) => {
                                let o = v6.octets();
                                crate::log_info!(target: "net";
                                    "net: dhcp iface_addr dev={} idx={} v6={:02x}{:02x}:{:02x}{:02x}:... /{}\n",
                                    self.device_index,
                                    i,
                                    o[0],
                                    o[1],
                                    o[2],
                                    o[3],
                                    cidr.prefix_len()
                                );
                            }
                        }
                    }
                }

                let routes = self.iface.routes_mut();
                routes.remove_default_ipv4_route();
                if let Some(router) = config.router {
                    match routes.add_default_ipv4_route(router) {
                        Ok(_route) => {
                            let r = router.octets();
                            crate::log_info!(target: "net";
                                "net: route v4 default dev={} gw={}.{}.{}.{} ok\n",
                                self.device_index,
                                r[0],
                                r[1],
                                r[2],
                                r[3]
                            );
                        }
                        Err(e) => {
                            let r = router.octets();
                            crate::log_info!(target: "net";
                                "net: route v4 default dev={} gw={}.{}.{}.{} err={:?}\n",
                                self.device_index,
                                r[0],
                                r[1],
                                r[2],
                                r[3],
                                e
                            );
                        }
                    }
                }

                self.dhcp_dns = [[0u8; 4]; DHCP_DNS_MAX];
                self.dhcp_dns_count = 0;
                for (i, s) in config.dns_servers.iter().take(DHCP_DNS_MAX).enumerate() {
                    self.dhcp_dns[i] = s.octets();
                    self.dhcp_dns_count = (i as u8) + 1;
                }
                crate::log_info!(target: "net";
                    "net: dhcp dns-count dev={} count={}\n",
                    self.device_index,
                    self.dhcp_dns_count
                );
                for i in 0..(self.dhcp_dns_count as usize) {
                    let d = self.dhcp_dns[i];
                    crate::log_info!(target: "net";
                        "net: dhcp dns dev={} idx={} server={}.{}.{}.{}\n",
                        self.device_index,
                        i,
                        d[0],
                        d[1],
                        d[2],
                        d[3]
                    );
                }

                if self.device_index == crate::net::primary_device_index() {
                    *PRIMARY_DHCP_DNS.lock() = (self.dhcp_dns, self.dhcp_dns_count);
                }

                // Network is configured (at least IPv4). This is intentionally weaker than
                // NET_GATEWAY_REACHABLE, which depends on an ICMP-to-router probe.
                crate::r::readiness::set(
                    crate::r::readiness::NET_ANY_CONFIGURED
                        | crate::r::readiness::NET_V4_CONFIGURED,
                );
            }
            Some(dhcpv4::Event::Deconfigured) => {
                if self.dhcp_has_lease {
                    self.dhcp_has_lease = false;
                    let (fallback_ip, fallback_gw) = apply_static_fallback_ipv4(
                        &mut self.iface,
                        self.device_index,
                        self.local_ipv6_ll,
                    );
                    self.local_ipv4 = Some(fallback_ip);
                    self.router_ipv4 = Some(fallback_gw);
                    self.dhcp_dns = [[0u8; 4]; DHCP_DNS_MAX];
                    self.dhcp_dns_count = 0;
                    let ip_o = fallback_ip.octets();
                    let gw_o = fallback_gw.octets();
                    crate::log_info!(target: "net";
                        "net: dhcp lost dev={} fallback ipv4={}.{}.{}.{} gw={}.{}.{}.{} ipv6={}\n",
                        self.device_index,
                        ip_o[0],
                        ip_o[1],
                        ip_o[2],
                        ip_o[3],
                        gw_o[0],
                        gw_o[1],
                        gw_o[2],
                        gw_o[3],
                        if self.local_ipv6_global.is_some() {
                            "ra"
                        } else {
                            "none"
                        }
                    );

                    if self.device_index == crate::net::primary_device_index() {
                        *PRIMARY_DHCP_DNS.lock() = ([[0u8; 4]; DHCP_DNS_MAX], 0);
                    }
                }
            }
        }

        self.poll_icmp();
        self.prune_icmp_inflight(timestamp);

        // After polling, try a deterministic ICMP ping to prove RX/TX + IP stack.
        // Do this per-NIC, but only when that NIC is link-up to avoid noisy
        // retries on unplugged interfaces.
        let link_up = crate::net::link_state_at(self.device_index)
            .map(|ls| ls.up)
            .unwrap_or(false);
        if link_up {
            self.maybe_send_icmp_ping(timestamp);
            self.maybe_send_icmp_ping_v6(timestamp);
        }

        work_done
    }

    fn maybe_send_icmp_ping(&mut self, timestamp: Instant) {
        if self.icmp_ping_pongs >= 1 {
            return;
        }

        let Some(target) = self.router_ipv4 else {
            return;
        };

        if let Some((_seq_no, sent_at)) = self.icmp_ping_inflight {
            if timestamp >= sent_at + SmolDuration::from_millis(2000) {
                self.icmp_ping_inflight = None;
            } else {
                return;
            }
        }

        // Re-send at most once per second until we get a reply.
        if let Some(last) = self.icmp_ping_last_sent
            && timestamp < last + SmolDuration::from_millis(1000)
        {
            return;
        }

        let socket = self.sockets.get_mut::<icmp::Socket>(self.icmp);
        if !socket.can_send() {
            return;
        }

        self.icmp_ping_seq = self.icmp_ping_seq.wrapping_add(1) & 0x7FFF;
        if self.icmp_ping_seq == 0 {
            self.icmp_ping_seq = 1;
        }
        let seq_no = self.icmp_ping_seq;
        let payload: &[u8] = b"TRUEOS-ping";
        let req = Icmpv4Repr::EchoRequest {
            ident: ICMP_IDENT,
            seq_no,
            data: payload,
        };
        let mut out = vec![0u8; req.buffer_len()];
        req.emit(&mut Icmpv4Packet::new_unchecked(&mut out), &ChecksumCapabilities::default());

        if socket.send_slice(&out, IpAddress::Ipv4(target)).is_ok() {
            let [a, b, c, d] = target.octets();
            if let Some(src) = self.local_ipv4 {
                let [sa, sb, sc, sd] = src.octets();
                crate::log_info!(target: "net";
                    "net: icmp ping dev={} src={}.{}.{}.{} seq={} -> {}.{}.{}.{}\n",
                    self.device_index,
                    sa,
                    sb,
                    sc,
                    sd,
                    seq_no,
                    a,
                    b,
                    c,
                    d
                );
            } else {
                crate::log_info!(target: "net";
                    "net: icmp ping dev={} seq={} -> {}.{}.{}.{}\n",
                    self.device_index,
                    seq_no,
                    a,
                    b,
                    c,
                    d
                );
            }
            self.icmp_ping_last_sent = Some(timestamp);
            self.icmp_ping_inflight = Some((seq_no, timestamp));
        }
    }

    fn maybe_send_icmp_ping_v6(&mut self, timestamp: Instant) {
        if self.icmp6_ping_pongs >= 1 {
            return;
        }

        // Need a router to test.
        let Some(target) = self.router_ipv6 else {
            return;
        };

        // Router is link-local; use link-local source for correct scope.

        if let Some((_seq_no, sent_at)) = self.icmp6_ping_inflight {
            if timestamp >= sent_at + SmolDuration::from_millis(2500) {
                self.icmp6_ping_inflight = None;
            } else {
                return;
            }
        }

        // Re-send at most once per second until we get a reply.
        if let Some(last) = self.icmp6_ping_last_sent
            && timestamp < last + SmolDuration::from_millis(1000)
        {
            return;
        }

        let socket = self.sockets.get_mut::<icmp::Socket>(self.icmp);
        if !socket.can_send() {
            return;
        }

        self.icmp6_ping_seq = self.icmp6_ping_seq.wrapping_add(1) & 0x7FFF;
        if self.icmp6_ping_seq == 0 {
            self.icmp6_ping_seq = 1;
        }
        let seq_no = self.icmp6_ping_seq;

        let payload: &[u8] = b"TRUEOS-ping6";
        let req = Icmpv6Repr::EchoRequest {
            ident: ICMP_IDENT,
            seq_no,
            data: payload,
        };

        let src = self.local_ipv6_ll;
        let dst = target;
        let mut out = vec![0u8; req.buffer_len()];
        req.emit(
            &src,
            &dst,
            &mut Icmpv6Packet::new_unchecked(&mut out),
            &ChecksumCapabilities::default(),
        );

        if socket.send_slice(&out, IpAddress::Ipv6(dst)).is_ok() {
            let o = dst.octets();
            crate::log_info!(target: "net";
                "net: icmp6 ping dev={} seq={} -> {:02x}{:02x}:{:02x}{:02x}:...\n",
                self.device_index,
                seq_no,
                o[0],
                o[1],
                o[2],
                o[3]
            );
            self.icmp6_ping_last_sent = Some(timestamp);
            self.icmp6_ping_inflight = Some((seq_no, timestamp));
        }
    }

    fn poll_icmp(&mut self) {
        let mut buf = [0u8; 2048];
        let socket = self.sockets.get_mut::<icmp::Socket>(self.icmp);
        while socket.can_recv() {
            let Ok((len, from)) = socket.recv_slice(&mut buf) else {
                break;
            };
            match from {
                IpAddress::Ipv4(src_v4) => {
                    let Ok(pkt) = Icmpv4Packet::new_checked(&buf[..len]) else {
                        continue;
                    };
                    let Ok(repr) = Icmpv4Repr::parse(&pkt, &ChecksumCapabilities::ignored()) else {
                        continue;
                    };

                    match repr {
                        Icmpv4Repr::EchoRequest {
                            ident,
                            seq_no,
                            data,
                        } => {
                            // Only reply to our bound ident; smoltcp already filters, but keep it explicit.
                            if ident != ICMP_IDENT {
                                continue;
                            }

                            let reply = Icmpv4Repr::EchoReply {
                                ident,
                                seq_no,
                                data,
                            };
                            let mut out = vec![0u8; reply.buffer_len()];
                            reply.emit(
                                &mut Icmpv4Packet::new_unchecked(&mut out),
                                &ChecksumCapabilities::default(),
                            );
                            let _ = socket.send_slice(&out, IpAddress::Ipv4(src_v4));
                        }
                        Icmpv4Repr::EchoReply { ident, seq_no, .. } => {
                            if ident != ICMP_IDENT {
                                continue;
                            }

                            if let Some(pos) =
                                self.icmp_vnet_inflight.iter().position(|p| p.seq == seq_no)
                            {
                                let inflight = self.icmp_vnet_inflight.remove(pos);
                                let rtt = now() - inflight.sent_at;
                                let _ = push_event(
                                    inflight.owner,
                                    NetEvent::IcmpReply {
                                        from: src_v4.octets(),
                                        seq: seq_no,
                                        rtt_ms: rtt.total_millis() as u32,
                                        data: buf[..len].to_vec(),
                                    },
                                );
                                continue;
                            }

                            if let Some((inflight_seq, sent_at)) = self.icmp_ping_inflight
                                && inflight_seq == seq_no
                            {
                                let rtt = now() - sent_at;
                                if let Some(local) = self.local_ipv4 {
                                    let [la, lb, lc, ld] = local.octets();
                                    crate::log_info!(target: "net";
                                        "net: icmp pong dev={} local={}.{}.{}.{} seq={} rtt={}ms\n",
                                        self.device_index,
                                        la,
                                        lb,
                                        lc,
                                        ld,
                                        seq_no,
                                        rtt.total_millis()
                                    );
                                } else {
                                    crate::log_info!(target: "net";
                                        "net: icmp pong dev={} seq={} rtt={}ms\n",
                                        self.device_index,
                                        seq_no,
                                        rtt.total_millis()
                                    );
                                }
                                self.icmp_ping_inflight = None;

                                // Consider the network reachable on the first successful pong.
                                if self.icmp_ping_pongs == 0 {
                                    self.icmp_ping_pongs = 1;
                                    crate::log_info!(target: "net";
                                        "net: icmp ok dev={} (gateway reachable)\n",
                                        self.device_index
                                    );
                                    crate::r::readiness::set(
                                        crate::r::readiness::NET_GATEWAY_REACHABLE
                                            | crate::r::readiness::NET_V4_GATEWAY_REACHABLE,
                                    );
                                }
                            }
                        }
                        _ => {}
                    }
                }
                IpAddress::Ipv6(src_v6) => {
                    // Buffer layout for ICMPv6 echo:
                    // [0]=type [1]=code [2..4]=cksum [4..6]=ident [6..8]=seq.
                    if len < 8 {
                        continue;
                    }
                    let icmp_type = buf[0];
                    if icmp_type != 129 {
                        continue;
                    }
                    let ident = u16::from_be_bytes([buf[4], buf[5]]);
                    if ident != ICMP_IDENT {
                        continue;
                    }
                    let seq_no = u16::from_be_bytes([buf[6], buf[7]]);

                    // Gateway reachability probe (icmp6 ping).
                    if let Some((inflight_seq, sent_at)) = self.icmp6_ping_inflight {
                        let payload = buf.get(8..len).unwrap_or(&[]);
                        if inflight_seq == seq_no && payload.starts_with(b"TRUEOS-ping6") {
                            let rtt = now() - sent_at;
                            let o = src_v6.octets();
                            crate::log_info!(target: "net";
                                "net: icmp6 pong dev={} seq={} rtt={}ms from={:02x}{:02x}:{:02x}{:02x}:...\n",
                                self.device_index,
                                seq_no,
                                rtt.total_millis(),
                                o[0],
                                o[1],
                                o[2],
                                o[3]
                            );
                            self.icmp6_ping_inflight = None;

                            if self.icmp6_ping_pongs == 0 {
                                self.icmp6_ping_pongs = 1;
                                crate::log_info!(target: "net";
                                    "net: icmp6 ok dev={} (v6 gateway reachable)\n",
                                    self.device_index
                                );
                                crate::r::readiness::set(
                                    crate::r::readiness::NET_GATEWAY_REACHABLE
                                        | crate::r::readiness::NET_V6_GATEWAY_REACHABLE,
                                );
                            }
                            continue;
                        }
                    }

                    if let Some(pos) = self.icmp_vnet_inflight.iter().position(|p| p.seq == seq_no)
                    {
                        let inflight = self.icmp_vnet_inflight.remove(pos);
                        let rtt = now() - inflight.sent_at;
                        let _ = push_event(
                            inflight.owner,
                            NetEvent::IcmpReplyV6 {
                                from: src_v6.octets(),
                                seq: seq_no,
                                rtt_ms: rtt.total_millis() as u32,
                                data: buf[..len].to_vec(),
                            },
                        );
                    }
                }
            }
        }
    }
}

fn now() -> Instant {
    use embassy_time_driver::TICK_HZ;
    let ticks = embassy_time_driver::now();
    let ms = (ticks as u128 * 1000u128 / (TICK_HZ as u128)) as i64;
    Instant::from_millis(ms)
}

fn eui64_interface_id(mac: [u8; 6]) -> [u8; 8] {
    // RFC 4291: interface identifier from MAC via EUI-64 (flip the U/L bit).
    [
        mac[0] ^ 0x02,
        mac[1],
        mac[2],
        0xff,
        0xfe,
        mac[3],
        mac[4],
        mac[5],
    ]
}

fn ipv6_link_local_from_mac(mac: [u8; 6]) -> Ipv6Address {
    let iid = eui64_interface_id(mac);
    let mut octets = [0u8; 16];
    octets[..8].copy_from_slice(&IPV6_LINK_LOCAL_PREFIX);
    octets[8..16].copy_from_slice(&iid);
    Ipv6Address::from_octets(octets)
}

#[inline]
fn ipv6_is_global_unicast(ip: Ipv6Address) -> bool {
    // 2000::/3
    (ip.octets()[0] & 0xE0) == 0x20
}

#[inline]
fn ipv6_is_ula(ip: Ipv6Address) -> bool {
    // fc00::/7
    (ip.octets()[0] & 0xFE) == 0xFC
}

fn fallback_ipv4_for_device(device_index: usize) -> Ipv4Address {
    let mut octets = STATIC_FALLBACK_BASE_IPV4;
    let base = octets[3] as usize;
    let host = (base + (device_index % 64)).clamp(2, 254) as u8;
    octets[3] = host;
    Ipv4Address::new(octets[0], octets[1], octets[2], octets[3])
}

fn apply_static_fallback_ipv4(
    iface: &mut Interface,
    device_index: usize,
    ipv6_ll: Ipv6Address,
) -> (Ipv4Address, Ipv4Address) {
    let ip = fallback_ipv4_for_device(device_index);
    let gw = Ipv4Address::new(
        STATIC_FALLBACK_GATEWAY[0],
        STATIC_FALLBACK_GATEWAY[1],
        STATIC_FALLBACK_GATEWAY[2],
        STATIC_FALLBACK_GATEWAY[3],
    );

    iface.update_ip_addrs(|addrs| {
        addrs.clear();
        let _ = addrs.push(IpCidr::new(IpAddress::Ipv6(ipv6_ll), IPV6_LINK_LOCAL_PREFIX_LEN));
        let _ = addrs.push(IpCidr::new(IpAddress::Ipv4(ip), STATIC_FALLBACK_PREFIX_LEN));
    });
    let routes = iface.routes_mut();
    routes.remove_default_ipv4_route();
    let _ = routes.add_default_ipv4_route(gw);
    (ip, gw)
}

// Reusable scratch buffer for polling sockets to avoid large stack zeroing
static POLL_SCRATCH_BUF: spin::Mutex<[u8; 8192]> = spin::Mutex::new([0u8; 8192]);

// Shared net service state so both the async service task and synchronous `time::block_on`
// hooks can drive the stack without diverging socket/interface state.
static NET_SERVICES: spin::Mutex<Option<Vec<&'static spin::Mutex<NetService>>>> =
    spin::Mutex::new(None);

fn owner_device_index(owner: &str) -> Option<usize> {
    crate::net::device_index_from_owner(owner)
}

fn ensure_services(count: usize) {
    let mut guard = NET_SERVICES.lock();
    let needs_init = guard.as_ref().map(|v| v.len() != count).unwrap_or(true);
    if needs_init {
        *guard = Some(
            (0..count)
                .map(|index| -> &'static spin::Mutex<NetService> {
                    Box::leak(Box::new(spin::Mutex::new(NetService::new(index))))
                })
                .collect(),
        );
    }
}

fn service_for_device(device_index: usize) -> Option<&'static spin::Mutex<NetService>> {
    let count = crate::net::device_count();
    if count == 0 {
        return None;
    }

    ensure_services(count);

    let guard = NET_SERVICES.lock();
    let services = guard.as_ref()?;
    services.get(device_index).copied()
}

fn service_tick_once(device_index: usize) -> bool {
    let Some(service) = service_for_device(device_index) else {
        return false;
    };

    let mut svc = service.lock();

    let mut busy = false;
    if svc.tick() {
        busy = true;
    }

    for _ in 0..MAX_DRAIN_PER_LOOP {
        let Some((owner, cmd)) = pop_command_for_device(device_index) else {
            break;
        };
        busy = true;
        svc.handle_command(owner, cmd);
    }

    svc.poll_sockets();

    busy
}

/// Synchronously advance the primary network service once.
///
/// This is used by std/Mio/Tokio compatibility shims that may be running inside
/// a current-thread runtime during early boot. In that window the async
/// `net_service_task` can be delayed by other bringup work, but socket clients
/// still need submitted VNet commands to be drained promptly.
pub fn service_tick_primary_once() -> bool {
    let idx = crate::net::primary_device_index();
    service_tick_once(idx)
}

/// Per-NIC RX poll loop.
///
/// This decouples device RX polling from the smoltcp/service loop so that
/// adding NICs doesn't make a single task heavier.
#[task(pool_size = MAX_NET_DEVICES)]
pub async fn net_poll_task(index: usize) {
    async move {
        loop {
            if crate::net::poll_at(index) {
                Timer::after(EmbassyDuration::from_micros(0)).await;
            } else {
                Timer::after(EmbassyDuration::from_micros(NET_POLL_SLEEP_US)).await;
            }
        }
    }
    .await;
}

#[task(pool_size = MAX_NET_DEVICES)]
pub async fn net_service_task(index: usize) {
    async move {
        let count = crate::net::device_count();
        if count == 0 || index >= count {
            crate::log_info!(target: "net"; "net: service disabled (nic={})\n", index);
            return;
        }

        ensure_services(count);

        loop {
            if service_tick_once(index) {
                Timer::after(EmbassyDuration::from_micros(0)).await;
            } else {
                Timer::after(EmbassyDuration::from_micros(NET_SERVICE_SLEEP_US)).await;
            }
        }
    }
    .await;
}
