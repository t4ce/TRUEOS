use crate::signal::RxFuture;
use crate::io;

pub(super) fn ctrl_c() -> io::Result<RxFuture> {
    Err(io::Error::new(
        io::ErrorKind::Other,
        "tokio signal ctrl-c is not supported on zkvm",
    ))
}
