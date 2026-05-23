#![doc = include_str!("../docs/response.md")]

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use crate::prelude::rust_2024::*;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use alloc::borrow::ToOwned;
use http::{header, HeaderValue, StatusCode};

mod redirect;

pub mod sse;

#[doc(no_inline)]
#[cfg(feature = "json")]
pub use crate::Json;

#[cfg(feature = "form")]
#[doc(no_inline)]
pub use crate::form::Form;

#[doc(no_inline)]
pub use crate::Extension;

#[doc(inline)]
pub use axum_core::response::{
    AppendHeaders, ErrorResponse, IntoResponse, IntoResponseParts, Response, ResponseParts, Result,
};

#[doc(inline)]
pub use self::redirect::Redirect;

#[doc(inline)]
pub use sse::Sse;

/// An HTML response.
///
/// Will automatically get `Content-Type: text/html`.
#[derive(Clone, Copy, Debug)]
#[must_use]
pub struct Html<T>(pub T);

impl<T> IntoResponse for Html<T>
where
    T: IntoResponse,
{
    fn into_response(self) -> Response {
        (
            [(
                header::CONTENT_TYPE,
                HeaderValue::from_static(mime::TEXT_HTML_UTF_8.as_ref()),
            )],
            self.0,
        )
            .into_response()
    }
}

impl<T> From<T> for Html<T> {
    fn from(inner: T) -> Self {
        Self(inner)
    }
}

/// An empty response with 204 No Content status.
///
/// Due to historical and implementation reasons, the `IntoResponse` implementation of `()`
/// (unit type) returns an empty response with 200 [`StatusCode::OK`] status.
/// If you specifically want a 204 [`StatusCode::NO_CONTENT`] status, you can use either `StatusCode` type
/// directly, or this shortcut struct for self-documentation.
///
/// ```
/// use axum::{extract::Path, response::NoContent};
///
/// async fn delete_user(Path(user): Path<String>) -> Result<NoContent, String> {
///     // ...access database...
/// # drop(user);
///     Ok(NoContent)
/// }
/// ```
#[derive(Debug, Clone, Copy)]
pub struct NoContent;

impl IntoResponse for NoContent {
    fn into_response(self) -> Response {
        StatusCode::NO_CONTENT.into_response()
    }
}
