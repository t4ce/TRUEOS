#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
use crate::fs::asyncify;
#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
use alloc::borrow::ToOwned;

use std::io;
use crate::path::{Path, PathBuf};

/// Reads a symbolic link, returning the file that the link points to.
///
/// This is an async version of [`std::fs::read_link`].
pub async fn read_link(path: impl AsRef<Path>) -> io::Result<PathBuf> {
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    let path = path.as_ref().to_owned();
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    return Err(io::Error::new(
        io::ErrorKind::Other,
        "TRUEOS fs read_link is not exposed through CABI yet",
    ));
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    asyncify(move || std::fs::read_link(path)).await
}
