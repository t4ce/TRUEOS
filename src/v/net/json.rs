extern crate alloc;

use alloc::{format, string::String, vec::Vec};

use super::https::{FetchError, fetch_https_body_async, post_https_json_async};

#[derive(Clone, Debug)]
pub enum JsonError {
    EmptyUrl,
    InvalidUtf8,
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

        let body = fetch_https_body_async(url, self.timeout_ms, self.max_bytes).await?;
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

        let body = post_https_json_async(
            url,
            String::from(body_json),
            self.auth_token.as_deref(),
            self.timeout_ms,
            self.max_bytes,
        )
        .await?;

        String::from_utf8(body).map_err(|_| JsonError::InvalidUtf8)
    }

    pub async fn post_bytes(&self, url: &str, body_json: &str) -> Result<Vec<u8>, JsonError> {
        if url.trim().is_empty() {
            return Err(JsonError::EmptyUrl);
        }

        post_https_json_async(
            url,
            String::from(body_json),
            self.auth_token.as_deref(),
            self.timeout_ms,
            self.max_bytes,
        )
        .await
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
