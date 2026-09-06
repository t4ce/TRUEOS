#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
use std::sync::LockResult;

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use core::convert::Infallible;

pub(crate) trait LockResultExt<T> {
    fn panic_if_poisoned(self) -> T;
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
impl<T> LockResultExt<T> for LockResult<T> {
    #[track_caller]
    fn panic_if_poisoned(self) -> T {
        match self {
            Ok(inner) => inner,
            Err(err) => panic!("lock poisoned by panic: {err}"),
        }
    }
}

// TRUEOS's spin mutex cannot be poisoned, so its lock result has an
// uninhabited error type. Keep Hyper's lock helper API while removing the
// host-`std` poisoning assumption.
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
impl<T> LockResultExt<T> for Result<T, Infallible> {
    #[inline]
    fn panic_if_poisoned(self) -> T {
        match self {
            Ok(inner) => inner,
            Err(err) => match err {},
        }
    }
}
