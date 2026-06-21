extern crate alloc;

include!("../cabi_codes.rs");

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::{vec, vec::Vec};
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use spin::Mutex;
use v::vnet;

use crate::net::tls::{TlsClientConfig, TlsRoots};
use crate::net::tls_socket::{TlsCommand, TlsEvent, TlsTimeouts, register_tls_app_queues};
use crate::r::net::{NetProfile, Queue};

static CABI_NET_FETCH_SEQ: AtomicU32 = AtomicU32::new(1);
static HTTPS_FETCH_TLS_SEQ: AtomicU32 = AtomicU32::new(1);
static CABI_NET_FETCH_RESULTS: Mutex<BTreeMap<u32, Option<i32>>> = Mutex::new(BTreeMap::new());
static CABI_NET_FETCH_BYTES_RESULTS: Mutex<BTreeMap<u32, CabiNetFetchBytesResult>> =
    Mutex::new(BTreeMap::new());

#[derive(Default)]
struct CabiNetFetchBytesResult {
    rc: Option<i32>,
    body: Vec<u8>,
}

fn monotonic_ms() -> u64 {
    let hz = embassy_time_driver::TICK_HZ.max(1);
    embassy_time_driver::now().saturating_mul(1000) / hz
}

fn fetch_error_to_code(err: &str) -> i32 {
    if err == "timed out" || err == "timeout" {
        NET_ERR_TIMEOUT
    } else if err == "url too long" || err == "empty url" {
        NET_ERR_BAD_URL
    } else {
        NET_ERR_HTTP
    }
}

fn write_bytes_to_file(path: &str, bytes: &[u8]) -> i32 {
    let Ok(handle) = crate::r::io::kfs::write_file_begin(path, bytes.len() as u64) else {
        return FS_ERR_IO;
    };
    if crate::r::io::kfs::write_file_chunk(handle, bytes).is_err() {
        let _ = crate::r::io::kfs::write_file_abort(handle);
        return FS_ERR_IO;
    }
    if crate::r::io::kfs::write_file_finish(handle).is_err() {
        return FS_ERR_IO;
    }
    0
}

struct FetchTarget {
    scheme: &'static str,
    host: String,
    port: u16,
    path_and_query: String,
}

struct HttpsRequest<'a> {
    method: &'static str,
    content_type: Option<&'static str>,
    headers: Vec<(String, String)>,
    body: &'a [u8],
}

fn parse_fetch_url(url: &str) -> Result<FetchTarget, &'static str> {
    let trimmed = url.trim();
    let (scheme, rest, default_port) = if let Some(rest) = trimmed.strip_prefix("https://") {
        ("https", rest, 443)
    } else if let Some(rest) = trimmed.strip_prefix("http://") {
        ("http", rest, 80)
    } else {
        return Err("unsupported scheme");
    };

    let authority_end = rest
        .find(|c| c == '/' || c == '?' || c == '#')
        .unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty() {
        return Err("bad url");
    }

    let (host, port) = if let Some((host, port_text)) = authority.rsplit_once(':') {
        let port = port_text.parse::<u16>().map_err(|_| "bad port")?;
        (host, port)
    } else {
        (authority, default_port)
    };
    if host.is_empty() {
        return Err("bad host");
    }

    let mut path_and_query = if authority_end >= rest.len() {
        String::from("/")
    } else {
        let suffix = &rest[authority_end..];
        if suffix.starts_with('?') {
            format!("/{}", suffix)
        } else {
            String::from(suffix)
        }
    };
    if let Some(fragment) = path_and_query.find('#') {
        path_and_query.truncate(fragment);
    }
    if path_and_query.is_empty() {
        path_and_query.push('/');
    }

    Ok(FetchTarget {
        scheme,
        host: String::from(host),
        port,
        path_and_query,
    })
}

fn leak_str(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn find_http_header_end(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|idx| idx + 4)
}

fn parse_http_status(bytes: &[u8]) -> Option<u16> {
    let line_end = bytes.windows(2).position(|w| w == b"\r\n")?;
    let line = core::str::from_utf8(&bytes[..line_end]).ok()?;
    let mut parts = line.split_whitespace();
    let _http = parts.next()?;
    parts.next()?.parse::<u16>().ok()
}

fn trim_ascii(mut bytes: &[u8]) -> &[u8] {
    while matches!(bytes.first(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
        bytes = &bytes[1..];
    }
    while matches!(bytes.last(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

fn header_value<'a>(headers: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
    for line in headers.split(|b| *b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Some(colon) = line.iter().position(|b| *b == b':') else {
            continue;
        };
        let key = &line[..colon];
        if key.len() == name.len()
            && key
                .iter()
                .zip(name.iter())
                .all(|(a, b)| a.to_ascii_lowercase() == b.to_ascii_lowercase())
        {
            let mut value = &line[colon + 1..];
            while value.first() == Some(&b' ') || value.first() == Some(&b'\t') {
                value = &value[1..];
            }
            return Some(value);
        }
    }
    None
}

fn header_value_has_token(value: &[u8], token: &[u8]) -> bool {
    value.split(|b| *b == b',' || *b == b';').any(|part| {
        let part = trim_ascii(part);
        part.len() == token.len()
            && part
                .iter()
                .zip(token.iter())
                .all(|(a, b)| a.to_ascii_lowercase() == b.to_ascii_lowercase())
    })
}

fn decode_chunked(body: &[u8], max_bytes: usize) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut offset = 0usize;
    loop {
        let line_rel = body[offset..].windows(2).position(|w| w == b"\r\n")?;
        let line = &body[offset..offset + line_rel];
        let size_text = core::str::from_utf8(line.split(|b| *b == b';').next()?).ok()?;
        let size = usize::from_str_radix(size_text.trim(), 16).ok()?;
        offset = offset.checked_add(line_rel + 2)?;
        if size == 0 {
            return Some(out);
        }
        if offset.checked_add(size + 2)? > body.len() {
            return None;
        }
        if out.len().checked_add(size)? > max_bytes {
            return None;
        }
        out.extend_from_slice(&body[offset..offset + size]);
        offset += size + 2;
    }
}

fn bad_response_message(response: &[u8]) -> String {
    let preview_len = response.len().min(24);
    let mut preview = String::new();
    for (idx, byte) in response[..preview_len].iter().copied().enumerate() {
        if idx != 0 {
            preview.push(' ');
        }
        preview.push_str(format!("{:02X}", byte).as_str());
    }
    if response.len() > preview_len {
        preview.push_str(" ...");
    }
    if preview.is_empty() {
        preview.push_str("<empty>");
    }
    format!("bad response len={} first={}", response.len(), preview)
}

fn complete_http_body_from_response(
    response: &[u8],
    max_bytes: usize,
) -> Result<Option<Vec<u8>>, String> {
    let Some(hdr_end) = find_http_header_end(response) else {
        return Ok(None);
    };
    let status = parse_http_status(response).ok_or_else(|| String::from("bad status"))?;
    if !(200..300).contains(&status) {
        return Err(format!("http status {}", status));
    }

    let headers = &response[..hdr_end];
    let body = &response[hdr_end..];
    if let Some(te) = header_value(headers, b"transfer-encoding")
        && header_value_has_token(te, b"chunked")
    {
        return Ok(decode_chunked(body, max_bytes));
    }

    if let Some(len_text) = header_value(headers, b"content-length")
        && let Ok(len) = core::str::from_utf8(trim_ascii(len_text))
            .unwrap_or("")
            .parse::<usize>()
    {
        if len > max_bytes {
            return Err(format!("too large content_length={} max={}", len, max_bytes));
        }
        if body.len() < len {
            return Ok(None);
        }
        return Ok(Some(body[..len].to_vec()));
    }

    Ok(None)
}

fn http_body_from_response(response: &[u8], max_bytes: usize) -> Result<Vec<u8>, String> {
    if let Some(body) = complete_http_body_from_response(response, max_bytes)? {
        return Ok(body);
    }

    let hdr_end = find_http_header_end(response).ok_or_else(|| bad_response_message(response))?;

    let headers = &response[..hdr_end];
    let body = &response[hdr_end..];
    if let Some(te) = header_value(headers, b"transfer-encoding")
        && header_value_has_token(te, b"chunked")
    {
        return decode_chunked(body, max_bytes).ok_or_else(|| String::from("bad chunked body"));
    }

    if let Some(len_text) = header_value(headers, b"content-length")
        && let Ok(len) = core::str::from_utf8(trim_ascii(len_text))
            .unwrap_or("")
            .parse::<usize>()
    {
        if len > max_bytes {
            return Err(format!("too large content_length={} max={}", len, max_bytes));
        }
        return Ok(body.get(..len).unwrap_or(body).to_vec());
    }

    if body.len() > max_bytes {
        return Err(String::from("too large"));
    }
    Ok(body.to_vec())
}

async fn fetch_https_bytes(
    target: &FetchTarget,
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<Vec<u8>, String> {
    let request = HttpsRequest {
        method: "GET",
        content_type: None,
        headers: Vec::new(),
        body: &[],
    };
    request_https_bytes(target, &request, timeout_ms, max_bytes).await
}

async fn request_https_bytes(
    target: &FetchTarget,
    request: &HttpsRequest<'_>,
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<Vec<u8>, String> {
    crate::r::readiness::wait_for(
        crate::r::readiness::NET_ANY_CONFIGURED | crate::r::readiness::TLS_SOCKET_SERVICE_READY,
    )
    .await;

    let device_index = NetProfile::default()
        .resolve_device_index()
        .ok_or_else(|| String::from("no nic"))?;
    let ip = crate::r::net::dns::resolve_ipv4_for_device(
        device_index,
        target.host.as_str(),
        crate::r::net::dns::DnsConfig::default().with_timeout_ms(timeout_ms as u64),
    )
    .await
    .map_err(|err| format!("dns {:?}", err))?;

    let seq = HTTPS_FETCH_TLS_SEQ.fetch_add(1, Ordering::Relaxed);
    let owner = leak_str(format!("https-fetch-{}@{}", seq, device_index));
    let cmds = Queue::new_leaked(leak_str(format!("{}-cmd", owner)), 128);
    let events = Queue::new_leaked(leak_str(format!("{}-evt", owner)), 1024);
    register_tls_app_queues(owner, cmds, events);

    cmds.push(TlsCommand::OpenTcpConnect {
        remote: vnet::EndpointV4 {
            addr: ip,
            port: target.port,
        },
        server_name: leak_str(target.host.clone()),
        cfg: TlsClientConfig::new().with_alpn_protocols(&[b"http/1.1"]),
        roots: TlsRoots::mozilla(),
        timeouts: TlsTimeouts {
            connect_ms: 20_000,
            tls_ms: 30_000,
            idle_ms: timeout_ms,
        },
    })
    .map_err(|_| String::from("tls queue full"))?;

    let deadline = Instant::now() + EmbassyDuration::from_millis(timeout_ms as u64);
    let mut tls_handle = None;
    let mut sent_request = false;
    let mut response = Vec::new();

    loop {
        for ev in events.drain(64) {
            match ev {
                TlsEvent::Opened { handle } => tls_handle = Some(handle),
                TlsEvent::Connected { handle } => {
                    if tls_handle.is_none() {
                        tls_handle = Some(handle);
                    }
                    if tls_handle != Some(handle) || sent_request {
                        continue;
                    }
                    let mut req = format!(
                        "{} {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: TRUEOS net-fetch\r\nAccept: */*\r\nAccept-Encoding: identity\r\nConnection: close\r\n",
                        request.method, target.path_and_query, target.host
                    );
                    if let Some(content_type) = request.content_type {
                        req.push_str("Content-Type: ");
                        req.push_str(content_type);
                        req.push_str("\r\n");
                    }
                    if !request.body.is_empty() {
                        req.push_str("Content-Length: ");
                        req.push_str(format!("{}", request.body.len()).as_str());
                        req.push_str("\r\n");
                    }
                    for (name, value) in &request.headers {
                        if !name.is_empty() && !value.is_empty() {
                            req.push_str(name.as_str());
                            req.push_str(": ");
                            req.push_str(value.as_str());
                            req.push_str("\r\n");
                        }
                    }
                    req.push_str("\r\n");
                    let mut data = req.into_bytes();
                    data.extend_from_slice(request.body);
                    cmds.push(TlsCommand::Send { handle, data })
                        .map_err(|_| String::from("tls send queue full"))?;
                    sent_request = true;
                }
                TlsEvent::Data { handle, data } => {
                    if tls_handle != Some(handle) {
                        continue;
                    }
                    if response.len().saturating_add(data.len()) > max_bytes.saturating_add(4096) {
                        let _ = cmds.push(TlsCommand::Close { handle });
                        return Err(format!(
                            "too large received={} next={} max={}",
                            response.len(),
                            data.len(),
                            max_bytes
                        ));
                    }
                    response.extend_from_slice(data.as_slice());
                    if let Some(body) =
                        complete_http_body_from_response(response.as_slice(), max_bytes)?
                    {
                        let _ = cmds.push(TlsCommand::Close { handle });
                        return Ok(body);
                    }
                }
                TlsEvent::Closed { handle } => {
                    if tls_handle == Some(handle) {
                        return http_body_from_response(response.as_slice(), max_bytes);
                    }
                }
                TlsEvent::Error { msg } => return Err(String::from(msg)),
                TlsEvent::TlsError { err } => return Err(format!("tls {:?}", err)),
            }
        }

        if Instant::now() >= deadline {
            if let Some(handle) = tls_handle {
                let _ = cmds.push(TlsCommand::Close { handle });
            }
            return Err(String::from("timeout"));
        }
        Timer::after(EmbassyDuration::from_millis(10)).await;
    }
}

pub async fn get_bytes_shared(
    url: &str,
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<Vec<u8>, String> {
    let target = parse_fetch_url(url).map_err(String::from)?;
    match target.scheme {
        "http" => crate::surfer::html_shack::fetch_bytes_via_pool(
            String::from(url),
            timeout_ms as u64,
            max_bytes,
        )
        .await
        .map(|fetch| fetch.bytes),
        "https" => fetch_https_bytes(&target, timeout_ms.max(1), max_bytes).await,
        _ => Err(String::from("unsupported scheme")),
    }
}

pub async fn get_bytes_bearer_shared(
    url: &str,
    bearer: Option<&str>,
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<Vec<u8>, String> {
    let target = parse_fetch_url(url).map_err(String::from)?;
    if target.scheme != "https" {
        return Err(String::from("unsupported scheme"));
    }
    let mut headers = Vec::new();
    if let Some(token) = bearer {
        headers.push((String::from("Authorization"), format!("Bearer {}", token)));
    }
    let request = HttpsRequest {
        method: "GET",
        content_type: None,
        headers,
        body: &[],
    };
    request_https_bytes(&target, &request, timeout_ms.max(1), max_bytes).await
}

pub async fn get_range_bytes_shared(
    url: &str,
    offset: usize,
    length: usize,
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<Vec<u8>, String> {
    let target = parse_fetch_url(url).map_err(String::from)?;
    if target.scheme != "https" {
        return Err(String::from("unsupported scheme"));
    }
    if length == 0 {
        return Ok(Vec::new());
    }
    let end = offset
        .checked_add(length)
        .and_then(|value| value.checked_sub(1))
        .ok_or_else(|| String::from("range overflow"))?;
    let request = HttpsRequest {
        method: "GET",
        content_type: None,
        headers: vec![(String::from("Range"), format!("bytes={}-{}", offset, end))],
        body: &[],
    };
    request_https_bytes(&target, &request, timeout_ms.max(1), max_bytes).await
}

pub async fn put_protobuf_shared(
    url: &str,
    body: &[u8],
    bearer: Option<&str>,
    connection_id: Option<&str>,
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<Vec<u8>, String> {
    let target = parse_fetch_url(url).map_err(String::from)?;
    if target.scheme != "https" {
        return Err(String::from("unsupported scheme"));
    }
    let mut headers = Vec::new();
    if let Some(token) = bearer {
        headers.push((String::from("Authorization"), format!("Bearer {}", token)));
    }
    if let Some(connection_id) = connection_id {
        headers.push((String::from("X-Spotify-Connection-Id"), String::from(connection_id)));
    }
    let request = HttpsRequest {
        method: "PUT",
        content_type: Some("application/x-protobuf"),
        headers,
        body,
    };
    request_https_bytes(&target, &request, timeout_ms.max(1), max_bytes).await
}

pub async fn post_protobuf_shared(
    url: &str,
    body: &[u8],
    bearer: Option<&str>,
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<Vec<u8>, String> {
    let target = parse_fetch_url(url).map_err(String::from)?;
    if target.scheme != "https" {
        return Err(String::from("unsupported scheme"));
    }
    let mut headers = Vec::new();
    if let Some(token) = bearer {
        headers.push((String::from("Authorization"), format!("Bearer {}", token)));
    }
    let request = HttpsRequest {
        method: "POST",
        content_type: Some("application/x-protobuf"),
        headers,
        body,
    };
    request_https_bytes(&target, &request, timeout_ms.max(1), max_bytes).await
}

async fn fetch_bytes(url: String, timeout_ms: u32, max_bytes: usize) -> Result<Vec<u8>, i32> {
    get_bytes_shared(url.as_str(), timeout_ms, max_bytes)
        .await
        .map_err(|err| fetch_error_to_code(err.as_str()))
}

async fn post_json_bytes(
    url: String,
    body_json: String,
    bearer: Option<String>,
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<Vec<u8>, i32> {
    let auth_header = bearer.map(|token| format!("Bearer {}", token));
    let headers_with_auth = [
        ("Accept", "application/json"),
        ("Authorization", auth_header.as_deref().unwrap_or_default()),
    ];
    let headers_without_auth = [("Accept", "application/json")];
    let headers = if auth_header.is_some() {
        &headers_with_auth[..]
    } else {
        &headers_without_auth[..]
    };

    crate::surfer::html_shack::post_bytes_via_pool(
        url,
        "application/json",
        headers,
        body_json.as_bytes(),
        timeout_ms as u64,
        max_bytes,
    )
    .await
    .map(|fetch| fetch.bytes)
    .map_err(|err| fetch_error_to_code(err.as_str()))
}

fn spawn_fetch_file(op_id: u32, url: String, path: String, timeout_ms: u32, max_bytes: usize) {
    crate::wait::spawn_local_detached(async move {
        let rc = match fetch_bytes(url, timeout_ms, max_bytes).await {
            Ok(bytes) => write_bytes_to_file(path.as_str(), bytes.as_slice()),
            Err(rc) => rc,
        };
        if let Some(slot) = CABI_NET_FETCH_RESULTS.lock().get_mut(&op_id) {
            *slot = Some(rc);
        }
    });
}

fn spawn_fetch_bytes(op_id: u32, url: String, timeout_ms: u32, max_bytes: usize) {
    crate::wait::spawn_local_detached(async move {
        let (rc, body) = match fetch_bytes(url, timeout_ms, max_bytes).await {
            Ok(bytes) => (0, bytes),
            Err(rc) => (rc, Vec::new()),
        };
        if let Some(slot) = CABI_NET_FETCH_BYTES_RESULTS.lock().get_mut(&op_id) {
            slot.rc = Some(rc);
            slot.body = body;
        }
    });
}

fn spawn_post_json_file(
    op_id: u32,
    url: String,
    path: String,
    body_json: String,
    bearer: Option<String>,
    timeout_ms: u32,
    max_bytes: usize,
) {
    crate::wait::spawn_local_detached(async move {
        let rc = match post_json_bytes(url, body_json, bearer, timeout_ms, max_bytes).await {
            Ok(bytes) => write_bytes_to_file(path.as_str(), bytes.as_slice()),
            Err(rc) => rc,
        };
        if let Some(slot) = CABI_NET_FETCH_RESULTS.lock().get_mut(&op_id) {
            *slot = Some(rc);
        }
    });
}

fn spawn_post_json_bytes(
    op_id: u32,
    url: String,
    body_json: String,
    bearer: Option<String>,
    timeout_ms: u32,
    max_bytes: usize,
) {
    crate::wait::spawn_local_detached(async move {
        let (rc, body) = match post_json_bytes(url, body_json, bearer, timeout_ms, max_bytes).await
        {
            Ok(bytes) => (0, bytes),
            Err(rc) => (rc, Vec::new()),
        };
        if let Some(slot) = CABI_NET_FETCH_BYTES_RESULTS.lock().get_mut(&op_id) {
            slot.rc = Some(rc);
            slot.body = body;
        }
    });
}

pub(crate) fn cabi_net_fetch_start_host(
    url_s: &str,
    path_s: &str,
    timeout_ms: u32,
    max_bytes: usize,
) -> u32 {
    if url_s.trim().is_empty() || path_s.trim().is_empty() {
        return 0;
    }
    let op_id = CABI_NET_FETCH_SEQ.fetch_add(1, Ordering::Relaxed);
    CABI_NET_FETCH_RESULTS.lock().insert(op_id, None);
    spawn_fetch_file(
        op_id,
        String::from(url_s),
        String::from(path_s),
        timeout_ms.max(1),
        max_bytes,
    );
    op_id
}

pub(crate) fn cabi_net_fetch_result_host(op_id: u32) -> i32 {
    match CABI_NET_FETCH_RESULTS.lock().get(&op_id) {
        Some(Some(rc)) => *rc,
        Some(None) | None => FS_ERR_NOT_FOUND,
    }
}

pub(crate) fn cabi_net_fetch_discard_host(op_id: u32) -> i32 {
    CABI_NET_FETCH_RESULTS.lock().remove(&op_id);
    0
}

pub(crate) fn cabi_net_fetch_bytes_start_host(
    url_s: &str,
    timeout_ms: u32,
    max_bytes: usize,
) -> u32 {
    if url_s.trim().is_empty() {
        return 0;
    }
    let op_id = CABI_NET_FETCH_SEQ.fetch_add(1, Ordering::Relaxed);
    CABI_NET_FETCH_BYTES_RESULTS
        .lock()
        .insert(op_id, CabiNetFetchBytesResult::default());
    spawn_fetch_bytes(op_id, String::from(url_s), timeout_ms.max(1), max_bytes);
    op_id
}

pub(crate) fn cabi_net_fetch_bytes_result_len_host(op_id: u32) -> isize {
    match CABI_NET_FETCH_BYTES_RESULTS.lock().get(&op_id) {
        Some(entry) => match entry.rc {
            Some(0) => entry.body.len() as isize,
            Some(rc) => rc as isize,
            None => FS_ERR_NOT_FOUND as isize,
        },
        None => FS_ERR_NOT_FOUND as isize,
    }
}

pub(crate) fn cabi_net_fetch_bytes_read_chunk_host(
    op_id: u32,
    offset: usize,
    out: &mut [u8],
) -> isize {
    let mut map = CABI_NET_FETCH_BYTES_RESULTS.lock();
    let Some(entry) = map.get(&op_id) else {
        return FS_ERR_NOT_FOUND as isize;
    };
    let Some(rc) = entry.rc else {
        return FS_ERR_NOT_FOUND as isize;
    };
    if rc != 0 {
        map.remove(&op_id);
        return rc as isize;
    }
    if offset > entry.body.len() {
        return FS_ERR_BAD_PARAM as isize;
    }
    let n = core::cmp::min(out.len(), entry.body.len().saturating_sub(offset));
    if n != 0 {
        out[..n].copy_from_slice(&entry.body[offset..offset + n]);
    }
    if offset.saturating_add(n) >= entry.body.len() {
        map.remove(&op_id);
    }
    n as isize
}

pub(crate) fn cabi_net_fetch_bytes_discard_host(op_id: u32) -> i32 {
    CABI_NET_FETCH_BYTES_RESULTS.lock().remove(&op_id);
    0
}

unsafe fn abi_str<'a>(ptr: *const u8, len: usize) -> Option<&'a str> {
    if ptr.is_null() || len == 0 {
        return None;
    }
    core::str::from_utf8(unsafe { core::slice::from_raw_parts(ptr, len) }).ok()
}

unsafe fn optional_abi_string(ptr: *const u8, len: usize) -> Option<String> {
    if ptr.is_null() || len == 0 {
        None
    } else {
        unsafe { abi_str(ptr, len) }.map(String::from)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_net_fetch_start(
    url_ptr: *const u8,
    url_len: usize,
    path_ptr: *const u8,
    path_len: usize,
) -> u32 {
    let Some(url) = (unsafe { abi_str(url_ptr, url_len) }) else {
        return 0;
    };
    let Some(path) = (unsafe { abi_str(path_ptr, path_len) }) else {
        return 0;
    };
    cabi_net_fetch_start_host(url, path, 45_000, 8 * 1024 * 1024)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_net_fetch_bytes_start(
    url_ptr: *const u8,
    url_len: usize,
) -> u32 {
    let Some(url) = (unsafe { abi_str(url_ptr, url_len) }) else {
        return 0;
    };
    cabi_net_fetch_bytes_start_host(url, 45_000, 8 * 1024 * 1024)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_net_prewarm_url_start(
    url_ptr: *const u8,
    url_len: usize,
) -> i32 {
    if unsafe { abi_str(url_ptr, url_len) }.is_some() {
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_net_fetch_post_json_start(
    url_ptr: *const u8,
    url_len: usize,
    path_ptr: *const u8,
    path_len: usize,
    body_ptr: *const u8,
    body_len: usize,
    bearer_ptr: *const u8,
    bearer_len: usize,
) -> u32 {
    unsafe {
        trueos_cabi_net_fetch_post_json_start_with_timeout(
            url_ptr, url_len, path_ptr, path_len, body_ptr, body_len, bearer_ptr, bearer_len,
            15_000,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_net_fetch_post_json_start_with_timeout(
    url_ptr: *const u8,
    url_len: usize,
    path_ptr: *const u8,
    path_len: usize,
    body_ptr: *const u8,
    body_len: usize,
    bearer_ptr: *const u8,
    bearer_len: usize,
    timeout_ms: u32,
) -> u32 {
    let Some(url) = (unsafe { abi_str(url_ptr, url_len) }) else {
        return 0;
    };
    let Some(path) = (unsafe { abi_str(path_ptr, path_len) }) else {
        return 0;
    };
    let Some(body) = (unsafe { abi_str(body_ptr, body_len) }) else {
        return 0;
    };
    let bearer = unsafe { optional_abi_string(bearer_ptr, bearer_len) };
    let op_id = CABI_NET_FETCH_SEQ.fetch_add(1, Ordering::Relaxed);
    CABI_NET_FETCH_RESULTS.lock().insert(op_id, None);
    spawn_post_json_file(
        op_id,
        String::from(url),
        String::from(path),
        String::from(body),
        bearer,
        timeout_ms.max(1),
        4 * 1024 * 1024,
    );
    op_id
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_net_fetch_post_json_bytes_start(
    url_ptr: *const u8,
    url_len: usize,
    body_ptr: *const u8,
    body_len: usize,
    bearer_ptr: *const u8,
    bearer_len: usize,
) -> u32 {
    unsafe {
        trueos_cabi_net_fetch_post_json_bytes_start_with_timeout(
            url_ptr, url_len, body_ptr, body_len, bearer_ptr, bearer_len, 15_000,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_net_fetch_post_json_bytes_start_with_timeout(
    url_ptr: *const u8,
    url_len: usize,
    body_ptr: *const u8,
    body_len: usize,
    bearer_ptr: *const u8,
    bearer_len: usize,
    timeout_ms: u32,
) -> u32 {
    let Some(url) = (unsafe { abi_str(url_ptr, url_len) }) else {
        return 0;
    };
    let Some(body) = (unsafe { abi_str(body_ptr, body_len) }) else {
        return 0;
    };
    let bearer = unsafe { optional_abi_string(bearer_ptr, bearer_len) };
    let op_id = CABI_NET_FETCH_SEQ.fetch_add(1, Ordering::Relaxed);
    CABI_NET_FETCH_BYTES_RESULTS
        .lock()
        .insert(op_id, CabiNetFetchBytesResult::default());
    spawn_post_json_bytes(
        op_id,
        String::from(url),
        String::from(body),
        bearer,
        timeout_ms.max(1),
        4 * 1024 * 1024,
    );
    op_id
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_net_fetch_result(op_id: u32) -> i32 {
    cabi_net_fetch_result_host(op_id)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_net_fetch_discard(op_id: u32) -> i32 {
    cabi_net_fetch_discard_host(op_id)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_net_fetch_bytes_result_len(op_id: u32) -> isize {
    cabi_net_fetch_bytes_result_len_host(op_id)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_net_fetch_bytes_read(
    op_id: u32,
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    if out_ptr.is_null() || out_cap == 0 {
        return cabi_net_fetch_bytes_result_len_host(op_id);
    }
    let out = unsafe { core::slice::from_raw_parts_mut(out_ptr, out_cap) };
    cabi_net_fetch_bytes_read_chunk_host(op_id, 0, out)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_net_fetch_bytes_discard(op_id: u32) -> i32 {
    cabi_net_fetch_bytes_discard_host(op_id)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_net_fetch_bytes_wait(op_id: u32, timeout_ms: u64) -> i32 {
    if op_id == 0 {
        return FS_ERR_BAD_PARAM;
    }
    let start = monotonic_ms();
    loop {
        let rc = cabi_net_fetch_bytes_result_len_host(op_id);
        if rc != FS_ERR_NOT_FOUND as isize {
            return if rc < 0 { rc as i32 } else { 0 };
        }
        if timeout_ms == 0 || monotonic_ms().saturating_sub(start) >= timeout_ms {
            return FS_ERR_TIMEOUT;
        }
        crate::wait::spin_step();
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_net_fetch_wait(op_id: u32, timeout_ms: u64) -> i32 {
    if op_id == 0 {
        return FS_ERR_BAD_PARAM;
    }
    let start = monotonic_ms();
    loop {
        let rc = cabi_net_fetch_result_host(op_id);
        if rc != FS_ERR_NOT_FOUND {
            return rc;
        }
        if timeout_ms == 0 || monotonic_ms().saturating_sub(start) >= timeout_ms {
            return FS_ERR_TIMEOUT;
        }
        crate::wait::spin_step();
    }
}
