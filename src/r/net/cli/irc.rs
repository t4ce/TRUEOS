#![allow(dead_code)]

extern crate alloc;

use alloc::{format, string::String, vec::Vec};

use embassy_time::{Duration, Instant, Timer};
use v::vnet::{Command, EndpointV4, Event, NetHandle, SocketKind};

use crate::net::tls::{KernelTlsRng, TlsClient, TlsClientConfig, TlsRoots, TlsTime};
use crate::r::net::dns::{self, DnsConfig};
use crate::r::net::{NetProfile, VNet};

pub const IRC_DEFAULT_PORT: u16 = crate::allports::well_known::IRC;
pub const IRC_TLS_PORT: u16 = crate::allports::well_known::IRC_TLS;

#[derive(Debug)]
pub enum IrcError {
    DnsFailed,
    ConnectFailed,
    Timeout,
    Io,
    Closed,
    NotRegistered,
    TlsFailed,
}

struct KernelTime;

impl TlsTime for KernelTime {
    fn unix_time_seconds(&self) -> Option<u64> {
        crate::time::unix_time_seconds()
    }
}

static KERNEL_TIME: KernelTime = KernelTime;

pub struct IrcSession {
    net: VNet,
    handle: NetHandle,
    rx: Vec<u8>,
    closed: bool,
    tls: Option<TlsClient>,
}

/// Parsed IRC message: `[:prefix] command [params...] [:trailing]`
pub struct IrcMessage {
    /// Optional prefix (without leading `:`), e.g. `"nick!user@host"`.
    pub prefix: Option<String>,
    /// Command word or 3-digit numeric string, e.g. `"PRIVMSG"` or `"001"`.
    pub command: String,
    /// Middle parameters (before the trailing `:`).
    pub params: Vec<String>,
    /// Trailing parameter (after ` :`), if present.
    pub trailing: Option<String>,
}

impl IrcMessage {
    /// Parse a single raw IRC line (without the CRLF terminator).
    pub fn parse(line: &str) -> Option<Self> {
        let mut rest = line;

        let prefix = if rest.starts_with(':') {
            let end = rest.find(' ')?;
            let p = String::from(&rest[1..end]);
            rest = rest[end..].trim_start_matches(' ');
            Some(p)
        } else {
            None
        };

        let trailing = if let Some(idx) = crate::r::pat::find_str(rest, " :") {
            let t = String::from(&rest[idx + 2..]);
            rest = &rest[..idx];
            Some(t)
        } else {
            None
        };

        let mut words = rest.split_whitespace();
        let command = String::from(words.next()?);
        let params: Vec<String> = words.map(String::from).collect();

        Some(IrcMessage {
            prefix,
            command,
            params,
            trailing,
        })
    }
}

impl IrcSession {
    /// Plain TCP connect (port 6667).
    pub async fn connect(
        host: &str,
        port: u16,
        profile: NetProfile,
        timeout_ms: u32,
    ) -> Result<Self, IrcError> {
        Self::connect_inner(host, port, profile, timeout_ms, false).await
    }

    /// Direct TLS connect (port 6697). TLS handshake begins immediately after TCP establishment.
    pub async fn connect_tls(
        host: &str,
        port: u16,
        profile: NetProfile,
        timeout_ms: u32,
    ) -> Result<Self, IrcError> {
        Self::connect_inner(host, port, profile, timeout_ms, true).await
    }

    async fn connect_inner(
        host: &str,
        port: u16,
        profile: NetProfile,
        timeout_ms: u32,
        use_tls: bool,
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
        let rx = Vec::new();

        loop {
            match net.pop_event() {
                Some(Event::Opened { handle: h, kind }) if kind == SocketKind::Tcp => {
                    handle = Some(h);
                }
                Some(Event::TcpEstablished { handle: h, .. }) if Some(h) == handle => break,
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

        let handle = handle.ok_or(IrcError::ConnectFailed)?;
        let mut session = Self {
            net,
            handle,
            rx,
            closed: false,
            tls: None,
        };

        if use_tls {
            session.do_tls_handshake(host, timeout_ms).await?;
        }

        Ok(session)
    }

    async fn do_tls_handshake(&mut self, host: &str, timeout_ms: u32) -> Result<(), IrcError> {
        let cfg = TlsClientConfig::new();
        let roots = TlsRoots::mozilla();
        let mut rng = KernelTlsRng::new();
        let mut tls = TlsClient::new(&cfg, &roots, host, &mut rng, &KERNEL_TIME)
            .map_err(|_| IrcError::TlsFailed)?;

        self.flush_tls_ciphertext(&mut tls)?;

        let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
        while !tls.is_connected() {
            match self.net.pop_event() {
                Some(Event::TcpData { handle, data }) if handle == self.handle => {
                    let plaintext = tls
                        .ingest_encrypted(data.as_slice())
                        .map_err(|_| IrcError::TlsFailed)?;
                    if !plaintext.is_empty() {
                        self.rx.extend_from_slice(&plaintext);
                    }
                    self.flush_tls_ciphertext(&mut tls)?;
                }
                Some(Event::Closed { handle }) if handle == self.handle => {
                    self.closed = true;
                    return Err(IrcError::Closed);
                }
                Some(Event::Error { .. }) => return Err(IrcError::Io),
                _ => {}
            }
            if Instant::now() >= deadline {
                return Err(IrcError::Timeout);
            }
            Timer::after(Duration::from_millis(1)).await;
        }

        self.tls = Some(tls);
        Ok(())
    }

    fn send_raw(&mut self, line: &str) -> Result<(), IrcError> {
        if self.closed {
            return Err(IrcError::Closed);
        }
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(line.as_bytes());
        buf.extend_from_slice(b"\r\n");
        self.send_bytes(&buf)
    }

    fn send_bytes(&mut self, bytes: &[u8]) -> Result<(), IrcError> {
        if self.tls.is_none() {
            self.net
                .send_tcp_all(self.handle, bytes)
                .map_err(|_| IrcError::Io)?;
            return Ok(());
        }
        if let Some(mut tls) = self.tls.take() {
            tls.write_plaintext(bytes)
                .map_err(|_| IrcError::TlsFailed)?;
            self.flush_tls_ciphertext(&mut tls)?;
            self.tls = Some(tls);
        }
        Ok(())
    }

    fn flush_tls_ciphertext(&mut self, tls: &mut TlsClient) -> Result<(), IrcError> {
        let ciphertext = tls
            .take_ciphertext_to_send()
            .map_err(|_| IrcError::TlsFailed)?;
        if ciphertext.is_empty() {
            return Ok(());
        }
        self.net
            .send_tcp_all(self.handle, &ciphertext)
            .map_err(|_| IrcError::Io)
    }

    fn drain_events(&mut self) -> bool {
        for _ in 0..32 {
            match self.net.pop_event() {
                Some(Event::TcpData { handle, data }) if handle == self.handle => {
                    if self.tls.is_none() {
                        self.rx.extend_from_slice(data.as_slice());
                    } else if let Some(mut tls) = self.tls.take() {
                        if let Ok(plaintext) = tls.ingest_encrypted(data.as_slice()) {
                            if !plaintext.is_empty() {
                                self.rx.extend_from_slice(&plaintext);
                            }
                        }
                        let _ = self.flush_tls_ciphertext(&mut tls);
                        self.tls = Some(tls);
                    }
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

    /// Send `PASS <password>` — call before `register()` when the server requires a connection password.
    pub fn pass(&mut self, password: &str) -> Result<(), IrcError> {
        self.send_raw(&format!("PASS {}", password))
    }

    /// Send NICK + USER and wait for numeric 001 (welcome).
    /// On 433 ERR_NICKNAMEINUSE, appends `_` to the nick and retries up to 3 times.
    pub async fn register(
        &mut self,
        nick: &str,
        username: &str,
        realname: &str,
        timeout_ms: u32,
    ) -> Result<(), IrcError> {
        let mut current_nick = String::from(nick);
        let mut attempts = 0u8;

        'outer: loop {
            self.send_raw(&format!("NICK {}", current_nick))?;
            self.send_raw(&format!("USER {} 0 * :{}", username, realname))?;

            let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
            loop {
                if let Some(line) = self.recv_line(50).await? {
                    match parse_numeric(&line) {
                        Some(1) => return Ok(()),
                        Some(433) => {
                            attempts += 1;
                            if attempts >= 3 {
                                return Err(IrcError::NotRegistered);
                            }
                            current_nick.push('_');
                            continue 'outer;
                        }
                        Some(n) if n >= 400 && n < 600 => return Err(IrcError::NotRegistered),
                        _ => {}
                    }
                }
                if Instant::now() >= deadline {
                    return Err(IrcError::Timeout);
                }
            }
        }
    }

    /// Send `JOIN #channel` without waiting for confirmation.
    pub fn join(&mut self, channel: &str) -> Result<(), IrcError> {
        self.send_raw(&format!("JOIN {}", channel))
    }

    /// Send `JOIN #channel` and wait for end-of-names (366) confirming the join succeeded.
    pub async fn join_wait(&mut self, channel: &str, timeout_ms: u32) -> Result<(), IrcError> {
        self.send_raw(&format!("JOIN {}", channel))?;
        self.wait_for_join(channel, timeout_ms).await
    }

    /// Wait for end-of-names (366) on the given channel after a JOIN has been sent.
    pub async fn wait_for_join(&mut self, channel: &str, timeout_ms: u32) -> Result<(), IrcError> {
        let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
        let chan_upper = channel.to_ascii_uppercase();
        loop {
            if let Some(line) = self.recv_line(50).await? {
                if let Some(n) = parse_numeric(&line) {
                    match n {
                        366 => {
                            if line.to_ascii_uppercase().contains(chan_upper.as_str()) {
                                return Ok(());
                            }
                        }
                        // can't-join errors
                        403 | 405 | 471 | 473 | 474 | 475 => {
                            return Err(IrcError::NotRegistered);
                        }
                        _ => {}
                    }
                }
            }
            if Instant::now() >= deadline {
                return Err(IrcError::Timeout);
            }
        }
    }

    pub fn privmsg(&mut self, target: &str, text: &str) -> Result<(), IrcError> {
        self.send_raw(&format!("PRIVMSG {} :{}", target, text))
    }

    /// Leave a channel with an optional reason.
    pub fn part(&mut self, channel: &str, reason: &str) -> Result<(), IrcError> {
        self.send_raw(&format!("PART {} :{}", channel, reason))
    }

    /// Change nick mid-session.
    pub fn nick(&mut self, new_nick: &str) -> Result<(), IrcError> {
        self.send_raw(&format!("NICK {}", new_nick))
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
