#![allow(dead_code)]

extern crate alloc;

use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};

use crate::t::net::https::{FetchError, fetch_https_body_hyper_async, post_https_json_hyper_async};

#[derive(Clone, Debug)]
pub enum JsonError {
    EmptyUrl,
    InvalidUtf8,
    RuntimeUnavailable,
    Fetch(FetchError),
}

impl From<FetchError> for JsonError {
    fn from(value: FetchError) -> Self {
        Self::Fetch(value)
    }
}

#[derive(Clone, Debug)]
pub struct JsonClient {
    timeout_ms: u32,
    max_bytes: usize,
    auth_token: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct JsonRequestOptions {
    timeout_ms: Option<u32>,
    max_bytes: Option<usize>,
    auth_token: Option<String>,
}

impl JsonRequestOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_timeout_ms(mut self, timeout_ms: u32) -> Self {
        self.timeout_ms = Some(timeout_ms.max(1));
        self
    }

    pub fn with_max_bytes(mut self, max_bytes: usize) -> Self {
        self.max_bytes = Some(max_bytes.max(1024));
        self
    }

    pub fn with_auth_token(mut self, token: impl Into<String>) -> Self {
        self.auth_token = Some(token.into());
        self
    }
}

impl Default for JsonClient {
    fn default() -> Self {
        Self {
            timeout_ms: 15_000,
            max_bytes: 128 * 1024,
            auth_token: None,
        }
    }
}

impl JsonClient {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_timeout_ms(mut self, timeout_ms: u32) -> Self {
        self.timeout_ms = timeout_ms.max(1);
        self
    }

    pub fn with_max_bytes(mut self, max_bytes: usize) -> Self {
        self.max_bytes = max_bytes.max(1024);
        self
    }

    pub fn with_auth_token(mut self, token: impl Into<String>) -> Self {
        self.auth_token = Some(token.into());
        self
    }

    pub fn clear_auth_token(mut self) -> Self {
        self.auth_token = None;
        self
    }

    pub fn from_options(options: &JsonRequestOptions) -> Self {
        let mut cli = Self::default();
        if let Some(timeout_ms) = options.timeout_ms {
            cli.timeout_ms = timeout_ms;
        }
        if let Some(max_bytes) = options.max_bytes {
            cli.max_bytes = max_bytes;
        }
        cli.auth_token = options.auth_token.clone();
        cli
    }

    pub async fn get(&self, url: &str) -> Result<String, JsonError> {
        if url.trim().is_empty() {
            return Err(JsonError::EmptyUrl);
        }

        let url = String::from(url);
        let timeout_ms = self.timeout_ms;
        let max_bytes = self.max_bytes;
        let body = crate::t::run_on_shared_tokio(move || async move {
            fetch_https_body_hyper_async(url.as_str(), timeout_ms, max_bytes).await
        })
        .await
        .map_err(|_| JsonError::RuntimeUnavailable)??;
        String::from_utf8(body).map_err(|_| JsonError::InvalidUtf8)
    }

    pub async fn get_with_query(
        &self,
        base_url: &str,
        params: &[(&str, &str)],
    ) -> Result<String, JsonError> {
        let url = build_query_url(base_url, params);
        self.get(url.as_str()).await
    }

    pub async fn post(&self, url: &str, body_json: &str) -> Result<String, JsonError> {
        if url.trim().is_empty() {
            return Err(JsonError::EmptyUrl);
        }

        let url = String::from(url);
        let body_json = String::from(body_json);
        let auth_token = self.auth_token.clone();
        let timeout_ms = self.timeout_ms;
        let max_bytes = self.max_bytes;
        let body = crate::t::run_on_shared_tokio(move || async move {
            post_https_json_hyper_async(
                url.as_str(),
                body_json,
                auth_token.as_deref(),
                timeout_ms,
                max_bytes,
            )
            .await
        })
        .await
        .map_err(|_| JsonError::RuntimeUnavailable)??;

        String::from_utf8(body).map_err(|_| JsonError::InvalidUtf8)
    }

    pub async fn post_bytes(&self, url: &str, body_json: &str) -> Result<Vec<u8>, JsonError> {
        if url.trim().is_empty() {
            return Err(JsonError::EmptyUrl);
        }

        let url = String::from(url);
        let body_json = String::from(body_json);
        let auth_token = self.auth_token.clone();
        let timeout_ms = self.timeout_ms;
        let max_bytes = self.max_bytes;
        crate::t::run_on_shared_tokio(move || async move {
            post_https_json_hyper_async(
                url.as_str(),
                body_json,
                auth_token.as_deref(),
                timeout_ms,
                max_bytes,
            )
            .await
        })
        .await
        .map_err(|_| JsonError::RuntimeUnavailable)?
        .map_err(JsonError::from)
    }
}

pub fn build_query_url(base_url: &str, params: &[(&str, &str)]) -> String {
    let trimmed = base_url.trim();
    if params.is_empty() {
        return String::from(trimmed);
    }

    let has_query = trimmed.as_bytes().contains(&b'?');
    let mut out = String::from(trimmed);
    out.push(if has_query { '&' } else { '?' });

    for (idx, (k, v)) in params.iter().enumerate() {
        if idx > 0 {
            out.push('&');
        }
        out.push_str(url_encode(k).as_str());
        out.push('=');
        out.push_str(url_encode(v).as_str());
    }

    out
}

pub fn url_encode(input: &str) -> String {
    let mut out = String::new();
    for &b in input.as_bytes() {
        // RFC3986 unreserved characters.
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push('%');
            out.push(hex((b >> 4) & 0x0F));
            out.push(hex(b & 0x0F));
        }
    }
    out
}

#[inline]
fn hex(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        _ => (b'A' + (n - 10)) as char,
    }
}

pub async fn get_json(url: &str) -> Result<String, JsonError> {
    JsonClient::default().get(url).await
}

/// Preferred kernel JSON GET path: Hyper HTTP/1 over TRUEOS TLS/VNet.
pub async fn get_json_hyper(url: &str) -> Result<String, JsonError> {
    get_json(url).await
}

pub async fn get_json_with_query(
    base_url: &str,
    params: &[(&str, &str)],
) -> Result<String, JsonError> {
    JsonClient::default().get_with_query(base_url, params).await
}

pub async fn get_json_with_options(
    base_url: &str,
    params: &[(&str, &str)],
    options: &JsonRequestOptions,
) -> Result<String, JsonError> {
    JsonClient::from_options(options)
        .get_with_query(base_url, params)
        .await
}

pub async fn post_json(url: &str, body_json: &str) -> Result<String, JsonError> {
    JsonClient::default().post(url, body_json).await
}

pub async fn post_json_with_options(
    url: &str,
    body_json: &str,
    options: &JsonRequestOptions,
) -> Result<String, JsonError> {
    JsonClient::from_options(options).post(url, body_json).await
}

pub fn summarize_openai_response_json(body: &str) -> Option<String> {
    let model = json_find_string(body, "model").unwrap_or_default();
    let answer = json_find_output_text(body)
        .or_else(|| json_find_string(body, "output_text"))
        .unwrap_or_default();
    let input_tokens = json_find_number(body, "input_tokens");
    let output_tokens = json_find_number(body, "output_tokens");
    let reasoning_effort = json_find_string(body, "effort").unwrap_or_default();
    let reasoning_summary = json_find_string(body, "summary").unwrap_or_default();
    let tools = json_collect_tool_types(body);

    let mut parts: Vec<String> = Vec::new();
    if !model.is_empty() {
        parts.push(format!("model={}", model));
    }
    if !answer.is_empty() {
        parts.push(format!("answer=\"{}\"", clip_for_log(&answer, 160)));
    }
    if let Some(v) = input_tokens {
        parts.push(format!("in={}", v));
    }
    if let Some(v) = output_tokens {
        parts.push(format!("out={}", v));
    }
    if !reasoning_effort.is_empty() && reasoning_effort != "none" {
        parts.push(format!("effort={}", reasoning_effort));
    }
    if !reasoning_summary.is_empty() && reasoning_summary != "null" {
        parts.push(format!("summary=\"{}\"", clip_for_log(&reasoning_summary, 160)));
    }
    if !tools.is_empty() {
        parts.push(format!("tools={}", tools.join(",")));
    }

    (!parts.is_empty()).then(|| parts.join(" "))
}

pub fn nominatim_search_url(query: &str, limit: u8) -> String {
    let n = limit.clamp(1, 50);
    let limit_s = format!("{}", n);
    build_query_url(
        "https://nominatim.openstreetmap.org/search",
        &[
            ("q", query),
            ("format", "json"),
            ("limit", limit_s.as_str()),
        ],
    )
}

fn clip_for_log(input: &str, max_chars: usize) -> String {
    let mut out = String::new();
    let mut last_was_space = false;
    for ch in input.chars() {
        let ch = if ch.is_whitespace() { ' ' } else { ch };
        if ch == ' ' {
            if last_was_space {
                continue;
            }
            last_was_space = true;
        } else {
            last_was_space = false;
        }
        if out.len() + ch.len_utf8() > max_chars {
            out.push_str("...");
            break;
        }
        if ch == '"' {
            out.push('\'');
        } else {
            out.push(ch);
        }
    }
    out.trim().to_string()
}

fn json_find_key_value_start(body: &str, key: &str) -> Option<usize> {
    let pattern = format!("\"{}\"", key);
    let mut start_at = 0usize;
    while let Some(rel) = body.get(start_at..)?.find(pattern.as_str()) {
        let key_pos = start_at + rel;
        let mut i = key_pos + pattern.len();
        i = json_skip_ws(body, i);
        if body.as_bytes().get(i).copied() != Some(b':') {
            start_at = key_pos + pattern.len();
            continue;
        }
        i += 1;
        return Some(json_skip_ws(body, i));
    }
    None
}

fn json_skip_ws(body: &str, mut i: usize) -> usize {
    while let Some(b) = body.as_bytes().get(i).copied() {
        if !matches!(b, b' ' | b'\n' | b'\r' | b'\t') {
            break;
        }
        i += 1;
    }
    i
}

fn json_find_string(body: &str, key: &str) -> Option<String> {
    let i = json_find_key_value_start(body, key)?;
    if body.as_bytes().get(i).copied() != Some(b'"') {
        return None;
    }
    json_parse_string(body, i).map(|(value, _)| value)
}

fn json_find_number(body: &str, key: &str) -> Option<u64> {
    let i = json_find_key_value_start(body, key)?;
    let bytes = body.as_bytes();
    let mut end = i;
    while let Some(b) = bytes.get(end).copied() {
        if !b.is_ascii_digit() {
            break;
        }
        end += 1;
    }
    (end > i)
        .then(|| body.get(i..end)?.parse::<u64>().ok())
        .flatten()
}

fn json_find_output_text(body: &str) -> Option<String> {
    let mut start_at = 0usize;
    while let Some(rel) = body.get(start_at..)?.find("\"type\"") {
        let key_pos = start_at + rel;
        let Some(val_pos_rel) =
            json_find_key_value_start(body.get(key_pos..).unwrap_or_default(), "type")
        else {
            start_at = key_pos + 6;
            continue;
        };
        let val_pos = key_pos + val_pos_rel;
        let Some((value, after)) = json_parse_string(body, val_pos) else {
            start_at = key_pos + 6;
            continue;
        };
        if value == "output_text" {
            let Some(text_pos_rel) =
                json_find_key_value_start(body.get(after..).unwrap_or_default(), "text")
            else {
                start_at = after;
                continue;
            };
            let text_pos = after + text_pos_rel;
            if let Some((text, _)) = json_parse_string(body, text_pos) {
                return Some(text);
            }
        }
        start_at = after;
    }
    None
}

fn json_collect_tool_types(body: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut start_at = 0usize;
    while let Some(rel) = body.get(start_at..).and_then(|s| s.find("\"type\"")) {
        let key_pos = start_at + rel;
        let Some(val_pos_rel) =
            json_find_key_value_start(body.get(key_pos..).unwrap_or_default(), "type")
        else {
            start_at = key_pos + 6;
            continue;
        };
        let val_pos = key_pos + val_pos_rel;
        let Some((value, after)) = json_parse_string(body, val_pos) else {
            start_at = key_pos + 6;
            continue;
        };
        let name = if let Some(stripped) = value.strip_suffix("_call") {
            stripped
        } else {
            ""
        };
        if !name.is_empty()
            && name != "output_text"
            && name != "input_text"
            && name != "message"
            && !out.iter().any(|it| it == name)
        {
            out.push(name.to_string());
        }
        start_at = after;
    }
    out
}

fn json_parse_string(body: &str, start: usize) -> Option<(String, usize)> {
    let bytes = body.as_bytes();
    if bytes.get(start).copied() != Some(b'"') {
        return None;
    }
    let mut i = start + 1;
    let mut out = String::new();
    while let Some(b) = bytes.get(i).copied() {
        match b {
            b'"' => return Some((out, i + 1)),
            b'\\' => {
                let esc = bytes.get(i + 1).copied()?;
                match esc {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'b' => out.push('\u{0008}'),
                    b'f' => out.push('\u{000C}'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => {
                        let hex = body.get(i + 2..i + 6)?;
                        if let Ok(code) = u16::from_str_radix(hex, 16) {
                            if let Some(ch) = char::from_u32(code as u32) {
                                out.push(ch);
                            }
                        }
                        i += 4;
                    }
                    _ => out.push(esc as char),
                }
                i += 2;
            }
            _ => {
                out.push(b as char);
                i += 1;
            }
        }
    }
    None
}
