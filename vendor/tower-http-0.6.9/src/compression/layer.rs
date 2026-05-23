use super::{Compression, Predicate};
use crate::compression::predicate::DefaultPredicate;
use crate::compression::CompressionLevel;
use crate::compression_utils::AcceptEncoding;
use tower_layer::Layer;

/// Compress response bodies of the underlying service.
///
/// This uses the `Accept-Encoding` header to pick an appropriate encoding and adds the
/// `Content-Encoding` header to responses.
///
/// See the [module docs](crate::compression) for more details.
#[derive(Clone, Debug, Default)]
pub struct CompressionLayer<P = DefaultPredicate> {
    accept: AcceptEncoding,
    predicate: P,
    quality: CompressionLevel,
}

impl<S, P> Layer<S> for CompressionLayer<P>
where
    P: Predicate,
{
    type Service = Compression<S, P>;

    fn layer(&self, inner: S) -> Self::Service {
        Compression {
            inner,
            accept: self.accept,
            predicate: self.predicate.clone(),
            quality: self.quality,
        }
    }
}

impl CompressionLayer {
    /// Creates a new [`CompressionLayer`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets whether to enable the gzip encoding.
    #[cfg(feature = "compression-gzip")]
    pub fn gzip(mut self, enable: bool) -> Self {
        self.accept.set_gzip(enable);
        self
    }

    /// Sets whether to enable the Deflate encoding.
    #[cfg(feature = "compression-deflate")]
    pub fn deflate(mut self, enable: bool) -> Self {
        self.accept.set_deflate(enable);
        self
    }

    /// Sets whether to enable the Brotli encoding.
    #[cfg(feature = "compression-br")]
    pub fn br(mut self, enable: bool) -> Self {
        self.accept.set_br(enable);
        self
    }

    /// Sets whether to enable the Zstd encoding.
    #[cfg(feature = "compression-zstd")]
    pub fn zstd(mut self, enable: bool) -> Self {
        self.accept.set_zstd(enable);
        self
    }

    /// Sets the compression quality.
    pub fn quality(mut self, quality: CompressionLevel) -> Self {
        self.quality = quality;
        self
    }

    /// Disables the gzip encoding.
    ///
    /// This method is available even if the `gzip` crate feature is disabled.
    pub fn no_gzip(mut self) -> Self {
        self.accept.set_gzip(false);
        self
    }

    /// Disables the Deflate encoding.
    ///
    /// This method is available even if the `deflate` crate feature is disabled.
    pub fn no_deflate(mut self) -> Self {
        self.accept.set_deflate(false);
        self
    }

    /// Disables the Brotli encoding.
    ///
    /// This method is available even if the `br` crate feature is disabled.
    pub fn no_br(mut self) -> Self {
        self.accept.set_br(false);
        self
    }

    /// Disables the Zstd encoding.
    ///
    /// This method is available even if the `zstd` crate feature is disabled.
    pub fn no_zstd(mut self) -> Self {
        self.accept.set_zstd(false);
        self
    }

    /// Replace the current compression predicate.
    ///
    /// See [`Compression::compress_when`] for more details.
    pub fn compress_when<C>(self, predicate: C) -> CompressionLayer<C>
    where
        C: Predicate,
    {
        CompressionLayer {
            accept: self.accept,
            predicate,
            quality: self.quality,
        }
    }
}
