#[cfg(not(feature = "std-compat"))]
use ::core::fmt;
use crate::io;

#[allow(dead_code)]
pub(crate) fn unsupported_io_error(detail: &'static str) -> io::Error {
    #[cfg(feature = "std-compat")]
    let kind = io::ErrorKind::Unsupported;
    #[cfg(not(feature = "std-compat"))]
    let kind = io::ErrorKind::Other;
    io::Error::new(kind, detail)
}

#[cfg(all(feature = "log", not(feature = "std-compat")))]
pub(crate) fn log(level: &'static str, args: fmt::Arguments<'_>) {
    match level {
        "trace" => ::log::trace!("mio.zkvm: {}", args),
        "warn" => ::log::warn!("mio.zkvm: {}", args),
        "error" => ::log::error!("mio.zkvm: {}", args),
        _ => ::log::debug!("mio.zkvm:{} {}", level, args),
    }
}

#[cfg(all(not(feature = "log"), not(feature = "std-compat")))]
#[allow(dead_code)]
pub(crate) fn log(_: &'static str, _: fmt::Arguments<'_>) {}
