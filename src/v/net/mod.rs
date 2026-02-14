use alloc::{boxed::Box, collections::VecDeque, format, vec::Vec};
use core::sync::atomic::{AtomicU32, Ordering};

pub mod dns;
pub mod ftp;
pub mod https;
pub mod ping;
pub mod ws;
pub mod wss;

use trueos_v::vnet as api;

use spin::Mutex;

use crate::net::adapter::{self, NetCommand, NetEndpoint, NetEvent, NetHandle, NetQueue, SocketKind};

pub type Queue<T> = adapter::NetQueue<T>;

static VNET_SEQ: AtomicU32 = AtomicU32::new(1);
const VNET_CMD_QUEUE_DEPTH: usize = 256;
const VNET_EVENT_QUEUE_DEPTH_DEFAULT: usize = 256;

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
  pub fn open_with_event_queue_depth(device_index: usize, event_queue_depth: usize) -> Option<Self> {
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
    } else if let Some((vid, pid)) = crate::net::pci_id_at(device_index) {
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
    adapter::register_app_queues(owner, cmds, events);

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

  pub fn open_primary() -> Option<Self> {
    Self::open(crate::net::primary_device_index())
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

  pub fn submit(&self, cmd: api::Command) -> Result<(), ()> {
    let cmd = to_kernel_cmd(cmd)?;
    self.cmds.push(cmd)
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
    api::Command::IcmpEcho { target, seq, data } => NetCommand::IcmpEcho {
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
    NetEvent::UdpPacket { handle, from, data } => api::Event::UdpPacket {
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
  })
}
