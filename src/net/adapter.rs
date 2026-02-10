use alloc::{boxed::Box, collections::VecDeque, vec, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use embassy_executor::task;
use embassy_time::{Duration as EmbassyDuration, Timer};
use smoltcp::iface::{Config as IfaceConfig, Interface, SocketHandle, SocketSet};
use smoltcp::phy::ChecksumCapabilities;
use smoltcp::phy::{Device, DeviceCapabilities, Medium, PacketMeta, RxToken, TxToken};
use smoltcp::socket::{dhcpv4, icmp, tcp, udp};
use smoltcp::time::{Duration as SmolDuration, Instant};
use smoltcp::wire::{
    EthernetAddress, HardwareAddress, IpAddress, IpCidr, IpEndpoint, Ipv4Address,
    Icmpv4Packet, Icmpv4Repr,
};

use crate::log;

// Boot can involve several concurrent TCP/TLS connections (DoH/DoT, fetches,
// net-shell, etc.). 8 was too tight and caused transient "no sockets available"
// failures under load.
const MAX_SOCKETS: usize = 128;
const MAX_DRAIN_PER_LOOP: usize = 128;
const TCP_RX_BUF_BYTES: usize = 1024 * 1024;
const TCP_TX_BUF_BYTES: usize = 1024 * 1024;
const ICMP_IDENT: u16 = 0x1234;
const ICMP_VNET_MAX_INFLIGHT: usize = 32;
const ICMP_VNET_TIMEOUT_MS: i64 = 2000;
const NET_POLL_SLEEP_US: u64 = 100;
const NET_SERVICE_SLEEP_US: u64 = 100;
const NET_LOG_RX_TAP: bool = false;
const NET_LOG_TX_TAP: bool = false;
const NET_LOG_DHCP_VERBOSE: bool = false;

const DHCP_DNS_MAX: usize = 4;
pub const MAX_NET_DEVICES: usize = 8;

// Best-effort primary DNS server snapshot (used by v-layer defaults).
// Kept intentionally simple: we record what DHCP reports for the active primary NIC.
static PRIMARY_DHCP_DNS: spin::Mutex<([[u8; 4]; DHCP_DNS_MAX], u8)> =
    spin::Mutex::new(([[0u8; 4]; DHCP_DNS_MAX], 0));
static PRIMARY_DEVICE_LOCKED: AtomicBool = AtomicBool::new(false);

pub fn primary_dhcp_dns_snapshot() -> ([[u8; 4]; DHCP_DNS_MAX], u8) {
    *PRIMARY_DHCP_DNS.lock()
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
static NET_TX_DROPPED_AT: [AtomicU64; MAX_NET_DEVICES] = [
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

// DHCP bring-up interoperability toggles.
//
// Some DHCP servers/routers only reply properly when the client requests
// broadcast replies (BOOTP flags bit15). For bring-up, we force this bit.
const DHCP_FORCE_BROADCAST_FLAG: bool = true;
// As a compatibility experiment, force UDP checksum to 0 (IPv4 allows this).
// Useful if we're accidentally producing an invalid checksum due to offload or
// descriptor behavior.
const DHCP_FORCE_UDP_CHECKSUM_ZERO: bool = true;

fn is_dhcp_client_udp(buf: &[u8]) -> bool {
    if buf.len() < 14 {
        return false;
    }

    let mut et = u16::from_be_bytes([buf[12], buf[13]]);
    let mut l2_off = 14usize;
    if et == 0x8100 {
        if buf.len() < 18 {
            return false;
        }
        et = u16::from_be_bytes([buf[16], buf[17]]);
        l2_off = 18;
    }
    if et != 0x0800 || buf.len() < l2_off + 20 {
        return false;
    }

    let ver_ihl = buf[l2_off];
    if (ver_ihl >> 4) != 4 {
        return false;
    }
    let ihl = ((ver_ihl & 0x0f) as usize) * 4;
    if ihl < 20 || buf.len() < l2_off + ihl + 8 {
        return false;
    }
    if buf[l2_off + 9] != 17 {
        return false;
    }
    let udp_off = l2_off + ihl;
    let sport = u16::from_be_bytes([buf[udp_off], buf[udp_off + 1]]);
    let dport = u16::from_be_bytes([buf[udp_off + 2], buf[udp_off + 3]]);
    sport == 68 && dport == 67
}

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
            add16(
                &mut sum,
                u16::from_be_bytes([udp_bytes[udp_bytes.len() - 1], 0]),
            );
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
    SendUdp {
        handle: NetHandle,
        remote: NetEndpoint,
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
    TcpEstablished {
        handle: NetHandle,
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

fn pop_command() -> Option<(&'static str, NetCommand)> {
    let guard = APP_QUEUES.lock();
    for entry in guard.iter() {
        if let Some(cmd) = entry.cmds.pop() {
            return Some((entry.name, cmd));
        }
    }
    None
}

fn push_event(target: &'static str, event: NetEvent) -> bool {
    let guard = APP_QUEUES.lock();
    if let Some(entry) = guard.iter().find(|e| e.name == target) {
        entry.events.push(event).is_ok()
    } else {
        false
    }
}

struct AdapterDeviceAt {
    index: usize,
}

impl Device for AdapterDeviceAt {
    type RxToken<'a>
        = AdapterRxToken
    where
        Self: 'a;
    type TxToken<'a>
        = AdapterTxTokenAt
    where
        Self: 'a;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        crate::net::pop_rx_packet_at(self.index).map(|packet| {
            let new_total = NET_RX_FRAMES.fetch_add(1, Ordering::Relaxed) + 1;
            let new_dev_total = NET_RX_FRAMES_AT
                .get(self.index)
                .map(|c| c.fetch_add(1, Ordering::Relaxed) + 1)
                .unwrap_or(new_total);
            if (new_total & NET_FRAME_LOG_MASK) == 0 {
                log!("net: rx frames={}\n", new_total);
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
                if et == 0x8100 && packet.len() >= 18 {
                    et = u16::from_be_bytes([packet[16], packet[17]]);
                    l2_off = 18;
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
                                        op = packet[dhcp_off + 0];
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
                                    if NET_LOG_DHCP_VERBOSE && (chaddr_match != 0 || offer_count == 1) {
                                        crate::log!(
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
            if NET_LOG_RX_TAP && new_dev_total <= 8 {
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

                    crate::log!(
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
                    crate::log!("net: rx-tap dev={} len={} (short)\n", self.index, packet.len());
                }
            }
            (
                AdapterRxToken { buffer: packet },
                AdapterTxTokenAt { index: self.index },
            )
        })
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        Some(AdapterTxTokenAt { index: self.index })
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

struct AdapterTxTokenAt {
    index: usize,
}

impl TxToken for AdapterTxTokenAt {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut buf = vec![0u8; len];
        let result = f(&mut buf[..]);

        // DHCP fixup must run for every outgoing DHCP client packet, not only
        // the initial tx-tap frames.
        dhcp_fixup_broadcast_and_udp_checksum(&mut buf[..]);

        let new_total = NET_TX_FRAMES.fetch_add(1, Ordering::Relaxed) + 1;
        let new_dev_total = NET_TX_FRAMES_AT
            .get(self.index)
            .map(|c| c.fetch_add(1, Ordering::Relaxed) + 1)
            .unwrap_or(new_total);

        // TX tap: log only the first few frames per NIC so we can verify
        // DHCP is actually being emitted on the wire.
        if NET_LOG_TX_TAP && new_dev_total <= 8 {
            if buf.len() >= 14 {
                let dst = &buf[0..6];
                let src = &buf[6..12];
                let mut et = u16::from_be_bytes([buf[12], buf[13]]);
                let mut l2_off = 14usize;
                if et == 0x8100 && buf.len() >= 18 {
                    et = u16::from_be_bytes([buf[16], buf[17]]);
                    l2_off = 18;
                }

                crate::log!(
                    "net: tx-tap dev={} len={} et=0x{:04x} dst={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} src={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}\n",
                    self.index,
                    buf.len(),
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
                    src[5]
                );

                if et == 0x0800 && buf.len() >= l2_off + 20 {
                    let ver_ihl = buf[l2_off];
                    let ihl = ((ver_ihl & 0x0f) as usize) * 4;
                    if (ver_ihl >> 4) == 4 && buf.len() >= l2_off + ihl + 8 {
                        let proto = buf[l2_off + 9];
                        if proto == 17 {
                            let udp_off = l2_off + ihl;
                            let sport = u16::from_be_bytes([buf[udp_off], buf[udp_off + 1]]);
                            let dport = u16::from_be_bytes([buf[udp_off + 2], buf[udp_off + 3]]);
                            if NET_LOG_DHCP_VERBOSE && sport == 68 && dport == 67 {
                                crate::log!(
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
                                let ip_tot_len = u16::from_be_bytes([buf[l2_off + 2], buf[l2_off + 3]]);
                                let ip_hdr_csum = u16::from_be_bytes([buf[l2_off + 10], buf[l2_off + 11]]);
                                let udp_len = u16::from_be_bytes([buf[udp_off + 4], buf[udp_off + 5]]);
                                let udp_csum = u16::from_be_bytes([buf[udp_off + 6], buf[udp_off + 7]]);

                                // Verify IPv4 header checksum.
                                let mut sum: u32 = 0;
                                let mut i = 0usize;
                                while i + 1 < ihl {
                                    if i == 10 {
                                        i += 2;
                                        continue;
                                    }
                                    let w = u16::from_be_bytes([
                                        buf[l2_off + i],
                                        buf[l2_off + i + 1],
                                    ]) as u32;
                                    sum = sum.wrapping_add(w);
                                    i += 2;
                                }
                                while (sum >> 16) != 0 {
                                    sum = (sum & 0xFFFF) + (sum >> 16);
                                }
                                let ip_hdr_csum_calc = !(sum as u16);

                                crate::log!(
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
                                    let op = buf[dhcp_off + 0];
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

                                    crate::log!(
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
                                    crate::log!(
                                        "net: dhcp-tx dev={} dhcp payload too short (len={})\n",
                                        self.index,
                                        buf.len().saturating_sub(dhcp_off)
                                    );
                                }
                            }
                        }
                    }
                }
            } else {
                crate::log!("net: tx-tap dev={} len={} (short)\n", self.index, buf.len());
            }
        }
        if crate::net::transmit_packet_at(self.index, &buf[..]).is_err() {
            let dropped = NET_TX_DROPPED.fetch_add(1, Ordering::Relaxed) + 1;
            let _ = NET_TX_DROPPED_AT
                .get(self.index)
                .map(|c| c.fetch_add(1, Ordering::Relaxed) + 1);

            // DHCP is low-rate but time-sensitive; if we drop it, say so.
            if NET_LOG_DHCP_VERBOSE && is_dhcp_client_udp(&buf[..]) {
                crate::log!(
                    "net: dhcp-tx-drop dev={} dropped_total={} len={}\n",
                    self.index,
                    dropped,
                    buf.len()
                );
            }
            if (dropped & NET_FRAME_LOG_MASK) == 0 {
                log!(
                    "net: tx busy (drop) frames={} dropped={} last_len={}\n",
                    new_total,
                    dropped,
                    len
                );
            }
        } else if (new_total & NET_FRAME_LOG_MASK) == 0 {
            log!(
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
    established: bool,
    last_tcp_state: Option<tcp::State>,
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
    records: Vec<SocketRecord>,
    next_handle: AtomicU32,
    icmp: SocketHandle,

    dhcp: SocketHandle,
    local_ipv4: Option<Ipv4Address>,
    router_ipv4: Option<Ipv4Address>,
    dhcp_dns: [[u8; 4]; DHCP_DNS_MAX],
    dhcp_dns_count: u8,

    // Minimal ICMP reachability probe (ping gateway).
    icmp_ping_seq: u16,
    icmp_ping_inflight: Option<(u16, Instant)>,
    icmp_ping_last_sent: Option<Instant>,
    icmp_ping_pongs: u8,
    icmp_vnet_inflight: Vec<IcmpInflight>,

    tcp_next_ephemeral: u16,
}

impl NetService {
    fn new(device_index: usize) -> Self {
        let mac = crate::net::mac_address_at(device_index).unwrap_or([0, 0, 0, 0, 0, 1]);
        let hw_addr = HardwareAddress::Ethernet(EthernetAddress(mac));

        let mut cfg = IfaceConfig::new(hw_addr);
        cfg.random_seed = crate::rng::rdrand_u64().unwrap_or(0x9E37_79B9);
        let mut device = AdapterDeviceAt { index: device_index };
        let mut iface = Interface::new(cfg, &mut device, now());
        // IPv4 is configured via DHCPv4.
        iface.update_ip_addrs(|addrs| addrs.clear());
        iface.routes_mut().remove_default_ipv4_route();

        let rx_meta = vec![icmp::PacketMetadata::EMPTY; 8];
        let rx_buf = vec![0u8; 2048];
        let tx_meta = vec![icmp::PacketMetadata::EMPTY; 8];
        let tx_buf = vec![0u8; 2048];
        let rx = icmp::PacketBuffer::new(rx_meta, rx_buf);
        let tx = icmp::PacketBuffer::new(tx_meta, tx_buf);
        let mut icmp_socket = icmp::Socket::new(rx, tx);
        let _ = icmp_socket.bind(icmp::Endpoint::Ident(ICMP_IDENT));

        let mut sockets = SocketSet::new(Vec::new());
        let icmp = sockets.add(icmp_socket);

        let dhcp_socket = dhcpv4::Socket::new();
        let dhcp = sockets.add(dhcp_socket);

        Self {
            device_index,
            iface,
            sockets,
            records: Vec::new(),
            next_handle: AtomicU32::new(1),
            icmp,

            dhcp,
            local_ipv4: None,
            router_ipv4: None,
            dhcp_dns: [[0u8; 4]; DHCP_DNS_MAX],
            dhcp_dns_count: 0,

            icmp_ping_seq: 0,
            icmp_ping_inflight: None,
            icmp_ping_last_sent: None,
            icmp_ping_pongs: 0,
            icmp_vnet_inflight: Vec::new(),

            tcp_next_ephemeral: 49152,
        }
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

    fn open_udp(&mut self, owner: &'static str, port: u16) -> Result<NetHandle, &'static str> {
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
            established: false,
            last_tcp_state: None,
        });
        Ok(handle)
    }

    fn open_tcp(&mut self, owner: &'static str, port: u16) -> Result<NetHandle, &'static str> {
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
            established: false,
            last_tcp_state: Some(initial_state),
        });
        Ok(handle)
    }

    fn open_tcp_connect(
        &mut self,
        owner: &'static str,
        remote: NetEndpoint,
    ) -> Result<NetHandle, &'static str> {
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

        let local = IpEndpoint::new(
            IpAddress::Ipv4(local_ip),
            local_port,
        );
        let remote_addr = remote.addr;
        let remote_port = remote.port;
        let remote = IpEndpoint::new(
            IpAddress::Ipv4(Ipv4Address::from_octets(remote_addr)),
            remote_port,
        );

        let local_octets = local_ip.octets();
        crate::log!(
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

        socket
            .connect(self.iface.context(), remote, local)
            .map_err(|_| "connect failed")?;

        let initial_state = socket.state();

        let handle = self.alloc_handle();
        let sh = self.sockets.add(socket);
        self.records.push(SocketRecord {
            owner,
            handle,
            kind: SocketKind::Tcp,
            socket: sh,
            tcp_tx: VecDeque::new(),
            established: false,
            last_tcp_state: Some(initial_state),
        });
        Ok(handle)
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
                    let _ = push_event(owner, NetEvent::Error { msg: "tcp send fail" });
                    break;
                }
            }

            if !(socket.can_send() && socket.may_send()) {
                break;
            }
        }

        if total_sent != 0 {
            let _ = push_event(owner, NetEvent::TcpSent { handle, len: total_sent });
        }
    }

    fn send_icmp_echo(&mut self, owner: &'static str, target: [u8; 4], seq: u16, data: Vec<u8>) {
        let socket = self.sockets.get_mut::<icmp::Socket>(self.icmp);
        if !socket.can_send() {
            let _ = push_event(owner, NetEvent::Error { msg: "icmp send blocked" });
            return;
        }

        let req = Icmpv4Repr::EchoRequest {
            ident: ICMP_IDENT,
            seq_no: seq,
            data: &data,
        };
        let mut out = vec![0u8; req.buffer_len()];
        req.emit(
            &mut Icmpv4Packet::new_unchecked(&mut out),
            &ChecksumCapabilities::default(),
        );

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
            let _ = push_event(owner, NetEvent::Error { msg: "icmp send fail" });
        }
    }

    fn prune_icmp_inflight(&mut self, timestamp: Instant) {
        let timeout = SmolDuration::from_millis(ICMP_VNET_TIMEOUT_MS as u64);
        self.icmp_vnet_inflight
            .retain(|p| timestamp < p.sent_at + timeout);
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
                match self.open_tcp_connect(owner, remote) {
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
            NetCommand::SendTcp { handle, data } => {
                if let Some(idx) = self.records.iter().position(|r| r.handle == handle) {
                    if self.records[idx].kind != SocketKind::Tcp {
                        let _ = push_event(owner, NetEvent::Error { msg: "not tcp" });
                        return;
                    }
                    // Don't drop on backpressure; queue and flush when the socket becomes writable.
                    // This is especially important for TLS handshakes (ClientHello) right after connect.
                    self.records[idx].tcp_tx.extend(data);
                    self.flush_tcp_tx(idx);
                } else {
                    let _ = push_event(owner, NetEvent::Error { msg: "bad handle" });
                }
            }
            NetCommand::IcmpEcho { target, seq, data } => {
                self.send_icmp_echo(owner, target, seq, data);
            }
            NetCommand::Close { handle } => {
                self.remove_record(handle);
                let _ = push_event(owner, NetEvent::Closed { handle });
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
            let IpAddress::Ipv4(addr) = endpoint.addr;
            let addr = addr.octets();
            let ep = NetEndpoint {
                addr,
                port: endpoint.port,
            };
            let data = bounce[..len].to_vec();
            let _ = push_event(
                owner,
                NetEvent::UdpPacket {
                    handle,
                    from: ep,
                    data,
                },
            );
        }
    }

    fn poll_tcp(&mut self, idx: usize) -> bool {
        if self.records.get(idx).map(|r| r.kind) != Some(SocketKind::Tcp) {
            return false;
        }

        let (owner, handle, socket_handle) = {
            let rec = &self.records[idx];
            (rec.owner, rec.handle, rec.socket)
        };

        let mut should_remove = false;
        let mut state: tcp::State;

        {
            let socket = self.sockets.get_mut::<tcp::Socket>(socket_handle);
            state = socket.state();

            let last = self.records[idx].last_tcp_state;
            if last != Some(state) {
                self.records[idx].last_tcp_state = Some(state);
                crate::log!(
                    "net: tcp state owner={} handle={} state={:?}\n",
                    owner,
                    handle.0,
                    state
                );
            }

            if socket.is_active() && socket.may_recv() {
                // Match vnet MAX_MSG (8KiB) to avoid large stack buffers per poll.
                let mut buf = [0u8; 8192];
                while let Ok(len) = socket.recv_slice(&mut buf) {
                    if len == 0 {
                        break;
                    }
                    let data = buf[..len].to_vec();
                    let _ = push_event(owner, NetEvent::TcpData { handle, data });
                }
            }

            // If the peer has closed its send side, smoltcp enters CLOSE-WAIT and will
            // remain there until the local side closes too. Many of our higher-level
            // protocols (HTTP demos, TLS demo) use "Connection: close" and expect to
            // observe a close event without needing an explicit `Close` command.
            //
            // Convert CLOSE-WAIT into an orderly local close so we eventually emit
            // `NetEvent::Closed`.
            if socket.state() == tcp::State::CloseWait {
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
            crate::log!(
                "net: tcp established branch owner={} handle={}\n",
                owner,
                handle.0
            );
            self.records[idx].established = true;
            let ok = push_event(owner, NetEvent::TcpEstablished { handle });
            crate::log!(
                "net: tcp established event owner={} handle={} queued={}\n",
                owner,
                handle.0,
                ok
            );
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

    fn tick(&mut self) {
        let timestamp = now();
        let mut device = AdapterDeviceAt {
            index: self.device_index,
        };
        let _ = self.iface.poll(timestamp, &mut device, &mut self.sockets);

        let dhcp_event = self
            .sockets
            .get_mut::<dhcpv4::Socket>(self.dhcp)
            .poll();
        match dhcp_event {
            None => {}
            Some(dhcpv4::Event::Configured(config)) => {
                let ip = config.address.address();
                let ip_o = ip.octets();
                crate::log!(
                    "net: dhcp configured dev={} ip={}.{}.{}.{}\n",
                    self.device_index,
                    ip_o[0],
                    ip_o[1],
                    ip_o[2],
                    ip_o[3]
                );

                self.local_ipv4 = Some(config.address.address());
                self.router_ipv4 = config.router;

                self.iface.update_ip_addrs(|addrs| {
                    addrs.clear();
                    let _ = addrs.push(IpCidr::Ipv4(config.address));
                });

                let routes = self.iface.routes_mut();
                routes.remove_default_ipv4_route();
                if let Some(router) = config.router {
                    let r = router.octets();
                    crate::log!(
                        "net: dhcp router dev={} gw={}.{}.{}.{}\n",
                        self.device_index,
                        r[0],
                        r[1],
                        r[2],
                        r[3]
                    );
                    let _ = routes.add_default_ipv4_route(router);
                } else {
                    crate::log!(
                        "net: dhcp router dev={} gw=none (readiness will not trip)\n",
                        self.device_index
                    );
                }

                self.dhcp_dns = [[0u8; 4]; DHCP_DNS_MAX];
                self.dhcp_dns_count = 0;
                for (i, s) in config.dns_servers.iter().take(DHCP_DNS_MAX).enumerate() {
                    self.dhcp_dns[i] = s.octets();
                    self.dhcp_dns_count = (i as u8) + 1;
                }

                if PRIMARY_DEVICE_LOCKED
                    .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
                {
                    crate::net::set_primary_device_index(self.device_index);
                }

                if self.device_index == crate::net::primary_device_index() {
                    *PRIMARY_DHCP_DNS.lock() = (self.dhcp_dns, self.dhcp_dns_count);
                }
            }
            Some(dhcpv4::Event::Deconfigured) => {
                crate::log!("net: dhcp deconfigured dev={}\n", self.device_index);
                self.local_ipv4 = None;
                self.router_ipv4 = None;
                self.dhcp_dns = [[0u8; 4]; DHCP_DNS_MAX];
                self.dhcp_dns_count = 0;

                self.iface.update_ip_addrs(|addrs| addrs.clear());
                self.iface.routes_mut().remove_default_ipv4_route();

                if self.device_index == crate::net::primary_device_index() {
                    *PRIMARY_DHCP_DNS.lock() = ([[0u8; 4]; DHCP_DNS_MAX], 0);
                    PRIMARY_DEVICE_LOCKED.store(false, Ordering::SeqCst);
                }
            }
        }

        self.poll_icmp();
        self.prune_icmp_inflight(timestamp);

        // After polling, try a deterministic ICMP ping to prove RX/TX + IP stack.
        self.maybe_send_icmp_ping(timestamp);
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
        if let Some(last) = self.icmp_ping_last_sent {
            if timestamp < last + SmolDuration::from_millis(1000) {
                return;
            }
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
        req.emit(
            &mut Icmpv4Packet::new_unchecked(&mut out),
            &ChecksumCapabilities::default(),
        );

        if socket.send_slice(&out, IpAddress::Ipv4(target)).is_ok() {
            let [a, b, c, d] = target.octets();
            crate::log!(
                "net: icmp ping dev={} seq={} -> {}.{}.{}.{}\n",
                self.device_index,
                seq_no,
                a,
                b,
                c,
                d
            );
            self.icmp_ping_last_sent = Some(timestamp);
            self.icmp_ping_inflight = Some((seq_no, timestamp));
        }
    }

    fn poll_icmp(&mut self) {
        let mut buf = [0u8; 2048];
        let socket = self.sockets.get_mut::<icmp::Socket>(self.icmp);
        while socket.can_recv() {
            let Ok((len, from)) = socket.recv_slice(&mut buf) else { break };
            let Ok(pkt) = Icmpv4Packet::new_checked(&buf[..len]) else { continue };
            let Ok(repr) = Icmpv4Repr::parse(&pkt, &ChecksumCapabilities::ignored()) else { continue };

            match repr {
                Icmpv4Repr::EchoRequest { ident, seq_no, data } => {
                    // Only reply to our bound ident; smoltcp already filters, but keep it explicit.
                    if ident != ICMP_IDENT {
                        continue;
                    }

                    let reply = Icmpv4Repr::EchoReply { ident, seq_no, data };
                    let mut out = vec![0u8; reply.buffer_len()];
                    reply.emit(
                        &mut Icmpv4Packet::new_unchecked(&mut out),
                        &ChecksumCapabilities::default(),
                    );
                    let _ = socket.send_slice(&out, from);
                }
                Icmpv4Repr::EchoReply { ident, seq_no, .. } => {
                    if ident != ICMP_IDENT {
                        continue;
                    }

                    if let Some(pos) = self.icmp_vnet_inflight.iter().position(|p| p.seq == seq_no) {
                        let inflight = self.icmp_vnet_inflight.remove(pos);
                        let rtt = now() - inflight.sent_at;
                        if let IpAddress::Ipv4(addr) = from {
                            let _ = push_event(
                                inflight.owner,
                                NetEvent::IcmpReply {
                                    from: addr.octets(),
                                    seq: seq_no,
                                    rtt_ms: rtt.total_millis() as u32,
                                    data: buf[..len].to_vec(),
                                },
                            );
                        }
                        continue;
                    }

                    if let Some((inflight_seq, sent_at)) = self.icmp_ping_inflight {
                        if inflight_seq == seq_no {
                            let rtt = now() - sent_at;
                            crate::log!(
                                "net: icmp pong dev={} seq={} rtt={}ms\n",
                                self.device_index,
                                seq_no,
                                rtt.total_millis()
                            );
                            self.icmp_ping_inflight = None;

                            // Consider the network reachable on the first successful pong.
                            if self.icmp_ping_pongs == 0 {
                                self.icmp_ping_pongs = 1;
                                crate::log!(
                                    "net: icmp ok dev={} (gateway reachable)\n",
                                    self.device_index
                                );
                                crate::v::readiness::set(crate::v::readiness::NET_GATEWAY_REACHABLE);
                            }
                        }
                    }
                }
                _ => {}
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

// Shared net service state so both the async service task and synchronous `time::block_on`
// hooks can drive the stack without diverging socket/interface state.
static NET_SERVICES: spin::Mutex<Option<Vec<NetService>>> = spin::Mutex::new(None);

fn owner_device_index(owner: &str) -> Option<usize> {
    let (base, suffix) = owner.rsplit_once('@')?;
    if base.is_empty() || suffix.is_empty() {
        return None;
    }
    if !suffix.as_bytes().iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    suffix.parse::<usize>().ok()
}

fn ensure_services(count: usize) {
    let mut guard = NET_SERVICES.lock();
    let needs_init = guard.as_ref().map(|v| v.len() != count).unwrap_or(true);
    if needs_init {
        *guard = Some((0..count).map(NetService::new).collect());
    }
}

fn service_tick_once() {
    let count = crate::net::device_count();
    if count == 0 {
        return;
    }

    ensure_services(count);

    let mut guard = NET_SERVICES.lock();
    let Some(services) = guard.as_mut() else {
        return;
    };

    for svc in services.iter_mut() {
        svc.tick();
    }

    for _ in 0..MAX_DRAIN_PER_LOOP {
        let Some((owner, cmd)) = pop_command() else {
            break;
        };
        let idx = owner_device_index(owner).unwrap_or_else(crate::net::primary_device_index);
        let idx = idx.min(services.len().saturating_sub(1));
        services[idx].handle_command(owner, cmd);
    }

    for svc in services.iter_mut() {
        svc.poll_sockets();
    }
}


/// Per-NIC RX poll loop.
///
/// This decouples device RX polling from the smoltcp/service loop so that
/// adding NICs doesn't make a single task heavier.
#[task(pool_size = MAX_NET_DEVICES)]
pub async fn net_poll_task(index: usize) {
    async move {
        loop {
            crate::net::poll_at(index);
            Timer::after(EmbassyDuration::from_micros(NET_POLL_SLEEP_US)).await;
        }
    }.await;
}

#[task]
pub async fn net_service_task() {
    async move {
        let count = crate::net::device_count();
        if count == 0 {
            crate::log!("net: service disabled (no NIC)\n");
            return;
        }

        ensure_services(count);

        loop {
            service_tick_once();
            Timer::after(EmbassyDuration::from_micros(NET_SERVICE_SLEEP_US)).await;
        }
    }.await;
}
