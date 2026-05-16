#[cfg(not(target_os = "zkvm"))]
use crate::fs::asyncify;
use alloc::borrow::ToOwned;

#[cfg(target_os = "zkvm")]
use crate::fs::trueos::Metadata;
#[cfg(not(target_os = "zkvm"))]
use std::fs::Metadata;
use std::io;
use std::path::Path;

/// Queries the file system metadata for a path.
///
/// This is an async version of [`std::fs::symlink_metadata`][std]
///
/// [std]: fn@std::fs::symlink_metadata
pub async fn symlink_metadata(path: impl AsRef<Path>) -> io::Result<Metadata> {
    let path = path.as_ref().to_owned();
    #[cfg(target_os = "zkvm")]
    return crate::fs::trueos::metadata(&path).await;
    #[cfg(not(target_os = "zkvm"))]
    asyncify(|| std::fs::symlink_metadata(path)).await
}
