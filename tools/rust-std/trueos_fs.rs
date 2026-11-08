// Reference std sys::fs backend. Native filesystem objects are not descriptors.
use crate::ffi::{OsStr, OsString};
use crate::fmt;
use crate::fs::TryLockError;
use crate::io::{self, BorrowedCursor, IoSlice, IoSliceMut, SeekFrom};
use crate::path::{Component, Path, PathBuf};
pub use crate::sys::fs::common::Dir;
use crate::sys::time::SystemTime;

unsafe extern "C" {
    fn trueos_cabi_async_fs_object_start_v1(request: *const u8, len: usize) -> i32;
    fn trueos_cabi_async_fs_metadata_start_v2(path: *const u8, len: usize) -> i32;
    fn trueos_cabi_async_fs_list_dir_start(path: *const u8, len: usize) -> i32;
    fn trueos_cabi_async_fs_create_dir_start_v1(path: *const u8, len: usize) -> i32;
    fn trueos_cabi_async_fs_remove_leaf_start_v1(path: *const u8, len: usize, kind: u32) -> i32;
    fn trueos_cabi_async_fs_rename_start_v2(
        old: *const u8,
        old_len: usize,
        new: *const u8,
        new_len: usize,
        flags: u32,
    ) -> i32;
    fn trueos_cabi_async_fs_status(id: u32) -> i32;
    fn trueos_cabi_async_fs_result_len(id: u32) -> isize;
    fn trueos_cabi_async_fs_result_read(
        id: u32,
        offset: usize,
        output: *mut u8,
        capacity: usize,
    ) -> isize;
    fn trueos_cabi_async_fs_discard(id: u32) -> i32;
}
// Fits one native VMCALL payload including the 24-byte object request header.
const CHUNK: usize = 64 * 1024;
fn unsupported<T>() -> io::Result<T> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "operation has no native filesystem contract",
    ))
}
fn error(code: i32) -> io::Error {
    let kind = match code {
        -1 | -4 | -6 => io::ErrorKind::InvalidInput,
        -3 => io::ErrorKind::OutOfMemory,
        -7 => io::ErrorKind::FileTooLarge,
        -8 => io::ErrorKind::NotFound,
        -9 => io::ErrorKind::AlreadyExists,
        -14 => io::ErrorKind::TimedOut,
        -17 | -18 => io::ErrorKind::WouldBlock,
        -19 => io::ErrorKind::PermissionDenied,
        -20 => io::ErrorKind::Unsupported,
        _ => io::ErrorKind::Other,
    };
    io::Error::new(kind, "native filesystem operation failed")
}
fn path_bytes(path: &Path) -> io::Result<&[u8]> {
    let bytes = path.as_os_str().as_encoded_bytes();
    if bytes.is_empty()
        || bytes.contains(&0)
        || bytes.len() > 1024
        || crate::str::from_utf8(bytes).is_err()
    {
        return Err(error(-4));
    }
    Ok(bytes)
}
fn job(id: i32) -> io::Result<Vec<u8>> {
    if id <= 0 {
        return Err(error(id));
    }
    struct Operation(u32);
    impl Drop for Operation {
        fn drop(&mut self) {
            unsafe {
                trueos_cabi_async_fs_discard(self.0);
            }
        }
    }
    let operation = Operation(id as u32);
    loop {
        match unsafe { trueos_cabi_async_fs_status(operation.0) } {
            0 => crate::thread::yield_now(),
            1 => break,
            code => return Err(error(code)),
        }
    }
    let length = unsafe { trueos_cabi_async_fs_result_len(operation.0) };
    if length < 0 {
        return Err(error(length as i32));
    }
    if length > 16 * 1024 * 1024 {
        return Err(error(-7));
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length as usize)
        .map_err(|_| error(-3))?;
    bytes.resize(length as usize, 0);
    let mut offset = 0;
    while offset < bytes.len() {
        let capacity = CHUNK.min(bytes.len() - offset);
        let read = unsafe {
            trueos_cabi_async_fs_result_read(
                operation.0,
                offset,
                bytes[offset..].as_mut_ptr(),
                capacity,
            )
        };
        if read <= 0 || read as usize > capacity {
            return Err(error(if read < 0 { read as i32 } else { -2 }));
        }
        offset += read as usize;
    }
    Ok(bytes)
}
fn object(op: u32, flags: u32, handle: u64, arg: u64, payload: &[u8]) -> io::Result<Vec<u8>> {
    let mut request = Vec::with_capacity(24 + payload.len());
    request.extend_from_slice(&op.to_le_bytes());
    request.extend_from_slice(&flags.to_le_bytes());
    request.extend_from_slice(&handle.to_le_bytes());
    request.extend_from_slice(&arg.to_le_bytes());
    request.extend_from_slice(payload);
    job(unsafe { trueos_cabi_async_fs_object_start_v1(request.as_ptr(), request.len()) })
}
fn number(bytes: Vec<u8>) -> io::Result<u64> {
    Ok(u64::from_le_bytes(bytes.try_into().map_err(|_| error(-2))?))
}

pub struct File {
    object: u64,
}
#[derive(Clone, Debug)]
pub struct FileAttr {
    kind: u32,
    len: u64,
    file_id: u64,
    root_id: u32,
    revision: u64,
}
impl FileAttr {
    fn decode(bytes: Vec<u8>) -> io::Result<Self> {
        if bytes.len() != 32 {
            return Err(error(-2));
        }
        Ok(Self {
            kind: u32::from_le_bytes(bytes[0..4].try_into().unwrap()),
            len: u64::from_le_bytes(bytes[4..12].try_into().unwrap()),
            file_id: u64::from_le_bytes(bytes[12..20].try_into().unwrap()),
            root_id: u32::from_le_bytes(bytes[20..24].try_into().unwrap()),
            revision: u64::from_le_bytes(bytes[24..32].try_into().unwrap()),
        })
    }
    pub fn size(&self) -> u64 {
        self.len
    }
    pub fn file_type(&self) -> FileType {
        FileType(self.kind)
    }
    pub fn perm(&self) -> FilePermissions {
        FilePermissions { mode: None }
    }
    pub fn modified(&self) -> io::Result<SystemTime> {
        unsupported()
    }
    pub fn accessed(&self) -> io::Result<SystemTime> {
        unsupported()
    }
    pub fn created(&self) -> io::Result<SystemTime> {
        unsupported()
    }
    pub fn root_id(&self) -> u32 {
        self.root_id
    }
    pub fn file_id(&self) -> Option<u64> {
        Some(self.file_id)
    }
    pub fn revision(&self) -> Option<u64> {
        Some(self.revision)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilePermissions {
    mode: Option<u32>,
}
impl FilePermissions {
    pub fn readonly(&self) -> bool {
        self.mode.is_some_and(|m| m & 0o222 == 0)
    }
    pub fn set_readonly(&mut self, read_only: bool) {
        self.mode = Some(if read_only { 0o444 } else { 0o666 });
    }
    pub fn mode(&self) -> u32 {
        self.mode.unwrap_or(0)
    }
    pub fn from_mode(mode: u32) -> Self {
        Self { mode: Some(mode) }
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct FileTimes;
impl FileTimes {
    pub fn set_accessed(&mut self, _: SystemTime) {}
    pub fn set_modified(&mut self, _: SystemTime) {}
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct FileType(u32);
impl FileType {
    pub fn is_dir(&self) -> bool {
        self.0 == 2
    }
    pub fn is_file(&self) -> bool {
        self.0 == 1
    }
    pub fn is_symlink(&self) -> bool {
        false
    }
    pub fn is(&self, _: u32) -> bool {
        false
    }
}
#[derive(Clone, Debug, Default)]
pub struct OpenOptions {
    flags: u32,
    mode: Option<u32>,
    custom_flags: i32,
}
impl OpenOptions {
    pub fn new() -> Self {
        Self::default()
    }
    fn flag(&mut self, bit: u32, enabled: bool) {
        if enabled {
            self.flags |= bit;
        } else {
            self.flags &= !bit;
        }
    }
    pub fn read(&mut self, v: bool) {
        self.flag(1, v);
    }
    pub fn write(&mut self, v: bool) {
        self.flag(2, v);
    }
    pub fn append(&mut self, v: bool) {
        self.flag(4, v);
    }
    pub fn truncate(&mut self, v: bool) {
        self.flag(8, v);
    }
    pub fn create(&mut self, v: bool) {
        self.flag(16, v);
    }
    pub fn create_new(&mut self, v: bool) {
        self.flag(32, v);
    }
    pub fn mode(&mut self, mode: u32) {
        self.mode = Some(mode);
    }
    pub fn custom_flags(&mut self, flags: i32) {
        self.custom_flags = flags;
    }
}
impl File {
    pub fn open(path: &Path, options: &OpenOptions) -> io::Result<Self> {
        // Native record access keys currently have no chmod/mode equivalent.
        // Never silently promise a Unix protection mode for credential files.
        if options.mode.is_some() || options.custom_flags != 0 {
            return unsupported();
        }
        let object = number(object(1, options.flags, 0, 0, path_bytes(path)?)?)?;
        if object == 0 {
            return Err(error(-2));
        }
        Ok(Self { object })
    }
    pub fn file_attr(&self) -> io::Result<FileAttr> {
        FileAttr::decode(object(5, 0, self.object, 0, &[])?)
    }
    pub fn has_same_access_key(&self, other: &Self) -> io::Result<bool> {
        match number(object(15, 0, self.object, other.object, &[])?)? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(error(-2)),
        }
    }
    pub fn fsync(&self) -> io::Result<()> {
        object(11, 0, self.object, 0, &[]).map(|_| ())
    }
    pub fn datasync(&self) -> io::Result<()> {
        self.fsync()
    }
    fn try_lock_kind(&self, shared: bool) -> Result<(), TryLockError> {
        object(9, if shared { 1 } else { 2 }, self.object, 0, &[])
            .map(|_| ())
            .map_err(|e| {
                if e.kind() == io::ErrorKind::WouldBlock {
                    TryLockError::WouldBlock
                } else {
                    TryLockError::Error(e)
                }
            })
    }
    pub fn try_lock(&self) -> Result<(), TryLockError> {
        self.try_lock_kind(false)
    }
    pub fn try_lock_shared(&self) -> Result<(), TryLockError> {
        self.try_lock_kind(true)
    }
    fn lock_kind(&self, shared: bool) -> io::Result<()> {
        loop {
            match self.try_lock_kind(shared) {
                Ok(()) => return Ok(()),
                Err(TryLockError::WouldBlock) => {
                    crate::thread::sleep(crate::time::Duration::from_millis(1))
                }
                Err(TryLockError::Error(e)) => return Err(e),
            }
        }
    }
    pub fn lock(&self) -> io::Result<()> {
        self.lock_kind(false)
    }
    pub fn lock_shared(&self) -> io::Result<()> {
        self.lock_kind(true)
    }
    pub fn unlock(&self) -> io::Result<()> {
        object(10, 0, self.object, 0, &[]).map(|_| ())
    }
    pub fn truncate(&self, size: u64) -> io::Result<()> {
        object(6, 0, self.object, size, &[]).map(|_| ())
    }
    pub fn read(&self, bytes: &mut [u8]) -> io::Result<usize> {
        let data = object(2, 0, self.object, bytes.len().min(CHUNK) as u64, &[])?;
        if data.len() > bytes.len() {
            return Err(error(-2));
        }
        bytes[..data.len()].copy_from_slice(&data);
        Ok(data.len())
    }
    pub fn read_vectored(&self, buffers: &mut [IoSliceMut<'_>]) -> io::Result<usize> {
        io::default_read_vectored(|b| self.read(b), buffers)
    }
    pub fn is_read_vectored(&self) -> bool {
        false
    }
    pub fn read_buf(&self, mut cursor: BorrowedCursor<'_, u8>) -> io::Result<()> {
        let data = object(2, 0, self.object, cursor.capacity().min(CHUNK) as u64, &[])?;
        if data.len() > cursor.capacity() {
            return Err(error(-2));
        }
        cursor.append(&data);
        Ok(())
    }
    pub fn write(&self, bytes: &[u8]) -> io::Result<usize> {
        let count = number(object(
            3,
            0,
            self.object,
            0,
            &bytes[..bytes.len().min(CHUNK)],
        )?)?;
        if count > bytes.len() as u64 {
            return Err(error(-2));
        }
        Ok(count as usize)
    }
    pub fn write_vectored(&self, buffers: &[IoSlice<'_>]) -> io::Result<usize> {
        io::default_write_vectored(|b| self.write(b), buffers)
    }
    pub fn is_write_vectored(&self) -> bool {
        false
    }
    pub fn flush(&self) -> io::Result<()> {
        Ok(())
    }
    pub fn seek(&self, position: SeekFrom) -> io::Result<u64> {
        let (flags, arg) = match position {
            SeekFrom::Start(v) => (0, v),
            SeekFrom::Current(v) => (1, v as u64),
            SeekFrom::End(v) => (2, v as u64),
        };
        number(object(4, flags, self.object, arg, &[])?)
    }
    pub fn tell(&self) -> io::Result<u64> {
        self.seek(SeekFrom::Current(0))
    }
    pub fn size(&self) -> Option<io::Result<u64>> {
        Some(self.file_attr().map(|m| m.len))
    }
    pub fn duplicate(&self) -> io::Result<Self> {
        Ok(Self {
            object: number(object(7, 0, self.object, 0, &[])?)?,
        })
    }
    pub fn set_permissions(&self, _: FilePermissions) -> io::Result<()> {
        unsupported()
    }
    pub fn set_times(&self, _: FileTimes) -> io::Result<()> {
        unsupported()
    }
    pub fn read_at(&self, bytes: &mut [u8], offset: u64) -> io::Result<usize> {
        let data = object(12, bytes.len().min(CHUNK) as u32, self.object, offset, &[])?;
        if data.len() > bytes.len() {
            return Err(error(-2));
        }
        bytes[..data.len()].copy_from_slice(&data);
        Ok(data.len())
    }
    pub fn read_buf_at(&self, mut cursor: BorrowedCursor<'_, u8>, offset: u64) -> io::Result<()> {
        let data = object(
            12,
            cursor.capacity().min(CHUNK) as u32,
            self.object,
            offset,
            &[],
        )?;
        if data.len() > cursor.capacity() {
            return Err(error(-2));
        }
        cursor.append(&data);
        Ok(())
    }
    pub fn read_vectored_at(
        &self,
        buffers: &mut [IoSliceMut<'_>],
        offset: u64,
    ) -> io::Result<usize> {
        io::default_read_vectored(|b| self.read_at(b, offset), buffers)
    }
    pub fn write_at(&self, bytes: &[u8], offset: u64) -> io::Result<usize> {
        Ok(number(object(
            13,
            0,
            self.object,
            offset,
            &bytes[..bytes.len().min(CHUNK)],
        )?)? as usize)
    }
    pub fn write_vectored_at(&self, buffers: &[IoSlice<'_>], offset: u64) -> io::Result<usize> {
        io::default_write_vectored(|b| self.write_at(b, offset), buffers)
    }
}
impl Drop for File {
    fn drop(&mut self) {
        let _ = object(8, 0, self.object, 0, &[]);
    }
}
impl fmt::Debug for File {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("File")
            .field("native_object", &self.object)
            .finish()
    }
}
#[derive(Debug)]
pub struct ReadDir {
    entries: crate::vec::IntoIter<io::Result<DirEntry>>,
}
impl Iterator for ReadDir {
    type Item = io::Result<DirEntry>;
    fn next(&mut self) -> Option<Self::Item> {
        self.entries.next()
    }
}
#[derive(Debug)]
pub struct DirEntry {
    path: PathBuf,
    name: OsString,
    metadata: FileAttr,
}
impl DirEntry {
    pub fn path(&self) -> PathBuf {
        self.path.clone()
    }
    pub fn file_name(&self) -> OsString {
        self.name.clone()
    }
    pub fn file_name_os_str(&self) -> &OsStr {
        &self.name
    }
    pub fn metadata(&self) -> io::Result<FileAttr> {
        stat(&self.path)
    }
    pub fn file_type(&self) -> io::Result<FileType> {
        Ok(self.metadata.file_type())
    }
    pub fn ino(&self) -> u64 {
        self.metadata.file_id
    }
}
#[derive(Debug)]
pub struct DirBuilder {
    mode: Option<u32>,
}
impl DirBuilder {
    pub fn new() -> Self {
        Self { mode: None }
    }
    pub fn set_mode(&mut self, mode: u32) {
        self.mode = Some(mode)
    }
    pub fn mkdir(&self, path: &Path) -> io::Result<()> {
        if self.mode.is_some() {
            return unsupported();
        }
        let path = path_bytes(path)?;
        job(unsafe { trueos_cabi_async_fs_create_dir_start_v1(path.as_ptr(), path.len()) })
            .map(|_| ())
    }
}
pub fn stat(path: &Path) -> io::Result<FileAttr> {
    let path = path_bytes(path)?;
    FileAttr::decode(job(unsafe {
        trueos_cabi_async_fs_metadata_start_v2(path.as_ptr(), path.len())
    })?)
}
pub fn lstat(path: &Path) -> io::Result<FileAttr> {
    stat(path)
}
pub fn unlink(path: &Path) -> io::Result<()> {
    remove(path, 1)
}
pub fn rmdir(path: &Path) -> io::Result<()> {
    remove(path, 2)
}
fn remove(path: &Path, kind: u32) -> io::Result<()> {
    let path = path_bytes(path)?;
    job(unsafe { trueos_cabi_async_fs_remove_leaf_start_v1(path.as_ptr(), path.len(), kind) })
        .map(|_| ())
}
pub fn remove_dir_all(path: &Path) -> io::Result<()> {
    crate::sys::fs::common::remove_dir_all(path)
}
pub fn rename(old: &Path, new: &Path) -> io::Result<()> {
    let old = path_bytes(old)?;
    let new = path_bytes(new)?;
    job(unsafe {
        trueos_cabi_async_fs_rename_start_v2(old.as_ptr(), old.len(), new.as_ptr(), new.len(), 1)
    })
    .map(|_| ())
}
pub fn exists(path: &Path) -> io::Result<bool> {
    match stat(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}
pub fn set_perm(_: &Path, _: FilePermissions) -> io::Result<()> {
    unsupported()
}
pub fn set_times(_: &Path, _: FileTimes) -> io::Result<()> {
    unsupported()
}
pub fn set_times_nofollow(_: &Path, _: FileTimes) -> io::Result<()> {
    unsupported()
}
pub fn readlink(_: &Path) -> io::Result<PathBuf> {
    unsupported()
}
pub fn symlink(_: &Path, _: &Path) -> io::Result<()> {
    unsupported()
}
pub fn link(_: &Path, _: &Path) -> io::Result<()> {
    unsupported()
}
pub fn canonicalize(path: &Path) -> io::Result<PathBuf> {
    stat(path)?;
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        crate::env::current_dir()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    Ok(normalized)
}
pub fn copy(from: &Path, to: &Path) -> io::Result<u64> {
    let mut read_options = OpenOptions::new();
    read_options.read(true);
    let input = File::open(from, &read_options)?;
    let source = input.file_attr()?;
    if let Ok(destination) = stat(to) {
        if source.root_id == destination.root_id && source.file_id == destination.file_id {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "copy source and destination are the same native object",
            ));
        }
    }
    let mut write_options = OpenOptions::new();
    write_options.write(true);
    write_options.create(true);
    write_options.truncate(true);
    let output = File::open(to, &write_options)?;
    let mut total = 0;
    let mut buffer = [0; CHUNK];
    loop {
        let got = input.read(&mut buffer)?;
        if got == 0 {
            break;
        }
        let mut offset = 0;
        while offset < got {
            let count = output.write(&buffer[offset..got])?;
            if count == 0 {
                return Err(io::Error::WRITE_ALL_EOF);
            }
            offset += count;
        }
        total += got as u64;
    }
    Ok(total)
}
pub fn readdir(path: &Path) -> io::Result<ReadDir> {
    let raw = path_bytes(path)?;
    let bytes = job(unsafe { trueos_cabi_async_fs_list_dir_start(raw.as_ptr(), raw.len()) })?;
    if bytes.len() < 12 || &bytes[..4] != b"TDL1" || bytes[4] != 0 {
        return Err(error(-7));
    }
    let count = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let mut offset = 12;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        if offset + 4 > bytes.len() {
            return Err(error(-2));
        }
        let len = u16::from_le_bytes(bytes[offset + 2..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        let end = offset.checked_add(len).ok_or_else(|| error(-2))?;
        let name = crate::str::from_utf8(bytes.get(offset..end).ok_or_else(|| error(-2))?)
            .map_err(|_| error(-2))?;
        if name.is_empty() || name == "." || name == ".." || name.contains('/') {
            return Err(error(-2));
        }
        let entry_path = path.join(name);
        entries.push(stat(&entry_path).map(|metadata| DirEntry {
            path: entry_path,
            name: OsString::from(name),
            metadata,
        }));
        offset = end;
    }
    if offset != bytes.len() {
        return Err(error(-2));
    }
    Ok(ReadDir {
        entries: entries.into_iter(),
    })
}
// Existing public Unix extension functions cannot invent native permission,
// ownership, descriptor, special-file or process-root contracts.
pub fn chown(_: &Path, _: u32, _: u32) -> io::Result<()> {
    unsupported()
}
pub fn lchown(_: &Path, _: u32, _: u32) -> io::Result<()> {
    unsupported()
}
pub fn fchown(_: i32, _: u32, _: u32) -> io::Result<()> {
    unsupported()
}
pub fn chroot(_: &Path) -> io::Result<()> {
    unsupported()
}
pub fn mkfifo(_: &Path, _: u32) -> io::Result<()> {
    unsupported()
}
pub(crate) fn debug_assert_fd_is_open(_: i32) {}

impl crate::sys::FromInner<u32> for FilePermissions {
    fn from_inner(mode: u32) -> Self {
        Self::from_mode(mode)
    }
}
