#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use crate::prelude::rust_2024::*;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use alloc::borrow::ToOwned;
use http::{Request, Uri};
use std::{
    sync::Arc,
    task::{Context, Poll},
};
use tower::Layer;
use tower_layer::layer_fn;
use tower_service::Service;

#[derive(Clone)]
pub(super) struct StripPrefix<S> {
    inner: S,
    prefix: Arc<str>,
}

impl<S> StripPrefix<S> {
    pub(super) fn layer(prefix: impl Into<Arc<str>>) -> impl Layer<S, Service = Self> + Clone {
        let prefix = prefix.into();
        layer_fn(move |inner| Self {
            inner,
            prefix: Arc::clone(&prefix),
        })
    }
}

impl<S, B> Service<Request<B>> for StripPrefix<S>
where
    S: Service<Request<B>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<B>) -> Self::Future {
        if let Some(new_uri) = strip_prefix(req.uri(), &self.prefix) {
            *req.uri_mut() = new_uri;
        }
        self.inner.call(req)
    }
}

fn strip_prefix(uri: &Uri, prefix: &str) -> Option<Uri> {
    let path_and_query = uri.path_and_query()?;

    // Check whether the prefix matches the path and if so how long the matching prefix is.
    //
    // For example:
    //
    // prefix = /api
    // path   = /api/users
    //          ^^^^ this much is matched and the length is 4. Thus if we chop off the first 4
    //          characters we get the remainder
    //
    // prefix = /api/{version}
    // path   = /api/v0/users
    //          ^^^^^^^ this much is matched and the length is 7.
    let mut matching_prefix_length = Some(0);
    for item in zip_longest(segments(path_and_query.path()), segments(prefix)) {
        // count the `/`
        *matching_prefix_length.as_mut().unwrap() += 1;

        match item {
            Item::Both(path_segment, prefix_segment) => {
                if is_capture(prefix_segment) || path_segment == prefix_segment {
                    // the prefix segment is either a param, which matches anything, or
                    // it actually matches the path segment
                    *matching_prefix_length.as_mut().unwrap() += path_segment.len();
                } else if prefix_segment.is_empty() {
                    // the prefix ended in a `/` so we got a match.
                    //
                    // For example:
                    //
                    // prefix = /foo/
                    // path   = /foo/bar
                    //
                    // The prefix matches and the new path should be `/bar`
                    break;
                } else {
                    // the prefix segment didn't match so there is no match
                    matching_prefix_length = None;
                    break;
                }
            }
            // the path had more segments than the prefix but we got a match.
            //
            // For example:
            //
            // prefix = /foo
            // path   = /foo/bar
            Item::First(_) => {
                break;
            }
            // the prefix had more segments than the path so there is no match
            Item::Second(_) => {
                matching_prefix_length = None;
                break;
            }
        }
    }

    // if the prefix matches it will always do so up until a `/`, it cannot match only
    // part of a segment. Therefore this will always be at a char boundary and `split_at` won't
    // panic
    let after_prefix = uri.path().split_at(matching_prefix_length?).1;

    let new_path_and_query = match (after_prefix.starts_with('/'), path_and_query.query()) {
        (true, None) => after_prefix.parse().unwrap(),
        (true, Some(query)) => format!("{after_prefix}?{query}").parse().unwrap(),
        (false, None) => format!("/{after_prefix}").parse().unwrap(),
        (false, Some(query)) => format!("/{after_prefix}?{query}").parse().unwrap(),
    };

    let mut parts = uri.clone().into_parts();
    parts.path_and_query = Some(new_path_and_query);

    Some(Uri::from_parts(parts).unwrap())
}

fn segments(s: &str) -> impl Iterator<Item = &str> {
    assert!(
        s.starts_with('/'),
        "path didn't start with '/'. axum should have caught this higher up."
    );

    s.split('/')
        // skip one because paths always start with `/` so `/a/b` would become ["", "a", "b"]
        // otherwise
        .skip(1)
}

fn zip_longest<I, I2>(a: I, b: I2) -> impl Iterator<Item = Item<I::Item>>
where
    I: Iterator,
    I2: Iterator<Item = I::Item>,
{
    let a = a.map(Some).chain(core::iter::repeat_with(|| None));
    let b = b.map(Some).chain(core::iter::repeat_with(|| None));
    a.zip(b).map_while(|(a, b)| match (a, b) {
        (Some(a), Some(b)) => Some(Item::Both(a, b)),
        (Some(a), None) => Some(Item::First(a)),
        (None, Some(b)) => Some(Item::Second(b)),
        (None, None) => None,
    })
}

fn is_capture(segment: &str) -> bool {
    segment.starts_with('{')
        && segment.ends_with('}')
        && !segment.starts_with("{{")
        && !segment.ends_with("}}")
        && !segment.starts_with("{*")
}

#[derive(Debug)]
enum Item<T> {
    Both(T, T),
    First(T),
    Second(T),
}
