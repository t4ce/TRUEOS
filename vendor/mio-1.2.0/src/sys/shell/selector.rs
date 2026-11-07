use alloc::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Weak},
    vec::Vec,
};
#[cfg(feature = "os-poll")]
#[cfg(debug_assertions)]
use core::sync::atomic::{AtomicUsize, Ordering};
use core::time::Duration;
use spin::Mutex;
#[cfg(all(unix, not(all(any(target_os = "trueos", target_os = "zkvm"), not(feature = "std-compat")))))]
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, RawFd};
use trueos_io as io;

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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum RegistrationKey {
    #[cfg(any(feature = "net", all(unix, feature = "os-ext")))]
    Source(usize),
    Socket(u32),
}

#[derive(Clone, Copy, Debug)]
struct Registration {
    token: Token,
    interests: Interest,
    version: u64,
    delivered: u8,
}

#[derive(Debug, Default)]
struct Registrations {
    entries: BTreeMap<RegistrationKey, Registration>,
    cursor: Option<RegistrationKey>,
    version: u64,
}

impl Registrations {
    fn next_version(&mut self) -> io::Result<u64> {
        self.version = self.version.checked_add(1).ok_or(io::ErrorKind::Other)?;
        Ok(self.version)
    }

    fn next(&mut self) -> Option<(RegistrationKey, Registration)> {
        use core::ops::Bound::{Excluded, Unbounded};
        let next = self.cursor.and_then(|cursor| {
            self.entries.range((Excluded(cursor), Unbounded)).next()
        }).or_else(|| self.entries.iter().next()).map(|(&key, &entry)| (key, entry));
        if let Some((key, _)) = next { self.cursor = Some(key); }
        next
    }
}

#[derive(Debug, Default)]
struct SelectorState {
    registrations: Mutex<Registrations>,
    ready: Mutex<VecDeque<Event>>,
}

/// A local registration lifetime; it never owns a second kernel socket handle.
#[derive(Debug)]
pub(crate) struct SocketRegistration {
    state: Weak<SelectorState>,
    key: RegistrationKey,
    version: u64,
}

impl SocketRegistration {
    pub(crate) fn is_live(&self) -> bool { self.state.strong_count() != 0 }

    fn belongs_to(&self, selector: &Selector) -> bool {
        self.state.as_ptr() == Arc::as_ptr(&selector.state)
    }

    pub(crate) fn reregister(&mut self, selector: &Selector, token: Token, interests: Interest) -> io::Result<()> {
        if !self.belongs_to(selector) { return Err(io::ErrorKind::NotFound.into()); }
        {
            let mut registrations = selector.state.registrations.lock();
            if !registrations.entries.get(&self.key).is_some_and(|entry| entry.version == self.version) {
                return Err(io::ErrorKind::NotFound.into());
            }
            let version = registrations.next_version()?;
            registrations.entries.insert(self.key, Registration { token, interests, version, delivered: 0 });
            self.version = version;
        }
        crate::platform::wake_io();
        Ok(())
    }

    pub(crate) fn rearm(&self, directions: u8) {
        let Some(state) = self.state.upgrade() else { return; };
        let changed = {
            let mut registrations = state.registrations.lock();
            if let Some(entry) = registrations.entries.get_mut(&self.key)
                && entry.version == self.version
            {
                let previous = entry.delivered;
                entry.delivered &= !directions;
                previous != entry.delivered
            } else { false }
        };
        if changed { crate::platform::wake_io(); }
    }

    pub(crate) fn deregister(&self, selector: &Selector) -> io::Result<()> {
        if !self.belongs_to(selector) || !self.remove() {
            return Err(io::ErrorKind::NotFound.into());
        }
        Ok(())
    }

    fn remove(&self) -> bool {
        let Some(state) = self.state.upgrade() else { return false; };
        let removed = {
            let mut registrations = state.registrations.lock();
            if registrations.entries.get(&self.key).is_some_and(|entry| entry.version == self.version) {
                registrations.entries.remove(&self.key);
                true
            } else { false }
        };
        if removed { crate::platform::wake_io(); }
        removed
    }
}

impl Drop for SocketRegistration {
    fn drop(&mut self) { self.remove(); }
}

#[cfg(feature = "os-poll")]
#[cfg(debug_assertions)]
static NEXT_SELECTOR_ID: AtomicUsize = AtomicUsize::new(1);

#[derive(Debug)]
pub struct Selector {
    #[cfg(debug_assertions)]
    id: usize,
    state: Arc<SelectorState>,
}

impl Selector {
    #[cfg(feature = "os-poll")]
    pub fn new() -> io::Result<Selector> {
        Ok(Selector {
            #[cfg(debug_assertions)]
            id: NEXT_SELECTOR_ID.fetch_add(1, Ordering::Relaxed),
            state: Arc::new(SelectorState::default()),
        })
    }

    pub fn try_clone(&self) -> io::Result<Selector> {
        Ok(Selector {
            #[cfg(debug_assertions)]
            id: self.id,
            state: Arc::clone(&self.state),
        })
    }

    pub fn select(&self, events: &mut Events, timeout: Option<Duration>) -> io::Result<()> {
        events.clear();
        // A u128 deadline preserves finite Duration::MAX without turning it
        // into either an early timeout or the platform's infinite sentinel.
        let deadline = timeout.map(|duration| u128::from(crate::platform::monotonic_nanos()) + duration.as_nanos());
        loop {
            // Observe before checking either source of readiness. A socket or
            // Waker racing the scan advances this same native wait generation.
            let observed = crate::platform::observe_io();
            self.drain_ready(events);
            self.drain_socket_ready(events);
            if !events.is_empty() { return Ok(()); }
            let wait_ms = match deadline {
                Some(deadline) => {
                    let now = u128::from(crate::platform::monotonic_nanos());
                    if now >= deadline { return Ok(()); }
                    (deadline - now).div_ceil(1_000_000).min(u128::from(u64::MAX - 1)) as u64
                }
                None => u64::MAX,
            };
            // Every registration/ready lock is released before parking.
            // Spurious wakes recheck readiness and the original deadline.
            crate::platform::wait_io_after(observed, wait_ms);
        }
    }

    pub(crate) fn register_socket(&self, socket_id: u32, token: Token, interests: Interest) -> io::Result<SocketRegistration> {
        crate::zkvm_net::socket_ready(socket_id, interests)?;
        let key = RegistrationKey::Socket(socket_id);
        let version = {
            let mut registrations = self.state.registrations.lock();
            if registrations.entries.contains_key(&key) { return Err(io::ErrorKind::AlreadyExists.into()); }
            let version = registrations.next_version()?;
            registrations.entries.insert(key, Registration { token, interests, version, delivered: 0 });
            version
        };
        crate::platform::wake_io();
        Ok(SocketRegistration { state: Arc::downgrade(&self.state), key, version })
    }

    #[cfg(any(feature = "net", all(unix, feature = "os-ext")))]
    pub(crate) fn register_source(&self, source_id: usize, token: Token, interests: Interest) -> io::Result<()> {
        {
            let mut registrations = self.state.registrations.lock();
            let version = registrations.next_version()?;
            registrations.entries.insert(RegistrationKey::Source(source_id), Registration { token, interests, version, delivered: 0 });
        }
        crate::platform::wake_io();
        Ok(())
    }

    #[cfg(any(feature = "net", all(unix, feature = "os-ext")))]
    pub(crate) fn reregister_source(&self, source_id: usize, token: Token, interests: Interest) -> io::Result<()> {
        self.register_source(source_id, token, interests)
    }

    #[cfg(any(feature = "net", all(unix, feature = "os-ext")))]
    pub(crate) fn deregister_source(&self, source_id: usize) -> io::Result<()> {
        self.state.registrations.lock().entries.remove(&RegistrationKey::Source(source_id));
        crate::platform::wake_io();
        Ok(())
    }

    pub(crate) fn push_waker_event(&self, token: Token) -> io::Result<()> {
        let inserted = {
            let mut ready = self.state.ready.lock();
            // Mio exposes readiness, not a counted semaphore. Coalesce wakes
            // that are already pending instead of growing a wake-storm backlog.
            if ready.iter().any(|event| event.token == token) { false } else {
                ready.push_back(Event { token, readiness: Ready { readable: true, ..Ready::default() } });
                true
            }
        };
        if inserted { crate::platform::wake_io(); }
        Ok(())
    }

    fn drain_ready(&self, events: &mut Events) {
        let mut ready = self.state.ready.lock();
        while events.len() < events.capacity() {
            let Some(event) = ready.pop_front() else { break; };
            events.push(event);
        }
    }

    fn drain_socket_ready(&self, events: &mut Events) {
        let count = self.state.registrations.lock().entries.len();
        for _ in 0..count {
            if events.len() == events.capacity() { break; }
            let Some((key, entry)) = self.state.registrations.lock().next() else { break; };
            let socket_id = match key {
                RegistrationKey::Socket(socket_id) => socket_id,
                #[cfg(any(feature = "net", all(unix, feature = "os-ext")))]
                RegistrationKey::Source(_) => continue,
            };
            // A VM call may suspend; never hold the userspace registration lock
            // across the native probe. Reregister/deregister can run meanwhile.
            let ready = crate::zkvm_net::socket_ready(socket_id, entry.interests);
            let mut registrations = self.state.registrations.lock();
            if !registrations.entries.get(&key).is_some_and(|current| current.version == entry.version) { continue; }
            match ready {
                Ok(bits) => {
                    let current = registrations.entries.get_mut(&key).unwrap();
                    // Read the current delivered mask after the probe: an I/O
                    // WouldBlock on another thread may have rearmed this entry.
                    // Level transitions back to zero also rearm that direction.
                    current.delivered &= bits;
                    let newly_ready = bits & !current.delivered;
                    if newly_ready != 0 {
                        current.delivered |= newly_ready;
                        events.push(Event { token: current.token, readiness: Ready::from_zkvm_bits(newly_ready) });
                    }
                },
                Err(_) => { registrations.entries.remove(&key); },
            }
        }
    }
}

#[cfg(all(any(target_os = "trueos", target_os = "zkvm"), not(feature = "std-compat")))]
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

#[cfg(all(unix, not(all(any(target_os = "trueos", target_os = "zkvm"), not(feature = "std-compat")))))]
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
    #[cfg(debug_assertions)]
    impl Selector {
        pub fn id(&self) -> usize {
            self.id
        }
    }
}

#[cfg(all(unix, not(all(any(target_os = "trueos", target_os = "zkvm"), not(feature = "std-compat")))))]
impl AsFd for Selector {
    fn as_fd(&self) -> BorrowedFd<'_> {
        os_required!()
    }
}

#[cfg(all(unix, not(all(any(target_os = "trueos", target_os = "zkvm"), not(feature = "std-compat")))))]
impl AsRawFd for Selector {
    fn as_raw_fd(&self) -> RawFd {
        os_required!()
    }
}

#[allow(clippy::trivially_copy_pass_by_ref)]
pub mod event {
    use crate::sys::Event;
    use crate::Token;
    use ::core::fmt;

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
