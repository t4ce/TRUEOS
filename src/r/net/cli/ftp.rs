#![allow(dead_code)]

extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU16, Ordering};

use embassy_time::{Duration, Instant, Timer};
use v::vnet::{self, ByteBuf, Command, EndpointV4, Event, NetHandle, SocketKind};

use crate::r::net::{NetProfile, VNet, ports};
use crate::t::net::dns::{self, DnsConfig};

const FTP_SERVER_IDLE_SLEEP_MS: u64 = 2;
const FTP_SERVER_IO_TIMEOUT_MS: u32 = 30_000;
const FTP_SERVER_MAX_UPLOAD_BYTES: usize = 16 * 1024 * 1024;

static FTP_SERVER_STARTED: AtomicBool = AtomicBool::new(false);
static FTP_SERVER_NEXT_PASV_PORT: AtomicU16 = AtomicU16::new(ports::FTP_SERVER_PASV_MIN);

#[derive(Clone, Debug)]
struct ParsedFtpUrl {
    host: String,
    port: u16,
}

#[derive(Clone, Debug)]
pub struct FtpResponse {
    pub code: u16,
    pub message: String,
}

#[derive(Debug)]
pub enum FtpError {
    InvalidUrl,
    ConnectFailed,
    DnsFailed,
    Timeout,
    Protocol,
    AuthFailed,
    Io,
    Closed,
    TooLarge,
}

pub struct FtpSocket {
    net: VNet,
    ctrl_handle: NetHandle,
    ctrl_rx: Vec<u8>,
    closed: bool,
}

impl FtpSocket {
    pub async fn connect(url: &str, timeout_ms: u32) -> Result<Self, FtpError> {
        Self::connect_with_profile(url, NetProfile::default(), timeout_ms).await
    }

    pub async fn connect_with_profile(
        url: &str,
        profile: NetProfile,
        timeout_ms: u32,
    ) -> Result<Self, FtpError> {
        let parsed = parse_ftp_url(url).ok_or(FtpError::InvalidUrl)?;
        let dev_idx = profile
            .resolve_device_index()
            .ok_or(FtpError::ConnectFailed)?;
        let ip = dns::resolve_ipv4_for_device(
            dev_idx,
            parsed.host.as_str(),
            DnsConfig::for_profile(profile),
        )
        .await
        .map_err(|_| FtpError::DnsFailed)?;

        let net = VNet::open_with_profile(profile).ok_or(FtpError::ConnectFailed)?;
        net.submit(Command::OpenTcpConnect {
            remote: EndpointV4::new(ip, parsed.port),
        })
        .map_err(|_| FtpError::ConnectFailed)?;

        let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
        let mut ctrl_handle: Option<NetHandle> = None;
        let mut ctrl_rx = Vec::new();

        loop {
            match net.pop_event() {
                Some(Event::Opened { handle, kind }) => {
                    if kind == SocketKind::Tcp {
                        ctrl_handle = Some(handle);
                    }
                }
                Some(Event::TcpEstablished { handle, .. }) => {
                    if Some(handle) == ctrl_handle {
                        break;
                    }
                }
                Some(Event::TcpData { handle, data }) => {
                    if Some(handle) == ctrl_handle {
                        ctrl_rx.extend_from_slice(data.as_slice());
                    }
                }
                Some(Event::Closed { handle }) => {
                    if Some(handle) == ctrl_handle {
                        return Err(FtpError::ConnectFailed);
                    }
                }
                Some(Event::Error { .. }) => return Err(FtpError::ConnectFailed),
                _ => {}
            }

            if Instant::now() >= deadline {
                return Err(FtpError::Timeout);
            }
            Timer::after(Duration::from_millis(1)).await;
        }

        let ctrl_handle = ctrl_handle.ok_or(FtpError::ConnectFailed)?;
        let mut socket = Self {
            net,
            ctrl_handle,
            ctrl_rx,
            closed: false,
        };

        let banner = socket.read_response(timeout_ms).await?;
        if banner.code != 220 {
            return Err(FtpError::Protocol);
        }

        Ok(socket)
    }

    pub async fn login(&mut self, user: &str, pass: &str, timeout_ms: u32) -> Result<(), FtpError> {
        self.send_command(format!("USER {}", user).as_str())?;
        let user_rsp = self.read_response(timeout_ms).await?;

        match user_rsp.code {
            230 => {
                self.set_binary(timeout_ms).await?;
                Ok(())
            }
            331 => {
                self.send_command(format!("PASS {}", pass).as_str())?;
                let pass_rsp = self.read_response(timeout_ms).await?;
                if pass_rsp.code != 230 {
                    return Err(FtpError::AuthFailed);
                }
                self.set_binary(timeout_ms).await?;
                Ok(())
            }
            _ => Err(FtpError::AuthFailed),
        }
    }

    pub async fn quit(&mut self, timeout_ms: u32) -> Result<(), FtpError> {
        if self.closed {
            return Ok(());
        }
        self.send_command("QUIT")?;
        let _ = self.read_response(timeout_ms).await;
        self.net
            .submit(Command::Close {
                handle: self.ctrl_handle,
            })
            .map_err(|_| FtpError::Io)?;
        self.closed = true;
        Ok(())
    }

    pub async fn retr(
        &mut self,
        path: &str,
        timeout_ms: u32,
        max_bytes: usize,
    ) -> Result<Vec<u8>, FtpError> {
        let data_remote = self.enter_passive(timeout_ms).await?;
        let data_handle = self.open_data_socket(data_remote, timeout_ms).await?;

        self.send_command(format!("RETR {}", path).as_str())?;
        let pre = self.read_response(timeout_ms).await?;
        if pre.code != 125 && pre.code != 150 {
            let _ = self.net.submit(Command::Close {
                handle: data_handle,
            });
            return Err(FtpError::Protocol);
        }

        let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
        let mut out = Vec::new();
        let mut data_closed = false;

        while !data_closed {
            match self.net.pop_event() {
                Some(Event::TcpData { handle, data }) => {
                    if handle == data_handle {
                        let room = max_bytes.saturating_sub(out.len());
                        if room == 0 {
                            let _ = self.net.submit(Command::Close {
                                handle: data_handle,
                            });
                            return Err(FtpError::TooLarge);
                        }
                        let bytes = data.as_slice();
                        let take = bytes.len().min(room);
                        out.extend_from_slice(&bytes[..take]);
                        if take < bytes.len() {
                            let _ = self.net.submit(Command::Close {
                                handle: data_handle,
                            });
                            return Err(FtpError::TooLarge);
                        }
                    } else if handle == self.ctrl_handle {
                        self.ctrl_rx.extend_from_slice(data.as_slice());
                    }
                }
                Some(Event::Closed { handle }) => {
                    if handle == data_handle {
                        data_closed = true;
                    } else if handle == self.ctrl_handle {
                        self.closed = true;
                        return Err(FtpError::Closed);
                    }
                }
                Some(Event::Error { .. }) => return Err(FtpError::Io),
                _ => {}
            }

            if Instant::now() >= deadline {
                let _ = self.net.submit(Command::Close {
                    handle: data_handle,
                });
                return Err(FtpError::Timeout);
            }
            Timer::after(Duration::from_millis(1)).await;
        }

        let done = self.read_response(timeout_ms).await?;
        if done.code != 226 && done.code != 250 {
            return Err(FtpError::Protocol);
        }

        Ok(out)
    }

    pub async fn stor(&mut self, path: &str, data: &[u8], timeout_ms: u32) -> Result<(), FtpError> {
        let data_remote = self.enter_passive(timeout_ms).await?;
        let data_handle = self.open_data_socket(data_remote, timeout_ms).await?;

        self.send_command(format!("STOR {}", path).as_str())?;
        let pre = self.read_response(timeout_ms).await?;
        if pre.code != 125 && pre.code != 150 {
            let _ = self.net.submit(Command::Close {
                handle: data_handle,
            });
            return Err(FtpError::Protocol);
        }

        for chunk in data.chunks(vnet::MAX_MSG) {
            self.net
                .submit(Command::SendTcp {
                    handle: data_handle,
                    data: ByteBuf::from_slice_trunc(chunk),
                })
                .map_err(|_| FtpError::Io)?;
        }

        self.net
            .submit(Command::Close {
                handle: data_handle,
            })
            .map_err(|_| FtpError::Io)?;

        let done = self.read_response(timeout_ms).await?;
        if done.code != 226 && done.code != 250 {
            return Err(FtpError::Protocol);
        }
        Ok(())
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }

    fn send_command(&self, command: &str) -> Result<(), FtpError> {
        if self.closed {
            return Err(FtpError::Closed);
        }

        let mut line = String::from(command);
        line.push_str("\r\n");

        for chunk in line.as_bytes().chunks(vnet::MAX_MSG) {
            self.net
                .submit(Command::SendTcp {
                    handle: self.ctrl_handle,
                    data: ByteBuf::from_slice_trunc(chunk),
                })
                .map_err(|_| FtpError::Io)?;
        }

        Ok(())
    }

    async fn set_binary(&mut self, timeout_ms: u32) -> Result<(), FtpError> {
        self.send_command("TYPE I")?;
        let rsp = self.read_response(timeout_ms).await?;
        if rsp.code == 200 {
            Ok(())
        } else {
            Err(FtpError::Protocol)
        }
    }

    async fn read_response(&mut self, timeout_ms: u32) -> Result<FtpResponse, FtpError> {
        if self.closed {
            return Err(FtpError::Closed);
        }

        let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);

        loop {
            if let Some((consumed, resp)) = parse_ftp_response(self.ctrl_rx.as_slice()) {
                self.ctrl_rx.drain(0..consumed);
                return Ok(resp);
            }

            match self.net.pop_event() {
                Some(Event::TcpData { handle, data }) if handle == self.ctrl_handle => {
                    self.ctrl_rx.extend_from_slice(data.as_slice());
                }
                Some(Event::Closed { handle }) if handle == self.ctrl_handle => {
                    self.closed = true;
                    return Err(FtpError::Closed);
                }
                Some(Event::Error { .. }) => return Err(FtpError::Io),
                _ => {}
            }

            if Instant::now() >= deadline {
                return Err(FtpError::Timeout);
            }
            Timer::after(Duration::from_millis(1)).await;
        }
    }

    async fn enter_passive(&mut self, timeout_ms: u32) -> Result<EndpointV4, FtpError> {
        self.send_command("PASV")?;
        let rsp = self.read_response(timeout_ms).await?;
        if rsp.code != 227 {
            return Err(FtpError::Protocol);
        }
        let (addr, port) = parse_pasv_endpoint(rsp.message.as_str()).ok_or(FtpError::Protocol)?;
        Ok(EndpointV4::new(addr, port))
    }

    async fn open_data_socket(
        &mut self,
        remote: EndpointV4,
        timeout_ms: u32,
    ) -> Result<NetHandle, FtpError> {
        self.net
            .submit(Command::OpenTcpConnect { remote })
            .map_err(|_| FtpError::Io)?;

        let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
        let mut data_handle: Option<NetHandle> = None;

        loop {
            match self.net.pop_event() {
                Some(Event::Opened { handle, kind }) if kind == SocketKind::Tcp => {
                    if handle != self.ctrl_handle {
                        data_handle = Some(handle);
                    }
                }
                Some(Event::TcpEstablished { handle, .. }) if Some(handle) == data_handle => {
                    return Ok(handle);
                }
                Some(Event::TcpData { handle, data }) if handle == self.ctrl_handle => {
                    self.ctrl_rx.extend_from_slice(data.as_slice());
                }
                Some(Event::Closed { handle }) if Some(handle) == data_handle => {
                    return Err(FtpError::ConnectFailed);
                }
                Some(Event::Closed { handle }) if handle == self.ctrl_handle => {
                    self.closed = true;
                    return Err(FtpError::Closed);
                }
                Some(Event::Error { .. }) => return Err(FtpError::ConnectFailed),
                _ => {}
            }

            if Instant::now() >= deadline {
                return Err(FtpError::Timeout);
            }
            Timer::after(Duration::from_millis(1)).await;
        }
    }
}

fn parse_ftp_url(url: &str) -> Option<ParsedFtpUrl> {
    let u = url.strip_prefix("ftp://")?;
    let authority = match u.split_once('/') {
        Some((a, _)) => a,
        None => u,
    };

    if authority.is_empty() {
        return None;
    }

    let (host, port) = if let Some((h, p)) = authority.rsplit_once(':') {
        if !p.is_empty() && p.as_bytes().iter().all(|b| b.is_ascii_digit()) {
            (String::from(h), p.parse::<u16>().ok()?)
        } else {
            (String::from(authority), 21)
        }
    } else {
        (String::from(authority), 21)
    };

    if host.is_empty() {
        return None;
    }

    Some(ParsedFtpUrl { host, port })
}

fn parse_ftp_response(buf: &[u8]) -> Option<(usize, FtpResponse)> {
    let first_end = find_crlf(buf)?;
    if first_end < 5 {
        return None;
    }

    let first = &buf[..first_end];
    let code = parse_reply_code(first)?;
    let sep = *first.get(3)?;

    if sep == b' ' {
        let consumed = first_end + 2;
        let message = String::from(core::str::from_utf8(first).ok()?);
        return Some((
            consumed,
            FtpResponse {
                code,
                message: String::from(message.trim_end_matches('\r')),
            },
        ));
    }

    if sep != b'-' {
        return None;
    }

    let mut idx = first_end + 2;
    while idx < buf.len() {
        let line_end_rel = find_crlf(&buf[idx..])?;
        let line_end = idx + line_end_rel;
        let line = &buf[idx..line_end];

        if line.len() >= 4
            && parse_reply_code(line) == Some(code)
            && line.get(3).copied() == Some(b' ')
        {
            let consumed = line_end + 2;
            let text = String::from(core::str::from_utf8(&buf[..line_end]).ok()?);
            return Some((
                consumed,
                FtpResponse {
                    code,
                    message: text,
                },
            ));
        }

        idx = line_end + 2;
    }

    None
}

fn parse_reply_code(line: &[u8]) -> Option<u16> {
    if line.len() < 3 {
        return None;
    }
    let a = *line.first()?;
    let b = *line.get(1)?;
    let c = *line.get(2)?;
    if !a.is_ascii_digit() || !b.is_ascii_digit() || !c.is_ascii_digit() {
        return None;
    }
    Some(((a - b'0') as u16) * 100 + ((b - b'0') as u16) * 10 + (c - b'0') as u16)
}

fn find_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\r\n")
}

fn parse_pasv_endpoint(message: &str) -> Option<([u8; 4], u16)> {
    let start = message.find('(')?;
    let end = message[start..].find(')')? + start;
    let inner = &message[start + 1..end];

    let mut nums = [0u16; 6];
    let mut count = 0usize;
    for part in inner.split(',') {
        if count >= 6 {
            return None;
        }
        nums[count] = part.trim().parse::<u16>().ok()?;
        count += 1;
    }
    if count != 6 {
        return None;
    }

    if nums[0] > 255
        || nums[1] > 255
        || nums[2] > 255
        || nums[3] > 255
        || nums[4] > 255
        || nums[5] > 255
    {
        return None;
    }

    let ip = [nums[0] as u8, nums[1] as u8, nums[2] as u8, nums[3] as u8];
    let port = (nums[4] << 8) | nums[5];
    Some((ip, port))
}

struct FtpServerSession {
    ctrl: NetHandle,
    rx: Vec<u8>,
    cwd: String,
    logged_in: bool,
    pasv_port: Option<u16>,
    pasv_listener: Option<NetHandle>,
    pasv_data: Option<NetHandle>,
}

impl FtpServerSession {
    fn new(ctrl: NetHandle) -> Self {
        Self {
            ctrl,
            rx: Vec::new(),
            cwd: String::from("/"),
            logged_in: false,
            pasv_port: None,
            pasv_listener: None,
            pasv_data: None,
        }
    }
}

pub fn ftp_server_port() -> u16 {
    ports::FTP_SERVER_PORT
}

#[embassy_executor::task]
pub async fn ftp_server_task() {
    if FTP_SERVER_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }

    loop {
        let Some(mut vnet) = VNet::open_default() else {
            Timer::after(Duration::from_millis(100)).await;
            continue;
        };

        let mut listener: Option<NetHandle> = None;
        let mut session: Option<FtpServerSession> = None;

        if vnet
            .submit(Command::OpenTcpListen {
                port: ports::FTP_SERVER_PORT,
            })
            .is_err()
        {
            Timer::after(Duration::from_millis(250)).await;
            continue;
        }

        crate::log!("ftp: listening on tcp {}\n", ports::FTP_SERVER_PORT);

        loop {
            while let Some(ev) = vnet.pop_event() {
                match ev {
                    Event::Opened { handle, kind } => {
                        if kind != SocketKind::Tcp {
                            continue;
                        }
                        if listener.is_none() {
                            listener = Some(handle);
                            continue;
                        }
                        if let Some(sess) = session.as_mut()
                            && sess.pasv_port.is_some()
                            && sess.pasv_listener.is_none()
                        {
                            sess.pasv_listener = Some(handle);
                        }
                    }
                    Event::TcpEstablished { handle, .. } => {
                        if Some(handle) == listener {
                            if let Some(old) = session.as_mut() {
                                ftp_close_passive(&vnet, old);
                                let _ = vnet.submit(Command::Close { handle: old.ctrl });
                            }
                            let new_sess = FtpServerSession::new(handle);
                            let _ = ftp_send_reply(&vnet, &new_sess, 220, "TRUEOS FTP ready");
                            session = Some(new_sess);
                            continue;
                        }

                        if let Some(sess) = session.as_mut()
                            && Some(handle) == sess.pasv_listener
                        {
                            sess.pasv_data = Some(handle);
                        }
                    }
                    Event::TcpData { handle, data } => {
                        if let Some(sess) = session.as_mut()
                            && handle == sess.ctrl
                        {
                            sess.rx.extend_from_slice(data.as_slice());
                            while let Some(line) = ftp_take_line(&mut sess.rx) {
                                if ftp_handle_command(&mut vnet, sess, line.as_str()).await {
                                    let _ = vnet.submit(Command::Close { handle: sess.ctrl });
                                    ftp_close_passive(&vnet, sess);
                                    session = None;
                                    break;
                                }
                            }
                        }
                    }
                    Event::Closed { handle } => {
                        if Some(handle) == listener {
                            listener = None;
                            let _ = vnet.submit(Command::OpenTcpListen {
                                port: ports::FTP_SERVER_PORT,
                            });
                            continue;
                        }

                        if let Some(sess) = session.as_mut() {
                            if handle == sess.ctrl {
                                ftp_close_passive(&vnet, sess);
                                session = None;
                            } else if Some(handle) == sess.pasv_listener {
                                sess.pasv_listener = None;
                                sess.pasv_data = None;
                                sess.pasv_port = None;
                            } else if Some(handle) == sess.pasv_data {
                                sess.pasv_data = None;
                            }
                        }
                    }
                    Event::Error { msg } => {
                        crate::log!("ftp: net event error {}\n", msg);
                    }
                    _ => {}
                }
            }

            Timer::after(Duration::from_millis(FTP_SERVER_IDLE_SLEEP_MS)).await;
        }
    }
}

async fn ftp_handle_command(vnet: &mut VNet, sess: &mut FtpServerSession, line: &str) -> bool {
    let mut parts = line.trim().splitn(2, ' ');
    let cmd = parts.next().unwrap_or("");
    let arg = parts.next().unwrap_or("").trim();
    let upper = cmd.to_ascii_uppercase();

    match upper.as_str() {
        "USER" => {
            let _ = ftp_send_reply(vnet, sess, 331, "password required");
        }
        "PASS" => {
            sess.logged_in = true;
            let _ = ftp_send_reply(vnet, sess, 230, "login successful");
        }
        "SYST" => {
            let _ = ftp_send_reply(vnet, sess, 215, "UNIX Type: L8");
        }
        "FEAT" => {
            let _ = ftp_send_raw(
                vnet,
                sess.ctrl,
                b"211-Features\r\n PASV\r\n SIZE\r\n UTF8\r\n211 End\r\n",
            );
        }
        "OPTS" => {
            let _ = ftp_send_reply(vnet, sess, 200, "ok");
        }
        "TYPE" => {
            let _ = ftp_send_reply(vnet, sess, 200, "type set");
        }
        "NOOP" => {
            let _ = ftp_send_reply(vnet, sess, 200, "ok");
        }
        "PWD" => {
            let msg = format!("\"{}\"", sess.cwd);
            let _ = ftp_send_reply(vnet, sess, 257, msg.as_str());
        }
        "CWD" => {
            if !sess.logged_in {
                let _ = ftp_send_reply(vnet, sess, 530, "login first");
            } else if let Some(path) = ftp_normalize_path(sess.cwd.as_str(), arg) {
                if let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() {
                    match crate::r::fs::trueosfs::list_dir_async(disk, path.as_str()).await {
                        Ok(Some(_)) => {
                            sess.cwd = if path.is_empty() {
                                String::from("/")
                            } else {
                                format!("/{}", path)
                            };
                            let _ = ftp_send_reply(vnet, sess, 250, "directory changed");
                        }
                        _ => {
                            let _ = ftp_send_reply(vnet, sess, 550, "directory unavailable");
                        }
                    }
                } else {
                    let _ = ftp_send_reply(vnet, sess, 550, "filesystem unavailable");
                }
            } else {
                let _ = ftp_send_reply(vnet, sess, 550, "invalid path");
            }
        }
        "PASV" => {
            if !sess.logged_in {
                let _ = ftp_send_reply(vnet, sess, 530, "login first");
            } else {
                ftp_close_passive(vnet, sess);

                let port = ftp_next_pasv_port();
                if vnet.submit(Command::OpenTcpListen { port }).is_err() {
                    let _ = ftp_send_reply(vnet, sess, 425, "cannot open passive socket");
                } else {
                    sess.pasv_port = Some(port);
                    let dev_idx = crate::net::device_index_from_owner(vnet.owner())
                        .unwrap_or_else(crate::net::default_device_index);
                    let ip = crate::net::adapter::ipv4_at(dev_idx).unwrap_or([127, 0, 0, 1]);
                    let p1 = (port >> 8) as u8;
                    let p2 = (port & 0xFF) as u8;
                    let msg = format!(
                        "Entering Passive Mode ({},{},{},{},{},{})",
                        ip[0], ip[1], ip[2], ip[3], p1, p2
                    );
                    let _ = ftp_send_reply(vnet, sess, 227, msg.as_str());
                }
            }
        }
        "LIST" | "NLST" => {
            if !sess.logged_in {
                let _ = ftp_send_reply(vnet, sess, 530, "login first");
            } else {
                let _ = ftp_send_reply(vnet, sess, 150, "opening data connection");
                match ftp_wait_passive_data(vnet, sess, FTP_SERVER_IO_TIMEOUT_MS).await {
                    Ok(data_handle) => {
                        let root = crate::r::fs::trueosfs::primary_root_handle();
                        if let Some(disk) = root {
                            let path = ftp_normalize_path(sess.cwd.as_str(), arg)
                                .unwrap_or_else(|| ftp_cwd_rel(sess.cwd.as_str()));
                            let payload =
                                match crate::r::fs::trueosfs::list_dir_async(disk, path.as_str())
                                    .await
                                {
                                    Ok(Some(list)) => ftp_listing_from_names(list.as_str()),
                                    _ => Vec::new(),
                                };
                            let _ = ftp_send_raw(vnet, data_handle, payload.as_slice());
                            let _ = vnet.submit(Command::Close {
                                handle: data_handle,
                            });
                            ftp_close_passive(vnet, sess);
                            let _ = ftp_send_reply(vnet, sess, 226, "transfer complete");
                        } else {
                            let _ = ftp_send_reply(vnet, sess, 550, "filesystem unavailable");
                        }
                    }
                    Err(_) => {
                        ftp_close_passive(vnet, sess);
                        let _ = ftp_send_reply(vnet, sess, 425, "data connection failed");
                    }
                }
            }
        }
        "SIZE" => {
            if !sess.logged_in {
                let _ = ftp_send_reply(vnet, sess, 530, "login first");
            } else if let Some(path) = ftp_normalize_path(sess.cwd.as_str(), arg) {
                if let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() {
                    match crate::r::fs::trueosfs::file_info_async(disk, path.as_str()).await {
                        Ok(Some(info)) => {
                            let _ = ftp_send_reply(
                                vnet,
                                sess,
                                213,
                                format!("{}", info.data_len).as_str(),
                            );
                        }
                        _ => {
                            let _ = ftp_send_reply(vnet, sess, 550, "not found");
                        }
                    }
                } else {
                    let _ = ftp_send_reply(vnet, sess, 550, "filesystem unavailable");
                }
            } else {
                let _ = ftp_send_reply(vnet, sess, 550, "invalid path");
            }
        }
        "RETR" => {
            if !sess.logged_in {
                let _ = ftp_send_reply(vnet, sess, 530, "login first");
            } else if let Some(path) = ftp_normalize_path(sess.cwd.as_str(), arg) {
                let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
                    let _ = ftp_send_reply(vnet, sess, 550, "filesystem unavailable");
                    return false;
                };

                let _ = ftp_send_reply(vnet, sess, 150, "opening data connection");
                let Ok(data_handle) =
                    ftp_wait_passive_data(vnet, sess, FTP_SERVER_IO_TIMEOUT_MS).await
                else {
                    ftp_close_passive(vnet, sess);
                    let _ = ftp_send_reply(vnet, sess, 425, "data connection failed");
                    return false;
                };

                match crate::r::fs::trueosfs::file_out_async(disk, path.as_str()).await {
                    Ok(Some(bytes)) => {
                        let _ = ftp_send_raw(vnet, data_handle, bytes.as_slice());
                        let _ = vnet.submit(Command::Close {
                            handle: data_handle,
                        });
                        ftp_close_passive(vnet, sess);
                        let _ = ftp_send_reply(vnet, sess, 226, "transfer complete");
                    }
                    _ => {
                        let _ = vnet.submit(Command::Close {
                            handle: data_handle,
                        });
                        ftp_close_passive(vnet, sess);
                        let _ = ftp_send_reply(vnet, sess, 550, "not found");
                    }
                }
            } else {
                let _ = ftp_send_reply(vnet, sess, 550, "invalid path");
            }
        }
        "STOR" => {
            if !sess.logged_in {
                let _ = ftp_send_reply(vnet, sess, 530, "login first");
            } else if let Some(path) = ftp_normalize_path(sess.cwd.as_str(), arg) {
                let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
                    let _ = ftp_send_reply(vnet, sess, 550, "filesystem unavailable");
                    return false;
                };

                let _ = ftp_send_reply(vnet, sess, 150, "opening data connection");
                let Ok(data_handle) =
                    ftp_wait_passive_data(vnet, sess, FTP_SERVER_IO_TIMEOUT_MS).await
                else {
                    ftp_close_passive(vnet, sess);
                    let _ = ftp_send_reply(vnet, sess, 425, "data connection failed");
                    return false;
                };

                match ftp_receive_data(
                    vnet,
                    sess,
                    data_handle,
                    FTP_SERVER_IO_TIMEOUT_MS,
                    FTP_SERVER_MAX_UPLOAD_BYTES,
                )
                .await
                {
                    Ok(bytes) => {
                        let _ = vnet.submit(Command::Close {
                            handle: data_handle,
                        });
                        ftp_close_passive(vnet, sess);
                        match crate::r::fs::trueosfs::file_in_async(
                            disk,
                            path.as_str(),
                            bytes.as_slice(),
                        )
                        .await
                        {
                            Ok(true) => {
                                let _ = ftp_send_reply(vnet, sess, 226, "transfer complete");
                            }
                            _ => {
                                let _ = ftp_send_reply(vnet, sess, 550, "write failed");
                            }
                        }
                    }
                    Err(_) => {
                        let _ = vnet.submit(Command::Close {
                            handle: data_handle,
                        });
                        ftp_close_passive(vnet, sess);
                        let _ = ftp_send_reply(vnet, sess, 426, "transfer aborted");
                    }
                }
            } else {
                let _ = ftp_send_reply(vnet, sess, 550, "invalid path");
            }
        }
        "DELE" => {
            if !sess.logged_in {
                let _ = ftp_send_reply(vnet, sess, 530, "login first");
            } else if let Some(path) = ftp_normalize_path(sess.cwd.as_str(), arg) {
                if let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() {
                    match crate::r::fs::trueosfs::file_delete_async(disk, path.as_str()).await {
                        Ok(true) => {
                            let _ = ftp_send_reply(vnet, sess, 250, "deleted");
                        }
                        _ => {
                            let _ = ftp_send_reply(vnet, sess, 550, "delete failed");
                        }
                    }
                } else {
                    let _ = ftp_send_reply(vnet, sess, 550, "filesystem unavailable");
                }
            } else {
                let _ = ftp_send_reply(vnet, sess, 550, "invalid path");
            }
        }
        "QUIT" => {
            let _ = ftp_send_reply(vnet, sess, 221, "bye");
            return true;
        }
        _ => {
            let _ = ftp_send_reply(vnet, sess, 502, "command not implemented");
        }
    }

    false
}

fn ftp_send_reply(
    vnet: &VNet,
    sess: &FtpServerSession,
    code: u16,
    message: &str,
) -> Result<(), ()> {
    let line = format!("{} {}\r\n", code, message);
    ftp_send_raw(vnet, sess.ctrl, line.as_bytes())
}

fn ftp_send_raw(vnet: &VNet, handle: NetHandle, bytes: &[u8]) -> Result<(), ()> {
    for chunk in bytes.chunks(vnet::MAX_MSG) {
        vnet.submit(Command::SendTcp {
            handle,
            data: ByteBuf::from_slice_trunc(chunk),
        })?;
    }
    Ok(())
}

fn ftp_take_line(rx: &mut Vec<u8>) -> Option<String> {
    let pos = rx.windows(2).position(|w| w == b"\r\n")?;
    let line = String::from(core::str::from_utf8(&rx[..pos]).ok()?);
    rx.drain(0..(pos + 2));
    Some(line)
}

fn ftp_close_passive(vnet: &VNet, sess: &mut FtpServerSession) {
    if let Some(h) = sess.pasv_data.take() {
        let _ = vnet.submit(Command::Close { handle: h });
    }
    if let Some(h) = sess.pasv_listener.take() {
        let _ = vnet.submit(Command::Close { handle: h });
    }
    sess.pasv_port = None;
}

fn ftp_next_pasv_port() -> u16 {
    let mut current = FTP_SERVER_NEXT_PASV_PORT.load(Ordering::Relaxed);
    loop {
        let next = if current >= ports::FTP_SERVER_PASV_MAX {
            ports::FTP_SERVER_PASV_MIN
        } else {
            current + 1
        };
        match FTP_SERVER_NEXT_PASV_PORT.compare_exchange(
            current,
            next,
            Ordering::AcqRel,
            Ordering::Relaxed,
        ) {
            Ok(_) => return current,
            Err(observed) => current = observed,
        }
    }
}

async fn ftp_wait_passive_data(
    vnet: &mut VNet,
    sess: &mut FtpServerSession,
    timeout_ms: u32,
) -> Result<NetHandle, ()> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
    loop {
        if let Some(h) = sess.pasv_data {
            return Ok(h);
        }

        while let Some(ev) = vnet.pop_event() {
            match ev {
                Event::Opened { handle, kind } if kind == SocketKind::Tcp => {
                    if sess.pasv_port.is_some()
                        && sess.pasv_listener.is_none()
                        && handle != sess.ctrl
                    {
                        sess.pasv_listener = Some(handle);
                    }
                }
                Event::TcpEstablished { handle, .. } => {
                    if Some(handle) == sess.pasv_listener {
                        sess.pasv_data = Some(handle);
                        return Ok(handle);
                    }
                }
                Event::TcpData { handle, data } if handle == sess.ctrl => {
                    sess.rx.extend_from_slice(data.as_slice());
                }
                Event::Closed { handle } if handle == sess.ctrl => {
                    return Err(());
                }
                Event::Closed { handle } if Some(handle) == sess.pasv_listener => {
                    sess.pasv_listener = None;
                    sess.pasv_port = None;
                }
                Event::Error { .. } => return Err(()),
                _ => {}
            }
        }

        if Instant::now() >= deadline {
            return Err(());
        }
        Timer::after(Duration::from_millis(1)).await;
    }
}

async fn ftp_receive_data(
    vnet: &mut VNet,
    sess: &mut FtpServerSession,
    data_handle: NetHandle,
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<Vec<u8>, ()> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
    let mut out = Vec::new();

    loop {
        while let Some(ev) = vnet.pop_event() {
            match ev {
                Event::TcpData { handle, data } if handle == data_handle => {
                    let room = max_bytes.saturating_sub(out.len());
                    if room == 0 {
                        return Err(());
                    }
                    let src = data.as_slice();
                    let take = src.len().min(room);
                    out.extend_from_slice(&src[..take]);
                    if take < src.len() {
                        return Err(());
                    }
                }
                Event::Closed { handle } if handle == data_handle => {
                    sess.pasv_data = None;
                    return Ok(out);
                }
                Event::TcpData { handle, data } if handle == sess.ctrl => {
                    sess.rx.extend_from_slice(data.as_slice());
                }
                Event::Closed { handle } if handle == sess.ctrl => {
                    return Err(());
                }
                Event::Error { .. } => return Err(()),
                _ => {}
            }
        }

        if Instant::now() >= deadline {
            return Err(());
        }
        Timer::after(Duration::from_millis(1)).await;
    }
}

fn ftp_cwd_rel(cwd: &str) -> String {
    let mut out = String::from(cwd.trim());
    while out.starts_with('/') {
        out.remove(0);
    }
    while out.ends_with('/') {
        out.pop();
    }
    out
}

fn ftp_normalize_path(cwd: &str, arg: &str) -> Option<String> {
    let raw = arg.trim();
    let full = if raw.is_empty() {
        String::from(cwd)
    } else if raw.starts_with('/') {
        String::from(raw)
    } else if cwd == "/" {
        format!("/{}", raw)
    } else {
        format!("{}/{}", cwd, raw)
    };

    let mut out = String::new();
    for part in full.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return None;
        }
        if !out.is_empty() {
            out.push('/');
        }
        out.push_str(part);
    }
    Some(out)
}

fn ftp_listing_from_names(list: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for name in list.lines() {
        if name.is_empty() {
            continue;
        }
        let line = format!("-rw-r--r-- 1 trueos trueos 0 Jan 01 00:00 {}\r\n", name);
        out.extend_from_slice(line.as_bytes());
    }
    out
}
