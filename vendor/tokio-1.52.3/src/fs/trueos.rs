use crate::io::ReadBuf;
use crate::loom::sync as trueos_sync;
use crate::runtime::prelude::*;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Copy, Debug, Default)]
pub struct FileType {
    is_file: bool,
    is_dir: bool,
    is_symlink: bool,
}

impl FileType {
    pub fn is_dir(&self) -> bool {
        self.is_dir
    }

    pub fn is_file(&self) -> bool {
        self.is_file
    }

    pub fn is_symlink(&self) -> bool {
        self.is_symlink
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Permissions {
    readonly: bool,
}

impl Permissions {
    pub fn readonly(&self) -> bool {
        self.readonly
    }

    pub fn set_readonly(&mut self, readonly: bool) {
        self.readonly = readonly;
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Metadata {
    len: u64,
    file_type: FileType,
    permissions: Permissions,
}

impl Metadata {
    fn file(len: u64) -> Self {
        Self {
            len,
            file_type: FileType {
                is_file: true,
                is_dir: false,
                is_symlink: false,
            },
            permissions: Permissions::default(),
        }
    }

    fn dir() -> Self {
        Self {
            len: 0,
            file_type: FileType {
                is_file: false,
                is_dir: true,
                is_symlink: false,
            },
            permissions: Permissions::default(),
        }
    }

    pub fn file_type(&self) -> FileType {
        self.file_type
    }

    pub fn is_dir(&self) -> bool {
        self.file_type.is_dir()
    }

    pub fn is_file(&self) -> bool {
        self.file_type.is_file()
    }

    pub fn is_symlink(&self) -> bool {
        self.file_type.is_symlink()
    }

    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn permissions(&self) -> Permissions {
        self.permissions
    }
}

#[derive(Debug)]
pub(crate) struct TrueosReadDir;

#[derive(Debug)]
pub(crate) struct TrueosDirEntry;

impl Iterator for TrueosReadDir {
    type Item = io::Result<TrueosDirEntry>;

    fn next(&mut self) -> Option<Self::Item> {
        None
    }
}

impl TrueosDirEntry {
    pub(crate) fn path(&self) -> PathBuf {
        PathBuf::new()
    }

    pub(crate) fn file_name(&self) -> crate::ffi::OsString {
        String::new()
    }

    pub(crate) fn metadata(&self) -> io::Result<Metadata> {
        unsupported("read_dir metadata")
    }

    pub(crate) fn file_type(&self) -> io::Result<FileType> {
        unsupported("read_dir file_type")
    }
}

fn unsupported<T>(_op: &'static str) -> io::Result<T> {
    Err(io::Error::new(
        io::ErrorKind::Other,
        "TRUEOS fs operation is not exposed through CABI yet",
    ))
}

struct Mutex<T>(trueos_sync::Mutex<T>);

impl<T> Mutex<T> {
    fn new(value: T) -> Self {
        Self(trueos_sync::Mutex::new(value))
    }

    fn lock(&self) -> io::Result<trueos_sync::MutexGuard<'_, T>> {
        Ok(self.0.lock())
    }
}

unsafe extern "C" {
    fn trueos_cabi_fs_read_file(
        path_ptr: *const u8,
        path_len: usize,
        out_ptr: *mut u8,
        out_cap: usize,
    ) -> isize;
    fn trueos_cabi_fs_write_begin(
        path_ptr: *const u8,
        path_len: usize,
        total_len: u64,
        out_handle: *mut u32,
    ) -> i32;
    fn trueos_cabi_fs_write_chunk(handle: u32, data_ptr: *const u8, data_len: usize) -> i32;
    fn trueos_cabi_fs_write_finish(handle: u32) -> i32;
    fn trueos_cabi_fs_write_abort(handle: u32) -> i32;
    fn trueos_cabi_fs_create_dir_all(path_ptr: *const u8, path_len: usize) -> i32;
    fn trueos_cabi_fs_exists(path_ptr: *const u8, path_len: usize) -> i32;
    fn trueos_cabi_fs_remove(path_ptr: *const u8, path_len: usize) -> i32;
    fn trueos_cabi_fs_stat(
        path_ptr: *const u8,
        path_len: usize,
        out_kind: *mut u32,
        out_len: *mut u64,
    ) -> i32;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TrueosPathAnchor {
    Current,
    Root,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TrueosPath {
    anchor: TrueosPathAnchor,
    components: Vec<String>,
}

impl TrueosPath {
    fn parse(path: &Path, allow_empty: bool) -> io::Result<Self> {
        let mut anchor = TrueosPathAnchor::Current;
        let mut components = Vec::new();

        for component in path.components() {
            match component {
                Component::CurDir => {}
                Component::RootDir => anchor = TrueosPathAnchor::Root,
                Component::Normal(part) => {
                    if part.as_bytes().contains(&0) {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "TRUEOS fs paths must not contain NUL",
                        ));
                    }
                    components.push(String::from(part));
                }
                Component::ParentDir => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "TRUEOS fs path escapes the blueprint app root",
                    ));
                }
                Component::Prefix(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "TRUEOS fs paths do not support platform prefixes",
                    ));
                }
            }
        }

        if components.is_empty() && !allow_empty {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "TRUEOS fs path must name a file",
            ));
        }

        Ok(Self { anchor, components })
    }

    fn is_root(&self) -> bool {
        self.anchor == TrueosPathAnchor::Root && self.components.is_empty()
    }

    fn to_cabi_string(&self) -> String {
        let mut out = String::new();
        for component in &self.components {
            if !out.is_empty() {
                out.push('/');
            }
            out.push_str(component);
        }
        out
    }

    fn to_display_path(&self) -> PathBuf {
        let mut out = PathBuf::from("/");
        for component in &self.components {
            out.push(component);
        }
        out
    }
}

fn cabi_path_string(path: &Path, allow_empty: bool) -> io::Result<String> {
    Ok(TrueosPath::parse(path, allow_empty)?.to_cabi_string())
}

fn fs_status_to_io(rc: i32, _op: &'static str) -> io::Error {
    let kind = match rc {
        -4 | -6 => io::ErrorKind::InvalidInput,
        -5 => io::ErrorKind::NotFound,
        -8 => io::ErrorKind::NotFound,
        -9 => io::ErrorKind::AlreadyExists,
        -14 => io::ErrorKind::TimedOut,
        _ => io::ErrorKind::Other,
    };
    io::Error::new(kind, "TRUEOS fs CABI operation failed")
}

fn read_sync(path: &Path) -> io::Result<Vec<u8>> {
    let path = cabi_path_string(path, false)?;
    let path = path.as_bytes();
    let len =
        unsafe { trueos_cabi_fs_read_file(path.as_ptr(), path.len(), core::ptr::null_mut(), 0) };
    if len < 0 {
        return Err(fs_status_to_io(len as i32, "read.len"));
    }

    let mut bytes = vec![0u8; len as usize];
    let got = unsafe {
        trueos_cabi_fs_read_file(path.as_ptr(), path.len(), bytes.as_mut_ptr(), bytes.len())
    };
    if got < 0 {
        return Err(fs_status_to_io(got as i32, "read"));
    }

    bytes.truncate(got as usize);
    Ok(bytes)
}

fn write_sync(path: &Path, contents: &[u8]) -> io::Result<()> {
    let path = cabi_path_string(path, false)?;
    let path = path.as_bytes();
    let mut handle = 0u32;
    let rc = unsafe {
        trueos_cabi_fs_write_begin(path.as_ptr(), path.len(), contents.len() as u64, &mut handle)
    };
    if rc != 0 {
        return Err(fs_status_to_io(rc, "write_begin"));
    }

    let rc = unsafe { trueos_cabi_fs_write_chunk(handle, contents.as_ptr(), contents.len()) };
    if rc != 0 {
        let _ = unsafe { trueos_cabi_fs_write_abort(handle) };
        return Err(fs_status_to_io(rc, "write_chunk"));
    }

    let rc = unsafe { trueos_cabi_fs_write_finish(handle) };
    if rc != 0 {
        let _ = unsafe { trueos_cabi_fs_write_abort(handle) };
        return Err(fs_status_to_io(rc, "write_finish"));
    }

    Ok(())
}

fn exists_sync(path: &Path) -> io::Result<bool> {
    let path = cabi_path_string(path, true)?;
    let path = path.as_bytes();
    let rc = unsafe { trueos_cabi_fs_exists(path.as_ptr(), path.len()) };
    if rc < 0 {
        return Err(fs_status_to_io(rc, "exists"));
    }
    Ok(rc != 0)
}

fn stat_sync(path: &Path) -> io::Result<Metadata> {
    let path = cabi_path_string(path, true)?;
    let mut kind = 0u32;
    let mut len = 0u64;
    let rc = unsafe {
        trueos_cabi_fs_stat(
            path.as_ptr(),
            path.len(),
            &mut kind as *mut u32,
            &mut len as *mut u64,
        )
    };
    if rc != 0 {
        return Err(fs_status_to_io(rc, "stat"));
    }

    match kind {
        1 => Ok(Metadata::file(len)),
        2 => Ok(Metadata::dir()),
        _ => Err(io::Error::new(
            io::ErrorKind::Other,
            "TRUEOS fs CABI returned an unknown node kind",
        )),
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TrueosOpenOptions {
    read: bool,
    write: bool,
    append: bool,
    truncate: bool,
    create: bool,
    create_new: bool,
}

impl TrueosOpenOptions {
    pub(crate) fn new() -> Self {
        Self {
            read: false,
            write: false,
            append: false,
            truncate: false,
            create: false,
            create_new: false,
        }
    }

    pub(crate) fn read(&mut self, read: bool) {
        self.read = read;
    }

    pub(crate) fn write(&mut self, write: bool) {
        self.write = write;
    }

    pub(crate) fn append(&mut self, append: bool) {
        self.append = append;
    }

    pub(crate) fn truncate(&mut self, truncate: bool) {
        self.truncate = truncate;
    }

    pub(crate) fn create(&mut self, create: bool) {
        self.create = create;
    }

    pub(crate) fn create_new(&mut self, create_new: bool) {
        self.create_new = create_new;
    }

    pub(crate) fn open(&self, path: PathBuf) -> io::Result<TrueosFile> {
        if !self.read && !self.write && !self.append {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "TRUEOS fs open requires read, write, or append access",
            ));
        }
        if self.truncate && !self.write && !self.append {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "TRUEOS fs truncate requires write or append access",
            ));
        }
        if self.create_new && !self.write && !self.append {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "TRUEOS fs create_new requires write or append access",
            ));
        }
        if self.create_new && exists_sync(&path)? {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "TRUEOS fs create_new target already exists",
            ));
        }

        let mut data = match read_sync(&path) {
            Ok(bytes) => bytes,
            Err(err)
                if err.kind() == io::ErrorKind::NotFound && (self.create || self.create_new) =>
            {
                Vec::new()
            }
            Err(err) => return Err(err),
        };

        if self.truncate || self.create_new {
            data.clear();
        }

        let pos = if self.append { data.len() as u64 } else { 0 };
        Ok(TrueosFile {
            inner: Mutex::new(TrueosFileInner {
                path,
                data,
                pos,
                read: self.read,
                write: self.write || self.append,
                append: self.append,
                dirty: self.truncate || self.create_new,
            }),
        })
    }
}

pub struct TrueosFile {
    inner: Mutex<TrueosFileInner>,
}

struct TrueosFileInner {
    path: PathBuf,
    data: Vec<u8>,
    pos: u64,
    read: bool,
    write: bool,
    append: bool,
    dirty: bool,
}

impl TrueosFile {
    pub(crate) fn poll_read_into(&self, dst: &mut ReadBuf<'_>) -> io::Result<()> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "TRUEOS fs file mutex poisoned"))?;
        if !inner.read {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "TRUEOS fs file was not opened for reading",
            ));
        }
        let pos = inner.pos as usize;
        let n = core::cmp::min(dst.remaining(), inner.data.len().saturating_sub(pos));
        dst.put_slice(&inner.data[pos..pos + n]);
        inner.pos = inner.pos.saturating_add(n as u64);
        Ok(())
    }

    pub(crate) fn poll_write_from(&self, src: &[u8]) -> io::Result<usize> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "TRUEOS fs file mutex poisoned"))?;
        if !inner.write {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "TRUEOS fs file was not opened for writing",
            ));
        }
        if inner.append {
            inner.pos = inner.data.len() as u64;
        }
        let pos = usize::try_from(inner.pos).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "TRUEOS fs cursor is too large")
        })?;
        if pos > inner.data.len() {
            inner.data.resize(pos, 0);
        }
        let end = pos.checked_add(src.len()).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "TRUEOS fs write is too large")
        })?;
        if end > inner.data.len() {
            inner.data.resize(end, 0);
        }
        inner.data[pos..end].copy_from_slice(src);
        inner.pos = end as u64;
        inner.dirty = true;
        Ok(src.len())
    }

    pub(crate) fn seek_inner(&self, pos: SeekFrom) -> io::Result<u64> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "TRUEOS fs file mutex poisoned"))?;
        let base = match pos {
            SeekFrom::Start(n) => {
                return {
                    inner.pos = n;
                    Ok(n)
                };
            }
            SeekFrom::End(n) => inner.data.len() as i128 + n as i128,
            SeekFrom::Current(n) => inner.pos as i128 + n as i128,
        };
        if base < 0 || base > u64::MAX as i128 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "TRUEOS fs seek target is out of range",
            ));
        }
        inner.pos = base as u64;
        Ok(inner.pos)
    }

    pub(crate) fn position(&self) -> io::Result<u64> {
        self.inner
            .lock()
            .map(|inner| inner.pos)
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "TRUEOS fs file mutex poisoned"))
    }

    pub(crate) fn sync_all(&self) -> io::Result<()> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "TRUEOS fs file mutex poisoned"))?;
        if inner.dirty {
            write_sync(&inner.path, &inner.data)?;
            inner.dirty = false;
        }
        Ok(())
    }

    pub(crate) fn sync_data(&self) -> io::Result<()> {
        self.sync_all()
    }

    pub(crate) fn set_len(&self, size: u64) -> io::Result<()> {
        let size = usize::try_from(size).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "TRUEOS fs length is too large")
        })?;
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "TRUEOS fs file mutex poisoned"))?;
        if !inner.write {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "TRUEOS fs file was not opened for writing",
            ));
        }
        inner.data.resize(size, 0);
        if inner.pos > size as u64 {
            inner.pos = size as u64;
        }
        inner.dirty = true;
        Ok(())
    }

    pub(crate) fn metadata(&self) -> io::Result<Metadata> {
        let inner = self
            .inner
            .lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "TRUEOS fs file mutex poisoned"))?;
        Ok(Metadata::file(inner.data.len() as u64))
    }

    pub(crate) fn try_clone(&self) -> io::Result<TrueosFile> {
        let inner = self
            .inner
            .lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "TRUEOS fs file mutex poisoned"))?;
        Ok(TrueosFile {
            inner: Mutex::new(TrueosFileInner {
                path: inner.path.clone(),
                data: inner.data.clone(),
                pos: inner.pos,
                read: inner.read,
                write: inner.write,
                append: inner.append,
                dirty: inner.dirty,
            }),
        })
    }

    pub(crate) fn set_permissions(&self, _perm: Permissions) -> io::Result<()> {
        unsupported("permissions")
    }
}

impl Drop for TrueosFile {
    fn drop(&mut self) {
        let _ = self.sync_all();
    }
}

impl fmt::Debug for TrueosFile {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.debug_struct("TrueosFile").finish_non_exhaustive()
    }
}

impl Read for &TrueosFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "TRUEOS fs file mutex poisoned"))?;
        if !inner.read {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "TRUEOS fs file was not opened for reading",
            ));
        }
        let pos = inner.pos as usize;
        let n = core::cmp::min(buf.len(), inner.data.len().saturating_sub(pos));
        buf[..n].copy_from_slice(&inner.data[pos..pos + n]);
        inner.pos = inner.pos.saturating_add(n as u64);
        Ok(n)
    }
}

impl Write for &TrueosFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.poll_write_from(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.sync_all()
    }
}

impl Seek for &TrueosFile {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.seek_inner(pos)
    }
}

pub(crate) async fn read(path: &Path) -> io::Result<Vec<u8>> {
    read_sync(path)
}

pub(crate) async fn read_to_string(path: &Path) -> io::Result<String> {
    let bytes = read(path).await?;
    String::from_utf8(bytes).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidData, "TRUEOS fs CABI file is not valid UTF-8")
    })
}

pub(crate) async fn write(path: &Path, contents: &[u8]) -> io::Result<()> {
    write_sync(path, contents)
}

pub(crate) async fn create_dir(path: &Path) -> io::Result<()> {
    create_dir_all(path).await
}

pub(crate) async fn create_dir_all(path: &Path) -> io::Result<()> {
    let path = cabi_path_string(path, true)?;
    let path = path.as_bytes();
    let rc = unsafe { trueos_cabi_fs_create_dir_all(path.as_ptr(), path.len()) };
    if rc != 0 {
        return Err(fs_status_to_io(rc, "create_dir_all"));
    }
    Ok(())
}

pub(crate) async fn try_exists(path: &Path) -> io::Result<bool> {
    exists_sync(path)
}

pub(crate) async fn canonicalize(path: &Path) -> io::Result<PathBuf> {
    let canonical = TrueosPath::parse(path, true)?;
    if !canonical.is_root() && !exists_sync(path)? {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "TRUEOS fs canonicalize target does not exist",
        ));
    }

    Ok(canonical.to_display_path())
}

pub(crate) async fn metadata(path: &Path) -> io::Result<Metadata> {
    stat_sync(path)
}

pub(crate) async fn read_dir(_path: &Path) -> io::Result<TrueosReadDir> {
    unsupported("read_dir")
}

pub(crate) async fn set_permissions(_path: &Path, _perm: Permissions) -> io::Result<()> {
    unsupported("permissions")
}

pub(crate) async fn remove_file(path: &Path) -> io::Result<()> {
    let path = cabi_path_string(path, false)?;
    let path = path.as_bytes();
    let rc = unsafe { trueos_cabi_fs_remove(path.as_ptr(), path.len()) };
    if rc != 0 {
        return Err(fs_status_to_io(rc, "remove_file"));
    }
    Ok(())
}
