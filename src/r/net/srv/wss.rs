#![allow(dead_code)]

extern crate alloc;

use alloc::{boxed::Box, format, string::String, vec::Vec};
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_time::{Duration, Timer};
use embedded_websocket::{
    WebSocketClient, WebSocketOptions, WebSocketReceiveMessageType, WebSocketSendMessageType,
};
use rand_chacha::ChaCha20Rng;
use rand_core::SeedableRng;

use v::vnet;

use crate::net::tls::{TlsClientConfig, TlsRoots};
use crate::net::tls_socket::{TlsCommand, TlsEvent, register_tls_app_queues};
use crate::r::net::{NetProfile, Queue};
use crate::time::unix_time_seconds;

static WSS_SEQ: AtomicU32 = AtomicU32::new(1);
const RX_BUF_SIZE: usize = 4096;

pub struct WssConnection {
    cmds: &'static Queue<TlsCommand>,
    events: &'static Queue<TlsEvent>,
    handle: Option<vnet::NetHandle>,
    client: WebSocketClient<ChaCha20Rng>,
    connected: bool,
    closed: bool,
    rx_buf: Vec<u8>,
}

#[derive(Debug)]
pub enum WssError {
    ConnectFailed,
    InvalidUrl,
    DnsFailed,
    TlsFailed,
    Io,
    Protocol,
    Closed,
}

impl WssConnection {
    pub async fn connect(url: &str) -> Result<Self, WssError> {
        Self::connect_with_profile(url, NetProfile::default()).await
    }

    pub async fn connect_with_profile(url: &str, profile: NetProfile) -> Result<Self, WssError> {
        Self::connect_inner(url, profile, None).await
    }

    pub async fn connect_with_headers<'a>(
        url: &str,
        additional_headers: &'a [&'a str],
    ) -> Result<Self, WssError> {
        Self::connect_inner(url, NetProfile::default(), Some(additional_headers)).await
    }

    async fn connect_inner<'a>(
        url: &str,
        profile: NetProfile,
        additional_headers: Option<&'a [&'a str]>,
    ) -> Result<Self, WssError> {
        let (host, port, path) = parse_wss_url(url).ok_or(WssError::InvalidUrl)?;

        let dev_idx = profile
            .resolve_device_index()
            .ok_or(WssError::ConnectFailed)?;
        let api_ip = match crate::r::net::dns::resolve_ipv4_for_device(
            dev_idx,
            &host,
            crate::r::net::dns::DnsConfig::for_profile(profile),
        )
        .await
        {
            Ok(ip) => ip,
            Err(_) => return Err(WssError::DnsFailed),
        };

        let seq = WSS_SEQ.fetch_add(1, Ordering::Relaxed) as u64;
        let selector = if let Some((bus, slot, func)) = crate::net::bdf_at(dev_idx) {
            format!("{:02x}:{:02x}.{}", bus, slot, func)
        } else if let Some((vid, pid)) = crate::net::pci_id_at(dev_idx) {
            format!("{:04x}:{:04x}", vid, pid)
        } else {
            format!("{}", dev_idx)
        };
        let owner = Box::leak(format!("wss-{}@{}", seq, selector).into_boxed_str());
        let cmds_name = Box::leak(format!("{}-wss-cmd", owner).into_boxed_str());
        let evts_name = Box::leak(format!("{}-wss-evt", owner).into_boxed_str());

        let cmds = Queue::new_leaked(cmds_name, 256);
        let events = Queue::new_leaked(evts_name, 1024);
        register_tls_app_queues(owner, cmds, events);

        let roots = TlsRoots::mozilla();
        let cfg = TlsClientConfig::new().with_alpn_protocols(&[b"http/1.1"]);

        let mut seed = [0u8; 32];
        let t = unix_time_seconds().unwrap_or(0);
        seed[0..8].copy_from_slice(&t.to_le_bytes());
        seed[8..16].copy_from_slice(&seq.to_le_bytes());
        let rng = ChaCha20Rng::from_seed(seed);

        let mut client = WebSocketClient::new_client(rng);
        let origin = alloc::format!("https://{}", host);
        let options = WebSocketOptions {
            path: path.as_str(),
            host: host.as_str(),
            origin: origin.as_str(),
            sub_protocols: None,
            additional_headers,
        };

        let mut frame_buf = [0u8; RX_BUF_SIZE];
        let (len, ws_key) = match client.client_connect(&options, &mut frame_buf) {
            Ok(res) => res,
            Err(_) => return Err(WssError::Protocol),
        };
        let handshake_buffer = Vec::from(&frame_buf[..len]);

        cmds.push(TlsCommand::OpenTcpConnect {
            remote: vnet::EndpointV4::new(api_ip, port),
            server_name: Box::leak(host.clone().into_boxed_str()),
            cfg,
            roots,
            timeouts: crate::net::tls_socket::TlsTimeouts {
                connect_ms: 15_000,
                tls_ms: 15_000,
                idle_ms: 60_000,
            },
        })
        .map_err(|_| WssError::ConnectFailed)?;

        let mut handle = None;

        let mut established = false;

        // Polling loop for connect
        let start = crate::time::uptime_seconds();
        let deadline = start + 15;

        // TLS Connect Loop
        loop {
            // Drain events
            let mut got_event = false;
            while let Some(ev) = events.pop() {
                got_event = true;
                match ev {
                    TlsEvent::Opened { handle: h } => {
                        handle = Some(h);
                    }
                    TlsEvent::Connected { handle: h } => {
                        if Some(h) == handle {
                            established = true;
                            // TLS established, send WS handshake
                            cmds.push(TlsCommand::Send {
                                handle: h,
                                data: handshake_buffer.clone(),
                            })
                            .map_err(|_| WssError::Io)?;
                            break;
                        }
                    }
                    TlsEvent::Error { .. }
                    | TlsEvent::TlsError { .. }
                    | TlsEvent::Closed { .. } => {
                        return Err(WssError::TlsFailed);
                    }
                    _ => {}
                }
            }
            if established {
                break;
            }
            if crate::time::uptime_seconds() > deadline {
                return Err(WssError::ConnectFailed);
            }
            if !got_event {
                Timer::after(Duration::from_micros(100)).await;
            }
        }

        // Silence warning for unused variable  if loop breaks early
        let _ = established;

        // Wait for handshake response; must call `client_accept` to transition to Open.
        let mut handshake_response = Vec::new();
        loop {
            let mut got_event = false;
            while let Some(ev) = events.pop() {
                got_event = true;
                match ev {
                    TlsEvent::Data { handle: h, data } => {
                        if Some(h) == handle {
                            handshake_response.extend_from_slice(&data);

                            match client.client_accept(&ws_key, handshake_response.as_slice()) {
                                Ok((consumed, _subproto)) => {
                                    let extra = handshake_response.split_off(consumed);
                                    // Handshake complete.
                                    return Ok(Self {
                                        cmds,
                                        events,
                                        handle,
                                        client,
                                        connected: true,
                                        closed: false,
                                        rx_buf: extra,
                                    });
                                }
                                Err(embedded_websocket::Error::HttpHeaderIncomplete) => {}
                                Err(e) => {
                                    if let Ok(s) =
                                        core::str::from_utf8(handshake_response.as_slice())
                                    {
                                        if let Some(line_end) = s.find("\r\n") {
                                            let line = &s[..line_end];
                                            crate::log!(
                                                "wss: handshake failed url={} status-line='{}' err={:?}\n",
                                                url,
                                                line,
                                                e
                                            );
                                        } else {
                                            crate::log!(
                                                "wss: handshake failed url={} bytes={} err={:?}\n",
                                                url,
                                                handshake_response.len(),
                                                e
                                            );
                                        }
                                    } else {
                                        crate::log!(
                                            "wss: handshake failed url={} bytes={} err={:?}\n",
                                            url,
                                            handshake_response.len(),
                                            e
                                        );
                                    }
                                    return Err(WssError::Protocol);
                                }
                            }
                        }
                    }
                    TlsEvent::Closed { .. } => return Err(WssError::Closed),
                    _ => {}
                }
            }

            if crate::time::uptime_seconds() > deadline {
                return Err(WssError::ConnectFailed);
            }
            if !got_event {
                Timer::after(Duration::from_micros(100)).await;
            }
        }
    }

    pub fn send(&mut self, text: &str) -> Result<(), WssError> {
        if self.closed || !self.connected {
            return Err(WssError::Closed);
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
            .map_err(|_| WssError::Protocol)?;

        if let Some(h) = self.handle {
            self.cmds
                .push(TlsCommand::Send {
                    handle: h,
                    data: Vec::from(&buf[..len]),
                })
                .map_err(|_| WssError::Io)?;
        }
        Ok(())
    }

    pub fn recv(&mut self) -> Option<String> {
        // Drain TLS events
        while let Some(ev) = self.events.pop() {
            match ev {
                TlsEvent::Data { handle, data } => {
                    if Some(handle) == self.handle {
                        self.rx_buf.extend_from_slice(&data);
                    }
                }
                TlsEvent::Closed { handle } => {
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
                        ) && let Some(h) = self.handle
                        {
                            let _ = self.cmds.push(TlsCommand::Send {
                                handle: h,
                                data: Vec::from(&buf[..len]),
                            });
                        }
                        None
                    }
                    WebSocketReceiveMessageType::Pong => None,
                    WebSocketReceiveMessageType::CloseMustReply => {
                        let buf = [0u8; RX_BUF_SIZE];
                        let mut payload = Vec::from(&out_buf[..read_result.len_to]);
                        if let Ok(len) = self.client.write(
                            WebSocketSendMessageType::CloseReply,
                            true,
                            &buf,
                            &mut payload,
                        ) && let Some(h) = self.handle
                        {
                            let _ = self.cmds.push(TlsCommand::Send {
                                handle: h,
                                data: Vec::from(&buf[..len]),
                            });
                        }
                        None
                    }
                    WebSocketReceiveMessageType::CloseCompleted => {
                        self.closed = true;
                        None
                    }
                }
            }
            // Error handling matching ws.rs
            Err(_) => {
                // Ignore for now
                None
            }
        }
    }
}

fn parse_wss_url(url: &str) -> Option<(String, u16, String)> {
    let url = url.strip_prefix("wss://")?;
    let (authority, path) = match url.split_once('/') {
        Some((a, p)) => (a, format!("/{}", p)),
        None => (url, String::from("/")),
    };
    let (host, port) = if let Some((h, p)) = authority.rsplit_once(':') {
        (String::from(h), p.parse::<u16>().ok()?)
    } else {
        (String::from(authority), 443)
    };
    Some((host, port, path))
}
