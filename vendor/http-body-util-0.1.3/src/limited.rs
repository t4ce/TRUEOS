use alloc::boxed::Box;
use bytes::Buf;
use http_body::{Body, Frame, SizeHint};
use pin_project_lite::pin_project;
use core::error::Error;
use ::core::fmt;
use core::pin::Pin;
use core::task::{Context, Poll};

pin_project! {
    /// A length limited body.
    ///
    /// This body will return an error if more than the configured number
    /// of bytes are returned on polling the wrapped body.
    #[derive(Clone, Copy, Debug)]
    pub struct Limited<B> {
        remaining: usize,
        #[pin]
        inner: B,
    }
}

impl<B> Limited<B> {
    /// Create a new `Limited`.
    pub fn new(inner: B, limit: usize) -> Self {
        Self {
            remaining: limit,
            inner,
        }
    }
}

impl<B> Body for Limited<B>
where
    B: Body,
    B::Error: Into<Box<dyn Error + Send + Sync>>,
{
    type Data = B::Data;
    type Error = Box<dyn Error + Send + Sync>;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = self.project();
        let res = match this.inner.poll_frame(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(None) => None,
            Poll::Ready(Some(Ok(frame))) => {
                if let Some(data) = frame.data_ref() {
                    if data.remaining() > *this.remaining {
                        *this.remaining = 0;
                        Some(Err(LengthLimitError.into()))
                    } else {
                        *this.remaining -= data.remaining();
                        Some(Ok(frame))
                    }
                } else {
                    Some(Ok(frame))
                }
            }
            Poll::Ready(Some(Err(err))) => Some(Err(err.into())),
        };

        Poll::Ready(res)
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        use core::convert::TryFrom;
        match u64::try_from(self.remaining) {
            Ok(n) => {
                let mut hint = self.inner.size_hint();
                if hint.lower() >= n {
                    hint.set_exact(n)
                } else if let Some(max) = hint.upper() {
                    hint.set_upper(n.min(max))
                } else {
                    hint.set_upper(n)
                }
                hint
            }
            Err(_) => self.inner.size_hint(),
        }
    }
}

/// An error returned when body length exceeds the configured limit.
#[derive(Debug)]
#[non_exhaustive]
pub struct LengthLimitError;

impl fmt::Display for LengthLimitError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("length limit exceeded")
    }
}

impl Error for LengthLimitError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BodyExt, Full, StreamBody};
    use bytes::Bytes;
    use core::convert::Infallible;



    fn body_from_iter<I>(into_iter: I) -> impl Body<Data = Bytes, Error = Infallible>
    where
        I: IntoIterator,
        I::Item: Into<Bytes> + 'static,
        I::IntoIter: Send + 'static,
    {
        let iter = into_iter
            .into_iter()
            .map(|it| Frame::data(it.into()))
            .map(Ok::<_, Infallible>);

        StreamBody::new(futures_util::stream::iter(iter))
    }




    struct SomeTrailers;

    impl Body for SomeTrailers {
        type Data = Bytes;
        type Error = Infallible;

        fn poll_frame(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
            Poll::Ready(Some(Ok(Frame::trailers(http::HeaderMap::new()))))
        }
    }


    #[derive(Debug)]
    struct ErrorBodyError;

    impl fmt::Display for ErrorBodyError {
        fn fmt(&self, _f: &mut fmt::Formatter) -> fmt::Result {
            Ok(())
        }
    }

    impl Error for ErrorBodyError {}

    struct ErrorBody;

    impl Body for ErrorBody {
        type Data = &'static [u8];
        type Error = ErrorBodyError;

        fn poll_frame(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
            Poll::Ready(Some(Err(ErrorBodyError)))
        }
    }

}
