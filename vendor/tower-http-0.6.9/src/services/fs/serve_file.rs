//! Service that serves a file.

use super::ServeDir;
use http::{HeaderValue, Request};
use mime::Mime;
use core::task::{Context, Poll};
#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
use std::path::Path;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use std::path::Path;
use tower_service::Service;

/// Service that serves a file.
#[derive(Clone, Debug)]
pub struct ServeFile(ServeDir);

// Note that this is just a special case of ServeDir
impl ServeFile {
    /// Create a new [`ServeFile`].
    ///
    /// The `Content-Type` will be guessed from the file extension.
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        let guess = guess_mime(path.as_ref());
        let mime = guess
            .first_raw()
            .map(HeaderValue::from_static)
            .unwrap_or_else(|| {
                HeaderValue::from_str(mime::APPLICATION_OCTET_STREAM.as_ref()).unwrap()
            });

        Self(ServeDir::new_single_file(path, mime))
    }

    /// Create a new [`ServeFile`] with a specific mime type.
    ///
    /// # Panics
    ///
    /// Will panic if the mime type isn't a valid [header value].
    ///
    /// [header value]: https://docs.rs/http/latest/http/header/struct.HeaderValue.html
    pub fn new_with_mime<P: AsRef<Path>>(path: P, mime: &Mime) -> Self {
        let mime = HeaderValue::from_str(mime.as_ref()).expect("mime isn't a valid header value");
        Self(ServeDir::new_single_file(path, mime))
    }

    /// Informs the service that it should also look for a precompressed gzip
    /// version of the file.
    ///
    /// If the client has an `Accept-Encoding` header that allows the gzip encoding,
    /// the file `foo.txt.gz` will be served instead of `foo.txt`.
    /// If the precompressed file is not available, or the client doesn't support it,
    /// the uncompressed version will be served instead.
    /// Both the precompressed version and the uncompressed version are expected
    /// to be present in the same directory. Different precompressed
    /// variants can be combined.
    pub fn precompressed_gzip(self) -> Self {
        Self(self.0.precompressed_gzip())
    }

    /// Informs the service that it should also look for a precompressed brotli
    /// version of the file.
    ///
    /// If the client has an `Accept-Encoding` header that allows the brotli encoding,
    /// the file `foo.txt.br` will be served instead of `foo.txt`.
    /// If the precompressed file is not available, or the client doesn't support it,
    /// the uncompressed version will be served instead.
    /// Both the precompressed version and the uncompressed version are expected
    /// to be present in the same directory. Different precompressed
    /// variants can be combined.
    pub fn precompressed_br(self) -> Self {
        Self(self.0.precompressed_br())
    }

    /// Informs the service that it should also look for a precompressed deflate
    /// version of the file.
    ///
    /// If the client has an `Accept-Encoding` header that allows the deflate encoding,
    /// the file `foo.txt.zz` will be served instead of `foo.txt`.
    /// If the precompressed file is not available, or the client doesn't support it,
    /// the uncompressed version will be served instead.
    /// Both the precompressed version and the uncompressed version are expected
    /// to be present in the same directory. Different precompressed
    /// variants can be combined.
    pub fn precompressed_deflate(self) -> Self {
        Self(self.0.precompressed_deflate())
    }

    /// Informs the service that it should also look for a precompressed zstd
    /// version of the file.
    ///
    /// If the client has an `Accept-Encoding` header that allows the zstd encoding,
    /// the file `foo.txt.zst` will be served instead of `foo.txt`.
    /// If the precompressed file is not available, or the client doesn't support it,
    /// the uncompressed version will be served instead.
    /// Both the precompressed version and the uncompressed version are expected
    /// to be present in the same directory. Different precompressed
    /// variants can be combined.
    pub fn precompressed_zstd(self) -> Self {
        Self(self.0.precompressed_zstd())
    }

    /// Set a specific read buffer chunk size.
    ///
    /// The default capacity is 64kb.
    pub fn with_buf_chunk_size(self, chunk_size: usize) -> Self {
        Self(self.0.with_buf_chunk_size(chunk_size))
    }

    /// Call the service and get a future that contains any `std::io::Error` that might have
    /// happened.
    ///
    /// See [`ServeDir::try_call`] for more details.
    pub fn try_call<ReqBody>(
        &mut self,
        req: Request<ReqBody>,
    ) -> super::serve_dir::future::ResponseFuture<ReqBody>
    where
        ReqBody: Send + 'static,
    {
        self.0.try_call(req)
    }
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn guess_mime(path: &Path) -> mime_guess::MimeGuess {
    mime_guess::from_path(path)
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn guess_mime(path: &Path) -> mime_guess::MimeGuess {
    mime_guess::from_path(path)
}

impl<ReqBody> Service<Request<ReqBody>> for ServeFile
where
    ReqBody: Send + 'static,
{
    type Error = <ServeDir as Service<Request<ReqBody>>>::Error;
    type Response = <ServeDir as Service<Request<ReqBody>>>::Response;
    type Future = <ServeDir as Service<Request<ReqBody>>>::Future;

    #[inline]
    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    #[inline]
    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        self.0.call(req)
    }
}
