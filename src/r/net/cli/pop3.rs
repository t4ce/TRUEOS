#![allow(dead_code)]

extern crate alloc;

use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    vec::Vec,
};
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_time::{Duration, Instant, Timer};
use v::vnet::{self, EndpointV4};

use crate::net::tls::{TlsClientConfig, TlsRoots};
use crate::net::tls_socket::{TlsCommand, TlsEvent, TlsTimeouts, register_tls_app_queues};
use crate::r::net::{NetProfile, Queue};
use crate::t::net::dns::{self, DnsConfig};

pub const POP3_HOST: &str = crate::allports::mail::POP3_HOST;
pub const POP3_PORT: u16 = crate::allports::mail::POP3_PORT;

static POP3_TLS_SEQ: AtomicU32 = AtomicU32::new(1);

#[derive(Debug)]
pub enum Pop3Error {
    DnsFailed,
    ConnectFailed,
    Timeout,
    Io,
    TlsFailed,
    Closed,
    Protocol,
    AuthFailed,
    TooLarge,
}

pub struct Pop3Client {
    cmds: &'static Queue<TlsCommand>,
    events: &'static Queue<TlsEvent>,
    handle: vnet::NetHandle,
    rx: Vec<u8>,
    closed: bool,
}

impl Pop3Client {
    pub async fn connect(timeout_ms: u32) -> Result<Self, Pop3Error> {
        Self::connect_with_profile(NetProfile::default(), timeout_ms).await
    }

    pub async fn connect_with_profile(
        profile: NetProfile,
        timeout_ms: u32,
    ) -> Result<Self, Pop3Error> {
        let dev_idx = profile
            .resolve_device_index()
            .ok_or(Pop3Error::ConnectFailed)?;
        let ip = dns::resolve_ipv4_for_device(dev_idx, POP3_HOST, DnsConfig::for_profile(profile))
            .await
            .map_err(|_| Pop3Error::DnsFailed)?;

        let seq = POP3_TLS_SEQ.fetch_add(1, Ordering::Relaxed) as u64;
        let selector = if let Some((bus, slot, func)) = crate::net::bdf_at(dev_idx) {
            format!("{:02x}:{:02x}.{}", bus, slot, func)
        } else if let Some((vid, pid)) = crate::net::pci_id_at(dev_idx) {
            format!("{:04x}:{:04x}", vid, pid)
        } else {
            format!("{}", dev_idx)
        };

        let owner = Box::leak(format!("pop3-{}@{}", seq, selector).into_boxed_str());
        let cmds_name = Box::leak(format!("{}-pop3-cmd", owner).into_boxed_str());
        let evts_name = Box::leak(format!("{}-pop3-evt", owner).into_boxed_str());

        let cmds = Queue::new_leaked(cmds_name, 256);
        let events = Queue::new_leaked(evts_name, 1024);
        register_tls_app_queues(owner, cmds, events);

        cmds.push(TlsCommand::OpenTcpConnect {
            remote: EndpointV4::new(ip, POP3_PORT),
            server_name: POP3_HOST,
            cfg: TlsClientConfig::new(),
            roots: TlsRoots::mozilla(),
            timeouts: TlsTimeouts {
                connect_ms: timeout_ms,
                tls_ms: timeout_ms,
                idle_ms: timeout_ms.saturating_mul(4).max(30_000),
            },
        })
        .map_err(|_| Pop3Error::ConnectFailed)?;

        let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
        let mut handle: Option<vnet::NetHandle> = None;
        let mut connected = false;
        let mut rx = Vec::new();

        while Instant::now() < deadline {
            while let Some(ev) = events.pop() {
                match ev {
                    TlsEvent::Opened { handle: h } => handle = Some(h),
                    TlsEvent::Connected { handle: h } if Some(h) == handle => {
                        connected = true;
                    }
                    TlsEvent::Data { handle: h, data } if Some(h) == handle => {
                        rx.extend_from_slice(&data);
                    }
                    TlsEvent::Closed { .. } => return Err(Pop3Error::Closed),
                    TlsEvent::Error { msg } => {
                        crate::log_trace!("pop3: tls-socket error msg={}\n", msg);
                        return Err(Pop3Error::TlsFailed);
                    }
                    TlsEvent::TlsError { err } => {
                        crate::log_trace!("pop3: tls error {:?}\n", err);
                        return Err(Pop3Error::TlsFailed);
                    }
                    _ => {}
                }
            }

            if connected {
                let mut client = Self {
                    cmds,
                    events,
                    handle: handle.ok_or(Pop3Error::ConnectFailed)?,
                    rx,
                    closed: false,
                };
                let greet = client.read_line(timeout_ms).await?;
                if !greet.starts_with("+OK") {
                    return Err(Pop3Error::Protocol);
                }
                return Ok(client);
            }

            Timer::after(Duration::from_millis(1)).await;
        }

        Err(Pop3Error::Timeout)
    }

    pub async fn login(
        &mut self,
        username: &str,
        password: &str,
        timeout_ms: u32,
    ) -> Result<(), Pop3Error> {
        self.command_ok(format!("USER {}", username).as_str(), timeout_ms)
            .await?;
        self.command_ok(format!("PASS {}", password).as_str(), timeout_ms)
            .await
            .map_err(|_| Pop3Error::AuthFailed)
    }

    pub async fn stat(&mut self, timeout_ms: u32) -> Result<(u32, u64), Pop3Error> {
        self.send_line("STAT")?;
        let line = self.read_line(timeout_ms).await?;
        if !line.starts_with("+OK") {
            return Err(Pop3Error::Protocol);
        }

        let mut it = line.split_whitespace();
        let _ok = it.next();
        let count = it
            .next()
            .and_then(|v| v.parse::<u32>().ok())
            .ok_or(Pop3Error::Protocol)?;
        let bytes = it
            .next()
            .and_then(|v| v.parse::<u64>().ok())
            .ok_or(Pop3Error::Protocol)?;
        Ok((count, bytes))
    }

    pub async fn list(&mut self, timeout_ms: u32) -> Result<Vec<(u32, u64)>, Pop3Error> {
        self.send_line("LIST")?;
        let first = self.read_line(timeout_ms).await?;
        if !first.starts_with("+OK") {
            return Err(Pop3Error::Protocol);
        }

        let lines = self.read_multiline(timeout_ms, 128 * 1024).await?;
        let mut out = Vec::new();
        for line in lines.lines() {
            let mut it = line.split_whitespace();
            let Some(id) = it.next().and_then(|v| v.parse::<u32>().ok()) else {
                continue;
            };
            let Some(size) = it.next().and_then(|v| v.parse::<u64>().ok()) else {
                continue;
            };
            out.push((id, size));
        }
        Ok(out)
    }

    pub async fn list_one(
        &mut self,
        msg_id: u32,
        timeout_ms: u32,
    ) -> Result<(u32, u64), Pop3Error> {
        self.send_line(format!("LIST {}", msg_id).as_str())?;
        let line = self.read_line(timeout_ms).await?;
        if !line.starts_with("+OK") {
            return Err(Pop3Error::Protocol);
        }

        let mut it = line.split_whitespace();
        let _ok = it.next();
        let id = it
            .next()
            .and_then(|v| v.parse::<u32>().ok())
            .ok_or(Pop3Error::Protocol)?;
        let size = it
            .next()
            .and_then(|v| v.parse::<u64>().ok())
            .ok_or(Pop3Error::Protocol)?;
        Ok((id, size))
    }

    pub async fn retr(
        &mut self,
        msg_id: u32,
        timeout_ms: u32,
        max_bytes: usize,
    ) -> Result<Vec<u8>, Pop3Error> {
        self.send_line(format!("RETR {}", msg_id).as_str())?;
        let first = self.read_line(timeout_ms).await?;
        if !first.starts_with("+OK") {
            return Err(Pop3Error::Protocol);
        }

        let text = self.read_multiline(timeout_ms, max_bytes).await?;
        Ok(text.into_bytes())
    }

    pub async fn top(
        &mut self,
        msg_id: u32,
        body_lines: u32,
        timeout_ms: u32,
        max_bytes: usize,
    ) -> Result<Vec<u8>, Pop3Error> {
        self.send_line(format!("TOP {} {}", msg_id, body_lines).as_str())?;
        let first = self.read_line(timeout_ms).await?;
        if !first.starts_with("+OK") {
            return Err(Pop3Error::Protocol);
        }

        let text = self.read_multiline(timeout_ms, max_bytes).await?;
        Ok(text.into_bytes())
    }

    pub async fn dele(&mut self, msg_id: u32, timeout_ms: u32) -> Result<(), Pop3Error> {
        self.command_ok(format!("DELE {}", msg_id).as_str(), timeout_ms)
            .await
    }

    pub async fn noop(&mut self, timeout_ms: u32) -> Result<(), Pop3Error> {
        self.command_ok("NOOP", timeout_ms).await
    }

    pub async fn quit(&mut self, timeout_ms: u32) -> Result<(), Pop3Error> {
        let _ = self.command_ok("QUIT", timeout_ms).await;
        if !self.closed {
            self.cmds
                .push(TlsCommand::Close {
                    handle: self.handle,
                })
                .map_err(|_| Pop3Error::Io)?;
            self.closed = true;
        }
        Ok(())
    }

    async fn command_ok(&mut self, line: &str, timeout_ms: u32) -> Result<(), Pop3Error> {
        self.send_line(line)?;
        let reply = self.read_line(timeout_ms).await?;
        if reply.starts_with("+OK") {
            Ok(())
        } else {
            Err(Pop3Error::Protocol)
        }
    }

    fn send_line(&mut self, line: &str) -> Result<(), Pop3Error> {
        if self.closed {
            return Err(Pop3Error::Closed);
        }
        let mut out = String::from(line);
        out.push_str("\r\n");
        self.cmds
            .push(TlsCommand::Send {
                handle: self.handle,
                data: out.into_bytes(),
            })
            .map_err(|_| Pop3Error::Io)
    }

    async fn read_line(&mut self, timeout_ms: u32) -> Result<String, Pop3Error> {
        let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
        loop {
            if let Some(line_end) = find_crlf(self.rx.as_slice()) {
                let line = core::str::from_utf8(&self.rx[..line_end])
                    .map_err(|_| Pop3Error::Protocol)?
                    .to_string();
                self.rx.drain(0..line_end + 2);
                return Ok(line);
            }

            self.pump_events()?;

            if Instant::now() >= deadline {
                return Err(Pop3Error::Timeout);
            }
            Timer::after(Duration::from_millis(1)).await;
        }
    }

    async fn read_multiline(
        &mut self,
        timeout_ms: u32,
        max_bytes: usize,
    ) -> Result<String, Pop3Error> {
        let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);

        loop {
            if let Some(term) = find_multiline_end(self.rx.as_slice()) {
                let raw = &self.rx[..term];
                let text = pop3_dot_unstuff(raw)?;
                self.rx.drain(0..term + 5); // "\r\n.\r\n"
                return Ok(text);
            }

            self.pump_events()?;
            if self.rx.len() > max_bytes {
                return Err(Pop3Error::TooLarge);
            }

            if Instant::now() >= deadline {
                return Err(Pop3Error::Timeout);
            }
            Timer::after(Duration::from_millis(1)).await;
        }
    }

    fn pump_events(&mut self) -> Result<(), Pop3Error> {
        while let Some(ev) = self.events.pop() {
            match ev {
                TlsEvent::Data { handle, data } if handle == self.handle => {
                    self.rx.extend_from_slice(&data);
                }
                TlsEvent::Closed { handle } if handle == self.handle => {
                    self.closed = true;
                    return Err(Pop3Error::Closed);
                }
                TlsEvent::Error { .. } | TlsEvent::TlsError { .. } => {
                    return Err(Pop3Error::TlsFailed);
                }
                _ => {}
            }
        }
        Ok(())
    }
}

fn find_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\r\n")
}

fn find_multiline_end(buf: &[u8]) -> Option<usize> {
    buf.windows(5).position(|w| w == b"\r\n.\r\n")
}

fn pop3_dot_unstuff(raw: &[u8]) -> Result<String, Pop3Error> {
    let txt = core::str::from_utf8(raw).map_err(|_| Pop3Error::Protocol)?;
    let mut out = String::new();
    for line in txt.split("\r\n") {
        if let Some(stripped) = line.strip_prefix("..") {
            out.push_str(stripped);
        } else {
            out.push_str(line);
        }
        out.push_str("\r\n");
    }
    Ok(out)
}
