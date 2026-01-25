
extern crate alloc;

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

/// Console routing + C ABI entrypoints used by embedded C code (QuickJS etc).
pub mod cabi {
	use alloc::vec::Vec;

	#[repr(u32)]
	#[derive(Clone, Copy, Debug, Eq, PartialEq)]
	pub enum CStream {
		Stdout = 1,
		Stderr = 2,
	}

	const FS_ERR_BAD_UTF8: i32 = -1;
	const FS_ERR_IO: i32 = -2;
	const FS_ERR_NO_SPACE: i32 = -3;
	const FS_ERR_BAD_PARAM: i32 = -4;

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

		let bytes: Vec<u8> = match crate::disc::files::Fs::read_file(path) {
			Ok(v) => v,
			Err(_) => return FS_ERR_IO as isize,
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

		match crate::disc::files::Fs::write_file(path, data) {
			Ok(()) => 0,
			Err(_) => FS_ERR_IO,
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

		match crate::disc::files::Fs::rename(src, dst) {
			Ok(()) => 0,
			Err(_) => FS_ERR_IO,
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

		let listing = match crate::disc::files::Fs::list_dir(path) {
			Ok(v) => v,
			Err(_) => return FS_ERR_IO as isize,
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

		match crate::disc::files::Fs::remove(path) {
			Ok(()) => 0,
			Err(_) => FS_ERR_IO,
		}
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
