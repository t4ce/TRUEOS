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
    prepare_owned(archive, workspace, account, true).await
}

/// Persistent private app data uses the same durable owner/key relation as
/// Codex, without installing its boot-local RAM file overlay.
pub(crate) async fn prepare_persistent(
    archive: &str, workspace: &str, account: Option<AccountId>,
) -> Result<()> {
    if !crate::hv::blueprint::uses_private_persistent_files(archive) {
        return Err(FsError::BadPath);
    }
    prepare_owned(archive, workspace, account, false).await.map(|_| ())
}

async fn prepare_owned(
    archive: &str, workspace: &str, account: Option<AccountId>, volatile: bool,
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
    let stats = if volatile {
        let stats = crate::r::fs::volatile_files::prepare(workspace).await?;
        crate::r::fs::volatile_files::stamp_workspace_key(workspace, key).await?;
        stats
    } else {
        // The marker is a comparison anchor, not a secret or a guest-selected
        // authority. Its current ancestor key is the stamped private root.
        let marker = alloc::format!("{workspace}/.trueos-private-root");
        if !fs::file_in_typed_checked_async(
            disk, &marker, b"trueos-private-workspace-v1\n", fs::ContentTypeId::BLOB,
            false, None, false,
        ).await.map_err(|error| match error {
            fs::CheckedWriteError::Device(error) => FsError::from(error),
            _ => FsError::Device(crate::disc::block::Error::Io),
        })? { return Err(FsError::NoSpace); }
        crate::r::fs::volatile_files::SeedStats::default()
    };
    crate::access_key_db::grant_owned_workspace(archive, workspace, account, key)
        .map_err(|_| FsError::PermissionDenied)?;
    crate::access_key_db::reserve_codex_sqlite(workspace, account, key)
        .map_err(|_| FsError::PermissionDenied)?;
    Ok(stats)
}

/// The dedicated SQLite dependency uses Codex's existing durable key/root.
/// It must be admitted by the authenticated owner; preparation is serialized
/// through the same trusted BSP lane before any service guest executes.
pub(crate) async fn prepare_sqlite(archive: &str, workspace: &str, account: Option<AccountId>) -> Result<()> {
    if crate::access_key_db::container_for_workspace(archive, workspace)
        .is_none_or(|id| id.app().as_str() != "sqlite") { return Err(FsError::BadPath); }
    let _ = prepare("codex.bp", "apps/codex", account).await?;
    let _prepare = PREPARE.lock().await;
    let disk = fs::primary_root_handle().ok_or(FsError::NoRoot)?;
    let key = fs::node_info_async(disk, "apps/codex").await?
        .ok_or(FsError::NotFound)?.record_key;
    // Adopt the current installation's legacy public service directory using
    // the same explicit owner adoption as its existing Codex app root. Stamp
    // before moving so no failure leaves the source publicly readable. The
    // directory move keeps IDs/data and refuses an existing destination.
    const OLD: &str = "apps/common/sqlite-service/codex";
    const NEW: &str = "apps/codex/.codex/sqlite-service/codex";
    if !fs::dir_create_all_async(disk, "apps/codex/.codex/sqlite-service").await? { return Err(FsError::NoSpace); }
    if let Some(old) = fs::node_info_async(disk, OLD).await? {
        if old.kind != trueos_fs::NodeKind::Directory
            || old.record_key != trueos_fs::RecordKey::Ffa && old.record_key != key {
            return Err(FsError::PermissionDenied);
        }
        if fs::node_info_async(disk, NEW).await?.is_some() { return Err(FsError::AlreadyExists); }
        if !fs::stamp_directory_record_key_async(disk, OLD, key).await? { return Err(FsError::PermissionDenied); }
        if !fs::dir_rename_async(disk, OLD, NEW).await? {
            return Err(FsError::Device(crate::disc::block::Error::Io));
        }
    }
    crate::access_key_db::grant_codex_sqlite(archive, workspace, account)
        .map_err(|_| FsError::PermissionDenied)
}
