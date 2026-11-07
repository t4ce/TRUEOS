//! Trusted Codex workspace provisioning on the BSP filesystem lane.
use trueos_crypto::AccountId;
use crate::r::{fs::trueosfs as fs, io::kfs::{FsError, Result}};
use sha2::{Digest, Sha256};

static PREPARE: embassy_sync::mutex::Mutex<crate::wait::EmbassySpinRawMutex, ()> =
    embassy_sync::mutex::Mutex::new(());
pub(crate) const CONFIGURATION_DB_PATH: &str = "users/configuration.redb";

/// Save ownership before stamping or granting. Failures never fall back to FFA.
/// Both disk and RAM roots are ready before the app receives its grant.
pub(crate) async fn prepare(
    archive: &str, workspace: &str, account: Option<AccountId>,
) -> Result<crate::r::fs::volatile_files::SeedStats> {
    let account = account.ok_or(FsError::PermissionDenied)?;
    if crate::access_key_db::container_for_workspace(archive, workspace).is_none() {
        return Err(FsError::BadPath);
    }
    let _prepare = PREPARE.lock().await;
    let disk = fs::primary_root_handle().ok_or(FsError::NoRoot)?;
    let image = fs::file_out_async(disk, CONFIGURATION_DB_PATH).await?;
    let mut handle = [0u8; 32];
    if !crate::tyche::fill_bytes(&mut handle) {
        return Err(FsError::Device(crate::disc::block::Error::NotReady));
    }
    let candidate = crate::access_key_store::software_key(handle)
        .map_err(|_| FsError::Device(crate::disc::block::Error::Corrupted))?;
    let update = crate::access_key_store::ensure_workspace(
        image.as_deref().unwrap_or_default(), workspace, account, candidate,
    ).map_err(|error| match error {
        crate::access_key_store::StoreError::ForeignOwner => FsError::PermissionDenied,
        crate::access_key_store::StoreError::InvalidWorkspace => FsError::BadPath,
        _ => FsError::Device(crate::disc::block::Error::Corrupted),
    })?;
    if let Some(next) = update.image {
        if !fs::dir_create_all_async(disk, "users").await? { return Err(FsError::NoSpace); }
        let digest = image.as_ref().map(|bytes| <[u8;32]>::from(Sha256::digest(bytes)));
        if !fs::file_in_typed_checked_async(
            disk, CONFIGURATION_DB_PATH, &next, fs::ContentTypeId::BLOB,
            image.is_none(), digest, false,
        ).await.map_err(|error| match error {
            fs::CheckedWriteError::Device(error) => FsError::from(error),
            fs::CheckedWriteError::AlreadyExists => FsError::AlreadyExists,
            fs::CheckedWriteError::Conflict => FsError::Device(crate::disc::block::Error::Io),
        })? { return Err(FsError::NoSpace); }
        let saved = fs::file_out_async(disk, CONFIGURATION_DB_PATH).await?;
        if saved.as_deref() != Some(next.as_slice()) {
            return Err(FsError::Device(crate::disc::block::Error::Io));
        }
    }
    let key = update.record.key;
    if !fs::dir_create_all_async(disk, workspace).await? { return Err(FsError::NoSpace); }
    if !fs::stamp_directory_key_async(disk, workspace, key).await? {
        return Err(FsError::PermissionDenied);
    }
    let stats = crate::r::fs::volatile_files::prepare(workspace).await?;
    crate::r::fs::volatile_files::stamp_workspace_key(workspace, key).await?;
    crate::access_key_db::grant_owned_workspace(archive, workspace, account, key)
        .map_err(|_| FsError::PermissionDenied)?;
    Ok(stats)
}
