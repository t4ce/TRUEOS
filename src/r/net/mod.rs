use alloc::{boxed::Box, collections::VecDeque, format, vec::Vec};
use core::sync::atomic::{AtomicU32, Ordering};

pub mod cli;
pub mod discovery;
pub mod dns;
pub mod dns_request_broker;
pub mod http_request_cabi;
pub mod https;
pub mod ipp_print;
pub mod mail_config;
pub mod ports;
pub mod printer;
pub mod pwg_raster;
pub mod request_stream;
pub mod smtp_cabi;
pub mod socket_cabi;
pub mod srv;
pub mod trueos_peer;
pub mod udp;
pub mod vlayer;
pub(crate) mod wall;

pub use cli::ntp;
pub use srv::sntp;

use v::vnet as api;

use spin::Mutex;

use crate::net::adapter::{
    self, NetCommand, NetEndpoint, NetEndpointV6, NetEvent, NetHandle, NetQueue, SocketKind,
};

pub(crate) use crate::net::adapter::SharedNetPayload;

pub type Queue<T> = adapter::NetQueue<T>;

static VNET_SEQ: AtomicU32 = AtomicU32::new(1);
const VNET_CMD_QUEUE_DEPTH: usize = crate::allcaps::net::VNET_CMD_QUEUE_DEPTH;
const VNET_EVENT_QUEUE_DEPTH_DEFAULT: usize = crate::allcaps::net::VNET_EVENT_QUEUE_DEPTH_DEFAULT;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetProfile {
    nic_index: Option<usize>,
}

impl Default for NetProfile {
    fn default() -> Self {
        Self { nic_index: None }
    }
}

impl NetProfile {
    pub const fn new() -> Self {
        Self { nic_index: None }
    }

    pub const fn with_nic(nic_index: usize) -> Self {
        Self {
            nic_index: Some(nic_index),
        }
    }

    pub const fn nic_index(self) -> Option<usize> {
        self.nic_index
    }

    pub fn resolve_device_index(self) -> Option<usize> {
        let count = crate::net::device_count();
        if count == 0 {
            return None;
        }

        match self.nic_index {
            Some(idx) if idx < count => Some(idx),
            Some(_) => None,
            None => Some(crate::net::primary_device_index()),
        }
    }
}

pub struct VNet {
    owner: &'static str,
    cmds: &'static NetQueue<NetCommand>,
    events: &'static NetQueue<NetEvent>,
    pending: Mutex<VecDeque<api::Event>>,
}

impl VNet {
    /// Create a new vnet client bound to a specific NIC index with explicit
    /// event queue depth.
    ///
    /// `device_index` selects which NIC the adapter service routes this client's commands to.
    pub fn open_with_event_queue_depth(
        device_index: usize,
        event_queue_depth: usize,
    ) -> Option<Self> {
        Self::open_with_queue_options(device_index, event_queue_depth, true)
    }

    /// Open a client whose consumer polls its own event queue on a timer.
    pub(crate) fn open_polled(device_index: usize) -> Option<Self> {
        Self::open_with_queue_options(device_index, VNET_EVENT_QUEUE_DEPTH_DEFAULT, false)
    }

    fn open_with_queue_options(
        device_index: usize,
        event_queue_depth: usize,
        wake_blueprint_io: bool,
    ) -> Option<Self> {
        if crate::net::device_count() == 0 {
            return None;
        }
        if device_index >= crate::net::device_count() {
            return None;
        }

        // Must be unique per call: `register_app_queues` ignores duplicates and would
        // otherwise leave our new queues undrained.
        let seq = VNET_SEQ.fetch_add(1, Ordering::Relaxed);

        let selector = if let Some((bus, slot, func)) = crate::net::bdf_at(device_index) {
            format!("{:02x}:{:02x}.{}", bus, slot, func)
        } else if let Some((vid, pid)) = crate::net::device_id_at(device_index) {
            format!("{:04x}:{:04x}", vid, pid)
        } else {
            format!("{}", device_index)
        };

        let owner: &'static str = {
            let s = format!("vnet-{}@{}", seq, selector);
            Box::leak(s.into_boxed_str())
        };

        let cmds_name: &'static str = {
            let s = format!("{}-cmd", owner);
            Box::leak(s.into_boxed_str())
        };
        let events_name: &'static str = {
            let s = format!("{}-evt", owner);
            Box::leak(s.into_boxed_str())
        };

        let depth = event_queue_depth.max(64);
        let cmds = NetQueue::new_leaked(cmds_name, VNET_CMD_QUEUE_DEPTH);
        let events = NetQueue::new_leaked(events_name, depth);
        adapter::register_app_queues_with_io_wake(owner, cmds, events, wake_blueprint_io);

        let vnet = Self {
            owner,
            cmds,
            events,
            pending: Mutex::new(VecDeque::new()),
        };

        if cfg!(debug_assertions) {
            vnet.exercise_api();
        }

        Some(vnet)
    }

    /// Create a new vnet client bound to a specific NIC index.
    pub fn open(device_index: usize) -> Option<Self> {
        Self::open_with_event_queue_depth(device_index, VNET_EVENT_QUEUE_DEPTH_DEFAULT)
    }

    pub fn open_with_profile(profile: NetProfile) -> Option<Self> {
        let device_index = profile.resolve_device_index()?;
        Self::open(device_index)
    }

    pub fn open_default() -> Option<Self> {
        Self::open_with_profile(NetProfile::default())
    }

    pub fn open_primary() -> Option<Self> {
        Self::open_default()
    }

    pub fn owner(&self) -> &'static str {
        self.owner
    }

    pub fn mac_address(&self) -> Option<api::MacAddr> {
        let idx = crate::net::device_index_from_owner(self.owner)?;
        crate::net::mac_address_at(idx).map(api::MacAddr)
    }

    fn exercise_api(&self) {
        let owner = self.owner();
        let mac = self.mac_address();
        let _ = api::EndpointV4::new([127, 0, 0, 1], 0);

        if crate::log_os::flags::VNET_EXERCISE_LOGS {
            match mac {
                Some(api::MacAddr(bytes)) => {
                    crate::log!(
                        "vnet: exercise owner={} mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}\n",
                        owner,
                        bytes[0],
                        bytes[1],
                        bytes[2],
                        bytes[3],
                        bytes[4],
                        bytes[5]
                    );
                }
                None => {
                    crate::log!("vnet: exercise owner={} mac=none\n", owner);
                }
            }
        }
    }

    pub fn submit(&self, cmd: api::Command) -> Result<(), ()> {
        if matches!(cmd, api::Command::Close { .. }) {
            return self.submit_kernel(to_kernel_cmd(cmd)?);
        }
        self.submit_for_caller(cmd, wall::Caller::current().ok_or(())?)
    }

    pub(crate) fn submit_for_actor(
        &self,
        cmd: api::Command,
        actor: crate::r::fs::trueosfs::authorized::Actor,
    ) -> Result<(), ()> {
        self.submit_for_caller(cmd, wall::Caller::from_actor(actor).ok_or(())?)
    }

    pub(crate) fn submit_for_caller(
        &self,
        cmd: api::Command,
        caller: wall::Caller,
    ) -> Result<(), ()> {
        use core::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
        use wall::Transport::{Tcp, Udp};
        let actor = caller.actor();
        let authorized = match cmd {
            api::Command::OpenTcpListen { port } => return self.submit_for_caller(
                api::Command::OpenTcpListenAt { local: (Ipv4Addr::UNSPECIFIED, port).into() }, caller,
            ),
            api::Command::OpenTcpListenAt { local } => {
                crate::access_key_db::authorize_tcp_listen(actor, local)?;
                let local = caller.restrict_bind(Tcp, local).ok_or(())?;
                NetCommand::OpenTcpListenForActor { actor, local }
            }
            api::Command::OpenTcpConnect { remote } => return self.submit_tcp_connect_for_caller(
                None, (Ipv4Addr::from(remote.addr), remote.port).into(), caller,
            ),
            api::Command::OpenTcpConnectV6 { remote } => return self.submit_tcp_connect_for_caller(
                None, (Ipv6Addr::from(remote.addr), remote.port).into(), caller,
            ),
            api::Command::OpenUdp { port } => return self.open_udp_for_caller(
                SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)), port == 0, caller,
            ),
            api::Command::OpenTun { .. } | api::Command::SendIpPacket { .. }
            | api::Command::IcmpEcho { .. } | api::Command::IcmpEchoV6 { .. }
                if !matches!(caller, wall::Caller::Kernel) => return Err(()),
            api::Command::SendUdp { .. } | api::Command::SendUdpV6 { .. }
                if !caller.permits_socket(Udp) => return Err(()),
            api::Command::SendTcp { .. } | api::Command::FinishTcp { .. }
                if !caller.permits_socket(Tcp) => return Err(()),
            other => return self.submit_kernel(to_kernel_cmd(other)?),
        };
        self.submit_kernel(authorized)
    }

    pub(crate) fn open_udp_for_caller(
        &self,
        local: core::net::SocketAddr,
        ephemeral: bool,
        caller: wall::Caller,
    ) -> Result<(), ()> {
        let allowed = if ephemeral {
            caller.permits_socket(wall::Transport::Udp)
        } else {
            caller.permits_bind(wall::Transport::Udp, local)
        };
        if !allowed {
            return Err(());
        }
        self.submit_kernel(NetCommand::OpenUdpForCaller {
            caller,
            local,
            ephemeral,
            tx_buffer_bytes: None,
        })
    }

    pub(crate) fn open_esp32_rtp_mic(&self) -> Result<(), ()> {
        if !matches!(wall::Caller::current(), Some(wall::Caller::Kernel)) {
            return Err(());
        }
        self.submit_kernel(NetCommand::OpenEsp32RtpMic)
    }

    pub(crate) fn submit_tcp_connect_from_for_actor(
        &self,
        local: core::net::SocketAddr,
        remote: core::net::SocketAddr,
        actor: crate::r::fs::trueosfs::authorized::Actor,
    ) -> Result<(), ()> {
        self.submit_tcp_connect_for_caller(
            Some(local),
            remote,
            wall::Caller::from_actor(actor).ok_or(())?,
        )
    }

    pub(crate) fn submit_tcp_connect_for_caller(
        &self,
        local: Option<core::net::SocketAddr>,
        remote: core::net::SocketAddr,
        caller: wall::Caller,
    ) -> Result<(), ()> {
        if !caller.permits_outbound(wall::Transport::Tcp, remote) {
            return Err(());
        }
        self.submit_kernel(NetCommand::OpenTcpConnectForActor {
            actor: caller.actor(),
            local,
            remote,
        })
    }

    pub(crate) fn submit_tcp_connect_from(
        &self, local: core::net::SocketAddr, remote: core::net::SocketAddr,
    ) -> Result<(), ()> {
        self.submit_kernel(NetCommand::OpenTcpConnectFrom { local, remote })
    }

    fn submit_kernel(&self, mut cmd: NetCommand) -> Result<(), ()> {
        // Under bursty load, the per-owner command queue can be briefly full.
        // Retry a few times to avoid silently dropping critical commands like
        // the first TCP send right after connect. A failed enqueue returns the
        // original command so payload-bearing commands can be retried without
        // cloning their Vec.
        for _ in 0..8 {
            match self.cmds.try_push(cmd) {
                Ok(()) => return Ok(()),
                Err(returned) => cmd = returned,
            }
            core::hint::spin_loop();
        }

        crate::log!("vnet: submit drop owner={}\n", self.owner);
        Err(())
    }

    pub(super) fn open_udp_with_tx_buffer(
        &self,
        port: u16,
        tx_buffer_bytes: usize,
    ) -> Result<(), ()> {
        self.submit_kernel(NetCommand::OpenUdpWithTxBuffer {
            port,
            tx_buffer_bytes,
        })
    }

    pub(super) fn send_udp_checked(
        &self,
        handle: api::NetHandle,
        remote: api::EndpointV4,
        receipt: u32,
        data: &SharedNetPayload,
    ) -> Result<(), ()> {
        self.submit_kernel(NetCommand::SendUdpChecked {
            handle: NetHandle(handle.0),
            remote: to_kernel_endpoint(remote),
            receipt,
            data: data.clone(),
        })
    }

    /// Pop the result for one checked UDP command without exposing the
    /// kernel-only receipt protocol through the public vnet ABI.
    pub(super) fn pop_udp_send_result(
        &self,
        handle: api::NetHandle,
        receipt: u32,
    ) -> Option<Result<(), &'static str>> {
        adapter::pop_checked_udp_send_result(self.owner, NetHandle(handle.0), receipt)
    }

    pub fn send_tcp_all(&self, handle: api::NetHandle, data: &[u8]) -> Result<(), ()> {
        for chunk in data.chunks(api::MAX_MSG) {
            self.submit(api::Command::SendTcp {
                handle,
                data: api::ByteBuf::from_slice_trunc(chunk),
            })?;
        }
        Ok(())
    }

    /// Submit exactly one chunk. Failure consumes zero bytes, so callers can
    /// retain an exact ciphertext offset without replaying an accepted prefix.
    pub(crate) fn try_send_tcp_chunk(&self, handle: api::NetHandle, data: &[u8]) -> Result<(), ()> {
        self.try_send_tcp_chunk_for_caller(handle, data, wall::Caller::current().ok_or(())?)
    }

    pub(crate) fn try_send_tcp_chunk_for_caller(
        &self,
        handle: api::NetHandle,
        data: &[u8],
        caller: wall::Caller,
    ) -> Result<(), ()> {
        if data.len() > api::MAX_MSG { return Err(()); }
        self.submit_for_caller(api::Command::SendTcp {
            handle, data: api::ByteBuf::from_slice_trunc(data),
        }, caller)
    }

    /// Retire this client's old sockets and events before its queue pair is
    /// reused. The caller must stop producers before starting this fence.
    pub(crate) async fn reset_for_reuse(&self) {
        let complete = alloc::sync::Arc::new(core::sync::atomic::AtomicBool::new(false));
        let mut command = NetCommand::ResetOwner { complete: complete.clone() };
        loop {
            match self.cmds.try_push(command) {
                Ok(()) => break,
                Err(returned) => command = returned,
            }
            trueos_time::Timer::after(trueos_time::Duration::from_millis(1)).await;
        }
        while !complete.load(core::sync::atomic::Ordering::Acquire) {
            trueos_time::Timer::after(trueos_time::Duration::from_millis(1)).await;
        }
        self.pending.lock().clear();
    }

    pub fn pop_event(&self) -> Option<api::Event> {
        if let Some(ev) = self.pending.lock().pop_front() {
            return Some(ev);
        }

        let ev = self.events.pop()?;
        match ev {
            // TCP is a byte stream; don't truncate payloads. Split into multiple
            // MAX_MSG chunks and queue the remainder.
            NetEvent::TcpData { handle, data } => {
                let mut chunks = data.chunks(api::MAX_MSG);
                let first = chunks.next()?;
                let mut pending = self.pending.lock();
                for chunk in chunks {
                    pending.push_back(api::Event::TcpData {
                        handle: api::NetHandle(handle.0),
                        data: api::ByteBuf::from_slice_trunc(chunk),
                    });
                }
                Some(api::Event::TcpData {
                    handle: api::NetHandle(handle.0),
                    data: api::ByteBuf::from_slice_trunc(first),
                })
            }

            // The public ABI carries a u16 count, while one adapter flush may
            // accept more than u16::MAX bytes. Preserve the full count as an
            // ordered series of events instead of silently truncating it.
            NetEvent::TcpSent { handle, len } => {
                let first = len.min(u16::MAX as usize) as u16;
                let mut remaining = len.saturating_sub(first as usize);
                let mut pending = self.pending.lock();
                while remaining != 0 {
                    let part = remaining.min(u16::MAX as usize) as u16;
                    pending.push_back(api::Event::TcpSent {
                        handle: api::NetHandle(handle.0),
                        len: part,
                    });
                    remaining -= part as usize;
                }
                Some(api::Event::TcpSent {
                    handle: api::NetHandle(handle.0),
                    len: first,
                })
            }

            // UDP is a datagram; keep the current truncation behavior.
            other => from_kernel_event(other),
        }
    }
}

fn to_kernel_endpoint(ep: api::EndpointV4) -> NetEndpoint {
    NetEndpoint {
        addr: ep.addr,
        port: ep.port,
    }
}

fn to_kernel_endpoint_v6(ep: api::EndpointV6) -> NetEndpointV6 {
    NetEndpointV6 {
        addr: ep.addr,
        port: ep.port,
    }
}

fn to_kernel_cmd(cmd: api::Command) -> Result<NetCommand, ()> {
    Ok(match cmd {
        api::Command::OpenTun {
            ipv4,
            ipv4_prefix_len,
            ipv6,
            ipv6_prefix_len,
            mtu,
        } => NetCommand::OpenTun {
            ipv4,
            ipv4_prefix_len,
            ipv6,
            ipv6_prefix_len,
            mtu,
        },
        api::Command::OpenUdp { port } => NetCommand::OpenUdp { port },
        api::Command::OpenTcpListen { port } => NetCommand::OpenTcpListen { port },
        api::Command::OpenTcpListenAt { local } => NetCommand::OpenTcpListenAt { local },
        api::Command::OpenTcpConnect { remote } => NetCommand::OpenTcpConnect {
            remote: to_kernel_endpoint(remote),
        },
        api::Command::OpenTcpConnectV6 { remote } => NetCommand::OpenTcpConnectV6 {
            remote: to_kernel_endpoint_v6(remote),
        },
        api::Command::SendUdp {
            handle,
            remote,
            data,
        } => NetCommand::SendUdp {
            handle: NetHandle(handle.0),
            remote: to_kernel_endpoint(remote),
            data: Vec::from(data.as_slice()),
        },
        api::Command::SendUdpV6 {
            handle,
            remote,
            data,
        } => NetCommand::SendUdpV6 {
            handle: NetHandle(handle.0),
            remote: to_kernel_endpoint_v6(remote),
            data: Vec::from(data.as_slice()),
        },
        api::Command::SendTcp { handle, data } => NetCommand::SendTcp {
            handle: NetHandle(handle.0),
            data: Vec::from(data.as_slice()),
        },
        api::Command::SendIpPacket { handle, packet } => NetCommand::SendIpPacket {
            handle: NetHandle(handle.0),
            packet: Vec::from(packet.as_slice()),
        },
        api::Command::FinishTcp { handle } => NetCommand::FinishTcp {
            handle: NetHandle(handle.0),
        },
        api::Command::Close { handle } => NetCommand::Close {
            handle: NetHandle(handle.0),
        },
        api::Command::IcmpEcho { target, seq, data } => NetCommand::IcmpEcho {
            target,
            seq,
            data: Vec::from(data.as_slice()),
        },
        api::Command::IcmpEchoV6 { target, seq, data } => NetCommand::IcmpEchoV6 {
            target,
            seq,
            data: Vec::from(data.as_slice()),
        },
    })
}

fn from_kernel_kind(kind: SocketKind) -> api::SocketKind {
    match kind {
        SocketKind::Udp => api::SocketKind::Udp,
        SocketKind::Tcp => api::SocketKind::Tcp,
        SocketKind::Tun => api::SocketKind::Tun,
    }
}

fn from_kernel_event(ev: NetEvent) -> Option<api::Event> {
    Some(match ev {
        NetEvent::Opened { handle, kind } => api::Event::Opened {
            handle: api::NetHandle(handle.0),
            kind: from_kernel_kind(kind),
        },
        NetEvent::TcpReadClosed { handle } => api::Event::TcpReadClosed {
            handle: api::NetHandle(handle.0),
        },
        NetEvent::Closed { handle } => api::Event::Closed {
            handle: api::NetHandle(handle.0),
        },
        NetEvent::Error { msg } => api::Event::Error { msg },
        NetEvent::UdpPacket { handle, from, data } => api::Event::UdpPacket {
            handle: api::NetHandle(handle.0),
            from: api::EndpointV4 {
                addr: from.addr,
                port: from.port,
            },
            data: api::ByteBuf::from_slice_trunc(&data[..]),
        },
        NetEvent::UdpPacketV6 { handle, from, data } => api::Event::UdpPacketV6 {
            handle: api::NetHandle(handle.0),
            from: api::EndpointV6 {
                addr: from.addr,
                port: from.port,
            },
            data: api::ByteBuf::from_slice_trunc(&data[..]),
        },
        NetEvent::TcpEstablished {
            handle,
            peer,
            peer6,
        } => api::Event::TcpEstablished {
            handle: api::NetHandle(handle.0),
            peer: peer.map(|peer| api::EndpointV4 {
                addr: peer.addr,
                port: peer.port,
            }),
            peer6: peer6.map(|peer| api::EndpointV6 {
                addr: peer.addr,
                port: peer.port,
            }),
        },
        NetEvent::TcpData { handle, data } => api::Event::TcpData {
            handle: api::NetHandle(handle.0),
            data: api::ByteBuf::from_slice_trunc(&data[..]),
        },
        NetEvent::TcpSent { handle, len } => api::Event::TcpSent {
            handle: api::NetHandle(handle.0),
            len: len.min(u16::MAX as usize) as u16,
        },
        NetEvent::IpPacket { handle, packet } => api::Event::IpPacket {
            handle: api::NetHandle(handle.0),
            packet: api::ByteBuf::from_slice_trunc(&packet[..]),
        },
        NetEvent::IcmpReply {
            from,
            seq,
            rtt_ms,
            data,
        } => api::Event::IcmpReply {
            from,
            seq,
            rtt_ms,
            data: api::ByteBuf::from_slice_trunc(&data[..]),
        },
        NetEvent::IcmpReplyV6 {
            from,
            seq,
            rtt_ms,
            data,
        } => api::Event::IcmpReplyV6 {
            from,
            seq,
            rtt_ms,
            data: api::ByteBuf::from_slice_trunc(&data[..]),
        },
    })
}

pub(crate) mod http_content;
