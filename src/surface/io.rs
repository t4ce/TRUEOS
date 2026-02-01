
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
		match crate::time::block_on(crate::v::fs::trueosfs::file_out_async(disk, name.as_str()))? {
			Some(bytes) => Ok(bytes),
			None => Err(FsError::NotFound),
		}
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
	pub fn write_file(
		path: &str,
		data: &[u8],
	) -> Result<()> {
		let disk = root_disk()?;
		let name = normalize_rel(path, false)?;
		let ok = crate::time::block_on(crate::v::fs::trueosfs::file_in_async(disk, name.as_str(), data))?;
		if ok {
			Ok(())
		} else {
			Err(FsError::NoSpace)
		}
	}

	#[inline]
	pub async fn write_file_async(path: &str, data: &[u8]) -> Result<()> {
		let disk = root_disk()?;
		let name = normalize_rel(path, false)?;
		let ok = crate::v::fs::trueosfs::file_in_async(disk, name.as_str(), data).await?;
		if ok {
			Ok(())
		} else {
			Err(FsError::NoSpace)
		}
	}

	#[inline]
	pub fn rename(
		src: &str,
		dst: &str,
	) -> Result<()> {
		let disk = root_disk()?;
		let src = normalize_rel(src, false)?;
		let dst = normalize_rel(dst, false)?;
		if src == dst {
			return Ok(());
		}
		if crate::time::block_on(crate::v::fs::trueosfs::file_exists_async(disk, dst.as_str()))? {
			return Err(FsError::AlreadyExists);
		}
		let Some(bytes) = crate::time::block_on(crate::v::fs::trueosfs::file_out_async(disk, src.as_str()))? else {
			return Err(FsError::NotFound);
		};
		let ok = crate::time::block_on(crate::v::fs::trueosfs::file_in_async(disk, dst.as_str(), bytes.as_slice()))?;
		if !ok {
			return Err(FsError::NoSpace);
		}
		let _ = crate::time::block_on(crate::v::fs::trueosfs::file_delete_async(disk, src.as_str()));
		Ok(())
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
		match crate::time::block_on(crate::v::fs::trueosfs::list_dir_async(disk, dir.as_str()))? {
			Some(v) => Ok(v),
			None => Err(FsError::NoRoot),
		}
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
		let ok = crate::time::block_on(crate::v::fs::trueosfs::file_delete_async(disk, name.as_str()))?;
		if ok {
			Ok(())
		} else {
			Err(FsError::NotFound)
		}
	}

	#[inline]
	pub async fn remove_async(path: &str) -> Result<()> {
		let disk = root_disk()?;
		let name = normalize_rel(path, false)?;
		let ok = crate::v::fs::trueosfs::file_delete_async(disk, name.as_str()).await?;
		if ok {
			Ok(())
		} else {
			Err(FsError::NotFound)
		}
	}

	#[inline]
	pub fn create_dir_all(
		path: &str,
	) -> Result<()> {
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
		Ok(crate::time::block_on(crate::v::fs::trueosfs::file_exists_async(disk, name.as_str()))?)
	}

	#[inline]
	pub async fn exists_async(path: &str) -> Result<bool> {
		let disk = root_disk()?;
		let name = normalize_rel(path, false)?;
		Ok(crate::v::fs::trueosfs::file_exists_async(disk, name.as_str()).await?)
	}

	/// Append `src` bytes into the file at `dst_path`, creating the file if needed.
	pub fn append_into_file(
		dst_path: &str,
		src: &[u8],
	) -> Result<()> {
		let disk = root_disk()?;
		let name = normalize_rel(dst_path, false)?;
		let ok = crate::time::block_on(crate::v::fs::trueosfs::file_append_async(disk, name.as_str(), src))?;
		if ok {
			Ok(())
		} else {
			Err(FsError::NoSpace)
		}
	}

	/// Async variant of [`append_into_file`].
	pub async fn append_into_file_async(dst_path: &str, src: &[u8]) -> Result<()> {
		let disk = root_disk()?;
		let name = normalize_rel(dst_path, false)?;
		let ok = crate::v::fs::trueosfs::file_append_async(disk, name.as_str(), src).await?;
		if ok {
			Ok(())
		} else {
			Err(FsError::NoSpace)
		}
	}
}

/// Console routing + C ABI entrypoints used by embedded C code (QuickJS etc).
pub mod cabi {
	use alloc::vec::Vec;
	use alloc::boxed::Box;
	use alloc::string::{String, ToString};
	use core::sync::atomic::{AtomicU32, Ordering};
	use embassy_time::{Duration as EmbassyDuration, Timer};
	use spin::Mutex;

	static QJS_NET_SEQ: AtomicU32 = AtomicU32::new(1);

	#[inline]
	fn next_qjs_net_seq() -> u32 {
		QJS_NET_SEQ.fetch_add(1, Ordering::Relaxed)
	}

	#[repr(u32)]
	#[derive(Clone, Copy, Debug, Eq, PartialEq)]
	pub enum CStream {
		Stdout = 1,
		Stderr = 2,
	}

	pub const FS_ERR_BAD_UTF8: i32 = -1;
	pub const FS_ERR_IO: i32 = -2;
	pub const FS_ERR_NO_SPACE: i32 = -3;
	pub const FS_ERR_BAD_PARAM: i32 = -4;
	pub const FS_ERR_USBMS_NOT_FOUND: i32 = -5;
	pub const FS_ERR_BAD_PATH: i32 = -6;
	pub const FS_ERR_TOO_LARGE: i32 = -7;
	pub const FS_ERR_NOT_FOUND: i32 = -8;
	pub const FS_ERR_ALREADY_EXISTS: i32 = -9;

	pub const NET_ERR_BAD_URL: i32 = -10;
	pub const NET_ERR_TIMEOUT: i32 = -11;
	pub const NET_ERR_HTTP: i32 = -12;
	pub const NET_ERR_TLS: i32 = -13;

	// More granular timeout diagnostics (same “class” as NET_ERR_TIMEOUT).
	// These intentionally live far from the base codes to avoid collisions.
	pub const NET_ERR_TIMEOUT_DNS: i32 = -111;
	pub const NET_ERR_TIMEOUT_CONNECT: i32 = -112;
	pub const NET_ERR_TIMEOUT_HEADERS: i32 = -113;
	pub const NET_ERR_TIMEOUT_BODY: i32 = -114;

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

	/// Best-effort symbolic name for a TRUEOS C ABI return code.
	///
	/// Intended for logs and error messages (stable string values help when triaging).
	#[inline]
	pub fn code_name(code: i32) -> &'static str {
		match code {
			0 => "OK",
			FS_ERR_BAD_UTF8 => "FS_ERR_BAD_UTF8",
			FS_ERR_IO => "FS_ERR_IO",
			FS_ERR_NO_SPACE => "FS_ERR_NO_SPACE",
			FS_ERR_BAD_PARAM => "FS_ERR_BAD_PARAM",
			FS_ERR_USBMS_NOT_FOUND => "FS_ERR_USBMS_NOT_FOUND",
			FS_ERR_BAD_PATH => "FS_ERR_BAD_PATH",
			FS_ERR_TOO_LARGE => "FS_ERR_TOO_LARGE",
			FS_ERR_NOT_FOUND => "FS_ERR_NOT_FOUND",
			FS_ERR_ALREADY_EXISTS => "FS_ERR_ALREADY_EXISTS",
			NET_ERR_BAD_URL => "NET_ERR_BAD_URL",
			NET_ERR_TIMEOUT => "NET_ERR_TIMEOUT",
			NET_ERR_HTTP => "NET_ERR_HTTP",
			NET_ERR_TLS => "NET_ERR_TLS",
			NET_ERR_TIMEOUT_DNS => "NET_ERR_TIMEOUT_DNS",
			NET_ERR_TIMEOUT_CONNECT => "NET_ERR_TIMEOUT_CONNECT",
			NET_ERR_TIMEOUT_HEADERS => "NET_ERR_TIMEOUT_HEADERS",
			NET_ERR_TIMEOUT_BODY => "NET_ERR_TIMEOUT_BODY",
			_ => "UNKNOWN",
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

		// Best-effort cap to avoid pathological scans if the pointer is bogus.
		// (This does not prevent faults if the pointer is invalid/unmapped.)
		const MAX: usize = 16 * 1024;

		let mut len = 0usize;
		while len < MAX {
			if *cstr.add(len) == 0 {
				break;
			}
			len += 1;
		}

		if len == 0 {
			return;
		}

		trueos_cabi_write(stream, cstr, len);
	}

	#[no_mangle]
	pub unsafe extern "C" fn trueos_cabi_copy_cstr_into(
		dst: *mut u8,
		cap: usize,
		cstr: *const u8,
	) -> i32 {
		if dst.is_null() || cap == 0 {
			return 0;
		}

		if cstr.is_null() {
			*dst = 0;
			return 0;
		}

		let mut i = 0usize;
		while i + 1 < cap {
			let b = *cstr.add(i);
			if b == 0 {
				break;
			}
			*dst.add(i) = b;
			i += 1;
		}

		*dst.add(i) = 0;
		i as i32
	}

	/// Returns the Limine-provided boot timestamp (seconds since Unix epoch), or 0 if unavailable.
	///
	/// Exposed as a C ABI so embedded C/Rust subsystems (QuickJS shims) can obtain a best-effort
	/// realtime base without depending on kernel-internal Rust modules.
	#[no_mangle]
	pub unsafe extern "C" fn trueos_cabi_boot_timestamp_secs() -> u64 {
		crate::limine::boot_timestamp_secs().unwrap_or(0)
	}

	#[no_mangle]
	pub unsafe extern "C" fn trueos_cabi_fs_read_file(
		path_ptr: *const u8,
		path_len: usize,
		out_ptr: *mut u8,
		out_cap: usize,
	) -> isize {
		if path_ptr.is_null() || path_len == 0 {
			return FS_ERR_BAD_PARAM as isize;
		}
		let path_bytes = core::slice::from_raw_parts(path_ptr, path_len);
		let Ok(path) = core::str::from_utf8(path_bytes) else {
			return FS_ERR_BAD_UTF8 as isize;
		};

		let bytes: Vec<u8> = match super::kfs::read_file(path) {
			Ok(v) => v,
			Err(e) => return fs_error_to_code(e) as isize,
		};

		// Query mode: caller passes null/0 to obtain required size.
		if out_ptr.is_null() || out_cap == 0 {
			return bytes.len() as isize;
		}
		if bytes.len() > out_cap {
			return FS_ERR_NO_SPACE as isize;
		}
		core::ptr::copy_nonoverlapping(bytes.as_ptr(), out_ptr, bytes.len());
		bytes.len() as isize
	}

	#[no_mangle]
	pub unsafe extern "C" fn trueos_cabi_fs_write_file(
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
		let path_bytes = core::slice::from_raw_parts(path_ptr, path_len);
		let Ok(path) = core::str::from_utf8(path_bytes) else {
			return FS_ERR_BAD_UTF8;
		};
		let data = if data_len == 0 {
			&[]
		} else {
			core::slice::from_raw_parts(data_ptr, data_len)
		};

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
		if src_ptr.is_null() || dst_ptr.is_null() || src_len == 0 || dst_len == 0 {
			return FS_ERR_BAD_PARAM;
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
		if path_ptr.is_null() || path_len == 0 {
			return FS_ERR_BAD_PARAM as isize;
		}
		let path_bytes = core::slice::from_raw_parts(path_ptr, path_len);
		let Ok(path) = core::str::from_utf8(path_bytes) else {
			return FS_ERR_BAD_UTF8 as isize;
		};

		let listing = match super::kfs::list_dir(path) {
			Ok(v) => v,
			Err(e) => return fs_error_to_code(e) as isize,
		};
		let bytes = listing.as_bytes();

		if out_ptr.is_null() || out_cap == 0 {
			return bytes.len() as isize;
		}
		if bytes.len() > out_cap {
			return FS_ERR_NO_SPACE as isize;
		}
		core::ptr::copy_nonoverlapping(bytes.as_ptr(), out_ptr, bytes.len());
		bytes.len() as isize
	}

	#[no_mangle]
	pub unsafe extern "C" fn trueos_cabi_fs_remove(
		path_ptr: *const u8,
		path_len: usize,
	) -> i32 {
		if path_ptr.is_null() || path_len == 0 {
			return FS_ERR_BAD_PARAM;
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

	// ---- Async FS service (for Promise-based QuickJS APIs) ----

	const QJS_ASYNC_FS_MAX_PATH: usize = 4096;
	const QJS_ASYNC_FS_MAX_DATA: usize = 1024 * 1024;
	const QJS_ASYNC_FS_MAX_QUEUE: usize = 64;

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
		if q.is_empty() {
			None
		} else {
			Some(q.remove(0))
		}
	}

	fn push_async_fs_completion(done: AsyncFsCompletion) {
		let id = done.id;
		ASYNC_FS_RESULTS.lock().push(done);
		ASYNC_FS_DONE.lock().push(id);
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
		let data = if data_len == 0 {
			&[]
		} else {
			core::slice::from_raw_parts(data_ptr, data_len)
		};

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

	fn leak_str(s: String) -> &'static str {
		Box::leak(s.into_boxed_str())
	}

	#[derive(Clone, Debug)]
	struct ParsedUrl {
		scheme_https: bool,
		host: String,
		port: u16,
		path: String,
	}

	fn parse_url(url: &str) -> core::result::Result<ParsedUrl, i32> {
		let mut u = url.trim();
		if u.is_empty() {
			return Err(NET_ERR_BAD_URL);
		}

		let scheme_https = if let Some(rest) = u.strip_prefix("https://") {
			u = rest;
			true
		} else if let Some(rest) = u.strip_prefix("http://") {
			u = rest;
			false
		} else {
			// Default to https when scheme omitted.
			true
		};

		let (authority, path) = match u.find('/') {
			Some(pos) => (&u[..pos], &u[pos..]),
			None => (u, "/"),
		};

		let authority = authority.trim();
		if authority.is_empty() {
			return Err(NET_ERR_BAD_URL);
		}

		let (host, port) = match authority.rsplit_once(':') {
			Some((h, p)) if !h.is_empty() && !p.is_empty() => {
				let port = p.parse::<u16>().map_err(|_| NET_ERR_BAD_URL)?;
				(h.to_string(), port)
			}
			_ => {
				let port = if scheme_https { 443 } else { 80 };
				(authority.to_string(), port)
			}
		};

		let path = if path.is_empty() { "/".to_string() } else { path.to_string() };

		Ok(ParsedUrl {
			scheme_https,
			host,
			port,
			path,
		})
	}

	fn find_http_header_end(buf: &[u8]) -> Option<usize> {
		buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
	}

	fn parse_http_status(buf: &[u8]) -> Option<u16> {
		if !buf.starts_with(b"HTTP/") {
			return None;
		}
		let mut i = 0;
		while i < buf.len() && buf[i] != b' ' {
			i += 1;
		}
		if i + 4 >= buf.len() || buf[i] != b' ' {
			return None;
		}
		let d1 = buf.get(i + 1)?.wrapping_sub(b'0');
		let d2 = buf.get(i + 2)?.wrapping_sub(b'0');
		let d3 = buf.get(i + 3)?.wrapping_sub(b'0');
		if d1 > 9 || d2 > 9 || d3 > 9 {
			return None;
		}
		Some((d1 as u16) * 100 + (d2 as u16) * 10 + (d3 as u16))
	}

	fn ascii_lower(b: u8) -> u8 {
		if (b'A'..=b'Z').contains(&b) { b + 32 } else { b }
	}

	fn header_get_value<'a>(headers: &'a [u8], header_name: &[u8]) -> Option<&'a [u8]> {
		let mut i = 0;
		while i < headers.len() {
			let mut j = i;
			while j < headers.len() && headers[j] != b'\n' {
				j += 1;
			}
			let line = &headers[i..j.min(headers.len())];
			i = (j + 1).min(headers.len());
			let Some(colon) = line.iter().position(|&b| b == b':') else {
				continue;
			};
			let name = &line[..colon];
			if name.len() != header_name.len() {
				continue;
			}
			if !name
				.iter()
				.zip(header_name.iter())
				.all(|(&a, &b)| ascii_lower(a) == ascii_lower(b))
			{
				continue;
			}
			let mut k = colon + 1;
			while k < line.len() && (line[k] == b' ' || line[k] == b'\t') {
				k += 1;
			}
			let mut v = &line[k..];
			if v.ends_with(b"\r") {
				v = &v[..v.len() - 1];
			}
			return Some(v);
		}
		None
	}

	fn parse_content_length(headers: &[u8]) -> Option<usize> {
		let v = header_get_value(headers, b"content-length")?;
		let s = core::str::from_utf8(v).ok()?;
		let s = s.trim();
		if s.is_empty() {
			return None;
		}
		s.parse::<usize>().ok()
	}

	fn is_chunked_encoding(headers: &[u8]) -> bool {
		let v = header_get_value(headers, b"transfer-encoding")
			.or_else(|| header_get_value(headers, b"Transfer-Encoding"));
		let Some(v) = v else {
			return false;
		};
		let Ok(s) = core::str::from_utf8(v) else {
			return false;
		};
		s.to_ascii_lowercase().contains("chunked")
	}

	fn parse_redirect_location(url: &ParsedUrl, headers: &[u8]) -> Option<String> {
		let v = header_get_value(headers, b"location")?;
		let s = core::str::from_utf8(v).ok()?.trim();
		if s.is_empty() {
			return None;
		}
		if s.starts_with("https://") {
			return Some(s.to_string());
		}
		if s.starts_with('/') {
			return Some(alloc::format!("https://{}{}", url.host, s));
		}
		None
	}

	fn parse_hex_usize(s: &[u8]) -> Option<usize> {
		let mut n: usize = 0;
		let mut any = false;
		for &b in s {
			let v = match b {
				b'0'..=b'9' => (b - b'0') as usize,
				b'a'..=b'f' => (b - b'a' + 10) as usize,
				b'A'..=b'F' => (b - b'A' + 10) as usize,
				_ => return None,
			};
			any = true;
			n = n.checked_mul(16)?.checked_add(v)?;
		}
		if any { Some(n) } else { None }
	}

	/// Attempt to decode a chunked HTTP/1.1 body.
	///
	/// Returns:
	/// - `Ok(Some(decoded))` when a full terminating chunk has been seen.
	/// - `Ok(None)` when more bytes are needed.
	/// - `Err(NET_ERR_HTTP)` on malformed input.
	fn try_decode_chunked_body(body: &[u8], max_bytes: usize) -> core::result::Result<Option<Vec<u8>>, i32> {
		let mut out: Vec<u8> = Vec::new();
		let mut i = 0usize;
		loop {
			// Find chunk-size line end.
			let mut line_end = None;
			let mut j = i;
			while j + 1 < body.len() {
				if body[j] == b'\r' && body[j + 1] == b'\n' {
					line_end = Some(j);
					break;
				}
				j += 1;
			}
			let Some(line_end) = line_end else {
				return Ok(None);
			};

			// Parse hex size up to optional ";".
			let line = &body[i..line_end];
			let hex_part = match line.iter().position(|&b| b == b';') {
				Some(pos) => &line[..pos],
				None => line,
			};
			let hex_part = hex_part.iter().copied().filter(|b| *b != b' ' && *b != b'\t').collect::<Vec<u8>>();
			let Some(chunk_len) = parse_hex_usize(hex_part.as_slice()) else {
				return Err(NET_ERR_HTTP);
			};
			i = line_end + 2; // skip CRLF

			if chunk_len == 0 {
				// Need at least trailing CRLF (and possibly trailers ending with CRLFCRLF).
				if body.len() < i + 2 {
					return Ok(None);
				}
				// Accept either immediate CRLF or full trailer block; easiest is to require CRLFCRLF.
				if body[i..].windows(4).any(|w| w == b"\r\n\r\n") {
					return Ok(Some(out));
				}
				return Ok(None);
			}

			// Need chunk data + trailing CRLF.
			if body.len() < i + chunk_len + 2 {
				return Ok(None);
			}
			if out.len().saturating_add(chunk_len) > max_bytes {
				return Err(NET_ERR_HTTP);
			}
			out.extend_from_slice(&body[i..i + chunk_len]);
			i += chunk_len;
			if body.get(i) != Some(&b'\r') || body.get(i + 1) != Some(&b'\n') {
				return Err(NET_ERR_HTTP);
			}
			i += 2;
		}
	}

	// --- Blocking HTTPS fetch (drives the Embassy executor while waiting) ---

	const SLIRP_DNS_IP: [u8; 4] = [10, 0, 2, 3];
	const SLIRP_GATEWAY_IP: [u8; 4] = [10, 0, 2, 2];
	const DNS_PORT: u16 = 53;

	fn parse_ipv4_literal(host: &str) -> Option<[u8; 4]> {
		let mut out = [0u8; 4];
		let mut i: usize = 0;
		for part in host.split('.') {
			if i >= 4 {
				return None;
			}
			let Ok(v) = part.parse::<u8>() else {
				return None;
			};
			out[i] = v;
			i += 1;
		}
		if i == 4 {
			Some(out)
		} else {
			None
		}
	}

	fn dns_query(id: u16, host: &str, qtype: u16) -> Vec<u8> {
		let mut q = Vec::new();
		q.extend_from_slice(&id.to_be_bytes());
		q.extend_from_slice(&0x0100u16.to_be_bytes()); // RD
		q.extend_from_slice(&1u16.to_be_bytes()); // qdcount
		q.extend_from_slice(&0u16.to_be_bytes());
		q.extend_from_slice(&0u16.to_be_bytes());
		q.extend_from_slice(&0u16.to_be_bytes());
		for label in host.split('.') {
			let bytes = label.as_bytes();
			let len = bytes.len().min(63);
			q.push(len as u8);
			q.extend_from_slice(&bytes[..len]);
		}
		q.push(0);
		q.extend_from_slice(&qtype.to_be_bytes());
		q.extend_from_slice(&1u16.to_be_bytes());
		q
	}

	fn dns_skip_name(pkt: &[u8], idx: &mut usize) -> bool {
		if *idx >= pkt.len() {
			return false;
		}
		let mut steps: u8 = 0;
		loop {
			if *idx >= pkt.len() {
				return false;
			}
			let b = pkt[*idx];
			if b == 0 {
				*idx += 1;
				return true;
			}
			if (b & 0xC0) == 0xC0 {
				if *idx + 1 >= pkt.len() {
					return false;
				}
				*idx += 2;
				return true;
			}
			let len = b as usize;
			*idx += 1;
			if *idx + len > pkt.len() {
				return false;
			}
			*idx += len;
			steps = steps.wrapping_add(1);
			if steps > 64 {
				return false;
			}
		}
	}

	#[derive(Clone, Debug)]
	enum DnsResolution {
		A([u8; 4]),
		Cname(alloc::string::String),
	}

	fn dns_read_name(pkt: &[u8], start: usize) -> Option<(alloc::string::String, usize)> {
		use alloc::string::String;

		if start >= pkt.len() {
			return None;
		}

		let mut name = String::new();
		let mut idx = start;
		let mut consumed_end: Option<usize> = None;
		let mut steps: u16 = 0;
		let mut jumped: u8 = 0;
		let mut first = true;

		loop {
			if idx >= pkt.len() {
				return None;
			}
			steps = steps.wrapping_add(1);
			if steps > 256 {
				return None;
			}

			let len = pkt[idx];
			if len == 0 {
				idx += 1;
				if consumed_end.is_none() {
					consumed_end = Some(idx);
				}
				break;
			}

			// Compression pointer.
			if (len & 0xC0) == 0xC0 {
				if idx + 1 >= pkt.len() {
					return None;
				}
				let b2 = pkt[idx + 1];
				let ptr = (((len as u16 & 0x3F) << 8) | b2 as u16) as usize;
				if ptr >= pkt.len() {
					return None;
				}
				if consumed_end.is_none() {
					consumed_end = Some(idx + 2);
				}
				idx = ptr;
				jumped = jumped.wrapping_add(1);
				if jumped > 16 {
					return None;
				}
				continue;
			}

			let lab_len = len as usize;
			idx += 1;
			if idx + lab_len > pkt.len() {
				return None;
			}
			if !first {
				let _ = name.push('.');
			}
			first = false;
			for &b in &pkt[idx..idx + lab_len] {
				let ch = if b.is_ascii_graphic() || b == b'-' {
					b as char
				} else {
					'_'
				};
				let _ = name.push(ch);
			}
			idx += lab_len;
		}

		Some((name, consumed_end.unwrap_or(idx)))
	}

	fn dns_parse_first_a_or_cname(pkt: &[u8], want_id: u16) -> Option<DnsResolution> {
		if pkt.len() < 12 {
			return None;
		}
		let id = u16::from_be_bytes([pkt[0], pkt[1]]);
		if id != want_id {
			return None;
		}
		let flags = u16::from_be_bytes([pkt[2], pkt[3]]);
		let rcode = (flags & 0x000F) as u8;
		if rcode != 0 {
			return None;
		}
		let qd = u16::from_be_bytes([pkt[4], pkt[5]]) as usize;
		let an = u16::from_be_bytes([pkt[6], pkt[7]]) as usize;
		let mut idx: usize = 12;
		for _ in 0..qd {
			if !dns_skip_name(pkt, &mut idx) {
				return None;
			}
			if idx + 4 > pkt.len() {
				return None;
			}
			idx += 4;
		}
		for _ in 0..an {
			if !dns_skip_name(pkt, &mut idx) {
				return None;
			}
			if idx + 10 > pkt.len() {
				return None;
			}
			let typ = u16::from_be_bytes([pkt[idx], pkt[idx + 1]]);
			let class = u16::from_be_bytes([pkt[idx + 2], pkt[idx + 3]]);
			let rdlen = u16::from_be_bytes([pkt[idx + 8], pkt[idx + 9]]) as usize;
			idx += 10;
			if idx + rdlen > pkt.len() {
				return None;
			}
			if class == 1 {
				if typ == 1 && rdlen == 4 {
					return Some(DnsResolution::A([pkt[idx], pkt[idx + 1], pkt[idx + 2], pkt[idx + 3]]));
				}
				if typ == 5 {
					if let Some((name, _end)) = dns_read_name(pkt, idx) {
						if !name.is_empty() {
							return Some(DnsResolution::Cname(name));
						}
					}
				}
			}
			idx += rdlen;
		}
		None
	}

	fn poll_executor_for_progress() {
		// Keep timers and tasks moving even when called from non-async contexts.
		crate::time::poll();
		crate::time::poll_executor();
	}

	fn resolve_ipv4_via_dot_blocking(dev_idx: usize, host: &str, timeout_ms: u64, cname_depth: u8) -> Option<[u8; 4]> {
		use crate::v::net::Queue;
		use trueos_v::vnet as vnet;
		use crate::net::tls_socket::{register_tls_app_queues, TlsCommand, TlsEvent};
		use crate::net::tls::{TlsClientConfig, TlsRoots};

		const DOT_PORT: u16 = 853; // DNS over TLS port
		// DoT involves a full TCP+TLS handshake; 8s was too tight under QEMU/SLIRP.
		let t = core::cmp::max(5_000, core::cmp::min(timeout_ms, 20_000));
		let dns_id: u16 = 0xED00;
		let query = dns_query(dns_id, host, 1);
		let mut framed = Vec::with_capacity(query.len().saturating_add(2));
		framed.extend_from_slice(&(query.len() as u16).to_be_bytes());
		framed.extend_from_slice(&query);

		let providers: &[([u8; 4], &'static str)] = &[
			([1, 1, 1, 1], "cloudflare-dns.com"),
			([8, 8, 8, 8], "dns.google"),
		];

		for &(server_ip, sni) in providers {
			let seq = next_qjs_net_seq();
			let owner = leak_str(alloc::format!("qjs-dot-{}@{}", seq, dev_idx));
			let cmds_name = leak_str(alloc::format!("{}-tls-cmd", owner));
			let evts_name = leak_str(alloc::format!("{}-tls-evt", owner));
			let cmds = Queue::new_leaked(cmds_name, 512);
			let events = Queue::new_leaked(evts_name, 512);
			register_tls_app_queues(owner, cmds, events);

			let roots = TlsRoots::mozilla();
			let cfg = TlsClientConfig::new();
			let server_name = sni;

			let start = embassy_time_driver::now() as u64;
			let deadline = start
				.saturating_add(t.saturating_mul(embassy_time_driver::TICK_HZ as u64 / 1000).max(1));
			let mut tls_handle: Option<vnet::NetHandle> = None;
			let mut sent_connect = false;
			let mut sent_query = false;
			let mut buf: Vec<u8> = Vec::new();

			loop {
				for ev in events.drain(256) {
					match ev {
						TlsEvent::Opened { handle } => tls_handle = Some(handle),
						TlsEvent::Connected { handle } => {
							if tls_handle != Some(handle) {
								continue;
							}
							if !sent_query {
								let _ = cmds.push(TlsCommand::Send {
									handle,
									data: framed.clone(),
								});
								sent_query = true;
							}
						}
						TlsEvent::Data { handle, data } => {
							if tls_handle != Some(handle) {
								continue;
							}
							buf.extend_from_slice(&data);
							if buf.len() >= 2 {
								let msg_len = u16::from_be_bytes([buf[0], buf[1]]) as usize;
								if buf.len() >= 2 + msg_len {
									let pkt = &buf[2..2 + msg_len];
									match dns_parse_first_a_or_cname(pkt, dns_id) {
										Some(DnsResolution::A(ip)) => {
											let _ = cmds.push(TlsCommand::Close { handle });
											return Some(ip);
										}
										Some(DnsResolution::Cname(name)) => {
											let _ = cmds.push(TlsCommand::Close { handle });
											if cname_depth == 0 {
												return None;
											}
											return resolve_ipv4_blocking_inner(
												dev_idx,
												name.as_str(),
												timeout_ms,
												cname_depth - 1,
											);
										}
										None => {}
									}
									break;
								}
							}
						}
						TlsEvent::Closed { .. } => break,
						TlsEvent::Error { .. } => break,
						TlsEvent::TlsError { .. } => break,
					}
				}

				if !sent_connect {
					sent_connect = true;
					let _ = cmds.push(TlsCommand::OpenTcpConnect {
						remote: vnet::EndpointV4 { addr: server_ip, port: DOT_PORT },
						server_name,
						cfg: cfg.clone(),
						roots: roots.clone(),
					});
				}

				let now = embassy_time_driver::now() as u64;
				if now >= deadline {
					if let Some(handle) = tls_handle {
						let _ = cmds.push(TlsCommand::Close { handle });
					}
					break;
				}
				poll_executor_for_progress();
			}
		}

		None
	}

	fn resolve_ipv4_via_doh_blocking(dev_idx: usize, host: &str, timeout_ms: u64, cname_depth: u8) -> Option<[u8; 4]> {
		use crate::v::net::Queue;
		use trueos_v::vnet as vnet;
		use crate::net::tls_socket::{register_tls_app_queues, TlsCommand, TlsEvent};
		use crate::net::tls::{TlsClientConfig, TlsRoots};

		const DOH_PORT: u16 = 443; // DNS over HTTPS port
		// DoH is even heavier than DoT (HTTP response parsing); allow a bit longer.
		let t = core::cmp::max(6_000, core::cmp::min(timeout_ms, 25_000));
		let dns_id: u16 = 0xEE00;
		let query = dns_query(dns_id, host, 1);
		let max_bytes: usize = 64 * 1024;

		let providers: &[([u8; 4], &'static str)] = &[
			([1, 1, 1, 1], "cloudflare-dns.com"),
			([8, 8, 8, 8], "dns.google"),
		];

		for &(server_ip, sni) in providers {
			let seq = next_qjs_net_seq();
			let owner = leak_str(alloc::format!("qjs-doh-{}@{}", seq, dev_idx));
			let cmds_name = leak_str(alloc::format!("{}-tls-cmd", owner));
			let evts_name = leak_str(alloc::format!("{}-tls-evt", owner));
			let cmds = Queue::new_leaked(cmds_name, 512);
			let events = Queue::new_leaked(evts_name, 512);
			register_tls_app_queues(owner, cmds, events);

			let roots = TlsRoots::mozilla();
			let cfg = TlsClientConfig::new().with_alpn_protocols(&[b"http/1.1"]);
			let server_name = sni;

			let start = embassy_time_driver::now() as u64;
			let deadline = start
				.saturating_add(t.saturating_mul(embassy_time_driver::TICK_HZ as u64 / 1000).max(1));
			let mut tls_handle: Option<vnet::NetHandle> = None;
			let mut sent_connect = false;
			let mut sent_query = false;
			let mut plaintext: Vec<u8> = Vec::new();

			let req = alloc::format!(
				"POST /dns-query HTTP/1.1\r\nHost: {}\r\nUser-Agent: TRUEOS qjs-doh\r\nAccept: application/dns-message\r\nContent-Type: application/dns-message\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
				sni,
				query.len()
			)
			.into_bytes();

			loop {
				for ev in events.drain(256) {
					match ev {
						TlsEvent::Opened { handle } => tls_handle = Some(handle),
						TlsEvent::Connected { handle } => {
							if tls_handle != Some(handle) {
								continue;
							}
							if !sent_query {
								let mut out = Vec::with_capacity(req.len() + query.len());
								out.extend_from_slice(&req);
								out.extend_from_slice(&query);
								if cmds.push(TlsCommand::Send { handle, data: out }).is_ok() {
									sent_query = true;
								}
							}
						}
						TlsEvent::Data { handle, data } => {
							if tls_handle != Some(handle) {
								continue;
							}
							if plaintext.len() < max_bytes {
								let room = max_bytes - plaintext.len();
								let take = data.len().min(room);
								plaintext.extend_from_slice(&data[..take]);
							}

							if let Some(hdr_end) = find_http_header_end(&plaintext) {
								let headers = &plaintext[..hdr_end];
								let status = parse_http_status(headers).unwrap_or(0);
								if status != 200 {
									break;
								}
								let body = &plaintext[hdr_end..];
								if is_chunked_encoding(headers) {
									if let Ok(Some(decoded)) =
										try_decode_chunked_body(body, max_bytes.saturating_sub(hdr_end))
									{
										match dns_parse_first_a_or_cname(&decoded, dns_id) {
											Some(DnsResolution::A(ip)) => {
												let _ = cmds.push(TlsCommand::Close { handle });
												return Some(ip);
											}
											Some(DnsResolution::Cname(name)) => {
												let _ = cmds.push(TlsCommand::Close { handle });
												if cname_depth == 0 {
													return None;
												}
												return resolve_ipv4_blocking_inner(
													dev_idx,
													name.as_str(),
													timeout_ms,
													cname_depth - 1,
												);
											}
											None => {}
										}
									}
								} else if let Some(cl) = parse_content_length(headers) {
									if body.len() >= cl {
										let pkt = &body[..cl];
										match dns_parse_first_a_or_cname(pkt, dns_id) {
											Some(DnsResolution::A(ip)) => {
												let _ = cmds.push(TlsCommand::Close { handle });
												return Some(ip);
											}
											Some(DnsResolution::Cname(name)) => {
												let _ = cmds.push(TlsCommand::Close { handle });
												if cname_depth == 0 {
													return None;
												}
												return resolve_ipv4_blocking_inner(
													dev_idx,
													name.as_str(),
													timeout_ms,
													cname_depth - 1,
												);
											}
											None => {}
										}
									}
								}
							}
						}
						TlsEvent::Closed { handle } => {
							if tls_handle != Some(handle) {
								continue;
							}
							let Some(hdr_end) = find_http_header_end(&plaintext) else { break };
							let headers = &plaintext[..hdr_end];
							let status = parse_http_status(headers).unwrap_or(0);
							if status != 200 {
								break;
							}
							let body = &plaintext[hdr_end..];
							let pkt: Vec<u8> = if is_chunked_encoding(headers) {
								match try_decode_chunked_body(body, max_bytes.saturating_sub(hdr_end)) {
									Ok(Some(decoded)) => decoded,
									_ => break,
								}
							} else if let Some(cl) = parse_content_length(headers) {
								if body.len() < cl {
									break;
								}
								body[..cl].to_vec()
							} else {
								body.to_vec()
							};
							match dns_parse_first_a_or_cname(&pkt, dns_id) {
								Some(DnsResolution::A(ip)) => return Some(ip),
								Some(DnsResolution::Cname(name)) => {
									if cname_depth == 0 {
										return None;
									}
									return resolve_ipv4_blocking_inner(
										dev_idx,
										name.as_str(),
										timeout_ms,
										cname_depth - 1,
									);
								}
								None => {}
							}
							break;
						}
						TlsEvent::Error { .. } => break,
						TlsEvent::TlsError { .. } => break,
					}
				}

				if !sent_connect {
					if cmds
						.push(TlsCommand::OpenTcpConnect {
							remote: vnet::EndpointV4 { addr: server_ip, port: DOH_PORT },
							server_name,
							cfg: cfg.clone(),
							roots: roots.clone(),
						})
						.is_ok()
					{
						sent_connect = true;
					}
				}

				let now = embassy_time_driver::now() as u64;
				if now >= deadline {
					if let Some(handle) = tls_handle {
						let _ = cmds.push(TlsCommand::Close { handle });
					}
					break;
				}
				poll_executor_for_progress();
			}
		}

		None
	}

	fn resolve_ipv4_blocking(dev_idx: usize, host: &str, timeout_ms: u64) -> Option<[u8; 4]> {
		resolve_ipv4_blocking_inner(dev_idx, host, timeout_ms, 4)
	}

	fn resolve_ipv4_blocking_inner(
		dev_idx: usize,
		host: &str,
		timeout_ms: u64,
		cname_depth: u8,
	) -> Option<[u8; 4]> {
		use crate::v::net::VNet;
		use trueos_v::vnet as vnet;

		if let Some(ip) = parse_ipv4_literal(host) {
			return Some(ip);
		}

		let dns_id: u16 = 0xEA00u16.wrapping_add(dev_idx as u16);
		let seq = next_qjs_net_seq();
		let net = VNet::open(dev_idx)?;

		// Important: the UDP port must be unique per call. Multiple concurrent
		// DNS resolves can otherwise collide on a fixed port, causing bind failures
		// and timeouts (especially during boot when multiple subsystems fetch).
		let local_port: u16 = 40000u16
			.wrapping_add(((dev_idx as u16).wrapping_mul(2000)) % 20000)
			.wrapping_add((seq as u16) % 2000);
		let _ = net.submit(vnet::Command::OpenUdp { port: local_port });

		let start = embassy_time_driver::now() as u64;
		let to_ticks = |ms: u64| -> u64 {
			ms.saturating_mul(embassy_time_driver::TICK_HZ as u64 / 1000).max(1)
		};
		let deadline = start.saturating_add(to_ticks(timeout_ms));
		let mut udp: Option<vnet::NetHandle> = None;
		let mut sent = false;
		let dns_servers: &[[u8; 4]] = &[
			SLIRP_DNS_IP,
			SLIRP_GATEWAY_IP,
			[1, 1, 1, 1],
			[8, 8, 8, 8],
		];
		let mut last_send_at: u64 = 0;
		let resend_ticks: u64 = (embassy_time_driver::TICK_HZ as u64 / 2).max(1);

		loop {
			for _ in 0..256 {
				let Some(ev) = net.pop_event() else {
					break;
				};
				match ev {
					vnet::Event::Opened { handle, kind } => {
						if kind == vnet::SocketKind::Udp {
							udp = Some(handle);
						}
					}
					vnet::Event::Error { msg } => {
						// Any UDP path error (open/bind/send). Don't sit on a long timeout:
						// immediately fall back to DoT/DoH.
						crate::log!(
							"qjs-dns: udp error dev={} host={} msg={} -> trying dot/doh\n",
							dev_idx,
							host,
							msg
						);
						if let Some(ip) = resolve_ipv4_via_dot_blocking(dev_idx, host, timeout_ms, cname_depth) {
							return Some(ip);
						}
						return resolve_ipv4_via_doh_blocking(dev_idx, host, timeout_ms, cname_depth);
					}
					vnet::Event::UdpPacket { handle, from, data } => {
						if udp != Some(handle) {
							continue;
						}
						if from.port != DNS_PORT {
							continue;
						}
						match dns_parse_first_a_or_cname(data.as_slice(), dns_id) {
							Some(DnsResolution::A(ip)) => {
								let _ = net.submit(vnet::Command::Close { handle });
								return Some(ip);
							}
							Some(DnsResolution::Cname(name)) => {
								let _ = net.submit(vnet::Command::Close { handle });
								if cname_depth == 0 {
									return None;
								}
								return resolve_ipv4_blocking_inner(dev_idx, name.as_str(), timeout_ms, cname_depth - 1);
							}
							None => {}
						}
					}
					_ => {}
				}
			}

			let now = embassy_time_driver::now() as u64;
			if !sent || now.saturating_sub(last_send_at) >= resend_ticks {
				if let Some(handle) = udp {
					let query = dns_query(dns_id, host, 1);
					for &server in dns_servers {
						let _ = net.submit(vnet::Command::SendUdp {
							handle,
							remote: vnet::EndpointV4 { addr: server, port: DNS_PORT },
							data: vnet::ByteBuf::from_slice_trunc(&query),
						});
					}
					sent = true;
					last_send_at = now;
				}
			}
			if now >= deadline {
				if let Some(handle) = udp {
					let _ = net.submit(vnet::Command::Close { handle });
				}
				crate::log!(
					"qjs-dns: udp timeout dev={} host={} -> trying dot/doh\n",
					dev_idx,
					host
				);
				if let Some(ip) = resolve_ipv4_via_dot_blocking(dev_idx, host, timeout_ms, cname_depth) {
					return Some(ip);
				}
				return resolve_ipv4_via_doh_blocking(dev_idx, host, timeout_ms, cname_depth);
			}

			poll_executor_for_progress();
		}
	}

	fn https_get_body_blocking(url: &ParsedUrl, timeout_ms: u64, max_bytes: usize) -> core::result::Result<Vec<u8>, i32> {
		https_get_body_blocking_inner(url, timeout_ms, max_bytes, 4)
	}

	fn https_get_body_blocking_inner(
		url: &ParsedUrl,
		timeout_ms: u64,
		max_bytes: usize,
		redirects_left: u8,
	) -> core::result::Result<Vec<u8>, i32> {
		use crate::v::net::Queue;
		use trueos_v::vnet as vnet;
		use crate::net::tls_socket::{register_tls_app_queues, TlsCommand, TlsEvent};
		use crate::net::tls::{TlsClientConfig, TlsRoots};

		// Use the primary NIC (device 0). Trying all NICs amplifies DNS/TLS socket
		// usage and can trigger transient "no sockets available" failures during boot.
		let dev_indices: [usize; 1] = [0];
		let mut last_err: i32 = NET_ERR_TIMEOUT;

		for dev_idx in dev_indices {
			// DNS resolution is often the slowest/most jittery part under QEMU/SLIRP.
			// Keep it bounded, but don't clamp so tightly that DoT/DoH can never complete.
			let dns_timeout_ms = core::cmp::max(5_000, core::cmp::min(timeout_ms, 20_000));
			let Some(ip) = resolve_ipv4_blocking(dev_idx, url.host.as_str(), dns_timeout_ms) else {
				last_err = NET_ERR_TIMEOUT_DNS;
				continue;
			};

			let seq = next_qjs_net_seq();
			let owner = leak_str(alloc::format!("qjs-https-{}@{}", seq, dev_idx));
			let cmds_name = leak_str(alloc::format!("{}-tls-cmd", owner));
			let evts_name = leak_str(alloc::format!("{}-tls-evt", owner));
			let cmds = Queue::new_leaked(cmds_name, 512);
			let events = Queue::new_leaked(evts_name, 512);
			register_tls_app_queues(owner, cmds, events);

			let mut tls_handle: Option<vnet::NetHandle> = None;
			let mut sent_connect = false;
			let mut http_sent = false;
			let mut plaintext: Vec<u8> = Vec::new();
			#[inline]
			fn ticks_from_ms(ms: u64) -> u64 {
				ms.saturating_mul(embassy_time_driver::TICK_HZ as u64)
					.saturating_div(1000)
					.max(1)
			}

			// `timeout_ms` is treated as an *inactivity* timeout (reset on progress).
			// Also apply a hard cap so a slow drip can't hang forever.
			let start = embassy_time_driver::now() as u64;
			let mut deadline = start.saturating_add(ticks_from_ms(timeout_ms));
			let hard_deadline = start.saturating_add(ticks_from_ms(core::cmp::max(timeout_ms, 180_000)));

			let roots = TlsRoots::mozilla();
			let cfg = TlsClientConfig::new().with_alpn_protocols(&[b"http/1.1"]);
			let server_name = leak_str(url.host.clone());

			'session: loop {
				for ev in events.drain(256) {
					match ev {
						TlsEvent::Opened { handle } => {
							tls_handle = Some(handle);
							// Progress in the state machine: reset inactivity timeout.
							deadline = (embassy_time_driver::now() as u64)
								.saturating_add(ticks_from_ms(timeout_ms));
						}
						TlsEvent::Connected { handle } => {
							if tls_handle != Some(handle) {
								continue;
							}
							if !http_sent {
								let req = alloc::format!(
									"GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: TRUEOS qjs-fetch\r\nAccept: */*\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n",
									url.path,
									url.host
								);
								if cmds
									.push(TlsCommand::Send {
									handle,
									data: req.into_bytes(),
								})
								.is_ok()
								{
									http_sent = true;
									// We successfully queued the request: reset inactivity timeout.
									deadline = (embassy_time_driver::now() as u64)
										.saturating_add(ticks_from_ms(timeout_ms));
								}
							}
						}
						TlsEvent::Data { handle, data } => {
							if tls_handle != Some(handle) {
								continue;
							}
							// We made forward progress: reset inactivity timeout.
							deadline = (embassy_time_driver::now() as u64)
								.saturating_add(ticks_from_ms(timeout_ms));
							if plaintext.len() < max_bytes {
								let room = max_bytes - plaintext.len();
								let take = data.len().min(room);
								plaintext.extend_from_slice(&data[..take]);
							}

							// If we already have full headers, we may be able to finish without waiting for close.
							if let Some(hdr_end) = find_http_header_end(&plaintext) {
								let headers = &plaintext[..hdr_end];
								let status = parse_http_status(headers).unwrap_or(0);
								if status == 200 {
									let body = &plaintext[hdr_end..];
									if is_chunked_encoding(headers) {
										match try_decode_chunked_body(body, max_bytes.saturating_sub(hdr_end)) {
											Ok(Some(decoded)) => return Ok(decoded),
											Ok(None) => {}
											Err(e) => {
												last_err = e;
												break 'session;
											}
									}
									} else if let Some(cl) = parse_content_length(headers) {
										if body.len() >= cl {
											return Ok(body[..cl].to_vec());
										}
									}
								} else if (status == 301 || status == 302 || status == 303 || status == 307 || status == 308)
									&& redirects_left > 0
								{
									if let Some(loc) = parse_redirect_location(url, headers) {
										let parsed = parse_url(loc.as_str())?;
										return https_get_body_blocking_inner(
											&parsed,
											timeout_ms,
											max_bytes,
											redirects_left.saturating_sub(1),
										);
									}
									last_err = NET_ERR_HTTP;
									break 'session;
								}
							}
						}
						TlsEvent::Closed { handle } => {
							if tls_handle != Some(handle) {
								continue;
							}
							let Some(hdr_end) = find_http_header_end(&plaintext) else {
								last_err = NET_ERR_HTTP;
								break 'session;
							};
							let headers = &plaintext[..hdr_end];
							let status = parse_http_status(headers).unwrap_or(0);
							if status != 200 {
								last_err = NET_ERR_HTTP;
								break 'session;
							}
							let body = &plaintext[hdr_end..];
							if is_chunked_encoding(headers) {
								match try_decode_chunked_body(body, max_bytes.saturating_sub(hdr_end)) {
									Ok(Some(decoded)) => return Ok(decoded),
									Ok(None) => {
										last_err = NET_ERR_TIMEOUT_BODY;
										break 'session;
									}
									Err(e) => {
										last_err = e;
										break 'session;
									}
								}
							} else if let Some(cl) = parse_content_length(headers) {
								if body.len() >= cl {
									return Ok(body[..cl].to_vec());
								}
								last_err = NET_ERR_TIMEOUT_BODY;
								break 'session;
							}
							return Ok(body.to_vec());
						}
						TlsEvent::Error { .. } => {
							last_err = NET_ERR_HTTP;
							break 'session;
						}
						TlsEvent::TlsError { .. } => {
							last_err = NET_ERR_TLS;
							break 'session;
						}
					}
				}

				if !sent_connect {
						if cmds
							.push(TlsCommand::OpenTcpConnect {
							remote: vnet::EndpointV4 { addr: ip, port: url.port },
						server_name,
						cfg: cfg.clone(),
						roots: roots.clone(),
						})
						.is_ok()
						{
							sent_connect = true;
						}
				}

				let now = embassy_time_driver::now() as u64;
				if now >= deadline || now >= hard_deadline {
					let hdr_end = find_http_header_end(&plaintext);
					let have_hdr = hdr_end.is_some();
					let (status, body_len) = if let Some(hdr_end) = hdr_end {
						(
							parse_http_status(&plaintext[..hdr_end]).unwrap_or(0) as u32,
							plaintext.len().saturating_sub(hdr_end) as u32,
						)
					} else {
						(0, 0)
					};
					crate::log!(
						"qjs-fetch: timeout dev={} host={} port={} http_sent={} bytes={} have_hdr={} status={} body_bytes={}\n",
						dev_idx,
						url.host.as_str(),
						url.port,
						http_sent,
						plaintext.len(),
						have_hdr,
						status,
						body_len,
					);

					last_err = if tls_handle.is_none() || !http_sent {
						NET_ERR_TIMEOUT_CONNECT
					} else if !have_hdr {
						NET_ERR_TIMEOUT_HEADERS
					} else {
						NET_ERR_TIMEOUT_BODY
					};
					break 'session;
				}
				poll_executor_for_progress();
			}
		}

		Err(last_err)
	}

	async fn resolve_ipv4_via_doh_async_inner(
		dev_idx: usize,
		host: &str,
		timeout_ms: u64,
		cname_depth: u8,
	) -> Option<[u8; 4]> {
		use alloc::string::String as AString;
		use crate::v::net::Queue;
		use trueos_v::vnet as vnet;
		use crate::net::tls_socket::{register_tls_app_queues, TlsCommand, TlsEvent};
		use crate::net::tls::{TlsClientConfig, TlsRoots};
		use embassy_time::Instant;

		const DOH_PORT: u16 = 443;
		let dns_id: u16 = 0xEE00;
		let max_bytes: usize = 64 * 1024;

		let providers: &[([u8; 4], &'static str)] = &[
			([1, 1, 1, 1], "cloudflare-dns.com"),
			([8, 8, 8, 8], "dns.google"),
		];

		let mut current_host: AString = AString::from(host);
		let mut cname_left: u8 = cname_depth;

		loop {
			if let Some(ip) = parse_ipv4_literal(current_host.as_str()) {
				return Some(ip);
			}

			let t = core::cmp::max(6_000, core::cmp::min(timeout_ms, 25_000));
			let query = dns_query(dns_id, current_host.as_str(), 1);
			let mut next_cname: Option<AString> = None;

			for &(server_ip, sni) in providers {
			let seq = next_qjs_net_seq();
			let owner = leak_str(alloc::format!("async-doh-{}@{}", seq, dev_idx));
			let cmds_name = leak_str(alloc::format!("{}-tls-cmd", owner));
			let evts_name = leak_str(alloc::format!("{}-tls-evt", owner));
			let cmds = Queue::new_leaked(cmds_name, 512);
			let events = Queue::new_leaked(evts_name, 512);
			register_tls_app_queues(owner, cmds, events);

			let roots = TlsRoots::mozilla();
			let cfg = TlsClientConfig::new().with_alpn_protocols(&[b"http/1.1"]);
			let server_name = sni;

			let deadline = Instant::now() + EmbassyDuration::from_millis(t);
			let mut tls_handle: Option<vnet::NetHandle> = None;
			let mut sent_connect = false;
			let mut sent_query = false;
			let mut plaintext: Vec<u8> = Vec::new();

			let req = alloc::format!(
				"POST /dns-query HTTP/1.1\r\nHost: {}\r\nUser-Agent: TRUEOS async-doh\r\nAccept: application/dns-message\r\nContent-Type: application/dns-message\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
				sni,
				query.len()
			)
			.into_bytes();

			let mut cname_from_provider: Option<AString> = None;
			'session: loop {
				for ev in events.drain(256) {
					match ev {
						TlsEvent::Opened { handle } => tls_handle = Some(handle),
						TlsEvent::Connected { handle } => {
							if tls_handle != Some(handle) {
								continue;
							}
							if !sent_query {
								let mut out = Vec::with_capacity(req.len() + query.len());
								out.extend_from_slice(&req);
								out.extend_from_slice(&query);
								if cmds.push(TlsCommand::Send { handle, data: out }).is_ok() {
									sent_query = true;
								}
							}
						}
						TlsEvent::Data { handle, data } => {
							if tls_handle != Some(handle) {
								continue;
							}
							if plaintext.len() < max_bytes {
								let room = max_bytes - plaintext.len();
								let take = data.len().min(room);
								plaintext.extend_from_slice(&data[..take]);
							}

							if let Some(hdr_end) = find_http_header_end(&plaintext) {
								let headers = &plaintext[..hdr_end];
								let status = parse_http_status(headers).unwrap_or(0);
								if status != 200 {
									break;
								}
								let body = &plaintext[hdr_end..];
								if is_chunked_encoding(headers) {
									if let Ok(Some(decoded)) =
										try_decode_chunked_body(body, max_bytes.saturating_sub(hdr_end))
									{
										match dns_parse_first_a_or_cname(&decoded, dns_id) {
											Some(DnsResolution::A(ip)) => {
												let _ = cmds.push(TlsCommand::Close { handle });
												return Some(ip);
											}
											Some(DnsResolution::Cname(name)) => {
												let _ = cmds.push(TlsCommand::Close { handle });
												cname_from_provider = Some(name);
												break 'session;
											}
											None => {}
										}
									}
								} else if let Some(cl) = parse_content_length(headers) {
									if body.len() >= cl {
										let pkt = &body[..cl];
										match dns_parse_first_a_or_cname(pkt, dns_id) {
											Some(DnsResolution::A(ip)) => {
												let _ = cmds.push(TlsCommand::Close { handle });
												return Some(ip);
											}
											Some(DnsResolution::Cname(name)) => {
												let _ = cmds.push(TlsCommand::Close { handle });
												cname_from_provider = Some(name);
												break 'session;
											}
											None => {}
										}
									}
								}
							}
						}
						TlsEvent::Closed { handle } => {
							if tls_handle != Some(handle) {
								continue;
							}
							let Some(hdr_end) = find_http_header_end(&plaintext) else { break };
							let headers = &plaintext[..hdr_end];
							let status = parse_http_status(headers).unwrap_or(0);
							if status != 200 {
								break;
							}
							let body = &plaintext[hdr_end..];
							let pkt: Vec<u8> = if is_chunked_encoding(headers) {
								match try_decode_chunked_body(body, max_bytes.saturating_sub(hdr_end)) {
									Ok(Some(decoded)) => decoded,
									_ => break,
								}
							} else if let Some(cl) = parse_content_length(headers) {
								if body.len() < cl {
									break;
								}
								body[..cl].to_vec()
							} else {
								body.to_vec()
							};
							match dns_parse_first_a_or_cname(&pkt, dns_id) {
								Some(DnsResolution::A(ip)) => return Some(ip),
								Some(DnsResolution::Cname(name)) => {
									cname_from_provider = Some(name);
									break 'session;
								}
								None => {}
							}
							break;
						}
						TlsEvent::Error { .. } => break,
						TlsEvent::TlsError { .. } => break,
					}
				}

				if !sent_connect {
					if cmds
						.push(TlsCommand::OpenTcpConnect {
							remote: vnet::EndpointV4 { addr: server_ip, port: DOH_PORT },
							server_name,
							cfg: cfg.clone(),
							roots: roots.clone(),
						})
						.is_ok()
					{
						sent_connect = true;
					}
				}

				if Instant::now() >= deadline {
					if let Some(handle) = tls_handle {
						let _ = cmds.push(TlsCommand::Close { handle });
					}
					break;
				}

				Timer::after(EmbassyDuration::from_millis(20)).await;
			}

			if let Some(name) = cname_from_provider.take() {
				next_cname = Some(name);
				break;
			}
		}

			let Some(name) = next_cname.take() else {
				return None;
			};
			if cname_left == 0 {
				return None;
			}
			cname_left = cname_left.saturating_sub(1);
			current_host = name;
		}
	}

	async fn resolve_ipv4_async(dev_idx: usize, host: &str, timeout_ms: u64) -> Option<[u8; 4]> {
		resolve_ipv4_via_doh_async_inner(dev_idx, host, timeout_ms, 4).await
	}

	async fn https_get_body_async(url: &ParsedUrl, timeout_ms: u64, max_bytes: usize) -> core::result::Result<Vec<u8>, i32> {
		https_get_body_async_inner(url, timeout_ms, max_bytes, 4).await
	}

	async fn https_get_body_async_inner(
		url: &ParsedUrl,
		timeout_ms: u64,
		max_bytes: usize,
		redirects_left: u8,
	) -> core::result::Result<Vec<u8>, i32> {
		use crate::v::net::Queue;
		use trueos_v::vnet as vnet;
		use crate::net::tls_socket::{register_tls_app_queues, TlsCommand, TlsEvent};
		use crate::net::tls::{TlsClientConfig, TlsRoots};
		use embassy_time::Instant;

		let mut current: ParsedUrl = url.clone();
		let mut redirects_remaining: u8 = redirects_left;

		'redirects: loop {
			let dev_indices: [usize; 1] = [0];
			let mut last_err: i32 = NET_ERR_TIMEOUT;
			let mut redirected: Option<ParsedUrl> = None;

			for dev_idx in dev_indices {
				let dns_timeout_ms = core::cmp::max(5_000, core::cmp::min(timeout_ms, 20_000));
				let Some(ip) = resolve_ipv4_async(dev_idx, current.host.as_str(), dns_timeout_ms).await else {
					last_err = NET_ERR_TIMEOUT_DNS;
					continue;
				};

			let seq = next_qjs_net_seq();
			let owner = leak_str(alloc::format!("async-https-{}@{}", seq, dev_idx));
			let cmds_name = leak_str(alloc::format!("{}-tls-cmd", owner));
			let evts_name = leak_str(alloc::format!("{}-tls-evt", owner));
			let cmds = Queue::new_leaked(cmds_name, 512);
			let events = Queue::new_leaked(evts_name, 512);
			register_tls_app_queues(owner, cmds, events);

			let mut tls_handle: Option<vnet::NetHandle> = None;
			let mut sent_connect = false;
			let mut http_sent = false;
			let mut plaintext: Vec<u8> = Vec::new();

			let start = Instant::now();
			let mut deadline = start + EmbassyDuration::from_millis(timeout_ms);
			let hard_deadline = start + EmbassyDuration::from_millis(core::cmp::max(timeout_ms, 180_000));

			let roots = TlsRoots::mozilla();
			let cfg = TlsClientConfig::new().with_alpn_protocols(&[b"http/1.1"]);
			let server_name = leak_str(current.host.clone());

			let mut redirect_to: Option<ParsedUrl> = None;

			'session: loop {
				for ev in events.drain(256) {
					match ev {
						TlsEvent::Opened { handle } => {
							tls_handle = Some(handle);
							deadline = Instant::now() + EmbassyDuration::from_millis(timeout_ms);
						}
						TlsEvent::Connected { handle } => {
							if tls_handle != Some(handle) {
								continue;
							}
							if !http_sent {
								let req = alloc::format!(
									"GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: TRUEOS async-fetch\r\nAccept: */*\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n",
									current.path,
									current.host
								);
								if cmds.push(TlsCommand::Send { handle, data: req.into_bytes() }).is_ok() {
									http_sent = true;
									deadline = Instant::now() + EmbassyDuration::from_millis(timeout_ms);
								}
							}
						}
						TlsEvent::Data { handle, data } => {
							if tls_handle != Some(handle) {
								continue;
							}
							deadline = Instant::now() + EmbassyDuration::from_millis(timeout_ms);
							if plaintext.len() < max_bytes {
								let room = max_bytes - plaintext.len();
								let take = data.len().min(room);
								plaintext.extend_from_slice(&data[..take]);
							}

							if let Some(hdr_end) = find_http_header_end(&plaintext) {
								let headers = &plaintext[..hdr_end];
								let status = parse_http_status(headers).unwrap_or(0);
								if status == 200 {
									let body = &plaintext[hdr_end..];
									if is_chunked_encoding(headers) {
										match try_decode_chunked_body(body, max_bytes.saturating_sub(hdr_end)) {
											Ok(Some(decoded)) => return Ok(decoded),
											Ok(None) => {}
											Err(e) => {
												last_err = e;
												break 'session;
											}
										}
									} else if let Some(cl) = parse_content_length(headers) {
										if body.len() >= cl {
											return Ok(body[..cl].to_vec());
										}
									}
								} else if (status == 301
									|| status == 302
									|| status == 303
									|| status == 307
									|| status == 308)
									&& redirects_remaining > 0
								{
									if let Some(loc) = parse_redirect_location(&current, headers) {
										if let Some(handle) = tls_handle {
											let _ = cmds.push(TlsCommand::Close { handle });
										}
										redirect_to = Some(parse_url(loc.as_str())?);
										break 'session;
									}
									last_err = NET_ERR_HTTP;
									break 'session;
								}
							}
						}
						TlsEvent::Closed { handle } => {
							if tls_handle != Some(handle) {
								continue;
							}
							let Some(hdr_end) = find_http_header_end(&plaintext) else {
								last_err = NET_ERR_HTTP;
								break 'session;
							};
							let headers = &plaintext[..hdr_end];
							let status = parse_http_status(headers).unwrap_or(0);
							if status != 200 {
								last_err = NET_ERR_HTTP;
								break 'session;
							}
							let body = &plaintext[hdr_end..];
							if is_chunked_encoding(headers) {
								match try_decode_chunked_body(body, max_bytes.saturating_sub(hdr_end)) {
									Ok(Some(decoded)) => return Ok(decoded),
									Ok(None) => {
										last_err = NET_ERR_TIMEOUT_BODY;
										break 'session;
									}
									Err(e) => {
										last_err = e;
										break 'session;
									}
								}
							} else if let Some(cl) = parse_content_length(headers) {
								if body.len() >= cl {
									return Ok(body[..cl].to_vec());
								}
								last_err = NET_ERR_TIMEOUT_BODY;
								break 'session;
							}
							return Ok(body.to_vec());
						}
						TlsEvent::Error { .. } => {
							last_err = NET_ERR_HTTP;
							break 'session;
						}
						TlsEvent::TlsError { .. } => {
							last_err = NET_ERR_TLS;
							break 'session;
						}
					}
				}

				if !sent_connect {
					if cmds
						.push(TlsCommand::OpenTcpConnect {
							remote: vnet::EndpointV4 { addr: ip, port: url.port },
							server_name,
							cfg: cfg.clone(),
							roots: roots.clone(),
						})
						.is_ok()
					{
						sent_connect = true;
					}
				}

				let now = Instant::now();
				if now >= deadline || now >= hard_deadline {
					let hdr_end = find_http_header_end(&plaintext);
					let have_hdr = hdr_end.is_some();
					let (status, body_len) = if let Some(hdr_end) = hdr_end {
						(
							parse_http_status(&plaintext[..hdr_end]).unwrap_or(0) as u32,
							plaintext.len().saturating_sub(hdr_end) as u32,
						)
					} else {
						(0, 0)
					};
					crate::log!(
						"async-fetch: timeout dev={} host={} port={} http_sent={} bytes={} have_hdr={} status={} body_bytes={}\n",
						dev_idx,
						url.host.as_str(),
						url.port,
						http_sent,
						plaintext.len(),
						have_hdr,
						status,
						body_len,
					);

					last_err = if tls_handle.is_none() || !http_sent {
						NET_ERR_TIMEOUT_CONNECT
					} else if !have_hdr {
						NET_ERR_TIMEOUT_HEADERS
					} else {
						NET_ERR_TIMEOUT_BODY
					};
					break 'session;
				}

				Timer::after(EmbassyDuration::from_millis(10)).await;
			}

			if let Some(parsed) = redirect_to.take() {
				redirected = Some(parsed);
				break;
			}
		}

			if let Some(parsed) = redirected.take() {
				if redirects_remaining == 0 {
					return Err(NET_ERR_HTTP);
				}
				redirects_remaining = redirects_remaining.saturating_sub(1);
				current = parsed;
				continue 'redirects;
			}

			return Err(last_err);
		}
	}

	/// Async variant of `net_fetch_https_body_blocking`.
	///
	/// This is intended for kernel async tasks (it does not poll the executor re-entrantly).
	pub async fn net_fetch_https_body_async(
		url_s: &str,
		timeout_ms: u64,
		max_bytes: usize,
	) -> core::result::Result<Vec<u8>, i32> {
		let parsed = match parse_url(url_s) {
			Ok(p) => p,
			Err(e) => return Err(e),
		};
		if !parsed.scheme_https {
			return Err(NET_ERR_BAD_URL);
		}
		https_get_body_async(&parsed, timeout_ms, max_bytes).await
	}

	/// Download the URL (currently expects HTTP/1.1 over TLS for https://) and write to `path`.
	///
	/// Behavior:
	/// - If `path` already exists, returns 0 (no-op).
	/// - Otherwise downloads and writes `path + ".tmp"`, then renames into place.
	///
	/// Notes:
	/// - Parent directories for `path` are created automatically (mkdir -p).
	/// - This function drives the Embassy executor while waiting for network events;
	///   it is intended to be called from non-async contexts.
	pub fn net_fetch_https_body_blocking(
		url_s: &str,
		timeout_ms: u64,
		max_bytes: usize,
	) -> core::result::Result<Vec<u8>, i32> {
		let parsed = match parse_url(url_s) {
			Ok(p) => p,
			Err(e) => return Err(e),
		};
		if !parsed.scheme_https {
			return Err(NET_ERR_BAD_URL);
		}
		https_get_body_blocking(&parsed, timeout_ms, max_bytes)
	}

	#[no_mangle]
	pub unsafe extern "C" fn trueos_cabi_net_fetch_to_file(
		url_ptr: *const u8,
		url_len: usize,
		path_ptr: *const u8,
		path_len: usize,
	) -> i32 {
		if url_ptr.is_null() || url_len == 0 || path_ptr.is_null() || path_len == 0 {
			return FS_ERR_BAD_PARAM;
		}
		let url_bytes = core::slice::from_raw_parts(url_ptr, url_len);
		let path_bytes = core::slice::from_raw_parts(path_ptr, path_len);
		let Ok(url_s) = core::str::from_utf8(url_bytes) else {
			return FS_ERR_BAD_UTF8;
		};
		let Ok(path) = core::str::from_utf8(path_bytes) else {
			return FS_ERR_BAD_UTF8;
		};

		// If already cached, done.
		match super::kfs::exists(path) {
			Ok(true) => return 0,
			Ok(false) => {}
			Err(e) => return fs_error_to_code(e),
		}

		// Ensure the cache directory exists before downloading.
		if let Some((parent, _name)) = path.rsplit_once('/') {
			if !parent.is_empty() {
				if let Err(e) = super::kfs::create_dir_all(parent) {
					return fs_error_to_code(e);
				}
			}
		}

		let body = match net_fetch_https_body_blocking(url_s, 30_000, 4 * 1024 * 1024) {
			Ok(b) => b,
			Err(e) => return e,
		};

		let tmp = alloc::format!("{}.tmp", path);
		if let Err(e) = super::kfs::write_file(tmp.as_str(), &body) {
			return fs_error_to_code(e);
		}
		if let Err(e) = super::kfs::rename(tmp.as_str(), path) {
			let _ = super::kfs::remove(tmp.as_str());
			return fs_error_to_code(e);
		}
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
