use alloc::string::String;
use alloc::vec::Vec;
use alloc::alloc::{alloc, dealloc, Layout};
use alloc::boxed::Box;
use core::sync::atomic::{AtomicU32, Ordering};

use fatfs::{
    format_volume, FileSystem, FormatVolumeOptions, FsOptions, IoBase, Read, Seek, SeekFrom, Write,
};

use crate::disc::{block, layout};
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

impl Fs {
    #[inline]
    pub fn read_file(path: &str) -> Result<alloc::vec::Vec<u8>, FsError> {
        read_usbms_file(path).map_err(FsError::Read)
    }

    #[inline]
    pub fn write_file(path: &str, data: &[u8]) -> Result<(), FsError> {
        write_usbms_file(path, data).map_err(FsError::Write)
    }

    #[inline]
    pub fn rename(src_path: &str, dst_path: &str) -> Result<(), FsError> {
        rename_usbms_path(src_path, dst_path).map_err(FsError::Rename)
    }

    #[inline]
    pub fn list_dir(path: &str) -> Result<String, FsError> {
        list_usbms_dir(path).map_err(FsError::ListDir)
    }

    #[inline]
    pub fn remove(path: &str) -> Result<(), FsError> {
        remove_usbms_path(path).map_err(FsError::Remove)
    }

    #[inline]
    pub fn exists(path: &str) -> Result<bool, FsError> {
        usbms_path_exists(path).map_err(FsError::Read)
    }

    /// Create a directory path recursively (mkdir -p semantics).
    ///
    /// Note: errors are reported via `FsError::Write` to avoid introducing a
    /// separate error category for now.
    #[inline]
    pub fn create_dir_all(path: &str) -> Result<(), FsError> {
        create_usbms_dir_all(path).map_err(FsError::Write)
    }
}

// These caps exist to keep memory usage bounded for filesystem operations.
// Some boot-cached assets (e.g. pci.ids) are ~1.6 MiB, so keep this comfortably above that.
const MAX_READ_BYTES: usize = 4 * 1024 * 1024;
const MAX_WRITE_BYTES: usize = 4 * 1024 * 1024;

pub fn read_usbms_file(path: &str) -> Result<alloc::vec::Vec<u8>, UsbFsReadError> {
    let Some(handle) = pick_usbms_device() else {
        return Err(UsbFsReadError::UsbmsNotFound);
    };

    let io = BlockDeviceIo::new(handle).map_err(UsbFsReadError::DeviceIo)?;
    let fs = FileSystem::new(io, FsOptions::new()).map_err(|_| UsbFsReadError::MountFailed)?;

    let rel = match crate::path::normalize_rel_no_parent(path) {
        Some(p) => p,
        None => {
            let _ = fs.unmount();
            return Err(UsbFsReadError::OpenFailed);
        }
    };
    if rel.is_empty() {
        let _ = fs.unmount();
        return Err(UsbFsReadError::OpenFailed);
    }

    let res = {
        let root = fs.root_dir();
        let mut file = root.open_file(rel.as_str()).map_err(|_| UsbFsReadError::OpenFailed)?;

        let mut out = alloc::vec::Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let n = file.read(&mut buf).map_err(|_| UsbFsReadError::ReadFailed)?;
            if n == 0 {
                break;
            }
            if out.len().saturating_add(n) > MAX_READ_BYTES {
                return Err(UsbFsReadError::TooLarge);
            }
            out.extend_from_slice(&buf[..n]);
        }
        Ok(out)
    };

    let _ = fs.unmount();
    res
}

pub fn usbms_path_exists(path: &str) -> Result<bool, UsbFsReadError> {
    let Some(handle) = pick_usbms_device() else {
        return Err(UsbFsReadError::UsbmsNotFound);
    };

    let io = BlockDeviceIo::new(handle).map_err(UsbFsReadError::DeviceIo)?;
    let fs = FileSystem::new(io, FsOptions::new()).map_err(|_| UsbFsReadError::MountFailed)?;

    let rel = match crate::path::normalize_rel_no_parent(path) {
        Some(p) => p,
        None => {
            let _ = fs.unmount();
            return Err(UsbFsReadError::OpenFailed);
        }
    };
    if rel.is_empty() {
        let _ = fs.unmount();
        return Err(UsbFsReadError::OpenFailed);
    }

    let res = {
        let root = fs.root_dir();
        match root.open_file(rel.as_str()) {
            Ok(_f) => Ok(true),
            Err(fatfs::Error::NotFound) => Ok(false),
            Err(_e) => Err(UsbFsReadError::OpenFailed),
        }
    };

    let _ = fs.unmount();
    res
}

pub fn write_usbms_file(path: &str, bytes: &[u8]) -> Result<(), UsbFsWriteError> {
    if bytes.len() > MAX_WRITE_BYTES {
        return Err(UsbFsWriteError::TooLarge);
    }

    let Some(handle) = pick_usbms_device() else {
        return Err(UsbFsWriteError::UsbmsNotFound);
    };

    let io = BlockDeviceIo::new(handle).map_err(UsbFsWriteError::DeviceIo)?;
    let fs = FileSystem::new(io, FsOptions::new()).map_err(|_| UsbFsWriteError::MountFailed)?;

    let rel = match crate::path::normalize_rel_no_parent(path) {
        Some(p) => p,
        None => {
            let _ = fs.unmount();
            return Err(UsbFsWriteError::BadPath);
        }
    };
    if rel.is_empty() {
        let _ = fs.unmount();
        return Err(UsbFsWriteError::BadPath);
    }

    let res = {
        let root = fs.root_dir();

        // Split into parent dirs and file name.
        let mut parts = rel.split('/').filter(|p| !p.is_empty());
        let mut comps: alloc::vec::Vec<&str> = alloc::vec::Vec::new();
        for p in parts.by_ref() {
            comps.push(p);
        }
        if comps.is_empty() {
            return Err(UsbFsWriteError::BadPath);
        }
        let file_name = comps.pop().ok_or(UsbFsWriteError::BadPath)?;
        if file_name.is_empty() {
            return Err(UsbFsWriteError::BadPath);
        }

        // Walk/create directories.
        let mut dir = root;
        for seg in comps.into_iter() {
            if seg.is_empty() {
                continue;
            }
            match dir.open_dir(seg) {
                Ok(next) => dir = next,
                Err(fatfs::Error::NotFound) => {
                    match dir.create_dir(seg) {
                        Ok(_) => {}
                        Err(fatfs::Error::AlreadyExists) => {}
                        Err(_) => return Err(UsbFsWriteError::DirFailed),
                    }
                    dir = dir.open_dir(seg).map_err(|_| UsbFsWriteError::DirFailed)?;
                }
                Err(_) => return Err(UsbFsWriteError::DirFailed),
            }
        }

        // Open existing or create new.
        let mut file = match dir.open_file(file_name) {
            Ok(f) => f,
            Err(fatfs::Error::NotFound) => dir.create_file(file_name).map_err(|_| UsbFsWriteError::OpenFailed)?,
            Err(_) => return Err(UsbFsWriteError::OpenFailed),
        };

        file.seek(SeekFrom::Start(0)).map_err(|_| UsbFsWriteError::WriteFailed)?;
        let _ = file.truncate();
        file.write_all(bytes).map_err(|_| UsbFsWriteError::WriteFailed)?;
        file.flush().map_err(|_| UsbFsWriteError::WriteFailed)?;
        Ok(())
    };

    let _ = fs.unmount();
    res
}

/// Create directories for `path` recursively (mkdir -p).
///
/// Accepts both absolute (`/qjs/cdn`) and relative (`qjs/cdn`) paths.
/// The empty path is treated as a no-op.
pub fn create_usbms_dir_all(path: &str) -> Result<(), UsbFsWriteError> {
    let Some(handle) = pick_usbms_device() else {
        return Err(UsbFsWriteError::UsbmsNotFound);
    };

    let io = BlockDeviceIo::new(handle).map_err(UsbFsWriteError::DeviceIo)?;
    let fs = FileSystem::new(io, FsOptions::new()).map_err(|_| UsbFsWriteError::MountFailed)?;

    let rel = match crate::path::normalize_rel_no_parent(path) {
        Some(p) => p,
        None => {
            let _ = fs.unmount();
            return Err(UsbFsWriteError::BadPath);
        }
    };

    let res = {
        let root = fs.root_dir();
        let mut dir = root;

        if !rel.is_empty() {
            for seg in rel.split('/').filter(|s| !s.is_empty()) {
                match dir.open_dir(seg) {
                    Ok(next) => dir = next,
                    Err(fatfs::Error::NotFound) => {
                        match dir.create_dir(seg) {
                            Ok(_) => {}
                            Err(fatfs::Error::AlreadyExists) => {}
                            Err(_) => return Err(UsbFsWriteError::DirFailed),
                        }
                        dir = dir.open_dir(seg).map_err(|_| UsbFsWriteError::DirFailed)?;
                    }
                    Err(_) => return Err(UsbFsWriteError::DirFailed),
                }
            }
        }

        Ok(())
    };

    let _ = fs.unmount();
    res
}

pub fn rename_usbms_path(src_path: &str, dst_path: &str) -> Result<(), UsbFsRenameError> {
    let Some(handle) = pick_usbms_device() else {
        return Err(UsbFsRenameError::UsbmsNotFound);
    };

    let io = BlockDeviceIo::new(handle).map_err(UsbFsRenameError::DeviceIo)?;
    let fs = FileSystem::new(io, FsOptions::new()).map_err(|_| UsbFsRenameError::MountFailed)?;

    let src_rel = match crate::path::normalize_rel_no_parent(src_path) {
        Some(p) => p,
        None => {
            let _ = fs.unmount();
            return Err(UsbFsRenameError::BadPath);
        }
    };
    let dst_rel = match crate::path::normalize_rel_no_parent(dst_path) {
        Some(p) => p,
        None => {
            let _ = fs.unmount();
            return Err(UsbFsRenameError::BadPath);
        }
    };
    if src_rel.is_empty() || dst_rel.is_empty() {
        let _ = fs.unmount();
        return Err(UsbFsRenameError::BadPath);
    }

    // Ensure destination parent directories exist (best-effort, like write_file).
    let res = {
        let root = fs.root_dir();

        let parent = match dst_rel.rsplit_once('/') {
            Some((p, _name)) => p,
            None => "",
        };

        if !parent.is_empty() {
            let mut dir = root.clone();
            for seg in parent.split('/').filter(|s| !s.is_empty()) {
                match dir.open_dir(seg) {
                    Ok(next) => dir = next,
                    Err(fatfs::Error::NotFound) => {
                        match dir.create_dir(seg) {
                            Ok(_) => {}
                            Err(fatfs::Error::AlreadyExists) => {}
                            Err(_) => return Err(UsbFsRenameError::RenameFailed),
                        }
                        dir = dir.open_dir(seg).map_err(|_| UsbFsRenameError::RenameFailed)?;
                    }
                    Err(_) => return Err(UsbFsRenameError::RenameFailed),
                }
            }
        }

        match root.rename(src_rel.as_str(), &root, dst_rel.as_str()) {
            Ok(()) => Ok(()),
            Err(fatfs::Error::NotFound) => Err(UsbFsRenameError::NotFound),
            Err(fatfs::Error::AlreadyExists) => Err(UsbFsRenameError::AlreadyExists),
            Err(_) => Err(UsbFsRenameError::RenameFailed),
        }
    };

    let _ = fs.unmount();
    res
}

const MAX_LISTING_BYTES: usize = 64 * 1024;

pub fn list_usbms_dir(path: &str) -> Result<String, UsbFsListDirError> {
    let Some(handle) = pick_usbms_device() else {
        return Err(UsbFsListDirError::UsbmsNotFound);
    };

    let io = BlockDeviceIo::new(handle).map_err(UsbFsListDirError::DeviceIo)?;
    let fs = FileSystem::new(io, FsOptions::new()).map_err(|_| UsbFsListDirError::MountFailed)?;

    let rel = match crate::path::normalize_rel_no_parent(path) {
        Some(p) => p,
        None => {
            let _ = fs.unmount();
            return Err(UsbFsListDirError::BadPath);
        }
    };

    let res = {
        let root = fs.root_dir();
        let mut dir = root.clone();
        if !rel.is_empty() {
            for seg in rel.split('/').filter(|s| !s.is_empty()) {
                dir = dir.open_dir(seg).map_err(|_| UsbFsListDirError::OpenFailed)?;
            }
        }

        let mut out = String::new();
        for entry in dir.iter() {
            let entry = entry.map_err(|_| UsbFsListDirError::IterFailed)?;
            let name = entry.file_name();
            if name.is_empty() || name == "." || name == ".." {
                continue;
            }
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&name);
            if out.len() > MAX_LISTING_BYTES {
                return Err(UsbFsListDirError::TooLarge);
            }
        }
        Ok(out)
    };

    let _ = fs.unmount();
    res
}

pub fn remove_usbms_path(path: &str) -> Result<(), UsbFsRemoveError> {
    let Some(handle) = pick_usbms_device() else {
        return Err(UsbFsRemoveError::UsbmsNotFound);
    };

    let io = BlockDeviceIo::new(handle).map_err(UsbFsRemoveError::DeviceIo)?;
    let fs = FileSystem::new(io, FsOptions::new()).map_err(|_| UsbFsRemoveError::MountFailed)?;

    let rel = match crate::path::normalize_rel_no_parent(path) {
        Some(p) => p,
        None => {
            let _ = fs.unmount();
            return Err(UsbFsRemoveError::BadPath);
        }
    };
    if rel.is_empty() {
        let _ = fs.unmount();
        return Err(UsbFsRemoveError::BadPath);
    }

    let res = {
        let root = fs.root_dir();
        match root.remove(rel.as_str()) {
            Ok(()) => Ok(()),
            Err(fatfs::Error::NotFound) => Err(UsbFsRemoveError::NotFound),
            Err(fatfs::Error::DirectoryIsNotEmpty) => Err(UsbFsRemoveError::NotEmpty),
            Err(_) => Err(UsbFsRemoveError::RemoveFailed),
        }
    };

    let _ = fs.unmount();
    res
}

#[derive(Debug)]
struct RamDisk {
    data: Vec<u8>,
    pos: usize,
}

impl RamDisk {
    fn new(size: usize) -> Self {
        Self {
            data: alloc::vec![0_u8; size],
            pos: 0,
        }
    }
}

impl IoBase for RamDisk {
    type Error = ();
}

impl Read for RamDisk {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        if self.pos >= self.data.len() {
            return Ok(0);
        }
        let available = self.data.len() - self.pos;
        let to_copy = available.min(buf.len());
        buf[..to_copy].copy_from_slice(&self.data[self.pos..self.pos + to_copy]);
        self.pos += to_copy;
        Ok(to_copy)
    }
}

impl Write for RamDisk {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        if self.pos >= self.data.len() {
            return Err(());
        }
        let available = self.data.len() - self.pos;
        let to_copy = available.min(buf.len());
        if to_copy == 0 {
            return Err(());
        }
        self.data[self.pos..self.pos + to_copy].copy_from_slice(&buf[..to_copy]);
        self.pos += to_copy;
        Ok(to_copy)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl Seek for RamDisk {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64, Self::Error> {
        let len = self.data.len() as i64;
        let next = match pos {
            SeekFrom::Start(offset) => {
                if offset > self.data.len() as u64 {
                    return Err(());
                }
                offset as i64
            }
            SeekFrom::End(delta) => len.checked_add(delta).ok_or(())?,
            SeekFrom::Current(delta) => (self.pos as i64).checked_add(delta).ok_or(())?,
        };
        if next < 0 || next as usize > self.data.len() {
            return Err(());
        }
        self.pos = next as usize;
        Ok(self.pos as u64)
    }
}

pub fn create_demo_file() {
    crate::log!("files: ");
    const RAMDISK_BYTES: usize = 1024 * 1024; // 1 MiB scratch volume
    let mut ramdisk = RamDisk::new(RAMDISK_BYTES);

    if format_volume(&mut ramdisk, FormatVolumeOptions::new()).is_err() {
        return;
    }
    let Ok(fs) = FileSystem::new(ramdisk, FsOptions::new()) else {
        return;
    };

    let root = fs.root_dir();
    if let Ok(mut file) = root.create_file("helloworld") {
        let _ = file.write_all(b"hello from fatfs in-memory demo");
    }

    if let Ok(stats) = fs.stats() {
        crate::log!(
            "files: clusters total={} free={}",
            stats.total_clusters(),
            stats.free_clusters()
        );
    }
}

struct AlignedBuf {
    ptr: *mut u8,
    len: usize,
    layout: Layout,
}

impl AlignedBuf {
    fn new(len: usize, align: usize) -> Option<Self> {
        let layout = Layout::from_size_align(len, align).ok()?;
        let ptr = unsafe { alloc(layout) };
        if ptr.is_null() {
            return None;
        }
        Some(Self {
            ptr,
            len,
            layout,
        })
    }

    fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.ptr, self.len) }
    }
}

impl Drop for AlignedBuf {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe { dealloc(self.ptr, self.layout) };
        }
    }
}

struct BlockDeviceIo {
    dev: block::DeviceHandle,
    base_lba: u64,
    blocks: u64,
    pos: u64,
    block_size: usize,
    scratch: AlignedBuf,
    read_count: u64,
    last_logged_lba: u64,
    cached_lba: u64,
}

impl BlockDeviceIo {
    fn new(dev: block::DeviceHandle) -> Result<Self, block::Error> {
        let info = dev.info();
        let block_size = info.block_size as usize;
        if block_size == 0 {
            return Err(block::Error::InvalidParam);
        }

        let (base_lba, blocks) = match layout::probe_fat_volume(dev) {
            Ok(found) => {
                let (base, blks) = layout::fat_slice_for_mount(found, info.block_count);
                (base, blks)
            }
            Err(layout::ProbeError::UnsupportedBlockSize(_)) => {
                return Err(block::Error::InvalidParam);
            }
            Err(layout::ProbeError::DeviceIo(e)) => {
                return Err(e);
            }
            Err(layout::ProbeError::UnknownLayout) => {
                crate::log!(
                    "files: usbms: unknown disk layout (expected FAT@LBA0 or MBR+FAT partition); mount will fail\n"
                );
                return Err(block::Error::Corrupted);
            }
        };
        crate::log!(
            "files: usbms: FAT volume base_lba={} blocks={} (disk_blocks={})\n",
            base_lba,
            blocks,
            info.block_count
        );
        if blocks == 0 {
            return Err(block::Error::OutOfBounds);
        }

        let align = info.dma_alignment.max(1) as usize;
        let mut scratch = AlignedBuf::new(block_size, align).ok_or(block::Error::DmaUnavailable)?;
        for b in scratch.as_mut_slice().iter_mut() {
            *b = 0;
        }
        Ok(Self {
            dev,
            base_lba,
            blocks,
            pos: 0,
            block_size,
            scratch,
            read_count: 0,
            last_logged_lba: u64::MAX,
            cached_lba: u64::MAX,
        })
    }

    fn io_read_block(&mut self, lba: u64) -> Result<(), block::Error> {
        if self.cached_lba == lba {
            return Ok(());
        }
        if lba >= self.blocks {
            return Err(block::Error::OutOfBounds);
        }
        self.read_count = self.read_count.wrapping_add(1);
        // Keep this intentionally low-noise: FAT mount may read lots of LBAs.
        if self.read_count <= 8 {
            crate::log!("files: io_read_block #{} lba={}\n", self.read_count, lba);
            self.last_logged_lba = lba;
        }
        let abs_lba = self.base_lba.saturating_add(lba);
        self.dev.read_blocks(abs_lba, self.scratch.as_mut_slice())?;
        self.cached_lba = lba;
        Ok(())
    }
}

impl IoBase for BlockDeviceIo {
    type Error = block::Error;
}

impl Read for BlockDeviceIo {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        if buf.is_empty() {
            return Ok(0);
        }

        let mut total = 0usize;
        while total < buf.len() {
            let lba = self.pos / (self.block_size as u64);
            let block_off = (self.pos % (self.block_size as u64)) as usize;

            self.io_read_block(lba)?;

            let avail = self.block_size - block_off;
            let want = core::cmp::min(avail, buf.len() - total);
            buf[total..total + want]
                .copy_from_slice(&self.scratch.as_mut_slice()[block_off..block_off + want]);

            total += want;
            self.pos = self.pos.wrapping_add(want as u64);
        }

        Ok(total)
    }
}

impl Write for BlockDeviceIo {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        if buf.is_empty() {
            return Ok(0);
        }
        if !self.dev.supports_write() {
            return Err(block::Error::NotSupported);
        }

        let mut total = 0usize;
        while total < buf.len() {
            let lba = self.pos / (self.block_size as u64);
            let block_off = (self.pos % (self.block_size as u64)) as usize;
            let avail = self.block_size - block_off;
            let want = core::cmp::min(avail, buf.len() - total);

            if lba >= self.blocks {
                return Err(block::Error::OutOfBounds);
            }

            if want != self.block_size {
                self.io_read_block(lba)?;
            }

            self.scratch.as_mut_slice()[block_off..block_off + want]
                .copy_from_slice(&buf[total..total + want]);
            let abs_lba = self.base_lba.saturating_add(lba);
            self.dev.write_blocks(abs_lba, self.scratch.as_mut_slice())?;
            self.cached_lba = lba;

            total += want;
            self.pos = self.pos.wrapping_add(want as u64);
        }

        Ok(total)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.dev.flush()
    }
}

impl Seek for BlockDeviceIo {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64, Self::Error> {
        let cap_bytes = (self.blocks as u128) * (self.block_size as u128);
        let cap_i64 = core::cmp::min(cap_bytes, i64::MAX as u128) as i64;

        let next = match pos {
            SeekFrom::Start(off) => {
                if (off as u128) > cap_bytes {
                    return Err(block::Error::OutOfBounds);
                }
                off as i64
            }
            SeekFrom::End(delta) => cap_i64.checked_add(delta).ok_or(block::Error::OutOfBounds)?,
            SeekFrom::Current(delta) => (self.pos as i64)
                .checked_add(delta)
                .ok_or(block::Error::OutOfBounds)?,
        };

        if next < 0 {
            return Err(block::Error::OutOfBounds);
        }
        self.pos = next as u64;
        Ok(self.pos)
    }
}

fn pick_usbms_device() -> Option<block::DeviceHandle> {
    for h in block::device_handles().into_iter() {
        let info = h.info();
        if info.label.as_deref() == Some("usbms") {
            return Some(h);
        }
    }
    None
}

fn run_fatfs_demo_on_device(handle: block::DeviceHandle) {
    let info = handle.info();
    crate::log!(
        "files: using device id={} blocks={} block_size={}\n",
        info.id.raw(),
        info.block_count,
        info.block_size
    );
    
    // Prove the block path is functional before we do anything destructive.
    // A successful read of LBA0 (even if it returns zeros) demonstrates end-to-end BOT READ(10).
    let align = info.dma_alignment.max(1) as usize;
    if let Some(mut probe) = AlignedBuf::new(info.block_size as usize, align) {
        match handle.read_blocks(0, probe.as_mut_slice()) {
            Ok(()) => {
                let b = probe.as_mut_slice();
                crate::log!(
                    "files: read lba0 ok bytes[0..16]=[{:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X}]\n",
                    b[0],
                    b[1],
                    b[2],
                    b[3],
                    b[4],
                    b[5],
                    b[6],
                    b[7],
                    b[8],
                    b[9],
                    b[10],
                    b[11],
                    b[12],
                    b[13],
                    b[14],
                    b[15]
                );
            }
            Err(e) => {
                crate::log!("files: read lba0 FAILED err={:?}\n", e);
            }
        }
    } else {
        crate::log!("files: read lba0 SKIPPED (no aligned DMA buffer)\n");
    }

    let io = match BlockDeviceIo::new(handle) {
        Ok(io) => io,
        Err(e) => {
            crate::log!("files: failed to init device io: {:?}\n", e);
            return;
        }
    };

    // Prefer non-destructive behavior: attempt to mount first.
    crate::log!("files: mount begin\n");
    let fs = match FileSystem::new(io, FsOptions::new()) {
        Ok(fs) => fs,
        Err(e) => {
            crate::log!("files: mount failed ({:?}); formatting usbms (destructive)\n", e);

            let mut io = match BlockDeviceIo::new(handle) {
                Ok(io) => io,
                Err(e) => {
                    crate::log!("files: failed to init device io for format: {:?}\n", e);
                    return;
                }
            };

            if let Err(e) = format_volume(&mut io, FormatVolumeOptions::new()) {
                crate::log!("files: format failed ({:?})\n", e);
                return;
            }

            match FileSystem::new(io, FsOptions::new()) {
                Ok(fs) => fs,
                Err(e) => {
                    crate::log!("files: mount after format failed ({:?})\n", e);
                    return;
                }
            }
        }
    };

    crate::log!("files: mount ok\n");

    // Keep this 8.3-safe (<=8.3) to avoid short-name alias confusion.
    const DEMO_FILE_NAME: &str = "trueos.txt";
    crate::log!("files: demo file name={}\n", DEMO_FILE_NAME);

    {
        let root = fs.root_dir();

        crate::log!("files: dir list begin\n");
        let mut listed = 0u32;
        for entry in root.iter() {
            match entry {
                Ok(entry) => {
                    crate::log!("files: dir entry: {}\n", entry.file_name());
                    listed = listed.wrapping_add(1);
                    if listed >= 16 {
                        crate::log!("files: dir list truncated\n");
                        break;
                    }
                }
                Err(e) => {
                    crate::log!("files: dir list entry failed ({:?})\n", e);
                    break;
                }
            }
        }
        crate::log!("files: dir list end (count={})\n", listed);

        fn log_read<T: Read>(label: &str, file: &mut T)
        where
            T::Error: core::fmt::Debug,
        {
            let mut buf = [0u8; 256];
            match file.read(&mut buf) {
                Ok(n) => {
                    let shown = core::cmp::min(n, 128);
                    crate::log!("files: {} read ok n={} shown={} [", label, n, shown);
                    for i in 0..shown {
                        crate::log!("{:02X}", buf[i]);
                        if i + 1 != shown {
                            crate::log!(" ");
                        }
                    }
                    if n > shown {
                        crate::log!(" ...");
                    }
                    crate::log!("]\n");
                }
                Err(e) => crate::log!("files: {} read failed ({:?})\n", label, e),
            }
        }

        crate::log!("files: open existing begin\n");
        match root.open_file(DEMO_FILE_NAME) {
            Ok(mut file) => {
                crate::log!("files: open existing ok\n");
                log_read("existing", &mut file);
            }
            Err(fatfs::Error::NotFound) => {
                crate::log!("files: existing file missing; create begin\n");
                match root.create_file(DEMO_FILE_NAME) {
                    Ok(mut file) => {
                        crate::log!("files: create ok; write_all begin\n");
                        if let Err(e) = file.write_all("TRUEOS§".as_bytes()) {
                            crate::log!("files: write_all failed ({:?})\n", e);
                        }
                        let _ = file.flush();

                        let _ = file.seek(SeekFrom::Start(0));
                        log_read("after_create", &mut file);
                    }
                    Err(e) => crate::log!("files: create_file failed ({:?})\n", e),
                }
            }
            Err(e) => crate::log!("files: open existing failed ({:?})\n", e),
        };
    }
}

async fn build_fatfs_tree_for_device_async(tree: &mut FileTree, parent: trueos_math::NodeId, handle: block::DeviceHandle) {
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

    let io = match BlockDeviceIo::new(handle) {
        Ok(io) => io,
        Err(e) => {
            crate::log!("files: device io init failed: {:?}\n", e);
            return;
        }
    };

    let fs = match FileSystem::new(io, FsOptions::new()) {
        Ok(fs) => fs,
        Err(e) => {
            crate::log!("files: mount failed ({:?})\n", e);
            return;
        }
    };

    // Add a per-device filesystem root node.
    let fs_root = match tree.add_child(
        dev_id,
        FileTreeEntry {
            kind: FileTreeKind::Root,
            name: String::from("/"),
        },
    ) {
        Some(id) => id,
        None => {
            crate::log!("files: tree full; cannot add root\n");
            let _ = fs.unmount();
            return;
        }
    };

    // Iterative walk to avoid deep recursion on kernel stacks.
    let mut stack: Vec<(String, usize, trueos_math::NodeId)> = Vec::new();
    stack.push((String::new(), 0, fs_root));

    // Yield periodically so we don't starve other cooperative tasks.
    const YIELD_DIRS_EVERY: u32 = 8;
    let mut dirs_processed: u32 = 0;

    while let Some((dir_path, depth, parent_id)) = stack.pop() {
        if tree.len() + 1 >= tree.capacity() {
            crate::log!("files: tree full; truncating walk\n");
            break;
        }

        let mut dir = fs.root_dir();
        let mut opened_ok = true;
        if !dir_path.is_empty() {
            for seg in dir_path.split('/').filter(|s| !s.is_empty()) {
                match dir.open_dir(seg) {
                    Ok(next) => dir = next,
                    Err(e) => {
                        crate::log!(
                            "files: open_dir failed path='{}' seg='{}' err={:?}\n",
                            dir_path,
                            seg,
                            e
                        );
                        opened_ok = false;
                        break;
                    }
                }
            }
        }

        if !opened_ok {
            continue;
        }

        for entry in dir.iter() {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    crate::log!("files: iter entry failed ({:?})\n", e);
                    break;
                }
            };

            let name = entry.file_name();
            if name.is_empty() || name == "." || name == ".." {
                continue;
            }

            let is_dir = entry.is_dir();
            let kind = if is_dir {
                FileTreeKind::Dir
            } else {
                FileTreeKind::File
            };

            let child_id = match tree.add_child(
                parent_id,
                FileTreeEntry {
                    kind,
                    name: name.clone(),
                },
            ) {
                Some(id) => id,
                None => {
                    crate::log!("files: tree full; truncating walk\n");
                    break;
                }
            };

            if is_dir {
                let full_path = if dir_path.is_empty() {
                    name
                } else {
                    alloc::format!("{}/{}", dir_path, name)
                };
                stack.push((full_path, depth.saturating_add(1), child_id));
            }
        }

        dirs_processed = dirs_processed.saturating_add(1);
        if (dirs_processed % YIELD_DIRS_EVERY) == 0 {
            Timer::after(EmbassyDuration::from_millis(1)).await;
        }
    }

    let _ = fs.unmount();
}

async fn scan_all_devices_and_build_tree(tree: &mut FileTree, root: trueos_math::NodeId) {
    let devices = block::device_handles();
    if devices.is_empty() {
        crate::log!("files: no block devices found\n");
        return;
    }

    crate::log!("files: scanning {} device(s)\n", devices.len());
    for dev in devices.into_iter() {
        build_fatfs_tree_for_device_async(tree, root, dev).await;
        Timer::after(EmbassyDuration::from_millis(1)).await;
    }
}

#[embassy_executor::task]
pub async fn fatfs_usb_demo_task() {
    // Dedicated on-demand service. Heavy scanning is performed only when requested.
    crate::log!("files: service online (type 'files' in shell)\n");

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
