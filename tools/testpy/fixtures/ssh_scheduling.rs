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
impl Instant {
    pub fn now() -> Self { Self(NOW.load(Ordering::SeqCst)) }
    pub const fn from_nanos(nanos: u64) -> Self { Self(nanos / 1_000_000) }
}
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
static TIMERS: spin::Mutex<Vec<(Instant, Waker)>> = spin::Mutex::new(Vec::new());
impl Future for Timer {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if Instant::now() >= self.0 { Poll::Ready(()) } else {
            TIMERS.lock().push((self.0, cx.waker().clone()));
            Poll::Pending
        }
    }
}
fn advance_time(ms: u64) {
    NOW.store(ms, Ordering::SeqCst);
    let mut timers = TIMERS.lock();
    let mut ready = Vec::new();
    timers.retain(|(at, waker)| {
        if *at <= Instant(ms) { ready.push(waker.clone()); false } else { true }
    });
    drop(timers);
    for waker in ready { waker.wake(); }
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
mod chronos {
    pub fn monotonic_nanos() -> u64 { crate::Instant::now().0 * 1_000_000 }
    pub mod signals {
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
    #[derive(Clone, PartialEq)]
    pub struct MetaFmtStr { pub text: String }
    impl MetaFmtStr { pub fn new(text: impl Into<String>) -> Self { Self { text: text.into() } } }
    #[path = "@TRUEOS_ROOT@/src/shell3/transition.rs"]
    mod transition;
    #[path = "@TRUEOS_ROOT@/src/shell3/command_latch.rs"]
    mod command_latch;
    #[derive(Default)]
    pub struct Shell3 {
        command_feedback: Option<command_latch::Feedback>,
        title_transition: transition::TokenSteps,
        aka_transition: transition::TokenSteps,
        vmx_active: bool,
    }
    pub static INITIAL_FEEDBACK: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
    pub static EARLY_APP_CLAIM: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
    impl Shell3 {
        pub fn new_terminal_sized_reserved(_: u32, _: Option<u16>, _: usize, _: usize) -> Self {
            let mut shell = Self::default();
            if INITIAL_FEEDBACK.load(core::sync::atomic::Ordering::SeqCst) {
                shell.command_feedback = Some(command_latch::Feedback::new("gpu", crate::chronos::monotonic_nanos(), false));
            }
            shell
        }
        fn active_vmx_app(&self) -> Option<()> { self.vmx_active.then_some(()) }
        @SHELL_PRESENTATION@
    }
    pub struct MatrixSlots;
    impl MatrixSlots {
        pub fn background_active_slots() -> Vec<String> { BACKGROUND.lock().clone() }
    }
    pub static BACKGROUND: spin::Mutex<Vec<String>> = spin::Mutex::new(Vec::new());
    pub mod tty {
        pub static RECONCILES: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
        pub static RELEASES: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
        pub struct View {
            pub lines: Option<Vec<()>>, pub terminal_active: bool, pub controls_sampled_at: u64,
        }
        pub struct Terminal {
            pub overflow: bool, pub closing: bool, pub shell: super::Shell3, pub view: View,
        }
        impl Terminal {
            pub fn new_ssh(shell: super::Shell3, _: Option<u64>) -> Self {
                Self { overflow: false, closing: false, shell,
                    view: View { lines: Some(Vec::new()), terminal_active: super::EARLY_APP_CLAIM.load(core::sync::atomic::Ordering::SeqCst), controls_sampled_at: crate::chronos::monotonic_nanos() } }
            }
            pub fn reconcile_matrix_selection(&mut self) {
                use core::sync::atomic::Ordering;
                RECONCILES.fetch_add(1, Ordering::SeqCst);
                let now = crate::chronos::monotonic_nanos();
                if self.shell.command_feedback.as_ref().is_some_and(|feedback| feedback.finished(now)) {
                    self.shell.command_feedback = None;
                    self.view.terminal_active = true;
                    RELEASES.fetch_add(1, Ordering::SeqCst);
                }
                self.view.controls_sampled_at = now;
            }
            @TERMINAL_DEADLINE@
        }
    }
    pub mod service {
        pub static RELEASED: spin::Mutex<Vec<u32>> = spin::Mutex::new(Vec::new());
        pub static WORKING: spin::Mutex<Vec<String>> = spin::Mutex::new(Vec::new());
        pub fn working_vmx_slots() -> Vec<String> { WORKING.lock().clone() }
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
            pub fn input(&mut self, bytes: &[u8]) { if bytes.starts_with(b"SSH-") { self.shell_requested = true; } }
            pub fn pump(&mut self, _: Option<&mut super::tty::Terminal>) { self.pumps += 1; }
            pub fn wants_shell(&self) -> bool { self.shell_requested }
            pub fn size(&self) -> Option<(usize, usize)> { Some((120, 40)) }
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
            use crate::{NOW, Instant, Ordering, Waker, poll, counting};

            fn reset() -> &'static NetQueue<NetCommand> {
                WORKER_QUEUES.lock().clear();
                NOW.store(0, core::sync::atomic::Ordering::SeqCst);
                crate::TIMERS.lock().clear();
                super::super::service::RELEASED.lock().clear();
                super::super::service::WORKING.lock().clear();
                super::super::BACKGROUND.lock().clear();
                super::super::INITIAL_FEEDBACK.store(false, core::sync::atomic::Ordering::SeqCst);
                super::super::EARLY_APP_CLAIM.store(false, core::sync::atomic::Ordering::SeqCst);
                super::super::tty::RECONCILES.store(0, core::sync::atomic::Ordering::SeqCst);
                super::super::tty::RELEASES.store(0, core::sync::atomic::Ordering::SeqCst);
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
            fn animation_deadlines_include_final_frame_and_feedback_completion() {
                use super::super::{command_latch::Feedback, transition::TokenSteps, MetaFmtStr};
                reset();
                let mut transition = TokenSteps::default();
                assert_eq!(transition.next_deadline(0), None);
                transition.start(&[MetaFmtStr::new("gpu")], &[], 75_000_000, false);
                assert_eq!(transition.next_deadline(0), Some(100_000_000), "future start must not repaint an unchanged frame");
                assert_eq!(transition.next_deadline(124_999_999), Some(125_000_000));
                assert_eq!(transition.next_deadline(125_000_000), Some(150_000_000), "the final blank frame needs its own deadline");
                assert_eq!(transition.next_deadline(150_000_000), None);

                let feedback = Feedback::new("gpu", 0, false);
                assert_eq!(feedback.next_deadline(0), Some(200_000_000));
                assert_eq!(feedback.next_deadline(200_000_000), Some(250_000_000));
                assert!(!feedback.finished(299_999_999));
                assert_eq!(feedback.next_deadline(299_999_999), Some(300_000_000));
                assert!(feedback.finished(300_000_000));
                assert_eq!(feedback.next_deadline(350_000_000), Some(350_000_000), "completed but unreconciled handoff must remain runnable");
            }

            #[test]
            fn visible_busy_markers_use_100ms_boundaries_but_idle_apps_have_no_timer() {
                use super::super::{service, MetaFmtStr, Shell3, tty::Terminal};
                reset();
                let mut terminal = Terminal::new_ssh(Shell3::default(), Some(7));
                assert_eq!(terminal.next_deadline(), None, "a stable idle shell has no presentation timer");
                terminal.shell.title_transition.start(&[MetaFmtStr::new("CMD")], &[MetaFmtStr::new("System")], 0, false);
                assert_eq!(terminal.next_deadline(), Some(Instant(25)));
                terminal.shell.vmx_active = true;
                assert_eq!(terminal.next_deadline(), None, "VMX app controls suppress the hidden mode legend transition");
                terminal.shell.aka_transition.start(&[MetaFmtStr::new("old")], &[MetaFmtStr::new("new")], 0, false);
                assert_eq!(terminal.next_deadline(), Some(Instant(25)));
                terminal.shell.title_transition.finish();
                terminal.shell.aka_transition.finish();
                terminal.shell.vmx_active = false;
                service::WORKING.lock().push("edit".into());
                assert_eq!(terminal.next_deadline(), Some(Instant(100)));
                terminal.view.controls_sampled_at = 199_000_000;
                assert_eq!(terminal.next_deadline(), Some(Instant(200)));
                service::WORKING.lock().clear();
                super::super::BACKGROUND.lock().push("edit".into());
                assert_eq!(terminal.next_deadline(), Some(Instant(200)));
                terminal.view.terminal_active = true;
                assert_eq!(terminal.next_deadline(), None, "an app owning the displayed surface must not inherit hidden shell animation polling");
                terminal.view.terminal_active = false;
                terminal.closing = true;
                assert_eq!(terminal.next_deadline(), None);
                terminal.closing = false;
                terminal.view.lines = None;
                assert_eq!(terminal.next_deadline(), None);
            }

            #[test]
            fn presentation_deadline_keeps_last_render_sample_even_if_clock_passes_final_step() {
                use super::super::{command_latch::Feedback, MetaFmtStr, Shell3, tty::Terminal};
                reset();
                let mut shell = Shell3::default();
                shell.command_feedback = Some(Feedback::new("gpu", 0, false));
                let mut terminal = Terminal::new_ssh(shell, Some(7));
                terminal.view.controls_sampled_at = 250_000_000;
                terminal.view.terminal_active = true;
                NOW.store(350, Ordering::SeqCst);
                assert_eq!(terminal.next_deadline(), Some(Instant(300)), "sample from the rendered frame keeps its now-expired completion deadline; an early app claim cannot cancel the hold timer");
                terminal.shell.command_feedback = None;
                terminal.view.terminal_active = false;
                terminal.shell.title_transition.start(&[MetaFmtStr::new("gpu")], &[], 0, false);
                terminal.view.controls_sampled_at = 50_000_000;
                assert_eq!(terminal.next_deadline(), Some(Instant(75)), "a legend transition also retains its last blank-frame deadline after completion");
            }

            #[test]
            fn owner_releases_timed_handoff_without_input_or_transport_close() {
                use super::super::{INITIAL_FEEDBACK, EARLY_APP_CLAIM, tty::{RECONCILES, RELEASES}};
                reset();
                INITIAL_FEEDBACK.store(true, Ordering::SeqCst);
                EARLY_APP_CLAIM.store(true, Ordering::SeqCst);
                let (count, waker) = counting();
                let mut task = std::pin::pin!(owner_task(2));
                assert!(poll(task.as_mut(), &waker).is_pending());
                let queue = queue_for(2).unwrap();
                queue.push(WorkerEvent::Accepted(NetHandle(91), None)).ok().unwrap();
                queue.push(WorkerEvent::Socket(NetEvent::TcpData { handle: NetHandle(91), data: b"SSH-test".to_vec() })).ok().unwrap();
                assert!(poll(task.as_mut(), &waker).is_pending());
                assert!(queue.is_empty());
                let reconciles = RECONCILES.load(Ordering::SeqCst);
                let wakes = count.0.load(Ordering::SeqCst);
                crate::advance_time(199);
                assert_eq!(count.0.load(Ordering::SeqCst), wakes);
                assert!(poll(task.as_mut(), &waker).is_pending());
                assert_eq!(RECONCILES.load(Ordering::SeqCst), reconciles);
                crate::advance_time(200);
                assert!(count.0.load(Ordering::SeqCst) > wakes, "presentation deadline must wake the production owner without packets");
                assert!(poll(task.as_mut(), &waker).is_pending());
                assert!(RECONCILES.load(Ordering::SeqCst) > reconciles);
                assert_eq!(RELEASES.load(Ordering::SeqCst), 0);
                crate::advance_time(250);
                assert!(poll(task.as_mut(), &waker).is_pending());
                assert_eq!(RELEASES.load(Ordering::SeqCst), 0);
                crate::advance_time(300);
                assert!(poll(task.as_mut(), &waker).is_pending());
                assert_eq!(RELEASES.load(Ordering::SeqCst), 1, "real feedback completion must release navigation without a key");
                assert!(queue.is_empty());
                assert!(COMMANDS.get().unwrap().is_empty(), "presentation expiry must not be treated as a transport-close timeout");
                let reconciles = RECONCILES.load(Ordering::SeqCst);
                let wakes = count.0.load(Ordering::SeqCst);
                crate::advance_time(1_000);
                assert!(poll(task.as_mut(), &waker).is_pending());
                assert_eq!(RECONCILES.load(Ordering::SeqCst), reconciles, "after the handoff, the active app has no shell timer");
                assert_eq!(count.0.load(Ordering::SeqCst), wakes);
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
