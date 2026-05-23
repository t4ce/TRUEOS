#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
use crate::fs::asyncify;
use crate::runtime::prelude::*;
use alloc::borrow::ToOwned;

use std::io;
use crate::path::Path;

/// Creates a future which will open a file for reading and read the entire
/// contents into a string and return said string.
///
/// This is the async equivalent of [`std::fs::read_to_string`][std].
///
/// This operation is implemented by running the equivalent blocking operation
/// on a separate thread pool using [`spawn_blocking`].
///
/// [`spawn_blocking`]: crate::task::spawn_blocking
/// [std]: fn@std::fs::read_to_string
///
/// # Examples
///
/// ```no_run
/// use tokio::fs;
///
/// # async fn dox() -> std::io::Result<()> {
/// let contents = fs::read_to_string("foo.txt").await?;
/// println!("foo.txt contains {} bytes", contents.len());
/// # Ok(())
/// # }
/// ```
pub async fn read_to_string(path: impl AsRef<Path>) -> io::Result<String> {
    let path = path.as_ref().to_owned();
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    return crate::fs::trueos::read_to_string(&path).await;
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    asyncify(move || std::fs::read_to_string(path)).await
}
