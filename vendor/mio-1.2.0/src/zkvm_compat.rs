use std::time::Duration;
use std::{fmt, io};

unsafe extern "C" {
    fn trueos_cabi_poll_once();
    fn trueos_tokio_time_now_nanos() -> u64;
}

pub(crate) fn unsupported_io_error(detail: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, detail)
}

#[inline]
pub(crate) fn poll_once() {
    unsafe { trueos_cabi_poll_once() }
}

#[inline]
pub(crate) fn now_nanos() -> u64 {
    unsafe { trueos_tokio_time_now_nanos() }
}

#[inline]
pub(crate) fn duration_to_nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

#[cfg(feature = "log")]
pub(crate) fn log(level: &'static str, args: fmt::Arguments<'_>) {
    match level {
        "trace" => ::log::trace!("mio.zkvm: {}", args),
        "warn" => ::log::warn!("mio.zkvm: {}", args),
        "error" => ::log::error!("mio.zkvm: {}", args),
        _ => ::log::debug!("mio.zkvm:{} {}", level, args),
    }
}

#[cfg(not(feature = "log"))]
#[allow(dead_code)]
pub(crate) fn log(_: &'static str, _: fmt::Arguments<'_>) {}