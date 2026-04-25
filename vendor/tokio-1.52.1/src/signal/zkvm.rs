use crate::signal::RxFuture;
use std::io;

pub(super) fn ctrl_c() -> io::Result<RxFuture> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "tokio signal ctrl-c is not supported on zkvm",
    ))
}
