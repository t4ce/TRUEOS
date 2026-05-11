extern crate alloc;
extern crate std;

use alloc::{boxed::Box, string::String as AllocString, string::ToString, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::{io, net::SocketAddr};

use axum::{
    Router,
    body::{Body, to_bytes},
    extract::Request,
    http::{
        Method, StatusCode,
        header::{CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE},
    },
    response::Response,
    routing::{any, get},
    serve::ListenerExt,
};
use embassy_time::{Duration as EmbassyDuration, Timer};
use trueos_chat::{ChatConfig, ChatHub, ChatMethod, ChatRequest, ChatResponse};

use crate::allports::services::CHAT_HTTP_TCP_PORT;

const CHAT_HTTP_BODY_MAX: usize = 64 * 1024;
const CHAT_HTTP_BIND_RETRY_MS: u64 = 100;
const CHAT_HTTP_BLOCKING_LANE_RETRY_MS: u64 = 1000;
const CHAT_SAVE_BATCH_MS: u64 = 10_000;
const CHAT_SAVE_IDLE_MS: u64 = 1000;
const CHAT_STORE_DIR: &str = "chat";
const CHAT_STORE_PATH: &str = "chat/rooms.json";
const TRUEOS_TAILWIND_CSS: &str = include_str!("../common/tailwind.css");
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

fn status_code(status: u16) -> StatusCode {
    StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
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

fn chat_response(response: ChatResponse) -> Response {
    let no_cache = response.content_type.starts_with("application/json");
    let mut builder = Response::builder()
        .status(status_code(response.status))
        .header(CONTENT_TYPE, response.content_type)
        .header(CONTENT_LENGTH, response.body.len().to_string());
    if no_cache {
        builder = builder.header(CACHE_CONTROL, "no-store");
    }
    builder
        .body(Body::from(response.body))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn request_too_large_response() -> Response {
    let body = b"{\"ok\":false,\"error\":\"request too large\"}".to_vec();
    Response::builder()
        .status(StatusCode::PAYLOAD_TOO_LARGE)
        .header(CONTENT_TYPE, "application/json; charset=utf-8")
        .header(CONTENT_LENGTH, body.len().to_string())
        .header(CACHE_CONTROL, "no-store")
        .body(Body::from(body))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

async fn handle_tailwind_css() -> Response {
    let body = TRUEOS_TAILWIND_CSS.as_bytes().to_vec();
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "text/css; charset=utf-8")
        .header(CONTENT_LENGTH, body.len().to_string())
        .header(CACHE_CONTROL, "no-cache")
        .body(Body::from(body))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

async fn handle_chat_request(request: Request) -> Response {
    load_chat_hub_once_sync();
    let (parts, body) = request.into_parts();
    let method = match parts.method {
        Method::GET => ChatMethod::Get,
        Method::POST => ChatMethod::Post,
        _ => ChatMethod::Other,
    };
    let path = parts.uri.path().to_string();
    let query = parts.uri.query().map(|query| query.to_string());
    let body = match to_bytes(body, CHAT_HTTP_BODY_MAX).await {
        Ok(body) => body.to_vec(),
        Err(_) => return request_too_large_response(),
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
    chat_response(response)
}

fn chat_router() -> Router {
    Router::new()
        .route("/", any(handle_chat_request))
        .route("/tailwind.css", get(handle_tailwind_css))
        .route("/api", any(handle_chat_request))
        .route("/api/rooms", any(handle_chat_request))
        .route("/api/rooms/{room}/messages", any(handle_chat_request))
        .fallback(handle_chat_request)
}

fn primary_ipv4_addr(port: u16) -> Option<SocketAddr> {
    let dev_idx = crate::net::primary_device_index();
    let ip = crate::net::adapter::ipv4_at(dev_idx)?;
    Some(SocketAddr::from((ip, port)))
}

fn log_chat_endpoint(addr: SocketAddr) {
    let dev_idx = crate::net::primary_device_index();
    let name = crate::net::device_name_at(dev_idx).unwrap_or("?");
    match crate::net::adapter::ipv4_at(dev_idx) {
        Some([a, b, c, d]) => crate::log!(
            "chat-http: axum listening on http://{}/ dev={} {} ip={}.{}.{}.{}\n",
            addr,
            dev_idx,
            name,
            a,
            b,
            c,
            d
        ),
        None => crate::log!(
            "chat-http: axum listening on http://{}/ dev={} {} ip=none\n",
            addr,
            dev_idx,
            name
        ),
    }
}

async fn chat_http_runtime() -> Result<(), io::Error> {
    crate::log!("chat-http: runtime async enter\n");
    tokio::task::spawn_local(crate::t::shared_tokio_job_pump());
    crate::log!("chat-http: loading hub\n");
    load_chat_hub_once_sync();
    crate::log!("chat-http: hub ready\n");

    let app = chat_router();
    loop {
        crate::log!("chat-http: bind begin port={}\n", CHAT_HTTP_TCP_PORT);
        let Some(addr) = primary_ipv4_addr(CHAT_HTTP_TCP_PORT) else {
            CHAT_HTTP_PORT.store(0, Ordering::Release);
            crate::log!("chat-http: waiting for primary ipv4\n");
            tokio::time::sleep(core::time::Duration::from_millis(CHAT_HTTP_BIND_RETRY_MS)).await;
            continue;
        };
        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => listener,
            Err(err) => {
                CHAT_HTTP_PORT.store(0, Ordering::Release);
                crate::log!(
                    "chat-http: bind {} failed port={} kind={:?} err={}\n",
                    addr,
                    CHAT_HTTP_TCP_PORT,
                    err.kind(),
                    err
                );
                tokio::time::sleep(core::time::Duration::from_millis(CHAT_HTTP_BIND_RETRY_MS))
                    .await;
                continue;
            }
        };

        CHAT_HTTP_PORT.store(addr.port(), Ordering::Release);
        log_chat_endpoint(addr);
        let listener = listener.tap_io(|_| crate::log!("chat-http: tcp accepted\n"));
        if let Err(err) = axum::serve(listener, app.clone()).await {
            CHAT_HTTP_PORT.store(0, Ordering::Release);
            crate::log!(
                "chat-http: serve failed port={} kind={:?} err={}\n",
                addr.port(),
                err.kind(),
                err
            );
            tokio::time::sleep(core::time::Duration::from_millis(1000)).await;
        }
    }
}

fn run_chat_http_runtime() -> Result<(), io::Error> {
    crate::log!("chat-http: runtime build begin\n");
    let mut builder = tokio::runtime::Builder::new_current_thread();
    builder.enable_io();
    builder.enable_time();
    let runtime = builder.build()?;
    crate::log!("chat-http: runtime build ok\n");
    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime, chat_http_runtime())
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
        let rc = crate::t::trueos_tokio_worker::spawn_blocking_job_with_purpose(
            Box::new(|| {
                crate::log!("chat-http: blocking closure enter\n");
                if let Err(err) = run_chat_http_runtime() {
                    crate::log!("chat-http: runtime failed {:?}\n", err);
                }
                crate::log!("chat-http: blocking closure exit\n");
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
