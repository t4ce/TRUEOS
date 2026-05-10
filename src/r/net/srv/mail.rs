extern crate alloc;
extern crate std;

use alloc::{boxed::Box, format, string::String, string::ToString, vec::Vec};
use core::{
    sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, Ordering},
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
const MAIL_CONFIG_PASSWORD_PLACEHOLDER: &str = "ENTER_MAIL_PASSWORD_HERE";
const MAIL_SMTP_TIMEOUT_MS: u32 = 20_000;
const MAIL_POP3_TIMEOUT_MS: u32 = 20_000;
const MAIL_LIST_LIMIT: usize = 10;
const MAIL_POP3_MAX_MESSAGES: usize = MAIL_LIST_LIMIT;
const MAIL_POP3_TOP_BODY_LINES: u32 = 80;
const MAIL_POP3_TOP_MAX_BYTES: usize = 128 * 1024;
const MAIL_POP3_MAX_MESSAGE_BYTES: usize = 512 * 1024;
const MAIL_INBOX_REFRESH_INTERVAL_SECS: u64 = 30;

static MAIL_HTTP_PORT: AtomicU16 = AtomicU16::new(0);
static MAIL_ID_SEQ: AtomicU32 = AtomicU32::new(1);
static MAIL_INBOX_REFRESH_RUNNING: AtomicBool = AtomicBool::new(false);
static MAIL_INBOX_LAST_REFRESH_SECS: AtomicU64 = AtomicU64::new(0);
static MAIL_INBOX_LAST_REFRESH_ADDED: AtomicU32 = AtomicU32::new(0);
static MAIL_INBOX_LAST_LIST_COUNT: AtomicU32 = AtomicU32::new(0);
static MAIL_INBOX_LAST_RETRIEVED: AtomicU32 = AtomicU32::new(0);
static MAIL_INBOX_LAST_PARSED: AtomicU32 = AtomicU32::new(0);

const WEBMAIL_INDEX_HTML: &str = include_str!("../../../tst/webmail/index.html");
const WEBMAIL_APP_JS: &str = include_str!("../../../tst/webmail/app.js");

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pop3_msg_id: Option<u32>,
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
    status: String,
    error: Option<String>,
}

#[derive(Deserialize)]
struct MailSendRequest {
    to: String,
    subject: String,
    body: String,
}

#[derive(Clone, Deserialize)]
struct MailConfig {
    smtp_user: String,
    smtp_pass: String,
    from: Option<String>,
}

struct LoadedMailConfig {
    config: MailConfig,
    source: &'static str,
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
        self.smtp_pass.trim().is_empty() || self.smtp_pass.contains("ENTER_")
    }

    fn merge_with_static(mut self) -> Self {
        let static_config = Self::static_account();
        if self.smtp_user.trim().is_empty() {
            self.smtp_user = static_config.smtp_user.clone();
        }
        if self.password_is_placeholder() {
            self.smtp_pass = static_config.smtp_pass.clone();
        }
        if self
            .from
            .as_deref()
            .map(|from| from.trim().is_empty())
            .unwrap_or(true)
        {
            self.from = static_config.from.clone();
        }
        self
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

async fn write_config_template(disk: crate::disc::block::DeviceHandle) -> Result<(), &'static str> {
    ensure_mail_dir(disk).await?;
    let template = serde_json::json!({
        "smtp_user": mail_config::ACCOUNT_EMAIL,
        "smtp_pass": MAIL_CONFIG_PASSWORD_PLACEHOLDER,
        "from": mail_config::ACCOUNT_EMAIL,
        "smtp_host": mail_config::SMTP_HOST,
        "smtp_port": mail_config::SMTP_PORT,
        "pop3_host": mail_config::POP3_HOST,
        "pop3_port": mail_config::POP3_PORT,
        "note": "Fill smtp_pass with the mailbox password. The kernel falls back to allports.rs while this placeholder remains."
    });
    let bytes = serde_json::to_vec_pretty(&template).map_err(|_| "config template serialize failed")?;
    match crate::r::fs::trueosfs::file_in_async(disk, MAIL_CONFIG_PATH, bytes.as_slice()).await {
        Ok(true) => Ok(()),
        Ok(false) => Err("config template write refused"),
        Err(_) => Err("config template write failed"),
    }
}

async fn load_config() -> Result<LoadedMailConfig, &'static str> {
    let disk = primary_root()?;
    match crate::r::fs::trueosfs::file_out_async(disk, MAIL_CONFIG_PATH).await {
        Ok(Some(bytes)) => match serde_json::from_slice::<MailConfig>(bytes.as_slice()) {
            Ok(config) => {
                crate::log!("webmail-http: config source={}\n", MAIL_CONFIG_PATH);
                Ok(LoadedMailConfig {
                    config: config.merge_with_static(),
                    source: MAIL_CONFIG_PATH,
                })
            }
            Err(_) => {
                crate::log!(
                    "webmail-http: bad {}; falling back to static account\n",
                    MAIL_CONFIG_PATH
                );
                Ok(LoadedMailConfig {
                    config: MailConfig::static_account(),
                    source: "static-bad-config",
                })
            }
        },
        Ok(None) => {
            match write_config_template(disk).await {
                Ok(()) => crate::log!(
                    "webmail-http: wrote config template path={} source=allports\n",
                    MAIL_CONFIG_PATH
                ),
                Err(err) => crate::log!(
                    "webmail-http: config template unavailable path={} err={} source=allports\n",
                    MAIL_CONFIG_PATH,
                    err
                ),
            }
            Ok(LoadedMailConfig {
                config: MailConfig::static_account(),
                source: "allports-template",
            })
        }
        Err(_) => Err("config read failed"),
    }
}

fn now_date_string() -> String {
    rfc2822_date_string(now_mail_seconds())
}

fn now_mail_seconds() -> u64 {
    crate::r::net::ntp::current_unix_seconds()
        .or_else(crate::time::unix_time_seconds)
        .unwrap_or_else(crate::time::uptime_seconds)
}

fn next_mail_id() -> String {
    let seq = MAIL_ID_SEQ.fetch_add(1, Ordering::Relaxed).max(1);
    let secs = crate::r::net::ntp::current_unix_seconds()
        .or_else(crate::time::unix_time_seconds)
        .unwrap_or_else(crate::time::uptime_seconds);
    format!("mail-{}-{}", secs, seq)
}

fn rfc2822_date_string(ts: u64) -> String {
    const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let days = ts / 86_400;
    let rem = ts % 86_400;
    let hour = rem / 3_600;
    let minute = (rem % 3_600) / 60;
    let second = rem % 60;
    let weekday = WEEKDAYS[((days + 4) % 7) as usize];
    let (year, month, day) = civil_from_days(days as i64);
    let month_name = MONTHS[(month.saturating_sub(1) as usize).min(MONTHS.len() - 1)];
    format!(
        "{}, {:02} {} {:04} {:02}:{:02}:{:02} +0000",
        weekday, day, month_name, year, hour, minute, second
    )
}

fn civil_from_days(days_since_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }
    (year as i32, month as u32, day as u32)
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
        "From: <{}>\r\nTo: <{}>\r\nSubject: {}\r\nDate: {}\r\nMessage-ID: <{}@{}>\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=US-ASCII\r\nContent-Transfer-Encoding: 7bit\r\nX-Mailer: TRUEOS Webmail\r\n\r\n{}",
        header_value(from),
        header_value(to),
        header_value(subject),
        now_date_string(),
        header_value(id),
        header_value(from_domain),
        sanitize_7bit_body(body)
    )
}

fn sanitize_7bit_body(body: &str) -> String {
    body.chars()
        .map(|ch| match ch {
            '\r' | '\n' | '\t' => ch,
            ch if ch.is_ascii() && !ch.is_control() => ch,
            _ => '?',
        })
        .collect()
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

fn parse_pop3_message(raw: &[u8], fallback_id: String, pop3_msg_id: u32) -> Option<MailMessage> {
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
        pop3_msg_id: Some(pop3_msg_id),
    })
}

async fn refresh_inbox_from_pop3(config: &MailConfig) -> Result<usize, &'static str> {
    if config.password_is_placeholder() {
        return Err("mail password placeholder");
    }

    crate::log!("webmail-http: pop3 refresh connect host={}\n", mail_config::POP3_HOST);
    let mut client = Pop3Client::connect(MAIL_POP3_TIMEOUT_MS)
        .await
        .map_err(|err| {
            crate::log!("webmail-http: pop3 connect failed err={:?}\n", err);
            "pop3 connect failed"
        })?;
    crate::log!("webmail-http: pop3 refresh login user={}\n", config.smtp_user.as_str());
    client
        .login(
            config.smtp_user.as_str(),
            config.smtp_pass.as_str(),
            MAIL_POP3_TIMEOUT_MS,
        )
        .await
        .map_err(|err| {
            crate::log!("webmail-http: pop3 login failed err={:?}\n", err);
            "pop3 login failed"
        })?;

    let (count, total_bytes) = client
        .stat(MAIL_POP3_TIMEOUT_MS)
        .await
        .map_err(|err| {
            crate::log!("webmail-http: pop3 stat failed err={:?}\n", err);
            "pop3 stat failed"
        })?;
    MAIL_INBOX_LAST_LIST_COUNT.store(count, Ordering::Release);
    crate::log!(
        "webmail-http: pop3 stat count={} bytes={} taking={}\n",
        count,
        total_bytes,
        MAIL_POP3_MAX_MESSAGES
    );

    let mut latest: Vec<(u32, u64)> = Vec::new();
    let first_id = count
        .saturating_sub(MAIL_POP3_MAX_MESSAGES as u32)
        .saturating_add(1);
    for msg_id in (first_id..=count).rev() {
        match client.list_one(msg_id, MAIL_POP3_TIMEOUT_MS).await {
            Ok(entry) => latest.push(entry),
            Err(err) => crate::log!(
                "webmail-http: pop3 LIST {} failed err={:?}; skipping\n",
                msg_id,
                err
            ),
        }
    }
    let mut store = load_store().await;
    let mut added = 0usize;
    let mut retrieved = 0usize;
    let mut parsed = 0usize;
    let latest_ids: Vec<u32> = latest.iter().map(|(msg_id, _)| *msg_id).collect();
    crate::log!(
        "webmail-http: pop3 latest listed count={} taking={}\n",
        MAIL_INBOX_LAST_LIST_COUNT.load(Ordering::Acquire),
        latest.len()
    );

    for (msg_id, size) in latest.into_iter() {
        let fallback_id = format!("pop3-{}-{}", msg_id, size);
        if let Some(existing) = store.messages.iter_mut().find(|message| message.id == fallback_id)
        {
            existing.pop3_msg_id = Some(msg_id);
            continue;
        }
        let raw = match client
            .top(
                msg_id,
                MAIL_POP3_TOP_BODY_LINES,
                MAIL_POP3_TIMEOUT_MS,
                MAIL_POP3_TOP_MAX_BYTES,
            )
            .await
        {
            Ok(raw) => raw,
            Err(top_err) => {
                crate::log!(
                    "webmail-http: pop3 TOP failed msg={} size={} err={:?}; trying RETR\n",
                    msg_id,
                    size,
                    top_err
                );
                match client
                    .retr(msg_id, MAIL_POP3_TIMEOUT_MS, MAIL_POP3_MAX_MESSAGE_BYTES)
                    .await
                {
                    Ok(raw) => raw,
                    Err(retr_err) => {
                        crate::log!(
                            "webmail-http: pop3 RETR failed msg={} size={} err={:?}\n",
                            msg_id,
                            size,
                            retr_err
                        );
                        continue;
                    }
                }
            }
        };
        retrieved = retrieved.saturating_add(1);
        let Some(message) = parse_pop3_message(raw.as_slice(), fallback_id, msg_id) else {
            crate::log!("webmail-http: pop3 parse failed msg={} bytes={}\n", msg_id, raw.len());
            continue;
        };
        parsed = parsed.saturating_add(1);
        if let Some(existing) = store.messages.iter_mut().find(|existing| existing.id == message.id)
        {
            existing.pop3_msg_id = Some(msg_id);
            continue;
        }
        store.messages.push(message);
        added = added.saturating_add(1);
    }

    let _ = client.quit(5_000).await;
    MAIL_INBOX_LAST_RETRIEVED.store(retrieved as u32, Ordering::Release);
    MAIL_INBOX_LAST_PARSED.store(parsed as u32, Ordering::Release);
    let before_retain = store.messages.len();
    store.messages.retain(|message| {
        message.status != "received"
            || message
                .pop3_msg_id
                .map(|msg_id| latest_ids.contains(&msg_id))
                .unwrap_or(true)
    });
    if added > 0 || store.messages.len() != before_retain {
        save_store(&store).await?;
    }
    crate::log!(
        "webmail-http: pop3 refresh done retrieved={} parsed={} added={} retained={}\n",
        retrieved,
        parsed,
        added,
        store.messages.len()
    );
    Ok(added)
}

async fn refresh_inbox_once(reason: &'static str) -> Result<usize, &'static str> {
    if MAIL_INBOX_REFRESH_RUNNING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        crate::log!("webmail-http: inbox refresh skip reason={} busy=1\n", reason);
        return Err("mail refresh already running");
    }

    let result = async {
        let loaded = load_config().await?;
        let result = refresh_inbox_from_pop3(&loaded.config).await;
        if result == Err("pop3 login failed") && loaded.source == MAIL_CONFIG_PATH {
            crate::log!(
                "webmail-http: pop3 login failed with {}; retrying allports account\n",
                MAIL_CONFIG_PATH
            );
            let static_config = MailConfig::static_account();
            refresh_inbox_from_pop3(&static_config).await
        } else {
            result
        }
    }
    .await;

    match result {
        Ok(added) => {
            MAIL_INBOX_LAST_REFRESH_SECS.store(now_mail_seconds(), Ordering::Release);
            MAIL_INBOX_LAST_REFRESH_ADDED.store(added as u32, Ordering::Release);
            crate::log!(
                "webmail-http: inbox refresh ok reason={} added={} limit={}\n",
                reason,
                added,
                MAIL_POP3_MAX_MESSAGES
            );
        }
        Err(err) => {
            crate::log!(
                "webmail-http: inbox refresh failed reason={} err={}\n",
                reason,
                err
            );
        }
    }

    MAIL_INBOX_REFRESH_RUNNING.store(false, Ordering::Release);
    result
}

async fn inbox_refresh_loop() {
    crate::log!(
        "webmail-http: inbox refresh loop interval={}s\n",
        MAIL_INBOX_REFRESH_INTERVAL_SECS
    );
    let _ = refresh_inbox_once("startup").await;
    loop {
        tokio::time::sleep(core::time::Duration::from_secs(
            MAIL_INBOX_REFRESH_INTERVAL_SECS,
        ))
        .await;
        let _ = refresh_inbox_once("interval").await;
    }
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
    let loaded = match load_config().await {
        Ok(loaded) => loaded,
        Err(err) => {
            update_message_status(id.as_str(), "config-missing", Some(String::from(err))).await;
            return;
        }
    };
    let config = loaded.config;
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
    crate::log!(
        "webmail-http: smtp send begin id={} from={} rcpts={} bytes={}\n",
        id.as_str(),
        from,
        rcpt_refs.len(),
        wire.len()
    );
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
        Ok(()) => {
            crate::log!("webmail-http: smtp send ok id={}\n", id.as_str());
            update_message_status(id.as_str(), "sent", None).await
        }
        Err(err) => {
            crate::log!("webmail-http: smtp send failed id={} err={:?}\n", id.as_str(), err);
            update_message_status(id.as_str(), "send-failed", Some(format!("{:?}", err))).await
        }
    }
}

async fn handle_index() -> Response {
    crate::log!("webmail-http: GET /\n");
    text_response(200, "text/html; charset=utf-8", WEBMAIL_INDEX_HTML)
}

async fn handle_app_js() -> Response {
    crate::log!("webmail-http: GET /app.js\n");
    text_response(200, "application/javascript; charset=utf-8", WEBMAIL_APP_JS)
}

async fn handle_list_local() -> Response {
    crate::log!("webmail-http: api list\n");
    let mut inbox: Vec<MailMessage> = load_store()
        .await
        .messages
        .into_iter()
        .filter(|message| message.status == "received")
        .collect();
    inbox.sort_by(|a, b| {
        b.pop3_msg_id
            .unwrap_or(0)
            .cmp(&a.pop3_msg_id.unwrap_or(0))
    });
    let mut messages: Vec<MailSummary> = inbox
        .into_iter()
        .map(|message| MailSummary {
            id: message.id,
            from: message.from,
            subject: message.subject,
            preview: preview(message.body.as_str()),
            date: message.date,
            unread: message.unread,
            status: message.status,
            error: message.error,
        })
        .collect();
    messages.truncate(MAIL_LIST_LIMIT);
    json_response(200, &MailListResponse { messages })
}

async fn handle_status_local() -> Response {
    crate::log!("webmail-http: api status\n");
    let store = load_store().await;
    let loaded_config = load_config().await.ok();
    let account = loaded_config
        .as_ref()
        .map(|loaded| {
            loaded
                .config
                .from
                .as_deref()
                .unwrap_or(loaded.config.smtp_user.as_str())
        })
        .unwrap_or(mail_config::ACCOUNT_EMAIL);
    let config_source = loaded_config
        .as_ref()
        .map(|loaded| loaded.source)
        .unwrap_or("unavailable");
    let inbox_count = store
        .messages
        .iter()
        .filter(|message| message.status == "received")
        .count();
    let unread_count = store
        .messages
        .iter()
        .filter(|message| message.status == "received" && message.unread)
        .count();
    json_response(
        200,
        &serde_json::json!({
            "ok": true,
            "service": "webmail-http",
            "account": account,
            "configSource": config_source,
            "storePath": MAIL_STORE_PATH,
            "configPath": MAIL_CONFIG_PATH,
            "smtp": format!("{}:{}", mail_config::SMTP_HOST, mail_config::SMTP_PORT),
            "pop3": format!("{}:{}", mail_config::POP3_HOST, mail_config::POP3_PORT),
            "messageCount": store.messages.len(),
            "inboxCount": inbox_count,
            "unreadCount": unread_count,
            "listLimit": MAIL_LIST_LIMIT,
            "refreshIntervalSeconds": MAIL_INBOX_REFRESH_INTERVAL_SECS,
            "lastRefreshUnix": MAIL_INBOX_LAST_REFRESH_SECS.load(Ordering::Acquire),
            "lastRefreshAdded": MAIL_INBOX_LAST_REFRESH_ADDED.load(Ordering::Acquire),
            "lastPop3ListCount": MAIL_INBOX_LAST_LIST_COUNT.load(Ordering::Acquire),
            "lastPop3Retrieved": MAIL_INBOX_LAST_RETRIEVED.load(Ordering::Acquire),
            "lastPop3Parsed": MAIL_INBOX_LAST_PARSED.load(Ordering::Acquire),
            "refreshRunning": MAIL_INBOX_REFRESH_RUNNING.load(Ordering::Acquire),
            "readiness": crate::r::readiness::mask(),
            "port": current_port(),
        }),
    )
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

async fn handle_refresh_local() -> Response {
    crate::log!("webmail-http: api refresh\n");
    match refresh_inbox_once("manual").await {
        Ok(added) => json_response(200, &serde_json::json!({"ok": true, "added": added})),
        Err(err) if err == "mail refresh already running" => {
            json_response(202, &serde_json::json!({"ok": true, "busy": true}))
        }
        Err(err) => json_response(200, &serde_json::json!({"ok": false, "error": err})),
    }
}

async fn handle_refresh() -> Response {
    run_mail_local(handle_refresh_local).await
}

async fn handle_status() -> Response {
    run_mail_local(handle_status_local).await
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
    crate::log!("webmail-http: api read\n");
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
    crate::log!("webmail-http: api send bytes={}\n", body.len());
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
        Ok(loaded) => loaded.config,
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
        pop3_msg_id: None,
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
        .route("/healthz", get(handle_status))
        .route("/api/healthz", get(handle_status))
        .route("/api/webmail/status", get(handle_status))
        .route("/api/webmail/refresh", get(handle_refresh).post(handle_refresh))
        .route("/api/webmail/list", get(handle_list))
        .route("/api/webmail/read", get(handle_read))
        .route("/api/webmail/send", post(handle_send))
        .layer(DefaultBodyLimit::max(MAIL_HTTP_BODY_MAX))
}

fn primary_ipv4_addr(port: u16) -> Option<SocketAddr> {
    let dev_idx = crate::net::primary_device_index();
    let ip = crate::net::adapter::ipv4_at(dev_idx)?;
    Some(SocketAddr::from((ip, port)))
}

async fn mail_http_runtime() -> Result<(), io::Error> {
    tokio::task::spawn_local(crate::t::shared_tokio_job_pump());
    tokio::task::spawn_local(inbox_refresh_loop());

    let app = mail_router();
    loop {
        let Some(addr) = primary_ipv4_addr(MAIL_HTTP_TCP_PORT) else {
            MAIL_HTTP_PORT.store(0, Ordering::Release);
            crate::log!("webmail-http: waiting for primary ipv4\n");
            tokio::time::sleep(core::time::Duration::from_millis(100)).await;
            continue;
        };

        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => listener,
            Err(err) => {
                MAIL_HTTP_PORT.store(0, Ordering::Release);
                crate::log!(
                    "webmail-http: bind {} failed kind={:?} err={}\n",
                    addr,
                    err.kind(),
                    err
                );
                tokio::time::sleep(core::time::Duration::from_millis(1000)).await;
                continue;
            }
        };

        MAIL_HTTP_PORT.store(addr.port(), Ordering::Release);
        crate::log!("webmail-http: axum listening on http://{}/\n", addr);
        let listener = listener.tap_io(|_| crate::log!("webmail-http: tcp accepted\n"));
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
        "webmail-http: launching Tokio runtime after NET_V4_CONFIGURED+TRUEOSFS_ROOT_MOUNTED\n"
    );

    loop {
        let rc = crate::trueos_tokio_worker::spawn_blocking_job_with_purpose(
            Box::new(|| {
                if let Err(err) = run_mail_http_runtime() {
                    crate::log!("webmail-http: runtime failed {:?}\n", err);
                }
            }),
            "webmail-http-runtime",
        );
        if rc == 0 {
            crate::log!("webmail-http: submitted Tokio runtime to blocking lane\n");
            core::future::pending::<()>().await;
        }
        crate::log!(
            "webmail-http: blocking lane unavailable rc={} retry={}ms\n",
            rc,
            MAIL_HTTP_BLOCKING_LANE_RETRY_MS
        );
        Timer::after(EmbassyDuration::from_millis(MAIL_HTTP_BLOCKING_LANE_RETRY_MS)).await;
    }
}
