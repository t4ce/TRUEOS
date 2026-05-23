//! Errors

/// An error indicating that the service with a `K`-typed key failed with an
/// error.
pub struct Failed<K>(pub K, pub crate::BoxError);

// === Failed ===

impl<K: ::core::fmt::Debug> ::core::fmt::Debug for Failed<K> {
    fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
        f.debug_tuple("Failed")
            .field(&self.0)
            .field(&self.1)
            .finish()
    }
}

impl<K> ::core::fmt::Display for Failed<K> {
    fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
        self.1.fmt(f)
    }
}

impl<K: ::core::fmt::Debug> core::error::Error for Failed<K> {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        Some(&*self.1)
    }
}
