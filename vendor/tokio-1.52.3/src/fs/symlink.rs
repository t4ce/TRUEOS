use crate::fs::asyncify;
use alloc::borrow::ToOwned;

use std::io;
use crate::path::Path;

/// Creates a new symbolic link on the filesystem.
///
/// The `link` path will be a symbolic link pointing to the `original` path.
///
/// This is an async version of [`std::os::unix::fs::symlink`].
pub async fn symlink(original: impl AsRef<Path>, link: impl AsRef<Path>) -> io::Result<()> {
    let original = original.as_ref().to_owned();
    let link = link.as_ref().to_owned();

    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    {
        let _ = (original, link);
        Err(io::Error::new(
            io::ErrorKind::Other,
            "symlink is not supported on trueos",
        ))
    }

    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    asyncify(move || std::os::unix::fs::symlink(original, link)).await
}
