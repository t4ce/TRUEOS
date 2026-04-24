use std::{fmt, io};

pub(crate) fn unsupported_io_error(detail: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, detail)
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