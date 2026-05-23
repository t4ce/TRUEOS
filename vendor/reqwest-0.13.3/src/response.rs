use url::Url;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ResponseUrl(pub Url);

/// Extension trait for http::response::Builder objects
///
/// Allows the user to add a `Url` to the http::Response
pub trait ResponseBuilderExt {
    /// A builder method for the `http::response::Builder` type that allows the user to add a `Url`
    /// to the `http::Response`
    fn url(self, url: Url) -> Self;
}

impl ResponseBuilderExt for http::response::Builder {
    fn url(self, url: Url) -> Self {
        self.extension(ResponseUrl(url))
    }
}
