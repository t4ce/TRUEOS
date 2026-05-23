//! Middleware that decompresses request and response bodies.
//!
//! # Examples
//!
//! #### Request
//!
//! ```rust
//! use bytes::Bytes;
//! use flate2::{write::GzEncoder, Compression};
//! use http::{header, HeaderValue, Request, Response};
//! use http_body_util::{Full, BodyExt};
//! use std::{error::Error, io::Write};
//! use tower::{Service, ServiceBuilder, service_fn, ServiceExt};
//! use tower_http::{BoxError, decompression::{DecompressionBody, RequestDecompressionLayer}};
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), BoxError> {
//! // A request encoded with gzip coming from some HTTP client.
//! let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
//! encoder.write_all(b"Hello?")?;
//! let request = Request::builder()
//!     .header(header::CONTENT_ENCODING, "gzip")
//!     .body(Full::from(encoder.finish()?))?;
//!
//! // Our HTTP server
//! let mut server = ServiceBuilder::new()
//!     // Automatically decompress request bodies.
//!     .layer(RequestDecompressionLayer::new())
//!     .service(service_fn(handler));
//!
//! // Send the request, with the gzip encoded body, to our server.
//! let _response = server.ready().await?.call(request).await?;
//!
//! // Handler receives request whose body is decoded when read
//! async fn handler(
//!     mut req: Request<DecompressionBody<Full<Bytes>>>,
//! ) -> Result<Response<Full<Bytes>>, BoxError>{
//!     let data = req.into_body().collect().await?.to_bytes();
//!     assert_eq!(&data[..], b"Hello?");
//!     Ok(Response::new(Full::from("Hello, World!")))
//! }
//! # Ok(())
//! # }
//! ```
//!
//! #### Response
//!
//! ```rust
//! use bytes::Bytes;
//! use http::{Request, Response};
//! use http_body_util::{Full, BodyExt};
//! use core::convert::Infallible;
//! use tower::{Service, ServiceExt, ServiceBuilder, service_fn};
//! use tower_http::{compression::Compression, decompression::DecompressionLayer, BoxError};
//! #
//! # #[tokio::main]
//! # async fn main() -> Result<(), tower_http::BoxError> {
//! # async fn handle(req: Request<Full<Bytes>>) -> Result<Response<Full<Bytes>>, Infallible> {
//! #     let body = Full::from("Hello, World!");
//! #     Ok(Response::new(body))
//! # }
//!
//! // Some opaque service that applies compression.
//! let service = Compression::new(service_fn(handle));
//!
//! // Our HTTP client.
//! let mut client = ServiceBuilder::new()
//!     // Automatically decompress response bodies.
//!     .layer(DecompressionLayer::new())
//!     .service(service);
//!
//! // Call the service.
//! //
//! // `DecompressionLayer` takes care of setting `Accept-Encoding`.
//! let request = Request::new(Full::<Bytes>::default());
//!
//! let response = client
//!     .ready()
//!     .await?
//!     .call(request)
//!     .await?;
//!
//! // Read the body
//! let body = response.into_body();
//! let bytes = body.collect().await?.to_bytes().to_vec();
//! let body = String::from_utf8(bytes).map_err(Into::<BoxError>::into)?;
//!
//! assert_eq!(body, "Hello, World!");
//! #
//! # Ok(())
//! # }
//! ```

mod request;

mod body;
mod future;
mod layer;
mod service;

pub use self::{
    body::DecompressionBody, future::ResponseFuture, layer::DecompressionLayer,
    service::Decompression,
};

pub use self::request::future::RequestDecompressionFuture;
pub use self::request::layer::RequestDecompressionLayer;
pub use self::request::service::RequestDecompression;
