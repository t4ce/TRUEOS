#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use crate::prelude::rust_2021::*;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use alloc::borrow::ToOwned;
use super::{rejection::*, FromRequestParts};
use crate::routing::{RouteId, NEST_TAIL_PARAM_CAPTURE};
use axum_core::extract::OptionalFromRequestParts;
use http::request::Parts;
use std::{collections::HashMap, convert::Infallible, sync::Arc};

/// Access the path in the router that matches the request.
///
/// ```
/// use axum::{
///     Router,
///     extract::MatchedPath,
///     routing::get,
/// };
///
/// let app = Router::new().route(
///     "/users/{id}",
///     get(|path: MatchedPath| async move {
///         let path = path.as_str();
///         // `path` will be "/users/{id}"
///     })
/// );
/// # let _: Router = app;
/// ```
///
/// # Accessing `MatchedPath` via extensions
///
/// `MatchedPath` can also be accessed from middleware via request extensions.
///
/// This is useful for example with [`Trace`](tower_http::trace::Trace) to
/// create a span that contains the matched path:
///
/// ```
/// use axum::{
///     Router,
///     extract::{Request, MatchedPath},
///     routing::get,
/// };
/// use tower_http::trace::TraceLayer;
///
/// let app = Router::new()
///     .route("/users/{id}", get(|| async { /* ... */ }))
///     .layer(
///         TraceLayer::new_for_http().make_span_with(|req: &Request<_>| {
///             let path = if let Some(path) = req.extensions().get::<MatchedPath>() {
///                 path.as_str()
///             } else {
///                 req.uri().path()
///             };
///             tracing::info_span!("http-request", %path)
///         }),
///     );
/// # let _: Router = app;
/// ```
#[cfg_attr(docsrs, doc(cfg(feature = "matched-path")))]
#[derive(Clone, Debug)]
pub struct MatchedPath(pub(crate) Arc<str>);

impl MatchedPath {
    /// Returns a `str` representation of the path.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<S> FromRequestParts<S> for MatchedPath
where
    S: Send + Sync,
{
    type Rejection = MatchedPathRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let matched_path = parts
            .extensions
            .get::<Self>()
            .ok_or(MatchedPathRejection::MatchedPathMissing(MatchedPathMissing))?
            .clone();

        Ok(matched_path)
    }
}

impl<S> OptionalFromRequestParts<S> for MatchedPath
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> Result<Option<Self>, Self::Rejection> {
        Ok(parts.extensions.get::<Self>().cloned())
    }
}

#[derive(Clone, Debug)]
struct MatchedNestedPath(Arc<str>);

pub(crate) fn set_matched_path_for_request(
    id: RouteId,
    route_id_to_path: &HashMap<RouteId, Arc<str>>,
    extensions: &mut http::Extensions,
) {
    let matched_path = if let Some(matched_path) = route_id_to_path.get(&id) {
        matched_path
    } else {
        #[cfg(debug_assertions)]
        panic!("should always have a matched path for a route id");
        #[cfg(not(debug_assertions))]
        return;
    };

    let matched_path = append_nested_matched_path(matched_path, extensions);

    if matched_path.ends_with(NEST_TAIL_PARAM_CAPTURE) {
        extensions.insert(MatchedNestedPath(matched_path));
        debug_assert!(extensions.remove::<MatchedPath>().is_none());
    } else {
        extensions.insert(MatchedPath(matched_path));
        extensions.remove::<MatchedNestedPath>();
    }
}

// a previous `MatchedPath` might exist if we're inside a nested Router
fn append_nested_matched_path(matched_path: &Arc<str>, extensions: &http::Extensions) -> Arc<str> {
    if let Some(previous) = extensions
        .get::<MatchedPath>()
        .map(|matched_path| matched_path.as_str())
        .or_else(|| Some(&extensions.get::<MatchedNestedPath>()?.0))
    {
        let previous = previous
            .strip_suffix(NEST_TAIL_PARAM_CAPTURE)
            .unwrap_or(previous);

        let matched_path = format!("{previous}{matched_path}");
        matched_path.into()
    } else {
        Arc::clone(matched_path)
    }
}
