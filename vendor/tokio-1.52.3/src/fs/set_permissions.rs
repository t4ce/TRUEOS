#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
use crate::fs::asyncify;
use alloc::borrow::ToOwned;

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use crate::fs::trueos::Permissions;
#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
use std::fs::Permissions;
use std::io;
use crate::path::Path;

/// Changes the permissions found on a file or a directory.
///
/// This is an async version of [`std::fs::set_permissions`][std]
///
/// [std]: fn@std::fs::set_permissions
pub async fn set_permissions(path: impl AsRef<Path>, perm: Permissions) -> io::Result<()> {
    let path = path.as_ref().to_owned();
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    return crate::fs::trueos::set_permissions(&path, perm).await;
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    asyncify(|| std::fs::set_permissions(path, perm)).await
}
