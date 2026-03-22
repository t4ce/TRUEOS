extern crate alloc;

use alloc::{format, string::String, vec::Vec};

use embassy_time::{Duration, Instant, Timer};
use v::vnet::{ByteBuf, Command, EndpointV4, Event, NetHandle, SocketKind};

use super::dns::{self, DnsConfig};
use crate::r::net::{NetProfile, VNet};

pub const IRC_DEFAULT_PORT: u16 = 6667;

#[derive(Debug)]
pub enum IrcError {
    DnsFailed,
    ConnectFailed,
    Timeout,
    Io,
    Closed,
    NotRegistered,
}

pub struct IrcSession {
    net: VNet,
    handle: NetHandle,
    rx: Vec<u8>,
    closed: bool,
}

impl IrcSession {
    pub async fn connect(
        host: &str,
        port: u16,
        profile: NetProfile,
        timeout_ms: u32,
    ) -> Result<Self, IrcError> {
        let dev_idx = profile
            .resolve_device_index()
            .ok_or(IrcError::ConnectFailed)?;
        let ip = dns::resolve_ipv4_for_device(dev_idx, host, DnsConfig::for_profile(profile))
            .await
            .map_err(|_| IrcError::DnsFailed)?;

        let net = VNet::open_with_profile(profile).ok_or(IrcError::ConnectFailed)?;
        net.submit(Command::OpenTcpConnect {
            remote: EndpointV4::new(ip, port),
        })
        .map_err(|_| IrcError::ConnectFailed)?;

        let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
        let mut handle: Option<NetHandle> = None;
        let mut rx = Vec::new();

        loop {
            match net.pop_event() {
                Some(Event::Opened { handle: h, kind }) if kind == SocketKind::Tcp => {
                    handle = Some(h);
                }
                Some(Event::TcpEstablished { handle: h }) if Some(h) == handle => break,
                Some(Event::TcpData { handle: h, data }) if Some(h) == handle => {
                    rx.extend_from_slice(data.as_slice());
                }
                Some(Event::Closed { .. }) | Some(Event::Error { .. }) => {
                    return Err(IrcError::ConnectFailed);
                }
                _ => {}
            }
            if Instant::now() >= deadline {
                return Err(IrcError::Timeout);
            }
            Timer::after(Duration::from_millis(1)).await;
        }

        Ok(Self {
            net,
            handle: handle.ok_or(IrcError::ConnectFailed)?,
            rx,
            closed: false,
        })
    }

    fn send_raw(&self, line: &str) -> Result<(), IrcError> {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(line.as_bytes());
        buf.extend_from_slice(b"\r\n");
        self.net
            .submit(Command::SendTcp {
                handle: self.handle,
                data: ByteBuf::from_slice_trunc(&buf),
            })
            .map_err(|_| IrcError::Io)
    }

    fn drain_events(&mut self) -> bool {
        for _ in 0..32 {
            match self.net.pop_event() {
                Some(Event::TcpData { handle, data }) if handle == self.handle => {
                    self.rx.extend_from_slice(data.as_slice());
                }
                Some(Event::Closed { handle }) if handle == self.handle => {
                    self.closed = true;
                    return true;
                }
                Some(Event::Error { .. }) => {
                    self.closed = true;
                    return true;
                }
                _ => {}
            }
        }
        false
    }

    fn pop_line(&mut self) -> Option<String> {
        let pos = self.rx.windows(2).position(|w| w == b"\r\n")?;
        let bytes = self.rx[..pos].to_vec();
        self.rx.drain(..pos + 2);
        String::from_utf8(bytes).ok()
    }

    /// Read the next IRC line, answering PING transparently. Returns `None` on timeout.
    pub async fn recv_line(&mut self, timeout_ms: u32) -> Result<Option<String>, IrcError> {
        let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
        loop {
            if self.drain_events() {
                return Err(IrcError::Closed);
            }
            if let Some(line) = self.pop_line() {
                if line.starts_with("PING ") {
                    let _ = self.send_raw(&format!("PONG {}", &line[5..]));
                    continue;
                }
                return Ok(Some(line));
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            Timer::after(Duration::from_millis(5)).await;
        }
    }

    /// Send NICK + USER and wait for numeric 001 (welcome).
    pub async fn register(
        &mut self,
        nick: &str,
        username: &str,
        realname: &str,
        timeout_ms: u32,
    ) -> Result<(), IrcError> {
        self.send_raw(&format!("NICK {}", nick))?;
        self.send_raw(&format!("USER {} 0 * :{}", username, realname))?;

        let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
        loop {
            if let Some(line) = self.recv_line(50).await? {
                match parse_numeric(&line) {
                    Some(1) => return Ok(()),
                    Some(n) if n >= 400 && n < 600 => return Err(IrcError::NotRegistered),
                    _ => {}
                }
            }
            if Instant::now() >= deadline {
                return Err(IrcError::Timeout);
            }
        }
    }

    pub fn join(&self, channel: &str) -> Result<(), IrcError> {
        self.send_raw(&format!("JOIN {}", channel))
    }

    pub fn privmsg(&self, target: &str, text: &str) -> Result<(), IrcError> {
        self.send_raw(&format!("PRIVMSG {} :{}", target, text))
    }

    pub async fn quit(&mut self, msg: &str, timeout_ms: u32) -> Result<(), IrcError> {
        if self.closed {
            return Ok(());
        }
        let _ = self.send_raw(&format!("QUIT :{}", msg));
        let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
        while Instant::now() < deadline {
            if self.drain_events() {
                break;
            }
            Timer::after(Duration::from_millis(5)).await;
        }
        let _ = self.net.submit(Command::Close {
            handle: self.handle,
        });
        self.closed = true;
        Ok(())
    }
}

/// Extract the 3-digit IRC numeric from `:server NNN nick :text`
fn parse_numeric(line: &str) -> Option<u16> {
    let mut parts = line.splitn(3, ' ');
    let _prefix = parts.next()?;
    let cmd = parts.next()?;
    if cmd.len() == 3 && cmd.bytes().all(|b| b.is_ascii_digit()) {
        cmd.parse::<u16>().ok()
    } else {
        None
    }
}
