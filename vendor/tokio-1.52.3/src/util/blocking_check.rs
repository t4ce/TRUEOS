#[cfg(all(unix, not(any(target_os = "trueos", target_os = "zkvm"))))]
use std::os::fd::AsFd;

#[cfg(all(unix, not(any(target_os = "trueos", target_os = "zkvm"))))]
#[allow(unused_variables)]
#[track_caller]
pub(crate) fn check_socket_for_blocking<S: AsFd>(s: &S) -> crate::io::Result<()> {
    #[cfg(not(tokio_allow_from_blocking_fd))]
    {
        let sock = socket2::SockRef::from(s);

        debug_assert!(
            sock.nonblocking()?,
            "Registering a blocking socket with the tokio runtime is unsupported. \
            If you wish to do anyways, please add `--cfg tokio_allow_from_blocking_fd` to your \
            RUSTFLAGS. See github.com/tokio-rs/tokio/issues/7172 for details."
        );
    }

    Ok(())
}

#[cfg(any(not(unix), any(target_os = "trueos", target_os = "zkvm"), any(target_os = "trueos", target_os = "zkvm")))]
#[allow(unused_variables)]
pub(crate) fn check_socket_for_blocking<S>(s: &S) -> crate::io::Result<()> {
    // we cannot retrieve the nonblocking status on windows
    // and i dont know how to support wasi yet
    Ok(())
}
