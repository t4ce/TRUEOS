extern crate alloc;
extern crate std;

use alloc::{boxed::Box, string::String as AllocString, string::ToString, vec::Vec};
use core::{
    convert::Infallible,
    pin::Pin,
    sync::atomic::{AtomicU16, Ordering},
    task::{Context, Poll},
};
use std::{future::poll_fn, io};

use embassy_time::{Duration as EmbassyDuration, Timer};
use hyper::{
    body::{Body, Bytes, Frame, Incoming, SizeHint},
    header,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream, ReadHalf, WriteHalf};
use trueos_chat::{ChatConfig, ChatHub, ChatMethod, ChatRequest};
use v::vnet as api;

use crate::{allports::services::CHAT_HTTP_TCP_PORT, r::net::VNet, t::net::hyper_io::HyperTokioIo};

const CHAT_HTTP_BODY_MAX: usize = 64 * 1024;
const CHAT_HTTP_PORT_RANGES: &[core::ops::RangeInclusive<u16>] = &[
    CHAT_HTTP_TCP_PORT..=CHAT_HTTP_TCP_PORT,
    82..=128,
    8080..=8090,
];
static CHAT_HUB: spin::Mutex<Option<ChatHub>> = spin::Mutex::new(None);
static CHAT_HTTP_PORT: AtomicU16 = AtomicU16::new(0);

pub fn current_port() -> Option<u16> {
    match CHAT_HTTP_PORT.load(Ordering::Acquire) {
        0 => None,
        port => Some(port),
    }
}

pub struct HyperBytesBody {
    bytes: Option<Bytes>,
}

impl HyperBytesBody {
    fn new(bytes: Vec<u8>) -> Self {
        Self {
            bytes: Some(Bytes::from(bytes)),
        }
    }
}

impl Body for HyperBytesBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        Poll::Ready(self.bytes.take().map(|bytes| Ok(Frame::data(bytes))))
    }

    fn is_end_stream(&self) -> bool {
        self.bytes.is_none()
    }

    fn size_hint(&self) -> SizeHint {
        match self.bytes.as_ref() {
            Some(bytes) => SizeHint::with_exact(bytes.len() as u64),
            None => SizeHint::with_exact(0),
        }
    }
}

struct ChatSession {
    handle: api::NetHandle,
    inbound: WriteHalf<DuplexStream>,
}

fn status_code(status: u16) -> hyper::StatusCode {
    hyper::StatusCode::from_u16(status).unwrap_or(hyper::StatusCode::INTERNAL_SERVER_ERROR)
}

fn now_ms() -> u64 {
    crate::r::net::ntp::current_unix_seconds()
        .or_else(crate::time::unix_time_seconds)
        .unwrap_or_else(crate::time::uptime_seconds)
        .saturating_mul(1000)
}

fn chat_form_value(body: &[u8], key: &str, max_len: usize) -> Option<AllocString> {
    let body = core::str::from_utf8(body).ok()?;
    for pair in body.split('&') {
        let (raw_key, raw_value) = pair.split_once('=').unwrap_or((pair, ""));
        if chat_url_decode(raw_key, 64).as_deref() == Some(key) {
            return chat_url_decode(raw_value, max_len);
        }
    }
    None
}

fn chat_url_decode(raw: &str, max_len: usize) -> Option<AllocString> {
    let bytes = raw.as_bytes();
    let mut out = Vec::new();
    let mut idx = 0usize;
    while idx < bytes.len() {
        let byte = match bytes[idx] {
            b'+' => {
                idx += 1;
                b' '
            }
            b'%' if idx + 2 < bytes.len() => {
                let hi = chat_hex(bytes[idx + 1])?;
                let lo = chat_hex(bytes[idx + 2])?;
                idx += 3;
                (hi << 4) | lo
            }
            b'%' => return None,
            other => {
                idx += 1;
                other
            }
        };
        if out.len() >= max_len {
            break;
        }
        out.push(byte);
    }
    AllocString::from_utf8(out).ok()
}

fn chat_hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn chat_form_push_encoded(out: &mut AllocString, value: &str) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for byte in value.as_bytes().iter().copied() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(byte));
            }
            b' ' => out.push('+'),
            other => {
                out.push('%');
                out.push(char::from(HEX[(other >> 4) as usize]));
                out.push(char::from(HEX[(other & 0x0f) as usize]));
            }
        }
    }
}

fn chat_post_body(user: &str, text: &str) -> Vec<u8> {
    let mut body = AllocString::from("user=");
    chat_form_push_encoded(&mut body, user);
    body.push_str("&text=");
    chat_form_push_encoded(&mut body, text);
    body.into_bytes()
}

pub fn post_local_message(room: &str, user: &str, text: &str) -> bool {
    let body = chat_post_body(user, text);
    let response = {
        let mut guard = CHAT_HUB.lock();
        let hub = guard.get_or_insert_with(|| ChatHub::new(ChatConfig::default()));
        hub.handle(ChatRequest {
            method: ChatMethod::Post,
            path: alloc::format!("/api/rooms/{}/messages", room),
            query: None,
            body,
            now_ms: now_ms(),
        })
    };
    response.status == 200
}

fn maybe_submit_lumen_chat_post(method: ChatMethod, path: &str, body: &[u8], status: u16) {
    if method != ChatMethod::Post || status != 200 || !path.ends_with("/messages") {
        return;
    }
    let Some(user) = chat_form_value(body, "user", 64) else {
        return;
    };
    if user.trim().eq_ignore_ascii_case("lumen") {
        return;
    }
    let Some(text) = chat_form_value(body, "text", 8 * 1024) else {
        return;
    };
    if !text.to_ascii_lowercase().contains("lumen") {
        return;
    }

    let prompt = alloc::format!("{}: {:?}", user.trim(), text.trim());
    crate::r::lumen_service::submit_chatroom_mention(prompt.as_str());
    crate::log!("chat: queued lumen prompt via POST path={}\n", path);
}

async fn incoming_to_vec(mut body: Incoming, limit: usize) -> Result<Vec<u8>, ()> {
    let mut out = Vec::new();
    loop {
        let next = poll_fn(|cx| Pin::new(&mut body).poll_frame(cx)).await;
        let Some(frame) = next else {
            return Ok(out);
        };
        let frame = frame.map_err(|_| ())?;
        if let Ok(data) = frame.into_data() {
            if out.len().saturating_add(data.len()) > limit {
                return Err(());
            }
            out.extend_from_slice(&data);
        }
    }
}

async fn handle_hyper_request(
    request: hyper::Request<Incoming>,
) -> Result<hyper::Response<HyperBytesBody>, Infallible> {
    let method = match *request.method() {
        hyper::Method::GET => ChatMethod::Get,
        hyper::Method::POST => ChatMethod::Post,
        _ => ChatMethod::Other,
    };
    let path = request.uri().path().to_string();
    let query = request.uri().query().map(|query| query.to_string());
    let body = match incoming_to_vec(request.into_body(), CHAT_HTTP_BODY_MAX).await {
        Ok(body) => body,
        Err(()) => {
            let body = b"{\"ok\":false,\"error\":\"request too large\"}".to_vec();
            return Ok(hyper::Response::builder()
                .status(hyper::StatusCode::PAYLOAD_TOO_LARGE)
                .header(header::CONTENT_TYPE, "application/json; charset=utf-8")
                .header(header::CONTENT_LENGTH, body.len().to_string())
                .header(header::CACHE_CONTROL, "no-store")
                .body(HyperBytesBody::new(body))
                .unwrap_or_else(|_| hyper::Response::new(HyperBytesBody::new(Vec::new()))));
        }
    };

    let response = {
        let mut guard = CHAT_HUB.lock();
        let hub = guard.get_or_insert_with(|| ChatHub::new(ChatConfig::default()));
        hub.handle(ChatRequest {
            method,
            path: path.clone(),
            query,
            body: body.clone(),
            now_ms: now_ms(),
        })
    };
    maybe_submit_lumen_chat_post(method, path.as_str(), body.as_slice(), response.status);
    let no_cache = response.content_type.starts_with("application/json");
    let mut builder = hyper::Response::builder()
        .status(status_code(response.status))
        .header(header::CONTENT_TYPE, response.content_type)
        .header(header::CONTENT_LENGTH, response.body.len().to_string());
    if no_cache {
        builder = builder.header(header::CACHE_CONTROL, "no-store");
    }
    Ok(builder
        .body(HyperBytesBody::new(response.body))
        .unwrap_or_else(|_| hyper::Response::new(HyperBytesBody::new(Vec::new()))))
}

async fn send_tcp_all(vnet: &VNet, handle: api::NetHandle, data: &[u8]) -> Result<(), ()> {
    for chunk in data.chunks(api::MAX_MSG) {
        let mut sent = false;
        for _ in 0..64 {
            if vnet
                .submit(api::Command::SendTcp {
                    handle,
                    data: api::ByteBuf::from_slice_trunc(chunk),
                })
                .is_ok()
            {
                sent = true;
                break;
            }
            tokio::time::sleep(core::time::Duration::from_millis(1)).await;
        }
        if !sent {
            return Err(());
        }
    }
    Ok(())
}

async fn outbound_bridge(
    vnet: &'static VNet,
    handle: api::NetHandle,
    mut stream: ReadHalf<DuplexStream>,
) {
    let mut buf = [0u8; 2048];
    loop {
        match stream.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => {
                if send_tcp_all(vnet, handle, &buf[..n]).await.is_err() {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let _ = vnet.submit(api::Command::Close { handle });
}

fn spawn_hyper_session(vnet: &'static VNet, handle: api::NetHandle) -> ChatSession {
    let (hyper_io, bridge_io) = tokio::io::duplex(32 * 1024);
    let (bridge_read, bridge_write) = tokio::io::split(bridge_io);
    tokio::spawn(outbound_bridge(vnet, handle, bridge_read));
    tokio::spawn(async move {
        let service = hyper::service::service_fn(handle_hyper_request);
        if hyper::server::conn::http1::Builder::new()
            .serve_connection(HyperTokioIo::new(hyper_io), service)
            .await
            .is_err()
        {
            crate::log!("chat-http: hyper connection failed handle={}\n", handle.0);
        }
    });
    ChatSession {
        handle,
        inbound: bridge_write,
    }
}

async fn open_lowest_available_listener(vnet: &VNet) -> Option<(api::NetHandle, u16)> {
    for range in CHAT_HTTP_PORT_RANGES {
        for port in range.clone() {
            if vnet.submit(api::Command::OpenTcpListen { port }).is_err() {
                continue;
            }
            let deadline = tokio::time::Instant::now() + core::time::Duration::from_millis(250);
            loop {
                while let Some(ev) = vnet.pop_event() {
                    match ev {
                        api::Event::Opened {
                            handle,
                            kind: api::SocketKind::Tcp,
                        } => return Some((handle, port)),
                        api::Event::Error { msg } => {
                            crate::log!("chat-http: tcp {} unavailable: {}\n", port, msg);
                            break;
                        }
                        _ => {}
                    }
                }
                if tokio::time::Instant::now() >= deadline {
                    crate::log!("chat-http: tcp {} listen timeout\n", port);
                    break;
                }
                tokio::time::sleep(core::time::Duration::from_millis(10)).await;
            }
        }
    }
    None
}

async fn chat_http_runtime() {
    let vnet_ref: &'static VNet = loop {
        if let Some(vnet) = VNet::open_primary() {
            break Box::leak(Box::new(vnet));
        }
        tokio::time::sleep(core::time::Duration::from_millis(50)).await;
    };

    let mut listener: Option<(api::NetHandle, u16)> = None;
    let mut sessions: Vec<ChatSession> = Vec::new();

    loop {
        if listener.is_none() {
            listener = open_lowest_available_listener(vnet_ref).await;
            if let Some((_, port)) = listener {
                CHAT_HTTP_PORT.store(port, Ordering::Release);
                crate::log!("chat-http: listening on tcp {}\n", port);
            } else {
                CHAT_HTTP_PORT.store(0, Ordering::Release);
                crate::log!("chat-http: no free tcp port in service ranges\n");
                tokio::time::sleep(core::time::Duration::from_secs(5)).await;
                continue;
            }
        }

        while let Some(ev) = vnet_ref.pop_event() {
            match ev {
                api::Event::TcpEstablished { handle } => {
                    if sessions.iter().all(|session| session.handle != handle) {
                        sessions.push(spawn_hyper_session(vnet_ref, handle));
                    }
                }
                api::Event::TcpData { handle, data } => {
                    let idx = match sessions.iter().position(|session| session.handle == handle) {
                        Some(idx) => idx,
                        None => {
                            sessions.push(spawn_hyper_session(vnet_ref, handle));
                            sessions.len().saturating_sub(1)
                        }
                    };
                    if sessions[idx]
                        .inbound
                        .write_all(data.as_slice())
                        .await
                        .is_err()
                    {
                        let _ = vnet_ref.submit(api::Command::Close { handle });
                        sessions.swap_remove(idx);
                    }
                }
                api::Event::Closed { handle } => {
                    if listener.map(|(listener_handle, _)| listener_handle) == Some(handle) {
                        crate::log!("chat-http: listener closed, reopening\n");
                        CHAT_HTTP_PORT.store(0, Ordering::Release);
                        listener = None;
                    }
                    if let Some(idx) = sessions.iter().position(|session| session.handle == handle)
                    {
                        let mut session = sessions.swap_remove(idx);
                        let _ = session.inbound.shutdown().await;
                    }
                }
                api::Event::Error { msg } => {
                    crate::log!("chat-http: vnet error {}\n", msg);
                }
                api::Event::Opened { .. }
                | api::Event::TcpSent { .. }
                | api::Event::UdpPacket { .. }
                | api::Event::UdpPacketV6 { .. }
                | api::Event::IcmpReply { .. }
                | api::Event::IcmpReplyV6 { .. } => {}
            }
        }

        tokio::time::sleep(core::time::Duration::from_millis(1)).await;
    }
}

fn run_chat_http_runtime() -> Result<(), io::Error> {
    let mut builder = tokio::runtime::Builder::new_current_thread();
    builder.enable_io();
    builder.enable_time();
    let runtime = builder.build()?;
    runtime.block_on(chat_http_runtime());
    Ok(())
}

#[embassy_executor::task]
pub async fn chat_http_service_task() {
    crate::r::readiness::wait_for(crate::r::readiness::NET_V4_CONFIGURED).await;
    crate::log!("chat-http: starting after NET_V4_CONFIGURED\n");
    Timer::after(EmbassyDuration::from_millis(1)).await;
    if let Err(err) = run_chat_http_runtime() {
        crate::log!("chat-http: runtime failed {:?}\n", err);
    }
}
