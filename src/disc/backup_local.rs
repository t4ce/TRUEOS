//! Local, whole-disk TRUEOSFS backup images.
//!
//! Images become visible to the restore catalog only after their stream and a
//! separate SHA-256 manifest have both committed.  A restore verifies that
//! digest before acquiring the target's destructive raw-write lease.

use alloc::{format, string::String, vec, vec::Vec};
use core::sync::atomic::{AtomicU32, Ordering};
use sha2::{Digest, Sha256};
use trueos_time::{Duration, Instant, Timer};

use super::block::{self, BackupLease, DeviceHandle, RawRestoreLease};
#[path = "backup_manifest.rs"]
mod backup_manifest;

const ACQUIRE_WINDOW_MS: u64 = 5_000;
const COPY_CHUNK_LIMIT: u64 = 256 * 1024;
static BACKUP_SEQUENCE: AtomicU32 = AtomicU32::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgressPhase {
    Acquiring,
    Copying,
    Verifying,
    Restoring,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Progress {
    pub phase: ProgressPhase,
    pub completed_bytes: u64,
    pub total_bytes: u64,
}

/// Returning `false` from the progress callback cancels before the next I/O.
pub type ProgressCallback<'a> = dyn FnMut(Progress) -> bool + 'a;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackupArtifact {
    /// Opaque catalog identity, also the generated filename stem.
    pub name: String,
    pub manifest_path: String,
    pub image_path: String,
    pub image_bytes: u64,
    pub block_size: u32,
    pub source_blocks: u64,
    pub source_label: Option<String>,
    pub sha256: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RestoreReport {
    pub restored_bytes: u64,
    pub target_blocks: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Block(block::Error),
    Busy,
    Cancelled,
    NoSpace,
    InvalidArtifact,
    RandomUnavailable,
}

impl From<block::Error> for Error {
    fn from(value: block::Error) -> Self {
        Self::Block(value)
    }
}

pub type Result<T> = core::result::Result<T, Error>;

fn now_ms() -> u64 {
    Instant::now().as_millis()
}

fn source_bytes(disk: DeviceHandle) -> Result<u64> {
    if disk.parent().is_some() || disk.block_size() == 0 {
        return Err(Error::Block(block::Error::InvalidParam));
    }
    disk.block_count()
        .checked_mul(disk.block_size() as u64)
        .filter(|bytes| *bytes != 0)
        .ok_or(Error::Block(block::Error::InvalidParam))
}

fn require_mounted_root(disk: DeviceHandle) -> Result<()> {
    if disk.parent().is_some()
        || !crate::r::fs::trueosfs::list_roots()
            .iter()
            .any(|root| root.disk_id == disk.id())
    {
        return Err(Error::Block(block::Error::NotReady));
    }
    Ok(())
}

fn chunk_bytes(disk: DeviceHandle) -> Result<usize> {
    let block_size = disk.block_size() as u64;
    let max = disk.max_transfer_bytes().min(COPY_CHUNK_LIMIT);
    let bytes = (max / block_size) * block_size;
    usize::try_from(bytes)
        .ok()
        .filter(|bytes| *bytes != 0)
        .ok_or(Error::Block(block::Error::InvalidParam))
}

fn invoke(
    progress: &mut ProgressCallback<'_>,
    phase: ProgressPhase,
    done: u64,
    total: u64,
) -> Result<()> {
    if progress(Progress {
        phase,
        completed_bytes: done,
        total_bytes: total,
    }) {
        Ok(())
    } else {
        Err(Error::Cancelled)
    }
}

async fn acquire_backup(
    disk: DeviceHandle,
    progress: &mut ProgressCallback<'_>,
) -> Result<BackupLease> {
    let deadline = now_ms().saturating_add(ACQUIRE_WINDOW_MS);
    loop {
        invoke(progress, ProgressPhase::Acquiring, 0, 0)?;
        match BackupLease::try_acquire(disk) {
            Ok(lease) => return Ok(lease),
            Err(_) if now_ms() < deadline => Timer::after(Duration::from_millis(50)).await,
            Err(_) => return Err(Error::Busy),
        }
    }
}

async fn acquire_restore(
    disk: DeviceHandle,
    progress: &mut ProgressCallback<'_>,
) -> Result<RawRestoreLease> {
    let deadline = now_ms().saturating_add(ACQUIRE_WINDOW_MS);
    loop {
        invoke(progress, ProgressPhase::Acquiring, 0, 0)?;
        match RawRestoreLease::try_acquire(disk) {
            Ok(lease) => return Ok(lease),
            Err(_) if now_ms() < deadline => Timer::after(Duration::from_millis(50)).await,
            Err(_) => return Err(Error::Busy),
        }
    }
}

fn generated_name(source: DeviceHandle) -> Result<String> {
    let mut random = [0u8; 8];
    if !crate::tyche::fill_bytes(&mut random) {
        return Err(Error::RandomUnavailable);
    }
    let sequence = BACKUP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    Ok(format!(
        "disc{:03}-{:08x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        source.id().raw(),
        sequence,
        random[0],
        random[1],
        random[2],
        random[3],
        random[4],
        random[5],
        random[6],
        random[7]
    ))
}

fn paths(name: &str) -> (String, String) {
    (format!("backups/{name}.manifest"), format!("backups/{name}.img"))
}

fn decode_manifest(name: String, bytes: &[u8]) -> Result<BackupArtifact> {
    let manifest = backup_manifest::decode(bytes).map_err(|_| Error::InvalidArtifact)?;
    let (manifest_path, image_path) = paths(name.as_str());
    Ok(BackupArtifact {
        name,
        manifest_path,
        image_path,
        image_bytes: manifest.image_bytes,
        block_size: manifest.block_size,
        source_blocks: manifest.source_blocks,
        source_label: manifest.source_label,
        sha256: manifest.sha256,
    })
}

/// List only completed image/manifest pairs. The caller keeps an entry from
/// this return value and passes its `manifest_path` back to restore.
pub async fn list_local_backups_async(root: DeviceHandle) -> Result<Vec<BackupArtifact>> {
    require_mounted_root(root)?;
    let Some(entries) = crate::r::fs::trueosfs::list_dir_async(root, "backups").await? else {
        return Ok(Vec::new());
    };
    let mut artifacts = Vec::new();
    for entry in entries.entries {
        if entry.kind != crate::r::fs::trueosfs::NodeKind::File
            || !entry.name.ends_with(".manifest")
        {
            continue;
        }
        let Some(name) = entry.name.strip_suffix(".manifest") else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        let manifest_path = format!("backups/{}", entry.name);
        let Some(info) =
            crate::r::fs::trueosfs::file_info_async(root, manifest_path.as_str()).await?
        else {
            continue;
        };
        if info.data_len != backup_manifest::LEN as u64 {
            continue;
        }
        let Some(manifest) =
            crate::r::fs::trueosfs::file_out_async(root, manifest_path.as_str()).await?
        else {
            continue;
        };
        let Ok(artifact) = decode_manifest(String::from(name), &manifest) else {
            continue;
        };
        let Some(info) =
            crate::r::fs::trueosfs::file_info_async(root, artifact.image_path.as_str()).await?
        else {
            continue;
        };
        if info.data_len == artifact.image_bytes {
            artifacts.push(artifact);
        }
    }
    Ok(artifacts)
}

pub async fn available_backup_bytes_async(root: DeviceHandle) -> Result<u64> {
    require_mounted_root(root)?;
    crate::r::fs::trueosfs::available_backup_bytes_async(root)
        .await?
        .ok_or(Error::Block(block::Error::NotReady))
}

/// Stream a whole source disk to a newly generated `backups/` image.
pub async fn create_local_backup_async(
    source: DeviceHandle,
    destination_root: DeviceHandle,
    progress: &mut ProgressCallback<'_>,
) -> Result<BackupArtifact> {
    if source.id() == destination_root.id() {
        return Err(Error::Block(block::Error::InvalidParam));
    }
    require_mounted_root(destination_root)?;
    let total = source_bytes(source)?;
    if available_backup_bytes_async(destination_root).await? < total {
        return Err(Error::NoSpace);
    }
    if !crate::r::fs::trueosfs::dir_create_all_async(destination_root, "backups").await? {
        return Err(Error::NoSpace);
    }
    let name = generated_name(source)?;
    let (manifest_path, image_path) = paths(name.as_str());
    let Some(stream) = crate::r::fs::trueosfs::file_write_begin_async(
        destination_root,
        image_path.as_str(),
        total,
    )
    .await?
    else {
        return Err(Error::NoSpace);
    };

    let outcome = async {
        let lease = acquire_backup(source, progress).await?;
        // Lease precedes guard so the mount is restored before ordinary I/O opens.
        let _mount = crate::r::fs::trueosfs::suspend_backup_mount(source.id());
        lease.flush().await?;
        let chunk = chunk_bytes(source)?;
        let mut hasher = Sha256::new();
        let mut offset = 0u64;
        while offset < total {
            invoke(progress, ProgressPhase::Copying, offset, total)?;
            let bytes = (total - offset).min(chunk as u64) as usize;
            let data = lease
                .read(offset / source.block_size() as u64, bytes / source.block_size() as usize)
                .await?;
            if data.len() != bytes {
                return Err(Error::Block(block::Error::Corrupted));
            }
            hasher.update(&data);
            crate::r::fs::trueosfs::file_write_chunk_async(stream, &data).await?;
            offset += bytes as u64;
        }
        invoke(progress, ProgressPhase::Copying, total, total)?;
        crate::r::fs::trueosfs::file_write_finish_async(stream).await?;
        destination_root.flush().await?;
        let mut sha256 = [0u8; 32];
        sha256.copy_from_slice(&hasher.finalize());
        let artifact = BackupArtifact {
            name,
            manifest_path,
            image_path,
            image_bytes: total,
            block_size: source.block_size(),
            source_blocks: source.block_count(),
            source_label: source.info().label,
            sha256,
        };
        if !crate::r::fs::trueosfs::file_write_all_async(
            destination_root,
            artifact.manifest_path.as_str(),
            &backup_manifest::encode(
                &backup_manifest::Manifest {
                    block_size: artifact.block_size,
                    source_blocks: artifact.source_blocks,
                    image_bytes: artifact.image_bytes,
                    sha256: artifact.sha256,
                    source_label: artifact.source_label.clone(),
                },
                source.id().raw(),
            ),
        )
        .await?
        {
            return Err(Error::NoSpace);
        }
        destination_root.flush().await?;
        Ok(artifact)
    }
    .await;
    if outcome.is_err() {
        let _ = crate::r::fs::trueosfs::file_write_abort_async(stream).await;
    }
    outcome
}

/// Verify the selected completed image, then overwrite the target raw blocks.
/// Target geometry must exactly match the source image; no format operation is
/// performed. A partial write discards stale mount state and requests a probe.
pub async fn restore_local_backup_async(
    artifact_root: DeviceHandle,
    expected: &BackupArtifact,
    target: DeviceHandle,
    progress: &mut ProgressCallback<'_>,
) -> Result<RestoreReport> {
    if artifact_root.id() == target.id() {
        return Err(Error::Block(block::Error::InvalidParam));
    }
    require_mounted_root(artifact_root)?;
    let (expected_manifest_path, expected_image_path) = paths(expected.name.as_str());
    if expected.name.is_empty()
        || expected.name.contains('/')
        || expected.manifest_path != expected_manifest_path
        || expected.image_path != expected_image_path
    {
        return Err(Error::InvalidArtifact);
    }
    if crate::r::fs::trueosfs::file_info_async(artifact_root, expected.manifest_path.as_str())
        .await?
        .is_none_or(|info| info.data_len != backup_manifest::LEN as u64)
    {
        return Err(Error::InvalidArtifact);
    }
    let Some(manifest) =
        crate::r::fs::trueosfs::file_out_async(artifact_root, expected.manifest_path.as_str())
            .await?
    else {
        return Err(Error::InvalidArtifact);
    };
    let artifact = decode_manifest(expected.name.clone(), &manifest)?;
    if artifact != *expected {
        return Err(Error::InvalidArtifact);
    }
    if target.block_size() != artifact.block_size
        || target.block_count() != artifact.source_blocks
        || !target.supports_write()
    {
        return Err(Error::Block(block::Error::InvalidParam));
    }
    let Some(read_handle) =
        crate::r::fs::trueosfs::file_read_open_async(artifact_root, artifact.image_path.as_str())
            .await?
    else {
        return Err(Error::InvalidArtifact);
    };
    if read_handle.data_len() != artifact.image_bytes {
        return Err(Error::InvalidArtifact);
    }
    // The handle pins this log record. The lease additionally prevents a raw
    // overwrite or filesystem mutation through the complete verify/restore.
    let artifact_lease = acquire_backup(artifact_root, progress).await?;
    if !crate::r::fs::trueosfs::file_read_handle_is_current(read_handle) {
        return Err(Error::InvalidArtifact);
    }
    let _artifact_mount = crate::r::fs::trueosfs::suspend_backup_mount(artifact_root.id());
    artifact_lease.flush().await?;
    let chunk = chunk_bytes(target)?;
    let mut buffer = vec![0u8; chunk];
    let mut hasher = Sha256::new();
    let mut offset = 0u64;
    while offset < artifact.image_bytes {
        invoke(progress, ProgressPhase::Verifying, offset, artifact.image_bytes)?;
        let count = (artifact.image_bytes - offset).min(chunk as u64) as usize;
        let got = crate::r::fs::trueosfs::file_read_handle_range_backup_lease_async(
            read_handle,
            &artifact_lease,
            offset,
            &mut buffer[..count],
        )
        .await?;
        if got != Some(count) {
            return Err(Error::InvalidArtifact);
        }
        hasher.update(&buffer[..count]);
        offset += count as u64;
    }
    invoke(progress, ProgressPhase::Verifying, artifact.image_bytes, artifact.image_bytes)?;
    if hasher.finalize().as_slice() != artifact.sha256 {
        return Err(Error::InvalidArtifact);
    }

    let lease = acquire_restore(target, progress).await?;
    let mut mount = crate::r::fs::trueosfs::suspend_backup_mount(target.id());
    let mut destructive_attempted = false;
    let mut write_result = async {
        let mut offset = 0u64;
        while offset < artifact.image_bytes {
            invoke(progress, ProgressPhase::Restoring, offset, artifact.image_bytes)?;
            let count = (artifact.image_bytes - offset).min(chunk as u64) as usize;
            let got = crate::r::fs::trueosfs::file_read_handle_range_backup_lease_async(
                read_handle,
                &artifact_lease,
                offset,
                &mut buffer[..count],
            )
            .await?;
            if got != Some(count) {
                return Err(Error::InvalidArtifact);
            }
            // A failed driver command can still alter media. Mark the target
            // before awaiting it so stale mount state is never restored.
            destructive_attempted = true;
            lease
                .write(offset / artifact.block_size as u64, &buffer[..count])
                .await?;
            offset += count as u64;
        }
        invoke(progress, ProgressPhase::Restoring, artifact.image_bytes, artifact.image_bytes)?;
        lease.flush().await?;
        Ok(())
    }
    .await;
    if destructive_attempted {
        if let Err(error) = lease.flush().await {
            if write_result.is_ok() {
                write_result = Err(Error::Block(error));
            }
        }
        mount.discard_after_raw_overwrite();
    }
    drop(mount);
    drop(lease);
    if destructive_attempted {
        crate::r::fs::trueosfs::request_mount_root(target);
    }
    write_result?;
    Ok(RestoreReport {
        restored_bytes: artifact.image_bytes,
        target_blocks: target.block_count(),
    })
}
