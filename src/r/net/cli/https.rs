extern crate alloc;

use super::dns::{self, DnsConfig};
use super::http::{self, HttpFetchError};
use super::https_limits::HttpsLimits;
use crate::net::tls::{TlsClientConfig, TlsRoots};
use crate::net::tls_socket::{TlsCommand, TlsEvent, register_tls_app_queues};
use crate::r::io::cabi::{
    FS_ERR_BAD_PARAM, FS_ERR_BAD_PATH, FS_ERR_IO, FS_ERR_NO_SPACE, FS_ERR_NOT_FOUND,
    FS_ERR_TIMEOUT, FS_ERR_TOO_LARGE, FS_ERR_USBMS_NOT_FOUND, NET_ERR_BAD_URL, NET_ERR_HTTP,
    NET_ERR_TIMEOUT, NET_ERR_TIMEOUT_BODY, NET_ERR_TIMEOUT_CONNECT, NET_ERR_TIMEOUT_DNS,
    NET_ERR_TIMEOUT_TLS, NET_ERR_TLS,
};
use crate::r::net::{NetProfile, Queue};
use crate::wait::WaitQueue;
use alloc::{boxed::Box, collections::BTreeMap, format, string::String, vec::Vec};
use core::{
    fmt::Write as _,
    sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicUsize, Ordering},
};
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use spin::Mutex;
use v::vnet;

static CABI_NET_FETCH_SEQ: AtomicU32 = AtomicU32::new(1);
static CABI_NET_FETCH_RESULTS: Mutex<BTreeMap<u32, Option<i32>>> = Mutex::new(BTreeMap::new());
struct CabiNetFetchBytesResult {
    rc: Option<i32>,
    body: Vec<u8>,
}

impl Default for CabiNetFetchBytesResult {
    fn default() -> Self {
        Self {
            rc: None,
            body: Vec::new(),
        }
    }
}

static CABI_NET_FETCH_BYTES_RESULTS: Mutex<BTreeMap<u32, CabiNetFetchBytesResult>> =
    Mutex::new(BTreeMap::new());
static CABI_NET_FETCH_WAIT: WaitQueue = WaitQueue::new();
static CABI_NET_FETCH_WAIT_MODE_LOGGED: AtomicU8 = AtomicU8::new(0);

#[inline]
fn wait_on_net_fetch_queue_blocking(timeout_ms: u64) -> bool {
    let ready = crate::r::readiness::is_set(
        crate::r::readiness::NET_CONFIGURED | crate::r::readiness::TLS_SOCKET_SERVICE_READY,
    );
    if ready {
        if CABI_NET_FETCH_WAIT_MODE_LOGGED
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            crate::log!("net-fetch-wait: mode=parked\n");
        }
        return CABI_NET_FETCH_WAIT.wait_for_event_blocking_parked(timeout_ms);
    }

    // Early boot and degraded bring-up still fall back to the conservative polling wait.
    if CABI_NET_FETCH_WAIT_MODE_LOGGED
        .compare_exchange(0, 2, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        crate::log!("net-fetch-wait: mode=spin\n");
    }
    CABI_NET_FETCH_WAIT.wait_for_event_blocking(timeout_ms)
}

// --- keep-alive pool (per host) ---

static VHTTPS_KEEPALIVE_SEQ: AtomicU32 = AtomicU32::new(1);

struct KeepAliveConn {
    cmds: &'static Queue<TlsCommand>,
    events: &'static Queue<TlsEvent>,
    in_use: AtomicBool,
    state: Mutex<KeepAliveState>,
}

#[derive(Clone, Copy)]
struct KeepAliveState {
    handle: Option<vnet::NetHandle>,
    connected: bool,
    last_used: Instant,
    connect_fail_streak: u8,
    connect_backoff_until: Instant,
    connect_hard_stop_until: Instant,
}

impl Default for KeepAliveState {
    fn default() -> Self {
        Self {
            handle: None,
            connected: false,
            last_used: Instant::from_ticks(0),
            connect_fail_streak: 0,
            connect_backoff_until: Instant::from_ticks(0),
            connect_hard_stop_until: Instant::from_ticks(0),
        }
    }
}

static VHTTPS_KEEPALIVE_POOL: Mutex<BTreeMap<String, &'static KeepAliveConn>> =
    Mutex::new(BTreeMap::new());

fn keepalive_pool_key(dev_idx: usize, host: &str, port: u16) -> String {
    // host is already a DNS name in our URL parser; no IPv6 here.
    alloc::format!("{}|{}|{}", dev_idx, host, port)
}

fn ensure_keepalive_conn(dev_idx: usize, host: &str, port: u16) -> &'static KeepAliveConn {
    let key = keepalive_pool_key(dev_idx, host, port);
    let mut pool = VHTTPS_KEEPALIVE_POOL.lock();
    if let Some(c) = pool.get(&key) {
        return c;
    }

    let seq = VHTTPS_KEEPALIVE_SEQ.fetch_add(1, Ordering::Relaxed);
    let selector = if let Some((bus, slot, func)) = crate::net::bdf_at(dev_idx) {
        alloc::format!("{:02x}:{:02x}.{}", bus, slot, func)
    } else if let Some((vid, pid)) = crate::net::pci_id_at(dev_idx) {
        alloc::format!("{:04x}:{:04x}", vid, pid)
    } else {
        alloc::format!("{}", dev_idx)
    };

    // Owner suffix pins tls-socket's VNet selection to the chosen NIC.
    let owner = leak_str(alloc::format!("vhttps-ka-{}@{}", seq, selector));
    let cmds_name = leak_str(alloc::format!("{}-tls-cmd", owner));
    let evts_name = leak_str(alloc::format!("{}-tls-evt", owner));
    let cmds = Queue::new_leaked(cmds_name, 256);
    let events = Queue::new_leaked(evts_name, 4096);
    register_tls_app_queues(owner, cmds, events);

    let conn = KeepAliveConn {
        cmds,
        events,
        in_use: AtomicBool::new(false),
        state: Mutex::new(KeepAliveState::default()),
    };
    let leaked: &'static KeepAliveConn = Box::leak(Box::new(conn));
    pool.insert(key, leaked);
    leaked
}

async fn keepalive_acquire(conn: &'static KeepAliveConn) {
    loop {
        if conn
            .in_use
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            return;
        }
        Timer::after(EmbassyDuration::from_millis(1)).await;
    }
}

fn keepalive_release(conn: &'static KeepAliveConn) {
    conn.in_use.store(false, Ordering::Release);
}

fn keepalive_sync_state(conn: &'static KeepAliveConn) {
    for ev in conn.events.drain(4096) {
        match ev {
            TlsEvent::Opened { handle } => {
                let mut st = conn.state.lock();
                if st.handle.is_none() {
                    st.handle = Some(handle);
                }
            }
            TlsEvent::Connected { handle } => {
                let mut st = conn.state.lock();
                if st.handle == Some(handle) {
                    st.connected = true;
                }
            }
            TlsEvent::Closed { handle } => {
                let mut st = conn.state.lock();
                if st.handle == Some(handle) {
                    st.handle = None;
                    st.connected = false;
                }
            }
            TlsEvent::TlsError { .. } => {
                let mut st = conn.state.lock();
                st.handle = None;
                st.connected = false;
            }
            _ => {}
        }
    }
}

fn keepalive_connect_wait_ms(conn: &'static KeepAliveConn) -> u64 {
    let st = conn.state.lock();
    let now = Instant::now();
    let backoff_ms = if now >= st.connect_backoff_until {
        0
    } else {
        st.connect_backoff_until
            .saturating_duration_since(now)
            .as_millis() as u64
    };
    let hard_stop_ms = if now >= st.connect_hard_stop_until {
        0
    } else {
        st.connect_hard_stop_until
            .saturating_duration_since(now)
            .as_millis() as u64
    };
    backoff_ms.max(hard_stop_ms)
}

fn keepalive_record_connect_success(conn: &'static KeepAliveConn) {
    let mut st = conn.state.lock();
    st.connect_fail_streak = 0;
    st.connect_backoff_until = Instant::from_ticks(0);
    st.connect_hard_stop_until = Instant::from_ticks(0);
}

fn keepalive_record_connect_failure(conn: &'static KeepAliveConn, host: &str, dev_idx: usize) {
    let mut st = conn.state.lock();
    st.connect_fail_streak = st.connect_fail_streak.saturating_add(1);
    let Some(delay_ms) = HttpsLimits::connect_backoff_ms(st.connect_fail_streak) else {
        return;
    };
    st.connect_backoff_until = Instant::now() + EmbassyDuration::from_millis(delay_ms);
    if let Some(hard_stop_ms) = HttpsLimits::connect_hard_stop_ms(st.connect_fail_streak) {
        st.connect_hard_stop_until = Instant::now() + EmbassyDuration::from_millis(hard_stop_ms);
        crate::log!(
            "vhttps-ka: hard-stop host={} dev={} streak={} wait_ms={}\n",
            host,
            dev_idx,
            st.connect_fail_streak,
            hard_stop_ms
        );
    }
    crate::log!(
        "vhttps-ka: backoff host={} dev={} streak={} wait_ms={}\n",
        host,
        dev_idx,
        st.connect_fail_streak,
        delay_ms
    );
}

struct KeepAliveReady {
    conn: &'static KeepAliveConn,
    handle: vnet::NetHandle,
    reused: bool,
    deadline: Instant,
    last_activity: Instant,
}

async fn keepalive_prepare_ready(
    parsed: &ParsedHttpsUrl,
    dev_idx: usize,
    timeout_ms: u32,
    log_prefix: &str,
) -> Result<KeepAliveReady, FetchError> {
    let conn = ensure_keepalive_conn(dev_idx, parsed.host.as_str(), parsed.port);
    keepalive_acquire(conn).await;

    // Drain pending events so each request starts from a clean boundary while
    // still preserving pooled socket state transitions.
    keepalive_sync_state(conn);

    let mut reused = false;
    {
        let mut st = conn.state.lock();
        let idle_ms = Instant::now()
            .saturating_duration_since(st.last_used)
            .as_millis() as u64;
        if st.handle.is_some() && idle_ms > HttpsLimits::KEEPALIVE_IDLE_CLOSE_MS {
            if let Some(h) = st.handle.take() {
                let _ = conn.cmds.push(TlsCommand::Close { handle: h });
            }
            st.connected = false;
        } else if st.handle.is_some() && st.connected {
            reused = true;
        }
    }

    let mut ip: Option<[u8; 4]> = None;
    if !reused {
        match dns::resolve_ipv4_for_device(
            dev_idx,
            parsed.host.as_str(),
            DnsConfig::for_device(dev_idx),
        )
        .await
        {
            Ok(v) => ip = Some(v),
            Err(dns::DnsError::Timeout) => {
                keepalive_release(conn);
                return Err(FetchError::DnsTimeout);
            }
            Err(_) => {
                keepalive_release(conn);
                return Err(FetchError::DnsFailed);
            }
        }
    }

    let roots = TlsRoots::mozilla();
    let cfg = TlsClientConfig::new().with_alpn_protocols(&[b"http/1.1"]);
    let server_name = leak_str(parsed.host.clone());
    let deadline = Instant::now() + EmbassyDuration::from_millis(timeout_ms as u64);
    let mut connect_in_flight = false;
    let mut last_activity = Instant::now();
    let mut ready_handle: Option<vnet::NetHandle> = None;

    'connect_wait: loop {
        let (handle, connected) = {
            let st = conn.state.lock();
            (st.handle, st.connected)
        };

        if handle.is_some() && connected {
            crate::log!(
                "{}: connected host={} dev={} handle={}\n",
                log_prefix,
                parsed.host,
                dev_idx,
                handle.expect("checked is_some").0
            );
            ready_handle = handle;
            break;
        }

        if handle.is_none() && !connect_in_flight {
            let wait_ms = keepalive_connect_wait_ms(conn);
            if wait_ms != 0 {
                Timer::after(EmbassyDuration::from_millis(wait_ms.min(250))).await;
                continue;
            }
            let Some(ip) = ip else {
                keepalive_release(conn);
                return Err(FetchError::ConnectTimeout);
            };
            crate::log!("{}: connect host={} dev={} fresh=1\n", log_prefix, parsed.host, dev_idx);
            let _ = conn.cmds.push(TlsCommand::OpenTcpConnect {
                remote: vnet::EndpointV4 {
                    addr: ip,
                    port: parsed.port,
                },
                server_name,
                cfg: cfg.clone(),
                roots: roots.clone(),
                timeouts: crate::net::tls_socket::TlsTimeouts {
                    connect_ms: (timeout_ms / 4).max(5_000),
                    tls_ms: (timeout_ms / 4).max(5_000),
                    idle_ms: timeout_ms,
                },
            });
            connect_in_flight = true;
        }

        for ev in conn.events.drain(1024) {
            match ev {
                TlsEvent::Opened { handle } => {
                    let mut st = conn.state.lock();
                    if st.handle.is_none() {
                        st.handle = Some(handle);
                        crate::log!(
                            "{}: opened host={} dev={} handle={}\n",
                            log_prefix,
                            parsed.host,
                            dev_idx,
                            handle.0
                        );
                    }
                    last_activity = Instant::now();
                }
                TlsEvent::Connected { handle } => {
                    let matched = {
                        let mut st = conn.state.lock();
                        if st.handle == Some(handle) {
                            st.connected = true;
                            crate::log!(
                                "{}: tls-connected host={} dev={} handle={}\n",
                                log_prefix,
                                parsed.host,
                                dev_idx,
                                handle.0
                            );
                            ready_handle = Some(handle);
                            true
                        } else {
                            false
                        }
                    };
                    if matched {
                        keepalive_record_connect_success(conn);
                        last_activity = Instant::now();
                        break 'connect_wait;
                    }
                }
                TlsEvent::Closed { handle } => {
                    let mut st = conn.state.lock();
                    if st.handle == Some(handle) {
                        let was_connected = st.connected;
                        crate::log!(
                            "{}: closed-connect-phase host={} dev={} handle={} was_connected={}\n",
                            log_prefix,
                            parsed.host,
                            dev_idx,
                            handle.0,
                            if was_connected { 1 } else { 0 }
                        );
                        st.handle = None;
                        st.connected = false;
                        connect_in_flight = false;
                        if !was_connected {
                            drop(st);
                            keepalive_record_connect_failure(conn, parsed.host.as_str(), dev_idx);
                        }
                    }
                }
                TlsEvent::TlsError { .. } => {
                    let mut st = conn.state.lock();
                    st.handle = None;
                    st.connected = false;
                    connect_in_flight = false;
                    drop(st);
                    keepalive_record_connect_failure(conn, parsed.host.as_str(), dev_idx);
                }
                _ => {}
            }
        }

        if Instant::now() >= deadline {
            keepalive_record_connect_failure(conn, parsed.host.as_str(), dev_idx);
            let (handle, connected) = {
                let st = conn.state.lock();
                (st.handle, st.connected)
            };
            crate::log!(
                "{}: connect-phase-timeout host={} dev={} handle={} connected={}\n",
                log_prefix,
                parsed.host,
                dev_idx,
                if handle.is_some() { 1 } else { 0 },
                if connected { 1 } else { 0 }
            );
            keepalive_release(conn);
            return Err(if handle.is_none() {
                FetchError::ConnectTimeout
            } else {
                FetchError::TlsTimeout
            });
        }
        Timer::after(EmbassyDuration::from_millis(2)).await;
    }

    Ok(KeepAliveReady {
        conn,
        handle: ready_handle.expect("connected handle"),
        reused,
        deadline,
        last_activity,
    })
}

#[inline]
fn keepalive_mark_disconnected(conn: &'static KeepAliveConn) {
    let mut st = conn.state.lock();
    st.handle = None;
    st.connected = false;
}

#[inline]
fn keepalive_release_connected(conn: &'static KeepAliveConn) {
    let mut st = conn.state.lock();
    st.last_used = Instant::now();
    keepalive_release(conn);
}

#[inline]
fn keepalive_discard_and_release(conn: &'static KeepAliveConn) {
    keepalive_mark_disconnected(conn);
    keepalive_release(conn);
}

#[inline]
fn keepalive_close_discard_and_release(conn: &'static KeepAliveConn, handle: vnet::NetHandle) {
    let _ = conn.cmds.push(TlsCommand::Close { handle });
    keepalive_discard_and_release(conn);
}

fn log_keepalive_done(
    log_prefix: &str,
    parsed: &ParsedHttpsUrl,
    dev_idx: usize,
    status: u16,
    bytes: usize,
    reused: bool,
    closed: bool,
) {
    if closed {
        crate::log!(
            "{}: done host={} dev={} status={} bytes={} reused={} closed=1\n",
            log_prefix,
            parsed.host,
            dev_idx,
            status,
            bytes,
            if reused { 1 } else { 0 }
        );
    } else {
        crate::log!(
            "{}: done host={} dev={} status={} bytes={} reused={}\n",
            log_prefix,
            parsed.host,
            dev_idx,
            status,
            bytes,
            if reused { 1 } else { 0 }
        );
    }
}

struct KeepAliveByteFetch<'a> {
    log_prefix: &'static str,
    request: String,
    request_path: Option<String>,
    done_log: bool,
    log_close_details: bool,
    progress: Option<&'a mut dyn FetchProgress>,
}

async fn fetch_keepalive_bytes(
    parsed: &ParsedHttpsUrl,
    dev_idx: usize,
    timeout_ms: u32,
    max_bytes: usize,
    mut fetch: KeepAliveByteFetch<'_>,
) -> Result<Vec<u8>, FetchError> {
    let KeepAliveReady {
        conn,
        handle,
        reused,
        deadline,
        mut last_activity,
    } = keepalive_prepare_ready(parsed, dev_idx, timeout_ms, fetch.log_prefix).await?;
    let mut last_progress = Instant::now();

    crate::log!(
        "{}: request host={} dev={} reused={}\n",
        fetch.log_prefix,
        parsed.host,
        dev_idx,
        if reused { 1 } else { 0 }
    );

    let req_len = fetch.request.len();
    let _ = conn.cmds.push(TlsCommand::Send {
        handle,
        data: fetch.request.into_bytes(),
    });
    if let Some(path) = fetch.request_path.as_deref() {
        crate::log!(
            "{}: request-sent host={} dev={} handle={} bytes={} path={}\n",
            fetch.log_prefix,
            parsed.host,
            dev_idx,
            handle.0,
            req_len,
            path
        );
    }

    let capture_cap = max_bytes.saturating_add(64 * 1024);
    let mut plaintext: Vec<u8> = Vec::new();
    let mut hdr_end_cached: Option<usize> = None;
    let mut content_len_cached: Option<Option<usize>> = None;
    let mut saw_any_data = false;
    let mut logged_first_data = false;
    let mut saw_header_end = false;

    loop {
        for ev in conn.events.drain(1024) {
            match ev {
                TlsEvent::Data { handle: h, data } => {
                    if h != handle || data.is_empty() {
                        continue;
                    }
                    saw_any_data = true;
                    if fetch.log_close_details && !logged_first_data {
                        crate::log!(
                            "{}: first-data host={} dev={} handle={} bytes={}\n",
                            fetch.log_prefix,
                            parsed.host,
                            dev_idx,
                            handle.0,
                            data.len()
                        );
                        logged_first_data = true;
                    }
                    last_activity = Instant::now();

                    let room = capture_cap.saturating_sub(plaintext.len());
                    if room == 0 {
                        keepalive_close_discard_and_release(conn, handle);
                        return Err(FetchError::ResponseTooLarge);
                    }
                    let take = data.len().min(room);
                    plaintext.extend_from_slice(&data[..take]);
                    if take < data.len() {
                        keepalive_close_discard_and_release(conn, handle);
                        return Err(FetchError::ResponseTooLarge);
                    }

                    let hdr_end = match hdr_end_cached {
                        Some(v) => v,
                        None => {
                            let v = find_http_header_end(&plaintext);
                            if let Some(v) = v {
                                hdr_end_cached = Some(v);
                            }
                            v.unwrap_or(0)
                        }
                    };

                    if let Some(hdr_end) = hdr_end_cached
                        && hdr_end != 0
                    {
                        saw_header_end = true;
                        if content_len_cached.is_none() {
                            let headers = &plaintext[..hdr_end];
                            content_len_cached = Some(header_parse_content_length(headers));
                        }

                        if let Some(ref mut p) = fetch.progress {
                            let now = Instant::now();
                            if now.saturating_duration_since(last_progress)
                                >= EmbassyDuration::from_millis(100)
                            {
                                let body_len = plaintext.len().saturating_sub(hdr_end);
                                p.on_progress(body_len, content_len_cached.unwrap_or(None));
                                last_progress = now;
                            }
                        }
                    }

                    if hdr_end == 0 {
                        continue;
                    }

                    let headers = &plaintext[..hdr_end];
                    let body = &plaintext[hdr_end..];
                    let status = parse_http_status(&plaintext).unwrap_or(0);
                    if status != 200 {
                        if is_redirect_status(status)
                            && let Some(next) = redirect_url_from_location(parsed, headers)
                        {
                            keepalive_close_discard_and_release(conn, handle);
                            return Err(FetchError::Redirect { status, url: next });
                        }

                        log_http_error_response(status, headers, body);
                        keepalive_close_discard_and_release(conn, handle);
                        return Err(FetchError::Http(status));
                    }

                    if status == 204 {
                        keepalive_release_connected(conn);
                        return Ok(Vec::new());
                    }

                    let Some(decoded) = try_decode_complete_http_body(headers, body) else {
                        continue;
                    };
                    ensure_body_within_limit(decoded.as_slice(), max_bytes)?;
                    if let Some(ref mut p) = fetch.progress {
                        p.on_progress(decoded.len(), Some(decoded.len()));
                    }
                    if fetch.done_log {
                        log_keepalive_done(
                            fetch.log_prefix,
                            parsed,
                            dev_idx,
                            status,
                            decoded.len(),
                            reused,
                            false,
                        );
                    }
                    keepalive_release_connected(conn);
                    return Ok(decoded);
                }
                TlsEvent::Closed { handle: h } => {
                    if h != handle {
                        continue;
                    }

                    if fetch.log_close_details && !saw_any_data {
                        crate::log!(
                            "{}: closed-no-data host={} dev={} handle={} reused={}\n",
                            fetch.log_prefix,
                            parsed.host,
                            dev_idx,
                            handle.0,
                            if reused { 1 } else { 0 }
                        );
                    } else if fetch.log_close_details && !saw_header_end {
                        crate::log!(
                            "{}: closed-before-header-end host={} dev={} handle={} raw_bytes={} reused={}\n",
                            fetch.log_prefix,
                            parsed.host,
                            dev_idx,
                            handle.0,
                            plaintext.len(),
                            if reused { 1 } else { 0 }
                        );
                    }

                    keepalive_mark_disconnected(conn);

                    let Some(hdr_end) = find_http_header_end(&plaintext) else {
                        keepalive_release(conn);
                        return Err(FetchError::Http(0));
                    };
                    let headers = &plaintext[..hdr_end];
                    let body = &plaintext[hdr_end..];
                    let status = parse_http_status(&plaintext).unwrap_or(0);

                    if status != 200 {
                        if is_redirect_status(status)
                            && let Some(next) = redirect_url_from_location(parsed, headers)
                        {
                            keepalive_release(conn);
                            return Err(FetchError::Redirect { status, url: next });
                        }
                        keepalive_release(conn);
                        return Err(FetchError::Http(status));
                    }

                    let decoded_body = decode_http_body_lossy(headers, body);
                    ensure_body_within_limit(decoded_body.as_slice(), max_bytes)?;
                    if let Some(ref mut p) = fetch.progress {
                        p.on_progress(decoded_body.len(), Some(decoded_body.len()));
                    }
                    if fetch.done_log {
                        log_keepalive_done(
                            fetch.log_prefix,
                            parsed,
                            dev_idx,
                            status,
                            decoded_body.len(),
                            reused,
                            true,
                        );
                    }
                    keepalive_release(conn);
                    return Ok(decoded_body);
                }
                TlsEvent::TlsError { .. } => {
                    keepalive_discard_and_release(conn);
                    return Err(FetchError::Tls);
                }
                TlsEvent::Error { .. } => {}
                _ => {}
            }
        }

        let now = Instant::now();
        let idle_deadline = last_activity + EmbassyDuration::from_millis(timeout_ms as u64);
        if now >= idle_deadline || now >= deadline {
            keepalive_close_discard_and_release(conn, handle);
            return Err(FetchError::BodyTimeout);
        }
        Timer::after(EmbassyDuration::from_millis(2)).await;
    }
}

// Net-fetch scheduler (used by QJS URL module cache):
// - coalesces concurrent requests for the same cache key
// - caps concurrency to avoid TLS-handshake storms starving the executor
const NET_FETCH_MAX_CONCURRENCY: usize = 4;
static NET_FETCH_ACTIVE: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Default)]
struct InflightFetch {
    owner_op_id: u32,
    followers: Vec<u32>,
}

static CABI_NET_FETCH_INFLIGHT: Mutex<BTreeMap<String, InflightFetch>> =
    Mutex::new(BTreeMap::new());

async fn net_fetch_acquire_slot() {
    loop {
        let cur = NET_FETCH_ACTIVE.load(Ordering::Relaxed);
        if cur < NET_FETCH_MAX_CONCURRENCY
            && NET_FETCH_ACTIVE
                .compare_exchange(cur, cur + 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        {
            return;
        }
        // Cooperative backoff.
        Timer::after(EmbassyDuration::from_millis(1)).await;
    }
}

async fn net_fetch_acquire_slot_while<F>(is_needed: F) -> bool
where
    F: Fn() -> bool,
{
    loop {
        if !is_needed() {
            return false;
        }

        let cur = NET_FETCH_ACTIVE.load(Ordering::Relaxed);
        if cur < NET_FETCH_MAX_CONCURRENCY
            && NET_FETCH_ACTIVE
                .compare_exchange(cur, cur + 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        {
            if !is_needed() {
                net_fetch_release_slot();
                return false;
            }
            return true;
        }

        // Cooperative backoff.
        Timer::after(EmbassyDuration::from_millis(1)).await;
    }
}

fn net_fetch_release_slot() {
    NET_FETCH_ACTIVE.fetch_sub(1, Ordering::AcqRel);
}

fn inflight_fetch_has_live_interest(
    owner_op_id: u32,
    followers: &[u32],
    results: &BTreeMap<u32, Option<i32>>,
) -> bool {
    results.contains_key(&owner_op_id) || followers.iter().any(|id| results.contains_key(id))
}

fn net_fetch_file_task_has_interest(op_id: u32, key: &str) -> bool {
    let (owner_op_id, followers) = {
        let inflight = CABI_NET_FETCH_INFLIGHT.lock();
        let Some(entry) = inflight.get(key) else {
            return false;
        };
        if entry.owner_op_id != op_id {
            return false;
        }
        (entry.owner_op_id, entry.followers.clone())
    };

    let results = CABI_NET_FETCH_RESULTS.lock();
    inflight_fetch_has_live_interest(owner_op_id, followers.as_slice(), &results)
}

fn net_fetch_bytes_op_is_live(op_id: u32) -> bool {
    CABI_NET_FETCH_BYTES_RESULTS.lock().contains_key(&op_id)
}

async fn cabi_net_fetch_task_inner(
    op_id: u32,
    key: String,
    url: String,
    path: String,
    timeout_ms: u32,
    max_bytes: usize,
) {
    let t0 = Instant::now();
    if !net_fetch_acquire_slot_while(|| net_fetch_file_task_has_interest(op_id, key.as_str())).await
    {
        crate::log!("net-fetch: skipped key={} reason=no_interest_before_slot\n", key);
        return;
    }
    let t_fetch_start = Instant::now();
    if !net_fetch_file_task_has_interest(op_id, key.as_str()) {
        net_fetch_release_slot();
        crate::log!("net-fetch: skipped key={} reason=no_interest_after_slot\n", key);
        return;
    }
    let rc =
        match fetch_https_to_file_async(url.as_str(), path.as_str(), timeout_ms, max_bytes).await {
            Ok(()) => 0,
            Err(code) => code,
        };
    net_fetch_release_slot();
    let total_ms = t0.elapsed().as_millis();
    let wait_ms = t_fetch_start.saturating_duration_since(t0).as_millis();
    let fetch_ms = total_ms.saturating_sub(wait_ms);

    let followers = {
        let mut inflight = CABI_NET_FETCH_INFLIGHT.lock();
        inflight
            .remove(&key)
            .map(|e| e.followers)
            .unwrap_or_default()
    };

    let mut map = CABI_NET_FETCH_RESULTS.lock();
    if let Some(slot) = map.get_mut(&op_id) {
        *slot = Some(rc);
    }
    for fid in &followers {
        if let Some(slot) = map.get_mut(fid) {
            *slot = Some(rc);
        }
    }

    crate::log!(
        "net-fetch: done key={} rc={} ms={} wait_ms={} fetch_ms={} followers={}\n",
        key,
        rc,
        total_ms,
        wait_ms,
        fetch_ms,
        followers.len()
    );

    CABI_NET_FETCH_WAIT.notify_all();
}

#[embassy_executor::task]
async fn cabi_net_fetch_task(
    op_id: u32,
    key: String,
    url: String,
    path: String,
    timeout_ms: u32,
    max_bytes: usize,
) {
    cabi_net_fetch_task_inner(op_id, key, url, path, timeout_ms, max_bytes).await;
}

fn spawn_cabi_net_fetch(
    op_id: u32,
    key: String,
    url: String,
    path: String,
    timeout_ms: u32,
    max_bytes: usize,
) {
    if let Some(spawner) = trueos_qjs::workers::pick_background_spawner()
        && let Ok(token) = cabi_net_fetch_task(
            op_id,
            key.clone(),
            url.clone(),
            path.clone(),
            timeout_ms,
            max_bytes,
        )
    {
        spawner.spawn(token);
        return;
    }

    crate::wait::spawn_local_detached(async move {
        cabi_net_fetch_task_inner(op_id, key, url, path, timeout_ms, max_bytes).await;
    });
}

async fn cabi_net_fetch_bytes_task_inner(
    op_id: u32,
    url: String,
    timeout_ms: u32,
    max_bytes: usize,
) {
    let t0 = Instant::now();
    if !net_fetch_acquire_slot_while(|| net_fetch_bytes_op_is_live(op_id)).await {
        crate::log!("net-fetch-bytes: skipped op_id={} reason=no_interest_before_slot\n", op_id);
        return;
    }
    let t_fetch_start = Instant::now();
    if !net_fetch_bytes_op_is_live(op_id) {
        net_fetch_release_slot();
        crate::log!("net-fetch-bytes: skipped op_id={} reason=no_interest_after_slot\n", op_id);
        return;
    }
    let (rc, body) = match fetch_https_body_async(url.as_str(), timeout_ms, max_bytes).await {
        Ok(body) => (0, body),
        Err(code) => (fetch_error_to_code(code), Vec::new()),
    };
    net_fetch_release_slot();
    let total_ms = t0.elapsed().as_millis();
    let wait_ms = t_fetch_start.saturating_duration_since(t0).as_millis();
    let fetch_ms = total_ms.saturating_sub(wait_ms);

    if let Some(slot) = CABI_NET_FETCH_BYTES_RESULTS.lock().get_mut(&op_id) {
        slot.rc = Some(rc);
        slot.body = body;
    }

    crate::log!(
        "net-fetch-bytes: done op_id={} rc={} ms={} wait_ms={} fetch_ms={} len={}\n",
        op_id,
        rc,
        total_ms,
        wait_ms,
        fetch_ms,
        CABI_NET_FETCH_BYTES_RESULTS
            .lock()
            .get(&op_id)
            .map(|v| v.body.len())
            .unwrap_or(0)
    );

    CABI_NET_FETCH_WAIT.notify_all();
}

#[embassy_executor::task]
async fn cabi_net_fetch_bytes_task(op_id: u32, url: String, timeout_ms: u32, max_bytes: usize) {
    cabi_net_fetch_bytes_task_inner(op_id, url, timeout_ms, max_bytes).await;
}

fn spawn_cabi_net_fetch_bytes(op_id: u32, url: String, timeout_ms: u32, max_bytes: usize) {
    if let Some(spawner) = trueos_qjs::workers::pick_background_spawner()
        && let Ok(token) = cabi_net_fetch_bytes_task(op_id, url.clone(), timeout_ms, max_bytes)
    {
        spawner.spawn(token);
        return;
    }

    crate::wait::spawn_local_detached(async move {
        cabi_net_fetch_bytes_task_inner(op_id, url, timeout_ms, max_bytes).await;
    });
}

async fn cabi_net_prewarm_url_task_inner(url: String) {
    let Some(parsed) = parse_https_url(url.as_str()) else {
        return;
    };
    let profile = NetProfile::default();
    let Some(dev_idx) = profile.resolve_device_index() else {
        return;
    };
    let _ = dns::resolve_ipv4_for_device(
        dev_idx,
        parsed.host.as_str(),
        DnsConfig::for_profile(profile),
    )
    .await;
}

#[embassy_executor::task]
async fn cabi_net_prewarm_url_task(url: String) {
    cabi_net_prewarm_url_task_inner(url).await;
}

fn spawn_cabi_net_prewarm_url(url: String) {
    if let Some(spawner) = trueos_qjs::workers::pick_background_spawner()
        && let Ok(token) = cabi_net_prewarm_url_task(url.clone())
    {
        spawner.spawn(token);
        return;
    }

    crate::wait::spawn_local_detached(async move {
        cabi_net_prewarm_url_task_inner(url).await;
    });
}

/// Errors returned by [`fetch_https_body_async`].
#[derive(Clone, Debug)]
pub enum FetchError {
    NoNic,
    BadUrl,
    DnsFailed,
    DnsTimeout,
    ConnectTimeout,
    TlsTimeout,
    BodyTimeout,
    Tls,
    Http(u16),
    Redirect { status: u16, url: String },
    ResponseTooLarge,
}

/// Progress callback for HTTPS body fetches.
///
/// `received` counts body bytes received so far (not including headers).
/// `total` is the Content-Length when known.
pub trait FetchProgress {
    fn on_progress(&mut self, received: usize, total: Option<usize>);
}

#[inline]
fn fetch_device_index(profile: NetProfile) -> Result<usize, FetchError> {
    profile.resolve_device_index().ok_or(FetchError::NoNic)
}

#[inline]
fn fetch_device_index_code(profile: NetProfile) -> Result<usize, i32> {
    profile
        .resolve_device_index()
        .ok_or(fetch_error_to_code(FetchError::NoNic))
}

/// Callback sink for Server-Sent Events (SSE) streaming.
///
/// The handler receives the raw `data:` payload (already concatenated across
/// multiple `data:` lines for a single SSE event).
pub trait SseHandler {
    fn on_data(&mut self, data: &str);
}

#[inline]
fn block_error_to_code(err: crate::disc::block::Error) -> i32 {
    use crate::disc::block::Error;
    match err {
        Error::InvalidParam | Error::OutOfBounds => FS_ERR_BAD_PARAM,
        Error::NotReady => FS_ERR_USBMS_NOT_FOUND,
        Error::Corrupted
        | Error::Io
        | Error::Timeout
        | Error::NotSupported
        | Error::DmaUnavailable
        | Error::MmioMapFailed => FS_ERR_IO,
    }
}

#[inline]
fn fetch_error_to_code(err: FetchError) -> i32 {
    match err {
        FetchError::NoNic => NET_ERR_TIMEOUT,
        FetchError::BadUrl => NET_ERR_BAD_URL,
        FetchError::DnsFailed | FetchError::DnsTimeout => NET_ERR_TIMEOUT_DNS,
        FetchError::ConnectTimeout => NET_ERR_TIMEOUT_CONNECT,
        FetchError::TlsTimeout => NET_ERR_TIMEOUT_TLS,
        FetchError::BodyTimeout => NET_ERR_TIMEOUT_BODY,
        FetchError::Tls => NET_ERR_TLS,
        FetchError::Http(status) => {
            let _status = status;
            NET_ERR_HTTP
        }
        FetchError::Redirect { .. } => NET_ERR_HTTP,
        FetchError::ResponseTooLarge => FS_ERR_TOO_LARGE,
    }
}

#[inline]
fn http_fetch_error_to_code(err: HttpFetchError) -> i32 {
    match err {
        HttpFetchError::BadUrl => NET_ERR_BAD_URL,
        HttpFetchError::TimedOut => NET_ERR_TIMEOUT,
        HttpFetchError::DnsFailed => NET_ERR_TIMEOUT_DNS,
        HttpFetchError::HttpStatus(status) => {
            let _status = status;
            NET_ERR_HTTP
        }
        HttpFetchError::Redirect(_) => NET_ERR_HTTP,
        HttpFetchError::ResponseTooLarge => FS_ERR_TOO_LARGE,
    }
}

async fn post_json_body_async(
    url: &str,
    body_json: String,
    bearer: Option<&str>,
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<Vec<u8>, i32> {
    if url.starts_with("http://") {
        let auth_header = bearer.map(|token| alloc::format!("Bearer {}", token));
        let headers_with_auth = [
            ("Content-Type", "application/json"),
            ("Accept", "application/json"),
            ("Authorization", auth_header.as_deref().unwrap_or_default()),
        ];
        let headers_without_auth = [
            ("Content-Type", "application/json"),
            ("Accept", "application/json"),
        ];
        let headers = if auth_header.is_some() {
            &headers_with_auth[..]
        } else {
            &headers_without_auth[..]
        };
        return http::post_http_body(url, headers, body_json.as_bytes(), timeout_ms, max_bytes)
            .await
            .map_err(http_fetch_error_to_code);
    }

    post_https_json_async(url, body_json, bearer, timeout_ms, max_bytes)
        .await
        .map_err(fetch_error_to_code)
}

#[inline]
fn is_redirect_status(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

fn redirect_url_from_location(current: &ParsedHttpsUrl, headers: &[u8]) -> Option<String> {
    let loc = header_get_value(headers, b"location")?;
    let loc = core::str::from_utf8(loc).ok()?.trim();
    if loc.is_empty() {
        return None;
    }

    // Only follow HTTPS redirects.
    if loc.starts_with("https://") {
        return Some(String::from(loc));
    }
    if loc.starts_with("http://") {
        return None;
    }

    // Origin-relative redirect: "/path".
    if loc.starts_with('/') {
        if current.port == 443 {
            return Some(format!("https://{}{}", current.host, loc));
        }
        return Some(format!("https://{}:{}{}", current.host, current.port, loc));
    }

    None
}

fn normalize_rel(path: &str, allow_empty: bool) -> Result<String, i32> {
    let mut out = String::new();
    let t = path.trim();
    if t.is_empty() {
        return if allow_empty {
            Ok(out)
        } else {
            Err(FS_ERR_BAD_PATH)
        };
    }

    for part in t.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err(FS_ERR_BAD_PATH);
        }
        if !out.is_empty() {
            out.push('/');
        }
        out.push_str(part);
    }

    if out.is_empty() && !allow_empty {
        return Err(FS_ERR_BAD_PATH);
    }
    Ok(out)
}

#[derive(Clone, Debug)]
struct ParsedHttpsUrl {
    host: String,
    port: u16,
    path: String,
}

#[derive(Clone, Copy)]
enum HttpRequestMethod {
    Get,
    Post,
}

#[derive(Clone, Copy)]
enum HttpConnectionMode {
    Close,
    KeepAlive,
}

struct HttpRequestSpec<'a> {
    method: HttpRequestMethod,
    host: &'a str,
    path: &'a str,
    connection: HttpConnectionMode,
    accept: &'a str,
    accept_encoding_identity: bool,
    content_type: Option<&'a str>,
    body: Option<&'a str>,
    auth_bearer: Option<&'a str>,
}

fn build_http_request(spec: HttpRequestSpec<'_>) -> String {
    let method = match spec.method {
        HttpRequestMethod::Get => "GET",
        HttpRequestMethod::Post => "POST",
    };
    let connection = match spec.connection {
        HttpConnectionMode::Close => "close",
        HttpConnectionMode::KeepAlive => "keep-alive",
    };

    let mut req = String::new();
    let _ = write!(
        &mut req,
        "{} {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: TRUEOS vhttps\r\nConnection: {}\r\n",
        method, spec.path, spec.host, connection,
    );
    if let Some(content_type) = spec.content_type {
        let _ = write!(&mut req, "Content-Type: {}\r\n", content_type);
    }
    let _ = write!(&mut req, "Accept: {}\r\n", spec.accept);
    if spec.accept_encoding_identity {
        req.push_str("Accept-Encoding: identity\r\n");
    }
    if let Some(token) = spec.auth_bearer {
        let _ = write!(&mut req, "Authorization: Bearer {}\r\n", token);
    }
    if let Some(body) = spec.body {
        let _ = write!(&mut req, "Content-Length: {}\r\n\r\n{}", body.len(), body);
    } else {
        req.push_str("\r\n");
    }
    req
}

fn parse_https_url(url: &str) -> Option<ParsedHttpsUrl> {
    let url = url.strip_prefix("https://")?;

    // Split authority and path.
    let (authority, path) = match url.split_once('/') {
        Some((a, p)) => (a, format!("/{}", p)),
        None => (url, String::from("/")),
    };

    if authority.is_empty() {
        return None;
    }

    // Parse optional ":port" in authority.
    let (host, port) = if let Some((h, p)) = authority.rsplit_once(':') {
        // Only treat as port if digits.
        if !p.is_empty() && p.as_bytes().iter().all(|b| b.is_ascii_digit()) {
            let port = p.parse::<u16>().ok()?;
            (String::from(h), port)
        } else {
            (String::from(authority), 443)
        }
    } else {
        (String::from(authority), 443)
    };

    if host.is_empty() {
        return None;
    }

    Some(ParsedHttpsUrl { host, port, path })
}

fn leak_str(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn find_http_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}

fn parse_http_status(buf: &[u8]) -> Option<u16> {
    // Expect: HTTP/1.1 200 ...\r\n
    if !buf.starts_with(b"HTTP/") {
        return None;
    }
    let mut i = 0;
    while i < buf.len() && buf[i] != b' ' {
        i += 1;
    }
    while i < buf.len() && buf[i] == b' ' {
        i += 1;
    }
    if i + 3 > buf.len() {
        return None;
    }
    let a = *buf.get(i)?;
    let b = *buf.get(i + 1)?;
    let c = *buf.get(i + 2)?;
    if !a.is_ascii_digit() || !b.is_ascii_digit() || !c.is_ascii_digit() {
        return None;
    }
    Some(((a - b'0') as u16) * 100 + ((b - b'0') as u16) * 10 + ((c - b'0') as u16))
}

fn header_get_value<'a>(headers: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
    // Case-insensitive header match. Returns trimmed value bytes.
    let mut i = 0;
    while i < headers.len() {
        let line_start = i;
        while i < headers.len() && headers[i] != b'\n' {
            i += 1;
        }
        let mut line = &headers[line_start..i];
        if i < headers.len() && headers[i] == b'\n' {
            i += 1;
        }
        if let Some((&b'\r', rest)) = line.split_last() {
            line = rest;
        }
        if line.is_empty() {
            continue;
        }
        let Some(colon) = line.iter().position(|b| *b == b':') else {
            continue;
        };
        let (k, mut v) = line.split_at(colon);
        // Skip ':'
        v = v.get(1..).unwrap_or(&[]);
        if k.len() != name.len() {
            continue;
        }
        if !k
            .iter()
            .zip(name.iter())
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
        {
            continue;
        }
        while !v.is_empty() && (v[0] == b' ' || v[0] == b'\t') {
            v = &v[1..];
        }
        return Some(v);
    }
    None
}

fn header_value_contains_token(value: &[u8], token: &[u8]) -> bool {
    let v = value
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect::<Vec<u8>>();
    let t = token
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect::<Vec<u8>>();

    v.split(|b| *b == b',' || *b == b' ' || *b == b'\t')
        .any(|part| part == t.as_slice())
}

fn header_contains_token(headers: &[u8], name: &[u8], token: &[u8]) -> bool {
    let Some(v) = header_get_value(headers, name) else {
        return false;
    };
    header_value_contains_token(v, token)
}

fn header_parse_content_length(headers: &[u8]) -> Option<usize> {
    let v = header_get_value(headers, b"content-length")?;
    let v = core::str::from_utf8(v).ok()?;
    v.trim().parse::<usize>().ok()
}

fn decode_http_chunked(body: &[u8]) -> Option<Vec<u8>> {
    // Minimal chunked decoder. Returns decoded bytes if fully present.
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0usize;

    loop {
        // Read chunk size line.
        let line_end = body[i..].windows(2).position(|w| w == b"\r\n")?;
        let line = &body[i..i + line_end];
        i += line_end + 2;

        // Strip extensions.
        let line = line.split(|b| *b == b';').next().unwrap_or(line);
        let line_str = core::str::from_utf8(line).ok()?;
        let size = usize::from_str_radix(line_str.trim(), 16).ok()?;

        if size == 0 {
            // Ignore trailers; we're done.
            return Some(out);
        }

        if i + size > body.len() {
            return None;
        }
        out.extend_from_slice(&body[i..i + size]);
        i += size;

        // Expect CRLF after data.
        if i + 2 > body.len() || &body[i..i + 2] != b"\r\n" {
            return None;
        }
        i += 2;
    }
}

fn decode_http_body_lossy(headers: &[u8], body: &[u8]) -> Vec<u8> {
    if header_contains_token(headers, b"transfer-encoding", b"chunked") {
        decode_http_chunked(body).unwrap_or_else(|| body.to_vec())
    } else if let Some(len) = header_parse_content_length(headers) {
        body.get(..len).unwrap_or(body).to_vec()
    } else {
        body.to_vec()
    }
}

fn try_decode_complete_http_body(headers: &[u8], body: &[u8]) -> Option<Vec<u8>> {
    if header_contains_token(headers, b"transfer-encoding", b"chunked") {
        decode_http_chunked(body)
    } else if let Some(len) = header_parse_content_length(headers) {
        if body.len() >= len {
            Some(body[..len].to_vec())
        } else {
            None
        }
    } else {
        None
    }
}

fn log_http_error_response(status: u16, headers: &[u8], body: &[u8]) {
    let decoded_body = decode_http_body_lossy(headers, body);
    crate::log!("vhttps: http_error status={} body_len={}\n", status, decoded_body.len());
    if let Ok(s) = core::str::from_utf8(decoded_body.as_slice()) {
        log_utf8_chunks("vhttps: http_error_body: ", s);
    } else {
        crate::log!("vhttps: http_error_body: [non-utf8]\n");
    }
}

#[inline]
fn ensure_body_within_limit(decoded_body: &[u8], max_bytes: usize) -> Result<(), FetchError> {
    if decoded_body.len() > max_bytes {
        Err(FetchError::ResponseTooLarge)
    } else {
        Ok(())
    }
}

fn log_utf8_chunks(prefix: &str, s: &str) {
    // Avoid log-line truncation by splitting into multiple lines.
    // UTF-8 safe: ensure chunk boundaries are on char boundaries.
    const CHUNK: usize = 768;
    let mut i = 0usize;
    while i < s.len() {
        let mut end = (i + CHUNK).min(s.len());
        while end < s.len() && !s.is_char_boundary(end) {
            end = end.saturating_sub(1);
        }
        if end == i {
            // Avoid infinite loop on unexpected boundary issues.
            end = (i + 1).min(s.len());
            while end < s.len() && !s.is_char_boundary(end) {
                end += 1;
            }
        }
        crate::log!("{}{}\n", prefix, &s[i..end]);
        i = end;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HttpBodyKind {
    ContentLength(usize),
    Chunked,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct HttpHead {
    status: u16,
    body: HttpBodyKind,
}

fn parse_http_head(headers: &[u8]) -> Option<HttpHead> {
    let status = parse_http_status(headers)?;
    let chunked = header_contains_token(headers, b"transfer-encoding", b"chunked");
    if chunked {
        return Some(HttpHead {
            status,
            body: HttpBodyKind::Chunked,
        });
    }
    let len = header_parse_content_length(headers)?;
    Some(HttpHead {
        status,
        body: HttpBodyKind::ContentLength(len),
    })
}

fn log_http_head(prefix: &str, host: &str, head: HttpHead) {
    match head.body {
        HttpBodyKind::ContentLength(len) => {
            crate::log!(
                "{} host={} status={} body=content-length len={}\n",
                prefix,
                host,
                head.status,
                len
            );
        }
        HttpBodyKind::Chunked => {
            crate::log!("{} host={} status={} body=chunked\n", prefix, host, head.status);
        }
    }
}

async fn write_body_to_tmp_file(
    disk: crate::disc::block::DeviceHandle,
    tmp_path: &str,
    body: &[u8],
) -> Result<(), i32> {
    let Some(sh) =
        crate::r::fs::trueosfs::file_write_begin_async(disk, tmp_path, body.len() as u64)
            .await
            .map_err(block_error_to_code)?
    else {
        return Err(FS_ERR_NO_SPACE);
    };
    if !body.is_empty() {
        crate::r::fs::trueosfs::file_write_chunk_async(sh, body)
            .await
            .map_err(block_error_to_code)?;
    }
    crate::r::fs::trueosfs::file_write_finish_async(sh)
        .await
        .map_err(block_error_to_code)?;
    Ok(())
}

static VHTTPS_SEQ: AtomicU32 = AtomicU32::new(1);

// Keep vhttps logging minimal by default; verbose prints are useful for debugging
// but can flood globalog during downloads.
async fn fetch_on_device(
    parsed: &ParsedHttpsUrl,
    dev_idx: usize,
    timeout_ms: u32,
    max_bytes: usize,
    body_json: Option<&str>,
    auth_token: Option<&str>,
    mut progress: Option<&mut dyn FetchProgress>,
) -> Result<Vec<u8>, FetchError> {
    // If the caller asked for progress updates, this is likely a large transfer.
    // Avoid per-chunk logging (which floods globalog); emit a single completion line instead.
    let want_done_log = progress.is_some();

    let ip = match dns::resolve_ipv4_for_device(
        dev_idx,
        parsed.host.as_str(),
        DnsConfig::for_device(dev_idx),
    )
    .await
    {
        Ok(ip) => ip,
        Err(dns::DnsError::Timeout) => return Err(FetchError::DnsTimeout),
        Err(_) => return Err(FetchError::DnsFailed),
    };

    let seq = VHTTPS_SEQ.fetch_add(1, Ordering::Relaxed);
    // Suffix with a stable selector so tls-socket can pin the underlying TCP socket to the chosen NIC.
    // Prefer PCI BDF (unique), otherwise fall back to VID:PID.
    let selector = if let Some((bus, slot, func)) = crate::net::bdf_at(dev_idx) {
        format!("{:02x}:{:02x}.{}", bus, slot, func)
    } else if let Some((vid, pid)) = crate::net::pci_id_at(dev_idx) {
        format!("{:04x}:{:04x}", vid, pid)
    } else {
        format!("{}", dev_idx)
    };
    let owner = leak_str(format!("vhttps-{}@{}", seq, selector));
    let cmds_name = leak_str(format!("{}-tls-cmd", owner));
    let evts_name = leak_str(format!("{}-tls-evt", owner));

    // These queues can see a burst of TCP segments (small `TlsEvent::Data` packets).
    // If the consumer drains too slowly, events may be dropped and large downloads can stall.
    let cmds = Queue::new_leaked(cmds_name, 256);
    let events = Queue::new_leaked(evts_name, 4096);
    register_tls_app_queues(owner, cmds, events);

    let roots = TlsRoots::mozilla();
    let cfg = TlsClientConfig::new().with_alpn_protocols(&[b"http/1.1"]);
    let server_name = leak_str(parsed.host.clone());

    let start = Instant::now();
    let total_deadline = start + EmbassyDuration::from_millis(timeout_ms as u64);
    let connect_ms: u64 = (timeout_ms as u64 / 4).max(5_000);
    let tls_ms: u64 = (timeout_ms as u64 / 4).max(5_000);
    let connect_deadline = start + EmbassyDuration::from_millis(connect_ms);
    // TLS can only start after TCP open; this is a best-effort wall clock deadline.
    let tls_deadline = start + EmbassyDuration::from_millis(connect_ms.saturating_add(tls_ms));

    let mut tls_handle: Option<vnet::NetHandle> = None;
    let mut sent_connect = false;
    let mut http_sent = false;
    let mut logged_request_sent = false;
    let mut logged_first_data = false;
    let mut saw_any_data = false;
    let mut saw_header_end = false;

    // Capture plaintext up to (headers + body cap). We parse after close.
    // Keep this bounded even if a server misbehaves.
    let capture_cap = max_bytes.saturating_add(64 * 1024);
    let mut plaintext: Vec<u8> = Vec::new();

    // Once we've seen complete headers, try to finish early (without waiting for TCP/TLS close)
    // when the response body is complete (Content-Length or chunked terminator).
    let mut hdr_end_cached: Option<usize> = None;
    let mut content_len_cached: Option<Option<usize>> = None;

    // Rate-limit progress callbacks.
    let mut last_progress: Instant = Instant::now();

    let mut last_activity = Instant::now();

    loop {
        for ev in events.drain(1024) {
            match ev {
                TlsEvent::Opened { handle } => {
                    last_activity = Instant::now();
                    tls_handle = Some(handle);
                }
                TlsEvent::Connected { handle } => {
                    last_activity = Instant::now();
                    if tls_handle != Some(handle) {
                        continue;
                    }
                    if !http_sent {
                        let req = build_http_request(HttpRequestSpec {
                            method: if body_json.is_some() {
                                HttpRequestMethod::Post
                            } else {
                                HttpRequestMethod::Get
                            },
                            host: parsed.host.as_str(),
                            path: parsed.path.as_str(),
                            connection: HttpConnectionMode::Close,
                            accept: "*/*",
                            accept_encoding_identity: false,
                            content_type: body_json.map(|_| "application/json"),
                            body: body_json,
                            auth_bearer: auth_token,
                        });
                        let _ = cmds.push(TlsCommand::Send {
                            handle,
                            data: req.into_bytes(),
                        });
                        http_sent = true;
                        if !logged_request_sent {
                            crate::log!(
                                "vhttps: request-sent host={} dev={} handle={} bytes={} path={}\n",
                                parsed.host,
                                dev_idx,
                                handle.0,
                                if let Some(body) = body_json {
                                    body.len()
                                } else {
                                    0
                                },
                                parsed.path
                            );
                            logged_request_sent = true;
                        }
                    }
                }
                TlsEvent::Data { handle, data } => {
                    last_activity = Instant::now();
                    if tls_handle != Some(handle) {
                        continue;
                    }
                    if !data.is_empty() {
                        saw_any_data = true;
                        if !logged_first_data {
                            crate::log!(
                                "vhttps: first-data host={} dev={} handle={} bytes={}\n",
                                parsed.host,
                                dev_idx,
                                handle.0,
                                data.len()
                            );
                            logged_first_data = true;
                        }
                        let room = capture_cap.saturating_sub(plaintext.len());
                        if room == 0 {
                            if let Some(h) = tls_handle {
                                let _ = cmds.push(TlsCommand::Close { handle: h });
                            }
                            return Err(FetchError::ResponseTooLarge);
                        }
                        let take = data.len().min(room);
                        plaintext.extend_from_slice(&data[..take]);
                        if take < data.len() {
                            if let Some(h) = tls_handle {
                                let _ = cmds.push(TlsCommand::Close { handle: h });
                            }
                            return Err(FetchError::ResponseTooLarge);
                        }

                        // If we have enough data to fully satisfy the response, finish now.
                        let hdr_end = match hdr_end_cached {
                            Some(v) => v,
                            None => {
                                let v = find_http_header_end(&plaintext);
                                if let Some(v) = v {
                                    hdr_end_cached = Some(v);
                                }
                                v.unwrap_or(0)
                            }
                        };

                        // Progress reporting: once headers are known, report body byte count.
                        if let Some(hdr_end) = hdr_end_cached
                            && hdr_end != 0
                        {
                            saw_header_end = true;
                            if content_len_cached.is_none() {
                                let headers = &plaintext[..hdr_end];
                                content_len_cached = Some(header_parse_content_length(headers));
                            }

                            if let Some(ref mut p) = progress {
                                // Avoid spamming UI: update at most ~10Hz.
                                let now = Instant::now();
                                if now.saturating_duration_since(last_progress)
                                    >= EmbassyDuration::from_millis(100)
                                {
                                    let body_len = plaintext.len().saturating_sub(hdr_end);
                                    p.on_progress(body_len, content_len_cached.unwrap_or(None));
                                    last_progress = now;
                                }
                            }
                        }
                        if hdr_end != 0 {
                            let headers = &plaintext[..hdr_end];
                            let body = &plaintext[hdr_end..];

                            let status = parse_http_status(&plaintext).unwrap_or(0);
                            if status != 200 {
                                if is_redirect_status(status)
                                    && let Some(next) = redirect_url_from_location(parsed, headers)
                                {
                                    if let Some(h) = tls_handle {
                                        let _ = cmds.push(TlsCommand::Close { handle: h });
                                    }
                                    return Err(FetchError::Redirect { status, url: next });
                                }

                                // Log error bodies (often JSON) to aid debugging.
                                log_http_error_response(status, headers, body);

                                if let Some(h) = tls_handle {
                                    let _ = cmds.push(TlsCommand::Close { handle: h });
                                }
                                return Err(FetchError::Http(status));
                            }
                            // 204 No Content
                            if status == 204 {
                                if let Some(h) = tls_handle {
                                    let _ = cmds.push(TlsCommand::Close { handle: h });
                                }
                                return Ok(Vec::new());
                            }

                            let is_chunked =
                                header_contains_token(headers, b"transfer-encoding", b"chunked");
                            if is_chunked {
                                if let Some(decoded) = decode_http_chunked(body) {
                                    if decoded.len() > max_bytes {
                                        if let Some(h) = tls_handle {
                                            let _ = cmds.push(TlsCommand::Close { handle: h });
                                        }
                                        return Err(FetchError::ResponseTooLarge);
                                    }
                                    if let Some(h) = tls_handle {
                                        let _ = cmds.push(TlsCommand::Close { handle: h });
                                    }
                                    if let Some(ref mut p) = progress {
                                        p.on_progress(decoded.len(), Some(decoded.len()));
                                    }
                                    if want_done_log {
                                        crate::log!(
                                            "vhttps: done host={} dev={} status={} bytes={}\n",
                                            parsed.host,
                                            dev_idx,
                                            status,
                                            decoded.len(),
                                        );
                                    }
                                    return Ok(decoded);
                                }
                            } else if let Some(len) = header_parse_content_length(headers) {
                                if body.len() >= len {
                                    let out = body[..len].to_vec();
                                    if out.len() > max_bytes {
                                        if let Some(h) = tls_handle {
                                            let _ = cmds.push(TlsCommand::Close { handle: h });
                                        }
                                        return Err(FetchError::ResponseTooLarge);
                                    }
                                    if let Some(h) = tls_handle {
                                        let _ = cmds.push(TlsCommand::Close { handle: h });
                                    }
                                    if let Some(ref mut p) = progress {
                                        p.on_progress(out.len(), Some(out.len()));
                                    }
                                    if want_done_log {
                                        crate::log!(
                                            "vhttps: done host={} dev={} status={} bytes={}\n",
                                            parsed.host,
                                            dev_idx,
                                            status,
                                            out.len(),
                                        );
                                    }
                                    return Ok(out);
                                }
                            } else {
                                // No chunked, no content-length. If Connection: close, we wait for close.
                                // If status implies no body (HEAD request, 1xx, 204, 304), handled above or implicitly.
                            }
                        }
                    }
                }
                TlsEvent::Closed { handle } => {
                    if tls_handle != Some(handle) {
                        continue;
                    }

                    if !saw_any_data {
                        crate::log!(
                            "vhttps: closed-no-data host={} dev={} handle={}\n",
                            parsed.host,
                            dev_idx,
                            handle.0
                        );
                    } else if !saw_header_end {
                        crate::log!(
                            "vhttps: closed-before-header-end host={} dev={} handle={} raw_bytes={}\n",
                            parsed.host,
                            dev_idx,
                            handle.0,
                            plaintext.len()
                        );
                    }

                    let Some(hdr_end) = find_http_header_end(&plaintext) else {
                        return Err(FetchError::Http(0));
                    };
                    let headers = &plaintext[..hdr_end];
                    let body = &plaintext[hdr_end..];

                    let status = parse_http_status(&plaintext).unwrap_or(0);
                    if status != 200 {
                        if is_redirect_status(status)
                            && let Some(next) = redirect_url_from_location(parsed, headers)
                        {
                            return Err(FetchError::Redirect { status, url: next });
                        }

                        // Log error bodies (often JSON) to aid debugging.
                        let is_chunked =
                            header_contains_token(headers, b"transfer-encoding", b"chunked");
                        let decoded_body = if is_chunked {
                            decode_http_chunked(body).unwrap_or_else(|| body.to_vec())
                        } else if let Some(len) = header_parse_content_length(headers) {
                            body.get(..len).unwrap_or(body).to_vec()
                        } else {
                            body.to_vec()
                        };
                        crate::log!(
                            "vhttps: http_error status={} body_len={}\n",
                            status,
                            decoded_body.len()
                        );
                        if let Ok(s) = core::str::from_utf8(decoded_body.as_slice()) {
                            log_utf8_chunks("vhttps: http_error_body: ", s);
                        } else {
                            crate::log!("vhttps: http_error_body: [non-utf8]\n");
                        }

                        return Err(FetchError::Http(status));
                    }

                    let is_chunked =
                        header_contains_token(headers, b"transfer-encoding", b"chunked");
                    let decoded_body = if is_chunked {
                        decode_http_chunked(body).unwrap_or_else(|| body.to_vec())
                    } else if let Some(len) = header_parse_content_length(headers) {
                        body.get(..len).unwrap_or(body).to_vec()
                    } else {
                        body.to_vec()
                    };

                    if decoded_body.len() > max_bytes {
                        return Err(FetchError::ResponseTooLarge);
                    }

                    if let Some(ref mut p) = progress {
                        p.on_progress(decoded_body.len(), Some(decoded_body.len()));
                    }

                    if want_done_log {
                        crate::log!(
                            "vhttps: done host={} dev={} status={} bytes={}\n",
                            parsed.host,
                            dev_idx,
                            status,
                            decoded_body.len(),
                        );
                    }

                    // Trim any accidental leading/trailing whitespace? No: callers want exact bytes.
                    return Ok(decoded_body);
                }
                TlsEvent::Error { .. } => {
                    // Keep waiting; underlying net can emit transient errors.
                }
                TlsEvent::TlsError { .. } => {
                    if let Some(h) = tls_handle {
                        let _ = cmds.push(TlsCommand::Close { handle: h });
                    }
                    return Err(FetchError::Tls);
                }
            }
        }

        if !sent_connect {
            let t = crate::net::tls_socket::TlsTimeouts {
                connect_ms: (timeout_ms / 4).max(5_000),
                tls_ms: (timeout_ms / 4).max(5_000),
                idle_ms: timeout_ms,
            };
            let _ = cmds.push(TlsCommand::OpenTcpConnect {
                remote: vnet::EndpointV4 {
                    addr: ip,
                    port: parsed.port,
                },
                server_name,
                cfg: cfg.clone(),
                roots: roots.clone(),
                timeouts: t,
            });
            crate::log!(
                "vhttps: connect host={} dev={} timeout_ms={}\n",
                parsed.host,
                dev_idx,
                timeout_ms
            );
            sent_connect = true;
        }

        let now = Instant::now();
        if tls_handle.is_none() && now >= connect_deadline {
            if let Some(h) = tls_handle {
                let _ = cmds.push(TlsCommand::Close { handle: h });
            }
            crate::log!(
                "vhttps: connect-timeout host={} dev={} saw_data={} hdr={} raw_bytes={}\n",
                parsed.host,
                dev_idx,
                if saw_any_data { 1 } else { 0 },
                if saw_header_end { 1 } else { 0 },
                plaintext.len()
            );
            return Err(FetchError::ConnectTimeout);
        }
        if tls_handle.is_some() && !http_sent && now >= tls_deadline {
            if let Some(h) = tls_handle {
                let _ = cmds.push(TlsCommand::Close { handle: h });
            }
            crate::log!(
                "vhttps: tls-timeout host={} dev={} opened=1 request_sent=0 saw_data={} raw_bytes={}\n",
                parsed.host,
                dev_idx,
                if saw_any_data { 1 } else { 0 },
                plaintext.len()
            );
            return Err(FetchError::TlsTimeout);
        }
        if http_sent {
            let idle_deadline = last_activity + EmbassyDuration::from_millis(timeout_ms as u64);
            if now >= idle_deadline {
                if let Some(h) = tls_handle {
                    let _ = cmds.push(TlsCommand::Close { handle: h });
                }
                crate::log!(
                    "vhttps: body-timeout host={} dev={} request_sent=1 saw_data={} hdr={} raw_bytes={}\n",
                    parsed.host,
                    dev_idx,
                    if saw_any_data { 1 } else { 0 },
                    if saw_header_end { 1 } else { 0 },
                    plaintext.len()
                );
                return Err(FetchError::BodyTimeout);
            }
        }
        if now >= total_deadline {
            if let Some(h) = tls_handle {
                let _ = cmds.push(TlsCommand::Close { handle: h });
            }
            crate::log!(
                "vhttps: total-timeout host={} dev={} opened={} request_sent={} saw_data={} hdr={} raw_bytes={}\n",
                parsed.host,
                dev_idx,
                if tls_handle.is_some() { 1 } else { 0 },
                if http_sent { 1 } else { 0 },
                if saw_any_data { 1 } else { 0 },
                if saw_header_end { 1 } else { 0 },
                plaintext.len()
            );
            // Fallback classification: we hit the total wall clock deadline.
            return Err(if tls_handle.is_none() {
                FetchError::ConnectTimeout
            } else if !http_sent {
                FetchError::TlsTimeout
            } else {
                FetchError::BodyTimeout
            });
        }

        Timer::after(EmbassyDuration::from_millis(2)).await;
    }
}

async fn fetch_on_device_sse(
    parsed: &ParsedHttpsUrl,
    dev_idx: usize,
    timeout_ms: u32,
    max_bytes: usize,
    body_json: &str,
    auth_token: Option<&str>,
    handler: &mut dyn SseHandler,
) -> Result<(), FetchError> {
    fn sse_json_type_hint(s: &str) -> Option<&str> {
        let needle = "\"type\":\"";
        let i = s.find(needle)?;
        let start = i + needle.len();
        let end_rel = s[start..].find('"')?;
        Some(&s[start..start + end_rel])
    }

    fn set_preview(dst: &mut String, src: &str, max_chars: usize) {
        dst.clear();
        for ch in src.chars().take(max_chars) {
            if ch.is_control() {
                dst.push(' ');
            } else {
                dst.push(ch);
            }
        }
    }

    let ip = match dns::resolve_ipv4_for_device(
        dev_idx,
        parsed.host.as_str(),
        DnsConfig::for_device(dev_idx),
    )
    .await
    {
        Ok(ip) => ip,
        Err(dns::DnsError::Timeout) => return Err(FetchError::DnsTimeout),
        Err(_) => return Err(FetchError::DnsFailed),
    };

    let seq = VHTTPS_SEQ.fetch_add(1, Ordering::Relaxed);
    let selector = if let Some((bus, slot, func)) = crate::net::bdf_at(dev_idx) {
        format!("{:02x}:{:02x}.{}", bus, slot, func)
    } else if let Some((vid, pid)) = crate::net::pci_id_at(dev_idx) {
        format!("{:04x}:{:04x}", vid, pid)
    } else {
        format!("{}", dev_idx)
    };
    let owner = leak_str(format!("vhttpssse-{}@{}", seq, selector));
    let cmds_name = leak_str(format!("{}-tls-cmd", owner));
    let evts_name = leak_str(format!("{}-tls-evt", owner));
    let cmds = Queue::new_leaked(cmds_name, 256);
    let events = Queue::new_leaked(evts_name, 4096);
    register_tls_app_queues(owner, cmds, events);

    let roots = TlsRoots::mozilla();
    let cfg = TlsClientConfig::new().with_alpn_protocols(&[b"http/1.1"]);
    let server_name = leak_str(parsed.host.clone());

    let start = Instant::now();
    let total_deadline = start + EmbassyDuration::from_millis(timeout_ms as u64);
    let connect_ms: u64 = (timeout_ms as u64 / 4).max(5_000);
    let tls_ms: u64 = (timeout_ms as u64 / 4).max(5_000);
    let connect_deadline = start + EmbassyDuration::from_millis(connect_ms);
    let tls_deadline = start + EmbassyDuration::from_millis(connect_ms.saturating_add(tls_ms));

    let mut tls_handle: Option<vnet::NetHandle> = None;
    let mut sent_connect = false;
    let mut http_sent = false;
    let mut last_activity = Instant::now();

    // Capture enough plaintext for headers + some body. Keep bounded.
    let capture_cap = max_bytes.saturating_add(64 * 1024);
    let mut plaintext: Vec<u8> = Vec::new();
    let mut hdr_end_cached: Option<usize> = None;

    // Streaming decode state.
    let mut raw_body_consumed: usize = 0;
    let mut decoded_body_len: usize = 0;
    let mut sse_buf: Vec<u8> = Vec::new();
    let mut chunked_done = false;
    let mut saw_done_event = false;
    let mut last_http_status: u16 = 0;
    let mut body_is_chunked = false;
    let mut sse_event_count: usize = 0;
    let mut last_sse_type: String = String::new();
    let mut last_sse_preview: String = String::new();
    let mut logged_http_sent = false;
    let mut logged_hdr_parsed = false;
    let mut logged_first_body = false;
    let mut logged_first_event = false;

    loop {
        for ev in events.drain(1024) {
            match ev {
                TlsEvent::Opened { handle } => {
                    last_activity = Instant::now();
                    tls_handle = Some(handle);
                }
                TlsEvent::Connected { handle } => {
                    last_activity = Instant::now();
                    if tls_handle != Some(handle) {
                        continue;
                    }
                    if !http_sent {
                        let req = build_http_request(HttpRequestSpec {
                            method: HttpRequestMethod::Post,
                            host: parsed.host.as_str(),
                            path: parsed.path.as_str(),
                            connection: HttpConnectionMode::Close,
                            accept: "text/event-stream",
                            accept_encoding_identity: true,
                            content_type: Some("application/json"),
                            body: Some(body_json),
                            auth_bearer: auth_token,
                        });
                        let _ = cmds.push(TlsCommand::Send {
                            handle,
                            data: req.into_bytes(),
                        });
                        http_sent = true;
                        if !logged_http_sent {
                            crate::log!(
                                "vhttps-sse: request-sent host={} dev={} timeout_ms={} body_len={}\n",
                                parsed.host,
                                dev_idx,
                                timeout_ms,
                                body_json.len(),
                            );
                            logged_http_sent = true;
                        }
                    }
                }
                TlsEvent::Data { handle, data } => {
                    last_activity = Instant::now();
                    if tls_handle != Some(handle) {
                        continue;
                    }
                    if data.is_empty() {
                        continue;
                    }

                    let room = capture_cap.saturating_sub(plaintext.len());
                    if room == 0 {
                        if let Some(h) = tls_handle {
                            let _ = cmds.push(TlsCommand::Close { handle: h });
                        }
                        return Err(FetchError::ResponseTooLarge);
                    }
                    let take = data.len().min(room);
                    plaintext.extend_from_slice(&data[..take]);
                    if take < data.len() {
                        if let Some(h) = tls_handle {
                            let _ = cmds.push(TlsCommand::Close { handle: h });
                        }
                        return Err(FetchError::ResponseTooLarge);
                    }

                    // Find headers once.
                    let hdr_end = match hdr_end_cached {
                        Some(v) => v,
                        None => {
                            let v = find_http_header_end(&plaintext);
                            if let Some(v) = v {
                                hdr_end_cached = Some(v);
                            }
                            v.unwrap_or(0)
                        }
                    };
                    if hdr_end == 0 {
                        continue;
                    }

                    let headers = &plaintext[..hdr_end];
                    let status = parse_http_status(&plaintext).unwrap_or(0);
                    last_http_status = status;
                    if status != 200 {
                        // Let existing error-body logging handle details.
                        if let Some(h) = tls_handle {
                            let _ = cmds.push(TlsCommand::Close { handle: h });
                        }
                        return Err(FetchError::Http(status));
                    }

                    let is_chunked =
                        header_contains_token(headers, b"transfer-encoding", b"chunked");
                    body_is_chunked = is_chunked;
                    if !logged_hdr_parsed {
                        crate::log!(
                            "vhttps-sse: headers host={} dev={} status={} chunked={} hdr_bytes={} plain_bytes={}\n",
                            parsed.host,
                            dev_idx,
                            status,
                            is_chunked,
                            hdr_end,
                            plaintext.len(),
                        );
                        logged_hdr_parsed = true;
                    }
                    let body_raw = &plaintext[hdr_end..];

                    if is_chunked {
                        // Incremental chunked decode.
                        while !chunked_done {
                            // Need size line.
                            let rem = &body_raw[raw_body_consumed..];
                            let Some(line_end) = rem.windows(2).position(|w| w == b"\r\n") else {
                                break;
                            };
                            let line = &rem[..line_end];
                            // Strip extensions.
                            let line = line.split(|b| *b == b';').next().unwrap_or(line);
                            let Ok(line_str) = core::str::from_utf8(line) else {
                                return Err(FetchError::Http(0));
                            };
                            let Ok(size) = usize::from_str_radix(line_str.trim(), 16) else {
                                return Err(FetchError::Http(0));
                            };
                            let after_line = raw_body_consumed + line_end + 2;
                            if size == 0 {
                                // Need terminating CRLF after 0 size and possible trailers; best-effort done.
                                chunked_done = true;
                                break;
                            }
                            if after_line + size + 2 > body_raw.len() {
                                break;
                            }
                            let chunk = &body_raw[after_line..after_line + size];
                            decoded_body_len = decoded_body_len.saturating_add(chunk.len());
                            if decoded_body_len > max_bytes {
                                return Err(FetchError::ResponseTooLarge);
                            }
                            sse_buf.extend_from_slice(chunk);
                            if !logged_first_body {
                                crate::log!(
                                    "vhttps-sse: first-body host={} dev={} decoded={} raw_consumed={}\n",
                                    parsed.host,
                                    dev_idx,
                                    decoded_body_len,
                                    raw_body_consumed,
                                );
                                logged_first_body = true;
                            }
                            raw_body_consumed = after_line + size + 2;
                        }
                    } else {
                        // Non-chunked: treat raw body bytes as decoded.
                        let new = &body_raw[raw_body_consumed..];
                        if !new.is_empty() {
                            decoded_body_len = decoded_body_len.saturating_add(new.len());
                            if decoded_body_len > max_bytes {
                                return Err(FetchError::ResponseTooLarge);
                            }
                            sse_buf.extend_from_slice(new);
                            if !logged_first_body {
                                crate::log!(
                                    "vhttps-sse: first-body host={} dev={} decoded={} raw_consumed={}\n",
                                    parsed.host,
                                    dev_idx,
                                    decoded_body_len,
                                    raw_body_consumed,
                                );
                                logged_first_body = true;
                            }
                            raw_body_consumed = body_raw.len();
                        }
                    }

                    // SSE parse: emit complete events as they arrive.
                    loop {
                        let delim = if let Some(p) = sse_buf.windows(2).position(|w| w == b"\n\n") {
                            Some((p, 2))
                        } else {
                            sse_buf
                                .windows(4)
                                .position(|w| w == b"\r\n\r\n")
                                .map(|p| (p, 4))
                        };
                        let Some((pos, dlen)) = delim else { break };
                        let block = sse_buf.drain(..pos + dlen).collect::<Vec<u8>>();
                        // Strip delimiter
                        let mut block = block;
                        if block.len() >= dlen {
                            block.truncate(block.len() - dlen);
                        }
                        if block.is_empty() {
                            continue;
                        }
                        let Ok(text) = core::str::from_utf8(block.as_slice()) else {
                            continue;
                        };
                        let mut data_out = String::new();
                        for line in text.lines() {
                            let line = line.trim_end_matches('\r');
                            if let Some(rest) = line.strip_prefix("data:") {
                                let mut rest = rest;
                                if rest.starts_with(' ') {
                                    rest = &rest[1..];
                                }
                                if !data_out.is_empty() {
                                    data_out.push('\n');
                                }
                                data_out.push_str(rest);
                            }
                        }
                        if data_out == "[DONE]" {
                            saw_done_event = true;
                            break;
                        }
                        if !data_out.is_empty() {
                            sse_event_count = sse_event_count.saturating_add(1);
                            last_sse_type.clear();
                            if let Some(t) = sse_json_type_hint(data_out.as_str()) {
                                last_sse_type.push_str(t);
                            }
                            set_preview(&mut last_sse_preview, data_out.as_str(), 96);
                            if !logged_first_event {
                                crate::log!(
                                    "vhttps-sse: first-event host={} dev={} type={} preview={}\n",
                                    parsed.host,
                                    dev_idx,
                                    if last_sse_type.is_empty() {
                                        "-"
                                    } else {
                                        last_sse_type.as_str()
                                    },
                                    if last_sse_preview.is_empty() {
                                        "-"
                                    } else {
                                        last_sse_preview.as_str()
                                    },
                                );
                                logged_first_event = true;
                            }
                            handler.on_data(data_out.as_str());
                        }
                    }

                    if saw_done_event {
                        if let Some(h) = tls_handle {
                            let _ = cmds.push(TlsCommand::Close { handle: h });
                        }
                        return Ok(());
                    }
                }
                TlsEvent::Closed { handle } => {
                    if tls_handle != Some(handle) {
                        continue;
                    }
                    crate::log!(
                        "vhttps-sse: closed host={} dev={} status={} hdr={} chunked={} raw={} decoded={} sse_buf={} events={} done={} last_type={} last_preview={}\n",
                        parsed.host,
                        dev_idx,
                        last_http_status,
                        hdr_end_cached.is_some(),
                        body_is_chunked,
                        raw_body_consumed,
                        decoded_body_len,
                        sse_buf.len(),
                        sse_event_count,
                        saw_done_event,
                        if last_sse_type.is_empty() {
                            "-"
                        } else {
                            last_sse_type.as_str()
                        },
                        if last_sse_preview.is_empty() {
                            "-"
                        } else {
                            last_sse_preview.as_str()
                        },
                    );
                    // Connection closed; treat as end of stream.
                    return Ok(());
                }
                TlsEvent::TlsError { .. } => {
                    if let Some(h) = tls_handle {
                        let _ = cmds.push(TlsCommand::Close { handle: h });
                    }
                    return Err(FetchError::Tls);
                }
                _ => {}
            }
        }

        if !sent_connect {
            let _ = cmds.push(TlsCommand::OpenTcpConnect {
                remote: vnet::EndpointV4 {
                    addr: ip,
                    port: parsed.port,
                },
                server_name,
                cfg: cfg.clone(),
                roots: roots.clone(),
                timeouts: crate::net::tls_socket::TlsTimeouts {
                    connect_ms: (timeout_ms / 4).max(5_000),
                    tls_ms: (timeout_ms / 4).max(5_000),
                    idle_ms: timeout_ms,
                },
            });
            sent_connect = true;
        }

        let now = Instant::now();
        if tls_handle.is_none() && now >= connect_deadline {
            return Err(FetchError::ConnectTimeout);
        }
        if tls_handle.is_some() && !http_sent && now >= tls_deadline {
            return Err(FetchError::TlsTimeout);
        }
        if http_sent {
            let idle_deadline = last_activity + EmbassyDuration::from_millis(timeout_ms as u64);
            if now >= idle_deadline {
                crate::log!(
                    "vhttps-sse: body-timeout host={} dev={} status={} hdr={} chunked={} raw={} decoded={} sse_buf={} events={} done={} idle_ms={} last_type={} last_preview={}\n",
                    parsed.host,
                    dev_idx,
                    last_http_status,
                    hdr_end_cached.is_some(),
                    body_is_chunked,
                    raw_body_consumed,
                    decoded_body_len,
                    sse_buf.len(),
                    sse_event_count,
                    saw_done_event,
                    timeout_ms,
                    if last_sse_type.is_empty() {
                        "-"
                    } else {
                        last_sse_type.as_str()
                    },
                    if last_sse_preview.is_empty() {
                        "-"
                    } else {
                        last_sse_preview.as_str()
                    },
                );
                return Err(FetchError::BodyTimeout);
            }
        }
        if now >= total_deadline {
            let err = if tls_handle.is_none() {
                FetchError::ConnectTimeout
            } else if !http_sent {
                FetchError::TlsTimeout
            } else {
                FetchError::BodyTimeout
            };
            if matches!(err, FetchError::BodyTimeout) {
                crate::log!(
                    "vhttps-sse: total-timeout(body) host={} dev={} status={} hdr={} chunked={} raw={} decoded={} sse_buf={} events={} done={} total_ms={} last_type={} last_preview={}\n",
                    parsed.host,
                    dev_idx,
                    last_http_status,
                    hdr_end_cached.is_some(),
                    body_is_chunked,
                    raw_body_consumed,
                    decoded_body_len,
                    sse_buf.len(),
                    sse_event_count,
                    saw_done_event,
                    timeout_ms,
                    if last_sse_type.is_empty() {
                        "-"
                    } else {
                        last_sse_type.as_str()
                    },
                    if last_sse_preview.is_empty() {
                        "-"
                    } else {
                        last_sse_preview.as_str()
                    },
                );
            }
            return Err(err);
        }

        Timer::after(EmbassyDuration::from_millis(2)).await;
    }
}

async fn fetch_on_device_keepalive(
    parsed: &ParsedHttpsUrl,
    dev_idx: usize,
    timeout_ms: u32,
    max_bytes: usize,
    mut progress: Option<&mut dyn FetchProgress>,
) -> Result<Vec<u8>, FetchError> {
    let want_done_log = progress.is_some();
    let request = build_http_request(HttpRequestSpec {
        method: HttpRequestMethod::Get,
        host: parsed.host.as_str(),
        path: parsed.path.as_str(),
        connection: HttpConnectionMode::KeepAlive,
        accept: "*/*",
        accept_encoding_identity: true,
        content_type: None,
        body: None,
        auth_bearer: None,
    });
    fetch_keepalive_bytes(
        parsed,
        dev_idx,
        timeout_ms,
        max_bytes,
        KeepAliveByteFetch {
            log_prefix: "vhttps-ka",
            request,
            request_path: Some(parsed.path.clone()),
            done_log: want_done_log,
            log_close_details: true,
            progress: progress.take(),
        },
    )
    .await
}

async fn fetch_on_device_keepalive_post_json(
    parsed: &ParsedHttpsUrl,
    dev_idx: usize,
    timeout_ms: u32,
    max_bytes: usize,
    body_json: &str,
    auth_token: Option<&str>,
) -> Result<Vec<u8>, FetchError> {
    let request = build_http_request(HttpRequestSpec {
        method: HttpRequestMethod::Post,
        host: parsed.host.as_str(),
        path: parsed.path.as_str(),
        connection: HttpConnectionMode::KeepAlive,
        accept: "*/*",
        accept_encoding_identity: false,
        content_type: Some("application/json"),
        body: Some(body_json),
        auth_bearer: auth_token,
    });
    fetch_keepalive_bytes(
        parsed,
        dev_idx,
        timeout_ms,
        max_bytes,
        KeepAliveByteFetch {
            log_prefix: "vhttps-ka-post",
            request,
            request_path: None,
            done_log: true,
            log_close_details: false,
            progress: None,
        },
    )
    .await
}

#[derive(Debug)]
enum FetchToFileError {
    Code(i32),
    Redirect { status: u16, url: String },
}

async fn fetch_on_device_to_file_keepalive(
    parsed: &ParsedHttpsUrl,
    dev_idx: usize,
    timeout_ms: u32,
    max_bytes: usize,
    disk: crate::disc::block::DeviceHandle,
    tmp_path: &str,
) -> Result<(), FetchToFileError> {
    crate::log!(
        "vhttps-ka-file: start host={} dev={} path={}\n",
        parsed.host,
        dev_idx,
        parsed.path
    );
    let KeepAliveReady {
        conn,
        handle,
        reused: _reused,
        deadline,
        last_activity: _last_activity,
    } = keepalive_prepare_ready(parsed, dev_idx, timeout_ms, "vhttps-ka-file")
        .await
        .map_err(fetch_error_to_code)
        .map_err(FetchToFileError::Code)?;
    crate::log!(
        "vhttps-ka-file: request host={} dev={} handle={}\n",
        parsed.host,
        dev_idx,
        handle.0
    );

    // Send HTTP GET request. Use keep-alive; we will stop reading once body complete.
    let req = build_http_request(HttpRequestSpec {
        method: HttpRequestMethod::Get,
        host: parsed.host.as_str(),
        path: parsed.path.as_str(),
        connection: HttpConnectionMode::KeepAlive,
        accept: "*/*",
        accept_encoding_identity: true,
        content_type: None,
        body: None,
        auth_bearer: None,
    });
    let _ = conn.cmds.push(TlsCommand::Send {
        handle,
        data: req.into_bytes(),
    });

    // Response parsing/writing (mostly identical to the non-keepalive path).
    let mut header_buf: Vec<u8> = Vec::new();
    let mut header_done = false;
    let mut body_is_chunked = false;
    let mut chunked_raw_body: Vec<u8> = Vec::new();
    let chunked_capture_cap = max_bytes.saturating_add(64 * 1024);
    let mut body_expected = 0usize;
    let mut body_written = 0usize;
    let mut stream_handle: Option<u32> = None;

    loop {
        for ev in conn.events.drain(1024) {
            match ev {
                TlsEvent::Data { handle: h, data } => {
                    if h != handle || data.is_empty() {
                        continue;
                    }

                    if !header_done {
                        header_buf.extend_from_slice(&data);
                        if header_buf.len() > (64 * 1024) {
                            if let Some(sh) = stream_handle.take() {
                                let _ = crate::r::fs::trueosfs::file_write_abort_async(sh).await;
                            }
                            keepalive_release(conn);
                            return Err(FetchToFileError::Code(fetch_error_to_code(
                                FetchError::Http(0),
                            )));
                        }

                        if let Some(hdr_end) = find_http_header_end(&header_buf) {
                            let headers = &header_buf[..hdr_end];
                            let status = parse_http_status(headers).unwrap_or(0);
                            if status != 200 {
                                if is_redirect_status(status)
                                    && let Some(next) = redirect_url_from_location(parsed, headers)
                                {
                                    keepalive_release(conn);
                                    return Err(FetchToFileError::Redirect { status, url: next });
                                }
                                keepalive_release(conn);
                                return Err(FetchToFileError::Code(fetch_error_to_code(
                                    FetchError::Http(status),
                                )));
                            }

                            let Some(head) = parse_http_head(headers) else {
                                crate::log!(
                                    "vhttps-ka-file: head-parse-failed host={} dev={} hdr_bytes={}\n",
                                    parsed.host,
                                    dev_idx,
                                    headers.len()
                                );
                                keepalive_release(conn);
                                return Err(FetchToFileError::Code(fetch_error_to_code(
                                    FetchError::Http(0),
                                )));
                            };

                            body_expected = match head.body {
                                HttpBodyKind::ContentLength(v) => v,
                                HttpBodyKind::Chunked => {
                                    body_is_chunked = true;
                                    0
                                }
                            };
                            crate::log!(
                                "vhttps-ka-file: headers host={} dev={} status={} chunked={} expected={} hdr_bytes={} buffered={} handle={}\n",
                                parsed.host,
                                dev_idx,
                                status,
                                if body_is_chunked { 1 } else { 0 },
                                body_expected,
                                headers.len(),
                                header_buf.len().saturating_sub(hdr_end),
                                handle.0
                            );

                            if !body_is_chunked && body_expected > max_bytes {
                                keepalive_release(conn);
                                return Err(FetchToFileError::Code(fetch_error_to_code(
                                    FetchError::ResponseTooLarge,
                                )));
                            }

                            if !body_is_chunked {
                                let sh = match crate::r::fs::trueosfs::file_write_begin_async(
                                    disk,
                                    tmp_path,
                                    body_expected as u64,
                                )
                                .await
                                .map_err(block_error_to_code)
                                .map_err(FetchToFileError::Code)?
                                {
                                    Some(h) => h,
                                    None => {
                                        keepalive_release(conn);
                                        return Err(FetchToFileError::Code(FS_ERR_NO_SPACE));
                                    }
                                };
                                stream_handle = Some(sh);
                            }

                            header_done = true;

                            let body_start = hdr_end;
                            if header_buf.len() > body_start {
                                let part = &header_buf[body_start..];
                                if body_is_chunked {
                                    let room =
                                        chunked_capture_cap.saturating_sub(chunked_raw_body.len());
                                    let take = part.len().min(room);
                                    chunked_raw_body.extend_from_slice(&part[..take]);
                                } else if let Some(sh) = stream_handle {
                                    let rem = body_expected.saturating_sub(body_written);
                                    let take = part.len().min(rem);
                                    if take > 0 {
                                        crate::r::fs::trueosfs::file_write_chunk_async(
                                            sh,
                                            &part[..take],
                                        )
                                        .await
                                        .map_err(block_error_to_code)
                                        .map_err(FetchToFileError::Code)?;
                                        body_written = body_written.saturating_add(take);
                                    }
                                }
                            }
                            header_buf.clear();

                            if !body_is_chunked && body_written >= body_expected {
                                crate::log!(
                                    "vhttps-ka-file: body-complete host={} dev={} written={} expected={} handle={}\n",
                                    parsed.host,
                                    dev_idx,
                                    body_written,
                                    body_expected,
                                    handle.0
                                );
                                if let Some(sh) = stream_handle.take() {
                                    crate::r::fs::trueosfs::file_write_finish_async(sh)
                                        .await
                                        .map_err(block_error_to_code)
                                        .map_err(FetchToFileError::Code)?;
                                }
                                let mut st = conn.state.lock();
                                st.last_used = Instant::now();
                                keepalive_release(conn);
                                return Ok(());
                            }
                        }
                    } else if body_is_chunked {
                        let room = chunked_capture_cap.saturating_sub(chunked_raw_body.len());
                        let take = data.len().min(room);
                        chunked_raw_body.extend_from_slice(&data[..take]);
                        if let Some(decoded) = decode_http_chunked(chunked_raw_body.as_slice()) {
                            if decoded.len() > max_bytes {
                                keepalive_release(conn);
                                return Err(FetchToFileError::Code(fetch_error_to_code(
                                    FetchError::ResponseTooLarge,
                                )));
                            }
                            crate::log!(
                                "vhttps-ka-file: chunked-complete host={} dev={} decoded={} raw={} handle={}\n",
                                parsed.host,
                                dev_idx,
                                decoded.len(),
                                chunked_raw_body.len(),
                                handle.0
                            );
                            write_body_to_tmp_file(disk, tmp_path, decoded.as_slice())
                                .await
                                .map_err(FetchToFileError::Code)?;
                            let mut st = conn.state.lock();
                            st.last_used = Instant::now();
                            keepalive_release(conn);
                            return Ok(());
                        }
                    } else if let Some(sh) = stream_handle {
                        let rem = body_expected.saturating_sub(body_written);
                        let take = data.len().min(rem);
                        if take > 0 {
                            crate::r::fs::trueosfs::file_write_chunk_async(sh, &data[..take])
                                .await
                                .map_err(block_error_to_code)
                                .map_err(FetchToFileError::Code)?;
                            body_written = body_written.saturating_add(take);
                        }
                        if body_written >= body_expected {
                            crate::log!(
                                "vhttps-ka-file: body-complete host={} dev={} written={} expected={} handle={}\n",
                                parsed.host,
                                dev_idx,
                                body_written,
                                body_expected,
                                handle.0
                            );
                            crate::r::fs::trueosfs::file_write_finish_async(sh)
                                .await
                                .map_err(block_error_to_code)
                                .map_err(FetchToFileError::Code)?;
                            let mut st = conn.state.lock();
                            st.last_used = Instant::now();
                            keepalive_release(conn);
                            return Ok(());
                        }
                    }
                }
                TlsEvent::Closed { handle: h } => {
                    if h == handle {
                        // Server closed; reset pool state and fail.
                        crate::log!(
                            "vhttps-ka-file: closed host={} dev={} header_done={} chunked={} written={} expected={} chunked_raw={} handle={}\n",
                            parsed.host,
                            dev_idx,
                            if header_done { 1 } else { 0 },
                            if body_is_chunked { 1 } else { 0 },
                            body_written,
                            body_expected,
                            chunked_raw_body.len(),
                            handle.0
                        );
                        {
                            let mut st = conn.state.lock();
                            st.handle = None;
                            st.connected = false;
                        }
                        if let Some(sh) = stream_handle.take() {
                            let _ = crate::r::fs::trueosfs::file_write_abort_async(sh).await;
                        }
                        keepalive_release(conn);
                        return Err(FetchToFileError::Code(fetch_error_to_code(
                            FetchError::BodyTimeout,
                        )));
                    }
                }
                TlsEvent::TlsError { .. } => {
                    {
                        let mut st = conn.state.lock();
                        st.handle = None;
                        st.connected = false;
                    }
                    if let Some(sh) = stream_handle.take() {
                        let _ = crate::r::fs::trueosfs::file_write_abort_async(sh).await;
                    }
                    keepalive_release(conn);
                    return Err(FetchToFileError::Code(fetch_error_to_code(FetchError::Tls)));
                }
                _ => {}
            }
        }

        if Instant::now() >= deadline {
            crate::log!(
                "vhttps-ka-file: timeout host={} dev={} header_done={} chunked={} written={} expected={} chunked_raw={} header_buf={} handle={}\n",
                parsed.host,
                dev_idx,
                if header_done { 1 } else { 0 },
                if body_is_chunked { 1 } else { 0 },
                body_written,
                body_expected,
                chunked_raw_body.len(),
                header_buf.len(),
                handle.0
            );
            if let Some(sh) = stream_handle.take() {
                let _ = crate::r::fs::trueosfs::file_write_abort_async(sh).await;
            }
            keepalive_release(conn);
            return Err(FetchToFileError::Code(fetch_error_to_code(FetchError::BodyTimeout)));
        }
        Timer::after(EmbassyDuration::from_millis(2)).await;
    }
}

async fn fetch_on_device_to_file(
    parsed: &ParsedHttpsUrl,
    dev_idx: usize,
    timeout_ms: u32,
    max_bytes: usize,
    disk: crate::disc::block::DeviceHandle,
    tmp_path: &str,
) -> Result<(), FetchToFileError> {
    let t0 = Instant::now();

    let ip = match dns::resolve_ipv4_for_device(
        dev_idx,
        parsed.host.as_str(),
        DnsConfig::for_device(dev_idx),
    )
    .await
    {
        Ok(ip) => ip,
        Err(dns::DnsError::Timeout) => {
            return Err(FetchToFileError::Code(fetch_error_to_code(FetchError::DnsTimeout)));
        }
        Err(_) => {
            return Err(FetchToFileError::Code(fetch_error_to_code(FetchError::DnsFailed)));
        }
    };

    let t_dns = Instant::now();

    let seq = VHTTPS_SEQ.fetch_add(1, Ordering::Relaxed);
    let selector = if let Some((bus, slot, func)) = crate::net::bdf_at(dev_idx) {
        format!("{:02x}:{:02x}.{}", bus, slot, func)
    } else if let Some((vid, pid)) = crate::net::pci_id_at(dev_idx) {
        format!("{:04x}:{:04x}", vid, pid)
    } else {
        format!("{}", dev_idx)
    };
    let owner = leak_str(format!("vhttps-file-{}@{}", seq, selector));
    let cmds_name = leak_str(format!("{}-tls-cmd", owner));
    let evts_name = leak_str(format!("{}-tls-evt", owner));

    let cmds = Queue::new_leaked(cmds_name, 256);
    let events = Queue::new_leaked(evts_name, 4096);
    register_tls_app_queues(owner, cmds, events);

    let roots = TlsRoots::mozilla();
    let cfg = TlsClientConfig::new().with_alpn_protocols(&[b"http/1.1"]);
    let server_name = leak_str(parsed.host.clone());

    let deadline = Instant::now() + EmbassyDuration::from_millis(timeout_ms as u64);
    let mut tls_handle: Option<vnet::NetHandle> = None;
    let mut sent_connect = false;
    let mut http_sent = false;

    let mut t_open_sent: Option<Instant> = None;
    let mut t_tcp_opened: Option<Instant> = None;
    let mut t_tls_connected: Option<Instant> = None;
    let mut t_header_done: Option<Instant> = None;
    let mut t_write_begin: Option<Instant> = None;
    let mut t_write_done: Option<Instant> = None;

    let mut header_buf: Vec<u8> = Vec::new();
    let mut header_done = false;
    let mut body_is_chunked = false;
    let mut chunked_raw_body: Vec<u8> = Vec::new();
    let chunked_capture_cap = max_bytes.saturating_add(64 * 1024);
    let mut body_expected = 0usize;
    let mut body_written = 0usize;
    let mut stream_handle: Option<u32> = None;

    let mut last_http_status: u16 = 0;

    #[inline]
    fn ms_since(a: Instant, b: Instant) -> u64 {
        b.saturating_duration_since(a).as_millis()
    }

    #[inline]
    fn log_vhttps_file_timing(
        host: &str,
        dev_idx: usize,
        status: u16,
        t0: Instant,
        t_dns: Instant,
        t_open_sent: Option<Instant>,
        t_tcp_opened: Option<Instant>,
        t_tls_connected: Option<Instant>,
        t_header_done: Option<Instant>,
        t_write_begin: Option<Instant>,
        t_write_done: Option<Instant>,
        rc: i32,
    ) {
        // Successful fetches are already summarized by the higher-level cache log.
        // Keep detailed timing only for failures (or when explicitly enabled).
        if rc == 0 && !crate::logflag::VHTTPS_VERBOSE {
            return;
        }
        let t_end = Instant::now();
        let dns_ms = ms_since(t0, t_dns);
        let tcp_ms = match (t_open_sent, t_tcp_opened) {
            (Some(a), Some(b)) => ms_since(a, b),
            _ => 0,
        };
        let tls_ms = match (t_tcp_opened, t_tls_connected) {
            (Some(a), Some(b)) => ms_since(a, b),
            _ => 0,
        };
        let hdr_ms = match (t_tls_connected, t_header_done) {
            (Some(a), Some(b)) => ms_since(a, b),
            _ => 0,
        };
        let write_ms = match (t_write_begin, t_write_done) {
            (Some(a), Some(b)) => ms_since(a, b),
            _ => 0,
        };
        let total_ms = ms_since(t0, t_end);
        crate::log!(
            "vhttps-file: timing host={} dev={} status={} rc={} dns={}ms tcp={}ms tls={}ms hdr={}ms write={}ms total={}ms\n",
            host,
            dev_idx,
            status,
            rc,
            dns_ms,
            tcp_ms,
            tls_ms,
            hdr_ms,
            write_ms,
            total_ms,
        );
    }

    loop {
        for ev in events.drain(1024) {
            match ev {
                TlsEvent::Opened { handle } => {
                    tls_handle = Some(handle);
                    if t_tcp_opened.is_none() {
                        t_tcp_opened = Some(Instant::now());
                    }
                }
                TlsEvent::Connected { handle } => {
                    if tls_handle != Some(handle) {
                        continue;
                    }
                    if t_tls_connected.is_none() {
                        t_tls_connected = Some(Instant::now());
                    }
                    if !http_sent {
                        let req = build_http_request(HttpRequestSpec {
                            method: HttpRequestMethod::Get,
                            host: parsed.host.as_str(),
                            path: parsed.path.as_str(),
                            connection: HttpConnectionMode::Close,
                            accept: "*/*",
                            accept_encoding_identity: false,
                            content_type: None,
                            body: None,
                            auth_bearer: None,
                        });
                        let _ = cmds.push(TlsCommand::Send {
                            handle,
                            data: req.into_bytes(),
                        });
                        http_sent = true;
                    }
                }
                TlsEvent::Data { handle, data } => {
                    if tls_handle != Some(handle) || data.is_empty() {
                        continue;
                    }

                    if !header_done {
                        header_buf.extend_from_slice(&data);
                        if header_buf.len() > (64 * 1024) {
                            if let Some(h) = tls_handle {
                                let _ = cmds.push(TlsCommand::Close { handle: h });
                            }
                            if let Some(sh) = stream_handle.take() {
                                let _ = crate::r::fs::trueosfs::file_write_abort_async(sh).await;
                            }
                            return Err(FetchToFileError::Code(fetch_error_to_code(
                                FetchError::Http(0),
                            )));
                        }

                        if let Some(hdr_end) = find_http_header_end(&header_buf) {
                            let headers = &header_buf[..hdr_end];
                            let status = parse_http_status(headers).unwrap_or(0);
                            last_http_status = status;
                            if status != 200 {
                                if is_redirect_status(status)
                                    && let Some(next) = redirect_url_from_location(parsed, headers)
                                {
                                    if let Some(h) = tls_handle {
                                        let _ = cmds.push(TlsCommand::Close { handle: h });
                                    }
                                    if let Some(sh) = stream_handle.take() {
                                        let _ = crate::r::fs::trueosfs::file_write_abort_async(sh)
                                            .await;
                                    }
                                    log_vhttps_file_timing(
                                        parsed.host.as_str(),
                                        dev_idx,
                                        last_http_status,
                                        t0,
                                        t_dns,
                                        t_open_sent,
                                        t_tcp_opened,
                                        t_tls_connected,
                                        t_header_done,
                                        t_write_begin,
                                        t_write_done,
                                        fetch_error_to_code(FetchError::Http(status)),
                                    );
                                    return Err(FetchToFileError::Redirect { status, url: next });
                                }

                                if let Some(h) = tls_handle {
                                    let _ = cmds.push(TlsCommand::Close { handle: h });
                                }
                                if let Some(sh) = stream_handle.take() {
                                    let _ =
                                        crate::r::fs::trueosfs::file_write_abort_async(sh).await;
                                }
                                let rc = fetch_error_to_code(FetchError::Http(status));
                                log_vhttps_file_timing(
                                    parsed.host.as_str(),
                                    dev_idx,
                                    last_http_status,
                                    t0,
                                    t_dns,
                                    t_open_sent,
                                    t_tcp_opened,
                                    t_tls_connected,
                                    t_header_done,
                                    t_write_begin,
                                    t_write_done,
                                    rc,
                                );
                                return Err(FetchToFileError::Code(rc));
                            }

                            let Some(head) = parse_http_head(headers) else {
                                if let Some(h) = tls_handle {
                                    let _ = cmds.push(TlsCommand::Close { handle: h });
                                }
                                if let Some(sh) = stream_handle.take() {
                                    let _ =
                                        crate::r::fs::trueosfs::file_write_abort_async(sh).await;
                                }
                                crate::log!(
                                    "vhttps-file: invalid-http-head host={} hdr_bytes={}\n",
                                    parsed.host,
                                    header_buf.len()
                                );
                                let rc = fetch_error_to_code(FetchError::Http(0));
                                log_vhttps_file_timing(
                                    parsed.host.as_str(),
                                    dev_idx,
                                    last_http_status,
                                    t0,
                                    t_dns,
                                    t_open_sent,
                                    t_tcp_opened,
                                    t_tls_connected,
                                    t_header_done,
                                    t_write_begin,
                                    t_write_done,
                                    rc,
                                );
                                return Err(FetchToFileError::Code(rc));
                            };
                            if crate::logflag::VHTTPS_VERBOSE {
                                log_http_head("vhttps-file: head", parsed.host.as_str(), head);
                            }

                            body_expected = match head.body {
                                HttpBodyKind::ContentLength(v) => v,
                                HttpBodyKind::Chunked => {
                                    body_is_chunked = true;
                                    0
                                }
                            };
                            if !body_is_chunked && body_expected > max_bytes {
                                if let Some(h) = tls_handle {
                                    let _ = cmds.push(TlsCommand::Close { handle: h });
                                }
                                if let Some(sh) = stream_handle.take() {
                                    let _ =
                                        crate::r::fs::trueosfs::file_write_abort_async(sh).await;
                                }
                                let rc = fetch_error_to_code(FetchError::ResponseTooLarge);
                                log_vhttps_file_timing(
                                    parsed.host.as_str(),
                                    dev_idx,
                                    last_http_status,
                                    t0,
                                    t_dns,
                                    t_open_sent,
                                    t_tcp_opened,
                                    t_tls_connected,
                                    t_header_done,
                                    t_write_begin,
                                    t_write_done,
                                    rc,
                                );
                                return Err(FetchToFileError::Code(rc));
                            }

                            if !body_is_chunked {
                                if t_write_begin.is_none() {
                                    t_write_begin = Some(Instant::now());
                                }
                                let sh = match crate::r::fs::trueosfs::file_write_begin_async(
                                    disk,
                                    tmp_path,
                                    body_expected as u64,
                                )
                                .await
                                .map_err(block_error_to_code)
                                .map_err(FetchToFileError::Code)?
                                {
                                    Some(h) => h,
                                    None => {
                                        log_vhttps_file_timing(
                                            parsed.host.as_str(),
                                            dev_idx,
                                            last_http_status,
                                            t0,
                                            t_dns,
                                            t_open_sent,
                                            t_tcp_opened,
                                            t_tls_connected,
                                            t_header_done,
                                            t_write_begin,
                                            t_write_done,
                                            FS_ERR_NO_SPACE,
                                        );
                                        return Err(FetchToFileError::Code(FS_ERR_NO_SPACE));
                                    }
                                };
                                stream_handle = Some(sh);
                            }
                            header_done = true;
                            if t_header_done.is_none() {
                                t_header_done = Some(Instant::now());
                            }

                            let body_start = hdr_end;
                            if header_buf.len() > body_start {
                                let part = &header_buf[body_start..];
                                if body_is_chunked {
                                    let room =
                                        chunked_capture_cap.saturating_sub(chunked_raw_body.len());
                                    if room == 0 {
                                        if let Some(h) = tls_handle {
                                            let _ = cmds.push(TlsCommand::Close { handle: h });
                                        }
                                        let rc = fetch_error_to_code(FetchError::ResponseTooLarge);
                                        log_vhttps_file_timing(
                                            parsed.host.as_str(),
                                            dev_idx,
                                            last_http_status,
                                            t0,
                                            t_dns,
                                            t_open_sent,
                                            t_tcp_opened,
                                            t_tls_connected,
                                            t_header_done,
                                            t_write_begin,
                                            t_write_done,
                                            rc,
                                        );
                                        return Err(FetchToFileError::Code(rc));
                                    }
                                    let take = part.len().min(room);
                                    chunked_raw_body.extend_from_slice(&part[..take]);
                                    if take < part.len() {
                                        if let Some(h) = tls_handle {
                                            let _ = cmds.push(TlsCommand::Close { handle: h });
                                        }
                                        let rc = fetch_error_to_code(FetchError::ResponseTooLarge);
                                        log_vhttps_file_timing(
                                            parsed.host.as_str(),
                                            dev_idx,
                                            last_http_status,
                                            t0,
                                            t_dns,
                                            t_open_sent,
                                            t_tcp_opened,
                                            t_tls_connected,
                                            t_header_done,
                                            t_write_begin,
                                            t_write_done,
                                            rc,
                                        );
                                        return Err(FetchToFileError::Code(rc));
                                    }
                                } else if let Some(sh) = stream_handle {
                                    let rem = body_expected.saturating_sub(body_written);
                                    let take = part.len().min(rem);
                                    if take > 0 {
                                        crate::r::fs::trueosfs::file_write_chunk_async(
                                            sh,
                                            &part[..take],
                                        )
                                        .await
                                        .map_err(block_error_to_code)
                                        .map_err(FetchToFileError::Code)?;
                                        body_written = body_written.saturating_add(take);
                                    }
                                }
                            }
                            header_buf.clear();

                            if !body_is_chunked && body_written >= body_expected {
                                if let Some(sh) = stream_handle.take() {
                                    crate::r::fs::trueosfs::file_write_finish_async(sh)
                                        .await
                                        .map_err(block_error_to_code)
                                        .map_err(FetchToFileError::Code)?;
                                    if t_write_done.is_none() {
                                        t_write_done = Some(Instant::now());
                                    }
                                }
                                if let Some(h) = tls_handle {
                                    let _ = cmds.push(TlsCommand::Close { handle: h });
                                }
                                log_vhttps_file_timing(
                                    parsed.host.as_str(),
                                    dev_idx,
                                    last_http_status,
                                    t0,
                                    t_dns,
                                    t_open_sent,
                                    t_tcp_opened,
                                    t_tls_connected,
                                    t_header_done,
                                    t_write_begin,
                                    t_write_done,
                                    0,
                                );
                                return Ok(());
                            }
                        }
                    } else if body_is_chunked {
                        let room = chunked_capture_cap.saturating_sub(chunked_raw_body.len());
                        if room == 0 {
                            if let Some(h) = tls_handle {
                                let _ = cmds.push(TlsCommand::Close { handle: h });
                            }
                            let rc = fetch_error_to_code(FetchError::ResponseTooLarge);
                            log_vhttps_file_timing(
                                parsed.host.as_str(),
                                dev_idx,
                                last_http_status,
                                t0,
                                t_dns,
                                t_open_sent,
                                t_tcp_opened,
                                t_tls_connected,
                                t_header_done,
                                t_write_begin,
                                t_write_done,
                                rc,
                            );
                            return Err(FetchToFileError::Code(rc));
                        }
                        let take = data.len().min(room);
                        chunked_raw_body.extend_from_slice(&data[..take]);
                        if take < data.len() {
                            if let Some(h) = tls_handle {
                                let _ = cmds.push(TlsCommand::Close { handle: h });
                            }
                            let rc = fetch_error_to_code(FetchError::ResponseTooLarge);
                            log_vhttps_file_timing(
                                parsed.host.as_str(),
                                dev_idx,
                                last_http_status,
                                t0,
                                t_dns,
                                t_open_sent,
                                t_tcp_opened,
                                t_tls_connected,
                                t_header_done,
                                t_write_begin,
                                t_write_done,
                                rc,
                            );
                            return Err(FetchToFileError::Code(rc));
                        }
                        if let Some(decoded) = decode_http_chunked(chunked_raw_body.as_slice()) {
                            if decoded.len() > max_bytes {
                                if let Some(h) = tls_handle {
                                    let _ = cmds.push(TlsCommand::Close { handle: h });
                                }
                                let rc = fetch_error_to_code(FetchError::ResponseTooLarge);
                                log_vhttps_file_timing(
                                    parsed.host.as_str(),
                                    dev_idx,
                                    last_http_status,
                                    t0,
                                    t_dns,
                                    t_open_sent,
                                    t_tcp_opened,
                                    t_tls_connected,
                                    t_header_done,
                                    t_write_begin,
                                    t_write_done,
                                    rc,
                                );
                                return Err(FetchToFileError::Code(rc));
                            }
                            if t_write_begin.is_none() {
                                t_write_begin = Some(Instant::now());
                            }
                            write_body_to_tmp_file(disk, tmp_path, decoded.as_slice())
                                .await
                                .map_err(FetchToFileError::Code)?;
                            if t_write_done.is_none() {
                                t_write_done = Some(Instant::now());
                            }
                            if let Some(h) = tls_handle {
                                let _ = cmds.push(TlsCommand::Close { handle: h });
                            }
                            log_vhttps_file_timing(
                                parsed.host.as_str(),
                                dev_idx,
                                last_http_status,
                                t0,
                                t_dns,
                                t_open_sent,
                                t_tcp_opened,
                                t_tls_connected,
                                t_header_done,
                                t_write_begin,
                                t_write_done,
                                0,
                            );
                            return Ok(());
                        }
                    } else if let Some(sh) = stream_handle {
                        let rem = body_expected.saturating_sub(body_written);
                        let take = data.len().min(rem);
                        if take > 0 {
                            crate::r::fs::trueosfs::file_write_chunk_async(sh, &data[..take])
                                .await
                                .map_err(block_error_to_code)
                                .map_err(FetchToFileError::Code)?;
                            body_written = body_written.saturating_add(take);
                        }
                        if body_written >= body_expected {
                            crate::r::fs::trueosfs::file_write_finish_async(sh)
                                .await
                                .map_err(block_error_to_code)
                                .map_err(FetchToFileError::Code)?;
                            if t_write_done.is_none() {
                                t_write_done = Some(Instant::now());
                            }
                            if let Some(h) = tls_handle {
                                let _ = cmds.push(TlsCommand::Close { handle: h });
                            }
                            log_vhttps_file_timing(
                                parsed.host.as_str(),
                                dev_idx,
                                last_http_status,
                                t0,
                                t_dns,
                                t_open_sent,
                                t_tcp_opened,
                                t_tls_connected,
                                t_header_done,
                                t_write_begin,
                                t_write_done,
                                0,
                            );
                            return Ok(());
                        }
                    }
                }
                TlsEvent::Closed { handle } => {
                    if tls_handle != Some(handle) {
                        continue;
                    }
                    if body_is_chunked && header_done {
                        let Some(decoded) = decode_http_chunked(chunked_raw_body.as_slice()) else {
                            if let Some(sh) = stream_handle.take() {
                                let _ = crate::r::fs::trueosfs::file_write_abort_async(sh).await;
                            }
                            let rc = fetch_error_to_code(FetchError::Http(0));
                            log_vhttps_file_timing(
                                parsed.host.as_str(),
                                dev_idx,
                                last_http_status,
                                t0,
                                t_dns,
                                t_open_sent,
                                t_tcp_opened,
                                t_tls_connected,
                                t_header_done,
                                t_write_begin,
                                t_write_done,
                                rc,
                            );
                            return Err(FetchToFileError::Code(rc));
                        };
                        if decoded.len() > max_bytes {
                            if let Some(sh) = stream_handle.take() {
                                let _ = crate::r::fs::trueosfs::file_write_abort_async(sh).await;
                            }
                            let rc = fetch_error_to_code(FetchError::ResponseTooLarge);
                            log_vhttps_file_timing(
                                parsed.host.as_str(),
                                dev_idx,
                                last_http_status,
                                t0,
                                t_dns,
                                t_open_sent,
                                t_tcp_opened,
                                t_tls_connected,
                                t_header_done,
                                t_write_begin,
                                t_write_done,
                                rc,
                            );
                            return Err(FetchToFileError::Code(rc));
                        }
                        if let Some(sh) = stream_handle.take() {
                            let _ = crate::r::fs::trueosfs::file_write_abort_async(sh).await;
                        }
                        if t_write_begin.is_none() {
                            t_write_begin = Some(Instant::now());
                        }
                        write_body_to_tmp_file(disk, tmp_path, decoded.as_slice())
                            .await
                            .map_err(FetchToFileError::Code)?;
                        if t_write_done.is_none() {
                            t_write_done = Some(Instant::now());
                        }
                        log_vhttps_file_timing(
                            parsed.host.as_str(),
                            dev_idx,
                            last_http_status,
                            t0,
                            t_dns,
                            t_open_sent,
                            t_tcp_opened,
                            t_tls_connected,
                            t_header_done,
                            t_write_begin,
                            t_write_done,
                            0,
                        );
                        return Ok(());
                    }
                    if let Some(sh) = stream_handle.take() {
                        let _ = crate::r::fs::trueosfs::file_write_abort_async(sh).await;
                    }
                    let rc = fetch_error_to_code(FetchError::BodyTimeout);
                    log_vhttps_file_timing(
                        parsed.host.as_str(),
                        dev_idx,
                        last_http_status,
                        t0,
                        t_dns,
                        t_open_sent,
                        t_tcp_opened,
                        t_tls_connected,
                        t_header_done,
                        t_write_begin,
                        t_write_done,
                        rc,
                    );
                    return Err(FetchToFileError::Code(rc));
                }
                TlsEvent::Error { .. } => {}
                TlsEvent::TlsError { .. } => {
                    if let Some(h) = tls_handle {
                        let _ = cmds.push(TlsCommand::Close { handle: h });
                    }
                    if let Some(sh) = stream_handle.take() {
                        let _ = crate::r::fs::trueosfs::file_write_abort_async(sh).await;
                    }
                    let rc = fetch_error_to_code(FetchError::Tls);
                    log_vhttps_file_timing(
                        parsed.host.as_str(),
                        dev_idx,
                        last_http_status,
                        t0,
                        t_dns,
                        t_open_sent,
                        t_tcp_opened,
                        t_tls_connected,
                        t_header_done,
                        t_write_begin,
                        t_write_done,
                        rc,
                    );
                    return Err(FetchToFileError::Code(rc));
                }
            }
        }

        if !sent_connect {
            let _ = cmds.push(TlsCommand::OpenTcpConnect {
                remote: vnet::EndpointV4 {
                    addr: ip,
                    port: parsed.port,
                },
                server_name,
                cfg: cfg.clone(),
                roots: roots.clone(),
                timeouts: crate::net::tls_socket::TlsTimeouts {
                    connect_ms: (timeout_ms / 4).max(5_000),
                    tls_ms: (timeout_ms / 4).max(5_000),
                    idle_ms: timeout_ms,
                },
            });
            if t_open_sent.is_none() {
                t_open_sent = Some(Instant::now());
            }
            sent_connect = true;
        }

        if Instant::now() >= deadline {
            if let Some(h) = tls_handle {
                let _ = cmds.push(TlsCommand::Close { handle: h });
            }
            if let Some(sh) = stream_handle.take() {
                let _ = crate::r::fs::trueosfs::file_write_abort_async(sh).await;
            }
            let rc = if tls_handle.is_none() {
                fetch_error_to_code(FetchError::ConnectTimeout)
            } else if !http_sent {
                fetch_error_to_code(FetchError::TlsTimeout)
            } else {
                fetch_error_to_code(FetchError::BodyTimeout)
            };
            log_vhttps_file_timing(
                parsed.host.as_str(),
                dev_idx,
                last_http_status,
                t0,
                t_dns,
                t_open_sent,
                t_tcp_opened,
                t_tls_connected,
                t_header_done,
                t_write_begin,
                t_write_done,
                rc,
            );
            return Err(FetchToFileError::Code(rc));
        }

        Timer::after(EmbassyDuration::from_millis(2)).await;
    }
}

/// Fetch an HTTPS URL and return the response body.
///
/// Notes:
/// - This is a minimal HTTP/1.1-over-TLS client intended for boot-time fetching.
/// - Binds the request to one resolved NIC for its full lifetime.
pub async fn fetch_https_body_async(
    url: &str,
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<Vec<u8>, FetchError> {
    fetch_https_body_with_profile_async(url, NetProfile::default(), timeout_ms, max_bytes).await
}

pub async fn fetch_https_body_with_profile_async(
    url: &str,
    profile: NetProfile,
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<Vec<u8>, FetchError> {
    let dev_idx = fetch_device_index(profile)?;

    const MAX_REDIRECTS: usize = 3;
    let mut current_url = String::from(url);

    for hop in 0..=MAX_REDIRECTS {
        let parsed = parse_https_url(current_url.as_str()).ok_or(FetchError::BadUrl)?;

        let res = if HttpsLimits::KEEPALIVE_ENABLE {
            fetch_on_device_keepalive(&parsed, dev_idx, timeout_ms, max_bytes, None).await
        } else {
            fetch_on_device(&parsed, dev_idx, timeout_ms, max_bytes, None, None, None).await
        };
        match res {
            Ok(v) => return Ok(v),
            Err(FetchError::Redirect { status, url }) => {
                if hop >= MAX_REDIRECTS {
                    return Err(FetchError::Http(status));
                }
                current_url = url;
                continue;
            }
            Err(e) => return Err(e),
        }
    }

    Err(FetchError::Http(0))
}

/// Fetch an HTTPS URL and return the response body, with progress updates.
///
/// Progress is based on received body bytes (after headers). If Content-Length
/// is present, `total` will be provided.
pub async fn fetch_https_body_progress_async(
    url: &str,
    timeout_ms: u32,
    max_bytes: usize,
    progress: &mut dyn FetchProgress,
) -> Result<Vec<u8>, FetchError> {
    fetch_https_body_progress_with_profile_async(
        url,
        NetProfile::default(),
        timeout_ms,
        max_bytes,
        progress,
    )
    .await
}

pub async fn fetch_https_body_progress_with_profile_async(
    url: &str,
    profile: NetProfile,
    timeout_ms: u32,
    max_bytes: usize,
    progress: &mut dyn FetchProgress,
) -> Result<Vec<u8>, FetchError> {
    let dev_idx = fetch_device_index(profile)?;

    const MAX_REDIRECTS: usize = 3;
    let mut current_url = String::from(url);

    for hop in 0..=MAX_REDIRECTS {
        let parsed = parse_https_url(current_url.as_str()).ok_or(FetchError::BadUrl)?;

        let res = if HttpsLimits::KEEPALIVE_ENABLE {
            fetch_on_device_keepalive(&parsed, dev_idx, timeout_ms, max_bytes, Some(progress)).await
        } else {
            fetch_on_device(&parsed, dev_idx, timeout_ms, max_bytes, None, None, Some(progress))
                .await
        };
        match res {
            Ok(v) => return Ok(v),
            Err(FetchError::Redirect { status, url }) => {
                if hop >= MAX_REDIRECTS {
                    return Err(FetchError::Http(status));
                }
                current_url = url;
                continue;
            }
            Err(e) => return Err(e),
        }
    }

    Err(FetchError::Http(0))
}

pub async fn post_https_json_async(
    url: &str,
    body_json: String,
    auth_token: Option<&str>,
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<Vec<u8>, FetchError> {
    post_https_json_with_profile_async(
        url,
        NetProfile::default(),
        body_json,
        auth_token,
        timeout_ms,
        max_bytes,
    )
    .await
}

pub async fn post_https_json_with_profile_async(
    url: &str,
    profile: NetProfile,
    body_json: String,
    auth_token: Option<&str>,
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<Vec<u8>, FetchError> {
    let dev_idx = fetch_device_index(profile)?;

    const MAX_REDIRECTS: usize = 3;
    let mut current_url = String::from(url);

    for hop in 0..=MAX_REDIRECTS {
        let parsed = parse_https_url(current_url.as_str()).ok_or(FetchError::BadUrl)?;

        // Keepalive POSTs have proven fragile against some API responses. Route JSON POSTs
        // through the close-after-response path so completion never depends on keepalive body
        // framing or pooled-connection state.
        let res = fetch_on_device(
            &parsed,
            dev_idx,
            timeout_ms,
            max_bytes,
            Some(body_json.as_str()),
            auth_token,
            None,
        )
        .await;

        match res {
            Ok(v) => return Ok(v),
            Err(FetchError::Redirect { status, url }) => {
                if hop >= MAX_REDIRECTS {
                    return Err(FetchError::Http(status));
                }
                current_url = url;
                continue;
            }
            Err(e) => return Err(e),
        }
    }

    Err(FetchError::Http(0))
}

/// POST JSON and stream response as SSE (`text/event-stream`).
///
/// This is intended for model streaming (`stream: true`). The handler will be
/// called for each parsed SSE `data:` payload.
pub async fn post_https_sse_async(
    url: &str,
    body_json: String,
    auth_token: Option<&str>,
    timeout_ms: u32,
    max_bytes: usize,
    handler: &mut dyn SseHandler,
) -> Result<(), FetchError> {
    post_https_sse_with_profile_async(
        url,
        NetProfile::default(),
        body_json,
        auth_token,
        timeout_ms,
        max_bytes,
        handler,
    )
    .await
}

pub async fn post_https_sse_with_profile_async(
    url: &str,
    profile: NetProfile,
    body_json: String,
    auth_token: Option<&str>,
    timeout_ms: u32,
    max_bytes: usize,
    handler: &mut dyn SseHandler,
) -> Result<(), FetchError> {
    let dev_idx = fetch_device_index(profile)?;

    const MAX_REDIRECTS: usize = 3;
    let mut current_url = String::from(url);

    for hop in 0..=MAX_REDIRECTS {
        let parsed = parse_https_url(current_url.as_str()).ok_or(FetchError::BadUrl)?;

        match fetch_on_device_sse(
            &parsed,
            dev_idx,
            timeout_ms,
            max_bytes,
            body_json.as_str(),
            auth_token,
            handler,
        )
        .await
        {
            Ok(()) => return Ok(()),
            Err(FetchError::Redirect { status, url }) => {
                if hop >= MAX_REDIRECTS {
                    return Err(FetchError::Http(status));
                }
                current_url = url;
                continue;
            }
            Err(e) => return Err(e),
        }
    }

    Err(FetchError::Http(0))
}

/// Fetch a URL into a TRUEOSFS key (cache file).
///
/// Behavior used by async net-fetch C-ABI:
/// - if `path` already exists: success
/// - otherwise: download body (capped) directly into `path` (atomic via streaming write)
pub async fn fetch_https_to_file_async(
    url: &str,
    path: &str,
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<(), i32> {
    fetch_https_to_file_with_profile_async(url, NetProfile::default(), path, timeout_ms, max_bytes)
        .await
}

pub async fn fetch_https_to_file_with_profile_async(
    url: &str,
    profile: NetProfile,
    path: &str,
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<(), i32> {
    let t0 = Instant::now();
    let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
        return Err(FS_ERR_USBMS_NOT_FOUND);
    };

    let key = normalize_rel(path, false)?;

    match crate::r::fs::trueosfs::file_exists_async(disk, key.as_str()).await {
        Ok(true) => return Ok(()),
        Ok(false) => {}
        Err(e) => return Err(block_error_to_code(e)),
    }

    crate::log!("vhttps-cache: start key={} url={}\n", key, url);

    let t_exists = Instant::now();

    const MAX_REDIRECTS: usize = 3;
    let dev_idx = fetch_device_index_code(profile)?;

    let mut current_url = String::from(url);
    let mut last_err = fetch_error_to_code(FetchError::DnsFailed);

    for hop in 0..=MAX_REDIRECTS {
        let parsed = parse_https_url(current_url.as_str())
            .ok_or(FetchError::BadUrl)
            .map_err(fetch_error_to_code)?;

        last_err = fetch_error_to_code(FetchError::DnsFailed);

        let r = if HttpsLimits::KEEPALIVE_ENABLE {
            fetch_on_device_to_file_keepalive(
                &parsed,
                dev_idx,
                timeout_ms,
                max_bytes,
                disk,
                key.as_str(),
            )
            .await
        } else {
            fetch_on_device_to_file(&parsed, dev_idx, timeout_ms, max_bytes, disk, key.as_str())
                .await
        };

        match r {
            Ok(()) => {
                last_err = 0;
                break;
            }
            Err(FetchToFileError::Redirect { status, url }) => {
                let _ = crate::r::fs::trueosfs::file_delete_async(disk, key.as_str()).await;
                if hop >= MAX_REDIRECTS {
                    return Err(fetch_error_to_code(FetchError::Http(status)));
                }
                current_url = url;
                continue;
            }
            Err(FetchToFileError::Code(rc)) => {
                let _ = crate::r::fs::trueosfs::file_delete_async(disk, key.as_str()).await;
                last_err = rc;
            }
        }

        return Err(last_err);
    }

    if last_err != 0 {
        return Err(last_err);
    }

    let t_dl = Instant::now();

    let total_ms = t_dl.saturating_duration_since(t0).as_millis();
    let exists_ms = t_exists.saturating_duration_since(t0).as_millis();
    let dl_ms = t_dl.saturating_duration_since(t_exists).as_millis();
    crate::log!(
        "vhttps-cache: done key={} ms_total={} exists={} dl={}\n",
        key,
        total_ms,
        exists_ms,
        dl_ms
    );

    Ok(())
}

/// TRUEOS C ABI: start async HTTPS fetch to cache file.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_net_fetch_start(
    url_ptr: *const u8,
    url_len: usize,
    path_ptr: *const u8,
    path_len: usize,
) -> u32 {
    if url_ptr.is_null() || url_len == 0 || path_ptr.is_null() || path_len == 0 {
        return 0;
    }

    let url_bytes = core::slice::from_raw_parts(url_ptr, url_len);
    let path_bytes = core::slice::from_raw_parts(path_ptr, path_len);
    let Ok(url_s) = core::str::from_utf8(url_bytes) else {
        return 0;
    };
    let Ok(path_s) = core::str::from_utf8(path_bytes) else {
        return 0;
    };

    // Fixed fetch limits for loader cache path.
    //
    // This powers the QJS URL-module cache (esm.sh / CDN imports). Some responses are
    // large and/or slow enough that a ~2.5s global deadline causes spurious
    // `NET_ERR_TIMEOUT_BODY` failures even when connectivity is fine.
    const TIMEOUT_MS: u32 = 45_000;
    const MAX_BYTES: usize = 8 * 1024 * 1024;

    // Normalize the cache key so coalescing matches how fetch_https_to_file_async resolves paths.
    let key = match normalize_rel(path_s, false) {
        Ok(v) => v,
        Err(_) => return 0,
    };

    let url = String::from(url_s);
    let path = String::from(path_s);
    let op_id = CABI_NET_FETCH_SEQ.fetch_add(1, Ordering::Relaxed);
    CABI_NET_FETCH_RESULTS.lock().insert(op_id, None);

    // Coalesce duplicates: if the same cache key is already being fetched, register as follower.
    {
        let mut inflight = CABI_NET_FETCH_INFLIGHT.lock();
        if let Some(entry) = inflight.get_mut(&key) {
            entry.followers.push(op_id);
            return op_id;
        }
        inflight.insert(
            key.clone(),
            InflightFetch {
                owner_op_id: op_id,
                followers: Vec::new(),
            },
        );
    }

    spawn_cabi_net_fetch(op_id, key, url, path, TIMEOUT_MS, MAX_BYTES);
    op_id
}

/// TRUEOS C ABI: start async HTTPS fetch to in-memory bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_net_fetch_bytes_start(
    url_ptr: *const u8,
    url_len: usize,
) -> u32 {
    if url_ptr.is_null() || url_len == 0 {
        return 0;
    }

    let url_bytes = core::slice::from_raw_parts(url_ptr, url_len);
    let Ok(url_s) = core::str::from_utf8(url_bytes) else {
        return 0;
    };

    const TIMEOUT_MS: u32 = 45_000;
    const MAX_BYTES: usize = 8 * 1024 * 1024;

    let op_id = CABI_NET_FETCH_SEQ.fetch_add(1, Ordering::Relaxed);
    CABI_NET_FETCH_BYTES_RESULTS
        .lock()
        .insert(op_id, CabiNetFetchBytesResult::default());
    spawn_cabi_net_fetch_bytes(op_id, String::from(url_s), TIMEOUT_MS, MAX_BYTES);
    op_id
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_net_prewarm_url_start(
    url_ptr: *const u8,
    url_len: usize,
) -> i32 {
    if url_ptr.is_null() || url_len == 0 {
        return -1;
    }

    let url_bytes = core::slice::from_raw_parts(url_ptr, url_len);
    let Ok(url_s) = core::str::from_utf8(url_bytes) else {
        return -2;
    };
    if parse_https_url(url_s).is_none() {
        return -3;
    }

    spawn_cabi_net_prewarm_url(String::from(url_s));
    0
}

/// TRUEOS C ABI: start async HTTP(S) POST(JSON) to file.
///
/// `bearer_ptr/bearer_len` are optional (pass null/0 for no Authorization header).
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
    if url_ptr.is_null()
        || url_len == 0
        || path_ptr.is_null()
        || path_len == 0
        || body_ptr.is_null()
        || body_len == 0
    {
        return 0;
    }

    let url_bytes = core::slice::from_raw_parts(url_ptr, url_len);
    let path_bytes = core::slice::from_raw_parts(path_ptr, path_len);
    let body_bytes = core::slice::from_raw_parts(body_ptr, body_len);

    let Ok(url_s) = core::str::from_utf8(url_bytes) else {
        return 0;
    };
    let Ok(path_s) = core::str::from_utf8(path_bytes) else {
        return 0;
    };
    let Ok(body_s) = core::str::from_utf8(body_bytes) else {
        return 0;
    };

    let bearer = if bearer_ptr.is_null() || bearer_len == 0 {
        None
    } else {
        let bearer_bytes = core::slice::from_raw_parts(bearer_ptr, bearer_len);
        let Ok(v) = core::str::from_utf8(bearer_bytes) else {
            return 0;
        };
        Some(String::from(v))
    };

    let key = match normalize_rel(path_s, false) {
        Ok(v) => v,
        Err(_) => return 0,
    };

    let url = String::from(url_s);
    let body_json = String::from(body_s);
    let op_id = CABI_NET_FETCH_SEQ.fetch_add(1, Ordering::Relaxed);
    CABI_NET_FETCH_RESULTS.lock().insert(op_id, None);

    crate::wait::spawn_local_detached(async move {
        const TIMEOUT_MS: u32 = 20_000;
        const MAX_BYTES: usize = 4 * 1024 * 1024;

        let t0 = Instant::now();
        net_fetch_acquire_slot().await;

        let rc = match post_json_body_async(
            url.as_str(),
            body_json,
            bearer.as_deref(),
            TIMEOUT_MS,
            MAX_BYTES,
        )
        .await
        {
            Ok(bytes) => {
                crate::log!("net-fetch-post: response_body_len={}\n", bytes.len());
                if let Ok(s) = core::str::from_utf8(bytes.as_slice()) {
                    if let Some(summary) = super::json::summarize_openai_response_json(s) {
                        crate::log!("net-fetch-post: summary {}\n", summary);
                    }
                    log_utf8_chunks("net-fetch-post: response_json: ", s);
                } else {
                    crate::log!("net-fetch-post: response_json: [non-utf8]\n");
                }
                if let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() {
                    match crate::r::fs::trueosfs::file_in_async(
                        disk,
                        key.as_str(),
                        bytes.as_slice(),
                    )
                    .await
                    {
                        Ok(true) => 0,
                        Ok(false) => FS_ERR_IO,
                        Err(e) => block_error_to_code(e),
                    }
                } else {
                    FS_ERR_USBMS_NOT_FOUND
                }
            }
            Err(rc) => rc,
        };

        net_fetch_release_slot();

        let elapsed_ms = t0.elapsed().as_millis();
        if let Some(slot) = CABI_NET_FETCH_RESULTS.lock().get_mut(&op_id) {
            *slot = Some(rc);
        }

        crate::log!("net-fetch-post: done key={} rc={} ms={}\n", key, rc, elapsed_ms);

        CABI_NET_FETCH_WAIT.notify_all();
    });

    op_id
}

/// TRUEOS C ABI: start async HTTP(S) POST(JSON) to in-memory bytes.
///
/// `bearer_ptr/bearer_len` are optional (pass null/0 for no Authorization header).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_net_fetch_post_json_bytes_start(
    url_ptr: *const u8,
    url_len: usize,
    body_ptr: *const u8,
    body_len: usize,
    bearer_ptr: *const u8,
    bearer_len: usize,
) -> u32 {
    if url_ptr.is_null() || url_len == 0 || body_ptr.is_null() || body_len == 0 {
        return 0;
    }

    let url_bytes = core::slice::from_raw_parts(url_ptr, url_len);
    let body_bytes = core::slice::from_raw_parts(body_ptr, body_len);

    let Ok(url_s) = core::str::from_utf8(url_bytes) else {
        return 0;
    };
    let Ok(body_s) = core::str::from_utf8(body_bytes) else {
        return 0;
    };

    let bearer = if bearer_ptr.is_null() || bearer_len == 0 {
        None
    } else {
        let bearer_bytes = core::slice::from_raw_parts(bearer_ptr, bearer_len);
        let Ok(v) = core::str::from_utf8(bearer_bytes) else {
            return 0;
        };
        Some(String::from(v))
    };

    let url = String::from(url_s);
    let body_json = String::from(body_s);
    let request_id = CABI_NET_FETCH_SEQ.fetch_add(1, Ordering::Relaxed);
    CABI_NET_FETCH_BYTES_RESULTS
        .lock()
        .insert(request_id, CabiNetFetchBytesResult::default());

    crate::wait::spawn_local_detached(async move {
        const TIMEOUT_MS: u32 = 20_000;
        const MAX_BYTES: usize = 4 * 1024 * 1024;

        let t0 = Instant::now();
        net_fetch_acquire_slot().await;

        let (rc, bytes) = match post_json_body_async(
            url.as_str(),
            body_json,
            bearer.as_deref(),
            TIMEOUT_MS,
            MAX_BYTES,
        )
        .await
        {
            Ok(bytes) => {
                crate::log!("net-fetch-post: response_body_len={}\n", bytes.len());
                if let Ok(s) = core::str::from_utf8(bytes.as_slice()) {
                    if let Some(summary) = super::json::summarize_openai_response_json(s) {
                        crate::log!("net-fetch-post: summary {}\n", summary);
                    }
                    log_utf8_chunks("net-fetch-post: response_json: ", s);
                } else {
                    crate::log!("net-fetch-post: response_json: [non-utf8]\n");
                }
                (0, bytes)
            }
            Err(rc) => (rc, Vec::new()),
        };

        net_fetch_release_slot();

        if let Some(slot) = CABI_NET_FETCH_BYTES_RESULTS.lock().get_mut(&request_id) {
            slot.rc = Some(rc);
            slot.body = bytes;
        }

        let elapsed_ms = t0.elapsed().as_millis();
        crate::log!(
            "net-fetch-post: done request_id={} rc={} ms={} len={}\n",
            request_id,
            rc,
            elapsed_ms,
            CABI_NET_FETCH_BYTES_RESULTS
                .lock()
                .get(&request_id)
                .map(|v| v.body.len())
                .unwrap_or(0)
        );

        CABI_NET_FETCH_WAIT.notify_all();
    });

    request_id
}

/// TRUEOS C ABI: query async HTTPS fetch result.
///
/// Returns:
/// - `FS_ERR_NOT_FOUND` while operation is pending/unknown
/// - `0` on success
/// - negative error code on completion failure
#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_net_fetch_result(op_id: u32) -> i32 {
    let map = CABI_NET_FETCH_RESULTS.lock();
    match map.get(&op_id) {
        Some(Some(rc)) => *rc,
        Some(None) => FS_ERR_NOT_FOUND,
        None => FS_ERR_NOT_FOUND,
    }
}

/// TRUEOS C ABI: discard async HTTPS fetch state.
#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_net_fetch_discard(op_id: u32) -> i32 {
    let mut map = CABI_NET_FETCH_RESULTS.lock();
    map.remove(&op_id);

    // Best-effort: remove from any follower lists so coalescing maps don't retain dead ids.
    // (Leader tasks may still complete; they will simply skip removed result slots.)
    {
        let mut inflight = CABI_NET_FETCH_INFLIGHT.lock();
        let mut dead_keys: Vec<String> = Vec::new();
        for (k, v) in inflight.iter_mut() {
            v.followers.retain(|&id| id != op_id);
            if !inflight_fetch_has_live_interest(v.owner_op_id, v.followers.as_slice(), &map) {
                dead_keys.push(k.clone());
            }
        }
        for key in dead_keys {
            inflight.remove(&key);
        }
    }
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_net_fetch_bytes_result_len(op_id: u32) -> isize {
    let map = CABI_NET_FETCH_BYTES_RESULTS.lock();
    match map.get(&op_id) {
        Some(v) => match v.rc {
            Some(0) => v.body.len() as isize,
            Some(rc) => rc as isize,
            None => FS_ERR_NOT_FOUND as isize,
        },
        None => FS_ERR_NOT_FOUND as isize,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_net_fetch_bytes_read(
    op_id: u32,
    out_ptr: *mut u8,
    out_cap: usize,
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
    let len = entry.body.len();
    if out_ptr.is_null() || out_cap == 0 {
        return len as isize;
    }
    if len > out_cap {
        return FS_ERR_NO_SPACE as isize;
    }
    let entry = map.remove(&op_id).expect("entry present");
    unsafe { core::ptr::copy_nonoverlapping(entry.body.as_ptr(), out_ptr, entry.body.len()) };
    len as isize
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_net_fetch_bytes_discard(op_id: u32) -> i32 {
    CABI_NET_FETCH_BYTES_RESULTS.lock().remove(&op_id);
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_net_fetch_bytes_wait(op_id: u32, timeout_ms: u64) -> i32 {
    if op_id == 0 {
        return FS_ERR_BAD_PARAM;
    }

    if timeout_ms == 0 {
        let rc = trueos_cabi_net_fetch_bytes_result_len(op_id);
        return if rc == FS_ERR_NOT_FOUND as isize {
            FS_ERR_NOT_FOUND
        } else if rc < 0 {
            rc as i32
        } else {
            0
        };
    }

    let start = embassy_time::Instant::now();
    let timeout = EmbassyDuration::from_millis(timeout_ms);
    loop {
        let rc = trueos_cabi_net_fetch_bytes_result_len(op_id);
        if rc != FS_ERR_NOT_FOUND as isize {
            return if rc < 0 { rc as i32 } else { 0 };
        }

        let elapsed = embassy_time::Instant::now().saturating_duration_since(start);
        if elapsed >= timeout {
            return FS_ERR_TIMEOUT;
        }
        let remain = timeout - elapsed;
        let step = core::cmp::min(remain, EmbassyDuration::from_millis(100));
        let _ = wait_on_net_fetch_queue_blocking(step.as_millis() as u64);
    }
}

/// TRUEOS C ABI: wait for a net-fetch operation to complete.
///
/// Returns:
/// - `FS_ERR_NOT_FOUND` while pending (only when timeout_ms == 0)
/// - `FS_ERR_TIMEOUT` when deadline expires
/// - `0` on success
/// - negative error code on completion failure
#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_net_fetch_wait(op_id: u32, timeout_ms: u64) -> i32 {
    if op_id == 0 {
        return FS_ERR_BAD_PARAM;
    }

    if timeout_ms == 0 {
        return trueos_cabi_net_fetch_result(op_id);
    }

    let start = embassy_time::Instant::now();
    let timeout = EmbassyDuration::from_millis(timeout_ms);
    loop {
        let rc = trueos_cabi_net_fetch_result(op_id);
        if rc != FS_ERR_NOT_FOUND {
            return rc;
        }

        let elapsed = embassy_time::Instant::now().saturating_duration_since(start);
        if elapsed >= timeout {
            return FS_ERR_TIMEOUT;
        }
        let remain = timeout - elapsed;
        let step = core::cmp::min(remain, EmbassyDuration::from_millis(100));
        let _ = wait_on_net_fetch_queue_blocking(step.as_millis() as u64);
    }
}
