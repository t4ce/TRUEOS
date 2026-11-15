//! Listen-only HTTP playback sink for completed clips. No hardware or capture.
pub(crate) mod handoff;
mod sink;
use sink::Sink;
use spin::Mutex;

static SINK: Mutex<Sink> = Mutex::new(Sink::new());

extern crate alloc;

use alloc::{format, vec::Vec};

use trueos_time::{Duration, Instant, Timer};
use v::vnet as api;

use crate::r::net::{VNet, ports};

const PAGE_PATH: &str = "/";
const LEGACY_PAGE_PATH: &str = "/audio/live";
const WAV_PATH: &str = "/audio/live.wav";
const STATUS_PATH: &str = "/audio/status";
const RX_BUF_MAX: usize = 8 * 1024;
const MAX_STREAM_CLIENTS: usize = 4;
const MAX_IDLE_REQUESTS: usize = 4;
const MAX_SESSIONS: usize = MAX_STREAM_CLIENTS + MAX_IDLE_REQUESTS;
const SAMPLE_RATE: usize = 48_000;
const CHANNELS: usize = 2;
const SEND_MS: usize = 10;
const POLL_MS: u64 = 5;
const REQUEST_IDLE_TIMEOUT_MS: u64 = 750;
const FIXED_SEND_TIMEOUT_MS: u64 = 1_500;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SessionMode {
    ReadingRequest,
    SendingFixed,
    Streaming,
}

struct AudioHttpSession {
    handle: api::NetHandle,
    rx: Vec<u8>,
    mode: SessionMode,
    pending_bytes: usize,
    sent_bytes: usize,
    deadline: Instant,
}

struct AudioHttpEndpoint {
    vnet: VNet,
    listener: Option<api::NetHandle>,
    dev_idx: usize,
    sessions: Vec<AudioHttpSession>,
}

impl AudioHttpSession {
    fn new(handle: api::NetHandle) -> Self {
        Self {
            handle,
            rx: Vec::new(),
            mode: SessionMode::ReadingRequest,
            pending_bytes: 0,
            sent_bytes: 0,
            deadline: Instant::now() + Duration::from_millis(REQUEST_IDLE_TIMEOUT_MS),
        }
    }

    fn refresh_request_deadline(&mut self) {
        self.deadline = Instant::now() + Duration::from_millis(REQUEST_IDLE_TIMEOUT_MS);
    }

    fn refresh_fixed_deadline(&mut self) {
        self.deadline = Instant::now() + Duration::from_millis(FIXED_SEND_TIMEOUT_MS);
    }

    fn is_timed_out(&self, now: Instant) -> bool {
        match self.mode {
            SessionMode::ReadingRequest | SessionMode::SendingFixed => now >= self.deadline,
            SessionMode::Streaming => false,
        }
    }
}

fn audio_http_open_endpoint(dev_idx: usize) -> Option<AudioHttpEndpoint> {
    let usable = crate::net::adapter::ipv4_at(dev_idx).is_some()
        || crate::net::link_state_at(dev_idx)
            .map(|state| state.up)
            .unwrap_or(false);
    if !usable {
        return None;
    }

    let vnet = VNet::open(dev_idx)?;
    if vnet
        .submit(api::Command::OpenTcpListen {
            port: ports::AUDIO_HTTP_TCP_PORT,
        })
        .is_err()
    {
        crate::log!(
            "audio-http: listen submit failed dev={} owner={}\n",
            dev_idx,
            vnet.owner()
        );
        return None;
    }

    let ip = crate::net::adapter::ipv4_at(dev_idx);
    let name = crate::net::device_name_at(dev_idx).unwrap_or("?");
    match ip {
        Some([a, b, c, d]) => crate::log!(
            "audio-http: listen submitted tcp {} owner={} dev={} {} ip={}.{}.{}.{}\n",
            ports::AUDIO_HTTP_TCP_PORT,
            vnet.owner(),
            dev_idx,
            name,
            a,
            b,
            c,
            d
        ),
        None => crate::log!(
            "audio-http: listen submitted tcp {} owner={} dev={} {} ip=none\n",
            ports::AUDIO_HTTP_TCP_PORT,
            vnet.owner(),
            dev_idx,
            name
        ),
    }

    Some(AudioHttpEndpoint {
        vnet,
        listener: None,
        dev_idx,
        sessions: Vec::new(),
    })
}

fn audio_http_add_endpoints(endpoints: &mut Vec<AudioHttpEndpoint>) -> usize {
    let mut added = 0usize;
    for dev_idx in 0..crate::net::device_count() {
        if endpoints.iter().any(|endpoint| endpoint.dev_idx == dev_idx) {
            continue;
        }
        if let Some(endpoint) = audio_http_open_endpoint(dev_idx) {
            endpoints.push(endpoint);
            added += 1;
        }
    }
    added
}

fn find_http_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}

fn path_only(target: &str) -> &str {
    target
        .split_once('?')
        .map(|(path, _)| path)
        .unwrap_or(target)
}

fn http_request_target(req: &str) -> Option<&str> {
    let line_end = crate::r::pat::find_str(req, "\r\n")
        .or_else(|| req.find('\n'))
        .unwrap_or(req.len());
    let line = req.get(..line_end)?;
    let mut it = line.split_whitespace();
    if it.next()? != "GET" {
        return None;
    }
    it.next()
}

fn send_tcp_bytes(vnet: &VNet, handle: api::NetHandle, bytes: &[u8]) -> bool {
    for chunk in bytes.chunks(api::MAX_MSG) {
        if vnet
            .submit(api::Command::SendTcp {
                handle,
                data: api::ByteBuf::from_slice_trunc(chunk),
            })
            .is_err()
        {
            return false;
        }
    }
    true
}

fn close_session(vnet: &VNet, handle: api::NetHandle) {
    let _ = vnet.submit(api::Command::Close { handle });
}

fn send_fixed_response(vnet: &VNet, session: &mut AudioHttpSession, bytes: &[u8]) -> bool {
    if !send_tcp_bytes(vnet, session.handle, bytes) {
        close_session(vnet, session.handle);
        return false;
    }
    session.mode = SessionMode::SendingFixed;
    session.pending_bytes = bytes.len();
    session.sent_bytes = 0;
    session.refresh_fixed_deadline();
    true
}

fn response_with_body(status: &str, content_type: &str, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(status.as_bytes());
    out.extend_from_slice(b"Content-Type: ");
    out.extend_from_slice(content_type.as_bytes());
    out.extend_from_slice(b"\r\nCache-Control: no-store\r\nContent-Length: ");
    out.extend_from_slice(format!("{}", body.len()).as_bytes());
    out.extend_from_slice(b"\r\nConnection: close\r\n\r\n");
    out.extend_from_slice(body);
    out
}

fn live_audio_page() -> Vec<u8> {
    response_with_body(
        "HTTP/1.1 200 OK\r\n", "text/html; charset=utf-8",
        br#"<!doctype html><html><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>TRUEOS Audio</title><style>body{font:1rem system-ui;margin:2rem;max-width:40rem}audio{width:100%}</style>
</head><body><h1>TRUEOS Audio</h1><p>Listen to the current playback stream.</p>
<audio controls preload="none" src="/audio/live.wav"></audio></body></html>"#,
    )
}

fn not_found_response() -> Vec<u8> {
    response_with_body(
        "HTTP/1.1 404 Not Found\r\n",
        "text/plain; charset=utf-8",
        b"not found\n",
    )
}

fn busy_response() -> Vec<u8> {
    response_with_body(
        "HTTP/1.1 503 Service Unavailable\r\n",
        "text/plain; charset=utf-8",
        b"too many live audio clients\n",
    )
}

fn wav_stream_head() -> Vec<u8> {
    let mut head = Vec::new();
    head.extend_from_slice(b"HTTP/1.1 200 OK\r\n");
    head.extend_from_slice(b"Content-Type: audio/wav\r\n");
    head.extend_from_slice(b"Cache-Control: no-store\r\n");
    head.extend_from_slice(b"Connection: close\r\n\r\n");
    // Unbounded PCM stream: browsers play until the connection closes.
    head.extend_from_slice(b"RIFF");
    head.extend_from_slice(&u32::MAX.to_le_bytes());
    head.extend_from_slice(b"WAVEfmt ");
    head.extend_from_slice(&16u32.to_le_bytes());
    head.extend_from_slice(&1u16.to_le_bytes());
    head.extend_from_slice(&(CHANNELS as u16).to_le_bytes());
    head.extend_from_slice(&(SAMPLE_RATE as u32).to_le_bytes());
    head.extend_from_slice(&((SAMPLE_RATE * CHANNELS * 2) as u32).to_le_bytes());
    head.extend_from_slice(&((CHANNELS * 2) as u16).to_le_bytes());
    head.extend_from_slice(&16u16.to_le_bytes());
    head.extend_from_slice(b"data");
    head.extend_from_slice(&(u32::MAX - 36).to_le_bytes());
    head
}

fn handle_request(vnet: &VNet, session: &mut AudioHttpSession, active_streams: usize) -> bool {
    let Some(header_end) = find_http_header_end(session.rx.as_slice()) else {
        return false;
    };

    let header = session.rx[..header_end].to_vec();
    let Ok(req) = core::str::from_utf8(header.as_slice()) else {
        close_session(vnet, session.handle);
        return true;
    };

    let target = http_request_target(req);
    let path = target.map(path_only);
    crate::log!(
        "audio-http: request handle={} path={:?} bytes={}\n",
        session.handle.0,
        path,
        header_end
    );

    match path {
        Some(PAGE_PATH) | Some(LEGACY_PAGE_PATH) => {
            let page = live_audio_page();
            !send_fixed_response(vnet, session, page.as_slice())
        }
        Some(STATUS_PATH) => {
            let response = status_response(active_streams);
            !send_fixed_response(vnet, session, response.as_slice())
        }
        Some(WAV_PATH) => {
            if active_streams >= MAX_STREAM_CLIENTS {
                let response = busy_response();
                return !send_fixed_response(vnet, session, response.as_slice());
            }
            let head = wav_stream_head();
            if !send_tcp_bytes(vnet, session.handle, head.as_slice()) {
                close_session(vnet, session.handle);
                return true;
            }
            session.mode = SessionMode::Streaming;
            session.rx.clear();
            session.pending_bytes = head.len();
            session.sent_bytes = 0;
            false
        }
        _ => {
            let response = not_found_response();
            !send_fixed_response(vnet, session, response.as_slice())
        }
    }
}

fn status_response(listeners: usize) -> Vec<u8> {
    let body = format!(
        "{{\"listeners\":{},\"capacity\":{}}}\n",
        listeners, MAX_STREAM_CLIENTS
    );
    response_with_body(
        "HTTP/1.1 200 OK\r\n",
        "application/json; charset=utf-8",
        body.as_bytes(),
    )
}

fn stream_audio_tick(vnet: &VNet, session: &mut AudioHttpSession, bytes: &[u8]) -> bool {
    // A slow listener cannot retain an unbounded TCP send backlog.
    const MAX_PENDING_BYTES: usize = SAMPLE_RATE * CHANNELS * 2 / 4;
    if session.pending_bytes.saturating_sub(session.sent_bytes) > MAX_PENDING_BYTES
        || !send_tcp_bytes(vnet, session.handle, bytes)
    {
        close_session(vnet, session.handle);
        return true;
    }
    session.pending_bytes = session.pending_bytes.saturating_add(bytes.len());
    false
}

fn active_stream_count(endpoint: &AudioHttpEndpoint) -> usize {
    endpoint
        .sessions
        .iter()
        .filter(|session| session.mode == SessionMode::Streaming)
        .count()
}

fn reopen_listener(endpoint: &mut AudioHttpEndpoint) {
    endpoint.listener = None;
    let _ = endpoint.vnet.submit(api::Command::OpenTcpListen {
        port: ports::AUDIO_HTTP_TCP_PORT,
    });
}

fn close_oldest_reading_request(endpoint: &mut AudioHttpEndpoint) -> bool {
    let Some(idx) = endpoint
        .sessions
        .iter()
        .position(|session| session.mode == SessionMode::ReadingRequest)
    else {
        return false;
    };

    let handle = endpoint.sessions[idx].handle;
    crate::log!(
        "audio-http: idle preconnect close dev={} handle={} active={}\n",
        endpoint.dev_idx,
        handle.0,
        endpoint.sessions.len()
    );
    close_session(&endpoint.vnet, handle);
    endpoint.sessions.remove(idx);
    true
}

fn prune_idle_sessions(endpoint: &mut AudioHttpEndpoint) {
    let now = Instant::now();
    let mut idx = 0usize;
    while idx < endpoint.sessions.len() {
        if endpoint.sessions[idx].is_timed_out(now) {
            let handle = endpoint.sessions[idx].handle;
            crate::log!(
                "audio-http: timeout close dev={} handle={} mode={:?}\n",
                endpoint.dev_idx,
                handle.0,
                endpoint.sessions[idx].mode
            );
            close_session(&endpoint.vnet, handle);
            endpoint.sessions.remove(idx);
        } else {
            idx += 1;
        }
    }
}

#[trueos_executor::task]
pub async fn audio_http_task() {
    let mut endpoints: Vec<AudioHttpEndpoint> = Vec::new();
    loop {
        audio_http_add_endpoints(&mut endpoints);
        if !endpoints.is_empty() {
            break;
        }
        crate::log!("audio-http: waiting for a usable NIC\n");
        Timer::after(Duration::from_millis(250)).await;
    }

    let mut endpoint_discovery_ticks = 0u32;
    let mut next_send = Instant::now();
    loop {
        if endpoint_discovery_ticks == 0 {
            audio_http_add_endpoints(&mut endpoints);
        }
        endpoint_discovery_ticks = (endpoint_discovery_ticks + 1) % 100;

        let now = Instant::now();
        let due = now >= next_send;
        let mut pcm_bytes = Vec::new();
        if due {
            // Advance the timeline instead of accumulating polling latency.
            next_send = next_send + Duration::from_millis(SEND_MS as u64);
            if now >= next_send + Duration::from_millis(100) {
                next_send = now + Duration::from_millis(SEND_MS as u64);
            }
            pcm_bytes.resize(SAMPLE_RATE * CHANNELS * 2 * SEND_MS / 1000, 0);
            SINK.lock().render(&mut pcm_bytes);
        }
        for endpoint in endpoints.iter_mut() {
            prune_idle_sessions(endpoint);

            while let Some(ev) = endpoint.vnet.pop_event() {
                match ev {
                    api::Event::Opened { handle, kind } => {
                        if kind == api::SocketKind::Tcp {
                            endpoint.listener = Some(handle);
                            crate::log!(
                                "audio-http: tcp listen opened dev={} handle={} port={} page={} stream={}\n",
                                endpoint.dev_idx,
                                handle.0,
                                ports::AUDIO_HTTP_TCP_PORT,
                                PAGE_PATH,
                                WAV_PATH
                            );
                        }
                    }
                    api::Event::TcpEstablished { handle, .. } => {
                        if endpoint.listener == Some(handle) {
                            reopen_listener(endpoint);
                        }

                        if endpoint
                            .sessions
                            .iter()
                            .any(|session| session.handle == handle)
                        {
                            continue;
                        }

                        if endpoint.sessions.len() >= MAX_SESSIONS {
                            close_oldest_reading_request(endpoint);
                            if endpoint.sessions.len() >= MAX_SESSIONS {
                                crate::log!(
                                    "audio-http: max sessions close dev={} handle={} active={}\n",
                                    endpoint.dev_idx,
                                    handle.0,
                                    endpoint.sessions.len()
                                );
                                close_session(&endpoint.vnet, handle);
                                continue;
                            }
                        }

                        endpoint.sessions.push(AudioHttpSession::new(handle));
                        crate::log!(
                            "audio-http: tcp established dev={} handle={}\n",
                            endpoint.dev_idx,
                            handle.0
                        );
                    }
                    api::Event::TcpData { handle, data } => {
                        if endpoint.listener == Some(handle) {
                            reopen_listener(endpoint);
                        }

                        let idx = match endpoint
                            .sessions
                            .iter()
                            .position(|session| session.handle == handle)
                        {
                            Some(idx) => idx,
                            None => {
                                if endpoint.sessions.len() >= MAX_SESSIONS {
                                    close_oldest_reading_request(endpoint);
                                    if endpoint.sessions.len() >= MAX_SESSIONS {
                                        crate::log!(
                                            "audio-http: max sessions close dev={} handle={} active={}\n",
                                            endpoint.dev_idx,
                                            handle.0,
                                            endpoint.sessions.len()
                                        );
                                        close_session(&endpoint.vnet, handle);
                                        continue;
                                    }
                                }

                                endpoint.sessions.push(AudioHttpSession::new(handle));
                                crate::log!(
                                    "audio-http: tcp data before established dev={} handle={}\n",
                                    endpoint.dev_idx,
                                    handle.0
                                );
                                endpoint.sessions.len() - 1
                            }
                        };
                        let active_streams = active_stream_count(endpoint);
                        let session = &mut endpoint.sessions[idx];
                        match session.mode {
                            SessionMode::ReadingRequest => {
                                if session.rx.len().saturating_add(data.len()) > RX_BUF_MAX {
                                    close_session(&endpoint.vnet, handle);
                                    endpoint.sessions.remove(idx);
                                    continue;
                                }
                                session.rx.extend_from_slice(data.as_slice());
                                session.refresh_request_deadline();
                                let closed =
                                    handle_request(&endpoint.vnet, session, active_streams);
                                if closed {
                                    endpoint.sessions.remove(idx);
                                }
                            }
                            SessionMode::SendingFixed | SessionMode::Streaming => {}
                        }
                    }
                    api::Event::Closed { handle } => {
                        if let Some(idx) = endpoint
                            .sessions
                            .iter()
                            .position(|session| session.handle == handle)
                        {
                            endpoint.sessions.remove(idx);
                        } else if Some(handle) == endpoint.listener {
                            endpoint.sessions.clear();
                            reopen_listener(endpoint);
                        }
                    }
                    api::Event::Error { msg } => {
                        if msg != "bad handle" {
                            crate::log!("audio-http: error dev={} {}\n", endpoint.dev_idx, msg);
                        }
                    }
                    api::Event::TcpSent { handle, len } => {
                        if let Some(session) = endpoint
                            .sessions
                            .iter_mut()
                            .find(|session| session.handle == handle)
                        {
                            session.sent_bytes = session.sent_bytes.saturating_add(len as usize);
                            if session.mode == SessionMode::SendingFixed
                                && session.sent_bytes >= session.pending_bytes
                            {
                                close_session(&endpoint.vnet, handle);
                            }
                        }
                    }
                    api::Event::UdpPacket { .. }
                    | api::Event::UdpPacketV6 { .. }
                    | api::Event::IpPacket { .. }
                    | api::Event::IcmpReply { .. }
                    | api::Event::IcmpReplyV6 { .. }
                    | api::Event::TcpReadClosed { .. } => {}
                }
            }

            let mut idx = 0usize;
            while idx < endpoint.sessions.len() {
                if due
                    && endpoint.sessions[idx].mode == SessionMode::Streaming
                    && stream_audio_tick(&endpoint.vnet, &mut endpoint.sessions[idx], &pcm_bytes)
                {
                    endpoint.sessions.remove(idx);
                } else {
                    idx += 1;
                }
            }
        }

        Timer::after(Duration::from_millis(POLL_MS)).await;
    }
}
