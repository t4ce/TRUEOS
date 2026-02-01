use alloc::{boxed::Box, collections::VecDeque, format, string::String, vec::Vec};
use core::sync::atomic::{AtomicU32, Ordering};

pub mod dns;
pub mod https;

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

fn http_parse_target(req: &[u8]) -> Option<&str> {
    let s = core::str::from_utf8(req).ok()?;
    let line_end = s.find("\r\n").or_else(|| s.find('\n')).unwrap_or(s.len());
    let line = s.get(..line_end)?;
    let mut it = line.split_whitespace();
    let method = it.next()?;
    let target = it.next()?;
    if method != "GET" {
        return None;
    }
    Some(target)
}

fn http_query_param<'a>(target: &'a str, key: &str) -> Option<&'a str> {
    let (_, q) = target.split_once('?')?;
    for part in q.split('&') {
        if let Some((k, v)) = part.split_once('=') {
            if k == key {
                return Some(v);
            }
        }
    }
    None
}

fn http_url_decode(s: &str, max_len: usize) -> Option<String> {
    if s.len() > max_len.saturating_mul(3) {
        return None;
    }

    let mut out = String::new();
    let mut i = 0usize;
    let b = s.as_bytes();
    while i < b.len() {
        if out.len() >= max_len {
            return None;
        }
        match b[i] {
            b'+' => {
                out.push(' ');
                i += 1;
            }
            b'%' => {
                if i + 2 >= b.len() {
                    return None;
                }
                let hi = b[i + 1];
                let lo = b[i + 2];
                let hex = |c: u8| -> Option<u8> {
                    match c {
                        b'0'..=b'9' => Some(c - b'0'),
                        b'a'..=b'f' => Some(c - b'a' + 10),
                        b'A'..=b'F' => Some(c - b'A' + 10),
                        _ => None,
                    }
                };
                let v = (hex(hi)? << 4) | hex(lo)?;
                out.push(char::from(v));
                i += 3;
            }
            other => {
                out.push(char::from(other));
                i += 1;
            }
        }
    }
    Some(out)
}

fn http_sanitize_filename(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        match ch {
            '"' | '\\' | '\r' | '\n' => out.push('_'),
            _ => out.push(ch),
        }
    }
    if out.is_empty() {
        out.push_str("download.bin");
    }
    out
}

#[embassy_executor::task]
pub async fn http_trueosfs_task() {
    // Permanent FSM gating: do not start until the network is actually usable.
    crate::v::readiness::wait_for(crate::v::readiness::NET_GATEWAY_REACHABLE).await;

    // Permanent FSM gating: do not serve until a root filesystem is mounted.
    crate::v::readiness::wait_for(crate::v::readiness::TRUEOSFS_ROOT_MOUNTED).await;

    // Once the network is reachable, `open_primary()` should succeed; keep it strict.
    let vnet = loop {
        if let Some(v) = VNet::open_primary() {
            break v;
        }
        Timer::after(EmbassyDuration::from_millis(50)).await;
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
                api::Event::TcpData { handle, data } => {
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

                    let target = http_parse_target(data.as_slice()).unwrap_or("/");

                    // Under the permanent FSM model, a TRUEOSFS root must exist.
                    let disk = crate::v::fs::trueosfs::primary_root_handle();

                    let (status, content_type, extra_headers, body_bytes): (
                        &'static str,
                        &'static str,
                        String,
                        Vec<u8>,
                    ) = if target.starts_with("/dl") {
                        // Download endpoint: /dl?path=<urlencoded path>
                        match disk {
                            None => (
                                "HTTP/1.1 503 Service Unavailable\r\n",
                                "text/plain; charset=utf-8",
                                String::new(),
                                b"no TRUEOSFS mounted\n".to_vec(),
                            ),
                            Some(disk) => {
                                let mut extra_headers = String::new();
                                match http_query_param(target, "path") {
                                    None => (
                                        "HTTP/1.1 400 Bad Request\r\n",
                                        "text/plain; charset=utf-8",
                                        extra_headers,
                                        b"missing path\n".to_vec(),
                                    ),
                                    Some(enc_path) => match http_url_decode(enc_path, 240) {
                                        None => (
                                            "HTTP/1.1 400 Bad Request\r\n",
                                            "text/plain; charset=utf-8",
                                            extra_headers,
                                            b"bad path\n".to_vec(),
                                        ),
                                        Some(mut path) => {
                                            while path.starts_with('/') {
                                                path.remove(0);
                                            }
                                            if path.is_empty() || path.contains("..") {
                                                (
                                                    "HTTP/1.1 400 Bad Request\r\n",
                                                    "text/plain; charset=utf-8",
                                                    extra_headers,
                                                    b"bad path\n".to_vec(),
                                                )
                                            } else {
                                                match crate::v::fs::trueosfs::file_out_async(disk, path.as_str()).await {
                                                    Ok(Some(bytes)) => {
                                                        let fname_raw = path.rsplit('/').next().unwrap_or("download.bin");
                                                        let fname = http_sanitize_filename(fname_raw);
                                                        extra_headers.push_str("Content-Disposition: attachment; filename=\"");
                                                        extra_headers.push_str(fname.as_str());
                                                        extra_headers.push_str("\"\r\n");
                                                        (
                                                            "HTTP/1.1 200 OK\r\n",
                                                            "application/octet-stream",
                                                            extra_headers,
                                                            bytes,
                                                        )
                                                    }
                                                    Ok(None) => (
                                                        "HTTP/1.1 404 Not Found\r\n",
                                                        "text/plain; charset=utf-8",
                                                        extra_headers,
                                                        b"not found\n".to_vec(),
                                                    ),
                                                    Err(_) => (
                                                        "HTTP/1.1 500 Internal Server Error\r\n",
                                                        "text/plain; charset=utf-8",
                                                        extra_headers,
                                                        b"read failed\n".to_vec(),
                                                    ),
                                                }
                                            }
                                        }
                                    },
                                }
                            }
                        }
                    } else {
                        // Build the HTML tree once per request (best-effort).
                        let tree_html = match disk {
                            None => None,
                            Some(disk) => match crate::v::fs::trueosfs::html_tree_async(disk, HTTP_TRUEOSFS_MAX_ENTRIES).await {
                                Ok(v) => v,
                                Err(_) => None,
                            },
                        };

                        let js = r#"<style>
body{font-family:system-ui,-apple-system,Segoe UI,sans-serif;max-width:980px;margin:2rem auto;padding:0 1rem}
ul{line-height:1.35}
li{margin:0.15rem 0}
a{color:#0b66ff;text-decoration:none}
a:hover{text-decoration:underline}
</style>
<script>
(()=>{
  function labelForLi(li){
    for(const n of li.childNodes){
      if(n.nodeType===Node.TEXT_NODE){
        const t=(n.textContent||"").trim();
        if(t) return t;
      }
    }
    return "";
  }
  function parentLi(li){
    let p=li.parentElement;
    while(p && p.tagName==="UL") p=p.parentElement;
    return (p && p.tagName==="LI") ? p : null;
  }
  function buildPath(li){
    const parts=[];
    let cur=li;
    while(cur){
      const label=labelForLi(cur);
      if(label && label!=="/"){
        let seg=label;
        if(seg.endsWith("/")) seg=seg.slice(0,-1);
        if(seg) parts.push(seg);
      }
      cur=parentLi(cur);
    }
    return parts.reverse().join("/");
  }
  for(const li of document.querySelectorAll("ul li")){
    if(li.querySelector("a")) continue;
    const label=labelForLi(li);
    if(!label || label==="/" || label.endsWith("/")) continue;
    const path=buildPath(li);
    if(!path) continue;
    const a=document.createElement("a");
    a.href="/dl?path="+encodeURIComponent(path);
    a.textContent=label;
    a.setAttribute("download", label);
    const firstText=[...li.childNodes].find(n=>n.nodeType===Node.TEXT_NODE && (n.textContent||"").trim());
    if(firstText){ li.insertBefore(a, firstText); li.removeChild(firstText); }
    else { li.insertBefore(a, li.firstChild); }
  }
})();
</script>"#;

                        let body = if let Some(tree) = tree_html {
                            format!(
                                "<!doctype html><html><head><meta charset=\"utf-8\"><title>TRUEOSFS</title>{}</head><body><h1>TRUEOSFS</h1><p>Click a file to download.</p>{}</body></html>",
                                js, tree
                            )
                        } else {
                            format!(
                                "<!doctype html><html><head><meta charset=\"utf-8\"><title>TRUEOSFS</title>{}</head><body><h1>TRUEOSFS</h1><p>(no TRUEOSFS mounted)</p></body></html>",
                                js
                            )
                        };

                        (
                            "HTTP/1.1 200 OK\r\n",
                            "text/html; charset=utf-8",
                            String::new(),
                            body.into_bytes(),
                        )
                    };

                    let mut header = String::new();
                    header.push_str(status);
                    header.push_str("Content-Type: ");
                    header.push_str(content_type);
                    header.push_str("\r\n");
                    if !extra_headers.is_empty() {
                        header.push_str(extra_headers.as_str());
                    }
                    header.push_str("Content-Length: ");
                    header.push_str(format!("{}", body_bytes.len()).as_str());
                    header.push_str("\r\nConnection: close\r\n\r\n");

                    // Send headers + body in MAX_MSG chunks.
                    for chunk in header.as_bytes().chunks(api::MAX_MSG) {
                        let _ = vnet.submit(api::Command::SendTcp {
                            handle,
                            data: api::ByteBuf::from_slice_trunc(chunk),
                        });
                        Timer::after(EmbassyDuration::from_millis(1)).await;
                    }
                    for chunk in body_bytes.as_slice().chunks(api::MAX_MSG) {
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
