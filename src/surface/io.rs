
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
