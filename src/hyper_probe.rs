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
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

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

struct HyperTokioIo {
    inner: tokio::io::DuplexStream,
}

impl HyperTokioIo {
    fn new(inner: tokio::io::DuplexStream) -> Self {
        Self { inner }
    }
}

impl Read for HyperTokioIo {
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

impl Write for HyperTokioIo {
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
}
