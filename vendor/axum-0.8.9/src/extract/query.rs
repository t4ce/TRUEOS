#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use crate::prelude::rust_2024::*;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use alloc::borrow::ToOwned;
use super::{rejection::*, FromRequestParts};
use http::{request::Parts, Uri};
use serde_core::de::DeserializeOwned;

/// Extractor that deserializes query strings into some type.
///
/// `T` is expected to implement [`serde::Deserialize`].
///
/// # Examples
///
/// ```rust,no_run
/// use axum::{
///     extract::Query,
///     routing::get,
///     Router,
/// };
/// use serde::Deserialize;
///
/// #[derive(Deserialize)]
/// struct Pagination {
///     page: usize,
///     per_page: usize,
/// }
///
/// // This will parse query strings like `?page=2&per_page=30` into `Pagination`
/// // structs.
/// async fn list_things(pagination: Query<Pagination>) {
///     let pagination: Pagination = pagination.0;
///
///     // ...
/// }
///
/// let app = Router::new().route("/list_things", get(list_things));
/// # let _: Router = app;
/// ```
///
/// If the query string cannot be parsed it will reject the request with a `400
/// Bad Request` response.
///
/// For handling values being empty vs missing see the [query-params-with-empty-strings][example]
/// example.
///
/// [example]: https://github.com/tokio-rs/axum/blob/v0.8.x/examples/query-params-with-empty-strings/src/main.rs
///
/// For handling multiple values for the same query parameter, in a `?foo=1&foo=2&foo=3`
/// fashion, use [`axum_extra::extract::Query`] instead.
///
/// [`axum_extra::extract::Query`]: https://docs.rs/axum-extra/latest/axum_extra/extract/struct.Query.html
#[cfg_attr(docsrs, doc(cfg(feature = "query")))]
#[derive(Debug, Clone, Copy, Default)]
pub struct Query<T>(pub T);

impl<T, S> FromRequestParts<S> for Query<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = QueryRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Self::try_from_uri(&parts.uri)
    }
}

impl<T> Query<T>
where
    T: DeserializeOwned,
{
    /// Attempts to construct a [`Query`] from a reference to a [`Uri`].
    ///
    /// # Example
    /// ```
    /// use axum::extract::Query;
    /// use http::Uri;
    /// use serde::Deserialize;
    ///
    /// #[derive(Deserialize)]
    /// struct ExampleParams {
    ///     foo: String,
    ///     bar: u32,
    /// }
    ///
    /// let uri: Uri = "http://example.com/path?foo=hello&bar=42".parse().unwrap();
    /// let result: Query<ExampleParams> = Query::try_from_uri(&uri).unwrap();
    /// assert_eq!(result.foo, String::from("hello"));
    /// assert_eq!(result.bar, 42);
    /// ```
    pub fn try_from_uri(value: &Uri) -> Result<Self, QueryRejection> {
        let query = value.query().unwrap_or_default();
        let deserializer =
            serde_urlencoded::Deserializer::new(form_urlencoded::parse(query.as_bytes()));
        let params = serde_path_to_error::deserialize(deserializer)
            .map_err(FailedToDeserializeQueryString::from_err)?;
        Ok(Query(params))
    }
}
