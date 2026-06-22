use ::core::fmt;
#[cfg(all(feature = "http1", any(feature = "client", feature = "server")))]
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};

use bytes::Bytes;
#[cfg(all(feature = "http1", any(feature = "client", feature = "server")))]
use futures_channel::oneshot;
#[cfg(all(
    any(feature = "http1", feature = "http2"),
    any(feature = "client", feature = "server")
))]
use futures_core::ready;
#[cfg(all(feature = "http1", any(feature = "client", feature = "server")))]
use futures_core::Stream;
#[cfg(all(feature = "http1", any(feature = "client", feature = "server")))]
use http::HeaderMap;
use http_body::{Body, Frame, SizeHint};
#[cfg(all(feature = "http1", any(feature = "client", feature = "server")))]
use crate::common::mpsc;

#[cfg(all(
    any(feature = "http1", feature = "http2"),
    any(feature = "client", feature = "server")
))]
use super::DecodedLength;
#[cfg(all(feature = "http1", any(feature = "client", feature = "server")))]
use crate::common::watch;
#[cfg(all(feature = "http2", any(feature = "client", feature = "server")))]
use crate::proto::h2::ping;

#[cfg(all(feature = "http1", any(feature = "client", feature = "server")))]
type BodySender = mpsc::UnboundedSender<Result<Bytes, crate::Error>>;
#[cfg(all(feature = "http1", any(feature = "client", feature = "server")))]
type BodyReceiver = mpsc::UnboundedReceiver<Result<Bytes, crate::Error>>;
#[cfg(all(feature = "http1", any(feature = "client", feature = "server")))]
type TrailersSender = oneshot::Sender<HeaderMap>;

/// A stream of `Bytes`, used when receiving bodies from the network.
///
/// Note that Users should not instantiate this struct directly. When working with the hyper client,
/// `Incoming` is returned to you in responses. Similarly, when operating with the hyper server,
/// it is provided within requests.
///
/// # Examples
///
/// ```rust,ignore
/// async fn echo(
///    req: Request<hyper::body::Incoming>,
/// ) -> Result<Response<BoxBody<Bytes, hyper::Error>>, hyper::Error> {
///    //Here, you can process `Incoming`
/// }
/// ```
#[must_use = "streams do nothing unless polled"]
pub struct Incoming {
    kind: Kind,
}

enum Kind {
    Empty,
    #[cfg(all(feature = "http1", any(feature = "client", feature = "server")))]
    Chan {
        content_length: DecodedLength,
        want_tx: watch::Sender,
        data_rx: BodyReceiver,
        trailers_rx: oneshot::Receiver<HeaderMap>,
    },
    #[cfg(all(feature = "http2", any(feature = "client", feature = "server")))]
    H2 {
        content_length: DecodedLength,
        data_done: bool,
        ping: ping::Recorder,
        recv: h2::RecvStream,
    },
    #[cfg(feature = "ffi")]
    Ffi(crate::ffi::UserBody),
}

/// A sender half created through [`Body::channel()`].
///
/// Useful when wanting to stream chunks from another thread.
///
/// ## Body Closing
///
/// Note that the request body will always be closed normally when the sender is dropped (meaning
/// that the empty terminating chunk will be sent to the remote). If you desire to close the
/// connection with an incomplete response (e.g. in the case of an error during asynchronous
/// processing), call the [`Sender::abort()`] method to abort the body in an abnormal fashion.
///
/// [`Body::channel()`]: struct.Body.html#method.channel
/// [`Sender::abort()`]: struct.Sender.html#method.abort
#[must_use = "Sender does nothing unless sent on"]
#[cfg(all(feature = "http1", any(feature = "client", feature = "server")))]
pub(crate) struct Sender {
    want_rx: watch::Receiver,
    data_tx: BodySender,
    trailers_tx: Option<TrailersSender>,
}

#[cfg(all(feature = "http1", any(feature = "client", feature = "server")))]
const WANT_PENDING: usize = 1;
#[cfg(all(feature = "http1", any(feature = "client", feature = "server")))]
const WANT_READY: usize = 2;

impl Incoming {
    /// Create a `Body` stream with an associated sender half.
    ///
    /// Useful when wanting to stream chunks from another thread.
    #[cfg(all(feature = "http1", any(feature = "client", feature = "server")))]
    #[inline]
    #[cfg(all(feature = "http1", any(feature = "client", feature = "server")))]
    pub(crate) fn new_channel(content_length: DecodedLength, wanter: bool) -> (Sender, Incoming) {
        let (data_tx, data_rx) = mpsc::unbounded();
        let (trailers_tx, trailers_rx) = oneshot::channel();

        // If wanter is true, `Sender::poll_ready()` won't becoming ready
        // until the `Body` has been polled for data once.
        let want = if wanter { WANT_PENDING } else { WANT_READY };

        let (want_tx, want_rx) = watch::channel(want);

        let tx = Sender {
            want_rx,
            data_tx,
            trailers_tx: Some(trailers_tx),
        };
        let rx = Incoming::new(Kind::Chan {
            content_length,
            want_tx,
            data_rx,
            trailers_rx,
        });

        (tx, rx)
    }

    fn new(kind: Kind) -> Incoming {
        Incoming { kind }
    }

    #[allow(dead_code)]
    pub(crate) fn empty() -> Incoming {
        Incoming::new(Kind::Empty)
    }

    #[cfg(feature = "ffi")]
    pub(crate) fn ffi() -> Incoming {
        Incoming::new(Kind::Ffi(crate::ffi::UserBody::new()))
    }

    #[cfg(all(feature = "http2", any(feature = "client", feature = "server")))]
    pub(crate) fn h2(
        recv: h2::RecvStream,
        mut content_length: DecodedLength,
        ping: ping::Recorder,
    ) -> Self {
        // If the stream is already EOS, then the "unknown length" is clearly
        // actually ZERO.
        if !content_length.is_exact() && recv.is_end_stream() {
            content_length = DecodedLength::ZERO;
        }

        Incoming::new(Kind::H2 {
            data_done: false,
            ping,
            content_length,
            recv,
        })
    }

    #[cfg(feature = "ffi")]
    pub(crate) fn as_ffi_mut(&mut self) -> &mut crate::ffi::UserBody {
        match self.kind {
            Kind::Ffi(ref mut body) => return body,
            _ => {
                self.kind = Kind::Ffi(crate::ffi::UserBody::new());
            }
        }

        match self.kind {
            Kind::Ffi(ref mut body) => body,
            _ => unreachable!(),
        }
    }
}

impl Body for Incoming {
    type Data = Bytes;
    type Error = crate::Error;

    fn poll_frame(
        #[cfg_attr(
            not(all(
                any(feature = "http1", feature = "http2"),
                any(feature = "client", feature = "server")
            )),
            allow(unused_mut)
        )]
        mut self: Pin<&mut Self>,
        #[cfg_attr(
            not(all(
                any(feature = "http1", feature = "http2"),
                any(feature = "client", feature = "server")
            )),
            allow(unused_variables)
        )]
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        match self.kind {
            Kind::Empty => Poll::Ready(None),
            #[cfg(all(feature = "http1", any(feature = "client", feature = "server")))]
            Kind::Chan {
                content_length: ref mut len,
                ref mut data_rx,
                ref mut want_tx,
                ref mut trailers_rx,
            } => {
                want_tx.send(WANT_READY);

                if !data_rx.is_closed() || !data_rx.is_empty() {
                    if let Some(chunk) = ready!(Pin::new(data_rx).poll_next(cx)) {
                        let chunk = chunk?;
                        len.sub_if(chunk.len() as u64);
                        return Poll::Ready(Some(Ok(Frame::data(chunk))));
                    }
                }

                // check trailers after data is terminated
                match ready!(Pin::new(trailers_rx).poll(cx)) {
                    Ok(t) => Poll::Ready(Some(Ok(Frame::trailers(t)))),
                    Err(_) => Poll::Ready(None),
                }
            }
            #[cfg(all(feature = "http2", any(feature = "client", feature = "server")))]
            Kind::H2 {
                ref mut data_done,
                ref ping,
                recv: ref mut h2,
                content_length: ref mut len,
            } => {
                if !*data_done {
                    match ready!(h2.poll_data(cx)) {
                        Some(Ok(bytes)) => {
                            let _ = h2.flow_control().release_capacity(bytes.len());
                            len.sub_if(bytes.len() as u64);
                            ping.record_data(bytes.len());
                            return Poll::Ready(Some(Ok(Frame::data(bytes))));
                        }
                        Some(Err(e)) => {
                            return match e.reason() {
                                // These reasons should cause the body reading to stop, but not fail it.
                                // The same logic as for `Read for H2Upgraded` is applied here.
                                Some(h2::Reason::NO_ERROR) | Some(h2::Reason::CANCEL) => {
                                    Poll::Ready(None)
                                }
                                _ => Poll::Ready(Some(Err(crate::Error::new_body(e)))),
                            };
                        }
                        None => {
                            *data_done = true;
                            // fall through to trailers
                        }
                    }
                }

                // after data, check trailers
                match ready!(h2.poll_trailers(cx)) {
                    Ok(t) => {
                        ping.record_non_data();
                        Poll::Ready(Ok(t.map(Frame::trailers)).transpose())
                    }
                    Err(e) => Poll::Ready(Some(Err(crate::Error::new_h2(e)))),
                }
            }

            #[cfg(feature = "ffi")]
            Kind::Ffi(ref mut body) => body.poll_data(cx),
        }
    }

    fn is_end_stream(&self) -> bool {
        match self.kind {
            Kind::Empty => true,
            #[cfg(all(feature = "http1", any(feature = "client", feature = "server")))]
            Kind::Chan { content_length, .. } => content_length == DecodedLength::ZERO,
            #[cfg(all(feature = "http2", any(feature = "client", feature = "server")))]
            Kind::H2 { recv: ref h2, .. } => h2.is_end_stream(),
            #[cfg(feature = "ffi")]
            Kind::Ffi(..) => false,
        }
    }

    fn size_hint(&self) -> SizeHint {
        #[cfg(all(
            any(feature = "http1", feature = "http2"),
            any(feature = "client", feature = "server")
        ))]
        fn opt_len(decoded_length: DecodedLength) -> SizeHint {
            if let Some(content_length) = decoded_length.into_opt() {
                SizeHint::with_exact(content_length)
            } else {
                SizeHint::default()
            }
        }

        match self.kind {
            Kind::Empty => SizeHint::with_exact(0),
            #[cfg(all(feature = "http1", any(feature = "client", feature = "server")))]
            Kind::Chan { content_length, .. } => opt_len(content_length),
            #[cfg(all(feature = "http2", any(feature = "client", feature = "server")))]
            Kind::H2 { content_length, .. } => opt_len(content_length),
            #[cfg(feature = "ffi")]
            Kind::Ffi(..) => SizeHint::default(),
        }
    }
}

impl fmt::Debug for Incoming {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        #[cfg(any(
            all(
                any(feature = "http1", feature = "http2"),
                any(feature = "client", feature = "server")
            ),
            feature = "ffi"
        ))]
        #[derive(Debug)]
        struct Streaming;
        #[derive(Debug)]
        struct Empty;

        let mut builder = f.debug_tuple("Body");
        match self.kind {
            Kind::Empty => builder.field(&Empty),
            #[cfg(any(
                all(
                    any(feature = "http1", feature = "http2"),
                    any(feature = "client", feature = "server")
                ),
                feature = "ffi"
            ))]
            _ => builder.field(&Streaming),
        };

        builder.finish()
    }
}

#[cfg(all(feature = "http1", any(feature = "client", feature = "server")))]
impl Sender {
    /// Check to see if this `Sender` can send more data.
    pub(crate) fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<crate::Result<()>> {
        // Check if the receiver end has tried polling for the body yet
        ready!(self.poll_want(cx)?);
        let _ = cx;
        Poll::Ready(Ok(()))
    }

    fn poll_want(&mut self, cx: &mut Context<'_>) -> Poll<crate::Result<()>> {
        match self.want_rx.load(cx) {
            WANT_READY => Poll::Ready(Ok(())),
            WANT_PENDING => Poll::Pending,
            watch::CLOSED => Poll::Ready(Err(crate::Error::new_closed())),
            unexpected => unreachable!("want_rx value: {}", unexpected),
        }
    }

    /// Send data on data channel when it is ready.

    /// Send trailers on trailers channel.
    #[allow(unused)]
    pub(crate) async fn send_trailers(&mut self, trailers: HeaderMap) -> crate::Result<()> {
        let tx = match self.trailers_tx.take() {
            Some(tx) => tx,
            None => return Err(crate::Error::new_closed()),
        };
        tx.send(trailers).map_err(|_| crate::Error::new_closed())
    }

    /// Try to send data on this channel.
    ///
    /// # Errors
    ///
    /// Returns `Err(Bytes)` if the channel could not (currently) accept
    /// another `Bytes`.
    ///
    /// # Note
    ///
    /// This is mostly useful for when trying to send from some other thread
    /// that doesn't have an async context. If in an async context, prefer
    /// `send_data()` instead.
    #[cfg(feature = "http1")]
    pub(crate) fn try_send_data(&mut self, chunk: Bytes) -> Result<(), Bytes> {
        self.data_tx
            .unbounded_send(Ok(chunk))
            .map_err(|err| err.into_inner().expect("just sent Ok"))
    }

    #[cfg(feature = "http1")]
    pub(crate) fn try_send_trailers(
        &mut self,
        trailers: HeaderMap,
    ) -> Result<(), Option<HeaderMap>> {
        let tx = match self.trailers_tx.take() {
            Some(tx) => tx,
            None => return Err(None),
        };

        tx.send(trailers).map_err(Some)
    }

    pub(crate) fn send_error(&mut self, err: crate::Error) {
        let _ = self.data_tx.unbounded_send(Err(err));
    }
}

#[cfg(all(feature = "http1", any(feature = "client", feature = "server")))]
impl fmt::Debug for Sender {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        #[derive(Debug)]
        struct Open;
        #[derive(Debug)]
        struct Closed;

        let mut builder = f.debug_tuple("Sender");
        match self.want_rx.peek() {
            watch::CLOSED => builder.field(&Closed),
            _ => builder.field(&Open),
        };

        builder.finish()
    }
}
