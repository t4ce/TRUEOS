use crate::fs::asyncify;

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use crate::fs::trueos::Metadata;
#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
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
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    return crate::fs::trueos::metadata(&path).await;
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    asyncify(|| std::fs::symlink_metadata(path)).await
}
