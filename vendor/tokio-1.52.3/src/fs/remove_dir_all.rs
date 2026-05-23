#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
use crate::fs::asyncify;
use alloc::borrow::ToOwned;

use crate::io;
use crate::path::Path;

/// Removes a directory at this path, after removing all its contents. Use carefully!
///
/// This is an async version of [`std::fs::remove_dir_all`][std]
///
/// [std]: fn@std::fs::remove_dir_all
pub async fn remove_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
    let path = path.as_ref().to_owned();
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    return crate::fs::trueos::remove_file(&path).await;
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    asyncify(move || std::fs::remove_dir_all(path)).await
}
