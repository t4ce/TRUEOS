//! Serve services.

use std::{
    convert::Infallible,
    fmt::Debug,
    future::{Future, IntoFuture},
    marker::PhantomData,
    pin::pin,
};

use axum_core::{body::Body, extract::Request, response::Response};
use futures_util::FutureExt;
use hyper::body::Incoming;
use hyper_util::rt::{TokioExecutor, TokioIo};
#[cfg(any(feature = "http1", feature = "http2"))]
use hyper_util::{server::conn::auto::Builder, service::TowerToHyperService};
use tokio::io;
use tokio::sync::watch;
use tower::ServiceExt as _;
use tower_service::Service;

mod listener;

pub use self::listener::{Listener, ListenerExt, TapIo};

// TRUEOS probe route: axum -> log facade -> globalog/logtotcp.
// Keep this safe because axum's vendored manifest still forbids unsafe code.
#[cfg(target_os = "trueos")]
macro_rules! axum_trueos_probe {
    ($($arg:tt)*) => {
        log::info!("[axum-probe] {}", format_args!($($arg)*));
    };
}

#[cfg(not(target_os = "trueos"))]
macro_rules! axum_trueos_probe {
    ($($arg:tt)*) => {};
}

/// Serve the service with the supplied listener.
///
/// This method of running a service is intentionally simple and doesn't support any configuration.
/// Use hyper or hyper-util if you need configuration.
///
/// It supports both HTTP/1 as well as HTTP/2.
///
/// # Examples
///
/// Serving a [`Router`]:
///
/// ```
/// use axum::{Router, routing::get};
///
/// # async {
/// let router = Router::new().route("/", get(|| async { "Hello, World!" }));
///
/// let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
/// axum::serve(listener, router).await.unwrap();
/// # };
/// ```
///
/// See also [`Router::into_make_service_with_connect_info`].
///
/// Serving a [`MethodRouter`]:
///
/// ```
/// use axum::routing::get;
///
/// # async {
/// let router = get(|| async { "Hello, World!" });
///
/// let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
/// axum::serve(listener, router).await.unwrap();
/// # };
/// ```
///
/// See also [`MethodRouter::into_make_service_with_connect_info`].
///
/// Serving a [`Handler`]:
///
/// ```
/// use axum::handler::HandlerWithoutStateExt;
///
/// # async {
/// async fn handler() -> &'static str {
///     "Hello, World!"
/// }
///
/// let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
/// axum::serve(listener, handler.into_make_service()).await.unwrap();
/// # };
/// ```
///
/// See also [`HandlerWithoutStateExt::into_make_service_with_connect_info`] and
/// [`HandlerService::into_make_service_with_connect_info`].
///
/// # Return Value
///
/// Although this future resolves to `io::Result<()>`, it will never actually complete or return an
/// error. Errors on the TCP socket will be handled by sleeping for a short while (currently, one
/// second).
///
/// [`Router`]: crate::Router
/// [`Router::into_make_service_with_connect_info`]: crate::Router::into_make_service_with_connect_info
/// [`MethodRouter`]: crate::routing::MethodRouter
/// [`MethodRouter::into_make_service_with_connect_info`]: crate::routing::MethodRouter::into_make_service_with_connect_info
/// [`Handler`]: crate::handler::Handler
/// [`HandlerWithoutStateExt::into_make_service_with_connect_info`]: crate::handler::HandlerWithoutStateExt::into_make_service_with_connect_info
/// [`HandlerService::into_make_service_with_connect_info`]: crate::handler::HandlerService::into_make_service_with_connect_info
#[cfg(all(feature = "tokio", any(feature = "http1", feature = "http2")))]
pub fn serve<L, M, S>(listener: L, make_service: M) -> Serve<L, M, S>
where
    L: Listener,
    M: for<'a> Service<IncomingStream<'a, L>, Error = Infallible, Response = S>,
    S: Service<Request, Response = Response, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send,
{
    axum_trueos_probe!("serve construct");
    Serve {
        listener,
        make_service,
        _marker: PhantomData,
    }
}

/// Future returned by [`serve`].
#[cfg(all(feature = "tokio", any(feature = "http1", feature = "http2")))]
#[must_use = "futures must be awaited or polled"]
pub struct Serve<L, M, S> {
    listener: L,
    make_service: M,
    _marker: PhantomData<fn() -> S>,
}

#[cfg(all(feature = "tokio", any(feature = "http1", feature = "http2")))]
impl<L, M, S> Serve<L, M, S>
where
    L: Listener,
{
    /// Prepares a server to handle graceful shutdown when the provided future completes.
    ///
    /// # Example
    ///
    /// ```
    /// use axum::{Router, routing::get};
    ///
    /// # async {
    /// let router = Router::new().route("/", get(|| async { "Hello, World!" }));
    ///
    /// let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    /// axum::serve(listener, router)
    ///     .with_graceful_shutdown(shutdown_signal())
    ///     .await
    ///     .unwrap();
    /// # };
    ///
    /// async fn shutdown_signal() {
    ///     // ...
    /// }
    /// ```
    ///
    /// # Return Value
    ///
    /// Similarly to [`serve`], although this future resolves to `io::Result<()>`, it will never
    /// error. It returns `Ok(())` only after the `signal` future completes.
    pub fn with_graceful_shutdown<F>(self, signal: F) -> WithGracefulShutdown<L, M, S, F>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        WithGracefulShutdown {
            listener: self.listener,
            make_service: self.make_service,
            signal,
            _marker: PhantomData,
        }
    }

    /// Returns the local address this server is bound to.
    pub fn local_addr(&self) -> io::Result<L::Addr> {
        self.listener.local_addr()
    }
}

#[cfg(all(feature = "tokio", any(feature = "http1", feature = "http2")))]
impl<L, M, S> Serve<L, M, S>
where
    L: Listener,
    L::Addr: Debug,
    M: for<'a> Service<IncomingStream<'a, L>, Error = Infallible, Response = S> + Send + 'static,
    for<'a> <M as Service<IncomingStream<'a, L>>>::Future: Send,
    S: Service<Request, Response = Response, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send,
{
    async fn run(self) -> ! {
        axum_trueos_probe!("serve run enter");
        let Self {
            mut listener,
            mut make_service,
            _marker,
        } = self;

        let (signal_tx, _signal_rx) = watch::channel(());
        let (_close_tx, close_rx) = watch::channel(());
        axum_trueos_probe!("serve channels ready");

        loop {
            axum_trueos_probe!("serve accept wait");
            let (io, remote_addr) = listener.accept().await;
            axum_trueos_probe!("serve accepted remote={remote_addr:?}");
            handle_connection(&mut make_service, &signal_tx, &close_rx, io, remote_addr).await;
            axum_trueos_probe!("serve accept loop returned from handle_connection");
        }
    }
}

#[cfg(all(feature = "tokio", any(feature = "http1", feature = "http2")))]
impl<L, M, S> Debug for Serve<L, M, S>
where
    L: Debug + 'static,
    M: Debug,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let Self {
            listener,
            make_service,
            _marker: _,
        } = self;

        let mut s = f.debug_struct("Serve");
        s.field("listener", listener)
            .field("make_service", make_service);

        s.finish()
    }
}

#[cfg(all(feature = "tokio", any(feature = "http1", feature = "http2")))]
impl<L, M, S> IntoFuture for Serve<L, M, S>
where
    L: Listener,
    L::Addr: Debug,
    M: for<'a> Service<IncomingStream<'a, L>, Error = Infallible, Response = S> + Send + 'static,
    for<'a> <M as Service<IncomingStream<'a, L>>>::Future: Send,
    S: Service<Request, Response = Response, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send,
{
    type Output = io::Result<()>;
    type IntoFuture = private::ServeFuture;

    fn into_future(self) -> Self::IntoFuture {
        axum_trueos_probe!("serve into_future");
        private::ServeFuture(Box::pin(async move { self.run().await }))
    }
}

/// Serve future with graceful shutdown enabled.
#[cfg(all(feature = "tokio", any(feature = "http1", feature = "http2")))]
#[must_use = "futures must be awaited or polled"]
pub struct WithGracefulShutdown<L, M, S, F> {
    listener: L,
    make_service: M,
    signal: F,
    _marker: PhantomData<fn() -> S>,
}

#[cfg(all(feature = "tokio", any(feature = "http1", feature = "http2")))]
impl<L, M, S, F> WithGracefulShutdown<L, M, S, F>
where
    L: Listener,
{
    /// Returns the local address this server is bound to.
    pub fn local_addr(&self) -> io::Result<L::Addr> {
        self.listener.local_addr()
    }
}

#[cfg(all(feature = "tokio", any(feature = "http1", feature = "http2")))]
impl<L, M, S, F> WithGracefulShutdown<L, M, S, F>
where
    L: Listener,
    L::Addr: Debug,
    M: for<'a> Service<IncomingStream<'a, L>, Error = Infallible, Response = S> + Send + 'static,
    for<'a> <M as Service<IncomingStream<'a, L>>>::Future: Send,
    S: Service<Request, Response = Response, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send,
    F: Future<Output = ()> + Send + 'static,
{
    async fn run(self) {
        let Self {
            mut listener,
            mut make_service,
            signal,
            _marker,
        } = self;

        let (signal_tx, signal_rx) = watch::channel(());
        tokio::spawn(async move {
            signal.await;
            trace!("received graceful shutdown signal. Telling tasks to shutdown");
            drop(signal_rx);
        });

        let (close_tx, close_rx) = watch::channel(());

        loop {
            let (io, remote_addr) = tokio::select! {
                conn = listener.accept() => conn,
                _ = signal_tx.closed() => {
                    trace!("signal received, not accepting new connections");
                    break;
                }
            };

            handle_connection(&mut make_service, &signal_tx, &close_rx, io, remote_addr).await;
        }

        drop(close_rx);
        drop(listener);

        trace!(
            "waiting for {} task(s) to finish",
            close_tx.receiver_count()
        );
        close_tx.closed().await;
    }
}

#[cfg(all(feature = "tokio", any(feature = "http1", feature = "http2")))]
impl<L, M, S, F> Debug for WithGracefulShutdown<L, M, S, F>
where
    L: Debug + 'static,
    M: Debug,
    S: Debug,
    F: Debug,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let Self {
            listener,
            make_service,
            signal,
            _marker: _,
        } = self;

        f.debug_struct("WithGracefulShutdown")
            .field("listener", listener)
            .field("make_service", make_service)
            .field("signal", signal)
            .finish()
    }
}

#[cfg(all(feature = "tokio", any(feature = "http1", feature = "http2")))]
impl<L, M, S, F> IntoFuture for WithGracefulShutdown<L, M, S, F>
where
    L: Listener,
    L::Addr: Debug,
    M: for<'a> Service<IncomingStream<'a, L>, Error = Infallible, Response = S> + Send + 'static,
    for<'a> <M as Service<IncomingStream<'a, L>>>::Future: Send,
    S: Service<Request, Response = Response, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send,
    F: Future<Output = ()> + Send + 'static,
{
    type Output = io::Result<()>;
    type IntoFuture = private::ServeFuture;

    fn into_future(self) -> Self::IntoFuture {
        private::ServeFuture(Box::pin(async move {
            self.run().await;
            Ok(())
        }))
    }
}

async fn handle_connection<L, M, S>(
    make_service: &mut M,
    signal_tx: &watch::Sender<()>,
    close_rx: &watch::Receiver<()>,
    io: <L as Listener>::Io,
    remote_addr: <L as Listener>::Addr,
) where
    L: Listener,
    L::Addr: Debug,
    M: for<'a> Service<IncomingStream<'a, L>, Error = Infallible, Response = S> + Send + 'static,
    for<'a> <M as Service<IncomingStream<'a, L>>>::Future: Send,
    S: Service<Request, Response = Response, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send,
{
    axum_trueos_probe!("connection enter remote={remote_addr:?}");
    let io = TokioIo::new(io);
    axum_trueos_probe!("connection io wrapped remote={remote_addr:?}");

    trace!("connection {remote_addr:?} accepted");

    axum_trueos_probe!("make_service ready begin remote={remote_addr:?}");
    make_service
        .ready()
        .await
        .unwrap_or_else(|err| match err {});
    axum_trueos_probe!("make_service ready ok remote={remote_addr:?}");

    axum_trueos_probe!("make_service call begin remote={remote_addr:?}");
    let tower_service = make_service
        .call(IncomingStream {
            io: &io,
            remote_addr,
        })
        .await
        .unwrap_or_else(|err| match err {})
        .map_request(|req: Request<Incoming>| req.map(Body::new));
    axum_trueos_probe!("make_service call ok; tower service built");

    let hyper_service = TowerToHyperService::new(tower_service);
    let signal_tx = signal_tx.clone();
    let close_rx = close_rx.clone();
    axum_trueos_probe!("hyper service wrapped; spawning connection task");

    tokio::spawn(async move {
        axum_trueos_probe!("connection task enter");
        #[allow(unused_mut)]
        let mut builder = Builder::new(TokioExecutor::new());
        axum_trueos_probe!("hyper builder created");
        // CONNECT protocol needed for HTTP/2 websockets
        #[cfg(feature = "http2")]
        {
            axum_trueos_probe!("http2 connect protocol enabled");
            builder.http2().enable_connect_protocol();
        }

        let mut conn = pin!(builder.serve_connection_with_upgrades(io, hyper_service));
        let mut signal_closed = pin!(signal_tx.closed().fuse());
        axum_trueos_probe!("serve_connection future ready");

        loop {
            tokio::select! {
                result = conn.as_mut() => {
                    match result {
                        Ok(()) => {
                            axum_trueos_probe!("connection completed ok");
                        }
                        Err(_err) => {
                            trace!("failed to serve connection: {_err:#}");
                            axum_trueos_probe!("connection completed err={_err:#}");
                        }
                    }
                    break;
                }
                _ = &mut signal_closed => {
                    trace!("signal received in task, starting graceful shutdown");
                    axum_trueos_probe!("connection graceful shutdown signal");
                    conn.as_mut().graceful_shutdown();
                }
            }
        }

        drop(close_rx);
        axum_trueos_probe!("connection task exit");
    });
    axum_trueos_probe!("connection task spawned");
}

/// An incoming stream.
///
/// Used with [`serve`] and [`IntoMakeServiceWithConnectInfo`].
///
/// [`IntoMakeServiceWithConnectInfo`]: crate::extract::connect_info::IntoMakeServiceWithConnectInfo
#[derive(Debug)]
pub struct IncomingStream<'a, L>
where
    L: Listener,
{
    io: &'a TokioIo<L::Io>,
    remote_addr: L::Addr,
}

impl<L> IncomingStream<'_, L>
where
    L: Listener,
{
    /// Get a reference to the inner IO type.
    pub fn io(&self) -> &L::Io {
        self.io.inner()
    }

    /// Returns the remote address that this stream is bound to.
    pub fn remote_addr(&self) -> &L::Addr {
        &self.remote_addr
    }
}

mod private {
    use std::{
        future::Future,
        pin::Pin,
        task::{Context, Poll},
    };
    use tokio::io;

    pub struct ServeFuture(pub(super) futures_util::future::BoxFuture<'static, io::Result<()>>);

    impl Future for ServeFuture {
        type Output = io::Result<()>;

        #[inline]
        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            self.0.as_mut().poll(cx)
        }
    }

    impl core::fmt::Debug for ServeFuture {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            f.debug_struct("ServeFuture").finish_non_exhaustive()
        }
    }
}
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use crate::prelude::rust_2021::*;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use alloc::borrow::ToOwned;
