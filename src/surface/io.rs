
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
	pub fn write_file(path: &str, data: &[u8]) -> Result<()> {
		let disk = root_disk()?;
		let name = normalize_rel(path, false)?;
		let data = data.to_vec();
		crate::wait::spawn_and_wait_local(async move {
			let ok = crate::v::fs::trueosfs::file_in_async(disk, name.as_str(), data.as_slice()).await?;
			if ok { Ok(()) } else { Err(FsError::NoSpace) }
		})
	}

	#[inline]
	pub async fn write_file_async(path: &str, data: &[u8]) -> Result<()> {
		let disk = root_disk()?;
		let name = normalize_rel(path, false)?;
		let ok = crate::v::fs::trueosfs::file_in_async(disk, name.as_str(), data).await?;
		if ok { Ok(()) } else { Err(FsError::NoSpace) }
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
	pub unsafe extern "C" fn trueos_cabi_fs_write_file(
		path_ptr: *const u8,
		path_len: usize,
		data_ptr: *const u8,
		data_len: usize,
	) -> i32 {
		if path_ptr.is_null() && path_len != 0 {
			return FS_ERR_BAD_PARAM;
		}
		if data_ptr.is_null() && data_len != 0 {
			return FS_ERR_BAD_PARAM;
		}
		if path_len > QJS_ASYNC_FS_MAX_PATH || data_len > QJS_ASYNC_FS_MAX_DATA {
			return FS_ERR_TOO_LARGE;
		}
		let path_bytes = core::slice::from_raw_parts(path_ptr, path_len);
		let Ok(path) = core::str::from_utf8(path_bytes) else {
			return FS_ERR_BAD_UTF8;
		};
		let data = if data_len == 0 { &[] } else { core::slice::from_raw_parts(data_ptr, data_len) };
		match super::kfs::write_file(path, data) {
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

	const QJS_ASYNC_FS_MAX_PATH: usize = 1024;
	const QJS_ASYNC_FS_MAX_DATA: usize = 2 * 1024 * 1024;
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

pub fn smoke_test() {
	crate::log!("io: smoke_test begin (minimal)\n");

	let _ = stdout().write_all(b"io: stdout write_all ok\n");
	let _ = stderr().write_all(b"io: stderr write_all ok\n");
}
