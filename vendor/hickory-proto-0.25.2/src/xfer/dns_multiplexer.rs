// Copyright 2015-2023 Benjamin Fry <benjaminfry@me.com>
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// https://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// https://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! `DnsMultiplexer` and associated types implement the state machines for sending DNS messages while using the underlying streams.

use alloc::{boxed::Box, sync::Arc};
use core::{
    borrow::Borrow,
    fmt::{self, Display},
    marker::Unpin,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use std::{
    collections::{HashMap, hash_map::Entry},
    time::{SystemTime, UNIX_EPOCH},
};

use futures_channel::mpsc;
use futures_util::{
    FutureExt,
    future::Future,
    ready,
    stream::{Stream, StreamExt},
};
use rand::Rng;
use tracing::debug;

use crate::{
    DnsStreamHandle,
    error::{ProtoError, ProtoErrorKind},
    op::{MessageFinalizer, MessageVerifier},
    runtime::Time,
    xfer::{
        BufDnsStreamHandle, CHANNEL_BUFFER_SIZE, DnsClientStream, DnsRequest, DnsRequestSender,
        DnsResponse, DnsResponseStream, SerialMessage, ignore_send,
    },
};

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn hickory_system_time_now() -> SystemTime {
    UNIX_EPOCH + core::time::Duration::from_secs(unsafe { trueos_platform_unix_seconds() })
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn hickory_system_time_now() -> SystemTime {
    SystemTime::now()
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
unsafe extern "Rust" {
    fn trueos_platform_unix_seconds() -> u64;
}

const QOS_MAX_RECEIVE_MSGS: usize = 100; // max number of messages to receive from the UDP socket

struct ActiveRequest {
    // the completion is the channel for a response to the original request
    completion: mpsc::Sender<Result<DnsResponse, ProtoError>>,
    request_id: u16,
    timeout: Box<dyn Future<Output = ()> + Send + Unpin>,
    verifier: Option<MessageVerifier>,
}

impl ActiveRequest {
    fn new(
        completion: mpsc::Sender<Result<DnsResponse, ProtoError>>,
        request_id: u16,
        timeout: Box<dyn Future<Output = ()> + Send + Unpin>,
        verifier: Option<MessageVerifier>,
    ) -> Self {
        Self {
            completion,
            request_id,
            // request,
            timeout,
            verifier,
        }
    }

    /// polls the timeout and converts the error
    fn poll_timeout(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        self.timeout.poll_unpin(cx)
    }

    /// Returns true of the other side canceled the request
    fn is_canceled(&self) -> bool {
        self.completion.is_closed()
    }

    /// the request id of the message that was sent
    fn request_id(&self) -> u16 {
        self.request_id
    }

    /// Sends an error
    fn complete_with_error(mut self, error: ProtoError) {
        ignore_send(self.completion.try_send(Err(error)));
    }
}

/// A DNS Client implemented over futures-rs.
///
/// This Client is generic and capable of wrapping UDP, TCP, and other underlying DNS protocol
///  implementations. This should be used for underlying protocols that do not natively support
///  multiplexed sessions.
#[must_use = "futures do nothing unless polled"]
pub struct DnsMultiplexer<S>
where
    S: DnsClientStream + 'static,
{
    stream: S,
    timeout_duration: Duration,
    stream_handle: BufDnsStreamHandle,
    active_requests: HashMap<u16, ActiveRequest>,
    signer: Option<Arc<dyn MessageFinalizer>>,
    is_shutdown: bool,
}

impl<S> DnsMultiplexer<S>
where
    S: DnsClientStream + Unpin + 'static,
{
    /// Spawns a new DnsMultiplexer Stream. This uses a default timeout of 5 seconds for all requests.
    ///
    /// # Arguments
    ///
    /// * `stream` - A stream of bytes that can be used to send/receive DNS messages
    ///   (see TcpClientStream or UdpClientStream)
    /// * `stream_handle` - The handle for the `stream` on which bytes can be sent/received.
    /// * `signer` - An optional signer for requests, needed for Updates with Sig0, otherwise not needed
    #[allow(clippy::new_ret_no_self)]
    pub fn new<F>(
        stream: F,
        stream_handle: BufDnsStreamHandle,
        signer: Option<Arc<dyn MessageFinalizer>>,
    ) -> DnsMultiplexerConnect<F, S>
    where
        F: Future<Output = Result<S, ProtoError>> + Send + Unpin + 'static,
    {
        Self::with_timeout(stream, stream_handle, Duration::from_secs(5), signer)
    }

    /// Spawns a new DnsMultiplexer Stream.
    ///
    /// # Arguments
    ///
    /// * `stream` - A stream of bytes that can be used to send/receive DNS messages
    ///   (see TcpClientStream or UdpClientStream)
    /// * `stream_handle` - The handle for the `stream` on which bytes can be sent/received.
    /// * `timeout_duration` - All requests may fail due to lack of response, this is the time to
    ///   wait for a response before canceling the request.
    /// * `signer` - An optional signer for requests, needed for Updates with Sig0, otherwise not needed
    pub fn with_timeout<F>(
        stream: F,
        stream_handle: BufDnsStreamHandle,
        timeout_duration: Duration,
        signer: Option<Arc<dyn MessageFinalizer>>,
    ) -> DnsMultiplexerConnect<F, S>
    where
        F: Future<Output = Result<S, ProtoError>> + Send + Unpin + 'static,
    {
        DnsMultiplexerConnect {
            stream,
            stream_handle: Some(stream_handle),
            timeout_duration,
            signer,
        }
    }

    /// loop over active_requests and remove cancelled requests
    ///  this should free up space if we already had 4096 active requests
    fn drop_cancelled(&mut self, cx: &mut Context<'_>) {
        let mut canceled = HashMap::<u16, ProtoError>::new();
        for (&id, active_req) in &mut self.active_requests {
            if active_req.is_canceled() {
                canceled.insert(id, ProtoError::from("requestor canceled"));
            }

            // check for timeouts...
            match active_req.poll_timeout(cx) {
                Poll::Ready(()) => {
                    debug!("request timed out: {}", id);
                    canceled.insert(id, ProtoError::from(ProtoErrorKind::Timeout));
                }
                Poll::Pending => (),
            }
        }

        // drop all the canceled requests
        for (id, error) in canceled {
            if let Some(active_request) = self.active_requests.remove(&id) {
                // complete the request, it's failed...
                active_request.complete_with_error(error);
            }
        }
    }

    /// creates random query_id, validates against all active queries
    fn next_random_query_id(&self) -> Result<u16, ProtoError> {
        let mut rand = rand::rng();

        for _ in 0..100 {
            let id: u16 = rand.random(); // the range is [0 ... u16::max]

            if !self.active_requests.contains_key(&id) {
                return Ok(id);
            }
        }

        Err(ProtoError::from(
            "id space exhausted, consider filing an issue",
        ))
    }

    /// Closes all outstanding completes with a closed stream error
    fn stream_closed_close_all(&mut self, error: ProtoError) {
        debug!(error = error.as_dyn(), stream = %self.stream);

        for (_, active_request) in self.active_requests.drain() {
            // complete the request, it's failed...
            active_request.complete_with_error(error.clone());
        }
    }
}

/// A wrapper for a future DnsExchange connection
#[must_use = "futures do nothing unless polled"]
pub struct DnsMultiplexerConnect<F, S>
where
    F: Future<Output = Result<S, ProtoError>> + Send + Unpin + 'static,
    S: Stream<Item = Result<SerialMessage, ProtoError>> + Unpin,
{
    stream: F,
    stream_handle: Option<BufDnsStreamHandle>,
    timeout_duration: Duration,
    signer: Option<Arc<dyn MessageFinalizer>>,
}

impl<F, S> Future for DnsMultiplexerConnect<F, S>
where
    F: Future<Output = Result<S, ProtoError>> + Send + Unpin + 'static,
    S: DnsClientStream + Unpin + 'static,
{
    type Output = Result<DnsMultiplexer<S>, ProtoError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let stream: S = ready!(self.stream.poll_unpin(cx))?;

        Poll::Ready(Ok(DnsMultiplexer {
            stream,
            timeout_duration: self.timeout_duration,
            stream_handle: self
                .stream_handle
                .take()
                .expect("must not poll after complete"),
            active_requests: HashMap::new(),
            signer: self.signer.clone(),
            is_shutdown: false,
        }))
    }
}

impl<S> Display for DnsMultiplexer<S>
where
    S: DnsClientStream + 'static,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        write!(formatter, "{}", self.stream)
    }
}

impl<S> DnsRequestSender for DnsMultiplexer<S>
where
    S: DnsClientStream + Unpin + 'static,
{
    fn send_message(&mut self, request: DnsRequest) -> DnsResponseStream {
        if self.is_shutdown {
            panic!("can not send messages after stream is shutdown")
        }

        if self.active_requests.len() > CHANNEL_BUFFER_SIZE {
            return ProtoError::from(ProtoErrorKind::Busy).into();
        }

        let query_id = match self.next_random_query_id() {
            Ok(id) => id,
            Err(e) => return e.into(),
        };

        let (mut request, _) = request.into_parts();
        request.set_id(query_id);

        let now = match hickory_system_time_now().duration_since(UNIX_EPOCH) {
            Ok(now) => now.as_secs(),
            Err(_) => return ProtoError::from("Current time is before the Unix epoch.").into(),
        };

        // TODO: truncates u64 to u32, error on overflow?
        let now = now as u32;

        let mut verifier = None;
        if let Some(signer) = &self.signer {
            if signer.should_finalize_message(&request) {
                match request.finalize(signer.borrow(), now) {
                    Ok(answer_verifier) => verifier = answer_verifier,
                    Err(e) => {
                        debug!("could not sign message: {}", e);
                        return e.into();
                    }
                }
            }
        }

        // store a Timeout for this message before sending
        let timeout = S::Time::delay_for(self.timeout_duration);

        let (complete, receiver) = mpsc::channel(CHANNEL_BUFFER_SIZE);

        // send the message
        let active_request =
            ActiveRequest::new(complete, request.id(), Box::new(timeout), verifier);

        match request.to_vec() {
            Ok(buffer) => {
                debug!(id = %active_request.request_id(), "sending message");
                let serial_message = SerialMessage::new(buffer, self.stream.name_server_addr());

                debug!(
                    "final message: {}",
                    serial_message
                        .to_message()
                        .expect("bizarre we just made this message")
                );

                // add to the map -after- the client send b/c we don't want to put it in the map if
                //  we ended up returning an error from the send.
                match self.stream_handle.send(serial_message) {
                    Ok(()) => self
                        .active_requests
                        .insert(active_request.request_id(), active_request),
                    Err(err) => return err.into(),
                };
            }
            Err(e) => {
                debug!(
                    id = %active_request.request_id(),
                    error = e.as_dyn(),
                    "error message"
                );
                // complete with the error, don't add to the map of active requests
                return e.into();
            }
        }

        receiver.into()
    }

    fn shutdown(&mut self) {
        self.is_shutdown = true;
    }

    fn is_shutdown(&self) -> bool {
        self.is_shutdown
    }
}

impl<S> Stream for DnsMultiplexer<S>
where
    S: DnsClientStream + Unpin + 'static,
{
    type Item = Result<(), ProtoError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Always drop the cancelled queries first
        self.drop_cancelled(cx);

        if self.is_shutdown && self.active_requests.is_empty() {
            debug!("stream is done: {}", self);
            return Poll::Ready(None);
        }

        // Collect all inbound requests, max 100 at a time for QoS
        //   by having a max we will guarantee that the client can't be DOSed in this loop
        // TODO: make the QoS configurable
        let mut messages_received = 0;
        for i in 0..QOS_MAX_RECEIVE_MSGS {
            match self.stream.poll_next_unpin(cx) {
                Poll::Ready(Some(Ok(buffer))) => {
                    messages_received = i;

                    //   deserialize or log decode_error
                    match DnsResponse::from_buffer(buffer.into_parts().0) {
                        Ok(response) => match self.active_requests.entry(response.id()) {
                            Entry::Occupied(mut request_entry) => {
                                // send the response, complete the request...
                                let active_request = request_entry.get_mut();
                                if let Some(verifier) = &mut active_request.verifier {
                                    ignore_send(
                                        active_request
                                            .completion
                                            .try_send(verifier(response.as_buffer())),
                                    );
                                } else {
                                    ignore_send(active_request.completion.try_send(Ok(response)));
                                }
                            }
                            Entry::Vacant(..) => debug!("unexpected request_id: {}", response.id()),
                        },
                        // TODO: return src address for diagnostics
                        Err(error) => debug!(error = error.as_dyn(), "error decoding message"),
                    }
                }
                Poll::Ready(err) => {
                    let err = match err {
                        Some(Err(e)) => e,
                        None => ProtoError::from("stream closed"),
                        _ => unreachable!(),
                    };

                    self.stream_closed_close_all(err);
                    self.is_shutdown = true;
                    return Poll::Ready(None);
                }
                Poll::Pending => break,
            }
        }

        // If still active, then if the qos (for _ in 0..100 loop) limit
        // was hit then "yield". This'll make sure that the future is
        // woken up immediately on the next turn of the event loop.
        if messages_received == QOS_MAX_RECEIVE_MSGS {
            // FIXME: this was a task::current().notify(); is this right?
            cx.waker().wake_by_ref();
        }

        // Finally, return not ready to keep the 'driver task' alive.
        Poll::Pending
    }
}
