//! A single native HTTP/1 exchange. Redirect/resume/storage policy stays in apps.
extern crate alloc;
use super::{NetProfile, Queue, VNet, http_request_cabi::ResponseSink};
use crate::net::tls_socket::{self, TlsCommand, TlsEvent, TlsTimeouts};
use crate::r::cabi_codes::*;
use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use core::{
    convert::Infallible,
    future::Future,
    net::{IpAddr, SocketAddr},
    pin::Pin,
    sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering},
    task::{Context, Poll},
};
use hyper::{
    body::{Body, Bytes, Frame, SizeHint},
    io,
    rt::{Read, ReadBufCursor, Write},
};
use spin::Mutex;
use trueos_time::{Duration, Instant, Timer};
use v::{
    vnet as api,
    vnetfs::http::{Header, Method, RequestOwned, ResponseHead},
};
const SLOT_MAX: usize = 64;
const CHUNK: usize = 32 * 1024;
const IO_TICK_MS: u64 = 1;
static SLOTS: Mutex<Vec<Arc<Slot>>> = Mutex::new(Vec::new());
struct Slot {
    device: usize,
    owner: &'static str,
    net: Arc<VNet>,
    cmds: &'static Queue<TlsCommand>,
    events: &'static Queue<TlsEvent>,
    busy: AtomicBool,
}
fn try_slot(device: usize) -> Option<Arc<Slot>> {
    let mut slots = SLOTS.lock();
    if let Some(slot) = slots.iter().find(|slot| {
        slot.device == device
            && slot
                .busy
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
    }) {
        return Some(slot.clone());
    }
    if slots.len() >= SLOT_MAX {
        return None;
    }
    let net = Arc::new(VNet::open_with_event_queue_depth(device, 64)?);
    let owner = Box::leak(format!("native-http-slot-{}@{}", slots.len(), device).into_boxed_str());
    let cmds = Queue::new_leaked(owner, 4);
    let events = Queue::new_leaked(owner, 4);
    tls_socket::register_tls_app_queues(owner, cmds, events);
    let slot = Arc::new(Slot {
        device,
        owner,
        net,
        cmds,
        events,
        busy: AtomicBool::new(true),
    });
    slots.push(slot.clone());
    Some(slot)
}
async fn cancelled_or<F: Future>(
    future: F,
    sink: &ResponseSink,
    ms: u32,
    error: i32,
) -> Result<F::Output, i32> {
    let mut future = core::pin::pin!(future);
    let start = Instant::now();
    let mut cancelled = core::pin::pin!(sink.cancelled());
    let mut tick = core::pin::pin!(Timer::after(Duration::from_millis(IO_TICK_MS)));
    core::future::poll_fn(|cx| {
        if cancelled.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Err(FS_ERR_NOT_FOUND));
        }
        if ms != 0 && Instant::now().saturating_duration_since(start).as_millis() >= u64::from(ms) {
            return Poll::Ready(Err(error));
        }
        if let Poll::Ready(result) = future.as_mut().poll(cx) {
            return Poll::Ready(Ok(result));
        }
        if tick.as_mut().poll(cx).is_ready() {
            tick.set(Timer::after(Duration::from_millis(IO_TICK_MS)));
            cx.waker().wake_by_ref();
        }
        Poll::Pending
    })
    .await
}
struct Empty;
impl Body for Empty {
    type Data = Bytes;
    type Error = Infallible;
    fn poll_frame(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        Poll::Ready(None)
    }
    fn is_end_stream(&self) -> bool {
        true
    }
    fn size_hint(&self) -> SizeHint {
        SizeHint::with_exact(0)
    }
}
struct Activity {
    last: Mutex<Instant>,
    error: AtomicI32,
    paused: AtomicBool,
    read_budget: AtomicUsize,
}
struct Transport {
    slot: Arc<Slot>,
    handle: api::NetHandle,
    tls: bool,
    rx: Vec<u8>,
    offset: usize,
    closed: bool,
    activity: Arc<Activity>,
}
impl Transport {
    fn copy(&mut self, buf: &mut ReadBufCursor<'_>) -> bool {
        if self.offset == self.rx.len() {
            return false;
        }
        let n = buf
            .remaining()
            .min(self.rx.len() - self.offset)
            .min(self.activity.read_budget.load(Ordering::Relaxed));
        if n == 0 {
            return false;
        }
        self.activity.read_budget.fetch_sub(n, Ordering::Relaxed);
        buf.put_slice(&self.rx[self.offset..self.offset + n]);
        self.offset += n;
        if self.offset == self.rx.len() {
            self.rx.clear();
            self.offset = 0;
        }
        true
    }
    fn fail(&self, code: i32) -> io::Error {
        self.activity.error.store(code, Ordering::Release);
        io::Error::new(io::ErrorKind::Other, "native HTTP transport failed")
    }
}
impl Read for Transport {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        mut buf: ReadBufCursor<'_>,
    ) -> Poll<Result<(), io::Error>> {
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        if self.activity.read_budget.load(Ordering::Relaxed) == 0 {
            return Poll::Pending;
        }
        if self.copy(&mut buf) {
            return Poll::Ready(Ok(()));
        }
        if self.closed {
            return Poll::Ready(Ok(()));
        }
        loop {
            if self.tls {
                match self.slot.events.pop() {
                    Some(TlsEvent::Data { handle, data }) if handle == self.handle => {
                        self.rx = data;
                        *self.activity.last.lock() = Instant::now();
                        self.copy(&mut buf);
                        return Poll::Ready(Ok(()));
                    }
                    Some(TlsEvent::Closed { handle }) if handle == self.handle => {
                        self.closed = true;
                        return Poll::Ready(Ok(()));
                    }
                    Some(TlsEvent::Error { .. } | TlsEvent::TlsError { .. }) => {
                        return Poll::Ready(Err(self.fail(NET_ERR_TLS)));
                    }
                    Some(_) => continue,
                    None => return Poll::Pending,
                }
            } else {
                match self.slot.net.pop_event() {
                    Some(api::Event::TcpData { handle, data }) if handle == self.handle => {
                        self.rx = data.as_slice().to_vec();
                        *self.activity.last.lock() = Instant::now();
                        self.copy(&mut buf);
                        return Poll::Ready(Ok(()));
                    }
                    Some(api::Event::Closed { handle } | api::Event::TcpReadClosed { handle })
                        if handle == self.handle =>
                    {
                        self.closed = true;
                        return Poll::Ready(Ok(()));
                    }
                    Some(api::Event::Error { .. }) => {
                        return Poll::Ready(Err(self.fail(NET_ERR_HTTP)));
                    }
                    Some(_) => continue,
                    None => return Poll::Pending,
                }
            }
        }
    }
}
impl Write for Transport {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        if self.closed {
            return Poll::Ready(Err(self.fail(NET_ERR_HTTP)));
        }
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let n = buf.len().min(CHUNK);
        let result = if self.tls {
            self.slot.cmds.push(TlsCommand::Send {
                handle: self.handle,
                data: buf[..n].to_vec(),
            })
        } else {
            self.slot.net.try_send_tcp_chunk(self.handle, &buf[..n])
        };
        match result {
            Ok(()) => Poll::Ready(Ok(n)),
            Err(()) => Poll::Pending,
        }
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }
}
fn parse(request: &RequestOwned) -> Result<(hyper::Uri, String, u16, bool), i32> {
    if request.url.contains('#') {
        return Err(NET_ERR_BAD_URL);
    }
    let uri: hyper::Uri = request.url.parse().map_err(|_| NET_ERR_BAD_URL)?;
    let tls = match uri.scheme_str() {
        Some("https") => true,
        Some("http") => false,
        _ => return Err(NET_ERR_BAD_URL),
    };
    let authority = uri.authority().ok_or(NET_ERR_BAD_URL)?;
    if authority.as_str().contains('@') {
        return Err(NET_ERR_BAD_URL);
    }
    let host = authority
        .host()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();
    if host.is_empty() {
        return Err(NET_ERR_BAD_URL);
    }
    let port = uri.port_u16().unwrap_or(if tls { 443 } else { 80 });
    if port == 0 {
        return Err(NET_ERR_BAD_URL);
    }
    Ok((uri, host, port, tls))
}
async fn resolve(
    slot: &Slot,
    host: &str,
    port: u16,
    request: &RequestOwned,
    sink: &ResponseSink,
) -> Result<SocketAddr, i32> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(SocketAddr::new(ip, port));
    }
    let cfg = super::dns::DnsConfig::for_device(slot.device).with_timeout_ms(5_000);
    let resolving = async {
        loop {
            match super::dns::resolve_ipv4_for_device(slot.device, host, cfg).await {
                Ok(ip) => return Ok(SocketAddr::new(IpAddr::V4(ip.into()), port)),
                Err(super::dns::DnsError::Timeout) if request.timeouts.connect_ms == 0 => continue,
                Err(_) => break,
            }
        }
        super::dns::resolve_ipv6_for_device(slot.device, host, cfg)
            .await
            .map(|ip| SocketAddr::new(IpAddr::V6(ip.into()), port))
            .map_err(|_| NET_ERR_TIMEOUT_DNS)
    };
    cancelled_or(
        resolving,
        sink,
        request.timeouts.connect_ms,
        NET_ERR_TIMEOUT_DNS,
    )
    .await?
}
async fn connect(
    slot: &Arc<Slot>,
    request: &RequestOwned,
    host: &str,
    remote: SocketAddr,
    tls: bool,
    sink: &ResponseSink,
) -> Result<api::NetHandle, i32> {
    if tls {
        tls_socket::open_reusable(
            slot.owner,
            slot.net.clone(),
            remote,
            host,
            TlsTimeouts {
                // The producer owns explicit phase deadlines; the legacy TLS
                // timeout events carry strings rather than typed phase codes.
                connect_ms: 0,
                tls_ms: 0,
                idle_ms: 0,
            },
        )
        .map_err(|_| NET_ERR_TLS)?;
    } else {
        let command = match remote {
            SocketAddr::V4(a) => api::Command::OpenTcpConnect {
                remote: api::EndpointV4::new(a.ip().octets(), a.port()),
            },
            SocketAddr::V6(a) => api::Command::OpenTcpConnectV6 {
                remote: api::EndpointV6::new(a.ip().octets(), a.port()),
            },
        };
        cancelled_or(
            async {
                loop {
                    if slot.net.submit(command.clone()).is_ok() {
                        break;
                    }
                    Timer::after(Duration::from_millis(1)).await;
                }
            },
            sink,
            request.timeouts.connect_ms,
            NET_ERR_TIMEOUT_CONNECT,
        )
        .await?;
    }
    let mut opened = None;
    let mut tcp_ready = false;
    let mut phase_start = Instant::now();
    loop {
        if sink.is_cancelled() {
            return Err(FS_ERR_NOT_FOUND);
        }
        if tls {
            while let Some(event) = slot.events.pop() {
                match event {
                    TlsEvent::Opened { handle } => {
                        opened = Some(handle);
                    }
                    TlsEvent::Connected { handle } if opened == Some(handle) => return Ok(handle),
                    TlsEvent::Error { .. }
                    | TlsEvent::TlsError { .. }
                    | TlsEvent::Closed { .. } => return Err(NET_ERR_TLS),
                    _ => {}
                }
            }
        } else {
            while let Some(event) = slot.net.pop_event() {
                match event {
                    api::Event::Opened { handle, .. } => opened = Some(handle),
                    api::Event::TcpEstablished { handle, .. } if opened == Some(handle) => {
                        return Ok(handle);
                    }
                    api::Event::Error { .. } | api::Event::Closed { .. } => {
                        return Err(NET_ERR_HTTP);
                    }
                    _ => {}
                }
            }
        }
        if tls && !tcp_ready && tls_socket::reusable_tcp_established(slot.owner) {
            tcp_ready = true;
            phase_start = Instant::now();
        }
        let ms = if tcp_ready {
            request.timeouts.tls_ms
        } else {
            request.timeouts.connect_ms
        };
        if ms != 0
            && Instant::now()
                .saturating_duration_since(phase_start)
                .as_millis()
                >= u64::from(ms)
        {
            return Err(if tcp_ready {
                NET_ERR_TIMEOUT_TLS
            } else {
                NET_ERR_TIMEOUT_CONNECT
            });
        }
        cancelled_or(Timer::after(Duration::from_millis(1)), sink, 0, 0).await?;
    }
}
async fn exchange(
    slot: &Arc<Slot>,
    request: &RequestOwned,
    sink: &ResponseSink,
) -> Result<(), i32> {
    let (uri, host, port, tls) = parse(request)?;
    let remote = resolve(slot, &host, port, request, sink).await?;
    let handle = connect(slot, request, &host, remote, tls, sink).await?;
    let activity = Arc::new(Activity {
        last: Mutex::new(Instant::now()),
        error: AtomicI32::new(0),
        paused: AtomicBool::new(false),
        read_budget: AtomicUsize::new(CHUNK),
    });
    let io = Transport {
        slot: slot.clone(),
        handle,
        tls,
        rx: Vec::new(),
        offset: 0,
        closed: false,
        activity: activity.clone(),
    };
    let mut builder = hyper::client::conn::http1::Builder::new();
    builder.max_headers(128).max_buf_size(32 * 1024);
    let (mut sender, connection) = builder
        .handshake::<_, Empty>(io)
        .await
        .map_err(|_| NET_ERR_HTTP)?;
    let mut builder = hyper::Request::builder()
        .method(match request.method {
            Method::Get => hyper::Method::GET,
            Method::Head => hyper::Method::HEAD,
        })
        .uri(uri.path_and_query().map(|v| v.as_str()).unwrap_or("/"))
        .header(hyper::header::HOST, uri.authority().unwrap().as_str());
    let mut accept_encoding = false;
    for header in &request.headers {
        if [
            "host",
            "content-length",
            "transfer-encoding",
            "connection",
            "upgrade",
            "trailer",
        ]
        .iter()
        .any(|reserved| header.name.eq_ignore_ascii_case(reserved))
        {
            return Err(FS_ERR_BAD_PARAM);
        }
        accept_encoding |= header.name.eq_ignore_ascii_case("accept-encoding");
        builder = builder.header(header.name.as_str(), header.value.as_slice());
    }
    if !accept_encoding {
        builder = builder.header(hyper::header::ACCEPT_ENCODING, "identity");
    }
    let outgoing = builder.body(Empty).map_err(|_| FS_ERR_BAD_PARAM)?;
    let exchanging = async {
        let mut response = sender.send_request(outgoing).await.map_err(|error| {
            if error.is_parse() {
                NET_ERR_RESPONSE_FRAMING
            } else {
                NET_ERR_HTTP
            }
        })?;
        if response
            .headers()
            .contains_key(hyper::header::TRANSFER_ENCODING)
            && response
                .headers()
                .contains_key(hyper::header::CONTENT_LENGTH)
        {
            return Err(NET_ERR_RESPONSE_FRAMING);
        }
        let headers = response
            .headers()
            .iter()
            .map(|(name, value)| Header {
                name: name.as_str().to_string(),
                value: value.as_bytes().to_vec(),
            })
            .collect();
        sink.publish_head(ResponseHead {
            status: response.status().as_u16(),
            url: request.url.clone(),
            headers,
        })?;
        if request.method == Method::Head {
            return Ok(());
        }
        while let Some(frame) =
            core::future::poll_fn(|cx| Pin::new(response.body_mut()).poll_frame(cx)).await
        {
            let frame = frame.map_err(|_| NET_ERR_RESPONSE_FRAMING)?;
            if let Ok(bytes) = frame.into_data() {
                for chunk in bytes.chunks(CHUNK) {
                    activity.paused.store(true, Ordering::Release);
                    let result = sink.push_body(chunk).await;
                    activity.paused.store(false, Ordering::Release);
                    *activity.last.lock() = Instant::now();
                    result?;
                }
            }
        }
        Ok(())
    };
    let mut exchanging = core::pin::pin!(exchanging);
    let mut connection = core::pin::pin!(connection);
    let mut completed = false;
    loop {
        let result = cancelled_or(
            core::future::poll_fn(|cx| {
                if let Poll::Ready(result) = exchanging.as_mut().poll(cx) {
                    return Poll::Ready(result);
                }
                if !completed && !activity.paused.load(Ordering::Acquire) {
                    activity.read_budget.store(CHUNK, Ordering::Relaxed);
                    if let Poll::Ready(result) = connection.as_mut().poll(cx) {
                        completed = true;
                        if let Poll::Ready(result) = exchanging.as_mut().poll(cx) {
                            return Poll::Ready(result);
                        }
                        // Preserve an in-flight Incoming frame across body-credit waits,
                        // even when its connection has already completed.
                        let _ = result;
                    }
                }
                Poll::Pending
            }),
            sink,
            1,
            NET_PENDING_LOCAL,
        )
        .await;
        match result {
            Ok(result) => {
                let underlying = activity.error.load(Ordering::Acquire);
                return result.map_err(|err| if underlying != 0 { underlying } else { err });
            }
            Err(NET_PENDING_LOCAL) => {}
            Err(error) => return Err(error),
        }
        if activity.paused.load(Ordering::Acquire) {
            *activity.last.lock() = Instant::now();
        } else if request.timeouts.read_idle_ms != 0
            && Instant::now()
                .saturating_duration_since(*activity.last.lock())
                .as_millis()
                >= u64::from(request.timeouts.read_idle_ms)
        {
            return Err(NET_ERR_TIMEOUT_BODY);
        }
    }
}
const NET_PENDING_LOCAL: i32 = -116;
/// Service wrapper keeps this future alive through the real two-level fence.
pub async fn start_exchange(request: RequestOwned, sink: ResponseSink) {
    let result = cancelled_or(
        crate::r::readiness::wait_for(
            crate::r::readiness::NET_ANY_CONFIGURED | crate::r::readiness::TLS_SOCKET_SERVICE_READY,
        ),
        &sink,
        request.timeouts.connect_ms,
        NET_ERR_TIMEOUT_CONNECT,
    )
    .await;
    if let Err(error) = result {
        sink.finish(Err(error));
        return;
    }
    let Some(device) = NetProfile::default().resolve_device_index() else {
        sink.finish(Err(NET_ERR_HTTP));
        return;
    };
    let slot = loop {
        if sink.is_cancelled() {
            sink.finish(Err(FS_ERR_NOT_FOUND));
            return;
        }
        if let Some(slot) = try_slot(device) {
            break slot;
        }
        Timer::after(Duration::from_millis(1)).await;
    };
    let result = exchange(&slot, &request, &sink).await;
    sink.finish(result);
    // Stop the TLS producer first, then fence all old lower commands/sockets and
    // drain lower events. The slot stays busy/quarantined until this ACK arrives.
    tls_socket::retire_reusable(slot.owner);
    slot.net.reset_for_reuse().await;
    slot.busy.store(false, Ordering::Release);
}
