extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_executor::task;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use spin::Mutex;

use v::vnet;

use crate::net::tls::{KernelTlsRng, TlsClient, TlsClientConfig, TlsError, TlsRoots, TlsTime};
use crate::r::net::{Queue, VNet};

static TLS_APP_QUEUES: Mutex<Vec<TlsAppQueues>> = Mutex::new(Vec::new());
static TLS_CONN_SEQ: AtomicU32 = AtomicU32::new(1);

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

fn push_tls_event(target: &'static str, event: TlsEvent) -> bool {
    let guard = TLS_APP_QUEUES.lock();
    if let Some(entry) = guard.iter().find(|e| e.name == target) {
        entry.events.push(event).is_ok()
    } else {
        false
    }
}

fn leak_str(s: alloc::string::String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn owner_device_index(owner: &str) -> Option<usize> {
    crate::net::device_index_from_owner(owner)
}

#[derive(Clone)]
pub enum TlsCommand {
    /// Open a TCP connection and layer TLS over it.
    OpenTcpConnect {
        remote: vnet::EndpointV4,
        server_name: &'static str,
        cfg: TlsClientConfig,
        roots: TlsRoots,
        timeouts: TlsTimeouts,
    },
    /// Send plaintext application bytes.
    Send {
        handle: vnet::NetHandle,
        data: Vec<u8>,
    },
    Close {
        handle: vnet::NetHandle,
    },
}

#[derive(Clone, Debug)]
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

struct KernelTime;

impl TlsTime for KernelTime {
    fn unix_time_seconds(&self) -> Option<u64> {
        crate::time::unix_time_seconds()
    }
}

static KERNEL_TIME: KernelTime = KernelTime;

struct TlsConn {
    user_owner: &'static str,
    net: VNet,

    handle: Option<vnet::NetHandle>,
    tls: TlsClient,
    connected_notified: bool,
    closed: bool,

    created_at: Instant,
    last_activity: Instant,
    timeouts: TlsTimeouts,
}

impl TlsConn {
    fn matches_handle(&self, handle: vnet::NetHandle) -> bool {
        self.handle == Some(handle)
    }
}

static TLS_CONNS: Mutex<Vec<TlsConn>> = Mutex::new(Vec::new());

fn send_tcp_all(net: &VNet, handle: vnet::NetHandle, data: &[u8]) {
    if data.is_empty() {
        return;
    }

    let mut offset: usize = 0;
    while offset < data.len() {
        let end = (offset + vnet::MAX_MSG).min(data.len());
        let chunk = &data[offset..end];
        let _ = net.submit(vnet::Command::SendTcp {
            handle,
            data: vnet::ByteBuf::from_slice_trunc(chunk),
        });
        offset = end;
    }
}

fn flush_outgoing_tls(conn: &mut TlsConn) {
    if conn.closed {
        return;
    }

    match conn.tls.take_ciphertext_to_send() {
        Ok(data) => {
            if !data.is_empty()
                && let Some(handle) = conn.handle
            {
                send_tcp_all(&conn.net, handle, &data);
            }
        }
        Err(e) => {
            conn.closed = true;
            let _ = push_tls_event(conn.user_owner, TlsEvent::TlsError { err: e });
            if let Some(handle) = conn.handle {
                let _ = conn.net.submit(vnet::Command::Close { handle });
            }
        }
    }
}

fn maybe_notify_connected(conn: &mut TlsConn) {
    if conn.connected_notified {
        return;
    }
    if conn.tls.is_connected()
        && let Some(handle) = conn.handle
    {
        crate::log!("tls-socket: tls connected owner={} handle={}\n", conn.user_owner, handle.0);
        // Only mark as notified once the event is successfully queued.
        // Otherwise the app may never observe `Connected` and will stall.
        if push_tls_event(conn.user_owner, TlsEvent::Connected { handle }) {
            conn.connected_notified = true;
        }
    }
}

fn tls_socket_tick_once() {
    if !crate::r::readiness::is_set(crate::r::readiness::NET_CONFIGURED) {
        return;
    }

    let mut conns = TLS_CONNS.lock();

    // Handle TLS-level commands from apps.
    let batches = drain_tls_commands();
    for (owner, cmds) in batches {
        for cmd in cmds {
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

                    crate::log!(
                        "tls-socket: net via vnet owner={} conn={} device={}\n",
                        owner,
                        seq,
                        dev_idx
                    );

                    let mut rng = KernelTlsRng::new();
                    let tls =
                        match TlsClient::new(&cfg, &roots, server_name, &mut rng, &KERNEL_TIME) {
                            Ok(c) => c,
                            Err(e) => {
                                crate::log!("tls-socket: TlsClient::new failed: {:?}\n", e);
                                let _ = push_tls_event(owner, TlsEvent::TlsError { err: e });
                                continue;
                            }
                        };

                    let conn = TlsConn {
                        user_owner: owner,
                        net,
                        handle: None,
                        tls,
                        connected_notified: false,
                        closed: false,
                        created_at: Instant::now(),
                        last_activity: Instant::now(),
                        timeouts,
                    };

                    let _ = conn.net.submit(vnet::Command::OpenTcpConnect { remote });
                    conns.push(conn);
                }
                TlsCommand::Send { handle, data } => {
                    if let Some(conn) = conns.iter_mut().find(|c| c.matches_handle(handle)) {
                        conn.last_activity = Instant::now();
                        if conn.closed {
                            let _ = push_tls_event(conn.user_owner, TlsEvent::Closed { handle });
                            continue;
                        }
                        if let Err(e) = conn.tls.write_plaintext(&data) {
                            conn.closed = true;
                            let _ = push_tls_event(conn.user_owner, TlsEvent::TlsError { err: e });
                            let _ = conn.net.submit(vnet::Command::Close { handle });
                            continue;
                        }
                        flush_outgoing_tls(conn);
                        maybe_notify_connected(conn);
                    }
                }
                TlsCommand::Close { handle } => {
                    if let Some(conn) = conns.iter_mut().find(|c| c.matches_handle(handle)) {
                        conn.closed = true;
                        let _ = conn.net.submit(vnet::Command::Close { handle });
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
        let t = if conns[idx].timeouts.connect_ms == 0
            && conns[idx].timeouts.tls_ms == 0
            && conns[idx].timeouts.idle_ms == 0
        {
            owner_timeouts(conns[idx].user_owner)
        } else {
            conns[idx].timeouts
        };

        let mut remove = false;

        // Connect timeout: never got a TCP handle.
        if conns[idx].handle.is_none() && t.connect_ms != 0 {
            let elapsed = now
                .saturating_duration_since(conns[idx].created_at)
                .as_millis();
            if elapsed >= t.connect_ms as u64 {
                let msg = leak_str(alloc::format!(
                    "tls-socket: connect timeout owner={}",
                    conns[idx].user_owner
                ));
                let _ = push_tls_event(conns[idx].user_owner, TlsEvent::Error { msg });
                remove = true;
            }
        }

        // TLS timeout: have a handle but handshake didn't complete.
        if !remove && conns[idx].handle.is_some() && !conns[idx].tls.is_connected() && t.tls_ms != 0
        {
            let elapsed = now
                .saturating_duration_since(conns[idx].last_activity)
                .as_millis();
            if elapsed >= t.tls_ms as u64 {
                if let Some(handle) = conns[idx].handle {
                    let _ = conns[idx].net.submit(vnet::Command::Close { handle });
                }
                let msg = leak_str(alloc::format!(
                    "tls-socket: tls timeout owner={}",
                    conns[idx].user_owner
                ));
                let _ = push_tls_event(conns[idx].user_owner, TlsEvent::Error { msg });
                remove = true;
            }
        }

        // Idle timeout after connect.
        if !remove && conns[idx].tls.is_connected() && t.idle_ms != 0 {
            let elapsed = now
                .saturating_duration_since(conns[idx].last_activity)
                .as_millis();
            if elapsed >= t.idle_ms as u64 {
                if let Some(handle) = conns[idx].handle {
                    let _ = conns[idx].net.submit(vnet::Command::Close { handle });
                }
                let msg = leak_str(alloc::format!(
                    "tls-socket: idle timeout owner={}",
                    conns[idx].user_owner
                ));
                let _ = push_tls_event(conns[idx].user_owner, TlsEvent::Error { msg });
                remove = true;
            }
        }

        if remove {
            conns.swap_remove(idx);
        } else {
            idx += 1;
        }
    }

    // Pump underlying TCP events into TLS and emit plaintext TLS events.
    let mut idx = 0;
    while idx < conns.len() {
        let mut remove = false;

        // Drain a bounded number of events per conn per tick.
        for _ in 0..256 {
            let Some(ev) = conns[idx].net.pop_event() else {
                break;
            };
            match ev {
                vnet::Event::Opened { handle, kind } => {
                    if kind == vnet::SocketKind::Tcp {
                        conns[idx].handle = Some(handle);
                        conns[idx].last_activity = Instant::now();
                        let _ = push_tls_event(conns[idx].user_owner, TlsEvent::Opened { handle });
                    }
                }
                vnet::Event::TcpEstablished { handle } => {
                    if conns[idx].handle == Some(handle) {
                        conns[idx].last_activity = Instant::now();
                        crate::log!(
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

                    conns[idx].last_activity = Instant::now();

                    let produced = match conns[idx].tls.ingest_encrypted(data.as_slice()) {
                        Ok(p) => p,
                        Err(e) => {
                            conns[idx].closed = true;
                            let _ = push_tls_event(
                                conns[idx].user_owner,
                                TlsEvent::TlsError { err: e },
                            );
                            let _ = conns[idx].net.submit(vnet::Command::Close { handle });
                            continue;
                        }
                    };

                    if !produced.is_empty() {
                        let _ = push_tls_event(
                            conns[idx].user_owner,
                            TlsEvent::Data {
                                handle,
                                data: produced,
                            },
                        );
                    }

                    flush_outgoing_tls(&mut conns[idx]);
                    maybe_notify_connected(&mut conns[idx]);
                }
                vnet::Event::Closed { handle } => {
                    if conns[idx].handle == Some(handle) {
                        conns[idx].closed = true;
                        let _ = push_tls_event(conns[idx].user_owner, TlsEvent::Closed { handle });
                        remove = true;
                    }
                }
                vnet::Event::Error { msg } => {
                    crate::log!(
                        "tls-socket: net error owner={} handle={:?} msg={}\n",
                        conns[idx].user_owner,
                        conns[idx].handle.map(|h| h.0),
                        msg
                    );
                    let _ = push_tls_event(conns[idx].user_owner, TlsEvent::Error { msg });
                }
                vnet::Event::TcpSent { .. } => {}
                vnet::Event::UdpPacket { .. } => {}
                vnet::Event::UdpPacketV6 { .. } => {}
                vnet::Event::IcmpReply { .. } => {}
                vnet::Event::IcmpReplyV6 { .. } => {}
            }
        }

        if remove {
            conns.swap_remove(idx);
        } else {
            idx += 1;
        }
    }
}

#[task]
pub async fn tls_socket_service_task() {
    async move {
        crate::log!("tls-socket: service running\n");
        crate::r::readiness::set(crate::r::readiness::TLS_SOCKET_SERVICE_READY);

        loop {
            tls_socket_tick_once();
            Timer::after(EmbassyDuration::from_millis(5)).await;
        }
    }
    .await;
}
