use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use futures_core::ready;
use http::HeaderMap;
use http_body::{Body, Frame};
use pin_project_lite::pin_project;

pin_project! {
    /// Adds trailers to a body.
    ///
    /// See [`BodyExt::with_trailers`] for more details.
    pub struct WithTrailers<T, F> {
        #[pin]
        state: State<T, F>,
    }
}

impl<T, F> WithTrailers<T, F> {
    pub(crate) fn new(body: T, trailers: F) -> Self {
        Self {
            state: State::PollBody {
                body,
                trailers: Some(trailers),
            },
        }
    }
}

pin_project! {
    #[project = StateProj]
    enum State<T, F> {
        PollBody {
            #[pin]
            body: T,
            trailers: Option<F>,
        },
        PollTrailers {
            #[pin]
            trailers: F,
            prev_trailers: Option<HeaderMap>,
        },
        Done,
    }
}

impl<T, F> Body for WithTrailers<T, F>
where
    T: Body,
    F: Future<Output = Option<Result<HeaderMap, T::Error>>>,
{
    type Data = T::Data;
    type Error = T::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        loop {
            let mut this = self.as_mut().project();

            match this.state.as_mut().project() {
                StateProj::PollBody { body, trailers } => match ready!(body.poll_frame(cx)?) {
                    Some(frame) => match frame.into_trailers() {
                        Ok(prev_trailers) => {
                            let trailers = trailers.take().unwrap();
                            this.state.set(State::PollTrailers {
                                trailers,
                                prev_trailers: Some(prev_trailers),
                            });
                        }
                        Err(frame) => {
                            return Poll::Ready(Some(Ok(frame)));
                        }
                    },
                    None => {
                        let trailers = trailers.take().unwrap();
                        this.state.set(State::PollTrailers {
                            trailers,
                            prev_trailers: None,
                        });
                    }
                },
                StateProj::PollTrailers {
                    trailers,
                    prev_trailers,
                } => {
                    let trailers = ready!(trailers.poll(cx)?);
                    match (trailers, prev_trailers.take()) {
                        (None, None) => return Poll::Ready(None),
                        (None, Some(trailers)) | (Some(trailers), None) => {
                            this.state.set(State::Done);
                            return Poll::Ready(Some(Ok(Frame::trailers(trailers))));
                        }
                        (Some(new_trailers), Some(mut prev_trailers)) => {
                            prev_trailers.extend(new_trailers);
                            this.state.set(State::Done);
                            return Poll::Ready(Some(Ok(Frame::trailers(prev_trailers))));
                        }
                    }
                }
                StateProj::Done => {
                    return Poll::Ready(None);
                }
            }
        }
    }

    #[inline]
    fn size_hint(&self) -> http_body::SizeHint {
        match &self.state {
            State::PollBody { body, .. } => body.size_hint(),
            State::PollTrailers { .. } | State::Done => Default::default(),
        }
    }
}
