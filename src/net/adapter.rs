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
            if (new_total & 0x3F) == 0 {
                log!("net: rx frames={}\n", new_total);
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
        let new_total = NET_TX_FRAMES.fetch_add(1, Ordering::Relaxed) + 1;
        if crate::net::transmit_packet_at(self.index, &buf[..]).is_err() {
            let dropped = NET_TX_DROPPED.fetch_add(1, Ordering::Relaxed) + 1;
            log!("net: TX busy, dropping {}-byte frame.\n", len);
            if (dropped & 0x3F) == 0 {
                log!("net: tx frames={} dropped={}\n", new_total, dropped);
            }
        } else if (new_total & 0x3F) == 0 {
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

struct NetService {
    device_index: usize,
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
    fn new(device_index: usize) -> Self {
        let mac = crate::net::mac_address_at(device_index).unwrap_or([0, 0, 0, 0, 0, 1]);
        let hw_addr = HardwareAddress::Ethernet(EthernetAddress(mac));

        let mut cfg = IfaceConfig::new(hw_addr);
        cfg.random_seed = crate::rng::rdrand_u64().unwrap_or(0x9E37_79B9);
        let mut device = AdapterDeviceAt { index: device_index };
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
            device_index,
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
        for _ in 0..32 {
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
                        if let Some(idx) = self.records.iter().position(|r| r.handle == handle) {
                            if self.records[idx].kind != SocketKind::Tcp {
                                let _ = push_event(owner, NetEvent::Error { msg: "not tcp" });
                                continue;
                            }
                            // Don't drop on backpressure; queue and flush when the socket becomes writable.
                            // This is especially important for TLS handshakes (ClientHello) right after connect.
                            self.records[idx].tcp_tx.extend(data);
                            self.flush_tcp_tx(idx);
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
                let mut buf = [0u8; 2048];
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

                            if self.icmp_ping_pongs < 3 {
                                self.icmp_ping_pongs = self.icmp_ping_pongs.saturating_add(1);
                                if self.icmp_ping_pongs == 3 {
                                    crate::log!(
                                        "net: icmp ok x3 dev={} (gateway reachable)\n",
                                        self.device_index
                                    );
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

pub const MAX_NET_DEVICES: usize = 8;

/// Per-NIC RX poll loop.
///
/// This decouples device RX polling from the smoltcp/service loop so that
/// adding NICs doesn't make a single task heavier.
#[task(pool_size = MAX_NET_DEVICES)]
pub async fn net_poll_task(index: usize) {
    loop {
        crate::net::poll_at(index);
        Timer::after(EmbassyDuration::from_millis(1)).await;
    }
}

#[task]
pub async fn net_service_task() {
    let count = crate::net::device_count();
    if count == 0 {
        crate::log!("net: service disabled (no NIC)\n");
        return;
    }

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

    let mut services: Vec<NetService> = (0..count).map(NetService::new).collect();

    loop {
        for svc in services.iter_mut() {
            svc.tick();
        }

        let cmds = drain_commands();
        for (owner, batch) in cmds {
            let idx = owner_device_index(owner).unwrap_or(0);
            let idx = idx.min(services.len().saturating_sub(1));
            services[idx].handle_commands(vec![(owner, batch)]);
        }

        for svc in services.iter_mut() {
            svc.poll_sockets();
        }
        Timer::after(EmbassyDuration::from_millis(5)).await;
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
                    }
                    pending = None;
                    pending_handle = Some(handle);
                    pending_ticks = 0;
                    pending_len = 0;
                    logged_first_rx = false;
                    tx_log_budget = 16;
                    crate::log!("net-shell: tcp established handle={}\n", handle.0);
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
