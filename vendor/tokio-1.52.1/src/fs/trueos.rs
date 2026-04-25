use crate::io::ReadBuf;
use std::fmt;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

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
    fn trueos_cabi_fs_exists(path_ptr: *const u8, path_len: usize) -> i32;
    fn trueos_cabi_fs_remove(path_ptr: *const u8, path_len: usize) -> i32;
}

fn path_bytes(path: &Path) -> io::Result<&[u8]> {
    path.to_str().map(str::as_bytes).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "TRUEOS fs CABI paths must be valid UTF-8",
        )
    })
}

fn fs_status_to_io(rc: i32, op: &'static str) -> io::Error {
    let kind = match rc {
        -4 | -6 => io::ErrorKind::InvalidInput,
        -5 => io::ErrorKind::NotFound,
        -8 => io::ErrorKind::NotFound,
        -9 => io::ErrorKind::AlreadyExists,
        -14 => io::ErrorKind::TimedOut,
        _ => io::ErrorKind::Other,
    };
    io::Error::new(kind, format!("TRUEOS fs CABI {op} failed rc={rc}"))
}

fn read_sync(path: &Path) -> io::Result<Vec<u8>> {
    let path = path_bytes(path)?;
    let len = unsafe {
        trueos_cabi_fs_read_file(path.as_ptr(), path.len(), core::ptr::null_mut(), 0)
    };
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
    let path = path_bytes(path)?;
    let mut handle = 0u32;
    let rc = unsafe {
        trueos_cabi_fs_write_begin(
            path.as_ptr(),
            path.len(),
            contents.len() as u64,
            &mut handle,
        )
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
    let path = path_bytes(path)?;
    let rc = unsafe { trueos_cabi_fs_exists(path.as_ptr(), path.len()) };
    if rc < 0 {
        return Err(fs_status_to_io(rc, "exists"));
    }
    Ok(rc != 0)
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
            Err(err) if err.kind() == io::ErrorKind::NotFound && (self.create || self.create_new) => {
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
        let mut inner = self.inner.lock().map_err(|_| {
            io::Error::new(io::ErrorKind::Other, "TRUEOS fs file mutex poisoned")
        })?;
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
        let mut inner = self.inner.lock().map_err(|_| {
            io::Error::new(io::ErrorKind::Other, "TRUEOS fs file mutex poisoned")
        })?;
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
        let mut inner = self.inner.lock().map_err(|_| {
            io::Error::new(io::ErrorKind::Other, "TRUEOS fs file mutex poisoned")
        })?;
        let base = match pos {
            SeekFrom::Start(n) => return {
                inner.pos = n;
                Ok(n)
            },
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
        let mut inner = self.inner.lock().map_err(|_| {
            io::Error::new(io::ErrorKind::Other, "TRUEOS fs file mutex poisoned")
        })?;
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
        let mut inner = self.inner.lock().map_err(|_| {
            io::Error::new(io::ErrorKind::Other, "TRUEOS fs file mutex poisoned")
        })?;
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

    pub(crate) fn metadata(&self) -> io::Result<std::fs::Metadata> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "TRUEOS fs metadata is not exposed through CABI yet",
        ))
    }

    pub(crate) fn try_clone(&self) -> io::Result<TrueosFile> {
        let inner = self.inner.lock().map_err(|_| {
            io::Error::new(io::ErrorKind::Other, "TRUEOS fs file mutex poisoned")
        })?;
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

    pub(crate) fn set_permissions(&self, _perm: std::fs::Permissions) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "TRUEOS fs permissions are not exposed through CABI yet",
        ))
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
        let mut inner = self.inner.lock().map_err(|_| {
            io::Error::new(io::ErrorKind::Other, "TRUEOS fs file mutex poisoned")
        })?;
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
        io::Error::new(
            io::ErrorKind::InvalidData,
            "TRUEOS fs CABI file is not valid UTF-8",
        )
    })
}

pub(crate) async fn write(path: &Path, contents: &[u8]) -> io::Result<()> {
    write_sync(path, contents)
}

pub(crate) async fn remove_file(path: &Path) -> io::Result<()> {
    let path = path_bytes(path)?;
    let rc = unsafe { trueos_cabi_fs_remove(path.as_ptr(), path.len()) };
    if rc != 0 {
        return Err(fs_status_to_io(rc, "remove_file"));
    }
    Ok(())
}
