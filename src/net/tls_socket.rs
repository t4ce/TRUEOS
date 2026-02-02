extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use embassy_executor::task;
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

use trueos_v::vnet as vnet;

use crate::v::net::{Queue, VNet};
use crate::net::tls::{KernelTlsRng, TlsClient, TlsClientConfig, TlsError, TlsRoots, TlsTime};

static TLS_APP_QUEUES: Mutex<Vec<TlsAppQueues>> = Mutex::new(Vec::new());
static TLS_CONN_SEQ: AtomicU32 = AtomicU32::new(1);
static TLS_BLOCK_ON_PUMP_LAST_TICK: AtomicU64 = AtomicU64::new(0);

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
    let (base, suffix) = owner.rsplit_once('@')?;
    if base.is_empty() || suffix.is_empty() {
        return None;
    }
    if !suffix.as_bytes().iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    suffix.parse::<usize>().ok()
}

#[derive(Clone)]
pub enum TlsCommand {
    /// Open a TCP connection and layer TLS over it.
    OpenTcpConnect {
        remote: vnet::EndpointV4,
        server_name: &'static str,
        cfg: TlsClientConfig,
        roots: TlsRoots,
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
            if !data.is_empty() {
                if let Some(handle) = conn.handle {
                    send_tcp_all(&conn.net, handle, &data);
                }
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
    if conn.tls.is_connected() {
        if let Some(handle) = conn.handle {
            crate::log!(
                "tls-socket: tls connected owner={} handle={}\n",
                conn.user_owner,
                handle.0
            );
            // Only mark as notified once the event is successfully queued.
            // Otherwise the app may never observe `Connected` and will stall.
            if push_tls_event(conn.user_owner, TlsEvent::Connected { handle }) {
                conn.connected_notified = true;
            }
        }
    }
}

fn tls_socket_tick_once() {
    if !crate::v::readiness::is_set(crate::v::readiness::NET_GATEWAY_REACHABLE) {
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
                } => {
                    crate::log!(
                        "tls-socket: open request owner={} remote={}.{}.{}.{}:{} sni={}\n",
                        owner,
                        remote.addr[0],
                        remote.addr[1],
                        remote.addr[2],
                        remote.addr[3],
                        remote.port,
                        server_name
                    );

                    let seq = TLS_CONN_SEQ.fetch_add(1, Ordering::Relaxed);
                    let dev_idx = owner_device_index(owner).unwrap_or(0);
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
                    let tls = match TlsClient::new(
                        &cfg,
                        &roots,
                        server_name,
                        &mut rng,
                        &KERNEL_TIME,
                    ) {
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
                    };

                    let _ = conn.net.submit(vnet::Command::OpenTcpConnect { remote });
                    conns.push(conn);
                }
                TlsCommand::Send { handle, data } => {
                    if let Some(conn) = conns.iter_mut().find(|c| c.matches_handle(handle)) {
                        if conn.closed {
                            let _ = push_tls_event(conn.user_owner, TlsEvent::Closed { handle });
                            continue;
                        }
                        if let Err(e) = conn.tls.write_plaintext(&data) {
                            conn.closed = true;
                            let _ =
                                push_tls_event(conn.user_owner, TlsEvent::TlsError { err: e });
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
                        crate::log!(
                            "tls-socket: tcp opened owner={} handle={}\n",
                            conns[idx].user_owner,
                            handle.0
                        );
                        let _ =
                            push_tls_event(conns[idx].user_owner, TlsEvent::Opened { handle });
                    }
                }
                vnet::Event::TcpEstablished { handle } => {
                    if conns[idx].handle == Some(handle) {
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
                        let _ = push_tls_event(conns[idx].user_owner, TlsEvent::Data {
                            handle,
                            data: produced,
                        });
                    }

                    flush_outgoing_tls(&mut conns[idx]);
                    maybe_notify_connected(&mut conns[idx]);
                }
                vnet::Event::Closed { handle } => {
                    if conns[idx].handle == Some(handle) {
                        conns[idx].closed = true;
                        crate::log!(
                            "tls-socket: tcp closed owner={} handle={}\n",
                            conns[idx].user_owner,
                            handle.0
                        );
                        let _ =
                            push_tls_event(conns[idx].user_owner, TlsEvent::Closed { handle });
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
            }
        }

        if remove {
            conns.swap_remove(idx);
        } else {
            idx += 1;
        }
    }
}

/// Pump TLS socket service from synchronous contexts (e.g. inside `time::block_on`).
///
/// Rate-limited to ~1kHz.
pub fn pump_block_on_hook() {
    let now = embassy_time_driver::now();
    let last = TLS_BLOCK_ON_PUMP_LAST_TICK.load(Ordering::Relaxed);
    if last == now {
        return;
    }
    if TLS_BLOCK_ON_PUMP_LAST_TICK
        .compare_exchange(last, now, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    tls_socket_tick_once();
}

#[task]
pub async fn tls_socket_service_task() {
    crate::log!("tls-socket: service running\n");

    loop {
        tls_socket_tick_once();
        Timer::after(EmbassyDuration::from_millis(5)).await;
    }
}
