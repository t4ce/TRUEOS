extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::vnet as api;

#[derive(Clone, Copy, Debug)]
pub struct HttpRequestLine<'a> {
    pub method: &'a str,
    pub target: &'a str,
    pub version: &'a str,
}

#[derive(Clone, Debug)]
pub struct HttpRequest {
    method: String,
    target: String,
    version: String,
    raw: Vec<u8>,
    header_end: usize,
    keep_alive: bool,
}

impl HttpRequest {
    pub fn method(&self) -> &str {
        self.method.as_str()
    }

    pub fn target(&self) -> &str {
        self.target.as_str()
    }

    pub fn version(&self) -> &str {
        self.version.as_str()
    }

    pub fn header_bytes(&self) -> &[u8] {
        &self.raw[..self.header_end]
    }

    pub fn body_bytes(&self) -> &[u8] {
        &self.raw[self.header_end..]
    }

    pub fn raw_bytes(&self) -> &[u8] {
        self.raw.as_slice()
    }

    pub fn keep_alive(&self) -> bool {
        self.keep_alive
    }
}

#[derive(Default)]
struct HttpSession {
    req: Vec<u8>,
    pending_close: bool,
    pending_bytes: usize,
    sent_bytes: usize,
}

pub enum HttpServerEvent {
    None,
    RequestReady {
        handle: api::NetHandle,
        request: HttpRequest,
    },
    RequestTooLarge {
        handle: api::NetHandle,
    },
    BadRequest {
        handle: api::NetHandle,
    },
    Submit(api::Command),
    Error(&'static str),
}

pub struct HttpServer {
    listener_handle: Option<api::NetHandle>,
    sessions: BTreeMap<u32, HttpSession>,
    max_request_bytes: usize,
    listen_port: u16,
}

impl HttpServer {
    pub fn new(listen_port: u16, max_request_bytes: usize) -> Self {
        Self {
            listener_handle: None,
            sessions: BTreeMap::new(),
            max_request_bytes,
            listen_port,
        }
    }

    pub fn on_event(&mut self, ev: api::Event) -> HttpServerEvent {
        match ev {
            api::Event::Opened { handle, kind } => {
                if kind == api::SocketKind::Tcp {
                    self.listener_handle = Some(handle);
                }
                HttpServerEvent::None
            }
            api::Event::TcpEstablished { handle } => {
                self.sessions.entry(handle.0).or_default();
                HttpServerEvent::None
            }
            api::Event::TcpData { handle, data } => {
                let session = self.sessions.entry(handle.0).or_default();
                session.req.extend_from_slice(data.as_slice());

                if session.pending_bytes != 0 {
                    return HttpServerEvent::None;
                }

                if session.req.len() > self.max_request_bytes {
                    return HttpServerEvent::RequestTooLarge { handle };
                }

                let Some(header_end) = header_end(session.req.as_slice()) else {
                    return HttpServerEvent::None;
                };

                let header_bytes = &session.req[..header_end];
                let Some(req_line) = parse_request_line(header_bytes) else {
                    return HttpServerEvent::BadRequest { handle };
                };

                let content_len = content_length(header_bytes).unwrap_or(0);
                let total_needed = header_end.saturating_add(content_len);
                if total_needed > self.max_request_bytes {
                    return HttpServerEvent::RequestTooLarge { handle };
                }
                if session.req.len() < total_needed {
                    return HttpServerEvent::None;
                }

                let method = req_line.method.to_string();
                let target = req_line.target.to_string();
                let version = req_line.version.to_string();
                let keep_alive = keep_alive(header_bytes, req_line.version);
                let req = session.req[..total_needed].to_vec();
                if keep_alive {
                    session.req.drain(..total_needed);
                } else {
                    session.req.clear();
                }

                HttpServerEvent::RequestReady {
                    handle,
                    request: HttpRequest {
                        method,
                        target,
                        version,
                        raw: req,
                        header_end,
                        keep_alive,
                    },
                }
            }
            api::Event::Closed { handle } => {
                self.sessions.remove(&handle.0);
                if self.listener_handle == Some(handle) {
                    self.listener_handle = None;
                    return HttpServerEvent::Submit(api::Command::OpenTcpListen {
                        port: self.listen_port,
                    });
                }
                HttpServerEvent::None
            }
            api::Event::Error { msg } => {
                if msg == "bad handle" {
                    HttpServerEvent::None
                } else {
                    HttpServerEvent::Error(msg)
                }
            }
            api::Event::TcpSent { handle, len } => {
                if let Some(session) = self.sessions.get_mut(&handle.0)
                    && session.pending_bytes != 0
                {
                    session.sent_bytes = session.sent_bytes.saturating_add(len as usize);
                    if session.sent_bytes >= session.pending_bytes {
                        let should_close = session.pending_close;
                        session.pending_close = false;
                        session.pending_bytes = 0;
                        session.sent_bytes = 0;
                        if should_close {
                            return HttpServerEvent::Submit(api::Command::Close { handle });
                        }
                        if session.req.len() > self.max_request_bytes {
                            session.req.clear();
                            return HttpServerEvent::Submit(api::Command::Close { handle });
                        }
                    }
                }
                HttpServerEvent::None
            }
            api::Event::UdpPacket { .. }
            | api::Event::UdpPacketV6 { .. }
            | api::Event::IcmpReply { .. }
            | api::Event::IcmpReplyV6 { .. } => HttpServerEvent::None,
        }
    }

    pub fn mark_response(&mut self, handle: api::NetHandle, pending_bytes: usize, keep_alive: bool) {
        let session = self.sessions.entry(handle.0).or_default();
        session.pending_bytes = pending_bytes;
        session.sent_bytes = 0;
        session.pending_close = !keep_alive;
    }
}

pub fn parse_request_line(req: &[u8]) -> Option<HttpRequestLine<'_>> {
    let s = core::str::from_utf8(req).ok()?;
    let line_end = s.find("\r\n").or_else(|| s.find('\n')).unwrap_or(s.len());
    let line = s.get(..line_end)?;
    let mut it = line.split_whitespace();
    let method = it.next()?;
    let target = it.next()?;
    let version = it.next()?;
    Some(HttpRequestLine {
        method,
        target,
        version,
    })
}

pub fn query_param<'a>(target: &'a str, key: &str) -> Option<&'a str> {
    let (_, q) = target.split_once('?')?;
    for part in q.split('&') {
        if let Some((k, v)) = part.split_once('=')
            && k == key
        {
            return Some(v);
        }
    }
    None
}

pub fn path_only(target: &str) -> &str {
    target.split_once('?').map(|(p, _)| p).unwrap_or(target)
}

pub fn header_has_token(req: &[u8], key: &str, token: &str) -> bool {
    let Some(value) = find_header(req, key) else {
        return false;
    };
    value
        .split(',')
        .any(|part| part.trim().eq_ignore_ascii_case(token))
}

pub fn keep_alive(req: &[u8], version: &str) -> bool {
    match version {
        "HTTP/1.1" => !header_has_token(req, "Connection", "close"),
        "HTTP/1.0" => header_has_token(req, "Connection", "keep-alive"),
        _ => false,
    }
}

pub fn header_end(req: &[u8]) -> Option<usize> {
    let mut i = 0usize;
    while i + 3 < req.len() {
        if &req[i..i + 4] == b"\r\n\r\n" {
            return Some(i + 4);
        }
        i += 1;
    }
    let mut j = 0usize;
    while j + 1 < req.len() {
        if &req[j..j + 2] == b"\n\n" {
            return Some(j + 2);
        }
        j += 1;
    }
    None
}

pub fn find_header<'a>(req: &'a [u8], key: &str) -> Option<&'a str> {
    let s = core::str::from_utf8(req).ok()?;
    let mut lines = s.split('\n');
    let _ = lines.next()?;
    for line in lines {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':')
            && k.trim().eq_ignore_ascii_case(key)
        {
            return Some(v.trim());
        }
    }
    None
}

pub fn content_length(req: &[u8]) -> Option<usize> {
    let value = find_header(req, "Content-Length")?;
    value.parse::<usize>().ok()
}

pub fn url_decode(s: &str, max_len: usize) -> Option<String> {
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

pub fn queue_response_head(
    out: &mut Vec<api::Command>,
    handle: api::NetHandle,
    status: &str,
    content_type: &str,
    extra_headers: &str,
    body_len: u64,
    keep_alive: bool,
) -> usize {
    let mut header = String::new();
    header.push_str(status);
    header.push_str("Content-Type: ");
    header.push_str(content_type);
    header.push_str("\r\n");
    if !extra_headers.is_empty() {
        header.push_str(extra_headers);
    }
    header.push_str("Content-Length: ");
    header.push_str(format!("{}", body_len).as_str());
    header.push_str(if keep_alive {
        "\r\nConnection: keep-alive\r\n\r\n"
    } else {
        "\r\nConnection: close\r\n\r\n"
    });
    let body_len_usize = body_len.min(usize::MAX as u64) as usize;
    let pending = header.len().saturating_add(body_len_usize);
    queue_send_bytes(out, handle, header.as_bytes());
    pending
}

pub fn queue_send_bytes(out: &mut Vec<api::Command>, handle: api::NetHandle, bytes: &[u8]) {
    for chunk in bytes.chunks(api::MAX_MSG) {
        out.push(api::Command::SendTcp {
            handle,
            data: api::ByteBuf::from_slice_trunc(chunk),
        });
    }
}
