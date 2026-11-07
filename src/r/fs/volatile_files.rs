//! Codex app storage with a durable `.codex` home and boot-local supporting files.
//! The logical paths stay unchanged; unrelated paths use the primary disk.

use alloc::{string::String, vec::Vec};
use embassy_sync::mutex::Mutex as AsyncMutex;
use spin::Mutex;

use super::trueosfs as fs;
use crate::disc::block;
use crate::r::io::kfs::{FsError, Result};

// Sparse: logical capacity reserves address space without allocating unused blocks.
const CAPACITY_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const COPY_CHUNK_BYTES: usize = 64 * 1024;
static PREPARE: AsyncMutex<crate::wait::EmbassySpinRawMutex, ()> = AsyncMutex::new(());
#[derive(Clone, Copy)]
struct Storage {
    disk: block::DeviceHandle,
    initialized: bool,
}
static STORAGE: Mutex<Option<Storage>> = Mutex::new(None);
static ROOTS: Mutex<Vec<String>> = Mutex::new(Vec::new());

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct SeedStats {
    pub files: usize,
    pub bytes: u64,
}

fn contains(root: &str, path: &str) -> bool {
    path == root
        || path
            .strip_prefix(root)
            .is_some_and(|tail| tail.starts_with('/'))
}

/// A flat Codex container home, including the bare container-0 directory.
/// Prefixes end at the underscore, so unrelated apps such as codex-tools do
/// not inherit Codex's supporting-file storage policy.
fn codex_root(path: &str) -> Option<&str> {
    let component = path.strip_prefix("apps/")?.split('/').next()?;
    let supported = matches!(component, "codex" | "codex-check")
        || ["codex_", "codex-check_"].into_iter().any(|prefix| {
            component.strip_prefix(prefix).is_some_and(|id| !id.is_empty())
        });
    supported.then(|| &path[.."apps/".len() + component.len()])
}

/// Each container's Codex home is durable. Match slash-delimited children,
/// never similarly prefixed names such as `.codex-backup`.
fn persistent_path(path: &str) -> bool {
    let Some(root) = codex_root(path) else { return false; };
    path.strip_prefix(root).and_then(|tail| tail.strip_prefix('/'))
        .is_some_and(|relative| contains(".codex", relative))
}

/// `path` must already be normalized and scoped by the app filesystem boundary.
pub(crate) fn disk_for_path(path: &str) -> Option<block::DeviceHandle> {
    if persistent_path(path) {
        return None;
    }
    let roots = ROOTS.lock();
    if roots.iter().any(|root| contains(root, path)) {
        STORAGE.lock().map(|storage| storage.disk)
    } else {
        None
    }
}

/// Select the same backing store for sync std, async std/Tokio, and file locks.
pub(crate) fn selected_disk(path: &str) -> Option<block::DeviceHandle> {
    disk_for_path(path).or_else(fs::primary_root_handle)
}

async fn create_home_directories(disk: block::DeviceHandle, home: &str) -> Result<()> {
    for leaf in [
        ".codex",
        "tmp",
        "config",
        "cache",
        ".local/share",
        ".local/state",
    ] {
        if !fs::dir_create_all_async(disk, &alloc::format!("{home}/{leaf}")).await? {
            return Err(FsError::NoSpace);
        }
    }
    Ok(())
}

/// Mirror a trusted app-root identity into an already prepared RAM workspace.
/// The durable root is stamped separately by its owner. A later preparation
/// copies that committed root key before creating any supporting children.
pub(crate) async fn stamp_workspace_key(root: &str, key: fs::RecordKey) -> Result<()> {
    let Some(disk) = disk_for_path(root) else { return Ok(()); };
    if !fs::stamp_directory_key_async(disk, root, key).await? { return Err(FsError::NoSpace); }
    Ok(())
}

async fn inherit_workspace_key(source: block::DeviceHandle, destination: block::DeviceHandle, root: &str) -> Result<()> {
    let node = fs::node_info_async(source, root).await?.ok_or(FsError::NotFound)?;
    if matches!(node.record_key, fs::RecordKey::Key(_))
        && !fs::stamp_directory_key_async(destination, root, node.record_key).await?
    { return Err(FsError::NoSpace); }
    Ok(())
}

async fn ensure_durable_codex_home(disk: block::DeviceHandle, root: &str) -> Result<()> {
    if !fs::dir_create_all_async(disk, &alloc::format!("{root}/.codex")).await? {
        return Err(FsError::NoSpace);
    }
    Ok(())
}

async fn cooperate() {
    let mut yielded = false;
    core::future::poll_fn(|cx| {
        if core::mem::replace(&mut yielded, true) {
            core::task::Poll::Ready(())
        } else {
            cx.waker().wake_by_ref();
            core::task::Poll::Pending
        }
    })
    .await;
}

/// Called exclusively through the BSP request broker before app code executes.
/// There is deliberately no disk fallback after a preparation error.
pub(crate) async fn prepare(root: &str) -> Result<SeedStats> {
    let home = crate::r::path::FsPath::parse(root, false)
        .map_err(|_| FsError::BadPath)?
        .to_relative_string();
    // Seed this exact container once. A later container-0 launch must never
    // overwrite a live additional container's supporting files in RAM.
    let root = codex_root(&home)
        .filter(|root| *root == home.as_str())
        .ok_or(FsError::BadPath)?;
    let _prepare = PREPARE.lock().await;
    let source = fs::primary_root_handle().ok_or(FsError::NoRoot)?;
    // Ensure the persistent home exists before any early return or volatile
    // seeding. Never copy an old `.codex` snapshot into the RAM placeholder.
    ensure_durable_codex_home(source, root).await?;
    if let Some(disk) = disk_for_path(root) {
        inherit_workspace_key(source, disk, root).await?;
        create_home_directories(disk, &home).await?;
        return Ok(SeedStats::default());
    }
    let existing = *STORAGE.lock();
    let storage = if let Some(storage) = existing {
        storage
    } else {
        let disk = crate::r::disc::ramdisk::create_private(CAPACITY_BYTES, 512, "codex-files")?;
        // Remember even an incomplete attempt, so retries never leak devices.
        let storage = Storage {
            disk,
            initialized: false,
        };
        *STORAGE.lock() = Some(storage);
        storage
    };
    let destination = storage.disk;
    if !storage.initialized {
        fs::format_blank_at_async(destination, 0).await?;
        if fs::mount_root_async(destination).await?.is_none() {
            return Err(FsError::Device(block::Error::NotReady));
        }
        *STORAGE.lock() = Some(Storage {
            disk: destination,
            initialized: true,
        });
    }
    let _snapshot = fs::read_snapshot_lease_async(source).await?;
    // A failed seed is never published. Retry replaces only its private subtree.
    fs::remove_recursive_async(destination, root).await?;
    if !fs::dir_create_all_async(destination, root).await? {
        return Err(FsError::NoSpace);
    }
    inherit_workspace_key(source, destination, root).await?;
    let mut stats = SeedStats::default();
    let mut directories = alloc::vec![String::from(root)];
    let mut buffer = alloc::vec![0u8; COPY_CHUNK_BYTES];
    while let Some(directory) = directories.pop() {
        cooperate().await;
        let Some(listing) = fs::list_dir_with_limit_async(source, &directory, usize::MAX).await?
        else {
            continue;
        };
        if listing.truncated {
            return Err(FsError::Device(block::Error::Corrupted));
        }
        for entry in listing.entries {
            let path = alloc::format!("{directory}/{}", entry.name);
            if persistent_path(&path) {
                continue;
            }
            if entry.kind == fs::NodeKind::Directory {
                if !fs::dir_create_all_async(destination, &path).await? {
                    return Err(FsError::NoSpace);
                }
                directories.push(path);
                continue;
            }
            let info = fs::file_info_async(source, &path)
                .await?
                .ok_or(FsError::NotFound)?;
            let read = fs::file_read_open_async(source, &path)
                .await?
                .ok_or(FsError::NotFound)?;
            let write = fs::file_write_begin_with_metadata_async(
                destination,
                &path,
                info.data_len,
                info.content_type,
                info.record_key,
                false,
            )
            .await?
            .ok_or(FsError::NoSpace)?;
            let copied = async {
                let mut offset = 0;
                while offset < info.data_len {
                    let count = (info.data_len - offset).min(buffer.len() as u64) as usize;
                    let count =
                        fs::file_read_handle_range_async(read, offset, &mut buffer[..count])
                            .await?
                            .filter(|count| *count != 0)
                            .ok_or(FsError::NotFound)?;
                    fs::file_write_chunk_async(write, &buffer[..count]).await?;
                    offset += count as u64;
                    cooperate().await;
                }
                fs::file_write_finish_async(write).await?;
                Ok::<_, FsError>(())
            }
            .await;
            if let Err(error) = copied {
                let _ = fs::file_write_abort_async(write).await;
                return Err(error);
            }
            stats.files += 1;
            stats.bytes += info.data_len;
        }
    }
    create_home_directories(destination, root).await?;
    create_home_directories(destination, &home).await?;
    // The source stays untouched. Only now can normal file operations see RAM.
    ROOTS.lock().push(String::from(root));
    crate::log_info!(target: "filesystem";
        "codex-files: ready path={} source_disc={} ram_disc={} files={} bytes={} capacity={} persistence=mixed durable_codex_home={}/.codex\n",
        root, source.id().raw(), destination.id().raw(), stats.files, stats.bytes, CAPACITY_BYTES, root
    );
    Ok(stats)
}
