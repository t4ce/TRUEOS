#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use crate::prelude::rust_2021::*;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use alloc::borrow::ToOwned;
use super::FromRequestParts;
use http::request::Parts;
use core::convert::Infallible;

/// Extractor that extracts the raw query string, without parsing it.
///
/// # Example
///
/// ```rust,no_run
/// use axum::{
///     extract::RawQuery,
///     routing::get,
///     Router,
/// };
/// use futures_util::StreamExt;
///
/// async fn handler(RawQuery(query): RawQuery) {
///     // ...
/// }
///
/// let app = Router::new().route("/users", get(handler));
/// # let _: Router = app;
/// ```
#[derive(Debug)]
pub struct RawQuery(pub Option<String>);

impl<S> FromRequestParts<S> for RawQuery
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let query = parts.uri.query().map(|query| query.to_owned());
        Ok(Self(query))
    }
}
