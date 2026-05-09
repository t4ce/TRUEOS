extern crate alloc;
extern crate std;

use alloc::{boxed::Box, format, string::String, string::ToString, vec::Vec};
use core::{
    convert::Infallible,
    pin::Pin,
    sync::atomic::{AtomicU16, AtomicU32, Ordering},
    task::{Context, Poll},
};
use std::io;

use embassy_time::{Duration as EmbassyDuration, Timer};
use hyper::{
    body::{Body, Bytes, Frame, Incoming, SizeHint},
    header,
};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream, ReadHalf, WriteHalf};
use v::vnet as api;

use crate::{
    allports::services::MAIL_HTTP_TCP_PORT,
    r::net::{smtp::SmtpClient, VNet},
    t::net::hyper_io::HyperTokioIo,
};

const MAIL_HTTP_BODY_MAX: usize = 64 * 1024;
const MAIL_HTTP_IDLE_POLL_MS: u64 = 10;
const MAIL_HTTP_VNET_OPEN_RETRY_MS: u64 = 100;
const MAIL_HTTP_LISTEN_POLL_MS: u64 = 25;
const MAIL_HTTP_BLOCKING_LANE_RETRY_MS: u64 = 1000;
const MAIL_STORE_PATH: &str = "mail/box.json";
const MAIL_CONFIG_PATH: &str = "mail/config.json";
const MAIL_SMTP_TIMEOUT_MS: u32 = 20_000;

static MAIL_HTTP_PORT: AtomicU16 = AtomicU16::new(0);
static MAIL_ID_SEQ: AtomicU32 = AtomicU32::new(1);

const MAIL_INDEX_HTML: &str = include_str!("mail_web/index.html");
const MAIL_APP_JS: &str = include_str!("mail_web/app.js");

pub fn current_port() -> Option<u16> {
    match MAIL_HTTP_PORT.load(Ordering::Acquire) {
        0 => None,
        port => Some(port),
    }
}

struct HyperBytesBody {
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

#[derive(Clone, Debug, Deserialize, Serialize)]
struct MailMessage {
    id: String,
    from: String,
    to: String,
    subject: String,
    date: String,
    body: String,
    unread: bool,
    status: String,
    error: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct MailStore {
    messages: Vec<MailMessage>,
}

#[derive(Serialize)]
struct MailListResponse {
    messages: Vec<MailSummary>,
}

#[derive(Serialize)]
struct MailSummary {
    id: String,
    from: String,
    subject: String,
    preview: String,
    date: String,
    unread: bool,
}

#[derive(Deserialize)]
struct MailSendRequest {
    to: String,
    subject: String,
    body: String,
}

#[derive(Deserialize)]
struct MailConfig {
    smtp_user: String,
    smtp_pass: String,
    from: Option<String>,
}

struct MailSession {
    handle: api::NetHandle,
    inbound: WriteHalf<DuplexStream>,
}

struct MailEndpoint {
    vnet: &'static VNet,
    listener: Option<(api::NetHandle, u16)>,
    sessions: Vec<MailSession>,
    dev_idx: usize,
}

fn status_code(status: u16) -> hyper::StatusCode {
    hyper::StatusCode::from_u16(status).unwrap_or(hyper::StatusCode::INTERNAL_SERVER_ERROR)
}

fn json_response<T: Serialize>(status: u16, value: &T) -> hyper::Response<HyperBytesBody> {
    let body = serde_json::to_vec(value).unwrap_or_else(|_| b"{\"ok\":false}".to_vec());
    response(status, "application/json; charset=utf-8", body, true)
}

fn text_response(status: u16, content_type: &'static str, body: &'static str) -> hyper::Response<HyperBytesBody> {
    response(status, content_type, body.as_bytes().to_vec(), false)
}

fn response(
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
    no_store: bool,
) -> hyper::Response<HyperBytesBody> {
    let mut builder = hyper::Response::builder()
        .status(status_code(status))
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, body.len().to_string());
    if no_store {
        builder = builder.header(header::CACHE_CONTROL, "no-store");
    }
    builder
        .body(HyperBytesBody::new(body))
        .unwrap_or_else(|_| hyper::Response::new(HyperBytesBody::new(Vec::new())))
}

async fn incoming_to_vec(mut body: Incoming, limit: usize) -> Result<Vec<u8>, ()> {
    let mut out = Vec::new();
    loop {
        let next = core::future::poll_fn(|cx| Pin::new(&mut body).poll_frame(cx)).await;
        let Some(frame) = next else {
            break;
        };
        let frame = frame.map_err(|_| ())?;
        if let Ok(data) = frame.into_data() {
            if out.len().saturating_add(data.len()) > limit {
                return Err(());
            }
            out.extend_from_slice(&data);
        }
    }
    Ok(out)
}

fn primary_root() -> Result<crate::disc::block::DeviceHandle, &'static str> {
    crate::r::fs::trueosfs::primary_root_handle().ok_or("mail root unavailable")
}

async fn load_store() -> MailStore {
    let Ok(disk) = primary_root() else {
        return MailStore::default();
    };
    match crate::r::fs::trueosfs::file_out_async(disk, MAIL_STORE_PATH).await {
        Ok(Some(bytes)) => serde_json::from_slice::<MailStore>(bytes.as_slice()).unwrap_or_default(),
        _ => MailStore::default(),
    }
}

async fn save_store(store: &MailStore) -> Result<(), &'static str> {
    let disk = primary_root()?;
    let bytes = serde_json::to_vec(store).map_err(|_| "serialize failed")?;
    match crate::r::fs::trueosfs::file_in_async(disk, MAIL_STORE_PATH, bytes.as_slice()).await {
        Ok(true) => Ok(()),
        Ok(false) => Err("mail store write refused"),
        Err(_) => Err("mail store write failed"),
    }
}

async fn load_config() -> Result<MailConfig, &'static str> {
    let disk = primary_root()?;
    let bytes = crate::r::fs::trueosfs::file_out_async(disk, MAIL_CONFIG_PATH)
        .await
        .map_err(|_| "config read failed")?
        .ok_or("missing mail/config.json")?;
    serde_json::from_slice::<MailConfig>(bytes.as_slice()).map_err(|_| "bad mail/config.json")
}

fn now_date_string() -> String {
    let secs = crate::r::net::ntp::current_unix_seconds()
        .or_else(crate::time::unix_time_seconds)
        .unwrap_or_else(crate::time::uptime_seconds);
    format!("{}", secs)
}

fn next_mail_id() -> String {
    let seq = MAIL_ID_SEQ.fetch_add(1, Ordering::Relaxed).max(1);
    let secs = crate::r::net::ntp::current_unix_seconds()
        .or_else(crate::time::unix_time_seconds)
        .unwrap_or_else(crate::time::uptime_seconds);
    format!("mail-{}-{}", secs, seq)
}

fn preview(body: &str) -> String {
    let mut out = String::new();
    for ch in body.chars().take(96) {
        if ch == '\r' || ch == '\n' || ch == '\t' {
            out.push(' ');
        } else {
            out.push(ch);
        }
    }
    out
}

fn header_value(raw: &str) -> String {
    raw.chars()
        .filter(|ch| *ch != '\r' && *ch != '\n')
        .take(240)
        .collect()
}

fn valid_addr(raw: &str) -> bool {
    let text = raw.trim();
    !text.is_empty()
        && text.len() <= 254
        && text.contains('@')
        && !text.chars().any(|ch| ch <= '\u{1f}' || ch == '<' || ch == '>')
}

fn recipients(to: &str) -> Vec<String> {
    to.split(',')
        .map(|part| part.trim())
        .filter(|part| valid_addr(part))
        .map(String::from)
        .collect()
}

fn build_message(from: &str, to: &str, subject: &str, body: &str, id: &str) -> String {
    let from_domain = from.split('@').nth(1).unwrap_or("trueos.local");
    format!(
        "From: <{}>\r\nTo: <{}>\r\nSubject: {}\r\nDate: {}\r\nMessage-ID: <{}@{}>\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=UTF-8\r\nContent-Transfer-Encoding: 8bit\r\n\r\n{}",
        header_value(from),
        header_value(to),
        header_value(subject),
        now_date_string(),
        header_value(id),
        header_value(from_domain),
        body
    )
}

async fn update_message_status(id: &str, status: &str, error: Option<String>) {
    let mut store = load_store().await;
    if let Some(message) = store.messages.iter_mut().find(|message| message.id == id) {
        message.status = String::from(status);
        message.error = error;
        let _ = save_store(&store).await;
    }
}

async fn send_mail_job(id: String) {
    let store = load_store().await;
    let Some(message) = store.messages.iter().find(|message| message.id == id).cloned() else {
        return;
    };
    let config = match load_config().await {
        Ok(config) => config,
        Err(err) => {
            update_message_status(id.as_str(), "config-missing", Some(String::from(err))).await;
            return;
        }
    };
    let from = config.from.as_deref().unwrap_or(config.smtp_user.as_str());
    let rcpts = recipients(message.to.as_str());
    if rcpts.is_empty() {
        update_message_status(id.as_str(), "invalid-recipient", None).await;
        return;
    }
    let rcpt_refs: Vec<&str> = rcpts.iter().map(|s| s.as_str()).collect();
    let wire = build_message(
        from,
        message.to.as_str(),
        message.subject.as_str(),
        message.body.as_str(),
        message.id.as_str(),
    );

    update_message_status(id.as_str(), "sending", None).await;
    let result = async {
        let mut client = SmtpClient::connect(MAIL_SMTP_TIMEOUT_MS).await?;
        client
            .auth_login(
                config.smtp_user.as_str(),
                config.smtp_pass.as_str(),
                MAIL_SMTP_TIMEOUT_MS,
            )
            .await?;
        client
            .send_mail(from, rcpt_refs.as_slice(), wire.as_str(), MAIL_SMTP_TIMEOUT_MS)
            .await?;
        let _ = client.quit(5_000).await;
        Ok::<(), crate::r::net::smtp::SmtpError>(())
    }
    .await;

    match result {
        Ok(()) => update_message_status(id.as_str(), "sent", None).await,
        Err(err) => update_message_status(id.as_str(), "send-failed", Some(format!("{:?}", err))).await,
    }
}

async fn handle_list() -> hyper::Response<HyperBytesBody> {
    let mut messages: Vec<MailSummary> = load_store()
        .await
        .messages
        .into_iter()
        .rev()
        .map(|message| MailSummary {
            id: message.id,
            from: message.from,
            subject: message.subject,
            preview: preview(message.body.as_str()),
            date: message.date,
            unread: message.unread,
        })
        .collect();
    messages.truncate(100);
    json_response(200, &MailListResponse { messages })
}

fn query_param<'a>(query: Option<&'a str>, name: &str) -> Option<&'a str> {
    for part in query.unwrap_or("").split('&') {
        let (key, value) = part.split_once('=').unwrap_or((part, ""));
        if key == name {
            return Some(value);
        }
    }
    None
}

async fn handle_read(query: Option<&str>) -> hyper::Response<HyperBytesBody> {
    let Some(id) = query_param(query, "id") else {
        return json_response(400, &serde_json::json!({"ok": false, "error": "missing id"}));
    };
    let store = load_store().await;
    match store.messages.into_iter().find(|message| message.id == id) {
        Some(message) => json_response(200, &message),
        None => json_response(404, &serde_json::json!({"ok": false, "error": "not found"})),
    }
}

async fn handle_send(body: Vec<u8>) -> hyper::Response<HyperBytesBody> {
    let req = match serde_json::from_slice::<MailSendRequest>(body.as_slice()) {
        Ok(req) => req,
        Err(_) => return json_response(400, &serde_json::json!({"ok": false, "error": "bad json"})),
    };
    let rcpts = recipients(req.to.as_str());
    if rcpts.is_empty() || req.body.trim().is_empty() {
        return json_response(
            400,
            &serde_json::json!({"ok": false, "error": "recipient and body required"}),
        );
    }

    let id = next_mail_id();
    let from = match load_config().await {
        Ok(config) => config.from.unwrap_or(config.smtp_user),
        Err(_) => String::from("trueos@local"),
    };
    let message = MailMessage {
        id: id.clone(),
        from,
        to: req.to.trim().to_string(),
        subject: header_value(req.subject.trim()),
        date: now_date_string(),
        body: req.body,
        unread: false,
        status: String::from("queued"),
        error: None,
    };

    let mut store = load_store().await;
    store.messages.push(message);
    if let Err(err) = save_store(&store).await {
        return json_response(500, &serde_json::json!({"ok": false, "error": err}));
    }

    let job_id = id.clone();
    tokio::task::spawn_local(async move {
        send_mail_job(job_id).await;
    });
    json_response(200, &serde_json::json!({"ok": true, "id": id}))
}

async fn handle_hyper_request(
    request: hyper::Request<Incoming>,
) -> Result<hyper::Response<HyperBytesBody>, Infallible> {
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let query = request.uri().query().map(|query| query.to_string());
    let body = match incoming_to_vec(request.into_body(), MAIL_HTTP_BODY_MAX).await {
        Ok(body) => body,
        Err(()) => {
            return Ok(json_response(
                413,
                &serde_json::json!({"ok": false, "error": "request too large"}),
            ));
        }
    };

    let response = match (method, path.as_str()) {
        (hyper::Method::GET, "/") | (hyper::Method::GET, "/index.html") => {
            text_response(200, "text/html; charset=utf-8", MAIL_INDEX_HTML)
        }
        (hyper::Method::GET, "/app.js") => {
            text_response(200, "application/javascript; charset=utf-8", MAIL_APP_JS)
        }
        (hyper::Method::GET, "/api/mail/list") => handle_list().await,
        (hyper::Method::GET, "/api/mail/read") => handle_read(query.as_deref()).await,
        (hyper::Method::POST, "/api/mail/send") => handle_send(body).await,
        _ => json_response(404, &serde_json::json!({"ok": false, "error": "not found"})),
    };
    Ok(response)
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

fn spawn_hyper_session(vnet: &'static VNet, handle: api::NetHandle) -> MailSession {
    let (hyper_io, bridge_io) = tokio::io::duplex(32 * 1024);
    let (bridge_read, bridge_write) = tokio::io::split(bridge_io);
    tokio::task::spawn_local(outbound_bridge(vnet, handle, bridge_read));
    tokio::task::spawn_local(async move {
        let service = hyper::service::service_fn(handle_hyper_request);
        if hyper::server::conn::http1::Builder::new()
            .serve_connection(HyperTokioIo::new(hyper_io), service)
            .await
            .is_err()
        {
            crate::log!("mail-http: hyper connection failed handle={}\n", handle.0);
        }
    });
    MailSession {
        handle,
        inbound: bridge_write,
    }
}

async fn open_mail_listener(vnet: &VNet) -> Option<(api::NetHandle, u16)> {
    let port = MAIL_HTTP_TCP_PORT;
    if vnet.submit(api::Command::OpenTcpListen { port }).is_err() {
        return None;
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
                    crate::log!("mail-http: tcp {} unavailable: {}\n", port, msg);
                    return None;
                }
                _ => {}
            }
        }
        if tokio::time::Instant::now() >= deadline {
            crate::log!("mail-http: tcp {} listen timeout\n", port);
            return None;
        }
        tokio::time::sleep(core::time::Duration::from_millis(MAIL_HTTP_LISTEN_POLL_MS)).await;
    }
}

fn mail_dev_usable(dev_idx: usize) -> bool {
    crate::net::adapter::ipv4_at(dev_idx).is_some()
        || crate::net::link_state_at(dev_idx)
            .map(|state| state.up)
            .unwrap_or(false)
}

fn log_mail_endpoint(dev_idx: usize, vnet: &VNet, port: u16) {
    let name = crate::net::device_name_at(dev_idx).unwrap_or("?");
    match crate::net::adapter::ipv4_at(dev_idx) {
        Some([a, b, c, d]) => crate::log!(
            "mail-http: listening on tcp {} owner={} dev={} {} ip={}.{}.{}.{}\n",
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
            "mail-http: listening on tcp {} owner={} dev={} {} ip=none\n",
            port,
            vnet.owner(),
            dev_idx,
            name
        ),
    }
}

async fn mail_add_endpoints(endpoints: &mut Vec<MailEndpoint>) -> usize {
    let mut added = 0;
    for dev_idx in 0..crate::net::device_count() {
        if endpoints.iter().any(|endpoint| endpoint.dev_idx == dev_idx) {
            continue;
        }
        if !mail_dev_usable(dev_idx) {
            continue;
        }
        let Some(vnet) = VNet::open(dev_idx) else {
            continue;
        };
        let listener = open_mail_listener(&vnet).await;
        let Some((_, port)) = listener else {
            continue;
        };
        let vnet_ref: &'static VNet = Box::leak(Box::new(vnet));
        if MAIL_HTTP_PORT.load(Ordering::Acquire) == 0 {
            MAIL_HTTP_PORT.store(port, Ordering::Release);
        }
        log_mail_endpoint(dev_idx, vnet_ref, port);
        endpoints.push(MailEndpoint {
            vnet: vnet_ref,
            listener,
            sessions: Vec::new(),
            dev_idx,
        });
        added += 1;
    }
    added
}

async fn mail_http_runtime() {
    tokio::task::spawn_local(crate::t::shared_tokio_job_pump());

    let mut endpoints: Vec<MailEndpoint> = Vec::new();
    loop {
        mail_add_endpoints(&mut endpoints).await;
        if !endpoints.is_empty() {
            break;
        }
        MAIL_HTTP_PORT.store(0, Ordering::Release);
        crate::log!("mail-http: waiting for a usable NIC\n");
        tokio::time::sleep(core::time::Duration::from_millis(MAIL_HTTP_VNET_OPEN_RETRY_MS)).await;
    }

    let mut endpoint_discovery_ticks = 0u32;
    loop {
        if endpoint_discovery_ticks == 0 {
            mail_add_endpoints(&mut endpoints).await;
        }
        endpoint_discovery_ticks = (endpoint_discovery_ticks + 1) % 100;

        for endpoint in endpoints.iter_mut() {
            while let Some(ev) = endpoint.vnet.pop_event() {
                match ev {
                    api::Event::TcpEstablished { handle } => {
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
                        if endpoint
                            .listener
                            .map(|(listener_handle, _)| listener_handle)
                            == Some(handle)
                        {
                            endpoint.listener = open_mail_listener(endpoint.vnet).await;
                            if let Some((_, port)) = endpoint.listener {
                                log_mail_endpoint(endpoint.dev_idx, endpoint.vnet, port);
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
                        crate::log!("mail-http: vnet error dev={} {}\n", endpoint.dev_idx, msg);
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

        tokio::time::sleep(core::time::Duration::from_millis(MAIL_HTTP_IDLE_POLL_MS)).await;
    }
}

fn run_mail_http_runtime() -> Result<(), io::Error> {
    let mut builder = tokio::runtime::Builder::new_current_thread();
    builder.enable_io();
    builder.enable_time();
    let runtime = builder.build()?;
    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime, mail_http_runtime());
    Ok(())
}

#[embassy_executor::task]
pub async fn mail_http_service_task() {
    crate::r::readiness::wait_for(
        crate::r::readiness::NET_V4_CONFIGURED | crate::r::readiness::TRUEOSFS_ROOT_MOUNTED,
    )
    .await;
    crate::log!(
        "mail-http: launching Tokio runtime after NET_V4_CONFIGURED+TRUEOSFS_ROOT_MOUNTED\n"
    );

    loop {
        let rc = crate::trueos_tokio_worker::spawn_blocking_job_with_purpose(
            Box::new(|| {
                if let Err(err) = run_mail_http_runtime() {
                    crate::log!("mail-http: runtime failed {:?}\n", err);
                }
            }),
            "mail-http-runtime",
        );
        if rc == 0 {
            crate::log!("mail-http: submitted Tokio runtime to blocking lane\n");
            core::future::pending::<()>().await;
        }
        crate::log!(
            "mail-http: blocking lane unavailable rc={} retry={}ms\n",
            rc,
            MAIL_HTTP_BLOCKING_LANE_RETRY_MS
        );
        Timer::after(EmbassyDuration::from_millis(MAIL_HTTP_BLOCKING_LANE_RETRY_MS)).await;
    }
}
