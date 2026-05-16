#[cfg(not(target_os = "zkvm"))]
use crate::fs::asyncify;
use alloc::borrow::ToOwned;

use std::io;
use std::path::Path;

/// Removes a file from the filesystem.
///
/// Note that there is no guarantee that the file is immediately deleted (e.g.
/// depending on platform, other open file descriptors may prevent immediate
/// removal).
///
/// This is an async version of [`std::fs::remove_file`].
pub async fn remove_file(path: impl AsRef<Path>) -> io::Result<()> {
    let path = path.as_ref().to_owned();
    #[cfg(target_os = "zkvm")]
    return crate::fs::trueos::remove_file(&path).await;
    #[cfg(not(target_os = "zkvm"))]
    asyncify(move || std::fs::remove_file(path)).await
}
