//! Shared port-22 listener for plaintext and authenticated kernel SSH terminals.
//! The listener routes socket events; AP pool tasks own and execute each model.
use super::tty::Terminal;
use crate::allports::services::SHELL3_TCP_PORT;
use crate::net::adapter::{
    NetCommand, NetEvent, NetHandle, NetQueue, SocketKind, register_app_queues,
};
use alloc::vec::Vec;
use trueos_time::{Duration, Instant, Timer};

const MAX_CONNECTIONS: usize = super::MAX_SHELL3_INSTANCES;
const WRITE_TIMEOUT_MS: u64 = 30_000;
const PROTOCOL_PROBE_MS: u64 = 1_000;
const SSH_PREFIX: &[u8] = b"SSH-";

struct PendingTerminal {
    slot: u32,
    peer_port: Option<u16>,
    until: Instant,
    prefix: Vec<u8>,
}

struct Connection {
    handle: NetHandle,
    terminal: Option<Terminal>,
    ssh: Option<super::ssh::Session>,
    pending: Option<PendingTerminal>,
    rejected: bool,
    in_flight: usize,
    deadline: Option<Instant>,
    finishing: bool,
}

impl Connection {
    fn new(handle: NetHandle, slot: u32, peer_port: Option<u16>, now: Instant) -> Self {
        Self {
            handle,
            terminal: None,
            ssh: None,
            pending: Some(PendingTerminal {slot, peer_port, until: now + Duration::from_millis(PROTOCOL_PROBE_MS), prefix: Vec::new()}),
            rejected: false,
            in_flight: 0,
            deadline: None,
            finishing: false,
        }
    }

    fn open_plaintext(&mut self) {
        let pending = self.pending.take().unwrap();
        let shell = super::Shell3::new_terminal_reserved(pending.slot, pending.peer_port);
        let mut terminal = Terminal::new(shell);
        terminal.input(&pending.prefix);
        self.terminal = Some(terminal);
    }

    fn reject_ssh(&mut self, reason: &str) {
        self.rejected = true;
        crate::log_info!(target: "service";
            "shell3-tcp: protocol=ssh handle={:?} peer_port={:?} action=reject reason={} plaintext=0\n",
            self.handle, self.pending.as_ref().and_then(|pending| pending.peer_port), reason,
        );
    }

    fn input(&mut self, bytes: &[u8]) {
        if self.rejected { return; }
        if let Some(ssh) = self.ssh.as_mut() {
            ssh.input(bytes);
            return;
        }
        if let Some(terminal) = self.terminal.as_mut() {
            terminal.input(bytes);
            return;
        }
        for (index, &byte) in bytes.iter().enumerate() {
            let pending = self.pending.as_mut().unwrap();
            pending.prefix.push(byte);
            if !SSH_PREFIX.starts_with(&pending.prefix) {
                self.open_plaintext();
                self.terminal.as_mut().unwrap().input(&bytes[index + 1..]);
                return;
            }
            if pending.prefix.len() == SSH_PREFIX.len() {
                match super::ssh::Session::new() {
                    Ok(mut ssh) => {
                        ssh.input(&pending.prefix);
                        ssh.input(&bytes[index + 1..]);
                        self.ssh = Some(ssh);
                        crate::log_info!(target: "service";
                            "shell3-tcp: protocol=ssh handle={:?} action=authenticate plaintext=0\n", self.handle,
                        );
                    }
                    Err(_) => self.reject_ssh("cry-credential-unavailable"),
                }
                return;
            }
        }
    }

    /// False means an immediate close was admitted and the model can be freed.
    fn flush(&mut self, commands: &NetQueue<NetCommand>, now: Instant) -> bool {
        if !self.rejected && self.ssh.is_none() && self.pending.as_ref().is_some_and(|pending| now >= pending.until) {
            if self.pending.as_ref().unwrap().prefix.is_empty() {
                self.open_plaintext();
            } else {
                // A partial SSH prefix must never be replayed as Shell3 input.
                self.reject_ssh("incomplete-identification");
            }
        }
        if let Some(ssh) = self.ssh.as_mut() {
            ssh.pump(self.terminal.as_mut());
            if self.terminal.is_none() && ssh.wants_shell() {
                let pending = self.pending.take().unwrap();
                self.terminal = Some(Terminal::new(super::Shell3::new_terminal_reserved(pending.slot, pending.peer_port)));
                crate::log_info!(target: "service";
                    "shell3-tcp: protocol=ssh handle={:?} action=shell-authenticated plaintext=0\n", self.handle,
                );
            }
        }
        let force_close = self.rejected || self.terminal.as_ref().is_some_and(|tty| tty.overflow)
            || self.ssh.as_ref().is_some_and(|ssh| ssh.closed)
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
        if let Some(ssh) = self.ssh.as_mut() {
            let bytes = ssh.output();
            if !bytes.is_empty() {
                let data = bytes.to_vec();
                let len = data.len();
                if commands.push(NetCommand::SendTcp { handle: self.handle, data }).is_ok() {
                    ssh.consume_output(len);
                    self.in_flight = len;
                    self.deadline = Some(now + Duration::from_millis(WRITE_TIMEOUT_MS));
                }
            } else if ssh.finished
                && commands.push(NetCommand::FinishTcp { handle: self.handle }).is_ok() {
                self.finishing = true;
                self.deadline = Some(now + Duration::from_millis(5_000));
            }
            return true;
        }
        let Some(tty) = self.terminal.as_mut() else { return true; };
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

impl Drop for Connection {
    fn drop(&mut self) {
        // Before plaintext starts, the admission has no Shell3 owner to free it.
        if let Some(pending) = self.pending.take() {
            super::service::release_shell_on_executor(pending.slot);
        }
    }
}

enum WorkerEvent {
    Accepted(NetHandle, Option<u16>),
    Socket(NetEvent),
}

static COMMANDS: spin::Once<&'static NetQueue<NetCommand>> = spin::Once::new();
static WORKER_QUEUES: spin::Mutex<Vec<(u32, &'static NetQueue<WorkerEvent>)>> =
    spin::Mutex::new(Vec::new());

fn queue_for(slot: u32) -> Option<&'static NetQueue<WorkerEvent>> {
    WORKER_QUEUES
        .lock()
        .iter()
        .find(|(s, _)| *s == slot)
        .map(|(_, queue)| *queue)
}

/// Lives in shell_worker_task, alongside the UI shells on this AP.
pub(super) struct WorkerTerminals {
    slot: u32,
    events: &'static NetQueue<WorkerEvent>,
    connections: Vec<Connection>,
}

impl WorkerTerminals {
    pub fn new(slot: u32) -> Self {
        let events = NetQueue::new_leaked("shell3-ap-events", 256);
        WORKER_QUEUES.lock().push((slot, events));
        Self {
            slot,
            events,
            connections: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.connections.is_empty()
    }
    pub fn has_events(&self) -> bool {
        !self.events.is_empty()
    }

    pub fn poll(&mut self) {
        let Some(commands) = COMMANDS.get() else {
            return;
        };
        for event in self.events.drain(64) {
            match event {
                WorkerEvent::Accepted(handle, peer_port) => {
                    self.connections.push(Connection::new(handle, self.slot, peer_port, Instant::now()));
                }
                WorkerEvent::Socket(NetEvent::TcpData { handle, data }) => {
                    if let Some(connection) =
                        self.connections.iter_mut().find(|c| c.handle == handle)
                    {
                        connection.input(&data);
                    }
                }
                WorkerEvent::Socket(NetEvent::TcpSent { handle, len }) => {
                    if let Some(connection) =
                        self.connections.iter_mut().find(|c| c.handle == handle)
                    {
                        connection.in_flight = connection.in_flight.saturating_sub(len);
                        if connection.in_flight == 0 && !connection.finishing {
                            connection.deadline = None;
                        }
                    }
                }
                WorkerEvent::Socket(NetEvent::Closed { handle }) => {
                    self.connections.retain(|c| c.handle != handle);
                }
                _ => {}
            }
        }
        for connection in &mut self.connections {
            if let Some(tty) = connection.terminal.as_mut() {
                tty.reconcile_matrix_selection();
            }
        }
        let now = Instant::now();
        self.connections
            .retain_mut(|connection| connection.flush(commands, now));
    }
}

#[trueos_executor::task]
pub async fn terminal_task() {
    super::ssh::init();
    let spawner = unsafe { trueos_executor::Spawner::for_current_executor().await };
    if let Ok(task) = super::ssh::auth_task() { spawner.spawn(task); }
    let commands = NetQueue::new_leaked("shell3-tcp-cmd", 128);
    let events = NetQueue::new_leaked("shell3-tcp-evt", 256);
    register_app_queues("shell3-tcp", commands, events);
    COMMANDS.call_once(|| commands);
    let mut listener = None;
    let mut opening = false;
    let mut retry_at = Instant::now();
    // Only transport routing lives here. Shell3 and Terminal never do.
    let mut routes: Vec<(NetHandle, u32)> = Vec::new();
    let mut deferred: Option<(u32, WorkerEvent)> = None;
    let mut rejected = None;
    let mut errors = 0u64;

    loop {
        if let Some(handle) = rejected {
            if commands.push(NetCommand::Close { handle }).is_ok() {
                rejected = None;
            }
        }
        if let Some((slot, event)) = deferred.take() {
            if let Some(queue) = queue_for(slot) {
                if let Err(event) = queue.try_push(event) {
                    deferred = Some((slot, event));
                } else {
                    super::service::notify_work();
                }
            }
        }
        // Retain a full event rather than losing input/close acknowledgements
        // under backpressure. Drain one at a time until that event is admitted.
        while deferred.is_none() && rejected.is_none() {
            let Some(event) = events.drain(1).pop() else {
                break;
            };
            match event {
                NetEvent::Opened {
                    handle,
                    kind: SocketKind::Tcp,
                } => {
                    listener = Some(handle);
                    opening = false;
                    crate::log_info!(target: "service";
                        "shell3-tcp: listening port={} sessions={} plaintext=1\n",
                        SHELL3_TCP_PORT, routes.len(),
                    );
                }
                NetEvent::TcpEstablished { handle, .. } | NetEvent::TcpData { handle, .. }
                    if listener == Some(handle) =>
                {
                    listener = None;
                    let peer_port = match &event {
                        NetEvent::TcpEstablished { peer, peer6, .. } => peer
                            .as_ref().map(|endpoint| endpoint.port)
                            .or_else(|| peer6.as_ref().map(|endpoint| endpoint.port)),
                        _ => None,
                    };
                    match super::service::reserve_terminal_slot() {
                        Ok(slot) => {
                            if let Some(queue) = queue_for(slot) {
                                if queue.push(WorkerEvent::Accepted(handle, peer_port)).is_ok() {
                                    routes.push((handle, slot));
                                    // Established is informational; early data must follow
                                    // Accepted in FIFO order on the same permanent owner.
                                    if matches!(event, NetEvent::TcpData { .. }) {
                                        if let Err(event) =
                                            queue.try_push(WorkerEvent::Socket(event))
                                        {
                                            deferred = Some((slot, event));
                                        }
                                    }
                                    super::service::notify_work();
                                    continue;
                                }
                            }
                            super::service::release_shell_on_executor(slot);
                            rejected = Some(handle);
                        }
                        Err(_) => rejected = Some(handle),
                    }
                }
                NetEvent::TcpData { handle, .. }
                | NetEvent::TcpSent { handle, .. }
                | NetEvent::Closed { handle } => {
                    if listener == Some(handle) {
                        listener = None;
                    }
                    if let Some(index) = routes.iter().position(|(h, _)| *h == handle) {
                        let slot = routes[index].1;
                        if matches!(event, NetEvent::Closed { .. }) {
                            routes.remove(index);
                        }
                        if let Some(queue) = queue_for(slot) {
                            if let Err(event) = queue.try_push(WorkerEvent::Socket(event)) {
                                deferred = Some((slot, event));
                            }
                            super::service::notify_work();
                        }
                    }
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
        if listener.is_none()
            && !opening
            && routes.len() < MAX_CONNECTIONS
            && !WORKER_QUEUES.lock().is_empty()
            && Instant::now() >= retry_at
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
