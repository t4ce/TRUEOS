use alloc::string::String;
use alloc::boxed::Box;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::disc::block;
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;
use trueos_math::Tree;

static FILES_SCAN_REQUESTS: AtomicU32 = AtomicU32::new(0);
static FILE_TREE_SEQ: AtomicU32 = AtomicU32::new(0);

const FILE_TREE_CAP: usize = 2048;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileTreeKind {
    Root,
    Device,
    Dir,
    File,
    TrueosFs,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileTreeEntry {
    pub kind: FileTreeKind,
    pub name: String,
}

pub type FileTree = Tree<FileTreeEntry, FILE_TREE_CAP>;

struct FileTreeCache {
    seq: u32,
    tree: Option<Box<FileTree>>,
}

static FILE_TREE_CACHE: Mutex<FileTreeCache> = Mutex::new(FileTreeCache { seq: 0, tree: None });

/// Request a filesystem tree scan/log from the dedicated files service task.
///
/// This is intentionally non-blocking; the heavy work happens in the embassy task.
pub fn request_files_scan() {
    FILES_SCAN_REQUESTS.fetch_add(1, Ordering::Release);
}

pub fn file_tree_len() -> usize {
    with_latest_file_tree(|_seq, tree| tree.len()).unwrap_or(0)
}

pub fn file_tree_seq() -> u32 {
    FILE_TREE_CACHE.lock().seq
}

pub fn with_latest_file_tree<R>(f: impl FnOnce(u32, &FileTree) -> R) -> Option<R> {
    let guard = FILE_TREE_CACHE.lock();
    guard.tree.as_deref().map(|t| f(guard.seq, t))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbFsReadError {
    UsbmsNotFound,
    DeviceIo(block::Error),
    MountFailed,
    OpenFailed,
    ReadFailed,
    TooLarge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbFsWriteError {
    UsbmsNotFound,
    DeviceIo(block::Error),
    MountFailed,
    BadPath,
    DirFailed,
    OpenFailed,
    WriteFailed,
    TooLarge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbFsRenameError {
    UsbmsNotFound,
    DeviceIo(block::Error),
    MountFailed,
    BadPath,
    NotFound,
    AlreadyExists,
    RenameFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbFsListDirError {
    UsbmsNotFound,
    DeviceIo(block::Error),
    MountFailed,
    BadPath,
    OpenFailed,
    IterFailed,
    TooLarge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbFsRemoveError {
    UsbmsNotFound,
    DeviceIo(block::Error),
    MountFailed,
    BadPath,
    NotFound,
    NotEmpty,
    RemoveFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsError {
    Read(UsbFsReadError),
    Write(UsbFsWriteError),
    Rename(UsbFsRenameError),
    ListDir(UsbFsListDirError),
    Remove(UsbFsRemoveError),
}

/// Minimal filesystem binding backed by the USBMS FAT volume.
///
/// Intended to map to higher-level APIs like `fs.readFile()` / `fs.writeFile()`.
pub struct Fs;

static TRUEOSFS_ROOT: Mutex<Option<block::DiscId>> = Mutex::new(None);

fn pick_trueosfs_root() -> Option<block::DeviceHandle> {
    // Fast path: cached ID.
    if let Some(id) = *TRUEOSFS_ROOT.lock() {
        if let Some(h) = block::device_handle(id) {
            return Some(h);
        }
        // Device disappeared; clear and rescan.
        *TRUEOSFS_ROOT.lock() = None;
    }

    // Slow path: scan whole-disk devices for TRUEOSFS.
    for h in block::device_handles().into_iter() {
        if h.parent().is_some() {
            continue;
        }
        if crate::disc::trueosfs::locate(h).ok().flatten().is_some() {
            let _ = crate::disc::trueosfs::mount_root(h);
            *TRUEOSFS_ROOT.lock() = Some(h.id());
            return Some(h);
        }
    }
    None
}

#[inline]
fn norm_rel(path: &str) -> Result<String, ()> {
    crate::path::normalize_rel_no_parent(path).ok_or(())
}

#[inline]
fn norm_rel_nonempty(path: &str) -> Result<String, ()> {
    let rel = norm_rel(path)?;
    if rel.is_empty() {
        return Err(());
    }
    Ok(rel)
}

impl Fs {
    #[inline]
    pub fn read_file(path: &str) -> Result<alloc::vec::Vec<u8>, FsError> {
        if let Some(disk) = pick_trueosfs_root() {
            let rel = norm_rel_nonempty(path).map_err(|_| FsError::Read(UsbFsReadError::OpenFailed))?;
            match crate::disc::trueosfs::file_out(disk, rel.as_str()) {
                Ok(Some(v)) => return Ok(v),
                Ok(None) => return Err(FsError::Read(UsbFsReadError::OpenFailed)),
                Err(e) => return Err(FsError::Read(UsbFsReadError::DeviceIo(e))),
            }
        }

        Err(FsError::Read(UsbFsReadError::UsbmsNotFound))
    }

    #[inline]
    pub fn write_file(path: &str, data: &[u8]) -> Result<(), FsError> {
        if data.len() > MAX_WRITE_BYTES {
            return Err(FsError::Write(UsbFsWriteError::TooLarge));
        }

        if let Some(disk) = pick_trueosfs_root() {
            let rel = norm_rel_nonempty(path).map_err(|_| FsError::Write(UsbFsWriteError::BadPath))?;
            match crate::disc::trueosfs::file_in(disk, rel.as_str(), data) {
                Ok(true) => return Ok(()),
                Ok(false) => return Err(FsError::Write(UsbFsWriteError::WriteFailed)),
                Err(e) => return Err(FsError::Write(UsbFsWriteError::DeviceIo(e))),
            }
        }

        Err(FsError::Write(UsbFsWriteError::UsbmsNotFound))
    }

    #[inline]
    pub fn rename(src_path: &str, dst_path: &str) -> Result<(), FsError> {
        if let Some(disk) = pick_trueosfs_root() {
            let src = norm_rel_nonempty(src_path).map_err(|_| FsError::Rename(UsbFsRenameError::BadPath))?;
            let dst = norm_rel_nonempty(dst_path).map_err(|_| FsError::Rename(UsbFsRenameError::BadPath))?;

            // Mirror FAT-ish semantics.
            match crate::disc::trueosfs::file_out(disk, src.as_str()) {
                Ok(Some(bytes)) => {
                    match crate::disc::trueosfs::file_exists(disk, dst.as_str()) {
                        Ok(true) => return Err(FsError::Rename(UsbFsRenameError::AlreadyExists)),
                        Ok(false) => {}
                        Err(e) => return Err(FsError::Rename(UsbFsRenameError::DeviceIo(e))),
                    }

                    match crate::disc::trueosfs::file_in(disk, dst.as_str(), bytes.as_slice()) {
                        Ok(true) => {}
                        Ok(false) => return Err(FsError::Rename(UsbFsRenameError::RenameFailed)),
                        Err(e) => return Err(FsError::Rename(UsbFsRenameError::DeviceIo(e))),
                    }

                    match crate::disc::trueosfs::file_delete(disk, src.as_str()) {
                        Ok(true) => return Ok(()),
                        Ok(false) => return Err(FsError::Rename(UsbFsRenameError::NotFound)),
                        Err(e) => return Err(FsError::Rename(UsbFsRenameError::DeviceIo(e))),
                    }
                }
                Ok(None) => return Err(FsError::Rename(UsbFsRenameError::NotFound)),
                Err(e) => return Err(FsError::Rename(UsbFsRenameError::DeviceIo(e))),
            }
        }

        Err(FsError::Rename(UsbFsRenameError::UsbmsNotFound))
    }

    #[inline]
    pub fn list_dir(path: &str) -> Result<String, FsError> {
        if let Some(disk) = pick_trueosfs_root() {
            match crate::disc::trueosfs::list_dir(disk, path) {
                Ok(Some(v)) => return Ok(v),
                Ok(None) => {}
                Err(e) => return Err(FsError::ListDir(UsbFsListDirError::DeviceIo(e))),
            }
        }

        Err(FsError::ListDir(UsbFsListDirError::UsbmsNotFound))
    }

    #[inline]
    pub fn remove(path: &str) -> Result<(), FsError> {
        if let Some(disk) = pick_trueosfs_root() {
            let rel = norm_rel_nonempty(path).map_err(|_| FsError::Remove(UsbFsRemoveError::BadPath))?;
            match crate::disc::trueosfs::file_delete(disk, rel.as_str()) {
                Ok(true) => return Ok(()),
                Ok(false) => return Err(FsError::Remove(UsbFsRemoveError::NotFound)),
                Err(e) => return Err(FsError::Remove(UsbFsRemoveError::DeviceIo(e))),
            }
        }

        Err(FsError::Remove(UsbFsRemoveError::UsbmsNotFound))
    }

    #[inline]
    pub fn exists(path: &str) -> Result<bool, FsError> {
        if let Some(disk) = pick_trueosfs_root() {
            let rel = norm_rel_nonempty(path).map_err(|_| FsError::Read(UsbFsReadError::OpenFailed))?;
            match crate::disc::trueosfs::file_exists(disk, rel.as_str()) {
                Ok(v) => return Ok(v),
                Err(e) => return Err(FsError::Read(UsbFsReadError::DeviceIo(e))),
            }
        }

        Err(FsError::Read(UsbFsReadError::UsbmsNotFound))
    }

    #[inline]
    pub fn create_dir_all(path: &str) -> Result<(), FsError> {
        if pick_trueosfs_root().is_some() {
            // TRUEOSFS stores flat keys; treat directory creation as a no-op.
            // Still validate paths to avoid surprising behavior.
            let _ = norm_rel(path).map_err(|_| FsError::Write(UsbFsWriteError::BadPath))?;
            return Ok(());
        }

        Err(FsError::Write(UsbFsWriteError::UsbmsNotFound))
    }

    #[inline]
    pub fn append_file(path: &str, data: &[u8]) -> Result<(), FsError> {
        if data.is_empty() {
            return Ok(());
        }
        if data.len() > MAX_WRITE_BYTES {
            return Err(FsError::Write(UsbFsWriteError::TooLarge));
        }

        if let Some(disk) = pick_trueosfs_root() {
            let rel = norm_rel_nonempty(path).map_err(|_| FsError::Write(UsbFsWriteError::BadPath))?;
            match crate::disc::trueosfs::file_append(disk, rel.as_str(), data) {
                Ok(true) => return Ok(()),
                Ok(false) => return Err(FsError::Write(UsbFsWriteError::WriteFailed)),
                Err(e) => return Err(FsError::Write(UsbFsWriteError::DeviceIo(e))),
            }
        }

        Err(FsError::Write(UsbFsWriteError::UsbmsNotFound))
    }
}

// These caps exist to keep memory usage bounded for filesystem operations.
// Some boot-cached assets (e.g. pci.ids) are ~1.6 MiB, so keep this comfortably above that.
const MAX_WRITE_BYTES: usize = 4 * 1024 * 1024;

async fn build_tree_for_device_async(
    tree: &mut FileTree,
    parent: trueos_math::NodeId,
    handle: block::DeviceHandle,
) {
    let info = handle.info();
    let dev_name = alloc::format!("{}", info.id);
    let dev_id = match tree.add_child(
        parent,
        FileTreeEntry {
            kind: FileTreeKind::Device,
            name: dev_name,
        },
    ) {
        Some(id) => id,
        None => {
            crate::log!("files: tree full; cannot add device\n");
            return;
        }
    };

    // TRUEOSFS: model as 1 root per *whole disk* (no slicing at this layer).
    if info.parent.is_none() {
        if crate::disc::trueosfs::locate(handle).ok().flatten().is_some() {
            let _ = crate::disc::trueosfs::mount_root(handle);

            if let Some(tfs_id) = tree.add_child(
                dev_id,
                FileTreeEntry {
                    kind: FileTreeKind::TrueosFs,
                    name: String::from("TRUEOSFS"),
                },
            ) {
                let _ = tree.add_child(
                    tfs_id,
                    FileTreeEntry {
                        kind: FileTreeKind::Root,
                        name: String::from("/"),
                    },
                );
            }
        }
    }
}

async fn scan_all_devices_and_build_tree(tree: &mut FileTree, root: trueos_math::NodeId) {
    let devices = block::device_handles();
    if devices.is_empty() {
        crate::log!("files: no block devices found\n");
        return;
    }

    crate::log!("files: scanning {} device(s)\n", devices.len());
    for dev in devices.into_iter() {
        build_tree_for_device_async(tree, root, dev).await;
        Timer::after(EmbassyDuration::from_millis(1)).await;
    }
}

#[embassy_executor::task]
pub async fn files_service_task() {
    // Dedicated on-demand service. Heavy scanning is performed only when requested.
    crate::log!("files: service online\n");

    // One-time boot scan (best-effort). Keep the wait bounded so boot bringup isn't held.
    for _ in 0..50 {
        if !block::device_handles().is_empty() {
            break;
        }
        Timer::after(EmbassyDuration::from_millis(100)).await;
    }
    crate::log!("files: boot scan\n");
    {
        let mut tree = FileTree::new();
        let Some(root) = tree.add_root(FileTreeEntry {
            kind: FileTreeKind::Root,
            name: String::from("files"),
        }) else {
            crate::log!("files: tree init failed\n");
            return;
        };
        scan_all_devices_and_build_tree(&mut tree, root).await;
        let seq = FILE_TREE_SEQ.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
        let mut cache = FILE_TREE_CACHE.lock();
        cache.seq = seq;
        cache.tree = Some(Box::new(tree));
        crate::log!(
            "files: tree built seq={} nodes={} cap={}\n",
            cache.seq,
            cache.tree.as_ref().map(|t| t.len()).unwrap_or(0),
            FILE_TREE_CAP
        );
    }

    loop {
        let pending = FILES_SCAN_REQUESTS.swap(0, Ordering::AcqRel);
        if pending == 0 {
            Timer::after(EmbassyDuration::from_millis(100)).await;
            continue;
        }

        crate::log!("files: scan requested (pending={})\n", pending);
        let mut tree = FileTree::new();
        let Some(root) = tree.add_root(FileTreeEntry {
            kind: FileTreeKind::Root,
            name: String::from("files"),
        }) else {
            crate::log!("files: tree init failed\n");
            continue;
        };
        scan_all_devices_and_build_tree(&mut tree, root).await;
        let seq = FILE_TREE_SEQ.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
        let mut cache = FILE_TREE_CACHE.lock();
        cache.seq = seq;
        cache.tree = Some(Box::new(tree));
        crate::log!(
            "files: tree rebuilt seq={} nodes={} cap={}\n",
            cache.seq,
            cache.tree.as_ref().map(|t| t.len()).unwrap_or(0),
            FILE_TREE_CAP
        );
    }
}
