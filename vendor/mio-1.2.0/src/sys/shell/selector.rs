use std::io;
#[cfg(unix)]
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, RawFd};
use std::time::Duration;

pub type Event = usize;

pub type Events = Vec<Event>;

#[derive(Debug)]
pub struct Selector {}

impl Selector {
    pub fn new() -> io::Result<Selector> {
        Ok(Selector {})
    }

    pub fn try_clone(&self) -> io::Result<Selector> {
        Ok(Selector {})
    }

    pub fn select(&self, events: &mut Events, _: Option<Duration>) -> io::Result<()> {
        events.clear();
        unsupported_io!("mio zkvm selector wait loop is not wired yet");
    }
}

#[cfg(unix)]
cfg_os_ext! {
    use crate::{Interest, Token};

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
            os_required!();
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

    pub fn token(_: &Event) -> Token {
        os_required!();
    }

    pub fn is_readable(_: &Event) -> bool {
        os_required!();
    }

    pub fn is_writable(_: &Event) -> bool {
        os_required!();
    }

    pub fn is_error(_: &Event) -> bool {
        os_required!();
    }

    pub fn is_read_closed(_: &Event) -> bool {
        os_required!();
    }

    pub fn is_write_closed(_: &Event) -> bool {
        os_required!();
    }

    pub fn is_priority(_: &Event) -> bool {
        os_required!();
    }

    pub fn is_aio(_: &Event) -> bool {
        os_required!();
    }

    pub fn is_lio(_: &Event) -> bool {
        os_required!();
    }

    pub fn debug_details(_: &mut fmt::Formatter<'_>, _: &Event) -> fmt::Result {
        os_required!();
    }
}
