use alloc::{boxed::Box, format, vec::Vec};

use trueos_v::vnet as api;

use crate::net::adapter::{self, NetCommand, NetEndpoint, NetEvent, NetHandle, NetQueue, SocketKind};

pub struct VNet {
    owner: &'static str,
    cmds: &'static NetQueue<NetCommand>,
    events: &'static NetQueue<NetEvent>,
}

impl VNet {
    /// Create a new vnet client bound to a specific NIC index.
    ///
    /// `device_index` selects which NIC the adapter service routes this client's commands to.
    pub fn open(device_index: usize) -> Option<Self> {
        if crate::net::device_count() == 0 {
            return None;
        }
        if device_index >= crate::net::device_count() {
            return None;
        }

        let owner: &'static str = {
            let s = format!("vnet@{}", device_index);
            Box::leak(s.into_boxed_str())
        };

        let cmds = NetQueue::new_leaked("vnet-cmd", 256);
        let events = NetQueue::new_leaked("vnet-evt", 256);
        adapter::register_app_queues(owner, cmds, events);

        Some(Self { owner, cmds, events })
    }

    pub fn open_primary() -> Option<Self> {
        Self::open(0)
    }

    pub fn owner(&self) -> &'static str {
        self.owner
    }

    pub fn mac_address(&self) -> Option<api::MacAddr> {
        let idx = owner_device_index(self.owner)?;
        crate::net::mac_address_at(idx).map(api::MacAddr)
    }

    pub fn submit(&self, cmd: api::Command) -> Result<(), ()> {
        let cmd = to_kernel_cmd(cmd)?;
        self.cmds.push(cmd)
    }

    pub fn pop_event(&self) -> Option<api::Event> {
        self.events.drain(1).pop().and_then(from_kernel_event)
    }
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

fn to_kernel_endpoint(ep: api::EndpointV4) -> NetEndpoint {
    NetEndpoint {
        addr: ep.addr,
        port: ep.port,
    }
}

fn to_kernel_cmd(cmd: api::Command) -> Result<NetCommand, ()> {
    Ok(match cmd {
        api::Command::OpenUdp { port } => NetCommand::OpenUdp { port },
        api::Command::OpenTcpListen { port } => NetCommand::OpenTcpListen { port },
        api::Command::OpenTcpConnect { remote } => NetCommand::OpenTcpConnect {
            remote: to_kernel_endpoint(remote),
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
        api::Command::SendTcp { handle, data } => NetCommand::SendTcp {
            handle: NetHandle(handle.0),
            data: Vec::from(data.as_slice()),
        },
        api::Command::Close { handle } => NetCommand::Close {
            handle: NetHandle(handle.0),
        },
    })
}

fn from_kernel_kind(kind: SocketKind) -> api::SocketKind {
    match kind {
        SocketKind::Udp => api::SocketKind::Udp,
        SocketKind::Tcp => api::SocketKind::Tcp,
    }
}

fn from_kernel_event(ev: NetEvent) -> Option<api::Event> {
    Some(match ev {
        NetEvent::Opened { handle, kind } => api::Event::Opened {
            handle: api::NetHandle(handle.0),
            kind: from_kernel_kind(kind),
        },
        NetEvent::Closed { handle } => api::Event::Closed {
            handle: api::NetHandle(handle.0),
        },
        NetEvent::Error { msg } => api::Event::Error { msg },
        NetEvent::UdpPacket {
            handle,
            from,
            data,
        } => api::Event::UdpPacket {
            handle: api::NetHandle(handle.0),
            from: api::EndpointV4 {
                addr: from.addr,
                port: from.port,
            },
            data: api::ByteBuf::from_slice_trunc(&data[..]),
        },
        NetEvent::TcpEstablished { handle } => api::Event::TcpEstablished {
            handle: api::NetHandle(handle.0),
        },
        NetEvent::TcpData { handle, data } => api::Event::TcpData {
            handle: api::NetHandle(handle.0),
            data: api::ByteBuf::from_slice_trunc(&data[..]),
        },
        NetEvent::TcpSent { handle, len } => api::Event::TcpSent {
            handle: api::NetHandle(handle.0),
            len: len.min(u16::MAX as usize) as u16,
        },
    })
}
