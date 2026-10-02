//! Plain TCP transport for Shell3 terminals; port 22 is reserved for future SSH.
//! One adapter handle and one model per connection, without UI4 presentation.
use super::tty::Terminal;
use crate::allports::services::SHELL3_TCP_PORT;
use crate::net::adapter::{
    NetCommand, NetEvent, NetHandle, NetQueue, SocketKind, register_app_queues,
};
use alloc::vec::Vec;
use trueos_time::{Duration, Instant, Timer};

const MAX_CONNECTIONS: usize = 16;
const WRITE_TIMEOUT_MS: u64 = 30_000;

struct Connection {
    handle: NetHandle,
    terminal: Option<Terminal>,
    in_flight: usize,
    deadline: Option<Instant>,
    finishing: bool,
}

impl Connection {
    fn new(handle: NetHandle) -> Self {
        Self {
            handle,
            terminal: super::Shell3::new_terminal().ok().map(Terminal::new),
            in_flight: 0,
            deadline: None,
            finishing: false,
        }
    }

    /// False means an immediate close was admitted and the model can be freed.
    fn flush(&mut self, commands: &NetQueue<NetCommand>, now: Instant) -> bool {
        let force_close = self.terminal.as_ref().is_none_or(|tty| tty.overflow)
            || self.deadline.is_some_and(|deadline| now >= deadline);
        if force_close {
            return commands
                .push(NetCommand::Close {
                    handle: self.handle,
                })
                .is_err();
        }
        if self.finishing || self.in_flight != 0 {
            return true;
        }
        let tty = self.terminal.as_mut().unwrap();
        if !tty.output.is_empty() {
            let data = core::mem::take(&mut tty.output);
            let len = data.len();
            match commands.try_push(NetCommand::SendTcp {
                handle: self.handle,
                data,
            }) {
                Ok(()) => {
                    self.in_flight = len;
                    self.deadline = Some(now + Duration::from_millis(WRITE_TIMEOUT_MS));
                }
                Err(NetCommand::SendTcp { data, .. }) => tty.output = data,
                Err(_) => unreachable!(),
            }
        } else if tty.closing
            && commands
                .push(NetCommand::FinishTcp {
                    handle: self.handle,
                })
                .is_ok()
        {
            self.finishing = true;
            self.deadline = Some(now + Duration::from_millis(5_000));
        }
        true
    }
}

#[trueos_executor::task]
pub async fn terminal_task() {
    let commands = NetQueue::new_leaked("shell3-tcp-cmd", 128);
    let events = NetQueue::new_leaked("shell3-tcp-evt", 256);
    register_app_queues("shell3-tcp", commands, events);
    super::service::refresh_appdb_names();
    let mut listener = None;
    let mut opening = false;
    let mut retry_at = Instant::now();
    let mut connections: Vec<Connection> = Vec::new();
    let mut errors = 0u64;

    loop {
        for event in events.drain(64) {
            match event {
                NetEvent::Opened {
                    handle,
                    kind: SocketKind::Tcp,
                } => {
                    listener = Some(handle);
                    opening = false;
                    crate::log_info!(target: "service";
                        "shell3-tcp: listening port={} sessions={} plaintext=1\n",
                        SHELL3_TCP_PORT, connections.len(),
                    );
                }
                NetEvent::TcpEstablished { handle, .. } | NetEvent::TcpData { handle, .. }
                    if listener == Some(handle) =>
                {
                    // The adapter can deliver data before Established. Accept
                    // either event exactly once and replenish the listener.
                    listener = None;
                    let mut connection = Connection::new(handle);
                    if let NetEvent::TcpData { data, .. } = event
                        && let Some(tty) = connection.terminal.as_mut()
                    {
                        tty.input(&data);
                    }
                    connections.push(connection);
                }
                NetEvent::TcpData { handle, data } => {
                    if let Some(connection) = connections.iter_mut().find(|c| c.handle == handle)
                        && let Some(tty) = connection.terminal.as_mut()
                    {
                        tty.input(&data);
                    }
                }
                NetEvent::TcpSent { handle, len } => {
                    if let Some(connection) = connections.iter_mut().find(|c| c.handle == handle) {
                        connection.in_flight = connection.in_flight.saturating_sub(len);
                        if connection.in_flight == 0 && !connection.finishing {
                            connection.deadline = None;
                        }
                    }
                }
                NetEvent::Closed { handle } => {
                    if listener == Some(handle) {
                        listener = None;
                    }
                    connections.retain(|connection| connection.handle != handle);
                }
                NetEvent::Error { msg } => {
                    errors = errors.saturating_add(1);
                    if errors <= 2 || errors.is_power_of_two() {
                        crate::log_warn!(target: "service"; "shell3-tcp: error={}\n", msg);
                    }
                    if opening {
                        opening = false;
                        retry_at = Instant::now() + Duration::from_millis(1_000);
                    }
                }
                _ => {}
            }
        }
        let now = Instant::now();
        connections.retain_mut(|connection| connection.flush(commands, now));
        if listener.is_none() && !opening && connections.len() < MAX_CONNECTIONS && now >= retry_at
        {
            opening = commands
                .push(NetCommand::OpenTcpListen {
                    port: SHELL3_TCP_PORT,
                })
                .is_ok();
        }
        Timer::after(Duration::from_millis(10)).await;
    }
}
