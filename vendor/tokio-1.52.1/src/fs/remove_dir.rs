#[cfg(not(target_os = "zkvm"))]
use crate::fs::asyncify;
use alloc::borrow::ToOwned;

use std::io;
use std::path::Path;

/// Removes an existing, empty directory.
///
/// This is an async version of [`std::fs::remove_dir`].
pub async fn remove_dir(path: impl AsRef<Path>) -> io::Result<()> {
    let path = path.as_ref().to_owned();
    #[cfg(target_os = "zkvm")]
    return crate::fs::trueos::remove_file(&path).await;
    #[cfg(not(target_os = "zkvm"))]
    asyncify(move || std::fs::remove_dir(path)).await
}
