use alloc::{boxed::Box, collections::VecDeque, format, string::String, vec::Vec};
use core::sync::atomic::{AtomicU32, Ordering};

use trueos_v::vnet as api;

use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

use crate::net::adapter::{self, NetCommand, NetEndpoint, NetEvent, NetHandle, NetQueue, SocketKind};

pub type Queue<T> = adapter::NetQueue<T>;

#[inline]
pub fn net_shell_read_byte() -> Option<u8> {
    adapter::net_shell_read_byte()
}

#[inline]
pub fn net_shell_write_bytes(bytes: &[u8]) {
    adapter::net_shell_write_bytes(bytes)
}

static VNET_SEQ: AtomicU32 = AtomicU32::new(1);

pub struct VNet {
    owner: &'static str,
    cmds: &'static NetQueue<NetCommand>,
    events: &'static NetQueue<NetEvent>,
    pending: Mutex<VecDeque<api::Event>>,
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

        // Must be unique per call: `register_app_queues` ignores duplicates and would
        // otherwise leave our new queues undrained.
        let seq = VNET_SEQ.fetch_add(1, Ordering::Relaxed);

        let owner: &'static str = {
            let s = format!("vnet-{}@{}", seq, device_index);
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

        let cmds = NetQueue::new_leaked(cmds_name, 256);
        let events = NetQueue::new_leaked(events_name, 256);
        adapter::register_app_queues(owner, cmds, events);

        Some(Self {
            owner,
            cmds,
            events,
            pending: Mutex::new(VecDeque::new()),
        })
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
        if let Some(ev) = self.pending.lock().pop_front() {
            return Some(ev);
        }

        let ev = self.events.drain(1).pop()?;
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

const HTTP_TRUEOSFS_TCP_PORT: u16 = 80;
const HTTP_TRUEOSFS_MAX_ENTRIES: usize = 256;

#[embassy_executor::task]
pub async fn http_trueosfs_task() {
    let Some(vnet) = VNet::open_primary() else {
        crate::log!("http-trueosfs: disabled (no NIC)\n");
        return;
    };

    if vnet.submit(api::Command::OpenTcpListen {
        port: HTTP_TRUEOSFS_TCP_PORT,
    }).is_err() {
        crate::log!("http-trueosfs: listen submit failed\n");
        return;
    }

    crate::log!(
        "http-trueosfs: listening on tcp {} (hostfwd localhost:8080 -> guest:80)\n",
        HTTP_TRUEOSFS_TCP_PORT
    );

    let mut listener_handle: Option<api::NetHandle> = None;
    let mut active_handle: Option<api::NetHandle> = None;
    let mut sent_for_active: bool = false;

    loop {
        while let Some(ev) = vnet.pop_event() {
            match ev {
                api::Event::Opened { handle, kind } => {
                    if kind == api::SocketKind::Tcp {
                        listener_handle = Some(handle);
                    }
                }
                api::Event::TcpEstablished { handle } => {
                    active_handle = Some(handle);
                    sent_for_active = false;
                }
                api::Event::TcpData { handle, .. } => {
                    if active_handle.is_none() {
                        active_handle = Some(handle);
                        sent_for_active = false;
                    }
                    if active_handle != Some(handle) {
                        continue;
                    }
                    if sent_for_active {
                        continue;
                    }
                    sent_for_active = true;

                    // Build the HTML tree once per request (best-effort).
                    let tree_html = match crate::v::fs::trueosfs::primary_root_handle() {
                        None => None,
                        Some(disk) => match crate::v::fs::trueosfs::html_tree_async(disk, HTTP_TRUEOSFS_MAX_ENTRIES).await {
                            Ok(v) => v,
                            Err(_) => None,
                        },
                    };

                    let body = if let Some(tree) = tree_html {
                        format!(
                            "<!doctype html><html><head><meta charset=\"utf-8\"><title>TRUEOSFS</title></head><body><h1>TRUEOSFS</h1>{}</body></html>",
                            tree
                        )
                    } else {
                        String::from(
                            "<!doctype html><html><head><meta charset=\"utf-8\"><title>TRUEOSFS</title></head><body><h1>TRUEOSFS</h1><p>(no TRUEOSFS mounted)</p></body></html>",
                        )
                    };

                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.as_bytes().len()
                    );

                    // Send headers + body in MAX_MSG chunks.
                    for chunk in header.as_bytes().chunks(api::MAX_MSG) {
                        let _ = vnet.submit(api::Command::SendTcp {
                            handle,
                            data: api::ByteBuf::from_slice_trunc(chunk),
                        });
                        Timer::after(EmbassyDuration::from_millis(1)).await;
                    }
                    for chunk in body.as_bytes().chunks(api::MAX_MSG) {
                        let _ = vnet.submit(api::Command::SendTcp {
                            handle,
                            data: api::ByteBuf::from_slice_trunc(chunk),
                        });
                        Timer::after(EmbassyDuration::from_millis(1)).await;
                    }

                    let _ = vnet.submit(api::Command::Close { handle });
                }
                api::Event::Closed { handle } => {
                    if active_handle == Some(handle) {
                        active_handle = None;
                        sent_for_active = false;
                    }

                    // If the listener handle closes (or smoltcp collapses listen/conn handles), relisten.
                    if listener_handle == Some(handle) {
                        listener_handle = None;
                        let _ = vnet.submit(api::Command::OpenTcpListen {
                            port: HTTP_TRUEOSFS_TCP_PORT,
                        });
                    }
                }
                api::Event::Error { msg } => {
                    crate::log!("http-trueosfs: error {}\n", msg);
                }
                api::Event::TcpSent { .. } => {}
                api::Event::UdpPacket { .. } => {}
            }
        }

        Timer::after(EmbassyDuration::from_millis(10)).await;
    }
}
