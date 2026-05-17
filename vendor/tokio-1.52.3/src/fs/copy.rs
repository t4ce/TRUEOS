#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
use crate::fs::asyncify;
use alloc::borrow::ToOwned;
use std::path::Path;

/// Copies the contents of one file to another. This function will also copy the permission bits
/// of the original file to the destination file.
/// This function will overwrite the contents of to.
///
/// This is the async equivalent of [`std::fs::copy`].
///
/// # Examples
///
/// ```no_run
/// use tokio::fs;
///
/// # async fn dox() -> std::io::Result<()> {
/// fs::copy("foo.txt", "bar.txt").await?;
/// # Ok(())
/// # }
/// ```
pub async fn copy(from: impl AsRef<Path>, to: impl AsRef<Path>) -> Result<u64, std::io::Error> {
    let from = from.as_ref().to_owned();
    let to = to.as_ref().to_owned();
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    {
        let bytes = crate::fs::trueos::read(&from).await?;
        let len = bytes.len() as u64;
        crate::fs::trueos::write(&to, &bytes).await?;
        return Ok(len);
    }
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    asyncify(|| std::fs::copy(from, to)).await
}
