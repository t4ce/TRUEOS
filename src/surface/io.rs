
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
/// These intentionally expose the same underlying `disc::files::Fs` operations
/// used by the shell, but keep the logic in one place.
pub mod kfs {
	use super::Vec;
	use alloc::string::String;

	#[inline]
	pub fn read_file(path: &str) -> core::result::Result<Vec<u8>, crate::disc::files::FsError> {
		crate::disc::files::Fs::read_file(path)
	}

	#[inline]
	pub fn write_file(
		path: &str,
		data: &[u8],
	) -> core::result::Result<(), crate::disc::files::FsError> {
		crate::disc::files::Fs::write_file(path, data)
	}

	#[inline]
	pub fn rename(
		src: &str,
		dst: &str,
	) -> core::result::Result<(), crate::disc::files::FsError> {
		crate::disc::files::Fs::rename(src, dst)
	}

	#[inline]
	pub fn list_dir(path: &str) -> core::result::Result<String, crate::disc::files::FsError> {
		crate::disc::files::Fs::list_dir(path)
	}

	#[inline]
	pub fn remove(path: &str) -> core::result::Result<(), crate::disc::files::FsError> {
		crate::disc::files::Fs::remove(path)
	}

	#[inline]
	pub fn create_dir_all(
		path: &str,
	) -> core::result::Result<(), crate::disc::files::FsError> {
		crate::disc::files::Fs::create_dir_all(path)
	}

	/// Read a destination file for an append operation.
	///
	/// Mirrors the shell's historical behavior: a missing/unopenable destination
	/// is treated as an empty file so `append` can create it.
	pub fn read_file_for_append(
		path: &str,
	) -> core::result::Result<Vec<u8>, crate::disc::files::FsError> {
		match crate::disc::files::Fs::read_file(path) {
			Ok(bytes) => Ok(bytes),
			Err(crate::disc::files::FsError::Read(
				crate::disc::files::UsbFsReadError::OpenFailed,
			)) => Ok(Vec::new()),
			Err(e) => Err(e),
		}
	}

	/// Append `src` bytes into the file at `dst_path`, creating the file if needed.
	pub fn append_into_file(
		dst_path: &str,
		src: &[u8],
	) -> core::result::Result<(), crate::disc::files::FsError> {
		if src.is_empty() {
			return Ok(());
		}
		let mut dst = read_file_for_append(dst_path)?;
		dst.extend_from_slice(src);
		write_file(dst_path, dst.as_slice())
	}
}

/// Shell-facing helpers for the `out` / `in` / `io` commands.
///
/// This keeps the filesystem+matrix job plumbing in one place (the surface I/O
/// layer), while the shell remains responsible for user interaction (printing
/// usage/errors, refreshing the prompt UI, etc).
pub mod shellcmd {
	use super::kfs;
	use alloc::vec::Vec;
	use embassy_executor::Spawner;
	use heapless::String;

	#[derive(Clone, Copy, Debug, Eq, PartialEq)]
	pub enum StartError {
		MatrixFull,
		SpawnFailed,
	}

	#[derive(Clone, Copy, Debug, Eq, PartialEq)]
	pub enum IoOutcome {
		ImmediateOk,
		ImmediateNoop,
		ImmediateErrSrcSlotNotFound,
		ImmediateErrDstSlotNotFound,
		Started(u8),
		MatrixFull,
		SpawnFailed,
	}

	#[derive(Clone, Copy, Debug, Eq, PartialEq)]
	pub enum PrintAction {
		None,
		One(&'static str),
		Two(&'static str, &'static str),
		Started(&'static str, u8),
	}

	#[derive(Clone, Copy, Debug, Eq, PartialEq)]
	pub struct CmdResponse {
		pub print: PrintAction,
		pub refresh_symbols: bool,
	}

	impl CmdResponse {
		#[inline]
		pub const fn none() -> Self {
			Self {
				print: PrintAction::None,
				refresh_symbols: false,
			}
		}

		#[inline]
		pub const fn one(s: &'static str) -> Self {
			Self {
				print: PrintAction::One(s),
				refresh_symbols: false,
			}
		}

		#[inline]
		pub const fn two(a: &'static str, b: &'static str) -> Self {
			Self {
				print: PrintAction::Two(a, b),
				refresh_symbols: false,
			}
		}

		#[inline]
		pub const fn started(label: &'static str, slot: u8) -> Self {
			Self {
				print: PrintAction::Started(label, slot),
				refresh_symbols: true,
			}
		}
	}

	#[inline]
	pub fn parse_slot_ref(s: &str) -> Option<u8> {
		let t = s.trim();
		let n = t.strip_prefix('§')?;
		if n.is_empty() {
			return None;
		}
		if !n.as_bytes().iter().all(|b| b.is_ascii_digit()) {
			return None;
		}
		let id = n.parse::<u8>().ok()?;
		if id == 0 {
			return None;
		}
		Some(id - 1)
	}

	fn title_with_prefix(prefix: &str, arg: &str) -> String<{ crate::matrix::TITLE_LEN }> {
		let mut title: String<{ crate::matrix::TITLE_LEN }> = String::new();
		let _ = title.push_str(prefix);
		for ch in arg.chars() {
			if title.push(ch).is_err() {
				break;
			}
		}
		title
	}

	fn path_160(path: &str) -> String<160> {
		let mut p: String<160> = String::new();
		for ch in path.chars() {
			if p.push(ch).is_err() {
				break;
			}
		}
		p
	}

	fn fill_slot_from_blob(slot_id: u8, bytes: Vec<u8>) {
		let _ = crate::matrix::set_blob_owned_with_preview(slot_id, bytes);
	}

	enum IoArg {
		Slot(u8),
		Path(String<160>),
	}

	fn read_dst_file_for_append(path: &str) -> Result<Vec<u8>, crate::disc::files::FsError> {
		kfs::read_file_for_append(path)
	}

	#[embassy_executor::task]
	async fn io_matrix_job(slot_id: u8, src: IoArg, dst: IoArg) {
		let src_bytes = match src {
			IoArg::Slot(id) => crate::matrix::blob_snapshot(id).unwrap_or_default(),
			IoArg::Path(path) => match kfs::read_file(path.as_str()) {
				Ok(bytes) => bytes,
				Err(e) => {
					crate::matrix::clear_lines(slot_id);
					crate::matrix::push_line(slot_id, "io: read src failed");
					crate::matrix::push_line(slot_id, "(see kernel log for details)");
					crate::log!("io: read_file src '{}' failed: {:?}\n", path.as_str(), e);
					crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
					return;
				}
			},
		};

		if src_bytes.is_empty() {
			crate::matrix::clear_lines(slot_id);
			crate::matrix::push_line(slot_id, "io: ok (noop)");
			crate::matrix::set_state(slot_id, crate::matrix::SlotState::Done);
			return;
		}

		let mut dst_bytes = match &dst {
			IoArg::Slot(id) => match crate::matrix::blob_snapshot(*id) {
				Some(bytes) => bytes,
				None => {
					crate::matrix::clear_lines(slot_id);
					crate::matrix::push_line(slot_id, "io: dst slot not found");
					crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
					return;
				}
			},
			IoArg::Path(path) => match read_dst_file_for_append(path.as_str()) {
				Ok(bytes) => bytes,
				Err(e) => {
					crate::matrix::clear_lines(slot_id);
					crate::matrix::push_line(slot_id, "io: read dst failed");
					crate::matrix::push_line(slot_id, "(see kernel log for details)");
					crate::log!("io: read_file dst '{}' failed: {:?}\n", path.as_str(), e);
					crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
					return;
				}
			},
		};

		dst_bytes.extend_from_slice(src_bytes.as_slice());

		match dst {
			IoArg::Slot(id) => {
				let _ = crate::matrix::set_blob_owned_with_preview(id, dst_bytes);
				crate::matrix::clear_lines(slot_id);
				crate::matrix::push_line(slot_id, "io: ok");
				crate::matrix::set_state(slot_id, crate::matrix::SlotState::Done);
			}
			IoArg::Path(path) => match kfs::write_file(path.as_str(), dst_bytes.as_slice()) {
				Ok(()) => {
					crate::matrix::clear_lines(slot_id);
					crate::matrix::push_line(slot_id, "io: ok");
					crate::matrix::set_state(slot_id, crate::matrix::SlotState::Done);
				}
				Err(e) => {
					crate::matrix::clear_lines(slot_id);
					crate::matrix::push_line(slot_id, "io: write dst failed");
					crate::matrix::push_line(slot_id, "(see kernel log for details)");
					crate::log!("io: write_file dst '{}' failed: {:?}\n", path.as_str(), e);
					crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
				}
			},
		}
	}

	#[embassy_executor::task]
	async fn out_matrix_job(slot_id: u8, path: String<160>) {
		match kfs::read_file(path.as_str()) {
			Ok(bytes) => {
				fill_slot_from_blob(slot_id, bytes);
				crate::matrix::set_state(slot_id, crate::matrix::SlotState::Done);
			}
			Err(e) => {
				crate::matrix::clear_lines(slot_id);
				crate::matrix::push_line(slot_id, "out: read_file failed");
				crate::matrix::push_line(slot_id, "(see kernel log for details)");
				crate::log!("out: read_file '{}' failed: {:?}\n", path.as_str(), e);
				crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
			}
		}
	}

	#[embassy_executor::task]
	async fn in_matrix_job(slot_id: u8, src_slot: u8, path: String<160>) {
		let snapshot = crate::matrix::blob_snapshot(src_slot).unwrap_or_default();
		if snapshot.is_empty() {
			crate::matrix::clear_lines(slot_id);
			crate::matrix::push_line(slot_id, "in: source slot empty");
			crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
			return;
		}

		match kfs::write_file(path.as_str(), snapshot.as_slice()) {
			Ok(()) => {
				crate::matrix::clear_lines(slot_id);
				crate::matrix::push_line(slot_id, "in: ok");
				crate::matrix::set_state(slot_id, crate::matrix::SlotState::Done);
			}
			Err(e) => {
				crate::matrix::clear_lines(slot_id);
				crate::matrix::push_line(slot_id, "in: write_file failed");
				crate::matrix::push_line(slot_id, "(see kernel log for details)");
				crate::log!("in: write_file '{}' failed: {:?}\n", path.as_str(), e);
				crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
			}
		}
	}

	pub fn start_out(spawner: &Spawner, path: &str) -> Result<u8, StartError> {
		let title = title_with_prefix("out ", path);
		let Some(slot) = crate::matrix::alloc_slot(title.as_str()) else {
			return Err(StartError::MatrixFull);
		};
		let p = path_160(path);
		spawner.spawn(out_matrix_job(slot, p)).map_err(|_| StartError::SpawnFailed)?;
		Ok(slot)
	}

	pub fn start_in(spawner: &Spawner, src_slot: u8, path: &str) -> Result<u8, StartError> {
		let title = title_with_prefix("in ", path);
		let Some(slot) = crate::matrix::alloc_slot(title.as_str()) else {
			return Err(StartError::MatrixFull);
		};
		let p = path_160(path);
		spawner
			.spawn(in_matrix_job(slot, src_slot, p))
			.map_err(|_| StartError::SpawnFailed)?;
		Ok(slot)
	}

	pub fn exec_io(spawner: &Spawner, src_token: &str, dst_token: &str) -> IoOutcome {
		let src_slot = parse_slot_ref(src_token);
		let dst_slot = parse_slot_ref(dst_token);

		// Fast path: slot -> slot append is immediate (no filesystem I/O).
		if let (Some(src_id), Some(dst_id)) = (src_slot, dst_slot) {
			let Some(src_bytes) = crate::matrix::blob_snapshot(src_id) else {
				return IoOutcome::ImmediateErrSrcSlotNotFound;
			};
			if src_bytes.is_empty() {
				return IoOutcome::ImmediateNoop;
			}
			let Some(mut dst_bytes) = crate::matrix::blob_snapshot(dst_id) else {
				return IoOutcome::ImmediateErrDstSlotNotFound;
			};
			dst_bytes.extend_from_slice(src_bytes.as_slice());
			let _ = crate::matrix::set_blob_owned_with_preview(dst_id, dst_bytes);
			return IoOutcome::ImmediateOk;
		}

		let title = title_with_prefix("io ", dst_token);
		let src = match src_slot {
			Some(id) => IoArg::Slot(id),
			None => IoArg::Path(path_160(src_token)),
		};
		let dst = match dst_slot {
			Some(id) => IoArg::Slot(id),
			None => IoArg::Path(path_160(dst_token)),
		};

		let Some(slot) = crate::matrix::alloc_slot(title.as_str()) else {
			return IoOutcome::MatrixFull;
		};
		if spawner.spawn(io_matrix_job(slot, src, dst)).is_err() {
			return IoOutcome::SpawnFailed;
		}
		IoOutcome::Started(slot)
	}

	pub fn handle_out(spawner: &Spawner, args: &str) -> CmdResponse {
		let mut parts = args.split_whitespace();
		let a = parts.next().unwrap_or("");
		let extra = parts.next().is_some();
		if a.is_empty() || extra {
			return CmdResponse::one("out: usage out <path>\r\n");
		}
		if parse_slot_ref(a).is_some() {
			return CmdResponse::two(
				"out: arg must be a path\r\n",
				"out: usage out <path>\r\n",
			);
		}

		match start_out(spawner, a) {
			Ok(slot) => CmdResponse::started("out", slot),
			Err(StartError::MatrixFull) => CmdResponse::one("out: matrix full\r\n"),
			Err(StartError::SpawnFailed) => CmdResponse::one("out: spawn failed\r\n"),
		}
	}

	pub fn handle_in(spawner: &Spawner, args: &str) -> CmdResponse {
		let mut parts = args.split_whitespace();
		let a = parts.next().unwrap_or("");
		let b = parts.next().unwrap_or("");
		let extra = parts.next().is_some();
		if a.is_empty() || b.is_empty() || extra {
			return CmdResponse::one("in: usage in §N <path>\r\n");
		}
		let Some(src_slot) = parse_slot_ref(a) else {
			return CmdResponse::two(
				"in: first arg must be a §N slot (no spaces)\r\n",
				"in: usage in §N <path>\r\n",
			);
		};
		if parse_slot_ref(b).is_some() {
			return CmdResponse::two(
				"in: second arg must be a path\r\n",
				"in: usage in §N <path>\r\n",
			);
		}

		match start_in(spawner, src_slot, b) {
			Ok(slot) => CmdResponse::started("in", slot),
			Err(StartError::MatrixFull) => CmdResponse::one("in: matrix full\r\n"),
			Err(StartError::SpawnFailed) => CmdResponse::one("in: spawn failed\r\n"),
		}
	}

	pub fn handle_io(spawner: &Spawner, args: &str) -> CmdResponse {
		let mut parts = args.split_whitespace();
		let a = parts.next().unwrap_or("");
		let b = parts.next().unwrap_or("");
		let extra = parts.next().is_some();
		if a.is_empty() || b.is_empty() || extra {
			return CmdResponse::two(
				"io: usage io <src> <dst>\r\n",
				"io: appends <src> into <dst>\r\n",
			);
		}

		match exec_io(spawner, a, b) {
			IoOutcome::ImmediateOk => CmdResponse {
				print: PrintAction::One("io: ok\r\n"),
				refresh_symbols: true,
			},
			IoOutcome::ImmediateNoop => CmdResponse::one("io: ok (noop)\r\n"),
			IoOutcome::ImmediateErrSrcSlotNotFound => CmdResponse::one("io: src slot not found\r\n"),
			IoOutcome::ImmediateErrDstSlotNotFound => CmdResponse::one("io: dst slot not found\r\n"),
			IoOutcome::Started(slot) => CmdResponse::started("io", slot),
			IoOutcome::MatrixFull => CmdResponse::one("io: matrix full\r\n"),
			IoOutcome::SpawnFailed => CmdResponse::one("io: spawn failed\r\n"),
		}
	}
}

/// Console routing + C ABI entrypoints used by embedded C code (QuickJS etc).
pub mod cabi {
	use alloc::vec::Vec;
	use alloc::boxed::Box;
	use alloc::string::{String, ToString};

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

	const NET_ERR_BAD_URL: i32 = -10;
	const NET_ERR_TIMEOUT: i32 = -11;
	const NET_ERR_HTTP: i32 = -12;
	const NET_ERR_TLS: i32 = -13;

	// More granular timeout diagnostics (same “class” as NET_ERR_TIMEOUT).
	// These intentionally live far from the base codes to avoid collisions.
	const NET_ERR_TIMEOUT_DNS: i32 = -111;
	const NET_ERR_TIMEOUT_CONNECT: i32 = -112;
	const NET_ERR_TIMEOUT_HEADERS: i32 = -113;
	const NET_ERR_TIMEOUT_BODY: i32 = -114;

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

		match super::kfs::write_file(path, data) {
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

		match super::kfs::rename(src, dst) {
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

		let listing = match super::kfs::list_dir(path) {
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

		match super::kfs::remove(path) {
			Ok(()) => 0,
			Err(_) => FS_ERR_IO,
		}
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
	const DNS_PORT: u16 = 53;

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

	fn dns_parse_first_a(pkt: &[u8], want_id: u16) -> Option<[u8; 4]> {
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
			if typ == 1 && class == 1 && rdlen == 4 {
				return Some([pkt[idx], pkt[idx + 1], pkt[idx + 2], pkt[idx + 3]]);
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

	fn resolve_ipv4_blocking(dev_idx: usize, host: &str, timeout_ms: u64) -> Option<[u8; 4]> {
		use crate::net::adapter::{register_app_queues, NetCommand, NetEndpoint, NetEvent, NetHandle, NetQueue, SocketKind};

		let dns_id: u16 = 0xEA00u16.wrapping_add(dev_idx as u16);
		let owner = leak_str(alloc::format!("qjs-dns@{}", dev_idx));
		let cmd_name = leak_str(alloc::format!("{}-cmd", owner));
		let evt_name = leak_str(alloc::format!("{}-evt", owner));
		let cmds = NetQueue::new_leaked(cmd_name, 64);
		let events = NetQueue::new_leaked(evt_name, 64);
		register_app_queues(owner, cmds, events);

		let local_port: u16 = 54000u16.wrapping_add(dev_idx as u16);
		let _ = cmds.push(NetCommand::OpenUdp { port: local_port });

		let start = embassy_time_driver::now() as u64;
		let deadline = start.saturating_add(timeout_ms.saturating_mul(embassy_time_driver::TICK_HZ as u64 / 1000).max(1));
		let mut udp: Option<NetHandle> = None;
		let mut sent = false;

		loop {
			for ev in events.drain(32) {
				match ev {
					NetEvent::Opened { handle, kind } => {
						if kind == SocketKind::Udp {
							udp = Some(handle);
						}
					}
					NetEvent::UdpPacket { handle, from, data } => {
						if udp != Some(handle) {
							continue;
						}
						if from.port == DNS_PORT && from.addr == SLIRP_DNS_IP {
							if let Some(ip) = dns_parse_first_a(&data, dns_id) {
								let _ = cmds.push(NetCommand::Close { handle });
								return Some(ip);
							}
						}
					}
					_ => {}
				}
			}

			if !sent {
				if let Some(handle) = udp {
					let _ = cmds.push(NetCommand::SendUdp {
						handle,
						remote: NetEndpoint {
							addr: SLIRP_DNS_IP,
							port: DNS_PORT,
						},
						data: dns_query(dns_id, host, 1),
					});
					sent = true;
				}
			}

			let now = embassy_time_driver::now() as u64;
			if now >= deadline {
				if let Some(handle) = udp {
					let _ = cmds.push(NetCommand::Close { handle });
				}
				return None;
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
		use crate::net::adapter::{NetEndpoint, NetQueue};
		use crate::net::tls_socket::{register_tls_app_queues, TlsCommand, TlsEvent};
		use crate::tls::{TlsClientConfig, TlsRoots};

		let dev_count = crate::net::device_count().max(1);
		let mut last_err: i32 = NET_ERR_TIMEOUT;

		for dev_idx in 0..dev_count {
			let Some(ip) = resolve_ipv4_blocking(dev_idx, url.host.as_str(), 1500) else {
				last_err = NET_ERR_TIMEOUT_DNS;
				continue;
			};

			let owner = leak_str(alloc::format!("qjs-https@{}", dev_idx));
			let cmds_name = leak_str(alloc::format!("{}-tls-cmd", owner));
			let evts_name = leak_str(alloc::format!("{}-tls-evt", owner));
			let cmds = NetQueue::new_leaked(cmds_name, 128);
			let events = NetQueue::new_leaked(evts_name, 128);
			register_tls_app_queues(owner, cmds, events);

			let mut tls_handle: Option<crate::net::adapter::NetHandle> = None;
			let mut sent_connect = false;
			let mut http_sent = false;
			let mut plaintext: Vec<u8> = Vec::new();
			let start = embassy_time_driver::now() as u64;
			let deadline = start.saturating_add(timeout_ms.saturating_mul(embassy_time_driver::TICK_HZ as u64 / 1000).max(1));

			let roots = TlsRoots::mozilla();
			let cfg = TlsClientConfig::new().with_alpn_protocols(&[b"http/1.1"]);
			let server_name = leak_str(url.host.clone());

			loop {
				for ev in events.drain(32) {
					match ev {
						TlsEvent::Opened { handle } => {
							tls_handle = Some(handle);
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
								let _ = cmds.push(TlsCommand::Send {
									handle,
									data: req.into_bytes(),
								});
								http_sent = true;
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
												break;
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
									break;
								}
							}
						}
						TlsEvent::Closed { handle } => {
							if tls_handle != Some(handle) {
								continue;
							}
							// Parse status and return body.
							let Some(hdr_end) = find_http_header_end(&plaintext) else {
								last_err = NET_ERR_HTTP;
								break;
							};
							let status = parse_http_status(&plaintext).unwrap_or(0);
							if status != 200 {
								last_err = NET_ERR_HTTP;
								break;
							}
							let body = plaintext.split_off(hdr_end);
							return Ok(body);
						}
						TlsEvent::Error { .. } => {
							last_err = NET_ERR_HTTP;
							break;
						}
						TlsEvent::TlsError { .. } => {
							last_err = NET_ERR_TLS;
							break;
						}
					}
				}

				if !sent_connect {
					sent_connect = true;
					let _ = cmds.push(TlsCommand::OpenTcpConnect {
						remote: NetEndpoint { addr: ip, port: url.port },
						server_name,
						cfg: cfg.clone(),
						roots: roots.clone(),
					});
				}

				let now = embassy_time_driver::now() as u64;
				if now >= deadline {
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
					break;
				}
				poll_executor_for_progress();
			}
		}

		Err(last_err)
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
		if super::kfs::read_file(path).is_ok() {
			return 0;
		}

		// Ensure the cache directory exists before downloading.
		if let Some((parent, _name)) = path.rsplit_once('/') {
			if !parent.is_empty() {
				if super::kfs::create_dir_all(parent).is_err() {
					return FS_ERR_IO;
				}
			}
		}

		let parsed = match parse_url(url_s) {
			Ok(p) => p,
			Err(e) => return e,
		};
		if !parsed.scheme_https {
			// TODO: support plaintext http:// if desired.
			return NET_ERR_BAD_URL;
		}

		let body = match https_get_body_blocking(&parsed, 30_000, 2 * 1024 * 1024) {
			Ok(b) => b,
			Err(e) => return e,
		};

		let tmp = alloc::format!("{}.tmp", path);
		if super::kfs::write_file(tmp.as_str(), &body).is_err() {
			return FS_ERR_IO;
		}
		if super::kfs::rename(tmp.as_str(), path).is_err() {
			let _ = super::kfs::remove(tmp.as_str());
			return FS_ERR_IO;
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
