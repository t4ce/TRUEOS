#![allow(dead_code, unused_imports)]
extern crate alloc;
extern crate self as trueos_time;
extern crate self as trueos_executor;
use core::{future::Future, pin::Pin, task::{Context, Poll}};
use std::{sync::{Arc, atomic::{AtomicU64, AtomicUsize, Ordering}}, task::{Wake, Waker}};

@WAIT@

static NOW: AtomicU64 = AtomicU64::new(0);
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Instant(u64);
impl Instant { pub fn now() -> Self { Self(NOW.load(Ordering::SeqCst)) } }
#[derive(Clone, Copy)]
pub struct Duration(u64);
impl Duration {
    pub const fn from_millis(value: u64) -> Self { Self(value) }
}
impl core::ops::Add<Duration> for Instant {
    type Output = Instant;
    fn add(self, duration: Duration) -> Instant { Instant(self.0 + duration.0) }
}
pub struct Timer(Instant);
impl Timer { pub fn at(at: Instant) -> Self { Self(at) } }
impl Future for Timer {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
        if Instant::now() >= self.0 { Poll::Ready(()) } else { Poll::Pending }
    }
}
pub struct Spawner;
impl Spawner {
    pub async unsafe fn for_current_executor() -> Self { Self }
    pub fn spawn(&self, _: ()) {}
}
#[macro_export]
macro_rules! log_info { (target:$target:literal;$($args:tt)*) => {{ let _ = format_args!($($args)*); }} }
#[macro_export]
macro_rules! log_warn { (target:$target:literal;$($args:tt)*) => {{ let _ = format_args!($($args)*); }} }
mod percpu {
    pub const CPU_SLOT_LIMIT: usize = 8;
    pub static SLOT: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(2);
    pub fn current_slot() -> usize { SLOT.load(core::sync::atomic::Ordering::SeqCst) }
}
mod chronos { pub mod signals {
    pub struct Every;
    impl Every { pub const MINUTE: Self = Self; }
    pub struct Subscription;
    pub fn subscribe(_: Every) -> Subscription { Subscription }
    impl Subscription { pub async fn next(&mut self) { core::future::pending::<()>().await; } }
} }
mod allports { pub mod services { pub const SHELL3_TCP_PORT: u16 = 22; } }
mod r { pub mod input_timing {
    pub struct Site;
    impl Site { pub const fn new(_: &str) -> Self { Self } pub fn record(&self, _: u64) {} }
    pub fn now_ms() -> u64 { crate::Instant::now().0 }
} }
mod net { pub mod adapter {
    use alloc::{boxed::Box, collections::VecDeque, vec::Vec};
    use core::sync::atomic::{AtomicU32, Ordering};
    #[derive(Clone, Copy, Debug, Eq, PartialEq)] pub struct NetHandle(pub u32);
    #[derive(Clone, Copy, Debug)] pub enum SocketKind { Tcp, Udp }
    #[derive(Clone, Copy, Debug)] pub struct Endpoint { pub port: u16 }
    #[derive(Debug)] pub enum NetCommand {
        OpenTcpListen { port: u16 }, SendTcp { handle: NetHandle, data: Vec<u8> },
        FinishTcp { handle: NetHandle }, Close { handle: NetHandle },
    }
    #[derive(Debug)] pub enum NetEvent {
        Opened { handle: NetHandle, kind: SocketKind },
        TcpEstablished { handle: NetHandle, peer: Option<Endpoint>, peer6: Option<Endpoint> },
        TcpData { handle: NetHandle, data: Vec<u8> }, TcpSent { handle: NetHandle, len: usize },
        Closed { handle: NetHandle }, Error { msg: &'static str },
    }
    @QUEUE@
    pub static REGISTRATION: spin::Mutex<Option<(&'static NetQueue<NetCommand>, &'static NetQueue<NetEvent>)>> = spin::Mutex::new(None);
    pub fn register_app_queues(_: &'static str, commands: &'static NetQueue<NetCommand>, events: &'static NetQueue<NetEvent>) {
        *REGISTRATION.lock() = Some((commands, events));
    }
} }
mod shell3 {
    pub const MAX_SHELL3_INSTANCES: usize = 8;
    pub struct Shell3;
    impl Shell3 { pub fn new_terminal_sized_reserved(_: u32, _: Option<u16>, _: usize, _: usize) -> Self { Self } }
    pub mod tty {
        pub struct Terminal { pub overflow: bool }
        impl Terminal {
            pub fn new_ssh(_: super::Shell3, _: Option<u64>) -> Self { Self { overflow: false } }
            pub fn reconcile_matrix_selection(&mut self) {}
        }
    }
    pub mod service {
        pub static RELEASED: spin::Mutex<Vec<u32>> = spin::Mutex::new(Vec::new());
        pub fn release_shell_on_executor(slot: u32) { RELEASED.lock().push(slot); }
        pub fn reserve_terminal_slot() -> Result<u32, ()> { Ok(2) }
    }
    pub mod ssh {
        pub struct Rdp(bool);
        impl Rdp { fn initialized(&self) -> bool { self.0 } }
        pub struct Session {
            pub closed: bool, pub finished: bool, pub runnable: bool,
            pub shell_requested: bool, rdp: Option<Rdp>, deadline: crate::Instant,
            pub encoded: Vec<u8>, pub chunk_limit: usize, pub pumps: usize,
        }
        impl Session {
            pub fn new() -> Result<Self, ()> {
                Ok(Self { closed: false, finished: false, runnable: false,
                    shell_requested: false, rdp: None, deadline: crate::Instant::now() + crate::Duration::from_millis(60_000),
                    encoded: Vec::new(), chunk_limit: 35_000, pumps: 0 })
            }
            pub fn established(bytes: &[u8]) -> Self {
                let mut session = Self::new().unwrap();
                session.rdp = Some(Rdp(true)); session.encoded = bytes.to_vec(); session
            }
            pub fn input(&mut self, _: &[u8]) {}
            pub fn pump(&mut self, _: Option<&mut super::tty::Terminal>) { self.pumps += 1; }
            pub fn wants_shell(&self) -> bool { false }
            pub fn size(&self) -> Option<(usize, usize)> { None }
            pub fn access_account(&self) -> Option<u64> { Some(7) }
            pub fn output(&mut self) -> &[u8] { &self.encoded[..self.encoded.len().min(self.chunk_limit)] }
            pub fn consume_output(&mut self, count: usize) { self.encoded.drain(..count); }
            @SSH_READINESS@
        }
        pub fn init() {}
        pub fn auth_task() -> Result<(), ()> { Ok(()) }
    }
    pub mod net {
        @TRANSPORT@

        #[cfg(test)] mod tests {
            use super::*;
            use crate::{NOW, Instant, Waker, poll, counting};

            fn reset() -> &'static NetQueue<NetCommand> {
                WORKER_QUEUES.lock().clear();
                NOW.store(0, core::sync::atomic::Ordering::SeqCst);
                super::super::service::RELEASED.lock().clear();
                *crate::net::adapter::REGISTRATION.lock() = None;
                crate::percpu::SLOT.store(2, core::sync::atomic::Ordering::SeqCst);
                let commands = *COMMANDS.call_once(|| NetQueue::new_leaked("test-ssh-commands", 1));
                commands.drain(usize::MAX);
                commands
            }
            fn established(handle: u32, output: &[u8]) -> Connection {
                let mut connection = Connection::new(NetHandle(handle), 2, Some(1234), Instant::now());
                connection.ssh = Some(super::super::ssh::Session::established(output));
                connection
            }
            fn sent(queue: &NetQueue<NetCommand>, expected_handle: u32, expected: &[u8]) {
                match queue.pop().expect("missing send command") {
                    NetCommand::SendTcp { handle, data } => { assert_eq!(handle, NetHandle(expected_handle)); assert_eq!(data, expected); }
                    other => panic!("expected SendTcp, got {other:?}"),
                }
            }

            #[test]
            fn tcp_ack_advances_35000_byte_chunks_without_time_or_ui_polling() {
                let commands = reset();
                let bytes: Vec<u8> = (0..70_123).map(|n| n as u8).collect();
                let mut owner = WorkerTerminals::new(2);
                owner.connections.push(established(41, &bytes));
                owner.poll(); sent(commands, 41, &bytes[..35_000]);
                assert_eq!(owner.connections[0].in_flight, 35_000);
                owner.events.push(WorkerEvent::Socket(NetEvent::TcpSent { handle: NetHandle(41), len: 20_000 })).ok().unwrap();
                owner.poll(); assert!(commands.is_empty());
                assert_eq!(owner.connections[0].in_flight, 15_000);
                owner.events.push(WorkerEvent::Socket(NetEvent::TcpSent { handle: NetHandle(41), len: 15_000 })).ok().unwrap();
                owner.poll(); sent(commands, 41, &bytes[35_000..70_000]);
                owner.events.push(WorkerEvent::Socket(NetEvent::TcpSent { handle: NetHandle(41), len: 35_000 })).ok().unwrap();
                owner.poll(); sent(commands, 41, &bytes[70_000..]);
                assert_eq!(Instant::now(), Instant(0));
                assert!(!owner.needs_progress());
            }

            #[test]
            fn full_command_queue_retains_output_until_capacity_notification() {
                let commands = reset();
                commands.push(NetCommand::Close { handle: NetHandle(99) }).unwrap();
                let mut owner = WorkerTerminals::new(2);
                owner.connections.push(established(42, b"encoded frame"));
                owner.poll();
                assert!(owner.commands_blocked()); assert!(!owner.needs_progress());
                assert_eq!(owner.connections[0].ssh.as_ref().unwrap().encoded, b"encoded frame");
                assert_eq!(owner.connections[0].in_flight, 0); assert_eq!(owner.next_deadline(), None);
                let observed = commands.change_generation();
                let mut capacity = std::pin::pin!(commands.wait_change_after(observed));
                assert!(poll(capacity.as_mut(), Waker::noop()).is_pending());
                commands.pop(); assert!(poll(capacity.as_mut(), Waker::noop()).is_ready());
                owner.poll(); sent(commands, 42, b"encoded frame"); assert!(!owner.commands_blocked());
            }

            #[test]
            fn finish_and_expired_close_survive_full_command_queue() {
                let commands = reset();
                let mut connection = established(43, b"");
                connection.ssh.as_mut().unwrap().finished = true;
                commands.push(NetCommand::Close { handle: NetHandle(99) }).unwrap();
                assert!(connection.flush(commands, Instant(0)));
                assert!(!connection.finishing); assert!(connection.command_blocked);
                assert_eq!(connection.next_deadline(), None);
                commands.pop(); assert!(connection.flush(commands, Instant(0)));
                assert!(matches!(commands.pop(), Some(NetCommand::FinishTcp { handle: NetHandle(43) })));
                assert!(connection.finishing); assert_eq!(connection.next_deadline(), Some(Instant(5_000)));
                commands.push(NetCommand::Close { handle: NetHandle(99) }).unwrap();
                assert!(connection.flush(commands, Instant(5_000)));
                assert!(connection.rejected); assert!(connection.command_blocked);
                assert_eq!(connection.next_deadline(), None);
                assert!(!connection.needs_progress());
                commands.pop(); assert!(!connection.flush(commands, Instant(5_001)));
                assert!(matches!(commands.pop(), Some(NetCommand::Close { handle: NetHandle(43) })));
            }

            #[test]
            fn protocol_and_auth_timeouts_do_not_become_periodic_wakeups() {
                let commands = reset();
                let mut connection = Connection::new(NetHandle(44), 2, None, Instant(0));
                assert_eq!(connection.next_deadline(), Some(Instant(1_000)));
                assert!(connection.flush(commands, Instant(999))); assert!(commands.is_empty());
                assert!(connection.flush(commands, Instant(1_000))); sent(commands, 44, GREETING);
                assert_eq!(connection.next_deadline(), Some(Instant(31_000)));
                let mut authenticating = established(45, b"");
                authenticating.ssh = Some(super::super::ssh::Session::new().unwrap());
                assert_eq!(authenticating.next_deadline(), Some(Instant(60_000)));
                authenticating.ssh.as_mut().unwrap().closed = true;
                assert!(!authenticating.flush(commands, Instant(60_000)));
                assert!(authenticating.rejected); assert_eq!(authenticating.next_deadline(), None);
            }

            #[test]
            fn owner_wait_does_not_self_wake_for_stalled_peer_or_unrelated_commands() {
                let commands = reset();
                let (count, waker) = counting();
                let mut task = std::pin::pin!(owner_task(2));
                assert!(poll(task.as_mut(), &waker).is_pending());
                let queue = queue_for(2).unwrap();
                queue.push(WorkerEvent::Accepted(NetHandle(46), None)).ok().unwrap();
                queue.push(WorkerEvent::Socket(NetEvent::TcpData { handle: NetHandle(46), data: b"not SSH".to_vec() })).ok().unwrap();
                assert!(poll(task.as_mut(), &waker).is_pending()); sent(commands, 46, GREETING);
                let before = count.0.load(core::sync::atomic::Ordering::SeqCst);
                for _ in 0..20 { assert!(poll(task.as_mut(), &waker).is_pending()); }
                assert_eq!(count.0.load(core::sync::atomic::Ordering::SeqCst), before);
                commands.push(NetCommand::Close { handle: NetHandle(99) }).unwrap(); commands.pop();
                assert_eq!(count.0.load(core::sync::atomic::Ordering::SeqCst), before, "in-flight owners must not subscribe to unrelated command traffic");
                NOW.store(30_000, core::sync::atomic::Ordering::SeqCst);
                assert!(poll(task.as_mut(), &waker).is_pending());
                assert!(matches!(commands.pop(), Some(NetCommand::Close { handle: NetHandle(46) })));
                assert_eq!(*super::super::service::RELEASED.lock(), vec![2]);
            }

            #[test]
            fn exact_owner_wake_and_media_output_need_no_new_input_packet() {
                let commands = reset();
                let mut owner = WorkerTerminals::new(2);
                owner.connections.push(established(47, b""));
                owner.poll(); assert!(!owner.needs_progress()); assert_eq!(owner.next_deadline(), None);
                let first = OWNER_WAKE[2].observe(); let other = OWNER_WAKE[3].observe();
                let mut first_wait = std::pin::pin!(OWNER_WAKE[2].wait_after(first));
                let mut other_wait = std::pin::pin!(OWNER_WAKE[3].wait_after(other));
                assert!(poll(first_wait.as_mut(), Waker::noop()).is_pending());
                assert!(poll(other_wait.as_mut(), Waker::noop()).is_pending());
                owner.connections[0].ssh.as_mut().unwrap().encoded.extend_from_slice(b"video without input");
                notify_owner(2);
                assert!(poll(first_wait.as_mut(), Waker::noop()).is_ready());
                assert!(poll(other_wait.as_mut(), Waker::noop()).is_pending());
                assert!(owner.events.is_empty()); owner.poll(); sent(commands, 47, b"video without input");
            }

            #[test]
            fn owner_budget_continues_only_when_ssh_reports_runnable_work() {
                reset();
                let mut owner = WorkerTerminals::new(2);
                let mut connection = established(48, b"");
                connection.ssh.as_mut().unwrap().runnable = true;
                owner.connections.push(connection); assert!(owner.needs_progress());
                let (count, waker) = counting();
                let mut yielded = std::pin::pin!(yield_ready());
                assert!(poll(yielded.as_mut(), &waker).is_pending());
                assert_eq!(count.0.load(core::sync::atomic::Ordering::SeqCst), 1);
                assert!(poll(yielded.as_mut(), &waker).is_ready());
                owner.connections[0].ssh.as_mut().unwrap().runnable = false;
                assert!(!owner.needs_progress());
                owner.connections[0].ssh.as_mut().unwrap().closed = true;
                owner.connections[0].ssh.as_mut().unwrap().runnable = true;
                assert!(!owner.needs_progress());
            }

            #[test]
            fn event_budget_yields_once_then_drains_without_a_timer() {
                reset();
                let (count, waker) = counting();
                let mut task = std::pin::pin!(owner_task(2));
                assert!(poll(task.as_mut(), &waker).is_pending());
                let events = queue_for(2).unwrap();
                for _ in 0..65 {
                    events.push(WorkerEvent::Socket(NetEvent::TcpSent { handle: NetHandle(999), len: 1 })).ok().unwrap();
                }
                let before = count.0.load(core::sync::atomic::Ordering::SeqCst);
                assert!(poll(task.as_mut(), &waker).is_pending());
                assert!(!events.is_empty(), "the fairness limit should leave the final event for another turn");
                assert_eq!(count.0.load(core::sync::atomic::Ordering::SeqCst), before + 1);
                assert!(poll(task.as_mut(), &waker).is_pending());
                assert!(events.is_empty()); assert_eq!(Instant::now(), Instant(0));
            }

            #[test]
            fn router_defers_closed_event_on_full_exact_owner_queue() {
                reset();
                let worker = WorkerTerminals::new(2);
                let unrelated = WorkerTerminals::new(3);
                let mut router = std::pin::pin!(terminal_task());
                assert!(poll(router.as_mut(), Waker::noop()).is_pending());
                let (commands, events) = crate::net::adapter::REGISTRATION.lock().unwrap();
                assert!(matches!(commands.pop(), Some(NetCommand::OpenTcpListen { port: 22 })));
                events.push(NetEvent::Opened { handle: NetHandle(51), kind: SocketKind::Tcp }).unwrap();
                events.push(NetEvent::TcpEstablished { handle: NetHandle(51), peer: None, peer6: None }).unwrap();
                assert!(poll(router.as_mut(), Waker::noop()).is_pending());
                assert!(matches!(worker.events.pop(), Some(WorkerEvent::Accepted(NetHandle(51), None))));
                assert!(unrelated.events.is_empty());
                commands.drain(usize::MAX);
                for _ in 0..256 { worker.events.push(WorkerEvent::Socket(NetEvent::TcpSent { handle: NetHandle(777), len: 1 })).ok().unwrap(); }
                events.push(NetEvent::Closed { handle: NetHandle(51) }).unwrap();
                let (count, waker) = counting();
                assert!(poll(router.as_mut(), &waker).is_pending());
                let before = count.0.load(core::sync::atomic::Ordering::SeqCst);
                for _ in 0..10 { assert!(poll(router.as_mut(), &waker).is_pending()); }
                assert_eq!(count.0.load(core::sync::atomic::Ordering::SeqCst), before);
                worker.events.pop();
                assert!(count.0.load(core::sync::atomic::Ordering::SeqCst) > before);
                assert!(poll(router.as_mut(), &waker).is_pending());
                let closed = worker.events.drain(256).into_iter().filter(|event| matches!(event, WorkerEvent::Socket(NetEvent::Closed { handle: NetHandle(51) }))).count();
                assert_eq!(closed, 1); assert!(unrelated.events.is_empty());
                assert!(events.is_empty());
            }
        }
    }
}

struct CountWake(AtomicUsize);
impl Wake for CountWake {
    fn wake(self: Arc<Self>) { self.0.fetch_add(1, Ordering::SeqCst); }
    fn wake_by_ref(self: &Arc<Self>) { self.0.fetch_add(1, Ordering::SeqCst); }
}
fn counting() -> (Arc<CountWake>, Waker) {
    let count = Arc::new(CountWake(AtomicUsize::new(0)));
    (count.clone(), Waker::from(count))
}
fn poll<F: Future>(future: Pin<&mut F>, waker: &Waker) -> Poll<F::Output> {
    future.poll(&mut Context::from_waker(waker))
}
