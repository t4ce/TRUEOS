#[cfg(any(
    all(any(feature = "client", feature = "server"), feature = "http2"),
    all(feature = "server", feature = "http1"),
))]
use std::time::Duration;
use std::{fmt, sync::Arc};
use std::{pin::Pin, time::Instant};

use crate::rt::Sleep;
use crate::rt::Timer;

#[cfg(target_os = "zkvm")]
fn instant_now() -> Instant {
    let duration = std::time::Duration::from_nanos(unsafe { trueos_tokio_time_now_nanos() });

    // Rust's unsupported std time backend stores Instant as a single Duration.
    // TRUEOS supplies the missing clock value through the std ABI shim.
    unsafe { core::mem::transmute::<std::time::Duration, Instant>(duration) }
}

#[cfg(not(target_os = "zkvm"))]
fn instant_now() -> Instant {
    Instant::now()
}

#[cfg(target_os = "zkvm")]
unsafe extern "C" {
    fn trueos_tokio_time_now_nanos() -> u64;
}

/// A user-provided timer to time background tasks.
#[derive(Clone)]
pub(crate) enum Time {
    Timer(Arc<dyn Timer + Send + Sync>),
    Empty,
}

#[cfg(all(feature = "server", feature = "http1"))]
#[derive(Clone, Copy, Debug)]
pub(crate) enum Dur {
    Default(Option<Duration>),
    Configured(Option<Duration>),
}

impl fmt::Debug for Time {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Time").finish()
    }
}

impl Time {
    #[cfg(all(any(feature = "client", feature = "server"), feature = "http2"))]
    pub(crate) fn sleep(&self, duration: Duration) -> Pin<Box<dyn Sleep>> {
        match *self {
            Time::Empty => {
                panic!("You must supply a timer.")
            }
            Time::Timer(ref t) => t.sleep(duration),
        }
    }

    #[cfg(all(feature = "server", feature = "http1"))]
    pub(crate) fn sleep_until(&self, deadline: Instant) -> Pin<Box<dyn Sleep>> {
        match *self {
            Time::Empty => {
                panic!("You must supply a timer.")
            }
            Time::Timer(ref t) => t.sleep_until(deadline),
        }
    }

    pub(crate) fn now(&self) -> Instant {
        match *self {
            Time::Empty => instant_now(),
            Time::Timer(ref t) => t.now(),
        }
    }

    pub(crate) fn reset(&self, sleep: &mut Pin<Box<dyn Sleep>>, new_deadline: Instant) {
        match *self {
            Time::Empty => {
                panic!("You must supply a timer.")
            }
            Time::Timer(ref t) => t.reset(sleep, new_deadline),
        }
    }

    #[cfg(all(feature = "server", feature = "http1"))]
    pub(crate) fn check(&self, dur: Dur, name: &'static str) -> Option<Duration> {
        match dur {
            Dur::Default(Some(dur)) => match self {
                Time::Empty => {
                    warn!("timeout `{}` has default, but no timer set", name,);
                    None
                }
                Time::Timer(..) => Some(dur),
            },
            Dur::Configured(Some(dur)) => match self {
                Time::Empty => panic!("timeout `{}` set, but no timer set", name,),
                Time::Timer(..) => Some(dur),
            },
            Dur::Default(None) | Dur::Configured(None) => None,
        }
    }
}
