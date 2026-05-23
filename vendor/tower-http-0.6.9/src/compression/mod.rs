//! Middleware that compresses response bodies.
//!
//! # Example
//!
//! Example showing how to respond with the compressed contents of a file.
//!
//! ```rust
//! use bytes::{Bytes, BytesMut};
//! use http::{Request, Response, header::ACCEPT_ENCODING};
//! use http_body_util::{Full, BodyExt, StreamBody, combinators::UnsyncBoxBody};
//! use http_body::Frame;
//! use core::convert::Infallible;
//! use tokio::fs::{self, File};
//! use tokio_util::io::ReaderStream;
//! use tower::{Service, ServiceExt, ServiceBuilder, service_fn};
//! use tower_http::{compression::CompressionLayer, BoxError};
//! use futures_util::TryStreamExt;
//!
//! type BoxBody = UnsyncBoxBody<Bytes, std::io::Error>;
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), BoxError> {
//! async fn handle(req: Request<Full<Bytes>>) -> Result<Response<BoxBody>, Infallible> {
//!     // Open the file.
//!     let file = File::open("Cargo.toml").await.expect("file missing");
//!     // Convert the file into a `Stream` of `Bytes`.
//!     let stream = ReaderStream::new(file);
//!     // Convert the stream into a stream of data `Frame`s.
//!     let stream = stream.map_ok(Frame::data);
//!     // Convert the `Stream` into a `Body`.
//!     let body = StreamBody::new(stream);
//!     // Erase the type because it's very hard to name in the function signature.
//!     let body = body.boxed_unsync();
//!     // Create response.
//!     Ok(Response::new(body))
//! }
//!
//! let mut service = ServiceBuilder::new()
//!     // Compress responses based on the `Accept-Encoding` header.
//!     .layer(CompressionLayer::new())
//!     .service_fn(handle);
//!
//! // Call the service.
//! let request = Request::builder()
//!     .header(ACCEPT_ENCODING, "gzip")
//!     .body(Full::<Bytes>::default())?;
//!
//! let response = service
//!     .ready()
//!     .await?
//!     .call(request)
//!     .await?;
//!
//! assert_eq!(response.headers()["content-encoding"], "gzip");
//!
//! // Read the body
//! let bytes = response
//!     .into_body()
//!     .collect()
//!     .await?
//!     .to_bytes();
//!
//! // The compressed body should be smaller 🤞
//! let uncompressed_len = fs::read_to_string("Cargo.toml").await?.len();
//! assert!(bytes.len() < uncompressed_len);
//! #
//! # Ok(())
//! # }
//! ```
//!

pub mod predicate;

mod body;
mod future;
mod layer;
mod pin_project_cfg;
mod service;

#[doc(inline)]
pub use self::{
    body::CompressionBody,
    future::ResponseFuture,
    layer::CompressionLayer,
    predicate::{DefaultPredicate, Predicate},
    service::Compression,
};
pub use crate::compression_utils::CompressionLevel;
