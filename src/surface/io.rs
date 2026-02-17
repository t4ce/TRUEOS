
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
	use alloc::string::String;
	use crate::disc::block;

	pub type Result<T> = core::result::Result<T, FsError>;

	#[derive(Clone, Copy, Debug, Eq, PartialEq)]	pub enum FsError {
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
			return if allow_empty { Ok(out) } else { Err(FsError::BadPath) };
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
			match crate::v::fs::trueosfs::file_write_begin_async(disk, name.as_str(), total_len).await? {
				Some(h) => Ok(h),
				None => Err(FsError::NoSpace),
			}
		})
	}

	#[inline]
	pub async fn write_file_begin_async(path: &str, total_len: u64) -> Result<u32> {
		let disk = root_disk()?;
		let name = normalize_rel(path, false)?;
		match crate::v::fs::trueosfs::file_write_begin_async(disk, name.as_str(), total_len).await? {
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
			let Some(bytes) = crate::v::fs::trueosfs::file_out_async(disk, src.as_str()).await? else {
				return Err(FsError::NotFound);
			};
			let ok = crate::v::fs::trueosfs::file_in_async(disk, dst.as_str(), bytes.as_slice()).await?;
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
		let ok = crate::v::fs::trueosfs::file_in_async(disk, dst.as_str(), bytes.as_slice()).await?;
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
			let ok = crate::v::fs::trueosfs::file_append_async(disk, name.as_str(), src.as_slice()).await?;
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

	#[no_mangle]
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

	#[no_mangle]
	pub extern "C" fn trueos_cabi_poll_once() {
		// This function is used by QuickJS smokes (and other C-ABI callers) as a
		// cooperative yield point while polling for async completions.
		//
		// Do NOT call `park_step()` here: it may execute `hlt`, and on configurations
		// without a reliable periodic interrupt source that can wake the CPU, that
		// can present as a hard BSP freeze (e.g. right after `qjs-pixi-rect-smoke: starting`).
		crate::wait::spin_step();
	}

	#[no_mangle]
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

	#[no_mangle]
	pub unsafe extern "C" fn trueos_cabi_copy_cstr_into(dst: *mut u8, cap: usize, cstr: *const u8) -> i32 {
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

	#[no_mangle]
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

	#[no_mangle]
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

	#[no_mangle]
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

	#[no_mangle]
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

	#[no_mangle]
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

	#[no_mangle]
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

	#[no_mangle]
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

	#[no_mangle]
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

	#[no_mangle]
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

	#[no_mangle]
	pub unsafe extern "C" fn trueos_cabi_fs_write_finish(handle: u32) -> i32 {
		match super::kfs::write_file_finish(handle) {
			Ok(()) => 0,
			Err(e) => fs_error_to_code(e),
		}
	}

	#[no_mangle]
	pub unsafe extern "C" fn trueos_cabi_fs_write_abort(handle: u32) -> i32 {
		match super::kfs::write_file_abort(handle) {
			Ok(()) => 0,
			Err(e) => fs_error_to_code(e),
		}
	}

	#[no_mangle]
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

	#[no_mangle]
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

	#[no_mangle]
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

	use trueos_gfx_core::{
		BufferDesc, BufferId, BufferUsage, ColorFormat, Command, CommandBuffer, Extent2D, GfxContext,
		ImageDesc, ImageFormat, ImageId, MemoryType, PipelineDesc, PipelineId, SwapchainDesc,
		TexCoordFormat, VertexLayout, Viewport,
	};
	use alloc::vec::Vec;

	struct GfxCabiState {
		pipeline: PipelineId,
		vbuf: BufferId,
		capacity: usize,
		tex_pipeline: PipelineId,
		tex_vbuf: BufferId,
		tex_capacity: usize,
		tex_images: Option<Vec<Option<TexImage>>>,
		epoch: u64,
		swapchain_configured: bool,
		swapchain_desc: SwapchainDesc,
		viewport_configured: bool,
		frame_active: bool,
		frame_clear_rgb: u32,
	}

	struct TexImage {
		image: ImageId,
		width: u32,
		height: u32,
	}

	impl GfxCabiState {
		const fn new() -> Self {
			Self {
				pipeline: PipelineId::invalid(),
				vbuf: BufferId::invalid(),
				capacity: 0,
				tex_pipeline: PipelineId::invalid(),
				tex_vbuf: BufferId::invalid(),
				tex_capacity: 0,
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
					frame_clear_rgb: 0x00_08_18_30,
				}
			}
		}

	static GFX_CABI_STATE: spin::Mutex<GfxCabiState> = spin::Mutex::new(GfxCabiState::new());

	fn ensure_gfx_resources(ctx: &mut dyn GfxContext, need_bytes: usize) -> Option<(PipelineId, BufferId, bool)> {
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
			st.vbuf = BufferId::invalid();
			st.capacity = 0;
			st.tex_pipeline = PipelineId::invalid();
			st.tex_vbuf = BufferId::invalid();
			st.tex_capacity = 0;
			st.tex_images = None;
			st.swapchain_configured = false;
			st.viewport_configured = false;
			st.frame_active = false;
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
				stride: 12, // f32 x, f32 y, u8 r,g,b, pad
				pos_offset: 0,
				color_offset: 8,
				color_format: ColorFormat::RgbU8,
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

		if !st.vbuf.is_valid() || st.capacity < need_bytes {
			if st.vbuf.is_valid() {
				ctx.destroy_buffer(st.vbuf);
				st.vbuf = BufferId::invalid();
				st.capacity = 0;
			}
			let cap = need_bytes.max(256);
			let b = ctx
				.create_buffer(BufferDesc {
					size: cap as u64,
					usage: BufferUsage::Vertex,
					memory: MemoryType::HostVisible,
				})
				.ok()?;
			st.vbuf = b;
			st.capacity = cap;
		}

		let need_set_viewport = !st.viewport_configured;
		st.viewport_configured = true;
		Some((st.pipeline, st.vbuf, need_set_viewport))
	}

	fn ensure_gfx_resources_tex(ctx: &mut dyn GfxContext, need_bytes: usize) -> Option<(PipelineId, BufferId, bool)> {
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
			st.vbuf = BufferId::invalid();
			st.capacity = 0;
			st.tex_pipeline = PipelineId::invalid();
			st.tex_vbuf = BufferId::invalid();
			st.tex_capacity = 0;
			st.tex_images = None;
			st.swapchain_configured = false;
			st.viewport_configured = false;
			st.frame_active = false;
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

		if !st.tex_vbuf.is_valid() || st.tex_capacity < need_bytes {
			if st.tex_vbuf.is_valid() {
				ctx.destroy_buffer(st.tex_vbuf);
				st.tex_vbuf = BufferId::invalid();
				st.tex_capacity = 0;
			}
			let cap = need_bytes.max(256);
			let b = ctx
				.create_buffer(BufferDesc {
					size: cap as u64,
					usage: BufferUsage::Vertex,
					memory: MemoryType::HostVisible,
				})
				.ok()?;
			st.tex_vbuf = b;
			st.tex_capacity = cap;
		}

		let need_set_viewport = !st.viewport_configured;
		st.viewport_configured = true;
		Some((st.tex_pipeline, st.tex_vbuf, need_set_viewport))
	}

	/// Draw a list of RGB triangles and present.
	///
	/// Vertex ABI (bytes): repeating struct { f32 x, f32 y, u8 r, u8 g, u8 b, u8 pad }
	#[no_mangle]
	pub unsafe extern "C" fn trueos_cabi_gfx_draw_rgb_triangles(
		clear_rgb: u32,
		vtx_ptr: *const u8,
		vtx_len: usize,
	) -> i32 {
		// A = console buffer: when LimineFb is active, the Limine framebuffer is owned by the
		// console/text renderer. Refuse gfx draws here to avoid two independent writers
		// hammering the same memory (overlay/tearing) during backend swaps.
		if crate::gfx::backend_kind() == Some(crate::gfx::BackendKind::LimineFb) {
			return -10;
		}

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

				if need_set_viewport {
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
					return match ctx.submit(CommandBuffer { commands: &cmds }) {
						Ok(_) => 0,
						Err(_) => -5,
					};
				}

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

				match ctx.submit(CommandBuffer { commands: &cmds }) {
					Ok(_) => 0,
					Err(_) => -5,
				}
			}) else {
				return -6;
			};
		ret
	}

	#[no_mangle]
	pub unsafe extern "C" fn trueos_cabi_gfx_upload_texture_rgba(
		tex_id: u32,
		width: u32,
		height: u32,
		data_ptr: *const u8,
		data_len: usize,
	) -> i32 {
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
			let mut st = GFX_CABI_STATE.lock();
			let images = st.tex_images.get_or_insert_with(Vec::new);
			let idx = tex_id.saturating_sub(1) as usize;
			if idx >= images.len() {
				images.resize_with(idx + 1, || None);
			}
			let mut image_id = ImageId::invalid();
			let mut recreate = true;
			if let Some(Some(entry)) = images.get(idx) {
				if entry.width == width && entry.height == height {
					image_id = entry.image;
					recreate = false;
				}
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

	#[no_mangle]
	pub unsafe extern "C" fn trueos_cabi_gfx_begin_frame(clear_rgb: u32) -> i32 {
		let Some(ret) = crate::gfx::with_context(|ctx| {
			let (_p, _v, need_set_viewport) = match ensure_gfx_resources(ctx, 0) {
				Some(v) => v,
				None => return -1,
			};
			let mut st = GFX_CABI_STATE.lock();
			st.frame_active = true;
			st.frame_clear_rgb = clear_rgb;

			let swap = ctx.swapchain_desc();
			let vp = Viewport {
				x: 0,
				y: 0,
				width: swap.extent.width as i32,
				height: swap.extent.height as i32,
			};

			let mut cmds_buf = [Command::ClearColor { rgb: clear_rgb }; 2];
			let cmds = if need_set_viewport {
				cmds_buf[0] = Command::SetViewport(vp);
				cmds_buf[1] = Command::ClearColor { rgb: clear_rgb };
				&cmds_buf[..2]
			} else {
				&cmds_buf[0..1]
			};
			match ctx.submit(CommandBuffer { commands: cmds }) {
				Ok(_) => 0,
				Err(_) => -2,
			}
		}) else {
			return -3;
		};
		ret
	}

	#[no_mangle]
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
		let vtx = core::slice::from_raw_parts(vtx_ptr, usable);
		let vcount = (usable / VTX_SIZE) as u32;
		if vcount == 0 {
			return 0;
		}

		let Some(ret) = crate::gfx::with_context(|ctx| {
			let (pipeline, vbuf, _need_set_viewport) = match ensure_gfx_resources(ctx, usable) {
				Some(v) => v,
				None => return -3,
			};
			if ctx.write_buffer(vbuf, 0, vtx).is_err() {
				return -4;
			}
			let cmds = [
				Command::BindPipeline(pipeline),
				Command::BindVertexBuffer {
					buffer: vbuf,
					offset: 0,
				},
				Command::Draw {
					vertex_count: vcount,
					first_vertex: 0,
				},
			];
			match ctx.submit(CommandBuffer { commands: &cmds }) {
				Ok(_) => 0,
				Err(_) => -5,
			}
		}) else {
			return -6;
		};
		ret
	}

	#[no_mangle]
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
		let vtx = core::slice::from_raw_parts(vtx_ptr, usable);
		let vcount = (usable / VTX_SIZE) as u32;
		if vcount == 0 {
			return 0;
		}

		let Some(ret) = crate::gfx::with_context(|ctx| {
			let (pipeline, vbuf, _need_set_viewport) = match ensure_gfx_resources_tex(ctx, usable) {
				Some(v) => v,
				None => return -4,
			};

			let image_id = {
				let mut st = GFX_CABI_STATE.lock();
				let images = st.tex_images.get_or_insert_with(Vec::new);
				let idx = tex_id.saturating_sub(1) as usize;
				let image = if let Some(Some(entry)) = images.get(idx) {
					entry.image
				} else {
					if idx >= images.len() {
						images.resize_with(idx + 1, || None);
					}
					let desc = ImageDesc {
						width: 1,
						height: 1,
						format: ImageFormat::Rgba8888,
					};
					let Ok(img) = ctx.create_image(desc) else {
						return -5;
					};
					let white = [0u8, 0u8, 0u8, 0u8];
					let _ = ctx.write_image(img, &white);
					images[idx] = Some(TexImage {
						image: img,
						width: 1,
						height: 1,
					});
					img
				};
				image
			};

			if ctx.write_buffer(vbuf, 0, vtx).is_err() {
				return -6;
			}
			let cmds = [
				Command::BindPipeline(pipeline),
				Command::BindImage(image_id),
				Command::BindVertexBuffer {
					buffer: vbuf,
					offset: 0,
				},
				Command::Draw {
					vertex_count: vcount,
					first_vertex: 0,
				},
			];
			match ctx.submit(CommandBuffer { commands: &cmds }) {
				Ok(_) => 0,
				Err(_) => -7,
			}
		}) else {
			return -8;
		};
		ret
	}

	#[no_mangle]
	pub unsafe extern "C" fn trueos_cabi_gfx_end_frame() -> i32 {
		let Some(ret) = crate::gfx::with_context(|ctx| {
			let cmds = [Command::Present];
			let result = match ctx.submit(CommandBuffer { commands: &cmds }) {
				Ok(_) => 0,
				Err(_) => -1,
			};
			let mut st = GFX_CABI_STATE.lock();
			st.frame_active = false;
			result
		}) else {
			return -2;
		};
		ret
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
