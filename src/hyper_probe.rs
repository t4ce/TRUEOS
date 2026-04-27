//! Direct Hyper integration probe.
//!
//! This lets the kernel own a first-class Hyper dependency directly beside
//! Tokio instead of relying on the temporary Octocrab path.

extern crate std;

use core::convert::Infallible;
use core::pin::Pin;
use core::task::{Context, Poll};
use std::io;

use hyper::body::{Body, Bytes, Frame, SizeHint};
use hyper::rt::{Read, ReadBufCursor, Write};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream};
use v::vnet as api;

const HYPER_HTTP_PROBE_PORT: u16 = crate::allports::services::HTTP_TRUEOSFS_TCP_PORT;

struct EmptyBody;

impl Body for EmptyBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        Poll::Ready(None)
    }

    fn is_end_stream(&self) -> bool {
        true
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::with_exact(0)
    }
}

struct HyperTokioIo<T> {
    inner: T,
}

impl<T> HyperTokioIo<T> {
    fn new(inner: T) -> Self {
        Self { inner }
    }
}

impl<T> Read for HyperTokioIo<T>
where
    T: AsyncRead + Unpin,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        mut buf: ReadBufCursor<'_>,
    ) -> Poll<Result<(), io::Error>> {
        let limit = buf.remaining().min(1024);
        if limit == 0 {
            return Poll::Ready(Ok(()));
        }

        let mut scratch = [0u8; 1024];
        let mut tokio_buf = tokio::io::ReadBuf::new(&mut scratch[..limit]);
        match Pin::new(&mut self.inner).poll_read(cx, &mut tokio_buf) {
            Poll::Ready(Ok(())) => {
                buf.put_slice(tokio_buf.filled());
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<T> Write for HyperTokioIo<T>
where
    T: AsyncWrite + Unpin,
{
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

async fn fake_http1_server(mut io: tokio::io::DuplexStream) -> Result<(), &'static str> {
    let mut request = [0u8; 512];
    let n = io.read(&mut request).await.map_err(|_| "server.read")?;
    let request = core::str::from_utf8(&request[..n]).map_err(|_| "server.request_utf8")?;
    if !request.starts_with("GET /hyper-probe HTTP/1.1\r\n") {
        return Err("server.request_line");
    }
    if !request.contains("\r\nhost: trueos.local\r\n")
        && !request.contains("\r\nHost: trueos.local\r\n")
    {
        return Err("server.host_header");
    }

    io.write_all(b"HTTP/1.1 204 No Content\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
        .await
        .map_err(|_| "server.write")?;
    io.shutdown().await.map_err(|_| "server.shutdown")?;
    Ok(())
}

async fn probe_hyper_http1_loopback() -> Result<(), &'static str> {
    let (client_io, server_io) = tokio::io::duplex(2048);
    let server = tokio::spawn(fake_http1_server(server_io));

    let (mut sender, connection) =
        hyper::client::conn::http1::handshake::<_, EmptyBody>(HyperTokioIo::new(client_io))
            .await
            .map_err(|_| "client.handshake")?;
    let connection = tokio::spawn(async move { connection.await });

    sender.ready().await.map_err(|_| "client.ready")?;
    let request = hyper::Request::builder()
        .method(hyper::Method::GET)
        .uri("/hyper-probe")
        .header(hyper::header::HOST, "trueos.local")
        .body(EmptyBody)
        .map_err(|_| "client.request_build")?;
    let response = sender
        .send_request(request)
        .await
        .map_err(|_| "client.send_request")?;

    if response.status() != hyper::StatusCode::NO_CONTENT {
        return Err("client.response_status");
    }

    match server.await {
        Ok(Ok(())) => {}
        Ok(Err(stage)) => return Err(stage),
        Err(_) => return Err("server.join"),
    }

    drop(sender);
    match tokio::time::timeout(core::time::Duration::from_millis(50), connection).await {
        Ok(Ok(Ok(()))) => Ok(()),
        Ok(Ok(Err(_))) => Err("client.connection"),
        Ok(Err(_)) => Err("client.connection_join"),
        Err(_) => Err("client.connection_timeout"),
    }
}

fn primary_ipv4_probe_endpoint(port: u16) -> Option<api::EndpointV4> {
    let dev_idx = crate::net::primary_device_index();
    let ip = crate::net::adapter::ipv4_at(dev_idx)?;
    Some(api::EndpointV4 { addr: ip, port })
}

async fn vnet_send_tcp_all(
    vnet: &crate::r::net::VNet,
    handle: api::NetHandle,
    data: &[u8],
) -> Result<(), &'static str> {
    for chunk in data.chunks(api::MAX_MSG) {
        let mut sent = false;
        for _ in 0..64 {
            if vnet
                .submit(api::Command::SendTcp {
                    handle,
                    data: api::ByteBuf::from_slice_trunc(chunk),
                })
                .is_ok()
            {
                sent = true;
                break;
            }
            tokio::time::sleep(core::time::Duration::from_millis(1)).await;
        }
        if !sent {
            return Err("vnet.send_tcp");
        }
    }
    Ok(())
}

async fn vnet_tcp_bridge(
    vnet: crate::r::net::VNet,
    handle: api::NetHandle,
    mut io: DuplexStream,
) -> Result<(), &'static str> {
    let mut outbound = [0u8; 2048];
    loop {
        tokio::select! {
            read = io.read(&mut outbound) => {
                let n = read.map_err(|_| "vnet.bridge_read")?;
                if n == 0 {
                    break;
                }
                vnet_send_tcp_all(&vnet, handle, &outbound[..n]).await?;
            }
            _ = tokio::time::sleep(core::time::Duration::from_millis(1)) => {
                for _ in 0..64 {
                    let Some(ev) = vnet.pop_event() else {
                        break;
                    };
                    match ev {
                        api::Event::TcpData { handle: h, data } if h == handle => {
                            io.write_all(data.as_slice())
                                .await
                                .map_err(|_| "vnet.bridge_write")?;
                        }
                        api::Event::Closed { handle: h } if h == handle => {
                            let _ = io.shutdown().await;
                            return Ok(());
                        }
                        api::Event::Error { .. } => return Err("vnet.bridge_error"),
                        _ => {}
                    }
                }
            }
        }
    }

    let _ = vnet.submit(api::Command::Close { handle });
    let _ = io.shutdown().await;
    Ok(())
}

async fn connect_trueosfs_vnet() -> Result<DuplexStream, &'static str> {
    let Some(remote) = primary_ipv4_probe_endpoint(HYPER_HTTP_PROBE_PORT) else {
        crate::log!("hyper_probe: note net.http1.trueosfs skipped (no primary ipv4 yet)\n");
        return Err("net.http1.trueosfs.no_ipv4");
    };

    let Some(vnet) = crate::r::net::VNet::open_primary() else {
        return Err("net.http1.trueosfs.vnet_open");
    };

    vnet.submit(api::Command::OpenTcpConnect { remote })
        .map_err(|_| "net.http1.trueosfs.vnet_connect_submit")?;

    let connect_deadline = tokio::time::Instant::now() + core::time::Duration::from_millis(2_000);
    let mut tcp_handle = None;
    let handle = 'connect: loop {
        while let Some(ev) = vnet.pop_event() {
            match ev {
                api::Event::Opened { handle, kind } if kind == api::SocketKind::Tcp => {
                    tcp_handle = Some(handle);
                }
                api::Event::TcpEstablished { handle } => {
                    if tcp_handle.is_none() {
                        tcp_handle = Some(handle);
                    }
                    if tcp_handle == Some(handle) {
                        break 'connect handle;
                    }
                }
                api::Event::Error { .. } => return Err("net.http1.trueosfs.vnet_connect"),
                _ => {}
            }
        }

        if tokio::time::Instant::now() >= connect_deadline {
            return Err("net.http1.trueosfs.vnet_connect_timeout");
        }

        tokio::time::sleep(core::time::Duration::from_millis(1)).await;
    };

    let (client_io, bridge_io) = tokio::io::duplex(16 * 1024);
    tokio::spawn(async move {
        if let Err(stage) = vnet_tcp_bridge(vnet, handle, bridge_io).await {
            crate::log!("hyper_probe: note net.http1.trueosfs bridge ended at {}\n", stage);
        }
    });

    Ok(client_io)
}

async fn probe_hyper_http1_trueosfs() -> Result<(), &'static str> {
    let stream = match connect_trueosfs_vnet().await {
        Ok(stream) => stream,
        Err("net.http1.trueosfs.no_ipv4") => return Ok(()),
        Err(stage) => return Err(stage),
    };

    let (mut sender, connection) =
        hyper::client::conn::http1::handshake::<_, EmptyBody>(HyperTokioIo::new(stream))
            .await
            .map_err(|_| "net.http1.trueosfs.handshake")?;
    let connection = tokio::spawn(async move { connection.await });

    sender
        .ready()
        .await
        .map_err(|_| "net.http1.trueosfs.ready")?;
    let request = hyper::Request::builder()
        .method(hyper::Method::GET)
        .uri("/")
        .header(hyper::header::HOST, "trueos.local")
        .body(EmptyBody)
        .map_err(|_| "net.http1.trueosfs.request_build")?;
    let response = sender
        .send_request(request)
        .await
        .map_err(|_| "net.http1.trueosfs.send_request")?;

    if response.status() != hyper::StatusCode::OK {
        return Err("net.http1.trueosfs.response_status");
    }

    crate::log!("hyper_probe: success net.http1.trueosfs GET / -> 200\n");

    drop(response);
    drop(sender);
    match tokio::time::timeout(core::time::Duration::from_millis(250), connection).await {
        Ok(Ok(Ok(()))) => Ok(()),
        Ok(Ok(Err(_))) => Err("net.http1.trueosfs.connection"),
        Ok(Err(_)) => Err("net.http1.trueosfs.connection_join"),
        Err(_) => {
            crate::log!("hyper_probe: note net.http1.trueosfs connection still draining\n");
            Ok(())
        }
    }
}

fn run_hyper_net_probe_runtime() {
    let mut runtime_builder = tokio::runtime::Builder::new_current_thread();
    runtime_builder.enable_io();
    runtime_builder.enable_time();

    let runtime = match runtime_builder.build() {
        Ok(runtime) => runtime,
        Err(_) => {
            crate::log!("hyper_probe: failure net.http1.rt_build\n");
            return;
        }
    };

    match runtime.block_on(probe_hyper_http1_trueosfs()) {
        Ok(()) => crate::log!("hyper_probe: success net.http1 probe_suite\n"),
        Err(stage) => crate::log!("hyper_probe: failure {}\n", stage),
    }
}

#[embassy_executor::task]
pub(crate) async fn hyper_net_probe_task() {
    crate::r::readiness::wait_for(
        crate::r::readiness::NET_CONFIGURED
            | crate::r::readiness::TRUEOSFS_ROOT_MOUNTED
            | crate::r::readiness::HTTP_TRUEOSFS_LISTENING,
    )
    .await;
    crate::log!(
        "hyper_probe: resume net.http1 probe after NET_CONFIGURED+TRUEOSFS_ROOT_MOUNTED+HTTP_TRUEOSFS_LISTENING\n"
    );
    run_hyper_net_probe_runtime();
}

pub(crate) fn log_boot_probe() {
    crate::log!("hyper_probe: wired hyper 1.9 client/http1 surface directly beside tokio\n");

    let _ = hyper::client::conn::http1::Builder::new;
    let _ = hyper::Method::GET;
    let _ = hyper::Version::HTTP_11;

    let mut runtime_builder = tokio::runtime::Builder::new_current_thread();
    runtime_builder.enable_time();

    let runtime = match runtime_builder.build() {
        Ok(runtime) => runtime,
        Err(_) => {
            crate::log!("hyper_probe: failure http1.loopback.rt_build\n");
            return;
        }
    };

    match runtime.block_on(probe_hyper_http1_loopback()) {
        Ok(()) => crate::log!("hyper_probe: success http1.loopback_request_response\n"),
        Err(stage) => crate::log!("hyper_probe: failure {}\n", stage),
    }

    crate::log!("hyper_probe: net.http1 probe is managed by spawn-svc readiness gating\n");
}
