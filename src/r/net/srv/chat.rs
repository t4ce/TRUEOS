extern crate alloc;
extern crate std;

use alloc::{boxed::Box, string::String as AllocString, string::ToString, vec::Vec};
use core::{
    convert::Infallible,
    pin::Pin,
    sync::atomic::{AtomicBool, AtomicU16, Ordering},
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
const CHAT_HTTP_IDLE_POLL_MS: u64 = 10;
const CHAT_HTTP_VNET_OPEN_RETRY_MS: u64 = 100;
const CHAT_HTTP_LISTEN_POLL_MS: u64 = 25;
const CHAT_HTTP_BLOCKING_LANE_RETRY_MS: u64 = 1000;
const CHAT_SAVE_BATCH_MS: u64 = 10_000;
const CHAT_SAVE_IDLE_MS: u64 = 1000;
const CHAT_STORE_DIR: &str = "chat";
const CHAT_STORE_PATH: &str = "chat/rooms.json";
const CHAT_HTTP_PORT_RANGES: &[core::ops::RangeInclusive<u16>] = &[
    CHAT_HTTP_TCP_PORT..=CHAT_HTTP_TCP_PORT,
    82..=128,
    8080..=8090,
];
static CHAT_HUB: spin::Mutex<Option<ChatHub>> = spin::Mutex::new(None);
static CHAT_HUB_LOADED: AtomicBool = AtomicBool::new(false);
static CHAT_SAVE_REQUESTED: AtomicBool = AtomicBool::new(false);
static CHAT_STORE_DIR_READY: AtomicBool = AtomicBool::new(false);
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

struct ChatEndpoint {
    vnet: &'static VNet,
    listener: Option<(api::NetHandle, u16)>,
    sessions: Vec<ChatSession>,
    dev_idx: usize,
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

fn load_chat_hub_once_sync() {
    if CHAT_HUB_LOADED.load(Ordering::Acquire) {
        return;
    }
    {
        let guard = CHAT_HUB.lock();
        if guard.is_some() {
            CHAT_HUB_LOADED.store(true, Ordering::Release);
            return;
        }
    }

    let bytes = match crate::r::io::kfs::read_file(CHAT_STORE_PATH) {
        Ok(bytes) => bytes,
        Err(crate::r::io::kfs::FsError::NoRoot) => return,
        Err(crate::r::io::kfs::FsError::NotFound) => {
            CHAT_HUB_LOADED.store(true, Ordering::Release);
            return;
        }
        Err(err) => {
            crate::log!("chat: load {} failed {:?}\n", CHAT_STORE_PATH, err);
            return;
        }
    };

    match ChatHub::from_json_bytes(ChatConfig::default(), bytes.as_slice()) {
        Ok(hub) => {
            let room_count = hub.room_count();
            let mut guard = CHAT_HUB.lock();
            if guard.is_none() {
                *guard = Some(hub);
                crate::log!("chat: loaded {} room(s) from {}\n", room_count, CHAT_STORE_PATH);
            }
            CHAT_HUB_LOADED.store(true, Ordering::Release);
        }
        Err(()) => {
            crate::log!("chat: ignored invalid {}\n", CHAT_STORE_PATH);
            CHAT_HUB_LOADED.store(true, Ordering::Release);
        }
    }
}

fn chat_hub_snapshot_bytes() -> Option<Vec<u8>> {
    let guard = CHAT_HUB.lock();
    guard.as_ref().map(ChatHub::to_json_bytes)
}

async fn ensure_chat_store_dir_async(disk: crate::disc::block::DeviceHandle) -> bool {
    if CHAT_STORE_DIR_READY.load(Ordering::Acquire) {
        return true;
    }
    let marker = alloc::format!("{}/.keep", CHAT_STORE_DIR);
    match crate::r::fs::trueosfs::file_in_async(disk, marker.as_str(), &[]).await {
        Ok(true) => {
            CHAT_STORE_DIR_READY.store(true, Ordering::Release);
            true
        }
        Ok(false) => false,
        Err(err) => {
            crate::log!("chat: save {} marker failed {:?}\n", CHAT_STORE_DIR, err);
            false
        }
    }
}

async fn save_chat_hub_snapshot_async() {
    let Some(bytes) = chat_hub_snapshot_bytes() else {
        return;
    };
    let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
        return;
    };
    if !ensure_chat_store_dir_async(disk).await {
        return;
    }
    let handle = match crate::r::fs::trueosfs::file_write_begin_async(
        disk,
        CHAT_STORE_PATH,
        bytes.len() as u64,
    )
    .await
    {
        Ok(Some(handle)) => handle,
        Ok(None) => {
            crate::log!("chat: save {} begin failed NoSpace\n", CHAT_STORE_PATH);
            return;
        }
        Err(err) => {
            crate::log!("chat: save {} begin failed {:?}\n", CHAT_STORE_PATH, err);
            return;
        }
    };
    if let Err(err) = crate::r::fs::trueosfs::file_write_chunk_async(handle, bytes.as_slice()).await
    {
        let _ = crate::r::fs::trueosfs::file_write_abort_async(handle).await;
        crate::log!("chat: save {} chunk failed {:?}\n", CHAT_STORE_PATH, err);
        return;
    }
    if let Err(err) = crate::r::fs::trueosfs::file_write_finish_async(handle).await {
        crate::log!("chat: save {} finish failed {:?}\n", CHAT_STORE_PATH, err);
    }
}

fn request_chat_hub_save(reason: &'static str) {
    let was_pending = CHAT_SAVE_REQUESTED.swap(true, Ordering::AcqRel);
    if !was_pending {
        crate::log!("chat: save requested reason={} mode=deferred\n", reason);
    }
}

async fn chat_hub_save_loop() -> ! {
    loop {
        if !CHAT_SAVE_REQUESTED.swap(false, Ordering::AcqRel) {
            Timer::after(EmbassyDuration::from_millis(CHAT_SAVE_IDLE_MS)).await;
            continue;
        }

        Timer::after(EmbassyDuration::from_millis(CHAT_SAVE_BATCH_MS)).await;
        let coalesced = CHAT_SAVE_REQUESTED.swap(false, Ordering::AcqRel);
        crate::log!("chat: save begin mode=batched\n");
        save_chat_hub_snapshot_async().await;
        crate::log!("chat: save done mode=batched coalesced_requests={}\n", coalesced);
    }
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

fn chat_json_value(body: &[u8], key: &str, max_len: usize) -> Option<AllocString> {
    let trimmed = chat_trim_ascii_ws(body);
    if trimmed.first() != Some(&b'{') {
        return None;
    }
    let value = serde_json::from_slice::<serde_json::Value>(trimmed).ok()?;
    let raw = value.get(key)?.as_str()?.trim();
    let mut out = AllocString::new();
    for ch in raw.chars() {
        if out.len().saturating_add(ch.len_utf8()) > max_len {
            break;
        }
        out.push(ch);
    }
    Some(out)
}

fn chat_message_value(body: &[u8], key: &str, max_len: usize) -> Option<AllocString> {
    chat_json_value(body, key, max_len).or_else(|| chat_form_value(body, key, max_len))
}

fn chat_trim_ascii_ws(mut bytes: &[u8]) -> &[u8] {
    while matches!(bytes.first(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
        bytes = &bytes[1..];
    }
    while matches!(bytes.last(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
        bytes = &bytes[..bytes.len().saturating_sub(1)];
    }
    bytes
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

fn chat_post_body(user: &str, text: &str, statement: Option<&str>) -> Vec<u8> {
    let mut body = AllocString::from("user=");
    chat_form_push_encoded(&mut body, user);
    body.push_str("&text=");
    chat_form_push_encoded(&mut body, text);
    if let Some(statement) = statement {
        body.push_str("&statement=");
        chat_form_push_encoded(&mut body, statement);
    }
    body.into_bytes()
}

pub fn post_local_message(room: &str, user: &str, text: &str) -> bool {
    post_local_message_with_persistence(room, user, text, None, true)
}

pub fn post_local_message_volatile(room: &str, user: &str, text: &str) -> bool {
    post_local_message_with_persistence(room, user, text, None, false)
}

pub fn post_local_statement_volatile(room: &str, user: &str, statement: &str, text: &str) -> bool {
    post_local_message_with_persistence(room, user, text, Some(statement), false)
}

fn post_local_message_with_persistence(
    room: &str,
    user: &str,
    text: &str,
    statement: Option<&str>,
    persist: bool,
) -> bool {
    load_chat_hub_once_sync();
    let body = chat_post_body(user, text, statement);
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
    let ok = response.status == 200;
    if ok && persist {
        request_chat_hub_save("local-post");
    }
    ok
}

fn maybe_submit_lumen_chat_post(method: ChatMethod, path: &str, body: &[u8], status: u16) {
    if method != ChatMethod::Post || status != 200 || !path.ends_with("/messages") {
        return;
    }
    let Some(room) = path
        .strip_prefix("/api/rooms/")
        .and_then(|rest| rest.strip_suffix("/messages"))
    else {
        return;
    };
    if !room.eq_ignore_ascii_case("lobby") {
        return;
    }
    let Some(user) = chat_message_value(body, "user", 64) else {
        return;
    };
    if user.trim().eq_ignore_ascii_case("lumen") {
        return;
    }
    let Some(text) = chat_message_value(body, "text", 8 * 1024) else {
        return;
    };
    if !text.to_ascii_lowercase().contains("lumen") {
        return;
    }

    let prompt = alloc::format!("{}: {}", user.trim(), text.trim());
    if crate::lumen::lumen_service::submit_chatroom_mention(prompt.as_str()) {
        crate::log!("chat: accepted lumen prompt via POST path={}\n", path);
    } else {
        crate::log!("chat: lumen prompt not accepted via POST path={}\n", path);
    }
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
    load_chat_hub_once_sync();
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
    if method == ChatMethod::Post && response.status == 200 {
        request_chat_hub_save("http-post");
    }
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
                tokio::time::sleep(core::time::Duration::from_millis(CHAT_HTTP_LISTEN_POLL_MS))
                    .await;
            }
        }
    }
    None
}

fn chat_dev_usable(dev_idx: usize) -> bool {
    crate::net::adapter::ipv4_at(dev_idx).is_some()
        || crate::net::link_state_at(dev_idx)
            .map(|state| state.up)
            .unwrap_or(false)
}

fn log_chat_endpoint(dev_idx: usize, vnet: &VNet, port: u16) {
    let name = crate::net::device_name_at(dev_idx).unwrap_or("?");
    match crate::net::adapter::ipv4_at(dev_idx) {
        Some([a, b, c, d]) => crate::log!(
            "chat-http: listening on tcp {} owner={} dev={} {} ip={}.{}.{}.{}\n",
            port,
            vnet.owner(),
            dev_idx,
            name,
            a,
            b,
            c,
            d
        ),
        None => crate::log!(
            "chat-http: listening on tcp {} owner={} dev={} {} ip=none\n",
            port,
            vnet.owner(),
            dev_idx,
            name
        ),
    }
}

async fn chat_add_endpoints(endpoints: &mut Vec<ChatEndpoint>) -> usize {
    let mut added = 0;
    for dev_idx in 0..crate::net::device_count() {
        if endpoints.iter().any(|endpoint| endpoint.dev_idx == dev_idx) {
            continue;
        }
        if !chat_dev_usable(dev_idx) {
            continue;
        }
        let Some(vnet) = VNet::open(dev_idx) else {
            continue;
        };
        let listener = open_lowest_available_listener(&vnet).await;
        let Some((_, port)) = listener else {
            crate::log!("chat-http: no free tcp port dev={}\n", dev_idx);
            continue;
        };
        let vnet_ref: &'static VNet = Box::leak(Box::new(vnet));
        if CHAT_HTTP_PORT.load(Ordering::Acquire) == 0 {
            CHAT_HTTP_PORT.store(port, Ordering::Release);
        }
        log_chat_endpoint(dev_idx, vnet_ref, port);
        endpoints.push(ChatEndpoint {
            vnet: vnet_ref,
            listener,
            sessions: Vec::new(),
            dev_idx,
        });
        added += 1;
    }
    added
}

async fn chat_http_runtime() {
    load_chat_hub_once_sync();

    let mut endpoints: Vec<ChatEndpoint> = Vec::new();
    loop {
        chat_add_endpoints(&mut endpoints).await;
        if !endpoints.is_empty() {
            break;
        }
        CHAT_HTTP_PORT.store(0, Ordering::Release);
        crate::log!("chat-http: waiting for a usable NIC\n");
        tokio::time::sleep(core::time::Duration::from_millis(CHAT_HTTP_VNET_OPEN_RETRY_MS)).await;
    }

    let mut endpoint_discovery_ticks = 0u32;
    loop {
        if endpoint_discovery_ticks == 0 {
            chat_add_endpoints(&mut endpoints).await;
        }
        endpoint_discovery_ticks = (endpoint_discovery_ticks + 1) % 100;

        for endpoint in endpoints.iter_mut() {
            while let Some(ev) = endpoint.vnet.pop_event() {
                match ev {
                    api::Event::TcpEstablished { handle } => {
                        crate::log!(
                            "chat-http: tcp established dev={} handle={}\n",
                            endpoint.dev_idx,
                            handle.0
                        );
                        if endpoint
                            .sessions
                            .iter()
                            .all(|session| session.handle != handle)
                        {
                            endpoint
                                .sessions
                                .push(spawn_hyper_session(endpoint.vnet, handle));
                        }
                    }
                    api::Event::TcpData { handle, data } => {
                        let idx = match endpoint
                            .sessions
                            .iter()
                            .position(|session| session.handle == handle)
                        {
                            Some(idx) => idx,
                            None => {
                                endpoint
                                    .sessions
                                    .push(spawn_hyper_session(endpoint.vnet, handle));
                                endpoint.sessions.len().saturating_sub(1)
                            }
                        };
                        if endpoint.sessions[idx]
                            .inbound
                            .write_all(data.as_slice())
                            .await
                            .is_err()
                        {
                            let _ = endpoint.vnet.submit(api::Command::Close { handle });
                            endpoint.sessions.swap_remove(idx);
                        }
                    }
                    api::Event::Closed { handle } => {
                        if endpoint.listener.map(|(listener_handle, _)| listener_handle)
                            == Some(handle)
                        {
                            crate::log!(
                                "chat-http: listener closed dev={}, reopening\n",
                                endpoint.dev_idx
                            );
                            endpoint.listener = open_lowest_available_listener(endpoint.vnet).await;
                            if let Some((_, port)) = endpoint.listener {
                                log_chat_endpoint(endpoint.dev_idx, endpoint.vnet, port);
                            }
                        }
                        if let Some(idx) = endpoint
                            .sessions
                            .iter()
                            .position(|session| session.handle == handle)
                        {
                            let mut session = endpoint.sessions.swap_remove(idx);
                            let _ = session.inbound.shutdown().await;
                        }
                    }
                    api::Event::Error { msg } => {
                        crate::log!("chat-http: vnet error dev={} {}\n", endpoint.dev_idx, msg);
                    }
                    api::Event::Opened { .. }
                    | api::Event::TcpSent { .. }
                    | api::Event::UdpPacket { .. }
                    | api::Event::UdpPacketV6 { .. }
                    | api::Event::IcmpReply { .. }
                    | api::Event::IcmpReplyV6 { .. } => {}
                }
            }
        }

        tokio::time::sleep(core::time::Duration::from_millis(CHAT_HTTP_IDLE_POLL_MS)).await;
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
    crate::r::readiness::wait_for(
        crate::r::readiness::NET_V4_CONFIGURED | crate::r::readiness::TRUEOSFS_ROOT_MOUNTED,
    )
    .await;
    crate::log!(
        "chat-http: launching Tokio runtime after NET_V4_CONFIGURED+TRUEOSFS_ROOT_MOUNTED\n"
    );

    loop {
        let rc = crate::trueos_tokio_worker::spawn_blocking_job_with_purpose(
            Box::new(|| {
                if let Err(err) = run_chat_http_runtime() {
                    crate::log!("chat-http: runtime failed {:?}\n", err);
                }
            }),
            "chat-http-runtime",
        );
        if rc == 0 {
            crate::log!("chat-http: submitted Tokio runtime to blocking lane\n");
            chat_hub_save_loop().await;
        }
        crate::log!(
            "chat-http: blocking lane unavailable rc={} retry={}ms\n",
            rc,
            CHAT_HTTP_BLOCKING_LANE_RETRY_MS
        );
        Timer::after(EmbassyDuration::from_millis(CHAT_HTTP_BLOCKING_LANE_RETRY_MS)).await;
    }
}
