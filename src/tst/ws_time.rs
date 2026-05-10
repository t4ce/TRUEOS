extern crate alloc;

use alloc::{format, string::String, vec::Vec};

use embassy_time::{Duration, Timer};
use embedded_websocket::{
    WebSocketKey, WebSocketReceiveMessageType, WebSocketSendMessageType, WebSocketServer,
};
use v::vnet as api;

use crate::r::net::{VNet, ports};

const WS_TIME_PATH: &str = "/time";
const RX_BUF_MAX: usize = 8 * 1024;
const TX_BUF_MAX: usize = 1024;
const FRAME_BUF_MAX: usize = 512;

struct TimeSession {
    handle: api::NetHandle,
    ws: WebSocketServer,
    rx: Vec<u8>,
    open: bool,
    last_sent_minute: Option<u64>,
}

impl TimeSession {
    fn new(handle: api::NetHandle) -> Self {
        Self {
            handle,
            ws: WebSocketServer::new_server(),
            rx: Vec::new(),
            open: false,
            last_sent_minute: None,
        }
    }
}

fn find_http_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}

fn http_request_path(req: &str) -> Option<&str> {
    let line_end = req
        .find("\r\n")
        .or_else(|| req.find('\n'))
        .unwrap_or(req.len());
    let line = req.get(..line_end)?;
    let mut it = line.split_whitespace();
    let method = it.next()?;
    let path = it.next()?;
    if method != "GET" {
        return None;
    }
    Some(path)
}

fn http_header_value<'a>(req: &'a str, key: &str) -> Option<&'a str> {
    let mut lines = req.split('\n');
    let _ = lines.next()?;
    for line in lines {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':')
            && k.trim().eq_ignore_ascii_case(key)
        {
            return Some(v.trim());
        }
    }
    None
}

fn is_valid_ws_upgrade(req: &str) -> bool {
    http_header_value(req, "Upgrade")
        .map(|v| v.eq_ignore_ascii_case("websocket"))
        .unwrap_or(false)
        && http_header_value(req, "Connection")
            .map(|v| {
                v.split(',')
                    .any(|part| part.trim().eq_ignore_ascii_case("Upgrade"))
            })
            .unwrap_or(false)
}

fn current_unix_seconds_and_source() -> (u64, &'static str) {
    if let Some(ts) = crate::r::net::ntp::current_unix_seconds() {
        return (ts, "ntp");
    }
    if let Some(ts) = crate::time::unix_time_seconds() {
        return (ts, "boot");
    }
    (crate::time::uptime_seconds(), "uptime")
}

fn is_leap_year(year: u32) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

fn month_lengths(year: u32) -> [u8; 12] {
    if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    }
}

fn unix_timestamp_to_ymdhms(ts: u64) -> (u32, u8, u8, u8, u8, u8) {
    const SECS_PER_MIN: u64 = 60;
    const SECS_PER_HOUR: u64 = 60 * SECS_PER_MIN;
    const SECS_PER_DAY: u64 = 24 * SECS_PER_HOUR;

    let mut days = ts / SECS_PER_DAY;
    let mut rem = ts % SECS_PER_DAY;

    let hour = (rem / SECS_PER_HOUR) as u8;
    rem %= SECS_PER_HOUR;
    let minute = (rem / SECS_PER_MIN) as u8;
    let second = (rem % SECS_PER_MIN) as u8;

    let mut year: u32 = 1970;
    loop {
        let days_in_year = if is_leap_year(year) { 366u64 } else { 365u64 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    let month_lengths = month_lengths(year);
    let mut month_idx = 0;
    while month_idx < month_lengths.len() {
        let len = month_lengths[month_idx] as u64;
        if days < len {
            let day = (days + 1) as u8;
            return (year, (month_idx + 1) as u8, day, hour, minute, second);
        }
        days -= len;
        month_idx += 1;
    }

    (year, 12, 31, hour, minute, second)
}

fn time_message_json() -> String {
    let (unix, source) = current_unix_seconds_and_source();
    let (year, month, day, hour, minute, second) = unix_timestamp_to_ymdhms(unix);
    format!(
        "{{\"kind\":\"time\",\"unix\":{},\"source\":\"{}\",\"utc\":\"{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC\"}}",
        unix, source, year, month, day, hour, minute, second
    )
}

fn send_tcp_bytes(vnet: &VNet, handle: api::NetHandle, bytes: &[u8]) {
    for chunk in bytes.chunks(api::MAX_MSG) {
        let _ = vnet.submit(api::Command::SendTcp {
            handle,
            data: api::ByteBuf::from_slice_trunc(chunk),
        });
    }
}

fn send_ws_text(vnet: &VNet, session: &mut TimeSession, text: &str) -> Result<(), ()> {
    let mut frame_buf = [0u8; TX_BUF_MAX];
    let len = session
        .ws
        .write(WebSocketSendMessageType::Text, true, text.as_bytes(), &mut frame_buf)
        .map_err(|_| ())?;
    send_tcp_bytes(vnet, session.handle, &frame_buf[..len]);
    Ok(())
}

fn send_ws_reply(
    vnet: &VNet,
    session: &mut TimeSession,
    msg_type: WebSocketSendMessageType,
    payload: &[u8],
) -> Result<(), ()> {
    let mut frame_buf = [0u8; TX_BUF_MAX];
    let len = session
        .ws
        .write(msg_type, true, payload, &mut frame_buf)
        .map_err(|_| ())?;
    send_tcp_bytes(vnet, session.handle, &frame_buf[..len]);
    Ok(())
}

fn close_session(vnet: &VNet, handle: api::NetHandle) {
    let _ = vnet.submit(api::Command::Close { handle });
}

fn handle_ws_frames(vnet: &VNet, session: &mut TimeSession) -> bool {
    let mut payload = [0u8; FRAME_BUF_MAX];

    loop {
        let res = match session.ws.read(session.rx.as_slice(), &mut payload) {
            Ok(v) => v,
            Err(embedded_websocket::Error::ReadFrameIncomplete) => break,
            Err(e) => {
                crate::log_trace!("ws-time: frame decode failed {:?}\n", e);
                close_session(vnet, session.handle);
                return true;
            }
        };

        if res.len_from == 0 {
            break;
        }
        let remaining = session.rx.split_off(res.len_from);
        session.rx = remaining;

        match res.message_type {
            WebSocketReceiveMessageType::Ping => {
                let _ = send_ws_reply(
                    vnet,
                    session,
                    WebSocketSendMessageType::Pong,
                    &payload[..res.len_to],
                );
            }
            WebSocketReceiveMessageType::CloseMustReply => {
                let _ = send_ws_reply(
                    vnet,
                    session,
                    WebSocketSendMessageType::CloseReply,
                    &payload[..res.len_to],
                );
                close_session(vnet, session.handle);
                return true;
            }
            WebSocketReceiveMessageType::CloseCompleted => {
                close_session(vnet, session.handle);
                return true;
            }
            WebSocketReceiveMessageType::Text
            | WebSocketReceiveMessageType::Binary
            | WebSocketReceiveMessageType::Pong => {}
        }
    }

    false
}

fn try_open_websocket(vnet: &VNet, session: &mut TimeSession) -> bool {
    let Some(header_end) = find_http_header_end(session.rx.as_slice()) else {
        return false;
    };

    let Ok(req) = core::str::from_utf8(&session.rx[..header_end]) else {
        crate::log_trace!(
            "ws-time: handshake invalid utf8 handle={} header_bytes={}\n",
            session.handle.0,
            header_end
        );
        close_session(vnet, session.handle);
        return true;
    };

    let path = http_request_path(req);
    let key = http_header_value(req, "Sec-WebSocket-Key");
    let upgrade_ok = http_header_value(req, "Upgrade")
        .map(|v| v.eq_ignore_ascii_case("websocket"))
        .unwrap_or(false);
    let connection_ok = http_header_value(req, "Connection")
        .map(|v| {
            v.split(',')
                .any(|part| part.trim().eq_ignore_ascii_case("Upgrade"))
        })
        .unwrap_or(false);
    let ws_upgrade_ok = is_valid_ws_upgrade(req);

    if path != Some(WS_TIME_PATH) || !ws_upgrade_ok || key.is_none() {
        let line_end = req
            .find("\r\n")
            .or_else(|| req.find('\n'))
            .unwrap_or(req.len());
        let req_line = &req[..line_end];
        crate::log_trace!(
            "ws-time: handshake reject handle={} line='{}' path={:?} expect='{}' upgrade_ok={} connection_ok={} ws_upgrade_ok={} key_present={}\n",
            session.handle.0,
            req_line,
            path,
            WS_TIME_PATH,
            upgrade_ok,
            connection_ok,
            ws_upgrade_ok,
            key.is_some()
        );
        close_session(vnet, session.handle);
        return true;
    }

    let Ok(key) = WebSocketKey::try_from(key.unwrap_or("")) else {
        crate::log_trace!(
            "ws-time: handshake bad key handle={} key={:?}\n",
            session.handle.0,
            http_header_value(req, "Sec-WebSocket-Key")
        );
        close_session(vnet, session.handle);
        return true;
    };

    let mut response = [0u8; TX_BUF_MAX];
    let Ok(len) = session.ws.server_accept(&key, None, &mut response) else {
        crate::log_trace!("ws-time: server_accept failed handle={} path={:?}\n", session.handle.0, path);
        close_session(vnet, session.handle);
        return true;
    };
    send_tcp_bytes(vnet, session.handle, &response[..len]);

    let remaining = session.rx.split_off(header_end);
    session.rx = remaining;
    session.open = true;
    session.last_sent_minute = None;
    crate::log_trace!("ws-time: websocket opened handle={}\n", session.handle.0);
    false
}

#[embassy_executor::task]
pub async fn ws_time_task() {
    loop {
        let Some(vnet) = VNet::open_primary() else {
            Timer::after(Duration::from_millis(100)).await;
            continue;
        };

        let mut listener: Option<api::NetHandle> = None;
        let mut session: Option<TimeSession> = None;

        if vnet
            .submit(api::Command::OpenTcpListen {
                port: ports::WS_TIME_TCP_PORT,
            })
            .is_err()
        {
            Timer::after(Duration::from_millis(250)).await;
            continue;
        }

        crate::log_trace!(
            "ws-time: listening on tcp {} path={}\n",
            ports::WS_TIME_TCP_PORT,
            WS_TIME_PATH
        );

        loop {
            while let Some(ev) = vnet.pop_event() {
                match ev {
                    api::Event::Opened { handle, kind } => {
                        if kind == api::SocketKind::Tcp && listener.is_none() {
                            listener = Some(handle);
                        }
                    }
                    api::Event::TcpEstablished { handle, .. } => {
                        if session.as_ref().map(|s| s.handle) != Some(handle) {
                            session = Some(TimeSession::new(handle));
                            crate::log_trace!("ws-time: tcp established handle={}\n", handle.0);
                        }
                    }
                    api::Event::TcpData { handle, data } => {
                        if session.is_none() {
                            session = Some(TimeSession::new(handle));
                            crate::log_trace!(
                                "ws-time: late session bind on first rx handle={} bytes={}\n",
                                handle.0,
                                data.len()
                            );
                        }
                        let Some(active) = session.as_mut() else {
                            continue;
                        };
                        if active.handle != handle {
                            continue;
                        }
                        if active.rx.len().saturating_add(data.len()) > RX_BUF_MAX {
                            crate::log_trace!("ws-time: rx overflow handle={}\n", handle.0);
                            close_session(&vnet, handle);
                            continue;
                        }
                        active.rx.extend_from_slice(data.as_slice());

                        let closed = if !active.open {
                            try_open_websocket(&vnet, active)
                        } else {
                            handle_ws_frames(&vnet, active)
                        };
                        if closed {
                            session = None;
                        } else if let Some(active) = session.as_mut()
                            && active.open
                            && handle_ws_frames(&vnet, active)
                        {
                            session = None;
                        }
                    }
                    api::Event::Closed { handle } => {
                        if Some(handle) == listener {
                            listener = None;
                            session = None;
                            let _ = vnet.submit(api::Command::OpenTcpListen {
                                port: ports::WS_TIME_TCP_PORT,
                            });
                        } else if session.as_ref().map(|s| s.handle) == Some(handle) {
                            session = None;
                        }
                    }
                    api::Event::Error { msg } => {
                        if msg != "bad handle" {
                            crate::log_trace!("ws-time: error {}\n", msg);
                        }
                    }
                    api::Event::TcpSent { .. } => {}
                    api::Event::UdpPacket { .. } => {}
                    api::Event::UdpPacketV6 { .. } => {}
                    api::Event::IcmpReply { .. } => {}
                    api::Event::IcmpReplyV6 { .. } => {}
                }
            }

            if let Some(active) = session.as_mut()
                && active.open
            {
                let (unix, _source) = current_unix_seconds_and_source();
                let minute = unix / 60;
                if active.last_sent_minute != Some(minute) {
                    let payload = time_message_json();
                    if send_ws_text(&vnet, active, payload.as_str()).is_ok() {
                        active.last_sent_minute = Some(minute);
                    } else {
                        close_session(&vnet, active.handle);
                    }
                }
            }

            Timer::after(Duration::from_millis(100)).await;
        }
    }
}
