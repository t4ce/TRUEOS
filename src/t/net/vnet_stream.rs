//! Tokio stream adapters for TRUEOS VNet TCP.

use crate::r::net::{NetProfile, VNet};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use v::vnet as api;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VNetStreamError {
    NoDevice,
    OpenTimedOut,
    ConnectTimedOut,
    BridgeRead,
    BridgeWrite,
    NetError,
}

impl VNetStreamError {
    pub const fn as_stage(self) -> &'static str {
        match self {
            Self::NoDevice => "vnet_stream.no_device",
            Self::OpenTimedOut => "vnet_stream.open_timeout",
            Self::ConnectTimedOut => "vnet_stream.connect_timeout",
            Self::BridgeRead => "vnet_stream.bridge_read",
            Self::BridgeWrite => "vnet_stream.bridge_write",
            Self::NetError => "vnet_stream.net_error",
        }
    }
}

async fn send_tcp_all_retry(
    net: &VNet,
    handle: api::NetHandle,
    data: &[u8],
) -> Result<(), VNetStreamError> {
    for chunk in data.chunks(api::MAX_MSG) {
        let mut sent = false;
        for _ in 0..64 {
            if net.send_tcp_all(handle, chunk).is_ok() {
                sent = true;
                break;
            }
            tokio::time::sleep(core::time::Duration::from_millis(1)).await;
        }
        if !sent {
            return Err(VNetStreamError::OpenTimedOut);
        }
    }
    Ok(())
}

async fn tcp_duplex_bridge(
    net: VNet,
    handle: api::NetHandle,
    mut io: DuplexStream,
) -> Result<(), VNetStreamError> {
    let mut outbound = [0u8; 4096];
    loop {
        tokio::select! {
            read = io.read(&mut outbound) => {
                let n = read.map_err(|_| VNetStreamError::BridgeRead)?;
                if n == 0 {
                    break;
                }
                send_tcp_all_retry(&net, handle, &outbound[..n]).await?;
            }
            _ = tokio::time::sleep(core::time::Duration::from_millis(1)) => {
                for _ in 0..256 {
                    let Some(ev) = net.pop_event() else { break };
                    match ev {
                        api::Event::TcpData { handle: h, data } if h == handle => {
                            io.write_all(data.as_slice())
                                .await
                                .map_err(|_| VNetStreamError::BridgeWrite)?;
                        }
                        api::Event::Closed { handle: h } if h == handle => {
                            let _ = io.shutdown().await;
                            return Ok(());
                        }
                        api::Event::Error { .. } => {
                            let _ = io.shutdown().await;
                            return Err(VNetStreamError::NetError);
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    let _ = net.submit(api::Command::Close { handle });
    let _ = io.shutdown().await;
    Ok(())
}

pub async fn connect_tcp_v4_stream(
    profile: NetProfile,
    remote: api::EndpointV4,
    timeout_ms: u32,
    duplex_size: usize,
    log_tag: &'static str,
) -> Result<DuplexStream, VNetStreamError> {
    let net = loop {
        if let Some(v) = VNet::open_with_profile(profile) {
            break v;
        }
        tokio::time::sleep(core::time::Duration::from_millis(50)).await;
    };

    let mut open_sent = false;
    for _ in 0..64 {
        if net.submit(api::Command::OpenTcpConnect { remote }).is_ok() {
            open_sent = true;
            break;
        }
        tokio::time::sleep(core::time::Duration::from_millis(1)).await;
    }
    if !open_sent {
        return Err(VNetStreamError::OpenTimedOut);
    }

    let deadline =
        tokio::time::Instant::now() + core::time::Duration::from_millis(timeout_ms as u64);
    let mut opened = None;
    let handle = 'connect_wait: loop {
        for _ in 0..256 {
            let Some(ev) = net.pop_event() else { break };
            match ev {
                api::Event::Opened { handle, kind } if kind == api::SocketKind::Tcp => {
                    opened = Some(handle);
                }
                api::Event::TcpEstablished { handle, .. } => {
                    if opened.is_none() {
                        opened = Some(handle);
                    }
                    if opened == Some(handle) {
                        break 'connect_wait handle;
                    }
                }
                api::Event::Closed { handle } if opened == Some(handle) => {
                    return Err(VNetStreamError::ConnectTimedOut);
                }
                api::Event::Error { .. } => return Err(VNetStreamError::NetError),
                _ => {}
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(VNetStreamError::ConnectTimedOut);
        }
        tokio::time::sleep(core::time::Duration::from_millis(2)).await;
    };

    let (client_io, bridge_io) = tokio::io::duplex(duplex_size);
    tokio::spawn(async move {
        if let Err(err) = tcp_duplex_bridge(net, handle, bridge_io).await {
            crate::log!("{}: vnet stream bridge ended err={:?}\n", log_tag, err);
        }
    });

    Ok(client_io)
}
