macro_rules! os_required {
    () => {
        panic!("mio must be compiled with `os-poll` to run.")
    };
}

mod selector;
pub(crate) use self::selector::{event, Event, Events, Selector};

#[cfg(not(target_os = "wasi"))]
mod waker;
#[cfg(not(target_os = "wasi"))]
pub(crate) use self::waker::Waker;

cfg_net! {
    pub(crate) mod tcp;
    pub(crate) mod udp;
    #[cfg(unix)]
    pub(crate) mod uds;
}

cfg_io_source! {
    use std::io;
    #[cfg(unix)]
    use std::os::fd::RawFd;
    // TODO: once <https://github.com/rust-lang/rust/issues/126198> is fixed this
    // can use `std::os::fd` and be merged with the above.
    #[cfg(target_os = "hermit")]
    use std::os::hermit::io::RawFd;
    #[cfg(windows)]
    use std::os::windows::io::RawSocket;

    #[cfg(any(windows, unix, target_os = "hermit"))]
    use crate::{Registry, Token, Interest};

    pub(crate) struct IoSourceState;

    impl IoSourceState {
        pub fn new() -> IoSourceState {
            IoSourceState
        }

        pub fn do_io<T, F, R>(&self, f: F, io: &T) -> io::Result<R>
        where
            F: FnOnce(&T) -> io::Result<R>,
        {
            // We don't hold state, so we can just call the function and
            // return.
            f(io)
        }
    }

    #[cfg(any(unix, target_os = "hermit"))]
    impl IoSourceState {
        pub fn register(
            &mut self,
            _: &Registry,
            _: Token,
            _: Interest,
            _: RawFd,
        ) -> io::Result<()> {
            unsupported_io!("mio zkvm source registration backend is not wired yet")
        }

        pub fn reregister(
            &mut self,
            _: &Registry,
            _: Token,
            _: Interest,
            _: RawFd,
        ) -> io::Result<()> {
            unsupported_io!("mio zkvm source reregistration backend is not wired yet")
        }

        pub fn deregister(&mut self, _: &Registry, _: RawFd) -> io::Result<()> {
            unsupported_io!("mio zkvm source deregistration backend is not wired yet")
        }
    }

    #[cfg(target_os = "zkvm")]
    impl IoSourceState {
        pub fn register(
            &mut self,
            _: &crate::Registry,
            _: crate::Token,
            _: crate::Interest,
        ) -> io::Result<()> {
            unsupported_io!("mio zkvm source registration backend is not wired yet")
        }

        pub fn reregister(
            &mut self,
            _: &crate::Registry,
            _: crate::Token,
            _: crate::Interest,
        ) -> io::Result<()> {
            unsupported_io!("mio zkvm source reregistration backend is not wired yet")
        }

        pub fn deregister(&mut self, _: &crate::Registry) -> io::Result<()> {
            unsupported_io!("mio zkvm source deregistration backend is not wired yet")
        }
    }

    #[cfg(windows)]
    impl IoSourceState {
         pub fn register(
            &mut self,
            _: &Registry,
            _: Token,
            _: Interest,
            _: RawSocket,
        ) -> io::Result<()> {
            unsupported_io!("mio zkvm source registration backend is not wired yet")
        }

        pub fn reregister(
            &mut self,
            _: &Registry,
            _: Token,
            _: Interest,
        ) -> io::Result<()> {
            unsupported_io!("mio zkvm source reregistration backend is not wired yet")
        }

        pub fn deregister(&mut self) -> io::Result<()> {
            unsupported_io!("mio zkvm source deregistration backend is not wired yet")
        }
    }
}
