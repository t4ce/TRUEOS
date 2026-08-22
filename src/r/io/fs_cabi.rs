extern crate alloc;

include!("../cabi_codes.rs");

use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::String;
use alloc::vec::Vec;
use core::{
    fmt,
    sync::atomic::{AtomicU32, Ordering},
};
use log_os_core::LogLevel;

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

const CABI_LOG_TARGET_MAX: usize = 256;
const CABI_LOG_MESSAGE_MAX: usize = 64 * 1024;

fn current_cpu_key() -> u32 {
    super::runtime_context_key()
}

fn level_from_tag(level: &str) -> Option<LogLevel> {
    match level {
        "ERROR" => Some(LogLevel::Error),
        "IMPORTANT" => Some(LogLevel::Important),
        "WARN" => Some(LogLevel::Warn),
        "ONCE" => Some(LogLevel::Once),
        "INFO" => Some(LogLevel::Info),
        "DEBUG" => Some(LogLevel::Debug),
        "TRACE" => Some(LogLevel::Trace),
        _ => None,
    }
}

fn parse_structured_guest_log(line: &str) -> Option<(&str, LogLevel, &str)> {
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

fn emit_guest_log_line(source: &str, level: LogLevel, message: &str) {
    crate::log_os::log_with_area_purpose(
        crate::log_os::flags::LogArea::Blueprint,
        level,
        Some(crate::log_os::purpose_for_level(level)),
        format_args!("{}: {}\n", source, message),
    );
}

fn plain_stream_level(stream: ConsoleStream, line: &str) -> LogLevel {
    if stream != ConsoleStream::Err {
        return LogLevel::Info;
    }

    let line = line.trim_start();
    if line.starts_with("error[") || line.starts_with("error:") {
        LogLevel::Error
    } else if line.starts_with("warning[") || line.starts_with("warning:") {
        LogLevel::Warn
    } else {
        LogLevel::Info
    }
}

fn emit_plain_stream_line(stream: ConsoleStream, line: &str) {
    crate::log_os::log_with_area_level(
        crate::log_os::flags::LogArea::Global,
        plain_stream_level(stream, line),
        format_args!("{}\n", line),
    );
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

fn guest_shell_attached_write(data: &[u8]) -> usize {
    guest_shell_write_op(trueos_vm::vmcall::OP_BP_SHELL_ATTACHED_WRITE, data)
}

fn guest_shell2_raw_write(data: &[u8]) -> usize {
    guest_shell_write_op(trueos_vm::vmcall::OP_BP_SHELL_RAW_WRITE, data)
}

fn guest_shell_write_op(op: u32, data: &[u8]) -> usize {
    let mut written = 0usize;
    while written < data.len() {
        let end = core::cmp::min(written + trueos_vm::vmcall::PAYLOAD_CAP, data.len());
        let chunk = &data[written..end];
        let (status, count) = trueos_vm::vmcall::call_with_payload(op, 0, 0, chunk, &mut []);
        if status != trueos_vm::vmcall::STATUS_OK {
            break;
        }
        let count = core::cmp::min(count as usize, chunk.len());
        if count == 0 {
            break;
        }
        written = written.saturating_add(count);
    }
    written
}

#[inline]
pub fn write_console_bytes(stream: ConsoleStream, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }

    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let _ = guest_shell_attached_write(bytes);
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

pub fn write_raw_console_bytes(bytes: &[u8]) -> usize {
    konsole_write_bytes(bytes)
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

/// Structured runtime log entry from a dynamically loaded blueprint.
///
/// This is intentionally separate from `trueos_cabi_write`: application log
/// records belong in the global `log_os` router and must not be mixed into a
/// rich terminal's byte stream. The numeric levels match `trueos::logl`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_log(
    level_code: u32,
    target_ptr: *const u8,
    target_len: usize,
    message_ptr: *const u8,
    message_len: usize,
) -> i32 {
    let level = match level_code {
        trueos_vm::vmcall::BP_LOG_LEVEL_ERROR => LogLevel::Error,
        trueos_vm::vmcall::BP_LOG_LEVEL_WARN => LogLevel::Warn,
        trueos_vm::vmcall::BP_LOG_LEVEL_INFO => LogLevel::Info,
        trueos_vm::vmcall::BP_LOG_LEVEL_DEBUG => LogLevel::Debug,
        trueos_vm::vmcall::BP_LOG_LEVEL_TRACE => LogLevel::Trace,
        trueos_vm::vmcall::BP_LOG_LEVEL_IMPORTANT => LogLevel::Important,
        trueos_vm::vmcall::BP_LOG_LEVEL_ONCE => LogLevel::Once,
        _ => return -1,
    };
    if target_ptr.is_null()
        || target_len == 0
        || target_len > CABI_LOG_TARGET_MAX
        || message_len > CABI_LOG_MESSAGE_MAX
        || (message_ptr.is_null() && message_len != 0)
    {
        return -1;
    }

    let target_bytes = unsafe { core::slice::from_raw_parts(target_ptr, target_len) };
    let message_bytes = if message_len == 0 {
        &[][..]
    } else {
        unsafe { core::slice::from_raw_parts(message_ptr, message_len) }
    };
    let Ok(target) = core::str::from_utf8(target_bytes) else {
        return -1;
    };
    let Ok(message) = core::str::from_utf8(message_bytes) else {
        return -1;
    };
    let message = message.trim_end_matches(&['\r', '\n'][..]);
    let purpose = crate::log_os::purpose_for_level(level);

    // A Hull owns a private copy of kernel static state. Routing a structured
    // Blueprint record through the guest's local LogOs instance therefore
    // cannot publish it to the BSP diagnostic stream. Use one atomic, typed
    // VMCall record so the host is the sole LogOs owner and the message never
    // enters the attached terminal data plane.
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let target_len = target_bytes.len().min(trueos_vm::vmcall::PAYLOAD_CAP);
        let mut message_len = message
            .len()
            .min(trueos_vm::vmcall::PAYLOAD_CAP.saturating_sub(target_len));
        while !message.is_char_boundary(message_len) {
            message_len = message_len.saturating_sub(1);
        }
        let mut record = Vec::with_capacity(target_len.saturating_add(message_len));
        record.extend_from_slice(&target_bytes[..target_len]);
        record.extend_from_slice(&message.as_bytes()[..message_len]);
        let (status, accepted) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_LOG_RECORD_V1,
            u64::from(level_code),
            target_len as u64,
            record.as_slice(),
            &mut [],
        );
        return if status == trueos_vm::vmcall::STATUS_OK && accepted as usize == record.len() {
            0
        } else {
            -1
        };
    }

    crate::log_os::log_with_area_purpose(
        crate::log_os::flags::LogArea::Apps,
        level,
        Some(purpose),
        format_args!("{}: {}\n", target, message),
    );
    0
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

#[inline]
fn should_log_fs_cabi_path_fail(op: &str, rc: i32) -> bool {
    if rc >= 0 {
        return false;
    }

    if rc == FS_ERR_NOT_FOUND && matches!(op, "read_len" | "read_chunk") {
        return false;
    }

    true
}

fn log_fs_cabi_path_fail(op: &str, raw: &str, resolved: Option<&str>, detail: &str, rc: i32) {
    if !should_log_fs_cabi_path_fail(op, rc) {
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
    if path.len() > BLUEPRINT_ASYNC_FS_MAX_PATH {
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
    if path.len() > BLUEPRINT_ASYNC_FS_MAX_PATH {
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
        Ok(len) => {
            if path.contains("ggml-tiny") {
                crate::log!("fs-cabi: read_len ok resolved={} len={}\n", path.as_str(), len);
            }
            len as isize
        }
        Err(e) => {
            let rc = fs_error_to_code(e);
            log_fs_cabi_path_fail("read_len", path.as_str(), Some(path.as_str()), "", rc);
            rc as isize
        }
    }
}

const MODEL_READ_PROGRESS_STEP: usize = 4 * 1024 * 1024;

fn should_log_model_read_chunk(path: &str, offset: usize, cap: usize, got: usize) -> bool {
    if !path.contains("ggml-tiny") {
        return false;
    }
    if offset == 0 || got == 0 {
        return true;
    }
    if cap < trueos_vm::vmcall::PAYLOAD_CAP {
        return true;
    }
    let end = offset.saturating_add(got);
    (offset / MODEL_READ_PROGRESS_STEP) != (end / MODEL_READ_PROGRESS_STEP)
}

pub(crate) fn fs_read_file_chunk_host(path: &str, offset: usize, out: &mut [u8]) -> isize {
    if path.len() > BLUEPRINT_ASYNC_FS_MAX_PATH {
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
    if path.len() > BLUEPRINT_ASYNC_FS_MAX_PATH {
        log_fs_cabi_path_fail(
            "read_chunk",
            path.as_str(),
            Some(path.as_str()),
            "reason=resolved-path-too-large",
            FS_ERR_TOO_LARGE,
        );
        return FS_ERR_TOO_LARGE as isize;
    }
    match super::kfs::read_file_range(path.as_str(), offset as u64, out) {
        Ok(got) => {
            if should_log_model_read_chunk(path.as_str(), offset, out.len(), got) {
                let end = offset.saturating_add(got);
                crate::log!(
                    "fs-cabi: read_chunk progress resolved={} offset={} end={} cap={} got={} mib={}\n",
                    path.as_str(),
                    offset,
                    end,
                    out.len(),
                    got,
                    end / (1024 * 1024)
                );
            }
            got as isize
        }
        Err(e) => {
            let rc = fs_error_to_code(e);
            log_fs_cabi_path_fail("read_chunk", path.as_str(), Some(path.as_str()), "", rc);
            rc as isize
        }
    }
}

pub(crate) fn fs_write_begin_host(path: &str, total_len: u64) -> i64 {
    if path.len() > BLUEPRINT_ASYNC_FS_MAX_PATH {
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
    if path.len() > BLUEPRINT_ASYNC_FS_MAX_PATH {
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
        Ok(h) => {
            if path.starts_with("apps/") {
                crate::log!(
                    "fs-cabi: write_begin ok resolved={} len={} handle={}\n",
                    path.as_str(),
                    total_len,
                    h
                );
            }
            h as i64
        }
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
    if path.len() > BLUEPRINT_ASYNC_FS_MAX_PATH {
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
    if path.len() > BLUEPRINT_ASYNC_FS_MAX_PATH {
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
        Err(error) => {
            let rc = fs_error_to_code(error);
            log_fs_cabi_path_fail("create_dir_all", raw, Some(path.as_str()), "", rc);
            rc
        }
    }
}

fn wait_for_guest_create_dir_all(path: &str) -> i32 {
    let operation = unsafe {
        super::async_fs_cabi::trueos_cabi_async_fs_create_dir_all_start(path.as_ptr(), path.len())
    };
    if operation <= 0 {
        return operation;
    }
    let operation = operation as u32;

    loop {
        match super::async_fs_cabi::trueos_cabi_async_fs_status(operation) {
            0 => {
                let _ = trueos_vm::vmcall::call(trueos_vm::vmcall::OP_YIELD, 0, 0);
            }
            1 => {
                return super::async_fs_cabi::trueos_cabi_async_fs_discard(operation);
            }
            rc => {
                let _ = super::async_fs_cabi::trueos_cabi_async_fs_discard(operation);
                return rc;
            }
        }
    }
}

pub(crate) fn fs_exists_host(path: &str) -> i32 {
    if path.len() > BLUEPRINT_ASYNC_FS_MAX_PATH {
        log_fs_cabi_path_fail("exists", path, None, "reason=raw-path-too-large", FS_ERR_TOO_LARGE);
        return FS_ERR_TOO_LARGE;
    }
    let raw = path;
    let Some(path) = super::env::resolve_fs_path(path, false) else {
        log_fs_cabi_path_fail("exists", raw, None, "reason=resolve-failed", FS_ERR_BAD_PATH);
        return FS_ERR_BAD_PATH;
    };
    if path.len() > BLUEPRINT_ASYNC_FS_MAX_PATH {
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
    if path.len() > BLUEPRINT_ASYNC_FS_MAX_PATH {
        log_fs_cabi_path_fail("stat", path, None, "reason=raw-path-too-large", FS_ERR_TOO_LARGE);
        return FS_ERR_TOO_LARGE;
    }
    let raw = path;
    let Some(path) = super::env::resolve_fs_path(path, true) else {
        log_fs_cabi_path_fail("stat", raw, None, "reason=resolve-failed", FS_ERR_BAD_PATH);
        return FS_ERR_BAD_PATH;
    };
    if path.len() > BLUEPRINT_ASYNC_FS_MAX_PATH {
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
            if path.contains("ggml-tiny") {
                crate::log!(
                    "fs-cabi: stat ok resolved={} kind={} len={}\n",
                    path.as_str(),
                    *out_kind,
                    *out_len
                );
            }
            0
        }
        Err(e) => {
            let rc = fs_error_to_code(e);
            log_fs_cabi_path_fail("stat", raw, Some(path.as_str()), "", rc);
            rc
        }
    }
}

pub(crate) fn fs_list_dir_host_text(path: &str) -> core::result::Result<String, i32> {
    if path.len() > BLUEPRINT_ASYNC_FS_MAX_PATH {
        log_fs_cabi_path_fail(
            "list_dir",
            path,
            None,
            "reason=raw-path-too-large",
            FS_ERR_TOO_LARGE,
        );
        return Err(FS_ERR_TOO_LARGE);
    }
    let raw = path;
    let Some(path) = super::env::resolve_fs_path(path, true) else {
        log_fs_cabi_path_fail("list_dir", raw, None, "reason=resolve-failed", FS_ERR_BAD_PATH);
        return Err(FS_ERR_BAD_PATH);
    };
    if path.len() > BLUEPRINT_ASYNC_FS_MAX_PATH {
        log_fs_cabi_path_fail(
            "list_dir",
            raw,
            Some(path.as_str()),
            "reason=resolved-path-too-large",
            FS_ERR_TOO_LARGE,
        );
        return Err(FS_ERR_TOO_LARGE);
    }
    match super::kfs::list_dir(path.as_str()) {
        Ok(text) => Ok(text),
        Err(e) => {
            let rc = fs_error_to_code(e);
            log_fs_cabi_path_fail("list_dir", raw, Some(path.as_str()), "", rc);
            Err(rc)
        }
    }
}

pub(crate) fn fs_list_dir_host(path: &str, out_ptr: *mut u8, out_cap: usize) -> isize {
    match fs_list_dir_host_text(path) {
        Ok(text) => unsafe { copy_text(text.as_bytes(), out_ptr, out_cap) },
        Err(rc) => rc as isize,
    }
}

pub(crate) fn fs_remove_host(path: &str) -> i32 {
    if path.len() > BLUEPRINT_ASYNC_FS_MAX_PATH {
        log_fs_cabi_path_fail("remove", path, None, "reason=raw-path-too-large", FS_ERR_TOO_LARGE);
        return FS_ERR_TOO_LARGE;
    }
    let raw = path;
    let Some(path) = super::env::resolve_fs_path(path, false) else {
        log_fs_cabi_path_fail("remove", raw, None, "reason=resolve-failed", FS_ERR_BAD_PATH);
        return FS_ERR_BAD_PATH;
    };
    if path.len() > BLUEPRINT_ASYNC_FS_MAX_PATH {
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
    let Some(out) = crate::std_abi_shim::abi_write_bytes(out_ptr, len) else {
        return FS_ERR_BAD_PARAM as isize;
    };

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
        out[offset..offset + got].copy_from_slice(&bytes[..got]);
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
    let mut out = [0u8; 12];
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
        let payload_kind = u32::from_le_bytes([out[0], out[1], out[2], out[3]]);
        if payload_kind != 0 {
            *out_kind = payload_kind;
            *out_len = u64::from_le_bytes([
                out[4], out[5], out[6], out[7], out[8], out[9], out[10], out[11],
            ]);
        } else {
            *out_kind = (data >> 32) as u32;
            *out_len = 0;
        }
    }
    0
}

unsafe fn guest_fs_list_dir(path_bytes: &[u8], out_ptr: *mut u8, out_cap: usize) -> isize {
    if path_bytes.len() > trueos_vm::vmcall::PAYLOAD_CAP {
        return FS_ERR_TOO_LARGE as isize;
    }

    let mut probe = [0u8; trueos_vm::vmcall::PAYLOAD_CAP];
    let (status, len) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_FS_LIST_DIR,
        0,
        0,
        path_bytes,
        &mut probe,
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
    let Some(out) = crate::std_abi_shim::abi_write_bytes(out_ptr, len) else {
        return FS_ERR_BAD_PARAM as isize;
    };

    let mut offset = 0usize;
    while offset < len {
        let mut bytes = [0u8; trueos_vm::vmcall::PAYLOAD_CAP];
        let want = core::cmp::min(trueos_vm::vmcall::PAYLOAD_CAP, len - offset);
        let (status, got) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_FS_LIST_DIR,
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
        if got == 0 {
            break;
        }
        out[offset..offset + got].copy_from_slice(&bytes[..got]);
        offset = offset.saturating_add(got);
    }

    offset as isize
}

fn guest_resolved_fs_path(path: &str, allow_empty: bool) -> Result<String, i32> {
    let Some(path) = super::env::resolve_fs_path(path, allow_empty) else {
        return Err(FS_ERR_BAD_PATH);
    };
    if path.len() > BLUEPRINT_ASYNC_FS_MAX_PATH {
        return Err(FS_ERR_TOO_LARGE);
    }
    Ok(path)
}

// Legacy synchronous filesystem entry point retained only for kernel shims.
// Its internal export name deliberately cannot satisfy a Blueprint CABI import.
#[unsafe(export_name = "trueos_kernel_sync_fs_read_file")]
pub unsafe extern "C" fn trueos_cabi_fs_read_file(
    path_ptr: *const u8,
    path_len: usize,
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    if path_ptr.is_null() && path_len != 0 {
        return FS_ERR_BAD_PARAM as isize;
    }
    if path_len > BLUEPRINT_ASYNC_FS_MAX_PATH {
        return FS_ERR_TOO_LARGE as isize;
    }
    let path_bytes = unsafe { core::slice::from_raw_parts(path_ptr, path_len) };
    let Ok(path) = core::str::from_utf8(path_bytes) else {
        return FS_ERR_BAD_UTF8 as isize;
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let path = match guest_resolved_fs_path(path, false) {
            Ok(path) => path,
            Err(rc) => return rc as isize,
        };
        return unsafe { guest_fs_read_file(path.as_bytes(), out_ptr, out_cap) };
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

#[unsafe(export_name = "trueos_kernel_sync_fs_write_begin")]
pub unsafe extern "C" fn trueos_cabi_fs_write_begin(
    path_ptr: *const u8,
    path_len: usize,
    total_len: u64,
    out_handle: *mut u32,
) -> i32 {
    if out_handle.is_null() || (path_ptr.is_null() && path_len != 0) {
        return FS_ERR_BAD_PARAM;
    }
    if path_len > BLUEPRINT_ASYNC_FS_MAX_PATH {
        return FS_ERR_TOO_LARGE;
    }
    let path_bytes = unsafe { core::slice::from_raw_parts(path_ptr, path_len) };
    let Ok(path) = core::str::from_utf8(path_bytes) else {
        return FS_ERR_BAD_UTF8;
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let path = match guest_resolved_fs_path(path, false) {
            Ok(path) => path,
            Err(rc) => return rc,
        };
        return guest_fs_write_begin(path.as_bytes(), total_len, out_handle);
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

#[unsafe(export_name = "trueos_kernel_sync_fs_create_dir_all")]
pub unsafe extern "C" fn trueos_cabi_fs_create_dir_all(
    path_ptr: *const u8,
    path_len: usize,
) -> i32 {
    if path_ptr.is_null() && path_len != 0 {
        return FS_ERR_BAD_PARAM;
    }
    if path_len > BLUEPRINT_ASYNC_FS_MAX_PATH {
        return FS_ERR_TOO_LARGE;
    }
    let path_bytes = unsafe { core::slice::from_raw_parts(path_ptr, path_len) };
    let Ok(path) = core::str::from_utf8(path_bytes) else {
        return FS_ERR_BAD_UTF8;
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return wait_for_guest_create_dir_all(path);
    }
    fs_create_dir_all_host(path)
}

#[unsafe(export_name = "trueos_kernel_sync_fs_write_chunk")]
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

#[unsafe(export_name = "trueos_kernel_sync_fs_write_finish")]
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

#[unsafe(export_name = "trueos_kernel_sync_fs_write_abort")]
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

#[unsafe(export_name = "trueos_kernel_sync_fs_exists")]
pub unsafe extern "C" fn trueos_cabi_fs_exists(path_ptr: *const u8, path_len: usize) -> i32 {
    if path_ptr.is_null() && path_len != 0 {
        return FS_ERR_BAD_PARAM;
    }
    if path_len > BLUEPRINT_ASYNC_FS_MAX_PATH {
        return FS_ERR_TOO_LARGE;
    }
    let path_bytes = unsafe { core::slice::from_raw_parts(path_ptr, path_len) };
    let Ok(path) = core::str::from_utf8(path_bytes) else {
        return FS_ERR_BAD_UTF8;
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let path = match guest_resolved_fs_path(path, false) {
            Ok(path) => path,
            Err(rc) => return rc,
        };
        return guest_fs_simple_path_op(trueos_vm::vmcall::OP_BP_FS_EXISTS, path.as_bytes());
    }
    fs_exists_host(path)
}

#[unsafe(export_name = "trueos_kernel_sync_fs_stat")]
pub unsafe extern "C" fn trueos_cabi_fs_stat(
    path_ptr: *const u8,
    path_len: usize,
    out_kind: *mut u32,
    out_len: *mut u64,
) -> i32 {
    if out_kind.is_null() || out_len.is_null() || (path_ptr.is_null() && path_len != 0) {
        return FS_ERR_BAD_PARAM;
    }
    if path_len > BLUEPRINT_ASYNC_FS_MAX_PATH {
        return FS_ERR_TOO_LARGE;
    }
    let path_bytes = unsafe { core::slice::from_raw_parts(path_ptr, path_len) };
    let Ok(path) = core::str::from_utf8(path_bytes) else {
        return FS_ERR_BAD_UTF8;
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let path = match guest_resolved_fs_path(path, true) {
            Ok(path) => path,
            Err(rc) => return rc,
        };
        return guest_fs_stat(path.as_bytes(), out_kind, out_len);
    }
    unsafe { fs_stat_host(path, &mut *out_kind, &mut *out_len) }
}

#[unsafe(export_name = "trueos_kernel_sync_fs_list_dir")]
pub unsafe extern "C" fn trueos_cabi_fs_list_dir(
    path_ptr: *const u8,
    path_len: usize,
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    if path_ptr.is_null() && path_len != 0 {
        return FS_ERR_BAD_PARAM as isize;
    }
    if path_len > BLUEPRINT_ASYNC_FS_MAX_PATH {
        return FS_ERR_TOO_LARGE as isize;
    }
    let path_bytes = unsafe { core::slice::from_raw_parts(path_ptr, path_len) };
    let Ok(path) = core::str::from_utf8(path_bytes) else {
        return FS_ERR_BAD_UTF8 as isize;
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let path = match guest_resolved_fs_path(path, true) {
            Ok(path) => path,
            Err(rc) => return rc as isize,
        };
        return unsafe { guest_fs_list_dir(path.as_bytes(), out_ptr, out_cap) };
    }
    fs_list_dir_host(path, out_ptr, out_cap)
}

#[unsafe(export_name = "trueos_kernel_sync_fs_remove")]
pub unsafe extern "C" fn trueos_cabi_fs_remove(path_ptr: *const u8, path_len: usize) -> i32 {
    if path_ptr.is_null() && path_len != 0 {
        return FS_ERR_BAD_PARAM;
    }
    if path_len > BLUEPRINT_ASYNC_FS_MAX_PATH {
        return FS_ERR_TOO_LARGE;
    }
    let path_bytes = unsafe { core::slice::from_raw_parts(path_ptr, path_len) };
    let Ok(path) = core::str::from_utf8(path_bytes) else {
        return FS_ERR_BAD_UTF8;
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let path = match guest_resolved_fs_path(path, false) {
            Ok(path) => path,
            Err(rc) => return rc,
        };
        return guest_fs_simple_path_op(trueos_vm::vmcall::OP_BP_FS_REMOVE, path.as_bytes());
    }
    fs_remove_host(path)
}

#[unsafe(export_name = "trueos_kernel_sync_trueosfs_primary_html_tree")]
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

#[unsafe(export_name = "trueos_kernel_sync_trueosfs_primary_json_all")]
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

#[unsafe(export_name = "trueos_kernel_sync_trueosfs_json_all")]
pub unsafe extern "C" fn trueos_cabi_trueosfs_json_all(
    max_entries: u32,
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    unsafe { trueos_cabi_trueosfs_primary_json_all(max_entries, out_ptr, out_cap) }
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

struct KonsoleFrameState {
    cols: u32,
    rows: u32,
    reserved_top_rows: u32,
    terminal_handoff: bool,
    cursor_row: u32,
    cursor_col: u32,
    cursor_visible: bool,
    bytes: Vec<u8>,
}

const KONSOLE_FRAME_FLAG_TERMINAL_HANDOFF: u32 = 1 << 31;
const KONSOLE_RESERVED_TOP_ROWS_MASK: u32 = 0x0000_FFFF;

static KONSOLE_FRAME_STATES: spin::Mutex<BTreeMap<u32, KonsoleFrameState>> =
    spin::Mutex::new(BTreeMap::new());

fn konsole_write_bytes(data: &[u8]) -> usize {
    if data.is_empty() {
        return 0;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return guest_shell2_raw_write(data);
    }
    if let Some(target) = super::env::console_target() {
        return crate::shell2::raw_write_matrix_target(&target, data);
    }
    data.len()
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
fn konsole_write_fmt(args: fmt::Arguments<'_>) -> usize {
    let text = alloc::format!("{}", args);
    konsole_write_bytes(text.as_bytes())
}

fn konsole_frame_push_fmt(bytes: &mut Vec<u8>, args: fmt::Arguments<'_>) {
    let text = alloc::format!("{}", args);
    bytes.extend_from_slice(text.as_bytes());
}

fn konsole_frame_push_hex_byte(bytes: &mut Vec<u8>, byte: u8) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    bytes.push(HEX[(byte >> 4) as usize]);
    bytes.push(HEX[(byte & 0x0f) as usize]);
}

fn konsole_frame_push_row_packet(bytes: &mut Vec<u8>, row: u32, col: u32, data: &[u8]) {
    konsole_frame_push_fmt(bytes, format_args!("\x1b]777;konsole_row={},{};", row, col));
    for &byte in data {
        konsole_frame_push_hex_byte(bytes, byte);
    }
    bytes.push(0x07);
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
        return guest_shell_attached_write(data);
    }
    if let Some(target) = super::env::console_target() {
        return crate::shell2::raw_write_matrix_target(&target, data);
    }
    if SHELL_ATTACHED_REJECTS.fetch_add(1, Ordering::Relaxed) == 0 {
        crate::log!("fs-cabi: shell attached write has no route\n");
    }
    data_len
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_shell2_raw_write(
    data_ptr: *const u8,
    data_len: usize,
) -> usize {
    if data_ptr.is_null() || data_len == 0 {
        return 0;
    }
    let data = unsafe { core::slice::from_raw_parts(data_ptr, data_len) };
    konsole_write_bytes(data)
}

const SHELL2_FRONTEND_READ_HEADER_LEN: usize = 24;

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_shell2_frontend_attach_v1(cols: u32, rows: u32) -> i32 {
    if cols == 0 || rows == 0 {
        return -1;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, rc) = trueos_vm::vmcall::call(
            trueos_vm::vmcall::OP_BP_SHELL2_FRONTEND_ATTACH_V1,
            u64::from(cols),
            u64::from(rows),
        );
        return if status == trueos_vm::vmcall::STATUS_OK {
            rc as i64 as i32
        } else {
            -3
        };
    }
    let Some(vm_id) = crate::hv::current_guest_execution_context_vm_id() else {
        return -3;
    };
    crate::shell2::backends::session_pool::attach(vm_id, cols as usize, rows as usize)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_shell2_frontend_read_v1(
    read_seq: u64,
    out_ptr: *mut u8,
    out_cap: usize,
    out_next_seq: *mut u64,
    out_epoch: *mut u64,
    out_flags: *mut u32,
) -> isize {
    if (out_cap != 0 && out_ptr.is_null())
        || out_next_seq.is_null()
        || out_epoch.is_null()
        || out_flags.is_null()
    {
        return -1;
    }

    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let cap = out_cap
            .min(trueos_vm::vmcall::PAYLOAD_CAP.saturating_sub(SHELL2_FRONTEND_READ_HEADER_LEN));
        let mut response = alloc::vec![0u8; SHELL2_FRONTEND_READ_HEADER_LEN + cap];
        let (status, rc) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_SHELL2_FRONTEND_READ_V1,
            read_seq,
            cap as u64,
            &[],
            response.as_mut_slice(),
        );
        if status != trueos_vm::vmcall::STATUS_OK {
            return -3;
        }
        let rc = vmcall_signed(rc);
        if rc < 0 {
            return rc;
        }
        let len = rc as usize;
        if len > cap {
            return -3;
        }
        let next_seq = u64::from_le_bytes(response[0..8].try_into().unwrap_or_default());
        let epoch = u64::from_le_bytes(response[8..16].try_into().unwrap_or_default());
        let flags = u32::from_le_bytes(response[16..20].try_into().unwrap_or_default());
        unsafe {
            out_next_seq.write(next_seq);
            out_epoch.write(epoch);
            out_flags.write(flags);
            if len != 0 {
                core::slice::from_raw_parts_mut(out_ptr, len).copy_from_slice(
                    &response
                        [SHELL2_FRONTEND_READ_HEADER_LEN..SHELL2_FRONTEND_READ_HEADER_LEN + len],
                );
            }
        }
        return len as isize;
    }

    let Some(vm_id) = crate::hv::current_guest_execution_context_vm_id() else {
        return -3;
    };
    let out = if out_cap == 0 {
        &mut [][..]
    } else {
        unsafe { core::slice::from_raw_parts_mut(out_ptr, out_cap) }
    };
    match crate::shell2::backends::session_pool::read(vm_id, read_seq, out) {
        Ok(read) => {
            unsafe {
                out_next_seq.write(read.next_seq);
                out_epoch.write(read.epoch);
                out_flags.write(read.flags);
            }
            read.len as isize
        }
        Err(rc) => rc as isize,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_shell2_frontend_submit_input_v1(
    data_ptr: *const u8,
    data_len: usize,
) -> isize {
    if data_len == 0 {
        return 0;
    }
    if data_ptr.is_null() || data_len > trueos_vm::vmcall::PAYLOAD_CAP {
        return -1;
    }
    let data = unsafe { core::slice::from_raw_parts(data_ptr, data_len) };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, rc) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_SHELL2_FRONTEND_SUBMIT_INPUT_V1,
            0,
            0,
            data,
            &mut [],
        );
        return if status == trueos_vm::vmcall::STATUS_OK {
            vmcall_signed(rc)
        } else {
            -3
        };
    }
    let Some(vm_id) = crate::hv::current_guest_execution_context_vm_id() else {
        return -3;
    };
    crate::shell2::backends::session_pool::submit_input(vm_id, data)
        .map(|written| written as isize)
        .unwrap_or_else(|rc| rc as isize)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_shell2_frontend_detach_v1() -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, rc) =
            trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_SHELL2_FRONTEND_DETACH_V1, 0, 0);
        return if status == trueos_vm::vmcall::STATUS_OK {
            rc as i64 as i32
        } else {
            -3
        };
    }
    let Some(vm_id) = crate::hv::current_guest_execution_context_vm_id() else {
        return -3;
    };
    crate::shell2::backends::session_pool::detach(vm_id)
}

/// Spawn this Blueprint archive in a hidden child Hull.  The child receives
/// `--trueos-child-worker` in argv and `initial_*` as its first parent message.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_blueprint_child_spawn_v1(
    initial_ptr: *const u8,
    initial_len: usize,
    out_handle: *mut u64,
) -> i32 {
    if out_handle.is_null()
        || initial_len > trueos_vm::vmcall::PAYLOAD_CAP
        || (initial_len != 0 && initial_ptr.is_null())
        || crate::hv::current_hull_guest_context_vm_id().is_none()
    {
        return -1;
    }
    let initial = if initial_len == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(initial_ptr, initial_len) }
    };
    let (status, handle) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_CHILD_SPAWN_V1,
        0,
        0,
        initial,
        &mut [],
    );
    if status != trueos_vm::vmcall::STATUS_OK || (handle as i64) <= 0 {
        return -1;
    }
    unsafe { out_handle.write(handle) };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_blueprint_child_send_v1(
    handle: u64,
    data_ptr: *const u8,
    data_len: usize,
) -> isize {
    if data_len > trueos_vm::vmcall::PAYLOAD_CAP
        || (data_len != 0 && data_ptr.is_null())
        || crate::hv::current_hull_guest_context_vm_id().is_none()
    {
        return -1;
    }
    let data = if data_len == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(data_ptr, data_len) }
    };
    let (status, result) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_CHILD_SEND_V1,
        handle,
        0,
        data,
        &mut [],
    );
    if status == trueos_vm::vmcall::STATUS_OK {
        vmcall_signed(result)
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_blueprint_child_receive_v1(
    handle: u64,
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    if crate::hv::current_hull_guest_context_vm_id().is_none() {
        return -1;
    }
    let mut response = alloc::vec![0u8; trueos_vm::vmcall::PAYLOAD_CAP];
    let (status, result) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_CHILD_RECEIVE_V1,
        handle,
        0,
        &[],
        response.as_mut_slice(),
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return -1;
    }
    let length = vmcall_signed(result);
    if length <= 0 {
        return length;
    }
    let length = length as usize;
    if out_ptr.is_null() || out_cap < length || length > response.len() {
        return length as isize;
    }
    unsafe { core::ptr::copy_nonoverlapping(response.as_ptr(), out_ptr, length) };
    length as isize
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_blueprint_child_status_v1(handle: u64) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_none() {
        return -1;
    }
    let (status, result) =
        trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_CHILD_STATUS_V1, handle, 0);
    if status == trueos_vm::vmcall::STATUS_OK {
        vmcall_signed_i32(result)
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_blueprint_child_terminate_v1(handle: u64) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_none() {
        return -1;
    }
    let (status, result) =
        trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_CHILD_TERMINATE_V1, handle, 0);
    if status == trueos_vm::vmcall::STATUS_OK {
        vmcall_signed_i32(result)
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_blueprint_exit_reason(
    data_ptr: *const u8,
    data_len: usize,
) -> i32 {
    if data_ptr.is_null() || data_len == 0 {
        return -1;
    }
    let data = unsafe { core::slice::from_raw_parts(data_ptr, data_len.min(512)) };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, _) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_EXIT_REASON,
            0,
            0,
            data,
            &mut [],
        );
        return if status == trueos_vm::vmcall::STATUS_OK {
            0
        } else {
            -1
        };
    }
    let reason = core::str::from_utf8(data).unwrap_or("non-utf8-exit-reason");
    crate::log!("blueprint-exit-reason: {}\n", reason);
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_lifecycle_poll(
    out: *mut v::bp_abi::TrueosLifecyclePreparePause,
) -> i32 {
    if out.is_null() || crate::hv::current_hull_guest_context_vm_id().is_none() {
        return -1;
    }
    let mut payload = [0u8; 16];
    let (status, operation) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_LIFECYCLE_POLL,
        0,
        0,
        &[],
        &mut payload,
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return -1;
    }
    if operation == 0 {
        unsafe {
            out.write(v::bp_abi::TrueosLifecyclePreparePause::default());
        }
        return 0;
    }
    let deadline_ms = u64::from_le_bytes(payload[..8].try_into().unwrap_or([0; 8]));
    let reason = u32::from_le_bytes(payload[8..12].try_into().unwrap_or([0; 4]));
    unsafe {
        out.write(v::bp_abi::TrueosLifecyclePreparePause {
            operation,
            deadline_ms,
            reason,
            reserved: 0,
        });
    }
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_lifecycle_ready(operation: u64, checkpoint_version: u64) -> i32 {
    if operation == 0 || crate::hv::current_hull_guest_context_vm_id().is_none() {
        return -1;
    }
    // A successful call does not return until this VM instance is resumed:
    // the host snapshots at the VMCALL boundary after writing the response.
    let (status, _) = trueos_vm::vmcall::call(
        trueos_vm::vmcall::OP_BP_LIFECYCLE_READY,
        operation,
        checkpoint_version,
    );
    if status == trueos_vm::vmcall::STATUS_OK {
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_lifecycle_identity(
    out: *mut v::bp_abi::TrueosLifecycleIdentity,
) -> i32 {
    if out.is_null() || crate::hv::current_hull_guest_context_vm_id().is_none() {
        return -1;
    }
    let mut payload = [0u8; 40];
    let (status, generation) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_LIFECYCLE_IDENTITY,
        0,
        0,
        &[],
        &mut payload,
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return -1;
    }
    let mut instance = [0u8; 16];
    instance.copy_from_slice(&payload[..16]);
    let mut lineage = [0u8; 16];
    lineage.copy_from_slice(&payload[16..32]);
    let flags = u32::from_le_bytes(payload[32..36].try_into().unwrap_or([0; 4]));
    unsafe {
        out.write(v::bp_abi::TrueosLifecycleIdentity {
            instance,
            lineage,
            generation,
            flags,
            reserved: 0,
        });
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_blueprint_shutdown(
    data_ptr: *const u8,
    data_len: usize,
) -> i32 {
    let data = if data_ptr.is_null() || data_len == 0 {
        b"blueprint shutdown requested".as_slice()
    } else {
        unsafe { core::slice::from_raw_parts(data_ptr, data_len.min(512)) }
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, _) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_SHUTDOWN,
            0,
            0,
            data,
            &mut [],
        );
        return if status == trueos_vm::vmcall::STATUS_OK {
            0
        } else {
            -1
        };
    }
    let reason = core::str::from_utf8(data).unwrap_or("non-utf8-shutdown-reason");
    crate::log!("blueprint-shutdown: {}\n", reason);
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_blueprint_return_to_cli() -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, _) = trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_RETURN_TO_CLI, 0, 0);
        return if status == trueos_vm::vmcall::STATUS_OK {
            0
        } else {
            -1
        };
    }
    -1
}

const TERMINAL_LEASE_CABI_TRANSPORT_ERROR: i32 = -6;

unsafe fn blueprint_terminal_lease_v1(
    operation: u32,
    value: u64,
    out_value: *mut u64,
    pending_is_success: bool,
) -> i32 {
    if out_value.is_null() || crate::hv::current_hull_guest_context_vm_id().is_none() {
        return TERMINAL_LEASE_CABI_TRANSPORT_ERROR;
    }
    let (status, data) = trueos_vm::vmcall::call(operation, value, 0);
    if status != trueos_vm::vmcall::STATUS_OK {
        let code = i32::try_from(data).unwrap_or(-TERMINAL_LEASE_CABI_TRANSPORT_ERROR);
        return -code.max(1);
    }
    if data == 0 {
        return if pending_is_success {
            1
        } else {
            TERMINAL_LEASE_CABI_TRANSPORT_ERROR
        };
    }
    unsafe { out_value.write(data) };
    0
}

/// Observe the active terminal epoch (`ready_epoch == 0`) or acknowledge that
/// the app has restored its TUI for an already-issued epoch.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_blueprint_terminal_lease_current_v1(
    ready_epoch: u64,
    out_epoch: *mut u64,
) -> i32 {
    unsafe {
        blueprint_terminal_lease_v1(
            trueos_vm::vmcall::OP_BP_TERMINAL_LEASE_CURRENT_V1,
            ready_epoch,
            out_epoch,
            false,
        )
    }
}

/// Release one exact active epoch and return its opaque parking ticket.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_blueprint_terminal_lease_release_v1(
    expected_epoch: u64,
    out_ticket: *mut u64,
) -> i32 {
    unsafe {
        blueprint_terminal_lease_v1(
            trueos_vm::vmcall::OP_BP_TERMINAL_LEASE_RELEASE_V1,
            expected_epoch,
            out_ticket,
            false,
        )
    }
}

/// Nonblocking reentry poll. `1` means Shell2 still owns the terminal; `0`
/// returns the newly acknowledged active epoch. Negative values are errors.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_blueprint_terminal_lease_poll_reentry_v1(
    ticket: u64,
    out_epoch: *mut u64,
) -> i32 {
    unsafe {
        blueprint_terminal_lease_v1(
            trueos_vm::vmcall::OP_BP_TERMINAL_LEASE_POLL_REENTRY_V1,
            ticket,
            out_epoch,
            true,
        )
    }
}

/// Snapshot the active terminal presentation. The generation changes when the
/// underlying surface identity changes (including a same-size direct TCP
/// reconnect); columns and rows are returned from the same host snapshot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_blueprint_terminal_surface_snapshot_v1(
    out_generation: *mut u64,
    out_cols: *mut u32,
    out_rows: *mut u32,
) -> i32 {
    if out_generation.is_null()
        || out_cols.is_null()
        || out_rows.is_null()
        || crate::hv::current_hull_guest_context_vm_id().is_none()
    {
        return TERMINAL_LEASE_CABI_TRANSPORT_ERROR;
    }

    let mut record = [0u8; 16];
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_TERMINAL_SURFACE_SNAPSHOT_V1,
        0,
        0,
        &[],
        &mut record,
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        let code = i32::try_from(data).unwrap_or(-TERMINAL_LEASE_CABI_TRANSPORT_ERROR);
        return -code.max(1);
    }

    let generation = u64::from_le_bytes(record[..8].try_into().unwrap_or([0; 8]));
    let cols = u32::from_le_bytes(record[8..12].try_into().unwrap_or([0; 4]));
    let rows = u32::from_le_bytes(record[12..16].try_into().unwrap_or([0; 4]));
    if generation == 0 || cols == 0 || rows == 0 {
        return TERMINAL_LEASE_CABI_TRANSPORT_ERROR;
    }
    unsafe {
        out_generation.write(generation);
        out_cols.write(cols);
        out_rows.write(rows);
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_konsole_size(out_cols: *mut u32, out_rows: *mut u32) -> i32 {
    if out_cols.is_null() || out_rows.is_null() {
        return -1;
    }

    let (cols, rows) = if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, packed) =
            trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_SHELL_KONSOLE_SIZE, 0, 0);
        if status != trueos_vm::vmcall::STATUS_OK {
            return -1;
        }
        ((packed >> 32) as u32, packed as u32)
    } else if let Some(target) = super::env::console_target() {
        let (cols, rows) = crate::shell2::konsole_viewport_size_for_target(&target);
        (cols.min(u32::MAX as usize) as u32, rows.min(u32::MAX as usize) as u32)
    } else {
        (180, 24)
    };

    unsafe {
        *out_cols = cols.max(1);
        *out_rows = rows.max(1);
    }
    0
}

fn konsole_begin_frame_size(cols: u32, rows: u32, terminal_handoff: bool) -> Option<(u32, u32)> {
    let cols = cols.min(512).max(1);
    let rows = rows.min(512).max(1);
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let flags = if terminal_handoff { 1u64 << 63 } else { 0 };
        let (status, packed) = trueos_vm::vmcall::call(
            trueos_vm::vmcall::OP_BP_SHELL_KONSOLE_BEGIN_FRAME,
            u64::from(cols),
            u64::from(rows) | flags,
        );
        if status != trueos_vm::vmcall::STATUS_OK {
            return None;
        }
        return Some(((packed >> 32) as u32, packed as u32));
    }
    if let Some(target) = super::env::console_target() {
        let (cols, rows) = crate::shell2::konsole_begin_frame_for_target(
            &target,
            cols as usize,
            rows as usize,
            terminal_handoff,
        );
        return Some((cols.min(u32::MAX as usize) as u32, rows.min(u32::MAX as usize) as u32));
    }
    Some((cols, rows))
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_konsole_begin_frame(
    cols: u32,
    rows: u32,
    reserved_top_rows: u32,
) -> i32 {
    if cols == 0 || rows == 0 {
        return -1;
    }

    let terminal_handoff = (reserved_top_rows & KONSOLE_FRAME_FLAG_TERMINAL_HANDOFF) != 0;
    let Some((frame_cols, frame_rows)) = konsole_begin_frame_size(cols, rows, terminal_handoff)
    else {
        return -1;
    };
    let state = KonsoleFrameState {
        cols: frame_cols,
        rows: frame_rows,
        reserved_top_rows: if crate::hv::current_hull_guest_context_vm_id().is_some() {
            0
        } else {
            (reserved_top_rows & KONSOLE_RESERVED_TOP_ROWS_MASK).min(32)
        },
        terminal_handoff,
        cursor_row: 0,
        cursor_col: 0,
        cursor_visible: false,
        bytes: Vec::new(),
    };
    let key = current_cpu_key();
    let mut states = KONSOLE_FRAME_STATES.lock();
    states.insert(key, state);
    if let Some(state) = states.get_mut(&key) {
        state.bytes.extend_from_slice(b"\x1b[0m\x1b[?25l");
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_konsole_write_row(
    row: u32,
    col: u32,
    data_ptr: *const u8,
    data_len: usize,
) -> i32 {
    if data_ptr.is_null() && data_len != 0 {
        return -1;
    }
    let mut states = KONSOLE_FRAME_STATES.lock();
    let Some(state) = states.get_mut(&current_cpu_key()) else {
        return -1;
    };
    if row >= state.rows || col >= state.cols {
        return -1;
    }

    if state.terminal_handoff {
        let data = if data_len == 0 {
            &[][..]
        } else {
            unsafe { core::slice::from_raw_parts(data_ptr, data_len) }
        };
        konsole_frame_push_row_packet(&mut state.bytes, row, col, data);
        return 0;
    }

    let terminal_row = state
        .reserved_top_rows
        .saturating_add(row)
        .saturating_add(1);
    let terminal_col = col.saturating_add(1);
    konsole_frame_push_fmt(
        &mut state.bytes,
        format_args!("\x1b[{};{}H\x1b[2K", terminal_row, terminal_col),
    );
    if data_len != 0 {
        let data = unsafe { core::slice::from_raw_parts(data_ptr, data_len) };
        state.bytes.extend_from_slice(data);
    }
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_konsole_set_cursor(row: u32, col: u32, visible: u32) -> i32 {
    let mut states = KONSOLE_FRAME_STATES.lock();
    let Some(state) = states.get_mut(&current_cpu_key()) else {
        return -1;
    };
    if visible == 0 {
        state.cursor_visible = false;
        return 0;
    }
    if row >= state.rows || col >= state.cols {
        return -1;
    }

    state.cursor_row = row;
    state.cursor_col = col;
    state.cursor_visible = true;
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_konsole_end_frame() -> i32 {
    let Some(mut state) = KONSOLE_FRAME_STATES.lock().remove(&current_cpu_key()) else {
        return -1;
    };
    if state.cursor_visible {
        let terminal_row = state
            .reserved_top_rows
            .saturating_add(state.cursor_row)
            .saturating_add(1);
        let terminal_col = state.cursor_col.saturating_add(1);
        konsole_frame_push_fmt(
            &mut state.bytes,
            format_args!("\x1b[{};{}H\x1b[?25h", terminal_row, terminal_col),
        );
    } else {
        state.bytes.extend_from_slice(b"\x1b[?25l");
        if !state.terminal_handoff {
            let terminal_row = state
                .reserved_top_rows
                .saturating_add(state.rows.saturating_sub(1))
                .saturating_add(1);
            konsole_frame_push_fmt(&mut state.bytes, format_args!("\x1b[{};1H", terminal_row));
        }
    }
    let _ = konsole_write_bytes(state.bytes.as_slice());
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_shell_attached_read_byte() -> i32 {
    let mut byte = [0u8; 1];
    if read_attached_console_bytes(&mut byte) == 1 {
        i32::from(byte[0])
    } else {
        -1
    }
}

pub fn read_attached_console_bytes(out: &mut [u8]) -> usize {
    if out.is_empty() {
        return 0;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let want = core::cmp::min(out.len(), trueos_vm::vmcall::PAYLOAD_CAP);
        let (status, read) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_SHELL_ATTACHED_READ,
            want as u64,
            0,
            &[],
            &mut out[..want],
        );
        if status == trueos_vm::vmcall::STATUS_OK {
            return core::cmp::min(read as usize, want);
        }
        return 0;
    }
    if let Some(target) = super::env::console_target() {
        let mut read = 0usize;
        while read < out.len() {
            let Some(byte) = crate::shell2::read_matrix_target_byte(&target) else {
                break;
            };
            out[read] = byte;
            read += 1;
        }
        return read;
    }
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_shell_attached_readable_len() -> usize {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, data) =
            trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_SHELL_ATTACHED_READABLE_LEN, 0, 0);
        if status == trueos_vm::vmcall::STATUS_OK {
            return data as usize;
        }
        return 0;
    }
    if let Some(target) = super::env::console_target() {
        return crate::shell2::read_matrix_target_pending_len(&target);
    }
    0
}

/// Wait for attached terminal input or a typed terminal-surface change.
///
/// Hull callers park in VMX root and are resumed by the producer itself; this
/// is the event-driven primitive used by the TRUEOS terminal-only `poll(2)`
/// path. Other kernel callers retain a bounded compatibility wait.
pub(crate) fn wait_attached_console_readable(timeout_ms: u64) -> bool {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, woke) = trueos_vm::vmcall::call(
            trueos_vm::vmcall::OP_BP_SHELL_ATTACHED_WAIT_READABLE,
            timeout_ms,
            0,
        );
        return status == trueos_vm::vmcall::STATUS_OK && woke != 0;
    }
    crate::wait::spin_until_timeout(timeout_ms.max(1), || {
        trueos_cabi_shell_attached_readable_len() != 0
    })
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
