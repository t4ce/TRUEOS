#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
use crate::fs::asyncify;
#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
use alloc::borrow::ToOwned;

use crate::io;
use crate::path::Path;

/// Creates a new hard link on the filesystem.
///
/// This is an async version of [`std::fs::hard_link`].
///
/// The `link` path will be a link pointing to the `original` path. Note that systems
/// often require these two paths to both be located on the same filesystem.
///
/// # Platform-specific behavior
///
/// This function currently corresponds to the `link` function on Unix
/// and the `CreateHardLink` function on Windows.
/// Note that, this [may change in the future][changes].
///
/// [changes]: https://doc.rust-lang.org/std/io/index.html#platform-specific-behavior
///
/// # Errors
///
/// This function will return an error in the following situations, but is not
/// limited to just these cases:
///
/// * The `original` path is not a file or doesn't exist.
///
/// # Examples
///
/// ```no_run
/// use tokio::fs;
///
/// #[tokio::main]
/// async fn main() -> std::io::Result<()> {
///     fs::hard_link("a.txt", "b.txt").await?; // Hard link a.txt to b.txt
///     Ok(())
/// }
/// ```
pub async fn hard_link(original: impl AsRef<Path>, link: impl AsRef<Path>) -> io::Result<()> {
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    let original = original.as_ref().to_owned();
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    let link = link.as_ref().to_owned();

    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    return Err(io::Error::new(
        io::ErrorKind::Other,
        "TRUEOS fs hard_link is not exposed through CABI yet",
    ));
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    asyncify(move || std::fs::hard_link(original, link)).await
}
