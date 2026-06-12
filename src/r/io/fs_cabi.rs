extern crate alloc;

include!("../cabi_codes.rs");

use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::String;
use core::sync::atomic::{AtomicU32, Ordering};

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConsoleStream {
    Out = 1,
    Err = 2,
}

struct StreamTextBuffers {
    stdout: String,
    stderr: String,
}

impl StreamTextBuffers {
    fn new() -> Self {
        Self {
            stdout: String::new(),
            stderr: String::new(),
        }
    }

    fn pending_mut(&mut self, stream: ConsoleStream) -> &mut String {
        match stream {
            ConsoleStream::Out => &mut self.stdout,
            ConsoleStream::Err => &mut self.stderr,
        }
    }
}

static CABI_TEXT_BUFFERS: spin::Mutex<BTreeMap<u32, StreamTextBuffers>> =
    spin::Mutex::new(BTreeMap::new());

fn current_cpu_key() -> u32 {
    super::runtime_context_key()
}

fn level_from_tag(level: &str) -> Option<log::Level> {
    match level {
        "ERROR" => Some(log::Level::Error),
        "WARN" => Some(log::Level::Warn),
        "INFO" => Some(log::Level::Info),
        "DEBUG" => Some(log::Level::Debug),
        "TRACE" => Some(log::Level::Trace),
        _ => None,
    }
}

fn purpose_for_level(level: log::Level) -> &'static str {
    match level {
        log::Level::Error => "error",
        log::Level::Warn => "warn",
        log::Level::Info => "info",
        log::Level::Debug => "debug",
        log::Level::Trace => "trace",
    }
}

fn parse_structured_guest_log(line: &str) -> Option<(&str, log::Level, &str)> {
    let rest = line.strip_prefix('[')?;
    let end = rest.find(']')?;
    let header = &rest[..end];
    let message = rest[end + 1..].trim_start();
    let split = header.rfind(':')?;
    let source = &header[..split];
    let level = level_from_tag(&header[split + 1..])?;
    if source.is_empty() {
        return None;
    }
    Some((source, level, message))
}

fn emit_guest_log_line(source: &str, level: log::Level, message: &str) {
    if !crate::logflag::blueprint_log_enabled(level) {
        return;
    }
    crate::globalog::log_with_purpose(
        Some(purpose_for_level(level)),
        format_args!("{}: {}\n", source, message),
    );
}

fn emit_plain_stream_line(_stream: ConsoleStream, line: &str) {
    crate::globalog::log(format_args!("{}\n", line));
}

fn emit_console_stream_line(stream: ConsoleStream, line: &str) {
    let Some(target) = super::env::console_target() else {
        return;
    };
    match stream {
        ConsoleStream::Out => crate::shell2::print_matrix_target_line(&target, line),
        ConsoleStream::Err => crate::shell2::print_matrix_target_line(
            &target,
            alloc::format!("error: {}", line).as_str(),
        ),
    }
}

fn emit_vm_console_stream_line(stream: ConsoleStream, line: &str) {
    match stream {
        ConsoleStream::Out => {
            crate::hv::log_active_blueprint_console_line(format_args!("guest: {}", line))
        }
        ConsoleStream::Err => {
            crate::hv::log_active_blueprint_console_line(format_args!("guest error: {}", line))
        }
    }
}

fn process_text_stream_impl(
    stream: ConsoleStream,
    text: &str,
    mut emit_line: impl FnMut(ConsoleStream, &str),
) {
    let cpu = current_cpu_key();
    let mut lines = VecDeque::new();

    {
        let mut buffers = CABI_TEXT_BUFFERS.lock();
        let pending = buffers
            .entry(cpu)
            .or_insert_with(StreamTextBuffers::new)
            .pending_mut(stream);
        pending.push_str(text);

        while let Some(newline_idx) = pending.find('\n') {
            let mut line = String::from(&pending[..newline_idx]);
            if line.ends_with('\r') {
                line.pop();
            }
            lines.push_back(line);
            pending.drain(..=newline_idx);
        }
    }

    for line in lines {
        emit_line(stream, line.as_str());
    }
}

fn process_vm_text_stream(stream: ConsoleStream, text: &str) {
    process_text_stream_impl(stream, text, |stream, line| {
        emit_console_stream_line(stream, line);
        emit_vm_console_stream_line(stream, line);
    });
}

fn process_text_stream(stream: ConsoleStream, text: &str) {
    process_text_stream_impl(stream, text, |stream, line| {
        emit_console_stream_line(stream, line);
        if let Some((source, level, message)) = parse_structured_guest_log(line) {
            emit_guest_log_line(source, level, message);
        } else {
            emit_plain_stream_line(stream, line);
        }
    });
}

#[inline]
pub fn write_console_bytes(stream: ConsoleStream, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }

    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        if let Ok(text) = core::str::from_utf8(bytes) {
            process_vm_text_stream(stream, text);
        } else {
            crate::hv::log_active_blueprint_console_line(format_args!(
                "guest: non-utf8 stream bytes={}",
                bytes.len()
            ));
        }
        return;
    }

    match core::str::from_utf8(bytes) {
        Ok(text) => process_text_stream(stream, text),
        Err(_) => {
            let text = alloc::string::String::from_utf8_lossy(bytes);
            process_text_stream(stream, text.as_ref());
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_write(stream: u32, bytes: *const u8, len: usize) {
    if bytes.is_null() || len == 0 {
        return;
    }

    let stream = match stream {
        1 => ConsoleStream::Out,
        2 => ConsoleStream::Err,
        _ => ConsoleStream::Out,
    };
    let slice = unsafe { core::slice::from_raw_parts(bytes, len) };
    write_console_bytes(stream, slice);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_write_cstr(stream: u32, cstr: *const u8) {
    if cstr.is_null() {
        return;
    }
    let mut len = 0usize;
    while unsafe { *cstr.add(len) } != 0 {
        len = len.saturating_add(1);
    }
    unsafe {
        trueos_cabi_write(stream, cstr, len);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_poll_once() {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        crate::hv::vmcall::guest_yield();
        return;
    }
    crate::wait::spin_step();
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_sleep_ms(ms: u64) {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        trueos_vm::vmcall::sleep_ms(ms);
        return;
    }
    let timeout = ms.max(1);
    let _ = crate::wait::spin_until_timeout(timeout, || false);
}

#[inline]
fn fs_error_to_code(err: super::kfs::FsError) -> i32 {
    use super::kfs::FsError;
    match err {
        FsError::NoRoot => FS_ERR_NOT_FOUND,
        FsError::BadPath => FS_ERR_BAD_PATH,
        FsError::NoSpace => FS_ERR_NO_SPACE,
        FsError::NotFound => FS_ERR_NOT_FOUND,
        FsError::AlreadyExists => FS_ERR_ALREADY_EXISTS,
        FsError::Device(e) => match e {
            crate::disc::block::Error::InvalidParam => FS_ERR_BAD_PARAM,
            crate::disc::block::Error::OutOfBounds => FS_ERR_BAD_PARAM,
            crate::disc::block::Error::NotReady => FS_ERR_NOT_FOUND,
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
fn fs_rc_name(rc: i32) -> &'static str {
    core::str::from_utf8(cabi_rc_name(rc)).unwrap_or("UNKNOWN")
}

fn log_fs_cabi_path_fail(op: &str, raw: &str, resolved: Option<&str>, detail: &str, rc: i32) {
    if rc >= 0 {
        return;
    }
    match resolved {
        Some(resolved) => crate::log!(
            "fs-cabi: {op} failed raw={raw} resolved={resolved} {detail} rc={rc} {}\n",
            fs_rc_name(rc)
        ),
        None => crate::log!(
            "fs-cabi: {op} failed raw={raw} resolved=<none> {detail} rc={rc} {}\n",
            fs_rc_name(rc)
        ),
    }
}

fn log_fs_cabi_handle_fail(op: &str, handle: u32, detail: &str, rc: i32) {
    if rc >= 0 {
        return;
    }
    crate::log!("fs-cabi: {op} failed handle={handle} {detail} rc={rc} {}\n", fs_rc_name(rc));
}

#[inline]
fn vmcall_signed(data: u64) -> isize {
    (data as i64) as isize
}

#[inline]
fn vmcall_signed_i32(data: u64) -> i32 {
    (data as i64) as i32
}

pub(crate) fn fs_read_file_len_host(path: &str) -> isize {
    if path.len() > QJS_ASYNC_FS_MAX_PATH {
        log_fs_cabi_path_fail(
            "read_len",
            path,
            None,
            "reason=raw-path-too-large",
            FS_ERR_TOO_LARGE,
        );
        return FS_ERR_TOO_LARGE as isize;
    }
    let Some(path) = super::env::resolve_fs_path(path, false) else {
        log_fs_cabi_path_fail("read_len", path, None, "reason=resolve-failed", FS_ERR_BAD_PATH);
        return FS_ERR_BAD_PATH as isize;
    };
    if path.len() > QJS_ASYNC_FS_MAX_PATH {
        log_fs_cabi_path_fail(
            "read_len",
            path.as_str(),
            Some(path.as_str()),
            "reason=resolved-path-too-large",
            FS_ERR_TOO_LARGE,
        );
        return FS_ERR_TOO_LARGE as isize;
    }
    match super::kfs::read_file_len(path.as_str()) {
        Ok(len) => len as isize,
        Err(e) => {
            let rc = fs_error_to_code(e);
            log_fs_cabi_path_fail("read_len", path.as_str(), Some(path.as_str()), "", rc);
            rc as isize
        }
    }
}

pub(crate) fn fs_read_file_chunk_host(path: &str, offset: usize, out: &mut [u8]) -> isize {
    if path.len() > QJS_ASYNC_FS_MAX_PATH {
        log_fs_cabi_path_fail(
            "read_chunk",
            path,
            None,
            "reason=raw-path-too-large",
            FS_ERR_TOO_LARGE,
        );
        return FS_ERR_TOO_LARGE as isize;
    }
    let Some(path) = super::env::resolve_fs_path(path, false) else {
        log_fs_cabi_path_fail("read_chunk", path, None, "reason=resolve-failed", FS_ERR_BAD_PATH);
        return FS_ERR_BAD_PATH as isize;
    };
    if path.len() > QJS_ASYNC_FS_MAX_PATH {
        log_fs_cabi_path_fail(
            "read_chunk",
            path.as_str(),
            Some(path.as_str()),
            "reason=resolved-path-too-large",
            FS_ERR_TOO_LARGE,
        );
        return FS_ERR_TOO_LARGE as isize;
    }
    match super::kfs::read_file(path.as_str()) {
        Ok(bytes) => {
            if offset >= bytes.len() || out.is_empty() {
                return 0;
            }
            let n = core::cmp::min(out.len(), bytes.len() - offset);
            out[..n].copy_from_slice(&bytes[offset..offset + n]);
            n as isize
        }
        Err(e) => {
            let rc = fs_error_to_code(e);
            log_fs_cabi_path_fail("read_chunk", path.as_str(), Some(path.as_str()), "", rc);
            rc as isize
        }
    }
}

pub(crate) fn fs_write_begin_host(path: &str, total_len: u64) -> i64 {
    if path.len() > QJS_ASYNC_FS_MAX_PATH {
        log_fs_cabi_path_fail(
            "write_begin",
            path,
            None,
            "reason=raw-path-too-large",
            FS_ERR_TOO_LARGE,
        );
        return FS_ERR_TOO_LARGE as i64;
    }
    let raw = path;
    let Some(path) = super::env::resolve_fs_path(path, false) else {
        log_fs_cabi_path_fail("write_begin", raw, None, "reason=resolve-failed", FS_ERR_BAD_PATH);
        return FS_ERR_BAD_PATH as i64;
    };
    if path.len() > QJS_ASYNC_FS_MAX_PATH {
        log_fs_cabi_path_fail(
            "write_begin",
            raw,
            Some(path.as_str()),
            "reason=resolved-path-too-large",
            FS_ERR_TOO_LARGE,
        );
        return FS_ERR_TOO_LARGE as i64;
    }
    match super::kfs::write_file_begin(path.as_str(), total_len) {
        Ok(h) => h as i64,
        Err(e) => {
            let rc = fs_error_to_code(e);
            log_fs_cabi_path_fail(
                "write_begin",
                raw,
                Some(path.as_str()),
                alloc::format!("len={total_len}").as_str(),
                rc,
            );
            rc as i64
        }
    }
}

pub(crate) fn fs_write_chunk_host(handle: u32, data: &[u8]) -> i32 {
    match super::kfs::write_file_chunk(handle, data) {
        Ok(()) => 0,
        Err(e) => {
            let rc = fs_error_to_code(e);
            log_fs_cabi_handle_fail(
                "write_chunk",
                handle,
                alloc::format!("len={}", data.len()).as_str(),
                rc,
            );
            rc
        }
    }
}

pub(crate) fn fs_write_finish_host(handle: u32) -> i32 {
    match super::kfs::write_file_finish(handle) {
        Ok(()) => 0,
        Err(e) => {
            let rc = fs_error_to_code(e);
            log_fs_cabi_handle_fail("write_finish", handle, "", rc);
            rc
        }
    }
}

pub(crate) fn fs_write_abort_host(handle: u32) -> i32 {
    match super::kfs::write_file_abort(handle) {
        Ok(()) => 0,
        Err(e) => {
            let rc = fs_error_to_code(e);
            log_fs_cabi_handle_fail("write_abort", handle, "", rc);
            rc
        }
    }
}

pub(crate) fn fs_create_dir_all_host(path: &str) -> i32 {
    if path.len() > QJS_ASYNC_FS_MAX_PATH {
        log_fs_cabi_path_fail(
            "create_dir_all",
            path,
            None,
            "reason=raw-path-too-large",
            FS_ERR_TOO_LARGE,
        );
        return FS_ERR_TOO_LARGE;
    }
    let raw = path;
    let Some(path) = super::env::resolve_fs_path(path, true) else {
        log_fs_cabi_path_fail(
            "create_dir_all",
            raw,
            None,
            "reason=resolve-failed",
            FS_ERR_BAD_PATH,
        );
        return FS_ERR_BAD_PATH;
    };
    if path.len() > QJS_ASYNC_FS_MAX_PATH {
        log_fs_cabi_path_fail(
            "create_dir_all",
            raw,
            Some(path.as_str()),
            "reason=resolved-path-too-large",
            FS_ERR_TOO_LARGE,
        );
        return FS_ERR_TOO_LARGE;
    }
    match super::kfs::create_dir_all(path.as_str()) {
        Ok(()) => 0,
        Err(e) => {
            let rc = fs_error_to_code(e);
            log_fs_cabi_path_fail("create_dir_all", raw, Some(path.as_str()), "", rc);
            rc
        }
    }
}

pub(crate) fn fs_exists_host(path: &str) -> i32 {
    if path.len() > QJS_ASYNC_FS_MAX_PATH {
        log_fs_cabi_path_fail("exists", path, None, "reason=raw-path-too-large", FS_ERR_TOO_LARGE);
        return FS_ERR_TOO_LARGE;
    }
    let raw = path;
    let Some(path) = super::env::resolve_fs_path(path, false) else {
        log_fs_cabi_path_fail("exists", raw, None, "reason=resolve-failed", FS_ERR_BAD_PATH);
        return FS_ERR_BAD_PATH;
    };
    if path.len() > QJS_ASYNC_FS_MAX_PATH {
        log_fs_cabi_path_fail(
            "exists",
            raw,
            Some(path.as_str()),
            "reason=resolved-path-too-large",
            FS_ERR_TOO_LARGE,
        );
        return FS_ERR_TOO_LARGE;
    }
    match super::kfs::exists(path.as_str()) {
        Ok(true) => 1,
        Ok(false) => 0,
        Err(e) => {
            let rc = fs_error_to_code(e);
            log_fs_cabi_path_fail("exists", raw, Some(path.as_str()), "", rc);
            rc
        }
    }
}

pub(crate) fn fs_stat_host(path: &str, out_kind: &mut u32, out_len: &mut u64) -> i32 {
    if path.len() > QJS_ASYNC_FS_MAX_PATH {
        log_fs_cabi_path_fail("stat", path, None, "reason=raw-path-too-large", FS_ERR_TOO_LARGE);
        return FS_ERR_TOO_LARGE;
    }
    let raw = path;
    let Some(path) = super::env::resolve_fs_path(path, true) else {
        log_fs_cabi_path_fail("stat", raw, None, "reason=resolve-failed", FS_ERR_BAD_PATH);
        return FS_ERR_BAD_PATH;
    };
    if path.len() > QJS_ASYNC_FS_MAX_PATH {
        log_fs_cabi_path_fail(
            "stat",
            raw,
            Some(path.as_str()),
            "reason=resolved-path-too-large",
            FS_ERR_TOO_LARGE,
        );
        return FS_ERR_TOO_LARGE;
    }
    match super::kfs::stat(path.as_str()) {
        Ok(stat) => {
            *out_kind = match stat.kind {
                super::kfs::FsNodeKind::File => 1,
                super::kfs::FsNodeKind::Directory => 2,
            };
            *out_len = stat.len;
            0
        }
        Err(e) => {
            let rc = fs_error_to_code(e);
            log_fs_cabi_path_fail("stat", raw, Some(path.as_str()), "", rc);
            rc
        }
    }
}

pub(crate) fn fs_remove_host(path: &str) -> i32 {
    if path.len() > QJS_ASYNC_FS_MAX_PATH {
        log_fs_cabi_path_fail("remove", path, None, "reason=raw-path-too-large", FS_ERR_TOO_LARGE);
        return FS_ERR_TOO_LARGE;
    }
    let raw = path;
    let Some(path) = super::env::resolve_fs_path(path, false) else {
        log_fs_cabi_path_fail("remove", raw, None, "reason=resolve-failed", FS_ERR_BAD_PATH);
        return FS_ERR_BAD_PATH;
    };
    if path.len() > QJS_ASYNC_FS_MAX_PATH {
        log_fs_cabi_path_fail(
            "remove",
            raw,
            Some(path.as_str()),
            "reason=resolved-path-too-large",
            FS_ERR_TOO_LARGE,
        );
        return FS_ERR_TOO_LARGE;
    }
    match super::kfs::remove(path.as_str()) {
        Ok(()) => 0,
        Err(e) => {
            let rc = fs_error_to_code(e);
            log_fs_cabi_path_fail("remove", raw, Some(path.as_str()), "", rc);
            rc
        }
    }
}

unsafe fn guest_fs_read_file(path_bytes: &[u8], out_ptr: *mut u8, out_cap: usize) -> isize {
    if path_bytes.len() > trueos_vm::vmcall::PAYLOAD_CAP {
        return FS_ERR_TOO_LARGE as isize;
    }
    let mut bytes = [0u8; trueos_vm::vmcall::PAYLOAD_CAP];
    let (status, len) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_FS_READ_FILE,
        0,
        0,
        path_bytes,
        &mut bytes,
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return FS_ERR_BAD_PARAM as isize;
    }
    let len = vmcall_signed(len);
    if len < 0 || out_ptr.is_null() || out_cap == 0 {
        return len;
    }
    let len = len as usize;
    if out_cap < len {
        return FS_ERR_NO_SPACE as isize;
    }

    let mut offset = 0usize;
    while offset < len {
        let want = core::cmp::min(trueos_vm::vmcall::PAYLOAD_CAP, len - offset);
        let (status, got) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_FS_READ_FILE,
            offset as u64,
            want as u64,
            path_bytes,
            &mut bytes,
        );
        if status != trueos_vm::vmcall::STATUS_OK {
            return FS_ERR_BAD_PARAM as isize;
        }
        let got = vmcall_signed(got);
        if got < 0 {
            return got;
        }
        let got = got as usize;
        if got == 0 || got > want {
            return FS_ERR_IO as isize;
        }
        unsafe {
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), out_ptr.add(offset), got);
        }
        offset += got;
    }
    len as isize
}

fn guest_fs_write_begin(path_bytes: &[u8], total_len: u64, out_handle: *mut u32) -> i32 {
    if path_bytes.len() > trueos_vm::vmcall::PAYLOAD_CAP {
        return FS_ERR_TOO_LARGE;
    }
    let mut out = [0u8; 1];
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_FS_WRITE_BEGIN,
        total_len,
        0,
        path_bytes,
        &mut out,
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return FS_ERR_BAD_PARAM;
    }
    let rc = data as i64;
    if rc <= 0 {
        return rc as i32;
    }
    unsafe {
        *out_handle = rc as u32;
    }
    0
}

fn guest_fs_write_chunk(handle: u32, data: &[u8]) -> i32 {
    let mut offset = 0usize;
    while offset < data.len() {
        let end = core::cmp::min(offset + trueos_vm::vmcall::PAYLOAD_CAP, data.len());
        let mut out = [0u8; 1];
        let (status, rc) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_FS_WRITE_CHUNK,
            handle as u64,
            0,
            &data[offset..end],
            &mut out,
        );
        if status != trueos_vm::vmcall::STATUS_OK {
            return FS_ERR_BAD_PARAM;
        }
        let rc = vmcall_signed_i32(rc);
        if rc != 0 {
            return rc;
        }
        offset = end;
    }
    0
}

fn guest_fs_simple_path_op(op: u32, path_bytes: &[u8]) -> i32 {
    if path_bytes.len() > trueos_vm::vmcall::PAYLOAD_CAP {
        return FS_ERR_TOO_LARGE;
    }
    let mut out = [0u8; 1];
    let (status, rc) = trueos_vm::vmcall::call_with_payload(op, 0, 0, path_bytes, &mut out);
    if status != trueos_vm::vmcall::STATUS_OK {
        return FS_ERR_BAD_PARAM;
    }
    vmcall_signed_i32(rc)
}

fn guest_fs_stat(path_bytes: &[u8], out_kind: *mut u32, out_len: *mut u64) -> i32 {
    if out_kind.is_null() || out_len.is_null() {
        return FS_ERR_BAD_PARAM;
    }
    if path_bytes.len() > trueos_vm::vmcall::PAYLOAD_CAP {
        return FS_ERR_TOO_LARGE;
    }
    let mut out = [0u8; 1];
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_FS_STAT,
        0,
        0,
        path_bytes,
        &mut out,
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return FS_ERR_BAD_PARAM;
    }
    let rc = vmcall_signed_i32(data);
    if rc != 0 {
        return rc;
    }
    unsafe {
        *out_kind = (data >> 32) as u32;
        *out_len = if *out_kind == 1 {
            let len = guest_fs_read_file(path_bytes, core::ptr::null_mut(), 0);
            if len < 0 {
                return len as i32;
            }
            len as u64
        } else {
            0
        };
    }
    0
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
    let path_bytes = unsafe { core::slice::from_raw_parts(path_ptr, path_len) };
    let Ok(path) = core::str::from_utf8(path_bytes) else {
        return FS_ERR_BAD_UTF8 as isize;
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return unsafe { guest_fs_read_file(path_bytes, out_ptr, out_cap) };
    }

    if out_ptr.is_null() || out_cap == 0 {
        return fs_read_file_len_host(path);
    }

    let len = fs_read_file_len_host(path);
    if len < 0 {
        return len;
    }
    if out_cap < len as usize {
        return FS_ERR_NO_SPACE as isize;
    }
    unsafe {
        fs_read_file_chunk_host(path, 0, core::slice::from_raw_parts_mut(out_ptr, len as usize))
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_fs_write_begin(
    path_ptr: *const u8,
    path_len: usize,
    total_len: u64,
    out_handle: *mut u32,
) -> i32 {
    if out_handle.is_null() || (path_ptr.is_null() && path_len != 0) {
        return FS_ERR_BAD_PARAM;
    }
    if path_len > QJS_ASYNC_FS_MAX_PATH {
        return FS_ERR_TOO_LARGE;
    }
    let path_bytes = unsafe { core::slice::from_raw_parts(path_ptr, path_len) };
    let Ok(path) = core::str::from_utf8(path_bytes) else {
        return FS_ERR_BAD_UTF8;
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return guest_fs_write_begin(path_bytes, total_len, out_handle);
    }
    let rc = fs_write_begin_host(path, total_len);
    if rc <= 0 {
        return rc as i32;
    }
    unsafe {
        *out_handle = rc as u32;
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_fs_create_dir_all(
    path_ptr: *const u8,
    path_len: usize,
) -> i32 {
    if path_ptr.is_null() && path_len != 0 {
        return FS_ERR_BAD_PARAM;
    }
    if path_len > QJS_ASYNC_FS_MAX_PATH {
        return FS_ERR_TOO_LARGE;
    }
    let path_bytes = unsafe { core::slice::from_raw_parts(path_ptr, path_len) };
    let Ok(path) = core::str::from_utf8(path_bytes) else {
        return FS_ERR_BAD_UTF8;
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return guest_fs_simple_path_op(trueos_vm::vmcall::OP_BP_FS_CREATE_DIR_ALL, path_bytes);
    }
    fs_create_dir_all_host(path)
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
        unsafe { core::slice::from_raw_parts(data_ptr, data_len) }
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return guest_fs_write_chunk(handle, data);
    }
    fs_write_chunk_host(handle, data)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_fs_write_finish(handle: u32) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, rc) =
            trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_FS_WRITE_FINISH, handle as u64, 0);
        return if status == trueos_vm::vmcall::STATUS_OK {
            vmcall_signed_i32(rc)
        } else {
            FS_ERR_BAD_PARAM
        };
    }
    fs_write_finish_host(handle)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_fs_write_abort(handle: u32) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, rc) =
            trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_FS_WRITE_ABORT, handle as u64, 0);
        return if status == trueos_vm::vmcall::STATUS_OK {
            vmcall_signed_i32(rc)
        } else {
            FS_ERR_BAD_PARAM
        };
    }
    fs_write_abort_host(handle)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_fs_exists(path_ptr: *const u8, path_len: usize) -> i32 {
    if path_ptr.is_null() && path_len != 0 {
        return FS_ERR_BAD_PARAM;
    }
    if path_len > QJS_ASYNC_FS_MAX_PATH {
        return FS_ERR_TOO_LARGE;
    }
    let path_bytes = unsafe { core::slice::from_raw_parts(path_ptr, path_len) };
    let Ok(path) = core::str::from_utf8(path_bytes) else {
        return FS_ERR_BAD_UTF8;
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return guest_fs_simple_path_op(trueos_vm::vmcall::OP_BP_FS_EXISTS, path_bytes);
    }
    fs_exists_host(path)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_fs_stat(
    path_ptr: *const u8,
    path_len: usize,
    out_kind: *mut u32,
    out_len: *mut u64,
) -> i32 {
    if out_kind.is_null() || out_len.is_null() || (path_ptr.is_null() && path_len != 0) {
        return FS_ERR_BAD_PARAM;
    }
    if path_len > QJS_ASYNC_FS_MAX_PATH {
        return FS_ERR_TOO_LARGE;
    }
    let path_bytes = unsafe { core::slice::from_raw_parts(path_ptr, path_len) };
    let Ok(path) = core::str::from_utf8(path_bytes) else {
        return FS_ERR_BAD_UTF8;
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return guest_fs_stat(path_bytes, out_kind, out_len);
    }
    unsafe { fs_stat_host(path, &mut *out_kind, &mut *out_len) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_fs_remove(path_ptr: *const u8, path_len: usize) -> i32 {
    if path_ptr.is_null() && path_len != 0 {
        return FS_ERR_BAD_PARAM;
    }
    if path_len > QJS_ASYNC_FS_MAX_PATH {
        return FS_ERR_TOO_LARGE;
    }
    let path_bytes = unsafe { core::slice::from_raw_parts(path_ptr, path_len) };
    let Ok(path) = core::str::from_utf8(path_bytes) else {
        return FS_ERR_BAD_UTF8;
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return guest_fs_simple_path_op(trueos_vm::vmcall::OP_BP_FS_REMOVE, path_bytes);
    }
    fs_remove_host(path)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_trueosfs_primary_html_tree(
    max_entries: u32,
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    let limit = if max_entries == 0 {
        64usize
    } else {
        max_entries as usize
    };

    match super::kfs::html_tree(limit) {
        Ok(html) => copy_text(html.as_bytes(), out_ptr, out_cap),
        Err(e) => fs_error_to_code(e) as isize,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_trueosfs_primary_json_all(
    max_entries: u32,
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    let limit = if max_entries == 0 {
        256usize
    } else {
        max_entries as usize
    };

    match super::kfs::json_all(limit) {
        Ok(json) => copy_text(json.as_bytes(), out_ptr, out_cap),
        Err(e) => fs_error_to_code(e) as isize,
    }
}

unsafe fn copy_text(bytes: &[u8], out_ptr: *mut u8, out_cap: usize) -> isize {
    if out_ptr.is_null() || out_cap == 0 {
        return bytes.len() as isize;
    }
    if bytes.len() > out_cap {
        return FS_ERR_NO_SPACE as isize;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), out_ptr, bytes.len());
    }
    bytes.len() as isize
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_env_args_count() -> usize {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, count) =
            trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_ENV_ARGS_COUNT, 0, 0);
        return if status == trueos_vm::vmcall::STATUS_OK {
            count as usize
        } else {
            0
        };
    }
    super::env::arg_count()
}

fn copy_guest_text_response(
    status: u32,
    len: u64,
    bytes: &[u8],
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    if status != trueos_vm::vmcall::STATUS_OK {
        return -1;
    }
    let len = len as usize;
    if out_ptr.is_null() || out_cap == 0 || out_cap < len {
        return len as isize;
    }
    if len > bytes.len() {
        return -1;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), out_ptr, len);
    }
    len as isize
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_env_arg(
    index: usize,
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let mut bytes = [0u8; trueos_vm::vmcall::PAYLOAD_CAP];
        let (status, len) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_ENV_ARG,
            index as u64,
            0,
            &[],
            &mut bytes,
        );
        return copy_guest_text_response(status, len, &bytes, out_ptr, out_cap);
    }
    let Some(arg) = super::env::arg(index) else {
        return -1;
    };
    unsafe { copy_text(arg.as_bytes(), out_ptr, out_cap) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_env_var(
    key_ptr: *const u8,
    key_len: usize,
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    if key_ptr.is_null() {
        return -1;
    }
    let key_bytes = unsafe { core::slice::from_raw_parts(key_ptr, key_len) };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        if key_bytes.len() > trueos_vm::vmcall::PAYLOAD_CAP {
            return -1;
        }
        let mut bytes = [0u8; trueos_vm::vmcall::PAYLOAD_CAP];
        let (status, len) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_ENV_VAR,
            0,
            0,
            key_bytes,
            &mut bytes,
        );
        return copy_guest_text_response(status, len, &bytes, out_ptr, out_cap);
    }
    let Ok(key) = core::str::from_utf8(key_bytes) else {
        return -1;
    };
    let Some(value) = super::env::var(key) else {
        return -1;
    };
    unsafe { copy_text(value.as_bytes(), out_ptr, out_cap) }
}

static SHELL_ATTACHED_REJECTS: AtomicU32 = AtomicU32::new(0);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_uart1_shell_write(
    data_ptr: *const u8,
    data_len: usize,
) -> usize {
    if data_ptr.is_null() || data_len == 0 {
        return 0;
    }
    let data = unsafe { core::slice::from_raw_parts(data_ptr, data_len) };
    crate::shell2::uart1_com1::write_bytes(data);
    data_len
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_shell_attached_write(
    data_ptr: *const u8,
    data_len: usize,
) -> usize {
    if data_ptr.is_null() || data_len == 0 {
        return 0;
    }
    let data = unsafe { core::slice::from_raw_parts(data_ptr, data_len) };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let mut written = 0usize;
        while written < data.len() {
            let end = core::cmp::min(written + trueos_vm::vmcall::PAYLOAD_CAP, data.len());
            let (status, count) = trueos_vm::vmcall::call_with_payload(
                trueos_vm::vmcall::OP_BP_SHELL_ATTACHED_WRITE,
                0,
                0,
                &data[written..end],
                &mut [],
            );
            if status != trueos_vm::vmcall::STATUS_OK {
                break;
            }
            written = written.saturating_add(count as usize);
            if count == 0 {
                break;
            }
        }
        return written;
    }
    if let Some(target) = super::env::console_target() {
        return crate::shell2::raw_write_matrix_target(&target, data);
    }
    if SHELL_ATTACHED_REJECTS.fetch_add(1, Ordering::Relaxed) == 0 {
        crate::log!("fs-cabi: shell attached write falling back to uart1\n");
    }
    crate::shell2::uart1_com1::write_bytes(data);
    data_len
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_shell_attached_read_byte() -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, data) =
            trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_SHELL_ATTACHED_READ_BYTE, 0, 0);
        if status == trueos_vm::vmcall::STATUS_OK && data != u64::MAX {
            return data as u8 as i32;
        }
        return -1;
    }
    if let Some(target) = super::env::console_target() {
        return crate::shell2::read_matrix_target_byte(&target)
            .map(i32::from)
            .unwrap_or(-1);
    }
    crate::shell2::uart1_com1::read_byte()
        .map(i32::from)
        .unwrap_or(-1)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_shell_attached_retarget_slot(
    slot_ptr: *const u8,
    slot_len: usize,
) -> i32 {
    if slot_ptr.is_null() || slot_len == 0 {
        return -1;
    }
    let slot = unsafe { core::slice::from_raw_parts(slot_ptr, slot_len) };
    let Ok(slot) = core::str::from_utf8(slot) else {
        return -1;
    };
    if super::env::retarget_console_slot(slot) {
        0
    } else {
        -1
    }
}
