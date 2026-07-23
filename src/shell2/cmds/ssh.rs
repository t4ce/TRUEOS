//! Shell2's `ssh` UX over the existing raw TCP terminal protocol.
//!
//! Authentication and encryption belong to a later transport layer; this
//! command owns only endpoint parsing and the terminal handoff lifecycle.

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_executor::{Spawner, task};
use embassy_time::{Duration, Instant, Timer};
use v::vnet as api;

use super::super::shell2_cmd::ParseOutcome;
use super::super::{
    MatrixTarget, ShellBackend2, TerminalHandoffOwner, matrix_target_for_backend,
    print_matrix_target_system_line, print_shell_line, set_matrix_target_active,
};
use crate::r::net::VNet;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const IO_POLL_INTERVAL: Duration = Duration::from_millis(2);
const LOCAL_READ_BYTES: usize = 4096;
const PENDING_REMOTE_BYTES: usize = 64 * 1024;

static SSH_SESSION_ID: AtomicU32 = AtomicU32::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SshEndpoint {
    addr: [u8; 4],
    port: u16,
}

impl SshEndpoint {
    fn text(self) -> String {
        alloc::format!(
            "{}.{}.{}.{}:{}",
            self.addr[0],
            self.addr[1],
            self.addr[2],
            self.addr[3],
            self.port
        )
    }
}

fn parse_endpoint(text: &str) -> Option<SshEndpoint> {
    let (addr_text, port_text) = text.rsplit_once(':')?;
    let port = port_text.parse::<u16>().ok().filter(|port| *port != 0)?;
    let mut addr = [0u8; 4];
    let mut octets = addr_text.split('.');
    for octet in &mut addr {
        *octet = octets.next()?.parse::<u8>().ok()?;
    }
    if octets.next().is_some() {
        return None;
    }
    Some(SshEndpoint { addr, port })
}

fn next_handoff_owner() -> TerminalHandoffOwner {
    let id = SSH_SESSION_ID.fetch_add(1, Ordering::Relaxed).max(1);
    TerminalHandoffOwner::stream(id)
}

struct MatrixActivity {
    target: MatrixTarget,
    active: bool,
}

impl MatrixActivity {
    fn begin(target: MatrixTarget) -> Self {
        set_matrix_target_active(&target, true);
        Self {
            target,
            active: true,
        }
    }

    fn finish(&mut self) {
        if self.active {
            set_matrix_target_active(&self.target, false);
            self.active = false;
        }
    }
}

impl Drop for MatrixActivity {
    fn drop(&mut self) {
        self.finish();
    }
}

struct TerminalHandoff {
    io: &'static dyn ShellBackend2,
    owner: TerminalHandoffOwner,
    active: bool,
}

impl TerminalHandoff {
    fn claim(io: &'static dyn ShellBackend2, owner: TerminalHandoffOwner) -> Option<Self> {
        io.claim_terminal_handoff(owner).then_some(Self {
            io,
            owner,
            active: true,
        })
    }

    fn release(&mut self) {
        if self.active {
            self.io.release_terminal_handoff(self.owner);
            self.active = false;
        }
    }
}

impl Drop for TerminalHandoff {
    fn drop(&mut self) {
        self.release();
    }
}

#[derive(Default)]
struct LocalEscape {
    at_line_start: bool,
    pending_tilde: bool,
}

impl LocalEscape {
    fn new() -> Self {
        Self {
            at_line_start: true,
            pending_tilde: false,
        }
    }

    /// Apply the OpenSSH-style local `~.` escape at the start of a line.
    ///
    /// `~~` sends one literal tilde, which keeps the transport usable for
    /// shells and full-screen applications that need a leading `~`.
    fn forward(&mut self, input: &[u8], output: &mut Vec<u8>) -> bool {
        for &byte in input {
            if self.pending_tilde {
                self.pending_tilde = false;
                match byte {
                    b'.' => return true,
                    b'~' => {
                        output.push(b'~');
                        self.at_line_start = false;
                        continue;
                    }
                    _ => {
                        output.push(b'~');
                        self.at_line_start = false;
                    }
                }
            } else if self.at_line_start && byte == b'~' {
                self.pending_tilde = true;
                continue;
            }

            output.push(byte);
            self.at_line_start = matches!(byte, b'\r' | b'\n');
        }
        false
    }
}

fn record_failure(target: &MatrixTarget, endpoint: SshEndpoint, reason: &str) {
    print_matrix_target_system_line(
        target,
        alloc::format!("ssh: {}: {}", endpoint.text(), reason).as_str(),
    );
}

#[task(pool_size = 2)]
async fn ssh_session(io: &'static dyn ShellBackend2, target: MatrixTarget, endpoint: SshEndpoint) {
    let mut activity = MatrixActivity::begin(target.clone());
    print_matrix_target_system_line(
        &target,
        alloc::format!("ssh: connecting {}", endpoint.text()).as_str(),
    );

    let Some(net) = VNet::open_primary() else {
        record_failure(&target, endpoint, "no network device");
        return;
    };
    if net
        .submit(api::Command::OpenTcpConnect {
            remote: api::EndpointV4::new(endpoint.addr, endpoint.port),
        })
        .is_err()
    {
        record_failure(&target, endpoint, "connect request failed");
        return;
    }

    let deadline = Instant::now() + CONNECT_TIMEOUT;
    let mut opened = None;
    let mut established = None;
    let mut pending_remote = Vec::new();
    while Instant::now() < deadline && established.is_none() {
        while let Some(event) = net.pop_event() {
            match event {
                api::Event::Opened { handle, kind } if kind == api::SocketKind::Tcp => {
                    opened = Some(handle);
                }
                api::Event::TcpEstablished { handle, .. }
                    if opened.is_none() || opened == Some(handle) =>
                {
                    opened = Some(handle);
                    established = Some(handle);
                }
                api::Event::TcpData { handle, data }
                    if opened.is_none() || opened == Some(handle) =>
                {
                    let remaining = PENDING_REMOTE_BYTES.saturating_sub(pending_remote.len());
                    pending_remote.extend_from_slice(&data.as_slice()[..data.len().min(remaining)]);
                }
                api::Event::Closed { handle } if opened.is_none() || opened == Some(handle) => {
                    record_failure(&target, endpoint, "connection closed");
                    return;
                }
                api::Event::Error { msg } => {
                    if let Some(handle) = opened {
                        let _ = net.submit(api::Command::Close { handle });
                    }
                    record_failure(&target, endpoint, msg);
                    return;
                }
                _ => {}
            }
        }
        if established.is_none() {
            Timer::after(IO_POLL_INTERVAL).await;
        }
    }

    let Some(handle) = established else {
        if let Some(handle) = opened {
            let _ = net.submit(api::Command::Close { handle });
        }
        record_failure(&target, endpoint, "connect timeout");
        return;
    };

    let owner = next_handoff_owner();
    let Some(mut handoff) = TerminalHandoff::claim(io, owner) else {
        let _ = net.submit(api::Command::Close { handle });
        record_failure(&target, endpoint, "terminal handoff busy or unsupported");
        return;
    };

    let connected = alloc::format!(
        "\r\nssh {} connected — local escape is ~. at the start of a line\r\n",
        endpoint.text()
    );
    let _ = io.terminal_handoff_write(owner, connected.as_bytes());
    if !pending_remote.is_empty() {
        let _ = io.terminal_handoff_write(owner, pending_remote.as_slice());
    }

    let mut local_input = [0u8; LOCAL_READ_BYTES];
    let mut forwarded = Vec::with_capacity(LOCAL_READ_BYTES);
    let mut escape = LocalEscape::new();
    let disconnect_reason = 'session: loop {
        let mut processed_events = 0usize;
        while processed_events < 64 {
            let Some(event) = net.pop_event() else {
                break;
            };
            processed_events += 1;
            match event {
                api::Event::TcpData {
                    handle: event_handle,
                    data,
                } if event_handle == handle => {
                    if !io.terminal_handoff_write(owner, data.as_slice()) {
                        break 'session "terminal handoff lost";
                    }
                }
                api::Event::Closed {
                    handle: event_handle,
                } if event_handle == handle => break 'session "remote closed",
                api::Event::Error { msg } => break 'session msg,
                _ => {}
            }
        }

        let read = io.terminal_handoff_read(owner, &mut local_input);
        if read != 0 {
            forwarded.clear();
            let local_disconnect = escape.forward(&local_input[..read], &mut forwarded);
            if !forwarded.is_empty() && net.send_tcp_all(handle, forwarded.as_slice()).is_err() {
                break 'session "send failed";
            }
            if local_disconnect {
                break 'session "local escape";
            }
        }

        Timer::after(IO_POLL_INTERVAL).await;
    };

    let _ = net.submit(api::Command::Close { handle });
    activity.finish();
    print_matrix_target_system_line(
        &target,
        alloc::format!("ssh: {} disconnected ({})", endpoint.text(), disconnect_reason).as_str(),
    );
    handoff.release();
}

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> ParseOutcome {
    let trimmed = rest.trim();
    if matches!(trimmed, "help" | "-h" | "--help") {
        print_shell_line(
            io,
            "ssh: usage `ssh <ipv4>:<port>`; leave with `~.` at the start of a line",
        );
        return ParseOutcome::Handled;
    }

    let mut args = trimmed.split_whitespace();
    let Some(endpoint_text) = args.next() else {
        print_shell_line(io, "ssh: usage `ssh <ipv4>:<port>`");
        return ParseOutcome::Handled;
    };
    let Some(endpoint) = parse_endpoint(endpoint_text) else {
        print_shell_line(io, "ssh: invalid endpoint; expected `<ipv4>:<port>`");
        return ParseOutcome::Handled;
    };
    if args.next().is_some() {
        print_shell_line(io, "ssh: usage `ssh <ipv4>:<port>`");
        return ParseOutcome::Handled;
    }
    if !io.supports_terminal_handoff() {
        print_shell_line(io, "ssh: this shell backend has no raw terminal handoff");
        return ParseOutcome::Handled;
    }

    match ssh_session(io, matrix_target_for_backend(io), endpoint) {
        Ok(token) => spawner.spawn(token),
        Err(_) => print_shell_line(io, "ssh: session task unavailable"),
    }
    ParseOutcome::Handled
}

#[cfg(test)]
mod tests {
    use super::{LocalEscape, SshEndpoint, parse_endpoint};
    use alloc::vec::Vec;

    #[test]
    fn parses_ipv4_and_port() {
        assert_eq!(
            parse_endpoint("192.168.178.94:4548"),
            Some(SshEndpoint {
                addr: [192, 168, 178, 94],
                port: 4548,
            })
        );
        assert_eq!(parse_endpoint("192.168.178.94"), None);
        assert_eq!(parse_endpoint("192.168.178.999:4548"), None);
        assert_eq!(parse_endpoint("192.168.178.94:0"), None);
    }

    #[test]
    fn disconnect_escape_only_applies_at_line_start() {
        let mut escape = LocalEscape::new();
        let mut output = Vec::new();
        assert!(!escape.forward(b"echo ~. stays\r", &mut output));
        assert_eq!(output, b"echo ~. stays\r");
        output.clear();
        assert!(escape.forward(b"~.", &mut output));
        assert!(output.is_empty());
    }

    #[test]
    fn doubled_tilde_forwards_one_literal_tilde() {
        let mut escape = LocalEscape::new();
        let mut output = Vec::new();
        assert!(!escape.forward(b"~~trueos\r", &mut output));
        assert_eq!(output, b"~trueos\r");
    }
}
