use std::{
    convert::Infallible,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::{Buf, Bytes};
use http::HeaderMap;
use http_body::{Body, Frame};

use crate::util::BufList;

/// A collected body produced by [`BodyExt::collect`] which collects all the DATA frames
/// and trailers.
///
/// [`BodyExt::collect`]: crate::BodyExt::collect
#[derive(Debug)]
pub struct Collected<B> {
    bufs: BufList<B>,
    trailers: Option<HeaderMap>,
}

impl<B: Buf> Collected<B> {
    /// If there is a trailers frame buffered, returns a reference to it.
    ///
    /// Returns `None` if the body contained no trailers.
    pub fn trailers(&self) -> Option<&HeaderMap> {
        self.trailers.as_ref()
    }

    /// Aggregate this buffered into a [`Buf`].
    pub fn aggregate(self) -> impl Buf {
        self.bufs
    }

    /// Convert this body into a [`Bytes`].
    pub fn to_bytes(mut self) -> Bytes {
        self.bufs.copy_to_bytes(self.bufs.remaining())
    }

    pub(crate) fn push_frame(&mut self, frame: Frame<B>) {
        let frame = match frame.into_data() {
            Ok(data) => {
                // Only push this frame if it has some data in it, to avoid crashing on
                // `BufList::push`.
                if data.has_remaining() {
                    self.bufs.push(data);
                }
                return;
            }
            Err(frame) => frame,
        };

        if let Ok(trailers) = frame.into_trailers() {
            if let Some(current) = &mut self.trailers {
                current.extend(trailers);
            } else {
                self.trailers = Some(trailers);
            }
        };
    }
}

impl<B: Buf> Body for Collected<B> {
    type Data = B;
    type Error = Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let frame = if let Some(data) = self.bufs.pop() {
            Frame::data(data)
        } else if let Some(trailers) = self.trailers.take() {
            Frame::trailers(trailers)
        } else {
            return Poll::Ready(None);
        };

        Poll::Ready(Some(Ok(frame)))
    }
}

impl<B> Default for Collected<B> {
    fn default() -> Self {
        Self {
            bufs: BufList::default(),
            trailers: None,
        }
    }
}

impl<B> Unpin for Collected<B> {}
