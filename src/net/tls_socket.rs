extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_executor::task;
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

use crate::net::adapter::{
    register_app_queues, NetCommand, NetEndpoint, NetEvent, NetHandle, NetQueue, SocketKind,
};
use crate::tls::{KernelTlsRng, TlsClient, TlsClientConfig, TlsError, TlsRoots, TlsTime};

static TLS_APP_QUEUES: Mutex<Vec<TlsAppQueues>> = Mutex::new(Vec::new());
static TLS_CONN_SEQ: AtomicU32 = AtomicU32::new(1);

struct TlsAppQueues {
    name: &'static str,
    cmds: &'static NetQueue<TlsCommand>,
    events: &'static NetQueue<TlsEvent>,
}

pub fn register_tls_app_queues(
    name: &'static str,
    cmds: &'static NetQueue<TlsCommand>,
    events: &'static NetQueue<TlsEvent>,
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
        let drained = entry.cmds.drain(32);
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

#[derive(Clone)]
pub enum TlsCommand {
    /// Open a TCP connection and layer TLS over it.
    OpenTcpConnect {
        remote: NetEndpoint,
        server_name: &'static str,
        cfg: TlsClientConfig,
        roots: TlsRoots,
    },
    /// Send plaintext application bytes.
    Send {
        handle: NetHandle,
        data: Vec<u8>,
    },
    Close {
        handle: NetHandle,
    },
}

#[derive(Clone, Debug)]
pub enum TlsEvent {
    Opened {
        handle: NetHandle,
    },
    /// TLS handshake complete.
    Connected {
        handle: NetHandle,
    },
    Data {
        handle: NetHandle,
        data: Vec<u8>,
    },
    Closed {
        handle: NetHandle,
    },
    Error {
        handle: Option<NetHandle>,
        msg: &'static str,
    },
    TlsError {
        handle: Option<NetHandle>,
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

    net_owner: &'static str,
    net_cmds: &'static NetQueue<NetCommand>,
    net_events: &'static NetQueue<NetEvent>,

    handle: Option<NetHandle>,
    tls: TlsClient,
    connected_notified: bool,
    closed: bool,
}

impl TlsConn {
    fn matches_handle(&self, handle: NetHandle) -> bool {
        self.handle == Some(handle)
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
                    let _ = conn
                        .net_cmds
                        .push(NetCommand::SendTcp { handle, data });
                }
            }
        }
        Err(e) => {
            conn.closed = true;
            let _ = push_tls_event(conn.user_owner, TlsEvent::TlsError {
                handle: conn.handle,
                err: e,
            });
            if let Some(handle) = conn.handle {
                let _ = conn.net_cmds.push(NetCommand::Close { handle });
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
            conn.connected_notified = true;
            let _ = push_tls_event(conn.user_owner, TlsEvent::Connected { handle });
        }
    }
}

#[task]
pub async fn tls_socket_service_task() {
    if crate::net::mac_address().is_none() {
        crate::log!("tls-socket: disabled (no NIC)\n");
        return;
    }

    let mut conns: Vec<TlsConn> = Vec::new();

    loop {
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
                        let seq = TLS_CONN_SEQ.fetch_add(1, Ordering::Relaxed);
                        let net_owner = leak_str(alloc::format!("tls@{}-{}", owner, seq));
                        let net_cmd_name = leak_str(alloc::format!("{}-net-cmd", net_owner));
                        let net_evt_name = leak_str(alloc::format!("{}-net-evt", net_owner));
                        let net_cmds = NetQueue::new_leaked(net_cmd_name, 128);
                        let net_events = NetQueue::new_leaked(net_evt_name, 128);
                        register_app_queues(net_owner, net_cmds, net_events);

                        let mut rng = KernelTlsRng::new();
                        let tls = match TlsClient::new(
                            &cfg,
                            &roots,
                            server_name,
                            &mut rng,
                            &KERNEL_TIME,
                            None,
                        ) {
                            Ok(c) => c,
                            Err(e) => {
                                let _ = push_tls_event(owner, TlsEvent::TlsError {
                                    handle: None,
                                    err: e,
                                });
                                continue;
                            }
                        };

                        let mut conn = TlsConn {
                            user_owner: owner,
                            net_owner,
                            net_cmds,
                            net_events,
                            handle: None,
                            tls,
                            connected_notified: false,
                            closed: false,
                        };

                        let _ = conn.net_cmds.push(NetCommand::OpenTcpConnect { remote });
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
                                let _ = push_tls_event(conn.user_owner, TlsEvent::TlsError {
                                    handle: Some(handle),
                                    err: e,
                                });
                                let _ = conn.net_cmds.push(NetCommand::Close { handle });
                                continue;
                            }
                            flush_outgoing_tls(conn);
                            maybe_notify_connected(conn);
                        }
                    }
                    TlsCommand::Close { handle } => {
                        if let Some(conn) = conns.iter_mut().find(|c| c.matches_handle(handle)) {
                            conn.closed = true;
                            let _ = conn.net_cmds.push(NetCommand::Close { handle });
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
            let drained = conns[idx].net_events.drain(32);
            for ev in drained {
                match ev {
                    NetEvent::Opened { handle, kind } => {
                        if kind == SocketKind::Tcp {
                            conns[idx].handle = Some(handle);
                            let _ = push_tls_event(conns[idx].user_owner, TlsEvent::Opened { handle });
                        }
                    }
                    NetEvent::TcpEstablished { handle } => {
                        if conns[idx].handle == Some(handle) {
                            // Send the already-prepared ClientHello.
                            flush_outgoing_tls(&mut conns[idx]);
                        }
                    }
                    NetEvent::TcpData { handle, data } => {
                        if conns[idx].handle != Some(handle) {
                            continue;
                        }

                        let produced = match conns[idx].tls.ingest_encrypted(&data) {
                            Ok(p) => p,
                            Err(e) => {
                                conns[idx].closed = true;
                                let _ = push_tls_event(conns[idx].user_owner, TlsEvent::TlsError {
                                    handle: Some(handle),
                                    err: e,
                                });
                                let _ = conns[idx].net_cmds.push(NetCommand::Close { handle });
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
                    NetEvent::Closed { handle } => {
                        if conns[idx].handle == Some(handle) {
                            conns[idx].closed = true;
                            let _ = push_tls_event(conns[idx].user_owner, TlsEvent::Closed { handle });
                            remove = true;
                        }
                    }
                    NetEvent::Error { msg } => {
                        let _ = push_tls_event(conns[idx].user_owner, TlsEvent::Error {
                            handle: conns[idx].handle,
                            msg,
                        });
                    }
                    NetEvent::TcpSent { .. } => {}
                    NetEvent::UdpPacket { .. } => {}
                }
            }

            if remove {
                conns.swap_remove(idx);
            } else {
                idx += 1;
            }
        }

        Timer::after(EmbassyDuration::from_millis(5)).await;
    }
}
