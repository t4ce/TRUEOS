extern crate alloc;
extern crate std;

use alloc::{boxed::Box, format, string::String, string::ToString, vec::Vec};
use core::{
    sync::atomic::{AtomicU16, AtomicU32, Ordering},
};
use std::{io, net::SocketAddr};

use axum::{
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, OriginalUri},
    http::{
        header::{CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE},
        StatusCode,
    },
    response::Response,
    routing::{get, post},
    serve::ListenerExt,
    Router,
};
use embassy_time::{Duration as EmbassyDuration, Timer};
use serde::{Deserialize, Serialize};

use crate::{
    allports::{mail as mail_config, services::MAIL_HTTP_TCP_PORT},
    r::net::{cli::pop3::Pop3Client, smtp::SmtpClient},
};

const MAIL_HTTP_BODY_MAX: usize = 64 * 1024;
const MAIL_HTTP_BLOCKING_LANE_RETRY_MS: u64 = 1000;
const MAIL_STORE_PATH: &str = "mail/box.json";
const MAIL_CONFIG_PATH: &str = "mail/config.json";
const MAIL_SMTP_TIMEOUT_MS: u32 = 20_000;
const MAIL_POP3_TIMEOUT_MS: u32 = 20_000;
const MAIL_LIST_LIMIT: usize = 10;
const MAIL_POP3_MAX_MESSAGES: usize = MAIL_LIST_LIMIT;
const MAIL_POP3_MAX_MESSAGE_BYTES: usize = 64 * 1024;

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

impl MailConfig {
    fn static_account() -> Self {
        Self {
            smtp_user: String::from(mail_config::ACCOUNT_EMAIL),
            smtp_pass: String::from(mail_config::ACCOUNT_PASSWORD),
            from: Some(String::from(mail_config::ACCOUNT_EMAIL)),
        }
    }

    fn password_is_placeholder(&self) -> bool {
        self.smtp_pass == mail_config::ACCOUNT_PASSWORD
            || self.smtp_pass.trim().is_empty()
            || self.smtp_pass.contains("ENTER_")
    }
}

fn status_code(status: u16) -> StatusCode {
    StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

fn json_response<T: Serialize>(status: u16, value: &T) -> Response {
    let body = serde_json::to_vec(value).unwrap_or_else(|_| b"{\"ok\":false}".to_vec());
    response(status, "application/json; charset=utf-8", body, true)
}

fn text_response(status: u16, content_type: &'static str, body: &'static str) -> Response {
    response(status, content_type, body.as_bytes().to_vec(), false)
}

fn response(
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
    no_store: bool,
) -> Response {
    let mut builder = Response::builder()
        .status(status_code(status))
        .header(CONTENT_TYPE, content_type)
        .header(CONTENT_LENGTH, body.len().to_string());
    if no_store {
        builder = builder.header(CACHE_CONTROL, "no-store");
    }
    builder
        .body(Body::from(body))
        .unwrap_or_else(|_| Response::new(Body::empty()))
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

async fn ensure_mail_dir(disk: crate::disc::block::DeviceHandle) -> Result<(), &'static str> {
    match crate::r::fs::trueosfs::file_in_async(disk, "mail/.keep", &[]).await {
        Ok(true) => Ok(()),
        Ok(false) => Err("mail dir create refused"),
        Err(_) => Err("mail dir create failed"),
    }
}

async fn save_store(store: &MailStore) -> Result<(), &'static str> {
    let disk = primary_root()?;
    ensure_mail_dir(disk).await?;
    let bytes = serde_json::to_vec(store).map_err(|_| "serialize failed")?;
    match crate::r::fs::trueosfs::file_in_async(disk, MAIL_STORE_PATH, bytes.as_slice()).await {
        Ok(true) => Ok(()),
        Ok(false) => Err("mail store write refused"),
        Err(_) => Err("mail store write failed"),
    }
}

async fn load_config() -> Result<MailConfig, &'static str> {
    let disk = primary_root()?;
    match crate::r::fs::trueosfs::file_out_async(disk, MAIL_CONFIG_PATH).await {
        Ok(Some(bytes)) => {
            serde_json::from_slice::<MailConfig>(bytes.as_slice()).map_err(|_| "bad mail/config.json")
        }
        Ok(None) => Ok(MailConfig::static_account()),
        Err(_) => Err("config read failed"),
    }
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

fn header_lookup<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

fn parse_mail_headers(raw: &str) -> Vec<(String, String)> {
    let mut headers: Vec<(String, String)> = Vec::new();
    for line in raw.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some((_, value)) = headers.last_mut() {
                value.push(' ');
                value.push_str(line.trim());
            }
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        headers.push((String::from(key.trim()), String::from(value.trim())));
    }
    headers
}

fn parse_pop3_message(raw: &[u8], fallback_id: String) -> Option<MailMessage> {
    let text = core::str::from_utf8(raw).ok()?;
    let (header_text, body) = text
        .split_once("\r\n\r\n")
        .or_else(|| text.split_once("\n\n"))
        .unwrap_or((text, ""));
    let headers = parse_mail_headers(header_text);
    let id = header_lookup(&headers, "Message-ID")
        .map(header_value)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback_id);
    let from = header_lookup(&headers, "From")
        .map(header_value)
        .unwrap_or_else(|| String::from("unknown"));
    let to = header_lookup(&headers, "To")
        .map(header_value)
        .unwrap_or_default();
    let subject = header_lookup(&headers, "Subject")
        .map(header_value)
        .unwrap_or_else(|| String::from("(no subject)"));
    let date = header_lookup(&headers, "Date")
        .map(header_value)
        .unwrap_or_else(now_date_string);

    Some(MailMessage {
        id,
        from,
        to,
        subject,
        date,
        body: String::from(body.trim()),
        unread: true,
        status: String::from("received"),
        error: None,
    })
}

async fn refresh_inbox_from_pop3(config: &MailConfig) -> Result<usize, &'static str> {
    if config.password_is_placeholder() {
        return Err("mail password placeholder");
    }

    let mut client = Pop3Client::connect(MAIL_POP3_TIMEOUT_MS)
        .await
        .map_err(|_| "pop3 connect failed")?;
    client
        .login(
            config.smtp_user.as_str(),
            config.smtp_pass.as_str(),
            MAIL_POP3_TIMEOUT_MS,
        )
        .await
        .map_err(|_| "pop3 login failed")?;

    let list = client
        .list(MAIL_POP3_TIMEOUT_MS)
        .await
        .map_err(|_| "pop3 list failed")?;
    let mut store = load_store().await;
    let mut added = 0usize;

    for (msg_id, size) in list.into_iter().rev().take(MAIL_POP3_MAX_MESSAGES) {
        let fallback_id = format!("pop3-{}-{}", msg_id, size);
        if store.messages.iter().any(|message| message.id == fallback_id) {
            continue;
        }
        let raw = match client
            .retr(msg_id, MAIL_POP3_TIMEOUT_MS, MAIL_POP3_MAX_MESSAGE_BYTES)
            .await
        {
            Ok(raw) => raw,
            Err(_) => continue,
        };
        let Some(message) = parse_pop3_message(raw.as_slice(), fallback_id) else {
            continue;
        };
        if store.messages.iter().any(|existing| existing.id == message.id) {
            continue;
        }
        store.messages.push(message);
        added = added.saturating_add(1);
    }

    let _ = client.quit(5_000).await;
    if added > 0 {
        save_store(&store).await?;
    }
    Ok(added)
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
    if config.password_is_placeholder() {
        update_message_status(
            id.as_str(),
            "config-missing",
            Some(String::from("mail password placeholder")),
        )
        .await;
        return;
    }
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

async fn handle_index() -> Response {
    crate::log!("mail-http: GET /\n");
    text_response(200, "text/html; charset=utf-8", MAIL_INDEX_HTML)
}

async fn handle_app_js() -> Response {
    crate::log!("mail-http: GET /app.js\n");
    text_response(200, "application/javascript; charset=utf-8", MAIL_APP_JS)
}

async fn handle_list_local() -> Response {
    match load_config().await {
        Ok(config) => {
            let _ = refresh_inbox_from_pop3(&config).await;
        }
        Err(err) => {
            crate::log!("mail-http: config unavailable for POP3 refresh: {}\n", err);
        }
    }

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
    messages.truncate(MAIL_LIST_LIMIT);
    json_response(200, &MailListResponse { messages })
}

fn mail_worker_unavailable_response() -> Response {
    json_response(
        500,
        &serde_json::json!({"ok": false, "error": "mail worker unavailable"}),
    )
}

async fn run_mail_local<F, MakeFuture>(make_future: MakeFuture) -> Response
where
    F: core::future::Future<Output = Response> + 'static,
    MakeFuture: FnOnce() -> F + Send + 'static,
{
    let (tx, rx) = tokio::sync::oneshot::channel();
    if crate::t::spawn_on_shared_tokio(move || async move {
        let _ = tx.send(make_future().await);
    })
    .is_err()
    {
        return mail_worker_unavailable_response();
    }
    rx.await
        .unwrap_or_else(|_| mail_worker_unavailable_response())
}

async fn handle_list() -> Response {
    run_mail_local(handle_list_local).await
}

async fn handle_read(OriginalUri(uri): OriginalUri) -> Response {
    let query = uri.query().map(String::from);
    run_mail_local(move || handle_read_local(query)).await
}

async fn handle_send(body: Bytes) -> Response {
    run_mail_local(move || handle_send_local(body)).await
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

async fn handle_read_local(query: Option<String>) -> Response {
    let Some(id) = query_param(query.as_deref(), "id") else {
        return json_response(400, &serde_json::json!({"ok": false, "error": "missing id"}));
    };
    let store = load_store().await;
    match store.messages.into_iter().find(|message| message.id == id) {
        Some(message) => json_response(200, &message),
        None => json_response(404, &serde_json::json!({"ok": false, "error": "not found"})),
    }
}

async fn handle_send_local(body: Bytes) -> Response {
    if body.len() > MAIL_HTTP_BODY_MAX {
        return json_response(
            413,
            &serde_json::json!({"ok": false, "error": "request too large"}),
        );
    }
    let req = match serde_json::from_slice::<MailSendRequest>(body.as_ref()) {
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
    let config = match load_config().await {
        Ok(config) => config,
        Err(_) => MailConfig::static_account(),
    };
    let from = match config.from {
        Some(from) => from,
        None => config.smtp_user,
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
    json_response(200, &serde_json::json!({"ok": true, "id": id, "status": "queued"}))
}

fn mail_router() -> Router {
    Router::new()
        .route("/", get(handle_index))
        .route("/index.html", get(handle_index))
        .route("/app.js", get(handle_app_js))
        .route("/api/mail/list", get(handle_list))
        .route("/api/mail/read", get(handle_read))
        .route("/api/mail/send", post(handle_send))
        .layer(DefaultBodyLimit::max(MAIL_HTTP_BODY_MAX))
}

fn primary_ipv4_addr(port: u16) -> Option<SocketAddr> {
    let dev_idx = crate::net::primary_device_index();
    let ip = crate::net::adapter::ipv4_at(dev_idx)?;
    Some(SocketAddr::from((ip, port)))
}

async fn mail_http_runtime() -> Result<(), io::Error> {
    tokio::task::spawn_local(crate::t::shared_tokio_job_pump());

    let app = mail_router();
    loop {
        let Some(addr) = primary_ipv4_addr(MAIL_HTTP_TCP_PORT) else {
            MAIL_HTTP_PORT.store(0, Ordering::Release);
            crate::log!("mail-http: waiting for primary ipv4\n");
            tokio::time::sleep(core::time::Duration::from_millis(100)).await;
            continue;
        };

        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => listener,
            Err(err) => {
                MAIL_HTTP_PORT.store(0, Ordering::Release);
                crate::log!(
                    "mail-http: bind {} failed kind={:?} err={}\n",
                    addr,
                    err.kind(),
                    err
                );
                tokio::time::sleep(core::time::Duration::from_millis(1000)).await;
                continue;
            }
        };

        MAIL_HTTP_PORT.store(addr.port(), Ordering::Release);
        crate::log!("mail-http: axum listening on http://{}/\n", addr);
        let listener = listener.tap_io(|_| crate::log!("mail-http: tcp accepted\n"));
        let result = axum::serve(listener, app).await;
        if result.is_err() {
            MAIL_HTTP_PORT.store(0, Ordering::Release);
        }
        return result;
    }
}

fn run_mail_http_runtime() -> Result<(), io::Error> {
    let mut builder = tokio::runtime::Builder::new_current_thread();
    builder.enable_io();
    builder.enable_time();
    let runtime = builder.build()?;
    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime, mail_http_runtime())
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
