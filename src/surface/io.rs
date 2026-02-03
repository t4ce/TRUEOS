
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
	pub async fn read_file_async(path: &str) -> Result<Vec<u8>> {
		let disk = root_disk()?;
		let name = normalize_rel(path, false)?;
		match crate::v::fs::trueosfs::file_out_async(disk, name.as_str()).await? {
			Some(bytes) => Ok(bytes),
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
	use alloc::string::{String, ToString};
	use alloc::vec::Vec;
	use core::sync::atomic::{AtomicU32, Ordering};
	use embassy_time::{Duration as EmbassyDuration, Timer};
	use spin::Mutex;
	use crate::wait::WaitQueue;

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

		match super::kfs::read_file(path) {
			Ok(bytes) => {
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

	const QJS_ASYNC_FS_MAX_QUEUE: usize = 64;
	const QJS_ASYNC_FS_MAX_PATH: usize = 1024;
	const QJS_ASYNC_FS_MAX_DATA: usize = 2 * 1024 * 1024;

	static QJS_ASYNC_FS_SEQ: AtomicU32 = AtomicU32::new(1);

	#[derive(Clone, Copy, Debug, Eq, PartialEq)]
	enum AsyncFsKind {
		ReadFile,
		WriteFile,
	}

	#[derive(Clone, Debug)]
	struct AsyncFsRequest {
		id: u32,
		kind: AsyncFsKind,
		path: String,
		data: Vec<u8>,
	}

	#[derive(Clone, Debug)]
	struct AsyncFsCompletion {
		id: u32,
		rc: i32,
		data: Vec<u8>,
	}

	static ASYNC_FS_REQS: Mutex<Vec<AsyncFsRequest>> = Mutex::new(Vec::new());
	static ASYNC_FS_DONE: Mutex<Vec<u32>> = Mutex::new(Vec::new());
	static ASYNC_FS_RESULTS: Mutex<Vec<AsyncFsCompletion>> = Mutex::new(Vec::new());
	static ASYNC_FS_WAIT: WaitQueue = WaitQueue::new();

	#[inline]
	fn next_async_fs_id() -> u32 {
		QJS_ASYNC_FS_SEQ.fetch_add(1, Ordering::Relaxed)
	}

	fn push_async_fs_req(req: AsyncFsRequest) -> Result<(), i32> {
		let mut q = ASYNC_FS_REQS.lock();
		if q.len() >= QJS_ASYNC_FS_MAX_QUEUE {
			return Err(FS_ERR_NO_SPACE);
		}
		q.push(req);
		Ok(())
	}

	fn take_async_fs_req() -> Option<AsyncFsRequest> {
		let mut q = ASYNC_FS_REQS.lock();
		if q.is_empty() { None } else { Some(q.remove(0)) }
	}

	fn push_async_fs_completion(done: AsyncFsCompletion) {
		let id = done.id;
		ASYNC_FS_RESULTS.lock().push(done);
		ASYNC_FS_DONE.lock().push(id);
		ASYNC_FS_WAIT.notify_all();
	}

	fn find_async_fs_completion(id: u32) -> Option<AsyncFsCompletion> {
		let res = ASYNC_FS_RESULTS.lock();
		res.iter().find(|c| c.id == id).cloned()
	}

	fn remove_async_fs_completion(id: u32) {
		let mut res = ASYNC_FS_RESULTS.lock();
		if let Some(pos) = res.iter().position(|c| c.id == id) {
			res.remove(pos);
		}
	}

	pub async fn async_fs_wait_for_completion(timeout_ms: u64) -> bool {
		ASYNC_FS_WAIT.wait_for_event_timeout(timeout_ms).await
	}

	/// Blocking wait for any async fs completion (C ABI).
	///
	/// Returns 1 if a completion occurred before timeout, 0 on timeout.
	#[no_mangle]
	pub unsafe extern "C" fn trueos_cabi_async_fs_wait_for_completion_blocking(timeout_ms: u64) -> i32 {
		if ASYNC_FS_WAIT.wait_for_event_blocking(timeout_ms) { 1 } else { 0 }
	}

	/// Background worker that executes async filesystem requests started via the C ABI.
	///
	/// Spawn this once at boot. It intentionally lives in the kernel (not the qjs crate)
	/// so it can call `kfs::*_async` without introducing crate cycles.
	#[embassy_executor::task]
	pub async fn qjs_async_fs_service_task() {
		loop {
			let Some(req) = take_async_fs_req() else {
				Timer::after(EmbassyDuration::from_millis(2)).await;
				continue;
			};

			match req.kind {
				AsyncFsKind::ReadFile => {
					let out = super::kfs::read_file_async(req.path.as_str()).await;
					match out {
						Ok(bytes) => push_async_fs_completion(AsyncFsCompletion {
							id: req.id,
							rc: 0,
							data: bytes,
						}),
						Err(e) => push_async_fs_completion(AsyncFsCompletion {
							id: req.id,
							rc: fs_error_to_code(e),
							data: Vec::new(),
						}),
					}
				}
				AsyncFsKind::WriteFile => {
					let out = super::kfs::write_file_async(req.path.as_str(), req.data.as_slice()).await;
					match out {
						Ok(()) => push_async_fs_completion(AsyncFsCompletion {
							id: req.id,
							rc: 0,
							data: Vec::new(),
						}),
						Err(e) => push_async_fs_completion(AsyncFsCompletion {
							id: req.id,
							rc: fs_error_to_code(e),
							data: Vec::new(),
						}),
					}
				}
			}
		}
	}

	/// Start an async read-file operation.
	///
	/// Returns an operation id (>=0) or a negative `FS_ERR_*`.
	#[no_mangle]
	pub unsafe extern "C" fn trueos_cabi_async_fs_read_file_start(path_ptr: *const u8, path_len: usize) -> i32 {
		if path_ptr.is_null() || path_len == 0 {
			return FS_ERR_BAD_PARAM;
		}
		if path_len > QJS_ASYNC_FS_MAX_PATH {
			return FS_ERR_TOO_LARGE;
		}
		let path_bytes = core::slice::from_raw_parts(path_ptr, path_len);
		let Ok(path) = core::str::from_utf8(path_bytes) else {
			return FS_ERR_BAD_UTF8;
		};

		let id = next_async_fs_id();
		let req = AsyncFsRequest {
			id,
			kind: AsyncFsKind::ReadFile,
			path: path.to_string(),
			data: Vec::new(),
		};
		match push_async_fs_req(req) {
			Ok(()) => id as i32,
			Err(code) => code,
		}
	}

	/// Start an async write-file operation.
	///
	/// Returns an operation id (>=0) or a negative `FS_ERR_*`.
	#[no_mangle]
	pub unsafe extern "C" fn trueos_cabi_async_fs_write_file_start(
		path_ptr: *const u8,
		path_len: usize,
		data_ptr: *const u8,
		data_len: usize,
	) -> i32 {
		if path_ptr.is_null() || path_len == 0 {
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

		let id = next_async_fs_id();
		let req = AsyncFsRequest {
			id,
			kind: AsyncFsKind::WriteFile,
			path: path.to_string(),
			data: data.to_vec(),
		};
		match push_async_fs_req(req) {
			Ok(()) => id as i32,
			Err(code) => code,
		}
	}

	/// Pop one completed async fs operation id.
	///
	/// Returns 1 if an id was written to `out_id`, 0 if none.
	#[no_mangle]
	pub unsafe extern "C" fn trueos_cabi_async_fs_poll_completed(out_id: *mut u32) -> i32 {
		if out_id.is_null() {
			return 0;
		}
		let mut done = ASYNC_FS_DONE.lock();
		let Some(id) = done.first().copied() else {
			return 0;
		};
		done.remove(0);
		*out_id = id;
		1
	}

	/// Query the result length for an async fs op.
	///
	/// Returns:
	/// - `>=0`: number of result bytes available (0 for ops with no data)
	/// - `<0`: `FS_ERR_*` code if the op failed or id is unknown
	#[no_mangle]
	pub unsafe extern "C" fn trueos_cabi_async_fs_result_len(op_id: u32) -> isize {
		let Some(c) = find_async_fs_completion(op_id) else {
			return FS_ERR_NOT_FOUND as isize;
		};
		if c.rc != 0 {
			return c.rc as isize;
		}
		c.data.len() as isize
	}

	/// Read (and consume) the result bytes for an async fs op.
	///
	/// Mirrors the sync `trueos_cabi_fs_read_file` contract.
	#[no_mangle]
	pub unsafe extern "C" fn trueos_cabi_async_fs_read_result(op_id: u32, out_ptr: *mut u8, out_cap: usize) -> isize {
		let Some(c) = find_async_fs_completion(op_id) else {
			return FS_ERR_NOT_FOUND as isize;
		};
		if c.rc != 0 {
			remove_async_fs_completion(op_id);
			return c.rc as isize;
		}

		if out_ptr.is_null() || out_cap == 0 {
			return c.data.len() as isize;
		}
		if c.data.len() > out_cap {
			return FS_ERR_NO_SPACE as isize;
		}
		core::ptr::copy_nonoverlapping(c.data.as_ptr(), out_ptr, c.data.len());
		let n = c.data.len() as isize;
		remove_async_fs_completion(op_id);
		n
	}

	/// Discard a completed async fs op without reading its data.
	#[no_mangle]
	pub unsafe extern "C" fn trueos_cabi_async_fs_discard(op_id: u32) -> i32 {
		remove_async_fs_completion(op_id);
		0
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

pub fn smoke_test() {
	crate::log!("io: smoke_test begin (minimal)\n");

	let _ = stdout().write_all(b"io: stdout write_all ok\n");
	let _ = stderr().write_all(b"io: stderr write_all ok\n");
}
