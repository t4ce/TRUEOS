extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_time::{Duration, Timer};
use embedded_websocket::{
    WebSocketClient, WebSocketOptions, WebSocketReceiveMessageType, WebSocketSendMessageType,
};
use rand_chacha::ChaCha20Rng;
use rand_core::SeedableRng;
use trueos_v::vnet::{ByteBuf, Command, EndpointV4, Event, NetHandle, SocketKind};

use crate::time::unix_time_seconds;
use crate::v::net::VNet;

static WS_SEQ: AtomicU32 = AtomicU32::new(1);
const RX_BUF_SIZE: usize = 4096;

pub struct WsConnection {
    net: VNet,
    handle: Option<NetHandle>,
    client: WebSocketClient<ChaCha20Rng>,
    connected: bool,
    closed: bool,
    rx_buf: Vec<u8>,
}

#[derive(Debug)]
pub enum WsError {
    ConnectFailed,
    InvalidUrl,
    DnsFailed,
    Io,
    Protocol,
    Closed,
}

impl WsConnection {
    pub async fn connect(url: &str) -> Result<Self, WsError> {
        let (host, port, path) = parse_ws_url(url).ok_or(WsError::InvalidUrl)?;

        let dev_idx = crate::net::primary_device_index();
        let api_ip = match super::dns::resolve_ipv4_for_device(
            dev_idx,
            &host,
            super::dns::DnsConfig::for_device(dev_idx),
        )
        .await
        {
            Ok(ip) => ip,
            Err(_) => return Err(WsError::DnsFailed),
        };

        let seq = WS_SEQ.fetch_add(1, Ordering::Relaxed) as u64;
        let net = VNet::open(dev_idx).ok_or(WsError::ConnectFailed)?;

        let mut seed = [0u8; 32];
        let t = unix_time_seconds().unwrap_or(0);
        seed[0..8].copy_from_slice(&t.to_le_bytes());
        seed[8..16].copy_from_slice(&seq.to_le_bytes());
        let rng = ChaCha20Rng::from_seed(seed);

        let mut client = WebSocketClient::new_client(rng);
        let origin = alloc::format!("http://{}", host);
        let options = WebSocketOptions {
            path: path.as_str(),
            host: host.as_str(),
            origin: origin.as_str(),
            sub_protocols: None,
            additional_headers: None,
        };

        let mut frame_buf = [0u8; RX_BUF_SIZE];
        let (len, ws_key) = match client.client_connect(&options, &mut frame_buf) {
            Ok(res) => res,
            Err(_) => return Err(WsError::Protocol),
        };
        let handshake_data = Vec::from(&frame_buf[..len]);

        net.submit(Command::OpenTcpConnect {
            remote: EndpointV4::new(api_ip, port),
        })
        .map_err(|_| WsError::ConnectFailed)?;

        let mut handle = None;

        let start = crate::time::uptime_seconds();
        let deadline = start + 10;

        loop {
            if let Some(ev) = net.pop_event() {
                match ev {
                    Event::Opened { handle: h, kind } => {
                        if kind == SocketKind::Tcp {
                            handle = Some(h);
                        }
                    }
                    Event::TcpEstablished { handle: h } => {
                        if Some(h) == handle {
                            net.submit(Command::SendTcp {
                                handle: h,
                                data: ByteBuf::from_slice_trunc(&handshake_data),
                            })
                            .map_err(|_| WsError::Io)?;
                            break;
                        }
                    }
                    Event::Error { msg: _ } => return Err(WsError::ConnectFailed),
                    Event::Closed { handle: h } => {
                        if Some(h) == handle {
                            return Err(WsError::ConnectFailed);
                        }
                    }
                    _ => {}
                }
            }
            if crate::time::uptime_seconds() > deadline {
                return Err(WsError::ConnectFailed);
            }
            Timer::after(Duration::from_micros(100)).await;
        }

        let mut rx_buf = Vec::new();
        loop {
            if let Some(ev) = net.pop_event() {
                match ev {
                    Event::TcpData { handle: h, data } => {
                        if Some(h) == handle {
                            rx_buf.extend_from_slice(data.as_slice());

                            match client.client_accept(&ws_key, rx_buf.as_slice()) {
                                Ok((consumed, _subproto)) => {
                                    // Keep any bytes beyond the HTTP header for WS frame parsing.
                                    let extra = rx_buf.split_off(consumed);
                                    rx_buf = extra;
                                    break;
                                }
                                Err(embedded_websocket::Error::HttpHeaderIncomplete) => {}
                                Err(e) => {
                                    // Most common failure in the wild: server returns 301/302 to https.
                                    if let Ok(s) = core::str::from_utf8(rx_buf.as_slice()) {
                                        if let Some(line_end) = s.find("\r\n") {
                                            let line = &s[..line_end];
                                            crate::log!(
                                                "ws: handshake failed url={} status-line='{}' err={:?}\n",
                                                url,
                                                line,
                                                e
                                            );
                                        } else {
                                            crate::log!(
                                                "ws: handshake failed url={} bytes={} err={:?}\n",
                                                url,
                                                rx_buf.len(),
                                                e
                                            );
                                        }
                                    } else {
                                        crate::log!(
                                            "ws: handshake failed url={} bytes={} err={:?}\n",
                                            url,
                                            rx_buf.len(),
                                            e
                                        );
                                    }
                                    return Err(WsError::Protocol);
                                }
                            }
                        }
                    }
                    Event::Closed { .. } => return Err(WsError::Closed),
                    _ => {}
                }
            }
            if crate::time::uptime_seconds() > deadline {
                return Err(WsError::ConnectFailed);
            }
            Timer::after(Duration::from_micros(100)).await;
        }

        Ok(Self {
            net,
            handle,
            client,
            connected: true,
            closed: false,
            rx_buf,
        })
    }

    pub fn send(&mut self, text: &str) -> Result<(), WsError> {
        if self.closed || !self.connected {
            return Err(WsError::Closed);
        }
        let mut buf = [0u8; RX_BUF_SIZE];

        let len = self
            .client
            .write(
                WebSocketSendMessageType::Text,
                true, // fin
                text.as_bytes(),
                &mut buf,
            )
            .map_err(|_| WsError::Protocol)?;

        if let Some(h) = self.handle {
            self.net
                .submit(Command::SendTcp {
                    handle: h,
                    data: ByteBuf::from_slice_trunc(&buf[..len]),
                })
                .map_err(|_| WsError::Io)?;
        }
        Ok(())
    }

    pub fn recv(&mut self) -> Option<String> {
        while let Some(ev) = self.net.pop_event() {
            match ev {
                Event::TcpData { handle, data } => {
                    if Some(handle) == self.handle {
                        self.rx_buf.extend_from_slice(data.as_slice());
                    }
                }
                Event::Closed { handle } => {
                    if Some(handle) == self.handle {
                        self.closed = true;
                    }
                }
                _ => {}
            }
        }

        if self.rx_buf.is_empty() {
            return None;
        }

        let mut out_buf = [0u8; RX_BUF_SIZE];
        match self.client.read(&self.rx_buf, &mut out_buf) {
            Ok(read_result) => {
                let consumed = read_result.len_from;
                self.rx_buf.drain(0..consumed);

                match read_result.message_type {
                    WebSocketReceiveMessageType::Text => {
                        let s = core::str::from_utf8(&out_buf[..read_result.len_to]).ok()?;
                        Some(String::from(s))
                    }
                    WebSocketReceiveMessageType::Binary => None,
                    WebSocketReceiveMessageType::Ping => {
                        // RFC6455: reply to Ping with Pong including identical payload.
                        let buf = [0u8; RX_BUF_SIZE];
                        let mut payload = Vec::from(&out_buf[..read_result.len_to]);
                        if let Ok(len) = self.client.write(
                            WebSocketSendMessageType::Pong,
                            true,
                            &buf,
                            &mut payload,
                        )
                            && let Some(h) = self.handle {
                                let _ = self.net.submit(Command::SendTcp {
                                    handle: h,
                                    data: ByteBuf::from_slice_trunc(&buf[..len]),
                                });
                            }
                        None
                    }
                    WebSocketReceiveMessageType::Pong => None,
                    WebSocketReceiveMessageType::CloseMustReply => {
                        // Reply to Close with CloseReply (same payload).
                        let buf = [0u8; RX_BUF_SIZE];
                        let mut payload = Vec::from(&out_buf[..read_result.len_to]);
                        if let Ok(len) = self.client.write(
                            WebSocketSendMessageType::CloseReply,
                            true,
                            &buf,
                            &mut payload,
                        )
                            && let Some(h) = self.handle {
                                let _ = self.net.submit(Command::SendTcp {
                                    handle: h,
                                    data: ByteBuf::from_slice_trunc(&buf[..len]),
                                });
                            }
                        None
                    }
                    WebSocketReceiveMessageType::CloseCompleted => {
                        self.closed = true;
                        None
                    }
                    _ => None,
                }
            }
            Err(_) => {
                // Ignore incomplete or others for now unless fatal
                None
            }
        }
    }
}

fn parse_ws_url(url: &str) -> Option<(String, u16, String)> {
    let url = url.strip_prefix("ws://")?;
    let (authority, path) = match url.split_once('/') {
        Some((a, p)) => (a, format!("/{}", p)),
        None => (url, String::from("/")),
    };
    let (host, port) = if let Some((h, p)) = authority.rsplit_once(':') {
        (String::from(h), p.parse::<u16>().ok()?)
    } else {
        (String::from(authority), 80)
    };
    Some((host, port, path))
}
