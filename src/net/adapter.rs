use alloc::{boxed::Box, collections::VecDeque, vec, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use embassy_executor::task;
use embassy_time::{Duration as EmbassyDuration, Timer};
use smoltcp::iface::{Config as IfaceConfig, Interface, SocketHandle, SocketSet};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, PacketMeta, RxToken, TxToken};
use smoltcp::phy::ChecksumCapabilities;
use smoltcp::socket::{icmp, tcp, udp};
use smoltcp::time::{Duration as SmolDuration, Instant};
use smoltcp::wire::{
    EthernetAddress, HardwareAddress, IpAddress, IpCidr, IpEndpoint, Ipv4Address, Ipv4Cidr,
    Icmpv4Packet, Icmpv4Repr,
};

use crate::log;

const MAX_SOCKETS: usize = 8;
const MAX_DRAIN_PER_LOOP: usize = 32;
const ICMP_IDENT: u16 = 0x1234;

const SMOKE_TCP_PORT: u16 = 4242;
const SMOKE_UDP_PORT: u16 = 4243;

static NET_RX_FRAMES: AtomicU64 = AtomicU64::new(0);
static NET_TX_FRAMES: AtomicU64 = AtomicU64::new(0);
static NET_TX_DROPPED: AtomicU64 = AtomicU64::new(0);

pub fn net_debug_counters() -> (u64, u64, u64) {
    (
        NET_RX_FRAMES.load(Ordering::Relaxed),
        NET_TX_FRAMES.load(Ordering::Relaxed),
        NET_TX_DROPPED.load(Ordering::Relaxed),
    )
}

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
    SendUdp {
        handle: NetHandle,
        remote: NetEndpoint,
        data: Vec<u8>,
    },
    SendTcp {
        handle: NetHandle,
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocketKind {
    Udp,
    Tcp,
}

pub struct NetQueue<T> {
    name: &'static str,
    capacity: usize,
    inner: spin::Mutex<VecDeque<T>>,
    dropped: AtomicU32,
}

static NET_SMOKE_STARTED: AtomicBool = AtomicBool::new(false);

impl<T> NetQueue<T> {
    pub fn new_leaked(name: &'static str, capacity: usize) -> &'static Self {
        let q = Self {
            name,
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

    pub fn stats(&self) -> (usize, usize, u32) {
        (
            self.inner.lock().len(),
            self.capacity,
            self.dropped.load(Ordering::Relaxed),
        )
    }

    pub fn name(&self) -> &'static str {
        self.name
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

fn drain_commands() -> Vec<(&'static str, Vec<NetCommand>)> {
    let guard = APP_QUEUES.lock();
    let mut out = Vec::new();
    for entry in guard.iter() {
        let drained = entry.cmds.drain(MAX_DRAIN_PER_LOOP);
        if !drained.is_empty() {
            out.push((entry.name, drained));
        }
    }
    out
}

fn push_event(target: &'static str, event: NetEvent) {
    let guard = APP_QUEUES.lock();
    if let Some(entry) = guard.iter().find(|e| e.name == target) {
        let _ = entry.events.push(event);
    }
}

struct AdapterDevice;

impl Device for AdapterDevice {
    type RxToken<'a>
        = AdapterRxToken
    where
        Self: 'a;
    type TxToken<'a>
        = AdapterTxToken
    where
        Self: 'a;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        crate::net::pop_rx_packet().map(|packet| {
            let new_total = NET_RX_FRAMES.fetch_add(1, Ordering::Relaxed) + 1;
            if (new_total & 0x3F) == 0 {
                log!("net: rx frames={}\n", new_total);
            }
            (AdapterRxToken { buffer: packet }, AdapterTxToken)
        })
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        Some(AdapterTxToken)
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = 1500;
        caps.max_burst_size = Some(1);
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

struct AdapterTxToken;

impl TxToken for AdapterTxToken {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut buf = vec![0u8; len];
        let result = f(&mut buf[..]);
        let new_total = NET_TX_FRAMES.fetch_add(1, Ordering::Relaxed) + 1;
        if crate::net::transmit_packet(&buf[..]).is_err() {
            let dropped = NET_TX_DROPPED.fetch_add(1, Ordering::Relaxed) + 1;
            log!("net: TX busy, dropping {}-byte frame.\n", len);
            if (dropped & 0x3F) == 0 {
                log!("net: tx frames={} dropped={}\n", new_total, dropped);
            }
        } else if (new_total & 0x3F) == 0 {
            log!("net: tx frames={} dropped={}\n", new_total, NET_TX_DROPPED.load(Ordering::Relaxed));
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
    established: bool,
}

struct NetService {
    iface: Interface,
    sockets: SocketSet<'static>,
    records: Vec<SocketRecord>,
    next_handle: AtomicU32,
    icmp: SocketHandle,

    // Minimal ICMP reachability probe (ping gateway).
    icmp_ping_seq: u16,
    icmp_ping_inflight: Option<(u16, Instant)>,
    icmp_ping_last_sent: Option<Instant>,
    icmp_ping_pongs: u8,
}

impl NetService {
    fn new() -> Self {
        let mac = crate::net::mac_address().unwrap_or([0, 0, 0, 0, 0, 1]);
        let hw_addr = HardwareAddress::Ethernet(EthernetAddress(mac));

        let mut cfg = IfaceConfig::new(hw_addr);
        cfg.random_seed = crate::rng::rdrand_u64().unwrap_or(0x9E37_79B9);
        let mut device = AdapterDevice;
        let mut iface = Interface::new(cfg, &mut device, now());
        iface.update_ip_addrs(|addrs| {
            let _ = addrs.push(IpCidr::Ipv4(Ipv4Cidr::new(
                Ipv4Address::new(10, 0, 2, 15),
                24,
            )));
        });
        let routes = iface.routes_mut();
        let _ = routes.add_default_ipv4_route(Ipv4Address::new(10, 0, 2, 2));

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

        Self {
            iface,
            sockets,
            records: Vec::new(),
            next_handle: AtomicU32::new(1),
            icmp,

            icmp_ping_seq: 0,
            icmp_ping_inflight: None,
            icmp_ping_last_sent: None,
            icmp_ping_pongs: 0,
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
            established: false,
        });
        Ok(handle)
    }

    fn open_tcp(&mut self, owner: &'static str, port: u16) -> Result<NetHandle, &'static str> {
        if self.records.len() >= MAX_SOCKETS {
            return Err("no sockets available");
        }

        let rx = tcp::SocketBuffer::new(vec![0; 4096]);
        let tx = tcp::SocketBuffer::new(vec![0; 4096]);
        let mut socket = tcp::Socket::new(rx, tx);
        socket.listen(port).map_err(|_| "listen failed")?;
        socket.set_keep_alive(Some(SmolDuration::from_secs(30)));

        let handle = self.alloc_handle();
        let sh = self.sockets.add(socket);
        self.records.push(SocketRecord {
            owner,
            handle,
            kind: SocketKind::Tcp,
            socket: sh,
            established: false,
        });
        Ok(handle)
    }

    fn handle_commands(&mut self, commands: Vec<(&'static str, Vec<NetCommand>)>) {
        for (owner, cmds) in commands.into_iter() {
            for cmd in cmds {
                match cmd {
                    NetCommand::OpenUdp { port } => match self.open_udp(owner, port) {
                        Ok(handle) => push_event(
                            owner,
                            NetEvent::Opened {
                                handle,
                                kind: SocketKind::Udp,
                            },
                        ),
                        Err(msg) => push_event(owner, NetEvent::Error { msg }),
                    },
                    NetCommand::OpenTcpListen { port } => match self.open_tcp(owner, port) {
                        Ok(handle) => push_event(
                            owner,
                            NetEvent::Opened {
                                handle,
                                kind: SocketKind::Tcp,
                            },
                        ),
                        Err(msg) => push_event(owner, NetEvent::Error { msg }),
                    },
                    NetCommand::SendUdp {
                        handle,
                        remote,
                        data,
                    } => {
                        if let Some(rec) = self.find_record(handle) {
                            if rec.kind != SocketKind::Udp {
                                push_event(owner, NetEvent::Error { msg: "not udp" });
                                continue;
                            }
                            let socket_handle = rec.socket;
                            let endpoint = IpEndpoint::new(
                                IpAddress::Ipv4(Ipv4Address::from_octets(remote.addr)),
                                remote.port,
                            );
                            let socket = self.sockets.get_mut::<udp::Socket>(socket_handle);
                            let _ = socket.send_slice(&data, endpoint).map_err(|_| {
                                push_event(
                                    owner,
                                    NetEvent::Error {
                                        msg: "udp send fail",
                                    },
                                );
                            });
                        } else {
                            push_event(owner, NetEvent::Error { msg: "bad handle" });
                        }
                    }
                    NetCommand::SendTcp { handle, data } => {
                        if let Some(rec) = self.find_record(handle) {
                            if rec.kind != SocketKind::Tcp {
                                push_event(owner, NetEvent::Error { msg: "not tcp" });
                                continue;
                            }
                            let socket_handle = rec.socket;
                            let socket = self.sockets.get_mut::<tcp::Socket>(socket_handle);
                            if socket.can_send() && socket.may_send() {
                                let _ = socket.send_slice(&data).map_err(|_| {
                                    push_event(
                                        owner,
                                        NetEvent::Error {
                                            msg: "tcp send fail",
                                        },
                                    );
                                });
                            } else {
                                push_event(
                                    owner,
                                    NetEvent::Error {
                                        msg: "tcp not ready",
                                    },
                                );
                            }
                        } else {
                            push_event(owner, NetEvent::Error { msg: "bad handle" });
                        }
                    }
                    NetCommand::Close { handle } => {
                        self.remove_record(handle);
                        push_event(owner, NetEvent::Closed { handle });
                    }
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
        let Some(rec) = self.records.get_mut(idx) else {
            return false;
        };
        if rec.kind != SocketKind::Tcp {
            return false;
        }

        let owner = rec.owner;
        let handle = rec.handle;
        let socket = self.sockets.get_mut::<tcp::Socket>(rec.socket);

        if socket.is_active() && socket.may_recv() {
            let mut buf = [0u8; 2048];
            while let Ok(len) = socket.recv_slice(&mut buf) {
                let data = buf[..len].to_vec();
                let _ = push_event(owner, NetEvent::TcpData { handle, data });
            }
        }

        if socket.state() == tcp::State::Established && !rec.established {
            rec.established = true;
            push_event(owner, NetEvent::TcpEstablished { handle });
        }

        if !socket.is_open() {
            push_event(owner, NetEvent::Closed { handle });
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
        let _ = self
            .iface
            .poll(timestamp, &mut AdapterDevice, &mut self.sockets);

        self.poll_icmp();

        // After polling, try a deterministic ICMP ping to prove RX/TX + IP stack.
        self.maybe_send_icmp_ping(timestamp);
    }

    fn maybe_send_icmp_ping(&mut self, timestamp: Instant) {
        if self.icmp_ping_pongs >= 3 {
            return;
        }

        // QEMU user-net (slirp) default gateway.
        let target = Ipv4Address::new(10, 0, 2, 2);

        if let Some((_, sent_at)) = self.icmp_ping_inflight {
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

        self.icmp_ping_seq = self.icmp_ping_seq.wrapping_add(1);
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
                "net: icmp ping seq={} -> {}.{}.{}.{}\n",
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

                    if let Some((inflight_seq, sent_at)) = self.icmp_ping_inflight {
                        if inflight_seq == seq_no {
                            let rtt = now() - sent_at;
                            crate::log!(
                                "net: icmp pong seq={} rtt={}ms\n",
                                seq_no,
                                rtt.total_millis()
                            );
                            self.icmp_ping_inflight = None;

                            if self.icmp_ping_pongs < 3 {
                                self.icmp_ping_pongs = self.icmp_ping_pongs.saturating_add(1);
                                if self.icmp_ping_pongs == 3 {
                                    crate::log!("net: icmp ok x3 (gateway reachable)\n");
                                }
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

#[task]
pub async fn net_service_task() {
    let mut svc = NetService::new();

    loop {
        crate::net::poll();
        svc.tick();

        let cmds = drain_commands();
        if !cmds.is_empty() {
            svc.handle_commands(cmds);
        }

        svc.poll_sockets();
        Timer::after(EmbassyDuration::from_millis(5)).await;
    }
}

/// Minimal deterministic smoke test for the TCP/IP stack.
///
/// - Registers an app queue.
/// - Opens a TCP listener (`SMOKE_TCP_PORT`) and a UDP socket (`SMOKE_UDP_PORT`).
/// - Logs key events and echoes any received TCP data back to the sender.
#[task]
pub async fn net_smoke_task() {
    if NET_SMOKE_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }

    const OWNER: &'static str = "net-smoke";

    let cmds = NetQueue::new_leaked("net-smoke-cmd", 64);
    let events = NetQueue::new_leaked("net-smoke-evt", 64);
    register_app_queues(OWNER, cmds, events);

    crate::log!(
        "net-smoke: iface=10.0.2.15/24 gw=10.0.2.2; tcp_listen={} udp_bind={}\n",
        SMOKE_TCP_PORT,
        SMOKE_UDP_PORT
    );

    let _ = cmds.push(NetCommand::OpenTcpListen { port: SMOKE_TCP_PORT });
    let _ = cmds.push(NetCommand::OpenUdp { port: SMOKE_UDP_PORT });

    let mut tcp_handle: Option<NetHandle> = None;
    let mut udp_handle: Option<NetHandle> = None;
    let mut saw_tcp_established = false;
    let mut saw_tcp_data = false;
    let mut udp_probe_sent = false;
    let mut ticks: u32 = 0;

    fn dns_query_example_com() -> Vec<u8> {
        // Minimal DNS query for A/IN example.com
        // Header: id=0x1234, flags=0x0100 (RD), qdcount=1
        let mut q = Vec::new();
        q.extend_from_slice(&0x1234u16.to_be_bytes());
        q.extend_from_slice(&0x0100u16.to_be_bytes());
        q.extend_from_slice(&1u16.to_be_bytes());
        q.extend_from_slice(&0u16.to_be_bytes());
        q.extend_from_slice(&0u16.to_be_bytes());
        q.extend_from_slice(&0u16.to_be_bytes());

        // QNAME: example.com
        q.push(7);
        q.extend_from_slice(b"example");
        q.push(3);
        q.extend_from_slice(b"com");
        q.push(0);

        // QTYPE=A (1), QCLASS=IN (1)
        q.extend_from_slice(&1u16.to_be_bytes());
        q.extend_from_slice(&1u16.to_be_bytes());
        q
    }

    loop {
        for ev in events.drain(16) {
            match ev {
                NetEvent::Opened { handle, kind } => {
                    crate::log!("net-smoke: opened {:?} handle={}\n", kind, handle.0);
                    if kind == SocketKind::Tcp {
                        tcp_handle = Some(handle);
                    }
                    if kind == SocketKind::Udp {
                        udp_handle = Some(handle);
                    }
                }
                NetEvent::TcpEstablished { handle } => {
                    crate::log!("net-smoke: tcp established handle={}\n", handle.0);
                    saw_tcp_established = true;
                }
                NetEvent::TcpData { handle, data } => {
                    crate::log!("net-smoke: tcp data handle={} len={}\n", handle.0, data.len());
                    saw_tcp_data = true;

                    // Echo back (bounded) to verify TX path.
                    let echo_len = data.len().min(512);
                    let _ = cmds.push(NetCommand::SendTcp {
                        handle,
                        data: data[..echo_len].to_vec(),
                    });
                }
                NetEvent::UdpPacket { handle, from, data } => {
                    crate::log!(
                        "net-smoke: udp rx handle={} from={}.{}.{}.{}:{} len={}\n",
                        handle.0,
                        from.addr[0],
                        from.addr[1],
                        from.addr[2],
                        from.addr[3],
                        from.port,
                        data.len()
                    );
                }
                NetEvent::Closed { handle } => {
                    crate::log!("net-smoke: closed handle={}\n", handle.0);
                    if tcp_handle == Some(handle) {
                        tcp_handle = None;
                    }
                }
                NetEvent::Error { msg } => {
                    crate::log!("net-smoke: error {}\n", msg);
                }
            }
        }

        // Try to force deterministic inbound traffic without requiring any host-side tooling:
        // QEMU user networking (slirp) exposes a DNS server at 10.0.2.3:53.
        if !udp_probe_sent {
            if let Some(handle) = udp_handle {
                let _ = cmds.push(NetCommand::SendUdp {
                    handle,
                    remote: NetEndpoint {
                        addr: [10, 0, 2, 3],
                        port: 53,
                    },
                    data: dns_query_example_com(),
                });
                udp_probe_sent = true;
                crate::log!("net-smoke: sent DNS query to 10.0.2.3:53\n");
            }
        }

        if saw_tcp_established && saw_tcp_data {
            crate::log!("net-smoke: ok (tcp established + data)\n");
            break;
        }

        ticks = ticks.wrapping_add(1);
        if (ticks % 20) == 0 {
            let (rx, tx, dropped) = net_debug_counters();
            crate::log!(
                "net-smoke: stats rx={} tx={} dropped={} tcp_handle={} udp_handle={}\n",
                rx,
                tx,
                dropped,
                tcp_handle.map(|h| h.0).unwrap_or(0),
                udp_handle.map(|h| h.0).unwrap_or(0)
            );
        }

        Timer::after(EmbassyDuration::from_millis(50)).await;
    }
}
