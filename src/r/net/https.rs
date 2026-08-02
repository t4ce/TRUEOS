extern crate alloc;

include!("../cabi_codes.rs");

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::{vec, vec::Vec};
use core::future::Future;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use core::task::{Context, Poll};

use atomic_waker::AtomicWaker;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use spin::Mutex;
use v::vnet;

use crate::net::tls::{TlsClientConfig, TlsRoots};
use crate::net::tls_socket::{
    TlsCancelCompletion, TlsCommand, TlsEvent, TlsTimeouts, register_tls_app_queues,
};
use crate::r::net::{NetProfile, Queue};

static CABI_NET_FETCH_SEQ: AtomicU32 = AtomicU32::new(1);
static HTTPS_FETCH_TLS_SEQ: AtomicU32 = AtomicU32::new(1);
const HTTPS_EVENT_DRAIN_MAX: usize = 512;
const HTTPS_COMMAND_QUEUE_DEPTH: usize = 128;
const HTTPS_EVENT_QUEUE_DEPTH: usize = 1024;
const HTTPS_RESPONSE_OVERHEAD_MAX: usize = 4096;
const HTTPS_RESOLVED_HOST_CACHE_MAX: usize = 8;
const HTTPS_IDLE_SLEEP_MS: u64 = 100;
const HTTPS_CLIENT_WAIT_MS: u64 = 10;
const HTTPS_CLIENT_FENCE_TIMEOUT_MS: u64 = 1_000;
const HTTPS_REQUEST_CANCELLED: &str = "cancelled";
static CABI_NET_FETCH_RESULTS: Mutex<BTreeMap<u32, Option<i32>>> = Mutex::new(BTreeMap::new());
static CABI_NET_FETCH_BYTES_RESULTS: Mutex<BTreeMap<u32, CabiNetFetchBytesResult>> =
    Mutex::new(BTreeMap::new());
static CABI_JSON_POST_CANCELLATIONS: Mutex<BTreeMap<u32, Arc<JsonPostCancellation>>> =
    Mutex::new(BTreeMap::new());

struct JsonPostCancellation {
    cancelled: AtomicBool,
    waker: AtomicWaker,
}

impl JsonPostCancellation {
    fn new() -> Self {
        Self {
            cancelled: AtomicBool::new(false),
            waker: AtomicWaker::new(),
        }
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.waker.wake();
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    fn poll_cancelled(&self, cx: &Context<'_>) -> bool {
        if self.is_cancelled() {
            return true;
        }
        self.waker.register(cx.waker());
        self.is_cancelled()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct JsonPostCancelled;

async fn await_json_post_or_cancel<F>(
    future: F,
    cancellation: &JsonPostCancellation,
) -> Result<F::Output, JsonPostCancelled>
where
    F: Future,
{
    let mut future = core::pin::pin!(future);
    core::future::poll_fn(|cx| {
        if cancellation.poll_cancelled(cx) {
            return Poll::Ready(Err(JsonPostCancelled));
        }
        if let Poll::Ready(output) = future.as_mut().poll(cx) {
            return Poll::Ready(Ok(output));
        }
        if cancellation.poll_cancelled(cx) {
            Poll::Ready(Err(JsonPostCancelled))
        } else {
            Poll::Pending
        }
    })
    .await
}

fn register_json_post_cancellation(op_id: u32) -> Arc<JsonPostCancellation> {
    let cancellation = Arc::new(JsonPostCancellation::new());
    CABI_JSON_POST_CANCELLATIONS
        .lock()
        .insert(op_id, cancellation.clone());
    cancellation
}

fn cancel_json_post_operation(op_id: u32) {
    let cancellation = CABI_JSON_POST_CANCELLATIONS.lock().remove(&op_id);
    if let Some(cancellation) = cancellation {
        cancellation.cancel();
    }
}

fn finish_json_post_operation(op_id: u32) {
    CABI_JSON_POST_CANCELLATIONS.lock().remove(&op_id);
}

#[derive(Default)]
struct CabiNetFetchBytesResult {
    rc: Option<i32>,
    body: Vec<u8>,
}

/// Decoded `OP_BP_FETCH_POST_JSON_BYTES_START` request.
///
/// The shared payload ordering is deliberately fixed as
/// `URL || bearer || JSON body`. The URL and bearer lengths are carried in the
/// low and high 32 bits of `arg1`, respectively, and the non-empty remainder
/// is the JSON body. Keeping the token between the two required fields makes
/// an absent bearer unambiguous without putting credentials in metadata or
/// logs.
pub(crate) struct CabiNetPostJsonBytesVmRequest<'a> {
    pub(crate) url: &'a str,
    pub(crate) bearer: Option<&'a str>,
    pub(crate) body: &'a str,
}

fn pack_post_json_bytes_vm_request(
    url: &str,
    body: &str,
    bearer: Option<&str>,
) -> Result<(Vec<u8>, u64), i32> {
    if url.is_empty() || body.is_empty() {
        return Err(FS_ERR_BAD_PARAM);
    }
    let bearer = bearer.unwrap_or("");
    let url_len = u32::try_from(url.len()).map_err(|_| FS_ERR_TOO_LARGE)?;
    let bearer_len = u32::try_from(bearer.len()).map_err(|_| FS_ERR_TOO_LARGE)?;
    let total = url
        .len()
        .checked_add(bearer.len())
        .and_then(|prefix| prefix.checked_add(body.len()))
        .ok_or(FS_ERR_TOO_LARGE)?;
    if total > trueos_vm::vmcall::PAYLOAD_CAP {
        return Err(FS_ERR_TOO_LARGE);
    }

    let mut payload = Vec::new();
    payload
        .try_reserve_exact(total)
        .map_err(|_| FS_ERR_NO_SPACE)?;
    payload.extend_from_slice(url.as_bytes());
    payload.extend_from_slice(bearer.as_bytes());
    payload.extend_from_slice(body.as_bytes());
    let packed_lengths = u64::from(url_len) | (u64::from(bearer_len) << 32);
    Ok((payload, packed_lengths))
}

pub(crate) fn decode_post_json_bytes_vm_request(
    payload: &[u8],
    packed_lengths: u64,
) -> Option<CabiNetPostJsonBytesVmRequest<'_>> {
    if payload.len() > trueos_vm::vmcall::PAYLOAD_CAP {
        return None;
    }
    let url_len = packed_lengths as u32 as usize;
    let bearer_len = (packed_lengths >> 32) as u32 as usize;
    let body_offset = url_len.checked_add(bearer_len)?;
    if url_len == 0 || body_offset >= payload.len() {
        return None;
    }

    let url = core::str::from_utf8(&payload[..url_len]).ok()?;
    let bearer_bytes = &payload[url_len..body_offset];
    let bearer = if bearer_bytes.is_empty() {
        None
    } else {
        Some(core::str::from_utf8(bearer_bytes).ok()?)
    };
    let body = core::str::from_utf8(&payload[body_offset..]).ok()?;
    Some(CabiNetPostJsonBytesVmRequest { url, bearer, body })
}

fn pack_fetch_bytes_vm_read(offset: usize, want: usize) -> Option<u64> {
    if want == 0 || want > trueos_vm::vmcall::PAYLOAD_CAP {
        return None;
    }
    let offset = u32::try_from(offset).ok()?;
    let want = u32::try_from(want).ok()?;
    Some((u64::from(offset) << 32) | u64::from(want))
}

pub(crate) fn decode_fetch_bytes_vm_read(packed: u64) -> Option<(usize, usize)> {
    let offset = (packed >> 32) as u32 as usize;
    let want = packed as u32 as usize;
    if want == 0 || want > trueos_vm::vmcall::PAYLOAD_CAP {
        None
    } else {
        Some((offset, want))
    }
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

async fn write_bytes_to_file(path: &str, bytes: &[u8]) -> i32 {
    // These C-ABI operations are implemented by BSP-local async tasks after
    // their network await. They therefore need native async TRUEOSFS I/O, not
    // the synchronous AP-lane compatibility bridge.
    let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
        return FS_ERR_IO;
    };
    match crate::r::fs::trueosfs::file_write_all_async(disk, path, bytes).await {
        Ok(true) => 0,
        Ok(false) | Err(_) => FS_ERR_IO,
    }
}

async fn write_bytes_to_file_cancellable(
    path: &str,
    bytes: &[u8],
    cancellation: &JsonPostCancellation,
) -> i32 {
    if cancellation.is_cancelled() {
        return fetch_error_to_code(HTTPS_REQUEST_CANCELLED);
    }
    let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
        return FS_ERR_IO;
    };
    let handle = match crate::r::fs::trueosfs::file_write_begin_async(
        disk,
        path,
        bytes.len() as u64,
    )
    .await
    {
        Ok(Some(handle)) => handle,
        Ok(None) | Err(_) => return FS_ERR_IO,
    };

    for chunk in bytes.chunks(64 * 1024) {
        if cancellation.is_cancelled() {
            let _ = crate::r::fs::trueosfs::file_write_abort_async(handle).await;
            return fetch_error_to_code(HTTPS_REQUEST_CANCELLED);
        }
        if crate::r::fs::trueosfs::file_write_chunk_async(handle, chunk)
            .await
            .is_err()
        {
            let _ = crate::r::fs::trueosfs::file_write_abort_async(handle).await;
            return FS_ERR_IO;
        }
    }
    if cancellation.is_cancelled() {
        let _ = crate::r::fs::trueosfs::file_write_abort_async(handle).await;
        return fetch_error_to_code(HTTPS_REQUEST_CANCELLED);
    }

    // Finishing publishes the stream atomically and is deliberately an
    // uncancellable commit section. A discard racing with this await can hide
    // the operation result, but cannot safely roll back a completed publish.
    let rc = match crate::r::fs::trueosfs::file_write_finish_async(handle).await {
        Ok(()) => 0,
        Err(_) => FS_ERR_IO,
    };
    if cancellation.is_cancelled() {
        fetch_error_to_code(HTTPS_REQUEST_CANCELLED)
    } else {
        rc
    }
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

/// A bounded HTTP response returned by [`HttpsJsonClient`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpsJsonResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

#[derive(Clone, Copy)]
struct HttpsClientQueues {
    device_index: usize,
    cmds: &'static Queue<TlsCommand>,
    events: &'static Queue<TlsEvent>,
}

struct HttpsResolvedHost {
    device_index: usize,
    host: String,
    ip: [u8; 4],
}

/// Reusable HTTPS/1.1 JSON client for a long-lived kernel-side caller.
///
/// Its TLS command/event queues are allocated and registered lazily on the
/// first request, then reused for every subsequent request. Keep one client
/// for the lifetime of the workload instead of constructing one per request.
pub struct HttpsJsonClient {
    queues: Option<HttpsClientQueues>,
    resolved_hosts: Vec<HttpsResolvedHost>,
    tls_config: TlsClientConfig,
    roots: TlsRoots,
}

impl Default for HttpsJsonClient {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpsJsonClient {
    pub fn new() -> Self {
        Self {
            queues: None,
            resolved_hosts: Vec::new(),
            tls_config: TlsClientConfig::new().with_alpn_protocols(&[b"http/1.1"]),
            roots: TlsRoots::mozilla(),
        }
    }

    fn queues_for_device(&mut self, requested_device_index: usize) -> HttpsClientQueues {
        if let Some(queues) = self.queues {
            return queues;
        }

        let seq = HTTPS_FETCH_TLS_SEQ.fetch_add(1, Ordering::Relaxed);
        let owner = leak_str(format!("https-json-client-{}@{}", seq, requested_device_index));
        let cmds = Queue::new_leaked(leak_str(format!("{}-cmd", owner)), HTTPS_COMMAND_QUEUE_DEPTH);
        let events = Queue::new_leaked(leak_str(format!("{}-evt", owner)), HTTPS_EVENT_QUEUE_DEPTH);
        register_tls_app_queues(owner, cmds, events);

        let queues = HttpsClientQueues {
            device_index: requested_device_index,
            cmds,
            events,
        };
        self.queues = Some(queues);
        queues
    }

    async fn resolve_host(
        &mut self,
        device_index: usize,
        host: &str,
        timeout_ms: u32,
    ) -> Result<[u8; 4], String> {
        if let Some(index) = self.resolved_hosts.iter().position(|cached| {
            cached.device_index == device_index && cached.host.eq_ignore_ascii_case(host)
        }) {
            let cached = self.resolved_hosts.remove(index);
            let ip = cached.ip;
            self.resolved_hosts.push(cached);
            return Ok(ip);
        }

        let ip = resolve_https_host(device_index, host, timeout_ms).await?;
        if self.resolved_hosts.len() >= HTTPS_RESOLVED_HOST_CACHE_MAX {
            self.resolved_hosts.remove(0);
        }
        self.resolved_hosts.push(HttpsResolvedHost {
            device_index,
            host: String::from(host),
            ip,
        });
        Ok(ip)
    }

    /// POST a JSON body and preserve both the HTTP status and bounded body.
    pub async fn post_json(
        &mut self,
        url: &str,
        body: &[u8],
        bearer: Option<&str>,
        timeout_ms: u32,
        max_bytes: usize,
    ) -> Result<HttpsJsonResponse, String> {
        self.post_json_inner(url, body, bearer, timeout_ms, max_bytes, None)
            .await
    }

    async fn post_json_cancellable(
        &mut self,
        url: &str,
        body: &[u8],
        bearer: Option<&str>,
        timeout_ms: u32,
        max_bytes: usize,
        cancellation: &JsonPostCancellation,
    ) -> Result<HttpsJsonResponse, String> {
        self.post_json_inner(url, body, bearer, timeout_ms, max_bytes, Some(cancellation))
            .await
    }

    async fn post_json_inner(
        &mut self,
        url: &str,
        body: &[u8],
        bearer: Option<&str>,
        timeout_ms: u32,
        max_bytes: usize,
        cancellation: Option<&JsonPostCancellation>,
    ) -> Result<HttpsJsonResponse, String> {
        if cancellation.is_some_and(JsonPostCancellation::is_cancelled) {
            return Err(String::from(HTTPS_REQUEST_CANCELLED));
        }
        let target = parse_fetch_url(url).map_err(String::from)?;
        if target.scheme != "https" {
            return Err(String::from("unsupported scheme"));
        }

        let mut headers = vec![(String::from("Accept"), String::from("application/json"))];
        if let Some(token) = bearer {
            if !valid_header_value(token) {
                return Err(String::from("bad bearer token"));
            }
            headers.push((String::from("Authorization"), format!("Bearer {}", token)));
        }
        let request = HttpsRequest {
            method: "POST",
            content_type: Some("application/json"),
            headers,
            body,
        };

        let readiness = crate::r::readiness::wait_for(
            crate::r::readiness::NET_ANY_CONFIGURED | crate::r::readiness::TLS_SOCKET_SERVICE_READY,
        );
        if let Some(cancellation) = cancellation {
            await_json_post_or_cancel(readiness, cancellation)
                .await
                .map_err(|_| String::from(HTTPS_REQUEST_CANCELLED))?;
        } else {
            readiness.await;
        }
        if cancellation.is_some_and(JsonPostCancellation::is_cancelled) {
            return Err(String::from(HTTPS_REQUEST_CANCELLED));
        }
        let requested_device_index = NetProfile::default()
            .resolve_device_index()
            .ok_or_else(|| String::from("no nic"))?;
        let queues = self.queues_for_device(requested_device_index);
        let resolve =
            self.resolve_host(queues.device_index, target.host.as_str(), timeout_ms.max(1));
        let ip = if let Some(cancellation) = cancellation {
            await_json_post_or_cancel(resolve, cancellation)
                .await
                .map_err(|_| String::from(HTTPS_REQUEST_CANCELLED))??
        } else {
            resolve.await?
        };
        if cancellation.is_some_and(JsonPostCancellation::is_cancelled) {
            return Err(String::from(HTTPS_REQUEST_CANCELLED));
        }
        let result = request_https_response(
            &target,
            ip,
            queues.cmds,
            queues.events,
            &request,
            &self.tls_config,
            &self.roots,
            timeout_ms.max(1),
            max_bytes,
            cancellation,
        )
        .await;
        // The queue pair and DNS cache are reusable, but the HTTP connection
        // is not. Fence the owner on every exit so an unscoped late TLS error
        // from this request cannot be mistaken for the next one. A failed
        // fence retires only the queue owner; cached DNS data remains useful.
        if !fence_https_owner(queues.cmds).await {
            self.queues = None;
        }
        result
    }

    /// Convenience JSON POST that requires a successful (2xx) status.
    pub async fn post_json_bearer(
        &mut self,
        url: &str,
        body: &[u8],
        bearer: &str,
        timeout_ms: u32,
        max_bytes: usize,
    ) -> Result<Vec<u8>, String> {
        success_body(
            self.post_json(url, body, Some(bearer), timeout_ms, max_bytes)
                .await?,
        )
    }
}

struct CabiHttpsJsonClientState {
    client: Option<HttpsJsonClient>,
    busy: bool,
}

static CABI_HTTPS_JSON_CLIENT: Mutex<CabiHttpsJsonClientState> =
    Mutex::new(CabiHttpsJsonClientState {
        client: None,
        busy: false,
    });

async fn take_cabi_https_json_client(
    cancellation: &JsonPostCancellation,
) -> Option<HttpsJsonClient> {
    loop {
        if cancellation.is_cancelled() {
            return None;
        }
        let acquired = {
            let mut state = CABI_HTTPS_JSON_CLIENT.lock();
            if state.busy {
                None
            } else {
                state.busy = true;
                Some(state.client.take())
            }
        };
        if let Some(client) = acquired {
            let client = client.unwrap_or_default();
            if cancellation.is_cancelled() {
                return_cabi_https_json_client(client);
                return None;
            }
            return Some(client);
        }
        if await_json_post_or_cancel(
            Timer::after(EmbassyDuration::from_millis(HTTPS_CLIENT_WAIT_MS)),
            cancellation,
        )
        .await
        .is_err()
        {
            return None;
        }
    }
}

fn return_cabi_https_json_client(client: HttpsJsonClient) {
    let mut state = CABI_HTTPS_JSON_CLIENT.lock();
    state.client = Some(client);
    state.busy = false;
}

fn request_has_header(request: &HttpsRequest<'_>, name: &str) -> bool {
    request
        .headers
        .iter()
        .any(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
}

fn valid_header_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn valid_header_value(value: &str) -> bool {
    !value
        .bytes()
        .any(|byte| byte == b'\r' || byte == b'\n' || byte == 0)
}

fn build_http_request(target: &FetchTarget, request: &HttpsRequest<'_>) -> Result<Vec<u8>, String> {
    if target.host.is_empty()
        || !valid_header_value(target.host.as_str())
        || target.path_and_query.is_empty()
        || !valid_header_value(target.path_and_query.as_str())
        || !valid_header_name(request.method)
    {
        return Err(String::from("bad http request"));
    }

    let default_port = match target.scheme {
        "https" => 443,
        "http" => 80,
        _ => target.port,
    };
    let host_header = if target.port == default_port {
        target.host.clone()
    } else {
        format!("{}:{}", target.host, target.port)
    };
    let mut req = format!(
        "{} {} HTTP/1.1\r\nHost: {}\r\n",
        request.method, target.path_and_query, host_header
    );
    if !request_has_header(request, "User-Agent") {
        req.push_str("User-Agent: TRUEOS net-fetch\r\n");
    }
    if !request_has_header(request, "Accept") {
        req.push_str("Accept: */*\r\n");
    }
    if !request_has_header(request, "Accept-Encoding") {
        req.push_str("Accept-Encoding: identity\r\n");
    }
    if !request_has_header(request, "Connection") {
        req.push_str("Connection: close\r\n");
    }
    if let Some(content_type) = request.content_type {
        if !valid_header_value(content_type) {
            return Err(String::from("bad content type"));
        }
        if !request_has_header(request, "Content-Type") {
            req.push_str("Content-Type: ");
            req.push_str(content_type);
            req.push_str("\r\n");
        }
    }
    if (!request.body.is_empty() || request.method != "GET")
        && !request_has_header(request, "Content-Length")
    {
        req.push_str("Content-Length: ");
        req.push_str(format!("{}", request.body.len()).as_str());
        req.push_str("\r\n");
    }
    for (name, value) in &request.headers {
        if !valid_header_name(name.as_str()) || !valid_header_value(value.as_str()) {
            return Err(String::from("bad http header"));
        }
        req.push_str(name.as_str());
        req.push_str(": ");
        req.push_str(value.as_str());
        req.push_str("\r\n");
    }
    req.push_str("\r\n");

    let mut data = req.into_bytes();
    data.extend_from_slice(request.body);
    Ok(data)
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
    if let Some(anchor) = path_and_query.find('#') {
        path_and_query.truncate(anchor);
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

fn complete_http_response(
    response: &[u8],
    max_bytes: usize,
) -> Result<Option<HttpsJsonResponse>, String> {
    let Some(hdr_end) = find_http_header_end(response) else {
        return Ok(None);
    };
    let status = parse_http_status(response).ok_or_else(|| String::from("bad status"))?;

    let headers = &response[..hdr_end];
    let body = &response[hdr_end..];
    if let Some(te) = header_value(headers, b"transfer-encoding")
        && header_value_has_token(te, b"chunked")
    {
        return Ok(decode_chunked(body, max_bytes).map(|body| HttpsJsonResponse { status, body }));
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
        return Ok(Some(HttpsJsonResponse {
            status,
            body: body[..len].to_vec(),
        }));
    }

    Ok(None)
}

fn http_response_from_bytes(
    response: &[u8],
    max_bytes: usize,
) -> Result<HttpsJsonResponse, String> {
    if let Some(response) = complete_http_response(response, max_bytes)? {
        return Ok(response);
    }

    let hdr_end = find_http_header_end(response).ok_or_else(|| bad_response_message(response))?;
    let status = parse_http_status(response).ok_or_else(|| String::from("bad status"))?;

    let headers = &response[..hdr_end];
    let body = &response[hdr_end..];
    if let Some(te) = header_value(headers, b"transfer-encoding")
        && header_value_has_token(te, b"chunked")
    {
        let body =
            decode_chunked(body, max_bytes).ok_or_else(|| String::from("bad chunked body"))?;
        return Ok(HttpsJsonResponse { status, body });
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
            return Err(format!("incomplete body received={} expected={}", body.len(), len));
        }
        return Ok(HttpsJsonResponse {
            status,
            body: body[..len].to_vec(),
        });
    }

    if body.len() > max_bytes {
        return Err(String::from("too large"));
    }
    Ok(HttpsJsonResponse {
        status,
        body: body.to_vec(),
    })
}

fn success_body(response: HttpsJsonResponse) -> Result<Vec<u8>, String> {
    if (200..300).contains(&response.status) {
        Ok(response.body)
    } else {
        Err(format!("http status {}", response.status))
    }
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
    let ip = resolve_https_host(device_index, target.host.as_str(), timeout_ms).await?;

    let seq = HTTPS_FETCH_TLS_SEQ.fetch_add(1, Ordering::Relaxed);
    let owner = leak_str(format!("https-fetch-{}@{}", seq, device_index));
    let cmds = Queue::new_leaked(leak_str(format!("{}-cmd", owner)), HTTPS_COMMAND_QUEUE_DEPTH);
    let events = Queue::new_leaked(leak_str(format!("{}-evt", owner)), HTTPS_EVENT_QUEUE_DEPTH);
    register_tls_app_queues(owner, cmds, events);
    let tls_config = TlsClientConfig::new().with_alpn_protocols(&[b"http/1.1"]);
    let roots = TlsRoots::mozilla();

    success_body(
        request_https_response(
            target,
            ip,
            cmds,
            events,
            request,
            &tls_config,
            &roots,
            timeout_ms,
            max_bytes,
            None,
        )
        .await?,
    )
}

async fn resolve_https_host(
    device_index: usize,
    host: &str,
    timeout_ms: u32,
) -> Result<[u8; 4], String> {
    crate::r::net::dns::resolve_ipv4_for_device(
        device_index,
        host,
        crate::r::net::dns::DnsConfig::for_device(device_index).with_timeout_ms(timeout_ms as u64),
    )
    .await
    .map_err(|err| format!("dns {:?}", err))
}

async fn fence_https_owner(cmds: &'static Queue<TlsCommand>) -> bool {
    embassy_time::with_timeout(EmbassyDuration::from_millis(HTTPS_CLIENT_FENCE_TIMEOUT_MS), async {
        let completion = Arc::new(TlsCancelCompletion::new());
        let mut command = TlsCommand::CancelOwner {
            completion: completion.clone(),
        };
        loop {
            match cmds.try_push(command) {
                Ok(()) => break,
                Err(returned) => {
                    command = returned;
                    Timer::after(EmbassyDuration::from_millis(HTTPS_CLIENT_WAIT_MS)).await;
                }
            }
        }
        completion.wait().await;
    })
    .await
    .is_ok()
}

async fn request_https_response(
    target: &FetchTarget,
    ip: [u8; 4],
    cmds: &'static Queue<TlsCommand>,
    events: &'static Queue<TlsEvent>,
    request: &HttpsRequest<'_>,
    tls_config: &TlsClientConfig,
    roots: &TlsRoots,
    timeout_ms: u32,
    max_bytes: usize,
    cancellation: Option<&JsonPostCancellation>,
) -> Result<HttpsJsonResponse, String> {
    let request_data = build_http_request(target, request)?;

    if cancellation.is_some_and(JsonPostCancellation::is_cancelled) {
        return Err(String::from(HTTPS_REQUEST_CANCELLED));
    }

    // A reusable client can have a final Closed event left over after it has
    // already consumed a complete length-delimited response. No connection is
    // active for this client here, so discard such stale events before opening.
    let _ = events.drain(HTTPS_EVENT_QUEUE_DEPTH);

    cmds.push(TlsCommand::OpenTcpConnect {
        remote: vnet::EndpointV4 {
            addr: ip,
            port: target.port,
        },
        server_name: target.host.clone(),
        cfg: tls_config.clone(),
        roots: roots.clone(),
        timeouts: TlsTimeouts {
            connect_ms: timeout_ms,
            tls_ms: timeout_ms,
            idle_ms: timeout_ms,
        },
    })
    .map_err(|_| String::from("tls queue full"))?;

    if cancellation.is_some_and(JsonPostCancellation::is_cancelled) {
        return Err(String::from(HTTPS_REQUEST_CANCELLED));
    }

    let deadline = Instant::now() + EmbassyDuration::from_millis(timeout_ms as u64);
    let mut tls_handle = None;
    let mut sent_request = false;
    let mut response = Vec::new();

    loop {
        if cancellation.is_some_and(JsonPostCancellation::is_cancelled) {
            return Err(String::from(HTTPS_REQUEST_CANCELLED));
        }
        let drained = events.drain(HTTPS_EVENT_DRAIN_MAX);
        let drained_any = !drained.is_empty();
        for ev in drained {
            if cancellation.is_some_and(JsonPostCancellation::is_cancelled) {
                return Err(String::from(HTTPS_REQUEST_CANCELLED));
            }
            match ev {
                TlsEvent::Opened { handle } => tls_handle = Some(handle),
                TlsEvent::Connected { handle } => {
                    if tls_handle.is_none() {
                        tls_handle = Some(handle);
                    }
                    if tls_handle != Some(handle) || sent_request {
                        continue;
                    }
                    if cancellation.is_some_and(JsonPostCancellation::is_cancelled) {
                        return Err(String::from(HTTPS_REQUEST_CANCELLED));
                    }
                    cmds.push(TlsCommand::Send {
                        handle,
                        data: request_data.clone(),
                    })
                    .map_err(|_| String::from("tls send queue full"))?;
                    sent_request = true;
                }
                TlsEvent::Data { handle, data } => {
                    if tls_handle != Some(handle) {
                        continue;
                    }
                    if response.len().saturating_add(data.len())
                        > max_bytes.saturating_add(HTTPS_RESPONSE_OVERHEAD_MAX)
                    {
                        let _ = cmds.push(TlsCommand::Close { handle });
                        return Err(format!(
                            "too large received={} next={} max={}",
                            response.len(),
                            data.len(),
                            max_bytes
                        ));
                    }
                    response.extend_from_slice(data.as_slice());
                    match complete_http_response(response.as_slice(), max_bytes) {
                        Ok(Some(response)) => {
                            let _ = cmds.push(TlsCommand::Close { handle });
                            return Ok(response);
                        }
                        Ok(None) => {}
                        Err(err) => {
                            let _ = cmds.push(TlsCommand::Close { handle });
                            return Err(err);
                        }
                    }
                }
                TlsEvent::Closed { handle } => {
                    if tls_handle == Some(handle) {
                        return http_response_from_bytes(response.as_slice(), max_bytes);
                    }
                }
                TlsEvent::Error { msg } => {
                    if let Some(handle) = tls_handle {
                        let _ = cmds.push(TlsCommand::Close { handle });
                    }
                    return Err(String::from(msg));
                }
                TlsEvent::TlsError { err } => {
                    if let Some(handle) = tls_handle {
                        let _ = cmds.push(TlsCommand::Close { handle });
                    }
                    return Err(format!("tls {:?}", err));
                }
            }
        }

        if Instant::now() >= deadline {
            if let Some(handle) = tls_handle {
                let _ = cmds.push(TlsCommand::Close { handle });
            }
            return Err(String::from("timeout"));
        }
        let sleep = if drained_any {
            Timer::after(EmbassyDuration::from_micros(0))
        } else {
            Timer::after(EmbassyDuration::from_millis(HTTPS_IDLE_SLEEP_MS))
        };
        if let Some(cancellation) = cancellation {
            if await_json_post_or_cancel(sleep, cancellation)
                .await
                .is_err()
            {
                return Err(String::from(HTTPS_REQUEST_CANCELLED));
            }
        } else {
            sleep.await;
        }
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

pub(crate) async fn get_media_bytes_profile_shared(
    url: &str,
    profile: &str,
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<Vec<u8>, String> {
    let target = parse_fetch_url(url).map_err(String::from)?;
    if target.scheme != "https" {
        return Err(String::from("unsupported scheme"));
    }
    let chrome_ua = String::from(
        "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36",
    );
    let mut headers = vec![
        (String::from("User-Agent"), chrome_ua),
        (String::from("Accept-Encoding"), String::from("identity")),
        (String::from("Connection"), String::from("close")),
    ];
    let use_range = !profile.contains("norange");
    match profile {
        "plain-range" | "plain-norange" => {
            headers.push((String::from("Accept"), String::from("*/*")));
        }
        _ => {
            headers.push((
                String::from("Accept"),
                String::from("video/webm,video/mp4,video/*;q=0.9,*/*;q=0.8"),
            ));
            headers.push((String::from("Accept-Language"), String::from("en-US,en;q=0.9")));
        }
    }
    if use_range {
        headers.push((String::from("Range"), String::from("bytes=0-")));
    }
    let request = HttpsRequest {
        method: "GET",
        content_type: None,
        headers,
        body: &[],
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
    cancellation: &JsonPostCancellation,
) -> Result<Vec<u8>, i32> {
    let target = parse_fetch_url(url.as_str()).map_err(fetch_error_to_code)?;
    if target.scheme == "http" {
        if cancellation.is_cancelled() {
            return Err(fetch_error_to_code(HTTPS_REQUEST_CANCELLED));
        }

        let authorization = if let Some(token) = bearer.as_deref() {
            if !valid_header_value(token) {
                return Err(fetch_error_to_code("bad bearer token"));
            }
            Some(format!("Bearer {}", token))
        } else {
            None
        };
        let mut headers = vec![("Accept", "application/json")];
        if let Some(value) = authorization.as_deref() {
            headers.push(("Authorization", value));
        }

        let post = crate::surfer::html_shack::post_bytes_via_pool(
            url,
            "application/json",
            headers.as_slice(),
            body_json.as_bytes(),
            timeout_ms as u64,
            max_bytes,
        );
        let fetch = await_json_post_or_cancel(post, cancellation)
            .await
            .map_err(|_| fetch_error_to_code(HTTPS_REQUEST_CANCELLED))?
            .map_err(|err| fetch_error_to_code(err.as_str()))?;
        if cancellation.is_cancelled() {
            return Err(fetch_error_to_code(HTTPS_REQUEST_CANCELLED));
        }
        return Ok(fetch.bytes);
    }

    // Blueprint callers use an operation id per request, but the kernel-side
    // HTTPS client is retained across operations. This keeps one registered
    // TLS queue pair and one bounded DNS cache for a long-running app loop.
    let Some(mut client) = take_cabi_https_json_client(cancellation).await else {
        return Err(fetch_error_to_code(HTTPS_REQUEST_CANCELLED));
    };
    let result = client
        .post_json_cancellable(
            url.as_str(),
            body_json.as_bytes(),
            bearer.as_deref(),
            timeout_ms,
            max_bytes,
            cancellation,
        )
        .await
        .and_then(success_body);
    return_cabi_https_json_client(client);
    result.map_err(|err| fetch_error_to_code(err.as_str()))
}

fn spawn_fetch_file(op_id: u32, url: String, path: String, timeout_ms: u32, max_bytes: usize) {
    crate::wait::spawn_local_detached(async move {
        let rc = match fetch_bytes(url, timeout_ms, max_bytes).await {
            Ok(bytes) => write_bytes_to_file(path.as_str(), bytes.as_slice()).await,
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
    cancellation: Arc<JsonPostCancellation>,
) {
    crate::wait::spawn_local_detached(async move {
        let rc = match post_json_bytes(
            url,
            body_json,
            bearer,
            timeout_ms,
            max_bytes,
            cancellation.as_ref(),
        )
        .await
        {
            Ok(bytes) if !cancellation.is_cancelled() => {
                write_bytes_to_file_cancellable(
                    path.as_str(),
                    bytes.as_slice(),
                    cancellation.as_ref(),
                )
                .await
            }
            Ok(_) => fetch_error_to_code(HTTPS_REQUEST_CANCELLED),
            Err(rc) => rc,
        };
        finish_json_post_operation(op_id);
        if !cancellation.is_cancelled()
            && let Some(slot) = CABI_NET_FETCH_RESULTS.lock().get_mut(&op_id)
        {
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
    cancellation: Arc<JsonPostCancellation>,
) {
    crate::wait::spawn_local_detached(async move {
        let (rc, body) = match post_json_bytes(
            url,
            body_json,
            bearer,
            timeout_ms,
            max_bytes,
            cancellation.as_ref(),
        )
        .await
        {
            Ok(bytes) => (0, bytes),
            Err(rc) => (rc, Vec::new()),
        };
        finish_json_post_operation(op_id);
        if !cancellation.is_cancelled()
            && let Some(slot) = CABI_NET_FETCH_BYTES_RESULTS.lock().get_mut(&op_id)
        {
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
    cancel_json_post_operation(op_id);
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

pub(crate) fn cabi_net_fetch_post_json_bytes_start_host(
    url_s: &str,
    body_s: &str,
    bearer: Option<&str>,
    timeout_ms: u32,
    max_bytes: usize,
) -> u32 {
    let op_id = CABI_NET_FETCH_SEQ.fetch_add(1, Ordering::Relaxed);
    CABI_NET_FETCH_BYTES_RESULTS
        .lock()
        .insert(op_id, CabiNetFetchBytesResult::default());
    let cancellation = register_json_post_cancellation(op_id);
    spawn_post_json_bytes(
        op_id,
        String::from(url_s),
        String::from(body_s),
        bearer.map(String::from),
        timeout_ms.max(1),
        max_bytes,
        cancellation,
    );
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
    cancel_json_post_operation(op_id);
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

fn guest_vmcall_op_id(status: u32, value: u64) -> u32 {
    if status != trueos_vm::vmcall::STATUS_OK || value == 0 || value > u64::from(u32::MAX) {
        0
    } else {
        value as u32
    }
}

fn cabi_net_fetch_bytes_start_guest(url: &str) -> u32 {
    if url.len() > trueos_vm::vmcall::PAYLOAD_CAP {
        return 0;
    }
    let (status, value) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_FETCH_BYTES_START,
        0,
        0,
        url.as_bytes(),
        &mut [],
    );
    guest_vmcall_op_id(status, value)
}

fn cabi_net_fetch_post_json_bytes_start_guest(
    url: &str,
    body: &str,
    bearer: Option<&str>,
    timeout_ms: u32,
) -> u32 {
    let Ok((payload, packed_lengths)) = pack_post_json_bytes_vm_request(url, body, bearer) else {
        return 0;
    };
    let (status, value) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_FETCH_POST_JSON_BYTES_START,
        u64::from(timeout_ms),
        packed_lengths,
        payload.as_slice(),
        &mut [],
    );
    guest_vmcall_op_id(status, value)
}

fn cabi_net_fetch_bytes_result_len_guest(op_id: u32) -> isize {
    let (status, value) = trueos_vm::vmcall::call(
        trueos_vm::vmcall::OP_BP_FETCH_BYTES_RESULT_LEN,
        u64::from(op_id),
        0,
    );
    if status == trueos_vm::vmcall::STATUS_OK {
        (value as i64) as isize
    } else {
        FS_ERR_BAD_PARAM as isize
    }
}

fn cabi_net_fetch_bytes_read_guest(op_id: u32, out: &mut [u8]) -> isize {
    let mut copied = 0usize;
    while copied < out.len() {
        let want = core::cmp::min(
            out.len().saturating_sub(copied),
            trueos_vm::vmcall::PAYLOAD_CAP,
        );
        let Some(packed_read) = pack_fetch_bytes_vm_read(copied, want) else {
            return FS_ERR_TOO_LARGE as isize;
        };
        let (status, value) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_FETCH_BYTES_READ,
            u64::from(op_id),
            packed_read,
            &[],
            &mut out[copied..copied + want],
        );
        if status != trueos_vm::vmcall::STATUS_OK {
            return FS_ERR_BAD_PARAM as isize;
        }
        let rc = (value as i64) as isize;
        if rc < 0 {
            return rc;
        }
        let count = rc as usize;
        if count > want {
            return FS_ERR_BAD_PARAM as isize;
        }
        copied = match copied.checked_add(count) {
            Some(copied) => copied,
            None => return FS_ERR_TOO_LARGE as isize,
        };
        if count < want {
            break;
        }
    }
    isize::try_from(copied).unwrap_or(FS_ERR_TOO_LARGE as isize)
}

fn cabi_net_fetch_bytes_discard_guest(op_id: u32) -> i32 {
    let (status, value) = trueos_vm::vmcall::call(
        trueos_vm::vmcall::OP_BP_FETCH_BYTES_DISCARD,
        u64::from(op_id),
        0,
    );
    if status == trueos_vm::vmcall::STATUS_OK {
        (value as i64) as i32
    } else {
        FS_ERR_BAD_PARAM
    }
}

fn guest_monotonic_ms() -> Option<u64> {
    let (status, nanos) = trueos_vm::vmcall::call(trueos_vm::vmcall::OP_MONOTONIC_NANOS, 0, 0);
    (status == trueos_vm::vmcall::STATUS_OK).then_some(nanos / 1_000_000)
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
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        cabi_net_fetch_bytes_start_guest(url)
    } else {
        cabi_net_fetch_bytes_start_host(url, 45_000, 8 * 1024 * 1024)
    }
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
    let cancellation = register_json_post_cancellation(op_id);
    spawn_post_json_file(
        op_id,
        String::from(url),
        String::from(path),
        String::from(body),
        bearer,
        timeout_ms.max(1),
        4 * 1024 * 1024,
        cancellation,
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
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        cabi_net_fetch_post_json_bytes_start_guest(
            url,
            body,
            bearer.as_deref(),
            timeout_ms,
        )
    } else {
        cabi_net_fetch_post_json_bytes_start_host(
            url,
            body,
            bearer.as_deref(),
            timeout_ms,
            4 * 1024 * 1024,
        )
    }
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
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        cabi_net_fetch_bytes_result_len_guest(op_id)
    } else {
        cabi_net_fetch_bytes_result_len_host(op_id)
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_net_fetch_bytes_read(
    op_id: u32,
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    if out_ptr.is_null() || out_cap == 0 {
        return trueos_cabi_net_fetch_bytes_result_len(op_id);
    }
    let out = unsafe { core::slice::from_raw_parts_mut(out_ptr, out_cap) };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        cabi_net_fetch_bytes_read_guest(op_id, out)
    } else {
        cabi_net_fetch_bytes_read_chunk_host(op_id, 0, out)
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_net_fetch_bytes_discard(op_id: u32) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        cabi_net_fetch_bytes_discard_guest(op_id)
    } else {
        cabi_net_fetch_bytes_discard_host(op_id)
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_net_fetch_bytes_wait(op_id: u32, timeout_ms: u64) -> i32 {
    if op_id == 0 {
        return FS_ERR_BAD_PARAM;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let Some(start) = guest_monotonic_ms() else {
            return FS_ERR_BAD_PARAM;
        };
        loop {
            let rc = cabi_net_fetch_bytes_result_len_guest(op_id);
            if rc != FS_ERR_NOT_FOUND as isize {
                return if rc < 0 { rc as i32 } else { 0 };
            }
            let Some(now) = guest_monotonic_ms() else {
                return FS_ERR_BAD_PARAM;
            };
            if timeout_ms == 0 || now.saturating_sub(start) >= timeout_ms {
                return FS_ERR_TIMEOUT;
            }
            // Yield the Hull lane to the host. `spin_step()` polls a host
            // per-CPU executor and is never valid from this guest context.
            trueos_vm::vmcall::sleep_ms(1);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_request_contains_auth_length_and_exact_body() {
        let target = parse_fetch_url("https://api.taalas.com:8443/v1/chat/completions").unwrap();
        let body = br#"{"model":"chatjimmy"}"#;
        let request = HttpsRequest {
            method: "POST",
            content_type: Some("application/json"),
            headers: vec![
                (String::from("Accept"), String::from("application/json")),
                (String::from("Authorization"), String::from("Bearer test-token")),
            ],
            body,
        };

        let encoded = build_http_request(&target, &request).unwrap();
        let header_end = find_http_header_end(encoded.as_slice()).unwrap();
        let headers = core::str::from_utf8(&encoded[..header_end]).unwrap();
        assert!(headers.starts_with("POST /v1/chat/completions HTTP/1.1\r\n"));
        assert!(headers.contains("Host: api.taalas.com:8443\r\n"));
        assert!(headers.contains("Authorization: Bearer test-token\r\n"));
        assert!(headers.contains("Content-Type: application/json\r\n"));
        assert!(headers.contains(format!("Content-Length: {}\r\n", body.len()).as_str()));
        assert_eq!(&encoded[header_end..], body);
    }

    #[test]
    fn response_preserves_non_success_status_and_body() {
        let bytes = b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 6\r\n\r\ndenied";
        let response = complete_http_response(bytes, 64).unwrap().unwrap();
        assert_eq!(response.status, 401);
        assert_eq!(response.body.as_slice(), b"denied");
        assert_eq!(success_body(response).unwrap_err(), "http status 401");
    }

    #[test]
    fn response_body_limit_is_enforced_from_content_length() {
        let bytes = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello";
        let err = complete_http_response(bytes, 4).unwrap_err();
        assert!(err.contains("content_length=5"));
    }

    #[test]
    fn closed_truncated_response_is_rejected() {
        let bytes = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhey";
        let err = http_response_from_bytes(bytes, 5).unwrap_err();
        assert!(err.contains("received=3 expected=5"));
    }

    #[test]
    fn chunked_response_is_decoded() {
        let bytes = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n";
        let response = complete_http_response(bytes, 5).unwrap().unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body.as_slice(), b"hello");
    }

    #[test]
    fn header_line_injection_is_rejected() {
        assert!(!valid_header_value("token\r\nInjected: yes"));
    }

    #[test]
    fn bytes_discard_signals_registered_json_post_cancellation() {
        const OP_ID: u32 = u32::MAX - 16;

        finish_json_post_operation(OP_ID);
        CABI_NET_FETCH_BYTES_RESULTS.lock().remove(&OP_ID);
        CABI_NET_FETCH_BYTES_RESULTS
            .lock()
            .insert(OP_ID, CabiNetFetchBytesResult::default());
        let cancellation = register_json_post_cancellation(OP_ID);

        assert_eq!(cabi_net_fetch_bytes_discard_host(OP_ID), 0);
        assert!(cancellation.is_cancelled());
        assert!(!CABI_NET_FETCH_BYTES_RESULTS.lock().contains_key(&OP_ID));
        assert!(!CABI_JSON_POST_CANCELLATIONS.lock().contains_key(&OP_ID));
    }

    #[test]
    fn ordinary_fetch_discard_does_not_cancel_another_json_post() {
        const JSON_OP_ID: u32 = u32::MAX - 17;
        const GET_OP_ID: u32 = u32::MAX - 18;

        finish_json_post_operation(JSON_OP_ID);
        CABI_NET_FETCH_RESULTS.lock().remove(&GET_OP_ID);
        let cancellation = register_json_post_cancellation(JSON_OP_ID);
        CABI_NET_FETCH_RESULTS.lock().insert(GET_OP_ID, None);

        assert_eq!(cabi_net_fetch_discard_host(GET_OP_ID), 0);
        assert!(!cancellation.is_cancelled());
        assert!(
            CABI_JSON_POST_CANCELLATIONS
                .lock()
                .contains_key(&JSON_OP_ID)
        );

        finish_json_post_operation(JSON_OP_ID);
    }
}
