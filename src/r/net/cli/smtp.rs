#![allow(dead_code)]

extern crate alloc;

use alloc::{format, string::String, vec::Vec};

use embassy_time::{Duration, Instant, Timer};
use v::vnet::{self, ByteBuf, Command, EndpointV4, Event, NetHandle, SocketKind};

use crate::r::t::net::dns::{self, DnsConfig};
use crate::net::tls::{KernelTlsRng, TlsClient, TlsClientConfig, TlsRoots, TlsTime};
use crate::r::net::{NetProfile, VNet};

pub const SMTP_HOST: &str = "smtp.mail.com";
pub const SMTP_PORT: u16 = crate::allports::well_known::SMTP_SUBMISSION;
pub const SMTP_EHLO_DOMAIN: &str = "trueos.local";

#[derive(Debug)]
pub enum SmtpError {
    DnsFailed,
    ConnectFailed,
    Timeout,
    Io,
    Closed,
    Protocol,
    TlsFailed,
    AuthFailed,
    ReplyError(u16),
}

#[derive(Clone, Debug)]
pub struct SmtpReply {
    pub code: u16,
    pub lines: Vec<String>,
}

struct KernelTime;

impl TlsTime for KernelTime {
    fn unix_time_seconds(&self) -> Option<u64> {
        crate::time::unix_time_seconds()
    }
}

static KERNEL_TIME: KernelTime = KernelTime;

pub struct SmtpClient {
    net: VNet,
    handle: NetHandle,
    rx: Vec<u8>,
    closed: bool,
    tls: Option<TlsClient>,
}

impl SmtpClient {
    pub async fn connect(timeout_ms: u32) -> Result<Self, SmtpError> {
        Self::connect_with_profile(NetProfile::default(), timeout_ms).await
    }

    pub async fn connect_with_profile(
        profile: NetProfile,
        timeout_ms: u32,
    ) -> Result<Self, SmtpError> {
        let dev_idx = profile
            .resolve_device_index()
            .ok_or(SmtpError::ConnectFailed)?;
        let ip = dns::resolve_ipv4_for_device(dev_idx, SMTP_HOST, DnsConfig::for_profile(profile))
            .await
            .map_err(|_| SmtpError::DnsFailed)?;

        let net = VNet::open_with_profile(profile).ok_or(SmtpError::ConnectFailed)?;
        net.submit(Command::OpenTcpConnect {
            remote: EndpointV4::new(ip, SMTP_PORT),
        })
        .map_err(|_| SmtpError::ConnectFailed)?;

        let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
        let mut handle: Option<NetHandle> = None;
        let mut rx = Vec::new();

        loop {
            match net.pop_event() {
                Some(Event::Opened { handle: h, kind }) if kind == SocketKind::Tcp => {
                    handle = Some(h);
                }
                Some(Event::TcpEstablished { handle: h }) if Some(h) == handle => {
                    break;
                }
                Some(Event::TcpData { handle: h, data }) if Some(h) == handle => {
                    rx.extend_from_slice(data.as_slice());
                }
                Some(Event::Closed { handle: h }) if Some(h) == handle => {
                    return Err(SmtpError::ConnectFailed);
                }
                Some(Event::Error { .. }) => return Err(SmtpError::ConnectFailed),
                _ => {}
            }

            if Instant::now() >= deadline {
                return Err(SmtpError::Timeout);
            }
            Timer::after(Duration::from_millis(1)).await;
        }

        let mut client = Self {
            net,
            handle: handle.ok_or(SmtpError::ConnectFailed)?,
            rx,
            closed: false,
            tls: None,
        };

        let banner = client.read_reply(timeout_ms).await?;
        if banner.code != 220 {
            return Err(SmtpError::Protocol);
        }

        client.ehlo(SMTP_EHLO_DOMAIN, timeout_ms).await?;
        client.starttls(timeout_ms).await?;
        client.ehlo(SMTP_EHLO_DOMAIN, timeout_ms).await?;

        Ok(client)
    }

    pub async fn ehlo(&mut self, domain: &str, timeout_ms: u32) -> Result<SmtpReply, SmtpError> {
        self.send_line(format!("EHLO {}", domain).as_str())?;
        let reply = self.read_reply(timeout_ms).await?;
        if reply.code == 250 {
            Ok(reply)
        } else {
            Err(SmtpError::ReplyError(reply.code))
        }
    }

    pub async fn auth_login(
        &mut self,
        username: &str,
        password: &str,
        timeout_ms: u32,
    ) -> Result<(), SmtpError> {
        self.send_line("AUTH LOGIN")?;
        let r1 = self.read_reply(timeout_ms).await?;
        if r1.code != 334 {
            return Err(SmtpError::AuthFailed);
        }

        self.send_line(base64_encode(username.as_bytes()).as_str())?;
        let r2 = self.read_reply(timeout_ms).await?;
        if r2.code != 334 {
            return Err(SmtpError::AuthFailed);
        }

        self.send_line(base64_encode(password.as_bytes()).as_str())?;
        let r3 = self.read_reply(timeout_ms).await?;
        if r3.code != 235 {
            return Err(SmtpError::AuthFailed);
        }

        Ok(())
    }

    pub async fn send_mail(
        &mut self,
        mail_from: &str,
        rcpt_to: &[&str],
        message: &str,
        timeout_ms: u32,
    ) -> Result<(), SmtpError> {
        self.send_line(format!("MAIL FROM:<{}>", mail_from).as_str())?;
        let mail_from_reply = self.read_reply(timeout_ms).await?;
        if mail_from_reply.code != 250 {
            crate::log!(
                "smtp: MAIL FROM rejected code={} line={}\n",
                mail_from_reply.code,
                mail_from_reply
                    .lines
                    .first()
                    .map(|s| s.as_str())
                    .unwrap_or("")
            );
            return Err(SmtpError::ReplyError(mail_from_reply.code));
        }

        for recipient in rcpt_to.iter().copied() {
            self.send_line(format!("RCPT TO:<{}>", recipient).as_str())?;
            let rcpt_reply = self.read_reply(timeout_ms).await?;
            if rcpt_reply.code != 250 && rcpt_reply.code != 251 {
                crate::log!(
                    "smtp: RCPT TO rejected code={} line={}\n",
                    rcpt_reply.code,
                    rcpt_reply.lines.first().map(|s| s.as_str()).unwrap_or("")
                );
                return Err(SmtpError::ReplyError(rcpt_reply.code));
            }
        }

        self.send_line("DATA")?;
        let data_reply = self.read_reply(timeout_ms).await?;
        if data_reply.code != 354 {
            crate::log!(
                "smtp: DATA rejected code={} line={}\n",
                data_reply.code,
                data_reply.lines.first().map(|s| s.as_str()).unwrap_or("")
            );
            return Err(SmtpError::ReplyError(data_reply.code));
        }

        self.send_data(message)?;
        let done_reply = self.read_reply(timeout_ms).await?;
        if done_reply.code != 250 {
            crate::log!(
                "smtp: message rejected code={} line={}\n",
                done_reply.code,
                done_reply.lines.first().map(|s| s.as_str()).unwrap_or("")
            );
            return Err(SmtpError::ReplyError(done_reply.code));
        }

        Ok(())
    }

    pub async fn quit(&mut self, timeout_ms: u32) -> Result<(), SmtpError> {
        self.send_line("QUIT")?;
        let _ = self.read_reply(timeout_ms).await;

        if !self.closed {
            self.net
                .submit(Command::Close {
                    handle: self.handle,
                })
                .map_err(|_| SmtpError::Io)?;
            self.closed = true;
        }

        Ok(())
    }

    async fn starttls(&mut self, timeout_ms: u32) -> Result<(), SmtpError> {
        self.send_line("STARTTLS")?;
        let reply = self.read_reply(timeout_ms).await?;
        if reply.code != 220 {
            return Err(SmtpError::ReplyError(reply.code));
        }

        let cfg = TlsClientConfig::new();
        let roots = TlsRoots::mozilla();
        let mut rng = KernelTlsRng::new();
        let mut tls = TlsClient::new(&cfg, &roots, SMTP_HOST, &mut rng, &KERNEL_TIME)
            .map_err(|_| SmtpError::TlsFailed)?;

        self.flush_tls_ciphertext(&mut tls)?;

        let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
        while !tls.is_connected() {
            match self.net.pop_event() {
                Some(Event::TcpData { handle, data }) if handle == self.handle => {
                    let plaintext = tls
                        .ingest_encrypted(data.as_slice())
                        .map_err(|_| SmtpError::TlsFailed)?;
                    if !plaintext.is_empty() {
                        self.rx.extend_from_slice(&plaintext);
                    }
                    self.flush_tls_ciphertext(&mut tls)?;
                }
                Some(Event::Closed { handle }) if handle == self.handle => {
                    self.closed = true;
                    return Err(SmtpError::Closed);
                }
                Some(Event::Error { .. }) => return Err(SmtpError::Io),
                _ => {}
            }

            if Instant::now() >= deadline {
                return Err(SmtpError::Timeout);
            }

            Timer::after(Duration::from_millis(1)).await;
        }

        self.tls = Some(tls);
        Ok(())
    }

    fn send_line(&mut self, line: &str) -> Result<(), SmtpError> {
        let mut buf = String::from(line);
        buf.push_str("\r\n");
        self.send_bytes(buf.as_bytes())
    }

    fn send_data(&mut self, body: &str) -> Result<(), SmtpError> {
        let mut out = String::new();
        for raw_line in body.split('\n') {
            let line = raw_line.trim_end_matches('\r');
            if line.starts_with('.') {
                out.push('.');
            }
            out.push_str(line);
            out.push_str("\r\n");
        }
        out.push_str(".\r\n");
        self.send_bytes(out.as_bytes())
    }

    fn send_bytes(&mut self, bytes: &[u8]) -> Result<(), SmtpError> {
        if self.closed {
            return Err(SmtpError::Closed);
        }

        if self.tls.is_none() {
            for chunk in bytes.chunks(vnet::MAX_MSG) {
                self.net
                    .submit(Command::SendTcp {
                        handle: self.handle,
                        data: ByteBuf::from_slice_trunc(chunk),
                    })
                    .map_err(|_| SmtpError::Io)?;
            }
            return Ok(());
        }

        // Write plaintext into TLS and flush produced ciphertext over TCP.
        if let Some(mut tls) = self.tls.take() {
            tls.write_plaintext(bytes)
                .map_err(|_| SmtpError::TlsFailed)?;
            self.flush_tls_ciphertext(&mut tls)?;
            self.tls = Some(tls);
        }

        Ok(())
    }

    async fn read_reply(&mut self, timeout_ms: u32) -> Result<SmtpReply, SmtpError> {
        let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);

        loop {
            if let Some((consumed, reply)) = parse_smtp_reply(self.rx.as_slice()) {
                self.rx.drain(0..consumed);
                return Ok(reply);
            }

            match self.net.pop_event() {
                Some(Event::TcpData { handle, data }) if handle == self.handle => {
                    if self.tls.is_none() {
                        self.rx.extend_from_slice(data.as_slice());
                    } else if let Some(mut tls) = self.tls.take() {
                        let plaintext = tls
                            .ingest_encrypted(data.as_slice())
                            .map_err(|_| SmtpError::TlsFailed)?;
                        if !plaintext.is_empty() {
                            self.rx.extend_from_slice(&plaintext);
                        }
                        self.flush_tls_ciphertext(&mut tls)?;
                        self.tls = Some(tls);
                    }
                }
                Some(Event::Closed { handle }) if handle == self.handle => {
                    self.closed = true;
                    return Err(SmtpError::Closed);
                }
                Some(Event::Error { .. }) => return Err(SmtpError::Io),
                _ => {}
            }

            if Instant::now() >= deadline {
                return Err(SmtpError::Timeout);
            }
            Timer::after(Duration::from_millis(1)).await;
        }
    }

    fn flush_tls_ciphertext(&mut self, tls: &mut TlsClient) -> Result<(), SmtpError> {
        let ciphertext = tls
            .take_ciphertext_to_send()
            .map_err(|_| SmtpError::TlsFailed)?;
        if ciphertext.is_empty() {
            return Ok(());
        }

        for chunk in ciphertext.chunks(vnet::MAX_MSG) {
            self.net
                .submit(Command::SendTcp {
                    handle: self.handle,
                    data: ByteBuf::from_slice_trunc(chunk),
                })
                .map_err(|_| SmtpError::Io)?;
        }

        Ok(())
    }
}

fn parse_smtp_reply(buf: &[u8]) -> Option<(usize, SmtpReply)> {
    let mut consumed = 0usize;
    let mut code: Option<u16> = None;
    let mut lines = Vec::new();

    loop {
        let rel_end = find_crlf(&buf[consumed..])?;
        let line_end = consumed + rel_end;
        let line = &buf[consumed..line_end];
        consumed = line_end + 2;

        if line.len() < 4 {
            return None;
        }

        let c = parse_reply_code(&line[..3])?;
        let sep = line[3];
        let text = core::str::from_utf8(&line[4..]).ok()?.trim();

        if let Some(existing) = code {
            if existing != c {
                return None;
            }
        } else {
            code = Some(c);
        }

        lines.push(String::from(text));

        match sep {
            b' ' => {
                return Some((consumed, SmtpReply { code: code?, lines }));
            }
            b'-' => {}
            _ => return None,
        }
    }
}

fn parse_reply_code(raw: &[u8]) -> Option<u16> {
    if raw.len() != 3 || !raw.iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let d0 = (raw[0] - b'0') as u16;
    let d1 = (raw[1] - b'0') as u16;
    let d2 = (raw[2] - b'0') as u16;
    Some(d0 * 100 + d1 * 10 + d2)
}

fn find_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\r\n")
}

fn base64_encode(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut out = String::new();
    let mut i = 0usize;
    while i < input.len() {
        let b0 = input[i];
        let b1 = if i + 1 < input.len() { input[i + 1] } else { 0 };
        let b2 = if i + 2 < input.len() { input[i + 2] } else { 0 };

        let n = ((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);

        out.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
        if i + 1 < input.len() {
            out.push(ALPHABET[((n >> 6) & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
        if i + 2 < input.len() {
            out.push(ALPHABET[(n & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }

        i += 3;
    }

    out
}
