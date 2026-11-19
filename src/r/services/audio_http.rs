//! Listen-only HTTP playback of completed or progressive PCM.
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
const PCM_PATH: &str = "/audio/live.pcm";
const WAV_PATH: &str = "/audio/live.wav";
const CLIP_PATH: &str = "/audio/clip.wav";
const STATUS_PATH: &str = "/audio/status";
const RX_BUF_MAX: usize = 8 * 1024;
const MAX_STREAM_CLIENTS: usize = 4;
const MAX_IDLE_REQUESTS: usize = 4;
const MAX_SESSIONS: usize = MAX_STREAM_CLIENTS + MAX_IDLE_REQUESTS;
const SAMPLE_RATE: usize = 48_000;
const CHANNELS: usize = 2;
const SEND_MS: usize = 10;
const FRAME_BYTES: usize = CHANNELS * 2;
const BLOCK_BYTES: usize = SAMPLE_RATE * FRAME_BYTES * SEND_MS / 1000;
const PCM_PREFIX_BYTES: usize = 4;
const POLL_MS: u64 = 5;
const REQUEST_IDLE_TIMEOUT_MS: u64 = 750;
const FIXED_SEND_TIMEOUT_MS: u64 = 1_500;

fn monotonic_millis() -> u64 {
    Instant::now().as_millis()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SessionMode {
    ReadingRequest,
    SendingFixed,
    Streaming,
    StreamingSources,
    SendingClip,
}

struct ClipTransfer {
    clip: sink::Clip,
    offset: usize,
}

struct AudioHttpSession {
    handle: api::NetHandle,
    rx: Vec<u8>,
    mode: SessionMode,
    pending_bytes: usize,
    sent_bytes: usize,
    deadline: Instant,
    clip: Option<ClipTransfer>,
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
            clip: None,
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
            SessionMode::ReadingRequest | SessionMode::SendingFixed | SessionMode::SendingClip => now >= self.deadline,
            SessionMode::Streaming | SessionMode::StreamingSources => false,
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
        include_bytes!("audio_http/player.html"),
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
    append_wav_header(&mut head, u32::MAX - 36);
    head
}

fn append_wav_header(head: &mut Vec<u8>, bytes: u32) {
    head.extend_from_slice(b"RIFF");
    head.extend_from_slice(&(bytes + 36).to_le_bytes());
    head.extend_from_slice(b"WAVEfmt ");
    head.extend_from_slice(&16u32.to_le_bytes());
    head.extend_from_slice(&1u16.to_le_bytes());
    head.extend_from_slice(&(CHANNELS as u16).to_le_bytes());
    head.extend_from_slice(&(SAMPLE_RATE as u32).to_le_bytes());
    head.extend_from_slice(&((SAMPLE_RATE * CHANNELS * 2) as u32).to_le_bytes());
    head.extend_from_slice(&((CHANNELS * 2) as u16).to_le_bytes());
    head.extend_from_slice(&16u16.to_le_bytes());
    head.extend_from_slice(b"data");
    head.extend_from_slice(&bytes.to_le_bytes());
}

fn clip_request_id(target: &str) -> Option<u64> {
    let query = target.split_once('?')?.1;
    let mut id = None;
    for part in query.split('&') {
        let (key, value) = part.split_once('=')?;
        if key == "id" {
            if id.is_some() { return None; }
            id = Some(value.parse().ok()?);
        }
    }
    id
}

fn start_clip_response(vnet: &VNet, session: &mut AudioHttpSession, id: Option<u64>) -> bool {
    let clip = SINK.lock().snapshot();
    let Some(clip) = clip.filter(|clip| Some(clip.id) == id) else {
        let response = response_with_body("HTTP/1.1 409 Conflict\r\n", "text/plain; charset=utf-8", b"clip no longer available\n");
        return !send_fixed_response(vnet, session, &response);
    };
    let mut head = format!("HTTP/1.1 200 OK\r\nContent-Type: audio/wav\r\nCache-Control: no-store\r\nContent-Length: {}\r\nX-TRUEOS-Clip-Id: {}\r\nConnection: close\r\n\r\n", clip.pcm.len() + 44, clip.id).into_bytes();
    append_wav_header(&mut head, clip.pcm.len() as u32);
    if !send_tcp_bytes(vnet, session.handle, &head) {
        close_session(vnet, session.handle);
        return true;
    }
    session.mode = SessionMode::SendingClip;
    session.pending_bytes = head.len();
    session.sent_bytes = 0;
    session.rx.clear();
    session.clip = Some(ClipTransfer { clip, offset: 0 });
    session.refresh_fixed_deadline();
    false
}

fn send_clip_tick(vnet: &VNet, session: &mut AudioHttpSession) -> bool {
    const WINDOW: usize = SAMPLE_RATE * CHANNELS * 2 / 4;
    let transfer = session.clip.as_mut().expect("clip response owns its PCM");
    while transfer.offset < transfer.clip.pcm.len() {
        let room = WINDOW.saturating_sub(session.pending_bytes.saturating_sub(session.sent_bytes));
        let count = room.min(api::MAX_MSG).min(transfer.clip.pcm.len() - transfer.offset);
        if count == 0 { break; }
        let bytes = &transfer.clip.pcm[transfer.offset..transfer.offset + count];
        if vnet.submit(api::Command::SendTcp { handle: session.handle, data: api::ByteBuf::from_slice_trunc(bytes) }).is_err() {
            break; // Keep the cursor for bounded backpressure retry.
        }
        transfer.offset += count;
        session.pending_bytes += count;
    }
    if transfer.offset == transfer.clip.pcm.len() && session.sent_bytes >= session.pending_bytes {
        close_session(vnet, session.handle);
        return true;
    }
    false
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
        Some(WAV_PATH) | Some(PCM_PATH) => {
            if active_streams >= MAX_STREAM_CLIENTS {
                let response = busy_response();
                return !send_fixed_response(vnet, session, response.as_slice());
            }
            let sources = path == Some(PCM_PATH);
            let head = if sources {
                b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n".to_vec()
            } else {
                wav_stream_head()
            };
            if !send_tcp_bytes(vnet, session.handle, head.as_slice()) {
                close_session(vnet, session.handle);
                return true;
            }
            session.mode = if sources {
                SessionMode::StreamingSources
            } else {
                SessionMode::Streaming
            };
            session.rx.clear();
            session.pending_bytes = head.len();
            session.sent_bytes = 0;
            false
        }
        Some(CLIP_PATH) => {
            if active_streams >= MAX_STREAM_CLIENTS {
                let response = busy_response();
                return !send_fixed_response(vnet, session, &response);
            }
            start_clip_response(vnet, session, target.and_then(clip_request_id))
        }
        _ => {
            let response = not_found_response();
            !send_fixed_response(vnet, session, response.as_slice())
        }
    }
}

fn status_response(listeners: usize) -> Vec<u8> {
    let sink = SINK.lock();
    let clip_id = sink.snapshot().map_or(0, |clip| clip.id);
    let body = format!(
        "{{\"listeners\":{},\"capacity\":{},\"clip_id\":\"{}\",\"playing\":{},\"producers\":{},\"producer_capacity\":{}}}\n",
        listeners, MAX_STREAM_CLIENTS, clip_id, sink.busy(), sink.active_count(), sink::MAX_PRODUCERS
    );
    response_with_body(
        "HTTP/1.1 200 OK\r\n",
        "application/json; charset=utf-8",
        body.as_bytes(),
    )
}

fn render_audio_block(
    sink: &mut Sink,
    mixed: &mut [u8],
    sources: &mut [u8],
) -> [sink::RenderReport; sink::MAX_PRODUCERS] {
    // The live PCM endpoint sends two u16le frame counts, then one fixed
    // stereo PCM block per producer. Unused frames in each slot are zero.
    let (prefix, samples) = sources.split_at_mut(PCM_PREFIX_BYTES);
    let (a, b) = samples.split_at_mut(BLOCK_BYTES);
    let reports = sink.render_sources(a, b);
    for (count, report) in prefix.chunks_exact_mut(2).zip(&reports) {
        count.copy_from_slice(&((report.bytes / FRAME_BYTES) as u16).to_le_bytes());
    }
    sink::mix_center(mixed, &a[..reports[0].bytes], &b[..reports[1].bytes]);
    reports
}

fn stream_audio_tick(vnet: &VNet, session: &mut AudioHttpSession, bytes: &[u8]) -> bool {
    // A slow listener cannot retain an unbounded TCP send backlog.
    let max_pending_bytes = (SAMPLE_RATE * FRAME_BYTES / 4) * if session.mode == SessionMode::StreamingSources { 2 } else { 1 };
    if session.pending_bytes.saturating_sub(session.sent_bytes) > max_pending_bytes
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
        .filter(|session| matches!(session.mode, SessionMode::Streaming | SessionMode::StreamingSources | SessionMode::SendingClip))
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
    let mut pcm_bytes = alloc::vec![0; BLOCK_BYTES];
    let mut source_bytes = alloc::vec![0; PCM_PREFIX_BYTES + sink::MAX_PRODUCERS * BLOCK_BYTES];
    loop {
        if endpoint_discovery_ticks == 0 {
            audio_http_add_endpoints(&mut endpoints);
        }
        endpoint_discovery_ticks = (endpoint_discovery_ticks + 1) % 100;

        let now = Instant::now();
        let due = now >= next_send;
        if due {
            // Advance the timeline instead of accumulating polling latency.
            next_send = next_send + Duration::from_millis(SEND_MS as u64);
            if now >= next_send + Duration::from_millis(100) {
                next_send = now + Duration::from_millis(SEND_MS as u64);
            }
            let (expired, reports) = {
                let mut sink = SINK.lock();
                let expired = sink.expire(now.as_millis());
                (expired, render_audio_block(&mut sink, &mut pcm_bytes, &mut source_bytes))
            };
            for id in expired.into_iter().flatten() {
                crate::log!("audio-http: progressive clip={} expired\n", id);
            }
            for report in reports {
                if let Some((id, started_ms)) = report.first {
                    crate::log!(
                        "audio-http: progressive clip={} first_render_ms={}\n",
                        id, now.as_millis().saturating_sub(started_ms)
                    );
                }
                if let Some((id, count)) = report.underrun {
                    crate::log!("audio-http: progressive clip={} underruns={}\n", id, count);
                }
            }
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
                            SessionMode::SendingFixed | SessionMode::Streaming | SessionMode::StreamingSources | SessionMode::SendingClip => {}
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
                            if session.mode == SessionMode::SendingClip { session.refresh_fixed_deadline(); }
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
                let session = &mut endpoint.sessions[idx];
                let remove = match session.mode {
                    SessionMode::Streaming if due => stream_audio_tick(&endpoint.vnet, session, &pcm_bytes),
                    SessionMode::StreamingSources if due => stream_audio_tick(&endpoint.vnet, session, &source_bytes),
                    SessionMode::SendingClip => send_clip_tick(&endpoint.vnet, session),
                    _ => false,
                };
                if remove {
                    endpoint.sessions.remove(idx);
                } else {
                    idx += 1;
                }
            }
        }

        Timer::after(Duration::from_millis(POLL_MS)).await;
    }
}
