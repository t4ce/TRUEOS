#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
use crate::fs::asyncify;
#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
use alloc::borrow::ToOwned;

use std::io;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use crate::path::Path;
#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
use std::path::Path;

/// Renames a file or directory to a new name, replacing the original file if
/// `to` already exists.
///
/// This will not work if the new name is on a different mount point.
///
/// This is an async version of [`std::fs::rename`].
pub async fn rename(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<()> {
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    let from = from.as_ref().to_owned();
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    let to = to.as_ref().to_owned();

    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    return Err(io::Error::new(
        io::ErrorKind::Other,
        "TRUEOS fs rename is not exposed through CABI yet",
    ));
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    asyncify(move || std::fs::rename(from, to)).await
}
