//! Small in-kernel client for the public Draw3D TCP service.
//!
//! Keeping this client on loopback is intentional: kernel callers exercise the
//! same framing, validation, ownership, and lifecycle boundary as external
//! clients instead of reaching into `draw3d_service` state directly.

use embassy_time::{Duration, Instant, Timer};
use trueos_draw3d::{Command, FrameDecoder, Response, encode_command};
use v::vnet::{Command as NetCommand, EndpointV4, Event, NetHandle, SocketKind};

use crate::r::net::VNet;

const LOOPBACK: [u8; 4] = [127, 0, 0, 1];
const CONNECT_TIMEOUT_MS: u64 = 2_000;
const REQUEST_TIMEOUT_MS: u64 = 5_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Draw3dClientError {
    NetUnavailable,
    ConnectSubmit,
    ConnectTimeout,
    ConnectionClosed,
    Send,
    Encode,
    Decode,
    RequestTimeout,
    ResponseMismatch,
}

impl Draw3dClientError {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::NetUnavailable => "net-unavailable",
            Self::ConnectSubmit => "connect-submit",
            Self::ConnectTimeout => "connect-timeout",
            Self::ConnectionClosed => "connection-closed",
            Self::Send => "send",
            Self::Encode => "encode",
            Self::Decode => "decode",
            Self::RequestTimeout => "request-timeout",
            Self::ResponseMismatch => "response-mismatch",
        }
    }
}

pub(crate) struct Draw3dTcpClient {
    net: VNet,
    handle: Option<NetHandle>,
    decoder: FrameDecoder,
    next_request_id: u32,
}

impl Draw3dTcpClient {
    pub(crate) async fn connect_loopback() -> Result<Self, Draw3dClientError> {
        let net = VNet::open_primary().ok_or(Draw3dClientError::NetUnavailable)?;
        let mut client = Self {
            net,
            handle: None,
            decoder: FrameDecoder::new(),
            next_request_id: 1,
        };
        client.ensure_connected().await?;
        Ok(client)
    }

    async fn ensure_connected(&mut self) -> Result<NetHandle, Draw3dClientError> {
        if let Some(handle) = self.handle {
            return Ok(handle);
        }
        self.net
            .submit(NetCommand::OpenTcpConnect {
                remote: EndpointV4::new(LOOPBACK, crate::r::draw3d_service::TCP_PORT),
            })
            .map_err(|_| Draw3dClientError::ConnectSubmit)?;

        let deadline = Instant::now() + Duration::from_millis(CONNECT_TIMEOUT_MS);
        let mut handle = None;
        loop {
            match self.net.pop_event() {
                Some(Event::Opened {
                    handle: opened,
                    kind: SocketKind::Tcp,
                }) => handle = Some(opened),
                Some(Event::TcpEstablished {
                    handle: established,
                    ..
                }) if handle == Some(established) => {
                    self.handle = Some(established);
                    self.decoder = FrameDecoder::new();
                    return Ok(established);
                }
                Some(Event::Closed { handle: closed }) if handle == Some(closed) => {
                    return Err(Draw3dClientError::ConnectionClosed);
                }
                Some(Event::Error { .. }) => {
                    if let Some(handle) = handle {
                        let _ = self.net.submit(NetCommand::Close { handle });
                    }
                    return Err(Draw3dClientError::ConnectionClosed);
                }
                _ => {}
            }
            if Instant::now() >= deadline {
                if let Some(handle) = handle {
                    let _ = self.net.submit(NetCommand::Close { handle });
                }
                return Err(Draw3dClientError::ConnectTimeout);
            }
            Timer::after(Duration::from_millis(1)).await;
        }
    }

    pub(crate) fn disconnect(&mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = self.net.submit(NetCommand::Close { handle });
        }
        self.decoder = FrameDecoder::new();
    }

    pub(crate) async fn request(
        &mut self,
        command: &Command,
    ) -> Result<Response, Draw3dClientError> {
        let handle = self.ensure_connected().await?;
        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
        let expected_opcode = command.opcode();
        let frame = encode_command(request_id, command).map_err(|_| Draw3dClientError::Encode)?;
        if self.net.send_tcp_all(handle, frame.as_slice()).is_err() {
            self.disconnect();
            return Err(Draw3dClientError::Send);
        }

        let deadline = Instant::now() + Duration::from_millis(REQUEST_TIMEOUT_MS);
        loop {
            match self.net.pop_event() {
                Some(Event::TcpData {
                    handle: event_handle,
                    data,
                }) if event_handle == handle => {
                    self.decoder.push(data.as_slice()).map_err(|_| {
                        self.disconnect();
                        Draw3dClientError::Decode
                    })?;
                    while let Some(decoded) = self.decoder.next_response().map_err(|_| {
                        self.disconnect();
                        Draw3dClientError::Decode
                    })? {
                        if decoded.request_id != request_id || decoded.opcode != expected_opcode {
                            self.disconnect();
                            return Err(Draw3dClientError::ResponseMismatch);
                        }
                        let response = decoded.response.map_err(|_| {
                            self.disconnect();
                            Draw3dClientError::Decode
                        })?;
                        return Ok(response);
                    }
                }
                Some(Event::Closed {
                    handle: event_handle,
                }) if event_handle == handle => {
                    self.disconnect();
                    return Err(Draw3dClientError::ConnectionClosed);
                }
                Some(Event::Error { .. }) => {
                    self.disconnect();
                    return Err(Draw3dClientError::ConnectionClosed);
                }
                _ => {}
            }
            if Instant::now() >= deadline {
                self.disconnect();
                return Err(Draw3dClientError::RequestTimeout);
            }
            Timer::after(Duration::from_millis(1)).await;
        }
    }
}

impl Drop for Draw3dTcpClient {
    fn drop(&mut self) {
        self.disconnect();
    }
}
