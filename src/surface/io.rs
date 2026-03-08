extern crate alloc;

use alloc::vec::Vec;
use core::fmt;

/// Minimal I/O surface.
///
/// This started as a std::io-like layer, but the kernel currently only needs a
/// small subset plus a console routing choke-point.

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorKind {
    WriteZero,
    Interrupted,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Error {
    kind: ErrorKind,
}

impl Error {
    pub const fn new(kind: ErrorKind) -> Self {
        Self { kind }
    }

    pub const fn kind(&self) -> ErrorKind {
        self.kind
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self.kind {
            ErrorKind::WriteZero => "failed to write whole buffer",
            ErrorKind::Interrupted => "operation interrupted",
            ErrorKind::Other => "io error",
        };
        f.write_str(msg)
    }
}

/// Trait for byte sinks.
pub trait Write {
    fn write(&mut self, buf: &[u8]) -> Result<usize>;

    fn flush(&mut self) -> Result<()>;

    fn write_all(&mut self, mut buf: &[u8]) -> Result<()> {
        while !buf.is_empty() {
            match self.write(buf) {
                Ok(0) => return Err(Error::new(ErrorKind::WriteZero)),
                Ok(n) => buf = &buf[n..],
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}

/// Kernel-facing helpers for basic file I/O.
///
/// These expose the TRUEOSFS root filesystem operations used by the shell,
/// but keep the logic in one place.
pub mod kfs {
    use super::Vec;
    use crate::disc::block;
    use alloc::string::String;

    pub type Result<T> = core::result::Result<T, FsError>;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum FsError {
        /// No TRUEOSFS root is currently mounted/selected.
        NoRoot,
        BadPath,
        NoSpace,
        NotFound,
        AlreadyExists,
        Device(block::Error),
    }

    impl From<block::Error> for FsError {
        fn from(value: block::Error) -> Self {
            FsError::Device(value)
        }
    }

    fn root_disk() -> Result<block::DeviceHandle> {
        crate::v::fs::trueosfs::primary_root_handle().ok_or(FsError::NoRoot)
    }

    fn normalize_rel(path: &str, allow_empty: bool) -> Result<String> {
        let mut out = String::new();
        let t = path.trim();
        if t.is_empty() {
            return if allow_empty {
                Ok(out)
            } else {
                Err(FsError::BadPath)
            };
        }

        for part in t.split('/') {
            if part.is_empty() || part == "." {
                continue;
            }
            if part == ".." {
                return Err(FsError::BadPath);
            }
            if !out.is_empty() {
                out.push('/');
            }
            out.push_str(part);
        }

        if out.is_empty() && !allow_empty {
            return Err(FsError::BadPath);
        }
        Ok(out)
    }

    #[inline]
    pub fn read_file(path: &str) -> Result<Vec<u8>> {
        let disk = root_disk()?;
        let name = normalize_rel(path, false)?;
        crate::wait::spawn_and_wait_local(async move {
            match crate::v::fs::trueosfs::file_out_async(disk, name.as_str()).await? {
                Some(bytes) => Ok(bytes),
                None => Err(FsError::NotFound),
            }
        })
    }

    #[inline]
    pub fn read_file_len(path: &str) -> Result<usize> {
        let disk = root_disk()?;
        let name = normalize_rel(path, false)?;
        crate::wait::spawn_and_wait_local(async move {
            match crate::v::fs::trueosfs::file_info_async(disk, name.as_str()).await? {
                Some(info) => Ok(info.data_len as usize),
                None => Err(FsError::NotFound),
            }
        })
    }

    #[inline]
    pub async fn read_file_async(path: &str) -> Result<Vec<u8>> {
        let disk = root_disk()?;
        let name = normalize_rel(path, false)?;
        match crate::v::fs::trueosfs::file_out_async(disk, name.as_str()).await? {
            Some(bytes) => Ok(bytes),
            None => Err(FsError::NotFound),
        }
    }

    #[inline]
    pub async fn read_file_len_async(path: &str) -> Result<usize> {
        let disk = root_disk()?;
        let name = normalize_rel(path, false)?;
        match crate::v::fs::trueosfs::file_info_async(disk, name.as_str()).await? {
            Some(info) => Ok(info.data_len as usize),
            None => Err(FsError::NotFound),
        }
    }

    #[inline]
    pub fn write_file_begin(path: &str, total_len: u64) -> Result<u32> {
        let disk = root_disk()?;
        let name = normalize_rel(path, false)?;
        crate::wait::spawn_and_wait_local(async move {
            match crate::v::fs::trueosfs::file_write_begin_async(disk, name.as_str(), total_len)
                .await?
            {
                Some(h) => Ok(h),
                None => Err(FsError::NoSpace),
            }
        })
    }

    #[inline]
    pub async fn write_file_begin_async(path: &str, total_len: u64) -> Result<u32> {
        let disk = root_disk()?;
        let name = normalize_rel(path, false)?;
        match crate::v::fs::trueosfs::file_write_begin_async(disk, name.as_str(), total_len).await?
        {
            Some(h) => Ok(h),
            None => Err(FsError::NoSpace),
        }
    }

    #[inline]
    pub fn write_file_chunk(handle: u32, data: &[u8]) -> Result<()> {
        let data = data.to_vec();
        crate::wait::spawn_and_wait_local(async move {
            crate::v::fs::trueosfs::file_write_chunk_async(handle, data.as_slice()).await?;
            Ok(())
        })
    }

    #[inline]
    pub async fn write_file_chunk_async(handle: u32, data: &[u8]) -> Result<()> {
        crate::v::fs::trueosfs::file_write_chunk_async(handle, data).await?;
        Ok(())
    }

    #[inline]
    pub fn write_file_finish(handle: u32) -> Result<()> {
        crate::wait::spawn_and_wait_local(async move {
            crate::v::fs::trueosfs::file_write_finish_async(handle).await?;
            Ok(())
        })
    }

    #[inline]
    pub async fn write_file_finish_async(handle: u32) -> Result<()> {
        crate::v::fs::trueosfs::file_write_finish_async(handle).await?;
        Ok(())
    }

    #[inline]
    pub fn write_file_abort(handle: u32) -> Result<()> {
        crate::wait::spawn_and_wait_local(async move {
            crate::v::fs::trueosfs::file_write_abort_async(handle).await?;
            Ok(())
        })
    }

    #[inline]
    pub async fn write_file_abort_async(handle: u32) -> Result<()> {
        crate::v::fs::trueosfs::file_write_abort_async(handle).await?;
        Ok(())
    }

    #[inline]
    pub fn rename(src: &str, dst: &str) -> Result<()> {
        let disk = root_disk()?;
        let src = normalize_rel(src, false)?;
        let dst = normalize_rel(dst, false)?;
        crate::wait::spawn_and_wait_local(async move {
            if src == dst {
                return Ok(());
            }
            if crate::v::fs::trueosfs::file_exists_async(disk, dst.as_str()).await? {
                return Err(FsError::AlreadyExists);
            }
            let Some(bytes) = crate::v::fs::trueosfs::file_out_async(disk, src.as_str()).await?
            else {
                return Err(FsError::NotFound);
            };
            let ok =
                crate::v::fs::trueosfs::file_in_async(disk, dst.as_str(), bytes.as_slice()).await?;
            if !ok {
                return Err(FsError::NoSpace);
            }
            let _ = crate::v::fs::trueosfs::file_delete_async(disk, src.as_str()).await;
            Ok(())
        })
    }

    #[inline]
    pub async fn rename_async(src: &str, dst: &str) -> Result<()> {
        let disk = root_disk()?;
        let src = normalize_rel(src, false)?;
        let dst = normalize_rel(dst, false)?;
        if src == dst {
            return Ok(());
        }
        if crate::v::fs::trueosfs::file_exists_async(disk, dst.as_str()).await? {
            return Err(FsError::AlreadyExists);
        }
        let Some(bytes) = crate::v::fs::trueosfs::file_out_async(disk, src.as_str()).await? else {
            return Err(FsError::NotFound);
        };
        let ok =
            crate::v::fs::trueosfs::file_in_async(disk, dst.as_str(), bytes.as_slice()).await?;
        if !ok {
            return Err(FsError::NoSpace);
        }
        let _ = crate::v::fs::trueosfs::file_delete_async(disk, src.as_str()).await;
        Ok(())
    }

    #[inline]
    pub fn list_dir(path: &str) -> Result<String> {
        let disk = root_disk()?;
        let dir = normalize_rel(path, true)?;
        crate::wait::spawn_and_wait_local(async move {
            match crate::v::fs::trueosfs::list_dir_async(disk, dir.as_str()).await? {
                Some(v) => Ok(v),
                None => Err(FsError::NoRoot),
            }
        })
    }

    #[inline]
    pub async fn list_dir_async(path: &str) -> Result<String> {
        let disk = root_disk()?;
        let dir = normalize_rel(path, true)?;
        match crate::v::fs::trueosfs::list_dir_async(disk, dir.as_str()).await? {
            Some(v) => Ok(v),
            None => Err(FsError::NoRoot),
        }
    }

    #[inline]
    pub fn remove(path: &str) -> Result<()> {
        let disk = root_disk()?;
        let name = normalize_rel(path, false)?;
        crate::wait::spawn_and_wait_local(async move {
            let ok = crate::v::fs::trueosfs::file_delete_async(disk, name.as_str()).await?;
            if ok { Ok(()) } else { Err(FsError::NotFound) }
        })
    }

    #[inline]
    pub async fn remove_async(path: &str) -> Result<()> {
        let disk = root_disk()?;
        let name = normalize_rel(path, false)?;
        let ok = crate::v::fs::trueosfs::file_delete_async(disk, name.as_str()).await?;
        if ok { Ok(()) } else { Err(FsError::NotFound) }
    }

    #[inline]
    pub fn create_dir_all(path: &str) -> Result<()> {
        // TRUEOSFS is key-based; directories are implied by path prefixes.
        // Still require a mounted root so callers can use this as an "FS ready" probe.
        let _disk = root_disk()?;
        let _ = normalize_rel(path, true)?;
        Ok(())
    }

    #[inline]
    pub fn exists(path: &str) -> Result<bool> {
        let disk = root_disk()?;
        let name = normalize_rel(path, false)?;
        crate::wait::spawn_and_wait_local(async move {
            Ok(crate::v::fs::trueosfs::file_exists_async(disk, name.as_str()).await?)
        })
    }

    #[inline]
    pub async fn exists_async(path: &str) -> Result<bool> {
        let disk = root_disk()?;
        let name = normalize_rel(path, false)?;
        Ok(crate::v::fs::trueosfs::file_exists_async(disk, name.as_str()).await?)
    }

    #[inline]
    pub fn is_root_read_only() -> Result<bool> {
        crate::v::fs::trueosfs::primary_root_is_read_only().ok_or(FsError::NoRoot)
    }

    /// Append `src` bytes into the file at `dst_path`, creating the file if needed.
    pub fn append_into_file(dst_path: &str, src: &[u8]) -> Result<()> {
        let disk = root_disk()?;
        let name = normalize_rel(dst_path, false)?;
        let src = src.to_vec();
        crate::wait::spawn_and_wait_local(async move {
            let ok = crate::v::fs::trueosfs::file_append_async(disk, name.as_str(), src.as_slice())
                .await?;
            if ok { Ok(()) } else { Err(FsError::NoSpace) }
        })
    }

    /// Async variant of [`append_into_file`].
    pub async fn append_into_file_async(dst_path: &str, src: &[u8]) -> Result<()> {
        let disk = root_disk()?;
        let name = normalize_rel(dst_path, false)?;
        let ok = crate::v::fs::trueosfs::file_append_async(disk, name.as_str(), src).await?;
        if ok { Ok(()) } else { Err(FsError::NoSpace) }
    }
}

/// Console routing + C ABI entrypoints used by embedded C code (QuickJS etc).
pub mod cabi {
    include!("cabi_codes.rs");

    #[repr(u32)]
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum CStream {
        Stdout = 1,
        Stderr = 2,
    }

    #[inline]
    fn fs_error_to_code(err: super::kfs::FsError) -> i32 {
        use super::kfs::FsError;
        match err {
            FsError::NoRoot => FS_ERR_USBMS_NOT_FOUND,
            FsError::BadPath => FS_ERR_BAD_PATH,
            FsError::NoSpace => FS_ERR_NO_SPACE,
            FsError::NotFound => FS_ERR_NOT_FOUND,
            FsError::AlreadyExists => FS_ERR_ALREADY_EXISTS,
            FsError::Device(e) => match e {
                crate::disc::block::Error::InvalidParam => FS_ERR_BAD_PARAM,
                crate::disc::block::Error::OutOfBounds => FS_ERR_BAD_PARAM,
                crate::disc::block::Error::NotReady => FS_ERR_USBMS_NOT_FOUND,
                crate::disc::block::Error::NotSupported => FS_ERR_IO,
                crate::disc::block::Error::Timeout => FS_ERR_IO,
                crate::disc::block::Error::Io => FS_ERR_IO,
                crate::disc::block::Error::Corrupted => FS_ERR_IO,
                crate::disc::block::Error::DmaUnavailable => FS_ERR_IO,
                crate::disc::block::Error::MmioMapFailed => FS_ERR_IO,
            },
        }
    }

    #[inline]
    pub fn write_bytes(stream: CStream, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }

        // Prefer routing through the existing global logger (debugcon + VGA + USB).
        // If the byte stream isn't valid UTF-8, fall back to raw debugcon output.
        match core::str::from_utf8(bytes) {
            Ok(s) => match stream {
                CStream::Stdout => crate::globalog::log(format_args!("{}", s)),
                CStream::Stderr => crate::globalog::log(format_args!("[stderr] {}", s)),
            },
            Err(_) => {
                for &b in bytes {
                    crate::globalog::debugcon_write_byte_raw(b);
                }
            }
        }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_write(stream: u32, bytes: *const u8, len: usize) {
        if bytes.is_null() || len == 0 {
            return;
        }

        let stream = match stream {
            1 => CStream::Stdout,
            2 => CStream::Stderr,
            _ => CStream::Stdout,
        };
        let slice = core::slice::from_raw_parts(bytes, len);
        write_bytes(stream, slice);
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn trueos_cabi_poll_once() {
        // This function is used by QuickJS smokes (and other C-ABI callers) as a
        // cooperative yield point while polling for async completions.
        //
        // Do NOT call `park_step()` here: it may execute `hlt`, and on configurations
        // without a reliable periodic interrupt source that can wake the CPU, that
        // can present as a hard BSP freeze (e.g. right after `qjs-pixi-rect-smoke: starting`).
        crate::wait::spin_step();
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_write_cstr(stream: u32, cstr: *const u8) {
        if cstr.is_null() {
            return;
        }
        let mut len: usize = 0;
        while *cstr.add(len) != 0 {
            len = len.saturating_add(1);
            // Hard cap to avoid runaway scans on malformed pointers.
            if len > (1024 * 1024) {
                break;
            }
        }
        trueos_cabi_write(stream, cstr, len);
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_copy_cstr_into(
        dst: *mut u8,
        cap: usize,
        cstr: *const u8,
    ) -> i32 {
        if cstr.is_null() {
            return 0;
        }
        let mut src_len: usize = 0;
        while *cstr.add(src_len) != 0 {
            src_len = src_len.saturating_add(1);
            if src_len > (1024 * 1024) {
                break;
            }
        }

        // Match snprintf-ish semantics: return the full source length (excluding NUL).
        if dst.is_null() || cap == 0 {
            return src_len as i32;
        }
        let n = core::cmp::min(src_len, cap.saturating_sub(1));
        core::ptr::copy_nonoverlapping(cstr, dst, n);
        *dst.add(n) = 0;
        src_len as i32
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn trueos_cabi_boot_timestamp_secs() -> u64 {
        crate::limine::boot_timestamp_secs().unwrap_or(0)
    }

    #[derive(Clone, Copy)]
    struct AllocMeta {
        size: usize,
        align: usize,
    }

    static CABI_ALLOC_TABLE: spin::Mutex<alloc::collections::BTreeMap<usize, AllocMeta>> =
        spin::Mutex::new(alloc::collections::BTreeMap::new());

    #[inline]
    fn cabi_malloc_align() -> usize {
        core::cmp::max(core::mem::align_of::<usize>(), 16)
    }

    #[inline]
    fn cabi_layout_for(size: usize, align: usize) -> Option<alloc::alloc::Layout> {
        if size == 0 {
            return None;
        }
        alloc::alloc::Layout::from_size_align(size, align).ok()
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_alloc(size: usize) -> *mut u8 {
        let align = cabi_malloc_align();
        let Some(layout) = cabi_layout_for(size, align) else {
            return core::ptr::null_mut();
        };
        let p = alloc::alloc::alloc(layout);
        if p.is_null() {
            return core::ptr::null_mut();
        }
        CABI_ALLOC_TABLE
            .lock()
            .insert(p as usize, AllocMeta { size, align });
        p
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_calloc(nmemb: usize, size: usize) -> *mut u8 {
        let Some(total) = nmemb.checked_mul(size) else {
            return core::ptr::null_mut();
        };
        if total == 0 {
            return core::ptr::null_mut();
        }
        let p = trueos_cabi_alloc(total);
        if !p.is_null() {
            core::ptr::write_bytes(p, 0, total);
        }
        p
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_free(ptr: *mut u8) {
        if ptr.is_null() {
            return;
        }
        let meta = CABI_ALLOC_TABLE.lock().remove(&(ptr as usize));
        let Some(meta) = meta else {
            return;
        };
        let Some(layout) = cabi_layout_for(meta.size, meta.align) else {
            return;
        };
        alloc::alloc::dealloc(ptr, layout);
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_realloc(ptr: *mut u8, size: usize) -> *mut u8 {
        if ptr.is_null() {
            return trueos_cabi_alloc(size);
        }
        if size == 0 {
            trueos_cabi_free(ptr);
            return core::ptr::null_mut();
        }

        let old_meta = {
            let map = CABI_ALLOC_TABLE.lock();
            map.get(&(ptr as usize)).copied()
        };
        let Some(old_meta) = old_meta else {
            return core::ptr::null_mut();
        };

        let new_ptr = trueos_cabi_alloc(size);
        if new_ptr.is_null() {
            return core::ptr::null_mut();
        }

        let copy_len = core::cmp::min(old_meta.size, size);
        core::ptr::copy_nonoverlapping(ptr, new_ptr, copy_len);
        trueos_cabi_free(ptr);
        new_ptr
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_malloc_usable_size(ptr: *const u8) -> usize {
        if ptr.is_null() {
            return 0;
        }
        CABI_ALLOC_TABLE
            .lock()
            .get(&(ptr as usize))
            .map(|m| m.size)
            .unwrap_or(0)
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_fs_read_file(
        path_ptr: *const u8,
        path_len: usize,
        out_ptr: *mut u8,
        out_cap: usize,
    ) -> isize {
        if path_ptr.is_null() && path_len != 0 {
            return FS_ERR_BAD_PARAM as isize;
        }
        if path_len > QJS_ASYNC_FS_MAX_PATH {
            return FS_ERR_TOO_LARGE as isize;
        }
        let path_bytes = core::slice::from_raw_parts(path_ptr, path_len);
        let Ok(path) = core::str::from_utf8(path_bytes) else {
            return FS_ERR_BAD_UTF8 as isize;
        };

        if out_ptr.is_null() || out_cap == 0 {
            return match super::kfs::read_file_len(path) {
                Ok(len) => len as isize,
                Err(e) => fs_error_to_code(e) as isize,
            };
        }

        match super::kfs::read_file(path) {
            Ok(bytes) => {
                if bytes.len() > out_cap {
                    return FS_ERR_NO_SPACE as isize;
                }
                core::ptr::copy_nonoverlapping(bytes.as_ptr(), out_ptr, bytes.len());
                bytes.len() as isize
            }
            Err(e) => fs_error_to_code(e) as isize,
        }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_fs_write_begin(
        path_ptr: *const u8,
        path_len: usize,
        total_len: u64,
        out_handle: *mut u32,
    ) -> i32 {
        if out_handle.is_null() {
            return FS_ERR_BAD_PARAM;
        }
        if path_ptr.is_null() && path_len != 0 {
            return FS_ERR_BAD_PARAM;
        }
        if path_len > QJS_ASYNC_FS_MAX_PATH {
            return FS_ERR_TOO_LARGE;
        }
        let path_bytes = core::slice::from_raw_parts(path_ptr, path_len);
        let Ok(path) = core::str::from_utf8(path_bytes) else {
            return FS_ERR_BAD_UTF8;
        };
        match super::kfs::write_file_begin(path, total_len) {
            Ok(h) => {
                *out_handle = h;
                0
            }
            Err(e) => fs_error_to_code(e),
        }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_fs_write_chunk(
        handle: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32 {
        if data_ptr.is_null() && data_len != 0 {
            return FS_ERR_BAD_PARAM;
        }
        let data = if data_len == 0 {
            &[]
        } else {
            core::slice::from_raw_parts(data_ptr, data_len)
        };
        match super::kfs::write_file_chunk(handle, data) {
            Ok(()) => 0,
            Err(e) => fs_error_to_code(e),
        }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_fs_write_finish(handle: u32) -> i32 {
        match super::kfs::write_file_finish(handle) {
            Ok(()) => 0,
            Err(e) => fs_error_to_code(e),
        }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_fs_write_abort(handle: u32) -> i32 {
        match super::kfs::write_file_abort(handle) {
            Ok(()) => 0,
            Err(e) => fs_error_to_code(e),
        }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_fs_rename(
        src_ptr: *const u8,
        src_len: usize,
        dst_ptr: *const u8,
        dst_len: usize,
    ) -> i32 {
        if (src_ptr.is_null() && src_len != 0) || (dst_ptr.is_null() && dst_len != 0) {
            return FS_ERR_BAD_PARAM;
        }
        if src_len > QJS_ASYNC_FS_MAX_PATH || dst_len > QJS_ASYNC_FS_MAX_PATH {
            return FS_ERR_TOO_LARGE;
        }
        let src_bytes = core::slice::from_raw_parts(src_ptr, src_len);
        let dst_bytes = core::slice::from_raw_parts(dst_ptr, dst_len);
        let Ok(src) = core::str::from_utf8(src_bytes) else {
            return FS_ERR_BAD_UTF8;
        };
        let Ok(dst) = core::str::from_utf8(dst_bytes) else {
            return FS_ERR_BAD_UTF8;
        };
        match super::kfs::rename(src, dst) {
            Ok(()) => 0,
            Err(e) => fs_error_to_code(e),
        }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_fs_list_dir(
        path_ptr: *const u8,
        path_len: usize,
        out_ptr: *mut u8,
        out_cap: usize,
    ) -> isize {
        if path_ptr.is_null() && path_len != 0 {
            return FS_ERR_BAD_PARAM as isize;
        }
        if path_len > QJS_ASYNC_FS_MAX_PATH {
            return FS_ERR_TOO_LARGE as isize;
        }
        let path_bytes = core::slice::from_raw_parts(path_ptr, path_len);
        let Ok(path) = core::str::from_utf8(path_bytes) else {
            return FS_ERR_BAD_UTF8 as isize;
        };

        match super::kfs::list_dir(path) {
            Ok(s) => {
                let bytes = s.as_bytes();
                if out_ptr.is_null() || out_cap == 0 {
                    return bytes.len() as isize;
                }
                if bytes.len() > out_cap {
                    return FS_ERR_NO_SPACE as isize;
                }
                core::ptr::copy_nonoverlapping(bytes.as_ptr(), out_ptr, bytes.len());
                bytes.len() as isize
            }
            Err(e) => fs_error_to_code(e) as isize,
        }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_fs_remove(path_ptr: *const u8, path_len: usize) -> i32 {
        if path_ptr.is_null() && path_len != 0 {
            return FS_ERR_BAD_PARAM;
        }
        if path_len > QJS_ASYNC_FS_MAX_PATH {
            return FS_ERR_TOO_LARGE;
        }
        let path_bytes = core::slice::from_raw_parts(path_ptr, path_len);
        let Ok(path) = core::str::from_utf8(path_bytes) else {
            return FS_ERR_BAD_UTF8;
        };
        match super::kfs::remove(path) {
            Ok(()) => 0,
            Err(e) => fs_error_to_code(e),
        }
    }

    // --- GFX C-ABI ---
    // This is the stable bridge between the in-kernel JS "WebGL" shim and the renderer.
    // It intentionally targets the gfx abstraction (`trueos_gfx_core`) rather than a GPU driver.

    use crate::usb;
    use alloc::vec::Vec;
    use trueos_gfx_core::{
        BlendDesc, BlendFactor, BufferDesc, BufferId, BufferUsage, ColorFormat, Command,
        CommandBuffer, Extent2D, GfxContext, ImageDesc, ImageFormat, ImageId, MemoryType,
        PipelineDesc, PipelineId, SamplerDesc, SamplerFilter, SamplerWrap, SwapchainDesc,
        TexCoordFormat, VertexLayout, Viewport,
    };

    const GFX_CABI_VBUF_RING_LEN: usize = 3;
    // Shared draw chunk budget used by cmd-stream draw capture paths.
    const MAX_CMDSTREAM_DRAW_BYTES: usize = 64 * 1024;
    // Conservative pre-submit guard to avoid submit_3d request overflow.
    const MAX_EST_SUBMIT_BYTES: usize = 240 * 1024;
    static SUBMIT_BUDGET_LOGS: core::sync::atomic::AtomicU32 =
        core::sync::atomic::AtomicU32::new(0);

    struct GfxCabiState {
        pipeline: PipelineId,
        ring_idx: usize,
        vbuf: [BufferId; GFX_CABI_VBUF_RING_LEN],
        capacity: [usize; GFX_CABI_VBUF_RING_LEN],
        tex_pipeline: PipelineId,
        tex_vbuf: [BufferId; GFX_CABI_VBUF_RING_LEN],
        tex_capacity: [usize; GFX_CABI_VBUF_RING_LEN],
        tex_images: Option<Vec<Option<TexImage>>>,
        epoch: u64,
        swapchain_configured: bool,
        swapchain_desc: SwapchainDesc,
        viewport_configured: bool,
        frame_active: bool,
        frame_clear_rgb: u32,
        frame_seq: u32,
        frame_rgb_draws: u32,
        frame_tex_draws: u32,
        frame_draw_bytes: usize,
        frame_draws: Vec<PendingDraw>,
        frame_rgb_blob: Vec<u8>,
        frame_tex_blob: Vec<u8>,
        cursor_frame_active: bool,
        cursor_frame_seq: u32,
        cursor_rgb_draws: u32,
        cursor_tex_draws: u32,
        cursor_draw_bytes: usize,
        cursor_draws: Vec<PendingDraw>,
        cursor_rgb_blob: Vec<u8>,
        cursor_tex_blob: Vec<u8>,
        base_cache_valid: bool,
        base_cache_updated_at_ticks: u64,
        base_cache_clear_rgb: u32,
        base_cache_draws: Vec<PendingDraw>,
        base_cache_rgb_blob: Vec<u8>,
        base_cache_tex_blob: Vec<u8>,
        cursor_cache_valid: bool,
        cursor_cache_draws: Vec<PendingDraw>,
        cursor_cache_rgb_blob: Vec<u8>,
        cursor_cache_tex_blob: Vec<u8>,
        // Current sampler state (set by the WebGL shim) that will be captured per textured draw.
        cur_sampler: SamplerDesc,
        // Current blend state (set by the WebGL shim) that will be captured per draw.
        cur_blend: BlendDesc,
        // Optional scissor clip in viewport pixel coordinates.
        cur_scissor: Option<ScissorRect>,
        last_missing_tex_id: u32,
        missing_tex_logs: u32,
    }

    #[derive(Clone, Copy)]
    struct ScissorRect {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    }

    struct TexImage {
        image: ImageId,
        width: u32,
        height: u32,
    }

    #[derive(Clone, Copy)]
    enum PendingDraw {
        Rgb {
            blob_offset: usize,
            blob_len: usize,
            blend: BlendDesc,
        },
        Tex {
            tex_id: u32,
            image: ImageId,
            sampler: SamplerDesc,
            blob_offset: usize,
            blob_len: usize,
            blend: BlendDesc,
        },
    }

    impl GfxCabiState {
        const fn new() -> Self {
            Self {
                pipeline: PipelineId::invalid(),
                ring_idx: 0,
                vbuf: [BufferId::invalid(); GFX_CABI_VBUF_RING_LEN],
                capacity: [0; GFX_CABI_VBUF_RING_LEN],
                tex_pipeline: PipelineId::invalid(),
                tex_vbuf: [BufferId::invalid(); GFX_CABI_VBUF_RING_LEN],
                tex_capacity: [0; GFX_CABI_VBUF_RING_LEN],
                tex_images: None,
                epoch: 0,
                swapchain_configured: false,
                swapchain_desc: SwapchainDesc {
                    format: ImageFormat::Rgbx8888,
                    extent: Extent2D {
                        width: 0,
                        height: 0,
                    },
                },
                viewport_configured: false,
                frame_active: false,
                frame_clear_rgb: 0x00ff_ffff,
                frame_seq: 0,
                frame_rgb_draws: 0,
                frame_tex_draws: 0,
                frame_draw_bytes: 0,
                frame_draws: Vec::new(),
                frame_rgb_blob: Vec::new(),
                frame_tex_blob: Vec::new(),
                cursor_frame_active: false,
                cursor_frame_seq: 0,
                cursor_rgb_draws: 0,
                cursor_tex_draws: 0,
                cursor_draw_bytes: 0,
                cursor_draws: Vec::new(),
                cursor_rgb_blob: Vec::new(),
                cursor_tex_blob: Vec::new(),
                base_cache_valid: false,
                base_cache_updated_at_ticks: 0,
                base_cache_clear_rgb: 0x00ff_ffff,
                base_cache_draws: Vec::new(),
                base_cache_rgb_blob: Vec::new(),
                base_cache_tex_blob: Vec::new(),
                cursor_cache_valid: false,
                cursor_cache_draws: Vec::new(),
                cursor_cache_rgb_blob: Vec::new(),
                cursor_cache_tex_blob: Vec::new(),
                cur_sampler: SamplerDesc {
                    wrap_s: SamplerWrap::ClampToEdge,
                    wrap_t: SamplerWrap::ClampToEdge,
                    min_filter: SamplerFilter::Linear,
                    mag_filter: SamplerFilter::Linear,
                },
                cur_blend: BlendDesc::disabled(),
                cur_scissor: None,
                last_missing_tex_id: 0,
                missing_tex_logs: 0,
            }
        }
    }

    #[derive(Clone, Copy)]
    struct RgbVtx {
        x: f32,
        y: f32,
        r: f32,
        g: f32,
        b: f32,
        a: f32,
    }

    #[inline]
    fn clamp01(v: f32) -> f32 {
        if v <= 0.0 {
            0.0
        } else if v >= 1.0 {
            1.0
        } else {
            v
        }
    }

    #[inline]
    fn lerp(a: f32, b: f32, t: f32) -> f32 {
        a + (b - a) * t
    }

    #[inline]
    fn read_rgb_vtx(bytes: &[u8], off: usize) -> Option<RgbVtx> {
        if off + 12 > bytes.len() {
            return None;
        }
        let x = f32::from_le_bytes([bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3]]);
        let y = f32::from_le_bytes([
            bytes[off + 4],
            bytes[off + 5],
            bytes[off + 6],
            bytes[off + 7],
        ]);
        Some(RgbVtx {
            x,
            y,
            r: (bytes[off + 8] as f32) / 255.0,
            g: (bytes[off + 9] as f32) / 255.0,
            b: (bytes[off + 10] as f32) / 255.0,
            a: (bytes[off + 11] as f32) / 255.0,
        })
    }

    #[inline]
    fn push_rgb_vtx(out: &mut Vec<u8>, v: RgbVtx) {
        out.extend_from_slice(&v.x.to_le_bytes());
        out.extend_from_slice(&v.y.to_le_bytes());
        out.push((clamp01(v.r) * 255.0 + 0.5) as u8);
        out.push((clamp01(v.g) * 255.0 + 0.5) as u8);
        out.push((clamp01(v.b) * 255.0 + 0.5) as u8);
        out.push((clamp01(v.a) * 255.0 + 0.5) as u8);
    }

    const CURSOR_TICK_SUPPRESS_AFTER_BASE_MS: u64 = 24;

    fn append_kernel_cursor_overlay_draws(
        draws: &mut Vec<PendingDraw>,
        rgb_blob: &mut Vec<u8>,
        vp_w: u32,
        vp_h: u32,
    ) {
        let blob_offset = rgb_blob.len();
        crate::surface::cursor::append_kernel_cursor_overlay_rgb(rgb_blob, vp_w, vp_h);

        let blob_len = rgb_blob.len().saturating_sub(blob_offset);
        if blob_len == 0 {
            return;
        }

        draws.push(PendingDraw::Rgb {
            blob_offset,
            blob_len,
            blend: BlendDesc::straight_alpha(),
        });
    }

    // Internal kernel-side cursor refresh path. This keeps cursor motion alive
    // even when app/UI rendering is on-demand and no new base frames are submitted.
    pub fn kernel_cursor_overlay_tick() -> i32 {
        crate::gfx::init(crate::limine::framebuffer_response());

        let now_ticks = embassy_time_driver::now();
        let suppress_ticks = ((embassy_time_driver::TICK_HZ as u64)
            .saturating_mul(CURSOR_TICK_SUPPRESS_AFTER_BASE_MS)
            .saturating_add(999))
            / 1000;

        let should_tick = {
            let st = GFX_CABI_STATE.lock();
            st.base_cache_valid
                && !st.frame_active
                && !st.cursor_frame_active
                && now_ticks.saturating_sub(st.base_cache_updated_at_ticks) >= suppress_ticks
        };
        if !should_tick {
            return 0;
        }

        let Some((vp_w, vp_h)) = crate::gfx::with_context(|ctx| {
            let e = ctx.swapchain_desc().extent;
            (e.width, e.height)
        }) else {
            return -12;
        };

        let mut draws: Vec<PendingDraw> = Vec::new();
        let mut rgb_blob: Vec<u8> = Vec::new();
        append_kernel_cursor_overlay_draws(&mut draws, &mut rgb_blob, vp_w, vp_h);
        if draws.is_empty() || rgb_blob.is_empty() {
            return 0;
        }

        let rc_begin = unsafe { trueos_cabi_gfx_cursor_begin_frame() };
        if rc_begin != 0 {
            return rc_begin;
        }

        // Straight-alpha for the overlay markers.
        let _ = unsafe { trueos_cabi_gfx_set_blend(1, 0x0302, 0x0303, 0x0302, 0x0303, 0, 0) };

        let rc_draw = unsafe {
            trueos_cabi_gfx_cursor_draw_rgb_triangles_no_present(rgb_blob.as_ptr(), rgb_blob.len())
        };
        if rc_draw != 0 {
            let _ = unsafe { trueos_cabi_gfx_cursor_end_frame() };
            return rc_draw;
        }

        unsafe { trueos_cabi_gfx_cursor_end_frame() }
    }

    #[inline]
    fn interp_rgb(a: RgbVtx, b: RgbVtx, t: f32) -> RgbVtx {
        RgbVtx {
            x: lerp(a.x, b.x, t),
            y: lerp(a.y, b.y, t),
            r: lerp(a.r, b.r, t),
            g: lerp(a.g, b.g, t),
            b: lerp(a.b, b.b, t),
            a: lerp(a.a, b.a, t),
        }
    }

    #[inline]
    fn scissor_to_ndc(scissor: ScissorRect, vp_w: u32, vp_h: u32) -> Option<(f32, f32, f32, f32)> {
        if vp_w == 0 || vp_h == 0 {
            return None;
        }
        let x0 = scissor.x.min(vp_w) as f32;
        let y0 = scissor.y.min(vp_h) as f32;
        let x1 = scissor.x.saturating_add(scissor.width).min(vp_w) as f32;
        let y1 = scissor.y.saturating_add(scissor.height).min(vp_h) as f32;
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        let w = vp_w as f32;
        let h = vp_h as f32;
        let left = (x0 / w) * 2.0 - 1.0;
        let right = (x1 / w) * 2.0 - 1.0;
        let top = 1.0 - (y0 / h) * 2.0;
        let bottom = 1.0 - (y1 / h) * 2.0;
        Some((left, right, bottom, top))
    }

    fn clip_poly_edge(input: &[RgbVtx], edge: u8, bound: f32, out: &mut Vec<RgbVtx>) {
        out.clear();
        if input.is_empty() {
            return;
        }

        let mut prev = input[input.len() - 1];
        let mut prev_in = match edge {
            0 => prev.x >= bound,
            1 => prev.x <= bound,
            2 => prev.y >= bound,
            _ => prev.y <= bound,
        };

        for &cur in input {
            let cur_in = match edge {
                0 => cur.x >= bound,
                1 => cur.x <= bound,
                2 => cur.y >= bound,
                _ => cur.y <= bound,
            };

            if cur_in != prev_in {
                let denom = match edge {
                    0 | 1 => cur.x - prev.x,
                    _ => cur.y - prev.y,
                };
                if denom.abs() > 1e-6 {
                    let t = match edge {
                        0 | 1 => (bound - prev.x) / denom,
                        _ => (bound - prev.y) / denom,
                    };
                    out.push(interp_rgb(prev, cur, t));
                }
            }

            if cur_in {
                out.push(cur);
            }

            prev = cur;
            prev_in = cur_in;
        }
    }

    fn clip_rgb_triangles_to_scissor(
        src: &[u8],
        scissor: ScissorRect,
        vp_w: u32,
        vp_h: u32,
    ) -> Vec<u8> {
        const VTX_SIZE: usize = 12;
        const TRI_SIZE: usize = VTX_SIZE * 3;

        let Some((left, right, bottom, top)) = scissor_to_ndc(scissor, vp_w, vp_h) else {
            return Vec::new();
        };

        let mut out = Vec::with_capacity(src.len());
        let usable = src.len() - (src.len() % TRI_SIZE);
        let mut poly_a: Vec<RgbVtx> = Vec::with_capacity(8);
        let mut poly_b: Vec<RgbVtx> = Vec::with_capacity(8);

        let mut off = 0usize;
        while off + TRI_SIZE <= usable {
            let Some(v0) = read_rgb_vtx(src, off) else {
                break;
            };
            let Some(v1) = read_rgb_vtx(src, off + VTX_SIZE) else {
                break;
            };
            let Some(v2) = read_rgb_vtx(src, off + (2 * VTX_SIZE)) else {
                break;
            };
            off += TRI_SIZE;

            poly_a.clear();
            poly_a.push(v0);
            poly_a.push(v1);
            poly_a.push(v2);

            clip_poly_edge(&poly_a, 0, left, &mut poly_b);
            if poly_b.len() < 3 {
                continue;
            }
            clip_poly_edge(&poly_b, 1, right, &mut poly_a);
            if poly_a.len() < 3 {
                continue;
            }
            clip_poly_edge(&poly_a, 2, bottom, &mut poly_b);
            if poly_b.len() < 3 {
                continue;
            }
            clip_poly_edge(&poly_b, 3, top, &mut poly_a);
            if poly_a.len() < 3 {
                continue;
            }

            let base = poly_a[0];
            for i in 1..(poly_a.len() - 1) {
                push_rgb_vtx(&mut out, base);
                push_rgb_vtx(&mut out, poly_a[i]);
                push_rgb_vtx(&mut out, poly_a[i + 1]);
            }
        }

        out
    }

    #[inline]
    fn gl_blend_factor_to_core(v: u32) -> BlendFactor {
        match v {
            0 => BlendFactor::Zero,                  // GL_ZERO
            1 => BlendFactor::One,                   // GL_ONE
            0x0301 => BlendFactor::OneMinusSrcColor, // GL_ONE_MINUS_SRC_COLOR
            0x0302 => BlendFactor::SrcAlpha,         // GL_SRC_ALPHA
            0x0303 => BlendFactor::OneMinusSrcAlpha, // GL_ONE_MINUS_SRC_ALPHA
            0x0306 => BlendFactor::DstColor,         // GL_DST_COLOR
            _ => BlendFactor::One,
        }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_set_blend(
        enabled: u32,
        src_rgb: u32,
        dst_rgb: u32,
        _src_alpha: u32,
        _dst_alpha: u32,
        _eq_rgb: u32,
        _eq_alpha: u32,
    ) -> i32 {
        // Minimal blend subset: support the common WebGL/Pixi cases.
        // Equation is currently assumed FUNC_ADD.
        let en = enabled != 0;
        let mut st = GFX_CABI_STATE.lock();
        st.cur_blend = BlendDesc {
            enabled: en,
            src: gl_blend_factor_to_core(src_rgb),
            dst: gl_blend_factor_to_core(dst_rgb),
        };
        0
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_set_sampler(
        wrap_s: u32,
        wrap_t: u32,
        min_filter: u32,
        mag_filter: u32,
    ) -> i32 {
        // Keep this mapping intentionally tiny: enough for Pixi/WebGL.
        // wrap: 0=ClampToEdge, 1=Repeat
        // filter: 0=Nearest, 1=Linear
        let ws = if wrap_s == 1 {
            SamplerWrap::Repeat
        } else {
            SamplerWrap::ClampToEdge
        };
        let wt = if wrap_t == 1 {
            SamplerWrap::Repeat
        } else {
            SamplerWrap::ClampToEdge
        };
        let minf = if min_filter == 0 {
            SamplerFilter::Nearest
        } else {
            SamplerFilter::Linear
        };
        let magf = if mag_filter == 0 {
            SamplerFilter::Nearest
        } else {
            SamplerFilter::Linear
        };
        let mut st = GFX_CABI_STATE.lock();
        st.cur_sampler = SamplerDesc {
            wrap_s: ws,
            wrap_t: wt,
            min_filter: minf,
            mag_filter: magf,
        };
        0
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_set_scissor(
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> i32 {
        let mut st = GFX_CABI_STATE.lock();
        st.cur_scissor = if width == 0 || height == 0 {
            None
        } else {
            Some(ScissorRect {
                x,
                y,
                width,
                height,
            })
        };
        0
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_clear_scissor() -> i32 {
        let mut st = GFX_CABI_STATE.lock();
        st.cur_scissor = None;
        0
    }

    static GFX_CABI_STATE: spin::Mutex<GfxCabiState> = spin::Mutex::new(GfxCabiState::new());

    #[inline]
    fn estimate_submit_bytes(draw_bytes: usize, command_count: usize) -> usize {
        // Rough upper-bound proxy: encoded draw payload plus command stream overhead.
        draw_bytes
            .saturating_mul(2)
            .saturating_add(command_count.saturating_mul(32))
            .saturating_add(4096)
    }

    #[inline]
    fn check_submit_budget(draw_bytes: usize, command_count: usize, site: &'static str) -> bool {
        let est = estimate_submit_bytes(draw_bytes, command_count);
        if est <= MAX_EST_SUBMIT_BYTES {
            return true;
        }
        let n = SUBMIT_BUDGET_LOGS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        if n < 16 {
            crate::globalog::log(format_args!(
                "gfx-cabi: submit budget exceeded site={} est={} draw={} cmds={} limit={}\n",
                site, est, draw_bytes, command_count, MAX_EST_SUBMIT_BYTES
            ));
        }
        false
    }

    fn ensure_gfx_resources(
        ctx: &mut dyn GfxContext,
        need_bytes: usize,
    ) -> Option<(PipelineId, BufferId, bool)> {
        let epoch = crate::gfx::backend_epoch();
        let swap = ctx.swapchain_desc();
        if swap.extent.width == 0 || swap.extent.height == 0 {
            return None;
        }
        let desired_swap = SwapchainDesc {
            format: swap.format,
            extent: swap.extent,
        };

        let mut st = GFX_CABI_STATE.lock();
        if st.epoch != epoch {
            // Backend changed; cached IDs belong to a different backend.
            st.pipeline = PipelineId::invalid();
            st.ring_idx = 0;
            st.vbuf = [BufferId::invalid(); GFX_CABI_VBUF_RING_LEN];
            st.capacity = [0; GFX_CABI_VBUF_RING_LEN];
            st.tex_pipeline = PipelineId::invalid();
            st.tex_vbuf = [BufferId::invalid(); GFX_CABI_VBUF_RING_LEN];
            st.tex_capacity = [0; GFX_CABI_VBUF_RING_LEN];
            st.tex_images = None;
            st.swapchain_configured = false;
            st.viewport_configured = false;
            st.frame_active = false;
            st.frame_seq = 0;
            st.frame_rgb_draws = 0;
            st.frame_tex_draws = 0;
            st.frame_draw_bytes = 0;
            st.frame_draws.clear();
            st.frame_rgb_blob.clear();
            st.frame_tex_blob.clear();
            st.cursor_frame_active = false;
            st.cursor_frame_seq = 0;
            st.cursor_rgb_draws = 0;
            st.cursor_tex_draws = 0;
            st.cursor_draw_bytes = 0;
            st.cursor_draws.clear();
            st.cursor_rgb_blob.clear();
            st.cursor_tex_blob.clear();
            st.base_cache_valid = false;
            st.base_cache_draws.clear();
            st.base_cache_rgb_blob.clear();
            st.base_cache_tex_blob.clear();
            st.cursor_cache_valid = false;
            st.cursor_cache_draws.clear();
            st.cursor_cache_rgb_blob.clear();
            st.cursor_cache_tex_blob.clear();
            st.epoch = epoch;
        }
        if !st.swapchain_configured || st.swapchain_desc != desired_swap {
            ctx.configure_swapchain(desired_swap).ok()?;
            st.swapchain_desc = desired_swap;
            st.swapchain_configured = true;
            st.viewport_configured = false;
        }

        if !st.pipeline.is_valid() {
            let layout = VertexLayout {
                stride: 12, // f32 x, f32 y, u8 r,g,b,a
                pos_offset: 0,
                color_offset: 8,
                color_format: ColorFormat::RgbaU8,
                texcoord_offset: 0,
                texcoord_format: TexCoordFormat::None,
            };
            let p = ctx
                .create_pipeline(PipelineDesc {
                    vertex_layout: layout,
                    vs: None,
                    fs: None,
                })
                .ok()?;
            st.pipeline = p;
        }

        let idx = st.ring_idx % GFX_CABI_VBUF_RING_LEN;
        let cur = st.vbuf[idx];
        let cur_cap = st.capacity[idx];
        if !cur.is_valid() || cur_cap < need_bytes {
            if cur.is_valid() {
                ctx.destroy_buffer(cur);
                st.vbuf[idx] = BufferId::invalid();
                st.capacity[idx] = 0;
            }
            let cap = need_bytes.max(256);
            let b = ctx
                .create_buffer(BufferDesc {
                    size: cap as u64,
                    usage: BufferUsage::Vertex,
                    memory: MemoryType::HostVisible,
                })
                .ok()?;
            st.vbuf[idx] = b;
            st.capacity[idx] = cap;
        }

        let need_set_viewport = !st.viewport_configured;
        st.viewport_configured = true;
        Some((st.pipeline, st.vbuf[idx], need_set_viewport))
    }

    fn ensure_gfx_resources_tex(
        ctx: &mut dyn GfxContext,
        need_bytes: usize,
    ) -> Option<(PipelineId, BufferId, bool)> {
        let epoch = crate::gfx::backend_epoch();
        let swap = ctx.swapchain_desc();
        if swap.extent.width == 0 || swap.extent.height == 0 {
            return None;
        }
        let desired_swap = SwapchainDesc {
            format: swap.format,
            extent: swap.extent,
        };

        let mut st = GFX_CABI_STATE.lock();
        if st.epoch != epoch {
            st.pipeline = PipelineId::invalid();
            st.ring_idx = 0;
            st.vbuf = [BufferId::invalid(); GFX_CABI_VBUF_RING_LEN];
            st.capacity = [0; GFX_CABI_VBUF_RING_LEN];
            st.tex_pipeline = PipelineId::invalid();
            st.tex_vbuf = [BufferId::invalid(); GFX_CABI_VBUF_RING_LEN];
            st.tex_capacity = [0; GFX_CABI_VBUF_RING_LEN];
            st.tex_images = None;
            st.swapchain_configured = false;
            st.viewport_configured = false;
            st.frame_active = false;
            st.frame_seq = 0;
            st.frame_rgb_draws = 0;
            st.frame_tex_draws = 0;
            st.frame_draw_bytes = 0;
            st.frame_draws.clear();
            st.frame_rgb_blob.clear();
            st.frame_tex_blob.clear();
            st.cursor_frame_active = false;
            st.cursor_frame_seq = 0;
            st.cursor_rgb_draws = 0;
            st.cursor_tex_draws = 0;
            st.cursor_draw_bytes = 0;
            st.cursor_draws.clear();
            st.cursor_rgb_blob.clear();
            st.cursor_tex_blob.clear();
            st.base_cache_valid = false;
            st.base_cache_draws.clear();
            st.base_cache_rgb_blob.clear();
            st.base_cache_tex_blob.clear();
            st.cursor_cache_valid = false;
            st.cursor_cache_draws.clear();
            st.cursor_cache_rgb_blob.clear();
            st.cursor_cache_tex_blob.clear();
            st.epoch = epoch;
        }
        if !st.swapchain_configured || st.swapchain_desc != desired_swap {
            ctx.configure_swapchain(desired_swap).ok()?;
            st.swapchain_desc = desired_swap;
            st.swapchain_configured = true;
            st.viewport_configured = false;
        }

        if !st.tex_pipeline.is_valid() {
            let layout = VertexLayout {
                stride: 20, // f32 x,y, f32 u,v, u8 r,g,b,a
                pos_offset: 0,
                color_offset: 16,
                color_format: ColorFormat::RgbaU8,
                texcoord_offset: 8,
                texcoord_format: TexCoordFormat::UvF32,
            };
            let p = ctx
                .create_pipeline(PipelineDesc {
                    vertex_layout: layout,
                    vs: None,
                    fs: None,
                })
                .ok()?;
            st.tex_pipeline = p;
        }

        let idx = st.ring_idx % GFX_CABI_VBUF_RING_LEN;
        let cur = st.tex_vbuf[idx];
        let cur_cap = st.tex_capacity[idx];
        if !cur.is_valid() || cur_cap < need_bytes {
            if cur.is_valid() {
                ctx.destroy_buffer(cur);
                st.tex_vbuf[idx] = BufferId::invalid();
                st.tex_capacity[idx] = 0;
            }
            let cap = need_bytes.max(256);
            let b = ctx
                .create_buffer(BufferDesc {
                    size: cap as u64,
                    usage: BufferUsage::Vertex,
                    memory: MemoryType::HostVisible,
                })
                .ok()?;
            st.tex_vbuf[idx] = b;
            st.tex_capacity[idx] = cap;
        }

        let need_set_viewport = !st.viewport_configured;
        st.viewport_configured = true;
        Some((st.tex_pipeline, st.tex_vbuf[idx], need_set_viewport))
    }

    /// Draw a list of RGB triangles and present.
    ///
    /// Vertex ABI (bytes): repeating struct { f32 x, f32 y, u8 r, u8 g, u8 b, u8 pad }
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_draw_rgb_triangles(
        clear_rgb: u32,
        vtx_ptr: *const u8,
        vtx_len: usize,
    ) -> i32 {
        crate::gfx::init(crate::limine::framebuffer_response());

        if vtx_ptr.is_null() {
            return if vtx_len == 0 { 0 } else { -1 };
        }
        if vtx_len == 0 {
            return 0;
        }
        const VTX_SIZE: usize = 12;
        let usable = vtx_len - (vtx_len % VTX_SIZE);
        if usable == 0 {
            return -2;
        }

        let vtx = core::slice::from_raw_parts(vtx_ptr, usable);
        let vcount = (usable / VTX_SIZE) as u32;
        if vcount == 0 {
            return 0;
        }

        let Some(ret) = crate::gfx::with_context(|ctx| {
            let (pipeline, vbuf, need_set_viewport) = match ensure_gfx_resources(ctx, usable) {
                Some(v) => v,
                None => return -3,
            };

            if ctx.write_buffer(vbuf, 0, vtx).is_err() {
                return -4;
            }

            let swap = ctx.swapchain_desc();
            let vp = Viewport {
                x: 0,
                y: 0,
                width: swap.extent.width as i32,
                height: swap.extent.height as i32,
            };

            let submit_res = if need_set_viewport {
                let cmds = [
                    Command::SetViewport(vp),
                    Command::ClearColor { rgb: clear_rgb },
                    Command::BindPipeline(pipeline),
                    Command::BindVertexBuffer {
                        buffer: vbuf,
                        offset: 0,
                    },
                    Command::Draw {
                        vertex_count: vcount,
                        first_vertex: 0,
                    },
                    Command::Present,
                ];
                if !check_submit_budget(usable, cmds.len(), "draw_rgb_triangles_vp") {
                    return -5;
                }
                ctx.submit(CommandBuffer { commands: &cmds })
            } else {
                let cmds = [
                    Command::ClearColor { rgb: clear_rgb },
                    Command::BindPipeline(pipeline),
                    Command::BindVertexBuffer {
                        buffer: vbuf,
                        offset: 0,
                    },
                    Command::Draw {
                        vertex_count: vcount,
                        first_vertex: 0,
                    },
                    Command::Present,
                ];
                if !check_submit_budget(usable, cmds.len(), "draw_rgb_triangles") {
                    return -5;
                }
                ctx.submit(CommandBuffer { commands: &cmds })
            };

            let ok = submit_res.is_ok();
            if ok {
                let mut st = GFX_CABI_STATE.lock();
                st.ring_idx = (st.ring_idx + 1) % GFX_CABI_VBUF_RING_LEN;
                0
            } else {
                -5
            }
        }) else {
            return -6;
        };
        ret
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_upload_texture_rgba(
        tex_id: u32,
        width: u32,
        height: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32 {
        crate::gfx::init(crate::limine::framebuffer_response());

        if tex_id == 0 || width == 0 || height == 0 {
            return -1;
        }
        if data_ptr.is_null() {
            return -2;
        }
        let expected = (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(4);
        if data_len < expected {
            return -3;
        }
        let data = core::slice::from_raw_parts(data_ptr, expected);

        let Some(ret) = crate::gfx::with_context(|ctx| {
            let epoch = crate::gfx::backend_epoch();
            let mut st = GFX_CABI_STATE.lock();
            if st.epoch != epoch {
                // Backend changed (or first use). Drop cached IDs so future draws don't
                // reference resources from a different backend, and so early texture uploads
                // won't be wiped by the first ensure_gfx_resources* call.
                st.pipeline = PipelineId::invalid();
                st.ring_idx = 0;
                st.vbuf = [BufferId::invalid(); GFX_CABI_VBUF_RING_LEN];
                st.capacity = [0; GFX_CABI_VBUF_RING_LEN];
                st.tex_pipeline = PipelineId::invalid();
                st.tex_vbuf = [BufferId::invalid(); GFX_CABI_VBUF_RING_LEN];
                st.tex_capacity = [0; GFX_CABI_VBUF_RING_LEN];
                st.tex_images = None;
                st.swapchain_configured = false;
                st.viewport_configured = false;
                st.frame_active = false;
                st.frame_seq = 0;
                st.frame_rgb_draws = 0;
                st.frame_tex_draws = 0;
                st.frame_draw_bytes = 0;
                st.frame_draws.clear();
                st.frame_rgb_blob.clear();
                st.frame_tex_blob.clear();
                st.cursor_frame_active = false;
                st.cursor_frame_seq = 0;
                st.cursor_rgb_draws = 0;
                st.cursor_tex_draws = 0;
                st.cursor_draw_bytes = 0;
                st.cursor_draws.clear();
                st.cursor_rgb_blob.clear();
                st.cursor_tex_blob.clear();
                st.base_cache_valid = false;
                st.base_cache_draws.clear();
                st.base_cache_rgb_blob.clear();
                st.base_cache_tex_blob.clear();
                st.cursor_cache_valid = false;
                st.cursor_cache_draws.clear();
                st.cursor_cache_rgb_blob.clear();
                st.cursor_cache_tex_blob.clear();
                st.epoch = epoch;
            }
            let images = st.tex_images.get_or_insert_with(Vec::new);
            let idx = tex_id.saturating_sub(1) as usize;
            if idx >= images.len() {
                images.resize_with(idx + 1, || None);
            }
            let mut image_id = ImageId::invalid();
            let mut recreate = true;
            if let Some(Some(entry)) = images.get(idx)
                && entry.width == width
                && entry.height == height
            {
                image_id = entry.image;
                recreate = false;
            }
            if recreate {
                if image_id.is_valid() {
                    ctx.destroy_image(image_id);
                }
                let desc = ImageDesc {
                    width,
                    height,
                    format: ImageFormat::Rgba8888,
                };
                let Ok(img) = ctx.create_image(desc) else {
                    return -4;
                };
                image_id = img;
                images[idx] = Some(TexImage {
                    image: image_id,
                    width,
                    height,
                });
            }
            if ctx.write_image(image_id, data).is_err() {
                return -5;
            }
            0
        }) else {
            return -6;
        };
        ret
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_begin_frame(clear_rgb: u32) -> i32 {
        crate::gfx::init(crate::limine::framebuffer_response());

        let mut st = GFX_CABI_STATE.lock();
        st.frame_seq = st.frame_seq.wrapping_add(1);
        st.frame_active = true;
        st.frame_clear_rgb = clear_rgb;
        st.frame_rgb_draws = 0;
        st.frame_tex_draws = 0;
        st.frame_draw_bytes = 0;
        st.frame_draws.clear();
        st.frame_rgb_blob.clear();
        st.frame_tex_blob.clear();
        let seq = st.frame_seq;
        if seq <= 10 || seq.is_multiple_of(20) {
            crate::globalog::log(format_args!(
                "gfx-cabi: begin seq={} clear=0x{:06X}\n",
                seq,
                clear_rgb & 0x00FF_FFFF
            ));
        }
        0
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_draw_rgb_triangles_no_present(
        vtx_ptr: *const u8,
        vtx_len: usize,
    ) -> i32 {
        if vtx_ptr.is_null() {
            return if vtx_len == 0 { 0 } else { -1 };
        }
        if vtx_len == 0 {
            return 0;
        }
        const VTX_SIZE: usize = 12;
        let usable = vtx_len - (vtx_len % VTX_SIZE);
        if usable == 0 {
            return -2;
        }
        let vcount = (usable / VTX_SIZE) as u32;
        if vcount == 0 {
            return 0;
        }
        let bytes = core::slice::from_raw_parts(vtx_ptr, usable);
        let mut st = GFX_CABI_STATE.lock();
        if !st.frame_active {
            crate::globalog::log(format_args!(
                "gfx-cabi: draw-rgb without active frame bytes={}\n",
                usable
            ));
            return -3;
        }

        let clipped_owned;
        let clipped = if let Some(scissor) = st.cur_scissor {
            let vp_w = st.swapchain_desc.extent.width;
            let vp_h = st.swapchain_desc.extent.height;
            clipped_owned = clip_rgb_triangles_to_scissor(bytes, scissor, vp_w, vp_h);
            clipped_owned.as_slice()
        } else {
            bytes
        };
        if clipped.is_empty() {
            return 0;
        }

        st.frame_rgb_draws = st.frame_rgb_draws.saturating_add(1);
        st.frame_draw_bytes = st.frame_draw_bytes.saturating_add(clipped.len());
        let blend = st.cur_blend;
        let mut off = 0usize;
        while off < clipped.len() {
            let rem = clipped.len() - off;
            let chunk = core::cmp::min(MAX_CMDSTREAM_DRAW_BYTES, rem);
            let chunk = chunk - (chunk % VTX_SIZE);
            if chunk == 0 {
                break;
            }
            let blob_offset = st.frame_rgb_blob.len();
            st.frame_rgb_blob
                .extend_from_slice(&clipped[off..off + chunk]);
            st.frame_draws.push(PendingDraw::Rgb {
                blob_offset,
                blob_len: chunk,
                blend,
            });
            off += chunk;
        }
        0
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_draw_tex_triangles_no_present(
        tex_id: u32,
        vtx_ptr: *const u8,
        vtx_len: usize,
    ) -> i32 {
        if tex_id == 0 {
            return -1;
        }
        if vtx_ptr.is_null() {
            return if vtx_len == 0 { 0 } else { -2 };
        }
        if vtx_len == 0 {
            return 0;
        }
        const VTX_SIZE: usize = 20;
        let usable = vtx_len - (vtx_len % VTX_SIZE);
        if usable == 0 {
            return -3;
        }
        let vcount = (usable / VTX_SIZE) as u32;
        if vcount == 0 {
            return 0;
        }
        let bytes = core::slice::from_raw_parts(vtx_ptr, usable);
        let mut st = GFX_CABI_STATE.lock();
        if !st.frame_active {
            crate::globalog::log(format_args!(
                "gfx-cabi: draw-tex without active frame bytes={}\n",
                usable
            ));
            return -4;
        }
        st.frame_tex_draws = st.frame_tex_draws.saturating_add(1);
        st.frame_draw_bytes = st.frame_draw_bytes.saturating_add(usable);
        let idx = tex_id.saturating_sub(1) as usize;
        let image = st
            .tex_images
            .as_ref()
            .and_then(|images| images.get(idx))
            .and_then(|e| e.as_ref())
            .map(|e| e.image)
            .unwrap_or(ImageId::invalid());
        let sampler = st.cur_sampler;
        let blend = st.cur_blend;
        let mut off = 0usize;
        while off < usable {
            let rem = usable - off;
            let chunk = core::cmp::min(MAX_CMDSTREAM_DRAW_BYTES, rem);
            let chunk = chunk - (chunk % VTX_SIZE);
            if chunk == 0 {
                break;
            }
            let blob_offset = st.frame_tex_blob.len();
            st.frame_tex_blob
                .extend_from_slice(&bytes[off..off + chunk]);
            st.frame_draws.push(PendingDraw::Tex {
                tex_id,
                image,
                sampler,
                blob_offset,
                blob_len: chunk,
                blend,
            });
            off += chunk;
        }
        0
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_end_frame() -> i32 {
        crate::gfx::init(crate::limine::framebuffer_response());

        let (seq, rgb_draws, tex_draws, draw_bytes, was_active, clear_rgb, draws, rgb_src, tex_src) = {
            let mut st = GFX_CABI_STATE.lock();
            let out = (
                st.frame_seq,
                st.frame_rgb_draws,
                st.frame_tex_draws,
                st.frame_draw_bytes,
                st.frame_active,
                st.frame_clear_rgb,
                core::mem::take(&mut st.frame_draws),
                core::mem::take(&mut st.frame_rgb_blob),
                core::mem::take(&mut st.frame_tex_blob),
            );
            st.frame_active = false;
            out
        };
        if !was_active {
            crate::globalog::log(format_args!("gfx-cabi: end without active frame\n"));
            return -3;
        }

        let Some(ret) = crate::gfx::with_context(|ctx| {
            let (_p, _v, need_set_viewport) = match ensure_gfx_resources(ctx, 0) {
                Some(v) => v,
                None => return -1,
            };
            let swap = ctx.swapchain_desc();
            // Compose cursor into app-driven presents to avoid one-frame cursor blink
            // between end_frame and the async cursor overlay tick.
            let mut submit_draws = draws.clone();
            let mut submit_rgb_src = rgb_src.clone();
            let mut submit_tex_src = tex_src.clone();
            append_kernel_cursor_overlay_draws(
                &mut submit_draws,
                &mut submit_rgb_src,
                swap.extent.width,
                swap.extent.height,
            );
            let vp = Viewport {
                x: 0,
                y: 0,
                width: swap.extent.width as i32,
                height: swap.extent.height as i32,
            };

            const MAX_PASS_VERTEX_BYTES: usize = 96 * 1024;

            enum Plan {
                Rgb {
                    offset: u64,
                    vcount: u32,
                    blend: BlendDesc,
                },
                Tex {
                    tex_id: u32,
                    image: ImageId,
                    sampler: SamplerDesc,
                    offset: u64,
                    vcount: u32,
                    blend: BlendDesc,
                },
            }

            let mut draw_idx = 0usize;
            let mut first_pass = true;

            while draw_idx < submit_draws.len() {
                let start = draw_idx;
                let mut pass_bytes = 0usize;
                let mut pass_kind: u8 = 0; // 1=rgb, 2=tex
                while draw_idx < submit_draws.len() {
                    let (kind, add) = match &submit_draws[draw_idx] {
                        PendingDraw::Rgb { blob_len, .. } => (1u8, blob_len - (blob_len % 12)),
                        PendingDraw::Tex { blob_len, .. } => (2u8, blob_len - (blob_len % 20)),
                    };
                    if add == 0 {
                        draw_idx += 1;
                        continue;
                    }
                    if pass_kind == 0 {
                        pass_kind = kind;
                    } else if kind != pass_kind {
                        // Keep pass submissions homogeneous by vertex format/pipeline type.
                        break;
                    }
                    if pass_bytes != 0 && pass_bytes.saturating_add(add) > MAX_PASS_VERTEX_BYTES {
                        break;
                    }
                    pass_bytes = pass_bytes.saturating_add(add);
                    draw_idx += 1;
                }

                let mut plans: Vec<Plan> = Vec::new();
                let mut rgb_blob: Vec<u8> = Vec::new();
                let mut tex_blob: Vec<u8> = Vec::new();

                for draw in submit_draws[start..draw_idx].iter() {
                    match draw {
                        PendingDraw::Rgb {
                            blob_offset,
                            blob_len,
                            blend,
                        } => {
                            const VTX_SIZE: usize = 12;
                            let usable = blob_len - (blob_len % VTX_SIZE);
                            if usable == 0 {
                                continue;
                            }
                            let start = *blob_offset;
                            let end = start.saturating_add(usable);
                            if end > submit_rgb_src.len() {
                                continue;
                            }
                            let vcount = (usable / VTX_SIZE) as u32;
                            let off = rgb_blob.len() as u64;
                            rgb_blob.extend_from_slice(&submit_rgb_src[start..end]);
                            plans.push(Plan::Rgb {
                                offset: off,
                                vcount,
                                blend: *blend,
                            });
                        }
                        PendingDraw::Tex {
                            tex_id,
                            image,
                            sampler,
                            blob_offset,
                            blob_len,
                            blend,
                        } => {
                            const VTX_SIZE: usize = 20;
                            let usable = blob_len - (blob_len % VTX_SIZE);
                            if usable == 0 {
                                continue;
                            }
                            let start = *blob_offset;
                            let end = start.saturating_add(usable);
                            if end > submit_tex_src.len() {
                                continue;
                            }
                            let vcount = (usable / VTX_SIZE) as u32;
                            let off = tex_blob.len() as u64;
                            tex_blob.extend_from_slice(&submit_tex_src[start..end]);
                            plans.push(Plan::Tex {
                                tex_id: *tex_id,
                                image: *image,
                                sampler: *sampler,
                                offset: off,
                                vcount,
                                blend: *blend,
                            });
                        }
                    }
                }

                if plans.is_empty() {
                    continue;
                }

                let mut rgb_res: Option<(PipelineId, BufferId)> = None;
                if !rgb_blob.is_empty() {
                    let (pipeline, vbuf, _) = match ensure_gfx_resources(ctx, rgb_blob.len()) {
                        Some(v) => v,
                        None => return -4,
                    };
                    if ctx.write_buffer(vbuf, 0, rgb_blob.as_slice()).is_err() {
                        return -5;
                    }
                    rgb_res = Some((pipeline, vbuf));
                }

                let mut tex_res: Option<(PipelineId, BufferId)> = None;
                if !tex_blob.is_empty() {
                    let (pipeline, vbuf, _) = match ensure_gfx_resources_tex(ctx, tex_blob.len()) {
                        Some(v) => v,
                        None => return -6,
                    };
                    if ctx.write_buffer(vbuf, 0, tex_blob.as_slice()).is_err() {
                        return -7;
                    }
                    tex_res = Some((pipeline, vbuf));
                }

                let is_last_pass = draw_idx >= submit_draws.len();
                let mut cmds: Vec<Command> = Vec::new();
                if first_pass && need_set_viewport {
                    cmds.push(Command::SetViewport(vp));
                }
                if first_pass {
                    cmds.push(Command::ClearColor { rgb: clear_rgb });
                }

                let mut last_blend: Option<BlendDesc> = None;

                for plan in plans.iter() {
                    match *plan {
                        Plan::Rgb {
                            offset,
                            vcount,
                            blend,
                        } => {
                            if last_blend != Some(blend) {
                                cmds.push(Command::SetBlend(blend));
                                last_blend = Some(blend);
                            }
                            let Some((pipeline, vbuf)) = rgb_res else {
                                return -8;
                            };
                            cmds.push(Command::BindPipeline(pipeline));
                            cmds.push(Command::BindVertexBuffer {
                                buffer: vbuf,
                                offset,
                            });
                            cmds.push(Command::Draw {
                                vertex_count: vcount,
                                first_vertex: 0,
                            });
                        }
                        Plan::Tex {
                            tex_id,
                            image,
                            sampler,
                            offset,
                            vcount,
                            blend,
                        } => {
                            if last_blend != Some(blend) {
                                cmds.push(Command::SetBlend(blend));
                                last_blend = Some(blend);
                            }
                            let Some((pipeline, vbuf)) = tex_res else {
                                return -9;
                            };
                            let (image_id, log_missing_tex) = if image.is_valid() {
                                (image, false)
                            } else {
                                let mut st = GFX_CABI_STATE.lock();
                                let idx = tex_id.saturating_sub(1) as usize;
                                let should_log =
                                    st.missing_tex_logs < 16 && st.last_missing_tex_id != tex_id;
                                if should_log {
                                    st.last_missing_tex_id = tex_id;
                                    st.missing_tex_logs = st.missing_tex_logs.saturating_add(1);
                                }
                                let desc = ImageDesc {
                                    width: 1,
                                    height: 1,
                                    format: ImageFormat::Rgba8888,
                                };
                                let Ok(img) = ctx.create_image(desc) else {
                                    return -10;
                                };
                                let white = [255u8, 255u8, 255u8, 255u8];
                                let _ = ctx.write_image(img, &white);
                                let images = st.tex_images.get_or_insert_with(Vec::new);
                                if idx >= images.len() {
                                    images.resize_with(idx + 1, || None);
                                }
                                images[idx] = Some(TexImage {
                                    image: img,
                                    width: 1,
                                    height: 1,
                                });
                                (img, should_log)
                            };
                            if log_missing_tex {
                                crate::globalog::log(format_args!(
                                    "gfx-cabi: missing texture tex_id={} (using 1x1 fallback)\n",
                                    tex_id
                                ));
                            }
                            cmds.push(Command::BindPipeline(pipeline));
                            cmds.push(Command::SetSampler(sampler));
                            cmds.push(Command::BindImage(image_id));
                            cmds.push(Command::BindVertexBuffer {
                                buffer: vbuf,
                                offset,
                            });
                            cmds.push(Command::Draw {
                                vertex_count: vcount,
                                first_vertex: 0,
                            });
                        }
                    }
                }

                if is_last_pass {
                    cmds.push(Command::Present);
                }

                if !check_submit_budget(
                    rgb_blob.len().saturating_add(tex_blob.len()),
                    cmds.len(),
                    "end_frame_pass",
                ) {
                    return -11;
                }
                let submit_res = ctx.submit(CommandBuffer {
                    commands: cmds.as_slice(),
                });
                if submit_res.is_ok() {
                    let mut st = GFX_CABI_STATE.lock();
                    st.ring_idx = (st.ring_idx + 1) % GFX_CABI_VBUF_RING_LEN;
                } else {
                    if let Err(e) = submit_res {
                        crate::globalog::log(format_args!("gfx-cabi: submit failed: {:?}\n", e));
                    }
                    return -11;
                }
                first_pass = false;
            }

            if first_pass {
                // No valid draw payloads in this frame; keep clear/present behavior.
                let mut cmds: Vec<Command> = Vec::new();
                if need_set_viewport {
                    cmds.push(Command::SetViewport(vp));
                }
                cmds.push(Command::ClearColor { rgb: clear_rgb });
                cmds.push(Command::Present);
                if !check_submit_budget(0, cmds.len(), "end_frame_clear_only") {
                    return -11;
                }
                let submit_res = ctx.submit(CommandBuffer {
                    commands: cmds.as_slice(),
                });
                if submit_res.is_ok() {
                    let mut st = GFX_CABI_STATE.lock();
                    st.ring_idx = (st.ring_idx + 1) % GFX_CABI_VBUF_RING_LEN;
                    return 0;
                }
                if let Err(e) = submit_res {
                    crate::globalog::log(format_args!("gfx-cabi: submit failed: {:?}\n", e));
                }
                return -11;
            }
            0
        }) else {
            return -12;
        };

        if ret == 0 {
            let mut st = GFX_CABI_STATE.lock();
            st.base_cache_valid = true;
            st.base_cache_updated_at_ticks = embassy_time_driver::now();
            st.base_cache_clear_rgb = clear_rgb;
            st.base_cache_draws = draws.clone();
            st.base_cache_rgb_blob = rgb_src.clone();
            st.base_cache_tex_blob = tex_src.clone();
        }

        if seq <= 10 || (seq % 20) == 0 {
            crate::globalog::log(format_args!(
                "gfx-cabi: end seq={} rgb={} tex={} bytes={} rc={}\n",
                seq, rgb_draws, tex_draws, draw_bytes, ret
            ));
        }
        ret
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_cursor_begin_frame() -> i32 {
        crate::gfx::init(crate::limine::framebuffer_response());

        let mut st = GFX_CABI_STATE.lock();
        st.cursor_frame_seq = st.cursor_frame_seq.wrapping_add(1);
        st.cursor_frame_active = true;
        st.cursor_rgb_draws = 0;
        st.cursor_tex_draws = 0;
        st.cursor_draw_bytes = 0;
        st.cursor_draws.clear();
        st.cursor_rgb_blob.clear();
        st.cursor_tex_blob.clear();
        0
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_cursor_draw_rgb_triangles_no_present(
        vtx_ptr: *const u8,
        vtx_len: usize,
    ) -> i32 {
        if vtx_ptr.is_null() {
            return if vtx_len == 0 { 0 } else { -1 };
        }
        if vtx_len == 0 {
            return 0;
        }
        const VTX_SIZE: usize = 12;
        let usable = vtx_len - (vtx_len % VTX_SIZE);
        if usable == 0 {
            return -2;
        }
        let vcount = (usable / VTX_SIZE) as u32;
        if vcount == 0 {
            return 0;
        }
        let bytes = core::slice::from_raw_parts(vtx_ptr, usable);
        let mut st = GFX_CABI_STATE.lock();
        if !st.cursor_frame_active {
            return -3;
        }
        st.cursor_rgb_draws = st.cursor_rgb_draws.saturating_add(1);
        st.cursor_draw_bytes = st.cursor_draw_bytes.saturating_add(usable);
        let blend = st.cur_blend;
        let mut off = 0usize;
        while off < usable {
            let rem = usable - off;
            let chunk = core::cmp::min(MAX_CMDSTREAM_DRAW_BYTES, rem);
            let chunk = chunk - (chunk % VTX_SIZE);
            if chunk == 0 {
                break;
            }
            let blob_offset = st.cursor_rgb_blob.len();
            st.cursor_rgb_blob
                .extend_from_slice(&bytes[off..off + chunk]);
            st.cursor_draws.push(PendingDraw::Rgb {
                blob_offset,
                blob_len: chunk,
                blend,
            });
            off += chunk;
        }
        0
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_cursor_draw_tex_triangles_no_present(
        tex_id: u32,
        vtx_ptr: *const u8,
        vtx_len: usize,
    ) -> i32 {
        if tex_id == 0 {
            return -1;
        }
        if vtx_ptr.is_null() {
            return if vtx_len == 0 { 0 } else { -2 };
        }
        if vtx_len == 0 {
            return 0;
        }
        const VTX_SIZE: usize = 20;
        let usable = vtx_len - (vtx_len % VTX_SIZE);
        if usable == 0 {
            return -3;
        }
        let vcount = (usable / VTX_SIZE) as u32;
        if vcount == 0 {
            return 0;
        }
        let bytes = core::slice::from_raw_parts(vtx_ptr, usable);
        let mut st = GFX_CABI_STATE.lock();
        if !st.cursor_frame_active {
            return -4;
        }
        st.cursor_tex_draws = st.cursor_tex_draws.saturating_add(1);
        st.cursor_draw_bytes = st.cursor_draw_bytes.saturating_add(usable);
        let idx = tex_id.saturating_sub(1) as usize;
        let image = st
            .tex_images
            .as_ref()
            .and_then(|images| images.get(idx))
            .and_then(|e| e.as_ref())
            .map(|e| e.image)
            .unwrap_or(ImageId::invalid());
        let sampler = st.cur_sampler;
        let blend = st.cur_blend;
        let mut off = 0usize;
        while off < usable {
            let rem = usable - off;
            let chunk = core::cmp::min(MAX_CMDSTREAM_DRAW_BYTES, rem);
            let chunk = chunk - (chunk % VTX_SIZE);
            if chunk == 0 {
                break;
            }
            let blob_offset = st.cursor_tex_blob.len();
            st.cursor_tex_blob
                .extend_from_slice(&bytes[off..off + chunk]);
            st.cursor_draws.push(PendingDraw::Tex {
                tex_id,
                image,
                sampler,
                blob_offset,
                blob_len: chunk,
                blend,
            });
            off += chunk;
        }
        0
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_cursor_end_frame() -> i32 {
        crate::gfx::init(crate::limine::framebuffer_response());

        let (
            _seq,
            was_active,
            cursor_draws,
            cursor_rgb_src,
            cursor_tex_src,
            base_cache_valid,
            base_cache_clear_rgb,
            base_cache_draws,
            base_cache_rgb_blob,
            base_cache_tex_blob,
        ) = {
            let mut st = GFX_CABI_STATE.lock();
            let out = (
                st.cursor_frame_seq,
                st.cursor_frame_active,
                core::mem::take(&mut st.cursor_draws),
                core::mem::take(&mut st.cursor_rgb_blob),
                core::mem::take(&mut st.cursor_tex_blob),
                st.base_cache_valid,
                st.base_cache_clear_rgb,
                st.base_cache_draws.clone(),
                st.base_cache_rgb_blob.clone(),
                st.base_cache_tex_blob.clone(),
            );
            st.cursor_frame_active = false;
            out
        };
        if !was_active {
            return -3;
        }
        if !base_cache_valid {
            return -13;
        }

        let cursor_cache_draws = cursor_draws.clone();
        let cursor_cache_rgb_blob = cursor_rgb_src.clone();
        let cursor_cache_tex_blob = cursor_tex_src.clone();

        // Rebuild a healthy frame from cached app content first, then append cursor overlay.
        let mut draws = base_cache_draws;
        let mut rgb_src = base_cache_rgb_blob;
        let mut tex_src = base_cache_tex_blob;
        let rgb_off = rgb_src.len();
        let tex_off = tex_src.len();
        rgb_src.extend_from_slice(cursor_rgb_src.as_slice());
        tex_src.extend_from_slice(cursor_tex_src.as_slice());
        for d in cursor_draws {
            match d {
                PendingDraw::Rgb {
                    blob_offset,
                    blob_len,
                    blend,
                } => draws.push(PendingDraw::Rgb {
                    blob_offset: blob_offset.saturating_add(rgb_off),
                    blob_len,
                    blend,
                }),
                PendingDraw::Tex {
                    tex_id,
                    image,
                    sampler,
                    blob_offset,
                    blob_len,
                    blend,
                } => draws.push(PendingDraw::Tex {
                    tex_id,
                    image,
                    sampler,
                    blob_offset: blob_offset.saturating_add(tex_off),
                    blob_len,
                    blend,
                }),
            }
        }

        let Some(ret) = crate::gfx::with_context(|ctx| {
            let (_p, _v, need_set_viewport) = match ensure_gfx_resources(ctx, 0) {
                Some(v) => v,
                None => return -1,
            };
            let swap = ctx.swapchain_desc();
            let vp = Viewport {
                x: 0,
                y: 0,
                width: swap.extent.width as i32,
                height: swap.extent.height as i32,
            };

            const MAX_PASS_VERTEX_BYTES: usize = 96 * 1024;

            enum Plan {
                Rgb {
                    offset: u64,
                    vcount: u32,
                    blend: BlendDesc,
                },
                Tex {
                    tex_id: u32,
                    image: ImageId,
                    sampler: SamplerDesc,
                    offset: u64,
                    vcount: u32,
                    blend: BlendDesc,
                },
            }

            let mut draw_idx = 0usize;
            let mut first_pass = true;

            while draw_idx < draws.len() {
                let start = draw_idx;
                let mut pass_bytes = 0usize;
                let mut pass_kind: u8 = 0;
                while draw_idx < draws.len() {
                    let (kind, add) = match &draws[draw_idx] {
                        PendingDraw::Rgb { blob_len, .. } => (1u8, blob_len - (blob_len % 12)),
                        PendingDraw::Tex { blob_len, .. } => (2u8, blob_len - (blob_len % 20)),
                    };
                    if add == 0 {
                        draw_idx += 1;
                        continue;
                    }
                    if pass_kind == 0 {
                        pass_kind = kind;
                    } else if kind != pass_kind {
                        break;
                    }
                    if pass_bytes != 0 && pass_bytes.saturating_add(add) > MAX_PASS_VERTEX_BYTES {
                        break;
                    }
                    pass_bytes = pass_bytes.saturating_add(add);
                    draw_idx += 1;
                }

                let mut plans: Vec<Plan> = Vec::new();
                let mut rgb_blob: Vec<u8> = Vec::new();
                let mut tex_blob: Vec<u8> = Vec::new();

                for draw in draws[start..draw_idx].iter() {
                    match draw {
                        PendingDraw::Rgb {
                            blob_offset,
                            blob_len,
                            blend,
                        } => {
                            const VTX_SIZE: usize = 12;
                            let usable = blob_len - (blob_len % VTX_SIZE);
                            if usable == 0 {
                                continue;
                            }
                            let start = *blob_offset;
                            let end = start.saturating_add(usable);
                            if end > rgb_src.len() {
                                continue;
                            }
                            let vcount = (usable / VTX_SIZE) as u32;
                            let off = rgb_blob.len() as u64;
                            rgb_blob.extend_from_slice(&rgb_src[start..end]);
                            plans.push(Plan::Rgb {
                                offset: off,
                                vcount,
                                blend: *blend,
                            });
                        }
                        PendingDraw::Tex {
                            tex_id,
                            image,
                            sampler,
                            blob_offset,
                            blob_len,
                            blend,
                        } => {
                            const VTX_SIZE: usize = 20;
                            let usable = blob_len - (blob_len % VTX_SIZE);
                            if usable == 0 {
                                continue;
                            }
                            let start = *blob_offset;
                            let end = start.saturating_add(usable);
                            if end > tex_src.len() {
                                continue;
                            }
                            let vcount = (usable / VTX_SIZE) as u32;
                            let off = tex_blob.len() as u64;
                            tex_blob.extend_from_slice(&tex_src[start..end]);
                            plans.push(Plan::Tex {
                                tex_id: *tex_id,
                                image: *image,
                                sampler: *sampler,
                                offset: off,
                                vcount,
                                blend: *blend,
                            });
                        }
                    }
                }

                if plans.is_empty() {
                    continue;
                }

                let mut rgb_res: Option<(PipelineId, BufferId)> = None;
                if !rgb_blob.is_empty() {
                    let (pipeline, vbuf, _) = match ensure_gfx_resources(ctx, rgb_blob.len()) {
                        Some(v) => v,
                        None => return -4,
                    };
                    if ctx.write_buffer(vbuf, 0, rgb_blob.as_slice()).is_err() {
                        return -5;
                    }
                    rgb_res = Some((pipeline, vbuf));
                }

                let mut tex_res: Option<(PipelineId, BufferId)> = None;
                if !tex_blob.is_empty() {
                    let (pipeline, vbuf, _) = match ensure_gfx_resources_tex(ctx, tex_blob.len()) {
                        Some(v) => v,
                        None => return -6,
                    };
                    if ctx.write_buffer(vbuf, 0, tex_blob.as_slice()).is_err() {
                        return -7;
                    }
                    tex_res = Some((pipeline, vbuf));
                }

                let is_last_pass = draw_idx >= draws.len();
                let mut cmds: Vec<Command> = Vec::new();
                if first_pass && need_set_viewport {
                    cmds.push(Command::SetViewport(vp));
                }
                if first_pass {
                    cmds.push(Command::ClearColor {
                        rgb: base_cache_clear_rgb,
                    });
                }

                let mut last_blend: Option<BlendDesc> = None;

                for plan in plans.iter() {
                    match *plan {
                        Plan::Rgb {
                            offset,
                            vcount,
                            blend,
                        } => {
                            if last_blend != Some(blend) {
                                cmds.push(Command::SetBlend(blend));
                                last_blend = Some(blend);
                            }
                            let Some((pipeline, vbuf)) = rgb_res else {
                                return -8;
                            };
                            cmds.push(Command::BindPipeline(pipeline));
                            cmds.push(Command::BindVertexBuffer {
                                buffer: vbuf,
                                offset,
                            });
                            cmds.push(Command::Draw {
                                vertex_count: vcount,
                                first_vertex: 0,
                            });
                        }
                        Plan::Tex {
                            tex_id,
                            image,
                            sampler,
                            offset,
                            vcount,
                            blend,
                        } => {
                            if last_blend != Some(blend) {
                                cmds.push(Command::SetBlend(blend));
                                last_blend = Some(blend);
                            }
                            let Some((pipeline, vbuf)) = tex_res else {
                                return -9;
                            };
                            let (image_id, _log_missing_tex) = if image.is_valid() {
                                (image, false)
                            } else {
                                let mut st = GFX_CABI_STATE.lock();
                                let idx = tex_id.saturating_sub(1) as usize;
                                let desc = ImageDesc {
                                    width: 1,
                                    height: 1,
                                    format: ImageFormat::Rgba8888,
                                };
                                let Ok(img) = ctx.create_image(desc) else {
                                    return -10;
                                };
                                let white = [255u8, 255u8, 255u8, 255u8];
                                let _ = ctx.write_image(img, &white);
                                let images = st.tex_images.get_or_insert_with(Vec::new);
                                if idx >= images.len() {
                                    images.resize_with(idx + 1, || None);
                                }
                                images[idx] = Some(TexImage {
                                    image: img,
                                    width: 1,
                                    height: 1,
                                });
                                (img, false)
                            };
                            cmds.push(Command::BindPipeline(pipeline));
                            cmds.push(Command::SetSampler(sampler));
                            cmds.push(Command::BindImage(image_id));
                            cmds.push(Command::BindVertexBuffer {
                                buffer: vbuf,
                                offset,
                            });
                            cmds.push(Command::Draw {
                                vertex_count: vcount,
                                first_vertex: 0,
                            });
                        }
                    }
                }

                if is_last_pass {
                    cmds.push(Command::Present);
                }

                if !check_submit_budget(
                    rgb_blob.len().saturating_add(tex_blob.len()),
                    cmds.len(),
                    "cursor_end_frame_pass",
                ) {
                    return -11;
                }
                let submit_res = ctx.submit(CommandBuffer {
                    commands: cmds.as_slice(),
                });
                if submit_res.is_ok() {
                    let mut st = GFX_CABI_STATE.lock();
                    st.ring_idx = (st.ring_idx + 1) % GFX_CABI_VBUF_RING_LEN;
                } else {
                    return -11;
                }
                first_pass = false;
            }

            if first_pass {
                let mut cmds: Vec<Command> = Vec::new();
                if need_set_viewport {
                    cmds.push(Command::SetViewport(vp));
                }
                cmds.push(Command::ClearColor {
                    rgb: base_cache_clear_rgb,
                });
                cmds.push(Command::Present);
                if !check_submit_budget(
                    rgb_src.len().saturating_add(tex_src.len()),
                    cmds.len(),
                    "cursor_end_frame_present_only",
                ) {
                    return -11;
                }
                let submit_res = ctx.submit(CommandBuffer {
                    commands: cmds.as_slice(),
                });
                if submit_res.is_ok() {
                    let mut st = GFX_CABI_STATE.lock();
                    st.ring_idx = (st.ring_idx + 1) % GFX_CABI_VBUF_RING_LEN;
                    return 0;
                }
                return -11;
            }

            0
        }) else {
            return -12;
        };

        if ret == 0 {
            let mut st = GFX_CABI_STATE.lock();
            st.cursor_cache_valid = true;
            st.cursor_cache_draws = cursor_cache_draws;
            st.cursor_cache_rgb_blob = cursor_cache_rgb_blob;
            st.cursor_cache_tex_blob = cursor_cache_tex_blob;
        }

        ret
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_gfx_present_rgba(
        tex_id: u32,
        width: u32,
        height: u32,
        data_ptr: *const u8,
        data_len: usize,
        clear_rgb: u32,
    ) -> i32 {
        #[repr(C)]
        #[derive(Clone, Copy)]
        struct TexVertex {
            x: f32,
            y: f32,
            u: f32,
            v: f32,
            r: u8,
            g: u8,
            b: u8,
            a: u8,
        }

        let rc = trueos_cabi_gfx_upload_texture_rgba(tex_id, width, height, data_ptr, data_len);
        if rc != 0 {
            return rc;
        }

        let rc = trueos_cabi_gfx_set_sampler(0, 0, 0, 0);
        if rc != 0 {
            return rc;
        }
        let rc = trueos_cabi_gfx_set_blend(0, 1, 0, 1, 0, 0, 0);
        if rc != 0 {
            return rc;
        }

        let rc = trueos_cabi_gfx_begin_frame(clear_rgb);
        if rc != 0 {
            return rc;
        }

        let verts = [
            TexVertex {
                x: -1.0,
                y: -1.0,
                u: 0.0,
                v: 1.0,
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
            TexVertex {
                x: 1.0,
                y: -1.0,
                u: 1.0,
                v: 1.0,
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
            TexVertex {
                x: 1.0,
                y: 1.0,
                u: 1.0,
                v: 0.0,
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
            TexVertex {
                x: -1.0,
                y: -1.0,
                u: 0.0,
                v: 1.0,
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
            TexVertex {
                x: 1.0,
                y: 1.0,
                u: 1.0,
                v: 0.0,
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
            TexVertex {
                x: -1.0,
                y: 1.0,
                u: 0.0,
                v: 0.0,
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
        ];
        let vtx_ptr = verts.as_ptr() as *const u8;
        let vtx_len = core::mem::size_of::<TexVertex>() * verts.len();
        let rc = trueos_cabi_gfx_draw_tex_triangles_no_present(tex_id, vtx_ptr, vtx_len);
        if rc != 0 {
            let _ = trueos_cabi_gfx_end_frame();
            return rc;
        }

        trueos_cabi_gfx_end_frame()
    }

    // --- Input C-ABI ---

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_input_pop_mouse(
        out_buttons: *mut u8,
        out_dx: *mut i8,
        out_dy: *mut i8,
        out_wheel: *mut i8,
    ) -> i32 {
        if out_buttons.is_null() || out_dx.is_null() || out_dy.is_null() || out_wheel.is_null() {
            return -1;
        }
        let Some(m) = usb::input::pop_mouse_event() else {
            return 0;
        };
        *out_buttons = m.buttons;
        *out_dx = m.dx;
        *out_dy = m.dy;
        *out_wheel = m.wheel;
        1
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_input_cursor_pos(
        cursor_id: u32,
        out_x: *mut i32,
        out_y: *mut i32,
    ) -> i32 {
        if out_x.is_null() || out_y.is_null() {
            return -1;
        }
        if cursor_id == 0 {
            return -1;
        }

        let idx = (cursor_id - 1) as usize;
        let mice = crate::usb::hid::mouse_cursor_snapshot();
        let tablets = crate::usb::hid::tablet_cursor_snapshot();

        let sample = if idx < mice.len() {
            Some(mice[idx])
        } else {
            let tidx = idx - mice.len();
            if tidx < tablets.len() {
                Some(tablets[tidx])
            } else {
                None
            }
        };

        let Some((nx, ny)) = sample else {
            return 1;
        };

        let (w, h) = crate::gfx::cpu_backbuffer_dimensions().unwrap_or((320, 200));
        let w1 = w.saturating_sub(1) as f64;
        let h1 = h.saturating_sub(1) as f64;

        *out_x = libm::round(nx * w1) as i32;
        *out_y = libm::round(ny * h1) as i32;
        0
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_input_cursor_buttons(
        cursor_id: u32,
        out_buttons_down: *mut u32,
    ) -> i32 {
        crate::surface::cursor::input_cursor_buttons(cursor_id, out_buttons_down)
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_input_pop_cursor_event(
        out: *mut crate::usb::hid::TrueosHidCursorEvent,
    ) -> i32 {
        crate::surface::cursor::input_pop_cursor_event(out)
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_input_read_cursor_events_since(
        read_seq: u64,
        out: *mut crate::usb::hid::TrueosHidCursorEvent,
        out_cap: u32,
        out_next_seq: *mut u64,
        out_dropped: *mut u32,
    ) -> u32 {
        crate::surface::cursor::input_read_cursor_events_since(
            read_seq,
            out,
            out_cap,
            out_next_seq,
            out_dropped,
        )
    }
}

/// Writer that routes bytes to the global console pipeline (stdout).
pub struct Stdout;

/// Writer that routes bytes to the global console pipeline (stderr).
pub struct Stderr;

pub const fn stdout() -> Stdout {
    Stdout
}

pub const fn stderr() -> Stderr {
    Stderr
}

impl Write for Stdout {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        cabi::write_bytes(cabi::CStream::Stdout, buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

impl Write for Stderr {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        cabi::write_bytes(cabi::CStream::Stderr, buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}
