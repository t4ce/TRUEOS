use core::convert::Infallible;
use core::task::{Context, Poll};
use tower_service::Service;

/// A [`MakeService`] that produces services by cloning an inner service.
///
/// [`MakeService`]: super::MakeService
///
/// # Example
///
/// ```
/// # use core::task::{Context, Poll};
/// # use core::pin::Pin;
/// # use core::convert::Infallible;
/// use core::future::{Ready, ready};
/// use tower::make::{MakeService, Shared};
/// use tower::buffer::Buffer;
/// use tower::Service;
///
/// // An example connection type
/// struct Connection {}
///
/// // An example request type
/// struct Request {}
///
/// // An example response type
/// struct Response {}
///
/// // Some service that doesn't implement `Clone`
/// struct MyService;
///
/// impl Service<Request> for MyService {
///     type Response = Response;
///     type Error = Infallible;
///     type Future = Ready<Result<Response, Infallible>>;
///
///     fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
///         Poll::Ready(Ok(()))
///     }
///
///     fn call(&mut self, req: Request) -> Self::Future {
///         ready(Ok(Response {}))
///     }
/// }
///
/// // Example function that runs a service by accepting new connections and using
/// // `Make` to create new services that might be bound to the connection.
/// //
/// // This is similar to what you might find in hyper.
/// async fn serve_make_service<Make>(make: Make)
/// where
///     Make: MakeService<Connection, Request>
/// {
///     // ...
/// }
///
/// # async {
/// // Our service
/// let svc = MyService;
///
/// // Make it `Clone` by putting a channel in front
/// let buffered = Buffer::new(svc, 1024);
///
/// // Convert it into a `MakeService`
/// let make = Shared::new(buffered);
///
/// // Run the service and just ignore the `Connection`s as `MyService` doesn't need them
/// serve_make_service(make).await;
/// # };
/// ```
#[derive(Debug, Clone, Copy)]
pub struct Shared<S> {
    service: S,
}

impl<S> Shared<S> {
    /// Create a new [`Shared`] from a service.
    pub const fn new(service: S) -> Self {
        Self { service }
    }
}

impl<S, T> Service<T> for Shared<S>
where
    S: Clone,
{
    type Response = S;
    type Error = Infallible;
    type Future = SharedFuture<S>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _target: T) -> Self::Future {
        SharedFuture::new(core::future::ready(Ok(self.service.clone())))
    }
}

opaque_future! {
    /// Response future from [`Shared`] services.
    pub type SharedFuture<S> = core::future::Ready<Result<S, Infallible>>;
}
