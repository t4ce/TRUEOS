use alloc::{boxed::Box, collections::VecDeque, vec, vec::Vec};
use core::sync::atomic::{AtomicU32, Ordering};

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
        crate::net::pop_rx_packet()
            .map(|packet| (AdapterRxToken { buffer: packet }, AdapterTxToken))
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
        if crate::net::transmit_packet(&buf[..]).is_err() {
            log!("net: TX busy, dropping {}-byte frame.\n", len);
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
    }

    fn poll_icmp(&mut self) {
        let mut buf = [0u8; 2048];
        let socket = self.sockets.get_mut::<icmp::Socket>(self.icmp);
        while socket.can_recv() {
            let Ok((len, from)) = socket.recv_slice(&mut buf) else { break };
            let Ok(pkt) = Icmpv4Packet::new_checked(&buf[..len]) else { continue };
            let Ok(repr) = Icmpv4Repr::parse(&pkt, &ChecksumCapabilities::ignored()) else { continue };

            if let Icmpv4Repr::EchoRequest { ident, seq_no, data } = repr {
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
