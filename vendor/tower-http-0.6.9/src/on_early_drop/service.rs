//! Service implementation for the on-early-drop middleware.

use crate::on_early_drop::body::OnEarlyDropBody;
use crate::on_early_drop::future::OnEarlyDropFuture;
use crate::on_early_drop::traits::{OnBodyDrop, OnFutureDrop};
use http::{Request, Response};
use core::task::{Context, Poll};
use tower_service::Service;

/// [`Service`] produced by [`OnEarlyDropLayer`].
///
/// See the [module docs](super) for details and examples.
///
/// [`OnEarlyDropLayer`]: super::OnEarlyDropLayer
pub struct OnEarlyDropService<S, OFD, OBD> {
    pub(crate) inner: S,
    pub(crate) on_future_drop: OFD,
    pub(crate) on_body_drop: OBD,
}

impl<S, OFD, OBD> ::core::fmt::Debug for OnEarlyDropService<S, OFD, OBD>
where
    S: ::core::fmt::Debug,
{
    fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
        f.debug_struct("OnEarlyDropService")
            .field("inner", &self.inner)
            .field("on_future_drop", &format_args!(".."))
            .field("on_body_drop", &format_args!(".."))
            .finish()
    }
}

impl<S, OFD, OBD> Clone for OnEarlyDropService<S, OFD, OBD>
where
    S: Clone,
    OFD: Clone,
    OBD: Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            on_future_drop: self.on_future_drop.clone(),
            on_body_drop: self.on_body_drop.clone(),
        }
    }
}

impl<S, OFD, OBD> OnEarlyDropService<S, OFD, OBD> {
    /// Construct a new service directly. Most uses go through
    /// [`OnEarlyDropLayer`](super::OnEarlyDropLayer).
    pub fn new(inner: S, on_future_drop: OFD, on_body_drop: OBD) -> Self {
        Self {
            inner,
            on_future_drop,
            on_body_drop,
        }
    }

    define_inner_service_accessors!();
}

impl<S, OFD, OBD, ReqB, ResB> Service<Request<ReqB>> for OnEarlyDropService<S, OFD, OBD>
where
    S: Service<Request<ReqB>, Response = Response<ResB>>,
    OFD: OnFutureDrop<ReqB>,
    OBD: OnBodyDrop<ReqB> + Clone,
    ResB: http_body::Body,
{
    type Response = Response<OnEarlyDropBody<ResB, OBD::Callback>>;
    type Error = S::Error;
    type Future = OnEarlyDropFuture<S::Future, OBD, ReqB, OFD::Callback, OBD::Callback>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqB>) -> Self::Future {
        let future_callback = self.on_future_drop.make(&req);
        let intermediate = self.on_body_drop.make_at_call(&req);
        let inner = self.inner.call(req);
        OnEarlyDropFuture::new(
            inner,
            future_callback,
            self.on_body_drop.clone(),
            intermediate,
        )
    }
}
