#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use crate::prelude::rust_2021::*;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use alloc::borrow::ToOwned;
use crate::extract::Request;
use crate::extract::{rejection::*, FromRequest};
use axum_core::extract::OptionalFromRequest;
use axum_core::response::{IntoResponse, Response};
use bytes::{BufMut, Bytes, BytesMut};
use http::{
    header::{self, HeaderMap, HeaderValue},
    StatusCode,
};
use serde_core::{de::DeserializeOwned, Serialize};

/// JSON Extractor / Response.
///
/// When used as an extractor, it can deserialize request bodies into some type that
/// implements [`serde::de::DeserializeOwned`]. The request will be rejected (and a [`JsonRejection`] will
/// be returned) if:
///
/// - The request doesn't have a `Content-Type: application/json` (or similar) header.
/// - The body doesn't contain syntactically valid JSON.
/// - The body contains syntactically valid JSON, but it couldn't be deserialized into the target type.
/// - Buffering the request body fails.
///
/// ⚠️ Since parsing JSON requires consuming the request body, the `Json` extractor must be
/// *last* if there are multiple extractors in a handler.
/// See ["the order of extractors"][order-of-extractors]
///
/// [order-of-extractors]: crate::extract#the-order-of-extractors
///
/// See [`JsonRejection`] for more details.
///
/// # Extractor example
///
/// ```rust,no_run
/// use axum::{
///     extract,
///     routing::post,
///     Router,
/// };
/// use serde::Deserialize;
///
/// #[derive(Deserialize)]
/// struct CreateUser {
///     email: String,
///     password: String,
/// }
///
/// async fn create_user(extract::Json(payload): extract::Json<CreateUser>) {
///     // payload is a `CreateUser`
/// }
///
/// let app = Router::new().route("/users", post(create_user));
/// # let _: Router = app;
/// ```
///
/// When used as a response, it can serialize any type that implements [`serde::Serialize`] to
/// `JSON`, and will automatically set `Content-Type: application/json` header.
///
/// If the [`Serialize`] implementation decides to fail
/// or if a map with non-string keys is used,
/// a 500 response will be issued
/// whose body is the error message in UTF-8.
///
/// # Response example
///
/// ```
/// use axum::{
///     extract::Path,
///     routing::get,
///     Router,
///     Json,
/// };
/// use serde::Serialize;
/// use uuid::Uuid;
///
/// #[derive(Serialize)]
/// struct User {
///     id: Uuid,
///     username: String,
/// }
///
/// async fn get_user(Path(user_id) : Path<Uuid>) -> Json<User> {
///     let user = find_user(user_id).await;
///     Json(user)
/// }
///
/// async fn find_user(user_id: Uuid) -> User {
///     // ...
///     # unimplemented!()
/// }
///
/// let app = Router::new().route("/users/{id}", get(get_user));
/// # let _: Router = app;
/// ```
#[derive(Debug, Clone, Copy, Default)]
#[cfg_attr(docsrs, doc(cfg(feature = "json")))]
#[must_use]
pub struct Json<T>(pub T);

impl<T, S> FromRequest<S> for Json<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = JsonRejection;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        if !json_content_type(req.headers()) {
            return Err(MissingJsonContentType.into());
        }

        let bytes = Bytes::from_request(req, state).await?;
        Self::from_bytes(&bytes)
    }
}

impl<T, S> OptionalFromRequest<S> for Json<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = JsonRejection;

    async fn from_request(req: Request, state: &S) -> Result<Option<Self>, Self::Rejection> {
        let headers = req.headers();
        if headers.get(header::CONTENT_TYPE).is_some() {
            if json_content_type(headers) {
                let bytes = Bytes::from_request(req, state).await?;
                Ok(Some(Self::from_bytes(&bytes)?))
            } else {
                Err(MissingJsonContentType.into())
            }
        } else {
            Ok(None)
        }
    }
}

fn json_content_type(headers: &HeaderMap) -> bool {
    let Some(content_type) = headers.get(header::CONTENT_TYPE) else {
        return false;
    };

    let Ok(content_type) = content_type.to_str() else {
        return false;
    };

    let Ok(mime) = content_type.parse::<mime::Mime>() else {
        return false;
    };

    let is_json_content_type = mime.type_() == "application"
        && (mime.subtype() == "json" || mime.suffix().is_some_and(|name| name == "json"));

    is_json_content_type
}

axum_core::__impl_deref!(Json);

impl<T> From<T> for Json<T> {
    fn from(inner: T) -> Self {
        Self(inner)
    }
}

impl<T> Json<T>
where
    T: DeserializeOwned,
{
    /// Construct a `Json<T>` from a byte slice. Most users should prefer to use the `FromRequest` impl
    /// but special cases may require first extracting a `Request` into `Bytes` then optionally
    /// constructing a `Json<T>`.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, JsonRejection> {
        // Extracted into separate fn so it's only compiled once for all T.
        fn make_rejection(err: serde_path_to_error::Error<serde_json::Error>) -> JsonRejection {
            match err.inner().classify() {
                serde_json::error::Category::Data => JsonDataError::from_err(err).into(),
                serde_json::error::Category::Syntax | serde_json::error::Category::Eof => {
                    JsonSyntaxError::from_err(err).into()
                }
                serde_json::error::Category::Io => {
                    if cfg!(debug_assertions) {
                        // we don't use `serde_json::from_reader` and instead always buffer
                        // bodies first, so we shouldn't encounter any IO errors
                        unreachable!()
                    } else {
                        JsonSyntaxError::from_err(err).into()
                    }
                }
            }
        }

        let mut deserializer = serde_json::Deserializer::from_slice(bytes);

        serde_path_to_error::deserialize(&mut deserializer)
            .map_err(make_rejection)
            .and_then(|value| {
                deserializer
                    .end()
                    .map(|()| Self(value))
                    .map_err(|err| JsonSyntaxError::from_err(err).into())
            })
    }
}

impl<T> IntoResponse for Json<T>
where
    T: Serialize,
{
    fn into_response(self) -> Response {
        // Extracted into separate fn so it's only compiled once for all T.
        fn make_response(buf: BytesMut, ser_result: serde_json::Result<()>) -> Response {
            match ser_result {
                Ok(()) => (
                    [(
                        header::CONTENT_TYPE,
                        HeaderValue::from_static(mime::APPLICATION_JSON.as_ref()),
                    )],
                    buf.freeze(),
                )
                    .into_response(),
                Err(err) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    [(
                        header::CONTENT_TYPE,
                        HeaderValue::from_static(mime::TEXT_PLAIN_UTF_8.as_ref()),
                    )],
                    err.to_string(),
                )
                    .into_response(),
            }
        }

        // Use a small initial capacity of 128 bytes like serde_json::to_vec
        // https://docs.rs/serde_json/1.0.82/src/serde_json/ser.rs.html#2189
        match serde_json::to_vec(&self.0) {
            Ok(bytes) => make_response(BytesMut::from(bytes.as_slice()), Ok(())),
            Err(err) => make_response(BytesMut::new(), Err(err)),
        }
    }
}
