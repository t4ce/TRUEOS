extern crate alloc;

#[path = "tls_flow.rs"]
mod flow;
use flow::{Delivery, PendingSend};

use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::fmt;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use core::task::{Context, Poll};

use atomic_waker::AtomicWaker;
use spin::Mutex;
use trueos_executor::task;
use trueos_time::{Duration as EmbassyDuration, Instant, Timer};

use v::vnet;

use crate::net::tls::{KernelTlsRng, TlsClient, TlsClientConfig, TlsError, TlsRoots, TlsTime};
use crate::r::net::{Queue, VNet};

static TLS_APP_QUEUES: Mutex<Vec<TlsAppQueues>> = Mutex::new(Vec::new());
static TLS_CONN_SEQ: AtomicU32 = AtomicU32::new(1);
const TLS_PRODUCTIVE_PASS_SOFT_CAP: usize = 32;
const TLS_SERVICE_SLEEP_MS: u64 = 8;

#[derive(Clone, Copy, Debug)]
pub struct TlsTimeouts {
    pub connect_ms: u32,
    pub tls_ms: u32,
    pub idle_ms: u32,
}

impl Default for TlsTimeouts {
    fn default() -> Self {
        Self {
            connect_ms: 30_000,
            tls_ms: 30_000,
            idle_ms: 120_000,
        }
    }
}

static TLS_OWNER_TIMEOUTS: Mutex<Vec<(&'static str, TlsTimeouts)>> = Mutex::new(Vec::new());

fn owner_timeouts(owner: &'static str) -> TlsTimeouts {
    let guard = TLS_OWNER_TIMEOUTS.lock();
    guard
        .iter()
        .find(|(o, _)| *o == owner)
        .map(|(_, t)| *t)
        .unwrap_or_default()
}

struct TlsAppQueues {
    name: &'static str,
    cmds: &'static Queue<TlsCommand>,
    events: &'static Queue<TlsEvent>,
}

pub fn register_tls_app_queues(
    name: &'static str,
    cmds: &'static Queue<TlsCommand>,
    events: &'static Queue<TlsEvent>,
) {
    let mut guard = TLS_APP_QUEUES.lock();
    if guard.iter().any(|entry| entry.name == name) {
        return;
    }
    guard.push(TlsAppQueues { name, cmds, events });
}

fn drain_tls_commands() -> Vec<(&'static str, Vec<TlsCommand>)> {
    let guard = TLS_APP_QUEUES.lock();
    let mut out = Vec::new();
    for entry in guard.iter() {
        let drained = entry.cmds.drain(256);
        if !drained.is_empty() {
            out.push((entry.name, drained));
        }
    }
    out
}

fn try_push_tls_event(target: &'static str, event: TlsEvent) -> Result<(), TlsEvent> {
    let guard = TLS_APP_QUEUES.lock();
    if let Some(entry) = guard.iter().find(|e| e.name == target) {
        entry.events.try_push(event)
    } else {
        Err(event)
    }
}

fn push_tls_event(target: &'static str, event: TlsEvent) -> bool {
    try_push_tls_event(target, event).is_ok()
}

fn discard_tls_events(target: &'static str) {
    let guard = TLS_APP_QUEUES.lock();
    if let Some(entry) = guard.iter().find(|entry| entry.name == target) {
        let _ = entry.events.drain(usize::MAX);
    }
}

fn leak_str(s: alloc::string::String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn owner_device_index(owner: &str) -> Option<usize> {
    crate::net::device_index_from_owner(owner)
}

/// Completion signal for an owner-level TLS cancellation.
pub struct TlsCancelCompletion {
    complete: AtomicBool,
    waker: AtomicWaker,
}

impl TlsCancelCompletion {
    pub fn new() -> Self {
        Self {
            complete: AtomicBool::new(false),
            waker: AtomicWaker::new(),
        }
    }

    pub async fn wait(&self) {
        core::future::poll_fn(|cx: &mut Context<'_>| {
            if self.complete.load(Ordering::Acquire) {
                return Poll::Ready(());
            }
            self.waker.register(cx.waker());
            if self.complete.load(Ordering::Acquire) {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await
    }

    fn finish(&self) {
        self.complete.store(true, Ordering::Release);
        self.waker.wake();
    }
}

#[derive(Clone)]
pub enum TlsCommand {
    /// Open a TCP connection and layer TLS over it.
    OpenTcpConnect {
        remote: vnet::EndpointV4,
        server_name: String,
        cfg: TlsClientConfig,
        roots: TlsRoots,
        timeouts: TlsTimeouts,
    },
    OpenTcpConnectV6 {
        remote: vnet::EndpointV6,
        server_name: String,
        cfg: TlsClientConfig,
        roots: TlsRoots,
        timeouts: TlsTimeouts,
    },
    /// Send plaintext application bytes.
    Send {
        handle: vnet::NetHandle,
        data: Vec<u8>,
    },
    /// Cancel every connection currently owned by this command queue.
    ///
    /// Pending opens remain tracked until a handle arrives so they can be
    /// closed without emitting stale events to a caller that has reused the
    /// same queue pair for a newer request.
    CancelOwner {
        completion: Arc<TlsCancelCompletion>,
    },
    Close {
        handle: vnet::NetHandle,
    },
}

#[derive(Clone)]
pub enum TlsEvent {
    Opened {
        handle: vnet::NetHandle,
    },
    /// TLS handshake complete.
    Connected {
        handle: vnet::NetHandle,
    },
    Data {
        handle: vnet::NetHandle,
        data: Vec<u8>,
    },
    Closed {
        handle: vnet::NetHandle,
    },
    Error {
        msg: &'static str,
    },
    TlsError {
        err: TlsError,
    },
}

struct TlsBytePreview<'a>(&'a [u8]);

impl fmt::Debug for TlsBytePreview<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const EDGE: usize = 10;

        f.write_str("[")?;
        let len = self.0.len();
        let mut first = true;

        let write_byte = |f: &mut fmt::Formatter<'_>, byte: u8, first: &mut bool| {
            if !*first {
                f.write_str(", ")?;
            }
            *first = false;
            write!(f, "{:02X}", byte)
        };

        if len <= EDGE * 2 {
            for &byte in self.0 {
                write_byte(f, byte, &mut first)?;
            }
        } else {
            for &byte in &self.0[..EDGE] {
                write_byte(f, byte, &mut first)?;
            }
            f.write_str(", ... ")?;
            first = true;
            for &byte in &self.0[len - EDGE..] {
                write_byte(f, byte, &mut first)?;
            }
        }

        f.write_str("]")
    }
}

impl fmt::Debug for TlsEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Opened { handle } => write!(f, "Opened {{ handle: {:?} }}", handle),
            Self::Connected { handle } => write!(f, "Connected {{ handle: {:?} }}", handle),
            Self::Data { handle, data } => write!(
                f,
                "Data {{ handle: {:?}, len: {}, data: {:?} }}",
                handle,
                data.len(),
                TlsBytePreview(data.as_slice())
            ),
            Self::Closed { handle } => write!(f, "Closed {{ handle: {:?} }}", handle),
            Self::Error { msg } => write!(f, "Error {{ msg: {:?} }}", msg),
            Self::TlsError { err } => write!(f, "TlsError {{ err: {:?} }}", err),
        }
    }
}

struct KernelTime;

impl TlsTime for KernelTime {
    fn unix_time_seconds(&self) -> Option<u64> {
        crate::r::net::ntp::current_unix_seconds().or_else(crate::time::unix_time_seconds)
    }
}

static KERNEL_TIME: KernelTime = KernelTime;

struct TlsConn {
    user_owner: &'static str,
    net: Arc<VNet>,
    delivery: Delivery<TlsEvent>,
    tx: PendingSend,
    open_pending: Option<vnet::Command>,
    close_pending: bool,
    terminal: bool,
    explicit_timeouts: bool,
    tcp_established: bool,

    handle: Option<vnet::NetHandle>,
    tls: TlsClient,
    connected_notified: bool,
    cancelled: bool,
    closed: bool,

    created_at: Instant,
    last_activity: Instant,
    timeouts: TlsTimeouts,
}

impl TlsConn {
    fn emit(&mut self, event: TlsEvent) -> bool {
        if self.cancelled {
            return false;
        }
        let owner = self.user_owner;
        self.delivery
            .emit(event, |event| try_push_tls_event(owner, event));
        true
    }
    fn flush_events(&mut self) -> bool {
        let owner = self.user_owner;
        self.delivery
            .flush(|event| try_push_tls_event(owner, event))
    }
    fn request_close(&mut self) {
        self.close_pending = true;
        self.retry_commands();
    }
    fn retry_commands(&mut self) {
        if let Some(command) = self.open_pending.take() {
            if self.net.submit(command.clone()).is_err() {
                self.open_pending = Some(command);
            }
        }
        if self.close_pending {
            if let Some(handle) = self.handle {
                if self.net.submit(vnet::Command::Close { handle }).is_ok() {
                    self.close_pending = false;
                }
            }
        }
    }
    fn matches_handle(&self, handle: vnet::NetHandle) -> bool {
        self.handle == Some(handle)
    }
}

static TLS_CONNS: Mutex<Vec<TlsConn>> = Mutex::new(Vec::new());

/// Open TLS over a fixed request slot's existing VNet owner. Unlike legacy
/// convenience opens, zero timeouts here explicitly disable their phases.
pub(crate) fn open_reusable(
    owner: &'static str,
    net: Arc<VNet>,
    remote: core::net::SocketAddr,
    server_name: &str,
    timeouts: TlsTimeouts,
) -> Result<(), TlsError> {
    let cfg = TlsClientConfig::new().with_alpn_protocols(&[b"http/1.1"]);
    let tls = TlsClient::new(
        &cfg,
        &TlsRoots::mozilla(),
        server_name,
        &mut KernelTlsRng::new(),
        &KERNEL_TIME,
    )?
    .with_buffer_limit(512 * 1024);
    let command = match remote {
        core::net::SocketAddr::V4(remote) => vnet::Command::OpenTcpConnect {
            remote: vnet::EndpointV4::new(remote.ip().octets(), remote.port()),
        },
        core::net::SocketAddr::V6(remote) => vnet::Command::OpenTcpConnectV6 {
            remote: vnet::EndpointV6::new(remote.ip().octets(), remote.port()),
        },
    };
    let mut conns = TLS_CONNS.lock();
    if conns.iter().any(|conn| conn.user_owner == owner) {
        return Err(TlsError::Io);
    }
    let mut conn = TlsConn {
        user_owner: owner,
        net,
        delivery: Delivery::new(),
        tx: PendingSend::new(),
        open_pending: Some(command),
        close_pending: false,
        terminal: false,
        explicit_timeouts: true,
        tcp_established: false,
        handle: None,
        tls,
        connected_notified: false,
        cancelled: false,
        closed: false,
        created_at: Instant::now(),
        last_activity: Instant::now(),
        timeouts,
    };
    conn.retry_commands();
    conns.push(conn);
    Ok(())
}

pub(crate) fn reusable_tcp_established(owner: &'static str) -> bool {
    TLS_CONNS
        .lock()
        .iter()
        .any(|conn| conn.user_owner == owner && conn.tcp_established)
}

/// Stop every TLS producer for a reusable slot before the caller submits and
/// awaits the lower VNet owner fence. This is not the lower transport ACK.
pub(crate) fn retire_reusable(owner: &'static str) {
    let mut conns = TLS_CONNS.lock();
    conns.retain(|conn| conn.user_owner != owner);
    let queues = TLS_APP_QUEUES.lock();
    if let Some(entry) = queues.iter().find(|entry| entry.name == owner) {
        let _ = entry.cmds.drain(usize::MAX);
        let _ = entry.events.drain(usize::MAX);
    }
}

fn flush_outgoing_tls(conn: &mut TlsConn) {
    if conn.closed {
        return;
    }
    let Some(handle) = conn.handle else {
        return;
    };
    if !conn.tx.flush(vnet::MAX_MSG, |bytes| {
        conn.net.try_send_tcp_chunk(handle, bytes)
    }) {
        return;
    }
    match conn.tls.take_ciphertext_to_send() {
        Ok(data) => {
            conn.tx.stage(data);
            let _ = conn.tx.flush(vnet::MAX_MSG, |bytes| {
                conn.net.try_send_tcp_chunk(handle, bytes)
            });
        }
        Err(err) => {
            conn.closed = true;
            conn.terminal = true;
            conn.emit(TlsEvent::TlsError { err });
            conn.request_close();
        }
    }
}

fn maybe_notify_connected(conn: &mut TlsConn) {
    if conn.connected_notified || conn.closed || conn.cancelled {
        return;
    }
    if conn.tls.is_connected()
        && let Some(handle) = conn.handle
    {
        crate::log_info!(target: "net"; "tls-socket: tls connected owner={} handle={}\n", conn.user_owner, handle.0);
        // Delivery retains ownership if the consumer's queue is full.
        if conn.emit(TlsEvent::Connected { handle }) {
            conn.connected_notified = true;
        }
    }
}

fn tls_socket_tick_once() -> bool {
    if !crate::r::readiness::is_set(crate::r::readiness::NET_ANY_CONFIGURED) {
        return false;
    }

    let mut did_work = false;
    let mut conns = TLS_CONNS.lock();

    // Handle TLS-level commands from apps.
    let batches = drain_tls_commands();
    if !batches.is_empty() {
        did_work = true;
    }
    for (owner, cmds) in batches {
        for cmd in cmds {
            let (cmd, remote_v6) = match cmd {
                TlsCommand::OpenTcpConnectV6 {
                    remote,
                    server_name,
                    cfg,
                    roots,
                    timeouts,
                } => (
                    TlsCommand::OpenTcpConnect {
                        remote: vnet::EndpointV4::new([0; 4], remote.port),
                        server_name,
                        cfg,
                        roots,
                        timeouts,
                    },
                    Some(remote),
                ),
                cmd => (cmd, None),
            };
            match cmd {
                TlsCommand::OpenTcpConnect {
                    remote,
                    server_name,
                    cfg,
                    roots,
                    timeouts,
                } => {
                    let seq = TLS_CONN_SEQ.fetch_add(1, Ordering::Relaxed);
                    let dev_idx =
                        owner_device_index(owner).unwrap_or_else(crate::net::default_device_index);
                    let Some(net) = VNet::open(dev_idx) else {
                        let msg = leak_str(alloc::format!(
                            "tls-socket: no vnet device={} (owner={})",
                            dev_idx,
                            owner
                        ));
                        let _ = push_tls_event(owner, TlsEvent::Error { msg });
                        continue;
                    };

                    crate::log_info!(target: "net";
                        "tls-socket: net via vnet owner={} conn={} device={}\n",
                        owner,
                        seq,
                        dev_idx
                    );

                    let mut rng = KernelTlsRng::new();
                    let tls = match TlsClient::new(
                        &cfg,
                        &roots,
                        server_name.as_str(),
                        &mut rng,
                        &KERNEL_TIME,
                    ) {
                        Ok(c) => c,
                        Err(e) => {
                            crate::log_info!(target: "net"; "tls-socket: TlsClient::new failed: {:?}\n", e);
                            let _ = push_tls_event(owner, TlsEvent::TlsError { err: e });
                            continue;
                        }
                    };

                    let mut conn = TlsConn {
                        user_owner: owner,
                        net: Arc::new(net),
                        delivery: Delivery::new(),
                        tx: PendingSend::new(),
                        open_pending: None,
                        close_pending: false,
                        terminal: false,
                        explicit_timeouts: false,
                        tcp_established: false,
                        handle: None,
                        tls,
                        connected_notified: false,
                        cancelled: false,
                        closed: false,
                        created_at: Instant::now(),
                        last_activity: Instant::now(),
                        timeouts,
                    };

                    let command = match remote_v6 {
                        Some(remote) => vnet::Command::OpenTcpConnectV6 { remote },
                        None => vnet::Command::OpenTcpConnect { remote },
                    };
                    conn.open_pending = Some(command);
                    conn.retry_commands();
                    conns.push(conn);
                }
                TlsCommand::OpenTcpConnectV6 { .. } => unreachable!(),
                TlsCommand::Send { handle, data } => {
                    if let Some(conn) = conns
                        .iter_mut()
                        .find(|c| c.user_owner == owner && c.matches_handle(handle))
                    {
                        conn.last_activity = Instant::now();
                        if conn.closed {
                            if !conn.cancelled {
                                let _ = conn.emit(TlsEvent::Closed { handle });
                            }
                            continue;
                        }
                        if let Err(e) = conn.tls.write_plaintext(&data) {
                            conn.closed = true;
                            conn.terminal = true;
                            let _ = conn.emit(TlsEvent::TlsError { err: e });
                            conn.request_close();
                            continue;
                        }
                        flush_outgoing_tls(conn);
                        maybe_notify_connected(conn);
                    }
                }
                TlsCommand::CancelOwner { completion } => {
                    for conn in conns.iter_mut().filter(|conn| conn.user_owner == owner) {
                        conn.cancelled = true;
                        conn.closed = true;
                        conn.delivery.clear();
                        if let Some(handle) = conn.handle {
                            conn.request_close();
                        }
                    }
                    discard_tls_events(owner);
                    completion.finish();
                }
                TlsCommand::Close { handle } => {
                    if let Some(conn) = conns
                        .iter_mut()
                        .find(|c| c.user_owner == owner && c.matches_handle(handle))
                    {
                        conn.closed = true;
                        conn.request_close();
                    }
                }
            }
        }
    }

    // Enforce per-owner / per-connection timeouts.
    // This prevents leaks where an app gives up but the tls-socket service keeps a stalled conn alive.
    let now = Instant::now();
    let mut idx = 0;
    while idx < conns.len() {
        conns[idx].retry_commands();
        let _ = conns[idx].flush_events();
        flush_outgoing_tls(&mut conns[idx]);
        maybe_notify_connected(&mut conns[idx]);
        if conns[idx].terminal {
            if !conns[idx].delivery.blocked() && !conns[idx].close_pending {
                conns.swap_remove(idx);
                did_work = true;
            } else {
                idx += 1;
            }
            continue;
        }
        // A slow consumer is not remote inactivity. Resume the idle clock when
        // output capacity returns; TCP retains the unread encrypted prefix.
        if conns[idx].delivery.blocked() {
            conns[idx].last_activity = now;
            idx += 1;
            continue;
        }
        let t = if !conns[idx].explicit_timeouts
            && conns[idx].timeouts.connect_ms == 0
            && conns[idx].timeouts.tls_ms == 0
            && conns[idx].timeouts.idle_ms == 0
        {
            owner_timeouts(conns[idx].user_owner)
        } else {
            conns[idx].timeouts
        };

        let mut remove = false;

        // Connect timeout: TCP has not become established.
        if !conns[idx].tcp_established && t.connect_ms != 0 {
            let elapsed = now
                .saturating_duration_since(conns[idx].created_at)
                .as_millis();
            if elapsed >= t.connect_ms as u64 {
                if !conns[idx].cancelled {
                    let msg = leak_str(alloc::format!(
                        "tls-socket: connect timeout owner={}",
                        conns[idx].user_owner
                    ));
                    let _ = conns[idx].emit(TlsEvent::Error { msg });
                }
                remove = true;
            }
        }

        // TLS timeout: TCP established but handshake did not complete.
        if !remove && conns[idx].tcp_established && !conns[idx].tls.is_connected() && t.tls_ms != 0
        {
            let elapsed = now
                .saturating_duration_since(conns[idx].last_activity)
                .as_millis();
            if elapsed >= t.tls_ms as u64 {
                conns[idx].request_close();
                if !conns[idx].cancelled {
                    let msg = leak_str(alloc::format!(
                        "tls-socket: tls timeout owner={}",
                        conns[idx].user_owner
                    ));
                    let _ = conns[idx].emit(TlsEvent::Error { msg });
                }
                remove = true;
            }
        }

        // Idle timeout after connect.
        if !remove && conns[idx].tls.is_connected() && t.idle_ms != 0 {
            let elapsed = now
                .saturating_duration_since(conns[idx].last_activity)
                .as_millis();
            if elapsed >= t.idle_ms as u64 {
                conns[idx].request_close();
                if !conns[idx].cancelled {
                    let msg = leak_str(alloc::format!(
                        "tls-socket: idle timeout owner={}",
                        conns[idx].user_owner
                    ));
                    let _ = conns[idx].emit(TlsEvent::Error { msg });
                }
                remove = true;
            }
        }

        if remove {
            conns[idx].terminal = true;
            conns[idx].closed = true;
            did_work = true;
        }
        idx += 1;
    }

    // Pump underlying TCP events into TLS and emit plaintext TLS events.
    let mut idx = 0;
    while idx < conns.len() {
        let mut remove = false;
        if conns[idx].terminal || !conns[idx].flush_events() {
            idx += 1;
            continue;
        }

        // Drain a bounded number of events per conn per tick.
        for _ in 0..256 {
            if conns[idx].delivery.blocked() || conns[idx].terminal {
                break;
            }
            let Some(ev) = conns[idx].net.pop_event() else {
                break;
            };
            did_work = true;
            match ev {
                vnet::Event::Opened { handle, kind } => {
                    if kind == vnet::SocketKind::Tcp {
                        conns[idx].handle = Some(handle);
                        conns[idx].last_activity = Instant::now();
                        if conns[idx].cancelled {
                            conns[idx].request_close();
                        } else {
                            let _ = conns[idx].emit(TlsEvent::Opened { handle });
                        }
                    }
                }
                vnet::Event::TcpEstablished { handle, .. } => {
                    if conns[idx].handle == Some(handle) {
                        conns[idx].tcp_established = true;
                        conns[idx].last_activity = Instant::now();
                        if conns[idx].cancelled {
                            conns[idx].request_close();
                            continue;
                        }
                        crate::log_info!(target: "net";
                            "tls-socket: tcp established owner={} handle={}\n",
                            conns[idx].user_owner,
                            handle.0
                        );
                        // Send the already-prepared ClientHello.
                        flush_outgoing_tls(&mut conns[idx]);
                    }
                }
                vnet::Event::TcpData { handle, data } => {
                    if conns[idx].handle != Some(handle) {
                        continue;
                    }
                    if conns[idx].cancelled {
                        conns[idx].request_close();
                        continue;
                    }

                    conns[idx].last_activity = Instant::now();

                    let (produced, error) = conns[idx].tls.ingest_stream(data.as_slice());
                    maybe_notify_connected(&mut conns[idx]);
                    if !produced.is_empty() {
                        conns[idx].emit(TlsEvent::Data {
                            handle,
                            data: produced,
                        });
                    }
                    if let Some(err) = error {
                        conns[idx].emit(TlsEvent::TlsError { err });
                        conns[idx].closed = true;
                        conns[idx].terminal = true;
                        conns[idx].request_close();
                        continue;
                    }

                    flush_outgoing_tls(&mut conns[idx]);
                    maybe_notify_connected(&mut conns[idx]);
                }
                vnet::Event::Closed { handle } => {
                    if conns[idx].handle == Some(handle) {
                        conns[idx].closed = true;
                        if !conns[idx].cancelled {
                            if conns[idx].explicit_timeouts && !conns[idx].tls.peer_closed() {
                                conns[idx].emit(TlsEvent::TlsError { err: TlsError::Io });
                            } else {
                                conns[idx].emit(TlsEvent::Closed { handle });
                            }
                        }
                        remove = true;
                    }
                }
                vnet::Event::Error { msg } => {
                    crate::log_info!(target: "net";
                        "tls-socket: net error owner={} handle={:?} msg={}\n",
                        conns[idx].user_owner,
                        conns[idx].handle.map(|h| h.0),
                        msg
                    );
                    if !conns[idx].cancelled {
                        let _ = conns[idx].emit(TlsEvent::Error { msg });
                    }
                    conns[idx].closed = true;
                    conns[idx].terminal = true;
                    conns[idx].request_close();
                }
                vnet::Event::TcpSent { .. } => {}
                vnet::Event::TcpReadClosed { .. } => {}
                vnet::Event::IpPacket { .. } => {}
                vnet::Event::UdpPacket { .. } => {}
                vnet::Event::UdpPacketV6 { .. } => {}
                vnet::Event::IcmpReply { .. } => {}
                vnet::Event::IcmpReplyV6 { .. } => {}
            }
        }

        if remove {
            conns[idx].terminal = true;
        }
        idx += 1;
    }

    did_work
}

#[task]
pub async fn tls_socket_service_task() {
    async move {
        let profile = crate::cpu::CpuProfile::current();
        crate::log_info!(target: "net";
            "tls-socket: service running cpu_slot={} core_kind={} placement=ecore-preferred-ap\n",
            profile.map(|profile| profile.slot()).unwrap_or(u32::MAX),
            profile
                .map(|profile| profile.core_kind_name())
                .unwrap_or("unknown")
        );
        crate::r::readiness::set(crate::r::readiness::TLS_SOCKET_SERVICE_READY);

        let mut productive_passes = 0usize;
        loop {
            if tls_socket_tick_once() {
                productive_passes = productive_passes.saturating_add(1);
                if productive_passes < TLS_PRODUCTIVE_PASS_SOFT_CAP {
                    Timer::after(EmbassyDuration::from_micros(0)).await;
                    continue;
                }
            }

            // Sleep when the backlog is exhausted, or after the productive
            // pass soft cap so another executor workload gets a fair window.
            productive_passes = 0;
            Timer::after(EmbassyDuration::from_millis(TLS_SERVICE_SLEEP_MS)).await;
        }
    }
    .await;
}
