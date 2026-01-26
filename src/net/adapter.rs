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

// QEMU slirp defaults we use today.
const SLIRP_GUEST_IP: Ipv4Address = Ipv4Address::new(10, 0, 2, 15);
const SLIRP_PREFIX: u8 = 24;
const SLIRP_GATEWAY_IP: Ipv4Address = Ipv4Address::new(10, 0, 2, 2);
const SLIRP_DNS_IP: Ipv4Address = Ipv4Address::new(10, 0, 2, 3);

const SMOKE_TCP_PORT: u16 = 4242;
const SMOKE_UDP_PORT: u16 = 4243;
const NET_SHELL_TCP_PORT: u16 = 4245;

static NET_RX_FRAMES: AtomicU64 = AtomicU64::new(0);
static NET_TX_FRAMES: AtomicU64 = AtomicU64::new(0);
static NET_TX_DROPPED: AtomicU64 = AtomicU64::new(0);

static NET_SHELL_STARTED: AtomicBool = AtomicBool::new(false);

struct NetShellState {
    handle: Option<NetHandle>,
    rx: VecDeque<u8>,
    tx: VecDeque<u8>,
}

static NET_SHELL_STATE: spin::Mutex<NetShellState> = spin::Mutex::new(NetShellState {
    handle: None,
    rx: VecDeque::new(),
    tx: VecDeque::new(),
});

pub fn net_shell_read_byte() -> Option<u8> {
    NET_SHELL_STATE.lock().rx.pop_front()
}

pub fn net_shell_write_bytes(bytes: &[u8]) {
    const MAX_TX: usize = 32 * 1024;
    let mut st = NET_SHELL_STATE.lock();
    for &b in bytes {
        if st.tx.len() >= MAX_TX {
            let _ = st.tx.pop_front();
        }
        st.tx.push_back(b);
    }
}

pub fn net_shell_write_byte(b: u8) {
    net_shell_write_bytes(&[b]);
}

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
static NET_ICMP_OK_X3: AtomicBool = AtomicBool::new(false);

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

fn push_event(target: &'static str, event: NetEvent) -> bool {
    let guard = APP_QUEUES.lock();
    if let Some(entry) = guard.iter().find(|e| e.name == target) {
        entry.events.push(event).is_ok()
    } else {
        false
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

struct LoopbackDevice {
    queue: VecDeque<Vec<u8>>,
}

struct LoopbackRxToken {
    buffer: Vec<u8>,
}

struct LoopbackTxToken<'a> {
    queue: &'a mut VecDeque<Vec<u8>>,
}

impl Device for LoopbackDevice {
    type RxToken<'a>
        = LoopbackRxToken
    where
        Self: 'a;
    type TxToken<'a>
        = LoopbackTxToken<'a>
    where
        Self: 'a;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let packet = self.queue.pop_front()?;
        Some((
            LoopbackRxToken { buffer: packet },
            LoopbackTxToken {
                queue: &mut self.queue,
            },
        ))
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        Some(LoopbackTxToken {
            queue: &mut self.queue,
        })
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = 1500;
        caps.max_burst_size = Some(1);
        caps.medium = Medium::Ethernet;
        caps
    }
}

impl RxToken for LoopbackRxToken {
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

impl<'a> TxToken for LoopbackTxToken<'a> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut buf = vec![0u8; len];
        let result = f(&mut buf[..]);
        self.queue.push_back(buf);
        result
    }

    fn set_meta(&mut self, _meta: PacketMeta) {}
}

struct TcpLoopbackSmoke {
    device: LoopbackDevice,
    iface: Interface,
    sockets: SocketSet<'static>,
    server: SocketHandle,
    client: SocketHandle,
    sent: bool,
    got_echo: bool,
    start: Instant,
}

impl TcpLoopbackSmoke {
    fn new() -> Self {
        const IP: Ipv4Address = Ipv4Address::new(192, 0, 2, 1);
        const PORT: u16 = 4244;

        let hw_addr = HardwareAddress::Ethernet(EthernetAddress([0x02, 0, 0, 0, 0, 1]));
        let mut cfg = IfaceConfig::new(hw_addr);
        cfg.random_seed = crate::rng::rdrand_u64().unwrap_or(0xC0DE_CAFE);

        let mut device = LoopbackDevice {
            queue: VecDeque::new(),
        };
        let mut iface = Interface::new(cfg, &mut device, now());
        iface.update_ip_addrs(|addrs| {
            let _ = addrs.push(IpCidr::Ipv4(Ipv4Cidr::new(IP, 24)));
        });

        let mut sockets = SocketSet::new(Vec::new());

        let server_rx = tcp::SocketBuffer::new(vec![0; 2048]);
        let server_tx = tcp::SocketBuffer::new(vec![0; 2048]);
        let mut server_socket = tcp::Socket::new(server_rx, server_tx);
        server_socket.listen(PORT).ok();
        let server = sockets.add(server_socket);

        let client_rx = tcp::SocketBuffer::new(vec![0; 2048]);
        let client_tx = tcp::SocketBuffer::new(vec![0; 2048]);
        let mut client_socket = tcp::Socket::new(client_rx, client_tx);
        client_socket.set_keep_alive(Some(SmolDuration::from_secs(30)));

        let local = IpEndpoint::new(IpAddress::Ipv4(IP), 49152);
        let remote = IpEndpoint::new(IpAddress::Ipv4(IP), PORT);
        let _ = client_socket.connect(iface.context(), remote, local);
        let client = sockets.add(client_socket);

        Self {
            device,
            iface,
            sockets,
            server,
            client,
            sent: false,
            got_echo: false,
            start: now(),
        }
    }

    fn step(&mut self) -> Option<bool> {
        let t = now();
        let _ = self
            .iface
            .poll(t, &mut self.device, &mut self.sockets);

        // Server: echo any received payload back.
        {
            let socket = self.sockets.get_mut::<tcp::Socket>(self.server);
            if socket.can_recv() {
                let mut buf = [0u8; 256];
                while let Ok(len) = socket.recv_slice(&mut buf) {
                    if len == 0 {
                        break;
                    }
                    if socket.can_send() && socket.may_send() {
                        let _ = socket.send_slice(&buf[..len]);
                    }
                }
            }
        }

        // Client: send once after established; then wait for echo.
        {
            const PAYLOAD: &[u8] = b"TRUEOS-tcp-loop";
            let socket = self.sockets.get_mut::<tcp::Socket>(self.client);
            if !self.sent
                && socket.state() == tcp::State::Established
                && socket.can_send()
                && socket.may_send()
            {
                let _ = socket.send_slice(PAYLOAD);
                self.sent = true;
            }

            if socket.can_recv() {
                let mut buf = [0u8; 256];
                while let Ok(len) = socket.recv_slice(&mut buf) {
                    if len == 0 {
                        break;
                    }
                    if &buf[..len] == PAYLOAD {
                        self.got_echo = true;
                    }
                }
            }
        }

        if self.sent && self.got_echo {
            return Some(true);
        }

        if t >= self.start + SmolDuration::from_millis(2000) {
            return Some(false);
        }

        None
    }
}

struct SocketRecord {
    owner: &'static str,
    handle: NetHandle,
    kind: SocketKind,
    socket: SocketHandle,
    established: bool,
    last_tcp_state: Option<tcp::State>,
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

    tcp_next_ephemeral: u16,
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
                SLIRP_GUEST_IP,
                SLIRP_PREFIX,
            )));
        });
        let routes = iface.routes_mut();
        let _ = routes.add_default_ipv4_route(SLIRP_GATEWAY_IP);

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
            established: false,
            last_tcp_state: None,
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
        let initial_state = socket.state();

        let handle = self.alloc_handle();
        let sh = self.sockets.add(socket);
        self.records.push(SocketRecord {
            owner,
            handle,
            kind: SocketKind::Tcp,
            socket: sh,
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

        let rx = tcp::SocketBuffer::new(vec![0; 4096]);
        let tx = tcp::SocketBuffer::new(vec![0; 4096]);
        let mut socket = tcp::Socket::new(rx, tx);
        socket.set_keep_alive(Some(SmolDuration::from_secs(30)));

        let local_port = self.tcp_next_ephemeral;
        self.tcp_next_ephemeral = self.tcp_next_ephemeral.wrapping_add(1).max(49152);

        let local = IpEndpoint::new(
            IpAddress::Ipv4(SLIRP_GUEST_IP),
            local_port,
        );
        let remote = IpEndpoint::new(
            IpAddress::Ipv4(Ipv4Address::from_octets(remote.addr)),
            remote.port,
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
            established: false,
            last_tcp_state: Some(initial_state),
        });
        Ok(handle)
    }

    fn handle_commands(&mut self, commands: Vec<(&'static str, Vec<NetCommand>)>) {
        for (owner, cmds) in commands.into_iter() {
            for cmd in cmds {
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
                                continue;
                            }
                            let socket_handle = rec.socket;
                            let endpoint = IpEndpoint::new(
                                IpAddress::Ipv4(Ipv4Address::from_octets(remote.addr)),
                                remote.port,
                            );
                            let socket = self.sockets.get_mut::<udp::Socket>(socket_handle);
                            let _ = socket.send_slice(&data, endpoint).map_err(|_| {
                                let _ = push_event(
                                    owner,
                                    NetEvent::Error {
                                        msg: "udp send fail",
                                    },
                                );
                            });
                        } else {
                            let _ = push_event(owner, NetEvent::Error { msg: "bad handle" });
                        }
                    }
                    NetCommand::SendTcp { handle, data } => {
                        if let Some(rec) = self.find_record(handle) {
                            if rec.kind != SocketKind::Tcp {
                                let _ = push_event(owner, NetEvent::Error { msg: "not tcp" });
                                continue;
                            }
                            let socket_handle = rec.socket;
                            let socket = self.sockets.get_mut::<tcp::Socket>(socket_handle);
                            if socket.can_send() && socket.may_send() {
                                match socket.send_slice(&data) {
                                    Ok(sent) => {
                                        let _ = push_event(
                                            owner,
                                            NetEvent::TcpSent {
                                                handle,
                                                len: sent,
                                            },
                                        );
                                    }
                                    Err(_) => {
                                        let _ = push_event(
                                            owner,
                                            NetEvent::Error {
                                                msg: "tcp send fail",
                                            },
                                        );
                                    }
                                }
                            } else {
                                let _ = push_event(
                                    owner,
                                    NetEvent::Error {
                                        msg: "tcp not ready",
                                    },
                                );
                            }
                        } else {
                            let _ = push_event(owner, NetEvent::Error { msg: "bad handle" });
                        }
                    }
                    NetCommand::Close { handle } => {
                        self.remove_record(handle);
                        let _ = push_event(owner, NetEvent::Closed { handle });
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

        let state = socket.state();
        if rec.last_tcp_state != Some(state) {
            rec.last_tcp_state = Some(state);
            crate::log!("net: tcp state owner={} handle={} state={:?}\n", owner, handle.0, state);
        }

        if socket.is_active() && socket.may_recv() {
            let mut buf = [0u8; 2048];
            while let Ok(len) = socket.recv_slice(&mut buf) {
                if len == 0 {
                    break;
                }
                let data = buf[..len].to_vec();
                let _ = push_event(owner, NetEvent::TcpData { handle, data });
            }
        }

        if socket.state() == tcp::State::Established && !rec.established {
            crate::log!(
                "net: tcp established branch owner={} handle={}\n",
                owner,
                handle.0
            );
            rec.established = true;
            let ok = push_event(owner, NetEvent::TcpEstablished { handle });
            crate::log!(
                "net: tcp established event owner={} handle={} queued={}\n",
                owner,
                handle.0,
                ok
            );
        }

        if !socket.is_open() {
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
                                    NET_ICMP_OK_X3.store(true, Ordering::Relaxed);
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

    let [a, b, c, d] = SLIRP_GUEST_IP.octets();
    let [ga, gb, gc, gd] = SLIRP_GATEWAY_IP.octets();
    crate::log!(
        "net-smoke: iface={}.{}.{}.{} /{} gw={}.{}.{}.{}; tcp_listen={} udp_bind={}\n",
        a,
        b,
        c,
        d,
        SLIRP_PREFIX,
        ga,
        gb,
        gc,
        gd,
        SMOKE_TCP_PORT,
        SMOKE_UDP_PORT
    );

    let _ = cmds.push(NetCommand::OpenTcpListen { port: SMOKE_TCP_PORT });
    let _ = cmds.push(NetCommand::OpenUdp { port: SMOKE_UDP_PORT });

    let mut tcp_handle: Option<NetHandle> = None;
    let mut udp_handle: Option<NetHandle> = None;
    let mut saw_tcp_established = false;
    let mut saw_tcp_data = false;
    let mut udp_probe_state: u8 = 0;
    let mut ticks: u32 = 0;
    let mut last_stats: Option<(u64, u64, u64, u32, u32)> = None;
    let mut tcp_loopback: Option<TcpLoopbackSmoke> = None;

    fn dns_query(id: u16, host: &str, qtype: u16) -> Vec<u8> {
        // Minimal DNS query for <qtype>/IN <host>.
        // Header: flags=0x0100 (RD), qdcount=1
        let mut q = Vec::new();
        q.extend_from_slice(&id.to_be_bytes());
        q.extend_from_slice(&0x0100u16.to_be_bytes());
        q.extend_from_slice(&1u16.to_be_bytes());
        q.extend_from_slice(&0u16.to_be_bytes());
        q.extend_from_slice(&0u16.to_be_bytes());
        q.extend_from_slice(&0u16.to_be_bytes());

        // QNAME
        for label in host.split('.') {
            let bytes = label.as_bytes();
            let len = bytes.len().min(63);
            q.push(len as u8);
            q.extend_from_slice(&bytes[..len]);
        }
        q.push(0);

        // QTYPE, QCLASS=IN (1)
        q.extend_from_slice(&qtype.to_be_bytes());
        q.extend_from_slice(&1u16.to_be_bytes());
        q
    }

    fn dns_skip_name(pkt: &[u8], idx: &mut usize) -> bool {
        if *idx >= pkt.len() {
            return false;
        }
        let mut steps: u8 = 0;
        loop {
            if *idx >= pkt.len() {
                return false;
            }
            let b = pkt[*idx];
            if b == 0 {
                *idx += 1;
                return true;
            }
            if (b & 0xC0) == 0xC0 {
                if *idx + 1 >= pkt.len() {
                    return false;
                }
                *idx += 2;
                return true;
            }
            let len = b as usize;
            *idx += 1;
            if *idx + len > pkt.len() {
                return false;
            }
            *idx += len;
            steps = steps.wrapping_add(1);
            if steps > 64 {
                return false;
            }
        }
    }

    fn dns_log_a_records_for(pkt: &[u8], want_id: u16, host: &str) {
        if pkt.len() < 12 {
            return;
        }

        let id = u16::from_be_bytes([pkt[0], pkt[1]]);
        if id != want_id {
            return;
        }

        let flags = u16::from_be_bytes([pkt[2], pkt[3]]);
        let rcode = (flags & 0x000F) as u8;
        let qd = u16::from_be_bytes([pkt[4], pkt[5]]) as usize;
        let an = u16::from_be_bytes([pkt[6], pkt[7]]) as usize;

        if rcode != 0 {
            crate::log!("net-smoke: nslookup {}: dns rcode={}\n", host, rcode);
            return;
        }

        let mut idx: usize = 12;
        for _ in 0..qd {
            if !dns_skip_name(pkt, &mut idx) {
                return;
            }
            if idx + 4 > pkt.len() {
                return;
            }
            idx += 4; // QTYPE/QCLASS
        }

        let mut found_any = false;
        for _ in 0..an {
            if !dns_skip_name(pkt, &mut idx) {
                return;
            }
            if idx + 10 > pkt.len() {
                return;
            }
            let typ = u16::from_be_bytes([pkt[idx], pkt[idx + 1]]);
            let class = u16::from_be_bytes([pkt[idx + 2], pkt[idx + 3]]);
            let rdlen = u16::from_be_bytes([pkt[idx + 8], pkt[idx + 9]]) as usize;
            idx += 10;
            if idx + rdlen > pkt.len() {
                return;
            }

            if typ == 1 && class == 1 && rdlen == 4 {
                let ip = [pkt[idx], pkt[idx + 1], pkt[idx + 2], pkt[idx + 3]];
                crate::log!(
                    "net-smoke: nslookup {} => A {}.{}.{}.{}\n",
                    host,
                    ip[0],
                    ip[1],
                    ip[2],
                    ip[3]
                );
                found_any = true;
            }

            idx += rdlen;
        }

        if !found_any {
            crate::log!("net-smoke: nslookup {}: no A records\n", host);
        }
    }

    fn dns_log_aaaa_records_for(pkt: &[u8], want_id: u16, host: &str) {
        if pkt.len() < 12 {
            return;
        }

        let id = u16::from_be_bytes([pkt[0], pkt[1]]);
        if id != want_id {
            return;
        }

        let flags = u16::from_be_bytes([pkt[2], pkt[3]]);
        let rcode = (flags & 0x000F) as u8;
        let qd = u16::from_be_bytes([pkt[4], pkt[5]]) as usize;
        let an = u16::from_be_bytes([pkt[6], pkt[7]]) as usize;

        if rcode != 0 {
            crate::log!("net-smoke: nslookup {}: dns rcode={}\n", host, rcode);
            return;
        }

        let mut idx: usize = 12;
        for _ in 0..qd {
            if !dns_skip_name(pkt, &mut idx) {
                return;
            }
            if idx + 4 > pkt.len() {
                return;
            }
            idx += 4; // QTYPE/QCLASS
        }

        let mut found_any = false;
        for _ in 0..an {
            if !dns_skip_name(pkt, &mut idx) {
                return;
            }
            if idx + 10 > pkt.len() {
                return;
            }
            let typ = u16::from_be_bytes([pkt[idx], pkt[idx + 1]]);
            let class = u16::from_be_bytes([pkt[idx + 2], pkt[idx + 3]]);
            let rdlen = u16::from_be_bytes([pkt[idx + 8], pkt[idx + 9]]) as usize;
            idx += 10;
            if idx + rdlen > pkt.len() {
                return;
            }

            if typ == 28 && class == 1 && rdlen == 16 {
                let s0 = u16::from_be_bytes([pkt[idx], pkt[idx + 1]]);
                let s1 = u16::from_be_bytes([pkt[idx + 2], pkt[idx + 3]]);
                let s2 = u16::from_be_bytes([pkt[idx + 4], pkt[idx + 5]]);
                let s3 = u16::from_be_bytes([pkt[idx + 6], pkt[idx + 7]]);
                let s4 = u16::from_be_bytes([pkt[idx + 8], pkt[idx + 9]]);
                let s5 = u16::from_be_bytes([pkt[idx + 10], pkt[idx + 11]]);
                let s6 = u16::from_be_bytes([pkt[idx + 12], pkt[idx + 13]]);
                let s7 = u16::from_be_bytes([pkt[idx + 14], pkt[idx + 15]]);
                crate::log!(
                    "net-smoke: nslookup {} => AAAA {:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}\n",
                    host,
                    s0,
                    s1,
                    s2,
                    s3,
                    s4,
                    s5,
                    s6,
                    s7
                );
                found_any = true;
            }

            idx += rdlen;
        }

        if !found_any {
            crate::log!("net-smoke: nslookup {}: no AAAA records\n", host);
        }
    }

    loop {
        for ev in events.drain(16) {
            match ev {
                NetEvent::Opened { handle, kind } => {
                    crate::log!("net-smoke: opened {:?} handle={}\n", kind, handle.0);
                    if kind == SocketKind::Tcp {
                        if tcp_handle.is_none() {
                            tcp_handle = Some(handle);
                        }
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
                NetEvent::TcpSent { .. } => {}
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

                    if from.port == 53 && from.addr == SLIRP_DNS_IP.octets() {
                        dns_log_a_records_for(&data, 0x1234, "example.com");
                        dns_log_a_records_for(&data, 0x1235, "trueos.eu");
                        dns_log_aaaa_records_for(&data, 0x1236, "trueos.eu");
                    }
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
        if let Some(handle) = udp_handle {
            if udp_probe_state == 0 {
                let _ = cmds.push(NetCommand::SendUdp {
                    handle,
                    remote: NetEndpoint {
                        addr: SLIRP_DNS_IP.octets(),
                        port: 53,
                    },
                    data: dns_query(0x1234, "example.com", 1),
                });
                udp_probe_state = 1;
                crate::log!("net-smoke: nslookup example.com (udp) via 10.0.2.3:53\n");
            } else if udp_probe_state == 1 {
                let _ = cmds.push(NetCommand::SendUdp {
                    handle,
                    remote: NetEndpoint {
                        addr: SLIRP_DNS_IP.octets(),
                        port: 53,
                    },
                    data: dns_query(0x1235, "trueos.eu", 1),
                });
                udp_probe_state = 2;
                crate::log!("net-smoke: nslookup trueos.eu (udp) via 10.0.2.3:53\n");
            } else if udp_probe_state == 2 {
                let _ = cmds.push(NetCommand::SendUdp {
                    handle,
                    remote: NetEndpoint {
                        addr: SLIRP_DNS_IP.octets(),
                        port: 53,
                    },
                    data: dns_query(0x1236, "trueos.eu", 28),
                });
                udp_probe_state = 3;
                crate::log!("net-smoke: nslookup trueos.eu (AAAA) (udp) via 10.0.2.3:53\n");
            }
        }

        // After the ICMP proof completes, exercise TCP deterministically using an in-kernel
        // loopback device (no dependence on slirp TCP services; cannot stall bring-up).
        if tcp_loopback.is_none() && NET_ICMP_OK_X3.load(Ordering::Relaxed) {
            tcp_loopback = Some(TcpLoopbackSmoke::new());
            crate::log!("net-smoke: starting tcp loopback probe\n");
        }

        if let Some(lb) = tcp_loopback.as_mut() {
            if let Some(ok) = lb.step() {
                if ok {
                    crate::log!("net-smoke: ok (tcp loopback echo)\n");
                    if let Some(h) = tcp_handle {
                        let _ = cmds.push(NetCommand::Close { handle: h });
                    }
                    if let Some(h) = udp_handle {
                        let _ = cmds.push(NetCommand::Close { handle: h });
                    }
                    break;
                } else {
                    crate::log!("net-smoke: tcp loopback probe timed out\n");
                    tcp_loopback = None;
                }
            }
        }

        if saw_tcp_established && saw_tcp_data {
            crate::log!("net-smoke: ok (tcp established + data)\n");
            if let Some(h) = tcp_handle {
                let _ = cmds.push(NetCommand::Close { handle: h });
            }
            if let Some(h) = udp_handle {
                let _ = cmds.push(NetCommand::Close { handle: h });
            }
            break;
        }

        ticks = ticks.wrapping_add(1);
        if (ticks % 20) == 0 {
            let (rx, tx, dropped) = net_debug_counters();
            let tcp_id = tcp_handle.map(|h| h.0).unwrap_or(0);
            let udp_id = udp_handle.map(|h| h.0).unwrap_or(0);
            let cur = (rx, tx, dropped, tcp_id, udp_id);
            if last_stats != Some(cur) {
                last_stats = Some(cur);
                crate::log!(
                    "net-smoke: stats rx={} tx={} dropped={} tcp_handle={} udp_handle={}\n",
                    rx,
                    tx,
                    dropped,
                    tcp_id,
                    udp_id
                );
            }
        }

        Timer::after(EmbassyDuration::from_millis(50)).await;
    }
}

/// TCP-backed shell I/O bridge.
///
/// - Listens on `NET_SHELL_TCP_PORT`.
/// - Buffers RX bytes into `net_shell_read_byte()`.
/// - Buffers shell output from `net_shell_write_bytes()` and flushes it over TCP.
#[task]
pub async fn net_shell_task() {
    if NET_SHELL_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }

    if crate::net::mac_address().is_none() {
        crate::log!("net-shell: disabled (no NIC)\n");
        return;
    }

    const OWNER: &'static str = "net-shell";

    let cmds = NetQueue::new_leaked("net-shell-cmd", 256);
    let events = NetQueue::new_leaked("net-shell-evt", 256);
    register_app_queues(OWNER, cmds, events);

    let _ = cmds.push(NetCommand::OpenTcpListen {
        port: NET_SHELL_TCP_PORT,
    });
    crate::log!("net-shell: listening on tcp {} (hostfwd localhost:{} -> guest)\n", NET_SHELL_TCP_PORT, NET_SHELL_TCP_PORT);

    let mut ticks: u32 = 0;
    let mut logged_first_rx: bool = false;
    let mut pending: Option<Vec<u8>> = None;
    let mut pending_handle: Option<NetHandle> = None;
    let mut pending_ticks: u32 = 0;
    let mut pending_len: usize = 0;
    let mut tx_log_budget: u32 = 16;
    let mut tcp_handle: Option<NetHandle> = None;

    loop {
        for ev in events.drain(32) {
            match ev {
                NetEvent::Opened { handle, kind } => {
                    if kind == SocketKind::Tcp {
                        tcp_handle = Some(handle);
                        crate::log!("net-shell: opened tcp handle={}\n", handle.0);
                    }
                }
                NetEvent::TcpEstablished { handle } => {
                    {
                        let mut st = NET_SHELL_STATE.lock();
                        let is_new_conn = st.handle != Some(handle);
                        st.handle = Some(handle);
                        if is_new_conn {
                            st.rx.clear();
                        }
                        // TCP shells are typically used from a raw terminal; start in prompt mode
                        // immediately (the regular shell starts in "cube mode" until Enter).
                        st.rx.push_back(b'\r');
                    }
                    pending = None;
                    pending_handle = Some(handle);
                    pending_ticks = 0;
                    pending_len = 0;
                    logged_first_rx = false;
                    tx_log_budget = 16;
                    crate::log!("net-shell: tcp established handle={}\n", handle.0);

                    // Nudge: make sure the client sees *something* even if the shell is quiet.
                    net_shell_write_bytes(b"\r\nTRUEOS net shell connected.\r\n");
                }
                NetEvent::TcpData { handle, data } => {
                    // Only accept bytes from the active connection.
                    // NOTE: Data can arrive before we process `TcpEstablished` (event ordering),
                    // so treat the first inbound bytes as selecting the active handle.
                    {
                        let mut st = NET_SHELL_STATE.lock();
                        if st.handle.is_none() {
                            st.handle = Some(handle);
                        }
                        if st.handle != Some(handle) {
                            continue;
                        }

                        if !logged_first_rx {
                            logged_first_rx = true;
                            crate::log!(
                                "net-shell: first rx {} bytes (including {:?})\n",
                                data.len(),
                                data.get(0).copied()
                            );
                        }

                        const MAX_RX: usize = 8 * 1024;
                        for b in data {
                            if st.rx.len() >= MAX_RX {
                                let _ = st.rx.pop_front();
                            }
                            st.rx.push_back(b);
                        }
                    }
                }
                NetEvent::TcpSent { handle, len } => {
                    if pending_handle != Some(handle) {
                        continue;
                    }

                    if tx_log_budget > 0 {
                        tx_log_budget -= 1;
                        crate::log!(
                            "net-shell: tx accepted handle={} len={} (pending_len={})\n",
                            handle.0,
                            len,
                            pending_len
                        );
                    }

                    // Drop the bytes we now know were accepted by smoltcp.
                    // NOTE: smoltcp may accept only a prefix of the buffer; keep the rest queued.
                    let mut st = NET_SHELL_STATE.lock();
                    for _ in 0..len {
                        let _ = st.tx.pop_front();
                    }
                    pending = None;
                    pending_ticks = 0;
                    pending_len = 0;
                }
                NetEvent::Closed { handle } => {
                    let mut st = NET_SHELL_STATE.lock();
                    if st.handle == Some(handle) {
                        st.handle = None;
                        st.rx.clear();
                        pending = None;
                        pending_handle = None;
                        pending_ticks = 0;
                        pending_len = 0;
                    }

                    if tcp_handle == Some(handle) {
                        tcp_handle = None;
                        crate::log!("net-shell: tcp closed handle={} (relisten)\n", handle.0);
                        let _ = cmds.push(NetCommand::OpenTcpListen {
                            port: NET_SHELL_TCP_PORT,
                        });
                    }
                }
                NetEvent::Error { msg } => {
                    // These are useful during bring-up; keep them visible but not too spammy.
                    if (ticks % 100) == 0 {
                        crate::log!("net-shell: error {}\n", msg);
                    }
                }
                NetEvent::UdpPacket { .. } => {}
            }
        }

        // Flush buffered TX to the active TCP connection.
        // Use an explicit ack event (`TcpSent`) so we only pop on success.
        if pending.is_none() {
            let (handle, chunk) = {
                let st = NET_SHELL_STATE.lock();
                match st.handle {
                    None => (None, Vec::new()),
                    Some(handle) => {
                        if st.tx.is_empty() {
                            (Some(handle), Vec::new())
                        } else {
                            let mut v = Vec::with_capacity(512);
                            for &b in st.tx.iter().take(512) {
                                v.push(b);
                            }
                            (Some(handle), v)
                        }
                    }
                }
            };

            if let Some(handle) = handle {
                if !chunk.is_empty() {
                    pending_handle = Some(handle);
                    pending = Some(chunk.clone());
                    pending_ticks = 0;
                    pending_len = chunk.len();

                    if tx_log_budget > 0 {
                        tx_log_budget -= 1;
                        crate::log!(
                            "net-shell: tx queue handle={} len={}\n",
                            handle.0,
                            pending_len
                        );
                    }

                    if cmds.push(NetCommand::SendTcp { handle, data: chunk }).is_err() {
                        // If the command queue is full, don't stall forever waiting for an event.
                        pending = None;
                        pending_ticks = 0;
                        pending_len = 0;
                        crate::log!("net-shell: tx queue full (dropping pending)\n");
                    }
                }
            }
        }

        // Safety: if we somehow miss the `TcpSent` event (or the socket is briefly not-ready),
        // don't wedge TX forever. We'll retry by clearing `pending` after a short timeout.
        if pending.is_some() {
            pending_ticks = pending_ticks.wrapping_add(1);
            if pending_ticks == 250 {
                crate::log!(
                    "net-shell: tx stalled (pending_len={}), retrying\n",
                    pending_len
                );
                pending = None;
                pending_ticks = 0;
                pending_len = 0;
            }
        }

        ticks = ticks.wrapping_add(1);
        Timer::after(EmbassyDuration::from_millis(10)).await;
        let _ = ticks;
    }
}
