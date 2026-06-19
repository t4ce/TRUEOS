extern crate alloc;

use alloc::{format, string::String, vec::Vec};

use embassy_time::{Duration, Instant, Timer};
use v::vnet as api;

use crate::r::net::{VNet, ports};

const PAGE_PATH: &str = "/";
const LEGACY_PAGE_PATH: &str = "/audio/live";
const WAV_PATH: &str = "/audio/live.wav";
const RX_BUF_MAX: usize = 8 * 1024;
const MAX_SESSIONS: usize = 16;
const SAMPLE_RATE: usize = 48_000;
const CHANNELS: usize = 2;
const PREROLL_MS: usize = 150;
const SEND_MS: usize = 50;
const POLL_MS: u64 = 5;
const REQUEST_IDLE_TIMEOUT_MS: u64 = 1_500;
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
    cursor: u64,
    pending_bytes: usize,
    sent_bytes: usize,
    deadline: Instant,
    stream_chunks: usize,
    stream_samples: usize,
}

struct AudioHttpEndpoint {
    vnet: VNet,
    listener: Option<api::NetHandle>,
    listener_ready: bool,
    dev_idx: usize,
    sessions: Vec<AudioHttpSession>,
}

impl AudioHttpSession {
    fn new(handle: api::NetHandle) -> Self {
        Self {
            handle,
            rx: Vec::new(),
            mode: SessionMode::ReadingRequest,
            cursor: 0,
            pending_bytes: 0,
            sent_bytes: 0,
            deadline: Instant::now() + Duration::from_millis(REQUEST_IDLE_TIMEOUT_MS),
            stream_chunks: 0,
            stream_samples: 0,
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
            port: ports::TINYAUDIO_LIVE_HTTP_TCP_PORT,
        })
        .is_err()
    {
        crate::log!(
            "tinyaudio-live-http: listen submit failed dev={} owner={}\n",
            dev_idx,
            vnet.owner()
        );
        return None;
    }

    let ip = crate::net::adapter::ipv4_at(dev_idx);
    let name = crate::net::device_name_at(dev_idx).unwrap_or("?");
    match ip {
        Some([a, b, c, d]) => crate::log!(
            "tinyaudio-live-http: listen submitted tcp {} owner={} dev={} {} ip={}.{}.{}.{}\n",
            ports::TINYAUDIO_LIVE_HTTP_TCP_PORT,
            vnet.owner(),
            dev_idx,
            name,
            a,
            b,
            c,
            d
        ),
        None => crate::log!(
            "tinyaudio-live-http: listen submitted tcp {} owner={} dev={} {} ip=none\n",
            ports::TINYAUDIO_LIVE_HTTP_TCP_PORT,
            vnet.owner(),
            dev_idx,
            name
        ),
    }

    Some(AudioHttpEndpoint {
        vnet,
        listener: None,
        listener_ready: false,
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

fn http_request_path(req: &str) -> Option<&str> {
    let line_end = req
        .find("\r\n")
        .or_else(|| req.find('\n'))
        .unwrap_or(req.len());
    let line = req.get(..line_end)?;
    let mut it = line.split_whitespace();
    let method = it.next()?;
    let target = it.next()?;
    if method != "GET" {
        return None;
    }
    Some(path_only(target))
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
        "HTTP/1.1 200 OK\r\n",
        "text/html; charset=utf-8",
        b"<!doctype html><html><head><meta charset=\"utf-8\"><title>TRUEOS Live Audio</title><style>body{font-family:sans-serif;margin:2rem;max-width:42rem}audio{width:100%;margin-top:1rem}</style></head><body><h1>TRUEOS Live Audio</h1><audio controls autoplay src=\"/audio/live.wav\"></audio></body></html>",
    )
}

fn not_found_response() -> Vec<u8> {
    response_with_body("HTTP/1.1 404 Not Found\r\n", "text/plain; charset=utf-8", b"not found\n")
}

fn wav_stream_head() -> Vec<u8> {
    let mut head = Vec::new();
    head.extend_from_slice(b"HTTP/1.1 200 OK\r\n");
    head.extend_from_slice(b"Content-Type: audio/wav\r\n");
    head.extend_from_slice(b"Cache-Control: no-store\r\n");
    head.extend_from_slice(b"Connection: close\r\n\r\n");
    head.extend_from_slice(crate::tst::esynth::live_wav_stream_header().as_slice());
    head
}

fn handle_request(vnet: &VNet, session: &mut AudioHttpSession) -> bool {
    let Some(header_end) = find_http_header_end(session.rx.as_slice()) else {
        return false;
    };

    let Ok(req) = core::str::from_utf8(&session.rx[..header_end]) else {
        close_session(vnet, session.handle);
        return true;
    };

    let path = http_request_path(req);
    crate::log!(
        "tinyaudio-live-http: request handle={} path={:?} bytes={}\n",
        session.handle.0,
        path,
        header_end
    );

    match path {
        Some(PAGE_PATH) | Some(LEGACY_PAGE_PATH) => {
            let page = live_audio_page();
            !send_fixed_response(vnet, session, page.as_slice())
        }
        Some(WAV_PATH) => {
            let preroll_samples = SAMPLE_RATE * CHANNELS * PREROLL_MS / 1000;
            session.cursor =
                crate::tst::esynth::live_pcm_stream_start_cursor(preroll_samples).unwrap_or(0);
            let head = wav_stream_head();
            if !send_tcp_bytes(vnet, session.handle, head.as_slice()) {
                close_session(vnet, session.handle);
                return true;
            }
            session.mode = SessionMode::Streaming;
            session.rx.clear();
            session.stream_chunks = 0;
            session.stream_samples = 0;
            crate::log!("tinyaudio-live-http: stream opened handle={}\n", session.handle.0);
            false
        }
        _ => {
            let response = not_found_response();
            !send_fixed_response(vnet, session, response.as_slice())
        }
    }
}

fn stream_audio_tick(vnet: &VNet, session: &mut AudioHttpSession) -> bool {
    let max_samples = SAMPLE_RATE * CHANNELS * SEND_MS / 1000;
    let mut samples = Vec::with_capacity(max_samples);

    let Some(next) =
        crate::tst::esynth::live_pcm_read_since(session.cursor, &mut samples, max_samples)
    else {
        return false;
    };
    session.cursor = next;

    if samples.is_empty() {
        return false;
    }

    let mut bytes = Vec::with_capacity(samples.len() * core::mem::size_of::<i16>());
    for sample in samples.iter().copied() {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }

    if !send_tcp_bytes(vnet, session.handle, bytes.as_slice()) {
        close_session(vnet, session.handle);
        return true;
    }

    session.stream_chunks = session.stream_chunks.saturating_add(1);
    session.stream_samples = session.stream_samples.saturating_add(samples.len());
    if session.stream_chunks <= 4 || session.stream_chunks.is_multiple_of(100) {
        crate::log!(
            "tinyaudio-live-http: stream chunk handle={} chunk={} samples={} total_samples={} bytes={}\n",
            session.handle.0,
            session.stream_chunks,
            samples.len(),
            session.stream_samples,
            bytes.len()
        );
    }

    false
}

fn prune_idle_sessions(endpoint: &mut AudioHttpEndpoint) {
    let now = Instant::now();
    let mut idx = 0usize;
    while idx < endpoint.sessions.len() {
        if endpoint.sessions[idx].is_timed_out(now) {
            let handle = endpoint.sessions[idx].handle;
            crate::log!(
                "tinyaudio-live-http: timeout close dev={} handle={} mode={:?}\n",
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

#[embassy_executor::task]
pub async fn tinyaudio_live_http_task() {
    let mut endpoints: Vec<AudioHttpEndpoint> = Vec::new();
    loop {
        audio_http_add_endpoints(&mut endpoints);
        if !endpoints.is_empty() {
            break;
        }
        crate::log!("tinyaudio-live-http: waiting for a usable NIC\n");
        Timer::after(Duration::from_millis(250)).await;
    }

    let mut endpoint_discovery_ticks = 0u32;
    loop {
        if endpoint_discovery_ticks == 0 {
            audio_http_add_endpoints(&mut endpoints);
        }
        endpoint_discovery_ticks = (endpoint_discovery_ticks + 1) % 100;

        for endpoint in endpoints.iter_mut() {
            prune_idle_sessions(endpoint);

            while let Some(ev) = endpoint.vnet.pop_event() {
                match ev {
                    api::Event::Opened { handle, kind } => {
                        if kind == api::SocketKind::Tcp {
                            endpoint.listener = Some(handle);
                            endpoint.listener_ready = true;
                            crate::log!(
                                "tinyaudio-live-http: tcp listen opened dev={} handle={} port={} page={} stream={}\n",
                                endpoint.dev_idx,
                                handle.0,
                                ports::TINYAUDIO_LIVE_HTTP_TCP_PORT,
                                PAGE_PATH,
                                WAV_PATH
                            );
                        }
                    }
                    api::Event::TcpEstablished { handle, .. } => {
                        if endpoint.sessions.len() >= MAX_SESSIONS {
                            crate::log!(
                                "tinyaudio-live-http: max sessions close dev={} handle={} active={}\n",
                                endpoint.dev_idx,
                                handle.0,
                                endpoint.sessions.len()
                            );
                            close_session(&endpoint.vnet, handle);
                        } else {
                            if endpoint.listener == Some(handle) {
                                endpoint.listener = None;
                                endpoint.listener_ready = false;
                                let _ = endpoint.vnet.submit(api::Command::OpenTcpListen {
                                    port: ports::TINYAUDIO_LIVE_HTTP_TCP_PORT,
                                });
                            }
                            endpoint.sessions.push(AudioHttpSession::new(handle));
                            crate::log!(
                                "tinyaudio-live-http: tcp established dev={} handle={}\n",
                                endpoint.dev_idx,
                                handle.0
                            );
                        }
                    }
                    api::Event::TcpData { handle, data } => {
                        let Some(idx) = endpoint
                            .sessions
                            .iter()
                            .position(|session| session.handle == handle)
                        else {
                            continue;
                        };
                        let session = &mut endpoint.sessions[idx];
                        if session.mode != SessionMode::ReadingRequest {
                            continue;
                        }
                        if session.rx.len().saturating_add(data.len()) > RX_BUF_MAX {
                            close_session(&endpoint.vnet, handle);
                            endpoint.sessions.remove(idx);
                            continue;
                        }
                        session.rx.extend_from_slice(data.as_slice());
                        session.refresh_request_deadline();
                        if handle_request(&endpoint.vnet, session) {
                            endpoint.sessions.remove(idx);
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
                            endpoint.listener = None;
                            endpoint.listener_ready = false;
                            endpoint.sessions.clear();
                            let _ = endpoint.vnet.submit(api::Command::OpenTcpListen {
                                port: ports::TINYAUDIO_LIVE_HTTP_TCP_PORT,
                            });
                        }
                    }
                    api::Event::Error { msg } => {
                        if msg != "bad handle" {
                            crate::log!(
                                "tinyaudio-live-http: error dev={} {}\n",
                                endpoint.dev_idx,
                                msg
                            );
                        }
                    }
                    api::Event::TcpSent { handle, len } => {
                        if let Some(session) = endpoint
                            .sessions
                            .iter_mut()
                            .find(|session| session.handle == handle)
                            && session.mode == SessionMode::SendingFixed
                        {
                            session.sent_bytes = session.sent_bytes.saturating_add(len as usize);
                            if session.sent_bytes >= session.pending_bytes {
                                close_session(&endpoint.vnet, handle);
                            }
                        }
                    }
                    api::Event::UdpPacket { .. }
                    | api::Event::UdpPacketV6 { .. }
                    | api::Event::IcmpReply { .. }
                    | api::Event::IcmpReplyV6 { .. } => {}
                }
            }

            let mut idx = 0usize;
            while idx < endpoint.sessions.len() {
                if endpoint.sessions[idx].mode == SessionMode::Streaming
                    && stream_audio_tick(&endpoint.vnet, &mut endpoint.sessions[idx])
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
