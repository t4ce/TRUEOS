use std::collections::{HashMap, VecDeque};
use std::io;
#[cfg(unix)]
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, RawFd};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::{Interest, Token};

#[derive(Clone, Copy, Debug)]
pub struct Event {
    token: Token,
    readiness: Ready,
}

pub type Events = Vec<Event>;

#[derive(Clone, Copy, Debug, Default)]
struct Ready {
    readable: bool,
    writable: bool,
    error: bool,
    read_closed: bool,
    write_closed: bool,
    priority: bool,
    aio: bool,
    lio: bool,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
struct Registration {
    token: Token,
    interests: Interest,
}

#[derive(Debug, Default)]
struct SelectorState {
    registrations: Mutex<HashMap<usize, Registration>>,
    ready: Mutex<VecDeque<Event>>,
}

static NEXT_SELECTOR_ID: AtomicUsize = AtomicUsize::new(1);

#[derive(Debug)]
pub struct Selector {
    id: usize,
    state: Arc<SelectorState>,
}

impl Selector {
    pub fn new() -> io::Result<Selector> {
        Ok(Selector {
            id: NEXT_SELECTOR_ID.fetch_add(1, Ordering::Relaxed),
            state: Arc::new(SelectorState::default()),
        })
    }

    pub fn try_clone(&self) -> io::Result<Selector> {
        Ok(Selector {
            id: self.id,
            state: Arc::clone(&self.state),
        })
    }

    pub fn select(&self, events: &mut Events, timeout: Option<Duration>) -> io::Result<()> {
        events.clear();

        if self.drain_ready(events) {
            return Ok(());
        }

        if self.drain_socket_ready(events) {
            return Ok(());
        }

        if matches!(timeout, Some(duration) if duration.is_zero()) {
            return Ok(());
        }

        let deadline = timeout.map(|duration| {
            crate::zkvm_compat::now_nanos()
                .saturating_add(crate::zkvm_compat::duration_to_nanos(duration))
        });

        loop {
            crate::zkvm_compat::poll_once();
            if self.drain_ready(events) {
                return Ok(());
            }

            if self.drain_socket_ready(events) {
                return Ok(());
            }

            if let Some(deadline) = deadline {
                if crate::zkvm_compat::now_nanos() >= deadline {
                    return Ok(());
                }
            }
        }
    }

    pub(crate) fn register_source(
        &self,
        source_id: usize,
        token: Token,
        interests: Interest,
    ) -> io::Result<()> {
        let mut registrations = self.state.registrations.lock().unwrap();
        registrations.insert(source_id, Registration { token, interests });
        Ok(())
    }

    pub(crate) fn reregister_source(
        &self,
        source_id: usize,
        token: Token,
        interests: Interest,
    ) -> io::Result<()> {
        self.register_source(source_id, token, interests)
    }

    pub(crate) fn deregister_source(&self, source_id: usize) -> io::Result<()> {
        let mut registrations = self.state.registrations.lock().unwrap();
        registrations.remove(&source_id);
        Ok(())
    }

    pub(crate) fn push_waker_event(&self, token: Token) -> io::Result<()> {
        let mut ready = self.state.ready.lock().unwrap();
        ready.push_back(Event {
            token,
            readiness: Ready {
                readable: true,
                ..Ready::default()
            },
        });
        Ok(())
    }

    fn drain_ready(&self, events: &mut Events) -> bool {
        let mut ready = self.state.ready.lock().unwrap();
        while events.len() < events.capacity() {
            let Some(event) = ready.pop_front() else {
                break;
            };
            events.push(event);
        }
        !events.is_empty()
    }

    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    fn drain_socket_ready(&self, events: &mut Events) -> bool {
        let remaining = events.capacity().saturating_sub(events.len());
        if remaining == 0 {
            return !events.is_empty();
        }

        let mut raw_events = Vec::with_capacity(remaining);
        crate::zkvm_net::selector_poll(self.id, &mut raw_events, Some(Duration::ZERO));
        for raw in raw_events {
            if events.len() >= events.capacity() {
                break;
            }
            events.push(Event {
                token: Token(raw.token),
                readiness: Ready::from_zkvm_bits(raw.readiness),
            });
        }

        !events.is_empty()
    }

    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    fn drain_socket_ready(&self, events: &mut Events) -> bool {
        !events.is_empty()
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
impl Ready {
    fn from_zkvm_bits(bits: u8) -> Self {
        Self {
            readable: (bits & crate::zkvm_net::READY_READABLE) != 0,
            writable: (bits & crate::zkvm_net::READY_WRITABLE) != 0,
            error: (bits & crate::zkvm_net::READY_ERROR) != 0,
            read_closed: (bits & crate::zkvm_net::READY_READ_CLOSED) != 0,
            write_closed: (bits & crate::zkvm_net::READY_WRITE_CLOSED) != 0,
            ..Self::default()
        }
    }
}

#[cfg(all(unix, not(any(target_os = "trueos", target_os = "zkvm"))))]
cfg_os_ext! {
    impl Selector {
        pub fn register(&self, _: RawFd, _: Token, _: Interest) -> io::Result<()> {
            unsupported_io!("mio zkvm selector registration backend is not wired yet");
        }

        pub fn reregister(&self, _: RawFd, _: Token, _: Interest) -> io::Result<()> {
            unsupported_io!("mio zkvm selector reregistration backend is not wired yet");
        }

        pub fn deregister(&self, _: RawFd) -> io::Result<()> {
            unsupported_io!("mio zkvm selector deregistration backend is not wired yet");
        }
    }
}

#[cfg(target_os = "wasi")]
cfg_any_os_ext! {
    use crate::{Interest, Token};

    impl Selector {
        pub fn register(&self, _: wasi::Fd, _: Token, _: Interest) -> io::Result<()> {
            unsupported_io!("mio zkvm selector registration backend is not wired yet");
        }

        pub fn reregister(&self, _: wasi::Fd, _: Token, _: Interest) -> io::Result<()> {
            unsupported_io!("mio zkvm selector reregistration backend is not wired yet");
        }

        pub fn deregister(&self, _: wasi::Fd) -> io::Result<()> {
            unsupported_io!("mio zkvm selector deregistration backend is not wired yet");
        }
    }
}

cfg_io_source! {
    #[cfg(any(debug_assertions, any(target_os = "trueos", target_os = "zkvm")))]
    impl Selector {
        pub fn id(&self) -> usize {
            self.id
        }
    }
}

#[cfg(unix)]
impl AsFd for Selector {
    fn as_fd(&self) -> BorrowedFd<'_> {
        os_required!()
    }
}

#[cfg(unix)]
impl AsRawFd for Selector {
    fn as_raw_fd(&self) -> RawFd {
        os_required!()
    }
}

#[allow(clippy::trivially_copy_pass_by_ref)]
pub mod event {
    use crate::sys::Event;
    use crate::Token;
    use std::fmt;

    pub fn token(event: &Event) -> Token {
        event.token
    }

    pub fn is_readable(event: &Event) -> bool {
        event.readiness.readable
    }

    pub fn is_writable(event: &Event) -> bool {
        event.readiness.writable
    }

    pub fn is_error(event: &Event) -> bool {
        event.readiness.error
    }

    pub fn is_read_closed(event: &Event) -> bool {
        event.readiness.read_closed
    }

    pub fn is_write_closed(event: &Event) -> bool {
        event.readiness.write_closed
    }

    pub fn is_priority(event: &Event) -> bool {
        event.readiness.priority
    }

    pub fn is_aio(event: &Event) -> bool {
        event.readiness.aio
    }

    pub fn is_lio(event: &Event) -> bool {
        event.readiness.lio
    }

    pub fn debug_details(f: &mut fmt::Formatter<'_>, event: &Event) -> fmt::Result {
        write!(
            f,
            "zkvm(token={:?}, readable={}, writable={}, error={}, read_closed={}, write_closed={}, priority={}, aio={}, lio={})",
            event.token,
            event.readiness.readable,
            event.readiness.writable,
            event.readiness.error,
            event.readiness.read_closed,
            event.readiness.write_closed,
            event.readiness.priority,
            event.readiness.aio,
            event.readiness.lio,
        )
    }
}
