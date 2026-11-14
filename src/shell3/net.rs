//! Port 22: SSH terminals; non-SSH connections receive a greeting and FIN.
//! The listener routes socket events; event-driven AP tasks own each SSH model.
use super::tty::Terminal;
use crate::allports::services::SHELL3_TCP_PORT;
use crate::net::adapter::{
    NetCommand, NetEvent, NetHandle, NetQueue, SocketKind, register_app_queues,
};
use alloc::vec::Vec;
use core::{future::Future, task::Poll};
use trueos_time::{Duration, Instant, Timer};

const MAX_CONNECTIONS: usize = super::MAX_SHELL3_INSTANCES;
const WRITE_TIMEOUT_MS: u64 = 30_000;
const PROTOCOL_PROBE_MS: u64 = 1_000;
const SSH_PREFIX: &[u8] = b"SSH-";
const GREETING: &[u8] = b"hello from TrueOS\r\n";

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
    greeting: bool,
    greeting_sent: bool,
    in_flight: usize,
    deadline: Option<Instant>,
    finishing: bool,
    command_blocked: bool,
}

impl Connection {
    fn new(handle: NetHandle, slot: u32, peer_port: Option<u16>, now: Instant) -> Self {
        Self {
            handle,
            terminal: None,
            ssh: None,
            pending: Some(PendingTerminal {slot, peer_port, until: now + Duration::from_millis(PROTOCOL_PROBE_MS), prefix: Vec::new()}),
            rejected: false,
            greeting: false,
            greeting_sent: false,
            in_flight: 0,
            deadline: None,
            finishing: false,
            command_blocked: false,
        }
    }

    fn reject_ssh(&mut self, reason: &str) {
        self.rejected = true;
        crate::log_info!(target: "service";
            "shell3-tcp: protocol=ssh handle={:?} peer_port={:?} action=reject reason={} plaintext=0\n",
            self.handle, self.pending.as_ref().and_then(|pending| pending.peer_port), reason,
        );
    }

    fn input(&mut self, bytes: &[u8]) {
        if self.rejected || self.greeting { return; }
        if let Some(ssh) = self.ssh.as_mut() {
            ssh.input(bytes);
            return;
        }
        for (index, &byte) in bytes.iter().enumerate() {
            let pending = self.pending.as_mut().unwrap();
            pending.prefix.push(byte);
            if !SSH_PREFIX.starts_with(&pending.prefix) {
                self.greeting = true;
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
        self.command_blocked = false;
        if !self.rejected && self.ssh.is_none() && self.pending.as_ref().is_some_and(|pending| now >= pending.until) {
            self.greeting = true;
        }
        if let Some(ssh) = self.ssh.as_mut() {
            ssh.pump(self.terminal.as_mut());
            if self.terminal.is_none() && ssh.wants_shell() {
                let pending = self.pending.take().unwrap();
                let (columns, rows) = ssh.size().expect("SSH shell requires accepted PTY dimensions");
                self.terminal = Some(Terminal::new_ssh(super::Shell3::new_terminal_sized_reserved(pending.slot, pending.peer_port, columns, rows), ssh.access_account()));
                crate::log_info!(target: "service";
                    "shell3-tcp: protocol=ssh handle={:?} action=shell-authenticated plaintext=0\n", self.handle,
                );
                // The initial terminal paint is ready even if the peer's
                // shell request did not ask for an SSH success reply.
                ssh.pump(self.terminal.as_mut());
            }
        }
        let force_close = self.rejected || self.terminal.as_ref().is_some_and(|tty| tty.overflow)
            || self.ssh.as_ref().is_some_and(|ssh| ssh.closed)
            || self.deadline.is_some_and(|deadline| now >= deadline);
        if force_close {
            // Closing is now pending queue capacity. An expired deadline must
            // not repeatedly wake us while the command queue remains full.
            self.rejected = true;
            self.deadline = None;
            self.command_blocked = commands
                .push(NetCommand::Close {
                    handle: self.handle,
                })
                .is_err();
            return self.command_blocked;
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
                } else {
                    self.command_blocked = true;
                }
            } else if ssh.finished {
                if commands.push(NetCommand::FinishTcp { handle: self.handle }).is_ok() {
                    self.finishing = true;
                    self.deadline = Some(now + Duration::from_millis(5_000));
                } else { self.command_blocked = true; }
            }
            return true;
        }
        if self.greeting {
            if !self.greeting_sent {
                if commands.push(NetCommand::SendTcp { handle: self.handle, data: GREETING.to_vec() }).is_ok() {
                    self.greeting_sent = true;
                    self.in_flight = GREETING.len();
                    self.deadline = Some(now + Duration::from_millis(WRITE_TIMEOUT_MS));
                } else {
                    self.command_blocked = true;
                }
            } else {
                if commands.push(NetCommand::FinishTcp { handle: self.handle }).is_ok() {
                    self.finishing = true;
                    self.deadline = Some(now + Duration::from_millis(5_000));
                } else { self.command_blocked = true; }
            }
        }
        true
    }

    fn next_deadline(&self) -> Option<Instant> {
        if self.rejected { return None; }
        let protocol = (!self.greeting && self.ssh.is_none())
            .then(|| self.pending.as_ref().map(|pending| pending.until)).flatten();
        [self.deadline, protocol, self.ssh.as_ref().and_then(|ssh| ssh.next_deadline()),
            self.terminal.as_ref().and_then(Terminal::next_deadline)]
            .into_iter().flatten().min()
    }

    fn needs_progress(&self) -> bool {
        !self.rejected && self.ssh.as_ref().is_some_and(|ssh| ssh.needs_progress())
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
static OWNER_WAKE: [crate::wait::WaitQueue; crate::percpu::CPU_SLOT_LIMIT] =
    [const { crate::wait::WaitQueue::new() }; crate::percpu::CPU_SLOT_LIMIT];
static ROUTER_WAKE: crate::wait::WaitQueue = crate::wait::WaitQueue::new();

pub(crate) fn notify_owner(slot: u32) {
    if let Some(wake) = OWNER_WAKE.get(slot as usize) { wake.notify_all(); }
}

/// Matrix output and shared shell state can affect any resident SSH terminal.
pub(crate) fn notify_work() {
    let slots: Vec<u32> = WORKER_QUEUES.lock().iter().map(|(slot, _)| *slot).collect();
    for slot in slots { notify_owner(slot); }
}

fn queue_for(slot: u32) -> Option<&'static NetQueue<WorkerEvent>> {
    WORKER_QUEUES
        .lock()
        .iter()
        .find(|(s, _)| *s == slot)
        .map(|(_, queue)| *queue)
}

/// Owned by the SSH readiness task on the same AP as its UI shell owner.
pub(super) struct WorkerTerminals {
    slot: u32,
    events: &'static NetQueue<WorkerEvent>,
    connections: Vec<Connection>,
}

impl WorkerTerminals {
    pub fn new(slot: u32) -> Self {
        let events = NetQueue::new_leaked("shell3-ap-events", 256);
        WORKER_QUEUES.lock().push((slot, events));
        ROUTER_WAKE.notify_all();
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

    fn next_deadline(&self) -> Option<Instant> {
        self.connections.iter().filter_map(Connection::next_deadline).min()
    }

    fn needs_progress(&self) -> bool {
        self.has_events() || self.connections.iter().any(Connection::needs_progress)
    }

    fn commands_blocked(&self) -> bool {
        self.connections.iter().any(|connection| connection.command_blocked)
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

async fn wait_until(deadline: Option<Instant>) {
    if let Some(deadline) = deadline { Timer::at(deadline).await; }
    else { core::future::pending::<()>().await; }
}

/// Yield after a bounded pass without introducing another timer period.
async fn yield_ready() {
    let mut yielded = false;
    core::future::poll_fn(|cx| {
        if yielded { Poll::Ready(()) } else {
            yielded = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }).await;
}

#[trueos_executor::task(pool_size = crate::percpu::CPU_SLOT_LIMIT)]
pub(super) async fn owner_task(slot: u32) {
    if crate::percpu::current_slot() as u32 != slot { return; }
    let mut terminals = WorkerTerminals::new(slot);
    let wake = &OWNER_WAKE[slot as usize];
    let mut clock = crate::chronos::signals::subscribe(crate::chronos::signals::Every::MINUTE);
    crate::log_info!(target: "service";
        "shell3-ssh: owner online slot={} scheduling=queue-readiness maintenance=civil-minute+protocol+presentation-deadlines\n", slot);
    loop {
        // Observe before processing/checking state. A publication anywhere
        // before registration then prevents sleeping through that work.
        let observed = wake.observe();
        let events_observed = terminals.events.change_generation();
        let commands = COMMANDS.get().copied();
        let commands_observed = commands.map(|queue| queue.change_generation()).unwrap_or(0);
        terminals.poll();
        if terminals.needs_progress() { yield_ready().await; continue; }
        let commands_blocked = terminals.commands_blocked();

        let mut owner = core::pin::pin!(wake.wait_after(observed));
        let mut events = core::pin::pin!(terminals.events.wait_change_after(events_observed));
        let mut capacity = core::pin::pin!(async {
            if let Some(commands) = commands.filter(|_| commands_blocked) {
                commands.wait_change_after(commands_observed).await;
            }
            else { core::future::pending::<()>().await; }
        });
        let mut deadline = core::pin::pin!(wait_until(terminals.next_deadline()));
        let mut clock_tick = core::pin::pin!(clock.next());
        core::future::poll_fn(|cx| {
            if owner.as_mut().poll(cx).is_ready() || events.as_mut().poll(cx).is_ready()
                || capacity.as_mut().poll(cx).is_ready() || deadline.as_mut().poll(cx).is_ready()
                || clock_tick.as_mut().poll(cx).is_ready()
            { Poll::Ready(()) } else { Poll::Pending }
        }).await;
    }
}

#[trueos_executor::task]
pub async fn terminal_task() {
    crate::log_info!(target: "service";
        "shell3-ssh: policy=development host-key=embedded-stable auth=account-proof anonymous=unregistered storage=independent\n");
    super::ssh::init();
    let spawner = unsafe { trueos_executor::Spawner::for_current_executor().await };
    match super::ssh::auth_task() {
        Ok(task) => spawner.spawn(task),
        Err(_) => crate::log_warn!(target: "service"; "shell3-ssh: authentication worker unavailable\n"),
    }
    let commands = NetQueue::new_leaked("shell3-tcp-cmd", 128);
    let events = NetQueue::new_leaked("shell3-tcp-evt", 256);
    register_app_queues("shell3-tcp", commands, events);
    COMMANDS.call_once(|| commands);
    notify_work();
    let mut listener = None;
    let mut opening = false;
    let mut retry_at = Instant::now();
    // Only transport routing lives here. Shell3 and Terminal never do.
    let mut routes: Vec<(NetHandle, u32)> = Vec::new();
    let mut deferred: Option<(u32, WorkerEvent, u32)> = None;
    let mut rejected = None;
    let mut errors = 0u64;

    static PASS: crate::r::input_timing::Site = crate::r::input_timing::Site::new("ssh-routing-pass");
    loop {
        let router_observed = ROUTER_WAKE.observe();
        let events_observed = events.change_generation();
        let commands_observed = commands.change_generation();
        let pass_started = crate::r::input_timing::now_ms();
        let mut commands_blocked = false;
        if let Some(handle) = rejected {
            if commands.push(NetCommand::Close { handle }).is_ok() {
                rejected = None;
            } else {
                commands_blocked = true;
            }
        }
        if let Some((slot, event, _)) = deferred.take() {
            if let Some(queue) = queue_for(slot) {
                let observed = queue.change_generation();
                if let Err(event) = queue.try_push(event) {
                    deferred = Some((slot, event, observed));
                } else {
                    notify_owner(slot);
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
                        "shell3-tcp: listening port={} sessions={} plaintext=0 nc=greeting-close\n",
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
                                        let observed = queue.change_generation();
                                        if let Err(event) =
                                            queue.try_push(WorkerEvent::Socket(event))
                                        {
                                            deferred = Some((slot, event, observed));
                                        }
                                    }
                                    notify_owner(slot);
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
                            let observed = queue.change_generation();
                            if let Err(event) = queue.try_push(WorkerEvent::Socket(event)) {
                                deferred = Some((slot, event, observed));
                            }
                            notify_owner(slot);
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
            commands_blocked |= !opening;
        }
        PASS.record(crate::r::input_timing::now_ms().saturating_sub(pass_started));
        let retry_deadline = (listener.is_none() && !opening && routes.len() < MAX_CONNECTIONS
            && !WORKER_QUEUES.lock().is_empty() && Instant::now() < retry_at).then_some(retry_at);
        let mut incoming = core::pin::pin!(events.wait_change_after(events_observed));
        let mut registration = core::pin::pin!(ROUTER_WAKE.wait_after(router_observed));
        let mut capacity = core::pin::pin!(async {
            if commands_blocked { commands.wait_change_after(commands_observed).await; }
            else { core::future::pending::<()>().await; }
        });
        let mut worker_capacity = core::pin::pin!(async {
            if let Some((slot, _, observed)) = &deferred {
                if let Some(queue) = queue_for(*slot) { queue.wait_change_after(*observed).await; return; }
            }
            core::future::pending::<()>().await;
        });
        let mut retry = core::pin::pin!(wait_until(retry_deadline));
        core::future::poll_fn(|cx| {
            if incoming.as_mut().poll(cx).is_ready() || registration.as_mut().poll(cx).is_ready()
                || capacity.as_mut().poll(cx).is_ready()
                || worker_capacity.as_mut().poll(cx).is_ready() || retry.as_mut().poll(cx).is_ready()
            { Poll::Ready(()) } else { Poll::Pending }
        }).await;
    }
}
