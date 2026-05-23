//! Extractor that parses `multipart/form-data` requests commonly used with file uploads.
//!
//! See [`Multipart`] for more details.

use super::{FromRequest, Request};
use crate::body::Bytes;
use axum_core::{
    __composite_rejection as composite_rejection, __define_rejection as define_rejection,
    extract::OptionalFromRequest,
    response::{IntoResponse, Response},
    RequestExt,
};
use futures_util::stream::Stream;
use http::{
    header::{HeaderMap, CONTENT_TYPE},
    StatusCode,
};
use std::{
    error::Error,
    fmt,
    pin::Pin,
    task::{Context, Poll},
};

/// Extractor that parses `multipart/form-data` requests (commonly used with file uploads).
///
/// ⚠️ Since extracting multipart form data from the request requires consuming the body, the
/// `Multipart` extractor must be *last* if there are multiple extractors in a handler.
/// See ["the order of extractors"][order-of-extractors]
///
/// [order-of-extractors]: crate::extract#the-order-of-extractors
///
/// # Example
///
/// ```rust,no_run
/// use axum::{
///     extract::Multipart,
///     routing::post,
///     Router,
/// };
/// use futures_util::stream::StreamExt;
///
/// async fn upload(mut multipart: Multipart) {
///     while let Some(mut field) = multipart.next_field().await.unwrap() {
///         let name = field.name().unwrap().to_string();
///         let data = field.bytes().await.unwrap();
///
///         println!("Length of `{}` is {} bytes", name, data.len());
///     }
/// }
///
/// let app = Router::new().route("/upload", post(upload));
/// # let _: Router = app;
/// ```
///
/// # Large Files
///
/// For security reasons, by default, `Multipart` limits the request body size to 2MB.
/// See [`DefaultBodyLimit`][default-body-limit] for how to configure this limit.
///
/// [default-body-limit]: crate::extract::DefaultBodyLimit
#[cfg_attr(docsrs, doc(cfg(feature = "multipart")))]
#[derive(Debug)]
pub struct Multipart {
    inner: multer::Multipart<'static>,
}

impl<S> FromRequest<S> for Multipart
where
    S: Send + Sync,
{
    type Rejection = MultipartRejection;

    async fn from_request(req: Request, _state: &S) -> Result<Self, Self::Rejection> {
        let boundary = content_type_str(req.headers())
            .and_then(|content_type| multer::parse_boundary(content_type).ok())
            .ok_or(InvalidBoundary)?;
        let stream = req.with_limited_body().into_body();
        let multipart = multer::Multipart::new(stream.into_data_stream(), boundary);
        Ok(Self { inner: multipart })
    }
}

impl<S> OptionalFromRequest<S> for Multipart
where
    S: Send + Sync,
{
    type Rejection = MultipartRejection;

    async fn from_request(req: Request, _state: &S) -> Result<Option<Self>, Self::Rejection> {
        let Some(content_type) = content_type_str(req.headers()) else {
            return Ok(None);
        };
        match multer::parse_boundary(content_type) {
            Ok(boundary) => {
                let stream = req.with_limited_body().into_body();
                let multipart = multer::Multipart::new(stream.into_data_stream(), boundary);
                Ok(Some(Self { inner: multipart }))
            }
            Err(multer::Error::NoMultipart) => Ok(None),
            Err(_) => Err(MultipartRejection::InvalidBoundary(InvalidBoundary)),
        }
    }
}

impl Multipart {
    /// Yields the next [`Field`] if available.
    pub async fn next_field(&mut self) -> Result<Option<Field<'_>>, MultipartError> {
        let field = self
            .inner
            .next_field()
            .await
            .map_err(MultipartError::from_multer)?;

        if let Some(field) = field {
            Ok(Some(Field {
                inner: field,
                _multipart: self,
            }))
        } else {
            Ok(None)
        }
    }
}

/// A single field in a multipart stream.
#[derive(Debug)]
pub struct Field<'a> {
    inner: multer::Field<'static>,
    // multer requires there to only be one live `multer::Field` at any point. This enforces that
    // statically, which multer does not do, it returns an error instead.
    _multipart: &'a mut Multipart,
}

impl Stream for Field<'_> {
    type Item = Result<Bytes, MultipartError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.inner)
            .poll_next(cx)
            .map_err(MultipartError::from_multer)
    }
}

impl Field<'_> {
    /// The field name found in the
    /// [`Content-Disposition`](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Content-Disposition)
    /// header.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.inner.name()
    }

    /// The file name found in the
    /// [`Content-Disposition`](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Content-Disposition)
    /// header.
    #[must_use]
    pub fn file_name(&self) -> Option<&str> {
        self.inner.file_name()
    }

    /// Get the [content type](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Content-Type) of the field.
    #[must_use]
    pub fn content_type(&self) -> Option<&str> {
        self.inner.content_type().map(|m| m.as_ref())
    }

    /// Get a map of headers as [`HeaderMap`].
    #[must_use]
    pub fn headers(&self) -> &HeaderMap {
        self.inner.headers()
    }

    /// Get the full data of the field as [`Bytes`].
    pub async fn bytes(self) -> Result<Bytes, MultipartError> {
        self.inner
            .bytes()
            .await
            .map_err(MultipartError::from_multer)
    }

    /// Get the full field data as text.
    pub async fn text(self) -> Result<String, MultipartError> {
        self.inner.text().await.map_err(MultipartError::from_multer)
    }

    /// Stream a chunk of the field data.
    ///
    /// When the field data has been exhausted, this will return [`None`].
    ///
    /// Note this does the same thing as `Field`'s [`Stream`] implementation.
    ///
    /// # Example
    ///
    /// ```
    /// use axum::{
    ///    extract::Multipart,
    ///    routing::post,
    ///    response::IntoResponse,
    ///    http::StatusCode,
    ///    Router,
    /// };
    ///
    /// async fn upload(mut multipart: Multipart) -> Result<(), (StatusCode, String)> {
    ///     while let Some(mut field) = multipart
    ///         .next_field()
    ///         .await
    ///         .map_err(|err| (StatusCode::BAD_REQUEST, err.to_string()))?
    ///     {
    ///         while let Some(chunk) = field
    ///             .chunk()
    ///             .await
    ///             .map_err(|err| (StatusCode::BAD_REQUEST, err.to_string()))?
    ///         {
    ///             println!("received {} bytes", chunk.len());
    ///         }
    ///     }
    ///
    ///     Ok(())
    /// }
    ///
    /// let app = Router::new().route("/upload", post(upload));
    /// # let _: Router = app;
    /// ```
    pub async fn chunk(&mut self) -> Result<Option<Bytes>, MultipartError> {
        self.inner
            .chunk()
            .await
            .map_err(MultipartError::from_multer)
    }
}

/// Errors associated with parsing `multipart/form-data` requests.
#[derive(Debug)]
pub struct MultipartError {
    source: multer::Error,
}

impl MultipartError {
    fn from_multer(multer: multer::Error) -> Self {
        Self { source: multer }
    }

    /// Get the response body text used for this rejection.
    #[must_use]
    pub fn body_text(&self) -> String {
        if is_body_limit_error(&self.source) {
            "Request payload is too large".to_owned()
        } else {
            self.source.to_string()
        }
    }

    /// Get the status code used for this rejection.
    #[must_use]
    pub fn status(&self) -> http::StatusCode {
        status_code_from_multer_error(&self.source)
    }
}

fn status_code_from_multer_error(err: &multer::Error) -> StatusCode {
    match err {
        multer::Error::UnknownField { .. }
        | multer::Error::IncompleteFieldData { .. }
        | multer::Error::IncompleteHeaders
        | multer::Error::ReadHeaderFailed(..)
        | multer::Error::DecodeHeaderName { .. }
        | multer::Error::DecodeContentType(..)
        | multer::Error::NoBoundary
        | multer::Error::DecodeHeaderValue { .. }
        | multer::Error::NoMultipart
        | multer::Error::IncompleteStream => StatusCode::BAD_REQUEST,
        multer::Error::FieldSizeExceeded { .. } | multer::Error::StreamSizeExceeded { .. } => {
            StatusCode::PAYLOAD_TOO_LARGE
        }
        multer::Error::StreamReadFailed(err) => {
            if let Some(err) = err.downcast_ref::<multer::Error>() {
                return status_code_from_multer_error(err);
            }

            if err
                .downcast_ref::<crate::Error>()
                .and_then(|err| err.source())
                .and_then(|err| err.downcast_ref::<http_body_util::LengthLimitError>())
                .is_some()
            {
                return StatusCode::PAYLOAD_TOO_LARGE;
            }

            StatusCode::INTERNAL_SERVER_ERROR
        }
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn is_body_limit_error(err: &multer::Error) -> bool {
    match err {
        multer::Error::FieldSizeExceeded { .. } | multer::Error::StreamSizeExceeded { .. } => true,
        multer::Error::StreamReadFailed(err) => {
            if let Some(err) = err.downcast_ref::<multer::Error>() {
                return is_body_limit_error(err);
            }
            err.downcast_ref::<crate::Error>()
                .and_then(|err| err.source())
                .and_then(|err| err.downcast_ref::<http_body_util::LengthLimitError>())
                .is_some()
        }
        _ => false,
    }
}

impl fmt::Display for MultipartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Error parsing `multipart/form-data` request")
    }
}

impl core::error::Error for MultipartError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        Some(&self.source)
    }
}

impl IntoResponse for MultipartError {
    fn into_response(self) -> Response {
        let body = self.body_text();
        axum_core::__log_rejection!(
            rejection_type = Self,
            body_text = body,
            status = self.status(),
        );
        (self.status(), body).into_response()
    }
}

fn content_type_str(headers: &HeaderMap) -> Option<&str> {
    headers.get(CONTENT_TYPE)?.to_str().ok()
}

composite_rejection! {
    /// Rejection used for [`Multipart`].
    ///
    /// Contains one variant for each way the [`Multipart`] extractor can fail.
    pub enum MultipartRejection {
        InvalidBoundary,
    }
}

define_rejection! {
    #[status = BAD_REQUEST]
    #[body = "Invalid `boundary` for `multipart/form-data` request"]
    /// Rejection type used if the `boundary` in a `multipart/form-data` is
    /// missing or invalid.
    pub struct InvalidBoundary;
}
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use crate::prelude::rust_2021::*;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use alloc::borrow::ToOwned;
