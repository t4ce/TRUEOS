#[allow(unused_macros)]
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
    #[cfg(all(unix, not(all(any(target_os = "trueos", target_os = "zkvm"), not(feature = "std-compat")))))]
    pub(crate) mod uds;
}

cfg_io_source! {
    use crate::io;
    #[cfg(all(any(target_os = "trueos", target_os = "zkvm"), not(feature = "std-compat")))]
    use core::sync::atomic::{AtomicUsize, Ordering};
    #[cfg(all(unix, not(all(any(target_os = "trueos", target_os = "zkvm"), not(feature = "std-compat")))))]
    use std::os::fd::RawFd;
    // TODO: once <https://github.com/rust-lang/rust/issues/126198> is fixed this
    // can use `std::os::fd` and be merged with the above.
    #[cfg(target_os = "hermit")]
    use std::os::hermit::io::RawFd;
    #[cfg(windows)]
    use std::os::windows::io::RawSocket;

    #[cfg(any(windows, all(unix, not(all(any(target_os = "trueos", target_os = "zkvm"), not(feature = "std-compat")))), target_os = "hermit"))]
    use crate::{Registry, Token, Interest};

    #[cfg(all(any(target_os = "trueos", target_os = "zkvm"), not(feature = "std-compat")))]
    #[allow(dead_code)]
    static NEXT_SOURCE_ID: AtomicUsize = AtomicUsize::new(1);

    pub(crate) struct IoSourceState {
        #[cfg(all(any(target_os = "trueos", target_os = "zkvm"), not(feature = "std-compat")))]
        source_id: usize,
    }

    #[allow(dead_code)]
    impl IoSourceState {
        pub fn new() -> IoSourceState {
            IoSourceState {
                #[cfg(all(any(target_os = "trueos", target_os = "zkvm"), not(feature = "std-compat")))]
                source_id: NEXT_SOURCE_ID.fetch_add(1, Ordering::Relaxed),
            }
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

    #[cfg(any(all(unix, not(all(any(target_os = "trueos", target_os = "zkvm"), not(feature = "std-compat")))), target_os = "hermit"))]
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

    #[cfg(all(any(target_os = "trueos", target_os = "zkvm"), not(feature = "std-compat")))]
    impl IoSourceState {
        pub fn register(
            &mut self,
            registry: &crate::Registry,
            token: crate::Token,
            interests: crate::Interest,
        ) -> io::Result<()> {
            registry
                .selector()
                .register_source(self.source_id, token, interests)
        }

        pub fn reregister(
            &mut self,
            registry: &crate::Registry,
            token: crate::Token,
            interests: crate::Interest,
        ) -> io::Result<()> {
            registry
                .selector()
                .reregister_source(self.source_id, token, interests)
        }

        pub fn deregister(&mut self, registry: &crate::Registry) -> io::Result<()> {
            registry.selector().deregister_source(self.source_id)
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
