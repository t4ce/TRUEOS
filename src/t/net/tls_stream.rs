//! Tokio stream adapters for TRUEOS TLS-over-VNet sessions.

extern crate alloc;
extern crate std;

use crate::net::tls::{TlsClientConfig, TlsRoots};
use crate::net::tls_socket::{TlsCommand, TlsEvent, TlsTimeouts, register_tls_app_queues};
use crate::r::net::Queue;
use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
};
use core::sync::atomic::{AtomicU32, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use v::vnet;

static TLS_STREAM_SEQ: AtomicU32 = AtomicU32::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TlsStreamError {
    OpenTimedOut,
    ConnectTimedOut,
    TlsTimedOut,
    BridgeRead,
    BridgeWrite,
    QueueFull,
    Tls,
}

impl TlsStreamError {
    pub const fn as_stage(self) -> &'static str {
        match self {
            Self::OpenTimedOut => "tls_stream.open_timeout",
            Self::ConnectTimedOut => "tls_stream.connect_timeout",
            Self::TlsTimedOut => "tls_stream.tls_timeout",
            Self::BridgeRead => "tls_stream.bridge_read",
            Self::BridgeWrite => "tls_stream.bridge_write",
            Self::QueueFull => "tls_stream.queue_full",
            Self::Tls => "tls_stream.tls_error",
        }
    }
}

fn leak_str(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn owner_selector_for_device(dev_idx: usize) -> String {
    if let Some((bus, slot, func)) = crate::net::bdf_at(dev_idx) {
        format!("{:02x}:{:02x}.{}", bus, slot, func)
    } else if let Some((vid, pid)) = crate::net::pci_id_at(dev_idx) {
        format!("{:04x}:{:04x}", vid, pid)
    } else {
        dev_idx.to_string()
    }
}

async fn send_plaintext(
    cmds: &'static Queue<TlsCommand>,
    handle: vnet::NetHandle,
    bytes: &[u8],
) -> Result<(), TlsStreamError> {
    for chunk in bytes.chunks(16 * 1024) {
        let mut sent = false;
        for _ in 0..64 {
            if cmds
                .push(TlsCommand::Send {
                    handle,
                    data: chunk.to_vec(),
                })
                .is_ok()
            {
                sent = true;
                break;
            }
            tokio::time::sleep(core::time::Duration::from_millis(1)).await;
        }
        if !sent {
            return Err(TlsStreamError::QueueFull);
        }
    }
    Ok(())
}

async fn tls_duplex_bridge(
    cmds: &'static Queue<TlsCommand>,
    events: &'static Queue<TlsEvent>,
    handle: vnet::NetHandle,
    mut io: DuplexStream,
) -> Result<(), TlsStreamError> {
    let mut outbound = [0u8; 4096];
    loop {
        tokio::select! {
            read = io.read(&mut outbound) => {
                let n = read.map_err(|_| TlsStreamError::BridgeRead)?;
                if n == 0 {
                    break;
                }
                send_plaintext(cmds, handle, &outbound[..n]).await?;
            }
            _ = tokio::time::sleep(core::time::Duration::from_millis(1)) => {
                for ev in events.drain(128) {
                    match ev {
                        TlsEvent::Data { handle: h, data } if h == handle => {
                            io.write_all(data.as_slice())
                                .await
                                .map_err(|_| TlsStreamError::BridgeWrite)?;
                        }
                        TlsEvent::Closed { handle: h } if h == handle => {
                            let _ = io.shutdown().await;
                            return Ok(());
                        }
                        TlsEvent::Error { .. } | TlsEvent::TlsError { .. } => {
                            let _ = io.shutdown().await;
                            return Err(TlsStreamError::Tls);
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    let _ = cmds.push(TlsCommand::Close { handle });
    let _ = io.shutdown().await;
    Ok(())
}

pub async fn connect_tls_v4_stream(
    dev_idx: usize,
    remote: vnet::EndpointV4,
    server_name: String,
    cfg: TlsClientConfig,
    roots: TlsRoots,
    timeouts: TlsTimeouts,
    timeout_ms: u32,
    duplex_size: usize,
    log_tag: &'static str,
) -> Result<DuplexStream, TlsStreamError> {
    let seq = TLS_STREAM_SEQ.fetch_add(1, Ordering::Relaxed);
    let selector = owner_selector_for_device(dev_idx);
    let owner = leak_str(format!("{}-{}@{}", log_tag, seq, selector));
    let cmds = Queue::new_leaked(leak_str(format!("{}-tls-cmd", owner)), 256);
    let events = Queue::new_leaked(leak_str(format!("{}-tls-evt", owner)), 4096);
    register_tls_app_queues(owner, cmds, events);

    let server_name = leak_str(server_name);
    cmds.push(TlsCommand::OpenTcpConnect {
        remote,
        server_name,
        cfg,
        roots,
        timeouts,
    })
    .map_err(|_| TlsStreamError::OpenTimedOut)?;

    let deadline =
        tokio::time::Instant::now() + core::time::Duration::from_millis(timeout_ms as u64);
    let mut opened = None;
    let handle = 'connect_wait: loop {
        for ev in events.drain(256) {
            match ev {
                TlsEvent::Opened { handle } => {
                    opened = Some(handle);
                    crate::log!("{}: opened dev={} handle={}\n", log_tag, dev_idx, handle.0);
                }
                TlsEvent::Connected { handle } => {
                    if opened.is_none() {
                        opened = Some(handle);
                    }
                    if opened == Some(handle) {
                        crate::log!(
                            "{}: tls-connected dev={} handle={}\n",
                            log_tag,
                            dev_idx,
                            handle.0
                        );
                        break 'connect_wait handle;
                    }
                }
                TlsEvent::Closed { handle } if opened == Some(handle) => {
                    return Err(TlsStreamError::Tls);
                }
                TlsEvent::Error { .. } | TlsEvent::TlsError { .. } => {
                    return Err(TlsStreamError::Tls);
                }
                _ => {}
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(if opened.is_some() {
                TlsStreamError::TlsTimedOut
            } else {
                TlsStreamError::ConnectTimedOut
            });
        }
        tokio::time::sleep(core::time::Duration::from_millis(2)).await;
    };

    let (client_io, bridge_io) = tokio::io::duplex(duplex_size);
    tokio::spawn(async move {
        if let Err(err) = tls_duplex_bridge(cmds, events, handle, bridge_io).await {
            crate::log!("{}: tls stream bridge ended err={:?}\n", log_tag, err);
        }
    });

    Ok(client_io)
}
