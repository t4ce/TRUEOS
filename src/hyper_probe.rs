//! Direct Hyper integration probe.
//!
//! This lets the kernel own a first-class Hyper dependency directly beside
//! Tokio instead of relying on the temporary Octocrab path.

extern crate std;

use core::convert::Infallible;
use core::pin::Pin;
use core::sync::atomic::{AtomicBool, Ordering};
use core::task::{Context, Poll};
use embassy_executor::task;
use std::io;
use std::net::SocketAddr;

use hyper::body::{Body, Bytes, Frame, SizeHint};
use hyper::rt::{Read, ReadBufCursor, Write};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const HYPER_HTTP_PROBE_PORT: u16 = crate::allports::services::HTTP_TRUEOSFS_TCP_PORT;

static HYPER_NET_PROBE_TASK_SPAWNED: AtomicBool = AtomicBool::new(false);

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

fn primary_ipv4_probe_addr(port: u16) -> Option<SocketAddr> {
    let dev_idx = crate::net::primary_device_index();
    let ip = crate::net::adapter::ipv4_at(dev_idx)?;
    Some(SocketAddr::from((ip, port)))
}

async fn probe_hyper_http1_trueosfs() -> Result<(), &'static str> {
    let Some(addr) = primary_ipv4_probe_addr(HYPER_HTTP_PROBE_PORT) else {
        crate::log!("hyper_probe: note net.http1.trueosfs skipped (no primary ipv4 yet)\n");
        return Ok(());
    };

    let connect_deadline = tokio::time::Instant::now() + core::time::Duration::from_millis(2_000);
    let stream = loop {
        match tokio::time::timeout(
            core::time::Duration::from_millis(250),
            tokio::net::TcpStream::connect(addr),
        )
        .await
        {
            Ok(Ok(stream)) => break stream,
            Ok(Err(_)) if tokio::time::Instant::now() < connect_deadline => {
                tokio::time::sleep(core::time::Duration::from_millis(25)).await;
            }
            Err(_) if tokio::time::Instant::now() < connect_deadline => {
                tokio::time::sleep(core::time::Duration::from_millis(25)).await;
            }
            Ok(Err(_)) => return Err("net.http1.trueosfs.connect"),
            Err(_) => return Err("net.http1.trueosfs.connect_timeout"),
        }
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

#[task]
async fn hyper_net_probe_task() {
    crate::r::readiness::wait_for(
        crate::r::readiness::NET_CONFIGURED | crate::r::readiness::TRUEOSFS_ROOT_MOUNTED,
    )
    .await;
    crate::log!("hyper_probe: resume net.http1 probe after NET_CONFIGURED+TRUEOSFS_ROOT_MOUNTED\n");
    run_hyper_net_probe_runtime();
}

fn spawn_deferred_hyper_net_probe() {
    if HYPER_NET_PROBE_TASK_SPAWNED.swap(true, Ordering::AcqRel) {
        return;
    }

    let Some(spawner) = crate::workers::spawner_for_slot(0) else {
        crate::log!("hyper_probe: note net.http1 task not spawned (no slot0 spawner)\n");
        return;
    };

    match hyper_net_probe_task() {
        Ok(token) => spawner.spawn(token),
        Err(err) => crate::log!("hyper_probe: note net.http1 task spawn failed: {:?}\n", err),
    }
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

    if crate::r::readiness::is_set(
        crate::r::readiness::NET_CONFIGURED | crate::r::readiness::TRUEOSFS_ROOT_MOUNTED,
    ) {
        run_hyper_net_probe_runtime();
    } else {
        crate::log!(
            "hyper_probe: note net.http1 deferred until NET_CONFIGURED+TRUEOSFS_ROOT_MOUNTED\n"
        );
        spawn_deferred_hyper_net_probe();
    }
}
