//! Direct Hyper integration probe.
//!
//! This lets the kernel own a first-class Hyper dependency directly beside

use core::convert::Infallible;
use core::pin::Pin;
use core::task::{Context, Poll};
use trueos_io as io;

use hyper::body::{Body, Bytes, Frame, SizeHint};
use hyper::rt::{Read, ReadBufCursor, Write};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream};
use v::vnet as api;

const HYPER_HTTP_PROBE_ADDR: [u8; 4] = [217, 160, 0, 248];
const HYPER_HTTP_PROBE_PORT: u16 = 80;
const HYPER_HTTP_PROBE_HOST: &str = "example.de";

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

#[cfg(feature = "vmx-web")]
async fn probe_hyper_http1_server_loopback() -> Result<(), &'static str> {
    let (client_io, server_io) = tokio::io::duplex(2048);

    let server = tokio::spawn(async move {
        let service = hyper::service::service_fn(
            |request: hyper::Request<hyper::body::Incoming>| async move {
                let ok = request.method() == hyper::Method::GET
                    && request.uri().path() == "/hyper-server-probe"
                    && request
                        .headers()
                        .get(hyper::header::HOST)
                        .is_some_and(|host| host == "trueos.local");
                let status = if ok {
                    hyper::StatusCode::OK
                } else {
                    hyper::StatusCode::BAD_REQUEST
                };
                hyper::Response::builder()
                    .status(status)
                    .header(hyper::header::CONTENT_LENGTH, "0")
                    .body(EmptyBody)
            },
        );

        hyper::server::conn::http1::Builder::new()
            .serve_connection(HyperTokioIo::new(server_io), service)
            .await
            .map_err(|_| "server_hyper.connection")
    });

    let (mut sender, connection) =
        hyper::client::conn::http1::handshake::<_, EmptyBody>(HyperTokioIo::new(client_io))
            .await
            .map_err(|_| "server_loopback.client.handshake")?;
    let connection = tokio::spawn(async move { connection.await });

    sender
        .ready()
        .await
        .map_err(|_| "server_loopback.client.ready")?;
    let request = hyper::Request::builder()
        .method(hyper::Method::GET)
        .uri("/hyper-server-probe")
        .header(hyper::header::HOST, "trueos.local")
        .body(EmptyBody)
        .map_err(|_| "server_loopback.client.request_build")?;
    let response = sender
        .send_request(request)
        .await
        .map_err(|_| "server_loopback.client.send_request")?;

    if response.status() != hyper::StatusCode::OK {
        return Err("server_loopback.client.response_status");
    }

    drop(response);
    drop(sender);

    match tokio::time::timeout(core::time::Duration::from_millis(50), connection).await {
        Ok(Ok(Ok(()))) => {}
        Ok(Ok(Err(_))) => return Err("server_loopback.client.connection"),
        Ok(Err(_)) => return Err("server_loopback.client.connection_join"),
        Err(_) => return Err("server_loopback.client.connection_timeout"),
    }

    match tokio::time::timeout(core::time::Duration::from_millis(50), server).await {
        Ok(Ok(Ok(()))) => Ok(()),
        Ok(Ok(Err(stage))) => Err(stage),
        Ok(Err(_)) => Err("server_loopback.server.join"),
        Err(_) => Err("server_loopback.server.timeout"),
    }
}

fn hyper_http_probe_endpoint() -> api::EndpointV4 {
    api::EndpointV4 {
        addr: HYPER_HTTP_PROBE_ADDR,
        port: HYPER_HTTP_PROBE_PORT,
    }
}

async fn connect_example_de_vnet() -> Result<DuplexStream, &'static str> {
    let remote = hyper_http_probe_endpoint();

    crate::log!(
        "hyper_probe: net.http1.example_de connect host={} remote={}.{}.{}.{}:{}\n",
        HYPER_HTTP_PROBE_HOST,
        remote.addr[0],
        remote.addr[1],
        remote.addr[2],
        remote.addr[3],
        remote.port
    );

    crate::t::net::vnet_stream::connect_tcp_v4_stream(
        crate::r::net::NetProfile::default(),
        remote,
        10_000,
        16 * 1024,
        "hyper_probe",
    )
    .await
    .map_err(|err| {
        crate::log!(
            "hyper_probe: net.http1.example_de vnet connect failed stage={}\n",
            err.as_stage()
        );
        err.as_stage()
    })
}

async fn probe_hyper_http1_example_de() -> Result<(), &'static str> {
    let stream = match connect_example_de_vnet().await {
        Ok(stream) => stream,
        Err(stage) => return Err(stage),
    };

    let (mut sender, connection) =
        hyper::client::conn::http1::handshake::<_, EmptyBody>(HyperTokioIo::new(stream))
            .await
            .map_err(|_| "net.http1.example_de.handshake")?;
    let connection = tokio::spawn(async move { connection.await });

    sender
        .ready()
        .await
        .map_err(|_| "net.http1.example_de.ready")?;
    let request = hyper::Request::builder()
        .method(hyper::Method::GET)
        .uri("/")
        .header(hyper::header::HOST, HYPER_HTTP_PROBE_HOST)
        .body(EmptyBody)
        .map_err(|_| "net.http1.example_de.request_build")?;
    let response = sender
        .send_request(request)
        .await
        .map_err(|_| "net.http1.example_de.send_request")?;

    if response.status() != hyper::StatusCode::OK {
        return Err("net.http1.example_de.response_status");
    }

    crate::log!("hyper_probe: success net.http1.example_de GET / -> 200\n");

    drop(response);
    drop(sender);
    match tokio::time::timeout(core::time::Duration::from_millis(250), connection).await {
        Ok(Ok(Ok(()))) => Ok(()),
        Ok(Ok(Err(_))) => Err("net.http1.example_de.connection"),
        Ok(Err(_)) => Err("net.http1.example_de.connection_join"),
        Err(_) => {
            crate::log!("hyper_probe: note net.http1.example_de connection still draining\n");
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

    match runtime.block_on(probe_hyper_http1_example_de()) {
        Ok(()) => crate::log!("hyper_probe: success net.http1 probe_suite\n"),
        Err(stage) => crate::log!("hyper_probe: failure {}\n", stage),
    }
}

#[embassy_executor::task]
pub(crate) async fn hyper_net_probe_task() {
    crate::r::readiness::wait_for(
        crate::r::readiness::NET_SOCKET_READY | crate::r::readiness::NET_V4_GATEWAY_REACHABLE,
    )
    .await;
    crate::log!(
        "hyper_probe: resume net.http1 example.de probe after NET_SOCKET_READY+NET_V4_GATEWAY_REACHABLE\n"
    );
    run_hyper_net_probe_runtime();
}

pub(crate) fn log_boot_probe() {
    crate::log!("hyper_probe: wired hyper 1.9 client/http1 surface directly beside tokio\n");

    let _ = hyper::client::conn::http1::Builder::new;
    #[cfg(feature = "vmx-web")]
    let _ = hyper::server::conn::http1::Builder::new;
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

    #[cfg(feature = "vmx-web")]
    match runtime.block_on(probe_hyper_http1_server_loopback()) {
        Ok(()) => crate::log!("hyper_probe: success http1.server_loopback_request_response\n"),
        Err(stage) => crate::log!("hyper_probe: failure {}\n", stage),
    }

    crate::log!("hyper_probe: net.http1 probe is managed by spawn-svc readiness gating\n");
}
